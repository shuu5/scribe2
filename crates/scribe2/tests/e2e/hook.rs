//! `.vessel` marker と hook の歯（設計 docs/design/vessel-hook.md §7）。
//!
//! tmp の git repo を `git init` + commit で作り、`vessel init --state-dir` で置き場を
//! 紐づけてから hook を撃つ。commit には identity が要るので **repo local** の
//! `user.name` / `user.email` を与える（global 設定は 1 byte も触らない）。

use crate::make_tmp_dir;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
use vessel::fleet::json_lite;
use vessel::hook::vessel::{Marker, GENERATION, MARKER};
use vessel::hook::{guard, inject_path, SCHEMA};
use vessel::name::NAME;

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
fn tmp() -> PathBuf {
    let dir = make_tmp_dir().expect("tmp dir を作れる");
    dir.canonicalize().expect("tmp dir の実体 path を解ける")
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
fn git_repo() -> PathBuf {
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
fn run_hook(event: &str, payload: &str) -> Output {
    let mut child = Command::new(bin())
        .arg("hook")
        .arg(event)
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
fn linked(repo: &Path) -> PathBuf {
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

#[test]
fn hook_session_start_is_noop_without_marker() {
    let repo = git_repo();
    let out = run_hook("session-start", &payload(&repo));
    assert_silent(&out, "marker が無い repo");
    clean(&[&repo]);
}

#[test]
fn hook_session_start_is_noop_for_other_name() {
    let repo = git_repo();
    let other = Marker {
        name: "other-vessel".to_owned(),
        version: GENERATION,
    };
    fs::write(repo.join(MARKER), other.render()).expect("marker を書ける");
    let out = run_hook("session-start", &payload(&repo));
    assert_silent(&out, "別の器が名乗る repo");
    clean(&[&repo]);
}

#[test]
fn hook_session_start_is_noop_without_state_dir() {
    let repo = git_repo();
    let mine = Marker {
        name: NAME.to_owned(),
        version: GENERATION,
    };
    fs::write(repo.join(MARKER), mine.render()).expect("marker を書ける");
    let out = run_hook("session-start", &payload(&repo));
    assert_silent(&out, "marker は在るが state dir が紐づいていない repo");
    clean(&[&repo]);
}

#[test]
fn hook_session_start_serves_own_marker() {
    let repo = git_repo();
    let state = linked(&repo);
    let out = run_hook("session-start", &payload(&repo));

    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "仕える周も rc 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&format!("[{NAME}/SessionStart]")),
        "名乗りの 1 行が出る: {stdout}"
    );

    let lines = inject_lines(&state);
    assert_eq!(lines.len(), 1, "注入の記録は 1 件（母集団 {}）", lines.len());
    let line = lines.first().map_or_else(String::new, Clone::clone);
    assert_eq!(
        value_of(&line, "schema"),
        Some(json_lite::Value::Num(SCHEMA)),
        "schema=1: {line}"
    );
    let bytes = value_of(&line, "bytes").and_then(|value| value.as_num());
    assert!(bytes.is_some_and(|found| found > 0), "bytes>0: {line}");
    assert_eq!(
        value_of(&line, "tokens"),
        Some(json_lite::Value::Null),
        "数えていない token は null で表す: {line}"
    );
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_denies_edit_outside_write_set() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/lib.rs\n");
    let out = run_hook(
        "pre-tool-use",
        &tool_payload(&repo, "Edit", "docs/other.md"),
    );

    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "deny は rc 2");
    assert!(out.stdout.is_empty(), "deny でも stdout は 0 byte");
    assert!(!out.stderr.is_empty(), "deny の理由が stderr に 1 行");

    let denies = inject_lines(&state)
        .into_iter()
        .filter(|line| value_of(line, "what") == Some(json_lite::Value::Str("deny".to_owned())))
        .count();
    assert_eq!(denies, 1, "deny も注入の記録に 1 件残る");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_denies_path_escaping_root() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/\n");
    let out = run_hook(
        "pre-tool-use",
        &tool_payload(&repo, "Write", "../outside.rs"),
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "root の外へ出る path は deny"
    );
    assert!(out.stdout.is_empty(), "deny でも stdout は 0 byte");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_allows_edit_inside_write_set() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/lib.rs\ndocs/\n");
    for target in ["src/lib.rs", "docs/design/x.md"] {
        let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", target));
        assert_silent(&out, &format!("write-set の内側（{target}）"));
    }
    let absolute = repo.join("src").join("lib.rs").display().to_string();
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", &absolute));
    assert_silent(&out, "絶対 path でも root 相対へ正規化して通す");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_fails_closed_when_policy_unreadable() {
    let repo = git_repo();
    let state = linked(&repo);
    let path = write_policy(&repo, "src/lib.rs\n");
    fs::remove_file(&path).expect("policy を消せる");
    fs::create_dir_all(&path).expect("policy の場所を dir にできる");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "src/lib.rs"));
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "policy が読めない周は通さず deny（fail-closed）"
    );
    assert!(out.stdout.is_empty(), "deny でも stdout は 0 byte");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_is_inactive_without_policy_file() {
    let repo = git_repo();
    let state = linked(&repo);
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "anywhere.rs"));
    assert_silent(&out, "policy file が無い周は不活性");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_ignores_bash_tool() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/lib.rs\n");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Bash", "anywhere.rs"));
    assert_silent(&out, "Bash は guard の対象でない（interpreter 経路は v3）");
    clean(&[&repo, &state]);
}

#[test]
fn vessel_init_renders_two_lines_and_writes_state_dir_config() {
    let repo = git_repo();
    let state = linked(&repo);

    let text = fs::read_to_string(repo.join(MARKER)).expect("marker を読める");
    assert!(text.ends_with('\n'), "末尾改行が在る: {text:?}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "marker は 2 行（母集団 {}）", lines.len());
    assert_eq!(lines.first().copied(), Some(format!("name={NAME}").as_str()));
    assert_eq!(
        lines.get(1).copied(),
        Some(format!("version={GENERATION}").as_str())
    );

    let configured = git(&repo, &["config", "--get", &format!("{NAME}.stateDir")]);
    assert_eq!(
        configured,
        state.display().to_string(),
        "state dir は repo の local 設定に在る"
    );
    clean(&[&repo, &state]);
}

#[test]
fn vessel_check_rc2_for_other_name() {
    let repo = git_repo();
    let other = Marker {
        name: "other-vessel".to_owned(),
        version: GENERATION,
    };
    fs::write(repo.join(MARKER), other.render()).expect("marker を書ける");
    let out = run_vessel(&["check", &repo.display().to_string()]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "別の器が名乗る repo は rc 2"
    );

    fs::remove_file(repo.join(MARKER)).expect("marker を消せる");
    let absent = run_vessel(&["check", &repo.display().to_string()]);
    assert_eq!(
        absent.status.code(),
        Some(i32::from(RC_REFUSED)),
        "名乗りが無い repo は rc 1"
    );
    clean(&[&repo]);
}

#[test]
fn vessel_init_refuses_to_overwrite_other_name() {
    let repo = git_repo();
    let other = Marker {
        name: "other-vessel".to_owned(),
        version: GENERATION,
    };
    let before = other.render();
    fs::write(repo.join(MARKER), &before).expect("marker を書ける");
    let state = tmp();

    let out = run_vessel(&[
        "init",
        "--state-dir",
        &state.display().to_string(),
        &repo.display().to_string(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "他の器の repo は奪わない（rc 2）"
    );
    let after = fs::read_to_string(repo.join(MARKER)).expect("marker を読める");
    assert_eq!(after, before, "1 byte も書き換えない");
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
