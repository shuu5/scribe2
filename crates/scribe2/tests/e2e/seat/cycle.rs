//! 席の注入と作り直しと終了の歯（`seat inject` / `seat cycle`・終了の手・立て直しの shell の門・
//! 設計 docs/design/seat-autonomy.md §3 / account-autonomy.md §5・接頭辞 `seat_inject_` / `seat_cycle_` /
//! `seat_exit_` / `seat_relaunch_`）。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。
//! 歯の本文は `seat.rs` から**挙動不変で移した**もの（`s2-07l.261`）。
// flip-check: moved s2-07l.261

use super::*;

// ─────────────────────────── inject ───────────────────────────

/// 送達すると tick.jsonl へ FR21 と同じ schema の 1 行が残る。
#[test]
fn seat_inject_delivers_and_records_on_isolated_socket() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatdeliver";
    // 実際の tmux target の形（`<session>:<window>.<pane>`）で撃つ。`:` と `.` を含むので
    // **記録の dir 名と表示は潰した字面**になり、潰しが効いていることが測れる。
    let target = "seatdeliver:0.0";
    let sanitized = "seatdeliver_0.0";
    let state = dir.join("state");
    // hook を持つ席の形: 打刻 file が在り、受けた行ごとに `UserPromptSubmit` を打つ（消費の証拠）。
    stamp_idle(&state, sanitized);
    let guard = start_clearing_seat(
        &socket, name, &dir.join("seat.log"), &state_file(&seat_dir_of(&state, sanitized)), (FakeStamp::Never, FakeStamp::Now),
    );
    assert!(guard.ready(), "独立 socket に打刻する席を立てられる");
    let marker = "seat-e2e-delivered";
    let payload = format!("echo {marker}");

    let out = run_seat(&[
        "inject",
        "--target",
        target,
        "--tmux-socket",
        &socket,
        "--state-dir",
        &state.display().to_string(),
        "--text",
        &payload,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!(
            "seat: inject delivered target={sanitized} bytes={} consumed=true{}\n",
            payload.len(),
            provenance(&state, "flag")
        ),
        "送達 ts 以後に UserPromptSubmit の打刻が足された周は consumed=true"
    );
    let pane = capture(&socket, target);
    assert!(pane.contains(marker), "pane に marker が現れる: {pane}");

    let recorded = fs::read_to_string(tick_file(&state, sanitized)).unwrap_or_default();
    let lines: Vec<&str> = recorded.lines().collect();
    assert_eq!(lines.len(), 1, "記録は 1 行: {recorded}");
    let line = lines.first().copied().unwrap_or_default();
    assert!(line.contains(r#""schema":1"#), "{line}");
    assert!(line.contains(r#""who":"seat-inject""#), "{line}");
    assert!(line.contains(r#""when":"inject""#), "{line}");
    assert!(line.contains(r#""tokens":null"#), "数えていない値は null: {line}");
    assert!(
        line.contains(&format!(r#""what":"{payload}""#)),
        "何を送ったかを残す（現物のまま・consumed は行の側）: {line}"
    );
    assert!(
        line.contains(&format!(r#""bytes":{}"#, payload.len())),
        "byte 数は payload の byte 長: {line}"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// inject の表示行は末尾に出所と置き場を持つが、**記録の what は payload の先頭のまま**
/// （FR21 と schema を共有・planner 裁定 2026-09-11）＝2 語を含まない負例まで測る。
#[test]
fn seat_inject_delivered_line_carries_state_dir_provenance() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatinjprov";
    let state = dir.join("state");
    stamp_idle(&state, name);
    let guard = start_clearing_seat(
        &socket, name, &dir.join("seat.log"), &state_file(&seat_dir_of(&state, name)), (FakeStamp::Never, FakeStamp::Now),
    );
    assert!(guard.ready(), "独立 socket に打刻する席を立てられる");
    let payload = "echo seat-e2e-prov";

    let out = run_seat(&[
        "inject", "--target", name, "--tmux-socket", &socket,
        "--state-dir", &state.display().to_string(), "--text", payload,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!(
            "seat: inject delivered target={name} bytes={} consumed=true{}\n",
            payload.len(),
            provenance(&state, "flag")
        ),
        "表示行の末尾に出所と置き場"
    );
    let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
    assert_eq!(recorded.lines().count(), 1, "記録は 1 行: {recorded}");
    assert!(recorded.contains(&format!(r#""what":"{payload}""#)), "what は payload の先頭のまま: {recorded}");
    assert!(
        !recorded.contains(" source=") && !recorded.contains(" state_dir="),
        "記録の what に 2 語を足さない（加工しない）: {recorded}"
    );
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 入力欄が非空の周は **1 key も送らない**（人間の打ちかけと merge しない）。
#[test]
fn seat_inject_refuses_when_input_line_is_busy() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-busy";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    let marker = "seat-e2e-busy";
    tmux(&socket, &["send-keys", "-t", name, "-l", "partial"]);
    sleep(Duration::from_millis(200));
    assert!(
        capture(&socket, name).contains("❯ partial"),
        "打ちかけが入力欄に描かれている"
    );

    let out = run_seat(&[
        "inject",
        "--target",
        name,
        "--tmux-socket",
        &socket,
        "--state-dir",
        &state.display().to_string(),
        "--text",
        &format!("echo {marker}"),
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stdout_of(&out), "", "断りの周は stdout 0 行");
    assert_eq!(stderr_of(&out), format!("seat: inject refused reason=busy{}\n", provenance(&state, "flag")));
    let pane = capture(&socket, name);
    assert!(!pane.contains(marker), "marker は 1 度も現れない: {pane}");
    assert!(
        pane.contains("❯ partial"),
        "打ちかけはそのまま残る（Enter も送っていない）: {pane}"
    );
    assert!(
        !tick_file(&state, name).exists(),
        "送っていない周は記録も書かない"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// tmux を撃てない周は fail-closed（1 byte も書かない）。
#[test]
fn seat_inject_fails_closed_when_tmux_target_is_unreachable() {
    let dir = tmp();
    let socket = dir.join("absent-sock").display().to_string();
    let state = dir.join("state");
    let name = "seat-absent";

    let out = run_seat(&[
        "inject",
        "--target",
        name,
        "--tmux-socket",
        &socket,
        "--state-dir",
        &state.display().to_string(),
        "--text",
        "echo seat-e2e-absent",
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stdout_of(&out), "");
    assert_eq!(
        stderr_of(&out),
        format!("seat: inject unconfirmed reason=tmux-failed{}\n", provenance(&state, "flag"))
    );
    assert!(!state.exists(), "1 byte も書かない");
    fs::remove_dir_all(&dir).ok();
}

/// 送った字面が pane に現れたが入力欄が空にならない周（席が busy で注入が queue された形）は
/// **送達成功**で、消費の証拠（送達 ts 以後の `UserPromptSubmit` の打刻）が窓の内に来ない周は
/// `consumed=false` として行に添える（queue＝次 turn で消費される・記録は現物のまま・`s2-07l.112`）。
///
/// 旧実装はこれを `unconfirmed reason=residual`（rc 1）にしていたが、その注入は席に届いて
/// turn の終わりに消費されていた（実測 2026-09-11: tick.jsonl の `inject-residual` 9 件が全部
/// 送達済み・bd `s2-07l.90`）。失敗として残すのは `absent` と `tmux-failed` の 2 つだけ。
/// 引用の閉じない payload は sh が継続 prompt へ移り、prompt 行の右に打鍵が残る＝同じ fixture。
#[test]
fn seat_inject_counts_queued_delivery_as_success_with_consumed_false() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-queued";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    // 打刻 file は在る（hook を持つ席）が、`sh -i` は受けた行で打刻しない＝新しい打刻が来ない形。
    stamp_idle(&state, name);
    let payload = ": seat-e2e-queued";

    let out = run_seat(&[
        "inject",
        "--target",
        name,
        "--tmux-socket",
        &socket,
        "--state-dir",
        &state.display().to_string(),
        "--text",
        payload,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!(
            "seat: inject delivered target={name} bytes={} consumed=false{}\n",
            payload.len(),
            provenance(&state, "flag")
        ),
        "現れた ＝ 送達成功・消費の打刻が来ない周は consumed=false"
    );
    assert_eq!(stderr_of(&out), "", "成功の周は stderr 0 行");
    let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
    let lines: Vec<&str> = recorded.lines().collect();
    assert_eq!(lines.len(), 1, "送達した周は 1 行を記録する: {recorded}");
    let line = lines.first().copied().unwrap_or_default();
    assert!(line.contains(r#""who":"seat-inject""#), "{line}");
    assert!(
        line.contains(&format!(r#""what":"{payload}""#)),
        "送達した周は現物を記録する（consumed は stdout 行と tick 行の側）: {line}"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 送った字面が pane に**現れない**周（`absent`）は引き続き失敗（rc 1・記録しない）＝
/// 極性は「現れた」の側だけ緩め、「現れない」は緩めない。
///
/// 席の echo を切る（`stty -echo`）と、送った字面は入力欄にも出力にも現れない（`:` は何も
/// 出さない）。
#[test]
fn seat_inject_still_fails_when_payload_never_appears() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-absent";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    tmux(&socket, &["send-keys", "-t", name, "-l", "stty -echo"]);
    tmux(&socket, &["send-keys", "-t", name, "Enter"]);
    assert!(wait_prompt(&socket, name), "echo を切った後に prompt が戻る");
    let state = dir.join("state");

    let out = run_seat(&[
        "inject",
        "--target",
        name,
        "--tmux-socket",
        &socket,
        "--state-dir",
        &state.display().to_string(),
        "--text",
        ": seat-e2e-absent",
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stdout_of(&out), "", "送達を確認できない周は stdout 0 行");
    assert_eq!(stderr_of(&out), format!("seat: inject unconfirmed reason=absent{}\n", provenance(&state, "flag")));
    assert!(
        !tick_file(&state, name).exists(),
        "送達を確認できない周は記録しない"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 折り返しの歯の pane 幅（列）。
const WRAP_WIDTH: &str = "40";

/// 折り返しの歯の目印: pane 幅より長い 1 行（非 ASCII を含む・全角は 2 列）。
const WRAP_MARKER: &str = ": seat-e2e-wrapped 折り返しの目印は幅を越えて次の行へ割れる";

/// 折り返しを結合した pane 本文（歯の側で「論理行に目印が在る / 無い」を測る）。
fn capture_joined(socket: &str, target: &str) -> String {
    String::from_utf8_lossy(&tmux(socket, &["capture-pane", "-p", "-J", "-t", target]).stdout).into_owned()
}

/// 狭い pane に `inject` を 1 回撃つ。
fn inject_on(socket: &str, name: &str, state: &Path, payload: &str) -> Output {
    run_seat(&[
        "inject", "--target", name, "--tmux-socket", socket,
        "--state-dir", &state.display().to_string(), "--text", payload,
    ])
}

/// pane 幅より長い 1 行は端末で折り返され、結合しない capture では目印が割れる。それでも
/// **届いた注入は `delivered` rc 0・記録 1 行**（`s2-07l.148`・base では `absent` rc 1）。
#[test]
fn seat_inject_wrapped_long_line_is_delivered_and_recorded() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-wrapped";
    let guard = start_seat_sized(&socket, name, "PS1=❯ ", PROMPT, WRAP_WIDTH);
    assert!(guard.ready(), "独立 socket に狭い pane の session を立てられる");
    let state = dir.join("state");
    stamp_idle(&state, name);

    let out = inject_on(&socket, name, &state, WRAP_MARKER);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!(
            "seat: inject delivered target={name} bytes={} consumed=false{}\n",
            WRAP_MARKER.len(),
            provenance(&state, "flag")
        ),
        "折り返された目印も現れた＝送達"
    );
    // 歯が空虚でないこと: 結合しない capture では目印が割れて当たらず、結合すると当たる。
    let split = capture(&socket, name);
    assert!(!split.contains(WRAP_MARKER), "結合しない capture では目印が割れている: {split}");
    let joined = capture_joined(&socket, name);
    assert!(joined.contains(WRAP_MARKER), "結合した論理行には目印が在る: {joined}");
    let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
    assert_eq!(recorded.lines().count(), 1, "記録は 1 行: {recorded}");
    assert!(recorded.contains(r#""who":"seat-inject""#), "{recorded}");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 狭い pane でも幅より短い 1 行は現物どおり `delivered` rc 0（結合しても短い行は変わらない）。
#[test]
fn seat_inject_wrapped_short_line_on_narrow_pane_is_delivered() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-wrapshort";
    let guard = start_seat_sized(&socket, name, "PS1=❯ ", PROMPT, WRAP_WIDTH);
    assert!(guard.ready(), "独立 socket に狭い pane の session を立てられる");
    let state = dir.join("state");
    stamp_idle(&state, name);
    let payload = ": seat-e2e-short";

    let out = inject_on(&socket, name, &state, payload);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!(
            "seat: inject delivered target={name} bytes={} consumed=false{}\n",
            payload.len(),
            provenance(&state, "flag")
        )
    );
    assert!(capture(&socket, name).contains(payload), "短い行は割れずに現れる");
    let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
    assert_eq!(recorded.lines().count(), 1, "記録は 1 行: {recorded}");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 結合して読んでも、**本当に届いていない周は `absent` rc 1 のまま**（極性不変）。echo を切った
/// 狭い pane には別の字面（`stty -echo`）だけが在り、目印は結合した論理行にも無い——門の断り
/// （busy / unknown-input）や tmux-failed で「別の理由で」落ちていないことを行の字面で確かめる。
#[test]
fn seat_inject_wrapped_absent_line_still_fails_closed() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-wrapabsent";
    let guard = start_seat_sized(&socket, name, "PS1=❯ ", PROMPT, WRAP_WIDTH);
    assert!(guard.ready(), "独立 socket に狭い pane の session を立てられる");
    tmux(&socket, &["send-keys", "-t", name, "-l", "stty -echo"]);
    tmux(&socket, &["send-keys", "-t", name, "Enter"]);
    assert!(wait_prompt(&socket, name), "echo を切った後に prompt が戻る");
    let state = dir.join("state");

    let out = inject_on(&socket, name, &state, WRAP_MARKER);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(stdout_of(&out), "", "送達を確認できない周は stdout 0 行");
    assert_eq!(stderr_of(&out), format!("seat: inject unconfirmed reason=absent{}\n", provenance(&state, "flag")));
    let joined = capture_joined(&socket, name);
    assert!(joined.contains("stty -echo"), "pane には別の字面が在る: {joined}");
    assert!(!joined.contains("seat-e2e-wrapped"), "目印は結合した論理行にも無い: {joined}");
    assert!(!tick_file(&state, name).exists(), "送達を確認できない周は記録しない");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// prompt 行を特定できない pane へは **1 key も送らない**（fail-closed）。
#[test]
fn seat_inject_refuses_when_prompt_is_not_locatable() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-noprompt";
    // `❯` を持たない席（入力欄の位置が読めない）。
    let guard = start_seat_with(&socket, name, "PS1=$ ", '$');
    assert!(
        guard.ready(),
        "独立 socket に prompt の字が違う session を立てられる"
    );
    let state = dir.join("state");
    let marker = "seat-e2e-noprompt";

    let out = run_seat(&[
        "inject",
        "--target",
        name,
        "--tmux-socket",
        &socket,
        "--state-dir",
        &state.display().to_string(),
        "--text",
        &format!("echo {marker}"),
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stdout_of(&out), "");
    assert_eq!(
        stderr_of(&out),
        format!("seat: inject refused reason=unknown-input{}\n", provenance(&state, "flag"))
    );
    let pane = capture(&socket, name);
    assert!(!pane.contains(marker), "marker は 1 度も現れない: {pane}");
    assert!(!tick_file(&state, name).exists(), "記録も書かない");
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 空の payload は使い方の誤りとして断る（空の口を「送った」に化けさせない）。
#[test]
fn seat_inject_refuses_empty_payload() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-empty";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    let before = capture(&socket, name);

    let out = run_seat(&[
        "inject",
        "--target",
        name,
        "--tmux-socket",
        &socket,
        "--state-dir",
        &state.display().to_string(),
        "--text",
        "   ",
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stdout_of(&out), "");
    assert!(
        stderr_of(&out).starts_with("usage: seat "),
        "使い方で断る: {}",
        stderr_of(&out)
    );
    assert_eq!(capture(&socket, name), before, "pane は 1 文字も変わらない");
    assert!(!state.exists(), "記録も書かない");
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// `.` / `..` の target は記録 dir 名として潰し、**state dir の外へ出さない**。
///
/// 期待する path は契約の字面から自分で組み、`tick_path` の戻りと突き合わせる
/// （実装の潰し方を歯が呼んで期待値を作ると、壊れても同じ壊れ方をして空虚になる）。
#[test]
fn seat_inject_sanitizes_dot_targets() {
    let dir = tmp();
    let state = dir.join("state");
    let seat_dir = state.join("seat");

    let dotdot = tick_path(&state, "..");
    assert_eq!(
        dotdot,
        seat_dir.join("__").join("tick.jsonl"),
        ".. は同じ長さの _ へ潰す"
    );
    assert!(dotdot.starts_with(&seat_dir), "state dir の外へ出ない: {dotdot:?}");

    let dot = tick_path(&state, ".");
    assert_eq!(dot, seat_dir.join("_").join("tick.jsonl"), ". も潰す");
    assert!(dot.starts_with(&seat_dir), "state dir の外へ出ない: {dot:?}");

    // 多段は従来どおり（`/` が `_` に潰れるので component を跨がない）。
    assert_eq!(
        tick_path(&state, "../x"),
        seat_dir.join(".._x").join("tick.jsonl"),
        "多段は 1 component のまま"
    );
    fs::remove_dir_all(&dir).ok();
}

// ─────────────────── cycle ───────────────────

/// prompt を 1 行も持たない pane（入力欄の位置が読めない）。
const NO_PROMPT: &str = "$ \n  10% 100k/1M Opus 5\n";
/// 入力欄に打ちかけが在る pane（送ると 1 行に merge する形）。
const INPUT_BUSY_PANE: &str = "❯ typing\n  10% 100k/1M Opus 5\n";
/// `/clear` を打ったが **submit されていない**実席（2026-09-11・匿名化済み）: 字面は入力行
/// そのものに残り（`input_tail` が非空）、slash command の候補が上に描かれる。
///
/// 消費済みの echo と同じ `❯ /clear` の字面だが、**最後の prompt 行**に在る＝作り直しの
/// 証拠ではない（Enter だけが落ちた周の形・実測 2026-09-11 planner 席）。
const UNSUBMITTED_CLEAR_PANE: &str = concat!(
    " ▐▛███▛█   Claude Code v2.1.268\n",
    "▝▜██████▀  Fable 5.1 with high effort · Claude Max\n",
    "  ▝▝ ▝▝    /…/repo · /rc\n",
    "  /clear                             Start a new session with empty context; previous\n",
    "                                     session stays on disk (resumable with /resume)\n",
    "  /ready-compaction                  context\n",
    "                                     cycle（/clear・respawn）の前に、失うと困る「命令…\n",
    "────────────────────────────────────────\n",
    "❯\u{a0} /clear\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  repo\n",
    "  Fable 5.1 [high] 5h:41%(2h8m) 7d:21%(6d5h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle)\n",
);
/// 作り直された席に **もう 1 度** `/clear` を打って submit していない実席（2026-09-11・匿名化
/// 済み）: 上に消費済みの echo、入力行にも `/clear`。echo が在っても入力行が非空なら作り直しの
/// 確認は立たない（正の証拠は入力欄が空のときだけ効く）。
const REBUILT_UNSUBMITTED_PANE: &str = concat!(
    " ▐▛███▛█   Claude Code v2.1.268\n",
    "▝▜██████▀  Fable 5.1 with high effort · Claude Max\n",
    "  ▝▝ ▝▝    /…/repo · /rc\n",
    "❯ /clear\n",
    "  /clear                             Start a new session with empty context; previous\n",
    "                                     session stays on disk (resumable with /resume)\n",
    "────────────────────────────────────────\n",
    "❯\u{a0} /clear\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  repo\n",
    "  Fable 5.1 [high] 5h:41%(2h8m) 7d:21%(6d5h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle)\n",
);
/// 作り直された直後に **hook の出力が echo の下に描かれる**形（本便の A/B で実席は
/// `Updated to latest` 2 行 + hook 2 行を描いた・版で行数が増えうる）。echo は入力行の
/// 10 非空行上＝裁定 (e) の上 6 非空行の**外**に在る。域を 6 行に絞る実装はこの pane を
/// 確認できず、`.94` と同じ行き止まり（復元を送らない）へ戻る。
const REBUILT_HOOKS_PANE: &str = concat!(
    " ▐▛███▛█   Claude Code v2.1.268\n",
    "▝▜██████▀  Fable 5.1 with high effort · Claude Max\n",
    "  ▝▝ ▝▝    /…/repo · /rc\n",
    "❯ /clear\n",
    "  ⎿  SessionStart:clear hook success: served version=2\n",
    "  ⎿  SessionStart:clear hook success: [LOCATION] host / cwd / branch\n",
    "  ⎿  SessionStart:clear hook success: [bd prime] workflow context\n",
    "  ⎿  SessionStart:clear hook success: lint report 0 errors\n",
    "  ⎿  SessionStart:clear hook success: lint report 0 errors\n",
    "  ⎿  SessionStart:clear hook success: memory index loaded\n",
    "  ⎿  SessionStart:clear hook success: fetch done\n",
    "  ⎿  SessionStart:clear hook success: statusline ready\n",
    "────────────────────────────────────────\n",
    "❯\u{a0} \n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  repo\n",
    "  Fable 5.1 [high] 5h:41%(2h8m) 7d:21%(6d5h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// submit 済みの user 発言が `/clear` **で始まる**が `/clear` ではない pane（作り直されて
/// いない）。発言の echo は行頭 `❯ …` に描かれるので、右側を「`/clear` で始まる」「`/clear` を
/// 含む」まで緩めた実装はこの pane を「済んだ」と読み、会話を捨てていない席へ復元を送る。
const PREFIXED_CLEAR_PANE: &str = concat!(
    "❯ /clear は不可逆なので、送る前に退避の完了を確かめてください\n",
    "● 承知しました。退避物の有無を先に見ます。\n",
    "────────────────────────────────────────\n",
    "❯\u{a0} \n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// idle な席の本文が echo の字面を**引用**している pane（作り直されていない）。assistant の
/// 本文は 2 桁字下げで描かれるので、引用の `❯ /clear` は行頭に来ない＝消費済みの echo と
/// 弁別できる。行頭の条件を外す実装はこの pane を「済んだ」と読み、復元を送ってしまう。
const QUOTED_CLEAR_PANE: &str = concat!(
    "● 作り直された席の pane は次の形になる:\n",
    "  ❯ /clear\n",
    "  ⎿  消費済みの echo が新しい prompt の直上に残る\n",
    "────────────────────────────────────────\n",
    "❯\u{a0} \n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// turn を終えた実席（2026-09-11・匿名化済み）: 完了行 `Crunched for …` は spinner の形
/// （`… (`）を持たない。これを busy と読むと tick は永久に noop になる（退行の歯）。
const IDLE_TALL_PANE: &str = concat!(
    "● 便の報告を送った。\n",
    "✻ Crunched for 10m 28s · done 12:41\n",
    "\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// `seat.cycle_lock_ttl_s` の宣言値。歯はこの値の**両側**を撃つ。
const TTL_S: u64 = 900;
/// `seat.cycle_settle_s` の宣言値（作り直しと復元の確認上限・秒）。
const SETTLE_S: u64 = 30;
/// `seat.cycle_poll_ms` の宣言値（確認を見に行く周期・ミリ秒）。
const POLL_MS: u64 = 500;
/// 証拠が**遅れて**来る側の歯の上限（秒）。席の `sleep 3` より長く取る（縮めると測る分岐が
/// 「窓の内に拾う」から「窓を越えた」へ変わる）。
const LATE_SETTLE_S: u64 = 6;

/// `/clear` を**消化も echo もしない**席を立てる（作り直しが起きていない席の形）。
///
/// `sh -i` に送ると tty の echo が `❯ /clear` を**行頭に**残し、実席の消費済み echo と同じ
/// 字面になる（実席ではその字面は「処理された」の証拠である）。ゆえに tty の echo を切り、
/// 受けた行は log にだけ残す。
fn start_swallowing_seat(socket: &str, name: &str, log: &Path) -> IsolatedSeat {
    let script = format!(
        "stty -echo; while :; do printf '❯ '; read -r line || exit 0; \
         printf '%s\\n' \"$line\" >> '{}'; printf '\\n'; done",
        log.display()
    );
    let mut seat = IsolatedSeat {
        socket: socket.to_owned(),
        name: name.to_owned(),
        ready: false,
    };
    let out = tmux(
        socket,
        &[
            "new-session", "-d", "-s", name, "-x", "120", "-y", "40", "sh", "-c", &script,
        ],
    );
    seat.ready = out.status.success() && wait_prompt(socket, name);
    seat
}

/// 退避物が無い席へは **1 key も送らない**（憲法 CON5 / SRS FR28）。
///
/// 退避物の判定は「名前の前置き ∧ `.md` ∧ `.consumed.md` でない ∧ frontmatter の `seat:` が
/// 一致」の全部で、どれか 1 つでも緩めると decoy が根拠に化ける。打刻が **Busy でも**
/// 理由が `wm-missing` になることで、手順 2（退避物）が 3（状態の門）より先だと測れる。
#[test]
fn seat_cycle_refuses_without_unconsumed_wm() {
    let target = "seatnowm";
    for case in &[
        GateCase { reason: "wm-missing", wm_seat: None, pane: Some(IDLE_PANE), broken_state: false,
                   stamp: StateFix::Busy { age_s: 0 } },
        GateCase { reason: "wm-missing", wm_seat: Some("other:seat"), pane: Some(IDLE_PANE), broken_state: false,
                   stamp: StateFix::Idle },
        GateCase { reason: "wm-unreadable", wm_seat: None, pane: Some(IDLE_PANE), broken_state: false,
                   stamp: StateFix::Idle },
    ] {
        let dir = tmp();
        let state = prepare_gate_case(&dir, case, target);
        assert_gate_case(&dir, case, target, &state);
        fs::remove_dir_all(&dir).ok();
    }
}

/// 退避物が在っても、pane を読めない・入力欄を特定できない・打ちかけ・置き場が壊れている周は
/// **1 key も送らない**（状態の門は `seat_state_cycle_refuses_unless_stamped_idle` が測る）。
#[test]
fn seat_cycle_refuses_at_each_gate_without_sending() {
    let target = "seatgate";
    for case in &[
        GateCase { reason: "pane-missing", wm_seat: Some("seatgate"), pane: None, broken_state: false,
                   stamp: StateFix::Idle },
        // 入力欄の位置が読めない pane へは送らない（注入と同じ門・fail-closed）。
        GateCase { reason: "input-unknown", wm_seat: Some("seatgate"), pane: Some(NO_PROMPT), broken_state: false,
                   stamp: StateFix::Idle },
        // 打ちかけ（入力欄が非空）へは送らない（人間の打鍵と 1 行に merge しない・注入と同じ門）。
        GateCase { reason: "input-busy", wm_seat: Some("seatgate"), pane: Some(INPUT_BUSY_PANE), broken_state: false,
                   stamp: StateFix::Idle },
        // 置き場が使えない周を `lock-held` と名乗ると、競合と故障を記録から分けられない。
        GateCase { reason: "state-dir", wm_seat: Some("seatgate"), pane: Some(IDLE_PANE), broken_state: true,
                   stamp: StateFix::Idle },
    ] {
        let dir = tmp();
        let state = prepare_gate_case(&dir, case, target);
        assert_gate_case(&dir, case, target, &state);
        fs::remove_dir_all(&dir).ok();
    }
}

/// live な lock は譲り、**失効した residue は取り直す**。
#[test]
fn seat_cycle_refuses_when_lock_is_live_and_reclaims_stale_lock() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatlock";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Now),
    );
    assert!(guard.ready(), "fake な席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.live.md", name);
    stamp_idle(&state, name);
    let seat = seat_dir_of(&state, name);
    fs::create_dir_all(&seat).ok();
    let lock = seat.join("cycle.lock");
    fs::write(&lock, "{\"pid\":1,\"deadline\":0}\n").ok();
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let args = [
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ];

    let out = run_seat(&args);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stderr_of(&out), format!("seat: cycle refused reason=lock-held{}\n", provenance(&state, "flag")));
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "", "1 key も送っていない");
    assert!(lock.exists(), "他の cycle の lock を消さない");

    // TTL（900 秒）の**内側**はまだ live＝譲る（境界の下側を測る）。
    backdate(&lock, TTL_S - 100);
    let out = run_seat(&args);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stderr_of(&out), format!("seat: cycle refused reason=lock-held{}\n", provenance(&state, "flag")));
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "", "まだ 1 key も送っていない");

    // TTL を超えた lock は residue＝取り直して進む。
    backdate(&lock, TTL_S + 100);
    let out = run_seat(&args);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("seat: cycle done target={name}{}\n", provenance(&state, "flag")));
    assert!(
        fs::read_to_string(&log).unwrap_or_default().contains("/clear"),
        "失効 lock は進行を止めない"
    );
    assert!(!lock.exists(), "済んだ lock は返す");
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// `/clear` の**後に**復元 command を送り、lock を返す。退避物そのものは動かさない。
///
/// 順序は pane の描画でなく**席が受け取った行**で測る（作り直した席では `/clear` の描画が
/// 消えるので、pane から順序を読むと「消えない席」でしか測れない歯になる）。
#[test]
fn seat_cycle_sends_clear_then_restore_in_order() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatcycle";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Now),
    );
    assert!(guard.ready(), "fake な席を立てられる");
    let wm = dir.join("wm");
    let parked = wm_file(&wm, "working-memory.parked.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("seat: cycle done target={name}{}\n", provenance(&state, "flag")));
    let received = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(received, "/clear\n/rebrief\n", "作り直しの後に復元を送る: {received}");
    assert!(
        !seat_dir_of(&state, name).join("cycle.lock").exists(),
        "済んだ lock は返す"
    );
    assert!(parked.exists(), "退避物は動かさない（consume は復元側の仕事）");
    let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
    assert!(recorded.contains(r#""who":"seat-cycle""#), "cycle を 1 行残す: {recorded}");
    assert!(
        recorded.contains(&format!(r#""what":"cycle done{}""#, provenance(&state, "flag"))),
        "記録の what にも 2 語: {recorded}"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 作り直しを確認できない席へは**復元を送らない**（`/clear` が通ったことにしない）。
///
/// 席は `/clear` を消化も echo もしない（[`start_swallowing_seat`]）＝消費済みの echo が
/// 1 度も現れない、作り直しが起きていない席の形である。確認を素通りさせる実装はここで落ちる。
#[test]
fn seat_cycle_reports_clear_unconfirmed_when_session_is_not_rebuilt() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatstuck";
    let log = dir.join("seat.log");
    let guard = start_swallowing_seat(&socket, name, &log);
    assert!(guard.ready(), "独立 socket に `/clear` を飲む席を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.stuck.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s, rules_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        fast_rules(&dir),
    );

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s, "--rules", &rules_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stderr_of(&out), format!("seat: cycle failed reason=clear-unconfirmed{}\n", provenance(&state, "flag")));
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n",
        "作り直しの注入自体は送っている・確認できない周に復元を送らない"
    );
    let pane = capture(&socket, name);
    assert!(!pane.contains("/rebrief"), "確認できない周に復元を送らない: {pane}");
    assert!(
        !seat_dir_of(&state, name).join("cycle.lock").exists(),
        "失敗した周も lock を返す"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 作り直しは確認できても、**復元の消費が確認できない**周は `restore-unconfirmed`。
///
/// 席は `/rebrief` の字面を描く（送達は成立）が `UserPromptSubmit` を打たない（[`FakeStamp::Never`]）
/// ＝消費の証拠が無い。復元の確認は作り直しの確認と同じ**上限まで待つ**（`s2-07l.97`）ので、
/// 2 s ではなく上限の後に失敗する（待つ時間が延びるのは失敗側だけ・成功条件は不変）。
#[test]
fn seat_cycle_reports_restore_unconfirmed_after_limit_when_seat_goes_silent() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatmute";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Never),
    );
    assert!(guard.ready(), "作り直し後に黙る席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.mute.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s, rules_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        fast_rules(&dir),
    );

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s, "--rules", &rules_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stderr_of(&out), format!("seat: cycle failed reason=restore-unconfirmed{}\n", provenance(&state, "flag")));
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "復元は送っている（消費の打刻が無いだけ）"
    );
    assert!(
        !seat_dir_of(&state, name).join("cycle.lock").exists(),
        "失敗した周も lock を返す"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 確認の刻み（上限・周期）は **rules 行が持つ**（値を code に焼かない・憲法 C5・`s2-07l.151`）:
/// 埋め込みは裁定の値（30 s / 500 ms）を返し、単位は秒とミリ秒である。**不発効の行と不在は
/// どちらも `None`**（値が在っても使わない・呼び側は `no-rule` で 1 key も送らずに断る）。
///
/// 焼いた定数へ戻す変異も、秒とミリ秒を入れ替える変異もここで落ちる。
#[test]
fn seat_cycle_settle_pace_comes_from_the_manifest_rows() {
    let embedded = Manifest::embedded().expect("埋め込み manifest を読める");
    assert_eq!(
        pace_of(&embedded),
        Some((Duration::from_secs(SETTLE_S), Duration::from_millis(POLL_MS))),
        "埋め込みは裁定の値を返す（上限 {SETTLE_S} s・周期 {POLL_MS} ms）"
    );
    let fast = Manifest::parse(&pace_manifest(FAST_SETTLE_S, FAST_POLL_MS, true))
        .expect("fixture を読める");
    assert_eq!(
        pace_of(&fast),
        Some((
            Duration::from_secs(FAST_SETTLE_S),
            Duration::from_millis(FAST_POLL_MS)
        )),
        "行の値がそのまま刻みになる（差し替えた周は差し替えた値）"
    );
    let off = Manifest::parse(&pace_manifest(FAST_SETTLE_S, FAST_POLL_MS, false))
        .expect("fixture を読める");
    assert_eq!(pace_of(&off), None, "不発効の行は引かない（値は在っても使わない）");
    let absent = Manifest::parse("schema = 1\n").expect("fixture を読める");
    assert_eq!(pace_of(&absent), None, "行そのものが無い周も引かない");
}

/// 確認できない周が返るまでの**壁時計**が `--rules` の行の値で決まる（`s2-07l.151`）。
///
/// 席は `/clear` を消化も echo もしない（[`start_swallowing_seat`]）＝分岐は宣言値のときと同じ
/// `clear-unconfirmed` で、変わるのは待つ長さだけである。上限 1 s で撃ち、**両側**を見る:
/// 上側は焼いた宣言値（[`SETTLE_S`] = 30 s）で待つ実装を落とし、下側は上限を無視して即断る実装を
/// 落とす。壁時計ゆえ等号は pin せず、1 s の手前と 30 s の手前に margin を取る（`s2-07l.118`）。
#[test]
fn seat_cycle_settle_limit_comes_from_rules() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatpace";
    let log = dir.join("seat.log");
    let guard = start_swallowing_seat(&socket, name, &log);
    assert!(guard.ready(), "`/clear` を飲む席を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.pace.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s, rules_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        pace_rules(&dir, 1, FAST_POLL_MS),
    );

    let started = Instant::now();
    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s, "--rules", &rules_s,
    ]);
    let waited = started.elapsed();

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(
        stderr_of(&out),
        format!("seat: cycle failed reason=clear-unconfirmed{}\n", provenance(&state, "flag")),
        "分岐は宣言値のときと同じ（上限まで待って確認できない）"
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n",
        "確認できない周に復元を送らない（待ちが短くても極性は不変）"
    );
    assert!(
        waited < Duration::from_secs(SETTLE_S / 2),
        "1 s の上限で返る（実測 {waited:?}・焼いた {SETTLE_S} s なら超える）"
    );
    assert!(
        waited >= Duration::from_millis(600),
        "上限を待たずに断っていない（実測 {waited:?}）"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 打刻は cycle 側の口で、`seat cycle` を直に回した周も **1 key も送らず refused で終わる周も**
/// 打つ（write-ahead・lock の内側）。tick 以外の経路で回した cycle の直後に tick が `/clear` を
/// 重ねない（lens-110 F-2）。
#[test]
fn seat_cycle_stamps_even_when_it_refuses_before_sending() {
    let dir = tmp();
    let target = "seatcyclestamp";
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).expect("wm dir を作れる");
    let (wm_s, state_s, sock_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        dir.join("absent-sock").display().to_string(),
    );

    let out = run_seat(&[
        "cycle", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
        "--state-dir", &state_s,
    ]);

    assert_ne!(rc_of(&out), i32::from(RC_OK), "退避物が無いので断る: {}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("refused reason=wm-missing"),
        "断った理由は退避物の不在（断りは stderr 側）: {}",
        stderr_of(&out)
    );
    let stamp = seat_dir_of(&state, target).join("cycle-stamp");
    assert!(stamp.is_file(), "断った周も cycle-stamp を打つ（評価した事実）");
    assert!(
        fs::read_to_string(&stamp).is_ok_and(|body| body.trim().parse::<u64>().is_ok()),
        "stamp の中身は unix 秒 1 行"
    );
    assert!(!seat_dir_of(&state, target).join("cycle.lock").exists(), "lock は返す");
    fs::remove_dir_all(&dir).ok();
}

/// stamp を**打てない**周は `/clear` を送る前に断る（`cycle-stamp-unwritable`・write-ahead）:
/// 打てないまま送ると次の周に記憶が無く、また送りうる（N1）。stamp の位置に dir を置いて
/// 書けなくする。`seat cycle` を直に回すので back-off の読みは通らず、書き口だけを測る。
#[test]
fn seat_cycle_refuses_without_sending_when_the_cycle_stamp_is_unwritable() {
    let dir = tmp();
    let target = "seatstampro";
    let state = dir.join("state");
    stamp_idle(&state, target);
    let seat = seat_dir_of(&state, target);
    fs::create_dir_all(seat.join("cycle-stamp")).expect("stamp の位置に dir を置ける");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.parked.md", target);
    let (wm_s, state_s, sock_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        dir.join("absent-sock").display().to_string(),
    );

    let (out, touched) = run_seat_probed(&dir, &[
        "cycle", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
        "--state-dir", &state_s,
    ]);

    assert_ne!(rc_of(&out), i32::from(RC_OK), "打てないので断る: {}", stdout_of(&out));
    assert!(
        stderr_of(&out).contains("refused reason=cycle-stamp-unwritable"),
        "断った理由は stamp を打てないこと: {}",
        stderr_of(&out)
    );
    assert!(!touched, "1 key も送らない（pane も読みに行かない）");
    assert!(!seat.join("cycle.lock").exists(), "lock は返す");
    fs::remove_dir_all(&dir).ok();
}

/// 作り直しの確認は **`/clear` の送達 ts 以後に足された `SessionStart` の打刻**で行う（`s2-07l.112`・
/// 設計 seat-state.md §6）。作り直された直後の実席の pane（[`REBUILT_PANE`]）を `--capture-file` で
/// 固定しても、判定に効くのは打刻で、cycle は復元へ進み `done` になる。
///
/// 旧来の echo `❯ /clear` を正の証拠に採る形（`.96`）は、前の `/clear` の echo が見えたまま今回の
/// `/clear` が消費されなかった周を「済んだ」と読む残余を持った（`.96` の残余 (1)）。
#[test]
fn seat_cycle_confirms_rebuilt_seat_by_session_start_stamp_and_restores() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatrebuilt";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Now),
    );
    assert!(guard.ready(), "偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.rebuilt.md", name);
    stamp_idle(&state, name);
    let pane = dir.join("pane.txt");
    fs::write(&pane, REBUILT_PANE).ok();
    let (wm_s, state_s, pane_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        pane.display().to_string(),
    );

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s, "--capture-file", &pane_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("seat: cycle done target={name}{}\n", provenance(&state, "flag")));
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "作り直しを打刻で確認して復元を送る"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// `/clear` が**入力行に残る**席（未 submit・[`UNSUBMITTED_CLEAR_PANE`] / echo と入力行の両方に
/// 在る [`REBUILT_UNSUBMITTED_PANE`]）は、打刻が Idle でも送る直前の入力欄の門で **`input-busy`**＝
/// 1 key も送らない（Enter だけが落ちた席へ `/clear` を重ねない・作り直し済みとも読まない）。
#[test]
fn seat_cycle_refuses_when_clear_is_stuck_in_input_line() {
    for (label, shape) in [
        ("unsubmitted", UNSUBMITTED_CLEAR_PANE),
        ("rebuilt-unsubmitted", REBUILT_UNSUBMITTED_PANE),
    ] {
        let dir = tmp();
        let name = "seatstuckinput";
        let (state, wm, pane) = (dir.join("state"), dir.join("wm"), dir.join("pane.txt"));
        wm_file(&wm, "working-memory.stuck.md", name);
        stamp_idle(&state, name);
        fs::write(&pane, shape).ok();
        let (wm_s, state_s, pane_s, sock_s) = (
            wm.display().to_string(),
            state.display().to_string(),
            pane.display().to_string(),
            dir.join("absent-sock").display().to_string(),
        );

        let (out, touched) = run_seat_probed(
            &dir,
            &[
                "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
                "--state-dir", &state_s, "--capture-file", &pane_s,
            ],
        );

        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{label}: stdout={}", stdout_of(&out));
        assert_eq!(stderr_of(&out), format!("seat: cycle refused reason=input-busy{}\n", provenance(&state, "flag")), "{label}");
        assert!(!touched, "{label}: 1 key も送らない（tmux を撃たない）");
        fs::remove_dir_all(&dir).ok();
    }
}

/// 置き場が**解けない**周（`--state-dir` 無し・git の外の cwd）は cycle も inject も 2 語を
/// 出さない（解いてもいない出所と書いてもいない置き場を名乗らない・tick と同じ規律）。
#[test]
fn seat_cycle_and_inject_omit_state_dir_provenance_when_unresolved() {
    let dir = tmp();
    let (wm_s, sock_s) = (dir.join("wm").display().to_string(), dir.join("absent-sock").display().to_string());

    let out = run_seat_in(&dir, &["cycle", "--target", "seatnostate", "--wm-dir", &wm_s, "--tmux-socket", &sock_s]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stdout_of(&out), "");
    assert_eq!(stderr_of(&out), "seat: cycle refused reason=state-dir\n", "解けない周は 2 語なし");

    let out = run_seat_in(&dir, &["inject", "--target", "seatnostate", "--tmux-socket", &sock_s, "--text", "hello"]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stdout_of(&out), "");
    assert_eq!(stderr_of(&out), "seat: inject unconfirmed reason=tmux-failed\n", "解けない周は 2 語なし");
    assert!(!dir.join("seat").exists(), "cwd に置き場を作らない");
    fs::remove_dir_all(&dir).ok();
}

/// cycle の成功行と記録は、置き場と出所（`.70` の 2 語）を末尾に持つ。A/B: 行の `state_dir=`
/// 以降から組んだ tick.jsonl の path が、記録が実際に書かれた path と 1 対 1 で一致する。
#[test]
fn seat_cycle_done_line_carries_state_dir_provenance() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatcycleprov";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Now),
    );
    assert!(guard.ready(), "fake な席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.parked.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(line, format!("seat: cycle done target={name}{}\n", provenance(&state, "flag")));
    let actual = tick_file(&state, name);
    let recorded = fs::read_to_string(&actual).unwrap_or_default();
    let last = recorded.lines().last().unwrap_or_default();
    assert!(
        last.contains(&format!(r#""what":"cycle done{}""#, provenance(&state, "flag"))),
        "記録の最終行の what にも同じ 2 語: {last}"
    );
    // A/B: 行が名乗る置き場から組んだ記録の path が、実際に書かれた記録と 1 対 1 で一致する。
    let claimed = state_dir_in(line.trim_end()).map(|d| tick_file(&d, name));
    assert_eq!(claimed.as_deref(), Some(actual.as_path()), "行の path と実体の記録が一致する");
    assert!(actual.exists(), "実体の記録が在る");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// `/clear` を**送った後**の pane に `/clear` の字面が在っても（`shape`）、`SessionStart` の打刻が
/// 足されない席は `clear-unconfirmed`＝復元を送らない（字面は証拠ではない・`s2-07l.112`）。
///
/// 入口の入力欄の門は送る前の pane で通す（[`IDLE_TALL_PANE`]）ので、字面は**送達の後に**
/// 現れた形になる（Enter だけが落ちた・席が echo を引用した周の再現）。形ごとに歯を分ける
/// のは、確認の待ちが直列に積み上がらないようにするため（待ちそのものは `--rules` の
/// [`FAST_SETTLE_S`] で縮める・分岐は宣言値のときと同じ・`s2-07l.151`）。
fn assert_clear_unconfirmed_after_send(label: &str, shape: &str) {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatecho";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Never, FakeStamp::Now),
    );
    assert!(guard.ready(), "{label}: 偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.echo.md", name);

    let out = cycle_with_pane_after_clear(&dir, name, IDLE_TALL_PANE, shape, FAST_SETTLE_S);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{label}: stdout={}", stdout_of(&out));
    assert_eq!(
        stderr_of(&out),
        format!("seat: cycle failed reason=clear-unconfirmed{}\n", provenance(&dir.join("state"), "flag")),
        "{label}"
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n",
        "{label}: 作り直しを確認できない周に復元を送らない"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 未 submit（[`UNSUBMITTED_CLEAR_PANE`]）: 字面が入力行に残る周は確認できない（打刻も無い）。
#[test]
fn seat_cycle_does_not_confirm_clear_left_unsubmitted_in_input_line() {
    assert_clear_unconfirmed_after_send("unsubmitted", UNSUBMITTED_CLEAR_PANE);
}

/// echo と入力行の両方に在る（[`REBUILT_UNSUBMITTED_PANE`]）: echo は証拠にならない（証拠は打刻）。
#[test]
fn seat_cycle_does_not_confirm_clear_when_echo_and_unsubmitted_coexist() {
    assert_clear_unconfirmed_after_send("rebuilt-unsubmitted", REBUILT_UNSUBMITTED_PANE);
}

/// 本文の引用（[`QUOTED_CLEAR_PANE`]）: 引用の字面は証拠にならない（証拠は打刻）。
#[test]
fn seat_cycle_does_not_confirm_clear_from_quoted_echo() {
    assert_clear_unconfirmed_after_send("quoted", QUOTED_CLEAR_PANE);
}

/// `/clear` で**始まる**発言の echo（[`PREFIXED_CLEAR_PANE`]）: 会話の生きた席の字面は証拠に
/// ならない（lens-96 HIGH-2 の形・打刻が無ければ復元を送らない）。
#[test]
fn seat_cycle_does_not_confirm_clear_from_prefixed_user_line() {
    assert_clear_unconfirmed_after_send("prefixed", PREFIXED_CLEAR_PANE);
}

/// `SessionStart` の打刻が**遅れて**来る席（実席の hook は `/clear` の数秒後に打つ）でも、窓の内に
/// 足されれば作り直しと読んで復元へ進む（`s2-07l.112`）。pane の echo の下に hook の出力行が何行
/// 増えても（[`REBUILT_HOOKS_PANE`]）判定に効かない＝字面の行数に依らない。
///
/// 上限は席の `sleep 3` より**長い** [`LATE_SETTLE_S`] を渡す（窓の内に来る側の歯ゆえ
/// [`FAST_SETTLE_S`] では遅れた打刻を拾えない＝測る分岐が別物になる）。
#[test]
fn seat_cycle_confirms_rebuild_when_session_start_stamp_arrives_late() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seathooks";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let stamps = state_file(&seat_dir_of(&state, name));
    let guard = start_clearing_seat_with(
        &socket,
        name,
        &log,
        &format!("sleep 3; {}", stamp_cmd(&stamps, "idle", "SessionStart", FakeStamp::Now)),
        &format!("{}; printf 'seat got %s\\n' \"$line\"", stamp_cmd(&stamps, "busy", "UserPromptSubmit", FakeStamp::Now)),
    );
    assert!(guard.ready(), "遅れて打刻する偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.hooks.md", name);

    let out = cycle_with_pane_after_clear(&dir, name, IDLE_TALL_PANE, REBUILT_HOOKS_PANE, LATE_SETTLE_S);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("seat: cycle done target={name}{}\n", provenance(&state, "flag")));
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "遅れた打刻を窓の内に拾って復元を送る"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// **送る前から同じ字面が pane に在る**周は送達の根拠にならない（`absent`・rc 1・記録なし）。
///
/// 席は打鍵を表示も実行もしない（`stty -echo` + `cat > /dev/null`）ので pane は 1 byte も
/// 変わらない。差は先在の字面だけで、`contains` 1 本の判定はこれを「届いた」と読む（lens-90
/// HIGH-1）。tick の pointer は target ごとに固定なので、2 周目以降は常にこの条件下に在る。
#[test]
fn seat_inject_does_not_count_preexisting_text_as_delivery() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-stale";
    let marker = ": seat-e2e-stale-marker";
    let mut seat = IsolatedSeat {
        socket: socket.clone(),
        name: name.to_owned(),
        ready: false,
    };
    let script =
        format!("printf '{marker}\\n\u{276f} '; stty -echo 2>/dev/null; exec cat > /dev/null");
    let out = tmux(
        &socket,
        &["new-session", "-d", "-s", name, "-x", "120", "-y", "40", "sh", "-c", &script],
    );
    seat.ready = out.status.success() && wait_prompt(&socket, name);
    assert!(seat.ready, "先在の字面つきの凍結席を立てられる");
    let state = dir.join("state");

    let out = run_seat(&[
        "inject",
        "--target",
        name,
        "--tmux-socket",
        &socket,
        "--state-dir",
        &state.display().to_string(),
        "--text",
        marker,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(stderr_of(&out), format!("seat: inject unconfirmed reason=absent{}\n", provenance(&state, "flag")));
    assert!(!tick_file(&state, name).exists(), "届いていない周は記録しない");
    drop(seat);
    fs::remove_dir_all(&dir).ok();
}

/// 作り直した席で復元 command が**入力欄に置き去り**（echo されたが誰も消費しない）の周は
/// `restore-unconfirmed`＝queue を復元成功と数えない（lens-90 HIGH-2）。
///
/// `/clear` 直後の席には走っている turn が無いので、そこでの queue は「turn の終わりに消費
/// される」ではなく submit されなかった打鍵である。
///
/// 復元の消費の確認は作り直しの確認と同じ**上限まで待つ**（`s2-07l.97`）: 置き去りの復元には
/// `UserPromptSubmit` の打刻が来ないので、待った後も `restore-unconfirmed` のまま（成功条件は不変）。
#[test]
fn seat_cycle_reports_restore_unconfirmed_after_limit_when_restore_is_left_in_input() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatleft";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    // `/clear` の後は作り直しの打刻を置いて prompt を描き、その後は **読まない**（echo は入る・
    // 消費者がいない＝`UserPromptSubmit` の打刻も来ない）。
    let guard = start_clearing_seat_with(
        &socket,
        name,
        &log,
        &format!(
            "{}; printf '\u{276f} '; exec cat > /dev/null",
            stamp_cmd(&state_file(&seat_dir_of(&state, name)), "idle", "SessionStart", FakeStamp::Now)
        ),
        ":",
    );
    assert!(guard.ready(), "偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.left.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s, rules_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        fast_rules(&dir),
    );

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s, "--rules", &rules_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(stderr_of(&out), format!("seat: cycle failed reason=restore-unconfirmed{}\n", provenance(&state, "flag")));
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n",
        "作り直しは送った・復元は席に読まれていない"
    );
    let pane = capture(&socket, name);
    assert!(pane.contains("/rebrief"), "復元の字面は入力欄に残っている: {pane}");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 作り直し直後の席が **hook の実行中で復元を入力欄に queue したまま**、後から消費する周は
/// `done`（`s2-07l.97`）。
///
/// 実席は `/clear` の後に SessionStart hook を数秒〜十数秒走らせ、その間に注入された行を
/// 入力欄に置いたまま turn を始めない（実測 2026-09-11 `.96` A/B: `/rebrief` は着地して
/// rebrief が走ったのに `restore-unconfirmed`）。2 s の settle の窓では必ず `Queued` で終わる
/// ので、**復元が正しく届く周ほど failed になる**。偽の席は prompt を描いた後 8 s 読まず、
/// その後に queue を消費する（入力欄が空になり echo が上に残る＝`Consumed` の形）。
/// 8 s は base（2 s の窓・実測 2.6 s で失敗）に対する RED の余裕を負荷時にも保つ長さ（lens-97 LOW-1）。
#[test]
fn seat_cycle_restores_after_seat_consumes_queued_restore() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatqueued";
    let log = dir.join("seat.log");
    // `/clear` の後は prompt を描いてから **hook のように 8 s 読まない**（2 s の窓より十分長い）。
    let state = dir.join("state");
    let stamps = state_file(&seat_dir_of(&state, name));
    let guard = start_clearing_seat_with(
        &socket,
        name,
        &log,
        &format!("{}; printf '\u{276f} '; sleep 8", stamp_cmd(&stamps, "idle", "SessionStart", FakeStamp::Now)),
        &format!("{}; printf 'seat got %s\\n' \"$line\"", stamp_cmd(&stamps, "busy", "UserPromptSubmit", FakeStamp::Now)),
    );
    assert!(guard.ready(), "hook 中の席を模す偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.queued.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("seat: cycle done target={name}{}\n", provenance(&state, "flag")));
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "queue された復元を席が消費した"
    );
    let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
    assert!(
        recorded.contains(&format!(r#""what":"cycle done{}""#, provenance(&state, "flag"))),
        "記録の what にも 2 語: {recorded}"
    );
    assert!(
        !seat_dir_of(&state, name).join("cycle.lock").exists(),
        "済んだ lock は返す"
    );
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 先頭行が**空**の payload でも、送達の目印は最初の非空行＝pane が伸びただけでは成立しない
/// （lens-90 再確認 NEW-1: 目印が空文字だと出現数が pane の長さに化け、stdin を読まない席でも
/// `consumed=true` になっていた）。
#[test]
fn seat_inject_uses_first_nonblank_line_as_marker() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-blankfirst";
    let mut seat = IsolatedSeat {
        socket: socket.clone(),
        name: name.to_owned(),
        ready: false,
    };
    // stdin を読まず、0.3 秒ごとに 1 行足す席（pane は伸びるが送った字面は現れない）。
    let script = "printf '\u{276f} '; stty -echo 2>/dev/null; while :; do sleep 0.3; printf '\\ntock'; done";
    let out = tmux(
        &socket,
        &["new-session", "-d", "-s", name, "-x", "120", "-y", "40", "sh", "-c", script],
    );
    seat.ready = out.status.success() && wait_prompt(&socket, name);
    assert!(seat.ready, "pane が伸びる席を立てられる");
    let state = dir.join("state");

    let out = run_seat(&[
        "inject",
        "--target",
        name,
        "--tmux-socket",
        &socket,
        "--state-dir",
        &state.display().to_string(),
        "--text",
        "\nseat-e2e-blank-first",
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(stderr_of(&out), format!("seat: inject unconfirmed reason=absent{}\n", provenance(&state, "flag")));
    assert!(!tick_file(&state, name).exists(), "届いていない周は記録しない");
    drop(seat);
    fs::remove_dir_all(&dir).ok();
}

// ─────────────────── 立て直しの shell の門（account-autonomy.md §5・接頭辞 `seat_relaunch_`） ───────────────────

/// 立て直しの注入の門で断った周（rc 1・`decision=error reason=<reason>`）: 起動の雛形は注入されず、登録 row は
/// 増えず、席の記録は 1 周目の退避の合図だけ。
fn acct_assert_shell_refused(place: &AcctPlace, target: &str, out: &Output, reason: &str) {
    let line = stderr_of(out);
    assert_eq!(rc_of(out), i32::from(RC_REFUSED), "{reason}: stdout={} stderr={line}", stdout_of(out));
    assert_eq!(tick_token(&line, "decision").as_deref(), Some("error"), "{line}");
    assert_eq!(tick_token(&line, "reason").as_deref(), Some(reason), "{line}");
    assert!(!place.dir.join("launched").exists(), "{reason}: 起動の雛形は注入されない");
    assert_eq!(acct_rows(&place.state).len(), 1, "{reason}: SeatRegistered は増えない");
    assert_eq!(acct_sent(&place.state, target).len(), 1, "{reason}: 1 周目の退避の合図だけ");
}

/// (1) 席が終わって pane の可視域が shell の prompt（`user@host:dir$ `・`❯` 無し）で終わる周は、shell の門を通って
/// 立て直す（起動 → 復元・判定行は `kind=relaunch`）。base は席の門（`❯` の行）で読み `relaunch-input-unknown`
/// 相当で断る（RED）。
#[test]
fn seat_relaunch_shell_prompt_without_the_seat_prompt_relaunches() {
    let place = acct_place();
    let name = "shellplain";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    acct_parked(&place, name, &acct_launcher(&place, name), 30);
    assert!(acct_shell_prompt(&place, name, ""), "shell の prompt を描ける");
    let seen = capture(&place.socket, name);
    assert!(!seen.contains('\u{276f}'), "可視域に `❯` は無い: {seen}");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("relaunch", ACCT_SPARE)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    acct_assert_launched_then_restored(&place, name);
    acct_assert_relabelled(&place, name);
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (2) shell の prompt の後に打ちかけ（`git st`）が在る周は送らない（`relaunch-input-busy`）。
#[test]
fn seat_relaunch_shell_prompt_with_a_draft_is_refused_busy() {
    let place = acct_place();
    let name = "shelldraft";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    acct_parked(&place, name, &acct_launcher(&place, name), 30);
    assert!(acct_shell_prompt(&place, name, ""), "shell の prompt を描ける");
    assert!(tmux(&place.socket, &["send-keys", "-t", name, "-l", "git st"]).status.success());
    assert!(acct_wait_pane(&place, name, |pane| pane.trim_end().ends_with("$ git st")), "打ちかけが描かれる");

    let out = acct_tick(&place, name, None);

    acct_assert_shell_refused(&place, name, &out, "relaunch-input-busy");
    assert!(capture(&place.socket, name).trim_end().ends_with("$ git st"), "打ちかけは触られない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (3) 可視域の上に終了した席の古い `❯` 行が残っていても、最後の非空行が shell の prompt なら (1) と同じく送る
/// （古い `❯` 行の右側を入力欄と読まない）。
#[test]
fn seat_relaunch_shell_prompt_below_a_stale_seat_prompt_relaunches() {
    let place = acct_place();
    let name = "shellstale";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    acct_parked(&place, name, &acct_launcher(&place, name), 30);
    assert!(acct_shell_prompt(&place, name, "\\342\\235\\257 /exit\\n"), "古い `❯` 行の下に shell の prompt を描ける");
    let seen = capture(&place.socket, name);
    assert!(seen.contains("\u{276f} /exit"), "古い `❯` 行が残る: {seen}");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("relaunch", ACCT_SPARE)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    acct_assert_launched_then_restored(&place, name);
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (4) 最後の非空行が shell の prompt 末尾で終わらない（`Password:` の問い）周は送らない（`relaunch-input-unknown`）。
#[test]
fn seat_relaunch_shell_without_a_prompt_tail_is_refused_unknown() {
    let place = acct_place();
    let name = "shellpassword";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    acct_parked(&place, name, &acct_launcher(&place, name), 30);
    assert!(acct_shell_prompt(&place, name, ""), "shell の prompt を描ける");
    assert!(tmux(&place.socket, &["send-keys", "-t", name, "-l", "printf 'Password:'; read answer"]).status.success());
    assert!(tmux(&place.socket, &["send-keys", "-t", name, "Enter"]).status.success());
    assert!(acct_wait_pane(&place, name, |pane| pane.trim_end().ends_with("Password:")), "問いが描かれる");

    let out = acct_tick(&place, name, None);

    acct_assert_shell_refused(&place, name, &out, "relaunch-input-unknown");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (5) 立て直しの起動行は agent view を切る env を先頭に前置して注入する（`s2-07l.239`・account-autonomy.md §5「agent view
/// の前提」）: 雛形 `CLAUDE_CONFIG_DIR={account_dir} sh <script>` は書き換えず、席の記録の起動行はちょうど
/// `CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR=<a2 の credential dir> sh <script>`。効果でも測る: shell が export した
/// 別の値を前置が上書きし、起こした process は `1` を見る。base は雛形のまま `CLAUDE_CONFIG_DIR=` で始まる（RED）。
#[test]
fn seat_relaunch_agent_view_off_prefixes_launch_line() {
    let place = acct_place();
    let name = "agentviewoff";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let script = acct_launch_script(&place, name, "\"$CLAUDE_CONFIG_DIR\" \"$CLAUDE_CODE_DISABLE_AGENT_VIEW\"");
    acct_parked(&place, name, &format!("CLAUDE_CONFIG_DIR={{account_dir}} sh {script}"), 30);
    // pane の shell が別の値を持つ（前置が無ければ起こした process はこれを継承する）。
    assert!(tmux(&place.socket, &["send-keys", "-t", name, "-l", "export CLAUDE_CODE_DISABLE_AGENT_VIEW=inherited"]).status.success());
    assert!(tmux(&place.socket, &["send-keys", "-t", name, "Enter"]).status.success());
    assert!(acct_shell_prompt(&place, name, ""), "shell の prompt を描ける");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("relaunch", ACCT_SPARE)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    let spare = place.state.join("accounts").join(ACCT_SPARE).display().to_string();
    let sent = acct_sent(&place.state, name);
    assert_eq!(
        sent.get(1),
        Some(&format!("CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={spare} sh {script}")),
        "起動行は agent view off を 1 つだけ前置した雛形: {sent:?}"
    );
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("{spare}\n1\n"),
        "起こした process は選んだ口座と agent view off を見る"
    );
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 退避後の終了の手（account-autonomy.md §5・`s2-07l.226`・接頭辞 `seat_exit_`） ───────────────────

/// 終了の手を受けた席が写す 1 行の置き場（名に `/exit` を含めない＝送達の目印の出現数を pane の echo で汚さない）。
const EXIT_LOG: &str = "received.log";

/// pane の前面 process の名（`#{pane_current_command}`・器の入口 (3) と同じ typed な読み）。
fn exit_foreground(place: &AcctPlace, name: &str) -> String {
    let out = tmux(&place.socket, &["list-panes", "-t", name, "-F", "#{pane_current_command}"]);
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// 前面が `want` になるまで待つ（上限 [`PROMPT_WAIT`]）。
fn exit_wait_foreground(place: &AcctPlace, name: &str, want: &str) -> bool {
    let deadline = Instant::now().checked_add(PROMPT_WAIT);
    while deadline.is_some_and(|at| Instant::now() < at) {
        if exit_foreground(place, name) == want {
            return true;
        }
        sleep(Duration::from_millis(100));
    }
    false
}

/// 前面が shell でない偽の席にする: `sh -i` の session（[`start_seat`]）に、prompt `❯ ` を描いて **1 行だけ読む `head`** を
/// 走らせる。前面 process は `head`（shell の列の外＝入口 (3) は立たない）で、受けた 1 行を [`EXIT_LOG`] に写して終わり、
/// shell へ戻る（`/exit` を受けた席が終わる形）。前面が `head` になったかを返す。
fn exit_seat(place: &AcctPlace, name: &str) -> bool {
    let line = format!("printf '\\342\\235\\257 '; head -n 1 >> '{}'", place.dir.join(EXIT_LOG).display());
    tmux(&place.socket, &["send-keys", "-t", name, "-l", &line]).status.success()
        && tmux(&place.socket, &["send-keys", "-t", name, "Enter"]).status.success()
        && exit_wait_foreground(place, name, "head")
}

/// 退避して止まり、前面が shell でない席の fixture: [`start_seat`] → 退避の合図 → `Stop`（[`acct_parked`]・候補 a2 = 30）→
/// 自席の未 consumed 退避物 → 前面を `head` に（退避の合図は前の `sh -i` が受ける＝`head` は 1 行も読んでいない）。
fn exit_parked(place: &AcctPlace, name: &str) -> IsolatedSeat {
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    acct_parked(place, name, &acct_launcher(place, name), 30);
    wm_file(&place.wm, "working-memory.parked.md", name);
    assert!(exit_seat(place, name), "前面が head の席を作れる");
    assert_eq!(exit_received(place), "", "head はまだ 1 行も読んでいない");
    guard
}

/// 席が受けた 1 行（受けていなければ空・`>>` は開いた時点で空 file を作る）。
fn exit_received(place: &AcctPlace) -> String {
    fs::read_to_string(place.dir.join(EXIT_LOG)).unwrap_or_default()
}

/// 送らなかった周の 3 面: 席は 1 行も受けていない・cycle-stamp も exit-stamp も打っていない・前面は `head` のまま（席は生きている）。
fn exit_assert_not_sent(place: &AcctPlace, name: &str, case: &str) {
    assert_eq!(exit_received(place), "", "{case}: 1 key も送らない");
    assert!(!seat_dir_of(&place.state, name).join("cycle-stamp").exists(), "{case}: cycle-stamp を打たない");
    assert!(!seat_dir_of(&place.state, name).join("exit-stamp").exists(), "{case}: exit-stamp を打たない");
    assert_eq!(exit_foreground(place, name), "head", "{case}: 席は生きたまま");
}

/// (a) 退避の合図 → `Stop` → 自席の未 consumed 退避物 → 前面が shell でない（`head`）の周: 器が `/exit` を入力欄の門を
/// 通して送り（判定行 `decision=inject … kind=exit`・席が受けた 1 行は `/exit`）、注入の記録が `inject.jsonl` の同じ形
/// （`who=seat-tick`・`kind=exit`）で残り、席の記録に送った 1 行（`who=seat-cycle`）が残る。受けた席は終わって前面が
/// shell へ戻る（次の周の立て直しの入口 (3)）。base は機能不在＝退避物の在る周は `/clear` の cycle に落ちる（RED）。
#[test]
fn seat_exit_injects_exit_to_the_parked_seat_and_stamps() {
    let place = acct_place();
    let name = "exitsend";
    let guard = exit_parked(&place, name);

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "exit"), ("account", "a1:100")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(tick_token(&line, "cycle"), None, "/clear の cycle は回さない: {line}");
    assert_eq!(exit_received(&place), "/exit\n", "席が受けた 1 行は /exit");
    assert!(seat_dir_of(&place.state, name).join("exit-stamp").exists(), "exit-stamp を打つ");
    let log = fs::read_to_string(place.state.join("inject.jsonl")).unwrap_or_default();
    let last = log.lines().last().unwrap_or_default();
    assert_eq!(acct_text(last, "who").as_deref(), Some("seat-tick"), "{log}");
    assert_eq!(acct_text(last, "seat").as_deref(), Some(name), "{log}");
    assert!(acct_text(last, "what").is_some_and(|what| what.contains(" kind=exit")), "判定行と同じ字面: {log}");
    assert_eq!(acct_sent(&place.state, name).last().map(String::as_str), Some("/exit"), "席の記録に送った 1 行が残る");
    assert!(exit_wait_foreground(&place, name, "sh"), "受けた席は終わって前面が shell へ戻る");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (a′) 退避の合図の後に席が Bash を撃ち、`inject.jsonl` の同じ席の最新行が hook の記録（`hook:pre-tool-use` の
/// role-allow）になった周も、合図は隠れず終了の手を選ぶ（`s2-07l.242`・.226 の実地 2 回目）。base は最新行だけを読み
/// 合図が見えない＝退避物の在る周は `/clear` の cycle に落ちる（RED）。
#[test]
fn seat_exit_signal_survives_hook_rows_and_sends_exit() {
    let place = acct_place();
    let name = "exithookrows";
    let guard = exit_parked(&place, name);
    let log = place.state.join("inject.jsonl");
    let mut text = fs::read_to_string(&log).unwrap_or_default();
    for what in ["decision=role-allow capability=launch", "decision=role-allow capability=read"] {
        text.push_str(&format!(
            r#"{{"schema":1,"who":"hook:pre-tool-use","what":"{what}","when":"PreToolUse","bytes":0,"tokens":null,"wall_ms":0,"seat":"{name}","ts":{}}}"#,
            unix_now()
        ));
        text.push('\n');
    }
    fs::write(&log, text).ok();

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "exit"), ("account", "a1:100")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(tick_token(&line, "cycle"), None, "/clear の cycle は回さない: {line}");
    assert_eq!(exit_received(&place), "/exit\n", "席が受けた 1 行は /exit");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) 自席の未 consumed 退避物が無い周は `/exit` を送らない（FR28 と同じ極性）: 閾値以上の口座は (1) の退避の合図の側へ
/// 落ち、席が受ける 1 行は退避の合図であって `/exit` ではない。cycle-stamp も打たない。
#[test]
fn seat_exit_is_not_sent_without_an_unconsumed_wm() {
    let place = acct_place();
    let name = "exitnowm";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    acct_parked(&place, name, &acct_launcher(&place, name), 30);
    assert!(exit_seat(&place, name), "前面が head の席を作れる");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    assert_eq!(tick_token(&line, "kind").as_deref(), Some("externalize"), "退避の合図の側へ落ちる: {line}");
    let received = exit_received(&place);
    assert!(received.contains("/ready-compaction") && !received.contains("/exit"), "席が受けたのは退避の合図: {received}");
    assert!(!seat_dir_of(&place.state, name).join("cycle-stamp").exists(), "cycle-stamp を打たない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) cycle lock が live な周は送らない（`noop reason=cycle-live`・作り直しの最中の席へ重ねない）。
#[test]
fn seat_exit_is_not_sent_while_the_cycle_lock_is_live() {
    let place = acct_place();
    let name = "exitlock";
    let guard = exit_parked(&place, name);
    fs::write(seat_dir_of(&place.state, name).join("cycle.lock"), "{}\n").ok();

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "noop"), ("reason", "cycle-live"), ("account", "a1:100")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    exit_assert_not_sent(&place, name, "cycle-live");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) 入力欄に文字（打ちかけ `draft`）が在る周は入力欄の門で断る（`decision=error reason=exit-input-busy`・rc 1）: 1 key も
/// 送らず、cycle-stamp も打たず、打ちかけは触られない。
#[test]
fn seat_exit_refuses_when_the_input_line_is_busy() {
    let place = acct_place();
    let name = "exitdraft";
    let guard = exit_parked(&place, name);
    assert!(tmux(&place.socket, &["send-keys", "-t", name, "-l", "draft"]).status.success());
    assert!(acct_wait_pane(&place, name, |pane| pane.trim_end().ends_with("\u{276f} draft")), "打ちかけが描かれる");

    let out = acct_tick(&place, name, None);

    let line = stderr_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={} stderr={line}", stdout_of(&out));
    for (key, want) in [("decision", "error"), ("reason", "exit-input-busy")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    exit_assert_not_sent(&place, name, "input-busy");
    assert!(capture(&place.socket, name).trim_end().ends_with("\u{276f} draft"), "打ちかけは触られない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (e) back-off: exit-stamp が `seat.tick_stale_s` 未満の前で前面が shell でない（`/exit` が効かなかった席）なら `/exit` を
/// **送り直さず**（席が受ける行は 0・`s2-07l.252` (3d)）stamp も打ち直さない（打ち直すと永久に止まる）。その周は見送りでなく
/// 第 2 手＝停止（`s2-07l.259`・account-autonomy.md §5「終了の手の第 2 手」）: 前面の `head` を TERM で止めて終了を確定し
/// （`kind=exit detail=terminated`）、前面は shell へ戻る。stamp が閾値以上前になった周が第 1 手から送り直す極性は
/// `seat_exit_stop_resends_exit_when_the_stamp_is_stale` が持つ。base（.252 以前）は exit-stamp を読まず 1 周目に送る（RED）。
#[test]
fn seat_exit_stamp_recent_does_not_resend_exit() {
    let place = acct_place();
    let name = "exitbackoff";
    let guard = exit_parked(&place, name);
    let stamp = seat_dir_of(&place.state, name).join("exit-stamp");
    fs::write(&stamp, "0\n").ok();
    let stamped_at = mtime_of(&stamp);

    let first = acct_tick(&place, name, None);

    let line = stdout_of(&first);
    assert_eq!(rc_of(&first), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&first));
    for (key, want) in [("decision", "inject"), ("kind", "exit"), ("detail", "terminated")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(exit_received(&place), "", "/exit を送り直さない（head は 1 行も受けずに止められた）");
    assert_eq!(mtime_of(&stamp), stamped_at, "第 2 手の周は stamp を打ち直さない");
    assert_eq!(acct_sent(&place.state, name).len(), 1, "席の記録は退避の合図だけ（/exit の行は増えない）");
    assert!(exit_wait_foreground(&place, name, "sh"), "head が止まり前面は shell へ戻る");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (f) 次の周（stamp を倒さない＝`/exit` の直後）: 直近の注入が `exit` ∧ 前面が shell（席が終わった）なら立て直しの入口が
/// 立ち、exit-stamp は立て直しの back-off に効かず（`cycle-stamp=none`）、雛形の穴が選んだ口座（a2）の credential dir で
/// 埋まった起動と復元がこの順で注入される（`kind=relaunch`・席の記録は 退避の合図 → `/exit` → 起動 → 復元 の 4 行・登録
/// row の口座は a2 に）。base は `/exit` が cycle-stamp を打つので `cycle-recent` で見送る（RED・`s2-07l.252` (3c)）。
#[test]
fn seat_exit_stamp_then_shell_relaunches_on_the_next_round() {
    let place = acct_place();
    let name = "exitrelaunch";
    let guard = exit_parked(&place, name);
    let first = stdout_of(&acct_tick(&place, name, None));
    assert_eq!(tick_token(&first, "kind").as_deref(), Some("exit"), "1 周目は終了の手: {first}");
    assert!(exit_wait_foreground(&place, name, "sh"), "席が終わって前面が shell へ戻る");
    assert!(acct_shell_prompt(&place, name, ""), "shell の prompt を描ける");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    assert!(seat_dir_of(&place.state, name).join("exit-stamp").exists(), "exit-stamp は閾値未満のまま在る");
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("relaunch", ACCT_SPARE), ("cycle-stamp", "none")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("{}\n", place.state.join("accounts").join(ACCT_SPARE).display()),
        "穴は選んだ口座の credential dir で埋まる"
    );
    assert_eq!(fs::read_to_string(place.dir.join("seat.log")).unwrap_or_default(), "/rebrief\n", "立ち上がった席が復元を受けた");
    let sent = acct_sent(&place.state, name);
    assert_eq!(sent.len(), 4, "退避の合図・/exit・起動・復元の 4 行: {sent:?}");
    assert_eq!(sent.get(1).map(String::as_str), Some("/exit"), "2 行目は終了の手: {sent:?}");
    assert!(
        sent.get(2).is_some_and(|what| what.starts_with("CLAUDE_CODE_DISABLE_AGENT_VIEW=1 sh ")),
        "3 行目は agent view off を前置した起動の雛形: {sent:?}"
    );
    assert_eq!(sent.get(3).map(String::as_str), Some("/rebrief"), "4 行目は復元: {sent:?}");
    acct_assert_relabelled(&place, name);
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (g) 前面が shell の周（session は終わっている）は `/exit` を送らない: 入口は立て直しの側で、候補が無ければ
/// `account-no-candidate`（shell へ `/exit` が届かない・記録は退避の合図だけ・cycle-stamp なし）。
#[test]
fn seat_exit_is_not_sent_to_a_shell() {
    let place = acct_place();
    let name = "exitshell";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    acct_parked(&place, name, &acct_launcher(&place, name), 90);
    wm_file(&place.wm, "working-memory.parked.md", name);
    assert!(acct_shell_prompt(&place, name, ""), "shell の prompt を描ける");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "noop"), ("reason", "account-no-candidate"), ("relaunch", "none:over-threshold")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(tick_token(&line, "kind"), None, "注入していない: {line}");
    assert!(!capture(&place.socket, name).contains("/exit"), "shell へ /exit を送らない");
    assert_eq!(acct_injected(&place.state, name).len(), 1, "記録は 1 周目の退避の合図だけ");
    assert!(!seat_dir_of(&place.state, name).join("cycle-stamp").exists(), "cycle-stamp を打たない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (h) 打刻の最終行が `Stop` より後の `UserPromptSubmit`（席が続きを始めた）なら送らない: 入口 (2) が立たず、以後は既存の
/// 順序どおり `noop reason=busy`（1 key も送らない・cycle-stamp なし・席は生きたまま）。
#[test]
fn seat_exit_is_not_sent_when_the_seat_resumed_after_stop() {
    let place = acct_place();
    let name = "exitresumed";
    let guard = exit_parked(&place, name);
    let file = state_file(&seat_dir_of(&place.state, name));
    let mut text = fs::read_to_string(&file).unwrap_or_default();
    text.push_str(&stamp_line("busy", "UserPromptSubmit", unix_now().saturating_add(2), ACCT_SID));
    text.push('\n');
    fs::write(&file, text).ok();

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "noop"), ("reason", "busy"), ("account", "a1:100")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    exit_assert_not_sent(&place, name, "resumed");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 終了の手の stamp と送達確認（account-autonomy.md §5 追補・`s2-07l.252`・接頭辞 `seat_exit_stamp_`） ───────────────────

/// (3a) `/exit` を受けて前面が shell に変わる席: 送達は前面 process の読みで確かめ（`decision=inject kind=exit
/// consumed=true`・`exit-absent` にならない）、打つのは exit-stamp で cycle-stamp は触られない。base は `/exit` の後に
/// cycle-stamp を打つ（RED）。
#[test]
fn seat_exit_stamp_is_separate_and_exit_is_confirmed_by_the_shell() {
    let place = acct_place();
    let name = "exitstamp";
    let guard = exit_parked(&place, name);

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "exit"), ("consumed", "true"), ("exit-stamp", "none")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(tick_token(&line, "cycle-stamp"), None, "終了の手は cycle-stamp を読まない: {line}");
    assert_eq!(exit_received(&place), "/exit\n", "席が受けた 1 行は /exit");
    let dir = seat_dir_of(&place.state, name);
    assert!(dir.join("exit-stamp").exists(), "exit-stamp を打つ");
    assert!(!dir.join("cycle-stamp").exists(), "cycle-stamp は触らない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (3b) `/exit` を受けても前面が shell にならない席（`cat` が読み続ける）: 窓の内に確かめられず `decision=error
/// reason=exit-unconfirmed`（rc 1）・exit-stamp は打たれたまま（再送しない）・cycle-stamp は無い。
#[test]
fn seat_exit_stamp_stays_when_the_exit_is_unconfirmed() {
    let place = acct_place();
    let name = "exitstuck";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    acct_parked(&place, name, &acct_launcher(&place, name), 30);
    wm_file(&place.wm, "working-memory.parked.md", name);
    let cat = format!("printf '\\342\\235\\257 '; cat >> '{}'", place.dir.join(EXIT_LOG).display());
    assert!(tmux(&place.socket, &["send-keys", "-t", name, "-l", &cat]).status.success());
    assert!(tmux(&place.socket, &["send-keys", "-t", name, "Enter"]).status.success());
    assert!(exit_wait_foreground(&place, name, "cat"), "前面が cat の席を作れる");

    let out = acct_tick(&place, name, None);

    let line = stderr_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={} stderr={line}", stdout_of(&out));
    for (key, want) in [("decision", "error"), ("reason", "exit-unconfirmed")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(exit_received(&place), "/exit\n", "/exit は送った");
    assert_eq!(exit_foreground(&place, name), "cat", "席は終わっていない");
    let dir = seat_dir_of(&place.state, name);
    assert!(dir.join("exit-stamp").exists(), "exit-stamp は打たれたまま");
    assert!(!dir.join("cycle-stamp").exists(), "cycle-stamp は触らない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (3e) 直前に立て直した（cycle-stamp が閾値未満）席は、前面が shell でも立て直しを `cycle-recent` で見送る（既存の
/// 極性は不変・exit-stamp が無くても cycle-stamp は効く）。
#[test]
fn seat_exit_stamp_cycle_stamp_still_holds_the_relaunch() {
    let place = acct_place();
    let name = "exitcyclerecent";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    acct_parked(&place, name, &acct_launcher(&place, name), 30);
    wm_file(&place.wm, "working-memory.parked.md", name);
    assert!(acct_shell_prompt(&place, name, ""), "shell の prompt を描ける");
    let dir = seat_dir_of(&place.state, name);
    fs::write(dir.join("cycle-stamp"), "0\n").ok();

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "noop"), ("reason", "cycle-recent")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(tick_token(&line, "kind"), None, "注入していない: {line}");
    assert!(!place.dir.join("launched").exists(), "起動しない");
    assert!(!dir.join("exit-stamp").exists(), "exit-stamp は打たない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 終了の手の第 2 手 = 停止（account-autonomy.md §5・`s2-07l.259`・接頭辞 `seat_exit_stop_`） ───────────────────

/// 前面の子が終わった後に pane の shell が写す `$?` の置き場（143 = TERM・137 = KILL・shell の慣習 128 + signal 番号＝
/// どの signal で止まったかを歯が pane の字面を読まずに知る）。
const STOP_RC_LOG: &str = "stop-rc.log";

/// pane の shell の pid（`#{pane_pid}`・器の入口と同じ typed な読み・歯の側で撃つ）。
fn stop_pane_pid(place: &AcctPlace, name: &str) -> Option<u32> {
    let out = tmux(&place.socket, &["list-panes", "-t", name, "-F", "#{pane_pid}"]);
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// `parent` の直下の子の pid（`/proc/*/stat` の ppid を**歯の側で**読む＝器の読み手を使わない・複数なら最大）。
fn stop_child_of(parent: u32) -> Option<u32> {
    let ppid_of = |pid: u32| {
        let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        text.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse::<u32>().ok()
    };
    fs::read_dir("/proc")
        .ok()?
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| ppid_of(*pid) == Some(parent))
        .max()
}

/// pane の shell の直下の子（[`stop_pane_pid`] → [`stop_child_of`]）。
fn stop_child(place: &AcctPlace, name: &str) -> Option<u32> {
    stop_pane_pid(place, name).and_then(stop_child_of)
}

/// pid が `/proc` に在るか（回収された process は無い）。
fn stop_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// 退避して止まり、`/exit` が通らない席の fixture（dialog の型）: [`exit_parked`] と同じ入口 (1)(2) と退避物の後、pane の
/// shell に `printf '❯ '; <child>; echo $? >> stop-rc.log` を打つ。`child` は stdin を読まない（`/exit` は tty に溜まり誰も
/// 読まない＝前面は shell に戻らない）。前面が `sleep` になるまで待つ（`exec` の後を確かめる・撃つ前の待ち）。
fn stop_parked(place: &AcctPlace, name: &str, child: &str) -> IsolatedSeat {
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    acct_parked(place, name, &acct_launcher(place, name), 30);
    wm_file(&place.wm, "working-memory.parked.md", name);
    let line = format!("printf '\\342\\235\\257 '; {child}; echo $? >> '{}'", place.dir.join(STOP_RC_LOG).display());
    assert!(tmux(&place.socket, &["send-keys", "-t", name, "-l", &line]).status.success());
    assert!(tmux(&place.socket, &["send-keys", "-t", name, "Enter"]).status.success());
    assert!(exit_wait_foreground(place, name, "sleep"), "前面が sleep の席を作れる");
    guard
}

/// 1 周目（第 1 手）: `/exit` を送るが前面は shell にならず `exit-unconfirmed`（rc 1）・exit-stamp が打たれる。stamp の mtime を返す。
fn stop_first_round(place: &AcctPlace, name: &str) -> SystemTime {
    let first = acct_tick(place, name, None);
    let line = stderr_of(&first);
    assert_eq!(rc_of(&first), i32::from(RC_REFUSED), "1 周目: stdout={} stderr={line}", stdout_of(&first));
    assert_eq!(tick_token(&line, "reason").as_deref(), Some("exit-unconfirmed"), "1 周目: {line}");
    let stamp = seat_dir_of(&place.state, name).join("exit-stamp");
    assert!(stamp.exists(), "1 周目: exit-stamp を打つ");
    mtime_of(&stamp)
}

/// pane の shell が子の `$?` を写すまで待って読む（子が止まった**後**の shell の後始末を待つ・停止そのものの待ちではない）。
fn stop_rc(place: &AcctPlace) -> String {
    let path = place.dir.join(STOP_RC_LOG);
    let deadline = Instant::now().checked_add(PROMPT_WAIT);
    while deadline.is_some_and(|at| Instant::now() < at) {
        let text = fs::read_to_string(&path).unwrap_or_default();
        if !text.is_empty() {
            return text;
        }
        sleep(Duration::from_millis(100));
    }
    String::new()
}

/// 停止して終了を確定した周の判定行と記録: `decision=inject kind=exit detail=terminated consumed=true`・exit-stamp は back-off の
/// 内側（`none` でない）・`inject.jsonl` の最新行が同じ字面（立て直しの入口 (1) の読み先）。
fn stop_assert_terminated(place: &AcctPlace, name: &str, out: &Output) {
    let line = stdout_of(out);
    assert_eq!(rc_of(out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(out));
    for (key, want) in [("decision", "inject"), ("kind", "exit"), ("detail", "terminated"), ("consumed", "true")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert!(tick_token(&line, "exit-stamp").is_some_and(|age| age != "none"), "back-off の内側: {line}");
    assert!(line.contains(" kind=exit detail=terminated "), "detail は kind の直後: {line}");
    let log = fs::read_to_string(place.state.join("inject.jsonl")).unwrap_or_default();
    let last = log.lines().last().unwrap_or_default();
    assert_eq!(acct_text(last, "who").as_deref(), Some("seat-tick"), "{log}");
    assert_eq!(acct_text(last, "seat").as_deref(), Some(name), "{log}");
    assert!(acct_text(last, "what").is_some_and(|what| what.contains(" kind=exit detail=terminated ")), "判定行と同じ字面: {log}");
}

/// (a) `/exit` が通らない席（前面の子 `sleep` は stdin を読まない＝dialog の型）: 1 周目は `exit-unconfirmed`・exit-stamp あり。
/// 2 周目（stamp が閾値未満・前面が shell でない）は `/exit` を再送せず pane の shell の子を TERM で止め、判定行は
/// `kind=exit detail=terminated`・子は唯一の wait で待ってから返るので `/proc` に無い（歯は poll しない）・shell が写した
/// `$?` は 143（= TERM）・exit-stamp は打ち直さない。base は 2 周目が `cycle-recent` の noop で子が生きたまま（RED）。
#[test]
fn seat_exit_stop_terminates_the_child_with_term_when_exit_is_unconfirmed() {
    let place = acct_place();
    let name = "stopterm";
    let guard = stop_parked(&place, name, "sleep 30");
    let child = stop_child(&place, name).unwrap_or_default();
    assert!(child >= 2, "pane の shell の子 sleep が取れる");
    let stamped_at = stop_first_round(&place, name);
    assert!(stop_alive(child), "1 周目: 子は生きている（/exit は通らない）");

    let out = acct_tick(&place, name, None);

    stop_assert_terminated(&place, name, &out);
    assert!(!stop_alive(child), "2 周目: 子は消えてから返る");
    assert_eq!(stop_rc(&place), "143\n", "shell が写した $? は 143 = TERM で止まった");
    assert_eq!(mtime_of(&seat_dir_of(&place.state, name).join("exit-stamp")), stamped_at, "停止の周は exit-stamp を打ち直さない");
    stop_assert_exit_sent_once(&place, name);
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// 席の記録は 退避の合図 → `/exit` の 2 行のまま（第 2 手は 1 key も送らない＝`/exit` を再送しない）。
fn stop_assert_exit_sent_once(place: &AcctPlace, name: &str) {
    let sent = acct_sent(&place.state, name);
    assert_eq!(sent.len(), 2, "退避の合図・/exit の 2 行だけ: {sent:?}");
    assert_eq!(sent.get(1).map(String::as_str), Some("/exit"), "2 行目は 1 周目の /exit: {sent:?}");
}

/// (b) TERM を無視する子（`trap '' TERM` を `exec` で継いだ `sleep`・前面が `sleep` になってから撃つ＝SIG_IGN の継承が確定）:
/// 2 周目は TERM → 猶予（`pipe.stop_grace_ms`）→ KILL で止め、判定行は同じ `detail=terminated`・shell が写した `$?` は
/// 137（= KILL）。base は `cycle-recent` の noop（RED）。
#[test]
fn seat_exit_stop_kills_the_child_that_ignores_term() {
    let place = acct_place();
    let name = "stopkill";
    let guard = stop_parked(&place, name, "sh -c 'trap \"\" TERM; exec sleep 30'");
    let child = stop_child(&place, name).unwrap_or_default();
    assert!(child >= 2, "pane の shell の子 sleep が取れる");
    let stamped_at = stop_first_round(&place, name);
    assert!(stop_alive(child), "1 周目: 子は生きている");

    let out = acct_tick(&place, name, None);

    stop_assert_terminated(&place, name, &out);
    assert!(!stop_alive(child), "2 周目: 子は消えてから返る");
    assert_eq!(stop_rc(&place), "137\n", "shell が写した $? は 137 = KILL で止まった（TERM は無視された）");
    assert_eq!(mtime_of(&seat_dir_of(&place.state, name).join("exit-stamp")), stamped_at, "停止の周は exit-stamp を打ち直さない");
    stop_assert_exit_sent_once(&place, name);
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) pane の shell の子が取れない席（shell 自身が `exec sleep` に置き換わり、前面は shell でないが直下の子が無い）: 2 周目は
/// 撃たず `decision=error reason=exit-pane-missing`（rc 1）・pane の process は生きたまま・`/exit` も再送しない・exit-stamp も
/// 打ち直さない。base は `cycle-recent` の noop（RED）。
#[test]
fn seat_exit_stop_refuses_when_the_pane_has_no_child() {
    let place = acct_place();
    let name = "stopnochild";
    let guard = stop_parked(&place, name, "exec sleep 30");
    let pane = stop_pane_pid(&place, name).unwrap_or_default();
    assert!(pane >= 2, "pane の pid が取れる");
    assert_eq!(stop_child(&place, name), None, "pane の process に子は無い");
    let stamped_at = stop_first_round(&place, name);

    let out = acct_tick(&place, name, None);

    let line = stderr_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={} stderr={line}", stdout_of(&out));
    for (key, want) in [("decision", "error"), ("reason", "exit-pane-missing")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert!(tick_token(&line, "exit-stamp").is_some_and(|age| age != "none"), "back-off の内側: {line}");
    assert!(stop_alive(pane), "pane の process は生きたまま");
    assert_eq!(exit_foreground(&place, name), "sleep", "前面は sleep のまま");
    assert_eq!(mtime_of(&seat_dir_of(&place.state, name).join("exit-stamp")), stamped_at, "exit-stamp を打ち直さない");
    stop_assert_exit_sent_once(&place, name);
    assert!(!place.dir.join(STOP_RC_LOG).exists(), "何も止めていない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) 2 周目に前面が shell になっている（`/exit` が遅れて通った・exit-stamp は閾値未満）: 停止せず立て直しの入口へ
/// （`kind=relaunch`・`detail=` 無し・既存 `seat_relaunch_` の歯と同じ結果）。exit-stamp は触られない。
#[test]
fn seat_exit_stop_does_not_stop_a_shell_and_relaunches_instead() {
    let place = acct_place();
    let name = "stopshell";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    acct_parked(&place, name, &acct_launcher(&place, name), 30);
    wm_file(&place.wm, "working-memory.parked.md", name);
    assert!(acct_shell_prompt(&place, name, ""), "shell の prompt を描ける");
    let stamp = seat_dir_of(&place.state, name).join("exit-stamp");
    fs::write(&stamp, "0\n").ok();
    let stamped_at = mtime_of(&stamp);

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("relaunch", ACCT_SPARE), ("cycle-stamp", "none")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(tick_token(&line, "detail"), None, "停止していない: {line}");
    assert_eq!(mtime_of(&stamp), stamped_at, "exit-stamp は触らない");
    acct_assert_launched_then_restored(&place, name);
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (e) exit-stamp が閾値より古い周は第 1 手 `/exit` から（既存の極性）: 前面の子 `sleep` は止めず、`/exit` を送り直して
/// `exit-unconfirmed`（rc 1）・stamp は打ち直される・子は生きたまま・`detail=` 無し。
#[test]
fn seat_exit_stop_resends_exit_when_the_stamp_is_stale() {
    let place = acct_place();
    let name = "stopstale";
    let guard = stop_parked(&place, name, "sleep 30");
    let child = stop_child(&place, name).unwrap_or_default();
    assert!(child >= 2, "pane の shell の子 sleep が取れる");
    let stamp = seat_dir_of(&place.state, name).join("exit-stamp");
    fs::write(&stamp, "0\n").ok();
    backdate(&stamp, STALE_S + 1);
    let stamped_at = mtime_of(&stamp);

    let out = acct_tick(&place, name, None);

    let line = stderr_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={} stderr={line}", stdout_of(&out));
    for (key, want) in [("decision", "error"), ("reason", "exit-unconfirmed")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(tick_token(&line, "detail"), None, "停止していない: {line}");
    assert!(mtime_of(&stamp) > stamped_at, "第 1 手は stamp を打ち直す");
    assert!(stop_alive(child), "子は生きたまま");
    assert_eq!(exit_foreground(&place, name), "sleep", "前面は sleep のまま");
    assert_eq!(acct_sent(&place.state, name).last().map(String::as_str), Some("/exit"), "/exit を送り直した");
    assert!(!place.dir.join(STOP_RC_LOG).exists(), "何も止めていない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}
