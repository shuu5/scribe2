//! 席への注入の歯（`seat inject`・接頭辞 `seat_inject_`）。
//!
//! 消えた `seat/cycle.rs` から**挙動不変で移した**もの（`s2-07l.479.1`）: 作り直しの cycle と終了の手と
//! 立て直しは ADR-0045 §2 (2) で消えたが、**注入の口は残る**（口ごと消すのは `s2-07l.479.3`）ので、その歯を
//! 消える file と一緒に落とさない。共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在る。
// flip-check: moved s2-07l.479.1

use super::*;

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

/// 偽 app が自分で折り返す幅（列）。pane 幅 [`WRAP_WIDTH`] より狭い＝割れるのは app の硬い改行で、
/// tmux の折り返しではない（`capture-pane -J` は結合しない）。
const APP_FOLD_WIDTH: &str = "24";

/// 偽 app の歯の目印: ASCII だけ（`fold` は byte 単位で割るので多 byte を途中で割らない字面にする）・
/// [`APP_FOLD_WIDTH`] より長い 1 行。
const APP_MARKER: &str = ": seat-e2e-appwrap the app folds this echo into hard lines";

/// 偽 app の script（`s2-07l.296`）: tty の echo を切り、起動の合図（`app-ready`・script の字面には無い形に
/// 組む＝echo された script 行との衝突を避ける）と prompt を描き、受けた 1 行ごとに `body` を実行して
/// prompt を描き直す。開発 session の TUI が入力欄と echo を**自分の幅で折り返して描く**形の再現。
fn folding_app(body: &str) -> String {
    format!("stty -echo; printf 'app%sready\\n❯ ' -; while IFS= read -r l; do {body}; printf '❯ '; done")
}

/// 受けた行を [`APP_FOLD_WIDTH`] 列の**硬い改行**で折り返して描く本文。
fn app_folds_echo() -> String {
    format!("printf '%s\\n' \"$l\" | fold -w {APP_FOLD_WIDTH}")
}

/// 受けた行を描かず、別の字面（`app-swallowed`・script の字面には無い形）だけを描く本文。
const APP_SWALLOWS_ECHO: &str = "printf 'app%sswallowed\\n' -";

/// `sh -i` の席の上に偽 app を立て、起動の合図と prompt が描かれるまで待つ（上限 [`PROMPT_WAIT`]）。
fn start_folding_app(socket: &str, name: &str, body: &str) -> bool {
    tmux(socket, &["send-keys", "-t", name, "-l", &folding_app(body)]);
    tmux(socket, &["send-keys", "-t", name, "Enter"]);
    let deadline = Instant::now().checked_add(PROMPT_WAIT);
    while deadline.is_some_and(|at| Instant::now() < at) {
        let pane = capture(socket, name);
        if pane.contains("app-ready") && pane.trim_end().ends_with(PROMPT) {
            return true;
        }
        sleep(Duration::from_millis(100));
    }
    false
}

/// 歯の側で「空白を畳んだ字面」を組む（実装の関数を呼ばない・`split_whitespace` で独立に組む）。
fn squashed(text: &str) -> String {
    text.split_whitespace().collect()
}

/// TUI が入力欄と echo を**自分の幅で折り返して描く**周（目印の途中に硬い改行が入る・`-J` で結合しても
/// 割れたまま）でも、**届いた注入は `delivered` rc 0・記録 1 行**（`s2-07l.296`・base では `absent` rc 1）。
///
/// 実害: 打刻の合図（80 cell）が pane 幅 80 の開発 session で 2 行に割れ、2026-09-14 07:45Z 以降の
/// `kind=pointer` が両席で 1 回も送達しなかった（memo `s2-07l.288`）。
#[test]
fn seat_inject_app_wrapped_marker_is_delivered() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-appwrap";
    let guard = start_seat_sized(&socket, name, "PS1=❯ ", PROMPT, WRAP_WIDTH);
    assert!(guard.ready(), "独立 socket に狭い pane の session を立てられる");
    assert!(start_folding_app(&socket, name, &app_folds_echo()), "偽 app が起動の合図と prompt を描く: {}", capture(&socket, name));
    let state = dir.join("state");
    stamp_idle(&state, name);

    let out = inject_on(&socket, name, &state, APP_MARKER);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!(
            "seat: inject delivered target={name} bytes={} consumed=false{}\n",
            APP_MARKER.len(),
            provenance(&state, "flag")
        ),
        "app が折り返して描いた目印も現れた＝送達"
    );
    // 歯が空虚でないこと（.148 の歯と同じ二重の確認）: 結合しても目印は割れたまま（TUI の折り返しの再現）で、
    // 畳んだ字面には在る。
    let joined = capture_joined(&socket, name);
    assert!(!joined.contains(APP_MARKER), "結合しても app の硬い改行は残る＝目印は割れている: {joined}");
    assert!(joined.contains("seat-e2e-appwrap"), "割れた断片は pane に在る: {joined}");
    assert!(squashed(&joined).contains(&squashed(APP_MARKER)), "畳んだ字面には目印が在る: {joined}");
    let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
    assert_eq!(recorded.lines().count(), 1, "記録は 1 行: {recorded}");
    assert!(recorded.contains(r#""who":"seat-inject""#), "{recorded}");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 畳んで読んでも、**本当に届いていない周は `absent` rc 1 のまま**（極性不変・base でも PASS の負例）。
/// 同じ偽 app に目印を描かせない（送った字面と別の字面を echo する）と、目印は結合した論理行にも
/// 畳んだ字面にも無い——門の断り（busy / unknown-input）や tmux-failed で「別の理由で」落ちていないことを
/// 行の字面で確かめる。
#[test]
fn seat_inject_app_wrapped_absent_line_still_fails_closed() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-appabsent";
    let guard = start_seat_sized(&socket, name, "PS1=❯ ", PROMPT, WRAP_WIDTH);
    assert!(guard.ready(), "独立 socket に狭い pane の session を立てられる");
    assert!(start_folding_app(&socket, name, APP_SWALLOWS_ECHO), "偽 app が起動の合図と prompt を描く: {}", capture(&socket, name));
    let state = dir.join("state");
    stamp_idle(&state, name);

    let out = inject_on(&socket, name, &state, APP_MARKER);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(stdout_of(&out), "", "送達を確認できない周は stdout 0 行");
    assert_eq!(stderr_of(&out), format!("seat: inject unconfirmed reason=absent{}\n", provenance(&state, "flag")));
    let joined = capture_joined(&socket, name);
    assert!(joined.contains("app-swallowed"), "app は受けた行に別の字面を描いた（注入は届いている）: {joined}");
    assert!(!joined.contains("seat-e2e-appwrap"), "目印は結合した論理行にも無い: {joined}");
    assert!(!squashed(&joined).contains(&squashed(APP_MARKER)), "畳んだ字面にも無い: {joined}");
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
