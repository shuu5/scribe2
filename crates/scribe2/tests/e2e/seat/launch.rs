//! 席の起動と復元の歯（Enter 落ちの修復と記録の席の列・復元の第 2 手・`seat launch`・設計
//! docs/design/seat-roles.md §7 / account-lifecycle.md §4 / account-autonomy.md §5・接頭辞 `seat_attrib_` /
//! `seat_restore_` / `seat_launch_`）。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。
//! 歯の本文は `seat/account.rs` から**挙動不変で移した**もの（`s2-07l.361`・seat-roles.md §7 の固定した組）。
// flip-check: moved s2-07l.361

use super::*;

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

/// (2) 同じ席で tick を 2 回撃つと、2 周目は入力欄の門が断る（`decision=error reason=inject-input-own-queued`・
/// rc 1）。**`pointer-recent` ではない**＝1 周目が stamp を打っていないことがここで測れる（base は 1 周目で
/// 自打刻するので 2 周目が `decision=noop reason=pointer-recent`・RED）。
///
/// 2 周目の字面は入力欄の門が 3 値になって `inject-busy` から変わった（`s2-07l.288`）: 入力欄に残るのは
/// **器自身が送った目印**（1 周目の記録が `tick.jsonl` に在る）なので Foreign ではなく OwnQueued で、この席は
/// Enter を submit にしない＝Enter 1 回の後も残るので `input-own-queued` で断る。attrib の写像は既存の 1 本
/// （`seat/tick/render.rs` が deliver の断りを `inject-<reason>` へ写す）をそのまま通る。
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
        format!("seat: tick decision=error reason=inject-input-own-queued{CTX_10}{ST_IDLE}{}\n", provenance(&state, "flag")),
        "入力欄に目印が残る席は入力欄の門が名乗る（黙った pointer-recent にならない）"
    );
    assert!(!tick_stamp_of(&dir, name).exists(), "2 周とも tick-stamp を打たない");
    assert_eq!(capture(&socket, name).matches(ATTRIB_POINTER).count(), 1, "2 周目は text を再送しない（Enter 1 回だけ）");
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

// ─────────────────── 席の起動（account-lifecycle.md §4・ADR-0026 §2.3・`s2-07l.244`・接頭辞 `seat_launch_`） ───────────────────

/// 期待する導出行（穴を埋める前・`launch` の row に載る形）。
fn launch_derived(place: &AcctPlace) -> String {
    format!(
        "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={{account_dir}} claude --plugin-dir {} --plugin-dir {} --plugin-dir {} {} {}",
        launch_anchor(place), LAUNCH_PLUGINS[0], LAUNCH_PLUGINS[1], LAUNCH_ARGS[0], LAUNCH_ARGS[1]
    )
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
    assert!(sent.first().is_some_and(|what| what.starts_with(&acct_launch_prefix(&launch_anchor(&place), "CLAUDE_CONFIG_DIR="))), "{sent:?}");
    assert_eq!(sent.get(1).map(String::as_str), Some("/rebrief"), "{sent:?}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (g) 起動行は登録 row の anchor への `cd` を先頭に前置する（`s2-07l.324`・account-lifecycle.md §4・C3「真実は row の anchor」）:
/// pane の shell の cwd が anchor でない（host の再起動後の復元で home に戻った pane の形）まま `seat launch --anchor <dir>` で
/// 起こすと、席の記録の起動行はちょうど `cd '<anchor>' && CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR=<l2> claude …`
/// （cd → agent view の env → claude の順）で、効果でも測る: 起こした偽 claude の cwd（pane の前面 process の cwd）は anchor。
/// 登録 row の `launch`（雛形）と偽 claude の argv に `cd` は載らない（雛形不変・`--plugin-dir` の列も不変）。base は pane の
/// cwd のまま起こす（RED）。
#[test]
fn seat_launch_injects_cd_to_the_row_anchor_before_the_line() {
    let place = launch_place();
    let name = "launchcd";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let anchor = launch_anchor(&place);
    let elsewhere = place.dir.display().to_string();
    assert!(tmux(&place.socket, &["send-keys", "-t", &target, "-l", &format!("cd '{elsewhere}'")]).status.success());
    assert!(tmux(&place.socket, &["send-keys", "-t", &target, "Enter"]).status.success());
    let cwd_of = || stdout_of(&tmux(&place.socket, &["display-message", "-p", "-t", &target, "#{pane_current_path}"])).trim_end().to_owned();
    assert!(
        acct_wait_pane(&place, &target, |pane| pane.trim_end().ends_with('$') && cwd_of() == elsewhere),
        "前提: 起こす前の pane の cwd は anchor ではない: {}",
        cwd_of()
    );

    let out = launch_run(&place, &path, &target, &["--account", "l2"]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={} pane={}", stderr_of(&out), capture(&place.socket, &target));
    let l2_dir = place.state.join("accounts").join("l2").display().to_string();
    let sent = acct_sent(&place.state, &format!("{name}_{name}"));
    assert_eq!(
        sent,
        vec![format!("cd '{anchor}' && {}", launch_derived(&place).replace("{account_dir}", &l2_dir))],
        "起動行は row の anchor への cd → agent view の env → claude の順の 1 行"
    );
    assert_eq!(cwd_of(), anchor, "起こした席の cwd は row の anchor（pane の cwd ではない）");
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_expected_argv(&place, "l2"), "argv に cd は載らない");
    launch_assert_registered_before_send(&place, &target, "l2");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
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
