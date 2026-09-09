//! 縦 1 本 (a) / (b) の歯（設計 docs/design/pipeline.md §8 (a) / (b)）。
//!
//! tmp の git repo を作り `vessel init --state-dir` で置き場を紐づけてから撃つ。
//! runner は `sh -c` の 1 行の fake で、実 Claude は (d) の手番である。
//! commit には identity が要るので **repo local** の設定を与える（global は触らない）。

use crate::make_tmp_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
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
    let rules = repo.join("tight.toml");
    fs::write(
        &rules,
        "schema = 1\n\n[[rule]]\nid = \"gate.lens_count\"\nkind = \"GateLensCount\"\nvalue = 1\nruling = \"t\"\nruled_at = \"d\"\n\n[[rule]]\nid = \"gate.token_cap\"\nkind = \"GateTokenCap\"\nvalue = 1\nruling = \"t\"\nruled_at = \"d\"\n\n[[rule]]\nid = \"fleet.lock_retry_ms\"\nkind = \"LockRetryMs\"\nvalue = 5000\nruling = \"t\"\nruled_at = \"d\"\n\n[[rule]]\nid = \"fleet.lock_stale_ms\"\nkind = \"LockStaleMs\"\nvalue = 30000\nruling = \"t\"\nruled_at = \"d\"\n",
    )
    .expect("tmp manifest を書ける");
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
    // 人由来の event は 1 件も無い（承認の要らない契約ゆえ）。
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(!log.contains("\"actor\":\"human\""), "人手 0 で通る: {log}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_resume_after_kill_between_spawn_and_gate() {
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
