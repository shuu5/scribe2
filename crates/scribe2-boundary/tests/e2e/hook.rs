//! `.vessel` marker と hook の歯（設計 docs/design/vessel-hook.md §7）。
//!
//! tmp の git repo を `git init` + commit で作り、`vessel init --state-dir` で置き場を
//! 紐づけてから hook を撃つ。commit には identity が要るので **repo local** の
//! `user.name` / `user.email` を与える（global 設定は 1 byte も触らない）。
//!
//! 歯は族ごとの子 module に置く（設計 docs/design/carry-prep.md §9 行 g・`s2-07l.679`）: `guards`・`session`・`group`・
//! `vessel_cli`。この file には共有の helper と const・外形 snapshot の歯・tmux の群の歯・他の族の歯だけを残す。

mod group;
mod guards;
mod session;
mod vessel_cli;
// flip-check: moved s2-07l.679

use crate::{make_tmp_dir, TmpDir};
use crate::seat::{socket_of, start_seat, tmux, IsolatedSeat};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
use vessel::fleet::{json_lite, Registration};
use vessel::hook::vessel::digest::{self, PluginRecord};
use vessel::hook::precompact::{self, Slot, TEXT_WIDTH};
use vessel::hook::vessel::{Marker, GENERATION, MARKER};
use vessel::hook::{guard, inject_path, SCHEMA};
use vessel::name::{BUILD_COMMIT, NAME, PLUGIN_DIR};
use vessel::pipe::declaration::DECL_FILE;
use vessel::seat::brief;
use vessel::seat::recent::{self, Kind, Unmeasured, BEAD_LIMIT, COMMIT_LIMIT, DIRTY_SCAN_LIMIT, TITLE_WIDTH, WINDOW_SECS};
use vessel::seat::role::{Capability, Role};

/// binary の path。
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_scribe2")
}

/// tmp dir を 1 つ作り、symlink を解いた path を返す。
///
/// `git rev-parse --show-toplevel` は実体 path を返すので、比較する側も解いておく。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn tmp() -> TmpDir {
    let dir = make_tmp_dir().expect("tmp dir を作れる");
    dir.canonical().expect("tmp dir の実体 path を解ける")
}

/// git を 1 回撃ち、rc 0 を要求して stdout を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git を起動できる");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    out.status
        .success()
        .then_some(text)
        .expect("git が rc 0 で終わる")
}

/// commit を 1 つ持つ tmp の git repo を作る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn git_repo() -> TmpDir {
    let dir = tmp();
    git(&dir, &["init", "-q"]);
    git(&dir, &["config", "user.name", "e2e"]);
    git(&dir, &["config", "user.email", "e2e@example.invalid"]);
    fs::create_dir_all(dir.join("src")).expect("src dir を作れる");
    fs::write(dir.join("src").join("lib.rs"), "// seed\n").expect("seed を書ける");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "seed"]);
    dir
}

/// `vessel` を binary で 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_vessel(args: &[&str]) -> Output {
    Command::new(bin())
        .arg("vessel")
        .args(args)
        .output()
        .expect("binary を起動できる")
}

/// `hook` を binary で 1 回撃つ。payload は stdin へ流す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_hook_args(args: &[&str], payload: &str) -> Output {
    let mut child = Command::new(bin())
        .arg("hook")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary を起動できる");
    child
        .stdin
        .as_mut()
        .expect("stdin を開ける")
        .write_all(payload.as_bytes())
        .expect("payload を書ける");
    child.wait_with_output().expect("終了を待てる")
}

/// `hook <event>` を撃つ（flag なし）。
fn run_hook(event: &str, payload: &str) -> Output {
    run_hook_args(&[event], payload)
}

/// stderr の行数。
fn stderr_lines(out: &Output) -> usize {
    String::from_utf8_lossy(&out.stderr).lines().count()
}

/// stderr の全文。
fn stderr_text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// `cwd` だけを持つ payload。
fn payload(cwd: &Path) -> String {
    format!("{{\"cwd\":\"{}\"}}", cwd.display())
}

/// tool 名と編集先を持つ payload（`tool_input` は入れ子）。
fn tool_payload(cwd: &Path, tool: &str, file: &str) -> String {
    format!(
        "{{\"cwd\":\"{}\",\"tool_name\":\"{tool}\",\"tool_input\":{{\"file_path\":\"{file}\"}}}}",
        cwd.display()
    )
}

/// repo を器へ紐づけ、置き場の path を返す。
fn linked(repo: &Path) -> TmpDir {
    let state = tmp();
    let out = run_vessel(&[
        "init",
        "--state-dir",
        &state.display().to_string(),
        &repo.display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "vessel init は rc 0");
    state
}

/// policy file（write-set）を書き、その path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_policy(repo: &Path, body: &str) -> PathBuf {
    let git_dir = PathBuf::from(git(repo, &["rev-parse", "--absolute-git-dir"]));
    let path = guard::policy_path(&git_dir);
    let parent = path.parent().expect("policy の親 dir が在る");
    fs::create_dir_all(parent).expect("policy の dir を作れる");
    fs::write(&path, body).expect("policy を書ける");
    path
}

/// inject.jsonl の行を読む。file が無ければ空。
fn inject_lines(state: &Path) -> Vec<String> {
    match fs::read_to_string(inject_path(state)) {
        Err(_) => Vec::new(),
        Ok(text) => text
            .lines()
            .map(str::to_owned)
            .filter(|line| !line.is_empty())
            .collect(),
    }
}

/// 1 行の flat JSON から文字列でない値を含めて 1 組を引く。
fn value_of(line: &str, key: &str) -> Option<json_lite::Value> {
    let pairs = json_lite::parse_object(line).ok()?;
    pairs
        .into_iter()
        .find(|(found, _)| found == key)
        .map(|(_, value)| value)
}

/// 黙る周であること（stdout も stderr も 0 byte・rc 0）。
fn assert_silent(out: &Output, why: &str) {
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{why}: rc 0");
    assert!(out.stdout.is_empty(), "{why}: stdout 0 byte");
    assert!(out.stderr.is_empty(), "{why}: stderr 0 byte");
}

/// 後片付け（tmp を残さない）。
fn clean(dirs: &[&Path]) {
    for dir in dirs {
        fs::remove_dir_all(dir).ok();
    }
}

// ─────────────── Bash の command guard（`s2-07l.168`・ADR-0025 §2.2・設計 vessel-hook.md §5・接頭辞 `hook_command_`） ───────────────
//
// rules 行 `runner.denied_commands` の語列に当たる Bash を実行の時点で止める。席の弁別はしない（`--pane` 無しの runner
// にも同じ判定）ので、偽 tmux は要らない。

/// 記録のうち command guard の行（`what` が `command-deny` で始まる）。
fn command_records(state: &Path) -> Vec<String> {
    inject_lines(state).into_iter().filter(|line| what_of(line).starts_with("command-deny")).collect()
}

/// deny の外形（rc 2・stdout 0 byte・stderr 1 行）を見て、stderr が当たった語列の出所の rules 行 id と語列を名指すことを
/// 確かめる（git の語列は host_guard.git・cargo の語列は runner.denied_commands＝設計 vessel-hook.md §11 の形 f 4）。
fn assert_command_deny(out: &Output, sequence: &str, row: &str, why: &str) -> String {
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{why}: deny は rc 2: {}", stderr_text(out));
    assert!(out.stdout.is_empty(), "{why}: deny でも stdout は 0 byte");
    assert_eq!(stderr_lines(out), 1, "{why}: deny の stderr は 1 行: {}", stderr_text(out));
    let text = stderr_text(out);
    assert!(text.starts_with(&format!("{NAME}: deny ")), "{why}: 器が名乗る: {text}");
    assert!(text.contains(&format!("rules 行 {row} が禁じる")), "{why}: rules 行 id {row} を名指す: {text}");
    assert!(text.contains(sequence), "{why}: 当たった語列 {sequence:?} を名指す: {text}");
    text
}

// ─────────────── host の破壊防止の見張り（`s2-07l.574`・設計 vessel-hook.md §11 行 b・ADR-0056・接頭辞 `host_guard_kind_`） ───────────────
//
// 口座の設定から呼ばれる subcommand `host-guard` を binary で撃つ。marker と anchor に依らない（hook の入口の沈黙を持ち込まない）
// ので、判定の歯は repo を器へ紐づけない。置き場は `--state-dir` の tmp だけ。

/// `host-guard` を binary で 1 回撃つ。payload は stdin へ流す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_host_guard(args: &[&str], payload: &str) -> Output {
    let mut child = Command::new(bin())
        .arg("host-guard")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary を起動できる");
    child
        .stdin
        .as_mut()
        .expect("stdin を開ける")
        .write_all(payload.as_bytes())
        .expect("payload を書ける");
    child.wait_with_output().expect("終了を待てる")
}

/// `--state-dir <state>` だけを付けて撃つ（埋め込みの rules）。
fn run_host_guard_in(state: &Path, payload: &str) -> Output {
    run_host_guard(&["--state-dir", &state.display().to_string()], payload)
}

/// 記録のうち host-guard の行（`who` が `host-guard`）。
fn host_guard_records(state: &Path) -> Vec<String> {
    inject_lines(state)
        .into_iter()
        .filter(|line| value_of(line, "who") == Some(json_lite::Value::Str("host-guard".to_owned())))
        .collect()
}

/// host-guard の deny の外形（rc 2・stdout 0 byte・stderr ちょうど 1 行・器の名乗り）を見て stderr を返す。
fn assert_host_guard_deny(out: &Output, why: &str) -> String {
    let text = stderr_text(out);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{why}: deny は rc 2: {text}");
    assert!(out.stdout.is_empty(), "{why}: deny でも stdout は 0 byte");
    assert_eq!(stderr_lines(out), 1, "{why}: stderr は 1 行: {text}");
    assert!(text.starts_with(&format!("{NAME}: host-guard deny kind=")), "{why}: 器が名乗る: {text}");
    text
}

/// fail-closed の deny（`hit=<理由>`）と、置き場が在る周の記録 1 行（`what = host-guard-deny reason=<理由>`）を確かめる。
fn assert_host_guard_fail_closed(state: &Path, out: &Output, reason: &str) {
    let text = assert_host_guard_deny(out, reason);
    assert!(text.contains(&format!(" kind=- hit={reason} row=- ruling=- — ")), "{reason}: {text}");
    let lines = host_guard_records(state);
    assert_eq!(lines.len(), 1, "{reason}: 記録 1 行: {lines:?}");
    assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), format!("host-guard-deny reason={reason}"), "{reason}");
}

// ─────────────── host-guard の rm の種類（`s2-07l.575`・設計 vessel-hook.md §11 行 c・接頭辞 `host_guard_rm_`） ───────────────
//
// 埋め込みの rules（host_guard.rm は 3 記号）で binary を撃つ。repo は commit 済みの tmp（`src/lib.rs` が tracked）。

/// rm の deny の 1 行（kind=rm・hit・行 id・裁定 id・代わりの経路）と記録 1 行を確かめる。
fn assert_rm_deny(state: &Path, out: &Output, hit: &str) {
    use vessel::hook::host_guard::Kind;
    let text = assert_host_guard_deny(out, hit);
    let want = format!(
        "{NAME}: host-guard deny kind=rm hit={hit} row=host_guard.rm ruling={HOST_GUARD_RULING} — {}",
        Kind::Rm.route()
    );
    assert_eq!(text.trim_end(), want, "5 欄の 1 行");
    assert_eq!(what_of(&host_guard_records(state).last().cloned().unwrap_or_default()), "host-guard-deny rm", "{hit}");
}

// ─────────────── host-guard の台帳の形（`s2-07l.568`・設計 vessel-hook.md §11 行 f・接頭辞 `host_guard_ledger_`） ───────────────
//
// 埋め込みの rules（ledger.denied_writes は 4 形）で binary を撃つ。台帳を持つ repo は root に `.beads` の dir と `scripts/bdw`
// の file を置いた tmp の git repo。

// ─────────────── host-guard の見張り自身の設定（`s2-07l.577`・設計 vessel-hook.md §12 行 e・接頭辞 `host_guard_self_`） ───────────────
//
// 埋め込みの rules で binary を撃つ。置き場の host.toml が口座 a を宣言し、a の settings.json は別の tmp の実体への symlink。

/// 口座 a を宣言した置き場と、a の settings.json の symlink が指す実体の file を作る（戻りは置き場・実体の dir・実体の path）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn self_state() -> (TmpDir, TmpDir, PathBuf) {
    let state = tmp();
    let shared = tmp();
    let real = shared.join("settings.json");
    fs::write(&real, "{}\n").expect("実体を書ける");
    fs::write(state.join("host.toml"), "schema = 1\n\n[[account]]\nlabel = \"a\"\n").expect("host.toml を書ける");
    let account = state.join("accounts").join("a");
    fs::create_dir_all(&account).expect("口座の dir を作れる");
    std::os::unix::fs::symlink(&real, account.join("settings.json")).expect("symlink を作れる");
    fs::write(account.join("other.json"), "{}\n").expect("別 file を書ける");
    (state, shared, real)
}

// ─────────────── 起票の門（`s2-07l.517`・設計 ledger-form.md §3 の 9 / §6 行 d・接頭辞 `hook_memo_guard_`） ───────────────
//
// `bd` / `bdw` の create を読み、memo の create に 4 節の本文を、契約の create に label `intake:memo` の不在を要求する。
// 席の弁別はしない（`--pane` 無しでも同じ判定）ので、偽 tmux は要らない。

/// memo の 4 節が揃った本文。
const MEMO_BODY: &str = "## memo\n### 出所\n- run: r\n### 観測\n- x\n### 候補\n### 昇格条件\n- 引き金: 再発 1\n";

/// 記録のうち起票の門の行（`what` が `ledger-deny` で始まる）。
fn ledger_records(state: &Path) -> Vec<String> {
    inject_lines(state).into_iter().filter(|line| what_of(line).starts_with("ledger-deny")).collect()
}

/// 起票の門の deny の外形（rc 2・stdout 0 byte・stderr 1 行・理由の 1 語）と記録 1 行（`what = ledger-deny <理由>`）を
/// 確かめ、stderr を返す。
fn assert_ledger_deny(state: &Path, repo: &Path, command: &str, reason: &str) -> String {
    let before = ledger_records(state).len();
    let out = run_hook("pre-tool-use", &bash_payload(repo, command));
    let text = stderr_text(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{command}: deny は rc 2: {text}");
    assert!(out.stdout.is_empty(), "{command}: deny でも stdout は 0 byte");
    assert_eq!(stderr_lines(&out), 1, "{command}: stderr は 1 行: {text}");
    assert!(text.starts_with(&format!("{NAME}: deny bd create ")), "{command}: 器が名乗る: {text}");
    assert!(text.contains(&format!("reason={reason}（")), "{command}: 閉じた理由 {reason}: {text}");
    let lines = ledger_records(state);
    assert_eq!(lines.len(), before + 1, "{command}: 記録は 1 行増える: {lines:?}");
    assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), format!("ledger-deny {reason}"), "{command}");
    text
}

/// 通る周（0 byte・rc 0・記録も増えない）。
fn assert_ledger_pass(state: &Path, repo: &Path, command: &str) {
    let before = inject_lines(state).len();
    let out = run_hook("pre-tool-use", &bash_payload(repo, command));
    assert_silent(&out, command);
    assert_eq!(inject_lines(state).len(), before, "{command}: 通す周は記録を残さない");
}

// ─────────────── 台帳 write の 4 形（`s2-07l.169`・設計 vessel-hook.md §10・接頭辞 `hook_ledger_write_`） ───────────────
//
// memo の判定で止まらない `bd` / `bdw` の segment を、rules 行 `ledger.denied_writes` の値に載る 4 形に掛ける。

/// 台帳 write の形の deny の外形（rc 2・stdout 0 byte・stderr 1 行・理由の 1 語）と記録 1 行（`what = ledger-deny <理由>`）。
fn assert_write_deny(state: &Path, out: &Output, command: &str, reason: &str) {
    let text = stderr_text(out);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{command}: deny は rc 2: {text}");
    assert!(out.stdout.is_empty(), "{command}: deny でも stdout は 0 byte");
    assert_eq!(stderr_lines(out), 1, "{command}: stderr は 1 行: {text}");
    assert!(text.starts_with(&format!("{NAME}: deny 台帳の write は起票の門が止める reason={reason}（")), "{command}: {text}");
    assert!(text.contains(" — "), "{command}: 次の一手を持つ: {text}");
    let lines = ledger_records(state);
    assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), format!("ledger-deny {reason}"), "{command}");
}

/// 埋め込みの rules で撃ち、形の deny と記録がちょうど 1 行増えることを確かめる。
fn assert_write_denied(state: &Path, repo: &Path, command: &str, reason: &str) {
    let before = ledger_records(state).len();
    let out = run_hook("pre-tool-use", &bash_payload(repo, command));
    assert_write_deny(state, &out, command, reason);
    assert_eq!(ledger_records(state).len(), before + 1, "{command}: 記録は 1 行増える");
}

#[test]
fn hook_is_silent_for_unknown_event_and_outside_repo() {
    let repo = git_repo();
    let state = linked(&repo);
    let out = run_hook("pre-compact", &payload(&repo));
    assert_silent(&out, "未知 event（他の器と衝突しない）");

    let bare = tmp();
    let out = run_hook("session-start", &payload(&bare));
    assert_silent(&out, "git repo でない dir");
    clean(&[&repo, &state, &bare]);
}

#[test]
fn hook_records_who_when_and_file_name() {
    let repo = git_repo();
    let state = linked(&repo);
    run_hook("session-start", &payload(&repo));
    // 置き場の file 名は C6.3 の「消費を記録する store 1 つ」の所在ゆえ字面で pin する。
    assert!(
        state.join("inject.jsonl").exists(),
        "記録は <state_dir>/inject.jsonl に在る"
    );
    let lines = inject_lines(&state);
    let line = lines.first().map_or_else(String::new, Clone::clone);
    assert_eq!(
        value_of(&line, "who"),
        Some(json_lite::Value::Str("hook:session-start".to_owned())),
        "who: {line}"
    );
    assert_eq!(
        value_of(&line, "when"),
        Some(json_lite::Value::Str("SessionStart".to_owned())),
        "when: {line}"
    );
    clean(&[&repo, &state]);
}

#[test]
fn vessel_external_form() {
    let marker = Marker {
        name: NAME.to_owned(),
        version: GENERATION,
    };
    let form = [marker.render().trim_end().to_owned(), vessel::hook::vessel::usage()].join("\n");
    insta::assert_snapshot!(form);
}

// ---- `vessel update`（設計 consumer-sync.md §5・接頭辞 `vessel_update_`）----
// 偽 `git` と偽 `cargo` を PATH の先頭に置き、撃たれた argv を 1 行ずつ写す。repo は偽 git が読まないので空 dir で足りる。

/// 偽 git が `rev-parse HEAD` に返す 40 桁（先頭 12 桁が記録の sha）。
const UPDATE_HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
/// 偽 cargo が報告する binary の path。
const UPDATE_BIN: &str = "/opt/e2e-bin/scribe2";

/// update の置き場（state dir・`[[vessel]] repo` が指す dir・偽 git / cargo の dir）。
struct UpdatePlace {
    state: TmpDir,
    repo: TmpDir,
    bin: TmpDir,
}

impl UpdatePlace {
    /// argv の写しの path。
    fn log(&self) -> PathBuf {
        self.bin.join("argv.log")
    }

    /// 写った argv の行（repo の path は `[repo]` に置き換える・撃たれなければ空）。
    fn argv(&self) -> Vec<String> {
        let repo = self.repo.display().to_string();
        fs::read_to_string(self.log()).unwrap_or_default().lines().map(|line| line.replace(&repo, "[repo]")).collect()
    }

    /// state dir の `InstallRecorded` の行（log が無ければ 0 件）。
    fn installs(&self) -> Vec<vessel::fleet::Event> {
        let events = vessel::fleet::store::read_all(&self.state).unwrap_or_default();
        events.into_iter().filter(|event| event.kind == vessel::fleet::EventKind::InstallRecorded).collect()
    }
}

/// 置き場を 1 つ作る: `declared` なら host の面に `[[vessel]] repo` を 1 行書き、偽 git（`status` は `status_out` を出す・
/// `merge` は `merge_rc` で終わる）と偽 cargo（`cargo_rc` で終わる・成功の周は install 先の行を stderr に出す）を置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn update_place(declared: bool, status_out: &str, merge_rc: u8, cargo_rc: u8) -> UpdatePlace {
    use std::os::unix::fs::PermissionsExt;
    let place = UpdatePlace { state: tmp(), repo: tmp(), bin: tmp() };
    if declared {
        let host = format!("schema = 1\n\n[[vessel]]\nrepo = \"{}\"\n", place.repo.display());
        fs::write(place.state.join("host.toml"), host).expect("host の面を書ける");
    }
    let log = place.log().display().to_string();
    let git = format!(
        "#!/bin/sh\nprintf '%s\\n' \"git $*\" >> '{log}'\ncase \"$3\" in\n  status) printf '{status_out}' ;;\n  merge) exit {merge_rc} ;;\n  rev-parse) echo {UPDATE_HEAD} ;;\nesac\nexit 0\n"
    );
    let cargo = format!(
        "#!/bin/sh\nprintf '%s\\n' \"cargo $*\" >> '{log}'\necho '  Installing {NAME} v0.1.0 (/src/crates/{NAME})' >&2\n[ {cargo_rc} -eq 0 ] || exit {cargo_rc}\necho '  Installing {UPDATE_BIN}' >&2\necho '   Installed package' >&2\n"
    );
    for (name, body) in [("git", git), ("cargo", cargo)] {
        let stub = place.bin.join(name);
        fs::write(&stub, body).expect("偽 binary を書ける");
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("偽 binary に実行権を付ける");
    }
    place
}

/// 偽 git / cargo の PATH で `vessel update --state-dir <state>` を撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_update(place: &UpdatePlace) -> Output {
    let path = format!("{}:{}", place.bin.display(), std::env::var("PATH").unwrap_or_default());
    Command::new(bin())
        .args(["vessel", "update", "--state-dir"])
        .arg(place.state.as_path())
        .env("PATH", path)
        .output()
        .expect("binary を起動できる")
}

/// 順序固定の 4 段の argv（`status` → `fetch` → `merge --ff-only` → `cargo install`・sha は install の後に読む）。
fn update_argv() -> Vec<String> {
    vec![
        "git -C [repo] status --porcelain".to_owned(),
        "git -C [repo] fetch origin".to_owned(),
        "git -C [repo] merge --ff-only origin/main".to_owned(),
        format!("cargo install --path [repo]/crates/{NAME} --locked --color never"),
        "git -C [repo] rev-parse HEAD".to_owned(),
    ]
}

/// transcript を名指す payload（`transcript_path` は payload の top-level）。
fn seat_payload(cwd: &Path, tool: &str, file: &str, transcript: Option<&str>) -> String {
    let head = match transcript {
        Some(found) => format!("\"transcript_path\":\"{found}\","),
        None => String::new(),
    };
    format!(
        "{{\"cwd\":\"{}\",{head}\"tool_name\":\"{tool}\",\"tool_input\":{{\"file_path\":\"{file}\"}}}}",
        cwd.display()
    )
}

/// 記録 1 行の `what`。
fn what_of(line: &str) -> String {
    match value_of(line, "what") {
        Some(json_lite::Value::Str(found)) => found,
        _ => String::new(),
    }
}

/// 承認の問いの答えから、**flat な決定 object だけ**を切り出す。
///
/// `json_lite::parse_object` は flat object 専用（入れ子は error）なので、3 段の入れ子を
/// そのままは通せない。内側の `{"behavior":…,"message":…}` を取り出して parse すれば、
/// **出力が JSON として読めること**と**値がそのまま届くこと**を機械で確かめられる
/// （字面の `contains` では key が壊れていても気づけない）。
///
/// ⚠ **escape そのものはこの周では測れない**（lens-43 M-1・実測）: 器が固定した message は
/// `"` も `\` も制御文字も含まないので、`json_lite::quote` を素の `format!` へ置き換える
/// 変異が**出力 byte 同一のまま生き残る**。escape は `deny_line` へ任意の message を渡せる
/// unit の歯（`hook::permission` の `deny_line_round_trips_quotes_and_backslashes`）が測る。
/// ここでそう書くのは、この周が測っている範囲を広く見せないためである。
#[expect(
    clippy::panic,
    reason = "統合 test の helper。clippy の allow-panic-in-tests は #[test] 関数の中だけに効く"
)]
fn decision_object(line: &str) -> String {
    let at = line.find("\"decision\":").unwrap_or_else(|| panic!("decision が在る: {line}"));
    let rest = &line[at..];
    let open = rest.find('{').unwrap_or_else(|| panic!("decision の object が在る: {line}"));
    let close = rest.find('}').unwrap_or_else(|| panic!("decision の object が閉じる: {line}"));
    rest[open..=close].to_owned()
}

/// tracked の生成 dir（workspace root + `PLUGIN_DIR`・設計 consumer-sync.md §17）。
fn tracked_payload() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join(PLUGIN_DIR)
}

/// tracked の生成物 hooks.json（生成 dir の下）の本文。
#[expect(
    clippy::panic,
    reason = "統合 test の helper。clippy の allow-panic-in-tests は #[test] 関数の中だけに効く"
)]
fn tracked_hooks_json() -> String {
    fs::read_to_string(digest::hooks_path(&tracked_payload())).unwrap_or_else(|err| panic!("hooks.json を読める: {err}"))
}

/// 生成物 hooks.json（生成 dir の下）が 3 つ目の entry を持ち、**既存 2 entry は不変**である。
#[test]
fn hooks_json_carries_permission_request_entry() {
    let body = tracked_hooks_json();
    for needle in [
        "\"PermissionRequest\"",
        "\"matcher\": \"Bash\"",
        "hook permission-request",
    ] {
        assert_eq!(
            body.matches(needle).count(),
            1,
            "{needle} はちょうど 1 回: {body}"
        );
    }
    for needle in [
        "\"SessionStart\"",
        "hook session-start",
        "\"PreToolUse\"",
        "hook pre-tool-use",
        // PreToolUse の matcher は `.201` で Bash を含む形へ改め、行 ca で末尾に AskUserQuestion を足した（vessel-hook.md §20）。
        "\"matcher\": \"Bash|Edit|Write|MultiEdit|NotebookEdit|AskUserQuestion\"",
    ] {
        assert_eq!(body.matches(needle).count(), 1, "既存 entry は不変: {needle}");
    }
}

/// marketplace.json は plugin.json と**同じ plugin** を名指し、PUBLIC 面に個人情報を持たない。
///
/// needle（private path・URL・email の字面）は**実行時に断片から組み立てる**。
/// 字面を歯の source に置くと `xtask check` の paths-clean が歯そのものを撃つ（本 repo は
/// PUBLIC・SRS CON2）。
#[test]
fn marketplace_json_names_the_same_plugin_as_plugin_json() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(".claude-plugin");
    let market = fs::read_to_string(dir.join("marketplace.json"))
        .unwrap_or_else(|err| panic!("marketplace.json を読める: {err}"));
    let plugin = fs::read_to_string(tracked_payload().join(".claude-plugin").join("plugin.json"))
        .unwrap_or_else(|err| panic!("plugin.json を読める: {err}"));

    let named = format!("\"name\": \"{NAME}\"");
    assert_eq!(
        plugin.matches(&named).count(),
        1,
        "plugin.json は自分の名前を 1 回名乗る: {plugin}"
    );
    let plugins = market
        .split_once("\"plugins\"")
        .map_or_else(String::new, |(_, tail)| tail.to_owned());
    assert_eq!(
        plugins.matches(&named).count(),
        1,
        "marketplace は plugin.json と**同じ字面**の plugin を 1 件だけ名指す: {market}"
    );
    assert!(
        market.contains(&format!("\"owner\": {{\n    \"name\": \"{NAME}\"\n  }}")),
        "owner は name だけを持つ（url / email を置かない）: {market}"
    );

    // 説明文は plugin.json と**同じ字面**である（片方だけ動かす変異はここで落ちる）。
    let described = plugin
        .split_once("\"description\": \"")
        .and_then(|(_, tail)| tail.split_once('"'))
        .map(|(value, _)| value.to_owned())
        .unwrap_or_default();
    assert!(!described.is_empty(), "plugin.json から description を読める: {plugin}");
    assert_eq!(
        market
            .matches(&format!("\"description\": \"{described}\""))
            .count(),
        2,
        "marketplace は plugin.json と同じ description を 2 回（marketplace 自身と plugins[0]）持つ: {market}"
    );

    // 個人情報・host 固有値の不在。断片から組み立てるので歯の source には字面が無い。
    for mark in [
        concat!("/", "home", "/"),
        concat!("~", "/"),
        // scheme の無い forge の host も塞ぐ。`http` だけだと `"repository":
        // "<forge>/<個人>"` の形が素通りする（実測 2026-09-10・lens-385 F1）。
        concat!("git", "hub", ".com/"),
        "http",
        "\"url\"",
        "\"email\"",
    ] {
        assert!(!mark.is_empty(), "needle が空だと不在の検査が空虚になる");
        assert!(
            !market.contains(mark),
            "PUBLIC 面に {mark} を書かない: {market}"
        );
    }
}

// ─────────────────── plugin 同梱の skill（`s2-07l.269` → `s2-07l.479.2` で撤去） ───────────────────

/// plugin root（repo root）の `skills/` は **もう無い**（ADR-0045 §2 (2)・`s2-07l.479.2`）: 判断層の skill
/// 2 つ（復元・退避）は口（`seat rebrief` / `seat consume` / `seat externalize`）ごと消えたので、dir も
/// 名前空間（`<NAME>:<dir 名>`）も残さない。
///
/// **消えたことを測る歯**である（base では 2 dir とも在り `SKILL.md` を読めるので RED）。
#[test]
fn hook_plugin_carries_no_judgement_skills() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    assert!(root.join("crates").is_dir(), "repo root を解けている: {}", root.display());
    assert!(!root.join("skills").exists(), "判断層の skill の dir は残らない: {}", root.display());
    for name in ["rebrief", "ready-compaction"] {
        let path = root.join("skills").join(name).join("SKILL.md");
        assert!(!path.exists(), "{name}: SKILL.md は残らない");
    }
}

// ─────────────────── 席の状態の打刻（hook 側・`s2-07l.95`） ───────────────────

/// 独立 socket の session の pane id（`%N`・生成 hooks.json の shell 行が `$TMUX_PANE` から渡す形）。
fn pane_id_of(socket: &str, name: &str) -> String {
    String::from_utf8_lossy(&tmux(socket, &["display-message", "-p", "-t", name, "#{pane_id}"]).stdout)
        .trim()
        .to_owned()
}

/// `cwd` と `session_id` を持つ payload（打刻 hook が読む 2 key）。
fn stamp_payload(cwd: &Path, sid: &str) -> String {
    format!("{{\"cwd\":\"{}\",\"session_id\":\"{sid}\"}}", cwd.display())
}

/// 打刻 file の path を**契約の字面から**組む（`<state_dir>/seat/<潰した target>/state.jsonl`・
/// target は `session:window` → `session_window`）。
fn state_file(state: &Path, name: &str) -> PathBuf {
    state.join("seat").join(format!("{name}_{name}")).join("state.jsonl")
}

/// 3 event の hook が `--pane` から target を解き、state.jsonl へ typed な打刻を 1 行ずつ残す
/// （SessionStart → idle・UserPromptSubmit → busy・Stop → idle・設計 seat-state.md §2）。打刻だけの
/// 2 event は **stdout 0 byte・stderr 0 byte・rc 0**、session-start の名乗りは不変。打刻は inject.jsonl
/// を増やさない（hook 予算を turn ごとの追記で食わない）。
#[test]
fn seat_state_hook_stamps_three_events_into_state_jsonl() {
    let repo = git_repo();
    let state = linked(&repo);
    let sock_dir = tmp();
    let socket = socket_of(&sock_dir);
    let name = "hookstamp";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    let pane = pane_id_of(&socket, name);
    assert!(pane.starts_with('%'), "pane id の形: {pane:?}");
    let payload = stamp_payload(&repo, "sid-e2e");

    let out = run_hook_args(&["session-start", "--pane", &pane, "--tmux-socket", &socket], &payload);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "session-start は rc 0");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&format!("[{NAME}/SessionStart]")),
        "名乗りの 1 行は不変: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(stderr_text(&out), "", "打刻が書けた周は stderr 0 byte");
    let out = run_hook_args(&["user-prompt-submit", "--pane", &pane, "--tmux-socket", &socket], &payload);
    assert_silent(&out, "user-prompt-submit は打刻だけ（席を止めない・context にも 1 byte も足さない）");
    let out = run_hook_args(&["stop", "--pane", &pane, "--tmux-socket", &socket], &payload);
    assert_silent(&out, "stop は打刻だけ");

    let file = state_file(&state, name);
    let text = fs::read_to_string(&file).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "3 event で 3 行（母集団 {}）: {}: {text}", lines.len(), file.display());
    let expected = [("idle", "SessionStart"), ("busy", "UserPromptSubmit"), ("idle", "Stop")];
    for (line, (state_word, event)) in lines.iter().zip(expected) {
        assert_eq!(value_of(line, "schema"), Some(json_lite::Value::Num(1)), "schema: {line}");
        assert_eq!(value_of(line, "state"), Some(json_lite::Value::Str(state_word.to_owned())), "state: {line}");
        assert_eq!(value_of(line, "event"), Some(json_lite::Value::Str(event.to_owned())), "event（出所）: {line}");
        let ts = value_of(line, "ts").and_then(|value| value.as_num());
        assert!(ts.is_some_and(|found| found > 1_700_000_000), "ts は 1970 年からの秒（0 や欠落でない）: {line}");
        assert_eq!(value_of(line, "sid"), Some(json_lite::Value::Str("sid-e2e".to_owned())), "sid は payload を写す: {line}");
    }
    assert_eq!(inject_lines(&state).len(), 1, "inject.jsonl は session-start の 1 件だけ（打刻は記録を増やさない）");
    // `stop_hook_active` は `Stop` の再入だけを黙らせる: 他の event の payload に在っても打刻する。
    let reentry = format!("{{\"cwd\":\"{}\",\"session_id\":\"sid-e2e\",\"stop_hook_active\":true}}", repo.display());
    let out = run_hook_args(&["user-prompt-submit", "--pane", &pane, "--tmux-socket", &socket], &reentry);
    assert_silent(&out, "user-prompt-submit（stop_hook_active 付き）");
    let text = fs::read_to_string(&file).unwrap_or_default();
    assert_eq!(text.lines().count(), 4, "Stop 以外の event は stop_hook_active に依らず打刻する（母集団 4 行）: {text}");
    drop(guard);
    clean(&[&repo, &state, &sock_dir]);
}

/// 打刻が**書けない**周（state.jsonl の位置に dir が在る）も席を止めない: rc 0・stdout 0 byte・
/// stderr はちょうど 1 行（黙って消さない・lens-95 MEDIUM-3）。
#[test]
fn seat_state_hook_surfaces_store_failure_without_stopping_the_seat() {
    let repo = git_repo();
    let state = linked(&repo);
    let sock_dir = tmp();
    let socket = socket_of(&sock_dir);
    let name = "hookbroken";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    let pane = pane_id_of(&socket, name);
    fs::create_dir_all(state_file(&state, name)).expect("state.jsonl の位置に dir を置ける");

    let out = run_hook_args(&["stop", "--pane", &pane, "--tmux-socket", &socket], &stamp_payload(&repo, "sid-e2e"));

    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "書けなくても rc 0（席を止めない）");
    assert!(out.stdout.is_empty(), "stdout 0 byte");
    assert_eq!(stderr_lines(&out), 1, "書けなかったことを 1 行だけ surface する: {}", stderr_text(&out));
    drop(guard);
    clean(&[&repo, &state, &sock_dir]);
}

/// 打刻しない周: `--pane` が無い・空・解けない pane id・`Stop` の再入（`stop_hook_active`）・器の外の
/// repo。いずれも state.jsonl を作らず、打刻だけの event は 0 byte・rc 0（guard ではない＝席を止めない）。
/// session-start は打刻できなくても名乗りを出す。
#[test]
fn seat_state_hook_stays_silent_when_it_cannot_stamp() {
    let repo = git_repo();
    let state = linked(&repo);
    let sock_dir = tmp();
    let socket = socket_of(&sock_dir);
    let name = "hooksilent";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    let pane = pane_id_of(&socket, name);
    let payload = stamp_payload(&repo, "sid-e2e");
    let reentry = format!("{{\"cwd\":\"{}\",\"session_id\":\"sid-e2e\",\"stop_hook_active\":true}}", repo.display());
    let cases: [(&str, Vec<&str>, &str); 4] = [
        ("--pane 無し（tmux の外の hooks.json）", vec!["user-prompt-submit"], &payload),
        ("--pane 空（$TMUX_PANE 未設定）", vec!["user-prompt-submit", "--pane", "", "--tmux-socket", &socket], &payload),
        ("pane id が解けない", vec!["stop", "--pane", "%99999", "--tmux-socket", &socket], &payload),
        ("Stop の再入", vec!["stop", "--pane", &pane, "--tmux-socket", &socket], &reentry),
    ];
    for (label, args, body) in cases {
        let out = run_hook_args(&args, body);
        assert_silent(&out, label);
    }
    let out = run_hook_args(&["session-start", "--pane", "", "--tmux-socket", &socket], &payload);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "session-start は rc 0");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&format!("[{NAME}/SessionStart]")),
        "打刻できなくても名乗りは出す"
    );
    let bare = tmp();
    let out = run_hook_args(&["user-prompt-submit", "--pane", &pane, "--tmux-socket", &socket], &stamp_payload(&bare, "x"));
    assert_silent(&out, "器の外の repo");
    let seats: Vec<String> = fs::read_dir(state.join("seat"))
        .map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    assert!(seats.is_empty(), "打刻の dir を 1 つも作らない（母集団 = seat 配下の entry）: {seats:?}");
    assert!(!state_file(&state, name).exists(), "state.jsonl は無い");
    drop(guard);
    clean(&[&repo, &state, &sock_dir, &bare]);
}

/// 生成物 hooks.json（生成 dir の下）は打刻の 2 entry（UserPromptSubmit / Stop）を持ち、5 つの command 行が
/// すべて `--pane "$TMUX_PANE"` を受ける（打刻の席と記録の `seat` 列・`s2-07l.150`）。
#[test]
fn seat_state_hooks_json_carries_stamp_entries() {
    let body = tracked_hooks_json();
    let pane_arg = " --pane \\\"$TMUX_PANE\\\"";
    for needle in ["\"UserPromptSubmit\"", "\"Stop\"", "hook user-prompt-submit", "hook stop"] {
        assert_eq!(body.matches(needle).count(), 1, "{needle} はちょうど 1 回: {body}");
    }
    assert_eq!(body.matches(pane_arg).count(), 6, "6 行すべてが pane id を受ける: {body}");
    for sub in ["session-start", "user-prompt-submit", "stop"] {
        assert_eq!(body.matches(&format!("hook {sub}{pane_arg}")).count(), 1, "{sub} の command 行に --pane");
    }
    assert_eq!(body.matches("\"type\": \"command\"").count(), 6, "entry は 6 つ（`.489` で PreCompact が +1）: {body}");
}

// ─────────────────── 読み込み元の記録（consumer-sync.md §3・`s2-07l.303`・接頭辞 `hook_plugin_record_`） ───────────────────

/// 独立 socket の席を 1 つ立てた置き場（repo・state dir・socket の dir・pane id）。
///
/// `guard` は先頭の欄（欄は宣言順に drop される＝席を畳んでから socket の dir を消す）。
struct PluginPlace {
    guard: IsolatedSeat,
    repo: TmpDir,
    state: TmpDir,
    sock_dir: TmpDir,
    socket: String,
    pane: String,
}

/// 席を 1 つ立てる（`name` は session = window の名）。
fn plugin_place(name: &str) -> PluginPlace {
    let repo = git_repo();
    let state = linked(&repo);
    let sock_dir = tmp();
    let socket = socket_of(&sock_dir);
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    let pane = pane_id_of(&socket, name);
    PluginPlace { repo, state, sock_dir, socket, pane, guard }
}

/// plugin の root を tmp に作る（`hooks/hooks.json` は `body` が在る周だけ置く）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn plugin_root(body: Option<&str>) -> TmpDir {
    let root = tmp();
    if let Some(text) = body {
        fs::create_dir_all(root.join("hooks")).expect("hooks dir を作れる");
        fs::write(digest::hooks_path(&root), text).expect("hooks.json を書ける");
    }
    root
}

/// `session-start` を `--plugin-root` 付きで撃つ（rc 0・名乗りは不変・stderr 0 byte）。
fn run_session_start(place: &PluginPlace, extra: &[&str], sid: &str) -> Output {
    let mut args = vec!["session-start", "--pane", &place.pane, "--tmux-socket", &place.socket];
    args.extend_from_slice(extra);
    let out = run_hook_args(&args, &stamp_payload(&place.repo, sid));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "session-start は rc 0: {}", stderr_text(&out));
    assert!(String::from_utf8_lossy(&out.stdout).contains(&format!("[{NAME}/SessionStart]")), "名乗りは不変");
    assert_eq!(stderr_text(&out), "", "stderr 0 byte");
    out
}

/// (a) `--plugin-root <tmp>` 付きの session-start が `seat/<target>/plugin` に 1 行を書く: `hooks=` は tmp の hooks.json の
/// FNV-1a 64・`binary=` は `env!` の build 元 commit・`sid=` は payload・`root=` は渡した path。2 回目は上書き（1 行のまま・
/// 新しい digest）。base は file が無い（RED）。
#[test]
fn hook_plugin_record_is_written_with_the_digest_of_hooks_json() {
    let place = plugin_place("hookplug");
    let root = plugin_root(Some("{\"hooks\":{}}\n"));
    let root_s = root.display().to_string();
    run_session_start(&place, &["--plugin-root", &root_s], "sid-plug");
    let seat_dir = place.state.join("seat").join("hookplug_hookplug");
    let path = digest::record_path(&seat_dir);
    let text = fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(text.lines().count(), 1, "1 file 1 行: {}: {text:?}", path.display());
    assert!(text.starts_with("schema=1 sid=sid-plug root="), "key の順: {text}");
    let want = digest::fnv1a_64(b"{\"hooks\":{}}\n");
    let PluginRecord::Recorded { root: found_root, hooks, binary, sid, ts } = PluginRecord::read(&seat_dir) else {
        panic!("記録が読める: {text}");
    };
    assert_eq!(found_root, root_s, "root は渡した path");
    assert_eq!(hooks.as_deref(), Some(want.as_str()), "hooks は hooks.json の FNV-1a 64: {text}");
    assert_eq!(binary, BUILD_COMMIT,"binary は build 元 commit: {text}");
    assert_eq!(sid, "sid-plug");
    assert!(ts > 1_700_000_000, "ts は 1970 年からの秒: {text}");
    assert_eq!(digest::hooks_digest(&root).as_deref(), Some(want.as_str()), "読み手も同じ digest");
    fs::write(digest::hooks_path(&root), "{\"hooks\":{\"Stop\":[]}}\n").expect("hooks.json を変えられる");
    run_session_start(&place, &["--plugin-root", &root_s], "sid-plug2");
    let again = fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(again.lines().count(), 1, "上書き（追記しない）: {again:?}");
    let PluginRecord::Recorded { hooks: newer, sid: newer_sid, .. } = PluginRecord::read(&seat_dir) else {
        panic!("記録が読める: {again}");
    };
    assert_eq!(newer.as_deref(), Some(digest::fnv1a_64(b"{\"hooks\":{\"Stop\":[]}}\n").as_str()), "新しい digest");
    assert_eq!(newer_sid, "sid-plug2", "最新 session の値");
    assert!(state_file(&place.state, "hookplug").exists(), "打刻は従来どおり");
    drop(place.guard);
    clean(&[&place.repo, &place.state, &place.sock_dir, &root]);
}

/// (形 5・consumer-sync.md §17) tracked の生成 dir（`PLUGIN_DIR`）が plugin root: 生成物の command 行は
/// `--plugin-root "$CLAUDE_PLUGIN_ROOT"` を 6 行すべてで渡したまま（hook の引数は不変）で、digest の path は root 相対の
/// `<root>/hooks/hooks.json` のまま生成 dir の下の hooks.json を指し、その bytes の FNV-1a 64 を返す。root 直下（旧 root）には
/// hooks.json が無く digest は `None`（**否定の枝**＝旧 root を読み込み元と読まない）。
#[test]
fn plugin_payload_generated_dir_is_the_plugin_root_of_the_digest() {
    let body = tracked_hooks_json();
    assert_eq!(body.matches(" --plugin-root \\\"$CLAUDE_PLUGIN_ROOT\\\"").count(), 6, "6 行が --plugin-root を渡す: {body}");
    let payload = tracked_payload();
    assert_eq!(digest::hooks_path(&payload), payload.join("hooks").join("hooks.json"), "digest の path は root 相対のまま");
    assert_eq!(digest::hooks_digest(&payload), Some(digest::fnv1a_64(body.as_bytes())), "生成 dir の下の hooks.json の digest");
    let old_root = payload.parent().map(Path::to_path_buf).unwrap_or_default();
    assert_eq!(digest::hooks_digest(&old_root), None, "root 直下に hooks.json は無い");
}

/// (b) `--plugin-root` 無し・空・pane が空（tmux の外）は記録しない（file 無し・rc 0・名乗りは出る）＝極性の対。
#[test]
fn hook_plugin_record_is_skipped_without_a_root() {
    let place = plugin_place("hooknoroot");
    let root = plugin_root(Some("{}\n"));
    let root_s = root.display().to_string();
    let seat_dir = place.state.join("seat").join("hooknoroot_hooknoroot");
    run_session_start(&place, &[], "sid-none");
    run_session_start(&place, &["--plugin-root", ""], "sid-empty");
    run_session_start(&place, &["--plugin-root", "  "], "sid-blank");
    assert!(!digest::record_path(&seat_dir).exists(), "root が無い・空の周は記録しない");
    assert_eq!(PluginRecord::read(&seat_dir), PluginRecord::Absent, "読み手は不在");
    let out = run_hook_args(&["session-start", "--pane", "", "--tmux-socket", &place.socket, "--plugin-root", &root_s], &stamp_payload(&place.repo, "sid-nopane"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "pane が空でも rc 0");
    assert!(String::from_utf8_lossy(&out.stdout).contains(&format!("[{NAME}/SessionStart]")), "名乗りは出る");
    let seats: Vec<String> = fs::read_dir(place.state.join("seat"))
        .map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    assert_eq!(seats, ["hooknoroot_hooknoroot"], "pane の無い周は席の dir を作らない（打刻の dir だけ）: {seats:?}");
    assert!(!digest::record_path(&seat_dir).exists(), "pane が空の周も記録しない");
    drop(place.guard);
    clean(&[&place.repo, &place.state, &place.sock_dir, &root]);
}

/// (c) root に hooks.json が無い周は `hooks=unreadable` で記録する（記録はする＝doctor が名指す・不在に潰さない）。
/// 記録 file の位置に dir が在って書けない周は名乗りを出し rc 0 のまま stderr 1 行。
#[test]
fn hook_plugin_record_marks_unreadable_when_hooks_json_is_missing() {
    let place = plugin_place("hookunread");
    let root = plugin_root(None);
    let root_s = root.display().to_string();
    let seat_dir = place.state.join("seat").join("hookunread_hookunread");
    run_session_start(&place, &["--plugin-root", &root_s], "sid-unread");
    let text = fs::read_to_string(digest::record_path(&seat_dir)).unwrap_or_default();
    assert!(text.contains(&format!(" hooks={} ", digest::UNREADABLE)), "hooks=unreadable: {text}");
    let PluginRecord::Recorded { hooks, root: found_root, .. } = PluginRecord::read(&seat_dir) else {
        panic!("記録が読める: {text}");
    };
    assert_eq!(hooks, None, "読めない digest は None: {text}");
    assert_eq!(found_root, root_s);
    fs::remove_file(digest::record_path(&seat_dir)).expect("記録を外せる");
    fs::create_dir_all(digest::record_path(&seat_dir)).expect("記録の位置に dir を置ける");
    let args = ["session-start", "--pane", &place.pane, "--tmux-socket", &place.socket, "--plugin-root", &root_s];
    let out = run_hook_args(&args, &stamp_payload(&place.repo, "sid-broken"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "書けなくても rc 0（席を止めない）");
    assert!(String::from_utf8_lossy(&out.stdout).contains(&format!("[{NAME}/SessionStart]")), "名乗りは出る");
    assert_eq!(stderr_lines(&out), 1, "書けなかったことを 1 行だけ surface する: {}", stderr_text(&out));
    assert_eq!(PluginRecord::read(&seat_dir), PluginRecord::Unreadable, "dir は読めない側（不在に潰さない）");
    drop(place.guard);
    clean(&[&place.repo, &place.state, &place.sock_dir, &root]);
}

// ─────────────────── 記録の席の列（`seat` / `ts`・`s2-07l.150`・接頭辞 `seat_attrib_`） ───────────────────

/// 記録 1 行の `seat` / `ts` / `schema` / `tokens` を見る（key が**在ること**も見る＝`None` は key 不在）。
fn assert_attributed(line: &str, seat: Option<&str>, why: &str) {
    let expected = seat.map_or(json_lite::Value::Null, |found| json_lite::Value::Str(found.to_owned()));
    assert_eq!(value_of(line, "seat"), Some(expected), "{why}: seat 列: {line}");
    let ts = value_of(line, "ts").and_then(|value| value.as_num());
    assert!(ts.is_some_and(|found| found > 1_700_000_000), "{why}: ts は 1970 年からの秒（0 や欠落でない）: {line}");
    assert_eq!(value_of(line, "schema"), Some(json_lite::Value::Num(SCHEMA)), "{why}: schema は 1 のまま: {line}");
    assert_eq!(value_of(line, "tokens"), Some(json_lite::Value::Null), "{why}: 数えていない値は null: {line}");
}

/// **測れない編集の記録が席を名乗る**: `--pane` 付きの pre-tool-use（transcript 無し）は通し、記録 1 行に
/// 独立 socket の pane から解いた席の名（`session_window`）を持つ。`--pane` が無い・空・解けない周は
/// `seat` が `null`（key は在る・空文字の席を作らない）。
///
/// `.201` 以後、`--pane` 付きの席は role guard も通る: 席は planner として登録し（`.192`）、編集先は planner の
/// 権能の内側（`design-intent/`）にする＝測れない記録の**後ろ**に role の記録が 1 行並ぶ。解けない pane は
/// 権能なし（target-unresolved）で止まるが、測れない記録は先に書かれ `seat` が `null` である。
#[test]
fn seat_attrib_hook_unmeasured_edit_records_the_seat_named_by_pane() {
    let place = role_place();
    let name = "hookattrib";
    let (guard, pane) = role_seat(&place, name, Some("orchestrator"));
    let seat = format!("{name}_{name}");
    let (state_s, target) = (place.state.display().to_string(), place.repo.join("design-intent").join("x.html").display().to_string());
    let cases: [(&str, Vec<&str>, Option<&str>, bool); 4] = [
        ("--pane 付き", vec!["pre-tool-use", "--state-dir", &state_s, "--pane", &pane, "--tmux-socket", &place.socket], Some(&seat), true),
        ("--pane 無し", vec!["pre-tool-use", "--state-dir", &state_s], None, false),
        ("--pane 空", vec!["pre-tool-use", "--state-dir", &state_s, "--pane", "", "--tmux-socket", &place.socket], None, false),
        ("解けない pane", vec!["pre-tool-use", "--state-dir", &state_s, "--pane", "%99999", "--tmux-socket", &place.socket], None, true),
    ];
    for (label, args, expected, roled) in cases {
        let before = inject_lines(&place.state).len();
        let out = run_hook_args(&args, &seat_payload(&place.repo, "Edit", &target, None));
        if label == "解けない pane" {
            assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{label}: 権能なしで止まる: {}", stderr_text(&out));
        } else {
            assert_silent(&out, &format!("{label}: 測れない周は通す"));
        }
        let lines = inject_lines(&place.state);
        // cap guard（測れない周の記録）は ADR-0045 §2 (2) で消えたので、残るのは role の記録だけである。
        let added = usize::from(roled);
        assert_eq!(lines.len(), before + added, "{label}: 記録は {added} 行増える（母集団 {}）: {lines:?}", lines.len());
        if roled {
            let line = &lines[before];
            assert!(what_of(line).starts_with("role-"), "{label}: role の記録が並ぶ: {lines:?}");
            assert_attributed(line, expected, label);
        }
    }
    drop(guard);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// session-start の名乗りと permission-request の deny の記録も、`--pane` から解いた席を持つ。
#[test]
fn seat_attrib_hook_session_start_and_permission_request_record_the_seat() {
    let repo = git_repo();
    let state = linked(&repo);
    let sock_dir = tmp();
    let socket = socket_of(&sock_dir);
    let name = "hookattribrest";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    let pane = pane_id_of(&socket, name);
    let seat = format!("{name}_{name}");

    let out = run_hook_args(&["session-start", "--pane", &pane, "--tmux-socket", &socket], &stamp_payload(&repo, "sid-attrib"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "session-start は rc 0");
    let lines = inject_lines(&state);
    assert_eq!(lines.len(), 1, "名乗りの記録は 1 行: {lines:?}");
    assert_eq!(what_of(&lines[0]), "session-start-header");
    assert_attributed(&lines[0], Some(&seat), "session-start");

    let out = run_hook_args(
        &["permission-request", "--pane", &pane, "--tmux-socket", &socket],
        &tool_payload(&repo, "Bash", "unused"),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "承認の答えは rc 0");
    let lines = inject_lines(&state);
    assert_eq!(lines.len(), 2, "deny の記録が 1 行増える: {lines:?}");
    assert_eq!(what_of(&lines[1]), "deny");
    assert_attributed(&lines[1], Some(&seat), "permission-request");
    drop(guard);
    clean(&[&repo, &state, &sock_dir]);
}

/// 生成物 hooks.json（生成 dir の下）の **6 entry すべて**が `--pane "$TMUX_PANE"` を渡す（記録の席の出所）。
#[test]
fn seat_attrib_hook_every_hooks_json_entry_passes_the_pane() {
    let body = tracked_hooks_json();
    let pane_arg = " --pane \\\"$TMUX_PANE\\\"";
    let entries = body.matches("\"type\": \"command\"").count();
    assert_eq!(entries, 6, "entry は 6 つ（母集団・`.489` で PreCompact が +1）: {body}");
    for sub in ["session-start", "pre-tool-use", "permission-request", "user-prompt-submit", "stop", "pre-compact"] {
        assert_eq!(body.matches(&format!("hook {sub}{pane_arg}")).count(), 1, "{sub} の command 行が pane id を渡す: {body}");
    }
    assert_eq!(body.matches(pane_arg).count(), entries, "pane id を渡す行は entry と同数: {body}");
}

// ─────────────────── 席の権能の執行（role guard・`s2-07l.201`・接頭辞 `hook_role_`） ───────────────────
//
// 設計 docs/design/seat-roles.md §3 / §4 / §7・ADR-0022 §2.2 / §2.3・SRS FR41 / FR45 / AC15 / AC16。
// 偽 tmux（独立 socket）に席を立て、hook の打刻（session-start）と `seat register`（`.192`）で登録 row を積み、
// fixture の rules manifest（`--rules`・役割ごとの行 2 つ）で `pre-tool-use --pane` を撃つ。

/// 役割の歯の置き場: 器に紐づけた repo・独立 socket・登録の雛形・fixture の rules manifest。
struct RolePlace {
    /// 器に紐づけた repo。
    repo: TmpDir,
    /// 置き場（`vessel init --state-dir`）。
    state: TmpDir,
    /// 独立 socket と fixture を置く dir。
    sock_dir: TmpDir,
    /// 独立 socket の path。
    socket: String,
    /// 登録の雛形の path。
    launch: String,
    /// fixture の rules manifest の path（役割の行 1 本）。
    rules: String,
    /// 偽の台帳 client の path（席の指示文の `{ledger}` を固定する）。
    bd: String,
}

/// orchestrator の権能（裁定 `user 2026-09-18T08:3xZ`・ADR-0045 §2 (1) の値に裁定 `user 2026-09-20`・ADR-0048 §2 が
/// 便 1 本を名指す停止 `stop` を足した列）。**起動と着地（launch / merge）と src の編集（edit-code）は持たない**。
const ORCHESTRATOR_CAPS: &[&str] = &[
    "answer",
    "approve",
    "go",
    "stop",
    "edit-contract",
    "edit-design-intent",
    "edit-design-doc",
    "edit-tests",
    "edit-outside",
];

/// 台帳の fixture（`bd --readonly list --json` の配列・open 2 / in_progress 1 / blocked 0）。
const LEDGER_JSON: &str = "[{\"id\":\"x-1\",\"status\":\"open\"},{\"id\":\"x-2\",\"status\":\"open\"},{\"id\":\"x-3\",\"status\":\"in_progress\"}]";

/// [`LEDGER_JSON`] を数えた 1 行（席の指示文の `{ledger}` の値）。
const LEDGER_LINE: &str = "open=2 in_progress=1 blocked=0";

/// 禁じる語列の fixture（rules 行 `runner.denied_commands`・埋め込みと同じ cargo の 2 語列＝git の語列は host_guard.git へ
/// 移した・設計 vessel-hook.md §11 の形 f 4）。
const DENIED_SEQUENCES: &[&str] = &["cargo mutants", "cargo publish"];

/// 禁じる語列の行の本文（[`DENIED_SEQUENCES`]）と host-guard の語列の 3 行（[`host_guard_rows_text`]）。Bash の command
/// guard はこの 4 行のどれかが無い manifest では全 Bash を止める（FailClosed）ので、Bash を撃つ fixture の manifest は必ず
/// この 4 行を持つ。
fn denied_row_text() -> String {
    denied_rows_text(true)
}

/// [`denied_row_text`] の 4 行（host_guard.tmux の enabled は引数）。
fn denied_rows_text(tmux_enabled: bool) -> String {
    let quoted: Vec<String> = DENIED_SEQUENCES.iter().map(|item| format!("\"{item}\"")).collect();
    format!(
        "\n[[rule]]\nid = \"runner.denied_commands\"\nkind = \"RunnerDeniedCommands\"\nvalue = [{}]\nenabled = true\nruling = \"r\"\nruled_at = \"2026-09-14\"\n{}",
        quoted.join(", "),
        host_guard_rows_text(tmux_enabled)
    )
}

/// host-guard の語列の fixture（どれも runner の語列に無い）。
const HOST_GUARD_SEQUENCES: [(&str, &str); 3] =
    [("host_guard.git", "git push --force"), ("host_guard.tmux", "tmux kill-server"), ("host_guard.ledger", "bd delete")];

/// fixture の host-guard の行の裁定 id。
const HOST_GUARD_RULING: &str = "user 2026-09-19T15:28Z";

/// host-guard の語列の 3 行の本文（host_guard.tmux の enabled は引数）。
fn host_guard_rows_text(tmux_enabled: bool) -> String {
    HOST_GUARD_SEQUENCES
        .iter()
        .map(|(id, sequence)| {
            let enabled = *id != "host_guard.tmux" || tmux_enabled;
            format!(
                "\n[[rule]]\nid = \"{id}\"\nkind = \"HostGuardDeniedCommands\"\nvalue = [\"{sequence}\"]\nenabled = {enabled}\nruling = \"{HOST_GUARD_RULING}\"\nruled_at = \"2026-09-19\"\n"
            )
        })
        .collect()
}

/// 役割の行の本文（役割は orchestrator 1 つ＝行も 1 本・`schema` 行と禁じる語列の行は持たない）。
fn role_rows_text(caps: &[&str]) -> String {
    let quoted: Vec<String> = caps.iter().map(|name| format!("\"{name}\"")).collect();
    format!(
        "\n[[rule]]\nid = \"role.orchestrator\"\nkind = \"RoleCapabilities\"\nvalue = [{}]\nenabled = true\nruling = \"r\"\nruled_at = \"2026-09-18\"\n",
        quoted.join(", ")
    )
}

/// `ORCHESTRATOR_CAPS` から 1 つ抜いた列（負例の行＝通したのが行の値であって判定の穴でないことを測る）。
fn caps_without(drop: &str) -> Vec<&'static str> {
    ORCHESTRATOR_CAPS.iter().copied().filter(|name| *name != drop).collect()
}

/// 台帳の待ち上限の行（`seat.ledger_timeout_s`・席の指示文の `{ledger}` が読む・行が無い周は `unknown`）。
const LEDGER_TIMEOUT_ROW: &str = concat!(
    "\n[[rule]]\nid = \"seat.ledger_timeout_s\"\nkind = \"LedgerTimeoutS\"\nvalue = 60\n",
    "enabled = true\nruling = \"r\"\nruled_at = \"2026-09-12\"\n",
);

/// 役割の行と禁じる語列の行と台帳の待ち上限の行を持つ rules manifest の本文。
fn role_rules_text(caps: &[&str]) -> String {
    format!("schema = 1\n{}{}{LEDGER_TIMEOUT_ROW}", denied_row_text(), role_rows_text(caps))
}

/// 置き場を 1 つ作る（rules は裁定の値と同じ 2 行）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn role_place() -> RolePlace {
    let repo = git_repo();
    let state = linked(&repo);
    let sock_dir = tmp();
    let socket = socket_of(&sock_dir);
    let launch = sock_dir.join("launch.txt");
    fs::write(&launch, "claude\n").expect("雛形を書ける");
    let rules = sock_dir.join("rules.toml");
    fs::write(&rules, role_rules_text(ORCHESTRATOR_CAPS)).expect("rules を書ける");
    let bd = fake_bd(&sock_dir, LEDGER_JSON);
    RolePlace {
        repo,
        state,
        sock_dir,
        socket,
        launch: launch.display().to_string(),
        rules: rules.display().to_string(),
        bd,
    }
}

/// 偽の台帳 client を 1 本置く（`seat rebrief` の歯と同じ型・引数に依らず `body` を stdout へ出す）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_bd(dir: &Path, body: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let json = dir.join("bd.json");
    fs::write(&json, body).expect("台帳の fixture を書ける");
    let path = dir.join("bd");
    fs::write(&path, format!("#!/bin/sh\ncat \"{}\"\n", json.display())).expect("偽の bd を書ける");
    let mut perm = fs::metadata(&path).expect("偽の bd の権限を読める").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("偽の bd を実行可能にできる");
    path.display().to_string()
}

/// 独立 socket に席を立て、hook の打刻（session-start）で sid を置き、`role` が在れば `seat register` で
/// 登録 row を積む（`.192` の口・pane id は row に載らない）。返りは (畳む guard, pane id)。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn role_seat(place: &RolePlace, name: &str, role: Option<&str>) -> (IsolatedSeat, String) {
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    let pane = pane_id_of(&place.socket, name);
    let out = run_hook_args(
        &["session-start", "--pane", &pane, "--tmux-socket", &place.socket],
        &stamp_payload(&place.repo, "sid-role"),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "打刻の session-start は rc 0: {}", stderr_text(&out));
    if let Some(role) = role {
        let target = format!("{name}:{name}");
        let out = Command::new(bin())
            .args(["seat", "register", "--state-dir", &place.state.display().to_string(), "--target", &target])
            .args(["--role", role, "--account", "a1", "--launch", &place.launch, "--anchor", &place.repo.display().to_string()])
            .output()
            .expect("binary を起動できる");
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "seat register は rc 0: {}", stderr_text(&out));
    }
    (guard, pane)
}

/// `Bash` の payload（command 行は JSON の escape を通す＝`"` や `\` を含んでも切れない）。
fn bash_payload(cwd: &Path, command: &str) -> String {
    format!(
        "{{\"cwd\":\"{}\",\"tool_name\":\"Bash\",\"tool_input\":{{\"command\":{}}}}}",
        cwd.display(),
        json_lite::quote(command)
    )
}

/// role guard を撃つ（`--pane` / `--tmux-socket` / fixture の `--rules` 付き・`extra` は追加 flag）。
fn run_role_hook(place: &RolePlace, pane: &str, extra: &[&str], payload: &str) -> Output {
    let mut args = vec!["pre-tool-use", "--pane", pane, "--tmux-socket", &place.socket, "--rules", &place.rules];
    args.extend_from_slice(extra);
    run_hook_args(&args, payload)
}

/// 回答の記帳（`pipe answer`）を含む command 行。
fn answer_line() -> String {
    format!("{NAME} pipe answer --run r --words \"ok\"")
}

/// 記録のうち role guard の行（`what` が `role-` で始まる）。
fn role_records(state: &Path) -> Vec<String> {
    inject_lines(state).into_iter().filter(|line| what_of(line).starts_with("role-")).collect()
}

/// deny の外形（rc 2・stdout 0 byte・stderr 1 行）を見て stderr を返す。
fn assert_role_deny(out: &Output, why: &str) -> String {
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{why}: deny は rc 2: {}", stderr_text(out));
    assert!(out.stdout.is_empty(), "{why}: deny でも stdout は 0 byte");
    assert_eq!(stderr_lines(out), 1, "{why}: deny の stderr は 1 行: {}", stderr_text(out));
    stderr_text(out)
}

/// 直近の role の記録 1 行が `what` と席を持つ。
fn assert_role_record(state: &Path, before: usize, what: &str, seat: &str) {
    let lines = role_records(state);
    assert_eq!(lines.len(), before + 1, "記録は 1 行増える: {lines:?}");
    let line = lines.last().cloned().unwrap_or_default();
    assert_eq!(what_of(&line), what, "記録の what: {line}");
    assert_attributed(&line, Some(seat), what);
}

/// (1)(2)(12): 登録済みの席の Bash 面は**行の値だけ**で決まる: `pipe answer`（行が持つ）→ allow（記録 1 行）・
/// `pipe run`（起動は dispatcher の口ゆえ行に無い）→ deny（deny 文は欠けた権能と rules 行 id・記録 1 行）。
/// 負例として `answer` を抜いた行では同じ command が deny＝通したのは行の値であって判定の穴ではない。
/// 埋め込み manifest（`--rules` 無し）でも同じ判定＝裁定の値が binary に在る。
#[test]
fn hook_role_bash_face_allows_answer_and_denies_launch() {
    let place = role_place();
    let (seat, pane) = role_seat(&place, "rolebash", Some("orchestrator"));
    let payload = bash_payload(&place.repo, &answer_line());

    let before = role_records(&place.state).len();
    assert_silent(&run_role_hook(&place, &pane, &[], &payload), "行が持つ answer は通す");
    assert_role_record(&place.state, before, "role-allow capability=answer", "rolebash_rolebash");

    let launch = bash_payload(&place.repo, &format!("{NAME} pipe run --run r --repo ."));
    let before = role_records(&place.state).len();
    let text = assert_role_deny(&run_role_hook(&place, &pane, &[], &launch), "行に無い launch");
    assert!(text.starts_with(&format!("{NAME}: ")), "器が名乗る: {text}");
    assert!(text.contains("launch"), "欠けた権能を名指す: {text}");
    assert!(text.contains("role.orchestrator"), "rules 行 id: {text}");
    assert_role_record(&place.state, before, "role-deny capability=launch", "rolebash_rolebash");

    // 負例: `answer` を抜いた行では同じ command が deny。
    let stripped = place.sock_dir.join("no-answer.toml");
    fs::write(&stripped, role_rules_text(&caps_without("answer"))).unwrap_or_else(|err| panic!("{err}"));
    let args = ["pre-tool-use", "--pane", &pane, "--tmux-socket", &place.socket, "--rules", &stripped.display().to_string()];
    let text = assert_role_deny(&run_hook_args(&args, &payload), "answer の無い行");
    assert!(text.contains("answer"), "{text}");

    // 埋め込み manifest（tracked の `role.orchestrator` の行）でも同じ判定。
    let out = run_hook_args(&["pre-tool-use", "--pane", &pane, "--tmux-socket", &place.socket], &payload);
    assert_silent(&out, "埋め込み manifest でも answer は通す");
    let out = run_hook_args(&["pre-tool-use", "--pane", &pane, "--tmux-socket", &place.socket], &launch);
    let text = assert_role_deny(&out, "埋め込み manifest でも launch は deny");
    assert!(text.contains("role.orchestrator"), "{text}");
    drop(seat);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (3)(4): 登録の無い pane → deny（権能なし・FailClosed・記録 1 行）／`--pane` 無し・空 → 通す（記録なし）。
#[test]
fn hook_role_denies_unregistered_pane_and_is_inactive_without_pane() {
    let place = role_place();
    let (seat, pane) = role_seat(&place, "roleghost", None);
    let payload = bash_payload(&place.repo, &answer_line());

    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &pane, &[], &payload);
    let text = assert_role_deny(&out, "登録の無い pane");
    assert!(text.contains("権能なし") && text.contains("reason=unregistered"), "理由を名指す: {text}");
    assert_role_record(&place.state, before, "role-deny capability=answer", "roleghost_roleghost");

    // 解けない pane id も同じ（target が解けない＝権能なし）。
    let out = run_role_hook(&place, "%99999", &[], &payload);
    let text = assert_role_deny(&out, "解けない pane");
    assert!(text.contains("reason=target-unresolved"), "{text}");

    let before = role_records(&place.state).len();
    let out = run_hook_args(&["pre-tool-use", "--rules", &place.rules], &payload);
    assert_silent(&out, "--pane 無し（tmux の外の runner / lens）は席ではない");
    let out = run_hook_args(&["pre-tool-use", "--pane", "", "--tmux-socket", &place.socket, "--rules", &place.rules], &payload);
    assert_silent(&out, "--pane 空（$TMUX_PANE 未設定）も席ではない");
    assert_eq!(role_records(&place.state).len(), before, "席でない周は記録も残さない");
    drop(seat);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// 契約表の行 g（`s2-07l.308`・§13）: 未登録の席の deny 文は理由の字面（`reason=unregistered`）を保ったまま末尾に
/// 代替ルート `route=<NAME> seat register …` の 1 句を持つ（FR45・止められた席が source を読まずに登録の口へ行ける）。
/// 解けない pane（`reason=target-unresolved`）と anchor の解けない席（`reason=no-anchor`）も理由ごとの route を持つ。
/// 外形は不変（rc 2・stdout 0 byte・stderr 1 行・記録 1 行）。
#[test]
fn hook_role_guard_route_unregistered_names_seat_register() {
    let place = role_place();
    let (seat, pane) = role_seat(&place, "roleroute", None);
    let payload = bash_payload(&place.repo, &answer_line());

    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &pane, &[], &payload);
    let text = assert_role_deny(&out, "登録の無い pane");
    assert!(text.contains("reason=unregistered（"), "理由の字面は不変: {text}");
    let (_, route) = text.trim_end().split_once(" route=").unwrap_or_default();
    assert!(route.starts_with(&format!("{NAME} seat register ")), "代替ルートは登録の口: {text}");
    for flag in ["--state-dir", "--target", "--role"] {
        assert!(route.contains(flag), "登録の口の引数 {flag}: {text}");
    }
    assert_eq!(text.matches("route=").count(), 1, "route は 1 句: {text}");
    assert_role_record(&place.state, before, "role-deny capability=answer", "roleroute_roleroute");

    // 解けない pane → 理由は不変で route は登録の口ではない（席の起動の口）。
    let text = assert_role_deny(&run_role_hook(&place, "%99999", &[], &payload), "解けない pane");
    assert!(text.contains("reason=target-unresolved（"), "{text}");
    assert!(text.contains(&format!(" route={NAME} seat launch ")), "{text}");
    // anchor の解けない席 → 仕える repo を作る口。
    let bare = tmp();
    let text = assert_role_deny(&run_role_hook(&place, &pane, &["--project", &bare.display().to_string()], &payload), "anchor 無し");
    assert!(text.contains("reason=no-anchor（"), "{text}");
    assert!(text.contains(&format!(" route={NAME} vessel init ")), "{text}");
    drop(seat);
    clean(&[&place.repo, &place.state, &place.sock_dir, &bare]);
}

/// (5)(7)(10): Edit 系は path 種別で判定する（ADR-0045 §2 (1)）: `design-intent/` と `docs/design/` と
/// 歯（`crates/<crate>/tests/`）→ allow・src と `README.md`（どちらも Code）→ deny（行に `edit-code` が無い）。
/// 権能付きでない Bash（`ls`）は通す（記録なし・write-set の policy が在っても Bash は write-set guard に届かない）。
#[test]
fn hook_role_edit_face_classifies_path_kind() {
    let place = role_place();
    let (seat, pane) = role_seat(&place, "roleedit", Some("orchestrator"));
    let edit = |file: &str| tool_payload(&place.repo, "Edit", file);

    let before = role_records(&place.state).len();
    let text = assert_role_deny(&run_role_hook(&place, &pane, &[], &edit("src/lib.rs")), "src の編集");
    assert!(text.contains("edit-code"), "種別の権能を名指す: {text}");
    assert_role_record(&place.state, before, "role-deny path=code", "roleedit_roleedit");

    let before = role_records(&place.state).len();
    assert_silent(&run_role_hook(&place, &pane, &[], &edit("design-intent/spec/srs.html")), "design-intent は通す");
    assert_role_record(&place.state, before, "role-allow path=design-intent", "roleedit_roleedit");

    let before = role_records(&place.state).len();
    assert_silent(&run_role_hook(&place, &pane, &[], &edit("crates/scribe2/tests/e2e/hook.rs")), "歯は通す");
    assert_role_record(&place.state, before, "role-allow path=tests", "roleedit_roleedit");

    assert_role_deny(&run_role_hook(&place, &pane, &[], &edit("README.md")), "README.md（Code）");
    assert_silent(&run_role_hook(&place, &pane, &[], &edit("docs/design/x.md")), "設計 doc は通す");
    // 負例: `edit-tests` を抜いた行では歯の編集も deny（通したのは行の値であって判定の穴ではない）。
    let stripped = place.sock_dir.join("no-tests.toml");
    fs::write(&stripped, role_rules_text(&caps_without("edit-tests"))).unwrap_or_else(|err| panic!("{err}"));
    let args = ["pre-tool-use", "--pane", &pane, "--tmux-socket", &place.socket, "--rules", &stripped.display().to_string()];
    let text = assert_role_deny(&run_hook_args(&args, &edit("crates/scribe2/tests/e2e/hook.rs")), "edit-tests の無い行");
    assert!(text.contains("edit-tests"), "種別の権能を名指す: {text}");
    // 絶対 path も root 相対へ畳んで同じ種別（Write / NotebookEdit も同じ面）。
    let absolute = place.repo.join("design-intent").join("x.html").display().to_string();
    assert_silent(&run_role_hook(&place, &pane, &[], &tool_payload(&place.repo, "Write", &absolute)), "絶対 path");
    let notebook = format!(
        "{{\"cwd\":\"{}\",\"tool_name\":\"NotebookEdit\",\"tool_input\":{{\"notebook_path\":\"src/x.ipynb\"}}}}",
        place.repo.display()
    );
    assert_role_deny(&run_role_hook(&place, &pane, &[], &notebook), "notebook の code");

    // 権能付きでない Bash は通し、記録も残さない（tmux も event log も撃たない・NFR5）。
    write_policy(&place.repo, "src/lib.rs\n");
    let before = role_records(&place.state).len();
    assert_silent(&run_role_hook(&place, &pane, &[], &bash_payload(&place.repo, "ls -la")), "ls は権能付きでない");
    let show = bash_payload(&place.repo, &format!("{NAME} pipe show --run r"));
    assert_silent(&run_role_hook(&place, &pane, &[], &show), "pipe show は権能付きでない");
    assert_eq!(role_records(&place.state).len(), before, "権能付きでない Bash は記録を残さない");
    // policy が在る周の write-set guard は従来どおり効く（Edit は先に write-set guard が止める）。
    let text = stderr_text(&run_role_hook(&place, &pane, &[], &edit("docs/design/x.md")));
    assert!(text.contains("write-set の外"), "write-set guard が先に止める: {text}");
    drop(seat);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// 便の写し `contract.toml` を run dir へ置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_run_contract(state: &Path, run: &str, extra: &str) {
    let dir = state.join("pipe").join(run);
    fs::create_dir_all(&dir).expect("run dir を作れる");
    let body = format!(
        "goal = \"g\"\ndone = \"d\"\nsize = \"S\"\nowner = \"{run}\"\ndisposition = \"A-now\"\nwrite-set = [\"src/lib.rs\", \"docs/\"]\nverify = [\"sh verify-ok.sh\"]\nreq = [\"FR45\"]\ndesign = \"docs/design/seat-roles.md\"\n{extra}"
    );
    fs::write(dir.join("contract.toml"), body).expect("契約の写しを書ける");
}

/// (6・AC16): 契約の印 `opens = ["code"]` で開いた便の write-set の内側 → 行が `edit-code` を持たない席でも
/// allow（記録は `opened`）・外 → deny・印の無い便は deny・印が別の種別（`design-doc`）なら code は deny。
#[test]
fn hook_role_contract_mark_opens_bead_write_set() {
    let place = role_place();
    let (admin, admin_pane) = role_seat(&place, "roleopen", Some("orchestrator"));
    write_run_contract(&place.state, "run-open", "opens = [\"code\"]\n");
    write_run_contract(&place.state, "run-plain", "");
    write_run_contract(&place.state, "run-doc", "opens = [\"design-doc\"]\n");
    let in_bead = |run: &str, rel: &str| {
        let path = place.repo.join(".worktrees").join(NAME).join(run).join(rel);
        tool_payload(&place.repo, "Edit", &path.display().to_string())
    };

    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &admin_pane, &[], &in_bead("run-open", "src/lib.rs"));
    assert_silent(&out, "印で開いた便の write-set の内側は行に無くても通す");
    assert_role_record(&place.state, before, "role-allow path=code opened", "roleopen_roleopen");
    assert_silent(&run_role_hook(&place, &admin_pane, &[], &in_bead("run-open", "docs/new.md")), "末尾 / の項目は配下全部");

    let text = assert_role_deny(&run_role_hook(&place, &admin_pane, &[], &in_bead("run-open", "src/other.rs")), "write-set の外");
    assert!(text.contains("edit-code"), "{text}");
    assert_role_deny(&run_role_hook(&place, &admin_pane, &[], &in_bead("run-plain", "src/lib.rs")), "印の無い便");
    assert_role_deny(&run_role_hook(&place, &admin_pane, &[], &in_bead("run-doc", "src/lib.rs")), "印が別の種別");
    assert_role_deny(&run_role_hook(&place, &admin_pane, &[], &in_bead("run-missing", "src/lib.rs")), "写しの無い便");
    // 印は種別を開くだけで、便の worktree の外（repo 本体）の code は開かない。
    assert_role_deny(&run_role_hook(&place, &admin_pane, &[], &tool_payload(&place.repo, "Edit", "src/lib.rs")), "repo 本体");
    drop(admin);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// `s2-07l.227`: 便の器でない worktree（`.worktrees/planner-x/`・docs PR 用の木）は repo の写し＝worktree 相対で
/// 分類する: 木の中の `design-intent/` → allow・同じ木の `src/` → deny（edit-code）・便の worktree の `src/` は
/// 印の無い周 deny のまま。
#[test]
fn hook_role_worktree_repo_copy_allows_design_intent() {
    let place = role_place();
    let (planner, planner_pane) = role_seat(&place, "roletree", Some("orchestrator"));
    let under = |parts: &[&str]| {
        let path = parts.iter().fold(place.repo.join(".worktrees"), |dir, part| dir.join(part));
        tool_payload(&place.repo, "Edit", &path.display().to_string())
    };

    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &planner_pane, &[], &under(&["planner-x", "design-intent", "x.html"]));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc 0: {}", stderr_text(&out));
    assert_silent(&out, "planner の worktree の design-intent は通す");
    assert_role_record(&place.state, before, "role-allow path=design-intent", "roletree_roletree");
    assert_silent(
        &run_role_hook(&place, &planner_pane, &[], &under(&["planner-x", "docs", "design", "a.md"])),
        "同じ木の設計 doc も通す",
    );

    let text = assert_role_deny(&run_role_hook(&place, &planner_pane, &[], &under(&["planner-x", "src", "x.rs"])), "同じ木の code");
    assert!(text.contains("edit-code"), "{text}");
    let text = assert_role_deny(&run_role_hook(&place, &planner_pane, &[], &under(&[NAME, "run-plain", "src", "x.rs"])), "便の worktree の code");
    assert!(text.contains("edit-code"), "{text}");
    drop(planner);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (8)(9): rules 行が読めない・行が無い → deny（FailClosed・理由を名指す）／1 行に `pipe answer` と
/// `pipe run` が並ぶ Bash は両方の権能が要る（行が `launch` を持たないので deny・記録の種別は両方の名）。
#[test]
fn hook_role_fails_closed_on_missing_rows_and_requires_every_capability_on_the_line() {
    let place = role_place();
    // 席は 1 つで足りる（鍵は (役割, anchor) で役割は 1 つ＝同じ anchor の 2 席目は前の row を置き換える）。
    let (admin, admin_pane) = role_seat(&place, "rolerowa", Some("orchestrator"));
    let planner_pane = admin_pane.clone();
    let launch = bash_payload(&place.repo, &format!("{NAME} pipe run --run r --repo ."));
    let answer = bash_payload(&place.repo, &answer_line());
    assert_silent(&run_role_hook(&place, &admin_pane, &[], &answer), "行が持つ answer は通す（行が読める周の対）");

    // 役割の行が無い manifest → 権能なし。
    let no_row = place.sock_dir.join("no-row.toml");
    fs::write(&no_row, format!("schema = 1\n{}", denied_row_text())).unwrap_or_else(|err| panic!("{err}"));
    let args = ["pre-tool-use", "--pane", &admin_pane, "--tmux-socket", &place.socket, "--rules", &no_row.display().to_string()];
    let text = assert_role_deny(&run_hook_args(&args, &launch), "行の無い manifest");
    assert!(text.contains("reason=no-row role.orchestrator"), "{text}");
    // 読めない manifest（無い file）→ 権能なし。
    let missing = place.sock_dir.join("nope.toml").display().to_string();
    let args = ["pre-tool-use", "--pane", &admin_pane, "--tmux-socket", &place.socket, "--rules", &missing];
    let text = assert_role_deny(&run_hook_args(&args, &launch), "読めない manifest");
    assert!(text.contains("reason=rules-unreadable"), "{text}");
    // 不発効の行も権能なし（値は写すが機械は効かせない）。不発効にするのは役割の行だけ（禁じる語列の行は発効の
    // まま＝先に立つ command guard の門で止まらない）。
    let disabled = place.sock_dir.join("disabled.toml");
    let roles = role_rows_text(ORCHESTRATOR_CAPS).replace("enabled = true", "enabled = false");
    let body = format!("schema = 1\n{}{roles}", denied_row_text());
    fs::write(&disabled, body).unwrap_or_else(|err| panic!("{err}"));
    let args = ["pre-tool-use", "--pane", &admin_pane, "--tmux-socket", &place.socket, "--rules", &disabled.display().to_string()];
    let text = assert_role_deny(&run_hook_args(&args, &launch), "不発効の行");
    assert!(text.contains("reason=no-row role.orchestrator"), "{text}");

    // 2 つの権能付き subcommand が並ぶ行は両方が要る。
    let both = bash_payload(&place.repo, &format!("{NAME} pipe answer --run r --words \"ok\" && {NAME} pipe run --run r"));
    let before = role_records(&place.state).len();
    let text = assert_role_deny(&run_role_hook(&place, &planner_pane, &[], &both), "行は launch を持たない");
    assert!(text.contains("launch") && !text.contains("（answer"), "欠けた権能だけを名指す: {text}");
    assert_role_record(&place.state, before, "role-deny capability=answer+launch", "rolerowa_rolerowa");
    // 対: `answer` も抜いた行では 2 つとも欠けた権能として名指される。
    let stripped = place.sock_dir.join("no-answer-row.toml");
    fs::write(&stripped, role_rules_text(&caps_without("answer"))).unwrap_or_else(|err| panic!("{err}"));
    let args = ["pre-tool-use", "--pane", &admin_pane, "--tmux-socket", &place.socket, "--rules", &stripped.display().to_string()];
    let text = assert_role_deny(&run_hook_args(&args, &both), "answer も launch も持たない行");
    assert!(text.contains("answer") && text.contains("launch"), "欠けた権能を両方名指す: {text}");
    drop(admin);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (11)(13)(14): anchor は payload の `cwd` でなく `--project` から解く（席が `cd` しても guard は外れない・run 2
/// の穴）: cwd = repo の外かつ `--project` = anchor で登録済みの席の `pipe run`（行に無い権能）→ deny と記録 1 行／
/// `--pane` 無しかつ cwd = repo の外 → 0 byte・rc 0（FR24 の沈黙は pane 無しだけ）／`--pane` 在りかつ
/// `--project` = repo でない dir なら権能付き Bash は deny・stderr 1 行（`--state-dir` が在れば記録 1 行）・`ls` は通す。
#[test]
fn hook_role_anchor_comes_from_project_not_cwd() {
    let place = role_place();
    let (admin, admin_pane) = role_seat(&place, "roleanchor", Some("orchestrator"));
    let outside = tmp();
    let launch_line = format!("{NAME} pipe run --run r --repo .");
    let payload = bash_payload(&outside, &launch_line);
    let project = place.repo.display().to_string();

    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &admin_pane, &["--project", &project], &payload);
    let text = assert_role_deny(&out, "cd で repo の外に居ても anchor から解けて deny");
    assert!(text.contains("role.orchestrator"), "{text}");
    assert_role_record(&place.state, before, "role-deny capability=launch", "roleanchor_roleanchor");
    // 同じ形で権能付きでない Bash と Edit（repo の外の path＝Outside・行は edit-outside を持つ＝裁定 12:04Z）。
    assert_silent(&run_role_hook(&place, &admin_pane, &["--project", &project], &bash_payload(&outside, "ls")), "ls");
    assert_silent(
        &run_role_hook(&place, &admin_pane, &["--project", &project], &tool_payload(&outside, "Edit", "x.rs")),
        "repo の外の編集",
    );

    // pane 無し + cwd = repo の外 → 黙る（FR24）。
    let out = run_hook_args(&["pre-tool-use", "--rules", &place.rules], &payload);
    assert_silent(&out, "pane 無しの周だけが FR24 の沈黙");

    // pane 在り + `--project` = repo でない dir → anchor が解けない＝権能付きの操作は deny（黙らない）。
    let bare = tmp();
    let bare_s = bare.display().to_string();
    let out = run_role_hook(&place, &admin_pane, &["--project", &bare_s], &payload);
    let text = assert_role_deny(&out, "anchor の解けない席");
    assert!(text.contains("reason=no-anchor"), "{text}");
    assert_silent(&run_role_hook(&place, &admin_pane, &["--project", &bare_s], &bash_payload(&outside, "ls")), "ls は通す");
    // `--project` が空（$CLAUDE_PROJECT_DIR 未設定）は無いのと同じ＝cwd から解く（互換）。
    let out = run_role_hook(&place, &admin_pane, &["--project", ""], &bash_payload(&place.repo, &launch_line));
    assert!(stderr_text(&out).contains("role.orchestrator"), "空の --project は cwd で解く: {}", stderr_text(&out));
    // 置き場を明示すれば anchor の解けない周も記録 1 行を残す。
    let state_s = place.state.display().to_string();
    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &admin_pane, &["--project", &bare_s, "--state-dir", &state_s], &payload);
    assert_role_deny(&out, "anchor の解けない席（置き場つき）");
    assert_role_record(&place.state, before, "role-deny capability=launch", "roleanchor_roleanchor");
    drop(admin);
    clean(&[&place.repo, &place.state, &place.sock_dir, &outside, &bare]);
}

/// repo の外の path（`Outside`）への Edit / Write を撃ち、fixture の rules・埋め込み manifest の両方で通ることと、
/// `edit-outside` を抜いた行では deny（権能を名指す）になることを見る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn assert_outside_edit_allowed_for(place: &RolePlace, pane: &str, seat: &str, caps: &[&str]) {
    let outside = tmp();
    let file = outside.join("note.md").display().to_string();
    let edit = tool_payload(&place.repo, "Edit", &file);

    let before = role_records(&place.state).len();
    assert_silent(&run_role_hook(place, pane, &[], &edit), "repo の外の Edit は通す");
    assert_role_record(&place.state, before, "role-allow path=outside", seat);
    assert_silent(&run_role_hook(place, pane, &[], &tool_payload(&place.repo, "Write", &file)), "repo の外の Write も通す");
    // 埋め込み manifest（tracked の `role.*` の行）でも同じ＝裁定 12:04Z の値が binary に在る。
    let out = run_hook_args(&["pre-tool-use", "--pane", pane, "--tmux-socket", &place.socket], &edit);
    assert_silent(&out, "埋め込み manifest でも repo の外の Edit は通す");

    // 対: edit-outside を抜いた行では deny（通したのは行の値であって判定の穴ではない）。
    let without: Vec<&str> = caps.iter().copied().filter(|name| *name != "edit-outside").collect();
    let stripped = place.sock_dir.join("no-outside.toml");
    fs::write(&stripped, role_rules_text(&without)).expect("rules を書ける");
    let args = ["pre-tool-use", "--pane", pane, "--tmux-socket", &place.socket, "--rules", &stripped.display().to_string()];
    let text = assert_role_deny(&run_hook_args(&args, &edit), "edit-outside の無い行");
    assert!(text.contains("edit-outside"), "種別の権能を名指す: {text}");
    // repo の内の種別の判定は変わらない（code は持たない）。
    assert_role_deny(&run_role_hook(place, pane, &[], &tool_payload(&place.repo, "Edit", "src/lib.rs")), "repo 内の code");
    clean(&[&outside]);
}

/// 裁定 `user 2026-09-13T12:04Z`: 登録済みの席の repo の外（state dir・auto-memory・scratchpad）への
/// Edit / Write は allow（`PathKind::Outside` → `edit-outside`）。
#[test]
fn hook_role_outside_edit_is_allowed() {
    let place = role_place();
    let (seat, pane) = role_seat(&place, "roleout", Some("orchestrator"));
    assert_outside_edit_allowed_for(&place, &pane, "roleout_roleout", ORCHESTRATOR_CAPS);
    drop(seat);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// command 行の中の escape（`"` / `\`）の後ろに在る subcommand も見落とさない（`field` の字面読みは escape
/// された `"` で切れる＝fail-open だった形）。
#[test]
fn hook_role_reads_the_command_line_through_json_escapes() {
    let place = role_place();
    let (admin, admin_pane) = role_seat(&place, "roleescape", Some("orchestrator"));
    let line = format!("echo \"hi\\\\there\" && {NAME} pipe run --run r");
    let out = run_role_hook(&place, &admin_pane, &[], &bash_payload(&place.repo, &line));
    let text = assert_role_deny(&out, "escape の後ろの起動");
    assert!(text.contains("role.orchestrator"), "{text}");
    drop(admin);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

// ─────────────── path の種別を対象 repo の vessel 宣言が名乗る（契約行 r・`s2-07l.491`・seat-roles.md §24・ADR-0047・接頭辞 `hook_role_paths_`） ───────────────
//
// toy repo に宣言（`.vessel.toml`・任意 key 3 本）を commit して PreToolUse の口から測る。席は orchestrator（行は
// `edit-design-intent` / `edit-design-doc` / `edit-tests` / `edit-outside` を持ち `edit-code` を持たない）で、pane →
// target は**偽 tmux**（設計 §7・[`stub_seat`]・PATH の先頭の script）で解く＝tmux を立てないので nextest の tmux
// group の外で走る。

/// 偽 tmux の席で role guard を撃つ（`--pane` は偽 tmux が読まない固定値・fixture の `--rules` 付き・`extra` は追加 flag）。
fn run_paths_hook(place: &RolePlace, path: &str, extra: &[&str], payload: &str) -> Output {
    let mut args = vec!["pre-tool-use", "--pane", STUB_PANE, "--rules", &place.rules];
    args.extend_from_slice(extra);
    run_stub_hook(path, &args, payload)
}

/// toy repo の宣言の本文（必須 key + 追加の行）。
fn paths_declaration(extra: &str) -> String {
    format!("schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git status\"]\n{extra}")
}

/// toy repo（`place.repo`）の宣言を `body` にして commit する（HEAD の tree に載せる・作業ツリーだけの宣言は
/// [`write_declaration`]）。
fn commit_declaration(place: &RolePlace, body: &str) {
    write_declaration(place, body);
    git(&place.repo, &["add", DECL_FILE]);
    git(&place.repo, &["commit", "-q", "--allow-empty", "-m", "declare"]);
}

/// toy repo の作業ツリーにだけ宣言を書く（commit しない）。
fn write_declaration(place: &RolePlace, body: &str) {
    assert!(fs::write(place.repo.join(DECL_FILE), body).is_ok(), "宣言を書ける");
}

/// repo 相対 `rel` への Edit の payload（cwd は repo）。
fn paths_edit(place: &RolePlace, rel: &str) -> String {
    tool_payload(&place.repo, "Edit", rel)
}

/// `rel` の Edit が通り、記録 1 行が `role-allow path=<kind>` を持つ。
fn assert_paths_allow(place: &RolePlace, path: &str, seat: &str, rel: &str, kind: &str) {
    let before = role_records(&place.state).len();
    assert_silent(&run_paths_hook(place, path, &[], &paths_edit(place, rel)), &format!("{rel} は {kind} として通す"));
    assert_role_record(&place.state, before, &format!("role-allow path={kind}"), seat);
}

/// `rel` の Edit が止まり、deny の 1 行が欠けた権能 `cap` を名指す（返りは deny の 1 行）。
fn assert_paths_deny(place: &RolePlace, path: &str, rel: &str, cap: &str) -> String {
    let text = assert_role_deny(&run_paths_hook(place, path, &[], &paths_edit(place, rel)), rel);
    assert!(text.contains(cap), "{rel}: 欠けた権能 {cap} を名指す: {text}");
    text
}

/// (a) 3 本の key を書いた repo: 宣言した仕様の dir・設計 doc の dir・test の dir の下の編集が orchestrator の席で通り、
/// それ以外（固定の prefix の `design-intent/` / `docs/design/` / `crates/<crate>/tests/` を含む）は code として断られる
/// ＝書かれた key は固定値を**置き換える**（足し合わせない）。負例: `edit-tests` を抜いた行では宣言した test の dir も
/// 断られる（通したのは行の値であって判定の穴ではない）。
#[test]
fn hook_role_paths_three_keys_replace_the_fixed_prefixes() {
    let place = role_place();
    let path = stub_seat(&place, "rolepatha", Some("orchestrator"));
    commit_declaration(&place, &paths_declaration("design-intent-paths = [\"spec/\"]\ndesign-doc-paths = [\"notes/design/\"]\ntests-paths = [\"t/\"]\n"));
    let me = "rolepatha_rolepatha";
    assert_paths_allow(&place, &path, me, "spec/srs.yaml", "design-intent");
    assert_paths_allow(&place, &path, me, "spec/deep/x.md", "design-intent");
    assert_paths_allow(&place, &path, me, "notes/design/a.md", "design-doc");
    assert_paths_allow(&place, &path, me, "t/x_test.py", "tests");
    for rel in ["design-intent/spec/srs.html", "docs/design/x.md", "crates/x/tests/y.rs", "src/lib.rs", "README.md", "notes/a.md", "specs/x.yaml"] {
        let text = assert_paths_deny(&place, &path, rel, "edit-code");
        assert!(!text.contains("paths="), "{rel}: 不正でない宣言は paths= を持たない: {text}");
    }
    // 負例: `edit-tests` を抜いた行では宣言した test の dir も deny。
    let stripped = place.sock_dir.join("no-tests.toml");
    assert!(fs::write(&stripped, role_rules_text(&caps_without("edit-tests"))).is_ok(), "rules を書ける");
    let args = ["pre-tool-use", "--pane", STUB_PANE, "--rules", &stripped.display().to_string()];
    let text = assert_role_deny(&run_stub_hook(&path, &args, &paths_edit(&place, "t/x_test.py")), "edit-tests の無い行");
    assert!(text.contains("edit-tests"), "{text}");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (b) `/` で終わらない項目は完全一致の 1 file だけが通り、同じ名で始まる別の file・別の段の同じ名・同じ名の dir の下は
/// 断られる。
#[test]
fn hook_role_paths_exact_item_matches_one_file_only() {
    let place = role_place();
    let path = stub_seat(&place, "rolepathb", Some("orchestrator"));
    commit_declaration(&place, &paths_declaration("design-doc-paths = [\"DESIGN.md\"]\n"));
    assert_paths_allow(&place, &path, "rolepathb_rolepathb", "DESIGN.md", "design-doc");
    let absolute = place.repo.join("DESIGN.md").display().to_string();
    assert_silent(&run_paths_hook(&place, &path, &[], &tool_payload(&place.repo, "Write", &absolute)), "絶対 path でも同じ 1 file");
    for rel in ["DESIGN.md.bak", "DESIGN.mdx", "docs/DESIGN.md", "DESIGN.md/x", "DESIGN", "docs/design/x.md"] {
        assert_paths_deny(&place, &path, rel, "edit-code");
    }
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (c) 1 本だけ書いた repo: その種別だけが宣言で決まり（固定の `crates/<crate>/tests/` は code に落ちる）、残りの
/// 種別は固定の判定のまま（`design-intent/` と `docs/design/` は通る）。
#[test]
fn hook_role_paths_one_key_leaves_the_other_kinds_fixed() {
    let place = role_place();
    let path = stub_seat(&place, "rolepathc", Some("orchestrator"));
    commit_declaration(&place, &paths_declaration("tests-paths = [\"t/\"]\n"));
    let me = "rolepathc_rolepathc";
    assert_paths_allow(&place, &path, me, "t/x_test.py", "tests");
    assert_paths_deny(&place, &path, "crates/x/tests/y.rs", "edit-code");
    assert_paths_allow(&place, &path, me, "design-intent/spec/srs.html", "design-intent");
    assert_paths_allow(&place, &path, me, "docs/design/x.md", "design-doc");
    assert_paths_deny(&place, &path, "src/lib.rs", "edit-code");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (d) 宣言 file を持たない repo と key を 1 本も書かない宣言の repo は今の分類と同じ（固定の 3 prefix が通り src は
/// 断られる・deny の行に paths= は無い）。
#[test]
fn hook_role_paths_absent_and_keyless_declarations_keep_the_fixed_classification() {
    let place = role_place();
    let path = stub_seat(&place, "rolepathd", Some("orchestrator"));
    let me = "rolepathd_rolepathd";
    let fixed = |why: &str| {
        assert_paths_allow(&place, &path, me, "design-intent/spec/srs.html", "design-intent");
        assert_paths_allow(&place, &path, me, "docs/design/x.md", "design-doc");
        assert_paths_allow(&place, &path, me, "crates/x/tests/y.rs", "tests");
        let text = assert_paths_deny(&place, &path, "src/lib.rs", "edit-code");
        assert!(!text.contains("paths="), "{why}: paths= を持たない: {text}");
        assert_paths_deny(&place, &path, "spec/x.yaml", "edit-code");
    };
    assert!(!place.repo.join(DECL_FILE).exists(), "toy repo は宣言 file を持たない");
    fixed("宣言 file の無い repo");
    commit_declaration(&place, &paths_declaration(""));
    fixed("key を書かない宣言");
    commit_declaration(&place, &paths_declaration("remote = \"origin\"\n"));
    fixed("他の任意 key だけの宣言");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (e) 宣言 file 自身の編集は、宣言がそれを名指していても断られる（席は自分の柵を広げられない・便の worktree と
/// repo の写しの中の宣言 file も同じ）。同じ宣言の他の項目は効いている。
#[test]
fn hook_role_paths_declaration_file_itself_is_always_code() {
    let place = role_place();
    let path = stub_seat(&place, "rolepathe", Some("orchestrator"));
    commit_declaration(&place, &paths_declaration(&format!("design-intent-paths = [\"{DECL_FILE}\", \"spec/\"]\n")));
    assert_paths_allow(&place, &path, "rolepathe_rolepathe", "spec/x.yaml", "design-intent");
    let text = assert_paths_deny(&place, &path, DECL_FILE, "edit-code");
    assert!(!text.contains("paths="), "宣言は不正ではない: {text}");
    let absolute = place.repo.join(DECL_FILE).display().to_string();
    assert_role_deny(&run_paths_hook(&place, &path, &[], &tool_payload(&place.repo, "Write", &absolute)), "絶対 path の宣言 file");
    let bead = place.repo.join(".worktrees").join(NAME).join("run-1").join(DECL_FILE).display().to_string();
    assert_role_deny(&run_paths_hook(&place, &path, &[], &tool_payload(&place.repo, "Edit", &bead)), "便の worktree の宣言 file");
    let copy = place.repo.join(".worktrees").join("planner-x").join(DECL_FILE).display().to_string();
    assert_role_deny(&run_paths_hook(&place, &path, &[], &tool_payload(&place.repo, "Edit", &copy)), "repo の写しの宣言 file");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (f) 不正な宣言（5 つの理由のそれぞれ）の repo は、固定値なら通る path（`design-intent/` 等）も宣言した path も含めて
/// repo 内の全編集が断られ、deny の 1 行が `paths=invalid:<理由>` の字面を持つ。repo の外の編集は宣言に依らず通る。
#[test]
fn hook_role_paths_invalid_declaration_denies_every_edit_with_the_reason() {
    let place = role_place();
    let path = stub_seat(&place, "rolepathf", Some("orchestrator"));
    let outside = tmp();
    let cases: [(&str, &str); 5] = [
        ("design-intent-paths = [\"spec/../x/\"]\n", "parent-segment"),
        ("design-intent-paths = [\"/spec/\"]\n", "absolute"),
        ("design-intent-paths = [\" \"]\n", "empty"),
        ("design-intent-paths = [\"spec/\"]\ndesign-doc-paths = [\"spec/design/\"]\n", "overlap"),
        ("design-intent-paths = \"spec/\"\n", "unreadable"),
    ];
    for (extra, reason) in cases {
        commit_declaration(&place, &paths_declaration(extra));
        for rel in ["design-intent/spec/srs.html", "docs/design/x.md", "crates/x/tests/y.rs", "spec/x.yaml", "src/lib.rs"] {
            let text = assert_paths_deny(&place, &path, rel, "edit-code");
            assert!(text.contains(&format!(" paths=invalid:{reason}")), "{reason}: {rel}: 理由の字面: {text}");
            assert_eq!(text.matches("paths=").count(), 1, "{text}");
        }
        let file = outside.join("note.md").display().to_string();
        let before = role_records(&place.state).len();
        assert_silent(&run_paths_hook(&place, &path, &[], &tool_payload(&place.repo, "Edit", &file)), "repo の外は宣言に依らない");
        assert_role_record(&place.state, before, "role-allow path=outside", "rolepathf_rolepathf");
    }
    // 直した宣言を commit すると戻る（固定値へ黙って戻していたのではなく、宣言を読んでいた）。
    commit_declaration(&place, &paths_declaration("design-intent-paths = [\"spec/\"]\n"));
    assert_paths_allow(&place, &path, "rolepathf_rolepathf", "spec/x.yaml", "design-intent");
    clean(&[&place.repo, &place.state, &place.sock_dir, &outside]);
}

/// (g) commit していない作業ツリーの宣言は効かない: key を書いた file を置いただけの repo は固定の判定のまま、
/// HEAD の宣言を作業ツリーで壊しても HEAD の宣言で分類する（不正にも倒れない）。
#[test]
fn hook_role_paths_uncommitted_declaration_does_not_apply() {
    let place = role_place();
    let path = stub_seat(&place, "rolepathg", Some("orchestrator"));
    let me = "rolepathg_rolepathg";
    write_declaration(&place, &paths_declaration("design-intent-paths = [\"spec/\"]\n"));
    assert_paths_deny(&place, &path, "spec/x.yaml", "edit-code");
    assert_paths_allow(&place, &path, me, "design-intent/spec/srs.html", "design-intent");
    commit_declaration(&place, &paths_declaration("tests-paths = [\"t/\"]\n"));
    assert_paths_allow(&place, &path, me, "t/x_test.py", "tests");
    write_declaration(&place, &paths_declaration("tests-paths = [\"/t/\"]\n"));
    assert_paths_allow(&place, &path, me, "t/x_test.py", "tests");
    let text = assert_paths_deny(&place, &path, "src/lib.rs", "edit-code");
    assert!(!text.contains("paths="), "作業ツリーの壊れた宣言は読まない: {text}");
    assert!(fs::remove_file(place.repo.join(DECL_FILE)).is_ok(), "作業ツリーの宣言を消せる");
    assert_paths_allow(&place, &path, me, "t/x_test.py", "tests");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (h) 便の worktree と repo の写し（便の器でない worktree）の中の file も **anchor の宣言**で分類する: worktree 自身の
/// HEAD が別の宣言（key 無し）を持っていても、anchor の宣言の prefix が通り、固定の prefix は断られる。
#[test]
fn hook_role_paths_worktree_files_are_classified_by_the_anchor_declaration() {
    let place = role_place();
    let path = stub_seat(&place, "rolepathh", Some("orchestrator"));
    let me = "rolepathh_rolepathh";
    // worktree の branch は key 無しの宣言を持ち、anchor（main）はその後に key を書く。
    commit_declaration(&place, &paths_declaration(""));
    let bead = place.repo.join(".worktrees").join(NAME).join("run-1");
    git(&place.repo, &["worktree", "add", "-q", "-b", "run-1", &bead.display().to_string()]);
    commit_declaration(&place, &paths_declaration("design-intent-paths = [\"spec/\"]\n"));
    assert!(git(&bead, &["show", &format!("HEAD:{DECL_FILE}")]).lines().all(|line| !line.contains("design-intent-paths")), "worktree の HEAD は key を持たない");
    let under = |parts: &[&str]| parts.iter().fold(place.repo.join(".worktrees"), |dir, part| dir.join(part)).display().to_string();
    let before = role_records(&place.state).len();
    assert_silent(&run_paths_hook(&place, &path, &[], &tool_payload(&place.repo, "Edit", &under(&[NAME, "run-1", "spec", "x.yaml"]))), "便の worktree");
    assert_role_record(&place.state, before, "role-allow path=design-intent", me);
    let text = assert_role_deny(&run_paths_hook(&place, &path, &[], &tool_payload(&place.repo, "Edit", &under(&[NAME, "run-1", "design-intent", "x.html"]))), "固定の prefix");
    assert!(text.contains("edit-code"), "{text}");
    assert_silent(&run_paths_hook(&place, &path, &[], &tool_payload(&place.repo, "Edit", &under(&["planner-x", "spec", "x.yaml"]))), "repo の写し");
    assert_role_deny(&run_paths_hook(&place, &path, &[], &tool_payload(&place.repo, "Edit", &under(&["planner-x", "design-intent", "x.html"]))), "写しの固定の prefix");
    // 席が便の worktree へ cd しても anchor（`--project`）の宣言で分類する。
    let project = place.repo.display().to_string();
    let out = run_paths_hook(&place, &path, &["--project", &project], &tool_payload(&bead, "Edit", "spec/y.yaml"));
    assert_silent(&out, "cwd が便の worktree でも anchor の宣言");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

// ─────────────── 便を止める権能 stop（契約行 s・`s2-07l.495`・seat-roles.md §25・ADR-0048・接頭辞 `hook_role_stop_`） ───────────────
//
// 登録済みの orchestrator の席（行は `stop` を持ち `launch` を持たない）で PreToolUse の Bash 面を撃つ。席は偽 tmux
// （[`stub_seat`]）で解く＝tmux を立てないので nextest の tmux group の外で走る。約束 3（名指しの停止は通る）・
// 約束 4（`--all` と名指しの無い形は起動の権能へ降りて断られる）・約束 7（`stop` を持たない行では名指しも断られる）。

/// 偽 tmux の席で Bash の command 行を role guard に撃つ（`rules` は fixture の manifest の path・`None` なら埋め込み）。
fn run_stop_hook(place: &RolePlace, path: &str, rules: Option<&str>, line: &str) -> Output {
    let mut args = vec!["pre-tool-use", "--pane", STUB_PANE];
    if let Some(rules) = rules {
        args.extend_from_slice(&["--rules", rules]);
    }
    run_stub_hook(path, &args, &bash_payload(&place.repo, line))
}

/// 名指しでない停止の窓 8 つ（in-file の母集団 9 形のうち降りる側・`--all`／`--run` 無し／値無し／`--run` と `--all`／
/// `--run=<id>` の 1 語／列の道具の flag つき／置き場の値の途中に pipe と別の `--run`／`$(` を含む形）。
const UNNAMED_STOP_WINDOWS: [&str; 8] =
    ["--all", "", "--run", "--run r-1 --all", "--run=r-1", "--run r-1 --runner x", "--run r-1 --state-dir /s|--run x", "--run $(cat id)"];

/// 停止の窓が降りた周の deny 文の末尾の 1 句（seat-roles.md §29・`role_guard.rs` の `STOP_HINT` の字面）。
const STOP_HINT: &str =
    "hint=名指しの停止は --run <id> と置き場・repo・rules の値の対だけの 1 行（前にも後ろにも何も付けない）で stop の権能で通る";

/// 約束 3: 便 1 本を名指す停止（`--run` と値だけ・置き場と repo と rules の flag を足した形も）は orchestrator の席で通り
/// （rc 0・stdout 0 byte・記録 1 行 `role-allow capability=stop`）、埋め込み manifest（`--rules` 無し）でも同じ判定
/// ＝裁定の値が binary に在る。
#[test]
fn hook_role_stop_named_run_passes_in_the_orchestrator_seat() {
    let place = role_place();
    let path = stub_seat(&place, "rolestopa", Some("orchestrator"));
    let me = "rolestopa_rolestopa";
    let named = format!("{NAME} pipe stop --run r-1");
    let before = role_records(&place.state).len();
    assert_silent(&run_stop_hook(&place, &path, Some(&place.rules), &named), "名指しの停止は通る");
    assert_role_record(&place.state, before, "role-allow capability=stop", me);
    let full = format!("{NAME} pipe stop --state-dir /s --run r-1 --repo . --rules /r.toml");
    let before = role_records(&place.state).len();
    assert_silent(&run_stop_hook(&place, &path, Some(&place.rules), &full), "置き場・repo・rules の flag を足した形も通る");
    assert_role_record(&place.state, before, "role-allow capability=stop", me);
    let by_path = format!("target/debug/{NAME} pipe stop --run r-1");
    assert_silent(&run_stop_hook(&place, &path, Some(&place.rules), &by_path), "binary の名は path の末尾でもよい");
    // 埋め込み manifest（tracked の `role.orchestrator` の行）でも同じ判定。
    let before = role_records(&place.state).len();
    assert_silent(&run_stop_hook(&place, &path, None, &named), "埋め込み manifest でも名指しの停止は通る");
    assert_role_record(&place.state, before, "role-allow capability=stop", me);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// 約束 4 / 5: `--all`・`--run` 無し・値無し・`--run` と `--all`・`--run=<id>` の 1 語・列の道具の flag・値に pipe・`$(` を
/// 含む停止と、後ろに別の command が続く行は起動の権能へ降り、行が `stop` を持っていても deny（deny 文は `launch` と
/// 行 id を名指し、末尾に通る名指しの形の 1 句 `hint=`〔§29〕を持つ・記録は `role-deny capability=launch`）。埋め込み
/// manifest でも同じ。窓の無い素の起動の deny 文は `hint=` を持たない。
#[test]
fn hook_role_stop_unnamed_forms_fall_to_launch_and_are_denied() {
    let place = role_place();
    let path = stub_seat(&place, "rolestopb", Some("orchestrator"));
    let me = "rolestopb_rolestopb";
    let trailing = format!("{NAME} pipe stop --run r-1 && ls");
    let lines: Vec<String> = UNNAMED_STOP_WINDOWS.iter().map(|window| format!("{NAME} pipe stop {window}")).chain([trailing]).collect();
    assert_eq!(lines.len(), 9, "降りる形は 9 つ");
    for line in &lines {
        let before = role_records(&place.state).len();
        let text = assert_role_deny(&run_stop_hook(&place, &path, Some(&place.rules), line), line);
        assert!(text.contains("（launch）") && text.contains("role.orchestrator"), "{line}: launch と行 id を名指す: {text}");
        assert!(text.contains("hint=") && text.contains("stop の権能で通る"), "{line}: 通る名指しの形を添える: {text}");
        assert!(text.contains(STOP_HINT), "{line}: 句の字面: {text}");
        assert_role_record(&place.state, before, "role-deny capability=launch", me);
    }
    let all = format!("{NAME} pipe stop --all");
    let text = assert_role_deny(&run_stop_hook(&place, &path, None, &all), "埋め込み manifest でも --all は deny");
    assert!(text.contains("（launch）") && text.contains(STOP_HINT), "{text}");
    let run = format!("{NAME} pipe run --run r-1 --repo .");
    let text = assert_role_deny(&run_stop_hook(&place, &path, Some(&place.rules), &run), "素の起動");
    assert!(text.contains("（launch）") && !text.contains("hint="), "窓の無い素の起動は句を持たない: {text}");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// 約束 7: `stop` を持たない行の席では名指しの停止も断られる（deny 文は `stop` と行 id を名指す・記録は
/// `role-deny capability=stop`）＝通したのは行の値であって判定の穴ではない。同じ行で `--all` は `launch` を名指す。
#[test]
fn hook_role_stop_is_denied_when_the_row_lacks_stop() {
    let place = role_place();
    let path = stub_seat(&place, "rolestopc", Some("orchestrator"));
    let me = "rolestopc_rolestopc";
    let stripped = place.sock_dir.join("no-stop.toml");
    assert!(fs::write(&stripped, role_rules_text(&caps_without("stop"))).is_ok(), "rules を書ける");
    let rules = stripped.display().to_string();
    let named = format!("{NAME} pipe stop --run r-1");
    let before = role_records(&place.state).len();
    let text = assert_role_deny(&run_stop_hook(&place, &path, Some(&rules), &named), "stop の無い行");
    assert!(text.contains("（stop）") && text.contains("role.orchestrator"), "stop と行 id を名指す: {text}");
    assert!(!text.contains("launch"), "名指しの停止は launch を要らない: {text}");
    assert!(!text.contains("hint="), "名指しの停止は窓が降りていない: {text}");
    assert_role_record(&place.state, before, "role-deny capability=stop", me);
    let all = format!("{NAME} pipe stop --all");
    let text = assert_role_deny(&run_stop_hook(&place, &path, Some(&rules), &all), "stop の無い行の --all");
    assert!(text.contains("（launch）"), "{text}");
    // 対: `stop` を持つ行では同じ名指しが通る。
    assert_silent(&run_stop_hook(&place, &path, Some(&place.rules), &named), "stop を持つ行では通る");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

// ─────────────── 器の口の basename の形（契約行 u・`s2-07l.556`・seat-roles.md §27・接頭辞 `hook_role_guard_self_`） ───────────────

/// §27: orchestrator の席（行は `launch` を持たない）で、tmp 配下の写し `<NAME>-pipe.bin` で便を起こす行は `<NAME>` の同じ
/// 行と同じ断り（deny 文が同じ字面・記録は `role-deny capability=launch`）で止まり、`<NAME>ctl` の同じ行は権能の guard を
/// 通る（rc 0・0 byte・role の記録なし）。写しの file は置かない＝guard は字面だけを見る。
#[test]
fn hook_role_guard_self_copied_binary_is_denied_like_the_name() {
    let place = role_place();
    let path = stub_seat(&place, "roleself", Some("orchestrator"));
    let me = "roleself_roleself";
    let tail = "pipe run --run r-1 --repo .";
    let plain = format!("{NAME} {tail}");
    let before = role_records(&place.state).len();
    let want = assert_role_deny(&run_stop_hook(&place, &path, Some(&place.rules), &plain), "器の名の行");
    assert!(want.contains("（launch）") && want.contains("role.orchestrator"), "launch と行 id を名指す: {want}");
    assert_role_record(&place.state, before, "role-deny capability=launch", me);

    let copy = format!("{}/{NAME}-pipe.bin {tail}", place.sock_dir.display());
    let before = role_records(&place.state).len();
    let text = assert_role_deny(&run_stop_hook(&place, &path, Some(&place.rules), &copy), "写しの行");
    assert_eq!(text, want, "写しの行は器の名の行と同じ断り");
    assert_role_record(&place.state, before, "role-deny capability=launch", me);

    let other = format!("{}/{NAME}ctl {tail}", place.sock_dir.display());
    let before = role_records(&place.state).len();
    assert_silent(&run_stop_hook(&place, &path, Some(&place.rules), &other), "NAMEctl は器の口でない");
    assert_eq!(role_records(&place.state).len(), before, "権能付きの操作でない＝role の記録なし");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

// ---- 席の指示文（設計 seat-roles.md §5・ADR-0022 §2.4・FR42 / FR44・AC17・`s2-07l.248`）----
// 登録済みの席の SessionStart は名乗りの後ろに役割の雛形 + rules 行から生成した指示文を出す。登録の無い席は 0 byte。

/// fixture の権能の名を typed に引く（列に無い名は fixture の欠陥＝落とす）。
fn brief_caps(names: &[&str]) -> Vec<Capability> {
    let found: Vec<Capability> = names.iter().filter_map(|name| Capability::parse(name)).collect();
    assert_eq!(found.len(), names.len(), "fixture の権能の名はすべて列に在る: {names:?}");
    found
}

/// 登録 row（`seat register` が積む値と同じ target / anchor・残りは生成文の穴でない）。
fn brief_registration(role: Role, target: &str, anchor: &str) -> Registration {
    Registration {
        role,
        anchor: anchor.to_owned(),
        target: target.to_owned(),
        sid: Some("sid-brief".to_owned()),
        account: "a1".to_owned(),
        launch: "claude\n".to_owned(),
        model: None,
    }
}

/// session-start の出力（rc 0・stderr 0 byte）から名乗りの 1 行を確かめて**その後ろの行**（指示文 + 復帰の DATA）を返す。
fn after_header(out: &Output) -> Vec<String> {
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "session-start は rc 0: {}", stderr_text(out));
    assert_eq!(stderr_text(out), "", "指示文を出せる周は stderr 0 byte");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut lines = stdout.lines();
    let header = lines.next().unwrap_or_default();
    assert!(header.starts_with(&format!("[{NAME}/SessionStart] served version=")), "名乗りは 1 行目のまま: {stdout}");
    lines.map(str::to_owned).collect()
}

/// 名乗りの後ろの行を指示文（§5・`[RECENT-` で始まらない先頭の区間）と復帰の DATA（§21・行頭の marker の区間）に割る。
fn split_recent(lines: Vec<String>) -> (Vec<String>, Vec<String>) {
    let at = lines.iter().position(|line| line.starts_with("[RECENT-")).unwrap_or(lines.len());
    let mut brief = lines;
    let recent = brief.split_off(at);
    (brief, recent)
}

/// session-start を撃ち、名乗りの後ろの**指示文の行だけ**を返す（復帰の DATA の区間は `hook_session_recent_` の歯が読む）。
fn brief_lines(place: &RolePlace, pane: &str, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["session-start", "--pane", pane, "--tmux-socket", &place.socket, "--bd", &place.bd];
    args.extend_from_slice(extra);
    let out = run_hook_args(&args, &stamp_payload(&place.repo, "sid-brief"));
    split_recent(after_header(&out)).0
}

/// (a) 登録済みの target の SessionStart で生成文が名乗りの後ろに出て、権能の名がすべて含まれる。生成文は
/// `render`（雛形の穴に登録 row の target / anchor と fixture の rules 行の値と台帳の現在値）と**同じ字面**で、
/// 行は 11 行（ADR-0045 §2 (3)）、記録は名乗り + 指示文の 2 行（指示文の `bytes` は生成文の byte 数・席を名乗る）。
/// `--rules` 無し（埋め込み manifest）でも同じ経路で出る＝裁定の値が binary に在る。
#[test]
fn hook_brief_session_start_emits_the_role_brief_with_every_capability() {
    let place = role_place();
    let embedded = vessel::rules::manifest::Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest: {errors:?}"));
    let (name, role, caps) = ("brieforch", Role::Orchestrator, ORCHESTRATOR_CAPS);
    let (seat, pane) = role_seat(&place, name, Some(role.as_str()));
    let target = format!("{name}:{name}");
    let registration = brief_registration(role, &target, &place.repo.display().to_string());
    let before = inject_lines(&place.state).len();
    let body = brief_lines(&place, &pane, &["--rules", &place.rules]);
    let expected = brief::render(role, &registration, &brief_caps(caps), LEDGER_LINE);
    assert_eq!(format!("{}\n", body.join("\n")), expected, "生成文は render と同じ字面");
    assert_eq!(body.len(), 11, "注入は 11 行（ADR-0045 §2 (3)）: {body:?}");
    for cap in caps {
        assert!(body.iter().any(|line| line.contains(cap)), "権能 {cap} の名が生成文に現れる: {body:?}");
    }
    assert!(
        body.iter().any(|line| line.contains(&target) && line.contains(&place.repo.display().to_string())),
        "target と anchor の穴: {body:?}"
    );
    assert!(body.iter().any(|line| line.contains(LEDGER_LINE)), "台帳の現在値の穴: {body:?}");
    assert!(
        body.iter().all(|line| !line.contains("edit-code")),
        "src の編集の権能は行にも生成文にも無い（歯だけが edit-tests で開く・ADR-0045 §2 (1)）: {body:?}"
    );
    let lines = inject_lines(&place.state);
    assert_eq!(lines.len(), before + 3, "記録は名乗り + 指示文 + 復帰の DATA の 3 行: {lines:?}");
    let brief_record = lines.iter().skip(before).find(|line| what_of(line) == "session-start-brief").cloned().unwrap_or_default();
    assert_eq!(what_of(&brief_record), "session-start-brief", "{lines:?}");
    assert_eq!(value_of(&brief_record, "bytes"), Some(json_lite::Value::Num(expected.len() as u64)), "bytes は生成文の byte 数: {brief_record}");
    assert_attributed(&brief_record, Some(&format!("{name}_{name}")), "指示文の記録");
    // 埋め込み manifest（`--rules` 無し）でも同じ経路。
    let held = brief::capabilities_of(&embedded, role).unwrap_or_else(|| panic!("埋め込みに行が在る"));
    let body = brief_lines(&place, &pane, &[]);
    assert_eq!(format!("{}\n", body.join("\n")), brief::render(role, &registration, &held, LEDGER_LINE), "埋め込みの行の値");
    drop(seat);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// 台帳を読めない周の `{ledger}` は `unknown`（**数に化けさせない**・憲法 C10）。`--bd` が無い file を指す周も
/// 席は止まらず（rc 0・stderr 0 byte）、行数は 11 行のままである。
#[test]
fn hook_brief_ledger_is_unknown_when_the_client_is_unreadable() {
    let place = role_place();
    let (seat, pane) = role_seat(&place, "briefnobd", Some("orchestrator"));
    let missing = place.sock_dir.join("no-such-bd").display().to_string();
    let args = ["session-start", "--pane", &pane, "--tmux-socket", &place.socket, "--bd", &missing, "--rules", &place.rules];
    let out = run_hook_args(&args, &stamp_payload(&place.repo, "sid-brief"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "席は止めない: {}", stderr_text(&out));
    assert_eq!(stderr_text(&out), "", "断りも出さない");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let (body, _) = split_recent(stdout.lines().skip(1).map(str::to_owned).collect());
    assert_eq!(body.len(), 11, "指示文の行数は変わらない: {body:?}");
    assert!(body.iter().any(|line| line.contains("台帳の現在値 = unknown（台帳を読めない）")), "{body:?}");
    assert!(body.iter().all(|line| !line.contains("open=")), "数に化けない: {body:?}");
    drop(seat);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (b) 登録の無い target で 0 byte（名乗りの 1 行だけ・断りも出さない・記録は名乗りの 1 行だけ）。`--pane` 無し・
/// 解けない pane id も同じ（席ではない＝注入しない）。
#[test]
fn hook_brief_is_silent_for_an_unregistered_target() {
    let place = role_place();
    let (seat, pane) = role_seat(&place, "briefghost", None);
    let before = inject_lines(&place.state).len();
    assert_eq!(brief_lines(&place, &pane, &["--rules", &place.rules]), Vec::<String>::new(), "登録の無い席は 0 byte");
    assert_eq!(brief_lines(&place, "%99999", &["--rules", &place.rules]), Vec::<String>::new(), "解けない pane も 0 byte");
    let out = run_hook_args(&["session-start", "--rules", &place.rules], &stamp_payload(&place.repo, "sid-brief"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)));
    assert_eq!(String::from_utf8_lossy(&out.stdout).lines().count(), 1, "--pane 無しは名乗りだけ");
    let lines = inject_lines(&place.state);
    assert_eq!(lines.len(), before + 3, "記録は名乗りの 3 行だけ（指示文の記録は増えない）: {lines:?}");
    assert!(lines.iter().skip(before).all(|line| what_of(line) == "session-start-header"), "{lines:?}");
    drop(seat);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// 読めない周は黙って 0 byte にしない: event log が壊れている（registry-unreadable）・rules 行が読めない
/// （rules-unreadable）・行の無い役割（no-row）は名乗りの後ろに指示文を出さず stderr に理由 1 行（rc 0 のまま＝席は止めない）。
#[test]
fn hook_brief_names_the_reason_when_it_cannot_resolve_capabilities() {
    let place = role_place();
    let (seat, pane) = role_seat(&place, "briefbroken", Some("orchestrator"));
    let payload = stamp_payload(&place.repo, "sid-brief");
    let refused = |extra: &[&str], reason: &str| {
        let mut args = vec!["session-start", "--pane", &pane, "--tmux-socket", &place.socket];
        args.extend_from_slice(extra);
        let out = run_hook_args(&args, &payload);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{reason}: 席は止めない");
        assert_eq!(String::from_utf8_lossy(&out.stdout).lines().count(), 1, "{reason}: 名乗りだけ");
        assert_eq!(stderr_lines(&out), 1, "{reason}: 理由 1 行: {}", stderr_text(&out));
        assert!(stderr_text(&out).contains(&format!("reason={reason}")), "{reason}: {}", stderr_text(&out));
    };
    let rowless = place.sock_dir.join("rowless.toml");
    fs::write(&rowless, format!("schema = 1\n{}", denied_row_text())).unwrap_or_else(|err| panic!("rules: {err}"));
    refused(&["--rules", &rowless.display().to_string()], "no-row role.orchestrator");
    let missing = place.sock_dir.join("missing.toml").display().to_string();
    refused(&["--rules", &missing], "rules-unreadable");
    let events = vessel::fleet::store::events_path(&place.state);
    let mut file = fs::OpenOptions::new().append(true).open(&events).unwrap_or_else(|err| panic!("events: {err}"));
    writeln!(file, "こわれた行").unwrap_or_else(|err| panic!("events: {err}"));
    refused(&["--rules", &place.rules], "registry-unreadable");
    drop(seat);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (c) 生成文の外形 snapshot（C12.5・穴は fixture の固定値・権能は裁定の値と同じ fixture の列）。
#[test]
fn hook_brief_orchestrator_external_form() {
    let registration = brief_registration(Role::Orchestrator, "fixture:orchestrator", "/srv/anchor");
    insta::assert_snapshot!(
        "hook_brief_orchestrator",
        brief::render(Role::Orchestrator, &registration, &brief_caps(ORCHESTRATOR_CAPS), LEDGER_LINE)
    );
}

/// (d) 注入の中身（ADR-0045 §2 (3)・ADR-0046 §2）: 生成文は **11 行**で、席の同一性 3 行（役割 / 権能 / 台帳の
/// 現在値）・憲法の効く部分 5 行（順位・A1・A4.2・A2 と A3・N1〜N3）・役割の特性 3 行（対話面の作法と信頼度・
/// 実装を自分で行わない・決定はしご）から成る。**C 条文は 1 行も注入しない**（CI の門と guard が執行する）。
#[test]
fn hook_brief_carries_the_ask_first_and_role_lines_without_c_articles() {
    let place = role_place();
    let (seat, pane) = role_seat(&place, "briefsurface", Some("orchestrator"));
    let body = brief_lines(&place, &pane, &["--rules", &place.rules]);
    assert_eq!(body.len(), 11, "注入は 11 行: {body:?}");
    assert!(body.iter().all(|line| line.contains("→ SSOT:")), "行はすべて出所 pointer を持つ: {body:?}");
    for pointer in ["憲法 A1", "憲法 A4.2", "憲法 A2", "憲法 A3", "憲法 N1", "憲法 C17"] {
        assert!(body.iter().any(|line| line.contains(pointer)), "{pointer} を指す行が在る: {body:?}");
    }
    assert!(body.iter().any(|line| line.contains("docs/design/dialogue-surface.md §2")), "対話面の作法: {body:?}");
    assert!(body.iter().any(|line| line.contains("verified / deduced / inferred / uncertain")), "信頼度の 4 語: {body:?}");
    assert!(body.iter().any(|line| line.contains("docs/constitution.md")), "憲法の全文の pointer: {body:?}");
    // 憲法の**規範文**（生成 file の英語の SHALL 文）は 1 行も載らない（C 条文の執行は CI の門と guard・
    // 注入は出所 pointer と要約だけ・ADR-0046 §2）。
    let normative: Vec<&String> = body.iter().filter(|line| line.contains("SHALL") || line.contains("MUST")).collect();
    assert!(normative.is_empty(), "規範文は注入しない（全文は生成 file の pointer が指す）: {normative:?}");
    drop(seat);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

// ---- 復帰の DATA（設計 seat-roles.md §21・FR42 / FR19・`s2-07l.489`・接頭辞 `hook_session_recent_`）----
// 登録済みの席の SessionStart は §5 の指示文（11 行・不変）の後ろに、台帳と git から機械で導いた事実の行を出す。
// 0 件（`[RECENT-NONE]`）と測れない（`[RECENT-UNMEASURED]`）を分ける。登録の無い席は今と同じく 0 byte。
// 席は**偽 tmux**（設計 §7「偽 tmux で pane → target を返す stub」＝PATH の先頭の script）で解く: tmux を立てないので
// nextest の tmux group の外で走り、`--pane` の値は偽 tmux が読まない固定値。

/// 偽 tmux に渡す pane id（値は読まれない・空でなければよい）。
const STUB_PANE: &str = "%0";

/// 偽 tmux: どの引数でも `<name>:<name>` を stdout に出す script を置き、その dir を先頭に足した PATH の値を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn stub_tmux_path(place: &RolePlace, name: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = place.sock_dir.join(format!("tmux-{name}"));
    fs::create_dir_all(&bin_dir).expect("偽 tmux の dir を作れる");
    let stub = bin_dir.join("tmux");
    fs::write(&stub, format!("#!/bin/sh\necho '{name}:{name}'\n")).expect("偽 tmux を書ける");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("偽 tmux に実行権を付ける");
    format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default())
}

/// 偽 tmux の PATH で `hook` を 1 回撃つ（payload は stdin へ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_stub_hook(path: &str, args: &[&str], payload: &str) -> Output {
    let mut child = Command::new(bin())
        .arg("hook")
        .args(args)
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary を起動できる");
    child.stdin.as_mut().expect("stdin を開ける").write_all(payload.as_bytes()).expect("payload を書ける");
    child.wait_with_output().expect("終了を待てる")
}

/// 偽 tmux の席を 1 つ作る: hook の打刻（session-start）で sid を置き、`role` が在れば `seat register` で登録 row を積む
/// （[`role_seat`] と同じ手順・tmux を立てない）。返りは偽 tmux を先頭に持つ PATH の値。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn stub_seat(place: &RolePlace, name: &str, role: Option<&str>) -> String {
    let path = stub_tmux_path(place, name);
    let out = run_stub_hook(&path, &["session-start", "--pane", STUB_PANE], &stamp_payload(&place.repo, "sid-recent"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "打刻の session-start は rc 0: {}", stderr_text(&out));
    if let Some(role) = role {
        let target = format!("{name}:{name}");
        let out = Command::new(bin())
            .args(["seat", "register", "--state-dir", &place.state.display().to_string(), "--target", &target])
            .args(["--role", role, "--account", "a1", "--launch", &place.launch, "--anchor", &place.repo.display().to_string()])
            .output()
            .expect("binary を起動できる");
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "seat register は rc 0: {}", stderr_text(&out));
    }
    path
}

/// 偽 tmux の席で session-start を撃ち、名乗りの後ろの行（指示文 + 復帰の DATA）を返す。
fn stub_session_lines(place: &RolePlace, path: &str, bd: &str) -> Vec<String> {
    let args = ["session-start", "--pane", STUB_PANE, "--rules", &place.rules, "--bd", bd];
    after_header(&run_stub_hook(path, &args, &stamp_payload(&place.repo, "sid-recent")))
}

/// 台帳の 1 件の JSON（`updated_at` は UTC の秒から `Z` の形・題は JSON の escape を通す）。
fn bead_json(id: &str, status: &str, updated_secs: u64, title: &str) -> String {
    format!(
        "{{\"id\":\"{id}\",\"status\":\"{status}\",\"title\":{},\"updated_at\":\"{}\"}}",
        json_lite::quote(title),
        vessel::fleet::cli::format_utc(updated_secs)
    )
}

/// 偽の台帳 client を `sock_dir` の子 dir に 1 本置く（`fake_bd` は dir ごとに 1 本＝名で分ける）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_bd_in(place: &RolePlace, name: &str, body: &str) -> String {
    let dir = place.sock_dir.join(name);
    fs::create_dir_all(&dir).expect("台帳の dir を作れる");
    fake_bd(&dir, body)
}

/// 直近の記録 1 行が復帰の DATA（`what` = `session-start-recent`・bytes は出した行の byte 数 + 改行 1 byte・席を名乗る）。
fn assert_recent_record(state: &Path, before: usize, recent: &[String], seat: &str) {
    let lines = inject_lines(state);
    assert_eq!(lines.len(), before + 3, "記録は名乗り + 指示文 + DATA の 3 行増える: {lines:?}");
    let last = lines.last().cloned().unwrap_or_default();
    assert_eq!(what_of(&last), "session-start-recent", "{last}");
    let bytes = recent.join("\n").len() as u64 + 1;
    assert_eq!(value_of(&last, "bytes"), Some(json_lite::Value::Num(bytes)), "bytes は DATA の byte 数: {last}");
    assert_attributed(&last, Some(seat), "DATA の記録");
}

/// `[RECENT-BEAD]` の区間: `r-0` … が新しい順に上限まで並び、`[RECENT-CUT]` が shown と total を持つ。
fn assert_bead_section(recent: &[String], total: usize) {
    let beads = lines_of(recent, Kind::Bead);
    assert_eq!(beads.len(), BEAD_LIMIT, "上限まで: {recent:?}");
    for (index, line) in beads.iter().enumerate() {
        assert!(line.starts_with(&format!("[RECENT-BEAD] r-{index} open ")), "新しい順: {line}");
        assert!(line.ends_with(&format!(" 直近 {index}")), "題: {line}");
    }
    let cut = marked(recent, recent::MARKER_CUT, Kind::Bead);
    assert_eq!(cut, vec![&format!("[RECENT-CUT] kind=bead shown={BEAD_LIMIT} total={total}")], "{recent:?}");
    assert!(beads.iter().all(|line| !line.contains(" w-1 ") && !line.contains(" w-2 ")), "WIP の id は BEAD に出ない: {beads:?}");
}

/// 偽 tmux の席で session-start を撃ち、名乗りの後ろを指示文（11 行を表明）と復帰の DATA に割って返す。
fn brief_and_recent(place: &RolePlace, path: &str, bd: &str) -> (Vec<String>, Vec<String>) {
    let (brief, recent) = split_recent(stub_session_lines(place, path, bd));
    assert_eq!(brief.len(), 11, "§5 の指示文は 11 行のまま: {brief:?}");
    assert!(brief.iter().all(|line| line.contains("→ SSOT:")), "指示文の行は pointer を持つ: {brief:?}");
    assert!(recent.iter().all(|line| line.starts_with("[RECENT-")), "DATA の行は行頭の marker で始まる: {recent:?}");
    (brief, recent)
}

/// `kind` の区間の本体の行（行頭が `kind` の marker のもの）。
fn lines_of(recent: &[String], kind: Kind) -> Vec<&String> {
    recent.iter().filter(|line| line.starts_with(&format!("{} ", kind.marker()))).collect()
}

/// `[RECENT-NONE] kind=<kind>` / `[RECENT-UNMEASURED] kind=<kind> …` / `[RECENT-CUT] kind=<kind> …` の行。
fn marked<'a>(recent: &'a [String], marker: &str, kind: Kind) -> Vec<&'a String> {
    let head = format!("{marker} kind={}", kind.as_str());
    recent.iter().filter(|line| line.as_str() == head || line.starts_with(&format!("{head} "))).collect()
}

/// worktree を 1 つ足す（`.wt/<name>`・branch `<name>`）。
fn add_worktree(repo: &Path, name: &str) -> PathBuf {
    let path = repo.join(".wt").join(name);
    git(repo, &["worktree", "add", "-q", "-b", name, &path.display().to_string()]);
    path
}

// ---- 圧縮の直前の 1 枠（設計 seat-roles.md §22・FR42 / FR19・`s2-07l.489`・接頭辞 `hook_precompact_`）----
// PreCompact が偽の transcript の末尾から席の直近の発言を `seat/<target>/precompact` に書き、`source = compact` の
// SessionStart が指示文の後ろ・DATA の前に `[PRECOMPACT]` の 1 行と逐語の文を 1 回だけ出して枠を消す。席は §21 と同じ
// 偽 tmux で解く（tmux を立てない）。

/// `cwd` / `session_id` / `source` を持つ SessionStart の payload。
fn session_payload(cwd: &Path, sid: &str, source: &str) -> String {
    format!("{{\"cwd\":\"{}\",\"session_id\":\"{sid}\",\"source\":\"{source}\"}}", cwd.display())
}

/// PreCompact の payload（`trigger` と `transcript_path`・path は `None` なら key ごと無い）。
fn precompact_payload(cwd: &Path, trigger: &str, transcript: Option<&Path>) -> String {
    let path = transcript
        .map(|found| json_lite::quote(&found.display().to_string()))
        .map_or_else(String::new, |quoted| format!(",\"transcript_path\":{quoted}"));
    format!("{{\"cwd\":\"{}\",\"session_id\":\"sid-pc\",\"trigger\":\"{trigger}\"{path}}}", cwd.display())
}

/// transcript の assistant の 1 行（content は block の列・`texts` の各要素が text block・`tool_use` を末尾に足す）。
fn assistant_line(texts: &[&str], with_tool_use: bool) -> String {
    let mut blocks: Vec<String> = texts.iter().map(|text| format!("{{\"type\":\"text\",\"text\":{}}}", json_lite::quote(text))).collect();
    if with_tool_use {
        blocks.push("{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\"input\":{\"command\":\"ls\"}}".to_owned());
    }
    format!("{{\"type\":\"assistant\",\"uuid\":\"u\",\"message\":{{\"role\":\"assistant\",\"content\":[{}]}}}}", blocks.join(","))
}

/// transcript の user の 1 行（content は文字列・席の発言ではない）。
fn user_line(text: &str) -> String {
    format!("{{\"type\":\"user\",\"uuid\":\"u\",\"message\":{{\"role\":\"user\",\"content\":{}}}}}", json_lite::quote(text))
}

/// 偽の transcript を `sock_dir` に 1 本置く（JSONL・末尾改行）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_transcript(place: &RolePlace, name: &str, lines: &[String]) -> PathBuf {
    let path = place.sock_dir.join(format!("{name}.jsonl"));
    fs::write(&path, format!("{}\n", lines.join("\n"))).expect("transcript を書ける");
    path
}

/// 枠の path を**契約の字面から**組む（`<state_dir>/seat/<潰した target>/precompact`）。
fn slot_file(place: &RolePlace, name: &str) -> PathBuf {
    place.state.join("seat").join(format!("{name}_{name}")).join(precompact::FILE)
}

/// 偽 tmux の席で `pre-compact` を撃ち、**rc 0・stdout 0 byte** を表明して stderr を返す（圧縮を止めない）。
fn run_precompact(place: &RolePlace, path: &str, payload: &str) -> String {
    let out = run_stub_hook(path, &["pre-compact", "--pane", STUB_PANE, "--rules", &place.rules], payload);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "pre-compact は rc 0: {}", stderr_text(&out));
    assert!(out.stdout.is_empty(), "pre-compact は stdout 0 byte: {}", String::from_utf8_lossy(&out.stdout));
    stderr_text(&out)
}

/// 偽 tmux の席で `source` 付きの session-start を撃ち、名乗りの後ろの行（指示文 + 枠 + DATA）を返す。
fn session_lines_with(place: &RolePlace, path: &str, source: &str) -> Vec<String> {
    let args = ["session-start", "--pane", STUB_PANE, "--rules", &place.rules, "--bd", &place.bd];
    after_header(&run_stub_hook(path, &args, &session_payload(&place.repo, "sid-pc", source)))
}

/// 名乗りの後ろの行から `[PRECOMPACT]` の区間（header と抜いた文）を切り出す: 指示文 11 行の**直後**に始まり、最初の
/// `[RECENT-` の**直前**で終わる。区間が無ければ空。
fn precompact_section(lines: &[String]) -> Vec<String> {
    let (brief, rest) = split_recent(lines.to_vec());
    let at = brief.iter().position(|line| line.starts_with(precompact::MARKER));
    assert!(rest.iter().all(|line| !line.starts_with(precompact::MARKER)), "枠は DATA の前に出る: {lines:?}");
    match at {
        None => Vec::new(),
        Some(at) => {
            assert_eq!(at, 11, "枠は §5 の指示文 11 行の直後: {lines:?}");
            brief.get(at..).map(<[String]>::to_vec).unwrap_or_default()
        }
    }
}

/// 記録のうち圧縮の枠の行（`what` が `precompact-` か `session-start-precompact` で始まる）。
fn precompact_records(state: &Path) -> Vec<String> {
    inject_lines(state)
        .into_iter()
        .filter(|line| what_of(line).starts_with("precompact-") || what_of(line) == "session-start-precompact")
        .collect()
}

/// `pre-compact` を撃ち、枠が `text` を逐語で持つ（trigger は `auto`・切っていない・stderr 0 byte・記録 `precompact-slot`
/// が席を名乗る）ことを確かめて読んだ枠を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn assert_slot_written(place: &RolePlace, path: &str, name: &str, transcript: &Path, text: &str) -> Slot {
    let before = precompact_records(&place.state).len();
    let stderr = run_precompact(place, path, &precompact_payload(&place.repo, "auto", Some(transcript)));
    assert_eq!(stderr, "", "書けた周は stderr 0 byte");
    let body = fs::read_to_string(slot_file(place, name)).expect("枠が在る");
    let parsed = Slot::parse(&body).expect("枠の形");
    assert_eq!(parsed.text, text, "逐語（同じ行の最後の text block・tool_use だけの行と user の行は飛ばす）");
    assert_eq!(parsed.trigger, "auto");
    assert!(!parsed.cut(), "幅の内側は切らない");
    let records = precompact_records(&place.state);
    assert_eq!(records.len(), before + 1, "記録 1 行: {records:?}");
    let last = records.last().cloned().unwrap_or_default();
    assert_eq!(what_of(&last), "precompact-slot", "{records:?}");
    assert_attributed(&last, Some(&format!("{name}_{name}")), "枠の記録");
    parsed
}

// ---- 復帰の 2 便の歯の補強（設計 seat-roles.md §23・FR42 / FR19 / NFR4・`s2-07l.489.3`・接頭辞 `hook_recovery_edge_`）----
// §21 / §22 の gate の変異検査で生存した面（台帳の子 process の終わり方・worktree を測る順・上限とちょうど同じ件数・
// 時差の字・枠が無い以外の理由で読めない周・socket を渡した席の解決）に歯を足す。どれも着地済みの挙動を測る＝base でも
// 緑（fn の先頭の札）。席は §21 と同じ偽 tmux で解く（tmux を立てない）。src は触らない。

/// 偽の台帳 client を `sock_dir` の子 dir に 1 本置く（[`fake_bd`] と違い script の本文を呼び手が書く＝終わり方を作る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_bd_script(place: &RolePlace, name: &str, body: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let dir = place.sock_dir.join(name);
    fs::create_dir_all(&dir).expect("台帳の dir を作れる");
    let path = dir.join("bd");
    fs::write(&path, format!("#!/bin/sh\n{body}")).expect("偽の bd を書ける");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("偽の bd を実行可能にできる");
    path.display().to_string()
}

/// 台帳の待ち上限を `secs` 秒にした rules manifest を書き、その path を返す（役割の行と禁じる語列の行は fixture と同じ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rules_with_ledger_timeout(place: &RolePlace, secs: u64) -> String {
    let row = format!(
        "\n[[rule]]\nid = \"seat.ledger_timeout_s\"\nkind = \"LedgerTimeoutS\"\nvalue = {secs}\nenabled = true\nruling = \"r\"\nruled_at = \"2026-09-12\"\n"
    );
    let path = place.sock_dir.join(format!("rules-timeout-{secs}.toml"));
    fs::write(&path, format!("schema = 1\n{}{}{row}", denied_row_text(), role_rows_text(ORCHESTRATOR_CAPS))).expect("rules を書ける");
    path.display().to_string()
}

/// 偽 tmux の席で `rules` を差し替えて session-start を撃ち、名乗りの後ろを指示文（11 行を表明）と復帰の DATA に割って返す。
fn brief_and_recent_with_rules(place: &RolePlace, path: &str, rules: &str, bd: &str) -> (Vec<String>, Vec<String>) {
    let args = ["session-start", "--pane", STUB_PANE, "--rules", rules, "--bd", bd];
    let lines = after_header(&run_stub_hook(path, &args, &stamp_payload(&place.repo, "sid-recent")));
    let (brief, recent) = split_recent(lines);
    assert_eq!(brief.len(), 11, "§5 の指示文は 11 行のまま: {brief:?}");
    assert!(recent.iter().all(|line| line.starts_with("[RECENT-")), "DATA の行は行頭の marker で始まる: {recent:?}");
    (brief, recent)
}

/// `dir` に file を 1 つ足して commit する（committer / author の時刻を `date` に固定＝HEAD の commit の新しさを作る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn commit_dated(dir: &Path, message: &str, date: &str) {
    fs::write(dir.join(format!("{message}.txt")), format!("{date}\n")).expect("file を書ける");
    git(dir, &["add", "-A"]);
    let out = Command::new("git")
        .args(["-C", &dir.display().to_string(), "commit", "-q", "-m", message])
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_AUTHOR_DATE", date)
        .output()
        .expect("git を起動できる");
    assert!(out.status.success(), "commit {message}: {}", String::from_utf8_lossy(&out.stderr));
}

/// `git worktree list --porcelain` の列挙の順（worktree の path・先頭は anchor）。
fn listed_worktrees(repo: &Path) -> Vec<String> {
    git(repo, &["worktree", "list", "--porcelain"])
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(str::to_owned)
        .collect()
}

/// 偽 tmux（socket を要る形）: `-S <socket>` を先頭に受けた周だけ `<name>:<name>` を出し、それ以外は rc 1 で黙る script を
/// 置き、その dir を先頭に足した PATH の値を返す（socket が席の解決に渡ったかを偽 tmux の側で測る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn socket_stub_tmux_path(place: &RolePlace, name: &str, socket: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = place.sock_dir.join(format!("tmux-sock-{name}"));
    fs::create_dir_all(&bin_dir).expect("偽 tmux の dir を作れる");
    let stub = bin_dir.join("tmux");
    let body = format!("#!/bin/sh\nif [ \"$1\" = \"-S\" ] && [ \"$2\" = \"{socket}\" ]; then\n  echo '{name}:{name}'\n  exit 0\nfi\nexit 1\n");
    fs::write(&stub, body).expect("偽 tmux を書ける");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("偽 tmux に実行権を付ける");
    format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default())
}

// ─────────────────── 実口座を記録しない（carry-prep.md §7 行 d・SRS FR71・名 `session_start_leaves_no_account_record`） ───────────────────

/// `cwd` と `session_id` と（在れば）`transcript_path` を持つ SessionStart の payload。
fn account_payload(cwd: &Path, sid: &str, transcript: Option<&Path>) -> String {
    let tail = transcript.map_or_else(String::new, |path| format!(",\"transcript_path\":\"{}\"", path.display()));
    format!("{{\"cwd\":\"{}\",\"session_id\":\"{sid}\"{tail}}}", cwd.display())
}

/// 置き場の accounts の下の transcript の path（file は置かない＝字面だけで導く）。
fn account_transcript(state: &Path, label: &str) -> PathBuf {
    state.join("accounts").join(label).join("projects").join("p").join("t.jsonl")
}

/// `session-start` を payload 付きで撃つ（rc 0・名乗りは不変・stderr 0 byte）。
fn run_account_start(place: &PluginPlace, extra: &[&str], payload: &str) -> Output {
    let mut args = vec!["session-start", "--pane", &place.pane, "--tmux-socket", &place.socket];
    args.extend_from_slice(extra);
    let out = run_hook_args(&args, payload);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "session-start は rc 0: {}", stderr_text(&out));
    assert!(String::from_utf8_lossy(&out.stdout).contains(&format!("[{NAME}/SessionStart]")), "名乗りは不変");
    assert_eq!(stderr_text(&out), "", "stderr 0 byte");
    out
}

/// pane を解ける周に置き場の accounts の下の transcript を持つ SessionStart を撃っても、席の dir に実口座の file
/// （名 `account`）を書かない。対照に同じ席の dir の読み込み元の記録（`plugin`）が在る＝target を解いた周である。
/// base は label を書く（RED）。
#[test]
fn session_start_leaves_no_account_record() {
    let place = plugin_place("acctnone");
    let root = plugin_root(Some("{}\n"));
    let root_s = root.display().to_string();
    let seat_dir = place.state.join("seat").join("acctnone_acctnone");
    let inside = account_transcript(&place.state, "a1");
    run_account_start(&place, &["--plugin-root", &root_s], &account_payload(&place.repo, "sid-none", Some(&inside)));
    assert!(digest::record_path(&seat_dir).exists(), "対照: 同じ席の dir に読み込み元の記録が在る（target を解いた）");
    let account = seat_dir.join("account");
    assert!(!account.exists(), "実口座の file を書かない: {}", account.display());
    drop(place.guard);
    clean(&[&place.repo, &place.state, &place.sock_dir, &root]);
}

// ─────── 群の逼迫の 1 行（account-lifecycle.md §19 形 5・契約表の行 h・`s2-07l.491`・接頭辞 `hook_group_`） ───────
//
// 席は偽 tmux（`stub_tmux_path`）で解き、登録 row は core の `register` で積む（口座を歯ごとに選ぶ）。閾値は埋め込みの
// manifest（`--rules` を渡さない＝5 時間窓 85 / 7 日窓 95 / モデル別窓 95・鮮度 300 秒）。実測は event log に置く。

// 群の名を Tier と数字に改めただけの歯（account-lifecycle.md §29 の行 s・base でも緑）。
// flip-check: retroactive s2-07l.647

/// 群の歯の群の名。
const GROUP_NAME: &str = "Tier1";

/// 群の歯の置き場（[`role_place`] と同じ形で、置き場を `sock_dir` の 1 段下に置く）。host の根は置き場の親の下に在る
/// （設計 §20 形 1 / 4）ので、置き場を tmp の根の直下に置くと hook が置く移動を頼む記録が歯どうしで共有され tmp の根に残る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn group_role_place() -> RolePlace {
    let repo = git_repo();
    let sock_dir = tmp();
    let state = sock_dir.join("state");
    fs::create_dir_all(&state).expect("置き場を作れる");
    let out = run_vessel(&["init", "--state-dir", &state.display().to_string(), &repo.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "vessel init は rc 0");
    let socket = socket_of(&sock_dir);
    let launch = sock_dir.join("launch.txt");
    fs::write(&launch, "claude\n").expect("雛形を書ける");
    let rules = sock_dir.join("rules.toml");
    fs::write(&rules, role_rules_text(ORCHESTRATOR_CAPS)).expect("rules を書ける");
    let bd = fake_bd(&sock_dir, LEDGER_JSON);
    RolePlace {
        repo,
        state: TmpDir { path: Some(state) },
        sock_dir,
        socket,
        launch: launch.display().to_string(),
        rules: rules.display().to_string(),
        bd,
    }
}

/// 群の歯の窓が開き直る時刻（遠い未来の番兵・時限にならない）。
const GROUP_FAR: &str = "2099-01-01T00:00:00Z";

/// 鮮度の外の実測の ts（埋め込みの鮮度 300 秒より十分古い）。
const GROUP_STALE_TS: &str = "2026-09-12T02:00:00Z";

/// host の面に口座 a1 / a2 と群 1 つ（置き場 = `anchor`・候補 = a1 → a2＝種は a1）を書く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_group(place: &RolePlace, anchor: &str) {
    let body = format!(
        "schema = 1\n\n[[account]]\nlabel = \"a1\"\n\n[[account]]\nlabel = \"a2\"\n\n[[account-group]]\nname = \"{GROUP_NAME}\"\n\
         anchors = [\"{anchor}\"]\naccounts = [\"a1\", \"a2\"]\n"
    );
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), body).expect("host の面を書ける");
}

/// 偽 tmux の席（target `<name>:<name>`）に登録 row（口座 `account`・anchor = repo）を積み、PATH の値を返す。
fn group_seat(place: &RolePlace, name: &str, account: &str) -> String {
    let path = stub_tmux_path(place, name);
    let row = Registration {
        role: Role::Orchestrator,
        anchor: place.repo.display().to_string(),
        target: format!("{name}:{name}"),
        sid: None,
        account: account.to_owned(),
        launch: String::new(),
        model: None,
    };
    assert!(vessel::seat::role::register(&place.state, row).is_ok(), "登録 row を積める");
    path
}

/// いまの UTC の ts（実測行と同じ字面・鮮度の内側）。
fn group_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    vessel::fleet::cli::format_utc(secs)
}

/// 埋め込みの役割 orchestrator の既定の model の表示名（hook は埋め込みの面を読む＝モデル別窓はこの名の窓だけが逼迫を測る・
/// account-lifecycle.md §33 形 1）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn group_role_model() -> &'static str {
    vessel::seat::role::defaults(Role::Orchestrator).expect("埋め込みの役割の既定を読める").model.display()
}

/// 口座 1 つの実測の回を `ts` で置く（窓ごとの使用率・モデル別窓の model は埋め込みの役割の既定の表示名）。
fn put_group_round(state: &Path, ts: &str, account: &str, windows: &[(vessel::fleet::WindowKind, u64)]) {
    put_group_round_as(state, ts, account, windows, group_role_model());
}

/// [`put_group_round`] のモデル別窓の model の名を `model` にした形。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_group_round_as(state: &Path, ts: &str, account: &str, windows: &[(vessel::fleet::WindowKind, u64)], model: &str) {
    use vessel::fleet::{Allowance, Event, EventKind, Measured, WindowKind};
    let policy = vessel::fleet::store::LockPolicy::embedded().expect("lock の規則を読める");
    for (window, used_pct) in windows {
        let measured = Measured {
            account: account.to_owned(),
            window: *window,
            model: (*window == WindowKind::SevenDayModel).then(|| model.to_owned()),
            endpoint: "oauth-usage".to_owned(),
            used_pct: *used_pct,
            resets_at: Some(GROUP_FAR.to_owned()),
        };
        let event = Event {
            schema: vessel::fleet::SCHEMA,
            ts: ts.to_owned(),
            kind: EventKind::AllowanceMeasured,
            run: String::new(),
            bead: String::new(),
            host: "h".to_owned(),
            actor: EventKind::AllowanceMeasured.default_actor().to_owned(),
            stage: None,
            seat: None,
            pid: None,
            detail: None,
            allowance: Some(Allowance::Measured(measured)),
            registration: None,
            mark: None,
            account: None,
            cost: None,
            rule: None,
            case: None,
        };
        vessel::fleet::store::append(state, &event, policy).expect("実測を置ける");
    }
}

/// 5 時間窓・7 日窓・モデル別窓の使用率の回（歯ごとに値だけを選ぶ）。
fn group_windows(five: u64, seven: u64, model: u64) -> [(vessel::fleet::WindowKind, u64); 3] {
    use vessel::fleet::WindowKind;
    [(WindowKind::FiveHour, five), (WindowKind::SevenDay, seven), (WindowKind::SevenDayModel, model)]
}

/// 逼迫の 1 行（器の字面を借りない＝外形を測る側は自分で書く・§21 形 2 (b) の後半）。
fn group_line(account: &str, window: &str, used: u64, cap: u64) -> String {
    format!("group={GROUP_NAME} account={account} window={window} used={used} cap={cap} — 次の 1 周が移り先を決める")
}

/// UserPromptSubmit を偽 tmux の席で撃ち、stdout の行を返す（rc 0・stderr 0 byte を要求）。
fn group_prompt_lines(place: &RolePlace, path: &str) -> Vec<String> {
    let out = run_stub_hook(path, &["user-prompt-submit", "--pane", STUB_PANE], &stamp_payload(&place.repo, "sid-group"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "UserPromptSubmit は rc 0: {}", stderr_text(&out));
    assert_eq!(stderr_text(&out), "", "stderr 0 byte");
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect()
}

/// SessionStart を偽 tmux の席で撃ち、名乗りの後ろの行（指示文 + 復帰の DATA + 群の行）のうち群の段の行だけを返す。
fn group_session_lines(place: &RolePlace, path: &str) -> Vec<String> {
    let args = ["session-start", "--pane", STUB_PANE, "--bd", &place.bd];
    let lines = after_header(&run_stub_hook(path, &args, &stamp_payload(&place.repo, "sid-group")));
    lines.into_iter().filter(|line| line.starts_with("group=") || line.starts_with("usage: ")).collect()
}

// ─────── 移動を頼む記録（account-lifecycle.md §20 形 4・契約表の行 i・接頭辞 `hook_group_move_`・§19 の歯と同じ fixture） ───────

/// host の根の群用 dir（`<置き場の親>/<NAME>-host/groups`・器の字面を借りない）の直下の、移動を頼む記録の file 名。
fn group_requests(place: &RolePlace) -> Vec<String> {
    let dir = place.state.parent().unwrap_or(&place.state).join(format!("{}-host", vessel::name::NAME)).join("groups");
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| entries.filter_map(Result::ok).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.retain(|name| name.ends_with(".request"));
    names.sort();
    names
}

/// 群 `Tier1` の移動を頼む記録の本文（無ければ空）。
fn group_request_body(place: &RolePlace) -> String {
    let dir = place.state.parent().unwrap_or(&place.state).join(format!("{}-host", vessel::name::NAME)).join("groups");
    fs::read_to_string(dir.join(format!("{GROUP_NAME}.request"))).unwrap_or_default()
}

// ─────── 記録の口座と登録 row の食い違い（account-lifecycle.md §21 形 2・契約表の行 j・接頭辞 `hook_group_current_`・§19 の fixture） ───────

/// 群 `Tier1` の今の口座の記録（host の根の群用 dir の `Tier1.account`）に `body` を書く。
fn put_group_record(place: &RolePlace, body: &str) {
    let dir = place.state.parent().unwrap_or(&place.state).join(format!("{}-host", vessel::name::NAME)).join("groups");
    fs::create_dir_all(&dir).ok();
    fs::write(dir.join(format!("{GROUP_NAME}.account")), body).ok();
}

/// 記録の形（[`put_group_record`] の本文・移り先 `account`・前の口座 `previous`）。
fn group_record(account: &str, previous: &str) -> String {
    format!("account={account}\nts=2026-09-24T00:00:00Z\nreason=move\nprevious={previous}\n")
}

/// 移動中の 1 行（器の字面を借りない）。
fn moving_line(row: &str, current: &str) -> String {
    format!("group={GROUP_NAME} row={row} current={current} — 器が移動中: 作業記憶を台帳と git に残して待つ（/exit は器が送る）")
}

