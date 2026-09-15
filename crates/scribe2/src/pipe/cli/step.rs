//! 段の手（`pipe approve` / `answer` / `gate` / `land` / `retire`・設計 §5「subcommand」）。
//!
//! どの手も段の前提を [`super::resolve`] で確かめてから、段の本体（`pipe::approve` / `pipe::gate` /
//! `pipe::land`）へ渡す。`s2-07l.295` で `cli.rs` から純移動した（本文は不変・各段の手順は宣言順のまま）。
//! 規則の値は rules 行から読む（数値を焼かない・C1 / C5）。

use super::{broken, flag, int_row, list_row, need, refused, resolve, state_dir_of, Extra};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::store::{LockPolicy, StoreError};
use crate::fleet::Stage;
use crate::pipe::approve::{Approve, RC_BLOCKED};
use crate::pipe::current;
use crate::pipe::declaration::{self, Ceiling, CEILING_ROW, DENIED_ROW};
use crate::pipe::gate::{Gate, Limits};
use crate::pipe::land::{Land, Retire};
use crate::pipe::review::{review, Review};
use crate::rules::manifest::Manifest;
use std::path::Path;

/// gate が要る lens の本数を持つ rules 行。
const ROW_LENS: &str = "gate.lens_count";

/// gate の diff 上限（byte）を持つ rules 行。
const ROW_CAP: &str = "gate.token_cap";

/// 変異検査の並列度の上限を持つ rules 行（受付の宣言値）。
const ROW_MUTANTS_JOBS: &str = "gate.mutants_jobs";

/// job 1 つが要る memory（MiB）を持つ rules 行（受付の分母）。
const ROW_JOB_MEMORY: &str = "gate.job_memory_mb";

/// 席と host のために残す memory（MiB）を持つ rules 行（受付の差引）。
const ROW_RESERVE_MEMORY: &str = "host.reserve_memory_mb";

/// 受付で枠が空くのを待つ上限（秒）を持つ rules 行。
const ROW_SLOT_WAIT: &str = "gate.slot_wait_s";

/// 追随が衝突した便を起こし直す回数の上限を持つ rules 行。
const ROW_RETRIES: &str = "pipe.follow_retries";

/// land が着地待ちの列で自分の番を待つ上限（秒）を持つ rules 行（設計 gate-cost.md §6）。
const ROW_LAND_WAIT: &str = "pipe.land_wait_s";

/// `pipe approve`。**逐語を event へ写すだけ**で、段は動かさない（resume が進める）。
pub(super) fn approve_run(args: &[String], id: &str, policy: LockPolicy) -> Outcome {
    let words = match need(args, "--words") {
        Ok(found) => found.to_owned(),
        Err(reason) => return refused(reason),
    };
    // 段は問わない（承認は「これから起こすこと」への許しで、遅れて来ても記帳する）が、
    // 便が在ることは確かめる＝無い run へ承認を書くと宛先の無い記録が残る。
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Ok(found) => found,
        Err(errors) => {
            return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect())
        }
    };
    let Some(run) = state.runs.get(id) else {
        return refused(format!("run {id} が無い"));
    };
    super::approve::approve(&Approve {
        run: id,
        bead: &run.bead,
        state_dir: &state_dir,
        words: &words,
        policy,
    })
}

/// `pipe answer`。**`Questioned` の run にだけ**逐語を event へ写す（段は動かさない・resume が進める）。
///
/// 承認（[`approve_run`]）と同型だが、段は問う——質問の無い便へ回答を書くと、後で来た質問の
/// 関門が前の回答で開く。段違いは `Blocked` の未承認と同じ **rc 3 で何も書かない**。
pub(super) fn answer_run(args: &[String], id: &str, policy: LockPolicy) -> Outcome {
    let words = match need(args, "--words") {
        Ok(found) => found.to_owned(),
        Err(reason) => return refused(reason),
    };
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Ok(found) => found,
        Err(errors) => {
            return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect())
        }
    };
    let Some(run) = state.runs.get(id) else {
        return refused(format!("run {id} が無い"));
    };
    if run.stage != Stage::Questioned {
        return Outcome::failed_line(
            RC_BLOCKED,
            format!("pipe: run {id} は質問で止まっていない（段 {}）", run.stage.as_str()),
        );
    }
    super::approve::answer(&Approve {
        run: id,
        bead: &run.bead,
        state_dir: &state_dir,
        words: &words,
        policy,
    })
}

/// 規則から gate の線（判定の 2 行と受付の 4 行）を読む。**数値を .rs へ焼かない**（憲法 C1 / C5）。
///
/// 受付の 4 行も `--rules` の manifest から読む（埋め込みから直に読まない）——待ちの上限を
/// 振る歯が fixture の値を gate へ届ける口はここだけである。
fn limits_of(manifest: &Manifest) -> Result<Limits, String> {
    Ok(Limits {
        lens_count: int_row(manifest, ROW_LENS)?,
        token_cap: int_row(manifest, ROW_CAP)?,
        mutants_jobs: int_row(manifest, ROW_MUTANTS_JOBS)?,
        job_memory_mb: int_row(manifest, ROW_JOB_MEMORY)?,
        reserve_memory_mb: int_row(manifest, ROW_RESERVE_MEMORY)?,
        slot_wait_s: int_row(manifest, ROW_SLOT_WAIT)?,
    })
}

/// 契約の審査の段（`pipe intake` の直後・`resume` の `Intake`・前提 stage = `Intake`・FR49・設計
/// contract-source.md §4）。
///
/// lens は gate と同じ `--lens`（無ければ INCONCLUSIVE＝終端・fail-closed）。要件面の path は HEAD の宣言から
/// 読む（`contracts check` と同じ読み口・無ければ既定）。読めない周は判定に届かず rc 2（判定を書かない）。
pub(super) fn review_run(args: &[String], id: &str, manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let resolved = match resolve(args, id, &[Stage::Intake], &Extra::Nothing) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let lens = match flag(args, "--lens") {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let requirements = match requirements_of(&resolved.repo, manifest) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    review(&Review {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        contract: &resolved.contract,
        requirements: &requirements,
        lens,
        policy,
    })
}

/// 要件面の repo 相対 path（HEAD の宣言 `requirements`・無ければ既定・[`declaration::table_facts`] の 1 本）。
fn requirements_of(repo: &Path, manifest: &Manifest) -> Result<String, String> {
    let commands = list_row(manifest, CEILING_ROW)?;
    let denied = list_row(manifest, DENIED_ROW)?;
    let ceiling = Ceiling { row: CEILING_ROW, commands: &commands, denied: &denied };
    declaration::table_facts(repo, &ceiling)
        .map(|facts| facts.requirements)
        .map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<String>>().join(" / "))
}

/// `pipe gate`。前提 stage = `Implemented` ∨ (`Gated` ∧ verdict が INCONCLUSIVE)。
///
/// **測り直せるのは「測れなかった」周だけ**である。INCONCLUSIVE は道具が足りなくて
/// 判定に届かなかった印（`--lens` 無し / diff が cap 超 / lens の不備）なので、道具を
/// 揃えれば同じ便を撃ち直せる。PASS / FAIL は判定に届いた周ゆえ**終端のまま**で、
/// 段違いの一般則どおり何もせず rc 1 を返す——FAIL から撃ち直す口を開けると、契約の
/// verify が赤い便が「壊れたまま進む」経路になる。
pub(super) fn gate_run(args: &[String], id: &str, manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let resolved = match resolve(args, id, &[Stage::Implemented, Stage::Gated], &Extra::Regate) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let limits = match limits_of(manifest) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let lens = match flag(args, "--lens") {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    super::gate::gate(&Gate {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        contract: &resolved.contract,
        lens,
        limits,
        policy,
    })
}

/// `pipe land`。前提 stage = Gated（PASS の検査は land 側が持つ）。
///
/// `--pr-cmd` は自 repo への PR の口ゆえ**承認 event を前提としない**（A4.3・ADR-0008）。
/// `--lens` と規則の線は main が動いた便の追随（rebase → gate の撃ち直し・設計 §5.4）で
/// gate へ渡すために読む（land 自身は数値を見ない）。
pub(super) fn land_run(args: &[String], id: &str, manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let resolved = match resolve(args, id, &[Stage::Gated], &Extra::Nothing) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let limits = match limits_of(manifest) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let pr_cmd = match flag(args, "--pr-cmd") {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let lens = match flag(args, "--lens") {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    // 追随が衝突した周は実装役を起こし直す（設計 pipeline-conflict.md §3）。`pipe run` は
    // 自分の runner をそのまま渡し、`--runner` を持たない `pipe land` は起こし直せない
    // ——衝突の記帳だけ残して断り、`pipe resume --runner` で続けられる。
    let runner = match flag(args, "--runner") {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let retries = match int_row(manifest, ROW_RETRIES) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    // 着地の順番を待つ上限（設計 gate-cost.md §6）。`--rules` の manifest から読む＝上限を振る歯の
    // fixture が land へ届く口はここだけである。
    let land_wait_s = match int_row(manifest, ROW_LAND_WAIT) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    super::land::land(&Land {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        contract: &resolved.contract,
        pr_cmd,
        lens,
        limits,
        runner,
        retries,
        land_wait_s,
        approved: resolved.approved,
        policy,
    })
}

/// `pipe retire`。前提 stage = `Landed` ∨ (`Failed` ∧ 最後の `RunStage` の detail が
/// `rebase-empty` / `rebase-conflict`) ∨ (`Gated` ∧ verdict が FAIL)（worktree 在り・clean の
/// 検査は retire 側が持つ）。
///
/// **段を動かさない口である**。`--pr-cmd` 形の便は main を動かさず worktree も残して
/// `Landed` で終端するので、merge の後に入れ物だけを畳む段が要る。同一変更の便が
/// `rebase-empty` で終端した周も**成果は既に main に在る**ので入れ物だけが残る形は同じで、
/// 畳める側に数える（`s2-07l.128`）。起こし直しの上限に達した便（`rebase-conflict`）と
/// 判定に届いた `Gated(FAIL)` も、終端して入れ物だけが残る形は同じである（設計
/// pipeline-conflict.md §5）。走っている便・他の理由で落ちた便を通すと「まだ読まれて
/// いない現物を動かす」経路になるため、段違いは一般則どおり rc 1。
///
/// 残す event の段は [`super::Resolved::stage`] のまま＝**`Landed` に決め打ちしない**（終端を動かさない）。
pub(super) fn retire_run(args: &[String], id: &str, policy: LockPolicy) -> Outcome {
    let allowed = [Stage::Landed, Stage::Failed, Stage::Gated];
    let resolved = match resolve(args, id, &allowed, &Extra::Retire) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    super::land::retire(&Retire {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        stage: resolved.stage,
        policy,
    })
}
