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

    let out = retire_once(&repo, &state, &id);
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
    let again = retire_once(&repo, &state, &id);
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
    let early = retire_once(&repo, &state, &id);
    assert_eq!(early.status.code(), Some(i32::from(RC_REFUSED)), "Landed 以外は rc 1");
    assert!(worktree_of(&repo, &id).exists(), "断った周は worktree を動かさない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    // 段だけを解くと同じ便が通る＝上の rc 1 は**段**を理由にしている。
    land_pr(&repo, &state, &id);
    let landed = retire_once(&repo, &state, &id);
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
    let out = retire_once(&dirty_repo, &dirty_state, &dirty_id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "dirty な worktree は rc 1");
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert!(stray.exists(), "汚れもそのまま残す（掃除しない）");
    assert_eq!(event_count(&dirty_state), dirty_before, "event を 1 件も書かない");
    let dirty_retired = dirty_repo.join(".worktrees").join("scribe2").join("retired").join(&dirty_id);
    assert!(!dirty_retired.exists(), "retired/<id> を作らない");
    // 汚れだけを拭うと同じ便が通る＝上の rc 1 は**clean**を理由にしている。
    fs::remove_file(&stray).expect("汚れを拭える");
    let cleaned = retire_once(&dirty_repo, &dirty_state, &dirty_id);
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
    assert_eq!(stderr_of(&out).trim(), reason, "断りは「読めない」を名乗る");
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
    assert_eq!(stderr_of(&outside).trim(), reason, "3 値の外も「読めない」");
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

/// 列の終端（設計 contract-source.md §52・行 bd）: 常に success の偽 CI を持つ 3 本の列の着地で、先端の便（c）だけが
/// push → CI → close の 3 段を通し、先端でない 2 本（a / b）は push の後に CI を照合せず `ci:unmeasurable` で止まる
/// （close しない）。偽 CI の呼び出しは先端の便の 2 回だけ（base は 3 本 × 2 回＝6 回で 3 本とも close する＝RED）。
#[test]
fn pipe_train_terminal_only_the_tip_checks_ci_and_closes() {
    let (repo, state) = repo_with_state();
    let tools = fake_terminal(&repo, &state, "success");
    let marker = state.join("lens-ran");
    let [id_a, id_b, id_c] = train_runs(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN]);
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let bd = state.join("fake-bd.sh").display().to_string();
    let out = land_extra(&repo, &state, &id_a, &["--bd", &bd, "--rules", &rules]);
    assert!(stdout_of(&out).contains(&format!("run={id_a} train=3")), "列で着地した: {} / {}", stdout_of(&out), stderr_of(&out));
    // 終端の行（`terminal:`）が `Landed` の後ろに並ぶので、`sha:` は着地そのものを記した行から読む。
    let tip_sha = landed_details(&state, &id_c)
        .iter()
        .find_map(|detail| detail.split_whitespace().find_map(|word| word.strip_prefix("sha:")).map(str::to_owned))
        .unwrap_or_default();
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), tip_sha, "main の先端は c");
    let tail = |id: &str| landed_details(&state, id).into_iter().filter(|detail| detail.starts_with("terminal:")).collect::<Vec<String>>();
    assert_eq!(
        tail(&id_c),
        ["terminal:push:fake", "terminal:ci:success", "terminal:close:ok"],
        "先端の便は push → CI → close: {:?}",
        landed_details(&state, &id_c)
    );
    for id in [&id_a, &id_b] {
        assert_eq!(
            tail(id),
            ["terminal:push:fake", "terminal:ci:unmeasurable"],
            "先端でない便は CI を照合せず止まり close しない: {id} {:?}",
            landed_details(&state, id)
        );
    }
    assert_eq!(tools.ci_call_count(), 2, "偽 CI は先端の便の待ちの最初の 1 回と読み直しの 1 回だけ");
    assert_eq!(git(&tools.remote, &["rev-parse", "refs/heads/main"]), tip_sha, "偽 remote の main は先端の sha");
    for (id, token) in [(&id_a, "ci:unmeasurable"), (&id_b, "ci:unmeasurable"), (&id_c, "closed")] {
        let line = stdout_of(&out).lines().find(|line| line.starts_with(&format!("run={id} landed="))).map(str::to_owned);
        assert!(
            line.as_deref().is_some_and(|found| found.ends_with(&format!("terminal={token}"))),
            "stdout の terminal= は {token}: {id} {}",
            stdout_of(&out)
        );
    }
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
