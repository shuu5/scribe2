//! 便の状態の helper（設計 §5「subcommand の置き場」・`s2-07l.349` で `cli.rs` から純移動・本文は不変）。
//!
//! 段の生死（[`live`]）・replay の段（[`stage_of`]）・段の前提の解き（[`resolve`] とその弁別 [`discriminate`] /
//! [`gated_is`]）・`--run` の読み（[`by_run`]）。材料の型（`Resolved` / `Extra`）は親の `cli` が持つ（構築点を
//! 動かさない）。外から呼ぶ path は `cli` の再輸出で不変（`super::resolve` 等）。

use super::intake::run_repo;
use super::{need, refused, state_dir_of, Extra, Resolved};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::store::StoreError;
use crate::fleet::{Stage, State};
use crate::pipe::contract::Contract;
use crate::pipe::follow;
use crate::pipe::gate::Verdict;
use crate::pipe::land::{verdict_of, REBASE_EMPTY};
use crate::pipe::review::ReviewCheck;
use crate::pipe::{contract_path, current, last_stage_detail};
use std::path::Path;

/// 便が live（終端でない）か。**段の網羅 match で書く**（段が増えたら compile で気付く）。
///
/// 終端 = `Landed` / `Failed` / `Stopped`、または `Gated` で verdict が FAIL（pipeline.md §4
/// 「FAIL は終端」）、または `Reviewed` で verdict が PASS でない（contract-source.md §4「FAIL / INCONCLUSIVE は
/// 終端」）。`RateLimited` は終端でない（口座の窓の都合で止まっただけ・ADR-0020 §2.1）。`Gated` / `Reviewed` の
/// 判定を読めない周は `None`＝**測れなかった**で、呼び手が断る側へ倒す（読めない判定を「終端でない」にも
/// 「終端」にも読み替えない）。
pub(in crate::pipe) fn live(state_dir: &Path, id: &str, stage: Stage) -> Option<bool> {
    match stage {
        Stage::Landed | Stage::Failed | Stage::Stopped => Some(false),
        Stage::Gated => verdict_of(state_dir, id).map(|found| found != Verdict::Fail),
        Stage::Reviewed => ReviewCheck::judge(state_dir, id).live(),
        Stage::Intake
        | Stage::Blocked
        | Stage::Spawned
        | Stage::Questioned
        | Stage::RateLimited
        | Stage::Implemented => Some(true),
    }
}

/// 前提の段を replay から読む。無ければ `Err`。
pub(in crate::pipe) fn stage_of(state: &State, id: &str) -> Result<Stage, String> {
    state
        .runs
        .get(id)
        .map(|run| run.stage)
        .ok_or(format!("run {id} が無い"))
}

/// 段の前提を確かめ、材料を永続面から解く。**3 つの段（spawn / gate / land）が共有する**。
///
/// **順序を変えない**: 置き場 → replay → 段 → 契約 → repo。段の検査を契約より後ろへ
/// 動かすと、段違いの周に契約の error（rc 2）が先に出て「前提違反は何もせず rc 1」が
/// 崩れる（event も 1 件も書かない、という不変条件はこの順序に乗っている）。
///
/// [`Extra`] は段だけでは決まらない周の弁別を頼む印である。この弁別も段の検査の一部ゆえ
/// **契約より前**に置く——外へ出すと「段違いなのに rc 2」が特定の段だけで起こり、上の
/// 不変条件が rc の語彙ごと崩れる（lens 実測 F1）。
pub(in crate::pipe) fn resolve(
    args: &[String],
    id: &str,
    allowed: &[Stage],
    extra: &Extra,
) -> Result<Resolved, Outcome> {
    let state_dir = state_dir_of(args).map_err(refused)?;
    let state = current(&state_dir).map_err(|errors| {
        Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect())
    })?;
    let stage = stage_of(&state, id).map_err(refused)?;
    if !allowed.contains(&stage) {
        return Err(refused(format!("run {id} の段は {} である", stage.as_str())));
    }
    discriminate(extra, &state_dir, id, stage)?;
    let Some(run) = state.runs.get(id) else {
        return Err(refused(format!("run {id} が無い")));
    };
    let contract = Contract::load(&contract_path(&state_dir, id)).map_err(|errors| {
        Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect())
    })?;
    let repo = run_repo(args, &state_dir, id).map_err(refused)?;
    Ok(Resolved {
        state_dir,
        repo,
        contract,
        bead: run.bead.clone(),
        stage,
        approved: run.approved,
    })
}

/// 段の中の弁別。外れは**段違いと同じ扱い**＝rc 1 で何も書かない（event も contract も触らない）。
///
/// - `Gated`: **測り直せるのは「測れなかった」周だけ**。PASS / FAIL は判定に届いた終端で、
///   判定が読めない周（file 不在 / 壊れ / 3 値の外）も測り直さない（fail-closed・C11.2）。
/// - `Failed`: **畳めるのは `rebase-empty` と `rebase-conflict` の周だけ**（前者は変更が既に
///   main に在る＝close してよい合図・後者は起こし直しの上限に達した便で、planner が契約を
///   切り直して流し直す・設計 pipeline-conflict.md §5）。他の理由（`main-red` /
///   `main-unmeasured` / `rebase-dirty` / `precheck:…`）は人が読む前に入れ物が動くと
///   「何が起きたか」を現物から追えなくなるので断る。理由を読めない周も断る（読めなかったを
///   畳める理由に読み替えない・fail-closed）。
/// - `Gated`: **畳めるのは verdict が FAIL の周だけ**（判定に届いた終端・`.132` の memo）。
///   PASS はまだ land が残っており、INCONCLUSIVE は測り直せる側ゆえ断る。
/// - `Reviewed`: **起こせるのは verdict が PASS の周だけ**（FR49・設計 contract-source.md §4「効き方」）。
///   FAIL / INCONCLUSIVE は終端で、判定を読めない周も起こさない（fail-closed・[`ReviewCheck`]）。
///
/// **理由も名乗る**: 段違いの一般則で断っている事実と、その便が通らない理由は別の情報で、
/// 片方だけだと読み手に届かない。
fn discriminate(extra: &Extra, state_dir: &Path, id: &str, stage: Stage) -> Result<(), Outcome> {
    match (extra, stage) {
        (&Extra::Regate, Stage::Gated) => gated_is(state_dir, id, Verdict::Inconclusive),
        (&Extra::Retire, Stage::Gated) => gated_is(state_dir, id, Verdict::Fail),
        (&Extra::Spawn, Stage::Reviewed) => match ReviewCheck::judge(state_dir, id) {
            ReviewCheck::Passed => Ok(()),
            found => Err(refused(format!("run {id} の段は Reviewed である（verdict={}）", found.as_str()))),
        },
        (&Extra::Retire, Stage::Failed) => {
            let detail = last_stage_detail(state_dir, id);
            let foldable = detail.as_deref().is_some_and(|found| {
                found == REBASE_EMPTY || found == follow::EXHAUSTED
            });
            match foldable {
                true => Ok(()),
                false => Err(refused(format!(
                    "run {id} の段は Failed である（detail={}）",
                    detail.as_deref().unwrap_or("読めない")
                ))),
            }
        }
        (&Extra::Nothing | &Extra::Regate | &Extra::Retire | &Extra::Spawn, _) => Ok(()),
    }
}

/// `Gated` の便の判定が求める 3 値か。**判定を読めない周は断る**（fail-closed・C11.2）。
fn gated_is(state_dir: &Path, id: &str, want: Verdict) -> Result<(), Outcome> {
    let verdict = verdict_of(state_dir, id);
    match verdict == Some(want) {
        true => Ok(()),
        false => Err(refused(format!(
            "run {id} の段は Gated である（verdict={}）",
            verdict.map_or("読めない", Verdict::as_str)
        ))),
    }
}

/// `--run` を読んでから段の関数へ渡す。
pub(super) fn by_run(args: &[String], step: impl FnOnce(&str) -> Outcome) -> Outcome {
    match need(args, "--run") {
        Err(reason) => refused(reason),
        Ok(id) => step(id),
    }
}
