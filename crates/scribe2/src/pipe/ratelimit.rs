//! rate-limit の途中再開の列（設計 account-autonomy.md §4・FR37・`pipe::cli` の `run` / `resume` から呼ぶ）。
//!
//! **env も HOME も読まない**（憲法 C2.2）。置き場と規則の値は `pipe::cli` と同じ口から解く。

use super::approve::RC_BLOCKED;
use super::cli::{broken, flag, refused, resolve, stage_of, state_dir_of, turn_of, Extra};
use super::follow;
use super::{current, runner_is_idle};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK};
use crate::fleet::select::Selection;
use crate::fleet::store::{LockPolicy, StoreError};
use crate::fleet::{self, Completion, Stage, Timeout};
use crate::rules::manifest::Manifest;
use std::path::Path;
use std::time::Duration;

/// `RateLimited` の便を別口座で起こし直す経路（設計 account-autonomy.md §4・ADR-0020 §2.3・FR37）。
///
/// 段が `RateLimited` である間、[`resume_rate_limited`] の 1 周（計測 → 選定 → 起こし直し）を
/// 繰り返す。**回数の上限を持たない**（起こし直した turn がまた上限で止まれば次の口座で続く・窓を
/// 跨ぐ）。終端は `pipe stop --run` だけで、器は自動では終端しない。段が `RateLimited` でない周は
/// 何もせず rc 0（`pipe run` が起動の直後に通す形）。
pub(super) fn ride_out_rate_limit(
    args: &[String],
    id: &str,
    runner: &str,
    manifest: &Manifest,
    policy: LockPolicy,
) -> Outcome {
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let labels = match declared_labels(manifest, &state_dir) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let mut outcome = Outcome::ok(Vec::new());
    loop {
        // 置き場を読めない周は rc 2（読めなさを「上限ではない」に読み替えて gate へ流さない）。
        let stage = match current(&state_dir) {
            Ok(state) => state.runs.get(id).map(|run| run.stage),
            Err(errors) => {
                outcome.err.extend(errors.iter().map(StoreError::to_string));
                outcome.rc = RC_BROKEN;
                return outcome;
            }
        };
        if stage != Some(Stage::RateLimited) {
            return outcome;
        }
        let turn = resume_rate_limited(args, id, runner, &labels, policy);
        outcome.out.extend(turn.out);
        outcome.err.extend(turn.err);
        if turn.rc != RC_OK {
            outcome.rc = turn.rc;
            return outcome;
        }
    }
}

/// 便の口座の宣言（設計 account-lifecycle.md §2「読み手」・ADR-0026 §2.1）: tracked の面（`--rules` か埋め込み）の
/// label に置き場の host の面（`<state_dir>/host.toml`）の label を足す（[`crate::rules::declared_labels`]・計測
/// `fleet usage` と同じ宣言を読む）。**pipe で口座の宣言を読む口はこの 1 本**。host の面が在るが読めない周は断る
/// （FailClosed・0 口座に潰さない）。
fn declared_labels(manifest: &Manifest, state_dir: &Path) -> Result<Vec<String>, String> {
    let tracked: Vec<String> = manifest
        .accounts()
        .iter()
        .map(|account| account.label().to_owned())
        .collect();
    crate::rules::declared_labels(&tracked, state_dir).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
            .join(" / ")
    })
}

/// `RateLimited` の便の 1 周（設計 account-autonomy.md §4）: (i) FR33 の計測を 1 回撃つ →
/// (ii) 便用の規則で口座を選ぶ → (iii) `Chosen` なら同じ worktree・契約・base の runner をその口座で
/// 起こし直す / (iv) 候補なしなら最も早い reset まで唯一の wait で待ち、成立なら (ii) から・`Timeout`
/// なら (i) から。
///
/// 判定行は `run=<id> next=spawn account=<label>` / `run=<id> next=wait reset=<ts>`（既存の
/// `next=gate` と同型）。計測の行は stderr 側（`fleet select` と同じ）。走っている runner の隣に
/// もう 1 つ起こさない（起きている周は断る・fail-closed）。
fn resume_rate_limited(
    args: &[String],
    id: &str,
    runner: &str,
    labels: &[String],
    policy: LockPolicy,
) -> Outcome {
    let resolved = match resolve(args, id, &[Stage::RateLimited], &Extra::Nothing) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    if runner_is_idle(&resolved.state_dir, id) != Some(true) {
        return refused(format!("run {id} の runner が起きている（隣にもう 1 つ起こさない）"));
    }
    let usage_args = match usage_args(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let mut outcome = Outcome::ok(Vec::new());
    let label = loop {
        // (i) 計測。口座ごとの失敗は `AllowanceUnmeasured` の行のまま（FailOpen・fleet-usage.md §6）で、
        // command を止めるのは引数・manifest・store の誤りだけ（その rc をそのまま返し、選ばない）。
        let measured = fleet::usage::run(&usage_args, &resolved.state_dir);
        if measured.rc != RC_OK {
            outcome.err.extend(measured.out.into_iter().chain(measured.err));
            outcome.rc = measured.rc;
            return outcome;
        }
        outcome.err.extend(measured.out.into_iter().chain(measured.err));
        // (ii) 選定（待ちが成立した周はここから撃ち直す＝計測は待ちの観測が読んだ行のまま）。
        match choose_or_wait(id, &resolved.state_dir, labels, &mut outcome) {
            Ok(Some(label)) => break label,
            Ok(None) => {}
            // 止まる周も、それまでの判定行（`next=wait …`）と計測の行は残す。
            Err(stopped) => {
                outcome.out.extend(stopped.out);
                outcome.err.extend(stopped.err);
                outcome.rc = stopped.rc;
                return outcome;
            }
        }
    };
    // (iii) 起こし直し。**経路は通常の起動と同じ `spawn_turn` の 1 本**（C6）。
    outcome.out.push(format!("run={id} next=spawn account={label}"));
    let turn = follow::spawn_turn(&turn_of(id, &resolved, runner, policy), Some(&label));
    outcome.out.extend(turn.out);
    outcome.err.extend(turn.err);
    outcome.rc = turn.rc;
    outcome
}

/// 選定と待ち（(ii) / (iv)）。`Ok(Some)` は選んだ label、`Ok(None)` は `Timeout`（計測から撃ち直す）、
/// `Err` はこの process が止まる周（便は `RateLimited` のまま live）。
///
/// 候補なしで **reset を持たない周**（測れない・除外で空）は待つ時刻が無いので rc 3 で止まる
/// （便は終端にしない・次の `resume` で選び直す）。待ちの間に段が動いた周（`pipe stop --run` が
/// 終端した等）は段違いとして断る＝起こし直さない。
fn choose_or_wait(
    id: &str,
    state_dir: &Path,
    labels: &[String],
    outcome: &mut Outcome,
) -> Result<Option<String>, Outcome> {
    loop {
        let state = current(state_dir).map_err(|errors| {
            Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect())
        })?;
        let stage = stage_of(&state, id).map_err(refused)?;
        if stage != Stage::RateLimited {
            return Err(refused(format!("run {id} の段は {} である（待ちの間に動いた）", stage.as_str())));
        }
        let found = match fleet::select_for_run(&state, labels, &fleet::cli::now_utc()) {
            Selection::Chosen(label) => return Ok(Some(label)),
            Selection::None(found) => found,
        };
        let Some(reset) = found.earliest_reset else {
            return Err(Outcome {
                out: vec![format!("run={id} next=wait reset=-")],
                err: vec![format!(
                    "pipe: run {id} は口座待ちである（候補なし: {}・待つ reset が無い）",
                    found.reason.as_str()
                )],
                rc: RC_BLOCKED,
            });
        };
        // deadline は reset 時刻から計算した値（rules 行ではない・縮退を持たない）。
        let Some(deadline) = until(&reset) else {
            return Err(broken(format!("run {id} の待ち先 reset {reset} を時刻として読めない")));
        };
        outcome.out.push(format!("run={id} next=wait reset={reset}"));
        let waited = fleet::wait(
            Completion::AccountFree {
                reset_at: reset,
                state_dir: state_dir.to_path_buf(),
                run: id.to_owned(),
                labels: labels.to_vec(),
            },
            deadline,
        );
        match waited {
            Ok(()) => {}
            Err(Timeout) => return Ok(None),
        }
    }
}

/// いまから `reset`（UTC の `YYYY-MM-DDTHH:MM:SSZ`）までの長さ。過ぎていれば 0。読めない形は `None`。
fn until(reset: &str) -> Option<Duration> {
    let target = fleet::epoch_of(reset)?;
    let now = fleet::epoch_of(&fleet::cli::now_utc())?;
    Some(Duration::from_secs(target.saturating_sub(now)))
}

/// 計測（[`fleet::usage::run`]）へ渡す引数: `--rules` と `--curl` だけを写す（他の pipe の flag は
/// 渡さない）。値欠けは黙って落とさず断る（NFR4）。
fn usage_args(args: &[String]) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    for name in ["--rules", "--curl"] {
        if let Some(value) = flag(args, name)? {
            found.extend([name.to_owned(), value.to_owned()]);
        }
    }
    Ok(found)
}
