//! host の破壊防止の見張り（subcommand `host-guard`・設計 docs/design/vessel-hook.md §11 行 b・ADR-0056・
//! SRS FR56 / FR20 / FR24 / AC7 / AC27 / NFR4 / NFR5）。
//!
//! 口座の設定から PreToolUse で呼ばれる器の 1 つの口で、**marker と anchor に依らず**判定する（hook の入口の沈黙
//! 〔FR24〕は持ち込まない＝`.vessel` の無い repo・他の name の marker・git repo でない cwd でも同じ判定）。種類は閉じた
//! 5 値 [`Kind`] で、判定は宣言順に 1 種類 1 関数、**先に当たった 1 つだけ**を断る（1 周に deny 1 行）。何を止めるかは
//! 種類ごとの rules 行（[`WORD_ROWS`] と [`RM_ROW`]・裁定 id つき）が持ち、見張り自身の設定の種類は行を持たない。
//!
//! 語列の 3 種類（git / tmux / 台帳）は、command 行を起票の門の分割（[`segments`]・引用符と `\` を解く）で切り、
//! `NAME=value` の前置きを読み飛ばしてから command guard と同じ照合（[`matched`]）に掛ける。rm の種類（行 c）は rm の
//! segment の path を payload の cwd から解き、守る集合（[`Protected`]）と一致・祖先・配下で当たる rm と、解けない path の
//! rm を断る。台帳の形・見張り自身の設定は行 f / e が同じ関数の中身を埋める（まだ当たらない）。
//!
//! payload が JSON でない・`tool_name` が無い・`Bash` なのに command が無い・`--state-dir` が無い・rules が読めない周は
//! **deny**（FailClosed・[`POLARITY`]）。Bash / 編集系でない tool は 1 byte も書かず rc 0。断る周だけ `inject.jsonl` に
//! 1 行を残す（tmux は撃たない・席は null）。env も HOME も読まない（C2.2）。git の子 process は rm の segment が在り
//! repo の root が解けた周の `git ls-files` 1 回だけ（NFR5）。

use super::command::{matched, BASH};
use super::ledger_guard::{is_assignment, segments};
use super::{append, command_of, InjectionRecord, SCHEMA};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::json_tree::{self, Tree};
use crate::name::NAME;
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// この境界の極性: 行為の時点で止め、payload・引数・rules を読めない周は通さない。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// git の履歴破壊の語列の行。
pub const GIT_ROW: &str = "host_guard.git";
/// tmux server の破壊の語列の行。
pub const TMUX_ROW: &str = "host_guard.tmux";
/// 台帳の破壊の語列の行。
pub const LEDGER_ROW: &str = "host_guard.ledger";
/// rm の守る集合の行。
pub const RM_ROW: &str = "host_guard.rm";

/// 語列の行の閉じた列（宣言順・command guard も enabled を見ずに読む）。
pub const WORD_ROWS: [&str; 3] = [GIT_ROW, TMUX_ROW, LEDGER_ROW];

/// 判定に載る tool（Bash と編集系）。これ以外は 1 byte も書かず通す。
const TOOLS: [&str; 5] = [BASH, "Edit", "Write", "MultiEdit", "NotebookEdit"];

/// 記録の置き場の flag。
const FLAG_STATE_DIR: &str = "--state-dir";
/// rules manifest を差し替える flag（歯の seam・hook と同じ形）。
const FLAG_RULES: &str = "--rules";
/// payload から拾う key（tool 名）。
const KEY_TOOL: &str = "tool_name";
/// payload から拾う key（相対 path の基準・repo の root を辿る起点）。
const KEY_CWD: &str = "cwd";

/// rm の動詞（launcher を剥いだ先頭語の basename）。
const RM: &str = "rm";
/// 後ろの segment の相対 path を解けなくする動詞（cd の先は追わない）。
const CD: [&str; 2] = ["cd", "pushd"];
/// 透過の launcher（剥ぐ語・値を取る flag・flag の後ろで読み飛ばす位置引数の数〔timeout の秒〕）の閉じた列。
const LAUNCHERS: &[(&str, &[&str], usize)] = &[
    ("sudo", &["-u", "-g", "-h", "-p", "-C", "-D", "-r", "-t", "-U", "-T"], 0),
    ("env", &["-u", "-C", "-S"], 0),
    ("timeout", &["-s", "-k"], 1),
    ("command", &[], 0),
    ("exec", &["-a"], 0),
    ("nice", &["-n"], 0),
];
/// 解けない形の字（`$` は変数展開と `$(` の command 置換・`` ` `` は command 置換・`{` `}` は brace 展開）。
const UNRESOLVED: [char; 4] = ['$', '`', '{', '}'];
/// glob の字（字より前の literal な接頭の dir を解いて判定する）。
const GLOB: [char; 3] = ['*', '?', '['];
/// repo の root の印（dir か worktree の file）。
const DOT_GIT: &str = ".git";
/// worktree の `.git` の file が本体の git dir を指す行の頭。
const GITDIR: &str = "gitdir:";
/// 本体の git dir の下で worktree ごとの dir を束ねる dir の名（common dir はその 1 つ上）。
const WORKTREES: &str = "worktrees";

/// rm の守る集合の記号（閉じた 3 値・宣言順が当たりの順・rules 行 host_guard.rm の値の語）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protected {
    /// `--state-dir` の実体自身とその配下全部。
    StateDir,
    /// cwd の repo の root からの `git ls-files` が返す file。
    RepoTracked,
    /// repo の root の `.git` と、worktree なら `gitdir:` 行が指す本体の common dir。
    RepoGit,
}

/// [`Protected`] の全 variant（宣言順）。
pub const PROTECTED: &[Protected] = &[Protected::StateDir, Protected::RepoTracked, Protected::RepoGit];

impl Protected {
    /// rules 行の値と断りの行の hit に出す記号。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StateDir => "state-dir",
            Self::RepoTracked => "repo-tracked",
            Self::RepoGit => "repo-git",
        }
    }

    /// 記号の字面から引く（綴り違いは `None`＝読み込みで拒む）。
    pub fn parse(text: &str) -> Option<Self> {
        PROTECTED.iter().copied().find(|found| found.as_str() == text)
    }
}

/// 判定の場: payload の cwd・`--state-dir`・git の program（歯が偽の git で呼出回数を数える seam）。
pub struct Scene<'a> {
    /// payload の `cwd`（無ければ process の cwd）。
    pub cwd: &'a Path,
    /// 記録の置き場（守る集合の state-dir）。
    pub state_dir: &'a Path,
    /// `git ls-files` を撃つ program。
    pub git: &'a Path,
}

/// 止める種類（閉じた 5 値・宣言順が判定の順）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// git の履歴破壊。
    Git,
    /// rm の不可逆削除（行 c）。
    Rm,
    /// tmux server の破壊。
    Tmux,
    /// 台帳の破壊と素 write（形の判定は行 f）。
    Ledger,
    /// 見張り自身の設定の編集（行 e・行を持たない）。
    Settings,
}

/// [`Kind`] の全 variant（宣言順）。
pub const KINDS: &[Kind] = &[Kind::Git, Kind::Rm, Kind::Tmux, Kind::Ledger, Kind::Settings];

impl Kind {
    /// 断りの行と記録に出す種類の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Git => "git",
            Self::Rm => "rm",
            Self::Tmux => "tmux",
            Self::Ledger => "ledger",
            Self::Settings => "settings",
        }
    }

    /// 種類の rules 行の id（見張り自身の設定は行を持たない＝切れない唯一の種類）。
    pub fn row(self) -> Option<&'static str> {
        match self {
            Self::Git => Some(GIT_ROW),
            Self::Rm => Some(RM_ROW),
            Self::Tmux => Some(TMUX_ROW),
            Self::Ledger => Some(LEDGER_ROW),
            Self::Settings => None,
        }
    }

    /// 代わりの経路（1 種類 1 文・断りの行の末尾）。
    pub fn route(self) -> &'static str {
        match self {
            Self::Git => "履歴を残す形で書く（--force-with-lease か新しい branch）",
            Self::Rm => "tracked なら git rm・それ以外は退役の mv で脇へ移す",
            Self::Tmux => "自席の window だけを操作する（server と他の session は壊さない）",
            Self::Ledger => "台帳は bdw と --append-notes で書く",
            Self::Settings => "見張り自身の設定は user が編集する",
        }
    }
}

/// host-guard の判定。**bool で持たない**（憲法 C11）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostGuardDecision {
    /// 通す（1 byte も書かない・記録も残さない）。
    Allow,
    /// 止める。`what` は記録の `what`・`line` は stderr へ出す 1 行。
    Deny {
        /// 記録の種別（`host-guard-deny <種類の語>`）。
        what: String,
        /// stderr の 1 行。
        line: String,
    },
}

/// 読めない周の閉じた理由（FailClosed）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreadable {
    /// 引数が `--state-dir` / `--rules` の対でない。
    BadArgs,
    /// `--state-dir` が無い。
    NoStateDir,
    /// payload が JSON として読めない。
    PayloadUnreadable,
    /// payload に `tool_name` が無い。
    NoToolName,
    /// `Bash` なのに command が無い。
    NoCommand,
    /// rules を読めない。
    RulesUnreadable,
}

impl Unreadable {
    /// 断りの行と記録に出す理由の 1 語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BadArgs => "bad-args",
            Self::NoStateDir => "no-state-dir",
            Self::PayloadUnreadable => "payload-unreadable",
            Self::NoToolName => "no-tool-name",
            Self::NoCommand => "no-command",
            Self::RulesUnreadable => "rules-unreadable",
        }
    }

    /// 断りの 1 行（種類は解けていない＝`kind=-`）。
    fn line(self) -> String {
        format!(
            "{NAME}: host-guard deny kind=- hit={} row=- ruling=- — 読めない周は通さない（fail-closed）: 配線の引数・payload・rules を直す",
            self.as_str()
        )
    }
}

/// 断る 1 件（種類・当たったもの・行 id・裁定 id）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Refusal {
    /// 種類。
    kind: Kind,
    /// 当たった語列か、解けない理由の 1 語。
    hit: String,
    /// 行 id。
    row: &'static str,
    /// 裁定 id（行が読めない周は `-`）。
    ruling: String,
}

impl Refusal {
    /// 種類の行が無い・列でない周（FailClosed）。
    fn no_row(kind: Kind, row: &'static str) -> Self {
        Self { kind, hit: "no-row".to_owned(), row, ruling: "-".to_owned() }
    }

    /// 判定へ写す。
    fn decision(self) -> HostGuardDecision {
        let line = format!(
            "{NAME}: host-guard deny kind={} hit={} row={} ruling={} — {}",
            self.kind.as_str(),
            self.hit,
            self.row,
            self.ruling,
            self.kind.route()
        );
        HostGuardDecision::Deny { what: format!("host-guard-deny {}", self.kind.as_str()), line }
    }
}

/// 判定の材料（tool と、Bash なら前置きを剥いだ segment の列と、判定の場）。
struct Subject<'a> {
    /// tool 名。
    tool: &'a str,
    /// segment の語（起票の門の分割・`NAME=value` の前置きを剥いだ後・空は捨てる）。
    segments: Vec<Vec<String>>,
    /// 判定の場。
    scene: &'a Scene<'a>,
}

/// `host-guard` に続く引数と stdin の payload を捌く。通す周は 0 byte・rc 0、断る周は rc 2・stderr 1 行・stdout 0 byte
/// （記録の置き場が解けた周は `inject.jsonl` に 1 行）。
pub fn dispatch(args: &[String], payload: &str) -> Outcome {
    let started = Instant::now();
    let Some((state_dir, rules)) = args_of(args) else {
        return Outcome::failed_line(RC_BROKEN, Unreadable::BadArgs.line());
    };
    let Some(state_dir) = state_dir else {
        return Outcome::failed_line(RC_BROKEN, Unreadable::NoStateDir.line());
    };
    let (what, line) = match decide(payload, rules, Path::new(state_dir)) {
        Ok(HostGuardDecision::Allow) => return Outcome::ok(Vec::new()),
        Ok(HostGuardDecision::Deny { what, line }) => (what, line),
        Err(reason) => (format!("host-guard-deny reason={}", reason.as_str()), reason.line()),
    };
    let record = InjectionRecord {
        schema: SCHEMA,
        who: "host-guard".to_owned(),
        what,
        when: "PreToolUse".to_owned(),
        bytes: u64::try_from(line.len()).unwrap_or(u64::MAX).saturating_add(1),
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        seat: None,
        ts: crate::seat::state::now_secs(),
    };
    // 記録の失敗で判定を濁さない（hook の deny と同じ・stderr は判定文 1 行だけ）。
    let _ = append(Path::new(state_dir), &record);
    Outcome::failed_line(RC_BROKEN, line)
}

/// `--state-dir D` と `--rules F` だけを読む（値欠け・空文字・重複・未知の引数は `None`）。
fn args_of(args: &[String]) -> Option<(Option<&str>, Option<&str>)> {
    let (mut state_dir, mut rules) = (None, None);
    for pair in args.chunks(2) {
        let value = pair.get(1).map(String::as_str).filter(|found| !found.trim().is_empty() && !found.starts_with("--"));
        match (pair.first().map(String::as_str), value) {
            (Some(FLAG_STATE_DIR), Some(found)) if state_dir.is_none() => state_dir = Some(found),
            (Some(FLAG_RULES), Some(found)) if rules.is_none() => rules = Some(found),
            _ => return None,
        }
    }
    Some((state_dir, rules))
}

/// payload と rules（`--rules` の差し替えか埋め込み）と記録の置き場から判定する。読めない周は理由を `Err` で返す
/// （FailClosed）。cwd は payload の `cwd`（無ければ process の cwd）。
pub fn decide(payload: &str, rules: Option<&str>, state_dir: &Path) -> Result<HostGuardDecision, Unreadable> {
    let tree = json_tree::parse(payload).map_err(|_| Unreadable::PayloadUnreadable)?;
    let tool = tree.get(KEY_TOOL).and_then(Tree::as_str).ok_or(Unreadable::NoToolName)?;
    if !TOOLS.contains(&tool) {
        return Ok(HostGuardDecision::Allow);
    }
    let command = match tool {
        BASH => command_of(payload).ok_or(Unreadable::NoCommand)?,
        _ => String::new(),
    };
    let manifest = rules
        .map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path)))
        .map_err(|_| Unreadable::RulesUnreadable)?;
    let cwd = match tree.get(KEY_CWD).and_then(Tree::as_str) {
        Some(found) => PathBuf::from(found),
        None => std::env::current_dir().unwrap_or_default(),
    };
    Ok(judge(tool, &command, &manifest, &Scene { cwd: &cwd, state_dir, git: Path::new("git") }))
}

/// manifest の行と判定の場から判定する。種類を宣言順に回し、先に当たった 1 つだけを断る。
pub fn judge(tool: &str, command: &str, manifest: &Manifest, scene: &Scene) -> HostGuardDecision {
    let segments = match tool {
        BASH => segments(command)
            .into_iter()
            .map(|words| words.into_iter().skip_while(|word| is_assignment(word)).collect::<Vec<String>>())
            .filter(|words| !words.is_empty())
            .collect(),
        _ => Vec::new(),
    };
    let subject = Subject { tool, segments, scene };
    match KINDS.iter().find_map(|kind| judge_kind(*kind, &subject, manifest)) {
        Some(found) => found.decision(),
        None => HostGuardDecision::Allow,
    }
}

/// 1 種類の判定（1 種類 1 arm）。当たらなければ `None`。
fn judge_kind(kind: Kind, subject: &Subject, manifest: &Manifest) -> Option<Refusal> {
    match kind {
        Kind::Git | Kind::Tmux | Kind::Ledger => sequences(kind, subject, manifest),
        Kind::Rm => removals(kind, subject, manifest),
        // 見張り自身の設定は行 e が中身を埋める（まだ当たらない）。
        Kind::Settings => None,
    }
}

/// 語列の種類の判定: Bash の segment を種類の行の語列に掛ける（command guard と同じ照合）。行が無い・列でない周は
/// 種類の語で断り（FailClosed）、`enabled = false` の行はこの口だけを切る（通す）。
fn sequences(kind: Kind, subject: &Subject, manifest: &Manifest) -> Option<Refusal> {
    let id = kind.row().filter(|_| subject.tool == BASH)?;
    let Some(row) = manifest.get(id) else {
        return Some(Refusal::no_row(kind, id));
    };
    if !row.enabled {
        return None;
    }
    let RuleValue::List(ref denied) = row.value else {
        return Some(Refusal::no_row(kind, id));
    };
    matched(&subject.segments, denied).map(|hit| Refusal { kind, hit: hit.sequence, row: id, ruling: row.ruling.clone() })
}

/// rm の種類の判定: rm の segment の path の語を 1 つずつ見て、解けない形か守る集合に当たる最初の 1 つで断る。rm の
/// segment が無い周は行を読まない。行が無い・列でない周は rm を断り（FailClosed）、`enabled = false` はこの種類だけを切る。
fn removals(kind: Kind, subject: &Subject, manifest: &Manifest) -> Option<Refusal> {
    let words = rm_words(&subject.segments)?;
    let Some(row) = manifest.get(RM_ROW) else {
        return Some(Refusal::no_row(kind, RM_ROW));
    };
    if !row.enabled {
        return None;
    }
    let RuleValue::List(ref values) = row.value else {
        return Some(Refusal::no_row(kind, RM_ROW));
    };
    let guarded = guarded(values, subject.scene);
    let hit = words.iter().find_map(|(word, after_cd)| hit_of(word, *after_cd, subject.scene.cwd, &guarded))?;
    Some(Refusal { kind, hit, row: RM_ROW, ruling: row.ruling.clone() })
}

/// 透過の launcher を剥いだ動詞の basename と、その後ろの語（rm の同定と行 e の動詞の同定が同じ 1 関数）。
pub(crate) fn verb_of(words: &[String]) -> Option<(&str, &[String])> {
    let mut at = 0_usize;
    loop {
        let word = words.get(at)?;
        let verb = word.rsplit('/').next().unwrap_or(word).trim_start_matches('\\');
        let Some((_, valued, positional)) = LAUNCHERS.iter().find(|(name, _, _)| *name == verb) else {
            return Some((verb, words.get(at.saturating_add(1)..).unwrap_or_default()));
        };
        at = past_launcher(words, at.saturating_add(1), valued).saturating_add(*positional);
    }
}

/// launcher の flag（値を取る flag はその値ごと）・`NAME=value`・`--` を読み飛ばした位置。
fn past_launcher(words: &[String], mut at: usize, valued: &[&str]) -> usize {
    while let Some(word) = words.get(at) {
        let step = match word.as_str() {
            "--" => return at.saturating_add(1),
            flag if valued.contains(&flag) => 2,
            flag if flag.starts_with('-') || is_assignment(flag) => 1,
            _ => break,
        };
        at = at.saturating_add(step);
    }
    at
}

/// rm の segment の path の語と、同じ command の cd / pushd の segment より後ろかどうかの対。rm の segment が無ければ
/// `None`。`-` で始まる語は flag として読み飛ばし、`--` の後ろは全部 path。
fn rm_words(segments: &[Vec<String>]) -> Option<Vec<(&str, bool)>> {
    let (mut after_cd, mut found) = (false, None::<Vec<(&str, bool)>>);
    for words in segments {
        match verb_of(words) {
            Some((RM, rest)) => {
                let mut ended = false;
                let paths = rest.iter().filter(|word| {
                    let path = ended || !(word.starts_with('-') && word.len() > 1);
                    ended = ended || word.as_str() == "--";
                    path
                });
                found.get_or_insert_with(Vec::new).extend(paths.map(|word| (word.as_str(), after_cd)));
            }
            Some((verb, _)) if CD.contains(&verb) => after_cd = true,
            _ => {}
        }
    }
    found
}

/// 守る path の 1 群（記号と、字面の path と実体の path の列）。
struct Guarded {
    /// 記号。
    symbol: Protected,
    /// 守る path（字面と、実体が在れば realpath）。
    paths: Vec<PathBuf>,
}

/// 行の値に載る記号だけを宣言順に解く（値に無い記号は守らない）。repo の root が解けない周は repo-tracked と
/// repo-git が空で、git は撃たない。
fn guarded(values: &[String], scene: &Scene) -> Vec<Guarded> {
    let root = root_of(scene.cwd);
    let listed = PROTECTED.iter().filter(|symbol| values.iter().any(|value| value == symbol.as_str()));
    listed
        .map(|symbol| {
            let paths = match (symbol, &root) {
                (Protected::StateDir, _) => both(scene.state_dir.to_path_buf()),
                (Protected::RepoTracked, Some(root)) => tracked(root, scene.git),
                (Protected::RepoGit, Some(root)) => git_paths(root),
                (Protected::RepoTracked | Protected::RepoGit, None) => Vec::new(),
            };
            Guarded { symbol: *symbol, paths }
        })
        .collect()
}

/// path の字面と、実体が在ればその realpath（同じなら 1 つ）。
fn both(path: PathBuf) -> Vec<PathBuf> {
    let real = fs::canonicalize(&path).ok().filter(|real| *real != path);
    std::iter::once(path).chain(real).collect()
}

/// cwd から `.git`（dir か file）を持つ最も近い祖先の dir（fs で辿る・git は撃たない）。cwd が絶対 path でなければ無い。
fn root_of(cwd: &Path) -> Option<PathBuf> {
    let found = cwd.ancestors().find(|dir| fs::symlink_metadata(dir.join(DOT_GIT)).is_ok());
    found.filter(|_| cwd.is_absolute()).map(Path::to_path_buf)
}

/// root からの `git ls-files -z` の file（root の字面と実体の両方で結ぶ）。git を読めない周は root 全体を守る
/// （tracked を知らずに通さない・fail-closed）。
fn tracked(root: &Path, git: &Path) -> Vec<PathBuf> {
    let listed = Command::new(git).arg("-C").arg(root).args(["ls-files", "-z"]).output().ok();
    let roots = both(root.to_path_buf());
    let Some(output) = listed.filter(|output| output.status.success()) else {
        return roots;
    };
    let names: Vec<&OsStr> =
        output.stdout.split(|byte| *byte == 0).filter(|name| !name.is_empty()).map(OsStr::from_bytes).collect();
    roots.iter().flat_map(|root| names.iter().map(move |name| root.join(name))).collect()
}

/// root の `.git` と、それが worktree の file なら `gitdir:` 行が指す本体の common dir（`worktrees/<名>` の 2 つ上）。
fn git_paths(root: &Path) -> Vec<PathBuf> {
    let dot = root.join(DOT_GIT);
    let common = fs::read_to_string(&dot).ok().and_then(|text| {
        let gitdir = fold(root, text.lines().find_map(|line| line.strip_prefix(GITDIR))?.trim());
        let parent = gitdir.parent()?;
        match parent.file_name() {
            Some(name) if name == WORKTREES => parent.parent().map(Path::to_path_buf),
            _ => Some(gitdir),
        }
    });
    let mut paths = both(dot);
    paths.extend(common.map(both).unwrap_or_default());
    paths
}

/// cwd を基準に `.` と `..` を字句で畳んだ path（`.` と空は cwd 自身・絶対 path はそのまま畳む）。
fn fold(cwd: &Path, word: &str) -> PathBuf {
    let mut folded = PathBuf::new();
    for part in cwd.join(word).components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                folded.pop();
            }
            other => folded.push(other),
        }
    }
    folded
}

/// 解けない形か: 変数展開・command 置換・brace を含む語、`~` で始まる語、cd / pushd の後ろ（か cwd が解けない周）の
/// 相対 path。
fn unresolved(word: &str, after_cd: bool, cwd: &Path) -> bool {
    let relative = !Path::new(word).is_absolute();
    word.contains(UNRESOLVED) || word.starts_with('~') || (relative && (after_cd || !cwd.is_absolute()))
}

/// rm の対象の字面の正規化と実体の path。実体の無い path は `None`（通す）。symlink そのものは link 自身の path
/// （親 dir だけ realpath に解く）で見る（末尾 `/` は link の先を指す）。
fn forms_of(cwd: &Path, word: &str) -> Option<Vec<PathBuf>> {
    let joined = cwd.join(word);
    let meta = fs::symlink_metadata(&joined).ok()?;
    let real = if meta.file_type().is_symlink() && !word.ends_with('/') {
        let parent = joined.parent().and_then(|parent| fs::canonicalize(parent).ok());
        parent.zip(joined.file_name()).map(|(parent, name)| parent.join(name))
    } else {
        fs::canonicalize(&joined).ok()
    };
    Some(std::iter::once(fold(cwd, word)).chain(real).collect())
}

/// path の語 1 つの hit（`unresolved:<語>` か `<記号>:<畳んだ path>`）。glob の語は glob の字より前の literal な接頭の
/// dir で判定する。当たる関係は一致・祖先・配下の 3 つ。
fn hit_of(word: &str, after_cd: bool, cwd: &Path, guarded: &[Guarded]) -> Option<String> {
    if unresolved(word, after_cd, cwd) {
        return Some(format!("unresolved:{word}"));
    }
    let target = match word.find(GLOB).and_then(|at| word.get(..at)) {
        Some(prefix) => prefix.rfind('/').and_then(|slash| prefix.get(..=slash)).unwrap_or_default(),
        None => word,
    };
    let forms = forms_of(cwd, target)?;
    let hits = |guard: &&Guarded| guard.paths.iter().any(|q| forms.iter().any(|p| p.starts_with(q) || q.starts_with(p)));
    let found = guarded.iter().find(hits)?;
    Some(format!("{}:{}", found.symbol.as_str(), fold(cwd, word).display()))
}

#[cfg(test)]
mod tests {
    use super::{
        decide, judge, HostGuardDecision, Kind, Protected, Scene, Unreadable, KINDS, LEDGER_ROW, PROTECTED, RM_ROW, TMUX_ROW,
        WORD_ROWS,
    };
    use crate::hook::command::{self, denied_in, CommandDecision};
    use crate::name::NAME;
    use crate::order::is_declaration_order;
    use crate::rules::manifest::Manifest;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// fixture の裁定 id。
    const RULING: &str = "user 2026-09-19T15:28Z";

    /// `runner.denied_commands` の fixture の語列（git の語列は host_guard.git と重複・tmux の語列は持たない）。
    const RUNNER: &[&str] = &["git push --force", "cargo mutants"];

    /// 1 行の本文。
    fn row(id: &str, kind: &str, value: &[&str], enabled: bool) -> String {
        let quoted: Vec<String> = value.iter().map(|item| format!("\"{item}\"")).collect();
        format!(
            "\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = [{}]\nenabled = {enabled}\nruling = \"{RULING}\"\nruled_at = \"2026-09-19\"\n",
            quoted.join(", ")
        )
    }

    /// `runner.denied_commands` と host_guard の語列の 3 行と `extra` の本文を持つ manifest（host_guard.tmux の enabled は
    /// 引数・`drop` の行は持たない）。
    fn manifest_with(tmux_enabled: bool, drop: Option<&str>, extra: &str) -> Manifest {
        let mut text = format!("schema = 1\n{}", row(command::ROW, "RunnerDeniedCommands", RUNNER, true));
        for (id, value, enabled) in [
            (super::GIT_ROW, &["git push --force", "git reset --hard"][..], true),
            (TMUX_ROW, &["tmux kill-server"][..], tmux_enabled),
            (LEDGER_ROW, &["bd delete"][..], true),
        ] {
            if drop != Some(id) {
                text.push_str(&row(id, "HostGuardDeniedCommands", value, enabled));
            }
        }
        text.push_str(extra);
        Manifest::parse(&text).unwrap_or_else(|errors| panic!("fixture の manifest を読める: {errors:?}"))
    }

    /// 語列の 3 行だけの manifest（rm の行を持たない＝rm の segment が無い command の判定は行 b と同じ）。
    fn manifest(tmux_enabled: bool, drop: Option<&str>) -> Manifest {
        manifest_with(tmux_enabled, drop, "")
    }

    /// 語列の判定の場（rm の segment を持たない command には効かない）。
    fn nowhere() -> Scene<'static> {
        Scene { cwd: Path::new("/nonexistent"), state_dir: Path::new("/nonexistent/state"), git: Path::new("git") }
    }

    /// Bash の判定の (what, line)。Allow なら `None`。
    fn denied(command: &str, manifest: &Manifest) -> Option<(String, String)> {
        match judge("Bash", command, manifest, &nowhere()) {
            HostGuardDecision::Deny { what, line } => Some((what, line)),
            HostGuardDecision::Allow => None,
        }
    }

    /// Bash の payload（command は JSON の escape を通す）。
    fn bash(command: &str) -> String {
        format!("{{\"cwd\":\"/tmp\",\"tool_name\":\"Bash\",\"tool_input\":{{\"command\":{}}}}}", crate::fleet::json_lite::quote(command))
    }

    /// 種類は閉じた 5 値で、const slice は宣言順に 5 本（git → rm → tmux → 台帳 → 自身の設定）。行を持たないのは自身の設定だけ。
    #[test]
    fn host_guard_kind_slice_is_the_five_kinds_in_declaration_order() {
        assert!(is_declaration_order(KINDS, |kind| kind as usize), "KINDS は宣言順: {KINDS:?}");
        let words: Vec<&str> = KINDS.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(words, ["git", "rm", "tmux", "ledger", "settings"], "5 値の語");
        let rows: Vec<Option<&str>> = KINDS.iter().map(|kind| kind.row()).collect();
        assert_eq!(rows, [Some(super::GIT_ROW), Some(RM_ROW), Some(TMUX_ROW), Some(LEDGER_ROW), None], "行 id");
        assert!(!is_declaration_order(&[Kind::Rm, Kind::Git], |kind| kind as usize), "述語は並べ替えを落とす");
    }

    /// 引用符の中に区切りを含む command で 2 つの口の判定が割れる（分割だけが 2 本・照合は同じ 1 関数）: command guard の
    /// 従来の分割は引用符の中の `;` で切って当て、host-guard は引用符を解くので当てない。逆に引用符で包んだ flag は
    /// host-guard だけが解いて当てる。
    #[test]
    fn host_guard_kind_quoted_separator_splits_the_two_mouths() {
        let manifest = manifest(true, None);
        let runner: Vec<String> = RUNNER.iter().map(|item| (*item).to_owned()).collect();
        let quoted = "echo \"x; git push --force origin\"";
        assert!(denied_in(quoted, &runner).is_some(), "command guard の分割は引用符の中の ; で切る");
        assert_eq!(denied(quoted, &manifest), None, "host-guard は引用符の中を 1 語に読む");
        let wrapped = "git push \"--force\" origin main";
        assert!(denied_in(wrapped, &runner).is_none(), "command guard は引用符を解かない");
        let (what, _) = denied(wrapped, &manifest).unwrap_or_else(|| panic!("host-guard は引用符を解いて当てる"));
        assert_eq!(what, "host-guard-deny git");
    }

    /// `NAME=value` の前置きを読み飛ばして当てる（前置きが 2 つでも）。
    #[test]
    fn host_guard_kind_skips_env_assignment_prefix() {
        let manifest = manifest(true, None);
        for (command, what) in [
            ("NAME=1 git push --force origin main", "host-guard-deny git"),
            ("A=1 B=x tmux kill-server", "host-guard-deny tmux"),
            ("cd /tmp && GIT_DIR=x git reset --hard", "host-guard-deny git"),
        ] {
            let found = denied(command, &manifest).map(|(found, _)| found);
            assert_eq!(found.as_deref(), Some(what), "{command}");
        }
    }

    /// git・tmux・台帳の語列がそれぞれ自分の行の id と裁定 id を名指し、断りの 1 行は 5 欄（kind / hit / row / ruling / 代わりの
    /// 経路）を持つ。
    #[test]
    fn host_guard_kind_each_word_kind_names_its_own_row() {
        let manifest = manifest(true, None);
        let (what, line) = denied("git push origin main --force", &manifest).unwrap_or_else(|| panic!("git は断る"));
        assert_eq!(what, "host-guard-deny git");
        assert_eq!(
            line,
            format!("{NAME}: host-guard deny kind=git hit=git push --force row=host_guard.git ruling={RULING} — {}", Kind::Git.route())
        );
        for (command, kind, id) in [("tmux -L x kill-server", Kind::Tmux, TMUX_ROW), ("bd delete s2-1", Kind::Ledger, LEDGER_ROW)] {
            let (what, line) = denied(command, &manifest).unwrap_or_else(|| panic!("{command} は断る"));
            assert_eq!(what, format!("host-guard-deny {}", kind.as_str()), "{command}");
            assert!(line.contains(&format!(" kind={} ", kind.as_str())) && line.contains(&format!(" row={id} ")), "{line}");
            assert!(line.contains(&format!(" ruling={RULING} — ")) && line.ends_with(kind.route()), "{line}");
            assert_eq!(line.lines().count(), 1, "1 行: {line}");
        }
        assert_eq!(denied("git push --force-with-lease origin x", &manifest), None, "語が違う flag は通す");
    }

    /// host_guard.tmux の `enabled = false` で、その行にだけ在る語列を host-guard は通す（他の種類は動く・同じ manifest で
    /// 行が発効なら断る＝切ったことだけが効く）。git の語列は runner.denied_commands と重複するので enabled の性質は tmux の
    /// 行で測る。
    #[test]
    fn host_guard_kind_disabled_tmux_row_passes_host_guard() {
        let off = manifest(false, None);
        assert_eq!(denied("tmux kill-server", &off), None, "切った行の語列は host-guard が通す");
        assert!(denied("git push --force", &off).is_some(), "他の種類は動く");
        let on = denied("tmux kill-server", &manifest(true, None)).map(|(what, _)| what);
        assert_eq!(on.as_deref(), Some("host-guard-deny tmux"), "発効の行なら断る");
    }

    /// 同じ `enabled = false` の fixture で、command guard の `denied_of` は host_guard.tmux の語列を行 id つきで返し、`judge`
    /// は断って行 id host_guard.tmux を名指す（enabled を見ない・disabled の行を捨てる変異で赤）。
    #[test]
    fn host_guard_kind_disabled_tmux_row_is_still_read_by_command_guard() {
        let manifest = manifest(false, None);
        let sources = command::denied_of(&manifest).unwrap_or_else(|| panic!("行が揃う"));
        assert!(sources.contains(&(TMUX_ROW, vec!["tmux kill-server".to_owned()])), "{sources:?}");
        let CommandDecision::Deny { what, line } = command::judge("tmux kill-server", &manifest) else {
            panic!("command guard は enabled を見ずに断る");
        };
        assert_eq!(what, "tmux kill-server");
        assert!(line.contains(&format!("rules 行 {TMUX_ROW} が禁じる")), "{line}");
    }

    /// 語列の行が無い manifest は、その行の種類の語で断る（FailClosed・当たらない command でも）。空の manifest は宣言順で
    /// 先の git の種類で断る。
    #[test]
    fn host_guard_kind_missing_row_fails_closed_with_the_kind_word() {
        for (id, kind) in WORD_ROWS.iter().zip(["git", "tmux", "ledger"]) {
            let (what, line) = denied("ls", &manifest(true, Some(*id))).unwrap_or_else(|| panic!("{id} が無い周は断る"));
            assert_eq!(what, format!("host-guard-deny {kind}"), "{id}");
            assert!(line.contains(&format!(" hit=no-row row={id} ruling=- — ")), "{line}");
        }
        let empty = Manifest::parse("schema = 1\n").unwrap_or_else(|errors| panic!("{errors:?}"));
        assert_eq!(denied("ls", &empty).map(|(what, _)| what).as_deref(), Some("host-guard-deny git"));
        assert_eq!(judge("Edit", "", &empty, &nowhere()), HostGuardDecision::Allow, "編集系には語列の種類が掛からない");
    }

    /// host_guard.tmux にだけ在る語列を command guard が断り、deny 文が行 id host_guard.tmux を名指す（runner の id を固定で
    /// 書かない）。runner と重複する語列は行の並びで先の runner.denied_commands を名指す。
    #[test]
    fn host_guard_kind_command_guard_names_the_tmux_row() {
        let manifest = manifest(true, None);
        let CommandDecision::Deny { what, line } = command::judge("tmux kill-server", &manifest) else {
            panic!("tmux の行の語列を command guard が断る");
        };
        assert_eq!(what, "tmux kill-server", "記録の what は当たった語列");
        assert!(line.starts_with(&format!("{NAME}: deny tmux kill-server は rules 行 {TMUX_ROW} が禁じる")), "{line}");
        assert!(!line.contains(command::ROW), "runner の id を名乗らない: {line}");
        let CommandDecision::Deny { line, .. } = command::judge("bd delete x", &manifest) else {
            panic!("台帳の行の語列も断る");
        };
        assert!(line.contains(&format!("rules 行 {LEDGER_ROW} が禁じる")), "{line}");
        let CommandDecision::Deny { line, .. } = command::judge("git push --force", &manifest) else {
            panic!("runner の語列は断る");
        };
        assert!(line.contains(&format!("rules 行 {} が禁じる", command::ROW)), "{line}");
    }

    /// runner.denied_commands は在るが host_guard の 1 行を欠く manifest では、command guard は当たらない command も
    /// `no-row <欠けた行の id>` で断る（`denied_of` は `None`・欠けた行を読み飛ばす変異で赤）。
    #[test]
    fn host_guard_kind_command_guard_fails_closed_without_a_host_row() {
        for id in WORD_ROWS {
            let manifest = manifest(true, Some(id));
            assert_eq!(command::denied_of(&manifest), None, "{id} を欠く周は揃わない");
            let CommandDecision::Deny { what, line } = command::judge("ls", &manifest) else {
                panic!("{id} が無い周は当たらない command も断る");
            };
            assert_eq!(what, format!("reason=no-row {id}"));
            assert!(line.contains(&format!("rules 行 {id} を読めない")), "{line}");
        }
        assert_eq!(command::judge("ls", &manifest(true, None)), CommandDecision::Allow, "揃えば当たらない command は通す");
    }

    /// git と tmux の語列を両方含む 1 command は宣言順で先の git だけを断り、deny は 1 行。
    #[test]
    fn host_guard_kind_first_kind_in_declaration_order_wins() {
        let manifest = manifest(true, None);
        for command in ["tmux kill-server; git push --force origin main", "git push --force && tmux kill-server"] {
            let (what, line) = denied(command, &manifest).unwrap_or_else(|| panic!("{command} は断る"));
            assert_eq!(what, "host-guard-deny git", "{command}");
            assert!(!line.contains("kind=tmux") && line.lines().count() == 1, "{line}");
        }
    }

    /// 置き場の実体が無い state dir で判定する（埋め込みか `--rules` の rules）。
    fn decided(payload: &str, rules: Option<&str>) -> Result<HostGuardDecision, Unreadable> {
        decide(payload, rules, Path::new("/nonexistent/state"))
    }

    /// 台帳の形・見張り自身の設定の 2 arm はまだ Allow: bdw を経ない台帳の write・settings.json への Write が通る（実体の
    /// 無い path の rm も通る＝rm の判定は行 c の `host_guard_rm_` の歯）。
    #[test]
    fn host_guard_kind_rm_ledger_arm_and_settings_arms_allow_in_row_b() {
        for command in ["rm -rf /tmp/state-dir", "bd close s2-1", "bd update s2-1 --notes x"] {
            assert_eq!(decided(&bash(command), None), Ok(HostGuardDecision::Allow), "{command}");
        }
        let write = "{\"cwd\":\"/tmp\",\"tool_name\":\"Write\",\"tool_input\":{\"file_path\":\"/tmp/acct/settings.json\"}}";
        assert_eq!(decided(write, None), Ok(HostGuardDecision::Allow), "settings.json への Write");
        assert!(matches!(decided(&bash("git push --force"), None), Ok(HostGuardDecision::Deny { .. })), "埋め込みの行で git は断る");
    }

    /// payload が JSON でない・tool_name が無い・Bash の command が無い・rules が読めない周は理由つきで断り、判定に載らない
    /// tool は rules を読まずに通す。
    #[test]
    fn host_guard_kind_unreadable_payload_fails_closed() {
        assert_eq!(decided("not json", None), Err(Unreadable::PayloadUnreadable));
        assert_eq!(decided("{\"cwd\":\"/tmp\"}", None), Err(Unreadable::NoToolName));
        assert_eq!(decided("{\"tool_name\":\"Bash\",\"tool_input\":{}}", None), Err(Unreadable::NoCommand));
        assert_eq!(decided(&bash("ls"), Some("/nonexistent/rules.toml")), Err(Unreadable::RulesUnreadable));
        assert_eq!(decided("{\"tool_name\":\"Read\"}", Some("/nonexistent/rules.toml")), Ok(HostGuardDecision::Allow));
    }

    // ─── rm の種類（行 c・接頭辞 `host_guard_rm_`） ───

    /// 守る集合の 3 記号を持つ rm の行（値は引数・`enabled` も引数）。
    fn rm_row(symbols: &[&str], enabled: bool) -> String {
        row(RM_ROW, "HostGuardRmProtected", symbols, enabled)
    }

    /// 語列の 3 行と rm の行（3 記号・発効）を持つ manifest。
    fn rm_manifest() -> Manifest {
        manifest_with(true, None, &rm_row(&["state-dir", "repo-tracked", "repo-git"], true))
    }

    /// 歯の置き場: `base/state`（host.toml と accounts）・`base/work/repo`（`src/lib.rs` だけ tracked・`notes.txt` と
    /// `.gitkeep` と `build/out.o` は untracked）・`base/work/other`（repo の外・file `x`）。
    struct Place {
        base: PathBuf,
        state: PathBuf,
        repo: PathBuf,
        other: PathBuf,
    }

    impl Place {
        /// 歯ごとに作り直す（名は歯ごとに一意・pid つき）。
        fn new(name: &str) -> Self {
            let base = std::env::temp_dir().join(format!("host-guard-rm-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&base);
            let _ = fs::create_dir_all(&base);
            let base = fs::canonicalize(&base).unwrap_or(base);
            let (state, repo, other) = (base.join("state"), base.join("work/repo"), base.join("work/other"));
            for dir in [state.join("accounts"), repo.join("src"), repo.join("build"), other.clone()] {
                let _ = fs::create_dir_all(dir);
            }
            for file in ["state/host.toml", "work/repo/src/lib.rs", "work/repo/notes.txt", "work/repo/.gitkeep"] {
                let _ = fs::write(base.join(file), "x\n");
            }
            let _ = fs::write(repo.join("build/out.o"), "x\n");
            let _ = fs::write(other.join("x"), "x\n");
            git(&repo, &["init", "-q"]);
            git(&repo, &["add", "src/lib.rs"]);
            Self { base, state, repo, other }
        }

        /// 本物の git で判定し、断る周の hit（` hit=` と ` row=` の間）を返す。通す周は `None`。
        fn hit(&self, command: &str, cwd: &Path) -> Option<String> {
            self.hit_in(command, cwd, &rm_manifest(), Path::new("git"))
        }

        /// manifest と git の program を指定して判定する。
        fn hit_in(&self, command: &str, cwd: &Path, manifest: &Manifest, git: &Path) -> Option<String> {
            let scene = Scene { cwd, state_dir: &self.state, git };
            match judge("Bash", command, manifest, &scene) {
                HostGuardDecision::Deny { line, .. } => Some(line.split(" hit=").nth(1)?.split(" row=").next()?.to_owned()),
                HostGuardDecision::Allow => None,
            }
        }

        /// 置き場を片付ける。
        fn clean(self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    /// git を 1 回撃つ（失敗は読み手の assert が落とす）。
    fn git(dir: &Path, args: &[&str]) {
        let _ = Command::new("git").arg("-C").arg(dir).args(args).output();
    }

    /// 記号は閉じた 3 値の宣言順で、字面から引け、rules 行の値の綴り違いは読み込みで拒み 3 記号は受理される。
    #[test]
    fn host_guard_rm_symbols_are_three_in_declaration_order() {
        assert!(is_declaration_order(PROTECTED, |symbol| symbol as usize), "PROTECTED は宣言順: {PROTECTED:?}");
        let words: Vec<&str> = PROTECTED.iter().map(|symbol| symbol.as_str()).collect();
        assert_eq!(words, ["state-dir", "repo-tracked", "repo-git"], "3 記号の字面");
        assert!(PROTECTED.iter().all(|symbol| Protected::parse(symbol.as_str()) == Some(*symbol)), "字面から引ける");
        assert_eq!(Protected::parse("state_dir"), None, "綴り違いは引けない");
        let _accepted = rm_manifest();
        let text = format!("schema = 1\n{}", rm_row(&["state-dir", "repo_git"], true));
        let errors = Manifest::parse(&text).err().unwrap_or_default();
        assert!(errors.iter().any(|error| error.message.contains("未知の守る集合の記号 repo_git")), "{errors:?}");
    }

    /// launcher（sudo・env・timeout・command・exec・nice・`\` の逃がし）を剥いだ先頭語の basename が rm の segment だけを
    /// 判定し、rmdir・git rm・rm を引数に持つだけの command は通す。
    #[test]
    fn host_guard_rm_verb_is_identified_after_launchers() {
        let place = Place::new("verb");
        let target = place.state.join("host.toml").display().to_string();
        for head in [
            "/bin/rm", "\\rm", "sudo rm", "sudo -u root rm", "env X=1 rm", "env -u Y rm", "timeout 5 rm", "timeout -s KILL 5 rm",
            "command rm", "exec rm", "nice -n 5 rm", "sudo -- env rm", "ls && rm", "sudo -E rm", "env - rm", "nice -5 rm",
        ] {
            let hit = place.hit(&format!("{head} {target}"), &place.other);
            assert_eq!(hit, Some(format!("state-dir:{target}")), "{head}");
        }
        for head in ["rmdir", "git rm", "echo rm", "timeout rm"] {
            assert_eq!(place.hit(&format!("{head} {target}"), &place.other), None, "{head} は rm でない");
        }
        place.clean();
    }

    /// `-` で始まる語は flag として読み飛ばし、`--` の後ろは全部 path（`-f` という名の file が守る dir に在っても flag は
    /// path に読まない）。
    #[test]
    fn host_guard_rm_flags_are_skipped_and_double_dash_ends_them() {
        let place = Place::new("flags");
        let _ = fs::write(place.state.join("-f"), "x\n");
        let _ = fs::write(place.state.join("-"), "x\n");
        assert_eq!(place.hit("rm -f nope", &place.state), None, "flag は path でない");
        let want = format!("state-dir:{}", place.state.join("-f").display());
        assert_eq!(place.hit("rm -- -f", &place.state), Some(want.clone()), "-- の後ろは path");
        assert_eq!(place.hit("rm -r -- -f --", &place.state), Some(want), "2 つ目の -- も path");
        let dash = format!("state-dir:{}", place.state.join("-").display());
        assert_eq!(place.hit("rm -", &place.state), Some(dash), "- 1 字は path");
        place.clean();
    }

    /// 実体の無い path は守る dir の配下の字面でも通す。
    #[test]
    fn host_guard_rm_missing_path_passes() {
        let place = Place::new("missing");
        for path in [place.state.join("nope"), place.repo.join("src/nope.rs"), place.repo.join(".git/nope")] {
            assert_eq!(place.hit(&format!("rm -rf {}", path.display()), &place.other), None, "{}", path.display());
        }
        assert!(place.hit(&format!("rm {}", place.state.join("host.toml").display()), &place.other).is_some(), "在れば断る");
        place.clean();
    }

    /// 相対 path は payload の cwd を基準に `.` と `..` を畳む（cwd が repo の root なら `.` も断られる）。
    #[test]
    fn host_guard_rm_relative_path_folds_against_cwd() {
        let place = Place::new("fold");
        let (src, repo) = (place.repo.join("src"), place.repo.display().to_string());
        assert_eq!(place.hit("rm ../src/lib.rs", &src), Some(format!("repo-tracked:{repo}/src/lib.rs")), "..");
        assert_eq!(place.hit("rm ../notes.txt", &src), None, "untracked は通る");
        assert_eq!(place.hit("rm -rf .", &place.repo), Some(format!("repo-tracked:{repo}")), "root の .");
        assert_eq!(place.hit("rm -rf build/..", &place.repo), Some(format!("repo-tracked:{repo}")), "畳むと root");
        assert_eq!(place.hit("rm -rf ./build", &place.repo), None, "untracked の dir");
        place.clean();
    }

    /// `$` の変数展開は解かずに断り（引用符を解いた語が hit）、同じ字面でも rm でない先頭語は通す。
    #[test]
    fn host_guard_rm_unresolved_variable_is_denied() {
        let place = Place::new("variable");
        assert_eq!(place.hit("rm \"$X\"", &place.other), Some("unresolved:$X".to_owned()));
        assert_eq!(place.hit("rm -rf ${HOME}", &place.other), Some("unresolved:${HOME}".to_owned()));
        assert_eq!(place.hit("echo \"$X\"", &place.other), None, "rm でない");
        place.clean();
    }

    /// `` ` `` と `$(` の command 置換は解かずに断り、同じ字面でも rm でない先頭語は通す。
    #[test]
    fn host_guard_rm_unresolved_command_substitution_is_denied() {
        let place = Place::new("substitution");
        assert_eq!(place.hit("rm `cat list`", &place.other), Some("unresolved:`cat".to_owned()));
        assert_eq!(place.hit("rm $(cat list)", &place.other), Some("unresolved:$(cat".to_owned()));
        assert_eq!(place.hit("echo `cat list`", &place.other), None, "rm でない");
        place.clean();
    }

    /// `~` 始まりの path は HOME を読まずに断り、同じ字面でも rm でない先頭語は通す（home の短縮の字面は paths-clean が
    /// 数えるので `concat!` で組む）。
    #[test]
    fn host_guard_rm_unresolved_tilde_is_denied() {
        const HOME_X: &str = concat!("~", "/x");
        let place = Place::new("tilde");
        assert_eq!(place.hit(&format!("rm -rf {HOME_X}"), &place.other), Some(format!("unresolved:{HOME_X}")));
        assert_eq!(place.hit("rm ~other/x", &place.other), Some("unresolved:~other/x".to_owned()));
        assert_eq!(place.hit(&format!("ls {HOME_X}"), &place.other), None, "rm でない");
        assert_eq!(place.hit("rm x~", &place.other), None, "~ で始まらない語は解く（実体が無い）");
        place.clean();
    }

    /// brace の `{` `}` を含む語は展開せずに断り、同じ字面でも rm でない先頭語は通す。
    #[test]
    fn host_guard_rm_unresolved_brace_is_denied() {
        let place = Place::new("brace");
        assert_eq!(place.hit("rm {a,b}.txt", &place.other), Some("unresolved:{a,b}.txt".to_owned()));
        assert_eq!(place.hit("rm a}", &place.other), Some("unresolved:a}".to_owned()));
        assert_eq!(place.hit("echo {a,b}.txt", &place.other), None, "rm でない");
        place.clean();
    }

    /// 同じ command の cd / pushd より後ろの segment の相対 path は解かずに断り、絶対 path は解いて判定する（cd より前の
    /// rm と、cwd が絶対 path でない周の相対 path も同じ向き）。
    #[test]
    fn host_guard_rm_unresolved_relative_after_cd_is_denied() {
        let place = Place::new("cd");
        assert_eq!(place.hit("cd sub && rm x", &place.other), Some("unresolved:x".to_owned()), "cd");
        assert_eq!(place.hit("pushd sub; rm -f y", &place.other), Some("unresolved:y".to_owned()), "pushd");
        assert_eq!(place.hit("cd sub && ls x", &place.other), None, "rm でない");
        assert_eq!(place.hit("rm nope && cd sub", &place.other), None, "cd より前の rm は解く");
        let tracked = place.repo.join("src/lib.rs").display().to_string();
        let outside = place.other.join("x").display().to_string();
        assert_eq!(place.hit(&format!("cd sub && rm {outside}"), &place.other), None, "cd の後ろの絶対 path は解く");
        assert_eq!(place.hit(&format!("cd / && rm {tracked}"), &place.repo), Some(format!("repo-tracked:{tracked}")));
        assert_eq!(place.hit("rm x", Path::new("")), Some("unresolved:x".to_owned()), "cwd が解けない周");
        place.clean();
    }

    /// glob の語は glob の字より前の literal な接頭の dir を解き、守る path と当たる関係に在れば断る（一時 dir の下と
    /// untracked の dir の下は通り、repo の root の配下と state dir の下は断る）。
    #[test]
    fn host_guard_rm_glob_is_judged_by_its_literal_prefix_dir() {
        let place = Place::new("glob");
        let repo = place.repo.display().to_string();
        assert_eq!(place.hit("rm *.bak", &place.other), None, "一時 dir の下");
        assert_eq!(place.hit("rm -f build/*.o", &place.repo), None, "untracked の dir の下");
        assert_eq!(place.hit("rm *.bak", &place.repo), Some(format!("repo-tracked:{repo}/*.bak")), "root の配下");
        assert_eq!(place.hit("rm src/l?b.rs", &place.repo), Some(format!("repo-tracked:{repo}/src/l?b.rs")), "?");
        assert_eq!(place.hit("rm bu[i]ld/x", &place.repo), Some(format!("repo-tracked:{repo}/bu[i]ld/x")), "[");
        let state = format!("{}/*", place.state.display());
        assert_eq!(place.hit(&format!("rm -rf {state}"), &place.other), Some(format!("state-dir:{state}")), "state dir");
        place.clean();
    }

    /// `rm <path>` を cwd で判定した hit（path は絶対 path）。
    fn hit_at(place: &Place, path: &Path, cwd: &Path) -> Option<String> {
        place.hit(&format!("rm -rf {}", path.display()), cwd)
    }

    /// 一致: state dir そのものは state-dir に当たり、兄弟の dir は通る。
    #[test]
    fn host_guard_rm_state_dir_itself_is_hit() {
        let place = Place::new("state-itself");
        let want = format!("state-dir:{}", place.state.display());
        assert_eq!(hit_at(&place, &place.state, &place.other), Some(want), "state dir そのもの");
        assert_eq!(hit_at(&place, &place.other, &place.other), None, "兄弟の dir");
        place.clean();
    }

    /// 配下: state dir の下の host.toml と accounts は state-dir に当たり、state dir の外の file は通る。
    #[test]
    fn host_guard_rm_state_dir_children_are_hit() {
        let place = Place::new("state-children");
        for child in ["host.toml", "accounts"] {
            let path = place.state.join(child);
            assert_eq!(hit_at(&place, &path, &place.other), Some(format!("state-dir:{}", path.display())), "{child}");
        }
        assert_eq!(hit_at(&place, &place.other.join("x"), &place.other), None, "外の file");
        place.clean();
    }

    /// 祖先: state dir の親 dir は state-dir に当たり、祖先でない兄弟 dir は通る。
    #[test]
    fn host_guard_rm_state_dir_parent_is_hit() {
        let place = Place::new("state-parent");
        let want = format!("state-dir:{}", place.base.display());
        assert_eq!(hit_at(&place, &place.base, &place.other), Some(want), "親 dir");
        assert_eq!(hit_at(&place, &place.other, &place.other), None, "祖先でない兄弟 dir");
        place.clean();
    }

    /// 一致: tracked file は repo-tracked に当たり、untracked の file は通る。
    #[test]
    fn host_guard_rm_tracked_file_is_hit() {
        let place = Place::new("tracked");
        let path = place.repo.join("src/lib.rs");
        assert_eq!(place.hit("rm src/lib.rs", &place.repo), Some(format!("repo-tracked:{}", path.display())), "tracked");
        assert_eq!(place.hit("rm notes.txt", &place.repo), None, "untracked");
        place.clean();
    }

    /// 祖先: tracked file を配下に持つ dir は repo-tracked に当たり、untracked だけの dir は通る。
    #[test]
    fn host_guard_rm_dir_holding_tracked_is_hit() {
        let place = Place::new("holding");
        let want = format!("repo-tracked:{}", place.repo.join("src").display());
        assert_eq!(place.hit("rm -rf src", &place.repo), Some(want), "tracked を持つ dir");
        assert_eq!(place.hit("rm -rf build", &place.repo), None, "untracked だけの dir");
        place.clean();
    }

    /// 配下: `.git` の下は repo-git に当たり、名が `.git` で始まるだけの兄弟 file は通る。
    #[test]
    fn host_guard_rm_git_dir_children_are_hit() {
        let place = Place::new("git-children");
        let want = format!("repo-git:{}", place.repo.join(".git/index").display());
        assert_eq!(place.hit("rm .git/index", &place.repo), Some(want), ".git の配下");
        assert_eq!(place.hit("rm .gitkeep", &place.repo), None, "兄弟の file");
        place.clean();
    }

    /// 祖先: repo の root の親 dir は当たり（tracked の祖先）、祖先でない兄弟 dir は通る。
    #[test]
    fn host_guard_rm_repo_root_parent_is_hit() {
        let place = Place::new("root-parent");
        let work = place.base.join("work");
        assert_eq!(hit_at(&place, &work, &place.repo), Some(format!("repo-tracked:{}", work.display())), "root の親");
        assert_eq!(hit_at(&place, &place.other, &place.repo), None, "祖先でない兄弟 dir");
        place.clean();
    }

    /// worktree: root は `.git` の file でも fs で解け、その file と `gitdir:` が指す本体の common dir が repo-git に当たる
    /// （worktree の cwd から本体の親を消す rm は当たる）。本体と関係の無い dir は通る。
    #[test]
    fn host_guard_rm_worktree_git_file_and_common_dir_are_hit() {
        let place = Place::new("worktree");
        let config = ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"];
        git(&place.repo, &[&config[..], &["commit", "-q", "-m", "seed"]].concat());
        let tree = place.base.join("tree");
        git(&place.repo, &["worktree", "add", "-q", &tree.display().to_string()]);
        let _ = fs::create_dir_all(place.base.join("spare"));
        let dot = format!("repo-git:{}", tree.join(".git").display());
        assert_eq!(place.hit("rm .git", &tree), Some(dot), "worktree の .git の file");
        let lib = format!("repo-tracked:{}", tree.join("src/lib.rs").display());
        assert_eq!(place.hit("rm src/lib.rs", &tree), Some(lib), "root は .git の file から解ける");
        let work = place.base.join("work");
        assert_eq!(hit_at(&place, &work, &tree), Some(format!("repo-git:{}", work.display())), "本体の親");
        let objects = place.repo.join(".git/objects");
        let want = format!("repo-git:{}", objects.display());
        assert_eq!(hit_at(&place, &objects, &tree), Some(want), "common dir の配下（gitdir の外）");
        assert_eq!(hit_at(&place, &place.base.join("spare"), &tree), None, "関係の無い dir");
        place.clean();
    }

    /// 守る file を指す symlink の rm は link 自身の path で見て通り、link の親が守る dir なら当たり、state dir を指す
    /// dir の link を経た path は realpath で当たる（末尾 `/` は link の先を指す）。
    #[test]
    fn host_guard_rm_symlink_is_judged_by_the_link_itself() {
        let place = Place::new("symlink");
        let _ = std::os::unix::fs::symlink(place.state.join("host.toml"), place.other.join("link"));
        let _ = std::os::unix::fs::symlink(place.other.join("x"), place.state.join("inner"));
        let _ = std::os::unix::fs::symlink(&place.state, place.other.join("st"));
        assert_eq!(place.hit("rm link", &place.other), None, "守る file を指す link");
        let inner = format!("state-dir:{}", place.state.join("inner").display());
        assert_eq!(hit_at(&place, &place.state.join("inner"), &place.other), Some(inner), "親が守る dir");
        let via = format!("state-dir:{}", place.other.join("st/accounts").display());
        assert_eq!(place.hit("rm -rf st/accounts", &place.other), Some(via), "realpath で当たる");
        assert_eq!(place.hit("rm st", &place.other), None, "dir の link そのもの");
        assert!(place.hit("rm -rf st/", &place.other).is_some(), "末尾 / は link の先");
        let link = place.other.join("st");
        let scene = Scene { cwd: &place.other, state_dir: &link, git: Path::new("git") };
        let command = format!("rm {}", place.state.join("host.toml").display());
        let found = judge("Bash", &command, &rm_manifest(), &scene);
        assert!(matches!(found, HostGuardDecision::Deny { .. }), "link で渡した state dir も実体で守る: {found:?}");
        place.clean();
    }

    /// git の子 process は rm の segment が在り root が解けた周の `git ls-files` 1 回だけ（偽の git で数える）。rm の
    /// segment が無い周・root が解けない周・repo-tracked を値に持たない周は 0 回で、root が解けない周は repo の 2 記号が空。
    #[test]
    fn host_guard_rm_runs_git_once_only_with_rm_and_root() {
        use std::os::unix::fs::PermissionsExt;
        let place = Place::new("git-once");
        let (fake, count) = (place.base.join("fake-git"), place.base.join("count"));
        let _ = fs::write(&fake, format!("#!/bin/sh\necho \"$*\" >> '{}'\nexec git \"$@\"\n", count.display()));
        let _ = fs::set_permissions(&fake, fs::Permissions::from_mode(0o755));
        let calls = || fs::read_to_string(&count).unwrap_or_default().lines().map(str::to_owned).collect::<Vec<_>>();
        let hit = place.hit_in("rm notes.txt; rm -f build/out.o && rm src/lib.rs", &place.repo, &rm_manifest(), &fake);
        assert_eq!(hit.as_deref().map(|found| found.starts_with("repo-tracked:")), Some(true), "{hit:?}");
        assert_eq!(calls(), ["-C ".to_owned() + &place.repo.display().to_string() + " ls-files -z"], "1 回");
        for (command, cwd) in [("ls src", &place.repo), ("git status", &place.repo), ("rm x", &place.other)] {
            assert_eq!(place.hit_in(command, cwd, &rm_manifest(), &fake), None, "{command}");
        }
        let state_only = manifest_with(true, None, &rm_row(&["state-dir", "repo-git"], true));
        assert!(place.hit_in("rm .git/index", &place.repo, &state_only, &fake).is_some(), "repo-git は git を撃たない");
        assert_eq!(calls().len(), 1, "増えない: {:?}", calls());
        let broken = place.base.join("broken-git");
        let _ = fs::write(&broken, "#!/bin/sh\nexit 1\n");
        let _ = fs::set_permissions(&broken, fs::Permissions::from_mode(0o755));
        let hit = place.hit_in("rm notes.txt", &place.repo, &rm_manifest(), &broken);
        let want = format!("repo-tracked:{}", place.repo.join("notes.txt").display());
        assert_eq!(hit, Some(want), "ls-files を読めない周は root 全体を守る");
        place.clean();
    }

    /// 行の値に無い記号は守らない（state-dir を外した値で state dir の配下の rm が通り、他の記号は動く）。
    #[test]
    fn host_guard_rm_symbol_missing_from_value_is_not_guarded() {
        let place = Place::new("value");
        let without = manifest_with(true, None, &rm_row(&["repo-tracked", "repo-git"], true));
        let command = format!("rm {}", place.state.join("host.toml").display());
        assert_eq!(place.hit_in(&command, &place.other, &without, Path::new("git")), None, "値に無い state-dir");
        assert!(place.hit(&command, &place.other).is_some(), "値に在れば断る");
        assert!(place.hit_in("rm src/lib.rs", &place.repo, &without, Path::new("git")).is_some(), "他の記号は動く");
        place.clean();
    }

    /// 行が無い・列でない周は rm の segment だけを `no-row` で断り（rm でない command は通る）、`enabled = false` は rm の
    /// 種類だけを切る（git の語列は動く）。
    #[test]
    fn host_guard_rm_missing_row_fails_closed_and_disabled_row_passes() {
        let place = Place::new("row");
        let target = format!("rm {}", place.state.join("host.toml").display());
        let not_list = "\n[[rule]]\nid = \"host_guard.rm\"\nkind = \"CoreLines\"\nvalue = 1\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n";
        for fixture in [manifest(true, None), manifest_with(true, None, not_list)] {
            let scene = Scene { cwd: &place.other, state_dir: &place.state, git: Path::new("git") };
            let HostGuardDecision::Deny { what, line } = judge("Bash", "rm nope", &fixture, &scene) else {
                panic!("行が読めない周は rm を断る");
            };
            assert_eq!(what, "host-guard-deny rm");
            assert!(line.contains(" kind=rm hit=no-row row=host_guard.rm ruling=- — "), "{line}");
            assert_eq!(judge("Bash", "ls", &fixture, &scene), HostGuardDecision::Allow, "rm でない command は通る");
        }
        let off = manifest_with(true, None, &rm_row(&["state-dir", "repo-tracked", "repo-git"], false));
        assert_eq!(place.hit_in(&target, &place.other, &off, Path::new("git")), None, "切った rm の種類");
        let git = place.hit_in("git push --force", &place.other, &off, Path::new("git"));
        assert_eq!(git.as_deref(), Some("git push --force"), "他の種類は動く");
        place.clean();
    }
}
