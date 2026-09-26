//! 口座の口（設計 docs/design/account-lifecycle.md §3・ADR-0026 §2.2・SRS FR58 / FR33 / FR36）: 登録（[`add`]）・
//! 一覧（[`ls_lines`]）・退役（[`retire`]）・戻し（[`restore`]）。
//!
//! doctor の口座行（[`doctor_lines`]・`s2-07l.233`）と `account ls` の行は**同じ 1 関数**（[`render_account`]）で作る
//! （一覧を 2 面に書かない・C10.2）。**env も HOME も読まない**（C2.2）: 置き場は `--state-dir` だけで、口座の dir を
//! **走査しない**（宣言が真実・C3）。退役は可逆な move と event 1 件で、削除の口を持たない（N1 / N1.2）。前提違反は file も
//! event も書かずに typed に断る（[`AccountError`]・C11.3）。credential には触れない（読まない・書かない・login を待たない）。
//! doctor の導入先の行（口座の行の後ろ・consumer-sync.md §4）は [`consumers`]。host-guard の配線の verb `wire` と doctor の
//! host-guard の 1 行（vessel-hook.md §12）は [`wire`]。

pub mod cli;
pub mod consumers;
pub mod wire;

use crate::fleet::json_tree::{self, Tree};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{
    account_dir, cli as fleet_cli, effective_accounts, replay, Allowance, AllowanceLatest, Event, EventKind, State,
    WindowKind, SCHEMA,
};
use crate::headless::{ACCOUNT_ENV, DEFAULT_CLAUDE};
use crate::invocation::Invocation;
use crate::rules::manifest::{AccountGroup, HostManifest, Manifest, TickUnit};
use crate::seat::{self, cycle, InputGate, REASON_TMUX_FAILED};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// 口座の設定 dir の直下に書く settings の file 名（器が起こす席の前提・account-autonomy.md §5 (4)）。
const SETTINGS_FILE: &str = "settings.json";
/// `settings.json` の本文（agent view を切る 1 項目だけ）。
const SETTINGS_BODY: &str = "{\"disableAgentView\": true}\n";
/// 退役先の dir 名（`<state_dir>/accounts/.retired/`・label の規則が `.` 始まりを取らない＝口座の dir と衝突しない）。
const RETIRED_DIR: &str = ".retired";
/// 宣言を読めない周の 1 行（0 行に潰さない・C11）。
const MANIFEST_UNREADABLE: &str = "accounts: manifest=unreadable";
/// 実測行が無い・その窓が測れていない周の残量の字面。
const UNMEASURED: &str = "unmeasured";
/// host の面が無い周に `[[account]]` 行を足す土台（`schema = 1` から作る）。
const HOST_HEAD: &str = "schema = 1\n";

/// 口座の口の断り（設計 §7・**閉じた enum**・前提違反は file も event も書かない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountError {
    /// 宣言済み（退役中を含む）の label を足そうとした。
    Exists,
    /// `<state_dir>/accounts/<label>` が既に在る（dir か link・中身は読まない）。
    DirExists,
    /// 宣言に無い label を退役・戻そうとした。
    Unknown,
    /// 既に退役中。
    AlreadyRetired,
    /// 退役中でない label を戻そうとした。
    NotRetired,
    /// 登録 row のどれかがその label を持つ（席が使っている口座を外さない）。
    InUse,
    /// label が規則（[`label_ok`]）に合わない。
    LabelInvalid,
    /// 置き場を読めない・書けない（宣言・event log・dir・跨 device の move）。
    WriteFailed,
}

/// [`AccountError`] の全 variant（宣言順）。
pub const ERRORS: &[AccountError] = &[
    AccountError::Exists,
    AccountError::DirExists,
    AccountError::Unknown,
    AccountError::AlreadyRetired,
    AccountError::NotRetired,
    AccountError::InUse,
    AccountError::LabelInvalid,
    AccountError::WriteFailed,
];

impl AccountError {
    /// 断りの行の `reason=` の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exists => "exists",
            Self::DirExists => "dir-exists",
            Self::Unknown => "unknown",
            Self::AlreadyRetired => "already-retired",
            Self::NotRetired => "not-retired",
            Self::InUse => "in-use",
            Self::LabelInvalid => "label-invalid",
            Self::WriteFailed => "write-failed",
        }
    }
}

/// label の規則: manifest の label の規則（空でない・引用符 1 組の文字列）に加え、`<state_dir>/accounts/` の 1 成分として閉じ、
/// login の起動行を shell が割らない字だけ——空でなく、`.` で始まらず（`.retired` と `.` / `..` を取らない）、ASCII の英数字と
/// `-` `_` `.` `@` `+` だけ。
pub fn label_ok(label: &str) -> bool {
    !label.is_empty()
        && !label.starts_with('.')
        && label.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '@' | '+'))
}

// ─────────────────────────── 一覧（doctor の口座行・`account ls`） ───────────────────────────

/// 口座の dir・credential・config の在る / 無い（`dir=` / `credential=` / `config=` の値・閉じた enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// 在る。
    Present,
    /// 無い。
    Missing,
}

impl Presence {
    /// 真偽から写す。
    fn of(found: bool) -> Self {
        if found {
            Self::Present
        } else {
            Self::Missing
        }
    }

    /// 行の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Missing => "missing",
        }
    }
}

/// agent view の読み（`agentview=` の値・account-autonomy.md §5「agent view の前提 (4)」・閉じた enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentView {
    /// `settings.json` の `disableAgentView` が `true`。
    Off,
    /// key が無い / `false`。
    On,
    /// file が無い・読めない・形が違う（`on` に潰さない・C11）。
    Unreadable,
}

impl AgentView {
    /// 真偽の key の読み（[`flag_at`]）から写す。
    fn of(read: Result<bool, ()>) -> Self {
        match read {
            Ok(true) => Self::Off,
            Ok(false) => Self::On,
            Err(()) => Self::Unreadable,
        }
    }

    /// 行の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Unreadable => "unreadable",
        }
    }
}

/// 「設定 dir × anchor」の trust の読み（`trust=` の値・account-autonomy.md §5「trust の前提」・閉じた enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// `.claude.json` の `projects[<anchor>].hasTrustDialogAccepted` が `true`。
    Accepted,
    /// key が無い / `false`。
    Missing,
    /// file が無い・読めない・形が違う・event log を読めない（`missing` に潰さない・C11）。
    Unreadable,
    /// 登録 row が 0 件（突き合わせる anchor が無い）。
    NotApplicable,
}

impl Trust {
    /// 真偽の key の読み（[`flag_at`]）から写す。
    fn of(read: Result<bool, ()>) -> Self {
        match read {
            Ok(true) => Self::Accepted,
            Ok(false) => Self::Missing,
            Err(()) => Self::Unreadable,
        }
    }

    /// 行の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Missing => "missing",
            Self::Unreadable => "unreadable",
            Self::NotApplicable => "n/a",
        }
    }
}

/// 退役の読み（`retired=` の値・有効な口座の集合〔[`effective_accounts`]〕の外か・閉じた enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retired {
    /// 退役中（宣言は在るが有効な口座の集合に無い）。
    Yes,
    /// 有効。
    No,
    /// event log を読めない（`no` に潰さない・C11）。
    Unreadable,
}

impl Retired {
    /// 有効な口座の集合（log を読めない周は `None`）から写す。
    fn of(effective: Option<&BTreeSet<String>>, label: &str) -> Self {
        match effective {
            None => Self::Unreadable,
            Some(found) if found.contains(label) => Self::No,
            Some(_) => Self::Yes,
        }
    }

    /// 行の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Unreadable => "unreadable",
        }
    }
}

/// 口座 1 つの前提の読み（`<state_dir>/accounts/<label>` の直下を読んだ結果・何も書かない）。
pub struct AccountProbe {
    /// dir か（link を辿って dir）。
    pub dir: Presence,
    /// 直下の `.credentials.json` が file か（中身は読まない）。
    pub credential: Presence,
    /// 直下の `settings.json` が file か（Claude Code の設定 dir の印）。
    pub config: Presence,
    /// 直下の `settings.json` の `disableAgentView`。
    pub agentview: AgentView,
    /// 登録 row の anchor ごとの trust（anchor の辞書順・event log を読めない周は `None`）。
    pub trust: Option<Vec<(String, Trust)>>,
}

/// JSON file を入れ子の reader で読む。file が無い・読めない・JSON でない周は `None`。
fn read_tree(path: &Path) -> Option<Tree> {
    json_tree::parse(&fs::read_to_string(path).ok()?).ok()
}

/// `path` の key を辿った真偽。途中か末端の key が無い周は `Ok(false)`・読めない file（`None`）と object で
/// ない途中・真偽でない末端は `Err`（形が違う）。
fn flag_at(tree: Option<&Tree>, path: &[&str]) -> Result<bool, ()> {
    let mut node = tree.ok_or(())?;
    for key in path {
        let Tree::Object(_) = node else { return Err(()) };
        match node.get(key) {
            Some(next) => node = next,
            None => return Ok(false),
        }
    }
    node.as_bool().ok_or(())
}

/// 口座の dir を読む（読むだけ・`.claude.json` / credential / settings に書かない）。
fn probe_account(dir: &Path, anchors: Option<&BTreeSet<String>>) -> AccountProbe {
    let is_file = |name: &str| fs::metadata(dir.join(name)).is_ok_and(|found| found.is_file());
    let claude = read_tree(&dir.join(".claude.json"));
    let trust_of = |anchor: &String| {
        let read = flag_at(claude.as_ref(), &["projects", anchor, "hasTrustDialogAccepted"]);
        (anchor.clone(), Trust::of(read))
    };
    AccountProbe {
        dir: Presence::of(fs::metadata(dir).is_ok_and(|found| found.is_dir())),
        credential: Presence::of(is_file(".credentials.json")),
        config: Presence::of(is_file(SETTINGS_FILE)),
        agentview: AgentView::of(flag_at(read_tree(&dir.join(SETTINGS_FILE)).as_ref(), &["disableAgentView"])),
        trust: anchors.map(|found| found.iter().map(trust_of).collect()),
    }
}

// ─────────────────────────── trust の書き手（席の起動の 1 本だけが撃つ） ───────────────────────────

/// 口座の設定 dir の直下の Claude Code の設定 file（trust の印の置き場・読みは [`probe_account`]）。
const CLAUDE_FILE: &str = ".claude.json";

/// 起動の前に trust の印を置いた結果（設計 host-init.md §7 形 3・**閉じた列**・起動の行の末尾の `trust=` の語）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustWrite {
    /// 印を置いて書き戻した。
    Written,
    /// file が無く、`projects` だけの最小の file を作って置いた。
    Created,
    /// 既に `true`（1 byte も書かない）。
    Accepted,
    /// file は在るが読めない・JSON でない・途中が object でない・末端が真偽でない（書かない）。
    Unreadable,
    /// 一時 file の書き・読み直しの不一致・rename のどれかが落ちた（置き換えない）。
    Unwritable,
}

impl TrustWrite {
    /// 行の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Written => "written",
            Self::Created => "created",
            Self::Accepted => "accepted",
            Self::Unreadable => "unreadable",
            Self::Unwritable => "unwritable",
        }
    }
}

/// 口座の dir `dir` の `.claude.json` の `projects[<anchor>].hasTrustDialogAccepted` を `true` に置く（設計 host-init.md §7 形 2・
/// ADR-0065）: 同じ読み手で読み → 木に印を置き（[`json_tree::set_bool`]）→ `render` の本文（元の file に末尾の改行が在れば保つ）を
/// 同じ dir の一時 file に書き → 読み直して印が `true` かを確かめ → rename で置き換える。file が無い周は空の木から作る（末尾の改行
/// あり）。既に `true` の周は書かない。lock は持たない（同じ file を書く Claude Code とは共有しない・§7 形 6）。
pub fn accept_trust(dir: &Path, anchor: &str) -> TrustWrite {
    let path = dir.join(CLAUDE_FILE);
    let (mut tree, newline, created) = match fs::read_to_string(&path) {
        Ok(text) => match json_tree::parse(&text) {
            Ok(tree) => (tree, text.ends_with('\n'), false),
            Err(_) => return TrustWrite::Unreadable,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Tree::Object(Vec::new()), true, true),
        Err(_) => return TrustWrite::Unreadable,
    };
    let key = ["projects", anchor, "hasTrustDialogAccepted"];
    match json_tree::set_bool(&mut tree, &key, true) {
        Ok(true) => {}
        Ok(false) => return TrustWrite::Accepted,
        Err(_) => return TrustWrite::Unreadable,
    }
    let staged = dir.join(format!("{CLAUDE_FILE}.{}.staged", std::process::id()));
    let body = format!("{}{}", json_tree::render(&tree), if newline { "\n" } else { "" });
    let mode = fs::metadata(&path).map(|found| found.permissions());
    let written = fs::write(&staged, body).and_then(|()| mode.map_or(Ok(()), |found| fs::set_permissions(&staged, found)));
    if written.is_ok() && flag_at(read_tree(&staged).as_ref(), &key) == Ok(true) && fs::rename(&staged, &path).is_ok() {
        return if created { TrustWrite::Created } else { TrustWrite::Written };
    }
    let _ = fs::remove_file(&staged);
    TrustWrite::Unwritable
}

/// 口座 1 行（pure・doctor と `account ls` の同じ 1 関数）。trust は anchor が 1 つなら `trust=<値>`・複数なら anchor ごとに
/// `trust=<潰した anchor>:<値>` を並べ（潰し方は席の dir 名と同じ [`seat::sanitize_target`]）、末尾に `retired=<値>`。
pub fn render_account(label: &str, probe: &AccountProbe, retired: Retired) -> String {
    let head = format!(
        "account={label} dir={} credential={} config={} agentview={}",
        probe.dir.as_str(),
        probe.credential.as_str(),
        probe.config.as_str(),
        probe.agentview.as_str()
    );
    let cells: Vec<String> = match probe.trust.as_deref() {
        None => vec![Trust::Unreadable.as_str().to_owned()],
        Some([]) => vec![Trust::NotApplicable.as_str().to_owned()],
        Some([(_, only)]) => vec![only.as_str().to_owned()],
        Some(many) => many
            .iter()
            .map(|(anchor, found)| format!("{}:{}", seat::sanitize_target(anchor), found.as_str()))
            .collect(),
    };
    let line = cells.iter().fold(head, |line, cell| format!("{line} trust={cell}"));
    format!("{line} retired={}", retired.as_str())
}

/// doctor の host の面の 1 行（`host-manifest=<present|absent|unreadable>`・account-lifecycle.md §2・読むだけ）。面に `[[tick]]` が
/// 在る周（`tick`）だけ末尾に `tick=declared` を 1 項目足す（値は書かない・表の無い host の行は 1 字も変わらない・設計
/// seat-heartbeat.md §5 形 3）。
pub fn render_host_manifest(word: &str, tick: Option<&TickUnit>) -> String {
    let declared = if tick.is_some() { " tick=declared" } else { "" };
    format!("host-manifest={word}{declared}")
}

/// 群の行の「1 つも無い」を表す語（席の登録 row がどの置き場にも無い周・`0` や空に潰さない）。
const GROUP_NONE: &str = "none";

/// 置き場の event log を読めない周の群の行の席の口座（`none` に潰さない・C11）。
const GROUP_UNREADABLE: &str = "unreadable";

/// doctor の群の 1 行（設計 account-lifecycle.md §17 の約束 7・**読むだけで判定しない**）。
///
/// 出すのは宣言値 3 つ（名・候補の label の列・置き場の数）と導出値 2 つ（その群の置き場を anchor に持つ席の登録 row の
/// 口座 label・重複は畳み辞書順・1 つも無ければ [`GROUP_NONE`]・`state` が `None`〔log を読めない〕周は [`GROUP_UNREADABLE`]
/// ／群の今の口座 `current=`＝[`render_current`]・§20 形 2 ／群の予約 `next=`＝[`render_next`]・§29 形 5）。記録は 1 件も書かない
/// （読むだけ）。
fn render_group(state_dir: &Path, manifest: &Manifest, group: &AccountGroup, state: Option<&State>) -> String {
    let seats = match state {
        None => GROUP_UNREADABLE.to_owned(),
        Some(found) => {
            let labels = seat_accounts(group, found);
            if labels.is_empty() {
                GROUP_NONE.to_owned()
            } else {
                labels.into_iter().collect::<Vec<&str>>().join(",")
            }
        }
    };
    format!(
        "group={} accounts={} anchors={} seat-accounts={seats} current={} next={} refused={}",
        group.name(),
        group.accounts().join(","),
        group.anchors().len(),
        render_current(state_dir, group),
        render_next(state_dir, manifest, group),
        render_refused(state_dir, group)
    )
}

/// 群の行の `refused=` の値（断りの印・account-lifecycle.md §31 形 3）: 印の ts・無ければ `-`・在るのに形でない・読めなければ
/// [`GROUP_UNREADABLE`]。
fn render_refused(state_dir: &Path, group: &AccountGroup) -> String {
    use crate::hook::group::{refused_path, refused_ts};
    match std::fs::read_to_string(refused_path(&crate::seat::host_groups_dir(state_dir), group.name())) {
        Ok(text) => refused_ts(&text).unwrap_or(GROUP_UNREADABLE).to_owned(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => "-".to_owned(),
        Err(_) => GROUP_UNREADABLE.to_owned(),
    }
}

/// 群の行の `next=` の値（群の予約の 1 関数 [`crate::hook::group::reserve`] を**測らずに**呼ぶ＝鮮度の外の候補は門を通らない）:
/// 予約の label・無ければ [`GROUP_NONE`]・記録か event log を読めなければ [`GROUP_UNREADABLE`]・閾値 / 鮮度 / 役割の model の
/// rules 行が無ければ [`NEXT_NO_RULE`]（どれにも潰さない・C10）。
fn render_next(state_dir: &Path, manifest: &Manifest, group: &AccountGroup) -> String {
    use crate::hook::group::{current_of, currents_of, reserve, Caps, Judge, Unreserved};
    if current_of(state_dir, group).is_err() {
        return GROUP_UNREADABLE.to_owned();
    }
    let Ok(caps) = Caps::of(manifest) else {
        return NEXT_NO_RULE.to_owned();
    };
    let currents = currents_of(state_dir, manifest);
    let judge =
        Judge { state_dir, manifest, group, head: &currents, taken: &currents, forced: &BTreeSet::new(), caps, measure: &|_, _| {} };
    match reserve(&judge, &mut BTreeSet::new()) {
        Ok(found) => found.unwrap_or_else(|| GROUP_NONE.to_owned()),
        Err(Unreserved::NoRule) => NEXT_NO_RULE.to_owned(),
        Err(Unreserved::Unreadable) => GROUP_UNREADABLE.to_owned(),
    }
}

/// 群の行の `next=` が rules 行の無さで導けないことを表す語。
const NEXT_NO_RULE: &str = "no-rule";

/// 群の行の `current=` の値（解決の 1 関数 [`crate::hook::group::current_of`] の読み）: 記録が在ればその label・無ければ
/// [`GROUP_SEED`]（種＝候補の先頭は `accounts=` の先頭に在る）・在るのに読めなければ [`GROUP_UNREADABLE`]（種に潰さない・C11）。
fn render_current(state_dir: &Path, group: &AccountGroup) -> String {
    match crate::hook::group::current_of(state_dir, group) {
        Ok(found) if found.source == crate::hook::group::Source::Record => found.label,
        Ok(_) => GROUP_SEED.to_owned(),
        Err(_) => GROUP_UNREADABLE.to_owned(),
    }
}

/// 群の行の `current=` が種（記録が無い）であることを表す語。
const GROUP_SEED: &str = "seed";

/// 群の置き場を anchor に持つ席の登録 row の口座 label（重複は畳み辞書順・**導きの 1 本**＝doctor の群の行と dispatch の
/// 1 周の群の段〔account-lifecycle.md §19 形 2〕が同じ集合を読む・C2）。
pub fn seat_accounts<'a>(group: &AccountGroup, state: &'a State) -> BTreeSet<&'a str> {
    state
        .registrations
        .values()
        .filter(|latest| group.anchors().contains(&latest.registration.anchor))
        .map(|latest| latest.registration.account.as_str())
        .collect()
}

/// event log の replay（読めない周は `None`）。
fn read_state(state_dir: &Path) -> Option<State> {
    store::read_all(state_dir).ok().map(|events| replay(&events))
}

/// 宣言の label の辞書順に (label, 口座行)（doctor と `account ls` の同じ 1 本）。trust は登録 row の anchor ごと・
/// `retired=` は有効な口座の集合（[`effective_accounts`]）の外か。`state` は event log の replay（読めない周は `None`）。
fn rows(state_dir: &Path, manifest: &Manifest, state: Option<&State>) -> Vec<(String, String)> {
    let labels: BTreeSet<&str> = manifest.accounts().iter().map(|account| account.label()).collect();
    let anchors: Option<BTreeSet<String>> =
        state.map(|found| found.registrations.values().map(|latest| latest.registration.anchor.clone()).collect());
    let effective: Option<BTreeSet<String>> = state.map(|found| effective_accounts(manifest, found).into_iter().collect());
    labels
        .into_iter()
        .map(|label| {
            let probe = probe_account(&account_dir(state_dir, label), anchors.as_ref());
            (label.to_owned(), render_account(label, &probe, Retired::of(effective.as_ref(), label)))
        })
        .collect()
}

/// doctor の口座の項目（C3.2 の「口座」と「退役」の面・account-autonomy.md §5）: 先頭に host の面の 1 行、続けて宣言
/// （`rules` = `--rules FILE` か埋め込みの tracked の面 + `<state_dir>/host.toml`・env を読まない）の `[[account]]` の label の
/// 辞書順に 1 行（[`rows`]）、最後に `[[account-group]]` の**宣言順**に 1 行（[`render_group`]・account-lifecycle.md §17）。
/// 判定しない（rc を変えず行を出すだけ）。宣言を読めない周は 1 行 `accounts: manifest=unreadable`
/// （0 行に潰さない・C11）。host の面が壊れている周も報告は止めない。
pub fn doctor_lines(state_dir: &Path, rules: Option<&str>) -> Vec<String> {
    let host = HostManifest::read(&crate::rules::host_manifest_path(state_dir));
    let word = host.as_str();
    let present = matches!(host, HostManifest::Present(_));
    let declared = rules.map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path))).map(|tracked| tracked.joined(host));
    // tracked の面が読めて合わせで落ちた周は host の面の欠陥（面をまたぐ重複を含む）＝file 単体が読めても unreadable。
    let word = if matches!(declared, Ok(Err(_))) { HostManifest::Unreadable(Vec::new()).as_str() } else { word };
    let tick = declared.as_ref().ok().and_then(|joined| joined.as_ref().ok()).and_then(Manifest::tick);
    let head = render_host_manifest(word, tick);
    let Ok(Ok(manifest)) = declared else {
        return vec![head, MANIFEST_UNREADABLE.to_owned()];
    };
    let state = read_state(state_dir);
    // 面が present の周だけ末尾に便用の口座の数（account-lifecycle.md §26 形 1・absent の行は 1 字も変わらない）。
    let head = if present { format!("{head} run-accounts={}", run_accounts(state_dir, &manifest, state.as_ref())) } else { head };
    let mut lines = vec![head];
    lines.extend(rows(state_dir, &manifest, state.as_ref()).into_iter().map(|(_, line)| line));
    // 群の行は口座の行の後ろに**宣言順**で（設計 account-lifecycle.md §17 の約束 7）。群を 1 つも宣言しない host は
    // 0 本＝既存の外形は 1 行も動かない（約束 8）。
    lines.extend(manifest.groups().iter().map(|group| render_group(state_dir, &manifest, group, state.as_ref())));
    lines
}

/// doctor の host の行の `run-accounts=` の値（設計 account-lifecycle.md §26 形 1・**判定しない**）: 有効な口座
/// （[`effective_accounts`]）のうち群の今の口座（[`crate::rules::grouped_accounts`]・便用の除外と同じ 1 本）に無いものの数。
/// 席の登録 row の除外（repo ごと）と計測の鮮度は読まない。log か群の記録を読めない周は [`GROUP_UNREADABLE`]（0 に潰さない・C11）。
fn run_accounts(state_dir: &Path, manifest: &Manifest, state: Option<&State>) -> String {
    let (Some(found), Ok(grouped)) = (state, crate::rules::grouped_accounts(state_dir)) else {
        return GROUP_UNREADABLE.to_owned();
    };
    effective_accounts(manifest, found).iter().filter(|label| !grouped.contains(*label)).count().to_string()
}

/// `account ls` の行（設計 §3）: doctor の口座行（[`rows`]・`retired=` 込み）に最新の実測行の要約（[`summary`]・event log を
/// 読むだけ・計測は撃たない）を足して label の辞書順に出す。宣言は埋め込み + `<state_dir>/host.toml`。判定・rc を持たない。
pub fn ls_lines(state_dir: &Path) -> Vec<String> {
    let Ok(manifest) = crate::rules::read(None, Some(state_dir)) else {
        return vec![MANIFEST_UNREADABLE.to_owned()];
    };
    let state = read_state(state_dir);
    rows(state_dir, &manifest, state.as_ref())
        .into_iter()
        .map(|(label, line)| format!("{line} {}", summary(state.as_ref(), &label)))
        .collect()
}

/// 口座の残量の要約（`five_hour=<pct%|unmeasured> seven_day=<pct%|unmeasured>`）: その口座の最新の回（ts が最大の行の
/// 集まり・`fleet usage --show` と同じ読み）で窓が実測されていれば使用率、無い・Unmeasured・log を読めない周は `unmeasured`
/// （0 に読み替えない・FR33）。
pub fn summary(state: Option<&State>, label: &str) -> String {
    let mine: Vec<&AllowanceLatest> = state
        .map(|found| found.allowance.iter().filter(|(key, _)| key.account == label).map(|(_, latest)| latest).collect())
        .unwrap_or_default();
    let newest = mine.iter().map(|latest| latest.ts.as_str()).max();
    let pct = |window: WindowKind| {
        mine.iter()
            .filter(|latest| Some(latest.ts.as_str()) == newest)
            .find_map(|latest| match &latest.allowance {
                Allowance::Measured(found) if found.window == window => Some(format!("{}%", found.used_pct)),
                Allowance::Measured(_) | Allowance::Unmeasured(_) => None,
            })
            .unwrap_or_else(|| UNMEASURED.to_owned())
    };
    format!("five_hour={} seven_day={}", pct(WindowKind::FiveHour), pct(WindowKind::SevenDay))
}

// ─────────────────────────── 登録（`account add`） ───────────────────────────

/// `account add` の入力。
pub struct Add<'a> {
    /// 置き場（`--state-dir`）。
    pub state_dir: &'a Path,
    /// 足す口座の label。
    pub label: &'a str,
    /// login の cwd（`--anchor`・trust の dialog を同じ session で通すため・無ければ置き場）。
    pub anchor: Option<&'a Path>,
    /// login の起動行を注入する tmux target（`--target`・無ければ stdout に出す）。
    pub target: Option<&'a str>,
    /// tmux の socket（`--tmux-socket`）。
    pub socket: Option<&'a str>,
}

/// login の起動行の届け方（dir・settings・宣言の行が揃った後の手・bool で持たない）。
pub enum Delivery {
    /// `--target` 無し: 行を stdout に出した（user が打つ）。
    Printed,
    /// target の shell へ注入した。
    Sent,
    /// 門を通らず **1 key も送っていない**（理由は注入の門と同じ語）。
    Refused(&'static str),
}

/// `account add` の成立。
pub struct Prepared {
    /// login の起動行（[`login_line`]）。
    pub next: String,
    /// 届け方。
    pub delivery: Delivery,
}

/// 口座を足す（設計 §3・順序固定）: label の規則（`label-invalid`）→ 宣言済み（退役中を含む・`exists`）→ dir の不在
/// （`dir-exists`）→ host の面の本文を組んで一時 file に書き検査（[`stage_host`]）→ dir・`settings.json`・rename
/// （[`create`]）→ login の起動行を `--target` の shell へ注入するか stdout に出す。credential に触れない・login を待たない。
pub fn add(request: &Add) -> Result<Prepared, AccountError> {
    let root = std::path::absolute(request.state_dir).map_err(|_| AccountError::WriteFailed)?;
    let label = request.label;
    if !label_ok(label) {
        return Err(AccountError::LabelInvalid);
    }
    let declared = crate::rules::read(None, Some(&root)).map_err(|_| AccountError::WriteFailed)?;
    if declared.accounts().iter().any(|account| account.label() == label) {
        return Err(AccountError::Exists);
    }
    let dir = account_dir(&root, label);
    if fs::symlink_metadata(&dir).is_ok() {
        return Err(AccountError::DirExists);
    }
    let staged = stage_host(&root, label)?;
    if let Err(error) = create(&dir, &staged, &crate::rules::host_manifest_path(&root)) {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }
    let cwd = request.anchor.map_or_else(|| root.clone(), |found| std::path::absolute(found).unwrap_or_else(|_| found.to_path_buf()));
    let next = login_line(&cwd, &dir);
    let delivery = match request.target {
        None => Delivery::Printed,
        Some(target) => deliver(request.socket, target, &next).map_or_else(Delivery::Refused, |()| Delivery::Sent),
    };
    Ok(Prepared { next, delivery })
}

/// host の面に `[[account]]` 行を 1 つ足した本文を同じ dir の一時 file（`host.toml.staged`）に書き、契約 (a) の loader で
/// 検査する（読み → 検査 → 一時 file・rename は [`create`]）。file が無ければ `schema = 1` から作る。検査に落ちた周は一時
/// file を残さず `write-failed`。
fn stage_host(root: &Path, label: &str) -> Result<PathBuf, AccountError> {
    let host = crate::rules::host_manifest_path(root);
    let text = match fs::read_to_string(&host) {
        Ok(found) => found,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => HOST_HEAD.to_owned(),
        Err(_) => return Err(AccountError::WriteFailed),
    };
    let separator = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
    let staged = root.join(format!("{}.staged", crate::rules::HOST_MANIFEST));
    let written = fs::create_dir_all(root).and_then(|()| fs::write(&staged, format!("{text}{separator}\n[[account]]\nlabel = \"{label}\"\n")));
    if written.is_ok() && staged_fits(&staged, label) {
        return Ok(staged);
    }
    let _ = fs::remove_file(&staged);
    Err(AccountError::WriteFailed)
}

/// 一時 file が host の面として読め（未知 key・重複・schema・面をまたぐ重複を契約 (a) の loader で）、`label` を宣言するか。
fn staged_fits(staged: &Path, label: &str) -> bool {
    face_fits(staged, |face| face.accounts().iter().any(|account| account.label() == label))
}

/// file が host の面として読め、埋め込みの面と合わせられ、`also` を満たすか（`account add` と `init` の検査の 1 本）。
pub fn face_fits(path: &Path, also: impl Fn(&Manifest) -> bool) -> bool {
    let HostManifest::Present(face) = HostManifest::read(path) else {
        return false;
    };
    also(&face) && Manifest::embedded().is_ok_and(|tracked| tracked.joined(HostManifest::Present(face)).is_ok())
}

/// `init` の 3 段目の 1 口座分（host-init.md §4 の 3・bool にしない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// 新しい置き場に既に在る（dir か link・中身は読まない）。
    Present,
    /// 雛形の `accounts/<label>` が symlink でも dir でもない。
    NoSource,
    /// 結べる（結ぶ先の絶対 path）。
    To(PathBuf),
}

/// 雛形の `accounts/<label>` が symlink ならその先、実 dir ならその dir を、新しい置き場の結ぶ先として引く（**読むのは
/// link と種別だけ**・credential は読まず写さない・FR58）。
pub fn link_source(template: &Path, state_dir: &Path, label: &str) -> Link {
    if fs::symlink_metadata(account_dir(state_dir, label)).is_ok() {
        return Link::Present;
    }
    let source = account_dir(template, label);
    match fs::symlink_metadata(&source) {
        Ok(meta) if meta.file_type().is_symlink() => fs::read_link(&source)
            .map_or(Link::NoSource, |target| Link::To(source.parent().map_or_else(|| target.clone(), |dir| dir.join(&target)))),
        Ok(meta) if meta.is_dir() => Link::To(source),
        _ => Link::NoSource,
    }
}

/// 新しい置き場の `accounts/<label>` を `target` への symlink にする（親の dir は作る）。
pub fn link_account(state_dir: &Path, label: &str, target: &Path) -> std::io::Result<()> {
    let dir = account_dir(state_dir, label);
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent)?;
    }
    std::os::unix::fs::symlink(target, dir)
}

/// 口座の dir を作り、直下に `settings.json` を書き、一時 file を host の面へ rename する（この順・rename は同じ dir の中で
/// 原子的＝宣言の行は部分書きにならない）。途中で落ちた周は**この呼出しが作った物だけ**を戻す（user の既存物には触れない・
/// 既存の dir は [`add`] が先に断る）。
fn create(dir: &Path, staged: &Path, host: &Path) -> Result<(), AccountError> {
    let failed = |_: std::io::Error| AccountError::WriteFailed;
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent).map_err(failed)?;
    }
    fs::create_dir(dir).map_err(failed)?;
    let settings = dir.join(SETTINGS_FILE);
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&settings)
        .and_then(|mut file| file.write_all(SETTINGS_BODY.as_bytes()));
    if written.is_err() || fs::rename(staged, host).is_err() {
        let _ = fs::remove_file(&settings);
        let _ = fs::remove_dir(dir);
        return Err(AccountError::WriteFailed);
    }
    Ok(())
}

/// login の起動行（`cd <cwd> && CLAUDE_CONFIG_DIR=<dir> claude`）。`claude` は語（shell の PATH が解く・器は claude の
/// 場所を持たない）。値の中の空白は解釈しない（[`cycle::derive_launch`] と同じ）。
pub fn login_line(cwd: &Path, dir: &Path) -> String {
    format!("cd {} && {ACCOUNT_ENV}={} {DEFAULT_CLAUDE}", cwd.display(), dir.display())
}

/// 起動行を target の shell へ注入する（契約 (b) と同じ門・account-lifecycle.md §5: 前面 process が shell
/// 〔[`seat::pane_is_shell`]〕∧ 可視域の最後の非空行が shell の prompt 末尾〔[`seat::shell_input_empty`]〕）。門を通らない周は
/// **1 key も送らない**（断りの語は `seat launch` と同じ）。
fn deliver(socket: Option<&str>, target: &str, line: &str) -> Result<(), &'static str> {
    if !seat::pane_is_shell(socket, target) {
        return Err(cycle::REASON_NOT_SHELL);
    }
    let pane = capture_joined(socket, target).ok_or(cycle::REASON_PANE_MISSING)?;
    seat::shell_input_empty(&pane).map_err(|gate| match gate {
        InputGate::Busy => cycle::REASON_INPUT_BUSY,
        InputGate::UnknownInput => cycle::REASON_INPUT_UNKNOWN,
    })?;
    let sent = seat::tmux_ok(socket, &["send-keys", "-t", target, "-l", line])
        && seat::tmux_ok(socket, &["send-keys", "-t", target, "Enter"]);
    sent.then_some(()).ok_or(REASON_TMUX_FAILED)
}

/// pane 本文を `-J`（行末の空白を保つ）で読む（shell の門は prompt 末尾の空白まで見る・`seat launch` の門と同じ読み）。
/// 撃てない・rc 非 0 は `None`。
fn capture_joined(socket: Option<&str>, target: &str) -> Option<String> {
    let mut command = Invocation::new("tmux");
    if let Some(path) = socket {
        command.arg("-S").arg(path);
    }
    let out = command.args(["capture-pane", "-p", "-J", "-t", target]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

// ─────────────────────────── 退役・戻し（`account retire` / `restore`） ───────────────────────────

/// 退役先（`<state_dir>/accounts/.retired/<label>.<ts の数字と T / Z>`）。ts は `AccountRetired` の ts で、戻しは replay から
/// 同じ path を導く（dir を走査しない・C3）。
pub fn retired_path(state_dir: &Path, label: &str, ts: &str) -> PathBuf {
    let stamp: String = ts.chars().filter(|ch| !matches!(ch, '-' | ':')).collect();
    account_dir(state_dir, RETIRED_DIR).join(format!("{label}.{stamp}"))
}

/// 口座を退役させる（設計 §3・N1.2・順序固定）: 規則と宣言（`label-invalid` / `unknown`）→ 未退役（`already-retired`）→
/// 登録 row のどれもその label を持たない（`in-use`）→ `<state_dir>/accounts/<label>` を `.retired/<label>.<ts>` へ rename
/// （link は link のまま・跨 device は `write-failed`）→ `AccountRetired` を 1 件。host の面の行は消さない。前提違反は何も書かない。
pub fn retire(state_dir: &Path, label: &str) -> Result<PathBuf, AccountError> {
    let state = declared_state(state_dir, label)?;
    if state.retired.contains_key(label) {
        return Err(AccountError::AlreadyRetired);
    }
    // 退役は host 全体の席を守る側＝置き場の全 row を見る（anchor で絞らない・設計 account-autonomy.md §14 (1)）。
    if state.registered_accounts(None).contains(label) {
        return Err(AccountError::InUse);
    }
    let ts = fleet_cli::now_utc();
    let from = account_dir(state_dir, label);
    let to = retired_path(state_dir, label, &ts);
    if fs::symlink_metadata(&from).is_err() || fs::symlink_metadata(&to).is_ok() {
        return Err(AccountError::WriteFailed);
    }
    let parent = to.parent().ok_or(AccountError::WriteFailed)?;
    fs::create_dir_all(parent).map_err(|_| AccountError::WriteFailed)?;
    move_and_record(state_dir, (&from, &to), EventKind::AccountRetired, label, &ts)?;
    Ok(to)
}

/// 退役させた口座を戻す（設計 §3）: 規則と宣言 → 退役中（`not-retired`）→ 元の場所が空いている（`dir-exists`）→ 最後の
/// `AccountRetired` の ts が名指す `.retired/<label>.<ts>`（＝最新）を元へ rename → `AccountRestored` を 1 件。
pub fn restore(state_dir: &Path, label: &str) -> Result<PathBuf, AccountError> {
    let state = declared_state(state_dir, label)?;
    let Some(retired_at) = state.retired.get(label) else {
        return Err(AccountError::NotRetired);
    };
    let from = retired_path(state_dir, label, retired_at);
    let to = account_dir(state_dir, label);
    if fs::symlink_metadata(&to).is_ok() {
        return Err(AccountError::DirExists);
    }
    move_and_record(state_dir, (&from, &to), EventKind::AccountRestored, label, &fleet_cli::now_utc())?;
    Ok(to)
}

/// label の規則 → 宣言（tracked + host の面）に在ること（`unknown`）→ event log の replay。読めない周は `write-failed`。
fn declared_state(state_dir: &Path, label: &str) -> Result<State, AccountError> {
    if !label_ok(label) {
        return Err(AccountError::LabelInvalid);
    }
    let manifest = crate::rules::read(None, Some(state_dir)).map_err(|_| AccountError::WriteFailed)?;
    if !manifest.accounts().iter().any(|account| account.label() == label) {
        return Err(AccountError::Unknown);
    }
    store::read_all(state_dir).map(|events| replay(&events)).map_err(|_| AccountError::WriteFailed)
}

/// `from` を `to` へ rename し、event を 1 件積む（積めない周は rename を戻す＝dir と event の片方だけを残さない）。
fn move_and_record(state_dir: &Path, (from, to): (&Path, &Path), kind: EventKind, label: &str, ts: &str) -> Result<(), AccountError> {
    fs::rename(from, to).map_err(|_| AccountError::WriteFailed)?;
    if record(state_dir, kind, label, ts).is_err() {
        let _ = fs::rename(to, from);
        return Err(AccountError::WriteFailed);
    }
    Ok(())
}

/// 退役・戻しの event を 1 件積む（`account` = label・`run` / `bead` を持たない・actor は machine・schema 1）。
fn record(state_dir: &Path, kind: EventKind, label: &str, ts: &str) -> Result<(), AccountError> {
    let event = Event {
        schema: SCHEMA,
        ts: ts.to_owned(),
        kind,
        run: String::new(),
        bead: String::new(),
        host: fleet_cli::host(),
        actor: kind.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: None,
        allowance: None,
        registration: None,
        mark: None,
        account: Some(label.to_owned()),
        cost: None,
        rule: None,
    };
    let policy = LockPolicy::embedded().map_err(|_| AccountError::WriteFailed)?;
    store::append(state_dir, &event, policy).map(|_| ()).map_err(|_| AccountError::WriteFailed)
}

#[cfg(test)]
mod tests {
    use super::{
        accept_trust, capture_joined, label_ok, login_line, retired_path, stage_host, staged_fits, summary, AccountError, TrustWrite,
        CLAUDE_FILE, ERRORS,
    };
    use crate::fleet::{Allowance, AllowanceLatest, Measured, State, Unmeasured, UnmeasuredReason, WindowKind};
    use crate::order::is_declaration_order;
    use crate::pipe::fixture::{exited, Call, Stub};
    use std::fs;
    use std::path::{Path, PathBuf};

    /// 口座の pane の読みは起動の記述を通る（設計 core-boundary.md §9 行 f）: program は tmux・引数は socket が在る周
    /// だけ `-S <path>` を前に付け、`capture-pane -p -J -t <target>`（`-J` で行末の空白を保つ）。rc 非 0 と起動の失敗は `None`。
    #[test]
    fn invocation_seat_account_pane_read_passes_the_target() {
        let stub = Stub::install(|call| match call.args.last().map(String::as_str) {
            Some("work:1") => exited(0, b"$ \n"),
            Some("fail:1") => exited(1, b"out\n"),
            _ => Err(std::io::Error::other("gone")),
        });
        assert_eq!(capture_joined(Some("/tmp/sock"), "work:1"), Some("$ \n".to_owned()), "socket 付きの stdout");
        assert_eq!(capture_joined(None, "work:1"), Some("$ \n".to_owned()), "socket 無しの stdout");
        assert_eq!(capture_joined(None, "fail:1"), None, "rc 非 0");
        assert_eq!(capture_joined(None, "gone:1"), None, "起動の失敗");
        let tmux = |args: &[&str]| Call {
            program: "tmux".to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            cwd: None,
            envs: Vec::new(),
        };
        let expected = [
            tmux(&["-S", "/tmp/sock", "capture-pane", "-p", "-J", "-t", "work:1"]),
            tmux(&["capture-pane", "-p", "-J", "-t", "work:1"]),
            tmux(&["capture-pane", "-p", "-J", "-t", "fail:1"]),
            tmux(&["capture-pane", "-p", "-J", "-t", "gone:1"]),
        ];
        assert_eq!(stub.calls(), expected, "tmux の program と引数");
    }

    /// 断りの字面は設計 §7 の 8 語で、宣言順に閉じる（variant を足した周はここの件数が変わる）。
    #[test]
    fn account_cmd_errors_are_the_eight_words_in_declaration_order() {
        let words: Vec<&str> = ERRORS.iter().map(|error| error.as_str()).collect();
        assert_eq!(
            words,
            ["exists", "dir-exists", "unknown", "already-retired", "not-retired", "in-use", "label-invalid", "write-failed"]
        );
        assert!(is_declaration_order(ERRORS, |error| error as usize), "ERRORS は宣言順: {ERRORS:?}");
        assert_eq!(AccountError::InUse.as_str(), "in-use");
    }

    /// label は `accounts/` の 1 成分として閉じる: 空・`.` 始まり（`.retired` / `..`）・区切り・空白・引用符・shell の字は取らない。
    #[test]
    fn account_cmd_label_rule_keeps_one_path_component() {
        for good in ["a1", "acct-1", "work_2", "x.y", "me@host", "a+b", "A9"] {
            assert!(label_ok(good), "{good} は通る");
        }
        for bad in ["", ".", "..", ".retired", ".x", "a/b", "../x", "a b", "a\"b", "a;b", "a$b", "a\nb", "ä"] {
            assert!(!label_ok(bad), "{bad:?} は断る");
        }
    }

    /// 退役先は `accounts/.retired/<label>.<ts から - と : を除いた字面>`（戻しは同じ ts から同じ path を導く）。
    #[test]
    fn account_cmd_retired_path_is_derived_from_the_retire_ts() {
        let path = retired_path(Path::new("/s"), "a1", "2026-09-14T15:20:07Z");
        assert_eq!(path, Path::new("/s/accounts/.retired/a1.20260914T152007Z"));
    }

    /// login の起動行は cwd へ移ってから口座の設定 dir で claude を起こす 1 行。
    #[test]
    fn account_cmd_login_line_moves_to_the_cwd_then_starts_claude_on_the_config_dir() {
        let line = login_line(Path::new("/repo"), Path::new("/s/accounts/a1"));
        assert_eq!(line, "cd /repo && CLAUDE_CONFIG_DIR=/s/accounts/a1 claude");
    }

    /// 実測 1 行。
    fn measured(label: &str, window: WindowKind, pct: u64) -> Allowance {
        Allowance::Measured(Measured {
            account: label.to_owned(),
            window,
            model: None,
            endpoint: "oauth-usage".to_owned(),
            used_pct: pct,
            resets_at: None,
        })
    }

    /// 回の列を物理順に積んだ表（同じ key は後が勝つ）。
    fn table(rounds: &[(&str, Vec<Allowance>)]) -> State {
        let mut state = State::default();
        for (ts, rows) in rounds {
            for row in rows {
                state.allowance.insert(row.key(), AllowanceLatest { ts: (*ts).to_owned(), allowance: row.clone() });
            }
        }
        state
    }

    /// 要約は最新の回だけを読む: 実測の窓は使用率・最新の回が口座単位の Unmeasured なら両窓 `unmeasured`（古い実測を
    /// 名乗らない）・行が無い口座と log を読めない周も `unmeasured`（0 に読み替えない）。
    #[test]
    fn account_cmd_summary_reads_only_the_latest_round() {
        let old = "2026-09-14T00:00:00Z";
        let new = "2026-09-14T01:00:00Z";
        let fresh = table(&[(old, vec![measured("a1", WindowKind::FiveHour, 90)]), (new, vec![measured("a1", WindowKind::FiveHour, 13), measured("a1", WindowKind::SevenDay, 41)])]);
        assert_eq!(summary(Some(&fresh), "a1"), "five_hour=13% seven_day=41%");
        let failed = Allowance::Unmeasured(Unmeasured {
            account: "a1".to_owned(),
            window: None,
            model: None,
            endpoint: "oauth-usage".to_owned(),
            reason: UnmeasuredReason::NoCredentials,
        });
        let broken = table(&[(old, vec![measured("a1", WindowKind::FiveHour, 13)]), (new, vec![failed])]);
        assert_eq!(summary(Some(&broken), "a1"), "five_hour=unmeasured seven_day=unmeasured", "古い実測を名乗らない");
        assert_eq!(summary(Some(&fresh), "a2"), "five_hour=unmeasured seven_day=unmeasured", "行の無い口座");
        assert_eq!(summary(None, "a1"), "five_hour=unmeasured seven_day=unmeasured", "log を読めない");
    }

    /// 歯ごとの空の tmp dir（in-file の歯の置き場・env を読まないのは器の本体の規律〔C2.2〕）。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("account-stage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        dir
    }

    /// `body` を一時 file に書いて [`staged_fits`] を撃つ（書けない周は歯を落とす＝空虚に通さない）。
    fn fits(dir: &Path, body: &str, label: &str) -> bool {
        let staged = dir.join("host.toml.staged");
        assert!(fs::write(&staged, body).is_ok(), "一時 file を書ける");
        staged_fits(&staged, label)
    }

    /// trust の書き手は末尾の改行を元の file に合わせる（無い file は無いまま・host-init.md §7 形 2）・2 度目は `accepted` で
    /// 全文が変わらない。
    #[test]
    fn account_trust_keeps_the_trailing_newline_of_the_original() {
        let dir = scratch("trust-newline");
        let file = dir.join(CLAUDE_FILE);
        assert!(fs::write(&file, "{\"k\": 1}").is_ok(), "fixture を書ける");
        assert_eq!(accept_trust(&dir, "/r"), TrustWrite::Written);
        let want = "{\n  \"k\": 1,\n  \"projects\": {\n    \"/r\": {\n      \"hasTrustDialogAccepted\": true\n    }\n  }\n}";
        assert_eq!(fs::read_to_string(&file).ok().as_deref(), Some(want), "末尾の改行は足さない");
        assert_eq!(accept_trust(&dir, "/r"), TrustWrite::Accepted);
        assert_eq!(fs::read_to_string(&file).ok().as_deref(), Some(want), "2 度目は書かない");
        let _ = fs::remove_dir_all(&dir);
    }

    // flip-check: retroactive s2-07l.283
    /// 読める face が `label` を宣言する周だけ true（.245 run 3 の生存 `staged_fits → true` を弁別する）。
    #[test]
    fn account_stage_fits_when_the_face_reads_and_declares_the_label() {
        let dir = scratch("declares");
        assert!(fits(&dir, "schema = 1\n\n[[account]]\nlabel = \"a1\"\n", "a1"));
    }

    // flip-check: retroactive s2-07l.283
    /// 読めても `label` を宣言しない face は false（宣言の行が落ちた周を通さない）。
    #[test]
    fn account_stage_does_not_fit_when_the_face_reads_but_lacks_the_label() {
        let dir = scratch("lacks");
        assert!(!fits(&dir, "schema = 1\n\n[[account]]\nlabel = \"a1\"\n", "a2"));
    }

    // flip-check: retroactive s2-07l.283
    /// 未知 key を持つ face は契約 (a) の loader が断る＝false（label が在っても）。
    #[test]
    fn account_stage_does_not_fit_when_the_face_has_an_unknown_key() {
        let dir = scratch("unknown-key");
        assert!(!fits(&dir, "schema = 1\n\n[[account]]\nlabel = \"a1\"\nbogus = 1\n", "a1"));
    }

    // flip-check: retroactive s2-07l.283
    /// [`stage_host`] は host.toml が**在るのに読めない**（dir）周と、読めても一時 file が face として読めない（未知 key）周の
    /// どちらも `write-failed` で、一時 file を残さない（`schema = 1` から作り直さず・壊れた面を rename に回さない・NFR4）。
    /// `add` の口は `rules::read` が先に断るので、この 2 分岐は関数を直接撃つ。
    #[test]
    fn account_stage_host_refuses_an_unreadable_or_unparseable_face_without_a_staged_file() {
        let dir = scratch("host");
        let host = dir.join(crate::rules::HOST_MANIFEST);
        let staged = dir.join("host.toml.staged");
        assert!(fs::create_dir(&host).is_ok(), "host.toml を dir として置ける");
        assert_eq!(stage_host(&dir, "a1"), Err(AccountError::WriteFailed), "在るのに読めない周は無いに読み替えない");
        assert!(fs::symlink_metadata(&staged).is_err(), "dir: 一時 file を残さない");
        assert!(fs::remove_dir(&host).is_ok() && fs::write(&host, "schema = 1\n\n[[account]]\nlabel = \"x\"\nbogus = 1\n").is_ok());
        assert_eq!(stage_host(&dir, "a1"), Err(AccountError::WriteFailed), "一時 file が face として読めない周");
        assert!(fs::symlink_metadata(&staged).is_err(), "未知 key: 一時 file を残さない");
        let _ = fs::remove_dir_all(&dir);
    }
}
