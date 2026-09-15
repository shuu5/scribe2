//! 席の**立て直し**（[`relaunch`]・設計 account-autonomy.md §5・SRS FR38）と、立て直しと `seat launch` が共有する起動の
//! 1 本（[`boot`]）・退避後の終了の手（[`send_exit`]・`s2-07l.252`）。[`super`] から純移動（`s2-07l.319`）。判定順・注入の
//! 字面・記録の行は不変で、lock・cycle-stamp・立ち上がりの確認（[`super::started`]）は親の 1 本を使う。

use super::launch::{fill_launch, model_of, single_model, with_agent_view_off, with_model, Holes};
use super::{
    lock_path, send_to, started, take_lock, ttl_s, write_stamp, Lock, ACCOUNTS_DIR, DEFAULT_RESTORE, REASON_INPUT_BUSY,
    REASON_INPUT_UNKNOWN, REASON_LAUNCH, REASON_LOCK_HELD, REASON_PANE_MISSING, REASON_REGISTER, REASON_RESTORE,
    REASON_ROW_MODEL_UNKNOWN, REASON_STAMP, REASON_STATE_DIR, WHEN_EXIT, WHEN_RELAUNCH, WHO,
};
use crate::fleet::select::{self, NoCandidate, Purpose, Selection};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{Registration, State};
use crate::hook::{seat_name, InjectionRecord, SCHEMA};
use crate::seat::role::Role;
use crate::seat::{inject, role, state, StateDir};
use std::collections::BTreeSet;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// 立て直し 1 回の入力（[`relaunch`]・入口の 3 条件と back-off は tick が見る）。
pub struct Relaunch<'a> {
    /// tmux target（shell へ戻った pane）。
    pub target: &'a str,
    /// tmux の socket。
    pub socket: Option<&'a str>,
    /// 解決済みの置き場（credential dir と event log の置き場）。
    pub state_dir: &'a StateDir,
    /// 復元 command（既定 [`DEFAULT_RESTORE`]＝FR28）。
    pub restore: Option<&'a str>,
    /// 立ち上がりと復元の確認上限（rules 行 `seat.cycle_settle_s`）。
    pub settle: Duration,
    /// 確認の周期（rules 行 `seat.cycle_poll_ms`）。
    pub step: Duration,
    /// 自席の登録 row（雛形・`model`・更新で写す項目の出所）。
    pub row: &'a Registration,
    /// replay の現在地（実測行と他の席の登録 row）。
    pub state: &'a State,
    /// 口座 label の列（manifest の `[[account]]`・宣言値）。
    pub labels: &'a [String],
    /// R-C9-1 の値（session 用の閾値）。
    pub threshold_pct: u64,
}

/// 立て直し 1 回の結果。**「送っていない」と「送ったが確かめられない」を分ける**（[`super::Cycle`] と同じ）。
pub enum Relaunched {
    /// 起動と復元を注入して復元の消費を確かめ、登録 row の口座を更新した（選んだ label・復元の消費）。
    Done(String, inject::Settled),
    /// 選べる口座が無い（**1 key も送っていない**・次の tick で選び直す）。
    None(NoCandidate),
    /// **1 key も送っていない**。
    Refused(&'static str),
    /// 送ったが確かめられない。
    Failed(&'static str),
}

/// 席を別口座で立て直す（設計 account-autonomy.md §5）: session 用の規則で口座を選び（[`choose`]）、雛形の穴を
/// 選んだ口座の credential dir（`<state_dir>/accounts/<label>`）で埋めて同じ target の shell へ注入し、立ち上がりを
/// 打刻で確かめて登録 row の口座を更新し、復元の command を注入する。
///
/// **cycle と同じ lock と cycle-stamp**（`s2-07l.110`・write-ahead）: lock を取った周は送る前に打刻し、打てない周は
/// 1 key も送らない＝立て直しの再注入の back-off は tick が読む既存の 1 本である。選べない・雛形の穴が 1 つでない
/// 周は lock も打刻も取らない（送っていない）。
pub fn relaunch(request: &Relaunch) -> Relaunched {
    let own = (request.row.role, request.row.anchor.as_str(), Some(request.row.account.as_str()));
    let label = match choose(own, request.state, request.labels, request.row.model.as_deref(), request.threshold_pct) {
        Selection::Chosen(label) => label,
        Selection::None(found) => return Relaunched::None(found),
    };
    // 注入するのは row の model を運ばせ穴を埋めた雛形に agent view off を前置した 1 行（記録にも同じ行が載る）。
    let Ok(model) = model_of(request.row.model.as_deref()) else { return Relaunched::Refused(REASON_ROW_MODEL_UNKNOWN) };
    let launch = match launch_line(request.state_dir, &with_model(&request.row.launch, model), &label) {
        Ok(found) => found,
        Err(reason) => return Relaunched::Refused(reason),
    };
    let ttl = match ttl_s() {
        Ok(found) => found,
        Err(read) => return Relaunched::Refused(read.no_rule()),
    };
    let dir = crate::seat::seat_dir(&request.state_dir.path, request.target);
    match take_lock(&dir, ttl) {
        Lock::Taken => {}
        Lock::Held => return Relaunched::Refused(REASON_LOCK_HELD),
        Lock::Broken => return Relaunched::Refused(REASON_STATE_DIR),
    }
    let held = if write_stamp(&dir).is_ok() {
        relaunch_held(request, &dir, &launch, &label)
    } else {
        Relaunched::Refused(REASON_STAMP)
    };
    std::fs::remove_file(lock_path(&dir)).ok();
    held
}

/// session 用の選定（[`select::select`] の 1 本・立て直しと `seat launch` の初回の選定が同じ関数を呼ぶ）。`own` は
/// 自席の鍵 (role, anchor) と留まる口座（立て直しは自席の row の口座・初回の起動は row が無いので `None`・
/// ADR-0028 §2.4・`s2-07l.312`）・`model` は席の model。除外は**他の席の**登録 row が持つ口座で、自席の row（同じ鍵）は
/// 入れない（account-autonomy.md §5 / account-lifecycle.md §4）。
pub(super) fn choose(own: (Role, &str, Option<&str>), state: &State, labels: &[String], model: Option<&str>, threshold_pct: u64) -> Selection {
    let (role, anchor, prefer) = own;
    let exclude: BTreeSet<String> = state
        .registrations
        .values()
        .map(|latest| &latest.registration)
        .filter(|found| (found.role, found.anchor.as_str()) != (role, anchor))
        .map(|found| found.account.clone())
        .collect();
    let now = crate::fleet::cli::now_utc();
    select::select(&select::Input {
        labels,
        allowance: &state.allowance,
        purpose: Purpose::Session,
        model,
        exclude: &exclude,
        // session 用は走行中の便数を読まない（便用の鍵・ADR-0027 §2.2）。
        inflight: &std::collections::BTreeMap::new(),
        threshold_pct,
        now: &now,
        prefer,
    })
}

/// 雛形 `template` の `--model` を高々 1 つと確かめ（[`single_model`]）、穴を口座 `label` の credential dir（`<state_dir>/accounts/<label>`）で埋め、agent view off を前置した起動の 1 行（立て直しと `seat launch` の同じ 1 つ・記録にも同じ行が載る）。断りは字面。
pub(super) fn launch_line(state_dir: &StateDir, template: &str, label: &str) -> Result<String, &'static str> {
    single_model(template)?;
    let account_dir = state_dir.path.join(ACCOUNTS_DIR).join(label);
    fill_launch(template, &account_dir.display().to_string()).map(|found| with_agent_view_off(&found)).map_err(Holes::as_str)
}

/// lock を握っている間の手順: 起動の 1 本（[`boot`]）に「立ち上がりの直後の登録 row の更新」を挟む。
///
/// 登録 row は**立ち上がりを確かめた直後**に更新する: 起動が届いた席は選んだ口座で走っており、復元を
/// 確かめられない周でも row が旧い口座を名乗ると、選定の除外と次の周の逼迫度が別の口座を見る。
/// 成功と数えるのは復元が**消費された**周だけ（[`super::send_restore`] と同じ・立ち上がった直後の席に turn は無い）。
fn relaunch_held(request: &Relaunch, dir: &Path, launch: &str, label: &str) -> Relaunched {
    let common = Boot {
        target: request.target,
        socket: request.socket,
        state_dir: request.state_dir,
        restore: Some(request.restore.unwrap_or(DEFAULT_RESTORE)),
        settle: request.settle,
        step: request.step,
    };
    let relabel = || role::relabel(&request.state_dir.path, request.row, label).map(|_| ()).map_err(|_| REASON_REGISTER);
    match boot(&common, dir, (launch, WHEN_RELAUNCH), relabel) {
        Booted::Done(Some(inject::Settled::Consumed)) => Relaunched::Done(label.to_owned(), inject::Settled::Consumed),
        Booted::Done(_) => Relaunched::Failed(REASON_RESTORE),
        Booted::Refused(reason) => Relaunched::Refused(reason),
        Booted::Failed(reason) => Relaunched::Failed(reason),
    }
}

/// 起動の注入の共通の入力（立て直し [`Relaunch`] と席の起動 [`super::launch::Launch`] が同じ 1 本 [`boot`] に渡す）。
pub(super) struct Boot<'a> {
    /// tmux target（前面が shell の pane）。
    pub(super) target: &'a str,
    /// tmux の socket。
    pub(super) socket: Option<&'a str>,
    /// 解決済みの置き場（打刻と記録の置き場）。
    pub(super) state_dir: &'a StateDir,
    /// 立ち上がった後に送る復元 command（`None` は送らない＝`seat launch` の `--restore` 無し）。
    pub(super) restore: Option<&'a str>,
    /// 立ち上がりと復元の確認上限（rules 行 `seat.cycle_settle_s`）。
    pub(super) settle: Duration,
    /// 確認の周期（rules 行 `seat.cycle_poll_ms`）。
    pub(super) step: Duration,
}

/// 起動の 1 本の結果（**「送っていない」と「送ったが確かめられない」を分ける**・[`Relaunched`] と同じ極性）。
pub(super) enum Booted {
    /// 起動が届いて立ち上がりを確かめた（復元を送った周はその消費・送らない周は `None`）。
    Done(Option<inject::Settled>),
    /// **1 key も送っていない**。
    Refused(&'static str),
    /// 送ったが確かめられない。
    Failed(&'static str),
}

/// 起動の注入の **1 本**（順序固定・立て直しと `seat launch` の共通の経路・account-lifecycle.md §4）: shell の入力欄の門 →
/// 起動行（`(line, when)`）を送って記録 → 立ち上がりの確認 → `between`（立て直しは登録 row の更新・起動は何もしない）→
/// 復元。
///
/// 起動は**前面が shell の pane** へ撃つので、門は席の `❯` の行（[`inject::guard_input`]）でなく shell の prompt 末尾
/// （[`crate::seat::shell_input_empty`]・account-autonomy.md §5「shell への注入の門」・`s2-07l.218`）で見る。断りの字面は
/// cycle の門と同じ `input-busy` / `input-unknown`。立ち上がった後の復元は席の pane なので従来どおり注入の門を通る。
pub(super) fn boot(common: &Boot, dir: &Path, (line, when): (&str, &str), between: impl FnOnce() -> Result<(), &'static str>) -> Booted {
    let Some(pane) = crate::seat::tmux_stdout(common.socket, &["capture-pane", "-p", "-J", "-t", common.target]) else {
        return Booted::Refused(REASON_PANE_MISSING);
    };
    match crate::seat::shell_input_empty(&pane) {
        Ok(()) => {}
        Err(inject::InputGate::Busy) => return Booted::Refused(REASON_INPUT_BUSY),
        Err(inject::InputGate::UnknownInput) => return Booted::Refused(REASON_INPUT_UNKNOWN),
    }
    let baseline = state::baseline(dir);
    let since = state::now_secs();
    let started_at = Instant::now();
    if !send_to(common.socket, common.target, line) {
        return Booted::Failed(inject::REASON_TMUX_FAILED);
    }
    record_sent(common.state_dir, common.target, (line, when), started_at);
    if !started(dir, (baseline, since), common.settle, common.step) {
        return Booted::Failed(REASON_LAUNCH);
    }
    if let Err(reason) = between() {
        return Booted::Failed(reason);
    }
    match common.restore {
        None => Booted::Done(None),
        Some(payload) => restore_when_ready(common, payload).map_or(Booted::Failed(REASON_RESTORE), |settled| Booted::Done(Some(settled))),
    }
}

/// 復元 command を注入する（FR28・立て直しの既定は [`DEFAULT_RESTORE`]）。立ち上がった直後の席は入力欄を描き終える前が
/// あり、入力欄の門は **1 key も送らずに断る**ので、窓（`settle`）の内は刻み（`step`）ごとに送り直す（断りは送って
/// いない＝二重投函にならない）。送達した周の消費を返し、窓の内に送達できない周は `None`。
fn restore_when_ready(common: &Boot, payload: &str) -> Option<inject::Settled> {
    let deadline = Instant::now().checked_add(common.settle);
    loop {
        match inject::deliver_within(&sending(common, payload), common.settle) {
            inject::Delivery::Delivered(_, settled) => return Some(settled),
            inject::Delivery::Unconfirmed(_) => return None,
            inject::Delivery::Refused(_) => {}
        }
        if !deadline.is_some_and(|at| Instant::now() < at) {
            return None;
        }
        sleep(common.step);
    }
}

/// 退避後の終了の手の 1 行（`line`）を `target` へ literal で送り、Enter を送る（`s2-07l.252`）。送れた周は席の
/// `tick.jsonl` に `who=seat-cycle`・`when=exit` の行を積む（立て直しの起動と同じ形）。送達の確認は呼び側
/// （tick）が前面 process の読みで行う: `/exit` を受けた席は終わって pane が shell に置き換わるので、目印の出現数
/// （[`inject::deliver`]）では成功が「現れない」に倒れる。
pub fn send_exit(socket: Option<&str>, target: &str, state_dir: &StateDir, line: &str) -> bool {
    let started_at = Instant::now();
    if !send_to(socket, target, line) {
        return false;
    }
    record_sent(state_dir, target, (line, WHEN_EXIT), started_at);
    true
}

/// 直に送った 1 行（`(what, when)`）を席の `tick.jsonl` に記録する（`who=seat-cycle`・`what` は送った 1 行そのまま・
/// `when` は立て直しの起動 `relaunch` か終了の手 `exit`）。直に送るので `seat inject` の経路の行ではない（C10）。
/// **置き場へ書けない周も結果を変えない**（記録は判定そのものではない）。
fn record_sent(state_dir: &StateDir, target: &str, (what, when): (&str, &str), started: Instant) {
    let entry = InjectionRecord {
        schema: SCHEMA,
        who: WHO.to_owned(),
        what: what.to_owned(),
        when: when.to_owned(),
        bytes: what.len() as u64,
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        seat: seat_name(target),
        ts: state::now_secs(),
    };
    let Ok(policy) = LockPolicy::embedded() else {
        return;
    };
    let path = inject::tick_path(&state_dir.path, target);
    let _ = store::append_line(&path, &entry.to_line(), policy);
}

/// 復元の 1 行を送る注入の入力（`seat inject` と同じ経路・記録は席の `tick.jsonl`）。
fn sending<'r>(common: &Boot<'r>, payload: &'r str) -> inject::Request<'r> {
    inject::Request {
        target: common.target,
        socket: common.socket,
        payload,
        state_dir: Some(common.state_dir),
    }
}
