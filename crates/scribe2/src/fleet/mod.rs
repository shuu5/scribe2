//! 便と席の現在地を、追記だけの event log を replay して読む面（憲法 C3・設計 §3 / §4）。
//!
//! 状態は process の記憶でなく永続面に在る。各段は log を読んで現在地を得て、段の
//! 終わりに event を 1 件追記して終わる。字面との変換は wildcard 無しの `match` に
//! 閉じ、variant を足したら compile error になる形を保つ（C1 / C11）。

pub mod cli;
pub mod json_lite;
pub mod store;

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
            | Self::ApprovalRequested => ACTOR_MACHINE,
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

/// log の 1 行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// schema 版。
    pub schema: u64,
    /// UTC の時刻（`YYYY-MM-DDTHH:MM:SSZ`）。
    pub ts: String,
    /// 種類。
    pub kind: EventKind,
    /// 便 id。
    pub run: String,
    /// 契約の bead id（台帳は読まない・文字列として持つだけ）。
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
}

impl Event {
    /// 1 行の JSON にする。
    pub fn to_line(&self) -> String {
        let mut pairs: Vec<(&str, Value)> = vec![
            ("schema", Value::Num(self.schema)),
            ("ts", Value::Str(self.ts.clone())),
            ("kind", Value::Str(self.kind.as_str().to_owned())),
            ("run", Value::Str(self.run.clone())),
            ("bead", Value::Str(self.bead.clone())),
            ("host", Value::Str(self.host.clone())),
            ("actor", Value::Str(self.actor.clone())),
        ];
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
    pub fn from_line(line: &str) -> Result<Self, String> {
        let pairs = json_lite::parse_object(line)?;
        for (key, _) in &pairs {
            if !KNOWN_KEYS.contains(&key.as_str()) {
                return Err(format!("未知の key {key}"));
            }
        }
        let get = |key: &str| pairs.iter().find(|(found, _)| found == key).map(|(_, v)| v);
        let schema = get("schema").and_then(Value::as_num).ok_or("schema が無い")?;
        if schema != SCHEMA {
            return Err(format!("schema が {SCHEMA} でない（実 {schema}）"));
        }
        let kind_text = text_of(get("kind"), "kind")?;
        let kind = EventKind::parse(&kind_text).ok_or(format!("kind {kind_text} は未知である"))?;
        let actor = text_of(get("actor"), "actor")?;
        let actor = parse_actor(&actor)
            .ok_or(format!("actor {actor} は machine でも human でもない"))?
            .to_owned();
        Ok(Self {
            schema,
            ts: text_of(get("ts"), "ts")?,
            kind,
            run: text_of(get("run"), "run")?,
            bead: text_of(get("bead"), "bead")?,
            host: text_of(get("host"), "host")?,
            actor,
            stage: optional_stage(get("stage"))?,
            seat: get("seat").and_then(Value::as_str).map(str::to_owned),
            pid: get("pid").and_then(Value::as_num),
            detail: get("detail").and_then(Value::as_str).map(str::to_owned),
        })
    }
}

/// 必須の文字列 field を取り出す。
fn text_of(value: Option<&Value>, key: &str) -> Result<String, String> {
    value
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(format!("{key} が無いか文字列でない"))
}

/// 任意の `stage` を読む。字面が未知なら `Err`（黙って落とさない）。
fn optional_stage(value: Option<&Value>) -> Result<Option<Stage>, String> {
    match value.and_then(Value::as_str) {
        None => Ok(None),
        Some(text) => Stage::parse(text)
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
}

/// event の並びから現在地を導く。物理順で後の event が勝つ。
pub fn replay(events: &[Event]) -> State {
    let mut state = State::default();
    for event in events {
        apply_run(&mut state, event);
        apply_seat(&mut state, event);
    }
    state
}

/// 1 件の event を便へ反映する。
fn apply_run(state: &mut State, event: &Event) {
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
    if event.kind == EventKind::ApprovalReceived {
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
        | EventKind::ApprovalReceived => {}
    }
}

/// 待つ対象。**述語を受ける口は作らない**（C3.4: 待機は 1 実装）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    /// runner の process が終わること。
    RunnerExited(u32),
    /// 席の process が消えること（TERM の後）。
    SeatGone(u32),
}

impl Completion {
    /// 見張る pid。
    pub fn pid(self) -> u32 {
        match self {
            Self::RunnerExited(pid) | Self::SeatGone(pid) => pid,
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
/// process の生存は `/proc/<pid>` の有無で見る（libc を足さないため・NFR3）。
pub fn wait(completion: Completion, deadline: Duration) -> Result<(), Timeout> {
    let started = Instant::now();
    loop {
        if !pid_is_live(completion.pid()) {
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
