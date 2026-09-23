//! 1 周の群の段（設計 docs/design/account-lifecycle.md §19 形 2〜4・§7・契約表の行 h・ADR-0055）。
//!
//! 起こす側の 1 周（[`super::fire`]）の先頭で走る。宣言された群ごとに、測る集合 = 群の今の口座（本段では記録が無いので
//! 種 = 候補の先頭）∪ 群の置き場の席の登録 row の口座（[`crate::account::seat_accounts`] の 1 本）を取り、鮮度の外の口座
//! だけを選定の前計測と同じ口（[`usage::run_fresh`]・`--account` で 1 口座に絞る）で 1 回測り、最新の実測を 1 周の置き場の
//! event log から読む（置き場を跨いで読まない・C3）。判定は席の hook と同じ読み手（[`group::pressed`]）。
//!
//! 逼迫の（群, 口座）ごとに 1 行を群の置き場すべての orchestrator の登録 row の席へ [`notify::send`] の口で注入し、
//! [`EventKind::GroupPressureNotified`] を 1 件記す。同じ群・口座・窓で前回の通知より**後に**新しい実測が無い周は送らず
//! 記さない（log の位置で判じ、値の比較で判じない・形 4）。群 0 の host・読めない面は 1 語も出さず、stdout にも
//! 足さない（列の行は 1 字も変わらない）。移動は作らない（第 3 段・§20）。

use super::super::notify;
use super::Input;
use crate::account::seat_accounts;
use crate::fleet::cli::{host, now_utc};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{replay, usage, Allowance, Event, EventKind, Pressure, State, SCHEMA};
use crate::hook::group::{self, Caps, Pressed};
use crate::name::NAME;
use crate::rules::manifest::AccountGroup;
use crate::seat::role::{registration_of_key, Role};
use crate::seat::{Provenance, StateDir};
use std::collections::BTreeSet;
use std::path::Path;

/// 1 周の群の段を撃つ（効果は席の pane の行と event だけ・結果を返さない＝列の 1 周の rc と行を変えない）。
pub(super) fn round(input: &Input<'_>) {
    let Ok(manifest) = crate::rules::with_state_dir(input.manifest.clone(), Some(input.state_dir)) else {
        return;
    };
    if manifest.groups().is_empty() {
        return;
    }
    let (Ok(caps), Ok(events)) = (Caps::of(&manifest), store::read_all(input.state_dir)) else {
        return;
    };
    let before = replay(&events);
    let plan: Vec<(&AccountGroup, BTreeSet<String>)> =
        manifest.groups().iter().map(|found| (found, measured_set(found, &before))).collect();
    // 口座ごとに**1 周に 1 回**だけ前計測の口を撃つ（2 つの群が同じ口座を持っても 1 回・鮮度の内側は子を起こさない）。
    let all: BTreeSet<&String> = plan.iter().flat_map(|(_, labels)| labels.iter()).collect();
    for label in all {
        let _ = usage::run_fresh(&usage_args(input, label), input.state_dir);
    }
    let Ok(events) = store::read_all(input.state_dir) else {
        return;
    };
    let read = Read { input, state: replay(&events), events: &events, caps };
    for (found, labels) in &plan {
        for label in labels {
            notice(&read, found, label);
        }
    }
}

/// 測る集合: 群の種（候補の先頭）∪ 群の置き場の席の登録 row の口座（重複は畳む）。
fn measured_set(group: &AccountGroup, state: &State) -> BTreeSet<String> {
    let seed = group.accounts().first().map(String::as_str);
    seed.into_iter().chain(seat_accounts(group, state)).map(str::to_owned).collect()
}

/// 前計測の口へ渡す引数（1 口座に絞る `--account` + 列に渡された `--rules` / `--curl` の写し）。
fn usage_args(input: &Input<'_>, label: &str) -> Vec<String> {
    let mut args = vec!["--account".to_owned(), label.to_owned()];
    for (name, value) in [("--rules", input.rules), ("--curl", input.curl)] {
        if let Some(found) = value {
            args.extend([name.to_owned(), found.to_owned()]);
        }
    }
    args
}

/// 計測の後の 1 周ぶんの読み（置き場の event の並び・その replay・閾値）。
struct Read<'a, 'b> {
    /// 列の 1 周の材料。
    input: &'a Input<'b>,
    /// 計測の後の replay。
    state: State,
    /// 計測の後の event の並び（通知の位置と実測の位置を比べる）。
    events: &'a [Event],
    /// 窓ごとの閾値。
    caps: Caps,
}

/// （群, 口座）1 つを判じ、逼迫で新しい実測が在れば送って記す。
fn notice(read: &Read<'_, '_>, group: &AccountGroup, label: &str) {
    let Some(found) = usage::latest_of(&read.state, label).and_then(|rows| group::pressed(&rows, read.caps)) else {
        return;
    };
    if !measured_since_notice(read.events, group.name(), label, found) {
        return;
    }
    let payload = format!(
        "{NAME} group: pressure group={} account={label} window={} used={} cap={}",
        group.name(),
        found.window.short(),
        found.used,
        found.cap
    );
    let input = read.input;
    let place = StateDir {
        path: std::path::absolute(input.state_dir).unwrap_or_else(|_| input.state_dir.to_path_buf()),
        source: Provenance::Flag,
    };
    let mut sent: u64 = 0;
    for anchor in group.anchors() {
        if registration_of_key(&read.state, Role::Orchestrator, anchor).is_none() {
            continue;
        }
        let _ = notify::send(&read.state, &place, Path::new(anchor), input.manifest, &payload);
        sent = sent.saturating_add(1);
    }
    let pressure = Pressure { group: group.name().to_owned(), window: found.window, used: found.used, cap: found.cap, sent };
    record(input, label, &pressure);
}

/// 同じ群・口座・窓の最後の通知より**後に**、その口座のその窓の実測の行が在るか（通知が無ければ `true`・形 4）。
fn measured_since_notice(events: &[Event], group: &str, label: &str, found: Pressed) -> bool {
    let noticed = events.iter().rposition(|event| {
        event.account.as_deref() == Some(label)
            && event.pressure().is_some_and(|body| body.group == group && body.window == found.window)
    });
    let measured = events.iter().rposition(|event| {
        event.kind == EventKind::AllowanceMeasured
            && matches!(&event.allowance, Some(Allowance::Measured(row)) if row.account == label && row.window == found.window)
    });
    match noticed {
        None => true,
        Some(at) => measured.is_some_and(|last| last > at),
    }
}

/// 通知の event を 1 件記す（読み返せない形は書かない＝append-only の log に読めない行を残さない）。
fn record(input: &Input<'_>, label: &str, pressure: &Pressure) {
    let detail = pressure.render();
    if Pressure::parse(&detail).as_ref() != Some(pressure) {
        return;
    }
    let Ok(policy) = LockPolicy::from_rules(input.manifest) else {
        return;
    };
    let kind = EventKind::GroupPressureNotified;
    let event = Event {
        schema: SCHEMA,
        ts: now_utc(),
        kind,
        run: String::new(),
        bead: String::new(),
        host: host(),
        actor: kind.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: Some(detail),
        allowance: None,
        registration: None,
        mark: None,
        account: Some(label.to_owned()),
        cost: None,
        rule: None,
    };
    let _ = store::append(input.state_dir, &event, policy);
}
