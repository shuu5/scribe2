//! event の並びから現在地（[`State`]）を導く replay（設計 fleet-event-log.md §4・憲法 C3）。
//!
//! 便用の口座の選定（[`select_for_run`]）は待ちの観測と再開が同じ 1 本を呼ぶので、ここに置く
//! （`s2-07l.260` で挙動不変に分割・外の呼び手の path は `fleet` の再 export で保つ）。

use super::{
    select, AllowanceKey, AllowanceLatest, Event, EventKind, RegistrationLatest, SeatState, Stage, ACTOR_HUMAN,
};
use crate::rules::manifest::Manifest;
use crate::seat::role::Role;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::Path;

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
    /// 便を起こした口座（最新の `SeatSpawned` の `account`・ADR-0027 §2.3）。field の無い行で起こした便は `None`
    /// ＝走行中の便数に数えない。
    pub account: Option<String>,
}

impl Run {
    /// 走行中か（終端の段 `Landed` / `Failed` / `Stopped` でなく、畳まれても〔`detail=retired`〕いない・pipeline.md §4）。
    pub fn is_inflight(&self) -> bool {
        let terminal = matches!(self.stage, Stage::Landed | Stage::Failed | Stage::Stopped);
        !terminal && self.detail.as_deref() != Some("retired")
    }
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
    /// 退役中の口座 label → その `AccountRetired` の ts（最後の Retired の後に Restored が無い label だけ・
    /// 設計 account-lifecycle.md §3）。ts は退役先 `.retired/<label>.<ts>` を名指す（dir を走査しない・C3）。
    pub retired: BTreeMap<String, String>,
}

impl State {
    /// `labels`（宣言順）から退役中の label を除いた列（**有効な口座の集合の本体**・[`effective_accounts`] と
    /// label 列しか持たない呼び手〔tick・便用の選定〕が同じここを通る）。
    pub fn without_retired<'a>(&self, labels: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        labels.into_iter().filter(|label| !self.retired.contains_key(*label)).map(str::to_owned).collect()
    }

    /// 席の登録 row が持つ口座 label の集合（便用の選定の除外集合・設計 account-autonomy.md §3 / §14）。
    ///
    /// **席の生死を問わない**——登録が在る限りその口座は席のものである（便が席の口座を食い潰す穴を
    /// 塞ぐのが除外の目的で、席が一時的に落ちている周に便がその口座を取ると、立て直しの口座が無い）。
    ///
    /// `anchor` は絞り（§14・FR40 の席の識別子は (役割, anchor)）: `None` は置き場の全 row の口座（退役の検査・
    /// `--anchor` 無しの `fleet select`＝保守側）、`Some(repo)` は `Registration.anchor` が `repo` と **`OsStr` の
    /// 等値**で一致する row の口座だけ（正規化も component の比較もしない＝登録が書いた値がそのまま鍵）。置き場を
    /// 共有する他 repo の席の口座は本 repo の便の候補に残る（user 裁定 2026-09-16・A1）。
    pub fn registered_accounts(&self, anchor: Option<&Path>) -> BTreeSet<String> {
        self.registrations
            .values()
            .filter(|latest| anchor.is_none_or(|repo| OsStr::new(&latest.registration.anchor) == repo.as_os_str()))
            .map(|latest| latest.registration.account.clone())
            .collect()
    }

    /// 口座 label → 走行中の便数（**導出値**・憲法 C10・ADR-0027 §2.3）。数えるのは [`Run::is_inflight`] な便のうち
    /// [`Run::account`] を持つものだけ（口座不明の便は 0）。便用の選定の 2 つ目の鍵（設計 account-autonomy.md §3）。
    pub fn inflight_by_account(&self) -> BTreeMap<String, usize> {
        let mut found = BTreeMap::new();
        for label in self.runs.values().filter(|run| run.is_inflight()).filter_map(|run| run.account.as_ref()) {
            *found.entry(label.clone()).or_insert(0) += 1;
        }
        found
    }
}

/// 口座 label の credential dir（`<state_dir>/accounts/<label>`・ADR-0017 §2.3）。runner の `--account-dir`
/// に渡す値で、[`usage`] が読む credential file はこの dir の直下に在る。
pub fn account_dir(state_dir: &std::path::Path, label: &str) -> std::path::PathBuf {
    state_dir.join("accounts").join(label)
}

/// **有効な口座の集合**（設計 account-lifecycle.md §3・宣言順）= 宣言（tracked + host の面の `[[account]]`）− 退役中。
/// 計測（`fleet usage`）・選定（`fleet select`）・tick の逼迫度・doctor / `account ls` の `retired=` はここを読む
/// （退役中の口座は測らず選ばない・宣言の行は消さない＝退役は event log の状態・C3）。
pub fn effective_accounts(manifest: &Manifest, state: &State) -> Vec<String> {
    state.without_retired(manifest.accounts().iter().map(|account| account.label()))
}

/// 便用の規則で口座を 1 つ選ぶ（設計 account-autonomy.md §3 / §4）。**便の再開と待ちの観測が同じ
/// 1 本を呼ぶ**（[`Completion::AccountFree`] の `is_met` と `pipe resume` の選定が別の入力を組まない）。
///
/// `model` は rules 行 `runner.model` の値（runner / lens が `--model` で毎回明示する model・設計 §3・`s2-07l.297`）
/// ＝便が消費するのはその model のモデル別窓だけなので、他の model の窓が 100 でも候補から外さない。字面のまま
/// 渡し、型にするのは `select` の中（別名 × 表示名の照合）。`None` は全 model 窓の最大（保守側）。除外は**便の repo
/// （`repo`・`pipe run --repo` の値）を anchor に持つ**登録 row の口座だけ（[`State::registered_accounts`]・設計 §14＝
/// 置き場を共有する他 repo の席の口座は候補）。走行中の便数は state から導く（[`State::inflight_by_account`]・呼び手は
/// 渡さない）。閾値は便用の規則が持たないので**窓の全量**（[`select::LIMIT_PCT`]）を置く＝session 用の分岐に届かない
/// 値であって、R-C9-1 の値ではない。
pub fn select_for_run(
    state: &State,
    repo: &Path,
    labels: &[String],
    model: Option<&str>,
    now: &str,
) -> select::Selection {
    let labels = state.without_retired(labels.iter().map(String::as_str));
    select::select(&select::Input {
        labels: &labels,
        allowance: &state.allowance,
        purpose: select::Purpose::Run,
        model,
        exclude: &state.registered_accounts(Some(repo)),
        inflight: &state.inflight_by_account(),
        threshold_pct: select::LIMIT_PCT,
        now,
        // 便用は留まる口座を読まない（session 用の規則・`s2-07l.312`）。
        prefer: None,
    })
}

/// event の並びから現在地を導く。物理順で後の event が勝つ。
pub fn replay(events: &[Event]) -> State {
    let mut state = State::default();
    for (seq, event) in events.iter().enumerate() {
        apply_run(&mut state, event);
        apply_seat(&mut state, event);
        apply_allowance(&mut state, event);
        apply_account(&mut state, event);
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

/// 1 件の event を退役の集合へ反映する（最後の Retired の後に Restored が無い label だけが残る）。
fn apply_account(state: &mut State, event: &Event) {
    let Some(label) = &event.account else {
        return;
    };
    match event.kind {
        EventKind::AccountRetired => {
            state.retired.insert(label.clone(), event.ts.clone());
        }
        EventKind::AccountRestored => {
            state.retired.remove(label);
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
        | EventKind::QuestionAnswered
        | EventKind::AllowanceMeasured
        | EventKind::AllowanceUnmeasured
        | EventKind::SeatRegistered => {}
    }
}

/// 1 件の event を便へ反映する。
fn apply_run(state: &mut State, event: &Event) {
    // 口座残量・登録・退役の行は便に紐づかない（`run` / `bead` を持たない）。ここで通すと id が空の
    // 幽霊の便が 1 つ生まれ、`show` / `export` の件数が実在しない便を数える。登録と退役は **kind で
    // 見分ける**（本体の有無ではない＝退役した役割の登録 row は本体を持たずに読まれる〔`Event::from_line`〕
    // ので、本体で見分けると幽霊の便が 1 つ生まれる・`account` の有無でもない＝口座つきの `SeatSpawned` は
    // 便に紐づく行・ADR-0027 §2.3）。
    if event.kind.is_allowance() || event.kind == EventKind::SeatRegistered || event.kind.is_account_lifecycle() {
        return;
    }
    let run = state.runs.entry(event.run.clone()).or_insert_with(|| Run {
        id: event.run.clone(),
        bead: event.bead.clone(),
        stage: Stage::Intake,
        updated: event.ts.clone(),
        detail: None,
        approved: false,
        account: None,
    });
    run.bead = event.bead.clone();
    run.updated = event.ts.clone();
    if let Some(stage) = event.stage {
        run.stage = stage;
    }
    if event.detail.is_some() {
        run.detail = event.detail.clone();
    }
    // 便を起こした口座は最新の `SeatSpawned` が持つ値（field の無い行で起こし直した周は不明に戻る）。
    if event.kind == EventKind::SeatSpawned {
        run.account = event.account.clone();
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
        | EventKind::SeatRegistered
        | EventKind::AccountRetired
        | EventKind::AccountRestored => {}
    }
}
