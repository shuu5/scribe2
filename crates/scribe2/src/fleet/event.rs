//! event log の 1 行（[`Event`]）の読み書き（設計 fleet-event-log.md §3）。
//!
//! 列挙と型の定義は [`super`]（`fleet/mod.rs`）に在り、本 file は行との変換だけを持つ
//! （`s2-07l.260` で挙動不変に分割・外の呼び手の path は `fleet` の再 export で保つ）。

use super::json_lite::{self, Value};
use super::{
    parse_actor, Allowance, EventKind, Measured, Registration, Stage, Unmeasured, UnmeasuredReason, WindowKind,
    SCHEMA,
};
use crate::seat::role::Role;

/// event の 1 行が持てる key の全体（設計 §3）。
///
/// 未知 key を受理すると、綴り違いの field が黙って捨てられる（`stgae` と書いた行が
/// 段の無い行として通る）。設計 §3 の「それ以外の形は error」に合わせて拒む。
const KNOWN_KEYS: &[&str] = &[
    "schema", "ts", "kind", "run", "bead", "host", "actor", "stage", "seat", "pid", "detail",
    "account", "window", "model", "endpoint", "used_pct", "resets_at", "reason", "role", "anchor", "target", "sid", "launch",
];

/// 口座残量の kind だけが持てる key（設計 fleet-usage.md §4）。
///
/// 既存 kind の行にこれが在れば malformed である。口座の field を持った `RunStage` の行を
/// 通すと、`run` を持つ行と持たない行の区別が kind から読めなくなる。例外は `account` を label として持つ
/// 退役・戻しの kind と、任意 field として持つ `SeatSpawned`（[`Body::spawned`]・ADR-0027 §2.3）だけ。
const ALLOWANCE_KEYS: &[&str] = &[
    "account",
    "window",
    "model",
    "endpoint",
    "used_pct",
    "resets_at",
    "reason",
];

/// 席の登録の kind だけが持てる key（他の kind の行に在れば malformed・`account` は口座残量と共有）。
const REGISTRATION_KEYS: &[&str] = &["role", "anchor", "target", "sid", "launch"];

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
    /// 席の登録の本体（[`EventKind::SeatRegistered`] でだけ `Some`）。
    pub registration: Option<Registration>,
    /// 口座 label。[`EventKind::AccountRetired`] / [`EventKind::AccountRestored`] では退役・戻しの本体（必須）、
    /// [`EventKind::SeatSpawned`] では**便を起こした口座**（任意・ADR-0027 §2.3・走行中の便数の出所・field の無い
    /// 旧い行は「口座不明」＝数えない）。他の kind に在れば malformed。kind ごとの typed payload・`detail`〔自由文〕を
    /// 判定入力にしない＝憲法 C3.3。
    pub account: Option<String>,
}

impl Event {
    /// 1 行の JSON にする。
    ///
    /// 本体は [`Self::allowance`] / [`Self::registration`] の有無と、退役・戻しの kind（[`EventKind::is_account_lifecycle`]）
    /// が決める（`run` / `bead` を持つ行と口座残量・登録・退役の行は同じ並びを共有しない）。`account` の有無では
    /// 決めない（`SeatSpawned` は `run` / `bead` と `account` を両方持つ）。食い違った組は [`Self::from_line`] が
    /// 読み返せず malformed になるので、書いた行が読めない形は歯で捕まる。
    pub fn to_line(&self) -> String {
        let mut pairs: Vec<(&str, Value)> = vec![
            ("schema", Value::Num(self.schema)),
            ("ts", Value::Str(self.ts.clone())),
            ("kind", Value::Str(self.kind.as_str().to_owned())),
        ];
        if self.allowance.is_none() && self.registration.is_none() && !self.kind.is_account_lifecycle() {
            pairs.push(("run", Value::Str(self.run.clone())));
            pairs.push(("bead", Value::Str(self.bead.clone())));
        }
        pairs.extend(self.account.iter().map(|label| ("account", Value::Str(label.clone()))));
        pairs.extend(self.allowance.iter().flat_map(Allowance::pairs));
        if let Some(found) = &self.registration {
            pairs.push(("role", Value::Str(found.role.as_str().to_owned())));
            pairs.push(("anchor", Value::Str(found.anchor.clone())));
            pairs.push(("target", Value::Str(found.target.clone())));
            // `sid` は任意（launch の row は `None`・key ごと書かない＝`null` を出さない・schema 1 のまま）。
            pairs.extend(found.sid.iter().map(|sid| ("sid", Value::Str(sid.clone()))));
            pairs.push(("account", Value::Str(found.account.clone())));
            pairs.push(("launch", Value::Str(found.launch.clone())));
            // `model` は任意（schema 1 のまま値の追加・None の row は key ごと書かない＝旧 row と同じ形）。
            pairs.extend(found.model.iter().map(|model| ("model", Value::Str(model.clone()))));
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
            registration: body.registration,
            account: body.account,
        })
    }
}

/// kind ごとに違う本体（`run` / `bead` を持つ行か、口座残量の行か、登録の行か）。
#[derive(Default)]
struct Body {
    /// 便 id（口座残量・登録の行では空）。
    run: String,
    /// bead id（口座残量・登録の行では空）。
    bead: String,
    /// 口座残量の本体。
    allowance: Option<Allowance>,
    /// 席の登録の本体。
    registration: Option<Registration>,
    /// 口座の退役・戻しの label。
    account: Option<String>,
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
            EventKind::SeatRegistered => Self::registration(pairs),
            EventKind::AccountRetired | EventKind::AccountRestored => Self::account(pairs),
            EventKind::SeatSpawned => Self::spawned(pairs),
            EventKind::RunCreated
            | EventKind::RunStage
            | EventKind::RunDone
            | EventKind::RunStopped
            | EventKind::SeatStopped
            | EventKind::ApprovalRequested
            | EventKind::ApprovalReceived
            | EventKind::QuestionRaised
            | EventKind::QuestionAnswered => {
                forbid(pairs, ALLOWANCE_KEYS.iter().chain(REGISTRATION_KEYS))?;
                Ok(Self {
                    run: text_of(field(pairs, "run"), "run")?,
                    bead: text_of(field(pairs, "bead"), "bead")?,
                    ..Self::default()
                })
            }
        }
    }

    /// 口座残量の行の本体。`run` / `bead` と登録の key は**持たない**（在れば malformed）。
    fn allowance(pairs: &[(String, Value)], allowance: Allowance) -> Result<Self, String> {
        forbid(pairs, ["run", "bead"].iter().chain(REGISTRATION_KEYS))?;
        Ok(Self { allowance: Some(allowance), ..Self::default() })
    }

    /// 登録の行の本体。`run` / `bead` と口座残量だけの key（`account` / `model` 以外）は**持たない**。
    /// `model` は任意（key が無い旧 row は `None`・在って文字列でなければ malformed）。`sid` も任意（key の省略か
    /// `null` が `None`＝launch の row・在って文字列でなければ malformed）。
    fn registration(pairs: &[(String, Value)]) -> Result<Self, String> {
        forbid(pairs, ["run", "bead"].iter().chain(ALLOWANCE_KEYS.iter().filter(|key| !["account", "model"].contains(key))))?;
        let text = |key: &str| text_of(field(pairs, key), key);
        let role = text("role")?;
        let registration = Registration {
            role: Role::parse(&role).ok_or(format!("role {role} は未知である"))?,
            anchor: text("anchor")?,
            target: text("target")?,
            sid: nullable_text(field(pairs, "sid"), "sid")?,
            account: text("account")?,
            launch: text("launch")?,
            model: optional_text(field(pairs, "model"), "model")?,
        };
        Ok(Self { registration: Some(registration), ..Self::default() })
    }

    /// 口座の退役・戻しの行の本体（`account` = label だけ）。`run` / `bead`・口座残量だけの key・登録の key は**持たない**。
    fn account(pairs: &[(String, Value)]) -> Result<Self, String> {
        let foreign = ALLOWANCE_KEYS.iter().filter(|key| **key != "account");
        forbid(pairs, ["run", "bead"].iter().chain(foreign).chain(REGISTRATION_KEYS))?;
        Ok(Self { account: Some(text_of(field(pairs, "account"), "account")?), ..Self::default() })
    }

    /// 席を立てた行の本体: `run` / `bead` に加えて `account`（便を起こした口座）を**任意**で持つ（ADR-0027 §2.3・
    /// schema 1 のまま値の追加＝key の無い旧い行は `None`・在って文字列でなければ malformed）。口座残量だけの key と
    /// 登録の key は持たない（`account` の例外を開けるのはこの kind だけ）。
    fn spawned(pairs: &[(String, Value)]) -> Result<Self, String> {
        let foreign = ALLOWANCE_KEYS.iter().filter(|key| **key != "account");
        forbid(pairs, foreign.chain(REGISTRATION_KEYS))?;
        Ok(Self {
            run: text_of(field(pairs, "run"), "run")?,
            bead: text_of(field(pairs, "bead"), "bead")?,
            account: optional_text(field(pairs, "account"), "account")?,
            ..Self::default()
        })
    }
}

/// 在ってはならない key の列。1 つでも在れば理由つきで `Err`。
fn forbid<'a>(pairs: &[(String, Value)], keys: impl IntoIterator<Item = &'a &'a str>) -> Result<(), String> {
    keys.into_iter().try_for_each(|key| absent(field(pairs, key), key))
}

/// `AllowanceMeasured` の field を読む。`seven_day_model` の行は `model` も必須。
/// `resets_at` は任意（欠け = 消費の無い窓・在って文字列でなければ malformed・ADR-0024 §2.3）。
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
        resets_at: optional_text(field(pairs, "resets_at"), "resets_at")?,
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

/// 任意の文字列 field のうち **`null` も「無い」と読む**もの（登録 row の `sid`・account-lifecycle.md §4）。
/// key が在って文字列でも `null` でもなければ `Err`（[`optional_text`] と同じ極性・型違いを `None` に落とさない）。
fn nullable_text(value: Option<&Value>, key: &str) -> Result<Option<String>, String> {
    match value {
        Some(Value::Null) => Ok(None),
        other => optional_text(other, key),
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
