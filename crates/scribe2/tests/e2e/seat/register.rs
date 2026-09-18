//! 席の状態・役割・登録の歯（打刻の門・`seat register` / `doctor` の突合・`register --model`・設計
//! docs/design/seat-roles.md §7・接頭辞 `seat_state_` / `seat_role_` / `seat_register_`）。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。
//! 歯の本文は `seat/account.rs` から**挙動不変で移した**もの（`s2-07l.361`・seat-roles.md §7 の固定した組）。
// flip-check: moved s2-07l.361

use super::*;

// ─────────────────── 席の状態（hook の打刻・typed・`s2-07l.95`） ───────────────────

/// 走行中の実席（2026-09-11・匿名化済み）: spinner 行は `esc to interrupt` を**持たず**、
/// 下から 8 非空行目に在る。この pane を idle と読んで `/clear` が送られた（bd `s2-07l.94`）。
/// 本便では**負例**に使う: 字面が走行中に見えても打刻が Idle なら注入される（字面を読んでいない証拠）。
const RUNNING_PANE: &str = concat!(
    "✻ Sublimating… (19m 36s · ↓ 36.4k tokens · thought for 59s)\n",
    "  ⎿  Tip: Use /btw to ask a quick side question without interrupting Claude's current work\n",
    "\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// 同上（19%）。
const CTX_19: &str = " context=19";

/// **負例（字面を読んでいない証拠）**: pane が走行中の spinner の字面だけ（[`RUNNING_PANE`]）でも、
/// 打刻が Idle なら tick は pointer を注入する。字面で busy を読む実装はこの席へ 1 key も送らない
/// （`noop reason=busy`）。判定は `--capture-file` の pane で通し、送信だけ独立 socket の席へ通す。
#[test]
fn seat_state_tick_injects_despite_spinner_text_when_stamped_idle() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatstateidle";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    stamp_idle(&state, name);
    let pane = dir.join("pane.txt");
    fs::write(&pane, RUNNING_PANE).ok();
    let (wm_s, state_s, pane_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        pane.display().to_string(),
    );

    let out = run_seat(&[
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s, "--capture-file", &pane_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=inject target={name} consumed=false kind=pointer{CTX_19}{ST_IDLE} pointer=sent step=0{}\n", provenance(&state, "flag")),
        "spinner の字面は判定に効かない＝打刻 Idle の席には注入する（梯子は記録なしの初段＝`s2-07l.423`）"
    );
    let seen = capture(&socket, name);
    assert!(seen.contains(&format!("seat heartbeat --target {name}")), "既定の 1 行が届く: {seen}");
    let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
    assert!(
        recorded.contains(&format!(
            r#""what":"decision=inject target={name} consumed=false kind=pointer{CTX_19}{ST_IDLE} pointer=sent step=0{}""#,
            provenance(&state, "flag")
        )),
        "記録にも state の列（出所 = Stop）が載る: {recorded}"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// **逆の負例**: 入力欄が空で走行中の印も無い字面（[`IDLE_PANE`]）でも、打刻が Busy なら注入しない
/// （`noop reason=busy`・tmux 未接触）。退避物 0 件・lock 空きなので、状態の門だけがこの席を止めている。
#[test]
fn seat_state_tick_refuses_idle_looking_pane_when_stamped_busy() {
    let dir = tmp();
    let target = "seatstatebusy";
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    write_state(&seat_dir_of(&state, target), StateFix::Busy { age_s: 0 });
    fs::write(dir.join("pane.txt"), IDLE_PANE).ok();
    let (wm_s, state_s, pane_s, sock_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        dir.join("pane.txt").display().to_string(),
        dir.join("absent-sock").display().to_string(),
    );

    let (out, touched) = run_seat_probed(
        &dir,
        &[
            "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
            "--state-dir", &state_s, "--capture-file", &pane_s,
        ],
    );

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=busy{CTX_10}{ST_BUSY}{}\n", provenance(&state, "flag")),
        "入力欄が空の字面でも打刻が Busy なら busy"
    );
    assert!(!touched, "Busy の席には 1 key も送らない（tmux を撃たない）");
    fs::remove_dir_all(&dir).ok();
}

/// 打刻が無い・読めない・Busy が古い席は、退避物 0 件・lock 空き・入力欄が空でも注入しない
/// （fail-closed・理由が typed・tmux 未接触）。missing を idle に、stale を busy に読み替える実装は
/// ここで落ちる（同じ fixture で注入か `busy` になる）。
#[test]
fn seat_state_tick_fails_closed_without_readable_fresh_stamp() {
    let target = "seatstateclosed";
    let cases = [
        (StateFix::Absent, "state-missing", ST_MISSING),
        (StateFix::Unreadable, "state-unreadable", ST_UNREADABLE),
        (StateFix::Busy { age_s: STALE_S + 1 }, "state-stale", ST_STALE),
    ];
    for (at, (fix, reason, column)) in cases.into_iter().enumerate() {
        let dir = tmp();
        let state = dir.join("state");
        let wm = dir.join("wm");
        fs::create_dir_all(&wm).ok();
        write_state(&seat_dir_of(&state, target), fix);
        fs::write(dir.join("pane.txt"), IDLE_PANE).ok();
        let (wm_s, state_s, pane_s, sock_s) = (
            wm.display().to_string(),
            state.display().to_string(),
            dir.join("pane.txt").display().to_string(),
            dir.join("absent-sock").display().to_string(),
        );

        let (out, touched) = run_seat_probed(
            &dir,
            &[
                "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
                "--state-dir", &state_s, "--capture-file", &pane_s,
            ],
        );

        assert_eq!(rc_of(&out), i32::from(RC_OK), "組 {at}（{reason}）: stderr={}", stderr_of(&out));
        assert_eq!(
            stdout_of(&out),
            format!("seat: tick decision=noop reason={reason}{CTX_10}{column}{}\n", provenance(&state, "flag")),
            "組 {at}: 読めない打刻は理由を分けて注入しない"
        );
        assert!(!touched, "組 {at}（{reason}）: 1 key も送らない（tmux を撃たない）");
        let recorded = fs::read_to_string(tick_file(&state, target)).unwrap_or_default();
        assert!(
            recorded.lines().last().is_some_and(|line| line.contains(&format!(r#""what":"decision=noop reason={reason}{CTX_10}{column} "#))),
            "組 {at}: 記録の末尾 1 行も同じ理由と state の列: {recorded}"
        );
        fs::remove_dir_all(&dir).ok();
    }
}

/// 閾値の内側に置く Busy の打刻の余白（秒）。fixture の打刻から tick の now までに負荷下で数秒経っても
/// stale 側へ越えない幅（`s2-07l.237`）。
const STALE_MARGIN_S: u64 = 3;

/// Busy の打刻 `age` を 1 周撃ち、判定行と記録が `reason` / `column` になることを確かめる。
fn assert_stale_side(target: &'static str, age: u64, reason: &'static str, column: &'static str) {
    let case = TickCase { reason, beat_age_s: None, pane: Some(IDLE_PANE),
                          wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                          stamp: StateFix::Busy { age_s: age }, state: column };
    let dir = tmp();
    let state = prepare_tick_case(&dir, &case, target);
    let (out, touched) = run_tick_case(&dir, &case, target, &state);
    assert_tick_case(&out, touched, &case, usize::try_from(age).unwrap_or_default(), &state);
    fs::remove_dir_all(&dir).ok();
}

/// stale の境界は `seat.tick_stale_s`（= 2400）を**共用**する（新しい閾値を足さない・ADR-0015 §2.4）:
/// Busy の打刻が閾値の少し手前の周は `busy`、1 秒超で `state-stale`。両側から撃つ＝manifest の値が
/// 変わると落ちる。壁時計では等号を pin しない（`age == STALE_S` を busy と置くと、打刻から now までの
/// 1 秒で stale へ反転した・`s2-07l.237`・.118 と同型）＝閾値の内側は境界から離して置く。
// flip-check: retroactive s2-07l.237
#[test]
fn seat_state_tick_stale_boundary_shares_tick_stale_s() {
    let target = "seatstatestale";
    for (age, reason, column) in
        [(STALE_S - STALE_MARGIN_S, "busy", ST_BUSY), (STALE_S + 1, "state-stale", ST_STALE)]
    {
        assert_stale_side(target, age, reason, column);
    }
}

/// 余白の 2 面: 閾値の 3 秒手前の Busy は `busy` のまま（負荷下で時間が経っても stale へ越えない）、
/// 閾値 + 1 の Busy は `state-stale`（時間が経っても stale のまま）。どちらの面も壁時計の遅れで
/// 反転しない側に置く（`s2-07l.237`）。
// flip-check: retroactive s2-07l.237
#[test]
fn seat_state_tick_stale_margin_keeps_busy_below_threshold() {
    assert_eq!(STALE_MARGIN_S, 3, "busy 側の余白は 3 秒");
    assert_stale_side("seatstatemarginbusy", STALE_S - STALE_MARGIN_S, "busy", ST_BUSY);
    assert_stale_side("seatstatemarginstale", STALE_S + 1, "state-stale", ST_STALE);
}

/// tick が読むのは**最終行**: Busy の後に Idle が在れば idle、Idle の後に Busy なら busy。末尾の
/// 空行は最終行に数えない。先頭行だけ・全行の多数決で読む実装はここで落ちる。
#[test]
fn seat_state_tick_reads_the_last_stamp_line() {
    let target = "seatstatelast";
    let now = unix_now();
    let cases = [
        (
            format!(
                "{}\n{}\n\n",
                stamp_line("busy", "UserPromptSubmit", now, "s"),
                stamp_line("idle", "Stop", now, "s")
            ),
            "cycle-live",
            ST_IDLE,
        ),
        (
            format!(
                "{}\n{}\n",
                stamp_line("idle", "SessionStart", now, "s"),
                stamp_line("busy", "UserPromptSubmit", now, "s")
            ),
            "busy",
            ST_BUSY,
        ),
    ];
    for (at, (body, reason, column)) in cases.iter().enumerate() {
        // Idle の組は lock（TTL 内）で止める＝状態の門を**通った**ことが理由の字面で分かる。
        let case = TickCase { reason, beat_age_s: None, pane: Some(IDLE_PANE),
                              wm_seat: Some("other:seat"), via_file: true, tmux: false, context: CTX_10,
                              stamp: StateFix::Absent, state: column };
        let dir = tmp();
        let state = prepare_tick_case(&dir, &case, target);
        fs::write(state_file(&seat_dir_of(&state, target)), body).ok();
        let (out, touched) = run_tick_case(&dir, &case, target, &state);
        assert_tick_case(&out, touched, &case, at, &state);
        fs::remove_dir_all(&dir).ok();
    }
}

/// cycle は tick と同じ読み口で状態の門を通す: Busy・打刻なし・読めない・Busy が古い席へは
/// 退避物が在っても **1 key も送らない**（fail-closed・理由が typed・tmux 未接触）。
#[test]
fn seat_state_cycle_refuses_unless_stamped_idle() {
    let target = "seatstategate";
    for case in &[
        // 壁時計では等号を pin しない（`s2-07l.118`）: 閾値の内側は境界から離す。
        // flip-check: retroactive s2-07l.118
        GateCase { reason: "busy", wm_seat: Some("seatstategate"), pane: Some(IDLE_PANE), broken_state: false,
                   stamp: StateFix::Busy { age_s: STALE_S / 2 } },
        GateCase { reason: "state-missing", wm_seat: Some("seatstategate"), pane: Some(IDLE_PANE), broken_state: false,
                   stamp: StateFix::Absent },
        GateCase { reason: "state-unreadable", wm_seat: Some("seatstategate"), pane: Some(IDLE_PANE), broken_state: false,
                   stamp: StateFix::Unreadable },
        GateCase { reason: "state-stale", wm_seat: Some("seatstategate"), pane: Some(IDLE_PANE), broken_state: false,
                   stamp: StateFix::Busy { age_s: STALE_S + 1 } },
    ] {
        let dir = tmp();
        let state = prepare_gate_case(&dir, case, target);
        assert_gate_case(&dir, case, target, &state);
        fs::remove_dir_all(&dir).ok();
    }
}

/// **cycle 側の負例**: 送る前の pane が走行中の spinner の字面（[`RUNNING_PANE`]）でも、打刻が Idle で
/// 入力欄が空なら `/clear` を送り、作り直しを確認して復元する（字面を読む実装は `busy` で断る）。
#[test]
fn seat_state_cycle_sends_clear_despite_spinner_text_when_stamped_idle() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatstatecycle";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Now),
    );
    assert!(guard.ready(), "偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.spin.md", name);

    let out = cycle_with_pane_after_clear(&dir, name, RUNNING_PANE, REBUILT_PANE, FAST_SETTLE_S);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("seat: cycle done target={name}{}\n", provenance(&dir.join("state"), "flag")));
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "spinner の字面は判定に効かない＝打刻 Idle の席は作り直して復元する"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 退避の合図（`kind=externalize`・SRS FR29「idle を待たずに」）は**状態の門の外**: 打刻が Busy でも、
/// 無くても、読めなくても、Busy が古くても、context が cap 以上で退避物 0・lock 空きなら送る
/// （planner 裁定 2026-09-11: FR29 > ADR-0015 §2.3）。判定行の state の列は打刻のまま載る。状態の門を
/// context の前へ動かす実装は Busy 以外の 3 形でここで落ちる（lens-95 MEDIUM-2）。
#[test]
fn seat_state_tick_sends_externalize_pointer_regardless_of_stamp() {
    // `consumed=` は打刻由来（`s2-07l.112`）: file が在れば新しい打刻が来ないので false、無い・読めない
    // 席は測れないので unknown で、tick 行も `consumed=` の直後に理由を名乗る（`seat inject` の行と同じ
    // 並び・`s2-07l.150`）。
    let cases = [
        (StateFix::Busy { age_s: 0 }, ST_BUSY, "false"),
        (StateFix::Absent, ST_MISSING, "unknown reason=state-missing"),
        (StateFix::Unreadable, ST_UNREADABLE, "unknown reason=state-unreadable"),
        (StateFix::Busy { age_s: STALE_S + 1 }, ST_STALE, "false"),
    ];
    for (at, (fix, column, consumed)) in cases.into_iter().enumerate() {
        let dir = tmp();
        let socket = socket_of(&dir);
        let name = "seatstatecap";
        let guard = start_seat(&socket, name);
        assert!(guard.ready(), "組 {at}: 独立 socket に prompt 付きの session を立てられる");
        let state = dir.join("state");
        let wm = dir.join("wm");
        fs::create_dir_all(&wm).ok();
        write_state(&seat_dir_of(&state, name), fix);
        let pane = dir.join("pane.txt");
        fs::write(&pane, busy_pane_at(96)).ok();
        let (wm_s, state_s, pane_s) = (
            wm.display().to_string(),
            state.display().to_string(),
            pane.display().to_string(),
        );

        let out = run_seat(&[
            "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
            "--state-dir", &state_s, "--capture-file", &pane_s,
        ]);

        assert_eq!(rc_of(&out), i32::from(RC_OK), "組 {at}: stderr={}", stderr_of(&out));
        assert_eq!(
            stdout_of(&out),
            format!("seat: tick decision=inject target={name} consumed={consumed} kind=externalize origin=context context=96{column}{}\n", provenance(&state, "flag")),
            "組 {at}: 退避の合図は打刻に依らず送る（state の列は打刻のまま・consumed は打刻由来）"
        );
        assert!(capture(&socket, name).contains("/ready-compaction"), "組 {at}: 退避 skill の名が届く");
        // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
        drop(guard);
        fs::remove_dir_all(&dir).ok();
    }
}

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
        "seat register: registered role=orchestrator target=rs:planner sid=sid-a account=acct-1 anchor=/repo/anchor\n"
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
    assert!(usage.contains("|register --state-dir S --target T --role R --account L --launch FILE [--anchor DIR]"), "{usage}");
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
    let tail = |out: &Output| stdout_of(out).lines().rev().take(4).map(str::to_owned).collect::<Vec<String>>();
    let consumer_gone = CONSUMER_REPO.replace("consumer=/repo ", "consumer=/repo/gone ");
    assert_eq!(
        tail(&out),
        [
            consumer_gone.as_str(),
            CONSUMER_REPO,
            HOST_ABSENT,
            "seats: registered=2 live=unmeasurable missing=unmeasurable"
        ],
        "突合の行の直後に host の面の行・末尾に anchor ごとの導入先の行"
    );
    let seat = start_seat(&place.socket, "rolesdoc");
    assert!(seat.ready(), "隔離 seat が立つ");
    let out = role_doctor(&place);
    let lines: Vec<String> = stdout_of(&out).lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 8, "2 行 + 登録 row 2 行 + 突合 1 行 + host の面 1 行 + 導入先 2 行: {lines:?}");
    assert_eq!(
        tail(&out),
        [consumer_gone.as_str(), CONSUMER_REPO, HOST_ABSENT, "seats: registered=2 live=1 missing=1"]
    );
    let doctor = |args: &[&str]| Command::new(bin()).arg("doctor").args(args).output().ok();
    let bare = doctor(&[]).map(|out| stdout_of(&out)).unwrap_or_default();
    assert_eq!(bare.lines().count(), 2, "引数無しは従来の 2 行: {bare}");
    let state = place.state.display().to_string();
    for bad in [&["--state-dir"][..], &["--state-dir", ""], &["--tmux-socket", "s"], &["--state-dir", &state, "--bogus", "x"], &["--state-dir", &state, "--state-dir", &state]] {
        let out = doctor(bad);
        assert_eq!(out.as_ref().map(rc_of), Some(i32::from(RC_REFUSED)), "{bad:?} は使い方で断る");
        assert!(out.is_some_and(|found| stdout_of(&found).starts_with("usage: ")), "{bad:?}");
    }
    drop(seat);
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────────────── register --model（契約 (e)・s2-07l.215） ───────────────────────────

/// target の登録 row の `model`（replay の読み手 `registration_of_target` から運ぶ）。
fn model_of_target(place: &RolePlace, target: &str) -> Option<String> {
    vessel::seat::role::registration_of_target(&role_state(place), target).and_then(|row| row.model.clone())
}

/// `--model Fable` で登録 → `SeatRegistered` 1 件の束に model = Fable・行の key に `"model":"Fable"`・
/// replay の読み手が `Some("Fable")` を返す・出力行の末尾に `model=Fable`（歯 (e)(1)・flip の RED）。
#[test]
fn seat_register_model_lands_in_the_row_and_the_reader_returns_it() {
    let place = role_place();
    role_stamp(&place, "rm:planner", Some("sid-m"));
    let out = role_register(&place, "rm:planner", "orchestrator", &["--anchor", "/repo/anchor", "--model", "Fable"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat register: registered role=orchestrator target=rm:planner sid=sid-m account=acct-1 anchor=/repo/anchor model=Fable\n"
    );
    let events = vessel::fleet::store::read_all(&place.state).unwrap_or_default();
    assert_eq!(events.len(), 1, "1 件だけ: {}", role_log(&place));
    let registration = events.first().and_then(|event| event.registration.clone()).unwrap_or_else(|| panic!("本体が在る"));
    assert_eq!(registration.model.as_deref(), Some("Fable"));
    assert_eq!(registration.role, vessel::seat::role::Role::Orchestrator, "他の項目はそのまま");
    assert_eq!(registration.account, "acct-1");
    let log = role_log(&place);
    assert!(log.contains("\"model\":\"Fable\""), "行の key: {log}");
    assert!(log.contains("\"schema\":1"), "schema 1 のまま: {log}");
    let state = role_state(&place);
    let row = vessel::seat::role::registration_of_target(&state, "rm:planner").unwrap_or_else(|| panic!("row が在る"));
    assert_eq!(row.model.as_deref(), Some("Fable"), "読み手が model を運ぶ");
    assert_eq!(row.role, vessel::seat::role::Role::Orchestrator);
    assert_eq!(vessel::seat::role::role_of_target(&state, "rm:planner"), Some(vessel::seat::role::Role::Orchestrator), "役割の解決は同じ 1 本");
    assert_eq!(vessel::seat::role::registration_of_target(&state, "rm:absent"), None, "登録の無い target は None");
    fs::remove_dir_all(&place.dir).ok();
}

/// `--model` 無しで登録 → `None`・行に `model` の key が無い（旧 row と同じ形）・出力行は従来のまま（歯 (e)(2)）。
#[test]
fn seat_register_model_absent_reads_as_none_and_keeps_the_old_row_form() {
    let place = role_place();
    role_stamp(&place, "rm:plain", Some("sid-p"));
    let out = role_register(&place, "rm:plain", "orchestrator", &["--anchor", "/repo/anchor"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), "seat register: registered role=orchestrator target=rm:plain sid=sid-p account=acct-1 anchor=/repo/anchor\n");
    let log = role_log(&place);
    assert_eq!(log.lines().count(), 1, "1 件: {log}");
    assert!(!log.contains("\"model\""), "None は key ごと書かない: {log}");
    assert_eq!(model_of_target(&place, "rm:plain"), None);
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

/// 同じ鍵で `--model` を変えて再登録 → 最新が効く（付ける → 変える → 外す・前の row は残る）（歯 (e)(4)）。
#[test]
fn seat_register_model_reregistration_replaces_the_model_with_the_latest() {
    let place = role_place();
    role_stamp(&place, "rm:again", Some("sid-g"));
    let register = |extra: &[&str]| {
        let out = role_register(&place, "rm:again", "orchestrator", extra);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    };
    register(&["--anchor", "/repo/main", "--model", "Opus"]);
    assert_eq!(model_of_target(&place, "rm:again").as_deref(), Some("Opus"));
    register(&["--anchor", "/repo/main", "--model", "Fable"]);
    assert_eq!(model_of_target(&place, "rm:again").as_deref(), Some("Fable"), "最新が効く");
    register(&["--anchor", "/repo/main"]);
    assert_eq!(model_of_target(&place, "rm:again"), None, "外した再登録は None に戻る（前の値を引き継がない）");
    assert_eq!(role_log(&place).lines().count(), 3, "前の row は残る（append のみ）");
    assert_eq!(role_state(&place).registrations.len(), 1, "同じ鍵は 1 つに畳む");
    fs::remove_dir_all(&place.dir).ok();
}

/// doctor の登録 row の一覧に `model` の欄（None は `-`・鍵の順・突合の行の前）（歯 (e)(5)）。
#[test]
fn seat_register_model_shows_in_the_doctor_rows_with_dash_for_none() {
    let place = role_place();
    for (target, role, extra) in [("rm:doc-a", "orchestrator", &["--anchor", "/repo/a", "--model", "Fable"][..]), ("rm:doc-b", "orchestrator", &["--anchor", "/repo/b"])] {
        role_stamp(&place, target, Some("sid-d"));
        let out = role_register(&place, target, role, extra);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    }
    let out = role_doctor(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let lines: Vec<String> = stdout_of(&out).lines().map(str::to_owned).collect();
    assert_eq!(
        lines.get(2..),
        Some(&[
            "seat: role=orchestrator anchor=/repo/a target=rm:doc-a account=acct-1 model=Fable".to_owned(),
            "seat: role=orchestrator anchor=/repo/b target=rm:doc-b account=acct-1 model=-".to_owned(),
            "seats: registered=2 live=unmeasurable missing=unmeasurable".to_owned(),
            HOST_ABSENT.to_owned(),
            consumer_line_of("/repo/a"),
            consumer_line_of("/repo/b"),
        ][..]),
        "{lines:?}"
    );
    let rows = vessel::seat::role::render_rows(&role_state(&place));
    assert_eq!(rows, lines.get(2..4).unwrap_or_default(), "pure の一覧と同じ");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────────────── doctor の口座の行（s2-07l.233・account-autonomy.md §5） ───────────────────────────

/// 登録 row の anchor `anchor` の導入先の行（[`CONSUMER_REPO`] と同じ形）。
fn consumer_line_of(anchor: &str) -> String {
    CONSUMER_REPO.replacen("/repo", anchor, 1)
}
