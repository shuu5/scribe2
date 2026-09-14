//! event の並びから現在地（[`State`]）を導く replay（設計 fleet-event-log.md §4・憲法 C3）。
//!
//! 便用の口座の選定（[`select_for_run`]）は待ちの観測と再開が同じ 1 本を呼ぶので、ここに置く
//! （`s2-07l.260` で挙動不変に分割・外の呼び手の path は `fleet` の再 export で保つ）。

use super::{
    select, AllowanceKey, AllowanceLatest, Event, EventKind, RegistrationLatest, SeatState, Stage, ACTOR_HUMAN,
};
use crate::seat::role::Role;
use std::collections::{BTreeMap, BTreeSet};

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
    /// (役割, anchor) → 最新の登録（設計 seat-roles.md §2・読み手は [`crate::seat::role`]）。
    pub registrations: BTreeMap<(Role, String), RegistrationLatest>,
}

impl State {
    /// 席の登録 row が持つ口座 label の集合（便用の選定の除外集合・設計 account-autonomy.md §3）。
    ///
    /// **席の生死を問わない**——登録が在る限りその口座は席のものである（便が席の口座を食い潰す穴を
    /// 塞ぐのが除外の目的で、席が一時的に落ちている周に便がその口座を取ると、立て直しの口座が無い）。
    pub fn registered_accounts(&self) -> BTreeSet<String> {
        self.registrations
            .values()
            .map(|latest| latest.registration.account.clone())
            .collect()
    }
}

/// 口座 label の credential dir（`<state_dir>/accounts/<label>`・ADR-0017 §2.3）。runner の `--account-dir`
/// に渡す値で、[`usage`] が読む credential file はこの dir の直下に在る。
pub fn account_dir(state_dir: &std::path::Path, label: &str) -> std::path::PathBuf {
    state_dir.join("accounts").join(label)
}

/// 便用の規則で口座を 1 つ選ぶ（設計 account-autonomy.md §3 / §4）。**便の再開と待ちの観測が同じ
/// 1 本を呼ぶ**（[`Completion::AccountFree`] の `is_met` と `pipe resume` の選定が別の入力を組まない）。
///
/// model は渡さない（runner の起動形は model を渡さず claude の既定＝モデル別窓すべての最大を数える
/// 保守側）。除外は登録 row の口座。閾値は便用の規則が持たないので**窓の全量**（[`select::LIMIT_PCT`]）
/// を置く＝session 用の分岐に届かない値であって、R-C9-1 の値ではない。
pub fn select_for_run(state: &State, labels: &[String], now: &str) -> select::Selection {
    select::select(&select::Input {
        labels,
        allowance: &state.allowance,
        purpose: select::Purpose::Run,
        model: None,
        exclude: &state.registered_accounts(),
        threshold_pct: select::LIMIT_PCT,
        now,
    })
}

/// event の並びから現在地を導く。物理順で後の event が勝つ。
pub fn replay(events: &[Event]) -> State {
    let mut state = State::default();
    for (seq, event) in events.iter().enumerate() {
        apply_run(&mut state, event);
        apply_seat(&mut state, event);
        apply_allowance(&mut state, event);
        // 登録は同じ鍵を後の行が置き換える（前の行は log に残る・append のみ）。
        if let Some(found) = &event.registration {
            let latest = RegistrationLatest { seq, registration: found.clone() };
            state.registrations.insert((found.role, found.anchor.clone()), latest);
        }
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
    if event.kind.is_allowance() || event.registration.is_some() {
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
        | EventKind::AllowanceUnmeasured
        | EventKind::SeatRegistered => {}
    }
}
