//! 退役（`pipe retire`・設計 pipeline.md §5.4・FR12・`pipe::land` から見せる）。
//!
//! 後始末は **可逆な move**（N1.2）。公開の入口は `pipe::land` の `pub use` が元の path のまま外へ見せる。

use super::land::{broken, refused, retire_worktree, WorktreeCheck};
use super::{emit, verdict_path, worktree_path, worktrees_dir, Emit};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::json_lite;
use crate::fleet::store::LockPolicy;
use crate::fleet::{EventKind, Stage};
use std::path::{Path, PathBuf};

/// land 済み worktree を寄せる dir 名。
const RETIRED_DIR: &str = "retired";

/// retire 1 回の材料（`--pr-cmd` 形の便を merge の後に畳む口・設計 §5.4）。
///
/// **契約を要らない**のが land との違いである。畳むのは worktree という入れ物だけで、
/// 契約の verify も write-set も読まない——読む理由が無い面を材料に数えると、契約が
/// 壊れた便の worktree が永久に畳めなくなる。
pub struct Retire<'a> {
    /// 便 id。
    pub run: &'a str,
    /// 契約の bead id。
    pub bead: &'a str,
    /// 対象 repo。
    pub repo: &'a Path,
    /// 置き場。
    pub state_dir: &'a Path,
    /// **その便の現在の終端の段**（`Landed` か `Failed`・呼び手が replay から解いたもの）。
    /// 畳んだ事実を残す event はこの段のままで、retire は段を 1 つも動かさない（`s2-07l.128`）。
    pub stage: Stage,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// land 後に worktree を寄せる先。
pub fn retired_path(repo: &Path, id: &str) -> PathBuf {
    worktrees_dir(repo).join(RETIRED_DIR).join(id)
}

/// `--pr-cmd` 形で終端した便の worktree を、merge の後に畳む（設計 §5.4）。
///
/// **`detail=pr` を前提にしない**。squash 形で move だけが落ちた便（land は rc 0 のまま
/// stderr 1 行で終わる）を後追いで畳む口にもなるので、見るのは永続面の事実——worktree が
/// 在るか・clean か——だけである。**merge 済みかは人が確かめる**（forge へ問い合わせない）。
///
/// **段を動かさない**（`s2-07l.128`）。畳める便は `Landed`・`Failed detail=rebase-empty`
/// （変更が既に main に在る）・`Failed detail=rebase-conflict`（起こし直しの上限に達した）・
/// `Gated` で verdict が FAIL（判定に届いた終端）の 4 通りで、どの周も残す event の段は
/// [`Retire::stage`] のまま＝`Landed` に決め打ちしない。畳む動作そのものは 1 本で、
/// 段の弁別は入口（`pipe::cli`）が持つ。
///
/// 前提違反は **rc 1 + stderr 1 行で何も書かない**（設計 §4 の一般則）。move の失敗だけは
/// 「対象そのものが壊れている」ので rc 2 で、どちらの周も event を 1 件も残さない。
pub fn retire(entry: &Retire<'_>) -> Outcome {
    let worktree = worktree_path(entry.repo, entry.run);
    if !worktree.is_dir() {
        // 2 度目の retire もここで止まる（1 度目が畳んでいるので元の場所に無い）。
        return refused(format!("run {} の worktree {} が無い", entry.run, worktree.display()));
    }
    let check = WorktreeCheck::judge(&worktree);
    if !check.is_clean() {
        return refused(format!("run {} の worktree が clean でない（{}）", entry.run, check.as_str()));
    }
    let failures = retire_worktree(entry.repo, entry.run, &worktree);
    if !failures.is_empty() {
        // **畳めていないのに「畳んだ」を記帳しない**（永続面と event が食い違う）。
        return Outcome { out: Vec::new(), err: failures, rc: RC_BROKEN };
    }
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            // **段は入口が解いた終端のまま**（終端を動かさない）。畳んだことは detail で残す。
            stage: Some(entry.stage),
            seat: None,
            pid: None,
            detail: Some("retired".to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => Outcome::ok_line(format!(
            "run={} retired={}",
            entry.run,
            retired_path(entry.repo, entry.run).display()
        )),
    }
}

/// `verdict.json` の文字列 field を 1 つ読む（**JSON の読み手はこの 1 本**・読めない周は `None`）。
pub(super) fn verdict_field(state_dir: &Path, id: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(verdict_path(state_dir, id)).ok()?;
    let pairs = json_lite::parse_object(text.trim()).ok()?;
    pairs
        .iter()
        .find(|(found, _)| found == key)
        .and_then(|(_, value)| value.as_str())
        .map(str::to_owned)
}
