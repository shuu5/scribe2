//! 便と席の現在地を、追記だけの event log を replay して読む面（憲法 C3・設計 §3 / §4）。
//!
//! 状態は process の記憶でなく永続面に在る。各段は log を読んで現在地を得て、段の
//! 終わりに event を 1 件追記して終わる。字面との変換は wildcard 無しの `match` に
//! 閉じ、variant を足したら compile error になる形を保つ（C1 / C11）。
//!
//! 本 file は閉じた列挙と型の定義を持つ。行の読み書きは [`event`]、replay は [`replay`]、
//! 完了待ちは [`wait`] に在る（`s2-07l.260` で挙動不変に分割）。外の呼び手の path は再 export で保つ。

pub mod cli;
pub mod json_lite;
pub mod json_tree;
pub mod select;
pub mod store;
pub mod usage;
mod event;
mod replay;
mod wait;

pub use event::Event;
pub use replay::{account_dir, effective_accounts, replay, select_for_run, Run, RunSelect, Seat, State};
pub use wait::{ci_now, epoch_of, wait, CiRun, Completion, Timeout};

use crate::polarity::{OnFailure, Polarity, Timing};
use crate::seat::role::Role;
use json_lite::Value;

/// event log の schema 版。非互換な変更で上げる。
pub const SCHEMA: u64 = 1;

/// 起きたことの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// 便を起こした。
    RunCreated,
    /// 便が段を進んだ。
    RunStage,
    /// 便が終わった。
    RunDone,
    /// 便を止めた。
    RunStopped,
    /// 席を立てた。
    SeatSpawned,
    /// 席を畳んだ。
    SeatStopped,
    /// 承認を求めた。
    ApprovalRequested,
    /// 承認を受け取った。
    ApprovalReceived,
    /// runner が契約の不足を質問 record で返して止まった（detail = 質問の逐語・FR31）。
    QuestionRaised,
    /// 契約の所有者が回答を記帳した（detail = 回答の逐語・actor は machine・FR32）。
    QuestionAnswered,
    /// 1 口座 1 窓の残量を実測した（FR33・設計 fleet-usage.md §4）。**便に紐づかない**。
    AllowanceMeasured,
    /// 残量を読めなかった（理由つき・0 に読み替えない）。**便に紐づかない**。
    AllowanceUnmeasured,
    /// 席を役割に登録した（FR40・設計 seat-roles.md §2）。**便に紐づかない**。
    SeatRegistered,
    /// 口座を退役させた（FR58・設計 account-lifecycle.md §3・`account` = label）。**便に紐づかない**。
    AccountRetired,
    /// 退役させた口座を戻した（FR58・`account` = label）。**便に紐づかない**。
    AccountRestored,
    /// 列の介入の印（`first` / `hold` / `release`・設計 dispatcher.md §4・[`Shape::Mark`]）。**便に紐づかない**
    /// ——印は bead に付き、`bead` と typed な [`Mark`] が本体である（台帳の priority は書き換えない・C15）。
    DispatchMark,
}

/// [`EventKind`] の全 variant。
pub const KINDS: &[EventKind] = &[
    EventKind::RunCreated,
    EventKind::RunStage,
    EventKind::RunDone,
    EventKind::RunStopped,
    EventKind::SeatSpawned,
    EventKind::SeatStopped,
    EventKind::ApprovalRequested,
    EventKind::ApprovalReceived,
    EventKind::QuestionRaised,
    EventKind::QuestionAnswered,
    EventKind::AllowanceMeasured,
    EventKind::AllowanceUnmeasured,
    EventKind::SeatRegistered,
    EventKind::AccountRetired,
    EventKind::AccountRestored,
    EventKind::DispatchMark,
];

impl EventKind {
    /// JSON に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunCreated => "RunCreated",
            Self::RunStage => "RunStage",
            Self::RunDone => "RunDone",
            Self::RunStopped => "RunStopped",
            Self::SeatSpawned => "SeatSpawned",
            Self::SeatStopped => "SeatStopped",
            Self::ApprovalRequested => "ApprovalRequested",
            Self::ApprovalReceived => "ApprovalReceived",
            Self::QuestionRaised => "QuestionRaised",
            Self::QuestionAnswered => "QuestionAnswered",
            Self::AllowanceMeasured => "AllowanceMeasured",
            Self::AllowanceUnmeasured => "AllowanceUnmeasured",
            Self::SeatRegistered => "SeatRegistered",
            Self::AccountRetired => "AccountRetired",
            Self::AccountRestored => "AccountRestored",
            Self::DispatchMark => "DispatchMark",
        }
    }

    /// 字面から引く。未知なら `None`。
    pub fn parse(text: &str) -> Option<Self> {
        KINDS.iter().copied().find(|kind| kind.as_str() == text)
    }

    /// 既定の actor。人由来は承認の受理だけである（FR22 の計測面）。
    pub fn default_actor(self) -> &'static str {
        match self {
            Self::ApprovalReceived => ACTOR_HUMAN,
            Self::RunCreated
            | Self::RunStage
            | Self::RunDone
            | Self::RunStopped
            | Self::SeatSpawned
            | Self::SeatStopped
            | Self::ApprovalRequested
            | Self::QuestionRaised
            | Self::QuestionAnswered
            | Self::AllowanceMeasured
            | Self::AllowanceUnmeasured
            | Self::SeatRegistered
            | Self::AccountRetired
            | Self::AccountRestored
            | Self::DispatchMark => ACTOR_MACHINE,
        }
    }

    /// 行が持つ**本体の形**（[`Shape`]）。
    ///
    /// 網羅 `match` で持つのは、kind を足した便に「この行はどの field を持つか」を必ず決めさせるためで
    /// ある（既定を持つと、紐づかない行が幽霊の `run` を作る側へ黙って倒れる）。**本体や `account` の有無
    /// では見分けない**——`SeatSpawned` も任意 field として `account`（便を起こした口座・ADR-0027 §2.3）を
    /// 持ち、退役した役割の登録 row は本体を持たずに読まれる（[`Event::from_line`]）ので、本体で見分けると
    /// 口座つきの spawn や登録 row が幽霊の便に化ける。
    pub fn shape(self) -> Shape {
        match self {
            Self::RunCreated
            | Self::RunStage
            | Self::RunDone
            | Self::RunStopped
            | Self::SeatSpawned
            | Self::SeatStopped
            | Self::ApprovalRequested
            | Self::ApprovalReceived
            | Self::QuestionRaised
            | Self::QuestionAnswered => Shape::Run,
            Self::AllowanceMeasured | Self::AllowanceUnmeasured => Shape::Allowance,
            Self::SeatRegistered => Shape::Registration,
            Self::AccountRetired | Self::AccountRestored => Shape::Account,
            Self::DispatchMark => Shape::Mark,
        }
    }

    /// 口座残量の kind か（`run` / `bead` を**持たない**側・設計 fleet-usage.md §4）。
    ///
    /// 分類は [`Self::shape`] の 1 本から導く（網羅 `match` の 2 本目を持たない・憲法 C2）。
    pub fn is_allowance(self) -> bool {
        matches!(self.shape(), Shape::Allowance)
    }
}

/// event の 1 行が持つ**本体の形**（設計 fleet-event-log.md §3）。
///
/// 行の並び（[`Event::to_line`]）と、行が要る field（`event` の `Body::read`）は同じ [`EventKind::shape`]
/// の 1 本が決める。`s2-07l.345` で `is_account_lifecycle`（退役・戻しだけを名乗る述語）を置き換えた
/// ——列の印が「便に紐づかないが `bead` は持つ」4 つ目の形を要り、2 値の述語では表せない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `run` + `bead`（便に紐づく行）。
    Run,
    /// 口座残量の本体（`run` / `bead` を持たない・設計 fleet-usage.md §4）。
    Allowance,
    /// 席の登録の本体（設計 seat-roles.md §2）。
    Registration,
    /// 口座 label だけ（退役・戻し・設計 account-lifecycle.md §3）。
    Account,
    /// `bead` + 列の印（便に紐づかない・設計 dispatcher.md §4）。
    Mark,
}

/// [`Shape`] の全 variant（宣言順・`enum-slices` が集合完全性を測る）。
pub const SHAPES: &[Shape] = &[Shape::Run, Shape::Allowance, Shape::Registration, Shape::Account, Shape::Mark];

/// 列の介入の印（設計 dispatcher.md §4）と、列が起こした事実の印（§17）。**閉じた 4 値**で、
/// [`EventKind::DispatchMark`] の行だけが持つ。
///
/// 印は一時の順序であって契約の性質ではないので、台帳の priority を書き換えない（台帳が持つのは task と
/// 裁定だけ・憲法 C15・設計 dispatcher.md §10）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// 次の 1 周で priority より先に評価する。
    First,
    /// 列には載せるが起こさない。
    Hold,
    /// 印を外して既定の順に戻す。
    Release,
    /// 列がこの bead の便を起こした（子を起こす**前**に書く・設計 dispatcher.md §17）。その後に
    /// `RunCreated` も `release` も無い bead は起こし直さない。
    Launched,
}

/// [`Mark`] の全 variant（宣言順・`enum-slices` が集合完全性を測る）。
pub const MARKS: &[Mark] = &[Mark::First, Mark::Hold, Mark::Release, Mark::Launched];

impl Mark {
    /// JSON の `mark` と `dispatch ls` に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Hold => "hold",
            Self::Release => "release",
            Self::Launched => "launched",
        }
    }

    /// 字面から引く。未知なら `None`（書き側と読み側で**同じ判定**を使う）。
    pub fn parse(text: &str) -> Option<Self> {
        MARKS.iter().copied().find(|mark| mark.as_str() == text)
    }
}

/// 機械が起こした event の actor。
pub const ACTOR_MACHINE: &str = "machine";
/// 人が起こした event の actor。
pub const ACTOR_HUMAN: &str = "human";

/// actor の字面を受理する。未知なら `None`。
///
/// **書き側と読み側で同じ判定を使う**。書き側が緩いと、読めない行が append-only の
/// log に残り、その置き場の `show` / `export` が以後ずっと rc 2 になる（行の削除は
/// 契約の射程外なので回復できない）。
pub fn parse_actor(text: &str) -> Option<&'static str> {
    if text == ACTOR_MACHINE {
        Some(ACTOR_MACHINE)
    } else if text == ACTOR_HUMAN {
        Some(ACTOR_HUMAN)
    } else {
        None
    }
}

/// 便の段。遷移は pipeline 側が持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 取り込み。
    Intake,
    /// 契約の審査を通した（verdict は `review.json` と detail・PASS だけが spawn へ進む・FR49・設計
    /// contract-source.md §4）。
    Reviewed,
    /// 人の承認待ちで止まっている。
    Blocked,
    /// 席を立てた。
    Spawned,
    /// runner が質問で止まり、回答を待っている（FR31）。
    Questioned,
    /// runner が上限 record で止まった（FR35・**終端でない**＝worktree と commit を保ち、別口座で
    /// 起こし直せる段・ADR-0020 §2.1）。
    RateLimited,
    /// 実装が済んだ。
    Implemented,
    /// gate を通した。
    Gated,
    /// land した。
    Landed,
    /// 止めた。
    Stopped,
    /// 落ちた。
    Failed,
}

/// [`Stage`] の全 variant。
pub const STAGES: &[Stage] = &[
    Stage::Intake,
    Stage::Reviewed,
    Stage::Blocked,
    Stage::Spawned,
    Stage::Questioned,
    Stage::RateLimited,
    Stage::Implemented,
    Stage::Gated,
    Stage::Landed,
    Stage::Stopped,
    Stage::Failed,
];

impl Stage {
    /// JSON に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intake => "Intake",
            Self::Reviewed => "Reviewed",
            Self::Blocked => "Blocked",
            Self::Spawned => "Spawned",
            Self::Questioned => "Questioned",
            Self::RateLimited => "RateLimited",
            Self::Implemented => "Implemented",
            Self::Gated => "Gated",
            Self::Landed => "Landed",
            Self::Stopped => "Stopped",
            Self::Failed => "Failed",
        }
    }

    /// 字面から引く。未知なら `None`。
    pub fn parse(text: &str) -> Option<Self> {
        STAGES.iter().copied().find(|stage| stage.as_str() == text)
    }
}

/// 席の状態。**bool で持たない**（C3.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatState {
    /// 生きている。
    Live,
    /// 畳んだ。
    Stopped,
}

impl SeatState {
    /// 表示に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "Live",
            Self::Stopped => "Stopped",
        }
    }
}

/// 残量を測る窓（設計 fleet-usage.md §3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WindowKind {
    /// 5 時間窓。
    FiveHour,
    /// 7 日窓。
    SevenDay,
    /// モデル別の 7 日窓（`model` を伴う）。
    SevenDayModel,
}

/// [`WindowKind`] の全 variant。
pub const WINDOWS: &[WindowKind] = &[
    WindowKind::FiveHour,
    WindowKind::SevenDay,
    WindowKind::SevenDayModel,
];

impl WindowKind {
    /// JSON に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FiveHour => "five_hour",
            Self::SevenDay => "seven_day",
            Self::SevenDayModel => "seven_day_model",
        }
    }

    /// 字面から引く。未知なら `None`。
    pub fn parse(text: &str) -> Option<Self> {
        WINDOWS.iter().copied().find(|found| found.as_str() == text)
    }
}

/// 残量を読めなかった理由（設計 fleet-usage.md §6）。**閉じた enum**である。
///
/// 理由を自由文で持たないのは、「測れなかった」の集合を育てる面が字面の揺れで数えられなく
/// なるのを塞ぐためである（新しい失敗の形は variant を足す＝網羅 `match` が手を入れさせる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnmeasuredReason {
    /// credential の置き場が無い。
    NoCredentials,
    /// credential に token が無い。
    NoToken,
    /// 置き場が墓標（使わない印）である。
    Tombstone,
    /// token の期限が切れている。
    TokenExpired,
    /// HTTP client の実行 file が無い。
    ClientMissing,
    /// HTTP client が非 0 で終わった。
    ClientFailed,
    /// HTTP status が 200 でない。
    HttpStatus,
    /// 期限までに応答が来なかった。
    Timeout,
    /// 本文を JSON として読めない。
    BodyUnreadable,
    /// 本文の形が想定と違う（窓・使用率・reset のどれかが引けない）。
    ShapeMismatch,
}

/// [`UnmeasuredReason`] の全 variant。
pub const REASONS: &[UnmeasuredReason] = &[
    UnmeasuredReason::NoCredentials,
    UnmeasuredReason::NoToken,
    UnmeasuredReason::Tombstone,
    UnmeasuredReason::TokenExpired,
    UnmeasuredReason::ClientMissing,
    UnmeasuredReason::ClientFailed,
    UnmeasuredReason::HttpStatus,
    UnmeasuredReason::Timeout,
    UnmeasuredReason::BodyUnreadable,
    UnmeasuredReason::ShapeMismatch,
];

impl UnmeasuredReason {
    /// この境界の極性（設計 fleet-usage.md §6）: 口座 × 窓の読みが失敗した周は**行として
    /// 記録して続行する**（計測は行為を止めない）。
    ///
    /// **Guard ではない**——編集・起動・merge・書込を止めうる判定ではないので、極性一覧
    /// （[`crate::polarity::ALL`]）の母集団には載らない。その除外を持つのはこの散文ではなく
    /// [`crate::polarity::NOT_A_GUARD`] で、`xtask polarity-sites` が site と網羅 match を両方向で
    /// 突き合わせる（`s2-07l.177`）。「0 に読み替えない」は極性ではなく行の構造
    /// （`AllowanceUnmeasured` に `used_pct` が在れば malformed）が守る。
    pub const POLARITY: Polarity = Polarity {
        timing: Timing::InLoop,
        on_failure: OnFailure::FailOpen,
    };

    /// JSON に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoCredentials => "no_credentials",
            Self::NoToken => "no_token",
            Self::Tombstone => "tombstone",
            Self::TokenExpired => "token_expired",
            Self::ClientMissing => "client_missing",
            Self::ClientFailed => "client_failed",
            Self::HttpStatus => "http_status",
            Self::Timeout => "timeout",
            Self::BodyUnreadable => "body_unreadable",
            Self::ShapeMismatch => "shape_mismatch",
        }
    }

    /// 字面から引く。未知なら `None`。
    pub fn parse(text: &str) -> Option<Self> {
        REASONS.iter().copied().find(|found| found.as_str() == text)
    }
}

/// 1 口座 1 窓の実測。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measured {
    /// 口座の不透明な label。
    pub account: String,
    /// どの窓か。
    pub window: WindowKind,
    /// モデル名（`seven_day_model` では必須）。
    pub model: Option<String>,
    /// 聞き先の短い識別子。
    pub endpoint: String,
    /// 使用率（整数 %・切り捨て・**100 で cap しない**）。
    pub used_pct: u64,
    /// 窓が開き直る時刻。`None` = 消費の無い窓（reset 未定・`used_pct` 0 の周に限る・ADR-0024 §2.1）。
    pub resets_at: Option<String>,
}

/// 読めなかった 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmeasured {
    /// 口座の不透明な label。
    pub account: String,
    /// 窓が分かっているなら（口座単位の失敗では `None`）。
    pub window: Option<WindowKind>,
    /// モデル名（要素単位の失敗のとき）。
    pub model: Option<String>,
    /// 聞き先の短い識別子。
    pub endpoint: String,
    /// なぜ読めなかったか。
    pub reason: UnmeasuredReason,
}

/// 口座残量の行の本体。
///
/// **`used_pct` は [`Measured`] にしか在り得ない**——「測れなかった」を使用率 0 として
/// 持てる型を作らない（FR33 の「0 に読み替えない」を型で守る・設計 §9 の却下案）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Allowance {
    /// 実測できた。
    Measured(Measured),
    /// 読めなかった。
    Unmeasured(Unmeasured),
}

impl Allowance {
    /// 最新を引く key（口座 × 窓 × model）。
    pub fn key(&self) -> AllowanceKey {
        match self {
            Self::Measured(found) => AllowanceKey {
                account: found.account.clone(),
                window: Some(found.window),
                model: found.model.clone(),
            },
            Self::Unmeasured(found) => AllowanceKey {
                account: found.account.clone(),
                window: found.window,
                model: found.model.clone(),
            },
        }
    }

    /// 行へ書く key/value（`run` / `bead` の代わりに並ぶ）。
    fn pairs(&self) -> Vec<(&'static str, Value)> {
        match self {
            Self::Measured(found) => {
                let mut pairs = window_pairs(&found.account, Some(found.window), &found.model);
                pairs.push(("endpoint", Value::Str(found.endpoint.clone())));
                pairs.push(("used_pct", Value::Num(found.used_pct)));
                if let Some(resets_at) = &found.resets_at {
                    pairs.push(("resets_at", Value::Str(resets_at.clone())));
                }
                pairs
            }
            Self::Unmeasured(found) => {
                let mut pairs = window_pairs(&found.account, found.window, &found.model);
                pairs.push(("endpoint", Value::Str(found.endpoint.clone())));
                pairs.push(("reason", Value::Str(found.reason.as_str().to_owned())));
                pairs
            }
        }
    }
}

/// `account` と（在れば）`window` / `model` を並べる。
fn window_pairs(
    account: &str,
    window: Option<WindowKind>,
    model: &Option<String>,
) -> Vec<(&'static str, Value)> {
    let mut pairs = vec![("account", Value::Str(account.to_owned()))];
    if let Some(window) = window {
        pairs.push(("window", Value::Str(window.as_str().to_owned())));
    }
    if let Some(model) = model {
        pairs.push(("model", Value::Str(model.clone())));
    }
    pairs
}

/// 口座残量の最新を引く key。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AllowanceKey {
    /// 口座の不透明な label。
    pub account: String,
    /// どの窓か（口座単位の失敗では `None`）。
    pub window: Option<WindowKind>,
    /// モデル名（`seven_day_model` の枠を分ける）。
    pub model: Option<String>,
}

/// 1 枠の最新の 1 行。**Measured / Unmeasured のどちらでも物理順で後が勝つ**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowanceLatest {
    /// その行の時刻。
    pub ts: String,
    /// 実測か、測れなかったか。
    pub allowance: Allowance,
}

/// 席の登録の行の本体（設計 seat-roles.md §2）: 鍵 = `role` × `anchor`（repo の root・hook の cwd と突合しない）、
/// 項目 = `target`（`session:window`）/ `sid`（任意・`seat register` は打刻から解いた `Some`・`seat launch` が書く row は
/// `None`＝session の側の証拠を持たない・account-lifecycle.md §4）/ `account` / `launch`（雛形の本文）/ `model`（任意・
/// 席が使う model の display name・契約 (e)・無い row / 旧 row は `None`＝逼迫度は保守側）。**pane id は持たない**。
/// `sid` の `None` は key の省略か `null` で読み、書く側は key ごと書かない（schema 1 のまま・既存 row は読める）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub role: Role,
    pub anchor: String,
    pub target: String,
    pub sid: Option<String>,
    pub account: String,
    pub launch: String,
    pub model: Option<String>,
}

/// 鍵ごとの最新の登録。`seq` は log の物理順（0 始まり）で、複数の鍵が同じ target なら大きい方が勝つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationLatest {
    pub seq: usize,
    pub registration: Registration,
}
