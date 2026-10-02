//! 退役（`pipe retire`・設計 pipeline.md §5.4・FR12・`pipe::land` から見せる）。
//!
//! 後始末は **可逆な move**（N1.2）。公開の入口は `pipe::land` の `pub use` が元の path のまま外へ見せる。

use super::declaration::terminal_facts;
use super::land::{broken, close_reason, refused, retire_worktree, CloseTail, WorktreeCheck, CLOSE_REASON, MAIN_REF};
use super::{branch_name, emit, git_bytes, git_ok, verdict_path, worktree_path, worktrees_dir, Emit};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::json_lite;
use crate::fleet::lifecycle::{self, Place};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{ci_read, pr_merge, CiRead, EventKind, PrMerge, Stage};
use crate::rules::manifest::Manifest;
use crate::seat::ledger::{read_ledger, timeout_of};
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
    /// 台帳 client（`--bd`・無ければ既定名）。PR の便の照合が読みと close に使う。
    pub bd: &'a str,
    /// `--fold-only`: 照合も close もせずに畳む（契約は開いたまま残す・設計 contract-source.md §61 形 3）。
    pub fold_only: bool,
    /// 規則（台帳の待ち上限の行 `seat.ledger_timeout_s` を読む）。
    pub manifest: &'a Manifest,
}

/// PR で着地した便の `Landed` の `RunDone` の detail（`pipe land --pr-cmd` が書く字面）。
const LANDED_PR: &str = "pr";

/// 台帳の closed の status の字面。
const LEDGER_CLOSED: &str = "closed";

/// close を通した周が `Landed` の `RunDone` に残す detail（land の終端が書く字面と同じ）。
const CLOSE_OK: &str = "terminal:close:ok";

/// commit id の桁数（40 桁の 16 進）。
const OID_LEN: usize = 40;

/// 照合が通らない周の閉じた 6 語の字面（[`Refusal`] の宣言順・stdout の `retire=` の値）。
pub const REFUSAL_WORDS: &[&str] =
    &["worktree-unready", "not-merged", "not-ancestor", "ci-not-success", "unmeasured", "unwritten"];

/// PR の便を畳まない理由（**閉じた 6 値**・設計 contract-source.md §61 形 11・字面は [`REFUSAL_WORDS`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// worktree が無いか clean でない、または close の後の move が落ちた。
    WorktreeUnready,
    /// PR が merge されていない。
    NotMerged,
    /// merge の commit が remote の main の先端の祖先でない。
    NotAncestor,
    /// 先端の CI が success でない（failure か、結果がまだ無い）。
    CiNotSuccess,
    /// 問いを撃てない・答えを読めない。
    Unmeasured,
    /// 台帳を閉じられなかった。
    Unwritten,
}

impl Refusal {
    /// stdout の `retire=` の語。
    fn word(self) -> &'static str {
        REFUSAL_WORDS.get(self as usize).copied().unwrap_or_default()
    }
}

/// 照合が通った PR の便の材料（merge の commit と remote の main の先端）。
struct Proof {
    /// PR の merge の commit id。
    merge: String,
    /// remote の main の先端の commit id。
    tip: String,
}

/// land 後に worktree を寄せる先。
pub fn retired_path(repo: &Path, id: &str) -> PathBuf {
    worktrees_dir(repo).join(RETIRED_DIR).join(id)
}

/// `--pr-cmd` 形で終端した便の worktree を、merge の後に畳む（設計 §5.4）。
///
/// **`detail=pr` を前提にしない**。squash 形で move だけが落ちた便（land は rc 0 のまま
/// stderr 1 行で終わる）を後追いで畳む口にもなるので、見るのは永続面の事実——worktree が
/// 在るか・clean か——だけである。**forge へ問うのは PR の便だけ**（段が `Landed` で `RunDone` の detail が `pr`・
/// [`pr_retire`]）で、他の対象は今のまま畳むだけである（forge にも台帳にも問わない）。
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
    match is_pr_landed(entry) {
        Err(outcome) => outcome,
        Ok(true) => pr_retire(entry),
        Ok(false) => plain_retire(entry),
    }
}

/// 段が `Landed` で、その `RunDone` の detail が `pr` の便か（経路 (3) を通る対象・event log を読めない周は rc 2）。
fn is_pr_landed(entry: &Retire<'_>) -> Result<bool, Outcome> {
    let landed = entry.stage == Stage::Landed;
    if !landed {
        return Ok(false);
    }
    let events = store::read_all(entry.state_dir).map_err(|errors| {
        Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect())
    })?;
    Ok(events.iter().any(|event| {
        event.run == entry.run
            && event.kind == EventKind::RunDone
            && event.stage == Some(Stage::Landed)
            && event.detail.as_deref() == Some(LANDED_PR)
    }))
}

/// PR の便以外の対象を畳むだけで畳む（forge にも台帳にも問わない・前提違反は rc 1 + stderr 1 行）。
fn plain_retire(entry: &Retire<'_>) -> Outcome {
    let worktree = worktree_path(entry.repo, entry.run);
    if !worktree.is_dir() {
        // 2 度目の retire もここで止まる（1 度目が畳んでいるので元の場所に無い）。
        return refused(format!("run {} の worktree {} が無い", entry.run, worktree.display()));
    }
    let check = WorktreeCheck::judge(&worktree);
    if !check.is_clean() {
        return refused(format!("run {} の worktree が clean でない（{}）", entry.run, check.as_str()));
    }
    fold(entry, &worktree)
}

/// worktree を可逆に move して `detail=retired` を 1 件記す（畳むだけの周の本体・move の失敗は rc 2）。
fn fold(entry: &Retire<'_>, worktree: &Path) -> Outcome {
    if let Some(failure) = move_and_record(entry, worktree) {
        return failure;
    }
    Outcome::ok_line(format!("run={} retired={}", entry.run, retired_path(entry.repo, entry.run).display()))
}

/// move して `retired` を記す。失敗は畳めなかった周の Outcome（rc 2）で、成功は `None`。
fn move_and_record(entry: &Retire<'_>, worktree: &Path) -> Option<Outcome> {
    let failures = retire_worktree(entry.repo, entry.run, worktree);
    if !failures.is_empty() {
        // **畳めていないのに「畳んだ」を記帳しない**（永続面と event が食い違う）。
        return Some(Outcome { out: Vec::new(), err: failures, rc: RC_BROKEN });
    }
    record(entry, EventKind::RunStage, "retired").err().map(broken)
}

/// 便の段のまま event を 1 件記す（終端を動かさない）。
fn record(entry: &Retire<'_>, kind: EventKind, detail: &str) -> Result<(), String> {
    emit(
        entry.state_dir,
        &Emit {
            kind,
            run: entry.run,
            bead: entry.bead,
            stage: Some(entry.stage),
            seat: None,
            pid: None,
            detail: Some(detail.to_owned()),
        },
        entry.policy,
    )
    .map_err(|err| err.to_string())
}

/// PR の便を照合してから close し、worktree を畳む（設計 contract-source.md §61・判定の順は形の番号どおり）。
///
/// 通らない周は stdout の 1 行 `run=<id> retire=<語>` と rc 1 で、event も台帳も書かない（close の後の move の
/// 失敗だけ `terminal:close:ok` の 1 件が残る）。
fn pr_retire(entry: &Retire<'_>) -> Outcome {
    let worktree = worktree_path(entry.repo, entry.run);
    if !worktree.is_dir() || !WorktreeCheck::judge(&worktree).is_clean() {
        return declined(entry, Refusal::WorktreeUnready);
    }
    if entry.fold_only {
        return fold(entry, &worktree);
    }
    match already_closed(entry) {
        Err(refusal) => return declined(entry, refusal),
        Ok(true) => return fold(entry, &worktree),
        Ok(false) => {}
    }
    let proof = match prove(entry) {
        Ok(found) => found,
        Err(refusal) => return declined(entry, refusal),
    };
    close_and_fold(entry, &worktree, &proof)
}

/// 通らない周の出力（stdout 1 行・rc 1・何も書かない）。
fn declined(entry: &Retire<'_>, refusal: Refusal) -> Outcome {
    Outcome { out: vec![format!("run={} retire={}", entry.run, refusal.word())], err: Vec::new(), rc: RC_REFUSED }
}

/// 便の bead が台帳で closed かつ `close_reason` の頭の語が `landed` か（台帳を 1 回読む・読めない周は `unmeasured`）。
fn already_closed(entry: &Retire<'_>) -> Result<bool, Refusal> {
    let timeout = timeout_of(entry.manifest).ok_or(Refusal::Unmeasured)?;
    let issues = read_ledger(entry.bd, entry.repo, timeout).map_err(|_| Refusal::Unmeasured)?;
    Ok(issues.iter().any(|issue| {
        issue.id == entry.bead
            && issue.status == LEDGER_CLOSED
            && issue.close_reason.split_whitespace().next() == Some(CLOSE_REASON)
    }))
}

/// 宣言の remote・forge・git・先端の CI の順に問う（1 つでも通らなければ理由 1 つ）。
fn prove(entry: &Retire<'_>) -> Result<Proof, Refusal> {
    let facts = terminal_facts(entry.repo).map_err(|_| Refusal::Unmeasured)?;
    let remote = facts.remote.ok_or(Refusal::Unmeasured)?;
    let merge = match pr_merge(entry.repo, &branch_name(entry.run)) {
        PrMerge::Merged(found) => found,
        PrMerge::NotMerged => return Err(Refusal::NotMerged),
        PrMerge::Unmeasured => return Err(Refusal::Unmeasured),
    };
    let tip = remote_tip(entry.repo, &remote)?;
    ensure_ancestor(entry.repo, &merge, &tip)?;
    match ci_read(entry.repo, &tip, &facts.ci_cmd) {
        CiRead::Success => Ok(Proof { merge, tip }),
        CiRead::Failure | CiRead::Pending => Err(Refusal::CiNotSuccess),
        CiRead::Unmeasured => Err(Refusal::Unmeasured),
    }
}

/// remote の main の先端の読み（**閉じた 3 値**・照合と着地の前の取り込みが同じ 1 本を通る・設計 pipeline.md §69 形 2）。
pub(in crate::pipe) enum RemoteTip {
    /// 先端の commit id（object は取った後）。
    Found(String),
    /// remote に main が無い（初めての push の前）。
    NoMain,
    /// 読めない・取れない（ls-remote・fetch・先端の object のどれかが落ちた周）。
    Unread,
}

/// remote の main の先端を読み、object を取る（ls-remote → fetch → `cat-file -e` の 3 手）。
pub(in crate::pipe) fn read_tip(repo: &Path, remote: &str) -> RemoteTip {
    let Some(listed) = git_bytes(repo, &["ls-remote", remote, MAIN_REF]) else {
        return RemoteTip::Unread;
    };
    let text = String::from_utf8_lossy(&listed);
    let found = text.lines().filter_map(|line| line.split_once(char::is_whitespace)).find(|(_, name)| name.trim() == MAIN_REF);
    let Some((tip, _)) = found else {
        return RemoteTip::NoMain;
    };
    let fetch = ["fetch", "--no-tags", "--no-write-fetch-head", remote, MAIN_REF];
    if !is_oid(tip) || !git_ok(repo, &fetch) || !git_ok(repo, &["cat-file", "-e", &format!("{tip}^{{commit}}")]) {
        return RemoteTip::Unread;
    }
    RemoteTip::Found(tip.to_owned())
}

/// [`read_tip`] を照合の戻りに写す（無い周と読めない周は同じ `unmeasured`）。
fn remote_tip(repo: &Path, remote: &str) -> Result<String, Refusal> {
    match read_tip(repo, remote) {
        RemoteTip::Found(tip) => Ok(tip),
        RemoteTip::NoMain | RemoteTip::Unread => Err(Refusal::Unmeasured),
    }
}

/// merge の commit が先端の祖先か（object が無い周と rc 1 は `not-ancestor`・他の rc は `unmeasured`）。
fn ensure_ancestor(repo: &Path, merge: &str, tip: &str) -> Result<(), Refusal> {
    if !git_ok(repo, &["cat-file", "-e", &format!("{merge}^{{commit}}")]) {
        return Err(Refusal::NotAncestor);
    }
    let ran = crate::invocation::Invocation::new("git")
        .arg("-C")
        .arg(repo)
        .args(["merge-base", "--is-ancestor", merge, tip])
        .output();
    match ran.map(|out| out.status.code()) {
        Ok(Some(0)) => Ok(()),
        Ok(Some(1)) => Err(Refusal::NotAncestor),
        _ => Err(Refusal::Unmeasured),
    }
}

/// 40 桁の 16 進か。
fn is_oid(text: &str) -> bool {
    text.len() == OID_LEN && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// 台帳を close して `terminal:close:ok` を記し、worktree を畳む（close が落ちた周は畳まない）。
fn close_and_fold(entry: &Retire<'_>, worktree: &Path, proof: &Proof) -> Outcome {
    let tail = CloseTail::CiSuccess((proof.tip != proof.merge).then_some(proof.tip.as_str()));
    let reason = close_reason(&proof.merge, tail);
    if crate::ledger::close(entry.bd, entry.repo, entry.bead, &reason).is_err() {
        return declined(entry, Refusal::Unwritten);
    }
    // **局面の出力の書き直し（契機 (d)）は close の Ok の後**（設計 case-lifecycle.md §12 約束 8）: 呼び手の rc と stdout は変えず、
    // `Written`・`Unchanged`・`Coalesced` の外の語だけ stderr の 1 行にする。
    let place = Place { state_dir: entry.state_dir, repo: entry.repo, manifest: entry.manifest, bd: entry.bd, policy: entry.policy };
    let lifecycle_lines = lifecycle::after_close(&place);
    let mut outcome = fold_after_close(entry, worktree);
    outcome.err.extend(lifecycle_lines);
    outcome
}

/// close の後に `terminal:close:ok` を記して worktree を畳む（[`close_and_fold`] の後半）。
fn fold_after_close(entry: &Retire<'_>, worktree: &Path) -> Outcome {
    if let Err(reason) = record(entry, EventKind::RunDone, CLOSE_OK) {
        return broken(reason);
    }
    let dest = retired_path(entry.repo, entry.run);
    if dest.exists() || !retire_worktree(entry.repo, entry.run, worktree).is_empty() {
        return declined(entry, Refusal::WorktreeUnready);
    }
    if let Err(reason) = record(entry, EventKind::RunStage, "retired") {
        return broken(reason);
    }
    Outcome::ok_line(format!("run={} retired={} close=ok", entry.run, dest.display()))
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
