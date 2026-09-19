//! 起動と連鎖（`pipe spawn` / `pipe run`・設計 §5「subcommand」）。
//!
//! runner を起こす経路は [`launch`] 1 本で、`pipe run` は intake → 審査 → spawn → ride_out → gate → land を
//! 1 process で連続させる。`s2-07l.295` で `cli.rs` から純移動した（本文は不変・連鎖の順序は宣言順のまま）。
//! `resume` と `follow_pending` は `cli.rs` に残る（run 1 の裁定 (b)・契約表 row c の閉包を保つ）。
//! **起こせるのは `Reviewed` かつ verdict PASS の便だけ**（FR49・[`Extra::Spawn`]・設計 contract-source.md §4）。

use super::intake::{intake_id, intake_line};
use super::step::{gate_run, land_run};
use super::{manifest_of, need, refused, resolve, review_then_launch, Extra, Resolved};
use crate::cli_outcome::{Outcome, RC_OK};
use crate::fleet::store::LockPolicy;
use crate::fleet::Stage;
use crate::pipe::follow::{self, Runner, Turn};
use crate::pipe::ratelimit::{ride_out_rate_limit, Pool};
use crate::rules::manifest::Manifest;

/// `pipe spawn`。前提 stage = `Reviewed`（verdict PASS）。
pub(super) fn start(args: &[String], policy: LockPolicy) -> Outcome {
    let parsed = (|| Ok::<_, String>((need(args, "--run")?.to_owned(), need(args, "--runner")?.to_owned())))();
    let (id, runner) = match parsed {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    launch(args, &id, &runner, policy, &[Stage::Reviewed])
}

/// 段を確かめてから turn の口を通す。
///
/// **runner を起こす経路はここ 1 本**で、材料を解いた後は `pipe::follow` の turn へ渡す
/// （Precheck → spawn → 追随の後始末が 1 本に収まる＝起こし直しと通常の起動で後始末が
/// 分かれない）。`Reviewed` の便は verdict が PASS の周だけ通る（[`Extra::Spawn`]・他の段は段の一致だけ）。
///
/// 口座は器が選ぶ（設計 account-autonomy.md §4「初回の起動も同じ選定を通す」・FR36）: 口座の宣言（`--rules` の
/// tracked の面 + 置き場の host の面・[`Pool::declared`]）が 1 つ以上在る周は `RateLimited` の再開と同じ 1 関数で
/// label を選び、0 の周は親の環境を継承する。操作役に口座を選ばせる flag は無い。待ちの間に便が居るはずの段は
/// 解いた現在の段（[`Resolved::stage`]）。manifest は `resume` の各段の口（`cli.rs` の `relaunch`）が渡さないので、
/// `--rules`（無ければ埋め込み）から同じ 1 本（[`manifest_of`]）で読み直す。
pub(super) fn launch(
    args: &[String],
    id: &str,
    runner: &str,
    policy: LockPolicy,
    allowed: &[Stage],
) -> Outcome {
    let resolved = match resolve(args, id, allowed, &Extra::Spawn) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let pool = match manifest_of(args).and_then(|manifest| Pool::declared(args, &manifest, &resolved.state_dir)) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let runner = Runner { cmd: runner, pool: pool.as_ref() };
    follow::spawn_selected(&turn_of(id, &resolved, runner, policy), resolved.stage)
}

/// turn の材料を解いた面から組む（起動と別口座での起こし直しが同じ 1 本で組む）。
pub(in crate::pipe) fn turn_of<'a>(id: &'a str, resolved: &'a Resolved, runner: Runner<'a>, policy: LockPolicy) -> Turn<'a> {
    Turn {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        contract: &resolved.contract,
        runner: Some(runner),
        approved: resolved.approved,
        policy,
    }
}

/// `pipe run`。intake → 審査 → spawn → gate → land を 1 process で連続させる。
///
/// 各段は永続面を読み書きするので、途中で落ちても `resume` が続きを引ける。
pub(super) fn run_all(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let runner = match need(args, "--runner") {
        Ok(found) => found.to_owned(),
        Err(reason) => return refused(reason),
    };
    let id = match intake_id(args, manifest, policy) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    // **driver の札をここで置く**（設計 dispatcher.md §5）: この process が死んだら、札が残って列の
    // 1 周が起こし直す。`Drop` で消えるので、どの段で終わっても残らない。
    let _driver = super::state_dir_of(args).ok().and_then(|state_dir| crate::pipe::Driver::hold(&state_dir, &id));
    // **run id は落ちた周も stdout に出す**。`resume` がこの id を要るためで、
    // ここで黙ると続きから引けない便が置き場に残る。
    let mut lines = vec![intake_line(args, &id)];
    // 審査が PASS でない周は spawn の前で止まる（構築点の呼出 0・AC22）。
    let spawned = review_then_launch(args, &id, &runner, manifest, policy);
    if let Some(stopped) = chain(&mut lines, spawned) {
        return stopped;
    }
    // runner が口座の上限で止まった周は別口座で起こし直してから gate へ（設計 account-autonomy.md §4）。
    let ridden = ride_out_rate_limit(args, &id, &runner, manifest, policy);
    if let Some(stopped) = chain(&mut lines, ridden) {
        return stopped;
    }
    let gated = gate_run(args, &id, manifest, policy);
    if let Some(stopped) = chain(&mut lines, gated) {
        return stopped;
    }
    let landed = land_run(args, &id, manifest, policy);
    if let Some(stopped) = chain(&mut lines, landed) {
        return stopped;
    }
    Outcome::ok(lines)
}

/// 段の結果を畳む。rc≠0 ならそこまでの行を載せて**止める形**を返す。
pub(super) fn chain(lines: &mut Vec<String>, outcome: Outcome) -> Option<Outcome> {
    if outcome.rc == RC_OK {
        lines.extend(outcome.out);
        return None;
    }
    let mut stopped = outcome;
    let mut out = std::mem::take(lines);
    out.extend(stopped.out);
    stopped.out = out;
    Some(stopped)
}
