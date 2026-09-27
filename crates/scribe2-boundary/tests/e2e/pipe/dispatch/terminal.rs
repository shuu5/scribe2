// flip-check: moved s2-07l.686
//! 終端の周の族の歯（接頭辞 `pipe_terminal_`・設計 docs/design/carry-prep.md §10 行 n・親 `tests/e2e/pipe/dispatch.rs` の
//! helper を `use super::*` で使う）。

use super::*;

/// (§5 手動の 1 周) subcommand の無い `pipe dispatch` は 1 周を撃ってその結果を 1 行で返し、通る便を
/// **起こす**（`RunCreated` が増える）。測れない周は件数でなく理由を名乗る（C10）。
///
/// 印の直後の 1 周は**便を起こさない歯**（`..._marks_fire_without_children`・行 g）が測る——ここで
/// 続けて測ると、起こした子 process が走っている最中の状態に依存する（`s2-07l.487`）。
#[test]
fn pipe_terminal_dispatch_manual_turn_starts_the_runs_it_can() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a"), issue("s2-toy.2", 0, "b")]);
    let turn = || -> Output {
        let args: Vec<String> = vec![
            "dispatch".to_owned(),
            "--state-dir".to_owned(),
            state.display().to_string(),
            "--repo".to_owned(),
            repo.display().to_string(),
            "--rules".to_owned(),
            dispatch_rules(&state),
            "--bd".to_owned(),
            bd.clone(),
            // 起こした便の審査は**偽 PASS の lens**で通す（既定の lens は実 claude を起こす）。
            "--lens".to_owned(),
            review_lens_pass(&state),
            // 実装役は偽の 1 行（器は runner の既定を持たない）。
            "--runner".to_owned(),
            "true".to_owned(),
        ];
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        run_pipe(&borrowed)
    };
    let manual = turn();
    assert_eq!(manual.status.code(), Some(i32::from(RC_OK)), "手動の 1 周は rc 0（{}）", told(&manual));
    assert_eq!(
        stdout_of(&manual).trim_end(),
        "dispatch=started:2,resumed:0,waiting:0",
        "交差しない 2 本は両方起こせる（{}）",
        told(&manual)
    );
    // **起こした効果**: 起こした 2 本ぶんの `RunCreated` が置き場に積まれる（構築点で止まらない・裁定 (A)）。
    assert_eq!(created(&state, &["s2-toy.1", "s2-toy.2"], 2), 2, "起こした便の RunCreated が 2 件");
    // 台帳を読めない周は件数でなく理由を名乗る（0 件と融合しない・C10）。
    let broken = script(&state.join("bd-broken2"), "exit 1\n");
    let unmeasured = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &broken,
        "--runner", "true",
    ]);
    assert_eq!(stdout_of(&unmeasured).trim_end(), "dispatch=unmeasured reason=ledger", "読めない周は理由（{}）", told(&unmeasured));
    // **実装役の口が無い周は台帳も読まない**（起こせないと分かっている＝理由が別の値・C10）。
    let no_runner = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &bd,
    ]);
    assert_eq!(
        stdout_of(&no_runner).trim_end(),
        "dispatch=unmeasured reason=no-runner",
        "runner が無い周（{}）",
        told(&no_runner)
    );
    clean(&[&repo, &state]);
}

/// (§5 便の終端) `pipe stop` の終端の記帳の直後に列が 1 周撃たれ、**交差の解けた便が起こされる**
/// （`RunCreated` が増える）。台帳を読めない周でも終端の rc は変わらない。
#[test]
fn pipe_terminal_dispatch_stop_starts_the_contract_whose_overlap_just_cleared() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    // 行 a を持つ live な便＝同じ行を指す候補と交差する。
    let live = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), "s2-live");
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a")]);
    let blocked = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&blocked, "s2-toy.1"), format!("overlap:{live}/1"), "止める前は交差で待つ");
    assert_eq!(count_of(&blocked), format!("{COUNT} total=1 ready=0"), "起こせる便は 0 本");
    let terminal = |client: &str| -> Output {
        run_pipe(&[
            "stop", "--run", &live,
            "--state-dir", &state.display().to_string(),
            "--repo", &repo.display().to_string(),
            "--rules", &dispatch_rules(&state),
            "--bd", client,
            "--lens", &review_lens_pass(&state),
            "--runner", "true",
        ])
    };
    let stopped = terminal(&bd);
    assert_eq!(stopped.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", stderr_of(&stopped));
    // **観測の面は増えない**（設計 §6）: 終端の stdout に列の行は出ず、効果だけが残る。
    assert!(!stdout_of(&stopped).contains("dispatch="), "終端は列の行を出さない: {}", stdout_of(&stopped));
    assert_eq!(created(&state, &["s2-toy.1"], 1), 1, "交差が解けた便が起こされる（RunCreated 1 件）");
    clean(&[&repo, &state]);
}

/// (§5 便の終端) 終端の中の 1 周が**失敗しても終端の rc は変わらない**（台帳を読めない周）。
#[test]
fn pipe_terminal_dispatch_keeps_the_terminal_rc_when_the_round_cannot_measure() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let live = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), "s2-live");
    let broken = script(&state.join("bd-dead"), "exit 1\n");
    let stopped = run_pipe(&[
        "stop", "--run", &live,
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &broken,
        "--runner", "true",
    ]);
    assert_eq!(stopped.status.code(), Some(i32::from(RC_OK)), "読めない台帳でも stop は rc 0: {}", stderr_of(&stopped));
    assert_eq!(created(&state, &["s2-toy.1"], 0), 0, "読めない周は 1 本も起こさない");
    // 終端そのものは通っている（便は止まっている）＝1 周の失敗が終端を巻き込んでいない。
    let again = run_pipe(&["stop", "--run", &live, "--state-dir", &state.display().to_string()]);
    assert_ne!(again.status.code(), Some(i32::from(RC_OK)), "既に終端ゆえ 2 度目は断られる");
    clean(&[&repo, &state]);
}

/// (§5 便の終端) `pipe run` の終端の 1 周は**その process 自身の道具**（`--rules` / `--lens` / `--runner`）で
/// 便を起こす。driver は自分の道具を知っているので、列に渡し直さなくても起こせる。
#[test]
fn pipe_terminal_dispatch_run_uses_its_own_tools_for_the_round() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    // 台帳の候補は行 a。`pipe run` が起こすのは行 b（交差しない）＝終端の 1 周で行 a が起きる。
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a")]);
    let ran = run_pipe(&[
        "run", "--design", &format!("{DESIGN_FILE}#b"), "--bead", "s2-own",
        "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &bd,
        "--lens", &review_lens_pass(&state),
        "--runner", "true",
    ]);
    // `pipe run` 自体の rc は問わない（toy の runner は何もしないので後段で止まる）。測るのは**終端の 1 周**である。
    assert!(!stdout_of(&ran).contains("dispatch="), "終端は列の行を出さない: {}", stdout_of(&ran));
    assert_eq!(created(&state, &["s2-toy.1"], 1), 1, "自分の道具で台帳の候補 1 本を起こす");
    clean(&[&repo, &state]);
}

/// (§5 便の終端 + §2 終端の便は列外) `pipe land` の終端の直後に列が 1 周撃たれ、**交差の外の候補が起こされる**
/// 一方で、**着地した便の bead は起こし直されない**（`settled`）。契約の行を改訂して sha が動けば列に戻る。
///
/// 着地から台帳を閉じるまでの間、終端が来るたびに同じ契約が起こし直される穴（`s2-07l.366` の自己レビュー）を
/// 両側で測る。母集団は `[DISPATCH-COUNT]` の `total=` で同時に出す。
#[test]
fn pipe_terminal_dispatch_land_starts_the_queue_without_restarting_the_landed_bead() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let design_a = format!("{DESIGN_FILE}#a");
    // 行 a の便を PASS の gate まで通す（bead は台帳の候補と同じ id にする）。
    let landing = intake_bead(&repo, &state, &design_a, "s2-toy.1");
    let spawned = run_pipe(&[
        "spawn", "--run", &landing, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&spawned));
    let lens = fake_lens(&state.join("gate-lens-ran"), &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &landing, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate は rc 0: {}", stderr_of(&gated));
    // 台帳の候補は 2 件: 着地する bead（行 a）と、交差しない bead（行 b）。
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a"), issue("s2-toy.2", 2, "b")]);
    let landed = run_pipe(&[
        "land", "--run", &landing, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &bd,
        "--lens", &review_lens_pass(&state),
        "--runner", "true",
    ]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&landed));
    // 終端の 1 周が撃たれた（行 b の候補が起きる）。
    assert_eq!(created(&state, &["s2-toy.2"], 1), 1, "land の終端の直後に交差の外の 1 本が起きる");
    // **着地した bead は起こし直されない**（RunCreated は着地した便の 1 件だけ）。
    assert_eq!(created(&state, &["s2-toy.1"], 0), 1, "着地した bead の便は 1 件のまま（起こし直さない）");
    let after = ls(&repo, &state, &bd);
    let reason = reason_of(&after, "s2-toy.1");
    assert!(reason.starts_with("settled:"), "着地した便の sha で列外: {}", stdout_of(&after));
    assert!(reason.ends_with("/Landed"), "段は Landed: {reason}");
    assert_eq!(count_of(&after), format!("{COUNT} total=2 ready=0"), "母集団 2 件・起こせる 0 本");
    // **契約の行を改訂して sha が動けば列に戻る**（着地した便は塞ぎ続けない）。
    write_design(
        &repo,
        &design_doc_rows(&[
            row_fields("a", &["write-set", "done"], &[r#"write-set = ["src/lib.rs"]"#, r#"done = "改訂した""#]),
            row_fields("b", &["write-set"], &[r#"write-set = ["src/b.rs"]"#]),
        ]),
    );
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "design-row-revised"]);
    let revised = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&revised, "s2-toy.1"), "-", "sha が動けば列に戻る: {}", stdout_of(&revised));
    clean(&[&repo, &state]);
}

/// (§5 起こす便へ渡す道具) 起こした子（`pipe run`）**自身も終端で 1 周撃つ**ので、列に渡した台帳 client も
/// そのまま渡る。落とすと子の 1 周が既定の台帳（PATH の `bd`）を読み、1 hop で列と食い違う。
///
/// 測り方: 偽の台帳を「呼ばれたら印を置く」形にし、**列の 1 周（1 回）と子の 1 周（1 回）で 2 回**呼ばれる
/// ことを見る。落ちていれば 1 回で止まる（子は PATH の `bd` を読む）。
#[test]
fn pipe_terminal_dispatch_hands_the_ledger_client_to_the_run_it_starts() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let json = state.join("ledger-counted.json");
    fs::write(&json, format!("[{}]\n", issue("s2-toy.2", 2, "b"))).expect("偽の台帳を書ける");
    let calls = state.join("bd-calls.log");
    let counted = script(
        &state.join("bd-counted"),
        &format!("printf 'x\\n' >> '{}'\ncat '{}'\n", calls.display(), json.display()),
    );
    let out = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &counted,
        "--lens", &review_lens_pass(&state),
        "--runner", "true",
    ]);
    assert_eq!(stdout_of(&out).trim_end(), "dispatch=started:1,resumed:0,waiting:0", "1 本起こす: {}", stderr_of(&out));
    assert_eq!(created(&state, &["s2-toy.2"], 1), 1, "起こした便の RunCreated");
    // 子の終端の 1 周が**同じ台帳**を読む＝印が 2 つ（列の 1 周 + 子の 1 周）。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let count = || fs::read_to_string(&calls).map(|text| text.lines().count()).unwrap_or_default();
    while count() < 2 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(count(), 2, "列の 1 周と子の 1 周で偽の台帳が 2 回呼ばれる（母集団 = 1 周 × 2 段）");
    clean(&[&repo, &state]);
}

/// (§5 印の直後) `first` / `release` の記録の直後にも 1 周が撃たれ、`hold` は撃たない。
///
/// **便を 1 本も起こさずに測る**（`s2-07l.487`・行 g）: 台帳の候補が live な便と交差する形にすれば、
/// 1 周は必ず `started:0` で、子 process が 1 つも生まれない。起こしてから印を打つ形（`.366` の元の歯）
/// は、**走っている子の状態に依存**する——`s2-07l.486` の merge sha で CI が 1 度赤くなり、同じ sha の
/// 再走では緑・ローカルの負荷では両側 0/20 で、原因を特定できなかった（台帳 notes）。
#[test]
fn pipe_terminal_dispatch_marks_fire_without_children() {
    // flip-check: retroactive s2-07l.487
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    // 行 a を持つ live な便を 1 本置く＝台帳の候補（行 a）は必ず交差して起こせない。
    let live = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), "s2-live");
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a")]);
    let turn = |extra: &[&str]| -> Output {
        let mut args: Vec<String> = vec!["dispatch".to_owned()];
        args.extend(extra.iter().map(|found| (*found).to_owned()));
        args.extend([
            "--state-dir".to_owned(),
            state.display().to_string(),
            "--repo".to_owned(),
            repo.display().to_string(),
            "--rules".to_owned(),
            dispatch_rules(&state),
            "--bd".to_owned(),
            bd.clone(),
            "--runner".to_owned(),
            "true".to_owned(),
        ]);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        run_pipe(&borrowed)
    };
    let waiting = "dispatch=started:0,resumed:0,waiting:1";
    let manual = turn(&[]);
    assert_eq!(stdout_of(&manual).trim_end(), waiting, "交差する候補は起こせない（{}）", told(&manual));
    // `first` の記録の直後に 1 周（印の行 → 1 周の行の 2 行）。
    let first = turn(&["first", "s2-toy.1"]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "first は rc 0（{}）", told(&first));
    assert_eq!(stdout_of(&first).lines().last(), Some(waiting), "first の直後に 1 周（{}）", told(&first));
    // `hold` は起こす側を増やさないので 1 周を撃たない（印の行だけ）。
    let held = turn(&["hold", "s2-toy.1"]);
    assert_eq!(stdout_of(&held).lines().count(), 1, "hold は印の行だけ（{}）", told(&held));
    assert!(!stdout_of(&held).contains("dispatch="), "hold は 1 周を撃たない（{}）", told(&held));
    // `release` の記録の直後にも 1 周。
    let released = turn(&["release", "s2-toy.1"]);
    assert_eq!(released.status.code(), Some(i32::from(RC_OK)), "release は rc 0（{}）", told(&released));
    assert_eq!(stdout_of(&released).lines().last(), Some(waiting), "release の直後に 1 周（{}）", told(&released));
    // **子 process は 1 つも生まれない**（母集団 = 撃った 1 周 4 回）。
    assert_eq!(created(&state, &["s2-toy.1"], 0), 0, "便を 1 本も起こさない（1 周 4 回）");
    assert_eq!(kind_count(&state, &live, vessel::fleet::EventKind::RunCreated), 1, "live な便は元の 1 件のまま");
    clean(&[&repo, &state]);
}
