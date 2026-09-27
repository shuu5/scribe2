// flip-check: moved s2-07l.686
//! 待ちと release と gated の族の歯（接頭辞 `pipe_dispatch_waiting_` / `pipe_dispatch_release_` / `pipe_dispatch_gated_` /
//! `pipe_dispatch_regated_` / `pipe_dispatch_revive_`・設計 docs/design/carry-prep.md §10 行 n・親
//! `tests/e2e/pipe/dispatch.rs` の helper を `use super::*` で使う）。

use super::*;

/// (§12 列へ戻す印) `Failed` で終端した便の bead は `settled` で列外だが、その後の `release` で**同じ sha の
/// まま**列に戻り（`reason=-`・`ready=1`）、起こし直した便が同じ sha でまた終端に着くと再び `settled` になる
/// （**印 1 回で起き直るのは 1 回**＝§2 の無限再起動を開け直さない）。
///
/// 起こし直した便は run dir の fixture で作る（同じ bead で秒を跨いで intake → rc 2 の runner）。起こす
/// 効果そのものは印の直後の 1 周の歯（`..._marks_fire_without_children`）と手動の 1 周の歯が測る。
#[test]
fn pipe_dispatch_release_requeues_a_failed_run_once_at_the_same_sha() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    let first = failed_run(&repo, &state, bead);
    let (settled, released) = reasons_around_release(&repo, &state, bead, "Failed");
    assert_eq!(released, "-", "release で同じ sha のまま列に戻る");
    let bd = fake_bd(&state, &[issue(bead, 2, "a")]);
    let back = ls(&repo, &state, &bd);
    assert_eq!(count_of(&back), format!("{COUNT} total=1 ready=1"), "戻った契約は起こせる（{}）", told(&back));
    // **起こし直した便が同じ sha でまた終端に着く**（秒を跨いで同じ bead の 2 本目・同じ契約 file）。
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = failed_run(&repo, &state, bead);
    assert_ne!(second, first, "起こし直した便は新しい run id");
    let again = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&again, bead), settled, "同じ sha でまた終端＝再び settled（印は 1 回しか効かない）（{}）", told(&again));
    assert_eq!(count_of(&again), format!("{COUNT} total=1 ready=0"), "2 度目は起こさない");
    // 2 度目の `release` はまた 1 回だけ戻す（印ごとに 1 回）。
    release(&state, bead);
    let twice = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&twice, bead), "-", "2 度目の release でまた戻る（{}）", told(&twice));
    clean(&[&repo, &state]);
}

/// (§12 列へ戻す印) 終端より**前**の `release` は効かない——印は便の最後の記帳より後に在る 1 件だけを見る。
#[test]
fn pipe_dispatch_release_requeues_nothing_when_the_mark_precedes_the_terminal() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    let id = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), bead);
    // live な便のうちに印を打つ（この時点では列外でなく、自分の便との交差で待つ）。
    release(&state, bead);
    let bd = fake_bd(&state, &[issue(bead, 2, "a")]);
    let live = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&live, bead), format!("overlap:{id}/1"), "終端の前は交差で待つ（{}）", told(&live));
    // その後に終端へ着く（rc 2 の runner）。
    fail_run(&repo, &state, &id);
    let after = ls(&repo, &state, &bd);
    let reason = reason_of(&after, bead);
    assert!(reason.starts_with("settled:"), "終端より前の release は効かない＝列外のまま（{}）", told(&after));
    assert!(reason.ends_with("/Failed"), "段は Failed: {reason}");
    assert_eq!(count_of(&after), format!("{COUNT} total=1 ready=0"), "起こさない");
    clean(&[&repo, &state]);
}

/// (§12 戻さない段) 審査 FAIL（`Reviewed` で終端）の便は `release` の後も `settled` のまま
/// （FR49「中身が変わるまで列に入らない」）。
#[test]
fn pipe_dispatch_release_requeues_not_a_review_failed_run() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    let id = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), bead);
    fs::write(state.join("pipe").join(&id).join(REVIEW_FILE), "{\"verdict\":\"FAIL\"}\n")
        .expect("審査の判定を書ける");
    let (settled, released) = reasons_around_release(&repo, &state, bead, "Reviewed");
    assert_eq!(released, settled, "審査 FAIL は release の後も列外のまま（理由も変わらない）");
    clean(&[&repo, &state]);
}

/// (§22 (b)) 審査を測れなかった便（`Reviewed` の INCONCLUSIVE `kind:unparsed`）は `release` で**同じ sha のまま**
/// 列に戻り（`reason=-`・`ready=1`）、起こし直した便が同じ sha でまた unparsed に着けば再び列外（印 1 回で 1 回）。
/// base は `Reviewed` を判定の中身を見ずに戻さない（`settled:…/Reviewed` のまま＝RED）。
#[test]
fn pipe_dispatch_release_unparsed_requeues_an_unmeasured_review_once_at_the_same_sha() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    let first = judged_run(&repo, &state, bead, UNPARSED);
    let (settled, released) = reasons_around_release(&repo, &state, bead, "Reviewed");
    assert_eq!(released, "-", "測れなかった審査は release で同じ sha のまま列に戻る");
    let bd = fake_bd(&state, &[issue(bead, 2, "a")]);
    let back = ls(&repo, &state, &bd);
    assert_eq!(count_of(&back), format!("{COUNT} total=1 ready=1"), "戻った契約は起こせる（{}）", told(&back));
    // **起こし直した便が同じ sha でまた unparsed に着く**（秒を跨いで同じ bead の 2 本目・同じ契約 file）。
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = judged_run(&repo, &state, bead, UNPARSED);
    assert_ne!(second, first, "起こし直した便は新しい run id");
    let again = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&again, bead), settled, "同じ sha でまた unparsed＝再び列外（印は 1 回しか効かない）（{}）", told(&again));
    assert_eq!(count_of(&again), format!("{COUNT} total=1 ready=0"), "2 度目は起こさない");
    clean(&[&repo, &state]);
}

/// (§22 (c)) 審査役が材料を読んで出した INCONCLUSIVE（`kind:section-material-missing`）と、kind が unparsed でも
/// FAIL の便は `release` の後も理由が変わらない（(b) が「INCONCLUSIVE を全部戻す」「unparsed を全部戻す」変異で
/// ないことを測る・FR49）。
#[test]
fn pipe_dispatch_release_unparsed_keeps_other_review_judgements_out() {
    for judgement in [
        "{\"verdict\":\"INCONCLUSIVE\",\"kind\":\"section-material-missing\"}",
        "{\"verdict\":\"FAIL\",\"kind\":\"unparsed\"}",
    ] {
        let (repo, state) = repo_with_state();
        two_rows(&repo);
        let bead = "s2-toy.1";
        judged_run(&repo, &state, bead, judgement);
        let (settled, released) = reasons_around_release(&repo, &state, bead, "Reviewed");
        assert_eq!(released, settled, "{judgement} は release の後も列外のまま（理由も変わらない）");
        clean(&[&repo, &state]);
    }
}

/// (§12 戻す段) gate の判定で終端になった便（`Gated` の verdict FAIL）は `release` で列に戻る——gate の
/// FAIL には flaky な歯で落ちた周が含まれ、契約の字を変えずに測り直す口が他に無い。
#[test]
fn pipe_dispatch_release_requeues_a_gate_failed_run() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    gated_run(&repo, &state, bead, "FAIL");
    let (_, released) = reasons_around_release(&repo, &state, bead, "Gated");
    assert_eq!(released, "-", "gate FAIL は release で戻る");
    clean(&[&repo, &state]);
}

/// (§13) 回答済みの `Questioned` の便（driver の札なし）は手動の 1 周で **`--drive` 付きの resume** で起こされ
/// （`resumed:1`）、先の段へ進んで人の手なしに `Landed` まで通る。base は札の無い便を触らない（RED）。
#[test]
fn pipe_dispatch_waiting_gate_answered_question_is_resumed_with_drive() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    assert!(!state.join("pipe").join(&id).join("driver").exists(), "前提: 正常に抜けた driver は札を外している");
    let answered = answer(&state, &id, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&answered));
    let out = waiting_turn(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "回答済みの便を 1 本起こし直す（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Implemented"), 1, "先の段へ進む（段の並び: {}）", stages_of(&state, &id));
    // **`--drive` 付き**である証拠: 1 段で止まらず、継ぎの driver が着地まで通す。
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "自走で着地まで（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§13) 未回答の `Questioned` の便は起こされない（`resumed:0`・段は 1 つも動かない）。
#[test]
fn pipe_dispatch_waiting_gate_unanswered_question_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let out = waiting_turn(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "関門が閉じた便は起こさない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Implemented"), 0, "段は動かない（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(spawned_now(&state, &id), 1, "起こし直していない（Spawned は初回の 1 件）");
    clean(&[&repo, &state]);
}

/// (§13) 古い質問に回答が在っても**最新の**質問が未回答なら関門は閉じている（`resumed:0`）。
#[test]
fn pipe_dispatch_waiting_gate_newest_question_unanswered_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let answered = answer(&state, &id, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "1 つ目の回答は rc 0（{}）", told(&answered));
    // 2 つ目の質問で止まる runner で手で resume する（`--drive` は無い＝1 段で止まる）。
    let second = "printf '%s\\n' '{\"question\":\"write-set の外を触ってよいか\"}'; exit 76";
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", second,
        "--bd", &fake_bd(&state, &[]),
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_BLOCKED)), "2 つ目の質問で止まる（{}）", told(&resumed));
    assert_eq!(reached_now(&state, &id, "Questioned"), 2, "前提: 質問は 2 件（段の並び: {}）", stages_of(&state, &id));
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "最新の質問が未回答なら起こさない（{}）", told(&out));
    assert_eq!(spawned_now(&state, &id), 2, "起こし直していない（Spawned は手の 2 件のまま）");
    clean(&[&repo, &state]);
}

/// (§13 札の 4 値) **所有者が死んでいる**札の便は起こす（`pipe run` が回答待ちまで進めて死んだ形）。
#[test]
fn pipe_dispatch_waiting_gate_dead_ticket_is_resumed() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let answered = answer(&state, &id, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&answered));
    put_dead_ticket(&state, &id);
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "死んだ所有者の札の便は起こす（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "自走で着地まで（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§13 札の 4 値) **所有者が生きている**札の便は触らない（別の driver が駆動している便に 2 本目を立てない）。
#[test]
fn pipe_dispatch_waiting_gate_live_ticket_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let answered = answer(&state, &id, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&answered));
    // 歯の process 自身の pid＝確実に生きている所有者。
    put_ticket_body(&state, &id, &format!("{}\n", std::process::id()));
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "生きている所有者の札の便は触らない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Implemented"), 0, "段は動かない（段の並び: {}）", stages_of(&state, &id));
    assert!(state.join("pipe").join(&id).join("driver").exists(), "札は奪わない");
    clean(&[&repo, &state]);
}

/// (§13 札の 4 値) **在るのに読めない**札の便は触らない（測れないを「居ない」に読み替えない・fail-closed）。
#[test]
fn pipe_dispatch_waiting_gate_unreadable_ticket_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let answered = answer(&state, &id, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&answered));
    put_ticket_body(&state, &id, "not-a-pid\n");
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "読めない札の便は触らない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Implemented"), 0, "段は動かない（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(spawned_now(&state, &id), 1, "起こし直していない");
    clean(&[&repo, &state]);
}

/// (§13 契機) 道具を渡した `pipe answer` は記帳の直後に同じ 1 周を撃ち、便が進む。**stdout は記帳の 1 行だけ**
/// （1 周の行は足さない・終端の 1 周と同じ黙る形）。
#[test]
fn pipe_dispatch_waiting_gate_answer_with_tools_fires_a_turn_silently() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let out = answer(&state, &id, "verify は 1 行目だけを撃つ", &toy_tools(&repo, &state));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out), format!("run={id} answered=true\n"), "stdout は記帳の 1 行だけ（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Implemented"), 1, "記帳の直後の 1 周が便を進める（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "自走で着地まで（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§13 契機) 道具を渡さない `pipe answer` は今までどおり記帳だけで rc 0（便は次の契機まで待つ）。
#[test]
fn pipe_dispatch_waiting_gate_answer_without_tools_only_records() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let before = kind_count(&state, &id, vessel::fleet::EventKind::RunStage);
    let out = answer(&state, &id, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out), format!("run={id} answered=true\n"), "stdout は記帳の 1 行だけ（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Implemented"), 0, "便は進まない（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(kind_count(&state, &id, vessel::fleet::EventKind::RunStage), before, "段の記帳は増えない");
    assert!(answered_once(&state, &id), "記帳は成っている");
    clean(&[&repo, &state]);
}

/// (§13 契機) 記帳の直後の 1 周が失敗しても（台帳を読めない周）回答の rc は変わらず、stdout も記帳の 1 行だけ。
#[test]
fn pipe_dispatch_waiting_gate_failed_turn_keeps_the_answer_rc() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    // 台帳 client を rc 1 で落ちる script に差し替える＝1 周は `unmeasured` で 1 本も起こさない。
    let broken = script(&state.join("bd-broken"), "exit 1\n");
    let tools: Vec<String> = toy_tools(&repo, &state)
        .into_iter()
        .map(|item| if item.ends_with("/bd") { broken.clone() } else { item })
        .collect();
    let out = answer(&state, &id, "verify は 1 行目だけを撃つ", &tools);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周が失敗しても回答は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out), format!("run={id} answered=true\n"), "stdout は記帳の 1 行だけ（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Implemented"), 0, "測れない周は起こさない（段の並び: {}）", stages_of(&state, &id));
    assert!(answered_once(&state, &id), "記帳は成っている（回答の逐語が残る）");
    clean(&[&repo, &state]);
}

/// (§13) 段を前へ進めた driver の終端の 1 周は、別の回答済みの便を起こす（`resumed:1`）。
///
/// 便 B を `Gated` まで手で進め、`--drive` の resume で着地させる（`Gated` → `Landed` は前進・自分の便は
/// 終端ゆえ渡さない）。その終端の 1 周が、同じ置き場で回答を待っていた便 A を起こし直す。
#[test]
fn pipe_dispatch_waiting_gate_forward_driver_turn_resumes_another_answered_run() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    // A: 行 a（`src/lib.rs`）の便を質問で止めて回答する（道具なし＝記帳だけ・札は無い）。
    let asked = questioned_bead(&repo, &state, "a", "s2-toy.1");
    let answered = answer(&state, &asked, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&answered));
    // B: 行 b（`src/b.rs`・A と交差しない）の便を Gated まで人の手で進める。
    let other = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#b"), "s2-toy.2");
    let spawned = run_pipe(&[
        "spawn", "--run", &other, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/b.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "B の spawn は rc 0（{}）", told(&spawned));
    let lens = fake_lens(&state.join("forward-lens-ran"), &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &other, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "B の gate は rc 0（{}）", told(&gated));
    assert_eq!(spawned_now(&state, &asked), 1, "前提: A はまだ起こし直されていない");
    // B の driver（`--drive`）が Gated → Landed と段を前へ進め、終端の 1 周で A を起こす。
    let mut tools = toy_tools(&repo, &state);
    tools.push("--drive".to_owned());
    let out = with_tools(&["resume", "--run", &other], &tools);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "B の resume は rc 0（{}）", told(&out));
    assert_eq!(
        stdout_of(&out).lines().last(),
        Some(format!("{} drive=settled", resumed_line(1)).as_str()),
        "終端の 1 周が A を 1 本起こす（{}）",
        told(&out)
    );
    assert_eq!(stage_reached(&state, &other, "Landed"), 1, "B は着地（段の並び: {}）", stages_of(&state, &other));
    assert_eq!(stage_reached(&state, &asked, "Implemented"), 1, "A が先の段へ進む（段の並び: {}）", stages_of(&state, &asked));
    assert_eq!(stage_reached(&state, &asked, "Landed"), 1, "A も自走で着地まで（段の並び: {}）", stages_of(&state, &asked));
    clean(&[&repo, &state]);
}

/// (§15 (a)) verdict PASS ∧ 札の無い `Gated` の便は手動の 1 周で **`--drive` 付きの resume** で起こされ（`resumed:1`）、
/// 先の段（`Landed`）へ進む。base は `Gated` の便を札の所有者が死んだものしか候補にしない（RED）。
#[test]
fn pipe_dispatch_gated_pass_without_ticket_is_resumed_with_drive() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "PASS");
    let out = waiting_turn(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "PASS の Gated の便を 1 本起こし直す（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "先の段へ進む（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§15 (b)) verdict INCONCLUSIVE ∧ 札の無い `Gated` の便は起こされない（`resumed:0`・再 gate も起きない＝器が
/// 勝手に 1 周ぶんの費用を払い直さない）。
#[test]
fn pipe_dispatch_gated_pass_inconclusive_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "INCONCLUSIVE");
    let out = waiting_turn(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "INCONCLUSIVE の便は起こさない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "着地しない（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(reached_now(&state, &id, "Gated"), 1, "再 gate も起きない（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§15 (c)) verdict を**読めない** `Gated` の便も起こされない（測れないを「通った」に読み替えない・fail-closed）。
#[test]
fn pipe_dispatch_gated_pass_unreadable_verdict_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "PASS");
    let verdict = state.join("pipe").join(&id).join("verdict.json");
    assert!(verdict.exists(), "前提: 判定 file は在る");
    fs::write(&verdict, "not json\n").unwrap_or_else(|err| panic!("判定 file を壊せる: {err}"));
    let out = waiting_turn(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "読めない verdict の便は起こさない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "着地しない（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(reached_now(&state, &id, "Gated"), 1, "段は動かない（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§15 (d) 札の 4 値) **所有者が生きている**札の PASS の `Gated` の便は触らない（別の driver が駆動している便に
/// 2 本目を立てない・札も奪わない）。
#[test]
fn pipe_dispatch_gated_pass_live_ticket_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "PASS");
    // 歯の process 自身の pid＝確実に生きている所有者。
    put_ticket_body(&state, &id, &format!("{}\n", std::process::id()));
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "生きている所有者の札の便は触らない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "着地しない（段の並び: {}）", stages_of(&state, &id));
    assert!(state.join("pipe").join(&id).join("driver").exists(), "札は奪わない");
    clean(&[&repo, &state]);
}

/// (§15 (d) 札の 4 値) **在るのに読めない**札の PASS の `Gated` の便は触らない（測れないを「居ない」に読み替えない）。
#[test]
fn pipe_dispatch_gated_pass_unreadable_ticket_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "PASS");
    put_ticket_body(&state, &id, "not-a-pid\n");
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "読めない札の便は触らない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "着地しない（段の並び: {}）", stages_of(&state, &id));
    assert!(state.join("pipe").join(&id).join("driver").exists(), "札は触らない");
    clean(&[&repo, &state]);
}

/// (§15 (e)) 札の**所有者が死んでいる** `Gated` の便は verdict に依らず今までどおり起こされる（既存の規則・
/// **母集団 = PASS と INCONCLUSIVE の 2 値**）。起こし直しが**実際に走った**証拠は、死んだ所有者の札を継いだ
/// resume が抜けるときに自分の札を外すこと（数えただけでは撃ったと言えない）。PASS の周は着地まで通り、
/// INCONCLUSIVE の周は resume が `next=gate` で止まる（自動では測り直さない・段は `Gated` のまま）。
#[test]
fn pipe_dispatch_gated_pass_dead_ticket_is_resumed_regardless_of_verdict() {
    for verdict in ["PASS", "INCONCLUSIVE"] {
        let (repo, state) = repo_with_state();
        let id = gated_without_ticket(&repo, &state, verdict);
        put_dead_ticket(&state, &id);
        let ticket = state.join("pipe").join(&id).join("driver");
        let out = waiting_turn(&repo, &state);
        assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "{verdict}: 死んだ所有者の札の便は起こす（{}）", told(&out));
        assert!(gone(&ticket), "{verdict}: 継いだ resume が抜けるときに死んだ札を外す（段の並び: {}）", stages_of(&state, &id));
        let landed = match verdict {
            "PASS" => stage_reached(&state, &id, "Landed"),
            _ => not_reached(&state, &id, "Landed"),
        };
        assert_eq!(landed, usize::from(verdict == "PASS"), "{verdict}: 着地は PASS の周だけ（段の並び: {}）", stages_of(&state, &id));
        clean(&[&repo, &state]);
    }
}

/// (§15 (f)) 段を前へ進めなかった driver の終端の 1 周は、この候補を 1 本も起こさない（空撃ちの連鎖を塞ぐ）。
///
/// 便 B（行 b）を INCONCLUSIVE の `Gated` に置き、INCONCLUSIVE の lens で `--drive` の resume を撃つ（再 gate で
/// 同じ段＝`no-progress`）。その終端の 1 周は、同じ置き場で PASS の `Gated` に在った便 A（行 a・札なし）を起こさない。
/// 正負の対: その後の手動の 1 周（driver でない契機）は A を起こす。
#[test]
fn pipe_dispatch_gated_pass_no_progress_driver_turn_resumes_nothing() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let passed = gated_bead(&repo, &state, "a", "s2-toy.1", "PASS");
    let other = gated_bead(&repo, &state, "b", "s2-toy.2", "INCONCLUSIVE");
    let unsure = fake_lens(&state.join("gated-pass-unsure-lens"), &lens_verdict("INCONCLUSIVE"));
    let out = resume_once(&repo, &state, &other, &unsure, true);
    assert_eq!(
        stdout_of(&out).lines().last(),
        Some(format!("{} drive=no-progress", resumed_line(0)).as_str()),
        "段が動かなかった driver の 1 周は A を起こさない（{}）",
        told(&out)
    );
    assert_eq!(not_reached(&state, &passed, "Landed"), 0, "A は着地しない（段の並び: {}）", stages_of(&state, &passed));
    let manual = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&manual).trim_end(), resumed_line(1), "手動の 1 周は A を起こす（{}）", told(&manual));
    assert_eq!(stage_reached(&state, &passed, "Landed"), 1, "A は自走で着地まで（段の並び: {}）", stages_of(&state, &passed));
    clean(&[&repo, &state]);
}

/// (§15 (h)) **flag の無い** resume が Implemented → `Gated`（PASS）で抜けた直後の自分の終端の 1 周は自分の便を
/// 起こさず（段は `Gated` のまま＝「1 段だけ」を保つ）、その後の手動の 1 周は同じ便を起こす（正負の対）。
///
/// flag の無い周も終端の 1 周は撃つ（`--repo` と `--state-dir` と `--runner` を渡す）。抜けた driver は札を外して
/// いるので、自分の id を列に渡さなければ PASS の枝が自分の便を拾い、着地まで運んでしまう。
#[test]
fn pipe_dispatch_gated_pass_flagless_driver_leaves_its_own_run_for_the_next_turn() {
    let (repo, state) = repo_with_state();
    let contract = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &contract);
    let lens = fake_lens(&state.join("gated-pass-flagless-lens"), &lens_verdict("PASS"));
    let out = resume_once(&repo, &state, &id, &lens, false);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "flag 無しの resume は rc 0（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Gated"), 1, "1 段だけ進む（段の並び: {}）", stages_of(&state, &id));
    assert!(!state.join("pipe").join(&id).join("driver").exists(), "抜けた driver は札を外している");
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "自分の終端の 1 周は自分の便を起こさない（段の並び: {}）", stages_of(&state, &id));
    let manual = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&manual).trim_end(), resumed_line(1), "手動の 1 周は同じ便を起こす（{}）", told(&manual));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "自走で着地まで（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§23 (a)) 札の無い regate 済みの便は手動の 1 周で `--drive` 付きの resume で起こされ（`resumed:1`）、`Gated` の
/// 記帳が 1 件増えて `Landed` まで進む。base は `resumed:0`（機能不在）。
#[test]
fn pipe_dispatch_regated_run_without_a_ticket_is_resumed_to_landed() {
    let (repo, state) = repo_with_state();
    let id = regated_without_ticket(&repo, &state);
    let gated = gate_runs(&state, &id);
    let out = waiting_turn(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "regate 済みの便を 1 本起こす（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "着地まで（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(gate_runs(&state, &id), gated + 1, "gate をもう 1 周（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§23 (b)) 札の所有者が死んでいる判定 FAIL の `Gated` の便は regate を通って worktree の path・HEAD・判定の
/// verdict が変わらず、続く手動の 1 周は `resumed:1`（二重起動 0）で `Gated` が 1 件だけ増え、死んだ札は外れる。
#[test]
fn pipe_dispatch_regated_dead_ticket_keeps_three_records_and_resumes_once() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "FAIL");
    put_dead_ticket(&state, &id);
    let ticket = state.join("pipe").join(&id).join("driver");
    let worktree = worktree_of(&repo, &id);
    let head = git(&worktree, &["rev-parse", "HEAD"]);
    regate_run(&repo, &state, &id);
    assert!(worktree.is_dir(), "worktree の path は同じ");
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head, "worktree の HEAD は動かない");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "FAIL", "判定の verdict は書き換えない");
    let gated = gate_runs(&state, &id);
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "起こすのは 1 本（{}）", told(&out));
    assert!(gone(&ticket), "継いだ resume が死んだ札を外す（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "着地まで（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(gate_runs(&state, &id), gated + 1, "Gated は 1 件だけ増える（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§23 (d)) 札の所有者が生きている regate 済みの便と、札が在るのに読めない regate 済みの便は起こさず札も触らない
/// （母集団 = 札の 2 値・(a)(b) と合わせて 4 値）。
#[test]
fn pipe_dispatch_regated_live_or_unreadable_ticket_is_left_alone() {
    for (form, body) in [("live", format!("{}\n", std::process::id())), ("unreadable", "not-a-pid\n".to_owned())] {
        let (repo, state) = repo_with_state();
        let id = regated_without_ticket(&repo, &state);
        put_ticket_body(&state, &id, &body);
        let before = kind_count(&state, &id, vessel::fleet::EventKind::RunStage);
        let out = waiting_turn(&repo, &state);
        assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "{form}: 起こさない（{}）", told(&out));
        assert_eq!(not_reached(&state, &id, "Gated"), 1, "{form}: gate を撃たない（段の並び: {}）", stages_of(&state, &id));
        assert_eq!(kind_count(&state, &id, vessel::fleet::EventKind::RunStage), before, "{form}: 段を動かさない");
        let kept = fs::read_to_string(state.join("pipe").join(&id).join("driver")).ok();
        assert_eq!(kept.as_deref(), Some(body.as_str()), "{form}: 札は触らない");
        clean(&[&repo, &state]);
    }
}

/// (§23 (e)) 段を前へ進めなかった driver の終端の 1 周は regate 済みの便を起こさず、その後の手動の 1 周は起こす。
///
/// 便 B（行 b）を INCONCLUSIVE の `Gated` に置き、INCONCLUSIVE の lens で `--drive` の resume を撃つ（`no-progress`）。
/// 同じ置き場の便 A（行 a・判定 FAIL を regate で戻した・札なし）は、その終端の 1 周では動かない。
#[test]
fn pipe_dispatch_regated_no_progress_driver_turn_leaves_it_for_the_manual_turn() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let regated = gated_bead(&repo, &state, "a", "s2-toy.1", "FAIL");
    regate_run(&repo, &state, &regated);
    let other = gated_bead(&repo, &state, "b", "s2-toy.2", "INCONCLUSIVE");
    let unsure = fake_lens(&state.join("regated-unsure-lens"), &lens_verdict("INCONCLUSIVE"));
    let out = resume_once(&repo, &state, &other, &unsure, true);
    assert_eq!(
        stdout_of(&out).lines().last(),
        Some(format!("{} drive=no-progress", resumed_line(0)).as_str()),
        "段が動かなかった driver の 1 周は A を起こさない（{}）",
        told(&out)
    );
    assert_eq!(not_reached(&state, &regated, "Gated"), 1, "A は gate を撃たれない（段の並び: {}）", stages_of(&state, &regated));
    let manual = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&manual).trim_end(), resumed_line(1), "手動の 1 周は A を起こす（{}）", told(&manual));
    assert_eq!(stage_reached(&state, &regated, "Landed"), 1, "A は自走で着地まで（段の並び: {}）", stages_of(&state, &regated));
    clean(&[&repo, &state]);
}

/// (§23 (f)・AC47 の自動の regate 0/K) 判定 FAIL の `Gated` の便（札なし・札の所有者が死んでいる の 2 形）に regate を
/// 撃たずに手動の 1 周を K 回撃っても、どの周も `resumed:0` で `Implemented` の記帳は 1 件も増えない。
#[test]
fn pipe_dispatch_regated_none_without_a_ruling_over_k_turns() {
    const K: usize = 3;
    for form in ["absent", "dead"] {
        let (repo, state) = repo_with_state();
        let id = gated_without_ticket(&repo, &state, "FAIL");
        if form == "dead" {
            put_dead_ticket(&state, &id);
        }
        let implemented_before = run_stages(&state, &id, "Implemented", "");
        for turn in 1..=K {
            let out = waiting_turn(&repo, &state);
            assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "{form}: 周 {turn}/{K} は起こさない（{}）", told(&out));
        }
        assert_eq!(
            (form, K, run_stages(&state, &id, "Implemented", "")),
            (form, K, implemented_before),
            "{form}: K={K} 周で Implemented の記帳は 0 件増（段の並び: {}）",
            stages_of(&state, &id)
        );
        clean(&[&repo, &state]);
    }
}

/// (§25 (a)) Gated PASS → 追随 `rebase:` で戻った札の無い便 A は、段を前へ進めた別の便 B の driver の終端の 1 周
/// （`gated` の周）で起こされ（`resumed:1`）、`--drive` 付きなので着地まで進む。base は `resumed:0`（機能不在）。
///
/// B は A より先に `Gated` へ着ける（列の鍵は最初の `Gated` の ts＝B が先に着地できる）。A の追随の記帳は main を
/// 動かさない字面（`rebase:<HEAD>..<HEAD>`）で手で置く。B の `--drive` の resume が `Gated` → `Landed` と進め、
/// その終端の 1 周が A を起こす。
#[test]
fn pipe_dispatch_revive_followed_rebase_is_resumed_by_a_progressing_driver_turn() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let driver = gated_bead(&repo, &state, "b", "s2-toy.2", "PASS");
    let followed = gated_bead(&repo, &state, "a", "s2-toy.1", "PASS");
    let head = git(&repo, &["rev-parse", "HEAD"]);
    put_run_stage(&state, &followed, "Implemented", Some(&format!("rebase:{head}..{head}")));
    let gated = gate_runs(&state, &followed);
    let mut tools = toy_tools(&repo, &state);
    tools.push("--drive".to_owned());
    let out = with_tools(&["resume", "--run", &driver], &tools);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "B の resume は rc 0（{}）", told(&out));
    assert_eq!(
        stdout_of(&out).lines().last(),
        Some(format!("{} drive=settled", resumed_line(1)).as_str()),
        "B の終端の 1 周が A を 1 本起こす（{}）",
        told(&out)
    );
    assert_eq!(stage_reached(&state, &driver, "Landed"), 1, "B は着地（段の並び: {}）", stages_of(&state, &driver));
    revived_to_landed(&state, &followed, gated);
    clean(&[&repo, &state]);
}

/// (§25 (b)) 衝突の起こし直し（`rebase-conflict:`）で戻った札の無い便も同じく起こされ、着地まで進む。
#[test]
fn pipe_dispatch_revive_followed_conflict_is_resumed_with_drive() {
    let (repo, state) = repo_with_state();
    let id = conflict_followed(&repo, &state);
    let gated = gate_runs(&state, &id);
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "衝突の後の便を 1 本起こす（{}）", told(&out));
    revived_to_landed(&state, &id, gated);
    clean(&[&repo, &state]);
}

/// (§25 (c)) 追随の後にもう 1 度 `Gated` を経た便は、その後ろの runner の完了の記帳で `Implemented` に在っても
/// 起こさない（最新の `Gated` より後ろに追随の記帳が無い）。
#[test]
fn pipe_dispatch_revive_followed_then_gated_again_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = conflict_followed(&repo, &state);
    put_run_stage(&state, &id, "Gated", Some("verdict:PASS"));
    put_run_stage(&state, &id, "Implemented", None);
    let before = kind_count(&state, &id, vessel::fleet::EventKind::RunStage);
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "追随の後に gate を経た便は起こさない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "着地しない（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(kind_count(&state, &id, vessel::fleet::EventKind::RunStage), before, "段を動かさない");
    clean(&[&repo, &state]);
}

/// (§25 (c) 書き換え・旧 §23 (c)) regate の後に PASS の gate を通し、main を進めて `pipe follow` で戻した便（札なし）
/// は、追随が最新の `Gated` より後ろなので起こされる（regate の後の追随でも起こす）。
#[test]
fn pipe_dispatch_revive_followed_after_a_regate_and_a_gate_is_resumed() {
    let (repo, state) = repo_with_state();
    let id = regated_without_ticket(&repo, &state);
    let lens = fake_lens(&state.join("regated-pass-lens"), &lens_verdict("PASS"));
    let passed = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "regate の後の gate は PASS（{}）", told(&passed));
    follow_run(&repo, &state, &id);
    let gated = gate_runs(&state, &id);
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "regate の後の追随でも起こす（{}）", told(&out));
    revived_to_landed(&state, &id, gated);
    clean(&[&repo, &state]);
}

/// (§25 (d)) 追随の記帳の無い `Implemented` ∧ 札の無い便は起こさない（§5「札の無い便は触らない」のまま）。
#[test]
fn pipe_dispatch_revive_followed_none_without_a_follow_record() {
    let (repo, state) = repo_with_state();
    let contract = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &contract);
    assert_eq!(run_stages(&state, &id, "Implemented", "rebase"), 0, "前提: 追随の記帳は無い");
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "追随していない便は起こさない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Gated"), 0, "gate を撃たない（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§25 (e)) driver でない契機（手動の 1 周）でも、追随 `rebase:` で戻った札の無い便を起こし、着地まで進む。
#[test]
fn pipe_dispatch_revive_followed_rebase_is_resumed_by_a_manual_turn() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "PASS");
    follow_run(&repo, &state, &id);
    let gated = gate_runs(&state, &id);
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "手動の 1 周が起こす（{}）", told(&out));
    revived_to_landed(&state, &id, gated);
    clean(&[&repo, &state]);
}
