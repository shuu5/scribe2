// flip-check: moved s2-07l.264
//! gate の歯: `pipe_gate_` / `pipe_detection_`（検出線）/ `pipe_confine_`（封じ込め）/ `pipe_slots_`（受付札）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く（歯の本文は移しただけ・`s2-07l.264`）。

use super::*;
use vessel::pipe::run_dir;

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

/// 接頭辞付きの write-set（`+src/new.rs` 新規 / `-src/old.rs` 縮む面）の便を Implemented まで進める。
/// runner は `src/new.rs` を足し `src/old.rs` を縮め、`extra` の command も撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn prefixed_run(extra: &str) -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    // `-` の先は base に在る file（受付が断る）。
    fs::write(repo.join("src").join("old.rs"), "// old\n// shrink me\n").expect("縮む file を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "prefixed-base"]);
    let design = write_contract(&repo, &["write-set"], &[r#"write-set = ["+src/new.rs", "-src/old.rs"]"#]);
    let id = intake(&repo, &state, &design);
    let runner = format!(
        "echo new > src/new.rs && echo '// old' > src/old.rs {extra} && git add -A && git commit -q -m runner"
    );
    let out = spawn_with(&repo, &state, &id, &runner);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    (repo, state, id)
}

/// **接頭辞は受付の宣言であって path の一部ではない**（設計 contract-source.md §3・`s2-07l.291`）: 段①は
/// 項目の `+` / `-` を剥がして diff の素の path と照合する。受付と guard を通った契約が gate で落ちない。
#[test]
fn pipe_gate_write_set_prefixed_items_match_plain_diff_paths() {
    let (repo, state, id) = prefixed_run("");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "接頭辞の項目に収まる便は通る: {}", stderr_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 1, "cmd"), "write-set", "段①は先頭（母集団 {} record）", rows.len());
    assert_eq!(row_value(&rows, 1, "rc"), "0", "`src/new.rs` / `src/old.rs` は write-set の内");
    clean(&[&repo, &state]);
}

/// 対: 同じ契約で write-set の外（`src/other.rs`）も足した便は段①が赤く、その path を名指す
/// （接頭辞の剥がしが照合を緩めていないことの証拠・`s2-07l.291`）。
#[test]
fn pipe_gate_write_set_prefixed_items_still_name_outside_paths() {
    let (repo, state, id) = prefixed_run("&& echo z > src/other.rs");
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "write-set の外は FAIL: {}", stderr_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 1, "cmd"), "write-set", "段①は先頭（母集団 {} record）", rows.len());
    assert_eq!(row_value(&rows, 1, "rc"), "1", "外れた便は段①が赤い");
    assert!(!marker.exists(), "段①が赤い周は lens を起動しない");
    let tail = fs::read_to_string(state.join("pipe").join(&id).join("verify.stderr.log"))
        .expect("verify.stderr.log を読める");
    assert!(tail.contains("src/other.rs"), "外れた path を名指す: {tail}");
    assert!(!tail.contains("src/new.rs"), "`+` の項目の path は列挙しない: {tail}");
    assert!(!tail.contains("src/old.rs"), "`-` の項目の path は列挙しない: {tail}");
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
    // 規則は lens を 1 本要る（gate.lens_count = 1）が `--lens` が無い。審査が残した写し（`lens.toml`・§26）も外す
    // ＝写しも flag も無い世界。
    fs::remove_file(run_dir(&state, &id).join("lens.toml")).expect("審査の写しを外せる");
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
        vec![
            "schema", "run", "verdict", "evidence", "verify_red", "diff_bytes", "tree", "scope",
            // 判定に届いた周は findings の件数と母集団も同じ record に載る（`s2-07l.188`）。
            "findings", "population", "ts",
        ],
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
    let forced = bin_cmd()
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

// ── lens の findings の閉じた category と母集団（`s2-07l.188`・設計 §6 / §17）──────────

/// 2 key を振れる偽 lens の本文（`extra` は `verdict` / `evidence` の後ろに足す字面・key の
/// **無い**形も作れる＝必須 key の歯は「書かない」を入力にする）。
fn findings_body(extra: &str) -> String {
    format!("{{\"verdict\":\"PASS\",\"evidence\":\"fake\"{extra}}}")
}

/// 8 category を 0 件で並べた字面（宣言順）。
///
/// **歯の側で字面を持つ**（共有 helper の [`FAKE_FINDINGS`] を引かない）——引くと base の木では
/// この file が compile できず、機能の不在が rc でなく compile error で「赤い」ことになる。
const ZERO_FINDINGS: &str = "contract-fit:0,teeth-nonvacuous:0,constitution:0,delete:0,stdlib:0,native:0,yagni:0,shrink:0";

/// 0 でない母集団（lens が読んだ周・[`ZERO_FINDINGS`] と同じ理由で歯の側に持つ）。
const READ_POPULATION: &str = "files:1,lines:1";

/// **2 key を持たない lens の verdict は INCONCLUSIVE**（`s2-07l.188`・C10・C11.2）。
///
/// 件数と母集団の無い判定は「見て 0 件だった」と「見ていない」を弁別できない。**PASS を
/// 名乗っていても倒す**（AC3「偽の PASS 0 件」）。欠けた key は理由が名指す。
#[test]
fn pipe_gate_findings_missing_population_is_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    // 母集団だけが無い（findings の 8 category は在る）。
    let body = findings_body(&format!(",\"findings\":\"{ZERO_FINDINGS}\""));
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &body)));
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "母集団の無い PASS は rc 3: {}", stdout_of(&out));
    assert!(marker.exists(), "lens 自体は呼んでいる（判定だけが届かない）");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "PASS を名乗っても通さない");
    assert!(value_of(&pairs, "evidence").contains("population が無い"), "欠けた key を名指す: {pairs:?}");
    assert_eq!(value_of(&pairs, "findings"), "", "測れていない周は field を書かない");

    // 対（findings が無い側）: INCONCLUSIVE は終端でないので同じ便を撃ち直せる。
    let body = findings_body(&format!(",\"population\":\"{READ_POPULATION}\""));
    let again = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &body)));
    assert_eq!(again.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "findings の無い PASS も rc 3");
    let pairs = verdict_pairs(&state, &id);
    assert!(value_of(&pairs, "evidence").contains("findings が無い"), "欠けた key を名指す: {pairs:?}");
    assert!(!value_of(&pairs, "evidence").contains("population が無い"), "無いのは findings の側だけ: {pairs:?}");

    // **弁別**: 2 key が揃えば同じ便が PASS で通る（歯が「常に INCONCLUSIVE」を測っていない）。
    let ok = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(ok.status.code(), Some(i32::from(RC_OK)), "2 key が揃えば通る: {}", stderr_of(&ok));
    clean(&[&repo, &state]);
}

/// **8 category の件数と母集団が判定の record に載る**（0 件も 0 と書く・`s2-07l.188`）。
///
/// lens の stdout の verdict record（`parse_lens` の入力）が**宣言順でない並び**で出しても、
/// `verdict.json` の字面は宣言順 1 つに正規化される（集計の順は器の表が持つ・C2）。`Gated` の
/// detail は verdict だけで**不変**である。
#[test]
fn pipe_gate_findings_counts_are_recorded_per_category() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let shuffled = "shrink:5,constitution:2,contract-fit:1,yagni:4,stdlib:3,delete:0,native:0,teeth-nonvacuous:0";
    let body = findings_body(&format!(",\"findings\":\"{shuffled}\",\"population\":\"files:7,lines:42\""));
    // lens の **stdout** の record を歯が読めるように写してから、同じ 1 行を gate へ流す。
    let record = state.join("lens-stdout.json");
    let lens = format!(
        "cat >/dev/null; printf '%s\\n' '{body}' > '{}'; cat '{}'",
        record.display(),
        record.display()
    );
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "2 key が揃えば通る: {}", stderr_of(&out));
    let written = fs::read_to_string(&record).expect("lens の stdout を読める");
    assert!(written.contains(shuffled), "lens の record が 8 category の件数を持つ: {written}");
    assert!(written.contains("\"population\":\"files:7,lines:42\""), "母集団も持つ: {written}");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(
        value_of(&pairs, "findings"),
        "contract-fit:1,teeth-nonvacuous:0,constitution:2,delete:0,stdlib:3,native:0,yagni:4,shrink:5",
        "8 category を宣言順で（0 件も 0 と）書く: {pairs:?}"
    );
    assert_eq!(value_of(&pairs, "population"), "files:7,lines:42", "母集団も同じ record に載る");
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "3 値は動かない");
    let seen = trail(&state, &id);
    assert!(
        seen.contains(&(EventKind::RunStage, Some(Stage::Gated), Some("verdict:PASS".to_owned()))),
        "`Gated` の detail は verdict だけ（不変）: {seen:?}"
    );
    clean(&[&repo, &state]);
}

/// **母集団 0 は INCONCLUSIVE**（監査 2026-09-12 塊 21 の `.175` の指摘そのもの・C10）。
///
/// 「読んでいない」を「穴が無い」と読むと、lens を呼んだ事実だけで PASS が出る。file 数と
/// 行数の**どちらが 0 でも**倒す。
#[test]
fn pipe_gate_findings_zero_population_is_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    for empty in ["files:0,lines:42", "files:7,lines:0"] {
        let body = findings_body(&format!(",\"findings\":\"{ZERO_FINDINGS}\",\"population\":\"{empty}\""));
        let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &body)));
        assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "母集団 {empty} は rc 3: {}", stdout_of(&out));
        let pairs = verdict_pairs(&state, &id);
        assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "母集団 {empty} を通さない");
        assert!(value_of(&pairs, "evidence").contains("population が 0"), "理由が残る（{empty}）: {pairs:?}");
        assert_eq!(value_of(&pairs, "population"), "", "測れていない周は field を書かない");
    }
    assert!(marker.exists(), "lens は呼んでいる（判定だけが届かない）");
    // **弁別**: 母集団が 1 以上なら同じ便が PASS で通る。
    let ok = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(ok.status.code(), Some(i32::from(RC_OK)), "母集団が 0 でなければ通る: {}", stderr_of(&ok));
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

/// run dir の `rulings.txt`（gate が書く裁定の写し・`s2-07l.309`）。
fn rulings_path(state: &Path, id: &str) -> PathBuf {
    state.join("pipe").join(id).join("rulings.txt")
}

/// 契約の写しの隣の `rulings.txt` を **lens が起きた時点で** `seen` へ写す fake lens（無ければ写さない）。
fn rulings_copying_lens(seen: &Path) -> String {
    format!(
        "cat >/dev/null; r=\"$(dirname '{{contract}}')/rulings.txt\"; if [ -e \"$r\" ]; then cp \"$r\" '{}'; fi; echo '{}'",
        seen.display(),
        lens_verdict("PASS")
    )
}

/// 2 つ目の質問 record（`about` 無し・1 つ目と字面が違う）。
const SECOND_QUESTION: &str = "write-set の外の file を触ってよいか";

/// 2 つ目の質問で止まる fake runner（`about` を持たない record・commit は作らない）。
fn second_question_runner() -> String {
    format!("printf '%s\\n' '{{\"question\":\"{SECOND_QUESTION}\"}}'; exit 76")
}

/// 便を QUESTION ×2（各 answer + resume）で運び、3 周目で実装して Implemented にする。回答の逐語を返す。
fn implemented_after_two_questions(repo: &Path, state: &Path) -> (String, [&'static str; 2]) {
    let answers = ["verify は 1 行目だけを撃つ", "触ってよい（裁定 (b)）"];
    let id = questioned(repo, state);
    // 1 周目の resume は 2 つ目の質問で止まり（rc 3）、2 周目の resume は実装して Implemented（rc 0）。
    let turns = [(second_question_runner(), RC_BLOCKED), (TOY_COMMIT.to_owned(), RC_OK)];
    for (answer, (runner, rc)) in answers.iter().zip(&turns) {
        let answered = run_pipe(&["answer", "--run", &id, "--words", answer, "--state-dir", &state.display().to_string()]);
        assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "回答は rc 0: {}", stderr_of(&answered));
        let out = run_pipe(&[
            "resume", "--run", &id, "--repo", &repo.display().to_string(),
            "--state-dir", &state.display().to_string(), "--runner", runner,
        ]);
        assert_eq!(out.status.code(), Some(i32::from(*rc)), "resume の rc: {}", stderr_of(&out));
    }
    assert_eq!(stage_count(state, &id, Stage::Questioned), 2, "QUESTION を 2 回通った: {:?}", stages(state, &id));
    assert!(show_line(repo, state, &id).contains("stage=Implemented"), "{}", show_line(repo, state, &id));
    (id, answers)
}

/// (a) QUESTION ×2（answer 付き）の便を gate に通すと run dir に `rulings.txt` が在り、対 2 つが**発生順**に
/// `question:` / `about:` / `answer:` の 3 行（`about` の無い対は `-`・対の間は空行）で逐語に載り、lens が起きた
/// 時点で契約の写しの隣に在る（`s2-07l.309`・設計 pipeline-question.md）。base は file を書かないので RED。
#[test]
fn pipe_gate_rulings_file_lists_every_question_answer_pair_in_order() {
    let (repo, state) = repo_with_state();
    let (id, [first, second]) = implemented_after_two_questions(&repo, &state);
    let seen = state.join("rulings-at-lens");
    let out = gate_once(&repo, &state, &id, Some(&rulings_copying_lens(&seen)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let kept = fs::read_to_string(rulings_path(&state, &id)).expect("run dir に rulings.txt が在る");
    let want = format!(
        "question: verify 行が矛盾する\nabout: verify\nanswer: {first}\n\nquestion: {SECOND_QUESTION}\nabout: -\nanswer: {second}\n"
    );
    assert_eq!(kept, want, "対 2 つが発生順・3 行の形・逐語");
    let at_lens = fs::read_to_string(&seen).expect("lens が起きた時点で契約の隣に rulings.txt が在る");
    assert_eq!(at_lens, kept, "lens が読んだ写しは run dir のものと同じ");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS", "従来どおり lens の verdict");
    clean(&[&repo, &state]);
}

/// (b) 質問 0 の便は `rulings.txt` を書かない（無いことが「裁定なし」・空の file を書かない・(a) の極性の対）。
#[test]
fn pipe_gate_rulings_file_is_absent_without_questions() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let seen = state.join("rulings-at-lens");
    let out = gate_once(&repo, &state, &id, Some(&rulings_copying_lens(&seen)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    assert!(state.join("pipe").join(&id).join("contract.toml").exists(), "run dir は在る（不在は dir の不在ではない）");
    assert!(!rulings_path(&state, &id).exists(), "質問の無い便に rulings.txt を書かない");
    assert!(!seen.exists(), "lens が起きた時点でも契約の隣に無い");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS", "従来どおり lens の verdict");
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
    // 審査を通っただけ（Reviewed）の便に gate は掛からない。**段違いは何もせず rc 1**（Failed で終端させない）。
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "段違いは rc 1");
    assert!(stderr_of(&out).contains("段は Reviewed である"), "理由: {}", stderr_of(&out));
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
    // 道具が足りない周（`--lens` を渡し忘れた便・審査の写しも無い）。INCONCLUSIVE は「測れなかった」で
    // あって「落ちた」ではないので、**ここで終端しない**。
    fs::remove_file(run_dir(&state, &id).join("lens.toml")).expect("審査の写しを外せる");
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
    fs::remove_file(run_dir(&state, &id).join("lens.toml")).expect("審査の写しを外せる（写しも flag も無い世界）");
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
    fs::remove_file(run_dir(&state, &id).join("lens.toml")).expect("審査の写しを外せる（写しも flag も無い世界）");
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

/// PATH の先頭に置く偽 `systemd-run`（返すのは PATH の値・**明示の口**なので本行は口を変えない）。
///
/// 本体は `crate::write_systemd_run_stub` の 1 つの生成関数から出る（設計 gate-cost.md §30 約束 3）——
/// argv を写してから `--` の後ろを exec し、記録は **`<unit>.args` の 1 起動 1 file**、同じ名の 2 本目は
/// 実 systemd と同じ字面で断る。記録の dir 名（[`SCOPE_RECORDS`]）と [`scope_record`] の読みは不変で、
/// 既存の歯の母集団は動かない。
// flip-check: retroactive s2-07l.504
fn systemd_stub(state: &Path) -> String {
    let bin_dir = state.join(SYSTEMD_BIN);
    crate::write_systemd_run_stub(&bin_dir, &state.join(SCOPE_RECORDS));
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
    let design = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &design);
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
            vec![
                "schema", "run", "verdict", "evidence", "verify_red", "diff_bytes", "tree", "scope",
                "findings", "population", "ts",
            ],
            "包めた周は 11 key（{want}）"
        );
        let scope = value_of(&pairs, "scope");
        assert!(names.contains(&scope.as_str()), "閉じた集合の名: {scope} ∉ {names:?}");
        assert_eq!(scope, want, "片付けの道具の答えの名");
        assert_eq!(value_of(&pairs, "verdict"), "PASS", "片付けの結果で判定は変えない");
    }
    // 包めない面: `scope` の key が欠ける（10 key）。
    let pairs = verdict_under(lean_path);
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "schema", "run", "verdict", "evidence", "verify_red", "diff_bytes", "tree",
            "findings", "population", "ts",
        ],
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
fn commit_slot_vessel(repo: &Path, state: &Path) {
    commit_slot_vessel_line(repo, state, SLOT_JOBS_LINE);
}

/// 受付の歯の `{jobs}` の行（穴は 2 つ・`{threads}` を持たない consumer の形）。
const SLOT_JOBS_LINE: &str = "sh verify-slot.sh {jobs}";

/// 受付の歯の `{jobs}` と `{threads}` の行（3 つ目の穴を持つ検出線と同じ形・設計 gate-cost.md §31 約束 6）。
const SLOT_THREADS_LINE: &str = "sh verify-slot.sh {jobs} {threads}";

/// [`commit_slot_vessel`] の `{jobs}` の行を差し替える形。撃たれた側は `$1`（jobs）と `$2`（threads・
/// 行に無ければ空）を別の file へ写す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn commit_slot_vessel_line(repo: &Path, state: &Path, jobs_line: &str) {
    let slots = host_slots(state);
    let dir = slots.display();
    let seen = "\"$(git rev-parse --absolute-git-dir)\"";
    let plain = format!("ls -A '{dir}' > {seen}/slots-plain 2>/dev/null\nexit 0\n");
    let jobs = format!(
        "printf '%s' \"$1\" > {seen}/jobs-seen\nprintf '%s' \"$2\" > {seen}/threads-seen\n\
         ls -A '{dir}' > {seen}/slots-during 2>/dev/null\n\
         cat '{dir}'/*.slot > {seen}/slots-body 2>/dev/null\nexit 0\n"
    );
    fs::write(repo.join("verify-slot-plain.sh"), plain).expect("script を書ける");
    fs::write(repo.join("verify-slot.sh"), jobs).expect("script を書ける");
    git(repo, &["add", "verify-slot-plain.sh", "verify-slot.sh"]);
    commit_vessel(repo, VESSEL_ALLOWED, &format!(r#"["sh verify-slot-plain.sh", "{jobs_line}"]"#));
}

/// 受付の歯の 1 便（stub の `systemd-run` で包める host を作り、`--rules` の fixture で gate）。
fn slot_gate(repo: &Path, state: &Path) -> (String, Output) {
    slot_gate_line(repo, state, SLOT_JOBS_LINE)
}

/// [`slot_gate`] の `{jobs}` の行を差し替える形。
fn slot_gate_line(repo: &Path, state: &Path, jobs_line: &str) -> (String, Output) {
    commit_slot_vessel_line(repo, state, jobs_line);
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

/// この歯の host の core 数（gate の binary が同じ host で測る値と同じ口・読めない周は `None`）。
fn host_cores() -> Option<u64> {
    std::thread::available_parallelism()
        .ok()
        .and_then(|found| u64::try_from(found.get()).ok())
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

/// 検出線の stub の行（`{base}` を印に埋める＝置換されたことを撃たれた側で読める）。
pub(super) const DETECTION_COUNT: &str = r#"["sh verify-count.sh detection-{base}"]"#;

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
pub(super) fn detection_repo(detection: &str) -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    commit_detection_vessel(&repo, detection);
    let design = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-count.sh contract"]"#]);
    (repo, state, design)
}

/// 呼出回数 file の行（撃たれた順）。
pub(super) fn detection_calls(repo: &Path) -> Vec<String> {
    fs::read_to_string(repo.join(".git").join("detection-calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// land の main 実測の record を全部読む。
pub(super) fn main_rows(state: &Path, id: &str) -> Vec<Vec<(String, vessel::fleet::json_lite::Value)>> {
    fs::read_to_string(state.join("pipe").join(id).join("verify-main.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| vessel::fleet::json_lite::parse_object(line.trim()).ok())
        .collect()
}

/// record 列の `kind` の並び。
pub(super) fn kinds(rows: &[Vec<(String, vessel::fleet::json_lite::Value)>]) -> Vec<String> {
    rows.iter().map(|row| value_of(row, "kind")).collect()
}

/// record 列のうち `skipped=` を持つもの（検出線を省いた record）。
pub(super) fn skip_rows(
    rows: &[Vec<(String, vessel::fleet::json_lite::Value)>],
) -> Vec<&Vec<(String, vessel::fleet::json_lite::Value)>> {
    rows.iter().filter(|row| !value_of(row, "skipped").is_empty()).collect()
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

/// PASS まで通し、`verdict.json` を `edit(本文, tree, base の木)` で差し替えてから land する。
fn detection_land(edit: fn(&str, &str, &str) -> String) -> DetectionLand {
    detection_land_shimmed(edit, None)
}

/// [`detection_land`] の本体。`shim` が在れば、引数列にその字面を含む git だけを rc 1 で落とす偽 git を
/// PATH の先頭に置いて land する（読めない周の極性を測る口・[`land_once_with_git_shim`]）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn detection_land_shimmed(edit: fn(&str, &str, &str) -> String, shim: Option<&str>) -> DetectionLand {
    let (repo, state, design) = detection_repo(DETECTION_COUNT);
    let base_tree = git(&repo, &["rev-parse", "HEAD^{tree}"]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let before = detection_calls(&repo).len();
    let verdict = state.join("pipe").join(&id).join("verdict.json");
    let text = fs::read_to_string(&verdict).expect("verdict.json を読める");
    let tree = value_of(&verdict_pairs(&state, &id), "tree");
    assert!(!tree.is_empty(), "差し替える前の verdict は tree を持つ: {text}");
    fs::write(&verdict, edit(&text, &tree, &base_tree)).expect("verdict.json を差し替えられる");
    let out = match shim {
        None => land_once(&repo, &state, &id),
        Some(failing) => land_once_with_git_shim(&repo, &state, &id, failing, None),
    };
    let added = detection_calls(&repo).split_off(before);
    let rows = main_rows(&state, &id);
    DetectionLand { added, rows, out, repo, state, id }
}

/// verdict の `tree` を**便の base の木**（実在する別の木・差分は便の `src/lib.rs` だけ＝面の外）へ差し替える。
fn verdict_tree_to_base(text: &str, tree: &str, base_tree: &str) -> String {
    text.replace(&format!("\"tree\":\"{tree}\""), &format!("\"tree\":\"{base_tree}\""))
}

/// (1) `detection-verify` が読めて、gate が **① write-set → ② common → ③ detection → ④ 契約** の順で撃つ。
#[test]
fn pipe_detection_verify_fires_third_in_gate() {
    let (repo, state, design) = detection_repo(DETECTION_COUNT);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
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
    let (repo, state, design) = detection_repo(DETECTION_COUNT);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let tree = value_of(&verdict_pairs(&state, &id), "tree");
    assert!(!tree.is_empty(), "tree が在る");
    assert_eq!(tree, git(&worktree_of(&repo, &id), &["rev-parse", "HEAD^{tree}"]), "HEAD の木と一致");
    assert_ne!(tree, git(&repo, &["rev-parse", "HEAD^{tree}"]), "base の木ではない（runner が commit した後の木）");
    clean(&[&repo, &state]);
}

// ---- 検出線の持ち越し（設計 pipeline.md §40 形 (b)・契約表の行 ai・接頭辞 `pipe_detection_carry_`）----------
//
// gate は検出線を撃つ周に便の diff の patch-id を record の `patch_id` に残す。追随の撃ち直しの側（写す周・撃つ周）は
// `land.rs` の同じ接頭辞の歯が持つ。

/// `dir` の `git diff <base>..<tip>` を `git patch-id --stable` に通した値（器の外で測る＝歯の期待値の出所）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn measured_patch_id(dir: &Path, base: &str, tip: &str) -> String {
    use std::io::Write;
    let diff = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["diff", &format!("{base}..{tip}")])
        .output()
        .expect("diff を撃てる")
        .stdout;
    assert!(!diff.is_empty(), "fixture: 便の diff は空でない");
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["patch-id", "--stable"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("patch-id を起こせる");
    child.stdin.take().expect("stdin を取れる").write_all(&diff).expect("diff を渡せる");
    let out = child.wait_with_output().expect("patch-id が終わる");
    String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap_or_default().to_owned()
}

/// (h) gate が検出線を撃つ周の record は便の diff の `patch_id` を持ち、`git patch-id --stable`（`<base>..HEAD`）と
/// 一致する。他の段の record は持たず、撃った record は `carried` を持たない（写しと実測の区別・C10）。
#[test]
fn pipe_detection_carry_gate_record_holds_the_patch_id_of_the_diff() {
    let (repo, state, design) = detection_repo(DETECTION_COUNT);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "detection", "contract"], "段の順序: {rows:?}");
    let want = measured_patch_id(&worktree_of(&repo, &id), &base, "HEAD");
    assert_eq!(want.len(), 40, "fixture: patch-id は 40 桁: {want}");
    assert_eq!(row_value(&rows, 3, "patch_id"), want, "検出線の record は便の diff の patch-id: {rows:?}");
    for n in [1, 2, 4] {
        assert!(row_value(&rows, n, "patch_id").is_empty(), "検出線の外の段は patch_id を持たない（n={n}）: {rows:?}");
    }
    assert!(row_value(&rows, 3, "carried").is_empty(), "撃った record は carried を持たない: {rows:?}");
    clean(&[&repo, &state]);
}

/// (3) 木が gate と同じ main 実測は **1 本も撃たず** `kind=main skipped=main tree=<sha>` の record 1 本を記す
/// （設計 gate-cost.md §27・ADR-0043 §2.1・`s2-07l.464`。以前は ②④ を撃って ③ だけを省いた）。
#[test]
fn pipe_detection_land_skips_detection_when_tree_matches() {
    let landed = detection_land(|text, _, _| text.to_owned());
    let (rows, out) = (&landed.rows, &landed.out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(out));
    assert!(landed.added.is_empty(), "main 実測は verify の cmd を 1 本も撃たない: {:?}", landed.added);
    assert_eq!(kinds(rows), ["main"], "record は主実測の skip 1 本だけ: {rows:?}");
    assert_eq!(row_value(rows, 1, "skipped"), "main", "skipped=main");
    assert_eq!(row_value(rows, 1, "tree"), git(&landed.repo, &["rev-parse", "refs/heads/main^{tree}"]), "tree=<land した木>");
    clean(&[&landed.repo, &landed.state]);
}

// ---- 主実測の省略（設計 gate-cost.md §27・ADR-0043 §2.1・`s2-07l.464`・接頭辞 `pipe_main_same_tree_`）----------
//
// gate が判定した木と着地の木が同じ周は、主実測は同じ木を同じ verify で撃ち直すだけ＝1 本も撃たず、
// `verify-main.jsonl` に主実測の skip record 1 本（`kind=main skipped=main tree=<land した木> reason=same-tree`）を
// 書いて緑。木が違う周と `tree` の無い周は従来の段数（③ の省き方は `pipe_detection_scope_` の歯の規則のまま）。

/// 便の `RunDone stage=Landed` の detail（無ければ空）。
fn landed_done_detail(state: &Path, id: &str) -> String {
    trail(state, id)
        .into_iter()
        .rev()
        .find(|(kind, stage, _)| *kind == EventKind::RunDone && *stage == Some(Stage::Landed))
        .and_then(|(_, _, detail)| detail)
        .unwrap_or_default()
}

/// (a) 同じ木の周: record は `kind=main skipped=main tree=<land した木> reason=same-tree` の 1 本（`n` = 1・key 列も
/// pin）で、verify の cmd は 1 本も走らず、land は rc 0・Landed の detail と main の先端（squash）は従来の形。
/// base は ①②④ を撃って 4 段の record を書く＝RED。
#[test]
fn pipe_main_same_tree_writes_one_record_and_fires_nothing() {
    let landed = detection_land(|text, _, _| text.to_owned());
    let (rows, out) = (&landed.rows, &landed.out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(out));
    let main_tree = git(&landed.repo, &["rev-parse", "refs/heads/main^{tree}"]);
    assert_eq!(value_of(&verdict_pairs(&landed.state, &landed.id), "tree"), main_tree, "fixture: gate の木 = land した木");
    assert!(landed.added.is_empty(), "主実測は verify の cmd を 1 本も撃たない: {:?}", landed.added);
    assert_eq!(kinds(rows), ["main"], "record は主実測の skip 1 本だけ: {rows:?}");
    let skip = rows.first().cloned().unwrap_or_default();
    let keys: Vec<&str> = skip.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, ["schema", "n", "kind", "skipped", "tree", "reason"], "skip record の key 列: {skip:?}");
    assert_eq!(value_of(&skip, "n"), "1", "`n` は 1");
    assert_eq!(value_of(&skip, "skipped"), "main", "skipped=main");
    assert_eq!(value_of(&skip, "tree"), main_tree, "tree=<land した木>");
    assert_eq!(value_of(&skip, "reason"), "same-tree", "理由は木の一致");
    let new = git(&landed.repo, &["rev-parse", "refs/heads/main"]);
    assert!(stdout_of(out).contains(&format!("landed={new}")), "main の先端は squash: {}", stdout_of(out));
    assert_eq!(landed_done_detail(&landed.state, &landed.id), format!("sha:{new} main:{new}"), "Landed の detail は従来の形");
    clean(&[&landed.repo, &landed.state]);
}

/// (b) 木が違う周（verdict の `tree` を便の base の木に差し替え）: record は従来の段数（① / ② / ③ の skip / ④）で
/// 主実測の skip record（`kind=main`）は無い（(a) と同じ fixture で分岐だけ違う負例の対＝base でも通る）。
#[test]
fn pipe_main_same_tree_different_tree_fires_all_stages() {
    let landed = detection_land(verdict_tree_to_base);
    let (rows, out) = (&landed.rows, &landed.out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(out));
    assert_ne!(
        value_of(&verdict_pairs(&landed.state, &landed.id), "tree"),
        git(&landed.repo, &["rev-parse", "refs/heads/main^{tree}"]),
        "fixture: gate の木と land した木は違う"
    );
    assert_eq!(landed.added, ["common", "contract"], "主実測は ②④ を撃つ: {:?}", landed.added);
    assert_eq!(kinds(rows), ["write-set", "common", "detection", "contract"], "従来の段数: {rows:?}");
    assert!(rows.iter().all(|row| value_of(row, "kind") != "main"), "主実測の skip record は無い: {rows:?}");
    clean(&[&landed.repo, &landed.state]);
}

/// (c) verdict に `tree` が無い周（旧 gate の形）: 全段を撃つ（`added` は ②③④・`kind=main` の record は無い）。
/// (a) との対＝base でも通る。
#[test]
fn pipe_main_same_tree_missing_tree_fires_all_stages() {
    let landed = detection_land(|text, tree, _| text.replace(&format!(",\"tree\":\"{tree}\""), ""));
    assert!(verdict_pairs(&landed.state, &landed.id).iter().all(|(key, _)| key != "tree"), "fixture は tree の無い形");
    assert_detection_fired(&landed);
    assert!(landed.rows.iter().all(|row| value_of(row, "kind") != "main"), "主実測の skip record は無い: {:?}", landed.rows);
    clean(&[&landed.repo, &landed.state]);
}

/// (e) 同じ木の判定は tmp worktree を**切る前**（設計 gate-cost.md §27 (1)）: 置き場を塞いでも同じ木の周は撃たずに
/// 緑で、塞いだ物に触れない。判定が worktree の後に在る変異は「切れない」で Unmeasurable（rc 2）に倒れる。
#[test]
fn pipe_main_same_tree_decides_before_cutting_the_worktree() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &path, &state.join("lens-ran"));
    let blocked = repo.join(".worktrees").join("scribe2").join("verify").join(&id);
    fs::create_dir_all(&blocked).expect("tmp の置き場を塞げる");
    fs::write(blocked.join("occupied"), "x\n").expect("塞げる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "同じ木の周は worktree を切らない＝塞いでも緑: {}", stderr_of(&out));
    assert_eq!(kinds(&main_rows(&state, &id)), ["main"], "主実測は skip record 1 本");
    assert!(blocked.join("occupied").exists(), "塞いだ置き場に触れていない");
    clean(&[&repo, &state]);
}

/// (f) skip record を書けない周は**緑を名乗らない**（C10・設計 gate-cost.md §27 (1)）: `verify-main.jsonl` の path を
/// dir で塞ぐと rc 2・`main-unmeasured` の event・Landed は無い。Err の腕を Green に倒す変異はここで落ちる。
#[test]
fn pipe_main_same_tree_unwritable_record_is_unmeasurable() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &path, &state.join("lens-ran"));
    let record = state.join("pipe").join(&id).join("verify-main.jsonl");
    fs::create_dir_all(&record).expect("record の path を dir で塞げる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "書けない周は rc 2: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("実測できない"), "赤ではなく測れないと名乗る: {}", stderr_of(&out));
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(log.contains("\"detail\":\"main-unmeasured\""), "main-unmeasured の event: {log}");
    assert!(!log.contains("\"stage\":\"Landed\""), "Landed は無い: {log}");
    clean(&[&repo, &state]);
}

// ---- 検出線の面（設計 pipeline.md §30・`s2-07l.397`・接頭辞 `pipe_detection_scope_`）------------
//
// 追随の再 gate 側（docs だけ / crates が動いた周）は `land.rs` の歯（同じ接頭辞）が持つ。ここは主実測の側。

/// (d) main が動いていない周の主実測は `kind=main skipped=main tree=<land した木> reason=same-tree` の record 1 本
/// （`n` = 1・key 列も pin・設計 gate-cost.md §27 で ②④ も撃たない形に改めた）。
#[test]
fn pipe_detection_scope_same_tree_records_reason() {
    let landed = detection_land(|text, _, _| text.to_owned());
    let (rows, out) = (&landed.rows, &landed.out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(out));
    assert!(landed.added.is_empty(), "主実測は 1 本も撃たない: {:?}", landed.added);
    assert_eq!(rows.len(), 1, "record は 1 本: {rows:?}");
    let skip = rows.first().cloned().unwrap_or_default();
    let keys: Vec<&str> = skip.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, ["schema", "n", "kind", "skipped", "tree", "reason"], "skip record の key 列: {skip:?}");
    assert_eq!(value_of(&skip, "n"), "1", "1 本目");
    assert_eq!(value_of(&skip, "kind"), "main", "kind=main");
    assert_eq!(value_of(&skip, "skipped"), "main", "skipped=main");
    assert_eq!(value_of(&skip, "tree"), git(&landed.repo, &["rev-parse", "refs/heads/main^{tree}"]), "tree=<land した木>");
    assert_eq!(value_of(&skip, "reason"), "same-tree", "理由は木の一致");
    clean(&[&landed.repo, &landed.state]);
}

/// (f) gate を撃った木と land した木が**違っても**、`diff-tree` の path が面に 1 つも触れなければ省く
/// （`reason=outside-scope`・設計 §30 (ii)）。fixture は verdict の `tree` を便の base の木に差し替える＝差分は
/// 便が触った `src/lib.rs` だけで、toy repo では面の外である。base は木の不一致で撃つ＝RED。
#[test]
fn pipe_detection_scope_main_skips_detection_when_tree_differs_outside_scope() {
    let landed = detection_land(verdict_tree_to_base);
    let (rows, out) = (&landed.rows, &landed.out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(out));
    let gated = value_of(&verdict_pairs(&landed.state, &landed.id), "tree");
    let main_tree = git(&landed.repo, &["rev-parse", "refs/heads/main^{tree}"]);
    assert_ne!(gated, main_tree, "fixture: gate の木と land した木は違う");
    assert_eq!(
        git(&landed.repo, &["diff-tree", "-r", "--name-only", &gated, &main_tree]),
        "src/lib.rs",
        "fixture: 差分は面の外の 1 path"
    );
    assert_eq!(landed.added, ["common", "contract"], "主実測は ②④ だけを撃つ: {:?}", landed.added);
    assert_eq!(kinds(rows), ["write-set", "common", "detection", "contract"], "省いた段も位置に record が在る: {rows:?}");
    assert_eq!(row_value(rows, 3, "skipped"), "detection");
    assert_eq!(row_value(rows, 3, "tree"), main_tree, "tree=<land した木>（gate の木ではない）");
    assert_eq!(row_value(rows, 3, "reason"), "outside-scope", "理由は面の外");
    clean(&[&landed.repo, &landed.state]);
}

/// (c) **読めない周は撃つ**（fail-closed・設計 §30）: verdict の `tree` を落とした形（木を比べられない）と、
/// 木は違うが `diff-tree` を読めない形（偽 git がその 1 呼出しだけ rc 1）の 2 つ。どちらも ③ を撃ち skip record を
/// 残さない（省く側へ倒すと、測っていない検出線を main で通したことになる）。
#[test]
fn pipe_detection_scope_unreadable_diff_fires_detection() {
    // 木を比べられない: `tree` の無い verdict（既存 (5) の型）。
    let no_tree = detection_land(|text, tree, _| text.replace(&format!(",\"tree\":\"{tree}\""), ""));
    assert!(verdict_pairs(&no_tree.state, &no_tree.id).iter().all(|(key, _)| key != "tree"), "fixture は tree の無い形");
    assert_detection_fired(&no_tree);
    clean(&[&no_tree.repo, &no_tree.state]);
    // 木は違う（(f) と同じ fixture＝偽 git が無ければ省く周）が diff を読めない。
    let unreadable = detection_land_shimmed(verdict_tree_to_base, Some(" diff-tree -r --name-only -z "));
    assert_detection_fired(&unreadable);
    clean(&[&unreadable.repo, &unreadable.state]);
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

/// (4) verdict の `tree` が land した木と違い**比べられない**（実在しない木＝`diff-tree` を読めない）周は **③ も撃つ**。
#[test]
fn pipe_detection_land_fires_detection_when_tree_differs() {
    let landed = detection_land(|text, tree, _| {
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
    let landed = detection_land(|text, tree, _| text.replace(&format!(",\"tree\":\"{tree}\""), ""));
    let verdict = verdict_pairs(&landed.state, &landed.id);
    assert!(verdict.iter().all(|(key, _)| key != "tree"), "fixture は tree の無い旧形: {verdict:?}");
    assert_eq!(value_of(&verdict, "verdict"), "PASS", "fixture の verdict は読める形のまま");
    assert_detection_fired(&landed);
    clean(&[&landed.repo, &landed.state]);
}

/// (6) 検出線の rc≠0 は **従来どおり gate FAIL**（測れなかったを通ったに化けさせない・lens を呼ばない）。
#[test]
fn pipe_detection_red_line_fails_gate() {
    let (repo, state, design) = detection_repo(r#"["sh verify-red.sh"]"#);
    let id = implemented(&repo, &state, &design);
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

// ---- 検出線の rc 2 = 測れなかった（`s2-07l.331`・設計 pipeline.md §5.3 の③・FR14）------------

/// rc を **worktree の外**から差し替える verify 行の跳び板 script（tracked・便の worktree にも在る）。
///
/// 本体は git の共通 dir（便の worktree からも同じ file）に置く [`DETECTION_STUB`] で、
/// 跳び板はそれを `sh` で撃つだけである。script 本体を worktree で書き換えると木が dirty になり、
/// precheck で止まって「測り直せる」（Gated のまま撃ち直す）を測れない。
const VERIFY_STUBBED: &str = "verify-stubbed.sh";

/// 共通 dir に置く stub の名（本文は `exit <rc>` の 1 行）。
const DETECTION_STUB: &str = "detection-stub.sh";

/// 跳び板を撃つ verify 行（宣言の `common-verify` / `detection-verify` に置く形）。
const STUBBED_LINE: &str = r#"["sh verify-stubbed.sh"]"#;

/// 跳び板を repo へ書いて commit する（**便の base に含める**＝`implemented` の前に呼ぶ）。
fn commit_stub_trampoline(repo: &Path) {
    fs::write(
        repo.join(VERIFY_STUBBED),
        format!("sh \"$(git rev-parse --git-common-dir)/{DETECTION_STUB}\"\n"),
    )
    .ok();
    git(repo, &["add", "-f", VERIFY_STUBBED]);
    git(repo, &["commit", "-q", "-m", "verify-stubbed"]);
}

/// stub を `exit <rc>` に書き換える（gate の前・撃ち直しの前のどちらでも・木は汚れない）。
fn write_detection_stub(repo: &Path, rc: u8) {
    fs::write(repo.join(".git").join(DETECTION_STUB), format!("exit {rc}\n")).ok();
}

/// 検出線が跳び板の便を実装済みにする（共通 verify は `verify-count.sh common`・契約 verify も stub）。
fn stubbed_detection_run(rc: u8) -> (PathBuf, PathBuf, String) {
    let (repo, state, design) = detection_repo(STUBBED_LINE);
    commit_stub_trampoline(&repo);
    write_detection_stub(&repo, rc);
    let id = implemented(&repo, &state, &design);
    (repo, state, id)
}

/// 便の `Gated` event の detail の並び（測り直しの履歴）。
fn gated_details(state: &Path, id: &str) -> Vec<String> {
    events(state)
        .into_iter()
        .filter(|event| event.run == id && event.stage == Some(Stage::Gated))
        .filter_map(|event| event.detail)
        .collect()
}

/// (a) 検出線の rc 2 は **赤ではなく「測れなかった」**——verdict は INCONCLUSIVE・`verify_red` は 0・
/// 段は Gated のまま（測り直せる）・lens は呼ばない。record（verify.jsonl の行・stderr.log の見出し）は
/// 現物のまま残る。負例: 検出線の rc 1（deny 昇格後の赤）と、共通 verify の rc 2 は従来どおり FAIL。
#[test]
fn pipe_gate_detection_unmeasured_is_inconclusive_not_fail() {
    let (repo, state, id) = stubbed_detection_run(2);
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_INCONCLUSIVE)),
        "測れなかった周の rc は 3: {} / {}",
        stdout_of(&out),
        stderr_of(&out)
    );
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "verdict.json も INCONCLUSIVE");
    assert_eq!(value_of(&pairs, "verify_red"), "0", "検出線の rc 2 は赤に数えない");
    let evidence = value_of(&pairs, "evidence");
    for needle in ["検出線", "n=3", "rc 2"] {
        assert!(evidence.contains(needle), "理由が {needle} を名指す: {evidence}");
    }
    assert!(!marker.exists(), "測れなかった周は lens を起動しない");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated のまま（測り直せる）");
    // record は不変（現物を消さない）。
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "detection", "contract"], "段の並び: {rows:?}");
    assert_eq!(row_value(&rows, 3, "rc"), "2", "③ の rc は現物のまま");
    let tail = fs::read_to_string(state.join("pipe").join(&id).join("verify.stderr.log")).unwrap_or_default();
    assert!(tail.contains("## n=3 rc=2"), "stderr.log の見出しも残る: {tail}");
    clean(&[&repo, &state]);

    // 負例 1: 検出線の rc 1 は赤（R-C12-1 の deny 昇格後の形・除外は rc 2 だけ）。
    let (repo, state, id) = stubbed_detection_run(1);
    assert_stays_red(&repo, &state, &id, ("detection", 3, "1"));
    clean(&[&repo, &state]);

    // 負例 2: 共通 verify の rc 2 は赤（除外は検出線だけ）。
    let (repo, state, id) = stubbed_common_run(2);
    assert_stays_red(&repo, &state, &id, ("common", 2, "2"));
    clean(&[&repo, &state]);
}

/// 共通 verify が跳び板・検出線は緑（`verify-ok.sh`）の便を実装済みにする（負例の fixture）。
fn stubbed_common_run(rc: u8) -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    commit_stub_trampoline(&repo);
    write_detection_stub(&repo, rc);
    write_vessel(&repo, VESSEL_ALLOWED, STUBBED_LINE);
    let path = repo.join(".vessel.toml");
    let body = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, format!("{body}detection-verify = [\"sh verify-ok.sh\"]\n")).ok();
    git(&repo, &["add", "-f", ".vessel.toml"]);
    git(&repo, &["commit", "-q", "-m", "vessel-common-stubbed"]);
    let design = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-ok.sh"]"#]);
    let id = implemented(&repo, &state, &design);
    (repo, state, id)
}

/// 除外の外の赤は従来どおり FAIL（`red` = (kind, n, rc) の 1 行が赤・`verify_red` は 1）。
fn assert_stays_red(repo: &Path, state: &Path, id: &str, red: (&str, usize, &str)) {
    let (kind, n, rc) = red;
    let out = gate_once(repo, state, id, Some(&fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{kind} の rc {rc} は FAIL: {}", stderr_of(&out));
    let rows = verify_rows(state, id);
    assert_eq!(row_value(&rows, n, "kind"), kind, "n={n} の段: {rows:?}");
    assert_eq!(row_value(&rows, n, "rc"), rc, "n={n} の rc は現物: {rows:?}");
    let pairs = verdict_pairs(state, id);
    assert_eq!(value_of(&pairs, "verdict"), "FAIL");
    assert_eq!(value_of(&pairs, "verify_red"), "1", "{kind} の rc {rc} は赤に数える: {pairs:?}");
}

/// (c) 測れなかった検出線は **撃ち直せる**——stub を `exit 0` へ書き換えて `pipe gate` を撃ち直すと
/// PASS（`pipe_gate_regates_after_inconclusive` と同型・1 度目の INCONCLUSIVE は event に残る）。
#[test]
fn pipe_gate_detection_unmeasured_regates_to_pass() {
    let (repo, state, id) = stubbed_detection_run(2);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let first = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(first.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "1 度目は測れなかった: {}", stderr_of(&first));
    assert!(!marker.exists(), "1 度目は lens を起動しない");

    write_detection_stub(&repo, 0);
    let second = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(
        second.status.code(),
        Some(i32::from(RC_OK)),
        "検出線を直した測り直しは通る: {} / {}",
        stdout_of(&second),
        stderr_of(&second)
    );
    assert!(marker.exists(), "2 度目は lens を起動する");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "verdict.json は新しい判定で上書きされる");
    assert_eq!(value_of(&pairs, "verify_red"), "0");
    // verify.jsonl は追記される（1 度目の rc 2 の record は残り、2 度目の ③ は緑）。
    let detection_rcs: Vec<String> = verify_rows(&state, &id)
        .iter()
        .filter(|row| value_of(row, "kind") == "detection")
        .map(|row| value_of(row, "rc"))
        .collect();
    assert_eq!(detection_rcs, ["2", "0"], "1 度目の record は残り、撃ち直した ③ は緑");
    assert_eq!(
        gated_details(&state, &id),
        vec!["verdict:INCONCLUSIVE".to_owned(), "verdict:PASS".to_owned()],
        "測り直しは 2 件目を追記する（1 件目を書き換えない）"
    );
    clean(&[&repo, &state]);
}

// ---- 検出線の rc 2 の撃ち直し（設計 gate-cost.md §21・`s2-07l.390`・接頭辞 `pipe_detection_retry_`）------------
//
// 検出線が rc 2（測れなかった）で終えた周は同じ行を **1 回だけ**撃ち直し、record は 2 回目の 1 本に
// `retried=1` を載せる。母集団 = `detection-calls` の行（撃たれた順・段の印つき）と record の kind 別の本数。

/// 自分の印の行数が奇数なら rc 2・偶数なら rc 0（1 回目 rc 2 → 2 回目 rc 0）。印は `verify-count.sh` と同じ file。
const VERIFY_FLAKY2: &str = "verify-flaky2.sh";

/// 印を 1 行足して常に rc 2（撃ち直しても測れない周）。
const VERIFY_UNMEASURED: &str = "verify-unmeasured.sh";

/// 印を 1 行足して rc 1（`verify-red.sh` の印つきの形＝deny 昇格後の赤・撃ち直さない周）。
const VERIFY_RED_COUNT: &str = "verify-red-count.sh";

/// 撃ち直しの歯の stub 3 本を repo に置いて commit する（便の base に含める＝`implemented` の前に呼ぶ）。
///
/// 印は **git の共通 dir** の `detection-calls`（[`detection_calls`] が読む file）で、第 1 引数を 1 行足す。
/// `verify-flaky2.sh` は足した後に**自分の印の行数**を数えるので、共通 verify や契約の行の印は数に入らない。
fn commit_retry_scripts(repo: &Path) {
    let mark = "calls=\"$(git rev-parse --git-common-dir)/detection-calls\"\nprintf '%s\\n' \"$1\" >> \"$calls\"\n";
    for (name, tail) in [
        (VERIFY_FLAKY2, "if test \"$(( $(grep -c -x -F -- \"$1\" \"$calls\") % 2 ))\" -eq 1; then exit 2; fi\nexit 0\n"),
        (VERIFY_UNMEASURED, "exit 2\n"),
        (VERIFY_RED_COUNT, "exit 1\n"),
    ] {
        fs::write(repo.join(name), format!("{mark}{tail}")).ok();
    }
    git(repo, &["add", "-f", VERIFY_FLAKY2, VERIFY_UNMEASURED, VERIFY_RED_COUNT]);
    git(repo, &["commit", "-q", "-m", "verify-retry-stubs"]);
}

/// 撃ち直しの歯の便を 1 本 gate まで通す（共通 verify は呼び手が選ぶ・検出線は `<script> detection-{base}`）。
///
/// 返すのは (repo, state, id, base, gate の出力)。印の file は gate の前は空（`implemented` は verify を撃たない）
/// なので、`detection_calls` の全行が gate 1 周の母集団である。
fn retry_gate(common: &str, detection_script: &str) -> (PathBuf, PathBuf, String, String, Output) {
    retry_gate_with(common, detection_script, r#"verify = ["sh verify-count.sh contract"]"#)
}

/// [`retry_gate`] の契約 verify も呼び手が選ぶ形（`contract` は契約 file の `verify = [...]` の行）。
fn retry_gate_with(
    common: &str,
    detection_script: &str,
    contract: &str,
) -> (PathBuf, PathBuf, String, String, Output) {
    let (repo, state) = repo_with_state();
    commit_retry_scripts(&repo);
    write_vessel(&repo, VESSEL_ALLOWED, common);
    let path = repo.join(".vessel.toml");
    let body = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, format!("{body}detection-verify = [\"sh {detection_script} detection-{{base}}\"]\n")).ok();
    git(&repo, &["add", "-f", ".vessel.toml"]);
    git(&repo, &["commit", "-q", "-m", "vessel-detection-retry"]);
    let design = write_contract(&repo, &["verify"], &[contract]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let id = implemented(&repo, &state, &design);
    assert!(detection_calls(&repo).is_empty(), "fixture: gate の前は印が無い");
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    (repo, state, id, base, out)
}

/// 印の行のうち、段の印 `mark` に一致する本数。
fn calls_marked(repo: &Path, mark: &str) -> usize {
    detection_calls(repo).iter().filter(|call| *call == mark).count()
}

/// record 列のうち `kind` の本数。
fn kind_count_of(rows: &[Vec<(String, vessel::fleet::json_lite::Value)>], kind: &str) -> usize {
    rows.iter().filter(|row| value_of(row, "kind") == kind).count()
}

/// record 列のうち `retried` を持つものの `kind`。
fn retried_kinds(rows: &[Vec<(String, vessel::fleet::json_lite::Value)>]) -> Vec<String> {
    rows.iter()
        .filter(|row| !value_of(row, "retried").is_empty())
        .map(|row| value_of(row, "kind"))
        .collect()
}

/// 撃ち直した周の record 列の共通 assert: 段ごとに 1 本（1 回目は積まない）・③は `rc=<rc>` ∧ `retried=1`・
/// `retried` を持つのは③だけ。
fn assert_retried_detection_record(rows: &[Vec<(String, vessel::fleet::json_lite::Value)>], rc: &str) {
    assert_eq!(kinds(rows), ["write-set", "common", "detection", "contract"], "record は段ごとに 1 本: {rows:?}");
    assert_eq!(kind_count_of(rows, "detection"), 1, "1 回目の rc 2 は record に積まない: {rows:?}");
    assert_eq!(row_value(rows, 3, "kind"), "detection", "③ の record");
    assert_eq!(row_value(rows, 3, "rc"), rc, "record は 2 回目の rc: {rows:?}");
    assert_eq!(row_value(rows, 3, "retried"), "1", "撃ち直した事実が record に残る: {rows:?}");
    assert_eq!(retried_kinds(rows), ["detection"], "`retried` を持つのは③だけ: {rows:?}");
}

/// (a) 1 回目 rc 2 → 2 回目 rc 0 の検出線は gate が **PASS** で終わる。検出線の印は +2（撃ち直した）・②④ は +1 ずつ・
/// `kind=detection` の record は **1 本**（1 回目は積まない）で `rc=0` ∧ `retried=1`・他の段に `retried` は無い。
#[test]
fn pipe_detection_retry_reruns_an_unmeasured_line_once_and_passes() {
    let (repo, state, id, base, out) = retry_gate(r#"["sh verify-count.sh common"]"#, VERIFY_FLAKY2);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "撃ち直して緑なら PASS: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=PASS"), "{}", stdout_of(&out));
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS", "verdict.json も PASS");
    assert!(state.join("lens-ran").exists(), "測れた周は lens を起動する");
    let calls = detection_calls(&repo);
    assert_eq!(calls.len(), 4, "母集団 = ② 1 + ③ 2 + ④ 1: {calls:?}");
    assert_eq!(calls_marked(&repo, &format!("detection-{base}")), 2, "③ は 1 回だけ撃ち直す: {calls:?}");
    assert_eq!(
        calls,
        ["common".to_owned(), format!("detection-{base}"), format!("detection-{base}"), "contract".to_owned()],
        "撃ち直しは同じ行の直後（段の順序は不変）"
    );
    let rows = verify_rows(&state, &id);
    assert_retried_detection_record(&rows, "0");
    assert_eq!(row_value(&rows, 3, "cmd"), format!("sh {VERIFY_FLAKY2} detection-{base}"), "穴の値は 2 回目も同じ");
    let log = verify_log(&state, &id);
    assert_eq!(log.matches("\"retried\":1").count(), 1, "生の record は数の 1 で 1 か所だけ: {log}");
    assert_eq!(log.matches("\"retried\"").count(), 1, "`retried` の字面は 1 record だけ（bool や別名を足さない）: {log}");
    clean(&[&repo, &state]);
}

/// (b) 2 回目も rc 2 なら従来どおり **INCONCLUSIVE**（3 回目は撃たない）。record は 1 本で `rc=2` ∧ `retried=1`。
#[test]
fn pipe_detection_retry_stops_after_one_retry_when_still_unmeasured() {
    let (repo, state, id, base, out) = retry_gate(r#"["sh verify-count.sh common"]"#, VERIFY_UNMEASURED);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_INCONCLUSIVE)),
        "2 回とも測れなかった周は rc 3: {} / {}",
        stdout_of(&out),
        stderr_of(&out)
    );
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE");
    assert_eq!(value_of(&pairs, "verify_red"), "0", "検出線の rc 2 は赤に数えない");
    assert!(value_of(&pairs, "evidence").contains("n=3"), "理由は③を名指す: {pairs:?}");
    assert!(!state.join("lens-ran").exists(), "測れなかった周は lens を起動しない");
    let calls = detection_calls(&repo);
    assert_eq!(calls.len(), 4, "母集団 = ② 1 + ③ 2 + ④ 1: {calls:?}");
    assert_eq!(calls_marked(&repo, &format!("detection-{base}")), 2, "撃ち直しは 1 回だけ（3 回目は無い）: {calls:?}");
    assert_retried_detection_record(&verify_rows(&state, &id), "2");
    clean(&[&repo, &state]);
}

/// (c) 撃ち直さない 2 つ: 検出線の **rc 1**（deny 昇格後の赤）と、**共通 verify の rc 2**。どちらも印は +1・
/// record に `retried` は無く・従来どおり FAIL（母集団 = 印の全行）。
#[test]
fn pipe_detection_retry_does_not_rerun_red_or_common_lines() {
    // 検出線の rc 1: 撃ち直さず FAIL。
    let (repo, state, id, base, out) = retry_gate(r#"["sh verify-count.sh common"]"#, VERIFY_RED_COUNT);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "検出線の rc 1 は FAIL: {}", stderr_of(&out));
    let calls = detection_calls(&repo);
    assert_eq!(calls, ["common".to_owned(), format!("detection-{base}"), "contract".to_owned()], "③ は 1 回だけ");
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "kind"), "detection");
    assert_eq!(row_value(&rows, 3, "rc"), "1", "③ が赤");
    assert!(retried_kinds(&rows).is_empty(), "撃ち直していない record に `retried` は無い: {rows:?}");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "FAIL");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "1", "rc 1 の検出線は赤に数える");
    clean(&[&repo, &state]);

    // 共通 verify の rc 2: 撃ち直さず FAIL（除外は検出線だけ）。検出線は緑（`verify-count.sh`）。
    let (repo, state, id, base, out) = retry_gate(r#"["sh verify-unmeasured.sh common"]"#, "verify-count.sh");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "共通 verify の rc 2 は FAIL: {}", stderr_of(&out));
    let calls = detection_calls(&repo);
    assert_eq!(calls, ["common".to_owned(), format!("detection-{base}"), "contract".to_owned()], "② は 1 回だけ");
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 2, "kind"), "common");
    assert_eq!(row_value(&rows, 2, "rc"), "2", "② の rc は現物のまま");
    assert!(retried_kinds(&rows).is_empty(), "共通 verify の rc 2 に `retried` は無い: {rows:?}");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "FAIL");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "1", "共通 verify の rc 2 は赤に数える");
    clean(&[&repo, &state]);
}

/// (d) 撃ち直した周の `verify.stderr.log` は **1 回目の見出し（`rc=2 retry=1`）→ 2 回目の見出し（`rc=0`）** の順で
/// 両方を持つ（1 回目を捨てる実装は落ちる・2 回目は緑でも見出しを残す）。1 回目を従来の見出しでは書かない。
#[test]
fn pipe_detection_retry_keeps_the_first_stderr_tail() {
    let (repo, state, id, base, out) = retry_gate(r#"["sh verify-count.sh common"]"#, VERIFY_FLAKY2);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "撃ち直して緑なら PASS: {}", stderr_of(&out));
    let tail = fs::read_to_string(state.join("pipe").join(&id).join("verify.stderr.log")).unwrap_or_default();
    let cmd = format!("sh {VERIFY_FLAKY2} detection-{base}");
    let first = tail.find(&format!("## n=3 rc=2 retry=1 cmd={cmd}"));
    let second = tail.find(&format!("## n=3 rc=0 cmd={cmd}"));
    assert!(first.is_some(), "1 回目の見出しが在る: {tail}");
    assert!(second.is_some(), "2 回目の見出しが在る: {tail}");
    assert!(first < second, "1 回目が先: {tail}");
    assert!(!tail.contains("## n=3 rc=2 cmd="), "1 回目を従来の見出しでは書かない: {tail}");
    let heads: Vec<&str> = tail.lines().filter(|line| line.starts_with("## ")).collect();
    assert_eq!(heads.len(), 2, "見出しは③の 2 段だけ（②④は緑で撃ち直しも無い）: {tail}");
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
    let design = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-ok.sh"]"#]);
    let id = implemented(&repo, &state, &design);
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

// ---- 赤い行が在る周は検出線が測れなくても FAIL（設計 gate-cost.md §28・`s2-07l.495`・接頭辞 `pipe_gate_red_wins_over_detection_`）----
//
// 検出線は測る前に元の木の歯を全部走らせるので、歯が赤い木では必ず rc 2 で終わる。rc 2 を赤より先に
// 読むと、赤いと分かっている便が INCONCLUSIVE のまま居座る。赤（検出線 ∧ rc 2 だけ除く）が 1 行でも
// 在れば FAIL・赤が 0 の周だけ rc 2 が INCONCLUSIVE に倒す。fixture は撃ち直しの歯と同じ stub
// （`verify-red-count.sh` = rc 1・`verify-unmeasured.sh` = 常に rc 2）。

/// 赤 ∧ 検出線 rc 2 の周の共通 assert: rc 1・`Gated` verdict=FAIL・evidence は赤い行の数（今の FAIL と
/// 同じ字面・検出線を名指さない）・`verify_red` は 1・lens は呼ばれない・③ の record は rc 2 の現物のまま。
fn assert_red_wins(state: &Path, id: &str, out: &Output, red: (&str, usize, &str)) {
    let (kind, n, rc) = red;
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "赤が在る周は FAIL の rc 1: {} / {}", stdout_of(out), stderr_of(out));
    assert!(stdout_of(out).contains("verdict=FAIL"), "{}", stdout_of(out));
    let pairs = verdict_pairs(state, id);
    assert_eq!(value_of(&pairs, "verdict"), "FAIL", "verdict.json も FAIL: {pairs:?}");
    assert_eq!(value_of(&pairs, "verify_red"), "1", "赤の数え方は不変（検出線の rc 2 は数えない）: {pairs:?}");
    let evidence = value_of(&pairs, "evidence");
    assert_eq!(evidence, "verify の 1 行が rc≠0", "evidence は赤い行の数（今の FAIL と同じ字面）");
    assert!(!evidence.contains("検出線"), "evidence は検出線を名指さない: {evidence}");
    assert!(!state.join("lens-ran").exists(), "赤い周は lens を起動しない");
    assert_eq!(gated_details(state, id), vec!["verdict:FAIL".to_owned()], "Gated の detail は FAIL 1 件");
    let rows = verify_rows(state, id);
    assert_eq!(kinds(&rows), ["write-set", "common", "detection", "contract"], "段の並び: {rows:?}");
    assert_eq!(row_value(&rows, n, "kind"), kind, "n={n} の段: {rows:?}");
    assert_eq!(row_value(&rows, n, "rc"), rc, "n={n} の rc は現物: {rows:?}");
    assert_eq!(row_value(&rows, 3, "rc"), "2", "③ の rc 2 は現物のまま（record の形は不変）: {rows:?}");
    assert_eq!(row_value(&rows, 3, "retried"), "1", "撃ち直し（§21）は不変＝撃ち直した後の値で順を読む: {rows:?}");
}

/// (a) 共通 verify が赤（rc 1）∧ 検出線が rc 2 の便は `Gated` verdict=FAIL に着く（INCONCLUSIVE ではない）。
/// evidence は赤い行の数・lens は呼ばれない。検出線は従来どおり 1 回撃ち直してから順を読む。
#[test]
fn pipe_gate_red_wins_over_detection_unmeasured_when_common_is_red() {
    let (repo, state, id, base, out) = retry_gate(r#"["sh verify-red-count.sh common"]"#, VERIFY_UNMEASURED);
    assert_red_wins(&state, &id, &out, ("common", 2, "1"));
    assert_eq!(
        detection_calls(&repo),
        ["common".to_owned(), format!("detection-{base}"), format!("detection-{base}"), "contract".to_owned()],
        "段の順と撃ち直しは不変（③ は 1 回だけ撃ち直す）"
    );
    clean(&[&repo, &state]);
}

/// (b) 契約 verify が赤（rc 1）∧ 検出線が rc 2 の便も同じく FAIL（赤の出所が②か④かを問わない）。
#[test]
fn pipe_gate_red_wins_over_detection_unmeasured_when_contract_is_red() {
    let (repo, state, id, base, out) = retry_gate_with(
        r#"["sh verify-count.sh common"]"#,
        VERIFY_UNMEASURED,
        r#"verify = ["sh verify-red-count.sh contract"]"#,
    );
    assert_red_wins(&state, &id, &out, ("contract", 4, "1"));
    assert_eq!(
        detection_calls(&repo),
        ["common".to_owned(), format!("detection-{base}"), format!("detection-{base}"), "contract".to_owned()],
        "④ は③の後に撃たれる（赤が④に在っても③の撃ち直しは不変）"
    );
    clean(&[&repo, &state]);
}

/// (c) 赤が 0 ∧ 検出線が rc 2 の便は今までどおり INCONCLUSIVE（`Gated` のまま測り直せる・lens は呼ばない）。
/// 「赤より先」を「常に FAIL」へ倒す変異はここで落ちる。
#[test]
fn pipe_gate_red_wins_over_detection_unmeasured_only_when_red_exists() {
    let (repo, state, id, _base, out) = retry_gate(r#"["sh verify-count.sh common"]"#, VERIFY_UNMEASURED);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_INCONCLUSIVE)),
        "赤が 0 なら検出線の rc 2 は INCONCLUSIVE: {} / {}",
        stdout_of(&out),
        stderr_of(&out)
    );
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE");
    assert_eq!(value_of(&pairs, "verify_red"), "0", "赤が 0");
    let evidence = value_of(&pairs, "evidence");
    for needle in ["検出線", "n=3", "rc 2"] {
        assert!(evidence.contains(needle), "理由が {needle} を名指す: {evidence}");
    }
    assert!(!state.join("lens-ran").exists(), "測れなかった周は lens を起動しない");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated のまま（測り直せる）");
    assert_eq!(gated_details(&state, &id), vec!["verdict:INCONCLUSIVE".to_owned()]);
    clean(&[&repo, &state]);
}

// ---- 判定行の記録（設計 gate-cost.md §5.1・`s2-07l.206`・接頭辞 `pipe_record_`）--------------------
//
// verify 行の stdout の末尾 1 行を record の `line=` に**逐語で**残す（kind と rc を問わず・判定には
// 使わない）。母集団 = 4 record（write-set 1 + 共通 1 + 検出線 1 + 契約 n）。

/// 検出線の stub が stdout に出す判定行（xtask `mutants-diff` の 1 行の形）。
const DETECTION_LINE: &str = "mutants-diff: total=3 caught=2 missed=1 unviable=0 timeout=0 scope=x";

/// 共通 verify の stub が **rc 0** で stdout に出す判定行（flip-check の形・`base-retried=` を持つ）。
const COMMON_LINE: &str = "flip-check: RED-on-base ok tests_changed=1 base-retried=1";

/// rc≠0 の契約 verify の stub が stdout の末尾に出す行（cmd にも stderr にも無い字面）。
const RED_TAIL: &str = "red-tail-marker total=0";

/// 判定行を出す stub 3 本を repo に置き、共通 verify と検出線をその stub で宣言して commit する。
///
/// 検出線の stub は判定行の**前に** noise を 1 行出す（末尾を取っていることを測る）。赤い stub は
/// stderr にも 1 行出す（`line` が stderr でなく stdout から来ることを弁別する）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn commit_line_vessel(repo: &Path) {
    for (name, body) in [
        ("verify-line-detection.sh", format!("printf 'noise\\n{DETECTION_LINE}\\n'\nexit 0\n")),
        ("verify-line-common.sh", format!("printf '{COMMON_LINE}\\n'\nexit 0\n")),
        ("verify-line-red.sh", format!("printf '{RED_TAIL}\\n'\nprintf 'why\\n' >&2\nexit 1\n")),
    ] {
        fs::write(repo.join(name), body).expect("stub を書ける");
    }
    write_vessel(repo, VESSEL_ALLOWED, r#"["sh verify-line-common.sh"]"#);
    let path = repo.join(".vessel.toml");
    let body = fs::read_to_string(&path).expect("宣言を読める");
    fs::write(&path, format!("{body}detection-verify = [\"sh verify-line-detection.sh\"]\n")).expect("宣言を書ける");
    git(repo, &["add", "-f", ".vessel.toml", "verify-line-detection.sh", "verify-line-common.sh", "verify-line-red.sh"]);
    git(repo, &["commit", "-q", "-m", "vessel-line"]);
}

/// 判定行の fixture で便を 1 本 gate まで通す（契約の verify 行は呼び手が選ぶ・rc は測らない）。
fn line_gate(verify: &str) -> (PathBuf, PathBuf, String, Output) {
    let (repo, state) = repo_with_state();
    commit_line_vessel(&repo);
    let design = write_contract(&repo, &["verify"], &[&format!("verify = {verify}")]);
    let id = implemented(&repo, &state, &design);
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    (repo, state, id, out)
}

/// `verify.jsonl` の生の本文。
fn verify_log(state: &Path, id: &str) -> String {
    fs::read_to_string(vessel::pipe::verify_log_path(state, id)).unwrap_or_default()
}

/// (a) 検出線の record に `line=<xtask の 1 行>` が逐語で載る（noise の行ではなく**末尾**の行）。
#[test]
fn pipe_record_detection_line_is_recorded_verbatim() {
    let (repo, state, id, out) = line_gate(r#"["sh verify-ok.sh"]"#);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "detection", "contract"], "母集団 4 record: {rows:?}");
    assert_eq!(row_value(&rows, 3, "kind"), "detection", "③ の record");
    assert_eq!(row_value(&rows, 3, "rc"), "0", "道具は完走した");
    assert_eq!(row_value(&rows, 3, "line"), DETECTION_LINE, "末尾の 1 行が逐語で載る: {rows:?}");
    let log = verify_log(&state, &id);
    assert!(log.contains(&format!("\"line\":\"{DETECTION_LINE}\"")), "生の record にも逐語: {log}");
    assert!(!log.contains("\"line\":\"noise\""), "末尾でない行は載らない: {log}");
    clean(&[&repo, &state]);
}

/// (b) 共通 verify の record にも `line=` が載る——**rc 0 でも載る**（本便の要点: flip-check の
/// `base-retried=N` は rc 0 で通った周の stdout にしか現れない）。
#[test]
fn pipe_record_common_line_is_recorded_even_when_green() {
    let (repo, state, id, out) = line_gate(r#"["sh verify-ok.sh"]"#);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 2, "kind"), "common", "② の record");
    assert_eq!(row_value(&rows, 2, "rc"), "0", "緑の行である（赤い行だけに載るのではない）");
    assert_eq!(row_value(&rows, 2, "line"), COMMON_LINE, "rc 0 の行にも逐語で載る: {rows:?}");
    // 全行が緑なので**段の見出しは 1 つも無い**＝緑の判定行の置き場は record の `line` だけである。
    // 診断 file に在るのは lens への入力の通知 1 行だけ（段ではないので見出しを持たない・設計 §21 (3)）。
    let tail = stderr_log_body(&state, &id);
    assert!(!tail.contains("## "), "緑の行は段の見出しを残さない: {tail}");
    assert_eq!(tail.lines().count(), 1, "在るのは通知の 1 行だけ: {tail}");
    clean(&[&repo, &state]);
}

/// (c) rc≠0 の行にも末尾 1 行が載る（stderr の log と重複してよい・`line` は stdout から来る）。
#[test]
fn pipe_record_red_line_keeps_its_stdout_tail() {
    let (repo, state, id, out) = line_gate(r#"["sh verify-ok.sh", "sh verify-line-red.sh"]"#);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=FAIL"), "{}", stdout_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(rows.len(), 5, "母集団 = write-set 1 + 共通 1 + 検出線 1 + 契約 2: {rows:?}");
    assert_eq!(row_value(&rows, 5, "cmd"), "sh verify-line-red.sh", "赤いのは契約の 2 本目");
    assert_eq!(row_value(&rows, 5, "rc"), "1", "赤い行");
    assert_eq!(row_value(&rows, 5, "line"), RED_TAIL, "赤い行にも stdout の末尾が載る: {rows:?}");
    let tail = fs::read_to_string(state.join("pipe").join(&id).join("verify.stderr.log")).unwrap_or_default();
    assert!(tail.contains("why"), "stderr の診断 file は従来どおり: {tail}");
    assert!(!tail.contains(RED_TAIL), "stdout の行は stderr の診断 file には混ざらない: {tail}");
    // 判定は stdout で変えない: 赤い行の `line` が在っても FAIL のまま・赤の本数は 1。
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "1", "赤は 1 本");
    clean(&[&repo, &state]);
}

/// (d) stdout の無い行は `line` を**欠く**（`"line":` 不在・空文字を書かない・C10）。
#[test]
fn pipe_record_silent_line_has_no_line_field() {
    let (repo, state, id, out) = line_gate(r#"["sh verify-ok.sh"]"#);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 4, "cmd"), "sh verify-ok.sh", "④ は何も出さない行");
    assert!(!row_has(&rows, 4, "line"), "stdout の無い行は `line` を欠く: {:?}", rows.get(3));
    assert!(!row_has(&rows, 1, "line"), "撃つ process を持たない段①も欠く: {:?}", rows.first());
    let log = verify_log(&state, &id);
    assert!(!log.contains("\"line\":\"\""), "空文字は書かない: {log}");
    assert_eq!(
        log.matches("\"line\":").count(),
        2,
        "`line` を持つのは stdout を出した ②③ の 2 record だけ（母集団 {} record）: {log}",
        rows.len()
    );
    clean(&[&repo, &state]);
}

/// (e) land の main 実測で ③ を省いた周は `verify-main.jsonl` の detection の record に `line` が無い
/// （撃っていない行の判定行を書かない）。同じ関数で書く ② の record には載る＝省いた段だけが欠ける。
/// fixture は verdict の木を base の木に差し替えて主実測を撃つ周にする（同じ木の周は主実測ごと省く・設計 gate-cost.md §27）。
#[test]
fn pipe_record_land_skipped_detection_has_no_line() {
    let (repo, state, id, out) = line_gate(r#"["sh verify-ok.sh"]"#);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    assert_eq!(row_value(&verify_rows(&state, &id), 3, "line"), DETECTION_LINE, "gate の周には載っている");
    super::land::make_tree_differ(&repo, &state, &id, "refs/heads/main");
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    let rows = main_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "detection", "contract"], "main 実測の母集団: {rows:?}");
    assert_eq!(row_value(&rows, 3, "skipped"), "detection", "面の外だけが違う周は ③ を省く");
    assert!(!row_has(&rows, 3, "line"), "省いた段に `line` は無い: {:?}", rows.get(2));
    assert_eq!(row_value(&rows, 2, "line"), COMMON_LINE, "撃った ② には main 実測でも載る（同じ 1 本で書く）");
    clean(&[&repo, &state]);
}

/// (f) `pipe show --run` は 1 行目の段に続けて **検出線の `line` だけ**を逐語で出す（他の kind の
/// `line` は出さない・gate 前は 1 行目だけ）。外形は snapshot（置き場は親の `snapshots/`）。
///
/// 行の末尾の段の秒（`secs=`・設計 gate-cost.md §26 形 (1)）は**周ごとに動く**ので、snapshot に入れる前に
/// [`mask_secs`] で `[secs]` へ置く（`default-features = false` の insta は `add_filter` を持たない・
/// `src/main.rs` の `doctor_external_form` と同じ型）。秒が数であることは [`gate_secs_only_fired_steps_carry_the_wall_clock`] が測る。
#[test]
fn pipe_record_show_external_form() {
    let (repo, state) = repo_with_state();
    commit_line_vessel(&repo);
    let design = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &design);
    let before = show_line(&repo, &state, &id);
    assert_eq!(before.lines().count(), 1, "gate 前は段の 1 行だけ: {before}");
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let shown = show_line(&repo, &state, &id);
    let mut lines = shown.lines();
    let first = lines.next().unwrap_or_default();
    assert!(first.starts_with(&format!("run={id} ")) && first.contains("stage=Gated"), "1 行目は便の段: {first}");
    let rest: Vec<&str> = lines.collect();
    assert!(!rest.iter().any(|line| line.contains("flip-check")), "他の kind の `line` は出さない: {rest:?}");
    let form = mask_secs(&rest.join("\n"));
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("../snapshots");
    settings.bind(|| insta::assert_snapshot!(form));
    clean(&[&repo, &state]);
}

/// 行末の段の秒を `[secs]` へ置く（値は周ごとに動く＝snapshot に入れない・外形だけを固定する）。
fn mask_secs(form: &str) -> String {
    form.lines()
        .map(|line| {
            line.split_once(" secs=")
                .map_or_else(|| line.to_owned(), |(head, _)| format!("{head} secs=[secs]"))
        })
        .collect::<Vec<String>>()
        .join("\n")
}

// ---- 周ごとの検出線の写し（設計 gate-cost.md §15・`s2-07l.298`・接頭辞 `pipe_gate_detection_copy_`）----
//
// gate は検出線を撃った直後に、その周の判定行と出力（`outcomes.json` / `missed.txt`）を run dir の
// **周ごとの置き場**へ写し、`pipe show` の判定行はその写しから読む。母集団 = 写しの dir とその中の file。

/// 偽の検出線の名（宣言の `detection-verify` に置く stub・本文は歯が選ぶ）。
const COPY_STUB: &str = "verify-copy.sh";

/// 写しの置き場の名（run dir 直下）。**字面で組む**——同じ定数を器から引くと、置き場を変えた実装でも
/// 歯が追随して通る（置き場そのものを pin する）。
const COPY_DIR: &str = "detection";

/// 写しの中の判定行の file 名。
const COPY_LINE: &str = "line";

/// 出力の無い周に置かれる marker の名。
const COPY_ABSENT_OUTPUT: &str = "outputs-absent";

/// 判定行の無い周に残る 1 行（**0 件の判定行と別の字面**）。
const COPY_ABSENT_LINE: &str = "detection-line: absent";

/// 周ごとに違う出力と判定行を出す偽の検出線（出力は cargo-mutants と同じ置き場へ書く）。
///
/// 周の番号は git の共通 dir に積む印の行数で、`missed.txt` の本文と判定行の `scope=` の両方に載る
/// ＝1 周目の写しが 2 周目に上書きされたら、どちらの面でも字面が変わる。
const COPY_STUB_ROUNDS: &str = r#"calls="$(git rev-parse --git-common-dir)/detection-calls"
printf 'detection\n' >> "$calls"
n="$(grep -c -x -F -- detection "$calls")"
out=target/mutants-diff/out/mutants.out
mkdir -p "$out"
printf 'src/lib.rs:1: replace one with round %s\n' "$n" > "$out/missed.txt"
printf '{"total_mutants":3}\n' > "$out/outcomes.json"
printf 'mutants-diff: total=3 caught=2 missed=1 unviable=0 timeout=0 scope=round%s\n' "$n"
exit 0
"#;

/// **0 件の周**の偽の検出線（出力は在り `missed.txt` が空・判定行は `missed=0`）。
const COPY_STUB_ZERO: &str = r#"out=target/mutants-diff/out/mutants.out
mkdir -p "$out"
: > "$out/missed.txt"
printf '{"total_mutants":0}\n' > "$out/outcomes.json"
printf 'mutants-diff: total=0 caught=0 missed=0 unviable=0 timeout=0 scope=x\n'
exit 0
"#;

/// 判定行だけを出す偽の検出線（出力を 1 つも書かない周）。
fn copy_stub_line_only() -> String {
    format!("printf '{DETECTION_LINE}\\n'\nexit 0\n")
}

/// `detection` を `detection-verify` に持つ便を Implemented まで進める（共通 verify と契約 verify は静かな stub）。
///
/// `target/` を ignore するのは、検出線の出力が untracked のまま残ると **2 周目の precheck** が
/// 「clean でない」で止まり、2 周分の写しを測れないためである（実 repo でも `target/` は ignore される）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn copy_run(detection: &str) -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    fs::write(repo.join(".gitignore"), "target/\n").expect(".gitignore を書ける");
    fs::write(repo.join(COPY_STUB), detection).expect("stub を書ける");
    write_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-ok.sh"]"#);
    let path = repo.join(".vessel.toml");
    let body = fs::read_to_string(&path).expect("宣言を読める");
    fs::write(&path, format!("{body}detection-verify = [\"sh {COPY_STUB}\"]\n")).expect("宣言を書ける");
    git(&repo, &["add", "-f", ".gitignore", COPY_STUB, ".vessel.toml"]);
    git(&repo, &["commit", "-q", "-m", "vessel-copy"]);
    let design = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-ok.sh"]"#]);
    let id = implemented(&repo, &state, &design);
    (repo, state, id)
}

/// 周 `round` の写しの中の 1 file。
fn copy_path(state: &Path, id: &str, round: u64, leaf: &str) -> PathBuf {
    state.join("pipe").join(id).join(COPY_DIR).join(round.to_string()).join(leaf)
}

/// 周 `round` の写しの 1 file の本文（無ければ空）。
fn read_copy(state: &Path, id: &str, round: u64, leaf: &str) -> String {
    fs::read_to_string(copy_path(state, id, round, leaf)).unwrap_or_default()
}

/// 在る写しの周の番号（昇順・番号でない名は母集団に入らない）。
fn copy_rounds(state: &Path, id: &str) -> Vec<u64> {
    let mut rounds: Vec<u64> = fs::read_dir(state.join("pipe").join(id).join(COPY_DIR))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|found| found.file_name().to_str().and_then(|name| name.parse().ok()))
        .collect();
    rounds.sort_unstable();
    rounds
}

/// PASS の gate を 1 回撃つ（偽 lens つき）。
fn copy_gate(repo: &Path, state: &Path, id: &str) -> Output {
    gate_once(repo, state, id, Some(&fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"))))
}

/// (a) 2 周撃った gate は**周ごとに別の置き場**を持ち、1 周目の生存の一覧が 2 周目の後も読める
/// （上書きされない）。`pipe show` も周の数だけ判定行を並べる。
#[test]
fn pipe_gate_detection_copy_keeps_one_place_per_round() {
    let (repo, state, id) = copy_run(COPY_STUB_ROUNDS);
    // 1 周目は道具の無い周（審査の写しも `--lens` も無い）＝INCONCLUSIVE で、同じ便を 2 周 gate できる。
    fs::remove_file(run_dir(&state, &id).join("lens.toml")).expect("審査の写しを外せる");
    let first = gate_once(&repo, &state, &id, None);
    assert_eq!(first.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "1 周目: {}", stderr_of(&first));
    let second = copy_gate(&repo, &state, &id);
    assert_eq!(second.status.code(), Some(i32::from(RC_OK)), "2 周目: {}", stderr_of(&second));
    assert_eq!(copy_rounds(&state, &id), vec![1, 2], "周ごとに別の置き場が 2 つ");
    for round in [1_u64, 2] {
        assert_eq!(
            read_copy(&state, &id, round, "missed.txt"),
            format!("src/lib.rs:1: replace one with round {round}\n"),
            "周 {round} の生存の一覧はその周の物（上書きされない）"
        );
        assert!(
            read_copy(&state, &id, round, COPY_LINE).contains(&format!("scope=round{round}")),
            "周 {round} の判定行はその周の物: {}",
            read_copy(&state, &id, round, COPY_LINE)
        );
    }
    let shown = show_line(&repo, &state, &id);
    let rest: Vec<&str> = shown.lines().skip(1).collect();
    assert_eq!(rest.len(), 2, "`pipe show` は周の数だけ並べる: {shown}");
    assert!(rest.first().is_some_and(|line| line.contains("scope=round1")), "番号順: {shown}");
    assert!(rest.get(1).is_some_and(|line| line.contains("scope=round2")), "番号順: {shown}");
    clean(&[&repo, &state]);
}

/// (b) 出力を 1 つも書かない周は marker が在り、判定行の写しは在る（(2) の否定の枝）。
#[test]
fn pipe_gate_detection_copy_marks_a_round_without_outputs() {
    let (repo, state, id) = copy_run(&copy_stub_line_only());
    let out = copy_gate(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    assert!(copy_path(&state, &id, 1, COPY_ABSENT_OUTPUT).exists(), "出力の無い周は marker を置く");
    assert!(!copy_path(&state, &id, 1, "outcomes.json").exists(), "写す出力は 1 つも無い");
    assert!(!copy_path(&state, &id, 1, "missed.txt").exists(), "写す出力は 1 つも無い");
    assert_eq!(read_copy(&state, &id, 1, COPY_LINE), format!("{DETECTION_LINE}\n"), "判定行の写しは在る");
    clean(&[&repo, &state]);
}

/// (c) 判定行も出力も無い周は**不在の 1 行**が在り、**0 件の周**（出力が在って missed が 0）とは
/// 別の字面である（空の写しを「0 件だった」に倒さない・C10）。
#[test]
fn pipe_gate_detection_copy_absent_line_differs_from_zero_counts() {
    let (repo, state, id) = copy_run("exit 0\n");
    let out = copy_gate(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    assert_eq!(read_copy(&state, &id, 1, COPY_LINE), format!("{COPY_ABSENT_LINE}\n"), "不在の 1 行");
    assert!(copy_path(&state, &id, 1, COPY_ABSENT_OUTPUT).exists(), "出力も無い周は marker も在る");
    clean(&[&repo, &state]);

    let (repo, state, id) = copy_run(COPY_STUB_ZERO);
    let out = copy_gate(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let zero = read_copy(&state, &id, 1, COPY_LINE);
    assert!(zero.contains("missed=0"), "0 件の周の判定行: {zero}");
    assert_ne!(zero, format!("{COPY_ABSENT_LINE}\n"), "0 件の周は不在の 1 行と別の字面");
    assert!(!copy_path(&state, &id, 1, COPY_ABSENT_OUTPUT).exists(), "出力の在る周に marker は無い");
    assert!(copy_path(&state, &id, 1, "missed.txt").exists(), "0 件の一覧は**空の写し**（不在ではない）");
    assert_eq!(read_copy(&state, &id, 1, "missed.txt"), "", "0 件の一覧の本文は空");
    clean(&[&repo, &state]);
}

/// (d) `pipe show` の判定行の出所は**写し**である（(3) の pin・2 例とも base では RED）。
///
/// (d1) 写しの判定行だけを別の字面へ書き換えると `pipe show` はその字面を出す（`verify.jsonl` の record は
/// 元のまま）。(d2) 写しの判定行を消すと不在の 1 行を出す（record に detection の `line` が在るまま）。
/// base は record から判定行を出すので、どちらも元の字面が出て落ちる。
#[test]
fn pipe_gate_detection_copy_is_the_source_of_the_shown_line() {
    let (repo, state, id) = copy_run(&copy_stub_line_only());
    let out = copy_gate(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    assert_eq!(row_value(&verify_rows(&state, &id), 3, "line"), DETECTION_LINE, "record には道具の 1 行");

    let rewritten = "mutants-diff: total=9 caught=9 missed=0 unviable=0 timeout=0 scope=rewritten";
    fs::write(copy_path(&state, &id, 1, COPY_LINE), format!("{rewritten}\n")).expect("写しを書き換えられる");
    let shown = show_line(&repo, &state, &id);
    assert!(shown.contains(rewritten), "(d1) show は写しの字面を出す: {shown}");
    assert!(!shown.contains(DETECTION_LINE), "(d1) record の字面は出ない: {shown}");
    assert!(verify_log(&state, &id).contains(DETECTION_LINE), "(d1) record は元のまま");

    fs::remove_file(copy_path(&state, &id, 1, COPY_LINE)).expect("写しを消せる");
    let gone = show_line(&repo, &state, &id);
    assert!(gone.contains(COPY_ABSENT_LINE), "(d2) 写しの無い周は不在の 1 行: {gone}");
    assert!(!gone.contains(rewritten), "(d2) 消した写しの字面は出ない: {gone}");
    assert!(!gone.contains(DETECTION_LINE), "(d2) record の字面へは戻らない: {gone}");
    assert_eq!(row_value(&verify_rows(&state, &id), 3, "line"), DETECTION_LINE, "(d2) record は在るまま");
    clean(&[&repo, &state]);
}

// ---- 段の秒（設計 gate-cost.md §26 形 (1)・`s2-07l.466`・接頭辞 `gate_secs_`）--------------------
//
// 撃った段の record だけが `secs=`（process の起動から終了までの壁時計・秒）を持つ。母集団 = 判定行の
// fixture の 4 record（write-set 1 + 共通 1 + 検出線 1 + 契約 1）で、撃つ process を持たない段①と、
// 撃たなかった段（land の skip record）は field を欠く（0 と書かない・C10）。

/// gate の `verify.jsonl` は**撃った段の全部**に `secs=` を持ち段①は持たない・`pipe show --run` の
/// 検出線の行が同じ秒をそのまま写す・land の `verify-main.jsonl` も同じ形（省いた段は持たない）。
#[test]
fn gate_secs_only_fired_steps_carry_the_wall_clock() {
    let (repo, state, id, out) = line_gate(r#"["sh verify-ok.sh"]"#);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "detection", "contract"], "母集団 4 record: {rows:?}");
    assert!(!row_has(&rows, 1, "secs"), "撃つ process を持たない段①は秒を欠く: {:?}", rows.first());
    for n in [2, 3, 4] {
        assert!(row_has(&rows, n, "secs"), "撃った段 {n} は秒を持つ: {rows:?}");
        assert!(row_value(&rows, n, "secs").parse::<u64>().is_ok(), "秒は数（{n} 番目）: {rows:?}");
    }
    let log = verify_log(&state, &id);
    assert_eq!(
        log.matches("\"secs\":").count(),
        3,
        "秒を持つのは撃った 3 record だけ（母集団 {} record）: {log}",
        rows.len()
    );
    // `pipe show --run` は record の秒をそのまま写す（器は数え直さない）。
    let shown = show_line(&repo, &state, &id);
    let line = shown.lines().nth(1).unwrap_or_default().to_owned();
    assert_eq!(
        line,
        format!("{DETECTION_LINE} secs={}", row_value(&rows, 3, "secs")),
        "検出線の行は判定行の逐語 + record の秒: {shown}"
    );
    // land の主実測も同じ 1 本（`step_record`）を通る＝撃った段は秒を持ち、省いた段は持たない。
    // fixture は verdict の木を base の木に差し替えて主実測を撃つ周にする（同じ木の周は主実測ごと省く・設計 gate-cost.md §27）。
    super::land::make_tree_differ(&repo, &state, &id, "refs/heads/main");
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    let main = main_rows(&state, &id);
    assert_eq!(kinds(&main), ["write-set", "common", "detection", "contract"], "main 実測の母集団: {main:?}");
    assert_eq!(row_value(&main, 3, "skipped"), "detection", "面の外だけが違う周は ③ を省く");
    assert!(!row_has(&main, 3, "secs"), "撃たなかった段（skip record）は秒を欠く: {:?}", main.get(2));
    assert!(row_has(&main, 2, "secs"), "verify-main.jsonl も同じ形で秒を持つ: {main:?}");
    clean(&[&repo, &state]);
}

// ---- 純移動の機械証明（設計 pipeline.md §5.3・`s2-07l.266`・接頭辞 `pipe_gate_move_`）--------------------
//
// base の `src/` に item を持つ file を commit し、runner が HEAD の形（置き場へ写した file 群）を `cp` して
// commit する。lens の入力が要約か diff かは判定行の `lens-input=`・run dir の `lens-input.txt`・fake lens が
// 読んだ stdin の 3 面で測る（`s2-07l.261` と同型の fixture = 1 file → 複数 file・`pub(super)` 化・module doc・
// 区切り線・`// flip-check: moved` の札）。

/// 移動前の `src/lib.rs`（helper 3 本 + struct 1 本・module doc・区切り線）。
const MOVE_BASE_LIB: &str = "//! seed crate.\n\n/// helper one.\nfn one() -> u8 {\n    1\n}\n\n/// helper two.\nfn two() -> u8 {\n    2\n}\n\n#[derive(Debug)]\npub struct Pair {\n    a: u8,\n}\n\n// ── section ──\n\nfn three() -> u8 {\n    3\n}\n";

/// 移動後の `src/lib.rs`（札・`mod` 宣言・`pub(crate)` 化・one は残る）。
const MOVE_HEAD_LIB: &str = "// flip-check: moved s2-07l.261\n//! seed crate（split）.\n\nmod alpha;\nmod beta;\n\n/// helper one.\npub(crate) fn one() -> u8 {\n    1\n}\n";

/// 移動後の `src/alpha.rs`（two と Pair・`pub(super)` 化・`use`）。
const MOVE_HEAD_ALPHA: &str = "//! alpha.\n\nuse super::one;\n\n/// helper two.\npub(super) fn two() -> u8 {\n    2\n}\n\n#[derive(Debug)]\npub struct Pair {\n    a: u8,\n}\n";

/// 移動後の `src/beta.rs`（three・区切り線）。
const MOVE_HEAD_BETA: &str = "//! beta.\n\n// ── section ──\n\npub(super) fn three() -> u8 {\n    3\n}\n";

/// 要約の先頭行（`lens-input.txt` と lens の stdin の先頭が名乗る字面）。
const SUMMARY_HEADLINE: &str = "これは diff ではなく純移動の要約である";

/// 純移動の fixture の HEAD の 3 file。
fn move_head() -> Vec<(&'static str, &'static str)> {
    vec![("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", MOVE_HEAD_ALPHA), ("beta.rs", MOVE_HEAD_BETA)]
}

/// 純移動の fixture の便を Implemented まで進める: base の file 群を seed の上に commit し、HEAD の file 群を
/// 置き場へ写して runner に `cp` させる（write-set は 3 file）。
fn move_run(base: &[(&str, &str)], head: &[(&str, &str)]) -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    let id = move_run_in(&repo, &state, base, head);
    (repo, state, id)
}

/// [`move_run`] の本体（toy repo は呼び手が用意する＝宣言を先に commit できる）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn move_run_in(repo: &Path, state: &Path, base: &[(&str, &str)], head: &[(&str, &str)]) -> String {
    for (name, body) in base {
        fs::write(repo.join("src").join(name), body).expect("base の file を書ける");
    }
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "move-base"]);
    let staged = state.join("head");
    fs::create_dir_all(&staged).expect("HEAD の写しの dir を作れる");
    for (name, body) in head {
        fs::write(staged.join(name), body).expect("HEAD の file を書ける");
    }
    let design = write_contract(repo, &["write-set"], &[r#"write-set = ["src/lib.rs", "src/alpha.rs", "src/beta.rs"]"#]);
    let id = intake(repo, state, &design);
    let runner = format!("cp '{}'/*.rs src/ && git add -A && git commit -q -m runner", staged.display());
    let out = spawn_with(repo, state, &id, &runner);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    id
}

/// stdin の全文を `seen` へ写してから PASS を返す fake lens。
fn recording_lens(seen: &Path) -> String {
    format!("cat > '{}'; echo '{}'", seen.display(), lens_verdict("PASS"))
}

/// 判定行の `key=<値>`（無ければ空）。
fn token_of(line: &str, key: &str) -> String {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(key))
        .unwrap_or_default()
        .to_owned()
}

/// run dir の `lens-input.txt`。
fn lens_input_path(state: &Path, id: &str) -> PathBuf {
    state.join("pipe").join(id).join("lens-input.txt")
}

/// 便の worktree の `git diff <base>..HEAD` の生 byte 数（runner の commit は 1 本＝`HEAD~1..HEAD`）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn raw_diff_len(repo: &Path, id: &str) -> usize {
    Command::new("git")
        .arg("-C")
        .arg(worktree_of(repo, id))
        .args(["diff", "HEAD~1..HEAD"])
        .output()
        .expect("git を起動できる")
        .stdout
        .len()
}

/// (i) 純移動の便は lens の入力が**要約**になる: 判定行 `lens-input=summary bytes=<要約の byte>`・`lens-input.txt` が
/// 在り先頭行が名乗る・fake lens が読んだ stdin は残した本文そのもの・verdict が読める・`diff_bytes` は diff の byte
/// のまま・stderr に理由の行は出ない。
#[test]
fn pipe_gate_move_proof_pure_move_sends_summary() {
    let (repo, state, id) = move_run(&[("lib.rs", MOVE_BASE_LIB)], &move_head());
    let seen = state.join("lens-stdin");
    let out = gate_once(&repo, &state, &id, Some(&recording_lens(&seen)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(token_of(&line, "lens-input="), "summary", "判定行: {line}");
    let kept = fs::read_to_string(lens_input_path(&state, &id)).expect("lens-input.txt が在る");
    assert_eq!(kept.lines().next(), Some(SUMMARY_HEADLINE), "先頭行が名乗る: {kept}");
    let received = fs::read_to_string(&seen).expect("lens が読んだ stdin を読める");
    assert_eq!(received, kept, "lens が読んだ stdin は残した本文そのもの");
    assert_eq!(received.lines().next(), Some(SUMMARY_HEADLINE), "stdin の先頭行も名乗る");
    assert_eq!(token_of(&line, "bytes="), kept.len().to_string(), "bytes= は要約の byte: {line}");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "verdict が読める");
    assert_eq!(value_of(&pairs, "evidence"), "fake", "lens の evidence を写す");
    assert_eq!(value_of(&pairs, "diff_bytes"), raw_diff_len(&repo, &id).to_string(), "diff_bytes は diff の byte のまま");
    assert_ne!(value_of(&pairs, "diff_bytes"), kept.len().to_string(), "要約の byte ではない");
    assert_eq!(stderr_of(&out), "", "純移動の周は理由の行を出さない");
    assert_summary_moves(&kept);
    assert_summary_residual(&kept);
    assert!(!kept.contains("carried markers"), "持ち越した札 0 の周は行を出さない: {kept}");
    clean(&[&repo, &state]);
}

/// 要約の中身 (1): 移動元 → 先と本数・動いた item の名・可視性の変化（残った item も動いた item も）。
fn assert_summary_moves(kept: &str) {
    assert!(kept.contains("src/lib.rs -> src/alpha.rs: items=2 lines="), "移動元 → 先: {kept}");
    assert!(kept.contains("\n  fn two\n  struct Pair\n"), "動いた item の名: {kept}");
    assert!(kept.contains("src/lib.rs -> src/beta.rs: items=1 lines=3\n  fn three\n"), "{kept}");
    assert!(kept.contains("src/lib.rs fn one: private -> pub(crate)\n"), "残った item の可視性: {kept}");
    assert!(kept.contains("src/alpha.rs fn two: private -> pub(super)\n"), "動いた item の可視性: {kept}");
    assert!(!kept.contains("    1\n"), "item の本文は要約に載らない: {kept}");
}

/// 要約の中身 (2): 残差分の逐語（札・区切り線・宣言）と判定行。
fn assert_summary_residual(kept: &str) {
    assert!(kept.contains("\n+// flip-check: moved s2-07l.261\n"), "札は残差分に逐語: {kept}");
    assert!(kept.contains("\n-// ── section ──\n"), "区切り線（base 側）: {kept}");
    assert!(kept.contains("\n+// ── section ──\n"), "区切り線（HEAD 側）: {kept}");
    assert!(kept.contains("\n+use super::one;\n"), "use の宣言: {kept}");
    assert!(kept.contains("\n+mod alpha;\n"), "mod の宣言: {kept}");
    assert!(kept.lines().last().is_some_and(|last| last.starts_with("判定: 名 + 本文の多重集合が一致 ")), "判定行: {kept}");
}

/// 純移動でない fixture を gate まで通し、lens の入力が diff であることの共通 assert（理由は呼び手が名指す）。
fn assert_sends_diff(head: &[(&str, &str)], reason: &str) {
    assert_sends_diff_from(&[("lib.rs", MOVE_BASE_LIB)], head, reason);
}

/// [`assert_sends_diff`] の base も呼び手が渡す形（base に札を持つ fixture・`s2-07l.362`）。
fn assert_sends_diff_from(base: &[(&str, &str)], head: &[(&str, &str)], reason: &str) {
    let (repo, state, id) = move_run(base, head);
    let seen = state.join("lens-stdin");
    let out = gate_once(&repo, &state, &id, Some(&recording_lens(&seen)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{reason}: lens は diff で呼ばれ PASS: {}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(token_of(&line, "lens-input="), "diff", "{reason}: 判定行: {line}");
    assert_eq!(token_of(&line, "bytes="), raw_diff_len(&repo, &id).to_string(), "{reason}: bytes= は diff の byte");
    assert!(!lens_input_path(&state, &id).exists(), "{reason}: 要約を残さない");
    // 読めない周は空＝下の `diff --git` の assert が落ちる（expect を helper に置かない）。
    let received = fs::read_to_string(&seen).unwrap_or_default();
    assert!(received.starts_with("diff --git "), "{reason}: stdin は diff: {received}");
    assert!(!received.contains(SUMMARY_HEADLINE), "{reason}: 要約を名乗らない");
    assert_eq!(
        stderr_of(&out).trim_end(),
        format!("pipe: lens-input=diff reason={reason}"),
        "閉じた理由 1 行（typed）"
    );
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS", "{reason}: 従来どおり lens の verdict");
    clean(&[&repo, &state]);
}

/// (ii) 本文を 1 行変えた fixture は純移動でなく diff が渡る（`items-differ`）。
#[test]
fn pipe_gate_move_proof_body_change_sends_diff() {
    let changed = MOVE_HEAD_BETA.replace("    3\n", "    4\n");
    assert_ne!(changed, MOVE_HEAD_BETA, "fixture は本文が 1 行違う");
    assert_sends_diff(&[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", MOVE_HEAD_ALPHA), ("beta.rs", &changed)], "items-differ");
}

/// (iii) 宣言と札とコメント以外の行が残差分に残る fixture も diff（`residual-line`）。
#[test]
fn pipe_gate_move_proof_residual_line_sends_diff() {
    let noisy = MOVE_HEAD_LIB.replace("mod alpha;\n", "#![allow(dead_code)]\nmod alpha;\n");
    assert_ne!(noisy, MOVE_HEAD_LIB, "fixture は宣言でない行を 1 つ持つ");
    assert_sends_diff(&[("lib.rs", &noisy), ("alpha.rs", MOVE_HEAD_ALPHA), ("beta.rs", MOVE_HEAD_BETA)], "residual-line");
}

/// (iv) 宣言だけの fixture（item は 1 つも動かない）は純移動でない（`nothing-moved`）。
#[test]
fn pipe_gate_move_proof_zero_moved_items_sends_diff() {
    let declared = format!("mod alpha;\nmod beta;\n\n{MOVE_BASE_LIB}");
    assert_sends_diff(&[("lib.rs", &declared), ("alpha.rs", "//! alpha.\n"), ("beta.rs", "//! beta.\n")], "nothing-moved");
}

/// (v) `// flip-check: retroactive` の札が残差分に在る fixture は diff（lens v2 medium・`foreign-marker`）。
#[test]
fn pipe_gate_move_proof_retroactive_marker_sends_diff() {
    let marked = MOVE_HEAD_BETA.replace("//! beta.\n\n", "//! beta.\n\n// flip-check: retroactive s2-07l.261\n");
    assert_ne!(marked, MOVE_HEAD_BETA, "fixture は retroactive の札を持つ");
    assert_sends_diff(&[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", MOVE_HEAD_ALPHA), ("beta.rs", &marked)], "foreign-marker");
}

/// 大きい本文を持つ fn（移動すると diff は本文を 2 度運び、要約は名だけを運ぶ）。
fn big_fn(visibility: &str) -> String {
    let mut body = format!("{visibility}fn big() -> u32 {{\n");
    for number in 0..120 {
        body.push_str(&format!("    let _ = \"line {number:03} of the moved body, long enough to weigh\";\n"));
    }
    body.push_str("    0\n}\n");
    body
}

/// (vi) 予算の照合は lens に渡す本文の byte で行う: 要約は cap 内・diff は cap 超の fixture が PASS/FAIL の判定へ
/// 進み INCONCLUSIVE にならず、`verdict.json` の `diff_bytes` は diff の byte（cap 超）のまま。
#[test]
fn pipe_gate_move_proof_budget_uses_summary_bytes() {
    let base_lib = format!("//! big.\n\n{}", big_fn(""));
    let head_lib = "//! big.\n\nmod alpha;\n".to_owned();
    let head_alpha = format!("//! alpha.\n\n{}", big_fn("pub(super) "));
    let (repo, state, id) = move_run(&[("lib.rs", &base_lib)], &[("lib.rs", &head_lib), ("alpha.rs", &head_alpha), ("beta.rs", "//! beta.\n")]);
    let cap = 4_000;
    let diff_len = raw_diff_len(&repo, &id);
    assert!(diff_len > cap, "前提: diff は cap 超（{diff_len} byte）");
    let rules = write_rules(&repo, "summary-cap.toml", 1, cap as u64);
    let marker = state.join("lens-ran");
    let out = gate_with_rules(&repo, &state, &id, &rules, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "判定へ進む: {}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(token_of(&line, "verdict="), "PASS", "{line}");
    assert_eq!(token_of(&line, "lens-input="), "summary", "{line}");
    let bytes: usize = token_of(&line, "bytes=").parse().unwrap_or(usize::MAX);
    assert!(bytes <= cap, "要約は cap 内: {line}");
    assert!(marker.exists(), "lens を起動した");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "INCONCLUSIVE にならない");
    assert!(!value_of(&pairs, "evidence").contains("cap"), "cap の理由が無い: {}", value_of(&pairs, "evidence"));
    assert_eq!(value_of(&pairs, "diff_bytes"), diff_len.to_string(), "diff_bytes は diff の byte のまま");
    let kept = fs::read_to_string(lens_input_path(&state, &id)).expect("lens-input.txt が在る");
    assert_eq!(kept.len(), bytes, "bytes= は残した要約の byte");
    assert!(kept.contains("src/lib.rs -> src/alpha.rs: items=1 lines=123\n  fn big\n"), "{kept}");
    clean(&[&repo, &state]);
}

/// (vii) 要約の外形（fixture (i) の `lens-input.txt` の全文・置き場は親の `snapshots/`）。
#[test]
fn pipe_gate_move_summary_external_form() {
    let (repo, state, id) = move_run(&[("lib.rs", MOVE_BASE_LIB)], &move_head());
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let form = fs::read_to_string(lens_input_path(&state, &id)).expect("lens-input.txt が在る");
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("../snapshots");
    settings.bind(|| insta::assert_snapshot!(form));
    clean(&[&repo, &state]);
}

// ---- item の中のコメント行（`s2-07l.294`・.286 = module を跨ぐ移動の doc link 書き換えが本文差と読まれた型）----

/// `two` の doc に intra-doc link を持つ形（`from` の path を `to` へ書き換える）。
fn linked(text: &str, path: &str) -> String {
    let linked = text.replace("/// helper two.\n", &format!("/// helper two (see [`{path}`]).\n"));
    assert_ne!(linked, text, "fixture は two の doc を持つ");
    linked
}

/// (viii) 移した item の doc コメントの link path だけを書き換えた便（`[`super::one`]` → `[`crate::one`]`）は
/// 純移動: 判定行 `lens-input=summary`・要約に「コメント行の差」の節（該当 item の名と行数の直後に base 側 `-` /
/// head 側 `+` の逐語・設計 §25・`s2-07l.377`）・他の面（移動・可視性・stderr）は (i) と同じ。
#[test]
fn pipe_gate_move_proof_comment_only_diff_inside_items_sends_summary() {
    let (base, alpha) = (linked(MOVE_BASE_LIB, "super::one"), linked(MOVE_HEAD_ALPHA, "crate::one"));
    let (repo, state, id) = move_run(&[("lib.rs", &base)], &[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", &alpha), ("beta.rs", MOVE_HEAD_BETA)]);
    let seen = state.join("lens-stdin");
    let out = gate_once(&repo, &state, &id, Some(&recording_lens(&seen)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(token_of(&line, "lens-input="), "summary", "判定行: {line}");
    let kept = fs::read_to_string(lens_input_path(&state, &id)).expect("lens-input.txt が在る");
    assert_eq!(fs::read_to_string(&seen).unwrap_or_default(), kept, "lens が読んだ stdin は残した本文そのもの");
    assert!(
        kept.contains("\n## コメント行の差（名: 行数）\nsrc/alpha.rs fn two: 1\n-/// helper two (see [`super::one`]).\n+/// helper two (see [`crate::one`]).\n## 残差分（逐語）\n"),
        "件数の行の直後に base 側 - / head 側 + の逐語: {kept}"
    );
    assert_eq!(kept.matches("helper two").count(), 2, "コメントの字面は - / + の 2 行だけに載る: {kept}");
    assert_eq!(stderr_of(&out), "", "純移動の周は理由の行を出さない");
    assert_summary_moves(&kept);
    assert_summary_residual(&kept);
    clean(&[&repo, &state]);
}

/// (ix) 移した item の中に `// flip-check: retroactive` の札を足した便は diff（`foreign-marker`）＝コメント行の除外が
/// 札まで緩めていない対（(v) の札は残差分・本 fixture の札は fn の本文の中）。
#[test]
fn pipe_gate_move_proof_comment_marker_inside_item_sends_diff() {
    let marked = MOVE_HEAD_ALPHA.replace("    2\n", "    // flip-check: retroactive s2-07l.294\n    2\n");
    assert_ne!(marked, MOVE_HEAD_ALPHA, "fixture は item の中に retroactive の札を持つ");
    assert_sends_diff(&[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", &marked), ("beta.rs", MOVE_HEAD_BETA)], "foreign-marker");
}

/// (x) コメント行の書き換え + 本文 1 行の書き換えは diff（`items-differ`）＝除外はコメント行だけに閉じる。
#[test]
fn pipe_gate_move_proof_comment_and_body_change_sends_diff() {
    let changed = linked(MOVE_HEAD_ALPHA, "crate::one").replace("    2\n", "    3\n");
    assert!(changed.contains("    3\n"), "fixture は本文も 1 行違う");
    assert_sends_diff(&[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", &changed), ("beta.rs", MOVE_HEAD_BETA)], "items-differ");
}

// ---- 持ち越しの札（`s2-07l.362`・.361 run 2 = base に元から在る `retroactive` の札が item ごと移り新規と読まれた型）----

/// `two` の本文の中に `// flip-check: retroactive <id>` の札を持つ形（base と HEAD の両側に同じ字面で置く）。
fn carried(text: &str, id: &str) -> String {
    let marked = text.replace("    2\n", &format!("    // flip-check: retroactive {id}\n    2\n"));
    assert_ne!(marked, text, "fixture は two の本文を持つ");
    marked
}

/// (xi) base の item に元から在る `retroactive` の札を、その item ごと別 file へ移した便は純移動: 判定行
/// `lens-input=summary`・要約が持ち越した札の本数を 1 行で名乗る（判定行の直前）・札の字面は残差分に載らない
/// （item の中の行）・他の面（移動・可視性・stderr）は (i) と同じ。
#[test]
fn pipe_gate_move_proof_carried_retroactive_marker_sends_summary() {
    let (base, alpha) = (carried(MOVE_BASE_LIB, "s2-07l.1"), carried(MOVE_HEAD_ALPHA, "s2-07l.1"));
    let (repo, state, id) = move_run(&[("lib.rs", &base)], &[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", &alpha), ("beta.rs", MOVE_HEAD_BETA)]);
    let seen = state.join("lens-stdin");
    let out = gate_once(&repo, &state, &id, Some(&recording_lens(&seen)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(token_of(&line, "lens-input="), "summary", "判定行: {line}");
    let kept = fs::read_to_string(lens_input_path(&state, &id)).expect("lens-input.txt が在る");
    assert_eq!(fs::read_to_string(&seen).unwrap_or_default(), kept, "lens が読んだ stdin は残した本文そのもの");
    assert!(kept.contains("\ncarried markers: 1\n判定: 名 + 本文の多重集合が一致 "), "持ち越した札の本数を判定行の直前で名乗る: {kept}");
    assert_eq!(kept.matches("carried markers").count(), 1, "1 行だけ: {kept}");
    assert!(!kept.contains("retroactive"), "item の中の札の字面は要約に載らない: {kept}");
    assert_eq!(stderr_of(&out), "", "純移動の周は理由の行を出さない");
    assert_summary_moves(&kept);
    assert_summary_residual(&kept);
    clean(&[&repo, &state]);
}

/// (xii) 同じ base で HEAD 側の札の id だけを変えた便は diff（`foreign-marker`）＝対は id まで含む字面で取る
/// （持ち越しを装って別の id の札を足す形を通さない・退行の pin）。
#[test]
fn pipe_gate_move_proof_carried_marker_with_a_different_id_sends_diff() {
    let (base, alpha) = (carried(MOVE_BASE_LIB, "s2-07l.1"), carried(MOVE_HEAD_ALPHA, "s2-07l.2"));
    assert_sends_diff_from(&[("lib.rs", &base)], &[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", &alpha), ("beta.rs", MOVE_HEAD_BETA)], "foreign-marker");
}

/// (xiii) base に在る札を HEAD で落とした便は diff（`foreign-marker`）＝消えた札も対が無い（札を消す変更は純移動でない）。
#[test]
fn pipe_gate_move_proof_carried_dropped_marker_sends_diff() {
    let base = carried(MOVE_BASE_LIB, "s2-07l.1");
    assert_sends_diff_from(&[("lib.rs", &base)], &move_head(), "foreign-marker");
}

// ───── lens の口座も器が選ぶ（`s2-07l.412`・設計 account-autonomy.md §15・SRS FR36 / FR33・接頭辞 `pipe_gate_lens_account_`） ─────
//
// 口座の当たり / 空きは置き場へ実測行を直接置くのでなく、便の歯と同じ fixture（`ratelimit.rs` の偽 curl の応答本文）で
// 作る——選定は毎回計測し直し、最新の 1 行が置き場の行を無条件に置き換えるので、直接置いた行は計測で上書きされ歯が
// 空虚になる。

/// 器が起動行の末尾に足した語を写す偽 lens（`sh <script>`＝行の末尾の語は script の引数に届く）。
///
/// argv を 1 行 1 語で `lens-argv` へ写し、呼ばれた回数を `lens-calls` へ積み、verdict の JSON 1 行を返す。
/// 「lens を**起こさなかった**」（候補なしの周）を呼出回数 0 で測れる形である。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn argv_lens(state: &Path, verdict: &str) -> String {
    let spy = lens_spy(state);
    fs::create_dir_all(&spy).expect("偽 lens の置き場を作れる");
    let body = format!(
        "#!/bin/sh\ncat >/dev/null\nprintf 'call\\n' >> '{}'\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s\\n' '{}'\n",
        spy.join("calls").display(),
        spy.join("argv").display(),
        lens_verdict(verdict)
    );
    let path = state.join("argv-lens.sh");
    fs::write(&path, body).expect("偽 lens を書ける");
    format!("sh {}", path.display())
}

/// 偽 lens の置き場（呼出回数と argv の写し）。
fn lens_spy(state: &Path) -> PathBuf {
    state.join("lens-spy")
}

/// 偽 lens に渡された argv（1 行 1 語・無ければ空）。
fn lens_argv(state: &Path) -> Vec<String> {
    fs::read_to_string(lens_spy(state).join("argv"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// 偽 lens が起こされた回数（file が無ければ 0）。
fn lens_calls(state: &Path) -> usize {
    fs::read_to_string(lens_spy(state).join("calls"))
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

/// `--rules`（口座の宣言を持つ写し）・偽 curl・偽 lens を渡して gate を 1 回撃つ。
fn gate_with_accounts(repo: &Path, state: &Path, id: &str, rules: &str, lens: &str) -> Output {
    run_pipe(&[
        "gate", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", rules, "--curl", &lifecycle::fake_usage_curl(state), "--lens", lens,
    ])
}

/// (a) 宣言口座のある置き場の gate は、lens を起こす直前に便用の規則で口座を選び、起動行の末尾に
/// `--account-dir <state>/accounts/<label>` を足して `Gated` の detail に `account:<label>` を残す。
///
/// a1 は当たっている（5 時間窓 100%）ので余裕の a2 が選ばれる＝flag の値は選定の結果である（固定の 1 つ目ではない）。
/// 計測は lens の前に 1 回（口座 2 つ分の偽 curl）。base は口座を選ばず足さない → RED。
#[test]
fn pipe_gate_lens_account_is_chosen_and_appended() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let rules = lifecycle::resume_rules(&state, &["a1", "a2"]);
    lifecycle::put_account(&state, "a1", &[lifecycle::windows(100, 10)]);
    lifecycle::put_account(&state, "a2", &[lifecycle::windows(40, 10)]);
    let out = gate_with_accounts(&repo, &state, &id, &rules, &argv_lens(&state, "PASS"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(lens_calls(&state), 1, "lens は 1 回起きる");
    assert_eq!(
        lifecycle::argv_account_dir(&lens_argv(&state)),
        Some(state.join("accounts").join("a2").display().to_string()),
        "lens の argv の末尾に選んだ口座の credential dir: {:?}",
        lens_argv(&state)
    );
    assert_eq!(lifecycle::curl_calls(&state), 2, "lens の前に FR33 の計測を 1 回（口座 2 つ）");
    assert_eq!(
        gated_details(&state, &id),
        vec!["verdict:PASS,account:a2".to_owned()],
        "記帳は判定と起こした口座を対で運ぶ"
    );
    clean(&[&repo, &state]);
}

/// (b) 口座の宣言が 0 の置き場は従来どおり親の環境を継承する（負例・極性不変）: lens の argv に `--account-dir` が
/// 無く・`Gated` の detail は `verdict:<V>` だけ・計測も撃たない。
#[test]
fn pipe_gate_lens_account_absent_when_no_declared_accounts() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let rules = write_rules(&state, "rules-plain.toml", 1, 1_000_000);
    let out = gate_with_accounts(&repo, &state, &id, &rules.display().to_string(), &argv_lens(&state, "PASS"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(lens_calls(&state), 1, "lens は 1 回起きる");
    assert_eq!(lifecycle::argv_account_dir(&lens_argv(&state)), None, "宣言 0 は起動行を変えない: {:?}", lens_argv(&state));
    assert_eq!(lifecycle::curl_calls(&state), 0, "宣言の無い置き場は測らない");
    assert_eq!(gated_details(&state, &id), vec!["verdict:PASS".to_owned()], "detail は判定だけ");
    clean(&[&repo, &state]);
}

/// (c) 全口座が当たっている周は **lens を起こさず** INCONCLUSIVE（理由に `account:none=<reason>`）。
///
/// gate は段の判定で待ちを持たない（`AccountFree` の待ちは runner 側だけ）＝便は `Gated` のまま測り直せる側に残り、
/// `resume` が撃ち直す。base は口座を見ずに lens を起こす → RED。
#[test]
fn pipe_gate_lens_account_none_is_inconclusive_without_calling_lens() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let rules = lifecycle::resume_rules(&state, &["a1", "a2"]);
    lifecycle::put_account(&state, "a1", &[lifecycle::windows(100, 10)]);
    lifecycle::put_account(&state, "a2", &[lifecycle::windows(100, 10)]);
    let out = gate_with_accounts(&repo, &state, &id, &rules, &argv_lens(&state, "PASS"));
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_INCONCLUSIVE)),
        "候補なしは測れなかった側: {} / {}",
        stdout_of(&out),
        stderr_of(&out)
    );
    assert_eq!(lens_calls(&state), 0, "lens を起こさない（写し 0）");
    assert_eq!(lifecycle::curl_calls(&state), 2, "計測は撃つ（選定の入力）");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "{pairs:?}");
    assert!(value_of(&pairs, "evidence").contains("account:none="), "理由は候補なしを名乗る: {pairs:?}");
    assert_eq!(gated_details(&state, &id), vec!["verdict:INCONCLUSIVE".to_owned()], "選べなかった周は account を足さない");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "便は Gated のまま（測り直せる側）");
    clean(&[&repo, &state]);
}

// ── lens の出力の形が読めなかった周の撃ち直し（設計 gate-cost.md §29・`s2-07l.495`）──────

/// stderr に写る撃ち直しの行の頭（1 回目の理由が `reason=` の後に続く）。
const REREAD_LINE: &str = "pipe: lens-reread=1 reason=";

/// 撃たれた回数を数え、**1 回目と 2 回目で別の出力**を返す偽 lens（§29 の歯の fixture・歯の中で書く）。
///
/// 回数は [`lens_calls`] の置き場に積む。`after` は出力の後に走る行（`exit 7` 等・空なら rc 0 で終わる）。
/// 3 回目以降も `second` を返す＝「3 回目は無い」は回数で測る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn counting_lens(state: &Path, first: &str, second: &str, after: &str) -> String {
    let spy = lens_spy(state);
    fs::create_dir_all(&spy).expect("偽 lens の置き場を作れる");
    let calls = spy.join("calls").display().to_string();
    let body = format!(
        "#!/bin/sh\ncat >/dev/null\nprintf 'call\\n' >> '{calls}'\nif [ \"$(wc -l < '{calls}')\" -eq 1 ]; then\n  printf '%s\\n' '{first}'\nelse\n  printf '%s\\n' '{second}'\nfi\n{after}\n"
    );
    let path = state.join("counting-lens.sh");
    fs::write(&path, body).expect("偽 lens を書ける");
    format!("sh {}", path.display())
}

/// 母集団の行数が数でない（実測 2026-09-20 の `~330` の形）本文。
fn unreadable_population(lines: &str) -> String {
    findings_body(&format!(",\"findings\":\"{ZERO_FINDINGS}\",\"population\":\"files:1,lines:{lines}\""))
}

/// stderr の撃ち直しの行（無ければ `None`）。
fn reread_line(out: &Output) -> Option<String> {
    stderr_of(out).lines().find(|line| line.starts_with(REREAD_LINE)).map(str::to_owned)
}

/// (a) 1 回目が数でない母集団・2 回目が正しい出力の lens は **PASS** で終わり、撃たれた回数が 2・stderr に
/// 撃ち直しの 1 行（1 回目の理由つき）が在る。
///
/// base は 1 回目の戻りをそのまま判定にする＝INCONCLUSIVE で回数 1 → RED。
#[test]
fn pipe_gate_lens_reread_unreadable_then_readable_passes_with_two_calls() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let lens = counting_lens(&state, &unreadable_population("~330"), &lens_verdict("PASS"), "");
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "2 回目の出力で PASS: {} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(lens_calls(&state), 2, "撃ち直しは 1 回（合計 2 回）");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "{pairs:?}");
    assert_eq!(value_of(&pairs, "evidence"), "fake", "理由は 2 回目の lens のもの: {pairs:?}");
    assert_eq!(value_of(&pairs, "population"), FAKE_POPULATION, "集計も 2 回目のもの: {pairs:?}");
    let line = reread_line(&out).unwrap_or_default();
    assert!(
        line.contains("population の lines が数でない（~330）"),
        "撃ち直しの行が 1 回目の理由を運ぶ: {}",
        stderr_of(&out)
    );
    assert_eq!(stderr_of(&out).matches(REREAD_LINE).count(), 1, "撃ち直しの行は 1 本: {}", stderr_of(&out));
    // record の field も verdict.json の schema も足さない（撃ち直した事実は stderr の行だけ）。
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    assert!(!keys.iter().any(|key| key.contains("reread")), "verdict.json に field を足さない: {keys:?}");
    assert_eq!(gated_details(&state, &id), vec!["verdict:PASS".to_owned()], "Gated の detail も不変");
    clean(&[&repo, &state]);
}

/// (b) 2 回とも数でない周は INCONCLUSIVE で回数が 2（**3 回目は無い**）・理由は 2 回目のもの（1 回目は stderr の行）。
#[test]
fn pipe_gate_lens_reread_twice_unreadable_is_inconclusive_with_second_reason() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let lens = counting_lens(&state, &unreadable_population("~330"), &unreadable_population("~331"), "");
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "2 回目も読めなければ rc 3: {}", stdout_of(&out));
    assert_eq!(lens_calls(&state), 2, "3 回目は撃たない");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "{pairs:?}");
    let evidence = value_of(&pairs, "evidence");
    assert!(evidence.contains("population の lines が数でない（~331）"), "理由は 2 回目のもの: {evidence}");
    assert!(!evidence.contains("~330"), "1 回目の理由は判定に載らない: {evidence}");
    assert_eq!(value_of(&pairs, "population"), "", "測れていない周は field を書かない");
    let line = reread_line(&out).unwrap_or_default();
    assert!(line.contains("（~330）"), "1 回目の理由は stderr の行に残る: {}", stderr_of(&out));
    assert_eq!(stderr_of(&out).matches(REREAD_LINE).count(), 1, "撃ち直しの行は 1 本: {}", stderr_of(&out));
    clean(&[&repo, &state]);
}

/// (c) **母集団が 0 の出力は撃ち直さない**（回数 1・INCONCLUSIVE・理由の字面は今までどおり）＝「読めたが規則で
/// 断った」側の pin。2 回目には正しい出力を用意してあるので、撃ち直せば PASS に化ける形——化けないことを測る。
#[test]
fn pipe_gate_lens_reread_does_not_rerun_zero_population() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let zero = findings_body(&format!(",\"findings\":\"{ZERO_FINDINGS}\",\"population\":\"files:0,lines:42\""));
    let lens = counting_lens(&state, &zero, &lens_verdict("PASS"), "");
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "母集団 0 は rc 3 のまま: {}", stdout_of(&out));
    assert_eq!(lens_calls(&state), 1, "読めたが規則で断った周は撃ち直さない");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "{pairs:?}");
    assert_eq!(
        value_of(&pairs, "evidence"),
        "lens のpopulation が 0（files:0,lines:42）＝lens は読んでいない",
        "理由の字面は 1 字も変わらない: {pairs:?}"
    );
    assert_eq!(reread_line(&out), None, "撃ち直しの行は出ない: {}", stderr_of(&out));
    clean(&[&repo, &state]);
}

/// (d) rc が非 0 で終わる lens と起動できない lens は撃ち直さない（stderr に撃ち直しの行が無い）。
///
/// rc 非 0 の側は 1 回目の出力が読めない形にしてある＝出力の形だけ見れば撃ち直す側だが、rc が先に読まれて
/// 撃ち直さない（箱の中の死と同じ列・撃ち直しで向きが変わらない）。
#[test]
fn pipe_gate_lens_reread_does_not_rerun_nonzero_rc_or_unlaunchable() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let lens = counting_lens(&state, &unreadable_population("~330"), &lens_verdict("PASS"), "exit 7");
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "rc 非 0 は rc 3: {}", stdout_of(&out));
    assert_eq!(lens_calls(&state), 1, "rc 非 0 は撃ち直さない");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "evidence"), "lens が rc 7 で終わった", "理由は rc のまま: {pairs:?}");
    assert_eq!(reread_line(&out), None, "撃ち直しの行は出ない: {}", stderr_of(&out));

    // 起動できない lens（script が無い）: 1 度も起きず（回数は前の 1 のまま）、撃ち直しもしない。
    // 起動の失敗は shell の rc≠0（値は shell により違う）として読まれる＝rc 非 0 と同じ列。
    let absent = format!("sh {}", state.join("absent-lens.sh").display());
    let out = gate_once(&repo, &state, &id, Some(&absent));
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "起動できない周は rc 3: {}", stdout_of(&out));
    assert_eq!(lens_calls(&state), 1, "起動できない lens は 1 度も数えられない（前の 1 のまま）");
    let pairs = verdict_pairs(&state, &id);
    assert!(value_of(&pairs, "evidence").starts_with("lens が rc "), "理由は起動の失敗（rc≠0）: {pairs:?}");
    assert_eq!(reread_line(&out), None, "撃ち直しの行は出ない: {}", stderr_of(&out));
    clean(&[&repo, &state]);
}

/// (e) lens が **自分で** `INCONCLUSIVE` を答えた周（集計は正しい）は撃ち直さない（回数 1）。
///
/// 3 値のうち INCONCLUSIVE だけが「読めなかった」と混ざりうる——形どおりの INCONCLUSIVE は判定であって
/// 読めなさではない。2 回目は PASS を用意してあるので、撃ち直せば通ってしまう形。
#[test]
fn pipe_gate_lens_reread_does_not_rerun_a_well_formed_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let lens = counting_lens(&state, &lens_verdict("INCONCLUSIVE"), &lens_verdict("PASS"), "");
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "lens の INCONCLUSIVE は rc 3: {}", stdout_of(&out));
    assert_eq!(lens_calls(&state), 1, "形どおりの答えは撃ち直さない");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "{pairs:?}");
    assert_eq!(value_of(&pairs, "evidence"), "fake", "理由は lens の evidence: {pairs:?}");
    assert_eq!(value_of(&pairs, "population"), FAKE_POPULATION, "集計は読めている（判定に届いた周）: {pairs:?}");
    assert_eq!(reread_line(&out), None, "撃ち直しの行は出ない: {}", stderr_of(&out));
    clean(&[&repo, &state]);
}

// ───── lens への入力の通知を rc に依らず記録に残す（`s2-07l.293`・設計 pipeline.md §21・接頭辞 `pipe_gate_notice_`） ─────
//
// 理由の 1 行は従来 stderr にしか出ず、rc 0 で終わった gate の周は呼び手が捨てると事後に読めなかった
// （`.286` の実測）。**要約の周も diff の周も**、段の記録と同じ log（`verify.stderr.log`）に 1 行残す。
// 置き場が `verify.jsonl` でないのは、record の通し番号 `n` を行数から導く読み手（land の引き継ぎ）が
// 在るためである——ここでも「record は 1 行 1 record」が保たれていることを対で測る。

/// 診断 file（`verify.stderr.log`）の全文（無ければ空）。
fn stderr_log_body(state: &Path, id: &str) -> String {
    fs::read_to_string(state.join("pipe").join(id).join("verify.stderr.log")).unwrap_or_default()
}

/// 通知の 1 行（`# lens-input=<kind> reason=<語>`）だけを拾う。
fn notice_lines(state: &Path, id: &str) -> Vec<String> {
    stderr_log_body(state, id)
        .lines()
        .filter(|line| line.starts_with("# lens-input="))
        .map(str::to_owned)
        .collect()
}

/// `verify.jsonl` の行が**全部 record である**こと（通知を混ぜていない）を測る。
///
/// 混ぜると land の引き継ぎ（`carry_gated_pass`）が行数から導く `n` が飛ぶ＝record の通し番号が壊れる。
fn assert_verify_log_is_all_records(state: &Path, id: &str) {
    let log = verify_log(state, id);
    assert_eq!(
        verify_rows(state, id).len(),
        log.lines().count(),
        "`verify.jsonl` の行は全部 record（通知は混ざらない）: {log}"
    );
    assert!(!log.contains("lens-input="), "通知は record の log に書かない: {log}");
}

/// (a) 純移動の便（lens の入力が**要約**）も通知が残る: `verify.stderr.log` に
/// `# lens-input=summary reason=-` の 1 行だけ（段の見出しは無い＝全行が緑）・**stderr は従来どおり空**
/// （`move_proof` の語彙は触らない）・`verify.jsonl` は record だけのまま。
///
/// 理由の語が `-` なのは「純移動でない理由が無い」であって 0 でも空でもない（C10）。
/// base は診断 file を 1 度も作らない＝RED。
#[test]
fn pipe_gate_notice_summary_round_keeps_a_dash_reason() {
    let (repo, state, id) = move_run(&[("lib.rs", MOVE_BASE_LIB)], &move_head());
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    assert_eq!(token_of(&stdout_of(&out), "lens-input="), "summary", "前提: 要約の周: {}", stdout_of(&out));
    assert_eq!(
        notice_lines(&state, &id),
        vec!["# lens-input=summary reason=-".to_owned()],
        "要約の周も rc 0 でも 1 行残る: {}",
        stderr_log_body(&state, &id)
    );
    assert!(!stderr_log_body(&state, &id).contains("## "), "通知は段の見出しではない: {}", stderr_log_body(&state, &id));
    assert_eq!(stderr_of(&out), "", "要約の周の stderr は従来どおり 1 行も出さない");
    assert_verify_log_is_all_records(&state, &id);
    clean(&[&repo, &state]);
}

/// (b) 純移動でない便（lens の入力が **diff**）は理由の語が載る: 緑の周（rc 0）も
/// `# lens-input=diff reason=items-differ` が残り、**stderr の 1 行は従来どおり**（両面に同じ理由）。
///
/// 対（`rc に依らず`）: verify の行が赤い便（rc 1）でも同じ通知が残り、赤い行の見出しと同居する
/// ——通知を rc 0 の周だけ書く実装・赤い周だけ書く実装のどちらも落ちる。base はどちらも残さない＝RED。
#[test]
fn pipe_gate_notice_diff_round_names_the_reason() {
    let changed = MOVE_HEAD_BETA.replace("    3\n", "    4\n");
    let (repo, state, id) = move_run(
        &[("lib.rs", MOVE_BASE_LIB)],
        &[("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", MOVE_HEAD_ALPHA), ("beta.rs", &changed)],
    );
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    assert_eq!(token_of(&stdout_of(&out), "lens-input="), "diff", "前提: diff の周: {}", stdout_of(&out));
    assert_eq!(
        notice_lines(&state, &id),
        vec!["# lens-input=diff reason=items-differ".to_owned()],
        "理由の語が記録に残る: {}",
        stderr_log_body(&state, &id)
    );
    assert_eq!(
        stderr_of(&out).trim_end(),
        "pipe: lens-input=diff reason=items-differ",
        "呼び手の stderr の 1 行は従来どおり"
    );
    assert_verify_log_is_all_records(&state, &id);
    clean(&[&repo, &state]);

    // 対: 赤い verify 行を持つ便（rc 1）でも通知は残り、赤い行の見出しと同居する。
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-red.sh"]"#]);
    let red = implemented(&repo, &state, &path);
    let out = gate_once(&repo, &state, &red, Some(&fake_lens(&state.join("lens-red"), &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1: {}", stderr_of(&out));
    let notices = notice_lines(&state, &red);
    assert_eq!(notices.len(), 1, "赤い周も通知は 1 行: {}", stderr_log_body(&state, &red));
    assert!(
        notices.first().is_some_and(|line| line.starts_with("# lens-input=diff reason=")),
        "赤い周の入力も diff（理由つき）: {notices:?}"
    );
    assert!(stderr_log_body(&state, &red).contains("## "), "赤い行の見出しと同居する: {}", stderr_log_body(&state, &red));
    assert_verify_log_is_all_records(&state, &red);
    clean(&[&repo, &state]);
}

// ---- 器の健康の遮断器（設計 gate-cost.md §32・契約表の行 x・接頭辞 `pipe_gate_health_`）------------------------
//
// `--rules` の fixture で倍率を振る: 倍率 0（＝走行可能 1 でも「混んでいる」）と `gate.slot_wait_s = 1` の便は
// verify の行を 1 本も撃たずに INCONCLUSIVE、倍率を十分大きく取った便は従来どおり全段を撃つ。呼出回数は
// `verify-count.sh` の印（[`detection_calls`]）で数える。

/// 遮断器の歯の便を Implemented まで通す（共通 verify と契約 verify は印を 1 行ずつ足す stub）。
fn health_run() -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-count.sh common"]"#);
    let design = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-count.sh contract"]"#]);
    let id = implemented(&repo, &state, &design);
    assert!(detection_calls(&repo).is_empty(), "fixture: gate の前は印が無い");
    (repo, state, id)
}

/// 倍率 `per_core`（走行可能と待ちの両方）の rules fixture で gate を 1 回撃つ（待ちの上限は [`SLOT_WAIT_S`]）。
fn health_gate(repo: &Path, state: &Path, id: &str, per_core: u64) -> Output {
    let slots = SlotFixture { runnable_per_core: per_core, blocked_per_core: per_core, ..default_slots() };
    let rules = write_rules_full(state, &format!("rules-health-{per_core}.toml"), (1, 1_000_000), FOLLOW_RETRIES, slots);
    gate_with_rules(repo, state, id, &rules, &fake_lens(&state.join("lens-ran"), &lens_verdict("PASS")))
}

/// (a) 倍率 0 の便は verify の行が 1 本も撃たれず（印が空）verdict が INCONCLUSIVE で、撃たなかった行の record に
/// 閉じた印 `host=busy` が載る（write-set 照合は process を持たないので印を持たない）。lens は起こさない。
#[test]
fn pipe_gate_health_busy_host_fires_no_line_and_is_inconclusive() {
    let (repo, state, id) = health_run();
    let out = health_gate(&repo, &state, &id, 0);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_INCONCLUSIVE)),
        "待ちの上限を超えた周は rc 3: {} / {}",
        stdout_of(&out),
        stderr_of(&out)
    );
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    assert!(detection_calls(&repo).is_empty(), "verify の行は 1 本も撃たれない: {:?}", detection_calls(&repo));
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "contract"], "record は段ごとに残る: {rows:?}");
    assert_eq!(row_value(&rows, 1, "host"), "", "write-set 照合は印を持たない: {rows:?}");
    assert_eq!(row_value(&rows, 2, "host"), "busy", "撃たなかった行に閉じた印: {rows:?}");
    assert_eq!(row_value(&rows, 3, "host"), "busy", "以後の行も待たずに閉じる: {rows:?}");
    assert_eq!(row_value(&rows, 2, "secs"), "", "撃っていない行は秒を持たない: {rows:?}");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE");
    assert_eq!(value_of(&pairs, "verify_red"), "0", "撃っていない行は赤に数えない");
    assert!(value_of(&pairs, "evidence").contains("n=2"), "理由は最初に閉じた行を名指す: {pairs:?}");
    assert!(!state.join("lens-ran").exists(), "測れなかった周は lens を起こさない");
    clean(&[&repo, &state]);
}

/// (b) 倍率を十分大きく取った便は従来どおり全段を撃って PASS で終わり、record に印が載らない。
#[test]
fn pipe_gate_health_calm_host_fires_every_line_and_passes_without_mark() {
    let (repo, state, id) = health_run();
    let out = health_gate(&repo, &state, &id, HEALTH_PER_CORE_OPEN);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "空いた host は PASS: {} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(detection_calls(&repo), ["common".to_owned(), "contract".to_owned()], "全段を 1 回ずつ撃つ");
    let log = fs::read_to_string(state.join("pipe").join(&id).join("verify.jsonl")).unwrap_or_default();
    assert!(!log.contains("\"host\""), "空いていた周は印を欠く: {log}");
    assert!(state.join("lens-ran").exists(), "測れた周は lens を起こす");
    clean(&[&repo, &state]);
}

/// (c) (a) の便は `Gated` に留まり（`Failed` へ終端しない）、同じ便を空いた host で撃ち直すと PASS に着く。
#[test]
fn pipe_gate_health_busy_run_stays_gated_and_can_be_regated() {
    let (repo, state, id) = health_run();
    let busy = health_gate(&repo, &state, &id, 0);
    assert_eq!(busy.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "1 周目は INCONCLUSIVE: {}", stderr_of(&busy));
    assert_eq!(gated_details(&state, &id), ["verdict:INCONCLUSIVE".to_owned()], "Gated に留まる");
    let failed = events(&state).into_iter().filter(|event| event.run == id && event.stage == Some(Stage::Failed)).count();
    assert_eq!(failed, 0, "FAIL で終端しない（Failed の event が無い）");
    let calm = health_gate(&repo, &state, &id, HEALTH_PER_CORE_OPEN);
    assert_eq!(calm.status.code(), Some(i32::from(RC_OK)), "撃ち直せて PASS: {} / {}", stdout_of(&calm), stderr_of(&calm));
    assert_eq!(detection_calls(&repo), ["common".to_owned(), "contract".to_owned()], "撃ち直しの周で初めて撃つ");
    assert_eq!(
        gated_details(&state, &id),
        ["verdict:INCONCLUSIVE".to_owned(), "verdict:PASS".to_owned()],
        "測り直しの履歴"
    );
    clean(&[&repo, &state]);
}

// ── 検出線の穴 `{teeth}`（設計 gate-cost.md §34 約束 4 / 5・行 aa） ────────────────────────────

/// 検出線の stub（受けた語を stdout の 1 行に出す＝撃たれた側が置換後の値を record の `line=` へ運ぶ）。
const TEETH_STUB: &str = "verify-teeth.sh";

/// `{teeth}` を持つ検出線の宣言（4 つ目の穴だけを持つ stub の行）。
const TEETH_DETECTION: &str = r#"["sh verify-teeth.sh {teeth}"]"#;

/// `{teeth}` を持つ検出線の宣言と、nextest の歯の置き場（`crates/toy/src/lib.rs` の `foo_` と `crates/toy/tests/e2e.rs`
/// の `bar_`）を置いた toy repo で、契約の verify 行を `verify` にした便を gate まで通す（rc は測らない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn teeth_gate(verify: &str) -> (PathBuf, PathBuf, String, Output) {
    let (repo, state) = repo_with_state();
    fs::write(repo.join(TEETH_STUB), "printf 'teeth=%s\\n' \"$1\"\nexit 0\n").expect("stub を書ける");
    for (path, body) in [
        ("crates/toy/src/lib.rs", "#[cfg(test)]\nmod tests {\n    #[test]\n    fn foo_one() {}\n}\n"),
        ("crates/toy/tests/e2e.rs", "#[test]\nfn bar_one() {}\n"),
    ] {
        let file = repo.join(path);
        fs::create_dir_all(file.parent().expect("親 dir が在る")).expect("dir を作れる");
        fs::write(&file, body).expect("歯の置き場を書ける");
    }
    write_vessel(&repo, r#"["git", "sh", "cargo"]"#, r#"["sh verify-ok.sh"]"#);
    let path = repo.join(".vessel.toml");
    let body = fs::read_to_string(&path).expect("宣言を読める");
    fs::write(&path, format!("{body}detection-verify = {TEETH_DETECTION}\n")).expect("宣言を書ける");
    git(&repo, &["add", "-f", ".vessel.toml", TEETH_STUB, "crates"]);
    git(&repo, &["commit", "-q", "-m", "vessel-teeth"]);
    let design = write_contract(
        &repo,
        &["verify", "write-set"],
        &[
            r#"write-set = ["src/lib.rs", "crates/toy/src/lib.rs", "crates/toy/tests/e2e.rs"]"#,
            &format!("verify = {verify}"),
        ],
    );
    let id = implemented(&repo, &state, &design);
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    (repo, state, id, out)
}

/// 撃たれた検出線の record に契約の verify 行の filter 語が宣言順に `,` で結ばれて載り（cmd と撃たれた側の stdout の
/// 両方）、filter を持たない行は飛ばされ、`{teeth}` の字面は record のどこにも残らない（語は環境変数でなく行の引数で
/// 渡る＝stub は `$1` しか読まない）。
#[test]
fn pipe_gate_teeth_detection_record_carries_the_contract_words_joined_by_comma() {
    let (repo, state, id, _out) = teeth_gate(
        r#"["cargo nextest run -p toy --lib --no-tests=fail foo_", "sh verify-ok.sh", "cargo nextest run -p toy --test e2e --no-tests=fail bar_"]"#,
    );
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows).get(2).map(String::as_str), Some("detection"), "③ が検出線: {rows:?}");
    assert_eq!(row_value(&rows, 3, "cmd"), "sh verify-teeth.sh foo_,bar_", "語が宣言順に , で結ばれる: {rows:?}");
    assert_eq!(row_value(&rows, 3, "rc"), "0", "stub は完走した");
    assert_eq!(row_value(&rows, 3, "line"), "teeth=foo_,bar_", "撃たれた側が受けた値: {rows:?}");
    let log = verify_log(&state, &id);
    assert!(!log.contains("{teeth}"), "穴の字面が残らない: {log}");
    clean(&[&repo, &state]);
}

/// 契約の verify 行が filter 語を 1 つも持たない便は `-` が置かれる（空文字で引数を欠かせない・`{teeth}` は残らない）。
#[test]
fn pipe_gate_teeth_zero_words_fill_a_dash() {
    let (repo, state, id, out) = teeth_gate(r#"["sh verify-ok.sh"]"#);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "cmd"), "sh verify-teeth.sh -", "0 本は -: {rows:?}");
    assert_eq!(row_value(&rows, 3, "line"), "teeth=-", "撃たれた側が受けた値: {rows:?}");
    assert!(!verify_log(&state, &id).contains("{teeth}"), "穴の字面が残らない");
    clean(&[&repo, &state]);
}

// ---- 契約が名指した的を撃つ（設計 gate-cost.md §16 (3)・行 g・`s2-07l.341`・接頭辞 `pipe_gate_targets_`）----
//
// 契約が `targets` を持つ便は、gate が的の列を run dir の file に置き、検出線の行の末尾に `--targets <file>` を足して
// 撃つ（的を絞った口）。持たない便は写しの行を 1 字も変えない（diff の追加行を母集団にする従来の経路）。
// 検出線は deny ではないので、どちらの周も verdict は検出線の中身で動かない。

/// 偽の検出線の名（宣言の `detection-verify` に置く stub）。
const AIM_STUB: &str = "verify-aim.sh";

/// 偽の検出線: `--targets <file>` を受けた周は file の的の本数を母集団にし、**全部を生存**として 5 値の行を出す
/// （生存が在っても verdict が動かないことを測る）。受けない周は diff の追加行を母集団にした従来の形の行を出す。
const AIM_STUB_BODY: &str = r#"if [ "$1" = "--targets" ]; then
  n="$(grep -c . "$2")"
  printf 'mutants-diff: total=%s caught=0 missed=%s unviable=0 timeout=0 absent=0 scope=x teeth=- population=targets\n' "$n" "$n"
else
  printf 'mutants-diff: total=7 caught=6 missed=1 unviable=0 timeout=0 scope=x teeth=-\n'
fi
exit 0
"#;

/// 的 3 本（桁つきの一覧の形を 1 本含む）。
const AIM_TARGETS: [&str; 3] =
    ["src/lib.rs:1:replace seed -> bool with true", "src/lib.rs:1:5: replace seed with ()", "src/other.rs:9:replace x with 0"];

/// 偽の検出線を宣言した便を Implemented まで進め、PASS の lens で 1 回 gate する（`targets` は行に足す欄・空 = 欄なし）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn aim_gate(targets: &[&str]) -> (PathBuf, PathBuf, String, Output) {
    let (repo, state) = repo_with_state();
    fs::write(repo.join(AIM_STUB), AIM_STUB_BODY).expect("stub を書ける");
    write_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-ok.sh"]"#);
    let path = repo.join(".vessel.toml");
    let body = fs::read_to_string(&path).expect("宣言を読める");
    fs::write(&path, format!("{body}detection-verify = [\"sh {AIM_STUB}\"]\n")).expect("宣言を書ける");
    git(&repo, &["add", "-f", AIM_STUB, ".vessel.toml"]);
    git(&repo, &["commit", "-q", "-m", "vessel-aim"]);
    let quoted: Vec<String> = targets.iter().map(|target| format!("\"{target}\"")).collect();
    let field = format!("targets = [{}]", quoted.join(", "));
    let mut add = vec![r#"verify = ["sh verify-ok.sh"]"#];
    if !targets.is_empty() {
        add.push(&field);
    }
    let design = write_contract(&repo, &["verify"], &add);
    let id = implemented(&repo, &state, &design);
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"))));
    (repo, state, id, out)
}

/// (c) 欄を持つ便は的を絞った口で撃つ: 検出線の record の cmd は写しの行の末尾に `--targets <run dir の file>` を持ち、
/// file は的を 1 行 1 本・逐語で持ち、撃たれた側の母集団 = 的の本数（3）。的が全部生存でも verdict は PASS のまま。
#[test]
fn pipe_gate_targets_row_fires_the_aimed_line_with_the_targets_as_its_population() {
    let (repo, state, id, out) = aim_gate(&AIM_TARGETS);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "検出線は deny ではない＝PASS: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=PASS"), "{}", stdout_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "detection", "contract"], "母集団 4 record: {rows:?}");
    let file = run_dir(&state, &id).join("targets");
    assert_eq!(
        row_value(&rows, 3, "cmd"),
        format!("sh {AIM_STUB} --targets {}", file.display()),
        "写しの行の末尾に的の file: {rows:?}"
    );
    let listed = fs::read_to_string(&file).unwrap_or_default();
    assert_eq!(listed, format!("{}\n", AIM_TARGETS.join("\n")), "的は 1 行 1 本・逐語");
    let line = row_value(&rows, 3, "line");
    assert_eq!(token_of(&line, "total="), "3", "母集団 = 的の本数: {line}");
    assert_eq!(token_of(&line, "missed="), "3", "的は全部生存: {line}");
    assert_eq!(token_of(&line, "population="), "targets", "{line}");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "0", "生存は赤に数えない");
    clean(&[&repo, &state]);
}

/// (c) 欄を持たない便は従来の経路のまま: 検出線の cmd は写しの行と 1 字も違わず（`--targets` を持たない）、run dir に
/// 的の file を置かず、母集団は diff の追加行の側（stub の従来の行）。生存 1 でも verdict は PASS のまま。
#[test]
fn pipe_gate_targets_row_without_targets_keeps_the_diff_population() {
    let (repo, state, id, out) = aim_gate(&[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=PASS"), "{}", stdout_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "cmd"), format!("sh {AIM_STUB}"), "写しの行のまま: {rows:?}");
    assert!(!run_dir(&state, &id).join("targets").exists(), "的の file を置かない");
    let line = row_value(&rows, 3, "line");
    assert_eq!(token_of(&line, "total="), "7", "diff の追加行の母集団: {line}");
    assert!(!line.contains("population="), "従来の形の行: {line}");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "0", "赤は 0");
    clean(&[&repo, &state]);
}

// ---- 純移動と証明された行を検出線の母集団から外す（設計 gate-cost.md §14・行 e・`s2-07l.292`・接頭辞
// `pipe_gate_detection_pure_move_`）----
//
// lens の入力が要約になる便は、gate が動いた item の区間の `+` 行を落とした diff を run dir の file に組み、検出線の行の
// 末尾に `--diff <file>` を足して撃ち、record に `pure-move=<落とした本数>` を残す。要約にならない便は行を 1 字も変えない。

/// 偽の検出線の名（宣言の `detection-verify` に置く stub）。
const POP_STUB: &str = "verify-population.sh";

/// 偽の検出線: `--diff <file>` を受けた周は file の `+` 行（`+++` の見出しを除く）の本数を母集団にした行を、受けない周は
/// 従来の形の行（total=7）を出す。どちらも rc 0（測れた周）。
const POP_STUB_BODY: &str = r#"if [ "$1" = "--diff" ]; then
  n="$(grep -v '^+++ ' "$2" | grep -c '^+')"
  printf 'mutants-diff: total=%s caught=%s missed=0 unviable=0 timeout=0 scope=x teeth=- population=diff\n' "$n" "$n"
else
  printf 'mutants-diff: total=7 caught=6 missed=1 unviable=0 timeout=0 scope=x teeth=-\n'
fi
exit 0
"#;

/// 1 本だけ動く純移動の base（`fn two` が `src/alpha.rs` へ移る・残差は空行の `-` だけ）。
const POP_BASE_LIB: &str = "fn one() -> u8 {\n    1\n}\n\nfn two() -> u8 {\n    2\n}\n";

/// 偽の検出線を宣言した toy repo で純移動の fixture を Implemented まで進め、PASS の lens で 1 回 gate する。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn pure_move_gate(base: &[(&str, &str)], head: &[(&str, &str)]) -> (PathBuf, PathBuf, String, Output) {
    let (repo, state) = repo_with_state();
    fs::write(repo.join(POP_STUB), POP_STUB_BODY).expect("stub を書ける");
    write_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-ok.sh"]"#);
    let path = repo.join(".vessel.toml");
    let body = fs::read_to_string(&path).expect("宣言を読める");
    fs::write(&path, format!("{body}detection-verify = [\"sh {POP_STUB}\"]\n")).expect("宣言を書ける");
    git(&repo, &["add", "-f", POP_STUB, ".vessel.toml"]);
    git(&repo, &["commit", "-q", "-m", "vessel-population"]);
    let id = move_run_in(&repo, &state, base, head);
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"))));
    (repo, state, id, out)
}

/// run dir の母集団の diff の path。
fn population_path(state: &Path, id: &str) -> PathBuf {
    run_dir(state, id).join("population.diff")
}

/// diff の `+` 行（`+++` の見出しを除く・`+` を付けたまま）。
fn added_lines(diff: &str) -> Vec<String> {
    diff.lines().filter(|line| line.starts_with('+') && !line.starts_with("+++ ")).map(str::to_owned).collect()
}

/// 便の worktree の生の diff（runner の commit は 1 本＝`HEAD~1..HEAD`）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn raw_diff(repo: &Path, id: &str) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(worktree_of(repo, id))
        .args(["diff", "HEAD~1..HEAD"])
        .output()
        .expect("git を起動できる");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 母集団の hunk の `+` 行が、見出しの HEAD 側の行番号で worktree の file の行と逐語で一致する本数（不一致は `None`）。
///
/// 道具は HEAD 側の行番号で変異を母集団へ当てる——番号がずれた母集団は別の行を撃つ。
fn anchored(worktree: &Path, population: &str) -> Option<usize> {
    let (mut lines, mut at, mut count): (Vec<String>, usize, usize) = (Vec::new(), 0, 0);
    for line in population.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            lines = fs::read_to_string(worktree.join(path)).ok()?.lines().map(str::to_owned).collect();
        } else if let Some(rest) = line.strip_prefix("@@ ") {
            let new = rest.split_whitespace().nth(1)?.strip_prefix('+')?;
            at = new.split(',').next()?.parse().ok()?;
        } else if let Some(text) = line.strip_prefix('+') {
            (lines.get(at.checked_sub(1)?)? == text).then_some(())?;
            at += 1;
            count += 1;
        }
    }
    Some(count)
}

/// (a) 純移動だけの便: 母集団の diff は `+` 行 0 本（動いた `fn two` の 3 行を落とす）・検出線の cmd は写しの行の末尾に
/// `--diff <run dir の file>`・撃たれた側の母集団は 0・record は `pure-move=3` を持ち rc 0（赤にも測定未了にもならない）・PASS。
#[test]
fn pipe_gate_detection_pure_move_only_move_has_an_empty_population_and_is_marked() {
    let head = [("lib.rs", "fn one() -> u8 {\n    1\n}\n"), ("alpha.rs", "fn two() -> u8 {\n    2\n}\n")];
    let (repo, state, id, out) = pure_move_gate(&[("lib.rs", POP_BASE_LIB)], &head);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=PASS"), "{}", stdout_of(&out));
    assert_eq!(token_of(&stdout_of(&out), "lens-input="), "summary", "前提: 純移動: {}", stdout_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "detection", "contract"], "母集団 4 record: {rows:?}");
    let file = population_path(&state, &id);
    assert_eq!(row_value(&rows, 3, "cmd"), format!("sh {POP_STUB} --diff {}", file.display()), "{rows:?}");
    let population = fs::read_to_string(&file).unwrap_or_else(|_| "unreadable".to_owned());
    assert_eq!(population, "", "残る `+` 行 0 本の母集団は空");
    assert_eq!(added_lines(&raw_diff(&repo, &id)).len(), 3, "前提: 生の diff は `+` 3 本");
    assert_eq!(row_value(&rows, 3, "rc"), "0", "測れた周: {rows:?}");
    assert_eq!(token_of(&row_value(&rows, 3, "line"), "total="), "0", "母集団 0");
    assert_eq!(row_value(&rows, 3, "pure-move"), "3", "落とした `+` 行の本数: {rows:?}");
    for n in [1, 2, 4] {
        assert_eq!(row_value(&rows, n, "pure-move"), "", "検出線の外は持たない（n={n}）: {rows:?}");
    }
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "0", "赤に数えない");
    clean(&[&repo, &state]);
}

/// (b) 移動と移動でない追加行が混ざる便（`s2-07l.261` 型の fixture）: 母集団には `mod` / `use` の宣言・module doc・同じ
/// file に残った item の可視性の変更が残り、動いた item（`fn two` / `struct Pair` / `fn three`・doc と属性ごと 11 行）の
/// 行は入らない。残した本数 + 落とした本数 = 生の diff の `+` 行・hunk の行番号は worktree の行と一致する。
#[test]
fn pipe_gate_detection_pure_move_mixed_run_keeps_only_the_lines_outside_moved_items() {
    let (repo, state, id, out) = pure_move_gate(&[("lib.rs", MOVE_BASE_LIB)], &move_head());
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    assert_eq!(token_of(&stdout_of(&out), "lens-input="), "summary", "前提: 純移動: {}", stdout_of(&out));
    let population = fs::read_to_string(population_path(&state, &id)).unwrap_or_default();
    let kept = added_lines(&population);
    for want in ["+mod alpha;", "+mod beta;", "+use super::one;", "+//! alpha.", "+pub(crate) fn one() -> u8 {", "+// flip-check: moved s2-07l.261"] {
        assert!(kept.iter().any(|line| line == want), "移動でない追加行は残る: {want}: {population}");
    }
    for gone in ["+pub(super) fn two() -> u8 {", "+/// helper two.", "+    2", "+pub struct Pair {", "+#[derive(Debug)]", "+pub(super) fn three() -> u8 {", "+    3"] {
        assert!(!kept.iter().any(|line| line == gone), "動いた item の行は入らない: {gone}: {population}");
    }
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "pure-move"), "11", "落とした本数: {rows:?}");
    let raw = added_lines(&raw_diff(&repo, &id)).len();
    assert_eq!(kept.len() + 11, raw, "残した + 落とした = 生の `+` 行: {population}");
    assert_eq!(anchored(&worktree_of(&repo, &id), &population), Some(kept.len()), "行番号は HEAD の行: {population}");
    assert_eq!(token_of(&row_value(&rows, 3, "line"), "total="), kept.len().to_string(), "撃たれた側の母集団 = 残した本数");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "0", "赤は 0");
    clean(&[&repo, &state]);
}

/// 要約にならない便の共通 assert: 検出線の cmd は写しの行のまま（`--diff` を持たない）・母集団の file を置かない・
/// `pure-move` を持たない・母集団は従来の側（stub の total=7）。
fn assert_keeps_the_git_diff_population(base: &[(&str, &str)], head: &[(&str, &str)], why: &str) {
    let (repo, state, id, out) = pure_move_gate(base, head);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{why}: PASS: {}", stderr_of(&out));
    assert_eq!(token_of(&stdout_of(&out), "lens-input="), "diff", "{why}: 前提: 要約にならない: {}", stdout_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "cmd"), format!("sh {POP_STUB}"), "{why}: 写しの行のまま: {rows:?}");
    assert!(!population_path(&state, &id).exists(), "{why}: 母集団の file を置かない");
    assert_eq!(row_value(&rows, 3, "pure-move"), "", "{why}: 印を持たない: {rows:?}");
    assert_eq!(token_of(&row_value(&rows, 3, "line"), "total="), "7", "{why}: 従来の母集団");
    clean(&[&repo, &state]);
}

/// (c) 移動でない追加行だけの便（`mod` 宣言の追加・可視性の変更＝移動 0）は従来どおり撃つ（全ての追加行が母集団）。
#[test]
fn pipe_gate_detection_pure_move_declarations_only_run_keeps_every_added_line() {
    let base = "fn one() -> u8 {\n    1\n}\n";
    let head = "mod gen;\n\npub fn one() -> u8 {\n    1\n}\n";
    assert_keeps_the_git_diff_population(&[("lib.rs", base)], &[("lib.rs", head)], "nothing-moved");
}

/// (d) `LensInput::Diff` の便（本文を 1 行変えた移動＝`items-differ`）は全ての追加行が母集団に入る（従来の極性）。
#[test]
fn pipe_gate_detection_pure_move_diff_input_keeps_every_added_line() {
    let changed = MOVE_HEAD_BETA.replace("    3\n", "    4\n");
    let head = [("lib.rs", MOVE_HEAD_LIB), ("alpha.rs", MOVE_HEAD_ALPHA), ("beta.rs", changed.as_str())];
    assert_keeps_the_git_diff_population(&[("lib.rs", MOVE_BASE_LIB)], &head, "items-differ");
}
