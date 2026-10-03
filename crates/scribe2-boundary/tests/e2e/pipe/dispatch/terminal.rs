// flip-check: moved s2-07l.686
//! 終端の周の族の歯（接頭辞 `pipe_terminal_`・設計 docs/design/carry-prep.md §10 行 n・親 `tests/e2e/pipe/dispatch.rs` の
//! helper を `use super::*` で使う）。起こす側の周の事前審査（接頭辞 `pipe_dispatch_precheck_`・dispatcher.md §27）と
//! 受付の断りの記帳（接頭辞 `pipe_dispatch_intake_refused_`・dispatcher.md §32）の歯も置く。

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

// ───── 事前審査（設計 docs/design/dispatcher.md §27・行 x・接頭辞 `pipe_dispatch_precheck_`） ─────
//
// 依存を待つ行に受付の判定を予想の base（未着地の祖先の宣言か Gated PASS の実物）で先に撃ち、結果を置き場の
// `pipe/precheck/<bead>` に残す。base には事前審査が無い＝結果の file も `[DISPATCH-PRECHECK]` の行も無い（RED）。

/// 事前審査の toy repo の宣言（`cargo` を許す＝行の verify に nextest の行を書ける）。
const PRECHECK_VESSEL: &str =
    "schema = 1\nallowed-commands = [\"git\", \"sh\", \"cargo\"]\ncommon-verify = [\"git rev-parse --verify {base}\"]\nrequirements = \"reqs.md\"\n";

/// 行 `id` の欄（write-set と verify だけ差し替える・`verify` が空なら既定の `sh verify-ok.sh`）。
fn precheck_row(id: &str, write_set: &str, verify: &str) -> Vec<String> {
    let mut add = vec![format!("write-set = {write_set}")];
    if !verify.is_empty() {
        add.push(format!("verify = [\"{verify}\"]"));
    }
    let borrowed: Vec<&str> = add.iter().map(String::as_str).collect();
    let drop: &[&str] = if verify.is_empty() { &["write-set"] } else { &["write-set", "verify"] };
    row_fields(id, drop, &borrowed)
}

/// 行と file を**そのまま** 1 回 commit した toy repo（行の素の項目を seed しない＝base に無い file を行が名指せる）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn precheck_repo(rows: &[Vec<String>], files: &[(&str, String)]) -> (std::path::PathBuf, std::path::PathBuf) {
    let (repo, state) = repo_with_state();
    write_design(&repo, &design_doc_rows(rows));
    let vessel = [(".vessel.toml", PRECHECK_VESSEL.to_owned())];
    for (path, body) in vessel.iter().chain(files) {
        let target = repo.join(path);
        fs::create_dir_all(target.parent().expect("親 dir が在る")).expect("dir を作れる");
        fs::write(&target, body).expect("file を書ける");
    }
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "precheck-rows"]);
    (repo, state)
}

/// 台帳の 1 件（行 `row` を指す open の bead・依存は (依存先, 種別) の列）。
fn waiting_on(id: &str, row: &str, deps: &[(&str, &str)]) -> String {
    listed(id, "open", 2, &format!("design = {DESIGN_FILE}#{row}"), deps)
}

/// 介入 `hold` を打つ（依存を持たない祖先を列が起こさないように・`hold` は 1 周を撃たない）。
fn hold(state: &Path, beads: &[&str]) {
    for &bead in beads {
        let out = run_pipe(&["dispatch", "hold", bead, "--state-dir", &state.display().to_string()]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "hold は rc 0（{}）", told(&out));
    }
}

/// 起こす側の手動の 1 周（道具つき・runner は偽の 1 行）。
fn precheck_turn(repo: &Path, state: &Path, bd: &str) -> Output {
    let out = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(state),
        "--bd", bd,
        "--lens", &review_lens_pass(state),
        "--runner", "true",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    out
}

/// 置き場の事前審査の結果の file。
fn precheck_file(state: &Path, bead: &str) -> std::path::PathBuf {
    state.join("pipe").join("precheck").join(bead)
}

/// 結果の file の本文（無ければ空）。
fn precheck_of(state: &Path, bead: &str) -> String {
    fs::read_to_string(precheck_file(state, bead)).unwrap_or_default()
}

/// 結果の file の `result=` の値（無ければ空）。
fn result_of(state: &Path, bead: &str) -> String {
    precheck_of(state, bead).lines().find_map(|line| line.strip_prefix("result=")).unwrap_or_default().to_owned()
}

/// (a) (e) 終端の 1 周: open な依存 A が `+` で宣言した file を素の path で持つ待ち行 B は予想の base で clean。`dispatch ls` は
/// 件数の行の前に `[DISPATCH-PRECHECK]` の 1 行を出し、`[DISPATCH]` の行の reason は依存のまま（予想は通行証でない）。
#[test]
fn pipe_dispatch_precheck_declared_new_file_makes_the_waiting_row_clean() {
    let rows = [
        precheck_row("a", r#"["+src/fresh.rs"]"#, ""),
        precheck_row("b", r#"["src/fresh.rs"]"#, ""),
        precheck_row("z", r#"["src/lib.rs"]"#, ""),
    ];
    let (repo, state) = precheck_repo(&rows, &[]);
    let bd = fake_bd(&state, &[waiting_on("s2-pre.1", "a", &[]), waiting_on("s2-pre.2", "b", &[("s2-pre.1", "blocks")])]);
    hold(&state, &["s2-pre.1"]);
    // 終端の 1 周（台帳に居ない便を止める）が事前審査を撃つ。
    let live = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#z"), "s2-pre.z");
    let stopped = run_pipe(&[
        "stop", "--run", &live,
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &bd,
        "--lens", &review_lens_pass(&state),
        "--runner", "true",
    ]);
    assert_eq!(stopped.status.code(), Some(i32::from(RC_OK)), "stop は rc 0（{}）", told(&stopped));
    assert_eq!(result_of(&state, "s2-pre.2"), "clean", "宣言の + を素で持つ行は clean: {}", precheck_of(&state, "s2-pre.2"));
    let listed = ls(&repo, &state, &bd);
    let out = stdout_of(&listed);
    let lines: Vec<&str> = out.lines().collect();
    let at = |prefix: &str| lines.iter().position(|line| line.starts_with(prefix));
    assert_eq!(
        lines.iter().filter(|line| line.starts_with("[DISPATCH-PRECHECK]")).copied().collect::<Vec<&str>>(),
        ["[DISPATCH-PRECHECK] bead=s2-pre.2 result=clean base=current"],
        "依存待ちの候補ごとに 1 行（母集団 候補 2・依存待ち 1）: {out}"
    );
    assert!(at("[DISPATCH-PRECHECK]") < at(COUNT) && at(COUNT).is_some(), "件数の行の前: {out}");
    assert_eq!(reason_of(&listed, "s2-pre.2"), "dependency:s2-pre.1", "reason は依存のまま: {out}");
    assert_eq!(count_of(&listed), format!("{COUNT} total=2 ready=0"), "件数の行の字は変わらない: {out}");
    clean(&[&repo, &state]);
}

/// (b) (c) base にも依存の宣言にも無い file を素で持つ行は firm:1（write-set-item-unresolved）・依存の write-set と交わる file の
/// 上限の余地が足りない行は provisional:1（cap-headroom・動く file と交わる file の列）。
#[test]
fn pipe_dispatch_precheck_splits_firm_and_provisional() {
    let rows = [
        precheck_row("a", r#"["+src/fresh.rs", "crates/toy/src/big.rs"]"#, ""),
        precheck_row("b", r#"["src/nowhere.rs"]"#, ""),
        precheck_row("c", r#"["crates/toy/src/big.rs"]"#, ""),
    ];
    let full = "x\n".repeat(usize::try_from(super::super::embedded_int("R-C4-2")).unwrap_or_default());
    let (repo, state) = precheck_repo(&rows, &[("crates/toy/src/big.rs", full)]);
    let blocks = [("s2-pre.1", "blocks")];
    let bd = fake_bd(
        &state,
        &[waiting_on("s2-pre.1", "a", &[]), waiting_on("s2-pre.2", "b", &blocks), waiting_on("s2-pre.3", "c", &blocks)],
    );
    hold(&state, &["s2-pre.1"]);
    precheck_turn(&repo, &state, &bd);
    let firm = precheck_of(&state, "s2-pre.2");
    assert_eq!(result_of(&state, "s2-pre.2"), "firm:1,provisional:0", "どこにも無い file は確定: {firm}");
    let line = firm.lines().find(|line| line.starts_with("finding=")).unwrap_or_default();
    assert!(line.starts_with("finding=firm name=write-set-item-unresolved at=files:src/nowhere.rs new=true"), "{firm}");
    let short = precheck_of(&state, "s2-pre.3");
    assert_eq!(result_of(&state, "s2-pre.3"), "firm:0,provisional:1", "依存と交わる file の余地不足は暫定: {short}");
    assert!(short.contains("finding=provisional name=cap-headroom at=files:crates/toy/src/big.rs new=false"), "{short}");
    clean(&[&repo, &state]);
}

/// (d) 同じ鍵の 2 周目は結果の file を書き直さず、依存の便を Gated PASS にした周は撃ち直され、依存の木で足した fn 名が待ち行の
/// filter 語に当たる teeth-outside-write-set が確定と new で出る（宣言だけの予想では見えない食い違い）。
#[test]
fn pipe_dispatch_precheck_same_key_is_kept_and_gated_pass_refires() {
    let verify = "cargo nextest run -p toy --no-tests=fail pre_b_";
    let rows = [
        precheck_row("a", r#"["+crates/toy/tests/a.rs"]"#, ""),
        precheck_row("b", r#"["crates/toy/tests/b.rs"]"#, verify),
    ];
    let (repo, state) = precheck_repo(&rows, &[("crates/toy/tests/b.rs", "#[test]\nfn pre_b_one() {}\n".to_owned())]);
    let bd = fake_bd(&state, &[waiting_on("s2-pre.1", "a", &[]), waiting_on("s2-pre.2", "b", &[("s2-pre.1", "blocks")])]);
    hold(&state, &["s2-pre.1"]);
    precheck_turn(&repo, &state, &bd);
    assert_eq!(result_of(&state, "s2-pre.2"), "clean", "宣言の予想は本文を持たない: {}", precheck_of(&state, "s2-pre.2"));
    // 同じ鍵の 2 周目は書き直さない（足した行が残る）。
    let probe = format!("{}probe\n", precheck_of(&state, "s2-pre.2"));
    fs::write(precheck_file(&state, "s2-pre.2"), &probe).expect("結果の file に印を足せる");
    precheck_turn(&repo, &state, &bd);
    assert_eq!(precheck_of(&state, "s2-pre.2"), probe, "同じ鍵の周は書き直さない");
    // 依存の便を Gated PASS にする（木で pre_b_ の歯を足す）。札は読めない形にして列に起こし直させない。
    let id = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), "s2-pre.1");
    let runner = "printf '#[test]\\nfn pre_b_two() {}\\n' > crates/toy/tests/a.rs && git add -A && git commit -q -m runner";
    let spawned = super::super::spawn_with(&repo, &state, &id, runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0（{}）", told(&spawned));
    let gated = gate_once(&repo, &state, &id, Some(&fake_lens(&state.join("pre-gate-lens"), &lens_verdict("PASS"))));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate は PASS（{}）", told(&gated));
    fs::write(state.join("pipe").join(&id).join("driver"), "not-a-pid\n").expect("札を書ける");
    precheck_turn(&repo, &state, &bd);
    let refired = precheck_of(&state, "s2-pre.2");
    assert!(!refired.contains("probe"), "鍵が動いた周は撃ち直す: {refired}");
    let keyed = refired.lines().any(|line| line.starts_with("key=head:") && line.contains(&format!("s2-pre.1=tree:{id}@")));
    assert!(keyed, "鍵の祖先の語は実物（tree:<便>@<sha>）: {refired}");
    assert_eq!(result_of(&state, "s2-pre.2"), "firm:1,provisional:0", "実物の歯は確定: {refired}");
    let found = refired.lines().find(|line| line.contains("teeth-outside-write-set")).unwrap_or_default();
    assert!(found.starts_with("finding=firm ") && found.contains(" new=true ") && found.contains("crates/toy/tests/a.rs"), "{refired}");
    clean(&[&repo, &state]);
}

/// (f) 予想で clean の行も、依存が `+` の file を作らずに閉じた周は実物の main の受付で断られて待つ（予想は通行証でない）。
/// 依存待ちに居なくなった bead の結果の file は同じ周に外れる。
#[test]
fn pipe_dispatch_precheck_is_not_a_pass_after_the_dependency_closes_without_the_file() {
    let rows = [precheck_row("a", r#"["+src/fresh.rs"]"#, ""), precheck_row("b", r#"["src/fresh.rs"]"#, "")];
    let (repo, state) = precheck_repo(&rows, &[]);
    let waiting = waiting_on("s2-pre.2", "b", &[("s2-pre.1", "blocks")]);
    let bd = fake_bd(&state, &[waiting_on("s2-pre.1", "a", &[]), waiting.clone()]);
    hold(&state, &["s2-pre.1"]);
    precheck_turn(&repo, &state, &bd);
    assert_eq!(result_of(&state, "s2-pre.2"), "clean", "予想では clean: {}", precheck_of(&state, "s2-pre.2"));
    let closed = fake_bd(&state, &[listed("s2-pre.1", "closed", 2, &format!("design = {DESIGN_FILE}#a"), &[]), waiting]);
    let listed_out = ls(&repo, &state, &closed);
    assert_eq!(reason_of(&listed_out, "s2-pre.2"), "admission:contract-table", "実物の受付で断られる（{}）", told(&listed_out));
    let after = precheck_turn(&repo, &state, &closed);
    assert!(stdout_of(&after).contains("started:0"), "起こさない（{}）", told(&after));
    assert!(!precheck_file(&state, "s2-pre.2").exists(), "依存待ちに居ない bead の file は外れる");
    clean(&[&repo, &state]);
}

/// (g) 推移の祖先: A → C → B の blocks で C が宣言する `+` の file を B が素で持つと clean。`parent-child` だけで繋がる bead の
/// 宣言と closed の bead の宣言は予想に入らず、その file を素で持つ行はそれぞれ firm:1。
#[test]
fn pipe_dispatch_precheck_follows_blocks_transitively_but_not_parents_or_closed() {
    let rows = [
        precheck_row("a", r#"["src/lib.rs"]"#, ""),
        precheck_row("c", r#"["+src/chain.rs"]"#, ""),
        precheck_row("b", r#"["src/chain.rs"]"#, ""),
        precheck_row("p", r#"["+src/parent.rs"]"#, ""),
        precheck_row("q", r#"["+src/closed.rs"]"#, ""),
        precheck_row("d", r#"["src/parent.rs"]"#, ""),
        precheck_row("e", r#"["src/closed.rs"]"#, ""),
    ];
    let (repo, state) = precheck_repo(&rows, &[]);
    let on_c = ("s2-pre.3", "blocks");
    let bd = fake_bd(
        &state,
        &[
            waiting_on("s2-pre.1", "a", &[]),
            waiting_on("s2-pre.3", "c", &[("s2-pre.1", "blocks")]),
            waiting_on("s2-pre.4", "b", &[on_c]),
            waiting_on("s2-pre.5", "p", &[]),
            listed("s2-pre.6", "closed", 2, &format!("design = {DESIGN_FILE}#q"), &[]),
            waiting_on("s2-pre.7", "d", &[on_c, ("s2-pre.5", "parent-child")]),
            waiting_on("s2-pre.8", "e", &[on_c, ("s2-pre.6", "blocks")]),
        ],
    );
    hold(&state, &["s2-pre.1", "s2-pre.5"]);
    precheck_turn(&repo, &state, &bd);
    assert_eq!(result_of(&state, "s2-pre.4"), "clean", "推移の祖先 C の宣言が入る: {}", precheck_of(&state, "s2-pre.4"));
    for (bead, file) in [("s2-pre.7", "src/parent.rs"), ("s2-pre.8", "src/closed.rs")] {
        let text = precheck_of(&state, bead);
        assert_eq!(result_of(&state, bead), "firm:1,provisional:0", "{bead} の祖先の外の宣言は入らない: {text}");
        assert!(text.contains(&format!("name=write-set-item-unresolved at=files:{file} ")), "{text}");
    }
    clean(&[&repo, &state]);
}

/// 行 `id` の欄に表の depends（同じ doc の行 id の列）を足す（write-set だけ差し替える）。
fn precheck_row_after(id: &str, write_set: &str, depends: &str) -> Vec<String> {
    let (set, after) = (format!("write-set = {write_set}"), format!("depends = {depends}"));
    row_fields(id, &["write-set"], &[set.as_str(), after.as_str()])
}

/// (h) 祖先が表の depends だけで繋がる（行 a1・設計 row-review.md §3 の口 (G)）: 台帳では hold した別の bead だけを blocks で待ち、表の
/// depends で `+` の file を宣言する open な行（hold）に繋がる行は、その file を素で持って clean。同じ台帳と同じ write-set で depends
/// を持たない対照の行は firm:1（write-set-item-unresolved）のまま。
#[test]
fn pipe_dispatch_precheck_table_depends_declares_the_new_file_without_a_ledger_block() {
    let rows = [
        precheck_row("a", r#"["+src/fresh.rs"]"#, ""),
        precheck_row("h", r#"["src/lib.rs"]"#, ""),
        precheck_row_after("b", r#"["src/fresh.rs"]"#, r#"["a"]"#),
        precheck_row("c", r#"["src/fresh.rs"]"#, ""),
    ];
    let (repo, state) = precheck_repo(&rows, &[]);
    let on_h = [("s2-pre.3", "blocks")];
    let bd = fake_bd(
        &state,
        &[
            waiting_on("s2-pre.1", "a", &[]),
            waiting_on("s2-pre.3", "h", &[]),
            waiting_on("s2-pre.2", "b", &on_h),
            waiting_on("s2-pre.4", "c", &on_h),
        ],
    );
    hold(&state, &["s2-pre.1", "s2-pre.3"]);
    precheck_turn(&repo, &state, &bd);
    let after = precheck_of(&state, "s2-pre.2");
    assert_eq!(result_of(&state, "s2-pre.2"), "clean", "表の depends の + を素で持つ行は clean: {after}");
    assert!(after.lines().any(|line| line.starts_with("key=head:") && line.contains(" ancestors:s2-pre.1=declared,")), "{after}");
    let control = precheck_of(&state, "s2-pre.4");
    assert_eq!(result_of(&state, "s2-pre.4"), "firm:1,provisional:0", "depends を持たない対照は確定のまま: {control}");
    assert!(control.contains("name=write-set-item-unresolved at=files:src/fresh.rs "), "{control}");
    clean(&[&repo, &state]);
}

// ───── 受付の断りの記帳（設計 docs/design/dispatcher.md §32・行 ag・接頭辞 `pipe_dispatch_intake_refused_`） ─────
//
// 起こす側の周が受付の断りを契約ごとに `IntakeRefused` 1 件に残す。base の周は 1 件も書かない（本数 0 で RED）。

/// 断られる行 r の bead（write-set が base に無い file を素で持つ）。
const REFUSED: &str = "s2-ref.1";

/// hold した行 h の bead（base に在る file の行・断られない）。
const HELD: &str = "s2-ref.2";

/// 行 r と hold した行 h の toy repo と台帳（`(repo, state, bd)`）。
fn refused_ledger() -> (std::path::PathBuf, std::path::PathBuf, String) {
    let rows = [precheck_row("r", r#"["src/absent.rs"]"#, ""), precheck_row("h", r#"["src/lib.rs"]"#, "")];
    let (repo, state) = precheck_repo(&rows, &[]);
    let bd = fake_bd(&state, &[waiting_on(REFUSED, "r", &[]), waiting_on(HELD, "h", &[])]);
    hold(&state, &[HELD]);
    (repo, state, bd)
}

/// event log の全行。
fn log_lines(state: &Path) -> Vec<String> {
    fs::read_to_string(state.join("fleet").join("events.jsonl")).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// event log の `IntakeRefused` の行（log の順）。
fn refusals(state: &Path) -> Vec<String> {
    log_lines(state).into_iter().filter(|line| line.contains("\"kind\":\"IntakeRefused\"")).collect()
}

/// (A) 周の前の `dispatch ls` は書かない。起こす側の 1 周は r の断りを 1 件だけ書き、2 周目と `dispatch ls` の後も 1 件のまま。
/// hold した h の行は書かない。
#[test]
fn pipe_dispatch_intake_refused_records_the_refusal_once_and_only_from_the_firing_round() {
    let (repo, state, bd) = refused_ledger();
    let before = ls(&repo, &state, &bd);
    assert!(reason_of(&before, REFUSED).starts_with("admission:"), "周の前にも r は受付で断られる（{}）", told(&before));
    assert_eq!(refusals(&state).len(), 0, "観測の口は書かない: {:?}", log_lines(&state));
    precheck_turn(&repo, &state, &bd);
    assert_eq!(refusals(&state).len(), 1, "1 周目は 1 件: {:?}", refusals(&state));
    let second = precheck_turn(&repo, &state, &bd);
    assert!(stdout_of(&second).contains("started:0"), "起こさない（{}）", told(&second));
    let after = ls(&repo, &state, &bd);
    let reason = reason_of(&after, REFUSED);
    let name = reason.strip_prefix("admission:").unwrap_or_default();
    assert!(!name.is_empty() && name != "mark" && name != "spawn", "受付の断りの名（{}）", told(&after));
    let found = refusals(&state);
    assert_eq!(found.len(), 1, "同じ断りが続く間は 1 件のまま: {found:?}");
    let line = found.first().map(String::as_str).unwrap_or_default();
    assert!(line.contains(&format!("\"bead\":\"{REFUSED}\"")), "bead は r: {line}");
    assert!(line.contains(&format!("\"refuse\":\"{name}\"")), "refuse は ls の理由の名 {name}: {line}");
    assert!(!found.iter().any(|line| line.contains(&format!("\"bead\":\"{HELD}\""))), "h は書かない: {found:?}");
    clean(&[&repo, &state]);
}

/// (B) 1 周の後に r へ `release` を打つと、次の周は同じ断りをもう 1 件書き、2 件目は release の行より後に在る。
#[test]
fn pipe_dispatch_intake_refused_records_again_after_a_release() {
    let (repo, state, bd) = refused_ledger();
    precheck_turn(&repo, &state, &bd);
    release(&state, REFUSED);
    precheck_turn(&repo, &state, &bd);
    let lines = log_lines(&state);
    let bead = format!("\"bead\":\"{REFUSED}\"");
    let at: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains("\"kind\":\"IntakeRefused\"") && line.contains(&bead))
        .map(|(at, _)| at)
        .collect();
    assert_eq!(at.len(), 2, "release の後の周はもう 1 件: {lines:?}");
    let released = lines.iter().position(|line| line.contains("\"mark\":\"release\"") && line.contains(&bead));
    assert!(released.is_some_and(|found| at.first() < Some(&found) && at.get(1) > Some(&found)), "release の行を挟む: {lines:?}");
    clean(&[&repo, &state]);
}

// ---- 上限の許可の見え方（接頭辞 `pipe_show_permit_word_` / `pipe_dispatch_ls_permit_`・limit-permit.md §21 行 e）----

/// 許可の対象の rules 行（許可の読み手を持つ行）。
const PERMIT_ROW: &str = "gate.token_cap";

/// 許可の期限（未来・UTC の秒の形・時計は差し替えない）。
const FUTURE: &str = "2099-01-01T00:00:00Z";

/// 許可の期限（過去）。
const PAST: &str = "2020-01-01T00:00:00Z";

/// event log に 1 行（行 b の on-disk の形）を足す。`keys` は `"kind":…` から始まる key の列。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_event(state: &Path, keys: &str) {
    let dir = state.join("fleet");
    fs::create_dir_all(&dir).expect("fleet dir を作れる");
    let line = format!("{{\"schema\":1,\"ts\":\"2026-10-03T00:00:00Z\",{keys},\"host\":\"h\",\"actor\":\"machine\"}}\n");
    let mut file = fs::OpenOptions::new().create(true).append(true).open(dir.join("events.jsonl")).expect("event log を開ける");
    std::io::Write::write_all(&mut file, line.as_bytes()).expect("行を書ける");
}

/// 便の `RunCreated`（段 Intake）。
fn put_run(state: &Path, run: &str, bead: &str) {
    put_event(state, &format!("\"kind\":\"RunCreated\",\"run\":\"{run}\",\"bead\":\"{bead}\",\"stage\":\"Intake\""));
}

/// 便の着地（段 Landed の `RunStage`）。
fn put_landed(state: &Path, run: &str, bead: &str) {
    put_event(state, &format!("\"kind\":\"RunStage\",\"run\":\"{run}\",\"bead\":\"{bead}\",\"stage\":\"Landed\""));
}

/// 許可の記帳の `detail` を字で足す（行 id が manifest に無い形など）。
fn put_detail(state: &Path, bead: &str, detail: &str) {
    put_event(state, &format!("\"kind\":\"LimitPermitted\",\"bead\":\"{bead}\",\"detail\":\"{detail}\""));
}

/// 許可の記帳（行は [`PERMIT_ROW`]・値・期限・裁定 id）。
fn put_permit(state: &Path, bead: &str, value: u64, until: &str, ruling: &str) {
    put_detail(state, bead, &format!("rule={PERMIT_ROW} value={value} until={until} ruling={ruling}"));
}

/// 取り消しの記帳。
fn put_revoke(state: &Path, bead: &str, rule: &str) {
    put_event(state, &format!("\"kind\":\"LimitPermitted\",\"bead\":\"{bead}\",\"detail\":\"rule={rule} revoked\""));
}

/// 埋め込みの manifest の `gate.token_cap`（`pipe show` の manifest の値 D）。
fn declared_d() -> u64 {
    super::super::embedded_int(PERMIT_ROW)
}

/// `pipe show --run` の stdout の行（`rules` は `--rules` の写し）。
fn show_of(repo: &Path, state: &Path, run: &str, rules: Option<&str>) -> Vec<String> {
    let mut args = vec!["show", "--run", run];
    let (repo, state) = (repo.display().to_string(), state.display().to_string());
    args.extend(["--repo", &repo, "--state-dir", &state]);
    if let Some(path) = rules {
        args.extend(["--rules", path]);
    }
    stdout_of(&run_pipe(&args)).lines().map(str::to_owned).collect()
}

/// `permit:` で始まる行だけ。
fn permit_rows(lines: &[String]) -> Vec<String> {
    lines.iter().filter(|line| line.starts_with("permit:")).cloned().collect()
}

/// `permit:` の 1 行の完全な字（`declared` は manifest の値）。
fn permit_row(value: u64, declared: u64, until: &str, ruling: &str, word: &str) -> String {
    format!("permit: rule={PERMIT_ROW} value={value} declared={declared} until={until} ruling={ruling} state={word}")
}

/// 便 1 本の `pipe show` の `permit:` の行（既定の manifest）。
fn permits_of(repo: &Path, state: &Path, run: &str) -> Vec<String> {
    permit_rows(&show_of(repo, state, run, None))
}

/// (a) active — 値 D+1・期限は未来の許可が 1 行で最後の行に出て、1 行目は今の形のまま。許可を持たない bead の便には行が無く、ほかの bead の許可も出ない。
#[test]
fn pipe_show_permit_word_active_is_one_last_line_and_other_beads_show_none() {
    let (repo, state) = repo_with_state();
    let d = declared_d();
    put_run(&state, "r-a1", "s2-pm.a1");
    put_run(&state, "r-a2", "s2-pm.a2");
    put_permit(&state, "s2-pm.a1", d + 1, FUTURE, "q-1");
    let shown = show_of(&repo, &state, "r-a1", None);
    let first = shown.first().map(String::as_str).unwrap_or_default();
    assert!(first.starts_with("run=r-a1 bead=s2-pm.a1 stage=Intake approved="), "1 行目は今の形: {shown:?}");
    assert_eq!(shown.last(), Some(&permit_row(d + 1, d, FUTURE, "q-1", "active")), "最後の行: {shown:?}");
    assert_eq!(permit_rows(&shown).len(), 1, "許可の行は 1 行: {shown:?}");
    let other = show_of(&repo, &state, "r-a2", None);
    assert!(permit_rows(&other).is_empty(), "許可を持たない bead の便に行は無い: {other:?}");
    assert_eq!(other.len(), 1, "出力は 1 行目だけ: {other:?}");
    clean(&[&repo, &state]);
}

/// (b) landed — 同じ許可が bead の便の着地の前は active・後は landed。
#[test]
fn pipe_show_permit_word_landed_follows_the_landing() {
    let (repo, state) = repo_with_state();
    let d = declared_d();
    put_run(&state, "r-b", "s2-pm.b");
    put_permit(&state, "s2-pm.b", d + 1, FUTURE, "q-1");
    assert_eq!(permits_of(&repo, &state, "r-b"), [permit_row(d + 1, d, FUTURE, "q-1", "active")], "着地の前");
    put_landed(&state, "r-b", "s2-pm.b");
    assert_eq!(permits_of(&repo, &state, "r-b"), [permit_row(d + 1, d, FUTURE, "q-1", "landed")], "着地の後");
    clean(&[&repo, &state]);
}

/// (c) revoked — 取り消しの前は active・後は revoked で、`permit:` の行はどちらもちょうど 1 行（取り消しを単独の行にしない）。
#[test]
fn pipe_show_permit_word_revoked_follows_the_revocation_in_one_line() {
    let (repo, state) = repo_with_state();
    let d = declared_d();
    put_run(&state, "r-c", "s2-pm.c");
    put_permit(&state, "s2-pm.c", d + 1, FUTURE, "q-1");
    assert_eq!(permits_of(&repo, &state, "r-c"), [permit_row(d + 1, d, FUTURE, "q-1", "active")], "取り消しの前");
    put_revoke(&state, "s2-pm.c", PERMIT_ROW);
    assert_eq!(permits_of(&repo, &state, "r-c"), [permit_row(d + 1, d, FUTURE, "q-1", "revoked")], "取り消しの後");
    clean(&[&repo, &state]);
}

/// (d) superseded — 2 つ目の許可の前は 1 つ目が active・後は 1 つ目が superseded で 2 つ目が active（記帳の順に 2 行）。
#[test]
fn pipe_show_permit_word_superseded_follows_the_newer_permit() {
    let (repo, state) = repo_with_state();
    let d = declared_d();
    put_run(&state, "r-d", "s2-pm.d");
    put_permit(&state, "s2-pm.d", d + 1, FUTURE, "q-1");
    assert_eq!(permits_of(&repo, &state, "r-d"), [permit_row(d + 1, d, FUTURE, "q-1", "active")], "2 つ目の前");
    put_permit(&state, "s2-pm.d", d + 2, FUTURE, "q-2");
    let both = [permit_row(d + 1, d, FUTURE, "q-1", "superseded"), permit_row(d + 2, d, FUTURE, "q-2", "active")];
    assert_eq!(permits_of(&repo, &state, "r-d"), both, "2 つ目の後");
    clean(&[&repo, &state]);
}

/// (e) expired — 期限が過去の許可は expired・同じ値で期限が未来の許可（別の bead）は active。
#[test]
fn pipe_show_permit_word_expired_follows_the_deadline() {
    let (repo, state) = repo_with_state();
    let d = declared_d();
    put_run(&state, "r-e1", "s2-pm.e1");
    put_run(&state, "r-e2", "s2-pm.e2");
    put_permit(&state, "s2-pm.e1", d + 1, PAST, "q-1");
    put_permit(&state, "s2-pm.e2", d + 1, FUTURE, "q-2");
    assert_eq!(permits_of(&repo, &state, "r-e1"), [permit_row(d + 1, d, PAST, "q-1", "expired")], "過去");
    assert_eq!(permits_of(&repo, &state, "r-e2"), [permit_row(d + 1, d, FUTURE, "q-2", "active")], "未来");
    clean(&[&repo, &state]);
}

/// (f) overtaken — 値 D は overtaken・D+1 は active、同じ歯で `--rules` の写し（値 D+10）を渡すと値 D+1 の許可も overtaken。
#[test]
fn pipe_show_permit_word_overtaken_follows_the_manifest_value() {
    let (repo, state) = repo_with_state();
    let d = declared_d();
    put_run(&state, "r-f1", "s2-pm.f1");
    put_run(&state, "r-f2", "s2-pm.f2");
    put_permit(&state, "s2-pm.f1", d, FUTURE, "q-1");
    put_permit(&state, "s2-pm.f2", d + 1, FUTURE, "q-2");
    assert_eq!(permits_of(&repo, &state, "r-f1"), [permit_row(d, d, FUTURE, "q-1", "overtaken")], "値 D");
    assert_eq!(permits_of(&repo, &state, "r-f2"), [permit_row(d + 1, d, FUTURE, "q-2", "active")], "値 D+1");
    let raised = super::super::write_rules(&state, "rules-raised.toml", 1, d + 10).display().to_string();
    let shown = permit_rows(&show_of(&repo, &state, "r-f2", Some(&raised)));
    assert_eq!(shown, [permit_row(d + 1, d + 10, FUTURE, "q-2", "overtaken")], "写しで D+10 に上げた周");
    clean(&[&repo, &state]);
}

/// (g) landed と revoked — 取り消しの後は revoked、さらに着地の後は landed。
#[test]
fn pipe_show_permit_word_landed_wins_over_revoked() {
    let (repo, state) = repo_with_state();
    let d = declared_d();
    put_run(&state, "r-g", "s2-pm.g");
    put_permit(&state, "s2-pm.g", d + 1, FUTURE, "q-1");
    put_revoke(&state, "s2-pm.g", PERMIT_ROW);
    assert_eq!(permits_of(&repo, &state, "r-g"), [permit_row(d + 1, d, FUTURE, "q-1", "revoked")], "取り消しの後");
    put_landed(&state, "r-g", "s2-pm.g");
    assert_eq!(permits_of(&repo, &state, "r-g"), [permit_row(d + 1, d, FUTURE, "q-1", "landed")], "着地の後");
    clean(&[&repo, &state]);
}

/// (h) landed と superseded — 新しい許可の後は 1 つ目が superseded、着地の後は 2 行とも landed。
#[test]
fn pipe_show_permit_word_landed_wins_over_superseded() {
    let (repo, state) = repo_with_state();
    let d = declared_d();
    put_run(&state, "r-h", "s2-pm.h");
    put_permit(&state, "s2-pm.h", d + 1, FUTURE, "q-1");
    put_permit(&state, "s2-pm.h", d + 2, FUTURE, "q-2");
    let before = [permit_row(d + 1, d, FUTURE, "q-1", "superseded"), permit_row(d + 2, d, FUTURE, "q-2", "active")];
    assert_eq!(permits_of(&repo, &state, "r-h"), before, "新しい許可の後");
    put_landed(&state, "r-h", "s2-pm.h");
    let after = [permit_row(d + 1, d, FUTURE, "q-1", "landed"), permit_row(d + 2, d, FUTURE, "q-2", "landed")];
    assert_eq!(permits_of(&repo, &state, "r-h"), after, "着地の後");
    clean(&[&repo, &state]);
}

/// (i) revoked と expired — 期限が過去の許可は expired、取り消しの後は revoked。
#[test]
fn pipe_show_permit_word_revoked_wins_over_expired() {
    let (repo, state) = repo_with_state();
    let d = declared_d();
    put_run(&state, "r-i", "s2-pm.i");
    put_permit(&state, "s2-pm.i", d + 1, PAST, "q-1");
    assert_eq!(permits_of(&repo, &state, "r-i"), [permit_row(d + 1, d, PAST, "q-1", "expired")], "取り消しの前");
    put_revoke(&state, "s2-pm.i", PERMIT_ROW);
    assert_eq!(permits_of(&repo, &state, "r-i"), [permit_row(d + 1, d, PAST, "q-1", "revoked")], "取り消しの後");
    clean(&[&repo, &state]);
}

/// (j) superseded と expired — 期限が過去の 1 つ目は expired、2 つ目の許可の後は superseded。
#[test]
fn pipe_show_permit_word_superseded_wins_over_expired() {
    let (repo, state) = repo_with_state();
    let d = declared_d();
    put_run(&state, "r-j", "s2-pm.j");
    put_permit(&state, "s2-pm.j", d + 1, PAST, "q-1");
    assert_eq!(permits_of(&repo, &state, "r-j"), [permit_row(d + 1, d, PAST, "q-1", "expired")], "2 つ目の前");
    put_permit(&state, "s2-pm.j", d + 1, FUTURE, "q-2");
    let both = [permit_row(d + 1, d, PAST, "q-1", "superseded"), permit_row(d + 1, d, FUTURE, "q-2", "active")];
    assert_eq!(permits_of(&repo, &state, "r-j"), both, "2 つ目の後");
    clean(&[&repo, &state]);
}

/// (k) expired と overtaken — 値 D・期限が未来は overtaken、値 D・期限が過去（別の bead）は expired。
#[test]
fn pipe_show_permit_word_expired_wins_over_overtaken() {
    let (repo, state) = repo_with_state();
    let d = declared_d();
    put_run(&state, "r-k1", "s2-pm.k1");
    put_run(&state, "r-k2", "s2-pm.k2");
    put_permit(&state, "s2-pm.k1", d, FUTURE, "q-1");
    put_permit(&state, "s2-pm.k2", d, PAST, "q-2");
    assert_eq!(permits_of(&repo, &state, "r-k1"), [permit_row(d, d, FUTURE, "q-1", "overtaken")], "未来");
    assert_eq!(permits_of(&repo, &state, "r-k2"), [permit_row(d, d, PAST, "q-2", "expired")], "過去");
    clean(&[&repo, &state]);
}

/// (l) manifest に無い行 id の許可は `permit: unmeasured rule=<行 id> records=1` で `state=` を含む行が無く、同じ bead の `gate.token_cap` の許可は語の行を持つ。
#[test]
fn pipe_show_permit_word_unreadable_rule_is_unmeasured_without_a_word() {
    let (repo, state) = repo_with_state();
    let d = declared_d();
    put_run(&state, "r-l", "s2-pm.l");
    put_detail(&state, "s2-pm.l", &format!("rule=gate.absent_row value=5 until={FUTURE} ruling=q-1"));
    put_permit(&state, "s2-pm.l", d + 1, FUTURE, "q-2");
    let shown = permits_of(&repo, &state, "r-l");
    let want = ["permit: unmeasured rule=gate.absent_row records=1".to_owned(), permit_row(d + 1, d, FUTURE, "q-2", "active")];
    assert_eq!(shown, want, "行 id の字の順");
    assert!(!shown.iter().any(|line| line.contains("rule=gate.absent_row") && line.contains("state=")), "語を出さない: {shown:?}");
    clean(&[&repo, &state]);
}

/// 列の写し（`dispatch_rules`）の `gate.token_cap` の値（`pipe dispatch ls` の manifest の値）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn ls_declared(state: &Path) -> u64 {
    let rules = dispatch_rules(state);
    let manifest = vessel::rules::manifest::Manifest::load(Path::new(&rules)).expect("列の写しを読める");
    vessel::rules::int_row(&manifest, PERMIT_ROW).expect("写しに gate.token_cap が在る")
}

/// `dispatch ls` の stdout の行。
fn ls_lines(repo: &Path, state: &Path, bd: &str) -> Vec<String> {
    stdout_of(&ls(repo, state, bd)).lines().map(str::to_owned).collect()
}

/// 行の `<name>` で始まる語の値。
fn word_of(line: &str, name: &str) -> String {
    line.split(' ').find_map(|word| word.strip_prefix(name)).unwrap_or_default().to_owned()
}

/// (m) 効いている 2 つ・取り消し・期限切れ・着地・manifest の値以下・新しい許可に替わった 1 つの bead で、`[DISPATCH-PERMIT]` の行は
/// 効いている許可だけが bead の字の順に出て 1 行目は `[DISPATCH-NONE]` のまま。同じ写しの各便の `pipe show` の `state=active` の行の集合と等しい。
#[test]
fn pipe_dispatch_ls_permit_lists_only_effective_permits_in_bead_order() {
    let (repo, state) = repo_with_state();
    let bd = fake_bd(&state, &[]);
    let d = ls_declared(&state);
    let rules = dispatch_rules(&state);
    let beads = ["s2-p.9", "s2-p.10", "s2-p.11", "s2-p.12", "s2-p.13", "s2-p.14", "s2-p.15"];
    for (at, bead) in beads.iter().enumerate() {
        put_run(&state, &format!("r-m{at}"), bead);
    }
    put_permit(&state, "s2-p.9", d + 1, FUTURE, "q-9");
    put_permit(&state, "s2-p.10", d + 2, FUTURE, "q-10");
    put_permit(&state, "s2-p.11", d + 1, FUTURE, "q-11");
    put_revoke(&state, "s2-p.11", PERMIT_ROW);
    put_permit(&state, "s2-p.12", d + 1, PAST, "q-12");
    put_permit(&state, "s2-p.13", d + 1, FUTURE, "q-13");
    put_landed(&state, "r-m4", "s2-p.13");
    put_permit(&state, "s2-p.14", d, FUTURE, "q-14");
    put_permit(&state, "s2-p.15", d + 1, FUTURE, "q-15a");
    put_permit(&state, "s2-p.15", d + 3, FUTURE, "q-15b");
    let lines = ls_lines(&repo, &state, &bd);
    assert_eq!(lines.first().map(String::as_str), Some("[DISPATCH-NONE]"), "1 行目は今のまま: {lines:?}");
    let listed: Vec<String> = lines.iter().filter(|line| line.starts_with("[DISPATCH-PERMIT]")).cloned().collect();
    let want = [
        format!("[DISPATCH-PERMIT] bead=s2-p.10 rule={PERMIT_ROW} value={} declared={d} until={FUTURE} ruling=q-10", d + 2),
        format!("[DISPATCH-PERMIT] bead=s2-p.15 rule={PERMIT_ROW} value={} declared={d} until={FUTURE} ruling=q-15b", d + 3),
        format!("[DISPATCH-PERMIT] bead=s2-p.9 rule={PERMIT_ROW} value={} declared={d} until={FUTURE} ruling=q-9", d + 1),
    ];
    assert_eq!(listed, want, "効いている許可だけが bead の字の順: {lines:?}");
    let mut words = std::collections::BTreeSet::new();
    let mut active = std::collections::BTreeSet::new();
    for (at, bead) in beads.iter().enumerate() {
        for row in permit_rows(&show_of(&repo, &state, &format!("r-m{at}"), Some(&rules))) {
            words.insert(word_of(&row, "state="));
            if word_of(&row, "state=") == "active" {
                active.insert(((*bead).to_owned(), word_of(&row, "value="), word_of(&row, "ruling=")));
            }
        }
    }
    let all: Vec<&str> = words.iter().map(String::as_str).collect();
    assert_eq!(all, ["active", "expired", "landed", "overtaken", "revoked", "superseded"], "6 語が全部出る");
    let listed_set: std::collections::BTreeSet<(String, String, String)> =
        listed.iter().map(|line| (word_of(line, "bead="), word_of(line, "value="), word_of(line, "ruling="))).collect();
    assert_eq!(active, listed_set, "state=active の集合と [DISPATCH-PERMIT] の集合が等しい");
    clean(&[&repo, &state]);
}

/// (n) event log の位置に dir を置いた周は `[DISPATCH-PERMIT-UNMEASURED reason=events]` が最後の行で、同じ歯の読める event log（許可の記帳 0 件）の周は
/// `[DISPATCH-PERMIT` で始まる行が無く、同じ歯の偽の bd が rc 1 の周は許可の記帳が在っても `[DISPATCH-UNMEASURED reason=…]` の 1 行だけ。
#[test]
fn pipe_dispatch_ls_permit_names_an_unreadable_event_log_and_stays_silent_otherwise() {
    let (repo, state) = repo_with_state();
    let d = ls_declared(&state);
    put_run(&state, "r-n", "s2-pn.1");
    put_permit(&state, "s2-pn.1", d + 1, FUTURE, "q-1");
    let broken = script(&state.join("bd-broken"), "exit 1\n");
    let measured = ls_lines(&repo, &state, &broken);
    assert_eq!(measured, ["[DISPATCH-UNMEASURED reason=ledger]"], "台帳を読めない周は 1 行だけ");
    let bd = fake_bd(&state, &[]);
    let readable = ls_lines(&repo, &state, &bd);
    assert!(readable.iter().any(|line| line.starts_with("[DISPATCH-PERMIT] bead=s2-pn.1 ")), "読める周は許可を出す: {readable:?}");
    let log = state.join("fleet").join("events.jsonl");
    fs::remove_file(&log).expect("log を消せる");
    fs::create_dir(&log).expect("log の位置に dir を置ける");
    let lines = ls_lines(&repo, &state, &bd);
    assert_eq!(lines.last().map(String::as_str), Some("[DISPATCH-PERMIT-UNMEASURED reason=events]"), "最後の行: {lines:?}");
    let (repo_b, state_b) = repo_with_state();
    let bd_b = fake_bd(&state_b, &[]);
    put_run(&state_b, "r-n2", "s2-pn.2");
    let quiet = ls_lines(&repo_b, &state_b, &bd_b);
    assert!(!quiet.iter().any(|line| line.starts_with("[DISPATCH-PERMIT")), "許可の記帳 0 件の周は行を足さない: {quiet:?}");
    clean(&[&repo, &state, &repo_b, &state_b]);
}

/// (o) 写しに無い行 id の許可は `[DISPATCH-PERMIT-UNMEASURED reason=rules rule=<行 id>]` を出し、同じ置き場の `gate.token_cap` の効いている許可は
/// `[DISPATCH-PERMIT]` の行で出る。
#[test]
fn pipe_dispatch_ls_permit_names_a_rule_the_copy_cannot_read() {
    let (repo, state) = repo_with_state();
    let bd = fake_bd(&state, &[]);
    let d = ls_declared(&state);
    put_detail(&state, "s2-po.1", &format!("rule=gate.absent_row value=5 until={FUTURE} ruling=q-1"));
    put_permit(&state, "s2-po.2", d + 1, FUTURE, "q-2");
    let lines = ls_lines(&repo, &state, &bd);
    let unread = "[DISPATCH-PERMIT-UNMEASURED reason=rules rule=gate.absent_row]".to_owned();
    let shown = format!("[DISPATCH-PERMIT] bead=s2-po.2 rule={PERMIT_ROW} value={} declared={d} until={FUTURE} ruling=q-2", d + 1);
    assert!(lines.contains(&unread), "読めない行 id を名乗る: {lines:?}");
    assert!(lines.contains(&shown), "効いている許可は出る: {lines:?}");
    clean(&[&repo, &state]);
}
