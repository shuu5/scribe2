//! 管理 tick `seat tick`（設計 docs/design/seat-heartbeat.md §2・契約表の行 a・ADR-0058 §2）。
//!
//! 黙った席（登録 row を持ち・状態の打刻の最終行が Idle で `seat.tick_stale_s` 以上前・入力欄が空）にだけ、器が 1 行の
//! 合図を差し入れる。判定は順序固定の AND（[`judge`]・最初に立たなかった条件を理由にする）で、閉じた [`TickDecision`] の
//! 3 値に畳む（bool で持たない・C11）。
//!
//! 無変化の席には合図の間隔を段ごとに伸ばす（梯子・[`Ladder`]）: 変化の digest は打刻の最終行の `ts` の 1 値、段 n の待ちは
//! `seat.tick_stale_s` × `seat.pointer_backoff_factor` ^ n（[`Pace::wait_of`]）、待ちが `seat.pointer_backoff_max_s` を超える段は
//! 送らない。記録は席の置き場の 1 file（[`LADDER_FILE`]・書き手はここだけ・一時 file → rename）で、注入の記録
//! （`tick.jsonl` の `InjectionRecord`）とは別に持つ。
//!
//! 注入は既存の 1 入口（[`deliver_within`]）を 1 回撃つだけで、`tick.jsonl` の 1 行もその経路が書く。口座は計測済みの記録
//! （[`fresh_rows`]）を読むだけで**測らない**（FR38・子 process を起こさない）。env・home・自分の実行 file の場所は読まない
//! （C2.2）。

use super::inject::{deliver_within, last_own_payload, pass_input, Blocked, Delivery, Request, Settled};
use super::state::{self, Event, SeatState, Stamp};
use super::{pane_of, sanitize_target, seat_dir, state_dir_of, StateDir};
use crate::cli_outcome::{Outcome, RC_OK, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::usage::fresh_rows;
use crate::fleet::State;
use crate::hook::group::{pressed, Caps};
use crate::name::NAME;
use crate::rules::manifest::Manifest;
use crate::rules::{int_row, RuleError};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// timer の周期の rules 行（tick の判定は読むだけ・unit を書く口が使う）。
pub const ROW_INTERVAL: &str = "seat.tick_interval_s";
/// 初段の待ち・黙りの閾値・Busy の古さの 3 役の rules 行（秒）。
pub const ROW_STALE: &str = "seat.tick_stale_s";
/// 梯子の係数の rules 行。
pub const ROW_FACTOR: &str = "seat.pointer_backoff_factor";
/// 梯子の上限の rules 行（秒）。
pub const ROW_MAX: &str = "seat.pointer_backoff_max_s";
/// 送達の窓の rules 行（dispatcher の通知と同じ行・行を増やさない）。
const ROW_WINDOW: &str = "pipe.stop_grace_ms";

/// 梯子の記録の file 名（席の置き場の直下）。
pub const LADDER_FILE: &str = "pointer-ladder";
/// 梯子の記録の schema 版。
const LADDER_SCHEMA: u64 = 1;
/// 評価していない欄の字面（0 に化けない・C10）。
const DASH: &str = "-";

/// 注入しない理由（**閉じた列**・宣言順は判定の列の順・字面は判定行の `reason=` の語）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoopReason {
    /// 登録 row が無い（器の管理外・FR40）。
    NoRow,
    /// 打刻 file が無い（hook が載っていない席）。
    StateMissing,
    /// 打刻 file を読めない・読める行が 1 つも無い。
    StateUnreadable,
    /// 最終行が Busy（turn の途中）。
    Busy,
    /// 最終行の Busy が `seat.tick_stale_s` 以上前（Stop の打刻を失った席＝人が見る）。
    StateStale,
    /// 送った合図の基準（digest）が未確定。
    Settling,
    /// 梯子の記録が在るのに読めない（0 件に潰さない・fail-closed）。
    RecordUnreadable,
    /// 最終行が `seat.tick_stale_s` 未満前（席は最近まで動いていた）。
    StampRecent,
    /// 段の候補の待ちが上限を超える（打ち切り）。
    Stopped,
    /// 記録の `sent_at` から段の候補の待ちが経っていない（床）。
    Wait,
    /// 自席の口座の鮮度の内側の記録が閾値以上。
    AccountPressed,
    /// pane を取れない。
    PaneMissing,
    /// 入力欄に人の文字が在る。
    InputBusy,
    /// prompt 行を特定できない。
    InputUnknown,
    /// 自席の前の合図が Enter 1 回の後も残る。
    InputOwnQueued,
    /// 梯子の記録を書けない（1 key も送らない・fail-closed）。
    RecordUnwritable,
}

/// [`NoopReason`] の全部（宣言順・歯の母集団）。
pub const NOOP_REASONS: &[NoopReason] = &[
    NoopReason::NoRow,
    NoopReason::StateMissing,
    NoopReason::StateUnreadable,
    NoopReason::Busy,
    NoopReason::StateStale,
    NoopReason::Settling,
    NoopReason::RecordUnreadable,
    NoopReason::StampRecent,
    NoopReason::Stopped,
    NoopReason::Wait,
    NoopReason::AccountPressed,
    NoopReason::PaneMissing,
    NoopReason::InputBusy,
    NoopReason::InputUnknown,
    NoopReason::InputOwnQueued,
    NoopReason::RecordUnwritable,
];

impl NoopReason {
    /// 判定行の `reason=` の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoRow => "no-row",
            Self::StateMissing => "state-missing",
            Self::StateUnreadable => "state-unreadable",
            Self::Busy => "busy",
            Self::StateStale => "state-stale",
            Self::Settling => "settling",
            Self::RecordUnreadable => "record-unreadable",
            Self::StampRecent => "stamp-recent",
            Self::Stopped => "stopped",
            Self::Wait => "wait",
            Self::AccountPressed => "account-pressed",
            Self::PaneMissing => "pane-missing",
            Self::InputBusy => "input-busy",
            Self::InputUnknown => "input-unknown",
            Self::InputOwnQueued => "input-own-queued",
            Self::RecordUnwritable => "record-unwritable",
        }
    }
}

/// 実行系が回らない理由（**閉じた列**・noop の語彙を汚さない＝席が静かなのか器が壊れているのかを記録から読める）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickError {
    /// 置き場を解けない。
    StateDir,
    /// 行 4 本・`pipe.stop_grace_ms`・群の閾値の行のどれかが読めない（不在・不発効・整数でない・manifest が壊れている）。
    NoRule,
    /// fleet の replay が読めない。
    Store,
}

/// [`TickError`] の全部（宣言順）。
pub const TICK_ERRORS: &[TickError] = &[TickError::StateDir, TickError::NoRule, TickError::Store];

impl TickError {
    /// 判定行の `reason=` の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StateDir => "state-dir",
            Self::NoRule => "no-rule",
            Self::Store => "store",
        }
    }
}

/// 1 周の判定（**閉じた 3 値**・C11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickDecision {
    /// 合図を送った（送達の結果は [`Verdict::sent`]）。
    Inject,
    /// 送らない（理由つき）。
    Noop(NoopReason),
    /// 実行系が回らない（rc 1）。
    Error(TickError),
}

impl TickDecision {
    /// 判定行の `decision=` の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inject => "inject",
            Self::Noop(_) => "noop",
            Self::Error(_) => "error",
        }
    }

    /// 判定行の `reason=` の語（注入した周は `-`）。
    fn reason(self) -> &'static str {
        match self {
            Self::Inject => DASH,
            Self::Noop(reason) => reason.as_str(),
            Self::Error(error) => error.as_str(),
        }
    }
}

/// 梯子の評価（判定行の `pointer=` の材料）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    /// 送った合図の基準が未確定。
    Settling,
    /// 床を過ぎていて上限の内（送った周は `sent`・送らなかった周は残り 0 秒の `wait:0`）。
    Open,
    /// 床の内（残り秒）。
    Wait(u64),
    /// 段の候補の待ちが上限を超える。
    Stopped,
}

impl Pointer {
    /// 判定行の字面。`sent` は送った周だけ。
    fn render(self, sent: bool) -> String {
        match self {
            Self::Settling => "settling".to_owned(),
            Self::Open if sent => "sent".to_owned(),
            Self::Open => "wait:0".to_owned(),
            Self::Wait(left) => format!("wait:{left}"),
            Self::Stopped => "stopped".to_owned(),
        }
    }
}

/// 行 3 本の値（梯子の形）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pace {
    /// `seat.tick_stale_s`。
    pub stale_s: u64,
    /// `seat.pointer_backoff_factor`。
    pub factor: u64,
    /// `seat.pointer_backoff_max_s`。
    pub max_s: u64,
}

impl Pace {
    /// 段 `step` の待ち（秒・`stale_s` × `factor` ^ `step`・飽和演算）。
    pub fn wait_of(self, step: u32) -> u64 {
        self.stale_s.saturating_mul(self.factor.saturating_pow(step))
    }
}

/// 梯子の記録（1 行 JSON・`schema` / `sent_at` / `step` / `digest`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ladder {
    /// 送った UTC 秒。
    pub sent_at: u64,
    /// 送った段（0 始まり）。
    pub step: u32,
    /// 基準の ts（未確定は `None`＝`null`）。
    pub digest: Option<u64>,
}

impl Ladder {
    /// 1 行の flat JSON にする。
    pub fn to_line(&self) -> String {
        json_lite::write_object(&[
            ("schema", Value::Num(LADDER_SCHEMA)),
            ("sent_at", Value::Num(self.sent_at)),
            ("step", Value::Num(u64::from(self.step))),
            ("digest", self.digest.map_or(Value::Null, Value::Num)),
        ])
    }

    /// file の本文を読む（ちょうど 1 行・key の欠け・型違い・schema 違い・段が u32 に収まらない形は `None`）。
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let pairs = json_lite::parse_object(lines.next()?).ok()?;
        if lines.next().is_some() {
            return None;
        }
        let value = |key: &str| pairs.iter().find(|(found, _)| found == key).map(|(_, value)| value);
        if value("schema")?.as_num()? != LADDER_SCHEMA {
            return None;
        }
        let digest = match value("digest")? {
            Value::Null => None,
            other => Some(other.as_num()?),
        };
        Some(Self { sent_at: value("sent_at")?.as_num()?, step: u32::try_from(value("step")?.as_num()?).ok()?, digest })
    }
}

/// 段の候補（pure）: 記録が無い・基準と今の digest が違う（変化）→ 0、同じ → 記録の段 + 1。基準の無い記録は 0（呼び手は
/// settle の後にだけ撃つ）。
pub fn candidate(record: Option<&Ladder>, digest: u64) -> u32 {
    match record.map(|found| (found.step, found.digest)) {
        Some((step, Some(base))) if base == digest => step.saturating_add(1),
        _ => 0,
    }
}

/// 基準の確定（pure）: `sent_at` より後の Stop の打刻の最後の ts（合図に応えた turn の終わり）、無ければ `sent_at` から
/// `stale_s` を過ぎた周の今の digest（応えない席＝梯子が登る側）、どちらでもなければ `None`（settling）。
pub fn settle(record: &Ladder, stamps: &[Stamp], digest: u64, now: u64, stale_s: u64) -> Option<u64> {
    let answered = stamps.iter().rev().find(|stamp| stamp.event == Event::Stop && stamp.ts > record.sent_at).map(|stamp| stamp.ts);
    answered.or_else(|| (now.saturating_sub(record.sent_at) >= stale_s).then_some(digest))
}

/// 段の候補の上限と床（pure）: 待ちが上限を超えれば [`Pointer::Stopped`]、記録の `sent_at` から待ちが経っていなければ
/// [`Pointer::Wait`]（残り秒）、それ以外（記録なしを含む）は [`Pointer::Open`]。
pub fn pointer_of(pace: Pace, sent_at: Option<u64>, step: u32, now: u64) -> Pointer {
    let wait = pace.wait_of(step);
    if wait > pace.max_s {
        return Pointer::Stopped;
    }
    match sent_at.map(|at| now.saturating_sub(at)) {
        Some(elapsed) if elapsed < wait => Pointer::Wait(wait.saturating_sub(elapsed)),
        _ => Pointer::Open,
    }
}

/// 打刻の合図の文面（**正本はこの 1 関数**・先頭の `<NAME> tick:` が器自身の目印・規則は持たない）。
pub fn signal(step: u32, pace: Pace) -> String {
    format!(
        "{NAME} tick: heartbeat step={step} — 台帳の現在地（bd --readonly ready --limit 0）から続きを進める（変化が無ければ次の合図は {} 秒後・上限で打ち切り）",
        pace.wait_of(step.saturating_add(1))
    )
}

/// 送達の結果（判定行の `consumed=` の材料・落ちても送ったと数える）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sent {
    /// 送達を確認した（消費 / queue / 測れない）。
    Settled(Settled),
    /// 送る直前の門で 1 key も送らなかった（理由）。
    Refused(&'static str),
    /// 送ったが送達を確認できない（理由）。
    Unconfirmed(&'static str),
}

impl Sent {
    /// 注入の結果から写す。
    fn of(delivery: Delivery) -> Self {
        match delivery {
            Delivery::Delivered(_, settled) => Self::Settled(settled),
            Delivery::Refused(why) => Self::Refused(why),
            Delivery::Unconfirmed(why) => Self::Unconfirmed(why),
        }
    }

    /// `consumed=` の字面（`true` / `false` / `unknown[:理由]`・届かなかった周は `unknown:<理由>`）。
    fn render(self) -> String {
        match self {
            Self::Settled(settled) => match settled.reason() {
                Some(why) => format!("{}:{why}", settled.as_str()),
                None => settled.as_str().to_owned(),
            },
            Self::Refused(why) | Self::Unconfirmed(why) => format!("unknown:{why}"),
        }
    }
}

/// 1 周の結果（判定行の材料）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    /// 判定。
    pub decision: TickDecision,
    /// 梯子を評価した周だけ（評価・段）。
    pub ladder: Option<(Pointer, u32)>,
    /// 注入した周だけ（送達の結果）。
    pub sent: Option<Sent>,
}

impl Verdict {
    /// 実行系が回らない周。
    fn error(error: TickError) -> Self {
        Self { decision: TickDecision::Error(error), ladder: None, sent: None }
    }

    /// 梯子の手前で止まった周。
    fn noop(reason: NoopReason) -> Self {
        Self { decision: TickDecision::Noop(reason), ladder: None, sent: None }
    }

    /// 梯子を評価した後で止まった周。
    fn noop_at(reason: NoopReason, pointer: Pointer, step: u32) -> Self {
        Self { decision: TickDecision::Noop(reason), ladder: Some((pointer, step)), sent: None }
    }
}

/// 判定行（stdout の 1 行）。`pointer=` / `step=` は梯子を評価した周だけ・`consumed=` は注入した周だけ（他は `-`）。
pub fn render(target: &str, verdict: &Verdict) -> String {
    let sent = verdict.sent.is_some();
    let (pointer, step) =
        verdict.ladder.map_or_else(|| (DASH.to_owned(), DASH.to_owned()), |(pointer, step)| (pointer.render(sent), step.to_string()));
    let consumed = verdict.sent.map_or_else(|| DASH.to_owned(), Sent::render);
    format!(
        "decision={} target={} reason={} pointer={pointer} step={step} consumed={consumed}",
        verdict.decision.as_str(),
        sanitize_target(target),
        verdict.decision.reason()
    )
}

/// 口が解いた引数（置き場・target・pane の出所）。
pub struct Flags<'a> {
    /// `--state-dir`。
    pub state_dir: &'a str,
    /// `--target S:W`。
    pub target: &'a str,
    /// `--tmux-socket`。
    pub socket: Option<&'a str>,
    /// `--capture-file`（pane の写し・tmux を撃たない読み）。
    pub capture: Option<&'a str>,
}

/// `seat tick` の本体: 判定行 1 行を stdout へ・rc は inject / noop が 0・error が 1。manifest が壊れている周は defect を
/// stderr へ並べる（`rules validate` と同じ字面）。
pub fn run(flags: &Flags, manifest: Result<Manifest, Vec<RuleError>>) -> Outcome {
    let (verdict, err) = match (state_dir_of(Some(flags.state_dir)), manifest) {
        (None, _) => (Verdict::error(TickError::StateDir), Vec::new()),
        (Some(_), Err(errors)) => (Verdict::error(TickError::NoRule), crate::rules::cli::render_defects(&errors)),
        (Some(state), Ok(manifest)) => {
            let input = Input { state: &state, target: flags.target, socket: flags.socket, capture: flags.capture, manifest: &manifest };
            (judge(&input), Vec::new())
        }
    };
    let rc = if matches!(verdict.decision, TickDecision::Error(_)) { RC_REFUSED } else { RC_OK };
    Outcome { out: vec![render(flags.target, &verdict)], err, rc }
}

/// 判定の入力。
pub struct Input<'a> {
    /// 解決済みの置き場。
    pub state: &'a StateDir,
    /// target。
    pub target: &'a str,
    /// tmux の socket。
    pub socket: Option<&'a str>,
    /// pane の写し。
    pub capture: Option<&'a str>,
    /// rules（`--rules` か埋め込み）。
    pub manifest: &'a Manifest,
}

/// 判定の列を 1 周撃つ（順序固定の AND・最初に立たなかった条件を理由にする）。
pub fn judge(input: &Input) -> Verdict {
    front(input).and_then(|found| back(input, &found)).unwrap_or_else(|stopped| stopped)
}

/// rules の行（行 4 本・送達の窓・群の閾値）。
struct Rows {
    /// 梯子の形。
    pace: Pace,
    /// 送達の窓（ms）。
    window_ms: u64,
    /// 口座の門の閾値。
    caps: Caps,
}

impl Rows {
    /// 全部を読む。どれかが読めない周は `Err`（既定値に倒さない・C1）。
    fn of(manifest: &Manifest) -> Result<Self, String> {
        int_row(manifest, ROW_INTERVAL)?;
        let pace = Pace { stale_s: int_row(manifest, ROW_STALE)?, factor: int_row(manifest, ROW_FACTOR)?, max_s: int_row(manifest, ROW_MAX)? };
        Ok(Self { pace, window_ms: int_row(manifest, ROW_WINDOW)?, caps: Caps::of(manifest)? })
    }
}

/// 梯子を評価し終えた周の材料（形 1 の 3 まで）。
struct Front {
    /// rules の行。
    rows: Rows,
    /// fleet の replay。
    fleet: State,
    /// 登録 row の口座 label。
    account: String,
    /// 席の置き場。
    seat: PathBuf,
    /// 今の digest（最終行の ts）。
    digest: u64,
    /// 段の候補。
    step: u32,
    /// 梯子の評価。
    pointer: Pointer,
    /// 判定の時刻（UTC 秒）。
    now: u64,
}

/// 形 1 の 1〜3（登録 row → 状態の打刻 → digest の比較）。
fn front(input: &Input) -> Result<Front, Verdict> {
    let rows = Rows::of(input.manifest).map_err(|_| Verdict::error(TickError::NoRule))?;
    let now = state::now_secs();
    let events = crate::fleet::store::read_all(&input.state.path).map_err(|_| Verdict::error(TickError::Store))?;
    let fleet = crate::fleet::replay(&events);
    let account = super::role::registration_of_target(&fleet, input.target)
        .map(|row| row.account.clone())
        .ok_or_else(|| Verdict::noop(NoopReason::NoRow))?;
    let seat = seat_dir(&input.state.path, input.target);
    let stamps = stamps_of(&seat, rows.pace.stale_s, now).map_err(Verdict::noop)?;
    let digest = stamps.last().map_or(0, |stamp| stamp.ts);
    let record = read_ladder(&seat).map_err(Verdict::noop)?;
    let record = match record {
        Some(found) => Some(settled(&seat, found, &stamps, digest, (now, rows.pace.stale_s))?),
        None => None,
    };
    let step = candidate(record.as_ref(), digest);
    let pointer = pointer_of(rows.pace, record.map(|found| found.sent_at), step, now);
    Ok(Front { rows, fleet, account, seat, digest, step, pointer, now })
}

/// 形 1 の 4〜10（黙りの門 → 上限 → 床 → 口座の門 → 入力欄の門 → 記録 → 注入）。
fn back(input: &Input, front: &Front) -> Result<Verdict, Verdict> {
    let at = |reason| Verdict::noop_at(reason, front.pointer, front.step);
    if !aged(front.digest, front.now, front.rows.pace.stale_s) {
        return Err(at(NoopReason::StampRecent));
    }
    match front.pointer {
        Pointer::Stopped => return Err(at(NoopReason::Stopped)),
        Pointer::Wait(_) => return Err(at(NoopReason::Wait)),
        Pointer::Open | Pointer::Settling => {}
    }
    if account_pressed(input.manifest, front) {
        return Err(at(NoopReason::AccountPressed));
    }
    input_gate(input).map_err(at)?;
    let record = Ladder { sent_at: front.now, step: front.step, digest: None };
    write_ladder(&front.seat, &record).map_err(|_| at(NoopReason::RecordUnwritable))?;
    let payload = signal(front.step, front.rows.pace);
    let request = Request { target: input.target, socket: input.socket, payload: &payload, state_dir: Some(input.state) };
    let delivery = deliver_within(&request, Duration::from_millis(front.rows.window_ms));
    Ok(Verdict { decision: TickDecision::Inject, ladder: Some((front.pointer, front.step)), sent: Some(Sent::of(delivery)) })
}

/// `ts` から `stale_s` 以上経ったか（黙りの門と Busy の古さの同じ 1 本・時計は打刻と同じ UTC 秒）。
fn aged(ts: u64, now: u64, stale_s: u64) -> bool {
    now.saturating_sub(ts) >= stale_s
}

/// 状態の打刻を読み、最終行（読めた行のうち最後）が Idle の周だけ全行を返す。
fn stamps_of(seat: &Path, stale_s: u64, now: u64) -> Result<Vec<Stamp>, NoopReason> {
    let text = match fs::read_to_string(state::path(seat)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(NoopReason::StateMissing),
        Err(_) => return Err(NoopReason::StateUnreadable),
    };
    let stamps: Vec<Stamp> = text.lines().filter_map(|line| Stamp::from_line(line).ok()).collect();
    let last = stamps.last().ok_or(NoopReason::StateUnreadable)?;
    match last.state {
        SeatState::Idle => Ok(stamps),
        SeatState::Busy if aged(last.ts, now, stale_s) => Err(NoopReason::StateStale),
        SeatState::Busy => Err(NoopReason::Busy),
    }
}

/// 梯子の記録の path。
pub fn ladder_path(seat: &Path) -> PathBuf {
    seat.join(LADDER_FILE)
}

/// 梯子の記録を読む（無い周は `None`・在るのに読めない周は [`NoopReason::RecordUnreadable`]）。
fn read_ladder(seat: &Path) -> Result<Option<Ladder>, NoopReason> {
    match fs::read_to_string(ladder_path(seat)) {
        Ok(text) => Ladder::parse(&text).map(Some).ok_or(NoopReason::RecordUnreadable),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(NoopReason::RecordUnreadable),
    }
}

/// 梯子の記録を書く（一時 file → rename・席の置き場は作らない＝打刻の在る dir にだけ書く）。
fn write_ladder(seat: &Path, record: &Ladder) -> std::io::Result<()> {
    let path = ladder_path(seat);
    let temporary = seat.join(format!("{LADDER_FILE}.tmp"));
    fs::write(&temporary, format!("{}\n", record.to_line()))?;
    fs::rename(&temporary, &path)
}

/// 基準の無い記録に settle を試み、確定した基準を書いた記録を返す（基準の在る記録はそのまま）。確定できない周は
/// `settling`・書けない周は `record-unwritable`（`clock` は判定の時刻と `seat.tick_stale_s`）。
fn settled(seat: &Path, mut record: Ladder, stamps: &[Stamp], digest: u64, clock: (u64, u64)) -> Result<Ladder, Verdict> {
    if record.digest.is_some() {
        return Ok(record);
    }
    let (now, stale_s) = clock;
    let Some(base) = settle(&record, stamps, digest, now, stale_s) else {
        return Err(Verdict::noop_at(NoopReason::Settling, Pointer::Settling, record.step));
    };
    record.digest = Some(base);
    write_ladder(seat, &record).map_err(|_| Verdict::noop_at(NoopReason::RecordUnwritable, Pointer::Settling, record.step))?;
    Ok(record)
}

/// 口座の門（形 4）: 登録 row の口座の鮮度の内側の記録（[`fresh_rows`]・計測は起こさない）が閾値以上なら真。記録が無い・
/// 鮮度の外・読めない周は偽（門は正の証拠でだけ閉じる）。
fn account_pressed(manifest: &Manifest, front: &Front) -> bool {
    matches!(fresh_rows(manifest, &front.fleet, &front.account), Ok(Some(rows)) if pressed(&rows, front.rows.caps).is_some())
}

/// pane と入力欄の門（形 1 の 8）: pane を取れない周は `pane-missing`・[`pass_input`]（自席の文は [`last_own_payload`]）を
/// 通し、人の文字は `input-busy`・prompt 行を特定できない周は `input-unknown`・Enter 1 回の後も残る自席の文は
/// `input-own-queued`。
fn input_gate(input: &Input) -> Result<(), NoopReason> {
    let pane = pane_of(input.socket, input.target, input.capture).ok_or(NoopReason::PaneMissing)?;
    let own = last_own_payload(&input.state.path, input.target);
    let recapture = || pane_of(input.socket, input.target, input.capture);
    pass_input(input.socket, input.target, &pane, own.as_deref(), recapture).map_err(|blocked| match blocked {
        Blocked::Foreign => NoopReason::InputBusy,
        Blocked::UnknownInput => NoopReason::InputUnknown,
        Blocked::OwnQueued => NoopReason::InputOwnQueued,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        candidate, pointer_of, render, settle, signal, Ladder, NoopReason, Pace, Pointer, Sent, TickDecision, TickError, Verdict,
        NOOP_REASONS, TICK_ERRORS,
    };
    use crate::name::NAME;
    use crate::order::is_declaration_order;
    use crate::seat::inject::{Settled, Unmeasured};
    use crate::seat::state::{Event, Stamp, SCHEMA};

    /// 初期値の梯子（rules 行の値を写さない fixture・値の対応は e2e が manifest で測る）。
    const PACE: Pace = Pace { stale_s: 2400, factor: 2, max_s: 86_400 };

    /// 打刻 1 行。
    fn stamp(event: Event, ts: u64) -> Stamp {
        Stamp { schema: SCHEMA, state: event.state(), event, ts, sid: String::new() }
    }

    /// 段 → 待ちは stale × factor ^ 段で倍々に伸び、段 5 までが上限の内・段 6 から打ち切り。巨大な値は飽和する。
    #[test]
    fn seat_tick_wait_doubles_per_step_and_saturates() {
        let waits: Vec<u64> = (0..=6).map(|step| PACE.wait_of(step)).collect();
        assert_eq!(waits, [2400, 4800, 9600, 19_200, 38_400, 76_800, 153_600]);
        assert_eq!(pointer_of(PACE, None, 5, 0), Pointer::Open, "段 5 は上限の内（記録なしは床を通る）");
        assert_eq!(pointer_of(PACE, None, 6, 0), Pointer::Stopped, "段 6 は上限を超える");
        let huge = Pace { stale_s: u64::MAX / 2, factor: 3, max_s: u64::MAX };
        assert_eq!(huge.wait_of(1), u64::MAX, "積の飽和");
        assert_eq!(Pace { stale_s: 1, factor: 2, max_s: 0 }.wait_of(200), u64::MAX, "冪の飽和");
        assert_eq!(pointer_of(Pace { stale_s: 10, factor: 2, max_s: 10 }, None, 0, 0), Pointer::Open, "上限と等しい待ちは送る側");
    }

    /// 床: `sent_at` から待ちが経っていない周は残り秒の Wait・ちょうど経った周は Open。
    #[test]
    fn seat_tick_floor_counts_the_seconds_left_from_sent_at() {
        assert_eq!(pointer_of(PACE, Some(1000), 1, 1000 + 3000), Pointer::Wait(1800));
        assert_eq!(pointer_of(PACE, Some(1000), 1, 1000 + 4800), Pointer::Open, "境界は経った側");
        assert_eq!(pointer_of(PACE, Some(1000), 0, 1000 + 2399), Pointer::Wait(1));
        assert_eq!(pointer_of(PACE, Some(5000), 0, 10), Pointer::Wait(2400), "未来の sent_at は経過 0");
    }

    /// 段の候補: 記録なし → 0・基準と同じ digest → 段 + 1・変化 → 0・基準の無い記録 → 0。
    #[test]
    fn seat_tick_candidate_climbs_on_same_digest_and_resets_on_change() {
        let record = Ladder { sent_at: 10, step: 3, digest: Some(77) };
        assert_eq!(candidate(None, 77), 0, "記録なし");
        assert_eq!(candidate(Some(&record), 77), 4, "無変化");
        assert_eq!(candidate(Some(&record), 78), 0, "変化");
        assert_eq!(candidate(Some(&Ladder { digest: None, ..record }), 77), 0, "基準なし");
        assert_eq!(candidate(Some(&Ladder { step: u32::MAX, ..record }), 77), u32::MAX, "段は飽和");
    }

    /// settle: `sent_at` より後の Stop の最後の ts・無ければ stale を過ぎた周の今の digest・どちらでもなければ None。
    #[test]
    fn seat_tick_settle_takes_the_answer_or_the_stale_digest() {
        let record = Ladder { sent_at: 100, step: 0, digest: None };
        let answered = [stamp(Event::Stop, 90), stamp(Event::UserPromptSubmit, 101), stamp(Event::Stop, 102), stamp(Event::Stop, 105)];
        assert_eq!(settle(&record, &answered, 105, 110, 2400), Some(105), "後の Stop の最後");
        let same_second = [stamp(Event::Stop, 100), stamp(Event::SessionStart, 104)];
        assert_eq!(settle(&record, &same_second, 104, 110, 2400), None, "sent_at と同じ秒の Stop と SessionStart は応答でない");
        assert_eq!(settle(&record, &same_second, 104, 100 + 2400, 2400), Some(104), "応えない席は stale を過ぎた周の digest");
        assert_eq!(settle(&record, &same_second, 104, 100 + 2399, 2400), None, "stale の手前は settling");
    }

    /// 記録の 1 行は round-trip し、1 行でない・schema 違い・key の欠け・型違い・段の桁あふれは読まない。
    #[test]
    fn seat_tick_ladder_record_round_trips_and_rejects_malformed_lines() {
        for record in [Ladder { sent_at: 1_757_600_000, step: 5, digest: Some(1_757_599_000) }, Ladder { sent_at: 1, step: 0, digest: None }] {
            let line = record.to_line();
            assert_eq!(Ladder::parse(&line), Some(record), "{line}");
            assert_eq!(Ladder::parse(&format!("{line}\n")), Some(record), "末尾の改行");
        }
        assert_eq!(
            Ladder { sent_at: 7, step: 1, digest: None }.to_line(),
            r#"{"schema":1,"sent_at":7,"step":1,"digest":null}"#,
            "外形"
        );
        for bad in [
            "",
            "{\"schema\":1,\"sent_at\":7,\"step\":1,\"digest\":null}\n{\"schema\":1,\"sent_at\":7,\"step\":1,\"digest\":null}",
            r#"{"schema":2,"sent_at":7,"step":1,"digest":null}"#,
            r#"{"schema":1,"step":1,"digest":null}"#,
            r#"{"schema":1,"sent_at":7,"step":1}"#,
            r#"{"schema":1,"sent_at":"7","step":1,"digest":null}"#,
            r#"{"schema":1,"sent_at":7,"step":4294967296,"digest":null}"#,
            r#"{"schema":1,"sent_at":7,"step":1,"digest":"x"}"#,
        ] {
            assert_eq!(Ladder::parse(bad), None, "{bad}");
        }
    }

    /// 理由の語は宣言順の列で重複せず、網羅 match の字面と一致する（error の語も同じ）。
    #[test]
    fn seat_tick_reasons_are_unique_in_declaration_order() {
        assert!(is_declaration_order(NOOP_REASONS, |reason| reason as usize), "NOOP_REASONS は宣言順");
        assert_eq!(NOOP_REASONS.len(), 16, "母集団");
        let words: Vec<&str> = NOOP_REASONS.iter().map(|reason| reason.as_str()).collect();
        assert_eq!(
            words,
            [
                "no-row", "state-missing", "state-unreadable", "busy", "state-stale", "settling", "record-unreadable",
                "stamp-recent", "stopped", "wait", "account-pressed", "pane-missing", "input-busy", "input-unknown",
                "input-own-queued", "record-unwritable",
            ]
        );
        assert!(is_declaration_order(TICK_ERRORS, |error| error as usize), "TICK_ERRORS は宣言順");
        let errors: Vec<&str> = TICK_ERRORS.iter().map(|error| error.as_str()).collect();
        assert_eq!(errors, ["state-dir", "no-rule", "store"]);
        let mut all: Vec<&str> = words.iter().chain(errors.iter()).copied().collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 19, "noop と error の語は重ならない");
    }

    /// 判定行: 梯子の手前は `pointer=- step=-`・梯子の後は評価と段・注入した周だけ `consumed=`（届かない周は unknown:理由）。
    #[test]
    fn seat_tick_render_dashes_before_the_ladder() {
        assert_eq!(
            render("s:w", &Verdict::noop(NoopReason::NoRow)),
            "decision=noop target=s_w reason=no-row pointer=- step=- consumed=-"
        );
        assert_eq!(
            render("s:w", &Verdict::error(TickError::NoRule)),
            "decision=error target=s_w reason=no-rule pointer=- step=- consumed=-"
        );
        assert_eq!(
            render("s:w", &Verdict::noop_at(NoopReason::Wait, Pointer::Wait(30), 2)),
            "decision=noop target=s_w reason=wait pointer=wait:30 step=2 consumed=-"
        );
        assert_eq!(
            render("s:w", &Verdict::noop_at(NoopReason::StampRecent, Pointer::Open, 0)),
            "decision=noop target=s_w reason=stamp-recent pointer=wait:0 step=0 consumed=-"
        );
        let sent = |found| Verdict { decision: TickDecision::Inject, ladder: Some((Pointer::Open, 1)), sent: Some(found) };
        assert_eq!(
            render("s:w", &sent(Sent::Settled(Settled::Queued))),
            "decision=inject target=s_w reason=- pointer=sent step=1 consumed=false"
        );
        assert!(render("s:w", &sent(Sent::Settled(Settled::Unmeasured(Unmeasured::StateMissing)))).ends_with(" consumed=unknown:state-missing"));
        assert!(render("s:w", &sent(Sent::Unconfirmed("absent"))).ends_with(" consumed=unknown:absent"));
    }

    /// 合図の文面は器の目印で始まり、段と次の段の待ちを名乗る。
    #[test]
    fn seat_tick_signal_names_the_step_and_the_next_wait() {
        let line = signal(0, PACE);
        assert!(line.starts_with(&format!("{NAME} tick: heartbeat step=0 — ")), "{line}");
        assert!(line.contains("（変化が無ければ次の合図は 4800 秒後・上限で打ち切り）"), "{line}");
        assert!(!line.contains('\n'), "1 行");
        assert!(signal(5, PACE).contains("次の合図は 153600 秒後"), "段 5 の次は上限の外でも数を名乗る");
    }
}
