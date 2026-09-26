//! 1 周の群の段（設計 docs/design/account-lifecycle.md §19 形 2〜4・§20・契約表の行 h / i・ADR-0049 §2・ADR-0055）。
//!
//! 起こす側の 1 周（[`super::fire`]）の先頭で走る（便の列の前・結果は列の 1 周の rc と行を変えない＝§20 形 7）。群を宣言した
//! host の周だけ、host の根の群用 dir（[`host_groups_dir`]）の lock（1 file・`create_new`）を握り、**段の全部をその内側で**行う
//! （lock の file が残る周は [`Stopped::Locked`] で止まり、計測も記録も event も撃たない）。
//!
//! lock（[`group::Lock`]）は管理 tick の移動の周（設計 seat-heartbeat.md §4）と同じ 1 本で、同じ target を 2 つの手が同じ周に
//! 撃つことを防ぐ。
//!
//! 群ごとの判定（測る集合 → 鮮度の外の計測 → 判定 → 記録と承認 event / 断りの event と頼みの履歴化）は tick と同じ 1 本
//! （[`group::judge`]・設計 seat-heartbeat.md §9 形 1）。周の頭は計測を撃たず、鮮度に依らず測る口座（移動を頼む記録の在る群の
//! 測る集合の和）とこの周で既に測った口座（空で始め 1 本が足す）の 2 集合を組んで 1 本へ渡す（口座ごとに 1 周 1 回）。
//!
//! - 今の口座が逼迫でない群は §19 の通知: 1 本が返す逼迫の（群, 口座）ごとに群の置き場の orchestrator の席へ [`notify::send`]
//!   の口で 1 行・[`EventKind::GroupPressureNotified`] 1 件（同じ群・口座・窓で前回の通知より**後に**新しい実測が無い周は送らず
//!   記さない）。
//! - 移った群は退避の合図 → settle の窓で shell に戻った置き場から同じ target へ `launch` の 1 本〔打刻の最終行の sid を
//!   `--resume` で運ぶ・seat-heartbeat.md §8〕・戻らない席は保留の event（[`evacuate`]）。1 本が断りの event を記した群は群の
//!   置き場ごとに断りの 1 行。どちらの周も §19 の通知は送らない（席への行は群の置き場ごとに高々 1 行）。
//! - 記録（新しい口座）と置き場の登録 row（古い口座）が食い違う群は判定をやり直さず、shell に戻った席を起こす続きだけを行う。
//!   pane が shell でない席には退避の合図と同じ宛先・門で `/exit` の 1 行を周ごとに 1 回送り、その周は起こさない（§21 形 1）。
//!   送りは届いても未確認でも inject の記録に残し、門が `/exit` の確認 dialog の既定の行を返す周は `/exit` の代わりに Enter を
//!   1 回だけ送る（§22 形 1 / 2）。
//!
//! 群 0 の host・読めない面は 1 語も出さず群用 dir も作らない（stdout にも足さない＝列の行は 1 字も変わらない）。

use super::super::notify;
use super::Input;
use crate::fleet::store;
use crate::fleet::{replay, usage, Event, EventKind, Pressure, State};
use crate::hook::group::{self, Caps, Current, Judgement, Pressed, Refusal, Source};
use crate::name::NAME;
use crate::rules::manifest::{AccountGroup, Manifest};
use crate::seat::cycle::{self, Launched, REASON_NO_ACCOUNT, REASON_NOT_SHELL};
use crate::seat::role::{registration_of_key, Role};
use crate::seat::state::resume_carry;
use crate::seat::{host_groups_dir, pane_is_shell, seat_dir, Provenance, StateDir};
use std::collections::BTreeSet;
use std::path::Path;
use std::thread::sleep;
use std::time::Instant;

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

/// 1 周の群の段を撃つ（効果は記録・event・席の pane の行・席の起動だけ＝列の 1 周の rc と行を変えない）。
pub(super) fn round(input: &Input<'_>) -> Result<(), Stopped> {
    let manifest = crate::rules::with_state_dir(input.manifest.clone(), Some(input.state_dir)).map_err(|_| Stopped::Unreadable)?;
    if manifest.groups().is_empty() {
        return Ok(());
    }
    let caps = Caps::of(&manifest).map_err(|_| Stopped::Unreadable)?;
    let grace_s = crate::rules::int_row(&manifest, crate::seat::tick::ROW_GRACE).map_err(|_| Stopped::Unreadable)?;
    let dir = host_groups_dir(input.state_dir);
    let _lock = group::Lock::take(&dir).map_err(|_| Stopped::Locked)?;
    let before = replay(&store::read_all(input.state_dir).map_err(|_| Stopped::Unreadable)?);
    // 記録を読めない群は段から外し（typed に止まる）、その候補は他の群の移り先にしない（今の口座が分からない＝fail-closed）。
    let (mut plans, mut blocked) = (Vec::new(), BTreeSet::new());
    for found in manifest.groups() {
        match group::current_of(input.state_dir, found) {
            Ok(current) => plans.push((found, current)),
            Err(_) => blocked.extend(found.accounts().iter().cloned()),
        }
    }
    // 周の頭は測らない: 鮮度に依らず測る口座（頼みの在る群の測る集合の和）と既に測った口座（空）を組み、1 本が口座ごとに 1 回測る。
    let forced: BTreeSet<String> = plans
        .iter()
        .filter(|(found, _)| group::request_path(&dir, found.name()).is_file())
        .flat_map(|(found, current)| group::measured_set(found, current, &before))
        .collect();
    let mut measured = BTreeSet::new();
    let mut read = Read::of(input, &manifest, (caps, grace_s)).ok_or(Stopped::Unreadable)?;
    // 先の群の予約は周の頭の今の口座から導き（移った群は自分の予約を使った＝後の群はその次へ・§29 形 2）、判じる群の移り先から
    // 外す「今の口座」は周の中で更新する（先に移った群の移り先を後の群が飛ばす＝2 群が同じ周に同じ label へ移らない）。
    let mut currents: Vec<String> = plans.iter().map(|(_, current)| current.label.clone()).collect();
    let head: BTreeSet<String> = currents.iter().chain(&blocked).cloned().collect();
    for (at, (found, current)) in plans.iter().enumerate() {
        let taken: BTreeSet<String> = currents.iter().chain(&blocked).cloned().collect();
        if let (Some(moved), Some(slot)) = (step(&mut read, (found, current), [&head, &taken, &forced], &mut measured), currents.get_mut(at)) {
            *slot = moved;
        }
    }
    Ok(())
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
    /// 退避の猶予（秒・`seat.move_grace_s`・設計 seat-heartbeat.md §13 形 7）。
    grace_s: u64,
}

impl<'a, 'b> Read<'a, 'b> {
    /// 置き場の event log を読む（読めない周は `None`）。
    fn of(input: &'a Input<'b>, manifest: &'a Manifest, (caps, grace_s): (Caps, u64)) -> Option<Self> {
        let events = store::read_all(input.state_dir).ok()?;
        Some(Self { input, manifest, state: replay(&events), events, caps, grace_s })
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

/// 群 1 つの 1 周（続き・移動・断り・通知のどれか 1 つ）。続きの周でなければ判定の 1 本を撃ち、その出力から送る。移した周は
/// 移り先を返す。
fn step(
    read: &mut Read<'_, '_>,
    (found, current): (&AccountGroup, &Current),
    [head, taken, forced]: [&BTreeSet<String>; 3],
    measured: &mut BTreeSet<String>,
) -> Option<String> {
    let behind = behind(&read.state, found, &current.label);
    if current.source == Source::Record && !behind.is_empty() {
        relaunch(read, found, group::signal_key(current), behind, Wait::Once);
        return None;
    }
    let input = read.input;
    let measure = |label: &str, force: bool| {
        let args = usage_args(input, label);
        let _ = if force { usage::run(&args, input.state_dir) } else { usage::run_fresh(&args, input.state_dir) };
    };
    let judge =
        group::Judge { state_dir: input.state_dir, manifest: read.manifest, group: found, head, taken, forced, caps: read.caps, measure: &measure };
    let judged = group::judge(&judge, measured);
    if !read.refresh() {
        return None;
    }
    match judged {
        Judgement::Stay(pressed) => {
            for (label, found_pressed) in pressed {
                notice(read, found, &label, found_pressed);
            }
            None
        }
        Judgement::Moved(target) => {
            evacuate(read, found, &target);
            Some(target)
        }
        Judgement::NoCandidate(Refusal::Recorded) => {
            let payload = group::refused_line(found);
            for anchor in found.anchors() {
                if registration_of_key(&read.state, Role::Orchestrator, anchor).is_some() {
                    let _ = notify::send(&read.state, &place(input), Path::new(anchor), input.manifest, &payload);
                }
            }
            None
        }
        Judgement::NoCandidate(Refusal::Repeated) | Judgement::Unreadable => None,
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

/// 移動の執行の残り（1 本が記録と承認 event を書いた後・同じ lock の内側・設計 §20 形 6）: 群の置き場の古い口座の席へ退避の
/// 合図（[`group::evacuate_line`]・残りの秒 = 猶予の値・送達を確認した席ごとに `at` = 今の [`group::write_signal`]＝
/// seat-heartbeat.md §14 形 4）→ settle の窓で shell に戻った置き場から同じ target へ新しい口座の席を起こす。
fn evacuate(read: &Read<'_, '_>, found: &AccountGroup, target: &str) {
    let (input, place) = (read.input, place(read.input));
    let seats = behind(&read.state, found, target);
    let payload = group::evacuate_line(found.name(), target, read.grace_s);
    let current = group::current_of(input.state_dir, found).ok();
    let ts = current.as_ref().map(|found| group::signal_key(found).1);
    for (anchor, seat) in &seats {
        let line = notify::send(&read.state, &place, Path::new(anchor), input.manifest, &payload);
        if let (true, Some(ts)) = (line.starts_with("notify=delivered "), ts) {
            let _ = group::write_signal(&seat_dir(&place.path, seat), (target, ts), crate::seat::state::now_secs());
        }
    }
    relaunch(read, found, (target, ts.unwrap_or_default()), seats, Wait::Settle);
}

/// 起こし直しの待ち方（閉じた 2 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    /// 移動の周: settle の窓の内で待ち、窓の内に戻らない席ごとに保留の event を 1 件記す。
    Settle,
    /// 続きの周: 1 回だけ見て、戻っていない席のうち同じ移動の合図の記録が猶予を越えた席へだけ [`group::EXIT`] の 1 行を送り次の
    /// 周へ残す（保留の event を重ねない・席ごとに読む・seat-heartbeat.md §14 形 4）。
    Once,
}

/// `seats` の置き場の席を、pane が shell に戻った順に同じ target へ `account` の口座で起こす（`launch` の 1 本・登録 row は
/// 起動が書き直す・会話は席の置き場の打刻の最終行の sid を [`resume_carry`] の 1 本で `carry` に運び初手の 1 語も同じ 1 本が積む
/// 〔row の launch と event には載せない・設計 seat-heartbeat.md §8 / §10 形 1〕・呼び手の窓の置き換えは許さない）。起こせなかった席は理由つきの保留の event を 1 件記す。
/// 続きの周（[`Wait::Once`]）に shell でない席は起こさず、席の置き場の合図の記録が同じ移動（鍵 `key`・[`group::signalled`]）で
/// 残り（[`group::grace_left`]）が `None` の席にだけ、退避の合図と同じ宛先・門（[`notify::send_or_confirm`]）で [`group::EXIT`] を
/// 1 回送る（移動の周は送らない＝席が作業記憶を残す番を 1 周ぶん持つ・設計 §21 形 1。残りが在る席と記録の無い・別の移動の席には
/// 送らない＝合図は tick が送る・seat-heartbeat.md §14 形 4）。門が [`group::exit_dialog`] の既定の行を返す周は `/exit` の代わりに
/// Enter を 1 回だけ送り、どちらの送りも群の段の名（[`WHO_GROUP`]）で inject の記録に残す（設計 §22 形 1 / 2）。
fn relaunch(read: &Read<'_, '_>, group: &AccountGroup, key: (&str, &str), mut seats: Vec<(String, String)>, wait: Wait) {
    let (input, account) = (read.input, key.0);
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
            let carry = resume_carry(&seat_dir(&place.path, target));
            let carry: Vec<&str> = carry.iter().map(String::as_str).collect();
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
                carry: &carry,
                replace_own: false,
                // 席の箱の行は埋め込み（群の段の manifest・設計 account-lifecycle.md §30 形 4）。
                seat_box: crate::pipe::confine::seat_box_of(&rules),
            });
            if let Some(reason) = launch_failure(&launched) {
                failed.push((anchor.clone(), target.clone(), reason));
            }
            false
        });
        if seats.is_empty() || wait != Wait::Settle || !deadline.is_some_and(|at| Instant::now() < at) {
            break;
        }
        sleep(step);
    }
    match wait {
        Wait::Settle => failed.extend(seats.into_iter().map(|(anchor, target)| (anchor, target, REASON_NOT_SHELL))),
        Wait::Once => {
            let now = crate::seat::state::now_secs();
            for (anchor, target) in &seats {
                let signalled = group::signalled(&seat_dir(&place.path, target), key);
                if !signalled.is_some_and(|at| group::grace_left(at, now, read.grace_s).is_none()) {
                    continue;
                }
                let dialog = group::exit_dialog(WHO_GROUP);
                let _ = notify::send_or_confirm(&read.state, &place, Path::new(anchor), input.manifest, (group::EXIT, &dialog));
            }
        }
    }
    for (anchor, target, reason) in failed {
        let detail = format!("group={} anchor={anchor} target={target} reason={reason}", group.name());
        group::append(input.state_dir, input.manifest, EventKind::GroupMovePending, account, detail);
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

/// 注入と起動の置き場（絶対 path・出所は flag＝列に渡された `--state-dir`）。
fn place(input: &Input<'_>) -> StateDir {
    StateDir {
        path: std::path::absolute(input.state_dir).unwrap_or_else(|_| input.state_dir.to_path_buf()),
        source: Provenance::Flag,
    }
}

/// 1 本が逼迫と判じた（群, 口座）1 つに、新しい実測が在れば送って記す（§19 形 3 / 4）。
fn notice(read: &Read<'_, '_>, group: &AccountGroup, label: &str, found: Pressed) {
    let noticed = |event: &Event| {
        event.account.as_deref() == Some(label)
            && event.pressure().is_some_and(|body| body.group == group.name() && body.window == found.window)
    };
    if !group::measured_since(&read.events, label, Some(found.window), noticed) {
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

/// 通知の event を 1 件記す（読み返せない形は書かない＝append-only の log に読めない行を残さない）。
fn record(input: &Input<'_>, label: &str, pressure: &Pressure) {
    let detail = pressure.render();
    if Pressure::parse(&detail).as_ref() != Some(pressure) {
        return;
    }
    group::append(input.state_dir, input.manifest, EventKind::GroupPressureNotified, label, detail);
}
