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

/// 入れ子の根の下の file（[`LANDED_LIB`] の歯は契約の verify の filter 語のために残す）だけを runner が足す便を gate まで通し、
/// `declared` なら gate の後に main の宣言へ key `crate-roots = ["nest/crates/"]` を足して commit し（HEAD の宣言が読み面）、
/// 面の外の docs の 1 commit で main を進めて land する（子の終わりを待つ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn landed_nested_run(declared: bool) -> LandedRun {
    let nested = "nest/crates/toy/src/a.rs";
    let mut case = landed_crates_case();
    case.base.push((nested, "// nested\n".to_owned()));
    case.runner = format!("echo '// landed' >> {nested} && git add -A && git commit -q -m runner");
    case.contract = vec![format!("write-set = [\"{LANDED_LIB}\", \"{nested}\"]"), landed_verify()];
    let mut run = landed_gated(&case);
    if declared {
        let text = fs::read_to_string(run.repo.join(".vessel.toml")).expect("宣言を読める");
        fs::write(run.repo.join(".vessel.toml"), format!("{text}crate-roots = [\"nest/crates/\"]\n")).expect("宣言を書ける");
        git(&run.repo, &["add", "-f", ".vessel.toml"]);
        git(&run.repo, &["commit", "-q", "-m", "crate-roots"]);
    }
    let landed = land_after_docs_move(&run);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {} / {}", stdout_of(&landed), stderr_of(&landed));
    await_detection_child(&run.state, &run.id);
    run.sha = git(&run.repo, &["rev-parse", "refs/heads/main"]);
    run
}

/// 宣言した根の下だけの着地は検出線の面の内: 口が stub を 1 回呼び、`detection=measured`・`landed` 付きの record +1。
#[test]
fn pipe_landed_detection_crate_roots_declared_root_fires_the_stub_once() {
    let run = landed_nested_run(true);
    let before = landed_before(&run);
    let out = detection_only(&run, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "口は rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("run={} detection=measured\n", run.id), "stdout は 1 行");
    assert_eq!(landed_calls(&run.repo).len(), before.calls + 1, "stub は 1 回呼ばれる");
    let row = added_landed_row(&run, &before);
    assert_eq!(value_of(&row, "line"), LANDED_LINE, "line= は stub の判定行: {row:?}");
    clean(&[&run.repo, &run.state]);
}

/// key の無い repo の同じ便は今どおり面の外: stub を呼ばず `reason=outside-scope` の skip record を書く（対照）。
#[test]
fn pipe_landed_detection_crate_roots_without_the_key_writes_outside_scope() {
    let run = landed_nested_run(false);
    let before = landed_before(&run);
    let out = detection_only(&run, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "口は rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("run={} detection=skipped\n", run.id), "stdout は 1 行");
    assert_eq!(landed_calls(&run.repo).len(), before.calls, "stub は呼ばれない");
    let row = added_landed_row(&run, &before);
    assert_eq!(value_of(&row, "reason"), "outside-scope", "理由は面の外: {row:?}");
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

// ---- 検出線を 1 日 1 回に間引く（設計 gate-cost.md §50・契約表の行 au・接頭辞 `pipe_detection_daily_`）----------------
//
// 1 つの repo と置き場に便を順に着地させる（便ごとに intake → spawn → gate → land）。検出線の stub は共通 dir に argv を
// 積むので、測るのは stub の argv と呼出数・検出の起点（`pipe/detection-origin/origin`）の字・便ごとの `detection:` の detail・
// `landed` 付きの record・写しの周の file。起点の fired は歯が書き換えて下限（86400 秒）を過ぎた周を作る。base の land は着地
// ごとに起こし、起点を書かず、`{base}` を親にする＝(h)(i) の対照を除いて RED。

/// 歯の便 1 本の形（契約表の行・runner の差分・契約の欄）。
struct DailyCar {
    /// 契約表の行 id（`docs/design/toy.md#<行>`・file 名にも使う）。
    row: &'static str,
    /// 台帳の bead（便 id は `<bead>-<秒>` なので便ごとに分ける）。
    bead: &'static str,
    /// 契約の verify 行の filter 語（宣言順・stub の `--teeth` に載る）。
    words: &'static [&'static str],
    /// runner の差分が docs だけか（面の外の着地）。
    docs_only: bool,
    /// 契約が的を持つか。
    targets: bool,
    /// write-set に足す別の便の歯の置き場の行（その便の語を verify に持つ契約が、歯の file を write-set に持つため）。
    also: Option<&'static str>,
}

impl DailyCar {
    /// crates の面の内に 1 行足す便。
    fn crates(row: &'static str, bead: &'static str, words: &'static [&'static str]) -> Self {
        Self { row, bead, words, docs_only: false, targets: false, also: None }
    }

    /// runner の差分が docs だけの便。
    fn docs(row: &'static str, bead: &'static str, words: &'static [&'static str]) -> Self {
        Self { docs_only: true, ..Self::crates(row, bead, words) }
    }

    /// 便が足す歯の置き場（filter 語ごとに 1 本の `#[test]` fn を持つ）。
    fn file(&self) -> String {
        format!("crates/toy/src/{}.rs", self.row)
    }

    /// 契約の欄（write-set と verify と任意の的）。
    fn fields(&self) -> Vec<String> {
        let mut write_set = vec![format!("\"{}\"", self.file())];
        write_set.extend(self.also.map(|row| format!("\"crates/toy/src/{row}.rs\"")));
        if self.docs_only {
            write_set.push(format!("\"docs/{}.md\"", self.row));
        }
        let verify: Vec<String> =
            self.words.iter().map(|word| format!("\"cargo nextest run -p toy --lib --no-tests=fail {word}\"")).collect();
        let mut fields = vec![format!("write-set = [{}]", write_set.join(", ")), format!("verify = [{}]", verify.join(", "))];
        if self.targets {
            let word = self.words.first().copied().unwrap_or_default();
            fields.push(format!("targets = [\"{}:1:replace {word}t with ()\"]", self.file()));
        }
        fields
    }

    /// runner の 1 行（commit を 1 本作る）。
    fn runner(&self) -> String {
        if self.docs_only {
            return format!("echo '<!-- landed -->' >> docs/{}.md && git add -A && git commit -q -m runner", self.row);
        }
        format!("echo '// landed' >> {} && git add -A && git commit -q -m runner", self.file())
    }

    /// base に置く歯の置き場の本文（先頭の filter 語の `#[test]` fn が 1 本・別の便の語は別の便の file が持つ＝歯の file が
    /// 便をまたいで重ならない）。
    fn body(&self) -> String {
        let word = self.words.first().copied().unwrap_or_default();
        format!("#[cfg(test)]\nmod tests {{\n    #[test]\n    fn {word}t() {{}}\n}}\n")
    }
}

/// 検出線を宣言した toy repo に、歯の便の契約の行を全部 commit する（便は [`daily_land`] が順に着地させる）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn daily_world(cars: &[DailyCar]) -> (PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    let trampoline = format!("sh \"$(git rev-parse --git-common-dir)/{LANDED_STUB}\" \"$@\"\n");
    fs::write(repo.join(LANDED_TRAMPOLINE), trampoline).expect("跳び板を書ける");
    for car in cars {
        let file = repo.join(car.file());
        fs::create_dir_all(file.parent().expect("親 dir が在る")).expect("dir を作れる");
        fs::write(&file, car.body()).expect("歯の置き場を書ける");
    }
    write_vessel(&repo, r#"["git", "sh", "cargo"]"#, r#"["sh verify-count.sh common"]"#);
    let path = repo.join(".vessel.toml");
    let body = fs::read_to_string(&path).expect("宣言を読める");
    fs::write(&path, format!("{body}detection-verify = {LANDED_DETECTION}\n")).expect("宣言を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "vessel-daily"]);
    write_landed_stub(&repo, 0);
    let rows: Vec<Vec<String>> = cars
        .iter()
        .map(|car| {
            let fields = car.fields();
            let borrowed: Vec<&str> = fields.iter().map(String::as_str).collect();
            row_fields(car.row, &["write-set", "verify"], &borrowed)
        })
        .collect();
    commit_rows(&repo, &rows);
    (repo, state)
}

/// 歯の便を gate まで通して `land` を 1 回撃つ（子の終わりは待たない・`rules` は land だけに渡す）。返す `LandedRun` の `sha` は
/// land の後の main の先端（着地した squash）。
fn daily_land(world: &(PathBuf, PathBuf), car: &DailyCar, rules: Option<&Path>) -> (LandedRun, Output) {
    let (repo, state) = world;
    let id = intake_bead(repo, state, &format!("{DESIGN_FILE}#{}", car.row), car.bead);
    let gate_base = git(repo, &["rev-parse", "refs/heads/main"]);
    let spawned = spawn_with(repo, state, &id, &car.runner());
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "{} の spawn: {}", car.row, stderr_of(&spawned));
    let (repo_arg, state_arg) = (repo.display().to_string(), state.display().to_string());
    let lens = fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"));
    let gated = run_pipe_with_path(
        &landed_path(state),
        &["gate", "--run", &id, "--repo", &repo_arg, "--state-dir", &state_arg, "--lens", &lens],
    );
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "{} の gate: {}", car.row, stderr_of(&gated));
    let mut args = vec!["land", "--run", id.as_str(), "--repo", &repo_arg, "--state-dir", &state_arg];
    let rules_arg = rules.map(|path| path.display().to_string());
    if let Some(found) = &rules_arg {
        args.extend(["--rules", found.as_str()]);
    }
    let out = run_pipe_with_path(&landed_path(state), &args);
    let sha = git(repo, &["rev-parse", "refs/heads/main"]);
    (LandedRun { repo: repo.clone(), state: state.clone(), id, sha, gate_base }, out)
}

/// [`daily_land`] を rc 0 で通し、子の終わりを待つ。
fn daily_landed(world: &(PathBuf, PathBuf), car: &DailyCar) -> LandedRun {
    let (run, out) = daily_land(world, car, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} の land: {} / {}", car.row, stdout_of(&out), stderr_of(&out));
    await_detection_child(&run.state, &run.id);
    run
}

/// 検出の起点の file。
fn origin_file(state: &Path) -> PathBuf {
    state.join("pipe").join("detection-origin").join("origin")
}

/// 検出の起点の字（無い周は空）。
fn origin_text(state: &Path) -> String {
    fs::read_to_string(origin_file(state)).unwrap_or_default()
}

/// 起点の 1 語目（`measured=<sha>`・起点が無い周は空）。
fn origin_measured(state: &Path) -> String {
    origin_text(state).split_whitespace().next().unwrap_or_default().to_owned()
}

/// 今の epoch 秒。
fn epoch_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |found| found.as_secs())
}

/// 起点を書き換える（置き場の dir を作る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn set_origin(state: &Path, measured: &str, fired: u64) {
    let file = origin_file(state);
    fs::create_dir_all(file.parent().expect("親 dir が在る")).expect("起点の dir を作れる");
    fs::write(&file, format!("measured={measured} fired={fired}\n")).expect("起点を書ける");
}

/// 起点の fired を下限（86400 秒）の外へ戻す（measured は読んだ値のまま）。
fn age_origin(state: &Path) {
    let measured = origin_measured(state);
    set_origin(state, measured.strip_prefix("measured=").unwrap_or("-"), epoch_now().saturating_sub(86_400));
}

/// 起点の字が `measured=<sha> fired=<秒>` の改行で終わる 1 行で、fired が `[from, to]` の内（land の前後の間）。
fn assert_origin_in(state: &Path, measured: &str, window: (u64, u64)) {
    let text = origin_text(state);
    let read: Option<u64> = text
        .strip_suffix('\n')
        .and_then(|line| line.strip_prefix(&format!("measured={measured} fired=")))
        .and_then(|rest| rest.parse().ok());
    assert!(read.is_some(), "起点は measured={measured} fired=<秒> の 1 行: {text:?}");
    let fired = read.unwrap_or_default();
    assert!(window.0 <= fired && fired <= window.1, "fired は land の前後の間: {fired} not in {window:?}");
}

/// land の stdout から便 id と sha を伏せた形（便の違いを除いた字面の比較）。
fn shaped(out: &Output, run: &LandedRun) -> String {
    stdout_of(out).replace(&run.id, "<id>").replace(&run.sha, "<sha>")
}

/// 便の `landed` 付きの record（口の撃った周の record）。
fn landed_rows(run: &LandedRun) -> Rows {
    split_landed(main_rows(&run.state, &run.id)).1
}

/// 起点の lock file を歯の process の生きた pid で置く（所有者が生きている lock は外されない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn hold_origin_lock(state: &Path) -> PathBuf {
    let lock = state.join("pipe").join("detection-origin").join("origin.lock");
    fs::create_dir_all(lock.parent().expect("親 dir が在る")).expect("起点の dir を作れる");
    fs::write(&lock, format!("{}\n", std::process::id())).expect("lock を置ける");
    lock
}

/// main を docs だけの 1 commit で進める（次の便の親が前の便の squash でなくなる）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn move_main_with_docs(repo: &Path, name: &str) -> String {
    fs::create_dir_all(repo.join("docs")).expect("docs を作れる");
    fs::write(repo.join("docs").join(name), "moved\n").expect("別便の docs を書ける");
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "docs-move"]);
    git(repo, &["rev-parse", "refs/heads/main"])
}

/// stub の最後の argv（無い周は空）。
fn last_argv(repo: &Path) -> String {
    landed_calls(repo).last().cloned().unwrap_or_default()
}

/// 検出線の便の 3 本（行 a・b・c）の歯の世界。
fn daily_trio() -> (PathBuf, PathBuf) {
    daily_world(&[
        DailyCar::crates("a", "s2-2e5", &["landed_a_"]),
        DailyCar::crates("b", "s2-3ax", &["landed_b_"]),
        DailyCar::crates("c", "s2-4cz", &["landed_c_"]),
    ])
}

/// (3)(5) 着地の A は起点が無いので口を起こし（spawned・起点 `measured=<A> fired=<A の land の前後の間>`・stub の argv は
/// `--base <A の親> --teeth landed_a_`）、直後の B は fired + 86400 > 今なので口を起こさない: rc 0・stdout は A と同じ形・
/// detail は `detection:deferred` の 1 件だけ（spawned 0 件）・stub の呼出が増えない・`landed` を持つ record 0 本・理由の file
/// が `skipped=deferred` と改行で判定行の file が無く show の行はちょうど `detection-line: absent skipped=deferred`・
/// 起点の bytes は land の前後で等しい。
#[test]
fn pipe_detection_daily_defers_a_landing_inside_the_floor() {
    let world = daily_trio();
    let t0 = epoch_now();
    let (a, out_a) = daily_land(&world, &DailyCar::crates("a", "s2-2e5", &["landed_a_"]), None);
    assert_eq!(out_a.status.code(), Some(i32::from(RC_OK)), "A の land: {}", stderr_of(&out_a));
    await_detection_child(&a.state, &a.id);
    let t1 = epoch_now();
    assert_eq!(detection_details(&a.state, &a.id), [SPAWNED_DETAIL, "detection:measured"], "A は起こして測る");
    let parent = git(&a.repo, &["rev-parse", &format!("{}^", a.sha)]);
    assert_eq!(landed_calls(&a.repo), [format!("--base {parent} --teeth landed_a_")], "A の argv は親と契約の語");
    assert_origin_in(&a.state, &a.sha, (t0, t1));
    let origin = origin_text(&a.state);
    let (b, out_b) = daily_land(&world, &DailyCar::crates("b", "s2-3ax", &["landed_b_"]), None);
    assert_eq!(out_b.status.code(), Some(i32::from(RC_OK)), "B の land は rc 0: {}", stderr_of(&out_b));
    assert_eq!(shaped(&out_b, &b), shaped(&out_a, &a), "stdout は A と同じ形");
    assert_eq!(detection_details(&b.state, &b.id), ["detection:deferred"], "B は deferred の 1 件で spawned を持たない");
    assert_eq!(landed_calls(&b.repo).len(), 1, "stub の呼出は増えない: {:?}", landed_calls(&b.repo));
    assert!(landed_rows(&b).is_empty(), "landed を持つ record は 0 本: {:?}", main_rows(&b.state, &b.id));
    let rounds = copy_rounds(&b.state, &b.id);
    assert_eq!(rounds.len(), 1, "写しの周は 1 つ: {rounds:?}");
    let round = rounds.last().copied().unwrap_or_default();
    assert_eq!(read_copy(&b.state, &b.id, round, "reason"), "skipped=deferred\n", "理由の file");
    assert!(!copy_path(&b.state, &b.id, round, "line").exists(), "判定行の file は置かない");
    assert_eq!(shown_copies(&b), [format!("{COPY_ABSENT_LINE} skipped=deferred")], "show の行");
    assert_eq!(origin_text(&b.state), origin, "起点は動かない");
    clean(&[&a.repo, &a.state]);
}

/// (3)(6)(8)(9)(10) 下限を過ぎた C は口を起こし、口は起点の measured（A）から C までに着地した便 B・C をまとめて撃つ:
/// stub の argv は `--base <A> --teeth landed_b_,landed_c_`（C の親 B と異なる・的を持つ契約でも `--targets` が無く、C 自身の
/// verify の語の順 c → b でなく列の順 b → c）。C の record は `since=<A>` と `runs=2`、写しの `since` は A と改行・`runs` は
/// B と C の便 id の 2 行、show の行は `<判定行> secs=<秒> since=<A の先頭 7 字> runs=2`、起点は `measured=<C> fired=<C の land の
/// 前後の間>`。
#[test]
fn pipe_detection_daily_fires_after_the_floor_and_measures_from_the_origin() {
    let c_car = DailyCar { targets: true, also: Some("b"), ..DailyCar::crates("c", "s2-4cz", &["landed_c_", "landed_b_"]) };
    let world = daily_world(&[
        DailyCar::crates("a", "s2-2e5", &["landed_a_"]),
        DailyCar::crates("b", "s2-3ax", &["landed_b_"]),
        DailyCar { targets: true, also: Some("b"), ..DailyCar::crates("c", "s2-4cz", &["landed_c_", "landed_b_"]) },
    ]);
    let a = daily_landed(&world, &DailyCar::crates("a", "s2-2e5", &["landed_a_"]));
    let b = daily_landed(&world, &DailyCar::crates("b", "s2-3ax", &["landed_b_"]));
    assert_eq!(detection_details(&b.state, &b.id), ["detection:deferred"], "B は間引かれる");
    age_origin(&a.state);
    let t0 = epoch_now();
    let c = daily_landed(&world, &c_car);
    let t1 = epoch_now();
    assert_eq!(detection_details(&c.state, &c.id), [SPAWNED_DETAIL, "detection:measured"], "C は起こして測る");
    assert_eq!(git(&c.repo, &["rev-parse", &format!("{}^", c.sha)]), b.sha, "fixture: C の親は B");
    assert_eq!(last_argv(&c.repo), format!("--base {} --teeth landed_b_,landed_c_", a.sha), "列の語で A から");
    assert_eq!(landed_calls(&c.repo).len(), 2, "stub は A と C の 2 回: {:?}", landed_calls(&c.repo));
    let rows = landed_rows(&c);
    assert_eq!(rows.len(), 1, "C の landed を持つ record は 1 本: {rows:?}");
    let row = rows.first().cloned().unwrap_or_default();
    assert_eq!((value_of(&row, "since"), value_of(&row, "runs")), (a.sha.clone(), "2".to_owned()), "record: {row:?}");
    let round = copy_rounds(&c.state, &c.id).last().copied().unwrap_or_default();
    assert_eq!(read_copy(&c.state, &c.id, round, "since"), format!("{}\n", a.sha), "写しの since");
    assert_eq!(read_copy(&c.state, &c.id, round, "runs"), format!("{}\n{}\n", b.id, c.id), "写しの runs は便 id を古い順に");
    let shown = shown_copies(&c).last().cloned().unwrap_or_default();
    let tail = format!(" since={} runs=2", a.sha.chars().take(7).collect::<String>());
    let secs = shown.strip_prefix(&format!("{LANDED_LINE} secs=")).and_then(|rest| rest.strip_suffix(&tail));
    assert!(secs.is_some_and(|found| found.parse::<u64>().is_ok()), "show の行は判定行 + secs + since + runs: {shown}");
    assert_origin_in(&c.state, &c.sha, (t0, t1));
    clean(&[&c.repo, &c.state]);
}

/// (4)（対照）起点の lock file を歯の process の生きた pid で置いた周の land は、起点を書かずに口を起こす: rc 0・stderr に
/// 理由の 1 行・spawned が 1 件・返った時点で起点の file が無く、子も lock を取れないので終えた後も起点は無い。
#[test]
fn pipe_detection_daily_fires_without_writing_while_the_origin_lock_is_held() {
    let world = daily_world(&[DailyCar::crates("a", "s2-2e5", &["landed_a_"])]);
    let lock = hold_origin_lock(&world.1);
    let (a, out) = daily_land(&world, &DailyCar::crates("a", "s2-2e5", &["landed_a_"]), None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "lock を取れない周も rc 0: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("検出の起点"), "stderr に理由の 1 行: {}", stderr_of(&out));
    assert_eq!(detection_details(&a.state, &a.id), [SPAWNED_DETAIL], "口は起こす（spawned 1 件）");
    assert!(!origin_file(&a.state).exists(), "返った時点で起点の file は無い");
    await_detection_child(&a.state, &a.id);
    assert_eq!(detection_details(&a.state, &a.id), [SPAWNED_DETAIL, "detection:measured"], "子は測る");
    assert!(!origin_file(&a.state).exists(), "子も起点を書かない");
    assert!(lock.exists(), "他者の lock は外さない");
    clean(&[&a.repo, &a.state]);
}

/// (6) 起点を消して main を docs で進めた B の口は、記録の移行の種（detail `detection:measured` を持ち着地した sha が B の
/// 真の祖先の最初の便 A）を {base} にする（B の親は docs の commit で A と異なる）。在らない 40 字の sha の measured の後の
/// C は祖先の関係が無い周なので記録を辿り、B を base にする（C の親と同じ＝今の形）。
#[test]
fn pipe_detection_daily_seeds_from_the_newest_measured_run() {
    let world = daily_trio();
    let a = daily_landed(&world, &DailyCar::crates("a", "s2-2e5", &["landed_a_"]));
    assert!(fs::remove_file(origin_file(&a.state)).is_ok(), "起点を消せる");
    let moved = move_main_with_docs(&a.repo, "moved.md");
    let b = daily_landed(&world, &DailyCar::crates("b", "s2-3ax", &["landed_b_"]));
    assert_eq!(git(&b.repo, &["rev-parse", &format!("{}^", b.sha)]), moved, "fixture: B の親は docs の commit");
    assert_ne!(moved, a.sha, "fixture: B の親は A でない");
    assert_eq!(last_argv(&b.repo), format!("--base {} --teeth landed_b_", a.sha), "B の base は A");
    set_origin(&b.state, &"7".repeat(40), epoch_now().saturating_sub(86_400));
    let c = daily_landed(&world, &DailyCar::crates("c", "s2-4cz", &["landed_c_"]));
    assert_eq!(detection_details(&c.state, &c.id), [SPAWNED_DETAIL, "detection:measured"], "C は起こして測る");
    assert_eq!(last_argv(&c.repo), format!("--base {} --teeth landed_c_", b.sha), "C の base は B");
    clean(&[&c.repo, &c.state]);
}

/// (6) 起点の lock file を生きた pid で置いた周の人が撃つ口（C は measured と等しい）は、起点を読めない周と同じで記録を辿り、
/// base は A・{teeth} は B と C の語の和（`--base <A> --teeth landed_b_,landed_c_`）・rc 0・起点の bytes は口の前後で等しい。
#[test]
fn pipe_detection_daily_mouth_seeds_while_the_origin_lock_is_held() {
    let world = daily_trio();
    let a = daily_landed(&world, &DailyCar::crates("a", "s2-2e5", &["landed_a_"]));
    daily_landed(&world, &DailyCar::crates("b", "s2-3ax", &["landed_b_"]));
    age_origin(&a.state);
    let c = daily_landed(&world, &DailyCar::crates("c", "s2-4cz", &["landed_c_"]));
    assert_eq!(origin_measured(&c.state), format!("measured={}", c.sha), "起点の measured は C");
    let origin = origin_text(&c.state);
    let lock = hold_origin_lock(&c.state);
    let out = detection_only(&c, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "口は rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("run={} detection=measured\n", c.id), "stdout は 1 行");
    assert_eq!(last_argv(&c.repo), format!("--base {} --teeth landed_b_,landed_c_", a.sha), "起点を読めない周は移行の種 A から");
    assert_eq!(origin_text(&c.state), origin, "起点の bytes は口の前後で等しい");
    fs::remove_file(lock).ok();
    clean(&[&c.repo, &c.state]);
}

/// (7) 口は下限を読まず、起点より古い便と measured と等しい便は §44 の今の形で撃つ（起点を後ろへ戻さない）: measured が C の
/// 置き場で、起点より古い B の口は rc 0・`detection=measured`・`--base <D（B の親の docs の commit）> --teeth landed_b_`、
/// measured と等しい C の口は `--base <B> --teeth landed_c_`。起点の bytes は 2 回の口の前後で等しい。
#[test]
fn pipe_detection_daily_old_run_fires_in_the_current_form_by_hand() {
    let world = daily_trio();
    let a = daily_landed(&world, &DailyCar::crates("a", "s2-2e5", &["landed_a_"]));
    let docs = move_main_with_docs(&a.repo, "moved.md");
    let b = daily_landed(&world, &DailyCar::crates("b", "s2-3ax", &["landed_b_"]));
    assert_eq!(git(&b.repo, &["rev-parse", &format!("{}^", b.sha)]), docs, "fixture: B の親は docs の commit");
    age_origin(&a.state);
    let c = daily_landed(&world, &DailyCar::crates("c", "s2-4cz", &["landed_c_"]));
    let origin = origin_text(&c.state);
    assert_eq!(origin_measured(&c.state), format!("measured={}", c.sha), "measured は C の置き場");
    let out = detection_only(&b, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "B の口は rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), format!("run={} detection=measured\n", b.id), "stdout は 1 行");
    assert_eq!(last_argv(&b.repo), format!("--base {docs} --teeth landed_b_"), "B は今の形");
    assert_eq!(origin_text(&b.state), origin, "起点を後ろへ戻さない");
    let out = detection_only(&c, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "C の口は rc 0: {}", stderr_of(&out));
    assert_eq!(last_argv(&c.repo), format!("--base {} --teeth landed_c_", b.sha), "C は今の形");
    assert_eq!(origin_text(&c.state), origin, "起点の bytes は 2 回の口の前後で等しい");
    clean(&[&c.repo, &c.state]);
}

/// (8) 自分の差分が docs だけの C でも、間引かれた B の差分（`crates/`）が base..着地した sha に入るので面の内で、stub を 1 回
/// 呼び `detection:measured`・argv は `--base <A> --teeth landed_b_,landed_c_`。
#[test]
fn pipe_detection_daily_scope_spans_the_deferred_runs() {
    let world = daily_world(&[
        DailyCar::crates("a", "s2-2e5", &["landed_a_"]),
        DailyCar::crates("b", "s2-3ax", &["landed_b_"]),
        DailyCar::docs("c", "s2-4cz", &["landed_c_"]),
    ]);
    let a = daily_landed(&world, &DailyCar::crates("a", "s2-2e5", &["landed_a_"]));
    daily_landed(&world, &DailyCar::crates("b", "s2-3ax", &["landed_b_"]));
    age_origin(&a.state);
    let before = landed_calls(&a.repo).len();
    let c = daily_landed(&world, &DailyCar::docs("c", "s2-4cz", &["landed_c_"]));
    assert_eq!(detection_details(&c.state, &c.id), [SPAWNED_DETAIL, "detection:measured"], "面の内として測る");
    assert_eq!(landed_calls(&c.repo).len(), before + 1, "stub は 1 回: {:?}", landed_calls(&c.repo));
    assert_eq!(last_argv(&c.repo), format!("--base {} --teeth landed_b_,landed_c_", a.sha), "列の語");
    clean(&[&c.repo, &c.state]);
}

/// (9) 測れなかった周は起点を進めない: stub が rc 2 の B の後も起点は `measured=<A>` のままで、次の C の argv は
/// `--base <A> --teeth landed_b_,landed_c_`（B の差分は次の日次の検出に含まれる）。
#[test]
fn pipe_detection_daily_keeps_the_origin_on_an_unmeasured_round() {
    let world = daily_trio();
    let a = daily_landed(&world, &DailyCar::crates("a", "s2-2e5", &["landed_a_"]));
    age_origin(&a.state);
    write_landed_stub(&a.repo, 2);
    let b = daily_landed(&world, &DailyCar::crates("b", "s2-3ax", &["landed_b_"]));
    assert_eq!(detection_details(&b.state, &b.id), [SPAWNED_DETAIL, "detection:unmeasured"], "B は測れない");
    assert_eq!(origin_measured(&b.state), format!("measured={}", a.sha), "起点は A のまま");
    write_landed_stub(&a.repo, 0);
    age_origin(&a.state);
    let c = daily_landed(&world, &DailyCar::crates("c", "s2-4cz", &["landed_c_"]));
    assert_eq!(last_argv(&c.repo), format!("--base {} --teeth landed_b_,landed_c_", a.sha), "C は A から B と C をまとめて測る");
    clean(&[&c.repo, &c.state]);
}

/// (9) 面の外の周は起点を進める: 差分が docs だけの B の子は stub を呼ばず、起点は `measured=<B>` になる。
#[test]
fn pipe_detection_daily_outside_scope_round_advances_the_origin() {
    let world = daily_world(&[DailyCar::crates("a", "s2-2e5", &["landed_a_"]), DailyCar::docs("b", "s2-3ax", &["landed_b_"])]);
    let a = daily_landed(&world, &DailyCar::crates("a", "s2-2e5", &["landed_a_"]));
    age_origin(&a.state);
    let b = daily_landed(&world, &DailyCar::docs("b", "s2-3ax", &["landed_b_"]));
    assert_eq!(detection_details(&b.state, &b.id), [SPAWNED_DETAIL, "detection:skipped"], "B は面の外");
    assert_eq!(landed_calls(&b.repo).len(), 1, "stub は A の 1 回だけ: {:?}", landed_calls(&b.repo));
    assert_eq!(origin_measured(&b.state), format!("measured={}", b.sha), "起点は B へ進む");
    clean(&[&b.repo, &b.state]);
}

/// (11)（対照）行を読めない周（`detection.daily_min_s` を `enabled = false` で足した `--rules`）は §44 のまま: A・B がどちらも
/// 口を起こし（spawned）、B の argv は `--base <B の親> --teeth landed_b_`・起点の file は無く、B の record に `since` の key が
/// 無く show の行に `since=` が無い。
#[test]
fn pipe_detection_daily_without_a_readable_row_keeps_the_landing_form() {
    let world = daily_world(&[DailyCar::crates("a", "s2-2e5", &["landed_a_"]), DailyCar::crates("b", "s2-3ax", &["landed_b_"])]);
    let rules = write_rules_full(&world.1, "rules-daily-off.toml", (1, 1_000_000), FOLLOW_RETRIES, default_slots());
    let text = fs::read_to_string(&rules).unwrap_or_default();
    let off = "[[rule]]\nid = \"detection.daily_min_s\"\nkind = \"DetectionDailyMinS\"\nvalue = 86400\nenabled = false\nruling = \"t\"\nruled_at = \"d\"\n";
    assert!(fs::write(&rules, format!("{text}\n{off}")).is_ok(), "行を足せる");
    let mut last = None;
    for car in [DailyCar::crates("a", "s2-2e5", &["landed_a_"]), DailyCar::crates("b", "s2-3ax", &["landed_b_"])] {
        let (run, out) = daily_land(&world, &car, Some(&rules));
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} の land: {} / {}", car.row, stdout_of(&out), stderr_of(&out));
        await_detection_child(&run.state, &run.id);
        assert_eq!(detection_details(&run.state, &run.id), [SPAWNED_DETAIL, "detection:measured"], "{} は起こして測る", car.row);
        last = Some(run);
    }
    let b = last.unwrap_or_else(|| panic!("便が 2 本"));
    let parent = git(&b.repo, &["rev-parse", &format!("{}^", b.sha)]);
    assert_eq!(last_argv(&b.repo), format!("--base {parent} --teeth landed_b_"), "B の base は親");
    assert!(!origin_file(&b.state).exists(), "起点の file は無い");
    let rows = landed_rows(&b);
    let row = rows.first().cloned().unwrap_or_default();
    assert!(row.iter().all(|(key, _)| key != "since" && key != "runs"), "record に since と runs の key が無い: {row:?}");
    assert!(shown_copies(&b).iter().all(|line| !line.contains("since=")), "show の行に since= が無い: {:?}", shown_copies(&b));
    clean(&[&b.repo, &b.state]);
}
