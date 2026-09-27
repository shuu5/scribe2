// flip-check: moved s2-07l.687
//! question と resume の族の歯（接頭辞 `pipe_question_` / `pipe_resume_`・設計 docs/design/carry-prep.md §10 行 o・親 `tests/e2e/pipe/spawn.rs` の helper を `use super::*` で使う）。

use super::*;

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

/// (a) `pipe run` を Implemented の時点で process group ごと SIGKILL → 中断点は event log に在り、別 process の
/// `resume` 2 回（gate → land）で Landed まで通る。人手の event 0・殺す前の結果（runner の commit）は保たれる（C9）。
#[test]
fn pipe_resume_kill_at_implemented_resumes_to_landed_in_new_process() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let (id, _killed) = killed_at_implemented(&repo, &state, &design);

    // 中断の現在地: Implemented が 1 件・終端の記帳は無い・show も Implemented を名乗る。
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).unwrap_or_default();
    assert_eq!(stage_count(&state, &id, Stage::Implemented), 1, "Implemented は 1 件: {log}");
    assert_eq!(kind_count(&state, &id, EventKind::RunDone), 0, "RunDone は 0 件: {log}");
    assert_eq!(log.matches("\"stage\":\"Landed\"").count(), 0, "Landed の字面は 0 件: {log}");
    let shown = show_line(&repo, &state, &id);
    assert!(shown.contains("stage=Implemented"), "中断点は event log に在る: {shown}");

    // 続き: 別 process が置き場だけを読んで gate → land を引く。
    let marker = state.join("lens-ran-after-kill");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "殺した後の Implemented → gate: {}", stderr_of(&gated));
    assert!(stdout_of(&gated).contains("verdict=PASS"), "{}", stdout_of(&gated));
    let landed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "Gated → land: {}", stderr_of(&landed));
    assert!(stdout_of(&landed).contains("landed="), "{}", stdout_of(&landed));
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).unwrap_or_default();
    assert_eq!(done_count(&state, &id, Stage::Landed), 1, "RunDone stage=Landed が 1 件: {log}");
    assert!(!log.contains("\"actor\":\"human\""), "人手なしで継いだ（C9）: {log}");
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(
        git(&repo, &["rev-list", "--count", &format!("{base}..{new}")]),
        "1",
        "殺す前の結果（runner の commit 1 本）が main に保たれている（C9）"
    );
    clean(&[&repo, &state]);
}

/// (b) 中断が**外されなかった lock**（殺された writer が残す形＝所有者の pid の 10 進 1 行）を残した周からの
/// resume。所有者は死んでいる（前提 assert）。rc 0 で Landed まで通り、lock file は残らない。
///
/// 警告の字面はここでは測らない（pipe の funnel は store の警告を捨てる＝`tests/e2e/fleet.rs` の歯が持つ）。
/// 時刻の窓を race で狙わず、殺した周に器が残しうる状態を構成して撃つ。
#[test]
fn pipe_resume_kill_dead_owner_lock_does_not_block_resume() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let (id, killed) = killed_at_implemented(&repo, &state, &design);
    assert!(!proc_alive(killed), "前提: 殺した pid {killed} は生きていない");
    let lock = state.join("fleet").join("events.jsonl.lock");
    fs::write(&lock, format!("{killed}\n")).expect("死んだ所有者の lock を置ける");

    let marker = state.join("lens-ran-after-kill");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(
        gated.status.code(),
        Some(i32::from(RC_OK)),
        "所有者の死んだ lock は resume を止めない: {}",
        stderr_of(&gated)
    );
    assert!(stdout_of(&gated).contains("verdict=PASS"), "{}", stdout_of(&gated));
    assert!(!lock.exists(), "死んだ所有者の lock は外されて残らない");
    let landed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "Gated → land: {}", stderr_of(&landed));
    assert!(stdout_of(&landed).contains("landed="), "{}", stdout_of(&landed));
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    assert!(!lock.exists(), "land の後も lock は残らない");
    clean(&[&repo, &state]);
}

/// (a) runner が死んだ `Spawned` の便に `resume --runner` を撃つと rc 0 で、`SeatStopped detail=runner-dead` を 1 件・
/// `Spawned detail=account:a1,resume:runner-dead` を 1 件記帳し、**同じ worktree**（`worktree=` の path が不変）で
/// 2 回目の runner が起きて `Implemented` に至る。1 回目が書いた未 commit の file は worktree に残る（N1）。
#[test]
fn pipe_resume_kill_at_spawned_respawns_in_same_worktree() {
    let (repo, state) = repo_with_state();
    // 行の commit が main を進めるので、base は**便を起こした後**に読む（契約 (b)）。
    let (id, _runner_pid, runner) = killed_at_spawned(&repo, &state, &[IMPLEMENT.to_owned()]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let before = show_line(&repo, &state, &id);

    let resumed = resume_dead_runner(&repo, &state, &id, &runner);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    assert!(stdout_of(&resumed).contains(&format!("run={id} next=spawn account=a1")), "{}", stdout_of(&resumed));
    assert!(stdout_of(&resumed).contains(&format!("run={id} stage=Implemented")), "{}", stdout_of(&resumed));
    assert_eq!(runner_dead_count(&state, &id), 1, "runner の死亡を 1 件記帳する");
    assert_eq!(
        spawned_details(&state, &id),
        vec![format!("base:{base}"), "account:a1,resume:runner-dead".to_owned()],
        "起こし直しの記帳は理由を runner-dead と名乗る"
    );
    assert_eq!(stub_calls(&state), 2, "runner を 1 回起こし直した");
    let after = show_line(&repo, &state, &id);
    assert!(after.contains("stage=Implemented"), "2 回目の runner で Implemented: {after}");
    let worktree_of_line = |line: &str| line.split("worktree=").nth(1).map(str::to_owned);
    assert_eq!(worktree_of_line(&before), worktree_of_line(&after), "同じ worktree で起き直る: {before} / {after}");
    let worktree = worktree_of(&repo, &id);
    assert!(worktree.join(WIP_FILE).exists(), "1 回目の未 commit の file を消さない（N1）");
    assert_eq!(git(&worktree, &["rev-list", "--count", "refs/heads/main..HEAD"]), "1", "2 回目の commit が同じ worktree に載る");
    assert!(!events(&state).iter().any(|event| event.actor == "human"), "人手なしで継いだ（C9）");
    clean(&[&repo, &state]);
}

/// (b) (a) の 2 回目の stdin: 「途中再開」節が契約の後に在り、理由の行（runner の死亡）と未 commit の file 名の行
/// （`git status --porcelain` の形）が載る。止まった時刻は `SeatStopped detail=runner-dead` の ts。
#[test]
fn pipe_resume_kill_at_spawned_lists_uncommitted_in_prompt() {
    let (repo, state) = repo_with_state();
    let (id, _runner_pid, runner) = killed_at_spawned(&repo, &state, &[IMPLEMENT.to_owned()]);
    let resumed = resume_dead_runner(&repo, &state, &id, &runner);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    // 2 回目の runner の行儀よい `SeatStopped` が後ろに在るので、理由つきの行を名指して読む。
    let stopped_at = events(&state)
        .into_iter()
        .filter(|event| {
            event.run == id && event.kind == EventKind::SeatStopped && event.detail.as_deref() == Some("runner-dead")
        })
        .map(|event| event.ts)
        .next_back()
        .unwrap_or_default();
    let prompt = stub_stdin(&state, 2);
    let (contract_at, resume_at) = (prompt.find("goal = "), prompt.find("## 途中再開"));
    assert!(matches!((contract_at, resume_at), (Some(c), Some(r)) if c < r), "契約 → 途中再開 の順: {prompt}");
    assert!(!prompt.contains("## 回答") && !prompt.contains("## 追随"), "質問も追随も無い便: {prompt}");
    let reason = format!("- 前の turn は {stopped_at} に runner の死亡で止まった（process が消えた）");
    assert!(!stopped_at.is_empty() && prompt.contains(&reason), "理由の行（SeatStopped の ts）: {prompt}");
    assert!(prompt.contains("未 commit の変更（worktree に在る・消さない・続きから commit する）:"), "一覧の見出し: {prompt}");
    assert!(prompt.contains(&format!("\n  - ?? {WIP_FILE}")), "未 commit の file 名の行: {prompt}");
    assert!(prompt.contains("base からの commit（worktree に在る・やり直さない）: なし"), "commit の無い便は なし: {prompt}");
    assert!(!stub_stdin(&state, 1).contains("## 途中再開"), "初回の turn には節が無い");
    clean(&[&repo, &state]);
}

/// (c) `pipe run` の group だけを殺して runner を生かした周: `resume --runner` は rc 1・判定行 `runner=alive pid=<pid>`・
/// event 0 件（母集団 = 撃つ前後の event 数）で、runner を 2 本にしない。その後 `reap_own` で runner を畳む。
#[test]
fn pipe_resume_kill_at_spawned_refuses_while_runner_alive() {
    let (repo, state) = repo_with_state();
    let contract = write_set_contract(&repo, "wip", &["src/lib.rs", &format!("+{WIP_FILE}")]);
    let pid_file = state.join("wip-sleep.pid");
    let runner = turn_runner(&state, &[wip_then_wait_turn(&pid_file), IMPLEMENT.to_owned()]);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let mut child = spawn_run_child(&repo, &state, &contract, &runner, &lens);
    let (id, runner_pid, sleeper) = wait_for_seat_spawned(&state, &mut child, &pid_file);
    kill_group(child.id());
    child.wait().ok();
    assert!(proc_alive(runner_pid), "前提: runner {runner_pid} は生きている");

    let before = event_count(&state);
    let resumed = resume_dead_runner(&repo, &state, &id, &runner);
    let alive = proc_alive(runner_pid);
    let calls = stub_calls(&state);
    let after = event_count(&state);
    // 片付け: 自分の子孫の `sleep` を止める → runner の script が `wait` から戻って畳まれる。
    reap_own(sleeper);
    wait_gone(runner_pid);

    assert_eq!(resumed.status.code(), Some(i32::from(RC_REFUSED)), "生きている runner の便は断る: {}", stdout_of(&resumed));
    assert!(stdout_of(&resumed).contains(&format!("run={id} runner=alive pid={runner_pid}")), "判定行: {}", stdout_of(&resumed));
    assert!(stderr_of(&resumed).contains("起きている"), "理由を名乗る: {}", stderr_of(&resumed));
    assert!(alive, "runner を殺さない");
    assert_eq!(calls, 1, "runner を 2 本にしない");
    assert_eq!(after, before, "event 0 件");
    assert_eq!(runner_dead_count(&state, &id), 0, "生きている便に runner-dead を書かない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_resume_reports_next_gate_on_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 審査が残した lens の写し（`lens.toml`・設計 pipeline.md §26）も外す＝写しも flag も無い世界。
    fs::remove_file(vessel::pipe::run_dir(&state, &id).join("lens.toml")).expect("審査の写しを外せる");
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

#[test]
fn pipe_question_spawn_records_questioned_in_order() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let tail: Vec<_> = trail(&state, &id).into_iter().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();
    assert_eq!(
        tail,
        vec![
            (EventKind::SeatStopped, None, None),
            (EventKind::QuestionRaised, None, Some("verify 行が矛盾する".to_owned())),
            (EventKind::RunStage, Some(Stage::Questioned), Some("about:verify".to_owned())),
        ],
        "SeatStopped → QuestionRaised(逐語) → RunStage(Questioned) の順"
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Questioned"), "永続面に Questioned が残る");
    // 質問で止まった便に Live 席は無い（`pipe stop --all` の母集団に入らない）。
    let stopped = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert!(stdout_of(&stopped).contains("seats=0"), "{}", stdout_of(&stopped));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_rc76_without_record_fails_closed() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    // record が無い / 壊れた JSON / question が空 / 非文字列 / 複数行 / key 無し、の各形。
    for (bead, last_line, reason) in [
        ("s2-none", "not a record", "JSON 行が無い"),
        ("s2-broken", r#"{"question":"verify"#, "読めない"),
        ("s2-empty", r#"{"question":"  "}"#, "無いか空"),
        ("s2-num", r#"{"question":1}"#, "無いか空"),
        ("s2-multi", r#"{"question":"a\nb"}"#, "1 行でない"),
        ("s2-nokey", r#"{"about":"verify"}"#, "無いか空"),
    ] {
        let id = intake_bead(&repo, &state, &path, bead);
        let runner = format!("printf '%s\\n' '{last_line}'; exit 76");
        let out = run_pipe(&[
            "spawn", "--run", &id, "--repo", &repo.display().to_string(),
            "--state-dir", &state.display().to_string(), "--runner", &runner,
        ]);
        assert!(stdout_of(&out).contains("stage=Failed"), "{bead}: {}", stdout_of(&out));
        let last = trail(&state, &id).pop();
        assert!(
            matches!(&last, Some((EventKind::RunStage, Some(Stage::Failed), Some(detail)))
                if detail.starts_with("question-record-missing:") && detail.contains(reason) && detail.ends_with(",commits:0")),
            "{bead}: 理由 question-record-missing:{reason}: {last:?}"
        );
        assert!(
            !trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised),
            "{bead}: 質問は記帳しない"
        );
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_runner_stdout_is_kept_in_run_dir() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // stdout を捕らえても、包みの観測行（rate-limit status の集合を育てる口）は残る。
    let runner = "echo 'runner: rc=0 records=3 observed=allowed_warning'; echo x >> src/lib.rs && git add -A && git commit -q -m runner";
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let kept = fs::read_to_string(state.join("pipe").join(&id).join("runner.stdout.log")).unwrap_or_default();
    assert!(kept.contains("observed=allowed_warning"), "観測行が残る: {kept}");
    assert!(kept.lines().next().is_some_and(|head| head.starts_with("## ") && head.ends_with(" rc=0")), "見出し行: {kept}");
    // stdout を出さない runner では file を作らない。**測り終えた便は `stop --run` で外す**
    // ——`Implemented` は終端でないので、同じ write-set の 2 本目は交差で断られる（`s2-07l.145`）。
    //
    // **包めない host で撃つ**（[`lean_path`]）。封じ込めが効く host では包みが終端行
    // （`confine-usage …`）を出すので stdout は空にならず、この面は host ごとに違う答えを
    // 出してしまう——測っているのは「**runner が**何も言わなかった周」である。
    stop_run_ok(&state, &id);
    let id2 = intake_bead(&repo, &state, &path, "s2-quiet");
    let quiet = run_pipe_with_path(
        &lean_path(&state),
        &["spawn", "--run", &id2, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", "true"],
    );
    assert!(stdout_of(&quiet).contains("stage=Failed"));
    assert!(!state.join("pipe").join(&id2).join("runner.stdout.log").exists(), "空の周は書かない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_rc0_does_not_read_record_line() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // 76 でない rc では最終行を読まない＝record を書いても従来どおり Implemented。
    let runner = format!(
        "echo x >> src/lib.rs && git add -A && git commit -q -m runner; printf '%s\\n' '{QUESTION_RECORD}'; exit 0"
    );
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(stdout_of(&out).contains("stage=Implemented"), "{}", stdout_of(&out));
    assert!(!trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised));
    // record と commit が同時の周は質問ではなく実装の失敗（rc 76 でも Failed）。
    // 測り終えた 1 本目は `stop --run` で外す（入口の排他・`s2-07l.145`）。
    stop_run_ok(&state, &id);
    let path2 = write_contract(&repo, &[], &[]);
    let id2 = intake_bead(&repo, &state, &path2, "s2-both");
    let both = format!("echo y >> src/lib.rs && git add -A && git commit -q -m r; printf '%s\\n' '{QUESTION_RECORD}'; exit 76");
    let out = run_pipe(&[
        "spawn", "--run", &id2, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &both,
    ]);
    assert!(stdout_of(&out).contains("stage=Failed"), "{}", stdout_of(&out));
    assert!(matches!(trail(&state, &id2).pop(), Some((_, Some(Stage::Failed), Some(d))) if d.starts_with("runner-rc:76,commits:1")));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_resume_waits_for_answer_without_writing() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let marker = state.join("runner-ran");
    let before = event_count(&state);
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner_cmd(&marker),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "回答の無い resume は rc 3: {}", stderr_of(&out));
    assert!(!marker.exists(), "runner を起こさない");
    assert_eq!(event_count(&state), before, "1 行も書かない");
    // 空の回答は書かない（rc 1）。
    let empty = run_pipe(&["answer", "--run", &id, "--words", "  ", "--state-dir", &state.display().to_string()]);
    assert_eq!(empty.status.code(), Some(i32::from(RC_REFUSED)), "{}", stderr_of(&empty));
    assert_eq!(event_count(&state), before, "空の回答は 1 byte も書かない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_answer_refuses_run_that_is_not_questioned() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let before = event_count(&state);
    let out = run_pipe(&["answer", "--run", &id, "--words", "verify は 1 行目だけ", "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "Questioned 以外は rc 3: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before, "何も書かない");
    let missing = run_pipe(&["answer", "--run", "nope", "--words", "x", "--state-dir", &state.display().to_string()]);
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "無い run は rc 1");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_answer_then_resume_respawns_with_answer_section() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let answered = run_pipe(&[
        "answer", "--run", &id, "--words", "verify は 1 行目だけを撃つ",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&answered));
    assert!(stdout_of(&answered).contains("answered=true"), "{}", stdout_of(&answered));
    let copied = state.join("got-stdin.txt");
    let runner = format!(
        "cat > '{}' && echo x >> src/lib.rs && git add -A && git commit -q -m runner",
        copied.display()
    );
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "回答の後は同じ便が進む: {}", stderr_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "{}", show_line(&repo, &state, &id));
    let stdin = fs::read_to_string(&copied).unwrap_or_default();
    assert!(stdin.contains("## 回答"), "stdin に回答節: {stdin}");
    assert!(stdin.contains("verify 行が矛盾する") && stdin.contains("verify は 1 行目だけを撃つ"), "質問と回答の逐語: {stdin}");
    assert!(stdin.contains("goal = "), "契約の本文も流す: {stdin}");
    // 同じ run が Spawned を通り直し、base は初回の記録と同じ。
    let stages: Vec<Option<Stage>> = trail(&state, &id).into_iter().map(|(_, stage, _)| stage).collect();
    assert_eq!(stages.iter().filter(|stage| **stage == Some(Stage::Spawned)).count(), 2, "Spawned を 2 回通る");
    let bases: BTreeSet<String> = trail(&state, &id)
        .into_iter()
        .filter_map(|(_, stage, detail)| (stage == Some(Stage::Spawned)).then_some(detail).flatten())
        .collect();
    assert_eq!(bases.len(), 1, "base は 1 つ: {bases:?}");
    // 回答は machine 由来（FR22 不変）。
    let report = report_once(&state);
    assert!(
        stdout_of(&report).contains("human_events=0 human_events_other_than_approval=0"),
        "{}",
        stdout_of(&report)
    );
    assert!(
        trail(&state, &id).iter().any(|(kind, _, detail)| *kind == EventKind::QuestionAnswered && detail.as_deref() == Some("verify は 1 行目だけを撃つ")),
        "回答の逐語が残る"
    );
    clean(&[&repo, &state]);
}

/// (a) 回答 → 行の write-set を 1 本広げて commit → resume: 写しの write-set が広がり、runner の stdin（契約節）にその
/// file が写り、段 `Questioned` の detail=contract:refreshed が 1 件・判定行に `contract=refreshed`。
#[test]
fn pipe_question_refresh_widened_row_rewrites_snapshot_and_records_once() {
    let (repo, state) = repo_with_state();
    let id = answered(&repo, &state);
    let snapshot = vessel::pipe::contract_path(&state, &id);
    let before = fs::read_to_string(&snapshot).unwrap_or_default();
    assert!(!before.contains("src/extra.rs"), "受付時の写しは広げる前: {before}");
    write_contract(&repo, &["write-set"], &[r#"write-set = ["src/lib.rs", "src/extra.rs"]"#]);
    let copied = state.join("got-stdin.txt");
    let out = resume_copying(&repo, &state, &id, &copied);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(stdout_of(&out).contains("contract=refreshed"), "判定行に contract=refreshed: {}", stdout_of(&out));
    let after = fs::read_to_string(&snapshot).unwrap_or_default();
    assert!(after.contains(r#"write-set = ["src/lib.rs", "src/extra.rs"]"#), "写しの write-set が広がる: {after}");
    let stdin = fs::read_to_string(&copied).unwrap_or_default();
    assert!(stdin.contains("src/extra.rs"), "契約節に広げた file: {stdin}");
    assert!(stdin.contains("## 回答") && stdin.contains("write-set を広げた"), "回答節は不変: {stdin}");
    assert_eq!(refreshed_count(&state, &id), 1, "記帳は 1 件: {:?}", trail(&state, &id));
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "{}", show_line(&repo, &state, &id));
    clean(&[&repo, &state]);
}

/// (b) 行を 1 字も変えずに resume した周は写しが byte 不変で記帳 0 件（判定行に `contract=refreshed` を出さない）。
#[test]
fn pipe_question_refresh_unchanged_row_keeps_snapshot_bytes() {
    let (repo, state) = repo_with_state();
    let id = answered(&repo, &state);
    let snapshot = vessel::pipe::contract_path(&state, &id);
    let before = fs::read(&snapshot).unwrap_or_default();
    let out = resume_copying(&repo, &state, &id, &state.join("got-stdin.txt"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(!stdout_of(&out).contains("contract=refreshed"), "{}", stdout_of(&out));
    assert_eq!(fs::read(&snapshot).unwrap_or_default(), before, "写しは byte 不変");
    assert_eq!(refreshed_count(&state, &id), 0, "記帳 0 件");
    clean(&[&repo, &state]);
}

/// (c) 行を doc から消して resume すると rc 1・runner を起こさず・写し byte 不変・event 0 件・断りが行を名乗る。
#[test]
fn pipe_question_refresh_missing_row_refuses_without_touching() {
    let (repo, state) = repo_with_state();
    let id = answered(&repo, &state);
    let snapshot = vessel::pipe::contract_path(&state, &id);
    let before = fs::read(&snapshot).unwrap_or_default();
    commit_rows(&repo, &[row_fields("b", &[], &[])]);
    let events_before = events_bytes(&state);
    let marker = state.join("runner-ran");
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner_cmd(&marker),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{}", stderr_of(&out));
    assert!(stderr_of(&out).contains("行 id a"), "断りが行を名乗る: {}", stderr_of(&out));
    assert!(!marker.exists(), "runner を起こさない");
    assert_eq!(fs::read(&snapshot).unwrap_or_default(), before, "写しは byte 不変");
    assert_eq!(events_bytes(&state), events_before, "event 0 件");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_run_stops_with_question_token() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let rules = ceiling_rules(&state);
    let out = run_pipe(&[
        "run", "--design", &path, "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules, "--runner", &question_runner(), "--lens", &review_lens_pass(&state),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "{}", stderr_of(&out));
    let id = run_id_of(&out);
    assert!(stdout_of(&out).contains(&format!("question={id}")), "判定行に question=: {}", stdout_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Questioned"));
    clean(&[&repo, &state]);
}
