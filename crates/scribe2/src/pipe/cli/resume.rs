//! `pipe resume`（設計 §5「subcommand の置き場」・`s2-07l.349` で `cli.rs` から純移動・本文は不変）。
//!
//! 現在の段から続きの段だけを通す（[`resume`]）。起こし直し（[`relaunch`]）と審査つきの起動
//! （[`review_then_launch`]・`pipe run` と共有・外から呼ぶ path は `cli` の再輸出で不変）、追随の続きの弁別
//! （[`follow_pending`]）もここに置く。

use super::run::{chain, launch};
use super::step::{gate_run, land_run, review_run};
use super::{need, refused, stage_of, state_dir_of};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::store::{LockPolicy, StoreError};
use crate::fleet::Stage;
use crate::pipe::approve::RC_BLOCKED;
use crate::pipe::follow;
use crate::pipe::gate::{Verdict, RC_INCONCLUSIVE};
use crate::pipe::land::verdict_of;
use crate::pipe::ratelimit::ride_out_rate_limit;
use crate::pipe::{current, last_stage_detail, question_of_run, runner_is_idle};
use crate::rules::manifest::Manifest;
use std::path::Path;

/// `pipe resume`。現在の段から続きの段だけを通す。
pub(super) fn resume(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let id = match need(args, "--run") {
        Ok(found) => found.to_owned(),
        Err(reason) => return refused(reason),
    };
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
    };
    match stage_of(&state, &id) {
        Err(reason) => refused(reason),
        // `Implemented` の先は 2 つに分かれる（設計 pipeline-conflict.md §3・ADR-0019 §2.6）。
        // 追随が衝突して段が戻った便（最後の `RunStage` の detail が `rebase-conflict:` で
        // 始まり、runner が起きていない）は**起こし直しの続き**で、`--runner` を要る。
        // それ以外の `Implemented` は従来どおり gate。
        Ok(Stage::Implemented) => match follow_pending(&state_dir, &id) {
            false => gate_run(args, &id, manifest, policy),
            true => relaunch(args, &id, policy, Stage::Implemented),
        },
        // Gated の先は判定で分かれる。**INCONCLUSIVE は land を試さない**——測れて
        // いない便に land の「PASS でない」を返すのは、吸収状態を言い換えただけである。
        // 次に撃つ段だけを名乗って rc 3 で止まる（**自動では測り直さない**＝道具の
        // 不足は人が直す）。PASS / FAIL の弁別は land 側が持ち、読む関数は
        // [`verdict_of`] の 1 本で共有する（判定の読み手は増やさない）。
        Ok(Stage::Gated) => match verdict_of(&state_dir, &id) {
            Some(Verdict::Inconclusive) => Outcome {
                out: vec![format!("run={id} next=gate")],
                err: Vec::new(),
                rc: RC_INCONCLUSIVE,
            },
            _ => land_run(args, &id, manifest, policy),
        },
        // 審査を通っていない便（`RunCreated` の直後に process が落ちた周）は**先に審査**し、PASS の周だけ
        // 起こす（FR49・設計 contract-source.md §4「効き方」）。審査の段の event と `review.json` はここで残る。
        Ok(Stage::Intake) => match need(args, "--runner") {
            Err(reason) => refused(reason),
            Ok(runner) => review_then_launch(args, &id, runner, manifest, policy),
        },
        // `Reviewed` から起こせるのは verdict が PASS の周だけ（[`super::Extra::Spawn`] が弁別する）。
        Ok(Stage::Reviewed) => relaunch(args, &id, policy, Stage::Reviewed),
        // Blocked から先へ進めるのは承認 event が在る周だけ。未承認は **rc 3 のまま
        // 何も書かない**——待っている事実は既に Blocked が記帳しており、resume の
        // たびに ApprovalRequested を積むと「何回聞いたか」が事実と食い違う。
        Ok(Stage::Blocked) => match state.runs.get(&id).is_some_and(|run| run.approved) {
            false => Outcome::failed_line(
                RC_BLOCKED,
                format!("pipe: run {id} は承認待ちである（pipe approve --words \"<user の逐語>\"）"),
            ),
            true => relaunch(args, &id, policy, Stage::Blocked),
        },
        // Questioned から先へ進めるのは**最新の質問への回答**が在る周だけ（`Blocked` と同型・
        // FR32）。無ければ rc 3 で何も書かない（待っている事実は Questioned が既に持つ）。
        Ok(Stage::Questioned) => match question_of_run(&state_dir, &id).is_some_and(|q| q.answer.is_some()) {
            false => Outcome::failed_line(
                RC_BLOCKED,
                format!("pipe: run {id} は回答待ちである（pipe answer --run {id} --words \"<回答の逐語>\"）"),
            ),
            true => relaunch(args, &id, policy, Stage::Questioned),
        },
        // 上限で止まった便は器が別口座を選んで起こし直す（設計 account-autonomy.md §4・FR37）。人の
        // 操作は要らない（候補なしは reset まで待つ・終端は stop だけ）。
        Ok(Stage::RateLimited) => match need(args, "--runner") {
            Err(reason) => refused(reason),
            Ok(runner) => ride_out_rate_limit(args, &id, runner, manifest, policy),
        },
        Ok(stage) => refused(format!("run {id} の段 {} からは再開しない", stage.as_str())),
    }
}

/// `--runner` を読んで、その段の便を起こし直す（`resume` の各段が共有する形・`--runner` 欠けは rc 1）。
fn relaunch(args: &[String], id: &str, policy: LockPolicy, stage: Stage) -> Outcome {
    match need(args, "--runner") {
        Err(reason) => refused(reason),
        Ok(runner) => launch(args, id, runner, policy, &[stage]),
    }
}

/// 審査（`Intake` → `Reviewed`）を通してから起こす（`pipe run` と `resume` が共有する 1 本・FR49）。
///
/// 審査が PASS でない周はその判定行と rc（FAIL = 1 / INCONCLUSIVE = 3）で止まり、**runner を起こさない**。
/// PASS の周だけ [`launch`] へ進む（[`super::Extra::Spawn`] が `review.json` を読み直す＝判定の読み手は 1 本）。
pub(super) fn review_then_launch(
    args: &[String],
    id: &str,
    runner: &str,
    manifest: &Manifest,
    policy: LockPolicy,
) -> Outcome {
    let mut lines = Vec::new();
    if let Some(stopped) = chain(&mut lines, review_run(args, id, manifest, policy)) {
        return stopped;
    }
    let spawned = launch(args, id, runner, policy, &[Stage::Reviewed]);
    chain(&mut lines, spawned).unwrap_or_else(|| Outcome::ok(lines))
}

/// `Implemented` の便が**起こし直しの続き**か（設計 pipeline-conflict.md §3 の `resume`）。
///
/// 条件は 2 つ——最後の `RunStage` の detail が `rebase-conflict:` で始まり、かつ runner が
/// 起きていない（走っている runner の隣にもう 1 つ起こさない）。どちらかを読めない周は
/// `false`＝従来どおり gate へ流す（読めなさで runner を起こさない・fail-closed）。
fn follow_pending(state_dir: &Path, id: &str) -> bool {
    let conflicted = last_stage_detail(state_dir, id)
        .is_some_and(|detail| follow::is_conflict(&detail));
    conflicted && runner_is_idle(state_dir, id) == Some(true)
}
