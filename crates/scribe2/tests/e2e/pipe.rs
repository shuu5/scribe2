//! 縦 1 本 (a) / (b) の歯（設計 docs/design/pipeline.md §8 (a) / (b)）。
//!
//! tmp の git repo を作り `vessel init --state-dir` で置き場を紐づけてから撃つ。
//! runner は `sh -c` の 1 行の fake で、実 Claude は (d) の手番である。
//! commit には identity が要るので **repo local** の設定を与える（global は触らない）。

use crate::make_tmp_dir;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
use vessel::fleet::{Event, EventKind, Stage};
use vessel::hook::inject_path;
use vessel::pipe::approve::RC_BLOCKED;
use vessel::pipe::land;

/// binary の path。
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_scribe2")
}

/// tmp dir を 1 つ作り、symlink を解いた path を返す。
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
    out.status.success().then_some(text).expect("git が rc 0 で終わる")
}

/// commit を 1 つ持つ tmp の git repo と、紐づけた置き場を作る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn repo_with_state() -> (PathBuf, PathBuf) {
    let repo = tmp();
    // 設計 §5.4 の land は `refs/heads/main` を進める。`git init` の既定 branch 名は
    // 環境依存（多くの host で `master`）なので、**test 側で main を明示する**。
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "e2e"]);
    git(&repo, &["config", "user.email", "e2e@example.invalid"]);
    fs::create_dir_all(repo.join("src")).expect("src dir を作れる");
    fs::write(repo.join("src").join("lib.rs"), "// seed\n").expect("seed を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "seed"]);
    let state = tmp();
    let out = Command::new(bin())
        .args(["vessel", "init", "--state-dir"])
        .arg(&state)
        .arg(&repo)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "vessel init は rc 0");
    (repo, state)
}

/// `pipe` を binary で 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_pipe(args: &[&str]) -> Output {
    Command::new(bin())
        .arg("pipe")
        .args(args)
        .output()
        .expect("binary を起動できる")
}

/// 正しく書けた契約 file の本文。差し替えたい行だけ上書きして使う。
fn contract_body() -> Vec<String> {
    [
        r#"goal = "縦 1 本を通す""#,
        r#"done = "run が Implemented になる""#,
        r#"size = "S""#,
        r#"owner = "s2-2e5""#,
        r#"disposition = "A-now""#,
        r#"write-set = ["src/lib.rs"]"#,
        r#"verify = ["true"]"#,
        r#"req = ["FR4"]"#,
        r#"design = "docs/design/pipeline.md""#,
    ]
    .iter()
    .map(|line| (*line).to_owned())
    .collect()
}

/// 契約 file を書き、その path を返す。`drop` の行を落とし `add` の行を足す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_contract(dir: &Path, drop: &[&str], add: &[&str]) -> PathBuf {
    let mut lines: Vec<String> = contract_body()
        .into_iter()
        .filter(|line| !drop.iter().any(|key| line.starts_with(key)))
        .collect();
    lines.extend(add.iter().map(|line| (*line).to_owned()));
    let path = dir.join("contract.toml");
    fs::write(&path, format!("{}\n", lines.join("\n"))).expect("契約 file を書ける");
    path
}

/// stdout の全文。
fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// stderr の全文。
fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// `run=<id>` の 1 行から id を取る。
fn run_id_of(out: &Output) -> String {
    stdout_of(out)
        .lines()
        .find_map(|line| line.strip_prefix("run=").map(str::to_owned))
        .unwrap_or_default()
}

/// intake を 1 回通して run id を返す。
fn intake(repo: &Path, state: &Path, contract: &Path) -> String {
    intake_bead(repo, state, contract, "s2-2e5")
}

/// bead を選んで intake を 1 回通す。**run id は `<bead>-<秒>`** なので、同じ秒に
/// 2 便を起こす歯は bead を分ける（同 bead だと id が衝突して 2 便目が断られる）。
fn intake_bead(repo: &Path, state: &Path, contract: &Path, bead: &str) -> String {
    let out = run_pipe(&[
        "intake",
        "--contract",
        &contract.display().to_string(),
        "--bead",
        bead,
        "--repo",
        &repo.display().to_string(),
        "--state-dir",
        &state.display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "intake は rc 0: {}", stderr_of(&out));
    run_id_of(&out)
}

/// 後片付け。
fn clean(dirs: &[&Path]) {
    for dir in dirs {
        fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn pipe_intake_rejects_missing_field() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["goal", "size"], &[]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "欠落を通さない");
    let err = stderr_of(&out);
    // **全件集めて返す**（1 件目で止めない）。落とした 2 本がどちらも出る。
    assert!(err.contains("goal"), "goal の欠落: {err}");
    assert!(err.contains("size"), "size の欠落: {err}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_rejects_multiline_verify() {
    let (repo, state) = repo_with_state();
    let path = repo.join("multiline.toml");
    let mut lines = contract_body()
        .into_iter()
        .filter(|line| !line.starts_with("verify"))
        .collect::<Vec<String>>();
    lines.push("verify = [".to_owned());
    lines.push(r#"  "true","#.to_owned());
    lines.push("]".to_owned());
    fs::write(&path, format!("{}\n", lines.join("\n"))).expect("契約 file を書ける");
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "1 行で完結しない verify を通さない");
    assert!(
        stderr_of(&out).contains("1 行で完結"),
        "理由は行を跨いだことである: {}",
        stderr_of(&out)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_rejects_contract_without_req_or_design() {
    let (repo, state) = repo_with_state();
    for (drop, add, want) in [
        (vec!["req"], vec![], "req"),
        (vec!["design"], vec![], "design"),
        (vec!["req"], vec![r#"req = []"#], "req"),
    ] {
        let path = write_contract(&repo, &drop, &add);
        let out = run_pipe(&[
            "intake", "--contract", &path.display().to_string(), "--bead", "b",
            "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        ]);
        assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "{want} が無い契約を通さない");
        assert!(stderr_of(&out).contains(want), "{want}: {}", stderr_of(&out));
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_records_run_in_fleet() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let stamp = id.strip_prefix("s2-2e5-").unwrap_or_default();
    assert!(!stamp.is_empty(), "run id は <bead>-<stamp>: {id}");
    // id は dir 名と branch 名になるので、stamp 側に : も - も残っていない。
    assert!(!stamp.contains(':'), "stamp に : が残る: {id}");
    assert!(!stamp.contains('-'), "stamp に - が残る: {id}");

    // 契約 file の写しが置き場に在る（process 間で持ち越す面）。
    assert!(
        state.join("pipe").join(&id).join("contract.toml").exists(),
        "契約の写しが置き場に在る"
    );
    // 現在地は event log から読める。
    let out = run_pipe(&["show", "--run", &id, "--state-dir", &state.display().to_string(),
                         "--repo", &repo.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "show は rc 0");
    assert!(stdout_of(&out).contains("stage=Intake"), "{}", stdout_of(&out));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_spawn_creates_worktree_and_records_implemented() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("stage=Implemented"), "{}", stdout_of(&out));
    assert!(
        repo.join(".worktrees").join("scribe2").join(&id).exists(),
        "worktree を切る"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_spawn_marks_failed_when_runner_makes_no_commit() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // rc 0 でも commit が 0 本なら完了ではない。
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", "true",
    ]);
    assert!(stdout_of(&out).contains("stage=Failed"), "{}", stdout_of(&out));
    let shown = run_pipe(&["show", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert!(stdout_of(&shown).contains("stage=Failed"), "永続面にも Failed が残る");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_spawn_writes_write_set_into_git_dir() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["write-set"], &[r#"write-set = ["src/lib.rs", "docs/"]"#]);
    let id = intake(&repo, &state, &path);
    run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", "true",
    ]);
    let worktree = repo.join(".worktrees").join("scribe2").join(&id);
    let git_dir = PathBuf::from(git(&worktree, &["rev-parse", "--absolute-git-dir"]));
    let policy = git_dir.join("scribe2").join("write-set.txt");
    let body = fs::read_to_string(&policy).expect("policy を読める");
    assert_eq!(body, "src/lib.rs\ndocs/\n", "1 行 1 path で guard が読む形: {body:?}");
    // tracked 面を汚さない。
    assert!(
        git(&worktree, &["status", "--porcelain"]).is_empty(),
        "policy は git status に出ない"
    );
    clean(&[&repo, &state]);
}

// flip-check: retroactive s2-07l.49
#[test]
fn pipe_spawn_substitutes_placeholders_and_adds_no_env() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let runner = "env > env.txt && printf '%s\\n%s\\n%s\\n%s\\n' \
                  {run} {base} {contract} {write_set} > subst.txt && \
                  git add -A && git commit -q -m runner";
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let worktree = repo.join(".worktrees").join("scribe2").join(&id);

    // 器が足した env は**親（この test process）の env との差分**で測る。母集団を
    // 「env.txt の当該接頭辞の行」だけに取ると、親が既に持っていた変数（器の binary を
    // env で指した shell から撃つ周）を器が足したものと弁別できず、**歯が親の環境で
    // 落ちる**——測っているのは「器が足したか」であって「その名の変数が在るか」ではない。
    const OURS: &str = "SCRIBE2_";
    let env_text = fs::read_to_string(worktree.join("env.txt")).expect("env の写しを読める");
    let parent: BTreeSet<String> = std::env::vars()
        .map(|(key, _)| key)
        .filter(|key| key.starts_with(OURS))
        .collect();
    let child: BTreeSet<String> = env_text
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, _)| key.to_owned())
        .filter(|key| key.starts_with(OURS))
        .collect();
    assert_eq!(
        child, parent,
        "器固有の env を 1 つも足さない（母集団 {} 行 / 親 {parent:?} / 子 {child:?}）",
        env_text.lines().count()
    );

    let subst = fs::read_to_string(worktree.join("subst.txt")).expect("置換の写しを読める");
    let lines: Vec<&str> = subst.lines().collect();
    assert_eq!(lines.first().copied(), Some(id.as_str()), "{{run}}: {subst}");
    assert_eq!(lines.get(1).copied(), Some(base.as_str()), "{{base}}: {subst}");
    assert!(
        lines.get(2).is_some_and(|line| line.ends_with("contract.toml")),
        "{{contract}}: {subst}"
    );
    assert!(
        lines.get(3).is_some_and(|line| line.ends_with("write-set.txt")),
        "{{write_set}}: {subst}"
    );
    clean(&[&repo, &state]);
}

/// 写しの照合に使う plugin manifest の本文。
const PLUGIN_JSON: &str = "{\"name\":\"toy-plugin\"}\n";

/// 写しの照合に使う hooks の本文。**plugin.json と字面を変える**のは、片方だけを
/// 写す実装でも bytes 一致が通ってしまうのを防ぐためである。
const HOOKS_JSON: &str = "{\"hooks\":{\"PreToolUse\":[]}}\n";

/// plugin（`.claude-plugin/` と `hooks/`）を持つ toy repo と置き場を作る。
///
/// `README.md` も置くのは、**写しに worktree の他の file が混ざらない**ことを負例で
/// 測るためである（plugin の 2 dir だけを写す、が契約）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn repo_with_plugin() -> (PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    fs::create_dir_all(repo.join(".claude-plugin")).expect(".claude-plugin を作れる");
    fs::write(repo.join(".claude-plugin").join("plugin.json"), PLUGIN_JSON)
        .expect("plugin.json を書ける");
    fs::create_dir_all(repo.join("hooks")).expect("hooks dir を作れる");
    fs::write(repo.join("hooks").join("hooks.json"), HOOKS_JSON).expect("hooks.json を書ける");
    fs::write(repo.join("README.md"), "# toy\n").expect("README を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "plugin"]);
    (repo, state)
}

#[test]
fn pipe_spawn_copies_plugin_outside_worktree_and_substitutes_plugin_dir() {
    let (repo, state) = repo_with_plugin();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "printf '%s' {plugin_dir} > plugin_dir.txt && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));

    let worktree = repo.join(".worktrees").join("scribe2").join(&id);
    let plugin = state.join("pipe").join(&id).join("plugin");
    let shown = fs::read_to_string(worktree.join("plugin_dir.txt")).expect("置換の写しを読める");
    // **これが本題**: runner が受け取った plugin dir が repo の外に在るから、worktree の
    // file が Claude Code の sensitive 判定（plugin dir 配下）に掛からない。**先に本題を
    // 測る**——先に path の一致を測ると、worktree の path を渡す退行も「一致しない」でしか
    // 落ちず、何が壊れたのかが読めない。
    assert!(
        !Path::new(&shown).starts_with(&repo),
        "runner が受けた plugin dir は repo の配下でない: shown={shown} repo={}",
        repo.display()
    );
    assert_eq!(shown, plugin.display().to_string(), "{{plugin_dir}} は run dir 配下の写し");
    assert!(
        !plugin.starts_with(&repo),
        "写しは repo の配下でない: plugin={} repo={}",
        plugin.display(),
        repo.display()
    );

    for (dir, name, body) in [
        (".claude-plugin", "plugin.json", PLUGIN_JSON),
        ("hooks", "hooks.json", HOOKS_JSON),
    ] {
        let source = fs::read(worktree.join(dir).join(name)).expect("worktree 側を読める");
        let copied = fs::read(plugin.join(dir).join(name)).expect("写しを読める");
        assert_eq!(copied, source, "{dir}/{name} の bytes が worktree と一致する");
        assert_eq!(copied, body.as_bytes(), "{dir}/{name} は toy repo に置いた本文");
    }

    assert!(!plugin.join("README.md").exists(), "写しに worktree の README を入れない");
    assert!(!plugin.join("src").exists(), "写しに worktree の src を入れない");
    let mut names: Vec<String> = fs::read_dir(&plugin)
        .expect("写しの dir を読める")
        .map(|entry| entry.expect("entry を読める").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![".claude-plugin".to_owned(), "hooks".to_owned()],
        "写しは plugin の 2 dir だけ（母集団 {} entry）",
        names.len()
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_spawn_makes_empty_plugin_dir_when_repo_has_none() {
    // plugin を持たない repo（toy repo の既定）でも便を止めない。
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));
    let plugin = state.join("pipe").join(&id).join("plugin");
    assert!(plugin.is_dir(), "空の plugin dir を作る: {}", plugin.display());
    let entries: Vec<String> = fs::read_dir(&plugin)
        .expect("写しの dir を読める")
        .map(|entry| entry.expect("entry を読める").file_name().to_string_lossy().into_owned())
        .collect();
    assert!(entries.is_empty(), "写すものが無ければ空（母集団 {} entry: {entries:?}）", entries.len());
    clean(&[&repo, &state]);
}

#[test]
fn pipe_spawn_refuses_wrong_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let one = run_pipe(&["spawn", "--run", &id, "--repo", &repo.display().to_string(),
                         "--state-dir", &state.display().to_string(), "--runner", "true"]);
    assert!(stdout_of(&one).contains("stage=Failed"), "1 回目で段が動く");
    // 段が Intake でなくなったので 2 回目は何もせず rc 1。
    let two = run_pipe(&["spawn", "--run", &id, "--repo", &repo.display().to_string(),
                         "--state-dir", &state.display().to_string(), "--runner", "true"]);
    assert_eq!(two.status.code(), Some(i32::from(RC_REFUSED)), "前提違反は rc 1");
    assert!(two.stdout.is_empty(), "前提違反では stdout 0 byte");
    assert_eq!(stderr_of(&two).lines().count(), 1, "stderr は 1 行: {}", stderr_of(&two));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_state_survives_process_restart() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    // 置き場を **--state-dir なしで** 解く＝repo に紐づいた git 設定から読む。
    let out = Command::new(bin())
        .args(["pipe", "intake", "--contract"])
        .arg(&path)
        .args(["--bead", "s2-2e5", "--repo"])
        .arg(&repo)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let id = run_id_of(&out);
    // **別 process** が同じ現在地を読む（process の記憶に何も置いていない）。
    let shown = Command::new(bin())
        .args(["pipe", "show", "--run", &id, "--repo"])
        .arg(&repo)
        .output()
        .expect("binary を起動できる");
    assert_eq!(shown.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&shown));
    assert!(stdout_of(&shown).contains("stage=Intake"), "{}", stdout_of(&shown));
    assert!(stdout_of(&shown).contains(&id), "{}", stdout_of(&shown));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_stop_all_rc0_when_nothing_to_stop() {
    let (repo, state) = repo_with_state();
    for _ in 0..2 {
        let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "対象なしは rc 0（冪等）");
        assert!(stdout_of(&out).contains("seats=0 stopped=0"), "{}", stdout_of(&out));
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_stop_all_terminates_live_runner() {
    let (repo, state) = repo_with_state();
    // **孫**として起こす。test process の子のままだと kill 後に zombie が残り、
    // `/proc/<pid>` が在るせいで「まだ生きている」と読めてしまう（実運用の席は
    // 別 process が起こすので init に引き取られる）。
    let spawned = Command::new("sh")
        .arg("-c")
        .arg("sleep 60 >/dev/null 2>&1 & echo $!")
        .output()
        .expect("fake runner を起こせる");
    let pid: u32 = String::from_utf8_lossy(&spawned.stdout)
        .trim()
        .parse()
        .expect("pid を読める");
    assert!(
        Path::new(&format!("/proc/{pid}")).exists(),
        "fake runner が動いている"
    );
    // 生きた席を event log に置く（spawn は runner の終了まで待つので、席が Live な
    // 周を作るには log 側から組む）。
    let record = Command::new(bin())
        .args([
            "fleet", "record", "--kind", "SeatSpawned", "--run", "r1", "--bead", "s2-2e5",
            "--seat", "r1", "--pid", &pid.to_string(), "--state-dir",
        ])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));

    let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "全部止まれば rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("seats=1 stopped=1"), "{}", stdout_of(&out));
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "runner の process は消えている"
    );
    // 2 回目は対象が無く rc 0（冪等）。
    let again = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "冪等");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_stop_returns_rc2_on_malformed_store() {
    let (repo, state) = repo_with_state();
    let events = state.join("fleet").join("events.jsonl");
    fs::create_dir_all(state.join("fleet")).expect("dir を作れる");
    fs::write(&events, "こわれ\n").expect("壊れた行を書ける");
    let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "state が読めない周は rc 2（rc 語彙の 3 値目）"
    );
    assert!(out.stdout.is_empty(), "rc 2 でも stdout は 0 byte");
    clean(&[&repo, &state]);
}

/// event log の行数（file が無ければ 0）。
fn event_count(state: &Path) -> usize {
    fs::read_to_string(state.join("fleet").join("events.jsonl"))
        .map(|text| text.lines().filter(|line| !line.is_empty()).count())
        .unwrap_or(0)
}

#[test]
fn pipe_refuses_without_writing_events() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let before = event_count(&state);
    // 段を進めてから、同じ段の前提を要る操作をもう一度撃つ。
    run_pipe(&["spawn", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--runner", "true"]);
    let settled = event_count(&state);
    let out = run_pipe(&["spawn", "--run", &id, "--repo", &repo.display().to_string(),
                         "--state-dir", &state.display().to_string(), "--runner", "true"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "前提違反は rc 1");
    assert_eq!(
        event_count(&state),
        settled,
        "**前提違反は event を 1 件も書かない**（intake 後 {before} → 実行後 {settled}）"
    );
    // 存在しない run も同じ（何も書かずに断る）。
    let missing = run_pipe(&["spawn", "--run", "no-such-run", "--repo", &repo.display().to_string(),
                             "--state-dir", &state.display().to_string(), "--runner", "true"]);
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "無い run は rc 1");
    assert_eq!(event_count(&state), settled, "無い run でも 1 件も書かない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_refuses_duplicate_run_id() {
    let (repo, state) = repo_with_state();
    let first = write_contract(&repo, &["write-set"], &[r#"write-set = ["src/A.rs"]"#]);
    let id = intake(&repo, &state, &first);
    // stamp は秒までなので、同じ秒の再 intake は id が衝突する。**黙って上書きしない**。
    let second = write_contract(&repo, &["write-set"], &[r#"write-set = ["src/B.rs"]"#]);
    let out = run_pipe(&[
        "intake", "--contract", &second.display().to_string(), "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    if out.status.code() == Some(i32::from(RC_OK)) {
        // 秒をまたいだ周は id が違う＝衝突していない。そのときは上書きが起きていない
        // ことだけを測る（時計に依存して flaky にしない）。
        assert_ne!(run_id_of(&out), id, "id が違うなら衝突していない");
    } else {
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "衝突は rc 1 で断る");
    }
    let kept = fs::read_to_string(state.join("pipe").join(&id).join("contract.toml"))
        .expect("最初の契約を読める");
    assert!(
        kept.contains("src/A.rs"),
        "最初の便の契約が別物に化けていない: {kept}"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_rejects_broken_arrays_and_unknown_keys() {
    let (repo, state) = repo_with_state();
    for (drop, add, want) in [
        // 区切り忘れを 1 本の壊れた文字列として受理しない。
        (vec!["verify"], vec![r#"verify = ["a" "b"]"#], "引用符 1 組の文字列でない"),
        (vec!["write-set"], vec![r#"write-set = []"#], "write-set は 1 本以上"),
        (vec!["verify"], vec![r#"verify = []"#], "verify は 1 本以上"),
        (vec![], vec![r#"nonsense = "x""#], "未知の key nonsense"),
        (vec![], vec![r#"classes = ["publish", "bogus"]"#], "未知の classes 値 bogus"),
    ] {
        let path = write_contract(&repo, &drop, &add);
        let out = run_pipe(&[
            "intake", "--contract", &path.display().to_string(), "--bead", "b",
            "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        ]);
        let err = stderr_of(&out);
        assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "{want} を通さない: {err}");
        assert!(err.contains(want), "理由に {want} が出る: {err}");
        assert!(err.contains("line="), "行番号を持つ: {err}");
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_reports_broken_value_without_claiming_absence() {
    let (repo, state) = repo_with_state();
    // 値が壊れているだけで key は書かれている。「無い」と二重に言わない。
    // 文字列 key を壊すと値が 1 つも取れない＝「書かれていた」を別に覚えていないと
    // 欠落として二重に報告される。
    let path = write_contract(&repo, &["goal"], &[r#"goal = 1"#]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    let err = stderr_of(&out);
    assert!(err.contains("goal の value が文字列でない"), "値の不備を言う: {err}");
    assert!(
        !err.contains("必須の key goal が無い"),
        "書かれている key を「無い」とは言わない: {err}"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_refuses_non_git_repo() {
    let (_repo, state) = repo_with_state();
    let bare = tmp();
    let path = write_contract(&bare, &[], &[]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &bare.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "git repo でなければ intake の時点で断る");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&bare, &state]);
}

#[test]
fn pipe_spawn_measures_repo_before_launching() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let bare = tmp();
    // 起動口の手前（Precheck）で断る。ここを外すと同じ rc 1 でも **別の理由**
    // （起動関数の中で HEAD を読めない）になるので、理由まで見て弁別する。
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &bare.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", "true",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "git repo でなければ rc 1");
    assert!(
        stderr_of(&out).contains("git repo でない"),
        "測る段で断る（起動関数へ入る前）: {}",
        stderr_of(&out)
    );
    assert!(
        !bare.join(".worktrees").exists(),
        "断った周は worktree を作らない"
    );
    clean(&[&repo, &state, &bare]);
}

#[test]
fn pipe_stop_counts_seat_without_pid() {
    let (repo, state) = repo_with_state();
    // pid の無い Live 席（`fleet record` の --pid は任意）。
    let record = Command::new(bin())
        .args(["fleet", "record", "--kind", "SeatSpawned", "--run", "r9", "--bead", "b",
               "--seat", "s9", "--state-dir"])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));
    let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert!(
        stdout_of(&out).contains("seats=1"),
        "pid の無い Live 席も母集団に数える: {}",
        stdout_of(&out)
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_REFUSED)),
        "止められない席が残るので rc 1（「対象なし rc 0」に化けない）"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_stop_keeps_unstoppable_seat_live() {
    let (repo, state) = repo_with_state();
    // pid 1 は殺せない。止めていない席を終端にしない（偽の全クリアを作らない）。
    let record = Command::new(bin())
        .args(["fleet", "record", "--kind", "SeatSpawned", "--run", "r8", "--bead", "b",
               "--seat", "s8", "--pid", "1", "--state-dir"])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));
    for round in 1..=2 {
        let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
        assert_eq!(
            out.status.code(),
            Some(i32::from(RC_REFUSED)),
            "{round} 回目も rc 1（止めていないのに rc 0 を返さない）: {}",
            stdout_of(&out)
        );
        assert!(stdout_of(&out).contains("seats=1"), "{round} 回目: {}", stdout_of(&out));
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_show_reads_repo_from_state() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // **--repo を渡さずに** 撃つ。cwd（この test を走らせている repo）でなく、
    // intake が書き留めた repo から worktree の path が組まれる。
    let out = run_pipe(&["show", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let line = stdout_of(&out);
    assert!(
        line.contains(&repo.display().to_string()),
        "worktree は便に紐づいた repo から組む: {line}"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_external_form() {
    let args = |raw: &[&str]| raw.iter().map(|item| (*item).to_owned()).collect::<Vec<String>>();
    let mut lines = vec![vessel::pipe::cli::usage()];
    lines.extend(vessel::pipe::cli::dispatch(&args(&["show"])).err);
    lines.extend(vessel::pipe::cli::dispatch(&args(&["stop"])).err);
    lines.extend(vessel::pipe::cli::dispatch(&args(&["nope"])).err);
    let form = lines.join("\n");
    insta::assert_snapshot!(form);
}

// ── (b) gate → land → verdict export → e2e（設計 §8 (b)） ──────────────────

/// 便を Implemented まで進める（intake → spawn）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn implemented(repo: &Path, state: &Path, contract: &Path) -> String {
    let id = intake(repo, state, contract);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));
    out.status.code().expect("spawn の rc を読める");
    id
}

/// 呼ばれたら marker を作り、JSON 1 行を返す fake lens（`sh -c` の 1 行）。
///
/// marker で「lens を**起動しなかった**」を測れるようにしてある。判定順の 2 分岐
/// （verify RED / cap 超過）は lens を呼ばないことが契約なので、verdict だけを見ると
/// 「呼んだうえで INCONCLUSIVE を返した」と区別がつかない。
fn fake_lens(marker: &Path, body: &str) -> String {
    format!("cat >/dev/null; touch '{}'; echo '{body}'", marker.display())
}

/// `--rules` に渡す tmp manifest を書く（gate の 2 行 + lock の 2 行だけ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_rules(dir: &Path, name: &str, lens_count: u64, cap: u64) -> PathBuf {
    let row = |id: &str, kind: &str, value: u64| {
        format!(
            "[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nruling = \"t\"\nruled_at = \"d\"\n"
        )
    };
    let body = format!(
        "schema = 1\n\n{}\n{}\n{}\n{}",
        row("gate.lens_count", "GateLensCount", lens_count),
        row("gate.token_cap", "GateTokenCap", cap),
        row("fleet.lock_retry_ms", "LockRetryMs", 5000),
        row("fleet.lock_stale_ms", "LockStaleMs", 30000),
    );
    let path = dir.join(name);
    fs::write(&path, body).expect("tmp manifest を書ける");
    path
}

/// `--rules` を足して gate を 1 回撃つ。
fn gate_with_rules(repo: &Path, state: &Path, id: &str, rules: &Path, lens: &str) -> Output {
    run_pipe(&[
        "gate", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &rules.display().to_string(), "--lens", lens,
    ])
}

/// 3 値を返す fake lens の本文。
fn lens_verdict(verdict: &str) -> String {
    format!("{{\"verdict\":\"{verdict}\",\"evidence\":\"fake\"}}")
}

/// 便の worktree。
fn worktree_of(repo: &Path, id: &str) -> PathBuf {
    repo.join(".worktrees").join("scribe2").join(id)
}

/// `verdict.json` を key/value の並びとして読む。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn verdict_pairs(state: &Path, id: &str) -> Vec<(String, vessel::fleet::json_lite::Value)> {
    let path = state.join("pipe").join(id).join("verdict.json");
    let text = fs::read_to_string(&path).expect("verdict.json を読める");
    vessel::fleet::json_lite::parse_object(text.trim()).expect("verdict.json は 1 行の JSON")
}

/// key/value の並びから 1 つの値を字面で取る。無ければ空。
fn value_of(pairs: &[(String, vessel::fleet::json_lite::Value)], key: &str) -> String {
    use vessel::fleet::json_lite::Value;
    pairs
        .iter()
        .find(|(found, _)| found == key)
        .map(|(_, value)| match value {
            Value::Str(text) => text.clone(),
            Value::Num(found) => found.to_string(),
            Value::Bool(found) => found.to_string(),
            Value::Null => "null".to_owned(),
        })
        .unwrap_or_default()
}

/// `pipe show` の 1 行。
fn show_line(repo: &Path, state: &Path, id: &str) -> String {
    let out = run_pipe(&[
        "show", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    stdout_of(&out)
}

/// gate を 1 回撃つ（lens は任意）。
fn gate_once(repo: &Path, state: &Path, id: &str, lens: Option<&str>) -> Output {
    let mut args: Vec<String> = ["gate", "--run", id]
        .iter()
        .map(|item| (*item).to_owned())
        .collect();
    args.extend([
        "--repo".to_owned(), repo.display().to_string(),
        "--state-dir".to_owned(), state.display().to_string(),
    ]);
    if let Some(cmd) = lens {
        args.extend(["--lens".to_owned(), cmd.to_owned()]);
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_pipe(&borrowed)
}

/// land を 1 回撃つ。
fn land_once(repo: &Path, state: &Path, id: &str) -> Output {
    run_pipe(&[
        "land", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ])
}

/// PASS の gate まで通した便を作る。
fn gated_pass(repo: &Path, state: &Path, contract: &Path, marker: &Path) -> String {
    let id = implemented(repo, state, contract);
    let lens = fake_lens(marker, &lens_verdict("PASS"));
    let out = gate_once(repo, state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS の gate は rc 0: {}", stderr_of(&out));
    id
}

#[test]
fn pipe_gate_refuses_dirty_worktree() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 実装後に worktree を汚す。gate は前提を満たさない。
    fs::write(worktree_of(&repo, &id).join("dirty.txt"), "x\n").expect("汚せる");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "前提違反は rc 1");
    assert!(stderr_of(&out).contains("clean でない"), "理由: {}", stderr_of(&out));
    assert!(!marker.exists(), "**lens を起動しない**（前提違反の周）");
    assert!(
        show_line(&repo, &state, &id).contains("stage=Failed"),
        "precheck 違反は Failed で残る: {}",
        show_line(&repo, &state, &id)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_fails_on_red_verify_line() {
    let (repo, state) = repo_with_state();
    // 2 行目が rc≠0。**逐条**で残るので 2 行とも verify.jsonl に出る。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["true", "false"]"#]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1");
    assert!(stdout_of(&out).contains("verdict=FAIL"), "{}", stdout_of(&out));
    // **lens は呼ばない**: verify が赤い周は lens の verdict に上書きされない。
    assert!(!marker.exists(), "verify RED の周は lens を起動しない");
    let log = fs::read_to_string(state.join("pipe").join(&id).join("verify.jsonl"))
        .expect("verify.jsonl を読める");
    assert_eq!(log.lines().count(), 2, "verify は逐条で残る: {log}");
    assert!(log.contains("\"rc\":0"), "1 行目の rc 0: {log}");
    assert!(log.contains("\"rc\":1"), "2 行目の rc 1: {log}");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "1", "赤は 1 本");
    clean(&[&repo, &state]);
}

/// 赤い verify 行の stderr の末尾が診断 file に残る（緑の行は残さない）。
///
/// `verify.jsonl` の rc だけでは「何がどう赤いか」が便の外から読めない。**stderr の
/// 本文で測る**——見出し行にも `cmd=` として字面が載るので、cmd に**無い**字面
/// （`boom` は `printf` が組み立てる）で「写しが空でない」を弁別する。
#[test]
fn pipe_gate_keeps_stderr_tail_of_red_verify_lines() {
    let (repo, state) = repo_with_state();
    // 2 行目が rc 3 で stderr に 1 行出す。cmd の字面には `boom` が無い。
    let path = write_contract(
        &repo,
        &["verify"],
        &[r#"verify = ["true", "printf 'bo%s\n' om >&2; exit 3"]"#],
    );
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1");
    assert!(stdout_of(&out).contains("verdict=FAIL"), "{}", stdout_of(&out));

    // record の形は変えない（schema 不変）。rc は逐条のまま `verify.jsonl` に在る。
    let log = fs::read_to_string(state.join("pipe").join(&id).join("verify.jsonl"))
        .expect("verify.jsonl を読める");
    let rows: Vec<&str> = log.lines().collect();
    assert_eq!(rows.len(), 2, "verify は逐条で残る: {log}");
    let second =
        vessel::fleet::json_lite::parse_object(rows.get(1).copied().unwrap_or_default().trim())
            .expect("2 行目は 1 行の JSON");
    assert_eq!(value_of(&second, "n"), "2", "赤いのは 2 行目: {log}");
    assert_eq!(value_of(&second, "rc"), "3", "n=2 の rc: {log}");

    let tail = fs::read_to_string(state.join("pipe").join(&id).join("verify.stderr.log"))
        .expect("verify.stderr.log を読める");
    let heads: Vec<&str> = tail.lines().filter(|line| line.starts_with("## ")).collect();
    assert_eq!(
        heads.len(),
        1,
        "見出しは赤い行の分だけ（母集団 {} 行）: {tail}",
        tail.lines().count()
    );
    let head = heads.first().copied().unwrap_or_default();
    assert!(head.contains("n=2 rc=3"), "見出しは赤い行を名指す: {head}");
    assert!(!head.contains("boom"), "見出しの cmd= に boom の字面は無い: {head}");
    assert!(!tail.contains("## n=1"), "緑の行は見出しを残さない: {tail}");
    assert!(tail.contains("boom"), "stderr の本文が残る: {tail}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_without_lens_when_required() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 規則は lens を 1 本要る（gate.lens_count = 1）が `--lens` が無い。
    let out = gate_once(&repo, &state, &id, None);
    assert_eq!(out.status.code(), Some(3), "判定できない周の rc は 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE");
    assert!(
        value_of(&pairs, "evidence").contains("--lens が無い"),
        "理由が残る: {}",
        value_of(&pairs, "evidence")
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_when_diff_exceeds_cap() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // cap を 1 byte にした manifest を渡す（**数値は規則から来る**ことを測る）。
    let rules = write_rules(&repo, "tight.toml", 1, 1);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "gate", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &rules.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(3), "cap 超過の rc は 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    // **lens を呼ばない**のが cap の意味である（呼んでから捨てるのでは予算を守れない）。
    assert!(!marker.exists(), "cap 超過の周は lens を起動しない");
    let pairs = verdict_pairs(&state, &id);
    assert!(
        value_of(&pairs, "evidence").contains("cap 1"),
        "cap の値は規則から来る: {}",
        value_of(&pairs, "evidence")
    );
    let bytes: u64 = value_of(&pairs, "diff_bytes").parse().unwrap_or(0);
    assert!(bytes > 1, "diff の byte 数を実測して比べている: {bytes}");

    // **境界**: 設計は「diff byte > cap → INCONCLUSIVE」＝等号は超えていない。
    // 同じ内容の別便を cap = ちょうどその byte 数で撃ち、PASS 側に残ることを測る。
    let twin = intake_bead(&repo, &state, &path, "s2-edge");
    let out = run_pipe(&[
        "spawn", "--run", &twin, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    let exact = write_rules(&repo, "exact.toml", 1, bytes);
    let edge = gate_with_rules(&repo, &state, &twin, &exact, &lens);
    assert_eq!(
        edge.status.code(),
        Some(i32::from(RC_OK)),
        "cap ちょうどは超えていない（> であって >= でない）: {}",
        stdout_of(&edge)
    );
    assert_eq!(
        value_of(&verdict_pairs(&state, &twin), "diff_bytes"),
        bytes.to_string(),
        "同じ内容の便なので diff の byte 数も同じ"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_records_structured_verdict() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS の rc は 0: {}", stderr_of(&out));
    assert!(marker.exists(), "判定に届いた周は lens を起動する");
    let pairs = verdict_pairs(&state, &id);
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["schema", "run", "verdict", "evidence", "verify_red", "diff_bytes", "ts"],
        "verdict.json の key 列（設計 §5.3）"
    );
    assert_eq!(value_of(&pairs, "schema"), "1");
    assert_eq!(value_of(&pairs, "run"), id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS");
    assert_eq!(value_of(&pairs, "evidence"), "fake", "lens の evidence を写す");
    assert_eq!(value_of(&pairs, "verify_red"), "0");
    assert!(
        show_line(&repo, &state, &id).contains("stage=Gated"),
        "段が Gated へ動く: {}",
        show_line(&repo, &state, &id)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_refuses_without_pass() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("FAIL"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert!(stdout_of(&gated).contains("verdict=FAIL"), "{}", stdout_of(&gated));
    let before = git(&repo, &["rev-parse", "refs/heads/main"]);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "PASS 以外は rc 1");
    assert!(stderr_of(&out).contains("PASS でない"), "理由: {}", stderr_of(&out));
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        before,
        "**何もしない**（main は 1 byte も動かない）"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_squashes_one_commit_with_identical_tree() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let tree = git(&worktree_of(&repo, &id), &["rev-parse", "HEAD^{tree}"]);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert!(stdout_of(&out).contains(&format!("landed={new}")), "{}", stdout_of(&out));
    assert_eq!(
        git(&repo, &["rev-list", "--count", &format!("{base}..{new}")]),
        "1",
        "**squash は 1 commit**"
    );
    assert_eq!(
        git(&repo, &["rev-parse", &format!("{new}^{{tree}}")]),
        tree,
        "tree は同一（lossless）"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_refuses_stale_base() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    // gate の後に main が別便で進む。CAS の old が動いた＝land してはならない。
    fs::write(repo.join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "stale base は rc 1");
    assert!(stderr_of(&out).contains("stale base"), "理由: {}", stderr_of(&out));
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        moved,
        "断った周は main を動かさない"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_reruns_verify_on_main_and_fails_loud() {
    let (repo, state) = repo_with_state();
    let seen = state.join("verify-seen");
    // 1 回目（worktree）は緑・2 回目（main の実測）は赤になる verify 行。
    let line = format!(
        r#"verify = ["test ! -f '{}' && touch '{}'"]"#,
        seen.display(),
        seen.display()
    );
    let path = write_contract(&repo, &["verify"], &[&line]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "main が赤ければ rc 1");
    assert!(stderr_of(&out).contains("main が赤い"), "理由: {}", stderr_of(&out));
    // **auto revert しない**: main は進んだまま loud に落ちる。
    assert_ne!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        base,
        "main は進んだまま（revert しない）"
    );
    assert!(
        show_line(&repo, &state, &id).contains("stage=Failed"),
        "Failed detail=main-red で残る: {}",
        show_line(&repo, &state, &id)
    );
    assert!(
        !land::verdicts_path(&state).exists(),
        "赤い周は面 5 へ export しない"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_exports_verdict_schema1() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let exported = fs::read_to_string(land::verdicts_path(&state)).expect("verdicts.jsonl を読める");
    assert_eq!(exported.lines().count(), 1, "land ごとに 1 行: {exported}");
    let line = exported.lines().next().unwrap_or_default();
    let pairs = vessel::fleet::json_lite::parse_object(line).expect("1 行の JSON");
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["schema", "run", "bead", "sha", "verdict", "evidence", "ts"],
        "面 5 の key 列（ADR-0004 §2.2・版番号に依らず固定）"
    );
    assert_eq!(value_of(&pairs, "schema"), "1");
    assert_eq!(value_of(&pairs, "run"), id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS");
    assert_eq!(
        value_of(&pairs, "sha"),
        git(&repo, &["rev-parse", "refs/heads/main"]),
        "sha は land した commit"
    );
    assert!(
        value_of(&pairs, "evidence").ends_with("verdict.json"),
        "evidence は verdict.json の path: {}",
        value_of(&pairs, "evidence")
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_retires_worktree_by_move_and_keeps_branch() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let live = worktree_of(&repo, &id);
    assert!(live.exists(), "land の前は便の worktree が在る");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(retired.exists(), "retired/ へ move する");
    assert!(!live.exists(), "元の場所には残らない");
    // **削除しない**（N1.2）: 中身が move で運ばれている。
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    let branches = git(&repo, &["branch", "--list", &format!("scribe2/{id}")]);
    assert!(!branches.trim().is_empty(), "branch は消さない: {branches}");
    // main 実測用の tmp worktree だけは畳む。
    assert!(
        !repo.join(".worktrees").join("scribe2").join("verify").join(&id).exists(),
        "main 実測の tmp worktree は remove する"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_e2e_toy_repo_lands_one_bead_with_fake_runner() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    // intake → spawn → gate → land を **1 process で人手 0** で通す。
    let out = run_pipe(&[
        "run", "--contract", &path.display().to_string(), "--bead", "s2-41o",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
        "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "run は rc 0: {}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(text.contains("verdict=PASS"), "gate まで通る: {text}");
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert!(text.contains(&format!("landed={new}")), "land まで通る: {text}");
    assert_eq!(
        git(&repo, &["rev-list", "--count", &format!("{base}..{new}")]),
        "1",
        "toy repo に 1 便が載る"
    );
    let id = run_id_of(&out);
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    // 人由来の event は 1 件も無い（承認の要らない契約ゆえ）。**この行だけでは (b) の
    // code を測れない**（human を書くのは (c) の `ApprovalReceived` だけ）ので、
    // 「1 便が最後まで載った」ことを event の側からも測る。
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(!log.contains("\"actor\":\"human\""), "人手 0 で通る: {log}");
    let landed_events = log
        .lines()
        .filter(|line| line.contains("\"kind\":\"RunDone\"") && line.contains("\"stage\":\"Landed\""))
        .count();
    assert_eq!(landed_events, 1, "RunDone stage=Landed が 1 件: {log}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_resume_continues_from_implemented_in_new_process() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    // spawn までで process が終わる（＝gate の手前で落ちた便と同じ現在地）。
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // **別 process** が置き場だけを読んで続きを引く。
    let gated = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "Implemented → gate: {}", stderr_of(&gated));
    assert!(stdout_of(&gated).contains("verdict=PASS"), "{}", stdout_of(&gated));
    // もう一度 resume すると Gated(PASS) → land へ進む。
    let landed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "Gated → land: {}", stderr_of(&landed));
    assert!(stdout_of(&landed).contains("landed="), "{}", stdout_of(&landed));
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_refuses_run_without_commits() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // commit を作らない runner。spawn は Failed で終える（commit 0 は完了ではない）。
    run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", "true",
    ]);
    // 段だけを Implemented へ書き換える（＝台帳が壊れている / 手で進めた周）。
    // worktree は在って clean なので、**commits の検査だけ**が gate を止める。
    let forced = Command::new(bin())
        .args(["fleet", "record", "--state-dir"])
        .arg(&state)
        .args(["--kind", "RunStage", "--stage", "Implemented", "--run", &id, "--bead", "s2-2e5"])
        .output()
        .expect("binary を起動できる");
    assert_eq!(forced.status.code(), Some(i32::from(RC_OK)), "record は rc 0");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "commit 0 は前提違反で rc 1");
    assert!(
        stderr_of(&out).contains("commit が 1 本も無い"),
        "理由は commits（dirty ではない）: {}",
        stderr_of(&out)
    );
    assert!(!marker.exists(), "前提違反の周は lens を起動しない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_on_unlisted_lens_verdict() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    // 3 値の外を名乗る lens。**PASS へ倒さない**（AC3「偽の PASS 0 件」）。
    let lens = fake_lens(&marker, &lens_verdict("OK"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(3), "3 値外は rc 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    assert!(marker.exists(), "lens 自体は呼んでいる（判定に届いた周）");
    assert_eq!(
        value_of(&verdict_pairs(&state, &id), "verdict"),
        "INCONCLUSIVE",
        "未知の verdict を通さない"
    );
    clean(&[&repo, &state]);
}

// ── lens review（2026-09-09）で「測っていない」と名指しされた経路を塞ぐ歯 ──────

#[test]
fn pipe_gate_inconclusive_when_lens_count_is_not_one() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // 0 本（lens を呼ばずに通す）も 2 本（1 本で足りたことにする）も**判定できていない**。
    // 規則 1 行で gate が飾りになる形を塞ぐ（AC3「偽の PASS 0 件」）。
    for (count, bead) in [(0_u64, "s2-zero"), (2, "s2-two")] {
        let id = intake_bead(&repo, &state, &path, bead);
        let out = run_pipe(&[
            "spawn", "--run", &id, "--repo", &repo.display().to_string(),
            "--state-dir", &state.display().to_string(),
            "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
        ]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
        let rules = write_rules(&repo, &format!("lens{count}.toml"), count, 150_000);
        let gated = gate_with_rules(&repo, &state, &id, &rules, &lens);
        assert_eq!(gated.status.code(), Some(3), "lens {count} 本は判定不能で rc 3");
        assert!(
            stdout_of(&gated).contains("verdict=INCONCLUSIVE"),
            "lens {count} 本: {}",
            stdout_of(&gated)
        );
        // **land まで行かせない**（面 5 に PASS を残さない）。
        let landed = land_once(&repo, &state, &id);
        assert_eq!(landed.status.code(), Some(i32::from(RC_REFUSED)), "PASS でなければ land しない");
    }
    assert!(!marker.exists(), "0 本の周は lens を起動しない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_when_lens_exits_nonzero() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // JSON は正しく吐くが rc≠0 で終える lens。**出力を信じて PASS へ倒さない**。
    let lens = format!("cat >/dev/null; echo '{}'; exit 7", lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(3), "lens が rc≠0 なら rc 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    assert!(
        value_of(&verdict_pairs(&state, &id), "evidence").contains("rc 7"),
        "理由に lens の rc が残る: {}",
        value_of(&verdict_pairs(&state, &id), "evidence")
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_when_lens_output_is_not_json() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // rc 0 だが JSON 行が無い lens。**読めなかったを通ったに化けさせない**。
    let lens = "cat >/dev/null; echo looks-fine".to_owned();
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(3), "parse 不能なら rc 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    assert!(
        value_of(&verdict_pairs(&state, &id), "evidence").contains("JSON 行が無い"),
        "理由: {}",
        value_of(&verdict_pairs(&state, &id), "evidence")
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_passes_diff_to_lens_on_stdin() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let seen = state.join("stdin-bytes");
    // lens が **実際に受け取った byte 数**を書き出す（設計 §5.3 / FR9 の中心）。
    let lens = format!("wc -c > '{}'; echo '{}'", seen.display(), lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let received = fs::read_to_string(&seen).expect("lens が受けた byte 数を読める");
    let received = received.trim().to_owned();
    assert_ne!(received, "0", "diff を渡さずに lens を呼んでいない");
    assert_eq!(
        received,
        value_of(&verdict_pairs(&state, &id), "diff_bytes"),
        "lens が受けた byte 数と verdict.json の diff_bytes は同じ diff を指す"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_substitutes_contract_placeholder_in_lens_cmd() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let seen = state.join("lens-contract-arg");
    // fake lens が **受け取った argv**（置換後の cmd に埋まった path）を写す。
    // 置換していなければ `{contract}` の字面がそのまま残る。
    // **穴は 2 つ置く**。1 つだけだと `replacen(_, _, 1)` へ縮める変異が生き残る
    // （review 2026-09-10）。2 つ目が置換されなければ字面のまま file に残る。
    let lens = format!(
        "printf '%s\\n%s' '{{contract}}' '{{contract}}' > '{}'; cat >/dev/null; echo '{}'",
        seen.display(),
        lens_verdict("PASS")
    );
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let handed = fs::read_to_string(&seen).expect("lens が受けた値を読める");
    let handed: Vec<&str> = handed.lines().collect();
    assert_eq!(handed.len(), 2, "穴 2 つ分が渡る: {handed:?}");
    assert!(
        !handed.iter().any(|line| line.contains("{contract}")),
        "placeholder が 1 つでも置換されずに渡っている: {handed:?}"
    );
    let handed = handed.first().copied().unwrap_or_default().trim().to_owned();
    // **path であって本文ではない**（cmd は `sh -c` の 1 行なので、本文を埋めると
    // 契約の中の引用符 1 つで cmd の構造が変わる）。
    let handed = PathBuf::from(handed.trim());
    assert!(handed.is_absolute(), "契約 copy の絶対 path が渡る: {}", handed.display());
    let body = fs::read_to_string(&handed).expect("lens は渡された path から契約を読める");
    // 渡ったのが **この便の契約 copy** であること（別の file を指していない）。
    assert!(body.contains("縦 1 本を通す"), "契約の goal が読める: {body}");
    assert_eq!(
        handed,
        state.join("pipe").join(&id).join("contract.toml"),
        "run の契約 copy を指す"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_refuses_wrong_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let before = event_count(&state);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // Intake の便に gate は掛からない。**段違いは何もせず rc 1**（Failed で終端させない）。
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "段違いは rc 1");
    assert!(stderr_of(&out).contains("段は Intake である"), "理由: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before, "段違いは event を 1 件も書かない");
    assert!(!marker.exists(), "lens を起動しない");
    // 終端していないので、正しい段まで進めれば通る（resume できる）。
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    // 逆向きも同じ: Implemented の便に land は掛からない。
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_REFUSED)), "gate 前の land は rc 1");
    assert!(
        stderr_of(&landed).contains("段は Implemented である"),
        "理由: {}",
        stderr_of(&landed)
    );
    // **判定が済んだ**便に gate は 2 度掛からない（測り直せるのは INCONCLUSIVE の
    // 周だけ・設計 §5.3。ここは PASS ゆえ終端側である）。
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "1 度目は通る: {}", stderr_of(&gated));
    let again = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "2 度目の gate は rc 1");
    assert!(stderr_of(&again).contains("段は Gated である"), "理由: {}", stderr_of(&again));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_reports_unmeasured_main_apart_from_red() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    // main 実測用の tmp の置き場を塞ぐ＝**verify を 1 行も撃てない**。
    let blocked = repo.join(".worktrees").join("scribe2").join("verify").join(&id);
    fs::create_dir_all(&blocked).expect("tmp の置き場を塞げる");
    fs::write(blocked.join("occupied"), "x\n").expect("塞げる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "測れない周は rc 2");
    assert!(
        stderr_of(&out).contains("実測できない"),
        "**赤ではなく測れない**と名乗る: {}",
        stderr_of(&out)
    );
    assert!(!stderr_of(&out).contains("赤い"), "赤を名乗らない: {}", stderr_of(&out));
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(log.contains("\"detail\":\"main-unmeasured\""), "別の名で残す: {log}");
    assert!(!log.contains("\"detail\":\"main-red\""), "main-red は書かない: {log}");
    assert!(!land::verdicts_path(&state).exists(), "面 5 へ export しない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_removes_dirty_tmp_worktree() {
    let (repo, state) = repo_with_state();
    // detached（= main 実測の tmp）のときだけ中間物を作る verify 行。便の worktree は
    // branch 上なので clean のままで、retire の move が塞がれない。
    let line = r#"verify = ["git rev-parse --abbrev-ref HEAD | grep -qx HEAD && touch build-artifact.txt; true"]"#;
    let path = write_contract(&repo, &["verify"], &[line]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    // **中間物で dirty になった tmp を leak させない**（設計 §5.4 の `--force`）。
    assert!(
        !repo.join(".worktrees").join("scribe2").join("verify").join(&id).exists(),
        "dirty な tmp worktree も畳む"
    );
    let listed = git(&repo, &["worktree", "list"]);
    assert!(!listed.contains("/verify/"), "worktree の登録も残らない: {listed}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_regates_after_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 道具が足りない周（`--lens` を渡し忘れた便）。INCONCLUSIVE は「測れなかった」で
    // あって「落ちた」ではないので、**ここで終端しない**。
    let first = gate_once(&repo, &state, &id, None);
    assert_eq!(first.status.code(), Some(3), "測れなかった周の rc は 3");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "INCONCLUSIVE");

    // 道具を揃えて撃ち直す。**段が Gated でも通る**のが本便で広げた 1 分岐である。
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let second = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(
        second.status.code(),
        Some(i32::from(RC_OK)),
        "道具を揃えた測り直しは通る: {}",
        stderr_of(&second)
    );
    assert!(marker.exists(), "2 度目は lens を起動する");
    assert_eq!(
        value_of(&verdict_pairs(&state, &id), "verdict"),
        "PASS",
        "verdict.json は新しい判定で上書きされる"
    );

    // **前の INCONCLUSIVE は event に残る**（append-only・書き換えない）。
    let gated: Vec<String> = events(&state)
        .into_iter()
        .filter(|event| event.run == id && event.stage == Some(Stage::Gated))
        .filter_map(|event| event.detail)
        .collect();
    assert_eq!(
        gated,
        vec!["verdict:INCONCLUSIVE".to_owned(), "verdict:PASS".to_owned()],
        "測り直しは 2 件目を追記する（1 件目を書き換えない）"
    );
    // 吸収状態が解けている＝そのまま land まで進む。
    let landed = land_once(&repo, &state, &id);
    assert_eq!(
        landed.status.code(),
        Some(i32::from(RC_OK)),
        "測り直した便は land できる: {}",
        stderr_of(&landed)
    );
    // **land 済みの便は測り直せない**。ここを開けると Gated を書き直して land を 2 度
    // 通す口になる（測り直しが開くのは Gated の 1 段だけ）。
    let after_land = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(
        after_land.status.code(),
        Some(i32::from(RC_REFUSED)),
        "Landed からの再 gate は rc 1"
    );
    assert!(
        stderr_of(&after_land).contains("段は Landed である"),
        "理由は段である: {}",
        stderr_of(&after_land)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_regates_after_relaxing_cap() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // **cap 超過も「測れなかった」**（lens を呼べていない）。契約が名指す測り直しの
    // 主用途はここで、`--lens` の渡し忘れだけを測っていると `--rules` 経由の周が丸ごと
    // 素通りする。
    let tight = write_rules(&repo, "tight.toml", 1, 1);
    let first = gate_with_rules(&repo, &state, &id, &tight, &lens);
    assert_eq!(first.status.code(), Some(3), "cap 超過の rc は 3");
    assert!(!marker.exists(), "cap 超過の周は lens を起動しない");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE");
    let bytes: u64 = value_of(&pairs, "diff_bytes").parse().unwrap_or(0);

    // **道具を揃えずに撃ち直しても通らない**（測り直しは検査を素通りする口ではない）。
    let retight = gate_with_rules(&repo, &state, &id, &tight, &lens);
    assert_eq!(retight.status.code(), Some(3), "cap のままなら 2 度目も 3");
    assert!(!marker.exists(), "2 度目も cap 超過なら lens を起動しない");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "INCONCLUSIVE");

    // 予算を実測値まで緩めて撃ち直す（道具を揃えるのは人の手番）。
    let relaxed = write_rules(&repo, "relaxed.toml", 1, bytes);
    let second = gate_with_rules(&repo, &state, &id, &relaxed, &lens);
    assert_eq!(
        second.status.code(),
        Some(i32::from(RC_OK)),
        "cap を緩めた測り直しは通る: {}",
        stderr_of(&second)
    );
    assert!(marker.exists(), "2 度目は lens を起動する");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS");
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_regates_after_fixing_lens_count() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // 規則が lens 2 本を定める周は「1 本で足りたことにしない」ため INCONCLUSIVE。
    // **規則の側の不備も道具の不足**なので、直せば同じ便を測り直せる。
    let two = write_rules(&repo, "two-lenses.toml", 2, 100_000);
    let first = gate_with_rules(&repo, &state, &id, &two, &lens);
    assert_eq!(first.status.code(), Some(3), "lens_count≠1 の rc は 3");
    assert!(!marker.exists(), "本数が合わない周は lens を起動しない");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "INCONCLUSIVE");

    // **規則を直さずに撃ち直しても通らない**（測り直しは本数照合を外す口ではない）。
    let retry = gate_with_rules(&repo, &state, &id, &two, &lens);
    assert_eq!(retry.status.code(), Some(3), "規則がそのままなら 2 度目も 3");
    assert!(!marker.exists(), "本数が合わないままなら lens を起動しない");

    let one = write_rules(&repo, "one-lens.toml", 1, 100_000);
    let second = gate_with_rules(&repo, &state, &id, &one, &lens);
    assert_eq!(
        second.status.code(),
        Some(i32::from(RC_OK)),
        "規則を直した測り直しは通る: {}",
        stderr_of(&second)
    );
    assert!(marker.exists(), "2 度目は lens を起動する");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_fails_regate_on_dirty_worktree() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let first = gate_once(&repo, &state, &id, None);
    assert_eq!(first.status.code(), Some(3), "測れなかった周の rc は 3");

    // 道具（lens・cap）を揃える過程で worktree を汚した周。**測り直しに来た便でも
    // worktree の事実の違反は Failed で終端する**——precheck の極性を段で変えると
    // 「段の検査は cli / worktree の事実は gate」の分離が濁るため（planner 裁定
    // 2026-09-10 Q3 案B）。運用は「道具を揃える前に worktree を clean へ戻す」。
    fs::write(worktree_of(&repo, &id).join("dirty.txt"), "x\n").expect("汚せる");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "前提違反は rc 1");
    assert!(stderr_of(&out).contains("clean でない"), "理由: {}", stderr_of(&out));
    assert!(!marker.exists(), "lens を起動しない");
    assert!(
        show_line(&repo, &state, &id).contains("stage=Failed"),
        "測り直しの周も precheck 違反は終端する: {}",
        show_line(&repo, &state, &id)
    );

    // **掃除しても引けない**（Failed は終端）。この非対称は測って残す。
    fs::remove_file(worktree_of(&repo, &id).join("dirty.txt")).expect("掃除できる");
    let again = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "Failed からは撃ち直せない");
    assert!(
        stderr_of(&again).contains("段は Failed である"),
        "理由: {}",
        stderr_of(&again)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_refuses_regate_after_fail() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let failed_marker = state.join("lens-fail");
    let failing = fake_lens(&failed_marker, &lens_verdict("FAIL"));
    let first = gate_once(&repo, &state, &id, Some(&failing));
    assert_eq!(first.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "FAIL");

    // **FAIL は終端のまま**。契約の verify が赤い便は測り直しても赤いので、撃ち直す口を
    // 開けない（開けると「壊れたまま進む」経路になる）。**段違いの一般則どおり何もしない**。
    let before = event_count(&state);
    let passing_marker = state.join("lens-pass");
    let passing = fake_lens(&passing_marker, &lens_verdict("PASS"));
    let second = gate_once(&repo, &state, &id, Some(&passing));
    assert_eq!(second.status.code(), Some(i32::from(RC_REFUSED)), "FAIL からの再 gate は rc 1");
    assert!(
        stderr_of(&second).contains("FAIL"),
        "断る理由は「段は Gated」ではなく判定である: {}",
        stderr_of(&second)
    );
    assert!(!passing_marker.exists(), "lens を起動しない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    assert_eq!(
        value_of(&verdict_pairs(&state, &id), "verdict"),
        "FAIL",
        "判定は書き換わらない（PASS の lens を渡しても）"
    );
    // resume も FAIL を測り直しへ案内しない（`next=` を名乗るのは INCONCLUSIVE だけ）。
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の resume は rc 1");
    assert!(
        !stdout_of(&resumed).contains("next="),
        "次の一手を名乗らない（終端である）: {}",
        stdout_of(&resumed)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_refuses_regate_without_readable_verdict() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let first = gate_once(&repo, &state, &id, None);
    assert_eq!(first.status.code(), Some(3), "測れなかった周の rc は 3");

    // **判定が読めない周は測り直さない**（fail-closed・C11.2）。前提は「Gated ∧ verdict が
    // INCONCLUSIVE」であって「Gated ∧ PASS でも FAIL でもない」ではない——後者だと
    // verdict.json が消えた / 壊れた便まで撃ち直せてしまい、「測れなかった」ではなく
    // **判定の記録が無い**便が gate を通る（自前の変異 M04 が生き延びた経路）。
    let judged = state.join("pipe").join(&id).join("verdict.json");
    let before = event_count(&state);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // `verdict_of` は 3 つの経路を等しく「読めない」へ畳む。**3 つとも測る**——1 つだけ
    // 測ると、残り 2 つを INCONCLUSIVE へ倒す実装（＝測れなかった便を測り直してよい便に
    // 化けさせる）が素通りする。
    for (label, body) in [
        ("file が無い", None),
        ("JSON が壊れている", Some("{ 壊れた\n")),
        (
            "verdict が 3 値の外",
            Some(r#"{"schema":1,"run":"x","verdict":"MAYBE","evidence":"","verify_red":0,"diff_bytes":0,"ts":"t"}"#),
        ),
    ] {
        match body {
            None => {
                fs::remove_file(&judged).ok();
            }
            Some(text) => fs::write(&judged, text).expect("verdict.json を書ける"),
        }
        let out = gate_once(&repo, &state, &id, Some(&lens));
        assert_eq!(
            out.status.code(),
            Some(i32::from(RC_REFUSED)),
            "{label}: 読めない周は rc 1"
        );
        assert!(
            stderr_of(&out).contains("読めない"),
            "{label}: **判定が読めない**と名乗る（INCONCLUSIVE と同じ扱いにしない）: {}",
            stderr_of(&out)
        );
        assert!(!marker.exists(), "{label}: lens を起動しない");
        assert_eq!(event_count(&state), before, "{label}: event を 1 件も書かない");
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_resume_reports_next_gate_on_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let first = gate_once(&repo, &state, &id, None);
    assert_eq!(first.status.code(), Some(3), "測れなかった周の rc は 3");

    // resume は **自動で測り直さない**（道具の不足は人が直す）。`--lens` を渡してあっても
    // 撃たず、次に何をすればよいかだけを名乗って止まる。
    let before = event_count(&state);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(resumed.status.code(), Some(3), "測れていない便の resume は rc 3");
    assert!(
        stdout_of(&resumed).contains("next=gate"),
        "次の一手を名乗る: {}",
        stdout_of(&resumed)
    );
    assert!(!marker.exists(), "resume は lens を起こさない（撃ち直すのは人）");
    assert_eq!(event_count(&state), before, "何も書かない");
    // **land を試して断られる形（吸収状態）に戻っていない**。
    assert!(
        !stderr_of(&resumed).contains("PASS でない"),
        "land を試さない: {}",
        stderr_of(&resumed)
    );
    clean(&[&repo, &state]);
}

// ── (c) 承認 Blocked と resume（設計 §8 (c)・FR15 / FR16 / AC5・憲法 A1 / C7.2） ──

/// event log の全行を型で読む（file が無ければ空）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn events(state: &Path) -> Vec<Event> {
    fs::read_to_string(state.join("fleet").join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| Event::from_line(line).expect("event の 1 行を読める"))
        .collect()
}

/// commit を 1 本作る fake runner。**起動されたら marker を残す**——「起きていない」を
/// rc でなく効果で測るための痕跡である。
fn runner_cmd(marker: &Path) -> String {
    format!(
        "touch {} && echo x >> src/lib.rs && git add -A && git commit -q -m runner",
        marker.display()
    )
}

/// `fleet record` で `ApprovalReceived` を 1 件直接積む。**`pipe approve` を通さない**
/// 経路で、書き手側の逐語検査を素通りした event が関門を開けないことを測るのに使う。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn record_approval(state: &Path, id: &str, extra: &[&str]) -> Output {
    let mut args = vec!["fleet".to_owned(), "record".to_owned(), "--state-dir".to_owned()];
    args.push(state.display().to_string());
    args.extend(
        ["--kind", "ApprovalReceived", "--run", id, "--bead", "s2-2e5"]
            .iter()
            .map(|item| (*item).to_owned()),
    );
    args.extend(extra.iter().map(|item| (*item).to_owned()));
    Command::new(bin()).args(&args).output().expect("binary を起動できる")
}

/// 3 クラスを名乗る契約で intake → spawn まで撃ち、Blocked で止まった便の
/// id と stdout を返す。stdout は「いまどの段に居るか」の主張なので測る対象である。
fn blocked(repo: &Path, state: &Path, marker: &Path, classes: &str) -> (String, String) {
    let path = write_contract(repo, &[], &[classes]);
    let id = intake(repo, state, &path);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", &runner_cmd(marker),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BLOCKED)),
        "承認が要る便は rc 3 で止まる: {}",
        stderr_of(&out)
    );
    (id, stdout_of(&out))
}

#[test]
fn pipe_approval_blocks_before_spawn_when_contract_declares_class() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    // **2 クラス**で撃つ。1 クラスだと detail の連結が恒等になり、区切りを測れない。
    let (id, said) = blocked(&repo, &state, &marker, r#"classes = ["delete", "publish"]"#);
    assert!(said.contains(&format!("run={id} stage=Blocked")), "止まった先を stdout で名乗る: {said}");
    // **数字そのものが約束である**（設計 §5.5）。定数を辿るだけの assert は、定数が
    // 動いたときに歯も黙って追随する——外形の 3 はここで literal に留める。
    assert_eq!(RC_BLOCKED, 3, "人の手番で止まっている周の rc は 3");
    // **効果で測る**: 止めたと名乗るだけでなく、runner が 1 度も起きていない。A1 の
    // 「実行前」は、消す / 出す / 使うが**起きた後**に聞くのでは意味が無い。
    assert!(!marker.exists(), "runner を起こさない");
    assert!(
        !repo.join(".worktrees").join("scribe2").join(&id).exists(),
        "worktree も切らない"
    );
    let mine: Vec<Event> = events(&state).into_iter().filter(|found| found.run == id).collect();
    let requested = mine
        .iter()
        .find(|found| found.kind == EventKind::ApprovalRequested)
        .expect("ApprovalRequested を記帳する");
    assert_eq!(
        requested.detail.as_deref(),
        Some("delete+publish"),
        "何のクラスで止めたかを名指す（複数なら全部・区切りは +）"
    );
    assert_eq!(requested.actor, "machine", "止めたのは機械であって人の event ではない");
    assert!(
        mine.iter().any(|found| found.stage == Some(Stage::Blocked)),
        "段は Blocked に落ちる"
    );
    let line = show_line(&repo, &state, &id);
    assert!(line.contains("stage=Blocked"), "{line}");
    assert!(line.contains("approved=false"), "{line}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_records_verbatim_as_human_event() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let (id, _) = blocked(&repo, &state, &marker, r#"classes = ["publish"]"#);
    // 引用符も全角も入った 1 行を **要約せずそのまま** 通す（C7.2）。前後の空白と
    // 大文字を混ぜてあるのは、正規化（trim / 小文字化）を「そのまま」と言い張れない
    // ようにするためである——fixture が綺麗だと歯は正規化を見逃す。
    let words = r#"  OK：出してよい（user 逐語 2026-09-09）："推奨で進めて"  "#;
    let out = run_pipe(&[
        "approve", "--run", &id, "--words", words,
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let human: Vec<Event> =
        events(&state).into_iter().filter(|found| found.actor == "human").collect();
    assert_eq!(human.len(), 1, "人由来の event は承認の 1 件だけである（FR22 の計測面）");
    let received = human.first().expect("承認 event が 1 件在る");
    assert_eq!(received.kind, EventKind::ApprovalReceived, "種類は ApprovalReceived");
    assert_eq!(received.detail.as_deref(), Some(words), "逐語をそのまま持つ");
    assert_eq!(received.bead, "s2-2e5", "どの契約への承認かを持つ");
    let line = show_line(&repo, &state, &id);
    assert!(line.contains("approved=true"), "{line}");
    // 承認は「許し」であって「前進」ではない——段を動かすのは resume である。
    assert!(line.contains("stage=Blocked"), "{line}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_refuses_empty_words() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let (id, _) = blocked(&repo, &state, &marker, r#"classes = ["consume"]"#);
    let before = event_count(&state);
    // 空も空白だけも承認ではない。「聞いた形」だけが残る記録を作らない。
    for words in ["", "   "] {
        let out = run_pipe(&[
            "approve", "--run", &id, "--words", words,
            "--state-dir", &state.display().to_string(),
        ]);
        assert_eq!(
            out.status.code(),
            Some(i32::from(RC_REFUSED)),
            "空の逐語を承認にしない: {words:?}"
        );
    }
    assert_eq!(event_count(&state), before, "1 byte も書かない");
    assert!(show_line(&repo, &state, &id).contains("approved=false"), "承認は立たない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_resume_spawns_after_received() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let (id, _) = blocked(&repo, &state, &marker, r#"classes = ["delete"]"#);
    let approved = run_pipe(&[
        "approve", "--run", &id, "--words", "消してよい",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(approved.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&approved));
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", &runner_cmd(&marker),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    // 効果で測る: 承認の後は runner が実際に起き、便が先の段へ進む。
    assert!(marker.exists(), "承認の後は runner が起きる");
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段が進む");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_resume_stays_blocked_without_received() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let (id, _) = blocked(&repo, &state, &marker, r#"classes = ["publish"]"#);
    let before = event_count(&state);
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", &runner_cmd(&marker),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "未承認の resume は rc 3");
    assert!(!marker.exists(), "runner を起こさない");
    // 待っている事実は Blocked が既に持っている。resume のたびに積むと
    // 「何回聞いたか」が事実と食い違う。
    assert_eq!(event_count(&state), before, "何も書かない");

    // **資格の無い `ApprovalReceived` では関門は開かない**（憲法 C7.2・planner 裁定 Q1）。
    // `fleet record` は公開の口なので、書き手（`pipe approve`）の逐語検査だけでは
    // 「承認は event に残った逐語だけ」を守れない——読み手が資格を見る。
    for disqualified in [
        vec!["--actor", "machine"],
        vec!["--actor", "human"],
        vec!["--actor", "human", "--detail", "   "],
        vec!["--actor", "machine", "--detail", "出してよい"],
    ] {
        let wrote = record_approval(&state, &id, &disqualified);
        assert_eq!(
            wrote.status.code(),
            Some(i32::from(RC_OK)),
            "event を積むこと自体はできる（塞ぐのは読み手である）: {}",
            stderr_of(&wrote)
        );
        assert!(
            show_line(&repo, &state, &id).contains("approved=false"),
            "資格の無い承認で関門は開かない: {disqualified:?}"
        );
        let again = run_pipe(&[
            "resume", "--run", &id, "--repo", &repo.display().to_string(),
            "--state-dir", &state.display().to_string(),
            "--runner", &runner_cmd(&marker),
        ]);
        assert_eq!(
            again.status.code(),
            Some(i32::from(RC_BLOCKED)),
            "資格の無い承認の後も rc 3 のまま: {disqualified:?}"
        );
        assert!(!marker.exists(), "runner を起こさない: {disqualified:?}");
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_unlisted_class_value_is_rejected_at_intake() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[r#"classes = ["deploy"]"#]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "名簿に無いクラスを通さない");
    let err = stderr_of(&out);
    assert!(err.contains("deploy"), "断る値を名指す: {err}");
    for listed in ["delete", "publish", "consume"] {
        assert!(err.contains(listed), "取れる値を全部見せる: {err}");
    }
    assert_eq!(event_count(&state), 0, "断った便は 1 行も記帳しない");
    // **弁別**: 断っているのは「classes が在ること」ではなく **値**である。
    let listed = write_contract(&repo, &[], &[r#"classes = ["publish"]"#]);
    let ok = run_pipe(&[
        "intake", "--contract", &listed.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(ok.status.code(), Some(i32::from(RC_OK)), "名簿に在る値は通す: {}", stderr_of(&ok));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_blocks_in_one_shot_run() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let path = write_contract(&repo, &[], &[r#"classes = ["publish"]"#]);
    // **一発経路**（intake → spawn → gate → land を 1 process で通す）でも関門は効く。
    // 段ごとの口だけを測ると、この経路だけ素通りする実装に気づけない——関門は唯一の
    // 起動口 `spawn()` に在るという主張を、経路の側から裏書きする歯である。
    let out = run_pipe(&[
        "run", "--contract", &path.display().to_string(), "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", &runner_cmd(&marker),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BLOCKED)),
        "一発経路でも spawn の手前で止まる: {}",
        stderr_of(&out)
    );
    assert!(!marker.exists(), "runner を起こさない");
    let id = run_id_of(&out);
    assert!(!id.is_empty(), "止まった周も run id を出す: {}", stdout_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Blocked"), "段は Blocked に落ちる");
    clean(&[&repo, &state]);
}

// ── (e) 到達点の計測（設計 §8 (e)・§9・FR22 / AC1 / AC2・憲法 A1） ──────────

/// `pipe report` を 1 回撃つ。
fn report_once(state: &Path) -> Output {
    run_pipe(&["report", "--state-dir", &state.display().to_string()])
}

/// `fleet record` で人由来の event を 1 件積む（**`pipe approve` を通さない**）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn record_human_stage(state: &Path, id: &str) -> Output {
    Command::new(bin())
        .args(["fleet", "record", "--state-dir"])
        .arg(state)
        .args([
            "--kind", "RunStage", "--actor", "human", "--run", id, "--bead", "s2-2e5",
            "--stage", "Intake", "--detail", "手で段を動かした",
        ])
        .output()
        .expect("binary を起動できる")
}

#[test]
fn pipe_report_counts_human_events() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    // 1 便を land まで通す（機械だけで進む便）。
    let landed_id = gated_pass(&repo, &state, &path, &marker);
    let landed = land_once(&repo, &state, &landed_id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    // もう 1 便は intake で止める（`landed` に数えない側）。
    let open_id = intake_bead(&repo, &state, &path, "s2-open");
    let approved = run_pipe(&[
        "approve", "--run", &open_id, "--words", "推奨で進めて",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(approved.status.code(), Some(i32::from(RC_OK)), "approve: {}", stderr_of(&approved));

    let out = report_once(&state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "report は rc 0: {}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out).trim(),
        "runs=2 landed=1 human_events=1 human_events_other_than_approval=0",
        "到達点の 1 行（設計 §5.8）"
    );

    // **approval 以外の人由来 event は別に数える**——ここが 0 であることが到達点の主張
    // なので、0 のままにしか動かない数え方だと主張を測れない。
    let recorded = record_human_stage(&state, &open_id);
    assert_eq!(recorded.status.code(), Some(i32::from(RC_OK)), "record: {}", stderr_of(&recorded));
    let after = report_once(&state);
    assert_eq!(
        stdout_of(&after).trim(),
        "runs=2 landed=1 human_events=2 human_events_other_than_approval=1",
        "approval 以外の人由来 event を数える"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_report_returns_rc2_on_malformed_store() {
    let (repo, state) = repo_with_state();
    let events = state.join("fleet").join("events.jsonl");
    fs::create_dir_all(state.join("fleet")).expect("dir を作れる");
    fs::write(&events, "こわれ\n").expect("壊れた行を書ける");
    // **数えられなかったを 0 に化けさせない**（C11.2）。到達点の 1 行は「人手 0」を
    // 主張する面なので、読めない台帳から 0 を出すと**偽の全クリア**そのものになる。
    let out = report_once(&state);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない台帳は rc 2");
    assert!(out.stdout.is_empty(), "rc 2 でも数を出さない");
    assert!(
        !stdout_of(&out).contains("human_events_other_than_approval=0"),
        "0 を名乗らない: {}",
        stdout_of(&out)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_pr_cmd_runs_without_approval_event() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let before = git(&repo, &["rev-parse", "refs/heads/main"]);
    let sent = state.join("pr-args");
    // **自 repo への PR は「出す」ではない**（憲法 A4.3・ADR-0008）。main を動かさず
    // branch も PR も閉じられる＝可逆ゆえ、承認 event を積まない周でも道具は起動する。
    // 3 クラスの判定は契約の自己申告（`classes`）だけに効き、seam を使ったことから
    // 導出しない（publish を名乗る契約は従来どおり spawn の手前で Blocked＝別の歯が守る）。
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--pr-cmd", &format!("printf '%s %s' {{branch}} {{base}} > '{}'", sent.display()),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_OK)),
        "承認 event 無しでも rc 0: {}",
        stderr_of(&out)
    );
    // **道具が起動したことを file で測る**（器は道具の中身を知らない）。
    assert_eq!(
        fs::read_to_string(&sent).expect("seam へ渡した引数を読める"),
        format!("scribe2/{id} {before}"),
        "`{{branch}}` と `{{base}}` を置換して渡す"
    );
    // **main は動かさない**（merge は人が押す・A4.3 の可逆はここに乗っている）。
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        before,
        "main は 1 byte も動かない"
    );
    assert!(
        show_line(&repo, &state, &id).contains("stage=Landed"),
        "段は Landed へ進む: {}",
        show_line(&repo, &state, &id)
    );
    assert!(
        stdout_of(&out).contains("landed=pr"),
        "PR を出した形だと名乗る: {}",
        stdout_of(&out)
    );
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(
        log.contains("\"stage\":\"Landed\"") && log.contains("\"detail\":\"pr\""),
        "Landed detail=pr で終える: {log}"
    );
    assert!(
        !land::verdicts_path(&state).exists(),
        "面 5 は main へ載った便の記録ゆえ、PR の段階では書かない"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_pr_cmd_pushes_branch_without_moving_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);

    // **道具の失敗で便を終端させない**: push も PR 作成も network で落ちうるので、
    // `Failed` を焼くと再試行できない便が残る。rc 1 で何も書かず段も動かさない——だから
    // この後そのまま成功へ進める（この 2 段で「何も書かない」を測っている）。
    let broken_seam = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--pr-cmd", "exit 7",
    ]);
    assert_eq!(broken_seam.status.code(), Some(i32::from(RC_REFUSED)), "道具が落ちた周は rc 1");
    assert!(
        stderr_of(&broken_seam).contains("rc 7"),
        "道具の rc を理由に写す: {}",
        stderr_of(&broken_seam)
    );
    assert!(
        !show_line(&repo, &state, &id).contains("stage=Landed"),
        "段も動かない: {}",
        show_line(&repo, &state, &id)
    );

    // seam は `{branch}` `{base}` を置換して `sh -c` する。**道具の中身は器が知らない**
    // ので、置換の結果を file へ写して測る。
    let sent = state.join("pr-args");
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--pr-cmd", &format!("printf '%s %s' {{branch}} {{base}} > '{}'", sent.display()),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "承認 event 無しでも rc 0: {}", stderr_of(&out));
    let args = fs::read_to_string(&sent).expect("seam へ渡した引数を読める");
    assert_eq!(
        args,
        format!("scribe2/{id} {base}"),
        "`{{branch}}` と `{{base}}` を置換して渡す"
    );
    // **main は動かさない**（merge は人が押す・憲法 A4.3）。
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        base,
        "この形では main を進めない"
    );
    assert!(
        stdout_of(&out).contains("landed=pr"),
        "PR を出した形だと名乗る: {}",
        stdout_of(&out)
    );
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(
        log.contains("\"stage\":\"Landed\"") && log.contains("\"detail\":\"pr\""),
        "Landed detail=pr で終える: {log}"
    );
    // 便の worktree は畳まない（**merge は人が押すまで終わっていない**）。
    assert!(worktree_of(&repo, &id).exists(), "PR 待ちの worktree は残す");
    assert!(
        !land::verdicts_path(&state).exists(),
        "面 5 は main へ載った便の記録ゆえ、PR の段階では書かない"
    );

    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_pr_cmd_ignores_moved_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);

    // **別便が main を進めた状況**を作る。squash の口はここで stale base を理由に断るが、
    // PR の口は ref を 1 本も動かさないので CAS の old が要らない——ここで base を縛ると
    // main が動いた瞬間に PR を出せなくなる（自己ホストの便が最も踏む）。
    fs::write(repo.join("other.txt"), "x\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(moved, base, "main が進んでいる");

    let sent = state.join("pr-base");
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--pr-cmd", &format!("printf '%s' {{base}} > '{}'", sent.display()),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_OK)),
        "main が動いていても PR は出せる: {}",
        stderr_of(&out)
    );
    // **`{base}` は便が記録した base**（いまの main ではない）＝PR の比較先は便の出発点。
    assert_eq!(
        fs::read_to_string(&sent).expect("seam へ渡した base を読める"),
        base,
        "便の base を渡す（現在の main へ滑らせない）"
    );
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        moved,
        "main は 1 byte も動かさない"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_pr_cmd_refuses_empty_or_missing_value() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let before = event_count(&state);

    // 値欠け（SRS NFR4「黙って落とさない」）。
    let missing = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--pr-cmd",
    ]);
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "値欠けは rc 1");
    assert!(
        stderr_of(&missing).contains("値が無い"),
        "値欠けだと名乗る（squash 経路へ滑らせない）: {}",
        stderr_of(&missing)
    );

    // **空文字**。`sh -c ""` は rc 0 で終わるので、素通しすると 1 行も公開していないのに
    // 「PR を出した」を記帳する（何もしていないのに「やった」が残る）。
    let empty = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--pr-cmd", "",
    ]);
    assert_eq!(empty.status.code(), Some(i32::from(RC_REFUSED)), "空の seam は rc 1");
    assert!(
        !show_line(&repo, &state, &id).contains("stage=Landed"),
        "段も動かない: {}",
        show_line(&repo, &state, &id)
    );
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_report_counts_landed_runs_not_landed_events() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    // 同じ便へ `Landed` の event をもう 1 件積む（手で積んだ / 台帳が壊れた周）。
    // **replay は便を数える**ので landed は 1 のまま——生の行を数える実装だと 2 になる。
    let doubled = Command::new(bin())
        .args(["fleet", "record", "--state-dir"])
        .arg(&state)
        .args(["--kind", "RunDone", "--stage", "Landed", "--run", &id, "--bead", "s2-2e5"])
        .output()
        .expect("binary を起動できる");
    assert_eq!(doubled.status.code(), Some(i32::from(RC_OK)), "record: {}", stderr_of(&doubled));
    let out = report_once(&state);
    assert_eq!(
        stdout_of(&out).trim(),
        "runs=1 landed=1 human_events=0 human_events_other_than_approval=0",
        "landed は便の数であって event の数ではない"
    );
    clean(&[&repo, &state]);
}

/// toy repo の 1 便を intake → spawn まで通す（bead を分けて id の衝突を避ける）。
fn toy_spawn(repo: &Path, state: &Path, bead: &str, contract: &Path, runner: &str) -> (String, Output) {
    let id = intake_bead(repo, state, contract, bead);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ]);
    (id, out)
}

/// commit を 1 本作る fake runner の 1 行。
const TOY_COMMIT: &str = "echo x >> src/lib.rs && git add -A && git commit -q -m runner";

/// **marker を tracked にする**。便の worktree は base の checkout なので、`.vessel` が
/// commit されていない repo では worktree に marker が無く `served()` は Absent＝guard は
/// 黙る。本 repo の root へ `.vessel` を置く理由がこれである（設計 §9・AC2）。
fn track_marker(repo: &Path) {
    git(repo, &["add", "-f", ".vessel"]);
    git(repo, &["commit", "-q", "-m", "vessel"]);
}

/// toy repo の 5 便が共有する材料（引数の本数を線の内へ収める）。
struct Toy<'a> {
    /// 対象 repo。
    repo: &'a Path,
    /// 置き場。
    state: &'a Path,
    /// PASS を返す fake lens。
    lens: &'a str,
}

/// 1 便を intake → spawn → gate(PASS) → land まで通す（正常形）。
fn toy_land(toy: &Toy<'_>, bead: &str, contract: &Path, runner: &str) {
    let (repo, state, lens) = (toy.repo, toy.state, toy.lens);
    let (id, spawned) = toy_spawn(repo, state, bead, contract, runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "{bead} spawn: {}", stderr_of(&spawned));
    let gated = gate_once(repo, state, &id, Some(lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "{bead} gate: {}", stderr_of(&gated));
    let landed = land_once(repo, state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "{bead} land: {}", stderr_of(&landed));
}

/// write-set の外を編集しようとして guard に止まる便。**止めたのが guard であることまで測る**
/// （runner が別の理由で落ちた周と弁別する）。
fn toy_denied(toy: &Toy<'_>, contract: &Path) {
    let (repo, state) = (toy.repo, toy.state);
    let runner = format!(
        "printf '{{\"cwd\":\"%s\",\"tool_name\":\"Write\",\"tool_input\":{{\"file_path\":\"docs/out.md\"}}}}' \"$PWD\" \
         | '{}' hook pre-tool-use; test $? -eq 0 || exit 1; {TOY_COMMIT}",
        bin()
    );
    let (id, stopped) = toy_spawn(repo, state, "toy-guard", contract, &runner);
    // **spawn の rc は 0 のまま**（段が結果を運ぶ・設計 §5.2）。便の終わり方は段で読む。
    assert!(stdout_of(&stopped).contains("stage=Failed"), "guard に止まった便は Failed: {}", stdout_of(&stopped));
    assert!(
        show_line(repo, state, &id).contains("stage=Failed"),
        "永続面にも Failed が残る: {}",
        show_line(repo, state, &id)
    );
    let denies = fs::read_to_string(inject_path(state))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("\"what\":\"deny\""))
        .count();
    assert_eq!(denies, 1, "write-set の外への Write が 1 件 deny されている");
}

/// gate が FAIL する便（land しない）。
fn toy_gate_fail(toy: &Toy<'_>, contract: &Path, lens: &str) {
    let (repo, state) = (toy.repo, toy.state);
    let (id, spawned) = toy_spawn(repo, state, "toy-fail", contract, TOY_COMMIT);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));
    let gated = gate_once(repo, state, &id, Some(lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の gate は rc 1");
    let refused = land_once(repo, state, &id);
    assert_eq!(refused.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の便は land しない");
}

/// 3 クラスを名乗る便: spawn の手前で Blocked → approve（逐語）→ resume → gate → land。
fn toy_approved_land(toy: &Toy<'_>, contract: &Path) {
    let (repo, state, lens) = (toy.repo, toy.state, toy.lens);
    let (id, blocked) = toy_spawn(repo, state, "toy-approve", contract, TOY_COMMIT);
    assert_eq!(blocked.status.code(), Some(i32::from(RC_BLOCKED)), "承認待ちは rc 3");
    let approved = run_pipe(&[
        "approve", "--run", &id, "--words", "この便は出してよい",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(approved.status.code(), Some(i32::from(RC_OK)), "approve: {}", stderr_of(&approved));
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT,
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "resume: {}", stderr_of(&resumed));
    let gated = gate_once(repo, state, &id, Some(lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let landed = land_once(repo, state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
}

#[test]
fn pipe_five_contracts_land_with_fake_runner_in_toy_repo() {
    let (repo, state) = repo_with_state();
    let pass = fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"));
    let fail = fake_lens(&state.join("lens-fail"), &lens_verdict("FAIL"));
    let plain = write_contract(&repo, &[], &[]);
    // `Command::output` は stdin を /dev/null にする＝**人の入力を待つ余地が無い**形で
    // 5 便を通す（設計 §8 (e)）。
    track_marker(&repo);

    let toy = Toy { repo: &repo, state: &state, lens: &pass };
    toy_land(&toy, "toy-ok", &plain, TOY_COMMIT);
    toy_denied(&toy, &plain);
    let with_tests = write_contract(&repo, &["write-set"], &[r#"write-set = ["src/lib.rs", "tests/"]"#]);
    let add_test = "mkdir -p tests && echo '#[test] fn t() {}' > tests/new.rs \
                    && git add -A && git commit -q -m test";
    toy_land(&toy, "toy-test", &with_tests, add_test);
    toy_gate_fail(&toy, &plain, &fail);
    let publish = write_contract(&repo, &[], &[r#"classes = ["publish"]"#]);
    toy_approved_land(&toy, &publish);

    // **到達点**: 5 便のうち 3 便が main に載り、人由来の event は承認の 1 件だけ。
    let out = report_once(&state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "report: {}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out).trim(),
        "runs=5 landed=3 human_events=1 human_events_other_than_approval=0",
        "5 便の到達点（AC1 の形・人手は承認 1 件だけ）"
    );
    let exported = fs::read_to_string(land::verdicts_path(&state)).expect("面 5 を読める");
    assert_eq!(exported.lines().count(), 3, "main に載った便だけが面 5 に出る: {exported}");
    clean(&[&repo, &state]);
}
