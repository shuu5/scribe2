//! tick の退避後の終了の手と立て直し（入口の 4 値・`/exit` の第 1 手・停止の第 2 手・別口座での立て直し・復元の第 2 手・
//! account-autonomy.md §5・[`super`] から純移動・`s2-07l.279`）。口座の軸の中の順は [`super::account`] が持つ。

use super::account::Seated;
use super::render::DETAIL_AFTER_TERMINATED;
use super::{
    back_off, held, InjectKind, NoopReason, Request, Seen, SignalOrigin, Stamped, TickDecision, Verdict,
    DETAIL_TERMINATED, EXIT, INJECT_WRITERS, REASON_EXIT_UNCONFIRMED, REASON_EXIT_UNSTOPPABLE, ROW_GRACE,
    SIGNAL_ORIGINS,
};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::{replay, store};
use crate::hook::{inject_path, seat_name};
use crate::seat::{cycle, inject, pane_of, role, state, WmScan};
use std::path::Path;
use std::time::{Duration, Instant};

/// 復元の第 2 手を**送ろうとした**周の打刻の名前（exit-stamp と同じ形・閾値は `seat.signal_backoff_s`・`s2-07l.318`）。
pub(super) const RESTORE_STAMP_FILE: &str = "restore-stamp";
/// restore-stamp を書けない＝復元を送る前に断る（`restore-stamp-unwritable`・write-ahead・N1）。
const REASON_RESTORE_STAMP: &str = "stamp-unwritable";

/// 退避して止まった席の入口（account-autonomy.md §5）。**閉じた 4 値**（憲法 C11・bool で持たない）: 立て直しと
/// 終了の手は同じ (1)(2) を共有し、前面 process の読みで分かれる。復元の第 2 手はどちらも立たない周だけ。
pub(super) enum Entry {
    /// 立て直し（前面が shell＝session は終わっている）。
    Relaunch,
    /// 終了の手（前面が shell でない ∧ 直近の合図が口座由来か hook 由来の退避の合図 ∧ 自席の未 consumed 退避物が在る）。
    Exit,
    /// 復元の第 2 手（起動したが 1 turn も始めていない・[`booted_without_a_turn`]・`s2-07l.318`）。
    Restore,
    /// どれも立たない（以後は既存の順序へ）。
    None,
}

/// 入口（順序固定・`s2-07l.318`）: 立て直し・終了の手（[`signalled_entry`]）の**後**、どれにも当たらない周だけ復元の
/// 第 2 手の (1)(2)（[`booted_without_a_turn`]）。(3) 状態の門と (4) restore-stamp の back-off は [`restore_turn`]。
pub(super) fn parked_entry(state_dir: &Path, target: &str, socket: Option<&str>, seat_dir: &Path, wm: &WmScan) -> Entry {
    match signalled_entry(state_dir, target, socket, seat_dir, wm) {
        Entry::None if booted_without_a_turn(seat_dir) => Entry::Restore,
        found => found,
    }
}

/// 入口の条件（順序固定・tmux を撃つ前面の読みは最後）: (1) 自席への直近の注入の記録が tick の合図（退避の合図か
/// 終了の合図・[`last_signal`]）(2) 打刻の最終行が `Stop`（[`stopped_after`]・退避の合図の周はその ts が (1) より後）
/// (3) pane の前面 process が shell（[`super::pane_is_shell`]）なら立て直し（**出所を問わない**＝user が手で終えた席も
/// 起こす）。shell でない周は、直近が退避の合図（終了の合図の後は再送しない・back-off は送れなかった周のためにある）
/// ∧ その**出所が口座か hook**（`origin=account|hook`・席を別口座で作り直す合図・`s2-07l.307`）∧ 自席の未 consumed
/// 退避物が在る（FR28「退避物が無い session には撃たない」と同じ極性）なら終了の手。context 由来（`origin=context`）と
/// 出所不明（`origin=` の無い旧 binary の記録・読めない値）は終了の手を立てず、以後は既存の順序（状態の門 → 退避物 →
/// `/clear` の cycle・同じ口座）へ——不可逆の `/exit` を出所不明の合図に送らない（N1）。(1) の無い停止（user の終了・
/// crash）は起こし直しも終了もしない（器が起こしたのでない停止に器が手を出さない）。
fn signalled_entry(state_dir: &Path, target: &str, socket: Option<&str>, seat_dir: &Path, wm: &WmScan) -> Entry {
    let Some((kind, origin, signalled)) = last_signal(state_dir, target) else {
        return Entry::None;
    };
    if !stopped_after(seat_dir, kind, signalled) {
        return Entry::None;
    }
    if super::pane_is_shell(socket, target) {
        return Entry::Relaunch;
    }
    match (kind, origin, wm) {
        (InjectKind::Externalize, Some(SignalOrigin::Account | SignalOrigin::Hook), WmScan::Unconsumed(_)) => Entry::Exit,
        _ => Entry::None,
    }
}

/// (1) `<state_dir>/inject.jsonl` の同じ席の**注入の最新行**（`who=seat-tick` か `seat launch` の `who=seat-launch`・
/// [`INJECT_WRITERS`]）が tick の合図（`kind=externalize` か `kind=exit`）なら、その種類・出所・ts。記録の形（判定行の
/// `kind=` / `origin=` の token）は同じ module の [`body`] が書く。出所は退避の合図の周だけ載り（`s2-07l.307`）、無い行
/// （旧 binary の記録・終了の合図）と [`SIGNAL_ORIGINS`] に無い値は `None`（出所不明＝呼び側は終了の手を立てない）。
/// inject.jsonl は hook の記録（`hook:pre-tool-use` 等）も共有する（vessel-hook.md §6）が、それは注入ではないので飛ばす
/// （`s2-07l.242`・退避の後に席が Bash を撃つと最新行が hook 行になり合図が見えなくなっていた）。launch の行
/// （`kind=launch`）は注入なので**直近**に数えるが合図ではない＝launch の後の停止は起こし直さない
/// （account-lifecycle.md §4・`s2-07l.244`）。
fn last_signal(state_dir: &Path, target: &str) -> Option<(InjectKind, Option<SignalOrigin>, u64)> {
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
    let origin = what
        .split_whitespace()
        .find_map(|token| token.strip_prefix("origin="))
        .and_then(|found| SIGNAL_ORIGINS.iter().copied().find(|origin| origin.as_str() == found));
    Some((kind, origin, field(&last, "ts").and_then(Value::as_num)?))
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
    last_stamp(seat_dir)
        .is_some_and(|stamp| stamp.event == state::Event::Stop && (matches!(kind, InjectKind::Exit) || stamp.ts > after))
}

/// 打刻の最終行（読めない・無い・壊れている周は `None`）。入口 (2) と復元の第 2 手の (2) の同じ読み。
fn last_stamp(seat_dir: &Path) -> Option<state::Stamp> {
    let text = std::fs::read_to_string(state::path(seat_dir)).ok()?;
    text.lines().rev().find(|line| !line.trim().is_empty()).and_then(|line| state::Stamp::from_line(line).ok())
}

/// 復元の第 2 手の入口 (1)(2)（account-autonomy.md §5「復元の第 2 手」・`s2-07l.318`）: (1) cycle-stamp（[`cycle::STAMP_FILE`]・
/// 中身は unix 秒・立て直しか作り直しが起きた証拠）が読め、(2) 打刻の最終行が `SessionStart` でその ts が stamp 以上（起動
/// したが 1 turn も始めていない＝復元が消費されていない typed な証拠・C3.3）。それ以外と読めない・無い周は偽（起こさない側）。
fn booted_without_a_turn(seat_dir: &Path) -> bool {
    let stamped = std::fs::read_to_string(cycle::stamp_path(seat_dir)).ok();
    let Some(since) = stamped.and_then(|text| text.trim().parse::<u64>().ok()) else { return false };
    last_stamp(seat_dir).is_some_and(|stamp| stamp.event == state::Event::SessionStart && stamp.ts >= since)
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
///
/// **同じ周で畳む**（`s2-07l.314`・C9「止まった席を人手なしで作り直す」の空白を最小に）: 第 1 手を送って窓の内に前面が
/// shell にならない周は error で終えず、続けて第 2 手 → 前面が shell に戻れば立て直し（[`stop_and_relaunch`]）。実地
/// 2026-09-15（admin・tick.jsonl）では `/exit` → `exit-unconfirmed` → 次の周の停止 → さらに次の周の立て直し、と 3 手が
/// 3 周（約 10 分）に割れていた。
pub(super) fn exit_turn(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen) -> Verdict {
    if cycle::lock_is_live(dir, seen.ttl_s) {
        return Verdict::of(TickDecision::Noop(NoopReason::CycleLive));
    }
    let (stamp, blocked) = back_off(dir, cycle::EXIT_STAMP_FILE, seen.stale_s);
    match blocked {
        Some(NoopReason::CycleRecent) => stop_seat(request, stamp),
        Some(reason) => held(stamp, reason),
        None => match send_exit(request, place, dir) {
            Sent::Confirmed => stamped(stamp, exit_settled()),
            Sent::Unconfirmed => stop_and_relaunch(request, place, dir, seen, stamp),
            Sent::Refused(reason) => stamped(stamp, exit_error(reason)),
        },
    }
}

/// 終了の手の第 2 手だけの周（back-off の内側＝`/exit` は前の周に送った・1 key も送らない）: 止まれば `kind=exit` と同じ
/// 判定（`consumed=true`・立て直しの入口 (1) がそのまま読む）に `detail=terminated` を添え（[`terminated`]）、撃てない・
/// 止まらない周は [`terminate_front`] の error。exit-stamp は触らない（打ち直すのは第 1 手だけ）。次の周は入口 (3)
/// 「前面が shell」で立て直しへ進む。
fn stop_seat(request: &Request, stamp: Stamped) -> Verdict {
    match terminate_front(request) {
        Ok(()) => terminated(stamp),
        Err(decision) => stamped(stamp, decision),
    }
}

/// 第 1 手が窓の内に通らなかった周の続き（`s2-07l.314`・順序固定・1 関数の列）: 第 2 手（[`terminate_front`]）→ 止まれば
/// 前面が shell に戻るのを第 1 手と同じ窓・同じ刻みで待つ（[`exited`]・子の回収の後の shell の prompt の描き直しは唯一の
/// wait の外で起きる）→ 口座の軸の材料を読み直し（[`reseat`]）→ 立て直し（[`relaunch_turn`]・cycle-stamp の back-off と
/// lock は従来どおり）に `detail=after-terminated` を添える（判定行は `kind=relaunch detail=after-terminated`・C10）。
///
/// 止まらない・pane の子が取れない周は既存の error（`exit-unstoppable` / `exit-pane-missing`・次の周は back-off の内側で
/// [`stop_seat`] が撃ち直す）。止めたのに窓の内に前面が shell にならない周（撃った子が前面の子でなかった等）は
/// `exit-unconfirmed`（`/exit` は送ったが shell を見ていない＝終了を確定したと書かない）。登録 row を読み直せない周は
/// 停止だけの判定（[`terminated`]・次の周が入口 (3) で立て直す）。exit-stamp は第 1 手で打ったまま（再送しない・N1）。
fn stop_and_relaunch(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen, stamp: Stamped) -> Verdict {
    if let Err(decision) = terminate_front(request) {
        return stamped(stamp, decision);
    }
    if !exited(request) {
        return stamped(stamp, exit_error(REASON_EXIT_UNCONFIRMED));
    }
    let Some(seated) = reseat(&place.path, request.target) else {
        return terminated(stamp);
    };
    Verdict { detail: Some(DETAIL_AFTER_TERMINATED), ..relaunch_turn(request, place, dir, seen, &seated) }
}

/// 第 2 手の本体（順序固定・1 key も送らない・`s2-07l.259`）: 猶予（rules 行 [`ROW_GRACE`]・読めない周は
/// `exit-no-rule:<variant>`）→ pane の shell の pid（[`cycle::pane_pid`]）→ その直下の子（[`cycle::foreground_child`]）→
/// [`cycle::terminate_group`]（TERM → 唯一の wait → KILL → 同じ wait・pid が 2 未満なら撃たない）。`Gone` / `Killed` は
/// `Ok`、`Unstoppable` は `exit-unstoppable`（error・rc 1・back-off の内側で次の周も撃つ＝TERM / KILL は冪等・stop-stamp
/// は持たない）、pane の pid・子が取れない・撃たない周は `exit-pane-missing`（既存の語に前置き）。
fn terminate_front(request: &Request) -> Result<(), TickDecision> {
    let grace = match super::int_rule(ROW_GRACE) {
        Ok(found) => Duration::from_millis(found),
        Err(read) => return Err(exit_error(read.no_rule())),
    };
    let stopped = cycle::pane_pid(request.socket, request.target)
        .and_then(cycle::foreground_child)
        .and_then(|pid| cycle::terminate_group(pid, grace));
    match stopped {
        None => Err(exit_error(cycle::REASON_PANE_MISSING)),
        Some(cycle::Stopped::Unstoppable) => Err(exit_error(REASON_EXIT_UNSTOPPABLE)),
        Some(cycle::Stopped::Gone | cycle::Stopped::Killed) => Ok(()),
    }
}

/// 同じ周で立て直しへ続く周の口座の軸の材料（[`Seated`]・登録 row と replay の現在地）。入口で読んだものを持ち回らず
/// 置き場を読み直す（[`exit_turn`] の引数を増やさない・C4）: 実測行の鮮度は同じ周の入口の計測が保っている。log を
/// 読めない・自席の登録 row が無い周は `None`（停止だけで周を終える側）。
fn reseat(state_dir: &Path, target: &str) -> Option<Seated> {
    let state = replay(&store::read_all(state_dir).ok()?);
    let row = role::registration_of_target(&state, target)?.clone();
    Some(Seated { row, state })
}

/// 終了を確定した判定（`kind=exit consumed=true`・第 1 手が通った周と第 2 手で止めた周の同じ形・立て直しの入口 (1) が読む）。
fn exit_settled() -> TickDecision {
    TickDecision::Inject(InjectKind::Exit, inject::Settled::Consumed)
}

/// 第 2 手で終了を確定した周の判定（[`exit_settled`] に `detail=terminated`）。
fn terminated(stamp: Stamped) -> Verdict {
    Verdict { stamp: Some(stamp), detail: Some(DETAIL_TERMINATED), ..Verdict::of(exit_settled()) }
}

/// 打刻を読んだ上での判定（[`held`] と同型・判定は呼び側が決める）。
fn stamped(stamp: Stamped, decision: TickDecision) -> Verdict {
    Verdict { stamp: Some(stamp), ..Verdict::of(decision) }
}

/// 第 1 手の結果（**閉じた 3 値**・憲法 C11・bool で持たない）。
enum Sent {
    /// 送って、窓の内に前面が shell になった（`/exit` が通った）。
    Confirmed,
    /// 送ったが、窓の内に前面が shell にならない（背景の仕事を持つ席の終了確認 dialog・実地 2026-09-14）。
    Unconfirmed,
    /// 送っていない（門の断り・stamp・tmux・pane が取れない・理由の語）。
    Refused(&'static str),
}

/// [`EXIT`] を 1 行送る（順序固定）: 入力欄の門（cycle の `/clear` と同じ [`inject::pass_input`]・断りは
/// `input-busy` / `input-own-queued` / `input-unknown` で **1 key も送らない**——自席の文が残る周だけは
/// Enter を 1 回送って着地させ、残れば `input-own-queued` で断る・`s2-07l.288`）→ exit-stamp（write-ahead・打てない周は送らない＝次の周も
/// 送りうる形を作らない）→ 送る（[`cycle::send_exit`]）→ 前面 process が shell になるかで送達を確かめる（[`exited`]）。
///
/// 送達を目印の出現数（[`inject::deliver`]）で測らないのは、`/exit` を受けた席は終わって pane が shell に置き換わり
/// 目印が増えない＝成功が `absent` に倒れるためである（実地 2026-09-14・`s2-07l.252`）。Enter の修復
/// （[`inject::nudge_enter`]）も通さない: 終わった席の pane に残る古い `❯ /exit` 行を入力欄と読んで **shell へ Enter を
/// 送る**形になる。窓の内に shell にならない周は [`Sent::Unconfirmed`] で、exit-stamp は打たれたまま（再送しない・
/// 同じ周の続きは [`stop_and_relaunch`]・`s2-07l.314`）。
fn send_exit(request: &Request, place: &super::StateDir, dir: &Path) -> Sent {
    let Some(pane) = pane_of(request.socket, request.target, request.capture_file) else {
        return Sent::Refused(cycle::REASON_PANE_MISSING);
    };
    let own = inject::last_own_payload(&place.path, request.target);
    let recapture = || pane_of(request.socket, request.target, request.capture_file);
    match inject::pass_input(request.socket, request.target, &pane, own.as_deref(), recapture) {
        Ok(()) => {}
        Err(inject::Blocked::Foreign) => return Sent::Refused(cycle::REASON_INPUT_BUSY),
        Err(inject::Blocked::UnknownInput) => return Sent::Refused(cycle::REASON_INPUT_UNKNOWN),
        Err(inject::Blocked::OwnQueued) => return Sent::Refused(cycle::REASON_INPUT_OWN_QUEUED),
    }
    if cycle::write_exit_stamp(dir).is_err() {
        return Sent::Refused(cycle::REASON_STAMP);
    }
    if !cycle::send_exit(request.socket, request.target, place, EXIT) {
        return Sent::Refused(inject::REASON_TMUX_FAILED);
    }
    if exited(request) {
        Sent::Confirmed
    } else {
        Sent::Unconfirmed
    }
}

/// `/exit` の送達の確認: 窓（[`Request::settle`]）の内に刻み（[`Request::step`]）ごとに、target の前面 process が
/// shell になったか（入口 (3) と同じ [`super::pane_is_shell`]・typed な metadata・字面を読まない）。作り直しの確認
/// （cycle の `started`）と同じ窓・同じ刻み。第 2 手で止めた後の前面の読みも同じ 1 本（[`stop_and_relaunch`]）。
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

/// 復元の第 2 手（account-autonomy.md §5「復元の第 2 手」・`s2-07l.318`・[`exit_turn`] と同じ形）: 立て直しと `seat launch`
/// は復元の消費を settle の窓の内で確かめ、確かめられなければ `restore-unconfirmed` で終端する（SessionStart hook の所要が
/// 窓を食う周ほど刺さらない・実地 2026-09-15 03:15Z: planner が復元されないまま 16 分放置）。入口の (1)(2) の後は (3) 状態の
/// 門（[`super::gate_of`]）→ cycle lock → (4) restore-stamp の back-off（`seat.signal_backoff_s` 未満なら `restore-recent`）→
/// [`send_restore`]。消費されなかった周は記録だけで**終端しない**（次の周が back-off の後にもう一度）。
pub(super) fn restore_turn(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen) -> Verdict {
    let live = || cycle::lock_is_live(dir, seen.ttl_s).then_some(NoopReason::CycleLive);
    if let Some(reason) = super::gate_of(seen.state).or_else(live) {
        return Verdict::of(TickDecision::Noop(reason));
    }
    let (stamp, blocked) = back_off(dir, RESTORE_STAMP_FILE, seen.backoff_s);
    match blocked {
        Some(NoopReason::CycleRecent) => held(stamp, NoopReason::RestoreRecent),
        Some(reason) => held(stamp, reason),
        None => stamped(stamp, send_restore(request, place, dir)),
    }
}

/// 復元の command（`--restore`・既定 [`cycle::DEFAULT_RESTORE`]）を 1 行送る（順序固定・cycle の復元と同じ経路）: restore-stamp
/// （write-ahead・打てない周は 1 key も送らない）→ [`inject::deliver_within`]（入力欄の門は中・断りは送っていない・窓は
/// cycle の復元と同じ [`Request::settle`]）。断りは `restore-` を前置きして注入の断りや noop の語彙と分ける（[`exit_error`] と同型）。
fn send_restore(request: &Request, place: &super::StateDir, dir: &Path) -> TickDecision {
    if std::fs::write(dir.join(RESTORE_STAMP_FILE), format!("{}\n", state::now_secs())).is_err() {
        return TickDecision::Error(format!("restore-{REASON_RESTORE_STAMP}"));
    }
    let payload = request.restore.unwrap_or(cycle::DEFAULT_RESTORE);
    let sending = inject::Request { target: request.target, socket: request.socket, payload, state_dir: Some(place) };
    match inject::deliver_within(&sending, request.settle) {
        inject::Delivery::Delivered(_, settled) => TickDecision::Inject(InjectKind::Restore, settled),
        inject::Delivery::Refused(reason) | inject::Delivery::Unconfirmed(reason) => {
            TickDecision::Error(format!("restore-{reason}"))
        }
    }
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
        assert_eq!(signal_of("launch-before", Some(&before)), Some(("externalize", None, 200)), "launch の後の合図は生きる");
    }

    /// (g) 退避の合図の行の `origin=` の token は typed に返る（`context` / `account` / `hook`・[`SIGNAL_ORIGINS`] の
    /// 全数）。token の無い行（旧 binary の記録）と列に無い値は `None`（出所不明・終了の手は立たない側）。終了の合図
    /// （`kind=exit`）は出所を持たない。base は型が無く compile で落ちる（RED・`s2-07l.307`）。
    #[test]
    fn seat_exit_signal_carries_the_origin_token() {
        for origin in super::SIGNAL_ORIGINS {
            let name = origin.as_str();
            let lines = [inject_line(WHO, &format!("decision=inject account=a1:100 kind=externalize origin={name}"), "seat1", 100)];
            assert_eq!(signal_of(&format!("origin-{name}"), Some(&lines)), Some(("externalize", Some(name), 100)), "{name}");
        }
        let bare = [inject_line(WHO, "decision=inject account=a1:100 kind=externalize", "seat1", 100)];
        assert_eq!(signal_of("origin-none", Some(&bare)), Some(("externalize", None, 100)), "token の無い行は出所不明");
        let unknown = [inject_line(WHO, "decision=inject kind=externalize origin=mars", "seat1", 100)];
        assert_eq!(signal_of("origin-unknown", Some(&unknown)), Some(("externalize", None, 100)), "列に無い値は出所不明");
        let exit = [inject_line(WHO, "decision=inject consumed=true kind=exit", "seat1", 300)];
        assert_eq!(signal_of("origin-exit", Some(&exit)), Some(("exit", None, 300)), "終了の合図は出所を持たない");
    }

    /// 記録 1 行（`InjectionRecord::to_line` と同じ key の flat JSON）。
    fn inject_line(who: &str, what: &str, seat: &str, ts: u64) -> String {
        format!(
            r#"{{"schema":1,"who":"{who}","what":"{what}","when":"tick","bytes":0,"tokens":null,"wall_ms":0,"seat":"{seat}","ts":{ts}}}"#
        )
    }

    /// `inject.jsonl` を `lines` で置いた state dir で [`last_signal`] を読む（`None` の lines は file を置かない）。
    /// 返りは `(kind, origin, ts)` の字面。
    fn signal_of(name: &str, lines: Option<&[String]>) -> Option<(&'static str, Option<&'static str>, u64)> {
        let dir = std::env::temp_dir().join(format!("seat-tick-signal-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        if let Some(lines) = lines {
            std::fs::write(inject_path(&dir), format!("{}\n", lines.join("\n"))).ok();
        }
        let found =
            last_signal(&dir, "seat1").map(|(kind, origin, ts)| (kind.as_str(), origin.map(super::SignalOrigin::as_str), ts));
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
        assert_eq!(signal_of("hook-rows", Some(&lines)), Some(("externalize", None, 100)));
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

    /// 登録 row 1 件（役割 planner・口座 `account`・target `target`）。
    fn row(target: &str, account: &str) -> crate::fleet::Registration {
        crate::fleet::Registration {
            role: crate::seat::role::Role::Planner,
            anchor: format!("/repo/{target}"),
            target: target.to_owned(),
            sid: None,
            account: account.to_owned(),
            launch: "cld {account_dir}".to_owned(),
            model: None,
        }
    }

    /// 同じ周で立て直しへ続く周の材料の読み直し（[`reseat`]・`s2-07l.314`）: 自席の登録 row が在る置き場はその row
    /// （同じ target の後の行が勝つ）と replay の現在地を返し、別の target の row だけ・row の無い置き場・置き場そのものが
    /// 無い周は `None`（停止だけで周を終える側）。base は関数が無く compile で落ちる（RED）。
    #[test]
    fn seat_exit_reseat_reads_the_own_registration_row_or_none() {
        let dir = std::env::temp_dir().join(format!("seat-tick-reseat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        assert!(super::reseat(&dir, "seat1").is_none(), "row の無い置き場");
        assert!(crate::seat::role::register(&dir, row("seat2", "a2")).is_ok(), "別の席の row を積める");
        assert!(super::reseat(&dir, "seat1").is_none(), "別の target の row だけ");
        assert!(crate::seat::role::register(&dir, row("seat1", "a1")).is_ok(), "自席の row を積める");
        assert!(crate::seat::role::register(&dir, row("seat1", "a3")).is_ok(), "自席の row の更新（同じ鍵）");
        let found = super::reseat(&dir, "seat1").map(|seated| (seated.row.account, seated.state.registrations.len()));
        assert_eq!(found, Some(("a3".to_owned(), 2)), "自席の最新の row と replay の現在地（鍵 2 つ）");
        std::fs::remove_dir_all(&dir).ok();
        assert!(super::reseat(&dir, "seat1").is_none(), "置き場が無い");
    }

    /// 復元の第 2 手の入口 (1)(2)（[`booted_without_a_turn`]・`s2-07l.318`）: stamp 以上の `SessionStart` だけ真（境界は
    /// 以上＝同じ秒は真）。stamp 無し・打刻無し・`Stop` / `UserPromptSubmit`・stamp より前は偽。base は関数が無い（RED）。
    #[test]
    fn seat_restore_entry_needs_a_cycle_stamp_and_a_session_start_after_it() {
        use crate::seat::state::{path, Event, Stamp};
        let dir = std::env::temp_dir().join(format!("seat-tick-restore-entry-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        assert!(!super::booted_without_a_turn(&dir), "打刻が無い");
        let booted = |event: Event, ts: u64| {
            std::fs::write(path(&dir), format!("{}\n", Stamp { ts, ..Stamp::now(event, "sid") }.to_line())).ok();
            super::booted_without_a_turn(&dir)
        };
        assert!(!booted(Event::SessionStart, 200), "cycle-stamp が無い");
        std::fs::write(crate::seat::cycle::stamp_path(&dir), "200\n").ok();
        assert!(booted(Event::SessionStart, 200), "stamp と同じ秒の SessionStart（以上）");
        assert!(!booted(Event::SessionStart, 199), "stamp より前の SessionStart（起動に届かなかった周）");
        assert!(!booted(Event::UserPromptSubmit, 300) && !booted(Event::Stop, 300), "turn を始めた席");
        std::fs::remove_dir_all(&dir).ok();
    }
}
