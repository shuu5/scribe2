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

/// 席の自律機能の口は**もう無い**（ADR-0045 §2 (2)・`s2-07l.479.1`）: 管理 tick・作り直しの cycle・
/// 打刻の heartbeat・context の計測 meter は口を持たない第 1 token として断られ（[`assert_gone_mouth`]）、
/// 使い方の 1 行にもその名が出ない。残る口（register / launch / inject）は従来どおり使い方に在る。
///
/// **消えたことを測る歯**である（base では 4 つとも自分の口として動くので RED）。
#[test]
fn seat_autonomy_subcommands_are_gone_from_the_usage() {
    let (dir, state) = gone_mouth_place();
    let usage = stderr_of(&run_seat(&[]));
    for gone in ["meter", "heartbeat", "tick", "cycle"] {
        assert_gone_mouth(&state, gone, &[]);
        assert!(!usage.contains(&format!("|{gone} ")), "{gone} は使い方に出ない: {usage}");
    }
    for kept in ["register", "launch"] {
        assert!(usage.contains(&format!("{kept} --")), "{kept} は使い方に残る: {usage}");
    }
    fs::remove_dir_all(&dir).ok();
}

/// 注入の口は**もう無い**（ADR-0045 §2 (2)・`s2-07l.479.3`）: `seat inject` は口を持たない第 1 token として
/// 断られ（[`assert_gone_mouth`]）、使い方の 1 行に口の名も `--text` / `--file` の flag も出ない。
/// 送達の機構そのものは残る（`seat launch` の復元が使う・ADR-0045 §2 (4) の不変の面）ので、
/// **消えたのは口だけ**である＝残る口 2 つは従来どおり使い方に在る。
///
/// **消えたことを測る歯**である（base では `seat inject --target s:w --text x` が自分の口として動くので RED）。
#[test]
fn seat_inject_subcommand_is_gone_from_the_usage() {
    let (dir, state) = gone_mouth_place();
    let usage = stderr_of(&run_seat(&[]));
    assert_gone_mouth(&state, "inject", &["--text", "x"]);
    fs::remove_dir_all(&dir).ok();
    for gone in ["inject --target", "--text", "--file"] {
        assert!(!usage.contains(gone), "{gone} は使い方に出ない: {usage}");
    }
    for kept in ["register --", "launch --"] {
        assert!(usage.contains(kept), "{kept} は使い方に残る: {usage}");
    }
}

/// 作業記憶の口は**もう無い**（ADR-0045 §2 (2)・`s2-07l.479.2`）: 退避（externalize）・復元（rebrief）・
/// 消費（consume）は口を持たない第 1 token として断られ（[`assert_gone_mouth`]）、使い方の 1 行にも
/// その名が出ない。`--rules` を受ける席の口も 0 になる（この 3 口が最後だった）。
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
    fs::remove_dir_all(&dir).ok();
    assert!(!usage.contains("--wm-dir"), "退避物の置き場の flag も残らない: {usage}");
    assert!(!usage.contains("--rules"), "席の口は `--rules` を 1 つも受けない: {usage}");
    for kept in ["register", "launch"] {
        assert!(usage.contains(&format!("{kept} --")), "{kept} は使い方に残る: {usage}");
    }
}

/// `seat` の使い方（usage 1 行・全 subcommand）を snapshot に固定する（C12.5）。
#[test]
fn seat_usage_external_form() {
    let form = stderr_of(&run_seat(&[]));
    insta::assert_snapshot!(form);
}

/// `seat` の既知の verb の閉じた enum（設計 contract-source.md §17 の形 (vii)）: const slice の件数（既知の verb の本数・
/// dispatch の腕の本数ではない）と宣言順が型と一致し、`as_str` と `parse` が往復し、各語は usage に載る。未知の token
/// （口座 label・空・flag・variant 名）は `parse` が `None`＝dispatch の label の腕へ落ちる側。
#[test]
fn seat_command_all_known_verbs_round_trip_and_unknown_tokens_are_none() {
    use vessel::seat::cli::{SeatCommand, SEAT_COMMANDS};
    assert_eq!(vessel::seat::cli::SEAT_COMMANDS.len(), 2, "記録時点の既知の verb: {SEAT_COMMANDS:?}");
    assert!(vessel::order::is_declaration_order(SEAT_COMMANDS, |command| command as usize), "宣言順: {SEAT_COMMANDS:?}");
    let words: Vec<&str> = SEAT_COMMANDS.iter().map(|command| command.as_str()).collect();
    assert_eq!(words, ["register", "launch"], "字面の閉じた列（宣言順）");
    let usage = vessel::seat::cli::usage();
    for command in SEAT_COMMANDS {
        assert_eq!(SeatCommand::parse(command.as_str()), Some(*command), "as_str ↔ parse の往復: {command:?}");
        assert!(usage.contains(&format!("{} --", command.as_str())), "{} は usage に載る: {usage}", command.as_str());
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
    assert_eq!(lines.len(), 6, "2 行 + 登録 row 1 行 + 突合 1 行 + host の面 1 行 + 導入先 1 行: {lines:?}");
    assert_eq!(
        lines.get(3..),
        Some(&["seats: registered=1 live=1 missing=0".to_owned(), HOST_ABSENT.to_owned(), CONSUMER_REPO.to_owned()][..]),
        "席の立つ socket"
    );
    let elsewhere = place.dir.join("no-server-sock").display().to_string();
    let away = doctor(&["--state-dir", &state, "--tmux-socket", &elsewhere, "--rules", &rules]);
    assert_eq!(away.as_ref().map(rc_of), Some(i32::from(RC_OK)), "{away:?}");
    let away_lines: Vec<String> = away.map(|out| stdout_of(&out)).unwrap_or_default().lines().map(str::to_owned).collect();
    assert_eq!(
        away_lines.get(3..),
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
    "consumer=/repo source=launch scope=- binary=unrecorded plugin=unrecorded ledger=- cache=absent head=undeclared drift=unrecorded";

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
