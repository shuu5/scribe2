//! 管理 tick `seat tick`（設計 docs/design/seat-heartbeat.md §2・契約表の行 a・ADR-0058 §2）。
//!
//! 黙った席（登録 row を持ち・状態の打刻の最終行が Idle で `seat.tick_stale_s` 以上前・入力欄が空）にだけ、器が 1 行の
//! 合図を差し入れる。判定は順序固定の AND（[`judge`]・最初に立たなかった条件を理由にする）で、閉じた [`TickDecision`] の
//! 3 値に畳む（bool で持たない・C11）。
//!
//! 無変化の席には合図の間隔を段ごとに伸ばす（梯子・[`Ladder`]）: 変化の digest は打刻の最終行の `ts` の 1 値、段 n の待ちは
//! `seat.pointer_ladder_s` の n 番目（[`Pace::wait_of`]・設計 §10 形 5）、列を越えた段は送らない。記録は席の置き場の 1 file
//! （[`LADDER_FILE`]・書き手はここだけ・一時 file → rename）で、注入の記録（`tick.jsonl` の `InjectionRecord`）とは別に持つ。
//!
//! 注入は既存の 1 入口（[`deliver_within`]）を 1 回撃つだけで、`tick.jsonl` の 1 行もその経路が書く。口座は計測済みの記録
//! （[`fresh_rows`]）を読むだけで**測らない**（FR38・子 process を起こさない）。env・home・自分の実行 file の場所は読まない
//! （C2.2）。
//!
//! 周期を作る systemd の unit は [`install`] が導出して書く（設計 §3・契約表の行 b）。
//!
//! 群の移動の続きも撃つ（設計 §4・契約表の行 c）: 梯子の評価の直後に移動の門（[`moving`]・自席の登録 row の口座 ≠ 群の今の
//! 口座）を置き、移動の周は群の段と同じ lock（[`Lock`]）の内側で、pane が shell なら同じ target に群の今の口座の席を起こし、
//! shell でなければ入力欄の門を通して `/exit`（dialog の既定の行なら Enter）を 1 手だけ送る。移動の周は合図を送らず梯子を
//! 触らず、event も記さない（記録は `tick.jsonl` と判定行だけ）。
//!
//! 死んだ席も起こす（設計 §7・契約表の行 f）: 登録 row を読んだ直後・打刻を読む前に窓が shell かを見て、shell の周は打刻と
//! 梯子を読まず起こす周（[`awake`]）へ進む。口座は anchor が群に属せば群の今の口座（lock の内側）、属さなければ row の口座で、
//! 打刻の最終行の sid を `--resume` で運び、初手の合図（[`relaunch_signal`]）を 1 語積む（[`state::resume_carry`]・§10 形 1）。移動の門（[`moving`]）は窓が shell でない周の退避だけを撃つ。
//! 移動の周の退避は打刻に依らない（設計 §10 形 8〜10・契約表の行 m）: 窓が shell でない周は打刻を読む前に移動の周かを見て、
//! 移動の周は打刻と梯子を読まず移動の門へ進む（Busy の周も /exit を周期ごとに送る・記録は 1 送信 1 行）。
//! 最終行の Busy が `seat.tick_stale_s` の 2 倍より古く入力欄が空の周（Stop の打刻を失った席）は Busy を無視して列の先へ進む
//! （設計 §7 形 7・契約表の行 h）。
//!
//! 群の移動の判定も撃つ（設計 §9・契約表の行 i）: `front` の後・移動の門の前に、自席の anchor が群に属し群の判定の打刻
//! （`<群>.judged`）が `fleet.usage_fresh_s` より古い（か無い）周だけ、群の段と同じ lock の内側で判定の 1 本
//! （[`crate::hook::group::judge`]）を撃って打刻を書く（[`judged`]）。判定の側は他の席に触らず、候補なしで断りの event を記した周
//! だけ断りの 1 行を自席へ送る。判定行の末尾は `judged=<moved:<label>|stay|none|error:<語>|->`。
//!
//! 合図は席ごとに止められる（設計 §12・契約表の行 o・ADR-0070）: 席の置き場の直下の停止の記録（[`HEARTBEAT_OFF_FILE`]・書き手は
//! [`heartbeat`] の口 1 本）が在る周は `back` の頭（黙りの門の前）で `heartbeat-off` の noop に止まり、梯子の記録を読まず書かない。
//! 起こし直し（[`awake`]）・退避（[`moving`]）・群の判定（[`judged`]）は記録を読まず、off の席でも撃つ。

pub mod install;

use super::cycle::{self, Launched, REASON_NO_ACCOUNT, REASON_NO_RULE};
use super::inject::{self, deliver_or_confirm, deliver_within, last_own_payload, pass_input, Blocked, Delivery, Request, Settled};
use super::role::Role;
use super::state::{self, Event, SeatState, Stamp};
use super::{host_groups_dir, pane_is_shell, pane_of, sanitize_target, seat_dir, state_dir_of, StateDir, REASON_TMUX_FAILED};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::usage::{self, fresh_rows};
use crate::fleet::State;
use crate::hook::group::{self, current_of, exit_dialog, group_of, pressed, Caps, Judgement, Lock, Refusal, EXIT};
use crate::name::NAME;
use crate::rules::manifest::{AccountGroup, Manifest};
use crate::rules::{int_row, list_row, RuleError};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// timer の周期の rules 行（tick の判定は読むだけ・unit を書く口が使う）。
pub const ROW_INTERVAL: &str = "seat.tick_interval_s";
/// 黙りの閾値・Busy の古さの 2 役の rules 行（秒）。
pub const ROW_STALE: &str = "seat.tick_stale_s";
/// 梯子の列の rules 行（秒の文字列の列・設計 §10 形 5）。
pub const ROW_LADDER: &str = "seat.pointer_ladder_s";
/// 送達の窓の rules 行（dispatcher の通知と同じ行・行を増やさない）。
const ROW_WINDOW: &str = "pipe.stop_grace_ms";

/// 梯子の記録の file 名（席の置き場の直下）。
pub const LADDER_FILE: &str = "pointer-ladder";
/// 停止の記録の file 名（席の置き場の直下・1 行 `ts=<UTC 秒>`・書き手は [`heartbeat`] の口 1 本・設計 §12 形 1）。
pub const HEARTBEAT_OFF_FILE: &str = "heartbeat-off";
/// 梯子の記録の schema 版。
const LADDER_SCHEMA: u64 = 1;
/// 評価していない欄の字面（0 に化けない・C10）。
const DASH: &str = "-";
/// 移動の周の送りの記録の `who`（`tick.jsonl` の 1 行・群の段の `pipe-group` と同じ形で名だけが違う）。
pub const WHO_MOVE: &str = "seat-tick-move";
/// 起こせた周の `launched=` の語。
const LAUNCHED_DONE: &str = "done";
/// dialog の既定の行へ Enter を送った周の `consumed=` の理由（消費の証拠を持たない送り＝`unknown:<理由>`）。
const CONSUMED_EXIT_DIALOG: &str = "exit-dialog";

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
    /// 段の候補が梯子の列を越える（打ち切り）。
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
    /// 群の今の口座の記録が在るのに読めない（種に読み替えない・C10）。
    GroupUnreadable,
    /// 群の段の lock を取れない（1 key も送らない＝同じ target を二重に撃たない）。
    GroupLocked,
    /// 停止の記録が在る（在るのに読めない周も・合図だけを止める・設計 §12 形 3・ADR-0070）。
    HeartbeatOff,
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
    NoopReason::GroupUnreadable,
    NoopReason::GroupLocked,
    NoopReason::HeartbeatOff,
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
            Self::GroupUnreadable => "group-unreadable",
            Self::GroupLocked => "group-locked",
            Self::HeartbeatOff => "heartbeat-off",
        }
    }
}

/// 移動の周の 1 手（**閉じた 3 値**・判定行の `move=` の語）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// pane が shell の席に群の今の口座の席を起こした（起動の結果は `launched=`）。
    Launch,
    /// `/exit` の 1 行を送った。
    Exit,
    /// `/exit` の確認 dialog の既定の行へ Enter を 1 回送った。
    Enter,
}

impl Move {
    /// 判定行の `move=` の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::Exit => "exit",
            Self::Enter => "enter",
        }
    }
}

/// 実行系が回らない理由（**閉じた列**・noop の語彙を汚さない＝席が静かなのか器が壊れているのかを記録から読める）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickError {
    /// 置き場を解けない。
    StateDir,
    /// 行 3 本・`pipe.stop_grace_ms`・群の閾値の行のどれかが読めない（不在・不発効・形違い・列が数でない / 昇順でない・壊れている）。
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

/// 1 周の判定（**閉じた 4 値**・C11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickDecision {
    /// 合図を送った（送達の結果は [`Verdict::sent`]）。
    Inject,
    /// 移動の周の 1 手を撃った（閉じた 3 値の手・送りの結果は [`Verdict::sent`]・起動の結果は [`Verdict::launched`]）。
    Move(Move),
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
            Self::Move(_) => "move",
            Self::Noop(_) => "noop",
            Self::Error(_) => "error",
        }
    }

    /// 判定行の `reason=` の語（注入した周と移動の周は `-`）。
    fn reason(self) -> &'static str {
        match self {
            Self::Inject | Self::Move(_) => DASH,
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
    /// 床を過ぎていて列の内（送った周は `sent`・送らなかった周は残り 0 秒の `wait:0`）。
    Open,
    /// 床の内（残り秒）。
    Wait(u64),
    /// 段の候補が梯子の列を越える。
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

/// 行 2 本の値（黙りの閾値と梯子の列）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pace {
    /// `seat.tick_stale_s`（黙りの閾値＝settle の基準・Busy の古さ）。
    pub stale_s: u64,
    /// `seat.pointer_ladder_s`（段 n の待ちは n 番目・非空・狭義に昇順）。
    pub ladder: Vec<u64>,
}

impl Pace {
    /// 列の字面を読む。要素が数でない・狭義に昇順でない・空の列は `Err`（既定に倒さない・C1）。
    pub fn of(stale_s: u64, items: &[String]) -> Result<Self, String> {
        let ladder = items.iter().map(|item| item.parse::<u64>().map_err(|_| format!("{ROW_LADDER} の要素 {item:?} が数でない")));
        let ladder = ladder.collect::<Result<Vec<u64>, String>>()?;
        if ladder.is_empty() || ladder.iter().zip(ladder.iter().skip(1)).any(|(before, after)| before >= after) {
            return Err(format!("{ROW_LADDER} が非空の狭義の昇順でない"));
        }
        Ok(Self { stale_s, ladder })
    }

    /// 段 `step` の待ち（秒・列の `step` 番目）。列を越えた段は `None`（打ち切り）。
    pub fn wait_of(&self, step: u32) -> Option<u64> {
        usize::try_from(step).ok().and_then(|at| self.ladder.get(at)).copied()
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

/// 段の候補の打ち切りと床（pure）: 段が列を越えれば [`Pointer::Stopped`]、記録の `sent_at` から待ちが経っていなければ
/// [`Pointer::Wait`]（残り秒）、それ以外（記録なしを含む）は [`Pointer::Open`]。
pub fn pointer_of(pace: &Pace, sent_at: Option<u64>, step: u32, now: u64) -> Pointer {
    let Some(wait) = pace.wait_of(step) else {
        return Pointer::Stopped;
    };
    match sent_at.map(|at| now.saturating_sub(at)) {
        Some(elapsed) if elapsed < wait => Pointer::Wait(wait.saturating_sub(elapsed)),
        _ => Pointer::Open,
    }
}

/// 打刻の合図の文面（**正本はこの 1 関数**・先頭の `<NAME> tick:` が器自身の目印・次の待ちは列から引き、最後の段は次が無い）。
pub fn signal(step: u32, pace: &Pace) -> String {
    let next = pace.wait_of(step.saturating_add(1)).map_or_else(|| "次の合図は無い・打ち切り".to_owned(), |wait| format!("次の合図は {wait} 秒後"));
    format!("{NAME} tick: heartbeat step={step} — 台帳の現在地（bd --readonly ready --limit 0）から続きを進める（変化が無ければ{next}）")
}

/// 起こし直しの初手の文面（**正本はこの 1 関数**・設計 §10 形 2・先頭の `<NAME> seat: relaunch` が器自身の目印）。起動行の末尾に
/// 単引用で括った 1 語として積まれる（[`state::resume_carry`]）ので、字面に単引用と改行を持たない。梯子の段には数えない。
pub fn relaunch_signal() -> String {
    format!("{NAME} seat: relaunch — 台帳の現在地（bd --readonly ready --limit 0）から続きを進める（会話は直前から続く・合図の梯子は段 0 から）")
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
    /// 注入した周と `/exit` / Enter を送った移動の周だけ（送達の結果）。
    pub sent: Option<Sent>,
    /// 起こした移動の周だけ（起動の結果の語）。
    pub launched: Option<&'static str>,
    /// 起こせた移動の周だけ（起動の前に置いた trust の印の語・判定には使わない・host-init.md §7 形 4）。
    pub trust: Option<&'static str>,
}

impl Verdict {
    /// 実行系が回らない周。
    fn error(error: TickError) -> Self {
        Self { decision: TickDecision::Error(error), ladder: None, sent: None, launched: None, trust: None }
    }

    /// 梯子の手前で止まった周（移動の門で止まった周も梯子を評価しない側）。
    fn noop(reason: NoopReason) -> Self {
        Self { decision: TickDecision::Noop(reason), ladder: None, sent: None, launched: None, trust: None }
    }

    /// 梯子を評価した後で止まった周。
    fn noop_at(reason: NoopReason, pointer: Pointer, step: u32) -> Self {
        Self { decision: TickDecision::Noop(reason), ladder: Some((pointer, step)), sent: None, launched: None, trust: None }
    }

    /// 移動の周の 1 手（梯子を評価しない＝`pointer=- step=-`）。
    fn moved(step: Move, sent: Option<Sent>, launched: Option<&'static str>) -> Self {
        Self { decision: TickDecision::Move(step), ladder: None, sent, launched, trust: None }
    }
}

/// 判定行（stdout の 1 行）。`pointer=` / `step=` は梯子を評価した周だけ・`consumed=` は送った周だけ・`move=` は移動の周だけ・
/// `launched=` は起こした移動の周だけ（他は `-`・列は固定で省かない）。起こせた周だけ `launched=` の後ろに `trust=<語>`。
pub fn render(target: &str, verdict: &Verdict) -> String {
    let sent = verdict.sent.is_some();
    let (pointer, step) =
        verdict.ladder.map_or_else(|| (DASH.to_owned(), DASH.to_owned()), |(pointer, step)| (pointer.render(sent), step.to_string()));
    let consumed = verdict.sent.map_or_else(|| DASH.to_owned(), Sent::render);
    let moved = match verdict.decision {
        TickDecision::Move(found) => found.as_str(),
        TickDecision::Inject | TickDecision::Noop(_) | TickDecision::Error(_) => DASH,
    };
    format!(
        "decision={} target={} reason={} pointer={pointer} step={step} consumed={consumed} move={moved} launched={}{}",
        verdict.decision.as_str(),
        sanitize_target(target),
        verdict.decision.reason(),
        verdict.launched.unwrap_or(DASH),
        verdict.trust.map(|word| format!(" trust={word}")).unwrap_or_default()
    )
}

/// 群の判定を撃った周の結果（**閉じた列**・判定行の `judged=` の語・撃たない周は [`Judged::Unjudged`] の `-`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judged {
    /// 撃たない（群の外・打刻が鮮度の内側・lock を取れない・前で止まった周）。
    Unjudged,
    /// 記録が移り先へ動いた。
    Moved(String),
    /// 今の口座は逼迫でない。
    Stay,
    /// 移り先が無い（断りの event を記した周も同じ実測に既に断った周も）。
    None,
    /// 判定できない（理由）。
    Error(&'static str),
}

impl Judged {
    /// 判定行の `judged=` の字面。
    fn render(&self) -> String {
        match self {
            Self::Unjudged => DASH.to_owned(),
            Self::Moved(label) => format!("moved:{label}"),
            Self::Stay => "stay".to_owned(),
            Self::None => "none".to_owned(),
            Self::Error(why) => format!("error:{why}"),
        }
    }
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
    let ((verdict, judged), err) = match (state_dir_of(Some(flags.state_dir)), manifest) {
        (None, _) => ((Verdict::error(TickError::StateDir), Judged::Unjudged), Vec::new()),
        (Some(_), Err(errors)) => ((Verdict::error(TickError::NoRule), Judged::Unjudged), crate::rules::cli::render_defects(&errors)),
        (Some(state), Ok(manifest)) => {
            let input = Input { state: &state, target: flags.target, socket: flags.socket, capture: flags.capture, manifest: &manifest };
            (judge(&input), Vec::new())
        }
    };
    let rc = if matches!(verdict.decision, TickDecision::Error(_)) { RC_REFUSED } else { RC_OK };
    Outcome { out: vec![format!("{} judged={}", render(flags.target, &verdict), judged.render())], err, rc }
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

/// 判定の列を 1 周撃つ（順序固定の AND・最初に立たなかった条件を理由にする）。`front` の中で窓が shell と読めた周は起こす周
/// （[`awake`]）で、移動の周（窓が claude）は移動の門（[`moving`]）で終わり、`front` の後に群の判定（[`judged`]）を撃ち、判定で
/// 移った周は同じ周の移動の門が退避を撃って、以後の列（黙り・上限・床・口座の門・合図の注入）を撃たない。
pub fn judge(input: &Input) -> (Verdict, Judged) {
    let found = match front(input) {
        Ok(found) => found,
        Err(stopped) => return (stopped, Judged::Unjudged),
    };
    let judged = judged(input, &found);
    let moved = matches!(judged, Judged::Moved(_))
        .then(|| moving(input, &found.anchor, &found.account, found.rows.window_ms))
        .flatten();
    (moved.map_or_else(|| back(input, &found), Ok).unwrap_or_else(|stopped| stopped), judged)
}

/// 群の判定の打刻の間隔の rules 行（計測の鮮度の行を流用・行を足さない・設計 §9 形 2）。
const ROW_FRESH: &str = "fleet.usage_fresh_s";

/// 群の判定（設計 §9 形 2 / 3）: 自席の anchor が群に属し、群の判定の打刻が `fleet.usage_fresh_s` より古い（か無い）周だけ、群の段
/// と同じ lock の内側で判定の 1 本を撃ち（他の群の今の口座 ∪ 記録を読めない群の候補を移り先から外し・鮮度に依らず測る口座と既に
/// 測った口座は空）、打刻を判定の時刻で書く。lock を取れない周は撃たない（列は今のまま）。断りの event を記した周だけ断りの 1 行を
/// 自席へ注入の経路で送る（入力欄の門を通らない周は落とす）。
fn judged(input: &Input, front: &Front) -> Judged {
    let state_dir = input.state.path.as_path();
    let Ok(manifest) = crate::rules::with_state_dir(input.manifest.clone(), Some(state_dir)) else {
        return Judged::Error(NoopReason::GroupUnreadable.as_str());
    };
    let Some(found) = group_of(&manifest, &front.anchor) else {
        return Judged::Unjudged;
    };
    let Ok(fresh_s) = int_row(&manifest, ROW_FRESH) else {
        return Judged::Error(TickError::NoRule.as_str());
    };
    let dir = host_groups_dir(state_dir);
    let stamp = group::judged_path(&dir, found.name());
    let last = fs::read_to_string(&stamp).ok().and_then(|text| text.trim().parse::<u64>().ok());
    if last.is_some_and(|ts| !aged(ts, front.now, fresh_s)) {
        return Judged::Unjudged;
    }
    let Ok(_lock) = Lock::take(&dir) else {
        return Judged::Unjudged;
    };
    // 各群の今の口座（記録を読めない群は候補の全部）＝移り先にせず、先の群の予約もこの集合から導く（§29 形 2 / 3）。
    let currents = group::currents_of(state_dir, &manifest);
    let measure = |label: &str, _: bool| {
        let _ = usage::run_fresh(&["--account".to_owned(), label.to_owned()], state_dir);
    };
    let forced = BTreeSet::new();
    let (head, taken) = (&currents, &currents);
    let judge = group::Judge { state_dir, manifest: &manifest, group: found, head, taken, forced: &forced, caps: front.rows.caps, measure: &measure };
    let judgement = group::judge(&judge, &mut BTreeSet::new());
    let _ = fs::write(&stamp, format!("{}\n", front.now));
    match judgement {
        Judgement::Moved(label) => Judged::Moved(label),
        Judgement::Stay(_) => Judged::Stay,
        Judgement::NoCandidate(Refusal::Recorded) => {
            refused(input, found, front.rows.window_ms);
            Judged::None
        }
        Judgement::NoCandidate(Refusal::Repeated) => Judged::None,
        Judgement::Unreadable => Judged::Error("unreadable"),
    }
}

/// 断りの 1 行（群の段の断りの字面）を自席へだけ送る（[`deliver_within`]＝入力欄の門を通った周だけ・通らない周は落とす）。
fn refused(input: &Input, found: &AccountGroup, window_ms: u64) {
    let payload = group::refused_line(found);
    let request = Request { target: input.target, socket: input.socket, payload: &payload, state_dir: Some(input.state) };
    let _ = deliver_within(&request, Duration::from_millis(window_ms));
}

/// rules の行（行 3 本・送達の窓・群の閾値）。
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
        let pace = Pace::of(int_row(manifest, ROW_STALE)?, list_row(manifest, ROW_LADDER)?)?;
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
    /// 登録 row の anchor（移動の門が群を引く）。
    anchor: String,
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
    /// 停止の記録が在る（梯子の記録を読まない周・`back` の頭で止まる）。
    off: bool,
}

/// 形 1 の 1〜3（登録 row → 窓が shell か〔§7 形 1〕→ 移動の周か〔§10 形 8〕→ 状態の打刻 → digest の比較）。窓が shell の周は
/// 打刻と梯子を読まず起こす周（[`awake`]）の判定で、移動の周（窓が claude）は打刻と梯子を読まず移動の門（[`moving`]）の判定で
/// 止まる（Busy の周も /exit を送る＝turn の途中の /exit は入力の列に積まれ turn の終わりで実行される・§10 形 9）。
fn front(input: &Input) -> Result<Front, Verdict> {
    let rows = Rows::of(input.manifest).map_err(|_| Verdict::error(TickError::NoRule))?;
    let now = state::now_secs();
    let events = crate::fleet::store::read_all(&input.state.path).map_err(|_| Verdict::error(TickError::Store))?;
    let fleet = crate::fleet::replay(&events);
    let (account, role, anchor) = super::role::registration_of_target(&fleet, input.target)
        .map(|found| (found.account.clone(), found.role, found.anchor.clone()))
        .ok_or_else(|| Verdict::noop(NoopReason::NoRow))?;
    let seat = seat_dir(&input.state.path, input.target);
    if pane_is_shell(input.socket, input.target) {
        return Err(awake(input, &account, role, &anchor, &seat));
    }
    if let Some(moved) = moving(input, &anchor, &account, rows.window_ms) {
        return Err(moved);
    }
    let stamps = stamps_of(&seat, rows.pace.stale_s, now, || input_gate(input).is_ok()).map_err(Verdict::noop)?;
    let digest = stamps.last().map_or(0, |stamp| stamp.ts);
    let off = heartbeat_off(&seat);
    let record = if off { None } else { read_ladder(&seat).map_err(Verdict::noop)? };
    let record = match record {
        Some(found) => Some(settled(&seat, found, &stamps, digest, (now, rows.pace.stale_s))?),
        None => None,
    };
    let step = candidate(record.as_ref(), digest);
    let pointer = pointer_of(&rows.pace, record.map(|found| found.sent_at), step, now);
    Ok(Front { rows, fleet, account, anchor, seat, digest, step, pointer, now, off })
}

/// 窓が shell の周（設計 §7 形 1〜3）: 打刻と梯子を読まず、同じ target に席を起こす（[`wake`]）。口座は anchor が群に属せば群の
/// 今の口座（[`current_of`]・記録 > 種・群の段と同じ lock の内側）、属さなければ登録 row の口座（群 0 の host を含む）。記録が
/// 在るのに読めない周と host の面が読めない周は `group-unreadable`・lock を取れない周は `group-locked`。起こし直しは打刻の最終行の
/// sid を `--resume` で運び初手の 1 語を積む（[`state::resume_carry`]・row の launch には載せない）。
fn awake(input: &Input, account: &str, role: Role, anchor: &str, seat: &Path) -> Verdict {
    let Ok(manifest) = crate::rules::with_state_dir(input.manifest.clone(), Some(&input.state.path)) else {
        return Verdict::noop(NoopReason::GroupUnreadable);
    };
    let carry = state::resume_carry(seat);
    let Some(group) = group_of(&manifest, anchor) else {
        return wake(input, &manifest, (role, anchor), account, &carry);
    };
    let Ok(current) = current_of(&input.state.path, group) else {
        return Verdict::noop(NoopReason::GroupUnreadable);
    };
    let Ok(_lock) = Lock::take(&host_groups_dir(&input.state.path)) else {
        return Verdict::noop(NoopReason::GroupLocked);
    };
    wake(input, &manifest, (role, anchor), &current.label, &carry)
}

/// 移動の門（設計 §4 形 1 / 2 / 4）: 登録 row の anchor が群に属し、群の今の口座（[`current_of`]・記録 > 種）が row の口座と違う周
/// だけ `Some`（移動の周）。群に属さない anchor・群 0 の host・記録と row が一致する席は `None`（今の列のまま）。記録が在るのに
/// 読めない周と host の面が読めない周は `group-unreadable`（種に読み替えない・C10）。移動の周は群の段と同じ lock の内側で退避の
/// 1 手（[`evacuate`]）を撃つ（窓が shell の周は `front` の [`awake`] が先に起こす）。lock を取れない周は `group-locked`。
/// 呼ぶ場所は `front` の中（打刻の前・§10 形 8）と、群の判定（[`judged`]）で移った周の後（§9 形 3）の 2 つで、関数は 1 本。
fn moving(input: &Input, anchor: &str, account: &str, window_ms: u64) -> Option<Verdict> {
    let Ok(manifest) = crate::rules::with_state_dir(input.manifest.clone(), Some(&input.state.path)) else {
        return Some(Verdict::noop(NoopReason::GroupUnreadable));
    };
    let group = group_of(&manifest, anchor)?;
    let Ok(current) = current_of(&input.state.path, group) else {
        return Some(Verdict::noop(NoopReason::GroupUnreadable));
    };
    if current.label == account {
        return None;
    }
    let Ok(_lock) = Lock::take(&host_groups_dir(&input.state.path)) else {
        return Some(Verdict::noop(NoopReason::GroupLocked));
    };
    Some(evacuate(input, window_ms))
}

/// 窓が shell の周の起こし（§4 形 3・§7 形 3）: `launch` の 1 本で同じ target に `account` の席を起こす（anchor と役割は自分の
/// row・置き場は自分の置き場・settle / step は rules 行・登録 row は起動が書き直す・会話は `carry` で運ぶ・呼び手の窓を置き換え
/// ない）。起こせない周（行が読めない・断り・失敗・候補なし）も語を載せて返す（次の周がまた判じる）。
fn wake(input: &Input, manifest: &Manifest, row: (Role, &str), account: &str, carry: &[String]) -> Verdict {
    let (Some((settle, step)), Ok(rules)) = (cycle::pace_of(manifest), super::embedded_manifest()) else {
        return Verdict::moved(Move::Launch, None, Some(REASON_NO_RULE));
    };
    let (role, anchor) = (row.0, Path::new(row.1));
    let carry: Vec<&str> = carry.iter().map(String::as_str).collect();
    let launched = cycle::launch(&cycle::Launch {
        target: input.target,
        socket: input.socket,
        state_dir: input.state,
        restore: None,
        settle,
        step,
        role,
        anchor,
        account: Some(account),
        model: None,
        manifest,
        rules: &rules,
        threshold_pct: 0,
        carry: &carry,
        replace_own: false,
    });
    let trust = match &launched {
        Launched::Done(_, _, trust) => Some(trust.as_str()),
        Launched::None(_) | Launched::Refused(_) | Launched::Failed(_) => None,
    };
    Verdict { trust, ..Verdict::moved(Move::Launch, None, Some(launched_word(&launched))) }
}

/// 起動の結果の語（起こせた周は [`LAUNCHED_DONE`]・他は断り・失敗の理由）。
fn launched_word(launched: &Launched) -> &'static str {
    match launched {
        Launched::Done(..) => LAUNCHED_DONE,
        Launched::None(_) => REASON_NO_ACCOUNT,
        Launched::Refused(reason) | Launched::Failed(reason) => reason,
    }
}

/// pane が shell でない移動の周（形 4）: 入力欄の門（[`input_gate`]＝§2 と同じ `pass_input`）を通し、空なら `/exit` の 1 行を
/// 送る（[`deliver_or_confirm`]＝`deliver_within` と同じ門・送り・settle・窓は `pipe.stop_grace_ms`・送った周は未確認でも
/// `tick.jsonl` に [`WHO_MOVE`] の 1 行）。門が人の文字（Foreign）で、その tail が dialog の既定の行の周だけ Enter を 1 回送る。
/// それ以外の Foreign / 特定できない入力欄 / 残る自席の文は今の語で止まる（OwnQueued の Enter は `pass_input` のまま）。
fn evacuate(input: &Input, window_ms: u64) -> Verdict {
    let foreign = match input_gate(input) {
        Ok(()) => false,
        Err(NoopReason::InputBusy) => true,
        Err(reason) => return Verdict::noop(reason),
    };
    let request = Request { target: input.target, socket: input.socket, payload: EXIT, state_dir: Some(input.state) };
    match deliver_or_confirm(&request, Duration::from_millis(window_ms), &exit_dialog(WHO_MOVE)) {
        inject::Sent::Confirmed(entered) => {
            let why = if entered { CONSUMED_EXIT_DIALOG } else { REASON_TMUX_FAILED };
            Verdict::moved(Move::Enter, Some(Sent::Unconfirmed(why)), None)
        }
        inject::Sent::Payload(Delivery::Refused(_)) if foreign => Verdict::noop(NoopReason::InputBusy),
        inject::Sent::Payload(delivery) => Verdict::moved(Move::Exit, Some(Sent::of(delivery)), None),
    }
}

/// 形 1 の 4〜10（停止の記録の門〔§12 形 3〕→ 黙りの門 → 上限 → 床 → 口座の門 → 入力欄の門 → 記録 → 注入）。
fn back(input: &Input, front: &Front) -> Result<Verdict, Verdict> {
    if front.off {
        return Err(Verdict::noop(NoopReason::HeartbeatOff));
    }
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
    let payload = signal(front.step, &front.rows.pace);
    let request = Request { target: input.target, socket: input.socket, payload: &payload, state_dir: Some(input.state) };
    let delivery = deliver_within(&request, Duration::from_millis(front.rows.window_ms));
    Ok(Verdict {
        decision: TickDecision::Inject,
        ladder: Some((front.pointer, front.step)),
        sent: Some(Sent::of(delivery)),
        launched: None,
        trust: None,
    })
}

/// `ts` から `stale_s` 以上経ったか（黙りの門と Busy の古さの同じ 1 本・時計は打刻と同じ UTC 秒）。
fn aged(ts: u64, now: u64, stale_s: u64) -> bool {
    now.saturating_sub(ts) >= stale_s
}

/// Stop の打刻を失った席と読む Busy の古さの係数（設計 §7 形 7・`seat.tick_stale_s` × 係数・rules 行を足さない＝係数は歯が pin する）。
const LOST_STOP_FACTOR: u64 = 2;

/// 状態の打刻を読み、最終行（読めた行のうち最後）が Idle の周だけ全行を返す。最終行の Busy が `stale_s` の
/// [`LOST_STOP_FACTOR`] 倍より古く `empty`（窓が claude の入力欄の門）が真の周も Busy を無視して全行を返す（設計 §7 形 7・打刻は
/// 書き換えない）。
fn stamps_of(seat: &Path, stale_s: u64, now: u64, empty: impl FnOnce() -> bool) -> Result<Vec<Stamp>, NoopReason> {
    let text = match fs::read_to_string(state::path(seat)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(NoopReason::StateMissing),
        Err(_) => return Err(NoopReason::StateUnreadable),
    };
    let stamps: Vec<Stamp> = text.lines().filter_map(|line| Stamp::from_line(line).ok()).collect();
    let last = stamps.last().ok_or(NoopReason::StateUnreadable)?;
    match last.state {
        SeatState::Idle => Ok(stamps),
        SeatState::Busy if aged(last.ts, now, stale_s.saturating_mul(LOST_STOP_FACTOR)) && empty() => Ok(stamps),
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

/// `seat heartbeat` の後ろの語（**閉じた 3 値**・設計 §12 形 2・positional）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    /// 停止の記録を置く（既に在れば触らない）。
    Off,
    /// 停止の記録を消す（無ければ何もしない）。
    On,
    /// 停止の記録の有無を 1 行で出す。
    Status,
}

/// [`Switch`] の全部（宣言順）。
pub const SWITCHES: &[Switch] = &[Switch::Off, Switch::On, Switch::Status];

impl Switch {
    /// 引数の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Status => "status",
        }
    }

    /// 字面から読む（3 語でなければ `None`）。
    pub fn parse(token: &str) -> Option<Self> {
        SWITCHES.iter().copied().find(|switch| switch.as_str() == token)
    }
}

/// 停止の記録の path。
pub fn heartbeat_off_path(seat: &Path) -> PathBuf {
    seat.join(HEARTBEAT_OFF_FILE)
}

/// 停止の記録が在るか。無い周だけ偽で、在るのに読めない周（dir・読めない）も真（合図は正の証拠でだけ送る）。
fn heartbeat_off(seat: &Path) -> bool {
    !matches!(fs::symlink_metadata(heartbeat_off_path(seat)), Err(err) if err.kind() == std::io::ErrorKind::NotFound)
}

/// `seat heartbeat off|on|status`（設計 §12 形 2）: 登録 row の無い target は `no-row`（rc 1・席の置き場を作らない＝FR40）。off は
/// 停止の記録を置き（一時 file → rename・既に在れば ts を書き換えない）、on は消し（無ければ何もしない）、status は有無を 1 行で
/// 出す（`last=` 以下は行 p の打刻・今は `-`）。event log には書かない。
pub fn heartbeat(switch: Switch, state_dir: &str, target: &str) -> Outcome {
    let head = format!("seat heartbeat {}:", switch.as_str());
    let refused = |rc, reason: &str| Outcome::failed_line(rc, format!("{head} refused reason={reason} target={target}"));
    let Some(state) = state_dir_of(Some(state_dir)) else {
        return refused(RC_REFUSED, TickError::StateDir.as_str());
    };
    let Ok(events) = crate::fleet::store::read_all(&state.path) else {
        return refused(RC_BROKEN, TickError::Store.as_str());
    };
    if super::role::registration_of_target(&crate::fleet::replay(&events), target).is_none() {
        return refused(RC_REFUSED, NoopReason::NoRow.as_str());
    }
    let seat = seat_dir(&state.path, target);
    let written = match switch {
        Switch::Off => switch_off(&seat),
        Switch::On => switch_on(&seat),
        Switch::Status => Ok(()),
    };
    if written.is_err() {
        return refused(RC_BROKEN, NoopReason::RecordUnwritable.as_str());
    }
    let word = if heartbeat_off(&seat) { Switch::Off } else { Switch::On }.as_str();
    let tail = match switch {
        Switch::Status => format!(" last={DASH} decision={DASH} reason={DASH}"),
        Switch::Off | Switch::On => String::new(),
    };
    Outcome::ok_line(format!("{head} target={target} heartbeat={word}{tail}"))
}

/// 停止の記録を置く（既に在る周は触らない・席の置き場は登録 row の在る席にだけ作る・一時 file → rename）。
fn switch_off(seat: &Path) -> std::io::Result<()> {
    if heartbeat_off(seat) {
        return Ok(());
    }
    fs::create_dir_all(seat)?;
    let temporary = seat.join(format!("{HEARTBEAT_OFF_FILE}.tmp"));
    fs::write(&temporary, format!("ts={}\n", state::now_secs()))?;
    fs::rename(&temporary, heartbeat_off_path(seat))
}

/// 停止の記録を消す（無い周は何もしない）。
fn switch_on(seat: &Path) -> std::io::Result<()> {
    match fs::remove_file(heartbeat_off_path(seat)) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
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
        candidate, pointer_of, render, settle, signal, Ladder, Move, NoopReason, Pace, Pointer, Sent, TickDecision, TickError,
        Verdict, NOOP_REASONS, TICK_ERRORS,
    };
    use crate::name::NAME;
    use crate::order::is_declaration_order;
    use crate::seat::inject::{Settled, Unmeasured};
    use crate::seat::state::{Event, Stamp, SCHEMA};

    /// 初期値の梯子の列（rules 行の値を写さない fixture・値の対応は e2e が manifest で測る）。
    const LADDER: [u64; 6] = [1800, 3600, 10_800, 21_600, 43_200, 86_400];

    /// 初期値の梯子（黙りの閾値 1800 と列）。
    fn pace() -> Pace {
        Pace { stale_s: 1800, ladder: LADDER.to_vec() }
    }

    /// 打刻 1 行。
    fn stamp(event: Event, ts: u64) -> Stamp {
        Stamp { schema: SCHEMA, state: event.state(), event, ts, sid: String::new() }
    }

    /// 段 → 待ちは列の n 番目で、段 0〜5 が列の内・段 6 から打ち切り（合図の本数は列の長さ）。
    #[test]
    fn seat_tick_wait_follows_the_ladder_and_stops_past_it() {
        let pace = pace();
        let waits: Vec<Option<u64>> = (0..=6).map(|step| pace.wait_of(step)).collect();
        let want: Vec<Option<u64>> = LADDER.iter().copied().map(Some).chain([None]).collect();
        assert_eq!(waits, want, "列の待ちと段 6 の打ち切り");
        let open = (0..=6).filter(|step| pointer_of(&pace, None, *step, 0) == Pointer::Open).count();
        assert_eq!(open, LADDER.len(), "送る段は列の長さ");
        assert_eq!(pointer_of(&pace, None, 6, 0), Pointer::Stopped, "段 6 は列を越える（段 0〜5 は記録なしで床を通る）");
        assert_eq!(pointer_of(&pace, None, u32::MAX, 0), Pointer::Stopped, "段の飽和も打ち切り");
        let one = Pace { stale_s: 1, ladder: vec![10] };
        assert_eq!((pointer_of(&one, None, 0, 0), pointer_of(&one, None, 1, 0)), (Pointer::Open, Pointer::Stopped), "1 要素の列");
    }

    /// 列の字面: 数で狭義に昇順の列は読み、数でない・昇順でない（等しい隣を含む）・空の列は断る。
    #[test]
    fn seat_tick_pace_reads_the_ladder_and_refuses_malformed_lists() {
        let words = ["1800", "3600", "10800", "21600", "43200", "86400"];
        assert_eq!(Pace::of(1800, &words.map(str::to_owned)), Ok(pace()), "初期値の列");
        for bad in [&["1800", "x"][..], &["1800", "-1"], &["1800", " 3600"], &["3600", "1800"], &["1800", "1800"], &[]] {
            let items: Vec<String> = bad.iter().map(|word| (*word).to_owned()).collect();
            assert!(Pace::of(1800, &items).is_err(), "{bad:?} は断る");
        }
    }

    /// 床: `sent_at` から待ちが経っていない周は残り秒の Wait・ちょうど経った周は Open。
    #[test]
    fn seat_tick_floor_counts_the_seconds_left_from_sent_at() {
        let pace = pace();
        assert_eq!(pointer_of(&pace, Some(1000), 1, 1000 + 3000), Pointer::Wait(600));
        assert_eq!(pointer_of(&pace, Some(1000), 1, 1000 + 3600), Pointer::Open, "境界は経った側");
        assert_eq!(pointer_of(&pace, Some(1000), 0, 1000 + 1799), Pointer::Wait(1));
        assert_eq!(pointer_of(&pace, Some(5000), 0, 10), Pointer::Wait(1800), "未来の sent_at は経過 0");
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
        assert_eq!(NOOP_REASONS.len(), 19, "母集団");
        let words: Vec<&str> = NOOP_REASONS.iter().map(|reason| reason.as_str()).collect();
        assert_eq!(
            words,
            [
                "no-row", "state-missing", "state-unreadable", "busy", "state-stale", "settling", "record-unreadable",
                "stamp-recent", "stopped", "wait", "account-pressed", "pane-missing", "input-busy", "input-unknown",
                "input-own-queued", "record-unwritable", "group-unreadable", "group-locked", "heartbeat-off",
            ]
        );
        assert!(is_declaration_order(TICK_ERRORS, |error| error as usize), "TICK_ERRORS は宣言順");
        let errors: Vec<&str> = TICK_ERRORS.iter().map(|error| error.as_str()).collect();
        assert_eq!(errors, ["state-dir", "no-rule", "store"]);
        let mut all: Vec<&str> = words.iter().chain(errors.iter()).copied().collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 22, "noop と error の語は重ならない");
    }

    /// 判定行: 梯子の手前は `pointer=- step=-`・梯子の後は評価と段・注入した周だけ `consumed=`（届かない周は unknown:理由）。
    /// 移動の周でない行は末尾に `move=- launched=-` を持つ。
    #[test]
    fn seat_tick_render_dashes_before_the_ladder() {
        assert_eq!(
            render("s:w", &Verdict::noop(NoopReason::NoRow)),
            "decision=noop target=s_w reason=no-row pointer=- step=- consumed=- move=- launched=-"
        );
        assert_eq!(
            render("s:w", &Verdict::error(TickError::NoRule)),
            "decision=error target=s_w reason=no-rule pointer=- step=- consumed=- move=- launched=-"
        );
        assert_eq!(
            render("s:w", &Verdict::noop_at(NoopReason::Wait, Pointer::Wait(30), 2)),
            "decision=noop target=s_w reason=wait pointer=wait:30 step=2 consumed=- move=- launched=-"
        );
        assert_eq!(
            render("s:w", &Verdict::noop_at(NoopReason::StampRecent, Pointer::Open, 0)),
            "decision=noop target=s_w reason=stamp-recent pointer=wait:0 step=0 consumed=- move=- launched=-"
        );
        let sent =
            |found| Verdict { decision: TickDecision::Inject, sent: Some(found), ..Verdict::noop_at(NoopReason::Wait, Pointer::Open, 1) };
        assert_eq!(
            render("s:w", &sent(Sent::Settled(Settled::Queued))),
            "decision=inject target=s_w reason=- pointer=sent step=1 consumed=false move=- launched=-"
        );
        assert!(render("s:w", &sent(Sent::Settled(Settled::Unmeasured(Unmeasured::StateMissing))))
            .ends_with(" consumed=unknown:state-missing move=- launched=-"));
        assert!(render("s:w", &sent(Sent::Unconfirmed("absent"))).ends_with(" consumed=unknown:absent move=- launched=-"));
    }

    /// 移動の門の 2 値と停止の記録の門の 1 値（設計 §12 形 3）は `NoopReason` の宣言順の末尾に在り、既存の 16 値の語と重ならない
    /// （母集団は 16 → 18 → 19）。
    #[test]
    fn seat_tick_tail_reasons_are_the_last_three_in_declaration_order() {
        assert!(is_declaration_order(NOOP_REASONS, |reason| reason as usize), "NOOP_REASONS は宣言順");
        let tail: Vec<&str> = NOOP_REASONS.iter().rev().take(3).map(|reason| reason.as_str()).collect();
        assert_eq!(tail, ["heartbeat-off", "group-locked", "group-unreadable"], "末尾の 3 値（逆順）");
        let mut words: Vec<&str> = NOOP_REASONS.iter().map(|reason| reason.as_str()).collect();
        let before = words.len();
        words.sort_unstable();
        words.dedup();
        assert_eq!((before, words.len()), (19, 19), "19 値で重複しない");
        let moves: Vec<&str> = [Move::Launch, Move::Exit, Move::Enter].iter().map(|found| found.as_str()).collect();
        assert_eq!(moves, ["launch", "exit", "enter"], "手の語");
    }

    /// 移動の周の判定行は `decision=move reason=- pointer=- step=-` で、送りの結果を `consumed=`・手を `move=`・起こした周だけ
    /// 起動の結果を `launched=` に載せる（他は `-`）。
    #[test]
    fn seat_tick_move_render_names_the_step_and_the_launch_word() {
        assert_eq!(
            render("s:w", &Verdict::moved(Move::Launch, None, Some("launch-unconfirmed"))),
            "decision=move target=s_w reason=- pointer=- step=- consumed=- move=launch launched=launch-unconfirmed"
        );
        assert_eq!(
            render("s:w", &Verdict { trust: Some("written"), ..Verdict::moved(Move::Launch, None, Some("done")) }),
            "decision=move target=s_w reason=- pointer=- step=- consumed=- move=launch launched=done trust=written",
            "起こせた周は launched= の後ろに trust="
        );
        assert_eq!(
            render("s:w", &Verdict::moved(Move::Exit, Some(Sent::Settled(Settled::Queued)), None)),
            "decision=move target=s_w reason=- pointer=- step=- consumed=false move=exit launched=-"
        );
        assert_eq!(
            render("s:w", &Verdict::moved(Move::Enter, Some(Sent::Unconfirmed("exit-dialog")), None)),
            "decision=move target=s_w reason=- pointer=- step=- consumed=unknown:exit-dialog move=enter launched=-"
        );
        assert_eq!(
            render("s:w", &Verdict::noop(NoopReason::GroupLocked)),
            "decision=noop target=s_w reason=group-locked pointer=- step=- consumed=- move=- launched=-"
        );
    }

    /// 合図の文面は器の目印で始まり、段と次の段の待ち（列から引く）を名乗り、最後の段は次が無いと書く。
    #[test]
    fn seat_tick_signal_names_the_step_and_the_next_wait() {
        let pace = pace();
        let line = signal(0, &pace);
        assert!(line.starts_with(&format!("{NAME} tick: heartbeat step=0 — ")), "{line}");
        assert!(line.ends_with("（変化が無ければ次の合図は 3600 秒後）"), "{line}");
        assert!(!line.contains('\n'), "1 行");
        let nexts: Vec<bool> =
            (0u32..).zip(LADDER.iter().skip(1)).map(|(step, next)| signal(step, &pace).contains(&format!("次の合図は {next} 秒後"))).collect();
        assert_eq!(nexts, [true; 5], "段 0〜4 の次は列の次の要素");
        let last = signal(5, &pace);
        assert!(last.ends_with("（変化が無ければ次の合図は無い・打ち切り）"), "最後の段は次が無い: {last}");
    }
}
