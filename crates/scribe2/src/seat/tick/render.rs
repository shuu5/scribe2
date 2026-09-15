//! tick の注入の本文と記録（1 行の注入と自打刻・判定行の字面・席の記録への 1 行・[`super`] から純移動・
//! `s2-07l.279`）。token の名前と順序は判定の産物（[`super::Judged`]）から作る。

use super::{stamp_path, InjectKind, Judged, Request, SignalOrigin, Stamped, TickDecision, REASON_STAMP, WHEN, WHO};
use crate::fleet::store::{self, LockPolicy};
use crate::hook::{seat_name, InjectionRecord, SCHEMA};
use crate::seat::{heartbeat, inject, meter, sanitize_target, state};
use std::path::Path;
use std::time::Instant;

/// 第 1 手が通らず同じ周で第 2 手（停止）→ 立て直しまで進んだ周に `kind=relaunch` の判定行へ足す `detail` の語
/// （`s2-07l.314`・停止だけで終えた周の [`super::DETAIL_TERMINATED`] と同じ欄に語を 1 つ足す・`InjectKind` は増やさない）。
pub(super) const DETAIL_AFTER_TERMINATED: &str = "after-terminated";

/// 注入する 1 行と、それを送る周の席の状態（[`inject_line`] の入力）。
///
/// 畳むのは憲法 C4 の引数上限（R-C4-4.args = 5）ゆえ: 修復の門（[`inject::repair_of`]）が席の状態を
/// 読むので、素の引数で足すと 6 になる。
pub(super) struct Signal<'a> {
    /// 何を注入するか。
    pub(super) kind: InjectKind,
    /// 退避の合図の出所（`kind` が [`InjectKind::Externalize`] の周だけ `Some`・打刻の合図は `None`・`s2-07l.307`）。
    /// 判定行の `origin=<…>` を経て終了の手の入口（[`super::exit::parked_entry`]）が読む。
    pub(super) origin: Option<SignalOrigin>,
    /// 注入する 1 行。
    pub(super) payload: &'a str,
    /// 席の状態の読み（typed・pane の字面ではない）。
    pub(super) state: state::Read,
}

/// 1 行を注入し、成立したら自打刻する（退避の合図も打刻の合図も同じ経路。自打刻は打刻の合図の
/// brake〔[`pointer_recent`]〕にだけ効き、退避の合図と cycle は次の周も評価する）。busy な席へは
/// queue の形で届く（`.90`）。
///
/// 送達の後に Enter だけが落ちた周を**同じ周の中で**修復する（[`inject::nudge_enter`]・`s2-07l.150`）。
/// 修復が閉じない周（`EnterLost`）は自打刻しない＝次の周の判定がそれを測れる（入力欄に目印が残る
/// 席は入力欄の門が断り、`pointer-recent` の黙った noop にならない）。再注入までは主張しない。
pub(super) fn inject_line(request: &Request, place: &super::StateDir, dir: &Path, signal: &Signal) -> TickDecision {
    let sending = inject::Request {
        target: request.target,
        socket: request.socket,
        payload: signal.payload,
        state_dir: Some(place),
    };
    match inject::deliver(&sending) {
        // 注入の断り（`busy` 等）は noop の語彙と字が重なるので、**前置きで分ける**。
        inject::Delivery::Refused(reason) | inject::Delivery::Unconfirmed(reason) => {
            TickDecision::Error(format!("inject-{reason}"))
        }
        inject::Delivery::Delivered(_, settled) => {
            match inject::nudge_enter(&sending, settled, signal.state) {
                inject::Settled::EnterLost => {
                    TickDecision::Inject(signal.kind, inject::Settled::EnterLost)
                }
                repaired => match heartbeat::touch_at(&stamp_path(dir)) {
                    Ok(()) => TickDecision::Inject(signal.kind, repaired),
                    Err(_) => TickDecision::Error(REASON_STAMP.to_owned()),
                },
            }
        }
    }
}

/// 判定の本体（記録の `what` と表示で**同じ字面**を使う）。context は判定の後ろ・cycle の前
/// （評価した順）。席の状態（`state=<値> event=<出所|none>`）は既存 token の**後ろに追加**する
/// （名前・順序・書式は不変・`s2-07l.95`・`event` は C10 の出所＝置き場の `source=` と混ぜない）。
/// cycle-stamp（`s2-07l.110`）は**その後ろ**（先に land した側の token が前・後から land した側が
/// その後ろ・planner 裁定 2026-09-12）で、cycle の評価まで進んだ周だけ載る。口座（`account=<label>:<pct>`）と
/// 立て直し（`relaunch=<label|none:理由>`・`s2-07l.211`）は同じ規律で**さらに後ろ**・評価した周だけ載る。
/// **置き場と出所は最後**（置き場が解けた周は判定に依らず載せる＝席側の打刻行と並べるだけで、
/// 別の dir を見ていることを記録から弁別できる・`s2-07l.70`）。
/// 注入した周は `consumed=<値>` の**直後**に理由（`reason=<語>`・queue と消費の周は無し）を足す
/// （`seat inject` の行と同じ並び・既存 token の名前と順序は不変・`s2-07l.150`）: 測れない周と
/// Enter が落ちた周を `false` と同じ顔で流さない（憲法 C10）。退避の合図を送った周は `kind=` の**直後**に出所
/// （`origin=<context|account|hook>`・`s2-07l.307`・それ以外の kind の周は載らない）、第 2 手で終了を確定した周は
/// 同じ位置に `detail=terminated`（`s2-07l.259`）、第 1 手が通らず同じ周で停止 → 立て直しまで進んだ周は `kind=relaunch` の
/// 同じ位置に `detail=after-terminated`（[`DETAIL_AFTER_TERMINATED`]・`s2-07l.314`・それ以外の周は載らない）。
pub(super) fn body(target: &str, judged: &Judged, place: &super::StateDir) -> String {
    let verdict = &judged.verdict;
    let head = match verdict.decision {
        TickDecision::Inject(kind, settled) => format!(
            "decision=inject target={} consumed={}{} kind={}{}{}",
            sanitize_target(target),
            settled.as_str(),
            settled.reason().map_or_else(String::new, |why| format!(" reason={why}")),
            kind.as_str(),
            verdict.origin.map_or_else(String::new, |found| format!(" origin={}", found.as_str())),
            verdict.detail.map_or_else(String::new, |found| format!(" detail={found}"))
        ),
        TickDecision::Noop(reason) => format!("decision=noop reason={}", reason.as_str()),
        TickDecision::Error(ref reason) => body_of_error(reason),
    };
    let with_context = format!("{head}{}", judged.context.suffix());
    let with_cycle = match verdict.cycled.as_deref() {
        Some(found) => format!("{with_context} cycle={found}"),
        None => with_context,
    };
    let with_state = judged.state.map_or(String::new(), state::Read::suffix);
    let with_stamp = verdict.stamp.map_or(String::new(), Stamped::suffix);
    let with_relaunch = verdict
        .relaunched
        .as_deref()
        .map_or_else(String::new, |found| format!(" relaunch={found}"));
    format!(
        "{with_cycle}{with_state}{with_stamp}{}{with_relaunch}{}",
        verdict.account.suffix(),
        place.suffix()
    )
}

/// 実行系が回らなかった周の本体。
pub(super) fn body_of_error(reason: &str) -> String {
    format!("decision=error reason={reason}")
}

/// 宣言 rule（cycle の確認の刻み）が読めない周の 1 行（`s2-07l.151`）。**判定を回さない**
/// ＝置き場も pane も見ない（置き場を解けない周と同じ早い側の断りで、記録も残らない）。
///
/// 語彙は [`decide`] の `no-rule` と同じ 1 つ: 読めない規則を理由に `decision=error` を名乗る
/// 形を 2 つに増やさない（理由の字面で routing する口を作らない・憲法 C11）。
pub fn render_no_rule() -> String {
    render(&body_of_error(meter::REASON_NO_RULE))
}

/// stdout / stderr へ出す 1 行。
pub(super) fn render(body: &str) -> String {
    format!("seat: tick {body}")
}

/// 1 回の記録（**全周 1 行**・判定行と同じ字面）。
pub(super) fn entry_of(target: &str, body: &str, started: Instant) -> InjectionRecord {
    InjectionRecord {
        schema: SCHEMA,
        who: WHO.to_owned(),
        what: body.to_owned(),
        when: WHEN.to_owned(),
        // 出力の byte 数＝判定行の長さ（注入 byte は便 2 の記録が持つ）。
        bytes: body.len() as u64,
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        seat: seat_name(target),
        ts: state::now_secs(),
    }
}

/// 1 回を席の記録へ積む。置き場が解けない周は書かない（rc は変えない）。
pub(super) fn record(state_dir: &Path, target: &str, entry: &InjectionRecord) {
    let Ok(policy) = LockPolicy::embedded() else {
        return;
    };
    let path = inject::tick_path(state_dir, target);
    let _ = store::append_line(&path, &entry.to_line(), policy);
}
