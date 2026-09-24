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
//! rm を断る。台帳の形（行 f）は payload の cwd の repo が台帳（`.beads` と `scripts/bdw`）を持つ周だけ、bd / bdw の
//! segment を起票の門と同じ 3 関数（[`write_of`] → [`ledger_guard::forms_of`] → [`judge_write`]）に掛ける。見張り自身の設定は行 e が
//! 同じ関数の中身を埋める（まだ当たらない）。
//!
//! payload が JSON でない・`tool_name` が無い・`Bash` なのに command が無い・`--state-dir` が無い・rules が読めない周は
//! **deny**（FailClosed・[`POLARITY`]）。Bash / 編集系でない tool は 1 byte も書かず rc 0。断る周だけ `inject.jsonl` に
//! 1 行を残す（tmux は撃たない・席は null）。env も HOME も読まない（C2.2）。git の子 process は rm の segment が在り
//! repo の root が解けた周の `git ls-files` 1 回だけ（NFR5）。

use super::command::{matched, BASH};
use super::ledger_guard::{self, is_assignment, judge_write, segments, write_of, Write, WRITES};
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
/// 台帳を持つ repo の印（root の dir）。
const BEADS: &str = ".beads";
/// 台帳の script（root からの相対 path の file・印の対）。
const BDW: &str = "scripts/bdw";
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
    /// 台帳の破壊と素 write（語列の後ろに形の判定・行 f）。
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
        Kind::Git | Kind::Tmux => sequences(kind, subject, manifest),
        Kind::Ledger => sequences(kind, subject, manifest).or_else(|| writes(kind, subject, manifest)),
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

/// 台帳の形の判定（語列の後ろ）: bd / bdw の segment を起票の門と同じ 3 関数に掛ける。掛かるのは payload の cwd の repo の
/// root（rm と同じ fs の辿り）が [`BEADS`] の dir と [`BDW`] の file を両方持つ周だけ。host_guard.ledger の `enabled = false`
/// は語列と形の両方を切る（行が無い・列でない周は語列の側が先に断った）。`ledger.denied_writes` が無い・不発効・列でない
/// 周は、書き込みの subcommand（[`WRITES`]）の segment を全部断る（FailClosed・読みの subcommand は通す）。
fn writes(kind: Kind, subject: &Subject, manifest: &Manifest) -> Option<Refusal> {
    let found: Vec<Write> = subject.segments.iter().filter_map(|words| write_of(words)).collect();
    if found.is_empty() {
        return None;
    }
    manifest.get(LEDGER_ROW).filter(|row| row.enabled)?;
    let root = root_of(subject.scene.cwd)?;
    if !(root.join(BEADS).is_dir() && root.join(BDW).is_file()) {
        return None;
    }
    match ledger_guard::forms_of(manifest) {
        Ok(forms) => {
            let ruling = manifest.get(ledger_guard::ROW).map_or_else(|| "-".to_owned(), |row| row.ruling.clone());
            let form = found.iter().find_map(|write| judge_write(write, &forms))?;
            Some(Refusal { kind, hit: form.as_str().to_owned(), row: ledger_guard::ROW, ruling })
        }
        Err(_) => found
            .iter()
            .any(|write| WRITES.contains(&write.subcommand.as_str()))
            .then(|| Refusal::no_row(kind, ledger_guard::ROW)),
    }
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
#[path = "host_guard_tests.rs"]
mod tests;
// flip-check: moved s2-07l.592
