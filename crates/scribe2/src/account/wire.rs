//! host-guard の配線と doctor の 1 行（設計 docs/design/vessel-hook.md §12 行 d・ADR-0056 §2・SRS FR75 / FR73 / FR61 /
//! FR24 / NFR4）。
//!
//! 口座の口の verb `wire`（[`wire`]）は host の面の `[[account]]` の全 label の `settings.json` を `canonicalize` で**実体**に
//! 解き、実体ごとに 1 回だけ PreToolUse の hook 行 1 本を**足すだけ**の merge（[`merge`]）で書く: host-guard の項目が在れば
//! 触らず、無ければ配列の末尾に 1 要素を足し（`hooks` / `PreToolUse` が無ければ作る）、他の key・順序・値は変えない。読めない
//! 実体は断って 1 byte も書かない。書くのは同じ dir の一時 file からの rename で、symlink の側は触らない（1 度で全口座に効く）。
//! event は書かない（配線の正本は file・doctor が実測する）。
//!
//! doctor の 1 行（[`doctor_line`]）は種類ごとの on / off / no-row・配線を持つ口座の数・実体の数・binary の解決を出す（判定
//! しない・rc を変えない・C10.2）。binary は `<NAME> --version` を子 process で 1 回撃つ（PATH は OS が解く）。env も
//! `current_exe` も読まない（C2.2）。

use super::SETTINGS_FILE;
use crate::fleet::account_dir;
use crate::fleet::json_tree::{self, Tree};
use crate::hook::host_guard::Kind;
use crate::invocation::Invocation;
use crate::name::NAME;
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// 配線の matcher（跨版で固定・生成 hooks.json の PreToolUse と同じ 5 道具）。
pub const MATCHER: &str = "Bash|Edit|Write|MultiEdit|NotebookEdit";
/// 配線の timeout の秒（跨版で固定・`hook.timeout_s` から写さない＝行の値の変更で口座の設定を動かさない・ADR-0056）。
pub const TIMEOUT_S: u64 = 10;
/// 見張りの subcommand（command の 2 語目）。
const SUBCOMMAND: &str = "host-guard";
/// doctor の行の頭。
const HEAD: &str = "host-guard:";
/// doctor の欄に出す種類（欄の順・4 つとも rules 行を持つ）。見張り自身の設定の種類は行を持たず `self=on` で固定。
const ROW_KINDS: [Kind; 4] = [Kind::Git, Kind::Tmux, Kind::Ledger, Kind::Rm];
/// 設定の key（hook の event の束・PreToolUse の配列・要素の hook の配列・hook の command）。
const KEY_HOOKS: &str = "hooks";
/// PreToolUse の配列の key。
const KEY_PRE_TOOL_USE: &str = "PreToolUse";
/// hook の command の key。
const KEY_COMMAND: &str = "command";

/// 1 実体の merge の結果（閉じた enum）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merge {
    /// 行を 1 要素足した本文（末尾改行つき）。
    Added(String),
    /// host-guard の項目が在る（触らない）。
    Kept,
    /// 読めない（JSON でない・object でない・`hooks` が object でない・`PreToolUse` が配列でない）。
    Refused,
}

/// `account wire` の数（出力の 1 行の欄）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Wired {
    /// 宣言の口座の数（退役中を含む）。
    pub accounts: usize,
    /// `settings.json` の実体の数。
    pub entities: usize,
    /// 行を足した実体の数。
    pub added: usize,
    /// 在ったので触らなかった実体の数。
    pub kept: usize,
    /// 読めず・書けずに断った実体の数。
    pub refused: usize,
}

impl Wired {
    /// 出力の 1 行。
    pub fn line(&self) -> String {
        format!(
            "account: wired accounts={} entities={} added={} kept={} refused={}",
            self.accounts, self.entities, self.added, self.kept, self.refused
        )
    }
}

/// 配線の command（binary は裸の `<NAME>`＝shell の PATH が解く・`state_dir` は呼び手が絶対化した path）。
pub fn command_for(state_dir: &Path) -> String {
    format!("{NAME} {SUBCOMMAND} --state-dir {}", state_dir.display())
}

/// 設定の本文に配線を merge する（足すだけ・在れば触らない・読めない本文は断る）。
pub fn merge(text: &str, command: &str) -> Merge {
    let Ok(tree @ Tree::Object(_)) = json_tree::parse(text) else {
        return Merge::Refused;
    };
    if is_wired(&tree) {
        return Merge::Kept;
    }
    added(tree, entry(command)).map_or(Merge::Refused, |found| Merge::Added(format!("{}\n", json_tree::render(&found))))
}

/// 設定の値が host-guard の command を持つか（`hooks.PreToolUse[*].hooks[*].command` の 1 語目の basename が `<NAME>` で
/// 2 語目が host-guard・binary の path や flag の違いは問わない＝user の書き換えも「在る」）。
fn is_wired(tree: &Tree) -> bool {
    tree.get(KEY_HOOKS)
        .and_then(|hooks| hooks.get(KEY_PRE_TOOL_USE))
        .and_then(Tree::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|group| group.get(KEY_HOOKS).and_then(Tree::as_array))
        .flatten()
        .filter_map(|hook| hook.get(KEY_COMMAND).and_then(Tree::as_str))
        .any(is_guard_command)
}

/// command の先頭 2 語が `<NAME>`（basename）と host-guard か。
fn is_guard_command(command: &str) -> bool {
    let mut words = command.split_whitespace();
    let first = words.next().map(|word| word.rsplit('/').next().unwrap_or(word));
    first == Some(NAME) && words.next() == Some(SUBCOMMAND)
}

/// 足す 1 要素（matcher・type・timeout は跨版で固定）。
fn entry(command: &str) -> Tree {
    let hook = Tree::Object(vec![
        ("type".to_owned(), Tree::Str("command".to_owned())),
        (KEY_COMMAND.to_owned(), Tree::Str(command.to_owned())),
        ("timeout".to_owned(), Tree::Num(TIMEOUT_S.to_string())),
    ]);
    Tree::Object(vec![("matcher".to_owned(), Tree::Str(MATCHER.to_owned())), (KEY_HOOKS.to_owned(), Tree::Array(vec![hook]))])
}

/// `hooks` → `PreToolUse` の配列の末尾に `entry` を足した値（無ければ作る・形が違えば `None`）。
fn added(tree: Tree, entry: Tree) -> Option<Tree> {
    let Tree::Object(mut pairs) = tree else {
        return None;
    };
    let Tree::Object(events) = slot(&mut pairs, KEY_HOOKS, Tree::Object(Vec::new()))? else {
        return None;
    };
    let Tree::Array(groups) = slot(events, KEY_PRE_TOOL_USE, Tree::Array(Vec::new()))? else {
        return None;
    };
    groups.push(entry);
    Some(Tree::Object(pairs))
}

/// object の `key` の値（無ければ `empty` を末尾に足してから引く）。
fn slot<'a>(pairs: &'a mut Vec<(String, Tree)>, key: &str, empty: Tree) -> Option<&'a mut Tree> {
    if !pairs.iter().any(|(found, _)| found == key) {
        pairs.push((key.to_owned(), empty));
    }
    pairs.iter_mut().find(|(found, _)| found == key).map(|(_, value)| value)
}

/// 宣言の label（重複は畳み辞書順）ごとの `settings.json` の実体（`canonicalize`・無い口座〔退役中を含む〕は `None`）。
fn account_entities(state_dir: &Path, manifest: &Manifest) -> Vec<Option<PathBuf>> {
    let labels: BTreeSet<&str> = manifest.accounts().iter().map(|account| account.label()).collect();
    labels.into_iter().map(|label| fs::canonicalize(account_dir(state_dir, label).join(SETTINGS_FILE)).ok()).collect()
}

/// 1 実体を merge して書く（足す周だけ書く・読めない / 書けない周は 1 byte も変えず `Refused`）。
pub fn wire_entity(entity: &Path, command: &str) -> Merge {
    match fs::read_to_string(entity).map(|text| merge(&text, command)) {
        Ok(Merge::Added(text)) if replace(entity, &text).is_ok() => Merge::Added(text),
        Ok(Merge::Kept) => Merge::Kept,
        _ => Merge::Refused,
    }
}

/// 実体と同じ dir の一時 file に書いて権限を写し、実体へ rename する（落ちた周は一時 file を消す）。
fn replace(entity: &Path, text: &str) -> std::io::Result<()> {
    let mut staged = entity.as_os_str().to_owned();
    staged.push(".staged");
    let staged = PathBuf::from(staged);
    let permissions = fs::metadata(entity)?.permissions();
    let written = fs::write(&staged, text)
        .and_then(|()| fs::set_permissions(&staged, permissions))
        .and_then(|()| fs::rename(&staged, entity));
    if written.is_err() {
        let _ = fs::remove_file(&staged);
    }
    written
}

/// `account wire`: 宣言（`--rules` か埋め込み + `<state_dir>/host.toml`）の全口座の実体に配線を書く。置き場を絶対化できない・
/// 宣言を読めない周は `None`（1 つも書かない）。
pub fn wire(state_dir: &Path, rules: Option<&Path>) -> Option<Wired> {
    let root = std::path::absolute(state_dir).ok()?;
    let manifest = crate::rules::read(rules, Some(&root)).ok()?;
    let accounts = account_entities(&root, &manifest);
    let entities: BTreeSet<&PathBuf> = accounts.iter().flatten().collect();
    let command = command_for(&root);
    let mut wired = Wired { accounts: accounts.len(), entities: entities.len(), ..Wired::default() };
    for entity in entities {
        let count = match wire_entity(entity, &command) {
            Merge::Added(_) => &mut wired.added,
            Merge::Kept => &mut wired.kept,
            Merge::Refused => &mut wired.refused,
        };
        *count = count.saturating_add(1);
    }
    Some(wired)
}

// ─────────────────────────── doctor の 1 行 ───────────────────────────

/// 種類の行の読み（`on` / `off` / `no-row`・閉じた enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowState {
    /// 行が在り `enabled = true`。
    On,
    /// 行が在り `enabled = false`。
    Off,
    /// 行が無い・列でない。
    NoRow,
}

impl RowState {
    /// manifest の種類の行から読む。
    fn of(manifest: &Manifest, kind: Kind) -> Self {
        match kind.row().and_then(|id| manifest.get(id)) {
            Some(row) if !matches!(row.value, RuleValue::List(_)) => Self::NoRow,
            Some(row) if row.enabled => Self::On,
            Some(_) => Self::Off,
            None => Self::NoRow,
        }
    }

    /// 欄の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::NoRow => "no-row",
        }
    }
}

/// binary の解決（`binary=` の値・閉じた enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binary {
    /// `--version` の 1 行目が doctor 自身の行と同じ。
    Ok,
    /// 起動できない。
    Missing,
    /// 起動できたが 1 行目が違う（値は書かない）。
    Other,
}

impl Binary {
    /// 欄の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Missing => "missing",
            Self::Other => "other",
        }
    }
}

/// `bin --version` を子 process で 1 回撃ち、1 行目を `version`（doctor 自身の `--version` の行）と比べる。
pub fn resolve(bin: &Path, version: &str) -> Binary {
    let Ok(out) = Invocation::new(bin).arg("--version").output() else {
        return Binary::Missing;
    };
    if String::from_utf8_lossy(&out.stdout).lines().next() == Some(version) {
        Binary::Ok
    } else {
        Binary::Other
    }
}

/// doctor の 1 行（`--state-dir` の周・導入先の行の後ろ）: `host-guard: git= tmux= ledger= rm= self=on rows=<発効>/4
/// wired=<配線を持つ口座>/<口座> entities=<実体> [unreadable=<読めない口座>] binary=<ok|missing|other>`。宣言（`rules` = `--rules`
/// か埋め込み + host の面）を読めない周は `host-guard: rules=unreadable`。実体が割れても一覧は出さない（1 行の外形を保つ）。
pub fn doctor_line(state_dir: &Path, rules: Option<&str>, version: &str, bin: &Path) -> String {
    let Ok(manifest) = crate::rules::read(rules.map(Path::new), Some(state_dir)) else {
        return format!("{HEAD} rules=unreadable");
    };
    let states: Vec<(Kind, RowState)> = ROW_KINDS.iter().map(|kind| (*kind, RowState::of(&manifest, *kind))).collect();
    let cells: Vec<String> = states.iter().map(|(kind, state)| format!("{}={}", kind.as_str(), state.as_str())).collect();
    let on = states.iter().filter(|(_, state)| *state == RowState::On).count();
    let accounts = account_entities(state_dir, &manifest);
    let (mut wired, mut unreadable) = (0_usize, 0_usize);
    for entity in accounts.iter().flatten() {
        match fs::read_to_string(entity).ok().and_then(|text| json_tree::parse(&text).ok()) {
            Some(tree @ Tree::Object(_)) if is_wired(&tree) => wired = wired.saturating_add(1),
            Some(Tree::Object(_)) => {}
            _ => unreadable = unreadable.saturating_add(1),
        }
    }
    let entities: BTreeSet<&PathBuf> = accounts.iter().flatten().collect();
    let unreadable = if unreadable > 0 { format!(" unreadable={unreadable}") } else { String::new() };
    format!(
        "{HEAD} {} self=on rows={on}/{} wired={wired}/{} entities={}{unreadable} binary={}",
        cells.join(" "),
        ROW_KINDS.len(),
        accounts.len(),
        entities.len(),
        resolve(bin, version).as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::{command_for, entry, merge, resolve, wire_entity, Binary, Merge, MATCHER, TIMEOUT_S};
    use crate::fleet::json_tree::{parse, Tree};
    use crate::rules::manifest::Manifest;
    use crate::rules::RuleValue;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// 歯ごとの空の tmp dir。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("host-guard-wire-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        dir
    }

    /// 配線の command（歯の置き場は固定の絶対 path）。
    fn command() -> String {
        command_for(Path::new("/s"))
    }

    /// `Added` の本文を読む（末尾改行つきで読める周だけ・それ以外は `null`＝比べる側の assert が落ちる）。
    fn added_tree(merged: Merge) -> Tree {
        match merged {
            Merge::Added(text) if text.ends_with("}\n") => parse(&text).unwrap_or(Tree::Null),
            Merge::Added(_) | Merge::Kept | Merge::Refused => Tree::Null,
        }
    }

    /// `path` の key を辿った値。
    fn at<'a>(tree: &'a Tree, path: &[&str]) -> Option<&'a Tree> {
        path.iter().try_fold(tree, |node, key| node.get(key))
    }

    /// 既存の要素を持つ `PreToolUse` の末尾に 1 要素（固定の matcher・command・timeout）を足し、他の key・順序・値は不変。
    #[test]
    fn host_guard_wire_adds_one_element_to_an_existing_pre_tool_use_and_keeps_the_rest() {
        let text = r#"{"model": "x", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "stop.sh"}]}], "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "other-guard", "timeout": 5}]}], "PostToolUse": []}, "n": 1.50, "env": {"A": "b"}}"#;
        let before = parse(text).unwrap_or_else(|error| panic!("fixture は読める: {error}"));
        let mut after = added_tree(merge(text, &command()));
        let groups = at(&after, &["hooks", "PreToolUse"]).and_then(Tree::as_array).unwrap_or_default().to_vec();
        assert_eq!(groups.len(), 2, "1 要素だけ増える: {groups:?}");
        assert_eq!(groups.last(), Some(&entry(&command())), "末尾に足す");
        let last = groups.last().cloned().unwrap_or(Tree::Null);
        assert_eq!(at(&last, &["matcher"]).and_then(Tree::as_str), Some(MATCHER));
        let hook = last.get("hooks").and_then(Tree::as_array).and_then(<[Tree]>::first).cloned().unwrap_or(Tree::Null);
        assert_eq!(hook.get("command").and_then(Tree::as_str), Some("scribe2 host-guard --state-dir /s"));
        assert_eq!(hook.get("timeout"), Some(&Tree::Num("10".to_owned())));
        assert_eq!(hook.get("type").and_then(Tree::as_str), Some("command"));
        if let Tree::Object(pairs) = &mut after {
            if let Some((_, Tree::Object(events))) = pairs.iter_mut().find(|(key, _)| key == "hooks") {
                if let Some((_, Tree::Array(items))) = events.iter_mut().find(|(key, _)| key == "PreToolUse") {
                    items.pop();
                }
            }
        }
        assert_eq!(after, before, "足した 1 要素を除けば key・順序・値（数の字面 1.50 も）が不変");
    }

    /// host-guard の項目が在る実体は byte 単位で不変（binary を絶対 path に書き換えた行・flag の違う行も「在る」）。basename が
    /// 違う・2 語目が違う command は「無い」として足す。
    #[test]
    fn host_guard_wire_keeps_the_bytes_when_a_host_guard_item_exists() {
        let dir = scratch("kept");
        let entity = dir.join("settings.json");
        for present in ["scribe2 host-guard --state-dir /elsewhere", "/opt/bin/scribe2 host-guard --rules r.toml --state-dir /s"] {
            let body = format!("{{\"hooks\":{{\"PreToolUse\":[{{\"matcher\":\"Bash\",\"hooks\":[{{\"command\":\"{present}\"}}]}}]}},\"k\":1}}");
            assert!(fs::write(&entity, &body).is_ok(), "fixture を書ける");
            assert_eq!(wire_entity(&entity, &command()), Merge::Kept, "{present}");
            assert_eq!(fs::read(&entity).unwrap_or_default(), body.as_bytes(), "{present}: byte 単位で不変");
            assert_eq!(wire_entity(&entity, &command()), Merge::Kept, "{present}: 冪等");
        }
        for absent in ["scribe2x host-guard --state-dir /s", "scribe2 hook pre-tool-use", "host-guard scribe2"] {
            let body = format!("{{\"hooks\":{{\"PreToolUse\":[{{\"hooks\":[{{\"command\":\"{absent}\"}}]}}]}}}}");
            assert!(matches!(merge(&body, &command()), Merge::Added(_)), "{absent} は host-guard の項目ではない");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// `hooks` の無い設定には `hooks` と `PreToolUse` を末尾に作り、`hooks` だけ在る設定には `PreToolUse` を作る（既存の key の
    /// 後ろ）。timeout の固定値 10 は埋め込みの `hook.timeout_s` の値と同じ（行を動かす便はこの配線を動かさない＝2 面）。
    #[test]
    fn host_guard_wire_creates_hooks_and_pre_tool_use_when_absent() {
        let bare = added_tree(merge("{\"disableAgentView\": true}\n", &command()));
        let Tree::Object(pairs) = &bare else { panic!("object: {bare:?}") };
        let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, ["disableAgentView", "hooks"], "既存の key の後ろに hooks");
        assert_eq!(at(&bare, &["hooks", "PreToolUse"]), Some(&Tree::Array(vec![entry(&command())])));
        let stop = added_tree(merge("{\"hooks\": {\"Stop\": []}}", &command()));
        let Some(Tree::Object(events)) = stop.get("hooks") else { panic!("hooks は object: {stop:?}") };
        let names: Vec<&str> = events.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(names, ["Stop", "PreToolUse"], "hooks の既存の event の後ろに作る");
        let embedded = Manifest::embedded().unwrap_or_else(|_| panic!("埋め込みの manifest を読める"));
        let row = embedded.get("hook.timeout_s").map(|found| found.value.clone());
        assert_eq!(row, Some(RuleValue::Int(TIMEOUT_S)), "配線の timeout は埋め込みの hook.timeout_s の値と同じ");
    }

    /// 読めない本文（JSON でない・root が配列・`hooks` が object でない・`PreToolUse` が配列でない）は断って 1 byte も書かず、
    /// 一時 file も残さない。
    #[test]
    fn host_guard_wire_refuses_an_unreadable_body_without_writing() {
        let dir = scratch("refused");
        let entity = dir.join("settings.json");
        for body in ["{", "[]", "{\"hooks\": []}", "{\"hooks\": {\"PreToolUse\": {}}}", "\"s\""] {
            assert!(fs::write(&entity, body).is_ok(), "fixture を書ける");
            assert_eq!(wire_entity(&entity, &command()), Merge::Refused, "{body}");
            assert_eq!(fs::read(&entity).unwrap_or_default(), body.as_bytes(), "{body}: 1 byte も書かない");
            assert!(fs::symlink_metadata(dir.join("settings.json.staged")).is_err(), "{body}: 一時 file を残さない");
        }
        assert_eq!(wire_entity(&dir.join("absent.json"), &command()), Merge::Refused, "読めない file");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 版の照合は起動の記述を通る（設計 core-boundary.md §9 行 h）: program は bin・引数は `--version` の 1 つ。stdout の
    /// 1 行目が doctor 自身の行と同じなら `Ok`・違えば（rc に依らず）`Other`・起動の失敗は `Missing`。
    #[test]
    fn invocation_hook_wire_version_reads_ok_other_and_missing() {
        use crate::pipe::fixture::{exited, Call, Stub};
        let stub = Stub::install(|call| match call.program.as_str() {
            "/bin/same" => exited(0, b"scribe2 1.0\nextra\n"),
            "/bin/other" => exited(0, b"scribe2 0.9\nscribe2 1.0\n"),
            "/bin/failed" => exited(1, b"scribe2 1.0\n"),
            _ => Err(std::io::Error::other("gone")),
        });
        let version = "scribe2 1.0";
        assert_eq!(resolve(Path::new("/bin/same"), version), Binary::Ok, "1 行目が一致");
        assert_eq!(resolve(Path::new("/bin/other"), version), Binary::Other, "1 行目が不一致（2 行目は見ない）");
        assert_eq!(resolve(Path::new("/bin/failed"), version), Binary::Ok, "rc は見ない");
        assert_eq!(resolve(Path::new("/bin/gone"), version), Binary::Missing, "起動の失敗");
        let call = |program: &str| Call {
            program: program.to_owned(),
            args: vec!["--version".to_owned()],
            cwd: None,
            envs: Vec::new(),
        };
        let expected = [call("/bin/same"), call("/bin/other"), call("/bin/failed"), call("/bin/gone")];
        assert_eq!(stub.calls(), expected, "bin の program と引数");
    }
}
