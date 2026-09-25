//! 1 周の群の段（設計 docs/design/account-lifecycle.md §19 形 2〜4・§20・契約表の行 h / i・ADR-0049 §2・ADR-0055）。
//!
//! 起こす側の 1 周（[`super::fire`]）の先頭で走る（便の列の前・結果は列の 1 周の rc と行を変えない＝§20 形 7）。群を宣言した
//! host の周だけ、host の根の群用 dir（[`host_groups_dir`]）の lock（1 file・`create_new`）を握り、**段の全部をその内側で**行う
//! （lock の file が残る周は [`Stopped::Locked`] で止まり、計測も記録も event も撃たない）。
//!
//! lock（[`group::Lock`]）は管理 tick の移動の周（設計 seat-heartbeat.md §4）と同じ 1 本で、同じ target を 2 つの手が同じ周に
//! 撃つことを防ぐ。
//!
//! 群ごとに、測る集合 = 群の今の口座（[`group::current_of`]・記録 > 種）∪ 群の置き場の席の登録 row の口座
//! （[`crate::account::seat_accounts`] の 1 本）を取り、鮮度の外の口座だけを選定の前計測と同じ口（[`usage::run_fresh`]・
//! `--account` で 1 口座に絞る）で 1 回測る。移動を頼む記録（席の hook が置く・[`group::put_request`]）の在る群は鮮度に依らず
//! 測り（[`usage::run`]）、判定の後に頼みを履歴へ move する。最新の実測は 1 周の置き場の event log から読む（置き場を跨いで
//! 読まない・C3）。判定は席の hook と同じ読み手（[`group::pressed`]）。
//!
//! - 今の口座が逼迫でない群は §19 の通知: 逼迫の（群, 口座）ごとに群の置き場の orchestrator の席へ [`notify::send`] の口で 1 行・
//!   [`EventKind::GroupPressureNotified`] 1 件（同じ群・口座・窓で前回の通知より**後に**新しい実測が無い周は送らず記さない）。
//! - 今の口座が逼迫の群は移り先を 1 回だけ決める（[`target_of`]）。在れば移す（[`execute`]: 記録 → 承認 event → 退避の合図 →
//!   settle の窓で shell に戻った置き場から同じ target へ `launch` の 1 本・戻らない席は保留の event）。無ければ断りの event と
//!   群の置き場ごとに 1 行（[`refuse`]）。どちらの周も §19 の通知は送らない（席への行は群の置き場ごとに高々 1 行）。
//! - 記録（新しい口座）と置き場の登録 row（古い口座）が食い違う群は判定をやり直さず、shell に戻った席を起こす続きだけを行う。
//!   pane が shell でない席には退避の合図と同じ宛先・門で `/exit` の 1 行を周ごとに 1 回送り、その周は起こさない（§21 形 1）。
//!   送りは届いても未確認でも inject の記録に残し、門が `/exit` の確認 dialog の既定の行を返す周は `/exit` の代わりに Enter を
//!   1 回だけ送る（§22 形 1 / 2）。
//!
//! 群 0 の host・読めない面は 1 語も出さず群用 dir も作らない（stdout にも足さない＝列の行は 1 字も変わらない）。

use super::super::notify;
use super::Input;
use crate::account::seat_accounts;
use crate::fleet::cli::{host, now_utc};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{replay, usage, Allowance, Event, EventKind, Pressure, State, WindowKind, SCHEMA};
use crate::hook::group::{self, Caps, Current, Pressed, Record, Source};
use crate::name::NAME;
use crate::rules::manifest::{AccountGroup, Manifest};
use crate::seat::cycle::{self, Launched, REASON_NO_ACCOUNT, REASON_NOT_SHELL};
use crate::seat::role::{registration_of_key, Role};
use crate::seat::{host_groups_dir, pane_is_shell, Provenance, StateDir};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::Instant;

/// 断りの理由（移り先の候補が無い＝妥協の移動を作らない・ADR-0020 §2.4）。
const NO_CANDIDATE: &str = "no-candidate";

/// 続きの周の送りの inject の記録の `who`（群の段の名・設計 §22 形 1）。`/exit` の 1 行と確認 dialog の既定の行の値は
/// [`group::EXIT`] / [`group::exit_dialog`] の 1 か所（tick の移動の周と同じ値・設計 seat-heartbeat.md §4 形 5）。
const WHO_GROUP: &str = "pipe-group";

/// 群の段が止まった理由（typed・列の 1 周の rc と行は変えない・設計 §20 形 7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stopped {
    /// 面・閾値の行・event log を読めない。
    Unreadable,
    /// lock の file が既に在る（別の周が握っている・前の周が残した）か、群用 dir に置けない。
    Locked,
}

/// 群 1 つの 1 周ぶんの計画（今の口座・測る集合・移動を頼む記録の有無）。
struct Plan<'m> {
    /// 群の宣言。
    group: &'m AccountGroup,
    /// 周の頭の今の口座（解決の 1 関数の値）。
    current: Current,
    /// 測る集合（今の口座 ∪ 群の置き場の席の登録 row の口座）。
    measured: BTreeSet<String>,
    /// 周の頭に移動を頼む記録が在ったか。
    requested: bool,
}

/// 1 周の群の段を撃つ（効果は記録・event・席の pane の行・席の起動だけ＝列の 1 周の rc と行を変えない）。
pub(super) fn round(input: &Input<'_>) -> Result<(), Stopped> {
    let manifest = crate::rules::with_state_dir(input.manifest.clone(), Some(input.state_dir)).map_err(|_| Stopped::Unreadable)?;
    if manifest.groups().is_empty() {
        return Ok(());
    }
    let caps = Caps::of(&manifest).map_err(|_| Stopped::Unreadable)?;
    let dir = host_groups_dir(input.state_dir);
    let _lock = group::Lock::take(&dir).map_err(|_| Stopped::Locked)?;
    let before = replay(&store::read_all(input.state_dir).map_err(|_| Stopped::Unreadable)?);
    // 記録を読めない群は段から外し（typed に止まる）、その候補は他の群の移り先にしない（今の口座が分からない＝fail-closed）。
    let (mut plans, mut blocked) = (Vec::new(), BTreeSet::new());
    for found in manifest.groups() {
        match group::current_of(input.state_dir, found) {
            Ok(current) => plans.push(Plan {
                measured: measured_set(found, &current, &before),
                requested: group::request_path(&dir, found.name()).is_file(),
                group: found,
                current,
            }),
            Err(_) => blocked.extend(found.accounts().iter().cloned()),
        }
    }
    // 口座ごとに**1 周に 1 回**だけ測る（2 つの群が同じ口座を持っても 1 回・頼みの在る群の口座は鮮度に依らず・他は鮮度の外だけ）。
    let mut forced: BTreeMap<&str, bool> = BTreeMap::new();
    for plan in &plans {
        for label in &plan.measured {
            *forced.entry(label.as_str()).or_insert(false) |= plan.requested;
        }
    }
    for (label, force) in forced {
        let args = usage_args(input, label);
        let _ = if force { usage::run(&args, input.state_dir) } else { usage::run_fresh(&args, input.state_dir) };
    }
    let mut read = Read::of(input, &manifest, caps).ok_or(Stopped::Unreadable)?;
    // 「他の群の今の口座」は周の中で更新する（先に移った群の移り先を後の群が飛ばす＝2 群が同じ周に同じ label へ移らない）。
    let mut currents: Vec<String> = plans.iter().map(|plan| plan.current.label.clone()).collect();
    for (at, plan) in plans.iter().enumerate() {
        let others: BTreeSet<String> =
            currents.iter().enumerate().filter(|(other, _)| *other != at).map(|(_, label)| label.clone()).chain(blocked.iter().cloned()).collect();
        if let (Some(moved), Some(slot)) = (step(&mut read, plan, &others), currents.get_mut(at)) {
            *slot = moved;
        }
        if plan.requested {
            let _ = group::to_history(&dir, &group::request_path(&dir, plan.group.name()));
        }
    }
    Ok(())
}

/// 測る集合: 群の今の口座 ∪ 群の置き場の席の登録 row の口座（重複は畳む）。
fn measured_set(group: &AccountGroup, current: &Current, state: &State) -> BTreeSet<String> {
    std::iter::once(current.label.as_str()).chain(seat_accounts(group, state)).map(str::to_owned).collect()
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

/// 計測の後の 1 周ぶんの読み（置き場の event の並び・その replay・合わせた面・閾値）。
struct Read<'a, 'b> {
    /// 列の 1 周の材料。
    input: &'a Input<'b>,
    /// 合わせた面（tracked + host の面・口座 / plugin / 起動引数 / 鮮度の行の出所）。
    manifest: &'a Manifest,
    /// 最後に読んだ event の replay。
    state: State,
    /// 最後に読んだ event の並び（通知・断りの位置と実測の位置を比べる）。
    events: Vec<Event>,
    /// 窓ごとの閾値。
    caps: Caps,
}

impl<'a, 'b> Read<'a, 'b> {
    /// 置き場の event log を読む（読めない周は `None`）。
    fn of(input: &'a Input<'b>, manifest: &'a Manifest, caps: Caps) -> Option<Self> {
        let events = store::read_all(input.state_dir).ok()?;
        Some(Self { input, manifest, state: replay(&events), events, caps })
    }

    /// 計測を撃った後に読み直す（読めない周は `false`）。
    fn refresh(&mut self) -> bool {
        let Ok(events) = store::read_all(self.input.state_dir) else {
            return false;
        };
        self.state = replay(&events);
        self.events = events;
        true
    }
}

/// 群 1 つの 1 周（続き・移動・断り・通知のどれか 1 つ）。移した周は移り先を返す。
fn step(read: &mut Read<'_, '_>, plan: &Plan<'_>, others: &BTreeSet<String>) -> Option<String> {
    let current = plan.current.label.as_str();
    let behind = behind(&read.state, plan.group, current);
    if plan.current.source == Source::Record && !behind.is_empty() {
        relaunch(read, plan.group, current, behind, Wait::Once);
        return None;
    }
    let pressed = usage::latest_of(&read.state, current).and_then(|rows| group::pressed(&rows, read.caps));
    if pressed.is_none() {
        for label in &plan.measured {
            notice(read, plan.group, label);
        }
        return None;
    }
    match target_of(read, plan, others) {
        Some(target) => execute(read, plan, &target).then_some(target),
        None => {
            refuse(read, plan.group, current);
            None
        }
    }
}

/// 群の置き場のうち、orchestrator の登録 row の口座が `account` と違う席（置き場・tmux target・宣言順）。
fn behind(state: &State, group: &AccountGroup, account: &str) -> Vec<(String, String)> {
    group
        .anchors()
        .iter()
        .filter_map(|anchor| {
            registration_of_key(state, Role::Orchestrator, anchor)
                .filter(|row| row.account != account)
                .map(|row| (anchor.clone(), row.target.clone()))
        })
        .collect()
}

/// 移り先（設計 §20 形 5）: 宣言の候補の順で、今の口座でなく・他の群の今の口座でなく・退役中でなく・1 周の置き場の live 便が
/// 使っていない口座を、鮮度の外なら 1 回測り、3 窓とも閾値未満の鮮度の内側の実測を持つ**最初の** label。無ければ `None`。
fn target_of(read: &mut Read<'_, '_>, plan: &Plan<'_>, others: &BTreeSet<String>) -> Option<String> {
    for label in plan.group.accounts() {
        let live = read.state.inflight_by_account().get(label).is_some_and(|runs| *runs > 0);
        if *label == plan.current.label || others.contains(label) || read.state.retired.contains_key(label) || live {
            continue;
        }
        let _ = usage::run_fresh(&usage_args(read.input, label), read.input.state_dir);
        if !read.refresh() {
            return None;
        }
        let fresh = usage::fresh_rows(read.manifest, &read.state, label).ok().flatten();
        if fresh.is_some_and(|rows| group::pressed(&rows, read.caps).is_none()) {
            return Some(label.clone());
        }
    }
    None
}

/// 移動の執行（同じ lock の内側・この順・設計 §20 形 6）: 記録を書く → 承認 event → 群の置き場の古い口座の席へ退避の合図 →
/// settle の窓で shell に戻った置き場から同じ target へ新しい口座の席を起こす。前提（宣言の逐語・確認の刻み・役割の既定の面）が
/// 揃わない周と記録を書けない周は 1 つも書かずに `false`。
fn execute(read: &mut Read<'_, '_>, plan: &Plan<'_>, target: &str) -> bool {
    let input = read.input;
    let (Some(words), Some(_), Ok(_)) =
        (declaration(input.state_dir, plan.group), cycle::pace_of(read.manifest), crate::seat::embedded_manifest())
    else {
        return false;
    };
    let record = Record { account: target.to_owned(), ts: now_utc(), previous: plan.current.label.clone() };
    if group::write_current(&host_groups_dir(input.state_dir), plan.group.name(), &record).is_err() {
        return false;
    }
    append(input, EventKind::GroupMoved, target, words);
    let seats = behind(&read.state, plan.group, target);
    let payload = format!("{NAME} group: evacuate group={} to={target} — 作業記憶を台帳と git に残して /exit", plan.group.name());
    for (anchor, _) in &seats {
        let _ = notify::send(&read.state, &place(input), Path::new(anchor), input.manifest, &payload);
    }
    relaunch(read, plan.group, target, seats, Wait::Settle);
    true
}

/// 起こし直しの待ち方（閉じた 2 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    /// 移動の周: settle の窓の内で待ち、窓の内に戻らない席ごとに保留の event を 1 件記す。
    Settle,
    /// 続きの周: 1 回だけ見て、戻っていない席へ [`group::EXIT`] の 1 行を送り次の周へ残す（保留の event を重ねない）。
    Once,
}

/// `seats` の置き場の席を、pane が shell に戻った順に同じ target へ `account` の口座で起こす（`launch` の 1 本・登録 row は
/// 起動が書き直す・会話は運ばない・呼び手の窓の置き換えは許さない）。起こせなかった席は理由つきの保留の event を 1 件記す。
/// 続きの周（[`Wait::Once`]）に shell でない席は起こさず、退避の合図と同じ宛先・門（[`notify::send_or_confirm`]）で
/// [`group::EXIT`] を 1 回送る（移動の周は送らない＝席が作業記憶を残す番を 1 周ぶん持つ・設計 §21 形 1）。門が
/// [`group::exit_dialog`] の既定の行を返す周は `/exit` の代わりに Enter を 1 回だけ送り、どちらの送りも群の段の名
/// （[`WHO_GROUP`]）で inject の記録に残す（設計 §22 形 1 / 2）。
fn relaunch(read: &Read<'_, '_>, group: &AccountGroup, account: &str, mut seats: Vec<(String, String)>, wait: Wait) {
    let input = read.input;
    let (Some((settle, step)), Ok(rules)) = (cycle::pace_of(read.manifest), crate::seat::embedded_manifest()) else {
        return;
    };
    let place = place(input);
    let deadline = Instant::now().checked_add(settle);
    let mut failed: Vec<(String, String, &'static str)> = Vec::new();
    loop {
        seats.retain(|(anchor, target)| {
            if !pane_is_shell(None, target) {
                return true;
            }
            let launched = cycle::launch(&cycle::Launch {
                target,
                socket: None,
                state_dir: &place,
                restore: None,
                settle,
                step,
                role: Role::Orchestrator,
                anchor: Path::new(anchor),
                account: Some(account),
                model: None,
                manifest: read.manifest,
                rules: &rules,
                threshold_pct: 0,
                carry: &[],
                replace_own: false,
            });
            if let Some(reason) = launch_failure(&launched) {
                failed.push((anchor.clone(), target.clone(), reason));
            }
            false
        });
        if seats.is_empty() || wait == Wait::Once || !deadline.is_some_and(|at| Instant::now() < at) {
            break;
        }
        sleep(step);
    }
    match wait {
        Wait::Settle => failed.extend(seats.into_iter().map(|(anchor, target)| (anchor, target, REASON_NOT_SHELL))),
        Wait::Once => {
            for (anchor, _) in &seats {
                let dialog = group::exit_dialog(WHO_GROUP);
                let _ = notify::send_or_confirm(&read.state, &place, Path::new(anchor), input.manifest, (group::EXIT, &dialog));
            }
        }
    }
    for (anchor, target, reason) in failed {
        let detail = format!("group={} anchor={anchor} target={target} reason={reason}", group.name());
        append(input, EventKind::GroupMovePending, account, detail);
    }
}

/// 起動の結果の断り・失敗の理由（起こせた周は `None`）。
fn launch_failure(launched: &Launched) -> Option<&'static str> {
    match launched {
        Launched::Done(..) => None,
        Launched::None(_) => Some(REASON_NO_ACCOUNT),
        Launched::Refused(reason) | Launched::Failed(reason) => Some(*reason),
    }
}

/// 移り先の無い群の断り（設計 §20 形 5）: 断りの event を 1 件記し、群の置き場の orchestrator の席へ 1 行ずつ送る。前の断りより
/// **後に**今の口座の新しい実測が無い周は繰り返さない（§19 形 4 と同じ log の位置の規則＝同じ実測に 2 度断らない）。
fn refuse(read: &Read<'_, '_>, group: &AccountGroup, current: &str) {
    let detail = format!("group={} reason={NO_CANDIDATE}", group.name());
    let refused = |event: &Event| event.kind == EventKind::GroupMoveRefused && event.detail.as_deref() == Some(detail.as_str());
    if !measured_since(&read.events, current, None, refused) {
        return;
    }
    append(read.input, EventKind::GroupMoveRefused, current, detail);
    let payload = format!("{NAME} group: move-refused group={} reason={NO_CANDIDATE}", group.name());
    for anchor in group.anchors() {
        if registration_of_key(&read.state, Role::Orchestrator, anchor).is_some() {
            let _ = notify::send(&read.state, &place(read.input), Path::new(anchor), read.input.manifest, &payload);
        }
    }
}

/// 群の宣言の行の逐語（承認の逐語・A1）: host の面の path と行番号 + `[[account-group]]` の行から次の表の手前までの行（末尾の
/// 空行は落とす）。読めない・行番号の行が群の表の頭でない周は `None`（逐語の無い承認で移さない）。
fn declaration(state_dir: &Path, group: &AccountGroup) -> Option<String> {
    let path = crate::rules::host_manifest_path(state_dir);
    let text = fs::read_to_string(&path).ok()?;
    let start = usize::try_from(group.line()).ok()?.checked_sub(1)?;
    let mut block: Vec<&str> = text
        .lines()
        .skip(start)
        .enumerate()
        .take_while(|(at, line)| *at == 0 || !line.trim_start().starts_with('['))
        .map(|(_, line)| line)
        .collect();
    while block.last().is_some_and(|line| line.trim().is_empty()) {
        block.pop();
    }
    (block.first().map(|head| head.trim()) == Some("[[account-group]]"))
        .then(|| format!("{}:{}\n{}", path.display(), group.line(), block.join("\n")))
}

/// 注入と起動の置き場（絶対 path・出所は flag＝列に渡された `--state-dir`）。
fn place(input: &Input<'_>) -> StateDir {
    StateDir {
        path: std::path::absolute(input.state_dir).unwrap_or_else(|_| input.state_dir.to_path_buf()),
        source: Provenance::Flag,
    }
}

/// （群, 口座）1 つを判じ、逼迫で新しい実測が在れば送って記す（§19 形 3 / 4）。
fn notice(read: &Read<'_, '_>, group: &AccountGroup, label: &str) {
    let Some(found) = usage::latest_of(&read.state, label).and_then(|rows| group::pressed(&rows, read.caps)) else {
        return;
    };
    let noticed = |event: &Event| {
        event.account.as_deref() == Some(label)
            && event.pressure().is_some_and(|body| body.group == group.name() && body.window == found.window)
    };
    if !measured_since(&read.events, label, Some(found.window), noticed) {
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
    let mut sent: u64 = 0;
    for anchor in group.anchors() {
        if registration_of_key(&read.state, Role::Orchestrator, anchor).is_none() {
            continue;
        }
        let _ = notify::send(&read.state, &place(input), Path::new(anchor), input.manifest, &payload);
        sent = sent.saturating_add(1);
    }
    record(input, label, &pressure_of(group, found, sent));
}

/// 通知の本体。
fn pressure_of(group: &AccountGroup, found: Pressed, sent: u64) -> Pressure {
    Pressure { group: group.name().to_owned(), window: found.window, used: found.used, cap: found.cap, sent }
}

/// `mark` に当たる最後の行より**後に**、口座 `label` の実測の行（`window` が在ればその窓だけ）が在るか（印が無ければ `true`・
/// log の位置で判じ、値の比較で判じない・§19 形 4）。
fn measured_since(events: &[Event], label: &str, window: Option<WindowKind>, mark: impl Fn(&Event) -> bool) -> bool {
    let marked = events.iter().rposition(mark);
    let measured = events.iter().rposition(|event| {
        event.kind == EventKind::AllowanceMeasured
            && matches!(&event.allowance, Some(Allowance::Measured(row))
                if row.account == label && window.is_none_or(|found| row.window == found))
    });
    match marked {
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
    append(input, EventKind::GroupPressureNotified, label, detail);
}

/// 群の段の event を 1 件記す（run / bead を持たない・actor は machine・`account` と `detail` の 2 つが本体）。書けない周も段は
/// 止めない（効果は記帳の有無だけ）。
fn append(input: &Input<'_>, kind: EventKind, account: &str, detail: String) {
    let Ok(policy) = LockPolicy::from_rules(input.manifest) else {
        return;
    };
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
        account: Some(account.to_owned()),
        cost: None,
        rule: None,
    };
    let _ = store::append(input.state_dir, &event, policy);
}
