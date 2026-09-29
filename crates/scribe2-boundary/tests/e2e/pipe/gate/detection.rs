// flip-check: moved s2-07l.685
//! 検出線と着地後の検出の口の族の歯（接頭辞 `pipe_detection_` / `pipe_landed_`・設計 docs/design/carry-prep.md §10 行 m）。
//!
//! 共有の helper と const と外形 snapshot の歯は親 module（`tests/e2e/pipe/gate.rs`）に在り、`use super::*` で使う。
//! 歯の本文は親から**挙動不変で移した**もの（`s2-07l.685`）。

use super::*;

/// (j) 検出線を宣言した便の gate は stub を 1 回も呼ばず（印は ②④ だけ）、`verify.jsonl` は ①②④ の 3 本で
/// `kind=detection` を持たず、写しの置き場を作らず、gate の後の `pipe show` は段の 1 行だけ（判定行が無い）。
#[test]
fn pipe_detection_off_gate_declared_run_fires_no_detection_and_shows_no_line() {
    let (repo, state, design) = detection_repo(DETECTION_COUNT);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let calls = detection_calls(&repo);
    assert_eq!(calls, ["common".to_owned(), "contract".to_owned()], "gate が撃つのは ②④ だけ: {calls:?}");
    assert!(detection_marks(&calls).is_empty(), "検出線の stub は 1 回も呼ばれない: {calls:?}");
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "contract"], "gate の段は ①②④: {rows:?}");
    assert!(rows.iter().all(|row| value_of(row, "kind") != "detection"), "kind=detection の record は無い: {rows:?}");
    assert!(!run_dir(&state, &id).join(COPY_DIR).exists(), "写しの置き場を作らない");
    let shown = show_line(&repo, &state, &id);
    assert_eq!(shown.lines().count(), 1, "gate の後の `pipe show` は段の 1 行だけ: {shown}");
    assert!(shown.contains("stage=Gated"), "1 行目は便の段: {shown}");
    assert!(!shown.contains("mutants-diff") && !shown.contains(COPY_ABSENT_LINE), "判定行は無い: {shown}");
    clean(&[&repo, &state]);
}

/// (n) main が `crates/`（検出線の面の内）に触れて動いた便の追随の再 gate も stub を呼ばない: 再 gate は撃たれる
/// （`verify.jsonl` は 1 度目 3 本 + 再 gate 3 本・引き継ぎの skip record は無い）が、どちらの周も ③ を撃たない。
/// 主実測は同じ木で撃たず、着地後の検出の子は面の外の着地で撃たない＝呼出は再 gate の ②④ だけ。
#[test]
fn pipe_detection_off_gate_regate_after_crates_moved_fires_no_detection() {
    let (repo, state, design) = detection_repo(DETECTION_COUNT);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let before = detection_calls(&repo).len();
    let moved = super::land::advance_main_with(&repo, "crates/toy/src/other.rs");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("rebase={base}..{moved}")), "追随は済む: {}", stdout_of(&out));
    await_detection_child(&state, &id);
    let calls = detection_calls(&repo);
    assert!(detection_marks(&calls).is_empty(), "どの周も検出線の stub を呼ばない: {calls:?}");
    let added = calls.get(before..).unwrap_or_default().to_vec();
    assert_eq!(added, ["common".to_owned(), "contract".to_owned()], "再 gate は ②④ を撃つ: {added:?}");
    let rows = verify_rows(&state, &id);
    assert_eq!(
        kinds(&rows),
        ["write-set", "common", "contract", "write-set", "common", "contract"],
        "1 度目 3 本 + 再 gate 3 本（③ も引き継ぎの skip record も無い）: {rows:?}"
    );
    assert!(skip_rows(&rows).is_empty(), "skip record は無い（再 gate は撃った）: {rows:?}");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

// ---- 検出線は主実測で撃たない（設計 gate-cost.md §44 形 (9)(13)・契約表の行 ao・接頭辞 `pipe_detection_off_main_`）----
//
// 主実測の段は ①②④ で、③ は着地後の検出の口だけが撃つ。検出線を宣言した便を land まで通すと、木が違う周も
// `verify-main.jsonl` の主実測の分（`landed` を持たない）は ①②④ の 3 本で `kind=detection` を持たない（撃った record も
// `skipped=detection` の record も無い）。候補の木の後続の側は `land.rs` の歯（同じ接頭辞）が持つ。
// base は ③ を撃つか（木を比べられない周）`skipped=detection` を書く（面の外の周）＝RED。

/// (p) 木が違う周の主実測は ①②④ だけ: 木を比べられない形（実在しない木＝base は ③ を撃つ）と、木は違うが差分が面の外の
/// 形（便の base の木＝base は ③ の位置に `skipped=detection` を書く）の 2 つ。どちらも stub は主実測から呼ばれず
/// （呼出は ②④）、主実測の record に `landed` の無い `kind=detection` は 0 本。
#[test]
fn pipe_detection_off_main_tree_differs_fires_only_three_stages() {
    let broken: fn(&str, &str, &str) -> String =
        |text, tree, _| text.replace(&format!("\"tree\":\"{tree}\""), &format!("\"tree\":\"{}\"", "0".repeat(40)));
    for (name, edit) in [("unreadable", broken), ("outside-scope", verdict_tree_to_base as fn(&str, &str, &str) -> String)] {
        let landed = detection_land(edit);
        let gated = value_of(&verdict_pairs(&landed.state, &landed.id), "tree");
        assert_ne!(gated, git(&landed.repo, &["rev-parse", "refs/heads/main^{tree}"]), "{name}: fixture は木が違う");
        assert!(detection_marks(&landed.added).is_empty(), "{name}: 主実測は stub を呼ばない: {:?}", landed.added);
        assert_main_three_stages(&landed);
        clean(&[&landed.repo, &landed.state]);
    }
}

/// (a) 測った周: rc 0・`detection=measured`・`landed` 付きの record +1（`line=` は stub の判定行）・stub の `--base` は
/// 着地した commit の親（gate の base と異なる）で `--teeth` は契約の語・共通 verify は撃たない・event 1 件・show の行
/// （判定行 + `secs=`）+1・台帳の見張りは着地の close の 1 件だけ。
#[test]
fn pipe_landed_detection_measured_round_records_the_landed_commit() {
    let run = landed_run(&landed_crates_case());
    let before = landed_before(&run);
    let shown_before = shown_copies(&run);
    let out = detection_only(&run, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "口は rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("run={} detection=measured\n", run.id), "stdout は 1 行");
    let row = added_landed_row(&run, &before);
    assert_eq!(value_of(&row, "line"), LANDED_LINE, "line= は stub の判定行: {row:?}");
    assert_eq!(value_of(&row, "rc"), "0", "{row:?}");
    assert_fired_on_the_parent(&run, &before);
    assert_eq!(added_details(&run, &before), ["detection:measured"], "event は 1 件");
    let round = added_round(&run, &before);
    assert_eq!(read_copy(&run.state, &run.id, round, COPY_LINE), format!("{LANDED_LINE}\n"), "写しの判定行");
    assert!(!copy_path(&run.state, &run.id, round, "reason").exists(), "測れた周は理由の file を置かない");
    let shown = shown_copies(&run);
    assert_eq!(shown.len(), shown_before.len() + 1, "show の行 +1: {shown:?}");
    let last = shown.last().cloned().unwrap_or_default();
    assert!(last.starts_with(&format!("{LANDED_LINE} secs=")), "判定行と secs=: {last}");
    let calls = crate::toolbox_ledger_record_names(&run.state);
    assert_eq!(calls.len(), 1, "台帳 client を起こしたのは着地の close の 1 回だけ（口は起こさない）: {calls:?}");
    assert!(show_line(&run.repo, &run.state, &run.id).contains("stage=Landed"), "段は Landed のまま");
    clean(&[&run.repo, &run.state]);
}

/// (b) 面の外: 着地した diff が docs だけの便は stub を呼ばず、`reason=outside-scope` の skip record・理由の file・
/// `detection:skipped`・show の行は `detection-line: absent skipped=outside-scope`。
#[test]
fn pipe_landed_detection_outside_scope_writes_a_skip_record() {
    let mut case = landed_crates_case();
    case.runner = "echo x >> docs/landed.md && git add -A && git commit -q -m runner".to_owned();
    case.contract = vec![format!("write-set = [\"{LANDED_LIB}\", \"docs/landed.md\"]"), landed_verify()];
    let run = landed_run(&case);
    let before = landed_before(&run);
    let out = detection_only(&run, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "口は rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("run={} detection=skipped\n", run.id), "stdout は 1 行");
    assert_eq!(landed_calls(&run.repo).len(), before.calls, "stub は呼ばれない");
    let row = added_landed_row(&run, &before);
    assert_eq!(value_of(&row, "skipped"), "detection", "skip record: {row:?}");
    assert_eq!(value_of(&row, "reason"), "outside-scope", "理由は面の外: {row:?}");
    let round = added_round(&run, &before);
    assert_eq!(read_copy(&run.state, &run.id, round, "reason"), "skipped=outside-scope\n", "理由の file");
    assert_eq!(added_details(&run, &before), ["detection:skipped"], "event は 1 件");
    let shown = shown_copies(&run);
    assert_eq!(shown.last().cloned().unwrap_or_default(), format!("{COPY_ABSENT_LINE} skipped=outside-scope"), "{shown:?}");
    clean(&[&run.repo, &run.state]);
}

/// (c) 測れなかった周: stub が rc 2 の便は stub が 1 回だけ呼ばれ（撃ち直さない）、record の rc が 2・`unmeasured=rc-2`。
#[test]
fn pipe_landed_detection_rc2_is_unmeasured_and_fired_once() {
    let run = landed_run(&landed_crates_case());
    write_landed_stub(&run.repo, 2);
    let before = landed_before(&run);
    let out = detection_only(&run, None);
    assert_eq!(landed_calls(&run.repo).len(), before.calls + 1, "stub は 1 回だけ呼ばれる");
    let row = assert_landed_unmeasured(&run, &before, &out, "rc-2");
    assert_eq!(value_of(&row, "rc"), "2", "record の rc は 2: {row:?}");
    clean(&[&run.repo, &run.state]);
}

/// (d) 遮断器: 倍率 0 の `--rules`（§32 の歯と同じ形）で撃つと stub は呼ばれず `unmeasured=host-closed` の record 1 本。
#[test]
fn pipe_landed_detection_closed_breaker_fires_nothing() {
    let run = landed_run(&landed_crates_case());
    let slots = SlotFixture { runnable_per_core: 0, blocked_per_core: 0, ..default_slots() };
    let rules = write_rules_full(&run.state, "rules-landed-busy.toml", (1, 1_000_000), FOLLOW_RETRIES, slots);
    let before = landed_before(&run);
    let out = detection_only(&run, Some(&rules));
    assert_eq!(landed_calls(&run.repo).len(), before.calls, "stub は呼ばれない");
    let row = assert_landed_unmeasured(&run, &before, &out, "host-closed");
    assert_eq!(value_of(&row, "unmeasured"), "host-closed", "record は理由を持つ: {row:?}");
    clean(&[&run.repo, &run.state]);
}

/// (d2) 出せない周: 置き場の別名の path に普通の file を置くと stub は呼ばれず `unmeasured=unprepared` の record 1 本。
#[test]
fn pipe_landed_detection_unprepared_worktree_is_unmeasured() {
    let run = landed_run(&landed_crates_case());
    let place = run.repo.join(".worktrees").join("scribe2").join("verify").join(format!("{}-detection", run.id));
    fs::create_dir_all(place.parent().unwrap_or(&run.repo)).ok();
    fs::write(&place, "occupied\n").expect("置き場の別名を file で塞げる");
    let before = landed_before(&run);
    let out = detection_only(&run, None);
    assert_eq!(landed_calls(&run.repo).len(), before.calls, "stub は呼ばれない");
    let row = assert_landed_unmeasured(&run, &before, &out, "unprepared");
    assert_eq!(value_of(&row, "unmeasured"), "unprepared", "record は理由を持つ: {row:?}");
    assert_eq!(fs::read_to_string(&place).unwrap_or_default(), "occupied\n", "塞いだ file に触れない");
    clean(&[&run.repo, &run.state]);
}

/// (e) 的と純移動: 契約が的を持つ便は stub の argv に `--targets <run dir の的の file>`、純移動と証明された便は
/// `--diff <run dir の母集団の file>` が載る（gate と同じ付け足し）。
#[test]
fn pipe_landed_detection_appends_targets_and_the_pure_move_population() {
    let mut aimed = landed_crates_case();
    aimed.contract.push(r#"targets = ["crates/toy/src/lib.rs:1:replace landed with ()"]"#.to_owned());
    let run = landed_run(&aimed);
    let out = detection_only(&run, None);
    assert_eq!(stdout_of(&out), format!("run={} detection=measured\n", run.id), "{}", stderr_of(&out));
    let file = run_dir(&run.state, &run.id).join("targets");
    let call = landed_calls(&run.repo).last().cloned().unwrap_or_default();
    assert!(call.ends_with(&format!(" --targets {}", file.display())), "的の file: {call}");
    clean(&[&run.repo, &run.state]);

    let moved = LandedCase {
        base: vec![(LANDED_LIB, POP_BASE_LIB.to_owned())],
        runner: "cp '{}'/*.rs crates/toy/src/ && git add -A && git commit -q -m runner".to_owned(),
        contract: vec![
            format!("write-set = [\"{LANDED_LIB}\", \"crates/toy/src/alpha.rs\"]"),
            r#"verify = ["sh verify-ok.sh"]"#.to_owned(),
        ],
    };
    let run = landed_pure_move(moved);
    let before = landed_before(&run);
    let out = detection_only(&run, None);
    assert_eq!(stdout_of(&out), format!("run={} detection=measured\n", run.id), "{}", stderr_of(&out));
    let file = run_dir(&run.state, &run.id).join("population.diff");
    let call = landed_calls(&run.repo).last().cloned().unwrap_or_default();
    assert!(call.ends_with(&format!(" --diff {}", file.display())), "母集団の file: {call}");
    let row = added_landed_row(&run, &before);
    assert_eq!(value_of(&row, "pure-move"), "3", "落とした `+` 行の本数: {row:?}");
    clean(&[&run.repo, &run.state]);
}

/// (f) 断り: 段が `Gated` の便と、宣言に検出線の行が無い便は rc 1 で、record・写し・event がどれも増えない。
#[test]
fn pipe_landed_detection_refuses_gated_and_undeclared_runs() {
    let gated = landed_gated(&landed_crates_case());
    let before = landed_before(&gated);
    let out = detection_only(&gated, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "Gated は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("Gated"), "段を名指す: {}", stderr_of(&out));
    assert_landed_untouched(&gated, &before);
    clean(&[&gated.repo, &gated.state]);

    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    let sha = git(&repo, &["rev-parse", "refs/heads/main"]);
    let bare = LandedRun { repo, state, id, sha, gate_base: String::new() };
    let before = landed_before(&bare);
    let out = detection_only(&bare, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "検出線の無い便は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("検出線の行が無い"), "理由: {}", stderr_of(&out));
    assert_landed_untouched(&bare, &before);
    clean(&[&bare.repo, &bare.state]);
}

/// (k) land は stub が終わる前に rc 0 で `Landed` を返し、返った時点で `detection:spawned` 1 件・終えた語 0 件。解放の後に
/// 子の終わりを待つと `detection:measured` 1 件と `landed` を持つ record 1 本（`line=` は stub の判定行・stub の `--base` は
/// 着地した commit の親）。gate は ③ を撃たない（`verify.jsonl` に `kind=detection` は無い・行 an）・台帳の見張り 0 件。
#[test]
fn pipe_detection_after_landing_land_returns_before_the_child_finishes() {
    let mut run = landed_gated(&landed_crates_case());
    write_blocking_stub(&run.repo);
    let out = land_after_docs_move(&run);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {} / {}", stdout_of(&out), stderr_of(&out));
    run.sha = git(&run.repo, &["rev-parse", "refs/heads/main"]);
    assert!(stdout_of(&out).contains(&format!("landed={}", run.sha)), "着地を返す: {}", stdout_of(&out));
    assert!(show_line(&run.repo, &run.state, &run.id).contains("stage=Landed"), "段は Landed");
    assert_eq!(detection_details(&run.state, &run.id), [SPAWNED_DETAIL], "返った時点: spawned 1 件・終えた語 0 件");
    fs::write(run.repo.join(".git").join(LANDED_RELEASE), "").expect("解放の file を置ける");
    await_detection_child(&run.state, &run.id);
    assert_eq!(
        detection_details(&run.state, &run.id),
        [SPAWNED_DETAIL, "detection:measured"],
        "解放の後: 子が measured を 1 件記す"
    );
    assert_child_measured_the_landed_commit(&run);
    clean(&[&run.repo, &run.state]);
}

/// (k) の対: land が受けた `--rules` は子へ同じ値で渡る。遮断器を閉じる `--rules`（行 ak の (d) と同じ形）で land すると、
/// 主実測は同じ木で 1 本も撃たずに着地し、子は stub を呼ばず `unmeasured=host-closed` の record 1 本を残す（渡さない変異は
/// 埋め込みの規則で撃って measured になる）。
#[test]
fn pipe_detection_after_landing_child_reads_the_rules_land_received() {
    let run = landed_gated(&landed_crates_case());
    let slots = SlotFixture { runnable_per_core: 0, blocked_per_core: 0, ..default_slots() };
    let rules = write_rules_full(&run.state, "rules-landed-busy.toml", (1, 1_000_000), FOLLOW_RETRIES, slots);
    let (repo_arg, state_arg, rules_arg) =
        (run.repo.display().to_string(), run.state.display().to_string(), rules.display().to_string());
    let out = run_pipe_with_path(
        &landed_path(&run.state),
        &["land", "--run", &run.id, "--repo", &repo_arg, "--state-dir", &state_arg, "--rules", &rules_arg],
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {} / {}", stdout_of(&out), stderr_of(&out));
    await_detection_child(&run.state, &run.id);
    assert_eq!(
        detection_details(&run.state, &run.id),
        [SPAWNED_DETAIL, "detection:unmeasured"],
        "子は同じ規則の遮断器で測れない"
    );
    assert!(landed_calls(&run.repo).is_empty(), "stub は 1 回も呼ばれない（gate は ③ を撃たず・子は遮断器で撃たない）");
    let (_, after) = split_landed(main_rows(&run.state, &run.id));
    assert_eq!(after.len(), 1, "landed を持つ record は 1 本: {after:?}");
    let row = after.first().cloned().unwrap_or_default();
    assert_eq!(value_of(&row, "unmeasured"), "host-closed", "記録は遮断器の理由: {row:?}");
    clean(&[&run.repo, &run.state]);
}

/// (l) 検出線を宣言しない便の land は `detection:` で始まる detail を 1 件も書かない（Landed の detail は着地そのものの行
/// だけの従来の並び・終端の `terminal:` の行は除いて比べる）。
#[test]
fn pipe_detection_after_landing_undeclared_run_writes_no_detection_detail() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert!(detection_details(&state, &id).is_empty(), "detection の detail は無い: {:?}", trail(&state, &id));
    let sha = git(&repo, &["rev-parse", "refs/heads/main"]);
    let landed: Vec<String> = trail(&state, &id)
        .into_iter()
        .filter(|(kind, stage, _)| *kind == EventKind::RunDone && *stage == Some(Stage::Landed))
        .filter_map(|(_, _, detail)| detail)
        .filter(|detail| !detail.starts_with("terminal:"))
        .collect();
    assert_eq!(landed, [format!("sha:{sha} main:{sha}")], "Landed の detail は着地の 1 件だけ");
    assert!(split_landed(main_rows(&state, &id)).1.is_empty(), "landed を持つ record は無い");
    clean(&[&repo, &state]);
}

/// (m) main-red で終えた便は `finish` に届かない＝`detection:spawned` を持たず、`landed` を持つ record も無い。
#[test]
fn pipe_detection_after_landing_main_red_run_spawns_nothing() {
    let (repo, state) = repo_with_state();
    commit_detection_vessel(&repo, DETECTION_COUNT);
    // 1 回目（gate）は緑・2 回目（主実測）は赤の契約 verify。主実測を撃つ周にするため verdict の木を main の木へ差し替える。
    let design = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-once.sh"]"#]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    super::land::make_tree_differ(&repo, &state, &id, "refs/heads/main");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "main が赤い land は rc 1: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("main-red".to_owned()))),
        "終端の理由は main-red: {:?}",
        stages(&state, &id)
    );
    assert!(detection_details(&state, &id).is_empty(), "spawned を持たない: {:?}", trail(&state, &id));
    assert!(split_landed(main_rows(&state, &id)).1.is_empty(), "landed を持つ record は無い");
    clean(&[&repo, &state]);
}

/// (7) `detection-verify` の行にも共通 verify と**同じ検査**を掛け、同じ理由の字面で rc 1 に断る。
#[test]
fn pipe_detection_intake_refuses_unfit_lines() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    for (detection, want) in [
        // `cargo` は上限には在るが、この repo の宣言の allowlist には無い。
        (r#"["cargo xtask mutants-diff --base {base}"]"#.to_owned(), "先頭 command cargo が"),
        ("[\"git --version\u{7}\"]".to_owned(), "制御文字"),
        (r#"["git rev-parse --git-dir ../../../etc"]"#.to_owned(), "repo の外"),
    ] {
        commit_detection_vessel(&repo, &detection);
        let out = intake_raw(&repo, &state, &path, "b");
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{detection} は rc 1: {err}");
        assert!(err.contains(want), "{detection} の理由は共通 verify と同じ字面 {want}: {err}");
        assert!(err.contains("detection-verify"), "どの key の行かを名指す: {err}");
    }
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    // 弁別: 検査を通る行なら同じ宣言の形で便が起きる（key そのものを断っているのではない）。
    commit_detection_vessel(&repo, DETECTION_COUNT);
    let ok = intake_raw(&repo, &state, &path, "b");
    assert_eq!(ok.status.code(), Some(i32::from(RC_OK)), "検査を通る検出線は読める: {}", stderr_of(&ok));
    clean(&[&repo, &state]);
}
