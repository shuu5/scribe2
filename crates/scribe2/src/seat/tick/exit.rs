//! tick の退避後の終了の手と立て直し（入口の 3 値・`/exit` の第 1 手・停止の第 2 手・別口座での立て直し・
//! account-autonomy.md §5・[`super`] から純移動・`s2-07l.279`）。口座の軸の中の順は [`super::account`] が持つ。

use super::account::Seated;
use super::{
    back_off, held, InjectKind, NoopReason, Request, Seen, Stamped, TickDecision, Verdict, DETAIL_TERMINATED, EXIT,
    INJECT_WRITERS, REASON_EXIT_UNCONFIRMED, REASON_EXIT_UNSTOPPABLE, ROW_GRACE,
};
use crate::fleet::json_lite::{self, Value};
use crate::hook::{inject_path, seat_name};
use crate::seat::{cycle, inject, pane_of, state, WmScan};
use std::path::Path;
use std::time::{Duration, Instant};

/// 退避して止まった席の入口（account-autonomy.md §5）。**閉じた 3 値**（憲法 C11・bool で持たない）: 立て直しと
/// 終了の手は同じ (1)(2) を共有し、前面 process の読みで分かれる。
pub(super) enum Entry {
    /// 立て直し（前面が shell＝session は終わっている）。
    Relaunch,
    /// 終了の手（前面が shell でない ∧ 直近の合図が退避の合図 ∧ 自席の未 consumed 退避物が在る）。
    Exit,
    /// どちらも立たない（以後は既存の順序へ）。
    None,
}

/// 入口の条件（順序固定・tmux を撃つ前面の読みは最後）: (1) 自席への直近の注入の記録が tick の合図（退避の合図か
/// 終了の合図・[`last_signal`]）(2) 打刻の最終行が `Stop`（[`stopped_after`]・退避の合図の周はその ts が (1) より後）
/// (3) pane の前面 process が shell（[`super::pane_is_shell`]）なら立て直し。shell でない周は、直近が退避の合図
/// （終了の合図の後は再送しない・back-off は送れなかった周のためにある）∧ 自席の未 consumed 退避物が在る
/// （FR28「退避物が無い session には撃たない」と同じ極性）なら終了の手。(1) の無い停止（user の終了・crash）は
/// 起こし直しも終了もしない（器が起こしたのでない停止に器が手を出さない）。
pub(super) fn parked_entry(state_dir: &Path, target: &str, socket: Option<&str>, seat_dir: &Path, wm: &WmScan) -> Entry {
    let Some((kind, signalled)) = last_signal(state_dir, target) else {
        return Entry::None;
    };
    if !stopped_after(seat_dir, kind, signalled) {
        return Entry::None;
    }
    if super::pane_is_shell(socket, target) {
        return Entry::Relaunch;
    }
    match (kind, wm) {
        (InjectKind::Externalize, WmScan::Unconsumed(_)) => Entry::Exit,
        _ => Entry::None,
    }
}

/// (1) `<state_dir>/inject.jsonl` の同じ席の**注入の最新行**（`who=seat-tick` か `seat launch` の `who=seat-launch`・
/// [`INJECT_WRITERS`]）が tick の合図（`kind=externalize` か `kind=exit`）なら、その種類と ts。記録の形（判定行の
/// `kind=` の token）は同じ module の [`body`] が書く。inject.jsonl は hook の記録（`hook:pre-tool-use` 等）も共有する
/// （vessel-hook.md §6）が、それは注入ではないので飛ばす（`s2-07l.242`・退避の後に席が Bash を撃つと最新行が hook 行に
/// なり合図が見えなくなっていた）。launch の行（`kind=launch`）は注入なので**直近**に数えるが合図ではない＝launch の
/// 後の停止は起こし直さない（account-lifecycle.md §4・`s2-07l.244`）。
fn last_signal(state_dir: &Path, target: &str) -> Option<(InjectKind, u64)> {
    let seat = seat_name(target)?;
    let text = std::fs::read_to_string(inject_path(state_dir)).ok()?;
    let last = text
        .lines()
        .rev()
        .filter_map(|line| json_lite::parse_object(line).ok())
        .find(|pairs| {
            field(pairs, "seat").and_then(Value::as_str) == Some(seat.as_str())
                && field(pairs, "who").and_then(Value::as_str).is_some_and(|who| INJECT_WRITERS.contains(&who))
        })?;
    let what = field(&last, "what").and_then(Value::as_str)?;
    if !what.starts_with("decision=inject ") {
        return None;
    }
    let token = what.split_whitespace().find_map(|token| token.strip_prefix("kind="))?;
    let kind = [InjectKind::Externalize, InjectKind::Exit]
        .into_iter()
        .find(|kind| kind.as_str() == token)?;
    Some((kind, field(&last, "ts").and_then(Value::as_num)?))
}

/// 記録 1 行の key の値。
fn field<'a>(pairs: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    pairs.iter().find(|(found, _)| found == key).map(|(_, value)| value)
}

/// (2) 打刻の最終行が `Stop` か（読めない・無い周は偽＝起こさない側）。退避の合図の周は、その ts が合図（`after`）
/// より後であること（合図に応えて退避し、止まった）。終了の合図の周は ts の前後を問わない: `/exit` は打刻を
/// 積まず（`Event` は 3 値・seat-state.md §6・終了の hook は無い）、`Stop` が合図より前なのは終了の合図を送った
/// 条件そのもの（退避の合図 → その後の `Stop` → `/exit`）で、席が続きを始めた周は最終行が `UserPromptSubmit` に
/// なるので偽。
fn stopped_after(seat_dir: &Path, kind: InjectKind, after: u64) -> bool {
    let text = std::fs::read_to_string(state::path(seat_dir)).unwrap_or_default();
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| state::Stamp::from_line(line).ok())
        .is_some_and(|stamp| {
            stamp.event == state::Event::Stop && (matches!(kind, InjectKind::Exit) || stamp.ts > after)
        })
}

/// 退避後の終了の手（account-autonomy.md §5・`s2-07l.226`）: 入口（[`parked_entry`]）の後は cycle lock（FR29 と同じ
/// 除外・作り直しの最中の席へ送らない）→ back-off（exit-stamp・閾値は `seat.tick_stale_s`・`s2-07l.252` で cycle-stamp
/// と分けた）→ 第 1 手 [`send_exit`]。見送った周の理由は既存の語（`cycle-live` / `cycle-stamp-unreadable`）。
///
/// **第 2 手 = 停止**（account-autonomy.md §5「終了の手の第 2 手」・`s2-07l.259`）: back-off が `cycle-recent`（exit-stamp
/// あり＝`/exit` は送った・閾値未満）で見送る周は、入口が終了の手（前面が shell でない・(1)(2) は立っている）なので
/// 見送らず [`stop_seat`] へ。`/exit` の再送は dialog に効かず（入力欄が無い）、器は描画を読まない（C3.3）ので選択肢も
/// 押せない——退避済みの席の process を止めて終了を確定する（失うものは無い・A1 非該当・可逆〔立て直す〕）。stamp が
/// 閾値より古い周は従来どおり第 1 手から（stamp を打ち直す）。
pub(super) fn exit_turn(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen) -> Verdict {
    if cycle::lock_is_live(dir, seen.ttl_s) {
        return Verdict::of(TickDecision::Noop(NoopReason::CycleLive));
    }
    let (stamp, blocked) = back_off(dir, cycle::EXIT_STAMP_FILE, seen.stale_s);
    match blocked {
        Some(NoopReason::CycleRecent) => stop_seat(request, stamp),
        Some(reason) => held(stamp, reason),
        None => Verdict { stamp: Some(stamp), ..Verdict::of(send_exit(request, place, dir)) },
    }
}

/// 終了の手の第 2 手（順序固定・1 key も送らない）: 猶予（rules 行 [`ROW_GRACE`]・読めない周は `exit-no-rule:<variant>`）→
/// pane の shell の pid（[`cycle::pane_pid`]）→ その直下の子（[`cycle::foreground_child`]）→ [`cycle::terminate_group`]
/// （TERM → 唯一の wait → KILL → 同じ wait・pid が 2 未満なら撃たない）。`Gone` / `Killed` は `kind=exit` と同じ判定
/// （`consumed=true`・立て直しの入口 (1) がそのまま読む）に `detail=terminated` を添え、`Unstoppable` は `exit-unstoppable`
/// （error・rc 1・back-off の内側で次の周も撃つ＝TERM / KILL は冪等・stop-stamp は持たない）、pane の pid・子が取れない・
/// 撃たない周は `exit-pane-missing`（既存の語に前置き）。exit-stamp は触らない（打ち直すのは第 1 手だけ）。
fn stop_seat(request: &Request, stamp: Stamped) -> Verdict {
    let grace = match super::int_rule(ROW_GRACE) {
        Ok(found) => Duration::from_millis(found),
        Err(read) => return held_error(stamp, exit_error(read.no_rule())),
    };
    let stopped = cycle::pane_pid(request.socket, request.target)
        .and_then(cycle::foreground_child)
        .and_then(|pid| cycle::terminate_group(pid, grace));
    match stopped {
        None => held_error(stamp, exit_error(cycle::REASON_PANE_MISSING)),
        Some(cycle::Stopped::Unstoppable) => held_error(stamp, exit_error(REASON_EXIT_UNSTOPPABLE)),
        Some(cycle::Stopped::Gone | cycle::Stopped::Killed) => Verdict {
            stamp: Some(stamp),
            detail: Some(DETAIL_TERMINATED),
            ..Verdict::of(TickDecision::Inject(InjectKind::Exit, inject::Settled::Consumed))
        },
    }
}

/// 打刻を読んだ上で撃てなかった周の判定（[`held`] の error 側）。
fn held_error(stamp: Stamped, decision: TickDecision) -> Verdict {
    Verdict { stamp: Some(stamp), ..Verdict::of(decision) }
}

/// [`EXIT`] を 1 行送る（順序固定）: 入力欄の門（cycle の `/clear` と同じ [`inject::guard_input`]・断りは
/// `input-busy` / `input-unknown` で **1 key も送らない**）→ exit-stamp（write-ahead・打てない周は送らない＝次の周も
/// 送りうる形を作らない）→ 送る（[`cycle::send_exit`]）→ 前面 process が shell になるかで送達を確かめる（[`exited`]）。
///
/// 送達を目印の出現数（[`inject::deliver`]）で測らないのは、`/exit` を受けた席は終わって pane が shell に置き換わり
/// 目印が増えない＝成功が `absent` に倒れるためである（実地 2026-09-14・`s2-07l.252`）。Enter の修復
/// （[`inject::nudge_enter`]）も通さない: 終わった席の pane に残る古い `❯ /exit` 行を入力欄と読んで **shell へ Enter を
/// 送る**形になる。窓の内に shell にならない周は `exit-unconfirmed` の error（rc 1）で、exit-stamp は打たれたまま
/// （再送しない・back-off の内側の次の周は第 2 手 [`stop_seat`] が process を止める・`s2-07l.259`）。
fn send_exit(request: &Request, place: &super::StateDir, dir: &Path) -> TickDecision {
    let Some(pane) = pane_of(request.socket, request.target, request.capture_file) else {
        return exit_error(cycle::REASON_PANE_MISSING);
    };
    match inject::guard_input(&pane) {
        Ok(()) => {}
        Err(inject::InputGate::Busy) => return exit_error(cycle::REASON_INPUT_BUSY),
        Err(inject::InputGate::UnknownInput) => return exit_error(cycle::REASON_INPUT_UNKNOWN),
    }
    if cycle::write_exit_stamp(dir).is_err() {
        return exit_error(cycle::REASON_STAMP);
    }
    if !cycle::send_exit(request.socket, request.target, place, EXIT) {
        return exit_error(inject::REASON_TMUX_FAILED);
    }
    if exited(request) {
        TickDecision::Inject(InjectKind::Exit, inject::Settled::Consumed)
    } else {
        exit_error(REASON_EXIT_UNCONFIRMED)
    }
}

/// `/exit` の送達の確認: 窓（[`Request::settle`]）の内に刻み（[`Request::step`]）ごとに、target の前面 process が
/// shell になったか（入口 (3) と同じ [`super::pane_is_shell`]・typed な metadata・字面を読まない）。作り直しの確認
/// （cycle の `started`）と同じ窓・同じ刻み。
fn exited(request: &Request) -> bool {
    let deadline = Instant::now().checked_add(request.settle);
    while deadline.is_some_and(|at| Instant::now() < at) {
        std::thread::sleep(request.step);
        if super::pane_is_shell(request.socket, request.target) {
            return true;
        }
    }
    false
}

/// 終了の手を送れなかった周の判定（`exit-` を前置きして注入の断りや noop の語彙と分ける・[`inject_line`] と同型）。
fn exit_error(reason: &str) -> TickDecision {
    TickDecision::Error(format!("exit-{reason}"))
}

/// 退避して止まった席の立て直し（account-autonomy.md §5）: back-off（`s2-07l.110` の cycle-stamp だけ・閾値は
/// `seat.tick_stale_s`・exit-stamp は読まない＝`/exit` の次の周に評価される・`s2-07l.252`）→ cycle lock（FR29 と同じ
/// 除外）→ [`cycle::relaunch`]（session 用の選定・雛形の穴埋め・
/// 起動と復元の注入・登録 row の口座の更新）。候補なしは `account-no-candidate` で注入せず次の tick で選び直し、
/// 立て直しが送れない・確かめられない周は `relaunch-<理由>` の error（rc 1・次の周は back-off が見る）。
pub(super) fn relaunch_turn(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen, seated: &Seated) -> Verdict {
    let (stamp, blocked) = back_off(dir, cycle::STAMP_FILE, seen.stale_s);
    if let Some(reason) = blocked {
        return held(stamp, reason);
    }
    if cycle::lock_is_live(dir, seen.ttl_s) {
        return held(stamp, NoopReason::CycleLive);
    }
    let result = cycle::relaunch(&cycle::Relaunch {
        target: request.target,
        socket: request.socket,
        state_dir: place,
        restore: request.restore,
        settle: request.settle,
        step: request.step,
        row: &seated.row,
        state: &seated.state,
        labels: request.accounts,
        threshold_pct: seen.threshold,
    });
    let (decision, relaunched) = match result {
        cycle::Relaunched::Done(label, settled) => (TickDecision::Inject(InjectKind::Relaunch, settled), label),
        cycle::Relaunched::None(found) => {
            (TickDecision::Noop(NoopReason::AccountNoCandidate), format!("none:{}", found.reason.as_str()))
        }
        cycle::Relaunched::Refused(reason) | cycle::Relaunched::Failed(reason) => {
            return Verdict { stamp: Some(stamp), ..Verdict::of(TickDecision::Error(format!("relaunch-{reason}"))) };
        }
    };
    Verdict { stamp: Some(stamp), relaunched: Some(relaunched), ..Verdict::of(decision) }
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.279
    use super::super::WHO;
    use super::{inject_path, last_signal};

    /// `seat launch` の行（`who=seat-launch`・`kind=launch`）は同じ席の**直近の注入**に数え、合図ではない: 退避の合図の
    /// 後ろに launch が在れば入口 (1) は立たない（launch の後の停止を起こし直さない・account-lifecycle.md §4）。
    #[test]
    fn seat_launch_row_overrides_the_signal_and_is_not_one() {
        let lines = [
            inject_line(WHO, "decision=inject account=a1:100 kind=externalize", "seat1", 100),
            inject_line(crate::seat::cycle::WHO_LAUNCH, "decision=inject target=seat1 kind=launch account=a2", "seat1", 200),
        ];
        assert_eq!(signal_of("launch-after", Some(&lines)), None, "launch が直近なら合図なし");
        let only_launch = [inject_line(crate::seat::cycle::WHO_LAUNCH, "decision=inject target=seat1 kind=launch account=a2", "seat1", 100)];
        assert_eq!(signal_of("launch-only", Some(&only_launch)), None, "launch だけ");
        let before = [
            inject_line(crate::seat::cycle::WHO_LAUNCH, "decision=inject target=seat1 kind=launch account=a2", "seat1", 100),
            inject_line(WHO, "decision=inject account=a2:100 kind=externalize", "seat1", 200),
        ];
        assert_eq!(signal_of("launch-before", Some(&before)), Some(("externalize", 200)), "launch の後の合図は生きる");
    }

    /// 記録 1 行（`InjectionRecord::to_line` と同じ key の flat JSON）。
    fn inject_line(who: &str, what: &str, seat: &str, ts: u64) -> String {
        format!(
            r#"{{"schema":1,"who":"{who}","what":"{what}","when":"tick","bytes":0,"tokens":null,"wall_ms":0,"seat":"{seat}","ts":{ts}}}"#
        )
    }

    /// `inject.jsonl` を `lines` で置いた state dir で [`last_signal`] を読む（`None` の lines は file を置かない）。
    fn signal_of(name: &str, lines: Option<&[String]>) -> Option<(&'static str, u64)> {
        let dir = std::env::temp_dir().join(format!("seat-tick-signal-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        if let Some(lines) = lines {
            std::fs::write(inject_path(&dir), format!("{}\n", lines.join("\n"))).ok();
        }
        let found = last_signal(&dir, "seat1").map(|(kind, ts)| (kind.as_str(), ts));
        std::fs::remove_dir_all(&dir).ok();
        found
    }

    /// (a) 退避の合図の後ろに同じ席の hook の記録（role-allow・session-start）が在っても、合図は hook 行に隠れない
    /// （hook の記録は注入ではない・vessel-hook.md §6）。別の席の tick の行も数えない。
    #[test]
    fn seat_exit_signal_skips_hook_rows_after_externalize() {
        let lines = [
            inject_line(WHO, "decision=inject account=a1:100 kind=externalize", "seat1", 100),
            inject_line("hook:pre-tool-use", "decision=role-allow capability=launch", "seat1", 200),
            inject_line("hook:session-start", "served version=2", "seat1", 300),
            inject_line(WHO, "decision=inject kind=pointer", "seat2", 400),
        ];
        assert_eq!(signal_of("hook-rows", Some(&lines)), Some(("externalize", 100)));
    }

    /// (b) 退避の合図の後ろに同じ席の tick の別の注入（heartbeat の `kind=pointer`）が在れば、合図は上書きされている。
    #[test]
    fn seat_exit_signal_is_overwritten_by_a_later_tick_pointer() {
        let lines = [
            inject_line(WHO, "decision=inject account=a1:100 kind=externalize", "seat1", 100),
            inject_line(WHO, "decision=inject kind=pointer", "seat1", 200),
            inject_line("hook:pre-tool-use", "decision=role-allow capability=launch", "seat1", 300),
        ];
        assert_eq!(signal_of("pointer", Some(&lines)), None);
    }

    /// (c) 同じ席の tick の行が無い・file が無い周は合図なし。
    #[test]
    fn seat_exit_signal_is_none_without_tick_rows() {
        let hook_only = [inject_line("hook:pre-tool-use", "decision=role-allow capability=launch", "seat1", 100)];
        assert_eq!(signal_of("hook-only", Some(&hook_only)), None, "hook の行だけ");
        assert_eq!(signal_of("empty", Some(&[])), None, "空");
        assert_eq!(signal_of("missing", None), None, "file が無い");
    }
}
