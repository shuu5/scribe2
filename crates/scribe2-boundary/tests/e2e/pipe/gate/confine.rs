// flip-check: moved s2-07l.685
//! 封じ込めと受付札の族の歯（接頭辞 `pipe_confine_` / `pipe_slots_`・設計 docs/design/carry-prep.md §10 行 m）。
//!
//! 共有の helper と const と外形 snapshot の歯は親 module（`tests/e2e/pipe/gate.rs`）に在り、`use super::*` で使う。
//! 歯の本文は親から**挙動不変で移した**もの（`s2-07l.685`）。

use super::*;

/// **`{jobs}` を持つ行は実効値へ置換され、job の箱で撃たれる**（設計 §3.3 / §4.2）。
///
/// base の gate はこの宣言を intake で断る（`{jobs}` は置けない穴）ので、この歯は base で
/// 落ちる＝flip の RED である。
#[test]
fn pipe_confine_fills_the_jobs_hole_and_uses_the_job_box() {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-jobs.sh {jobs}"]"#);
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));

    // **撃たれた側**が受け取った値（record の cmd だけでは置換したことを測れない）。
    // 値は受付の実測で決まる（host 依存）ので、record の `jobs=` と突き合わせる。
    let rows = verify_rows(&state, &id);
    let jobs = row_value(&rows, 2, "jobs");
    let count: u64 = jobs.parse().unwrap_or(0);
    assert!(
        (1..=embedded_int("gate.mutants_jobs")).contains(&count),
        "実効 jobs は 1 以上・上限以下: {jobs}"
    );
    let git_dir = git(&worktree_of(&repo, &id), &["rev-parse", "--absolute-git-dir"]);
    assert_eq!(
        fs::read_to_string(Path::new(&git_dir).join("jobs-seen")).unwrap_or_default(),
        jobs,
        "撃たれた側は record と同じ実効 jobs を受け取る"
    );
    assert_eq!(row_value(&rows, 2, "cmd"), format!("sh verify-jobs.sh {jobs}"), "record の cmd も置換後である");
    assert_eq!(row_value(&rows, 2, "confined"), "true", "包めている");

    // 箱は `実効 jobs × gate.job_memory_mb`・重みは rules 行そのもの（値は manifest が持つ・C1）。
    let record = scope_record(&state, "-common-2-");
    assert_eq!(
        scope_prop(&record, "MemoryMax"),
        format!("{}M", count * embedded_int("gate.job_memory_mb")),
        "job の箱: {record}"
    );
    assert_eq!(
        scope_prop(&record, "CPUWeight"),
        embedded_int("gate.cpu_weight").to_string(),
        "CPU の重み: {record}"
    );
    assert!(!record.contains("MemoryHigh"), "MemoryHigh は付けない（設計 §4.2）: {record}");
    clean(&[&repo, &state]);
}

/// **箱は 2 種で、同じ gate の中で互いに違う値になる**（設計 §4.2）。
///
/// 1 本ずつ別の unit の記録を読む——`{jobs}` 行と非 `{jobs}` 行の記録を混ぜると、
/// `limit_of` を片方へ潰した実装でも両方の assert が通る（fixture 衝突）。
#[test]
fn pipe_confine_uses_two_distinct_boxes_in_one_gate() {
    let (repo, state) = repo_with_state();
    commit_vessel(
        &repo,
        VESSEL_ALLOWED,
        r#"["sh verify-jobs.sh {jobs}", "sh verify-ok.sh"]"#,
    );
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));

    let jobs: u64 = row_value(&verify_rows(&state, &id), 2, "jobs").parse().unwrap_or(0);
    let job_box = scope_prop(&scope_record(&state, "-common-2-"), "MemoryMax");
    let host_box = scope_prop(&scope_record(&state, "-common-3-"), "MemoryMax");
    assert_eq!(
        job_box,
        format!("{}M", jobs * embedded_int("gate.job_memory_mb")),
        "{{jobs}} を持つ行は job の箱（実効 jobs {jobs}）"
    );
    let want_host = vessel::pipe::confine::mem_total_mb(&fs::read_to_string("/proc/meminfo").unwrap_or_default())
        .and_then(|total| total.checked_sub(embedded_int("host.reserve_memory_mb")))
        .filter(|mb| *mb > 0);
    assert_eq!(
        Some(host_box.clone()),
        want_host.map(|mb| format!("{mb}M")),
        "{{jobs}} を持たない行は host の箱（MemTotal − reserve）"
    );
    assert_ne!(job_box, host_box, "2 つの箱は互いに違う値である");
    for unit in ["-common-2-", "-common-3-"] {
        let record = scope_record(&state, unit);
        assert!(
            record.lines().any(|line| line == "OOMPolicy=continue"),
            "{unit} の包みを systemd の OOM 停止から外す: {record}"
        );
    }
    clean(&[&repo, &state]);
}

/// **包めない host では素の `sh -c` で撃ち、record に理由を残す**（止めない・設計 §4.2）。
#[test]
fn pipe_confine_falls_back_to_the_plain_shell_without_the_tool() {
    let (repo, state) = repo_with_state();
    let path = lean_path(&state);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(
        gated.status.code(),
        Some(i32::from(RC_OK)),
        "systemd-run の無い host でも便は流れる: {}",
        stderr_of(&gated)
    );
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "confined"), "false", "包めていない");
    assert_eq!(row_value(&rows, 3, "reason"), "no-systemd-run", "理由は閉じた enum の名");
    assert_eq!(row_value(&rows, 3, "peak_mb"), "-", "測れない peak は 0 と書かない");
    assert_eq!(row_value(&rows, 3, "rc"), "0", "行そのものは撃たれている");
    clean(&[&repo, &state]);
}

/// **peak は包みの終端行から読む**（設計 §4.3）。終端行の無い行は `-` である。
#[test]
fn pipe_confine_reads_the_peak_from_the_trailing_line() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let design = write_contract(
        &repo,
        &["verify"],
        &[r#"verify = ["sh verify-peak.sh", "sh verify-ok.sh"]"#],
    );
    let id = intake(&repo, &state, &design);
    let spawned = run_pipe_with_path(
        &path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT],
    );
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));
    let marker = state.join("lens-ran");
    let gated = run_pipe_with_path(
        &path,
        &["gate", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(),
          "--lens", &fake_lens(&marker, &lens_verdict("PASS"))],
    );
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "peak_mb"), "3", "3145728 byte は 3 MiB");
    assert_eq!(row_value(&rows, 4, "peak_mb"), "-", "終端行の無い行は不明（0 ではない）");
    clean(&[&repo, &state]);
}

/// **runner と lens の起動も同じ包みを通る**（設計 §4.1 の 2 つ目と 3 つ目）。
#[test]
fn pipe_confine_wraps_the_runner_and_the_lens() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let (_id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    assert!(marker.exists(), "lens は実際に撃たれている（包みは行を殺さない）");
    for stage in ["-runner-1-", "-lens-1-"] {
        let record = scope_record(&state, stage);
        assert!(record.lines().any(|line| line == "--scope"), "{stage} は scope である: {record}");
        assert!(
            record.lines().any(|line| line == "OOMPolicy=continue"),
            "{stage} も OOM 停止から外す: {record}"
        );
    }
    clean(&[&repo, &state]);
}

/// **gate の lens の箱は 1 × `gate.job_memory_mb`・同じ gate の `{jobs}` を持たない verify 行は host の箱のまま**
/// （設計 §12・行 c・裁定 id user 2026-09-15T18:2xZ）。両方向を 1 本で撃つ——lens だけを見る歯は全部を
/// job の箱へ潰す実装でも生き残り、verify 行だけを見る歯は lens を host の箱に残す実装で生き残る。
#[test]
fn pipe_confine_lens_box_is_one_job_and_the_plain_line_keeps_the_host_box() {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-jobs.sh {jobs}", "sh verify-ok.sh"]"#);
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let (_id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    assert!(marker.exists(), "lens は実際に撃たれている");

    let one_job = format!("{}M", embedded_int("gate.job_memory_mb"));
    let lens = scope_record(&state, "-lens-1-");
    assert_eq!(scope_prop(&lens, "MemoryMax"), one_job, "lens の箱は 1 × gate.job_memory_mb: {lens}");

    let want_host = vessel::pipe::confine::mem_total_mb(&fs::read_to_string("/proc/meminfo").unwrap_or_default())
        .and_then(|total| total.checked_sub(embedded_int("host.reserve_memory_mb")))
        .filter(|mb| *mb > 0)
        .map(|mb| format!("{mb}M"));
    let plain = scope_record(&state, "-common-3-");
    assert_eq!(
        Some(scope_prop(&plain, "MemoryMax")),
        want_host,
        "{{jobs}} を持たない verify 行は host の箱（MemTotal − reserve）のまま: {plain}"
    );
    assert_ne!(Some(one_job), want_host, "前提: 2 つの箱は互いに違う値である");
    clean(&[&repo, &state]);
}

/// **箱の中で殺された verify 行は赤ではなく「測れなかった」**（設計 §4.2）。
///
/// rc は 0 のままの fixture で撃つ＝根拠が rc ではなく終端行の `oom_kill` であることを測る。
#[test]
fn pipe_confine_oom_verify_line_is_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let design = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-oom.sh"]"#]);
    let id = intake(&repo, &state, &design);
    let spawned = run_pipe_with_path(
        &path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT],
    );
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));
    let marker = state.join("lens-ran");
    let gated = run_pipe_with_path(
        &path,
        &["gate", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(),
          "--lens", &fake_lens(&marker, &lens_verdict("PASS"))],
    );
    assert_eq!(gated.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stdout_of(&gated));
    assert!(stdout_of(&gated).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&gated));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "rc"), "0", "rc は 0 のまま（rc では見ていない）");
    assert_eq!(row_value(&rows, 3, "reason"), "oom-kill", "外からの kill と弁別する");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "0", "赤には数えない");
    assert!(!marker.exists(), "測れなかった周は lens を起動しない");
    clean(&[&repo, &state]);
}

/// **runner の箱が溢れた便は `Failed detail=oom-kill` で終端する**（設計 §4.2・理由 1 つ）。
#[test]
fn pipe_confine_oom_runner_fails_the_run() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let design = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &design);
    let runner = format!("{TOY_COMMIT}\nprintf 'confine-usage peak_bytes=9437184 oom_kill=1\\n'");
    let out = run_pipe_with_path(
        &path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", &runner],
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "記帳は通る: {}", stderr_of(&out));
    let seen = trail(&state, &id);
    assert!(
        seen.contains(&(EventKind::RunStage, Some(Stage::Failed), Some("oom-kill".to_owned()))),
        "閉じた理由 1 つで終端する: {seen:?}"
    );
    clean(&[&repo, &state]);
}

/// **lens の箱が溢れた周は INCONCLUSIVE**（FR9 の既存極性のまま・便は終端しない・設計 §4.2）。
#[test]
fn pipe_confine_oom_lens_is_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let body = lens_verdict("PASS");
    let lens = format!(
        "{}; printf 'confine-usage peak_bytes=9437184 oom_kill=1\\n'",
        fake_lens(&marker, &body)
    );
    let (id, gated) = confined_run(&repo, &state, &path, &lens);
    assert_eq!(gated.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stdout_of(&gated));
    assert!(marker.exists(), "lens は起動されている（判定だけが届かない）");
    assert!(
        value_of(&verdict_pairs(&state, &id), "evidence").contains("oom-kill"),
        "理由が verdict に残る: {:?}",
        verdict_pairs(&state, &id)
    );
    assert!(
        show_line(&repo, &state, &id).contains("stage=Gated"),
        "便は終端しない（測り直せる）: {}",
        show_line(&repo, &state, &id)
    );
    clean(&[&repo, &state]);
}

/// (a) **verify 行の終端で `kill --signal=SIGKILL <unit>.scope` が 1 回撃たれ**、unit は
/// `systemd-run` が受けた名と一致する。base では systemctl が撃たれず落ちる（機能不在）。
#[test]
fn pipe_confine_release_kills_the_verify_scope_once_by_its_unit() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    systemctl_stub(&state, SYSTEMCTL_KILLED);
    let marker = state.join("lens-ran");
    let (_id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let unit = scope_unit(&state, "-contract-3-");
    assert_eq!(
        release_calls(&state, &unit),
        release_sequence(&unit),
        "終端で 1 回だけ、包んだ名の scope を SIGKILL で片付ける（後に reset-failed が 1 回）"
    );
    clean(&[&repo, &state]);
}

/// `s2-07l.421`: 包んだ scope の引数に `--collect` が**ちょうど 1 回**在り、終端の片付けは `kill` の**後に**
/// `reset-failed` を 1 回撃つ（同じ unit 名・順序つき）。行の record は `scope=killed` のまま（Released の語彙は不変）。
/// base は `--collect` も `reset-failed` も撃たないので RED。
#[test]
fn pipe_confine_collect_release_resets_failed_after_kill() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    systemctl_stub(&state, SYSTEMCTL_KILLED);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let unit = scope_unit(&state, "-contract-3-");
    let record = scope_record(&state, "-contract-3-");
    assert_eq!(
        record.lines().filter(|line| *line == "--collect").count(),
        1,
        "--collect は 1 回: {record}"
    );
    let calls = release_calls(&state, &unit);
    assert_eq!(calls, release_sequence(&unit), "kill の後に reset-failed が 1 回");
    assert_eq!(
        calls.iter().filter(|call| call.contains(" reset-failed ")).count(),
        1,
        "reset-failed は 1 回（母集団 {} 本）",
        calls.len()
    );
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "scope"), "killed", "record の語彙は不変");
    assert_eq!(row_value(&rows, 3, "rc"), "0", "行の rc は不変");
    clean(&[&repo, &state]);
}

/// `s2-07l.421`: unit が無い周（`kill` も `reset-failed` も「not loaded」で断る）も `Gone` のまま＝record に
/// `scope=` を書かず、行の rc と判定は変わらない（reset の rc を record に写さない）。
#[test]
fn pipe_confine_collect_gone_unit_keeps_the_record() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    systemctl_stub(&state, SYSTEMCTL_GONE);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let unit = scope_unit(&state, "-contract-3-");
    assert_eq!(release_calls(&state, &unit), release_sequence(&unit), "無い unit にも kill → reset-failed");
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "confined"), "true", "包めている");
    assert!(!row_has(&rows, 3, "scope"), "Gone は書かない: {:?}", rows.get(2));
    assert_eq!(row_value(&rows, 3, "rc"), "0", "行の rc は不変");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS", "判定は不変");
    clean(&[&repo, &state]);
}

/// (b) 偽 systemctl が rc 0 → record に `scope=killed`（判定は変えない）。
#[test]
fn pipe_confine_release_killed_is_recorded_on_the_row() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    systemctl_stub(&state, SYSTEMCTL_KILLED);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "判定は変えない: {}", stderr_of(&gated));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "confined"), "true", "包めている");
    assert_eq!(row_value(&rows, 3, "scope"), "killed", "残りを殺した周は record に残す");
    assert!(!row_has(&rows, 1, "scope"), "撃つ process を持たない段①は片付けない");
    clean(&[&repo, &state]);
}

/// (c) unit が既に無い（not loaded）→ record に `scope=` が無い（正常は書かない）。
#[test]
fn pipe_confine_release_gone_leaves_no_scope_field() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    systemctl_stub(&state, SYSTEMCTL_GONE);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let unit = scope_unit(&state, "-contract-3-");
    assert_eq!(release_calls(&state, &unit), release_sequence(&unit), "片付けは撃たれている（不在は撃たなかったせいではない）");
    let rows = verify_rows(&state, &id);
    assert!(!row_has(&rows, 3, "scope"), "Gone は書かない: {:?}", rows.get(2));
    assert!(!row_has(&verify_rows(&state, &id), 2, "scope"), "Gone は書かない（共通 verify の行）");
    clean(&[&repo, &state]);
}

/// (d) `systemctl` の無い host → `scope=no-tool`・行の verdict は不変（縮退・C11.2）。
#[test]
fn pipe_confine_release_without_systemctl_is_no_tool_and_keeps_the_verdict() {
    let (repo, state) = repo_with_state();
    systemd_stub(&state);
    let path = format!("{}:{}", state.join(SYSTEMD_BIN).display(), lean_path(&state));
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "片付けの失敗で赤にしない: {}", stderr_of(&gated));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "confined"), "true", "systemd-run は在る＝包めている");
    assert_eq!(row_value(&rows, 3, "scope"), "no-tool", "道具が無い周の名");
    assert_eq!(row_value(&rows, 3, "rc"), "0", "行の rc は不変");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS", "判定は不変");
    clean(&[&repo, &state]);
}

/// (e) land の追随の再 gate は 1 周目の gate と**別 process** で撃たれ、偽 `systemd-run` が同名の 2 本目を断る
/// 形でも起動できる（unit 名は場所 + 段 + pid + 通し番号）。再 gate が判定した木をそのまま land するので
/// **主実測は撃たない**（設計 gate-cost.md §27・`s2-07l.464`＝同じ process が 2 周撃つ経路はここで消えた。
/// 通し番号で別名になる性質は in-file の `confine_unit_name_differs_for_the_same_arguments` /
/// `confine_seq_increases_monotonically` が持つ）。
///
/// 別便が動かす面は検出線の面の内（`crates/other.txt`）＝追随が従来どおり再 gate を撃つ形（設計 §33）。
// flip-check: retroactive s2-07l.416
#[test]
fn pipe_confine_release_regate_in_one_process_uses_distinct_unit_names() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    systemctl_stub(&state, SYSTEMCTL_GONE);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "1 周目の gate: {}", stderr_of(&gated));
    // 別便は **面の内**（`crates/` 配下）を動かす——面の外だけが動いた周の追随は再 gate を撃たずに
    // 前周の判定を引き継ぐ（設計 §33）ので、2 周が別名で撃たれたことを測れない。
    fs::create_dir_all(repo.join("crates")).expect("面の内の dir を作れる");
    fs::write(repo.join("crates").join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    // land の process が撃つ名だけを数える（1 周目の gate は別 process の記録）。
    let records = state.join(SCOPE_RECORDS);
    fs::remove_dir_all(&records).expect("記録を空にできる");
    fs::create_dir_all(&records).expect("記録の dir を作り直せる");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let landed = run_pipe_with_path(
        &path,
        &["land", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--lens", &lens],
    );
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "追随して載る: {}", stderr_of(&landed));
    assert!(stdout_of(&landed).contains("rebase="), "追随の再 gate を通った: {}", stdout_of(&landed));
    let names = dir_names(&records);
    let contract: Vec<&String> = names.iter().filter(|name| name.contains("-contract-3-")).collect();
    assert_eq!(contract.len(), 1, "land の process が撃つ契約 verify は再 gate の 1 周だけ（主実測は撃たない）: {names:?}");
    assert_eq!(kinds(&main_rows(&state, &id)), ["main"], "主実測は再 gate と同じ木＝skip record 1 本");
    clean(&[&repo, &state]);
}

/// (f) **runner と lens の scope も終端で片付ける**（1 起動に 1 回・lens の結果は verdict に残る）。
#[test]
fn pipe_confine_release_runner_and_lens_scopes_are_released() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    systemctl_stub(&state, SYSTEMCTL_KILLED);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    for stage in ["-runner-1-", "-lens-1-"] {
        let unit = scope_unit(&state, stage);
        assert_eq!(
            release_calls(&state, &unit),
            release_sequence(&unit),
            "{stage} の scope も終端で 1 回片付ける"
        );
    }
    assert_eq!(value_of(&verdict_pairs(&state, &id), "scope"), "killed", "lens の片付けは verdict に残る");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS", "判定は不変");
    clean(&[&repo, &state]);
}

/// **project 2 つの gate が同じ host の slot dir を見て、死んだ札を回収する**（設計 §3.2・歯 (1) (3)）。
///
/// 札は置き場ごとではなく `<state_dir の親>` から導く＝同じ tmp root の 2 つの置き場で、片方の
/// gate が回収した後にもう片方の gate が**同じ dir に置き直した**札を回収する。
#[test]
fn pipe_slots_two_projects_share_one_dir_and_reclaim_dead_tickets() {
    let root = tmp();
    let (repo_a, state_a) = repo_with_state_in(&root.join("state-a"));
    let (repo_b, state_b) = repo_with_state_in(&root.join("state-b"));
    assert_eq!(host_slots(&state_a), host_slots(&state_b), "2 つの置き場の親は同じ");

    let dead = plant_ticket(&state_a, DEAD_PID, 1);
    let (id_a, gated_a) = slot_gate(&repo_a, &state_a);
    assert_eq!(gated_a.status.code(), Some(i32::from(RC_OK)), "gate a: {}", stderr_of(&gated_a));
    assert_reclaimed_one(&value_of(&slot_row(&verify_rows(&state_a, &id_a)), "slot"), "a が回収した");
    assert!(!dead.exists(), "死んだ札は削除された");

    // **b の置き場からは札を置かない**（a の親に置いた札を b の gate が拾う＝親が一致する）。
    let again = plant_ticket(&state_a, DEAD_PID, 1);
    let (id_b, gated_b) = slot_gate(&repo_b, &state_b);
    assert_eq!(gated_b.status.code(), Some(i32::from(RC_OK)), "gate b: {}", stderr_of(&gated_b));
    assert_reclaimed_one(&value_of(&slot_row(&verify_rows(&state_b, &id_b)), "slot"), "b も同じ dir を回収した");
    assert!(!again.exists(), "置き直した札も削除された");
    clean(&[&repo_a, &repo_b, &root]);
}

/// **生きている札が枠を食い尽くすと待ち、上限を超えたら並列度 1 で進む**（歯 (2)）。
///
/// 札の pid はこの歯の process（gate の間ずっと生きている）で、jobs を host の総量より大きく
/// 置いて `by_token` を 0 にする。待ちの上限は rules fixture の `slot_wait_s = 1`。
#[test]
fn pipe_slots_live_ticket_waits_then_degrades_to_one_job() {
    let (repo, state) = repo_with_state();
    let live = plant_ticket(&state, u64::from(std::process::id()), 1_000_000);
    let started = std::time::Instant::now();
    let (id, gated) = slot_gate(&repo, &state);
    let took = started.elapsed();
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "縮退しても便は流れる: {}", stderr_of(&gated));
    let row = slot_row(&verify_rows(&state, &id));
    assert_eq!(value_of(&row, "slot"), "degraded", "上限を超えた: {row:?}");
    assert_eq!(value_of(&row, "jobs"), "1", "並列度 1 で進む（0 で走らせない）");
    assert_eq!(value_of(&row, "cmd"), "sh verify-slot.sh 1", "置換後の cmd に 1 が載る");
    assert!(took >= std::time::Duration::from_secs(SLOT_WAIT_S), "待った: {took:?}");
    assert!(live.exists(), "生きている札は回収しない");
    clean(&[&repo, &state]);
}

/// **待ちの途中で塞いでいた札が消えると、上限を待たずに枠を配って進む**（歯 (9)）。
///
/// - 容量の 2 線を fixture で最小（job 1 MiB・reserve 0）にし、枠を配れるかを host の空き memory に
///   依らせない（設計 §7）。塞ぐのは自 pid の札（jobs を host の総量より大きく置く）。
/// - 待ちの始まりは**先に置いた死んだ札が 1 周目の受付で回収される**ことで知る（壁時計に頼らない）。
/// - 待ちの間に死んだ札をもう 1 枚置き、**回収されずに残る**ことを測る（待ちの観測は lock も回収も
///   持たない＝観測を常に「空いた」と読む実装は受付を回し続けて札を回収する）。
/// - 観測を常に「空かない」と読む実装は上限まで待って `degraded` になる。
#[test]
fn pipe_slots_wait_ends_early_when_the_blocking_ticket_goes() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let (repo, state) = repo_with_state();
    commit_slot_vessel(&repo, &state);
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let slots = SlotFixture { job_mb: 1, reserve_mb: 0, wait_s: SLOT_WAIT_LONG_S, ..default_slots() };
    let rules = write_rules_full(&state, "rules-slot-long.toml", (1, 1_000_000), FOLLOW_RETRIES, slots);
    let design = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &design);
    let spawned = run_pipe_with_path(
        &path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT],
    );
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));

    let first_dead = plant_ticket(&state, DEAD_PID, 1);
    let live = plant_ticket(&state, u64::from(std::process::id()), 1_000_000_000_000);
    let mut child = bin_cmd()
        .arg("pipe")
        .args(["gate", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--lens", &lens,
               "--rules", &rules.display().to_string()])
        .env("PATH", &path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary を起動できる");

    // 1 周目の受付が死んだ札を回収した＝枠が 0 で待ちに入った（塞ぐ札は生きている）。
    let begun = Instant::now();
    while first_dead.exists() {
        assert!(child.try_wait().ok().flatten().is_none(), "gate が受付の前に終わった");
        assert!(begun.elapsed() < Duration::from_secs(120), "1 周目の受付が来ない");
        std::thread::sleep(Duration::from_millis(10));
    }
    let second_dead = plant_ticket(&state, DEAD_PID, 1);
    std::thread::sleep(Duration::from_millis(500));
    assert!(second_dead.exists(), "待ちの間は札を回収しない（観測は受付を回さない）");
    fs::remove_file(&second_dead).expect("札を消せる");

    let freed = Instant::now();
    fs::remove_file(&live).expect("塞いでいた札を消せる");
    let out = child.wait_with_output().expect("gate を待てる");
    let took = freed.elapsed();
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&out));
    let row = slot_row(&verify_rows(&state, &id));
    // Granted の字面（回収 1 枚＝1 周目の死んだ札・`slot_detail` の合成）。
    assert_eq!(value_of(&row, "slot"), "reclaimed:1", "上限を待たずに枠を配った: {row:?}");
    assert!(took < Duration::from_secs(SLOT_WAIT_LONG_S / 2), "上限を待っていない: {took:?}");
    assert!(slot_names(&state).is_empty(), "終了で札が消える: {:?}", slot_names(&state));
    clean(&[&repo, &state]);
}

/// **全部の行が札を置き（`{jobs}` の無い行は jobs 1）、終了で消し、置換後の cmd に実効 jobs が載る**（歯 (4) (5) (6)）。
#[test]
fn pipe_slots_ticket_lives_only_during_the_jobs_line() {
    let (repo, state) = repo_with_state();
    let (id, gated) = slot_gate(&repo, &state);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let rows = verify_rows(&state, &id);
    let row = slot_row(&rows);
    assert_ne!(value_of(&row, "slot"), "unmeasured", "meminfo の在る host では測れる: {row:?}");
    let jobs = value_of(&row, "jobs");
    let count: u64 = jobs.parse().unwrap_or(0);
    assert!((1..=embedded_int("gate.mutants_jobs")).contains(&count), "実効 jobs は 1..=上限: {jobs}");
    assert_eq!(value_of(&row, "cmd"), format!("sh verify-slot.sh {jobs}"), "(6) record の cmd は置換後");

    let git_dir = PathBuf::from(git(&worktree_of(&repo, &id), &["rev-parse", "--absolute-git-dir"]));
    let read = |name: &str| fs::read_to_string(git_dir.join(name)).unwrap_or_default();
    assert_eq!(read("jobs-seen"), jobs, "(6) 撃たれた側も同じ実効 jobs");
    // (4) `{jobs}` の無い行の間も自便の札がちょうど 1 枚在り、本文の jobs は 1（全部の行が受付を通る・設計 gate-cost.md §45）。
    let plain: Vec<String> =
        read("slots-plain").lines().filter(|name| name.ends_with(".slot")).map(str::to_owned).collect();
    assert_eq!(plain.len(), 1, "(4) {{jobs}} の無い行の間も札 1 枚: {plain:?}");
    assert!(plain.iter().all(|name| name.ends_with(&format!("-{id}.slot"))), "(4) 札の名は <pid>-<run>.slot: {plain:?}");
    let plain_body = vessel::fleet::json_lite::parse_object(read("slots-plain-body").trim()).unwrap_or_default();
    assert_eq!(value_of(&plain_body, "jobs"), "1", "(4) {{jobs}} の無い行の札の jobs は 1: {}", read("slots-plain-body"));
    assert_eq!(value_of(&plain_body, "run"), id, "(4) 札の run は便 id");
    // (5) `{jobs}` の行の間は自便の札がちょうど 1 枚在り、本文の jobs が実効 jobs と一致する。
    let during: Vec<String> = read("slots-during")
        .lines()
        .filter(|name| name.ends_with(".slot"))
        .map(str::to_owned)
        .collect();
    assert_eq!(during.len(), 1, "(5) 行の間は札 1 枚: {during:?}");
    assert!(during.iter().all(|name| name.ends_with(&format!("-{id}.slot"))), "札の名は <pid>-<run>.slot: {during:?}");
    let body = vessel::fleet::json_lite::parse_object(read("slots-body").trim()).unwrap_or_default();
    assert_eq!(value_of(&body, "jobs"), jobs, "札の jobs は実効 jobs: {}", read("slots-body"));
    assert_eq!(value_of(&body, "run"), id, "札の run は便 id");
    // (5) 終了で札は消える。
    assert!(slot_names(&state).is_empty(), "(5) 終了で札が消える: {:?}", slot_names(&state));
    clean(&[&repo, &state]);
}

/// **3 つの穴を持つ行の置換後の `cmd` に実効 jobs と実効 thread が両方載る**（設計 gate-cost.md §31 約束 4 / 6）。
///
/// thread の値は受付が決める: 枠を配れた周は `max(1, floor(cores / gate.mutants_jobs))`（cores はこの歯が同じ
/// host で測る）、縮退の周は 1（枠の可否は host の memory に依るので、record の `slot=` で読み分ける）。
/// 撃たれた側（`$2`）も同じ値を受け取る＝record の字面だけの置換ではない。
#[test]
fn pipe_slots_threads_cmd_carries_effective_jobs_and_threads() {
    let (repo, state) = repo_with_state();
    let (id, gated) = slot_gate_line(&repo, &state, SLOT_THREADS_LINE);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let row = slot_row(&verify_rows(&state, &id));
    let slot = value_of(&row, "slot");
    assert_ne!(slot, "unmeasured", "meminfo と cores の在る host では測れる: {row:?}");
    let jobs = value_of(&row, "jobs");
    let cmd = value_of(&row, "cmd");
    let (head, threads) = cmd.rsplit_once(' ').unwrap_or_default();
    assert_eq!(head, format!("sh verify-slot.sh {jobs}"), "置換後の cmd は jobs の後ろに thread を持つ: {cmd}");
    let count: u64 = threads.parse().unwrap_or(0);
    let cap = embedded_int("gate.mutants_jobs");
    let price = host_cores().map_or(1, |cores| (cores / cap.max(1)).max(1));
    if slot.starts_with("degraded") {
        assert_eq!(count, 1, "縮退の周は thread も 1: {cmd}");
        assert_eq!(jobs, "1", "縮退の周は jobs 1: {row:?}");
    } else {
        assert_eq!(count, price, "枠を配れた周の thread は値段 max(1, cores / cap): {cmd}");
    }
    assert!(count >= 1, "thread は 1 以上（0 と書かない）: {cmd}");
    let git_dir = PathBuf::from(git(&worktree_of(&repo, &id), &["rev-parse", "--absolute-git-dir"]));
    let read = |name: &str| fs::read_to_string(git_dir.join(name)).unwrap_or_default();
    assert_eq!(read("jobs-seen"), jobs, "撃たれた側も同じ実効 jobs");
    assert_eq!(read("threads-seen"), threads, "撃たれた側も同じ実効 thread");
    clean(&[&repo, &state]);
}

/// **待ちの上限を超えた周の `cmd` は jobs も thread も 1**（設計 gate-cost.md §31 約束 4・`slot=degraded` と対）。
///
/// 塞ぐ札は自 pid（gate の間ずっと生きている）で jobs を host の総量より大きく置く＝memory の `by_token` も
/// CPU の `by_cpu` も 0 になる。待ちの上限は rules fixture の `slot_wait_s = 1`。thread を `cores / 1` で導く
/// 実装は縮退の周に core 数ぶんの thread を許す（2026-09-20 の事故の出所）ので、ここで 1 を pin する。
#[test]
fn pipe_slots_threads_degraded_run_gets_one_job_and_one_thread() {
    let (repo, state) = repo_with_state();
    let live = plant_ticket(&state, u64::from(std::process::id()), 1_000_000_000_000);
    let (id, gated) = slot_gate_line(&repo, &state, SLOT_THREADS_LINE);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "縮退しても便は流れる: {}", stderr_of(&gated));
    let row = slot_row(&verify_rows(&state, &id));
    assert_eq!(value_of(&row, "slot"), "degraded", "上限を超えた: {row:?}");
    assert_eq!(value_of(&row, "jobs"), "1", "並列度 1 で進む");
    assert_eq!(value_of(&row, "cmd"), "sh verify-slot.sh 1 1", "置換後の cmd は jobs 1・thread 1");
    let git_dir = PathBuf::from(git(&worktree_of(&repo, &id), &["rev-parse", "--absolute-git-dir"]));
    let read = |name: &str| fs::read_to_string(git_dir.join(name)).unwrap_or_default();
    assert_eq!(read("jobs-seen"), "1", "撃たれた側の jobs も 1");
    assert_eq!(read("threads-seen"), "1", "撃たれた側の thread も 1（core 数ぶんではない）");
    assert!(live.exists(), "生きている札は回収しない");
    clean(&[&repo, &state]);
}

// ---- 箱の CPU の幅（設計 docs/design/gate-cost.md §45・ADR-0095・接頭辞 `cpu_width_`）------------------------
//
// 偽 systemd-run の記録（`scope_record` / `scope_prop`）の `CPUQuota` を読む。受付が配った行の箱は `jobs × threads × 100%`
// （縮退と測れない周は 100%）、配らない箱は 1 job の値段 × 100%（core 数は歯が `Cpus_allowed_list` から数え、`gate.mutants_jobs`
// は埋め込みの値）。枠を配れたかは host の空き memory に依る（設計 §7）ので、行の record の `slot=` で読み分ける。

/// 1 job の値段 × 100（%・core 数を読めない周は空＝`CPUQuota` の語が無い）。
fn one_job_quota() -> String {
    let cap = embedded_int("gate.mutants_jobs").max(1);
    host_cores().map_or_else(String::new, |cores| format!("{}%", (cores / cap).max(1) * 100))
}

/// 受付が配った周の便の箱の上限（`slot` が縮退か測れない周は 100%・配れた周は 1 job の値段）。
fn granted_quota(slot: &str) -> String {
    if slot.starts_with("degraded") || slot.starts_with("unmeasured") {
        "100%".to_owned()
    } else {
        one_job_quota()
    }
}

/// `needle` を名に含む scope の `CPUQuota`（記録がちょうど 1 件・語が無ければ空）。
fn quota_of(state: &Path, needle: &str) -> String {
    scope_prop(&scope_record(state, needle), "CPUQuota")
}

/// (d) `{jobs}` を持たない共通 verify と契約の verify の行は受付を通り（record が `slot=` を持ち・jobs は 1）、箱の `CPUQuota` は
/// 1 job の値段 × 100%（縮退した周は 100%）。base は受付を通らず（`slot=` が無い）箱に `CPUQuota` が無い。
#[test]
fn cpu_width_lines_without_jobs_pass_admission_and_get_the_one_job_price() {
    let (repo, state) = repo_with_state();
    let (id, gated) = slot_gate(&repo, &state);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "common", "contract"], "母集団 4 record: {rows:?}");
    for (n, needle) in [(3, "-common-3-"), (4, "-contract-4-")] {
        assert!(row_has(&rows, n, "slot"), "{n} 番目（{{jobs}} を持たない行）も受付を通る: {rows:?}");
        assert_eq!(row_value(&rows, n, "jobs"), "1", "{{jobs}} を持たない行は 1 job（{n} 番目）");
        let slot = row_value(&rows, n, "slot");
        assert_ne!(slot, "unmeasured", "meminfo と cores の在る host では測れる: {rows:?}");
        assert_eq!(quota_of(&state, needle), granted_quota(&slot), "{needle} の箱は 1 job の値段（slot={slot}）");
    }
    assert!(!row_has(&rows, 1, "slot"), "撃つ process を持たない段①は受付を通らない");
    clean(&[&repo, &state]);
}

/// (e) `{jobs}` を持つ共通 verify の行の箱の `CPUQuota` は置換された jobs × threads × 100%（record の cmd の実値から組む）。
#[test]
fn cpu_width_jobs_line_box_is_the_substituted_jobs_times_threads() {
    let (repo, state) = repo_with_state();
    let (id, gated) = slot_gate_line(&repo, &state, SLOT_THREADS_LINE);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let row = slot_row(&verify_rows(&state, &id));
    let jobs: u64 = value_of(&row, "jobs").parse().unwrap_or(0);
    let cmd = value_of(&row, "cmd");
    let threads: u64 = cmd.rsplit_once(' ').and_then(|(_, tail)| tail.parse().ok()).unwrap_or(0);
    assert!(jobs >= 1 && threads >= 1, "置換された実値は 1 以上: {cmd}");
    assert_eq!(quota_of(&state, "-common-2-"), format!("{}%", jobs * threads * 100), "jobs × threads × 100%: {cmd}");
    clean(&[&repo, &state]);
}

/// (f) land の主実測の行は札を取らず（record に `slot=` が無い）、箱は 1 job の値段 × 100%（受付が配った幅を持たない箱）。
/// 主実測を撃つ周にするため verdict の木を差し替える（同じ木の周は主実測ごと省く・設計 §27）。
#[test]
fn cpu_width_land_main_run_takes_no_ticket_and_gets_the_one_job_price() {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-ok.sh"]"#);
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    super::super::land::make_tree_differ(&repo, &state, &id, "refs/heads/main");
    // land の process が撃つ箱だけを読む（gate の記録と同じ段の名が並ぶ）。
    let records = state.join(SCOPE_RECORDS);
    fs::remove_dir_all(&records).expect("記録を空にできる");
    fs::create_dir_all(&records).expect("記録の dir を作り直せる");
    let landed = run_pipe_with_path(
        &path,
        &["land", "--run", &id, "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string()],
    );
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    await_detection_child(&state, &id);
    let (main, _) = split_landed(main_rows(&state, &id));
    assert_eq!(kinds(&main), ["write-set", "common", "contract"], "主実測の母集団（①②④）: {main:?}");
    assert!(main.iter().all(|row| !row.iter().any(|(key, _)| key == "slot")), "主実測は札を取らない: {main:?}");
    for needle in ["-common-2-", "-contract-3-"] {
        assert_eq!(quota_of(&state, needle), one_job_quota(), "{needle} の箱は 1 job の値段");
    }
    clean(&[&repo, &state]);
}

/// (g) 受付が縮退した周（生きている札で枠を埋め・待ちの上限は fixture の 1 秒）の行の箱は `CPUQuota=100%`——`{jobs}` を持つ行も
/// `{jobs}` を持たない行も契約の行も同じ（縮退の Grant は jobs 1 × thread 1）。
#[test]
fn cpu_width_degraded_run_boxes_are_one_thread() {
    let (repo, state) = repo_with_state();
    let live = plant_ticket(&state, u64::from(std::process::id()), 1_000_000_000_000);
    let (id, gated) = slot_gate_line(&repo, &state, SLOT_THREADS_LINE);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "縮退しても便は流れる: {}", stderr_of(&gated));
    let rows = verify_rows(&state, &id);
    for n in [2, 3, 4] {
        assert_eq!(row_value(&rows, n, "slot"), "degraded", "{n} 番目も縮退した: {rows:?}");
    }
    for needle in ["-common-2-", "-common-3-", "-contract-4-"] {
        assert_eq!(quota_of(&state, needle), "100%", "{needle} の箱は縮退の 100%");
    }
    assert!(live.exists(), "生きている札は回収しない");
    clean(&[&repo, &state]);
}

/// (h) runner と lens の包みの箱は 1 job の値段 × 100% の `CPUQuota` と `CPUWeight`（受付を通らない箱）。
#[test]
fn cpu_width_runner_and_lens_boxes_get_the_one_job_price() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let (_id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    for needle in ["-runner-1-", "-lens-1-"] {
        let record = scope_record(&state, needle);
        assert_eq!(scope_prop(&record, "CPUQuota"), one_job_quota(), "{needle} の箱は 1 job の値段: {record}");
        assert_eq!(scope_prop(&record, "CPUWeight"), embedded_int("gate.cpu_weight").to_string(), "重みは付けたまま: {record}");
    }
    clean(&[&repo, &state]);
}
