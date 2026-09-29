// flip-check: moved s2-07l.684
//! follow の族の歯（接頭辞 `pipe_follow_`・設計 docs/design/carry-prep.md §10 行 l・親 `tests/e2e/pipe/land.rs` の helper を `use super::*` で使う）。

use super::*;

/// 衝突は**便を終端にしない**（設計 pipeline-conflict.md §3 手順 1〜2）。木を戻して
/// `RunStage Implemented detail=rebase-conflict:<base>..<main>` を **1 件**記帳し、`Failed` は
/// 1 件も書かない。main は 1 byte も動かず、worktree は rebase の途中でなく clean である。
#[test]
fn pipe_follow_records_the_conflict_without_failing_the_run() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_INCONCLUSIVE)),
        "起こし直した周は rc 3（次の段を名乗って止まる）: {}",
        stderr_of(&out)
    );
    assert!(stdout_of(&out).contains(&format!("run={id} next=gate")), "次に撃つ段: {}", stdout_of(&out));
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は 1 件: {:?}", stages(&state, &id));
    assert!(
        stages(&state, &id).iter().any(|(stage, detail)| *stage == Some(Stage::Implemented)
            && detail.as_deref() == Some(format!("rebase-conflict:{base}..{moved}").as_str())),
        "detail は base と main を名乗る: {:?}",
        stages(&state, &id)
    );
    assert!(
        !stages(&state, &id).iter().any(|(stage, _)| *stage == Some(Stage::Failed)),
        "便を終端にしない: {:?}",
        stages(&state, &id)
    );
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は 1 byte も動かない");
    assert!(!mid_rebase(&repo, &id), "木は rebase の途中でない");
    assert!(git(&worktree_of(&repo, &id), &["status", "--porcelain"]).is_empty(), "木は clean");
    assert_eq!(stub_calls(&state), 2, "実装役を 1 回起こし直した");
    clean(&[&repo, &state]);
}

/// 起こし直しの turn の stdin は、契約の写しの**後ろ**に「## 追随」節を持ち、main の sha を
/// 名指す（設計 §3 手順 4）。turn 1 の stdin には節が無い（不在が既定）。
#[test]
fn pipe_follow_second_turn_receives_the_follow_section() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    let first = stub_stdin(&state, 1);
    assert!(first.contains("goal = "), "turn 1 も契約の本文を受ける: {first}");
    assert!(!first.contains("## 追随"), "追随の無い turn には節が付かない: {first}");
    let second = stub_stdin(&state, 2);
    assert!(second.contains("## 追随"), "起こし直しの turn に節が付く: {second}");
    assert!(second.contains(&moved), "節は main の sha を名指す: {second}");
    let contract_at = second.find("goal = ");
    let follow_at = second.find("## 追随");
    assert!(
        matches!((contract_at, follow_at), (Some(c), Some(f)) if c < f),
        "順序は 契約 → 追随: {second}"
    );
    clean(&[&repo, &state]);
}

/// 回答済みの質問を持つ便の起こし直しは、stdin に「回答」と「追随」を**この順**で持つ
/// （設計 §3 手順 4 の「契約 → 回答 → 追随」）。
#[test]
fn pipe_follow_answered_question_comes_before_the_follow_section() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let id = questioned(&repo, &state);
    let answered = run_pipe(&[
        "answer", "--run", &id, "--words", "verify は 1 行目だけを撃つ",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&answered));
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "回答の後の turn: {}", stderr_of(&resumed));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate: {}", stderr_of(&gated));
    move_main_into_conflict(&repo);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    let second = stub_stdin(&state, 2);
    let answer_at = second.find("## 回答");
    let follow_at = second.find("## 追随");
    assert!(
        matches!((answer_at, follow_at), (Some(a), Some(f)) if a < f),
        "順序は 回答 → 追随: {second}"
    );
    assert!(second.contains("verify は 1 行目だけを撃つ"), "回答の逐語も運ぶ: {second}");
    clean(&[&repo, &state]);
}

/// 実装役が衝突を解いた周: 器が**実測した merge-base**で base を進め（`rebase:<old>..<new>`）、
/// 続きの gate が新しい base で PASS（先着便の file が write-set の外に載らない）→ land で
/// `Landed`。起こし直しは 1 回だけ（3 turn 目は起こされない）。
#[test]
fn pipe_follow_resolved_conflict_advances_the_base_and_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "起こし直した周は rc 3: {}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains(&format!("run={id} rebase={base}..{moved}")),
        "新しい base を名乗る: {}",
        stdout_of(&out)
    );
    assert!(
        stages(&state, &id).iter().any(|(stage, detail)| *stage == Some(Stage::Implemented)
            && detail.as_deref() == Some(format!("rebase:{base}..{moved}").as_str())),
        "器が base を進めた記帳: {:?}",
        stages(&state, &id)
    );
    assert!(!mid_rebase(&repo, &id), "木は rebase の途中でない");
    // 続きは gate から（新しい base の 2 点 diff は write-set の中だけ）。
    fs::remove_file(&marker).ok();
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "新しい base の gate は PASS: {}", stderr_of(&gated));
    let landed = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(
        git(&repo, &["show", &format!("{new}:src/lib.rs")]),
        "// seed\ny\nx",
        "先着便の行と便の行が両方 main に載る"
    );
    let landings = trail(&state, &id)
        .into_iter()
        .filter(|(kind, stage, _)| *kind == EventKind::RunDone && *stage == Some(Stage::Landed))
        .count();
    // 着地の 1 件と、remote を持たない toy の終端が台帳を閉じた `terminal:close:ok` の 1 件（着地の後ろ）。
    assert_eq!(landings, 2, "Landed は 2 件（着地 + 終端の close）");
    assert_eq!(stub_calls(&state), 2, "起こし直しは 1 回だけ");
    clean(&[&repo, &state]);
}

/// turn の間に main がさらに進んだ周でも、**書く値は実測した merge-base**であって現在の main
/// ではない（2 点 diff に main の新しい commit の逆向きを載せない・設計 §3 手順 5）。
#[test]
fn pipe_follow_records_the_measured_merge_base_not_the_moving_main() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let racing = format!("{RESOLVE}\ngit -C '{}' commit -q --allow-empty -m racing", repo.display());
    let runner = stub_runner(&state, &racing);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    let raced = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(raced, moved, "turn の中で main がさらに進んでいる");
    let details: Vec<String> = stages(&state, &id)
        .into_iter()
        .filter_map(|(_, detail)| detail)
        .filter(|detail| detail.starts_with("rebase:"))
        .collect();
    assert_eq!(
        details,
        vec![format!("rebase:{base}..{moved}")],
        "書く値は merge-base（現在の main {raced} でない）"
    );
    clean(&[&repo, &state]);
}

/// 上限の歯（値 1 ＝最大 1 回起こし直す）: 2 回目の衝突で `Failed detail=rebase-conflict` になり、
/// runner は 3 turn 目に起こされない。main は動かない。
#[test]
fn pipe_follow_stops_retrying_at_the_limit() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, moved) = exhausted_run(&repo, &state, &marker);
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-conflict".to_owned()))),
        "終端の理由: {:?}",
        stages(&state, &id)
    );
    assert_eq!(conflict_count(&state, &id), 2, "衝突は 2 件記帳された: {:?}", stages(&state, &id));
    assert_eq!(stub_calls(&state), 2, "3 turn 目は起こされない");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "段は Failed");
    clean(&[&repo, &state]);
}

/// 上限に達して終端した便（`Failed detail=rebase-conflict`）は `pipe retire` で畳める
/// （move・元 dir 不在・retired/ に在る・**残す event の段は終端のまま**・設計 §5）。
#[test]
fn pipe_follow_retire_folds_an_exhausted_run_and_keeps_the_stage() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, moved) = exhausted_run(&repo, &state, &marker);
    let live = worktree_of(&repo, &id);
    assert!(live.exists(), "終端した便の worktree は残る（retire の入口の前提）");
    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rebase-conflict の便も畳める: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    assert!(!live.exists(), "元の場所が空く");
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("retired".to_owned()))),
        "残す event の段は終端のまま: {:?}",
        stages(&state, &id)
    );
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は 1 byte も動かない");
    clean(&[&repo, &state]);
}

/// `Gated` で verdict が FAIL の便（判定に届いた終端・`.132` の memo）も畳める。残す event の
/// 段は `Gated` のままで、`Landed` へ動かさない。
#[test]
fn pipe_follow_retire_folds_a_gated_fail_run_and_keeps_the_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = implemented(&repo, &state, &path);
    let lens = fake_lens(&marker, &lens_verdict("FAIL"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の gate は rc 1: {}", stderr_of(&gated));
    let live = worktree_of(&repo, &id);
    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "Gated(FAIL) の便も畳める: {}", stderr_of(&out));
    assert!(!live.exists(), "元の場所が空く");
    assert!(
        repo.join(".worktrees").join("scribe2").join("retired").join(&id).exists(),
        "retired/ に在る"
    );
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Gated), Some("retired".to_owned()))),
        "段は Gated のまま: {:?}",
        stages(&state, &id)
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "畳んだ後も段は Gated");
    clean(&[&repo, &state]);
}

/// `resume` は「最後の段が `Implemented` ∧ 最後の detail が `rebase-conflict:` ∧ runner が
/// 起きていない」周に**同じ起こし直し**を撃つ。`--runner` の無い周は rc 1 で events.jsonl が
/// byte 不変（衝突の記帳は land の時点で済んでいる）。detail が `rebase-conflict:` でない
/// `Implemented` が従来どおり gate へ行くことは
/// `pipe_resume_continues_from_implemented_in_new_process` が測る。
#[test]
fn pipe_follow_resume_needs_a_runner_and_continues_the_retry() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    // `--runner` を渡さない land は衝突を記帳して断る（起こし直せない）。
    let bare_land = land_extra(&repo, &state, &id, &[]);
    assert_eq!(bare_land.status.code(), Some(i32::from(RC_REFUSED)), "起こし直せない land は rc 1");
    assert!(stderr_of(&bare_land).contains("--runner が要る"), "理由: {}", stderr_of(&bare_land));
    assert_eq!(stub_calls(&state), 1, "起こし直していない");
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は残る");
    // `--runner` の無い resume は 1 byte も書かない。
    let before = events_bytes(&state);
    let bare = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(bare.status.code(), Some(i32::from(RC_REFUSED)), "--runner 無しの resume は rc 1");
    assert_eq!(events_bytes(&state), before, "events.jsonl は byte 不変");
    assert_eq!(stub_calls(&state), 1, "runner を起こさない");
    // `--runner` 付きの resume は land の衝突と同じ turn を撃つ。
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "resume の起こし直し: {}", stderr_of(&out));
    assert_eq!(stub_calls(&state), 2, "resume が実装役を起こした");
    assert!(
        stdout_of(&out).contains(&format!("run={id} rebase={base}..{moved}")),
        "resume の turn も base を進める: {}",
        stdout_of(&out)
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段は Implemented（次は gate）");
    clean(&[&repo, &state]);
}

/// 追随を解けずに**木を戻して質問 record で止まった** turn は `Questioned` である
/// （ADR-0019 §2.6: 判定は turn 開始時の tip 基準＝便が base から持つ commit を数えない）。
#[test]
fn pipe_follow_question_after_abort_stops_at_questioned() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, ABORT_AND_ASK);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "質問は rc 3: {}", stderr_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Questioned"), "段は Questioned");
    let worktree = worktree_of(&repo, &id);
    assert!(git(&worktree, &["status", "--porcelain"]).is_empty(), "木は clean");
    assert!(!mid_rebase(&repo, &id), "木は rebase の途中でない");
    assert_eq!(
        git(&worktree, &["rev-list", "--count", &format!("{base}..HEAD")]),
        "1",
        "便が base から持つ commit は在る（turn で増えていないだけ）"
    );
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(
        trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised),
        "質問の逐語が残る"
    );
    clean(&[&repo, &state]);
}

/// 負例: 起こし直しの turn で **commit を作ってから**質問 record を出した周は質問ではなく
/// 実装の失敗である（`Failed detail=runner-rc:76,commits:1`）。
#[test]
fn pipe_follow_commit_before_question_is_a_failure() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, COMMIT_THEN_ASK);
    let (id, _base, _moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "land はしない: {}", stdout_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("runner-rc:76,commits:1".to_owned()))),
        "turn で作った commit を数える: {:?}",
        stages(&state, &id)
    );
    assert!(
        !trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised),
        "質問は記帳しない"
    );
    clean(&[&repo, &state]);
}

/// (a) 追随の rebase で main の commit を HEAD に載せ、**自分の commit は作らずに**質問 record で止まった
/// turn は `Questioned` である（main の commit は便の commit に数えない）。
#[test]
fn pipe_follow_main_absorbed_then_question_stops_at_questioned() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, &format!("{ABSORB_MAIN}\n{ASK}"));
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "質問は rc 3: {} {:?}", stderr_of(&out), stages(&state, &id));
    assert!(show_line(&repo, &state, &id).contains("stage=Questioned"), "段は Questioned: {:?}", stages(&state, &id));
    assert!(
        trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised),
        "質問の逐語が残る"
    );
    assert_head_is_main(&repo, &id, &moved);
    clean(&[&repo, &state]);
}

/// (b) 負例: 同じ木で**自分の commit を 1 本作ってから**質問 record を出した turn は従来どおり実装の失敗で、
/// detail は `runner-rc:76,commits:1`（除外は main の commit だけ＝質問を無条件に通さない）。
#[test]
fn pipe_follow_main_own_commit_then_question_is_a_failure() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let body = format!("{ABSORB_MAIN}\nprintf 'z\\n' >> src/lib.rs\ngit add -A\ngit commit -q -m extra\n{ASK}");
    let runner = stub_runner(&state, &body);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "land はしない: {}", stdout_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("runner-rc:76,commits:1".to_owned()))),
        "turn で作った commit だけを数える: {:?}",
        stages(&state, &id)
    );
    assert!(
        !trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised),
        "質問は記帳しない"
    );
    assert_eq!(
        git(&worktree_of(&repo, &id), &["rev-parse", "HEAD~1"]),
        moved,
        "自分の commit は main の真上に 1 本"
    );
    clean(&[&repo, &state]);
}

/// (c) 追随で main を取り込んだだけで rc 0 で終わった turn は `Failed detail=runner-rc:0,commits:0`
/// （完了の判定も base から数え、同じ除外を受ける）。
#[test]
fn pipe_follow_main_absorbed_only_with_rc_zero_is_not_implemented() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, &format!("{ABSORB_MAIN}\nexit 0"));
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "land はしない: {}", stdout_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("runner-rc:0,commits:0".to_owned()))),
        "main の commit は完了の数にも入らない: {:?}",
        stages(&state, &id)
    );
    assert_head_is_main(&repo, &id, &moved);
    clean(&[&repo, &state]);
}

/// (d) main を**読めない**周は除外なしの従来の数え方に落ちる: (a) と同じ木でも、数え手の main の読みだけを
/// 落とす偽 git の下では `Failed detail=runner-rc:76,commits:1`（読めなさを 0 に倒して質問へ通さない）。
#[test]
fn pipe_follow_main_unreadable_main_counts_without_exclusion() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, &format!("{ABSORB_MAIN}\n{ASK}"));
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let path = shim_path(&state, "main-bin", "case \"$*\" in *'rev-parse --verify -q refs/heads/main'*) exit 1;; esac");
    let out = bin_cmd()
        .args(["pipe", "land", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--runner", &runner])
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "land はしない: {}", stdout_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("runner-rc:76,commits:1".to_owned()))),
        "除外なしで数える: {:?}",
        stages(&state, &id)
    );
    assert_head_is_main(&repo, &id, &moved);
    clean(&[&repo, &state]);
}

/// runner が **rebase の途中で** turn を終えた周は `Failed detail=rebase-dirty` で終端する
/// （clean 前提を守る・fail-closed・設計 §3 手順 6）。
#[test]
fn pipe_follow_mid_rebase_turn_fails_dirty() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, LEAVE_MID_REBASE);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "rebase の途中は rc 1: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-dirty".to_owned()))),
        "終端の理由: {:?}",
        stages(&state, &id)
    );
    assert!(mid_rebase(&repo, &id), "木は rebase の途中のまま（器は触らない）");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    clean(&[&repo, &state]);
}

/// 回数は **replay の導出値**である（別の状態 file を持たない・C3）。衝突の `RunStage` を手で
/// 2 件積んだ便は、上限 2（埋め込みの値）の下で次の衝突が終端になり、置き場に新しい file は
/// 1 つも増えない。
#[test]
fn pipe_follow_counts_the_retries_from_the_event_log() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    for _ in 0..2 {
        record_conflict(&state, &id, &format!("{base}..{moved}"));
    }
    // 手で積んだ段は `Implemented` なので、land の前に gate を撃ち直す。
    fs::remove_file(&marker).ok();
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "撃ち直しの gate: {}", stderr_of(&gated));
    let before = dir_names(&state.join("pipe").join(&id));
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "上限に達した周は rc 1: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-conflict".to_owned()))),
        "終端の理由: {:?}",
        stages(&state, &id)
    );
    assert_eq!(stub_calls(&state), 1, "起こし直さない（回数は log が持つ）");
    assert_eq!(dir_names(&state.join("pipe").join(&id)), before, "置き場に新しい file を作らない");
    clean(&[&repo, &state]);
}

/// 回数を**読めない**周は起こし直さず rc 2 で止まる（上限到達の rc 1 と分ける・NFR4）。読めなさは、rebase の
/// 呼出しに合わせて event log へ壊れた行を混ぜる偽 git で作る。
///
/// 段を進める記帳は記帳の門（設計 pipeline.md §39・行 ag）を通り、門は読めない log に書かない（fail-closed）ので、
/// `Failed detail=follow-unmeasured` は書かれず、最後の行は壊れた行のままである。
#[test]
fn pipe_follow_unreadable_retry_count_fails_closed_with_rc_two() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let events = state.join("fleet").join("events.jsonl");
    let path = shim_path(
        &state,
        "poison-bin",
        &format!("case \"$*\" in *' rebase '*) printf 'not-json\\n' >> '{}' ;; esac", events.display()),
    );
    let out = bin_cmd()
        .args(["pipe", "land", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--runner", &runner])
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない回数は rc 2: {}", stderr_of(&out));
    assert_eq!(stub_calls(&state), 1, "起こし直さない");
    let log = fs::read_to_string(&events).expect("event log");
    let last = log.lines().rfind(|line| !line.is_empty()).unwrap_or_default();
    assert_eq!(last, "not-json", "読めない log に段を書かない（Failed も上限到達も書かれない）: {log}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    clean(&[&repo, &state]);
}

/// 起こし直しの spawn も **Budget を要る口だけ**を通る（`pipe_spawn_measures_repo_before_launching`
/// と同型の観測）。repo の HEAD を読めない git を前に置くと、起こし直しは Precheck の段で断られ、
/// runner は起こされない。
#[test]
fn pipe_follow_retry_measures_the_repo_before_launching() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE);
    let (id, _base, _moved) = conflicting_run(&repo, &state, &marker, &runner);
    let failing = format!("-C {} rev-parse HEAD", repo.display());
    let path = shim_path(&state, "measure-bin", &format!("case \"$*\" in *'{failing}'*) exit 1;; esac"));
    let out = bin_cmd()
        .args(["pipe", "land", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--runner", &runner])
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "測れない repo は rc 1: {}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("git repo でない"),
        "測る段で断る（起動関数へ入る前）: {}",
        stderr_of(&out)
    );
    assert_eq!(stub_calls(&state), 1, "起こし直しの runner は起きない");
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は残る（resume で続けられる）");
    clean(&[&repo, &state]);
}

/// **追随節の無い turn**（spawn 時 main = base）で runner が自ら `git rebase` を撃った周も、
/// 器は turn の終了後に merge-base を実測して base を進める（設計 §3 手順 5・節の有無で経路を
/// 分けない）。記帳は `rebase:<base>..<new main>` の 1 件・`base_of_run` は new main・続きの
/// gate は PASS（main 側の `other.txt` を write-set の外れと数えない）。
#[test]
fn pipe_follow_self_rebase_advances_the_base_without_a_follow_section() {
    let (repo, state) = repo_with_state();
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let runner = stub_runner_turns(&state, &self_rebase_body(&repo), KEEP_CONFLICT);
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let spawned = spawn_with(&repo, &state, &id, &runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "turn 1 の spawn: {}", stderr_of(&spawned));
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(moved, base, "turn の中で main が進んだ");
    assert!(!stub_stdin(&state, 1).contains("## 追随"), "spawn 時は main = base ゆえ節は無い: {}", stub_stdin(&state, 1));
    assert_eq!(stub_calls(&state), 1, "turn は 1 回");
    assert_eq!(
        rebase_details(&state, &id),
        vec![format!("rebase:{base}..{moved}")],
        "器が base を進めた記帳は 1 件: {:?}",
        stages(&state, &id)
    );
    assert!(
        stdout_of(&spawned).contains(&format!("run={id} rebase={base}..{moved}")),
        "新しい base を名乗る: {}",
        stdout_of(&spawned)
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段は Implemented（次は gate）");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(
        gated.status.code(),
        Some(i32::from(RC_OK)),
        "新しい base の gate は PASS（main 側の other.txt を外れと数えない）: {}",
        stderr_of(&gated)
    );
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 1, "cmd"), "write-set", "段①");
    assert_eq!(row_value(&rows, 1, "rc"), "0", "段①（write-set）が緑");
    assert_eq!(
        row_value(&rows, 2, "cmd"),
        format!("git rev-parse --verify {moved}"),
        "gate が読む base（`base_of_run`）は new main"
    );
    clean(&[&repo, &state]);
}

/// 負例: 追随節なしで main が進んでも、runner が rebase しなければ **`rebase:` の記帳は無く**
/// `Implemented` の detail は空のまま（merge-base は記録済みの base と同じ＝進んでいない）。
#[test]
fn pipe_follow_self_rebase_records_nothing_when_the_runner_does_not_rebase() {
    let (repo, state) = repo_with_state();
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let runner = stub_runner_turns(&state, &no_rebase_body(&repo), KEEP_CONFLICT);
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let spawned = spawn_with(&repo, &state, &id, &runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "turn 1 の spawn: {}", stderr_of(&spawned));
    assert_ne!(git(&repo, &["rev-parse", "refs/heads/main"]), base, "turn の中で main が進んだ");
    assert!(rebase_details(&state, &id).is_empty(), "base は進めない: {:?}", stages(&state, &id));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Implemented), None)),
        "Implemented の detail は空のまま: {:?}",
        stages(&state, &id)
    );
    assert!(!stdout_of(&spawned).contains("rebase="), "base を名乗らない: {}", stdout_of(&spawned));
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "元の base の gate は PASS: {}", stderr_of(&gated));
    assert_eq!(
        row_value(&verify_rows(&state, &id), 2, "cmd"),
        format!("git rev-parse --verify {base}"),
        "gate が読む base は記録済みのまま"
    );
    clean(&[&repo, &state]);
}

/// 追随節の無い turn でも、木が rebase の途中のまま終わった周は `Failed detail=rebase-dirty`
/// で終端する（`mid_rebase` の検査が節の有無に依らない・fail-closed・設計 §3 手順 6）。
#[test]
fn pipe_follow_self_rebase_mid_rebase_turn_fails_dirty_without_a_follow_section() {
    let (repo, state) = repo_with_state();
    let runner = stub_runner_turns(&state, &mid_rebase_body(&repo), KEEP_CONFLICT);
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let spawned = spawn_with(&repo, &state, &id, &runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_REFUSED)), "rebase の途中は rc 1: {}", stderr_of(&spawned));
    assert!(!stub_stdin(&state, 1).contains("## 追随"), "節は無い: {}", stub_stdin(&state, 1));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-dirty".to_owned()))),
        "終端の理由: {:?}",
        stages(&state, &id)
    );
    assert!(mid_rebase(&repo, &id), "木は rebase の途中のまま（器は触らない）");
    assert!(rebase_details(&state, &id).is_empty(), "途中の木では base を進めない: {:?}", stages(&state, &id));
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "段は Failed");
    clean(&[&repo, &state]);
}

/// (a) docs だけで main が動いた便の追随（設計 §33）: **再 gate を丸ごと撃たず**（lens も起こさず）
/// 前周の Gated PASS を新しい base へ引き継いで着地する。撃つのは主実測の ②④ だけ（③ は主実測も撃たない・
/// 設計 gate-cost.md §44 形 (9)）で、`verify.jsonl` に足されるのは引き継ぎの 1 本だけ。
///
/// base は再 gate を撃つので、呼出が ②④ の 2 行 + record 3 本ぶん多く、偽 lens の marker も立つ＝RED。
#[test]
fn pipe_follow_docs_only_carries_gated_pass_without_regate() {
    let (repo, state, id, base, before) = detection_gated();
    let moved = advance_main_with(&repo, "docs/design/toy.md");
    // 1 度目の gate が立てた marker を外す＝「land の中で lens が起きたか」だけを効果で測る。
    let marker = state.join("lens-ran");
    fs::remove_file(&marker).expect("1 度目の gate の marker を消せる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("rebase={base}..{moved}")), "追随は済む: {stdout}");
    assert!(stdout.contains("verdict=PASS"), "引き継いだ判定行: {stdout}");
    assert!(!marker.exists(), "再 gate を撃たない＝lens は 1 度も起きない: {stdout}");
    // 着地後の検出の子（設計 gate-cost.md §44 形 (11)）は着地した diff が面の外（toy の `src/lib.rs`）＝stub を撃たず
    // `landed` 付きの skip record 1 本だけを足す。子の終わりを待ってから測る。
    super::gate::await_detection_child(&state, &id);
    let added = super::gate::detection_calls(&repo).split_off(before);
    assert_eq!(added, ["common", "contract"], "撃つのは主実測の ②④ だけ（母集団 = 前 {before} 行）");
    assert_regate_skip_record(&verify_rows(&state, &id));
    // 主実測は別 file（`verify-main.jsonl`）に ①②④ の 3 本。木は gate を撃った周と違う（docs の 1 file ぶん進んでいる）
    // ので主実測は撃ち、③ は撃たない（record も置かない・設計 gate-cost.md §44 形 (9)）。
    let (main, after) = super::gate::split_landed(super::gate::main_rows(&state, &id));
    assert_eq!(after.len(), 1, "着地後の record は 1 本: {after:?}");
    assert_eq!(super::gate::kinds(&main), ["write-set", "common", "contract"], "主実測は ①②④ だけ: {main:?}");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed: {}", show_line(&repo, &state, &id));
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS", "引き継いだ verdict");
    assert_follow_events(&state, &base, &moved, &git(&repo, &["rev-parse", "refs/heads/main"]));
    clean(&[&repo, &state]);
}

/// (a) 便が消した path を名指す行が追随で入った周は `Implemented detail=rebase-stale-rows:<base>..<main>` を記帳して
/// runner を 1 回起こし直し、写しの write-set の**末尾に 1 項目**だけ行の設計 doc を足す（既存の項目は字面も順序も不変）。
/// 再 gate は撃たない（偽 lens の marker 0）。起こし直しの stdin の「追随」節は行を `<doc>#<id>: <項目>` で名指す。
/// base（従来どおり docs だけの追随＝Gated PASS を引き継いで着地・runner は 1 回）では記帳も起こし直しも無い＝RED。
#[test]
fn pipe_follow_stale_rows_restarts_the_runner_and_appends_the_design_doc() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, base, moved, runner) = stale_rows_run(&repo, &state, &marker, FIX_ROW, "src/gone.rs");
    let before = write_set_line(&state, &id);
    assert!(before.ends_with(']') && !before.contains(STALE_DOC), "fixture: 写しの write-set の行: {before}");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = land_extra(&repo, &state, &id, &["--runner", &runner, "--lens", &lens]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "起こし直した周は rc 3: {} / {}", stdout_of(&out), stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("run={id} next=gate")), "次に撃つ段: {stdout}");
    assert!(!stdout.contains("landed="), "land しない: {stdout}");
    assert_eq!(stub_calls(&state), 2, "runner を 1 回起こし直した");
    assert!(!marker.exists(), "再 gate は撃たれない（偽 lens の写し 0）");
    assert_stale_rows_record(&state, &id, &format!("{base}..{moved}"));
    let widened = format!("{}, \"{STALE_DOC}\"]", before.strip_suffix(']').unwrap_or_default());
    assert_eq!(write_set_line(&state, &id), widened, "末尾へ 1 項目だけ追記（既存の項目は不変）");
    assert!(stderr_of(&out).contains(STALE_DOC), "追記した項目を stderr に写す: {}", stderr_of(&out));
    let second = stub_stdin(&state, 2);
    assert!(second.contains("## 追随"), "起こし直しの turn に節が付く: {second}");
    assert!(second.contains(&format!("{STALE_DOC}#z: src/gone.rs")), "節は行と未解決の項目を名指す: {second}");
    assert!(stdout.contains(&format!("run={id} rebase={base}..{moved}")), "turn の後に base が進む: {stdout}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段は Implemented（次は gate）");
    clean(&[&repo, &state]);
}

/// (b) 行が便と無関係の path（便が消していない・base にも無い）を名指す周は起こし直さず、従来どおりの追随（docs だけ＝
/// Gated PASS を引き継ぐ）で着地する。記帳 0・runner は turn 1 の 1 回だけ・写しの write-set は不変。
#[test]
fn pipe_follow_stale_rows_unrelated_path_goes_on_as_before() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, base, moved, runner) = stale_rows_run(&repo, &state, &marker, FIX_ROW, "src/never.rs");
    let before = write_set_line(&state, &id);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = land_extra(&repo, &state, &id, &["--runner", &runner, "--lens", &lens]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "従来どおり着地: {} / {}", stdout_of(&out), stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("rebase={base}..{moved}")), "追随は済む: {stdout}");
    assert!(stdout.contains("landed="), "着地する: {stdout}");
    assert_eq!(stub_calls(&state), 1, "起こし直さない");
    assert_eq!(stale_rows_count(&state, &id), 0, "記帳しない: {:?}", stages(&state, &id));
    assert_eq!(write_set_line(&state, &id), before, "写しの write-set は不変");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// (c) 回数は衝突と**同じ 1 つの上限**（`pipe.follow_retries`＝fixture で 1）: 衝突の記帳を 1 件持つ便は、最初の
/// 契約表の行の周で上限に達して `Failed detail=rebase-stale-rows` + rc 1 で終端する（runner は起こさない・main は不動）。
#[test]
fn pipe_follow_stale_rows_exhausted_fails_typed_under_the_conflict_limit() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, base, moved, runner) = stale_rows_run(&repo, &state, &marker, FIX_ROW, "src/gone.rs");
    record_conflict(&state, &id, &format!("{base}..{moved}"));
    let rules = write_rules_with_retries(&state, "rules-stale-rows-1.toml", 1, 1_000_000, 1);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_with_rules(&repo, &state, &id, &rules, &lens);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "撃ち直しの gate: {}", stderr_of(&gated));
    fs::remove_file(&marker).ok();
    let out = land_extra(&repo, &state, &id, &["--runner", &runner, "--lens", &lens, "--rules", &rules.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "上限に達した周は rc 1: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stderr_of(&out).contains("上限"), "理由は上限を名乗る: {}", stderr_of(&out));
    let trail = stages(&state, &id);
    assert_eq!(
        trail.last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-stale-rows".to_owned()))),
        "終端の理由: {trail:?}"
    );
    assert_eq!(stale_rows_count(&state, &id), 1, "記帳は 1 件: {trail:?}");
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は手で積んだ 1 件のまま: {trail:?}");
    assert_eq!(stub_calls(&state), 1, "runner を起こさない");
    assert!(!marker.exists(), "再 gate は撃たれない");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "段は Failed");
    clean(&[&repo, &state]);
}

/// (d) `--runner` の無い land は記帳（と写しの追記）を残して rc 1 で止まり（`--runner が要る`・段は `Implemented`）、
/// `pipe resume --runner` が同じ起こし直しを続ける（衝突と同じ弁別の 1 本）。resume の turn も行の一覧を受ける。
#[test]
fn pipe_follow_stale_rows_no_runner_records_and_resume_continues() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, base, moved, runner) = stale_rows_run(&repo, &state, &marker, FIX_ROW, "src/gone.rs");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = land_extra(&repo, &state, &id, &["--lens", &lens]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "起こし直せない land は rc 1: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stderr_of(&out).contains("--runner が要る"), "理由: {}", stderr_of(&out));
    assert_eq!(stale_rows_count(&state, &id), 1, "記帳は残る: {:?}", stages(&state, &id));
    assert_eq!(stub_calls(&state), 1, "起こし直していない");
    assert!(write_set_line(&state, &id).ends_with(&format!(", \"{STALE_DOC}\"]")), "追記は残る: {}", write_set_line(&state, &id));
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段は Implemented（続けられる側）");
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "resume の起こし直し: {} / {}", stdout_of(&resumed), stderr_of(&resumed));
    assert_eq!(stub_calls(&state), 2, "resume が runner を起こした");
    assert!(
        stub_stdin(&state, 2).contains(&format!("{STALE_DOC}#z: src/gone.rs")),
        "resume の turn も行を名指す: {}",
        stub_stdin(&state, 2)
    );
    assert!(stdout_of(&resumed).contains(&format!("run={id} rebase={base}..{moved}")), "base が進む: {}", stdout_of(&resumed));
    assert!(!marker.exists(), "再 gate は撃たれない");
    clean(&[&repo, &state]);
}
