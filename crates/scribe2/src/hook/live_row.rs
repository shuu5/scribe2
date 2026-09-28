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
    let found = changes(doc, before, after);
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
        for hit in hits(doc, &before, &index, runs).into_iter().chain(hits(doc, &before, &work, runs)) {
            if !found.iter().any(|seen: &Hit| seen.run == hit.run && seen.row == hit.row) {
                found.push(hit);
            }
        }
    }
    refusal(exclude_own(found, &root), scene.state_dir, scene.root)
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

/// 断りの 1 行（設計 §15 形 6）: 先頭の当たりの理由・行・run id・段・残りの本数と、止めずに済ませる道（終端まで待つ・
/// Questioned なら答える口）を名指し、**行の末尾**に止めてから変える 1 行を置く（後ろに何も付けない）。
pub(crate) fn deny_line(hits: &[Hit], state_dir: &Path, root: &Path) -> Option<String> {
    let first = hits.first()?;
    let state = absolute(state_dir);
    let repo = absolute(first.repo.as_deref().unwrap_or(root));
    let answer = match first.stage {
        Stage::Questioned => format!(
            "・問いなら答える「{NAME} pipe answer --run {} --words <答えの逐語> --state-dir {}」",
            first.run,
            state.display()
        ),
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
    use super::{deny_line, exclude_own, git_segments, hits, Reason};
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
}
