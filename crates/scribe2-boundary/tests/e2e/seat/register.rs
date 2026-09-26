//! 席の状態・役割・登録の歯（打刻の門・`seat register` / `doctor` の突合・`register --model`・設計
//! docs/design/seat-roles.md §7・接頭辞 `seat_state_` / `seat_role_` / `seat_register_`）。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。
//! 歯の本文は `seat/account.rs` から**挙動不変で移した**もの（`s2-07l.361`・seat-roles.md §7 の固定した組）。
// flip-check: moved s2-07l.361

use super::*;
use vessel::pipe::declaration::path_kinds::PathKinds;
use vessel::pipe::declaration::DECL_FILE;

// ─────────────────── 席の状態（hook の打刻・typed・`s2-07l.95`） ───────────────────

// ─────────────────────────── role / register ───────────────────────────

/// event log の本文（無ければ空）。
fn role_log(place: &RolePlace) -> String {
    fs::read_to_string(vessel::fleet::store::events_path(&place.state)).unwrap_or_default()
}

/// event log を replay した現在地。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn role_state(place: &RolePlace) -> vessel::fleet::State {
    vessel::fleet::replay(&vessel::fleet::store::read_all(&place.state).expect("event log を読める"))
}

/// 打刻の在る target の登録は `SeatRegistered` を 1 件追記し、replay で 6 項目が読める（歯 (a)(1)）。
#[test]
fn seat_role_register_appends_one_seat_registered_row() {
    let place = role_place();
    role_stamp(&place, "rs:planner", Some("sid-a"));
    let out = role_register(&place, "rs:planner", "orchestrator", &["--anchor", "/repo/anchor"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat register: registered role=orchestrator target=rs:planner sid=sid-a account=acct-1 anchor=/repo/anchor model=Opus\n"
    );
    let events = vessel::fleet::store::read_all(&place.state).unwrap_or_default();
    assert_eq!(events.len(), 1, "1 件だけ: {}", role_log(&place));
    let event = events.first().cloned().unwrap_or_else(|| panic!("行が在る"));
    assert_eq!(event.kind, vessel::fleet::EventKind::SeatRegistered);
    assert!(event.run.is_empty() && event.bead.is_empty(), "便に紐づかない");
    let registration = event.registration.unwrap_or_else(|| panic!("本体が在る"));
    assert_eq!(registration.role, vessel::seat::role::Role::Orchestrator);
    assert_eq!(registration.anchor, "/repo/anchor");
    assert_eq!(registration.target, "rs:planner");
    assert_eq!(registration.sid.as_deref(), Some("sid-a"), "register の口の row は打刻の sid を持つ");
    assert_eq!(registration.account, "acct-1");
    assert_eq!(registration.launch, LAUNCH_BODY, "雛形の本文がそのまま載る");
    let state = role_state(&place);
    assert_eq!(state.runs.len(), 0, "幽霊の便を作らない");
    assert_eq!(vessel::seat::role::role_of_target(&state, "rs:planner"), Some(vessel::seat::role::Role::Orchestrator));
    fs::remove_dir_all(&place.dir).ok();
}

/// 同じ鍵の再登録は前の row を残したまま最新だけが解ける・別 target へ移すと旧 target では解けない・
/// 別の anchor に同じ target を登録すると replay の最新が解ける（歯 (a)(2)(8)）。
///
/// 鍵は **役割 × anchor** で、役割は orchestrator 1 つ（ADR-0045 §2 (1)）なので鍵を分けるのは anchor である
/// ＝弁別は解けた row の `anchor` で測る（役割の名では測れない）。
#[test]
fn seat_role_reregister_keeps_old_rows_and_resolves_the_latest() {
    use vessel::seat::role::{registration_of_target, role_of_target, Role};
    let place = role_place();
    for target in ["rr:one", "rr:two"] {
        role_stamp(&place, target, Some("sid-r"));
    }
    let register = |target: &str, anchor: &str| {
        let out = role_register(&place, target, "orchestrator", &["--anchor", anchor]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    };
    let anchor_of = |state: &vessel::fleet::State, target: &str| {
        registration_of_target(state, target).map(|row| row.anchor.clone())
    };
    let (main, wt) = ("/repo/main", "/repo/.worktrees/wt");
    register("rr:one", main);
    register("rr:one", main);
    assert_eq!(role_log(&place).lines().count(), 2, "前の row は残る（append のみ）");
    let state = role_state(&place);
    assert_eq!(state.registrations.len(), 1, "同じ鍵は 1 つに畳む");
    assert_eq!(state.registrations.values().map(|latest| latest.seq).collect::<Vec<_>>(), vec![1], "2 件目が解決される");
    assert_eq!(role_of_target(&state, "rr:one"), Some(Role::Orchestrator));
    register("rr:one", wt);
    assert_eq!(anchor_of(&role_state(&place), "rr:one").as_deref(), Some(wt), "別の anchor の後の row が勝つ");
    register("rr:two", wt);
    let moved = role_state(&place);
    assert_eq!(anchor_of(&moved, "rr:two").as_deref(), Some(wt));
    assert_eq!(anchor_of(&moved, "rr:one").as_deref(), Some(main), "移した鍵の旧 target は解けず、別の鍵の row が残る");
    register("rr:two", main);
    assert_eq!(role_of_target(&role_state(&place), "rr:one"), None, "どの鍵も持たない target は解けない");
    fs::remove_dir_all(&place.dir).ok();
}

/// 打刻が無い・読めない・sid が空の target は `NoStamp` で rc 1・event を書かない（歯 (a)(3)(4)）。
#[test]
fn seat_role_register_refuses_without_a_stamped_sid() {
    let place = role_place();
    role_stamp(&place, "rn:nostamp", None);
    role_stamp(&place, "rn:empty", Some(""));
    role_stamp(&place, "rn:blank", Some("  "));
    let unreadable = place.state.join("seat").join("rn_dir");
    fs::create_dir_all(state_file(&unreadable)).ok();
    for target in ["rn:absent", "rn:nostamp", "rn:empty", "rn:blank", "rn:dir"] {
        let out = role_register(&place, target, "orchestrator", &["--anchor", "/repo"]);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{target}: stdout={}", stdout_of(&out));
        assert_eq!(stderr_of(&out), format!("seat register: refused reason=no-stamp target={target}\n"));
        assert!(stdout_of(&out).is_empty(), "{target}: stdout は空");
    }
    assert!(!vessel::fleet::store::events_path(&place.state).exists(), "event log に行が増えない");
    fs::remove_dir_all(&place.dir).ok();
}

/// `--anchor` 無しは cwd の repo root が `anchor` に入る（repo の下の dir から撃っても root）（歯 (a)(5)）。
#[test]
fn seat_role_register_defaults_anchor_to_the_cwd_repo_root() {
    let place = role_place();
    let repo = place.dir.join("repo");
    let nested = repo.join("sub").join("deeper");
    fs::create_dir_all(&nested).ok();
    let init = Command::new("git").arg("-C").arg(&repo).args(["init", "-q"]).output();
    assert!(init.is_ok_and(|out| out.status.success()), "tmp repo を作れる");
    let top = Command::new("git").arg("-C").arg(&repo).args(["rev-parse", "--show-toplevel"]).output();
    let root = top.map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned()).unwrap_or_default();
    assert!(!root.is_empty(), "root を実測できる");
    role_stamp(&place, "ra:anchor", Some("sid-anchor"));
    let state = place.state.display().to_string();
    let out = run_seat_in(&nested, &[
        "register", "--state-dir", &state, "--target", "ra:anchor", "--role", "orchestrator", "--account", "acct-1",
        "--launch", &place.launch,
    ]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let anchors: Vec<String> = role_state(&place).registrations.values().map(|latest| latest.registration.anchor.clone()).collect();
    assert_eq!(anchors, vec![root], "cwd の repo root");
    fs::remove_dir_all(&place.dir).ok();
}

/// pane id を差し替えても同じ target なら同じ役割・tmux の env は効かない・window を rename すると解けない・
/// pane 文字列は event log に 0 回（歯 (a)(6)(7)）。
#[test]
fn seat_role_resolution_reads_only_rows_on_isolated_tmux() {
    use vessel::seat::role::{role_of_target, Role};
    use vessel::seat::target_of_pane;
    let place = role_place();
    let seat = start_seat(&place.socket, "rolesess");
    assert!(seat.ready(), "隔離 seat が立つ");
    let pane_of = |window: &str, env: &str| {
        // `-t rolesess:` は session を指す（`rolesess` だけだと同名の window と読まれ index が衝突する）。
        let out = tmux(&place.socket, &["new-window", "-d", "-P", "-F", "#{pane_id}", "-t", "rolesess:", "-n", window, "-e", env, "sh"]);
        (String::from_utf8_lossy(&out.stdout).trim().to_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let (first, why) = pane_of("win", "SCRIBE2_ROLE=admin");
    assert!(first.starts_with('%'), "pane id: {first} stderr={why}");
    let target = target_of_pane(Some(&place.socket), &first).unwrap_or_default();
    assert_eq!(target, "rolesess:win");
    role_stamp(&place, &target, Some("sid-live"));
    let out = role_register(&place, &target, "orchestrator", &["--anchor", "/repo"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let resolve = |pane: &str| target_of_pane(Some(&place.socket), pane).and_then(|found| role_of_target(&role_state(&place), &found));
    assert_eq!(resolve(&first), Some(Role::Orchestrator), "env の値でなく row の役割");
    assert!(tmux(&place.socket, &["kill-window", "-t", "rolesess:win"]).status.success(), "window を畳める");
    let (second, why) = pane_of("win", "SCRIBE2_ROLE=admin");
    assert!(second.starts_with('%') && second != first, "pane id が差し替わる: {second} stderr={why}");
    assert_eq!(resolve(&second), Some(Role::Orchestrator), "同じ target なら同じ役割");
    assert!(tmux(&place.socket, &["rename-window", "-t", "rolesess:win", "moved"]).status.success(), "rename できる");
    assert_eq!(resolve(&second), None, "rename した window は別 target＝解けない");
    let log = role_log(&place);
    for pane in [&first, &second] {
        assert_eq!(log.matches(pane.as_str()).count(), 0, "pane 文字列は event log に現れない: {log}");
    }
    assert!(!log.contains("\"%"), "pane の形の値も無い: {log}");
    drop(seat);
    fs::remove_dir_all(&place.dir).ok();
}

/// 未知の `--role`・値欠けは使い方で断り、usage に register が載る（歯 (a)(9)）。
#[test]
fn seat_role_register_refuses_unknown_role_with_usage() {
    let place = role_place();
    role_stamp(&place, "ru:x", Some("sid-u"));
    let usage = stderr_of(&run_seat(&[]));
    // 注入の口が消えて `register` が使い方の先頭の verb になった（`s2-07l.479.3`）＝区切りは `<`。
    assert!(usage.contains("<register --state-dir S --target T --role R --account L --launch FILE [--anchor DIR]"), "{usage}");
    for role in ["Planner", "reviewer", ""] {
        let out = role_register(&place, "ru:x", role, &["--anchor", "/repo"]);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "role={role:?}");
        assert_eq!(stderr_of(&out), usage, "role={role:?} は使い方で断る");
    }
    let out = run_seat(&["register", "--target", "ru:x", "--role", "orchestrator"]);
    assert_eq!(stderr_of(&out), usage, "必須 flag の欠けも使い方");
    assert!(!vessel::fleet::store::events_path(&place.state).exists(), "行を書かない");
    fs::remove_dir_all(&place.dir).ok();
}

/// 役割は 1 つ（orchestrator・ADR-0045 §2 (1)）で宣言順・variant 名の字面は受けない・受付の極性は
/// in-loop / fail-closed。
#[test]
fn seat_role_enum_is_closed_in_declaration_order() {
    use vessel::polarity::{OnFailure, Timing};
    use vessel::seat::role::{Role, ALL, POLARITY};
    assert_eq!(ALL.len(), 1, "記録時点の母集団");
    assert!(vessel::order::is_declaration_order(ALL, |role| role as usize), "ALL は宣言順: {ALL:?}");
    assert_eq!(Role::parse("Orchestrator"), None, "variant 名は字面でない");
    assert_eq!(Role::parse(""), None, "空は役割でない");
    assert_eq!((POLARITY.timing, POLARITY.on_failure), (Timing::InLoop, OnFailure::FailClosed));
}

/// 突合の行は畳んだ行を数え、log を読めない周・tmux を撃てない周は 0 と書かない（pure・歯 (d)）。
#[test]
fn seat_role_reconcile_line_counts_folded_rows_and_never_writes_zero_for_unmeasured() {
    use vessel::seat::role::render_reconcile;
    let place = role_place();
    for target in ["rc:gone", "rc:live", "rc:away"] {
        role_stamp(&place, target, Some("sid-rc"));
    }
    // 鍵は (役割, anchor) で役割は 1 つ＝同じ anchor の 2 件が 1 つに畳まれ、別 anchor の 1 件が残る。
    for (target, anchor) in [("rc:gone", "/repo"), ("rc:live", "/repo"), ("rc:away", "/repo/away")] {
        assert_eq!(rc_of(&role_register(&place, target, "orchestrator", &["--anchor", anchor])), i32::from(RC_OK));
    }
    let state = role_state(&place);
    let live = vec!["rc:live".to_owned(), "rc:gone".to_owned()];
    assert_eq!(render_reconcile(Some(&state), Some(&live)), "seats: registered=2 live=1 missing=1", "同じ鍵の旧 row は数えない");
    assert_eq!(render_reconcile(Some(&state), None), "seats: registered=2 live=unmeasurable missing=unmeasurable");
    assert_eq!(render_reconcile(None, Some(&live)), "seats: registered=unreadable live=unmeasurable missing=unmeasurable");
    fs::remove_dir_all(&place.dir).ok();
}

/// doctor は登録 row と実在の target を突き合わせ、撃てない周は 0 と書かない（歯 (d)）。末尾は登録 row の anchor の
/// 導入先の行（consumer-sync.md §4・`s2-07l.303`）。
#[test]
fn seat_role_doctor_reconciles_rows_with_live_targets() {
    let place = role_doctor_place();
    // 実在しない target の row を 1 件足す（鍵を分けるのは anchor・`missing` の面を測る）。
    crate::seat::role_register_extra(&place, "gone:gone", "/repo/gone");
    let out = role_doctor(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let tail = |out: &Output| stdout_of(out).lines().rev().take(5).map(str::to_owned).collect::<Vec<String>>();
    let consumer_gone = CONSUMER_REPO.replace("consumer=/repo ", "consumer=/repo/gone ");
    assert_eq!(
        tail(&out),
        [
            HOST_GUARD_BARE,
            consumer_gone.as_str(),
            CONSUMER_REPO,
            HOST_ABSENT,
            "seats: registered=2 live=unmeasurable missing=unmeasurable"
        ],
        "突合の行の直後に host の面の行・anchor ごとの導入先の行・末尾に host-guard の 1 行"
    );
    // 登録 row の行は行から導いた model と `--rules` の manifest の既定の欄を持つ（本歯の manifest は行を持たない＝理由の字面・
    // 設計 seat-roles.md §20 の約束 5 / 8）。
    let seats: Vec<String> = stdout_of(&out).lines().filter(|line| line.starts_with("seat: ")).map(str::to_owned).collect();
    assert_eq!(seats.len(), 2, "{seats:?}");
    assert!(seats.iter().all(|line| line.contains(" model=Opus default=no-rule:missing paths=")), "{seats:?}");
    let seat = start_seat(&place.socket, "rolesdoc");
    assert!(seat.ready(), "隔離 seat が立つ");
    let out = role_doctor(&place);
    let lines: Vec<String> = stdout_of(&out).lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 11, "2 行 + host-template 1 行 + init 1 行 + 登録 row 2 行 + 突合 1 行 + host の面 1 行 + 導入先 2 行 + host-guard 1 行: {lines:?}");
    assert_eq!(
        tail(&out),
        [HOST_GUARD_BARE, consumer_gone.as_str(), CONSUMER_REPO, HOST_ABSENT, "seats: registered=2 live=1 missing=1"]
    );
    let doctor = |args: &[&str]| Command::new(bin()).arg("doctor").args(args).output().ok();
    let bare = doctor(&[]).map(|out| stdout_of(&out)).unwrap_or_default();
    assert_eq!(bare.lines().count(), 3, "引数無しは骨格の 2 行 + host-template 1 行: {bare}");
    assert!(bare.lines().nth(2).is_some_and(|line| line.starts_with("host-template=")), "骨格の直後: {bare}");
    let state = place.state.display().to_string();
    for bad in [&["--state-dir"][..], &["--state-dir", ""], &["--tmux-socket", "s"], &["--state-dir", &state, "--bogus", "x"], &["--state-dir", &state, "--state-dir", &state]] {
        let out = doctor(bad);
        assert_eq!(out.as_ref().map(rc_of), Some(i32::from(RC_REFUSED)), "{bad:?} は使い方で断る");
        assert!(out.is_some_and(|found| stdout_of(&found).starts_with("usage: ")), "{bad:?}");
    }
    drop(seat);
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 登録 row の退役（account-lifecycle.md §24・契約表の行 m・接頭辞 `seat_retire_`） ───────────────────

/// `seat retire` を 1 回撃つ（`extra` は `--reason` などの追加 flag）。
fn role_retire(place: &RolePlace, target: &str, extra: &[&str]) -> Output {
    let state = place.state.display().to_string();
    let mut args = vec!["retire", "--state-dir", &state, "--target", target];
    args.extend_from_slice(extra);
    run_seat(&args)
}

/// 形 1: 登録済みの target の退役は `SeatRetired` を 1 件積み（role / anchor / target / account は row の写し・`detail` = reason・
/// actor は human・便に紐づかない）、rc 0 で stdout に 1 行。replay はその row を読まなくなる（base では `retire` が使い方の誤り＝RED）。
#[test]
fn seat_retire_records_one_event_with_the_row_copy_and_prints_one_line() {
    use vessel::fleet::EventKind;
    let place = role_place();
    role_stamp(&place, "rt:one", Some("sid-t"));
    let out = role_register(&place, "rt:one", "orchestrator", &["--anchor", "/repo/rt"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let out = role_retire(&place, "rt:one", &["--reason", "moved to another place"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), "seat retire: retired target=rt:one role=orchestrator account=acct-1\n");
    assert!(stderr_of(&out).is_empty(), "stderr 0 byte: {}", stderr_of(&out));
    let events = vessel::fleet::store::read_all(&place.state).unwrap_or_default();
    let kinds: Vec<EventKind> = events.iter().map(|event| event.kind).collect();
    assert_eq!(kinds, [EventKind::SeatRegistered, EventKind::SeatRetired], "退役の event は 1 件: {}", role_log(&place));
    let retired = events.last().cloned().unwrap_or_else(|| panic!("行が在る"));
    assert_eq!(retired.actor, "human", "退役は人由来");
    assert_eq!(retired.detail.as_deref(), Some("moved to another place"), "detail は reason の逐語");
    assert!(retired.run.is_empty() && retired.bead.is_empty(), "便に紐づかない");
    let row = retired.registration.unwrap_or_else(|| panic!("本体は row の写し"));
    let registered = events.first().and_then(|event| event.registration.clone()).unwrap_or_else(|| panic!("登録の本体"));
    assert_eq!(
        (row.role, row.anchor.as_str(), row.target.as_str(), row.account.as_str()),
        (vessel::seat::role::Role::Orchestrator, "/repo/rt", "rt:one", "acct-1"),
        "role / anchor / target / account"
    );
    assert_eq!(row, registered, "row の写し（項目を書き換えない）");
    let state = role_state(&place);
    assert_eq!(vessel::seat::role::registration_of_target(&state, "rt:one"), None, "退役した row は解けない");
    assert!(state.registrations.is_empty() && state.runs.is_empty() && state.seats.is_empty(), "{state:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// 形 1: 登録 row の無い target（log が無い・別の target だけ・退役済み）は `no-row` で rc 1・stdout 0 byte・event 0。
#[test]
fn seat_retire_refuses_a_target_without_a_row_and_writes_no_event() {
    let place = role_place();
    let out = role_retire(&place, "rt:absent", &[]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(stderr_of(&out), "seat retire: refused reason=no-row target=rt:absent\n");
    assert!(stdout_of(&out).is_empty(), "stdout は空");
    assert!(!vessel::fleet::store::events_path(&place.state).exists(), "log の無い置き場に行を作らない");
    role_stamp(&place, "rt:kept", Some("sid-k"));
    assert_eq!(rc_of(&role_register(&place, "rt:kept", "orchestrator", &["--anchor", "/repo/kept"])), i32::from(RC_OK));
    let other = role_retire(&place, "rt:other", &[]);
    assert_eq!(rc_of(&other), i32::from(RC_REFUSED));
    assert_eq!(stderr_of(&other), "seat retire: refused reason=no-row target=rt:other\n");
    assert_eq!(role_log(&place).lines().count(), 1, "別の target だけの log は増えない");
    assert_eq!(rc_of(&role_retire(&place, "rt:kept", &[])), i32::from(RC_OK), "row の在る target は退役できる");
    let again = role_retire(&place, "rt:kept", &[]);
    assert_eq!(rc_of(&again), i32::from(RC_REFUSED), "退役済みは row が無い");
    assert_eq!(stderr_of(&again), "seat retire: refused reason=no-row target=rt:kept\n");
    assert_eq!(role_log(&place).lines().count(), 2, "2 度目の退役は event を書かない: {}", role_log(&place));
    fs::remove_dir_all(&place.dir).ok();
}

/// 形 1: 値欠け・空文字・`S:W` でない target・空の reason は使い方で断る（rc 1・event 0）。
#[test]
fn seat_retire_refuses_malformed_args_with_usage_and_no_event() {
    let place = role_place();
    let usage = stderr_of(&run_seat(&[]));
    let state = place.state.display().to_string();
    for bad in [
        &["retire", "--state-dir", &state][..],
        &["retire", "--state-dir", &state, "--target", ""],
        &["retire", "--state-dir", &state, "--target", "nowindow"],
        &["retire", "--target", "rt:absent"],
        &["retire", "--state-dir", &state, "--target", "rt:absent", "--reason", ""],
        &["retire", "--state-dir", &state, "--target", "rt:absent", "--reason"],
    ] {
        let out = run_seat(bad);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{bad:?}");
        assert_eq!(stderr_of(&out), usage, "{bad:?} は使い方で断る");
    }
    assert!(!vessel::fleet::store::events_path(&place.state).exists(), "使い方の断りも行を書かない");
    fs::remove_dir_all(&place.dir).ok();
}

/// 形 3: 退役の後の doctor は `registered=` が 1 減り、退役した row の行を出さない（missing にも数えない・読み手は不変）。
#[test]
fn seat_retire_drops_the_row_from_the_doctor_reconcile() {
    let place = role_doctor_place();
    crate::seat::role_register_extra(&place, "gone:gone", "/repo/gone");
    let seats_of = |out: &Output| stdout_of(out).lines().filter(|line| line.starts_with("seat: ") || line.starts_with("seats: ")).map(str::to_owned).collect::<Vec<String>>();
    let before = seats_of(&role_doctor(&place));
    assert_eq!(before.last().map(String::as_str), Some("seats: registered=2 live=unmeasurable missing=unmeasurable"), "{before:?}");
    assert_eq!(before.len(), 3, "row 2 行 + 突合 1 行: {before:?}");
    let out = role_retire(&place, "gone:gone", &["--reason", "gone"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let after = seats_of(&role_doctor(&place));
    assert_eq!(after.last().map(String::as_str), Some("seats: registered=1 live=unmeasurable missing=unmeasurable"), "1 減る: {after:?}");
    assert_eq!(after.len(), 2, "退役した row の行は出ない: {after:?}");
    assert!(after.iter().all(|line| !line.contains("target=gone:gone")), "{after:?}");
    assert_eq!(after.first(), before.iter().find(|line| line.contains("target=rolesdoc:rolesdoc")), "残る row の行は 1 字も変わらない");
    fs::remove_dir_all(&place.dir).ok();
}

/// 形 2: 退役の後に同じ target を `seat register` すると row が戻る（物理順で後の登録が勝つ・退役の event は log に残る）。
#[test]
fn seat_retire_then_register_brings_the_row_back() {
    use vessel::seat::role::registration_of_target;
    let place = role_place();
    role_stamp(&place, "rt:back", Some("sid-b"));
    assert_eq!(rc_of(&role_register(&place, "rt:back", "orchestrator", &["--anchor", "/repo/back"])), i32::from(RC_OK));
    assert_eq!(rc_of(&role_retire(&place, "rt:back", &[])), i32::from(RC_OK));
    assert_eq!(registration_of_target(&role_state(&place), "rt:back"), None, "退役の直後は row が無い");
    let out = role_register(&place, "rt:back", "orchestrator", &["--anchor", "/repo/back"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let state = role_state(&place);
    let row = registration_of_target(&state, "rt:back").unwrap_or_else(|| panic!("row が戻る"));
    assert_eq!((row.anchor.as_str(), row.account.as_str()), ("/repo/back", "acct-1"));
    assert_eq!(state.registrations.values().map(|latest| latest.seq).collect::<Vec<_>>(), vec![2], "3 件目の登録が解決される");
    assert_eq!(role_log(&place).lines().count(), 3, "登録 → 退役 → 登録（append のみ）");
    assert_eq!(rc_of(&role_retire(&place, "rt:back", &[])), i32::from(RC_OK), "戻った row はもう一度退役できる");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────────────── register --model（契約 (e)・s2-07l.215） ───────────────────────────

/// target の登録 row の `model`（replay の読み手 `registration_of_target` から運ぶ）。
fn model_of_target(place: &RolePlace, target: &str) -> Option<String> {
    vessel::seat::role::registration_of_target(&role_state(place), target).and_then(|row| row.model.clone())
}

/// `--model Opus` で登録 → `SeatRegistered` 1 件の束に model = Opus・行の key に `"model":"Opus"`・
/// replay の読み手が `Some("Opus")` を返す・出力行の末尾に `model=Opus`（歯 (e)(1)・flip の RED）。
#[test]
fn seat_register_model_lands_in_the_row_and_the_reader_returns_it() {
    let place = role_place();
    role_stamp(&place, "rm:planner", Some("sid-m"));
    let out = role_register(&place, "rm:planner", "orchestrator", &["--anchor", "/repo/anchor", "--model", "Opus"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat register: registered role=orchestrator target=rm:planner sid=sid-m account=acct-1 anchor=/repo/anchor model=Opus\n"
    );
    let events = vessel::fleet::store::read_all(&place.state).unwrap_or_default();
    assert_eq!(events.len(), 1, "1 件だけ: {}", role_log(&place));
    let registration = events.first().and_then(|event| event.registration.clone()).unwrap_or_else(|| panic!("本体が在る"));
    assert_eq!(registration.model.as_deref(), Some("Opus"));
    assert_eq!(registration.role, vessel::seat::role::Role::Orchestrator, "他の項目はそのまま");
    assert_eq!(registration.account, "acct-1");
    let log = role_log(&place);
    assert!(log.contains("\"model\":\"Opus\""), "行の key: {log}");
    assert!(log.contains("\"schema\":1"), "schema 1 のまま: {log}");
    let state = role_state(&place);
    let row = vessel::seat::role::registration_of_target(&state, "rm:planner").unwrap_or_else(|| panic!("row が在る"));
    assert_eq!(row.model.as_deref(), Some("Opus"), "読み手が model を運ぶ");
    assert_eq!(row.role, vessel::seat::role::Role::Orchestrator);
    assert_eq!(vessel::seat::role::role_of_target(&state, "rm:planner"), Some(vessel::seat::role::Role::Orchestrator), "役割の解決は同じ 1 本");
    assert_eq!(vessel::seat::role::registration_of_target(&state, "rm:absent"), None, "登録の無い target は None");
    fs::remove_dir_all(&place.dir).ok();
}

/// `--model` 無しで登録 → 器が役割の既定の行から導いた値（表示名 `Opus`）が row に載り、出力行の末尾にも `model=Opus`
/// （歯 (e)(2)・設計 seat-roles.md §20 の約束 5 で「省いた周は導出値」へ期待値が変わった）。旧 row（`model` の key 無し）は
/// 読み手が `None` で読む形のまま（fleet の歯が測る）。
#[test]
fn seat_register_model_absent_reads_as_none_and_keeps_the_old_row_form() {
    let place = role_place();
    role_stamp(&place, "rm:plain", Some("sid-p"));
    let out = role_register(&place, "rm:plain", "orchestrator", &["--anchor", "/repo/anchor"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat register: registered role=orchestrator target=rm:plain sid=sid-p account=acct-1 anchor=/repo/anchor model=Opus\n"
    );
    let log = role_log(&place);
    assert_eq!(log.lines().count(), 1, "1 件: {log}");
    assert!(log.contains("\"model\":\"Opus\""), "導出値が key に載る: {log}");
    assert_eq!(model_of_target(&place, "rm:plain").as_deref(), Some("Opus"));
    assert!(vessel::seat::role::registration_of_target(&role_state(&place), "rm:plain").is_some(), "row は在る");
    fs::remove_dir_all(&place.dir).ok();
}

/// `--model ''`（空白だけも）・値欠けは使い方で断り rc 1・event を書かない（歯 (e)(3)）。
#[test]
fn seat_register_model_empty_is_refused_with_usage_and_no_event() {
    let place = role_place();
    role_stamp(&place, "rm:empty", Some("sid-e"));
    let usage = stderr_of(&run_seat(&[]));
    for extra in [&["--anchor", "/repo", "--model", ""][..], &["--anchor", "/repo", "--model", "  "], &["--anchor", "/repo", "--model"]] {
        let out = role_register(&place, "rm:empty", "orchestrator", extra);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{extra:?}: stdout={}", stdout_of(&out));
        assert_eq!(stderr_of(&out), usage, "{extra:?} は使い方で断る");
        assert!(stdout_of(&out).is_empty(), "{extra:?}: stdout は空");
    }
    assert!(!vessel::fleet::store::events_path(&place.state).exists(), "event log に行が増えない");
    fs::remove_dir_all(&place.dir).ok();
}

/// 同じ鍵の再登録 → 最新が効く（付ける → 外す・前の row は残る）（歯 (e)(4)）。`--model` は行との照合なので（設計 seat-roles.md
/// §20 の約束 5）、行と食い違う値の再登録は断られ row は前の値のまま・外した再登録は行から導いた値。
#[test]
fn seat_register_model_reregistration_replaces_the_model_with_the_latest() {
    let place = role_place();
    role_stamp(&place, "rm:again", Some("sid-g"));
    let register = |extra: &[&str]| role_register(&place, "rm:again", "orchestrator", extra);
    let out = register(&["--anchor", "/repo/main", "--model", "Opus"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(model_of_target(&place, "rm:again").as_deref(), Some("Opus"));
    let other = register(&["--anchor", "/repo/main", "--model", "Fable"]);
    assert_eq!(rc_of(&other), i32::from(RC_REFUSED), "行と食い違う値は断る: stdout={}", stdout_of(&other));
    assert_eq!(model_of_target(&place, "rm:again").as_deref(), Some("Opus"), "断った周は row が変わらない");
    let out = register(&["--anchor", "/repo/main"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(model_of_target(&place, "rm:again").as_deref(), Some("Opus"), "外した再登録は行から導いた値");
    assert_eq!(role_log(&place).lines().count(), 2, "前の row は残る（append のみ・断った周は書かない）");
    assert_eq!(role_state(&place).registrations.len(), 1, "同じ鍵は 1 つに畳む");
    fs::remove_dir_all(&place.dir).ok();
}

/// doctor の登録 row の一覧に `model` の欄（鍵の順・突合の行の前）（歯 (e)(5)）。`--model` を省いた登録も行から導いた値を
/// 持つ（設計 seat-roles.md §20 の約束 5）。`default=` の欄は `--rules` の manifest の既定（本歯の manifest は行を持たない＝
/// 理由の字面）。末尾の `paths=` の欄は anchor の宣言の state（存在しない anchor は宣言 file が無い repo と同じ＝`default`・§24）。
#[test]
fn seat_register_model_shows_in_the_doctor_rows_with_dash_for_none() {
    let place = role_place();
    for (target, role, extra) in [("rm:doc-a", "orchestrator", &["--anchor", "/repo/a", "--model", "Opus"][..]), ("rm:doc-b", "orchestrator", &["--anchor", "/repo/b"])] {
        role_stamp(&place, target, Some("sid-d"));
        let out = role_register(&place, target, role, extra);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    }
    let out = role_doctor(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let lines: Vec<String> = stdout_of(&out).lines().map(str::to_owned).collect();
    assert_eq!(
        lines.get(4..),
        Some(&[
            format!("seat: role=orchestrator anchor=/repo/a target=rm:doc-a account=acct-1 model=Opus default=no-rule:missing paths=default{TICK_TAIL}"),
            format!("seat: role=orchestrator anchor=/repo/b target=rm:doc-b account=acct-1 model=Opus default=no-rule:missing paths=default{TICK_TAIL}"),
            "seats: registered=2 live=unmeasurable missing=unmeasurable".to_owned(),
            HOST_ABSENT.to_owned(),
            consumer_line_of("/repo/a"),
            consumer_line_of("/repo/b"),
            HOST_GUARD_BARE.to_owned(),
        ][..]),
        "{lines:?}"
    );
    let rows = vessel::seat::role::render_rows(&role_state(&place), |_| PathKinds::Default, |_| Err(vessel::seat::RuleRead::Missing));
    let rows: Vec<String> = rows.into_iter().map(|row| format!("{row}{TICK_TAIL}")).collect();
    assert_eq!(rows, lines.get(4..6).unwrap_or_default(), "pure の一覧に `paths=` の後ろの 2 項目を足した形と同じ");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────────────── doctor の paths の欄（契約行 r・s2-07l.491・seat-roles.md §24） ───────────────────────────

/// 宣言の本文（必須 key + 追加の行）。
fn paths_declaration(extra: &str) -> String {
    format!("schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git status\"]\n{extra}")
}

/// `dir` に commit 1 つの git repo を作り、`declaration` が在ればその本文の `.vessel.toml` を commit する。
/// 返りは anchor に使う path（tmp の実体 path・`--anchor` は絶対化するだけ）。
fn paths_repo(place: &RolePlace, name: &str, declaration: Option<&str>) -> String {
    let dir = place.dir.join(name);
    assert!(crate::git_repo_at(&dir).is_some(), "{name}: git repo を作れる");
    if let Some(body) = declaration {
        assert!(fs::write(dir.join(DECL_FILE), body).is_ok(), "{name}: 宣言を書ける");
        assert!(crate::git_out(&dir, &["add", "-A"]).is_some(), "{name}: add");
        assert!(crate::git_out(&dir, &["commit", "-q", "-m", "declare"]).is_some(), "{name}: commit");
    }
    dir.display().to_string()
}

/// `paths=` の後ろの 2 項目（打刻の無い席・周期の行を持たない [`NO_ACCOUNT_RULES`] の周・seat-heartbeat.md §12 行 p 形 3）。
const TICK_TAIL: &str = " heartbeat=on tick=no-rule:missing";

/// doctor の登録 row の行のうち anchor `anchor` の 1 行（無ければ空＝呼び側の assert が落ちる）。
fn seat_line_of(lines: &[String], anchor: &str) -> String {
    let needle = format!(" anchor={anchor} ");
    lines.iter().find(|line| line.starts_with("seat: ") && line.contains(&needle)).cloned().unwrap_or_default()
}

/// doctor の席の行の末尾の `paths=` の欄は 3 つの state を名乗る（§24・行は増えない）: 宣言 file を持たない anchor
/// （存在しない dir も同じ）と key を 1 本も書かない宣言は `paths=default`、key を書いた宣言は `paths=declared:<数>`、
/// 不正な宣言は `paths=invalid:<理由>`。総行数は欄の追加で変わらない（2 行 + host-template + init + row + 突合 + host +
/// 導入先 + host-guard）。
#[test]
fn seat_role_doctor_paths_names_default_declared_and_invalid_per_anchor() {
    let place = role_doctor_place();
    let bare = paths_repo(&place, "bare", None);
    let keyless = paths_repo(&place, "keyless", Some(&paths_declaration("")));
    let two = paths_repo(&place, "two", Some(&paths_declaration("design-intent-paths = [\"spec/\"]\ntests-paths = [\"t/\"]\n")));
    let three = paths_repo(
        &place,
        "three",
        Some(&paths_declaration("design-intent-paths = [\"spec/\"]\ndesign-doc-paths = [\"DESIGN.md\"]\ntests-paths = [\"t/\"]\n")),
    );
    let overlap = paths_repo(&place, "overlap", Some(&paths_declaration("design-intent-paths = [\"spec/\"]\ndesign-doc-paths = [\"spec/design/\"]\n")));
    for (target, anchor) in [("pd:bare", &bare), ("pd:keyless", &keyless), ("pd:two", &two), ("pd:three", &three), ("pd:overlap", &overlap)] {
        crate::seat::role_register_extra(&place, target, anchor);
    }
    let out = role_doctor(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let lines: Vec<String> = stdout_of(&out).lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 2 + 1 + 1 + 6 + 1 + 1 + 6 + 1,"欄の追加で行は増えない（末尾は host-guard の 1 行）: {lines:?}");
    assert_eq!(
        seat_line_of(&lines, "/repo"),
        format!("seat: role=orchestrator anchor=/repo target=rolesdoc:rolesdoc account=acct-1 model=Opus default=no-rule:missing paths=default{TICK_TAIL}"),
        "存在しない anchor は宣言 file が無い repo と同じ"
    );
    for (anchor, want) in [
        (&bare, "paths=default"),
        (&keyless, "paths=default"),
        (&two, "paths=declared:2"),
        (&three, "paths=declared:3"),
        (&overlap, "paths=invalid:overlap"),
    ] {
        let line = seat_line_of(&lines, anchor);
        assert!(line.ends_with(&format!("{want}{TICK_TAIL}")), "{anchor}: `paths=` の欄 {want} と 2 項目: {line}");
        assert_eq!(line.matches("paths=").count(), 1, "{line}");
    }
    assert!(!lines.iter().any(|line| line.starts_with("paths:")), "paths の行は足さない（席の行の欄で名乗る）: {lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// `invalid` の理由の字面は 5 種のそれぞれが doctor の欄に出る（`..` の段 / 絶対 path / 空文字 / 重なり / 読めない）。
/// commit していない作業ツリーの宣言は効かない（key を書いた file を置いただけの repo は `default` のまま・HEAD の宣言
/// を作業ツリーで壊しても HEAD の state のまま）。
#[test]
fn seat_role_doctor_paths_invalid_reasons_and_worktree_declarations() {
    let place = role_doctor_place();
    let cases: [(&str, &str, &str); 5] = [
        ("parent", "tests-paths = [\"t/../u/\"]\n", "parent-segment"),
        ("absolute", "tests-paths = [\"/t/\"]\n", "absolute"),
        ("empty", "tests-paths = [\" \"]\n", "empty"),
        ("overlap", "design-doc-paths = [\"docs/\"]\ntests-paths = [\"docs/t/\"]\n", "overlap"),
        ("unreadable", "tests-paths = \"t/\"\n", "unreadable"),
    ];
    let anchors: Vec<String> = cases.iter().map(|(name, extra, _)| paths_repo(&place, name, Some(&paths_declaration(extra)))).collect();
    let uncommitted = paths_repo(&place, "uncommitted", None);
    let written = fs::write(Path::new(&uncommitted).join(DECL_FILE), paths_declaration("tests-paths = [\"t/\"]\n"));
    assert!(written.is_ok(), "作業ツリーの宣言を書ける");
    let broken = paths_repo(&place, "broken", Some(&paths_declaration("tests-paths = [\"t/\"]\n")));
    let written = fs::write(Path::new(&broken).join(DECL_FILE), paths_declaration("tests-paths = [\"/t/\"]\n"));
    assert!(written.is_ok(), "作業ツリーの壊れた宣言を書ける");
    for (at, anchor) in anchors.iter().chain([&uncommitted, &broken]).enumerate() {
        crate::seat::role_register_extra(&place, &format!("pi:{at}"), anchor);
    }
    let lines = doctor_rows(&place, NO_ACCOUNT_RULES);
    for ((_, _, reason), anchor) in cases.iter().zip(&anchors) {
        let line = seat_line_of(&lines, anchor);
        assert!(line.ends_with(&format!(" paths=invalid:{reason}{TICK_TAIL}")), "{anchor}: {line}");
    }
    assert!(seat_line_of(&lines, &uncommitted).ends_with(&format!(" paths=default{TICK_TAIL}")), "作業ツリーの宣言は効かない");
    assert!(seat_line_of(&lines, &broken).ends_with(&format!(" paths=declared:1{TICK_TAIL}")), "HEAD の宣言で名乗る（作業ツリーの壊れは見ない）");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────────────── doctor の口座の行（s2-07l.233・account-autonomy.md §5） ───────────────────────────

/// 登録 row の anchor `anchor` の導入先の行（[`CONSUMER_REPO`] と同じ形）。
fn consumer_line_of(anchor: &str) -> String {
    CONSUMER_REPO.replacen("/repo", anchor, 1)
}

// ─────────────────── 席の既定の model と effort（設計 seat-roles.md §20・`s2-07l.433`・接頭辞 `seat_defaults_`） ───────────────────

/// 約束 5 / 6: `--model` を省いた登録は器が行から導いた値を row に書き、その字面は実測の行と同じ**表示名**（`Opus`・別名の
/// `opus` ではない）・row に effort の項目は載らない。行と食い違う `--model Fable` は登録の断りの閉じた列の 1 つ
/// （`reason=model-mismatch`）で rc 1・event を 1 件も書かない。
#[test]
fn seat_defaults_register_writes_the_derived_model_and_refuses_a_mismatch() {
    let place = role_place();
    role_stamp(&place, "rd:mismatch", Some("sid-x"));
    let refused = role_register(&place, "rd:mismatch", "orchestrator", &["--anchor", "/repo/x", "--model", "Fable"]);
    assert_eq!(rc_of(&refused), i32::from(RC_REFUSED), "stdout={}", stdout_of(&refused));
    assert!(stdout_of(&refused).is_empty(), "stdout は空");
    assert_eq!(stderr_of(&refused), "seat register: refused reason=model-mismatch target=rd:mismatch\n");
    assert!(!vessel::fleet::store::events_path(&place.state).exists(), "event を 1 件も書かない");

    role_stamp(&place, "rd:derived", Some("sid-d"));
    let out = role_register(&place, "rd:derived", "orchestrator", &["--anchor", "/repo/d"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let rows: Vec<vessel::fleet::Registration> =
        vessel::fleet::store::read_all(&place.state).unwrap_or_default().into_iter().filter_map(|event| event.registration).collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows.first().and_then(|row| row.model.as_deref()), Some("Opus"), "表示名（実測の行と同じ語彙）: {rows:?}");
    let log = role_log(&place);
    assert!(log.contains("\"model\":\"Opus\"") && !log.contains("\"model\":\"opus\""), "別名の字面を row に書かない: {log}");
    assert!(!log.contains("effort"), "row に effort の項目は無い: {log}");
    // 別名で渡しても row に載るのは行と同じ表示名。
    let alias = role_register(&place, "rd:derived", "orchestrator", &["--anchor", "/repo/d", "--model", "opus"]);
    assert_eq!(rc_of(&alias), i32::from(RC_OK), "stderr={}", stderr_of(&alias));
    assert!(stdout_of(&alias).ends_with(" model=Opus\n"), "{}", stdout_of(&alias));
    assert!(!role_log(&place).contains("\"model\":\"opus\""), "別名は表示名で書く: {}", role_log(&place));
    fs::remove_dir_all(&place.dir).ok();
}

/// `[[rule]]` 2 行（役割の既定の対・発効は `enabled`）の manifest の本文。
fn defaults_rules(model: &str, effort: &str, enabled: bool) -> String {
    let row = |id: &str, kind: &str, value: &str| {
        format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = \"{value}\"\nenabled = {enabled}\nruling = \"r\"\nruled_at = \"d\"\n")
    };
    format!(
        "{NO_ACCOUNT_RULES}{}{}",
        row("seat.model.orchestrator", "RoleModel", model),
        row("seat.effort.orchestrator", "RoleEffort", effort)
    )
}

/// 約束 8: doctor の登録 row の行は `model=` の直後に行の既定を 1 語添える（`--rules` 無しは埋め込みの行＝`default=Opus/xhigh`・
/// `--rules` の行の値が違えばその値）。行を読めない manifest（行なし・不発効）は既定の語を出さず理由の字面（`no-rule:<variant>`）を
/// 出し、rc は 0 のまま・行の数も変わらない。突合の面は doctor の 1 つだけ（`seat register` の出力行は既定を添えない）。
#[test]
fn seat_defaults_doctor_adds_the_row_default_or_names_why_it_cannot() {
    let place = role_doctor_place();
    let state = place.state.display().to_string();
    let bare = Command::new(bin()).args(["doctor", "--state-dir", &state, "--tmux-socket", &place.socket]).output();
    let bare = bare.map(|out| (rc_of(&out), stdout_of(&out))).unwrap_or_default();
    assert_eq!(bare.0, i32::from(RC_OK), "{}", bare.1);
    let row_of = |text: &str| text.lines().find(|line| line.starts_with("seat: ")).map(str::to_owned).unwrap_or_default();
    assert_eq!(
        row_of(&bare.1),
        "seat: role=orchestrator anchor=/repo target=rolesdoc:rolesdoc account=acct-1 model=Opus default=Opus/xhigh paths=default heartbeat=on tick=absent",
        "埋め込みの行の既定（周期の行も埋め込み＝打刻の無い席は absent）"
    );
    let count = doctor_rows(&place, NO_ACCOUNT_RULES).len();
    for (body, want) in [
        (defaults_rules("fable", "high", true), "default=Fable/high"),
        (defaults_rules("opus", "xhigh", false), "default=no-rule:disabled"),
        (NO_ACCOUNT_RULES.to_owned(), "default=no-rule:missing"),
    ] {
        let lines = doctor_rows(&place, &body);
        let row = lines.iter().find(|line| line.starts_with("seat: ")).cloned().unwrap_or_default();
        assert!(row.contains(&format!(" model=Opus {want} paths=")), "{want}: {row}");
        assert_eq!(row.matches("default=").count(), 1, "1 語だけ: {row}");
        assert_eq!(lines.len(), count, "{want}: 行の数は変わらない: {lines:?}");
    }
    fs::remove_dir_all(&place.dir).ok();
}
