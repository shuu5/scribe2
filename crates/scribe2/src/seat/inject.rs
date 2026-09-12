//! tmux pane への注入（設計 §3・記録は SRS FR21 と同じ schema）。
//!
//! **送る前に入力欄を見る**: 人間の打ちかけと 1 行に merge する co-submit 事故を、
//! 「非空なら 1 key も送らない」で構造的に塞ぐ（prompt 行を特定できない周も送らない
//! ＝fail-closed）。rc は **0 / 1 の 2 値**だけで、v1 の偽陰性（4 / 7）を作らない。
//!
//! pane を読むのは**入力欄の門と送達の目印**（送った字面が現れた = 送達・`.90`）だけで、
//! **消費（`consumed=`）は席の打刻**で決める（送達 ts 以後の `UserPromptSubmit`・
//! [`state::evidence_after`]・設計 seat-state.md §6・`s2-07l.112`）。入力欄が空になったかは読まない。
//!
//! 不可逆の口は持たない（憲法 CON5）: ここが送るのは呼び側が渡した 1 行だけで、
//! `/clear` のような session を作り直す注入はこの便では扱わない。

use crate::polarity::{OnFailure, Polarity, Timing};
use super::{capture, input_tail, sanitize_target, state, tmux_ok, StateDir};
use crate::fleet::store::{self, LockPolicy};
use crate::hook::{InjectionRecord, SCHEMA};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// settle の 1 回あたりの待ち。
const SETTLE_STEP: Duration = Duration::from_millis(200);
/// settle の最大回数（既定の窓 = 2 s）。呼び側が窓を渡す口は [`deliver_within`]。
const SETTLE_TRIES: u32 = 10;
/// 記録に載せる payload の先頭 byte 数。
const WHAT_CAP: usize = 80;
/// 記録 file の名前。
const TICK_FILE: &str = "tick.jsonl";
/// 記録の置き場（state dir 直下の dir 名）。
const SEAT_DIR: &str = "seat";
/// 記録の who。
const WHO: &str = "seat-inject";
/// 記録の when。
const WHEN: &str = "inject";

/// 入力欄が非空（人間が打ちかけている）。
pub const REASON_BUSY: &str = "busy";
/// 入力欄を特定できない。
pub const REASON_UNKNOWN_INPUT: &str = "unknown-input";
/// payload に非空の行が 1 つも無い（送達の目印を持てない）。
pub const REASON_EMPTY: &str = "empty";
/// 送ったが pane に現れない。
pub const REASON_ABSENT: &str = "absent";
/// tmux を撃てなかった。
pub const REASON_TMUX_FAILED: &str = "tmux-failed";

/// 注入 1 回の入力。
pub struct Request<'a> {
    /// tmux target（pane の指定）。
    pub target: &'a str,
    /// tmux の socket（既定の server を使うなら `None`）。
    pub socket: Option<&'a str>,
    /// 送る 1 行。
    pub payload: &'a str,
    /// 解決済みの置き場（出所付き・`None` = 解けない周＝記録しない・表示に 2 語を出さない）。
    /// **解決は呼び側が 1 回だけ行う**: 表示と記録が別々に解くと、settle の窓の内に git 設定が
    /// 変わった周に表示行が書いてもいない dir を名乗る（lens-100 HIGH-1・実測 2026-09-11）。
    pub state_dir: Option<&'a StateDir>,
}

/// 送達した注入を席が**その場で消費したか**（送達 ts 以後に `UserPromptSubmit` の打刻が足されたか・
/// 設計 §6・`s2-07l.112`）。
///
/// 席が busy な周は注入が入力欄に queue され、turn が終わる（次の submit）まで打刻が来ない。それでも
/// 送達は成功している（実測 2026-09-11: `inject-residual` 9 件が全部 turn の終わりに消費されていた・
/// bd `s2-07l.90`）ので、これは成功の**記録の detail** であって失敗の理由ではない。
#[derive(Clone, Copy)]
pub enum Settled {
    /// 送達 ts 以後の `UserPromptSubmit` の打刻が在る＝席が消費した。
    Consumed,
    /// 打刻 file は読めるが窓の内に新しい打刻が無い＝queue された（次の submit で消費される）。
    Queued,
    /// 消費を**測れない**（打刻 file が無い・読めない・置き場が解けない）。`false` と混ぜず、
    /// missing を消費と読み替えない（憲法 C10 の測定 / 未測定の弁別）。理由は [`Unmeasured`]。
    Unmeasured(Unmeasured),
}

/// 消費を測れない理由。**閉じた 3 値**（憲法 C11）。
#[derive(Clone, Copy)]
pub enum Unmeasured {
    /// 打刻 file が無い（hook が載っていない席）。
    StateMissing,
    /// 打刻 file を読めない。
    StateUnreadable,
    /// 置き場が解けない（打刻の在処を知らない）。
    StateDir,
}

impl Unmeasured {
    /// 行に添える字面（`reason=<値>`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StateMissing => "state-missing",
            Self::StateUnreadable => "state-unreadable",
            Self::StateDir => "state-dir",
        }
    }
}

impl Settled {
    /// 記録と表示の字面（`consumed=true|false|unknown`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Consumed => "true",
            Self::Queued => "false",
            Self::Unmeasured(_) => "unknown",
        }
    }

    /// 測れなかった理由（測れた周は `None`）。
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::Consumed | Self::Queued => None,
            Self::Unmeasured(why) => Some(why.as_str()),
        }
    }
}

/// この境界の極性（[`Delivery::Refused`]）: 送る前に入力欄を見て、非空・prompt 行を特定できない周は 1 key も送らない。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 注入 1 回の結果。
pub enum Delivery {
    /// 送って送達を確認した（byte 数・その場で消費したか）。
    Delivered(u64, Settled),
    /// **1 key も送っていない**（入力欄の状態で止めた）。
    Refused(&'static str),
    /// 送ったが送達を確認できない。
    Unconfirmed(&'static str),
}

/// 注入を 1 回行う（settle の窓は既定の 2 s）。
pub fn deliver(request: &Request) -> Delivery {
    deliver_within(request, SETTLE_STEP.saturating_mul(SETTLE_TRIES))
}

/// 注入を 1 回行い、settle を `window` まで見続ける。
///
/// 成功の形は [`deliver`] と同じ（目印が現れた = 送達・送達 ts 以後の `UserPromptSubmit` の打刻 =
/// `Consumed`・窓の終わりに無ければ `Queued`）で、変わるのは**窓の長さだけ**。作り直し直後の席は
/// SessionStart hook の間（数秒〜十数秒）注入を入力欄に queue したまま turn を始めないので、2 s の
/// 窓では復元が正しく届く周ほど `Queued` に落ちる（bd `s2-07l.97`）。cycle は作り直しの確認と同じ
/// 上限を渡す。**窓はここで決めない**（`s2-07l.151`）: cycle 側の rules 行
/// （`seat.cycle_settle_s`）が持つ値がそのまま引数で来る＝この面は規則を読まない。
pub fn deliver_within(request: &Request, window: Duration) -> Delivery {
    let started = Instant::now();
    let Some(pane) = capture(request.socket, request.target) else {
        return Delivery::Unconfirmed(REASON_TMUX_FAILED);
    };
    if let Err(gate) = guard_input(&pane) {
        return Delivery::Refused(gate.as_str());
    }
    // 送る**前**の pane で目印の出現数を数えておく: 同じ字面が先に在る（前周の pointer の写し・
    // tool の出力の引用）と `contains` 1 本では届いていない周が「届いた」に化ける（lens-90 HIGH-1）。
    let Some(marker) = marker_of(request.payload) else {
        return Delivery::Refused(REASON_EMPTY);
    };
    let before = pane.matches(marker).count();
    // 消費の証拠を見る先も送る**前**に取る（基線と送達 ts・設計 §6）。
    let seat = request
        .state_dir
        .map(|found| super::seat_dir(&found.path, request.target));
    let watch = Watch {
        seat: seat.as_deref().map(|dir| (dir, state::baseline(dir))),
        since: state::now_secs(),
    };
    if !send(request) {
        return Delivery::Unconfirmed(REASON_TMUX_FAILED);
    }
    match settle(request, marker, before, tries_within(window), &watch) {
        Ok(settled) => {
            let bytes = request.payload.len() as u64;
            record(request, bytes, started);
            Delivery::Delivered(bytes, settled)
        }
        Err(reason) => Delivery::Unconfirmed(reason),
    }
}

/// 消費の証拠を見る先（送る**前**に取る・設計 §6）。置き場が解けない周は `seat` が `None`＝測れない。
struct Watch<'a> {
    /// 席の置き場と、送る前の打刻の基線。
    seat: Option<(&'a Path, state::Baseline)>,
    /// 送達 ts（打刻と同じ時計・秒）。
    since: u64,
}

impl Watch<'_> {
    /// いまの証拠の読み（置き場が解けない周は `None`）。
    fn evidence(&self) -> Option<state::Evidence> {
        self.seat.map(|(dir, baseline)| {
            state::evidence_after(dir, baseline, state::Event::UserPromptSubmit, self.since)
        })
    }
}

/// 窓の終わりの証拠を [`Settled`] に写す（`Found` は途中で返るので届かないが、網羅のため写す）。
fn settled_of(evidence: Option<state::Evidence>) -> Settled {
    match evidence {
        Some(state::Evidence::Found(_)) => Settled::Consumed,
        Some(state::Evidence::NotYet) => Settled::Queued,
        Some(state::Evidence::Missing) => Settled::Unmeasured(Unmeasured::StateMissing),
        Some(state::Evidence::Unreadable) => Settled::Unmeasured(Unmeasured::StateUnreadable),
        None => Settled::Unmeasured(Unmeasured::StateDir),
    }
}

/// 入力欄の門の断り。**閉じた 2 値**（憲法 C11: 境界ごとの enum・字面で routing しない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputGate {
    /// 入力欄が非空（人間が打ちかけている）。
    Busy,
    /// prompt 行を特定できない。
    UnknownInput,
}

impl InputGate {
    /// 断りの 1 行に使う字面（[`REASON_BUSY`] / [`REASON_UNKNOWN_INPUT`]）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Busy => REASON_BUSY,
            Self::UnknownInput => REASON_UNKNOWN_INPUT,
        }
    }
}

/// 送る前の入力欄の門（co-submit 止め・**送達の面の唯一の字面読み**）: prompt 行を特定できない
/// pane は [`InputGate::UnknownInput`]、入力欄が非空なら [`InputGate::Busy`] で、どちらも 1 key も
/// 送らない側へ倒す。席の busy / idle の判定ではない（それは [`super::state`] が typed に持つ・
/// ADR-0015）——人間の打ちかけと 1 行に merge する事故を、送る直前の入力欄で塞ぐ門である。
/// cycle の `/clear` も同じ門を通る（第 2 の判定を作らない）。
pub fn guard_input(pane: &str) -> Result<(), InputGate> {
    match input_tail(pane) {
        None => Err(InputGate::UnknownInput),
        Some(tail) if !tail.is_empty() => Err(InputGate::Busy),
        Some(_) => Ok(()),
    }
}

/// payload を literal で送り、Enter を送る。
fn send(request: &Request) -> bool {
    let target = request.target;
    tmux_ok(
        request.socket,
        &["send-keys", "-t", target, "-l", request.payload],
    ) && tmux_ok(request.socket, &["send-keys", "-t", target, "Enter"])
}

/// 送達の目印 = payload の**最初の非空行**。無ければ `None`（呼び側は 1 key も送らず断る）。
///
/// 先頭行が空だと目印が空文字になり、出現数が pane の長さに化けて「pane が伸びた」だけで
/// 成立する（lens-90 再確認 NEW-1・stdin を読まない席でも `consumed=true` になった）。
fn marker_of(payload: &str) -> Option<&str> {
    payload.lines().find(|line| !line.trim().is_empty())
}

/// 送達を確認する。**目印（最初の非空行）の出現数が送る前より増えた**ら成立で、消費（送達 ts 以後の
/// `UserPromptSubmit` の打刻）が窓の内に来たかを [`Settled`] として添える。
///
/// 「現れた ∧ 消費した」を成立の条件にすると、busy な席へ queue された注入（届いている）を
/// 失敗と数える（bd `s2-07l.90`・裁定: 現れた ＝ 成功）。失敗は `absent`（現れない）と
/// `tmux-failed` の 2 つだけ。「在る」でなく「増えた」で見るのは、先に同じ字面が pane に在る周
/// （tick の pointer は固定文字列で前周の写しが残る）に届いていない注入を成功と数えないため。
/// 代償は、窓の内に pane が巻き上がって**古い写しだけ**が消えた周（新しい写しが見えていても
/// 総数は増えない）が `absent` へ倒れうること（fail-closed 側の誤り・tick では重複注入になる。
/// 周ごとに一意な目印にする案は「注入の内容を変えない」の契約外＝lens-90 再確認 NEW-2）。
/// `consumed` は**窓の終わりの証拠**で決める（入力欄が空かは読まない・`s2-07l.112`）。
fn settle(
    request: &Request,
    marker: &str,
    before: usize,
    tries: u32,
    watch: &Watch<'_>,
) -> Result<Settled, &'static str> {
    let mut seen = false;
    let mut evidence = None;
    for _ in 0..tries {
        sleep(SETTLE_STEP);
        let Some(pane) = capture(request.socket, request.target) else {
            return Err(REASON_TMUX_FAILED);
        };
        seen = seen || pane.matches(marker).count() > before;
        evidence = watch.evidence();
        if seen && matches!(evidence, Some(state::Evidence::Found(_))) {
            return Ok(Settled::Consumed);
        }
    }
    if !seen {
        return Err(REASON_ABSENT);
    }
    Ok(settled_of(evidence))
}

/// 窓を settle の回数へ写す（[`SETTLE_STEP`] 刻み・**1 回は必ず見る**・既定の窓なら
/// [`SETTLE_TRIES`] と同じ値）。
fn tries_within(window: Duration) -> u32 {
    let step = SETTLE_STEP.as_millis().max(1);
    u32::try_from(window.as_millis() / step)
        .unwrap_or(u32::MAX)
        .max(1)
}

/// 記録 file の path。
pub fn tick_path(state_dir: &Path, target: &str) -> PathBuf {
    state_dir
        .join(SEAT_DIR)
        .join(sanitize_target(target))
        .join(TICK_FILE)
}

/// 送達した 1 回を記録する。**置き場が解けない周は書かない**（rc は変えない）。
///
/// `what` は payload の先頭そのまま（現物を加工しない）。その場で消費したかは tick の記録
/// （`decision=inject … consumed=…`）と `seat inject` の stdout 行が持つ（planner 裁定 2026-09-11・
/// 記録 schema は FR21 と共有ゆえ field は足さない）。
fn record(request: &Request, bytes: u64, started: Instant) {
    let Some(dir) = request.state_dir.map(|state| state.path.as_path()) else {
        return;
    };
    let entry = InjectionRecord {
        schema: SCHEMA,
        who: WHO.to_owned(),
        what: head(request.payload, WHAT_CAP),
        when: WHEN.to_owned(),
        bytes,
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    };
    let Ok(policy) = LockPolicy::embedded() else {
        return;
    };
    // 記録の失敗で注入の結果（rc）を変えない（FR21 は推奨で、判定そのものではない）。
    let _ = store::append_line(&tick_path(dir, request.target), &entry.to_line(), policy);
}

/// payload の先頭 `cap` byte（**文字の途中で切らない**）。
fn head(payload: &str, cap: usize) -> String {
    let mut end = 0;
    for (at, ch) in payload.char_indices() {
        let next = at.saturating_add(ch.len_utf8());
        if next > cap {
            break;
        }
        end = next;
    }
    payload.get(..end).unwrap_or_default().to_owned()
}

/// 表示行の末尾に足す置き場と出所（`.70` の 2 語）。**置き場が解けない周は空**（2 語を出さない）。
///
/// inject の所在は表示行が担う: 記録の `what` は payload の先頭のまま加工しない（FR21 と schema を
/// 共有・planner 裁定 2026-09-11）。
pub fn suffix_of(state: Option<&StateDir>) -> String {
    state.map_or_else(String::new, StateDir::suffix)
}

/// 成立の 1 行（その場で消費したかを添える）。測れなかった周だけ `reason=` を **`consumed=` の直後**に
/// 添える（既存 token の名前・順序は不変・置き場の 2 語は行末のまま）。
pub fn render_delivered(target: &str, bytes: u64, settled: Settled, state: Option<&StateDir>) -> String {
    let reason = settled
        .reason()
        .map_or_else(String::new, |why| format!(" reason={why}"));
    format!(
        "seat: inject delivered target={} bytes={bytes} consumed={}{reason}{}",
        sanitize_target(target),
        settled.as_str(),
        suffix_of(state)
    )
}

/// **送っていない**断りの 1 行。
pub fn render_refused(reason: &str, state: Option<&StateDir>) -> String {
    format!("seat: inject refused reason={reason}{}", suffix_of(state))
}

/// 送ったが確認できなかった 1 行。
pub fn render_unconfirmed(reason: &str, state: Option<&StateDir>) -> String {
    format!("seat: inject unconfirmed reason={reason}{}", suffix_of(state))
}

#[cfg(test)]
mod tests {
    use super::{tries_within, SETTLE_STEP, SETTLE_TRIES};
    use std::time::Duration;

    /// 既定の窓（[`SETTLE_STEP`] × [`SETTLE_TRIES`]）は従来と同じ回数に写る（`deliver` の
    /// 既定 2 s が変わらないことの pin）。掛け算や剰余に化けた写しはここで落ちる。
    #[test]
    fn inject_tries_within_default_window_is_settle_tries() {
        assert_eq!(
            tries_within(SETTLE_STEP.saturating_mul(SETTLE_TRIES)),
            SETTLE_TRIES
        );
    }

    /// cycle が渡す上限（30 s）は 200 ms 刻みで 150 回（hook の実行を跨ぐ長さ）。
    #[test]
    fn inject_tries_within_cycle_limit_spans_hook_run() {
        assert_eq!(tries_within(Duration::from_secs(30)), 150);
    }

    /// 刻みより短い窓・空の窓でも **1 回は必ず見る**（0 回だと目印を見ずに `absent` に倒れる）。
    #[test]
    fn inject_tries_within_never_zero() {
        assert_eq!(tries_within(Duration::ZERO), 1);
        assert_eq!(tries_within(Duration::from_millis(100)), 1);
    }

    /// 巨大な窓は u32 で飽和させる（panic しない・C11）。
    #[test]
    fn inject_tries_within_saturates_on_huge_window() {
        assert_eq!(tries_within(Duration::MAX), u32::MAX);
    }
}
