//! `pipe stop` の停止の列（設計 pipeline.md §5・`pipe::cli` の `dispatch` から呼ぶ）。
//!
//! **env も HOME も読まない**（憲法 C2.2）。置き場と規則の値は `pipe::cli` と同じ口から解く。

use super::cli::{broken, flag, int_row, live, refused, state_dir_of};
use super::confine::{self, Reaped};
use super::regate::REASON_FLAG;
use super::{current, emit, is_stopping, Emit, STOPPING};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::{LockPolicy, StoreError};
use crate::fleet::{self, Completion, EventKind, SeatState, Stage, State};
use crate::rules::manifest::Manifest;
use std::path::Path;
use std::time::Duration;

/// 停止の猶予を持つ rules 行。
const ROW_GRACE: &str = "pipe.stop_grace_ms";

/// `pipe stop`。`--run <id>` は便 1 本を外し、`--all` は生きている席を全部止める。
///
/// **2 つの口の意味は別である**: `--all` は席の掃除（対象なしは rc 0 の冪等）、`--run` は
/// 放置された便を排他の母集団から外す管理席の操作（設計 pipeline-conflict.md §2）。
///
/// `--reason` は `--all` の側だけが受ける（設計 pipeline.md §51 形 2）。`--run` と同時に渡す周は使い方の誤りとして
/// 断る（1 本を外す操作に逐語の行き先は無い・黙って落とさない＝SRS NFR4）。
pub(super) fn stop(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let words = match flag(args, REASON_FLAG) {
        Err(reason) => return refused(reason),
        Ok(found) => found,
    };
    match flag(args, "--run") {
        Err(reason) => refused(reason),
        Ok(Some(_)) if words.is_some() => {
            refused(format!("--run と {REASON_FLAG} は同時に渡せない（{REASON_FLAG} は --all の逐語）"))
        }
        Ok(Some(id)) => stop_run(args, manifest, policy, id),
        Ok(None) => stop_all(args, manifest, policy),
    }
}

/// `pipe stop --run <id>`。終端でない便 1 本に `RunStopped` を書く（席が Live なら先に group 宛てに
/// 止める・**止め切れなかった周は `RunStopped` を書かず rc 1**＝run は live のまま）。席の後に、札が生きた運転手を
/// 指す周はその process group も止める（設計 pipeline.md §39・順序は 席 → 運転手 → `RunStopped`）。
///
/// **終端の便には event を増やさず rc 1**（書込は冪等・rc は冪等でない）。2 回撃った 2 件目が
/// この経路に落ちる＝events.jsonl は 1 件しか増えない。判定を読めない `Gated` は rc 2 で断る
/// （読めない周を「終端でない」に読み替えない・fail-closed）。
fn stop_run(args: &[String], manifest: &Manifest, policy: LockPolicy, id: &str) -> Outcome {
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
        Ok(found) => found,
    };
    let Some(run) = state.runs.get(id) else {
        return refused(format!("run {id} が無い"));
    };
    match live(&state_dir, id, run.stage) {
        None => return broken(format!("run {id} の判定を読めない（終端かを測れない）")),
        Some(false) => {
            return refused(format!("run {id} は既に終端である（段 {}）", run.stage.as_str()))
        }
        Some(true) => {}
    }
    let grace = match int_row(manifest, ROW_GRACE) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    // **pid を持たない Live 席も母集団に数える**（`--all` と同じ理由＝止めていない席を
    // 黙って落とすと「全部止めた」に化ける）。止められた席にだけ `SeatStopped` を書く。
    let live_seats: Vec<(String, Option<u64>)> = state
        .seats
        .values()
        .filter(|seat| seat.state == SeatState::Live && seat.run == id)
        .map(|seat| (seat.id.clone(), seat.pid))
        .collect();
    // **最初の signal を送る前に停止中の印を書く**（設計 pipeline.md §23）。runner の消滅を見た spawn は
    // 印を読んで段を書かない（stop の kill を oom-kill に化けさせない）。signal を送らない周（pid 付きの
    // 席が 0）は書かない。止め切れなかった前の周の印が残る便には 2 度書かない。
    let signals = live_seats.iter().any(|(_, pid)| pid.is_some());
    if signals && is_stopping(&state_dir, id) != Some(true) {
        if let Err(err) = record_stopping(&state_dir, &state, id, run.stage, policy) {
            return broken(err);
        }
    }
    let mut stopped = 0_usize;
    for (seat, pid) in &live_seats {
        if !pid.is_some_and(|found| terminate(found, grace)) {
            continue;
        }
        stopped = stopped.saturating_add(1);
        if let Err(err) = record_seat_stop(&state_dir, &state, (seat, id, *pid), None, policy) {
            return broken(err);
        }
    }
    let mut line = format!("stop: run={id} seats={} stopped={stopped}", live_seats.len());
    // **席を 1 つでも止め切れなかった周は `RunStopped` を書かない**（FailClosed・C9 / C6.2）。
    // 書くと run は終端として排他の母集団から外れるのに、その runner は走り続ける。
    if stopped != live_seats.len() {
        return unstoppable(line, live_seats.len().saturating_sub(stopped));
    }
    // **席の後・`RunStopped` の前に運転手を止める**（設計 pipeline.md §39 (2)(3)）。
    match settle_driver(&state_dir, &state, id, grace, policy) {
        Ok(false) => {}
        Ok(true) => line.push_str(" driver=stopped"),
        Err((rc, reason)) => return Outcome { out: vec![line], err: vec![reason], rc },
    }
    // **席と運転手の後・`RunStopped` の前に、作り手が死んだ scope を畳む**（設計 gate-cost.md §38 形 4 / 5）。
    line.push_str(&format!(" scopes={}", confine::reap_orphan_scopes().word()));
    if let Err(err) = record_run_stopped(&state_dir, &state, id, None, policy) {
        return broken(err);
    }
    Outcome::ok_line(line)
}

/// 運転手を止めて記録する（`Ok(true)` = 止めて `seat=driver` を記帳した・`Ok(false)` = 止める相手が居ない）。
///
/// 止め切れない周は rc 1、記帳できない周は rc 2 の `Err`（どちらも呼び手は `RunStopped` を書かない）。
fn settle_driver(
    state_dir: &Path,
    state: &State,
    id: &str,
    grace: u64,
    policy: LockPolicy,
) -> Result<bool, (u8, String)> {
    match stop_driver(state_dir, id, grace) {
        DriverStop::Absent => Ok(false),
        DriverStop::Stopped(pid) => {
            let seat = (DRIVER_SEAT, id, Some(u64::from(pid)));
            record_seat_stop(state_dir, state, seat, Some(STOPPED_BY_STOP), policy).map_err(|err| (RC_BROKEN, err))?;
            Ok(true)
        }
        DriverStop::Unstoppable(pid) => Err((RC_REFUSED, format!("pipe: 運転手 {pid} を止められない（run は終端にしない）"))),
    }
}

/// 運転手を止めた記録の席 id（`SeatStopped seat=driver`・設計 pipeline.md §39 (2)）。
const DRIVER_SEAT: &str = "driver";

/// 運転手を止めた記録の detail。
const STOPPED_BY_STOP: &str = "stopped-by-stop";

/// 運転手の止め方の結果（3 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriverStop {
    /// 止める相手が居ない（札が無い・読めない・死んでいる・札の pid が自分自身）。
    Absent,
    /// 札の pid（の group）を止めた。
    Stopped(u32),
    /// 猶予の後の KILL でも止め切れなかった。
    Unstoppable(u32),
}

/// 札が生きた運転手を指す周だけ、その process group を**席と同じ 1 関数**（[`terminate`]）で止める。
///
/// 札の pid が**自分自身**（`pipe run` の中から stop を撃つ形）の周は止めない。札が無い・読めない・死んでいる
/// 周も止めない（測れないを「止めた」に読み替えない）。
fn stop_driver(state_dir: &Path, id: &str, grace: u64) -> DriverStop {
    let Some(pid) = super::live_driver(state_dir, id) else {
        return DriverStop::Absent;
    };
    if pid == std::process::id() {
        return DriverStop::Absent;
    }
    if terminate(u64::from(pid), grace) {
        DriverStop::Stopped(pid)
    } else {
        DriverStop::Unstoppable(pid)
    }
}

/// 止め切れなかった周の形（rc 1・run は終端にしない）。
fn unstoppable(line: String, left: usize) -> Outcome {
    Outcome {
        out: vec![line],
        err: vec![format!("pipe: 止められない席が {left} 残った（run は終端にしない）")],
        rc: RC_REFUSED,
    }
}

/// `pipe stop --all`。生きている席を止める。**冪等**（対象なしは rc 0）。Live な席が指す便が 2 本以上の周は
/// `--reason` の逐語を要る（設計 pipeline.md §51）。
fn stop_all(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    stop_all_with(args, manifest, policy, &terminate, &confine::reap_orphan_scopes)
}

/// [`stop_all`] の本体。席を止める実装（`terminate`）と scope を畳む実装（`reap`）を受ける＝in-file の歯は
/// 実 signal も実 `systemctl` も撃たない stub を渡す。
fn stop_all_with(
    args: &[String],
    manifest: &Manifest,
    policy: LockPolicy,
    terminate: &dyn Fn(u64, u64) -> bool,
    reap: &dyn Fn() -> Reaped,
) -> Outcome {
    if !super::cli::present(args, "--all") {
        return refused("--all が要る".to_owned());
    }
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        // state が読めない周だけ rc 2（stop の rc 語彙 3 値）。
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
        Ok(found) => found,
    };
    let grace = match int_row(manifest, ROW_GRACE) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    // **pid を持たない Live 席も母集団に数える**。落とすと「対象なし rc 0」に化け、
    // 止まっていない席が在るのに全クリアを名乗ってしまう。
    let live: Vec<LiveSeat> = state
        .seats
        .values()
        .filter(|seat| seat.state == SeatState::Live)
        .map(|seat| (seat.id.clone(), seat.run.clone(), seat.pid))
        .collect();
    // **何かを止める前に**受付を決める（設計 pipeline.md §51 形 1）。断る周は席も events も動かさない。
    let detail = match flag(args, REASON_FLAG).and_then(|words| admit_scope(live_runs(&live), live.len(), words)) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let mut stopped = 0_usize;
    // 止めた席の便（記帳順）と、止め切れなかった席を持つ便。
    let mut stopped_runs: Vec<&str> = Vec::new();
    let mut unstopped_runs: Vec<&str> = Vec::new();
    for (id, run, pid) in &live {
        // 止められなかった席に「止めた」を記帳しない。記帳すると次の周が
        // 「対象なし」を返し、生きている席が終端として消える（偽の全クリア）。
        if !pid.is_some_and(|found| terminate(found, grace)) {
            if !unstopped_runs.contains(&run.as_str()) {
                unstopped_runs.push(run);
            }
            continue;
        }
        stopped += 1;
        if let Err(err) = record_seat_stop(&state_dir, &state, (id, run, *pid), None, policy) {
            return broken(err);
        }
        if !stopped_runs.contains(&run.as_str()) {
            stopped_runs.push(run);
        }
    }
    // 席の後・`RunStopped` の前に、作り手が死んだ scope を畳む（設計 gate-cost.md §38 形 4）。
    let scopes = reap();
    // `--run` と同じ極性: **止め切れなかった席を持つ便には `RunStopped` を書かない**。
    let ended: Vec<&str> = stopped_runs.iter().filter(|run| !unstopped_runs.contains(run)).copied().collect();
    for run in &ended {
        if let Err(err) = record_run_stopped(&state_dir, &state, run, detail.as_deref(), policy) {
            return broken(err);
        }
    }
    let fields = run_fields(&ended, &unstopped_runs);
    let line = format!("stop: seats={} stopped={stopped}{fields} scopes={}", live.len(), scopes.word());
    if stopped == live.len() {
        Outcome::ok_line(line)
    } else {
        unstoppable(line, live.len().saturating_sub(stopped))
    }
}

/// 全部止めが数える Live 席（席 id・便 id・pid）。
type LiveSeat = (String, String, Option<u64>);

/// 終端を記帳した便の欄の key（記帳順）。
const ENDED_FIELD: &str = "ended";

/// 止め切れなかった席を持つ便の欄の key。
const UNSTOPPED_FIELD: &str = "unstopped";

/// 便の終端の記帳に載せる逐語の頭（後ろは `--reason` の入力そのもの）。
const REASON_PREFIX: &str = "reason:";

/// 止めた便と止め切れなかった便の 2 欄（設計 pipeline.md §51 形 4）。**空の欄は出さない**（便 0 本の周は空文字）。
fn run_fields(ended: &[&str], unstopped: &[&str]) -> String {
    [(ENDED_FIELD, ended), (UNSTOPPED_FIELD, unstopped)]
        .iter()
        .filter(|(_, runs)| !runs.is_empty())
        .map(|(key, runs)| format!(" {key}={}", runs.join(",")))
        .collect()
}

/// Live な席の列が指す**別々の便**の本数（pure・設計 pipeline.md §51 形 1）。席数ではない。
fn live_runs(live: &[LiveSeat]) -> usize {
    let mut runs: Vec<&str> = live.iter().map(|(_, run, _)| run.as_str()).collect();
    runs.sort_unstable();
    runs.dedup();
    runs.len()
}

/// 全部止めの受付（設計 pipeline.md §51 形 1 / 3）。便が 2 本以上の周は逐語を要る。通る周は便の終端の記帳に
/// 載せる detail（逐語が在れば `reason:<逐語>`）を返す。空の逐語は本数に依らず断る（逐語を名乗る空を記帳しない）。
fn admit_scope(runs: usize, seats: usize, words: Option<&str>) -> Result<Option<String>, String> {
    match words {
        Some(found) if found.trim().is_empty() => Err(format!("{REASON_FLAG} の逐語が空である")),
        Some(found) => Ok(Some(format!("{REASON_PREFIX}{found}"))),
        None if runs >= 2 => Err(format!(
            "Live な席が指す便が {runs} 本在る（母集団: 便 {runs} 本・席 {seats}）——--all だけでは止めない。\
             --run ID で 1 本ずつ止めるか、{REASON_FLAG} WORDS で止める理由の逐語を付ける"
        )),
        None => Ok(None),
    }
}

/// TERM → 猶予だけ待つ → 残れば KILL。**待機は fleet の 1 実装を通る**（C3.4）。
///
/// 席の pid は `pipe spawn` が立てた process group の leader（= group id）なので、**group 宛て**に
/// 撃ち、group の全員が消えるのを待つ（wrapper だけが死んで子・孫が残る形を塞ぐ）。group が無い周
/// （`kill` が rc 非 0 = group leader でない旧 record の席）と pid ≤ 1 は**単一 pid** へ撃つ（互換）。
/// true を返すのは group（互換の周は pid）が消えた周だけである。
fn terminate(pid: u64, grace_ms: u64) -> bool {
    let grace = Duration::from_millis(grace_ms);
    let Ok(target) = u32::try_from(pid) else {
        return false;
    };
    if let StopPlan::Group(group) = stop_plan(pid) {
        if signal(&group.target(), "-TERM") {
            if fleet::wait(Completion::GroupGone(target), grace).is_ok() {
                return true;
            }
            signal(&group.target(), "-KILL");
            return fleet::wait(Completion::GroupGone(target), grace).is_ok();
        }
    }
    signal(&pid.to_string(), "-TERM");
    if fleet::wait(Completion::SeatGone(target), grace).is_ok() {
        return true;
    }
    signal(&pid.to_string(), "-KILL");
    fleet::wait(Completion::SeatGone(target), grace).is_ok()
}

/// 席の止め方（2 値）。**選ぶのは [`stop_plan`] ただ 1 本**で、実 signal を送らずに pin できる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopPlan {
    /// process group 宛て（group id = 席の pid）。
    Group(GroupId),
    /// 単一 pid 宛て（group 宛てにしてはならない pid）。
    Single,
}

/// group 宛ての signal の宛先。**pid ≥ 2 だけを持てる**——`kill -- -1` は user の全 process、
/// `kill -- -0` は自分の group であり、`-{pid}` の字面はこの型からしか作らない
/// （guard 1 本の短絡に依らない・2026-09-13 の事故）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GroupId(u64);

impl GroupId {
    /// pid ≤ 1 は `None`。
    fn new(pid: u64) -> Option<Self> {
        (pid >= 2).then_some(Self(pid))
    }

    /// `kill` へ渡す負の pid の字面。
    fn target(self) -> String {
        format!("-{}", self.0)
    }
}

/// 席の pid から止め方を選ぶ（pure）。
fn stop_plan(pid: u64) -> StopPlan {
    match GroupId::new(pid) {
        Some(group) => StopPlan::Group(group),
        None => StopPlan::Single,
    }
}

/// pid へ signal を送る（std に kill は無いので `kill` を撃つ）。rc 0 の周だけ true。
fn signal(target: &str, name: &str) -> bool {
    std::process::Command::new("kill")
        .arg(name)
        .arg("--")
        .arg(target)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// 席に「止めた」を記帳する。**止められた席にだけ書く**（偽の全クリアを作らない）。
///
/// 運転手を止めた記録（`seat=driver`・detail `stopped-by-stop`）も同じ形で書く（設計 pipeline.md §39 (2)）。
fn record_seat_stop(
    state_dir: &Path,
    state: &State,
    seat: (&str, &str, Option<u64>),
    detail: Option<&str>,
    policy: LockPolicy,
) -> Result<(), String> {
    let (id, run, pid) = seat;
    emit(
        state_dir,
        &Emit {
            kind: EventKind::SeatStopped,
            run,
            bead: bead_of(state, run),
            stage: None,
            seat: Some(id.to_owned()),
            pid,
            detail: detail.map(str::to_owned),
        },
        policy,
    )
    .map_err(|err| err.to_string())
}

/// 便に停止中の印を記帳する（`RunStage stage=<現段> detail=stopping`＝段は動かない・設計 pipeline.md §23）。
fn record_stopping(
    state_dir: &Path,
    state: &State,
    run: &str,
    stage: Stage,
    policy: LockPolicy,
) -> Result<(), String> {
    emit(
        state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run,
            bead: bead_of(state, run),
            stage: Some(stage),
            seat: None,
            pid: None,
            detail: Some(STOPPING.to_owned()),
        },
        policy,
    )
    .map_err(|err| err.to_string())
}

/// 便に「止めた」を記帳する（段 = `Stopped`＝終端＝排他の母集団から外れる）。
///
/// `detail` は全部止めの逐語（`reason:<逐語>`）だけが載る。`--run` の周は `None`（設計 pipeline.md §51 形 3）。
fn record_run_stopped(
    state_dir: &Path,
    state: &State,
    run: &str,
    detail: Option<&str>,
    policy: LockPolicy,
) -> Result<(), String> {
    emit(
        state_dir,
        &Emit {
            kind: EventKind::RunStopped,
            run,
            bead: bead_of(state, run),
            stage: Some(Stage::Stopped),
            seat: None,
            pid: None,
            detail: detail.map(str::to_owned),
        },
        policy,
    )
    .map_err(|err| err.to_string())
}

/// 便の bead id（replay に無ければ空）。
fn bead_of<'a>(state: &'a State, run: &str) -> &'a str {
    state.runs.get(run).map_or("", |found| found.bead.as_str())
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.253
    use super::{live_runs, stop, stop_all_with, LiveSeat, stop_plan, GroupId, Reaped, StopPlan};
    use crate::cli_outcome::{RC_OK, RC_REFUSED};
    use crate::fleet::store::{self, LockPolicy};
    use crate::fleet::{Event, EventKind};
    use crate::pipe::fixture::{append_all, event, scratch};
    use crate::rules::manifest::Manifest;
    use std::path::Path;

    /// 便 `run` の席 `seat` を pid 付きで起こした event（**実 pid ではない**・stub の kill だけが読む）。
    fn seat_up(run: &str, seat: &str, pid: u64) -> Event {
        Event { pid: Some(pid), ..event(run, EventKind::SeatSpawned, None, Some(seat), None) }
    }

    /// 置き場の `kind` の event を持つ便（物理順）。
    fn runs_of(state: &Path, kind: EventKind) -> Vec<String> {
        store::read_all(state)
            .unwrap_or_default()
            .into_iter()
            .filter(|found| found.kind == kind)
            .map(|found| found.run)
            .collect()
    }

    /// `stop --all` を stub の kill で撃つ（`stoppable` に在る pid だけが止まる）。`extra` は `--all` の後ろに足す引数。
    fn stop_with_stub(state: &Path, stoppable: &'static [u64], extra: &[&str]) -> crate::cli_outcome::Outcome {
        let args = stop_args(state, &[&["--all"], extra].concat());
        let Ok(manifest) = Manifest::embedded() else {
            return crate::cli_outcome::Outcome::failed_line(2, "埋め込みの manifest を読めない".to_owned());
        };
        let Ok(policy) = LockPolicy::from_rules(&manifest) else {
            return crate::cli_outcome::Outcome::failed_line(2, "lock 規則を読めない".to_owned());
        };
        stop_all_with(&args, &manifest, policy, &|pid, _grace| stoppable.contains(&pid), &|| Reaped::Unmeasured)
    }

    /// `stop <flags> --state-dir <state>` の引数列。
    fn stop_args(state: &Path, flags: &[&str]) -> Vec<String> {
        std::iter::once("stop")
            .chain(flags.iter().copied())
            .map(str::to_owned)
            .chain(["--state-dir".to_owned(), state.display().to_string()])
            .collect()
    }

    /// 置き場の `RunStopped` の (便, detail)（物理順）。
    fn run_stopped_details(state: &Path) -> Vec<(String, Option<String>)> {
        store::read_all(state)
            .unwrap_or_default()
            .into_iter()
            .filter(|found| found.kind == EventKind::RunStopped)
            .map(|found| (found.run, found.detail))
            .collect()
    }

    /// 席の列（`live_runs` に渡す形）。
    fn seats(pairs: &[(&str, &str)]) -> Vec<LiveSeat> {
        pairs.iter().map(|(seat, run)| ((*seat).to_owned(), (*run).to_owned(), None)).collect()
    }

    /// 逐語（fixture の他の字面〔便 id・席 id・detail の既存の語〕と衝突しない形）。
    const WORDS: &str = "host-畳み ζ9 撤収";

    /// (a) 席の列が指す**別々の便**を数える（便 0 / 1 / 2 / 3 本の 4 形）。同じ便を指す席 2 つは 1 本
    /// （席数と便数が食い違う形で測る）。
    #[test]
    fn pipe_stop_scope_counts_distinct_runs_of_live_seats() {
        let forms: [(Vec<LiveSeat>, usize); 4] = [
            (seats(&[]), 0),
            (seats(&[("s1", "run-a"), ("s2", "run-a")]), 1),
            (seats(&[("s1", "run-a"), ("s2", "run-b"), ("s3", "run-a")]), 2),
            (seats(&[("s1", "run-c"), ("s2", "run-a"), ("s3", "run-b"), ("s4", "run-c")]), 3),
        ];
        let population: Vec<(usize, usize)> = forms.iter().map(|(live, runs)| (live.len(), *runs)).collect();
        for (live, runs) in &forms {
            assert_eq!(live_runs(live), *runs, "母集団 4 形 (席数, 便数) = {population:?}: {live:?}");
        }
    }

    /// (b) 便が 2 本 live な置き場に `--all` だけ・`--all` と逐語・`--run` と逐語 の 3 形（母集団 3 形）。通るのは
    /// 2 形目だけで、断る 2 形は events が 1 件も増えず席も生きたまま（`SeatStopped` 0 件）。
    #[test]
    fn pipe_stop_scope_needs_the_reason_when_two_runs_are_live() {
        let state = scratch("stop-scope-admit");
        append_all(&state, &[seat_up("run-a", "seat-a", 9_201), seat_up("run-b", "seat-b", 9_202)]);
        let before = store::read_all(&state).unwrap_or_default().len();

        let bare = stop_with_stub(&state, &[9_201, 9_202], &[]);
        assert_eq!(bare.rc, RC_REFUSED, "1 形目（--all だけ）は断る: {:?}", bare.out);
        assert!(bare.out.is_empty(), "断る周は stdout を出さない: {:?}", bare.out);
        assert!(bare.err.iter().any(|line| line.contains("便が 2 本")), "便の本数を母集団として名指す: {:?}", bare.err);
        assert_eq!(store::read_all(&state).unwrap_or_default().len(), before, "1 形目: events は増えない");

        let manifest = Manifest::embedded().expect("埋め込みの manifest を読める");
        let policy = LockPolicy::from_rules(&manifest).expect("lock 規則を読める");
        let with_run = stop(&stop_args(&state, &["--run", "run-a", "--reason", WORDS]), &manifest, policy);
        assert_eq!(with_run.rc, RC_REFUSED, "3 形目（--run と逐語）は断る: {:?}", with_run.out);
        assert_eq!(store::read_all(&state).unwrap_or_default().len(), before, "3 形目: events は増えない");
        assert!(runs_of(&state, EventKind::SeatStopped).is_empty(), "断る 2 形の後も席は生きている");

        let worded = stop_with_stub(&state, &[9_201, 9_202], &["--reason", WORDS]);
        assert_eq!(worded.rc, RC_OK, "2 形目（--all と逐語）は通る: {:?}", worded.err);
        assert_eq!(runs_of(&state, EventKind::SeatStopped), vec!["run-a", "run-b"], "2 形目: 席を止める");
        let _ = std::fs::remove_dir_all(&state);
    }

    /// (c) 通った周の便の終端の記帳は `reason:` の後ろに入力の逐語をそのまま持つ（全便・記帳順）。空の逐語は断る。
    #[test]
    fn pipe_stop_scope_reason_lands_verbatim_on_every_run_stopped() {
        let state = scratch("stop-scope-words");
        append_all(&state, &[seat_up("run-b", "seat-1", 9_301), seat_up("run-a", "seat-2", 9_302)]);
        let blank = stop_with_stub(&state, &[9_301, 9_302], &["--reason", " "]);
        assert_eq!(blank.rc, RC_REFUSED, "空の逐語は断る: {:?}", blank.out);
        assert!(runs_of(&state, EventKind::SeatStopped).is_empty(), "空の逐語の周は何も止めない");

        let out = stop_with_stub(&state, &[9_301, 9_302], &["--reason", WORDS]);
        assert_eq!(out.rc, RC_OK, "{:?}", out.err);
        let detail = Some(format!("reason:{WORDS}"));
        assert_eq!(
            run_stopped_details(&state),
            vec![("run-b".to_owned(), detail.clone()), ("run-a".to_owned(), detail)],
            "終端の記帳は逐語をそのまま持つ"
        );
        assert_eq!(out.out, vec!["stop: seats=2 stopped=2 ended=run-b,run-a scopes=-".to_owned()], "欄は記帳順");
        let _ = std::fs::remove_dir_all(&state);
    }

    // flip-check: retroactive s2-07l.222
    /// (a) 止め切れた便が複数在る周は**全便**に `RunStopped` を 1 件ずつ書く（同じ便の 2 席目で 2 件目を書かない・
    /// 記帳済みの判定 `!` を消すと 1 件も書かれない）。席ごとに `SeatStopped` を書き、rc 0・`stopped=` は止めた数。
    /// 実 signal は撃たない（kill は stub・pid は `/proc` と無関係な字面）。
    #[test]
    fn mutant_in_pipe_stop_all_writes_run_stopped_to_every_stopped_run() {
        let state = scratch("stop-all-every");
        append_all(&state, &[seat_up("run-a", "seat-a1", 9_001), seat_up("run-a", "seat-a2", 9_002), seat_up("run-b", "seat-b", 9_003)]);
        let out = stop_with_stub(&state, &[9_001, 9_002, 9_003], &["--reason", WORDS]);
        assert_eq!(out.rc, RC_OK, "全席を止めた: {:?}", out.err);
        assert_eq!(out.out, vec!["stop: seats=3 stopped=3 ended=run-a,run-b scopes=-".to_owned()]);
        assert_eq!(runs_of(&state, EventKind::SeatStopped), vec!["run-a", "run-a", "run-b"], "止めた席ごとに 1 件");
        assert_eq!(runs_of(&state, EventKind::RunStopped), vec!["run-a", "run-b"], "止めた便の全便に 1 件ずつ");
        let _ = std::fs::remove_dir_all(&state);
    }

    // flip-check: retroactive s2-07l.222
    /// (b) 止め切れなかった席を持つ便には `RunStopped` を書かず、止め切れた便にだけ書く（filter の `!` を消すと
    /// 逆の便に書く）。止められなかった席には `SeatStopped` も書かず、rc 1・`stopped=` は止めた数だけ
    /// （`+=` を `*=` にすると 0 のまま・`==` を `!=` にすると rc が反転する）。
    #[test]
    fn mutant_in_pipe_stop_all_skips_runs_with_an_unstopped_seat() {
        let state = scratch("stop-all-partial");
        append_all(&state, &[seat_up("run-a", "seat-a1", 9_101), seat_up("run-a", "seat-a2", 9_102), seat_up("run-b", "seat-b", 9_103)]);
        let out = stop_with_stub(&state, &[9_101, 9_103], &["--reason", WORDS]);
        assert_eq!(out.rc, RC_REFUSED, "止め切れなかった席が残る: {:?}", out.err);
        assert_eq!(out.out, vec!["stop: seats=3 stopped=2 ended=run-b unstopped=run-a scopes=-".to_owned()]);
        assert_eq!(runs_of(&state, EventKind::SeatStopped), vec!["run-a", "run-b"], "止めた席にだけ書く");
        assert_eq!(runs_of(&state, EventKind::RunStopped), vec!["run-b"], "止め切れた便にだけ書く");
        let _ = std::fs::remove_dir_all(&state);
    }

    /// pid 0 / 1 は単一 pid 宛て・2 以上は group 宛て（実 signal を送らずに止め方の選択を pin する）。
    #[test]
    fn pipe_stop_group_plan_never_targets_pid_zero_or_one_as_group() {
        assert_eq!(stop_plan(0), StopPlan::Single, "pid 0 を group 宛てにしない（自分の group）");
        assert_eq!(stop_plan(1), StopPlan::Single, "pid 1 を group 宛てにしない（user の全 process）");
        assert!(matches!(stop_plan(2), StopPlan::Group(found) if found.target() == "-2"), "{:?}", stop_plan(2));
        assert!(
            matches!(stop_plan(4242), StopPlan::Group(found) if found.target() == "-4242"),
            "{:?}",
            stop_plan(4242)
        );
    }

    /// group 宛ての字面は型からしか作れず、型は pid ≤ 1 を持てない。
    #[test]
    fn pipe_stop_group_id_rejects_pid_zero_and_one() {
        assert_eq!(GroupId::new(0), None);
        assert_eq!(GroupId::new(1), None);
        assert_eq!(GroupId::new(2).map(GroupId::target), Some("-2".to_owned()));
        assert_eq!(GroupId::new(u64::MAX).map(GroupId::target), Some(format!("-{}", u64::MAX)));
    }
}
