//! 着地が anchor を揃えなかった周の印（設計 docs/design/pipeline.md §57・契約表の行 az・memo `s2-07l.695`）。
//!
//! land が anchor（`--repo` の checkout）の index と作業の木を揃えなかった周（dirty / collision / unreadable /
//! sync-failed）は、anchor の git dir の `<NAME>/` の下に印を 1 本置く（ADR-0004 D-4 の git dir の下の file の形・
//! write-set guard の policy と同じ置き場）。中身は着地の前の main の sha（from＝index と作業の木が最後に揃って
//! いた main）と改行 1 つだけで、理由の語は持たない（`Landed` の detail が持つ）。
//!
//! 古さの判定（[`stale`]）は印と今の中身で決める（読み手は印を書き換えない）。印を外す口（[`anchor_sync`]・
//! `pipe anchor-sync`）は、index と作業の木が from の中身のままの着地の path だけを main の先端へ戻す——利用者の
//! 編集を持つ path と着地の外の path は 1 byte も触らない。

use super::super::{git_bytes, git_line};
use super::{nul_paths, AnchorSkip, AnchorSync};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::invocation::Invocation;
use crate::name::NAME;
use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// 印の file 名（`<git-dir>/<NAME>/` の下・版番号に依らず固定）。
const MARK_FILE: &str = "anchor-stale";

/// 印の path（`<git-dir>/<NAME>/anchor-stale`・[`crate::hook::guard::policy_path`] と同じ dir）。
pub fn mark_path(git_dir: &Path) -> PathBuf {
    git_dir.join(NAME).join(MARK_FILE)
}

/// anchor の git dir（`--absolute-git-dir`・linked worktree は自分の git dir）。repo でない周は `None`。
fn git_dir_of(repo: &Path) -> Option<PathBuf> {
    git_line(repo, &["rev-parse", "--absolute-git-dir"]).map(PathBuf::from)
}

/// 印を書いた結果（**閉じた 3 値**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marked {
    /// 印を置いた。
    Written,
    /// 印が既に在った（書き換えない＝index に残る古い中身は最初に揃えなかった main のまま・from を保つ）。
    Kept,
    /// from と着地後の main が同じ（既着地の old → old）＝書かない。
    Same,
}

/// 印を置く（**書き手はこの 1 本**・land の 2 か所と境界 crate の歯の fixture が呼ぶ）。
///
/// 一時 file から rename で置く（読み手が書きかけの印を読まない）。git dir を解けない・書けない周は `Err`（1 行）。
pub fn write_mark(repo: &Path, from: &str, landed: &str) -> Result<Marked, String> {
    if from == landed {
        return Ok(Marked::Same);
    }
    let git_dir = git_dir_of(repo).ok_or_else(|| "git dir を解けない".to_owned())?;
    write_mark_in(&git_dir, from, landed)
}

/// [`write_mark`] の本体（git dir を解いた後）。
fn write_mark_in(git_dir: &Path, from: &str, landed: &str) -> Result<Marked, String> {
    if from == landed {
        return Ok(Marked::Same);
    }
    let path = mark_path(git_dir);
    if path.symlink_metadata().is_ok() {
        return Ok(Marked::Kept);
    }
    let dir = git_dir.join(NAME);
    std::fs::create_dir_all(&dir).map_err(|err| format!("{} を作れない: {err}", dir.display()))?;
    let tmp = dir.join(format!("{MARK_FILE}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, format!("{from}\n")).map_err(|err| format!("{} を書けない: {err}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|err| {
        let _ = std::fs::remove_file(&tmp);
        format!("{} を置けない: {err}", path.display())
    })?;
    Ok(Marked::Written)
}

/// 印を消す（無い周は何もしない）。消せない周は `Err`（1 行）。
fn clear_in(git_dir: &Path) -> Result<(), String> {
    let path = mark_path(git_dir);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("{} を消せない: {err}", path.display())),
    }
}

/// 印の 1 行を読む（**16 進の 40 字か 64 字と改行 1 つだけ**が読める・欠け・余り・16 進でない字・2 行目は `None`）。
fn parse_mark(body: &[u8]) -> Option<String> {
    let sha = std::str::from_utf8(body).ok()?.strip_suffix('\n')?;
    let hex = sha.bytes().all(|byte| byte.is_ascii_hexdigit());
    (hex && matches!(sha.len(), 40 | 64)).then(|| sha.to_owned())
}

/// 印の読み（**閉じた 3 値**）。
enum MarkRead {
    /// 印が無い。
    Absent,
    /// 印の from。
    Found(String),
    /// 在るのに読めない（1 行の sha でない・読めない file）。
    Unreadable,
}

/// git dir の印を読む。
fn read_mark(git_dir: &Path) -> MarkRead {
    match std::fs::read(mark_path(git_dir)) {
        Err(err) if err.kind() == ErrorKind::NotFound => MarkRead::Absent,
        Err(_) => MarkRead::Unreadable,
        Ok(body) => parse_mark(&body).map_or(MarkRead::Unreadable, MarkRead::Found),
    }
}

/// 揃えた結果と、land の 1 周が印に残した結果（[`record`] の戻り）。
pub(super) struct Anchored {
    /// 揃えた結果（判定行の token と warning・`Failed` の側はこれだけを読む）。
    pub(super) sync: AnchorSync,
    /// `Landed` の detail の末尾（印を置いた skip の周だけ ` anchor=skipped:<理由>`・他の周は空）。
    pub(super) detail: String,
    /// 印を書けない・消せない周の stderr の 1 行（land の rc は変えない）。
    pub(super) err: Option<String>,
}

/// `sync_anchor` の直後（main の実測の前）に印を置く / 消す（設計 §57 形 1・2）。
///
/// dirty / collision / unreadable / sync-failed の周は印を置き（既に在れば保つ・from と着地後の main が同じ周は
/// 書かない）、synced の周は印を消し、not-main の周は触らない。
pub(super) fn record(repo: &Path, sync: AnchorSync, from: &str, landed: &str) -> Anchored {
    let (detail, err) = match &sync {
        AnchorSync::Skipped(AnchorSkip::NotMain) => (String::new(), None),
        AnchorSync::Synced => {
            let cleared = git_dir_of(repo).ok_or_else(|| "git dir を解けない".to_owned()).and_then(|dir| clear_in(&dir));
            (String::new(), cleared.err().map(|reason| format!("pipe: anchor の古さの印を消せない（{reason}）")))
        }
        AnchorSync::Skipped(_) => match write_mark(repo, from, landed) {
            Ok(Marked::Written | Marked::Kept) => (format!(" {}", sync.token()), None),
            Ok(Marked::Same) => (String::new(), None),
            Err(reason) => (String::new(), Some(format!("pipe: anchor の古さの印を置けない（{reason}）・land-window は古さを測れない"))),
        },
    };
    Anchored { sync, detail, err }
}

/// anchor の古さ（**閉じた 3 値**・形 4 の land-window・形 5 の口・行 h の門が同じ [`stale`] を撃つ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Staleness {
    /// 印が無い、または着地の path に from の中身のままの path が無い（手で揃えた周）。
    Fresh,
    /// 着地の path のうち index か作業の木が from と同じ path が在る。
    Stale {
        /// 印の from。
        from: String,
        /// 古い path の本数。
        paths: usize,
    },
    /// 印か from か HEAD を読めない（理由の語）。**古いと同じ側に倒す**（fail-closed）。
    Unreadable(&'static str),
}

impl Staleness {
    /// 窓を閉じる周の token（`Fresh` は `None`＝行は 1 字も変わらない）。
    pub fn token(&self) -> Option<&'static str> {
        match self {
            Self::Fresh => None,
            Self::Stale { .. } => Some("anchor=stale"),
            Self::Unreadable(_) => Some("anchor=unreadable"),
        }
    }
}

/// anchor の古さを判定する（**読むだけ**・印を書き換えない・設計 §57 形 3）。
///
/// 印が無ければ git dir を解く 1 本のほかは撃たずに新しい（repo でない・git dir を解けない周も印が無い＝新しい）。
pub fn stale(repo: &Path) -> Staleness {
    let Some(git_dir) = git_dir_of(repo) else {
        return Staleness::Fresh;
    };
    let from = match read_mark(&git_dir) {
        MarkRead::Absent => return Staleness::Fresh,
        MarkRead::Unreadable => return Staleness::Unreadable(MARK),
        MarkRead::Found(from) => from,
    };
    match material(repo, &from) {
        Err(Unread::From) => Staleness::Unreadable(FROM),
        Err(Unread::Git(reason)) => Staleness::Unreadable(reason),
        Ok((_, sorted)) if sorted.is_empty() => Staleness::Fresh,
        Ok((_, sorted)) => Staleness::Stale { from, paths: sorted.restorable.len() + sorted.mixed.len() },
    }
}

/// 読めない印の理由の語。
const MARK: &str = "mark";

/// 印の from を commit として解けない周の理由の語。
const FROM: &str = "from";

/// 材料の読めなさ（from は印の側・他は git の側）。
enum Unread {
    /// 印の from を commit として解けない。
    From,
    /// git の読みが断った（理由の語）。
    Git(&'static str),
}

/// 着地の path 1 本（from → HEAD の name-status・rename は割る）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Landed {
    /// path。
    path: String,
    /// from に無く HEAD に足された path か。
    added: bool,
}

/// 着地の path の仕分け（[`sort`] の戻り）。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Sorted {
    /// index と作業の木の両方が from と同じで戻せる path。
    restorable: Vec<String>,
    /// 片方だけが from と同じ path・足された path に file が在る path（古いが戻さない）。
    mixed: Vec<String>,
}

impl Sorted {
    /// 古い path が 1 本も無いか。
    fn is_empty(&self) -> bool {
        self.restorable.is_empty() && self.mixed.is_empty()
    }
}

/// 着地の path を仕分ける（**pure**・設計 §57 形 3 と形 5 が同じ 1 本を通す）。
///
/// `index` / `work` は index / 作業の木が from と違う path の集合、`present` は作業の木に在る path。index も作業の木も
/// from と違う path（手で揃えた・利用者の編集）は古くない。着地の外の path は数えない（`landed` だけを回る）。
fn sort(landed: &[Landed], index: &BTreeSet<String>, work: &BTreeSet<String>, present: &BTreeSet<String>) -> Sorted {
    let mut sorted = Sorted::default();
    for found in landed {
        let (index_old, work_old) = (!index.contains(&found.path), !work.contains(&found.path));
        let occupied = found.added && present.contains(&found.path);
        match (index_old, work_old) {
            (false, false) => {}
            (true, true) if !occupied => sorted.restorable.push(found.path.clone()),
            _ => sorted.mixed.push(found.path.clone()),
        }
    }
    sorted
}

/// `diff --name-status -z` の出力を着地の path の列にする。
fn landed_of(bytes: &[u8]) -> Vec<Landed> {
    nul_paths(bytes)
        .chunks(2)
        .filter_map(|pair| match pair {
            [status, path] => Some(Landed { path: path.clone(), added: status.starts_with('A') }),
            _ => None,
        })
        .collect()
}

/// 判定の材料を読んで仕分ける（git の読みは from と HEAD の解きと name-only / name-status の 3 本）。戻りは HEAD の sha と仕分け。
fn material(repo: &Path, from: &str) -> Result<(String, Sorted), Unread> {
    git_line(repo, &["rev-parse", "--verify", "--quiet", &format!("{from}^{{commit}}")]).ok_or(Unread::From)?;
    let head = git_line(repo, &["rev-parse", "--verify", "--quiet", "HEAD"]).ok_or(Unread::Git("head"))?;
    let status = git_bytes(repo, &["diff", "--no-renames", "--name-status", "-z", from, &head]).ok_or(Unread::Git("landed"))?;
    let index = git_bytes(repo, &["diff", "--cached", "--no-renames", "--name-only", "-z", from]).ok_or(Unread::Git("index"))?;
    let work = git_bytes(repo, &["diff", "--no-renames", "--name-only", "-z", from]).ok_or(Unread::Git("worktree"))?;
    let landed = landed_of(&status);
    let present: BTreeSet<String> = landed
        .iter()
        .filter(|found| found.added && repo.join(&found.path).symlink_metadata().is_ok())
        .map(|found| found.path.clone())
        .collect();
    let set = |bytes: &[u8]| nul_paths(bytes).into_iter().collect::<BTreeSet<String>>();
    Ok((head, sort(&landed, &set(&index), &set(&work), &present)))
}

/// `pipe anchor-sync --repo R`: 古い path を main の先端の中身へ戻して印を外す（設計 §57 形 5・1 行と rc）。
pub(in crate::pipe) fn anchor_sync(repo: &Path) -> Outcome {
    let Some(git_dir) = git_dir_of(repo) else {
        return said(RC_REFUSED, "anchor-sync=refused unreadable:git-dir".to_owned());
    };
    let from = match read_mark(&git_dir) {
        MarkRead::Absent => return said(RC_OK, "anchor-sync=none".to_owned()),
        MarkRead::Unreadable => return unreadable_mark(repo, &git_dir, MARK),
        MarkRead::Found(from) => from,
    };
    let (head, sorted) = match material(repo, &from) {
        Ok(found) => found,
        Err(Unread::From) => return unreadable_mark(repo, &git_dir, FROM),
        Err(Unread::Git(reason)) => return said(RC_REFUSED, format!("anchor-sync=refused unreadable:{reason}")),
    };
    if sorted.is_empty() {
        return cleared(&git_dir, "anchor-sync=already".to_owned());
    }
    if !sorted.restorable.is_empty() {
        if let Err(line) = restore(repo, &head, &sorted.restorable) {
            return said(RC_BROKEN, format!("anchor-sync=failed restore:{line}"));
        }
    }
    if !sorted.mixed.is_empty() {
        return said(RC_REFUSED, format!("anchor-sync=refused mixed={}", sorted.mixed.join(",")));
    }
    // 戻した後の判定が新しい周だけ印を外す（戻ったと言い切れない周は印を残して名指す）。
    match stale(repo) {
        Staleness::Fresh => cleared(&git_dir, format!("anchor-sync=synced paths={}", sorted.restorable.len())),
        Staleness::Stale { from, paths } => said(RC_BROKEN, format!("anchor-sync=failed restore:stale from={from} paths={paths}")),
        Staleness::Unreadable(reason) => said(RC_BROKEN, format!("anchor-sync=failed restore:unreadable:{reason}")),
    }
}

/// 印が読めない周: tracked の変更が無ければ（古い中身を持ちえない）印を消して `already`、在れば `refused unreadable`。
fn unreadable_mark(repo: &Path, git_dir: &Path, reason: &str) -> Outcome {
    match git_bytes(repo, &["status", "--porcelain", "--untracked-files=no"]) {
        None => said(RC_REFUSED, "anchor-sync=refused unreadable:status".to_owned()),
        Some(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => cleared(git_dir, "anchor-sync=already".to_owned()),
        Some(_) => said(RC_REFUSED, format!("anchor-sync=refused unreadable:{reason}")),
    }
}

/// 印を消してから `line` を rc 0 で返す（消せない周は rc 2 と stderr の 1 行）。
fn cleared(git_dir: &Path, line: String) -> Outcome {
    match clear_in(git_dir) {
        Ok(()) => said(RC_OK, line),
        Err(reason) => Outcome { out: vec![line], err: vec![format!("pipe: {reason}")], rc: RC_BROKEN },
    }
}

/// 1 行と rc。
fn said(rc: u8, line: String) -> Outcome {
    Outcome { out: vec![line], err: Vec::new(), rc }
}

/// 戻せる path だけを HEAD の中身へ戻す（`git --literal-pathspecs restore --source=<HEAD> --staged --worktree --`）。
/// 断った周は git の stderr の最初の行（無ければ rc）。
fn restore(repo: &Path, head: &str, paths: &[String]) -> Result<(), String> {
    let source = format!("--source={head}");
    let ran = Invocation::new("git")
        .arg("-C")
        .arg(repo)
        .args(["--literal-pathspecs", "restore", &source, "--staged", "--worktree", "--"])
        .args(paths)
        .output();
    match ran {
        Err(err) => Err(err.to_string()),
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stderr);
            let first = text.lines().map(str::trim).find(|line| !line.is_empty());
            Err(first.map_or_else(|| format!("rc {}", output.status.code().unwrap_or(-1)), str::to_owned))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{mark_path, parse_mark, read_mark, sort, write_mark_in, Landed, MarkRead, Marked, Sorted};
    use std::collections::BTreeSet;

    fn set(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    fn landed(path: &str, added: bool) -> Landed {
        Landed { path: path.to_owned(), added }
    }

    /// 形 3 と形 5 の仕分け（設計 §57 の歯 (a)〜(f)）: 両方が from と同じ path は戻せ、片方だけが同じ path と足された path に
    /// file が在る path は戻さず（mixed）、両方が違う path は古くなく、着地の外の path は数えない。
    #[test]
    fn pipe_anchor_stale_sort_classifies_each_landed_path() {
        let paths = [
            landed("a-both-old", false),
            landed("b-index-old", false),
            landed("c-work-old", false),
            landed("d-added-present", true),
            landed("e-both-new", false),
            landed("g-added-absent", true),
        ];
        // index が from と違う path・作業の木が from と違う path（着地の外の `f-outside` も両方に在る）。
        let index = set(&["c-work-old", "e-both-new", "f-outside"]);
        let work = set(&["b-index-old", "e-both-new", "f-outside"]);
        let present = set(&["d-added-present", "f-outside"]);
        let sorted = sort(&paths, &index, &work, &present);
        assert_eq!(
            sorted,
            Sorted {
                restorable: vec!["a-both-old".to_owned(), "g-added-absent".to_owned()],
                mixed: vec!["b-index-old".to_owned(), "c-work-old".to_owned(), "d-added-present".to_owned()],
            }
        );
        // 足されていない path は作業の木に在っても戻せる（file の在否は足された path だけに効く）。
        let kept = sort(&[landed("d-added-present", false)], &set(&[]), &set(&[]), &present);
        assert_eq!(kept.restorable, vec!["d-added-present".to_owned()], "{kept:?}");
        // 両方が違う path だけなら古くない（手で揃えた周）。
        assert!(sort(&[landed("e-both-new", false)], &index, &work, &present).is_empty(), "手で揃えた周は新しい");
        assert!(sort(&[], &index, &work, &present).is_empty(), "着地の外の path は数えない");
    }

    /// 印の 1 行の読み: 16 進の 40 字か 64 字と改行 1 つだけが読める。
    #[test]
    fn pipe_anchor_stale_mark_reads_only_one_hex_line() {
        let (sha1, sha256) = ("0123456789abcdef0123456789abcdef01234567", "a".repeat(64));
        assert_eq!(parse_mark(format!("{sha1}\n").as_bytes()), Some(sha1.to_owned()), "40 字");
        assert_eq!(parse_mark(format!("{sha256}\n").as_bytes()), Some(sha256.clone()), "64 字");
        for bad in [
            sha1.to_owned(),
            format!("{sha1}\n\n"),
            format!("{sha1}\nsecond\n"),
            "0123456789abcdef0123456789abcdef0123456\n".to_owned(),
            format!("{sha1}0\n"),
            "0123456789abcdef0123456789abcdef0123456g\n".to_owned(),
            format!(" {sha1}\n"),
            String::new(),
            "\n".to_owned(),
        ] {
            assert_eq!(parse_mark(bad.as_bytes()), None, "読めない: {bad:?}");
        }
    }

    /// 印の書き: 無ければ置き（from の 1 行）、既に在れば書き換えず、from と着地後の main が同じ周は書かない。
    #[test]
    fn pipe_anchor_stale_mark_write_keeps_existing_and_skips_same() {
        let root = crate::pipe::fixture::scratch("anchor-mark");
        let (old, new, newer) = ("1".repeat(40), "2".repeat(40), "3".repeat(40));
        assert_eq!(write_mark_in(&root, &old, &old), Ok(Marked::Same), "old → old は書かない");
        assert!(!mark_path(&root).exists(), "Same の周は file を作らない");
        assert!(matches!(read_mark(&root), MarkRead::Absent), "無い印は Absent");
        assert_eq!(write_mark_in(&root, &old, &new), Ok(Marked::Written));
        assert_eq!(std::fs::read_to_string(mark_path(&root)).ok(), Some(format!("{old}\n")), "from の 1 行");
        assert_eq!(write_mark_in(&root, &new, &newer), Ok(Marked::Kept), "既に在る印は書き換えない");
        assert!(matches!(read_mark(&root), MarkRead::Found(found) if found == old), "from を保つ");
        let leftovers: Vec<String> = std::fs::read_dir(root.join(crate::name::NAME))
            .map(|dir| dir.filter_map(Result::ok).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        assert_eq!(leftovers, vec!["anchor-stale".to_owned()], "一時 file は残らない");
        let _ = std::fs::remove_dir_all(&root);
    }
}
