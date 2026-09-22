//! doctor の導入先の行（設計 consumer-sync.md §4・ADR-0028 §2.3・SRS FR61 / AC31・C3.2）。
//!
//! 母集団 = 席の登録 row の anchor（`source=launch`・読み込み元は §3 の記録）∪ 有効な口座（account-lifecycle.md §3 の
//! [`effective_accounts`]）の帳簿 `<account_dir>/plugins/installed_plugins.json` の `plugins["<NAME>@<NAME>"][*].projectPath`
//! （`source=install`）。同じ path は 1 行に畳む（`launch+install`）・path の辞書順。帳簿と cache は**読むだけ**
//! （Claude Code の所有・器は書かない・C3）。**判定しない**（行を出すだけ・C10.2）: 食い違いは [`Drift`] の語が名指し、
//! 記録の無い導入先は `unrecorded`（`none` に潰さない）。値は全部実測か `unknown` / `unrecorded` / `undeclared`。

use crate::fleet::json_tree::{self, Tree};
use crate::fleet::{account_dir, effective_accounts, replay, store, State};
use crate::hook::vessel::digest::{self, PluginRecord};
use crate::name::{NAME, PLUGIN_DIR};
use crate::rules::manifest::Manifest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 帳簿の dir（口座の設定 dir の直下）。
const LEDGER_DIR: &str = "plugins";
/// 帳簿の file 名。
const LEDGER_FILE: &str = "installed_plugins.json";
/// 帳簿の key（plugin 名 → 導入先の列）。
const KEY_PLUGINS: &str = "plugins";
/// 導入先 1 つの key（project の path）。
const KEY_PROJECT: &str = "projectPath";
/// 導入先 1 つの key（scope）。
const KEY_SCOPE: &str = "scope";
/// 導入先 1 つの key（cache の場所）。
const KEY_INSTALL: &str = "installPath";
/// 導入先 1 つの key（install 時の commit）。
const KEY_SHA: &str = "gitCommitSha";
/// 記録が無い周の字面。
const UNRECORDED: &str = "unrecorded";
/// 食い違い 0 語の字面。
const NONE: &str = "none";
/// 値の無い欄の字面。
const DASH: &str = "-";
/// 短縮 sha の桁数（`head=` の表示・build 元 commit と同じ幅）。
const SHA_LEN: usize = 12;

/// 導入の出所（closed・3 値・設計 §4「導入の形」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// 器が起こした席の anchor（登録 row）。
    Launch,
    /// 口座の帳簿にだけ在る（user が手で起こす session）。
    Install,
    /// 両方に在る。
    Both,
}

impl Source {
    /// 行の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::Install => "install",
            Self::Both => "launch+install",
        }
    }
}

/// 食い違いの語（closed・宣言順 = 出力順・空 = `none`・設計 §8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drift {
    /// 記録の build 元 commit ≠ doctor 自身の build 元 commit。
    Binary,
    /// 記録の digest ≠ 記録の root に今在る hooks.json の digest。
    Plugin,
    /// 帳簿の `gitCommitSha` ≠ vessel repo の HEAD（`source=install` を含む行だけ）。
    Ledger,
    /// 記録の root が checkout の生成 dir（`<repo>/<PLUGIN_DIR>`・consumer-sync.md §17 形 5）で、かつ同じ path の帳簿にも器が
    /// 在る（hook が二重）。
    Dual,
    /// 記録が無い・読めない（`none` に潰さない）。
    Unrecorded,
}

/// [`Drift`] の全 variant（宣言順）。
pub const DRIFTS: &[Drift] = &[Drift::Binary, Drift::Plugin, Drift::Ledger, Drift::Dual, Drift::Unrecorded];

impl Drift {
    /// 行の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Plugin => "plugin",
            Self::Ledger => "ledger",
            Self::Dual => "dual",
            Self::Unrecorded => UNRECORDED,
        }
    }
}

/// vessel repo の HEAD（`head=` の値・closed 3 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// `[[vessel]]` が無い（止めない）。
    Undeclared,
    /// 宣言は在るが git が HEAD を返さない。
    Unknown,
    /// HEAD の sha（全桁・表示は先頭 12 桁）。
    Sha(String),
}

impl Head {
    /// 行の字面。
    pub fn render(&self) -> String {
        match self {
            Self::Undeclared => "undeclared".to_owned(),
            Self::Unknown => "unknown".to_owned(),
            Self::Sha(sha) => sha.chars().take(SHA_LEN).collect(),
        }
    }
}

/// 帳簿の導入先 1 つ（読んだ値だけ・無い key は `None`）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    project: String,
    scope: Option<String>,
    install: Option<String>,
    sha: Option<String>,
}

/// 口座 1 つの帳簿の読み（closed）。
enum Ledger {
    /// file が無い（器を入れていない口座・飛ばす）。
    Absent,
    /// 在るが読めない・形が違う（1 行で名指す）。
    Unreadable,
    /// 読めた（器の導入先の列・器が無ければ空）。
    Entries(Vec<Entry>),
}

/// 導入先 1 つの実測（行の材料・pure な描画 [`render_consumer`] の入力）。
pub struct Consumer {
    /// 導入の出所。
    pub source: Source,
    /// 帳簿の scope（帳簿に無ければ `None` = `-`）。
    pub scope: Option<String>,
    /// §3 の記録（登録 row の席の打刻 dir から・複数の席は最新の ts）。
    pub record: PluginRecord,
    /// 帳簿の `gitCommitSha`（`source=install` を含む行だけ・無ければ `-`）。
    pub ledger: Option<String>,
    /// 帳簿の `installPath` に今在る hooks.json の digest（無い・読めない周は `absent`）。
    pub cache: Option<String>,
}

/// 行を組む前の材料（登録 row の target の列と帳簿の導入先）。
#[derive(Default)]
struct Draft {
    targets: Vec<String>,
    entry: Option<Entry>,
}

impl Draft {
    /// 実測へ写す（記録と cache の digest はここで読む）。
    fn measure(&self, state_dir: &Path) -> Consumer {
        let source = match (self.targets.is_empty(), &self.entry) {
            (false, None) => Source::Launch,
            (false, Some(_)) => Source::Both,
            (true, _) => Source::Install,
        };
        Consumer {
            source,
            scope: self.entry.as_ref().and_then(|entry| entry.scope.clone()),
            record: record_of(state_dir, &self.targets),
            ledger: self.entry.as_ref().and_then(|entry| entry.sha.clone()),
            cache: self.entry.as_ref().and_then(|entry| entry.install.as_deref()).and_then(|dir| digest::hooks_digest(Path::new(dir))),
        }
    }
}

/// 帳簿の path（`<state_dir>/accounts/<label>/plugins/installed_plugins.json`）。
pub fn ledger_path(state_dir: &Path, label: &str) -> PathBuf {
    account_dir(state_dir, label).join(LEDGER_DIR).join(LEDGER_FILE)
}

/// 帳簿を読む（読むだけ）。**無い**（NotFound）だけが `Absent`・JSON でない・`plugins` が無い・列の要素に
/// `projectPath` が無い周は `Unreadable`（黙って落とさない・NFR4）・器の key が無い周は空の列。
fn read_ledger(path: &Path) -> Ledger {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ledger::Absent,
        Err(_) => return Ledger::Unreadable,
    };
    let Some(plugins) = json_tree::parse(&text).ok().and_then(|tree| tree.get(KEY_PLUGINS).cloned()) else {
        return Ledger::Unreadable;
    };
    let Some(rows) = plugins.get(&format!("{NAME}@{NAME}")) else {
        return Ledger::Entries(Vec::new());
    };
    let Some(rows) = rows.as_array() else {
        return Ledger::Unreadable;
    };
    let mut entries = Vec::new();
    for row in rows {
        let text_of = |key: &str| row.get(key).and_then(Tree::as_str).map(str::to_owned);
        let Some(project) = text_of(KEY_PROJECT) else {
            return Ledger::Unreadable;
        };
        entries.push(Entry { project, scope: text_of(KEY_SCOPE), install: text_of(KEY_INSTALL), sha: text_of(KEY_SHA) });
    }
    Ledger::Entries(entries)
}

/// 記録の ts（`Recorded` だけ）。
fn ts_of(record: &PluginRecord) -> Option<u64> {
    match record {
        PluginRecord::Recorded { ts, .. } => Some(*ts),
        PluginRecord::Absent | PluginRecord::Unreadable => None,
    }
}

/// 同じ anchor の席の記録のうち最新（ts が最大）。読めた記録が無ければ、読めない記録が 1 つでも在れば `Unreadable`・
/// どれも無ければ `Absent`。
fn record_of(state_dir: &Path, targets: &[String]) -> PluginRecord {
    let mut found = PluginRecord::Absent;
    for target in targets {
        let read = PluginRecord::read(&crate::seat::seat_dir(state_dir, target));
        match (ts_of(&read), ts_of(&found)) {
            (Some(newer), Some(older)) if newer > older => found = read,
            (Some(_), None) => found = read,
            (None, None) if read == PluginRecord::Unreadable => found = read,
            _ => {}
        }
    }
    found
}

/// `[[vessel]] repo` の HEAD（git を 1 回撃つ・宣言が無ければ `Undeclared`・返らなければ `Unknown`）。
pub fn head_of(vessel: Option<&Path>) -> Head {
    let Some(dir) = vessel else {
        return Head::Undeclared;
    };
    let out = Command::new("git").arg("-C").arg(dir).args(["rev-parse", "HEAD"]).output().ok();
    match out.filter(|found| found.status.success()).map(|found| String::from_utf8_lossy(&found.stdout).trim().to_owned()) {
        Some(sha) if !sha.is_empty() => Head::Sha(sha),
        _ => Head::Unknown,
    }
}

/// 2 つの dir が同じ場所か（実体 path で比べ、解けない周は字面）。
fn same_dir(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => left == right,
    }
}

/// 食い違いの語 1 つが立つか（判定の正本・語ごとの定義は [`Drift`] の doc）。
fn holds(word: Drift, consumer: &Consumer, head: &Head, vessel: Option<&Path>) -> bool {
    let recorded = match &consumer.record {
        PluginRecord::Recorded { root, hooks, binary, .. } => Some((root, hooks, binary)),
        PluginRecord::Absent | PluginRecord::Unreadable => None,
    };
    match word {
        Drift::Binary => recorded.is_some_and(|(_, _, binary)| binary != env!("SCRIBE2_BUILD_COMMIT")),
        Drift::Plugin => recorded.is_some_and(|(root, hooks, _)| *hooks != digest::hooks_digest(Path::new(root))),
        Drift::Ledger => matches!((head, &consumer.ledger), (Head::Sha(sha), Some(ledger)) if sha != ledger),
        Drift::Dual => {
            consumer.source != Source::Launch
                && recorded.zip(vessel).is_some_and(|((root, _, _), repo)| same_dir(Path::new(root), &repo.join(PLUGIN_DIR)))
        }
        Drift::Unrecorded => recorded.is_none(),
    }
}

/// 導入先 1 つの食い違いの語（宣言順・空 = `none`）。
pub fn drift_of(consumer: &Consumer, head: &Head, vessel: Option<&Path>) -> Vec<Drift> {
    DRIFTS.iter().copied().filter(|word| holds(*word, consumer, head, vessel)).collect()
}

/// 導入先 1 行（pure）。
pub fn render_consumer(path: &str, consumer: &Consumer, head: &Head, drift: &[Drift]) -> String {
    let (binary, plugin) = match &consumer.record {
        PluginRecord::Recorded { root, hooks, binary, .. } => {
            (binary.clone(), format!("{root}:{}", hooks.as_deref().unwrap_or(digest::UNREADABLE)))
        }
        PluginRecord::Absent | PluginRecord::Unreadable => (UNRECORDED.to_owned(), UNRECORDED.to_owned()),
    };
    let words: Vec<&str> = drift.iter().map(|word| word.as_str()).collect();
    let drift = if words.is_empty() { NONE.to_owned() } else { words.join("+") };
    format!(
        "consumer={path} source={} scope={} binary={binary} plugin={plugin} ledger={} cache={} head={} drift={drift}",
        consumer.source.as_str(),
        consumer.scope.as_deref().unwrap_or(DASH),
        consumer.ledger.as_deref().unwrap_or(DASH),
        consumer.cache.as_deref().unwrap_or("absent"),
        head.render()
    )
}

/// 壊れた帳簿の 1 行（口座の帳簿を名指し・他の欄は測れない側の字面）。
fn render_broken(path: &Path, head: &Head) -> String {
    format!(
        "consumer={} source={} scope={DASH} binary={UNRECORDED} plugin={UNRECORDED} ledger={} cache=absent head={} drift={UNRECORDED}",
        path.display(),
        Source::Install.as_str(),
        digest::UNREADABLE,
        head.render()
    )
}

/// 母集団を集める（登録 row の anchor → 帳簿の導入先の順・同じ path は畳む）。壊れた帳簿の口座は別に返す。
fn gather(state_dir: &Path, manifest: &Manifest, state: Option<&State>) -> (BTreeMap<String, Draft>, Vec<PathBuf>) {
    let mut drafts: BTreeMap<String, Draft> = BTreeMap::new();
    let mut broken = Vec::new();
    for latest in state.map(|found| found.registrations.values()).into_iter().flatten() {
        let row = &latest.registration;
        drafts.entry(row.anchor.clone()).or_default().targets.push(row.target.clone());
    }
    let labels = state.map_or_else(
        || manifest.accounts().iter().map(|account| account.label().to_owned()).collect(),
        |found| effective_accounts(manifest, found),
    );
    for label in labels {
        let path = ledger_path(state_dir, &label);
        match read_ledger(&path) {
            Ledger::Absent => {}
            Ledger::Unreadable => broken.push(path),
            Ledger::Entries(entries) => {
                for entry in entries {
                    let project = entry.project.clone();
                    drafts.entry(project).or_default().entry = Some(entry);
                }
            }
        }
    }
    (drafts, broken)
}

/// doctor の導入先の項目（口座の行の後ろ・導入先ごとに 1 行・path の辞書順・壊れた帳簿は末尾に 1 行ずつ）。宣言
/// （`rules` = `--rules FILE` か埋め込み + `<state_dir>/host.toml`）を読めない周は 0 行（口座の項目が 1 行で名指す）。
/// event log を読めない周は登録 row の側を持たず、口座は宣言の全件。判定しない（rc を変えず行を出すだけ）。
pub fn doctor_lines(state_dir: &Path, rules: Option<&str>) -> Vec<String> {
    let Ok(manifest) = crate::rules::read(rules.map(Path::new), Some(state_dir)) else {
        return Vec::new();
    };
    let state = store::read_all(state_dir).ok().map(|events| replay(&events));
    let vessel = manifest.vessel().map(|found| PathBuf::from(found.repo()));
    let head = head_of(vessel.as_deref());
    let (drafts, broken) = gather(state_dir, &manifest, state.as_ref());
    let mut lines: Vec<String> = drafts
        .iter()
        .map(|(path, draft)| {
            let consumer = draft.measure(state_dir);
            render_consumer(path, &consumer, &head, &drift_of(&consumer, &head, vessel.as_deref()))
        })
        .collect();
    lines.extend(broken.iter().map(|path| render_broken(path, &head)));
    lines
}

#[cfg(test)]
mod tests {
    use super::{drift_of, head_of, read_ledger, record_of, render_consumer, same_dir, Consumer, Drift, Head, Ledger, Source, DRIFTS};
    use crate::hook::vessel::digest::{self, PluginRecord};
    use crate::order::is_declaration_order;
    use crate::seat::seat_dir;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// 歯ごとの空の tmp dir（in-file の歯の置き場・env を読まないのは器の本体の規律〔C2.2〕）。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("account-consumers-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        dir
    }

    /// 席 `target` の記録 file に `sid` / `ts` の読める記録を書く（形は `digest.rs` の書き手 [`PluginRecord::to_line`]）。
    fn record(state_dir: &Path, target: &str, sid: &str, ts: u64) -> PluginRecord {
        let found = PluginRecord::Recorded {
            root: "/r".to_owned(),
            hooks: Some("0".repeat(16)),
            binary: "b".repeat(12),
            sid: sid.to_owned(),
            ts,
        };
        let seat = seat_dir(state_dir, target);
        let _ = fs::create_dir_all(&seat);
        let _ = fs::write(digest::record_path(&seat), format!("{}\n", found.to_line().unwrap_or_default()));
        found
    }

    /// 席 `target` の記録 file を読めない形（2 行）にする。
    fn unreadable(state_dir: &Path, target: &str) {
        let seat = seat_dir(state_dir, target);
        let _ = fs::create_dir_all(&seat);
        let _ = fs::write(digest::record_path(&seat), "schema=1 sid=s root=/r hooks=x binary=b ts=1\nextra\n");
    }

    /// `targets` の字面を列に。
    fn targets(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// git を 1 回撃つ（失敗は読み手の assert が落とす）。
    fn git(dir: &Path, args: &[&str]) {
        let _ = Command::new("git").arg("-C").arg(dir).args(args).output();
    }

    // flip-check: retroactive s2-07l.338
    /// ts の異なる 2 本は、targets のどちらの順でも ts の大きい方（`>` を `<` / `==` に替えると先に見た方が残る）。
    #[test]
    fn consumers_record_of_picks_the_newest_regardless_of_target_order() {
        let state = scratch("newest");
        let _ = record(&state, "old", "sid-old", 10);
        let newest = record(&state, "new", "sid-new", 20);
        assert_eq!(record_of(&state, &targets(&["old", "new"])), newest, "古→新の順");
        assert_eq!(record_of(&state, &targets(&["new", "old"])), newest, "新→古の順");
    }

    /// ts が同じ 2 本は先に見つけた方が残る（`>` を `>=` に替えると後の方に置き換わる）。
    #[test]
    fn consumers_record_of_keeps_the_first_on_equal_ts() {
        let state = scratch("equal");
        let first = record(&state, "one", "sid-one", 5);
        let second = record(&state, "two", "sid-two", 5);
        assert_ne!(first, second, "sid で区別できる");
        assert_eq!(record_of(&state, &targets(&["one", "two"])), first, "one が先");
        assert_eq!(record_of(&state, &targets(&["two", "one"])), second, "two が先");
    }

    /// 読めた記録 > 読めない記録 > 不在（順序に依らない・全部無ければ `Absent`・読めないが 1 本でも在れば `Unreadable`）。
    #[test]
    fn consumers_record_of_prefers_readable_over_unreadable_and_unreadable_over_absent() {
        let state = scratch("prefers");
        unreadable(&state, "broken");
        let readable = record(&state, "fine", "sid-fine", 3);
        assert_eq!(record_of(&state, &targets(&[])), PluginRecord::Absent, "targets 0 本");
        assert_eq!(record_of(&state, &targets(&["none-a", "none-b"])), PluginRecord::Absent, "全部無い");
        assert_eq!(record_of(&state, &targets(&["broken", "none-a"])), PluginRecord::Unreadable, "読めない→無い");
        assert_eq!(record_of(&state, &targets(&["none-a", "broken"])), PluginRecord::Unreadable, "無い→読めない");
        assert_eq!(record_of(&state, &targets(&["broken", "fine"])), readable, "読めない→読めた");
        assert_eq!(record_of(&state, &targets(&["fine", "broken"])), readable, "読めた→読めない");
        assert_eq!(record_of(&state, &targets(&["none-a", "fine", "broken"])), readable, "無い→読めた→読めない");
    }

    /// 帳簿は NotFound だけが `Absent`・他の失敗（dir を渡す）と形違いは `Unreadable`・読めた周は導入先の列。
    #[test]
    fn consumers_read_ledger_tells_not_found_from_unreadable() {
        let dir = scratch("ledger");
        assert!(matches!(read_ledger(&dir.join("missing.json")), Ledger::Absent), "無い file");
        assert!(matches!(read_ledger(&dir), Ledger::Unreadable), "dir は NotFound でない失敗");
        let broken = dir.join("broken.json");
        let _ = fs::write(&broken, "{\"other\": {}}\n");
        assert!(matches!(read_ledger(&broken), Ledger::Unreadable), "plugins key が無い");
        let empty = dir.join("empty.json");
        let _ = fs::write(&empty, "{\"plugins\": {}}\n");
        assert!(matches!(read_ledger(&empty), Ledger::Entries(entries) if entries.is_empty()), "器の key が無い");
        let filled = dir.join("filled.json");
        let name = crate::name::NAME;
        let _ = fs::write(&filled, format!("{{\"plugins\": {{\"{name}@{name}\": [{{\"projectPath\": \"/p\", \"scope\": \"project\"}}]}}}}\n"));
        let Ledger::Entries(entries) = read_ledger(&filled) else {
            panic!("読める帳簿");
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].project, "/p");
        assert_eq!(entries[0].scope.as_deref(), Some("project"));
        assert_eq!((entries[0].install.as_deref(), entries[0].sha.as_deref()), (None, None), "無い key は None");
    }

    /// symlink とその実体は同じ・別々に実在する 2 dir は違う・解けない周は字面で比べる。
    #[test]
    fn consumers_same_dir_compares_real_paths_and_falls_back_to_literal() {
        let dir = scratch("same-dir");
        let (real, other) = (dir.join("real"), dir.join("other"));
        let _ = fs::create_dir_all(&real);
        let _ = fs::create_dir_all(&other);
        let link = dir.join("link");
        assert!(std::os::unix::fs::symlink(&real, &link).is_ok(), "symlink を作れる");
        assert!(same_dir(&link, &real), "symlink と実体");
        assert!(same_dir(&real, &link), "実体と symlink");
        assert!(same_dir(&real, &real), "同じ実体");
        assert!(!same_dir(&real, &other), "別々に実在する 2 dir");
        assert!(!same_dir(&link, &other), "symlink と別の dir");
        let (gone_a, gone_b) = (dir.join("gone"), dir.join("gone"));
        assert!(same_dir(&gone_a, &gone_b), "存在しない同じ字面");
        assert!(!same_dir(&gone_a, &dir.join("gone-b")), "存在しない別の字面");
        assert!(!same_dir(&gone_a, &real), "存在しない path と実在する dir");
    }

    /// commit の無い repo（init だけ）は `Unknown`・宣言が無ければ `Undeclared`・commit が在れば全桁の sha。
    #[test]
    fn consumers_head_of_is_unknown_without_a_commit() {
        let repo = scratch("head-of");
        git(&repo, &["init", "-q", "-b", "main"]);
        assert_eq!(head_of(Some(&repo)), Head::Unknown, "commit が無い");
        assert_eq!(head_of(None), Head::Undeclared, "宣言が無い");
        assert_eq!(head_of(Some(&repo.join("missing"))), Head::Unknown, "repo が無い");
        git(&repo, &["config", "user.name", "consumer"]);
        git(&repo, &["config", "user.email", "consumer@example.invalid"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "seed"]);
        let Head::Sha(sha) = head_of(Some(&repo)) else {
            panic!("commit の後は sha");
        };
        assert_eq!(sha.len(), 40, "全桁: {sha}");
        assert!(sha.chars().all(|ch| ch.is_ascii_hexdigit()), "hex: {sha}");
    }

    /// 語は 5 つで宣言順に閉じる（variant を足した周はここの件数が変わる）。
    #[test]
    fn doctor_consumer_drift_words_are_closed_in_declaration_order() {
        let words: Vec<&str> = DRIFTS.iter().map(|word| word.as_str()).collect();
        assert_eq!(words, ["binary", "plugin", "ledger", "dual", "unrecorded"]);
        assert!(is_declaration_order(DRIFTS, |word| word as usize), "DRIFTS は宣言順: {DRIFTS:?}");
        assert_eq!(Source::Both.as_str(), "launch+install");
    }

    /// 記録の無い導入先は `unrecorded`（`none` に潰さない）で、帳簿の食い違いは記録が無くても測る（宣言順に `+` で繋ぐ）。
    #[test]
    fn doctor_consumer_unrecorded_is_not_folded_into_none_and_ledger_is_still_measured() {
        let consumer = Consumer {
            source: Source::Install,
            scope: Some("project".to_owned()),
            record: PluginRecord::Absent,
            ledger: Some("a".repeat(40)),
            cache: None,
        };
        let head = Head::Sha("b".repeat(40));
        assert_eq!(drift_of(&consumer, &head, None), [Drift::Ledger, Drift::Unrecorded]);
        let line = render_consumer("/c", &consumer, &head, &drift_of(&consumer, &head, None));
        assert_eq!(
            line,
            format!("consumer=/c source=install scope=project binary=unrecorded plugin=unrecorded ledger={} cache=absent head=bbbbbbbbbbbb drift=ledger+unrecorded", "a".repeat(40))
        );
        assert_eq!(drift_of(&consumer, &Head::Undeclared, None), [Drift::Unrecorded], "head が無ければ帳簿は測れない");
        assert_eq!(render_consumer("/c", &consumer, &Head::Undeclared, &[]).rsplit(' ').next(), Some("drift=none"), "空は none");
    }
}
