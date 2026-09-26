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
    assert_eq!(
        stdout_of(&out),
        "decision=noop target=s_w reason=no-row pointer=- step=- consumed=- move=- launched=- judged=-\n",
        "判定行 1 行"
    );
    assert!(stderr_of(&out).is_empty(), "stderr 0 byte");
}

/// 席の自律機能の口のうち、作り直しの cycle・打刻の heartbeat・context の計測 meter は**もう無い**（ADR-0045 §2 (2)・
/// `s2-07l.479.1`）: 口を持たない第 1 token として断られ（[`assert_gone_mouth`]）、使い方の 1 行にもその名が出ない。
/// 管理 tick は backoff つきで**戻った**（ADR-0058・`s2-07l.582`）＝在る側（[`assert_tick_mouth`]）。`heartbeat` の語は打刻の口
/// としては戻らず、席ごとの合図の停止の記録の口（`off` / `on` / `status` の 3 語・ADR-0070・`s2-07l.646`）として使い方に在る。
/// 残る口（register / launch）は従来どおり使い方に在る。
///
/// **消えたことと戻ったことを測る歯**である（base では tick が消えた口として断られるので RED）。
#[test]
fn seat_autonomy_subcommands_are_gone_from_the_usage() {
    let (dir, state) = gone_mouth_place();
    let usage = stderr_of(&run_seat(&[]));
    for gone in ["meter", "cycle"] {
        assert_gone_mouth(&state, gone, &[]);
        assert!(!usage.contains(&format!("|{gone} ")), "{gone} は使い方に出ない: {usage}");
    }
    for switch in ["off", "on", "status"] {
        assert!(usage.contains(&format!("|heartbeat {switch} --state-dir S --target S:W|")), "heartbeat {switch} は停止の記録の口: {usage}");
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
    let status = "tick status --state-dir S [--target S:W] [--rules F]".to_owned();
    assert_eq!(takers, [TICK_USAGE.to_owned(), unit("install"), unit("uninstall"), status], "席の口で `--rules` を受けるのは tick（と unit の口 2 つ・`s2-07l.583`・status の口・`s2-07l.650`）だけ: {usage}");
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
    assert_eq!(vessel::seat::cli::SEAT_COMMANDS.len(), 6, "記録時点の既知の verb（`.582` で +1〔管理 tick〕・`.620` で +1〔登録 row の退役〕・`.646` で +1〔合図の停止の記録〕）: {SEAT_COMMANDS:?}");
    assert!(vessel::order::is_declaration_order(SEAT_COMMANDS, |command| command as usize), "宣言順: {SEAT_COMMANDS:?}");
    let words: Vec<&str> = SEAT_COMMANDS.iter().map(|command| command.as_str()).collect();
    assert_eq!(words, ["register", "launch", "ruling", "tick", "retire", "heartbeat"], "字面の閉じた列（宣言順）");
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
    assert_eq!(lines.len(), 9, "2 行 + host-template 1 行 + init 1 行 + 登録 row 1 行 + 突合 1 行 + host の面 1 行 + 導入先 1 行 + host-guard 1 行: {lines:?}");
    assert_eq!(
        lines.get(5..8),
        Some(&["seats: registered=1 live=1 missing=0".to_owned(), HOST_ABSENT.to_owned(), CONSUMER_REPO.to_owned()][..]),
        "席の立つ socket"
    );
    assert!(lines.last().is_some_and(|line| line.starts_with(HOST_GUARD_HEAD)), "末尾は host-guard の 1 行: {lines:?}");
    let elsewhere = place.dir.join("no-server-sock").display().to_string();
    let away = doctor(&["--state-dir", &state, "--tmux-socket", &elsewhere, "--rules", &rules]);
    assert_eq!(away.as_ref().map(rc_of), Some(i32::from(RC_OK)), "{away:?}");
    let away_lines: Vec<String> = away.map(|out| stdout_of(&out)).unwrap_or_default().lines().map(str::to_owned).collect();
    assert_eq!(
        away_lines.get(5..8),
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
/// **前**に来る＝`cd … && CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY=1 <tail>`（env の 2 語は
/// この順・seat-heartbeat.md §11 行 n）。`tail` は前置の後の字面の先頭（雛形か `CLAUDE_CONFIG_DIR=`）。
fn acct_launch_prefix(anchor: &str, tail: &str) -> String {
    format!("cd '{anchor}' && CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY=1 {tail}")
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

/// 期待する偽 claude の記録（argv を 1 語 1 行・続けて env の 3 行）。
fn launch_expected_argv(place: &AcctPlace, label: &str) -> String {
    let account_dir = place.state.join("accounts").join(label).display().to_string();
    format!(
        "--plugin-dir\n{}\n--plugin-dir\n{}\n--plugin-dir\n{}\n{}\n{}\nenv:CLAUDE_CONFIG_DIR={account_dir}\nenv:CLAUDE_CODE_DISABLE_AGENT_VIEW=1\n\
         env:CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY=1\n",
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
         printf 'env:CLAUDE_CONFIG_DIR=%s\\nenv:CLAUDE_CODE_DISABLE_AGENT_VIEW=%s\\nenv:CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY=%s\\n' \
         \"$CLAUDE_CONFIG_DIR\" \"$CLAUDE_CODE_DISABLE_AGENT_VIEW\" \"$CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY\" >> '{launched}'\n\
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
const TICK_STALE: u64 = 1800;
/// `seat.pointer_ladder_s` の初期値（秒の列・設計 seat-heartbeat.md §10 形 5・値は rules の歯が埋め込み manifest で pin する）。
const TICK_LADDER: [u64; 6] = [1800, 3600, 10_800, 21_600, 43_200, 86_400];
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

/// 偽 tmux と偽 client を `<dir>/bin` に置き、PATH の字面を返す。偽 tmux は呼出を 1 行残し、`list-panes` は前面の語 `claude`
/// （席が立っている窓・pane は触らない）を、`capture-pane` は pane の file を返し（file が無ければ rc 1）、`send-keys … -l <text>`
/// は text を入力欄へ足し、`send-keys … Enter` は入力欄を送って新しい prompt を描く（[`TICK_STUCK`] が在れば何もしない・
/// [`TICK_REFUSE`] が在れば `send-keys` は rc 1）。
fn tick_shims(dir: &Path) -> String {
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).ok();
    let at = |name: &str| dir.join(name).display().to_string();
    let tmux = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{calls}'\n[ \"$1\" = list-panes ] && echo claude && exit 0\n[ -f '{pane}' ] || exit 1\n\
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

/// 段 n の待ち（秒・初期値の梯子の列の n 番目・列を越えた段は `None`＝打ち切り）。
fn tick_wait(step: u32) -> Option<u64> {
    usize::try_from(step).ok().and_then(|at| TICK_LADDER.get(at)).copied()
}

/// 段 `step` の合図の文面（契約の字面・設計 §2 形 3・§10 形 7＝次の待ちは列から引き、最後の段は次が無い）。
fn tick_signal(step: u32) -> String {
    let next = tick_wait(step.saturating_add(1))
        .map_or_else(|| "次の合図は無い・打ち切り".to_owned(), |secs| format!("次の合図は {secs} 秒後"));
    format!("{NAME} tick: heartbeat step={step} — 台帳の現在地（bd --readonly ready --limit 0）から続きを進める（変化が無ければ{next}）")
}

/// 管理 tick の行 3 本（id・kind・TOML の値の字面・設計 §10 形 5 / 6）。
fn tick_rule_rows() -> Vec<(&'static str, &'static str, String)> {
    let ladder: Vec<String> = TICK_LADDER.iter().map(|secs| format!("\"{secs}\"")).collect();
    vec![
        ("seat.tick_interval_s", "SeatTickIntervalS", "15".to_owned()),
        ("seat.tick_stale_s", "SeatTickStaleS", TICK_STALE.to_string()),
        ("seat.pointer_ladder_s", "SeatPointerLadderS", format!("[{}]", ladder.join(", "))),
    ]
}

/// rules の写しの本文（`schema = 1` の後に `[[rule]]` を行の順に並べる）。
fn tick_rules_body(rows: &[(&str, &str, String)]) -> String {
    rows.iter().fold("schema = 1\n".to_owned(), |text, (id, kind, value)| {
        format!("{text}\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n")
    })
}

/// tick の判定が読む行の全部（行 3 本・送達の窓・群の閾値）の写し。`skip` の id の行を落とし、`ladder` が在れば梯子の列の
/// 値をその字面に差し替える。
fn tick_rules_text(skip: &str, ladder: Option<&str>) -> String {
    let mut rows = tick_rule_rows();
    rows.extend(
        [
            ("pipe.stop_grace_ms", "StopGraceMs", 2000),
            ("fleet.usage_fresh_s", "UsageFreshS", 300),
            ("fleet.group_pressure_5h_pct", "GroupPressure5hPct", 85),
            ("fleet.group_pressure_7d_pct", "GroupPressure7dPct", 95),
            ("fleet.group_pressure_model_pct", "GroupPressureModelPct", 95),
        ]
        .map(|(id, kind, value)| (id, kind, value.to_string())),
    );
    rows.retain(|(id, _, _)| *id != skip);
    for row in rows.iter_mut().filter(|(id, _, _)| *id == "seat.pointer_ladder_s") {
        row.2 = ladder.map_or_else(|| row.2.clone(), str::to_owned);
    }
    tick_rules_body(&rows)
}

/// 合図の text を送った key（`-l` の呼出の行）。
fn tick_text_key(step: u32) -> String {
    format!("send-keys -t {TICK_TARGET} -l {}", tick_signal(step))
}

/// noop の判定行。
fn tick_noop(reason: &str, pointer: &str, step: &str) -> String {
    format!("decision=noop target={TICK_SEAT} reason={reason} pointer={pointer} step={step} consumed=-{TICK_NO_MOVE}\n")
}

/// inject の判定行（偽 tmux は消費の打刻を打たない＝queue の `consumed=false`）。
fn tick_inject(step: u32) -> String {
    format!("decision=inject target={TICK_SEAT} reason=- pointer=sent step={step} consumed=false{TICK_NO_MOVE}\n")
}

/// 移動の周でなく群の判定を撃たない判定行の末尾 3 欄（設計 seat-heartbeat.md §4 形 8 / §9 形 4・列は固定で省かない）。
const TICK_NO_MOVE: &str = " move=- launched=- judged=-";

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

/// 段 `step` を列の待ち `wait` で登る（`--rules` は `rules`）: 前の段の `sent_at` が待ちの 5 秒手前の周は床の内（`wait`）で
/// 送らず、待ちを 5 秒過ぎた周に段 `step` の合図を送り、記録は段 `step`・基準 null になる。
fn tick_climb(place: &TickPlace, rules: &str, step: u32, wait: u64) {
    let run = || stdout_of(&tick_run(place, &["--rules", rules]));
    let early = unix_now() - wait + 5;
    tick_ladder_put(place, early, step - 1, None);
    tick_stamps(place, &[("busy", "UserPromptSubmit", early + 1), ("idle", "Stop", early + 2)]);
    let line = run();
    assert!(line.contains(" reason=wait pointer=wait:") && line.contains(&format!(" step={step} ")), "段 {step} の床の内: {line}");
    let sent = unix_now() - wait - 5;
    tick_ladder_put(place, sent, step - 1, None);
    tick_stamps(place, &[("busy", "UserPromptSubmit", sent + 1), ("idle", "Stop", sent + 2)]);
    assert_eq!(run(), tick_inject(step), "段 {step}（待ち {wait} 秒）");
    assert_eq!(tick_ladder(place).map(|(_, found, digest)| (found, digest)), Some((u64::from(step), None)), "段 {step} の記録");
}

/// (d) 梯子（設計 §10 形 5・`--rules` は梯子の列の写し）: 送った直後は `settling`・`sent_at` より後の Stop を足すと基準が入り
/// 無変化の床は `wait:<s> step=1`・`sent_at` を列の待ちの分だけ過去に書くと段 1〜5 の合図が出て、7 段目（段 6）は `stopped` で
/// 送らない（合図は列の長さの 6 本で打ち切り）。段 n の待ちは列の n 番目で、その 5 秒手前の周は床の内（`wait`）。
#[test]
fn seat_tick_ladder_climbs_six_signals_then_stops() {
    let place = tick_place(true);
    let rules = fixture(&place.dir, "ladder.toml", &tick_rules_text("", None));
    let run = || tick_run(&place, &["--rules", &rules]);
    tick_silent_for(&place, TICK_STALE + 60);
    assert_eq!(stdout_of(&run()), tick_inject(0), "段 0");
    assert_eq!(stdout_of(&run()), tick_noop("settling", "settling", "0"), "送った直後");
    let sent = unix_now() - 3000;
    tick_ladder_put(&place, sent, 0, None);
    tick_stamps(&place, &[("busy", "UserPromptSubmit", sent + 1), ("idle", "Stop", sent + 2)]);
    let line = stdout_of(&run());
    assert!(line.starts_with(&format!("decision=noop target={TICK_SEAT} reason=wait pointer=wait:")), "{line}");
    assert!(line.ends_with(&format!(" step=1 consumed=-{TICK_NO_MOVE}\n")), "{line}");
    let left = tick_token(&line, "pointer").and_then(|found| found.strip_prefix("wait:").and_then(|secs| secs.parse::<u64>().ok()));
    assert!(left.is_some_and(|secs| (595..=600).contains(&secs)), "残り秒 = 列の 2 番目 3600 − 3000: {line}");
    assert_eq!(tick_ladder(&place), Some((sent, 0, Some(sent + 2))), "基準は sent_at より後の Stop の ts");
    for step in 1..=5 {
        tick_climb(&place, &rules, step, tick_wait(step).expect("段 1〜5 は列の内"));
    }
    let sent = unix_now() - 200_000;
    tick_ladder_put(&place, sent, 5, None);
    tick_stamps(&place, &[("busy", "UserPromptSubmit", sent + 1), ("idle", "Stop", sent + 2)]);
    let keys = tick_keys(&place).len();
    assert_eq!(stdout_of(&run()), tick_noop("stopped", "stopped", "6"), "7 段目（段 6）は打ち切り");
    assert_eq!(tick_keys(&place).len(), keys, "段 6 は 0 key");
    assert_eq!(tick_ladder(&place), Some((sent, 5, Some(sent + 2))), "記録は段 5 のまま（基準だけが入る）");
    let texts: Vec<String> = tick_keys(&place).into_iter().filter(|key| key.contains(" -l ")).collect();
    assert_eq!(texts, (0..=5).map(tick_text_key).collect::<Vec<_>>(), "合図は段 0〜5 の 6 本（列の長さ・段 5 は次が無い）");
    assert_eq!(tick_injections(&place).len(), 6, "注入の記録 6 行");
}

/// (e) 基準の後に最終行の ts が動くと段 0 に戻り、列の先頭の 1800 秒（30 分）黙った周に段 0 で送る（打ち切りの後も同じ）。
/// 1800 秒の手前の周は `stamp-recent` で送らない（埋め込み manifest の `seat.tick_stale_s` = 1800・列の先頭 = 1800）。
#[test]
fn seat_tick_change_returns_to_step_zero_after_thirty_silent_minutes() {
    let place = tick_place(true);
    let now = unix_now();
    let base = now - 20_000;
    tick_ladder_put(&place, base - 10, 3, Some(base));
    tick_stamps(&place, &[("idle", "Stop", base), ("busy", "UserPromptSubmit", now - 120), ("idle", "Stop", now - 100)]);
    tick_assert_quiet(&place, &tick_noop("stamp-recent", "wait:0", "0"));
    tick_stamps(&place, &[("idle", "Stop", base), ("idle", "Stop", now - 1800 + 30)]);
    tick_assert_quiet(&place, &tick_noop("stamp-recent", "wait:0", "0"));
    tick_stamps(&place, &[("idle", "Stop", base), ("idle", "Stop", now - 1800 - 60)]);
    assert_eq!(stdout_of(&tick_run(&place, &[])), tick_inject(0), "変化の後 1800 秒黙った周は段 0");
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

/// (f) 応えない席: `sent_at` から `seat.tick_stale_s`（30 分）の手前は `settling`・過ぎても Stop が無い周はその周の digest で基準が入り、段の候補は段 + 1。
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
    assert!(line.ends_with(&format!(" step=1 consumed=-{TICK_NO_MOVE}\n")), "段 + 1: {line}");
    let left = tick_token(&line, "pointer").and_then(|found| found.strip_prefix("wait:").and_then(|secs| secs.parse::<u64>().ok()));
    assert!(left.is_some_and(|secs| (1095..=1100).contains(&secs)), "残り秒 = 列の 2 番目 3600 − 2500 秒: {line}");
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
        format!("decision=inject target={TICK_SEAT} reason=- pointer=sent step=0 consumed=unknown:tmux-failed{TICK_NO_MOVE}\n"),
        "送れない周も inject と数える"
    );
    assert_eq!(tick_ladder(&place).map(|(_, step, digest)| (step, digest)), Some((0, None)), "梯子の記録は残る");
    assert!(tick_injections(&place).is_empty(), "送達していない注入は tick.jsonl に書かない");
}

/// (j) `--rules` の写しが行 3 本のどれかを欠く周・梯子の列の要素が数でない周・列が狭義に昇順でない周は tick の読みが
/// `decision=error reason=no-rule` rc 1・0 key（stderr は空・全部を持つ写しは判定へ進む）。空の列 `[]` の写しは面の読みが
/// 断り、同じ `no-rule` rc 1・0 key で stderr が `rules: ` で始まり「配列が空である」を含む（経路の違いを stderr で弁別する）。
/// manifest が壊れている周も `no-rule`（defect は stderr）・event log が読めない周は `store`。
#[test]
fn seat_tick_missing_rule_rows_and_unreadable_store_are_errors() {
    let place = tick_place(true);
    tick_silent_for(&place, 100);
    let full = fixture(&place.dir, "full.toml", &tick_rules_text("", None));
    let out = tick_run(&place, &["--rules", &full]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_OK), tick_noop("stamp-recent", "wait:0", "0")), "全部を持つ写し");
    let error =
        |reason: &str| format!("decision=error target={TICK_SEAT} reason={reason} pointer=- step=- consumed=-{TICK_NO_MOVE}\n");
    let refused_by_tick = |label: &str, body: &str| {
        let rules = fixture(&place.dir, "refused.toml", body);
        let out = tick_run(&place, &["--rules", &rules]);
        assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), error("no-rule")), "{label}");
        assert_eq!(stderr_of(&out), "", "{label}: tick の読みの断り（面は通る）");
    };
    for (id, _, _) in tick_rule_rows() {
        refused_by_tick(&format!("{id} を欠く写し"), &tick_rules_text(id, None));
    }
    for ladder in [r#"["1800", "x"]"#, r#"["1800", "-3600"]"#, r#"["3600", "1800"]"#, r#"["1800", "1800"]"#] {
        refused_by_tick(&format!("列 {ladder}"), &tick_rules_text("", Some(ladder)));
    }
    let empty = fixture(&place.dir, "empty.toml", &tick_rules_text("", Some("[]")));
    let out = tick_run(&place, &["--rules", &empty]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), error("no-rule")), "空の列");
    let stderr = stderr_of(&out);
    assert!(stderr.starts_with("rules: ") && stderr.contains("配列が空である"), "空の列は面の読みが断る: {stderr}");
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

// ───────────── tick の移動の周（seat-heartbeat.md §4・契約表の行 c・`s2-07l.617`・接頭辞 `seat_tick_move_`） ─────────────
//
// §2 の fixture（PATH の偽 tmux・手書きの `state.jsonl`・`--rules` の写し）に、偽 tmux の `list-panes` の前面（file で作り分ける）
// と host の群用 dir の記録（置き場の親の下）を足す。置き場は親を共有できる形（`<root>/<name>`）で作り、群は host の面（`host.toml`）
// に口座 2 つ（A・B・種は A）と群 1 つで宣言する。settle は写しの `seat.cycle_settle_s` で 1 秒に縮める。字面は契約から組む。

/// 登録 row の口座（群の種）。
const MOVE_A: &str = "acct-a";
/// 群の移り先の口座。
const MOVE_B: &str = "acct-b";
/// 群の置き場（1 つ目の置き場の席の anchor）。
const MOVE_ANCHOR: &str = "/repo";
/// 群の置き場（2 つ目の置き場の席の anchor）。
const MOVE_ANCHOR_TWO: &str = "/repo-two";
// 群の名を Tier と数字に改めただけの歯（account-lifecycle.md §29 の行 s・base でも緑）。
// flip-check: retroactive s2-07l.647

/// 群の名。
const MOVE_GROUP: &str = "Tier1";
/// 役割の model の行の id（写しの値 fable の表示名 Fable は偽 client の本文のモデル別窓の名・account-lifecycle.md §29 形 1）。
const MOVE_ROLE_ROW: &str = "seat.model.orchestrator";
/// 偽 tmux の前面の file（無ければ席＝`claude`）。
const MOVE_FRONT: &str = "front";
/// `/exit` の確認 dialog の既定の行（設計 account-lifecycle.md §22 形 2 の literal）。
const MOVE_DIALOG_ROW: &str = "1. Exit and stop tasks";

/// 移動の歯の置き場 1 つ（`<root>/<name>` の置き場・`<root>/<name>-tools` の偽 tmux と pane と規則の写し）。
struct MovePlace {
    /// `--state-dir`。
    state: PathBuf,
    /// 偽 tmux・pane・呼出の記録・規則の写しの dir。
    tools: PathBuf,
    /// 偽の bin を先頭に置いた PATH。
    path: String,
    /// `--rules` の写し。
    rules: String,
}

impl MovePlace {
    /// tools の下の file。
    fn at(&self, name: &str) -> PathBuf {
        self.tools.join(name)
    }
}

/// host の根の群用 dir（`<置き場の親>/<NAME>-host/groups`・器の字面を借りない）。
fn move_groups_dir(root: &Path) -> PathBuf {
    root.join(format!("{NAME}-host")).join("groups")
}

/// 群の今の口座の記録を書く（契約の 4 行の形・`<群用 dir>/<群の名>.account`）。
fn move_record(root: &Path, account: &str) {
    let dir = move_groups_dir(root);
    fs::create_dir_all(&dir).ok();
    let body = format!("account={account}\nts=2026-09-25T00:00:00Z\nreason=move\nprevious={MOVE_A}\n");
    fs::write(dir.join(format!("{MOVE_GROUP}.account")), body).ok();
}

/// 置き場を 1 つ作る: host の面に口座 A / B と群（置き場 2 つ・候補 A, B）を書き、`anchor` の席の登録 row（口座 A）を積み、
/// 最終行 Idle の打刻（いま）と空の入力欄の pane を置く。前面の file は置かない（席＝`claude`）。群の判定の打刻（いま）も置く
/// （判定は鮮度の内側＝移動の門の歯は計測を起こさない・判定の歯は [`judge_place`] が消す）。
fn move_place(root: &Path, name: &str, anchor: &str) -> MovePlace {
    let state = root.join(name);
    let tools = root.join(format!("{name}-tools"));
    let seat = seat_dir_of(&state, TICK_SEAT);
    fs::create_dir_all(&seat).ok();
    fs::create_dir_all(&tools).ok();
    fs::create_dir_all(move_groups_dir(root)).ok();
    fs::write(judge_stamp(root), format!("{}\n", unix_now())).ok();
    let host = format!(
        "schema = 1\n\n[[account]]\nlabel = \"{MOVE_A}\"\n\n[[account]]\nlabel = \"{MOVE_B}\"\n\n[[account-group]]\nname = \"{MOVE_GROUP}\"\n\
         anchors = [\"{MOVE_ANCHOR}\", \"{MOVE_ANCHOR_TWO}\"]\naccounts = [\"{MOVE_A}\", \"{MOVE_B}\"]\n"
    );
    fs::write(state.join("host.toml"), host).ok();
    fs::write(state_file(&seat), format!("{}\n", stamp_line("idle", "SessionStart", unix_now(), "sid-move"))).ok();
    let launch = fixture(&tools, "launch.txt", "claude\n");
    let path = state.display().to_string();
    let out = run_seat(&[
        "register", "--state-dir", &path, "--target", TICK_TARGET, "--role", "orchestrator", "--account", MOVE_A, "--launch",
        &launch, "--anchor", anchor,
    ]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "登録 row を積める: {}", stderr_of(&out));
    fs::write(tools.join(TICK_PANE), TICK_CLEAR_PANE).ok();
    let mut rows = tick_rule_rows();
    let others = [
        ("pipe.stop_grace_ms", "StopGraceMs", 300),
        ("fleet.usage_fresh_s", "UsageFreshS", 300),
        ("fleet.group_pressure_5h_pct", "GroupPressure5hPct", 85),
        ("fleet.group_pressure_7d_pct", "GroupPressure7dPct", 95),
        ("fleet.group_pressure_model_pct", "GroupPressureModelPct", 95),
        ("seat.cycle_settle_s", "SeatCycleSettleS", 1),
        ("seat.cycle_poll_ms", "SeatCyclePollMs", 100),
        ("fleet.lock_retry_ms", "LockRetryMs", 5000),
        ("fleet.lock_stale_ms", "LockStaleMs", 30_000),
    ];
    rows.extend(others.map(|(id, kind, value)| (id, kind, value.to_string())));
    rows.push((MOVE_ROLE_ROW, "RoleModel", "\"fable\"".to_owned()));
    let rules = fixture(&tools, "rules.toml", &tick_rules_body(&rows));
    let path = move_shims(&tools);
    MovePlace { state, tools, path, rules }
}

/// 移動の歯の偽 tmux を `<tools>/bin` に置き、PATH の字面を返す。呼出を 1 行残し、`list-panes` は前面の file（無ければ
/// `claude`）・`display-message` は pid の file（無ければ空）・`list-windows` は窓 `tk`・`has-session` は在る・`capture-pane` は pane の file を返し、`send-keys … -l <text>` は
/// text を pane へ足し、`send-keys … Enter` は新しい prompt を描く。偽 client（`curl` / `claude`）も置く。
fn move_shims(tools: &Path) -> String {
    let bin = tools.join("bin");
    fs::create_dir_all(&bin).ok();
    let at = |name: &str| tools.join(name).display().to_string();
    let tmux = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{calls}'\n\
         case \"$1\" in\n\
         list-panes) cat '{front}' 2>/dev/null || echo claude; exit 0;;\n\
         display-message) cat '{pid}' 2>/dev/null; exit 0;;\n\
         list-windows) echo tk; exit 0;;\n\
         has-session) exit 0;;\n\
         capture-pane) cat '{pane}'; exit 0;;\n\
         send-keys) ;;\n\
         *) exit 0;;\n\
         esac\n\
         for last in \"$@\"; do :; done\n\
         case \" $* \" in *' -l '*) printf '%s' \"$last\" >> '{pane}'; exit 0;; esac\n\
         printf '\\n\u{276f} ' >> '{pane}'\n",
        calls = at(TICK_CALLS),
        front = at(MOVE_FRONT),
        pid = at(PANE_SHELL_PID),
        pane = at(TICK_PANE),
    );
    let client = format!("#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> '{}'\n", at(TICK_CLIENT));
    for (name, body) in [("tmux", tmux), ("curl", client.clone()), ("claude", client)] {
        let path = bin.join(name);
        fs::write(&path, body).ok();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).ok();
    }
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default())
}

/// `seat tick` を偽の PATH と規則の写しで 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn move_run(place: &MovePlace) -> Output {
    let state = place.state.display().to_string();
    Command::new(bin())
        .args(["seat", "tick", "--state-dir", &state, "--target", TICK_TARGET, "--rules", &place.rules])
        .env("PATH", &place.path)
        .output()
        .expect("binary を起動できる")
}

/// 偽 tmux へ送った key（`send-keys` の呼出の行・送った順）。
fn move_keys(place: &MovePlace) -> Vec<String> {
    fs::read_to_string(place.at(TICK_CALLS))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("send-keys"))
        .map(str::to_owned)
        .collect()
}

/// 席の記録（`tick.jsonl`）の行。
fn move_injections(place: &MovePlace) -> Vec<String> {
    fs::read_to_string(tick_file(&place.state, TICK_SEAT)).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// 移動の周の判定行（`reason=- pointer=- step=-`・群の判定を撃たない周の `judged=-`・契約の字面）。
fn move_line(step: &str, consumed: &str, launched: &str) -> String {
    format!("decision=move target={TICK_SEAT} reason=- pointer=- step=- consumed={consumed} move={step} launched={launched} judged=-\n")
}

/// 移動の門で止まった周の判定行（梯子を評価しない側＝`pointer=- step=-`）。
fn move_noop(reason: &str) -> String {
    format!("decision=noop target={TICK_SEAT} reason={reason} pointer=- step=- consumed=-{TICK_NO_MOVE}\n")
}

/// 撃って判定行が `want` で rc 0 の周に、key も梯子の記録も席の記録も増えず、偽 client も呼ばれないことを測る。
fn move_assert_quiet(place: &MovePlace, want: &str) {
    let (keys, injections) = (move_keys(place).len(), move_injections(place).len());
    let out = move_run(place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "{want}: rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), want, "判定行");
    assert_eq!(move_keys(place).len(), keys, "{want}: 1 key も送らない");
    assert!(!place.state.join("seat").join(TICK_SEAT).join("pointer-ladder").exists(), "{want}: 梯子の記録は書かれない");
    assert_eq!(move_injections(place).len(), injections, "{want}: 席の記録は増えない");
    assert!(!place.at(TICK_CLIENT).exists(), "{want}: 偽 client は呼ばれない");
}

/// `tick.jsonl` の 1 行の `who` と `what`。
fn move_who_what(line: &str) -> (Option<String>, Option<String>) {
    (acct_text(line, "who"), acct_text(line, "what"))
}

/// (a) 登録 row（口座 A・anchor は群 Tier1）∧ 記録は口座 B ∧ 最終行 Idle（いま）∧ pane が claude ∧ 入力欄が空 → `decision=move
/// move=exit`・`/exit` の text 1 回 + Enter 1 回・`tick.jsonl` に `who=seat-tick-move what=/exit` の 1 行・梯子の記録は書かれず
/// 合図の text は 0 key（base では黙りの門の `stamp-recent` ＝ RED）。
#[test]
fn seat_tick_move_evacuates_a_seat_whose_row_differs_from_the_group_record() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), move_line("exit", "false", "-"), "判定行 1 行");
    assert_eq!(
        move_keys(&place),
        [format!("send-keys -t {TICK_TARGET} -l /exit"), format!("send-keys -t {TICK_TARGET} Enter")],
        "/exit の text 1 回 + Enter 1 回・合図の text は 0"
    );
    let injections = move_injections(&place);
    assert_eq!(injections.len(), 1, "席の記録 1 行: {injections:?}");
    let line = injections.first().map(String::as_str).unwrap_or_default();
    assert_eq!(move_who_what(line), (Some("seat-tick-move".to_owned()), Some("/exit".to_owned())), "{line}");
    assert!(!place.state.join("seat").join(TICK_SEAT).join("pointer-ladder").exists(), "梯子の記録は書かれない");
    assert!(!place.at(TICK_CLIENT).exists(), "偽 client は呼ばれない");
    assert!(!move_groups_dir(&root).join("lock").exists(), "lock は周の後に外れる");
}

/// (b) pane の最後の `❯` 行が dialog の既定の行 → Enter 1 key だけ・`/exit` 0・記録の `what` は `enter:exit-dialog`／tail が別の
/// 字面 → `input-busy`・0 key／prompt 行なし → `input-unknown`・0 key（どちらも記録 0）。
#[test]
fn seat_tick_move_confirms_the_exit_dialog_and_refuses_other_input() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    fs::write(place.at(TICK_PANE), format!("{TICK_CLEAR_PANE}half typed")).ok();
    move_assert_quiet(&place, &move_noop("input-busy"));
    fs::write(place.at(TICK_PANE), "no prompt here\n").ok();
    move_assert_quiet(&place, &move_noop("input-unknown"));
    fs::write(place.at(TICK_PANE), format!("Exit?\n\u{276f} {MOVE_DIALOG_ROW}\n  2. Cancel\n")).ok();
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), move_line("enter", "unknown:exit-dialog", "-"), "stderr={}", stderr_of(&out));
    assert_eq!(move_keys(&place), [format!("send-keys -t {TICK_TARGET} Enter")], "Enter の 1 key だけ・/exit 0");
    let injections = move_injections(&place);
    assert_eq!(injections.len(), 1, "記録 1 行: {injections:?}");
    let line = injections.first().map(String::as_str).unwrap_or_default();
    assert_eq!(move_who_what(line), (Some("seat-tick-move".to_owned()), Some("enter:exit-dialog".to_owned())), "{line}");
}

/// (c) pane が shell → `move=launch`・`send-keys` に起動行 1 行（口座 B の設定 dir を持つ）・fleet に口座 B の登録 row が 1 件
/// 増える・`/exit` 0（偽 tmux は打刻を打たない＝settle 1 秒で `launched=launch-unconfirmed`）。
#[test]
fn seat_tick_move_launches_the_group_account_into_a_shell_pane() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    fs::write(place.at(MOVE_FRONT), "bash\n").ok();
    fs::write(place.at(TICK_PANE), "old output\n$ ").ok();
    let before = acct_rows(&place.state).len();
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), move_line("launch", "-", "launch-unconfirmed"), "判定行 1 行");
    let dir = place.state.join("accounts").join(MOVE_B).display().to_string();
    let texts: Vec<String> = move_keys(&place).into_iter().filter(|key| key.contains(" -l ")).collect();
    assert_eq!(texts.len(), 1, "起動行 1 行: {texts:?}");
    assert!(texts.iter().all(|key| key.contains(&dir) && !key.contains("/exit")), "口座 B の設定 dir を持つ起動行: {texts:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), before + 1, "登録 row が 1 件増える");
    let last = rows.last().map(|row| (row.account.as_str(), row.anchor.as_str(), row.target.as_str()));
    assert_eq!(last, Some((MOVE_B, MOVE_ANCHOR, TICK_TARGET)), "口座 B・同じ anchor と target");
    assert!(!place.state.join("seat").join(TICK_SEAT).join("pointer-ladder").exists(), "梯子の記録は書かれない");
}

/// (d) 記録が dir（読めない）→ `group-unreadable`・0 key／lock の file が在る → `group-locked`・0 key・記録 0・起動行 0（pane が
/// shell でも起こさない）。最終行 Busy の移動の周は §10 形 8 で退避へ進む（`seat_tick_evacuate_` の歯）。
#[test]
fn seat_tick_move_stops_on_unreadable_record_and_held_lock() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    let lock = move_groups_dir(&root).join("lock");
    fs::write(&lock, "pid=1\n").ok();
    move_assert_quiet(&place, &move_noop("group-locked"));
    fs::write(place.at(MOVE_FRONT), "bash\n").ok();
    fs::write(place.at(TICK_PANE), "old output\n$ ").ok();
    let rows = acct_rows(&place.state).len();
    move_assert_quiet(&place, &move_noop("group-locked"));
    assert_eq!(acct_rows(&place.state).len(), rows, "起こさない（登録 row は増えない）");
    assert!(lock.exists(), "他の手の lock は外さない");
    fs::remove_file(&lock).ok();
    let record = move_groups_dir(&root).join(format!("{MOVE_GROUP}.account"));
    fs::remove_file(&record).ok();
    fs::create_dir_all(&record).ok();
    move_assert_quiet(&place, &move_noop("group-unreadable"));
}

/// (e) 記録の口座 = row の口座（移動済み）／群に属さない anchor／記録なしで種 = row → §2 の列のまま（`stamp-recent`・黙った席は
/// `inject`・どちらも `move=- launched=-`）。
#[test]
fn seat_tick_move_leaves_the_list_unchanged_when_the_row_matches_or_the_anchor_is_outside() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_assert_quiet(&place, &format!("decision=noop target={TICK_SEAT} reason=stamp-recent pointer=wait:0 step=0 consumed=-{TICK_NO_MOVE}\n"));
    move_record(&root, MOVE_A);
    move_assert_quiet(&place, &format!("decision=noop target={TICK_SEAT} reason=stamp-recent pointer=wait:0 step=0 consumed=-{TICK_NO_MOVE}\n"));
    let seat = seat_dir_of(&place.state, TICK_SEAT);
    fs::write(state_file(&seat), format!("{}\n", stamp_line("idle", "Stop", unix_now() - TICK_STALE - 60, "sid-move"))).ok();
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), tick_inject(0), "記録と一致する黙った席は合図: stderr={}", stderr_of(&out));
    let outside = tmp();
    let other = move_place(&outside, "state", "/elsewhere");
    move_record(&outside, MOVE_B);
    move_assert_quiet(&other, &format!("decision=noop target={TICK_SEAT} reason=stamp-recent pointer=wait:0 step=0 consumed=-{TICK_NO_MOVE}\n"));
    assert!(!move_groups_dir(&outside).join("lock").exists(), "群に属さない席は lock も取らない");
}

/// (f) 親を共有する 2 つの置き場に 1 席ずつ（群の置き場 2 つ）・記録は親の下の 1 file → 両方の tick が `move=exit`（置き場を
/// 跨いで同じ記録を読む＝別 project の席も移る）。
#[test]
fn seat_tick_move_reaches_seats_in_two_state_dirs_under_one_parent() {
    let root = tmp();
    let one = move_place(&root, "one", MOVE_ANCHOR);
    let two = move_place(&root, "two", MOVE_ANCHOR_TWO);
    move_record(&root, MOVE_B);
    for place in [&one, &two] {
        let out = move_run(place);
        assert_eq!(stdout_of(&out), move_line("exit", "false", "-"), "stderr={}", stderr_of(&out));
        assert_eq!(move_keys(place).first(), Some(&format!("send-keys -t {TICK_TARGET} -l /exit")), "/exit を送る");
        assert_eq!(move_injections(place).len(), 1, "置き場ごとに記録 1 行");
    }
}

// ───────────── tick が群の移動の判定を撃つ（seat-heartbeat.md §9・契約表の行 i・`s2-07l.631`・接頭辞 `seat_tick_judge_`） ─────────────
//
// §4 の移動の fixture（偽 tmux・host の群用 dir・`--rules` の写し）に、pipe/dispatch.rs の群の fixture と同じ形の偽 usage client
// （stdin の token で口座ごとの本文を返し・argv を file に残す）と口座 A / B の credential を足す。群の 2 つ目の置き場の席
// （`tk:two`・口座 A）を同じ置き場に積み、判定の側がその席へ 1 key も送らないことを測る。判定の打刻は消して始める。

/// 群の判定の打刻（`<群用 dir>/<群>.judged`・契約の字面）。
fn judge_stamp(root: &Path) -> PathBuf {
    move_groups_dir(root).join(format!("{MOVE_GROUP}.judged"))
}

/// 偽 usage client の argv の記録。
const JUDGE_ARGS: &str = "usage-args";
/// 群の 2 つ目の置き場の席の target（判定の側が触らない席）。
const JUDGE_OTHER: &str = "tk:two";

/// 判定の歯の置き場: [`move_place`] に口座 A / B の credential と本文（5 時間窓だけ `five`・他の窓は 10）・偽 usage client・群の
/// 2 つ目の置き場の席を足し、判定の打刻を消す。
fn judge_place(root: &Path, anchor: &str, five: [u64; 2]) -> MovePlace {
    let place = move_place(root, "state", anchor);
    fs::remove_file(judge_stamp(root)).ok();
    for (label, used) in [(MOVE_A, five[0]), (MOVE_B, five[1])] {
        let dir = place.state.join("accounts").join(label);
        fs::create_dir_all(&dir).ok();
        let credential =
            format!("{{\"claudeAiOauth\":{{\"accessToken\":\"tok-{label}\",\"refreshToken\":\"r\",\"expiresAt\":4102444800000}}}}");
        fs::write(dir.join(".credentials.json"), credential).ok();
        let far = "2099-01-01T00:00:00Z";
        let body = format!(
            "{{\"five_hour\":{{\"utilization\":{used},\"resets_at\":\"{far}\"}},\"seven_day\":{{\"utilization\":10,\"resets_at\":\"{far}\"}},\
             \"limits\":[{{\"kind\":\"weekly_scoped\",\"percent\":10,\"resets_at\":\"{far}\",\"scope\":{{\"model\":{{\"display_name\":\"Fable\"}}}}}}]}}"
        );
        fs::write(place.at(&format!("body-tok-{label}")), body).ok();
    }
    let (args, tools) = (place.at(JUDGE_ARGS).display().to_string(), place.tools.display().to_string());
    let curl = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{args}'\ncfg=$(cat)\nfor f in '{tools}'/body-*; do\n\
         case \"$cfg\" in *\"Bearer ${{f##*/body-}}\\\"\"*) cat \"$f\" ;; esac\ndone\nprintf '\\n%s' '200'\n"
    );
    fs::write(place.tools.join("bin").join("curl"), curl).ok();
    let two = seat_dir_of(&place.state, &JUDGE_OTHER.replace(':', "_"));
    fs::create_dir_all(&two).ok();
    fs::write(state_file(&two), format!("{}\n", stamp_line("idle", "SessionStart", unix_now(), "sid-two"))).ok();
    let launch = fixture(&place.tools, "launch-two.txt", "claude\n");
    let path = place.state.display().to_string();
    let out = run_seat(&[
        "register", "--state-dir", &path, "--target", JUDGE_OTHER, "--role", "orchestrator", "--account", MOVE_A, "--launch",
        &launch, "--anchor", MOVE_ANCHOR_TWO,
    ]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "2 つ目の席の登録 row を積める: {}", stderr_of(&out));
    place
}

/// 偽 usage client の呼出の回数（口座 1 つにつき 1 回）。
fn judge_calls(place: &MovePlace) -> usize {
    fs::read_to_string(place.at(JUDGE_ARGS)).unwrap_or_default().lines().filter(|arg| *arg == "--max-time").count()
}

/// 置き場の event log の `kind` の件数。
fn judge_events(place: &MovePlace, kind: vessel::fleet::EventKind) -> usize {
    vessel::fleet::store::read_all(&place.state).unwrap_or_default().iter().filter(|event| event.kind == kind).count()
}

/// 判定の打刻の ts（無い・読めなければ `None`）。
fn judge_ts(root: &Path) -> Option<u64> {
    fs::read_to_string(judge_stamp(root)).ok()?.trim().parse().ok()
}

/// 黙っていない席（最終行はいま）で移動の周でない判定行（`judged=` の語だけを変える）。
fn judge_recent(judged: &str) -> String {
    format!("decision=noop target={TICK_SEAT} reason=stamp-recent pointer=wait:0 step=0 consumed=- move=- launched=- judged={judged}\n")
}

/// 断りの 1 行（器の字面を借りない）。
fn judge_refused() -> String {
    format!("{NAME} group: move-refused group={MOVE_GROUP} reason=no-candidate")
}

/// (a) 群の今の口座（種 A）が逼迫 ∧ 候補 B は閾値未満 ∧ 判定の打刻なし ∧ 前面 `claude` ∧ 入力欄が空 → 記録が B へ動き・承認
/// event 1・`judged=moved:acct-b`・自席への key は移動の門の `/exit` の 1 行だけ・2 つ目の席へ 0 key・通知 0・打刻は判定の周の
/// ts（base では記録不変 ∧ 打刻の file 無し ＝ RED）。
#[test]
fn seat_tick_judge_moves_the_record_and_only_the_move_gate_sends_the_exit() {
    use vessel::fleet::EventKind;
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 10]);
    let before = unix_now();
    let out = move_run(&place);
    let after = unix_now();
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let want = format!("decision=move target={TICK_SEAT} reason=- pointer=- step=- consumed=false move=exit launched=- judged=moved:{MOVE_B}\n");
    assert_eq!(stdout_of(&out), want, "判定行 1 行");
    let record = fs::read_to_string(move_groups_dir(&root).join(format!("{MOVE_GROUP}.account"))).unwrap_or_default();
    assert!(record.starts_with(&format!("account={MOVE_B}\n")) && record.contains(&format!("previous={MOVE_A}\n")), "{record}");
    assert_eq!(judge_events(&place, EventKind::GroupMoved), 1, "承認 event 1");
    assert_eq!(judge_events(&place, EventKind::GroupPressureNotified), 0, "通知 0");
    assert_eq!(
        move_keys(&place),
        [format!("send-keys -t {TICK_TARGET} -l /exit"), format!("send-keys -t {TICK_TARGET} Enter")],
        "自席への /exit は移動の門の 1 行だけ・他の席へ 0 key・通知の行 0"
    );
    assert_eq!(judge_calls(&place), 2, "今の口座と候補を 1 回ずつ測る");
    assert!(judge_ts(&root).is_some_and(|ts| (before..=after).contains(&ts)), "打刻は判定の周の ts: {:?}", judge_ts(&root));
    assert!(!move_groups_dir(&root).join("lock").exists(), "lock は周の後に外れる");
}

/// (b) 判定の打刻が鮮度の内側 → 計測 0・`judged=-`・打刻は不変。fixture が置いた打刻の周と、(a) の周が書いた打刻の直後に
/// もう 1 周撃つ周（自前の打刻を鮮度の内側と読む）の 2 本。
#[test]
fn seat_tick_judge_skips_a_fresh_stamp_without_measuring() {
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 10]);
    let put = unix_now() - 10;
    fs::write(judge_stamp(&root), format!("{put}\n")).ok();
    move_assert_quiet(&place, &judge_recent("-"));
    assert_eq!((judge_calls(&place), judge_ts(&root)), (0, Some(put)), "計測 0・打刻は不変");
    assert!(!move_groups_dir(&root).join(format!("{MOVE_GROUP}.account")).exists(), "記録は書かれない");
    let again = tmp();
    let place = judge_place(&again, MOVE_ANCHOR, [90, 10]);
    let first = move_run(&place);
    assert!(stdout_of(&first).ends_with(&format!(" judged=moved:{MOVE_B}\n")), "1 周目は判定する: {}", stdout_of(&first));
    let (calls, stamped) = (judge_calls(&place), judge_ts(&again));
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), move_line("exit", "false", "-"), "2 周目は自前の打刻で撃たない: stderr={}", stderr_of(&out));
    assert_eq!((judge_calls(&place), judge_ts(&again)), (calls, stamped), "2 周目の計測 0・打刻は不変");
}

/// (c) 候補なし（A / B とも逼迫）∧ 入力欄が空 → `judged=none`・断りの event 1・記録不変・自席へ断りの 1 行（群の段の断りの
/// 字面）・他の席へ 0 key（base では 0 行 ＝ RED）。
#[test]
fn seat_tick_judge_without_a_candidate_refuses_to_its_own_seat_only() {
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 90]);
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), judge_recent("none"), "stderr={}", stderr_of(&out));
    assert_eq!(judge_events(&place, vessel::fleet::EventKind::GroupMoveRefused), 1, "断りの event 1");
    assert!(!move_groups_dir(&root).join(format!("{MOVE_GROUP}.account")).exists(), "記録不変");
    assert_eq!(
        move_keys(&place),
        [format!("send-keys -t {TICK_TARGET} -l {}", judge_refused()), format!("send-keys -t {TICK_TARGET} Enter")],
        "自席へ断りの 1 行だけ・他の席へ 0 key"
    );
}

/// (c2) 候補なし ∧ 同じ実測に断りの event が既に在る（(c) の直後に打刻を消してもう 1 周）→ `judged=none`・event 0・自席へ 0 行。
#[test]
fn seat_tick_judge_does_not_refuse_the_same_measurement_twice() {
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 90]);
    let first = move_run(&place);
    assert_eq!(stdout_of(&first), judge_recent("none"), "1 周目は断る: stderr={}", stderr_of(&first));
    fs::remove_file(judge_stamp(&root)).ok();
    let keys = move_keys(&place).len();
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), judge_recent("none"), "stderr={}", stderr_of(&out));
    assert_eq!(judge_events(&place, vessel::fleet::EventKind::GroupMoveRefused), 1, "event 0（1 のまま）");
    assert_eq!(move_keys(&place).len(), keys, "自席へ 0 行");
}

/// (d) 群用 dir に lock が在る → 判定 0（計測 0・記録 0・打刻なし）・列は今のまま（`judged=-`）。
#[test]
fn seat_tick_judge_held_lock_judges_nothing() {
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 10]);
    let lock = move_groups_dir(&root).join("lock");
    fs::write(&lock, "pid=1\n").ok();
    move_assert_quiet(&place, &judge_recent("-"));
    assert_eq!((judge_calls(&place), judge_ts(&root)), (0, None), "計測 0・打刻なし");
    assert_eq!(judge_events(&place, vessel::fleet::EventKind::GroupMoved), 0, "承認 event 0");
    assert!(lock.exists(), "他の手の lock は外さない");
}

/// (e) 群に属さない anchor → `judged=-`・0 key・計測 0（今の口座が逼迫でも判定しない）。
#[test]
fn seat_tick_judge_outside_the_group_is_unjudged() {
    let root = tmp();
    let place = judge_place(&root, "/elsewhere", [90, 10]);
    move_assert_quiet(&place, &judge_recent("-"));
    assert_eq!((judge_calls(&place), judge_ts(&root)), (0, None), "計測 0・打刻なし");
}

// ───────────── tick の判定も群の予約で移る（account-lifecycle.md §29 形 2 / 3・契約表の行 r・接頭辞 `seat_tick_judge_reserve_`） ─────────────
//
// §9 の判定の fixture（[`judge_place`]）の host の面を群 2 つに書き換える: 先の Tier1（置き場は席の無い `/elsewhere`・候補
// [D, B, C]・種 D）と自席の Tier2（置き場 2 つ・候補 [A, B, C]・種 A）。

/// 群 2 つ目の候補の口座（7 日窓 30＝残量 70 < B の残量 90）。
const RESERVE_C: &str = "acct-c";
/// Tier1 の種の口座（測らない）。
const RESERVE_D: &str = "acct-d";

/// [`judge_place`] の host の面を群 2 つ（Tier1 → Tier2 の宣言順）に書き換え、口座 C の credential と本文を足す。
fn reserve_place(root: &Path) -> MovePlace {
    let place = judge_place(root, MOVE_ANCHOR, [90, 10]);
    let host = format!(
        "schema = 1\n\n[[account]]\nlabel = \"{MOVE_A}\"\n\n[[account]]\nlabel = \"{MOVE_B}\"\n\n[[account]]\nlabel = \"{RESERVE_C}\"\n\n\
         [[account]]\nlabel = \"{RESERVE_D}\"\n\n[[account-group]]\nname = \"Tier1\"\nanchors = [\"/elsewhere\"]\n\
         accounts = [\"{RESERVE_D}\", \"{MOVE_B}\", \"{RESERVE_C}\"]\n\n[[account-group]]\nname = \"Tier2\"\n\
         anchors = [\"{MOVE_ANCHOR}\", \"{MOVE_ANCHOR_TWO}\"]\naccounts = [\"{MOVE_A}\", \"{MOVE_B}\", \"{RESERVE_C}\"]\n"
    );
    fs::write(place.state.join("host.toml"), host).ok();
    let dir = place.state.join("accounts").join(RESERVE_C);
    fs::create_dir_all(&dir).ok();
    let credential = format!("{{\"claudeAiOauth\":{{\"accessToken\":\"tok-{RESERVE_C}\",\"refreshToken\":\"r\",\"expiresAt\":4102444800000}}}}");
    fs::write(dir.join(".credentials.json"), credential).ok();
    let far = "2099-01-01T00:00:00Z";
    let body = format!(
        "{{\"five_hour\":{{\"utilization\":10,\"resets_at\":\"{far}\"}},\"seven_day\":{{\"utilization\":30,\"resets_at\":\"{far}\"}},\
         \"limits\":[{{\"kind\":\"weekly_scoped\",\"percent\":10,\"resets_at\":\"{far}\",\"scope\":{{\"model\":{{\"display_name\":\"Fable\"}}}}}}]}}"
    );
    fs::write(place.at(&format!("body-tok-{RESERVE_C}")), body).ok();
    place
}

/// 自席の群 Tier2 の今の口座（記録の `account=`・無ければ `None`）。
fn reserve_record(root: &Path) -> Option<String> {
    let text = fs::read_to_string(move_groups_dir(root).join("Tier2.account")).ok()?;
    text.lines().find_map(|line| line.strip_prefix("account=")).map(str::to_owned)
}

/// Tier2 の今の口座 A が逼迫の周、tick の判定は先の Tier1 の予約 B（Tier1 の鍵の先頭・残量 90）を飛ばして C へ移る
/// （`judged=moved:acct-c`・承認 event 1・base は宣言順の B ＝ RED）。
#[test]
fn seat_tick_judge_reserve_skips_the_tier1_reservation() {
    let root = tmp();
    let place = reserve_place(&root);
    let out = move_run(&place);
    assert!(stdout_of(&out).ends_with(&format!(" judged=moved:{RESERVE_C}\n")), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    assert_eq!(reserve_record(&root).as_deref(), Some(RESERVE_C), "記録は C");
    assert_eq!(judge_events(&place, vessel::fleet::EventKind::GroupMoved), 1, "承認 event 1");
    assert!(!move_groups_dir(&root).join("Tier1.account").exists(), "Tier1 の記録は書かない（予約は記録しない）");
}

/// 群の席の役割の `seat.model.orchestrator` 行を欠く `--rules` の tick は、今の口座が逼迫でも移らず `judged=error:unreadable`
/// で 0 key・記録 0・承認 event 0・断りの event 0（集合を空に読み替えない・base は B へ移る ＝ RED）。
#[test]
fn seat_tick_judge_reserve_missing_role_row_is_an_error_with_zero_keys() {
    use vessel::fleet::EventKind;
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 10]);
    let row = format!("\n[[rule]]\nid = \"{MOVE_ROLE_ROW}\"\nkind = \"RoleModel\"\nvalue = \"fable\"\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n");
    let rules = fs::read_to_string(&place.rules).unwrap_or_default();
    assert!(rules.contains(&row), "写しに役割の行が在る");
    fs::write(&place.rules, rules.replace(&row, "")).ok();
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), judge_recent("error:unreadable"), "stderr={}", stderr_of(&out));
    assert_eq!(move_keys(&place), Vec::<String>::new(), "0 key");
    assert!(!move_groups_dir(&root).join(format!("{MOVE_GROUP}.account")).exists(), "記録 0");
    assert_eq!((judge_events(&place, EventKind::GroupMoved), judge_events(&place, EventKind::GroupMoveRefused)), (0, 0), "event 0");
}

// ───────────── pane が shell かの判定は子 process まで見る（seat-heartbeat.md §6・契約表の行 e・`s2-07l.624`・接頭辞 `seat_pane_shell_`） ─────────────
//
// §4 の移動の fixture（偽 tmux の前面の file・群の記録）に、偽 tmux の `display-message` が返す pid の file を足す。pid は歯が
// 起こした実 process（子を持つ `sh -c 'sleep 60; :'`・子の無い `sleep 60`）のもの。判定は tick の移動の周の手（`move=exit` は
// shell でない・`move=launch` は shell）で測る。

/// 偽 tmux の `display-message` が返す pid の file（無ければ空＝pid 不明）。
const PANE_SHELL_PID: &str = "pane-pid";

/// 歯が起こした実 process（自分の process group の頭）。drop で group ごと止める（子の `sleep` を残さない）。
struct PaneShellKin(std::process::Child);

impl Drop for PaneShellKin {
    fn drop(&mut self) {
        Command::new("kill").args(["-KILL", "--", &format!("-{}", self.0.id())]).output().ok();
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

/// `args` を自分の process group で起こし、`children` が空でない（`want_child`）か pid が在る周まで待つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn pane_shell_kin(args: &[&str], want_child: bool) -> PaneShellKin {
    use std::os::unix::process::CommandExt;
    let (program, rest) = args.split_first().expect("起こす command を持つ");
    let kin = PaneShellKin(Command::new(program).args(rest).process_group(0).spawn().expect("実 process を起こせる"));
    let pid = kin.0.id();
    let children = format!("/proc/{pid}/task/{pid}/children");
    let deadline = Instant::now() + Duration::from_secs(10);
    while want_child && fs::read_to_string(&children).unwrap_or_default().trim().is_empty() && Instant::now() < deadline {
        sleep(Duration::from_millis(20));
    }
    assert_eq!(!fs::read_to_string(&children).unwrap_or_default().trim().is_empty(), want_child, "{children} の子の有無");
    kin
}

/// 子の居ない shell の pane の字面（§4 (c) と同じ形）。
const PANE_SHELL_PROMPT: &str = "old output\n$ ";

/// 移動の周（row は口座 A・記録は口座 B・前面 `sh`）の置き場を作り、pane を `pane` で・pid の file を `pid` で置いて tick を
/// 1 回撃つ（tmp の根も返す＝drop で置き場が消えない）。
fn pane_shell_round(pane: &str, pid: Option<&str>) -> (TmpDir, MovePlace, Output) {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    fs::write(place.at(MOVE_FRONT), "sh\n").ok();
    fs::write(place.at(TICK_PANE), pane).ok();
    if let Some(body) = pid {
        fs::write(place.at(PANE_SHELL_PID), body).ok();
    }
    let out = move_run(&place);
    (root, place, out)
}

/// 偽 tmux の呼出のうち `verb` で始まる行。
fn pane_shell_calls(place: &MovePlace, verb: &str) -> Vec<String> {
    fs::read_to_string(place.at(TICK_CALLS))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with(verb))
        .map(str::to_owned)
        .collect()
}

/// (a) 前面 `sh` ∧ pid の shell が子を持つ → shell でない（席が中で動いている）＝移動の周は `move=exit`・`/exit` を送り
/// 起動行は 0・`display-message` は pane の pid を 1 回だけ引く（base では前面の語だけで shell と読み `move=launch` ＝ RED）。
#[test]
fn seat_pane_shell_reads_a_shell_with_a_child_as_a_running_seat() {
    let kin = pane_shell_kin(&["sh", "-c", "sleep 60; :"], true);
    let (_root, place, out) = pane_shell_round(TICK_CLEAR_PANE, Some(&format!("{}\n", kin.0.id())));
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), move_line("exit", "false", "-"), "子を持つ sh は席");
    assert_eq!(
        move_keys(&place),
        [format!("send-keys -t {TICK_TARGET} -l /exit"), format!("send-keys -t {TICK_TARGET} Enter")],
        "/exit の text 1 回 + Enter 1 回・起動行 0"
    );
    assert_eq!(
        pane_shell_calls(&place, "display-message"),
        [format!("display-message -p -t {TICK_TARGET} #{{pane_pid}}")],
        "pane の pid を 1 回引く"
    );
}

/// (b) 前面 `sh` ∧ pid の process が子なし → shell のまま（`move=launch`・起動行 1 行）。
#[test]
fn seat_pane_shell_keeps_a_childless_shell_as_a_shell() {
    let kin = pane_shell_kin(&["sleep", "60"], false);
    let (_root, place, out) = pane_shell_round(PANE_SHELL_PROMPT, Some(&format!("{}\n", kin.0.id())));
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), move_line("launch", "-", "launch-unconfirmed"), "子なしの sh は shell");
    let texts: Vec<String> = move_keys(&place).into_iter().filter(|key| key.contains(" -l ")).collect();
    assert_eq!(texts.len(), 1, "起動行 1 行: {texts:?}");
    assert!(texts.iter().all(|key| !key.contains("/exit")), "/exit 0: {texts:?}");
}

/// (c) 前面 `sh` ∧ pid の file が無い／整数でない／`/proc` に無い pid → 不明＝前面の語だけで shell（今のまま `move=launch`）。
#[test]
fn seat_pane_shell_falls_back_to_the_front_word_when_the_pid_is_unknown() {
    for pid in [None, Some("not-a-pid\n"), Some("-1\n"), Some("4294967295\n")] {
        let (_root, place, out) = pane_shell_round(PANE_SHELL_PROMPT, pid);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{pid:?}: stderr={}", stderr_of(&out));
        assert_eq!(stdout_of(&out), move_line("launch", "-", "launch-unconfirmed"), "{pid:?}: 不明は shell のまま");
        assert!(!pane_shell_calls(&place, "display-message").is_empty(), "{pid:?}: pid を引いた上で前面の語に倒れる");
    }
}

/// (d) 前面 `claude` → tmux の判定は `list-panes` の 1 回だけ（`display-message` の呼出 0・pid の file が在っても引かない）。
#[test]
fn seat_pane_shell_does_not_ask_the_pid_when_the_front_is_not_a_shell() {
    let kin = pane_shell_kin(&["sh", "-c", "sleep 60; :"], true);
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    fs::write(place.at(PANE_SHELL_PID), format!("{}\n", kin.0.id())).ok();
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), move_line("exit", "false", "-"), "stderr={}", stderr_of(&out));
    assert_eq!(pane_shell_calls(&place, "list-panes").len(), 1, "前面は 1 回だけ引く");
    assert_eq!(pane_shell_calls(&place, "display-message"), Vec::<String>::new(), "display-message は 0 回");
}

// ───────────── tick が死んだ席を起こし会話を運ぶ（seat-heartbeat.md §7・契約表の行 f・`s2-07l.626`・接頭辞 `seat_tick_wake_`） ─────────────
//
// §4 の移動の fixture（偽 tmux の前面の file・群の記録・`--rules` の写し）に §6 の `display-message` の口（pid の file が無い＝
// 不明で前面の語に倒れる）をそのまま使い、打刻の最終行を Busy（いま）で書く。群の外の row は anchor を群の置き場の外に置く。

/// 打刻に書く会話 id（UUID の形）。
const WAKE_SID: &str = "3f2a9c4e-1b7d-4e8a-9c0f-5d6e7a8b9c0d";

/// 窓が shell（前面 `bash`・子なしの prompt）で、最終行が Busy（いま・`sid`）の置き場を `anchor` の row で作る（群の記録は口座 B）。
fn wake_place(root: &Path, anchor: &str, sid: &str) -> MovePlace {
    let place = move_place(root, "state", anchor);
    move_record(root, MOVE_B);
    fs::write(place.at(MOVE_FRONT), "bash\n").ok();
    fs::write(place.at(TICK_PANE), PANE_SHELL_PROMPT).ok();
    let seat = seat_dir_of(&place.state, TICK_SEAT);
    fs::write(state_file(&seat), format!("{}\n", stamp_line("busy", "UserPromptSubmit", unix_now() - 10, sid))).ok();
    place
}

/// 送った起動行（`-l` の text の key）。
fn wake_texts(place: &MovePlace) -> Vec<String> {
    move_keys(place).into_iter().filter(|key| key.contains(" -l ")).collect()
}

/// 口座 `account` の設定 dir（起動行が持つ字面）。
fn wake_account_dir(place: &MovePlace, account: &str) -> String {
    place.state.join("accounts").join(account).display().to_string()
}

/// 起動行の末尾の初手の 1 語（§10 形 1〜3・単引用で括った `<NAME> seat: relaunch …`）。
fn wake_first_word() -> String {
    let word = vessel::seat::state::relaunch_word();
    assert!(word.starts_with(&format!("'{NAME} seat: relaunch ")) && word.ends_with('\''), "単引用の初手: {word}");
    word
}

/// 起こした周を測る: 判定行は `move=launch`・起動行 1 行が `account` の設定 dir を持ち `/exit` を持たない・fleet の最後の row は
/// `account` で同じ anchor と target・row の launch に `--resume` も初手も載らない。起動行を返す。
fn wake_assert_launched(place: &MovePlace, account: &str, anchor: &str) -> String {
    let out = move_run(place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), move_line("launch", "-", "launch-unconfirmed"), "判定行 1 行: stderr={}", stderr_of(&out));
    let texts = wake_texts(place);
    assert_eq!(texts.len(), 1, "起動行 1 行: {texts:?}");
    let text = texts.first().cloned().unwrap_or_default();
    assert!(text.contains(&wake_account_dir(place, account)) && !text.contains("/exit"), "{account} の設定 dir を持つ起動行: {text}");
    let rows = acct_rows(&place.state);
    let last = rows.last().map(|row| (row.account.as_str(), row.anchor.as_str(), row.target.as_str()));
    assert_eq!(last, Some((account, anchor, TICK_TARGET)), "起こした row");
    assert!(rows.iter().all(|row| !row.launch.contains("--resume")), "row の launch に --resume は載らない: {rows:?}");
    assert!(rows.iter().all(|row| !row.launch.contains("seat: relaunch")), "row の launch に初手は載らない: {rows:?}");
    assert!(!place.state.join("seat").join(TICK_SEAT).join("pointer-ladder").exists(), "梯子の記録は書かれない");
    text
}

/// (a) 最終行 busy ∧ 前面 `bash` ∧ 群の外の row → `move=launch`・起動行 1 行が row の口座（A）を持ち、末尾に `--resume <打刻の
/// sid> '<NAME> seat: relaunch …'`（base では `--resume <sid>` で終わる ＝ RED）。群の外の席は lock を取らない。
#[test]
fn seat_tick_wake_launches_a_dead_seat_outside_a_group_with_the_stamped_sid() {
    let root = tmp();
    let place = wake_place(&root, "/elsewhere", WAKE_SID);
    let text = wake_assert_launched(&place, MOVE_A, "/elsewhere");
    assert!(text.ends_with(&format!(" --resume {WAKE_SID} {}", wake_first_word())), "末尾に --resume <sid> と初手: {text}");
    assert_eq!(text.matches("--resume").count(), 1, "--resume は 1 つ: {text}");
    assert_eq!(text.matches("seat: relaunch").count(), 1, "初手は 1 つ: {text}");
    assert!(!move_groups_dir(&root).join("lock").exists(), "lock は残らない");
}

/// (b) 同じ席で最終行の sid が無い・会話 id の形でない → 起こすが `--resume` は無く、末尾は初手の 1 語だけ（前の行の sid にも倒れない）。
#[test]
fn seat_tick_wake_without_a_session_id_carries_nothing() {
    for sid in ["", "sid-move"] {
        let root = tmp();
        let place = wake_place(&root, "/elsewhere", sid);
        let seat = seat_dir_of(&place.state, TICK_SEAT);
        let before = stamp_line("idle", "Stop", unix_now() - 100, WAKE_SID);
        let last = stamp_line("busy", "UserPromptSubmit", unix_now() - 10, sid);
        fs::write(state_file(&seat), format!("{before}\n{last}\n")).ok();
        let text = wake_assert_launched(&place, MOVE_A, "/elsewhere");
        assert!(!text.contains("--resume"), "{sid:?}: --resume 無し: {text}");
        assert!(!text.contains(WAKE_SID), "{sid:?}: 前の行の sid に倒れない: {text}");
        assert!(text.ends_with(&format!(" {}", wake_first_word())), "{sid:?}: 末尾は初手の 1 語: {text}");
        assert_eq!(text.matches("seat: relaunch").count(), 1, "{sid:?}: 初手は 1 つ: {text}");
    }
}

/// (c) 最終行 busy ∧ 前面 `bash` ∧ 群の row（口座 A）∧ 記録 = 口座 B → 記録の口座 B で起動・末尾に `--resume <sid>` と初手の 1 語。
#[test]
fn seat_tick_wake_launches_a_group_seat_with_the_record_account() {
    let root = tmp();
    let place = wake_place(&root, MOVE_ANCHOR, WAKE_SID);
    let text = wake_assert_launched(&place, MOVE_B, MOVE_ANCHOR);
    assert!(text.ends_with(&format!(" --resume {WAKE_SID} {}", wake_first_word())), "末尾に --resume <sid> と初手: {text}");
    assert!(!text.contains(&wake_account_dir(&place, MOVE_A)), "row の口座 A では起こさない: {text}");
    assert!(!move_groups_dir(&root).join("lock").exists(), "lock は周の後に外れる");
}

/// (d) 前面 `claude` ∧ 最終行 busy → `noop busy`（今のまま・0 key・起動行 0）。群の外の row も、記録 = row の群の row も同じ
/// （記録 ≠ row の移動の周は §10 形 8 で退避へ進む＝`seat_tick_evacuate_` の歯）。
#[test]
fn seat_tick_wake_leaves_a_busy_claude_front_alone() {
    for anchor in ["/elsewhere", MOVE_ANCHOR] {
        let root = tmp();
        let place = wake_place(&root, anchor, WAKE_SID);
        move_record(&root, MOVE_A);
        fs::remove_file(place.at(MOVE_FRONT)).ok();
        fs::write(place.at(TICK_PANE), TICK_CLEAR_PANE).ok();
        let rows = acct_rows(&place.state).len();
        move_assert_quiet(&place, &move_noop("busy"));
        assert_eq!(acct_rows(&place.state).len(), rows, "{anchor}: 起こさない");
    }
}

/// (e) 前面 `bash` ∧ row 無し（別の target）→ `no-row`・0 key・前面を引かない（row の無い窓は 1 字も変わらない）。
#[test]
fn seat_tick_wake_does_not_touch_a_window_without_a_row() {
    let root = tmp();
    let place = wake_place(&root, "/elsewhere", WAKE_SID);
    let state = place.state.display().to_string();
    let out = Command::new(bin())
        .args(["seat", "tick", "--state-dir", &state, "--target", "tk:other", "--rules", &place.rules])
        .env("PATH", &place.path)
        .output()
        .expect("binary を起動できる");
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("decision=noop target=tk_other reason=no-row pointer=- step=- consumed=-{TICK_NO_MOVE}\n"),
        "判定行"
    );
    assert!(move_keys(&place).is_empty(), "0 key");
    assert!(pane_shell_calls(&place, "list-panes").is_empty(), "前面を引かない");
    assert!(!place.at(TICK_CLIENT).exists(), "偽 client は呼ばれない");
}

// ─────────────────── state-stale の再判定（seat-heartbeat.md §7 形 7・契約表の行 h・`s2-07l.629`・接頭辞 `seat_tick_stale_`） ───────────────────

/// (f) 最終行の Busy が stale の 2 倍より古い ∧ 前面 `claude` ∧ 入力欄が空 → Busy を無視して列の先へ進み、合図 1 行を注入する
/// （base では `state-stale`）。打刻 file は 1 byte も書き換えない。
#[test]
fn seat_tick_stale_busy_past_twice_the_stale_with_a_clear_input_goes_on_to_the_signal() {
    let place = tick_place(true);
    tick_stamps(&place, &[("busy", "UserPromptSubmit", unix_now() - 2 * TICK_STALE - 60)]);
    let stamps = fs::read_to_string(state_file(&place.seat())).unwrap_or_default();
    let out = tick_run(&place, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), tick_inject(0), "判定行は列の先の語");
    assert_eq!(tick_keys(&place), [tick_text_key(0), format!("send-keys -t {TICK_TARGET} Enter")], "text 1 回 + Enter 1 回");
    assert_eq!(fs::read_to_string(state_file(&place.seat())).unwrap_or_default(), stamps, "打刻は書き換えない");
}

/// (f) 最終行の Busy が stale の 2 倍より古い ∧ 前面 `claude` ∧ 入力欄に字が在る → `state-stale` のまま（人が見る）・0 key。
#[test]
fn seat_tick_stale_busy_past_twice_the_stale_with_typed_input_stays_state_stale() {
    let place = tick_place(true);
    tick_stamps(&place, &[("busy", "UserPromptSubmit", unix_now() - 2 * TICK_STALE - 60)]);
    fs::write(place.at(TICK_PANE), format!("{TICK_CLEAR_PANE}half typed")).ok();
    tick_assert_quiet(&place, &tick_noop("state-stale", "-", "-"));
    assert!(tick_keys(&place).is_empty(), "0 key");
}

/// (g) 最終行の Busy が stale より古く 2 倍以内 ∧ 前面 `claude` ∧ 入力欄が空 → `state-stale` のまま・0 key（(f) と対で係数 2 を
/// pin する＝係数を 1 にする変異はここで落ちる）。
#[test]
fn seat_tick_stale_busy_within_twice_the_stale_with_a_clear_input_stays_state_stale() {
    let place = tick_place(true);
    for ago in [TICK_STALE + 60, 2 * TICK_STALE - 60] {
        tick_stamps(&place, &[("busy", "UserPromptSubmit", unix_now() - ago)]);
        tick_assert_quiet(&place, &tick_noop("state-stale", "-", "-"));
    }
    assert!(tick_keys(&place).is_empty(), "0 key");
}

// ─────── 移動の周の退避は打刻に依らない（seat-heartbeat.md §10 形 8〜10・契約表の行 m・`s2-07l.635`・接頭辞 `seat_tick_evacuate_`） ───────
//
// §4 の移動の fixture（偽 tmux・host の群用 dir・`--rules` の写し・前面 `claude`・空の入力欄）の打刻を Busy に書き換える。

/// 移動の fixture（登録 row は口座 A・anchor は群）に群の記録 `record` と最終行 Busy（`ago` 秒前）の打刻を置く。
fn evacuate_place(root: &Path, record: &str, ago: u64) -> MovePlace {
    let place = move_place(root, "state", MOVE_ANCHOR);
    move_record(root, record);
    let seat = seat_dir_of(&place.state, TICK_SEAT);
    fs::write(state_file(&seat), format!("{}\n", stamp_line("busy", "UserPromptSubmit", unix_now() - ago, "sid-move"))).ok();
    place
}

/// 1 周撃ち、判定行が `move=exit` で `/exit` の text 1 回 + Enter 1 回が増え、席の記録が `who=seat-tick-move what=/exit` の 1 行
/// 増え、梯子の記録が書かれないことを測る。
fn evacuate_assert_exit(place: &MovePlace) {
    let (keys, injections) = (move_keys(place).len(), move_injections(place).len());
    let out = move_run(place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), move_line("exit", "false", "-"), "判定行 1 行（busy / state-stale は出ない）");
    let sent: Vec<String> = move_keys(place).into_iter().skip(keys).collect();
    assert_eq!(sent, [format!("send-keys -t {TICK_TARGET} -l /exit"), format!("send-keys -t {TICK_TARGET} Enter")], "/exit 1 行");
    let lines = move_injections(place);
    assert_eq!(lines.len(), injections + 1, "記録は 1 送信 1 行: {lines:?}");
    let line = lines.last().map(String::as_str).unwrap_or_default();
    assert_eq!(move_who_what(line), (Some("seat-tick-move".to_owned()), Some("/exit".to_owned())), "{line}");
    assert!(!place.state.join("seat").join(TICK_SEAT).join("pointer-ladder").exists(), "梯子の記録は書かれない");
}

/// (k) 記録 ≠ row ∧ 前面 `claude` ∧ 最終行 Busy（新しい）∧ 入力欄が空 → `/exit` 1 行・`decision=move move=exit`（base では
/// `noop busy` ＝ RED）。
#[test]
fn seat_tick_evacuate_sends_the_exit_to_a_fresh_busy_seat() {
    let root = tmp();
    evacuate_assert_exit(&evacuate_place(&root, MOVE_B, 10));
}

/// (l) 同じ席で最終行の Busy が stale より古い → `/exit` 1 行（base では `state-stale` ＝ RED）。
#[test]
fn seat_tick_evacuate_sends_the_exit_to_a_stale_busy_seat() {
    let root = tmp();
    evacuate_assert_exit(&evacuate_place(&root, MOVE_B, TICK_STALE + 60));
}

/// (m) 記録 = row ∧ 最終行 Busy → `noop busy`・0 key（移動の周でない席は今のまま）。
#[test]
fn seat_tick_evacuate_leaves_a_busy_seat_whose_row_matches_the_record() {
    let root = tmp();
    move_assert_quiet(&evacuate_place(&root, MOVE_A, 10), &move_noop("busy"));
}

/// (n) 記録 ≠ row ∧ 最終行 Busy ∧ 入力欄に字 → `input-busy`・0 key（移動の門の入力欄の門のまま）。
#[test]
fn seat_tick_evacuate_refuses_typed_input() {
    let root = tmp();
    let place = evacuate_place(&root, MOVE_B, 10);
    fs::write(place.at(TICK_PANE), format!("{TICK_CLEAR_PANE}half typed")).ok();
    move_assert_quiet(&place, &move_noop("input-busy"));
}

/// (o) 2 周続けて撃つ → `/exit` が 2 行（積む・止めない）・記録も 2 行。
#[test]
fn seat_tick_evacuate_sends_the_exit_every_round() {
    let root = tmp();
    let place = evacuate_place(&root, MOVE_B, 10);
    evacuate_assert_exit(&place);
    evacuate_assert_exit(&place);
    let exits = move_keys(&place).iter().filter(|key| key.ends_with(" -l /exit")).count();
    assert_eq!((exits, move_injections(&place).len()), (2, 2), "/exit 2 行・記録 2 行");
}

// ───────── 合図を席ごとに止める（seat-heartbeat.md §12・契約表の行 o・`s2-07l.646`・接頭辞 `seat_heartbeat_`） ─────────
//
// §2 の tick の fixture（[`tick_place`] / [`tick_run`]）と §4 / §7 / §9 の移動・起こし・判定の fixture をそのまま使い、停止の記録を
// `seat heartbeat off|on` の口で置き消す。字面は契約から組む（実装の helper を使わない）。

/// 停止の記録の path（席の置き場の直下の `heartbeat-off`・契約の字面から組む）。
fn heartbeat_record(state: &Path) -> PathBuf {
    seat_dir_of(state, TICK_SEAT).join("heartbeat-off")
}

/// `seat heartbeat <switch>` を置き場 `state` の [`TICK_TARGET`] へ 1 回撃つ。
fn heartbeat_run(state: &Path, switch: &str) -> Output {
    let path = state.display().to_string();
    run_seat(&["heartbeat", switch, "--state-dir", &path, "--target", TICK_TARGET])
}

/// 撃って rc 0・stdout が `want`・stderr 0 byte を測る。
fn heartbeat_assert(state: &Path, switch: &str, want: &str) {
    let out = heartbeat_run(state, switch);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "heartbeat {switch}: rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), want, "heartbeat {switch}: 1 行");
    assert!(stderr_of(&out).is_empty(), "heartbeat {switch}: stderr 0 byte");
}

/// off / on の 1 行（契約の字面）。
fn heartbeat_line(switch: &str, word: &str) -> String {
    format!("seat heartbeat {switch}: target={TICK_TARGET} heartbeat={word}\n")
}

/// status の 1 行（行 p の前は `last=` 以下の 3 欄が `-`・契約の字面）。
fn heartbeat_status(word: &str) -> String {
    format!("seat heartbeat status: target={TICK_TARGET} heartbeat={word} last=- decision=- reason=-\n")
}

/// (a) off が停止の記録（1 行 `ts=<UTC 秒>`）を置き、黙った席への tick は `noop heartbeat-off pointer=- step=-`・0 key・梯子の記録 0。
/// 基準の無い梯子の記録が在る席でも記録は 1 byte も動かない（読まず書かない）（base では verb が無く、tick は注入 ＝ RED）。
#[test]
fn seat_heartbeat_off_silences_the_signal_to_a_silent_seat() {
    let place = tick_place(true);
    tick_silent_for(&place, TICK_STALE + 60);
    let before = unix_now();
    heartbeat_assert(&place.state, "off", &heartbeat_line("off", "off"));
    let after = unix_now();
    let record = fs::read_to_string(heartbeat_record(&place.state)).unwrap_or_default();
    let ts = record.strip_prefix("ts=").and_then(|rest| rest.strip_suffix('\n')).and_then(|secs| secs.parse::<u64>().ok());
    assert!(ts.is_some_and(|secs| (before..=after).contains(&secs)), "1 行 ts=<置いた時刻>: {record:?}");
    tick_assert_quiet(&place, &tick_noop("heartbeat-off", "-", "-"));
    assert!(tick_keys(&place).is_empty(), "0 key");
    assert!(!tick_ladder_path(&place).exists(), "梯子の記録 0");
    tick_ladder_put(&place, unix_now() - TICK_STALE - 120, 0, None);
    let ladder = fs::read(tick_ladder_path(&place)).unwrap_or_default();
    tick_assert_quiet(&place, &tick_noop("heartbeat-off", "-", "-"));
    assert_eq!(fs::read(tick_ladder_path(&place)).unwrap_or_default(), ladder, "settle も書かない（記録は 1 byte も動かない）");
}

/// (b) on が停止の記録を消し、次の tick は黙った席へ合図を注入する。
#[test]
fn seat_heartbeat_on_resumes_the_signal() {
    let place = tick_place(true);
    tick_silent_for(&place, TICK_STALE + 60);
    heartbeat_assert(&place.state, "off", &heartbeat_line("off", "off"));
    tick_assert_quiet(&place, &tick_noop("heartbeat-off", "-", "-"));
    heartbeat_assert(&place.state, "on", &heartbeat_line("on", "on"));
    assert!(!heartbeat_record(&place.state).exists(), "停止の記録は消える");
    let out = tick_run(&place, &[]);
    assert_eq!(stdout_of(&out), tick_inject(0), "on の後は注入: stderr={}", stderr_of(&out));
    assert_eq!(tick_keys(&place), [tick_text_key(0), format!("send-keys -t {TICK_TARGET} Enter")], "text 1 回 + Enter 1 回");
}

/// (c) 停止の記録が dir（在るのに読めない）の周も `heartbeat-off`・0 key（合図は正の証拠でだけ送る）。
#[test]
fn seat_heartbeat_record_that_is_a_dir_still_reads_off() {
    let place = tick_place(true);
    tick_silent_for(&place, TICK_STALE + 60);
    fs::create_dir_all(heartbeat_record(&place.state)).ok();
    tick_assert_quiet(&place, &tick_noop("heartbeat-off", "-", "-"));
    assert!(tick_keys(&place).is_empty() && !tick_ladder_path(&place).exists(), "0 key・梯子の記録 0");
}

/// (d-1) off の席でも移動の周の退避（`/exit` の text 1 回 + Enter 1 回）はそのまま撃つ。
#[test]
fn seat_heartbeat_off_still_evacuates_on_a_move_round() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    heartbeat_assert(&place.state, "off", &heartbeat_line("off", "off"));
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), move_line("exit", "false", "-"), "退避の判定行: stderr={}", stderr_of(&out));
    assert_eq!(
        move_keys(&place),
        [format!("send-keys -t {TICK_TARGET} -l /exit"), format!("send-keys -t {TICK_TARGET} Enter")],
        "/exit の text 1 回 + Enter 1 回"
    );
}

/// (d-2) off の席でも窓が shell なら起こし直し（起動行の末尾の `--resume <sid>` と初手の合図）はそのまま撃つ。
#[test]
fn seat_heartbeat_off_still_relaunches_a_dead_seat() {
    let root = tmp();
    let place = wake_place(&root, "/elsewhere", WAKE_SID);
    heartbeat_assert(&place.state, "off", &heartbeat_line("off", "off"));
    let text = wake_assert_launched(&place, MOVE_A, "/elsewhere");
    assert!(text.ends_with(&format!(" --resume {WAKE_SID} {}", wake_first_word())), "初手の合図も積む: {text}");
}

/// (e) off の席でも群の判定は撃ち（計測と打刻・`judged=stay`）、同じ周の合図の段だけが `heartbeat-off` で止まる。
#[test]
fn seat_heartbeat_off_still_judges_the_group() {
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [10, 10]);
    heartbeat_assert(&place.state, "off", &heartbeat_line("off", "off"));
    let before = unix_now();
    let out = move_run(&place);
    let after = unix_now();
    let want = format!("decision=noop target={TICK_SEAT} reason=heartbeat-off pointer=- step=- consumed=- move=- launched=- judged=stay\n");
    assert_eq!(stdout_of(&out), want, "判定は撃ち合図は止まる: stderr={}", stderr_of(&out));
    assert!(judge_calls(&place) > 0, "群の判定は計測する");
    assert!(judge_ts(&root).is_some_and(|ts| (before..=after).contains(&ts)), "打刻は判定の周の ts: {:?}", judge_ts(&root));
    assert!(move_keys(&place).is_empty(), "0 key");
}

/// (f) 登録 row の無い target は off / on / status とも rc 1・語 `no-row`・置き場に 1 file も作らない。
#[test]
fn seat_heartbeat_without_a_row_is_refused_and_writes_nothing() {
    let dir = tmp();
    let state = dir.join("state");
    fs::create_dir_all(&state).ok();
    for switch in ["off", "on", "status"] {
        let out = heartbeat_run(&state, switch);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{switch}: rc 1");
        assert!(stdout_of(&out).is_empty(), "{switch}: stdout 0 byte");
        assert_eq!(stderr_of(&out), format!("seat heartbeat {switch}: refused reason=no-row target={TICK_TARGET}\n"), "{switch}");
        assert_eq!(fs::read_dir(&state).map(Iterator::count).unwrap_or(usize::MAX), 0, "{switch}: 置き場に何も作らない");
    }
    let place = tick_place(false);
    assert_eq!(rc_of(&heartbeat_run(&place.state, "off")), i32::from(RC_REFUSED), "席の置き場が在っても row が無ければ断る");
    assert!(!heartbeat_record(&place.state).exists(), "停止の記録 0");
}

/// (i) status の 1 行が停止の記録の有無を映す（記録なしで `heartbeat=on`・off の後で `heartbeat=off`・行 p の前は 3 欄とも `-`）。
#[test]
fn seat_heartbeat_status_prints_one_line_for_the_record() {
    let place = tick_place(true);
    heartbeat_assert(&place.state, "status", &heartbeat_status("on"));
    heartbeat_assert(&place.state, "off", &heartbeat_line("off", "off"));
    heartbeat_assert(&place.state, "status", &heartbeat_status("off"));
    heartbeat_assert(&place.state, "on", &heartbeat_line("on", "on"));
    heartbeat_assert(&place.state, "status", &heartbeat_status("on"));
}

/// (j) off を 2 度撃つと 2 度目は記録の `ts=` の行を 1 byte も変えず rc 0（fixture の ts を先に書き、2 度目の後に同じ bytes を読む）。
#[test]
fn seat_heartbeat_off_twice_keeps_the_record_bytes() {
    let place = tick_place(true);
    heartbeat_assert(&place.state, "off", &heartbeat_line("off", "off"));
    let fixed = "ts=1000\n";
    fs::write(heartbeat_record(&place.state), fixed).ok();
    heartbeat_assert(&place.state, "off", &heartbeat_line("off", "off"));
    assert_eq!(fs::read_to_string(heartbeat_record(&place.state)).unwrap_or_default(), fixed, "ts は書き換えない");
}

/// (k) 記録の無い席への on は rc 0 で、file は無いまま。
#[test]
fn seat_heartbeat_on_without_a_record_is_a_no_op() {
    let place = tick_place(true);
    heartbeat_assert(&place.state, "on", &heartbeat_line("on", "on"));
    assert!(!heartbeat_record(&place.state).exists(), "file 無しのまま");
}

// ───── 最後の周の打刻と健全（seat-heartbeat.md §12・契約表の行 p・ADR-0070・`s2-07l.650`・接頭辞 `seat_tick_status_`） ─────
//
// §2 の tick の fixture（[`tick_place`] / [`tick_run`] / [`tick_rules_text`]＝周期 15）で tick を撃ち、席の置き場の直下の `tick-last` と
// `seat tick status` / doctor の席の行を読む。打刻の ts は過去に書いて経過を作る（偽の時計）。字面は契約から組む。

/// 最後の周の打刻の path（席の置き場の直下の `tick-last`・契約の字面から組む）。
fn status_last_path(place: &TickPlace) -> PathBuf {
    place.seat().join("tick-last")
}

/// 最後の周の打刻を `ts` で書く（1 行 `ts=<秒> decision=<語> reason=<語>`・契約の字面）。
fn status_last_put(place: &TickPlace, ts: u64) {
    fs::write(status_last_path(place), format!("ts={ts} decision=noop reason=stamp-recent\n")).ok();
}

/// 周期 15 の tick の行の全部の写し（`skip` の id の行を落とす）。
fn status_rules(place: &TickPlace, skip: &str) -> String {
    fixture(&place.dir, "status-rules.toml", &tick_rules_text(skip, None))
}

/// `seat tick status --state-dir S` を `extra` で 1 回撃つ。
fn status_run(place: &TickPlace, extra: &[&str]) -> Output {
    let state = place.state.display().to_string();
    let mut args = vec!["tick", "status", "--state-dir", state.as_str()];
    args.extend_from_slice(extra);
    run_seat(&args)
}

/// status の 1 行（契約の字面）。
fn status_line(last: &str, age: &str, healthy: &str, heartbeat: &str, ladder: (&str, &str)) -> String {
    let (step, next) = ladder;
    format!("seat tick status: target={TICK_TARGET} last={last} age={age} healthy={healthy} heartbeat={heartbeat} step={step} next={next}\n")
}

/// 経過 `ago` 秒の打刻を置いて status を撃ち（`--rules` は `rules`）、打刻の ts と stdout を返す。秒を跨いで経過がずれた周は
/// 撃ち直す（`age=` が `ago` に一致した周を返す・10 周で一致しなければ最後の周）。
fn status_at(place: &TickPlace, rules: &str, ago: u64) -> (u64, String) {
    let mut last = (0, String::new());
    for _ in 0..10 {
        let ts = unix_now() - ago;
        status_last_put(place, ts);
        let out = status_run(place, &["--rules", rules]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
        last = (ts, stdout_of(&out));
        if last.1.contains(&format!(" age={ago} ")) {
            break;
        }
    }
    last
}

/// doctor を置き場の `--state-dir` と `--rules`（`rules`）で撃ち、rc 0 と登録 row の行 1 本を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn status_doctor_row(place: &TickPlace, rules: &str) -> String {
    let state = place.state.display().to_string();
    let socket = place.at("no-such-sock").display().to_string();
    let out = Command::new(bin())
        .args(["doctor", "--state-dir", &state, "--tmux-socket", &socket, "--rules", rules])
        .env("PATH", &place.path)
        .output()
        .expect("binary を起動できる");
    assert_eq!(rc_of(&out), i32::from(RC_OK), "doctor は rc を変えない: stderr={}", stderr_of(&out));
    let rows: Vec<String> = stdout_of(&out).lines().filter(|line| line.starts_with("seat: ")).map(str::to_owned).collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    rows.concat()
}

/// 経過 `ago` 秒の打刻を置いて doctor を撃ち、席の行の `tick=` の語を返す（秒を跨いだ周は撃ち直す・10 周まで）。
fn status_doctor_at(place: &TickPlace, rules: &str, ago: u64) -> Option<String> {
    let mut word = None;
    for _ in 0..10 {
        let before = unix_now();
        status_last_put(place, before - ago);
        word = tick_token(&status_doctor_row(place, rules), "tick");
        if unix_now() == before {
            break;
        }
    }
    word
}

/// (h) tick の 1 周の後に `tick-last` が 1 行在り、ts は撃った時刻・decision / reason は判定行と同じ（noop と inject の 2 周）
/// （base では file 無し ＝ RED）。
#[test]
fn seat_tick_status_tick_last_mirrors_the_judgement_line() {
    let place = tick_place(true);
    for ago in [100, TICK_STALE + 60] {
        tick_silent_for(&place, ago);
        let before = unix_now();
        let out = tick_run(&place, &[]);
        let after = unix_now();
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
        let line = stdout_of(&out);
        let (decision, reason) = (tick_token(&line, "decision").unwrap_or_default(), tick_token(&line, "reason").unwrap_or_default());
        let text = fs::read_to_string(status_last_path(&place)).unwrap_or_default();
        let ts = text.strip_prefix("ts=").and_then(|rest| rest.split(' ').next()).and_then(|secs| secs.parse::<u64>().ok());
        assert!(ts.is_some_and(|secs| (before..=after).contains(&secs)), "ts は撃った時刻: {text:?}");
        let ts = ts.unwrap_or_default();
        assert_eq!(text, format!("ts={ts} decision={decision} reason={reason}\n"), "判定行と同じ 2 語: {line}");
    }
    let text = fs::read_to_string(status_last_path(&place)).unwrap_or_default();
    assert!(text.ends_with(" decision=inject reason=-\n"), "注入の周は reason=-: {text:?}");
    assert!(!place.seat().join("tick-last.tmp").exists(), "一時 file は残らない");
}

/// (i) 周期の行を欠く写しの no-rule（rc 1）の周も打刻を書く。
#[test]
fn seat_tick_status_no_rule_round_still_stamps() {
    let place = tick_place(true);
    tick_silent_for(&place, 100);
    let rules = status_rules(&place, "seat.tick_interval_s");
    let out = tick_run(&place, &["--rules", &rules]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "no-rule は rc 1");
    let text = fs::read_to_string(status_last_path(&place)).unwrap_or_default();
    assert!(text.starts_with("ts=") && text.ends_with(" decision=error reason=no-rule\n"), "rc 1 の周も書く: {text:?}");
}

/// (j) 登録 row の無い target の tick は打刻を書かず、席の置き場の無い周は dir も作らない。status の `--target` に row が無ければ
/// rc 1・語 `no-row`・stdout 0 行。
#[test]
fn seat_tick_status_without_a_row_writes_no_stamp() {
    let bare = tick_place(false);
    tick_silent_for(&bare, 100);
    assert_eq!(rc_of(&tick_run(&bare, &[])), i32::from(RC_OK));
    assert!(!status_last_path(&bare).exists(), "row の無い席に打刻を書かない");
    fs::remove_dir_all(bare.seat()).ok();
    assert_eq!(rc_of(&tick_run(&bare, &[])), i32::from(RC_OK));
    assert!(!bare.seat().exists(), "席の置き場を作らない");
    let place = tick_place(true);
    let rules = status_rules(&place, "");
    let out = status_run(&place, &["--target", "zz:zz", "--rules", &rules]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "row の無い target は rc 1");
    assert!(stdout_of(&out).is_empty(), "stdout 0 行");
    assert_eq!(stderr_of(&out), "seat tick status: refused reason=no-row target=zz:zz\n");
}

/// (k) healthy は経過 ≤ 2 × 周期（周期 15）: 30 秒ちょうどは yes・31 秒は no・16 秒は yes、打刻が無ければ no と `last=- age=-`。
/// doctor の `tick=` も同じ 3 点で healthy / stale / healthy・無ければ absent（係数 1 と `<` の変異を別々の assert が落とす）。
#[test]
fn seat_tick_status_healthy_is_within_twice_the_interval() {
    let place = tick_place(true);
    let rules = status_rules(&place, "");
    let out = status_run(&place, &["--rules", &rules]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_OK), status_line("-", "-", "no", "on", ("-", "-"))), "打刻なし");
    assert_eq!(tick_token(&status_doctor_row(&place, &rules), "tick").as_deref(), Some("absent"), "doctor も打刻なし");
    for (ago, healthy, word) in [(30, "yes", "healthy"), (31, "no", "stale"), (16, "yes", "healthy")] {
        let (ts, text) = status_at(&place, &rules, ago);
        assert_eq!(text, status_line(&ts.to_string(), &ago.to_string(), healthy, "on", ("-", "-")), "経過 {ago} 秒");
        assert_eq!(status_doctor_at(&place, &rules, ago).as_deref(), Some(word), "doctor の経過 {ago} 秒");
    }
    fs::write(status_last_path(&place), "ts=x\n").ok();
    let out = status_run(&place, &["--rules", &rules, "--target", TICK_TARGET]);
    assert_eq!(stdout_of(&out), status_line("-", "-", "no", "on", ("-", "-")), "読めない打刻は no");
    assert_eq!(tick_token(&status_doctor_row(&place, &rules), "tick").as_deref(), Some("unreadable"), "doctor は unreadable");
}

/// (l) `heartbeat=` は停止の記録を映す（status と doctor の席の行）。
#[test]
fn seat_tick_status_heartbeat_reflects_the_off_record() {
    let place = tick_place(true);
    let rules = status_rules(&place, "");
    let heartbeat = |want: &str| {
        let out = status_run(&place, &["--rules", &rules]);
        assert_eq!(tick_token(&stdout_of(&out), "heartbeat").as_deref(), Some(want), "status: {}", stdout_of(&out));
        assert_eq!(tick_token(&status_doctor_row(&place, &rules), "heartbeat").as_deref(), Some(want), "doctor");
    };
    heartbeat("on");
    heartbeat_assert(&place.state, "off", &heartbeat_line("off", "off"));
    heartbeat("off");
    heartbeat_assert(&place.state, "on", &heartbeat_line("on", "on"));
    heartbeat("on");
}

/// (m) step / next は梯子の記録から: step は記録の段・next は次の段（段 + 1）の待ち − 経過（段 0 の記録なら段 1 の待ちで段 0 の待ち
/// ではない）・待ちを過ぎた記録は 0・次の段が列を越える記録（段 5）は `stopped`。
#[test]
fn seat_tick_status_step_and_next_follow_the_ladder_record() {
    let place = tick_place(true);
    let rules = status_rules(&place, "");
    let ladder = |step: u32, ago: u64| {
        let before = unix_now();
        tick_ladder_put(&place, before - ago, step, Some(1));
        let out = status_run(&place, &["--rules", &rules]);
        let spent = unix_now() - before;
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
        let text = stdout_of(&out);
        (tick_token(&text, "step"), tick_token(&text, "next").and_then(|next| next.parse::<u64>().ok()), spent, text)
    };
    let (step, next, spent, text) = ladder(0, 100);
    assert_eq!(step.as_deref(), Some("0"), "{text}");
    let want = TICK_LADDER[1] - 100;
    assert!(next.is_some_and(|secs| (want - spent..=want).contains(&secs)), "段 1 の待ち − 経過: {text}");
    let (step, next, _, text) = ladder(2, TICK_LADDER[3] + 10);
    assert_eq!((step.as_deref(), next), (Some("2"), Some(0)), "待ちを過ぎた記録: {text}");
    let (step, _, _, text) = ladder(5, 100);
    assert_eq!((step.as_deref(), tick_token(&text, "next").as_deref()), (Some("5"), Some("stopped")), "列を越える次の段: {text}");
}

/// (o) tick の 1 周の後の `seat heartbeat status` は `last=` / `decision=` / `reason=` を tick-last と同じ値で出す
/// （base では 3 欄とも `-` ＝ RED）。
#[test]
fn seat_tick_status_heartbeat_status_carries_the_tick_last() {
    let place = tick_place(true);
    tick_silent_for(&place, 100);
    assert_eq!(rc_of(&tick_run(&place, &[])), i32::from(RC_OK));
    let text = fs::read_to_string(status_last_path(&place)).unwrap_or_default();
    let ts = tick_token(&text, "ts").unwrap_or_default();
    assert!(!ts.is_empty(), "打刻が在る: {text:?}");
    let want = format!("seat heartbeat status: target={TICK_TARGET} heartbeat=on last={ts} decision=noop reason=stamp-recent\n");
    heartbeat_assert(&place.state, "status", &want);
}

/// (p) 周期の行を欠く写しで status は rc 1・語 `no-rule`・stdout 0 行、doctor の `tick=` は `tick-unit=` と同じ no-rule の語（rc 0）。
#[test]
fn seat_tick_status_missing_interval_row_is_no_rule() {
    let place = tick_place(true);
    status_last_put(&place, unix_now());
    let rules = status_rules(&place, "seat.tick_interval_s");
    let out = status_run(&place, &["--rules", &rules]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "rc 1");
    assert!(stdout_of(&out).is_empty(), "stdout 0 行: {}", stdout_of(&out));
    assert_eq!(stderr_of(&out), "seat tick status: refused reason=no-rule\n");
    assert_eq!(tick_token(&status_doctor_row(&place, &rules), "tick").as_deref(), Some("no-rule:missing"), "doctor の語");
}

/// (q-1) 梯子の記録が dir の席は `step=unreadable next=unreadable` で他の欄は不変・rc 0。
#[test]
fn seat_tick_status_ladder_record_dir_is_unreadable() {
    let place = tick_place(true);
    let rules = status_rules(&place, "");
    fs::create_dir_all(tick_ladder_path(&place)).ok();
    let out = status_run(&place, &["--rules", &rules]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "報告であって判定でない: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), status_line("-", "-", "no", "on", ("unreadable", "unreadable")));
}

/// (q-2) 梯子の行を欠く写しで status は rc 1・語 `no-rule`・stdout 0 行、doctor の `tick=` は梯子の行を読まず healthy のまま。
#[test]
fn seat_tick_status_missing_ladder_row_is_no_rule_and_doctor_keeps_its_word() {
    let place = tick_place(true);
    let rules = status_rules(&place, "seat.pointer_ladder_s");
    let out = status_run(&place, &["--rules", &rules]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), String::new()), "rc 1・stdout 0 行");
    assert_eq!(stderr_of(&out), "seat tick status: refused reason=no-rule\n");
    assert_eq!(status_doctor_at(&place, &rules, 0).as_deref(), Some("healthy"), "doctor は周期の行だけ読む");
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
    assert_eq!(unit_bodies(&place), unit_expected(&place, None, 15), "導出の bytes（周期は埋め込みの seat.tick_interval_s = 15）");
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
    let changed = unit_expected(&place, None, 15)[0].replacen("oneshot", "oneshoT", 1);
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

// ─────────── doctor は host の面の `[[tick]]` を既定にする（seat-heartbeat.md §5 形 3・契約表の行 d・接頭辞 `seat_doctor_tick_`） ───────────
//
// §3 の unit の置き場（[`unit_place`]）の host の面に `[[tick]]` を書き、doctor を flag 無し・flag 付きで撃つ。

/// 置き場の host の面に `[[tick]]`（unit dir = 置き場の unit dir・binary = `binary`）だけを書く。
fn doctor_tick_face(place: &UnitPlace, binary: &str) {
    let body = format!("schema = 1\n\n[[tick]]\nunit-dir = \"{}\"\nbinary = \"{binary}\"\n", place.units.display());
    fs::write(place.tick.state.join(vessel::rules::HOST_MANIFEST), body).ok();
}

/// doctor の host の面の行（`host-manifest=` 始まり）。
fn doctor_tick_host_lines(out: &Output) -> Vec<String> {
    stdout_of(out).lines().filter(|line| line.starts_with("host-manifest=")).map(str::to_owned).collect()
}

/// doctor を撃ち、rc 0 と登録 row の行 1 本の末尾の `tick-unit=` の語（`want`）と host の面の行（`host`）を測る。
fn doctor_tick_assert(place: &UnitPlace, extra: &[&str], want: &str, host: &str) {
    let out = unit_doctor(place, extra);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "{extra:?}: stderr={}", stderr_of(&out));
    let rows = unit_doctor_rows(&out);
    assert_eq!(rows.len(), 1, "{extra:?}: {rows:?}");
    assert!(rows.iter().all(|row| row.ends_with(&format!(" tick-unit={want}"))), "{extra:?}: {want}: {rows:?}");
    assert_eq!(rows.iter().map(|row| row.matches("tick-unit=").count()).sum::<usize>(), 1, "{extra:?}: 1 語だけ: {rows:?}");
    assert_eq!(doctor_tick_host_lines(&out), [host.to_owned()], "{extra:?}: host の面の行");
}

/// (面が既定) `--unit-dir` / `--binary` が無く面に `[[tick]]` が在る周は、面の値で登録 row の行に `tick-unit=` を足す（install 前は
/// absent・install 後は present）。host の面の行は `host-manifest=present tick=declared run-accounts=0`（口座 0 の面）。base は `[[tick]]` を未知の表として読めず
/// host の面の行が `unreadable`・`tick-unit=` の項目が無い（RED）。
#[test]
fn seat_doctor_tick_face_values_default_the_probe_without_flags() {
    let place = unit_place(true);
    doctor_tick_face(&place, &place.binary);
    doctor_tick_assert(&place, &[], "absent", "host-manifest=present tick=declared run-accounts=0");
    assert_eq!(rc_of(&unit_run(&place, "install", &[])), i32::from(RC_OK));
    doctor_tick_assert(&place, &[], "present", "host-manifest=present tick=declared run-accounts=0");
    fs::remove_dir_all(&place.tick.dir).ok();
}

/// (flag が勝つ) 面の binary（`/elsewhere/bin`）と違う binary で install した unit は、flag 無しでは面の値で導出し直して `foreign`、
/// install と同じ `--unit-dir` / `--binary` を渡した周は flag の値で `present`。
#[test]
fn seat_doctor_tick_flags_win_over_the_face_values() {
    let place = unit_place(true);
    doctor_tick_face(&place, "/elsewhere/bin");
    assert_eq!(rc_of(&unit_run(&place, "install", &[])), i32::from(RC_OK));
    doctor_tick_assert(&place, &[], "foreign", "host-manifest=present tick=declared run-accounts=0");
    let units = place.units.display().to_string();
    doctor_tick_assert(&place, &["--unit-dir", &units, "--binary", &place.binary], "present", "host-manifest=present tick=declared run-accounts=0");
    fs::remove_dir_all(&place.tick.dir).ok();
}

/// (面にも flag にも無い) host の面が無い周と `[[tick]]` の無い面の周は、登録 row の行に `tick-unit=` を足さず、host の面の行は
/// 従来の字面のまま（`tick=` の項目が無い＝既存の外形は動かない・面の在る周の末尾は §26 の `run-accounts=` だけ）。
#[test]
fn seat_doctor_tick_without_face_or_flags_adds_nothing() {
    let place = unit_place(true);
    for (body, host) in
        [(None, "host-manifest=absent"), (Some("schema = 1\n\n[[account]]\nlabel = \"h1\"\n"), "host-manifest=present run-accounts=1")]
    {
        if let Some(text) = body {
            fs::write(place.tick.state.join(vessel::rules::HOST_MANIFEST), text).ok();
        }
        let out = unit_doctor(&place, &[]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{host}: stderr={}", stderr_of(&out));
        assert!(!stdout_of(&out).contains("tick-unit="), "{host}: {}", stdout_of(&out));
        let text = stdout_of(&out);
        assert!(text.lines().filter(|line| !line.starts_with("seat: ")).all(|line| !line.contains("tick=")), "{host}: 席の行の外に tick= は無い: {text}");
        assert_eq!(doctor_tick_host_lines(&out), [host.to_owned()], "host の面の行は従来の字面");
        assert_eq!(unit_doctor_rows(&out).len(), 1, "登録 row の行は在る");
    }
    fs::remove_dir_all(&place.tick.dir).ok();
}

// ─────────── doctor が便用の口座の数を出す（account-lifecycle.md §26・契約表の行 o・接頭辞 `seat_doctor_run_accounts_`） ───────────
//
// §17 の host.toml の fixture（口座と群を host の面に宣言・tracked の面は口座 0）で doctor を撃ち、host の面の行を測る。

/// 置き場の host の面に口座 `accounts` と、`group` が空でなければ群 `Tier1`（置き場 `/repo`・候補 `group`）を宣言する。
fn run_accounts_face(place: &RolePlace, accounts: &[&str], group: &[&str]) {
    let quoted: Vec<String> = group.iter().map(|label| format!("\"{label}\"")).collect();
    let table = if group.is_empty() {
        String::new()
    } else {
        format!("\n[[account-group]]\nname = \"Tier1\"\nanchors = [\"/repo\"]\naccounts = [{}]\n", quoted.join(", "))
    };
    fs::create_dir_all(&place.state).ok();
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), format!("{}{table}", account_rules(accounts))).ok();
}

/// doctor を撃ち（rc 0 は [`doctor_rows`] が測る）、host の面の行を返す（無ければ空）。
fn run_accounts_host(place: &RolePlace) -> String {
    doctor_rows(place, NO_ACCOUNT_RULES).into_iter().find(|line| line.starts_with("host-manifest=")).unwrap_or_default()
}

/// (宣言 3・群 1) 群の今の口座（記録なし＝種 `r1`）だけが外れ、host の行の末尾に `run-accounts=2`。base は欄が無い（RED）。
/// event log を読めない周は `unreadable`（0 に潰さない・C11）。
#[test]
fn seat_doctor_run_accounts_subtracts_the_group_current_account() {
    let place = role_place();
    run_accounts_face(&place, &["r1", "r2", "r3"], &["r1", "r2"]);
    assert_eq!(run_accounts_host(&place), "host-manifest=present run-accounts=2");
    let log = vessel::fleet::store::events_path(&place.state);
    fs::create_dir_all(log.parent().unwrap_or(&place.state)).expect("log の dir を作れる");
    fs::write(&log, "not an event\n").expect("log を壊せる");
    assert_eq!(run_accounts_host(&place), "host-manifest=present run-accounts=unreadable");
    fs::remove_dir_all(&place.dir).ok();
}

/// (退役 1) 群の外の `r3` を `account retire` で退役させると、有効な口座が 2 に減って `run-accounts=1`。
#[test]
fn seat_doctor_run_accounts_drops_a_retired_account() {
    let place = role_place();
    run_accounts_face(&place, &["r1", "r2", "r3"], &["r1", "r2"]);
    fs::create_dir_all(place.state.join("accounts").join("r3")).expect("口座の dir を作れる");
    let state = place.state.display().to_string();
    let out = Command::new(bin()).args(["account", "retire", "r3", "--state-dir", &state]).output().expect("binary を起動できる");
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(run_accounts_host(&place), "host-manifest=present run-accounts=1");
    fs::remove_dir_all(&place.dir).ok();
}

/// (群 0) 群を宣言しない host は何も外れず、有効な口座の全部 `run-accounts=3`。
#[test]
fn seat_doctor_run_accounts_counts_every_account_without_groups() {
    let place = role_place();
    run_accounts_face(&place, &["r1", "r2", "r3"], &[]);
    assert_eq!(run_accounts_host(&place), "host-manifest=present run-accounts=3");
    fs::remove_dir_all(&place.dir).ok();
}

/// (0 は判定しない) 宣言 1・群 1（有効な口座の全部が群の今の口座）は `run-accounts=0` で、rc と stderr と他の行は `2` の周
/// （宣言 3・同じ群）と 1 字も変わらない（比べるのは口座の数を名乗る行〔口座の行・host-guard の `wired=`〕の外・共通の口座
/// `r1` の行は同じ字面）。
#[test]
fn seat_doctor_run_accounts_zero_changes_no_rc_or_other_line() {
    let place = role_place();
    run_accounts_face(&place, &["r1", "r2", "r3"], &["r1"]);
    let two = role_doctor_rules(&place, NO_ACCOUNT_RULES);
    run_accounts_face(&place, &["r1"], &["r1"]);
    let zero = role_doctor_rules(&place, NO_ACCOUNT_RULES);
    assert_eq!((rc_of(&zero), rc_of(&two)), (i32::from(RC_OK), i32::from(RC_OK)), "stderr={}", stderr_of(&zero));
    assert_eq!(stderr_of(&zero), stderr_of(&two), "0 でも断りも警告も足さない");
    let (two, zero): (Vec<String>, Vec<String>) =
        (stdout_of(&two).lines().map(str::to_owned).collect(), stdout_of(&zero).lines().map(str::to_owned).collect());
    let host = |lines: &[String]| lines.iter().filter(|line| line.starts_with("host-manifest=")).cloned().collect::<Vec<String>>();
    assert_eq!(host(&two), ["host-manifest=present run-accounts=2"], "{two:?}");
    assert_eq!(host(&zero), ["host-manifest=present run-accounts=0"], "{zero:?}");
    let rest = |lines: &[String]| -> Vec<String> {
        lines
            .iter()
            .filter(|line| {
                !line.starts_with("host-manifest=")
                    && !(line.starts_with("account=") && !line.starts_with("account=r1 "))
                    && !line.starts_with(HOST_GUARD_HEAD)
            })
            .cloned()
            .collect()
    };
    assert_eq!(rest(&zero), rest(&two), "host の行の値の外は 1 行も動かない");
    assert_eq!(zero.len() + 2, two.len(), "行の数の差は口座の行 2 本だけ: {zero:?} / {two:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (面が無い / 読めない) host の面が absent・unreadable の周は欄を足さない（host の行は従来の字面・`run-accounts=` はどこにも無い）。
#[test]
fn seat_doctor_run_accounts_absent_or_unreadable_face_adds_no_field() {
    let place = role_place();
    fs::create_dir_all(&place.state).expect("置き場を作れる");
    let lines = doctor_rows(&place, NO_ACCOUNT_RULES);
    assert!(lines.contains(&HOST_ABSENT.to_owned()), "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("run-accounts=")), "{lines:?}");
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), "schema = 1\n\n[[account]]\nlabel = \"r1\"\nbogus = 1\n").expect("面を壊せる");
    let lines = doctor_rows(&place, NO_ACCOUNT_RULES);
    assert!(lines.contains(&"host-manifest=unreadable".to_owned()), "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("run-accounts=")), "{lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}
