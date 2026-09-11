//! `seat meter` / `seat inject` の歯（設計 docs/design/seat-autonomy.md §3）。
//!
//! tmux は **独立 socket**（`-S <tmp>/sock -f /dev/null`）の server だけを撃ち、開発席の
//! live な server には 1 度も触れない。pane の読みは `--capture-file` で本文を直に渡す
//! ので、meter 側の歯は tmux を 1 度も起動しない。

use crate::make_tmp_dir;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};
use vessel::cli_outcome::{RC_OK, RC_REFUSED};
use vessel::rules::manifest::Manifest;
use vessel::seat::inject::tick_path;
use vessel::seat::meter::window_of;

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

/// **transcript が名指された周は transcript を見る**（健全な statusline が在っても）。
///
/// 出所を入力で決める 1 本道にするための歯である——pane 一次のままだと、hook の中で pane を
/// 持てない guard（C2.2）と meter が同じ席の同じ瞬間に違う値を返す。値は最後の有効な usage で、
/// decoy 3 種（sidechain / usage null / 和 0）はいずれも数えない。
#[test]
fn seat_meter_reads_the_named_transcript_over_the_pane() {
    let dir = tmp();
    // pane 側は**健全な statusline**（90%）。transcript が勝つので、この値は出ない。
    let pane = fixture(&dir, "pane.txt", "❯ \n  90% 900k/1M Opus 5\n");
    let jsonl = concat!(
        r#"{"type":"assistant","isSidechain":false,"message":{"usage":{"input_tokens":200,"cache_creation_input_tokens":300,"cache_read_input_tokens":6500}}}"#,
        "\n",
        r#"{"type":"assistant","isSidechain":true,"message":{"usage":{"input_tokens":999999,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        "\n",
        r#"{"type":"assistant","isSidechain":false,"message":{"usage":null}}"#,
        "\n",
        r#"{"type":"assistant","isSidechain":false,"message":{"usage":{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        "\n",
        // **最後の**有効 entry（これが採られる＝先頭の 7000 ではない）。宣言窓 1000000 に対して
        // 250000 = 25% で、**使用率が 0 でも 100 でもない**値になる形にしてある（0% は
        // 「割っていない実装」でも通ってしまう）。
        r#"{"type":"assistant","isSidechain":false,"message":{"usage":{"input_tokens":30000,"cache_creation_input_tokens":70000,"cache_read_input_tokens":150000}}}"#,
        "\n",
    );
    let transcript = fixture(&dir, "transcript.jsonl", jsonl);
    let out = meter_on(&pane, Some(&transcript));

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: meter used_pct=25 used_tokens=250000 window_tokens=1000000 source=jsonl+rules\n",
        "最後の有効な和を宣言窓で割った実値が載る（pane の 90% ではない）"
    );
    fs::remove_dir_all(&dir).ok();
}

/// **空の `--transcript` は「渡していない」と同じ**（trim 後）。
///
/// 空の口をそのまま path として扱うと、渡し忘れが `unreadable`（file が壊れている）に化け、
/// 健全な pane が在るのに不成立になる＝記録から原因を取り違える。
#[test]
fn seat_meter_treats_empty_transcript_as_absent() {
    let dir = tmp();
    let pane = fixture(&dir, "pane.txt", "❯ \n  90% 900k/1M Opus 5\n");
    let out = meter_on(&pane, Some("   "));
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: meter used_pct=90 used_tokens=900000 window_tokens=1000000 source=pane\n",
        "空の口は渡し忘れと同じ＝pane を読む（unreadable に化けない）"
    );
    fs::remove_dir_all(&dir).ok();
}

/// transcript が名指されたのに測れない周は、**guard の記録と同じ語**で不成立になる。
///
/// 同じ条件を 2 面が別の語で呼ぶと（本便より前の `jsonl-no-usage` と `no-usage`）、記録と
/// CLI を突き合わせたときに同じ事象が別物に見える。語は 2 段を畳まない（file が読めないのか、
/// 有効な usage が無いのか）。
#[test]
fn seat_meter_names_the_same_unmeasured_reasons_as_the_guard() {
    let dir = tmp();
    let pane = fixture(&dir, "pane.txt", "❯ \n  90% 900k/1M Opus 5\n");
    let empty = fixture(&dir, "empty.jsonl", "");
    for (transcript, reason) in [(format!("{empty}-nope"), "unreadable"), (empty.clone(), "no-usage")] {
        let out = meter_on(&pane, Some(&transcript));
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "測れない周は rc≠0: {reason}");
        assert_eq!(stdout_of(&out), "", "不成立に stdout は出さない: {reason}");
        assert_eq!(
            stderr_of(&out),
            format!("seat: meter unmeasured reason={reason}\n"),
            "guard の記録と同じ語で名乗る（pane の 90% へ逃げない）"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

/// 窓の宣言が**不発効**か**0** なら窓を引けない（＝割らずに不成立へ倒す側）。
///
/// 埋め込みの manifest では起こらないが、3 つの述語（行の有無 / `enabled` / `> 0`）を測る口が
/// 無いと、どれを外しても歯が落ちない。`Manifest::parse` へ fixture を渡して動かす
/// （`tests/e2e/fleet.rs` の `LockPolicy::from_rules` と同型）。
#[test]
fn seat_meter_refuses_a_window_row_that_is_off_or_zero() {
    let row = |extra: &str, value: u64| {
        format!(
            "schema = 1\n\n[[rule]]\nid = \"seat.context_window_tokens\"\nkind = \"SeatContextWindowTokens\"\nvalue = {value}\n{extra}ruling = \"r\"\nruled_at = \"d\"\n"
        )
    };
    let live = Manifest::parse(&row("", 1_000_000)).expect("fixture を読める");
    assert_eq!(window_of(&live), Some(1_000_000), "発効した正の行は引ける");
    let off = Manifest::parse(&row("enabled = false\n", 1_000_000)).expect("fixture を読める");
    assert_eq!(window_of(&off), None, "不発効の行は引かない（値は在っても使わない）");
    let zero = Manifest::parse(&row("", 0)).expect("fixture を読める");
    assert_eq!(window_of(&zero), None, "0 は引かない（0 で割らない）");
    let absent = Manifest::parse("schema = 1\n").expect("fixture を読める");
    assert_eq!(window_of(&absent), None, "行そのものが無い周も引かない");
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

/// 独立 socket の session を畳む RAII guard。
///
/// **panic 経路でも drop が走る**のが要点である: 明示の後始末は assert が落ちた周に
/// 飛ばされるので、隔離 socket の server（と子の `sh`）が残り続けた——`flip-check` の
/// base overlay のように「わざと RED」を撃つたびに増える（実測 2026-09-10: 49 本）。
struct IsolatedSeat {
    /// 独立 socket の path。
    socket: String,
    /// session 名。
    name: String,
    /// prompt が描かれたか。
    ready: bool,
}

impl IsolatedSeat {
    /// prompt が描かれたか。**落とすのは呼び側の `#[test]`** で helper では panic しない。
    fn ready(&self) -> bool {
        self.ready
    }
}

impl Drop for IsolatedSeat {
    /// session を畳む（server は session が尽きると自分で終わる・`kill-server` は使わない）。
    ///
    /// ★**socket file を消した後では届かない**（実測 2026-09-10: `error connecting` で
    /// rc 1・server は生き残る）。ゆえに歯の成功経路では `fs::remove_dir_all` の**前**に
    /// 明示 `drop` する——panic 経路では remove が飛ぶので、この drop が最後の砦になる。
    ///
    /// ★**ここでは panic しない**: `tmux` を spawn できない周（PATH に無い等）に
    /// [`tmux`] の `expect` を通すと、cleanup 中の drop で二重 panic になり **abort** する
    /// ——読める失敗が SIGABRT へ化ける（実測 2026-09-10: rc 134・`panic in a destructor
    /// during cleanup`）。spawn の失敗は捨てる（その周は server がそもそも立っていない）。
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-S", &self.socket, "-f", "/dev/null", "kill-session", "-t", &self.name])
            .output();
    }
}

/// 独立 socket に `sh -i` の session を 1 つ立て、畳む guard を返す。
///
/// **live server には触れない**（socket は tmp・設定は `-f /dev/null`）。判定を guard へ
/// 載せて返すのは helper で panic しないためで、落とすのは呼び側の `#[test]` である。
fn start_seat(socket: &str, name: &str) -> IsolatedSeat {
    start_seat_with(socket, name, "PS1=❯ ", PROMPT)
}

/// prompt の字を選んで session を立てる（`❯` を持たない席も作れる）。
fn start_seat_with(socket: &str, name: &str, ps1: &str, needle: char) -> IsolatedSeat {
    // guard を**先に**作る: `new-session` が通った後で prompt を待つ間に panic しても畳む。
    let mut seat = IsolatedSeat {
        socket: socket.to_owned(),
        name: name.to_owned(),
        ready: false,
    };
    let out = tmux(
        socket,
        &[
            "new-session", "-d", "-s", name, "-x", "120", "-y", "40", "-e", ps1, "sh", "-i",
        ],
    );
    if !out.status.success() {
        return seat;
    }
    let deadline = Instant::now() + PROMPT_WAIT;
    while Instant::now() < deadline {
        if capture(socket, name).trim_end().ends_with(needle) {
            seat.ready = true;
            return seat;
        }
        sleep(Duration::from_millis(100));
    }
    seat
}

/// prompt が描かれるのを待つ上限。
const PROMPT_WAIT: Duration = Duration::from_secs(5);

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
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
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
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
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
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
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
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
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
        "seat: inject refused reason=unknown-input\n"
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

/// `seat` の使い方を snapshot 1 本に固定する（C12.5）。
#[test]
fn seat_external_form() {
    let form = stderr_of(&run_seat(&[]));
    insta::assert_snapshot!(form);
}

// ─────────────────── heartbeat / tick / cycle ───────────────────

/// idle な pane（入力欄が空・探索域に走行中の印が無い）。
const IDLE_PANE: &str = "❯ \n  10% 100k/1M Opus 5\n";
/// 走行中の印が**入力欄より下**（statusline の位置）に在る pane。
const BUSY_BELOW: &str = "❯ \n  10% 100k/1M Opus 5 (esc to interrupt)\n";
/// 走行中の印が**入力欄より上**に在る pane（実際の席の spinner はこの位置に出る形がある）。
///
/// prompt より下だけを見る実装はこの pane を idle と読み、`/clear` を送ってしまう
/// （実測 2026-09-10・lens-384 C-1）。
const BUSY_ABOVE: &str = "✻ Thinking… (23s · esc to interrupt)\n❯ \n";
/// prompt を 1 行も持たない pane（入力欄の位置が読めない）。
const NO_PROMPT: &str = "$ \n  10% 100k/1M Opus 5\n";
/// 走行中の実席（2026-09-11・匿名化済み）: spinner 行は `esc to interrupt` を**持たず**、
/// 下から 8 非空行目に在る。この pane を idle と読んで `/clear` が送られた（bd `s2-07l.94`）。
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
/// API の再試行中の実席（2026-09-11・匿名化済み）: spinner の形を取らないが走っている。
const RETRYING_PANE: &str = concat!(
    "  queue された入力の写し\n",
    "✻ API error · Retrying in 0s · attempt 1/10\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// 走行中の印が statusline の**下から 4 行目**（旧 `TAIL_LINES` の域の外）に在る pane。
///
/// 下は全行を見る（statusline の高さに依らない）ことを、上の域の数（6）とは別に測る。
const BUSY_FAR_BELOW: &str = concat!(
    "❯ \n",
    "  line 1\n",
    "  line 2\n",
    "  line 3\n",
    "  line 4\n",
    "  line 5\n",
    "  line 6\n",
    "  10% 100k/1M Opus 5 (esc to interrupt)\n",
);
/// `/clear` を送った直後で**まだ作り直されていない**席（echo された `/clear` が次の prompt の
/// 上に在り、statusline 3 行で末尾 6 非空行の外へ押し出されている）。
///
/// 末尾 6 非空行で見る作り直し確認はこの pane を「済んだ」と読み、復元を送ってしまう。
const CLEAR_ECHO_PANE: &str = concat!(
    "❯ /clear\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// 現行版の statusline（区切り 2 行 + 3 行・匿名化済み）。fixture の末尾に共通で付ける。
const TALL_TAIL: &str = concat!(
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// todo が走っている周の spinner: 語が **空白入りの文**（todo の activeForm）になる
/// （本体 2.1.268 の実装: `overrideMessage ?? todo の activeForm ?? 乱択の 1 語`・lens-94 HIGH-1）。
const TODO_SPINNER_PANE: &str = concat!(
    "✻ Reviewing the seat idle predicate… (2m 3s · ↓ 12k tokens)\n",
    "  ⎿  Tip: Use /btw to ask a quick side question\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// API 応答待ちの banner（`retryStatus.kind == stalled`）: spinner 行ごと置き換わり `Retrying in`
/// も `esc to interrupt` も無い（lens-94 HIGH-2）。
const STALLED_PANE: &str = concat!(
    "✻ Waiting for API response · will retry in 3s · check your network\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// API が応答しない周の banner（2 行）。
const NO_RESPONSE_PANE: &str = concat!(
    "✽ No response from the API after 1m · retrying, waiting up to 2m · attempt 1/10\n",
    "  A proxy or gateway that buffers responses may be the cause\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// turn 開始から 16 秒未満の spinner: 括弧（経過・token）がまだ描かれない（lens-94 MEDIUM-4）。
const BARE_SPINNER_PANE: &str = concat!(
    "✻ Sublimating…\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// compaction 中: `…` すら無い専用の行。
const COMPACTING_PANE: &str = concat!(
    "✻ Compacting conversation\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// idle な席の本文が印に**似た形**を持つ pane: 行頭 `●`（assistant 本文）+ `… (3 件)`。
///
/// 席は idle で出力を出さないので、これを busy と読むと tick は永久に noop になる
/// （lens-94 HIGH-3）。`●` は spinner の frame（· ✢ ✳ ✶ ✻ ✽ *）に無い。
const IDLE_COUNT_PANE: &str = concat!(
    "● 直した… (3 件)\n",
    "- 対応済… (2 件)\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  19% 190k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// idle な席の本文が `esc to interrupt` を**引用**している pane（本 bead の報告文がこの形）。
///
/// この字は statusline（prompt より下）でだけ印として読む。
const IDLE_QUOTE_PANE: &str = concat!(
    "● 現行版の spinner は `esc to interrupt` を持たない。\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
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
/// `seat.tick_stale_s` の宣言値（`rules/manifest.toml`）。歯はこの値の**両側**を撃つ。
const STALE_S: u64 = 2400;
/// `seat.cycle_lock_ttl_s` の宣言値。歯はこの値の**両側**を撃つ。
const TTL_S: u64 = 900;

/// file の mtime を `secs` 秒だけ過去へ倒す。
///
/// 経過を**時計の粒度に依存せず**作る（`sleep` で待つ歯は遅く、粒度の粗い fs では進まない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn backdate(path: &Path, secs: u64) {
    let file = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("marker を開ける");
    let at = SystemTime::now()
        .checked_sub(Duration::from_secs(secs))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    fs::File::set_modified(&file, at).expect("mtime を倒せる");
}

/// file の mtime（読めなければ epoch）。
fn mtime_of(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// 退避物を 1 つ置く（frontmatter で席を名乗る・FR23 / 裁定 (c)）。
fn wm_file(dir: &Path, name: &str, seat: &str) -> PathBuf {
    let path = dir.join(name);
    fs::create_dir_all(dir).ok();
    fs::write(&path, format!("---\nseat: {seat}\n---\n\n## 計画弧\n- 続き\n")).ok();
    path
}

/// 退避物**ではない** file を 3 つ置く（数えたら `/clear` の根拠が水増しされる形）。
///
/// 名前の前置きが違う `.md`／`.consumed.md`／`seat:` が frontmatter でなく**本文**に在る WM。
///
/// ★前置きが違う decoy は **18 文字以上の長い名前**にする。短い名前（旧 `notes.md` = 8 文字）
/// だと前置きの条件を外す変異が長さの条件（最短形 18 文字）で偶然落ちるので、前置きの歯が
/// 空虚になる——長さの条件は負例 1 本（`seat_tick_ignores_short_wm_like_names`）で単独に測る。
fn wm_decoys(dir: &Path, seat: &str) {
    wm_file(dir, "session-notes-archive-2026.md", seat);
    wm_file(dir, "working-memory.old.consumed.md", seat);
    // frontmatter を持たず、**本文の行頭**に名乗りが在る形（anchor を外すと拾ってしまう）。
    fs::write(
        dir.join("working-memory.body.md"),
        format!("# 見出し\n\nseat: {seat}\n"),
    )
    .ok();
}

/// 席の置き場（`<state>/seat/<潰した target>/`）。
fn seat_dir_of(state: &Path, target: &str) -> PathBuf {
    state.join("seat").join(target)
}

/// PATH の先頭に「呼ばれたら印を残して失敗する tmux」を置いて `seat` を 1 回撃つ。
///
/// 「1 key も送らない」「tmux を叩かない」は**触れたら分かる形**でしか測れない: 存在しない
/// socket を渡すだけだと、tmux を撃って失敗した周と、そもそも撃たなかった周が同じ結果になる。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_seat_probed(dir: &Path, args: &[&str]) -> (Output, bool) {
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("shim の dir を作れる");
    let mark = dir.join("tmux-called");
    let shim = bin_dir.join("tmux");
    fs::write(
        &shim,
        format!("#!/bin/sh\necho called >> '{}'\nexit 1\n", mark.display()),
    )
    .expect("shim を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("shim に実行権を付ける");
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new(bin())
        .arg("seat")
        .args(args)
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    (out, mark.exists())
}

/// cwd を指定して `seat` を 1 回撃つ（置き場を git 設定から解く経路を測る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_seat_in(cwd: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .arg("seat")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("binary を起動できる")
}

/// `/clear` を受けたら**画面を作り直す** fake な席を独立 socket に立てる。
///
/// 実際の席は `/clear` で pane を消すが `sh` は消さない。`sh` のままだと「作り直せた」を
/// 測る歯が、確認をどう実装しても通る（実測: 確認を常に真へ倒す変異が全歯 GREEN で生存）。
/// 受け取った行は `log` へ 1 行ずつ積むので、**送った順序は pane の描画でなく席が受けた行**で
/// 測れる。`mute_after_clear` の席は作り直した後に echo を止める＝**作り直しは確認できるが
/// 復元の送達は確認できない**周（`restore-unconfirmed`）を作る。
fn start_clearing_seat(
    socket: &str,
    name: &str,
    log: &Path,
    mute_after_clear: bool,
) -> IsolatedSeat {
    let (after_clear, on_other) = if mute_after_clear {
        ("stty -echo 2>/dev/null", ":")
    } else {
        (":", "printf 'seat got %s\\n' \"$line\"")
    };
    let script = format!(
        "while :; do printf '❯ '; read -r line || exit 0; printf '%s\\n' \"$line\" >> '{}'; \
         case \"$line\" in '/clear') printf '\\033[2J\\033[3J\\033[H'; {after_clear} ;; \
         *) {on_other} ;; esac; done",
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

/// prompt が描かれるのを待つ。
fn wait_prompt(socket: &str, name: &str) -> bool {
    let deadline = Instant::now().checked_add(PROMPT_WAIT);
    while deadline.is_some_and(|at| Instant::now() < at) {
        if capture(socket, name).trim_end().ends_with(PROMPT) {
            return true;
        }
        sleep(Duration::from_millis(100));
    }
    false
}

/// 打刻は無ければ作り、2 回目で mtime が進む。空の `--state-dir` は使い方の誤りとして断る。
#[test]
fn seat_heartbeat_touches_seat_file() {
    let dir = tmp();
    let state = dir.join("state");
    let state_s = state.display().to_string();
    // `:` と `.` を含む実際の target の形で撃つ（置き場の名前は潰した字面になる）。
    let marker = seat_dir_of(&state, "seatbeat_0.0").join("heartbeat");

    let out = run_seat(&["heartbeat", "--target", "seatbeat:0.0", "--state-dir", &state_s]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), "seat: heartbeat target=seatbeat_0.0\n");
    assert_eq!(stderr_of(&out), "", "成立した周は stderr へ 1 byte も書かない");
    assert!(marker.exists(), "打刻 file が在る");

    backdate(&marker, 600);
    let before = mtime_of(&marker);
    let out = run_seat(&["heartbeat", "--target", "seatbeat:0.0", "--state-dir", &state_s]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert!(mtime_of(&marker) > before, "2 回目で mtime が進む（開くだけでは進まない）");

    // 空の置き場は cwd 相対に化けるので、渡し忘れとして断る（1 byte も書かない）。
    let out = run_seat_in(&dir, &["heartbeat", "--target", "empt", "--state-dir", ""]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert!(stderr_of(&out).starts_with("usage: seat "), "{}", stderr_of(&out));
    assert!(!dir.join("seat").exists(), "cwd に置き場を作らない");
    fs::remove_dir_all(&dir).ok();
}

/// tick の 1 組。**先に立たない条件だけが違う**——後ろの条件はどの組でも立たないので、
/// 順序が入れ替われば別の理由が出る（理由の字面が順序の証拠になる）。
struct TickCase {
    /// 期待する理由。
    reason: &'static str,
    /// 打刻を何秒前に置くか（`None` = 置かない＝stale）。
    beat_age_s: Option<u64>,
    /// pane の本文（`None` = 本文を置かない＝読めない）。
    pane: Option<&'static str>,
    /// 退避物が名乗る席（`None` = 退避物の dir ごと無い＝読めない）。
    wm_seat: Option<&'static str>,
    /// pane を `--capture-file` で渡すか（`false` = tmux 経路＝shim に当たる）。
    via_file: bool,
    /// この組で tmux を撃つはずか。
    tmux: bool,
}

/// 1 組の fixture を組む。**どの組も TTL 内の lock を置く**＝cycle は評価されない
/// （裁定 (b) の「それ以外の周は評価しない」側で、判定 1 行だけを測る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn prepare_tick_case(dir: &Path, case: &TickCase, target: &str) -> PathBuf {
    let state = dir.join("state");
    let seat = seat_dir_of(&state, target);
    fs::create_dir_all(&seat).expect("seat dir を作れる");
    fs::write(seat.join("cycle.lock"), "{\"pid\":1,\"deadline\":0}\n").expect("lock を置ける");
    if let Some(age) = case.beat_age_s {
        let beat = seat.join("heartbeat");
        fs::write(&beat, "").expect("打刻を置ける");
        backdate(&beat, age);
    }
    if let Some(seat_name) = case.wm_seat {
        let wm = dir.join("wm");
        wm_file(&wm, "working-memory.parked.md", seat_name);
        wm_decoys(&wm, target);
    }
    if let Some(body) = case.pane {
        fs::write(dir.join("pane.txt"), body).expect("pane fixture を置ける");
    }
    state
}

/// 4 条件は**順序固定**で見て、最初に立たなかった条件を理由にする。
///
/// 閾値は両側から撃つ（`STALE_S - 1` は fresh・`STALE_S + 1` は stale）＝manifest の値が
/// 変わると落ちる。fresh の組は `--capture-file` を渡さないので、pane を先に読む実装なら
/// shim に当たる＝「fresh の周は tmux を叩かない」も測れる。
#[test]
fn seat_tick_noop_reasons_in_fixed_order() {
    let target = "seatorder";
    let cases = [
        // 鮮度が内側（fresh）: pane も WM も lock も立たない組だが、鮮度で止まる。
        TickCase { reason: "heartbeat-fresh", beat_age_s: Some(STALE_S - 1), pane: None,
                   wm_seat: Some(target), via_file: false, tmux: false },
        TickCase { reason: "pane-missing", beat_age_s: None, pane: None,
                   wm_seat: Some(target), via_file: true, tmux: false },
        // 鮮度が外側（stale）: 同じ fixture でも pane を読みに行く＝tmux に当たる。
        TickCase { reason: "pane-missing", beat_age_s: Some(STALE_S + 1), pane: None,
                   wm_seat: Some(target), via_file: false, tmux: true },
        TickCase { reason: "busy", beat_age_s: None, pane: Some(BUSY_BELOW),
                   wm_seat: Some(target), via_file: true, tmux: false },
        TickCase { reason: "busy", beat_age_s: None, pane: Some(BUSY_ABOVE),
                   wm_seat: Some(target), via_file: true, tmux: false },
        TickCase { reason: "busy", beat_age_s: None, pane: Some(NO_PROMPT),
                   wm_seat: Some(target), via_file: true, tmux: false },
        TickCase { reason: "wm-unreadable", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: None, via_file: true, tmux: false },
        TickCase { reason: "wm-unconsumed", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false },
        // 席の名乗りが違う退避物と decoy は自席の根拠にしない＝3 を**通って** 4 で止まる。
        TickCase { reason: "cycle-live", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some("other:seat"), via_file: true, tmux: false },
    ];

    for (at, case) in cases.iter().enumerate() {
        let dir = tmp();
        let state = prepare_tick_case(&dir, case, target);
        let (wm_s, pane_s) = (
            dir.join("wm").display().to_string(),
            dir.join("pane.txt").display().to_string(),
        );
        let (sock_s, state_s) = (
            dir.join("absent-sock").display().to_string(),
            state.display().to_string(),
        );
        let mut args = vec![
            "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
            "--state-dir", &state_s,
        ];
        if case.via_file {
            args.push("--capture-file");
            args.push(&pane_s);
        }
        let (out, touched) = run_seat_probed(&dir, &args);
        assert_tick_case(&out, touched, case, at);
        let recorded = fs::read_to_string(tick_file(&state, target)).unwrap_or_default();
        assert_eq!(recorded.lines().count(), 1, "組 {at}: 記録は 1 行: {recorded}");
        assert!(recorded.contains(r#""who":"seat-tick""#), "組 {at}: {recorded}");
        assert!(
            recorded.contains(&format!(r#""what":"decision=noop reason={}""#, case.reason)),
            "組 {at}: 判定を残す: {recorded}"
        );
        fs::remove_dir_all(&dir).ok();
    }
}

/// 1 組の判定を見る。
fn assert_tick_case(out: &Output, touched: bool, case: &TickCase, at: usize) {
    let reason = case.reason;
    assert_eq!(rc_of(out), i32::from(RC_OK), "組 {at}: stderr={}", stderr_of(out));
    assert_eq!(
        stdout_of(out),
        format!("seat: tick decision=noop reason={reason}\n"),
        "組 {at}"
    );
    assert_eq!(
        touched, case.tmux,
        "組 {at}（{reason}）: tmux を撃つか＝{}（撃つ組が在ることで、撃たない組の測定が空虚でない）",
        case.tmux
    );
}

/// 4 条件が揃った周は注入し、**自分で打刻する**（次の周は fresh で撃たない＝storm 止め）。
#[test]
fn seat_tick_injects_pointer_and_stamps_on_isolated_socket() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seattick";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let args = [
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ];

    let out = run_seat(&args);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("seat: tick decision=inject target={name}\n"));
    let pane = capture(&socket, name);
    assert!(
        pane.contains(&format!("seat heartbeat --target {name}")),
        "既定の 1 行が pane に現れる: {pane}"
    );
    let stamp = seat_dir_of(&state, name).join("tick-stamp");
    assert!(stamp.exists(), "自打刻が残る");

    let out = run_seat(&args);
    assert_eq!(
        stdout_of(&out),
        "seat: tick decision=noop reason=heartbeat-fresh\n",
        "自分の打刻で fresh になる（注入の直後に撃ち続けない）"
    );
    // 打刻を閾値の外へ倒すと、また撃つ側に戻る（fresh が「打刻が在る」ではなく経過で決まる）。
    backdate(&stamp, STALE_S + 1);
    let out = run_seat(&args);
    assert_eq!(stdout_of(&out), format!("seat: tick decision=inject target={name}\n"));
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 実行系が回らない周は `decision=error` と **rc 1**（noop の語彙を汚さない）。
#[test]
fn seat_tick_reports_error_with_rc_one_when_state_dir_is_unresolvable() {
    let dir = tmp();
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();

    // `--state-dir` を渡さない周は repo の git 設定から解く。repo の外では解けない。
    let out = run_seat_in(&dir, &["tick", "--target", "seaterr", "--wm-dir", &wm.display().to_string()]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "失敗は rc 1（timer から見て成功に見せない）");
    assert_eq!(stdout_of(&out), "", "失敗した周は stdout 0 行");
    assert_eq!(stderr_of(&out), "seat: tick decision=error reason=state-dir\n");
    fs::remove_dir_all(&dir).ok();
}

// flip-check: retroactive s2-07l.50
// 下の 3 本は後から足す歯である。実装（`src/seat/*.rs`）は 1 行も変えておらず、穴は歯の
// 側に在った——base に対して新しく赤くなる歯を作れないので、逃がしを札 1 行で明示する。

/// pane は読めるが tmux を撃てない周は `decision=error reason=inject-…` と **rc 1**。
///
/// 既存の error 歯は `run()` の早期 return（`--state-dir` が解けない組）しか撃たず、
/// **judgment を経由した Error 腕**——順序 4 を通って `inject_pointer` が注入を断られる
/// 組——に届いていなかった。pane を `--capture-file` で読ませると tmux を 1 度も撃たずに
/// 順序 2 を通れるので、**注入だけが tmux に当たって落ちる**組が作れる。
///
/// reason の続き（`tmux-failed` 等）は固定しない: 注入の断りは字面が noop の語彙と重なる
/// ので、器が約束しているのは **`inject-` の前置きで分けること**だけである。
#[test]
fn seat_tick_reports_error_when_pane_is_readable_but_tmux_is_unreachable() {
    let dir = tmp();
    let target = "seatunreach";
    let state = dir.join("state");
    let wm = dir.join("wm");
    // 順序 3 は**自席の**退避物だけを見る。別席の名乗りと decoy は「3 を通った」側の材料で、
    // ここで止まると順序 4 へ届かず、この歯は Error 腕を 1 度も撃たない。
    wm_file(&wm, "working-memory.parked.md", "other:seat");
    wm_decoys(&wm, target);
    fs::write(dir.join("pane.txt"), IDLE_PANE).expect("pane fixture を置ける");
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let (sock_s, pane_s) = (
        dir.join("absent-sock").display().to_string(),
        dir.join("pane.txt").display().to_string(),
    );

    let out = run_seat(&[
        "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
        "--state-dir", &state_s, "--capture-file", &pane_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "注入を断られた周は rc 1");
    assert_eq!(stdout_of(&out), "", "失敗した周は stdout 0 行");
    assert!(
        stderr_of(&out).starts_with("seat: tick decision=error reason=inject-"),
        "注入の断りは inject- の前置きで noop の語彙と分ける: {}",
        stderr_of(&out)
    );
    let recorded = fs::read_to_string(tick_file(&state, target)).unwrap_or_default();
    assert!(
        recorded
            .lines()
            .last()
            .is_some_and(|line| line.contains(r#""what":"decision=error reason=inject-"#)),
        "記録の末尾 1 行も理由まで残す（表示と記録で同じ字面）: {recorded}"
    );
    fs::remove_dir_all(&dir).ok();
}

/// `working-memory.` で始まり `.md` で終わるが **17 文字**の名前は退避物に数えない。
///
/// 長さの条件（前置き + 接尾の最短形 = 18 文字）を外す変異が生き延びていた。decoy が
/// 短い名前だった間は、前置きを外す変異が**長さで**落ち、長さを外す変異は誰にも撃たれ
/// なかった——ここは長さだけが効く負例を単独で置く。
///
/// 期待は「自席の退避物なし」で順序 3 を**通って** 4 で止まる形（`cycle-live`）である。
/// 長さの条件が消えると同じ fixture が `wm-unconsumed` へ倒れる＝理由の字面が変異を捕まえる。
#[test]
fn seat_tick_ignores_short_wm_like_names() {
    let dir = tmp();
    let target = "seatshort";
    let state = dir.join("state");
    let seat = seat_dir_of(&state, target);
    fs::create_dir_all(&seat).expect("seat dir を作れる");
    // 順序 4 で止める（TTL 内の lock）＝3 を通ったことが理由の字面で分かる。
    // ★`lock_is_live` は **mtime だけ**を見て中身を読まない: `deadline:0` は失効に見えるが、
    // いま書いた file なので live 側である（既存の歯と同じ idiom）。
    fs::write(seat.join("cycle.lock"), "{\"pid\":1,\"deadline\":0}\n").expect("lock を置ける");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).expect("wm dir を作れる");
    // 前置きと接尾は満たすが 17 文字＝最短形に 1 文字足りない（名乗りは自席にしておく）。
    let short = wm.join("working-memory.md");
    fs::write(&short, format!("---\nseat: {target}\n---\n\n## 計画弧\n- 続き\n"))
        .expect("短い名前の file を置ける");
    assert_eq!(
        short.file_name().and_then(std::ffi::OsStr::to_str).map(str::len),
        Some(17),
        "負例は 17 文字ちょうど（境界の 1 文字下）"
    );
    fs::write(dir.join("pane.txt"), IDLE_PANE).expect("pane fixture を置ける");
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let (sock_s, pane_s) = (
        dir.join("absent-sock").display().to_string(),
        dir.join("pane.txt").display().to_string(),
    );

    let out = run_seat(&[
        "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
        "--state-dir", &state_s, "--capture-file", &pane_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: tick decision=noop reason=cycle-live\n",
        "短すぎる名前は自席の退避物に数えない＝3 を通って 4 で止まる"
    );
    // 表示の完全一致だけでは 1 段しか測れない（`wm-unconsumed` を除く assert は上の
    // 完全一致に包含されて**発火しない**）。記録側の末尾 1 行でもう 1 段測る。
    let recorded = fs::read_to_string(tick_file(&state, target)).unwrap_or_default();
    assert!(
        recorded
            .lines()
            .last()
            .is_some_and(|line| line.contains(r#""what":"decision=noop reason=cycle-live""#)),
        "記録の末尾 1 行も cycle-live（長さの条件が消えると wm-unconsumed へ倒れる）: {recorded}"
    );
    fs::remove_dir_all(&dir).ok();
}

/// guard が drop されたら独立 socket の server は終わっている。
///
/// 後始末を「歯の末尾の 1 行」で持つと、assert が落ちた周に飛ばされて server が残る。
/// RAII なら panic 経路でも畳めるので、**畳まれることそのもの**を 1 本で測る。
#[test]
fn seat_isolated_session_is_torn_down_when_guard_drops() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatguard";
    {
        let guard = start_seat(&socket, name);
        assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
        // guard が生きている間は繋がる＝下の「繋がらない」が空虚でないことの対照。
        assert!(
            tmux(&socket, &["list-sessions"]).status.success(),
            "guard が生きている間は server も生きている"
        );
    }

    // session が尽きた server は自分で終わる＝socket へ繋げなくなる（終了は非同期ゆえ待つ）。
    let deadline = Instant::now() + PROMPT_WAIT;
    let mut gone = false;
    while Instant::now() < deadline {
        if !tmux(&socket, &["list-sessions"]).status.success() {
            gone = true;
            break;
        }
        sleep(Duration::from_millis(100));
    }
    assert!(gone, "guard の drop で server が終わっている");
    fs::remove_dir_all(&dir).ok();
}

/// cycle が **1 key も送らずに断る** 1 組。
struct GateCase {
    /// 期待する理由。
    reason: &'static str,
    /// 退避物が名乗る席（`None` = 退避物の dir ごと無い＝読めない）。
    wm_seat: Option<&'static str>,
    /// pane の本文（`None` = 本文を置かない＝読めない）。
    pane: Option<&'static str>,
    /// 置き場の位置に file を置く（dir を作れない）。
    broken_state: bool,
}

/// 1 組の fixture を組む。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn prepare_gate_case(dir: &Path, case: &GateCase, target: &str) -> PathBuf {
    let state = dir.join("state");
    fs::create_dir_all(state.join("seat")).expect("state dir を作れる");
    if case.broken_state {
        // 置き場の位置に file が在ると dir を作れない（error kind は競合と同じ AlreadyExists）。
        fs::write(seat_dir_of(&state, target), "").expect("邪魔な file を置ける");
    }
    let wm = dir.join("wm");
    // 退避物**ではない** file は、どの組でも「在る」の根拠にならない。
    wm_decoys(&wm, target);
    if let Some(seat_name) = case.wm_seat {
        wm_file(&wm, "working-memory.parked.md", seat_name);
    }
    if let Some(body) = case.pane {
        fs::write(dir.join("pane.txt"), body).expect("pane fixture を置ける");
    }
    state
}

/// 1 組を撃ち、**送っていない**ことまで見る。
fn assert_gate_case(dir: &Path, case: &GateCase, target: &str, state: &Path) {
    let (wm_s, pane_s) = (
        dir.join("wm").display().to_string(),
        dir.join("pane.txt").display().to_string(),
    );
    let (sock_s, state_s) = (
        dir.join("absent-sock").display().to_string(),
        state.display().to_string(),
    );
    let wm_arg = if case.wm_seat.is_some() || case.reason != "wm-unreadable" {
        wm_s
    } else {
        dir.join("absent-wm").display().to_string()
    };
    let (out, touched) = run_seat_probed(
        dir,
        &[
            "cycle", "--target", target, "--wm-dir", &wm_arg, "--capture-file", &pane_s,
            "--tmux-socket", &sock_s, "--state-dir", &state_s,
        ],
    );
    let reason = case.reason;
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{reason}: stdout={}", stdout_of(&out));
    assert_eq!(stdout_of(&out), "", "{reason}: 断りの周は stdout 0 行");
    assert_eq!(stderr_of(&out), format!("seat: cycle refused reason={reason}\n"));
    assert!(!touched, "{reason}: tmux を 1 度も撃たない＝1 key も送っていない");
    if !case.broken_state {
        assert!(
            !seat_dir_of(state, target).join("cycle.lock").exists(),
            "{reason}: 断った周も lock を残さない"
        );
    }
}

/// 退避物が無い席へは **1 key も送らない**（憲法 CON5 / SRS FR28）。
///
/// 退避物の判定は「名前の前置き ∧ `.md` ∧ `.consumed.md` でない ∧ frontmatter の `seat:` が
/// 一致」の全部で、どれか 1 つでも緩めると decoy が根拠に化ける。pane が **busy でも**
/// 理由が `wm-missing` になることで、手順 2（退避物）が 3（idle）より先だと測れる。
#[test]
fn seat_cycle_refuses_without_unconsumed_wm() {
    let target = "seatnowm";
    for case in &[
        GateCase { reason: "wm-missing", wm_seat: None, pane: Some(BUSY_BELOW), broken_state: false },
        GateCase { reason: "wm-missing", wm_seat: Some("other:seat"), pane: Some(IDLE_PANE), broken_state: false },
        GateCase { reason: "wm-unreadable", wm_seat: None, pane: Some(IDLE_PANE), broken_state: false },
    ] {
        let dir = tmp();
        let state = prepare_gate_case(&dir, case, target);
        assert_gate_case(&dir, case, target, &state);
        fs::remove_dir_all(&dir).ok();
    }
}

/// 退避物が在っても、席が打ちかけ・読めない・置き場が壊れている周は **1 key も送らない**。
#[test]
fn seat_cycle_refuses_at_each_gate_without_sending() {
    let target = "seatgate";
    for case in &[
        GateCase { reason: "busy", wm_seat: Some("seatgate"), pane: Some(BUSY_BELOW), broken_state: false },
        // 走行中の印が入力欄の**上**に在る周（実際の席の spinner 位置）も打ちかけである。
        GateCase { reason: "busy", wm_seat: Some("seatgate"), pane: Some(BUSY_ABOVE), broken_state: false },
        // 現行の版の spinner（`esc to interrupt` を持たず・statusline 3 行の上）と API 再試行中。
        GateCase { reason: "busy", wm_seat: Some("seatgate"), pane: Some(RUNNING_PANE), broken_state: false },
        GateCase { reason: "busy", wm_seat: Some("seatgate"), pane: Some(RETRYING_PANE), broken_state: false },
        // 入力欄の位置が読めない pane は idle と名乗らない（fail-closed）。
        GateCase { reason: "busy", wm_seat: Some("seatgate"), pane: Some(NO_PROMPT), broken_state: false },
        GateCase { reason: "pane-missing", wm_seat: Some("seatgate"), pane: None, broken_state: false },
        // 置き場が使えない周を `lock-held` と名乗ると、競合と故障を記録から分けられない。
        GateCase { reason: "state-dir", wm_seat: Some("seatgate"), pane: Some(IDLE_PANE), broken_state: true },
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
    let guard = start_clearing_seat(&socket, name, &log, false);
    assert!(guard.ready(), "fake な席を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.live.md", name);
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
    assert_eq!(stderr_of(&out), "seat: cycle refused reason=lock-held\n");
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "", "1 key も送っていない");
    assert!(lock.exists(), "他の cycle の lock を消さない");

    // TTL（900 秒）の**内側**はまだ live＝譲る（境界の下側を測る）。
    backdate(&lock, TTL_S - 100);
    let out = run_seat(&args);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stderr_of(&out), "seat: cycle refused reason=lock-held\n");
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "", "まだ 1 key も送っていない");

    // TTL を超えた lock は residue＝取り直して進む。
    backdate(&lock, TTL_S + 100);
    let out = run_seat(&args);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("seat: cycle done target={name}\n"));
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
    let guard = start_clearing_seat(&socket, name, &log, false);
    assert!(guard.ready(), "fake な席を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    let parked = wm_file(&wm, "working-memory.parked.md", name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("seat: cycle done target={name}\n"));
    let received = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(received, "/clear\n/rebrief\n", "作り直しの後に復元を送る: {received}");
    assert!(
        !seat_dir_of(&state, name).join("cycle.lock").exists(),
        "済んだ lock は返す"
    );
    assert!(parked.exists(), "退避物は動かさない（consume は復元側の仕事）");
    let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
    assert!(recorded.contains(r#""who":"seat-cycle""#), "cycle を 1 行残す: {recorded}");
    assert!(recorded.contains(r#""what":"cycle done""#), "{recorded}");
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 作り直しを確認できない席へは**復元を送らない**（`/clear` が通ったことにしない）。
///
/// `sh` は `/clear` を消化しないので、送った字面が直近の非空行に残り続ける＝作り直しが
/// 起きていない席の形である。確認を素通りさせる実装はここで落ちる。
#[test]
fn seat_cycle_reports_clear_unconfirmed_when_session_is_not_rebuilt() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatstuck";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.stuck.md", name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stderr_of(&out), "seat: cycle failed reason=clear-unconfirmed\n");
    let pane = capture(&socket, name);
    assert!(pane.contains("/clear"), "作り直しの注入自体は送っている: {pane}");
    assert!(!pane.contains("/rebrief"), "確認できない周に復元を送らない: {pane}");
    assert!(
        !seat_dir_of(&state, name).join("cycle.lock").exists(),
        "失敗した周も lock を返す"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 作り直しは確認できても、**復元の送達が確認できない**周は `restore-unconfirmed`。
#[test]
fn seat_cycle_reports_restore_unconfirmed_when_seat_goes_silent() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatmute";
    let log = dir.join("seat.log");
    let guard = start_clearing_seat(&socket, name, &log, true);
    assert!(guard.ready(), "作り直し後に黙る席を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.mute.md", name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert_eq!(stderr_of(&out), "seat: cycle failed reason=restore-unconfirmed\n");
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "復元は送っている（確認できないだけ）"
    );
    assert!(
        !seat_dir_of(&state, name).join("cycle.lock").exists(),
        "失敗した周も lock を返す"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 退避して止まっている席は、tick がその場で cycle を回す（裁定 (b)）。
#[test]
fn seat_tick_runs_cycle_when_parked() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatparked";
    let log = dir.join("seat.log");
    let guard = start_clearing_seat(&socket, name, &log, false);
    assert!(guard.ready(), "fake な席を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.parked.md", name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        "seat: tick decision=noop reason=wm-unconsumed cycle=done\n",
        "判定は noop のまま・cycle を回したことは末尾に足す"
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "作り直して復元した"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 走行中の pane はどの形でも **busy**: 現行の spinner（`esc to interrupt` 無し・statusline
/// 3 行）／API 再試行の banner 4 形のうち 3 形（`Retrying in` / stalled / no-response）／
/// 印が statusline の遠い下／todo 形（語が文）／括弧無し（開始 16 秒未満）／compaction 中。
///
/// 域を末尾 6 非空行に取る実装は spinner を idle と読む（実測 2026-09-11: `/rebrief` 走行中の
/// 席へ `/clear` が送られた・bd `s2-07l.94`）。`BUSY_FAR_BELOW` は「下は全行を見る」を上の
/// 域の数とは別に測る。後半 5 形は lens-94 HIGH-1 / HIGH-2 / MEDIUM-4（本体の実装を実読）。
#[test]
fn seat_tick_reads_current_running_panes_as_busy() {
    let target = "seatrunning";
    let panes = [
        RUNNING_PANE, RETRYING_PANE, BUSY_FAR_BELOW, TODO_SPINNER_PANE, STALLED_PANE,
        NO_RESPONSE_PANE, BARE_SPINNER_PANE, COMPACTING_PANE,
    ];
    for (at, pane) in panes.into_iter().enumerate() {
        let case = TickCase { reason: "busy", beat_age_s: None, pane: Some(pane),
                              wm_seat: Some(target), via_file: true, tmux: false };
        let dir = tmp();
        let state = prepare_tick_case(&dir, &case, target);
        let (out, touched) = run_tick_case(&dir, &case, target, &state);
        assert_tick_case(&out, touched, &case, at);
        fs::remove_dir_all(&dir).ok();
    }
}

/// turn を終えた実席（完了行 `Crunched for …` + statusline 3 行）は **idle**＝手順 3
/// （退避物）へ進む。busy と読むと tick が永久に noop になる（退行の歯・base でも GREEN）。
#[test]
fn seat_tick_reads_finished_pane_with_tall_statusline_as_idle() {
    let target = "seatfinished";
    for (at, pane) in [IDLE_TALL_PANE, IDLE_COUNT_PANE, IDLE_QUOTE_PANE].into_iter().enumerate() {
        let case = TickCase { reason: "wm-unconsumed", beat_age_s: None, pane: Some(pane),
                              wm_seat: Some(target), via_file: true, tmux: false };
        let dir = tmp();
        let state = prepare_tick_case(&dir, &case, target);
        let (out, touched) = run_tick_case(&dir, &case, target, &state);
        assert_tick_case(&out, touched, &case, at);
        fs::remove_dir_all(&dir).ok();
    }
}

/// 上の域は **6 非空行ちょうど**: spinner が prompt 行の 6 非空行上なら busy、7 なら idle
/// （上限の側を明示する歯。域の理由は `docs/design/seat-autonomy.md` 裁定 (e)）。
#[test]
fn seat_tick_above_region_is_exactly_six_nonempty_lines() {
    let target = "seatabove";
    for (fillers, reason) in [(4_usize, "busy"), (5_usize, "wm-unconsumed")] {
        // spinner → filler 行 × n → 区切り（1）→ prompt: spinner は prompt の (n + 2) 非空行上。
        let mut pane = String::from("✻ Sublimating… (19m 36s · ↓ 36.4k tokens)\n");
        for at in 0..fillers {
            pane.push_str(&format!("  filler {at}\n"));
        }
        pane.push_str(TALL_TAIL);
        let dir = tmp();
        let path = dir.join("above.txt");
        fs::write(&path, &pane).ok();
        let case = TickCase { reason, beat_age_s: None, pane: None,
                              wm_seat: Some(target), via_file: true, tmux: false };
        let state = prepare_tick_case(&dir, &case, target);
        fs::write(dir.join("pane.txt"), &pane).ok();
        let (out, touched) = run_tick_case(&dir, &case, target, &state);
        assert_tick_case(&out, touched, &case, fillers);
        fs::remove_dir_all(&dir).ok();
    }
}

/// tick を 1 組ぶん撃つ（`seat_tick_noop_reasons_in_fixed_order` と同じ引数の組み方）。
fn run_tick_case(dir: &Path, case: &TickCase, target: &str, state: &Path) -> (Output, bool) {
    let (wm_s, pane_s) = (
        dir.join("wm").display().to_string(),
        dir.join("pane.txt").display().to_string(),
    );
    let (sock_s, state_s) = (
        dir.join("absent-sock").display().to_string(),
        state.display().to_string(),
    );
    let mut args = vec![
        "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
        "--state-dir", &state_s,
    ];
    if case.via_file {
        args.push("--capture-file");
        args.push(&pane_s);
    }
    run_seat_probed(dir, &args)
}

/// echo された `/clear` が statusline の高さで末尾 6 非空行の外に在る周は、**作り直しを確認
/// できない**（`clear-unconfirmed`・復元を送らない）。
///
/// 末尾 6 非空行で見る実装はこの pane を「済んだ」と読み、作り直されていない席へ復元を送る
/// （base: `cycle done`）。pane は `--capture-file` で固定し、送信だけ偽の席へ通す。
#[test]
fn seat_cycle_does_not_confirm_clear_from_echo_pushed_out_by_tall_statusline() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatecho";
    let log = dir.join("seat.log");
    let guard = start_clearing_seat(&socket, name, &log, false);
    assert!(guard.ready(), "偽の席を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.echo.md", name);
    let pane = dir.join("pane.txt");
    fs::write(&pane, CLEAR_ECHO_PANE).ok();
    let (wm_s, state_s, pane_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        pane.display().to_string(),
    );

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s, "--capture-file", &pane_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(stderr_of(&out), "seat: cycle failed reason=clear-unconfirmed\n");
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n",
        "作り直しを確認できない周に復元を送らない"
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}
