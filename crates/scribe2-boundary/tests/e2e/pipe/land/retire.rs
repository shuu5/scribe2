// flip-check: moved s2-07l.684
//! retire と train の族の歯（接頭辞 `pipe_retire_` / `pipe_train_`・設計 docs/design/carry-prep.md §10 行 l・親 `tests/e2e/pipe/land.rs` の helper を `use super::*` で使う）。

use super::*;

#[test]
fn pipe_retire_moves_pr_landed_worktree_and_keeps_branch() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = landed_pr(&repo, &state, &path, &marker);
    let live = worktree_of(&repo, &id);
    // `--pr-cmd` 形は worktree を畳まない（merge は人が押す）＝retire の入口の前提である。
    assert!(live.exists(), "PR 形の land の後も便の worktree は在る");

    let out = retire_fold_only(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "retire は rc 0: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(
        stdout_of(&out).contains(&format!("retired={}", retired.display())),
        "畳んだ先を名乗る: {}",
        stdout_of(&out)
    );
    // **削除しない**（N1.2）: 中身が move で運ばれている。
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    assert!(!live.exists(), "元の場所には残らない");
    let branches = git(&repo, &["branch", "--list", &format!("scribe2/{id}")]);
    assert!(!branches.trim().is_empty(), "branch は消さない: {branches}");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let last = log.lines().rfind(|line| !line.is_empty()).unwrap_or_default();
    assert!(
        last.contains("\"stage\":\"Landed\"") && last.contains("\"detail\":\"retired\""),
        "最終行は Landed detail=retired（段は Landed のまま）: {last}"
    );
    let after = event_count(&state);

    // 2 度目は前提（worktree が在る）を満たさない＝**rc 1 で何も書かない**。畳んだ先へ
    // 2 周目の move を当てると、retired/<id>/<id> のような入れ子が静かに生まれる。
    let again = retire_fold_only(&repo, &state, &id);
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "2 度目は rc 1");
    assert_eq!(event_count(&state), after, "前提違反は event を 1 件も書かない");
    assert!(retired.exists(), "畳んだ先は在るまま");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_retire_refuses_unless_landed_and_clean() {
    // **2 つの前提は「断ってから解いて通す」で測る**。rc 1 だけを見ると、subcommand を
    // 持っていない器でも同じ rc 1 が返るので歯が空虚になる（stderr の文言は pin しない）。
    //
    // (a) 段が Gated のまま＝**終端していない便の worktree は畳まない**。
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let before = event_count(&state);
    let early = retire_fold_only(&repo, &state, &id);
    assert_eq!(early.status.code(), Some(i32::from(RC_REFUSED)), "Landed 以外は rc 1");
    assert!(worktree_of(&repo, &id).exists(), "断った周は worktree を動かさない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    // 段だけを解くと同じ便が通る＝上の rc 1 は**段**を理由にしている。
    land_pr(&repo, &state, &id);
    let landed = retire_fold_only(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "Landed なら通る: {}", stderr_of(&landed));
    clean(&[&repo, &state]);

    // (b) Landed でも worktree が dirty なら畳まない（fail-closed・untracked も数える）。
    // move は中身ごと運ぶので、未 commit の仕事を持った worktree を黙って動かすと
    // 「どこへ行ったか」が便の外から読めなくなる。
    let (dirty_repo, dirty_state) = repo_with_state();
    let dirty_path = write_contract(&dirty_repo, &[], &[]);
    let dirty_marker = dirty_state.join("lens-ran");
    let dirty_id = landed_pr(&dirty_repo, &dirty_state, &dirty_path, &dirty_marker);
    let live = worktree_of(&dirty_repo, &dirty_id);
    let stray = live.join("dirty.txt");
    fs::write(&stray, "x\n").expect("worktree を汚せる");
    let dirty_before = event_count(&dirty_state);
    let out = retire_fold_only(&dirty_repo, &dirty_state, &dirty_id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "dirty な worktree は rc 1");
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert!(stray.exists(), "汚れもそのまま残す（掃除しない）");
    assert_eq!(event_count(&dirty_state), dirty_before, "event を 1 件も書かない");
    let dirty_retired = dirty_repo.join(".worktrees").join("scribe2").join("retired").join(&dirty_id);
    assert!(!dirty_retired.exists(), "retired/<id> を作らない");
    // 汚れだけを拭うと同じ便が通る＝上の rc 1 は**clean**を理由にしている。
    fs::remove_file(&stray).expect("汚れを拭える");
    let cleaned = retire_fold_only(&dirty_repo, &dirty_state, &dirty_id);
    assert_eq!(cleaned.status.code(), Some(i32::from(RC_OK)), "clean なら通る: {}", stderr_of(&cleaned));
    assert!(dirty_retired.exists(), "畳んだ先が出来る");
    clean(&[&dirty_repo, &dirty_state]);
}

/// 同一変更の 2 便: 2 本目が `Failed detail=rebase-empty` で終端した後、その worktree を
/// `pipe retire` が畳む（`s2-07l.128`）。成果は既に main に在り**入れ物だけが残る**形は
/// `--pr-cmd` 形の `Landed` と同じで、畳み方も同じ 1 本（move・branch は残す・main 不変）。
/// 残す event の段は **`Failed` のまま**＝retire は終端を動かさない。
#[test]
fn pipe_retire_rebase_empty_folds_failed_run_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_b, landed) = gated_run_whose_change_is_already_on_main(&repo, &state, &marker);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let empty = run_pipe(&[
        "land", "--run", &id_b, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(
        empty.status.code(),
        Some(i32::from(RC_REFUSED)),
        "2 本目は rebase-empty で終端する: {}",
        stderr_of(&empty)
    );
    assert!(show_line(&repo, &state, &id_b).contains("stage=Failed"), "終端の段は Failed");
    let live = worktree_of(&repo, &id_b);
    assert!(live.exists(), "終端した便の worktree は残る（retire の入口の前提）");

    let out = retire_once(&repo, &state, &id_b);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rebase-empty の便も畳める: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id_b);
    assert!(
        stdout_of(&out).contains(&format!("retired={}", retired.display())),
        "畳んだ先を名乗る: {}",
        stdout_of(&out)
    );
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    assert!(!live.exists(), "元の場所が空く");
    let branches = git(&repo, &["branch", "--list", &format!("scribe2/{id_b}")]);
    assert!(!branches.trim().is_empty(), "branch は消さない: {branches}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), landed, "main は 1 byte も動かない");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let last = log.lines().rfind(|line| !line.is_empty()).unwrap_or_default();
    assert!(
        last.contains("\"stage\":\"Failed\"") && last.contains("\"detail\":\"retired\""),
        "最終行は Failed detail=retired（段を Landed へ動かさない）: {last}"
    );
    assert!(show_line(&repo, &state, &id_b).contains("stage=Failed"), "畳んだ後も段は Failed");
    clean(&[&repo, &state]);
}

/// `Failed` の便は **detail を問わず**畳める（設計 §24 の約束 1 / 2）。base が断っていた 4 つの
/// detail を 1 つずつ名指す 4 本の 1 本目——**main の実測が赤かった便**（`main-red`）。
///
/// 段の弁別から detail を外しても **clean の検査は残る**ので、汚れた木で断ってから拭って通す
/// 対で測る（上の rc 1 は clean を理由にしており、detail ではない）。
#[test]
fn pipe_retire_failed_any_detail_main_red_folds_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    // 1 回目（worktree）は緑・2 回目（main の実測）は赤になる verify 行＝`main-red` で終端する。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-once.sh"]"#]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    make_tree_differ(&repo, &state, &id, "refs/heads/main");
    let red = land_once(&repo, &state, &id);
    assert_eq!(red.status.code(), Some(i32::from(RC_REFUSED)), "main が赤い land は rc 1: {}", stderr_of(&red));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("main-red".to_owned()))),
        "終端の理由は main-red: {:?}",
        stages(&state, &id)
    );

    // 対の負例: 汚れた木は畳まない（rc 1・worktree 不動・event 0 増）。
    let live = worktree_of(&repo, &id);
    let stray = live.join("dirty.txt");
    fs::write(&stray, "x\n").expect("worktree を汚せる");
    let before = event_count(&state);
    let dirty = retire_once(&repo, &state, &id);
    assert_eq!(dirty.status.code(), Some(i32::from(RC_REFUSED)), "dirty な worktree は rc 1: {}", stdout_of(&dirty));
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert!(
        !repo.join(".worktrees").join("scribe2").join("retired").join(&id).exists(),
        "retired/<run> を作らない"
    );
    assert_eq!(event_count(&state), before, "前提違反は event を 1 件も書かない");
    fs::remove_file(&stray).expect("汚れを拭える");

    folds_and_keeps_stage(&repo, &state, &id, Stage::Failed);
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "畳んだ後も段は Failed");
    clean(&[&repo, &state]);
}

/// 2 本目: main を**実測できなかった**便（`main-unmeasured`・rc 2 で終端する側）。
#[test]
fn pipe_retire_failed_any_detail_main_unmeasured_folds_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    make_tree_differ(&repo, &state, &id, "refs/heads/main");
    // main 実測用の tmp の置き場を塞ぐ＝**verify を 1 行も撃てない**
    // （`pipe_land_reports_unmeasured_main_apart_from_red` と同じ fixture）。
    let blocked = repo.join(".worktrees").join("scribe2").join("verify").join(&id);
    fs::create_dir_all(&blocked).expect("tmp の置き場を塞げる");
    fs::write(blocked.join("occupied"), "x\n").expect("塞げる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "測れない周は rc 2: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("main-unmeasured".to_owned()))),
        "終端の理由は main-unmeasured: {:?}",
        stages(&state, &id)
    );
    folds_and_keeps_stage(&repo, &state, &id, Stage::Failed);
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "畳んだ後も段は Failed");
    clean(&[&repo, &state]);
}

/// 3 本目: 追随の rebase の**途中で** turn が終わった便（`rebase-dirty`）。木が rebase の途中＝
/// dirty なので retire は clean の検査で断り、木を戻すと同じ便が通る——この対で「断りは clean・
/// 段の弁別は `Failed` を detail ごと通す」の両方が測れる。
#[test]
fn pipe_retire_failed_any_detail_rebase_dirty_folds_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, LEAVE_MID_REBASE);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "rebase の途中は rc 1: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-dirty".to_owned()))),
        "終端の理由は rebase-dirty: {:?}",
        stages(&state, &id)
    );
    assert!(mid_rebase(&repo, &id), "木は rebase の途中のまま（器は触らない）");

    let live = worktree_of(&repo, &id);
    let before = event_count(&state);
    let dirty = retire_once(&repo, &state, &id);
    assert_eq!(dirty.status.code(), Some(i32::from(RC_REFUSED)), "rebase の途中の木は rc 1: {}", stdout_of(&dirty));
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert_eq!(event_count(&state), before, "前提違反は event を 1 件も書かない");
    // 木を戻すと同じ便が通る＝上の rc 1 は clean を理由にしている（終端の理由ではない）。
    git(&live, &["rebase", "--abort"]);
    folds_and_keeps_stage(&repo, &state, &id, Stage::Failed);
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は 1 byte も動かない");
    clean(&[&repo, &state]);
}

/// 4 本目: gate の前提違反で終端した便（`precheck:…`）。commit を 1 本も持たない木は gate が
/// `Failed detail=precheck:…` で残す（木は clean のまま）＝ここでの rc 0 は **detail の弁別が
/// 無い**ことだけを測る。
#[test]
fn pipe_retire_failed_any_detail_precheck_folds_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 便の commit を捨てる＝precheck が「commit が 1 本も無い」で落ちる（木は clean のまま）。
    let live = worktree_of(&repo, &id);
    git(&live, &["reset", "--hard", "refs/heads/main"]);
    assert!(git(&live, &["status", "--porcelain"]).is_empty(), "木は clean のまま");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "前提違反は rc 1: {}", stderr_of(&out));
    assert!(!marker.exists(), "**lens を起動しない**（precheck で止まる周）");
    let terminal = stages(&state, &id).last().cloned();
    assert!(
        matches!(&terminal, Some((Some(Stage::Failed), Some(detail))) if detail.starts_with("precheck:")),
        "終端の理由は precheck:…: {terminal:?}"
    );
    folds_and_keeps_stage(&repo, &state, &id, Stage::Failed);
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "畳んだ後も段は Failed");
    clean(&[&repo, &state]);
}

/// 退行の pin（設計 §24 の約束 4）: **非終端**（`Spawned` / `Implemented`）と `Gated(PASS)` は
/// 畳まない。`Failed` から detail の弁別を外しても `allowed` の列は不変で、段の一般則は効き
/// 続ける——`Spawned` は終端させると同じ便が通る＝その rc 1 は**段**を理由にしている。
#[test]
fn pipe_retire_failed_any_detail_still_refuses_live_runs_and_gated_pass() {
    // (a) `Spawned`（runner が生きている便）。
    let (repo, state) = repo_with_state();
    let id = spawned_run(&repo, &state);
    let live = worktree_of(&repo, &id);
    let before = event_count(&state);
    let spawned = retire_once(&repo, &state, &id);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_REFUSED)), "Spawned は rc 1: {}", stdout_of(&spawned));
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    // 段を終端（`Stopped`）へ解くと同じ便が通る＝`allowed` の列は不変である。
    stop_run_ok(&state, &id);
    let stopped = retire_once(&repo, &state, &id);
    assert_eq!(stopped.status.code(), Some(i32::from(RC_OK)), "Stopped なら通る: {}", stderr_of(&stopped));
    clean(&[&repo, &state]);

    // (b) `Implemented`（gate を待つ便）→ (c) `Gated(PASS)`（land が残っている便）。
    let (other, other_state) = repo_with_state();
    let path = write_contract(&other, &[], &[]);
    let waiting_id = implemented(&other, &other_state, &path);
    let waiting_tree = worktree_of(&other, &waiting_id);
    let waiting_before = event_count(&other_state);
    let waiting = retire_once(&other, &other_state, &waiting_id);
    assert_eq!(waiting.status.code(), Some(i32::from(RC_REFUSED)), "Implemented は rc 1: {}", stdout_of(&waiting));
    assert!(waiting_tree.exists(), "断った周は worktree を動かさない");
    assert_eq!(event_count(&other_state), waiting_before, "event を 1 件も書かない");

    let marker = other_state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&other, &other_state, &waiting_id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate は rc 0: {}", stderr_of(&gated));
    let pass_before = event_count(&other_state);
    let pass = retire_once(&other, &other_state, &waiting_id);
    assert_eq!(pass.status.code(), Some(i32::from(RC_REFUSED)), "Gated(PASS) は rc 1: {}", stdout_of(&pass));
    assert!(waiting_tree.exists(), "断った周は worktree を動かさない");
    assert!(
        !other.join(".worktrees").join("scribe2").join("retired").join(&waiting_id).exists(),
        "retired/<run> を作らない"
    );
    assert_eq!(event_count(&other_state), pass_before, "event を 1 件も書かない");
    clean(&[&other, &other_state]);
}

/// `Stopped` の便を `pipe retire` が畳む（`s2-07l.284`・設計 pipeline-conflict.md §5）。stop は
/// 畳まない（C2）ので、止めた便の commit 0・clean の worktree はこの口でしか動かせない。畳み方は
/// 他の終端と同じ 1 本（move・branch は残す・main 不変）で、残す event の段は **`Stopped` のまま**。
#[test]
fn pipe_retire_stopped_folds_a_clean_stopped_run_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let main_before = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = stopped_run(&repo, &state);
    let live = worktree_of(&repo, &id);

    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "Stopped の便も畳める: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(
        stdout_of(&out).contains(&format!("retired={}", retired.display())),
        "畳んだ先を名乗る: {}",
        stdout_of(&out)
    );
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    assert!(!live.exists(), "元の場所が空く");
    let branches = git(&repo, &["branch", "--list", &format!("scribe2/{id}")]);
    assert!(!branches.trim().is_empty(), "branch は消さない: {branches}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main_before, "main は 1 byte も動かない");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let last = log.lines().rfind(|line| !line.is_empty()).unwrap_or_default();
    assert!(
        last.contains("\"stage\":\"Stopped\"") && last.contains("\"detail\":\"retired\""),
        "最終行は Stopped detail=retired（段を Landed へ動かさない）: {last}"
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Stopped"), "畳んだ後も段は Stopped");
    let after = event_count(&state);

    // 2 度目は前提（worktree が在る）を満たさない＝rc 1 で何も書かない。
    let again = retire_once(&repo, &state, &id);
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "2 度目は rc 1");
    assert_eq!(event_count(&state), after, "前提違反は event を 1 件も書かない");
    assert!(retired.exists(), "畳んだ先は在るまま");
    clean(&[&repo, &state]);
}

/// 負例: `Stopped` でも worktree が dirty なら畳まない（rc 1・worktree 不動・event 0 増）。
/// 負例を**clean 検査で断る位置**に置く＝段の検査は通っている（上の歯との対で、`Stopped` に
/// `Extra::Retire` の clean 検査が同じく効くことを担保する・untracked も数える）。
#[test]
fn pipe_retire_stopped_refuses_a_dirty_worktree() {
    let (repo, state) = repo_with_state();
    let id = stopped_run(&repo, &state);
    let live = worktree_of(&repo, &id);
    let stray = live.join("dirty.txt");
    fs::write(&stray, "x\n").expect("worktree を汚せる");
    let before = event_count(&state);

    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "dirty な worktree は rc 1: {}", stdout_of(&out));
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert!(stray.exists(), "汚れもそのまま残す（掃除しない）");
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(!retired.exists(), "retired/<run> を作らない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Stopped"), "段は Stopped のまま");
    // 汚れだけを拭うと同じ便が通る＝上の rc 1 は**clean**を理由にしている（段ではない）。
    fs::remove_file(&stray).expect("汚れを拭える");
    let cleaned = retire_once(&repo, &state, &id);
    assert_eq!(cleaned.status.code(), Some(i32::from(RC_OK)), "clean なら通る: {}", stderr_of(&cleaned));
    assert!(retired.exists(), "畳んだ先が出来る");
    clean(&[&repo, &state]);
}

/// 審査が **FAIL** で終端した `Reviewed` の便を畳む（判定に届いた終端・段の列に `Reviewed` が在る）。
/// 畳み方は他の終端と同じ 1 本（move・branch は残す・main 不変）で、残す event の段は `Reviewed` のまま。
#[test]
fn pipe_retire_reviewed_fail_folds_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let id = reviewed_with_worktree(&repo, &state, &review_body("FAIL"));
    let main_before = git(&repo, &["rev-parse", "refs/heads/main"]);
    folds_and_keeps_stage(&repo, &state, &id, Stage::Reviewed);
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main_before, "main は 1 byte も動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Reviewed"), "畳んだ後も段は Reviewed");
    let after = event_count(&state);

    // 2 度目は前提（worktree が在る）を満たさない＝rc 1 で何も書かない（入れ子の retired/<run>/<run> を作らない）。
    let again = retire_once(&repo, &state, &id);
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "2 度目は rc 1");
    assert_eq!(event_count(&state), after, "前提違反は event を 1 件も書かない");
    clean(&[&repo, &state]);
}

/// 審査が **INCONCLUSIVE** で終端した便も同じく畳める（FAIL との弁別は入口に無い＝どちらも「この材料では
/// 通らなかった」終端）。残す event の段は `Reviewed` のまま・`detail=retired`。
#[test]
fn pipe_retire_reviewed_inconclusive_folds_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let id = reviewed_with_worktree(&repo, &state, &review_body("INCONCLUSIVE"));
    let main_before = git(&repo, &["rev-parse", "refs/heads/main"]);
    folds_and_keeps_stage(&repo, &state, &id, Stage::Reviewed);
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main_before, "main は 1 byte も動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Reviewed"), "畳んだ後も段は Reviewed");
    clean(&[&repo, &state]);
}

/// 負例 (1): 審査が **PASS** の便は畳まない——これから起こす側（live）で、入れ物は次の段が使う。
///
/// 断りは**段だけでなく判定も名乗る**。字面は 1 行丸ごとで測る＝段違いの一般則の字面（`run <id> の段は
/// Reviewed である`・verdict の括弧を持たない）では通らず、括弧の語は `ReviewCheck::as_str` が `Passed` に
/// 返す `PASS` である。一般則そのものの形は末尾で `Implemented` の便から実測して対に置く。
#[test]
fn pipe_retire_reviewed_pass_refused_and_names_the_verdict() {
    let (repo, state) = repo_with_state();
    let id = reviewed_with_worktree(&repo, &state, &review_body("PASS"));
    let live = worktree_of(&repo, &id);
    let before = event_count(&state);

    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "Reviewed(PASS) は rc 1: {}", stdout_of(&out));
    assert_eq!(
        stderr_of(&out).trim(),
        format!("pipe: run {id} の段は Reviewed である（verdict=PASS）"),
        "断りは判定の語まで名乗る"
    );
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert!(
        !repo.join(".worktrees").join("scribe2").join("retired").join(&id).exists(),
        "retired/<run> を作らない"
    );
    assert_eq!(event_count(&state), before, "前提違反は event を 1 件も書かない");
    // 判定だけを終端の側へ解くと同じ便が通る＝上の rc 1 は **verdict** を理由にしている（段ではない）。
    write_review_verdict(&state, &id, &review_body("FAIL"));
    let folded = retire_once(&repo, &state, &id);
    assert_eq!(folded.status.code(), Some(i32::from(RC_OK)), "FAIL なら通る: {}", stderr_of(&folded));
    clean(&[&repo, &state]);

    // 対の実測: 段の列に無い段（`Implemented`）の断りは **一般則のまま**で、verdict の括弧を持たない。
    let (other, other_state) = repo_with_state();
    let other_path = write_contract(&other, &[], &[]);
    let waiting = implemented(&other, &other_state, &other_path);
    let general = retire_once(&other, &other_state, &waiting);
    assert_eq!(general.status.code(), Some(i32::from(RC_REFUSED)), "Implemented は rc 1: {}", stdout_of(&general));
    assert_eq!(
        stderr_of(&general).trim(),
        format!("pipe: run {waiting} の段は Implemented である"),
        "一般則は段だけを名乗る"
    );
    clean(&[&other, &other_state]);
}

/// 断りの照合に使う stderr（終端の周が写す `notify=` の行を除いた本文・設計 dispatcher.md §29 形 4）。
fn refusal_of(out: &Output) -> String {
    stderr_of(out).lines().filter(|line| !line.starts_with("notify=")).collect::<Vec<_>>().join("\n").trim().to_owned()
}

// flip-check: retroactive s2-07l.732
/// 負例 (2): 判定を**読めない**便も畳まない（fail-closed・読めない判定を終端に読み替えない）。母集団は
/// 「JSON でない本文」と「3 値の外」の 2 つで、どちらも同じ 1 行（括弧の語は `Unreadable` の `読めない`）。
#[test]
fn pipe_retire_reviewed_unreadable_refused_and_names_the_verdict() {
    let (repo, state) = repo_with_state();
    let id = reviewed_with_worktree(&repo, &state, "not json\n");
    let live = worktree_of(&repo, &id);
    let before = event_count(&state);
    let reason = format!("pipe: run {id} の段は Reviewed である（verdict=読めない）");

    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "読めない判定は rc 1: {}", stdout_of(&out));
    assert_eq!(refusal_of(&out), reason, "断りは「読めない」を名乗る");
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert!(
        !repo.join(".worktrees").join("scribe2").join("retired").join(&id).exists(),
        "retired/<run> を作らない"
    );
    assert_eq!(event_count(&state), before, "前提違反は event を 1 件も書かない");
    // 3 値の外も同じ断り（PASS でない字面を「終端」に読み替えない）。
    write_review_verdict(&state, &id, &review_body("MAYBE"));
    let outside = retire_once(&repo, &state, &id);
    assert_eq!(outside.status.code(), Some(i32::from(RC_REFUSED)), "3 値の外も rc 1: {}", stdout_of(&outside));
    assert_eq!(refusal_of(&outside), reason, "3 値の外も「読めない」");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    // 判定を読める終端へ直すと同じ便が通る＝上の rc 1 は**判定の読めなさ**を理由にしている（段ではない）。
    write_review_verdict(&state, &id, &review_body("INCONCLUSIVE"));
    let folded = retire_once(&repo, &state, &id);
    assert_eq!(folded.status.code(), Some(i32::from(RC_OK)), "読める終端なら通る: {}", stderr_of(&folded));
    clean(&[&repo, &state]);
}

/// (a) 上限 3 で先頭を land すると 3 本が**列の順に**着地する: 親の連鎖（base → a → b → c）・main の先端は c・`Landed`
/// 3 件・面 5 は 3 行で後続 2 本が `order=train`・後続の追随は 0 回。先頭の `verify.jsonl` に共通 verify が `train=3` で
/// 1 組、後続の `verify.jsonl` に足された record は契約 verify だけ（検出線は写しに無い）。着地済みの便の land は
/// rc 0 で main も event も動かさない。base（`train=` の無い経路）は a だけが載り b / c は Gated のまま＝RED。
#[test]
fn pipe_train_three_runs_land_in_queue_order_with_one_candidate_check() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let [id_a, id_b, id_c] = train_runs(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let before: Vec<usize> = [&id_a, &id_b, &id_c].iter().map(|id| verify_rows(&state, id).len()).collect();
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "列の land は rc 0: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={id_a} train=3")), "stdout に train=3: {}", stdout_of(&out));
    let main = assert_train_chain(&repo, &state, &base, [&id_a, &id_b, &id_c]);
    assert_train_records(&state, [&id_a, &id_b, &id_c], &before);
    let events = event_count(&state);
    let again = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "着地済みの便の land は rc 0: {}", stderr_of(&again));
    assert!(stdout_of(&again).contains("already-landed"), "already-landed を名乗る: {}", stdout_of(&again));
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main, "main は不変");
    assert_eq!(event_count(&state), events, "event は増えない");
    clean(&[&repo, &state]);
}

/// (a) 番待ちで待っている 2 本目の land（子 process・`pipe.land_wait_s` の窓）と並行に先頭が列で着地すると、2 本目は
/// 起きた後に rc 0 の `already-landed` で終端し event を 1 件も足さない（追随へ進まない）。
#[test]
fn pipe_train_waiting_second_run_wakes_already_landed() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let [id_a, id_b, _id_c] = train_runs(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN]);
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let mut waiting = land_in_background(&repo, &state, &id_b, &rules, &lens);
    std::thread::sleep(Duration::from_secs(2));
    assert!(waiting.try_wait().expect("子の状態を読める").is_none(), "2 本目は列の前が空くまで待っている");
    let first = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "先頭の land: {} / {}", stdout_of(&first), stderr_of(&first));
    assert!(stdout_of(&first).contains("train=3"), "列で着地した: {}", stdout_of(&first));
    let events = event_count(&state);
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    let out = waiting.wait_with_output().expect("待っていた land が終わる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "起きた 2 本目は rc 0: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains("already-landed"), "already-landed で終端: {}", stdout_of(&out));
    assert_eq!(event_count(&state), events, "event は増えない");
    assert_eq!(follow_count(&state, &id_b), 0, "追随しない: {:?}", stages(&state, &id_b));
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main, "main は動かない");
    clean(&[&repo, &state]);
}

/// (b) 3 本目の契約 verify が候補の木で赤なら列を解き、先頭だけが既存の経路で着地する。後続 2 本は Gated PASS の
/// まま列に残り、stdout に `dissolved`。
#[test]
fn pipe_train_red_contract_dissolves_and_only_the_front_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let [id_a, id_b, id_c] = train_runs(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, TRAIN_RED]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "先頭は着地する: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={id_a} train=3 dissolved")), "列を解いた: {}", stdout_of(&out));
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(landed_sha_of(&state, &id_a), main, "先頭だけが載る");
    assert_eq!(git(&repo, &["rev-list", "--count", &format!("{base}..{main}")]), "1", "main は 1 本だけ進む");
    for id in [&id_b, &id_c] {
        assert!(show_line(&repo, &state, id).contains("stage=Gated"), "後続は Gated のまま: {id}");
        assert_eq!(value_of(&verdict_pairs(&state, id), "verdict"), "PASS", "後続の判定は PASS のまま: {id}");
        assert!(worktree_of(&repo, id).is_dir(), "後続の worktree は列に残る: {id}");
        assert_eq!(landed_count(&state, id), 0, "後続は着地しない: {id}");
    }
    assert_eq!(verdict_lines(&state), 1, "面 5 は先頭の 1 行");
    clean(&[&repo, &state]);
}

/// (c) 2 本目が先頭と衝突する周（gate の後に 2 本目の branch が先頭と同じ file を足した形）は 2 本目を候補から外して
/// 1 本目と 3 本目が着地する。2 本目の worktree は clean で HEAD も動かず、event は 1 件も増えない。
#[test]
fn pipe_train_conflicting_second_is_left_out_and_the_rest_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let [id_a, id_b, id_c] = train_runs(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN]);
    let second = worktree_of(&repo, &id_b);
    fs::create_dir_all(second.join("crates").join("toy")).expect("衝突の dir を作れる");
    fs::write(second.join("crates").join("toy").join("a.rs"), "conflict\n").expect("衝突の file を書ける");
    git(&second, &["add", "-A"]);
    git(&second, &["commit", "-q", "-m", "conflict"]);
    let head_before = git(&second, &["rev-parse", "HEAD"]);
    let trail_before = trail(&state, &id_b).len();
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "列の land は rc 0: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={id_a} train=2")), "積んだのは 2 本: {}", stdout_of(&out));
    let (sha_a, sha_c) = (landed_sha_of(&state, &id_a), landed_sha_of(&state, &id_c));
    assert_eq!(git(&repo, &["rev-parse", &format!("{sha_c}^")]), sha_a, "c は a の上に詰めて載る");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), sha_c, "main の先端は c");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Gated"), "2 本目は Gated のまま");
    assert_eq!(trail(&state, &id_b).len(), trail_before, "2 本目の event は増えない");
    assert_eq!(git(&second, &["rev-parse", "HEAD"]), head_before, "2 本目の HEAD は動かない");
    assert!(git(&second, &["status", "--porcelain"]).is_empty(), "2 本目の worktree は clean");
    assert_eq!(exported_order(&state, &id_c), "train", "3 本目は train");
    clean(&[&repo, &state]);
}

/// (d) 上限 1 と行の不在は先頭だけが着地し（`train=` を出さない）、後続は自分の land で従来どおり追随 1 回。
#[test]
fn pipe_train_limit_one_and_absent_row_land_only_the_front() {
    for (name, limit) in [("limit-1", Some(1)), ("absent", None)] {
        let (repo, state) = repo_with_state();
        let marker = state.join("lens-ran");
        let [id_a, id_b, _id_c] = train_runs(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN]);
        let rules = write_rules_train(&state, "rules-train.toml", limit);
        let first = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
        assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "{name}: 先頭の land: {}", stderr_of(&first));
        assert!(!stdout_of(&first).contains("train="), "{name}: 列を積まない: {}", stdout_of(&first));
        assert!(show_line(&repo, &state, &id_b).contains("stage=Gated"), "{name}: 2 本目は Gated のまま");
        let lens = fake_lens(&marker, &lens_verdict("PASS"));
        let second = land_extra(&repo, &state, &id_b, &["--rules", &rules, "--lens", &lens]);
        assert_eq!(second.status.code(), Some(i32::from(RC_OK)), "{name}: 2 本目の land: {}", stderr_of(&second));
        assert_eq!(follow_count(&state, &id_b), 1, "{name}: 追随は 1 回: {:?}", stages(&state, &id_b));
        assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "{name}: 2 本目は Landed");
        clean(&[&repo, &state]);
    }
}

/// 列の 3 本を常に `conclusion` の偽 CI・偽 remote・偽 bd を宣言した repo で先頭の land から着地させ、(道具, stdout,
/// 列の便 id, 先端の sha) を返す（先端の sha は `sha:` を持つ `Landed` の行から読む＝終端の行を飛ばす）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn land_train_with_terminal(repo: &Path, state: &Path, conclusion: &str) -> (FakeTerminal, String, [String; 3], String) {
    let tools = fake_terminal(repo, state, conclusion);
    let marker = state.join("lens-ran");
    let ids = train_runs(repo, state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN]);
    let rules = write_rules_train(state, "rules-train.toml", Some(3));
    let bd = state.join("fake-bd.sh").display().to_string();
    let out = land_extra(repo, state, &ids[0], &["--bd", &bd, "--rules", &rules]);
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("run={} train=3", ids[0])), "列で着地した: {stdout} / {}", stderr_of(&out));
    let tip_sha = sha_of_landing(state, &ids[2]).expect("先端の便の sha を読める");
    (tools, stdout, ids, tip_sha)
}

/// 便の着地そのものを記した `Landed` の行の `sha:`（終端の行 `terminal:` は `sha:` を持たないので飛ばす）。
fn sha_of_landing(state: &Path, id: &str) -> Option<String> {
    landed_details(state, id)
        .iter()
        .find_map(|detail| detail.split_whitespace().find_map(|word| word.strip_prefix("sha:")).map(str::to_owned))
}

/// 便の `Landed` の後ろに並ぶ終端の行（`terminal:`）。
fn terminal_tail(state: &Path, id: &str) -> Vec<String> {
    landed_details(state, id).into_iter().filter(|detail| detail.starts_with("terminal:")).collect()
}

/// 列の終端（設計 contract-source.md §53・行 be）: 常に success の偽 CI を持つ 3 本の列の着地で、先端でない 2 本（a / b）
/// も自分を祖先に持つ先端の CI で照合し、3 本とも push → CI → close の 3 段を通す。偽 CI の呼び出しは 3 本 × 2 回＝6 回で、
/// 最後に渡った sha は先端の sha（最後に終端する便は先頭の a＝先端でない便も先端の sha で照合した）。base は先端でない
/// 便が CI を照合せず `ci:unmeasurable` で止まる（呼び出し 2 回・close 1 本）＝RED。
#[test]
fn pipe_train_terminal_every_run_checks_the_tip_ci_and_closes() {
    let (repo, state) = repo_with_state();
    let (tools, stdout, [id_a, id_b, id_c], tip_sha) = land_train_with_terminal(&repo, &state, "success");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), tip_sha, "main の先端は c");
    for id in [&id_a, &id_b, &id_c] {
        assert_eq!(
            terminal_tail(&state, id),
            ["terminal:push:fake", "terminal:ci:success", "terminal:close:ok"],
            "列の便は push → CI → close: {id} {:?}",
            landed_details(&state, id)
        );
        let line = stdout.lines().find(|line| line.starts_with(&format!("run={id} landed="))).map(str::to_owned);
        assert!(
            line.as_deref().is_some_and(|found| found.ends_with("terminal=closed")),
            "stdout の terminal= は closed: {id} {stdout}"
        );
    }
    assert_eq!(tools.ci_call_count(), 6, "偽 CI は 3 本 × （待ちの最初の 1 回と読み直しの 1 回）");
    let ci_argv = fs::read_to_string(&tools.ci_log).expect("偽 CI が撃たれた");
    assert_eq!(ci_argv.lines().collect::<Vec<&str>>(), [tip_sha.as_str()], "最後に渡った sha は先端の sha");
    assert_eq!(git(&tools.remote, &["rev-parse", "refs/heads/main"]), tip_sha, "偽 remote の main は先端の sha");
    clean(&[&repo, &state]);
}

/// 先端の CI で照合した close（設計 contract-source.md §53・行 be）: 1 周目は常に success の偽 CI の列の着地で、偽 bd の
/// 最後の close（最後に終端する列の先頭の便 a）の reason が `landed <a の sha> ci=success tip=<先端の sha>`。2 周目は偽 CI が
/// failure を返す列の着地で、3 本とも `ci:failure` で止まり台帳を 1 度も閉じない。base は a が close せず止まる＝RED。
#[test]
fn pipe_train_tip_close_reason_names_the_tip_and_failure_closes_none() {
    let (repo, state) = repo_with_state();
    let (tools, _, [id_a, _, _], tip_sha) = land_train_with_terminal(&repo, &state, "success");
    let sha_a = sha_of_landing(&state, &id_a).expect("先頭の便の sha を読める");
    assert_ne!(sha_a, tip_sha, "前提: 先頭の便は先端でない");
    let argv = fs::read_to_string(&tools.bd_log).expect("偽 bd が撃たれた");
    let words: Vec<&str> = argv.lines().collect();
    assert_eq!(
        words.get(..3),
        Some(["close", "s2-2e5", "--reason"].as_slice()),
        "最後の close は先頭の便の bead（train_runs の a）: {words:?}"
    );
    assert_eq!(
        words.get(3).copied(),
        Some(format!("landed {sha_a} ci=success tip={tip_sha}").as_str()),
        "reason は先端の id を持つ: {words:?}"
    );
    clean(&[&repo, &state]);

    let (repo, state) = repo_with_state();
    let (tools, stdout, ids, _) = land_train_with_terminal(&repo, &state, "failure");
    for id in &ids {
        assert_eq!(
            terminal_tail(&state, id),
            ["terminal:push:fake", "terminal:ci:failure"],
            "failure は close しない: {id} {:?}",
            landed_details(&state, id)
        );
        let line = stdout.lines().find(|line| line.starts_with(&format!("run={id} landed="))).map(str::to_owned);
        assert!(line.as_deref().is_some_and(|found| found.ends_with("terminal=ci:failure")), "stdout: {id} {stdout}");
    }
    assert!(!tools.bd_log.exists(), "台帳は 1 度も閉じない");
    clean(&[&repo, &state]);
}

/// (g) 先端の木の主実測が赤の周は列の便すべてが `Failed detail=main-red` で `Landed` 0 件・main は 3 本ぶん進んだまま
/// （巻き戻さない・既存の極性・どの便が赤かは帰属しない）。
#[test]
fn pipe_train_red_main_fails_every_run_and_keeps_main_advanced() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let [id_a, id_b, id_c] = train_runs(&repo, &state, &marker, [MAIN_RED, ALL_GREEN, ALL_GREEN]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "main が赤い周は rc 1: {} / {}", stdout_of(&out), stderr_of(&out));
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-list", "--count", &format!("{base}..{main}")]), "3", "main は 3 本ぶん進んだまま");
    for id in [&id_a, &id_b, &id_c] {
        assert_eq!(
            stages(&state, id).last().cloned(),
            Some((Some(Stage::Failed), Some("main-red".to_owned()))),
            "列の便はすべて main-red: {id}"
        );
        assert_eq!(landed_count(&state, id), 0, "Landed は 0 件: {id}");
    }
    assert_eq!(verdict_lines(&state), 0, "面 5 へ書かない");
    clean(&[&repo, &state]);
}

// ───── PR で着地した便を照合してから close し、worktree を畳む（設計 contract-source.md §61・接頭辞 `pr_retire_`） ─────
//
// fixture: PR の便（`landed_pr`）・偽 remote（bare repo・宣言の `remote`）・宣言の `ci-cmd` の偽 CI・PATH の先頭の偽 gh・
// `--bd` の偽 client。答えは file で替える。merge の commit は便の branch と main から `commit-tree` で作り、偽 remote の
// main へ path の URL で押す（先端でない fixture はその上に 1 commit 足して押す）。

/// retire に `--fold-only` を足して撃つ（親 `land.rs` の `retire_once` は他の畳みの歯が使うので変えない）。
fn retire_fold_only(repo: &Path, state: &Path, id: &str) -> Output {
    run_pipe(&[
        "retire", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--fold-only",
    ])
}

/// PR の便の照合の道具一式（答えは `dir` の file）。
struct PrTools {
    repo: PathBuf,
    state: PathBuf,
    id: String,
    bead: String,
    /// 答えの file と記録の file を置く dir。
    dir: PathBuf,
    /// 偽 remote（bare repo）。
    remote: PathBuf,
    /// 偽 gh を先頭に積んだ PATH。
    path: String,
    /// 偽の台帳 client。
    bd: String,
    /// merge の commit（偽 remote の main へ押した）。
    merge: String,
}

impl PrTools {
    /// 答えの file を書く。
    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn put(&self, name: &str, body: &str) {
        fs::write(self.dir.join(name), body).expect("答えの file を書ける");
    }

    /// 記録の file の行（撃たれなければ空）。
    fn lines(&self, name: &str) -> Vec<String> {
        fs::read_to_string(self.dir.join(name)).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    /// 偽の台帳の JSON の全文。
    fn ledger(&self) -> String {
        fs::read_to_string(self.dir.join("ledger")).unwrap_or_default()
    }

    /// 畳んだ先。
    fn retired(&self) -> PathBuf {
        self.repo.join(".worktrees").join("scribe2").join("retired").join(&self.id)
    }

    /// 偽 remote の main を `sha` にする（強制）。
    fn push_main(&self, sha: &str) {
        git(&self.repo, &["push", "-q", "-f", &self.remote.display().to_string(), &format!("{sha}:refs/heads/main")]);
    }

    /// `parent` の木に 1 commit を足した commit（main は動かさない）。
    fn commit_on(&self, parent: &str, message: &str) -> String {
        let tree = git(&self.repo, &["rev-parse", &format!("{parent}^{{tree}}")]);
        git(&self.repo, &["commit-tree", &tree, "-p", parent, "-m", message])
    }

    /// retire を撃つ（偽 gh を PATH の先頭に・`--bd` は偽 client）。
    fn retire(&self, extra: &[&str]) -> Output {
        let (repo, state) = (self.repo.display().to_string(), self.state.display().to_string());
        let mut args = vec!["retire", "--run", &self.id, "--repo", &repo, "--state-dir", &state, "--bd", &self.bd];
        args.extend(extra);
        run_pipe_with_path(&self.path, &args)
    }

    /// 撃つと出る stdout の 1 行（通らない周の形）。
    fn refusal_line(&self, word: &str) -> String {
        format!("run={} retire={word}", self.id)
    }
}

/// PR の便・偽 remote・宣言・偽の道具を用意する（`declared` が偽なら宣言に `remote` を書かない）。
/// 答えの初期値は merged・CI success・台帳 open・close rc 0・merge の commit が偽 remote の先端。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn pr_tools(declared: bool) -> PrTools {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let id = landed_pr(&repo, &state, &design, &state.join("lens-ran"));
    let bead = events(&state).into_iter().find(|event| event.run == id).map(|event| event.bead).unwrap_or_default();
    let dir = state.join("pr-tools");
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).expect("道具の dir を作れる");
    let remote = state.join("remote.git");
    git(&state, &["init", "--bare", "-q", &remote.display().to_string()]);
    git(&repo, &["remote", "add", "fake", &remote.display().to_string()]);
    let ci = exec_script(&dir.join("fake-ci.sh"), "printf '%s\\n' \"$@\" >> \"$0.argv\"\ncat \"$0.answer\"\n");
    exec_script(&bin.join("gh"), &format!("echo \"$*\" >> '{0}/gh-calls'\ncat '{0}/gh-answer'\nexit $(cat '{0}/gh-rc')\n", dir.display()));
    let bd = exec_script(
        &dir.join("fake-bd.sh"),
        &format!(
            "d='{0}'\nif [ \"$1\" = close ]; then\n  echo \"$*\" >> \"$d/close-log\"\n  rc=$(cat \"$d/close-rc\")\n  \
             if [ \"$rc\" = 0 ]; then printf '[{{\"id\":\"%s\",\"status\":\"closed\",\"close_reason\":\"%s\"}}]\\n' \"$2\" \"$4\" > \"$d/ledger\"; fi\n  \
             exit \"$rc\"\nfi\ncat \"$d/ledger\"\nexit $(cat \"$d/list-rc\")\n",
            dir.display()
        ),
    );
    let remote_line = if declared { "remote = \"fake\"\n" } else { "" };
    let body = fs::read_to_string(repo.join(".vessel.toml")).expect("宣言を読める");
    fs::write(repo.join(".vessel.toml"), format!("{body}{remote_line}ci-cmd = \"{ci} {{sha}}\"\n")).expect("宣言を書ける");
    git(&repo, &["add", "-f", ".vessel.toml"]);
    git(&repo, &["commit", "-q", "-m", "terminal-decl"]);
    let (main, branch) = (git(&repo, &["rev-parse", "refs/heads/main"]), format!("scribe2/{id}"));
    let tree = git(&repo, &["rev-parse", &format!("{branch}^{{tree}}")]);
    let merge = git(&repo, &["commit-tree", &tree, "-p", &main, "-p", &branch, "-m", "merge"]);
    let path = format!("{}:{}", bin.display(), crate::toolbox_path(&state));
    let tools = PrTools { repo, state, id, bead, dir, remote, path, bd, merge };
    tools.push_main(&tools.merge);
    tools.put("gh-answer", &format!("{{\"state\":\"MERGED\",\"mergeCommit\":{{\"oid\":\"{}\"}}}}\n", tools.merge));
    tools.put("gh-rc", "0\n");
    tools.put("fake-ci.sh.answer", "[{\"status\":\"completed\",\"conclusion\":\"success\"}]\n");
    tools.put("ledger", &format!("[{{\"id\":\"{}\",\"status\":\"open\",\"close_reason\":\"\"}}]\n", tools.bead));
    tools.put("close-rc", "0\n");
    tools.put("list-rc", "0\n");
    tools
}

/// 通った周の後の面を測る（rc 0・stdout・close の理由・畳んだ先・branch・event の 2 件・偽 gh と偽 CI の argv）。
fn assert_closed_then_folded(tools: &PrTools, out: &Output, tip: &str) {
    let (id, merge) = (&tools.id, &tools.merge);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "通った周は rc 0: {} / {}", stdout_of(out), stderr_of(out));
    let retired = tools.retired();
    assert_eq!(stdout_of(out).trim(), format!("run={id} retired={} close=ok", retired.display()), "stdout");
    let tail = if tip == merge { String::new() } else { format!(" tip={tip}") };
    assert_eq!(
        tools.lines("close-log"),
        vec![format!("close {} --reason landed {merge} ci=success{tail}", tools.bead)],
        "close はちょうど 1 回・理由は merge の commit と CI"
    );
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと畳む");
    assert!(!worktree_of(&tools.repo, id).exists(), "元の場所は空く");
    assert!(!git(&tools.repo, &["branch", "--list", &format!("scribe2/{id}")]).is_empty(), "branch は残る");
    let events = trail(&tools.state, id);
    let last_two: Vec<_> = events.iter().rev().take(2).rev().cloned().collect();
    assert_eq!(
        last_two,
        vec![
            (EventKind::RunDone, Some(Stage::Landed), Some("terminal:close:ok".to_owned())),
            (EventKind::RunStage, Some(Stage::Landed), Some("retired".to_owned())),
        ],
        "Landed の後ろは close:ok と retired（段は Landed のまま）"
    );
    assert_eq!(tools.lines("gh-calls"), vec![format!("pr view scribe2/{id} --json state,mergeCommit")], "偽 gh の argv");
    assert_eq!(tools.lines("fake-ci.sh.argv"), vec![tip.to_owned()], "偽 CI は先端の commit id で 1 回");
}

/// (a) merge の commit が先端そのものの周と、先端がその上に在る周: どちらも close 1 回で畳む。先端が違う周だけ理由に `tip=`。
#[test]
fn pr_retire_closes_then_folds_when_the_merge_is_under_a_green_tip() {
    let same = pr_tools(true);
    let out = same.retire(&[]);
    assert_closed_then_folded(&same, &out, &same.merge);
    clean(&[&same.repo, &same.state]);

    let behind = pr_tools(true);
    let tip = behind.commit_on(&behind.merge, "tip");
    behind.push_main(&tip);
    let out = behind.retire(&[]);
    assert_closed_then_folded(&behind, &out, &tip);
    clean(&[&behind.repo, &behind.state]);
}

/// (b) 通らない 9 周: 閉じた 6 語のどれか 1 行・rc 1・worktree は元の場所・event と偽の台帳は不変。
#[test]
fn pr_retire_refuses_with_one_closed_word_and_writes_nothing() {
    type Break = fn(&PrTools);
    let cases: [(&str, bool, Break, &str); 9] = [
        ("dirty", true, write_dirty, "worktree-unready"),
        ("not-merged", true, |t| t.put("gh-answer", "{\"state\":\"OPEN\",\"mergeCommit\":null}\n"), "not-merged"),
        ("not-ancestor", true, |t| {
            let other = t.commit_on(&git(&t.repo, &["rev-parse", "refs/heads/main"]), "other");
            t.push_main(&other);
        }, "not-ancestor"),
        ("ci-failure", true, |t| t.put("fake-ci.sh.answer", "[{\"status\":\"completed\",\"conclusion\":\"failure\"}]\n"), "ci-not-success"),
        ("ci-empty", true, |t| t.put("fake-ci.sh.answer", "[]\n"), "ci-not-success"),
        ("no-remote", false, |_| {}, "unmeasured"),
        ("gh-rc", true, |t| t.put("gh-rc", "1\n"), "unmeasured"),
        ("close-rc", true, |t| t.put("close-rc", "3\n"), "unwritten"),
        ("ledger-rc", true, |t| t.put("list-rc", "1\n"), "unmeasured"),
    ];
    for (name, declared, breakage, word) in cases {
        let tools = pr_tools(declared);
        breakage(&tools);
        let (events_before, ledger_before) = (event_count(&tools.state), tools.ledger());
        let out = tools.retire(&[]);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{name}: rc 1: {} / {}", stdout_of(&out), stderr_of(&out));
        assert_eq!(stdout_of(&out).trim_end(), tools.refusal_line(word), "{name}: stdout は閉じた語の 1 行");
        assert!(worktree_of(&tools.repo, &tools.id).exists(), "{name}: worktree は元の場所");
        assert!(!tools.retired().exists(), "{name}: 畳まない");
        assert_eq!(event_count(&tools.state), events_before, "{name}: event を書かない");
        assert_eq!(tools.ledger(), ledger_before, "{name}: 台帳の JSON は不変");
        if matches!(name, "dirty" | "no-remote") {
            assert!(tools.lines("gh-calls").is_empty(), "{name}: 偽 gh は撃たれない");
        }
        if name == "dirty" {
            // 汚れを拭って success の答えにすると、同じ撃ちが close して畳む。
            fs::remove_file(worktree_of(&tools.repo, &tools.id).join("dirty.txt")).ok();
            let again = tools.retire(&[]);
            assert_closed_then_folded(&tools, &again, &tools.merge);
        }
        clean(&[&tools.repo, &tools.state]);
    }
}

/// worktree を汚す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_dirty(tools: &PrTools) {
    fs::write(worktree_of(&tools.repo, &tools.id).join("dirty.txt"), "x\n").expect("worktree を汚せる");
}

/// (c) `--fold-only` は照合も close もせず畳む。閉じ済みの契約は照合せず畳む（close の後の move が落ちた周の撃ち直し）。
#[test]
fn pr_retire_fold_only_and_closed_contracts_fold_without_checks() {
    fold_only_folds_and_leaves_the_contract_open();
    closed_contract_folds_after_a_failed_move_without_checks();
}

/// (c) の前半: not-merged の fixture に `--fold-only` を付けた撃ち。
fn fold_only_folds_and_leaves_the_contract_open() {
    let open = pr_tools(true);
    open.put("gh-answer", "{\"state\":\"OPEN\",\"mergeCommit\":null}\n");
    let out = open.retire(&["--fold-only"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "--fold-only は not-merged でも畳む: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim(), format!("run={} retired={}", open.id, open.retired().display()), "close=ok を名乗らない");
    assert!(open.retired().exists() && !worktree_of(&open.repo, &open.id).exists(), "畳んだ");
    assert!(open.lines("close-log").is_empty(), "close は撃たない");
    assert!(open.lines("gh-calls").is_empty(), "偽 gh は撃たれない");
    assert_eq!(open.ledger().matches("\"open\"").count(), 1, "契約は開いたまま");
    clean(&[&open.repo, &open.state]);
}

/// (c) の後半: `retired/<id>` を先に塞いだ success の fixture（close の後の move が落ちる）と、退けた撃ち直し。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn closed_contract_folds_after_a_failed_move_without_checks() {
    let closed = pr_tools(true);
    fs::create_dir_all(closed.retired()).expect("畳む先を塞げる");
    let out = closed.retire(&[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "move が落ちた周は rc 1: {}", stdout_of(&out));
    assert_eq!(stdout_of(&out).trim_end(), closed.refusal_line("worktree-unready"), "worktree-unready");
    assert_eq!(closed.lines("close-log").len(), 1, "close は残る");
    assert!(closed.ledger().contains("\"closed\"") && closed.ledger().contains("landed "), "台帳は closed・landed");
    assert!(worktree_of(&closed.repo, &closed.id).exists(), "worktree は元の場所");
    fs::remove_dir(closed.retired()).expect("塞いだ dir を退けられる");
    let (gh_before, ci_before) = (closed.lines("gh-calls").len(), closed.lines("fake-ci.sh.argv").len());
    let again = closed.retire(&[]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "退けた撃ち直しは畳む: {} / {}", stdout_of(&again), stderr_of(&again));
    assert_eq!(stdout_of(&again).trim(), format!("run={} retired={}", closed.id, closed.retired().display()), "畳むだけの形");
    assert_eq!(closed.lines("close-log").len(), 1, "close は増えない");
    assert_eq!(closed.lines("gh-calls").len(), gh_before, "偽 gh は増えない");
    assert_eq!(closed.lines("fake-ci.sh.argv").len(), ci_before, "偽 CI は増えない");
    clean(&[&closed.repo, &closed.state]);
}
