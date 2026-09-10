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
    );
    let transcript = fixture(&dir, "transcript.jsonl", jsonl);
    let out = meter_on(&pane, Some(&transcript));

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: meter used_pct=- used_tokens=7000 window_tokens=- source=jsonl\n",
        "取れない値は - で、0 に化けない"
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

/// pane の末尾が prompt（入力欄が空）で終わっているか。
///
/// 実装の parser を呼ばずに字面で見る（呼ぶと「実装が壊れても歯が同じ壊れ方をする」
/// 自己言及になり、assert が空虚になる）。
fn prompt_is_idle(pane: &str) -> bool {
    pane.trim_end().ends_with(PROMPT)
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
    let out = tmux(
        socket,
        &[
            "new-session", "-d", "-s", name, "-x", "120", "-y", "40", "-e", "PS1=❯ ", "sh", "-i",
        ],
    );
    if !out.status.success() {
        return false;
    }
    let deadline = Instant::now() + PROMPT_WAIT;
    while Instant::now() < deadline {
        if prompt_is_idle(&capture(socket, name)) {
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
    let name = "seat-deliver";
    assert!(start_seat(&socket, name), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    let marker = "seat-e2e-delivered";
    let payload = format!("echo {marker}");

    let out = run_seat(&[
        "inject",
        "--target",
        name,
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
        format!("seat: inject delivered target={name} bytes={}\n", payload.len())
    );
    let pane = capture(&socket, name);
    assert!(pane.contains(marker), "pane に marker が現れる: {pane}");

    let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
    let lines: Vec<&str> = recorded.lines().collect();
    assert_eq!(lines.len(), 1, "記録は 1 行: {recorded}");
    let line = lines.first().copied().unwrap_or_default();
    assert!(line.contains(r#""schema":1"#), "{line}");
    assert!(line.contains(r#""who":"seat-inject""#), "{line}");
    assert!(line.contains(r#""when":"inject""#), "{line}");
    assert!(line.contains(r#""tokens":null"#), "数えていない値は null: {line}");
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

/// `seat` の使い方を snapshot 1 本に固定する（C12.5）。
#[test]
fn seat_external_form() {
    let form = stderr_of(&run_seat(&[]));
    insta::assert_snapshot!(form);
}
