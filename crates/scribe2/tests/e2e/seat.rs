//! 席の歯の module root（設計 docs/design/seat-roles.md・seat-autonomy.md §3・working-memory.md §8・
//! account-lifecycle.md §8）。
//!
//! 歯は題ごとの submodule に置く（`s2-07l.261`・契約の write-set が題の file 単位で交差しないため・接頭辞ごとの
//! 固定した組は seat-roles.md §7・`s2-07l.361`）: `wm`（退避 / 消費 / 復元）・`tick`（tick / heartbeat / meter /
//! 証拠）・`cycle`（inject / cycle / 終了 / 立て直しの shell の門）・`account`（口座の退避と立て直し / hook 集合の
//! 食い違い / doctor の口座の行）・`launch`（起動 / 復元の第 2 手 / Enter 落ちの修復）・`register`（状態 / 役割 /
//! 登録）・`rules`（host の面 / 壊れた `--rules`）。この file には **2 つ以上の submodule が使う共有 helper と
//! fixture**・外形 snapshot の歯（面ごとに 1 本＝`seat_usage_external_form` / `seat_rebrief_external_form` /
//! `seat_doctor_external_form`・`s2-07l.327`・snapshot 名が module path を含むので動かさない）・変異生存の検出線の
//! 歯（`mutant_e2e_*`）だけを残す。
//!
//! tmux は **独立 socket**（`-S <tmp>/sock -f /dev/null`）の server だけを撃ち、開発席の
//! live な server には 1 度も触れない。pane の読みは `--capture-file` で本文を直に渡す
//! ので、meter 側の歯は tmux を 1 度も起動しない。
// flip-check: moved s2-07l.261
// flip-check: moved s2-07l.361

mod account;
mod cycle;
mod launch;
mod register;
mod rules;
mod tick;
mod wm;

use self::wm::rebrief_forms;
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
use vessel::seat::cycle::pace_of;
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

// 外形 snapshot は**面ごとに 1 本**（usage / rebrief の DATA / doctor の末尾・`s2-07l.327`・seat-roles.md §7）。
// 1 本に連結すると seat 面の契約が全部この 1 file で交差する（実測 2026-09-15: 4 便が互いに当たり同時に
// 出せるのが 2 便）。面を触る契約だけがその面の file に当たる形にする（pipe の外形と同じ割り方）。

/// `seat` の使い方（usage 1 行・全 subcommand）を snapshot に固定する（C12.5）。
#[test]
fn seat_usage_external_form() {
    let form = stderr_of(&run_seat(&[]));
    insta::assert_snapshot!(form);
}

/// rebrief の DATA の 3 形（found / candidate / missing の marker の並び）を snapshot に固定する（C12.5）。
#[test]
fn seat_rebrief_external_form() {
    let mut form = String::new();
    for out in rebrief_forms() {
        form.push_str(&stdout_of(&out));
    }
    insta::assert_snapshot!(form);
}

/// `doctor --state-dir` の突合の項目（tmux を撃てない周の形）・その直後の host の面の行・末尾の導入先の行
/// （登録 row の anchor・記録なし・consumer-sync.md §4）を snapshot に固定する（C12.5）。
#[test]
fn seat_doctor_external_form() {
    let mut form = String::new();
    let place = role_doctor_place();
    for line in stdout_of(&role_doctor(&place)).lines().skip_while(|line| !line.starts_with("seats: ")) {
        form.push_str(line);
        form.push('\n');
    }
    fs::remove_dir_all(&place.dir).ok();
    insta::assert_snapshot!(form);
}

// ─────────────────── heartbeat / tick / cycle の共有 fixture ───────────────────

/// 入力欄が空の pane（**idle に見える字面**。busy / idle は打刻で与える＝字面は判定に効かない）。
const IDLE_PANE: &str = "❯ \n  10% 100k/1M Opus 5\n";
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
/// `seat.tick_stale_s` の宣言値（`rules/manifest.toml`）。歯はこの値の**両側**を撃つ。
const STALE_S: u64 = 2400;
/// 判定行の context の列（fixture の statusline は 10% / 19%）。
const CTX_10: &str = " context=10";

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
/// 歯が `--rules` で渡す**短い**確認上限（秒）。宣言値（[`SETTLE_S`]）では確認できない周の歯が
/// 1 本 30 秒かかり、suite の壁時計の大半をそれが占めていた（実測 2026-09-12: 10 本で 300 s）。
/// 分岐は同じで、縮むのは待ちだけである（`s2-07l.151`）。
const FAST_SETTLE_S: u64 = 2;
/// 同上（周期・ミリ秒）。
const FAST_POLL_MS: u64 = 100;

/// cycle の確認の刻みだけを宣言する manifest の字面（`--rules` に渡す fixture）。
///
/// 他の行は持たない＝`seat` の他の閾値（`seat.tick_stale_s` / `seat.cycle_lock_ttl_s` /
/// `seat.context_*`）は埋め込みのまま引かれる（読み口が別・[`vessel::seat::cycle::pace_of`] だけが
/// この file を読む）。`enabled` を引数に取るのは、不発効の行を「無い」と同じに倒す側の歯（負例）を
/// 同じ字面の組で作るためである。
fn pace_manifest(settle_s: u64, poll_ms: u64, enabled: bool) -> String {
    format!(
        "schema = 1\n\n\
         [[rule]]\nid = \"seat.cycle_settle_s\"\nkind = \"SeatCycleSettleS\"\nvalue = {settle_s}\n\
         enabled = {enabled}\nruling = \"user 2026-09-12T12:08Z\"\nruled_at = \"2026-09-12\"\n\n\
         [[rule]]\nid = \"seat.cycle_poll_ms\"\nkind = \"SeatCyclePollMs\"\nvalue = {poll_ms}\n\
         enabled = {enabled}\nruling = \"user 2026-09-12T12:08Z\"\nruled_at = \"2026-09-12\"\n"
    )
}

/// 上の fixture を `dir/rules.toml` へ書き、`--rules` に渡す path を返す。
fn pace_rules(dir: &Path, settle_s: u64, poll_ms: u64) -> String {
    fixture(dir, "rules.toml", &pace_manifest(settle_s, poll_ms, true))
}

/// 歯の既定の刻み（[`FAST_SETTLE_S`] / [`FAST_POLL_MS`]）を書いて path を返す。
fn fast_rules(dir: &Path) -> String {
    pace_rules(dir, FAST_SETTLE_S, FAST_POLL_MS)
}

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

/// 判定行の `key=value` を 1 つ取り出す（末尾の改行は落とす）。
fn tick_token(line: &str, key: &str) -> Option<String> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
        .map(str::to_owned)
}

/// cycle を撃ち、`/clear` が席の log（`<dir>/seat.log`）に着いた**後**で pane の写し
/// （`--capture-file`）を `after` へ差し替えてから結果を待つ。写しは作り直しの確認が周期ごとに
/// 読み直すので、「送る前は idle・送った後にこの形」の pane を 1 本の file で再現できる。
/// socket は [`socket_of`]・log は偽の席と同じ path から導く（引数上限・憲法 C4）。
///
/// 確認の刻みは `--rules` の fixture（上限 `settle_s` 秒・周期 [`FAST_POLL_MS`]）で渡す: 確認できない
/// 周の歯は宣言値だと 1 本 30 秒待つ（`s2-07l.151`）。**待つ長さを縮めるだけで分岐は変えない**ので、
/// 窓の内に証拠が来る側の歯（遅れて打刻する席）は sleep より長い上限を渡す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn cycle_with_pane_after_clear(
    dir: &Path,
    name: &str,
    before: &str,
    after: &str,
    settle_s: u64,
) -> Output {
    let (socket, log) = (socket_of(dir), dir.join("seat.log"));
    let (state, wm, pane) = (dir.join("state"), dir.join("wm"), dir.join("pane.txt"));
    stamp_idle(&state, name);
    fs::write(&pane, before).expect("pane fixture を置ける");
    let (wm_s, state_s, pane_s, rules_s) = (
        wm.display().to_string(),
        state.display().to_string(),
        pane.display().to_string(),
        pace_rules(dir, settle_s, FAST_POLL_MS),
    );
    let child = Command::new(bin())
        .args([
            "seat", "cycle", "--target", name, "--wm-dir", &wm_s, "--tmux-socket", &socket,
            "--state-dir", &state_s, "--capture-file", &pane_s, "--rules", &rules_s,
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

/// 記録 1 行の flat JSON から `key` の値を引く（無ければ `None`）。
fn json_value(line: &str, key: &str) -> Option<vessel::fleet::json_lite::Value> {
    vessel::fleet::json_lite::parse_object(line)
        .ok()?
        .into_iter()
        .find(|(found, _)| found == key)
        .map(|(_, value)| value)
}

/// wm dir と state dir の全 entry の (path, size, mtime)（dir の mtime も含む＝lock の作成を捕まえる）。
fn tree_stat(root: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path.clone());
            }
            found.push((path, meta.len(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        }
    }
    found.sort();
    found
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

/// 登録 row の口座（埋め込み manifest の `[[account]]` の 1 つ目・宣言値）。
const ACCT_SEAT: &str = "a1";
/// 立て直しの候補（埋め込みの 2 つ目）。
const ACCT_SPARE: &str = "a2";
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
    dir: PathBuf,
    /// `--state-dir`。
    state: PathBuf,
    /// `--wm-dir`（空で在る＝走査が 0 件と確かめられる）。
    wm: PathBuf,
    /// 独立 socket。
    socket: String,
    /// `--rules` の写しに宣言する `[[account]]` の label 列（既定は tracked の manifest と同じ a1〜a5）。
    labels: &'static [&'static str],
}

/// tracked の manifest の `[[account]]`（`--rules` の写しの既定＝埋め込みと同じ宣言）。
const ACCT_LABELS: &[&str] = &["a1", "a2", "a3", "a4", "a5"];

/// 置き場を 1 つ作る（登録 row の anchor の dir も実在させる＝起動行の `cd` が通る）。
fn acct_place() -> AcctPlace {
    let dir = tmp();
    let state = dir.join("state");
    let wm = dir.join("wm");
    fs::create_dir_all(&wm).ok();
    fs::create_dir_all(dir.join(ACCT_ANCHOR_DIR)).ok();
    let socket = socket_of(&dir);
    AcctPlace { dir, state, wm, socket, labels: ACCT_LABELS }
}

/// 登録 row の anchor（置き場の配下の実在する dir・絶対 path）。
fn acct_anchor(place: &AcctPlace) -> String {
    place.dir.join(ACCT_ANCHOR_DIR).display().to_string()
}

/// host の写しの形の `--rules`（歯の刻み・計測の上限・`labels` の `[[account]]`）を書いて path を返す。
fn acct_rules(dir: &Path, labels: &[&str]) -> String {
    let accounts: String = labels.iter().map(|label| format!("\n[[account]]\nlabel = \"{label}\"\n")).collect();
    let body = format!(
        "{}\n[[rule]]\nid = \"fleet.usage_timeout_s\"\nkind = \"UsageTimeoutS\"\nvalue = 30\n\
         enabled = true\nruling = \"user 2026-09-12T02:01Z\"\nruled_at = \"2026-09-12\"\n{accounts}",
        pace_manifest(FAST_SETTLE_S, FAST_POLL_MS, true)
    );
    fixture(dir, "rules.toml", &body)
}

/// 席を planner として `seat register` の口で登録する（口座は [`ACCT_SEAT`]）。`launch` は起動の雛形の本文。
fn acct_register(place: &AcctPlace, target: &str, launch: &str) -> Output {
    acct_register_as(place, target, ACCT_SEAT, launch)
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
            account: None,
        };
        vessel::fleet::store::append(state, &event, policy).expect("実測行を積める");
    }
}

/// いまの UTC（実測行の時刻の字面）。
fn acct_now() -> String {
    vessel::fleet::cli::now_utc()
}

/// tick の引数（確認の刻みは短い fixture・`pane` が在れば判定はその file・送信は独立 socket）。
fn acct_args(place: &AcctPlace, target: &str, pane: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "tick".to_owned(),
        "--target".to_owned(),
        target.to_owned(),
        "--wm-dir".to_owned(),
        place.wm.display().to_string(),
        "--tmux-socket".to_owned(),
        place.socket.clone(),
        "--state-dir".to_owned(),
        place.state.display().to_string(),
        "--rules".to_owned(),
        acct_rules(&place.dir, place.labels),
    ];
    if let Some(found) = pane {
        args.extend(["--capture-file".to_owned(), found.to_owned()]);
    }
    args
}

/// tick を 1 回撃つ。
fn acct_tick(place: &AcctPlace, target: &str, pane: Option<&str>) -> Output {
    let args = acct_args(place, target, pane);
    run_seat(&args.iter().map(String::as_str).collect::<Vec<&str>>())
}

/// PATH の先頭に「呼ばれたら印を残して失敗する tmux」を置いて tick を 1 回撃つ（tmux に触れたかを返す）。
fn acct_tick_probed(place: &AcctPlace, target: &str, pane: &str) -> (Output, bool) {
    let args = acct_args(place, target, Some(pane));
    run_seat_probed(&place.dir, &args.iter().map(String::as_str).collect::<Vec<&str>>())
}

/// `--capture-file` の判定行（pane は [`IDLE_PANE`]＝context=10）。
fn acct_line(head: &str, tail: &str, state: &Path) -> String {
    format!("seat: tick {head}{CTX_10}{tail}{}\n", provenance(state, "flag"))
}

/// 記録 1 行の文字列の値。
fn acct_text(line: &str, key: &str) -> Option<String> {
    json_value(line, key).and_then(|value| value.as_str().map(str::to_owned))
}

/// 席の記録（`tick.jsonl`）のうち `seat inject` の経路が積んだ行の `what`（送った順）。
fn acct_injected(state: &Path, target: &str) -> Vec<String> {
    fs::read_to_string(tick_file(state, target))
        .unwrap_or_default()
        .lines()
        .filter(|line| acct_text(line, "who").as_deref() == Some("seat-inject"))
        .filter_map(|line| acct_text(line, "what"))
        .collect()
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

/// `<state>/accounts/<label>/.credentials.json` に期限の遠い読める credential を置く（`fleet usage` の読み先）。
fn acct_credential(place: &AcctPlace, label: &str) {
    let dir = place.state.join("accounts").join(label);
    fs::create_dir_all(&dir).ok();
    fs::write(
        dir.join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"tok-acct","expiresAt":4102444800000}}"#,
    )
    .ok();
}

/// PATH の先頭（[`run_seat_probed`] の shim の dir）に置く偽 curl（`fleet_usage_` の歯と同じ seam＝計測の子
/// process）。argv を `curl-args` へ追記で写し、5 時間窓 `pct` の本文と status 200 を返す。
fn acct_fake_curl(place: &AcctPlace, pct: u64) {
    let bin = place.dir.join("bin");
    fs::create_dir_all(&bin).ok();
    let body = format!(
        r#"{{"five_hour":{{"utilization":{pct}.0,"resets_at":"{ACCT_RESET}"}},"seven_day":{{"utilization":1.0,"resets_at":"{ACCT_RESET}"}},"limits":[]}}"#
    );
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\ncat > /dev/null\nprintf '%s\\n200' '{body}'\n",
        place.dir.join("curl-args").display()
    );
    let curl = bin.join("curl");
    fs::write(&curl, script).ok();
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).ok();
}

/// 偽 curl が呼ばれた回数（口座 1 つにつき `--max-time` が 1 回）。
fn acct_curl_calls(place: &AcctPlace) -> usize {
    fs::read_to_string(place.dir.join("curl-args"))
        .unwrap_or_default()
        .lines()
        .filter(|arg| *arg == "--max-time")
        .count()
}

/// 立て直しの歯の起動 script（雛形 `sh <dir>/l.sh {account_dir}` で起こす偽の session）を書き、雛形を返す。
///
/// script は渡された credential dir を `launched` に写し、prompt を描いて hook の代わりに `SessionStart` を打ち、
/// 以後は受けた行を `seat.log` に積んで `UserPromptSubmit` → `Stop` を打つ（復元の消費の証拠）。前面は `sh` の
/// ままなので、立て直した後の周に入口 (3) が立ち続けても (1) が崩れる形を測れる。
fn acct_launcher(place: &AcctPlace, target: &str) -> String {
    format!("sh {} {{account_dir}}", acct_launch_script(place, target, "\"$1\""))
}

/// [`acct_launcher`] の偽の session の script を書き、path を返す。起動時に `words`（sh の語の並び）を 1 語 1 行で
/// `launched` へ写す（雛形の穴を引数で受ける形は `"$1"`・env で受ける形は `"$CLAUDE_CONFIG_DIR"` など）。
fn acct_launch_script(place: &AcctPlace, target: &str, words: &str) -> String {
    let file = state_file(&seat_dir_of(&place.state, target));
    let script = format!(
        "printf '%s\\n' {words} >> '{launched}'\nprintf '\u{276f} '\n{start}\n\
         while read -r line; do printf '%s\\n' \"$line\" >> '{log}'; {busy}; {stop}; printf '\u{276f} '; done\n",
        launched = place.dir.join("launched").display(),
        log = place.dir.join("seat.log").display(),
        start = stamp_cmd(&file, "idle", "SessionStart", FakeStamp::Now),
        busy = stamp_cmd(&file, "busy", "UserPromptSubmit", FakeStamp::Now),
        stop = stamp_cmd(&file, "idle", "Stop", FakeStamp::Now),
    );
    fixture(&place.dir, "l.sh", &script)
}

/// 退避の合図を実物の tick で 1 回注入させる（打刻 Busy・口座が閾値以上の fixture が前提）。判定行を返す。
fn acct_signal(place: &AcctPlace, target: &str) -> String {
    write_state(&seat_dir_of(&place.state, target), StateFix::Busy { age_s: 0 });
    stdout_of(&acct_tick(place, target, None))
}

/// 席が止まった打刻（`Stop`・時刻 `ts`）を打刻 file の末尾に足す。
fn acct_stop(place: &AcctPlace, target: &str, ts: u64) {
    let file = state_file(&seat_dir_of(&place.state, target));
    let mut text = fs::read_to_string(&file).unwrap_or_default();
    text.push_str(&stamp_line("idle", "Stop", ts, ACCT_SID));
    text.push('\n');
    fs::write(&file, text).ok();
}

/// 退避して止まった席の fixture（登録 row の口座 a1 = 100・候補 a2 = `spare`・実物の tick の退避の合図 → その後の
/// `Stop`）＝立て直しの入口の (1)(2)。(3) は呼び側が立てる席（shell か否か）で決まる。
fn acct_parked(place: &AcctPlace, target: &str, launch: &str, spare: u64) {
    let registered = acct_register(place, target, launch);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 100, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, spare, &acct_now());
    let first = acct_signal(place, target);
    assert_eq!(tick_token(&first, "kind").as_deref(), Some("externalize"), "1 周目は退避の合図: {first}");
    acct_stop(place, target, unix_now().saturating_add(1));
}

/// 立て直しと起動の起動行の前置（`s2-07l.324`・account-lifecycle.md §4）: `cd '<row の anchor>' && ` が agent view の env より
/// **前**に来る＝`cd … && CLAUDE_CODE_DISABLE_AGENT_VIEW=1 <tail>`。`tail` は前置の後の字面の先頭（雛形か `CLAUDE_CONFIG_DIR=`）。
fn acct_launch_prefix(anchor: &str, tail: &str) -> String {
    format!("cd '{anchor}' && CLAUDE_CODE_DISABLE_AGENT_VIEW=1 {tail}")
}

/// 立て直した周の注入: 雛形の穴が選んだ口座の credential dir で埋まって起動が走り、その後に立ち上がった席が復元の
/// command を受けた（席の記録でも 退避の合図 → 起動 → 復元 の順）。起動行は row の anchor への `cd` → agent view off →
/// 雛形の順（`s2-07l.324`）。
fn acct_assert_launched_then_restored(place: &AcctPlace, target: &str) {
    let spare_dir = place.state.join("accounts").join(ACCT_SPARE);
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("{}\n", spare_dir.display()),
        "穴は選んだ口座の credential dir で埋まる"
    );
    assert_eq!(
        fs::read_to_string(place.dir.join("seat.log")).unwrap_or_default(),
        "/rebrief\n",
        "立ち上がった席が復元の command を受けた（起動の後）"
    );
    let sent = acct_sent(&place.state, target);
    assert_eq!(sent.len(), 3, "退避の合図・起動・復元の 3 行: {sent:?}");
    assert!(
        sent.get(1).is_some_and(|what| what.starts_with(&acct_launch_prefix(&acct_anchor(place), "sh "))),
        "2 行目は row の anchor への cd と agent view off を前置した起動の雛形: {sent:?}"
    );
    assert_eq!(sent.get(2).map(String::as_str), Some("/rebrief"), "3 行目は復元: {sent:?}");
}

/// 立て直した周の登録 row: `SeatRegistered` が 1 件増え、口座だけが選んだ口座に変わる（他の項目は既存 row から
/// 写す）。注入の前に cycle-stamp を打つ（再注入の back-off）。
fn acct_assert_relabelled(place: &AcctPlace, target: &str) {
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "SeatRegistered が 1 件増える: {rows:?}");
    assert_eq!(rows.first().map(|row| row.account.as_str()), Some(ACCT_SEAT));
    let copied = rows.first().map(|row| vessel::fleet::Registration { account: ACCT_SPARE.to_owned(), ..row.clone() });
    assert_eq!(rows.last().cloned(), copied, "口座だけが変わり target / sid / launch / anchor / model は既存 row から写す");
    assert!(seat_dir_of(&place.state, target).join("cycle-stamp").exists(), "立て直しの注入の前に cycle-stamp を打つ");
}

/// 席が終わった後の shell の prompt（`❯` を持たない・user の bash prompt の形）。
const ACCT_SHELL_PS1: &str = "user@host:dir$ ";

/// 席が終わって shell へ戻った pane を作る: prompt を [`ACCT_SHELL_PS1`] に替えて画面を消し、`extra`（printf の書式）を
/// 描いてから prompt を待つ。前面 process は `sh` のまま（入口 (3) は立つ）。描けたかを返す（panic しない）。
fn acct_shell_prompt(place: &AcctPlace, target: &str, extra: &str) -> bool {
    let line = format!("PS1='{ACCT_SHELL_PS1}'; printf '\\033[H\\033[2J{extra}'");
    if !tmux(&place.socket, &["send-keys", "-t", target, "-l", &line]).status.success()
        || !tmux(&place.socket, &["send-keys", "-t", target, "Enter"]).status.success()
    {
        return false;
    }
    acct_wait_pane(place, target, |pane| pane.trim_end().ends_with('$') && !pane.contains("PS1="))
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
    pub(super) dir: PathBuf,
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

/// anchor の dir（登録 row の `anchor`・起動行の 1 つ目の `--plugin-dir`）。
fn launch_anchor(place: &AcctPlace) -> String {
    place.dir.join("anchor").display().to_string()
}

/// 期待する偽 claude の記録（argv を 1 語 1 行・続けて env の 2 行）。
fn launch_expected_argv(place: &AcctPlace, label: &str) -> String {
    let account_dir = place.state.join("accounts").join(label).display().to_string();
    format!(
        "--plugin-dir\n{}\n--plugin-dir\n{}\n--plugin-dir\n{}\n{}\n{}\nenv:CLAUDE_CONFIG_DIR={account_dir}\nenv:CLAUDE_CODE_DISABLE_AGENT_VIEW=1\n",
        launch_anchor(place), LAUNCH_PLUGINS[0], LAUNCH_PLUGINS[1], LAUNCH_ARGS[0], LAUNCH_ARGS[1]
    )
}

/// test 自身の PATH に在る tmux（包みが exec する実体）。
fn real_tmux() -> Option<PathBuf> {
    std::env::var("PATH").ok()?.split(':').map(|dir| Path::new(dir).join("tmux")).find(|path| path.is_file())
}

/// shim の dir を作る（偽 `claude`・argv を写して実体へ exec する `tmux` の包み）: 偽 claude は argv と env を `launched` へ
/// 写し、その時点の event log を [`LAUNCH_EVENTS_SEEN`] へ複製し、prompt を描いて `SessionStart` を打ち、以後は受けた行を
/// [`LAUNCH_LOG`] に積んで `UserPromptSubmit` → `Stop` を打つ（立て直しの偽 session と同じ形）。PATH の字面を返す。
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
        start = stamp_cmd(&file, "idle", "SessionStart", FakeStamp::Now),
        log = place.dir.join(LAUNCH_LOG).display(),
        busy = stamp_cmd(&file, "busy", "UserPromptSubmit", FakeStamp::Now),
        stop = stamp_cmd(&file, "idle", "Stop", FakeStamp::Now),
    );
    let tmux = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec '{}' \"$@\"\n",
        place.dir.join(LAUNCH_TMUX_ARGS).display(),
        real_tmux().unwrap_or_default().display()
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
