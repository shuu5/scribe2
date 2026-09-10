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

    // 負例。同じ dir でも**退避物の名前でない**編集は上限に掛かる（口を広く取っていない）。
    let other = repo.join(".claude-session").join("notes.md");
    let out = run_hook_args(
        &["pre-tool-use", "--state-dir", &state.display().to_string()],
        &seat_payload(&repo, "Write", &other.display().to_string(), Some(&script)),
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "退避 dir でも別名の編集は止める: {}",
        stderr_text(&out)
    );
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
