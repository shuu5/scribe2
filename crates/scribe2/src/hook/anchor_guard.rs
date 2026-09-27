//! anchor の門（`pre-tool-use` の Bash の門・設計 docs/design/vessel-hook.md §14・契約表の行 h・FR20 / FR24 / FR30・
//! NFR5）。
//!
//! 起票の門の直後・権能 guard の前の 1 段で、2 つを断る: (i) 着地が揃えなかった anchor（行 az の印・pipeline.md §57）で
//! index を載せる git の 7 語（[`INDEX_VERBS`]）、(ii) 着地列の窓（`window_now`・pipeline.md §19）が閉じている間の
//! main の anchor での `git commit` と `gh pr merge`。git の segment の読み手は行 i の 1 本（[`git_segments`]）を呼び、
//! この module が持つのは動詞の仕分け（[`targets_of`]）と判定だけである。古さの判定（[`stale`]）と窓の判定
//! （[`window_now`]）も既存の 1 本を呼ぶ（2 本目を書かない・C2）。
//!
//! 7 語の git でも `gh pr merge` でもない Bash は git を 1 本も撃たずに通す。印・git・置き場・event log を読めない周は
//! 断る（FailClosed・[`POLARITY`]）。

use super::host_guard::verb_of;
use super::ledger_guard::{is_assignment, segments};
use super::live_row::git_segments;
use super::vessel::{self, Served};
use crate::invocation::Invocation;
use crate::name::NAME;
use crate::pipe::cli::window_now;
use crate::pipe::land::{stale, Staleness};
use crate::polarity::{OnFailure, Polarity, Timing};
use std::path::{Path, PathBuf};

/// この境界の極性: 実行の時点で止め、印・git・置き場・event log を読めない周は断る。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// index を載せる git の動詞（閉じた 7 語・git の語彙であって裁定ではない＝rules 行に置かない）。
const INDEX_VERBS: [&str; 7] = ["commit", "merge", "pull", "rebase", "cherry-pick", "revert", "am"];
/// 窓も撃つ git の動詞。
const COMMIT: &str = "commit";
/// gh の動詞と、`-` の語を除いた最初の 2 語。
const GH: &str = "gh";
/// `gh pr merge` の 2 語。
const PR_MERGE: [&str; 2] = ["pr", "merge"];
/// 記録の `anchor-deny` の後ろに置く `gh pr merge` の語。
const WHAT_MERGE: &str = "gh-pr-merge";
/// 窓を撃つ checkout の HEAD が指す ref。
const MAIN_REF: &str = "refs/heads/main";
/// 窓の行が anchor の古さで閉じている印（`Staleness::token` の語）。
const WINDOW_STALE: &str = "anchor=stale";

/// 門の判定（閉じた 2 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorDecision {
    /// 通す。
    Pass,
    /// 断る（`what` は動詞の 1 語・`line` は stderr の 1 行）。
    Deny {
        /// 動詞の 1 語（記録の `anchor-deny <what>`）。
        what: String,
        /// stderr の 1 行。
        line: String,
    },
}

/// 門が撃つ segment 1 つ（仕分けの閉じた 3 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// commit 以外の 6 語の git（動詞・対象の dir）＝古さだけを撃つ。
    Index {
        /// git の動詞。
        verb: String,
        /// 対象の dir（解けない周は `--project` の root）。
        dir: PathBuf,
    },
    /// git commit（対象の dir）＝古さと窓を撃つ。
    Commit {
        /// 対象の dir（解けない周は `--project` の root）。
        dir: PathBuf,
    },
    /// `gh pr merge`（対象は `--project` の root）＝窓を撃つ。
    Merge,
}

/// 動詞の仕分け（**1 関数**・設計 §14 形 2）: git の segment は行 i の読み手の動詞を 7 語と commit に分け、gh の segment は
/// launcher と `NAME=value` を剥いだ後ろの `-` で始まらない最初の 2 語が `pr merge` のときだけ当てる。git の segment を先に、
/// gh の segment を後に並べる。
pub(crate) fn targets_of(command: &str, cwd: &Path, root: &Path) -> Vec<Target> {
    let git = git_segments(command, cwd, root).into_iter().filter_map(|seg| match seg.verb.as_str() {
        COMMIT => Some(Target::Commit { dir: seg.dir }),
        verb if INDEX_VERBS.contains(&verb) => Some(Target::Index { verb: seg.verb, dir: seg.dir }),
        _ => None,
    });
    let gh = segments(command).into_iter().filter(|words| is_pr_merge(words)).map(|_| Target::Merge);
    git.chain(gh).collect()
}

/// gh の segment が `pr merge` か（`-` の語を除いた最初の 2 語）。
fn is_pr_merge(words: &[String]) -> bool {
    let lead = words.iter().take_while(|word| is_assignment(word)).count();
    let Some((GH, rest)) = verb_of(words.get(lead..).unwrap_or_default()) else {
        return false;
    };
    let head: Vec<&str> = rest.iter().map(String::as_str).filter(|word| !word.starts_with('-')).take(2).collect();
    head == PR_MERGE
}

/// 門の入口（Bash の command 行・payload の `cwd`・`--project` の root・hook の置き場）。当たる segment が無ければ git を
/// 撃たずに通す。当たる segment を command の中の順に判定し、最初の断りを返す。
pub fn decide(command: &str, cwd: &Path, root: &Path, state_dir: &Path) -> AnchorDecision {
    for target in targets_of(command, cwd, root) {
        let decision = match &target {
            Target::Index { verb, dir } => fresh(verb, dir),
            Target::Commit { dir } => match fresh(COMMIT, dir) {
                AnchorDecision::Pass => commit_window(dir),
                denied => denied,
            },
            Target::Merge => window(WHAT_MERGE, state_dir, root),
        };
        if decision != AnchorDecision::Pass {
            return decision;
        }
    }
    AnchorDecision::Pass
}

/// (i) 対象の checkout の古さ（行 az の判定）: 新しければ通し、古い周と読めない周は揃える 1 行の literal で断る。
fn fresh(verb: &str, dir: &Path) -> AnchorDecision {
    let (reason, why) = match stale(dir) {
        Staleness::Fresh => return AnchorDecision::Pass,
        Staleness::Stale { from, paths } => (
            format!("anchor-stale from={from} paths={paths}"),
            "着地が揃えなかった anchor の index と作業の木は着地の前の中身のままで、ここで index を載せると着地を巻き戻す",
        ),
        Staleness::Unreadable(what) => (
            format!("anchor-unreadable:{what}"),
            "anchor の古さの印か着地の前の main か HEAD を読めない（読めない周は古いと同じ側に倒す・fail-closed）",
        ),
    };
    let checkout = vessel::repo_root(dir).unwrap_or_else(|| dir.to_path_buf());
    let line = format!(
        "{NAME}: deny anchor-guard verb={verb} reason={reason} checkout={} — {why}（vessel-hook.md §14・pipeline.md §57）。\
         揃えてから撃つ: {}",
        checkout.display(),
        sync_line(&checkout)
    );
    AnchorDecision::Deny { what: verb.to_owned(), line }
}

/// 揃える 1 行の literal。
fn sync_line(root: &Path) -> String {
    format!("{NAME} pipe anchor-sync --repo {}", root.display())
}

/// (ii) commit の窓: 対象が main の worktree で HEAD が `refs/heads/main` を指し、仕える repo の周だけ窓を撃つ。他の repo・
/// linked worktree・別 branch は通す。git と置き場を読めない周は断る。
fn commit_window(dir: &Path) -> AnchorDecision {
    let Some(root) = vessel::repo_root(dir) else {
        return AnchorDecision::Pass;
    };
    if !matches!(vessel::served(&root), Served::ByMe(_)) {
        return AnchorDecision::Pass;
    }
    let Some(state_dir) = vessel::state_dir(&root) else {
        return unreadable(&root, "state-dir");
    };
    match on_main(&root) {
        None => unreadable(&root, "git"),
        Some(false) => AnchorDecision::Pass,
        Some(true) => window(COMMIT, &state_dir, &root),
    }
}

/// main の worktree（git dir と common dir が同じ）で HEAD が `refs/heads/main` を指すか（git の読み 1 本）。読めなければ
/// `None`。
fn on_main(root: &Path) -> Option<bool> {
    let output = Invocation::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--path-format=absolute", "--git-dir", "--git-common-dir", "--symbolic-full-name", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let lines: Vec<&str> = text.lines().collect();
    let [git_dir, common, head] = lines.as_slice() else {
        return None;
    };
    let real = |path: &str| Path::new(path).canonicalize().unwrap_or_else(|_| PathBuf::from(path));
    Some(real(git_dir) == real(common) && *head == MAIN_REF)
}

/// 窓を撃ち、閉じていれば窓の busy の行を写して断る（次の一手は窓が anchor=stale を持てば揃える 1 行・他は窓を見る 1 行）。
fn window(what: &str, state_dir: &Path, root: &Path) -> AnchorDecision {
    let found = window_now(state_dir, root);
    if found.is_open() {
        return AnchorDecision::Pass;
    }
    let busy = found.line();
    let next = match busy.split(' ').any(|token| token == WINDOW_STALE) {
        true => sync_line(root),
        false => format!("{NAME} pipe land-window --repo {}", root.display()),
    };
    let line = format!(
        "{NAME}: deny anchor-guard verb={what} reason=window-busy {busy} checkout={} — 着地列の窓が閉じている間は main の \
         anchor で commit と gh pr merge を撃たない（着地を分岐で壊す・vessel-hook.md §14・pipeline.md §19）。次の一手: {next}",
        root.display()
    );
    AnchorDecision::Deny { what: what.to_owned(), line }
}

/// git か置き場を読めない周の断り（fail-closed）。
fn unreadable(root: &Path, what: &str) -> AnchorDecision {
    let line = format!(
        "{NAME}: deny anchor-guard verb={COMMIT} reason=unreadable:{what} checkout={} — main の anchor か、着地列の窓を読む\
         置き場を解けないので、窓が開いていると示せない（fail-closed・vessel-hook.md §14）。次の一手: {NAME} pipe land-window \
         --repo {}",
        root.display(),
        root.display()
    );
    AnchorDecision::Deny { what: COMMIT.to_owned(), line }
}

#[cfg(test)]
mod tests {
    use super::{targets_of, Target};
    use std::path::{Path, PathBuf};

    /// 仕分けの読み（cwd は `/w`・root は `/root`）。
    fn sorted(line: &str) -> Vec<Target> {
        targets_of(line, Path::new("/w"), Path::new("/root"))
    }

    /// 6 語の git の当たり。
    fn index(verb: &str, dir: &str) -> Target {
        Target::Index { verb: verb.to_owned(), dir: PathBuf::from(dir) }
    }

    /// commit の当たり。
    fn commit(dir: &str) -> Target {
        Target::Commit { dir: PathBuf::from(dir) }
    }

    /// 7 語の git は commit と 6 語に分かれ、対象の dir は行 i の読み手のまま（解けない対象は root）。
    #[test]
    fn hook_anchor_verb_sorts_the_seven_git_verbs_and_commit() {
        for (line, want) in [
            ("git -C x -c k=v commit", commit("/w/x")),
            ("git --no-pager pull", index("pull", "/w")),
            ("FOO=1 timeout 5 git merge x", index("merge", "/w")),
            ("cd x && git rebase y", index("rebase", "/w/x")),
            ("git cherry-pick a", index("cherry-pick", "/w")),
            ("git revert a", index("revert", "/w")),
            ("git am p", index("am", "/w")),
            ("cd \"$X\" && git commit", commit("/root")),
            ("git --git-dir=y commit", commit("/root")),
        ] {
            assert_eq!(sorted(line), [want], "{line}");
        }
    }

    /// `gh pr merge` だけが当たり、他の git の動詞・git の語を引数に持つ command・gh の他の動詞は当たらない。
    #[test]
    fn hook_anchor_verb_matches_gh_pr_merge_and_nothing_near() {
        assert_eq!(sorted("gh pr merge 1"), [Target::Merge]);
        assert_eq!(sorted("gh pr merge --squash 1"), [Target::Merge], "flag を除いた 2 語");
        assert_eq!(sorted("git status && gh pr merge 1"), [Target::Merge], "連結の後ろの segment");
        for line in ["git log --grep commit", "git status", "git add x", "echo git commit", "git commitx", "gh pr view 1"] {
            assert_eq!(sorted(line), [], "{line}");
        }
        assert_eq!(sorted("echo gh pr merge 1"), [], "動詞が gh でない");
    }
}
