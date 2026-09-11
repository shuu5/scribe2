//! 管理 tick（設計 §3・裁定 (b)〜(e)・SRS FR27 / FR29 / FR21 / FR23・憲法 R-E12 / C10 / C11 / C2.2）。
//!
//! 席の**外**（host の timer）から回り、条件を**順序固定**で見て、成立した周だけ 1 行を
//! 注入する。R-E12 のとおり席は自分で周期起動を張らず、tick は席へ event としてしか届かない。
//!
//! **順序は load-bearing である**: 退避物の走査 → **状態**（state.jsonl の最終行を読む）→ pane 取得 →
//! **context**（cap 以上なら退避の合図・状態の門の外）→ 状態の門（Idle だけが進む）→ 未 consumed
//! 退避物 → cycle lock → 打刻の合図の brake（tick-stamp）。**heartbeat の鮮度 gate は持たない**
//! （`s2-07l.109`）: 鮮度 gate は「走行中の席の pane を字面で読んで誤判定する」のを避ける門だったが、
//! 席の状態が hook の打刻（typed・`s2-07l.95`）になって pane を idle の判定入力にしなくなり、理由が
//! 消えた。残していた害は「打刻の直後に cap を超えた席が最大 `seat.tick_stale_s` の間 退避の合図を
//! 受けない」盲点で（`s2-07l.105` が退避物の在る周だけ飛ばす特例で一部を塞いだ）、撤去で盲点は
//! tick の周期だけになる。heartbeat / tick-stamp の file は「席が生きている」の記録として残す
//! （`seat heartbeat` と合図の文面は不変・判定入力ではない）。退避の合図を状態の門の
//! **前**に置くのは、context が cap を超えた席は
//! busy（lens 待ち・長い cargo）であり、busy を理由に noop すると誰にも止められず auto-compact に
//! 至るためである（`s2-07l.89`・実インシデント 2026-09-11・SRS FR29「idle を待たずに」＝planner
//! 裁定 2026-09-11: FR29 > ADR-0015 §2.3）。退避物を lock より先に見るのは、「退避して止まっている席」
//! を cycle の入口（裁定 (b)）へ落とすためである。順序を入れ替えると同じ条件でも別の理由が出る＝
//! 理由の字面は順序の証拠でもある。
//!
//! **席の busy / idle は hook の打刻（[`state`]）が一次で、pane の字面は判定入力にしない**（憲法 C3.3・
//! ADR-0015）。打刻が無い・読めない・Busy が古い周は理由を分けて注入しない（fail-closed）。pane を
//! 読むのは context（statusline の数値）と注入の送達確認だけである。
//!
//! **打刻の合図（pointer）の頻度は tick 自身の打刻で決める**（planner 裁定 2026-09-12・案 A）: 合図を
//! 注入した周は tick-stamp を打ち、その mtime が `seat.tick_stale_s` **未満**の周は合図を送らない
//! （`pointer-recent`・`.110` の `cycle-recent` と同型・閾値は共用）。brake が掛かるのは合図だけで、
//! 退避の合図と cycle の評価はその周も行う（heartbeat の mtime は見ない＝FR27 の合図は席の生存の
//! 記録でなく「続きを進めろ」の促し）。

use super::{cycle, heartbeat, inject, meter, pane_of, sanitize_target, state, WmScan};
use crate::cli_outcome::{Outcome, RC_REFUSED};
use crate::fleet::store::{self, LockPolicy};
use crate::hook::{InjectionRecord, SCHEMA};
use crate::name::NAME;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// 自打刻 marker の名前。
pub const STAMP_FILE: &str = "tick-stamp";
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

/// pane から読んだ context 使用率。**pane を取得した周だけ評価する**——取得しない周は
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

/// 判定 1 回の全体（判定・cycle の結果・context・状態の読み・cycle-stamp）。
struct Judged {
    /// 判定。
    decision: TickDecision,
    /// cycle を回した周の要約（回していない周は `None`）。
    cycled: Option<String>,
    /// context の評価。
    context: Context,
    /// 席の状態の読み（rules 行が読めず判定に入らない周は `None`・判定行に載らない）。
    state: Option<state::Read>,
    /// cycle-stamp の読み（cycle の評価まで進まなかった周は `None`＝読んでいない。進んだ周は
    /// 評価した・見送った・読めなかったのいずれでも `Some`）。
    stamp: Option<CycleStamp>,
}

impl Judged {
    /// 状態を読む前に決まった周（context も状態も評価していない）。
    fn bare(decision: TickDecision) -> Self {
        Self {
            decision,
            cycled: None,
            context: Context::Unevaluated,
            state: None,
            stamp: None,
        }
    }
}

/// cycle を評価した周の打刻の読み（`s2-07l.110`）。**「無い」と「読めない」を混ぜない**（憲法 C11）:
/// 読めない周を「無い」に読み替えると不可逆の `/clear` へ倒れる（N1 の向きは繰り返さない側）。
#[derive(Clone, Copy)]
enum CycleStamp {
    /// 打刻が無い＝一度も評価していない。
    None,
    /// 打刻からの経過（秒）。
    Age(u64),
    /// 打刻を読めない（置き場が壊れている等）。
    Unreadable,
}

impl CycleStamp {
    /// `<seat_dir>/cycle-stamp`（[`cycle::STAMP_FILE`]・書くのは cycle 側）を読む。mtime が未来の
    /// 周は経過 0＝評価しない側へ倒す。
    fn read(seat_dir: &Path) -> Self {
        match std::fs::metadata(cycle::stamp_path(seat_dir)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::None,
            Err(_) => Self::Unreadable,
            Ok(meta) => match meta.modified() {
                Ok(at) => Self::Age(at.elapsed().map_or(0, |age| age.as_secs())),
                Err(_) => Self::Unreadable,
            },
        }
    }

    /// 判定行に足す字面（cycle の評価まで進んだ周だけ＝評価した・back-off で見送った・読めな
    /// かった、のいずれか）。
    fn suffix(self) -> String {
        match self {
            Self::None => " cycle-stamp=none".to_owned(),
            Self::Age(age) => format!(" cycle-stamp={age}"),
            Self::Unreadable => " cycle-stamp=unreadable".to_owned(),
        }
    }
}

/// pane を取得した後に判定へ渡す材料（pane 本文そのものは持たない＝判定は字面を読まない）。
struct Seen {
    /// 自席の退避物の数え。
    wm: WmScan,
    /// context の評価。
    context: Context,
    /// cycle lock の TTL（秒）。
    ttl_s: u64,
    /// 席の状態の読み（typed・pane の字面ではない）。
    state: state::Read,
    /// stale の閾値（秒・rules 行 `seat.tick_stale_s`）。cycle-stamp の back-off と打刻の合図の brake も
    /// **同じ値**を共用する（新しい閾値を足さない・C5）。
    stale_s: u64,
}

/// 撃たなかった理由。**順序固定の条件のうち最初に立たなかったもの**を表す。
#[derive(Clone, Copy)]
pub enum NoopReason {
    /// 2. pane を読めない。
    PaneMissing,
    /// 3. 席の最終の打刻が Busy（turn が走っている）。
    Busy,
    /// 3. 打刻 file が無い（hook が載っていない席・v1 の席）。**idle と読み替えない**。
    StateMissing,
    /// 3. 打刻 file が読めない・最終行が壊れている。**idle と読み替えない**。
    StateUnreadable,
    /// 3. 最終の Busy が `seat.tick_stale_s` より古い（hook が死んだ疑い）。**busy とも idle とも言わない**。
    StateStale,
    /// 4. 自席の未 consumed 退避物が在る（＝退避して止まっている）。
    WmUnconsumed,
    /// 4. 退避物の dir を読めない（**0 件と読み替えない**）。
    WmUnreadable,
    /// 5. 他の cycle が走っている。
    CycleLive,
    /// 5. 同じ席の cycle を `seat.tick_stale_s` 未満の前に評価した（back-off・`s2-07l.110`）。
    CycleRecent,
    /// 5. cycle-stamp を読めない（**「無い」と読み替えない**＝不可逆の `/clear` へ倒さない）。
    CycleStampUnreadable,
    /// 6. 打刻の合図を `seat.tick_stale_s` 未満の前に注入した（tick-stamp・storm 止め・`s2-07l.109`）。
    PointerRecent,
}

impl NoopReason {
    /// 記録と表示に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PaneMissing => "pane-missing",
            Self::Busy => "busy",
            Self::StateMissing => "state-missing",
            Self::StateUnreadable => "state-unreadable",
            Self::StateStale => "state-stale",
            Self::WmUnconsumed => "wm-unconsumed",
            Self::WmUnreadable => "wm-unreadable",
            Self::CycleLive => "cycle-live",
            Self::CycleRecent => "cycle-recent",
            Self::CycleStampUnreadable => "cycle-stamp-unreadable",
            Self::PointerRecent => "pointer-recent",
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
    let Some(place) = super::state_dir_of(request.state_dir) else {
        // 置き場が無いと記録も打刻も持てない＝判定を回さない（撃たない側へ倒す）。
        return Outcome::failed_line(RC_REFUSED, render(&body_of_error(REASON_STATE_DIR)));
    };
    let dir = super::seat_dir(&place.path, request.target);
    let judged = decide(request, &place, &dir);
    let body = body(request.target, &judged, &place);
    record(&place.path, request.target, &body, started);
    match judged.decision {
        TickDecision::Error(_) => Outcome::failed_line(RC_REFUSED, render(&body)),
        TickDecision::Inject(..) | TickDecision::Noop(_) => Outcome::ok_line(render(&body)),
    }
}

/// 条件を順序固定で見る（退避物の走査 → 状態の読み → pane 取得 → context → 状態の門 → 退避物 → lock →
/// 合図の brake）。鮮度 gate は持たない（`s2-07l.109`・module doc）。
///
/// 退避物を最初に走査するのは、走査の結果を後段（context の合図の可否 / 退避物）で同じ値として
/// 使うためである（cycle を回す周だけは cycle 側が自分の入口でもう 1 度走査する＝lock の内側で
/// 確かめ直す）。読めない周を「在る」に読み替えない。
fn decide(request: &Request, place: &super::StateDir, dir: &Path) -> Judged {
    let (Some(stale_s), Some(ttl_s)) = (state::stale_s(), cycle::ttl_s()) else {
        return Judged::bare(TickDecision::Error(meter::REASON_NO_RULE.to_owned()));
    };
    let wm = super::scan_wm(Path::new(request.wm_dir), request.target);
    // 状態は pane より先に読む（file 1 つ・tmux を叩かない）。読みは tick と cycle で 1 本。
    let read = state::read_last(dir, stale_s);
    let Some(pane) = pane_of(request.socket, request.target, request.capture_file) else {
        return Judged {
            state: Some(read),
            ..Judged::bare(TickDecision::Noop(NoopReason::PaneMissing))
        };
    };
    let seen = Seen {
        wm,
        context: measure_context(&pane),
        ttl_s,
        state: read,
        stale_s,
    };
    let (decision, cycled, stamp) = judge(request, place, dir, &seen);
    Judged {
        decision,
        cycled,
        context: seen.context,
        state: Some(read),
        stamp,
    }
}

/// 判定・cycle の要約・cycle-stamp の読み（後 2 つは cycle の評価まで進んだ周だけ `Some`）。
type Verdict = (TickDecision, Option<String>, Option<CycleStamp>);

/// pane を取得した後の条件（context → 状態の門 → 退避物 → lock → cycle-stamp／合図の brake）。
fn judge(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen) -> Verdict {
    // 他の cycle が走っている席（lock が live）には退避の pointer も送らない——作り直しの最中に
    // 行を queue しても、届く先は消えるか作り直された席である（排他は cycle 側と同じ 1 本の lock）。
    // 退避の合図は**状態の門の外**（FR29「idle を待たずに」・busy な席へは queue の形で届く）。
    if let Some((pct, cap)) = over_cap(seen).filter(|_| !cycle::lock_is_live(dir, seen.ttl_s)) {
        let payload = externalize_pointer(pct, cap);
        return (inject_line(request, place, dir, InjectKind::Externalize, &payload), None, None);
    }
    if let Some(reason) = gate_of(seen.state) {
        return (TickDecision::Noop(reason), None, None);
    }
    match seen.wm {
        WmScan::Unreadable => return (TickDecision::Noop(NoopReason::WmUnreadable), None, None),
        WmScan::Unconsumed(_) => return parked(request, place, dir, seen),
        WmScan::None => {}
    }
    if cycle::lock_is_live(dir, seen.ttl_s) {
        return (TickDecision::Noop(NoopReason::CycleLive), None, None);
    }
    if pointer_recent(dir, seen.stale_s) {
        return (TickDecision::Noop(NoopReason::PointerRecent), None, None);
    }
    let payload = request
        .pointer
        .map_or_else(|| default_pointer(request.target), str::to_owned);
    (inject_line(request, place, dir, InjectKind::Pointer, &payload), None, None)
}

/// 打刻の合図の brake（`s2-07l.109`・planner 裁定 2026-09-12 案 A）: tick 自身の打刻（tick-stamp）の
/// mtime が閾値**未満**なら、この周は合図を送らない。不在・読めない周は送る側（合図は可逆な
/// 1 行で、読めないことを理由に止めると合図が永久に止まる）。mtime が未来の周は送らない側
/// （経過を負に読まない）。境界は未満＝経過が閾値ちょうどの周は送る。
fn pointer_recent(seat_dir: &Path, stale_s: u64) -> bool {
    std::fs::metadata(stamp_path(seat_dir))
        .and_then(|meta| meta.modified())
        .is_ok_and(|at| at.elapsed().map_or(true, |age| age.as_secs() < stale_s))
}

/// 状態の門（ADR-0015 §2.3・fail-closed）: **Idle だけが通る**。Busy・打刻なし・読めない・Busy が
/// 古い、はそれぞれ別の理由で撃たない（missing を idle に、stale を busy に読み替えない）。
fn gate_of(read: state::Read) -> Option<NoopReason> {
    match read {
        state::Read::Idle(_) => None,
        state::Read::Busy(_) => Some(NoopReason::Busy),
        state::Read::Missing => Some(NoopReason::StateMissing),
        state::Read::Unreadable => Some(NoopReason::StateUnreadable),
        state::Read::Stale(_) => Some(NoopReason::StateStale),
    }
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
/// 回すのは「退避物が在る ∧ 打刻が Idle ∧ lock が空いている ∧ **直前に評価していない**」周だけで、
/// **それ以外の周は cycle を評価しない**——tick 行に `cycle=` が付かないこと自体が「評価して
/// いない」の印である。
///
/// **back-off**（`s2-07l.110`・裁定 (a)）: cycle が lock を取れた周（結果が done / failed / refused の
/// いずれでも）は cycle 側が lock の内側・`/clear` より先に `cycle-stamp` を打ち（write-ahead・
/// 打てない周は 1 key も送らず refused。lock を取れない周〔lock-held / state-dir / no-rule〕は打たない
/// ＝他の cycle が打っているか置き場が使えない）、同じ席は打刻から `seat.tick_stale_s` **未満**の間
/// cycle を評価しない（`cycle-recent`・見送った周は打ち直さない＝永久には止まらない）。stamp を
/// 読むのは tick のここだけで、`seat cycle` を手で回す口は back-off を見ない（人の判断）。`/clear` は不可逆の口（N1）で、復元されない退避物（`/rebrief` が走らない・
/// consume しない）へ周期ごとに繰り返してはならない。stamp を読めない周は「無い」に読み替えず
/// 評価しない（`cycle-stamp-unreadable`・読めないことを理由に不可逆の側へ倒さない）。閾値は
/// state-stale と共用し（`seat.tick_stale_s`）、新しい rules 行を足さない（C5）。境界は**未満**（経過が閾値ちょうどの周は評価
/// する）。
fn parked(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen) -> Verdict {
    let noop = TickDecision::Noop(NoopReason::WmUnconsumed);
    if cycle::lock_is_live(dir, seen.ttl_s) {
        return (noop, None, None);
    }
    let stamp = CycleStamp::read(dir);
    match stamp {
        CycleStamp::Unreadable => {
            return (TickDecision::Noop(NoopReason::CycleStampUnreadable), None, Some(stamp));
        }
        CycleStamp::Age(age) if age < seen.stale_s => {
            return (TickDecision::Noop(NoopReason::CycleRecent), None, Some(stamp));
        }
        CycleStamp::Age(_) | CycleStamp::None => {}
    }
    let result = cycle::run(&cycle::Request {
        target: request.target,
        wm_dir: request.wm_dir,
        socket: request.socket,
        capture_file: request.capture_file,
        state_dir: place,
        restore: request.restore,
    });
    (noop, Some(cycle::summary(&result)), Some(stamp))
}

/// 1 行を注入し、成立したら自打刻する（退避の合図も打刻の合図も同じ経路。自打刻は打刻の合図の
/// brake〔[`pointer_recent`]〕にだけ効き、退避の合図と cycle は次の周も評価する）。busy な席へは
/// queue の形で届く（`.90`）。
fn inject_line(
    request: &Request,
    place: &super::StateDir,
    dir: &Path,
    kind: InjectKind,
    payload: &str,
) -> TickDecision {
    let sent = inject::deliver(&inject::Request {
        target: request.target,
        socket: request.socket,
        payload,
        state_dir: Some(place),
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
/// （評価した順）。席の状態（`state=<値> event=<出所|none>`）は既存 token の**後ろに追加**する
/// （名前・順序・書式は不変・`s2-07l.95`・`event` は C10 の出所＝置き場の `source=` と混ぜない）。
/// cycle-stamp（`s2-07l.110`）は**その後ろ**（先に land した側の token が前・後から land した側が
/// その後ろ・planner 裁定 2026-09-12）で、cycle の評価まで進んだ周だけ載る。
/// **置き場と出所は最後**（置き場が解けた周は判定に依らず載せる＝席側の打刻行と並べるだけで、
/// 別の dir を見ていることを記録から弁別できる・`s2-07l.70`）。
fn body(target: &str, judged: &Judged, place: &super::StateDir) -> String {
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
    let with_state = judged.state.map_or(String::new(), state::Read::suffix);
    let with_stamp = judged.stamp.map_or(String::new(), CycleStamp::suffix);
    format!("{with_cycle}{with_state}{with_stamp}{}", place.suffix())
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
