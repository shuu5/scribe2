//! 席の歯の module root（設計 docs/design/seat-roles.md・account-lifecycle.md §8）。
//!
//! 歯は題ごとの submodule に置く（`s2-07l.261`・契約の write-set が題の file 単位で交差しないため・接頭辞ごとの
//! 固定した組は seat-roles.md §7・`s2-07l.361`）: `inject`（注入の口）・`account`（口座の退避と立て直し /
//! hook 集合の食い違い / doctor の口座の行）・`launch`（起動 / 復元の第 2 手 / Enter 落ちの修復）・
//! `register`（状態 / 役割 / 登録）・`rules`（doctor の host の面）。この file には **2 つ以上の submodule が
//! 使う共有 helper と fixture**・外形 snapshot の歯（面ごとに 1 本＝`seat_usage_external_form` /
//! `seat_doctor_external_form`・`s2-07l.327`・snapshot 名が module path を含むので動かさない）・変異生存の検出線の
//! 歯（`mutant_e2e_*`）だけを残す。
//!
//! tmux は **独立 socket**（`-S <tmp>/sock -f /dev/null`）の server だけを撃ち、開発席の
//! live な server には 1 度も触れない。pane の読みは `--capture-file` で本文を直に渡す
//! ので、meter 側の歯は tmux を 1 度も起動しない。
// flip-check: moved s2-07l.261
// flip-check: moved s2-07l.361

mod account;
mod launch;
mod register;
mod ruling;
mod rules;

use crate::{make_tmp_dir, TmpDir};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};
use vessel::cli_outcome::{RC_OK, RC_REFUSED};
use vessel::name::{NAME, PLUGIN_DIR};

/// binary の path。
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_scribe2")
}

/// tmp dir を 1 つ作る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn tmp() -> TmpDir {
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

/// 消えた口の名を第 1 token に置いて撃ち、**自分の口として動かない**ことを測る（消えた口の歯 3 本の共有の 1 本）。
///
/// 役割の flag が任意になった後（seat-roles.md §26 の約束 1・`s2-07l.488`）、口を持たない第 1 token は**短い形の
/// 口座 label** として読まれる＝使い方へ落ちる形はもう無い。ゆえに「消えた」を測る側は**断りの出所**で見る:
/// rc 1・stdout 0 byte・stderr は起動の口の typed な 1 行（`seat launch: refused reason=…`）で、消えた口の
/// 名を持つ行は 1 つも出ない。置き場は明示して経路を決め打ちにする（log も row も無い空の dir）。
fn assert_gone_mouth(state: &str, gone: &str, extra: &[&str]) {
    let mut args = vec![gone, "--target", "s:w", "--state-dir", state];
    args.extend_from_slice(extra);
    let out = run_seat(&args);
    let line = stderr_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{gone}: 断る: {line}");
    assert!(stdout_of(&out).is_empty(), "{gone}: stdout 0 byte");
    assert!(line.starts_with("seat launch: refused reason="), "{gone}: 自分の口として動かない: {line}");
    assert!(!line.contains(gone), "{gone}: 消えた口の名を行に持たない: {line}");
}

/// 消えた口の歯が使う空の置き場（log も row も無い dir・後始末は呼び側）。
fn gone_mouth_place() -> (TmpDir, String) {
    let dir = tmp();
    let state = dir.join("state");
    fs::create_dir_all(&state).ok();
    let path = state.display().to_string();
    (dir, path)
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

// ─────────────────────────── 独立 socket の席 ───────────────────────────

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
    start_seat_sized(socket, name, ps1, needle, "120")
}

/// pane 幅（列数）を選んで session を立てる（折り返しの歯が狭い pane を作る）。
fn start_seat_sized(socket: &str, name: &str, ps1: &str, needle: char, width: &str) -> IsolatedSeat {
    // guard を**先に**作る: `new-session` が通った後で prompt を待つ間に panic しても畳む。
    let mut seat = IsolatedSeat {
        socket: socket.to_owned(),
        name: name.to_owned(),
        ready: false,
    };
    let out = tmux(
        socket,
        &[
            "new-session", "-d", "-s", name, "-n", name, "-x", width, "-y", "40", "-e", ps1, "sh", "-i",
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
// flip-check: retroactive s2-07l.392
const PROMPT_WAIT: Duration = Duration::from_secs(60);

// 外形 snapshot は**面ごとに 1 本**（usage / doctor の末尾・`s2-07l.327`・seat-roles.md §7）。
// 1 本に連結すると seat 面の契約が全部この 1 file で交差する（実測 2026-09-15: 4 便が互いに当たり同時に
// 出せるのが 2 便）。面を触る契約だけがその面の file に当たる形にする（pipe の外形と同じ割り方）。

/// 使い方の 1 行の tick の口（設計 seat-heartbeat.md §2 形 7・席の口で `--rules` を受けるのはこの口だけ）。
const TICK_USAGE: &str = "tick --state-dir S --target S:W [--rules F]";

/// tick の口が**在る**ことを測る（不在の歯 3 本の共有の 1 本・`s2-07l.582`）: 使い方の 1 行に口が在り、登録 row の無い空の
/// 置き場へ撃つと自分の口として判定行 1 行（`reason=no-row`・梯子の手前ゆえ `pointer=- step=-`）を stdout に出して rc 0。
fn assert_tick_mouth(state: &str, usage: &str) {
    assert!(usage.contains(&format!("|{TICK_USAGE}|")), "tick は使い方に在る: {usage}");
    let out = run_seat(&["tick", "--target", "s:w", "--state-dir", state]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "tick は自分の口として動く: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), "decision=noop target=s_w reason=no-row pointer=- step=- consumed=-\n", "判定行 1 行");
    assert!(stderr_of(&out).is_empty(), "stderr 0 byte");
}

/// 席の自律機能の口のうち、作り直しの cycle・打刻の heartbeat・context の計測 meter は**もう無い**（ADR-0045 §2 (2)・
/// `s2-07l.479.1`）: 口を持たない第 1 token として断られ（[`assert_gone_mouth`]）、使い方の 1 行にもその名が出ない。
/// 管理 tick は backoff つきで**戻った**（ADR-0058・`s2-07l.582`）＝在る側（[`assert_tick_mouth`]）。残る口（register /
/// launch）は従来どおり使い方に在る。
///
/// **消えたことと戻ったことを測る歯**である（base では tick が消えた口として断られるので RED）。
#[test]
fn seat_autonomy_subcommands_are_gone_from_the_usage() {
    let (dir, state) = gone_mouth_place();
    let usage = stderr_of(&run_seat(&[]));
    for gone in ["meter", "heartbeat", "cycle"] {
        assert_gone_mouth(&state, gone, &[]);
        assert!(!usage.contains(&format!("|{gone} ")), "{gone} は使い方に出ない: {usage}");
    }
    assert_tick_mouth(&state, &usage);
    for kept in ["register", "launch", "tick"] {
        assert!(usage.contains(&format!("{kept} --")), "{kept} は使い方に在る: {usage}");
    }
    fs::remove_dir_all(&dir).ok();
}

/// 注入の口は**もう無い**（ADR-0045 §2 (2)・`s2-07l.479.3`）: `seat inject` は口を持たない第 1 token として
/// 断られ（[`assert_gone_mouth`]）、使い方の 1 行に口の名も `--text` / `--file` の flag も出ない。
/// 送達の機構そのものは残る（`seat launch` の復元が使う・ADR-0045 §2 (4) の不変の面）ので、
/// **消えたのは口だけ**である＝残る口 2 つは従来どおり使い方に在る。管理 tick（`s2-07l.582`）は同じ送達の機構を
/// 呼ぶ在る側の口で（[`assert_tick_mouth`]）、`seat inject` の口は戻らない（ADR-0058・C17.2）。
///
/// **消えたことを測る歯**である（base では `seat inject --target s:w --text x` が自分の口として動くので RED）。
#[test]
fn seat_inject_subcommand_is_gone_from_the_usage() {
    let (dir, state) = gone_mouth_place();
    let usage = stderr_of(&run_seat(&[]));
    assert_gone_mouth(&state, "inject", &["--text", "x"]);
    assert_tick_mouth(&state, &usage);
    fs::remove_dir_all(&dir).ok();
    for gone in ["inject --target", "--text", "--file"] {
        assert!(!usage.contains(gone), "{gone} は使い方に出ない: {usage}");
    }
    for kept in ["register --", "launch --", "tick --"] {
        assert!(usage.contains(kept), "{kept} は使い方に在る: {usage}");
    }
}

/// 作業記憶の口は**もう無い**（ADR-0045 §2 (2)・`s2-07l.479.2`）: 退避（externalize）・復元（rebrief）・
/// 消費（consume）は口を持たない第 1 token として断られ（[`assert_gone_mouth`]）、使い方の 1 行にも
/// その名が出ない。`--rules` を受ける席の口は**管理 tick の 1 つだけ**（この 3 口が消えて 0 になり、`s2-07l.582` で tick が
/// 歯の seam として受ける＝設計 seat-heartbeat.md §2 形 7）: 使い方で `--rules` を持つ口は tick だけで、他の既知の verb に
/// `--rules` を渡すと未知の引数として rc 2 で断られる。
///
/// **消えたことを測る歯**である（base では 3 つとも自分の口として動くので RED）。
#[test]
fn seat_working_memory_subcommands_are_gone_from_the_usage() {
    let (dir, state) = gone_mouth_place();
    let usage = stderr_of(&run_seat(&[]));
    for gone in ["externalize", "rebrief", "consume"] {
        assert_gone_mouth(&state, gone, &[]);
        assert!(!usage.contains(&format!("|{gone} ")), "{gone} は使い方に出ない: {usage}");
    }
    assert_tick_mouth(&state, &usage);
    let rules = fixture(&dir, "rules.toml", "schema = 1\n");
    let launch = fixture(&dir, "launch.txt", "claude\n");
    let verbs: [Vec<&str>; 3] = [
        vec!["register", "--state-dir", &state, "--target", "s:w", "--role", "orchestrator", "--account", "a1", "--launch", &launch],
        vec!["launch", "--state-dir", &state, "--role", "orchestrator", "--target", "s:w"],
        vec!["ruling", "ls", "--state-dir", &state],
    ];
    for mut verb in verbs {
        verb.extend_from_slice(&["--rules", &rules]);
        let out = run_seat(&verb);
        assert_eq!(rc_of(&out), 2, "{verb:?}: `--rules` は tick の外で未知の引数: {}", stderr_of(&out));
        assert!(stderr_of(&out).starts_with("seat: 未知の引数 --rules\n"), "{verb:?}: {}", stderr_of(&out));
    }
    fs::remove_dir_all(&dir).ok();
    assert!(!usage.contains("--wm-dir"), "退避物の置き場の flag も残らない: {usage}");
    let takers: Vec<&str> = usage.split(['<', '|', '>']).filter(|mouth| mouth.contains("--rules")).collect();
    let unit = |verb: &str| format!("tick {verb} --state-dir S --target S:W --unit-dir U --binary PATH [--rules F]");
    assert_eq!(takers, [TICK_USAGE.to_owned(), unit("install"), unit("uninstall")], "席の口で `--rules` を受けるのは tick（と unit の口 2 つ・`s2-07l.583`）だけ: {usage}");
    for kept in ["register", "launch", "tick"] {
        assert!(usage.contains(&format!("{kept} --")), "{kept} は使い方に在る: {usage}");
    }
}

/// `seat` の使い方（usage 1 行・全 subcommand）を snapshot に固定する（C12.5）。
#[test]
fn seat_usage_external_form() {
    let form = stderr_of(&run_seat(&[]));
    insta::assert_snapshot!(form);
}

/// (8) `seat` の口: 既知の 3 verb（register / launch / ruling）に**未知の flag** を足すと rc 2・理由の 1 行が flag を名指し usage を添え・
/// 置き場に 1 file も作らない（row も event も書かず tmux も撃たない）。`--help` は usage を stdout へ出して rc 0
/// （設計 pipeline.md §14 約束 3 / 4 / 8）。
#[test]
fn seat_args_unknown_flag_is_refused_with_rc_2_on_every_verb() {
    let dir = tmp();
    let state = dir.join("state");
    fs::create_dir_all(&state).ok();
    let path = state.display().to_string();
    let launch = fixture(&dir, "launch.txt", "claude\n");
    let socket = dir.join("no-such-sock").display().to_string();
    let verbs: [Vec<&str>; 3] = [
        vec!["register", "--state-dir", &path, "--target", "s:w", "--role", "orchestrator", "--account", "a1", "--launch", &launch],
        vec!["launch", "--state-dir", &path, "--role", "orchestrator", "--target", "s:w", "--account", "a1", "--tmux-socket", &socket],
        vec!["ruling", "add", "--state-dir", &path, "--target", "s:w", "--words", "w"],
    ];
    for verb in verbs {
        let mut args = verb.clone();
        args.extend_from_slice(&["--bogus", "x"]);
        let out = run_seat(&args);
        assert_eq!(rc_of(&out), 2, "{verb:?}: rc 2: {}", stderr_of(&out));
        assert!(stdout_of(&out).is_empty(), "{verb:?}: stdout 0 byte");
        assert_eq!(stderr_of(&out), format!("seat: 未知の引数 --bogus\n{}\n", vessel::seat::cli::usage()), "{verb:?}");
        assert_eq!(fs::read_dir(&state).map(Iterator::count).unwrap_or(usize::MAX), 0, "{verb:?}: 置き場に何も作らない");
        let mut help = verb.clone();
        help.push("--help");
        let out = run_seat(&help);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{verb:?}: --help は rc 0: {}", stderr_of(&out));
        assert_eq!(stdout_of(&out), format!("{}\n", vessel::seat::cli::usage()), "{verb:?}: usage を stdout へ");
        assert!(stderr_of(&out).is_empty(), "{verb:?}: stderr 0 byte");
        assert_eq!(fs::read_dir(&state).map(Iterator::count).unwrap_or(usize::MAX), 0, "{verb:?}: --help も置き場に何も作らない");
    }
    fs::remove_dir_all(&dir).ok();
}

/// `seat` の既知の verb の閉じた enum（設計 contract-source.md §17 の形 (vii)）: const slice の件数（既知の verb の本数・
/// dispatch の腕の本数ではない）と宣言順が型と一致し、`as_str` と `parse` が往復し、各語は usage に載る。未知の token
/// （口座 label・空・flag・variant 名）は `parse` が `None`＝dispatch の label の腕へ落ちる側。
#[test]
fn seat_command_all_known_verbs_round_trip_and_unknown_tokens_are_none() {
    use vessel::seat::cli::{SeatCommand, SEAT_COMMANDS};
    assert_eq!(vessel::seat::cli::SEAT_COMMANDS.len(), 4, "記録時点の既知の verb（`.582` で +1〔管理 tick〕）: {SEAT_COMMANDS:?}");
    assert!(vessel::order::is_declaration_order(SEAT_COMMANDS, |command| command as usize), "宣言順: {SEAT_COMMANDS:?}");
    let words: Vec<&str> = SEAT_COMMANDS.iter().map(|command| command.as_str()).collect();
    assert_eq!(words, ["register", "launch", "ruling", "tick"], "字面の閉じた列（宣言順）");
    let usage = vessel::seat::cli::usage();
    for command in SEAT_COMMANDS {
        assert_eq!(SeatCommand::parse(command.as_str()), Some(*command), "as_str ↔ parse の往復: {command:?}");
        assert!(usage.contains(&format!("|{} ", command.as_str())) || usage.contains(&format!("<{} ", command.as_str())), "{} は usage に載る: {usage}", command.as_str());
    }
    for unknown in ["work", "", "--state-dir", "Register", "registers"] {
        assert_eq!(SeatCommand::parse(unknown), None, "未知の token {unknown:?} は None（label の腕へ落ちる）");
    }
}

/// `doctor --state-dir` の登録 row の行（`model` と `paths` の欄つき・seat-roles.md §24）・突合の項目（tmux を撃てない
/// 周の形）・その直後の host の面の行・末尾の導入先の行（登録 row の anchor・記録なし・consumer-sync.md §4）を
/// snapshot に固定する（C12.5）。
#[test]
fn seat_doctor_external_form() {
    let mut form = String::new();
    let place = role_doctor_place();
    for line in stdout_of(&role_doctor(&place)).lines().skip_while(|line| !line.starts_with("seat: ")) {
        form.push_str(line);
        form.push('\n');
    }
    fs::remove_dir_all(&place.dir).ok();
    insta::assert_snapshot!(form);
}

// ─────────────────── heartbeat / tick / cycle の共有 fixture ───────────────────

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

/// 偽の席が打刻を 1 行 append する shell 断片（**契約の字面から**組む・設計 seat-state.md §2）。
/// 時刻は常にいま（送達 ts 以後＝証拠になる側）で、打たない形は `s2-07l.479.3` で読み手ごと消えた。
fn stamp_cmd(state_file: &Path, state: &str, event: &str) -> String {
    format!(
        "printf '{{\"schema\":1,\"state\":\"{state}\",\"event\":\"{event}\",\"ts\":%s,\"sid\":\"fake\"}}\\n' \"$(date +%s)\" >> '{}'",
        state_file.display()
    )
}

/// 打刻 file の path（契約の字面から組む）。
fn state_file(seat: &Path) -> PathBuf {
    seat.join("state.jsonl")
}

/// 打刻の成功行と tick の記録の末尾に載る**置き場の出所と path**（契約の字面から組む）。
/// path は行末（空白や ` source=` を含む path でも出所を偽れない）。
fn provenance(state: &Path, source: &str) -> String {
    format!(" source={source} state_dir={}", state.display())
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

/// 判定行の `key=value` を 1 つ取り出す（末尾の改行は落とす）。
fn tick_token(line: &str, key: &str) -> Option<String> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
        .map(str::to_owned)
}

/// 記録 1 行の flat JSON から `key` の値を引く（無ければ `None`）。
fn json_value(line: &str, key: &str) -> Option<vessel::fleet::json_lite::Value> {
    vessel::fleet::json_lite::parse_object(line)
        .ok()?
        .into_iter()
        .find(|(found, _)| found == key)
        .map(|(_, value)| value)
}

// ─────────────────────────── 変異生存の検出線（s2-07l.196・ADR-0013） ───────────────────────────

/// `doctor` の突合は **渡した `--tmux-socket` の server** を見る（登録 row 1 つ・独立 socket）: 席の立つ
/// socket なら `live=1 missing=0`・server の無い別 socket なら `unmeasurable`・**`--tmux-socket` の重複は
/// 使い方で断る**（rc 1・突合の行を出さない）。
///
/// .192 の検出線で生き残った変異 `render_doctor_with` の match guard `socket.is_none()` → `true` は
/// 重複の周だけ挙動が変わる（2 つ目が黙って勝ち rc 0 で突合の行が出る）。現物の挙動を pin する歯なので
/// base でも通る（retroactive）。live な server には触れない（socket は tmp・`-f /dev/null`）。
// flip-check: retroactive s2-07l.196
#[test]
fn mutant_e2e_doctor_reconciles_against_the_given_tmux_socket_and_refuses_a_duplicate() {
    let place = role_place();
    role_stamp(&place, "mutdoc:mutdoc", Some("sid-mut"));
    let out = role_register(&place, "mutdoc:mutdoc", "orchestrator", &["--anchor", "/repo"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let seat = start_seat(&place.socket, "mutdoc");
    assert!(seat.ready(), "隔離 seat が立つ");
    let state = place.state.display().to_string();
    let rules = fixture(&place.dir, "no-account.toml", NO_ACCOUNT_RULES);
    let doctor = |args: &[&str]| Command::new(bin()).arg("doctor").args(args).output().ok();
    let live = doctor(&["--state-dir", &state, "--tmux-socket", &place.socket, "--rules", &rules]);
    assert_eq!(live.as_ref().map(rc_of), Some(i32::from(RC_OK)), "{live:?}");
    let lines: Vec<String> = live.map(|out| stdout_of(&out)).unwrap_or_default().lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 7, "2 行 + 登録 row 1 行 + 突合 1 行 + host の面 1 行 + 導入先 1 行 + host-guard 1 行: {lines:?}");
    assert_eq!(
        lines.get(3..6),
        Some(&["seats: registered=1 live=1 missing=0".to_owned(), HOST_ABSENT.to_owned(), CONSUMER_REPO.to_owned()][..]),
        "席の立つ socket"
    );
    assert!(lines.last().is_some_and(|line| line.starts_with(HOST_GUARD_HEAD)), "末尾は host-guard の 1 行: {lines:?}");
    let elsewhere = place.dir.join("no-server-sock").display().to_string();
    let away = doctor(&["--state-dir", &state, "--tmux-socket", &elsewhere, "--rules", &rules]);
    assert_eq!(away.as_ref().map(rc_of), Some(i32::from(RC_OK)), "{away:?}");
    let away_lines: Vec<String> = away.map(|out| stdout_of(&out)).unwrap_or_default().lines().map(str::to_owned).collect();
    assert_eq!(
        away_lines.get(3..6),
        Some(&["seats: registered=1 live=unmeasurable missing=unmeasurable".to_owned(), HOST_ABSENT.to_owned(), CONSUMER_REPO.to_owned()][..]),
        "server の無い socket は 0 と書かない"
    );
    for dup in [
        &["--state-dir", &state, "--tmux-socket", &place.socket, "--tmux-socket", &place.socket][..],
        &["--state-dir", &state, "--tmux-socket", &elsewhere, "--tmux-socket", &place.socket],
        &["--tmux-socket", &place.socket, "--state-dir", &state, "--tmux-socket", &place.socket],
    ] {
        let out = doctor(dup);
        assert_eq!(out.as_ref().map(rc_of), Some(i32::from(RC_REFUSED)), "{dup:?} は使い方で断る");
        let stdout = out.map(|found| stdout_of(&found)).unwrap_or_default();
        assert!(stdout.starts_with("usage: "), "{dup:?}: {stdout}");
        assert_eq!(stdout.lines().count(), 1, "{dup:?}: 使い方の 1 行だけ: {stdout}");
        assert!(!stdout.contains("seats:"), "{dup:?}: 2 つ目が黙って勝たない（突合の行を出さない）: {stdout}");
    }
    drop(seat);
    fs::remove_dir_all(&place.dir).ok();
}

/// binary を `--version` で撃つと `NAME version (build 元 commit)` の 1 行（rc 0・stderr 空）。`--version` を先頭以外に
/// 置いても使い方（rc 1）で、version の行は出ない。
///
/// .192 の検出線で生き残った変異 `dispatch` の `Some("--version")` arm の削除は、bin crate の in-module 歯
/// （`render_version` の戻り値を見る）からは binary を撃てないので捕まらない。e2e 側で binary の外形を pin する
/// （ADR-0013）。現物の挙動を pin する歯なので base でも通る（retroactive）。括弧の中身は同じ package の
/// `build.rs` が焼いた `SCRIBE2_BUILD_COMMIT`（e2e の target にも同じ値が渡る・consumer-sync.md §2・`s2-07l.302`）。
// flip-check: retroactive s2-07l.196
#[test]
fn mutant_e2e_version_flag_prints_name_and_version_on_the_binary() {
    let out = Command::new(bin()).arg("--version").output().ok();
    assert_eq!(out.as_ref().map(rc_of), Some(i32::from(RC_OK)), "{out:?}");
    let expected = format!("{NAME} {} ({})\n", env!("CARGO_PKG_VERSION"), env!("SCRIBE2_BUILD_COMMIT"));
    assert_eq!(out.as_ref().map(stdout_of), Some(expected.clone()), "NAME + version + (build 元 commit) の 1 行");
    assert_eq!(out.as_ref().map(stderr_of).as_deref(), Some(""), "stderr は空");
    assert!(!expected.starts_with("usage: "), "使い方でない");
    let name = Command::new(bin()).arg("name").output().ok();
    assert_eq!(name.as_ref().map(stdout_of).as_deref(), Some(format!("{NAME}\n").as_str()), "`name` は NAME だけ（version の行と別）");
    let misplaced = Command::new(bin()).args(["doctor", "--version"]).output().ok();
    assert_eq!(misplaced.as_ref().map(rc_of), Some(i32::from(RC_REFUSED)), "{misplaced:?}");
    let stdout = misplaced.map(|found| stdout_of(&found)).unwrap_or_default();
    assert!(stdout.starts_with("usage: ") && !stdout.contains(&expected), "先頭以外の `--version` は使い方: {stdout}");
}

// ─────────── build script の再走の引き金（consumer-sync.md §18・行 i・`s2-07l.317`） ───────────

// `rerun_paths` の実体は build.rs と同じ 1 file（列挙の実装は 1 か所・build script は歯の外なので、関数を
// 同じ file から読んで直に撃つ）。
include!("../../../scribe2/build/rerun.rs");

/// `git -C <dir> <args>` を撃ち、rc 0 だったか。
fn build_rerun_git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// tmp の git repo（symlink を解いた root）: tracked 3 本（1 本は sub dir）・untracked 1 本・tracked にしてから
/// 作業木で消した 1 本。commit は撃たない（`ls-files` は index を読む）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn build_rerun_repo() -> TmpDir {
    let dir = tmp().canonical().expect("tmp dir の実 path を解ける");
    assert!(build_rerun_git(&dir, &["init", "-q"]), "git init が通る");
    fs::create_dir_all(dir.join("sub")).expect("sub dir を作れる");
    for name in ["a.txt", "b.txt", "sub/c.txt", "gone.txt", "untracked.txt"] {
        fs::write(dir.join(name), name).expect("fixture を書ける");
    }
    assert!(
        build_rerun_git(&dir, &["add", "--", "a.txt", "b.txt", "sub/c.txt", "gone.txt"]),
        "git add が通る"
    );
    fs::remove_file(dir.join("gone.txt")).expect("tracked の 1 本を作業木から消せる");
    dir
}

/// tmp の git repo で、tracked で在る 3 本の絶対 path と HEAD / index を sort 済みで返し、untracked と消えた
/// tracked を含まない（行 i の done (1)）。
#[test]
fn build_rerun_paths_are_existing_tracked_files_and_git_meta() {
    let dir = build_rerun_repo();
    let found = rerun_paths(&dir);
    let expected: Vec<PathBuf> = [".git/HEAD", ".git/index", "a.txt", "b.txt", "sub/c.txt"]
        .iter()
        .map(|name| dir.join(name))
        .collect();
    assert_eq!(found, expected, "tracked で在る 3 本 + HEAD / index（絶対 path・sort 済み）");
    assert!(found.iter().all(|path| path.is_absolute()), "全部が絶対 path: {found:?}");
    assert!(!found.contains(&dir.join("untracked.txt")), "untracked は含まない: {found:?}");
    assert!(!found.contains(&dir.join("gone.txt")), "消えた tracked は含まない: {found:?}");
}

/// 同じ repo で 2 回撃つと同じ並びを返し、その並びは sort 済みで重複が無い（決定的・行 i の done (1)）。
#[test]
fn build_rerun_paths_are_deterministic() {
    let dir = build_rerun_repo();
    let first = rerun_paths(&dir);
    let second = rerun_paths(&dir);
    assert_eq!(first, second, "2 回目も同じ並び");
    let mut sorted = first.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(first, sorted, "sort 済みで重複なし");
    assert_eq!(first.len(), 5, "数も揃う: {first:?}");
}

/// git repo でない dir では、`<dir>/../../.git` の HEAD / index のうち在る物だけを返す（0 本を含む）。
/// `.git` は HEAD だけを置いた素の dir（git は repo と読まない）で、tracked の列挙は 1 本も足されない。
#[test]
fn build_rerun_paths_outside_a_repo_are_only_existing_git_meta() {
    let root = tmp().canonical();
    assert!(root.is_some(), "tmp dir の実 path を解ける");
    let Some(root) = root else { return };
    let dir = root.join("crates").join("pkg");
    assert!(fs::create_dir_all(&dir).is_ok(), "dir を作れる");
    assert!(!build_rerun_git(&dir, &["rev-parse", "--show-toplevel"]), "前提: git repo でない");
    assert_eq!(rerun_paths(&dir), Vec::<PathBuf>::new(), "meta も無い周は 0 本");
    assert!(fs::create_dir_all(root.join(".git")).is_ok(), ".git dir を作れる");
    assert!(fs::write(root.join(".git").join("HEAD"), "ref: refs/heads/main\n").is_ok(), "HEAD を書ける");
    assert!(fs::write(dir.join("stray.txt"), "x").is_ok(), "dir に file を書ける");
    assert!(!build_rerun_git(&dir, &["rev-parse", "--show-toplevel"]), "前提: HEAD だけの .git は repo でない");
    let found = rerun_paths(&dir);
    assert_eq!(found, vec![dir.join("..").join("..").join(".git").join("HEAD")], "在る HEAD だけ（index は無い）");
}

/// build.rs は列挙を `build/rerun.rs` から `include!` で読み、自分で `rerun_paths` を定義しない＝歯が撃つ関数と
/// build script が撃つ関数が同じ 1 つ（行 i の done (2)）。build.rs の doc は再走の母集団を tracked 全 file と名指す
/// （done (3)）。
#[test]
fn build_rerun_build_script_reads_the_shared_file() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(NAME);
    let script = fs::read_to_string(crate_dir.join("build.rs")).unwrap_or_default();
    let shared = fs::read_to_string(crate_dir.join("build").join("rerun.rs")).unwrap_or_default();
    assert_eq!(script.matches("\ninclude!(\"build/rerun.rs\");\n").count(), 1, "build.rs が 1 回 include! する");
    assert_eq!(script.matches("fn rerun_paths(").count(), 0, "build.rs は列挙を自分で持たない");
    assert_eq!(shared.matches("\nfn rerun_paths(").count(), 1, "列挙は共有 file の 1 関数");
    assert!(script.contains("emit(&format!(\"cargo:rerun-if-changed={}\", path.display()))"), "列挙を rerun-if-changed に出す");
    assert!(script.contains("**tracked 全 file**"), "doc が再走の母集団を tracked 全 file と名指す");
}

// ─────────────────── 口座の歯の共有 fixture（account-autonomy.md §5・`s2-07l.211`） ───────────────────

/// 実測行の reset（遠い未来＝どの「いま」でも古くない）。
const ACCT_RESET: &str = "2099-01-01T05:00:00Z";
/// 登録を撃った session の sid（打刻から解かれて row に載る）。
const ACCT_SID: &str = "sid-acct";
/// 登録 row の anchor（置き場の配下に実在する dir・[`acct_anchor`]・起動行の `cd` 先＝`s2-07l.324`）。
const ACCT_ANCHOR_DIR: &str = "acct";
/// 立て直さない歯の起動の雛形（穴 1 つ・実行はされない）。
const ACCT_LAUNCH: &str = "cld {account_dir}";

/// 口座の歯の置き場（tmp・置き場・退避物の dir・独立 socket）。
struct AcctPlace {
    /// tmp の根。
    dir: TmpDir,
    /// `--state-dir`。
    state: PathBuf,
    /// `--wm-dir`（空で在る＝走査が 0 件と確かめられる）。
    /// 独立 socket。
    socket: String,
}

/// 置き場を 1 つ作る（登録 row の anchor の dir も実在させる＝起動行の `cd` が通る）。
fn acct_place() -> AcctPlace {
    let dir = tmp();
    let state = dir.join("state");
    fs::create_dir_all(dir.join(ACCT_ANCHOR_DIR)).ok();
    let socket = socket_of(&dir);
    AcctPlace { dir, state, socket }
}

/// 登録 row の anchor（置き場の配下の実在する dir・絶対 path）。
fn acct_anchor(place: &AcctPlace) -> String {
    place.dir.join(ACCT_ANCHOR_DIR).display().to_string()
}

/// 席を planner として口座 `account` で登録する（打刻の sid を先に置く＝`seat register` の条件）。
fn acct_register_as(place: &AcctPlace, target: &str, account: &str, launch: &str) -> Output {
    let seat = seat_dir_of(&place.state, target);
    fs::create_dir_all(&seat).ok();
    fs::write(state_file(&seat), format!("{}\n", stamp_line("idle", "SessionStart", unix_now(), ACCT_SID))).ok();
    let launch_file = fixture(&place.dir, "launch.txt", launch);
    let state = place.state.display().to_string();
    run_seat(&[
        "register", "--state-dir", &state, "--target", target, "--role", "orchestrator", "--account", account,
        "--launch", &launch_file, "--anchor", &acct_anchor(place),
    ])
}

/// 口座 1 つの実測行（5 時間窓 = `pct`・7 日窓 = 1）を時刻 `ts` で event log に積む（`fleet usage` が積むのと同じ形）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn acct_measured(state: &Path, label: &str, pct: u64, ts: &str) {
    use vessel::fleet::{Allowance, Event, EventKind, Measured, WindowKind, SCHEMA};
    let policy = vessel::fleet::store::LockPolicy::embedded().expect("lock の規則を読める");
    for (window, used_pct) in [(WindowKind::FiveHour, pct), (WindowKind::SevenDay, 1)] {
        let allowance = Allowance::Measured(Measured {
            account: label.to_owned(),
            window,
            model: None,
            endpoint: "oauth-usage".to_owned(),
            used_pct,
            resets_at: Some(ACCT_RESET.to_owned()),
        });
        let event = Event {
            schema: SCHEMA,
            ts: ts.to_owned(),
            kind: EventKind::AllowanceMeasured,
            run: String::new(),
            bead: String::new(),
            host: "h".to_owned(),
            actor: "machine".to_owned(),
            stage: None,
            seat: None,
            pid: None,
            detail: None,
            allowance: Some(allowance),
            registration: None,
            mark: None,
            account: None,
            cost: None,
            rule: None,
        };
        vessel::fleet::store::append(state, &event, policy).expect("実測行を積める");
    }
}

/// いまの UTC（実測行の時刻の字面）。
fn acct_now() -> String {
    vessel::fleet::cli::now_utc()
}

/// 記録 1 行の文字列の値。
fn acct_text(line: &str, key: &str) -> Option<String> {
    json_value(line, key).and_then(|value| value.as_str().map(str::to_owned))
}

/// 席の記録（`tick.jsonl`）のうち注入の経路（`seat-inject`）と立て直しの起動（`seat-cycle`）が積んだ行の `what`（送った順）。
fn acct_sent(state: &Path, target: &str) -> Vec<String> {
    fs::read_to_string(tick_file(state, target))
        .unwrap_or_default()
        .lines()
        .filter(|line| matches!(acct_text(line, "who").as_deref(), Some("seat-inject" | "seat-cycle")))
        .filter_map(|line| acct_text(line, "what"))
        .collect()
}

/// event log の登録 row（積んだ順）。
fn acct_rows(state: &Path) -> Vec<vessel::fleet::Registration> {
    vessel::fleet::store::read_all(state)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|event| event.registration)
        .collect()
}

/// 立て直しと起動の起動行の前置（`s2-07l.324`・account-lifecycle.md §4）: `cd '<row の anchor>' && ` が agent view の env より
/// **前**に来る＝`cd … && CLAUDE_CODE_DISABLE_AGENT_VIEW=1 <tail>`。`tail` は前置の後の字面の先頭（雛形か `CLAUDE_CONFIG_DIR=`）。
fn acct_launch_prefix(anchor: &str, tail: &str) -> String {
    format!("cd '{anchor}' && CLAUDE_CODE_DISABLE_AGENT_VIEW=1 {tail}")
}

/// pane が `ready` を満たすまで待つ（上限 [`PROMPT_WAIT`]）。
fn acct_wait_pane(place: &AcctPlace, target: &str, ready: impl Fn(&str) -> bool) -> bool {
    let deadline = Instant::now().checked_add(PROMPT_WAIT);
    while deadline.is_some_and(|at| Instant::now() < at) {
        if ready(&capture(&place.socket, target)) {
            return true;
        }
        sleep(Duration::from_millis(100));
    }
    false
}

// ─────────────────── role / register の共有 fixture（`register` / `account` / `rules` と外形 snapshot が使う） ───────────────────

/// 登録の置き場の fixture（state dir・雛形 file・tmux socket）。
pub(super) struct RolePlace {
    /// tmp dir の root。
    pub(super) dir: TmpDir,
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

/// `doctor --state-dir` を撃つ（manifest は `[[account]]` の無い `--rules`＝口座の行 0 本・突合までの外形）。
pub(super) fn role_doctor(place: &RolePlace) -> Output {
    role_doctor_rules(place, NO_ACCOUNT_RULES)
}

/// `doctor --state-dir --tmux-socket --rules --bin` を撃つ（`body` の manifest を置き場の dir に書いて渡す・binary の解決は
/// 歯の binary 自身へ差し替える＝`binary=ok` で継いだ PATH に依らない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn role_doctor_rules(place: &RolePlace, body: &str) -> Output {
    let state = place.state.display().to_string();
    let rules = fixture(&place.dir, "doctor-rules.toml", body);
    Command::new(bin())
        .args(["doctor", "--state-dir", &state, "--tmux-socket", &place.socket, "--rules", &rules, "--bin", bin()])
        .output()
        .expect("binary を起動できる")
}

/// 登録 1 件（anchor `/repo`）を置いた置き場（tmux の server は呼び側が立てる）。
///
/// 鍵は (役割, anchor) で役割は 1 つ（ADR-0045 §2 (1)）＝2 件を置くには anchor を分けるほかなく、
/// anchor が増えると導入先の行と口座の trust の値も増える。**2 件が要る歯はその歯が自分で足す**
/// （[`role_register_extra`]）。
pub(super) fn role_doctor_place() -> RolePlace {
    let place = role_place();
    role_stamp(&place, "rolesdoc:rolesdoc", Some("sid-doc"));
    let out = role_register(&place, "rolesdoc:rolesdoc", "orchestrator", &["--anchor", "/repo"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    place
}

/// 別 anchor の登録 row を 1 件足す（鍵を分けるのは anchor・[`role_doctor_place`] の対）。
pub(super) fn role_register_extra(place: &RolePlace, target: &str, anchor: &str) {
    role_stamp(place, target, Some("sid-doc"));
    let out = role_register(place, target, "orchestrator", &["--anchor", anchor]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
}

// ─────────────────── doctor の口座の行の共有 fixture（s2-07l.233・account-autonomy.md §5・`account` / `rules` が使う） ───────────────────

/// `[[account]]` の無い manifest（口座の行 0 本）。
pub(super) const NO_ACCOUNT_RULES: &str = "schema = 1\n";

/// 置き場に `host.toml` の無い周の doctor の host の面の行（突合の行の直後・口座の行の直前）。
pub(super) const HOST_ABSENT: &str = "host-manifest=absent";

/// 登録 row の anchor `/repo` が導入先として出る 1 行（口座の行の後ろ・記録なし・帳簿なし・`[[vessel]]` なし・
/// consumer-sync.md §4・`s2-07l.303`）。
pub(super) const CONSUMER_REPO: &str =
    "consumer=/repo source=launch scope=- binary=unrecorded plugin=unrecorded ledger=- cache=absent head=undeclared behind=- drift=unrecorded";

/// doctor の host-guard の 1 行の頭（導入先の行の後ろ・`--state-dir` の周は口座 0 でも必ず 1 行・vessel-hook.md §12 形 5）。
pub(super) const HOST_GUARD_HEAD: &str = "host-guard: ";

/// 種類の行を持たない manifest（[`NO_ACCOUNT_RULES`]）・口座 0 の host・`--bin` が歯の binary の周の host-guard の 1 行。
pub(super) const HOST_GUARD_BARE: &str =
    "host-guard: git=no-row tmux=no-row ledger=no-row rm=no-row self=on rows=0/4 wired=0/0 entities=0 binary=ok";

/// `[[account]]` を `labels` の順に宣言した manifest の本文。
fn account_rules(labels: &[&str]) -> String {
    labels.iter().fold(NO_ACCOUNT_RULES.to_owned(), |body, label| format!("{body}\n[[account]]\nlabel = \"{label}\"\n"))
}

/// doctor を撃ち、rc 0 を確かめて stdout の行を返す。
fn doctor_rows(place: &RolePlace, rules: &str) -> Vec<String> {
    let out = role_doctor_rules(place, rules);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    stdout_of(&out).lines().map(str::to_owned).collect()
}

// ─────────────────── 席の起動の共有 fixture（account-lifecycle.md §4・`s2-07l.244`・`launch` / `account` が使う） ───────────────────

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
/// 包みの tmux が `-t` の無い `display-message` に返す**呼び手の target**（`session:window`・無ければ問いは rc 1）。
const LAUNCH_CALLER_TARGET: &str = "caller-target";
/// 同じく**呼び手の session の名**（空の file を置けば「名が空」の周になる）。
const LAUNCH_CALLER_SESSION: &str = "caller-session";
/// 包みの tmux が rc 1 で返す verb（1 語・撃てない tmux の周を作る）。
const LAUNCH_TMUX_REFUSE: &str = "tmux-refuse";

/// 偽 tmux が `-t` の無い `display-message` に返す呼び手の target を据える（呼び手の pane が target の pane そのものの周）。
fn launch_caller_target(place: &AcctPlace, target: &str) {
    fs::write(place.dir.join(LAUNCH_CALLER_TARGET), target).ok();
}

/// 同じく呼び手の session の名を据える（空文字なら「名が空」の周）。
fn launch_caller_session(place: &AcctPlace, session: &str) {
    fs::write(place.dir.join(LAUNCH_CALLER_SESSION), session).ok();
}

/// 偽 tmux が `verb` の呼出しを rc 1 で返すようにする（`capture-pane` = pane を読めない・`new-window` = 窓を作れない）。
fn launch_tmux_refuse(place: &AcctPlace, verb: &str) {
    fs::write(place.dir.join(LAUNCH_TMUX_REFUSE), verb).ok();
}

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

/// anchor の dir（登録 row の `anchor`）。
fn launch_anchor(place: &AcctPlace) -> String {
    place.dir.join("anchor").display().to_string()
}

/// anchor の下の生成 dir（起動行の 1 つ目の `--plugin-dir`・設計 consumer-sync.md §17 形 3）。
fn launch_plugin_root(place: &AcctPlace) -> String {
    place.dir.join("anchor").join(PLUGIN_DIR).display().to_string()
}

/// 期待する偽 claude の記録（argv を 1 語 1 行・続けて env の 2 行）。
fn launch_expected_argv(place: &AcctPlace, label: &str) -> String {
    let account_dir = place.state.join("accounts").join(label).display().to_string();
    format!(
        "--plugin-dir\n{}\n--plugin-dir\n{}\n--plugin-dir\n{}\n{}\n{}\nenv:CLAUDE_CONFIG_DIR={account_dir}\nenv:CLAUDE_CODE_DISABLE_AGENT_VIEW=1\n",
        launch_plugin_root(place), LAUNCH_PLUGINS[0], LAUNCH_PLUGINS[1], LAUNCH_ARGS[0], LAUNCH_ARGS[1]
    )
}

/// test 自身の PATH に在る tmux（包みが exec する実体）。
fn real_tmux() -> Option<PathBuf> {
    std::env::var("PATH").ok()?.split(':').map(|dir| Path::new(dir).join("tmux")).find(|path| path.is_file())
}

/// shim の dir を作る（偽 `claude`・argv を写して実体へ exec する `tmux` の包み）: 偽 claude は argv と env を `launched` へ
/// 写し、その時点の event log を [`LAUNCH_EVENTS_SEEN`] へ複製し、prompt を描いて `SessionStart` を打ち、以後は受けた行を
/// [`LAUNCH_LOG`] に積んで `UserPromptSubmit` → `Stop` を打つ（立て直しの偽 session と同じ形）。PATH の字面を返す。
///
/// 偽 tmux は 3 つの口を持つ（seat-roles.md §26・どれも fixture file が在る周だけ働き、無い周は実体へ素通しする）:
/// [`LAUNCH_TMUX_REFUSE`] の verb を rc 1 で返す口・**`-t` の無い** `display-message` に
/// [`LAUNCH_CALLER_TARGET`] / [`LAUNCH_CALLER_SESSION`] を返す口（呼び手の pane を器が測る 1 問い・file が無ければ rc 1）。
/// `-t` を持つ呼出しは 1 つも横取りしない（pane の実物は本物の tmux が答える）。
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
        start = stamp_cmd(&file, "idle", "SessionStart"),
        log = place.dir.join(LAUNCH_LOG).display(),
        busy = stamp_cmd(&file, "busy", "UserPromptSubmit"),
        stop = stamp_cmd(&file, "idle", "Stop"),
    );
    let real = real_tmux().unwrap_or_default().display().to_string();
    let tmux = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{args}'\n\
         refuse=''\n[ -f '{refuse}' ] && refuse=$(cat '{refuse}')\n\
         if [ -n \"$refuse\" ]; then case \" $* \" in *\" $refuse \"*) exit 1;; esac; fi\n\
         case \" $* \" in *\" -t \"*) exec '{real}' \"$@\";; esac\n\
         case \"$*\" in\n\
         *'#{{session_name}}:#{{window_name}}') [ -f '{ctarget}' ] || exit 1; cat '{ctarget}'; exit 0;;\n\
         *'#{{session_name}}') [ -f '{csession}' ] || exit 1; cat '{csession}'; exit 0;;\n\
         esac\n\
         exec '{real}' \"$@\"\n",
        args = place.dir.join(LAUNCH_TMUX_ARGS).display(),
        refuse = place.dir.join(LAUNCH_TMUX_REFUSE).display(),
        ctarget = place.dir.join(LAUNCH_CALLER_TARGET).display(),
        csession = place.dir.join(LAUNCH_CALLER_SESSION).display(),
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
        "seat", "launch", "--state-dir", &state, "--role", "orchestrator", "--target", target, "--anchor", &anchor, "--tmux-socket", &place.socket,
    ];
    args.extend_from_slice(extra);
    Command::new(bin()).args(&args).env("PATH", path).output().expect("binary を起動できる")
}

/// 送った行のうち `--model` の語の数（起動行が model を**ちょうど 1 つ**運ぶことの計測）。
fn launch_model_words(line: &str) -> usize {
    line.split(' ').filter(|word| *word == "--model").count()
}

// ─────────────────── 管理 tick（seat-heartbeat.md §2・契約表の行 a・`s2-07l.582`・接頭辞 `seat_tick_`） ───────────────────
//
// pane は PATH の偽 tmux（file の pane を読み書きし、呼出を 1 行ずつ残す）が持ち、tmux の server は 1 つも立てない＝送った key
// を呼出の行で数える（`send-keys` の 1 行が 1 回の送り）。偽 client（`curl` / `claude`）も PATH に置き、呼ばれたら file に残す。
// 時刻は打刻の `ts` と梯子の記録の `sent_at` を過去に書いて進める（偽の時計）。字面は契約から組む（実装の helper を使わない）。

/// tick の歯の target。
const TICK_TARGET: &str = "tk:tk";
/// 潰した target（席の置き場の dir 名・判定行の `target=`）。
const TICK_SEAT: &str = "tk_tk";
/// 登録 row の口座 label。
const TICK_ACCOUNT: &str = "acct-tick";
/// `seat.tick_stale_s` の初期値（秒・値は rules の歯が埋め込み manifest で pin する）。
const TICK_STALE: u64 = 2400;
/// 偽 tmux の pane の file。
const TICK_PANE: &str = "pane";
/// 偽 tmux の呼出の記録。
const TICK_CALLS: &str = "tmux-calls";
/// 在れば偽 tmux の Enter が入力欄を空にしない（Enter の効かない席）。
const TICK_STUCK: &str = "tmux-stuck";
/// 在れば偽 tmux の `send-keys` が rc 1（送れない tmux）。
const TICK_REFUSE: &str = "tmux-refuse-send";
/// 偽 client の呼出の記録。
const TICK_CLIENT: &str = "client-calls";
/// 空の入力欄の pane。
const TICK_CLEAR_PANE: &str = "old output\n\u{276f} ";

/// tick の歯の置き場（tmp・`--state-dir`・偽 tmux と偽 client を先頭に置いた PATH）。
struct TickPlace {
    /// tmp の根（pane と呼出の記録の置き場）。
    dir: TmpDir,
    /// `--state-dir`。
    state: PathBuf,
    /// 偽の bin を先頭に置いた PATH。
    path: String,
}

impl TickPlace {
    /// 席の置き場（`<state>/seat/<潰した target>/`）。
    fn seat(&self) -> PathBuf {
        seat_dir_of(&self.state, TICK_SEAT)
    }

    /// tmp の根の下の file。
    fn at(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

/// 置き場を 1 つ作る（`registered` なら登録 row を `seat register` で積む・pane は空の入力欄）。
fn tick_place(registered: bool) -> TickPlace {
    let dir = tmp();
    let state = dir.join("state");
    let seat = seat_dir_of(&state, TICK_SEAT);
    fs::create_dir_all(&seat).ok();
    fs::write(state_file(&seat), format!("{}\n", stamp_line("idle", "SessionStart", unix_now(), "sid-tick"))).ok();
    if registered {
        let launch = fixture(&dir, "launch.txt", "claude\n");
        let path = state.display().to_string();
        let out = run_seat(&[
            "register", "--state-dir", &path, "--target", TICK_TARGET, "--role", "orchestrator", "--account", TICK_ACCOUNT,
            "--launch", &launch, "--anchor", "/repo",
        ]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "登録 row を積める: {}", stderr_of(&out));
    }
    fs::write(dir.join(TICK_PANE), TICK_CLEAR_PANE).ok();
    let path = tick_shims(&dir);
    TickPlace { dir, state, path }
}

/// 偽 tmux と偽 client を `<dir>/bin` に置き、PATH の字面を返す。偽 tmux は呼出を 1 行残し、`capture-pane` は pane の file を
/// 返し（file が無ければ rc 1）、`send-keys … -l <text>` は text を入力欄へ足し、`send-keys … Enter` は入力欄を送って新しい
/// prompt を描く（[`TICK_STUCK`] が在れば何もしない・[`TICK_REFUSE`] が在れば `send-keys` は rc 1）。
fn tick_shims(dir: &Path) -> String {
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).ok();
    let at = |name: &str| dir.join(name).display().to_string();
    let tmux = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{calls}'\n[ -f '{pane}' ] || exit 1\n\
         case \" $* \" in *' capture-pane '*) cat '{pane}'; exit 0;; esac\n\
         [ -f '{refuse}' ] && exit 1\n\
         for last in \"$@\"; do :; done\n\
         case \" $* \" in *' -l '*) printf '%s' \"$last\" >> '{pane}'; exit 0;; esac\n\
         [ -f '{stuck}' ] && exit 0\n\
         printf '\\n\u{276f} ' >> '{pane}'\n",
        calls = at(TICK_CALLS),
        pane = at(TICK_PANE),
        refuse = at(TICK_REFUSE),
        stuck = at(TICK_STUCK),
    );
    let client = format!("#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> '{}'\n", at(TICK_CLIENT));
    for (name, body) in [("tmux", tmux), ("curl", client.clone()), ("claude", client)] {
        let path = bin.join(name);
        fs::write(&path, body).ok();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).ok();
    }
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default())
}

/// `seat tick` を偽の PATH で 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn tick_run(place: &TickPlace, extra: &[&str]) -> Output {
    let state = place.state.display().to_string();
    Command::new(bin())
        .args(["seat", "tick", "--state-dir", &state, "--target", TICK_TARGET])
        .args(extra)
        .env("PATH", &place.path)
        .output()
        .expect("binary を起動できる")
}

/// 打刻 file を `(state, event, ts)` の列で書き直す。
fn tick_stamps(place: &TickPlace, stamps: &[(&str, &str, u64)]) {
    let body: String = stamps.iter().map(|(state, event, ts)| format!("{}\n", stamp_line(state, event, *ts, "sid-tick"))).collect();
    fs::write(state_file(&place.seat()), body).ok();
}

/// 最終行が Idle の Stop で `ago` 秒前の打刻 1 行に書き直す。
fn tick_silent_for(place: &TickPlace, ago: u64) {
    tick_stamps(place, &[("idle", "Stop", unix_now().saturating_sub(ago))]);
}

/// 梯子の記録の path（契約の字面から組む）。
fn tick_ladder_path(place: &TickPlace) -> PathBuf {
    place.seat().join("pointer-ladder")
}

/// 梯子の記録を書く（偽の時計・契約の字面から組む）。
fn tick_ladder_put(place: &TickPlace, sent_at: u64, step: u32, digest: Option<u64>) {
    let digest = digest.map_or_else(|| "null".to_owned(), |ts| ts.to_string());
    let line = format!("{{\"schema\":1,\"sent_at\":{sent_at},\"step\":{step},\"digest\":{digest}}}\n");
    fs::write(tick_ladder_path(place), line).ok();
}

/// 梯子の記録（`sent_at` / `step` / `digest`）。**ちょうど 1 行・schema 1** の周だけ `Some`。
fn tick_ladder(place: &TickPlace) -> Option<(u64, u64, Option<u64>)> {
    use vessel::fleet::json_lite::Value;
    let text = fs::read_to_string(tick_ladder_path(place)).ok()?;
    let mut lines = text.lines();
    let line = lines.next()?;
    if lines.next().is_some() || json_value(line, "schema")?.as_num()? != 1 {
        return None;
    }
    let digest = match json_value(line, "digest")? {
        Value::Null => None,
        other => Some(other.as_num()?),
    };
    Some((json_value(line, "sent_at")?.as_num()?, json_value(line, "step")?.as_num()?, digest))
}

/// 偽 tmux へ送った key（`send-keys` の呼出の行・送った順）。
fn tick_keys(place: &TickPlace) -> Vec<String> {
    fs::read_to_string(place.at(TICK_CALLS))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("send-keys"))
        .map(str::to_owned)
        .collect()
}

/// 注入の記録（`tick.jsonl`）の行。
fn tick_injections(place: &TickPlace) -> Vec<String> {
    fs::read_to_string(tick_file(&place.state, TICK_SEAT)).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// 段 n の待ち（秒・初期値の梯子 2400 × 2 ^ n）。
fn tick_wait(step: u32) -> u64 {
    TICK_STALE.saturating_mul(2_u64.saturating_pow(step))
}

/// 段 `step` の合図の文面（契約の字面・設計 §2 形 3）。
fn tick_signal(step: u32) -> String {
    format!(
        "{NAME} tick: heartbeat step={step} — 台帳の現在地（bd --readonly ready --limit 0）から続きを進める（変化が無ければ次の合図は {} 秒後・上限で打ち切り）",
        tick_wait(step.saturating_add(1))
    )
}

/// 合図の text を送った key（`-l` の呼出の行）。
fn tick_text_key(step: u32) -> String {
    format!("send-keys -t {TICK_TARGET} -l {}", tick_signal(step))
}

/// noop の判定行。
fn tick_noop(reason: &str, pointer: &str, step: &str) -> String {
    format!("decision=noop target={TICK_SEAT} reason={reason} pointer={pointer} step={step} consumed=-\n")
}

/// inject の判定行（偽 tmux は消費の打刻を打たない＝queue の `consumed=false`）。
fn tick_inject(step: u32) -> String {
    format!("decision=inject target={TICK_SEAT} reason=- pointer=sent step={step} consumed=false\n")
}

/// 撃って判定行が `want` で rc 0 の周に、key も梯子の記録も注入の記録も増えず、偽 client も呼ばれないことを測る。
fn tick_assert_quiet(place: &TickPlace, want: &str) {
    let (keys, ladder, injections) = (tick_keys(place).len(), tick_ladder(place), tick_injections(place).len());
    let out = tick_run(place, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "{want}: rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), want, "判定行");
    assert_eq!(tick_keys(place).len(), keys, "{want}: 1 key も送らない");
    assert_eq!(tick_ladder(place), ladder, "{want}: 梯子の記録は動かない");
    assert_eq!(tick_injections(place).len(), injections, "{want}: 注入の記録は増えない");
    assert!(!place.at(TICK_CLIENT).exists(), "{want}: 偽 client は呼ばれない");
}

/// (a) 登録 row ∧ 最終行 Idle が 40 分以上前 ∧ 入力欄が空 → 合図 1 行を注入し、梯子の記録 1 行（段 0・基準 null）と注入の記録
/// 1 行（`who`=`seat-inject`）が増える。送った key は text 1 回と Enter 1 回だけ。
#[test]
fn seat_tick_injects_one_signal_into_a_silent_registered_seat() {
    let place = tick_place(true);
    tick_silent_for(&place, TICK_STALE + 60);
    let before = unix_now();
    let out = tick_run(&place, &[]);
    let after = unix_now();
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), tick_inject(0), "判定行 1 行");
    assert_eq!(tick_keys(&place), [tick_text_key(0), format!("send-keys -t {TICK_TARGET} Enter")], "text 1 回 + Enter 1 回");
    let pane = fs::read_to_string(place.at(TICK_PANE)).unwrap_or_default();
    assert_eq!(pane.matches(&tick_signal(0)).count(), 1, "pane に合図 1 行: {pane}");
    let (sent_at, step, digest) = tick_ladder(&place).expect("梯子の記録が 1 行在る");
    assert!((before..=after).contains(&sent_at), "sent_at は送った時刻: {sent_at}");
    assert_eq!((step, digest), (0, None), "段 0・基準 null");
    let injections = tick_injections(&place);
    assert_eq!(injections.len(), 1, "注入の記録 1 行: {injections:?}");
    let line = injections.first().map(String::as_str).unwrap_or_default();
    assert_eq!(acct_text(line, "who").as_deref(), Some("seat-inject"), "既存の注入の経路の who: {line}");
    let what = acct_text(line, "what").unwrap_or_default();
    assert!(what.starts_with(&format!("{NAME} tick: heartbeat step=0")) && tick_signal(0).starts_with(&what), "what は合図の頭: {what}");
    assert!(!place.at(TICK_CLIENT).exists(), "偽 client は呼ばれない");
}

/// (b) 梯子の手前で止まる周（登録 row 無し・打刻無し・読めない・Busy・Busy が 40 分より古い）は理由を判定行に出し、
/// `pointer=- step=-`（評価していない印）で 1 key も送らず記録も増えない。
#[test]
fn seat_tick_refuses_before_the_ladder_without_a_key_or_a_record() {
    let bare = tick_place(false);
    tick_silent_for(&bare, TICK_STALE + 60);
    tick_assert_quiet(&bare, &tick_noop("no-row", "-", "-"));
    let place = tick_place(true);
    let file = state_file(&place.seat());
    fs::remove_file(&file).ok();
    tick_assert_quiet(&place, &tick_noop("state-missing", "-", "-"));
    fs::write(&file, "not json\n").ok();
    tick_assert_quiet(&place, &tick_noop("state-unreadable", "-", "-"));
    fs::remove_file(&file).ok();
    fs::create_dir_all(&file).ok();
    tick_assert_quiet(&place, &tick_noop("state-unreadable", "-", "-"));
    fs::remove_dir(&file).ok();
    let now = unix_now();
    tick_stamps(&place, &[("idle", "Stop", now - 5000), ("busy", "UserPromptSubmit", now - 10)]);
    tick_assert_quiet(&place, &tick_noop("busy", "-", "-"));
    tick_stamps(&place, &[("busy", "UserPromptSubmit", now - TICK_STALE - 60)]);
    tick_assert_quiet(&place, &tick_noop("state-stale", "-", "-"));
    assert!(tick_keys(&place).is_empty() && tick_keys(&bare).is_empty(), "どの周も 0 key");
}

/// (c) 最終行 Idle が 40 分未満前 → `stamp-recent`（梯子は評価済み＝段 0・床は開いて残り 0 秒）で 0 key。
#[test]
fn seat_tick_stamp_recent_sends_nothing() {
    let place = tick_place(true);
    for ago in [100, TICK_STALE - 30] {
        tick_silent_for(&place, ago);
        tick_assert_quiet(&place, &tick_noop("stamp-recent", "wait:0", "0"));
    }
    assert!(tick_keys(&place).is_empty(), "0 key");
}

/// (d) 梯子: 送った直後は `settling`・`sent_at` より後の Stop を足すと基準が入り無変化の床は `wait:<s> step=1`・`sent_at` を待ちの
/// 分だけ過去に書くと段 1〜5 の合図が出て、段 6 は `stopped` で送らない（合図 6 本で打ち切り）。
#[test]
fn seat_tick_ladder_climbs_six_signals_then_stops() {
    let place = tick_place(true);
    tick_silent_for(&place, TICK_STALE + 60);
    assert_eq!(stdout_of(&tick_run(&place, &[])), tick_inject(0), "段 0");
    tick_assert_quiet(&place, &tick_noop("settling", "settling", "0"));
    let sent = unix_now() - 3000;
    tick_ladder_put(&place, sent, 0, None);
    tick_stamps(&place, &[("busy", "UserPromptSubmit", sent + 1), ("idle", "Stop", sent + 2)]);
    let out = tick_run(&place, &[]);
    let line = stdout_of(&out);
    assert!(line.starts_with(&format!("decision=noop target={TICK_SEAT} reason=wait pointer=wait:")), "{line}");
    assert!(line.ends_with(" step=1 consumed=-\n"), "{line}");
    let left = tick_token(&line, "pointer").and_then(|found| found.strip_prefix("wait:").and_then(|secs| secs.parse::<u64>().ok()));
    assert!(left.is_some_and(|secs| (1795..=1800).contains(&secs)), "残り秒 = 80 分 − 50 分: {line}");
    assert_eq!(tick_ladder(&place), Some((sent, 0, Some(sent + 2))), "基準は sent_at より後の Stop の ts");
    for step in 1..=5 {
        let sent = unix_now() - tick_wait(step) - 5;
        tick_ladder_put(&place, sent, step - 1, None);
        tick_stamps(&place, &[("busy", "UserPromptSubmit", sent + 1), ("idle", "Stop", sent + 2)]);
        assert_eq!(stdout_of(&tick_run(&place, &[])), tick_inject(step), "段 {step}");
        assert_eq!(tick_ladder(&place).map(|(_, found, digest)| (found, digest)), Some((u64::from(step), None)), "段 {step} の記録");
    }
    let sent = unix_now() - 200_000;
    tick_ladder_put(&place, sent, 5, None);
    tick_stamps(&place, &[("busy", "UserPromptSubmit", sent + 1), ("idle", "Stop", sent + 2)]);
    let keys = tick_keys(&place).len();
    assert_eq!(stdout_of(&tick_run(&place, &[])), tick_noop("stopped", "stopped", "6"), "段 6 は打ち切り");
    assert_eq!(tick_keys(&place).len(), keys, "段 6 は 0 key");
    assert_eq!(tick_ladder(&place), Some((sent, 5, Some(sent + 2))), "記録は段 5 のまま（基準だけが入る）");
    tick_assert_quiet(&place, &tick_noop("stopped", "stopped", "6"));
    let texts: Vec<String> = tick_keys(&place).into_iter().filter(|key| key.contains(" -l ")).collect();
    assert_eq!(texts, (0..=5).map(tick_text_key).collect::<Vec<_>>(), "合図は段 0〜5 の 6 本");
    assert_eq!(tick_injections(&place).len(), 6, "注入の記録 6 行");
}

/// (e) 基準の後に最終行の ts が動くと段 0 に戻り、40 分黙った周に段 0 で送る（打ち切りの後も同じ）。
#[test]
fn seat_tick_change_returns_to_step_zero_after_forty_silent_minutes() {
    let place = tick_place(true);
    let now = unix_now();
    let base = now - 20_000;
    tick_ladder_put(&place, base - 10, 3, Some(base));
    tick_stamps(&place, &[("idle", "Stop", base), ("busy", "UserPromptSubmit", now - 120), ("idle", "Stop", now - 100)]);
    tick_assert_quiet(&place, &tick_noop("stamp-recent", "wait:0", "0"));
    tick_stamps(&place, &[("idle", "Stop", base), ("idle", "Stop", now - TICK_STALE - 60)]);
    assert_eq!(stdout_of(&tick_run(&place, &[])), tick_inject(0), "変化の後 40 分黙った周は段 0");
    let base = now - 200_000;
    tick_ladder_put(&place, base - 10, 5, Some(base));
    tick_stamps(&place, &[("idle", "Stop", base)]);
    tick_assert_quiet(&place, &tick_noop("stopped", "stopped", "6"));
    tick_stamps(&place, &[("idle", "Stop", base), ("idle", "Stop", now - 100)]);
    tick_assert_quiet(&place, &tick_noop("stamp-recent", "wait:0", "0"));
    tick_stamps(&place, &[("idle", "Stop", base), ("idle", "Stop", now - TICK_STALE - 60)]);
    assert_eq!(stdout_of(&tick_run(&place, &[])), tick_inject(0), "打ち切りの後も変化で段 0");
    let texts: Vec<String> = tick_keys(&place).into_iter().filter(|key| key.contains(" -l ")).collect();
    assert_eq!(texts, [tick_text_key(0), tick_text_key(0)], "送ったのは段 0 の 2 本だけ");
}

/// (f) 応えない席: `sent_at` から 40 分の手前は `settling`・過ぎても Stop が無い周はその周の digest で基準が入り、段の候補は段 + 1。
#[test]
fn seat_tick_unanswered_seat_settles_on_the_stale_digest_and_climbs() {
    let place = tick_place(true);
    let now = unix_now();
    let last = now - 6000;
    tick_stamps(&place, &[("idle", "Stop", last)]);
    tick_ladder_put(&place, now - 1000, 0, None);
    tick_assert_quiet(&place, &tick_noop("settling", "settling", "0"));
    tick_ladder_put(&place, now - 2500, 0, None);
    let line = stdout_of(&tick_run(&place, &[]));
    assert!(line.starts_with(&format!("decision=noop target={TICK_SEAT} reason=wait pointer=wait:")), "{line}");
    assert!(line.ends_with(" step=1 consumed=-\n"), "段 + 1: {line}");
    let left = tick_token(&line, "pointer").and_then(|found| found.strip_prefix("wait:").and_then(|secs| secs.parse::<u64>().ok()));
    assert!(left.is_some_and(|secs| (2295..=2300).contains(&secs)), "残り秒 = 80 分 − 2500 秒: {line}");
    assert_eq!(tick_ladder(&place), Some((now - 2500, 0, Some(last))), "基準はその周の digest");
    assert!(tick_keys(&place).is_empty(), "0 key");
}

/// (g) 口座の門は鮮度の内側の記録だけを読む: 記録無し・鮮度の外は通り（次の門の `input-unknown` で止まる）、鮮度の内側の閾値
/// 以上は `account-pressed`。どの周も偽 client の呼出は 0 件（計測を起こさない）。
#[test]
fn seat_tick_account_gate_reads_only_fresh_records_without_measuring() {
    let place = tick_place(true);
    tick_silent_for(&place, TICK_STALE + 60);
    fs::write(place.at(TICK_PANE), "no prompt here\n").ok();
    tick_assert_quiet(&place, &tick_noop("input-unknown", "wait:0", "0"));
    acct_measured(&place.state, TICK_ACCOUNT, 99, "2020-01-01T00:00:00Z");
    tick_assert_quiet(&place, &tick_noop("input-unknown", "wait:0", "0"));
    acct_measured(&place.state, TICK_ACCOUNT, 90, &acct_now());
    tick_assert_quiet(&place, &tick_noop("account-pressed", "wait:0", "0"));
    assert!(!place.at(TICK_CLIENT).exists(), "偽 client の呼出 0 件");
}

/// (h) 入力欄に人の文字 → `input-busy`・prompt 行の無い pane → `input-unknown`（どちらも 0 key）・自席の前の合図が残る → Enter
/// 1 回の後に `input-own-queued`（送ったのは Enter の 1 key だけ・合図の text は 0 key）。どの周も記録は増えない。
#[test]
fn seat_tick_input_gate_refuses_typed_text_unknown_pane_and_own_queued_signal() {
    let place = tick_place(true);
    tick_silent_for(&place, TICK_STALE + 60);
    fs::write(place.at(TICK_PANE), format!("{TICK_CLEAR_PANE}half typed")).ok();
    tick_assert_quiet(&place, &tick_noop("input-busy", "wait:0", "0"));
    fs::write(place.at(TICK_PANE), "no prompt here\n").ok();
    tick_assert_quiet(&place, &tick_noop("input-unknown", "wait:0", "0"));
    assert!(tick_keys(&place).is_empty(), "0 key");
    fs::write(place.at(TICK_PANE), TICK_CLEAR_PANE).ok();
    assert_eq!(stdout_of(&tick_run(&place, &[])), tick_inject(0), "自席の注入の記録を 1 行作る");
    fs::remove_file(tick_ladder_path(&place)).ok();
    fs::write(place.at(TICK_PANE), format!("{TICK_CLEAR_PANE}{}", tick_signal(0))).ok();
    fs::write(place.at(TICK_STUCK), "").ok();
    let before = tick_keys(&place);
    let out = tick_run(&place, &[]);
    assert_eq!(stdout_of(&out), tick_noop("input-own-queued", "wait:0", "0"), "stderr={}", stderr_of(&out));
    let after = tick_keys(&place);
    assert_eq!(after.get(before.len()..), Some(&[format!("send-keys -t {TICK_TARGET} Enter")][..]), "Enter の 1 key だけ");
    assert_eq!(tick_ladder(&place), None, "梯子の記録は増えない");
    assert_eq!(tick_injections(&place).len(), 1, "注入の記録は増えない");
}

/// (i) 記録が読めない（dir）→ `record-unreadable`（梯子を評価できない＝`pointer=- step=-`）・席 dir が読み取り専用 →
/// `record-unwritable`（0 key・pane 不変）・送れない tmux の周も梯子の記録は残る（送ったと数える・`consumed=unknown:<理由>`）。
#[test]
fn seat_tick_record_faults_send_nothing_and_a_failed_send_keeps_the_record() {
    let place = tick_place(true);
    tick_silent_for(&place, TICK_STALE + 60);
    fs::create_dir_all(tick_ladder_path(&place)).ok();
    tick_assert_quiet(&place, &tick_noop("record-unreadable", "-", "-"));
    fs::remove_dir(tick_ladder_path(&place)).ok();
    fs::set_permissions(place.seat(), fs::Permissions::from_mode(0o555)).ok();
    tick_assert_quiet(&place, &tick_noop("record-unwritable", "wait:0", "0"));
    fs::set_permissions(place.seat(), fs::Permissions::from_mode(0o755)).ok();
    assert_eq!(fs::read_to_string(place.at(TICK_PANE)).unwrap_or_default(), TICK_CLEAR_PANE, "pane 不変");
    assert!(tick_keys(&place).is_empty(), "0 key");
    fs::write(place.at(TICK_REFUSE), "").ok();
    let out = tick_run(&place, &[]);
    assert_eq!(
        stdout_of(&out),
        format!("decision=inject target={TICK_SEAT} reason=- pointer=sent step=0 consumed=unknown:tmux-failed\n"),
        "送れない周も inject と数える"
    );
    assert_eq!(tick_ladder(&place).map(|(_, step, digest)| (step, digest)), Some((0, None)), "梯子の記録は残る");
    assert!(tick_injections(&place).is_empty(), "送達していない注入は tick.jsonl に書かない");
}

/// (j) `--rules` の写しが行 4 本のどれかを欠く周は `decision=error reason=no-rule` rc 1・0 key（全部を持つ写しは判定へ進む）。
/// manifest が壊れている周も `no-rule`（defect は stderr）・event log が読めない周は `store`。
#[test]
fn seat_tick_missing_rule_rows_and_unreadable_store_are_errors() {
    let place = tick_place(true);
    tick_silent_for(&place, 100);
    let rows = [
        ("seat.tick_interval_s", "SeatTickIntervalS", 60),
        ("seat.tick_stale_s", "SeatTickStaleS", TICK_STALE),
        ("seat.pointer_backoff_factor", "SeatPointerBackoffFactor", 2),
        ("seat.pointer_backoff_max_s", "SeatPointerBackoffMaxS", 86_400),
        ("pipe.stop_grace_ms", "StopGraceMs", 2000),
        ("fleet.usage_fresh_s", "UsageFreshS", 300),
        ("fleet.group_pressure_5h_pct", "GroupPressure5hPct", 85),
        ("fleet.group_pressure_7d_pct", "GroupPressure7dPct", 95),
        ("fleet.group_pressure_model_pct", "GroupPressureModelPct", 95),
    ];
    let body = |skip: &str| {
        rows.iter().filter(|(id, _, _)| *id != skip).fold("schema = 1\n".to_owned(), |text, (id, kind, value)| {
            format!("{text}\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n")
        })
    };
    let full = fixture(&place.dir, "full.toml", &body(""));
    let out = tick_run(&place, &["--rules", &full]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_OK), tick_noop("stamp-recent", "wait:0", "0")), "全部を持つ写し");
    let error = |reason: &str| format!("decision=error target={TICK_SEAT} reason={reason} pointer=- step=- consumed=-\n");
    for (id, _, _) in rows.iter().take(4) {
        let rules = fixture(&place.dir, "missing.toml", &body(id));
        let out = tick_run(&place, &["--rules", &rules]);
        assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), error("no-rule")), "{id} を欠く写し");
    }
    let broken = fixture(&place.dir, "broken.toml", "こわれ\n");
    let out = tick_run(&place, &["--rules", &broken]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), error("no-rule")), "壊れた manifest");
    assert!(stderr_of(&out).starts_with("rules: "), "defect を stderr へ: {}", stderr_of(&out));
    let events = vessel::fleet::store::events_path(&place.state);
    fs::remove_file(&events).ok();
    fs::create_dir_all(&events).ok();
    let out = tick_run(&place, &[]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), error("store")), "event log が読めない");
    assert!(tick_keys(&place).is_empty() && tick_ladder(&place).is_none(), "どの周も 0 key・記録 0");
}

// ─────────────────── tick の unit（seat-heartbeat.md §3・契約表の行 b・`s2-07l.583`・接頭辞 `seat_unit_`） ───────────────────
//
// unit dir と binary は tmp・`systemctl` は PATH の偽 script が引数を 1 行ずつ file に残す（host の systemd を 1 度も撃たない）。
// 置き場と登録 row は tick の歯の fixture（[`tick_place`]）を使う。unit の字面は契約から組む（実装の導出を使わない）。

/// 偽 systemctl の呼出の記録。
const UNIT_CALLS: &str = "systemctl-calls";
/// 器の印（2 file の先頭行・契約の字面）。
const UNIT_MARK: &str = "tick-install schema=1";

/// unit の歯の置き場（tick の置き場 + unit dir + binary の path）。
struct UnitPlace {
    /// 置き場・登録 row・偽の PATH。
    tick: TickPlace,
    /// `--unit-dir`。
    units: PathBuf,
    /// `--binary`（在る必要は無い＝unit の字面にだけ載る）。
    binary: String,
}

impl UnitPlace {
    /// service と timer の file 名（契約の字面・`<NAME>-seat-tick-<潰した target>`）。
    fn names(&self) -> [String; 2] {
        [format!("{NAME}-seat-tick-{TICK_SEAT}.service"), format!("{NAME}-seat-tick-{TICK_SEAT}.timer")]
    }

    /// unit dir の下の file。
    fn unit(&self, name: &str) -> PathBuf {
        self.units.join(name)
    }
}

/// 置き場を 1 つ作り、偽 systemctl を偽の bin に足す。
fn unit_place(registered: bool) -> UnitPlace {
    let tick = tick_place(registered);
    let path = tick.dir.join("bin").join("systemctl");
    fs::write(&path, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", tick.at(UNIT_CALLS).display())).ok();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).ok();
    let units = tick.dir.join("units");
    let binary = tick.dir.join("opt").join(NAME).display().to_string();
    UnitPlace { tick, units, binary }
}

/// `seat tick <verb>` を偽の PATH で 1 回撃つ（`--binary` は置き場の値・`extra` は `--rules` など）。
fn unit_run(place: &UnitPlace, verb: &str, extra: &[&str]) -> Output {
    let binary = place.binary.clone();
    let mut args = vec!["--binary", binary.as_str()];
    args.extend_from_slice(extra);
    unit_run_bare(place, verb, &args)
}

/// `seat tick <verb> --state-dir --target --unit-dir` に `rest` だけを足して撃つ（`--binary` を欠く周の形）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn unit_run_bare(place: &UnitPlace, verb: &str, rest: &[&str]) -> Output {
    let (state, units) = (place.tick.state.display().to_string(), place.units.display().to_string());
    Command::new(bin())
        .args(["seat", "tick", verb, "--state-dir", &state, "--target", TICK_TARGET, "--unit-dir", &units])
        .args(rest)
        .env("PATH", &place.tick.path)
        .output()
        .expect("binary を起動できる")
}

/// 偽 systemctl の呼出（引数の行・呼んだ順）。
fn unit_calls(place: &UnitPlace) -> Vec<String> {
    fs::read_to_string(place.tick.at(UNIT_CALLS)).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// unit dir の直下の名（昇順・dir も含む）。
fn unit_listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|found| found.filter_map(Result::ok).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// `tick.jsonl` のうち `who`=`seat-tick-install` の行。
fn unit_installs(place: &UnitPlace) -> Vec<String> {
    tick_injections(&place.tick).into_iter().filter(|line| acct_text(line, "who").as_deref() == Some("seat-tick-install")).collect()
}

/// `seat.tick_interval_s` だけを持つ rules の写し（`n` 秒）。
fn unit_rules(place: &UnitPlace, n: u64) -> String {
    let body = format!(
        "schema = 1\n\n[[rule]]\nid = \"seat.tick_interval_s\"\nkind = \"SeatTickIntervalS\"\nvalue = {n}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n"
    );
    fixture(&place.tick.dir, "unit-rules.toml", &body)
}

/// 導出の 2 file の本文（契約の字面から組む・設計 §3）: `rules` は `--rules` の値・`n` は周期。
fn unit_expected(place: &UnitPlace, rules: Option<&str>, n: u64) -> [String; 2] {
    let state = place.tick.state.display();
    let rules = rules.map(|found| format!(" --rules {found}")).unwrap_or_default();
    [
        format!(
            "# {NAME} {UNIT_MARK}\n[Unit]\nDescription={NAME} seat tick {TICK_TARGET}\n\n[Service]\nType=oneshot\nExecStart={} seat tick --state-dir {state} --target {TICK_TARGET}{rules}\n",
            place.binary
        ),
        format!(
            "# {NAME} {UNIT_MARK}\n[Unit]\nDescription={NAME} seat tick timer {TICK_TARGET}\n\n[Timer]\nOnBootSec={n}s\nOnUnitActiveSec={n}s\nPersistent=false\n\n[Install]\nWantedBy=timers.target\n"
        ),
    ]
}

/// 2 file の今の本文（無い file は空）。
fn unit_bodies(place: &UnitPlace) -> [String; 2] {
    place.names().map(|name| fs::read_to_string(place.unit(&name)).unwrap_or_default())
}

/// 導出の 2 file（install した後の service と timer）の外形を snapshot に固定する（C12.5）。tmp の根は `[tmp]` に置換する。
#[test]
fn seat_unit_external_form() {
    let place = unit_place(true);
    let out = unit_run(&place, "install", &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let [service, timer] = unit_bodies(&place);
    let [service_name, timer_name] = place.names();
    let form = format!("== {service_name}\n{service}== {timer_name}\n{timer}").replace(&place.tick.dir.display().to_string(), "[tmp]");
    fs::remove_dir_all(&place.tick.dir).ok();
    insta::assert_snapshot!(form);
}

/// (a) install は登録 row の席の service と timer を導出の bytes で書き（`Environment` / `WorkingDirectory` / `%h` 無し・周期は rules 行の
/// 値・先頭行は器の印）、偽 systemctl を `daemon-reload` → `enable --now <timer>` の順で 2 回撃ち、`tick.jsonl` に
/// `who`=`seat-tick-install` の 1 行を足して rc 0。
#[test]
fn seat_unit_install_writes_the_derived_pair_and_enables_the_timer() {
    let place = unit_place(true);
    let [service_name, timer_name] = place.names();
    let out = unit_run(&place, "install", &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert!(stdout_of(&out).starts_with(&format!("seat tick install: installed timer={timer_name} ")), "{}", stdout_of(&out));
    assert_eq!(unit_listing(&place.units), [service_name.clone(), timer_name.clone()], "2 file だけ（一時 file を残さない）");
    assert_eq!(unit_bodies(&place), unit_expected(&place, None, 60), "導出の bytes（周期は埋め込みの seat.tick_interval_s = 60）");
    for body in unit_bodies(&place) {
        assert!(body.starts_with(&format!("# {NAME} {UNIT_MARK}\n")), "先頭行は器の印: {body}");
        for banned in ["Environment", "WorkingDirectory", "%h"] {
            assert!(!body.contains(banned), "{banned} を持たない: {body}");
        }
    }
    assert_eq!(unit_calls(&place), ["--user daemon-reload".to_owned(), format!("--user enable --now {timer_name}")], "reload → enable の順");
    let installs = unit_installs(&place);
    assert_eq!(installs.len(), 1, "記録 1 行: {installs:?}");
    assert_eq!(acct_text(installs.first().map(String::as_str).unwrap_or_default(), "what").as_deref(), Some(timer_name.as_str()), "what は unit 名");
    let place = unit_place(true);
    let rules = unit_rules(&place, 90);
    let out = unit_run(&place, "install", &["--rules", &rules]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(unit_bodies(&place), unit_expected(&place, Some(&rules), 90), "`--rules` は ExecStart の末尾に載り、周期は写しの行の値");
    fs::remove_dir_all(&place.tick.dir).ok();
}

/// 断りを測る: `rest`（`--binary` を含む）で撃つと rc 1・stderr に `seat tick <verb>: refused <tail> target=<target>`・偽 systemctl の
/// 呼出は増えず、unit dir の 2 file の本文も動かない。
fn unit_assert_refused(place: &UnitPlace, verb: &str, rest: &[&str], tail: &str) {
    let (calls, bodies) = (unit_calls(place).len(), unit_bodies(place));
    let out = unit_run_bare(place, verb, rest);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{tail}: rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains(&format!("seat tick {verb}: refused {tail} target={TICK_TARGET}")), "{tail}: {}", stderr_of(&out));
    assert_eq!(unit_calls(place).len(), calls, "{tail}: systemctl 0 回");
    assert_eq!(unit_bodies(place), bodies, "{tail}: file は動かない");
}

/// (b) 同じ bytes の再 install は `unchanged`（file の mtime 不変・`enable --now` だけ 1 回）・1 byte 違う既存 file は `unit-exists` で
/// file 不変・systemctl 0 回・rc 1。
#[test]
fn seat_unit_install_is_unchanged_on_same_bytes_and_refuses_a_different_file() {
    let place = unit_place(true);
    let [service_name, timer_name] = place.names();
    assert_eq!(rc_of(&unit_run(&place, "install", &[])), i32::from(RC_OK));
    let mtimes = || place.names().map(|name| fs::metadata(place.unit(&name)).and_then(|meta| meta.modified()).ok());
    let before = mtimes();
    let out = unit_run(&place, "install", &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert!(stdout_of(&out).starts_with(&format!("seat tick install: unchanged timer={timer_name} ")), "{}", stdout_of(&out));
    assert_eq!(mtimes(), before, "file は書き直さない");
    assert_eq!(unit_calls(&place).get(2..), Some(&[format!("--user enable --now {timer_name}")][..]), "enable だけ 1 回");
    let changed = unit_expected(&place, None, 60)[0].replacen("oneshot", "oneshoT", 1);
    fs::write(place.unit(&service_name), &changed).ok();
    unit_assert_refused(&place, "install", &["--binary", &place.binary], &format!("reason=unit-exists unit={service_name}"));
    assert_eq!(fs::read_to_string(place.unit(&service_name)).unwrap_or_default(), changed, "1 byte 違いの file は不変");
    fs::remove_dir_all(&place.tick.dir).ok();
}

/// (b) 登録 row 無しは `no-row`・`seat.tick_interval_s` を欠く `--rules` は `no-rule`（どちらも file 0・systemctl 0 回・rc 1）。
#[test]
fn seat_unit_install_refuses_no_row_and_no_rule_without_a_file() {
    let bare = unit_place(false);
    unit_assert_refused(&bare, "install", &["--binary", &bare.binary], "reason=no-row");
    assert!(unit_listing(&bare.units).is_empty(), "file 0");
    fs::remove_dir_all(&bare.tick.dir).ok();
    let place = unit_place(true);
    let rules = fixture(&place.tick.dir, "no-interval.toml", "schema = 1\n");
    unit_assert_refused(&place, "install", &["--binary", &place.binary, "--rules", &rules], "reason=no-rule");
    assert!(unit_listing(&place.units).is_empty(), "file 0");
    fs::remove_dir_all(&place.tick.dir).ok();
}

/// (c) uninstall は install と同じ `--binary` / `--rules` で導出し直し、`disable --now` の後に 2 file を `.retired/<name>.<ts>` へ同じ
/// bytes で移す（元の場所に無い）。
#[test]
fn seat_unit_uninstall_retires_the_pair_with_the_same_bytes() {
    let place = unit_place(true);
    let rules = unit_rules(&place, 90);
    assert_eq!(rc_of(&unit_run(&place, "install", &["--rules", &rules])), i32::from(RC_OK));
    let installed = unit_bodies(&place);
    let calls = unit_calls(&place).len();
    let before = unix_now();
    let out = unit_run(&place, "uninstall", &["--rules", &rules]);
    let after = unix_now();
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let timer_name = &place.names()[1];
    assert_eq!(unit_calls(&place).get(calls..), Some(&[format!("--user disable --now {timer_name}")][..]), "disable --now 1 回");
    assert_eq!(unit_listing(&place.units), [".retired"], "元の場所に無い");
    let retired = place.units.join(".retired");
    let moved = unit_listing(&retired);
    assert_eq!(moved.len(), 2, "{moved:?}");
    for (name, body) in place.names().iter().zip(&installed) {
        let stamp = |entry: &&String| entry.strip_prefix(&format!("{name}.")).and_then(|ts| ts.parse::<u64>().ok());
        let found = moved.iter().find(|entry| stamp(entry).is_some_and(|ts| (before..=after).contains(&ts)));
        let found = found.map(|entry| fs::read_to_string(retired.join(entry)).unwrap_or_default());
        assert_eq!(found.as_ref(), Some(body), "{name} は .retired/<name>.<ts> に同じ bytes");
    }
    fs::remove_dir_all(&place.tick.dir).ok();
}

/// (c) uninstall の断り: `--binary` を欠く周は使い方の誤り・別の `--binary` は導出し直した bytes が違う＝`unit-exists`・印の無い file は
/// `unit-foreign`・印は在るが 1 byte 違う file は `unit-exists`（どれも動かさず systemctl 0 回）。
#[test]
fn seat_unit_uninstall_refuses_foreign_or_changed_files_without_moving_them() {
    let place = unit_place(true);
    let [service_name, timer_name] = place.names();
    let rules = unit_rules(&place, 90);
    assert_eq!(rc_of(&unit_run(&place, "install", &["--rules", &rules])), i32::from(RC_OK));
    let installed = unit_bodies(&place);
    for rest in [&["--rules", rules.as_str()][..], &[]] {
        let out = unit_run_bare(&place, "uninstall", rest);
        assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), String::new()), "`--binary` を欠く周は使い方の誤り");
        assert!(stderr_of(&out).starts_with("usage: seat "), "{}", stderr_of(&out));
    }
    unit_assert_refused(&place, "uninstall", &["--binary", "/elsewhere/bin", "--rules", &rules], &format!("reason=unit-exists unit={service_name}"));
    assert_eq!(unit_bodies(&place), installed, "断った周は動かない");
    let changed = format!("{}\n", installed[1]);
    fs::write(place.unit(&timer_name), &changed).ok();
    unit_assert_refused(&place, "uninstall", &["--binary", &place.binary, "--rules", &rules], &format!("reason=unit-exists unit={timer_name}"));
    fs::write(place.unit(&service_name), "[Service]\nExecStart=/bin/true\n").ok();
    unit_assert_refused(&place, "uninstall", &["--binary", &place.binary, "--rules", &rules], &format!("reason=unit-foreign unit={service_name}"));
    assert_eq!(unit_listing(&place.units), [service_name, timer_name], "退役 dir を作らない");
    assert!(!unit_calls(&place).iter().any(|call| call.contains("disable")), "disable を撃たない");
    fs::remove_dir_all(&place.tick.dir).ok();
}

/// doctor を unit の置き場の `--state-dir` と `extra` で撃つ（tmux は偽の PATH・socket は無い path）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn unit_doctor(place: &UnitPlace, extra: &[&str]) -> Output {
    let state = place.tick.state.display().to_string();
    let socket = place.tick.at("no-such-sock").display().to_string();
    Command::new(bin())
        .args(["doctor", "--state-dir", &state, "--tmux-socket", &socket])
        .args(extra)
        .env("PATH", &place.tick.path)
        .output()
        .expect("binary を起動できる")
}

/// doctor の登録 row の行（`seat: ` 始まり）。
fn unit_doctor_rows(out: &Output) -> Vec<String> {
    stdout_of(out).lines().filter(|line| line.starts_with("seat: ")).map(str::to_owned).collect()
}

/// (d) doctor `--unit-dir U --binary PATH` は登録 row の行ごとに `tick-unit=present|absent|foreign` を足し（install 後 present・撤去後
/// absent・印の無い file は foreign）、`--unit-dir` だけ・`--binary` だけは使い方の誤り、`--unit-dir` 無しは項目を足さない。
#[test]
fn seat_unit_doctor_names_present_absent_and_foreign_per_row() {
    let place = unit_place(true);
    let units = place.units.display().to_string();
    let binary = place.binary.clone();
    let probe = ["--unit-dir", units.as_str(), "--binary", binary.as_str()];
    let word = |want: &str| {
        let out = unit_doctor(&place, &probe);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
        let rows = unit_doctor_rows(&out);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows.iter().all(|row| row.ends_with(&format!(" tick-unit={want}"))), "{want}: {rows:?}");
    };
    word("absent");
    assert_eq!(rc_of(&unit_run(&place, "install", &[])), i32::from(RC_OK));
    word("present");
    let bare = unit_doctor(&place, &[]);
    assert_eq!(rc_of(&bare), i32::from(RC_OK));
    assert!(!stdout_of(&bare).contains("tick-unit="), "`--unit-dir` 無しは項目を足さない: {}", stdout_of(&bare));
    let [service_name, timer_name] = place.names();
    fs::write(place.unit(&timer_name), "[Timer]\nOnUnitActiveSec=1s\n").ok();
    word("foreign");
    fs::remove_file(place.unit(&timer_name)).ok();
    word("foreign");
    fs::remove_file(place.unit(&service_name)).ok();
    word("absent");
    for half in [&probe[..2], &probe[2..]] {
        let out = unit_doctor(&place, half);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{half:?}: 片方だけは使い方の誤り");
        assert!(unit_doctor_rows(&out).is_empty(), "{half:?}: {}", stdout_of(&out));
    }
    fs::remove_dir_all(&place.tick.dir).ok();
}

/// (e) 使い方の 1 行に `tick install …` / `tick uninstall …` が在り、`seat tick install` の flag の閉包は tmux の flag を受けない。
#[test]
fn seat_unit_usage_names_install_and_uninstall() {
    let usage = stderr_of(&run_seat(&[]));
    for verb in ["install", "uninstall"] {
        let mouth = format!("|tick {verb} --state-dir S --target S:W --unit-dir U --binary PATH [--rules F]|");
        assert!(usage.contains(&mouth), "{verb} は使い方に在る: {usage}");
    }
    let place = unit_place(true);
    let out = unit_run(&place, "install", &["--tmux-socket", "x"]);
    assert_eq!(rc_of(&out), 2, "unit の口は tmux の flag を受けない: {}", stderr_of(&out));
    assert!(unit_listing(&place.units).is_empty() && unit_calls(&place).is_empty(), "file 0・systemctl 0 回");
    fs::remove_dir_all(&place.tick.dir).ok();
}
