//! 管理 tick（設計 §3・裁定 (b)〜(e)・SRS FR27 / FR21 / FR23・憲法 R-E12 / C10 / C11 / C2.2）。
//!
//! 席の**外**（host の timer）から回り、4 条件を**順序固定の AND** で見て、成立した周だけ
//! 1 行を注入する。R-E12 のとおり席は自分で周期起動を張らず、tick は席へ event としてしか
//! 届かない。
//!
//! **順序は load-bearing である**。鮮度を最初に見るのは、fresh な周に tmux を 1 度も
//! 叩かないためで（生きている席を毎周 capture しない）、退避物を lock より先に見るのは、
//! 「退避して止まっている席」を cycle の入口（裁定 (b)）へ落とすためである。順序を入れ替えると
//! 同じ 4 条件でも別の理由が出る＝理由の字面は順序の証拠でもある。

use super::{cycle, heartbeat, inject, is_idle, pane_of, sanitize_target, WmScan};
use crate::cli_outcome::{Outcome, RC_REFUSED};
use crate::fleet::store::{self, LockPolicy};
use crate::hook::{InjectionRecord, SCHEMA};
use crate::name::NAME;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// 自打刻 marker の名前。
pub const STAMP_FILE: &str = "tick-stamp";
/// stale 閾値を宣言する rules 行の id（**値は code に焼かない**・憲法 C5）。
const ID_STALE: &str = "seat.tick_stale_s";
/// 記録の who。
const WHO: &str = "seat-tick";
/// 記録の when。
const WHEN: &str = "tick";
/// 置き場を解けない。
const REASON_STATE_DIR: &str = "state-dir";
/// 閾値の宣言（rules 行）が読めない。
const REASON_NO_RULE: &str = "no-rule";
/// 注入は済んだが自打刻を書けない（次の周も撃つ＝storm になるので断る）。
const REASON_STAMP: &str = "stamp-unwritable";

/// tick 1 回の入力。
pub struct Request<'a> {
    /// tmux target（pane の指定）。
    pub target: &'a str,
    /// 退避物の dir。
    pub wm_dir: &'a str,
    /// 注入する 1 行（既定は [`default_pointer`]）。
    pub pointer: Option<&'a str>,
    /// tmux の socket。
    pub socket: Option<&'a str>,
    /// pane 本文の代わりに読む file。
    pub capture_file: Option<&'a str>,
    /// 記録と marker の置き場。
    pub state_dir: Option<&'a str>,
    /// cycle を回す周に渡す復元 command。
    pub restore: Option<&'a str>,
}

/// tick 1 回の判定。**bool で持たない**（憲法 C11）。
///
/// 「撃たなかった」が 2 つに分かれているのは、理由が別物だからである——[`Self::Noop`] は
/// 4 条件のどれかが立たなかった**正常**で、[`Self::Error`] は判定そのものが回らなかった
/// 異常である。1 つに畳むと、席が静かなのか機械が壊れているのかを記録から読めなくなる。
pub enum TickDecision {
    /// 4 条件が揃った＝注入した。
    Inject,
    /// 条件が立たない＝撃たない（正常）。
    Noop(NoopReason),
    /// 実行系が回らない＝撃たない（異常・rc 1）。
    Error(String),
}

/// 撃たなかった理由。**順序固定の 4 条件のうち最初に立たなかったもの**を表す。
#[derive(Clone, Copy)]
pub enum NoopReason {
    /// 1. 席は最近動いている。
    HeartbeatFresh,
    /// 2. pane を読めない。
    PaneMissing,
    /// 2. 席が打ちかけである。
    Busy,
    /// 3. 自席の未 consumed 退避物が在る（＝退避して止まっている）。
    WmUnconsumed,
    /// 3. 退避物の dir を読めない（**0 件と読み替えない**）。
    WmUnreadable,
    /// 4. 他の cycle が走っている。
    CycleLive,
}

impl NoopReason {
    /// 記録と表示に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HeartbeatFresh => "heartbeat-fresh",
            Self::PaneMissing => "pane-missing",
            Self::Busy => "busy",
            Self::WmUnconsumed => "wm-unconsumed",
            Self::WmUnreadable => "wm-unreadable",
            Self::CycleLive => "cycle-live",
        }
    }
}

/// 自打刻 marker の path。
pub fn stamp_path(seat_dir: &Path) -> PathBuf {
    seat_dir.join(STAMP_FILE)
}

/// 既定の注入 1 行。席に自分の打刻を促し、続きへ戻す。
///
/// **短く保つ**: 注入の送達確認は pane に現れた字面で測るので、pane の幅を超えて折り返すと
/// 送達したのに確認できない周が出る。
pub fn default_pointer(target: &str) -> String {
    format!("管理 tick: {NAME} seat heartbeat --target {target} を撃ち、続きを進めてください")
}

/// tick を 1 回回す。
pub fn run(request: &Request) -> Outcome {
    let started = Instant::now();
    let Some(state) = super::state_dir_of(request.state_dir) else {
        // 置き場が無いと記録も打刻も持てない＝判定を回さない（撃たない側へ倒す）。
        return Outcome::failed_line(RC_REFUSED, render(&body_of_error(REASON_STATE_DIR)));
    };
    let dir = super::seat_dir(&state, request.target);
    let (decision, cycled) = decide(request, &state, &dir);
    let body = body(request.target, &decision, cycled.as_deref());
    record(&state, request.target, &body, started);
    match decision {
        TickDecision::Error(_) => Outcome::failed_line(RC_REFUSED, render(&body)),
        TickDecision::Inject | TickDecision::Noop(_) => Outcome::ok_line(render(&body)),
    }
}

/// 4 条件を順序固定で見る。
fn decide(request: &Request, state: &Path, dir: &Path) -> (TickDecision, Option<String>) {
    let (Some(stale_s), Some(ttl_s)) = (super::int_rule(ID_STALE), cycle::ttl_s()) else {
        return (TickDecision::Error(REASON_NO_RULE.to_owned()), None);
    };
    if is_fresh(dir, stale_s) {
        return (TickDecision::Noop(NoopReason::HeartbeatFresh), None);
    }
    let Some(pane) = pane_of(request.socket, request.target, request.capture_file) else {
        return (TickDecision::Noop(NoopReason::PaneMissing), None);
    };
    if !is_idle(&pane) {
        return (TickDecision::Noop(NoopReason::Busy), None);
    }
    match super::scan_wm(Path::new(request.wm_dir), request.target) {
        WmScan::Unreadable => return (TickDecision::Noop(NoopReason::WmUnreadable), None),
        WmScan::Unconsumed(_) => return parked(request, state, dir, ttl_s),
        WmScan::None => {}
    }
    if cycle::lock_is_live(dir, ttl_s) {
        return (TickDecision::Noop(NoopReason::CycleLive), None);
    }
    (inject_pointer(request, state, dir), None)
}

/// 退避して止まっている周（裁定 (b)）: lock が空いていれば cycle を**その場で**回す。
///
/// 回すのは「退避物が在る ∧ idle ∧ lock が空いている」周だけで、**それ以外の周は cycle を
/// 評価しない**——tick 行に `cycle=` が付かないこと自体が「評価していない」の印である。
fn parked(
    request: &Request,
    state: &Path,
    dir: &Path,
    ttl_s: u64,
) -> (TickDecision, Option<String>) {
    let noop = TickDecision::Noop(NoopReason::WmUnconsumed);
    if cycle::lock_is_live(dir, ttl_s) {
        return (noop, None);
    }
    let result = cycle::run(&cycle::Request {
        target: request.target,
        wm_dir: request.wm_dir,
        socket: request.socket,
        capture_file: request.capture_file,
        state_dir: state,
        restore: request.restore,
    });
    (noop, Some(cycle::summary(&result)))
}

/// 鮮度: `now − max(heartbeat, tick-stamp)` が閾値**以下**か。**両方不在なら stale**。
///
/// 自分の打刻も見るのは、注入した直後の周が「席がまだ打刻していない」を理由に撃ち続ける
/// storm を塞ぐためである（裁定 (d)）。mtime が未来の周は fresh 側＝撃たない側へ倒す。
fn is_fresh(seat_dir: &Path, stale_s: u64) -> bool {
    [heartbeat::path(seat_dir), stamp_path(seat_dir)]
        .iter()
        .filter_map(|marker| {
            std::fs::metadata(marker)
                .and_then(|meta| meta.modified())
                .ok()
        })
        .any(|at| at.elapsed().map_or(true, |age| age.as_secs() <= stale_s))
}

/// 1 行を注入し、成立したら自打刻する。
fn inject_pointer(request: &Request, state: &Path, dir: &Path) -> TickDecision {
    let Some(state_str) = state.to_str() else {
        return TickDecision::Error(REASON_STATE_DIR.to_owned());
    };
    let payload = request
        .pointer
        .map_or_else(|| default_pointer(request.target), str::to_owned);
    let sent = inject::deliver(&inject::Request {
        target: request.target,
        socket: request.socket,
        payload: &payload,
        state_dir: Some(state_str),
    });
    match sent {
        // 注入の断り（`busy` 等）は noop の語彙と字が重なるので、**前置きで分ける**。
        inject::Delivery::Refused(reason) | inject::Delivery::Unconfirmed(reason) => {
            TickDecision::Error(format!("inject-{reason}"))
        }
        inject::Delivery::Delivered(_) => match heartbeat::touch_at(&stamp_path(dir)) {
            Ok(()) => TickDecision::Inject,
            Err(_) => TickDecision::Error(REASON_STAMP.to_owned()),
        },
    }
}

/// 判定の本体（記録の `what` と表示で**同じ字面**を使う）。
fn body(target: &str, decision: &TickDecision, cycled: Option<&str>) -> String {
    let head = match *decision {
        TickDecision::Inject => format!("decision=inject target={}", sanitize_target(target)),
        TickDecision::Noop(reason) => format!("decision=noop reason={}", reason.as_str()),
        TickDecision::Error(ref reason) => body_of_error(reason),
    };
    match cycled {
        Some(found) => format!("{head} cycle={found}"),
        None => head,
    }
}

/// 実行系が回らなかった周の本体。
fn body_of_error(reason: &str) -> String {
    format!("decision=error reason={reason}")
}

/// stdout / stderr へ出す 1 行。
fn render(body: &str) -> String {
    format!("seat: tick {body}")
}

/// 1 回を記録する（**全周 1 行**）。置き場が解けない周は書かない（rc は変えない）。
fn record(state_dir: &Path, target: &str, body: &str, started: Instant) {
    let entry = InjectionRecord {
        schema: SCHEMA,
        who: WHO.to_owned(),
        what: body.to_owned(),
        when: WHEN.to_owned(),
        // 出力の byte 数＝判定行の長さ（注入 byte は便 2 の記録が持つ）。
        bytes: body.len() as u64,
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    };
    let Ok(policy) = LockPolicy::embedded() else {
        return;
    };
    let path = inject::tick_path(state_dir, target);
    let _ = store::append_line(&path, &entry.to_line(), policy);
}
