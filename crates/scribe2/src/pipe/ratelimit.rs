//! rate-limit の途中再開の列と、便用の口座の選定（設計 account-autonomy.md §3 / §4・FR36 / FR37・`pipe::cli` の
//! `run` / `resume` と `pipe::follow` から呼ぶ）。
//!
//! **env も HOME も読まない**（憲法 C2.2）。置き場と規則の値は `pipe::cli` と同じ口から解く。
//!
//! 便用の選定（計測 → 選定 → 候補なしの待ち）は [`choose_account`] の **1 関数**で、初回の起動・承認後・回答後・
//! 衝突の起こし直し・上限で止まった便の再開の**すべて**がそれを通る（規則を器の外の散文〔launcher の固定 list〕で
//! 運ばない・憲法 N2 / C2 / C9.2）。

use super::approve::RC_BLOCKED;
use super::cli::{broken, flag, refused, resolve, stage_of, state_dir_of, turn_of, Extra};
use super::follow::{self, Runner, Turn};
use super::spawn::Account;
use super::{current, runner_is_idle};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK};
use crate::fleet::select::{Model, Selection};
use crate::fleet::store::{LockPolicy, StoreError};
use crate::fleet::{self, Completion, Stage, Timeout};
use crate::headless::ROW_MODEL;
use crate::rules::manifest::Manifest;
use crate::rules::str_row;
use std::path::Path;
use std::time::Duration;

/// 便用の選定の入力（設計 account-autonomy.md §3・`s2-07l.297` / `s2-07l.285`）: 計測の引数と、置き場を持たない
/// 宣言値（口座 label の列・便が使う model）。
///
/// model は rules 行 `runner.model` の値の**字面のまま**運ぶ（型にするのは選定の `counts` の中）。runner / lens が
/// 同じ行の model を `--model` で毎回明示するので、便が消費するのはその model のモデル別窓だけ＝選定もその窓だけを
/// 数える（他の model の窓が 100 でも候補から外さない・設計 account-autonomy.md §3）。
pub struct Pool {
    /// 計測（[`fleet::usage::run`]）へ渡す引数（`--rules` / `--curl` の写し・[`usage_args`]）。
    args: Vec<String>,
    /// manifest の `[[account]]` の label 列（tracked + host の面・[`declared_labels`]）。
    labels: Vec<String>,
    /// rules 行 `runner.model` の値。
    model: String,
}

impl Pool {
    /// 初回の起動（`pipe::cli::run::launch`）と衝突の起こし直し（`pipe land`）の入力: 宣言した口座が **1 つ以上
    /// 在る周だけ** `Some`。0 の周は `None`（runner は親の環境を継承・model の行も読まない＝口座を宣言しない
    /// 置き場に `runner.model` の行を要求しない）。宣言の読み手は [`declared_labels`] の 1 本。
    pub(in crate::pipe) fn declared(args: &[String], manifest: &Manifest, state_dir: &Path) -> Result<Option<Self>, String> {
        let labels = declared_labels(manifest, state_dir)?;
        if labels.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self { args: usage_args(args)?, labels, model: runner_model_of(manifest)?.to_owned() }))
    }
}

/// `RateLimited` の便を別口座で起こし直す経路（設計 account-autonomy.md §4・ADR-0020 §2.3・FR37）。
///
/// 段が `RateLimited` である間、[`resume_rate_limited`] の 1 周（計測 → 選定 → 起こし直し）を
/// 繰り返す。**回数の上限を持たない**（起こし直した turn がまた上限で止まれば次の口座で続く・窓を
/// 跨ぐ）。終端は `pipe stop --run` だけで、器は自動では終端しない。段が `RateLimited` でない周は
/// 何もせず rc 0（`pipe run` が起動の直後に通す形）。
///
/// 例外は 1 つ——runner が死んだ便（`Spawned` の段で runner が起きていない＝`pipe resume` が生死を唯一の wait で
/// 測って `SeatStopped detail=runner-dead` を記帳した後・設計 §4「runner が死んだ便の起こし直し」）も**同じ 1 周**
/// を通す（起こし直しの経路を増やさない・C6）。その turn がまた上限で止まれば上と同じ loop で続く。
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
        // `Spawned` は runner が起きていない周（死んで記帳済み）だけ通す。`pipe run` の直後は turn の結果の段に
        // 居るので `Spawned` には来ない＝従来の「RateLimited でなければ返る」は不変。
        let expected = if stage == Some(Stage::RateLimited) {
            Stage::RateLimited
        } else if stage == Some(Stage::Spawned) && runner_is_idle(&state_dir, id) == Some(true) {
            Stage::Spawned
        } else {
            return outcome;
        };
        // 便が使う model は上限で止まった周にだけ要る（`pipe run` は段が動かない周にこの行を読まない）。
        // 行が無い / 不発効 / 文字列でない / 閉じた表に無い周は typed に断り、claude を呼ばず再開もしない。
        // 宣言が 0 の置き場でも組む（候補なしを名乗って口座待ちで止まる＝初回の起動の「継承」とは違う）。
        let pool = match runner_model_of(manifest).and_then(|model| {
            Ok(Pool { args: usage_args(args)?, labels: labels.clone(), model: model.to_owned() })
        }) {
            Ok(found) => found,
            Err(reason) => return refused(reason),
        };
        let turn = resume_rate_limited(args, id, Runner { cmd: runner, pool: Some(&pool) }, policy, expected);
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

/// rules 行 `runner.model` の値（runner / lens が `--model` で毎回明示する model・設計 pipeline.md §6）。読み手は
/// [`str_row`] の 1 本（headless と同じ）で、値は閉じた表（[`Model::parse`]）で検査してから**字面のまま**返す
/// （選定へ渡す値を書き換えない・型にするのは選定の中）。表に無い値は行が無いのと同じ typed な断り。
fn runner_model_of(manifest: &Manifest) -> Result<&str, String> {
    let text = str_row(manifest, ROW_MODEL)?;
    if Model::parse(text).is_none() {
        return Err(format!("{ROW_MODEL} の値 {text} は未知の model である"));
    }
    Ok(text)
}

/// 途中再開の 1 周（設計 account-autonomy.md §4）: [`choose_account`]（計測 → 選定 → 候補なしの
/// 待ち）で口座を選び、同じ worktree・契約・base の runner をその口座で起こし直す。`stage` は便が居る段
/// （上限で止まった `RateLimited`・runner が死んだ `Spawned`）で、待ちの間もその段に居ることを求める。
///
/// 判定行は `run=<id> next=spawn account=<label>` / `run=<id> next=wait reset=<ts>`（既存の
/// `next=gate` と同型）。計測の行は stderr 側（`fleet select` と同じ）。走っている runner の隣に
/// もう 1 つ起こさない（起きている周は断る・fail-closed）。
fn resume_rate_limited(
    args: &[String],
    id: &str,
    runner: Runner<'_>,
    policy: LockPolicy,
    stage: Stage,
) -> Outcome {
    let resolved = match resolve(args, id, &[stage], &Extra::Nothing) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    if runner_is_idle(&resolved.state_dir, id) != Some(true) {
        return refused(format!("run {id} の runner が起きている（隣にもう 1 つ起こさない）"));
    }
    let mut outcome = Outcome::ok(Vec::new());
    // 宣言が 0 でも選定を通す（候補なし → 口座待ちで止まる・親の環境を継承して起こし直さない）。宣言 0 の周も
    // 呼び手が `pool` を組んで渡す（`None` は無い＝初回の起動の「継承」とは違う）。
    let Some(pool) = runner.pool else {
        return refused(format!("run {id} の再開に便用の選定の入力が要る"));
    };
    // turn の材料は選定の**前に**組む——便の repo（`Turn.repo`）は選定の除外の鍵（設計 account-autonomy.md §14・
    // その repo を anchor に持つ席の口座だけを外す）で、起こし直しと同じ 1 本（`turn_of`）から取る。
    let entry = turn_of(id, &resolved, runner, policy);
    let label = match choose_account(pool, &entry, stage, &mut outcome) {
        Ok(label) => label,
        // 止まる周も、それまでの判定行（`next=wait …`）と計測の行は残す。
        Err(stopped) => {
            outcome.out.extend(stopped.out);
            outcome.err.extend(stopped.err);
            outcome.rc = stopped.rc;
            return outcome;
        }
    };
    // (iii) 起こし直し。**経路は通常の起動と同じ `spawn_turn` の 1 本**（C6）。
    outcome.out.push(format!("run={id} next=spawn account={label}"));
    let turn = follow::spawn_turn(&entry, Account::Resumed(&label));
    outcome.out.extend(turn.out);
    outcome.err.extend(turn.err);
    outcome.rc = turn.rc;
    outcome
}

/// 便用の口座の選定（**1 関数**・設計 account-autonomy.md §3 / §4「初回の起動も同じ選定を通す」・FR36）:
/// (i) FR33 の計測を 1 回撃つ → (ii) 便用の規則（[`fleet::select_for_run`]・便の `repo` を anchor に持つ席の登録
/// row の口座は除外・設計 §14）で選ぶ → (iii) 候補なしなら最も早い reset まで唯一の wait で待ち、成立なら (ii)
/// から・`Timeout` なら (i) から。
///
/// `Ok` は選んだ label。`Err` はこの process が止まる周（計測の引数・manifest・store の誤り、待つ reset の
/// 無い候補なし、待ちの間に段が動いた）で、便の段は動かさない。口座の宣言が 0 の周（[`Pool::declared`] が
/// `None`）の分岐は呼び手（`follow::spawn_selected`）が持ち、ここへは来ない（再開は宣言 0 でも通す＝候補なし）。
///
/// `entry` は起こす turn の材料（読むのは便 id・置き場・便の repo＝`Turn.repo`・`pipe run --repo` の値・起こし直しと
/// 同じ 1 本 `turn_of` で組む）。`expected` は待ちの間に便が居るはずの段（再開なら `RateLimited`・runner が死んだ便の
/// 起こし直しなら `Spawned`・初回なら起動前の段・衝突の起こし直しなら `Implemented`）。待ちの観測
/// （[`Completion::AccountFree`]）も同じ repo と段を運ぶ＝段が動いた周は待ちから抜けて断る。
pub(super) fn choose_account(
    pool: &Pool,
    entry: &Turn<'_>,
    expected: Stage,
    outcome: &mut Outcome,
) -> Result<String, Outcome> {
    loop {
        // (i) 計測。口座ごとの失敗は `AllowanceUnmeasured` の行のまま（FailOpen・fleet-usage.md §6）で、
        // command を止めるのは引数・manifest・store の誤りだけ（その rc をそのまま返し、選ばない）。
        let measured = fleet::usage::run(&pool.args, entry.state_dir);
        if measured.rc != RC_OK {
            return Err(Outcome::failed(measured.rc, measured.out.into_iter().chain(measured.err).collect()));
        }
        outcome.err.extend(measured.out.into_iter().chain(measured.err));
        // (ii) 選定（待ちが成立した周はここから撃ち直す＝計測は待ちの観測が読んだ行のまま）。
        if let Some(label) = choose_or_wait(entry, pool, expected, outcome)? {
            return Ok(label);
        }
    }
}

/// gate の lens 1 本のための選定の結果（設計 account-autonomy.md §15・**待ちを持たない**）。
pub(super) enum LensAccount {
    /// 選んだ口座の label。
    Chosen(String),
    /// 候補なし（理由は選定が名乗る閉じた字面・[`fleet::select::NoCandidateReason::as_str`]）。
    None(String),
}

/// lens 1 本の口座の選定（設計 account-autonomy.md §15 (1)(4)・FR36 / FR33）。
///
/// 便の起動（[`choose_account`]）と同じ 2 段——(i) FR33 の計測を 1 回撃つ → (ii) 便用の規則
/// （[`fleet::select_for_run`]・便の `repo` を anchor に持つ席の口座は除外・§14）——を通すが、**候補なしで
/// 待たない**（[`LensAccount::None`] を返す）: gate は段の判定で待ちを持たず、候補なしの周は lens を起こさず
/// INCONCLUSIVE へ倒れて `resume` が撃ち直す（`AccountFree` の待ちは §4 の runner 側だけ）。
///
/// `Err` は測れなかった周（計測の rc≠0・置き場を読めない）で、呼び手は判定に届かなかった側へ倒す
/// （fail-closed・PASS には決してならない）。計測の行（`fleet select` と同じ stderr 側）は `notes` へ写す。
pub(super) fn select_lens_account(
    pool: &Pool,
    state_dir: &Path,
    repo: &Path,
    notes: &mut Vec<String>,
) -> Result<LensAccount, String> {
    let measured = fleet::usage::run(&pool.args, state_dir);
    if measured.rc != RC_OK {
        let lines: Vec<String> = measured.out.into_iter().chain(measured.err).collect();
        return Err(format!("計測が rc {} で終わった（{}）", measured.rc, lines.join(" / ")));
    }
    notes.extend(measured.out.into_iter().chain(measured.err));
    let state = current(state_dir).map_err(|errors| {
        errors.iter().map(StoreError::to_string).collect::<Vec<String>>().join(" / ")
    })?;
    let found = fleet::select_for_run(&state, repo, &pool.labels, Some(&pool.model), &fleet::cli::now_utc());
    Ok(match found {
        Selection::Chosen(label) => LensAccount::Chosen(label),
        Selection::None(none) => LensAccount::None(none.reason.as_str().to_owned()),
    })
}

/// 選定と待ち（(ii) / (iii)）。`Ok(Some)` は選んだ label、`Ok(None)` は `Timeout`（計測から撃ち直す）、
/// `Err` はこの process が止まる周（便は `expected` の段のまま live）。
///
/// 候補なしで **reset を持たない周**（測れない・除外で空）は待つ時刻が無いので rc 3 で止まる
/// （便は終端にしない・次の `resume` で選び直す）。待ちの間に段が動いた周（`pipe stop --run` が
/// 終端した等）は段違いとして断る＝起こさない。
fn choose_or_wait(
    entry: &Turn<'_>,
    pool: &Pool,
    expected: Stage,
    outcome: &mut Outcome,
) -> Result<Option<String>, Outcome> {
    let (id, state_dir, repo) = (entry.run, entry.state_dir, entry.repo);
    loop {
        let state = current(state_dir).map_err(|errors| {
            Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect())
        })?;
        let stage = stage_of(&state, id).map_err(refused)?;
        if stage != expected {
            return Err(refused(format!("run {id} の段は {} である（待ちの間に動いた）", stage.as_str())));
        }
        // 便用の選定は便が使う model の窓だけを数え、除外は便の repo を anchor に持つ席の口座だけ（待ちの観測
        // `AccountFree` も同じ model と repo を運ぶ・C3.4）。
        let found = match fleet::select_for_run(&state, repo, &pool.labels, Some(&pool.model), &fleet::cli::now_utc()) {
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
                repo: repo.to_path_buf(),
                run: id.to_owned(),
                expected,
                labels: pool.labels.clone(),
                model: Some(pool.model.clone()),
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
