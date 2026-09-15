//! 席の状態・役割・登録・口座・rules の歯（打刻の門・`seat register` / `doctor` / 口座の退避と立て直し /
//! `seat launch` / 壊れた `--rules`・設計 docs/design/seat-roles.md / account-lifecycle.md §8・接頭辞
//! `seat_state_` / `seat_attrib_` / `seat_role_` / `seat_register_` / `doctor_accounts_` / `rules_host_` /
//! `seat_rules_` / `seat_account_` / `seat_launch_`）。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。
//! 歯の本文は `seat.rs` から**挙動不変で移した**もの（`s2-07l.261`）。
// flip-check: moved s2-07l.261

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
        format!("seat: tick decision=inject target={name} consumed=false kind=pointer{CTX_19}{ST_IDLE}{}\n", provenance(&state, "flag")),
        "spinner の字面は判定に効かない＝打刻 Idle の席には注入する"
    );
    let seen = capture(&socket, name);
    assert!(seen.contains(&format!("seat heartbeat --target {name}")), "既定の 1 行が届く: {seen}");
    let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
    assert!(
        recorded.contains(&format!(
            r#""what":"decision=inject target={name} consumed=false kind=pointer{CTX_19}{ST_IDLE}{}""#,
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

// ─────────────────── Enter 落ちの修復と記録の席の列（`s2-07l.150`・接頭辞 `seat_attrib_`） ───────────────────

/// Enter 落ちの歯が注入する 1 行（ASCII＝pane の幅で折り返さない・目印は最初の非空行＝この全文）。
const ATTRIB_POINTER: &str = "attrib: seat-attrib-draft";

/// **Enter を submit にしない**席（prompt を 1 つ出して入力を読み捨てる）を独立 socket に立てる。
///
/// tty の echo は残るので、送った字面は最後の prompt 行の右に残り、Enter は改行を描くだけで新しい
/// prompt も打刻も出さない＝実機の「Enter だけが落ちた周」と同じ形（壁時計の閾値は使わない）。
fn start_enter_lost_seat(socket: &str, name: &str) -> IsolatedSeat {
    let mut seat = IsolatedSeat {
        socket: socket.to_owned(),
        name: name.to_owned(),
        ready: false,
    };
    let out = tmux(
        socket,
        &["new-session", "-d", "-s", name, "-x", "120", "-y", "40", "sh", "-c", "printf '\u{276f} '; exec cat >/dev/null"],
    );
    seat.ready = out.status.success() && wait_prompt(socket, name);
    seat
}

/// 修復の歯の置き場を組む（退避物 0 件・打刻は `fix`・判定に使う pane は `pane`）。
fn attrib_setup(dir: &Path, name: &str, fix: StateFix, pane: &str) {
    fs::create_dir_all(dir.join("wm")).ok();
    write_state(&seat_dir_of(&dir.join("state"), name), fix);
    fs::write(dir.join("pane.txt"), pane).ok();
}

/// 修復の歯の tick を 1 回撃つ（判定は `--capture-file`・送信は独立 socket の席）。
fn attrib_tick(dir: &Path, name: &str, socket: &str) -> Output {
    let (wm, state, pane) = (
        dir.join("wm").display().to_string(),
        dir.join("state").display().to_string(),
        dir.join("pane.txt").display().to_string(),
    );
    run_seat(&[
        "tick", "--target", name, "--wm-dir", &wm, "--tmux-socket", socket, "--state-dir", &state,
        "--capture-file", &pane, "--pointer", ATTRIB_POINTER,
    ])
}

/// tick-stamp の在処（契約の字面から組む）。
fn tick_stamp_of(dir: &Path, name: &str) -> PathBuf {
    seat_dir_of(&dir.join("state"), name).join("tick-stamp")
}

/// (1) idle ∧ 送った目印が入力欄に残る周: Enter だけを送り直し、それでも消費の打刻が来ないので
/// `consumed=false reason=enter-lost`・**tick-stamp を打たない**・目印は 1 つのまま（text を再送しない）。
/// base は Enter 落ちを queue と同じ `consumed=false` で流し、自打刻する（RED）。
#[test]
fn seat_attrib_tick_resends_enter_once_and_reports_enter_lost() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatattriblost";
    let guard = start_enter_lost_seat(&socket, name);
    assert!(guard.ready(), "Enter を submit にしない席を立てられる");
    attrib_setup(&dir, name, StateFix::Idle, IDLE_PANE);
    let state = dir.join("state");

    let out = attrib_tick(&dir, name, &socket);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!(
            "seat: tick decision=inject target={name} consumed=false reason=enter-lost kind=pointer{CTX_10}{ST_IDLE}{}\n",
            provenance(&state, "flag")
        ),
        "送り直しても消費されない周は enter-lost を名乗る"
    );
    assert!(!tick_stamp_of(&dir, name).exists(), "修復が閉じない周は tick-stamp を打たない");
    let pane = capture(&socket, name);
    assert_eq!(pane.matches(ATTRIB_POINTER).count(), 1, "text は再送しない（目印は 1 つのまま）: {pane}");
    let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
    assert!(
        recorded.lines().last().is_some_and(|line| line.contains("consumed=false reason=enter-lost kind=pointer")),
        "記録の判定行も同じ理由: {recorded}"
    );
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// (2) 同じ席で tick を 2 回撃つと、2 周目は入力欄の門が断る（`decision=error reason=inject-busy`・rc 1）。
/// **`pointer-recent` ではない**＝1 周目が stamp を打っていないことがここで測れる（base は 1 周目で自打刻
/// するので 2 周目が `decision=noop reason=pointer-recent`・RED）。
#[test]
fn seat_attrib_tick_next_round_names_the_busy_input_not_pointer_recent() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatattribnext";
    let guard = start_enter_lost_seat(&socket, name);
    assert!(guard.ready(), "Enter を submit にしない席を立てられる");
    attrib_setup(&dir, name, StateFix::Idle, IDLE_PANE);
    let state = dir.join("state");

    let first = attrib_tick(&dir, name, &socket);
    assert_eq!(rc_of(&first), i32::from(RC_OK), "stderr={}", stderr_of(&first));
    let second = attrib_tick(&dir, name, &socket);

    assert_eq!(rc_of(&second), i32::from(RC_REFUSED), "stdout={}", stdout_of(&second));
    assert_eq!(stdout_of(&second), "", "error の周は stdout 0 byte");
    assert_eq!(
        stderr_of(&second),
        format!("seat: tick decision=error reason=inject-busy{CTX_10}{ST_IDLE}{}\n", provenance(&state, "flag")),
        "入力欄に目印が残る席は入力欄の門が名乗る（黙った pointer-recent にならない）"
    );
    assert!(!tick_stamp_of(&dir, name).exists(), "2 周とも tick-stamp を打たない");
    assert_eq!(capture(&socket, name).matches(ATTRIB_POINTER).count(), 1, "2 周目は 1 key も送らない");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// (3) idle ∧ 入力欄が空（`sh -i` は受けた行を実行して prompt を描き直す）: 修復は発火せず、従来どおり
/// `consumed=false`（理由なし）で自打刻する。
#[test]
fn seat_attrib_tick_empty_input_keeps_queued_and_stamps() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatattribempty";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    attrib_setup(&dir, name, StateFix::Idle, IDLE_PANE);

    let out = attrib_tick(&dir, name, &socket);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=inject target={name} consumed=false kind=pointer{CTX_10}{ST_IDLE}{}\n", provenance(&dir.join("state"), "flag")),
        "入力欄が空の周は queue のまま（reason を足さない）"
    );
    assert!(tick_stamp_of(&dir, name).exists(), "修復しない周は従来どおり自打刻する");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// (4) 打刻が Busy の席への退避の合図: 目印が入力欄に残る席でも**送り直さない**（idle でない周は
/// turn の終わりに消費される queue）。従来どおり `consumed=false`（理由なし）で自打刻する。
#[test]
fn seat_attrib_tick_busy_seat_is_not_nudged() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatattribbusy";
    let guard = start_enter_lost_seat(&socket, name);
    assert!(guard.ready(), "Enter を submit にしない席を立てられる");
    attrib_setup(&dir, name, StateFix::Busy { age_s: 0 }, &busy_pane_at(96));

    let out = attrib_tick(&dir, name, &socket);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=inject target={name} consumed=false kind=externalize origin=context context=96{ST_BUSY}{}\n", provenance(&dir.join("state"), "flag")),
        "Busy の席は修復の門を通らない"
    );
    assert!(tick_stamp_of(&dir, name).exists(), "修復しない周は従来どおり自打刻する");
    assert_eq!(capture(&socket, name).matches("/ready-compaction").count(), 1, "退避の合図は 1 回だけ届く");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// (5) 打刻 file が無い・読めない席: tick 行も `consumed=unknown` の直後に理由を名乗る（C10: 測れて
/// いない周を `false` と同じ顔で流さない・`seat inject` の行と同じ並び）。記録の判定行も同じ字面。
#[test]
fn seat_attrib_tick_names_the_unmeasured_reason() {
    for (fix, column, reason) in [
        (StateFix::Absent, ST_MISSING, "state-missing"),
        (StateFix::Unreadable, ST_UNREADABLE, "state-unreadable"),
    ] {
        let dir = tmp();
        let socket = socket_of(&dir);
        let name = "seatattribunknown";
        let guard = start_seat(&socket, name);
        assert!(guard.ready(), "{reason}: 独立 socket に prompt 付きの session を立てられる");
        attrib_setup(&dir, name, fix, &busy_pane_at(96));
        let state = dir.join("state");

        let out = attrib_tick(&dir, name, &socket);

        assert_eq!(rc_of(&out), i32::from(RC_OK), "{reason}: stderr={}", stderr_of(&out));
        let body = format!("decision=inject target={name} consumed=unknown reason={reason} kind=externalize origin=context context=96{column}");
        assert_eq!(stdout_of(&out), format!("seat: tick {body}{}\n", provenance(&state, "flag")), "{reason}");
        let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
        assert!(
            recorded.lines().last().is_some_and(|line| line.contains(&format!(r#""what":"{body} "#))),
            "{reason}: 記録の判定行も同じ字面: {recorded}"
        );
        drop(guard);
        fs::remove_dir_all(&dir).ok();
    }
}

/// 記録の書き手 3 面（tick / inject / cycle）が**同じ `tick.jsonl`** に書く行は、どれも潰した席の名
/// （`seat`）と時刻（`ts`・1970 年からの秒）を持ち、`schema` 1 と `tokens` null は不変。退避して止まって
/// いる席へ tick を撃ち、cycle（`/clear` → 復元の inject）まで 1 周で回して 3 種の `who` を弁別して数える。
/// `ts` は作り直しの打刻（`SessionStart`）の ts **以上**（3 行とも作り直しを確認した後に書く・等号は pin しない）。
#[test]
fn seat_attrib_record_tick_inject_and_cycle_lines_name_the_seat() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatattribrec";
    let target = format!("{name}:0");
    let seat = format!("{name}_0");
    let state = dir.join("state");
    let stamps = state_file(&seat_dir_of(&state, &seat));
    stamp_idle(&state, &seat);
    let guard = start_clearing_seat(&socket, name, &dir.join("seat.log"), &stamps, (FakeStamp::Now, FakeStamp::Now));
    assert!(guard.ready(), "偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.rec.md", &target);
    fs::write(dir.join("pane.txt"), IDLE_PANE).ok();
    let rules = fast_rules(&dir);
    let (wm_s, state_s, pane_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        dir.join("pane.txt").display().to_string(),
    );

    let out = run_seat(&[
        "tick", "--target", &target, "--wm-dir", &wm_s, "--tmux-socket", &socket, "--state-dir", &state_s,
        "--capture-file", &pane_s, "--rules", &rules,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert!(stdout_of(&out).contains(" cycle=done "), "cycle まで 1 周で回る: {}", stdout_of(&out));
    let stamped = fs::read_to_string(&stamps).unwrap_or_default();
    let rebuilt = stamped
        .lines()
        .filter(|line| json_value(line, "event") == Some(vessel::fleet::json_lite::Value::Str("SessionStart".to_owned())))
        .find_map(|line| json_value(line, "ts").and_then(|value| value.as_num()));
    assert!(rebuilt.is_some(), "作り直しの打刻が在る: {stamped}");
    let recorded = fs::read_to_string(tick_file(&state, &seat)).unwrap_or_default();
    let lines: Vec<&str> = recorded.lines().collect();
    for who in ["seat-inject", "seat-cycle", "seat-tick"] {
        let count = lines
            .iter()
            .filter(|line| json_value(line, "who") == Some(vessel::fleet::json_lite::Value::Str(who.to_owned())))
            .count();
        assert_eq!(count, 1, "{who} の行はちょうど 1 つ（母集団 {} 行）: {recorded}", lines.len());
    }
    assert_eq!(lines.len(), 3, "3 面が 1 行ずつ: {recorded}");
    for line in &lines {
        assert_eq!(json_value(line, "seat"), Some(vessel::fleet::json_lite::Value::Str(seat.clone())), "席の名（潰した target）: {line}");
        let ts = json_value(line, "ts").and_then(|value| value.as_num());
        assert!(ts.is_some_and(|found| rebuilt.is_some_and(|at| found >= at)), "ts は作り直しの打刻以上: {line} / {rebuilt:?}");
        assert_eq!(json_value(line, "schema"), Some(vessel::fleet::json_lite::Value::Num(1)), "schema は 1 のまま: {line}");
        assert_eq!(json_value(line, "tokens"), Some(vessel::fleet::json_lite::Value::Null), "tokens は null のまま: {line}");
    }
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

// ─────────────────────────── role / register ───────────────────────────

/// 登録の置き場の fixture（state dir・雛形 file・tmux socket）。
pub(super) struct RolePlace {
    /// tmp dir の root。
    pub(super) dir: PathBuf,
    /// event log の置き場。
    pub(super) state: PathBuf,
    /// 起動の雛形の file。
    launch: String,
    /// 独立 socket（server は立てない周もある）。
    pub(super) socket: String,
}

/// 雛形の本文（event の `launch` にそのまま載る）。
const LAUNCH_BODY: &str = "launch {credential-dir}\n\"quoted\" line\n";

/// 登録の置き場を 1 つ作る。
pub(super) fn role_place() -> RolePlace {
    let dir = tmp();
    let state = dir.join("state");
    let launch = fixture(&dir, "launch.txt", LAUNCH_BODY);
    let socket = socket_of(&dir);
    RolePlace { dir, state, launch, socket }
}

/// target の打刻を置く（dir 名は契約の字面どおり `:` を `_` に潰す）。`sid` が `None` なら dir だけ作る。
pub(super) fn role_stamp(place: &RolePlace, target: &str, sid: Option<&str>) {
    let seat = place.state.join("seat").join(target.replace(':', "_"));
    fs::create_dir_all(&seat).ok();
    if let Some(sid) = sid {
        fs::write(state_file(&seat), format!("{}\n", stamp_line("idle", "SessionStart", unix_now(), sid))).ok();
    }
}

/// `seat register` を 1 回撃つ（`extra` は `--anchor` などの追加 flag）。
pub(super) fn role_register(place: &RolePlace, target: &str, role: &str, extra: &[&str]) -> Output {
    let state = place.state.display().to_string();
    let mut args = vec![
        "register", "--state-dir", &state, "--target", target, "--role", role, "--account", "acct-1",
        "--launch", &place.launch,
    ];
    args.extend_from_slice(extra);
    run_seat(&args)
}

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
    let out = role_register(&place, "rs:planner", "planner", &["--anchor", "/repo/anchor"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat register: registered role=planner target=rs:planner sid=sid-a account=acct-1 anchor=/repo/anchor\n"
    );
    let events = vessel::fleet::store::read_all(&place.state).unwrap_or_default();
    assert_eq!(events.len(), 1, "1 件だけ: {}", role_log(&place));
    let event = events.first().cloned().unwrap_or_else(|| panic!("行が在る"));
    assert_eq!(event.kind, vessel::fleet::EventKind::SeatRegistered);
    assert!(event.run.is_empty() && event.bead.is_empty(), "便に紐づかない");
    let registration = event.registration.unwrap_or_else(|| panic!("本体が在る"));
    assert_eq!(registration.role, vessel::seat::role::Role::Planner);
    assert_eq!(registration.anchor, "/repo/anchor");
    assert_eq!(registration.target, "rs:planner");
    assert_eq!(registration.sid.as_deref(), Some("sid-a"), "register の口の row は打刻の sid を持つ");
    assert_eq!(registration.account, "acct-1");
    assert_eq!(registration.launch, LAUNCH_BODY, "雛形の本文がそのまま載る");
    let state = role_state(&place);
    assert_eq!(state.runs.len(), 0, "幽霊の便を作らない");
    assert_eq!(vessel::seat::role::role_of_target(&state, "rs:planner"), Some(vessel::seat::role::Role::Planner));
    fs::remove_dir_all(&place.dir).ok();
}

/// 同じ鍵の再登録は前の row を残したまま最新だけが解ける・別 target へ移すと旧 target では解けない・
/// 別の anchor に同じ target を登録すると replay の最新が解ける（歯 (a)(2)(8)）。
#[test]
fn seat_role_reregister_keeps_old_rows_and_resolves_the_latest() {
    use vessel::seat::role::{role_of_target, Role};
    let place = role_place();
    for target in ["rr:one", "rr:two"] {
        role_stamp(&place, target, Some("sid-r"));
    }
    let register = |target: &str, role: &str, anchor: &str| {
        let out = role_register(&place, target, role, &["--anchor", anchor]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    };
    register("rr:one", "admin", "/repo/main");
    register("rr:one", "admin", "/repo/main");
    assert_eq!(role_log(&place).lines().count(), 2, "前の row は残る（append のみ）");
    let state = role_state(&place);
    assert_eq!(state.registrations.len(), 1, "同じ鍵は 1 つに畳む");
    assert_eq!(state.registrations.values().map(|latest| latest.seq).collect::<Vec<_>>(), vec![1], "2 件目が解決される");
    assert_eq!(role_of_target(&state, "rr:one"), Some(Role::Admin));
    register("rr:one", "planner", "/repo/.worktrees/wt");
    assert_eq!(role_of_target(&role_state(&place), "rr:one"), Some(Role::Planner), "別の anchor の後の row が勝つ");
    register("rr:two", "planner", "/repo/.worktrees/wt");
    let moved = role_state(&place);
    assert_eq!(role_of_target(&moved, "rr:two"), Some(Role::Planner));
    assert_eq!(role_of_target(&moved, "rr:one"), Some(Role::Admin), "移した鍵の旧 target は解けず、別の鍵の row が残る");
    register("rr:two", "admin", "/repo/main");
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
        let out = role_register(&place, target, "planner", &["--anchor", "/repo"]);
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
        "register", "--state-dir", &state, "--target", "ra:anchor", "--role", "admin", "--account", "acct-1",
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
    let out = role_register(&place, &target, "planner", &["--anchor", "/repo"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let resolve = |pane: &str| target_of_pane(Some(&place.socket), pane).and_then(|found| role_of_target(&role_state(&place), &found));
    assert_eq!(resolve(&first), Some(Role::Planner), "env の値でなく row の役割");
    assert!(tmux(&place.socket, &["kill-window", "-t", "rolesess:win"]).status.success(), "window を畳める");
    let (second, why) = pane_of("win", "SCRIBE2_ROLE=admin");
    assert!(second.starts_with('%') && second != first, "pane id が差し替わる: {second} stderr={why}");
    assert_eq!(resolve(&second), Some(Role::Planner), "同じ target なら同じ役割");
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
    let out = run_seat(&["register", "--target", "ru:x", "--role", "planner"]);
    assert_eq!(stderr_of(&out), usage, "必須 flag の欠けも使い方");
    assert!(!vessel::fleet::store::events_path(&place.state).exists(), "行を書かない");
    fs::remove_dir_all(&place.dir).ok();
}

/// 役割は 2 つで宣言順・variant 名の字面は受けない・受付の極性は in-loop / fail-closed。
#[test]
fn seat_role_enum_is_closed_in_declaration_order() {
    use vessel::polarity::{OnFailure, Timing};
    use vessel::seat::role::{Role, ALL, POLARITY};
    assert_eq!(ALL.len(), 2, "記録時点の母集団");
    assert!(vessel::order::is_declaration_order(ALL, |role| role as usize), "ALL は宣言順: {ALL:?}");
    assert_eq!(Role::parse("Planner"), None, "variant 名は字面でない");
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
    for (target, role) in [("rc:gone", "planner"), ("rc:live", "planner"), ("rc:away", "admin")] {
        assert_eq!(rc_of(&role_register(&place, target, role, &["--anchor", "/repo"])), i32::from(RC_OK));
    }
    let state = role_state(&place);
    let live = vec!["rc:live".to_owned(), "rc:gone".to_owned()];
    assert_eq!(render_reconcile(Some(&state), Some(&live)), "seats: registered=2 live=1 missing=1", "同じ鍵の旧 row は数えない");
    assert_eq!(render_reconcile(Some(&state), None), "seats: registered=2 live=unmeasurable missing=unmeasurable");
    assert_eq!(render_reconcile(None, Some(&live)), "seats: registered=unreadable live=unmeasurable missing=unmeasurable");
    fs::remove_dir_all(&place.dir).ok();
}

/// `doctor --state-dir` を撃つ（manifest は `[[account]]` の無い `--rules`＝口座の行 0 本・突合までの外形）。
pub(super) fn role_doctor(place: &RolePlace) -> Output {
    role_doctor_rules(place, NO_ACCOUNT_RULES)
}

/// `doctor --state-dir --tmux-socket --rules` を撃つ（`body` の manifest を置き場の dir に書いて渡す）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn role_doctor_rules(place: &RolePlace, body: &str) -> Output {
    let state = place.state.display().to_string();
    let rules = fixture(&place.dir, "doctor-rules.toml", body);
    Command::new(bin())
        .args(["doctor", "--state-dir", &state, "--tmux-socket", &place.socket, "--rules", &rules])
        .output()
        .expect("binary を起動できる")
}

/// 登録 2 件（実在の target 1 件）を置いた置き場（tmux の server は呼び側が立てる）。
pub(super) fn role_doctor_place() -> RolePlace {
    let place = role_place();
    for (target, role) in [("rolesdoc:rolesdoc", "planner"), ("gone:gone", "admin")] {
        role_stamp(&place, target, Some("sid-doc"));
        let out = role_register(&place, target, role, &["--anchor", "/repo"]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    }
    place
}

/// doctor は登録 row と実在の target を突き合わせ、撃てない周は 0 と書かない（歯 (d)）。末尾は登録 row の anchor の
/// 導入先の行（consumer-sync.md §4・`s2-07l.303`）。
#[test]
fn seat_role_doctor_reconciles_rows_with_live_targets() {
    let place = role_doctor_place();
    let out = role_doctor(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let tail = |out: &Output| stdout_of(out).lines().rev().take(3).map(str::to_owned).collect::<Vec<String>>();
    assert_eq!(
        tail(&out),
        [CONSUMER_REPO, HOST_ABSENT, "seats: registered=2 live=unmeasurable missing=unmeasurable"],
        "突合の行の直後に host の面の行・末尾に導入先の行"
    );
    let seat = start_seat(&place.socket, "rolesdoc");
    assert!(seat.ready(), "隔離 seat が立つ");
    let out = role_doctor(&place);
    let lines: Vec<String> = stdout_of(&out).lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 7, "2 行 + 登録 row 2 行 + 突合 1 行 + host の面 1 行 + 導入先 1 行: {lines:?}");
    assert_eq!(tail(&out), [CONSUMER_REPO, HOST_ABSENT, "seats: registered=2 live=1 missing=1"]);
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
    let out = role_register(&place, "rm:planner", "planner", &["--anchor", "/repo/anchor", "--model", "Fable"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat register: registered role=planner target=rm:planner sid=sid-m account=acct-1 anchor=/repo/anchor model=Fable\n"
    );
    let events = vessel::fleet::store::read_all(&place.state).unwrap_or_default();
    assert_eq!(events.len(), 1, "1 件だけ: {}", role_log(&place));
    let registration = events.first().and_then(|event| event.registration.clone()).unwrap_or_else(|| panic!("本体が在る"));
    assert_eq!(registration.model.as_deref(), Some("Fable"));
    assert_eq!(registration.role, vessel::seat::role::Role::Planner, "他の項目はそのまま");
    assert_eq!(registration.account, "acct-1");
    let log = role_log(&place);
    assert!(log.contains("\"model\":\"Fable\""), "行の key: {log}");
    assert!(log.contains("\"schema\":1"), "schema 1 のまま: {log}");
    let state = role_state(&place);
    let row = vessel::seat::role::registration_of_target(&state, "rm:planner").unwrap_or_else(|| panic!("row が在る"));
    assert_eq!(row.model.as_deref(), Some("Fable"), "読み手が model を運ぶ");
    assert_eq!(row.role, vessel::seat::role::Role::Planner);
    assert_eq!(vessel::seat::role::role_of_target(&state, "rm:planner"), Some(vessel::seat::role::Role::Planner), "役割の解決は同じ 1 本");
    assert_eq!(vessel::seat::role::registration_of_target(&state, "rm:absent"), None, "登録の無い target は None");
    fs::remove_dir_all(&place.dir).ok();
}

/// `--model` 無しで登録 → `None`・行に `model` の key が無い（旧 row と同じ形）・出力行は従来のまま（歯 (e)(2)）。
#[test]
fn seat_register_model_absent_reads_as_none_and_keeps_the_old_row_form() {
    let place = role_place();
    role_stamp(&place, "rm:plain", Some("sid-p"));
    let out = role_register(&place, "rm:plain", "admin", &["--anchor", "/repo/anchor"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), "seat register: registered role=admin target=rm:plain sid=sid-p account=acct-1 anchor=/repo/anchor\n");
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
        let out = role_register(&place, "rm:empty", "planner", extra);
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
        let out = role_register(&place, "rm:again", "admin", extra);
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
    for (target, role, extra) in [("rm:doc-a", "planner", &["--anchor", "/repo/a", "--model", "Fable"][..]), ("rm:doc-b", "admin", &["--anchor", "/repo/b"])] {
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
            "seat: role=planner anchor=/repo/a target=rm:doc-a account=acct-1 model=Fable".to_owned(),
            "seat: role=admin anchor=/repo/b target=rm:doc-b account=acct-1 model=-".to_owned(),
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

/// `[[account]]` の無い manifest（口座の行 0 本）。
pub(super) const NO_ACCOUNT_RULES: &str = "schema = 1\n";

/// 置き場に `host.toml` の無い周の doctor の host の面の行（突合の行の直後・口座の行の直前）。
pub(super) const HOST_ABSENT: &str = "host-manifest=absent";

/// 登録 row の anchor `/repo` が導入先として出る 1 行（口座の行の後ろ・記録なし・帳簿なし・`[[vessel]]` なし・
/// consumer-sync.md §4・`s2-07l.303`）。
pub(super) const CONSUMER_REPO: &str =
    "consumer=/repo source=launch scope=- binary=unrecorded plugin=unrecorded ledger=- cache=absent head=undeclared drift=unrecorded";

/// 登録 row の anchor `anchor` の導入先の行（[`CONSUMER_REPO`] と同じ形）。
fn consumer_line_of(anchor: &str) -> String {
    CONSUMER_REPO.replacen("/repo", anchor, 1)
}

/// `[[account]]` を `labels` の順に宣言した manifest の本文。
fn account_rules(labels: &[&str]) -> String {
    labels.iter().fold(NO_ACCOUNT_RULES.to_owned(), |body, label| format!("{body}\n[[account]]\nlabel = \"{label}\"\n"))
}

/// `<state>/accounts/<label>` を dir で作り、直下に `files`（名前・本文）を置く。
fn account_fixture(place: &RolePlace, label: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = place.state.join("accounts").join(label);
    fs::create_dir_all(&dir).ok();
    for (name, body) in files {
        fs::write(dir.join(name), body).ok();
    }
    dir
}

/// doctor を撃ち、rc 0 を確かめて stdout の行を返す。
fn doctor_rows(place: &RolePlace, rules: &str) -> Vec<String> {
    let out = role_doctor_rules(place, rules);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    stdout_of(&out).lines().map(str::to_owned).collect()
}

/// 行の列のうち `account=<label> ` で始まる 1 行（無ければ空）。
fn account_line(lines: &[String], label: &str) -> String {
    let head = format!("account={label} ");
    lines.iter().find(|line| line.starts_with(&head)).cloned().unwrap_or_default()
}

/// `path` から下の全 entry の (path, 本文, mtime)（path の順）。dir の本文は空。
fn tree_facts(path: &Path) -> Vec<(PathBuf, Vec<u8>, Option<SystemTime>)> {
    let mtime = fs::metadata(path).and_then(|found| found.modified()).ok();
    let mut facts = vec![(path.to_path_buf(), fs::read(path).unwrap_or_default(), mtime)];
    let mut children: Vec<PathBuf> =
        fs::read_dir(path).map(|entries| entries.filter_map(|entry| entry.ok().map(|found| found.path())).collect()).unwrap_or_default();
    children.sort();
    for child in children {
        facts.extend(tree_facts(&child));
    }
    facts
}

/// credential だけの dir と設定 dir 全体（`settings.json` を持つ・link で置く周も）で `config=` が分かれ、
/// `agentview=` は `disableAgentView` を読む（歯 (a)・flip の RED＝base は口座の行を出さない）。
#[test]
fn doctor_accounts_config_splits_credential_only_dir_from_full_config_dir() {
    let place = role_place();
    account_fixture(&place, "cred-only", &[(".credentials.json", "{}")]);
    account_fixture(&place, "full-off", &[(".credentials.json", "{}"), ("settings.json", "{\"disableAgentView\": true}")]);
    account_fixture(&place, "full-on", &[(".credentials.json", "{}"), ("settings.json", "{\"disableAgentView\": false}")]);
    account_fixture(&place, "full-bare", &[("settings.json", "{}")]);
    account_fixture(&place, "full-broken", &[("settings.json", "{\"disableAgentView\": tru")]);
    account_fixture(&place, "full-shape", &[("settings.json", "{\"disableAgentView\": \"yes\"}")]);
    let real = place.dir.join("config-home");
    fs::create_dir_all(&real).ok();
    fs::write(real.join(".credentials.json"), "{}").ok();
    fs::write(real.join("settings.json"), "{\"disableAgentView\": true}").ok();
    std::os::unix::fs::symlink(&real, place.state.join("accounts").join("linked")).expect("link を置ける");
    let labels = ["cred-only", "full-off", "full-on", "full-bare", "full-broken", "full-shape", "linked", "gone"];
    let lines = doctor_rows(&place, &account_rules(&labels));
    for (label, rest) in [
        ("cred-only", "dir=present credential=present config=missing agentview=unreadable"),
        ("full-off", "dir=present credential=present config=present agentview=off"),
        ("full-on", "dir=present credential=present config=present agentview=on"),
        ("full-bare", "dir=present credential=missing config=present agentview=on"),
        ("full-broken", "dir=present credential=missing config=present agentview=unreadable"),
        ("full-shape", "dir=present credential=missing config=present agentview=unreadable"),
        ("linked", "dir=present credential=present config=present agentview=off"),
        ("gone", "dir=missing credential=missing config=missing agentview=unreadable"),
    ] {
        assert_eq!(account_line(&lines, label), format!("account={label} {rest} trust=n/a retired=no"), "{lines:?}");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// `.claude.json` の `projects[<anchor>].hasTrustDialogAccepted` で `trust=` が分かれ、file が無い・壊れた・形が違う
/// 周は `unreadable`（`missing` に潰さない）。別の anchor の key は数えない（歯 (b)）。
#[test]
fn doctor_accounts_trust_reads_the_anchor_key_and_never_folds_unreadable_into_missing() {
    let place = role_doctor_place();
    let key = |value: &str| format!("{{\"projects\":{{\"/repo\":{{\"hasTrustDialogAccepted\":{value}}}}}}}");
    let cases = [
        ("t-true", Some(key("true")), "accepted"),
        ("t-absent", Some("{\"projects\":{\"/repo\":{}}}".to_owned()), "missing"),
        ("t-false", Some(key("false")), "missing"),
        ("t-nofile", None, "unreadable"),
        ("t-broken", Some("{\"projects\":".to_owned()), "unreadable"),
        ("t-other", Some("{\"projects\":{\"/elsewhere\":{\"hasTrustDialogAccepted\":true}}}".to_owned()), "missing"),
        ("t-noproj", Some("{}".to_owned()), "missing"),
        ("t-shape", Some("{\"projects\":[]}".to_owned()), "unreadable"),
        ("t-string", Some(key("\"true\"")), "unreadable"),
    ];
    for (label, body, _) in &cases {
        let files: Vec<(&str, &str)> = body.iter().map(|found| (".claude.json", found.as_str())).collect();
        account_fixture(&place, label, &files);
    }
    let labels: Vec<&str> = cases.iter().map(|(label, _, _)| *label).collect();
    let lines = doctor_rows(&place, &account_rules(&labels));
    for (label, _, value) in &cases {
        let want = format!("account={label} dir=present credential=missing config=missing agentview=unreadable trust={value} retired=no");
        assert_eq!(account_line(&lines, label), want, "{lines:?}");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// 登録 row 0 件は `trust=n/a`・anchor が複数なら anchor ごとに `trust=<潰した anchor>:<値>`（辞書順）・event log を
/// 読めない周は `trust=unreadable`（n/a に潰さない）（歯 (c)）。
#[test]
fn doctor_accounts_trust_is_na_without_rows_and_per_anchor_with_many() {
    let place = role_place();
    let body = "{\"projects\":{\"/repo/a\":{\"hasTrustDialogAccepted\":true},\"/repo/b\":{}}}";
    account_fixture(&place, "multi", &[(".claude.json", body)]);
    let rules = account_rules(&["multi"]);
    let head = "account=multi dir=present credential=missing config=missing agentview=unreadable";
    assert_eq!(account_line(&doctor_rows(&place, &rules), "multi"), format!("{head} trust=n/a retired=no"), "登録 row 0 件");
    for (target, role, anchor) in [("mb:x", "planner", "/repo/b"), ("ma:x", "admin", "/repo/a"), ("mc:x", "admin", "/repo/b")] {
        role_stamp(&place, target, Some("sid-t"));
        let out = role_register(&place, target, role, &["--anchor", anchor]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    }
    let lines = doctor_rows(&place, &rules);
    assert_eq!(account_line(&lines, "multi"), format!("{head} trust=_repo_a:accepted trust=_repo_b:missing retired=no"), "{lines:?}");
    fs::write(vessel::fleet::store::events_path(&place.state), "not an event\n").expect("log を壊せる");
    let lines = doctor_rows(&place, &rules);
    // log を読めない周は退役も読めない（`no` に潰さない・C11）。
    assert_eq!(account_line(&lines, "multi"), format!("{head} trust=unreadable retired=unreadable"), "{lines:?}");
    assert!(lines.iter().any(|line| line.starts_with("seats: registered=unreadable")), "{lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// 口座の行は突合の行の直後に label の辞書順で並ぶ（宣言順ではない）（歯 (d)）。
#[test]
fn doctor_accounts_lines_follow_the_seat_lines_in_label_order() {
    let place = role_doctor_place();
    let lines = doctor_rows(&place, &account_rules(&["zeta", "alpha", "mid"]));
    let labels: Vec<&str> =
        lines.iter().filter_map(|line| line.strip_prefix("account=")).filter_map(|rest| rest.split(' ').next()).collect();
    assert_eq!(labels, ["alpha", "mid", "zeta"], "{lines:?}");
    let seats = lines.iter().position(|line| line.starts_with("seats: "));
    let host = lines.iter().position(|line| line == HOST_ABSENT);
    let first = lines.iter().position(|line| line.starts_with("account="));
    assert_eq!(host, seats.map(|at| at + 1), "host の面の行は突合の行の直後: {lines:?}");
    assert_eq!(first, seats.map(|at| at + 2), "口座の行は host の面の行の直後: {lines:?}");
    assert_eq!(lines.len(), 10, "2 行 + 登録 row 2 行 + 突合 1 行 + host の面 1 行 + 口座 3 行 + 導入先 1 行: {lines:?}");
    assert_eq!(lines.last().map(String::as_str), Some(CONSUMER_REPO), "導入先の行は口座の行の後ろ: {lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// `[[account]]` の無い manifest は口座の行 0 本で他の行は不変・`--rules` 無しは host の面（置き場の `host.toml`）の宣言・
/// 宣言なしなら行 0・読めない manifest は 1 行で名乗り rc は変えない・`--rules` の誤りは使い方で断る（歯 (e)・
/// 埋め込みの宣言に依らない形＝`s2-07l.243` の裁定 (B)）。
#[test]
fn doctor_accounts_no_declared_account_adds_no_line_and_keeps_the_rest() {
    let place = role_doctor_place();
    let without = doctor_rows(&place, NO_ACCOUNT_RULES);
    let with = doctor_rows(&place, &account_rules(&["solo"]));
    assert!(!without.iter().any(|line| line.starts_with("account=")), "{without:?}");
    assert_eq!(without.len(), 7, "2 行 + 登録 row 2 行 + 突合 1 行 + host の面 1 行 + 導入先 1 行: {without:?}");
    let rest: Vec<String> = with.iter().filter(|line| !line.starts_with("account=")).cloned().collect();
    assert_eq!(rest, without, "他の行は不変");
    assert_eq!(with.len(), without.len() + 1, "{with:?}");
    let state = place.state.display().to_string();
    let doctor = |args: &[&str]| Command::new(bin()).arg("doctor").args(args).output().expect("binary を起動できる");
    let labels_of = |out: &Output| -> Vec<String> {
        stdout_of(out)
            .lines()
            .filter_map(|line| line.strip_prefix("account=").and_then(|rest| rest.split(' ').next()).map(str::to_owned))
            .collect()
    };
    let bare = doctor(&["--state-dir", &state, "--tmux-socket", &place.socket]);
    assert_eq!(rc_of(&bare), i32::from(RC_OK), "stderr={}", stderr_of(&bare));
    assert_eq!(labels_of(&bare), Vec::<String>::new(), "--rules 無し・host の面も無い周は口座の行 0");
    let bare_out = stdout_of(&bare);
    let bare_tail: Vec<&str> = bare_out.lines().rev().take(2).collect();
    assert_eq!(bare_tail, [CONSUMER_REPO, HOST_ABSENT], "口座の行 0 でも導入先の行は出る: {bare_out}");
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), account_rules(&["zhost", "ahost"])).expect("host の面を書ける");
    let hosted = doctor(&["--state-dir", &state, "--tmux-socket", &place.socket]);
    assert_eq!(rc_of(&hosted), i32::from(RC_OK), "stderr={}", stderr_of(&hosted));
    assert_eq!(labels_of(&hosted), ["ahost", "zhost"], "--rules 無しは host の面の label の辞書順");
    fs::remove_file(place.state.join(vessel::rules::HOST_MANIFEST)).expect("host の面を外せる");
    let absent = place.dir.join("no-such-rules.toml").display().to_string();
    let unreadable = doctor(&["--state-dir", &state, "--tmux-socket", &place.socket, "--rules", &absent]);
    assert_eq!(rc_of(&unreadable), i32::from(RC_OK), "rc は変えない");
    assert_eq!(stdout_of(&unreadable).lines().last(), Some("accounts: manifest=unreadable"), "0 行に潰さない");
    let rules = fixture(&place.dir, "solo.toml", &account_rules(&["solo"]));
    for bad in [
        &["--rules", &rules][..],
        &["--state-dir", &state, "--rules", ""],
        &["--state-dir", &state, "--rules"],
        &["--state-dir", &state, "--rules", &rules, "--rules", &rules],
    ] {
        let out = doctor(bad);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{bad:?} は使い方で断る");
        let stdout = stdout_of(&out);
        assert!(stdout.starts_with("usage: ") && !stdout.contains("account="), "{bad:?}: {stdout}");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// doctor は口座の dir に何も書かない（全 entry の本文・mtime が不変・無い口座の dir を作らない）（歯 (f)）。
#[test]
fn doctor_accounts_writes_nothing_into_the_account_dirs() {
    let place = role_doctor_place();
    let full = [(".credentials.json", "{\"k\":1}"), ("settings.json", "{\"disableAgentView\": true}"), (".claude.json", "{\"projects\":{}}")];
    account_fixture(&place, "w-full", &full);
    account_fixture(&place, "w-empty", &[]);
    let accounts = place.state.join("accounts");
    let before = tree_facts(&accounts);
    assert_eq!(before.len(), 6, "accounts + dir 2 つ + file 3 つ: {before:?}");
    sleep(Duration::from_millis(20));
    let lines = doctor_rows(&place, &account_rules(&["w-full", "w-empty", "w-gone"]));
    assert_eq!(lines.iter().filter(|line| line.starts_with("account=")).count(), 3, "{lines:?}");
    assert_eq!(tree_facts(&accounts), before, "本文・mtime が不変");
    assert!(!accounts.join("w-gone").exists(), "無い口座の dir を作らない");
    fs::remove_dir_all(&place.dir).ok();
}

/// (f) doctor の `host-manifest=` は 3 値: 無い = `absent`・読める = `present`（口座の行は host の面込みの宣言）・
/// 壊れている / 面をまたいで重複する = `unreadable`（報告は止めない＝rc 0・口座の行は `accounts: manifest=unreadable`）。
/// 位置は突合の行の直後・口座の行の直前。
#[test]
fn rules_host_doctor_names_the_host_manifest_in_three_values() {
    let place = role_doctor_place();
    let host = place.state.join(vessel::rules::HOST_MANIFEST);
    let tail_of = |lines: &[String]| -> Vec<String> {
        let seats = lines.iter().position(|line| line.starts_with("seats: ")).unwrap_or(lines.len());
        lines.iter().skip(seats + 1).cloned().collect()
    };
    assert_eq!(
        tail_of(&doctor_rows(&place, &account_rules(&["tracked"]))),
        [HOST_ABSENT, account_line_of("tracked").as_str(), CONSUMER_REPO],
        "口座の行の後ろに導入先の行"
    );
    fs::write(&host, account_rules(&["hosted"])).expect("host の面を書ける");
    assert_eq!(
        tail_of(&doctor_rows(&place, &account_rules(&["tracked"]))),
        ["host-manifest=present".to_owned(), account_line_of("hosted"), account_line_of("tracked"), CONSUMER_REPO.to_owned()],
        "host の面込みの宣言（label の辞書順）"
    );
    let unreadable = ["host-manifest=unreadable", "accounts: manifest=unreadable"];
    fs::write(&host, "schema = 1\n\n[[account]]\nlabel = \"hosted\"\nbogus = 1\n").expect("host の面を壊せる");
    assert_eq!(tail_of(&doctor_rows(&place, &account_rules(&["tracked"]))), unreadable, "壊れた host の面");
    fs::write(&host, account_rules(&["tracked"])).expect("host の面を書ける");
    assert_eq!(tail_of(&doctor_rows(&place, &account_rules(&["tracked"]))), unreadable, "面をまたぐ重複も読めない側");
    fs::remove_dir_all(&place.dir).ok();
}

/// 登録 row が anchor `/repo` の置き場で、dir の無い口座 1 つの doctor の行。
fn account_line_of(label: &str) -> String {
    format!("account={label} dir=missing credential=missing config=missing agentview=unreadable trust=unreadable retired=no")
}

/// (d) 壊れた host の面では `seat tick` が typed に止まる: 判定を回さず stderr 1 行
/// `seat: tick decision=error reason=no-rule:manifest-unreadable`・rc 1・stdout 0 byte・event も記録も書かない・
/// 計測（client）を撃たない。
#[test]
fn rules_host_broken_host_manifest_stops_seat_tick_without_events() {
    let place = acct_place();
    let name = "hostbroken";
    let registered = acct_register(&place, name, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_credential(&place, ACCT_SEAT);
    acct_fake_curl(&place, 50);
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let events = vessel::fleet::store::events_path(&place.state);
    let before = fs::read(&events).unwrap_or_default();
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), "schema = 1\n\n[[launch-arg]]\nvalue = true\n").expect("host の面を壊せる");
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

    let (out, touched) = acct_tick_probed(&place, name, &pane);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    assert_eq!(stdout_of(&out), "", "stdout は 0 byte");
    assert_eq!(stderr_of(&out), "seat: tick decision=error reason=no-rule:manifest-unreadable\n", "1 行");
    assert!(!touched, "tmux に触れない");
    assert_eq!(fs::read(&events).unwrap_or_default(), before, "event を書かない");
    assert!(!tick_file(&place.state, name).exists(), "tick の記録も書かない");
    assert_eq!(acct_curl_calls(&place), 0, "計測を撃たない");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────────────── 壊れた --rules（s2-07l.154） ───────────────────────────

/// 欠陥 3 件の manifest（未知 kind / `ruling` 欠け / id 重複）。`line=` は key の行ではなく
/// `[[rule]]` の見出し行（3・11・26）。id 重複の 2 行は他に欠陥の無い完全な行にする
/// （欠陥のある行は `rows` に載らず、重複検査の母集団から消える）。
const BROKEN_RULES: &str = concat!(
    "schema = 1\n",
    "\n",
    "[[rule]]\n",
    "id = \"seat.unknown_kind\"\n",
    "kind = \"NoSuchKind\"\n",
    "value = 1\n",
    "enabled = true\n",
    "ruling = \"user 2026-09-12T02:01Z\"\n",
    "ruled_at = \"2026-09-12\"\n",
    "\n",
    "[[rule]]\n",
    "id = \"seat.no_ruling\"\n",
    "kind = \"LedgerTimeoutS\"\n",
    "value = 30\n",
    "enabled = true\n",
    "ruled_at = \"2026-09-12\"\n",
    "\n",
    "[[rule]]\n",
    "id = \"seat.ledger_timeout_s\"\n",
    "kind = \"LedgerTimeoutS\"\n",
    "value = 30\n",
    "enabled = true\n",
    "ruling = \"user 2026-09-12T02:01Z\"\n",
    "ruled_at = \"2026-09-12\"\n",
    "\n",
    "[[rule]]\n",
    "id = \"seat.ledger_timeout_s\"\n",
    "kind = \"LedgerTimeoutS\"\n",
    "value = 30\n",
    "enabled = true\n",
    "ruling = \"user 2026-09-12T02:01Z\"\n",
    "ruled_at = \"2026-09-12\"\n",
);

/// [`BROKEN_RULES`] の 3 欠陥が名指す行（見出し行）。
const BROKEN_LINES: [&str; 3] = [" line=3", " line=11", " line=26"];

/// tick の既存の断り行（判定行・契約の字面）。
const TICK_NO_RULE: &str = "seat: tick decision=error reason=no-rule";

/// `--rules` を受ける seat の口の全体（usage 行の 4 口）。
const RULES_FACES: [&str; 4] = ["tick", "cycle", "externalize", "rebrief"];

/// 壊れた `--rules` の歯の席名。
const RULES_TARGET: &str = "seatrules";

/// 壊れた `--rules` の歯の場所（tmp・空の置き場・rules の fixture）。
struct RulesPlace {
    /// tmp の root。
    dir: PathBuf,
    /// 置き場（空で作る＝断りの周に 1 file も増えないことを測る）。
    state: PathBuf,
    /// `--rules` に渡す path。
    rules: String,
}

/// 場所を作り、`body` を rules の fixture として書く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rules_place(body: &str) -> RulesPlace {
    let dir = tmp();
    let state = dir.join("state");
    for sub in [dir.join("wm"), state.clone()] {
        fs::create_dir_all(sub).expect("dir を作れる");
    }
    let rules = fixture(&dir, "rules.toml", body);
    RulesPlace { dir, state, rules }
}

/// stderr を行へ切る。
fn stderr_lines(out: &Output) -> Vec<String> {
    stderr_of(out).lines().map(str::to_owned).collect()
}

/// 同じ fixture への `rules validate --rules` の stderr の行（同じ描画の再利用を測る基準）。
fn validate_lines(rules: &str) -> Vec<String> {
    Command::new(bin())
        .args(["rules", "validate", "--rules", rules])
        .output()
        .map(|out| stderr_lines(&out))
        .unwrap_or_default()
}

/// 口 `face` を `--rules` 付きで 1 回撃つ（判定の前に断る周なので、plan / directives は名前だけ渡す）。
fn run_face(place: &RulesPlace, face: &str) -> Output {
    let wm = place.dir.join("wm").display().to_string();
    let state = place.state.display().to_string();
    let anchor = place.dir.display().to_string();
    let plan = place.dir.join("plan.md").display().to_string();
    let mut args = vec![face, "--target", RULES_TARGET, "--wm-dir", wm.as_str(), "--state-dir", state.as_str()];
    match face {
        "externalize" => args.extend(["--anchor", anchor.as_str(), "--plan", plan.as_str(), "--directives", plan.as_str()]),
        "rebrief" => args.extend(["--anchor", anchor.as_str()]),
        _ => {}
    }
    args.extend(["--rules", place.rules.as_str()]);
    run_seat(&args)
}

/// 口ごとの既存の断り行（行が無い周に出すものと同じ描画）。
fn judged_of(face: &str, state: &Path) -> Vec<String> {
    use vessel::seat::{cycle, externalize, rebrief};
    match face {
        "tick" => vec![vessel::seat::tick::render_no_rule()],
        "cycle" => {
            let place = vessel::seat::state_dir_of(Some(&state.display().to_string()));
            vec![cycle::render(RULES_TARGET, &cycle::Cycle::Refused(cycle::REASON_NO_RULE), place.as_ref())]
        }
        "externalize" => externalize::render_refused(&externalize::ExternalizeError::NoRule),
        _ => vec![rebrief::render_unavailable(rebrief::RebriefError::NoRule)],
    }
}

/// (a) 欠陥 3 件の manifest を `seat tick --rules` へ渡すと、defect を `rules validate` と同じ行で
/// 全件並べ、末尾に既存の判定行 1 行を残して rc 1（設計 rules-manifest.md §4.2 / §5・seat-autonomy.md §3）。
#[test]
fn seat_rules_broken_manifest_lists_every_defect() {
    let place = rules_place(BROKEN_RULES);
    let want = validate_lines(&place.rules);
    assert_eq!(want.len(), 3, "基準の rules validate は 3 件: {want:?}");
    let out = run_face(&place, "tick");
    let lines = stderr_lines(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{lines:?}");
    assert_eq!(out.stdout.len(), 0, "断りの周は stdout 0 byte");
    assert_eq!(lines.len(), 3 + 1, "defect 3 行 + 判定行 1 行: {lines:?}");
    assert_eq!(lines.get(..3), Some(want.as_slice()), "defect 行は rules validate と 1 byte 同じ");
    assert_eq!(lines.last().map(String::as_str), Some(TICK_NO_RULE), "判定行は消さない");
    for at in BROKEN_LINES {
        let hits = lines.iter().take(3).filter(|line| line.ends_with(at)).count();
        assert_eq!(hits, 1, "{at} は 1 回ずつ: {lines:?}");
    }
    assert!(lines.iter().take(3).all(|line| line.contains("line=")), "{lines:?}");
    assert!(!place.state.join("seat").exists(), "判定へ入らない（置き場の seat/ を作らない）");
    let left = tree_stat(&place.state);
    assert!(left.is_empty(), "置き場は空のまま: {left:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) 読めるが行を持たない manifest は既存どおり判定行 1 行だけ（「壊れている」と「行が無い」の弁別の
/// もう片側・全部を defect 列挙へ倒す実装を落とす）。
#[test]
fn seat_rules_absent_rows_still_refuse_with_one_line() {
    let place = rules_place("schema = 1\n");
    let out = run_face(&place, "tick");
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stderr={}", stderr_of(&out));
    assert_eq!(stderr_of(&out), format!("{TICK_NO_RULE}\n"), "行が無い周は 1 行のまま");
    assert_eq!(stderr_lines(&out).iter().filter(|line| line.contains("line=")).count(), 0);
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) `--rules` を受ける 4 口のどれも、壊れた manifest の defect を全件並べ、末尾に口の既存の断り行を
/// 残して rc 1 で断る（置き場へ 1 file も書かない）。`.ok()?` が 1 口でも残れば落ちる。
#[test]
fn seat_rules_broken_manifest_refuses_on_every_seat_face() {
    let mut refused = Vec::new();
    for face in RULES_FACES {
        let place = rules_place(BROKEN_RULES);
        let want = validate_lines(&place.rules);
        let out = run_face(&place, face);
        let lines = stderr_lines(&out);
        let judged = judged_of(face, &place.state);
        assert_eq!(want.len(), 3, "{face}（4 口のうち）: 基準 {want:?}");
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{face}（4 口のうち）: {lines:?}");
        assert_eq!(lines.len(), 3 + 1, "{face}（4 口のうち）: {lines:?}");
        assert_eq!(lines.get(..3), Some(want.as_slice()), "{face}（4 口のうち）");
        assert!(lines.iter().take(3).all(|line| line.contains("line=")), "{face}（4 口のうち）: {lines:?}");
        assert_eq!(lines.get(3..), Some(judged.as_slice()), "{face}（4 口のうち）: 末尾は既存の断り行");
        let left = tree_stat(&place.state);
        assert!(left.is_empty(), "{face}（4 口のうち）: 置き場は空のまま {left:?}");
        refused.push(face);
        fs::remove_dir_all(&place.dir).ok();
    }
    assert_eq!(refused, RULES_FACES, "4 / 4 の口が defect を全件並べて断る");
}

/// (d) 欠陥 k 個（k ∈ 1..=5）の manifest で、`seat tick --rules` の先頭 k 行は `rules validate` の行と
/// 多重集合で一致し、末尾に判定行 1 行が残る。性質の歯は通常 `prop.rs`（純関数の面）に置くが、
/// 本件は外形（binary の stderr）の性質なのでこの file に置き、verify 行を 1 本に保つ。
mod rules_prop {
    use super::{rules_place, run_face, stderr_lines, validate_lines, TICK_NO_RULE};
    use proptest::prelude::*;
    use proptest::test_runner::Config;
    use std::fs;

    /// 1 行に当てる欠陥（1 行につきちょうど 1 つ・loader が 1 欠陥 1 行で報告する形だけ）。
    ///
    /// 型不一致は `enabled` / `kind` にだけ当てる: `id` の型違いは「id が空である」を追撃して
    /// 2 行になり、`value` は未知 kind の行では報告されない。
    #[derive(Debug, Clone, Copy)]
    enum Defect {
        /// 未知 kind。
        UnknownKind,
        /// `ruling` 欠け。
        NoRuling,
        /// `enabled` が bool でない。
        EnabledType,
        /// `kind` が文字列でない。
        KindType,
    }

    /// 欠陥の生成。
    fn any_defect() -> impl Strategy<Value = Defect> {
        prop_oneof![Just(Defect::UnknownKind), Just(Defect::NoRuling), Just(Defect::EnabledType), Just(Defect::KindType)]
    }

    /// 欠陥 1 つを持つ `[[rule]]` 1 行（id は行ごとに別＝重複を混ぜない）。
    fn row(at: usize, defect: Defect) -> String {
        let (kind, enabled, ruling) = match defect {
            Defect::UnknownKind => ("\"NoSuchKind\"", "true", "ruling = \"user 2026-09-12T02:01Z\"\n"),
            Defect::NoRuling => ("\"LedgerTimeoutS\"", "true", ""),
            Defect::EnabledType => ("\"LedgerTimeoutS\"", "\"yes\"", "ruling = \"user 2026-09-12T02:01Z\"\n"),
            Defect::KindType => ("1", "true", "ruling = \"user 2026-09-12T02:01Z\"\n"),
        };
        format!("\n[[rule]]\nid = \"seat.row_{at}\"\nkind = {kind}\nvalue = 30\nenabled = {enabled}\n{ruling}ruled_at = \"2026-09-12\"\n")
    }

    /// case ごとに binary を 2 回起動するので 16 に絞り、反例の永続化を切る。
    fn config() -> Config {
        Config {
            cases: 16,
            failure_persistence: None,
            ..Config::default()
        }
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn seat_rules_broken_manifest_lists_k_defects(defects in prop::collection::vec(any_defect(), 1..=5)) {
            let rows: String = defects.iter().enumerate().map(|(at, defect)| row(at, *defect)).collect();
            let place = rules_place(&format!("schema = 1\n{rows}"));
            let mut want = validate_lines(&place.rules);
            let lines = stderr_lines(&run_face(&place, "tick"));
            fs::remove_dir_all(&place.dir).ok();
            let k = defects.len();
            prop_assert_eq!(want.len(), k);
            prop_assert_eq!(lines.len(), k + 1);
            let mut head: Vec<String> = lines.iter().take(k).cloned().collect();
            prop_assert!(head.iter().all(|line| line.contains("line=")));
            head.sort();
            want.sort();
            prop_assert_eq!(head, want);
            prop_assert_eq!(lines.last().map(String::as_str), Some(TICK_NO_RULE));
        }
    }
}

// ─────────────────── 口座の退避と立て直し（account-autonomy.md §5・`s2-07l.211`・接頭辞 `seat_account_`） ───────────────────

/// 立て直さなかったことの 3 面: 起動 script が走っていない・登録 row が増えない・cycle-stamp を打っていない。
fn acct_assert_not_relaunched(place: &AcctPlace, target: &str, case: &str) {
    assert!(!place.dir.join("launched").exists(), "{case}: 起動の雛形は注入されない");
    assert_eq!(acct_rows(&place.state).len(), 1, "{case}: SeatRegistered は増えない");
    assert!(!seat_dir_of(&place.state, target).join("cycle-stamp").exists(), "{case}: cycle-stamp を打たない");
}

/// 前面が shell でない席（prompt を 1 つ描いて `cat` に exec する＝前面 process は `cat`）を立てる。
fn acct_cat_seat(socket: &str, name: &str) -> IsolatedSeat {
    let mut seat = IsolatedSeat {
        socket: socket.to_owned(),
        name: name.to_owned(),
        ready: false,
    };
    let out = tmux(
        socket,
        &["new-session", "-d", "-s", name, "-n", name, "-x", "120", "-y", "40", "sh", "-c", "printf '\u{276f} '; exec cat >/dev/null"],
    );
    seat.ready = out.status.success() && wait_prompt(socket, name);
    seat
}

/// manifest の `[[account]]` に無い口座の row は計測しても行が積まれないので撃たない（毎周の計測に化けさせない）:
/// 他の口座に credential と偽 curl が在っても呼出 0・測れないまま `account=ghost:unmeasured`。
#[test]
fn seat_account_tick_never_measures_an_undeclared_account() {
    let place = acct_place();
    let name = "acctghost";
    let registered = acct_register_as(&place, name, "ghost", ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_credential(&place, ACCT_SEAT);
    acct_fake_curl(&place, 50);
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

    let (out, touched) = acct_tick_probed(&place, name, &pane);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), acct_line("decision=noop reason=busy", &format!("{ST_BUSY} account=ghost:unmeasured plugin=unrecorded"), &place.state));
    assert!(!touched, "注入しない");
    assert_eq!(acct_curl_calls(&place), 0, "宣言外の口座のために計測を撃たない");
    fs::remove_dir_all(&place.dir).ok();
}

/// (1) 登録 row の口座が閾値以上（90 ≥ 85）の席は、打刻が Busy でも idle を待たずに退避の合図 1 行を注入する
/// （`kind=externalize origin=account`・判定行に `account=a1:90`・打刻の合図は出ない）。注入の記録は
/// `<state_dir>/inject.jsonl` にも同じ席の行として残る（立て直しの入口 (1) の読み先・出所は終了の手の入口が読む・
/// `s2-07l.307`）。base は口座を見ず `noop reason=busy`（RED）。
#[test]
fn seat_account_tick_signals_externalize_over_threshold_while_busy() {
    let place = acct_place();
    let name = "acctover";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let registered = acct_register(&place, name, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 90, &acct_now());
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

    let out = acct_tick(&place, name, Some(&pane));

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        acct_line(
            &format!("decision=inject target={name} consumed=false kind=externalize origin=account"),
            &format!("{ST_BUSY} account=a1:90"),
            &place.state
        ),
        "busy でも退避の合図・判定行に出所（口座）と口座と逼迫度"
    );
    let seen = capture(&place.socket, name);
    assert!(
        seen.contains("口座 a1 90%") && seen.contains("閾値 85%") && seen.contains("/ready-compaction"),
        "口座・実測値・閾値・退避 skill が届く: {seen}"
    );
    assert!(!seen.contains("seat heartbeat"), "打刻の合図は出ない: {seen}");
    let log = fs::read_to_string(place.state.join("inject.jsonl")).unwrap_or_default();
    let rows: Vec<&str> = log.lines().collect();
    assert_eq!(rows.len(), 1, "注入 1 件の記録: {log}");
    let row = rows.first().copied().unwrap_or_default();
    assert_eq!(acct_text(row, "who").as_deref(), Some("seat-tick"), "{log}");
    assert_eq!(acct_text(row, "seat").as_deref(), Some(name), "{log}");
    assert!(
        acct_text(row, "what").is_some_and(|what| what.contains(" kind=externalize origin=account ") && what.contains(" account=a1:90")),
        "判定行と同じ字面（出所は kind の直後）: {log}"
    );
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (2) 閾値未満（50）は注入せず判定行に `account=a1:50` を載せ、以後は既存の順序どおり: Busy なら `busy`（tmux に
/// 触れない）、Idle なら打刻の合図（FR27 の「使用率が閾値未満」が立つ）。
#[test]
fn seat_account_tick_below_threshold_keeps_the_existing_order() {
    let place = acct_place();
    let name = "acctunder";
    let registered = acct_register(&place, name, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 50, &acct_now());
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

    let (out, touched) = acct_tick_probed(&place, name, &pane);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), acct_line("decision=noop reason=busy", &format!("{ST_BUSY} account=a1:50 plugin=unrecorded"), &place.state));
    assert!(!touched, "注入しない周は tmux に触れない");

    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    write_state(&seat_dir_of(&place.state, name), StateFix::Idle);
    let out = acct_tick(&place, name, Some(&pane));
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        acct_line(
            &format!("decision=inject target={name} consumed=false kind=pointer"),
            &format!("{ST_IDLE} account=a1:50 plugin=unrecorded"),
            &place.state
        ),
        "閾値未満の Idle な席には打刻の合図"
    );
    assert!(capture(&place.socket, name).contains("seat heartbeat"), "打刻の合図が届く");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (3) 実測行が無い口座は計測を 1 回撃ち（credential の無い口座は子 process を起こさず `no_credentials` の
/// Unmeasured 行になる）、測れないまま＝注入も停止もせず、打刻の合図だけを送らない（`account-unmeasured`・
/// tmux に触れない・注入の記録なし）。
#[test]
fn seat_account_tick_unmeasured_account_is_a_typed_noop() {
    use vessel::fleet::{Allowance, UnmeasuredReason};
    let place = acct_place();
    let name = "acctunmeasured";
    let registered = acct_register(&place, name, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    write_state(&seat_dir_of(&place.state, name), StateFix::Idle);
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

    let (out, touched) = acct_tick_probed(&place, name, &pane);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        acct_line("decision=noop reason=account-unmeasured", &format!("{ST_IDLE} account=a1:unmeasured plugin=unrecorded"), &place.state)
    );
    assert!(!touched, "注入しない（tmux に触れない）");
    assert!(!place.state.join("inject.jsonl").exists(), "注入の記録なし");
    let reasons: Vec<UnmeasuredReason> = vessel::fleet::store::read_all(&place.state)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|event| event.allowance)
        .filter_map(|row| match row {
            Allowance::Unmeasured(found) if found.account == ACCT_SEAT => Some(found.reason),
            Allowance::Unmeasured(_) | Allowance::Measured(_) => None,
        })
        .collect();
    assert_eq!(reasons, vec![UnmeasuredReason::NoCredentials], "実測行の無い周は計測を 1 回撃つ: {reasons:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (4) 最新の実測行が `seat.tick_stale_s` より古い口座は、判定の前に計測を 1 回撃ち（偽 curl の呼出 1 回）、新しい
/// 行（50）で判定する（古い行の 95 なら退避の合図になる）。行が新しくなった次の周は撃たない（定期計測はこの 1 形）。
#[test]
fn seat_account_tick_measures_once_when_the_latest_row_is_stale() {
    let place = acct_place();
    let name = "acctstale";
    let registered = acct_register(&place, name, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    let stale = vessel::fleet::cli::format_utc(unix_now().saturating_sub(STALE_S + 600));
    acct_measured(&place.state, ACCT_SEAT, 95, &stale);
    acct_credential(&place, ACCT_SEAT);
    acct_fake_curl(&place, 50);
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);
    let want = acct_line("decision=noop reason=busy", &format!("{ST_BUSY} account=a1:50 plugin=unrecorded"), &place.state);

    for round in [1_u32, 2] {
        let (out, touched) = acct_tick_probed(&place, name, &pane);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{round} 周目: stderr={}", stderr_of(&out));
        assert_eq!(stdout_of(&out), want, "{round} 周目: 計測した新しい行で判定する");
        assert!(!touched, "{round} 周目: 注入しない");
        assert_eq!(acct_curl_calls(&place), 1, "{round} 周目: 計測の子 process は通算 1 回");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// (5) 閾値以上でも FR29 と同じ除外の周は注入しない: 自席の未 consumed 退避物が在る・cycle lock が live（Busy の
/// 席は以後の順序どおり `busy`・tmux に触れない・注入の記録なし）。除外が無ければ (1) のとおり注入する。
#[test]
fn seat_account_tick_keeps_the_fr29_exclusions_over_threshold() {
    for case in ["wm-unconsumed", "cycle-live"] {
        let place = acct_place();
        let name = "acctexcluded";
        let registered = acct_register(&place, name, ACCT_LAUNCH);
        assert_eq!(rc_of(&registered), i32::from(RC_OK), "{case}: stderr={}", stderr_of(&registered));
        acct_measured(&place.state, ACCT_SEAT, 90, &acct_now());
        let seat = seat_dir_of(&place.state, name);
        write_state(&seat, StateFix::Busy { age_s: 0 });
        if case == "wm-unconsumed" {
            wm_file(&place.wm, "working-memory.parked.md", name);
        } else {
            fs::write(seat.join("cycle.lock"), "{}\n").ok();
        }
        let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

        let (out, touched) = acct_tick_probed(&place, name, &pane);

        assert_eq!(rc_of(&out), i32::from(RC_OK), "{case}: stderr={}", stderr_of(&out));
        assert_eq!(
            stdout_of(&out),
            acct_line("decision=noop reason=busy", &format!("{ST_BUSY} account=a1:90 plugin=unrecorded"), &place.state),
            "{case}: 閾値以上でも注入しない"
        );
        assert!(!touched, "{case}: tmux に触れない");
        assert!(!place.state.join("inject.jsonl").exists(), "{case}: 注入の記録なし");
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// (6) 退避して止まった席（直近の注入が退避の合図 ∧ その後の `Stop` ∧ pane の前面が shell）は、session 用の規則で
/// 選んだ別口座（a1 = 100 は当たっている・a2 = 30）で立て直す: 雛形の穴を a2 の credential dir で埋めた 1 行 →
/// 復元の command 1 行がこの順で注入され、`SeatRegistered`（account=a2・他の項目は既存 row のまま）が 1 件増え、
/// cycle-stamp が打たれる。次の周は更新した口座（a2）を読み、立て直しを繰り返さない。
#[test]
fn seat_account_relaunch_fills_the_template_on_another_account_and_restores() {
    let place = acct_place();
    let name = "acctrelaunch";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    acct_parked(&place, name, &acct_launcher(&place, name), 30);
    assert!(acct_shell_prompt(&place, name, ""), "席の終了後の pane は shell の prompt で終わる");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("account", "a1:100"), ("relaunch", ACCT_SPARE)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    acct_assert_launched_then_restored(&place, name);
    acct_assert_relabelled(&place, name);

    let again = stdout_of(&acct_tick(&place, name, None));
    assert_eq!(tick_token(&again, "account").as_deref(), Some("a2:30"), "次の周は更新した口座を読む: {again}");
    assert_eq!(tick_token(&again, "relaunch"), None, "立て直しを繰り返さない: {again}");
    assert_eq!(acct_rows(&place.state).len(), 2, "登録 row は増えない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (6) の入口の 3 条件は同時に要る: (a) 退避の合図の記録が無い停止（user の終了・crash）(b) `Stop` が合図より前
/// (c) pane の前面が shell でない（session が終わっていない）——いずれも立て直さず、閾値以上の口座は (1) の退避の
/// 合図の側へ落ちる（起動の雛形は注入されない・登録 row は増えない・cycle-stamp を打たない）。
#[test]
fn seat_account_relaunch_needs_the_signal_the_stop_and_a_shell() {
    for case in ["no-signal", "stop-before-signal", "not-a-shell"] {
        let place = acct_place();
        let name = "acctentry";
        let guard = if case == "not-a-shell" { acct_cat_seat(&place.socket, name) } else { start_seat(&place.socket, name) };
        assert!(guard.ready(), "{case}: 席を立てられる");
        let launch = acct_launcher(&place, name);
        let registered = acct_register(&place, name, &launch);
        assert_eq!(rc_of(&registered), i32::from(RC_OK), "{case}: stderr={}", stderr_of(&registered));
        acct_measured(&place.state, ACCT_SEAT, 100, &acct_now());
        acct_measured(&place.state, ACCT_SPARE, 30, &acct_now());
        match case {
            "no-signal" => acct_stop(&place, name, unix_now()),
            "stop-before-signal" => {
                acct_signal(&place, name);
                acct_stop(&place, name, unix_now().saturating_sub(100));
            }
            _ => {
                acct_signal(&place, name);
                acct_stop(&place, name, unix_now().saturating_add(1));
            }
        }

        let out = acct_tick(&place, name, None);

        let line = format!("{}{}", stdout_of(&out), stderr_of(&out));
        assert_eq!(tick_token(&line, "relaunch"), None, "{case}: 立て直しを評価しない: {line}");
        assert!(line.contains(" account=a1:100"), "{case}: 口座の軸は評価した: {line}");
        acct_assert_not_relaunched(&place, name, case);
        drop(guard);
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// (7) 選べる口座が無い（a1 = 100 は当たっている・a2 = 90 は閾値以上・他は実測行なし）周は立て直さない:
/// `noop reason=account-no-candidate` と選定の理由（`relaunch=none:over-threshold`）を判定行に残し、注入 0・
/// `SeatRegistered` 0・cycle-stamp なし（次の tick で選び直す）。
#[test]
fn seat_account_relaunch_without_a_candidate_is_a_typed_noop() {
    let place = acct_place();
    let name = "acctnone";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let launch = acct_launcher(&place, name);
    let registered = acct_register(&place, name, &launch);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 100, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, 90, &acct_now());
    let first = acct_signal(&place, name);
    assert_eq!(tick_token(&first, "kind").as_deref(), Some("externalize"), "1 周目は退避の合図: {first}");
    acct_stop(&place, name, unix_now().saturating_add(1));

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "候補なしは断りではない: stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "noop"), ("reason", "account-no-candidate"), ("account", "a1:100"), ("relaunch", "none:over-threshold")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    acct_assert_not_relaunched(&place, name, "no-candidate");
    assert_eq!(acct_injected(&place.state, name).len(), 1, "1 周目の退避の合図だけ（立て直しの注入 0）");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (8) 起動の雛形の穴が無い・2 つ在る row は typed に断る（`relaunch-launch-no-hole` / `relaunch-launch-many-holes`・
/// rc 1）: 注入 0・`SeatRegistered` 0・cycle-stamp なし（lock も打刻も取らない）。
#[test]
fn seat_account_relaunch_refuses_templates_without_exactly_one_hole() {
    for (tail, reason) in [("", "relaunch-launch-no-hole"), (" {account_dir} {account_dir}", "relaunch-launch-many-holes")] {
        let place = acct_place();
        let name = "acctholes";
        let guard = start_seat(&place.socket, name);
        assert!(guard.ready(), "{reason}: 独立 socket に shell の session を立てられる");
        let template = format!("{}{tail}", acct_launcher(&place, name).trim_end_matches(" {account_dir}"));
        let registered = acct_register(&place, name, &template);
        assert_eq!(rc_of(&registered), i32::from(RC_OK), "{reason}: stderr={}", stderr_of(&registered));
        acct_measured(&place.state, ACCT_SEAT, 100, &acct_now());
        acct_measured(&place.state, ACCT_SPARE, 30, &acct_now());
        acct_signal(&place, name);
        acct_stop(&place, name, unix_now().saturating_add(1));

        let out = acct_tick(&place, name, None);

        let line = stderr_of(&out);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{reason}: stdout={}", stdout_of(&out));
        assert_eq!(tick_token(&line, "decision").as_deref(), Some("error"), "{line}");
        assert_eq!(tick_token(&line, "reason").as_deref(), Some(reason), "{line}");
        acct_assert_not_relaunched(&place, name, reason);
        assert_eq!(acct_injected(&place.state, name).len(), 1, "{reason}: 1 周目の退避の合図だけ");
        drop(guard);
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// context 由来の退避の合図を実物の tick で 1 回注入させる（`--capture-file` の pane が cap 以上・打刻 Busy・登録 row の
/// 口座は閾値未満でも送る＝context の軸は口座の軸より前）。判定行が `kind=externalize origin=context` であることを
/// 確かめる。口座の軸の合図（[`acct_signal`]・`origin=account`）との対（`s2-07l.307`）。
pub(super) fn acct_context_signal(place: &AcctPlace, name: &str) {
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", &busy_pane_at(96));
    let line = stdout_of(&acct_tick(place, name, Some(&pane)));
    for (key, want) in [("kind", "externalize"), ("origin", "context")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "context 由来の合図: {key}: {line}");
    }
}

/// (9) 立て直しの入口 (3)「前面が shell」は合図の**出所を問わない**（`s2-07l.307`・極性不変の対・base でも PASS）:
/// context 由来の退避（記録の合図の行が `kind=externalize origin=context`・[`acct_context_signal`]）→ `Stop` の後に
/// user が手で session を終えた席（前面が shell）は、口座由来と同じく別口座で立て直す（`kind=relaunch`・起動 → 復元・
/// 登録 row の口座は a2 に）。終了の手（`/exit`）だけが出所で分かれる。
#[test]
fn seat_account_relaunch_runs_after_a_context_signal_when_the_front_is_a_shell() {
    let place = acct_place();
    let name = "acctctxshell";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let registered = acct_register(&place, name, &acct_launcher(&place, name));
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 100, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, 30, &acct_now());
    acct_context_signal(&place, name);
    acct_stop(&place, name, unix_now().saturating_add(1));
    assert!(acct_shell_prompt(&place, name, ""), "席の終了後の pane は shell の prompt で終わる");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("account", "a1:100"), ("relaunch", ACCT_SPARE)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(tick_token(&line, "origin"), None, "立て直しの判定行に出所は載らない: {line}");
    acct_assert_launched_then_restored(&place, name);
    acct_assert_relabelled(&place, name);
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (10) 立て直しは**自席の登録 row の口座に留まる**（ADR-0028 §2.4・consumer-sync.md §6・`s2-07l.312`）: context 由来の
/// 退避 → `Stop` → 前面が shell の席で、自席の口座 a1 = 13%（閾値未満）・他候補 a2 = 5% → 立て直しは a1（判定行
/// `account=a1:13 relaunch=a1`・雛形の穴は a1 の credential dir・`SeatRegistered` は口座 a1 のまま 1 件増える）。
/// base（逼迫度最小の a2）→ RED。planner / admin が立て直しのたびに別口座へ動いた形（2026-09-15 01:15Z 実測）の対。
#[test]
fn seat_account_relaunch_keeps_the_current_account_below_threshold() {
    let place = acct_place();
    let name = "acctstay";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let registered = acct_register(&place, name, &acct_launcher(&place, name));
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 13, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, 5, &acct_now());
    acct_context_signal(&place, name);
    acct_stop(&place, name, unix_now().saturating_add(1));
    assert!(acct_shell_prompt(&place, name, ""), "席の終了後の pane は shell の prompt で終わる");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("account", "a1:13"), ("relaunch", ACCT_SEAT)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    let own_dir = place.state.join("accounts").join(ACCT_SEAT);
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("{}\n", own_dir.display()),
        "穴は自席の口座（a1）の credential dir で埋まる（a2 ではない）"
    );
    assert_eq!(fs::read_to_string(place.dir.join("seat.log")).unwrap_or_default(), "/rebrief\n", "立ち上がった席が復元を受けた");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "SeatRegistered は 1 件増える: {rows:?}");
    assert_eq!(rows.last().map(|row| row.account.as_str()), Some(ACCT_SEAT), "口座は a1 のまま: {rows:?}");
    assert_eq!(rows.last(), rows.first(), "row は既存 row の写し（口座も含めて同じ）");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (11) (10) の極性の対: 自席の口座が閾値以上（85 ≥ R-C9-1 の 85）なら留まらず別口座（a2 = 5%）へ（口座由来の退避 →
/// `Stop` → 前面が shell）。判定行は `account=a1:85 relaunch=a2`・穴は a2 の credential dir・row の口座は a2 に
/// （base でも PASS＝極性不変）。
#[test]
fn seat_account_relaunch_leaves_the_current_account_at_threshold() {
    let place = acct_place();
    let name = "acctleave";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let registered = acct_register(&place, name, &acct_launcher(&place, name));
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 85, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, 5, &acct_now());
    let first = acct_signal(&place, name);
    assert_eq!(tick_token(&first, "kind").as_deref(), Some("externalize"), "閾値ちょうどは退避の合図: {first}");
    acct_stop(&place, name, unix_now().saturating_add(1));
    assert!(acct_shell_prompt(&place, name, ""), "席の終了後の pane は shell の prompt で終わる");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("account", "a1:85"), ("relaunch", ACCT_SPARE)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    acct_assert_launched_then_restored(&place, name);
    acct_assert_relabelled(&place, name);
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── hook 集合の食い違いの後の終了の手と立て直し（consumer-sync.md §6・AC32・`s2-07l.304`・接頭辞 `seat_tick_hook_drift_`） ───────────────────

/// 終了の手を受けた席が写す 1 行の置き場（名に `/exit` を含めない＝送達の目印の出現数を pane の echo で汚さない）。
const DRIFT_LOG: &str = "drift-received.log";

/// pane の前面 process の名（`#{pane_current_command}`・器の入口 (3) と同じ typed な読み）。
fn drift_front(place: &AcctPlace, name: &str) -> String {
    let out = tmux(&place.socket, &["list-panes", "-t", name, "-F", "#{pane_current_command}"]);
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// 前面が `want` になるまで待つ（上限 [`PROMPT_WAIT`]）。
fn drift_wait_front(place: &AcctPlace, name: &str, want: &str) -> bool {
    let deadline = Instant::now().checked_add(PROMPT_WAIT);
    while deadline.is_some_and(|at| Instant::now() < at) {
        if drift_front(place, name) == want {
            return true;
        }
        sleep(Duration::from_millis(100));
    }
    false
}

/// 前面が shell でない偽の席にする（終了の手の歯と同じ形）: `sh -i` の session に prompt `❯ ` を描いて **1 行だけ読む `head`**
/// を走らせる。前面は `head`（入口 (3) は立たない）で、受けた 1 行を [`DRIFT_LOG`] に写して終わり、shell へ戻る
/// （`/exit` を受けた席が終わる形）。前面が `head` になったかを返す。
fn drift_seat_head(place: &AcctPlace, name: &str) -> bool {
    let line = format!("printf '\\342\\235\\257 '; head -n 1 >> '{}'", place.dir.join(DRIFT_LOG).display());
    tmux(&place.socket, &["send-keys", "-t", name, "-l", &line]).status.success()
        && tmux(&place.socket, &["send-keys", "-t", name, "Enter"]).status.success()
        && drift_wait_front(place, name, "head")
}

/// (b) 経路の統合: hook 由来の退避の合図（記録 A ≠ 今 B・口座 a1 = 13 は閾値未満・候補 a2 = 5 はさらに低い）→ 退避 → `Stop` →
/// 前面が shell でない席には次の周に `/exit`（`.307` の入口・`origin=hook` は口座由来と同じ側）→ 席が終わって前面が shell →
/// 次の周に立て直しが**同じ target・同じ口座 a1**（閾値未満なら自席の口座を優先・`.312`）で走り、起動の後に `/rebrief` が
/// 注入される（席の記録は 退避の合図 → `/exit` → 起動 → 復元 の 4 行・登録 row は a1 のまま 1 件増える）。base は軸が無く
/// 合図が出ない（RED）。
#[test]
fn seat_tick_hook_drift_then_exit_and_relaunch_on_the_same_account() {
    let place = acct_place();
    let name = "hookrelaunch";
    let guard = drift_parked(&place, name);

    let second = acct_tick(&place, name, None);
    let line = stdout_of(&second);
    assert_eq!(rc_of(&second), i32::from(RC_OK), "2 周目: stdout={line} stderr={}", stderr_of(&second));
    for (key, want) in [("decision", "inject"), ("kind", "exit"), ("account", "a1:13")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "2 周目は終了の手: {key}: {line}");
    }
    assert_eq!((tick_token(&line, "cycle"), tick_token(&line, "plugin")), (None, None), "/clear の cycle は回さず hook の軸は評価しない: {line}");
    assert_eq!(fs::read_to_string(place.dir.join(DRIFT_LOG)).unwrap_or_default(), "/exit\n", "席が受けた 1 行は /exit");
    assert!(drift_wait_front(&place, name, "sh"), "受けた席は終わって前面が shell へ戻る");
    assert!(acct_shell_prompt(&place, name, ""), "shell の prompt を描ける");

    let third = acct_tick(&place, name, None);
    let line = stdout_of(&third);
    assert_eq!(rc_of(&third), i32::from(RC_OK), "3 周目: stdout={line} stderr={}", stderr_of(&third));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("account", "a1:13"), ("relaunch", ACCT_SEAT)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "3 周目は同じ口座で立て直し: {key}: {line}");
    }
    drift_assert_relaunched_on_own_account(&place, name);
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// hook 由来の退避で止まり、前面が shell でない席の fixture: 登録 row（起動の雛形は偽の session の script）・口座 a1 = 13
/// （閾値未満）・候補 a2 = 5（さらに低い＝逼迫度最小は a2）・記録 A ≠ 今 B → 実物の tick の合図（判定行
/// `kind=externalize origin=hook … plugin=drift`）→ `Stop` → 自席の未 consumed 退避物 → 前面を `head` に。
fn drift_parked(place: &AcctPlace, name: &str) -> IsolatedSeat {
    use vessel::hook::vessel::digest;
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    let registered = acct_register(place, name, &acct_launcher(place, name));
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 13, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, 5, &acct_now());
    let root = super::tick::drift_record(place, name, Some(super::tick::DRIFT_HOOKS_A), env!("SCRIBE2_BUILD_COMMIT"));
    fs::write(digest::hooks_path(&root), super::tick::DRIFT_HOOKS_B).ok();
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);
    let first = stdout_of(&acct_tick(place, name, Some(&pane)));
    for (key, want) in [("decision", "inject"), ("kind", "externalize"), ("origin", "hook"), ("account", "a1:13"), ("plugin", "drift")] {
        assert_eq!(tick_token(&first, key).as_deref(), Some(want), "1 周目は hook 由来の退避の合図: {key}: {first}");
    }
    acct_stop(place, name, unix_now().saturating_add(1));
    wm_file(&place.wm, "working-memory.parked.md", name);
    assert!(drift_seat_head(place, name), "前面が head の席を作れる");
    guard
}

/// 立て直した周の 4 面: 雛形の穴は自席の口座（a1）の credential dir・立ち上がった席が `/rebrief` を受けた・席の記録は
/// 退避の合図（`hook-drift`）→ `/exit` → 起動 → 復元 の 4 行・登録 row は a1 のまま 1 件増える（既存 row の写し）。
fn drift_assert_relaunched_on_own_account(place: &AcctPlace, name: &str) {
    let own_dir = place.state.join("accounts").join(ACCT_SEAT);
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("{}\n", own_dir.display()),
        "穴は自席の口座（a1）の credential dir で埋まる（a2 ではない）"
    );
    assert_eq!(fs::read_to_string(place.dir.join("seat.log")).unwrap_or_default(), "/rebrief\n", "立ち上がった席が復元を受けた");
    let sent = acct_sent(&place.state, name);
    assert_eq!(sent.len(), 4, "退避の合図・/exit・起動・復元の 4 行: {sent:?}");
    assert!(sent.first().is_some_and(|what| what.contains("hook-drift")), "1 行目は hook 由来の退避の合図: {sent:?}");
    assert_eq!(sent.get(1).map(String::as_str), Some("/exit"), "2 行目は終了の手: {sent:?}");
    assert_eq!(sent.get(3).map(String::as_str), Some("/rebrief"), "4 行目は復元: {sent:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "SeatRegistered は 1 件増える: {rows:?}");
    assert_eq!(rows.last(), rows.first(), "row は既存 row の写し（口座 a1 も含めて同じ）: {rows:?}");
}

// ─────────────────── 復元の第 2 手（account-autonomy.md §5「復元の第 2 手」・`s2-07l.318`・接頭辞 `seat_restore_`） ───────────────────

/// 起動したが 1 turn も始めていない席の fixture（立て直しが `restore-unconfirmed` で終端した直後の形・実地 2026-09-15
/// 03:15Z）: 受けた行を `seat.log` に積んで `UserPromptSubmit` → `Stop` を打つ偽の席（[`start_clearing_seat`]・入力欄は
/// 空）を独立 socket に立て、planner として口座 a1（50%・閾値未満＝退避の合図は出ない）で登録し（打刻は `SessionStart`・
/// いま）、cycle-stamp（中身は `stamp_ts`＝立て直しが起きた秒）を置く。`inject.jsonl` は無い（立て直しの失敗は注入の
/// 記録を残さない）。
fn restore_seat(place: &AcctPlace, name: &str, stamp_ts: u64) -> IsolatedSeat {
    let seat = seat_dir_of(&place.state, name);
    let log = place.dir.join("seat.log");
    let guard = start_clearing_seat(&place.socket, name, &log, &state_file(&seat), (FakeStamp::Now, FakeStamp::Now));
    assert!(guard.ready(), "独立 socket に偽の席を立てられる");
    let registered = acct_register(place, name, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 50, &acct_now());
    fs::write(seat.join("cycle-stamp"), format!("{stamp_ts}\n")).ok();
    guard
}

/// 席が受けた `/rebrief` の数（`seat.log` の行）。
fn restore_received(place: &AcctPlace) -> usize {
    fs::read_to_string(place.dir.join("seat.log")).unwrap_or_default().lines().filter(|line| *line == "/rebrief").count()
}

/// 打刻の 1 行を末尾に足す（`state` / `event` は契約の字面・時刻はいま）。
fn restore_append_stamp(place: &AcctPlace, name: &str, state: &str, event: &str) {
    let file = state_file(&seat_dir_of(&place.state, name));
    let mut text = fs::read_to_string(&file).unwrap_or_default();
    text.push_str(&stamp_line(state, event, unix_now(), ACCT_SID));
    text.push('\n');
    fs::write(&file, text).ok();
}

/// (a) 復元の第 2 手（`s2-07l.318`）: cycle-stamp が在り、打刻の最終行が `SessionStart`（ts ≥ stamp）・idle・入力欄が空の
/// 席（立て直しが `restore-unconfirmed` で終端した次の周の形）には、tick が復元の command をもう一度注入する
/// （`decision=inject … kind=restore consumed=true`・判定行に `restore-stamp=none` と口座・席が受けた行は `/rebrief` 1 回・
/// restore-stamp が打たれる・`inject.jsonl` に `kind=restore` の 1 行）。次の周は最終行が `Stop`（turn を始めた）なので
/// 第 2 手は立たない（`/rebrief` は 1 回のまま）。base は入口が 3 値で `kind=pointer` の打刻の合図（RED）。
#[test]
fn seat_restore_resends_when_the_seat_booted_without_a_turn() {
    let place = acct_place();
    let name = "acctrestore";
    let guard = restore_seat(&place, name, unix_now().saturating_sub(5));

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [
        ("decision", "inject"), ("kind", "restore"), ("consumed", "true"), ("state", "idle"),
        ("event", "SessionStart"), ("restore-stamp", "none"), ("account", "a1:50"),
    ] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(fs::read_to_string(place.dir.join("seat.log")).unwrap_or_default(), "/rebrief\n", "席が受けた行は復元 1 回");
    assert!(seat_dir_of(&place.state, name).join("restore-stamp").exists(), "送る前に restore-stamp を打つ");
    let log = fs::read_to_string(place.state.join("inject.jsonl")).unwrap_or_default();
    assert_eq!(log.lines().count(), 1, "注入の記録は 1 行: {log}");
    assert!(log.contains(" kind=restore "), "記録は復元の第 2 手: {log}");

    let again = stdout_of(&acct_tick(&place, name, None));
    assert_ne!(tick_token(&again, "kind").as_deref(), Some("restore"), "turn を始めた席には立たない: {again}");
    assert_eq!(restore_received(&place), 1, "/rebrief は 1 回のまま");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) 復元の第 2 手の brake（`s2-07l.318`・同じ rules 行 `seat.signal_backoff_s`・`.315` の読み口）: restore-stamp が
/// `seat.signal_backoff_s` 未満の前に打たれた周は送らない（`decision=noop reason=restore-recent`・判定行に
/// `restore-stamp=<経過>` と口座・rc 0・席は `/rebrief` を受けない）。stamp を窓の外（経過 = back-off・境界は未満）へ倒した
/// 次の周は送る（`kind=restore consumed=true`・`/rebrief` 1 回）＝brake は永久には止まらない。base に語が無い（RED）。
#[test]
fn seat_restore_backs_off_within_signal_backoff() {
    let place = acct_place();
    let name = "acctrestorebrake";
    let guard = restore_seat(&place, name, unix_now().saturating_sub(5));
    let stamp = seat_dir_of(&place.state, name).join("restore-stamp");
    fs::write(&stamp, format!("{}\n", unix_now())).ok();

    let first = acct_tick(&place, name, None);

    let line = stdout_of(&first);
    assert_eq!(rc_of(&first), i32::from(RC_OK), "再送しない周は正常の noop: stderr={}", stderr_of(&first));
    for (key, want) in [("decision", "noop"), ("reason", "restore-recent"), ("state", "idle"), ("event", "SessionStart"), ("account", "a1:50")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert!(tick_token(&line, "restore-stamp").is_some_and(|age| age != "none"), "back-off を読んだ周は経過が載る: {line}");
    assert_eq!(restore_received(&place), 0, "窓の内は席が受けない");

    backdate(&stamp, super::tick::SIGNAL_BACKOFF_S);
    let second = acct_tick(&place, name, None);

    let line = stdout_of(&second);
    assert_eq!(rc_of(&second), i32::from(RC_OK), "stderr={}", stderr_of(&second));
    for (key, want) in [("decision", "inject"), ("kind", "restore"), ("consumed", "true")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(restore_received(&place), 1, "stamp が窓の外（経過 = back-off）なら送る");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) 負例（(a) と 1 点だけ違える・別の理由で通らない形）: 最終行が `UserPromptSubmit` / `Stop`（turn を始めた席）・
/// cycle-stamp が無い（立て直しが起きていない）・`SessionStart` が stamp より前（起動に届かなかった周）は、いずれも
/// 第 2 手を送らない（判定行に `kind=restore` は出ず、席は `/rebrief` を受けず、restore-stamp も打たれない）。
#[test]
fn seat_restore_stays_quiet_after_the_first_turn() {
    for case in ["after-prompt", "after-stop", "no-cycle-stamp", "start-before-stamp"] {
        let place = acct_place();
        let name = "acctnorestore";
        let stamp_ts = if case == "start-before-stamp" { unix_now().saturating_add(100) } else { unix_now().saturating_sub(5) };
        let guard = restore_seat(&place, name, stamp_ts);
        let seat = seat_dir_of(&place.state, name);
        match case {
            "after-prompt" => restore_append_stamp(&place, name, "busy", "UserPromptSubmit"),
            "after-stop" => restore_append_stamp(&place, name, "idle", "Stop"),
            "no-cycle-stamp" => assert!(fs::remove_file(seat.join("cycle-stamp")).is_ok(), "{case}: stamp を消せる"),
            _ => {}
        }

        let out = acct_tick(&place, name, None);

        let line = format!("{}{}", stdout_of(&out), stderr_of(&out));
        assert_ne!(tick_token(&line, "kind").as_deref(), Some("restore"), "{case}: 第 2 手は立たない: {line}");
        assert_eq!(tick_token(&line, "restore-stamp"), None, "{case}: 第 2 手を評価しない: {line}");
        assert_eq!(restore_received(&place), 0, "{case}: 席は復元を受けない");
        assert!(!seat.join("restore-stamp").exists(), "{case}: restore-stamp を打たない");
        drop(guard);
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// (d) restore-stamp を**打てない**周は 1 key も送らず断る（`decision=error reason=restore-stamp-unwritable`・rc 1・
/// write-ahead・N1）: stamp の位置に dir を置き、その mtime を back-off の外へ倒す（`restore-recent` でなく書き口で
/// 落ちる形）。pane は `--capture-file` で通し、tmux の shim で「触れていない」を測る。
#[test]
fn seat_restore_writes_the_stamp_before_sending() {
    let place = acct_place();
    let name = "acctrestorero";
    let guard = restore_seat(&place, name, unix_now().saturating_sub(5));
    let blocker = seat_dir_of(&place.state, name).join("restore-stamp");
    let old = SystemTime::now()
        .checked_sub(Duration::from_secs(super::tick::SIGNAL_BACKOFF_S.saturating_add(60)))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let placed = fs::create_dir_all(&blocker).and_then(|()| fs::File::open(&blocker)).and_then(|dir| dir.set_modified(old));
    assert!(placed.is_ok(), "stamp の位置に古い dir を置ける: {placed:?}");
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

    let (out, touched) = acct_tick_probed(&place, name, &pane);

    let line = stderr_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    for (key, want) in [("decision", "error"), ("reason", "restore-stamp-unwritable"), ("state", "idle"), ("event", "SessionStart")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert!(tick_token(&line, "restore-stamp").is_some_and(|age| age != "none"), "back-off は読んだ: {line}");
    assert!(!touched, "tmux に触れない＝1 key も送らない");
    assert_eq!(restore_received(&place), 0, "席は復元を受けない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// 席を planner として口座 [`ACCT_SEAT`] と `--model <model>` で `seat register` の口で登録する（[`acct_register_as`] と同じ
/// 打刻の前提・`model` は表示名か別名）。
fn acct_register_with_model(place: &AcctPlace, target: &str, launch: &str, model: &str) -> Output {
    let seat = seat_dir_of(&place.state, target);
    fs::create_dir_all(&seat).ok();
    fs::write(state_file(&seat), format!("{}\n", stamp_line("idle", "SessionStart", unix_now(), ACCT_SID))).ok();
    let launch_file = fixture(&place.dir, "launch.txt", launch);
    let state = place.state.display().to_string();
    run_seat(&[
        "register", "--state-dir", &state, "--target", target, "--role", "planner", "--account", ACCT_SEAT,
        "--launch", &launch_file, "--anchor", ACCT_ANCHOR, "--model", model,
    ])
}

/// 退避して止まった席の fixture（[`acct_parked`] と同じ・登録は済んでいる周・a1 = 100・a2 = 30・退避の合図 → `Stop` → shell）。
fn acct_park_registered(place: &AcctPlace, target: &str) {
    acct_measured(&place.state, ACCT_SEAT, 100, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, 30, &acct_now());
    let first = acct_signal(place, target);
    assert_eq!(tick_token(&first, "kind").as_deref(), Some("externalize"), "1 周目は退避の合図: {first}");
    acct_stop(place, target, unix_now().saturating_add(1));
    assert!(acct_shell_prompt(place, target, ""), "席の終了後の pane は shell の prompt で終わる");
}

/// (12) 起動行は row の model を typed に運ぶ（`s2-07l.313`・C10 / SRS FR36 / FR38・account-lifecycle.md §4）: `--model Opus`
/// （表示名）で登録した席を立て直すと、注入する起動行は雛形の末尾（`claude` の語を持たない雛形）に**別名** `--model opus` を
/// ちょうど 1 つ持つ。雛形（row の `launch`）は書き換えず（写した row にも `--model` の語は無い・`model` は `Opus` のまま写す）、
/// 穴は a2 の credential dir で埋まり復元も届く。base の起動行は `--model` を持たない（RED）。
#[test]
fn seat_account_relaunch_carries_the_row_model() {
    let place = acct_place();
    let name = "acctmodel";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let launch = acct_launcher(&place, name);
    let registered = acct_register_with_model(&place, name, &launch, "Opus");
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    assert!(stdout_of(&registered).trim_end().ends_with(" model=Opus"), "表示名で登録できる: {}", stdout_of(&registered));
    acct_park_registered(&place, name);

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("relaunch", ACCT_SPARE)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    acct_assert_launched_then_restored(&place, name);
    acct_assert_relabelled(&place, name);
    let spare_dir = place.state.join("accounts").join(ACCT_SPARE).display().to_string();
    let sent = acct_sent(&place.state, name);
    assert_eq!(
        sent.get(1).map(String::as_str),
        Some(format!("CLAUDE_CODE_DISABLE_AGENT_VIEW=1 {} --model opus", launch.replace("{account_dir}", &spare_dir)).as_str()),
        "起動行は row の model を別名で 1 つ運ぶ（`claude` の語が無い雛形は末尾）: {sent:?}"
    );
    let rows = acct_rows(&place.state);
    assert!(rows.iter().all(|row| row.model.as_deref() == Some("Opus")), "row の model は表示名のまま: {rows:?}");
    assert!(rows.iter().all(|row| !row.launch.split(' ').any(|word| word == "--model")), "雛形に `--model` は書かない: {rows:?}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (13) row の `model` が表に無い（`nope`）席は立て直さない: `relaunch-model-unknown`（rc 1・decision=error）・注入 0・
/// `SeatRegistered` 0・cycle-stamp なし（壊れた宣言値で黙って settings の model で立てない・C10）。`seat register --model nope`
/// 自体は使い方で断り row を書かない（未知の値を row に書かない）ので、壊れた row は `role::register` で直に積む。base は黙って
/// 起こす（RED）。
#[test]
fn seat_account_relaunch_refuses_an_unknown_row_model() {
    let place = acct_place();
    let name = "acctnomodel";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let launch = acct_launcher(&place, name);
    let via_cli = acct_register_with_model(&place, name, &launch, "nope");
    assert_eq!(rc_of(&via_cli), i32::from(RC_REFUSED), "表に無い --model は使い方で断る: stdout={}", stdout_of(&via_cli));
    assert_eq!(stderr_of(&via_cli), stderr_of(&run_seat(&[])), "使い方の 1 本と同じ字面");
    assert!(stdout_of(&via_cli).is_empty(), "stdout は空");
    assert!(acct_rows(&place.state).is_empty(), "未知の値は row に書かない");
    let broken = vessel::fleet::Registration {
        role: vessel::seat::role::Role::Planner,
        anchor: ACCT_ANCHOR.to_owned(),
        target: name.to_owned(),
        sid: Some(ACCT_SID.to_owned()),
        account: ACCT_SEAT.to_owned(),
        launch,
        model: Some("nope".to_owned()),
    };
    assert!(vessel::seat::role::register(&place.state, broken).is_ok(), "壊れた row を直に積める");
    acct_park_registered(&place, name);

    let out = acct_tick(&place, name, None);

    let line = stderr_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(tick_token(&line, "decision").as_deref(), Some("error"), "{line}");
    assert_eq!(tick_token(&line, "reason").as_deref(), Some("relaunch-model-unknown"), "{line}");
    acct_assert_not_relaunched(&place, name, "model-unknown");
    assert_eq!(acct_injected(&place.state, name).len(), 1, "1 周目の退避の合図だけ");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 席の起動（account-lifecycle.md §4・ADR-0026 §2.3・`s2-07l.244`・接頭辞 `seat_launch_`） ───────────────────

/// host の面（`<state>/host.toml`）に宣言する口座（tracked の manifest に口座は無い）。
const LAUNCH_LABELS: [&str; 2] = ["l1", "l2"];
/// host の面の `[[plugin]]` の dir（宣言順＝起動行の順・名前の昇順ではない）。
const LAUNCH_PLUGINS: [&str; 2] = ["/opt/plug-b", "/opt/plug-a"];
/// host の面の `[[launch-arg]]` の value（宣言順）。
const LAUNCH_ARGS: [&str; 2] = ["--permission-mode", "bypassPermissions"];
/// 起動の歯の shell の prompt（`$ ` で終わる＝shell の門を通る）。
const LAUNCH_PS1: &str = "PS1=$ ";
/// 偽 claude が受けた行の置き場。
const LAUNCH_LOG: &str = "seat.log";
/// 偽 claude が起動時に写す event log の複製（登録 row が**送る前**に在ったことの証拠）。
const LAUNCH_EVENTS_SEEN: &str = "events-at-launch";
/// 包みの tmux が写す argv の置き場。
const LAUNCH_TMUX_ARGS: &str = "tmux-args";

/// 起動の歯の置き場: [`acct_place`] に host の面（口座 2 つ・plugin 2 つ・引数 2 つ）と anchor の dir を足す。
fn launch_place() -> AcctPlace {
    let place = acct_place();
    fs::create_dir_all(&place.state).ok();
    fs::create_dir_all(place.dir.join("anchor")).ok();
    let accounts: String = LAUNCH_LABELS.iter().map(|label| format!("\n[[account]]\nlabel = \"{label}\"\n")).collect();
    let plugins: String = LAUNCH_PLUGINS.iter().map(|dir| format!("\n[[plugin]]\ndir = \"{dir}\"\n")).collect();
    let args: String = LAUNCH_ARGS.iter().map(|value| format!("\n[[launch-arg]]\nvalue = \"{value}\"\n")).collect();
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), format!("schema = 1\n{accounts}{plugins}{args}")).ok();
    place
}

/// anchor の dir（登録 row の `anchor`・起動行の 1 つ目の `--plugin-dir`）。
fn launch_anchor(place: &AcctPlace) -> String {
    place.dir.join("anchor").display().to_string()
}

/// 期待する導出行（穴を埋める前・`launch` の row に載る形）。
fn launch_derived(place: &AcctPlace) -> String {
    format!(
        "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={{account_dir}} claude --plugin-dir {} --plugin-dir {} --plugin-dir {} {} {}",
        launch_anchor(place), LAUNCH_PLUGINS[0], LAUNCH_PLUGINS[1], LAUNCH_ARGS[0], LAUNCH_ARGS[1]
    )
}

/// 期待する偽 claude の記録（argv を 1 語 1 行・続けて env の 2 行）。
fn launch_expected_argv(place: &AcctPlace, label: &str) -> String {
    let account_dir = place.state.join("accounts").join(label).display().to_string();
    format!(
        "--plugin-dir\n{}\n--plugin-dir\n{}\n--plugin-dir\n{}\n{}\n{}\nenv:CLAUDE_CONFIG_DIR={account_dir}\nenv:CLAUDE_CODE_DISABLE_AGENT_VIEW=1\n",
        launch_anchor(place), LAUNCH_PLUGINS[0], LAUNCH_PLUGINS[1], LAUNCH_ARGS[0], LAUNCH_ARGS[1]
    )
}

/// test 自身の PATH に在る tmux（包みが exec する実体）。
fn real_tmux() -> Option<PathBuf> {
    std::env::var("PATH").ok()?.split(':').map(|dir| Path::new(dir).join("tmux")).find(|path| path.is_file())
}

/// shim の dir を作る（偽 `claude`・argv を写して実体へ exec する `tmux` の包み）: 偽 claude は argv と env を `launched` へ
/// 写し、その時点の event log を [`LAUNCH_EVENTS_SEEN`] へ複製し、prompt を描いて `SessionStart` を打ち、以後は受けた行を
/// [`LAUNCH_LOG`] に積んで `UserPromptSubmit` → `Stop` を打つ（立て直しの偽 session と同じ形）。PATH の字面を返す。
fn launch_shims(place: &AcctPlace, target: &str) -> String {
    let bin = place.dir.join("bin");
    fs::create_dir_all(&bin).ok();
    let seat = seat_dir_of(&place.state, &target.replace(':', "_"));
    let file = state_file(&seat);
    let claude = format!(
        "#!/bin/sh\nmkdir -p '{seat}'\nprintf '%s\\n' \"$@\" >> '{launched}'\n\
         printf 'env:CLAUDE_CONFIG_DIR=%s\\nenv:CLAUDE_CODE_DISABLE_AGENT_VIEW=%s\\n' \"$CLAUDE_CONFIG_DIR\" \"$CLAUDE_CODE_DISABLE_AGENT_VIEW\" >> '{launched}'\n\
         cp '{events}' '{seen}' 2>/dev/null\nprintf '\u{276f} '\n{start}\n\
         while read -r line; do printf '%s\\n' \"$line\" >> '{log}'; {busy}; {stop}; printf '\u{276f} '; done\n",
        seat = seat.display(),
        launched = place.dir.join("launched").display(),
        events = vessel::fleet::store::events_path(&place.state).display(),
        seen = place.dir.join(LAUNCH_EVENTS_SEEN).display(),
        start = stamp_cmd(&file, "idle", "SessionStart", FakeStamp::Now),
        log = place.dir.join(LAUNCH_LOG).display(),
        busy = stamp_cmd(&file, "busy", "UserPromptSubmit", FakeStamp::Now),
        stop = stamp_cmd(&file, "idle", "Stop", FakeStamp::Now),
    );
    let tmux = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec '{}' \"$@\"\n",
        place.dir.join(LAUNCH_TMUX_ARGS).display(),
        real_tmux().unwrap_or_default().display()
    );
    for (name, body) in [("claude", claude), ("tmux", tmux)] {
        let path = bin.join(name);
        fs::write(&path, body).ok();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).ok();
    }
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default())
}

/// 独立 socket に session `name`（初期 window も `name`・`sh -i`・prompt `$ `・PATH は shim 先頭）を立て、以後の window も同じ
/// 形（`default-command` = 同じ shell の command・login shell にしない＝home の profile の PATH / PS1 を継承しない）にする。
fn launch_session(place: &AcctPlace, name: &str, path: &str) -> IsolatedSeat {
    let mut seat = IsolatedSeat { socket: place.socket.clone(), name: name.to_owned(), ready: false };
    // PATH は shell の command の中で据える（session の環境変数 `-e PATH=` は login の profile に上書きされる・実測 2026-09-14）。
    let shell = format!("PATH='{path}'; export PATH; exec sh -i");
    let out = tmux(
        &place.socket,
        &["new-session", "-d", "-s", name, "-n", name, "-x", "120", "-y", "40", "-e", LAUNCH_PS1, "sh", "-c", &shell],
    );
    if !out.status.success() {
        return seat;
    }
    let shell = tmux(&place.socket, &["set-option", "-t", name, "default-command", &shell]);
    seat.ready = shell.status.success() && acct_wait_pane(place, &format!("{name}:{name}"), |pane| pane.trim_end().ends_with('$'));
    seat
}

/// `seat launch` を shim の PATH で 1 回撃つ（`--tmux-socket` は独立 socket・`--anchor` は置き場の anchor）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn launch_run(place: &AcctPlace, path: &str, target: &str, extra: &[&str]) -> Output {
    let state = place.state.display().to_string();
    let anchor = launch_anchor(place);
    let mut args = vec![
        "seat", "launch", "--state-dir", &state, "--role", "planner", "--target", target, "--anchor", &anchor, "--tmux-socket", &place.socket,
    ];
    args.extend_from_slice(extra);
    Command::new(bin()).args(&args).env("PATH", path).output().expect("binary を起動できる")
}

/// 包みの tmux が写した argv のうち `verb` で始まる呼出しの数（`-S <socket>` の後ろを見る）。
fn launch_tmux_calls(place: &AcctPlace, verb: &str) -> usize {
    fs::read_to_string(place.dir.join(LAUNCH_TMUX_ARGS))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.split_whitespace().nth(2) == Some(verb))
        .count()
}

/// `<state>/inject.jsonl` の行のうち `kind=launch` を持つものの `(who, what)`。
fn launch_inject_rows(place: &AcctPlace) -> Vec<(String, String)> {
    fs::read_to_string(place.state.join("inject.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| Some((acct_text(line, "who")?, acct_text(line, "what")?)))
        .filter(|(_, what)| tick_token(what, "kind").as_deref() == Some("launch"))
        .collect()
}

/// 起動しなかったことの 3 面（`case`）: 偽 claude は走らず・`send-keys` は 0 回・登録 row は `rows` 件。
fn launch_assert_not_sent(place: &AcctPlace, rows: usize, case: &str) {
    assert!(!place.dir.join("launched").exists(), "{case}: 起動行は届かない");
    assert_eq!(launch_tmux_calls(place, "send-keys"), 0, "{case}: 1 key も送らない");
    assert_eq!(acct_rows(&place.state).len(), rows, "{case}: 登録 row の件数");
    assert!(launch_inject_rows(place).is_empty(), "{case}: inject.jsonl に launch の行は無い");
}

/// (a)(b) window が無い target へ `--account l2` で起こす: `new-window` が 1 回・導出した行（`CLAUDE_CONFIG_DIR=<state>/accounts/l2`・
/// agent view off・anchor の `--plugin-dir` → `[[plugin]]` の dir → `[[launch-arg]]` の value の順）が偽 claude に 1 回だけ届き、
/// `SeatRegistered` は送る**前**に 1 件（`sid` 無し・`launch` = 導出した行〔穴を埋める前〕・account=l2）、`inject.jsonl` に
/// `kind=launch` 1 行、席の打刻に `SessionStart`。base は `launch` の subcommand が無く使い方で断る（RED）。
#[test]
fn seat_launch_creates_the_window_and_injects_the_derived_line_once() {
    let place = launch_place();
    let name = "launchnew";
    let target = format!("{name}:seat");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let out = launch_run(&place, &path, &target, &["--account", "l2"]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    assert_eq!(line, format!("seat launch: launched target={name}_seat account=l2{}\n", provenance(&place.state, "flag")));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "window を 1 回作る");
    assert!(
        fs::read_to_string(place.dir.join(LAUNCH_TMUX_ARGS)).unwrap_or_default().lines().any(|found| found.ends_with(&format!("new-window -t ={name}: -n seat"))),
        "`new-window -t <session> -n <window>` の形（session は exact の名 + 次の空き index）: {}",
        fs::read_to_string(place.dir.join(LAUNCH_TMUX_ARGS)).unwrap_or_default()
    );
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_expected_argv(&place, "l2"), "導出した行が 1 回だけ届く");
    launch_assert_registered_before_send(&place, &target, "l2");
    let stamps = fs::read_to_string(state_file(&seat_dir_of(&place.state, &format!("{name}_seat")))).unwrap_or_default();
    assert!(stamps.contains("\"event\":\"SessionStart\""), "席が立った打刻: {stamps}");
    assert!(!place.dir.join(LAUNCH_LOG).exists(), "`--restore` 無しは復元を送らない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) 登録 row は**送る前**に 1 件（偽 claude が起動時に写した event log に既に在る・`sid` 無し・`launch` = 導出した行・
/// account = `label`・鍵 = planner × anchor）・`inject.jsonl` に `kind=launch`（`who=seat-launch`）が 1 行。
fn launch_assert_registered_before_send(place: &AcctPlace, target: &str, label: &str) {
    let rows = acct_rows(&place.state);
    let want = vessel::fleet::Registration {
        role: vessel::seat::role::Role::Planner,
        anchor: launch_anchor(place),
        target: target.to_owned(),
        sid: None,
        account: label.to_owned(),
        launch: launch_derived(place),
        model: None,
    };
    assert_eq!(rows, vec![want], "SeatRegistered 1 件・sid 無し・launch は導出した行（穴を埋める前）");
    let seen_text = fs::read_to_string(place.dir.join(LAUNCH_EVENTS_SEEN)).unwrap_or_default();
    assert_eq!(seen_text.lines().filter(|found| found.contains("\"kind\":\"SeatRegistered\"")).count(), 1, "row は起動行を送る前に在る: {seen_text}");
    assert!(!seen_text.contains("\"sid\""), "送る前の row にも sid は無い: {seen_text}");
    let injected = launch_inject_rows(place);
    assert_eq!(injected.len(), 1, "inject.jsonl に kind=launch 1 行: {injected:?}");
    assert_eq!(injected.first().map(|(who, _)| who.as_str()), Some("seat-launch"));
    assert!(injected.first().is_some_and(|(_, what)| tick_token(what, "account").as_deref() == Some(label)), "{injected:?}");
}

/// (c) `--account` 無しは session 用の選定: 実測行 2 口座（l1 = 30・l2 = 40・どちらも閾値未満・逼迫度の最小は l1）のうち、
/// 別席（同じ planner でも別 anchor＝別の鍵）の登録 row が持つ l1 を除外し、残る l2 が選ばれる（既存 window `name:name` へ・
/// `new-window` は 0 回）。
#[test]
fn seat_launch_without_account_selects_excluding_other_seats_accounts() {
    let place = launch_place();
    let name = "launchpick";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    acct_measured(&place.state, "l1", 30, &acct_now());
    acct_measured(&place.state, "l2", 40, &acct_now());
    let other = acct_register_as(&place, "otherseat", "l1", ACCT_LAUNCH);
    assert_eq!(rc_of(&other), i32::from(RC_OK), "別席の登録: stderr={}", stderr_of(&other));

    let out = launch_run(&place, &path, &target, &[]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    assert!(line.starts_with(&format!("seat launch: launched target={name}_{name} account=l2 ")), "l1 は別席の口座＝除外: {line}");
    assert_eq!(launch_tmux_calls(&place, "new-window"), 0, "既存 window には作らない");
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_expected_argv(&place, "l2"));
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "別席の row + 自席の row: {rows:?}");
    assert_eq!(rows.last().map(|row| (row.account.as_str(), row.sid.clone())), Some(("l2", None)));
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) typed な断り（rc 1・stderr 1 行・`reason=` を値で名指す）で row も key も書かない: 候補なし（実測行なし＝`no-account`
/// `detail=unmeasured`）／session 無し（`session-missing`・作らない）／`--account` が宣言に無い（`account-unknown`）。
/// 入力欄に打ちかけ（`input-busy`）だけは門で止まる周＝row は書き終えているが 1 key も送らない。
#[test]
fn seat_launch_refuses_typed_without_sending_or_registering() {
    for (case, extra, rows) in [
        ("no-account", &[][..], 0),
        ("session-missing", &["--account", "l2"][..], 0),
        ("account-unknown", &["--account", "ghost"][..], 0),
        ("input-busy", &["--account", "l2"][..], 1),
    ] {
        let place = launch_place();
        let name = "launchrefuse";
        let target = format!("{name}:{name}");
        let path = launch_shims(&place, &target);
        let guard = launch_session(&place, name, &path);
        assert!(guard.ready(), "{case}: 独立 socket に shell の session を立てられる");
        if case == "input-busy" {
            assert!(tmux(&place.socket, &["send-keys", "-t", &target, "-l", "git st"]).status.success());
            assert!(acct_wait_pane(&place, &target, |pane| pane.trim_end().ends_with("$ git st")), "打ちかけが描かれる");
        }
        let aimed = if case == "session-missing" { "nosuch:seat" } else { target.as_str() };

        let out = launch_run(&place, &path, aimed, extra);

        let line = stderr_of(&out);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{case}: stdout={}", stdout_of(&out));
        assert!(stdout_of(&out).is_empty(), "{case}: stdout は空");
        assert!(line.starts_with("seat launch: refused reason="), "{case}: {line}");
        assert_eq!(tick_token(&line, "reason").as_deref(), Some(case), "{case}: {line}");
        if case == "no-account" {
            assert_eq!(tick_token(&line, "detail").as_deref(), Some("unmeasured"), "{line}");
        }
        launch_assert_not_sent(&place, rows, case);
        assert!(!tmux(&place.socket, &["has-session", "-t", "=nosuch"]).status.success(), "{case}: session を作らない");
        drop(guard);
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// (e) `--restore /rebrief` は立ち上がり（`SessionStart` の打刻）の**後**に 1 回届く: 偽 claude の受けた行は `/rebrief` だけ・
/// 打刻の順は SessionStart → UserPromptSubmit・席の記録は 起動行 → `/rebrief` の 2 行・成立の行に `consumed=true`。
#[test]
fn seat_launch_restore_is_sent_once_after_session_start() {
    let place = launch_place();
    let name = "launchrestore";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let out = launch_run(&place, &path, &target, &["--account", "l1", "--restore", "/rebrief"]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={} pane={}", stderr_of(&out), capture(&place.socket, &target));
    assert!(line.starts_with(&format!("seat launch: launched target={name}_{name} account=l1 consumed=true ")), "{line}");
    assert_eq!(fs::read_to_string(place.dir.join(LAUNCH_LOG)).unwrap_or_default(), "/rebrief\n", "復元は 1 回だけ届く");
    let stamps = fs::read_to_string(state_file(&seat_dir_of(&place.state, &format!("{name}_{name}")))).unwrap_or_default();
    let events: Vec<String> = stamps.lines().filter_map(|found| acct_text(found, "event")).collect();
    assert_eq!(events, ["SessionStart", "UserPromptSubmit", "Stop"], "復元は立ち上がりの後: {stamps}");
    let sent = acct_sent(&place.state, &format!("{name}_{name}"));
    assert_eq!(sent.len(), 2, "起動行と復元の 2 行: {sent:?}");
    assert!(sent.first().is_some_and(|what| what.starts_with("CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR=")), "{sent:?}");
    assert_eq!(sent.get(1).map(String::as_str), Some("/rebrief"), "{sent:?}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// 送った行のうち `--model` の語の数（起動行が model を**ちょうど 1 つ**運ぶことの計測）。
fn launch_model_words(line: &str) -> usize {
    line.split(' ').filter(|word| *word == "--model").count()
}

/// (f) 起動行は `--model` を typed に運ぶ（`s2-07l.313`・C2.2 / C10・SRS FR59 / FR36）: `--model Fable`（表示名）で起こすと偽
/// claude の argv は `--model fable`（別名）で**始まり**（`claude` の直後・anchor の `--plugin-dir` より前）、送った行に
/// `--model` の語は 1 つ・登録 row は `model=Fable` を持ち **`launch`（雛形）に `--model` の語は無い**（model 無しの導出行のまま）。
/// 表に無い `--model nope` は `launch-model-unknown` で row も key も書かない。base は `--model` を運ばず黙って settings の
/// model で立てる（RED）。
#[test]
fn seat_launch_carries_the_model_alias_in_the_launch_line() {
    let place = launch_place();
    let name = "launchmodel";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let unknown = launch_run(&place, &path, &target, &["--account", "l2", "--model", "nope"]);
    assert_eq!(rc_of(&unknown), i32::from(RC_REFUSED), "stdout={}", stdout_of(&unknown));
    assert_eq!(tick_token(&stderr_of(&unknown), "reason").as_deref(), Some("launch-model-unknown"), "{}", stderr_of(&unknown));
    launch_assert_not_sent(&place, 0, "model-unknown");

    let out = launch_run(&place, &path, &target, &["--account", "l2", "--model", "Fable"]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("--model\nfable\n{}", launch_expected_argv(&place, "l2")),
        "別名の `--model fable` が `claude` の直後に 1 回だけ届く"
    );
    let sent = acct_sent(&place.state, &format!("{name}_{name}"));
    assert_eq!(sent.len(), 1, "起動行の 1 行: {sent:?}");
    assert!(sent.first().is_some_and(|what| what.contains(" claude --model fable --plugin-dir ") && launch_model_words(what) == 1), "{sent:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows.first().map(|row| row.model.as_deref()), Some(Some("Fable")), "row の model は表示名のまま: {rows:?}");
    assert_eq!(rows.first().map(|row| row.launch.as_str()), Some(launch_derived(&place).as_str()), "雛形は model 無しの導出行: {rows:?}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) 雛形に literal の `--model` が在る（host の面の `[[launch-arg]]` に `--model` / `opus`＝暫定の再登録の形）周に `--model Fable`
/// で起こすと、器の 1 つと二重になるので後勝ちにせず `launch-model-duplicated` で断る（row も key も書かない）。`--model` 無しなら
/// literal の 1 つだけが載って起こせる（二重**だけ**を断る）。
#[test]
fn seat_launch_refuses_a_duplicated_model_in_the_template() {
    let place = launch_place();
    let name = "launchdup";
    let target = format!("{name}:{name}");
    let host = place.state.join(vessel::rules::HOST_MANIFEST);
    let literal = format!("{}\n[[launch-arg]]\nvalue = \"--model\"\n\n[[launch-arg]]\nvalue = \"opus\"\n", fs::read_to_string(&host).unwrap_or_default());
    fs::write(&host, literal).ok();
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let out = launch_run(&place, &path, &target, &["--account", "l2", "--model", "Fable"]);

    let line = stderr_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(tick_token(&line, "reason").as_deref(), Some("launch-model-duplicated"), "{line}");
    launch_assert_not_sent(&place, 0, "model-duplicated");

    let single = launch_run(&place, &path, &target, &["--account", "l2"]);
    assert_eq!(rc_of(&single), i32::from(RC_OK), "stdout={} stderr={}", stdout_of(&single), stderr_of(&single));
    let argv = fs::read_to_string(place.dir.join("launched")).unwrap_or_default();
    assert_eq!(argv, launch_expected_argv(&place, "l2").replacen("\nenv:", "\n--model\nopus\nenv:", 1), "literal の 1 つだけが載る: {argv}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (f) `--model Fable` で起こした席の立て直しは同じ model を運ぶ（`s2-07l.313`・row の `launch` は model 無しの雛形・model は
/// row の `model` の 1 か所）: 偽 claude を `seat launch --model Fable --account l1` で起こし → 退避の合図（l1 = 100）→ `Stop` →
/// 偽 claude を終えて shell へ戻す → tick が l2 で立て直す。立て直しの起動行は `claude --model fable` を**ちょうど 1 つ**持ち
/// （`launch-model-duplicated` にならない）、偽 claude の 2 回目の argv も `--model fable` で始まり、写した row の `launch` に
/// `--model` の語は無い。run 1 の実装（雛形に `--model` を書く）は二重で断る（RED）。
#[test]
fn seat_account_relaunch_keeps_the_model_of_a_seat_launched_with_model() {
    let place = launch_place();
    let name = "launchkeep";
    let target = format!("{name}:{name}");
    let seat = format!("{name}_{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let launched = launch_run(&place, &path, &target, &["--account", "l1", "--model", "Fable"]);
    assert_eq!(rc_of(&launched), i32::from(RC_OK), "stdout={} stderr={}", stdout_of(&launched), stderr_of(&launched));
    acct_measured(&place.state, "l1", 100, &acct_now());
    acct_measured(&place.state, "l2", 30, &acct_now());
    write_state(&seat_dir_of(&place.state, &seat), StateFix::Busy { age_s: 0 });
    let first = stdout_of(&acct_tick(&place, &target, None));
    assert_eq!(tick_token(&first, "kind").as_deref(), Some("externalize"), "1 周目は退避の合図: {first}");
    acct_stop(&place, &seat, unix_now().saturating_add(1));
    assert!(tmux(&place.socket, &["send-keys", "-t", &target, "C-d"]).status.success(), "偽 claude を終える");
    assert!(acct_shell_prompt(&place, &target, ""), "席の終了後の pane は shell の prompt で終わる");

    let out = acct_tick(&place, &target, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("account", "l1:100"), ("relaunch", "l2")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("--model\nfable\n{}--model\nfable\n{}", launch_expected_argv(&place, "l1"), launch_expected_argv(&place, "l2")),
        "起動と立て直しの argv はどちらも `--model fable` で始まる"
    );
    let sent = acct_sent(&place.state, &seat);
    assert_eq!(sent.len(), 4, "起動・退避の合図・立て直し・復元の 4 行: {sent:?}");
    let l2_dir = place.state.join("accounts").join("l2").display().to_string();
    assert!(
        sent.get(2).is_some_and(|what| what.starts_with(&format!("CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={l2_dir} claude --model fable --plugin-dir ")) && launch_model_words(what) == 1),
        "立て直しの起動行は model を 1 つ運ぶ: {sent:?}"
    );
    assert_eq!(sent.get(3).map(String::as_str), Some("/rebrief"), "{sent:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "SeatRegistered は 1 件増える: {rows:?}");
    assert!(rows.iter().all(|row| row.model.as_deref() == Some("Fable") && launch_model_words(&row.launch) == 0), "雛形に `--model` は無い: {rows:?}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}
