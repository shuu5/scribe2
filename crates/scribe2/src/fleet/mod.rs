//! 便と席の現在地を、追記だけの event log を replay して読む面（憲法 C3・設計 §3 / §4）。
//!
//! 状態は process の記憶でなく永続面に在る。各段は log を読んで現在地を得て、段の
//! 終わりに event を 1 件追記して終わる。字面との変換は wildcard 無しの `match` に
//! 閉じ、variant を足したら compile error になる形を保つ（C1 / C11）。

pub mod cli;
pub mod json_lite;
pub mod json_tree;
pub mod store;
pub mod usage;

use crate::polarity::{OnFailure, Polarity, Timing};
use json_lite::Value;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// event log の schema 版。非互換な変更で上げる。
pub const SCHEMA: u64 = 1;

/// event の 1 行が持てる key の全体（設計 §3）。
///
/// 未知 key を受理すると、綴り違いの field が黙って捨てられる（`stgae` と書いた行が
/// 段の無い行として通る）。設計 §3 の「それ以外の形は error」に合わせて拒む。
const KNOWN_KEYS: &[&str] = &[
    "schema", "ts", "kind", "run", "bead", "host", "actor", "stage", "seat", "pid", "detail",
    "account", "window", "model", "endpoint", "used_pct", "resets_at", "reason",
];

/// 口座残量の kind だけが持てる key（設計 fleet-usage.md §4）。
///
/// 既存 kind の行にこれが在れば malformed である。口座の field を持った `RunStage` の行を
/// 通すと、`run` を持つ行と持たない行の区別が kind から読めなくなる。
const ALLOWANCE_KEYS: &[&str] = &[
    "account",
    "window",
    "model",
    "endpoint",
    "used_pct",
    "resets_at",
    "reason",
];

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
            | Self::AllowanceUnmeasured => ACTOR_MACHINE,
        }
    }

    /// 口座残量の kind か（`run` / `bead` を**持たない**側・設計 fleet-usage.md §4）。
    ///
    /// 網羅 `match` で持つのは、kind を足した便に「この行は便に紐づくか」を必ず決めさせる
    /// ためである（既定を持つと、紐づかない行が幽霊の `run` を作る側へ黙って倒れる）。
    pub fn is_allowance(self) -> bool {
        match self {
            Self::AllowanceMeasured | Self::AllowanceUnmeasured => true,
            Self::RunCreated
            | Self::RunStage
            | Self::RunDone
            | Self::RunStopped
            | Self::SeatSpawned
            | Self::SeatStopped
            | Self::ApprovalRequested
            | Self::ApprovalReceived
            | Self::QuestionRaised
            | Self::QuestionAnswered => false,
        }
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
    /// 人の承認待ちで止まっている。
    Blocked,
    /// 席を立てた。
    Spawned,
    /// runner が質問で止まり、回答を待っている（FR31）。
    Questioned,
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
    Stage::Blocked,
    Stage::Spawned,
    Stage::Questioned,
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
            Self::Blocked => "Blocked",
            Self::Spawned => "Spawned",
            Self::Questioned => "Questioned",
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
    /// （[`crate::polarity::ALL`]）の母集団には載らない。「0 に読み替えない」は極性ではなく
    /// 行の構造（`AllowanceUnmeasured` に `used_pct` が在れば malformed）が守る。
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
    /// 窓が開き直る時刻。
    pub resets_at: String,
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
                pairs.push(("resets_at", Value::Str(found.resets_at.clone())));
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

/// log の 1 行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// schema 版。
    pub schema: u64,
    /// UTC の時刻（`YYYY-MM-DDTHH:MM:SSZ`）。
    pub ts: String,
    /// 種類。
    pub kind: EventKind,
    /// 便 id。**口座残量の kind では空**で、行にも書かない（[`EventKind::is_allowance`]）。
    pub run: String,
    /// 契約の bead id（台帳は読まない・文字列として持つだけ）。`run` と同じく口座残量の
    /// kind では空である。
    pub bead: String,
    /// host 名（C3 の host 列）。
    pub host: String,
    /// `machine` か `human`。
    pub actor: String,
    /// 段（任意）。
    pub stage: Option<Stage>,
    /// 席 id（任意）。
    pub seat: Option<String>,
    /// runner の pid（任意）。
    pub pid: Option<u64>,
    /// 自由文（任意）。
    pub detail: Option<String>,
    /// 口座残量の本体（口座残量の kind でだけ `Some`）。
    pub allowance: Option<Allowance>,
}

impl Event {
    /// 1 行の JSON にする。
    ///
    /// 本体は kind ではなく [`Self::allowance`] の有無が決める（`run` / `bead` を持つ行と
    /// 口座残量の行は同じ並びを共有しない）。食い違った組は [`Self::from_line`] が読み返せず
    /// malformed になるので、書いた行が読めない形は歯で捕まる。
    pub fn to_line(&self) -> String {
        let mut pairs: Vec<(&str, Value)> = vec![
            ("schema", Value::Num(self.schema)),
            ("ts", Value::Str(self.ts.clone())),
            ("kind", Value::Str(self.kind.as_str().to_owned())),
        ];
        match &self.allowance {
            None => {
                pairs.push(("run", Value::Str(self.run.clone())));
                pairs.push(("bead", Value::Str(self.bead.clone())));
            }
            Some(allowance) => pairs.extend(allowance.pairs()),
        }
        pairs.push(("host", Value::Str(self.host.clone())));
        pairs.push(("actor", Value::Str(self.actor.clone())));
        if let Some(stage) = self.stage {
            pairs.push(("stage", Value::Str(stage.as_str().to_owned())));
        }
        if let Some(seat) = &self.seat {
            pairs.push(("seat", Value::Str(seat.clone())));
        }
        if let Some(pid) = self.pid {
            pairs.push(("pid", Value::Num(pid)));
        }
        if let Some(detail) = &self.detail {
            pairs.push(("detail", Value::Str(detail.clone())));
        }
        json_lite::write_object(&pairs)
    }

    /// 1 行の JSON から読む。欠けや未知の値は理由つきで `Err`。
    ///
    /// **key の有無と値の型の両方**を見る。`as_str` / `as_num` の `None` を「無い」と読んで
    /// よいのは**任意 field が key ごと無い**周だけで、key が在って型が違えば malformed に
    /// する（型不一致を `None` に落とすと、`reason` が数の行が Ok で通り、読めた顔をして
    /// 中身の無い行が replay に届く・NFR4「黙って落とす 0 件」）。
    pub fn from_line(line: &str) -> Result<Self, String> {
        let pairs = json_lite::parse_object(line)?;
        for (key, _) in &pairs {
            if !KNOWN_KEYS.contains(&key.as_str()) {
                return Err(format!("未知の key {key}"));
            }
        }
        let schema = field(&pairs, "schema")
            .and_then(Value::as_num)
            .ok_or("schema が無い")?;
        if schema != SCHEMA {
            return Err(format!("schema が {SCHEMA} でない（実 {schema}）"));
        }
        let kind_text = text_of(field(&pairs, "kind"), "kind")?;
        let kind = EventKind::parse(&kind_text).ok_or(format!("kind {kind_text} は未知である"))?;
        let actor = text_of(field(&pairs, "actor"), "actor")?;
        let actor = parse_actor(&actor)
            .ok_or(format!("actor {actor} は machine でも human でもない"))?
            .to_owned();
        let body = Body::read(&pairs, kind)?;
        Ok(Self {
            schema,
            ts: text_of(field(&pairs, "ts"), "ts")?,
            kind,
            run: body.run,
            bead: body.bead,
            host: text_of(field(&pairs, "host"), "host")?,
            actor,
            stage: optional_stage(field(&pairs, "stage"))?,
            seat: optional_text(field(&pairs, "seat"), "seat")?,
            pid: optional_num(field(&pairs, "pid"), "pid")?,
            detail: optional_text(field(&pairs, "detail"), "detail")?,
            allowance: body.allowance,
        })
    }
}

/// kind ごとに違う本体（`run` / `bead` を持つ行か、口座残量の行か）。
struct Body {
    /// 便 id（口座残量の行では空）。
    run: String,
    /// bead id（口座残量の行では空）。
    bead: String,
    /// 口座残量の本体。
    allowance: Option<Allowance>,
}

impl Body {
    /// kind ごとの必須 field を**網羅 `match`** で読む。
    ///
    /// kind を足した便は、その kind の行がどの field を要るかをここで必ず決める。
    fn read(pairs: &[(String, Value)], kind: EventKind) -> Result<Self, String> {
        match kind {
            EventKind::AllowanceMeasured => {
                Self::allowance(pairs, Allowance::Measured(measured_of(pairs)?))
            }
            EventKind::AllowanceUnmeasured => {
                Self::allowance(pairs, Allowance::Unmeasured(unmeasured_of(pairs)?))
            }
            EventKind::RunCreated
            | EventKind::RunStage
            | EventKind::RunDone
            | EventKind::RunStopped
            | EventKind::SeatSpawned
            | EventKind::SeatStopped
            | EventKind::ApprovalRequested
            | EventKind::ApprovalReceived
            | EventKind::QuestionRaised
            | EventKind::QuestionAnswered => {
                for key in ALLOWANCE_KEYS {
                    absent(field(pairs, key), key)?;
                }
                Ok(Self {
                    run: text_of(field(pairs, "run"), "run")?,
                    bead: text_of(field(pairs, "bead"), "bead")?,
                    allowance: None,
                })
            }
        }
    }

    /// 口座残量の行の本体。`run` / `bead` は**持たない**（在れば malformed）。
    fn allowance(pairs: &[(String, Value)], allowance: Allowance) -> Result<Self, String> {
        absent(field(pairs, "run"), "run")?;
        absent(field(pairs, "bead"), "bead")?;
        Ok(Self {
            run: String::new(),
            bead: String::new(),
            allowance: Some(allowance),
        })
    }
}

/// `AllowanceMeasured` の field を読む。`seven_day_model` の行は `model` も必須。
fn measured_of(pairs: &[(String, Value)]) -> Result<Measured, String> {
    absent(field(pairs, "reason"), "reason")?;
    let window = window_of(field(pairs, "window"))?;
    let model = optional_text(field(pairs, "model"), "model")?;
    if window == WindowKind::SevenDayModel && model.is_none() {
        return Err(format!("window {} の行に model が無い", window.as_str()));
    }
    Ok(Measured {
        account: text_of(field(pairs, "account"), "account")?,
        window,
        model,
        endpoint: text_of(field(pairs, "endpoint"), "endpoint")?,
        used_pct: num_of(field(pairs, "used_pct"), "used_pct")?,
        resets_at: text_of(field(pairs, "resets_at"), "resets_at")?,
    })
}

/// `AllowanceUnmeasured` の field を読む。
///
/// **`used_pct` と `resets_at` は持てない**——「測れなかった」に使用率や reset を添えられる
/// 形を作らないためである（0 の捏造を構造で拒む・設計 fleet-usage.md §4）。
fn unmeasured_of(pairs: &[(String, Value)]) -> Result<Unmeasured, String> {
    absent(field(pairs, "used_pct"), "used_pct")?;
    absent(field(pairs, "resets_at"), "resets_at")?;
    Ok(Unmeasured {
        account: text_of(field(pairs, "account"), "account")?,
        window: optional_window(field(pairs, "window"))?,
        model: optional_text(field(pairs, "model"), "model")?,
        endpoint: text_of(field(pairs, "endpoint"), "endpoint")?,
        reason: reason_of(field(pairs, "reason"))?,
    })
}

/// key 1 つを引く。無ければ `None`。
fn field<'a>(pairs: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    pairs
        .iter()
        .find(|(found, _)| found == key)
        .map(|(_, value)| value)
}

/// 必須の文字列 field を取り出す。
fn text_of(value: Option<&Value>, key: &str) -> Result<String, String> {
    value
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(format!("{key} が無いか文字列でない"))
}

/// 任意の文字列 field。**key が在って文字列でなければ `Err`**。
fn optional_text(value: Option<&Value>, key: &str) -> Result<Option<String>, String> {
    match value {
        None => Ok(None),
        Some(found) => found
            .as_str()
            .map(|text| Some(text.to_owned()))
            .ok_or(format!("{key} が文字列でない")),
    }
}

/// 必須の整数 field。
fn num_of(value: Option<&Value>, key: &str) -> Result<u64, String> {
    value
        .and_then(Value::as_num)
        .ok_or(format!("{key} が無いか整数でない"))
}

/// 任意の整数 field。**key が在って整数でなければ `Err`**。
fn optional_num(value: Option<&Value>, key: &str) -> Result<Option<u64>, String> {
    match value {
        None => Ok(None),
        Some(found) => found.as_num().map(Some).ok_or(format!("{key} が整数でない")),
    }
}

/// key が在ってはならない field。在れば理由つきで `Err`。
fn absent(value: Option<&Value>, key: &str) -> Result<(), String> {
    match value {
        None => Ok(()),
        Some(_) => Err(format!("この kind の行は {key} を持たない")),
    }
}

/// 必須の `window`。字面が [`WindowKind`] に無ければ `Err`。
fn window_of(value: Option<&Value>) -> Result<WindowKind, String> {
    let text = text_of(value, "window")?;
    WindowKind::parse(&text).ok_or(format!("window {text} は未知である"))
}

/// 任意の `window`。key が在れば字面まで見る。
fn optional_window(value: Option<&Value>) -> Result<Option<WindowKind>, String> {
    match value {
        None => Ok(None),
        Some(_) => window_of(value).map(Some),
    }
}

/// 必須の `reason`。字面が [`UnmeasuredReason`] に無ければ `Err`。
fn reason_of(value: Option<&Value>) -> Result<UnmeasuredReason, String> {
    let text = text_of(value, "reason")?;
    UnmeasuredReason::parse(&text).ok_or(format!("reason {text} は未知である"))
}

/// 任意の `stage` を読む。字面が未知なら `Err`（黙って落とさない）。
fn optional_stage(value: Option<&Value>) -> Result<Option<Stage>, String> {
    match optional_text(value, "stage")? {
        None => Ok(None),
        Some(text) => Stage::parse(&text)
            .map(Some)
            .ok_or(format!("stage {text} は未知である")),
    }
}

/// 便の現在地。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// 便 id。
    pub id: String,
    /// 契約の bead id。
    pub bead: String,
    /// 物理順で最後に見た段。
    pub stage: Stage,
    /// 最後に触れた時刻。
    pub updated: String,
    /// 最後に見た自由文。
    pub detail: Option<String>,
    /// 承認 event が在るか（導出値・状態 enum ではない）。
    pub approved: bool,
}

/// 席の現在地。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    /// 席 id。
    pub id: String,
    /// 紐づく便 id。
    pub run: String,
    /// runner の pid。
    pub pid: Option<u64>,
    /// 生きているか畳んだか。
    pub state: SeatState,
    /// 最後に触れた時刻。
    pub updated: String,
}

/// replay で得た現在地の全体。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    /// 便 id → 現在地。
    pub runs: BTreeMap<String, Run>,
    /// 席 id → 現在地。
    pub seats: BTreeMap<String, Seat>,
    /// 口座 × 窓 × model → 最新の残量の行（設計 fleet-usage.md §4）。
    pub allowance: BTreeMap<AllowanceKey, AllowanceLatest>,
}

/// event の並びから現在地を導く。物理順で後の event が勝つ。
pub fn replay(events: &[Event]) -> State {
    let mut state = State::default();
    for event in events {
        apply_run(&mut state, event);
        apply_seat(&mut state, event);
        apply_allowance(&mut state, event);
    }
    state
}

/// 1 件の event を口座残量へ反映する。
///
/// **読めた行を捨てる経路を持たない**——本体が在れば必ずその枠の最新になる（捨てるべき行は
/// [`Event::from_line`] が読みの段で `Err` にしており、ここへは届かない）。捨てる枝を残すと、
/// 型不一致の Unmeasured が黙って落ちて**古い実測が「最新」を名乗る**。
fn apply_allowance(state: &mut State, event: &Event) {
    let Some(allowance) = &event.allowance else {
        return;
    };
    state.allowance.insert(
        allowance.key(),
        AllowanceLatest {
            ts: event.ts.clone(),
            allowance: allowance.clone(),
        },
    );
}

/// 1 件の event を便へ反映する。
fn apply_run(state: &mut State, event: &Event) {
    // 口座残量の行は便に紐づかない（`run` / `bead` を持たない）。ここで通すと id が空の
    // 幽霊の便が 1 つ生まれ、`show` / `export` の件数が実在しない便を数える。
    if event.kind.is_allowance() {
        return;
    }
    let run = state.runs.entry(event.run.clone()).or_insert_with(|| Run {
        id: event.run.clone(),
        bead: event.bead.clone(),
        stage: Stage::Intake,
        updated: event.ts.clone(),
        detail: None,
        approved: false,
    });
    run.bead = event.bead.clone();
    run.updated = event.ts.clone();
    if let Some(stage) = event.stage {
        run.stage = stage;
    }
    if event.detail.is_some() {
        run.detail = event.detail.clone();
    }
    // **承認は event に残った逐語だけである**（憲法 C7.2）。kind だけで関門を開けると、
    // `fleet record --kind ApprovalReceived` で積んだ逐語 0 字の機械 event でも開いてしまい、
    // 書き手側（`pipe approve`）の逐語検査が作法頼みになる。読み手が資格を見る。
    if event.kind == EventKind::ApprovalReceived
        && event.actor == ACTOR_HUMAN
        && event.detail.as_deref().is_some_and(|words| !words.trim().is_empty())
    {
        run.approved = true;
    }
}

/// 1 件の event を席へ反映する。
fn apply_seat(state: &mut State, event: &Event) {
    let Some(id) = event.seat.clone() else {
        return;
    };
    let seat = state.seats.entry(id.clone()).or_insert_with(|| Seat {
        id,
        run: event.run.clone(),
        pid: event.pid,
        state: SeatState::Live,
        updated: event.ts.clone(),
    });
    seat.run = event.run.clone();
    seat.updated = event.ts.clone();
    if event.pid.is_some() {
        seat.pid = event.pid;
    }
    match event.kind {
        EventKind::SeatSpawned => seat.state = SeatState::Live,
        EventKind::SeatStopped => seat.state = SeatState::Stopped,
        EventKind::RunCreated
        | EventKind::RunStage
        | EventKind::RunDone
        | EventKind::RunStopped
        | EventKind::ApprovalRequested
        | EventKind::ApprovalReceived
        | EventKind::QuestionRaised
        | EventKind::QuestionAnswered
        | EventKind::AllowanceMeasured
        | EventKind::AllowanceUnmeasured => {}
    }
}

/// 待つ対象。**述語を受ける口は作らない**（C3.4: 待機は 1 実装）。
///
/// variant が運ぶのは**データ**だけである。何を読んで満たされたと判じるかは [`wait`] の
/// 内側が持つ（受付の枠なら meminfo と札の読み手・設計 gate-cost.md §3.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    /// runner の process が終わること。
    RunnerExited(u32),
    /// 席の process が消えること（TERM の後）。
    SeatGone(u32),
    /// host の受付に枠が 1 つ以上空くこと（[`crate::pipe::admission`]）。
    SlotFree {
        /// 受付札の置き場（`<state_dir の親>/<NAME>-host/slots/`）。
        slots_dir: std::path::PathBuf,
        /// この受付が要る枠（jobs の数）。
        want: u64,
        /// job 1 つが要る memory（MiB・rules 行 `gate.job_memory_mb`）。
        job_mb: u64,
        /// 席と host のために残す memory（MiB・rules 行 `host.reserve_memory_mb`）。
        reserve_mb: u64,
        /// 並列度の上限（rules 行 `gate.mutants_jobs`）。
        cap: u64,
    },
    /// process group の全員が消えること（group 宛ての TERM / KILL の後・値は group id）。
    GroupGone(u32),
    /// land の番が来ること（[`crate::pipe::land`]・設計 gate-cost.md §6）: 同じ置き場の着地待ちの列で
    /// 自分より前の便が居なくなる。列を導けない周も満たされた側である（待たずに進む・記録は land が残す）。
    LandTurn {
        /// event log の置き場（列は replay から導く・別の状態 file を持たない）。
        state_dir: std::path::PathBuf,
        /// 待つ便の id。
        run: String,
    },
}

impl Completion {
    /// 見張る pid。**pid を見張らない variant（[`Self::SlotFree`] / [`Self::LandTurn`]）は 0**——pid 0 は
    /// `/proc/0` を持たない（user の process に振られない）ので、生きている pid と取り違えない。
    /// [`Self::GroupGone`] は group id（= group leader の pid）を返す。
    pub fn pid(&self) -> u32 {
        match *self {
            Self::RunnerExited(pid) | Self::SeatGone(pid) | Self::GroupGone(pid) => pid,
            Self::SlotFree { .. } | Self::LandTurn { .. } => 0,
        }
    }

    /// 満たされたか（1 周分の観測）。
    fn is_met(&self) -> bool {
        match self {
            Self::RunnerExited(pid) | Self::SeatGone(pid) => !pid_is_live(*pid),
            Self::GroupGone(group) => !group_is_live(*group),
            Self::SlotFree { slots_dir, want, job_mb, reserve_mb, cap } => {
                crate::pipe::admission::has_room(
                    slots_dir,
                    (*want).min(*cap),
                    crate::pipe::admission::Sizes { job_mb: *job_mb, reserve_mb: *reserve_mb },
                )
            }
            Self::LandTurn { state_dir, run } => !matches!(
                crate::pipe::land::turn_now(state_dir, run),
                crate::pipe::land::Turn::After(_)
            ),
        }
    }
}

/// 期限までに終わらなかった。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeout;

/// 待つ間隔。
const POLL: Duration = Duration::from_millis(20);

/// [`Completion`] が満たされるまで待つ。**これが唯一の待機実装である**。
///
/// process の生存は `/proc/<pid>` の有無で見る（libc を足さないため・NFR3）。受付の枠は
/// 周ごとに meminfo と札を読み直す（周期はこの [`POLL`] のまま・上限は呼び手の期限）。
pub fn wait(completion: Completion, deadline: Duration) -> Result<(), Timeout> {
    let started = Instant::now();
    loop {
        if completion.is_met() {
            return Ok(());
        }
        if started.elapsed() >= deadline {
            return Err(Timeout);
        }
        std::thread::sleep(POLL);
    }
}

/// pid が生きているか。
fn pid_is_live(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// pgid が `group` の process が `/proc` に 1 つでも在るか（zombie も数える＝回収されるまで在る）。
///
/// **`/proc` を読めない周は「在る」**（消えたと測れていないものを消えたにしない・fail-closed）。
/// 読む間に消えた process の stat は読めないので飛ばす。
fn group_is_live(group: u32) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return true;
    };
    entries.flatten().any(|entry| {
        let numeric = entry.file_name().to_str().is_some_and(|name| name.bytes().all(|b| b.is_ascii_digit()));
        numeric
            && std::fs::read_to_string(entry.path().join("stat"))
                .ok()
                .and_then(|text| pgid_of(&text))
                == Some(group)
    })
}

/// `/proc/<pid>/stat` の 1 行から pgid（第 5 欄）を読む（pure）。
///
/// `comm` は空白も `)` も含みうるので、**最後の `)`** の後ろから数える（state・ppid・pgrp の順）。
/// 欄が足りない行・数でない欄は `None`。
fn pgid_of(stat_text: &str) -> Option<u32> {
    let (_, rest) = stat_text.rsplit_once(')')?;
    rest.split_whitespace().nth(2)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{pgid_of, Completion};

    #[test]
    fn pipe_stop_group_pgid_of_reads_the_fifth_field() {
        assert_eq!(pgid_of("4242 (sleep) S 4200 4100 4100 0 -1 4194560 91 0"), Some(4100), "通常の comm");
        assert_eq!(pgid_of("4242 (Web Content) S 4200 777 777 0 -1"), Some(777), "空白入り comm");
        assert_eq!(pgid_of("4242 (a) S 1 2 (b)) R 9 31 32 0"), Some(31), "`)` 入り comm は最後の `)` から数える");
        assert_eq!(pgid_of("4242 (sleep) S 4200"), None, "欄が足りない");
        assert_eq!(pgid_of("4242 (sleep) S 4200 x 1"), None, "数でない欄");
        assert_eq!(pgid_of("4242 sleep S 4200 4100"), None, "comm の閉じが無い");
    }

    #[test]
    fn pipe_stop_group_completion_pid_is_the_group_id() {
        assert_eq!(Completion::GroupGone(31337).pid(), 31337);
    }

    /// wait の網羅 match が新 variant を含む（variant を足したら compile で気付く形の歯）。
    #[test]
    fn pipe_stop_group_completion_match_is_exhaustive() {
        let all = [
            Completion::RunnerExited(7),
            Completion::SeatGone(8),
            Completion::SlotFree {
                slots_dir: std::path::PathBuf::from("slots"),
                want: 1,
                job_mb: 1,
                reserve_mb: 1,
                cap: 1,
            },
            Completion::GroupGone(9),
            Completion::LandTurn { state_dir: std::path::PathBuf::from("state"), run: "r".to_owned() },
        ];
        let names: Vec<&str> = all
            .iter()
            .map(|found| match found {
                Completion::RunnerExited(_) => "RunnerExited",
                Completion::SeatGone(_) => "SeatGone",
                Completion::SlotFree { .. } => "SlotFree",
                Completion::GroupGone(_) => "GroupGone",
                Completion::LandTurn { .. } => "LandTurn",
            })
            .collect();
        assert_eq!(
            names,
            ["RunnerExited", "SeatGone", "SlotFree", "GroupGone", "LandTurn"],
            "宣言順の末尾に LandTurn"
        );
    }
}
