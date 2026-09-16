//! `pipe resume`（設計 §5「subcommand の置き場」・`s2-07l.349` で `cli.rs` から純移動・本文は不変）。
//!
//! 現在の段から続きの段だけを通す（[`resume`]）。起こし直し（[`relaunch`]）と審査つきの起動
//! （[`review_then_launch`]・`pipe run` と共有・外から呼ぶ path は `cli` の再輸出で不変）、追随の続きの弁別
//! （[`follow_pending`]）もここに置く。

use super::run::{chain, launch};
use super::step::{gate_run, land_run, review_run};
use super::{broken, need, refused, stage_of, state_dir_of};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::{self, LockPolicy, StoreError};
use crate::fleet::{self, Completion, EventKind, Stage, Timeout};
use crate::pipe::approve::RC_BLOCKED;
use crate::pipe::follow;
use crate::pipe::gate::{Verdict, RC_INCONCLUSIVE};
use crate::pipe::land::verdict_of;
use crate::pipe::ratelimit::ride_out_rate_limit;
use crate::pipe::{current, emit, last_stage_detail, question_of_run, runner_is_idle, Emit};
use crate::rules::manifest::Manifest;
use std::path::Path;
use std::time::Duration;

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
        // `Spawned` から再開できるのは **runner が死んだ便だけ**（host の再起動・OOM・kill で `SeatStopped` が
        // 書かれないまま消えた形・設計 account-autonomy.md §4「runner が死んだ便の起こし直し」・C9）。生死は
        // 唯一の wait で測り、生きている便は断る（runner を 2 本にしない）。
        Ok(Stage::Spawned) => match need(args, "--runner") {
            Err(reason) => refused(reason),
            Ok(runner) => revive(args, &id, runner, manifest, policy),
        },
        Ok(stage) => refused(format!("run {id} の段 {} からは再開しない", stage.as_str())),
    }
}

/// `Spawned` の便の runner の生死を測り、死んでいれば起こし直す（設計 account-autonomy.md §4・FR37 / AC39）。
///
/// 測るのは最後の `SeatSpawned` の pid で、唯一の wait（[`Completion::SeatGone`]・deadline 0）に問う——`Timeout` が
/// 「生きている」で、typed に断って runner を 2 本にしない（判定行 `run=<id> runner=alive pid=<pid>`・rc 1・event 0 件・
/// C3.3 / C3.4）。`SeatSpawned` / pid が無い周も断る（測れないを「死んだ」に読み替えない・fail-closed）。死んでいれば
/// `SeatStopped detail=runner-dead`（pid 付き）を 1 件記帳してから、上限の周と同じ起こし直しの 1 本
/// （[`ride_out_rate_limit`]: 計測 → 便用の選定 → `spawn_turn`）へ流す。未 commit の file は消さない（N1）。
fn revive(args: &[String], id: &str, runner: &str, manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let events = match store::read_all(&state_dir) {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
    };
    // 最後の `SeatSpawned` の行から pid と bead を読む（席の event の原本・replay の `Run` は pid を持たない）。
    let seated = events
        .iter()
        .rev()
        .find(|event| event.run == id && event.kind == EventKind::SeatSpawned)
        .and_then(|event| Some((u32::try_from(event.pid?).ok()?, event.bead.as_str())));
    let Some((pid, bead)) = seated else {
        return refused(format!("run {id} の runner の pid を読めない（Spawned から再開できるのは runner が死んだ便だけ）"));
    };
    if fleet::wait(Completion::SeatGone(pid), Duration::ZERO) == Err(Timeout) {
        return Outcome {
            out: vec![format!("run={id} runner=alive pid={pid}")],
            err: vec![format!("pipe: run {id} の runner が起きている（pid {pid}・隣にもう 1 つ起こさない）")],
            rc: RC_REFUSED,
        };
    }
    let stopped = emit(
        &state_dir,
        &Emit {
            kind: EventKind::SeatStopped,
            run: id,
            bead,
            stage: None,
            seat: Some(id.to_owned()),
            pid: Some(u64::from(pid)),
            detail: Some(follow::RUNNER_DEAD.to_owned()),
        },
        policy,
    );
    if let Err(err) = stopped {
        return broken(err.to_string());
    }
    ride_out_rate_limit(args, id, runner, manifest, policy)
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
