// flip-check: moved s2-07l.264
//! gate の歯: `pipe_gate_` / `pipe_detection_`（検出線）/ `pipe_confine_`（封じ込め）/ `pipe_slots_`（受付札）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く（歯の本文は移しただけ・`s2-07l.264`）。

use super::*;

#[test]
fn pipe_gate_refuses_dirty_worktree() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 実装後に worktree を汚す。gate は前提を満たさない。
    fs::write(worktree_of(&repo, &id).join("dirty.txt"), "x\n").expect("汚せる");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "前提違反は rc 1");
    assert!(stderr_of(&out).contains("clean でない"), "理由: {}", stderr_of(&out));
    assert!(!marker.exists(), "**lens を起動しない**（前提違反の周）");
    assert!(
        show_line(&repo, &state, &id).contains("stage=Failed"),
        "precheck 違反は Failed で残る: {}",
        show_line(&repo, &state, &id)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_fails_on_red_verify_line() {
    let (repo, state) = repo_with_state();
    // 2 行目が rc≠0。**逐条**で残るので 2 行とも verify.jsonl に出る。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-ok.sh", "sh verify-red.sh"]"#]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1");
    assert!(stdout_of(&out).contains("verdict=FAIL"), "{}", stdout_of(&out));
    // **lens は呼ばない**: verify が赤い周は lens の verdict に上書きされない。
    assert!(!marker.exists(), "verify RED の周は lens を起動しない");
    let log = fs::read_to_string(state.join("pipe").join(&id).join("verify.jsonl"))
        .expect("verify.jsonl を読める");
    // 母集団 = 4 record（write-set 照合 1 + 写しの共通 verify 1 + 契約 2 本）。
    assert_eq!(log.lines().count(), 4, "verify は逐条で残る: {log}");
    assert!(log.contains("\"n\":3,\"rc\":0,\"cmd\":\"sh verify-ok.sh\""), "契約 1 本目は緑: {log}");
    assert!(log.contains("\"n\":4,\"rc\":1,\"cmd\":\"sh verify-red.sh\""), "契約 2 本目が赤: {log}");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "1", "赤は 1 本");
    clean(&[&repo, &state]);
}

/// **write-set の外へ出た便は gate が落とす**（ADR-0009 §2.4・段①）。
///
/// guard（hook）は misbehave した runner のための backstop で、`sh -c` の runner には
/// 効かない。**器の側で数える**面がここである。
#[test]
fn pipe_gate_fails_when_diff_leaves_write_set() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // 契約の write-set は `src/lib.rs` だけ。runner が別 file も足す。
    // `src/lib.rs.bak` は **write-set の entry の接頭辞だが segment 境界で外れる** path。
    // これが無いと「`/` を 1 文字落とす」変異（`starts_with(trimmed)`）が生き残る。
    let runner = "echo x >> src/lib.rs && echo y > stray.md && echo z > src/lib.rs.bak \
                  && git add -A && git commit -q -m runner";
    let spawned = spawn_with(&repo, &state, &id, runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&spawned));
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "write-set の外は FAIL: {}", stderr_of(&out));

    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 1, "cmd"), "write-set", "段①は先頭（母集団 {} record）", rows.len());
    assert_eq!(row_value(&rows, 1, "rc"), "1", "外れた便は段①が赤い");
    // **lens は呼ばない**（赤い周は lens の顔色で通らない）。
    assert!(!marker.exists(), "段①が赤い周は lens を起動しない");
    let tail = fs::read_to_string(state.join("pipe").join(&id).join("verify.stderr.log"))
        .expect("verify.stderr.log を読める");
    assert!(tail.contains("stray.md"), "外れた path を列挙する: {tail}");
    assert!(tail.contains("src/lib.rs.bak"), "接頭辞が一致しても segment 境界で外れる: {tail}");
    // **内に収まる path は列挙しない**（`src/lib.rs.bak` を含む行を除いて数える＝字面の
    // 包含関係で assert が空虚にならないようにする）。
    assert!(
        !tail.lines().any(|line| line.contains("src/lib.rs") && !line.contains("src/lib.rs.bak")),
        "内に収まる path は列挙しない: {tail}"
    );

    // **弁別**: 同じ契約でも write-set の内に収まる便は段①が緑になる（`stray.md` を足さない）。
    // **bead を分ける**（run id は `<bead>-<秒>` なので、同じ秒の 2 便目は id が衝突する）。
    let second = write_contract(&repo, &[], &[]);
    let inside = intake_bead(&repo, &state, &second, "s2-41o");
    let ran = spawn_with(&repo, &state, &inside, TOY_COMMIT);
    assert_eq!(ran.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&ran));
    let ok = gate_once(&repo, &state, &inside, Some(&fake_lens(&state.join("lens-2"), &lens_verdict("PASS"))));
    assert_eq!(ok.status.code(), Some(i32::from(RC_OK)), "内に収まる便は通る: {}", stderr_of(&ok));
    assert_eq!(row_value(&verify_rows(&state, &inside), 1, "rc"), "0", "段①が緑");
    clean(&[&repo, &state]);
}

/// **共通 verify は便の写しから撃ち、契約の verify より前に来る**（段②→段③）。
///
/// `{base}` は共通 verify の行だけが置ける穴で、契約の行には置換しない（.56 の intake が
/// 契約行の穴を断っているので、契約側に穴は在り得ない）。
#[test]
fn pipe_gate_runs_common_verify_from_vessel_copy_before_contract() {
    let (repo, state) = repo_with_state();
    // git だけで rc 0 / rc≠0 になる 2 行（宣言の allowlist は git / sh）。
    commit_vessel(
        &repo,
        VESSEL_ALLOWED,
        r#"["git rev-parse --verify {base}", "git cat-file -e {base}:no-such-file"]"#,
    );
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "共通 verify が赤ければ FAIL: {}", stderr_of(&out));

    let rows = verify_rows(&state, &id);
    assert_eq!(rows.len(), 4, "母集団 = write-set 1 + 共通 2 + 契約 1");
    assert_eq!(row_value(&rows, 1, "cmd"), "write-set", "段①");
    assert_eq!(row_value(&rows, 2, "rc"), "0", "共通の 1 本目は緑");
    assert_ne!(row_value(&rows, 3, "rc"), "0", "共通の 2 本目は赤");
    assert_eq!(row_value(&rows, 4, "cmd"), "sh verify-ok.sh", "契約の行は共通の後ろ");
    // **`{base}` は置換されている**（穴のまま撃つと `git rev-parse --verify {base}` は赤い）。
    assert_eq!(
        row_value(&rows, 2, "cmd"),
        format!("git rev-parse --verify {base}"),
        "共通の行の穴は便の base へ置換される"
    );
    // 「契約の行には置換しない」は **intake が契約行の穴を断つ**ので gate では観測できない
    // （穴を持つ契約は run にならない）。その保証は
    // `pipe_intake_refuses_contract_verify_with_placeholder` が持つ＝ここでは測らない。
    assert!(!marker.exists(), "段②が赤い周も lens を起動しない");
    clean(&[&repo, &state]);
}

/// **便の実装が worktree の宣言を書き換えても、gate は写しの行を撃つ**（ADR-0010 §2.4）。
///
/// 読み直す実装だと、便が自分の検証を消して通れる（自己拡張）。
#[test]
fn pipe_gate_ignores_worktree_vessel_declaration() {
    let (repo, state) = repo_with_state();
    // 宣言も write-set に入れる＝段①は緑のまま段②だけを測る。
    let path = write_contract(&repo, &["write-set"], &[r#"write-set = ["src/lib.rs", ".vessel.toml"]"#]);
    let id = intake(&repo, &state, &path);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    // runner が worktree の宣言を「必ず赤くなる共通 verify」へ書き換えて commit する。
    let runner = "printf 'schema = 1\\nallowed-commands = [\"git\"]\\n\
                  common-verify = [\"git cat-file -e HEAD:no-such-file\"]\\n' > .vessel.toml \
                  && echo x >> src/lib.rs && git add -A && git commit -q -m runner";
    let spawned = spawn_with(&repo, &state, &id, runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&spawned));
    // **repo 側の宣言も intake の後に赤い行へ差し替える**＝写しからしか読まないことを
    // worktree 面と repo 面の 2 面で縛る（main が進んだ周に凍っていない行を撃たない）。
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["git cat-file -e HEAD:no-such-file"]"#);
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "写しの行は緑なので通る: {}", stderr_of(&out));

    let rows = verify_rows(&state, &id);
    assert_eq!(
        row_value(&rows, 2, "cmd"),
        format!("git rev-parse --verify {base}"),
        "撃つのは**写し**の行"
    );
    let log = fs::read_to_string(state.join("pipe").join(&id).join("verify.jsonl"))
        .expect("verify.jsonl を読める");
    assert!(!log.contains("no-such-file"), "worktree の宣言は読み直さない: {log}");
    // worktree 側の宣言が実際に書き換わっていることも測る（**測っていない**を「通った」に
    // 化けさせないため＝runner が何もしていなければこの歯は空虚になる）。
    let changed = fs::read_to_string(worktree_of(&repo, &id).join(".vessel.toml"))
        .expect("worktree の宣言を読める");
    assert!(changed.contains("no-such-file"), "runner は宣言を書き換えている: {changed}");
    clean(&[&repo, &state]);
}

/// 赤い verify 行の stderr の末尾が診断 file に残る（緑の行は残さない）。
///
/// `verify.jsonl` の rc だけでは「何がどう赤いか」が便の外から読めない。**stderr の
/// 本文で測る**——見出し行にも `cmd=` として字面が載るので、cmd に**無い**字面
/// （`boom` は `printf` が組み立てる）で「写しが空でない」を弁別する。
#[test]
fn pipe_gate_keeps_stderr_tail_of_red_verify_lines() {
    let (repo, state) = repo_with_state();
    // 2 行目が rc 3 で stderr に 1 行出す。cmd の字面には `boom` が無い。
    let path = write_contract(
        &repo,
        &["verify"],
        &[r#"verify = ["sh verify-ok.sh", "sh verify-noisy.sh"]"#],
    );
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1");
    assert!(stdout_of(&out).contains("verdict=FAIL"), "{}", stdout_of(&out));

    // record の形は変えない（schema 不変）。rc は逐条のまま `verify.jsonl` に在る。
    let log = fs::read_to_string(state.join("pipe").join(&id).join("verify.jsonl"))
        .expect("verify.jsonl を読める");
    let rows: Vec<&str> = log.lines().collect();
    // 母集団 = 4 record（write-set 照合 1 + 写しの共通 verify 1 + 契約 2 本）。
    assert_eq!(rows.len(), 4, "verify は逐条で残る: {log}");
    let second =
        vessel::fleet::json_lite::parse_object(rows.get(3).copied().unwrap_or_default().trim())
            .expect("4 行目は 1 行の JSON");
    assert_eq!(value_of(&second, "n"), "4", "赤いのは契約の 2 本目: {log}");
    assert_eq!(value_of(&second, "rc"), "3", "赤い契約行の rc: {log}");

    let tail = fs::read_to_string(state.join("pipe").join(&id).join("verify.stderr.log"))
        .expect("verify.stderr.log を読める");
    let heads: Vec<&str> = tail.lines().filter(|line| line.starts_with("## ")).collect();
    assert_eq!(
        heads.len(),
        1,
        "見出しは赤い行の分だけ（母集団 {} 行）: {tail}",
        tail.lines().count()
    );
    let head = heads.first().copied().unwrap_or_default();
    assert!(head.contains("n=4 rc=3"), "見出しは赤い行を名指す: {head}");
    assert!(!head.contains("boom"), "見出しの cmd= に boom の字面は無い: {head}");
    assert!(!tail.contains("## n=3"), "緑の行は見出しを残さない: {tail}");
    assert!(tail.contains("boom"), "stderr の本文が残る: {tail}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_without_lens_when_required() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 規則は lens を 1 本要る（gate.lens_count = 1）が `--lens` が無い。
    let out = gate_once(&repo, &state, &id, None);
    assert_eq!(out.status.code(), Some(3), "判定できない周の rc は 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE");
    assert!(
        value_of(&pairs, "evidence").contains("--lens が無い"),
        "理由が残る: {}",
        value_of(&pairs, "evidence")
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_when_diff_exceeds_cap() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // cap を 1 byte にした manifest を渡す（**数値は規則から来る**ことを測る）。
    let rules = write_rules(&repo, "tight.toml", 1, 1);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "gate", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &rules.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(3), "cap 超過の rc は 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    // **lens を呼ばない**のが cap の意味である（呼んでから捨てるのでは予算を守れない）。
    assert!(!marker.exists(), "cap 超過の周は lens を起動しない");
    let pairs = verdict_pairs(&state, &id);
    assert!(
        value_of(&pairs, "evidence").contains("cap 1"),
        "cap の値は規則から来る: {}",
        value_of(&pairs, "evidence")
    );
    let bytes: u64 = value_of(&pairs, "diff_bytes").parse().unwrap_or(0);
    assert!(bytes > 1, "diff の byte 数を実測して比べている: {bytes}");

    // **境界**: 設計は「diff byte > cap → INCONCLUSIVE」＝等号は超えていない。
    // 同じ内容の別便を cap = ちょうどその byte 数で撃ち、PASS 側に残ることを測る。
    // INCONCLUSIVE は終端でない＝同じ write-set の便と交差する（`s2-07l.145`）ので、
    // 測り終えた 1 本目を `stop --run` で外してから双子を起こす。
    stop_run_ok(&state, &id);
    let twin = intake_bead(&repo, &state, &path, "s2-edge");
    let out = run_pipe(&[
        "spawn", "--run", &twin, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    let exact = write_rules(&repo, "exact.toml", 1, bytes);
    let edge = gate_with_rules(&repo, &state, &twin, &exact, &lens);
    assert_eq!(
        edge.status.code(),
        Some(i32::from(RC_OK)),
        "cap ちょうどは超えていない（> であって >= でない）: {}",
        stdout_of(&edge)
    );
    assert_eq!(
        value_of(&verdict_pairs(&state, &twin), "diff_bytes"),
        bytes.to_string(),
        "同じ内容の便なので diff の byte 数も同じ"
    );
    clean(&[&repo, &state]);
}

/// **verdict.json の key 列を完全一致で pin する**（設計 §5.3）。
///
/// 偽 `systemd-run` と偽 `systemctl`（殺した）を積み、**包める周に固定**して撃つ。素の環境で
/// 撃つと包めるかは runner の周ごとに変わり、`scope` が載る周と欠ける周が混ざる（PR #154 の
/// CI で FAIL → 再実行で success・`s2-07l.236`）。欠ける面は [`pipe_gate_verdict_scope_is_a_closed_name_or_absent`]。
#[test]
fn pipe_gate_records_structured_verdict() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    systemctl_stub(&state, SYSTEMCTL_KILLED);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let (id, out) = confined_run(&repo, &state, &path, &lens);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS の rc は 0: {}", stderr_of(&out));
    assert!(marker.exists(), "判定に届いた周は lens を起動する");
    let pairs = verdict_pairs(&state, &id);
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["schema", "run", "verdict", "evidence", "verify_red", "diff_bytes", "tree", "scope", "ts"],
        "verdict.json の key 列（設計 §5.3・包めた周は lens の片付けの `scope` が載る）"
    );
    assert_eq!(value_of(&pairs, "scope"), "killed", "lens の scope に残りを殺した周");
    assert_eq!(value_of(&pairs, "schema"), "1");
    assert_eq!(value_of(&pairs, "run"), id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS");
    assert_eq!(value_of(&pairs, "evidence"), "fake", "lens の evidence を写す");
    assert_eq!(value_of(&pairs, "verify_red"), "0");
    assert!(
        show_line(&repo, &state, &id).contains("stage=Gated"),
        "段が Gated へ動く: {}",
        show_line(&repo, &state, &id)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_refuses_run_without_commits() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // commit を作らない runner。spawn は Failed で終える（commit 0 は完了ではない）。
    run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", "true",
    ]);
    // 段だけを Implemented へ書き換える（＝台帳が壊れている / 手で進めた周）。
    // worktree は在って clean なので、**commits の検査だけ**が gate を止める。
    let forced = Command::new(bin())
        .args(["fleet", "record", "--state-dir"])
        .arg(&state)
        .args(["--kind", "RunStage", "--stage", "Implemented", "--run", &id, "--bead", "s2-2e5"])
        .output()
        .expect("binary を起動できる");
    assert_eq!(forced.status.code(), Some(i32::from(RC_OK)), "record は rc 0");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "commit 0 は前提違反で rc 1");
    assert!(
        stderr_of(&out).contains("commit が 1 本も無い"),
        "理由は commits（dirty ではない）: {}",
        stderr_of(&out)
    );
    assert!(!marker.exists(), "前提違反の周は lens を起動しない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_on_unlisted_lens_verdict() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    // 3 値の外を名乗る lens。**PASS へ倒さない**（AC3「偽の PASS 0 件」）。
    let lens = fake_lens(&marker, &lens_verdict("OK"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(3), "3 値外は rc 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    assert!(marker.exists(), "lens 自体は呼んでいる（判定に届いた周）");
    assert_eq!(
        value_of(&verdict_pairs(&state, &id), "verdict"),
        "INCONCLUSIVE",
        "未知の verdict を通さない"
    );
    clean(&[&repo, &state]);
}

// ── lens review（2026-09-09）で「測っていない」と名指しされた経路を塞ぐ歯 ──────

#[test]
fn pipe_gate_inconclusive_when_lens_count_is_not_one() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // 0 本（lens を呼ばずに通す）も 2 本（1 本で足りたことにする）も**判定できていない**。
    // 規則 1 行で gate が飾りになる形を塞ぐ（AC3「偽の PASS 0 件」）。
    for (count, bead) in [(0_u64, "s2-zero"), (2, "s2-two")] {
        let id = intake_bead(&repo, &state, &path, bead);
        let out = run_pipe(&[
            "spawn", "--run", &id, "--repo", &repo.display().to_string(),
            "--state-dir", &state.display().to_string(),
            "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
        ]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
        let rules = write_rules(&repo, &format!("lens{count}.toml"), count, 150_000);
        let gated = gate_with_rules(&repo, &state, &id, &rules, &lens);
        assert_eq!(gated.status.code(), Some(3), "lens {count} 本は判定不能で rc 3");
        assert!(
            stdout_of(&gated).contains("verdict=INCONCLUSIVE"),
            "lens {count} 本: {}",
            stdout_of(&gated)
        );
        // **land まで行かせない**（面 5 に PASS を残さない）。
        let landed = land_once(&repo, &state, &id);
        assert_eq!(landed.status.code(), Some(i32::from(RC_REFUSED)), "PASS でなければ land しない");
        // INCONCLUSIVE は終端でない＝次の便と write-set が交差する（`s2-07l.145`）。測り終えた
        // 便を `stop --run` で外してから次の周を回す（測る内容は 1 つも変えていない）。
        stop_run_ok(&state, &id);
    }
    assert!(!marker.exists(), "0 本の周は lens を起動しない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_when_lens_exits_nonzero() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // JSON は正しく吐くが rc≠0 で終える lens。**出力を信じて PASS へ倒さない**。
    let lens = format!("cat >/dev/null; echo '{}'; exit 7", lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(3), "lens が rc≠0 なら rc 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    assert!(
        value_of(&verdict_pairs(&state, &id), "evidence").contains("rc 7"),
        "理由に lens の rc が残る: {}",
        value_of(&verdict_pairs(&state, &id), "evidence")
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_when_lens_output_is_not_json() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // rc 0 だが JSON 行が無い lens。**読めなかったを通ったに化けさせない**。
    let lens = "cat >/dev/null; echo looks-fine".to_owned();
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(3), "parse 不能なら rc 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    assert!(
        value_of(&verdict_pairs(&state, &id), "evidence").contains("JSON 行が無い"),
        "理由: {}",
        value_of(&verdict_pairs(&state, &id), "evidence")
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_passes_diff_to_lens_on_stdin() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let seen = state.join("stdin-bytes");
    // lens が **実際に受け取った byte 数**を書き出す（設計 §5.3 / FR9 の中心）。
    let lens = format!("wc -c > '{}'; echo '{}'", seen.display(), lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let received = fs::read_to_string(&seen).expect("lens が受けた byte 数を読める");
    let received = received.trim().to_owned();
    assert_ne!(received, "0", "diff を渡さずに lens を呼んでいない");
    assert_eq!(
        received,
        value_of(&verdict_pairs(&state, &id), "diff_bytes"),
        "lens が受けた byte 数と verdict.json の diff_bytes は同じ diff を指す"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_substitutes_contract_placeholder_in_lens_cmd() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let seen = state.join("lens-contract-arg");
    // fake lens が **受け取った argv**（置換後の cmd に埋まった path）を写す。
    // 置換していなければ `{contract}` の字面がそのまま残る。
    // **穴は 2 つ置く**。1 つだけだと `replacen(_, _, 1)` へ縮める変異が生き残る
    // （review 2026-09-10）。2 つ目が置換されなければ字面のまま file に残る。
    let lens = format!(
        "printf '%s\\n%s' '{{contract}}' '{{contract}}' > '{}'; cat >/dev/null; echo '{}'",
        seen.display(),
        lens_verdict("PASS")
    );
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let handed = fs::read_to_string(&seen).expect("lens が受けた値を読める");
    let handed: Vec<&str> = handed.lines().collect();
    assert_eq!(handed.len(), 2, "穴 2 つ分が渡る: {handed:?}");
    assert!(
        !handed.iter().any(|line| line.contains("{contract}")),
        "placeholder が 1 つでも置換されずに渡っている: {handed:?}"
    );
    let handed = handed.first().copied().unwrap_or_default().trim().to_owned();
    // **path であって本文ではない**（cmd は `sh -c` の 1 行なので、本文を埋めると
    // 契約の中の引用符 1 つで cmd の構造が変わる）。
    let handed = PathBuf::from(handed.trim());
    assert!(handed.is_absolute(), "契約 copy の絶対 path が渡る: {}", handed.display());
    let body = fs::read_to_string(&handed).expect("lens は渡された path から契約を読める");
    // 渡ったのが **この便の契約 copy** であること（別の file を指していない）。
    assert!(body.contains("縦 1 本を通す"), "契約の goal が読める: {body}");
    assert_eq!(
        handed,
        state.join("pipe").join(&id).join("contract.toml"),
        "run の契約 copy を指す"
    );
    clean(&[&repo, &state]);
}

/// gate は `--lens` の cmd の `{worktree}` へ **便の worktree** を埋める。
///
/// 本契約（`s2-07l.60`）の実利は「lens の context に憲法を載せる」ことで、その唯一の
/// 経路が gate → `{worktree}` → lens の `cwd` である。`substitute()` という純関数の中
/// だけを測る歯では、**呼び手が別の path を穴へ入れる退行**を捕まえられない——実測
/// 2026-09-10: `ask_lens(&substitute(cmd, &contract, worktree), …)` の第 3 引数を
/// `entry.state_dir` へ差し替えても workspace の 297 本が 1 本も落ちなかった。
/// ゆえに**穴の中身が正しいか**をここで測る（`{contract}` 側と対称にする）。
#[test]
fn pipe_gate_substitutes_worktree_placeholder_in_lens_cmd() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let seen = state.join("lens-worktree-arg");
    // fake lens が **受け取った argv** を写す。置換していなければ `{worktree}` の字面が残る。
    let lens = format!(
        "printf '%s' '{{worktree}}' > '{}'; cat >/dev/null; echo '{}'",
        seen.display(),
        lens_verdict("PASS")
    );
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let handed = fs::read_to_string(&seen).expect("lens が受けた値を読める");
    assert!(
        !handed.contains("{worktree}"),
        "placeholder が置換されずに渡っている: {handed}"
    );
    let handed = PathBuf::from(handed.trim());
    assert!(handed.is_absolute(), "絶対 path が渡る: {}", handed.display());
    // **便の worktree ちょうど**を指す。
    assert_eq!(
        handed,
        repo.join(".worktrees").join("scribe2").join(&id),
        "run の worktree を指す"
    );
    // 近い path を渡す退行を負例で外す（置き場も anchor の repo も worktree ではない）。
    assert_ne!(handed, state, "置き場を渡していない");
    assert_ne!(handed, repo, "anchor の repo を渡していない");
    // 憲法が載る経路である＝渡った dir は便の base の checkout である。
    assert!(
        handed.join(".git").exists(),
        "worktree の checkout を指す: {}",
        handed.display()
    );
    clean(&[&repo, &state]);
}


#[test]
fn pipe_gate_refuses_wrong_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let before = event_count(&state);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // Intake の便に gate は掛からない。**段違いは何もせず rc 1**（Failed で終端させない）。
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "段違いは rc 1");
    assert!(stderr_of(&out).contains("段は Intake である"), "理由: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before, "段違いは event を 1 件も書かない");
    assert!(!marker.exists(), "lens を起動しない");
    // 終端していないので、正しい段まで進めれば通る（resume できる）。
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    // 逆向きも同じ: Implemented の便に land は掛からない。
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_REFUSED)), "gate 前の land は rc 1");
    assert!(
        stderr_of(&landed).contains("段は Implemented である"),
        "理由: {}",
        stderr_of(&landed)
    );
    // **判定が済んだ**便に gate は 2 度掛からない（測り直せるのは INCONCLUSIVE の
    // 周だけ・設計 §5.3。ここは PASS ゆえ終端側である）。
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "1 度目は通る: {}", stderr_of(&gated));
    let again = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "2 度目の gate は rc 1");
    assert!(stderr_of(&again).contains("段は Gated である"), "理由: {}", stderr_of(&again));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_regates_after_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 道具が足りない周（`--lens` を渡し忘れた便）。INCONCLUSIVE は「測れなかった」で
    // あって「落ちた」ではないので、**ここで終端しない**。
    let first = gate_once(&repo, &state, &id, None);
    assert_eq!(first.status.code(), Some(3), "測れなかった周の rc は 3");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "INCONCLUSIVE");

    // 道具を揃えて撃ち直す。**段が Gated でも通る**のが本便で広げた 1 分岐である。
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let second = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(
        second.status.code(),
        Some(i32::from(RC_OK)),
        "道具を揃えた測り直しは通る: {}",
        stderr_of(&second)
    );
    assert!(marker.exists(), "2 度目は lens を起動する");
    assert_eq!(
        value_of(&verdict_pairs(&state, &id), "verdict"),
        "PASS",
        "verdict.json は新しい判定で上書きされる"
    );

    // **前の INCONCLUSIVE は event に残る**（append-only・書き換えない）。
    let gated: Vec<String> = events(&state)
        .into_iter()
        .filter(|event| event.run == id && event.stage == Some(Stage::Gated))
        .filter_map(|event| event.detail)
        .collect();
    assert_eq!(
        gated,
        vec!["verdict:INCONCLUSIVE".to_owned(), "verdict:PASS".to_owned()],
        "測り直しは 2 件目を追記する（1 件目を書き換えない）"
    );
    // 吸収状態が解けている＝そのまま land まで進む。
    let landed = land_once(&repo, &state, &id);
    assert_eq!(
        landed.status.code(),
        Some(i32::from(RC_OK)),
        "測り直した便は land できる: {}",
        stderr_of(&landed)
    );
    // **land 済みの便は測り直せない**。ここを開けると Gated を書き直して land を 2 度
    // 通す口になる（測り直しが開くのは Gated の 1 段だけ）。
    let after_land = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(
        after_land.status.code(),
        Some(i32::from(RC_REFUSED)),
        "Landed からの再 gate は rc 1"
    );
    assert!(
        stderr_of(&after_land).contains("段は Landed である"),
        "理由は段である: {}",
        stderr_of(&after_land)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_regates_after_relaxing_cap() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // **cap 超過も「測れなかった」**（lens を呼べていない）。契約が名指す測り直しの
    // 主用途はここで、`--lens` の渡し忘れだけを測っていると `--rules` 経由の周が丸ごと
    // 素通りする。
    let tight = write_rules(&repo, "tight.toml", 1, 1);
    let first = gate_with_rules(&repo, &state, &id, &tight, &lens);
    assert_eq!(first.status.code(), Some(3), "cap 超過の rc は 3");
    assert!(!marker.exists(), "cap 超過の周は lens を起動しない");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE");
    let bytes: u64 = value_of(&pairs, "diff_bytes").parse().unwrap_or(0);

    // **道具を揃えずに撃ち直しても通らない**（測り直しは検査を素通りする口ではない）。
    let retight = gate_with_rules(&repo, &state, &id, &tight, &lens);
    assert_eq!(retight.status.code(), Some(3), "cap のままなら 2 度目も 3");
    assert!(!marker.exists(), "2 度目も cap 超過なら lens を起動しない");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "INCONCLUSIVE");

    // 予算を実測値まで緩めて撃ち直す（道具を揃えるのは人の手番）。
    let relaxed = write_rules(&repo, "relaxed.toml", 1, bytes);
    let second = gate_with_rules(&repo, &state, &id, &relaxed, &lens);
    assert_eq!(
        second.status.code(),
        Some(i32::from(RC_OK)),
        "cap を緩めた測り直しは通る: {}",
        stderr_of(&second)
    );
    assert!(marker.exists(), "2 度目は lens を起動する");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS");
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_regates_after_fixing_lens_count() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // 規則が lens 2 本を定める周は「1 本で足りたことにしない」ため INCONCLUSIVE。
    // **規則の側の不備も道具の不足**なので、直せば同じ便を測り直せる。
    let two = write_rules(&repo, "two-lenses.toml", 2, 100_000);
    let first = gate_with_rules(&repo, &state, &id, &two, &lens);
    assert_eq!(first.status.code(), Some(3), "lens_count≠1 の rc は 3");
    assert!(!marker.exists(), "本数が合わない周は lens を起動しない");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "INCONCLUSIVE");

    // **規則を直さずに撃ち直しても通らない**（測り直しは本数照合を外す口ではない）。
    let retry = gate_with_rules(&repo, &state, &id, &two, &lens);
    assert_eq!(retry.status.code(), Some(3), "規則がそのままなら 2 度目も 3");
    assert!(!marker.exists(), "本数が合わないままなら lens を起動しない");

    let one = write_rules(&repo, "one-lens.toml", 1, 100_000);
    let second = gate_with_rules(&repo, &state, &id, &one, &lens);
    assert_eq!(
        second.status.code(),
        Some(i32::from(RC_OK)),
        "規則を直した測り直しは通る: {}",
        stderr_of(&second)
    );
    assert!(marker.exists(), "2 度目は lens を起動する");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_fails_regate_on_dirty_worktree() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let first = gate_once(&repo, &state, &id, None);
    assert_eq!(first.status.code(), Some(3), "測れなかった周の rc は 3");

    // 道具（lens・cap）を揃える過程で worktree を汚した周。**測り直しに来た便でも
    // worktree の事実の違反は Failed で終端する**——precheck の極性を段で変えると
    // 「段の検査は cli / worktree の事実は gate」の分離が濁るため（planner 裁定
    // 2026-09-10 Q3 案B）。運用は「道具を揃える前に worktree を clean へ戻す」。
    fs::write(worktree_of(&repo, &id).join("dirty.txt"), "x\n").expect("汚せる");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "前提違反は rc 1");
    assert!(stderr_of(&out).contains("clean でない"), "理由: {}", stderr_of(&out));
    assert!(!marker.exists(), "lens を起動しない");
    assert!(
        show_line(&repo, &state, &id).contains("stage=Failed"),
        "測り直しの周も precheck 違反は終端する: {}",
        show_line(&repo, &state, &id)
    );

    // **掃除しても引けない**（Failed は終端）。この非対称は測って残す。
    fs::remove_file(worktree_of(&repo, &id).join("dirty.txt")).expect("掃除できる");
    let again = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "Failed からは撃ち直せない");
    assert!(
        stderr_of(&again).contains("段は Failed である"),
        "理由: {}",
        stderr_of(&again)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_refuses_regate_after_fail() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let failed_marker = state.join("lens-fail");
    let failing = fake_lens(&failed_marker, &lens_verdict("FAIL"));
    let first = gate_once(&repo, &state, &id, Some(&failing));
    assert_eq!(first.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "FAIL");

    // **FAIL は終端のまま**。契約の verify が赤い便は測り直しても赤いので、撃ち直す口を
    // 開けない（開けると「壊れたまま進む」経路になる）。**段違いの一般則どおり何もしない**。
    let before = event_count(&state);
    let passing_marker = state.join("lens-pass");
    let passing = fake_lens(&passing_marker, &lens_verdict("PASS"));
    let second = gate_once(&repo, &state, &id, Some(&passing));
    assert_eq!(second.status.code(), Some(i32::from(RC_REFUSED)), "FAIL からの再 gate は rc 1");
    assert!(
        stderr_of(&second).contains("FAIL"),
        "断る理由は「段は Gated」ではなく判定である: {}",
        stderr_of(&second)
    );
    assert!(!passing_marker.exists(), "lens を起動しない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    assert_eq!(
        value_of(&verdict_pairs(&state, &id), "verdict"),
        "FAIL",
        "判定は書き換わらない（PASS の lens を渡しても）"
    );
    // resume も FAIL を測り直しへ案内しない（`next=` を名乗るのは INCONCLUSIVE だけ）。
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の resume は rc 1");
    assert!(
        !stdout_of(&resumed).contains("next="),
        "次の一手を名乗らない（終端である）: {}",
        stdout_of(&resumed)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_refuses_regate_without_readable_verdict() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let first = gate_once(&repo, &state, &id, None);
    assert_eq!(first.status.code(), Some(3), "測れなかった周の rc は 3");

    // **判定が読めない周は測り直さない**（fail-closed・C11.2）。前提は「Gated ∧ verdict が
    // INCONCLUSIVE」であって「Gated ∧ PASS でも FAIL でもない」ではない——後者だと
    // verdict.json が消えた / 壊れた便まで撃ち直せてしまい、「測れなかった」ではなく
    // **判定の記録が無い**便が gate を通る（自前の変異 M04 が生き延びた経路）。
    let judged = state.join("pipe").join(&id).join("verdict.json");
    let before = event_count(&state);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // `verdict_of` は 3 つの経路を等しく「読めない」へ畳む。**3 つとも測る**——1 つだけ
    // 測ると、残り 2 つを INCONCLUSIVE へ倒す実装（＝測れなかった便を測り直してよい便に
    // 化けさせる）が素通りする。
    for (label, body) in [
        ("file が無い", None),
        ("JSON が壊れている", Some("{ 壊れた\n")),
        (
            "verdict が 3 値の外",
            Some(r#"{"schema":1,"run":"x","verdict":"MAYBE","evidence":"","verify_red":0,"diff_bytes":0,"ts":"t"}"#),
        ),
    ] {
        match body {
            None => {
                fs::remove_file(&judged).ok();
            }
            Some(text) => fs::write(&judged, text).expect("verdict.json を書ける"),
        }
        let out = gate_once(&repo, &state, &id, Some(&lens));
        assert_eq!(
            out.status.code(),
            Some(i32::from(RC_REFUSED)),
            "{label}: 読めない周は rc 1"
        );
        assert!(
            stderr_of(&out).contains("読めない"),
            "{label}: **判定が読めない**と名乗る（INCONCLUSIVE と同じ扱いにしない）: {}",
            stderr_of(&out)
        );
        assert!(!marker.exists(), "{label}: lens を起動しない");
        assert_eq!(event_count(&state), before, "{label}: event を 1 件も書かない");
    }
    clean(&[&repo, &state]);
}

/// `CHECKS` の並びが**宣言順**と一致する（ADR-0013 D2）。**この並びが適用順序である**ので、
/// 乖離は段の実行順が静かに変わることを意味する。
#[test]
fn gate_checks_follow_declaration_order() {
    assert!(
        is_declaration_order(CHECKS, |check| check as usize),
        "CHECKS の並びが宣言順と乖離している（母集団 {} 段）",
        CHECKS.len()
    );
}

/// `VERDICTS` の並びが**宣言順**と一致する（ADR-0013 D2）。
#[test]
fn gate_verdicts_follow_declaration_order() {
    assert!(
        is_declaration_order(VERDICTS, |verdict| verdict as usize),
        "VERDICTS の並びが宣言順と乖離している（母集団 {} 値）",
        VERDICTS.len()
    );
}

/// gate の段①（write-set 照合）で diff を**読めない**周は「測れなかった」であって赤ではない
/// （`s2-07l.65`・`.57` 申し送り・land の `MainCheck::Unmeasurable` と同じ極性）。
///
/// 読めない状態は **base commit の tree object を消して**作る（実測: `git status` と
/// `git rev-list --count` は通り、`git diff --name-only -z <base>..HEAD` だけが `unable to read tree`
/// で落ちる＝precheck を抜けて段①に届く）。verdict は INCONCLUSIVE（既存 3 値の内側・rc 3）で、
/// lens は呼ばれず（判定に届いていない）、verify.jsonl の段①の行は残る（現物を消さない）。
///
/// 消した後も tree が読める周が CI で出た（`s2-07l.185`・rc 1 の flaky）。読める経路の推定:
/// spawn の worktree は object store を共有し、commit が起こす detached の auto maintenance が
/// loose を pack へ詰めた周だけ読める（commit-graph は root tree の oid しか持たず中身は運ばない）。
///
/// そこで fixture の auto maintenance を seed の前に止め、消した直後に「読めない」を**前提として**
/// assert する——崩れた周は `count-objects -v` の値付きで理由を出して落ちる（偽の緑にも偽の赤にもしない）。
// flip-check: retroactive s2-07l.185
#[test]
fn pipe_gate_turns_unreadable_diff_into_inconclusive() {
    let (repo, state) = repo_with_state_configured(&tmp().join(STATE_LEAF), NO_AUTO_MAINTENANCE);
    // 契約行に**赤い行を 1 本**混ぜる: 「測れなかったは赤より先」（判定順）と「段①の -1 は赤に
    // 数えないが ②③ の赤は数える」（`verify_red` = 1）を同じ便で測る。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-ok.sh", "sh verify-red.sh"]"#]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let id = implemented(&repo, &state, &path);
    let tree = git(&repo, &["rev-parse", &format!("{base}^{{tree}}")]);
    let (head, tail) = tree.split_at(2);
    let object = repo.join(".git").join("objects").join(head).join(tail);
    fs::remove_file(&object).expect("base の tree object を消せる");
    let readable = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["cat-file", "-e", &tree])
        .output()
        .expect("git を起動できる");
    assert!(
        !readable.status.success(),
        "前提: 読めない状態を作れなかった（loose を消しても tree {tree} が読める）: count-objects -v = {}",
        git(&repo, &["count-objects", "-v"]).replace('\n', " ")
    );
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(3), "測れなかった周は INCONCLUSIVE の rc: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "赤い契約行が在っても、測れなかったが先");
    assert_eq!(value_of(&pairs, "verify_red"), "1", "段①の -1 は赤に数えず ②③ の赤（1 本）だけを数える");
    assert!(value_of(&pairs, "evidence").contains("読めない"), "理由が残る: {}", value_of(&pairs, "evidence"));
    assert!(!marker.exists(), "判定に届いていないので lens は呼ばない");
    let log = fs::read_to_string(vessel::pipe::verify_log_path(&state, &id)).expect("verify.jsonl を読める");
    let first = log.lines().next().unwrap_or_default();
    assert!(first.contains("\"n\":1") && first.contains("\"cmd\":\"write-set\""), "段①の record は残る: {first}");
    assert!(first.contains("\"rc\":255"), "段①の rc -1 は u64 の記録形 255 で残る（schema 不変）: {first}");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated へ進む（測り直せる）");
    clean(&[&repo, &state]);
}

// ---- 封じ込め（設計 docs/design/gate-cost.md §4・ADR-0021 §2.2）--------------------------

/// 偽 `systemd-run` の記録を置く dir 名（**起動ごとに 1 file**）。
const SCOPE_RECORDS: &str = "scope-args";

/// 偽 `systemd-run`（と偽 `systemctl`）を置く dir 名。
const SYSTEMD_BIN: &str = "systemd-bin";

/// PATH の先頭に置く偽 `systemd-run`（返すのは PATH の値）。
///
/// argv を写してから `--` の後ろを exec する＝**包みの中身は実際に撃たれる**。
///
/// 記録は **`<unit>.args` の 1 起動 1 file** である。1 file へ追記する形は、probe の記録や
/// 別の行の記録まで同じ母集団に入り、`contains` の assert が**撃っていない起動の引数**で
/// 充足する（run 1 の実測: `limit_of` を常に `PerJob` にする変異で 21/21 が緑だった）。
///
/// **同じ名の 2 本目は実 systemd と同じ字面で断る**（`s2-07l.234`・.208 run 3 の stderr 逐語）——
/// 記録を上書きする形だと、同じ process が同名を 2 度撃つ周が歯に見えない。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn systemd_stub(state: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = state.join(SYSTEMD_BIN);
    let records = state.join(SCOPE_RECORDS);
    fs::create_dir_all(&bin_dir).expect("stub の dir を作れる");
    fs::create_dir_all(&records).expect("記録の dir を作れる");
    let shim = bin_dir.join("systemd-run");
    let script = format!(
        "#!/bin/sh\n\
         __unit=no-unit\n\
         for __a in \"$@\"; do case \"$__a\" in --unit=*) __unit=${{__a#--unit=}};; esac; done\n\
         if [ -e '{0}'/\"$__unit\".args ]; then\n\
         printf 'Failed to start transient scope unit: Unit %s.scope was already loaded or has a fragment file.\\n' \"$__unit\" >&2\n\
         exit 1\n\
         fi\n\
         printf '%s\\n' \"$@\" > '{0}'/\"$__unit\".args\n\
         while [ $# -gt 0 ] && [ \"$1\" != \"--\" ]; do shift; done\n\
         shift\n\
         exec \"$@\"\n",
        records.display()
    );
    fs::write(&shim, script).expect("stub を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("stub に実行権を付ける");
    format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default())
}

/// `needle` を名に含む scope 記録の**ちょうど 1 件**の本文（1 行 1 引数）。
///
/// 0 件も 2 件以上も `panic` にするのは、母集団を確かめずに `contains` すると、別の起動の
/// 引数で assert が充足するからである（fixture 衝突）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn scope_record(state: &Path, needle: &str) -> String {
    let dir = state.join(SCOPE_RECORDS);
    let names = dir_names(&dir);
    let hits: Vec<&String> = names.iter().filter(|name| name.contains(needle)).collect();
    assert_eq!(hits.len(), 1, "{needle} の記録はちょうど 1 件（母集団 {names:?}）");
    let name = hits.first().expect("1 件在る");
    fs::read_to_string(dir.join(name)).expect("記録を読める")
}

/// scope 記録の `-p <KEY>=<値>` の値（無ければ空）。
fn scope_prop(record: &str, key: &str) -> String {
    let head = format!("{key}=");
    record
        .lines()
        .find_map(|line| line.strip_prefix(&head))
        .unwrap_or_default()
        .to_owned()
}

/// PATH を差し替えて spawn → gate まで通す（gate の rc と便 id を返す）。
fn confined_run(repo: &Path, state: &Path, path: &str, lens: &str) -> (String, Output) {
    let contract = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &contract);
    let spawned = run_pipe_with_path(
        path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT],
    );
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));
    // `--rules` の tmp manifest で撃つ（受付の待ちの上限を fixture の値にする）。
    let gated = run_pipe_with_path(
        path,
        &["gate", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--lens", lens,
          "--rules", &ceiling_rules(state)],
    );
    (id, gated)
}

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
    let contract = write_contract(
        &repo,
        &["verify"],
        &[r#"verify = ["sh verify-peak.sh", "sh verify-ok.sh"]"#],
    );
    let id = intake(&repo, &state, &contract);
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

/// **箱の中で殺された verify 行は赤ではなく「測れなかった」**（設計 §4.2）。
///
/// rc は 0 のままの fixture で撃つ＝根拠が rc ではなく終端行の `oom_kill` であることを測る。
#[test]
fn pipe_confine_oom_verify_line_is_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let contract = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-oom.sh"]"#]);
    let id = intake(&repo, &state, &contract);
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
    let contract = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &contract);
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

// ---- 行の終端の scope の片付け（設計 gate-cost.md §4.4 errata・s2-07l.234）------------------

/// 偽 `systemctl` が撃たれた argv を 1 行 1 呼出で追記する file 名。
const SYSTEMCTL_CALLS: &str = "systemctl-calls";

/// 偽 `systemctl` の答え: 殺した（rc 0）。
const SYSTEMCTL_KILLED: &str = "exit 0";

/// 偽 `systemctl` の答え: unit が無い（実 systemctl と同じ字面の stderr・rc 1）。
const SYSTEMCTL_GONE: &str = "__u=\nfor __a in \"$@\"; do __u=$__a; done\n\
                              printf 'Failed to kill unit %s: Unit %s not loaded.\\n' \"$__u\" \"$__u\" >&2\nexit 1";

/// [`systemd_stub`] の dir に偽 `systemctl` を置く（argv を写してから `answer` を撃つ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn systemctl_stub(state: &Path, answer: &str) {
    use std::os::unix::fs::PermissionsExt;
    let shim = state.join(SYSTEMD_BIN).join("systemctl");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n{answer}\n",
        state.join(SYSTEMCTL_CALLS).display()
    );
    fs::write(&shim, script).expect("stub を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("stub に実行権を付ける");
}

/// `needle` を名に含む scope の unit 名（**ちょうど 1 件**・偽 `systemd-run` が受けた名）。
fn scope_unit(state: &Path, needle: &str) -> String {
    let names = dir_names(&state.join(SCOPE_RECORDS));
    let hits: Vec<&String> = names.iter().filter(|name| name.contains(needle)).collect();
    assert_eq!(hits.len(), 1, "{needle} の unit はちょうど 1 件（母集団 {names:?}）");
    hits.first().map_or_else(String::new, |name| name.trim_end_matches(".args").to_owned())
}

/// 偽 `systemctl` の呼出のうち `unit` を含む行。
fn release_calls(state: &Path, unit: &str) -> Vec<String> {
    fs::read_to_string(state.join(SYSTEMCTL_CALLS))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains(unit))
        .map(str::to_owned)
        .collect()
}

/// 1 行の record が `key` を持つか（値が空の field と不在を弁別する）。
fn row_has(rows: &[Vec<(String, vessel::fleet::json_lite::Value)>], n: usize, key: &str) -> bool {
    rows.get(n.saturating_sub(1)).is_some_and(|row| row.iter().any(|(found, _)| found == key))
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
        vec![format!("--user kill --signal=SIGKILL {unit}.scope")],
        "終端で 1 回だけ、包んだ名の scope を SIGKILL で片付ける"
    );
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
    assert_eq!(release_calls(&state, &unit).len(), 1, "片付けは撃たれている（不在は撃たなかったせいではない）");
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

/// (e) **同じ process が gate を 2 周撃つ**（land の追随 → 再 gate → main 実測・どちらも場所は run id）
/// と、2 周目の unit 名は 1 周目と異なる。偽 `systemd-run` は同名の 2 本目を実 systemd と同じ字面で
/// 断るので、base（名に通し番号が無い）では main 実測の行が起動できず land が落ちる（.208 run 3）。
#[test]
fn pipe_confine_release_regate_in_one_process_uses_distinct_unit_names() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    systemctl_stub(&state, SYSTEMCTL_GONE);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "1 周目の gate: {}", stderr_of(&gated));
    fs::write(repo.join("other.txt"), "other\n").expect("別便の変更を書ける");
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
    assert_eq!(contract.len(), 2, "再 gate と main 実測の 2 周が別名で撃たれた: {names:?}");
    assert_ne!(contract.first(), contract.get(1), "2 周の名は異なる");
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
            vec![format!("--user kill --signal=SIGKILL {unit}.scope")],
            "{stage} の scope も終端で 1 回片付ける"
        );
    }
    assert_eq!(value_of(&verdict_pairs(&state, &id), "scope"), "killed", "lens の片付けは verdict に残る");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS", "判定は不変");
    clean(&[&repo, &state]);
}

/// 便を 1 本 gate まで通し、`path_of` が組んだ PATH の下で書かれた verdict.json を読む
/// （置き場は片付けてから返す）。
fn verdict_under(path_of: impl Fn(&Path) -> String) -> Vec<(String, vessel::fleet::json_lite::Value)> {
    let (repo, state) = repo_with_state();
    let path = path_of(&state);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let pairs = verdict_pairs(&state, &id);
    // marker では測らない: lean な PATH には `touch` が無い（lens の答えは builtin の echo で届く）。
    assert_eq!(value_of(&pairs, "evidence"), "fake", "lens まで届いた周（片付ける lens の起動が在る）");
    clean(&[&repo, &state]);
    pairs
}

/// **verdict の `scope` は閉じた集合の名か、key ごと欠けるかのどちらか**（設計 §4.4 / §5.3・C10）。
///
/// 包めた周（偽 `systemd-run`）は lens の片付けの結果を [`Released::as_str`] の名で書く。包めない
/// 周（PATH に `systemd-run` が無い）は片付ける scope が無い＝key を欠く（`none` 等を書かない）。
/// 道具の有無は 2 面とも fixture で固定する——素の環境で撃つと周ごとに面が入れ替わる（`s2-07l.236`）。
// flip-check: retroactive s2-07l.236
#[test]
fn pipe_gate_verdict_scope_is_a_closed_name_or_absent() {
    use vessel::pipe::confine::Released;
    let names: Vec<&str> = [Released::Gone, Released::Killed, Released::Failed(1), Released::NoTool]
        .into_iter()
        .map(Released::as_str)
        .collect();
    // 包めた面: 片付けの道具の答えを振る（`killed` は [`pipe_gate_records_structured_verdict`] が測る）。
    let no_bus = "echo 'Failed to connect to bus: No medium found' >&2\nexit 1";
    for (answer, want) in [(Some(no_bus), "failed"), (None, "no-tool")] {
        let pairs = verdict_under(|state| {
            let stubbed = systemd_stub(state);
            match answer {
                Some(found) => {
                    systemctl_stub(state, found);
                    stubbed
                }
                None => format!("{}:{}", state.join(SYSTEMD_BIN).display(), lean_path(state)),
            }
        });
        let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["schema", "run", "verdict", "evidence", "verify_red", "diff_bytes", "tree", "scope", "ts"],
            "包めた周は 9 key（{want}）"
        );
        let scope = value_of(&pairs, "scope");
        assert!(names.contains(&scope.as_str()), "閉じた集合の名: {scope} ∉ {names:?}");
        assert_eq!(scope, want, "片付けの道具の答えの名");
        assert_eq!(value_of(&pairs, "verdict"), "PASS", "片付けの結果で判定は変えない");
    }
    // 包めない面: `scope` の key が欠ける（8 key）。
    let pairs = verdict_under(lean_path);
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["schema", "run", "verdict", "evidence", "verify_red", "diff_bytes", "tree", "ts"],
        "包めない周は `scope` を欠く（測れなかった値は書かない）"
    );
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "包めなくても便は流れる");
}

/// host の受付札の置き場（`<state_dir の親>/scribe2-host/slots`・設計 gate-cost.md §3.2）。
///
/// **lib の関数を通さず字面で組む**——同じ関数で置き場を引くと、導き方を変えた実装でも歯が
/// 追随して通る（置き場の字面そのものを pin する）。
fn host_slots(state: &Path) -> PathBuf {
    state.parent().unwrap_or(state).join("scribe2-host").join("slots")
}

/// 存在しない pid（`pid_max` の上限 4194304 を超える）。
const DEAD_PID: u64 = 4_000_000_000;

/// 受付札を 1 枚置く（歯が置く札・中身は 1 行 JSON）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn plant_ticket(state: &Path, pid: u64, jobs: u64) -> PathBuf {
    let dir = host_slots(state);
    fs::create_dir_all(&dir).expect("slot dir を作れる");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX));
    let path = dir.join(format!("{pid}-planted.slot"));
    let body = format!("{{\"schema\":1,\"pid\":{pid},\"run\":\"planted\",\"jobs\":{jobs},\"ts\":{ts}}}\n");
    fs::write(&path, body).expect("札を置ける");
    path
}

/// 受付の歯の宣言: `{jobs}` の無い行 → `{jobs}` の行の順に、**撃たれた側で** slot dir を写す。
///
/// 札は行の終了で消えるので、外から gate の後に見ても「在った」ことは測れない。行の中から
/// dir の中身と札の本文を git の dir へ写す（script の本文は宣言の検査の外）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn commit_slot_vessel(repo: &Path, state: &Path) {
    let slots = host_slots(state);
    let dir = slots.display();
    let seen = "\"$(git rev-parse --absolute-git-dir)\"";
    let plain = format!("ls -A '{dir}' > {seen}/slots-plain 2>/dev/null\nexit 0\n");
    let jobs = format!(
        "printf '%s' \"$1\" > {seen}/jobs-seen\nls -A '{dir}' > {seen}/slots-during 2>/dev/null\n\
         cat '{dir}'/*.slot > {seen}/slots-body 2>/dev/null\nexit 0\n"
    );
    fs::write(repo.join("verify-slot-plain.sh"), plain).expect("script を書ける");
    fs::write(repo.join("verify-slot.sh"), jobs).expect("script を書ける");
    git(repo, &["add", "verify-slot-plain.sh", "verify-slot.sh"]);
    commit_vessel(repo, VESSEL_ALLOWED, r#"["sh verify-slot-plain.sh", "sh verify-slot.sh {jobs}"]"#);
}

/// 受付の歯の 1 便（stub の `systemd-run` で包める host を作り、`--rules` の fixture で gate）。
fn slot_gate(repo: &Path, state: &Path) -> (String, Output) {
    commit_slot_vessel(repo, state);
    let path = systemd_stub(state);
    let marker = state.join("lens-ran");
    confined_run(repo, state, &path, &fake_lens(&marker, &lens_verdict("PASS")))
}

/// `slot=` を持つ record（**ちょうど 1 件**・母集団を確かめてから読む）。
fn slot_row(
    rows: &[Vec<(String, vessel::fleet::json_lite::Value)>],
) -> Vec<(String, vessel::fleet::json_lite::Value)> {
    let hits: Vec<&Vec<(String, vessel::fleet::json_lite::Value)>> =
        rows.iter().filter(|row| row.iter().any(|(key, _)| key == "slot")).collect();
    assert_eq!(hits.len(), 1, "受付を通った record はちょうど 1 件: {rows:?}");
    hits.first().map(|row| (*row).clone()).unwrap_or_default()
}

/// slot dir の `.slot` の名（dir が無ければ空）。
fn slot_names(state: &Path) -> Vec<String> {
    fs::read_dir(host_slots(state))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".slot"))
                .collect()
        })
        .unwrap_or_default()
}

/// record の `slot` が回収 1 枚を含む（`reclaimed:1` か `degraded,reclaimed:1`）。
///
/// 枠を配れたかは host の空き memory に依る（MemAvailable が `reserve + job` 未満の host では
/// 縮退する）ので pin しない——e2e は実 host の meminfo に依らない（設計 §7）。
fn assert_reclaimed_one(slot: &str, why: &str) {
    assert!(
        slot == "reclaimed:1" || slot == "degraded,reclaimed:1",
        "{why}: slot は回収 1 枚を含む: {slot}"
    );
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

/// 待ちが解ける歯の待ちの上限（秒）。歯が札を消すまでの時間より十分に長く置く。
const SLOT_WAIT_LONG_S: u64 = 60;

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
    let slots = SlotFixture { job_mb: 1, reserve_mb: 0, wait_s: SLOT_WAIT_LONG_S };
    let rules = write_rules_full(&state, "rules-slot-long.toml", (1, 1_000_000), FOLLOW_RETRIES, slots);
    let contract = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &contract);
    let spawned = run_pipe_with_path(
        &path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT],
    );
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));

    let first_dead = plant_ticket(&state, DEAD_PID, 1);
    let live = plant_ticket(&state, u64::from(std::process::id()), 1_000_000_000_000);
    let mut child = Command::new(bin())
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

/// **`{jobs}` の行だけが札を置き、終了で消し、置換後の cmd に実効 jobs が載る**（歯 (4) (5) (6)）。
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
    // (4) `{jobs}` の無い行の間は札が無い（先に撃つ行・札を置く前）。
    assert!(!read("slots-plain").contains(".slot"), "(4) {{jobs}} の無い行は札を作らない: {}", read("slots-plain"));
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

/// 検出線の stub の行（`{base}` を印に埋める＝置換されたことを撃たれた側で読める）。
const DETECTION_COUNT: &str = r#"["sh verify-count.sh detection-{base}"]"#;

/// `detection-verify` を持つ宣言を commit する（共通 verify も stub・`allowed-commands` は toy のまま）。
fn commit_detection_vessel(repo: &Path, detection: &str) {
    write_vessel(repo, VESSEL_ALLOWED, r#"["sh verify-count.sh common"]"#);
    let path = repo.join(".vessel.toml");
    let body = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, format!("{body}detection-verify = {detection}\n")).ok();
    git(repo, &["add", "-f", ".vessel.toml"]);
    git(repo, &["commit", "-q", "-m", "vessel-detection"]);
}

/// 検出線を持つ toy repo と、契約 verify も stub にした契約 file。
fn detection_repo(detection: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    commit_detection_vessel(&repo, detection);
    let contract = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-count.sh contract"]"#]);
    (repo, state, contract)
}

/// 呼出回数 file の行（撃たれた順）。
fn detection_calls(repo: &Path) -> Vec<String> {
    fs::read_to_string(repo.join(".git").join("detection-calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// land の main 実測の record を全部読む。
fn main_rows(state: &Path, id: &str) -> Vec<Vec<(String, vessel::fleet::json_lite::Value)>> {
    fs::read_to_string(state.join("pipe").join(id).join("verify-main.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| vessel::fleet::json_lite::parse_object(line.trim()).ok())
        .collect()
}

/// record 列の `kind` の並び。
fn kinds(rows: &[Vec<(String, vessel::fleet::json_lite::Value)>]) -> Vec<String> {
    rows.iter().map(|row| value_of(row, "kind")).collect()
}

/// [`detection_land`] の結果。
struct DetectionLand {
    /// **land が足した**呼出行（撃たれた順）。
    added: Vec<String>,
    /// main 実測の record。
    rows: Vec<Vec<(String, vessel::fleet::json_lite::Value)>>,
    /// land の出力。
    out: Output,
    /// 対象 repo。
    repo: PathBuf,
    /// 置き場。
    state: PathBuf,
    /// 便 id。
    id: String,
}

/// PASS まで通し、`verdict.json` を `edit(本文, tree)` で差し替えてから land する。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn detection_land(edit: fn(&str, &str) -> String) -> DetectionLand {
    let (repo, state, contract) = detection_repo(DETECTION_COUNT);
    let id = gated_pass(&repo, &state, &contract, &state.join("lens-ran"));
    let before = detection_calls(&repo).len();
    let verdict = state.join("pipe").join(&id).join("verdict.json");
    let text = fs::read_to_string(&verdict).expect("verdict.json を読める");
    let tree = value_of(&verdict_pairs(&state, &id), "tree");
    assert!(!tree.is_empty(), "差し替える前の verdict は tree を持つ: {text}");
    fs::write(&verdict, edit(&text, &tree)).expect("verdict.json を差し替えられる");
    let out = land_once(&repo, &state, &id);
    let added = detection_calls(&repo).split_off(before);
    let rows = main_rows(&state, &id);
    DetectionLand { added, rows, out, repo, state, id }
}

/// (1) `detection-verify` が読めて、gate が **① write-set → ② common → ③ detection → ④ 契約** の順で撃つ。
#[test]
fn pipe_detection_verify_fires_third_in_gate() {
    let (repo, state, contract) = detection_repo(DETECTION_COUNT);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let id = gated_pass(&repo, &state, &contract, &state.join("lens-ran"));
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "detection", "contract"], "段の順序と kind: {rows:?}");
    assert_eq!(row_value(&rows, 3, "cmd"), format!("sh verify-count.sh detection-{base}"), "③ の穴は置換される");
    assert_eq!(
        detection_calls(&repo),
        ["common".to_owned(), format!("detection-{base}"), "contract".to_owned()],
        "撃たれた側の順序も ②③④"
    );
    clean(&[&repo, &state]);
}

/// (2) `verdict.json` の `tree` は **gate を撃った HEAD の木**（base の木ではない）。
#[test]
fn pipe_detection_verdict_carries_tree_of_gated_head() {
    let (repo, state, contract) = detection_repo(DETECTION_COUNT);
    let id = gated_pass(&repo, &state, &contract, &state.join("lens-ran"));
    let tree = value_of(&verdict_pairs(&state, &id), "tree");
    assert!(!tree.is_empty(), "tree が在る");
    assert_eq!(tree, git(&worktree_of(&repo, &id), &["rev-parse", "HEAD^{tree}"]), "HEAD の木と一致");
    assert_ne!(tree, git(&repo, &["rev-parse", "HEAD^{tree}"]), "base の木ではない（runner が commit した後の木）");
    clean(&[&repo, &state]);
}

/// (3) 木が gate と同じ main 実測は **③ だけを撃たず** `skipped=detection tree=<sha>` を記す（②④は撃つ）。
#[test]
fn pipe_detection_land_skips_detection_when_tree_matches() {
    let landed = detection_land(|text, _| text.to_owned());
    let (rows, out) = (&landed.rows, &landed.out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(out));
    assert_eq!(landed.added, ["common", "contract"], "main 実測は ②④ だけを撃つ");
    assert_eq!(kinds(rows), ["write-set", "common", "detection", "contract"], "省いた段も位置に record が在る: {rows:?}");
    assert_eq!(row_value(rows, 3, "skipped"), "detection", "skipped=detection");
    assert_eq!(row_value(rows, 3, "tree"), git(&landed.repo, &["rev-parse", "refs/heads/main^{tree}"]), "tree=<land した木>");
    assert_eq!(row_value(rows, 4, "rc"), "0", "④ は撃って緑");
    clean(&[&landed.repo, &landed.state]);
}

/// ③ を撃った main 実測の共通 assert（(4) / (5)）。
fn assert_detection_fired(landed: &DetectionLand) {
    let (added, rows) = (&landed.added, &landed.rows);
    assert_eq!(landed.out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&landed.out));
    assert_eq!(added.len(), 3, "②③④ を全部撃つ: {added:?}");
    assert!(added.get(1).is_some_and(|call| call.starts_with("detection-")), "③ が呼ばれる: {added:?}");
    assert_eq!(kinds(rows), ["write-set", "common", "detection", "contract"], "③ は撃った record: {rows:?}");
    assert!(rows.iter().all(|row| value_of(row, "skipped").is_empty()), "省いた record は無い: {rows:?}");
}

/// (4) verdict の `tree` が land した木と違えば **③ も撃つ**。
#[test]
fn pipe_detection_land_fires_detection_when_tree_differs() {
    let landed = detection_land(|text, tree| {
        text.replace(&format!("\"tree\":\"{tree}\""), "\"tree\":\"0000000000000000000000000000000000000000\"")
    });
    let verdict = verdict_pairs(&landed.state, &landed.id);
    assert_eq!(value_of(&verdict, "tree"), "0".repeat(40), "fixture は壊した tree");
    assert_detection_fired(&landed);
    clean(&[&landed.repo, &landed.state]);
}

/// (5) `tree` の無い verdict（旧 gate の形）でも **③ を撃つ**。
#[test]
fn pipe_detection_land_fires_detection_when_verdict_has_no_tree() {
    let landed = detection_land(|text, tree| text.replace(&format!(",\"tree\":\"{tree}\""), ""));
    let verdict = verdict_pairs(&landed.state, &landed.id);
    assert!(verdict.iter().all(|(key, _)| key != "tree"), "fixture は tree の無い旧形: {verdict:?}");
    assert_eq!(value_of(&verdict, "verdict"), "PASS", "fixture の verdict は読める形のまま");
    assert_detection_fired(&landed);
    clean(&[&landed.repo, &landed.state]);
}

/// (6) 検出線の rc≠0 は **従来どおり gate FAIL**（測れなかったを通ったに化けさせない・lens を呼ばない）。
#[test]
fn pipe_detection_red_line_fails_gate() {
    let (repo, state, contract) = detection_repo(r#"["sh verify-red.sh"]"#);
    let id = implemented(&repo, &state, &contract);
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=FAIL"), "{}", stdout_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "kind"), "detection", "③ の record");
    assert_eq!(row_value(&rows, 3, "rc"), "1", "③ が赤");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "1", "赤は ③ の 1 本");
    assert!(!marker.exists(), "赤い周は lens を起動しない");
    clean(&[&repo, &state]);
}

/// 検出線の oom の歯の結果。
struct DetectionOom {
    /// gate の出力。
    gated: Output,
    /// 対象 repo。
    repo: PathBuf,
    /// 置き場。
    state: PathBuf,
    /// 便 id。
    id: String,
    /// lens を起動したかの印。
    marker: PathBuf,
}

/// 撃った sh（包みの中の `sh -c`）ごと signal で殺す script（終端行を出せない周の形）。
const VERIFY_SIGKILL: &str = "verify-sigkill.sh";

/// 共通 verify と検出線を宣言した便を実装済みにし、**偽 `systemd-run` の PATH で** gate を撃つ
/// （包めた周＝`oom_kill` の終端行が読まれる周・設計 gate-cost.md §4.2）。
fn detection_oom_gate(common: &str, detection: &str) -> DetectionOom {
    let (repo, state) = repo_with_state();
    fs::write(repo.join(VERIFY_SIGKILL), "kill -9 $PPID\nexit 0\n").ok();
    write_vessel(&repo, VESSEL_ALLOWED, common);
    let path = repo.join(".vessel.toml");
    let body = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, format!("{body}detection-verify = {detection}\n")).ok();
    git(&repo, &["add", "-f", ".vessel.toml", VERIFY_SIGKILL]);
    git(&repo, &["commit", "-q", "-m", "vessel-detection-oom"]);
    let contract = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-ok.sh"]"#]);
    let id = implemented(&repo, &state, &contract);
    let stub = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let gated = run_pipe_with_path(
        &stub,
        &["gate", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(),
          "--lens", &fake_lens(&marker, &lens_verdict("PASS"))],
    );
    DetectionOom { gated, repo, state, id, marker }
}

/// (a) 検出線の行の `oom_kill`（rc 0 で完走）は **測れた周**——verdict は lens の verdict で、
/// record の `reason=oom-kill` は残る（`s2-07l.228`・設計 gate-cost.md §4.2）。
#[test]
fn pipe_detection_oom_kill_in_detection_line_is_measured() {
    let run = detection_oom_gate(r#"["sh verify-ok.sh"]"#, r#"["sh verify-oom.sh"]"#);
    let (gated, state, id) = (&run.gated, &run.state, &run.id);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {} / {}", stdout_of(gated), stderr_of(gated));
    assert!(stdout_of(gated).contains("verdict=PASS"), "lens の verdict が届く: {}", stdout_of(gated));
    assert!(!stdout_of(gated).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(gated));
    let rows = verify_rows(state, id);
    assert_eq!(kinds(&rows), ["write-set", "common", "detection", "contract"], "段の並び: {rows:?}");
    assert_eq!(row_value(&rows, 3, "confined"), "true", "包めた周である");
    assert_eq!(row_value(&rows, 3, "rc"), "0", "道具は完走した");
    assert_eq!(row_value(&rows, 3, "reason"), "oom-kill", "record の reason は現物のまま残す");
    assert!(
        rows.iter().filter(|row| value_of(row, "reason") == "oom-kill").count() == 1,
        "oom-kill は検出線の 1 行だけ（fixture 衝突なし）: {rows:?}"
    );
    assert_eq!(value_of(&verdict_pairs(state, id), "verdict"), "PASS", "verdict.json も PASS");
    assert!(run.marker.exists(), "lens は起動される");
    clean(&[&run.repo, &run.state]);
}

/// (b) 共通 verify の行の `oom_kill` は **従来どおり INCONCLUSIVE**（退行の柵）。
#[test]
fn pipe_detection_oom_kill_in_common_line_stays_inconclusive() {
    let run = detection_oom_gate(r#"["sh verify-oom.sh"]"#, r#"["sh verify-ok.sh"]"#);
    let (gated, state, id) = (&run.gated, &run.state, &run.id);
    assert_eq!(gated.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stdout_of(gated));
    assert!(stdout_of(gated).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(gated));
    let rows = verify_rows(state, id);
    assert_eq!(row_value(&rows, 2, "kind"), "common", "② の record");
    assert_eq!(row_value(&rows, 2, "confined"), "true", "包めた周である");
    assert_eq!(row_value(&rows, 2, "reason"), "oom-kill", "② が箱の中で殺された");
    assert_eq!(row_value(&rows, 3, "reason"), "", "③ は殺されていない");
    assert!(
        value_of(&verdict_pairs(state, id), "evidence").contains("oom-kill"),
        "理由が verdict に残る: {:?}",
        verdict_pairs(state, id)
    );
    assert!(!run.marker.exists(), "測れなかった周は lens を起動しない");
    clean(&[&run.repo, &run.state]);
}

/// (c) 検出線の行が **包みごと signal で死んだ**周（終端行なし・rc 255）は従来どおり INCONCLUSIVE。
#[test]
fn pipe_detection_oom_signal_death_in_detection_line_stays_inconclusive() {
    let run = detection_oom_gate(r#"["sh verify-ok.sh"]"#, r#"["sh verify-sigkill.sh"]"#);
    let (gated, state, id) = (&run.gated, &run.state, &run.id);
    assert_eq!(gated.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stdout_of(gated));
    assert!(stdout_of(gated).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(gated));
    let rows = verify_rows(state, id);
    assert_eq!(row_value(&rows, 3, "kind"), "detection", "③ の record");
    assert_eq!(row_value(&rows, 3, "confined"), "true", "包めた周である");
    assert_eq!(row_value(&rows, 3, "rc"), "255", "rc の無い周は 255 に畳む");
    assert_eq!(row_value(&rows, 3, "reason"), "signal", "終端行を出せずに死んだ");
    assert_eq!(value_of(&verdict_pairs(state, id), "verify_red"), "0", "赤には数えない");
    assert!(
        value_of(&verdict_pairs(state, id), "evidence").contains("signal"),
        "理由が verdict に残る: {:?}",
        verdict_pairs(state, id)
    );
    assert!(!run.marker.exists(), "測れなかった周は lens を起動しない");
    clean(&[&run.repo, &run.state]);
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
