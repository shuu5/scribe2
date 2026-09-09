//! 縦 1 本 (a) の歯（設計 docs/design/pipeline.md §8 (a)）。
//!
//! tmp の git repo を作り `vessel init --state-dir` で置き場を紐づけてから撃つ。
//! runner は `sh -c` の 1 行の fake で、実 Claude は (d) の手番である。
//! commit には identity が要るので **repo local** の設定を与える（global は触らない）。

use crate::make_tmp_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};

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
    git(&repo, &["init", "-q"]);
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
    let out = run_pipe(&[
        "intake",
        "--contract",
        &contract.display().to_string(),
        "--bead",
        "s2-2e5",
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
    assert!(id.starts_with("s2-2e5-"), "run id は <bead>-<stamp>: {id}");
    assert!(!id.contains(':'), "id は dir 名と branch 名になるので : を含まない: {id}");

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

    let env_text = fs::read_to_string(worktree.join("env.txt")).expect("env の写しを読める");
    let ours: Vec<&str> = env_text.lines().filter(|line| line.starts_with("SCRIBE2_")).collect();
    assert!(
        ours.is_empty(),
        "器固有の env を 1 つも足さない（母集団 {} 行 / 該当 {:?}）",
        env_text.lines().count(),
        ours
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
