// flip-check: moved s2-07l.686
//! 待ちと release と gated の族の歯（接頭辞 `pipe_dispatch_waiting_` / `pipe_dispatch_release_` / `pipe_dispatch_gated_` /
//! `pipe_dispatch_regated_` / `pipe_dispatch_revive_`・設計 docs/design/carry-prep.md §10 行 n・親
//! `tests/e2e/pipe/dispatch.rs` の helper を `use super::*` で使う）。

use super::*;
use vessel::fleet::json_lite;

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

// ───── memo の引き金の満ち（設計 dispatcher.md §40・契約表の行 ao・接頭辞 `pipe_dispatch_memo_trigger_`） ─────

/// memo の行の書き出し（器の字面を借りない）。
const MEMO_LINE: &str = "[DISPATCH-MEMO]";

/// 作られた時刻を気にしない memo の時刻。
const MEMO_CREATED: &str = "2026-09-01T00:00:00Z";

/// 5 形の引き金の歯の置き場: 行 a・b を commit した repo に、台帳の接頭辞（依存と昇格の行の id の形が読む）を置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn memo_repo() -> (std::path::PathBuf, std::path::PathBuf) {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    fs::create_dir_all(repo.join(".beads")).expect(".beads を作れる");
    fs::write(repo.join(".beads").join("config.yaml"), "issue-prefix: s2\n").expect("台帳の接頭辞を書ける");
    (repo, state)
}

/// memo の台帳の 1 件（label `intake:memo`・昇格条件の節に `triggers` の行を置く・時刻の欄は `created` が `None` なら持たない）。
fn memo_of(id: &str, status: &str, created: Option<&str>, triggers: &[&str], notes: &str) -> String {
    let description = format!("### 出所\nx\n### 観測\nx\n### 候補\nx\n### 昇格条件\n{}\n", triggers.join("\n"));
    let created = created.map(|found| format!(",\"created_at\":{}", json_lite::quote(found))).unwrap_or_default();
    format!(
        "{{\"id\":\"{id}\",\"status\":\"{status}\",\"priority\":2,\"labels\":[\"intake:memo\"],\
         \"description\":{},\"notes\":{}{created}}}",
        json_lite::quote(&description),
        json_lite::quote(notes)
    )
}

/// 開いた memo（時刻は [`MEMO_CREATED`]）。
fn memo_bead(id: &str, triggers: &[&str], notes: &str) -> String {
    memo_of(id, "open", Some(MEMO_CREATED), triggers, notes)
}

/// `dispatch ls` の memo の行（出てきた順）。
fn memo_lines(out: &Output) -> Vec<String> {
    stdout_of(out).lines().filter(|line| line.starts_with(MEMO_LINE)).map(str::to_owned).collect()
}

/// memo 1 本の行の `key=` の値（行が無いか key が無い周は空）。
fn memo_field(out: &Output, id: &str, key: &str) -> String {
    memo_lines(out)
        .iter()
        .find(|line| line.contains(&format!(" memo={id} ")))
        .and_then(|line| line.split_whitespace().find_map(|word| word.strip_prefix(key)))
        .unwrap_or_default()
        .to_owned()
}

/// memo 1 本の `trigger=` の値の一覧（`(id, 値)`）を 1 回の ls で引く。
fn triggers_of(out: &Output, ids: &[&str]) -> Vec<(String, String)> {
    ids.iter().map(|id| ((*id).to_owned(), memo_field(out, id, "trigger="))).collect()
}

/// 期待の `(id, 値)` の列。
fn expected(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(id, value)| ((*id).to_owned(), (*value).to_owned())).collect()
}

/// (a) 再発: 本数＝値で満ち、値−1 で満ちない。満ちない形（再発 9）と満ちる形（期日が過去）の 2 本を持つ memo は満ちた形の語になる。
///
/// base は memo の行を出さない（RED・機能不在）。
#[test]
fn pipe_dispatch_memo_trigger_recurrence_meets_at_the_value_and_not_below() {
    let (repo, state) = memo_repo();
    let two = "[再発] 2026-09-28 一度目\n[再発] 2026-09-29 二度目\n";
    let bd = fake_bd(
        &state,
        &[
            memo_bead("s2-m.1", &["引き金: 再発 2"], two),
            memo_bead("s2-m.2", &["引き金: 再発 3"], two),
            memo_bead("s2-m.3", &["引き金: 再発 9", "引き金: 期日 2000-01-01T00:00Z"], two),
        ],
    );
    let out = ls(&repo, &state, &bd);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", told(&out));
    assert_eq!(
        triggers_of(&out, &["s2-m.1", "s2-m.2", "s2-m.3"]),
        expected(&[("s2-m.1", "met:再発"), ("s2-m.2", "unmet"), ("s2-m.3", "met:期日")]),
        "{}",
        told(&out)
    );
    clean(&[&repo, &state]);
}

/// (b) 期日: 周の時刻以前の期日で満ち、未来の期日で満ちない。
#[test]
fn pipe_dispatch_memo_trigger_deadline_meets_in_the_past_and_not_in_the_future() {
    let (repo, state) = memo_repo();
    let bd = fake_bd(
        &state,
        &[
            memo_bead("s2-m.1", &["引き金: 期日 2000-01-01T00:00Z"], ""),
            memo_bead("s2-m.2", &["引き金: 期日 2999-01-01T00:00Z"], ""),
        ],
    );
    let out = ls(&repo, &state, &bd);
    assert_eq!(
        triggers_of(&out, &["s2-m.1", "s2-m.2"]),
        expected(&[("s2-m.1", "met:期日"), ("s2-m.2", "unmet")]),
        "{}",
        told(&out)
    );
    clean(&[&repo, &state]);
}

/// (c) 同梱: 開いた契約の write-set の項目と等しい path と、その項目を含む dir で満ち、開いた契約の無い行の path（行 b の
/// `src/b.rs`）は満ちない。
#[test]
fn pipe_dispatch_memo_trigger_bundle_meets_an_open_contract_write_set_item() {
    let (repo, state) = memo_repo();
    let bd = fake_bd(
        &state,
        &[
            issue("s2-toy.1", 2, "a"),
            memo_bead("s2-m.1", &["引き金: 同梱 src/lib.rs"], ""),
            memo_bead("s2-m.2", &["引き金: 同梱 src/b.rs"], ""),
            memo_bead("s2-m.3", &["引き金: 同梱 src/"], ""),
        ],
    );
    let out = ls(&repo, &state, &bd);
    assert_eq!(
        triggers_of(&out, &["s2-m.1", "s2-m.2", "s2-m.3"]),
        expected(&[("s2-m.1", "met:同梱"), ("s2-m.2", "unmet"), ("s2-m.3", "met:同梱")]),
        "{}",
        told(&out)
    );
    clean(&[&repo, &state]);
}

/// (d) 依存: 相手の bead が閉じると満ち、開いていると満ちない。
#[test]
fn pipe_dispatch_memo_trigger_dependency_meets_when_the_bead_is_closed() {
    let (repo, state) = memo_repo();
    let bd = fake_bd(
        &state,
        &[
            listed("s2-toy.9", "closed", 2, "", &[]),
            listed("s2-toy.8", "open", 2, "", &[]),
            memo_bead("s2-m.1", &["引き金: 依存 s2-toy.9"], ""),
            memo_bead("s2-m.2", &["引き金: 依存 s2-toy.8"], ""),
        ],
    );
    let out = ls(&repo, &state, &bd);
    assert_eq!(
        triggers_of(&out, &["s2-m.1", "s2-m.2"]),
        expected(&[("s2-m.1", "met:依存"), ("s2-m.2", "unmet")]),
        "{}",
        told(&out)
    );
    clean(&[&repo, &state]);
}

/// (e) 着地: 値の設計 pointer を持つ bead が閉じると満ち、開いていると満ちない。
#[test]
fn pipe_dispatch_memo_trigger_landing_meets_when_the_pointer_bead_is_closed() {
    let (repo, state) = memo_repo();
    let bd = fake_bd(
        &state,
        &[
            listed("s2-toy.5", "closed", 2, &format!("design = {DESIGN_FILE}#a"), &[]),
            issue("s2-toy.6", 2, "b"),
            memo_bead("s2-m.1", &[&format!("引き金: 着地 {DESIGN_FILE}#a")], ""),
            memo_bead("s2-m.2", &[&format!("引き金: 着地 {DESIGN_FILE}#b")], ""),
        ],
    );
    let out = ls(&repo, &state, &bd);
    assert_eq!(
        triggers_of(&out, &["s2-m.1", "s2-m.2"]),
        expected(&[("s2-m.1", "met:着地"), ("s2-m.2", "unmet")]),
        "{}",
        told(&out)
    );
    clean(&[&repo, &state]);
}

/// 置き場の判定の file を書く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_memo_verdict(state: &Path, memo: &str, body: &str) {
    let dir = state.join("pipe").join("memo").join(memo);
    fs::create_dir_all(&dir).expect("memo の置き場を作れる");
    fs::write(dir.join("verdict"), body).expect("判定を書ける");
}

/// (f) 置き場の判定と時刻が `verdict=` と `judged=` に写り、置き場の無い memo は `-`、読めない判定の file は `unreadable`。
/// age は作られた時刻からの時間の切り捨て（5 時間 30 分前は 5h）で、作られた時刻の無い memo は `age=-`。
#[test]
fn pipe_dispatch_memo_trigger_verdict_and_age_are_read_from_the_place_and_the_ledger() {
    let (repo, state) = memo_repo();
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|found| found.as_secs()).unwrap_or_default();
    let ago = vessel::fleet::cli::format_utc(secs - 5 * 3600 - 1800);
    let unmet = ["引き金: 期日 2999-01-01T00:00Z"];
    let bd = fake_bd(
        &state,
        &[
            memo_of("s2-m.1", "open", Some(&ago), &unmet, ""),
            memo_of("s2-m.2", "open", Some(MEMO_CREATED), &unmet, ""),
            memo_of("s2-m.3", "open", None, &unmet, ""),
            memo_of("s2-m.4", "open", Some(MEMO_CREATED), &unmet, ""),
        ],
    );
    put_memo_verdict(
        &state,
        "s2-m.1",
        "{\"verdict\":\"promote\",\"at\":\"2026-09-30T01:02:03Z\",\"evidence\":\"e\",\"sketch\":\"s\"}\n",
    );
    put_memo_verdict(&state, "s2-m.4", "{\"verdict\":\"maybe\"}\n");
    let out = ls(&repo, &state, &bd);
    let first = memo_lines(&out).into_iter().next().unwrap_or_default();
    assert_eq!(
        first,
        format!("{MEMO_LINE} memo=s2-m.1 trigger=unmet verdict=promote age=5h judged=2026-09-30T01:02:03Z"),
        "{}",
        told(&out)
    );
    assert_eq!(memo_field(&out, "s2-m.2", "verdict="), "-", "置き場の無い memo");
    assert_eq!(memo_field(&out, "s2-m.2", "judged="), "-");
    assert_eq!(memo_field(&out, "s2-m.3", "age="), "-", "作られた時刻の無い memo");
    assert_eq!(memo_field(&out, "s2-m.4", "verdict="), "unreadable", "読めない判定: {}", told(&out));
    assert!(memo_field(&out, "s2-m.2", "age=").ends_with('h'), "作られた時刻の在る memo の age は時間");
    clean(&[&repo, &state]);
}

/// (g) 最後の昇格の行が「全部」の memo は行が無く、「一部」・読めない行・行の無い memo は在る（最後の行が勝つ）。
#[test]
fn pipe_dispatch_memo_trigger_population_drops_only_the_fully_promoted() {
    let (repo, state) = memo_repo();
    let unmet = ["引き金: 期日 2999-01-01T00:00Z"];
    let bd = fake_bd(
        &state,
        &[
            memo_bead("s2-m.1", &unmet, "昇格: 全部 s2-toy.1\n"),
            memo_bead("s2-m.2", &unmet, "昇格: 一部 s2-toy.1\n"),
            memo_bead("s2-m.3", &unmet, "昇格: 全部 s2-toy.1\n昇格: 一部 s2-toy.1\n"),
            memo_bead("s2-m.4", &unmet, "昇格: 全部 x-1\n"),
            memo_bead("s2-m.5", &unmet, ""),
        ],
    );
    let out = ls(&repo, &state, &bd);
    let ids: Vec<String> = ["s2-m.1", "s2-m.2", "s2-m.3", "s2-m.4", "s2-m.5"]
        .iter()
        .filter(|id| !memo_field(&out, id, "trigger=").is_empty())
        .map(|id| (*id).to_owned())
        .collect();
    assert_eq!(ids, ["s2-m.2", "s2-m.3", "s2-m.4", "s2-m.5"], "全部だけが母集団から外れる: {}", told(&out));
    clean(&[&repo, &state]);
}

/// (h) 読める引き金が 0 の memo は最初の読めない行の理由の語、引き金の行の無い memo は `unreadable:none`。読める行が 1 本でも在れば
/// 読めない行が在っても `unmet` になる。
#[test]
fn pipe_dispatch_memo_trigger_unreadable_names_the_first_reason_or_none() {
    let (repo, state) = memo_repo();
    let bd = fake_bd(
        &state,
        &[
            memo_bead("s2-m.1", &["引き金: 失敗 3"], ""),
            memo_bead("s2-m.2", &["引き金: 再発", "引き金: 失敗 3"], ""),
            memo_bead("s2-m.3", &[], ""),
            memo_bead("s2-m.4", &["引き金: 失敗 3", "引き金: 期日 2999-01-01T00:00Z"], ""),
            memo_bead("s2-m.5", &["引き金: 再発 0"], ""),
        ],
    );
    let out = ls(&repo, &state, &bd);
    assert_eq!(
        triggers_of(&out, &["s2-m.1", "s2-m.2", "s2-m.3", "s2-m.4", "s2-m.5"]),
        expected(&[
            ("s2-m.1", "unreadable:kind"),
            ("s2-m.2", "unreadable:words"),
            ("s2-m.3", "unreadable:none"),
            ("s2-m.4", "unmet"),
            ("s2-m.5", "unreadable:value"),
        ]),
        "{}",
        told(&out)
    );
    clean(&[&repo, &state]);
}

/// 台帳の子 process の呼びを 1 行ずつ記録して JSON を返す偽の `bd`。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn counting_bd(state: &Path, name: &str, issues: &[String]) -> (String, std::path::PathBuf) {
    let json = state.join(format!("{name}.json"));
    let log = state.join(format!("{name}.calls"));
    fs::write(&json, format!("[{}]\n", issues.join(","))).expect("偽の台帳を書ける");
    let bd = script(&state.join(name), &format!("echo x >> '{}'\ncat '{}'\n", log.display(), json.display()));
    (bd, log)
}

/// 偽の `bd` が記録した呼びの本数。
fn calls_of(log: &Path) -> usize {
    fs::read_to_string(log).map(|text| text.lines().count()).unwrap_or_default()
}

/// (i) 候補 0 の周は `[DISPATCH-NONE]` の直後、列の行の在る周は件数の行の後ろに、bead id の字の順（`s2-m.10` が `s2-m.9` の前）で
/// 出る。同じ台帳の手動の 1 周の stdout に memo の行は無い。memo の行を出す ls の偽の bd の呼びの本数は、memo の無い台帳の ls と等しい。
#[test]
fn pipe_dispatch_memo_trigger_lines_follow_the_queue_lines_in_id_order_and_read_the_ledger_once() {
    let (repo, state) = memo_repo();
    let unmet = ["引き金: 期日 2999-01-01T00:00Z"];
    let memos = [memo_bead("s2-m.9", &unmet, ""), memo_bead("s2-m.10", &unmet, "")];
    let only = fake_bd(&state, &memos);
    let none = ls(&repo, &state, &only);
    let shown: Vec<String> = stdout_of(&none).lines().map(|line| line.split(" trigger=").next().unwrap_or_default().to_owned()).collect();
    assert_eq!(
        shown,
        [NONE_LINE.to_owned(), format!("{MEMO_LINE} memo=s2-m.10"), format!("{MEMO_LINE} memo=s2-m.9")],
        "候補 0 の周は NONE の直後・id の字の順: {}",
        told(&none)
    );
    let mixed: Vec<String> = std::iter::once(issue("s2-toy.1", 2, "a")).chain(memos.iter().cloned()).collect();
    let bd = fake_bd(&state, &mixed);
    let listed_out = ls(&repo, &state, &bd);
    let lines: Vec<String> = stdout_of(&listed_out).lines().map(str::to_owned).collect();
    let count_at = lines.iter().position(|line| line.starts_with(COUNT));
    let first_memo = lines.iter().position(|line| line.starts_with(MEMO_LINE));
    assert!(count_at.is_some_and(|at| first_memo == Some(at + 1)), "件数の行の直後から memo の行: {}", told(&listed_out));
    assert_eq!(memo_lines(&listed_out).len(), 2, "{}", told(&listed_out));
    let manual = launch_turn(&repo, &state, &only, IMPLEMENT);
    assert_eq!(manual.status.code(), Some(i32::from(RC_OK)), "{}", told(&manual));
    assert!(!stdout_of(&manual).contains(MEMO_LINE), "手動の 1 周に memo の行は無い: {}", told(&manual));
    let contract = [issue("s2-toy.1", 2, "a")];
    let (bare, bare_log) = counting_bd(&state, "bd-bare", &contract);
    let (rich, rich_log) = counting_bd(&state, "bd-rich", &mixed);
    let _ = ls(&repo, &state, &bare);
    let with_memos = ls(&repo, &state, &rich);
    assert_eq!(memo_lines(&with_memos).len(), 2, "{}", told(&with_memos));
    assert!(calls_of(&bare_log) >= 1, "前提: ls は台帳を読む");
    assert_eq!(calls_of(&rich_log), calls_of(&bare_log), "memo の行を出しても台帳の呼びは増えない");
    clean(&[&repo, &state]);
}

/// (j) memo を close した後の周に、close の前の周に在った行が消える。
#[test]
fn pipe_dispatch_memo_trigger_line_disappears_after_the_memo_is_closed() {
    let (repo, state) = memo_repo();
    let unmet = ["引き金: 期日 2999-01-01T00:00Z"];
    let open = fake_bd(&state, &[memo_of("s2-m.1", "open", Some(MEMO_CREATED), &unmet, "")]);
    let before = ls(&repo, &state, &open);
    assert_eq!(memo_lines(&before).len(), 1, "close の前は行が在る: {}", told(&before));
    let closed = fake_bd(&state, &[memo_of("s2-m.1", "closed", Some(MEMO_CREATED), &unmet, "")]);
    let after = ls(&repo, &state, &closed);
    assert!(memo_lines(&after).is_empty(), "close の後は行が消える: {}", told(&after));
    clean(&[&repo, &state]);
}

/// (k) 台帳を読めない周は `[DISPATCH-UNMEASURED reason=ledger]` の 1 行だけ（memo の行を出さない）で、同じ置き場の読める周は行が在る。
#[test]
fn pipe_dispatch_memo_trigger_unmeasured_ledger_prints_no_memo_line() {
    let (repo, state) = memo_repo();
    let broken = script(&state.join("bd-broken"), "exit 1\n");
    let out = ls(&repo, &state, &broken);
    assert_eq!(stdout_of(&out).trim_end(), UNMEASURED, "読めない周の 1 行だけ: {}", told(&out));
    let bd = fake_bd(&state, &[memo_bead("s2-m.1", &["引き金: 期日 2999-01-01T00:00Z"], "")]);
    let read = ls(&repo, &state, &bd);
    assert_eq!(memo_lines(&read).len(), 1, "読める周は行が在る: {}", told(&read));
    clean(&[&repo, &state]);
}

// ───── memo の審査の裏の process（設計 dispatcher.md §41・契約表の行 ap・接頭辞 `pipe_dispatch_memo_lens_`） ─────

use super::super::ratelimit as lifecycle;

/// 偽の lens の置き場（呼びの回数・argv・stdin・返す出力を `name` ごとに持つ）。
fn memo_spy(state: &Path, name: &str) -> std::path::PathBuf {
    state.join("memo-lens-spy").join(name)
}

/// 偽の lens（argv と stdin を置き場へ記し、`out` を stdout へ返して `rc` で終わる shell）。返すのは `--lens` の値（穴つき）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn memo_lens_cmd(state: &Path, name: &str, out: &str, rc: u8) -> String {
    let spy = memo_spy(state, name);
    fs::create_dir_all(&spy).expect("偽 lens の置き場を作れる");
    fs::write(spy.join("out"), out).expect("偽 lens の出力を書ける");
    let body = format!(
        "printf 'call\\n' >> '{d}/calls'\nprintf '%s\\n' \"$@\" > '{d}/argv'\ncat > '{d}/stdin'\ncat '{d}/out'\nexit {rc}\n",
        d = spy.display()
    );
    let path = script(&state.join(format!("{name}.sh")), &body);
    format!("sh {path} --contract {{contract}} --worktree {{worktree}}")
}

/// 偽の lens が起こされた回数。
fn memo_lens_calls(state: &Path, name: &str) -> usize {
    calls_of(&memo_spy(state, name).join("calls"))
}

/// 偽の lens が受けた argv（1 行 1 語）。
fn memo_lens_argv(state: &Path, name: &str) -> Vec<String> {
    fs::read_to_string(memo_spy(state, name).join("argv")).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// `pipe dispatch memo-lens <memo>` を 1 回撃つ（`extra` は `--curl` などの足し）。
fn memo_lens(repo: &Path, state: &Path, bd: &str, memo: &str, tools: (&str, &str)) -> Output {
    memo_lens_with(repo, state, (bd, memo), tools, &[])
}

/// [`memo_lens`] に `--curl` などの足しを渡す形。
fn memo_lens_with(repo: &Path, state: &Path, (bd, memo): (&str, &str), (lens, rules): (&str, &str), extra: &[&str]) -> Output {
    let (state_arg, repo_arg) = (state.display().to_string(), repo.display().to_string());
    let mut args = vec![
        "dispatch", "memo-lens", memo,
        "--state-dir", &state_arg,
        "--repo", &repo_arg,
        "--rules", rules,
        "--bd", bd,
        "--lens", lens,
    ];
    args.extend(extra);
    run_pipe(&args)
}

/// memo の置き場の dir。
fn memo_place(state: &Path, memo: &str) -> std::path::PathBuf {
    state.join("pipe").join("memo").join(memo)
}

/// 置き場の file の中身（無ければ空）。
fn place_file(state: &Path, memo: &str, name: &str) -> String {
    fs::read_to_string(memo_place(state, memo).join(name)).unwrap_or_default()
}

/// JSON の 1 object の `(key, 文字列の値)` の列（文字列でない値は空）。
fn string_pairs(text: &str) -> Vec<(String, String)> {
    json_lite::parse_object(text.trim())
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| (key, value.as_str().unwrap_or_default().to_owned()))
        .collect()
}

/// `pairs` の `key` の値（無ければ空）。
fn pair_of(pairs: &[(String, String)], key: &str) -> String {
    pairs.iter().find(|(found, _)| found == key).map(|(_, value)| value.clone()).unwrap_or_default()
}

/// event log の `MemoJudged` の行（log の順）。
fn judged_lines(state: &Path) -> Vec<String> {
    fs::read_to_string(state.join("fleet").join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("\"kind\":\"MemoJudged\""))
        .map(str::to_owned)
        .collect()
}

/// 引き金が満ちない開いた memo（id の列）の台帳。
fn unmet_memos(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| memo_bead(id, &["引き金: 期日 2999-01-01T00:00Z"], "")).collect()
}

/// (a) 最後の行に promote の JSON を返す lens（前の行に close の JSON を置く＝最後の行を読まない実装を落とす）の周は、`verdict` の
/// file が promote・`at`・evidence・sketch を持ち、`MemoJudged` が 1 行（bead が memo の id・detail が promote・本体の key は bead だけ）、
/// stdout が `memo-lens memo=<id> verdict=promote` の 1 行で、`pid` が消えて `rc` が在る。keep を返す周の `verdict` の語は keep で sketch を
/// 持たず、`scribe2 pipe` の使い方の行の最初の `<…>` の verb の列に memo-lens は無い。base は `memo-lens` が使い方の誤りで RED（機能不在）。
#[test]
fn pipe_dispatch_memo_lens_writes_the_last_json_line_as_the_verdict_and_one_event() {
    let (repo, state) = memo_repo();
    let bd = fake_bd(&state, &unmet_memos(&["s2-m.1", "s2-m.2"]));
    let rules = dispatch_rules(&state);
    let promote = "前置き\n{\"verdict\":\"close\",\"evidence\":\"前の行\",\"sketch\":\"\"}\n{\"verdict\":\"promote\",\"evidence\":\"ev-zq\",\"sketch\":\"sk-zq\"}\n";
    let lens = memo_lens_cmd(&state, "promote", promote, 0);
    let out = memo_lens(&repo, &state, &bd, "s2-m.1", (&lens, &rules));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", told(&out));
    assert_eq!(stdout_of(&out), "memo-lens memo=s2-m.1 verdict=promote\n", "{}", told(&out));
    let verdict = string_pairs(&place_file(&state, "s2-m.1", "verdict"));
    assert_eq!(pair_of(&verdict, "verdict"), "promote", "{verdict:?}");
    assert!(pair_of(&verdict, "at").ends_with('Z'), "判定の時刻: {verdict:?}");
    assert_eq!((pair_of(&verdict, "evidence"), pair_of(&verdict, "sketch")), ("ev-zq".to_owned(), "sk-zq".to_owned()), "{verdict:?}");
    let events = judged_lines(&state);
    assert_eq!(events.len(), 1, "MemoJudged は 1 行: {events:?}");
    let event = string_pairs(events.first().map_or("", String::as_str));
    assert_eq!((pair_of(&event, "bead"), pair_of(&event, "detail")), ("s2-m.1".to_owned(), "promote".to_owned()), "{event:?}");
    let keys: std::collections::BTreeSet<&str> = event.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, ["actor", "bead", "detail", "host", "kind", "schema", "ts"].into_iter().collect(), "本体の key は bead だけ: {event:?}");
    assert_eq!(pair_of(&event, "actor"), "machine", "{event:?}");
    assert!(!memo_place(&state, "s2-m.1").join("pid").exists(), "pid は消える");
    assert_eq!(place_file(&state, "s2-m.1", "rc").trim(), "0", "rc が在る");
    assert!(place_file(&state, "s2-m.1", "out").contains("ev-zq"), "lens の出力は out に残る");
    let keep = memo_lens_cmd(&state, "keep", "{\"verdict\":\"keep\",\"evidence\":\"ev-keep\",\"sketch\":\"捨てる\"}\n", 0);
    let kept = memo_lens(&repo, &state, &bd, "s2-m.2", (&keep, &rules));
    assert_eq!(stdout_of(&kept), "memo-lens memo=s2-m.2 verdict=keep\n", "{}", told(&kept));
    let verdict = string_pairs(&place_file(&state, "s2-m.2", "verdict"));
    assert_eq!((pair_of(&verdict, "verdict"), pair_of(&verdict, "sketch")), ("keep".to_owned(), String::new()), "keep は sketch を持たない: {verdict:?}");
    assert_memo_lens_is_a_dispatch_word_not_a_verb();
    clean(&[&repo, &state]);
}

/// `scribe2 pipe` の使い方の行の最初の `<…>` の verb の列に memo-lens は無く、`dispatch:` の副の語の列には在る。
fn assert_memo_lens_is_a_dispatch_word_not_a_verb() {
    let usage = run_pipe(&[]);
    let first = stderr_of(&usage).lines().next().unwrap_or_default().to_owned();
    let verbs = first.split_once('<').and_then(|(_, rest)| rest.split_once('>')).map(|(found, _)| found.to_owned()).unwrap_or_default();
    assert!(!verbs.contains("memo-lens") && verbs.contains("dispatch"), "verb の列に memo-lens は無い: {first}");
    assert!(first.contains("release BEAD|memo-lens MEMO]"), "dispatch の副の語の列には在る: {first}");
}

/// (b) JSON の無い出力・語の外の JSON・lens の rc 1 の 3 形は、どれも `unparsed`（verdict の file と stdout と event の detail）。同じ歯の
/// promote の周（rc 0）は promote で、3 形は JSON の良し悪しでなく形の読みで分かれる。
#[test]
fn pipe_dispatch_memo_lens_unreadable_output_is_unparsed_in_three_shapes() {
    let (repo, state) = memo_repo();
    let bd = fake_bd(&state, &unmet_memos(&["s2-m.1", "s2-m.2", "s2-m.3", "s2-m.4"]));
    let rules = dispatch_rules(&state);
    let good = "{\"verdict\":\"promote\",\"evidence\":\"e\",\"sketch\":\"s\"}\n";
    let shapes = [
        ("s2-m.1", "no-json", "JSON の無い出力\n".to_owned(), 0, "unparsed"),
        ("s2-m.2", "word", "{\"verdict\":\"maybe\",\"evidence\":\"e\"}\n".to_owned(), 0, "unparsed"),
        ("s2-m.3", "rc1", good.to_owned(), 1, "unparsed"),
        ("s2-m.4", "pair", good.to_owned(), 0, "promote"),
    ];
    for (memo, name, output, rc, word) in shapes {
        let lens = memo_lens_cmd(&state, name, &output, rc);
        let out = memo_lens(&repo, &state, &bd, memo, (&lens, &rules));
        assert_eq!(stdout_of(&out), format!("memo-lens memo={memo} verdict={word}\n"), "{name}: {}", told(&out));
        let verdict = string_pairs(&place_file(&state, memo, "verdict"));
        assert_eq!(pair_of(&verdict, "verdict"), word, "{name}: {verdict:?}");
        assert!(word == "promote" || !pair_of(&verdict, "evidence").is_empty(), "{name}: unparsed は理由を evidence に書く: {verdict:?}");
        assert_eq!(place_file(&state, memo, "rc").trim(), rc.to_string(), "{name}: lens の rc");
    }
    let details: Vec<String> = judged_lines(&state).iter().map(|line| pair_of(&string_pairs(line), "detail")).collect();
    assert_eq!(details, ["unparsed", "unparsed", "unparsed", "promote"], "event の detail は判定の語");
    clean(&[&repo, &state]);
}

/// (c) 偽の bd の呼びは台帳の読みの 1 回（`--readonly` つき）だけで書きが 0 回。lens が close を返した周も `verdict` の語は close で、台帳の
/// 内容は変わらず memo は開いたまま。
#[test]
fn pipe_dispatch_memo_lens_reads_the_ledger_once_and_never_writes_it() {
    let (repo, state) = memo_repo();
    let json = state.join("ledger.json");
    fs::write(&json, format!("[{}]\n", unmet_memos(&["s2-m.1"]).join(","))).expect("偽の台帳を書ける");
    let before = fs::read_to_string(&json).unwrap_or_default();
    let log = state.join("bd-logged.calls");
    let bd = script(&state.join("bd-logged"), &format!("echo \"$@\" >> '{}'\ncat '{}'\n", log.display(), json.display()));
    let rules = dispatch_rules(&state);
    let lens = memo_lens_cmd(&state, "close", "{\"verdict\":\"close\",\"evidence\":\"もう要らない\",\"sketch\":\"\"}\n", 0);
    let out = memo_lens(&repo, &state, &bd, "s2-m.1", (&lens, &rules));
    assert_eq!(stdout_of(&out), "memo-lens memo=s2-m.1 verdict=close\n", "{}", told(&out));
    assert_eq!(pair_of(&string_pairs(&place_file(&state, "s2-m.1", "verdict")), "verdict"), "close");
    let calls: Vec<String> = fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_owned).collect();
    assert_eq!(calls.len(), 1, "台帳の呼びは読みの 1 回だけ: {calls:?}");
    assert!(calls.iter().all(|call| call.starts_with("--readonly ")), "書きの呼びは 0: {calls:?}");
    assert_eq!(fs::read_to_string(&json).unwrap_or_default(), before, "台帳は変わらない");
    assert!(fs::read_to_string(&json).unwrap_or_default().contains("\"status\":\"open\""), "memo は閉じない");
    clean(&[&repo, &state]);
}

/// discovered-from の依存（`from` が `on` を指す）を台帳の 1 件の JSON に足す。
fn discovered_from(json: &str, from: &str, on: &str) -> String {
    let dep = format!(",\"dependencies\":[{{\"issue_id\":\"{from}\",\"depends_on_id\":\"{on}\",\"type\":\"discovered-from\"}}]}}");
    json.strip_suffix('}').map_or_else(|| json.to_owned(), |head| format!("{head}{dep}"))
}

/// (d) 偽の lens が受けた argv の末尾は `--stage memo` で、`--contract` が置き場の material を・`--worktree` が repo を指し、material が
/// memo の description・notes・引き金の読み・辿れる契約の status（memo から出る向きも memo へ入る向きも）を持ち、辿れない契約は持たない。
#[test]
fn pipe_dispatch_memo_lens_argv_ends_with_stage_memo_and_material_carries_the_memo() {
    let (repo, state) = memo_repo();
    let memo = discovered_from(&memo_bead("s2-m.1", &["引き金: 再発 2"], "zq-notes-body\n[再発] 2026-09-28 a\n[再発] 2026-09-29 b\n"), "s2-m.1", "s2-c.1");
    let reverse = listed("s2-c.3", "open", 2, "", &[("s2-m.1", "discovered-from")]);
    let bd = fake_bd(&state, &[memo, listed("s2-c.1", "closed", 2, "", &[]), listed("s2-c.2", "open", 2, "", &[]), reverse]);
    let rules = dispatch_rules(&state);
    let lens = memo_lens_cmd(&state, "argv", "{\"verdict\":\"keep\",\"evidence\":\"e\",\"sketch\":\"\"}\n", 0);
    let out = memo_lens(&repo, &state, &bd, "s2-m.1", (&lens, &rules));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", told(&out));
    let argv = memo_lens_argv(&state, "argv");
    assert_eq!(argv.iter().rev().take(2).rev().cloned().collect::<Vec<_>>(), ["--stage", "memo"], "末尾は --stage memo: {argv:?}");
    let material = memo_place(&state, "s2-m.1").join("material").display().to_string();
    let value = |flag: &str| argv.windows(2).find(|pair| pair.first().is_some_and(|found| found == flag)).and_then(|pair| pair.get(1).cloned());
    assert_eq!(value("--contract"), Some(material.clone()), "--contract は置き場の material: {argv:?}");
    assert_eq!(value("--worktree"), Some(repo.display().to_string()), "--worktree は repo: {argv:?}");
    let text = fs::read_to_string(&material).unwrap_or_default();
    for want in ["### 昇格条件", "引き金: 再発 2", "zq-notes-body", "met:再発", "- s2-c.1 status=closed", "- s2-c.3 status=open"] {
        assert!(text.contains(want), "material は {want} を持つ: {text}");
    }
    assert!(!text.contains("s2-c.2"), "辿れない契約は持たない: {text}");
    clean(&[&repo, &state]);
}

/// (e) 生きた持ち主の `pid` が在る周は lens を撃たず rc 1（`pid` は持ち主のまま・`rc` も `verdict` も書かない）・死んだ持ち主の `pid` の
/// 周は撃って `pid` を消す。
#[test]
fn pipe_dispatch_memo_lens_live_owner_blocks_and_dead_owner_fires() {
    let (repo, state) = memo_repo();
    let bd = fake_bd(&state, &unmet_memos(&["s2-m.1"]));
    let rules = dispatch_rules(&state);
    let lens = memo_lens_cmd(&state, "owner", "{\"verdict\":\"keep\",\"evidence\":\"e\",\"sketch\":\"\"}\n", 0);
    let mut owner = Command::new("sleep").arg("30").spawn().expect("持ち主の process を起こせる");
    fs::create_dir_all(memo_place(&state, "s2-m.1")).expect("置き場を作れる");
    let pid_file = memo_place(&state, "s2-m.1").join("pid");
    fs::write(&pid_file, format!("{}\n", owner.id())).expect("pid を置ける");
    let blocked = memo_lens(&repo, &state, &bd, "s2-m.1", (&lens, &rules));
    assert_eq!(blocked.status.code(), Some(1), "生きた持ち主が在れば rc 1: {}", told(&blocked));
    assert_eq!(memo_lens_calls(&state, "owner"), 0, "lens は撃たない");
    assert_eq!(fs::read_to_string(&pid_file).unwrap_or_default().trim(), owner.id().to_string(), "pid は持ち主のまま");
    assert!(place_file(&state, "s2-m.1", "rc").is_empty() && place_file(&state, "s2-m.1", "verdict").is_empty(), "rc も verdict も書かない");
    owner.kill().expect("持ち主を止められる");
    owner.wait().expect("持ち主を回収できる");
    let fired = memo_lens(&repo, &state, &bd, "s2-m.1", (&lens, &rules));
    assert_eq!(fired.status.code(), Some(i32::from(RC_OK)), "死んだ持ち主の周は撃つ: {}", told(&fired));
    assert_eq!(memo_lens_calls(&state, "owner"), 1, "lens を撃つ");
    assert!(!pid_file.exists(), "終わりに pid を消す");
    clean(&[&repo, &state]);
}

/// 口座を宣言した manifest（列の写しに計測の行・鮮度の行・便の model の行と `[[account]]` を足す。`measured` が偽なら計測の待ち時間の行を
/// 落とす＝計測が撃てない置き場）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn account_rules(state: &Path, labels: &[&str], measured: bool) -> String {
    let mut body = fs::read_to_string(dispatch_rules(state)).expect("列の写しを読める");
    let row = |id: &str, kind: &str, value: &str| format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n");
    if measured {
        body.push_str(&row("fleet.usage_timeout_s", "UsageTimeoutS", "13"));
    }
    body.push_str(&row("fleet.usage_fresh_s", "UsageFreshS", "0"));
    body.push_str(&row("runner.model", "RunnerModel", "\"opus\""));
    for label in labels {
        body.push_str(&format!("\n[[account]]\nlabel = \"{label}\"\n"));
    }
    let path = state.join("rules-memo-accounts.toml");
    fs::write(&path, body).expect("口座つきの写しを書ける");
    path.display().to_string()
}

/// (f) 宣言した口座の計測が落ちる置き場は lens を撃たず `rc` が `account-unmeasured`・宣言した口座が全部便用の規則の外（当たっている）の
/// 置き場は `rc` が `account-none` で、どちらも `verdict` も event も書かない。同じ歯の口座を宣言しない置き場は撃ち（`--account-dir` 無し）、
/// 便用の規則の内で余裕の在る口座を宣言した置き場は偽 lens の argv の `--account-dir` が選んだ口座の dir を指し、`--stage memo` が末尾に残る。
#[test]
fn pipe_dispatch_memo_lens_chooses_the_account_or_stops_without_a_verdict() {
    let (repo, state) = memo_repo();
    let bd = fake_bd(&state, &unmet_memos(&["s2-m.1", "s2-m.2", "s2-m.3", "s2-m.4"]));
    let ok = "{\"verdict\":\"keep\",\"evidence\":\"e\",\"sketch\":\"\"}\n";
    let curl = lifecycle::fake_usage_curl(&state);
    lifecycle::put_account(&state, "a1", &[lifecycle::windows(100, 10)]);
    lifecycle::put_account(&state, "a2", &[lifecycle::windows(100, 10)]);
    let unmeasured = memo_lens_cmd(&state, "unmeasured", ok, 0);
    let out = memo_lens_with(&repo, &state, (&bd, "s2-m.1"), (&unmeasured, &account_rules(&state, &["a1"], false)), &["--curl", &curl]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "撃たない周は成功でない: {}", told(&out));
    assert_eq!(place_file(&state, "s2-m.1", "rc").trim(), "account-unmeasured", "{}", told(&out));
    let none = memo_lens_cmd(&state, "none", ok, 0);
    let out = memo_lens_with(&repo, &state, (&bd, "s2-m.2"), (&none, &account_rules(&state, &["a1", "a2"], true)), &["--curl", &curl]);
    assert_eq!(place_file(&state, "s2-m.2", "rc").trim(), "account-none", "{}", told(&out));
    for (memo, name) in [("s2-m.1", "unmeasured"), ("s2-m.2", "none")] {
        assert_eq!(memo_lens_calls(&state, name), 0, "{name}: lens を撃たない");
        assert!(place_file(&state, memo, "verdict").is_empty(), "{name}: verdict を書かない");
        assert!(!memo_place(&state, memo).join("pid").exists(), "{name}: pid は消える");
    }
    assert!(judged_lines(&state).is_empty(), "判定の無い周は event を書かない");
    let plain = memo_lens_cmd(&state, "plain", ok, 0);
    let out = memo_lens(&repo, &state, &bd, "s2-m.3", (&plain, &dispatch_rules(&state)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "口座を宣言しない置き場は撃つ: {}", told(&out));
    assert_eq!(memo_lens_calls(&state, "plain"), 1);
    assert_eq!(lifecycle::argv_account_dir(&memo_lens_argv(&state, "plain")), None, "宣言 0 は起動行を変えない");
    lifecycle::put_account(&state, "b1", &[lifecycle::windows(100, 10)]);
    lifecycle::put_account(&state, "b2", &[lifecycle::windows(40, 10)]);
    let chosen = memo_lens_cmd(&state, "chosen", ok, 0);
    let out = memo_lens_with(&repo, &state, (&bd, "s2-m.4"), (&chosen, &account_rules(&state, &["b1", "b2"], true)), &["--curl", &curl]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", told(&out));
    let argv = memo_lens_argv(&state, "chosen");
    assert_eq!(lifecycle::argv_account_dir(&argv), Some(state.join("accounts").join("b2").display().to_string()), "選んだ口座の dir: {argv:?}");
    assert_eq!(argv.iter().rev().take(2).rev().cloned().collect::<Vec<_>>(), ["--stage", "memo"], "末尾は --stage memo: {argv:?}");
    clean(&[&repo, &state]);
}

// ---- 行の予約（設計 row-review.md §7・契約表の行 f・接頭辞 `pipe_dispatch_row_reservation_`） ----

/// 落ちた契約 B（P1・行 a）・後ろの C（P2・行 c）・前の D（P0・行 d）の bead（3 行とも同じ file `src/lib.rs` を書く）。
const FALLEN: &str = "s2-toy.1";
const LATER: &str = "s2-toy.2";
const EARLIER: &str = "s2-toy.3";

/// 3 行（a・c・d）が同じ `src/lib.rs` を書く設計 doc を commit する。B の行 a は節 2・後ろの C と前の D の行は節 1 に置く
/// （同じ節の行は兄弟の待ちで待つので、行の予約の歯は B の行を別の節に置いて兄弟の待ちを外す・設計 row-review.md §8）。
fn reserve_rows(repo: &Path) {
    commit_sections(repo, &[("a", "2"), ("c", "1"), ("d", "1")].map(|(id, section)| section_row(id, section, "src/lib.rs")));
}

/// 列の写し（[`dispatch_rules`]）に `pipe.reserve_h` の行を足した写し（`None` は行を持たない写し）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn reserve_rules(state: &Path, hours: Option<u64>) -> String {
    let base = fs::read_to_string(dispatch_rules(state)).expect("列の写しを読める");
    let row = hours.map_or_else(String::new, |value| {
        format!("\n[[rule]]\nid = \"pipe.reserve_h\"\nkind = \"PipeReserveH\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n")
    });
    let path = state.join(format!("rules-reserve-{}.toml", hours.map_or("none".to_owned(), |value| value.to_string())));
    fs::write(&path, format!("{base}{row}")).expect("写しを書ける");
    path.display().to_string()
}

/// event log の行数（読めない周は 0）。
fn event_lines(state: &Path) -> usize {
    fs::read_to_string(state.join("fleet").join("events.jsonl")).map_or(0, |text| text.lines().count())
}

/// `rules` の写しで `dispatch ls` を `rounds` 周撃ち、**各周の前後で event log の行数が同じ**（行の予約は記帳しない）ことを測って最後の周を返す。
fn reserve_ls(repo: &Path, state: &Path, bd: &str, rules: &str, rounds: usize) -> Output {
    let args = ["dispatch", "ls", "--state-dir", &state.display().to_string(), "--repo", &repo.display().to_string(), "--rules", rules, "--bd", bd];
    let mut last = run_pipe(&args);
    for round in 1..=rounds {
        let before = event_lines(state);
        last = run_pipe(&args);
        let held = stdout_of(&last).lines().filter(|line| line.contains("reason=reserved:")).count();
        assert_eq!(event_lines(state), before, "{rounds} 周中 {round} 周目・待った候補 {held} 本: 行の予約は event を増やさない");
    }
    last
}

/// 便 `id` の event の ts を `secs` 秒前へ書き換える（終端の古さを作る・event の行数と並びは変えない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn age_run(state: &Path, id: &str, secs: u64) {
    let path = state.join("fleet").join("events.jsonl");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |found| found.as_secs());
    let ts = vessel::fleet::cli::format_utc(now.saturating_sub(secs));
    let key = "\"ts\":\"";
    let retime = |line: &str| -> String {
        match line.split_once(key).and_then(|(head, rest)| Some((head, rest.split_once('"')?.1))) {
            Some((head, tail)) if line.contains(&format!("\"run\":\"{id}\"")) => format!("{head}{key}{ts}\"{tail}"),
            _ => line.to_owned(),
        }
    };
    let text = fs::read_to_string(&path).expect("event log を読める");
    let aged: Vec<String> = text.lines().map(retime).collect();
    fs::write(&path, format!("{}\n", aged.join("\n"))).expect("event log を書ける");
}

/// 行 a の便を終端の形 `form` に着ける（Reviewed の判定が PASS でない 2 形・Gated FAIL・Failed・Stopped と、予約を持たない Landed）。
#[expect(
    clippy::panic,
    reason = "統合 test の helper。clippy の allow-panic-in-tests は #[test] 関数の中だけに効く"
)]
fn reserve_fallen(repo: &Path, state: &Path, form: &str) -> String {
    match form {
        "reviewed-fail" => judged_run(repo, state, FALLEN, "{\"verdict\":\"FAIL\"}"),
        "reviewed-inconclusive" => judged_run(repo, state, FALLEN, "{\"verdict\":\"INCONCLUSIVE\",\"kind\":\"section-material-missing\"}"),
        "gated-fail" => gated_run(repo, state, FALLEN, "FAIL"),
        "failed" => failed_run(repo, state, FALLEN),
        "stopped" => {
            let id = intake_bead(repo, state, &format!("{DESIGN_FILE}#a"), FALLEN);
            super::super::stop_run_ok(state, &id);
            id
        }
        "landed" => {
            let id = gated_run(repo, state, FALLEN, "PASS");
            let landed = super::super::land_once(repo, state, &id);
            assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land は rc 0（{}）", told(&landed));
            id
        }
        other => panic!("知らない形: {other}"),
    }
}

/// 落ちた B（形 `form`）と後ろの C だけの台帳の置き場（repo・state・B の直前の便・台帳 client）。
fn reserve_place(form: &str) -> (std::path::PathBuf, std::path::PathBuf, String, String) {
    let (repo, state) = repo_with_state();
    reserve_rows(&repo);
    let run = reserve_fallen(&repo, &state, form);
    let bd = fake_bd(&state, &[issue(FALLEN, 1, "a"), issue(LATER, 2, "c")]);
    (repo, state, run, bd)
}

/// (a) 直前の便が Reviewed FAIL の B と同じ file を触る後ろの C（P2）は `reserved:<B>/<本数>` で待ち、B より前の D（P0）は同じ file を触っても起こされる。
/// 判定は event を増やさない（3 周）。
#[test]
fn pipe_dispatch_row_reservation_holds_a_later_crossing_candidate_and_wakes_an_earlier_one() {
    let (repo, state) = repo_with_state();
    reserve_rows(&repo);
    reserve_fallen(&repo, &state, "reviewed-fail");
    let bd = fake_bd(&state, &[issue(FALLEN, 1, "a"), issue(LATER, 2, "c"), issue(EARLIER, 0, "d")]);
    let out = reserve_ls(&repo, &state, &bd, &reserve_rules(&state, Some(24)), 3);
    assert_eq!(reason_of(&out, LATER), format!("reserved:{FALLEN}/1"), "後ろの C は B の予約で待つ（{}）", told(&out));
    assert_eq!(reason_of(&out, EARLIER), "-", "B より前の D は待たない（{}）", told(&out));
    assert!(reason_of(&out, FALLEN).starts_with("settled:"), "B 自身は列外のまま（{}）", told(&out));
    assert_eq!(count_of(&out), format!("{COUNT} total=3 ready=1"), "起こせるのは D だけ");
    clean(&[&repo, &state]);
}

/// (b) 終端 4 形（Reviewed の判定が PASS でない・Gated FAIL・Failed・Stopped）の B はどれも予約を持ち、直前の便が Landed の B は持たない（5 形・Reviewed は 2 判定）。
#[test]
fn pipe_dispatch_row_reservation_belongs_to_the_terminal_forms_and_not_to_a_landed_run() {
    for (form, held) in [("reviewed-fail", true), ("reviewed-inconclusive", true), ("gated-fail", true), ("failed", true), ("stopped", true), ("landed", false)] {
        let (repo, state, _, bd) = reserve_place(form);
        let out = reserve_ls(&repo, &state, &bd, &reserve_rules(&state, Some(24)), 1);
        let want = if held { format!("reserved:{FALLEN}/1") } else { "-".to_owned() };
        assert_eq!(reason_of(&out, LATER), want, "{form}: C の理由（{}）", told(&out));
        clean(&[&repo, &state]);
    }
}

/// (c) B の契約を変えた周は B が C より先に起こされ（起こした記録は B だけ・C は reserved のまま）、B の新しい便の後の周は C の reserved が解ける。
#[test]
fn pipe_dispatch_row_reservation_wakes_the_fixed_bead_before_the_held_one() {
    let (repo, state, _, bd) = reserve_place("reviewed-fail");
    let widened = r#"write-set = ["src/lib.rs", "src/extra.rs"]"#;
    commit_rows(&repo, &[row_fields("a", &["write-set"], &[widened]), row_fields("c", &["write-set"], &[r#"write-set = ["src/lib.rs"]"#])]);
    let listed = reserve_ls(&repo, &state, &bd, &reserve_rules(&state, Some(24)), 1);
    assert_eq!(reason_of(&listed, FALLEN), "-", "契約を変えた B は列に戻る（{}）", told(&listed));
    assert_eq!(reason_of(&listed, LATER), format!("reserved:{FALLEN}/1"), "B が新しい便を起こすまで C は待つ（{}）", told(&listed));
    // run id は `<bead>-<秒>`: 前の便と同じ秒に起こすと id が衝突する。
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let fired = launch_turn(&repo, &state, &bd, "true");
    assert_eq!(stdout_of(&fired).trim_end(), "dispatch=started:1,resumed:0,waiting:1", "B だけ起こす（{}）", told(&fired));
    assert_eq!(created(&state, &[FALLEN], 2), 2, "B の新しい便の RunCreated（前の便と合わせて 2 件）");
    assert_eq!(created(&state, &[LATER], 0), 0, "C は起こされない");
    let mark = |bead: &str| line_at(&state, &[LAUNCHED_MARK[0], LAUNCHED_MARK[1], &format!("\"bead\":\"{bead}\"")]);
    assert!(mark(FALLEN).is_some() && mark(LATER).is_none(), "起こした記録は B だけ（B={:?} C={:?}）", mark(FALLEN), mark(LATER));
    clean(&[&repo, &state]);

    // 新しい便の RunCreated の後の周: C は reserved でなく、B の live な便との交差で待つ。
    let (repo, state, _, bd) = reserve_place("reviewed-fail");
    let rules = reserve_rules(&state, Some(24));
    let held = reserve_ls(&repo, &state, &bd, &rules, 1);
    assert_eq!(reason_of(&held, LATER), format!("reserved:{FALLEN}/1"), "前提: 新しい便の前は reserved（{}）", told(&held));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let live = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), FALLEN);
    let after = reserve_ls(&repo, &state, &bd, &rules, 1);
    assert_eq!(reason_of(&after, LATER), format!("overlap:{live}/1"), "B の新しい便の後は reserved が解ける（{}）", told(&after));
    clean(&[&repo, &state]);
}

/// 台帳の B（P1・行 a）に label を 1 つ付けた 1 件。
fn labeled_fallen(label: &str) -> String {
    issue(FALLEN, 1, "a").replacen("\"labels\":[]", &format!("\"labels\":[\"{label}\"]"), 1)
}

/// (d) B への hold の印・期限（値 1 の写しで 2 時間前の終端）・B の close・B の memo の label・B の問いの label のそれぞれで C の reserved が解ける（5 形）。
#[test]
fn pipe_dispatch_row_reservation_ends_by_hold_expiry_close_or_a_memo_or_question_label() {
    let acceptance = format!("design = {DESIGN_FILE}#a");
    for form in ["hold", "expiry", "close", "memo", "question"] {
        let (repo, state, run, bd) = reserve_place("reviewed-fail");
        let held = reserve_ls(&repo, &state, &bd, &reserve_rules(&state, Some(24)), 1);
        assert_eq!(reason_of(&held, LATER), format!("reserved:{FALLEN}/1"), "{form}: 前提は予約が掛かる（{}）", told(&held));
        let (rules, bd) = match form {
            "hold" => {
                let out = run_pipe(&["dispatch", "hold", FALLEN, "--state-dir", &state.display().to_string()]);
                assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "hold: {}", stderr_of(&out));
                (reserve_rules(&state, Some(24)), bd)
            }
            "expiry" => {
                age_run(&state, &run, 2 * 3600);
                (reserve_rules(&state, Some(1)), bd)
            }
            "close" => (reserve_rules(&state, Some(24)), fake_bd(&state, &[listed(FALLEN, "closed", 1, &acceptance, &[]), issue(LATER, 2, "c")])),
            label => {
                let word = if label == "memo" { "intake:memo" } else { "intake:question" };
                (reserve_rules(&state, Some(24)), fake_bd(&state, &[labeled_fallen(word), issue(LATER, 2, "c")]))
            }
        };
        let out = reserve_ls(&repo, &state, &bd, &rules, 1);
        assert_eq!(reason_of(&out, LATER), "-", "{form}: C の予約が解ける（{}）", told(&out));
        clean(&[&repo, &state]);
    }
}

/// (e) B が C を台帳の blocks で持つ周（C は B の祖先）に C は reserved で待たず起こされる（B が依存で待ち C が予約で待つ輪を作らない）。
#[test]
fn pipe_dispatch_row_reservation_spares_the_blocks_ancestor_of_the_fallen_bead() {
    let (repo, state, _, bd) = reserve_place("reviewed-fail");
    let rules = reserve_rules(&state, Some(24));
    let plain = reserve_ls(&repo, &state, &bd, &rules, 1);
    assert_eq!(reason_of(&plain, LATER), format!("reserved:{FALLEN}/1"), "対照: blocks の無い周は待つ（{}）", told(&plain));
    let acceptance = format!("design = {DESIGN_FILE}#a");
    let bd = fake_bd(&state, &[listed(FALLEN, "open", 1, &acceptance, &[(LATER, "blocks")]), issue(LATER, 2, "c")]);
    let out = reserve_ls(&repo, &state, &bd, &rules, 1);
    assert_eq!(reason_of(&out, FALLEN), format!("dependency:{LATER}"), "B は C を待つ（{}）", told(&out));
    assert_eq!(reason_of(&out, LATER), "-", "B の祖先の C は待たず起こされる（{}）", told(&out));
    clean(&[&repo, &state]);
}

/// (f) 期限の行の無い rules の写しは期限なしで予約を掛けて理由の値の末尾が `/unset` になり、値 0 の写しは 100 時間前の終端でも予約を掛ける（2 形）。
#[test]
fn pipe_dispatch_row_reservation_without_the_deadline_row_is_unset_and_zero_never_expires() {
    let (repo, state, run, bd) = reserve_place("reviewed-fail");
    age_run(&state, &run, 100 * 3600);
    let unset = reserve_ls(&repo, &state, &bd, &reserve_rules(&state, None), 1);
    assert_eq!(reason_of(&unset, LATER), format!("reserved:{FALLEN}/1/unset"), "行の無い写しは期限なし・末尾 /unset（{}）", told(&unset));
    let zero = reserve_ls(&repo, &state, &bd, &reserve_rules(&state, Some(0)), 1);
    assert_eq!(reason_of(&zero, LATER), format!("reserved:{FALLEN}/1"), "値 0 は期限なし（{}）", told(&zero));
    let expired = reserve_ls(&repo, &state, &bd, &reserve_rules(&state, Some(24)), 1);
    assert_eq!(reason_of(&expired, LATER), "-", "対照: 値 24 は 100 時間前の終端の予約を手放す（{}）", told(&expired));
    clean(&[&repo, &state]);
}

/// (g) B が列で依存待ちの周（B の理由は `dependency`）も C は reserved で待つ。
#[test]
fn pipe_dispatch_row_reservation_holds_while_the_fallen_bead_waits_on_a_dependency() {
    let (repo, state, _, _) = reserve_place("reviewed-fail");
    let acceptance = format!("design = {DESIGN_FILE}#a");
    let waiting = listed(FALLEN, "open", 1, &acceptance, &[("s2-dep", "blocks")]);
    let bd = fake_bd(&state, &[waiting, listed("s2-dep", "open", 2, "memo", &[]), issue(LATER, 2, "c")]);
    let out = reserve_ls(&repo, &state, &bd, &reserve_rules(&state, Some(24)), 1);
    assert_eq!(reason_of(&out, FALLEN), "dependency:s2-dep", "B は依存で待つ（{}）", told(&out));
    assert_eq!(reason_of(&out, LATER), format!("reserved:{FALLEN}/1"), "候補に居ない B の予約でも C は待つ（{}）", told(&out));
    clean(&[&repo, &state]);
}

// ---- 兄弟の待ち（設計 row-review.md §8・契約表の行 g・接頭辞 `pipe_dispatch_sibling_wait_`） ----
//
// 行 a の便（B・bead は [`FALLEN`]）が設計の側の終端に着いた置き場で、同じ設計 doc の行を持つ候補（bead は [`sib`]）を `dispatch ls` で測る。
// doc は節 1 と節 2 を持つ（[`commit_sections`]）。兄弟の行の write-set は B の行 a の `src/lib.rs` と交差させない（(h) を除く）。

/// 設計 doc に節 2 を足し、`rows`（[`section_row`]）を 1 回の commit で書く（行の `section` が節 1 か 2 を指す・表の前に節 2 の見出しと本文を置く）。
fn commit_sections(repo: &Path, rows: &[Vec<String>]) {
    let begin = vessel::pipe::table::BEGIN;
    let body = design_doc_rows(rows).replacen(begin, &format!("## 2. 別の節\n\n別の節の本文。\n\n{begin}"), 1);
    if fs::read_to_string(repo.join(DESIGN_FILE)).is_ok_and(|found| found == body) {
        return;
    }
    for fields in rows {
        super::super::seed_write_set(repo, fields);
    }
    write_design(repo, &body);
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "design-sections"]);
    assert!(fs::read_to_string(repo.join(DESIGN_FILE)).is_ok_and(|found| found.contains("## 2.")), "前提: 節 2 を持つ doc を commit した");
}

/// 行 `id`（節 `section`・write-set は `file` 1 本）の欄の列。
fn section_row(id: &str, section: &str, file: &str) -> Vec<String> {
    row_fields(id, &["write-set", "section"], &[&format!("write-set = [\"{file}\"]"), &format!("section = \"{section}\"")])
}

/// 行 `row` の bead（B の行 a は [`FALLEN`]・ほかは `s2-sib-<行>`）。
fn sib(row: &str) -> String {
    if row == "a" { FALLEN.to_owned() } else { format!("s2-sib-{row}") }
}

/// 行 `rows` を commit した置き場（repo・state）。`rows` は (行 id・節・write-set の file)。
fn sib_place(rows: &[(&str, &str, &str)]) -> (std::path::PathBuf, std::path::PathBuf) {
    let (repo, state) = repo_with_state();
    commit_sections(&repo, &rows.iter().map(|(id, section, file)| section_row(id, section, file)).collect::<Vec<_>>());
    (repo, state)
}

/// 台帳（`rows` の行ごとに 1 本・B の行 a は P1・ほかは P2）の偽 bd。
fn sib_bd(state: &Path, rows: &[&str]) -> String {
    fake_bd(state, &rows.iter().map(|row| issue(&sib(row), if *row == "a" { 1 } else { 2 }, row)).collect::<Vec<_>>())
}

/// 行 a の便 B を終端の形 `form` に着ける（[`reserve_fallen`] の形に、unparsed の INCONCLUSIVE を足す）。
fn sib_fallen(repo: &Path, state: &Path, form: &str) -> String {
    match form {
        "reviewed-unparsed" => judged_run(repo, state, FALLEN, UNPARSED),
        other => reserve_fallen(repo, state, other),
    }
}

/// 行 `row` の便を審査の判定 `judgement` で Reviewed の終端に着ける（B2 の行が行 a でない形）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn sib_reviewed(repo: &Path, state: &Path, row: &str, judgement: &str) -> String {
    let id = intake_bead(repo, state, &format!("{DESIGN_FILE}#{row}"), &sib(row));
    fs::write(state.join("pipe").join(&id).join(REVIEW_FILE), format!("{judgement}\n")).expect("審査の判定を書ける");
    id
}

/// `rules` の写しで `dispatch ls` を `rounds` 周撃ち、各周の前後で event log の行数が同じ（兄弟の待ちは記帳しない）ことを測って最後の周を返す。
fn sib_ls(repo: &Path, state: &Path, bd: &str, rules: &str, rounds: usize) -> Output {
    let args = ["dispatch", "ls", "--state-dir", &state.display().to_string(), "--repo", &repo.display().to_string(), "--rules", rules, "--bd", bd];
    let mut last = run_pipe(&args);
    for round in 1..=rounds {
        let before = event_lines(state);
        last = run_pipe(&args);
        let waited = stdout_of(&last).lines().filter(|line| line.contains("reason=sibling:")).count();
        assert_eq!(event_lines(state), before, "{rounds} 周中 {round} 周目・待った候補 {waited} 本: 兄弟の待ちは event を増やさない");
    }
    last
}

/// 便 `run` の写し（契約 file と材料の dir の design.txt）から、行の審査の口 (B) で求めた行の digest。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn copy_digest(state: &Path, run: &str) -> String {
    let contract = fs::read_to_string(state.join("pipe").join(run).join("contract.toml")).expect("契約の写しを読める");
    let section = fs::read_to_string(section_copy(state, run)).expect("節の写しを読める");
    vessel::pipe::row_review::row_digest(contract.strip_suffix('\n').unwrap_or(&contract), section.strip_suffix('\n').unwrap_or(&section))
}

/// 行 `row` の今の digest（別の bead で 1 便を起こして写しから求め、すぐ止める＝台帳の bead の便は増えない）。
fn row_digest_now(repo: &Path, state: &Path, row: &str) -> String {
    let id = intake_bead(repo, state, &format!("{DESIGN_FILE}#{row}"), &format!("s2-probe-{row}"));
    let digest = copy_digest(state, &id);
    super::super::stop_run_ok(state, &id);
    digest
}

/// 行の記録を 1 つ置く（`<根>/<名>/record`・§9 の形・`tail` は basis・判定・理由の型・at の秒）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn sib_record(state: &Path, name: &str, row: &str, digest: &str, tail: (&str, &str, &str, u64)) {
    let (basis, verdict, kind, at) = tail;
    let dir = state.join("pipe").join("row-review").join(name);
    fs::create_dir_all(&dir).expect("記録の dir を作れる");
    let body = format!("schema=1\nrow={DESIGN_FILE}#{row}\ndigest={digest}\nkey=0\nbasis={basis}\nverdict={verdict}\nkind={kind}\nat={at}\n");
    fs::write(dir.join("record"), body).expect("行の記録を書ける");
}

/// ref の記録を 1 本置く（名は `digit` を 40 回・`rows` は (行・digest)）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn sib_ref(state: &Path, digit: char, rows: &[(&str, &str)]) {
    let dir = state.join("pipe").join("row-review").join("ref");
    fs::create_dir_all(&dir).expect("ref の dir を作れる");
    let listed: String = rows.iter().map(|(row, digest)| format!("row={DESIGN_FILE}#{row} digest={digest} id=0000000000000001 verdict=FAIL basis=actual\n")).collect();
    let body = format!("schema=1\nbase={}\ntables={DESIGN_FILE}\n{listed}result=fail\n", "b".repeat(40));
    fs::write(dir.join(digit.to_string().repeat(40)), body).expect("ref の記録を書ける");
}

/// 今の秒（記録の at を終端の前後に置く）。
fn sib_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |found| found.as_secs())
}

/// (a) 直前の便が Reviewed FAIL の B と同じ節の行の候補は `sibling:<B>` で待ち、同じ doc の別の節の行の候補は起こされる。判定は event を増やさない（3 周）。
#[test]
fn pipe_dispatch_sibling_wait_holds_the_same_section_and_wakes_the_other_section() {
    let (repo, state) = sib_place(&[("a", "1", "src/lib.rs"), ("b", "1", "src/b.rs"), ("g", "2", "src/g.rs")]);
    sib_fallen(&repo, &state, "reviewed-fail");
    let out = sib_ls(&repo, &state, &sib_bd(&state, &["a", "b", "g"]), &reserve_rules(&state, Some(24)), 3);
    assert_eq!(reason_of(&out, &sib("b")), format!("sibling:{FALLEN}"), "同じ節の行は待つ（{}）", told(&out));
    assert_eq!(reason_of(&out, &sib("g")), "-", "別の節の行は起こされる（{}）", told(&out));
    assert!(reason_of(&out, FALLEN).starts_with("settled:"), "B 自身は列外のまま（{}）", told(&out));
    assert_eq!(count_of(&out), format!("{COUNT} total=3 ready=1"), "起こせるのは別の節の 1 本だけ");
    clean(&[&repo, &state]);
}

/// ref の記録 1 本（名の字・載せる行と digest の対）。
type RefFile<'a> = (char, Vec<(&'a str, &'a str)>);

/// (b) B の写しから求めた digest で B の行を載せた ref の記録に載る別の節の行も待つ（push 2 回の PR の 2 file の形と、直しの PR の 1 file の形）。
/// B の行を別の digest で載せた ref の記録だけに載る行は起こされる（対照）。
#[test]
fn pipe_dispatch_sibling_wait_follows_the_ref_records_that_carry_the_fallen_row() {
    let other = "ffffffffffffffff";
    // 形ごとに (名・ref の記録の列・待つ行・起こされる行)。記録の列は (digit・[(行・digest 〔`D` は B の digest〕)])・行は 1 字ずつ。
    let forms: [(&str, Vec<RefFile>, &str, &str); 3] = [
        ("push 2 回の PR の 2 file", vec![('1', vec![("a", "D"), ("x", "1")]), ('2', vec![("a", "D"), ("y", "1")])], "xy", "z"),
        ("直しの PR の 1 file", vec![('3', vec![("a", "D"), ("x", "1")])], "x", "yz"),
        ("別の digest の記録だけ", vec![('4', vec![("a", other), ("x", "1"), ("y", "1")])], "", "xyz"),
    ];
    for (form, refs, waits, wakes) in forms {
        let (repo, state) = sib_place(&[("a", "1", "src/lib.rs"), ("x", "2", "src/x.rs"), ("y", "2", "src/y.rs"), ("z", "2", "src/z.rs")]);
        let run = sib_fallen(&repo, &state, "reviewed-fail");
        let digest = copy_digest(&state, &run);
        for (digit, rows) in &refs {
            sib_ref(&state, *digit, &rows.iter().map(|(row, word)| (*row, if *word == "D" { digest.as_str() } else { *word })).collect::<Vec<_>>());
        }
        let out = sib_ls(&repo, &state, &sib_bd(&state, &["a", "x", "y", "z"]), &reserve_rules(&state, Some(24)), 1);
        for row in waits.chars() {
            assert_eq!(reason_of(&out, &sib(&row.to_string())), format!("sibling:{FALLEN}"), "{form}: {row} は待つ（{}）", told(&out));
        }
        for row in wakes.chars() {
            assert_eq!(reason_of(&out, &sib(&row.to_string())), "-", "{form}: {row} は起こされる（{}）", told(&out));
        }
        clean(&[&repo, &state]);
    }
}

/// (c) first の印を持つ候補・B が blocks で直に待つ同じ節の候補・blocks を 2 段たどった祖先の候補は起こされ、同じ周の同じ節の印の無い候補は待つ（対照は先に全員が待つ周）。
#[test]
fn pipe_dispatch_sibling_wait_spares_the_first_mark_and_the_blocks_ancestors() {
    let rows = [("a", "1", "src/lib.rs"), ("b", "1", "src/b.rs"), ("c", "1", "src/c.rs"), ("d", "1", "src/d.rs"), ("e", "1", "src/e.rs"), ("f", "1", "src/f.rs")];
    let (repo, state) = sib_place(&rows);
    sib_fallen(&repo, &state, "reviewed-fail");
    let rules = reserve_rules(&state, Some(24));
    let plain = sib_ls(&repo, &state, &sib_bd(&state, &["a", "b", "c", "d", "e", "f"]), &rules, 1);
    for row in ["b", "c", "d", "e", "f"] {
        assert_eq!(reason_of(&plain, &sib(row)), format!("sibling:{FALLEN}"), "対照: {row} は待つ（{}）", told(&plain));
    }
    let marked = run_pipe(&["dispatch", "first", &sib("c"), "--state-dir", &state.display().to_string()]);
    assert_eq!(marked.status.code(), Some(i32::from(RC_OK)), "first: {}", told(&marked));
    let acceptance = |row: &str| format!("design = {DESIGN_FILE}#{row}");
    // B は d（直）と e（e は f を待つので f は 2 段）を blocks で待つ。
    let bd = fake_bd(
        &state,
        &[
            listed(FALLEN, "open", 1, &acceptance("a"), &[(&sib("d"), "blocks"), (&sib("e"), "blocks")]),
            issue(&sib("b"), 2, "b"),
            issue(&sib("c"), 2, "c"),
            issue(&sib("d"), 2, "d"),
            listed(&sib("e"), "open", 2, &acceptance("e"), &[(&sib("f"), "blocks")]),
            issue(&sib("f"), 2, "f"),
        ],
    );
    let out = sib_ls(&repo, &state, &bd, &rules, 1);
    assert_eq!(reason_of(&out, &sib("b")), format!("sibling:{FALLEN}"), "印の無い候補は待つ（{}）", told(&out));
    assert_eq!(reason_of(&out, &sib("c")), "-", "first の印の候補は待たない（{}）", told(&out));
    assert_eq!(reason_of(&out, &sib("d")), "-", "B が直に待つ候補は待たない（{}）", told(&out));
    assert_eq!(reason_of(&out, &sib("f")), "-", "blocks を 2 段たどった祖先は待たない（{}）", told(&out));
    clean(&[&repo, &state]);
}

/// (d) 直前の便が Failed・Stopped・unparsed の INCONCLUSIVE の B と、終端の後に新しい便が起きた B は兄弟を待たせず、Gated FAIL と unparsed でない INCONCLUSIVE の B は待たせる。
/// 各形は同じ周の Reviewed FAIL の別の B2（行 g）の兄弟（行 h・別の節）が待つ対照を持つ。
#[test]
fn pipe_dispatch_sibling_wait_belongs_to_the_design_side_terminal_forms() {
    let forms = [
        ("failed", false),
        ("stopped", false),
        ("reviewed-unparsed", false),
        ("rerun", false),
        ("gated-fail", true),
        ("reviewed-inconclusive", true),
    ];
    for (form, waits) in forms {
        let (repo, state) = sib_place(&[("a", "1", "src/lib.rs"), ("b", "1", "src/b.rs"), ("g", "2", "src/g.rs"), ("h", "2", "src/h.rs")]);
        sib_reviewed(&repo, &state, "g", "{\"verdict\":\"FAIL\"}");
        if form == "rerun" {
            sib_fallen(&repo, &state, "reviewed-fail");
            // run id は `<bead>-<秒>`: 前の便と同じ秒に起こすと id が衝突する。
            std::thread::sleep(std::time::Duration::from_millis(1100));
            intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), FALLEN);
        } else {
            sib_fallen(&repo, &state, form);
        }
        let out = sib_ls(&repo, &state, &sib_bd(&state, &["a", "b", "g", "h"]), &reserve_rules(&state, Some(24)), 1);
        let want = if waits { format!("sibling:{FALLEN}") } else { "-".to_owned() };
        assert_eq!(reason_of(&out, &sib("b")), want, "{form}: B の兄弟（{}）", told(&out));
        assert_eq!(reason_of(&out, &sib("h")), format!("sibling:{}", sib("g")), "{form}: 対照の B2 の兄弟は待つ（{}）", told(&out));
        clean(&[&repo, &state]);
    }
}

/// (e) 解けの 8 形（B の行を直して main に commit・兄弟自身の今の digest の PASS の記録が終端の後・B への hold・終端の後の release・B の close・値 1 の写しで 2 時間前の終端・
/// 兄弟自身の forecast と partial の unparsed でない INCONCLUSIVE の記録が終端の後）は先に待ってから起こされ、解けない 4 形（兄弟自身の FAIL の記録が終端の後・PASS の記録が終端の前・
/// actual の unparsed でない INCONCLUSIVE の記録が終端の後・forecast の unparsed の INCONCLUSIVE の記録が終端の後）は待ったまま。
#[test]
fn pipe_dispatch_sibling_wait_ends_by_a_fix_a_mark_a_close_a_deadline_or_the_sibling_own_record() {
    let (later, earlier) = (sib_now() + 3600, 1);
    let forms: [(&str, bool); 12] = [
        ("fix", true),
        ("own-pass", true),
        ("hold", true),
        ("release", true),
        ("close", true),
        ("expiry", true),
        ("own-forecast", true),
        ("own-partial", true),
        ("own-fail", false),
        ("pass-before", false),
        ("own-actual-inconclusive", false),
        ("own-forecast-unparsed", false),
    ];
    for (form, woken) in forms {
        let (repo, state) = sib_place(&[("a", "1", "src/lib.rs"), ("b", "1", "src/b.rs")]);
        let run = sib_fallen(&repo, &state, "reviewed-fail");
        let digest = row_digest_now(&repo, &state, "b");
        let waiting = sib_ls(&repo, &state, &sib_bd(&state, &["a", "b"]), &reserve_rules(&state, Some(24)), 1);
        assert_eq!(reason_of(&waiting, &sib("b")), format!("sibling:{FALLEN}"), "{form}: 前提は待つ（{}）", told(&waiting));
        let records = [
            ("own-pass", "actual", "PASS", "-", later),
            ("own-forecast", "forecast", "INCONCLUSIVE", "section-material-missing", later),
            ("own-partial", "partial", "INCONCLUSIVE", "section-material-missing", later),
            ("own-fail", "forecast", "FAIL", "-", later),
            ("pass-before", "actual", "PASS", "-", earlier),
            ("own-actual-inconclusive", "actual", "INCONCLUSIVE", "section-material-missing", later),
            ("own-forecast-unparsed", "forecast", "INCONCLUSIVE", "unparsed", later),
        ];
        let (mut bd, mut hours) = (sib_bd(&state, &["a", "b"]), 24);
        if let Some((_, basis, verdict, kind, at)) = records.iter().find(|found| found.0 == form) {
            sib_record(&state, form, "b", &digest, (basis, verdict, kind, *at));
        }
        match form {
            "fix" => {
                let widened = row_fields("a", &["write-set", "section"], &[r#"write-set = ["src/lib.rs", "src/extra.rs"]"#, r#"section = "1""#]);
                commit_sections(&repo, &[widened, section_row("b", "1", "src/b.rs")]);
            }
            "hold" => {
                let out = run_pipe(&["dispatch", "hold", FALLEN, "--state-dir", &state.display().to_string()]);
                assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "hold: {}", stderr_of(&out));
            }
            "release" => release(&state, FALLEN),
            "close" => bd = fake_bd(&state, &[listed(FALLEN, "closed", 1, &format!("design = {DESIGN_FILE}#a"), &[]), issue(&sib("b"), 2, "b")]),
            "expiry" => {
                age_run(&state, &run, 2 * 3600);
                hours = 1;
            }
            _ => {}
        }
        let out = sib_ls(&repo, &state, &bd, &reserve_rules(&state, Some(hours)), 1);
        let want = if woken { "-".to_owned() } else { format!("sibling:{FALLEN}") };
        assert_eq!(reason_of(&out, &sib("b")), want, "{form}: 解けるか（{}）", told(&out));
        clean(&[&repo, &state]);
    }
}

/// (f) 兄弟で依存待ちの候補は dependency のまま（床の合格の周）。床の検査が合格の周に sibling で待つ候補は、不合格の周は floor で待つ（2 形）。
#[test]
fn pipe_dispatch_sibling_wait_keeps_a_dependency_and_yields_to_a_failing_floor() {
    // 依存待ちの兄弟は床の合格の周に dependency のまま（床の不合格の周は既存の上書きで floor）。
    for (body, want, dependent) in [("exit 0", format!("sibling:{FALLEN}/unset"), "dependency:s2-dep"), ("exit 1", "floor:1".to_owned(), "floor:1")] {
        let place = floor_place(Some(FLOOR_CMD), body);
        commit_sections(&place.repo, &[section_row("a", "1", "src/lib.rs"), section_row("b", "1", "src/b.rs"), section_row("c", "1", "src/c.rs")]);
        sib_fallen(&place.repo, &place.state, "reviewed-fail");
        // 床の結果は起こす側の 1 周が置く（台帳は空）。この写しは期限の行を持たないので値の末尾は `/unset`。
        let rules = floor_rules(&place.state, Some(600));
        floor_round(&place, &rules);
        let acceptance = |row: &str| format!("design = {DESIGN_FILE}#{row}");
        let bd = fake_bd(
            &place.state,
            &[issue(FALLEN, 1, "a"), issue(&sib("b"), 2, "b"), listed(&sib("c"), "open", 2, &acceptance("c"), &[("s2-dep", "blocks")]), listed("s2-dep", "open", 2, "memo", &[])],
        );
        let out = wait_ls(&place, &rules, &bd);
        assert_eq!(reason_of(&out, &sib("b")), want, "{body}: 兄弟の理由（{}）", told(&out));
        assert_eq!(reason_of(&out, &sib("c")), dependent, "{body}: 依存待ちの兄弟（{}）", told(&out));
        clean(&[&place.repo, &place.state]);
    }
}

/// (g) 期限の行の無い rules の写しの周は `sibling:<B>/unset`、値 0 の写しは 100 時間前の終端でも `sibling:<B>` で待つ（2 形・値 24 は手放す対照）。
#[test]
fn pipe_dispatch_sibling_wait_without_the_deadline_row_is_unset_and_zero_never_expires() {
    let (repo, state) = sib_place(&[("a", "1", "src/lib.rs"), ("b", "1", "src/b.rs")]);
    let run = sib_fallen(&repo, &state, "reviewed-fail");
    age_run(&state, &run, 100 * 3600);
    let bd = sib_bd(&state, &["a", "b"]);
    let unset = sib_ls(&repo, &state, &bd, &reserve_rules(&state, None), 1);
    assert_eq!(reason_of(&unset, &sib("b")), format!("sibling:{FALLEN}/unset"), "行の無い写しは期限なし・末尾 /unset（{}）", told(&unset));
    let zero = sib_ls(&repo, &state, &bd, &reserve_rules(&state, Some(0)), 1);
    assert_eq!(reason_of(&zero, &sib("b")), format!("sibling:{FALLEN}"), "値 0 は期限なし（{}）", told(&zero));
    let expired = sib_ls(&repo, &state, &bd, &reserve_rules(&state, Some(24)), 1);
    assert_eq!(reason_of(&expired, &sib("b")), "-", "対照: 値 24 は 100 時間前の終端の待ちを手放す（{}）", told(&expired));
    clean(&[&repo, &state]);
}

/// (h) B と同じ節で write-set が B の行の予約と交差する後ろの候補は reserved でなく `sibling:<B>` で待つ（別の節の同じ file の候補は対照で `reserved:<B>/1`）。
#[test]
fn pipe_dispatch_sibling_wait_precedes_the_row_reservation() {
    let (repo, state) = sib_place(&[("a", "1", "src/lib.rs"), ("l", "1", "src/lib.rs"), ("m", "2", "src/lib.rs")]);
    sib_fallen(&repo, &state, "reviewed-fail");
    let out = sib_ls(&repo, &state, &sib_bd(&state, &["a", "l", "m"]), &reserve_rules(&state, Some(24)), 1);
    assert_eq!(reason_of(&out, &sib("l")), format!("sibling:{FALLEN}"), "同じ節の交差する候補は兄弟の待ち（{}）", told(&out));
    assert_eq!(reason_of(&out, &sib("m")), format!("reserved:{FALLEN}/1"), "対照: 別の節の交差する候補は行の予約（{}）", told(&out));
    clean(&[&repo, &state]);
}
