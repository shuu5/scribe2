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
//! 登録 row の口座が墓標の席は打刻と梯子を読まず群の判定へ進み、合図は `account-dead` で止める（[`tombstone`]・設計
//! account-lifecycle.md §38 行 ad）。窓が shell の群の席は今の口座が墓標なら起こす前に群の判定を撃つ。
//!
//! park の区画の席も移す（設計 §21・契約表の行 z）: 群の移動の門の後に区画の判定（[`park`]・読むだけ＝計測・lock・記録の書き換え
//! は 0）を周ごとに 1 回撃ち、移り先の周は群と同じ退避の合図と起こし直しで区画の行の口座へ移る（鍵は row の口座と登録の seq）。
//!
//! 合図は席ごとに止められる（設計 §12・契約表の行 o・ADR-0070）: 実効の値（[`beat::resolve`]・明示の記録 → 群の表の行の key →
//! 種類の既定・設計 §22・書き手は [`heartbeat`] の口 1 本）が off か unreadable の周は `back` の頭（黙りの門の前）で
//! `heartbeat-off` の noop に止まり、梯子の記録を読まず書かない。起こし直し（[`awake`]）・退避（[`moving`]）・群の判定
//! （[`judged`]）は値を読まず、off の席でも撃つ。
//!
//! 毎周の判定の後に最後の周の打刻（[`TICK_LAST_FILE`]）を書き、健全は読み手（[`status`]・doctor）が [`Health`] で判じる（行 p）。
//!
//! 移動の周の `/exit` は猶予（`seat.move_grace_s`・起点は席に合図を書いた時刻）を越えてから送り、合図の記録が無ければ先に退避の
//! 合図の 1 行を 1 度だけ送る（設計 §13 / §14・契約表の行 q / 行 r・ADR-0071 / ADR-0073＝席の裏の subagent を `/exit` で落とさない）。

pub mod beat;
pub mod install;
mod park;
pub(crate) mod signal;

pub use signal::{candidate, pointer_of, raise, settle, signal, Ladder, Pace};
use signal::idle_alarm;
use super::cycle::{self, Launched, REASON_NO_ACCOUNT, REASON_NO_RULE};
use super::inject::{self, deliver_or_confirm, deliver_within, last_own_payload, pass_input, Blocked, Delivery, Request, Settled};
use super::role::Role;
use super::state::{self, SeatState, Stamp};
use super::{host_groups_dir, pane_is_shell, pane_of, sanitize_target, seat_dir, state_dir_of, StateDir, REASON_TMUX_FAILED};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::fleet::lifecycle_read;
use crate::fleet::usage::{self, fresh_rows};
use crate::fleet::select::{select, Input as SelectInput, Purpose, Selection, LIMIT_PCT};
use crate::fleet::{Registration, RegistrationLatest, State};
use crate::hook::group::{self, current_of, exit_dialog, group_of, pressed, Caps, Judgement, Lock, Pending, Refusal, Step, EXIT};
use crate::name::NAME;
use crate::pipe::dispatch::facts;
use crate::rules::manifest::{AccountGroup, HostManifest, Manifest};
use crate::rules::{int_row, list_row, RuleError};
use std::collections::{BTreeMap, BTreeSet};
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
/// 群の移動の退避の猶予の rules 行（秒・起点は群の記録の ts・設計 §13 形 1）。
pub const ROW_GRACE: &str = "seat.move_grace_s";
/// heartbeat の段の上げの rules 行（秒・任意の行＝[`Rows::of`] の必須に入れない・設計 §17 形 1）。
pub const ROW_IDLE_ALARM: &str = "seat.idle_alarm_s";
/// 事前審査の確定の束の段の上げの rules 行（秒・任意の行・設計 dispatcher.md §27 形 4）。
pub const ROW_PRECHECK_ALARM: &str = "seat.precheck_alarm_s";

/// 梯子の記録の file 名（席の置き場の直下）。
pub const LADDER_FILE: &str = "pointer-ladder";
/// 最後の周の打刻の file 名（席の置き場の直下・1 行 `ts=<UTC 秒> decision=<語> reason=<語>`・書き手は [`run`] 1 本・設計 §12 行 p 形 1）。
pub const TICK_LAST_FILE: &str = "tick-last";
/// 健全の係数（経過 ≤ `seat.tick_interval_s` × 係数・rules 行を足さない＝係数は歯が pin する・設計 §12 行 p 形 2）。
const HEALTH_FACTOR: u64 = 2;
/// 在るのに読めない記録の字面（`-` に潰さない）。
const UNREADABLE: &str = "unreadable";
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
    /// 登録 row の口座が墓標（合図を送らない・毎周 credential の file を読む・設計 account-lifecycle.md §38 形 8）。
    AccountDead,
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
    NoopReason::AccountDead,
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
            Self::AccountDead => "account-dead",
        }
    }
}

/// 移動の周の 1 手（**閉じた 5 値**・判定行の `move=` の語）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// pane が shell の席に群の今の口座の席を起こした（起動の結果は `launched=`）。
    Launch,
    /// `/exit` の 1 行を送った。
    Exit,
    /// `/exit` の確認 dialog の既定の行へ Enter を 1 回送った。
    Enter,
    /// 猶予の内側で退避の合図の 1 行を送った（設計 §13 形 5 (c)）。
    Signal,
    /// 猶予の内側でこの移動の合図を送り済み（0 key・設計 §13 形 5 (b)）。
    Wait,
}

impl Move {
    /// 判定行の `move=` の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::Exit => "exit",
            Self::Enter => "enter",
            Self::Signal => "signal",
            Self::Wait => "wait",
        }
    }
}

/// 実行系が回らない理由（**閉じた列**・noop の語彙を汚さない＝席が静かなのか器が壊れているのかを記録から読める）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickError {
    /// 置き場を解けない。
    StateDir,
    /// 行 3 本・`pipe.stop_grace_ms`・`seat.move_grace_s`・群の閾値の行のどれかが読めない（不在・不発効・形違い・列が数でない /
    /// 昇順でない・壊れている）。
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
    /// 移動の周の 1 手を撃った（閉じた 5 値の手・送りの結果は [`Verdict::sent`]・起動の結果は [`Verdict::launched`]）。
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

/// 起こし直しの初手の文面（**正本はこの 1 関数**・設計 §10 形 2・§18 形 6・先頭の `<NAME> seat: relaunch` が器自身の目印）。起動行の
/// 末尾に単引用で括った 1 語として積まれる（[`state::resume_carry`]）ので、字面に単引用と改行を持たない。梯子の段には数えない。
pub fn relaunch_signal() -> String {
    format!(
        "{NAME} seat: relaunch — 台帳の現在地（bd --readonly ready --limit 0）から続きを進める（会話は直前から続く・移動で落ちた subagent は台帳に書いた要旨と path から起こし直す・合図の梯子は段 0 から）"
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
    /// 区画の席の移り先（区画の行の口座・設計 §21 形 5）。
    Parked(String),
    /// 区画の席の row の口座が測れない（記録なし・鮮度の外）。
    Unmeasured,
    /// 区画の席の移り先が無い。
    ParkNone,
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
            Self::Parked(label) => format!("park:{label}"),
            Self::Unmeasured => "unmeasured".to_owned(),
            Self::ParkNone => "park-no-candidate".to_owned(),
        }
    }

    /// 区画の判定の結果から写す（区画の外の席は撃たない周＝`-`）。
    fn of_park(found: &park::Park) -> Self {
        match found {
            park::Park::Outside => Self::Unjudged,
            park::Park::NoRule => Self::Error(TickError::NoRule.as_str()),
            park::Park::Unmeasured => Self::Unmeasured,
            park::Park::Stay => Self::Stay,
            park::Park::To(label) => Self::Parked(label.clone()),
            park::Park::NoCandidate => Self::ParkNone,
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
    let state = state_dir_of(Some(flags.state_dir));
    let ((verdict, judged), err) = match (&state, manifest) {
        (None, _) => ((Verdict::error(TickError::StateDir), Judged::Unjudged), Vec::new()),
        (Some(_), Err(errors)) => ((Verdict::error(TickError::NoRule), Judged::Unjudged), crate::rules::cli::render_defects(&errors)),
        (Some(state), Ok(manifest)) => {
            let input = Input { state, target: flags.target, socket: flags.socket, capture: flags.capture, manifest: &manifest };
            (judge(&input), Vec::new())
        }
    };
    if let Some(state) = &state {
        stamp_last(&state.path, flags.target, verdict.decision);
        // 局面の出力の部分の書き直し（出力の無い置き場は何もしない・返りは捨てる＝rc と字は変えない・case-lifecycle.md §13）。
        let _ = crate::fleet::lifecycle_partial::rewrite(&state.path);
    }
    let rc = if matches!(verdict.decision, TickDecision::Error(_)) { RC_REFUSED } else { RC_OK };
    Outcome { out: vec![format!("{} judged={}", render(flags.target, &verdict), judged.render())], err, rc }
}

/// 最後の周の打刻を書く（設計 §12 行 p 形 1・判定の後・rc 1 の周も・登録 row の在る席だけ＝row の無い target は dir も作らない・
/// 一時 file → rename・書けない周は判定行も rc も変えない・event log には書かない）。
fn stamp_last(state_dir: &Path, target: &str, decision: TickDecision) {
    let Ok(events) = crate::fleet::store::read_all(state_dir) else {
        return;
    };
    if super::role::registration_of_target(&crate::fleet::replay(&events), target).is_none() {
        return;
    }
    let seat = seat_dir(state_dir, target);
    let temporary = seat.join(format!("{TICK_LAST_FILE}.tmp"));
    let line = format!("ts={} decision={} reason={}\n", state::now_secs(), decision.as_str(), decision.reason());
    let _ = fs::create_dir_all(&seat).and_then(|()| fs::write(&temporary, line)).and_then(|()| fs::rename(&temporary, seat.join(TICK_LAST_FILE)));
}

/// 最後の周の打刻の 3 欄（`ts` / `decision` / `reason`）。
type Last = (u64, String, String);

/// 最後の周の打刻を読む（無い周は `Ok(None)`・在るのに読めない周〔dir・1 行の 3 欄でない〕は `Err`）。読み手は status・doctor・
/// `seat heartbeat status` の 1 本。
fn read_last(seat: &Path) -> Result<Option<Last>, ()> {
    let text = match fs::read_to_string(seat.join(TICK_LAST_FILE)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    let fields = match text.lines().collect::<Vec<&str>>().as_slice() {
        [line] => match line.split(' ').collect::<Vec<&str>>().as_slice() {
            [ts, decision, reason] => ts.strip_prefix("ts=").and_then(|secs| secs.parse::<u64>().ok()).zip(decision.strip_prefix("decision=")).zip(reason.strip_prefix("reason=")),
            _ => None,
        },
        _ => None,
    };
    fields.map(|((ts, decision), reason)| Some((ts, decision.to_owned(), reason.to_owned()))).ok_or(())
}

/// 最後の周の健全（**閉じた 4 値**・判じるのは読み手で tick は判じない・設計 §12 行 p 形 2 / 3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// 打刻が在り読めて、経過 ≤ 周期 × [`HEALTH_FACTOR`]。
    Healthy,
    /// 打刻が在り読めて、経過が越える。
    Stale,
    /// 打刻が無い。
    Absent,
    /// 打刻が在るのに読めない。
    Unreadable,
}

impl Health {
    /// doctor の `tick=` の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Stale => "stale",
            Self::Absent => "absent",
            Self::Unreadable => UNREADABLE,
        }
    }
}

/// 健全を判じる 1 関数（status と doctor の同じ 1 本）。
fn health_of(last: &Result<Option<Last>, ()>, interval_s: u64, now: u64) -> Health {
    match last {
        Ok(Some((ts, _, _))) if now.saturating_sub(*ts) <= interval_s.saturating_mul(HEALTH_FACTOR) => Health::Healthy,
        Ok(Some(_)) => Health::Stale,
        Ok(None) => Health::Absent,
        Err(()) => Health::Unreadable,
    }
}

/// 席の置き場の健全（doctor の `tick=` の読み手・時計は判定と同じ UTC 秒）。
pub fn health(seat: &Path, interval_s: u64) -> Health {
    health_of(&read_last(seat), interval_s, state::now_secs())
}

/// `seat tick status`（設計 §12 行 p 形 2・§20 形 4）: 登録 row の席ごとに 1 行（鍵の順・`target` は 1 席）。周期・梯子・退避の猶予の
/// 行のどれかを読めない周は `no-rule`・row の無い target は `no-row`（rc 1・stdout 0 行・既定を出さない）。記録は読むだけ。
pub fn status(state_dir: &str, target: Option<&str>, manifest: Result<Manifest, Vec<RuleError>>) -> Outcome {
    let named = target.map(|found| format!(" target={found}")).unwrap_or_default();
    let refused = |rc, reason: &str| Outcome::failed_line(rc, format!("seat tick status: refused reason={reason}{named}"));
    let Some(state) = state_dir_of(Some(state_dir)) else {
        return refused(RC_REFUSED, TickError::StateDir.as_str());
    };
    // 黙りの閾値は status が読まない（梯子の列だけを使う）ので 0 を置く。
    let rows = manifest.ok().and_then(|found| {
        let pace = Pace::of(0, list_row(&found, ROW_LADDER).ok()?).ok()?;
        Some((int_row(&found, ROW_INTERVAL).ok()?, pace, int_row(&found, ROW_GRACE).ok()?, found))
    });
    let Some((interval_s, pace, grace_s, found)) = rows else {
        return refused(RC_REFUSED, TickError::NoRule.as_str());
    };
    let Ok(events) = crate::fleet::store::read_all(&state.path) else {
        return refused(RC_BROKEN, TickError::Store.as_str());
    };
    let fleet = crate::fleet::replay(&events);
    let seats: Vec<&Registration> = match target.map(|found| super::role::registration_of_target(&fleet, found)) {
        Some(Some(row)) => vec![row],
        Some(None) => return refused(RC_REFUSED, NoopReason::NoRow.as_str()),
        None => fleet.registrations.values().map(|latest| &latest.registration).collect(),
    };
    // host の面は 1 回だけ合わせる（合わせられない周は全部の席の move= / grace_left= が unreadable）。
    let manifest = crate::rules::with_state_dir(found, Some(&state.path)).ok();
    let now = state::now_secs();
    let clock = (now, crate::fleet::cli::format_utc(now));
    let line = |row: &Registration| {
        let seat = seat_dir(&state.path, &row.target);
        let tail = status_tail(&seat, (&state.path, manifest.as_ref(), &fleet), row, (clock.0, &clock.1), grace_s);
        let beat = beat::resolve(&seat, &row.anchor, manifest.as_ref().map_or(beat::Table::Unreadable, beat::Table::Read));
        format!("{}{tail}", status_line(&seat, &row.target, (interval_s, &pace), (now, beat)))
    };
    Outcome::ok(seats.into_iter().map(line).collect())
}

/// status の行の末尾の 3 欄（設計 §20 形 1〜3・値は判定の 1 本の答えの写しだけ）: `reopens=` は選定（[`select`]）を登録 row の口座
/// 1 つ・席用・row の model・閾値 [`LIMIT_PCT`] で撃った結果、`move=` / `grace_left=` は移動の見立て（[`group::pending`]）と次の手
/// （[`group::step_of`]）。`manifest` は host の面を合わせた manifest（合わせられない周は `None`＝`unreadable`）。
fn status_tail(seat: &Path, read: (&Path, Option<&Manifest>, &State), row: &Registration, (now, utc): (u64, &str), grace_s: u64) -> String {
    let (state_dir, manifest, fleet) = read;
    let (labels, exclude, inflight) = ([row.account.clone()], BTreeSet::new(), BTreeMap::new());
    let (allowance, purpose, model) = (&fleet.allowance, Purpose::Session, row.model.as_deref());
    let input =
        SelectInput { labels: &labels, allowance, purpose, model, exclude: &exclude, inflight: &inflight, threshold_pct: LIMIT_PCT, now: utc, prefer: None };
    let reopens = match select(&input) {
        Selection::Chosen(_) => DASH.to_owned(),
        Selection::None(found) if found.limited.is_empty() => "unmeasured".to_owned(),
        Selection::None(found) => found.earliest_reset.unwrap_or_else(|| "unknown".to_owned()),
    };
    let (moved, left) = match manifest.map_or(Pending::Unreadable, |found| group::pending(found, state_dir, (&row.anchor, &row.account))) {
        Pending::None => (DASH.to_owned(), DASH.to_owned()),
        Pending::Unreadable => (UNREADABLE.to_owned(), UNREADABLE.to_owned()),
        Pending::Moving(_, current) => {
            let left = match group::step_of(seat, group::signal_key(&current), now, grace_s) {
                Step::Exit => "0".to_owned(),
                Step::Wait(left) => left.to_string(),
                Step::Signal => DASH.to_owned(),
            };
            (current.label, left)
        }
    };
    format!(" reopens={reopens} move={moved} grace_left={left}")
}

/// status の 1 行: `next=` は次の段（記録の段 + 1）の待ちの残り（判定行の `pointer=` と同じ [`pointer_of`]・列を越える段は `stopped`）。
fn status_line(seat: &Path, target: &str, (interval_s, pace): (u64, &Pace), (now, beat): (u64, beat::Beat)) -> String {
    let last = read_last(seat);
    let healthy = if health_of(&last, interval_s, now) == Health::Healthy { "yes" } else { "no" };
    let (ts, age) = match &last {
        Ok(Some((ts, _, _))) => (ts.to_string(), now.saturating_sub(*ts).to_string()),
        Ok(None) | Err(()) => (DASH.to_owned(), DASH.to_owned()),
    };
    let (step, next) = match read_ladder(seat) {
        Ok(Some(record)) => match pointer_of(pace, Some(record.sent_at), record.step.saturating_add(1), now) {
            Pointer::Wait(left) => (record.step.to_string(), left.to_string()),
            Pointer::Stopped => (record.step.to_string(), Pointer::Stopped.render(false)),
            Pointer::Open | Pointer::Settling => (record.step.to_string(), "0".to_owned()),
        },
        Ok(None) => (DASH.to_owned(), DASH.to_owned()),
        Err(_) => (UNREADABLE.to_owned(), UNREADABLE.to_owned()),
    };
    let (value, by) = (beat.value.as_str(), beat.by.as_str());
    format!("seat tick status: target={target} last={ts} age={age} healthy={healthy} heartbeat={value} heartbeat_by={by} step={step} next={next}")
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
    let mut parked = Judged::Unjudged;
    let found = match front(input, &mut parked) {
        Ok(found) => found,
        Err(stopped) => return (stopped, parked),
    };
    let judged = if parked == Judged::Unjudged { judged(input, &found.anchor, (&found.rows, found.now)) } else { parked };
    let moved = matches!(judged, Judged::Moved(_))
        .then(|| moving(input, (&found.anchor, &found.account), &found.seat, (&found.rows, found.now)))
        .flatten();
    (moved.map_or_else(|| back(input, &found), Ok).unwrap_or_else(|stopped| stopped), judged)
}

/// 群の判定の打刻の間隔の rules 行（計測の鮮度の行を流用・行を足さない・設計 §9 形 2）。
const ROW_FRESH: &str = "fleet.usage_fresh_s";

/// 群の判定（設計 §9 形 2 / 3）: 自席の anchor が群に属し、群の判定の打刻が `fleet.usage_fresh_s` より古い（か無い）周だけ、群の段
/// と同じ lock の内側で判定の 1 本を撃ち（他の群の今の口座 ∪ 記録を読めない群の候補を移り先から外し・鮮度に依らず測る口座と既に
/// 測った口座は空）、打刻を判定の時刻で書く。lock を取れない周は撃たない（列は今のまま）。断りの event を記した周だけ断りの 1 行を
/// 自席へ注入の経路で送る（入力欄の門を通らない周は落とす）。
fn judged(input: &Input, anchor: &str, (rows, now): (&Rows, u64)) -> Judged {
    let state_dir = input.state.path.as_path();
    let Ok(manifest) = crate::rules::with_state_dir(input.manifest.clone(), Some(state_dir)) else {
        return Judged::Error(NoopReason::GroupUnreadable.as_str());
    };
    let Some(found) = group_of(&manifest, anchor) else {
        return Judged::Unjudged;
    };
    let Ok(fresh_s) = int_row(&manifest, ROW_FRESH) else {
        return Judged::Error(TickError::NoRule.as_str());
    };
    let dir = host_groups_dir(state_dir);
    let stamp = group::judged_path(&dir, found.name());
    let last = fs::read_to_string(&stamp).ok().and_then(|text| text.trim().parse::<u64>().ok());
    if last.is_some_and(|ts| !aged(ts, now, fresh_s)) {
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
    let judge = group::Judge { state_dir, manifest: &manifest, group: found, head, taken, forced: &forced, caps: rows.caps, measure: &measure };
    let judgement = group::judge(&judge, &mut BTreeSet::new());
    let _ = fs::write(&stamp, format!("{now}\n"));
    match judgement {
        Judgement::Moved(label) => Judged::Moved(label),
        Judgement::Stay(_) => Judged::Stay,
        Judgement::NoCandidate(Refusal::Recorded) => {
            refused(input, found, rows.window_ms);
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

/// rules の行（行 3 本・送達の窓・退避の猶予・群の閾値）。
struct Rows {
    /// 梯子の形。
    pace: Pace,
    /// 送達の窓（ms）。
    window_ms: u64,
    /// 退避の猶予（秒・0 は猶予なし・設計 §13 形 1）。
    grace_s: u64,
    /// 口座の門の閾値。
    caps: Caps,
    /// 段の上げの閾値（秒・任意の行が無い・読めない周は `None`＝上げず `alarm=idle-unset`・設計 §17 形 1）。
    idle_alarm_s: Option<u64>,
    /// 事前審査の束の段の上げの閾値（秒・任意の行・読めない周は `None`＝上げず `alarm=precheck-unset`・dispatcher.md §27 形 4）。
    precheck_alarm_s: Option<u64>,
}

impl Rows {
    /// 全部を読む。どれかが読めない周は `Err`（既定値に倒さない・C1）。段の上げの 2 行だけは任意で、読めない周も `Err` にしない。
    fn of(manifest: &Manifest) -> Result<Self, String> {
        int_row(manifest, ROW_INTERVAL)?;
        let pace = Pace::of(int_row(manifest, ROW_STALE)?, list_row(manifest, ROW_LADDER)?)?;
        let (window_ms, grace_s) = (int_row(manifest, ROW_WINDOW)?, int_row(manifest, ROW_GRACE)?);
        let (idle_alarm_s, precheck_alarm_s) = (int_row(manifest, ROW_IDLE_ALARM).ok(), int_row(manifest, ROW_PRECHECK_ALARM).ok());
        Ok(Self { pace, window_ms, grace_s, caps: Caps::of(manifest)?, idle_alarm_s, precheck_alarm_s })
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
    /// 登録 row の役割（口座の門がモデル別窓を役割の model に絞る・account-lifecycle.md §33 形 3）。
    role: Role,
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
    /// 実効の値が off か unreadable（梯子の記録を読まない周・`back` の頭で止まる・設計 §22 形 4）。
    off: bool,
    /// 登録 row の口座が墓標（打刻と梯子を読まない周・`back` が `account-dead` で止まる・設計 §38 形 7）。
    dead: bool,
    /// 黙りの門の閾値（段を上げた周は短い・[`raise`]）。
    stale_s: u64,
    /// 合図の末尾（§16 の並列の実測と §17 の ` alarm=` の列・列が空の周は key を出さない）。
    tail: String,
}

/// 形 1 の 1〜3（登録 row → 窓が shell か〔§7 形 1〕→ 移動の周か〔§10 形 8〕→ 状態の打刻 → digest の比較）。窓が shell の周は
/// 打刻と梯子を読まず起こす周（[`awake`]）の判定で、移動の周（窓が claude）は打刻と梯子を読まず移動の門（[`moving`]）の判定で
/// 止まる（Busy の周も /exit を送る＝turn の途中の /exit は入力の列に積まれ turn の終わりで実行される・§10 形 9）。
fn front(input: &Input, judged: &mut Judged) -> Result<Front, Verdict> {
    let rows = Rows::of(input.manifest).map_err(|_| Verdict::error(TickError::NoRule))?;
    let now = state::now_secs();
    let events = crate::fleet::store::read_all(&input.state.path).map_err(|_| Verdict::error(TickError::Store))?;
    let fleet = crate::fleet::replay(&events);
    let (account, role, anchor) = super::role::registration_of_target(&fleet, input.target)
        .map(|found| (found.account.clone(), found.role, found.anchor.clone()))
        .ok_or_else(|| Verdict::noop(NoopReason::NoRow))?;
    let seat = seat_dir(&input.state.path, input.target);
    if pane_is_shell(input.socket, input.target) {
        return Err(awake(input, (&account, role, &anchor), &seat, (&fleet, (&rows, now), judged)));
    }
    if let Some(moved) = moving(input, (&anchor, &account), &seat, (&rows, now)) {
        return Err(moved);
    }
    if let Some(moved) = parking(input, &fleet, &seat, (&rows, now), judged) {
        return Err(moved);
    }
    let merged = crate::rules::with_state_dir(input.manifest.clone(), Some(&input.state.path));
    let off = beat::resolve(&seat, &anchor, merged.as_ref().map_or(beat::Table::Unreadable, beat::Table::Read)).silent();
    let dead = tombstone(&input.state.path, &account);
    let (stale_s, tail) = (rows.pace.stale_s, String::new());
    let front = Front { rows, fleet, account, role, anchor, seat, digest: 0, step: 0, pointer: Pointer::Open, now, off, dead, stale_s, tail };
    if dead { Ok(front) } else { ladder(input, front) }
}

/// 口座 `label` の credential が墓標か（読みは [`usage::credential_of`] の 1 本・毎周読む・設計 §38 形 1）。
fn tombstone(state_dir: &Path, label: &str) -> bool {
    usage::credential_of(&crate::fleet::account_dir(state_dir, label)) == usage::Credential::Dead
}

/// 形 1 の 3（状態の打刻 → 梯子の記録 → digest の比較）: 墓標でない周だけ読み、`front` の打刻と梯子の欄を埋める。
fn ladder(input: &Input, front: Front) -> Result<Front, Verdict> {
    let (rows, seat, now) = (&front.rows, &front.seat, front.now);
    let stamps = stamps_of(seat, rows.pace.stale_s, now, || input_gate(input).is_ok()).map_err(Verdict::noop)?;
    let digest = stamps.last().map_or(0, |stamp| stamp.ts);
    let record = if front.off { None } else { read_ladder(seat).map_err(Verdict::noop)? };
    let record = match record {
        Some(found) => Some(settled(seat, found, &stamps, digest, (now, rows.pace.stale_s))?),
        None => None,
    };
    // 並列の実測（設計 §16・列の結果なし＝`held=` を出さない・台帳も列も撃たない）で段を上げるかを決める（§17 形 2）。
    let measured = facts::facts(&input.state.path, None, now);
    // 局面の出力は置き場の出力・古さの印の file・event log の印だけ読む（比べる組は event log の 1 種類・台帳も git も撃たない・§24 約束 2）。
    let lifecycle = lifecycle_read::read(&input.state.path, &input.state.path, &[lifecycle_read::Input::Events]);
    let (alarm_s, words) = idle_alarm(rows, &measured, now, &lifecycle);
    let (stale_s, step) = raise(&rows.pace, candidate(record.as_ref(), digest), alarm_s);
    let pointer = pointer_of(&rows.pace, record.map(|found| found.sent_at), step, now);
    let alarm = if words.is_empty() { String::new() } else { format!(" alarm={}", words.join(",")) };
    let tail = format!("{}{alarm}", facts::line(&measured));
    Ok(Front { digest, step, pointer, stale_s, tail, ..front })
}

/// 窓が shell の周（設計 §7 形 1〜3）: 打刻と梯子を読まず、同じ target に席を起こす（[`wake`]）。口座は anchor が群に属せば群の
/// 今の口座（[`current_of`]・記録 > 種・群の段と同じ lock の内側）、属さなければ登録 row の口座（群 0 の host を含む・区画の席は区画の判定が移り先を返せばその口座・lock を取らない）。記録が
/// 在るのに読めない周と host の面が読めない周は `group-unreadable`・lock を取れない周は `group-locked`。起こし直しは打刻の最終行の
/// sid を `--resume` で運び初手の 1 語を積む（[`state::resume_carry`]・row の launch には載せない）。群の今の口座が墓標
/// （[`tombstone`]）なら lock を取る前に群の判定（[`judged`]）を撃って `judged` に載せ、今の口座を読み直して起こす（設計 §38 形 10）。
fn awake(
    input: &Input,
    (account, role, anchor): (&str, Role, &str),
    seat: &Path,
    (fleet, clock, judged): (&State, (&Rows, u64), &mut Judged),
) -> Verdict {
    let Ok(manifest) = crate::rules::with_state_dir(input.manifest.clone(), Some(&input.state.path)) else {
        return Verdict::noop(NoopReason::GroupUnreadable);
    };
    let carry = state::resume_carry(seat);
    let Some(group) = group_of(&manifest, anchor) else {
        let parked = parked_by(input, &manifest, fleet, judged);
        return wake(input, &manifest, (role, anchor), parked.as_ref().map_or(account, |(_, to)| to), &carry);
    };
    let Ok(mut current) = current_of(&input.state.path, group) else {
        return Verdict::noop(NoopReason::GroupUnreadable);
    };
    if tombstone(&input.state.path, &current.label) {
        *judged = self::judged(input, anchor, clock);
        let Ok(moved) = current_of(&input.state.path, group) else {
            return Verdict::noop(NoopReason::GroupUnreadable);
        };
        current = moved;
    }
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
/// 見立ては [`group::pending`]、次の手は [`group::step_of`] の 1 本ずつ（`seat tick status` と同じもの・設計 §20 形 2）。
/// 移動の周は 3 分岐（設計 §14 形 3・§18 形 2）: exit なら lock → `/exit`、wait なら `move=wait`（0 key・lock を取らない・file を
/// 書かない）、signal なら退避の合図の 1 行を同じ門で送り、送れた周だけ `at` = 周の始めの今（送る前）の記録を書く
/// （[`group::write_signal`]）。群の記録の ts の古さと種は分岐に入らない。
fn moving(input: &Input, (anchor, account): (&str, &str), seat: &Path, (rows, now): (&Rows, u64)) -> Option<Verdict> {
    let Ok(manifest) = crate::rules::with_state_dir(input.manifest.clone(), Some(&input.state.path)) else {
        return Some(Verdict::noop(NoopReason::GroupUnreadable));
    };
    let (group, current) = match group::pending(&manifest, &input.state.path, (anchor, account)) {
        Pending::None => return None,
        Pending::Unreadable => return Some(Verdict::noop(NoopReason::GroupUnreadable)),
        Pending::Moving(group, current) => (group, current),
    };
    let lock = Some(host_groups_dir(&input.state.path));
    Some(advance(input, seat, (group::signal_key(&current), (group.name(), &current.label)), (rows, now), lock))
}

/// 区画の席の移動の周（設計 §21 形 3）: 区画の判定（[`parked_by`]・1 周 1 回）が移り先を返した周だけ `Some`。鍵は（row の口座,
/// `park.<seq>`）で、次の手は群の移動の周と同じ [`advance`]（lock は取らない）。移り先でない周は今の列へ進む。
fn parking(input: &Input, fleet: &State, seat: &Path, (rows, now): (&Rows, u64), judged: &mut Judged) -> Option<Verdict> {
    let manifest = crate::rules::with_state_dir(input.manifest.clone(), Some(&input.state.path)).ok()?;
    let (latest, to) = parked_by(input, &manifest, fleet, judged)?;
    let (account, tag) = park::key(latest);
    let name = manifest.park()?.name();
    Some(advance(input, seat, ((&account, &tag), (name, &to)), (rows, now), None))
}

/// 区画の判定を自席の登録 row で 1 回撃ち、判定行の語を `judged` に載せる。移り先の口座を返した周だけ `Some`（登録 row と口座）。
fn parked_by<'a>(input: &Input, manifest: &Manifest, fleet: &'a State, judged: &mut Judged) -> Option<(&'a RegistrationLatest, String)> {
    let latest = park::latest_of(fleet, input.target)?;
    let found = park::judge(manifest, fleet, latest, &crate::fleet::cli::now_utc());
    *judged = Judged::of_park(&found);
    match found {
        park::Park::To(to) => Some((latest, to)),
        _ => None,
    }
}

/// 移動の周の次の手（[`group::step_of`] の 1 本・群の移動の周と区画の席の移動の周の同じ 3 分岐）: `key` は移動の鍵、`(name,
/// to)` は退避の合図の群の名と移り先。exit なら（`lock` の dir が在れば lock の内側で）`/exit`、wait なら `move=wait`、signal なら
/// 退避の合図の 1 行を同じ門で送り、送れた周だけ `at` = 周の始めの今（送る前）の記録を書く（[`group::write_signal`]）。
fn advance(input: &Input, seat: &Path, (key, (name, to)): ((&str, &str), (&str, &str)), (rows, now): (&Rows, u64), lock: Option<PathBuf>) -> Verdict {
    match group::step_of(seat, key, now, rows.grace_s) {
        Step::Exit => {
            let Ok(_lock) = lock.as_deref().map(Lock::take).transpose() else {
                return Verdict::noop(NoopReason::GroupLocked);
            };
            return evacuate(input, (EXIT, Move::Exit), rows.window_ms).0;
        }
        Step::Wait(_) => return Verdict::moved(Move::Wait, None, None),
        Step::Signal => {}
    }
    let payload = group::evacuate_line(name, to, rows.grace_s);
    let (verdict, delivered) = evacuate(input, (&payload, Move::Signal), rows.window_ms);
    if delivered {
        let _ = group::write_signal(seat, key, now);
    }
    verdict
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
        // 席の箱の行は `--rules` の写しか埋め込み（自分の manifest・設計 account-lifecycle.md §30 形 4）。
        seat_box: crate::pipe::confine::seat_box_of(manifest),
    });
    let trust = match &launched {
        Launched::Done(_, _, trust, _) => Some(trust.as_str()),
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

/// pane が shell でない移動の周（形 4）: 入力欄の門（[`input_gate`]＝§2 と同じ `pass_input`）を通し、空なら `payload`（`/exit`
/// か退避の合図）の 1 行を送る（[`deliver_or_confirm`]＝`deliver_within` と同じ門・送り・settle・窓は `pipe.stop_grace_ms`・送った
/// 周は未確認でも `tick.jsonl` に [`WHO_MOVE`] の 1 行）。門が人の文字（Foreign）で、その tail が dialog の既定の行の周だけ Enter を
/// 1 回送る。それ以外の Foreign / 特定できない入力欄 / 残る自席の文は今の語で止まる（OwnQueued の Enter は `pass_input` のまま）。
/// 2 つ目の値は送達を確認した周（`Delivered`）だけ真。
fn evacuate(input: &Input, (payload, step): (&str, Move), window_ms: u64) -> (Verdict, bool) {
    let foreign = match input_gate(input) {
        Ok(()) => false,
        Err(NoopReason::InputBusy) => true,
        Err(reason) => return (Verdict::noop(reason), false),
    };
    let request = Request { target: input.target, socket: input.socket, payload, state_dir: Some(input.state) };
    match deliver_or_confirm(&request, Duration::from_millis(window_ms), &exit_dialog(WHO_MOVE)) {
        inject::Sent::Confirmed(entered) => {
            let why = if entered { CONSUMED_EXIT_DIALOG } else { REASON_TMUX_FAILED };
            (Verdict::moved(Move::Enter, Some(Sent::Unconfirmed(why)), None), false)
        }
        inject::Sent::Payload(Delivery::Refused(_)) if foreign => (Verdict::noop(NoopReason::InputBusy), false),
        inject::Sent::Payload(delivery) => {
            let delivered = matches!(delivery, Delivery::Delivered(..));
            (Verdict::moved(step, Some(Sent::of(delivery)), None), delivered)
        }
    }
}

/// 形 1 の 4〜10（停止の記録の門〔§12 形 3〕→ 墓標の門〔account-lifecycle.md §38 形 8〕→ 黙りの門 → 上限 → 床 → 口座の門 → 入力欄の門 → 記録 → 注入）。
fn back(input: &Input, front: &Front) -> Result<Verdict, Verdict> {
    if front.off {
        return Err(Verdict::noop(NoopReason::HeartbeatOff));
    }
    if front.dead {
        return Err(Verdict::noop(NoopReason::AccountDead));
    }
    let at = |reason| Verdict::noop_at(reason, front.pointer, front.step);
    if !aged(front.digest, front.now, front.stale_s) {
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
    let payload = format!("{}{}", signal(front.step, &front.rows.pace), front.tail);
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

/// `seat heartbeat` の後ろの語（**閉じた 4 値**・設計 §12 形 2・§22 形 5・positional）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    /// 明示 off を置いて明示 on を消す。
    Off,
    /// 明示 off を消して明示 on を置く。
    On,
    /// 明示の記録を両方消す（群の表の行の key と種類の既定に戻す）。
    Default,
    /// 実効の値と決まり方と最後の周の打刻を 1 行で出す。
    Status,
}

/// [`Switch`] の全部（宣言順）。
pub const SWITCHES: &[Switch] = &[Switch::Off, Switch::On, Switch::Default, Switch::Status];

impl Switch {
    /// 引数の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Default => "default",
            Self::Status => "status",
        }
    }

    /// 字面から読む（4 語でなければ `None`）。
    pub fn parse(token: &str) -> Option<Self> {
        SWITCHES.iter().copied().find(|switch| switch.as_str() == token)
    }
}

/// `seat heartbeat off|on|default|status`（設計 §12 形 2・§22 形 5）: 登録 row の無い target は `no-row`（rc 1・席の置き場を作らない
/// ＝FR40）。off / on / default は明示の記録を書き換え（[`beat`]）、status は書かない。出力は実効の値と決まり方（host の面は state dir の
/// host の面の file から読む）で、status は後ろに最後の周の打刻（[`read_last`]・無い / 読めない周は `-`）。event log には書かない。
pub fn heartbeat(switch: Switch, state_dir: &str, target: &str) -> Outcome {
    let head = format!("seat heartbeat {}:", switch.as_str());
    let refused = |rc, reason: &str| Outcome::failed_line(rc, format!("{head} refused reason={reason} target={target}"));
    let Some(state) = state_dir_of(Some(state_dir)) else {
        return refused(RC_REFUSED, TickError::StateDir.as_str());
    };
    let Ok(events) = crate::fleet::store::read_all(&state.path) else {
        return refused(RC_BROKEN, TickError::Store.as_str());
    };
    let fleet = crate::fleet::replay(&events);
    let Some(row) = super::role::registration_of_target(&fleet, target) else {
        return refused(RC_REFUSED, NoopReason::NoRow.as_str());
    };
    let seat = seat_dir(&state.path, target);
    let written = match switch {
        Switch::Off => beat::set_off(&seat),
        Switch::On => beat::set_on(&seat),
        Switch::Default => beat::set_default(&seat),
        Switch::Status => Ok(()),
    };
    if written.is_err() {
        return refused(RC_BROKEN, NoopReason::RecordUnwritable.as_str());
    }
    let host = HostManifest::read(&crate::rules::host_manifest_path(&state.path));
    let table = match &host {
        HostManifest::Present(face) => beat::Table::Read(face),
        HostManifest::Absent => beat::Table::Absent,
        HostManifest::Unreadable(_) => beat::Table::Unreadable,
    };
    let found = beat::resolve(&seat, &row.anchor, table);
    let tail = match (switch, read_last(&seat)) {
        (Switch::Status, Ok(Some((ts, decision, reason)))) => format!(" last={ts} decision={decision} reason={reason}"),
        (Switch::Status, _) => format!(" last={DASH} decision={DASH} reason={DASH}"),
        (Switch::Off | Switch::On | Switch::Default, _) => String::new(),
    };
    Outcome::ok_line(format!("{head} target={target} heartbeat={} heartbeat_by={}{tail}", found.value.as_str(), found.by.as_str()))
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
/// 鮮度の外・読めない周は偽（門は正の証拠でだけ閉じる）。モデル別窓は自席の役割 1 つの model の窓だけを数える（役割の行が
/// 無い周は空の集合＝モデル別窓を数えない・account-lifecycle.md §33 形 3）。
fn account_pressed(manifest: &Manifest, front: &Front) -> bool {
    let models: BTreeSet<&str> = group::role_model(manifest, front.role).into_iter().collect();
    matches!(fresh_rows(manifest, &front.fleet, &front.account), Ok(Some(rows)) if pressed(&rows, front.rows.caps, &models).is_some())
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
#[path = "tick_tests.rs"]
mod tests;
// flip-check: moved s2-07l.738.37.8
