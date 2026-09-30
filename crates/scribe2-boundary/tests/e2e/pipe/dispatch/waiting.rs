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
