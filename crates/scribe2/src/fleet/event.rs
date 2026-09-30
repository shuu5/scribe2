//! event log の 1 行（[`Event`]）の読み書き（設計 fleet-event-log.md §3）。
//!
//! 列挙と型の定義は [`super`]（`fleet/mod.rs`）に在り、本 file は行との変換だけを持つ
//! （`s2-07l.260` で挙動不変に分割・外の呼び手の path は `fleet` の再 export で保つ）。

use super::json_lite::{self, Value};
use super::{
    parse_actor, Allowance, Case, Channel, Cost, CostSource, EventKind, Install, Mark, Measured, Pressure, Registration,
    Shape, Sorting, Stage, Unmeasured, UnmeasuredReason, Usage, WindowKind, SCHEMA,
};
use crate::ledger::question::ASKED;
use crate::seat::role::Role;

/// event の 1 行が持てる key の全体（設計 §3）。
///
/// 未知 key を受理すると、綴り違いの field が黙って捨てられる（`stgae` と書いた行が
/// 段の無い行として通る）。設計 §3 の「それ以外の形は error」に合わせて拒む。
const KNOWN_KEYS: &[&str] = &[
    "schema", "ts", "kind", "run", "bead", "host", "actor", "stage", "seat", "pid", "detail",
    "account", "window", "model", "endpoint", "used_pct", "resets_at", "reason", "role", "anchor", "target", "sid", "launch",
    "mark", "source", "usage", "turns", "wall_ms", "rule",
    "channel", "session", "utterance", "sorting", "refuse", "version", "main",
    "ruling", "question_ts", "asked",
];

/// run 無しの裁定の kind（[`Shape::Ruling`]）だけが持てる key（他の kind の行に在れば malformed・設計 §9・§14）。
const RULING_KEYS: &[&str] = &["rule", "ruling", "question_ts", "asked"];

/// 結びの裁定（[`Case::Ruling`]）の 5 key（設計 §14・`rule` と併せ持てない）。`utterance` と `channel` は案件の kind とも共有する。
const BOUND_KEYS: &[&str] = &["ruling", "utterance", "channel", "question_ts", "asked"];

/// 結びの裁定が案件の kind と共有する key（[`Shape::Ruling`] の行はこの 2 つだけ [`CASE_KEYS`] から持てる）。
const BOUND_SHARED_KEYS: &[&str] = &["utterance", "channel"];

/// 案件の一生の kind（[`Shape::Case`]）だけが持てる key（他の kind の行に在れば malformed・kind ごとの内訳は
/// [`Body::case`] の `own`・設計 §12）。
const CASE_KEYS: &[&str] = &["channel", "session", "utterance", "sorting", "refuse", "version", "main"];

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

/// 列の印の kind（[`Shape::Mark`]）だけが持てる key（他の kind の行に在れば malformed）。
const MARK_KEYS: &[&str] = &["mark"];

/// 消費の kind（[`Shape::Cost`]）だけが持てる key（他の kind の行に在れば malformed・[`Cost`] の 4 key）。
const COST_KEYS: &[&str] = &["source", "usage", "turns", "wall_ms"];

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
    /// 席の登録の本体（[`EventKind::SeatRegistered`] と退役の [`EventKind::SeatRetired`] でだけ `Some`）。
    pub registration: Option<Registration>,
    /// 列の介入の印（[`EventKind::DispatchMark`] でだけ `Some`＝必須・他の kind に在れば malformed）。
    /// **typed な値で持つ**——`detail`〔自由文〕を判定入力にしない（憲法 C3.3）。
    pub mark: Option<Mark>,
    /// 口座 label。[`EventKind::AccountRetired`] / [`EventKind::AccountRestored`] では退役・戻しの本体（必須）、
    /// [`EventKind::SeatSpawned`] では**便を起こした口座**（任意・ADR-0027 §2.3・走行中の便数の出所・field の無い
    /// 旧い行は「口座不明」＝数えない）。他の kind に在れば malformed。kind ごとの typed payload・`detail`〔自由文〕を
    /// 判定入力にしない＝憲法 C3.3。
    pub account: Option<String>,
    /// 消費の本体（[`EventKind::RunCost`] でだけ `Some`＝必須・他の kind に在れば malformed・設計 gate-cost.md §26 形 (2)）。
    pub cost: Option<Cost>,
    /// 裁定が指す rules 行の id（[`EventKind::RulingReceived`] でだけ任意に `Some`・他の kind に在れば malformed・設計 §9）。
    pub rule: Option<String>,
    /// 案件の一生の 5 kind の本体（[`Shape::Case`] の kind でだけ `Some`＝必須・他の kind に在れば malformed・設計 §12）。
    pub case: Option<Case>,
}

impl Event {
    /// 1 行の JSON にする。
    ///
    /// 本体の並びは kind の [`EventKind::shape`] が決める（`run` / `bead` を持つ行と、口座残量・登録・退役・
    /// 列の印の行は同じ並びを共有しない）。**本体や `account` の有無では決めない**——`SeatSpawned` は
    /// `run` / `bead` と `account` を両方持つ。食い違った組（形と本体が噛み合わない値）は [`Self::from_line`] が
    /// 読み返せず malformed になるので、書いた行が読めない形は歯で捕まる。
    pub fn to_line(&self) -> String {
        let mut pairs: Vec<(&str, Value)> = vec![
            ("schema", Value::Num(self.schema)),
            ("ts", Value::Str(self.ts.clone())),
            ("kind", Value::Str(self.kind.as_str().to_owned())),
        ];
        match self.kind.shape() {
            Shape::Run | Shape::Cost => {
                pairs.push(("run", Value::Str(self.run.clone())));
                pairs.push(("bead", Value::Str(self.bead.clone())));
            }
            Shape::Mark => {
                pairs.push(("bead", Value::Str(self.bead.clone())));
                pairs.extend(self.mark.iter().map(|mark| ("mark", Value::Str(mark.as_str().to_owned()))));
            }
            // 裁定の `bead` は任意（空なら key ごと書かない＝`null` を出さない）。
            Shape::Ruling => {
                pairs.extend(Some(&self.bead).filter(|bead| !bead.is_empty()).map(|bead| ("bead", Value::Str(bead.clone()))));
                pairs.extend(self.rule.iter().map(|rule| ("rule", Value::Str(rule.clone()))));
                pairs.extend(self.case.iter().flat_map(|case| case.pairs(&self.bead)));
            }
            Shape::Case => pairs.extend(self.case.iter().flat_map(|case| case.pairs(&self.bead))),
            Shape::Allowance | Shape::Registration | Shape::Account | Shape::Install | Shape::Pressure | Shape::Group => {}
        }
        pairs.extend(self.account.iter().map(|label| ("account", Value::Str(label.clone()))));
        pairs.extend(self.allowance.iter().flat_map(Allowance::pairs));
        pairs.extend(self.cost.iter().flat_map(Cost::pairs));
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
            mark: body.mark,
            account: body.account,
            cost: body.cost,
            rule: body.rule,
            case: body.case,
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
    /// 列の介入の印。
    mark: Option<Mark>,
    /// 口座の退役・戻しの label。
    account: Option<String>,
    /// 消費の本体。
    cost: Option<Cost>,
    /// 裁定が指す rules 行の id。
    rule: Option<String>,
    /// 案件の一生の kind の本体。
    case: Option<Case>,
}

impl Body {
    /// kind ごとの必須 field を**網羅 `match`** で読む。
    ///
    /// kind を足した便は、その kind の行がどの field を要るかをここで必ず決める。消費の key（[`COST_KEYS`]）は
    /// [`Shape::Cost`] の kind の行だけが持てる（他の kind の行に在れば malformed）。
    fn read(pairs: &[(String, Value)], kind: EventKind) -> Result<Self, String> {
        if kind.shape() != Shape::Cost {
            forbid(pairs, COST_KEYS)?;
        }
        if kind.shape() != Shape::Ruling {
            forbid(pairs, RULING_KEYS)?;
        }
        if kind.shape() != Shape::Case {
            let shared = if kind.shape() == Shape::Ruling { BOUND_SHARED_KEYS } else { &[] };
            forbid(pairs, CASE_KEYS.iter().filter(|key| !shared.contains(key)))?;
        }
        match kind {
            EventKind::AllowanceMeasured => {
                Self::allowance(pairs, Allowance::Measured(measured_of(pairs)?))
            }
            EventKind::AllowanceUnmeasured => {
                Self::allowance(pairs, Allowance::Unmeasured(unmeasured_of(pairs)?))
            }
            // 退役の行は登録の行と同じ本体（退役した row の写し・account-lifecycle.md §24）＝同じ読み手 1 本。
            EventKind::SeatRegistered | EventKind::SeatRetired => Self::registration(pairs),
            EventKind::AccountRetired | EventKind::AccountRestored => Self::account(pairs),
            EventKind::DispatchMark => Self::mark(pairs),
            EventKind::SeatSpawned => Self::spawned(pairs),
            EventKind::InstallRecorded => Self::install(pairs),
            EventKind::RunCost => Self::cost(pairs),
            EventKind::RulingReceived => Self::ruling(pairs),
            EventKind::GroupPressureNotified => Self::pressure(pairs),
            EventKind::GroupMoved | EventKind::GroupMoveRefused | EventKind::GroupMovePending => Self::group(pairs),
            EventKind::UtteranceReceived => Self::case(pairs, &["channel", "session"], utterance_of),
            EventKind::UtteranceSorted => Self::case(pairs, &["utterance", "sorting", "bead"], sorted_of),
            EventKind::TurnEndUnjudged => Self::case(pairs, &["session", "reason"], turn_end_of),
            EventKind::IntakeRefused => Self::case(pairs, &["bead", "refuse"], refused_of),
            EventKind::LifecycleCutover => Self::case(pairs, &["version", "main"], cutover_of),
            EventKind::MemoJudged => Self::case(pairs, &["bead"], judged_of),
            EventKind::RunCreated
            | EventKind::RunStage
            | EventKind::RunDone
            | EventKind::RunStopped
            | EventKind::SeatStopped
            | EventKind::ApprovalRequested
            | EventKind::ApprovalReceived
            | EventKind::QuestionRaised
            | EventKind::QuestionAnswered => {
                forbid(pairs, ALLOWANCE_KEYS.iter().chain(REGISTRATION_KEYS).chain(MARK_KEYS))?;
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
        forbid(pairs, ["run", "bead"].iter().chain(REGISTRATION_KEYS).chain(MARK_KEYS))?;
        Ok(Self { allowance: Some(allowance), ..Self::default() })
    }

    /// 登録の行の本体。`run` / `bead` と口座残量だけの key（`account` / `model` 以外）は**持たない**。
    /// `model` は任意（key が無い旧 row は `None`・在って文字列でなければ malformed）。`sid` も任意（key の省略か
    /// `null` が `None`＝launch の row・在って文字列でなければ malformed）。
    fn registration(pairs: &[(String, Value)]) -> Result<Self, String> {
        forbid(
            pairs,
            ["run", "bead"]
                .iter()
                .chain(ALLOWANCE_KEYS.iter().filter(|key| !["account", "model"].contains(key)))
                .chain(MARK_KEYS),
        )?;
        let text = |key: &str| text_of(field(pairs, key), key);
        let role = text("role")?;
        // **知らない役割の行は本体を持たない行として読む**（退役した役割の row・憲法 N4 の schema 互換）。
        // 既に在る log は役割を 1 つにする前の行を持つので、ここで `Err` に倒すと**その 1 行で replay 全体が
        // unreadable**になり、role guard は FailClosed ゆえ全席の権能付きの操作が deny になる（実測 2026-09-18）。
        // 項目の欠け（`role` の key が無い）は従来どおり malformed で、「知らない値」と混ぜない。
        let Some(parsed) = Role::parse(&role) else {
            return Ok(Self::default());
        };
        let registration = Registration {
            role: parsed,
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
        forbid(pairs, ["run", "bead"].iter().chain(foreign).chain(REGISTRATION_KEYS).chain(MARK_KEYS))?;
        Ok(Self { account: Some(text_of(field(pairs, "account"), "account")?), ..Self::default() })
    }

    /// 列の印の行の本体（`bead` + typed な `mark`）。**`run` は持たない**（印は bead に付く・設計
    /// dispatcher.md §4）。口座残量だけの key・登録の key も持たない。3 値の外の `mark` は malformed で、
    /// `None` に落とさない（書き側と読み側が同じ判定を使う）。
    fn mark(pairs: &[(String, Value)]) -> Result<Self, String> {
        forbid(pairs, ["run"].iter().chain(ALLOWANCE_KEYS).chain(REGISTRATION_KEYS))?;
        let text = text_of(field(pairs, "mark"), "mark")?;
        let mark = Mark::parse(&text).ok_or(format!("mark {text} は first でも hold でも release でもない"))?;
        Ok(Self { bead: text_of(field(pairs, "bead"), "bead")?, mark: Some(mark), ..Self::default() })
    }

    /// 席を立てた行の本体: `run` / `bead` に加えて `account`（便を起こした口座）を**任意**で持つ（ADR-0027 §2.3・
    /// schema 1 のまま値の追加＝key の無い旧い行は `None`・在って文字列でなければ malformed）。口座残量だけの key と
    /// 登録の key は持たない（`account` の例外を開けるのはこの kind だけ）。
    fn spawned(pairs: &[(String, Value)]) -> Result<Self, String> {
        let foreign = ALLOWANCE_KEYS.iter().filter(|key| **key != "account");
        forbid(pairs, foreign.chain(REGISTRATION_KEYS).chain(MARK_KEYS))?;
        Ok(Self {
            run: text_of(field(pairs, "run"), "run")?,
            bead: text_of(field(pairs, "bead"), "bead")?,
            account: optional_text(field(pairs, "account"), "account")?,
            ..Self::default()
        })
    }

    /// install の行の本体: `detail` が [`Install`] の 1 行であることだけを見る（値は `detail` のまま持つ）。`run` /
    /// `bead`・`stage` / `seat` / `pid`（`seat` が在ると replay が幽霊の席を作る）・口座残量・登録・列の印の key は
    /// **持たない**（在れば malformed・`fleet record` は書き側で断る）。
    fn install(pairs: &[(String, Value)]) -> Result<Self, String> {
        let foreign = ["run", "bead", "stage", "seat", "pid"];
        forbid(pairs, foreign.iter().chain(ALLOWANCE_KEYS).chain(REGISTRATION_KEYS).chain(MARK_KEYS))?;
        let detail = text_of(field(pairs, "detail"), "detail")?;
        Install::parse(&detail).ok_or(format!("detail {detail:?} は sha=<sha12> path=<path> でない"))?;
        Ok(Self::default())
    }

    /// 消費の行の本体: `run` / `bead` と [`Cost`]（`source` の 3 値と 6 値・**全部が必須**＝欠けを 0 に倒さない）。
    /// 口座残量・登録・列の印の key は持たない（在れば malformed）。
    fn cost(pairs: &[(String, Value)]) -> Result<Self, String> {
        forbid(pairs, ALLOWANCE_KEYS.iter().chain(REGISTRATION_KEYS).chain(MARK_KEYS))?;
        let source = text_of(field(pairs, "source"), "source")?;
        let source = CostSource::parse(&source).ok_or(format!("source {source} は runner / lens / review でない"))?;
        let usage = Usage::from_pairs(pairs).ok_or("usage / turns / wall_ms の 6 値が揃わない")?;
        Ok(Self {
            run: text_of(field(pairs, "run"), "run")?,
            bead: text_of(field(pairs, "bead"), "bead")?,
            cost: Some(Cost { source, usage }),
            ..Self::default()
        })
    }

    /// run 無しの裁定の行の本体（設計 §9・§14）: `detail`（user の逐語）が**必須**・`bead` と `rule` は任意（在って文字列でなければ
    /// malformed）。`run` / `stage` / `seat` / `pid`（`seat` が在ると replay が幽霊の席を作る）・口座残量・登録・列の印の key は
    /// **持たない**（在れば malformed）。結びの 5 key（[`BOUND_KEYS`]）を 1 つでも持つ行は新しい形で、`rule` と併せ持てず
    /// （key を名指して malformed）、`ruling` / `utterance` / `channel` / `question_ts` と `bead`（問い id）が必須。
    fn ruling(pairs: &[(String, Value)]) -> Result<Self, String> {
        let foreign = ["run", "stage", "seat", "pid"];
        forbid(pairs, foreign.iter().chain(ALLOWANCE_KEYS).chain(REGISTRATION_KEYS).chain(MARK_KEYS))?;
        text_of(field(pairs, "detail"), "detail")?;
        let bead = optional_text(field(pairs, "bead"), "bead")?.unwrap_or_default();
        let Some(bound) = BOUND_KEYS.iter().find(|key| field(pairs, key).is_some()) else {
            return Ok(Self { bead, rule: optional_text(field(pairs, "rule"), "rule")?, ..Self::default() });
        };
        if field(pairs, "rule").is_some() {
            return Err(format!("rule と {bound} を併せ持つ（結びの行は rule を持たない）"));
        }
        if bead.is_empty() {
            return Err("結びの行に bead（問い id）が無い".to_owned());
        }
        let channel = word_of(pairs, "channel")?;
        let channel = Channel::parse(&channel).ok_or(format!("channel {channel} は chat / gui でない"))?;
        let asked = optional_word(pairs, "asked")?;
        if let Some(found) = asked.as_deref().filter(|found| !ASKED.contains(found)) {
            return Err(format!("asked {found} は {} のどちらでもない", ASKED.join(" / ")));
        }
        let case = Case::Ruling {
            ruling: word_of(pairs, "ruling")?,
            utterance: word_of(pairs, "utterance")?,
            channel,
            question_ts: word_of(pairs, "question_ts")?,
            asked,
        };
        Ok(Self { bead, case: Some(case), ..Self::default() })
    }

    /// 群の逼迫の通知の行の本体（設計 account-lifecycle.md §19 形 3）: `account`（口座 label）と `detail`（[`Pressure`] の
    /// 1 行）が**必須**。`run` / `bead` / `stage` / `seat` / `pid`（`seat` が在ると replay が幽霊の席を作る）・口座残量だけの
    /// key・登録・列の印の key は**持たない**（在れば malformed）。
    fn pressure(pairs: &[(String, Value)]) -> Result<Self, String> {
        let foreign = ["run", "bead", "stage", "seat", "pid"];
        let allowance = ALLOWANCE_KEYS.iter().filter(|key| **key != "account");
        forbid(pairs, foreign.iter().chain(allowance).chain(REGISTRATION_KEYS).chain(MARK_KEYS))?;
        let detail = text_of(field(pairs, "detail"), "detail")?;
        Pressure::parse(&detail).ok_or(format!("detail {detail:?} は group=<名> window=<窓> used= cap= sent= でない"))?;
        Ok(Self { account: Some(text_of(field(pairs, "account"), "account")?), ..Self::default() })
    }

    /// 群の移動の承認・断り・保留の行の本体（設計 account-lifecycle.md §20 形 5 / 6）: `account`（口座 label）と空でない
    /// `detail` が**必須**。持たない key は [`Self::pressure`] と同じ（`run` / `bead` / `stage` / `seat` / `pid`・口座残量だけの
    /// key・登録・列の印の key は在れば malformed＝承認の行が幽霊の便や席を作らない）。
    fn group(pairs: &[(String, Value)]) -> Result<Self, String> {
        let foreign = ["run", "bead", "stage", "seat", "pid"];
        let allowance = ALLOWANCE_KEYS.iter().filter(|key| **key != "account");
        forbid(pairs, foreign.iter().chain(allowance).chain(REGISTRATION_KEYS).chain(MARK_KEYS))?;
        if text_of(field(pairs, "detail"), "detail")?.trim().is_empty() {
            return Err("detail が空".to_owned());
        }
        Ok(Self { account: Some(text_of(field(pairs, "account"), "account")?), ..Self::default() })
    }

    /// 案件の一生の kind の行の本体（設計 §12 の表）: `own` はこの kind が持てる本体の key（`bead` と、TurnEndUnjudged の
    /// `reason` を含む）で、`read` が本体と `bead` を読む。`run` / `stage` / `seat` / `pid`・`own` の外の [`CASE_KEYS`]・口座残量・
    /// 登録・列の印の key は**持たない**（在れば malformed）。`detail` は任意（発話の逐語の必須は `read` が見る）。
    fn case(pairs: &[(String, Value)], own: &[&str], read: CaseReader) -> Result<Self, String> {
        let keys = ["run", "bead", "stage", "seat", "pid"].iter().chain(CASE_KEYS).chain(ALLOWANCE_KEYS);
        forbid(pairs, keys.chain(REGISTRATION_KEYS).chain(MARK_KEYS).filter(|key| !own.contains(key)))?;
        let (case, bead) = read(pairs)?;
        Ok(Self { bead: bead.unwrap_or_default(), case: Some(case), ..Self::default() })
    }
}

/// 案件の一生の kind の本体の読み手（本体と、在れば `bead`）。
type CaseReader = fn(&[(String, Value)]) -> Result<(Case, Option<String>), String>;

/// UtteranceReceived: `channel` の 2 値と、chat の行だけが持つ `session`・`detail`（逐語）は必須。
fn utterance_of(pairs: &[(String, Value)]) -> Result<(Case, Option<String>), String> {
    let text = word_of(pairs, "channel")?;
    let channel = Channel::parse(&text).ok_or(format!("channel {text} は chat / gui でない"))?;
    let session = optional_word(pairs, "session")?;
    match (channel, &session) {
        (Channel::Chat, None) => return Err("channel chat の行に session が無い".to_owned()),
        (Channel::Gui, Some(_)) => return Err("channel gui の行は session を持たない".to_owned()),
        (Channel::Chat, Some(_)) | (Channel::Gui, None) => {}
    }
    text_of(field(pairs, "detail"), "detail")?;
    Ok((Case::Utterance { channel, session }, None))
}

/// UtteranceSorted: `utterance` と `sorting` の 2 値・request の行だけが持つ `bead`（開いた memo の id）。
fn sorted_of(pairs: &[(String, Value)]) -> Result<(Case, Option<String>), String> {
    let utterance = word_of(pairs, "utterance")?;
    let text = word_of(pairs, "sorting")?;
    let sorting = Sorting::parse(&text).ok_or(format!("sorting {text} は request / chat でない"))?;
    let bead = optional_word(pairs, "bead")?;
    match (sorting, &bead) {
        (Sorting::Request, None) => return Err("sorting request の行に bead が無い".to_owned()),
        (Sorting::Chat, Some(_)) => return Err("sorting chat の行は bead を持たない".to_owned()),
        (Sorting::Request, Some(_)) | (Sorting::Chat, None) => {}
    }
    Ok((Case::Sorted { utterance, sorting }, bead))
}

/// TurnEndUnjudged: `reason` は必須・`session` は任意。
fn turn_end_of(pairs: &[(String, Value)]) -> Result<(Case, Option<String>), String> {
    let session = optional_word(pairs, "session")?;
    Ok((Case::TurnEnd { session, reason: word_of(pairs, "reason")? }, None))
}

/// IntakeRefused: `bead`（契約の id）と `refuse`（受付の断りの名）が必須。
fn refused_of(pairs: &[(String, Value)]) -> Result<(Case, Option<String>), String> {
    let bead = word_of(pairs, "bead")?;
    Ok((Case::Refused { refuse: word_of(pairs, "refuse")? }, Some(bead)))
}

/// MemoJudged: `bead`（memo の id）と空でない `detail`（判定の語）が必須（本体の欄は持たない）。
fn judged_of(pairs: &[(String, Value)]) -> Result<(Case, Option<String>), String> {
    let bead = word_of(pairs, "bead")?;
    if text_of(field(pairs, "detail"), "detail")?.is_empty() {
        return Err("detail が空".to_owned());
    }
    Ok((Case::Judged, Some(bead)))
}

/// LifecycleCutover: `version` と `main`（小文字の 16 進）が必須。
fn cutover_of(pairs: &[(String, Value)]) -> Result<(Case, Option<String>), String> {
    let version = word_of(pairs, "version")?;
    let main = word_of(pairs, "main")?;
    if !main.chars().all(|ch| matches!(ch, '0'..='9' | 'a'..='f')) {
        return Err(format!("main {main} は小文字の 16 進でない"));
    }
    Ok((Case::Cutover { version, main }, None))
}

/// 必須の空でない文字列 field（空は key を名指して `Err`）。
fn word_of(pairs: &[(String, Value)], key: &str) -> Result<String, String> {
    let text = text_of(field(pairs, key), key)?;
    if text.is_empty() {
        return Err(format!("{key} が空"));
    }
    Ok(text)
}

/// 任意の空でない文字列 field（key が在って文字列でないか空なら `Err`）。
fn optional_word(pairs: &[(String, Value)], key: &str) -> Result<Option<String>, String> {
    match field(pairs, key) {
        None => Ok(None),
        Some(_) => word_of(pairs, key).map(Some),
    }
}

impl Event {
    /// install の行の本体（[`EventKind::InstallRecorded`] でだけ `Some`）。
    pub fn install(&self) -> Option<Install> {
        match self.kind.shape() {
            Shape::Install => self.detail.as_deref().and_then(Install::parse),
            Shape::Run
            | Shape::Allowance
            | Shape::Registration
            | Shape::Account
            | Shape::Mark
            | Shape::Cost
            | Shape::Ruling
            | Shape::Pressure
            | Shape::Group
            | Shape::Case => None,
        }
    }

    /// 群の逼迫の通知の本体（[`EventKind::GroupPressureNotified`] でだけ `Some`）。
    pub fn pressure(&self) -> Option<Pressure> {
        match self.kind.shape() {
            Shape::Pressure => self.detail.as_deref().and_then(Pressure::parse),
            Shape::Run
            | Shape::Allowance
            | Shape::Registration
            | Shape::Account
            | Shape::Mark
            | Shape::Install
            | Shape::Cost
            | Shape::Ruling
            | Shape::Group
            | Shape::Case => None,
        }
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

#[cfg(test)]
mod tests {
    use super::{Event, BOUND_KEYS};
    use crate::fleet::{Case, Channel, EventKind};

    /// 裁定の行の頭（kind と actor と host は固定・detail は逐語）。`extra` は本体の key の列（`,` で始める）。
    fn line(extra: &str) -> String {
        format!(r#"{{"schema":1,"ts":"2026-09-30T07:06:00Z","kind":"RulingReceived"{extra},"host":"h","actor":"human","detail":"推奨で"}}"#)
    }

    /// 結びの形の本体（5 key）。
    const BOUND: &str = r#","bead":"s2-q1","ruling":"s2-q1:20260930T0705Z-1","utterance":"2026-09-30T07:05:09.123Z","channel":"chat","question_ts":"2026-09-30T06:00:00Z","asked":"seat""#;

    /// 新しい形の行は 5 key を `Case::Ruling` として読み、書き戻すと同じ行になる（asked の無い形は asked の key ごと書かない）。
    #[test]
    fn fleet_ruling_body_bound_line_round_trips() {
        let text = line(BOUND);
        let event = Event::from_line(&text).unwrap_or_else(|err| panic!("新しい形を読める: {err}"));
        let case = Case::Ruling {
            ruling: "s2-q1:20260930T0705Z-1".to_owned(),
            utterance: "2026-09-30T07:05:09.123Z".to_owned(),
            channel: Channel::Chat,
            question_ts: "2026-09-30T06:00:00Z".to_owned(),
            asked: Some("seat".to_owned()),
        };
        assert_eq!((event.kind, event.bead.as_str(), event.rule.as_ref()), (EventKind::RulingReceived, "s2-q1", None));
        assert_eq!(event.case, Some(case), "本体");
        assert_eq!(event.to_line(), text, "書き戻すと同じ行（key の並びは ruling・utterance・channel・question_ts・asked）");
        let without = line(&BOUND.replace(r#","asked":"seat""#, ""));
        let plain = Event::from_line(&without).unwrap_or_else(|err| panic!("asked の無い形を読める: {err}"));
        assert!(matches!(&plain.case, Some(Case::Ruling { asked: None, .. })), "asked は None: {:?}", plain.case);
        assert_eq!(plain.to_line(), without, "asked の key を書かない");
    }

    /// `rule` との併せ持ちは key を名指して malformed（5 key のどれと併せても）。結びの行に bead（問い id）が無い・必須の key が欠ける・
    /// channel か asked が閉じた値の外・結びの key が別の kind の行に在る周も malformed。
    #[test]
    fn fleet_ruling_body_refuses_rule_alongside_bound_keys_and_malformed_bound_lines() {
        for key in BOUND_KEYS {
            let text = line(&format!(r#","bead":"s2-q1","rule":"R-C9-1","{key}":"x""#));
            let err = Event::from_line(&text).expect_err("rule と併せ持つ行は読めない");
            assert!(err.contains("rule") && err.contains(key), "{key}: key を名指す: {err}");
        }
        assert!(Event::from_line(&line(&format!(r#"{BOUND},"rule":"R-C9-1""#))).is_err(), "5 key と rule");
        let broken = [
            BOUND.replace(r#""bead":"s2-q1","#, ""),
            BOUND.replace(r#","question_ts":"2026-09-30T06:00:00Z""#, ""),
            BOUND.replace(r#""ruling":"s2-q1:20260930T0705Z-1","#, ""),
            BOUND.replace(r#""channel":"chat""#, r#""channel":"phone""#),
            BOUND.replace(r#""asked":"seat""#, r#""asked":"nobody""#),
            BOUND.replace(r#""asked":"seat""#, r#""asked":"""#),
        ];
        for extra in broken {
            assert!(Event::from_line(&line(&extra)).is_err(), "読めない形: {extra}");
        }
        for key in ["ruling", "question_ts", "asked"] {
            let text = format!(r#"{{"schema":1,"ts":"t","kind":"UtteranceReceived","channel":"gui","{key}":"x","host":"h","actor":"human","detail":"d"}}"#);
            let err = Event::from_line(&text).expect_err("結びの key は裁定の kind だけ");
            assert!(err.contains(key), "{key}: {err}");
        }
    }

    /// `rule` だけの古い行は今までどおり読め（本体は無い）、書き戻すと同じ行になる。
    #[test]
    fn fleet_ruling_body_reads_the_old_rule_only_line() {
        let text = line(r#","bead":"s2-x.1","rule":"R-C9-1""#);
        let event = Event::from_line(&text).unwrap_or_else(|err| panic!("古い形を読める: {err}"));
        assert_eq!((event.rule.as_deref(), event.bead.as_str()), (Some("R-C9-1"), "s2-x.1"));
        assert_eq!(event.case, None, "本体は無い");
        assert_eq!(event.to_line(), text, "書き戻すと同じ行");
        let bare = Event::from_line(&line("")).unwrap_or_else(|err| panic!("rule も bead も無い古い行を読める: {err}"));
        assert_eq!((bare.rule, bare.case, bare.bead.as_str()), (None, None, ""));
    }
}
