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
use vessel::name::NAME;
use vessel::rules::manifest::Manifest;
use vessel::seat::inject::tick_path;
use vessel::seat::meter::{cap_of, window_of};

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

/// cap は guard と管理 tick が**同じ関数**で読む（`s2-07l.89`）: 発効した行は引き、不発効・
/// 型違い・行不在は `None`（測らない側）。**0 は引く**（「常に止める」の宣言であって欠落では
/// ない・窓の `> 0` とは別物）。tick 側の境界の歯（60 / 59）と対で「1 本の口」を測る。
#[test]
fn seat_meter_reads_cap_from_the_manifest_row() {
    let row = |extra: &str, value: &str| {
        format!(
            "schema = 1\n\n[[rule]]\nid = \"seat.context_cap_pct\"\nkind = \"SeatContextCapPct\"\nvalue = {value}\n{extra}ruling = \"r\"\nruled_at = \"d\"\n"
        )
    };
    let live = Manifest::parse(&row("", "60")).expect("fixture を読める");
    assert_eq!(cap_of(&live), Some(60), "発効した行は引ける");
    let off = Manifest::parse(&row("enabled = false\n", "60")).expect("fixture を読める");
    assert_eq!(cap_of(&off), None, "不発効の行は引かない（値は在っても使わない）");
    let zero = Manifest::parse(&row("", "0")).expect("fixture を読める");
    assert_eq!(cap_of(&zero), Some(0), "0 は引く（常に止める宣言・欠落ではない）");
    let absent = Manifest::parse("schema = 1\n").expect("fixture を読める");
    assert_eq!(cap_of(&absent), None, "行そのものが無い周は引かない");
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
pub(crate) fn socket_of(dir: &Path) -> String {
    dir.join("sock").display().to_string()
}

/// 独立 socket の tmux を 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(crate) fn tmux(socket: &str, args: &[&str]) -> Output {
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
pub(crate) struct IsolatedSeat {
    /// 独立 socket の path。
    socket: String,
    /// session 名。
    name: String,
    /// prompt が描かれたか。
    ready: bool,
}

impl IsolatedSeat {
    /// prompt が描かれたか。**落とすのは呼び側の `#[test]`** で helper では panic しない。
    pub(crate) fn ready(&self) -> bool {
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

/// 独立 socket に `sh -i` の session を 1 つ立て、畳む guard を返す。window 名も `name`（打刻の
/// target `session:window` が `name:name` に解ける）。
///
/// **live server には触れない**（socket は tmp・設定は `-f /dev/null`）。判定を guard へ
/// 載せて返すのは helper で panic しないためで、落とすのは呼び側の `#[test]` である。
pub(crate) fn start_seat(socket: &str, name: &str) -> IsolatedSeat {
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
            "new-session", "-d", "-s", name, "-n", name, "-x", "120", "-y", "40", "-e", ps1, "sh", "-i",
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

/// `seat` の使い方を snapshot 1 本に固定する（C12.5）。
#[test]
fn seat_external_form() {
    let form = stderr_of(&run_seat(&[]));
    insta::assert_snapshot!(form);
}

// ─────────────────── heartbeat / tick / cycle ───────────────────

/// 入力欄が空の pane（**idle に見える字面**。busy / idle は打刻で与える＝字面は判定に効かない）。
const IDLE_PANE: &str = "❯ \n  10% 100k/1M Opus 5\n";
/// prompt を 1 行も持たない pane（入力欄の位置が読めない）。
const NO_PROMPT: &str = "$ \n  10% 100k/1M Opus 5\n";
/// 入力欄に打ちかけが在る pane（送ると 1 行に merge する形）。
const INPUT_BUSY_PANE: &str = "❯ typing\n  10% 100k/1M Opus 5\n";
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
/// `/clear` が届いて**作り直された直後**の実席（2026-09-11・匿名化済み）: banner 3 行の下に
/// **消費済みの echo `❯ /clear`**（行頭・col 0）が残り、その直下に空の新 prompt と statusline。
///
/// 「探索域に `/clear` の字面が無い」で見る作り直し確認は、この echo が prompt の直上に
/// **必ず**残るので構造的に偽のまま固定され、復元を送らずに席を空のまま残す（bd `s2-07l.96`）。
const REBUILT_PANE: &str = concat!(
    " ▐▛███▛█   Claude Code v2.1.268\n",
    "▝▜██████▀  Fable 5.1 with high effort · Claude Max\n",
    "  ▝▝ ▝▝    /…/repo · /rc\n",
    "❯ /clear\n",
    "────────────────────────────────────────\n",
    "❯\u{a0} \n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  repo\n",
    "  Fable 5.1 [high] 5h:41%(2h8m) 7d:21%(6d5h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
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
/// `seat.tick_stale_s` の宣言値（`rules/manifest.toml`）。歯はこの値の**両側**を撃つ。
const STALE_S: u64 = 2400;
/// 判定行の context の列（fixture の statusline は 10% / 19%）。
const CTX_10: &str = " context=10";
/// 同上（19%）。
const CTX_19: &str = " context=19";
/// prompt より下が空＝出所なし。
const CTX_NO_SOURCE: &str = " context=unmeasured reason=no-source";
/// cap（manifest 行 `seat.context_cap_pct` = 60）**以上**の席の pane（インシデントの形: lens 待ちの
/// まま 96%・spinner の字面が prompt の上）。busy は打刻で与える。両側から撃つ＝manifest の値が変わると落ちる。
const OVER_CAP_BUSY_PANE: &str = concat!(
    "✻ Sublimating… (19m 36s · ↓ 36.4k tokens)\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  96% 960k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// cap 以上で turn を終えた字面の席（退避済みの席が `/clear` を待つ形）。
const OVER_CAP_IDLE_PANE: &str = concat!(
    "✻ Crunched for 10m 28s · done 12:41\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  96% 960k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// cap **未満**で spinner の字面を持つ席。
const BELOW_CAP_BUSY_PANE: &str = concat!(
    "✻ Sublimating… (2m 3s · ↓ 4.1k tokens)\n",
    "────────────────────────────────────────\n",
    "❯\u{a0}\n",
    "────────────────────────────────────────\n",
    "  user@host (user@example.com)  scribe2  main\n",
    "  12% 120k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
);
/// statusline を持たない idle の席（prompt より下に候補でない行だけ）。
const NO_STATUSLINE_IDLE_PANE: &str = "❯ \n  ⏵⏵ bypass permissions on (shift+tab to cycle)\n";
/// statusline の候補は在るが健全性を外れた idle の席（pct > 100）。
const OUT_OF_BOUND_IDLE_PANE: &str = "❯ \n  150% 1500k/1M Opus 5\n";

/// spinner の字面を持つ pane を使用率だけ変えて組む（cap = 60 の**両側**を撃つための fixture）。
fn busy_pane_at(pct: u64) -> String {
    format!(
        "✻ Sublimating… (2m 3s · ↓ 4.1k tokens)\n\
         ────────────────────────────────────────\n\
         ❯\u{a0}\n\
         ────────────────────────────────────────\n\
         \x20 user@host (user@example.com)  scribe2  main\n\
         \x20 {pct}% {}k/1M Fable 5.1 [high] 5h:10%(4h22m) 7d:15%(6d8h)\n\
         \x20 ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent\n",
        pct.saturating_mul(10)
    )
}
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

/// 打刻の 1 行を**契約の字面から**組む（実装の型を使わない・設計 seat-state.md §2）。
fn stamp_line(state: &str, event: &str, ts: u64, sid: &str) -> String {
    format!(r#"{{"schema":1,"state":"{state}","event":"{event}","ts":{ts},"sid":"{sid}"}}"#)
}

/// 1970 年からの秒。
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// 席の状態の fixture（`<seat dir>/state.jsonl`・hook の打刻の代わりに置く）。
#[derive(Clone, Copy)]
enum StateFix {
    /// file を置かない（hook が載っていない席）。
    Absent,
    /// file の位置に dir を置く（読めない形）。
    Unreadable,
    /// 最終行が Busy（`age_s` 秒前の `UserPromptSubmit`）。
    Busy { age_s: u64 },
    /// 最終行が Idle（`Stop`）。
    Idle,
}

/// 打刻 file の path（契約の字面から組む）。
fn state_file(seat: &Path) -> PathBuf {
    seat.join("state.jsonl")
}

/// fixture を置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_state(seat: &Path, fix: StateFix) {
    fs::create_dir_all(seat).expect("seat dir を作れる");
    let body = match fix {
        StateFix::Absent => return,
        StateFix::Unreadable => {
            fs::create_dir_all(state_file(seat)).expect("state.jsonl の位置に dir を置ける");
            return;
        }
        StateFix::Busy { age_s } => {
            stamp_line("busy", "UserPromptSubmit", unix_now().saturating_sub(age_s), "sid-fix")
        }
        StateFix::Idle => stamp_line("idle", "Stop", unix_now(), "sid-fix"),
    };
    fs::write(state_file(seat), format!("{body}\n")).expect("打刻を置ける");
}

/// Idle の打刻を置く（tick / cycle が状態の門を通る周の fixture）。
fn stamp_idle(state: &Path, target: &str) {
    write_state(&seat_dir_of(state, target), StateFix::Idle);
}

/// 判定行の state の列（`state=<値> event=<出所|none>`）。
const ST_IDLE: &str = " state=idle event=Stop";
/// 同上（Busy・閾値の内側）。
const ST_BUSY: &str = " state=busy event=UserPromptSubmit";
/// 同上（Busy が閾値より古い）。
const ST_STALE: &str = " state=stale event=UserPromptSubmit";
/// 同上（打刻 file なし）。
const ST_MISSING: &str = " state=missing event=none";
/// 同上（読めない）。
const ST_UNREADABLE: &str = " state=unreadable event=none";

/// 打刻の成功行と tick の記録の末尾に載る**置き場の出所と path**（契約の字面から組む）。
/// path は行末（空白や ` source=` を含む path でも出所を偽れない）。
fn provenance(state: &Path, source: &str) -> String {
    format!(" source={source} state_dir={}", state.display())
}

/// 成功行の `state_dir=` の値を切り出す（行末までの全部が path）。
fn state_dir_in(line: &str) -> Option<PathBuf> {
    line.split_once(" state_dir=")
        .map(|(_, path)| PathBuf::from(path))
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
/// 偽の席が hook の代わりに置く打刻の**時刻**（`s2-07l.112`）。
#[derive(Clone, Copy)]
enum FakeStamp {
    /// いま（送達 ts 以後＝証拠になる）。
    Now,
    /// 100 秒前（送達 ts より前＝古い打刻・証拠にならない）。
    Old,
    /// 打たない（hook が死んだ・載っていない席の形）。
    Never,
}

/// 偽の席が打刻を 1 行 append する shell 断片（**契約の字面から**組む・設計 seat-state.md §2）。
/// [`FakeStamp::Never`] は何もしない `:`。
fn stamp_cmd(state_file: &Path, state: &str, event: &str, when: FakeStamp) -> String {
    let ts = match when {
        FakeStamp::Now => "$(date +%s)",
        FakeStamp::Old => "$(( $(date +%s) - 100 ))",
        FakeStamp::Never => return ":".to_owned(),
    };
    format!(
        "printf '{{\"schema\":1,\"state\":\"{state}\",\"event\":\"{event}\",\"ts\":%s,\"sid\":\"fake\"}}\\n' \"{ts}\" >> '{}'",
        state_file.display()
    )
}

/// `/clear` を受けると画面を消して echo を描き直し、hook の代わりに `SessionStart` を `on_clear` の
/// 時刻で打つ偽の席。それ以外の行は `UserPromptSubmit`（復元の消費の証拠）を `on_line` の時刻で打ち、
/// 受けた字面を描いて `Stop`（turn の終わり＝実席と同じく Idle へ戻る）を同じ時刻で打つ。
/// 打刻の file は `state_file`（`<seat dir>/state.jsonl`）。
fn start_clearing_seat(
    socket: &str,
    name: &str,
    log: &Path,
    state_file: &Path,
    (on_clear, on_line): (FakeStamp, FakeStamp),
) -> IsolatedSeat {
    let after_clear = stamp_cmd(state_file, "idle", "SessionStart", on_clear);
    let on_other = format!(
        "{}; printf 'seat got %s\\n' \"$line\"; {}",
        stamp_cmd(state_file, "busy", "UserPromptSubmit", on_line),
        stamp_cmd(state_file, "idle", "Stop", on_line)
    );
    start_clearing_seat_with(socket, name, log, &after_clear, &on_other)
}

/// `/clear` の後に走らせる shell と、それ以外の行への応答を指定して偽の席を立てる。
///
/// `/clear` を受けた席は画面を消した後、実席と同じく echo `❯ /clear` を行頭に描き直す（実測
/// 2026-09-11）。**echo は作り直しの証拠ではない**（`s2-07l.112`）: 証拠は `after_clear` が置く
/// `SessionStart` の打刻で、echo だけを描いて打刻しない席は「作り直しを確認できない席」の形になる。
fn start_clearing_seat_with(
    socket: &str,
    name: &str,
    log: &Path,
    after_clear: &str,
    on_other: &str,
) -> IsolatedSeat {
    let script = format!(
        "while :; do printf '❯ '; read -r line || exit 0; printf '%s\\n' \"$line\" >> '{}'; \
         case \"$line\" in '/clear') printf '\\033[2J\\033[3J\\033[H❯ /clear\\n'; {after_clear} ;; \
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
    assert_eq!(
        stdout_of(&out),
        format!("seat: heartbeat target=seatbeat_0.0{}\n", provenance(&state, "flag")),
        "成功行は打刻先と、その解決の出所（--state-dir）を持つ"
    );
    assert_eq!(stderr_of(&out), "", "成立した周は stderr へ 1 byte も書かない");
    assert!(marker.exists(), "打刻 file が在る");
    // A/B: 行が名乗る置き場から組んだ marker が、実際に書かれた marker と 1 対 1 で一致する。
    let claimed = state_dir_in(stdout_of(&out).trim_end()).map(|d| seat_dir_of(&d, "seatbeat_0.0").join("heartbeat"));
    assert_eq!(claimed.as_deref(), Some(marker.as_path()), "行の path と実体の親 dir が一致する");

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

/// 相対の `--state-dir` は **cwd で絶対化した path** を名乗る（flag の字面の echo ではない）。
/// 相対のままでは cwd に依存して「どこへ」を名乗れず、行が実体と 1 対 1 にならない。
/// 空白と ` source=` を含む dir 名でも、出所が先・path が行末なので出所を偽れない。
#[test]
fn seat_heartbeat_names_absolute_state_dir_for_relative_flag() {
    let dir = tmp();
    let cwd = dir.join("sub");
    fs::create_dir_all(&cwd).ok();
    let rel = "rel state source=git-config";
    let state = cwd.join(rel);
    let marker = seat_dir_of(&state, "seatrel").join("heartbeat");

    let out = run_seat_in(&cwd, &["heartbeat", "--target", "seatrel", "--state-dir", rel]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: heartbeat target=seatrel{}\n", provenance(&state, "flag")),
        "相対の flag は cwd で絶対化した path を名乗り、出所は flag のまま"
    );
    assert!(marker.exists(), "打刻は cwd 相対の実体に落ちる");
    // A/B: 行が名乗る path（flag の字面ではない）から組んだ marker が実体と 1 対 1 で一致する。
    let claimed = state_dir_in(stdout_of(&out).trim_end()).map(|d| seat_dir_of(&d, "seatrel").join("heartbeat"));
    assert_eq!(claimed.as_deref(), Some(marker.as_path()), "行の path と実体の親 dir が一致する");
    assert!(
        !stdout_of(&out).contains(&format!(" state_dir={rel}")),
        "flag の字面をそのまま echo しない: {}",
        stdout_of(&out)
    );
    fs::remove_dir_all(&dir).ok();
}

/// `--state-dir` が無い周は **tmp repo** の git 設定から置き場を解き、行と記録に
/// `source=git-config` と解いた path を出す（anchor の設定は触らない）。
///
/// 出所の事故（2026-09-10・planner 席）: 設定が死んだ dir を指していても rc 0 の成功行が出る。
/// 席側の打刻行と tick 側の記録を並べるだけで別の dir を見ていると分かる形にする。
#[test]
fn seat_heartbeat_and_tick_resolve_state_dir_from_git_config() {
    let dir = tmp();
    let repo = dir.join("repo");
    let state = dir.join("state-from-config");
    fs::create_dir_all(&repo).ok();
    let init = Command::new("git").args(["-C"]).arg(&repo).args(["init", "-q"]).output();
    assert!(init.is_ok_and(|out| out.status.success()), "tmp repo を作れる");
    let set = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["config", &format!("{NAME}.stateDir")])
        .arg(&state)
        .output();
    assert!(set.is_ok_and(|out| out.status.success()), "tmp repo に置き場を設定できる");
    let marker = seat_dir_of(&state, "seatgit").join("heartbeat");

    let out = run_seat_in(&repo, &["heartbeat", "--target", "seatgit"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: heartbeat target=seatgit{}\n", provenance(&state, "git-config")),
        "git 設定から解いた周は source=git-config"
    );
    assert!(marker.exists(), "打刻は設定が指す dir に落ちる");
    let claimed = state_dir_in(stdout_of(&out).trim_end()).map(|d| seat_dir_of(&d, "seatgit").join("heartbeat"));
    assert_eq!(claimed.as_deref(), Some(marker.as_path()), "行の path と実体の親 dir が一致する");

    // 同じ設定から解く tick は、直前の打刻で fresh＝tmux を叩かずに noop。記録にも同じ 2 語が載る。
    let (wm_s, sock_s) = (dir.join("wm").display().to_string(), dir.join("absent-sock").display().to_string());
    let out = run_seat_in(&repo, &["tick", "--target", "seatgit", "--wm-dir", &wm_s, "--tmux-socket", &sock_s]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=heartbeat-fresh{}\n", provenance(&state, "git-config"))
    );
    let recorded = fs::read_to_string(tick_file(&state, "seatgit")).unwrap_or_default();
    assert!(
        recorded.contains(&format!(
            r#""what":"decision=noop reason=heartbeat-fresh{}""#,
            provenance(&state, "git-config")
        )),
        "記録の what にも置き場と出所が載る: {recorded}"
    );
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
    /// 判定行の末尾に載る context（`" context=<pct>"` / `" context=unmeasured reason=<語>"` /
    /// pane を取得しない周は `""`＝評価していない）。
    context: &'static str,
    /// 席の状態の fixture（打刻）。
    stamp: StateFix,
    /// 判定行の末尾に載る state の列（fresh で読まない周は `""`）。
    state: &'static str,
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
    write_state(&seat, case.stamp);
    state
}

/// 条件は**順序固定**で見て、最初に立たなかった条件を理由にする。
///
/// 閾値は両側から撃つ（`STALE_S - 1` は fresh・`STALE_S + 1` は stale）＝manifest の値が
/// 変わると落ちる。fresh の組は `--capture-file` を渡さないので、pane を先に読む実装なら
/// shim に当たる＝「fresh の周は tmux を叩かない」も測れる。
#[test]
fn seat_tick_noop_reasons_in_fixed_order() {
    let target = "seatorder";
    let cases = fixed_order_cases(target);
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
        assert_tick_case(&out, touched, case, at, &state);
        let recorded = fs::read_to_string(tick_file(&state, target)).unwrap_or_default();
        assert_eq!(recorded.lines().count(), 1, "組 {at}: 記録は 1 行: {recorded}");
        assert!(recorded.contains(r#""who":"seat-tick""#), "組 {at}: {recorded}");
        assert!(
            recorded.contains(&format!(
                r#""what":"decision=noop reason={}{}{}{}""#,
                case.reason,
                case.context,
                case.state,
                provenance(&state, "flag")
            )),
            "組 {at}: 判定を残す（context と state の列と置き場の出所も記録に載る）: {recorded}"
        );
        fs::remove_dir_all(&dir).ok();
    }
}

/// 順序固定の歯の組（**先に立たない条件だけが違う**）。
fn fixed_order_cases(target: &'static str) -> Vec<TickCase> {
    vec![
        // 鮮度が内側（fresh）: pane も lock も立たない組だが、鮮度で止まる（状態も読まない）。退避物は
        // **他席**の名乗り＝自席の申告ではないので鮮度を飛ばさない（`.105`・自席の退避物が在る周は
        // `seat_tick_cycles_freshly_stamped_seat_when_own_wm_is_unconsumed` が持つ）。
        TickCase { reason: "heartbeat-fresh", beat_age_s: Some(STALE_S - 1), pane: None,
                   wm_seat: Some("other:seat"), via_file: false, tmux: false, context: "",
                   stamp: StateFix::Busy { age_s: 0 }, state: "" },
        // pane より先に状態を読む＝pane-missing の行にも state の列が載る。
        TickCase { reason: "pane-missing", beat_age_s: None, pane: None,
                   wm_seat: Some(target), via_file: true, tmux: false, context: "",
                   stamp: StateFix::Idle, state: ST_IDLE },
        // 鮮度が外側（stale）: 同じ fixture でも pane を読みに行く＝tmux に当たる。
        TickCase { reason: "pane-missing", beat_age_s: Some(STALE_S + 1), pane: None,
                   wm_seat: Some(target), via_file: false, tmux: true, context: "",
                   stamp: StateFix::Idle, state: ST_IDLE },
        // 状態の門は typed の打刻で決まる: 入力欄が空の字面でも Busy の打刻なら busy。
        TickCase { reason: "busy", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Busy { age_s: STALE_S }, state: ST_BUSY },
        TickCase { reason: "state-missing", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Absent, state: ST_MISSING },
        TickCase { reason: "state-unreadable", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Unreadable, state: ST_UNREADABLE },
        TickCase { reason: "state-stale", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Busy { age_s: STALE_S + 1 }, state: ST_STALE },
        TickCase { reason: "wm-unreadable", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: None, via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Idle, state: ST_IDLE },
        TickCase { reason: "wm-unconsumed", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Idle, state: ST_IDLE },
        // 席の名乗りが違う退避物と decoy は自席の根拠にしない＝4 を**通って** 5 で止まる。
        TickCase { reason: "cycle-live", beat_age_s: None, pane: Some(IDLE_PANE),
                   wm_seat: Some("other:seat"), via_file: true, tmux: false, context: CTX_10,
                   stamp: StateFix::Idle, state: ST_IDLE },
    ]
}

/// 1 組の判定を見る。
fn assert_tick_case(out: &Output, touched: bool, case: &TickCase, at: usize, state: &Path) {
    let (reason, context, stamped) = (case.reason, case.context, case.state);
    assert_eq!(rc_of(out), i32::from(RC_OK), "組 {at}: stderr={}", stderr_of(out));
    assert_eq!(
        stdout_of(out),
        format!(
            "seat: tick decision=noop reason={reason}{context}{stamped}{}\n",
            provenance(state, "flag")
        ),
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
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let args = [
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ];

    let out = run_seat(&args);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=inject target={name} consumed=false kind=pointer{CTX_NO_SOURCE}{ST_IDLE}{}\n", provenance(&state, "flag")),
        "`sh -i` の席は打刻しない＝消費の証拠が来ないので consumed=false（送達は成立）"
    );
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
        format!("seat: tick decision=noop reason=heartbeat-fresh{}\n", provenance(&state, "flag")),
        "自分の打刻で fresh になる（注入の直後に撃ち続けない）"
    );
    // 打刻を閾値の外へ倒すと、また撃つ側に戻る（fresh が「打刻が在る」ではなく経過で決まる）。
    backdate(&stamp, STALE_S + 1);
    let out = run_seat(&args);
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=inject target={name} consumed=false kind=pointer{CTX_NO_SOURCE}{ST_IDLE}{}\n", provenance(&state, "flag"))
    );
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// context が cap 以上 ∧ **打刻が Busy** の席（インシデントの形）には、idle を待たずに退避の
/// pointer 1 行を注入する（`kind=externalize`・SRS FR29「idle を待たずに」・退避の合図は状態の門の
/// **外**＝planner 裁定 2026-09-11: FR29 > ADR-0015 §2.3）。payload の先頭行は退避 skill と実測値を
/// 持ち、注入後は自打刻する（次の周は fresh で撃たない＝storm 止め）。
///
/// 判定は `--capture-file` の pane で通し、送信だけ独立 socket の席へ通す。busy を理由に noop
/// する実装はこの席を誰も止められない＝auto-compact に至る（bd `s2-07l.89`）。
#[test]
fn seat_tick_injects_externalize_pointer_when_context_reaches_cap_while_busy() {
    // cap = 60 の**等号側**（60 ちょうど）も撃つ: `>=` を `>` へ緩める変異はここで落ちる。
    for pct in [96_u64, 60] {
        let dir = tmp();
        let socket = socket_of(&dir);
        let name = "seatovercap";
        let guard = start_seat(&socket, name);
        assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
        let state = dir.join("state");
        let wm = dir.join("wm");
        fs::create_dir_all(&wm).ok();
        write_state(&seat_dir_of(&state, name), StateFix::Busy { age_s: 0 });
        let pane = dir.join("pane.txt");
        fs::write(&pane, busy_pane_at(pct)).ok();
        let (wm_s, state_s, pane_s) = (
            wm.display().to_string(),
            state.display().to_string(),
            pane.display().to_string(),
        );
        // `--pointer` は打刻の促しの上書きであって、実測値を運ぶ退避の行には掛からない。
        let args = [
            "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
            "--state-dir", &state_s, "--capture-file", &pane_s, "--pointer", "custom-pointer",
        ];

        let out = run_seat(&args);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{pct}%: stderr={}", stderr_of(&out));
        assert_eq!(
            stdout_of(&out),
            format!("seat: tick decision=inject target={name} consumed=false kind=externalize context={pct}{ST_BUSY}{}\n", provenance(&state, "flag")),
            "{pct}%: 打刻が Busy でも退避の合図は送る（state の列は busy のまま載る・busy な席は queue＝consumed=false）"
        );
        let seen = capture(&socket, name);
        assert!(seen.contains("/ready-compaction"), "{pct}%: 退避 skill の名が届く: {seen}");
        assert!(seen.contains(&format!("{pct}%")) && seen.contains("60%"), "{pct}%: 実測値と cap が届く: {seen}");
        assert!(!seen.contains("seat heartbeat"), "{pct}%: heartbeat の pointer ではない: {seen}");
        assert!(!seen.contains("custom-pointer"), "{pct}%: --pointer は退避の行を上書きしない: {seen}");
        let stamp = seat_dir_of(&state, name).join("tick-stamp");
        assert!(stamp.exists(), "{pct}%: 自打刻が残る");
        let recorded = fs::read_to_string(tick_file(&state, name)).unwrap_or_default();
        assert!(
            recorded.contains(&format!(
                r#""what":"decision=inject target=seatovercap consumed=false kind=externalize context={pct}{ST_BUSY}{}""#,
                provenance(&state, "flag")
            )),
            "{pct}%: 記録にも kind と context と state と置き場の出所が載る: {recorded}"
        );

        let out = run_seat(&args);
        assert_eq!(
            stdout_of(&out),
            format!("seat: tick decision=noop reason=heartbeat-fresh{}\n", provenance(&state, "flag")),
            "{pct}%: 自分の打刻で fresh になる（同じ席へ周ごとに再注入しない・context も読まない）"
        );
        // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
        drop(guard);
        fs::remove_dir_all(&dir).ok();
    }
}

/// cap **未満**の busy な席は、退避物 0 件 ∧ cycle lock が空いていても注入しない（`noop reason=busy`・
/// tmux 未接触）。「cap 以上」の述語を常に真へ倒す変異（cargo mutants で唯一生存した形）は、
/// 測れた席すべてへ退避の pointer を送る＝ここで落ちる。
#[test]
fn seat_tick_does_not_inject_below_cap_when_nothing_else_stops_it() {
    let dir = tmp();
    let target = "seatbelowfree";
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    write_state(&seat_dir_of(&state, target), StateFix::Busy { age_s: 0 });
    fs::write(dir.join("pane.txt"), busy_pane_at(59)).ok();
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
        format!("seat: tick decision=noop reason=busy context=59{ST_BUSY}{}\n", provenance(&state, "flag"))
    );
    assert!(!touched, "cap 未満の busy な席には 1 key も送らない（tmux を撃たない）");
    fs::remove_dir_all(&dir).ok();
}

/// 注入が**成立しなかった**周は打刻しない（storm 止めの極性の裏側）: 打刻を送達の前へ動かすと、
/// 退避の促しが届かないまま次の周が fresh で黙る＝cap 以上の席を握り潰す。pane は読めるが
/// tmux を撃てない席（shim）で `decision=error reason=inject-…`・rc 1・stamp 不在。
#[test]
fn seat_tick_does_not_stamp_when_externalize_injection_fails() {
    let dir = tmp();
    let target = "seatcapfail";
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    write_state(&seat_dir_of(&state, target), StateFix::Busy { age_s: 0 });
    fs::write(dir.join("pane.txt"), OVER_CAP_BUSY_PANE).ok();
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

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert!(
        stderr_of(&out).starts_with("seat: tick decision=error reason=inject-"),
        "注入の断りは inject- の前置き: {}",
        stderr_of(&out)
    );
    assert!(stderr_of(&out).contains(" context=96"), "context は評価済み: {}", stderr_of(&out));
    assert!(touched, "注入だけが tmux に当たる");
    assert!(
        !seat_dir_of(&state, target).join("tick-stamp").exists(),
        "成立しなかった注入では打刻しない（次の周も撃つ）"
    );
    fs::remove_dir_all(&dir).ok();
}

/// cap **未満** ∧ busy は従来どおり `noop reason=busy` のまま、判定行に context の値が載る。
#[test]
fn seat_tick_reports_context_when_busy_below_cap() {
    let target = "seatbelowcap";
    // 59 は cap の**直下**（`>=` を `>` へ緩めても 60 で落ちる歯と対で、境界を両側から撃つ）。
    for (pane, context) in [(BELOW_CAP_BUSY_PANE.to_owned(), " context=12"), (busy_pane_at(59), " context=59")] {
        let case = TickCase { reason: "busy", beat_age_s: None, pane: None,
                              wm_seat: Some(target), via_file: true, tmux: false, context,
                              stamp: StateFix::Busy { age_s: 0 }, state: ST_BUSY };
        let dir = tmp();
        let state = prepare_tick_case(&dir, &case, target);
        fs::write(dir.join("pane.txt"), &pane).ok();
        let (out, touched) = run_tick_case(&dir, &case, target, &state);
        assert_tick_case(&out, touched, &case, 0, &state);
        fs::remove_dir_all(&dir).ok();
    }
}

/// statusline が無い ∧ idle は従来の判定（退避物）へ進み、判定行に `context=unmeasured` と
/// 理由が載る（測れないことを理由に注入も停止もしない・AC9 条 3 と同じ極性）。
#[test]
fn seat_tick_proceeds_with_unmeasured_context_without_statusline() {
    let target = "seatnostatus";
    let cases = [
        (NO_STATUSLINE_IDLE_PANE, " context=unmeasured reason=pane-no-statusline"),
        // 候補は在るが健全性を外れた周も同じ極性（捏造値で cap を超えない・注入しない）。
        (OUT_OF_BOUND_IDLE_PANE, " context=unmeasured reason=pane-out-of-bound"),
    ];
    for (at, (pane, context)) in cases.into_iter().enumerate() {
        let case = TickCase { reason: "wm-unconsumed", beat_age_s: None, pane: Some(pane),
                              wm_seat: Some(target), via_file: true, tmux: false, context,
                              stamp: StateFix::Idle, state: ST_IDLE };
        let dir = tmp();
        let state = prepare_tick_case(&dir, &case, target);
        let (out, touched) = run_tick_case(&dir, &case, target, &state);
        assert_tick_case(&out, touched, &case, at, &state);
        fs::remove_dir_all(&dir).ok();
    }
}

/// fresh の周は cap 以上でも pane も打刻も読まない＝`noop reason=heartbeat-fresh` で context も
/// state も載らない（盲点の限界を pin する歯）。
#[test]
fn seat_tick_does_not_read_context_when_fresh() {
    let target = "seatfreshcap";
    // 退避物は他席の名乗り（自席の退避物が在る周は鮮度を飛ばして pane を読む・`.105`）。
    let case = TickCase { reason: "heartbeat-fresh", beat_age_s: Some(STALE_S - 1),
                          pane: Some(OVER_CAP_BUSY_PANE), wm_seat: Some("other:seat"),
                          via_file: true, tmux: false, context: "",
                          stamp: StateFix::Busy { age_s: 0 }, state: "" };
    let dir = tmp();
    let state = prepare_tick_case(&dir, &case, target);
    let (out, touched) = run_tick_case(&dir, &case, target, &state);
    assert_tick_case(&out, touched, &case, 0, &state);
    fs::remove_dir_all(&dir).ok();
}

/// cap 以上でも **自席の未 consumed 退避物が在る**（退避済み）／退避物の dir を読めない／
/// cycle lock が live の周は pointer を注入せず次の条件へ進む（planner 裁定 2026-09-11・livelock の補正）: 退避済みの席は
/// `/clear` 前で cap 以上のままなので、注入すると毎周 pointer を重ねて cycle に一度も落ちない。
/// busy なら `busy`、idle なら `wm-unconsumed` / `wm-unreadable`（lock は TTL 内＝cycle は評価しない）。
#[test]
fn seat_tick_falls_through_to_wm_when_parked_over_cap() {
    let target = "seatparkedcap";
    let cases = [
        TickCase { reason: "busy", beat_age_s: None, pane: Some(OVER_CAP_BUSY_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: " context=96",
                   stamp: StateFix::Busy { age_s: 0 }, state: ST_BUSY },
        TickCase { reason: "wm-unconsumed", beat_age_s: None, pane: Some(OVER_CAP_IDLE_PANE),
                   wm_seat: Some(target), via_file: true, tmux: false, context: " context=96",
                   stamp: StateFix::Idle, state: ST_IDLE },
        TickCase { reason: "wm-unreadable", beat_age_s: None, pane: Some(OVER_CAP_IDLE_PANE),
                   wm_seat: None, via_file: true, tmux: false, context: " context=96",
                   stamp: StateFix::Idle, state: ST_IDLE },
        // 退避物 0 件でも他の cycle が走っている（lock が TTL 内）周は注入しない（排他）。
        TickCase { reason: "cycle-live", beat_age_s: None, pane: Some(OVER_CAP_IDLE_PANE),
                   wm_seat: Some("other:seat"), via_file: true, tmux: false, context: " context=96",
                   stamp: StateFix::Idle, state: ST_IDLE },
    ];
    for (at, case) in cases.iter().enumerate() {
        let dir = tmp();
        let state = prepare_tick_case(&dir, case, target);
        let (out, touched) = run_tick_case(&dir, case, target, &state);
        assert_tick_case(&out, touched, case, at, &state);
        fs::remove_dir_all(&dir).ok();
    }
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
    stamp_idle(&state, target);
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
    assert!(
        !seat_dir_of(&state, target).join("tick-stamp").exists(),
        "成立しなかった注入では打刻しない（打刻を送達の前へ動かす変異はここで落ちる）"
    );
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
    write_state(&seat, StateFix::Idle);
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
        format!("seat: tick decision=noop reason=cycle-live{CTX_10}{ST_IDLE}{}\n", provenance(&state, "flag")),
        "短すぎる名前は自席の退避物に数えない＝4 を通って 5 で止まる"
    );
    // 表示の完全一致だけでは 1 段しか測れない（`wm-unconsumed` を除く assert は上の
    // 完全一致に包含されて**発火しない**）。記録側の末尾 1 行でもう 1 段測る。
    let recorded = fs::read_to_string(tick_file(&state, target)).unwrap_or_default();
    assert!(
        recorded
            .lines()
            .last()
            .is_some_and(|line| line.contains(r#""what":"decision=noop reason=cycle-live "#)),
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
    /// 席の状態の fixture（打刻）。置き場が壊れている組では置かない。
    stamp: StateFix,
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
    if !case.broken_state {
        write_state(&seat_dir_of(&state, target), case.stamp);
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
    assert_eq!(
        stderr_of(&out),
        format!("seat: cycle refused reason={reason}{}\n", provenance(state, "flag")),
        "置き場が解けた周は断りの行にも 2 語が載る"
    );
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
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
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
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
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

/// 退避して止まっている席は、tick がその場で cycle を回す（裁定 (b)）。
#[test]
fn seat_tick_runs_cycle_when_parked() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatparked";
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
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=wm-unconsumed{CTX_NO_SOURCE} cycle=done{ST_IDLE} cycle-stamp=none{}\n", provenance(&state, "flag")),
        "判定は noop のまま・context の後ろに cycle を回したことを足し、state の列が続く（置き場の出所は最後）"
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

/// 退避を終えた席は heartbeat を**直前**に打っていることが多い。自席の未 consumed 退避物が
/// 在る周は鮮度 gate を飛ばして cycle を評価する（退避物の存在 = 席の「作り直してよい」の
/// 申告・裁定 (a)・`s2-07l.105`・user 直命 2026-09-11「流石に長すぎだろ」）。
///
/// base は鮮度で `heartbeat-fresh` の noop になり、退避物が在るのに最大 `seat.tick_stale_s`
/// の間 cycle が評価されない（admin2 で実測: 退避完了時 age 588 s → 約 30 分 idle）。
#[test]
fn seat_tick_cycles_freshly_stamped_seat_when_own_wm_is_unconsumed() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatfreshparked";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Now),
    );
    assert!(guard.ready(), "fake な席を立てられる");
    let seat = seat_dir_of(&state, name);
    fs::create_dir_all(&seat).expect("seat dir を作れる");
    // 直前の打刻（mtime = いま）＝鮮度だけなら fresh で止まる周。
    fs::write(seat.join("heartbeat"), "").expect("打刻を置ける");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.parked.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=wm-unconsumed{CTX_NO_SOURCE} cycle=done{ST_IDLE} cycle-stamp=none{}\n", provenance(&state, "flag")),
        "打刻が fresh でも退避物が在れば鮮度を飛ばして cycle を評価する（`cycle=` が付く）"
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "作り直して復元した"
    );
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 判定行の `key=value` を 1 つ取り出す（末尾の改行は落とす）。
fn tick_token(line: &str, key: &str) -> Option<String> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
        .map(str::to_owned)
}

/// cycle を評価した周は `cycle-stamp` を打ち、次の周以降は同じ退避物が残っていても
/// `seat.tick_stale_s` 未満なら cycle を評価しない（`cycle-recent`・`/clear` を送らない＝
/// 偽の席の受信 log が増えない）。`/clear` は不可逆の口（N1）で、復元されない退避物へ
/// 5 分ごとに繰り返してはならない（`s2-07l.110`・裁定 (a)）。
///
/// base は cycle を回した事実を残さず、2 度目の周も `cycle=done` になる（RED）。
#[test]
fn seat_tick_backs_off_after_a_recent_cycle() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatbackoff";
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
    let args = [
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ];

    let first = run_seat(&args);
    assert_eq!(rc_of(&first), i32::from(RC_OK), "stderr={}", stderr_of(&first));
    assert_eq!(
        stdout_of(&first),
        format!("seat: tick decision=noop reason=wm-unconsumed{CTX_NO_SOURCE} cycle=done{ST_IDLE} cycle-stamp=none{}\n", provenance(&state, "flag")),
        "1 周目: 打刻が無いので cycle を回す（stamp は none・列は cycle → state → cycle-stamp → 出所）"
    );
    let stamp = seat_dir_of(&state, name).join("cycle-stamp");
    assert!(stamp.is_file(), "cycle を評価した周は cycle-stamp を打つ");
    let stamped_at = mtime_of(&stamp);

    // 退避物は残ったまま（偽の席は /rebrief で consume しない）。
    let second = run_seat(&args);
    assert_eq!(rc_of(&second), i32::from(RC_OK), "stderr={}", stderr_of(&second));
    let line = stdout_of(&second);
    assert_eq!(
        tick_token(&line, "reason").as_deref(),
        Some("cycle-recent"),
        "2 周目: 直前に cycle を評価したので評価しない: {line}"
    );
    assert!(tick_token(&line, "cycle").is_none(), "2 周目は cycle を評価しない（`cycle=` が付かない）: {line}");
    let age = tick_token(&line, "cycle-stamp")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(u64::MAX);
    assert!(age < STALE_S, "cycle-stamp の age（秒）は閾値未満: {line}");
    assert!(
        line.ends_with(&format!("{ST_IDLE} cycle-stamp={age}{}\n", provenance(&state, "flag"))),
        "state の列 → cycle-stamp → 置き場の出所の順のまま: {line}"
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "2 周目は /clear を送らない（偽の席の受信は 1 周目の 2 行のまま）"
    );
    assert_eq!(
        mtime_of(&stamp),
        stamped_at,
        "見送った周は stamp を打ち直さない（打ち直すと back-off が永久になる）"
    );
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// back-off は永久ではない: cycle-stamp が `seat.tick_stale_s` 以上前なら再び cycle を
/// 評価する（復元が失敗したまま放置された席を、次の stale な周で拾い直す）。
#[test]
fn seat_tick_re_evaluates_cycle_once_the_cycle_stamp_is_stale() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatstalestamp";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Now),
    );
    assert!(guard.ready(), "fake な席を立てられる");
    stamp_idle(&state, name);
    let stamp = seat_dir_of(&state, name).join("cycle-stamp");
    fs::write(&stamp, "0\n").expect("stamp を置ける");
    // 境界は**未満**: 経過が閾値ちょうどの周は評価する（`<=` にすると見送る）。
    backdate(&stamp, STALE_S);
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.parked.md", name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(tick_token(&line, "reason").as_deref(), Some("wm-unconsumed"), "{line}");
    assert_eq!(tick_token(&line, "cycle").as_deref(), Some("done"), "stale な stamp は cycle を止めない: {line}");
    let age = tick_token(&line, "cycle-stamp")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    assert!(age >= STALE_S, "判定行に stamp の age（秒）を載せる: {line}");
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "/clear\n/rebrief\n", "作り直して復元した");
    assert!(
        mtime_of(&stamp).elapsed().is_ok_and(|since| since.as_secs() < STALE_S),
        "評価した周は stamp を打ち直す"
    );
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

/// stamp を読めない周は「無い」に読み替えず、cycle を評価しない（`cycle-stamp-unreadable`・
/// 読めないことを理由に不可逆の `/clear` へ倒さない＝N1 の向き・planner 裁定 2026-09-11）。
/// stamp を自分を指す symlink（loop）にして metadata を読めなくする（state の打刻は Idle のまま
/// ＝状態の門は通る）。tmux は叩かない。
#[test]
fn seat_tick_does_not_evaluate_cycle_when_the_cycle_stamp_is_unreadable() {
    let dir = tmp();
    let target = "seatbadstamp";
    let state = dir.join("state");
    stamp_idle(&state, target);
    let stamp = seat_dir_of(&state, target).join("cycle-stamp");
    std::os::unix::fs::symlink("cycle-stamp", &stamp).expect("自分を指す symlink を置ける");
    assert!(fs::metadata(&stamp).is_err(), "負例の前提: stamp の metadata は読めない（loop）");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.parked.md", target);
    fs::write(dir.join("pane.txt"), IDLE_PANE).expect("pane fixture を置ける");
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let (sock_s, pane_s) = (
        dir.join("absent-sock").display().to_string(),
        dir.join("pane.txt").display().to_string(),
    );

    let (out, touched) = run_seat_probed(&dir, &[
        "tick", "--target", target, "--wm-dir", &wm_s, "--tmux-socket", &sock_s,
        "--state-dir", &state_s, "--capture-file", &pane_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: tick decision=noop reason=cycle-stamp-unreadable{CTX_10}{ST_IDLE} cycle-stamp=unreadable{}\n", provenance(&state, "flag")),
        "読めない stamp は評価しない側へ倒す（`cycle=` が付かない）"
    );
    assert!(!touched, "1 key も送らない");
    fs::remove_dir_all(&dir).ok();
}

/// 鮮度を飛ばした周も**それ以降の条件は不変**: 打刻が Busy の席には送らない（cycle も
/// 評価しない＝`cycle=` が付かない）。鮮度で止まる実装は pane も打刻も読まずに `heartbeat-fresh` で止まる。
#[test]
fn seat_tick_freshly_stamped_seat_with_own_wm_is_still_not_cycled_when_busy() {
    let target = "seatfreshbusy";
    let case = TickCase { reason: "busy", beat_age_s: Some(STALE_S - 1), pane: Some(IDLE_PANE),
                          wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                          stamp: StateFix::Busy { age_s: 0 }, state: ST_BUSY };
    let dir = tmp();
    let state = prepare_tick_case(&dir, &case, target);
    let (out, touched) = run_tick_case(&dir, &case, target, &state);
    assert_tick_case(&out, touched, &case, 0, &state);
    fs::remove_dir_all(&dir).ok();
}

/// 鮮度 gate を飛ばすのは**自席の**未 consumed 退避物が在る周だけ（裁定 (c) 不変）: 他席の
/// 名乗りの退避物だけの周・退避物の dir を読めない周は従来どおり `heartbeat-fresh` で止まり、
/// tmux を叩かない（他席の文脈で cycle しない・読めない周を「在る」に読み替えない）。
#[test]
fn seat_tick_freshly_stamped_seat_keeps_heartbeat_gate_without_own_wm() {
    let target = "seatfreshother";
    let cases = [
        TickCase { reason: "heartbeat-fresh", beat_age_s: Some(STALE_S - 1), pane: Some(IDLE_PANE),
                   wm_seat: Some("other:seat"), via_file: false, tmux: false, context: "",
                   stamp: StateFix::Idle, state: "" },
        TickCase { reason: "heartbeat-fresh", beat_age_s: Some(STALE_S - 1), pane: Some(IDLE_PANE),
                   wm_seat: None, via_file: false, tmux: false, context: "",
                   stamp: StateFix::Idle, state: "" },
    ];
    for (at, case) in cases.iter().enumerate() {
        let dir = tmp();
        let state = prepare_tick_case(&dir, case, target);
        let (out, touched) = run_tick_case(&dir, case, target, &state);
        assert_tick_case(&out, touched, case, at, &state);
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
/// のは、確認の待ち（30 秒）が直列に積み上がらないようにするため。
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

    let out = cycle_with_pane_after_clear(&dir, name, IDLE_TALL_PANE, shape);

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

    let out = cycle_with_pane_after_clear(&dir, name, IDLE_TALL_PANE, REBUILT_HOOKS_PANE);

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

/// cycle を撃ち、`/clear` が席の log（`<dir>/seat.log`）に着いた**後**で pane の写し
/// （`--capture-file`）を `after` へ差し替えてから結果を待つ。写しは作り直しの確認が 500 ms
/// ごとに読み直すので、「送る前は idle・送った後にこの形」の pane を 1 本の file で再現できる。
/// socket は [`socket_of`]・log は偽の席と同じ path から導く（引数上限・憲法 C4）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn cycle_with_pane_after_clear(dir: &Path, name: &str, before: &str, after: &str) -> Output {
    let (socket, log) = (socket_of(dir), dir.join("seat.log"));
    let (state, wm, pane) = (dir.join("state"), dir.join("wm"), dir.join("pane.txt"));
    stamp_idle(&state, name);
    fs::write(&pane, before).expect("pane fixture を置ける");
    let (wm_s, state_s, pane_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        pane.display().to_string(),
    );
    let child = Command::new(bin())
        .args([
            "seat", "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
            "--state-dir", &state_s, "--capture-file", &pane_s,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("binary を起動できる");
    let deadline = Instant::now() + PROMPT_WAIT;
    while Instant::now() < deadline
        && !fs::read_to_string(&log).unwrap_or_default().contains("/clear")
    {
        sleep(Duration::from_millis(100));
    }
    fs::write(&pane, after).expect("pane fixture を差し替えられる");
    child.wait_with_output().expect("binary の終了を待てる")
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
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
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

/// 打刻 file が**無い**席（hook が載っていない）・**読めない**席へは送達しても消費を測れない＝
/// `consumed=unknown` に **`reason=`** を添える（missing を消費と読み替えない・`false` とも混ぜない・
/// 憲法 C10 の測定 / 未測定の弁別・`s2-07l.112`）。送達（目印が現れた）は成立ゆえ rc 0・記録は残る。
#[test]
fn seat_evidence_inject_reports_unknown_with_reason_when_stamp_file_is_missing_or_unreadable() {
    for (fix, reason) in [(StateFix::Absent, "state-missing"), (StateFix::Unreadable, "state-unreadable")] {
        let dir = tmp();
        let socket = socket_of(&dir);
        let name = "seat-nostamp";
        let guard = start_seat(&socket, name);
        assert!(guard.ready(), "{reason}: 独立 socket に prompt 付きの session を立てられる");
        let state = dir.join("state");
        write_state(&seat_dir_of(&state, name), fix);
        let payload = ": seat-e2e-nostamp";

        let out = run_seat(&[
            "inject", "--target", name, "--tmux-socket", &socket,
            "--state-dir", &state.display().to_string(), "--text", payload,
        ]);

        assert_eq!(rc_of(&out), i32::from(RC_OK), "{reason}: stderr={}", stderr_of(&out));
        assert_eq!(
            stdout_of(&out),
            format!(
                "seat: inject delivered target={name} bytes={} consumed=unknown reason={reason}{}\n",
                payload.len(),
                provenance(&state, "flag")
            ),
            "{reason}: 測れない周は unknown に理由を添える（true / false と混ぜない）"
        );
        assert!(capture(&socket, name).contains("seat-e2e-nostamp"), "{reason}: 字面は現れている（送達は成立）");
        assert!(tick_file(&state, name).exists(), "{reason}: 送達した周は記録する");
        drop(guard);
        fs::remove_dir_all(&dir).ok();
    }
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

// ─────────────────── 席の状態（hook の打刻・typed・`s2-07l.95`） ───────────────────

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

/// stale の境界は `seat.tick_stale_s`（= 2400）を**共用**する（新しい閾値を足さない・ADR-0015 §2.4）:
/// Busy の打刻が閾値ちょうど古い周は `busy`、1 秒超で `state-stale`。両側から撃つ＝manifest の値が
/// 変わると落ちる。
#[test]
fn seat_state_tick_stale_boundary_shares_tick_stale_s() {
    let target = "seatstatestale";
    for (age, reason, column) in [(STALE_S, "busy", ST_BUSY), (STALE_S + 1, "state-stale", ST_STALE)] {
        let case = TickCase { reason, beat_age_s: None, pane: Some(IDLE_PANE),
                              wm_seat: Some(target), via_file: true, tmux: false, context: CTX_10,
                              stamp: StateFix::Busy { age_s: age }, state: column };
        let dir = tmp();
        let state = prepare_tick_case(&dir, &case, target);
        let (out, touched) = run_tick_case(&dir, &case, target, &state);
        assert_tick_case(&out, touched, &case, usize::try_from(age).unwrap_or_default(), &state);
        fs::remove_dir_all(&dir).ok();
    }
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
        GateCase { reason: "busy", wm_seat: Some("seatstategate"), pane: Some(IDLE_PANE), broken_state: false,
                   stamp: StateFix::Busy { age_s: STALE_S } },
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

    let out = cycle_with_pane_after_clear(&dir, name, RUNNING_PANE, REBUILT_PANE);

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
    // 席は測れないので unknown（tick 行は既存 token のまま＝理由は `seat inject` の行が持つ）。
    let cases = [
        (StateFix::Busy { age_s: 0 }, ST_BUSY, "false"),
        (StateFix::Absent, ST_MISSING, "unknown"),
        (StateFix::Unreadable, ST_UNREADABLE, "unknown"),
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
            format!("seat: tick decision=inject target={name} consumed={consumed} kind=externalize context=96{column}{}\n", provenance(&state, "flag")),
            "組 {at}: 退避の合図は打刻に依らず送る（state の列は打刻のまま・consumed は打刻由来）"
        );
        assert!(capture(&socket, name).contains("/ready-compaction"), "組 {at}: 退避 skill の名が届く");
        // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
        drop(guard);
        fs::remove_dir_all(&dir).ok();
    }
}

// ─────────────────── 作り直しと送達の証拠（打刻由来・`s2-07l.112`・接頭辞 `seat_evidence_`） ───────────────────

/// 作り直しの証拠は **`/clear` の送達 ts 以後に足された `SessionStart` の打刻**だけ（設計 seat-state.md §6）。
/// pane は `--capture-file` で [`IDLE_PANE`] に固定し `/clear` の echo を**一度も**見せない＝echo を正の
/// 証拠に採る実装（base）は 30 s 待って `clear-unconfirmed` になる（RED）。復元の消費も打刻で確認する。
#[test]
fn seat_evidence_cycle_confirms_rebuild_by_session_start_stamp_without_echo() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevclear";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let stamps = state_file(&seat_dir_of(&state, name));
    let guard = start_clearing_seat(&socket, name, &log, &stamps, (FakeStamp::Now, FakeStamp::Now));
    assert!(guard.ready(), "打刻する偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.ev.md", name);
    stamp_idle(&state, name);
    let before = fs::read_to_string(&stamps).unwrap_or_default().lines().count();
    let pane = dir.join("pane.txt");
    fs::write(&pane, IDLE_PANE).ok();
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
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "/clear\n/rebrief\n", "作り直しを打刻で確認して復元を送る");
    let added: Vec<String> = fs::read_to_string(&stamps)
        .unwrap_or_default()
        .lines()
        .skip(before)
        .map(str::to_owned)
        .collect();
    assert!(added.iter().any(|line| line.contains(r#""event":"SessionStart""#)), "作り直しの証拠が足されている: {added:?}");
    assert!(added.iter().any(|line| line.contains(r#""event":"UserPromptSubmit""#)), "復元の消費の証拠が足されている: {added:?}");
    // socket を消す**前**に畳む（消してからでは kill-session が届かない・実測 2026-09-10）。
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 送達 ts より**前**の `SessionStart` しか足されない席（[`FakeStamp::Old`]・時計が戻った・古い hook の
/// 遅延書込）は、`/clear` の echo が pane に在っても作り直しと読まない（古い打刻を証拠に採らない）＝
/// `clear-unconfirmed`・復元を送らない。base は echo で `done` になる（RED）。
#[test]
fn seat_evidence_cycle_ignores_session_start_stamp_older_than_clear() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevold";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Old, FakeStamp::Now),
    );
    assert!(guard.ready(), "古い打刻を置く偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.old.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(stderr_of(&out), format!("seat: cycle failed reason=clear-unconfirmed{}\n", provenance(&state, "flag")));
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "/clear\n", "古い打刻では復元を送らない");
    let pane = capture(&socket, name);
    assert!(pane.contains("/clear"), "echo は在る（字面は証拠ではない）: {pane}");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// `/clear` の後に `SessionStart` を**打たない**席（hook が死んだ・載っていない）は、echo を描いても
/// 作り直しを確認できない＝`clear-unconfirmed`・復元を送らない（作り直しを確認できない席へ復元を
/// 刺さない・fail-closed）。送る**前**の最終行は `SessionStart`（Idle・前の作り直しの打刻）で、その
/// ts は未来（+5 s）＝「送達 ts 以後」に見える——**基線**（送る前に在った行は見ない）だけがこれを
/// 除外できる（lens-112 HIGH-1）。base は echo で `done` になる（RED）。
#[test]
fn seat_evidence_cycle_reports_clear_unconfirmed_when_no_session_start_stamp_follows() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevnone";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Never, FakeStamp::Now),
    );
    assert!(guard.ready(), "打刻しない偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.none.md", name);
    let seat = seat_dir_of(&state, name);
    fs::create_dir_all(&seat).ok();
    fs::write(
        state_file(&seat),
        format!("{}\n", stamp_line("idle", "SessionStart", unix_now().saturating_add(5), "preexisting-future")),
    )
    .expect("打刻 fixture を置ける");
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());

    let out = run_seat(&[
        "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(stderr_of(&out), format!("seat: cycle failed reason=clear-unconfirmed{}\n", provenance(&state, "flag")));
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "/clear\n", "打刻が無い周は復元を送らない");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 送達の消費は **送達 ts 以後に足された `UserPromptSubmit` の打刻**で決める（設計 §6）。席は受けた行で
/// 打刻し、その後 prompt に打ちかけ（`❯ pending`）を残す＝「入力欄が空」で消費を読む実装（base）は
/// `consumed=false` になり（RED）、打刻で読む実装は `true`。
#[test]
fn seat_evidence_inject_consumed_true_by_user_prompt_submit_stamp_even_with_pending_input() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevconsumed";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    let stamps = state_file(&seat_dir_of(&state, name));
    stamp_idle(&state, name);
    let guard = start_clearing_seat_with(
        &socket,
        name,
        &log,
        ":",
        &format!(
            "{}; printf 'seat got %s\\n' \"$line\"; printf '\u{276f} pending'; sleep 30",
            stamp_cmd(&stamps, "busy", "UserPromptSubmit", FakeStamp::Now)
        ),
    );
    assert!(guard.ready(), "打刻して打ちかけを残す偽の席を立てられる");
    let payload = "seat-e2e-stamped";

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
        "打ちかけが残っていても、送達 ts 以後の UserPromptSubmit 打刻で consumed=true"
    );
    let pane = capture(&socket, name);
    assert!(pane.contains("\u{276f} pending"), "入力欄は非空のまま: {pane}");
    assert!(
        fs::read_to_string(&stamps).unwrap_or_default().contains(r#""event":"UserPromptSubmit""#),
        "消費の打刻が在る"
    );
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// **送る前から在る**打刻しか無い席（`sh -i`＝受けた行で打刻しない・入力欄はすぐ空に戻る）は
/// `consumed=false`（queue の形）。fixture は `UserPromptSubmit` を 2 行置く: 100 秒前のものと、
/// **ts が送達 ts 以後に見える未来（+5 s）のもの**——後者は「送る前に在った行は見ない」基線だけが
/// 除外できる（`ts >= since` では拾ってしまう・lens-112 HIGH-1）。base は入力欄が空なので `true`（RED）。
#[test]
fn seat_evidence_inject_reports_consumed_false_when_only_older_stamps_exist() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevolder";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    // 送る前から在る打刻だけ（UserPromptSubmit 2 行・片方は ts が未来）＝送達後に足される行は無い。
    let seat = seat_dir_of(&state, name);
    fs::create_dir_all(&seat).ok();
    fs::write(
        state_file(&seat),
        format!(
            "{}\n{}\n",
            stamp_line("busy", "UserPromptSubmit", unix_now().saturating_sub(100), "old"),
            stamp_line("busy", "UserPromptSubmit", unix_now().saturating_add(5), "preexisting-future")
        ),
    )
    .expect("打刻 fixture を置ける");
    let payload = ": seat-e2e-older";

    let out = run_seat(&[
        "inject", "--target", name, "--tmux-socket", &socket,
        "--state-dir", &state.display().to_string(), "--text", payload,
    ]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!(
            "seat: inject delivered target={name} bytes={} consumed=false{}\n",
            payload.len(),
            provenance(&state, "flag")
        ),
        "送る前から在る打刻しか無い周は consumed=false（ts が未来でも基線より前の行は証拠にしない）"
    );
    assert_eq!(stderr_of(&out), "", "成功の周は stderr 0 行");
    let pane = capture(&socket, name);
    assert!(pane.contains("seat-e2e-older"), "字面は現れている（送達は成立）: {pane}");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// 置き場が**解けない**周（`--state-dir` 無し・git の外の cwd）に送達した注入は、打刻の在処を知らない
/// ので `consumed=unknown reason=state-dir`（測れない・2 語は出さない・記録もしない）。3 つ目の理由の pin。
#[test]
fn seat_evidence_inject_reports_unknown_reason_state_dir_when_place_is_unresolved() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevnodir";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let payload = ": seat-e2e-nodir";

    let out = run_seat_in(&dir, &["inject", "--target", name, "--tmux-socket", &socket, "--text", payload]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("seat: inject delivered target={name} bytes={} consumed=unknown reason=state-dir\n", payload.len()),
        "置き場が解けない周は unknown reason=state-dir（2 語なし）"
    );
    assert!(!dir.join("seat").exists(), "cwd に置き場を作らない");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// `consumed=false`（queue）は**送達の成功**であって失敗ではない: tick は `decision=inject` rc 0 で自打刻し、
/// 次の周は `heartbeat-fresh` で**再送しない**（pointer は pane に 1 度だけ現れる）。false を失敗と読んで
/// 打刻を飛ばす実装は 2 周目にもう 1 本送ってここで落ちる（planner 裁定 2026-09-12 の条件）。
#[test]
fn seat_evidence_tick_treats_consumed_false_as_delivered_and_does_not_resend() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevqueue";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    // 打刻 file は在る（Idle）が `sh -i` は submit で打刻しない＝送達した pointer は consumed=false になる形。
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let args = [
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ];

    let first = run_seat(&args);
    assert_eq!(rc_of(&first), i32::from(RC_OK), "stderr={}", stderr_of(&first));
    assert_eq!(tick_token(&stdout_of(&first), "decision").as_deref(), Some("inject"), "{}", stdout_of(&first));
    assert_eq!(tick_token(&stdout_of(&first), "consumed").as_deref(), Some("false"), "{}", stdout_of(&first));
    assert!(seat_dir_of(&state, name).join("tick-stamp").is_file(), "false でも送達は成立＝自打刻する");

    let second = run_seat(&args);
    assert_eq!(rc_of(&second), i32::from(RC_OK), "stderr={}", stderr_of(&second));
    assert_eq!(tick_token(&stdout_of(&second), "reason").as_deref(), Some("heartbeat-fresh"), "2 周目は再送しない: {}", stdout_of(&second));
    let pointer = format!("seat heartbeat --target {name}");
    assert_eq!(capture(&socket, name).matches(&pointer).count(), 1, "pointer は 1 度だけ現れる");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// cycle の復元が queue のまま（`UserPromptSubmit` の打刻が来ない）周は `restore-unconfirmed` と**記録する**
/// だけで、次の周の tick は back-off（`cycle-recent`・`.110`）で `/clear` も復元も**再送しない**。
/// `/clear` と `/rebrief` は偽の席の受信 log に 1 度ずつしか現れない。
#[test]
fn seat_evidence_cycle_does_not_resend_when_restore_stays_queued() {
    let dir = tmp();
    let socket = socket_of(&dir);
    let name = "seatevnoresend";
    let log = dir.join("seat.log");
    let state = dir.join("state");
    // `/clear` で SessionStart は打つが、復元の行では打刻しない（queue のまま）。
    let guard = start_clearing_seat(
        &socket, name, &log, &state_file(&seat_dir_of(&state, name)), (FakeStamp::Now, FakeStamp::Never),
    );
    assert!(guard.ready(), "復元を消費しない偽の席を立てられる");
    let wm = dir.join("wm");
    wm_file(&wm, "working-memory.noresend.md", name);
    stamp_idle(&state, name);
    let (wm_s, state_s) = (wm.display().to_string(), state.display().to_string());
    let args = [
        "tick", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
        "--state-dir", &state_s,
    ];

    let first = run_seat(&args);
    assert_eq!(rc_of(&first), i32::from(RC_OK), "stderr={}", stderr_of(&first));
    assert_eq!(
        tick_token(&stdout_of(&first), "cycle").as_deref(),
        Some("failed"),
        "復元の消費が確認できない周は failed（restore-unconfirmed）: {}",
        stdout_of(&first)
    );
    assert!(stdout_of(&first).contains(" reason=restore-unconfirmed"), "{}", stdout_of(&first));

    let second = run_seat(&args);
    assert_eq!(rc_of(&second), i32::from(RC_OK), "stderr={}", stderr_of(&second));
    assert_eq!(tick_token(&stdout_of(&second), "reason").as_deref(), Some("cycle-recent"), "2 周目は back-off: {}", stdout_of(&second));
    assert_eq!(
        fs::read_to_string(&log).unwrap_or_default(),
        "/clear\n/rebrief\n",
        "/clear も復元も 1 度ずつ（再送しない）"
    );
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}
