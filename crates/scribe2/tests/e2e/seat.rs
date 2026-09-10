//! `seat meter` / `seat inject` の歯（設計 docs/design/seat-autonomy.md §3）。
//!
//! tmux は **独立 socket**（`-S <tmp>/sock -f /dev/null`）の server だけを撃ち、開発席の
//! live な server には 1 度も触れない。pane の読みは `--capture-file` で本文を直に渡す
//! ので、meter 側の歯は tmux を 1 度も起動しない。

use crate::make_tmp_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread::sleep;
use std::time::{Duration, Instant};
use vessel::cli_outcome::{RC_OK, RC_REFUSED};
use vessel::seat::inject::tick_path;

/// binary の path。
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_scribe2")
}

/// tmp dir を 1 つ作る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn tmp() -> PathBuf {
    make_tmp_dir().expect("tmp dir を作れる")
}

/// `seat` を binary で 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_seat(args: &[&str]) -> Output {
    Command::new(bin())
        .arg("seat")
        .args(args)
        .output()
        .expect("binary を起動できる")
}

/// rc を数で見る（`None` は signal 死＝この歯では起きない）。
fn rc_of(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

/// stdout を文字列で見る。
fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// stderr を文字列で見る。
fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// pane 本文の fixture を 1 つ書き、その path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fixture(dir: &Path, name: &str, body: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, body).expect("fixture を書ける");
    path.display().to_string()
}

/// `seat meter` を capture-file 経由で 1 回撃つ（tmux を呼ばない経路）。
fn meter_on(path: &str, transcript: Option<&str>) -> Output {
    let mut args = vec!["meter", "--target", "unused", "--capture-file", path];
    if let Some(found) = transcript {
        args.push("--transcript");
        args.push(found);
    }
    run_seat(&args)
}

// ─────────────────────────── meter ───────────────────────────

/// statusline は **最後の prompt 行より下**から採る（上の decoy は候補にしない）。
#[test]
fn seat_meter_parses_statusline_below_prompt_anchor() {
    let dir = tmp();
    let pane = "99% 1k/2k Fable\n❯ \n  45% 90k/200k Opus 5 [high]\n";
    let path = fixture(&dir, "pane.txt", pane);
    let out = meter_on(&path, None);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: meter used_pct=45 used_tokens=90000 window_tokens=200000 source=pane\n"
    );
    assert_eq!(stderr_of(&out), "", "成立した周は stderr へ 1 byte も書かない");

    // anchor の負例: 有効な statusline が **prompt より上にしか無い** pane は採らない
    // （「後ろから探す」だけの実装だと、この pane から 45% を拾ってしまう）。
    let above = fixture(&dir, "above.txt", "  45% 90k/200k Opus 5 [high]\n❯ \n");
    let out = meter_on(&above, None);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "prompt より上の statusline は採らない");
    assert_eq!(stdout_of(&out), "");
    assert_eq!(stderr_of(&out), "seat: meter unmeasured reason=no-source\n");

    // 健全性の**内側の境界**（pct = 100 ちょうど・used == window・M 倍率）は採る。
    let edge = fixture(&dir, "edge.txt", "❯ \n  100% 1M/1M Opus 5\n");
    let out = meter_on(&edge, None);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: meter used_pct=100 used_tokens=1000000 window_tokens=1000000 source=pane\n"
    );

    // window の下限ちょうど（100000）も採る。
    let floor = fixture(&dir, "floor.txt", "❯ \n  1% 1k/100k Opus 5\n");
    let out = meter_on(&floor, None);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: meter used_pct=1 used_tokens=1000 window_tokens=100000 source=pane\n"
    );
    fs::remove_dir_all(&dir).ok();
}

/// 健全性（pct ≤ 100 ∧ used ≤ window ∧ window ≥ 100000）を外れた値は流さない。
#[test]
fn seat_meter_rejects_out_of_bound_statusline() {
    let dir = tmp();
    let cases = [
        ("pct.txt", "❯ \n  120% 90k/200k\n"),
        ("used.txt", "❯ \n  45% 300k/200k\n"),
        ("window.txt", "❯ \n  45% 10k/50k\n"),
    ];
    for (name, body) in cases {
        let path = fixture(&dir, name, body);
        let out = meter_on(&path, None);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{name}");
        assert_eq!(stdout_of(&out), "", "{name}: 不成立の周は stdout 0 行");
        assert_eq!(
            stderr_of(&out),
            "seat: meter unmeasured reason=pane-out-of-bound\n",
            "{name}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// pane に statusline が無い周は transcript の **最後の有効な usage** へ落ちる。
///
/// decoy 3 種（sidechain / usage null / 和 0）はいずれも数えない。
#[test]
fn seat_meter_falls_back_to_transcript_last_usage() {
    let dir = tmp();
    let pane = fixture(&dir, "pane.txt", "❯ \n  ready\n");
    let jsonl = concat!(
        r#"{"type":"assistant","isSidechain":false,"message":{"usage":{"input_tokens":200,"cache_creation_input_tokens":300,"cache_read_input_tokens":6500}}}"#,
        "\n",
        r#"{"type":"assistant","isSidechain":true,"message":{"usage":{"input_tokens":999999,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        "\n",
        r#"{"type":"assistant","isSidechain":false,"message":{"usage":null}}"#,
        "\n",
        r#"{"type":"assistant","isSidechain":false,"message":{"usage":{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        "\n",
        // **最後の**有効 entry（これが採られる＝先頭の 7000 ではない）。
        r#"{"type":"assistant","isSidechain":false,"message":{"usage":{"input_tokens":10,"cache_creation_input_tokens":20,"cache_read_input_tokens":70}}}"#,
        "\n",
    );
    let transcript = fixture(&dir, "transcript.jsonl", jsonl);
    let out = meter_on(&pane, Some(&transcript));

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: meter used_pct=- used_tokens=100 window_tokens=- source=jsonl\n",
        "最後の有効な和を採る（先頭の 7000 ではない）・取れない値は - で 0 に化けない"
    );
    fs::remove_dir_all(&dir).ok();
}

/// 出所が 1 つも成立しない周は理由つきで不成立になる（0% に化けない）。
///
/// 契約の pin は (a)。(b) は同じ「statusline 無し」でも **pane 本文は在る**周で、
/// 理由語を `no-source` と弁別する（読みの SSOT = 設計 §3 の箇条）。
#[test]
fn seat_meter_unmeasured_without_any_source() {
    let dir = tmp();
    let empty = fixture(&dir, "empty.txt", "");
    let out = meter_on(&empty, None);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stdout_of(&out), "");
    assert_eq!(stderr_of(&out), "seat: meter unmeasured reason=no-source\n");

    let noisy = fixture(&dir, "noisy.txt", "❯ \n  building…\n  ready\n");
    let out = meter_on(&noisy, None);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stdout_of(&out), "");
    assert_eq!(
        stderr_of(&out),
        "seat: meter unmeasured reason=pane-no-statusline\n"
    );
    fs::remove_dir_all(&dir).ok();
}

// ─────────────────────────── inject ───────────────────────────

/// 記録 file の path を**契約の字面から**組む（実装の helper を使わない）。
fn tick_file(state: &Path, target: &str) -> PathBuf {
    state.join("seat").join(target).join("tick.jsonl")
}

/// 開発 session の入力欄を指す prompt の字。
const PROMPT: char = '❯';

/// 独立 socket の path。
fn socket_of(dir: &Path) -> String {
    dir.join("sock").display().to_string()
}

/// 独立 socket の tmux を 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn tmux(socket: &str, args: &[&str]) -> Output {
    Command::new("tmux")
        .args(["-S", socket, "-f", "/dev/null"])
        .args(args)
        .output()
        .expect("tmux を起動できる")
}

/// pane 本文を読む。
fn capture(socket: &str, target: &str) -> String {
    String::from_utf8_lossy(&tmux(socket, &["capture-pane", "-p", "-t", target]).stdout).into_owned()
}

/// 独立 socket に `sh -i` の session を 1 つ立て、prompt が描かれたら `true`。
///
/// **live server には触れない**（socket は tmp・設定は `-f /dev/null`）。判定を返すのは
/// helper で panic しないためで、落とすのは呼び側の `#[test]` である。
fn start_seat(socket: &str, name: &str) -> bool {
    start_seat_with(socket, name, "PS1=❯ ", PROMPT)
}

/// prompt の字を選んで session を立てる（`❯` を持たない席も作れる）。
fn start_seat_with(socket: &str, name: &str, ps1: &str, needle: char) -> bool {
    let out = tmux(
        socket,
        &[
            "new-session", "-d", "-s", name, "-x", "120", "-y", "40", "-e", ps1, "sh", "-i",
        ],
    );
    if !out.status.success() {
        return false;
    }
    let deadline = Instant::now() + PROMPT_WAIT;
    while Instant::now() < deadline {
        if capture(socket, name).trim_end().ends_with(needle) {
            return true;
        }
        sleep(Duration::from_millis(100));
    }
    false
}

/// prompt が描かれるのを待つ上限。
const PROMPT_WAIT: Duration = Duration::from_secs(5);

/// session を畳む（server は session が尽きると自分で終わる・`kill-server` は使わない）。
fn stop_seat(socket: &str, name: &str) {
    tmux(socket, &["kill-session", "-t", name]);
}

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
    assert!(start_seat(&socket, name), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
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
        format!("seat: inject delivered target={sanitized} bytes={}\n", payload.len())
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
        "何を送ったかを残す: {line}"
    );
    assert!(
        line.contains(&format!(r#""bytes":{}"#, payload.len())),
        "byte 数は payload の byte 長: {line}"
    );
    stop_seat(&socket, name);
    fs::remove_dir_all(&dir).ok();
}

/// 入力欄が非空の周は **1 key も送らない**（人間の打ちかけと merge しない）。
#[test]
fn seat_inject_refuses_when_input_line_is_busy() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-busy";
    assert!(start_seat(&socket, name), "独立 socket に prompt 付きの session を立てられる");
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
    assert_eq!(stderr_of(&out), "seat: inject refused reason=busy\n");
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
    stop_seat(&socket, name);
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
        "seat: inject unconfirmed reason=tmux-failed\n"
    );
    assert!(!state.exists(), "1 byte も書かない");
    fs::remove_dir_all(&dir).ok();
}

/// 送ったが**入力欄が空にならない**周は `residual` で断る（送達判定の連言の片方）。
///
/// 引用の閉じない payload は sh が継続 prompt へ移り、prompt 行の右に打鍵が残る。
/// 「pane に現れた」だけで送達と数える実装だと、この周が rc 0 に化ける。
#[test]
fn seat_inject_reports_residual_when_input_line_stays_dirty() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-residual";
    assert!(start_seat(&socket, name), "独立 socket に prompt 付きの session を立てられる");
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
        "echo 'unterminated",
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stdout_of(&out), "", "送達していない周は stdout 0 行");
    assert_eq!(
        stderr_of(&out),
        "seat: inject unconfirmed reason=residual\n"
    );
    assert!(
        !tick_file(&state, name).exists(),
        "送達を確認できない周は記録しない"
    );
    stop_seat(&socket, name);
    fs::remove_dir_all(&dir).ok();
}

/// prompt 行を特定できない pane へは **1 key も送らない**（fail-closed）。
#[test]
fn seat_inject_refuses_when_prompt_is_not_locatable() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-noprompt";
    // `❯` を持たない席（入力欄の位置が読めない）。
    assert!(
        start_seat_with(&socket, name, "PS1=$ ", '$'),
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
        "seat: inject refused reason=unknown-input\n"
    );
    let pane = capture(&socket, name);
    assert!(!pane.contains(marker), "marker は 1 度も現れない: {pane}");
    assert!(!tick_file(&state, name).exists(), "記録も書かない");
    stop_seat(&socket, name);
    fs::remove_dir_all(&dir).ok();
}

/// 空の payload は使い方の誤りとして断る（空の口を「送った」に化けさせない）。
#[test]
fn seat_inject_refuses_empty_payload() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seat-empty";
    assert!(start_seat(&socket, name), "独立 socket に prompt 付きの session を立てられる");
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
    stop_seat(&socket, name);
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

/// `seat` の使い方を snapshot 1 本に固定する（C12.5）。
#[test]
fn seat_external_form() {
    let form = stderr_of(&run_seat(&[]));
    insta::assert_snapshot!(form);
}
