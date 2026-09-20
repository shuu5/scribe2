//! `seat launch` が通る起動の 1 本（[`boot`]）と、席の初回の口座の選定（[`choose`]）・起動行の穴埋め
//! （[`launch_line`]）。**立て直しと退避後の終了の手は持たない**（ADR-0045 §2 (2) で管理 tick ごと消えた・
//! `s2-07l.479.1`）。判定順・注入の字面・記録の行は起動の面のまま。

use super::launch::{before_deadline, fill_launch, single_model, with_agent_view_off, with_anchor_cd, Holes};
use super::{
    send_to, started, ACCOUNTS_DIR, REASON_INPUT_BUSY, REASON_INPUT_UNKNOWN, REASON_LAUNCH, REASON_PANE_MISSING,
    REASON_RESTORE, WHO,
};
use crate::fleet::select::{self, Purpose, Selection};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::State;
use crate::hook::{seat_name, InjectionRecord, SCHEMA};
use crate::seat::role::Role;
use crate::seat::{inject, state, InputGate, StateDir, REASON_TMUX_FAILED};
use std::collections::BTreeSet;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

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

/// 雛形 `template` の `--model` を高々 1 つと確かめ（[`single_model`]）、穴を口座 `label` の credential dir（`<state_dir>/accounts/<label>`）で埋め、agent view off と
/// 登録 row の `anchor` への `cd` を前置した起動の 1 行（`cd '<anchor>' && ENV=… claude …`・立て直しと `seat launch` の同じ 1 つ・記録にも同じ行が載る・`s2-07l.324`）。断りは字面。
pub(super) fn launch_line(state_dir: &StateDir, template: &str, label: &str, anchor: &str) -> Result<String, &'static str> {
    single_model(template)?;
    let account_dir = state_dir.path.join(ACCOUNTS_DIR).join(label);
    fill_launch(template, &account_dir.display().to_string()).map(|found| with_anchor_cd(&with_agent_view_off(&found), anchor)).map_err(Holes::as_str)
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

/// 起動の 1 本の結果（**「届いた」と「送ったが確かめられない」を分ける**・[`Relaunched`] と同じ極性）。
///
/// 「**1 key も送っていない**」側は持たない: 門を [`boot`] から [`input_gate`] へ切り出した後は、送る前の断りは
/// 全部 [`super::launch::prepare`] が登録 row の前で返す（設計 seat-roles.md §26 の約束 5）。
pub(super) enum Booted {
    /// 起動が届いて立ち上がりを確かめた（復元を送った周はその消費・送らない周は `None`）。
    Done(Option<inject::Settled>),
    /// 送ったが確かめられない。
    Failed(&'static str),
}

/// shell の入力欄の門（**[`boot`] の本体の先頭から切り出した 1 本**・設計 seat-roles.md §26 の約束 5・
/// 中身と 3 つの理由の字面と判定の順序は 1 語も変えない純粋な切り出し）: pane を capture できなければ
/// [`REASON_PANE_MISSING`]・[`crate::seat::shell_input_empty`] が断れば [`REASON_INPUT_BUSY`] /
/// [`REASON_INPUT_UNKNOWN`]。**1 key も送らない側**（C11.2・緩めない）。
///
/// 起動は**前面が shell の pane** へ撃つので、門は席の `❯` の行（[`inject::guard_input`]）でなく shell の prompt 末尾
/// （account-autonomy.md §5「shell への注入の門」・`s2-07l.218`）で見る。呼び手は
/// [`super::launch::prepare`] の 1 か所だけで、登録 row を書く**前**に撃つ（[`boot`] はもう持たない＝
/// 同じ判定を 2 か所で撃たない・C2）。
pub(super) fn input_gate(socket: Option<&str>, target: &str) -> Result<(), &'static str> {
    let Some(pane) = crate::seat::tmux_stdout(socket, &["capture-pane", "-p", "-J", "-t", target]) else {
        return Err(REASON_PANE_MISSING);
    };
    match crate::seat::shell_input_empty(&pane) {
        Ok(()) => Ok(()),
        Err(InputGate::Busy) => Err(REASON_INPUT_BUSY),
        Err(InputGate::UnknownInput) => Err(REASON_INPUT_UNKNOWN),
    }
}

/// 起動の注入の **1 本**（順序固定・立て直しと `seat launch` の共通の経路・account-lifecycle.md §4）:
/// 起動行（`(line, when)`）を送って記録 → 立ち上がりの確認 → `between`（立て直しは登録 row の更新・起動は何もしない）→
/// 復元。
///
/// 入力欄の門は本体の先頭に**持たない**（[`input_gate`] へ切り出し、呼び手が登録 row の前に撃つ）。立ち上がった
/// 後の復元は席の pane なので従来どおり注入の門を通る。
pub(super) fn boot(common: &Boot, dir: &Path, (line, when): (&str, &str), between: impl FnOnce() -> Result<(), &'static str>) -> Booted {
    let baseline = state::baseline(dir);
    let since = state::now_secs();
    let started_at = Instant::now();
    if !send_to(common.socket, common.target, line) {
        return Booted::Failed(REASON_TMUX_FAILED);
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
        if restore_window_closed(Instant::now(), deadline) {
            return None;
        }
        sleep(common.step);
    }
}

/// 復元の窓が閉じたか（**pure**・[`restore_when_ready`] の送り直しを止める側・`s2-07l.344`）: 測った `now` が `deadline`
/// の手前（[`before_deadline`]）で**ない**周に true＝窓に達した・過ぎた・窓が無い周は送り直さず `None` へ倒れる。手前の
/// 周だけ false（負例＝送り直す）。呼び手は `Instant::now()` を渡す。
fn restore_window_closed(now: Instant, deadline: Option<Instant>) -> bool {
    !before_deadline(now, deadline)
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

#[cfg(test)]
mod tests {
    use super::restore_window_closed;
    use std::time::{Duration, Instant};

    /// 復元の送り直しを止めるのは窓の**外**だけ（`s2-07l.344`・.319 の検出線の生存 `!` の削除を潰す）: 窓の手前は
    /// 閉じていない（負例＝送り直す）・窓に達した周・過ぎた周・窓が無い周は閉じている（`None` へ倒れる）。
    /// `now` は測定値を 1 回だけ取り、表は Instant の算術だけで作る（sleep しない）。
    #[test]
    fn cycle_relaunch_fires_only_after_the_deadline() {
        let now = Instant::now();
        let later = now + Duration::from_millis(1);
        assert!(!restore_window_closed(now, Some(later)), "窓の手前は送り直す（負例）");
        assert!(restore_window_closed(now, Some(now)), "窓に達した周は止める");
        assert!(restore_window_closed(later, Some(now)), "窓を過ぎた周は止める");
        assert!(restore_window_closed(now, None), "窓が無い周は止める");
    }
}
