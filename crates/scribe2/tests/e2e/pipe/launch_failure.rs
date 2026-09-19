//! 器の側の理由で終端に着いた便の歯（設計 docs/design/dispatcher.md §12・契約行 i・`s2-07l.495`）:
//! `pipe_spawn_runner_stderr_`（実装役の stderr を run dir に残し呼び手の stderr にも流す）と
//! `pipe_repo_relative_`（`--repo` の値を口で絶対 path に直す）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く。

use super::*;

/// 偽 runner が stderr へ書く理由の 1 行（**器の `pipe:` 行にも cmd にも無い語**で作る＝出所を区別できる）。
const LAUNCH_REASON: &str = "fake-runner: claude binary is missing";

/// stderr に理由の 1 行を書いて rc 2 で落ちる偽 runner（commit は作らない＝起動の失敗の形）。
fn failing_runner() -> String {
    format!("printf '%s\\n' '{LAUNCH_REASON}' >&2; exit 2")
}

/// 便の run dir の stderr の log。
fn stderr_log(state: &Path, id: &str) -> PathBuf {
    state.join("pipe").join(id).join("runner.stderr.log")
}

/// 便の最後の `RunStage` の `(段, detail)`。
fn last_stage(state: &Path, id: &str) -> Option<(Option<Stage>, Option<String>)> {
    stages(state, id).pop()
}

/// (a) stderr に 1 行書いて rc 2 で落ちる偽 runner の便は `Failed detail=runner-rc:2,commits:0` に着き、run dir の
/// `runner.stderr.log` にその 1 行が見出し行（`## <ts> rc=2`）付きで残り、呼び手の stderr にも同じ行が出る。
/// stdout の log とは別の file である（stderr の行は stdout の log に混ざらない）。
#[test]
fn pipe_spawn_runner_stderr_is_kept_in_run_dir_and_relayed() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let out = spawn_with(&repo, &state, &id, &failing_runner());
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "終端は記帳できた: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("stage=Failed"), "{}", stdout_of(&out));
    assert_eq!(
        last_stage(&state, &id),
        Some((Some(Stage::Failed), Some("runner-rc:2,commits:0".to_owned()))),
        "段の判定は rc と commit の数だけで決まる"
    );
    let kept = fs::read_to_string(stderr_log(&state, &id)).unwrap_or_default();
    let mut lines = kept.lines();
    let head = lines.next().unwrap_or_default();
    assert!(head.starts_with("## ") && head.ends_with(" rc=2"), "見出し行 `## <ts> rc=2`: {kept}");
    assert_eq!(lines.next(), Some(LAUNCH_REASON), "理由の 1 行が見出しの直後に残る: {kept}");
    assert!(
        stderr_of(&out).lines().any(|line| line == LAUNCH_REASON),
        "呼び手の stderr にも同じ行がそのまま出る: {}",
        stderr_of(&out)
    );
    let stdout_log = fs::read_to_string(state.join("pipe").join(&id).join("runner.stdout.log")).unwrap_or_default();
    assert!(!stdout_log.contains(LAUNCH_REASON), "stderr の行は stdout の log に混ざらない: {stdout_log}");
    clean(&[&repo, &state]);
}

/// (b) stderr が空の周は file を作らない（**包めない host で撃つ**・[`lean_path`]＝封じ込めの包みが出す行を
/// runner の言葉と混ぜない）。runner が rc 2 で落ちても、stderr に何も言わなければ log は無い。
#[test]
fn pipe_spawn_runner_stderr_empty_writes_no_file() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let out = run_pipe_with_path(
        &lean_path(&state),
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", "exit 2"],
    );
    assert!(stdout_of(&out).contains("stage=Failed"), "{}", stdout_of(&out));
    assert_eq!(
        last_stage(&state, &id),
        Some((Some(Stage::Failed), Some("runner-rc:2,commits:0".to_owned()))),
        "終端の段は同じ"
    );
    assert!(!stderr_log(&state, &id).exists(), "空の周は書かない");
    clean(&[&repo, &state]);
}

/// (c) stderr に書いても rc 0 で commit 1 の偽 runner は `Implemented` に着く（stderr の中身は判定の入力でない・C3.3）。
/// 残す規律は段に依らず同じ＝log には見出し `rc=0` 付きで同じ行が残る。
#[test]
fn pipe_spawn_runner_stderr_does_not_change_the_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let runner = format!("printf '%s\\n' '{LAUNCH_REASON}' >&2; {TOY_COMMIT}");
    let out = spawn_with(&repo, &state, &id, &runner);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert_eq!(last_stage(&state, &id), Some((Some(Stage::Implemented), None)), "stderr は段を変えない");
    let kept = fs::read_to_string(stderr_log(&state, &id)).unwrap_or_default();
    assert!(kept.lines().next().is_some_and(|head| head.starts_with("## ") && head.ends_with(" rc=0")), "見出し行: {kept}");
    assert!(kept.lines().any(|line| line == LAUNCH_REASON), "同じ行が残る: {kept}");
    clean(&[&repo, &state]);
}

/// (d) 連鎖（`pipe run`）でも理由の行は呼び手の stderr に届く: 起動の失敗で `Failed` に着いた周は spawn の段が
/// rc 0 で終わり（記帳はできた）、次の gate が段の不一致で止まる——その周の stderr に runner の 1 行が在る。
/// 列が起こす driver は `pipe run` の形なので、この経路で落ちると理由は端末にも log にも残らない。
#[test]
fn pipe_spawn_runner_stderr_reaches_caller_through_pipe_run() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let out = run_pipe(&[
        "run", "--design", &path, "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state), "--runner", &failing_runner(), "--lens", &review_lens_pass(&state),
    ]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "Failed の便は Landed まで進まない: {}", stdout_of(&out));
    let id = run_id_of(&out);
    assert!(!id.is_empty(), "止まった周も run id を出す: {}", stdout_of(&out));
    assert_eq!(
        last_stage(&state, &id),
        Some((Some(Stage::Failed), Some("runner-rc:2,commits:0".to_owned()))),
        "起動の失敗の終端"
    );
    assert!(
        stderr_of(&out).lines().any(|line| line == LAUNCH_REASON),
        "連鎖でも runner の行が呼び手の stderr に出る: {}",
        stderr_of(&out)
    );
    assert!(
        fs::read_to_string(stderr_log(&state, &id)).unwrap_or_default().lines().any(|line| line == LAUNCH_REASON),
        "run dir の log にも残る"
    );
    clean(&[&repo, &state]);
}

/// `git worktree list --porcelain` が持つ worktree の path の集合（repo から見た git の記録）。
fn worktree_paths(repo: &Path) -> BTreeSet<String> {
    git(repo, &["worktree", "list", "--porcelain"])
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(str::to_owned)
        .collect()
}

/// (e) `--repo` を相対 path で渡した `pipe run` は、絶対 path で渡した周と**同じ worktree の場所**
/// （`<repo>/.worktrees/scribe2/<id>`・git の記録も絶対）と**同じ段**（`Landed`）に着く。cwd は repo の親で、
/// `--repo` はその leaf 名だけ（`--state-dir` は絶対のまま＝相対で効くのは repo の側だけである）。
#[test]
fn pipe_repo_relative_path_lands_in_the_same_worktree_as_absolute() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let rules = ceiling_rules(&state);
    let lens = review_lens_pass(&state);
    let absolute = run_pipe(&[
        "run", "--design", &path, "--bead", "s2-abs",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules, "--runner", TOY_COMMIT, "--lens", &lens,
    ]);
    assert_eq!(absolute.status.code(), Some(i32::from(RC_OK)), "絶対: {} / {}", stdout_of(&absolute), stderr_of(&absolute));
    let abs_id = run_id_of(&absolute);
    assert!(show_line(&repo, &state, &abs_id).contains("stage=Landed"), "絶対の周は Landed");
    let parent = repo.parent().map(Path::to_path_buf).unwrap_or_default();
    let leaf = repo.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let relative = Command::new(bin())
        .current_dir(&parent)
        .args([
            "pipe", "run", "--design", &path, "--bead", "s2-rel",
            "--repo", &leaf, "--state-dir", &state.display().to_string(),
            "--rules", &rules, "--runner", TOY_COMMIT, "--lens", &lens,
        ])
        .output()
        .expect("binary を起動できる");
    assert_eq!(relative.status.code(), Some(i32::from(RC_OK)), "相対: {} / {}", stdout_of(&relative), stderr_of(&relative));
    let rel_id = run_id_of(&relative);
    assert!(!rel_id.is_empty(), "run id を出す: {}", stdout_of(&relative));
    assert!(show_line(&repo, &state, &rel_id).contains("stage=Landed"), "相対の周も同じ段（Landed）");
    // worktree の場所は絶対 path で同じ規則（land 後は `<repo>/.worktrees/scribe2/retired/<id>` へ寄せられる）。git の
    // 記録（`worktree list`）も絶対で、cwd の下に `leaf/leaf/...` の形で切られていない。
    for id in [&abs_id, &rel_id] {
        let worktree = repo.join(".worktrees").join("scribe2").join("retired").join(id);
        assert!(worktree.join(".git").exists(), "{id}: worktree は repo の直下の規則の場所に在る: {}", worktree.display());
        assert!(worktree_paths(&repo).contains(&worktree.display().to_string()), "{id}: git の記録も絶対 path");
    }
    assert!(!parent.join(&leaf).join(&leaf).exists(), "相対のまま git -C に渡した形（leaf/leaf）を作らない");
    // 便の写し面の repo も絶対（`show` や `resume` が cwd に依らず読める）。
    let remembered = fs::read_to_string(state.join("pipe").join(&rel_id).join("repo")).unwrap_or_default();
    assert_eq!(remembered.trim(), repo.display().to_string(), "写し面の repo は絶対 path");
    clean(&[&repo, &state]);
}
