//! 管理 tick（設計 §3・裁定 (b)〜(e)・SRS FR27 / FR29 / FR21 / FR23・憲法 R-E12 / C10 / C11 / C2.2）。
//!
//! 席の**外**（host の timer）から回り、条件を**順序固定**で見て、成立した周だけ 1 行を
//! 注入する。R-E12 のとおり席は自分で周期起動を張らず、tick は席へ event としてしか届かない。
//!
//! **順序は load-bearing である**: 退避物の走査 → 鮮度 → pane 取得 → **context** → idle →
//! 未 consumed 退避物 → cycle lock。鮮度を pane より先に見るのは、fresh な周に tmux を 1 度も
//! 叩かないためで（生きている席を毎周 capture しない）、その鮮度より先に自席の退避物を走査する
//! のは、退避を終えた席は heartbeat を**直前**に打っていることが多く、退避物が在る（＝席が
//! 「作り直してよい」と申告している）のに鮮度で最大 `seat.tick_stale_s` の間 cycle が評価されない
//! ためである（`s2-07l.105`・裁定 (a)・admin2 で実測: 退避完了時 age 588 s → 約 30 分 idle）。
//! 飛ばすのは**自席の未 consumed 退避物が在る周だけ**（裁定 (c)・走査が読めない周は「在る」に
//! 読み替えず従来どおり鮮度から）で、飛ばした周もそれ以降の条件は不変（busy の席・lock が live な
//! 周には送らない）。context を idle の**前**に見るのは、context が cap を超えた席は
//! busy（lens 待ち・長い cargo）であり、busy を理由に noop すると誰にも止められず auto-compact に
//! 至るためである（`s2-07l.89`・実インシデント 2026-09-11）。退避物を lock より先に見るのは、
//! 「退避して止まっている席」を cycle の入口（裁定 (b)）へ落とすためである。順序を入れ替えると
//! 同じ条件でも別の理由が出る＝理由の字面は順序の証拠でもある。
//!
//! **代償 = 盲点は最大 `seat.tick_stale_s`**（宣言値）: 退避物の無い fresh の周は pane を読まない
//! ので、打刻の直後に cap を超えた席は次の stale な周まで見えない。閾値は動かさない（C5）＝この
//! 限界は設計 §3 に「残る側」として書く。

use super::{cycle, heartbeat, inject, is_idle, meter, pane_of, sanitize_target, WmScan};
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
/// 注入は済んだが自打刻を書けない（次の周も撃つ＝storm になるので断る）。
const REASON_STAMP: &str = "stamp-unwritable";
/// 退避を促す 1 行の skill 名（席の中で打つ command）。
const EXTERNALIZE_SKILL: &str = "/ready-compaction";

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
/// 条件のどれかが立たなかった**正常**で、[`Self::Error`] は判定そのものが回らなかった
/// 異常である。1 つに畳むと、席が静かなのか機械が壊れているのかを記録から読めなくなる。
pub enum TickDecision {
    /// 条件が揃った＝注入した（何を注入したか・席がその場で消費したかを添える）。
    Inject(InjectKind, inject::Settled),
    /// 条件が立たない＝撃たない（正常）。
    Noop(NoopReason),
    /// 実行系が回らない＝撃たない（異常・rc 1）。
    Error(String),
}

/// 注入した 1 行の種類。**bool で持たない**（憲法 C11）。
///
/// 2 つに分けるのは、促す行為が別物だからである——[`Self::Pointer`] は「打刻して続きへ」、
/// [`Self::Externalize`] は「退避せよ」。記録に種類が残らないと、席が退避の pointer を何度
/// 受けたか（storm の有無）を後から数えられない。
#[derive(Clone, Copy)]
pub enum InjectKind {
    /// 席に自分の打刻を促す 1 行（idle・退避物なし・lock 空きの揃った周）。
    Pointer,
    /// context が cap 以上の席へ退避を促す 1 行（idle を待たない）。
    Externalize,
}

impl InjectKind {
    /// 記録と表示に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pointer => "pointer",
            Self::Externalize => "externalize",
        }
    }
}

/// pane から読んだ context 使用率。**pane を取得した周だけ評価する**——fresh で取得しない周は
/// [`Self::Unevaluated`]（`cycle=` と同じ「評価していない」の印＝判定行に載らない）。
///
/// 測れない周は [`Self::Unmeasured`] で理由を 1 語持ち、**注入も停止もしない**（測れないことを
/// 理由に席を止めない・AC9 条 3 と同じ極性）。0% に化けさせない（FR25）。
#[derive(Clone, Copy)]
enum Context {
    /// pane を取得していない＝評価していない。
    Unevaluated,
    /// 測れた（使用率, cap）。cap を添えるのは判定と表示が同じ読みを使うため。
    Measured(u64, u64),
    /// 測れない（meter の 1 語）。
    Unmeasured(&'static str),
}

impl Context {
    /// 判定行の末尾に足す字面（評価していない周は空）。
    fn suffix(self) -> String {
        match self {
            Self::Unevaluated => String::new(),
            Self::Measured(pct, _) => format!(" context={pct}"),
            Self::Unmeasured(reason) => format!(" context=unmeasured reason={reason}"),
        }
    }
}

/// 判定 1 回の全体（判定・cycle の結果・context）。
struct Judged {
    /// 判定。
    decision: TickDecision,
    /// cycle を回した周の要約（回していない周は `None`）。
    cycled: Option<String>,
    /// context の評価。
    context: Context,
}

impl Judged {
    /// pane を取得する前に決まった周（context は評価していない）。
    fn bare(decision: TickDecision) -> Self {
        Self {
            decision,
            cycled: None,
            context: Context::Unevaluated,
        }
    }
}

/// pane を取得した後に判定へ渡す材料。
struct Seen<'a> {
    /// pane 本文。
    pane: &'a str,
    /// 自席の退避物の数え。
    wm: WmScan,
    /// context の評価。
    context: Context,
    /// cycle lock の TTL（秒）。
    ttl_s: u64,
}

/// 撃たなかった理由。**順序固定の条件のうち最初に立たなかったもの**を表す。
#[derive(Clone, Copy)]
pub enum NoopReason {
    /// 1. 席は最近動いている（自席の未 consumed 退避物が在る周は見ない・`.105`）。
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

/// 退避を促す 1 行（context が cap 以上の席へ）。退避 skill の名と実測値・cap を含め、
/// [`default_pointer`] と同じ理由で pane の幅に収まる短さに保つ。`--pointer` では上書きしない
/// （打刻の促しの上書きであって、実測値を運ぶ行を固定文字列に置き換える口ではない）。
pub fn externalize_pointer(pct: u64, cap: u64) -> String {
    format!("退避 tick: context {pct}% ≥ cap {cap}%・{EXTERNALIZE_SKILL} で退避してください")
}

/// tick を 1 回回す。
pub fn run(request: &Request) -> Outcome {
    let started = Instant::now();
    let Some(state) = super::state_dir_of(request.state_dir) else {
        // 置き場が無いと記録も打刻も持てない＝判定を回さない（撃たない側へ倒す）。
        return Outcome::failed_line(RC_REFUSED, render(&body_of_error(REASON_STATE_DIR)));
    };
    let dir = super::seat_dir(&state.path, request.target);
    let judged = decide(request, &state, &dir);
    let body = body(request.target, &judged, &state);
    record(&state.path, request.target, &body, started);
    match judged.decision {
        TickDecision::Error(_) => Outcome::failed_line(RC_REFUSED, render(&body)),
        TickDecision::Inject(..) | TickDecision::Noop(_) => Outcome::ok_line(render(&body)),
    }
}

/// 条件を順序固定で見る（退避物の走査 → 鮮度 → pane 取得 → context → idle → 退避物 → lock）。
///
/// 退避物を鮮度より先に走査するのは、**自席の未 consumed 退避物が在る周は鮮度 gate を飛ばす**
/// ためである（裁定 (a)・`s2-07l.105`）: 退避物の存在は席の「作り直してよい」の申告なので、
/// 直前の打刻を理由に pane も見ずに noop すると、退避から作り直しまでが最大 `seat.tick_stale_s`
/// に延びる。走査の結果は後段（context / 退避物）でも同じ値を使う（cycle を回す周だけは cycle 側が
/// 自分の入口でもう 1 度走査する＝lock の内側で確かめ直す）。
/// 他席の名乗り・0 件・読めない周は従来どおり鮮度から（読めない周を「在る」に読み替えない）。
fn decide(request: &Request, state: &super::StateDir, dir: &Path) -> Judged {
    let (Some(stale_s), Some(ttl_s)) = (super::int_rule(ID_STALE), cycle::ttl_s()) else {
        return Judged::bare(TickDecision::Error(meter::REASON_NO_RULE.to_owned()));
    };
    let wm = super::scan_wm(Path::new(request.wm_dir), request.target);
    let parked_here = matches!(wm, WmScan::Unconsumed(_));
    if !parked_here && is_fresh(dir, stale_s) {
        return Judged::bare(TickDecision::Noop(NoopReason::HeartbeatFresh));
    }
    let Some(pane) = pane_of(request.socket, request.target, request.capture_file) else {
        return Judged::bare(TickDecision::Noop(NoopReason::PaneMissing));
    };
    let seen = Seen {
        pane: &pane,
        wm,
        context: measure_context(&pane),
        ttl_s,
    };
    let (decision, cycled) = judge(request, state, dir, &seen);
    Judged {
        decision,
        cycled,
        context: seen.context,
    }
}

/// pane を取得した後の条件（context → idle → 退避物 → lock）。
fn judge(request: &Request, state: &super::StateDir, dir: &Path, seen: &Seen) -> (TickDecision, Option<String>) {
    // 他の cycle が走っている席（lock が live）には退避の pointer も送らない——作り直しの最中に
    // 行を queue しても、届く先は消えるか作り直された席である（排他は cycle 側と同じ 1 本の lock）。
    if let Some((pct, cap)) = over_cap(seen).filter(|_| !cycle::lock_is_live(dir, seen.ttl_s)) {
        let payload = externalize_pointer(pct, cap);
        return (inject_line(request, state, dir, InjectKind::Externalize, &payload), None);
    }
    if !is_idle(seen.pane) {
        return (TickDecision::Noop(NoopReason::Busy), None);
    }
    match seen.wm {
        WmScan::Unreadable => return (TickDecision::Noop(NoopReason::WmUnreadable), None),
        WmScan::Unconsumed(_) => return parked(request, state, dir, seen.ttl_s),
        WmScan::None => {}
    }
    if cycle::lock_is_live(dir, seen.ttl_s) {
        return (TickDecision::Noop(NoopReason::CycleLive), None);
    }
    let payload = request
        .pointer
        .map_or_else(|| default_pointer(request.target), str::to_owned);
    (inject_line(request, state, dir, InjectKind::Pointer, &payload), None)
}

/// context が cap 以上で、退避の pointer を送るべき周か（`(使用率, cap)`・cycle lock は呼び側が見る）。
///
/// **自席の未 consumed 退避物が 0 件と確認できた周に限る**（planner 裁定 2026-09-11）: 退避済みの
/// 席は `/clear` 前で cap 以上のままなので、退避物を見ずに注入すると毎周 pointer を重ねて
/// cycle に一度も落ちない（livelock）。`Unconsumed` は次の条件（idle → 退避物 → cycle）へ、
/// `Unreadable` は「0 件」と読み替えずに同じく次の条件へ（fail-closed 側＝注入しない）。
/// 測れない周も注入しない（測れないことを理由に止めもしない）。
fn over_cap(seen: &Seen) -> Option<(u64, u64)> {
    match (seen.context, &seen.wm) {
        (Context::Measured(pct, cap), WmScan::None) if pct >= cap => Some((pct, cap)),
        _ => None,
    }
}

/// pane 本文から context を評価する。cap も使用率も **meter の口**で読む（自前の literal も
/// parse も持たない＝guard / meter / tick の 3 面が同じ関数を見る）。
fn measure_context(pane: &str) -> Context {
    let Some(cap) = meter::declared_cap() else {
        return Context::Unmeasured(meter::REASON_NO_RULE);
    };
    match meter::used_from_pane_pct(pane) {
        Ok((pct, _, _)) => Context::Measured(pct, cap),
        Err(reason) => Context::Unmeasured(reason),
    }
}

/// 退避して止まっている周（裁定 (b)）: lock が空いていれば cycle を**その場で**回す。
///
/// 回すのは「退避物が在る ∧ idle ∧ lock が空いている」周だけで、**それ以外の周は cycle を
/// 評価しない**——tick 行に `cycle=` が付かないこと自体が「評価していない」の印である。
fn parked(
    request: &Request,
    state: &super::StateDir,
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

/// 1 行を注入し、成立したら自打刻する（退避の pointer も打刻の pointer も同じ経路＝次の周は
/// fresh で撃たない・storm 止め）。busy な席へは queue の形で届く（`.90`）。
fn inject_line(
    request: &Request,
    state: &super::StateDir,
    dir: &Path,
    kind: InjectKind,
    payload: &str,
) -> TickDecision {
    let sent = inject::deliver(&inject::Request {
        target: request.target,
        socket: request.socket,
        payload,
        state_dir: Some(state),
    });
    match sent {
        // 注入の断り（`busy` 等）は noop の語彙と字が重なるので、**前置きで分ける**。
        inject::Delivery::Refused(reason) | inject::Delivery::Unconfirmed(reason) => {
            TickDecision::Error(format!("inject-{reason}"))
        }
        inject::Delivery::Delivered(_, settled) => match heartbeat::touch_at(&stamp_path(dir)) {
            Ok(()) => TickDecision::Inject(kind, settled),
            Err(_) => TickDecision::Error(REASON_STAMP.to_owned()),
        },
    }
}

/// 判定の本体（記録の `what` と表示で**同じ字面**を使う）。context は判定の後ろ・cycle の前
/// （評価した順）。**置き場と出所は最後**（置き場が解けた周は判定に依らず載せる＝席側の打刻行と
/// 並べるだけで、別の dir を見ていることを記録から弁別できる・`s2-07l.70`）。
fn body(target: &str, judged: &Judged, state: &super::StateDir) -> String {
    let head = match judged.decision {
        TickDecision::Inject(kind, settled) => format!(
            "decision=inject target={} consumed={} kind={}",
            sanitize_target(target),
            settled.as_str(),
            kind.as_str()
        ),
        TickDecision::Noop(reason) => format!("decision=noop reason={}", reason.as_str()),
        TickDecision::Error(ref reason) => body_of_error(reason),
    };
    let with_context = format!("{head}{}", judged.context.suffix());
    let with_cycle = match judged.cycled.as_deref() {
        Some(found) => format!("{with_context} cycle={found}"),
        None => with_context,
    };
    format!("{with_cycle}{}", state.suffix())
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
