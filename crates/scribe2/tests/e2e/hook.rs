//! `.vessel` marker と hook の歯（設計 docs/design/vessel-hook.md §7）。
//!
//! tmp の git repo を `git init` + commit で作り、`vessel init --state-dir` で置き場を
//! 紐づけてから hook を撃つ。commit には identity が要るので **repo local** の
//! `user.name` / `user.email` を与える（global 設定は 1 byte も触らない）。

use crate::make_tmp_dir;
use crate::seat::{socket_of, start_seat, tmux, IsolatedSeat};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
use vessel::fleet::{json_lite, Registration};
use vessel::hook::vessel::{Marker, GENERATION, MARKER};
use vessel::hook::{guard, inject_path, SCHEMA};
use vessel::name::NAME;
use vessel::seat::brief;
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
    // **畳んだ後の名前を policy が許す**形で撃つ。こうしないと「write-set の外」でも
    // deny になり、root を出たことを一切測らない歯になる（理由まで弁別する）。
    write_policy(&repo, "outside.rs\n");
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
    assert!(
        stderr_text(&out).contains("repo の外"),
        "deny の理由は write-set 違反でなく root 逸脱である: {}",
        stderr_text(&out)
    );
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
fn hook_guard_denies_decoy_payload() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/lib.rs\n");
    // 値が key 名そのもので、本物の key より手前に在る payload。字面の 1 発目を拾う
    // reader だと、guard は本物の編集先でなく decoy の後ろの値を判定してしまう。
    let decoy = format!(
        "{{\"cwd\":\"{}\",\"tool_name\":\"MultiEdit\",\"tool_input\":{{\"edits\":[{{\"old_string\":\"file_path\",\"new_string\":\"src/lib.rs\"}}],\"file_path\":\"docs/other.md\"}}}}",
        repo.display()
    );
    let out = run_hook("pre-tool-use", &decoy);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "decoy を挟んでも本物の file_path を判定する"
    );
    // notebook 側も同じ形で撃つ（file_path key が存在しない tool）。
    let notebook = format!(
        "{{\"cwd\":\"{}\",\"tool_name\":\"NotebookEdit\",\"tool_input\":{{\"cell_id\":\"file_path\",\"cell_type\":\"src/lib.rs\",\"notebook_path\":\"docs/evil.ipynb\"}}}}",
        repo.display()
    );
    let out = run_hook("pre-tool-use", &notebook);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "notebook_path も decoy に迂回されない"
    );
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_resolves_relative_path_against_payload_cwd() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/lib.rs\n");
    // cwd が subdir のとき "src/lib.rs" の実体は <repo>/docs/src/lib.rs であり
    // write-set の外。root 基準で解くと通ってしまう。
    let subdir = repo.join("docs");
    fs::create_dir_all(&subdir).expect("subdir を作れる");
    let out = run_hook("pre-tool-use", &tool_payload(&subdir, "Edit", "src/lib.rs"));
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "相対 path は payload の cwd 基準で解く"
    );
    // 同じ path でも cwd が root なら通る（基準が効いていることの対）。
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "src/lib.rs"));
    assert_silent(&out, "cwd が root なら同じ相対 path は write-set の内側");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_denies_symlink_escape() {
    let repo = git_repo();
    let state = linked(&repo);
    let outside = tmp();
    let docs = repo.join("docs");
    fs::create_dir_all(&docs).expect("docs を作れる");
    std::os::unix::fs::symlink(&outside, docs.join("link")).expect("symlink を張れる");
    write_policy(&repo, "docs/\n");
    // 字句では docs/evil.rs に畳まれて allowlist に当たるが、実体は repo の外。
    let out = run_hook(
        "pre-tool-use",
        &tool_payload(&repo, "Write", "docs/link/../evil.rs"),
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "symlink 経由で repo の外へ出る path は deny"
    );
    clean(&[&repo, &state, &outside]);
}

#[test]
fn hook_guard_denies_symlink_inside_repo_outside_write_set() {
    let repo = git_repo();
    let state = linked(&repo);
    let docs = repo.join("docs");
    fs::create_dir_all(&docs).expect("docs を作れる");
    // repo の**内側**で閉じる symlink。root からは一歩も出ないので「repo の外」の段では
    // 落ちない。字句の docs/link/evil.rs は allowlist に当たるが、実体は src/evil.rs。
    std::os::unix::fs::symlink("../src", docs.join("link")).expect("symlink を張れる");
    write_policy(&repo, "docs/\n");
    let out = run_hook(
        "pre-tool-use",
        &tool_payload(&repo, "Write", "docs/link/evil.rs"),
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "repo 内 symlink 経由でも write-set の外へは書かせない"
    );
    assert!(out.stdout.is_empty(), "deny でも stdout は 0 byte");
    // 理由まで測る。root 逸脱の枝で落ちても rc は同じなので、字面で弁別しないと
    // 「実体で allowlist を当てる」ことを一切測らない歯になる。名指すのも実体側である。
    assert_eq!(
        stderr_text(&out).trim_end(),
        NAME.to_owned() + ": deny src/evil.rs は契約 write-set の外（C16）",
        "deny の 1 行は実体側の path を名指す write-set 違反である"
    );
    // 逆向き（実体が write-set の内側）は通る。「symlink を含む path は一律 deny」という
    // 直し方だとここが赤くなる＝実体で解いていることを弁別する対の歯である。
    std::os::unix::fs::symlink("../docs", repo.join("src").join("into-docs"))
        .expect("symlink を張れる");
    let out = run_hook(
        "pre-tool-use",
        &tool_payload(&repo, "Write", "src/into-docs/new.md"),
    );
    assert_silent(&out, "実体が write-set の内側なら symlink 経由でも通す");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_deny_stderr_is_one_line() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/lib.rs\n");
    // 古い lock を置くと記録側が警告を返す。deny の判定文はそれに濁らされない。
    let lock = inject_path(&state).with_extension("jsonl.lock");
    fs::write(&lock, "").expect("lock を置ける");
    let touched = Command::new("touch")
        .args(["-d", "2020-01-01"])
        .arg(&lock)
        .status()
        .expect("touch を起動できる");
    assert!(touched.success(), "lock の mtime を古くできる");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "docs/x.md"));
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "deny は rc 2");
    assert!(out.stdout.is_empty(), "deny でも stdout は 0 byte");
    assert_eq!(
        stderr_lines(&out),
        1,
        "deny の stderr は 1 行（母集団: {}）",
        stderr_text(&out)
    );
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_denies_when_policy_is_empty() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "\n   \n");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "src/lib.rs"));
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "空の policy は不活性でなく deny（fail-closed）"
    );
    // 理由まで測る。空 allowlist は「どの項目にも当たらない」でも deny になるので、
    // 理由を見ないと「policy が読めない」枝を消しても通る歯になる。
    assert!(
        stderr_text(&out).contains("policy unreadable"),
        "理由は policy を読めないことである: {}",
        stderr_text(&out)
    );
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_normalizes_dot_dot_inside_root() {
    let repo = git_repo();
    let state = linked(&repo);
    // allowlist は畳んだ後の 1 本だけを許す。`..` を畳まないと `src/a/lib.rs` になり
    // deny 側へ倒れるので、この歯は正規化そのものを測る。
    write_policy(&repo, "src/lib.rs\n");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "src/a/../lib.rs"));
    assert_silent(&out, "root の内側で閉じる .. は畳んで通す");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_denies_sibling_of_allowed_dir() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/\n");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "src-other/x.rs"));
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "末尾 / の項目は配下だけを許す（前方一致で兄弟 dir を拾わない）"
    );
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "src/a.rs"));
    assert_silent(&out, "配下は通る");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_allows_bash_when_policy_unreadable() {
    let repo = git_repo();
    let state = linked(&repo);
    let path = write_policy(&repo, "src/lib.rs\n");
    fs::remove_file(&path).expect("policy を消せる");
    fs::create_dir_all(&path).expect("policy の場所を dir にできる");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Bash", "anywhere.rs"));
    assert_silent(&out, "policy が壊れていても Bash は guard の対象でない");
    clean(&[&repo, &state]);
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
fn hook_session_start_honors_state_dir_flag() {
    let repo = git_repo();
    let linked_state = linked(&repo);
    let override_state = tmp();
    let out = run_hook_args(
        &[
            "session-start",
            "--state-dir",
            &override_state.display().to_string(),
        ],
        &payload(&repo),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "仕える周は rc 0");
    assert_eq!(
        inject_lines(&override_state).len(),
        1,
        "--state-dir が置き場を上書きする"
    );
    assert!(
        inject_lines(&linked_state).is_empty(),
        "git 設定の置き場へは書かない"
    );
    clean(&[&repo, &linked_state, &override_state]);
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
fn vessel_check_needs_marker_and_state_dir() {
    let repo = git_repo();
    let mine = Marker {
        name: NAME.to_owned(),
        version: GENERATION,
    };
    fs::write(repo.join(MARKER), mine.render()).expect("marker を書ける");
    let out = run_vessel(&["check", &repo.display().to_string()]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_REFUSED)),
        "marker だけでは仕えない（state dir が紐づいて初めて ByMe）"
    );
    let state = linked(&repo);
    let out = run_vessel(&["check", &repo.display().to_string()]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_OK)),
        "紐づけば ByMe（rc 0）"
    );
    clean(&[&repo, &state]);
}

#[test]
fn vessel_marker_is_two_lf_lines() {
    let repo = git_repo();
    let state = linked(&repo);
    let bytes = fs::read(repo.join(MARKER)).expect("marker を読める");
    // 跨版 面 1 は別実装が byte で読む。LF・順序・末尾改行・余白なしを byte で pin する。
    assert_eq!(
        bytes,
        format!("name={NAME}\nversion={GENERATION}\n").into_bytes(),
        "marker の byte 列: {:?}",
        String::from_utf8_lossy(&bytes)
    );
    clean(&[&repo, &state]);
}

#[test]
fn vessel_init_leaves_no_marker_without_git_repo() {
    let bare = tmp();
    let state = tmp();
    let out = run_vessel(&[
        "init",
        "--state-dir",
        &state.display().to_string(),
        &bare.display().to_string(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "git repo でなければ rc 2"
    );
    assert!(
        !bare.join(MARKER).exists(),
        "設定に失敗した周に marker だけを残さない"
    );
    clean(&[&bare, &state]);
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

/// 使用 token が `used` になる transcript を書き、その path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn transcript_at(dir: &Path, name: &str, body: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, body).expect("transcript を書ける");
    path.display().to_string()
}

/// `used` token ちょうどの有効な usage 行 1 本。
fn usage_jsonl(used: u64) -> String {
    format!(
        "{{\"type\":\"assistant\",\"isSidechain\":false,\"message\":{{\"usage\":{{\"input_tokens\":{used},\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}}}}}}\n"
    )
}

/// 記録 1 行の `what`。
fn what_of(line: &str) -> String {
    match value_of(line, "what") {
        Some(json_lite::Value::Str(found)) => found,
        _ => String::new(),
    }
}

/// **guard と meter が同じ入力で同じ使用率を返す**（出所が 1 本であることの歯）。
///
/// pane（statusline 90%）と transcript（650000 = 宣言窓 1000000 の 65%）を**わざと食い違わせ**、
/// 同じ transcript を 2 面へ渡す。出所が 2 本ある形では meter が pane の 90 を、guard が
/// transcript の 65 を返して割れる——cap 60% が何に対する 60% かを 1 か所で言えない状態である。
#[test]
fn hook_seat_guard_and_meter_agree_on_used_pct() {
    let repo = git_repo();
    let state = linked(&repo);
    let script = transcript_at(&state, "agree.jsonl", &usage_jsonl(650_000));
    let pane = transcript_at(&state, "pane.txt", "❯ \n  90% 900k/1M Opus 5\n");
    let meter = Command::new(bin())
        .arg("seat")
        .args(["meter", "--target", "unused", "--capture-file", &pane, "--transcript", &script])
        .output()
        .expect("binary を起動できる");
    let shown = String::from_utf8_lossy(&meter.stdout).into_owned();
    let target = repo.join("src").join("lib.rs");
    let out = run_hook_args(
        &["pre-tool-use", "--state-dir", &state.display().to_string()],
        &seat_payload(&repo, "Edit", &target.display().to_string(), Some(&script)),
    );
    // **先に deny そのものを見る**。ここを見ないと、guard が通してしまう変異が
    // 「使用率の字面が無い」という取り出しの失敗として落ち、理由を取り違える。
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "65% は cap 以上なので止まる");
    let text = stderr_text(&out);
    // 2 面の使用率を**字面から取り出して**突き合わせる（どちらかを定数で書くと片側しか測れない）。
    let from_meter = shown
        .split("used_pct=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("meter が使用率を出す");
    let from_guard = text
        .split("deny context ")
        .nth(1)
        .and_then(|rest| rest.split('%').next())
        .expect("guard が使用率を名乗る");
    assert_eq!(from_meter, from_guard, "2 面が同じ値を返す: meter={shown} guard={text}");
    assert_eq!(from_meter, "65", "transcript を宣言窓で割った値である: {shown}");
    assert!(!shown.contains("used_pct=90"), "pane の 90% は出ない: {shown}");
}

/// **使用率は切り捨てである**——`59.9001%` の周は止めない（cap 60%）。
///
/// 丸めは load-bearing で、切り上げると **cap 未満の周まで止まる**。1 本の口へ寄せた後は
/// guard と meter が必ず同じだけずれるので、2 面の一致を見る歯では丸めを検出できない
/// （s2-07l.75 lens H-1）。**floor と ceil が割れる点**（599001 / 1000000）を 1 つ置いて、
/// 極性が反転すること自体を測る。
#[test]
fn hook_seat_guard_keeps_the_floor_rounding_at_the_boundary() {
    let repo = git_repo();
    let state = linked(&repo);
    let script = transcript_at(&state, "floor.jsonl", &usage_jsonl(599_001));
    let before = inject_lines(&state).len();
    let target = repo.join("src").join("lib.rs");
    let out = run_hook_args(
        &["pre-tool-use", "--state-dir", &state.display().to_string()],
        &seat_payload(&repo, "Edit", &target.display().to_string(), Some(&script)),
    );
    assert_silent(&out, "59.9001% は cap 未満＝止めない（切り上げるとここが deny になる）");
    assert_eq!(inject_lines(&state).len(), before, "通した周は記録を増やさない");
}

/// 上限**以上**の周は編集を止める（rc 2 + stderr 1 行 + stdout 0 byte + 記録 1 行）。
///
/// 宣言（rules 行）は cap 60% / 窓 1000000 token なので、650000 は 65% で上限以上である。
#[test]
fn hook_seat_guard_denies_edit_at_or_above_cap() {
    let repo = git_repo();
    let state = linked(&repo);
    let script = transcript_at(&state, "big.jsonl", &usage_jsonl(650_000));
    let before = inject_lines(&state).len();
    let target = repo.join("src").join("lib.rs");
    let out = run_hook_args(
        &["pre-tool-use", "--state-dir", &state.display().to_string()],
        &seat_payload(&repo, "Edit", &target.display().to_string(), Some(&script)),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "上限以上は rc 2");
    assert!(out.stdout.is_empty(), "deny の stdout は 0 byte");
    assert_eq!(stderr_lines(&out), 1, "deny の stderr は 1 行");
    let text = stderr_text(&out);
    assert!(text.contains(&format!("{NAME}: deny context ")), "判定文が名乗る: {text}");
    assert!(text.contains("65%"), "実測の使用率を名指す: {text}");
    assert!(text.contains("cap 60%"), "宣言の上限を名指す: {text}");
    let lines = inject_lines(&state);
    assert_eq!(lines.len(), before + 1, "記録は 1 行だけ増える: {lines:?}");
    assert_eq!(
        what_of(&lines[lines.len() - 1]),
        "seat-guard-deny",
        "記録の what: {lines:?}"
    );
    clean(&[&repo, &state]);
}

/// **退避の口は上限に関わらず通す**（止めると席は退避すらできない・FR23）。
///
/// 同じ transcript（65%＝上限以上）で、編集先だけを退避物に替えると通る。負例を兼ねる:
/// 上の歯と差し替わるのは `file_path` 1 つだけなので、退避の口が効いていなければ落ちる。
#[test]
fn hook_seat_guard_lets_externalize_through_above_cap() {
    let repo = git_repo();
    let state = linked(&repo);
    let script = transcript_at(&state, "big.jsonl", &usage_jsonl(650_000));
    let before = inject_lines(&state).len();
    let wm = repo.join(".claude-session").join("working-memory.x.md");
    let out = run_hook_args(
        &["pre-tool-use", "--state-dir", &state.display().to_string()],
        &seat_payload(&repo, "Write", &wm.display().to_string(), Some(&script)),
    );
    assert_silent(&out, "退避の口は上限以上でも通る");
    assert_eq!(
        inject_lines(&state).len(),
        before,
        "通した周は記録も残さない"
    );

    // 負例。口を成す述語を**1 つずつ**外した形は、いずれも上限に掛かる。1 つの負例で
    // 済ませると、他の述語を消す変異が全部生き残る（実測 2026-09-10・lens-383 F1:
    // `notes.md` は長さ照合だけで落ちるので dir 名・前置き・後置き・段数の assert が空虚だった）。
    for (why, rel) in [
        ("dir 名が違う", "docs/working-memory.x.md"),
        ("直下でない（段が深い）", "a/.claude-session/working-memory.x.md"),
        ("前置きが違う", ".claude-session/some-quite-long-name.md"),
        ("後置きが違う", ".claude-session/working-memory.x.txt"),
        ("前置きと後置きが重なる", ".claude-session/working-memory.md"),
        ("退避 dir の別名", ".claude-session/notes.md"),
    ] {
        let other = repo.join(rel);
        let out = run_hook_args(
            &["pre-tool-use", "--state-dir", &state.display().to_string()],
            &seat_payload(&repo, "Write", &other.display().to_string(), Some(&script)),
        );
        assert_eq!(
            out.status.code(),
            Some(i32::from(RC_BROKEN)),
            "{why} は口に入れない（{rel}）: {}",
            stderr_text(&out)
        );
    }
    clean(&[&repo, &state]);
}

/// 上限**未満**の周は 1 byte も書かない（hook budget を毎編集の追記で食い潰さない）。
#[test]
fn hook_seat_guard_is_silent_below_cap() {
    let repo = git_repo();
    let state = linked(&repo);
    let script = transcript_at(&state, "small.jsonl", &usage_jsonl(100_000));
    let before = inject_lines(&state).len();
    let target = repo.join("src").join("lib.rs");
    let out = run_hook_args(
        &["pre-tool-use", "--state-dir", &state.display().to_string()],
        &seat_payload(&repo, "Edit", &target.display().to_string(), Some(&script)),
    );
    assert_silent(&out, "上限未満は黙って通す");
    assert_eq!(
        inject_lines(&state).len(),
        before,
        "上限未満は記録も残さない"
    );
    clean(&[&repo, &state]);
}

/// **測れない周は deny しない**——ただし測れていないことを記録に 1 行残す（FR26 / FR21）。
///
/// 3 通りとも通し、理由を弁別して書く。理由を 1 語へ潰すと「transcript を渡し忘れている」
/// のか「usage の形が変わった」のかを、記録から後で分けられない。
#[test]
fn hook_seat_guard_allows_and_records_when_unmeasured() {
    let repo = git_repo();
    let state = linked(&repo);
    let missing = state.join("nope.jsonl").display().to_string();
    let empty = transcript_at(&state, "empty.jsonl", "{\"type\":\"user\"}\n");
    let target = repo.join("src").join("lib.rs").display().to_string();
    let cases: [(Option<&str>, &str); 3] = [
        (None, "no-transcript-path"),
        (Some(&missing), "unreadable"),
        (Some(&empty), "no-usage"),
    ];
    for (transcript, reason) in cases {
        let before = inject_lines(&state).len();
        let out = run_hook_args(
            &["pre-tool-use", "--state-dir", &state.display().to_string()],
            &seat_payload(&repo, "Edit", &target, transcript),
        );
        assert_silent(&out, &format!("測れない周（{reason}）は通す"));
        let lines = inject_lines(&state);
        assert_eq!(lines.len(), before + 1, "{reason}: 記録は 1 行: {lines:?}");
        assert_eq!(
            what_of(&lines[lines.len() - 1]),
            format!("seat-guard-unmeasured reason={reason}"),
            "{reason}: 理由まで書く: {lines:?}"
        );
    }
    clean(&[&repo, &state]);
}

/// 上限と窓は **manifest の行**が持つ（code に焼かない・憲法 C5 の裁定 id つき）。
///
/// base には行が無く `rules get` が rc 1 になる＝この歯は base で RED である。
#[test]
fn rules_manifest_carries_seat_rows() {
    for (id, want) in [
        ("seat.context_cap_pct", "60"),
        ("seat.context_window_tokens", "1000000"),
    ] {
        let out = Command::new(bin())
            .args(["rules", "get", id])
            .output()
            .unwrap_or_else(|err| panic!("binary を起動できる: {err}"));
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{id} は読める");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            want,
            "{id} の値"
        );
    }
}

/// 境界は「上限**以上**で止める」（`>=` を `>` へ緩める変異はここで落ちる）。
///
/// 600000 / 1000000 = ちょうど 60%。真に超過した周（65%）だけで測ると等号側が空虚になる
/// ——歯の名前が `at_or_above` と名乗っているのに `above` しか撃っていない状態だった
/// （実測 2026-09-10・lens-383 F2）。
#[test]
fn hook_seat_guard_denies_exactly_at_cap() {
    let repo = git_repo();
    let state = linked(&repo);
    let script = transcript_at(&state, "edge.jsonl", &usage_jsonl(600_000));
    let target = repo.join("src").join("lib.rs");
    let out = run_hook_args(
        &["pre-tool-use", "--state-dir", &state.display().to_string()],
        &seat_payload(&repo, "Edit", &target.display().to_string(), Some(&script)),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "ちょうど上限でも止める");
    let text = stderr_text(&out);
    assert!(text.contains("context 60% ≥ cap 60%"), "等号の周の判定文: {text}");
    clean(&[&repo, &state]);
}

/// **`Bash` は見ない**（write-set guard と同じ集合だけを見る）。
///
/// 上限を超えた transcript を添えても `Bash` は通り、記録も残らない。tool 集合の照合を
/// 落とす変異は、ここで `Bash` が deny されて落ちる——上限未満の payload で測ると、
/// その変異は同じく 0 byte を返すので生き残る（実測 2026-09-10・lens-383 F3）。
#[test]
fn hook_seat_guard_ignores_bash_even_above_cap() {
    let repo = git_repo();
    let state = linked(&repo);
    let script = transcript_at(&state, "big.jsonl", &usage_jsonl(650_000));
    let before = inject_lines(&state).len();
    let target = repo.join("src").join("lib.rs");
    let out = run_hook_args(
        &["pre-tool-use", "--state-dir", &state.display().to_string()],
        &seat_payload(&repo, "Bash", &target.display().to_string(), Some(&script)),
    );
    assert_silent(&out, "Bash は上限以上でも見ない");
    assert_eq!(inject_lines(&state).len(), before, "Bash の周は記録も残さない");
    clean(&[&repo, &state]);
}

/// 退避の口は **link で口の外を指す file** を通さない（通す側は実体まで見る）。
///
/// 字句 1 段だけだと、`working-memory.*.md` という名前の symlink を 1 本張るだけで
/// 口の外の file を上限越しに書ける（実測 2026-09-10・lens-383 F4: `src/lib.rs` への
/// link が 65% で rc 0 になった）。
#[test]
fn hook_seat_guard_rejects_externalize_symlink_out_of_mouth() {
    let repo = git_repo();
    let state = linked(&repo);
    let script = transcript_at(&state, "big.jsonl", &usage_jsonl(650_000));
    let session = repo.join(".claude-session");
    fs::create_dir_all(&session).unwrap_or_else(|err| panic!("退避 dir を作れる: {err}"));
    let link = session.join("working-memory.z.md");
    std::os::unix::fs::symlink(repo.join("src").join("lib.rs"), &link)
        .unwrap_or_else(|err| panic!("symlink を張れる: {err}"));
    let out = run_hook_args(
        &["pre-tool-use", "--state-dir", &state.display().to_string()],
        &seat_payload(&repo, "Write", &link.display().to_string(), Some(&script)),
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "口の外を指す link は通さない: {}",
        stderr_text(&out)
    );

    // 負例。**まだ無い**退避物（これから作る周）は link ではありえないので通る
    // ——在ることを口の条件にすると退避そのものが止まる。
    let fresh = session.join("working-memory.fresh.md").display().to_string();
    let out = run_hook_args(
        &["pre-tool-use", "--state-dir", &state.display().to_string()],
        &seat_payload(&repo, "Write", &fresh, Some(&script)),
    );
    assert_silent(&out, "まだ無い退避物は通す");
    clean(&[&repo, &state]);
}

/// `transcript_path` が**空文字**の周は「渡されていない」と同じに扱う。
///
/// 空の口をそのまま path として扱うと `unreadable`（file が壊れている）に化け、記録から
/// 原因を取り違える（実測 2026-09-10・lens-383 F7）。
#[test]
fn hook_seat_guard_treats_empty_transcript_path_as_absent() {
    let repo = git_repo();
    let state = linked(&repo);
    let before = inject_lines(&state).len();
    let target = repo.join("src").join("lib.rs");
    let out = run_hook_args(
        &["pre-tool-use", "--state-dir", &state.display().to_string()],
        &seat_payload(&repo, "Edit", &target.display().to_string(), Some("")),
    );
    assert_silent(&out, "空の transcript_path でも通す");
    let lines = inject_lines(&state);
    assert_eq!(lines.len(), before + 1, "記録は 1 行: {lines:?}");
    assert_eq!(
        what_of(&lines[lines.len() - 1]),
        "seat-guard-unmeasured reason=no-transcript-path",
        "空文字は渡し忘れとして書く（unreadable ではない）: {lines:?}"
    );
    clean(&[&repo, &state]);
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

/// `Bash` の周は **stdout ちょうど 1 行**で deny を返し、記録を 1 行残す。
#[test]
fn hook_permission_request_denies_bash_with_one_json_line() {
    let repo = git_repo();
    let state = linked(&repo);
    let before = inject_lines(&state).len();
    let out = run_hook_args(
        &["permission-request", "--state-dir", &state.display().to_string()],
        &tool_payload(&repo, "Bash", "unused"),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "承認の答えは rc 0");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(text.lines().count(), 1, "stdout はちょうど 1 行: {text}");
    assert!(
        text.contains("\"hookEventName\":\"PermissionRequest\""),
        "どの event への答えかを名乗る: {text}"
    );
    // 入れ子を剥がして **parse できること**まで測る（escape が壊れていれば落ちる）。
    let pairs = json_lite::parse_object(&decision_object(text.trim()))
        .unwrap_or_else(|err| panic!("決定 object を parse できる: {err} / {text}"));
    let behavior = pairs.iter().find(|(key, _)| key == "behavior").map(|(_, v)| v.clone());
    assert_eq!(
        behavior,
        Some(json_lite::Value::Str("deny".to_owned())),
        "一律 deny である: {text}"
    );
    let message = match pairs.iter().find(|(key, _)| key == "message").map(|(_, v)| v.clone()) {
        Some(json_lite::Value::Str(found)) => found,
        other => panic!("message が文字列である: {other:?}"),
    };
    assert!(message.starts_with(&format!("{NAME}: ")), "器が名乗る: {message}");
    assert!(
        message.contains("literal path") && message.contains("$(...)"),
        "次の一手（literal path で書き直す）まで返す: {message}"
    );
    let lines = inject_lines(&state);
    assert_eq!(lines.len(), before + 1, "記録は 1 行だけ増える: {lines:?}");
    // `who` だけを見ると、`what` / `when` を別の値へ書き換える変異が生き残る
    // （実測 2026-09-10・lens-43 M-2）。契約が名指した field は全部測る。
    let last = &lines[lines.len() - 1];
    for (key, want) in [
        ("who", "hook:permission-request"),
        ("what", "deny"),
        ("when", "PermissionRequest"),
    ] {
        assert_eq!(
            value_of(last, key),
            Some(json_lite::Value::Str(want.to_owned())),
            "記録の {key}: {lines:?}"
        );
    }
    assert_eq!(
        value_of(last, "tokens"),
        Some(json_lite::Value::Null),
        "数えていない token は 0 でなく null: {lines:?}"
    );
    clean(&[&repo, &state]);
}

/// `Bash` 以外の問いには答えない（**0 byte・rc 0**＝既定の問いへ戻す・FR24）。
///
/// 答えてしまうと、器が引き受ける筋合いの無い承認まで機械が deny する。
#[test]
fn hook_permission_request_is_silent_for_other_tools() {
    let repo = git_repo();
    let state = linked(&repo);
    let before = inject_lines(&state).len();
    for tool in ["Edit", "Write", "WebFetch", ""] {
        let out = run_hook_args(
            &["permission-request", "--state-dir", &state.display().to_string()],
            &tool_payload(&repo, tool, "unused"),
        );
        assert_silent(&out, &format!("{tool} の問いには答えない"));
    }
    // payload が読めない面も同じく黙る（FR24 の「payload 不能」・lens-43 L-3）。
    // `cwd` を持つが tool 名が無い / 壊れている形を撃つ——`cwd` を落とすと process の
    // cwd へ落ちて別 repo を判定するので、ここで測りたいのは tool 名側の欠落である。
    for (why, payload) in [
        ("tool_name が無い", format!("{{\"cwd\":\"{}\"}}", repo.display())),
        (
            "tool_input だけ在る",
            format!("{{\"cwd\":\"{}\",\"tool_input\":{{}}}}", repo.display()),
        ),
        (
            "JSON として壊れている",
            format!("{{\"cwd\":\"{}\",\"tool_name\"", repo.display()),
        ),
    ] {
        let out = run_hook_args(
            &["permission-request", "--state-dir", &state.display().to_string()],
            &payload,
        );
        assert_silent(&out, &format!("payload 不能（{why}）でも答えない"));
    }
    assert_eq!(
        inject_lines(&state).len(),
        before,
        "答えなかった周は記録も残さない"
    );
    clean(&[&repo, &state]);
}

// flip-check: retroactive s2-07l.47
/// 記録に失敗しても **deny は取り消さず**、失敗を stderr へ出す（FR21 は推奨で判定ではない）。
///
/// `inject.jsonl` を **dir** にすると append が open の時点で落ちる。この周でも stdout は
/// ちょうど 1 行の deny・rc 0 のままで、落ちたことは stderr の行になる——`record_lines` を
/// `let _ = append(…)` へ替える変異は、ここで stderr が 0 行になって落ちる。
#[test]
fn hook_permission_request_surfaces_record_failure_on_stderr() {
    let repo = git_repo();
    let state = linked(&repo);
    // `inject_path` は器と同じ path を組む（file 名の字面を歯へ 2 本目として置かない）。
    fs::create_dir(inject_path(&state)).expect("inject.jsonl を dir として作れる");
    let out = run_hook_args(
        &["permission-request", "--state-dir", &state.display().to_string()],
        &tool_payload(&repo, "Bash", "unused"),
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_OK)),
        "記録が落ちても hook は落ちない"
    );
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(text.lines().count(), 1, "stdout はちょうど 1 行: {text}");
    assert!(
        text.contains("\"behavior\":\"deny\""),
        "記録に失敗しても deny は取り消さない: {text}"
    );
    assert!(
        stderr_lines(&out) >= 1,
        "記録の失敗を黙って消さない: {:?}",
        stderr_text(&out)
    );
    clean(&[&repo, &state]);
}

/// marker の無い repo では **`Bash` でも黙る**（他の器と衝突しない・FR24）。
#[test]
fn hook_permission_request_is_silent_outside_vessel() {
    let repo = git_repo();
    let out = run_hook_args(&["permission-request"], &tool_payload(&repo, "Bash", "unused"));
    assert_silent(&out, "marker の無い repo では答えない");
    clean(&[&repo]);
}

/// 生成物 `hooks/hooks.json` が 3 つ目の entry を持ち、**既存 2 entry は不変**である。
#[test]
fn hooks_json_carries_permission_request_entry() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let body = fs::read_to_string(root.join("hooks").join("hooks.json"))
        .unwrap_or_else(|err| panic!("hooks.json を読める: {err}"));
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
        // PreToolUse の matcher は `.201` で Bash を含む形へ改めた（role guard の Bash 面・設計 seat-roles.md §4）。
        "\"matcher\": \"Bash|Edit|Write|MultiEdit|NotebookEdit\"",
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
    let plugin = fs::read_to_string(dir.join("plugin.json"))
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

// ─────────────────── plugin 同梱の skill（`s2-07l.269`・設計 working-memory.md §6） ───────────────────

/// plugin root（repo root）の `skills/` 直下に置く判断層の skill の dir 名。名前空間は `<NAME>:<dir 名>`（憲法 C2.2）。
const SKILL_DIRS: [&str; 2] = ["rebrief", "ready-compaction"];

/// plugin root の `skills/<name>/SKILL.md` を読む（読めない周は空にせず落とす＝base の機能不在が RED）。
fn skill_body(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("skills")
        .join(name)
        .join("SKILL.md");
    let body = fs::read_to_string(&path);
    assert!(body.is_ok(), "{} を読める: {:?}", path.display(), body.as_ref().err());
    body.unwrap_or_default()
}

/// frontmatter（先頭の `---` … `---`）の `name:` の値。
fn skill_frontmatter_name(body: &str) -> Option<String> {
    let rest = body.strip_prefix("---\n")?;
    let (front, _) = rest.split_once("\n---\n")?;
    front
        .lines()
        .find_map(|line| line.strip_prefix("name:"))
        .map(|value| value.trim().to_owned())
}

/// (a) `skills/` 直下に 2 dir が在り、各 `SKILL.md` の frontmatter `name:` が dir 名と一致する（Claude Code plugin の
/// skill 配置＝自動発見・名前空間 `<NAME>:<dir 名>`）。相互参照は名前空間付き（`/<NAME>:<対の名>`）で書く。
#[test]
fn hook_skill_dirs_exist_with_frontmatter_name_matching_the_dir() {
    for (name, pair) in [(SKILL_DIRS[0], SKILL_DIRS[1]), (SKILL_DIRS[1], SKILL_DIRS[0])] {
        let body = skill_body(name);
        assert_eq!(
            skill_frontmatter_name(&body).as_deref(),
            Some(name),
            "{name}: frontmatter の name は dir 名と同じ"
        );
        assert!(
            body.contains(&format!("/{NAME}:{pair}")),
            "{name}: 対の skill を名前空間付き（`/{NAME}:{pair}`）で名指す"
        );
    }
}

/// (b) 復元の skill は器の口 `seat rebrief` と `seat consume` を、退避の skill は `seat externalize` を本文で名指す
/// （口と skill の対応＝口の名を変えた周に落ちる・SRS FR23）。
#[test]
fn hook_skill_bodies_name_the_vessel_subcommands() {
    for (name, mouths) in [
        ("rebrief", &["seat rebrief", "seat consume"][..]),
        ("ready-compaction", &["seat externalize"][..]),
    ] {
        let body = skill_body(name);
        for mouth in mouths {
            assert!(
                body.contains(&format!("`{mouth}`")),
                "{name}: 器の口 `{mouth}` を本文で名指す"
            );
        }
    }
}

/// (c) 負例＝2 file とも private path 形（home dir・tilde の path 形）を持たない（paths-clean と同じ極性を歯でも pin・PUBLIC 面）。
/// needle は断片から組み立てる（字面を歯の source に置くと paths-clean が歯そのものを撃つ）。
#[test]
fn hook_skill_bodies_carry_no_private_path_marks() {
    for name in SKILL_DIRS {
        let body = skill_body(name);
        assert!(!body.is_empty(), "{name}: 空の本文では不在の検査が空虚になる");
        for mark in [concat!("/", "home", "/"), concat!("~", "/")] {
            assert!(!mark.is_empty(), "needle が空だと不在の検査が空虚になる");
            assert!(!body.contains(mark), "{name}: PUBLIC 面に {mark} を書かない");
        }
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

/// hook の打刻と tick の読みが**同じ dir を見る**（継ぎ目の歯・lens-95 HIGH-1 / HIGH-2）: 独立 socket の席
/// （window は `-n` で名付ける＝運用の契約）で `hook user-prompt-submit` を撃った後、operator が渡す形の
/// `--target session:window` で `seat tick` を撃つと判定行が `state=busy event=UserPromptSubmit`、`hook stop`
/// の後は `state=idle event=Stop` で pointer が注入される。writer と reader の path・潰し・schema のどれかが
/// ずれると `state-missing` / `state-unreadable` に倒れてここで落ちる。
#[test]
fn seat_state_hook_stamp_is_read_by_tick_on_the_same_target() {
    let repo = git_repo();
    let state = linked(&repo);
    let sock_dir = tmp();
    let socket = socket_of(&sock_dir);
    let name = "hookseam";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    let pane = pane_id_of(&socket, name);
    let payload = stamp_payload(&repo, "sid-seam");
    let target = format!("{name}:{name}");
    let wm = sock_dir.join("wm");
    fs::create_dir_all(&wm).expect("wm dir を作れる");
    let pane_file = sock_dir.join("pane.txt");
    fs::write(&pane_file, "\u{276f} \n  10% 100k/1M Opus 5\n").expect("pane fixture を置ける");
    let (wm_s, state_s, pane_s) = (wm.display().to_string(), state.display().to_string(), pane_file.display().to_string());
    let tick = |extra: &[&str]| {
        let mut args = vec!["seat", "tick", "--target", &target, "--wm-dir", &wm_s, "--tmux-socket", &socket, "--state-dir", &state_s];
        args.extend_from_slice(extra);
        Command::new(bin()).args(&args).output().expect("binary を起動できる")
    };
    let suffix = format!(" source=flag state_dir={}", state.display());

    let out = run_hook_args(&["user-prompt-submit", "--pane", &pane, "--tmux-socket", &socket], &payload);
    assert_silent(&out, "打刻");
    let out = tick(&["--capture-file", &pane_s]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stderr={}", stderr_text(&out));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("seat: tick decision=noop reason=busy context=10 state=busy event=UserPromptSubmit{suffix}\n"),
        "hook の打刻（Busy）を tick が同じ dir で読む"
    );

    let out = run_hook_args(&["stop", "--pane", &pane, "--tmux-socket", &socket], &payload);
    assert_silent(&out, "打刻");
    let out = tick(&["--capture-file", &pane_s]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stderr={}", stderr_text(&out));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("seat: tick decision=inject target={name}_{name} consumed=false kind=pointer context=10 state=idle event=Stop{suffix}\n"),
        "hook の打刻（Idle）で tick が pointer を注入する（`sh -i` の席は submit で打刻しない＝consumed=false・`s2-07l.112`）"
    );
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

/// 生成物 `hooks/hooks.json` は打刻の 2 entry（UserPromptSubmit / Stop）を持ち、5 つの command 行が
/// すべて `--pane "$TMUX_PANE"` を受ける（打刻の席と記録の `seat` 列・`s2-07l.150`）。
#[test]
fn seat_state_hooks_json_carries_stamp_entries() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let body = fs::read_to_string(root.join("hooks").join("hooks.json"))
        .unwrap_or_else(|err| panic!("hooks.json を読める: {err}"));
    let pane_arg = " --pane \\\"$TMUX_PANE\\\"";
    for needle in ["\"UserPromptSubmit\"", "\"Stop\"", "hook user-prompt-submit", "hook stop"] {
        assert_eq!(body.matches(needle).count(), 1, "{needle} はちょうど 1 回: {body}");
    }
    assert_eq!(body.matches(pane_arg).count(), 5, "5 行すべてが pane id を受ける: {body}");
    for sub in ["session-start", "user-prompt-submit", "stop"] {
        assert_eq!(body.matches(&format!("hook {sub}{pane_arg}")).count(), 1, "{sub} の command 行に --pane");
    }
    assert_eq!(body.matches("\"type\": \"command\"").count(), 5, "entry は 5 つ: {body}");
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
    let (guard, pane) = role_seat(&place, name, Some("planner"));
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
        let added = if roled { 2 } else { 1 };
        assert_eq!(lines.len(), before + added, "{label}: 記録は {added} 行増える（母集団 {}）: {lines:?}", lines.len());
        let line = &lines[before];
        assert_eq!(what_of(line), "seat-guard-unmeasured reason=no-transcript-path", "{label}");
        assert_attributed(line, expected, label);
        if roled {
            assert!(what_of(&lines[before + 1]).starts_with("role-"), "{label}: role の記録が後ろに並ぶ: {lines:?}");
        }
    }
    drop(guard);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// cap 以上の編集は従来どおり止め（rc 2 + stderr 1 行 + stdout 0 byte）、記録は席を名乗る。
#[test]
fn seat_attrib_hook_cap_deny_records_the_seat() {
    let repo = git_repo();
    let state = linked(&repo);
    let sock_dir = tmp();
    let socket = socket_of(&sock_dir);
    let name = "hookattribcap";
    let guard = start_seat(&socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    let pane = pane_id_of(&socket, name);
    let script = transcript_at(&state, "big.jsonl", &usage_jsonl(650_000));
    let target = repo.join("src").join("lib.rs").display().to_string();
    let before = inject_lines(&state).len();
    let out = run_hook_args(
        &["pre-tool-use", "--state-dir", &state.display().to_string(), "--pane", &pane, "--tmux-socket", &socket],
        &seat_payload(&repo, "Edit", &target, Some(&script)),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "上限以上は rc 2");
    assert!(out.stdout.is_empty(), "deny の stdout は 0 byte");
    assert_eq!(stderr_lines(&out), 1, "deny の stderr は 1 行（席の解決は判定文を濁さない）: {}", stderr_text(&out));
    let lines = inject_lines(&state);
    assert_eq!(lines.len(), before + 1, "記録は 1 行だけ増える: {lines:?}");
    let line = &lines[lines.len() - 1];
    assert_eq!(what_of(line), "seat-guard-deny");
    assert_attributed(line, Some(&format!("{name}_{name}")), "cap 以上");
    drop(guard);
    clean(&[&repo, &state, &sock_dir]);
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

/// 生成物 `hooks/hooks.json` の **5 entry すべて**が `--pane "$TMUX_PANE"` を渡す（記録の席の出所）。
#[test]
fn seat_attrib_hook_every_hooks_json_entry_passes_the_pane() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let body = fs::read_to_string(root.join("hooks").join("hooks.json"))
        .unwrap_or_else(|err| panic!("hooks.json を読める: {err}"));
    let pane_arg = " --pane \\\"$TMUX_PANE\\\"";
    let entries = body.matches("\"type\": \"command\"").count();
    assert_eq!(entries, 5, "entry は 5 つ（母集団）: {body}");
    for sub in ["session-start", "pre-tool-use", "permission-request", "user-prompt-submit", "stop"] {
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
    repo: PathBuf,
    /// 置き場（`vessel init --state-dir`）。
    state: PathBuf,
    /// 独立 socket と fixture を置く dir。
    sock_dir: PathBuf,
    /// 独立 socket の path。
    socket: String,
    /// 登録の雛形の path。
    launch: String,
    /// fixture の rules manifest の path（役割ごとの行 2 つ）。
    rules: String,
}

/// planner の権能（裁定 `user 2026-09-13T03:14Z` の値に、裁定 `user 2026-09-13T12:04Z` の `edit-outside` を足したもの）。
const PLANNER_CAPS: &[&str] =
    &["answer", "approve", "go", "relay", "edit-contract", "edit-design-intent", "edit-design-doc", "edit-outside"];

/// 管理席の権能（同じ 2 つの裁定）。
const ADMIN_CAPS: &[&str] = &["launch", "relay", "merge", "edit-outside"];

/// 役割ごとの行を持つ rules manifest の本文（`admin` が `None` なら管理席の行を置かない）。
fn role_rules_text(planner: &[&str], admin: Option<&[&str]>) -> String {
    let row = |id: &str, names: &[&str]| {
        let quoted: Vec<String> = names.iter().map(|name| format!("\"{name}\"")).collect();
        format!(
            "\n[[rule]]\nid = \"{id}\"\nkind = \"RoleCapabilities\"\nvalue = [{}]\nenabled = true\nruling = \"r\"\nruled_at = \"2026-09-13\"\n",
            quoted.join(", ")
        )
    };
    let mut text = format!("schema = 1\n{}", row("role.planner", planner));
    if let Some(names) = admin {
        text.push_str(&row("role.admin", names));
    }
    text
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
    fs::write(&rules, role_rules_text(PLANNER_CAPS, Some(ADMIN_CAPS))).expect("rules を書ける");
    RolePlace {
        repo,
        state,
        sock_dir,
        socket,
        launch: launch.display().to_string(),
        rules: rules.display().to_string(),
    }
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

/// (1)(2)(12): 管理席の target から `pipe answer` を含む Bash → deny（deny 文に権能を持つ役割の名と rules 行 id・
/// 記録 1 行）／planner の target から同じ command → allow（記録 1 行）。`--project` 無し + cwd = anchor の形
/// （旧 hooks.json）でも cwd で解けて同じ deny（互換）。埋め込み manifest（`--rules` 無し）でも同じ。
#[test]
fn hook_role_bash_face_denies_admin_and_allows_planner_for_answer() {
    let place = role_place();
    let (admin, admin_pane) = role_seat(&place, "roleadmin", Some("admin"));
    let (planner, planner_pane) = role_seat(&place, "roleplanner", Some("planner"));
    let payload = bash_payload(&place.repo, &answer_line());

    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &admin_pane, &[], &payload);
    let text = assert_role_deny(&out, "管理席の answer");
    assert!(text.starts_with(&format!("{NAME}: ")), "器が名乗る: {text}");
    assert!(text.contains("answer"), "権能を名指す: {text}");
    assert!(text.contains("planner 席の権能（rules 行 role.planner）"), "権能を持つ役割の名と rules 行 id: {text}");
    assert!(text.contains("admin 席は持たない"), "自席の役割: {text}");
    assert_role_record(&place.state, before, "role-deny capability=answer", "roleadmin_roleadmin");

    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &planner_pane, &[], &payload);
    assert_silent(&out, "planner の answer は通す");
    assert_role_record(&place.state, before, "role-allow capability=answer", "roleplanner_roleplanner");

    // 埋め込み manifest（tracked の `role.*` の行）でも同じ判定＝裁定の値が binary に在る。
    let out = run_hook_args(&["pre-tool-use", "--pane", &admin_pane, "--tmux-socket", &place.socket], &payload);
    let text = assert_role_deny(&out, "埋め込み manifest でも管理席の answer は deny");
    assert!(text.contains("role.planner"), "{text}");
    let out = run_hook_args(&["pre-tool-use", "--pane", &planner_pane, "--tmux-socket", &place.socket], &payload);
    assert_silent(&out, "埋め込み manifest でも planner の answer は通す");
    drop(admin);
    drop(planner);
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

/// (5)(7)(10): Edit 系は path 種別で判定する: 管理席の code path → deny・planner の design-intent → allow・
/// planner の code path → deny・`README.md`（Code）は両役割とも deny（裁定: code は誰も持たない）・
/// `docs/design/x.md` は planner だけ allow。権能付きでない Bash（`ls`）は通す（記録なし・write-set の policy が
/// 在っても Bash は write-set guard に届かない）。
#[test]
fn hook_role_edit_face_classifies_path_kind_per_role() {
    let place = role_place();
    let (admin, admin_pane) = role_seat(&place, "roleeditadmin", Some("admin"));
    let (planner, planner_pane) = role_seat(&place, "roleeditplanner", Some("planner"));
    let edit = |file: &str| tool_payload(&place.repo, "Edit", file);

    let before = role_records(&place.state).len();
    let text = assert_role_deny(&run_role_hook(&place, &admin_pane, &[], &edit("src/lib.rs")), "管理席の code");
    assert!(text.contains("edit-code"), "種別の権能を名指す: {text}");
    assert_role_record(&place.state, before, "role-deny path=code", "roleeditadmin_roleeditadmin");

    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &planner_pane, &[], &edit("design-intent/spec/srs.html"));
    assert_silent(&out, "planner の design-intent は通す");
    assert_role_record(&place.state, before, "role-allow path=design-intent", "roleeditplanner_roleeditplanner");

    let text = assert_role_deny(&run_role_hook(&place, &planner_pane, &[], &edit("src/lib.rs")), "planner の code");
    assert!(text.contains("edit-code") && text.contains("どの役割の席の権能でもない"), "{text}");
    for (pane, why) in [(&admin_pane, "管理席"), (&planner_pane, "planner")] {
        assert_role_deny(&run_role_hook(&place, pane, &[], &edit("README.md")), &format!("{why} の README.md（Code）"));
    }
    assert_silent(&run_role_hook(&place, &planner_pane, &[], &edit("docs/design/x.md")), "planner の設計 doc は通す");
    let text = assert_role_deny(&run_role_hook(&place, &admin_pane, &[], &edit("docs/design/x.md")), "管理席の設計 doc");
    assert!(text.contains("planner 席の権能（rules 行 role.planner）"), "{text}");
    // 絶対 path も root 相対へ畳んで同じ種別（Write / NotebookEdit も同じ面）。
    let absolute = place.repo.join("design-intent").join("x.html").display().to_string();
    assert_silent(&run_role_hook(&place, &planner_pane, &[], &tool_payload(&place.repo, "Write", &absolute)), "絶対 path");
    let notebook = format!(
        "{{\"cwd\":\"{}\",\"tool_name\":\"NotebookEdit\",\"tool_input\":{{\"notebook_path\":\"src/x.ipynb\"}}}}",
        place.repo.display()
    );
    assert_role_deny(&run_role_hook(&place, &planner_pane, &[], &notebook), "notebook の code");

    // 権能付きでない Bash は両役割とも通し、記録も残さない（tmux も event log も撃たない・NFR5）。
    write_policy(&place.repo, "src/lib.rs\n");
    let before = role_records(&place.state).len();
    for pane in [&admin_pane, &planner_pane] {
        let out = run_role_hook(&place, pane, &[], &bash_payload(&place.repo, "ls -la"));
        assert_silent(&out, "ls は権能付きでない");
        let out = run_role_hook(&place, pane, &[], &bash_payload(&place.repo, &format!("{NAME} pipe show --run r")));
        assert_silent(&out, "pipe show は権能付きでない");
    }
    assert_eq!(role_records(&place.state).len(), before, "権能付きでない Bash は記録を残さない");
    // policy が在る周の write-set guard は従来どおり効く（Edit は先に write-set guard が止める）。
    let text = stderr_text(&run_role_hook(&place, &planner_pane, &[], &edit("docs/design/x.md")));
    assert!(text.contains("write-set の外"), "write-set guard が先に止める: {text}");
    drop(admin);
    drop(planner);
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

/// (6・AC16): 契約の印 `opens = ["code"]` で開いた便の write-set の内側 → 管理席でも allow（記録は `opened`）・
/// 外 → deny・印の無い便は deny・印が別の種別（`design-doc`）なら code は deny。
#[test]
fn hook_role_contract_mark_opens_bead_write_set_for_admin() {
    let place = role_place();
    let (admin, admin_pane) = role_seat(&place, "roleopenadmin", Some("admin"));
    write_run_contract(&place.state, "run-open", "opens = [\"code\"]\n");
    write_run_contract(&place.state, "run-plain", "");
    write_run_contract(&place.state, "run-doc", "opens = [\"design-doc\"]\n");
    let in_bead = |run: &str, rel: &str| {
        let path = place.repo.join(".worktrees").join(NAME).join(run).join(rel);
        tool_payload(&place.repo, "Edit", &path.display().to_string())
    };

    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &admin_pane, &[], &in_bead("run-open", "src/lib.rs"));
    assert_silent(&out, "印で開いた便の write-set の内側は管理席でも通す");
    assert_role_record(&place.state, before, "role-allow path=code opened", "roleopenadmin_roleopenadmin");
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

/// `s2-07l.227`: 便の器でない worktree（`.worktrees/planner-x/`・planner の docs PR 用の木）は repo の写し＝
/// worktree 相対で分類する: planner の `design-intent/` → allow・同じ木の `src/` → deny（edit-code）・便の worktree の
/// `src/` は印の無い周 deny のまま。
#[test]
fn hook_role_worktree_repo_copy_allows_planner_design_intent() {
    let place = role_place();
    let (planner, planner_pane) = role_seat(&place, "roletreeplanner", Some("planner"));
    let under = |parts: &[&str]| {
        let path = parts.iter().fold(place.repo.join(".worktrees"), |dir, part| dir.join(part));
        tool_payload(&place.repo, "Edit", &path.display().to_string())
    };

    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &planner_pane, &[], &under(&["planner-x", "design-intent", "x.html"]));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc 0: {}", stderr_text(&out));
    assert_silent(&out, "planner の worktree の design-intent は通す");
    assert_role_record(&place.state, before, "role-allow path=design-intent", "roletreeplanner_roletreeplanner");
    assert_silent(
        &run_role_hook(&place, &planner_pane, &[], &under(&["planner-x", "docs", "design", "a.md"])),
        "planner の worktree の設計 doc も通す",
    );

    let text = assert_role_deny(&run_role_hook(&place, &planner_pane, &[], &under(&["planner-x", "src", "x.rs"])), "同じ木の code");
    assert!(text.contains("edit-code"), "{text}");
    let text = assert_role_deny(&run_role_hook(&place, &planner_pane, &[], &under(&[NAME, "run-plain", "src", "x.rs"])), "便の worktree の code");
    assert!(text.contains("edit-code"), "{text}");
    drop(planner);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (8)(9): rules 行が読めない・行に無い役割 → deny（FailClosed・理由を名指す）／1 行に `pipe answer` と
/// `pipe run` が並ぶ Bash は両方の権能が要る（planner も管理席も deny・記録の種別は両方の名）。
#[test]
fn hook_role_fails_closed_on_missing_rows_and_requires_every_capability_on_the_line() {
    let place = role_place();
    let (admin, admin_pane) = role_seat(&place, "rolerowadmin", Some("admin"));
    let (planner, planner_pane) = role_seat(&place, "rolerowplanner", Some("planner"));
    let launch = bash_payload(&place.repo, &format!("{NAME} pipe run --run r --repo ."));
    assert_silent(&run_role_hook(&place, &admin_pane, &[], &launch), "管理席の起動は通す（行が読める周の対）");

    // 管理席の行が無い manifest → 権能なし。
    let no_admin = place.sock_dir.join("no-admin.toml");
    fs::write(&no_admin, role_rules_text(PLANNER_CAPS, None)).unwrap_or_else(|err| panic!("{err}"));
    let args = ["pre-tool-use", "--pane", &admin_pane, "--tmux-socket", &place.socket, "--rules", &no_admin.display().to_string()];
    let text = assert_role_deny(&run_hook_args(&args, &launch), "行の無い役割");
    assert!(text.contains("reason=no-row role.admin"), "{text}");
    // 読めない manifest（無い file）→ 権能なし。
    let missing = place.sock_dir.join("nope.toml").display().to_string();
    let args = ["pre-tool-use", "--pane", &admin_pane, "--tmux-socket", &place.socket, "--rules", &missing];
    let text = assert_role_deny(&run_hook_args(&args, &launch), "読めない manifest");
    assert!(text.contains("reason=rules-unreadable"), "{text}");
    // 不発効の行も権能なし（値は写すが機械は効かせない）。
    let disabled = place.sock_dir.join("disabled.toml");
    let body = role_rules_text(PLANNER_CAPS, Some(ADMIN_CAPS)).replace("enabled = true", "enabled = false");
    fs::write(&disabled, body).unwrap_or_else(|err| panic!("{err}"));
    let args = ["pre-tool-use", "--pane", &admin_pane, "--tmux-socket", &place.socket, "--rules", &disabled.display().to_string()];
    let text = assert_role_deny(&run_hook_args(&args, &launch), "不発効の行");
    assert!(text.contains("reason=no-row role.admin"), "{text}");

    // 2 つの権能付き subcommand が並ぶ行は両方が要る。
    let both = bash_payload(&place.repo, &format!("{NAME} pipe answer --run r --words \"ok\" && {NAME} pipe run --run r"));
    let before = role_records(&place.state).len();
    let text = assert_role_deny(&run_role_hook(&place, &planner_pane, &[], &both), "planner は launch を持たない");
    assert!(text.contains("launch") && !text.contains("（answer"), "欠けた権能だけを名指す: {text}");
    assert_role_record(&place.state, before, "role-deny capability=answer+launch", "rolerowplanner_rolerowplanner");
    let text = assert_role_deny(&run_role_hook(&place, &admin_pane, &[], &both), "管理席は answer を持たない");
    assert!(text.contains("answer") && !text.contains("（launch"), "欠けた権能だけを名指す: {text}");
    drop(admin);
    drop(planner);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (11)(13)(14): anchor は payload の `cwd` でなく `--project` から解く（席が `cd` しても guard は外れない・run 2
/// の穴）: cwd = repo の外かつ `--project` = anchor で登録済みの管理席の `pipe answer` → deny と記録 1 行／
/// `--pane` 無しかつ cwd = repo の外 → 0 byte・rc 0（FR24 の沈黙は pane 無しだけ）／`--pane` 在りかつ
/// `--project` = repo でない dir なら権能付き Bash は deny・stderr 1 行（`--state-dir` が在れば記録 1 行）・`ls` は通す。
#[test]
fn hook_role_anchor_comes_from_project_not_cwd() {
    let place = role_place();
    let (admin, admin_pane) = role_seat(&place, "roleanchoradmin", Some("admin"));
    let outside = tmp();
    let payload = bash_payload(&outside, &answer_line());
    let project = place.repo.display().to_string();

    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &admin_pane, &["--project", &project], &payload);
    let text = assert_role_deny(&out, "cd で repo の外に居ても anchor から解けて deny");
    assert!(text.contains("role.planner"), "{text}");
    assert_role_record(&place.state, before, "role-deny capability=answer", "roleanchoradmin_roleanchoradmin");
    // 同じ形で権能付きでない Bash と Edit（repo の外の path＝Outside・管理席は edit-outside を持つ＝裁定 12:04Z）。
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
    let out = run_role_hook(&place, &admin_pane, &["--project", ""], &bash_payload(&place.repo, &answer_line()));
    assert!(stderr_text(&out).contains("role.planner"), "空の --project は cwd で解く: {}", stderr_text(&out));
    // 置き場を明示すれば anchor の解けない周も記録 1 行を残す。
    let state_s = place.state.display().to_string();
    let before = role_records(&place.state).len();
    let out = run_role_hook(&place, &admin_pane, &["--project", &bare_s, "--state-dir", &state_s], &payload);
    assert_role_deny(&out, "anchor の解けない席（置き場つき）");
    assert_role_record(&place.state, before, "role-deny capability=answer", "roleanchoradmin_roleanchoradmin");
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
    let (planner, admin) = if caps == PLANNER_CAPS { (without.as_slice(), ADMIN_CAPS) } else { (PLANNER_CAPS, without.as_slice()) };
    let stripped = place.sock_dir.join("no-outside.toml");
    fs::write(&stripped, role_rules_text(planner, Some(admin))).expect("rules を書ける");
    let args = ["pre-tool-use", "--pane", pane, "--tmux-socket", &place.socket, "--rules", &stripped.display().to_string()];
    let text = assert_role_deny(&run_hook_args(&args, &edit), "edit-outside の無い行");
    assert!(text.contains("edit-outside"), "種別の権能を名指す: {text}");
    // repo の内の種別の判定は変わらない（code は持たない）。
    assert_role_deny(&run_role_hook(place, pane, &[], &tool_payload(&place.repo, "Edit", "src/lib.rs")), "repo 内の code");
    clean(&[&outside]);
}

/// 裁定 `user 2026-09-13T12:04Z`: 登録済み planner 席の repo の外（dispatch file の state dir・auto-memory）への
/// Edit / Write は allow（`PathKind::Outside` → `edit-outside`）。
#[test]
fn hook_role_outside_edit_is_allowed_for_planner() {
    let place = role_place();
    let (planner, planner_pane) = role_seat(&place, "roleoutplanner", Some("planner"));
    assert_outside_edit_allowed_for(&place, &planner_pane, "roleoutplanner_roleoutplanner", PLANNER_CAPS);
    drop(planner);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// 裁定 `user 2026-09-13T12:04Z`: 登録済み管理席の repo の外（`.claude-session/`・scratchpad）への Edit / Write は allow。
#[test]
fn hook_role_outside_edit_is_allowed_for_admin() {
    let place = role_place();
    let (admin, admin_pane) = role_seat(&place, "roleoutadmin", Some("admin"));
    assert_outside_edit_allowed_for(&place, &admin_pane, "roleoutadmin_roleoutadmin", ADMIN_CAPS);
    drop(admin);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// command 行の中の escape（`"` / `\`）の後ろに在る subcommand も見落とさない（`field` の字面読みは escape
/// された `"` で切れる＝fail-open だった形）。
#[test]
fn hook_role_reads_the_command_line_through_json_escapes() {
    let place = role_place();
    let (admin, admin_pane) = role_seat(&place, "roleescapeadmin", Some("admin"));
    let line = format!("echo \"hi\\\\there\" && {NAME} pipe answer --run r --words \"ok\"");
    let out = run_role_hook(&place, &admin_pane, &[], &bash_payload(&place.repo, &line));
    let text = assert_role_deny(&out, "escape の後ろの answer");
    assert!(text.contains("role.planner"), "{text}");
    drop(admin);
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

/// session-start を撃ち（rc 0・stderr 0 byte）、名乗りの 1 行を確かめて**その後ろの行**を返す。
fn brief_lines(place: &RolePlace, pane: &str, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["session-start", "--pane", pane, "--tmux-socket", &place.socket];
    args.extend_from_slice(extra);
    let out = run_hook_args(&args, &stamp_payload(&place.repo, "sid-brief"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "session-start は rc 0: {}", stderr_text(&out));
    assert_eq!(stderr_text(&out), "", "指示文を出せる周は stderr 0 byte");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut lines = stdout.lines();
    let header = lines.next().unwrap_or_default();
    assert!(header.starts_with(&format!("[{NAME}/SessionStart] served version=")), "名乗りは 1 行目のまま: {stdout}");
    lines.map(str::to_owned).collect()
}

/// (a) 登録済みの target の SessionStart で生成文が名乗りの後ろに出て、権能の名がすべて含まれる（planner / admin の
/// 2 fixture）。生成文は `render`（雛形の穴に登録 row の target / anchor と fixture の rules 行の値）と**同じ字面**で、
/// 記録は名乗り + 指示文の 2 行（指示文の `bytes` は生成文の byte 数・席を名乗る）。`--rules` 無し（埋め込み manifest）
/// でも同じ経路で出る＝裁定の値が binary に在る。
#[test]
fn hook_brief_session_start_emits_the_role_brief_with_every_capability() {
    let place = role_place();
    let embedded = vessel::rules::manifest::Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest: {errors:?}"));
    for (name, role, caps) in [("briefplanner", Role::Planner, PLANNER_CAPS), ("briefadmin", Role::Admin, ADMIN_CAPS)] {
        let (seat, pane) = role_seat(&place, name, Some(role.as_str()));
        let target = format!("{name}:{name}");
        let registration = brief_registration(role, &target, &place.repo.display().to_string());
        let before = inject_lines(&place.state).len();
        let body = brief_lines(&place, &pane, &["--rules", &place.rules]);
        let expected = brief::render(role, &registration, &brief_caps(caps));
        assert_eq!(format!("{}\n", body.join("\n")), expected, "{name}: 生成文は render と同じ字面");
        assert!(body.len() >= 5, "{name}: 設計 §5 の項目を持つ: {body:?}");
        for cap in caps {
            assert!(body.iter().any(|line| line.contains(cap)), "{name}: 権能 {cap} の名が生成文に現れる: {body:?}");
        }
        assert!(body.iter().any(|line| line.contains(&target) && line.contains(&place.repo.display().to_string())), "{name}: target と anchor の穴: {body:?}");
        let lines = inject_lines(&place.state);
        assert_eq!(lines.len(), before + 2, "{name}: 記録は名乗り + 指示文の 2 行: {lines:?}");
        let last = lines.last().cloned().unwrap_or_default();
        assert_eq!(what_of(&last), "session-start-brief", "{last}");
        assert_eq!(value_of(&last, "bytes"), Some(json_lite::Value::Num(expected.len() as u64)), "bytes は生成文の byte 数: {last}");
        assert_attributed(&last, Some(&format!("{name}_{name}")), "指示文の記録");
        // 埋め込み manifest（`--rules` 無し）でも同じ経路。
        let held = brief::capabilities_of(&embedded, role).unwrap_or_else(|| panic!("{name}: 埋め込みに行が在る"));
        let body = brief_lines(&place, &pane, &[]);
        assert_eq!(format!("{}\n", body.join("\n")), brief::render(role, &registration, &held), "{name}: 埋め込みの行の値");
        drop(seat);
    }
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
    let (seat, pane) = role_seat(&place, "briefbroken", Some("admin"));
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
    let planner_only = place.sock_dir.join("planner-only.toml");
    fs::write(&planner_only, role_rules_text(PLANNER_CAPS, None)).unwrap_or_else(|err| panic!("rules: {err}"));
    refused(&["--rules", &planner_only.display().to_string()], "no-row role.admin");
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
fn hook_brief_planner_external_form() {
    let registration = brief_registration(Role::Planner, "fixture:planner", "/srv/anchor");
    insta::assert_snapshot!("hook_brief_planner", brief::render(Role::Planner, &registration, &brief_caps(PLANNER_CAPS)));
}

/// (c) 管理席の生成文の外形 snapshot。
#[test]
fn hook_brief_admin_external_form() {
    let registration = brief_registration(Role::Admin, "fixture:admin", "/srv/anchor");
    insta::assert_snapshot!("hook_brief_admin", brief::render(Role::Admin, &registration, &brief_caps(ADMIN_CAPS)));
}
