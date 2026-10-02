//! 走っている便の契約の行の字を、編集と commit の時点で止める門（設計 vessel-hook.md §15・契約表の行 i・FR47 / FR53 /
//! FR32・NFR5）。
//!
//! 便は受付の時に設計 doc の行を写し取り、その写しで最後まで走る。live な便の行を書き換える編集と commit を、hook の
//! 権能 guard の後ろの 1 段で断る。比べは契約表の parser（[`read_table`]）で欄ごとに行う pure な 1 関数（[`hits`]）で、
//! 字面の diff は使わない（行番号の移動・行の並べ替え・表の外の散文は当たらない）。live な便の列は pipe の側の 1 本
//! （[`live_runs`]）が持ち、生死の判定を 2 本にしない（C2）。
//!
//! git の segment の読み手（[`git_segments`]）も本 module の 1 本で、hook の子 module から呼べる `pub(crate)` に置く
//! （行 h も同じ 1 本を呼ぶ）。

use super::host_guard::verb_of;
use super::ledger_guard::{is_assignment, segments};
use super::vessel;
use crate::fleet::json_tree::{self, Tree};
use crate::fleet::Stage;
use crate::invocation::Invocation;
use crate::name::NAME;
use crate::pipe::cli::{live_runs, LiveRun, Tag};
use crate::pipe::land::MAIN_REF;
use crate::pipe::question_of_run;
use crate::pipe::table::{form_of, promises_of, read_table, ContractRow, PromiseRow, BEGIN, DESIGN_DIR};
use crate::polarity::{OnFailure, Polarity, Timing};
use std::path::{Component, Path, PathBuf};

/// この境界の極性: 編集と commit の時点で止め、event log を読めない周は行が変わる編集と commit を断る。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 門が読む編集系の道具（NotebookEdit は表の doc を書かない）。
const EDIT_TOOLS: [&str; 3] = ["Edit", "Write", "MultiEdit"];
/// Bash の道具名。
const BASH: &str = "Bash";
/// 門が読む git の動詞。
const COMMIT: &str = "commit";
/// 作業 dir を替える動詞。
const CD: [&str; 2] = ["cd", "pushd"];
/// literal でない語の字（変数展開・command 置換・brace・glob）。
const NOT_LITERAL: [char; 7] = ['$', '`', '{', '}', '*', '?', '['];
/// git の対象を別の場所へ向ける env の前置き（対象を解けなくする）。
const TARGET_ENV: [&str; 2] = ["GIT_DIR=", "GIT_WORK_TREE="];

/// 門の判定（閉じた 2 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveRowDecision {
    /// 通す。
    Pass,
    /// 断る（`what` は理由の 1 語・`line` は stderr の 1 行）。
    Deny {
        /// 理由の 1 語（記録の `live-row-deny <what>`）。
        what: String,
        /// stderr の 1 行。
        line: String,
    },
}

/// 当たりの理由（閉じた 3 つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// 欄が変わった（line を除く全欄か、その行を親に持つ約束の行）。
    Changed,
    /// 行が消えた。
    Removed,
    /// 変更後の本文の表が読めない（変更前に在った行を変えていないと示せない）。
    TableUnreadable,
}

impl Reason {
    /// 断りと記録に載せる 1 語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Changed => "changed",
            Self::Removed => "removed",
            Self::TableUnreadable => "table-unreadable",
        }
    }
}

/// 当たり 1 つ（run id・段・行・理由と、止める 1 行の `--repo` に写す便の repo）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hit {
    /// run id。
    pub(crate) run: String,
    /// 便の段。
    pub(crate) stage: Stage,
    /// 当たった行（`<doc>#<行 id>`）。
    pub(crate) row: String,
    /// 理由。
    pub(crate) reason: Reason,
    /// 便の repo。
    repo: Option<PathBuf>,
    /// 便の worktree。
    worktree: Option<PathBuf>,
    /// 行が便自身の行か（名札が行の形で当たった）。
    own: bool,
}

/// 変更前の表の行ごとの変化（行 id と理由・doc 順）。変更前の表が読めない周は空（壊れた表を直す編集を止めない）。
fn changes(doc: &str, before: &str, after: &str) -> Vec<(String, Reason)> {
    let Ok((rows, promises)) = read_table(doc, before) else {
        return Vec::new();
    };
    let after = read_table(doc, after);
    rows.iter()
        .filter_map(|row| {
            let reason = match &after {
                Err(_) => Some(Reason::TableUnreadable),
                Ok((later, later_promises)) => match later.iter().find(|found| found.id == row.id) {
                    None => Some(Reason::Removed),
                    Some(found) => (!same_row(row, found)
                        || promised(&promises, &row.id) != promised(later_promises, &row.id))
                    .then_some(Reason::Changed),
                },
            };
            reason.map(|found| (row.id.clone(), found))
        })
        .collect()
}

/// line を除いて行が同じか。
fn same_row(left: &ContractRow, right: &ContractRow) -> bool {
    ContractRow { line: 0, ..left.clone() } == ContractRow { line: 0, ..right.clone() }
}

/// 行 id を親に持つ約束の行（line を除き、番号の順＝doc 上の並べ替えは当たらない）。
fn promised(promises: &[PromiseRow], id: &str) -> Vec<PromiseRow> {
    let mut found: Vec<PromiseRow> =
        promises_of(promises, id).into_iter().map(|promise| PromiseRow { line: 0, ..promise.clone() }).collect();
    found.sort_by_key(|promise| promise.n);
    found
}

/// 行の比べ（**pure な 1 関数**・設計 §15 形 1）: doc の repo 相対 path・変更前と変更後の本文・live な便の列から当たりを
/// 返す。名札が行の便はその行の変化、doc だけの便はその doc の最初の変化、名札の無い便はどの doc でも最初の変化に当たる。
pub(crate) fn hits(doc: &str, before: &str, after: &str, runs: &[LiveRun]) -> Vec<Hit> {
    hits_merging(doc, before, after, None, runs)
}

/// [`hits`] に merge の相手（MERGE_HEAD）の本文を足した比べ（**pure な 1 関数**・設計 §24 約束 4）: 変化の列のうち、変更後と
/// 相手の本文の両方で表が読め、その行 id の行が両方で同じ変化を名札に当てる前に落とす（doc だけと名札の無い便の「最初の変化」も
/// 落とした後の列から取る）。相手が無い・表が読めない周は [`hits`] と同じ。
pub(crate) fn hits_merging(doc: &str, before: &str, after: &str, merge: Option<&str>, runs: &[LiveRun]) -> Vec<Hit> {
    let found = match merge {
        Some(theirs) => unmerged(doc, changes(doc, before, after), after, theirs),
        None => changes(doc, before, after),
    };
    runs.iter()
        .filter_map(|run| {
            let (change, own) = match &run.tag {
                Tag::Row(pointer) if pointer.path == doc => (found.iter().find(|(id, _)| *id == pointer.id), true),
                Tag::Row(_) => (None, false),
                Tag::Doc(path) => (found.first().filter(|_| path == doc), false),
                Tag::Unread => (found.first(), false),
            };
            change.map(|(id, reason)| Hit {
                run: run.id.clone(),
                stage: run.stage,
                row: format!("{doc}#{id}"),
                reason: *reason,
                repo: run.repo.clone(),
                worktree: run.worktree.clone(),
                own,
            })
        })
        .collect()
}

/// 表を読んだ結果（行と約束の行）。
type Table = (Vec<ContractRow>, Vec<PromiseRow>);

/// 変化の列から、変更後の本文と merge の相手の本文で行が同じものを落とす（どちらかの表が読めない周は列のまま）。
fn unmerged(doc: &str, found: Vec<(String, Reason)>, after: &str, theirs: &str) -> Vec<(String, Reason)> {
    let (Ok(later), Ok(other)) = (read_table(doc, after), read_table(doc, theirs)) else {
        return found;
    };
    found.into_iter().filter(|(id, _)| !same_in(id, &later, &other)).collect()
}

/// 行 id の行（line を除く全欄）とその行を親に持つ約束の行が 2 つの表で同じか（両方に無いも同じ）。
fn same_in(id: &str, (left, left_promises): &Table, (right, right_promises): &Table) -> bool {
    let rows = match (left.iter().find(|row| row.id == id), right.iter().find(|row| row.id == id)) {
        (Some(one), Some(other)) => same_row(one, other),
        (None, None) => true,
        _ => false,
    };
    rows && promised(left_promises, id) == promised(right_promises, id)
}

/// 実装役の自分の行の除外（設計 §15 形 5）: 便の worktree が編集か commit の worktree の root と同じで、行が便自身の行
/// である当たりだけを落とす（同じ worktree から他の live な便の行を変える当たりは残る）。
pub(crate) fn exclude_own(hits: Vec<Hit>, root: &Path) -> Vec<Hit> {
    hits.into_iter().filter(|hit| !(hit.own && hit.worktree.as_deref() == Some(root))).collect()
}

/// git の segment 1 つの読み（動詞・対象の dir・解けたか・動詞の後ろの語）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitSegment {
    /// 大域の option を読み飛ばした最初の語（git の動詞）。
    pub(crate) verb: String,
    /// 対象の dir（解けない周は `--project` の root）。
    pub(crate) dir: PathBuf,
    /// 対象を字面で解けたか。
    pub(crate) resolved: bool,
    /// 動詞の後ろの語（push の引数・設計 §16 形 4）。
    pub(crate) rest: Vec<String>,
}

/// segment 1 つの辿り（前置きの `NAME=value`・launcher を剥いだ先頭の語〔basename〕から後ろの語・作業 dir・解けたか）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Walked {
    /// 前置きの `NAME=value` の語。
    pub(crate) lead: Vec<String>,
    /// 先頭の語（launcher を剥いだ basename）と後ろの語。
    pub(crate) words: Vec<String>,
    /// この segment の作業 dir（前の literal な `cd` / `pushd` の先）。
    pub(crate) dir: PathBuf,
    /// 作業 dir を字面で解けたか。
    pub(crate) resolved: bool,
}

/// segment の読み手（**1 本**・設計 §15 形 4 / §16 形 4）: command 行を [`segments`] で切り、[`verb_of`] で launcher を
/// 剥いで segment ごとの辿りを返す。作業 dir の始まりは payload の `cwd` で、literal な `cd` / `pushd` の先へ替わる。
pub(crate) fn walked(command: &str, cwd: &Path) -> Vec<Walked> {
    let (mut dir, mut resolved) = (cwd.to_path_buf(), true);
    let mut found = Vec::new();
    for words in segments(command) {
        let lead = words.iter().take_while(|word| is_assignment(word)).count();
        let Some((verb, rest)) = verb_of(words.get(lead..).unwrap_or_default()) else {
            continue;
        };
        let head = std::iter::once(verb.to_owned()).chain(rest.iter().cloned()).collect();
        found.push(Walked { lead: words.iter().take(lead).cloned().collect(), words: head, dir: dir.clone(), resolved });
        if CD.contains(&verb) {
            (dir, resolved) = moved(&dir, resolved, rest);
        }
    }
    found
}

/// git の segment の読み手（**1 本**・設計 §15 形 4・行 h も同じ 1 本を呼ぶ）: [`walked`] の上で、先頭の語が git の segment
/// ごとに（git の動詞・対象の dir・解けたか・動詞の後ろの語）を返す。解けない対象は `root` に倒して印を立てる。
pub(crate) fn git_segments(command: &str, cwd: &Path, root: &Path) -> Vec<GitSegment> {
    walked(command, cwd).iter().filter_map(|seg| git_segment(seg, root)).collect()
}

/// 辿った segment 1 つを git の segment として読む（先頭の語が git でない・動詞が無い周は `None`）。
pub(crate) fn git_segment(seg: &Walked, root: &Path) -> Option<GitSegment> {
    let rest = seg.words.split_first().filter(|(head, _)| *head == "git")?.1;
    let redirected = seg.lead.iter().any(|word| TARGET_ENV.iter().any(|env| word.starts_with(env)));
    let (verb, target, ok, after) = git_verb(rest, &seg.dir)?;
    let ok = ok && seg.resolved && !redirected;
    let dir = if ok { target } else { root.to_path_buf() };
    Some(GitSegment { verb, dir, resolved: ok, rest: after })
}

/// 写しの行き先の閉じた 3 形（設計 §25 約束 1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Dest {
    /// literal な位置の語を作業 dir から解いて字面で畳んだ path（絶対 path の語は作業 dir が解けなくても path）。
    Path(PathBuf),
    /// 行き先の語を持たない clone＝作業 dir の直下（作業 dir の path）。
    Under(PathBuf),
    /// 字面で解けない。
    Unresolved,
}

/// 写しを作る segment 1 つ（動詞の語 `worktree-add` / `clone` と行き先）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    /// 動詞の語。
    pub(crate) verb: &'static str,
    /// 行き先。
    pub(crate) to: Dest,
}

/// 動詞ごとの flag の閉じた 2 列（git 2.43 の `-h` の字・値を取る列と取らない列・長い名は `--` を除いた名）。
struct Flags {
    /// 値を取る長い名。
    value_long: &'static [&'static str],
    /// 値を取る 1 字。
    value_short: &'static str,
    /// 値を取らない長い名。
    plain_long: &'static [&'static str],
    /// 値を取らない 1 字。
    plain_short: &'static str,
}

/// `git worktree add` の flag。
const WORKTREE_ADD: Flags = Flags {
    value_long: &["reason"],
    value_short: "bB",
    plain_long: &["force", "orphan", "detach", "checkout", "lock", "quiet", "track", "guess-remote"],
    plain_short: "fdq",
};

/// `git clone` の flag。
const CLONE: Flags = Flags {
    value_long: &[
        "jobs", "template", "reference", "reference-if-able", "origin", "branch", "upload-pack", "depth", "shallow-since",
        "shallow-exclude", "separate-git-dir", "config", "server-option", "filter", "bundle-uri",
    ],
    value_short: "jobuc",
    plain_long: &[
        "verbose", "quiet", "progress", "reject-shallow", "checkout", "bare", "mirror", "local", "hardlinks", "shared",
        "recurse-submodules", "recursive", "dissociate", "single-branch", "tags", "shallow-submodules", "ipv4", "ipv6",
        "also-filter-submodules", "remote-submodules", "sparse",
    ],
    plain_short: "vqnls46",
};

/// `=<値>` の続け書きを持てる値を取らない長い名。
const OPTIONAL_VALUE: [&str; 2] = ["recurse-submodules", "recursive"];

/// 写しの行き先の読み手（**pure な 1 関数**・設計 §25 約束 1）: [`git_segments`] の上で、動詞が `worktree` で次の語が `add` の
/// segment と動詞が `clone` の segment ごとに（動詞の語・行き先）を返す。`-h` / `--help` を持つ segment・他の動詞・頭の語が git
/// でない segment は読まない。
pub(crate) fn copy_places(command: &str, cwd: &Path, root: &Path) -> Vec<Place> {
    git_segments(command, cwd, root).iter().filter_map(place_of).collect()
}

/// git の segment 1 つを写しの segment として読む（写しを作らない・表示だけの segment は `None`）。
fn place_of(seg: &GitSegment) -> Option<Place> {
    let (verb, flags, nth, words) = match (seg.verb.as_str(), seg.rest.split_first()) {
        ("worktree", Some((add, words))) if add == "add" => ("worktree-add", &WORKTREE_ADD, 0_usize, words),
        ("clone", _) => ("clone", &CLONE, 1_usize, seg.rest.as_slice()),
        _ => return None,
    };
    if words.iter().any(|word| word == "-h" || word == "--help") {
        return None;
    }
    let to = match positionals(words, flags) {
        None => Dest::Unresolved,
        Some(found) => match found.get(nth) {
            Some(word) => path_of(word, seg),
            None if verb == "clone" => under(seg),
            None => Dest::Unresolved,
        },
    };
    Some(Place { verb, to })
}

/// 位置の語の行き先（literal でない・相対で作業 dir が解けない周は解けない）。
fn path_of(word: &str, seg: &GitSegment) -> Dest {
    let target = Path::new(word);
    match (literal(word), target.is_absolute(), seg.resolved) {
        (false, _, _) | (true, false, false) => Dest::Unresolved,
        (true, true, _) => Dest::Path(normalized(target)),
        (true, false, true) => Dest::Path(normalized(&seg.dir.join(target))),
    }
}

/// 行き先の語を持たない clone の行き先（作業 dir の直下・作業 dir が解けなければ解けない）。
fn under(seg: &GitSegment) -> Dest {
    match seg.resolved {
        true => Dest::Under(normalized(&seg.dir)),
        false => Dest::Unresolved,
    }
}

/// redirect の語か（頭に数字が在ってよく、続く字が `<` か `>` で始まる語）。
fn is_redirect(word: &str) -> bool {
    word.trim_start_matches(|found: char| found.is_ascii_digit()).starts_with(['<', '>'])
}

/// 語の途中（頭の数字の後ろでない所）に `<` か `>` を持つ語か。
fn mid_redirect(word: &str) -> bool {
    !is_redirect(word) && word.contains(['<', '>'])
}

/// 動詞の後ろの語から位置の語を取り出す（redirect の語は数えず、演算子だけの語は次の語も飛ばし、`--` の後ろは全部位置の語・
/// flag は閉じた 2 列で読む）。読めない形（知らない flag・語の途中の redirect）は `None`。
fn positionals<'a>(words: &'a [String], flags: &Flags) -> Option<Vec<&'a str>> {
    if words.iter().any(|word| mid_redirect(word)) {
        return None;
    }
    let (mut found, mut at, mut rest) = (Vec::new(), 0_usize, false);
    while let Some(word) = words.get(at) {
        at = at.saturating_add(1);
        if is_redirect(word) {
            let operator = word.chars().all(|ch| ch.is_ascii_digit() || ch == '<' || ch == '>');
            at = at.saturating_add(usize::from(operator));
        } else if rest || !word.starts_with('-') {
            found.push(word.as_str());
        } else if word == "--" {
            rest = true;
        } else {
            let extra = flag_extra(word, flags)?;
            if extra > 0 {
                words.get(at)?;
            }
            at = at.saturating_add(extra);
        }
    }
    Some(found)
}

/// flag の語が次の語を値として取る数（0 か 1）。閉じた 2 列の外・`-` だけの語は `None`。
fn flag_extra(word: &str, flags: &Flags) -> Option<usize> {
    match word.strip_prefix("--") {
        Some(long) => long_extra(long, flags),
        None => short_extra(word.strip_prefix('-').filter(|cluster| !cluster.is_empty())?, flags),
    }
}

/// 長い flag（`--` を除いた語）が次の語を値として取る数（`--<名>=<値>` は 0・`--no-<名>` は取らない側）。
fn long_extra(long: &str, flags: &Flags) -> Option<usize> {
    let (name, inline) = match long.split_once('=') {
        Some((name, _)) => (name, true),
        None => (long, false),
    };
    if flags.value_long.contains(&name) {
        return Some(usize::from(!inline));
    }
    let (base, negated) = name.strip_prefix("no-").map_or((name, false), |rest| (rest, true));
    let known = flags.value_long.contains(&base) || flags.plain_long.contains(&base);
    (known && (!inline || (!negated && OPTIONAL_VALUE.contains(&base)))).then_some(0)
}

/// 1 字の flag の束ね（左から読み、値を取る字に当たればその字の後ろの残りか次の語を値とする）が次の語を値として取る数。
fn short_extra(cluster: &str, flags: &Flags) -> Option<usize> {
    for (at, found) in cluster.char_indices() {
        if flags.value_short.contains(found) {
            return Some(usize::from(cluster.get(at.saturating_add(found.len_utf8())..)?.is_empty()));
        }
        if !flags.plain_short.contains(found) {
            return None;
        }
    }
    Some(0)
}

/// `cd` / `pushd` の後ろの作業 dir（引数が literal なら前の dir から解き、でなければ解けなくする）。
fn moved(dir: &Path, resolved: bool, rest: &[String]) -> (PathBuf, bool) {
    let target = rest.iter().find(|word| !(word.starts_with('-') && word.len() > 1));
    match target {
        Some(word) if literal(word) && word != "-" => {
            let next = Path::new(word);
            (dir.join(next), resolved || next.is_absolute())
        }
        _ => (dir.to_path_buf(), false),
    }
}

/// literal な path の語か（変数・command 置換・brace・glob・`~` 始まりを持たない）。
fn literal(word: &str) -> bool {
    !word.is_empty() && !word.starts_with('~') && !word.contains(NOT_LITERAL)
}

/// git の後ろの語から大域の option を読み飛ばし、（動詞・`-C` を連鎖で足した dir・解けたか・動詞の後ろの語）を返す。動詞が
/// 無ければ `None`。
fn git_verb(rest: &[String], dir: &Path) -> Option<(String, PathBuf, bool, Vec<String>)> {
    let (mut at, mut dir, mut ok) = (0_usize, dir.to_path_buf(), true);
    loop {
        let word = rest.get(at)?.as_str();
        let value = rest.get(at.saturating_add(1)).map(String::as_str);
        let step = match word {
            "-C" => {
                match value.filter(|found| literal(found)) {
                    Some(found) => dir = dir.join(found),
                    None => ok = false,
                }
                2
            }
            "-c" | "--namespace" => 2,
            "--git-dir" | "--work-tree" => {
                ok = false;
                2
            }
            flag if flag.starts_with("--git-dir=") || flag.starts_with("--work-tree=") => {
                ok = false;
                1
            }
            flag if flag.starts_with('-') => 1,
            verb => return Some((verb.to_owned(), dir, ok, rest.get(at.saturating_add(1)..).unwrap_or_default().to_vec())),
        };
        at = at.saturating_add(step);
    }
}

/// 門に渡す材料（道具・command 行・payload・作業 dir・anchor・置き場）。
pub struct Scene<'a> {
    /// tool 名。
    pub tool: &'a str,
    /// Bash の command 行。
    pub command: Option<&'a str>,
    /// payload の全文。
    pub payload: &'a str,
    /// payload の `cwd`。
    pub cwd: &'a Path,
    /// hook の `--project` の root（anchor）。
    pub root: &'a Path,
    /// hook の置き場。
    pub state_dir: &'a Path,
}

/// 門の入口: 編集系の道具は [`decide_edit`]・Bash は [`decide_commit`]。他の道具は置き場を読まずに通す。
pub fn decide(scene: &Scene) -> LiveRowDecision {
    if EDIT_TOOLS.contains(&scene.tool) {
        decide_edit(scene)
    } else if scene.tool == BASH {
        decide_commit(scene)
    } else {
        LiveRowDecision::Pass
    }
}

/// 編集の門（設計 §15 形 3）: 対象が置き場の同じ worktree の docs/design/ の下の表の doc の周だけ置き場を読み、変更前 =
/// disk の本文・変更後 = 道具の入力を当てた本文で比べる。
fn decide_edit(scene: &Scene) -> LiveRowDecision {
    let Some((path, before, after)) = edited(scene) else {
        return LiveRowDecision::Pass;
    };
    let Some((root, doc)) = design_doc(&path, scene.state_dir) else {
        return LiveRowDecision::Pass;
    };
    if doc.ends_with(".md") && !has_region(&before) && !has_region(&after) {
        return LiveRowDecision::Pass;
    }
    let Ok(runs) = live_runs(scene.state_dir) else {
        return match changes(&doc, &before, &after).is_empty() {
            true => LiveRowDecision::Pass,
            false => unreadable_state(scene.state_dir, &doc),
        };
    };
    refusal(exclude_own(hits(&doc, &before, &after, &runs), &root), scene.state_dir, scene.root)
}

/// 区間の始まりの行を持つか。
fn has_region(text: &str) -> bool {
    text.lines().any(|line| line.trim() == BEGIN)
}

/// 編集先の絶対 path と、変更前（disk の本文・無ければ空）と道具の入力を当てた変更後の本文。path が表の置き場の形で
/// ない・入力が当たらない周は `None`。
fn edited(scene: &Scene) -> Option<(PathBuf, String, String)> {
    let tree = json_tree::parse(scene.payload).ok()?;
    let input = tree.get("tool_input")?;
    let file = input.get("file_path").and_then(Tree::as_str)?;
    let path = normalized(&scene.cwd.join(file));
    if !path.to_string_lossy().contains(DESIGN_DIR) || form_of(&path.to_string_lossy()).is_err() {
        return None;
    }
    let before = std::fs::read_to_string(&path).unwrap_or_default();
    let after = match scene.tool {
        "Write" => input.get("content").and_then(Tree::as_str).map(str::to_owned),
        "Edit" => replaced(&before, input),
        _ => input.get("edits").and_then(Tree::as_array)?.iter().try_fold(before.clone(), |text, edit| replaced(&text, edit)),
    }?;
    Some((path, before, after))
}

/// `old_string` を `new_string` へ（`replace_all` なら全部・でなければ最初の 1 つ）。`old_string` が無ければ `None`
/// （道具が落ちるので通す）。
fn replaced(text: &str, edit: &Tree) -> Option<String> {
    let old = edit.get("old_string").and_then(Tree::as_str).filter(|found| !found.is_empty())?;
    let new = edit.get("new_string").and_then(Tree::as_str).unwrap_or_default();
    if !text.contains(old) {
        return None;
    }
    match edit.get("replace_all").and_then(Tree::as_bool) {
        Some(true) => Some(text.replace(old, new)),
        _ => Some(text.replacen(old, new, 1)),
    }
}

/// `.` と `..` を字面で畳む（fs を読まない）。
fn normalized(path: &Path) -> PathBuf {
    let mut found = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                found.pop();
            }
            other => found.push(other),
        }
    }
    found
}

/// path の worktree の root と repo 相対 path（置き場が hook の置き場と同じで、docs/design/ の下の周だけ）。
fn design_doc(path: &Path, state_dir: &Path) -> Option<(PathBuf, String)> {
    let existing = path.ancestors().skip(1).find(|dir| dir.is_dir())?;
    let root = vessel::repo_root(existing)?;
    if !same_place(&vessel::state_dir(&root)?, state_dir) {
        return None;
    }
    let real = existing.canonicalize().ok()?.join(path.strip_prefix(existing).ok()?);
    let rel = real.strip_prefix(&root).ok()?.to_string_lossy().into_owned();
    rel.starts_with(DESIGN_DIR).then_some((root, rel))
}

/// 2 つの置き場が同じか（実体 path で比べ、解けなければ字面）。
fn same_place(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(one), Ok(other)) => one == other,
        _ => left == right,
    }
}

/// commit の門（設計 §15 形 4）: git の commit の segment ごとに、解けない対象は live な便の有無で断り、解けた対象は
/// HEAD との差（index と作業の木の和・rename は旧 path と新 path の 2 つ）の docs/design/ の doc を比べる。
fn decide_commit(scene: &Scene) -> LiveRowDecision {
    let command = scene.command.unwrap_or_default();
    let commits: Vec<GitSegment> =
        git_segments(command, scene.cwd, scene.root).into_iter().filter(|seg| seg.verb == COMMIT).collect();
    for seg in &commits {
        let decision = match seg.resolved {
            true => resolved_commit(scene, &seg.dir),
            false => unresolved_commit(scene),
        };
        if decision != LiveRowDecision::Pass {
            return decision;
        }
    }
    LiveRowDecision::Pass
}

/// 解けない対象の commit: live な便が 0 本なら通し、1 本以上なら差分を見ずに断る。event log を読めない周も断る。
fn unresolved_commit(scene: &Scene) -> LiveRowDecision {
    let runs = match live_runs(scene.state_dir) {
        Ok(found) => found,
        Err(_) => return unreadable_state(scene.state_dir, "-"),
    };
    let Some(first) = runs.first() else {
        return LiveRowDecision::Pass;
    };
    let line = format!(
        "{NAME}: deny live-row reason=dir-unresolved live={} run={} — commit の対象の dir を字面で解けない（変数の cd・\
         --git-dir・--work-tree）ので live な便の行を守れない（vessel-hook.md §15）。dir を literal で書き直す: git -C <絶対 path> \
         commit …（か cd <絶対 path> && git commit …）",
        runs.len(),
        first.id
    );
    LiveRowDecision::Deny { what: "dir-unresolved".to_owned(), line }
}

/// 解けた対象の commit: 同じ置き場の worktree で、HEAD との差に docs/design/ の表の doc が在り、live な便が 1 本以上の
/// 周だけ、変更前 = HEAD の blob・変更後 = index の blob と作業の木の本文の両方で比べる。
fn resolved_commit(scene: &Scene, dir: &Path) -> LiveRowDecision {
    let Some(root) = vessel::repo_root(dir) else {
        return LiveRowDecision::Pass;
    };
    if !vessel::state_dir(&root).is_some_and(|found| same_place(&found, scene.state_dir)) {
        return LiveRowDecision::Pass;
    }
    let Some(changed) = changed_paths(&root) else {
        return unresolved_commit(scene);
    };
    let docs: Vec<String> =
        changed.into_iter().filter(|path| path.starts_with(DESIGN_DIR) && form_of(path).is_ok()).collect();
    if docs.is_empty() {
        return LiveRowDecision::Pass;
    }
    let runs = live_runs(scene.state_dir);
    let mut found = Vec::new();
    let mut ancestor: Option<bool> = None;
    for doc in &docs {
        let before = git_bytes(&root, &["show", &format!("HEAD:{doc}")]).unwrap_or_default();
        let index = git_bytes(&root, &["show", &format!(":{doc}")]).unwrap_or_default();
        let work = std::fs::read_to_string(root.join(doc)).unwrap_or_default();
        let Ok(runs) = &runs else {
            if !changes(doc, &before, &index).is_empty() || !changes(doc, &before, &work).is_empty() {
                return unreadable_state(scene.state_dir, doc);
            }
            continue;
        };
        let (mut at_index, mut at_work) = (hits(doc, &before, &index, runs), hits(doc, &before, &work, runs));
        let counted = !exclude_own(at_index.clone(), &root).is_empty() || !exclude_own(at_work.clone(), &root).is_empty();
        if counted && *ancestor.get_or_insert_with(|| merge_head_is_main_ancestor(&root)) {
            if let Some(theirs) = git_bytes(&root, &["show", &format!("MERGE_HEAD:{doc}")]) {
                at_index = hits_merging(doc, &before, &index, Some(&theirs), runs);
                at_work = hits_merging(doc, &before, &work, Some(&theirs), runs);
            }
        }
        for hit in at_index.into_iter().chain(at_work) {
            if !found.iter().any(|seen: &Hit| seen.run == hit.run && seen.row == hit.row) {
                found.push(hit);
            }
        }
    }
    refusal(exclude_own(found, &root), scene.state_dir, scene.root)
}

/// MERGE_HEAD が anchor の main の祖先か（MERGE_HEAD が無い・main の祖先でない・測れない周は偽）。
fn merge_head_is_main_ancestor(root: &Path) -> bool {
    git_bytes(root, &["merge-base", "--is-ancestor", "MERGE_HEAD", MAIN_REF]).is_some()
}

/// HEAD との差の path の列（index と作業の木の和・`--no-renames`＝rename は旧 path と新 path）。git が落ちれば `None`。
fn changed_paths(root: &Path) -> Option<Vec<String>> {
    let staged = git_bytes(root, &["diff", "--cached", "--name-only", "-z", "--no-renames", "HEAD"])?;
    let working = git_bytes(root, &["diff", "--name-only", "-z", "--no-renames", "HEAD"])?;
    let mut found: Vec<String> =
        staged.split('\0').chain(working.split('\0')).filter(|path| !path.is_empty()).map(str::to_owned).collect();
    found.sort();
    found.dedup();
    Some(found)
}

/// git を 1 回撃って stdout の全文を返す（trim しない）。落ちれば `None`。
fn git_bytes(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Invocation::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 置き場の path を絶対にする（止める 1 行に写す形）。
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// event log を読めない周の断り（止める 1 行は持たない）。
fn unreadable_state(state_dir: &Path, doc: &str) -> LiveRowDecision {
    let line = format!(
        "{NAME}: deny live-row reason=state-unreadable doc={doc} — 置き場 {} の event log を読めないので、走っている便の行を\
         変えていないと示せない（fail-closed・vessel-hook.md §15）。event log を直すまで表の行の字を変えない",
        absolute(state_dir).display()
    );
    LiveRowDecision::Deny { what: "state-unreadable".to_owned(), line }
}

/// 当たりの列を判定にする（空なら通す）。
fn refusal(hits: Vec<Hit>, state_dir: &Path, root: &Path) -> LiveRowDecision {
    match deny_line(&hits, state_dir, root) {
        Some(line) => LiveRowDecision::Deny { what: hits.first().map_or("", |hit| hit.reason.as_str()).to_owned(), line },
        None => LiveRowDecision::Pass,
    }
}

/// 断りの 1 行（設計 §15 形 6・§24 約束 3）: 先頭の当たりの段が Questioned の周だけ、その便の最新の問いの about を
/// [`question_of_run`] の 1 本で読み（通す周は読まない）、[`deny_sentence`] で文にする。
pub(crate) fn deny_line(hits: &[Hit], state_dir: &Path, root: &Path) -> Option<String> {
    let first = hits.first()?;
    let about = match first.stage {
        Stage::Questioned => question_of_run(state_dir, &first.run).and_then(|question| question.about),
        _ => None,
    };
    deny_sentence(hits, about.as_deref(), state_dir, root)
}

/// write-set の問いの句（答えでは広がらない・止めてから行を広げる道）。
const WRITE_SET_WAY: &str =
    "write-set の問いは答えでは広がらない（contract-source.md §7）— 止めてから行の write-set を広げ、受付で次の便を起こす";

/// 断りの文の組み立て（**pure な 1 関数**・設計 §24 約束 1・2）: 先頭の当たりの理由・行・run id・段・残りの本数と、止めずに
/// 済ませる道（終端まで待つ）を名指し、Questioned の段は about で分ける（`write-set` は句だけ・それ以外の about は答える口だけ・
/// about 無しは両方）。**行の末尾**に止めてから変える 1 行を置く（後ろに何も付けない）。
pub(crate) fn deny_sentence(hits: &[Hit], about: Option<&str>, state_dir: &Path, root: &Path) -> Option<String> {
    let first = hits.first()?;
    let state = absolute(state_dir);
    let repo = absolute(first.repo.as_deref().unwrap_or(root));
    let mouth = format!(
        "・問いなら答える「{NAME} pipe answer --run {} --words <答えの逐語> --state-dir {}」",
        first.run,
        state.display()
    );
    let answer = match (first.stage, about) {
        (Stage::Questioned, Some("write-set")) => format!("・{WRITE_SET_WAY}"),
        (Stage::Questioned, Some(_)) => mouth,
        (Stage::Questioned, None) => format!("{mouth}・{WRITE_SET_WAY}"),
        _ => String::new(),
    };
    Some(format!(
        "{NAME}: deny live-row reason={} row={} run={} stage={} others={} — 走っている便の行の字は変えない（便は受付の写しで\
         走る・vessel-hook.md §15）。止めずに済ませるなら段が終端（Landed / Failed / Stopped・判定 FAIL の Gated・審査が PASS \
         でない Reviewed）に着くまで待つ{answer}。止めてから変えるなら: {NAME} pipe stop --run {} --state-dir {} --repo {}",
        first.reason.as_str(),
        first.row,
        first.run,
        first.stage.as_str(),
        hits.len().saturating_sub(1),
        first.run,
        state.display(),
        repo.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::{copy_places, deny_line, deny_sentence, exclude_own, git_segments, hits, hits_merging, Dest, Reason};
    use crate::fleet::Stage;
    use crate::name::NAME;
    use crate::pipe::cli::{LiveRun, Tag};
    use crate::pipe::table::Pointer;
    use std::path::{Path, PathBuf};

    /// 表の doc の path。
    const DOC: &str = "docs/design/x.md";

    /// 1 行の契約（done だけを差し替える）。
    fn row(id: &str, done: &str) -> String {
        format!(
            "[[contract]]\nid = \"{id}\"\ntitle = \"t\"\nreq = [\"FR1\"]\nsection = \"1\"\nverify = [\"cargo test\"]\n\
             size = \"S\"\ndone = \"{done}\"\n"
        )
    }

    /// 前置きの散文と行の列で doc を組む。
    fn doc(prose: &str, rows: &[String]) -> String {
        format!("# t\n{prose}\n<!-- contracts:begin -->\nschema = 1\n\n{}<!-- contracts:end -->\n", rows.join("\n"))
    }

    /// live な便 1 本。
    fn run(id: &str, stage: Stage, tag: Tag, worktree: Option<&str>) -> LiveRun {
        LiveRun { id: id.to_owned(), stage, tag, repo: None, worktree: worktree.map(PathBuf::from) }
    }

    /// 行の名札。
    fn row_tag(doc: &str, id: &str) -> Tag {
        Tag::Row(Pointer { path: doc.to_owned(), id: id.to_owned() })
    }

    /// (a) live な行の done を変えた本文は当たり 1 つ（run id と段つき）・同じ変更を live でない行に当てると 0。
    #[test]
    fn hook_live_row_changed_done_hits_only_the_live_row() {
        let before = doc("", &[row("a", "d"), row("b", "d")]);
        let runs = [run("r1", Stage::Questioned, row_tag(DOC, "a"), None)];
        let found = hits(DOC, &before, &doc("", &[row("a", "e"), row("b", "d")]), &runs);
        assert_eq!(found.len(), 1, "{found:?}");
        let hit = found.first().cloned();
        assert_eq!(hit.as_ref().map(|h| (h.run.as_str(), h.stage, h.row.as_str(), h.reason)), Some(("r1", Stage::Questioned, "docs/design/x.md#a", Reason::Changed)));
        assert!(hits(DOC, &before, &doc("", &[row("a", "d"), row("b", "e")]), &runs).is_empty(), "live でない行");
    }

    /// (b) 表の前に散文を 3 行足す・行の順を入れ替えるだけの本文は 0（line も比べる変異で赤）。
    #[test]
    fn hook_live_row_moved_lines_and_reordered_rows_do_not_hit() {
        let before = doc("", &[row("a", "d"), row("b", "d")]);
        let runs = [run("r1", Stage::Spawned, row_tag(DOC, "a"), None), run("r2", Stage::Spawned, Tag::Unread, None)];
        assert!(hits(DOC, &before, &doc("1\n2\n3", &[row("a", "d"), row("b", "d")]), &runs).is_empty(), "散文 3 行");
        assert!(hits(DOC, &before, &doc("", &[row("b", "d"), row("a", "d")]), &runs).is_empty(), "並べ替え");
    }

    /// (c) live な行を消すと理由が行の消失。
    #[test]
    fn hook_live_row_removed_row_is_named_removed() {
        let runs = [run("r1", Stage::Implemented, row_tag(DOC, "a"), None)];
        let found = hits(DOC, &doc("", &[row("a", "d"), row("b", "d")]), &doc("", &[row("b", "d")]), &runs);
        assert_eq!(found.iter().map(|hit| hit.reason).collect::<Vec<_>>(), [Reason::Removed]);
    }

    /// (d) 変更後の表を壊すと理由が表の読めなさ・変更前の表が壊れていて変更後が直っていれば 0。
    #[test]
    fn hook_live_row_broken_after_table_hits_and_broken_before_does_not() {
        let runs = [run("r1", Stage::Blocked, row_tag(DOC, "a"), None)];
        let good = doc("", &[row("a", "d")]);
        let broken = doc("", &[format!("{}bogus\n", row("a", "d"))]);
        let found = hits(DOC, &good, &broken, &runs);
        assert_eq!(found.iter().map(|hit| hit.reason).collect::<Vec<_>>(), [Reason::TableUnreadable]);
        assert!(hits(DOC, &broken, &good, &runs).is_empty(), "壊れた表を直す編集は止めない");
    }

    /// (e) live な行を親に持つ約束の行の欄を変えると当たり。
    #[test]
    fn hook_live_row_promise_of_the_live_row_is_compared() {
        let promise = |text: &str| {
            format!(
                "[[promise]]\nof = \"a\"\nn = 1\ntext = \"{text}\"\nfiles = [\"x.rs\"]\nteeth = [\"t\"]\nfixture = \"f\"\n\
                 expect = \"e\"\n"
            )
        };
        let runs = [run("r1", Stage::Questioned, row_tag(DOC, "a"), None)];
        let before = doc("", &[row("a", "d"), promise("p")]);
        let found = hits(DOC, &before, &doc("", &[row("a", "d"), promise("q")]), &runs);
        assert_eq!(found.iter().map(|hit| hit.reason).collect::<Vec<_>>(), [Reason::Changed], "{before}");
    }

    /// (f) 名札が doc だけの便はその doc の行が 1 つ変われば当たり・他の doc なら 0、名札が無い便はどの doc でも行が
    /// 1 つ変われば当たり・散文だけなら 0。
    #[test]
    fn hook_live_row_doc_only_and_unread_tags() {
        let before = doc("", &[row("a", "d"), row("b", "d")]);
        let after = doc("", &[row("a", "d"), row("b", "e")]);
        let other = "docs/design/y.md";
        let doc_only = [run("r1", Stage::Gated, Tag::Doc(DOC.to_owned()), None)];
        assert_eq!(hits(DOC, &before, &after, &doc_only).len(), 1, "同じ doc");
        assert!(hits(other, &before, &after, &doc_only).is_empty(), "他の doc");
        let unread = [run("r2", Stage::Reviewed, Tag::Unread, None)];
        assert_eq!(hits(other, &before, &after, &unread).len(), 1, "どの doc でも");
        assert!(hits(other, &before, &doc("prose", &[row("a", "d"), row("b", "d")]), &unread).is_empty(), "散文だけ");
    }

    /// (g) 読み手 1 本の読み: 解けた例は dir つきで commit と同定し、解けない例は root に倒して印を立て、commit でない
    /// 例は同定しない。
    #[test]
    fn hook_live_row_git_segments_read_verb_and_dir() {
        let (cwd, root) = (Path::new("/w"), Path::new("/root"));
        let commit = |line: &str| {
            git_segments(line, cwd, root).into_iter().find(|seg| seg.verb == "commit").map(|seg| (seg.dir, seg.resolved))
        };
        for (line, dir) in [
            ("git -C /d commit -am x", "/d"),
            ("git -C a -C b commit", "/w/a/b"),
            ("sudo git commit", "/w"),
            ("cd /d && git commit", "/d"),
            ("pushd /d && git commit", "/d"),
            ("git --namespace n commit", "/w"),
        ] {
            assert_eq!(commit(line), Some((PathBuf::from(dir), true)), "{line}");
        }
        for line in ["git --git-dir=y commit", "git --work-tree y commit", "cd \"$X\" && git commit"] {
            assert_eq!(commit(line), Some((root.to_path_buf(), false)), "{line}");
        }
        for line in ["git log --grep commit", "git status", "echo git commit", "git commitx"] {
            assert_eq!(commit(line), None, "{line}");
        }
    }

    /// (h) 除外: 便の worktree で自分の行は落ち、同じ worktree で他の live な行は残る。
    #[test]
    fn hook_live_row_exclude_own_row_only_in_its_worktree() {
        let before = doc("", &[row("a", "d"), row("b", "d")]);
        let after = doc("", &[row("a", "e"), row("b", "e")]);
        let runs = [
            run("r1", Stage::Spawned, row_tag(DOC, "a"), Some("/wt/r1")),
            run("r2", Stage::Spawned, row_tag(DOC, "b"), Some("/wt/r2")),
        ];
        let left = exclude_own(hits(DOC, &before, &after, &runs), Path::new("/wt/r1"));
        assert_eq!(left.iter().map(|hit| hit.run.as_str()).collect::<Vec<_>>(), ["r2"], "自分の行だけ落ちる");
        assert_eq!(exclude_own(hits(DOC, &before, &after, &runs), Path::new("/anchor")).len(), 2, "anchor では両方");
    }

    /// (i) 断りの 1 行は末尾がちょうど止める 1 行で終わり、Questioned の段だけが答える口を持つ。
    #[test]
    fn hook_live_row_deny_line_ends_with_the_stop_line() {
        let before = doc("", &[row("a", "d")]);
        let after = doc("", &[row("a", "e")]);
        let (state, root) = (Path::new("/s"), Path::new("/repo"));
        for (stage, answers) in [(Stage::Questioned, true), (Stage::Gated, false)] {
            let found = hits(DOC, &before, &after, &[run("r1", stage, row_tag(DOC, "a"), None)]);
            let line = deny_line(&found, state, root).unwrap_or_default();
            assert!(line.ends_with(&format!("{NAME} pipe stop --run r1 --state-dir /s --repo /repo")), "{line}");
            assert_eq!(line.contains("pipe answer --run r1"), answers, "{line}");
            assert!(line.contains("stage=") && line.contains("run=r1") && line.contains("others=0"), "{line}");
        }
    }

    /// 同じ当たり（行 a・段 `stage`）を about つきで文にする。
    fn sentence(stage: Stage, about: Option<&str>) -> String {
        let found = hits(DOC, &doc("", &[row("a", "d")]), &doc("", &[row("a", "e")]), &[run("r1", stage, row_tag(DOC, "a"), None)]);
        deny_sentence(&found, about, Path::new("/s"), Path::new("/repo")).unwrap_or_default()
    }

    /// (a)(b)(c) about の write-set は句だけ・write-set でない about は答える口だけ・about 無しは両方・Gated はどちらも無し。
    #[test]
    fn live_row_about_splits_the_answer_mouth_and_the_write_set_way() {
        let (way, mouth) = ("write-set の問いは答えでは広がらない（contract-source.md §7）— 止めてから行の write-set を広げ、受付で次の便を起こす", "pipe answer --run r1");
        let stop = format!("{NAME} pipe stop --run r1 --state-dir /s --repo /repo");
        let write_set = sentence(Stage::Questioned, Some("write-set"));
        assert!(write_set.contains(way) && !write_set.contains("pipe answer") && write_set.ends_with(&stop), "{write_set}");
        let verify = sentence(Stage::Questioned, Some("verify"));
        assert!(verify.contains(mouth) && !verify.contains(way) && verify.ends_with(&stop), "{verify}");
        let none = sentence(Stage::Questioned, None);
        assert!(none.contains(mouth) && none.contains(way) && none.ends_with(&stop), "{none}");
        let gated = sentence(Stage::Gated, Some("write-set"));
        assert!(gated.contains("stage=Gated") && !gated.contains(way) && !gated.contains("pipe answer"), "{gated}");
    }

    /// command 行（cwd `/w`・root `/root`）の写しの segment（動詞の語・行き先）の列。
    fn places(line: &str) -> Vec<(&'static str, Dest)> {
        copy_places(line, Path::new("/w"), Path::new("/root")).into_iter().map(|place| (place.verb, place.to)).collect()
    }

    /// (a) 読みの表（設計 §25 約束 1）: flag の値・続け書き・束ね・redirect を読み飛ばして行き先を取る。
    #[test]
    fn hook_drafts_place_reads_the_destination_through_flags() {
        let path = |verb: &'static str, to: &str| vec![(verb, Dest::Path(PathBuf::from(to)))];
        for (line, expected) in [
            ("git worktree add ../x", path("worktree-add", "/x")),
            ("git worktree add -b feat x main", path("worktree-add", "/w/x")),
            ("git worktree add --lock --reason r x", path("worktree-add", "/w/x")),
            ("git worktree add -bfeat x", path("worktree-add", "/w/x")),
            ("git worktree add -fd x", path("worktree-add", "/w/x")),
            ("git -C /d worktree add y", path("worktree-add", "/d/y")),
            ("cd /d && git worktree add --detach y", path("worktree-add", "/d/y")),
            ("git clone u /e", path("clone", "/e")),
            ("git clone -b main --depth 1 u e", path("clone", "/w/e")),
            ("git clone --branch=main -- u e", path("clone", "/w/e")),
            ("git clone --recurse-submodules=p u e", path("clone", "/w/e")),
            ("git clone --no-reference u e", path("clone", "/w/e")),
            ("git clone u", vec![("clone", Dest::Under(PathBuf::from("/w")))]),
            ("git worktree add -qb feat x", path("worktree-add", "/w/x")),
            ("git worktree add 2>e x", path("worktree-add", "/w/x")),
            ("git clone u e > log", path("clone", "/w/e")),
            ("cd $X && git worktree add /e/y", path("worktree-add", "/e/y")),
        ] {
            assert_eq!(places(line), expected, "{line}");
        }
    }

    /// (b) 解けない 9 形は行き先を持たない（`Dest::Unresolved`）。
    #[test]
    fn hook_drafts_place_unresolved_forms_have_no_destination() {
        let tilde = concat!("git worktree add ~", "/x");
        for line in [
            "git worktree add \"$D\"",
            tilde,
            "git worktree add --frob x",
            "git worktree add -qz x",
            "git clone u {a,b}",
            "cd $X && git clone u e",
            "git --git-dir=y worktree add x",
            "git worktree add x>log",
            "git worktree add",
        ] {
            let verb = if line.contains("clone") { "clone" } else { "worktree-add" };
            assert_eq!(places(line), vec![(verb, Dest::Unresolved)], "{line}");
        }
    }

    /// (c) 読まない 8 形: 写しを作らない動詞・表示だけ・他の頭の語。
    #[test]
    fn hook_drafts_place_skips_what_does_not_make_a_copy() {
        for line in [
            "git worktree list",
            "git worktree remove x",
            "git worktree move a b",
            "echo git clone u e",
            "git clone -h",
            "git clone --help",
            "git cloned u",
            "gh repo clone o/n",
        ] {
            assert!(places(line).is_empty(), "{line}");
        }
    }

    /// 行 a と行 b を `(a, b)` の done にした本文。
    fn two(a: &str, b: &str) -> String {
        doc("", &[row("a", a), row("b", b)])
    }

    /// 行 a の名札の便 1 本に MERGE_HEAD の本文つきで当てた理由の列。
    fn merged(before: &str, after: &str, theirs: Option<&str>, tag: Tag) -> Vec<(String, Reason)> {
        hits_merging(DOC, before, after, theirs, &[run("r1", Stage::Spawned, tag, None)])
            .into_iter()
            .map(|hit| (hit.row, hit.reason))
            .collect()
    }

    /// (a)(b)(c) MERGE_HEAD の行と同じ字の変化は落ち（消えたも同じ）、違えば残り、表が読めない・MERGE_HEAD が無い周は `hits` と同じ。
    #[test]
    fn live_row_merge_head_drops_only_the_changes_the_merge_already_has() {
        let tag = || row_tag(DOC, "a");
        let (before, after) = (two("d", "d"), two("e", "d"));
        assert!(merged(&before, &after, Some(&two("e", "d")), tag()).is_empty(), "同じ字");
        for other in ["d", "f"] {
            assert_eq!(merged(&before, &after, Some(&two(other, "d")), tag()), [("docs/design/x.md#a".to_owned(), Reason::Changed)], "{other}");
        }
        let gone = doc("", &[row("b", "d")]);
        assert!(merged(&before, &gone, Some(&gone), tag()).is_empty(), "MERGE_HEAD にも無い");
        assert_eq!(merged(&before, &gone, Some(&before), tag()), [("docs/design/x.md#a".to_owned(), Reason::Removed)]);
        let plain = hits(DOC, &before, &after, &[run("r1", Stage::Spawned, tag(), None)]).len();
        for theirs in [Some("not a table"), None] {
            assert_eq!(merged(&before, &after, theirs, tag()).len(), plain, "{theirs:?}");
        }
    }

    /// (d) 名札が doc だけの便は、落とした後の列の先頭＝行 b に当たる。
    #[test]
    fn live_row_merge_head_doc_only_tag_takes_the_first_change_left() {
        let found = merged(&two("d", "d"), &two("e", "e"), Some(&two("e", "d")), Tag::Doc(DOC.to_owned()));
        assert_eq!(found, [("docs/design/x.md#b".to_owned(), Reason::Changed)]);
    }

    /// (e) 行 a は同じ字でも、行 a を親に持つ約束の行が MERGE_HEAD と違えば changed。
    #[test]
    fn live_row_merge_head_compares_the_promises_of_the_row() {
        let promise = |text: &str| {
            format!("[[promise]]\nof = \"a\"\nn = 1\ntext = \"{text}\"\nfiles = [\"x.rs\"]\nteeth = [\"t\"]\nfixture = \"f\"\nexpect = \"e\"\n")
        };
        let with = |done: &str, text: &str| doc("", &[row("a", done), promise(text)]);
        let (before, after) = (with("d", "p"), with("e", "q"));
        assert_eq!(merged(&before, &after, Some(&with("e", "r")), row_tag(DOC, "a")).len(), 1, "約束だけ違う");
        assert!(merged(&before, &after, Some(&with("e", "q")), row_tag(DOC, "a")).is_empty(), "約束も同じ");
    }
}
