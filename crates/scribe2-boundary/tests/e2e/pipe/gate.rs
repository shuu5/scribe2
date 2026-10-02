// flip-check: moved s2-07l.264
//! gate の歯: `pipe_gate_` / `pipe_detection_`（検出線）/ `pipe_confine_`（封じ込め）/ `pipe_slots_`（受付札）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く（歯の本文は移しただけ・`s2-07l.264`）。
//!
//! 歯の一部は族ごとの子 module に置く（設計 docs/design/carry-prep.md §10 行 m・`s2-07l.685`）: `confine`（接頭辞
//! `pipe_confine_` / `pipe_slots_`）・`detection`（接頭辞 `pipe_detection_` / `pipe_landed_`）・`pure_move`（接頭辞
//! `pipe_gate_move_` / `pipe_gate_elide_`）。この file には共有の helper と const・外形 snapshot の歯（snapshot 名が
//! module path を含むので動かさない）・他の族の歯だけを残す。

mod confine;
mod detection;
mod promised;
mod pure_move;
// flip-check: moved s2-07l.685
// flip-check: retroactive s2-07l.736.23

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
    let out = spawn_without_gate(&repo, &state, &id, &runner);
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
        seen.contains(&(EventKind::RunStage, Some(Stage::Gated), Some(format!("verdict:PASS,rules:{}", rules_source(&repo, None))))),
        "`Gated` の detail は verdict と読んだ manifest の出所だけ: {seen:?}"
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
        vec![format!("verdict:INCONCLUSIVE,rules:{}", rules_source(&repo, None)), format!("verdict:PASS,rules:{}", rules_source(&repo, None))],
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

/// (h) 埋め込みの manifest で撃った gate の `Gated` の detail は `verdict:PASS,rules:embedded`（設計 limit-permit.md §17 約束 9）で、
/// 判定・rc・stdout の判定行・`verdict.json` は変わらない（出所は event の detail だけに載る）。base は `rules:` を足さない → RED。
#[test]
fn pipe_gate_rules_source_embedded_run_records_embedded() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let lens = fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "判定は変わらない: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=PASS"), "stdout の判定行は不変: {}", stdout_of(&out));
    assert_eq!(gated_details(&state, &id), ["verdict:PASS,rules:embedded".to_owned()], "埋め込みは embedded");
    let pairs = verdict_pairs(&state, &id);
    assert!(!pairs.iter().any(|(key, _)| key.contains("rules")), "verdict.json に出所を書かない: {pairs:?}");
    clean(&[&repo, &state]);
}

/// (i) cap の狭い fixture の INCONCLUSIVE と、広い fixture の撃ち直しの PASS の 2 件の detail が、それぞれの file の blob id（歯が
/// `git hash-object --no-filters` で測る）を持ち、2 つは違う（`--rules` の file ごとに出所が分かれる）。
#[test]
fn pipe_gate_rules_source_names_each_rules_file_by_its_blob_id() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let lens = fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"));
    let tight = write_rules(&repo, "tight.toml", 1, 1);
    let first = gate_with_rules(&repo, &state, &id, &tight, &lens);
    assert_eq!(first.status.code(), Some(3), "cap 超過の rc は 3: {}", stderr_of(&first));
    let bytes: u64 = value_of(&verdict_pairs(&state, &id), "diff_bytes").parse().unwrap_or(0);
    let relaxed = write_rules(&repo, "relaxed.toml", 1, bytes);
    let second = gate_with_rules(&repo, &state, &id, &relaxed, &lens);
    assert_eq!(second.status.code(), Some(i32::from(RC_OK)), "cap を緩めた撃ち直しは通る: {}", stderr_of(&second));
    let (narrow, wide) = (rules_source(&repo, Some(&tight.display().to_string())), rules_source(&repo, Some(&relaxed.display().to_string())));
    assert_ne!(narrow, wide, "2 つの file の blob id は違う");
    assert!(narrow.len() >= 40 && narrow.chars().all(|found| found.is_ascii_hexdigit()), "歯が測った blob id は 16 進: {narrow}");
    assert_eq!(
        gated_details(&state, &id),
        [format!("verdict:INCONCLUSIVE,rules:{narrow}"), format!("verdict:PASS,rules:{wide}")],
        "detail は file ごとの blob id を運ぶ"
    );
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

/// 判定 FAIL の `Gated` で止まった便を 1 本置く（偽 lens の FAIL・`(repo, state, 便 id)`）。
fn gated_fail_run() -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let failing = fake_lens(&state.join("lens-fail"), &lens_verdict("FAIL"));
    let first = gate_once(&repo, &state, &id, Some(&failing));
    assert_eq!(first.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1");
    (repo, state, id)
}

/// `pipe regate` を 1 回撃つ（rc を assert しない形）。
fn regate_once(repo: &Path, state: &Path, id: &str, reason: &str) -> Output {
    run_pipe(&[
        "regate", "--run", id, "--reason", reason, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ])
}

/// 形 1 / 形 5（設計 pipeline.md §49・行 ar）: 判定 FAIL の `Gated` の便に `pipe regate` を撃つと rc 0 で
/// `regate: run=<id> from=Gated to=Implemented` の 1 行が出て、段が `Implemented` に戻り、worktree の path と HEAD
/// は変わらない。戻った便は同じ worktree で gate をもう 1 周撃てる。2 度目は断って何も書かない。段の種別 11 個と
/// event の種別 19 個は増えない（種別を足さずに `RunStage` の detail で裁定を運ぶ）。
#[test]
fn pipe_regate_returns_gated_fail_to_implemented_on_the_same_worktree() {
    let (repo, state, id) = gated_fail_run();
    let worktree = worktree_of(&repo, &id);
    let head = || crate::git_out(&worktree, &["rev-parse", "HEAD"]);
    let head_before = head();
    let before = event_count(&state);
    let words = "裁定: 検出線の同名衝突で落ちた（契約の赤でない）";
    let out = regate_once(&repo, &state, &id, words);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim(), format!("regate: run={id} from=Gated to=Implemented"));
    assert_eq!(event_count(&state), before + 1, "記帳は 1 件");
    let current = vessel::pipe::current(&state).expect("置き場を読める");
    let run = current.runs.get(&id).expect("便が在る");
    assert_eq!(run.stage, Stage::Implemented, "段は Implemented に戻る");
    assert_eq!(run.detail.as_deref(), Some(format!("regate:{words}").as_str()), "逐語をそのまま");
    assert!(worktree.is_dir(), "worktree の path は同じ");
    assert_eq!(head(), head_before, "worktree の HEAD は動かない");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "FAIL", "判定の file は書き換えない");

    let again = regate_once(&repo, &state, &id, "もう一度");
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "2 度目は断る");
    assert_eq!(event_count(&state), before + 1, "断った周は何も書かない");
    let passing = fake_lens(&state.join("lens-pass"), &lens_verdict("PASS"));
    let regated = gate_once(&repo, &state, &id, Some(&passing));
    assert_eq!(regated.status.code(), Some(i32::from(RC_OK)), "同じ worktree で gate をもう 1 周: {}", stderr_of(&regated));
    assert_eq!(head(), head_before, "再 gate も同じ worktree の同じ commit");
    // event の種別は account-lifecycle.md §19 形 3 の群の逼迫の通知で 19 → 20・§20 形 5 / 6 の群の移動の 3 種で 20 → 23・
    // §24 形 4 の登録 row の退役で 23 → 24・fleet-event-log.md §12 の案件の一生の 5 kind で 24 → 29・dispatcher.md §41 の MemoJudged で
    // 29 → 30（regate は種別を足さない）。
    assert_eq!((vessel::fleet::STAGES.len(), vessel::fleet::KINDS.len()), (11, 30), "段と event の種別は増えない");
    clean(&[&repo, &state]);
}

/// 形 1 / 形 6（設計 pipeline.md §52・行 au）: PASS の `Gated` の便の後に main が進んだ周、`pipe follow` を撃つと
/// rc 0 で `follow: run=<id> rebase=<base>..<main>` の 1 行が出て、段が `Implemented` に戻り、木の base が main の
/// 先端になる。main の sha は動かず、記帳は 1 件で、段の種別 11 個と event の種別 30 個（account-lifecycle.md §19 形 3 の
/// 群の逼迫の通知で 19 → 20・§20 形 5 / 6 の群の移動の 3 種で 20 → 23・§24 形 4 の登録 row の退役で 23 → 24・fleet-event-log.md
/// §12 の案件の一生の 5 kind で 24 → 29・dispatcher.md §41 の MemoJudged で 29 → 30）は増えない。
#[test]
fn pipe_follow_step_moves_gated_tree_onto_main_and_returns_to_implemented() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &state.join("lens-ran"));
    fs::write(repo.join("moved.txt"), "moved\n").expect("別便の変更を書ける");
    git(&repo, &["add", "moved.txt"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(base, moved, "fixture: main が進んだ");
    let worktree = worktree_of(&repo, &id);
    let before = event_count(&state);
    let out = run_pipe(&[
        "follow", "--run", &id, "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim(), format!("follow: run={id} rebase={base}..{moved}"), "便 id と 2 sha の 1 行");
    assert_eq!(event_count(&state), before + 1, "記帳は 1 件");
    let current = vessel::pipe::current(&state).expect("置き場を読める");
    let run = current.runs.get(&id).expect("便が在る");
    assert_eq!(run.stage, Stage::Implemented, "段は Implemented に戻る");
    assert_eq!(run.detail.as_deref(), Some(format!("rebase:{base}..{moved}").as_str()), "着地の追随と同じ字面");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert_eq!(git(&worktree, &["merge-base", "HEAD", &moved]), moved, "木の base は main の先端");
    assert_eq!((vessel::fleet::STAGES.len(), vessel::fleet::KINDS.len()), (11, 30), "段と event の種別は増えない");
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
/// 既存の歯の母集団は動かない。後ろには host の PATH でなく道具箱（[`crate::toolbox_path`]）を積む＝この口の周も台帳 client の
/// 見張りを通り、着地が台帳を閉じても host の `bd` を起こさない。
// flip-check: retroactive s2-07l.504
fn systemd_stub(state: &Path) -> String {
    let bin_dir = state.join(SYSTEMD_BIN);
    crate::write_systemd_run_stub(&bin_dir, &state.join(SCOPE_RECORDS));
    format!("{}:{}", bin_dir.display(), crate::toolbox_path(state))
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
    // spawn も行 `runner.end_gate_rounds` を持たない `--rules` の fixture で撃つ＝終わりの門は撃たれない（要約の語 unmeasured・
    // 歯の関心は gate の封じ込めと受付で、門の分の scope の記録と受付の待ちを gate の前に積まない・設計 pipeline.md §66）。
    let spawned = run_pipe_with_path(
        path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT,
          "--rules", &ceiling_rules(state)],
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

/// 終端の片付けの呼出の並び: `kill --signal=SIGKILL` の後に `reset-failed` を 1 回（設計 gate-cost.md §25）。
fn release_sequence(unit: &str) -> Vec<String> {
    vec![
        format!("--user kill --signal=SIGKILL {unit}.scope"),
        format!("--user reset-failed {unit}.scope"),
    ]
}

/// (g) 止める口の終端で、作り手の process が死んだ scope だけを畳む（設計 gate-cost.md §38 形 4 / 5・`s2-07l.452`）。
///
/// 偽 `systemctl` の一覧は active な scope を 2 本返す（1 本は生きた pid＝この歯の process、1 本は待ち終えた
/// `true` の死んだ pid を名に持つ）。kill は死んだ側にだけ 1 回（後に reset-failed が 1 回）撃たれ、生きた側には
/// 0 回で、行の末尾が `scopes=1/2` になる。base は一覧を 1 度も撃たない＝機能不在の RED。
#[test]
fn pipe_scope_reap_stop_kills_only_the_dead_creators_scope() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let mut gone = Command::new("true").spawn().expect("true を起こせる");
    let dead = gone.id();
    gone.wait().expect("true を待てる");
    let live_unit = format!("{NAME}-reap-live-contract-1-{}-0", std::process::id());
    let dead_unit = format!("{NAME}-reap-dead-contract-1-{dead}-0");
    let answer = format!(
        "case \"$2\" in list-units) printf '%s loaded active running x\\n' '{live_unit}.scope' '{dead_unit}.scope';; esac\nexit 0"
    );
    systemctl_stub(&state, &answer);
    let recorded = bin_cmd()
        .args(["fleet", "record", "--kind", "RunStage", "--run", "reap", "--bead", "b", "--stage", "Implemented"])
        .args(["--detail", "x", "--state-dir"])
        .arg(&state)
        .output()
        .expect("fleet record を撃てる");
    assert_eq!(recorded.status.code(), Some(i32::from(RC_OK)), "走行中の便を置ける: {}", stderr_of(&recorded));
    let out = run_pipe_with_path(&path, &["stop", "--run", "reap", "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc は不変: {}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out).trim(),
        "stop: run=reap seats=0 stopped=0 scopes=1/2",
        "既存の token の後に畳んだ数 / 一覧の件数"
    );
    let calls = fs::read_to_string(state.join(SYSTEMCTL_CALLS)).unwrap_or_default();
    let listed = calls.lines().filter(|line| line.starts_with("--user list-units ")).count();
    assert_eq!(listed, 1, "一覧は 1 回（母集団 {calls:?}）");
    assert_eq!(release_calls(&state, &dead_unit), release_sequence(&dead_unit), "死んだ作り手の scope は既存の片付けで 1 回");
    assert!(release_calls(&state, &live_unit).is_empty(), "生きた作り手の scope は触らない: {calls:?}");
    assert_eq!(kind_count(&state, "reap", EventKind::RunStopped), 1, "RunStopped は従来どおり 1 件");
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

/// 受付の歯の宣言: `{jobs}` の行 → `{jobs}` の無い行の順に、**撃たれた側で** slot dir を写す。
///
/// 札は行の終了で消えるので、外から gate の後に見ても「在った」ことは測れない。行の中から
/// dir の中身と札の本文を git の dir へ写す（script の本文は宣言の検査の外）。受付の待ちと死んだ札の回収は**先に撃たれる行**の
/// 受付が持つ（次の行の受付の時には回収する札が無く、塞ぐ札が消えた後は待たない・設計 gate-cost.md §45）ので、待ちと回収と縮退を測る
/// 歯が読む record（[`slot_row`]）は先に撃たれる `{jobs}` の行の側に置く。
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
    let plain = format!(
        "ls -A '{dir}' > {seen}/slots-plain 2>/dev/null\n\
         cat '{dir}'/*.slot > {seen}/slots-plain-body 2>/dev/null\nexit 0\n"
    );
    let jobs = format!(
        "printf '%s' \"$1\" > {seen}/jobs-seen\nprintf '%s' \"$2\" > {seen}/threads-seen\n\
         ls -A '{dir}' > {seen}/slots-during 2>/dev/null\n\
         cat '{dir}'/*.slot > {seen}/slots-body 2>/dev/null\nexit 0\n"
    );
    fs::write(repo.join("verify-slot-plain.sh"), plain).expect("script を書ける");
    fs::write(repo.join("verify-slot.sh"), jobs).expect("script を書ける");
    git(repo, &["add", "verify-slot-plain.sh", "verify-slot.sh"]);
    commit_vessel(repo, VESSEL_ALLOWED, &format!(r#"["{jobs_line}", "sh verify-slot-plain.sh"]"#));
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

/// 受付の歯の `{jobs}` の行の record（**`cmd` で選んでちょうど 1 件**・母集団を確かめてから読む）。
///
/// 全部の行が受付を通る（設計 gate-cost.md §45）ので `slot=` の有無では選べない。`{jobs}` の無い行の
/// `sh verify-slot-plain.sh` と字面が違う `sh verify-slot.sh` の頭で選ぶ。
fn slot_row(
    rows: &[Vec<(String, vessel::fleet::json_lite::Value)>],
) -> Vec<(String, vessel::fleet::json_lite::Value)> {
    let hits: Vec<&Vec<(String, vessel::fleet::json_lite::Value)>> =
        rows.iter().filter(|row| value_of(row, "cmd").starts_with("sh verify-slot.sh")).collect();
    assert_eq!(hits.len(), 1, "{{jobs}} の行の record はちょうど 1 件: {rows:?}");
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

/// 待ちが解ける歯の待ちの上限（秒）。歯が札を消すまでの時間より十分に長く置く。
const SLOT_WAIT_LONG_S: u64 = 60;

/// この歯の host の core 数（gate の binary が同じ host で測る値と同じ読み・読めない周は `None`）。
///
/// `/proc/self/status` の `Cpus_allowed_list`（`0,2,4-7` の形）の区間を数える。`available_parallelism` は cgroup の上限で縮むので、
/// 便の箱の上限の中で走る歯は器と同じ数を読めない（設計 gate-cost.md §45 形 1）。器の読みとは別に、この歯が字面から数える。
fn host_cores() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let list = status.lines().find_map(|line| line.strip_prefix("Cpus_allowed_list:"))?.trim();
    list.split(',')
        .map(|range| {
            let (from, to) = range.split_once('-').unwrap_or((range, range));
            to.parse::<u64>().ok()?.checked_sub(from.parse::<u64>().ok()?)?.checked_add(1)
        })
        .try_fold(0_u64, |sum, count| sum.checked_add(count?))
        .filter(|cores| *cores > 0)
}

/// 検出線の stub の行（`{base}` を印に埋める＝置換されたことを撃たれた側で読める）。
pub(super) const DETECTION_COUNT: &str = r#"["sh verify-count.sh detection-{base}"]"#;

/// `detection-verify` を持つ宣言を commit する（共通 verify も stub・`allowed-commands` は toy のまま）。
pub(super) fn commit_detection_vessel(repo: &Path, detection: &str) {
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

/// 着地後の検出の子（land が `Landed` の後に切り離して起こす・設計 gate-cost.md §44 形 (11)）の終わりを待つ（§44 実装の
/// 決め (i)）。便の event に `detection:spawned` が在れば、同じ便の終えた語（measured / unmeasured / skipped）が spawned の
/// 件数に届くまで上限つきで待つ。spawned が無ければ待たない（検出線を宣言しない便・main-red の便）。
pub(super) fn await_detection_child(state: &Path, id: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(CHILD_WAIT_S);
    loop {
        let details = detection_details(state, id);
        let spawned = details.iter().filter(|detail| detail.as_str() == SPAWNED_DETAIL).count();
        let finished = details.iter().filter(|detail| FINISHED_DETAILS.contains(&detail.as_str())).count();
        if finished >= spawned || std::time::Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// land が子を起こせた周の detail。
const SPAWNED_DETAIL: &str = "detection:spawned";

/// 子（口）が終えた語の detail（閉じた 3 値）。
const FINISHED_DETAILS: [&str; 3] = ["detection:measured", "detection:unmeasured", "detection:skipped"];

/// 子の終わりを待つ上限（秒）。
const CHILD_WAIT_S: u64 = 60;

/// record の列（1 record = key と値の対の列）。
pub(super) type Rows = Vec<Vec<(String, vessel::fleet::json_lite::Value)>>;

/// `verify-main.jsonl` の record を主実測の分と着地後の検出の分（`landed` を持つ）に分ける。
pub(super) fn split_landed(rows: Rows) -> (Rows, Rows) {
    rows.into_iter().partition(|row| value_of(row, "landed").is_empty())
}

/// [`detection_land`] の結果。
struct DetectionLand {
    /// **land が足した**呼出行（撃たれた順・主実測の分の後ろに着地後の検出の子の分）。
    added: Vec<String>,
    /// main 実測の record（`landed` を持たない）。
    rows: Rows,
    /// 着地後の検出の子の record（`landed` を持つ）。
    after: Rows,
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
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn detection_land(edit: fn(&str, &str, &str) -> String) -> DetectionLand {
    let (repo, state, design) = detection_repo(DETECTION_COUNT);
    let base_tree = git(&repo, &["rev-parse", "HEAD^{tree}"]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let before = detection_calls(&repo).len();
    let verdict = state.join("pipe").join(&id).join("verdict.json");
    let text = fs::read_to_string(&verdict).expect("verdict.json を読める");
    let tree = value_of(&verdict_pairs(&state, &id), "tree");
    assert!(!tree.is_empty(), "差し替える前の verdict は tree を持つ: {text}");
    fs::write(&verdict, edit(&text, &tree, &base_tree)).expect("verdict.json を差し替えられる");
    let out = land_once(&repo, &state, &id);
    await_detection_child(&state, &id);
    let added = detection_calls(&repo).split_off(before);
    let (rows, after) = split_landed(main_rows(&state, &id));
    DetectionLand { added, rows, after, out, repo, state, id }
}

/// 着地後の検出の子が `landed=<着地した sha>` の record を 1 本足し、呼出行は主実測の `main` 本の後ろに子の分（`fired`
/// なら着地した commit の親を `{base}` に置いた 1 行・面の外の着地なら 0 行）だけが並ぶ（設計 gate-cost.md §44 形 (11)）。
fn assert_landed_after(landed: &DetectionLand, main: usize, fired: bool) {
    let sha = git(&landed.repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(landed.after.len(), 1, "着地後の record は 1 本: {:?}", landed.after);
    let row = landed.after.first().cloned().unwrap_or_default();
    assert_eq!(value_of(&row, "landed"), sha, "landed=<着地した sha>: {row:?}");
    assert_eq!(value_of(&row, "kind"), "detection", "kind=detection: {row:?}");
    assert_eq!(landed.added.len(), main + usize::from(fired), "呼出は主実測 {main} 本 + 子の分: {:?}", landed.added);
    if fired {
        let parent = git(&landed.repo, &["rev-parse", &format!("{sha}^")]);
        assert_eq!(landed.added.last().cloned().unwrap_or_default(), format!("detection-{parent}"), "子は着地した commit の親");
    } else {
        assert_eq!(value_of(&row, "reason"), "outside-scope", "toy の着地は面の外: {row:?}");
    }
}

/// verdict の `tree` を**便の base の木**（実在する別の木・差分は便の `src/lib.rs` だけ＝面の外）へ差し替える。
fn verdict_tree_to_base(text: &str, tree: &str, base_tree: &str) -> String {
    text.replace(&format!("\"tree\":\"{tree}\""), &format!("\"tree\":\"{base_tree}\""))
}

// ---- 検出線は gate で撃たない（設計 gate-cost.md §44 形 (9)(13)・契約表の行 an・接頭辞 `pipe_detection_off_gate_`）----
//
// gate の段は ①②④ で、③ は着地後の検出の口だけが撃つ。検出線を宣言した便でも gate は stub を 1 回も呼ばず、
// `verify.jsonl` に `kind=detection` の record を書かず、周ごとの写しの置き場を作らない（＝`pipe show` に判定行が無い）。
// 母集団 = `verify-count.sh` の印の行（撃たれた順・③ の印は `detection-<base>`）と record の段の並び。base は ③ を撃つ＝RED。

/// 印の行のうち検出線の stub の呼出（`detection-` で始まる行）。
fn detection_marks(calls: &[String]) -> Vec<&String> {
    calls.iter().filter(|call| call.starts_with("detection-")).collect()
}

// ---- 主実測の省略（設計 gate-cost.md §27・ADR-0043 §2.1・`s2-07l.464`・接頭辞 `pipe_main_same_tree_`）----------
//
// gate が判定した木と着地の木が同じ周は、主実測は同じ木を同じ verify で撃ち直すだけ＝1 本も撃たず、
// `verify-main.jsonl` に主実測の skip record 1 本（`kind=main skipped=main tree=<land した木> reason=same-tree`）を
// 書いて緑。木が違う周と `tree` の無い周は ①②④ を撃つ（③ は撃たない・設計 gate-cost.md §44 形 (9)）。

/// 便の `RunDone stage=Landed` の detail のうち着地そのものの行（無ければ空・着地後の検出の `detection:` と終端の
/// `terminal:` は読み飛ばす）。
fn landed_done_detail(state: &Path, id: &str) -> String {
    trail(state, id)
        .into_iter()
        .rev()
        .filter(|(kind, stage, _)| *kind == EventKind::RunDone && *stage == Some(Stage::Landed))
        .filter_map(|(_, _, detail)| detail)
        .find(|detail| !detail.starts_with("detection:") && !detail.starts_with("terminal:"))
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
    assert_landed_after(&landed, 0, false);
    clean(&[&landed.repo, &landed.state]);
}

/// (b) 木が違う周（verdict の `tree` を便の base の木に差し替え）: record は ①②④ の 3 本（③ の record は無い）で
/// 主実測の skip record（`kind=main`）は無い（(a) と同じ fixture で分岐だけ違う負例の対）。
#[test]
fn pipe_main_same_tree_different_tree_fires_all_stages() {
    let landed = detection_land(verdict_tree_to_base);
    assert_ne!(
        value_of(&verdict_pairs(&landed.state, &landed.id), "tree"),
        git(&landed.repo, &["rev-parse", "refs/heads/main^{tree}"]),
        "fixture: gate の木と land した木は違う"
    );
    assert_main_three_stages(&landed);
    clean(&[&landed.repo, &landed.state]);
}

/// (c) verdict に `tree` が無い周（旧 gate の形）: ①②④ を撃つ（`added` は ②④・③ と `kind=main` の record は無い）。
/// (a) との対。
#[test]
fn pipe_main_same_tree_missing_tree_fires_all_stages() {
    let landed = detection_land(|text, tree, _| text.replace(&format!(",\"tree\":\"{tree}\""), ""));
    assert!(verdict_pairs(&landed.state, &landed.id).iter().all(|(key, _)| key != "tree"), "fixture は tree の無い形");
    assert_main_three_stages(&landed);
    clean(&[&landed.repo, &landed.state]);
}

/// 主実測を撃った周の共通 assert: land は rc 0・主実測の呼出は ②④ だけ（stub を呼ばない）・`verify-main.jsonl` の
/// 主実測の分（`landed` を持たない）は ①②④ の 3 本で `kind=detection` も `kind=main` も skip record も無い・着地後の検出の
/// 子は面の外の着地で撃たず `landed` 付きの 1 本だけ（設計 gate-cost.md §44 形 (9)）。
fn assert_main_three_stages(landed: &DetectionLand) {
    let (rows, out) = (&landed.rows, &landed.out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(out));
    assert_eq!(landed.added, ["common", "contract"], "主実測は ②④ だけを撃つ: {:?}", landed.added);
    assert_eq!(kinds(rows), ["write-set", "common", "contract"], "主実測の段は ①②④: {rows:?}");
    assert!(rows.iter().all(|row| value_of(row, "kind") != "detection"), "landed の無い kind=detection は 0 本: {rows:?}");
    assert!(skip_rows(rows).is_empty(), "skip record は無い: {rows:?}");
    assert_landed_after(landed, 2, false);
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

// ---- 着地後の検出の口（設計 gate-cost.md §44 行 ak・ADR-0060・接頭辞 `pipe_landed_detection_`）----------------
//
// `Landed` の便に `pipe land --run <id> --detection-only` を撃つ。fixture は gate の後に main を docs だけの 1 commit で
// 進めてから着地させる（追随は再 gate を省く＝gate が撃った `{base}` と着地した commit の親が異なる）。測るのは口の
// 前後の差（record +1・写しの周 +1・stub の呼出 +1）で、stub の本体は git の共通 dir に置き rc を口の直前に振る。

/// 検出線の跳び板（tracked・本体は共通 dir の [`LANDED_STUB`]・引数をそのまま渡す）。
const LANDED_TRAMPOLINE: &str = "verify-landed.sh";

/// 共通 dir に置く検出線の stub の名。
const LANDED_STUB: &str = "landed-stub.sh";

/// stub が argv を 1 起動 1 行で積む file の名（共通 dir）。
const LANDED_CALLS: &str = "landed-calls";

/// 宣言の検出線（`{base}` と `{teeth}` の穴を持つ）。
const LANDED_DETECTION: &str = r#"["sh verify-landed.sh --base {base} --teeth {teeth}"]"#;

/// stub が stdout に出す判定行。
const LANDED_LINE: &str = "mutants-diff: total=2 caught=1 missed=1 unviable=0 timeout=0 scope=landed";

/// 面の内の歯の置き場（契約の verify 行の filter 語 [`LANDED_WORD`] に当たる歯を持つ）。
const LANDED_LIB: &str = "crates/toy/src/lib.rs";

/// [`LANDED_LIB`] の base の本文。
const LANDED_LIB_BODY: &str = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn landed_one() {}\n}\n";

/// 契約の verify 行の filter 語（stub の `--teeth` に載る）。
const LANDED_WORD: &str = "landed_";

/// 着地後の検出の歯の便の形（base に置く file・runner の 1 行・契約の欄）。
struct LandedCase {
    /// 宣言と一緒に commit する file（repo 相対 path と本文）。
    base: Vec<(&'static str, String)>,
    /// runner の 1 行（commit を 1 本作る）。
    runner: String,
    /// 契約の欄（`write-set` と `verify` は必ず・他は任意）。
    contract: Vec<String>,
}

/// 契約の verify 行（偽 `cargo` が rc 0 で返す nextest の形・filter 語は [`LANDED_WORD`]）。
fn landed_verify() -> String {
    format!("verify = [\"cargo nextest run -p toy --lib --no-tests=fail {LANDED_WORD}\"]")
}

/// runner が [`LANDED_LIB`]（面の内＝`crates/`）に 1 行足す便。
fn landed_crates_case() -> LandedCase {
    LandedCase {
        base: vec![(LANDED_LIB, LANDED_LIB_BODY.to_owned())],
        runner: format!("echo '// landed' >> {LANDED_LIB} && git add -A && git commit -q -m runner"),
        contract: vec![format!("write-set = [\"{LANDED_LIB}\"]"), landed_verify()],
    }
}

/// 着地した便と、gate が撃った `{base}`（便の spawn の base）。
struct LandedRun {
    /// toy repo。
    repo: PathBuf,
    /// 置き場。
    state: PathBuf,
    /// 便 id。
    id: String,
    /// 着地した commit（`Gated` の周は空）。
    sha: String,
    /// gate が検出線の `{base}` に置いた sha。
    gate_base: String,
}

/// 偽 `cargo`（rc 0）を道具箱の前に積んだ PATH（契約の nextest 行を toy repo で緑にする）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn landed_path(state: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = state.join("landed-cargo-bin");
    let cargo = bin_dir.join("cargo");
    if !cargo.exists() {
        fs::create_dir_all(&bin_dir).expect("偽 cargo の dir を作れる");
        fs::write(&cargo, "#!/bin/sh\nexit 0\n").expect("偽 cargo を書ける");
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).expect("偽 cargo に実行権を付ける");
    }
    format!("{}:{}", bin_dir.display(), crate::toolbox_path(state))
}

/// 共通 dir の stub を書く（argv を積み、判定行を出して rc で終える・木は汚れない）。
fn write_landed_stub(repo: &Path, rc: u8) {
    let body = format!(
        "printf '%s\\n' \"$*\" >> \"$(git rev-parse --git-common-dir)/{LANDED_CALLS}\"\nprintf '%s\\n' '{LANDED_LINE}'\nexit {rc}\n"
    );
    fs::write(repo.join(".git").join(LANDED_STUB), body).ok();
}

/// stub が受けた argv（撃たれた順・1 起動 1 行）。
fn landed_calls(repo: &Path) -> Vec<String> {
    fs::read_to_string(repo.join(".git").join(LANDED_CALLS))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// 便を PASS の gate まで通す（検出線は跳び板・共通 verify は `verify-count.sh common`・gate は偽 cargo の PATH）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn landed_gated(case: &LandedCase) -> LandedRun {
    let (repo, state) = repo_with_state();
    let trampoline = format!("sh \"$(git rev-parse --git-common-dir)/{LANDED_STUB}\" \"$@\"\n");
    fs::write(repo.join(LANDED_TRAMPOLINE), trampoline).expect("跳び板を書ける");
    for (path, body) in &case.base {
        let file = repo.join(path);
        fs::create_dir_all(file.parent().expect("親 dir が在る")).expect("dir を作れる");
        fs::write(&file, body).expect("base の file を書ける");
    }
    write_vessel(&repo, r#"["git", "sh", "cargo"]"#, r#"["sh verify-count.sh common"]"#);
    let path = repo.join(".vessel.toml");
    let body = fs::read_to_string(&path).expect("宣言を読める");
    fs::write(&path, format!("{body}detection-verify = {LANDED_DETECTION}\n")).expect("宣言を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "vessel-landed"]);
    write_landed_stub(&repo, 0);
    let fields: Vec<&str> = case.contract.iter().map(String::as_str).collect();
    let design = write_contract(&repo, &["write-set", "verify"], &fields);
    let id = intake(&repo, &state, &design);
    let gate_base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let spawned = spawn_with(&repo, &state, &id, &case.runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));
    let (repo_arg, state_arg) = (repo.display().to_string(), state.display().to_string());
    let lens = fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"));
    let gated = run_pipe_with_path(
        &landed_path(&state),
        &["gate", "--run", &id, "--repo", &repo_arg, "--state-dir", &state_arg, "--lens", &lens],
    );
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate: {}", stderr_of(&gated));
    LandedRun { repo, state, id, sha: String::new(), gate_base }
}

/// [`landed_gated`] の便を、main を docs だけの 1 commit で進めてから着地させる（`sha` = 着地した squash）。land が起こした
/// 着地後の検出の子の終わりを待ってから返す（[`await_detection_child`]・口の歯は子の後の前後の差で測る）。
fn landed_run(case: &LandedCase) -> LandedRun {
    let mut run = landed_gated(case);
    let landed = land_after_docs_move(&run);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {} / {}", stdout_of(&landed), stderr_of(&landed));
    await_detection_child(&run.state, &run.id);
    run.sha = git(&run.repo, &["rev-parse", "refs/heads/main"]);
    run
}

/// main を docs だけの 1 commit で進めてから [`landed_gated`] の便に land を 1 回撃つ（子の終わりは待たない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn land_after_docs_move(run: &LandedRun) -> Output {
    fs::create_dir_all(run.repo.join("docs")).expect("docs を作れる");
    fs::write(run.repo.join("docs").join("moved.md"), "moved\n").expect("別便の docs を書ける");
    git(&run.repo, &["add", "-A"]);
    git(&run.repo, &["commit", "-q", "-m", "docs-move"]);
    let (repo_arg, state_arg) = (run.repo.display().to_string(), run.state.display().to_string());
    run_pipe_with_path(
        &landed_path(&run.state),
        &["land", "--run", &run.id, "--repo", &repo_arg, "--state-dir", &state_arg],
    )
}

/// 着地後の検出の口を 1 回撃つ（`rules` は任意）。
fn detection_only(run: &LandedRun, rules: Option<&Path>) -> Output {
    let (repo_arg, state_arg) = (run.repo.display().to_string(), run.state.display().to_string());
    let mut args = vec!["land", "--run", run.id.as_str(), "--detection-only", "--repo", &repo_arg, "--state-dir", &state_arg];
    let rules_arg = rules.map(|path| path.display().to_string());
    if let Some(found) = &rules_arg {
        args.extend(["--rules", found.as_str()]);
    }
    run_pipe_with_path(&landed_path(&run.state), &args)
}

/// 口の前の面（主実測の record 数・stub の呼出数・共通 verify の呼出数・写しの周・event 数）。
struct LandedBefore {
    /// `verify-main.jsonl` の record 数。
    rows: usize,
    /// 検出線の stub の呼出数。
    calls: usize,
    /// 共通 verify の stub の呼出数。
    common: usize,
    /// 写しの周の番号。
    rounds: Vec<u64>,
    /// event log の行数。
    events: usize,
    /// 便の `detection:` の detail の件数（着地後の検出の子の分を含む）。
    details: usize,
}

/// 口の前の面を読む。
fn landed_before(run: &LandedRun) -> LandedBefore {
    LandedBefore {
        rows: main_rows(&run.state, &run.id).len(),
        calls: landed_calls(&run.repo).len(),
        common: detection_calls(&run.repo).len(),
        rounds: copy_rounds(&run.state, &run.id),
        events: event_count(&run.state),
        details: detection_details(&run.state, &run.id).len(),
    }
}

/// 口が足した `detection:` の detail（口の前後の差・設計 gate-cost.md §44 実装の決め (i)）。
fn added_details(run: &LandedRun, before: &LandedBefore) -> Vec<String> {
    detection_details(&run.state, &run.id).split_off(before.details)
}

/// 口が足した写しの周の番号（前より 1 つ増えたことを要求する）。
fn added_round(run: &LandedRun, before: &LandedBefore) -> u64 {
    let rounds = copy_rounds(&run.state, &run.id);
    assert_eq!(rounds.len(), before.rounds.len() + 1, "写しの周は +1: {:?} → {rounds:?}", before.rounds);
    rounds.last().copied().unwrap_or_default()
}

/// 便の `RunDone stage=Landed` の detail のうち `detection:` で始まるもの（物理順）。
fn detection_details(state: &Path, id: &str) -> Vec<String> {
    trail(state, id)
        .into_iter()
        .filter(|(kind, stage, _)| *kind == EventKind::RunDone && *stage == Some(Stage::Landed))
        .filter_map(|(_, _, detail)| detail)
        .filter(|detail| detail.starts_with("detection:"))
        .collect()
}

/// `pipe show` の写しの行（判定行か不在の 1 行で始まる行・周の番号順）。
fn shown_copies(run: &LandedRun) -> Vec<String> {
    show_line(&run.repo, &run.state, &run.id)
        .lines()
        .filter(|line| line.starts_with("mutants-diff:") || line.starts_with(COPY_ABSENT_LINE))
        .map(str::to_owned)
        .collect()
}

/// 口が record を 1 本足し、その record が `kind=detection` と着地した sha・木を持つ（新しい record を返す）。
fn added_landed_row(run: &LandedRun, before: &LandedBefore) -> Vec<(String, vessel::fleet::json_lite::Value)> {
    let rows = main_rows(&run.state, &run.id);
    assert_eq!(rows.len(), before.rows + 1, "record は +1: {rows:?}");
    let row = rows.last().cloned().unwrap_or_default();
    assert_eq!(value_of(&row, "kind"), "detection", "kind=detection: {row:?}");
    assert_eq!(value_of(&row, "landed"), run.sha, "landed=<着地した sha>: {row:?}");
    assert_eq!(value_of(&row, "tree"), git(&run.repo, &["rev-parse", &format!("{}^{{tree}}", run.sha)]), "着地した木");
    row
}

/// 口が stub を 1 回だけ撃ち、`--base` が着地した commit の親（gate の base と異なる）・`--teeth` が契約の語で、共通
/// verify の stub は呼ばれない。
fn assert_fired_on_the_parent(run: &LandedRun, before: &LandedBefore) {
    let parent = git(&run.repo, &["rev-parse", &format!("{}^", run.sha)]);
    assert_ne!(parent, run.gate_base, "fixture: gate の base と着地した commit の親は異なる");
    let calls = landed_calls(&run.repo);
    assert_eq!(calls.len(), before.calls + 1, "stub は 1 回だけ呼ばれる: {calls:?}");
    let gated = format!("--base {} --teeth {LANDED_WORD}", run.gate_base);
    assert!(!calls.contains(&gated), "gate は stub を呼ばない（gate の base の呼出は無い・設計 gate-cost.md §44 形 (9)）: {calls:?}");
    assert_eq!(calls.last().cloned().unwrap_or_default(), format!("--base {parent} --teeth {LANDED_WORD}"), "親と契約の語");
    assert_eq!(detection_calls(&run.repo).len(), before.common, "共通 verify は撃たない");
}

/// 測れなかった周の共通 assert: rc 0・`detection=unmeasured`・`landed` 付きの record +1・理由の file と show の行の末尾が
/// `unmeasured=<理由>`・event は `detection:unmeasured` 1 件・段は `Landed` のまま・main の sha は動かない（record を返す）。
fn assert_landed_unmeasured(
    run: &LandedRun,
    before: &LandedBefore,
    out: &Output,
    reason: &str,
) -> Vec<(String, vessel::fleet::json_lite::Value)> {
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "測れなかった周も rc 0: {}", stderr_of(out));
    assert_eq!(stdout_of(out), format!("run={} detection=unmeasured\n", run.id), "stdout は 1 行");
    let row = added_landed_row(run, before);
    let round = added_round(run, before);
    assert_eq!(read_copy(&run.state, &run.id, round, "reason"), format!("unmeasured={reason}\n"), "理由の file");
    let last = shown_copies(run).last().cloned().unwrap_or_default();
    assert!(last.ends_with(&format!(" unmeasured={reason}")), "show の行の末尾に理由: {last}");
    assert_eq!(added_details(run, before), ["detection:unmeasured"], "event は 1 件");
    assert!(show_line(&run.repo, &run.state, &run.id).contains("stage=Landed"), "段は Landed のまま");
    assert_eq!(git(&run.repo, &["rev-parse", "refs/heads/main"]), run.sha, "main の sha は動かない");
    row
}

/// 純移動の便（`fn two` を `alpha.rs` へ移す）: runner の `{}` を HEAD の写しの dir に置き換えて着地させる。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn landed_pure_move(mut case: LandedCase) -> LandedRun {
    let staged = tmp().join("landed-head");
    fs::create_dir_all(&staged).expect("HEAD の写しの dir を作れる");
    fs::write(staged.join("lib.rs"), "fn one() -> u8 {\n    1\n}\n").expect("HEAD の file を書ける");
    fs::write(staged.join("alpha.rs"), "fn two() -> u8 {\n    2\n}\n").expect("HEAD の file を書ける");
    case.runner = case.runner.replace("{}", &staged.display().to_string());
    landed_run(&case)
}

/// 断った周の共通 assert: record・写し・event がどれも増えず、stdout は空。
fn assert_landed_untouched(run: &LandedRun, before: &LandedBefore) {
    assert_eq!(main_rows(&run.state, &run.id).len(), before.rows, "record は増えない");
    assert_eq!(copy_rounds(&run.state, &run.id), before.rounds, "写しは増えない");
    assert_eq!(event_count(&run.state), before.events, "event は増えない");
    assert_eq!(landed_calls(&run.repo).len(), before.calls, "stub は呼ばれない");
}

// ---- land が口を切り離して起こす（設計 gate-cost.md §44 行 am・形 (11)・接頭辞 `pipe_detection_after_landing_`）--------
//
// land は `Landed` の event の後に、宣言の写しに検出線の行が在る便だけ行 ak の口を子 process で起こして待たない。
// ③ は本行ではまだ gate も撃つ（gate の record は残る）。base の land は子を起こさない＝`detection:spawned` が 0 件で RED。
// 候補の木の後続の歯（(o)）は候補の木の fixture が在る `land.rs` の同じ接頭辞。

/// 解放の file の名（共通 dir・歯が置くまで [`write_blocking_stub`] の stub は上限つきで待つ）。
const LANDED_RELEASE: &str = "landed-release";

/// 共通 dir の stub を「argv を積んでから解放の file を上限つきで待ち、判定行を出して rc 0」に書き換える。
fn write_blocking_stub(repo: &Path) {
    let body = format!(
        "common=\"$(git rev-parse --git-common-dir)\"\nprintf '%s\\n' \"$*\" >> \"$common/{LANDED_CALLS}\"\ni=0\nwhile [ ! -f \"$common/{LANDED_RELEASE}\" ] && [ \"$i\" -lt 600 ]; do sleep 0.1; i=$((i + 1)); done\nprintf '%s\\n' '{LANDED_LINE}'\nexit 0\n"
    );
    fs::write(repo.join(".git").join(LANDED_STUB), body).ok();
}

/// (k) の解放の後の面: `landed` を持つ record 1 本（stub の判定行・rc 0）・stub は子の 1 回だけで子の `--base` は
/// 着地した commit の親・gate の ③ の record は無い（gate は ③ を撃たない・設計 gate-cost.md §44 形 (9)）・台帳の見張りは着地の close の 1 件だけ。
fn assert_child_measured_the_landed_commit(run: &LandedRun) {
    let (_, after) = split_landed(main_rows(&run.state, &run.id));
    assert_eq!(after.len(), 1, "landed を持つ record は 1 本: {after:?}");
    let row = after.first().cloned().unwrap_or_default();
    assert_eq!(value_of(&row, "landed"), run.sha, "landed=<着地した sha>: {row:?}");
    assert_eq!(value_of(&row, "line"), LANDED_LINE, "line= は stub の判定行: {row:?}");
    assert_eq!(value_of(&row, "rc"), "0", "{row:?}");
    let parent = git(&run.repo, &["rev-parse", &format!("{}^", run.sha)]);
    assert_ne!(parent, run.gate_base, "fixture: gate の base と着地した commit の親は異なる");
    let calls = landed_calls(&run.repo);
    assert_eq!(calls, [format!("--base {parent} --teeth {LANDED_WORD}")], "stub は着地後の 1 回だけで子は着地した commit の親");
    let gated: Vec<_> = verify_rows(&run.state, &run.id).into_iter().filter(|row| value_of(row, "kind") == "detection").collect();
    assert!(gated.is_empty(), "gate の ③ の record は無い: {gated:?}");
    let calls = crate::toolbox_ledger_record_names(&run.state);
    assert_eq!(calls.len(), 1, "台帳 client を起こしたのは着地の close の 1 回だけ: {calls:?}");
}

/// 便の `Gated` event の detail の並び（測り直しの履歴）。
fn gated_details(state: &Path, id: &str) -> Vec<String> {
    events(state)
        .into_iter()
        .filter(|event| event.run == id && event.stage == Some(Stage::Gated))
        .filter_map(|event| event.detail)
        .collect()
}

// ---- 判定行の記録（設計 gate-cost.md §5.1・`s2-07l.206`・接頭辞 `pipe_record_`）--------------------
//
// verify 行の stdout の末尾 1 行を record の `line=` に**逐語で**残す（kind と rc を問わず・判定には
// 使わない）。母集団 = gate の record（write-set 1 + 共通 1 + 契約 n・fixture は検出線を宣言するが gate は ③ を撃たない
// ＝設計 gate-cost.md §44 形 (9)）。

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
    assert_eq!(rows.len(), 4, "母集団 = write-set 1 + 共通 1 + 契約 2: {rows:?}");
    assert_eq!(row_value(&rows, 4, "cmd"), "sh verify-line-red.sh", "赤いのは契約の 2 本目");
    assert_eq!(row_value(&rows, 4, "rc"), "1", "赤い行");
    assert_eq!(row_value(&rows, 4, "line"), RED_TAIL, "赤い行にも stdout の末尾が載る: {rows:?}");
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
    assert_eq!(row_value(&rows, 3, "cmd"), "sh verify-ok.sh", "④ は何も出さない行");
    assert!(!row_has(&rows, 3, "line"), "stdout の無い行は `line` を欠く: {:?}", rows.get(2));
    assert!(!row_has(&rows, 1, "line"), "撃つ process を持たない段①も欠く: {:?}", rows.first());
    let log = verify_log(&state, &id);
    assert!(!log.contains("\"line\":\"\""), "空文字は書かない: {log}");
    assert_eq!(
        log.matches("\"line\":").count(),
        1,
        "`line` を持つのは stdout を出した ② の 1 record だけ（母集団 {} record）: {log}",
        rows.len()
    );
    clean(&[&repo, &state]);
}

/// (f) `pipe show --run` は 1 行目の段に続けて **検出線の写しの `line` だけ**を逐語で出す（他の kind の
/// `line` は出さない・gate 前は 1 行目だけ）。gate は ③ を撃たず写しも書かない（設計 gate-cost.md §44 形 (9)）ので、
/// 検出線を宣言した便でも gate の後の外形は判定行を持たない。外形は snapshot（置き場は親の `snapshots/`）。
///
/// 行の末尾の段の秒（`secs=`・設計 gate-cost.md §26 形 (1)）は**周ごとに動く**ので、snapshot に入れる前に
/// [`mask_secs`] で `[secs]` へ置く（`default-features = false` の insta は `add_filter` を持たない・
/// `src/main.rs` の `doctor_external_form` と同じ型）。
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

// ---- lens の claude の消費（設計 gate-cost.md §26 形 (2)・接頭辞 `run_cost_`）----
//
// 偽 lens は判定 object に消費の 3 対（`usage` / `turns` / `wall_ms`）を足して出す（headless の lens が json の封筒から
// 足す形と同じ）。母集団 = 置き場の `RunCost` の event 数。

/// 6 値が揃った消費の 3 対（数は互いに違う）。
const LENS_COST: &str = r#""usage":"in:11,out:22,cache_read:33,cache_create:44","turns":5,"wall_ms":6000"#;

/// [`lens_verdict`] の PASS に `extra` の対を足した偽 lens の本文。
fn verdict_with(extra: &str) -> String {
    let base = lens_verdict("PASS");
    format!("{},{extra}}}", base.strip_suffix('}').unwrap_or_default())
}

/// 便を Implemented まで進め、`body` を出す偽 lens で gate を 1 回撃つ（repo・置き場・便 id・gate の出力）。
fn gated_with(body: &str) -> (PathBuf, PathBuf, String, Output) {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &design);
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, body)));
    (repo, state, id, out)
}

/// 置き場の消費の event（物理順の位置と event）。
fn cost_events(state: &Path) -> Vec<(usize, Event)> {
    events(state).into_iter().enumerate().filter(|(_, event)| event.kind == EventKind::RunCost).collect()
}

/// (e) gate の lens の消費: 判定 object が 6 値を運ぶ周は `RunCost source=lens` が 1 件、`Gated` の前に書かれ、値は偽
/// lens の出した数と一致する。判定と rc は PASS のまま。`pipe show --run` は母集団と消費の行を、`pipe report` の 2 行目は
/// 便の数と token の和を写す（母集団 = event 数 1）。
#[test]
fn run_cost_gate_records_one_lens_event_before_gated() {
    let (repo, state, id, out) = gated_with(&verdict_with(LENS_COST));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS のまま: {}", stderr_of(&out));
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS", "判定は動かない");
    let costs = cost_events(&state);
    assert_eq!(costs.len(), 1, "消費の event は lens の 1 件（偽 runner は usage を運ばない）: {costs:?}");
    let Some((at, event)) = costs.first() else {
        panic!("消費の event が無い");
    };
    let want = vessel::fleet::Usage { input: 11, output: 22, cache_read: 33, cache_create: 44, turns: 5, wall_ms: 6000 };
    let source = vessel::fleet::CostSource::Lens;
    assert_eq!(event.cost, Some(vessel::fleet::Cost { source, usage: want }), "値は偽 lens の数");
    let gated = events(&state).iter().position(|found| found.stage == Some(Stage::Gated));
    assert!(gated.is_some_and(|found| *at < found), "Gated の前に書く: {at} / {gated:?}");
    let shown = show_line(&repo, &state, &id);
    let lines: Vec<&str> = shown.lines().filter(|line| line.starts_with("cost:")).collect();
    assert_eq!(
        lines,
        ["cost: events=1", "cost: source=lens usage=in:11,out:22,cache_read:33,cache_create:44 turns=5 wall_ms=6000"],
        "{shown}"
    );
    let report = report_once(&state);
    let second = stdout_of(&report).lines().nth(1).unwrap_or_default().to_owned();
    assert!(second.starts_with("cost: with_usage=1 out=22 cache_read=33 gate_secs="), "{}", stdout_of(&report));
    clean(&[&repo, &state]);
}

/// (f)(g) usage を出さない偽 lens・`turns` だけ欠く・`wall_ms` が数でない周は消費の event を書かず、判定と rc は 6 値
/// 揃いの周と同じ PASS / rc 0（「測れなかった」は field の不在で運ぶ・欠けを 0 に倒さない＝同じ歯の中で揃い＝1 件を対に
/// 並べる）。
#[test]
fn run_cost_gate_without_full_usage_writes_no_event_and_keeps_the_verdict() {
    let turns_missing = LENS_COST.replacen(",\"turns\":5", "", 1);
    let wall_text = LENS_COST.replacen("\"wall_ms\":6000", "\"wall_ms\":\"6000\"", 1);
    let cases = [
        (lens_verdict("PASS"), 0, "usage を出さない"),
        (verdict_with(&turns_missing), 0, "turns だけ欠く"),
        (verdict_with(&wall_text), 0, "wall_ms が数でない"),
        (verdict_with(LENS_COST), 1, "6 値揃い"),
    ];
    for (body, want, why) in cases {
        let (repo, state, id, out) = gated_with(&body);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{why}: rc は PASS のまま: {}", stderr_of(&out));
        assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS", "{why}: 判定は動かない");
        assert_eq!(cost_events(&state).len(), want, "{why}: 消費の event 数");
        assert_eq!(show_line(&repo, &state, &id).contains("cost:"), want > 0, "{why}: 描画");
        clean(&[&repo, &state]);
    }
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

// ---- 周ごとの検出線の写し（設計 gate-cost.md §15・`s2-07l.298`）の読み手 ----
//
// 検出線を撃った周の判定行と出力は run dir の**周ごとの置き場**へ写り、`pipe show` の判定行はその写しから読む。写すのは
// 着地後の検出の口だけ（gate は ③ を撃たない・設計 gate-cost.md §44 形 (9)）で、下の読み手は口の歯
// （`pipe_landed_detection_`）が使う。

/// 写しの置き場の名（run dir 直下）。**字面で組む**——同じ定数を器から引くと、置き場を変えた実装でも
/// 歯が追随して通る（置き場そのものを pin する）。
const COPY_DIR: &str = "detection";

/// 写しの中の判定行の file 名。
const COPY_LINE: &str = "line";

/// 判定行の無い周に残る 1 行（**0 件の判定行と別の字面**）。
const COPY_ABSENT_LINE: &str = "detection-line: absent";

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

// ---- 段の秒（設計 gate-cost.md §26 形 (1)・`s2-07l.466`・接頭辞 `gate_secs_`）--------------------
//
// 撃った段の record だけが `secs=`（process の起動から終了までの壁時計・秒）を持つ。母集団 = 判定行の
// fixture の gate の 3 record（write-set 1 + 共通 1 + 契約 1・gate は ③ を撃たない＝設計 gate-cost.md §44 形 (9)）で、
// 撃つ process を持たない段①と、撃たなかった段（land の skip record）は field を欠く（0 と書かない・C10）。

/// gate の `verify.jsonl` は**撃った段の全部**に `secs=` を持ち段①は持たない・land の `verify-main.jsonl` も同じ形
/// （省いた段は持たない）。
#[test]
fn gate_secs_only_fired_steps_carry_the_wall_clock() {
    let (repo, state, id, out) = line_gate(r#"["sh verify-ok.sh"]"#);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "contract"], "母集団 3 record: {rows:?}");
    assert!(!row_has(&rows, 1, "secs"), "撃つ process を持たない段①は秒を欠く: {:?}", rows.first());
    for n in [2, 3] {
        assert!(row_has(&rows, n, "secs"), "撃った段 {n} は秒を持つ: {rows:?}");
        assert!(row_value(&rows, n, "secs").parse::<u64>().is_ok(), "秒は数（{n} 番目）: {rows:?}");
    }
    let log = verify_log(&state, &id);
    assert_eq!(
        log.matches("\"secs\":").count(),
        2,
        "秒を持つのは撃った 2 record だけ（母集団 {} record）: {log}",
        rows.len()
    );
    // land の主実測も同じ 1 本（`step_record`）を通る＝撃った段は秒を持ち、撃つ process を持たない段①は持たない。
    // fixture は verdict の木を base の木に差し替えて主実測を撃つ周にする（同じ木の周は主実測ごと省く・設計 gate-cost.md §27）。
    super::land::make_tree_differ(&repo, &state, &id, "refs/heads/main");
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    await_detection_child(&state, &id);
    let (main, after) = split_landed(main_rows(&state, &id));
    assert_eq!(after.len(), 1, "着地後の record は 1 本: {after:?}");
    assert_eq!(kinds(&main), ["write-set", "common", "contract"], "main 実測の母集団（③ は撃たない）: {main:?}");
    assert!(!row_has(&main, 1, "secs"), "段①は秒を欠く: {:?}", main.first());
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

/// 1 本だけ動く純移動の base（`fn two` が `src/alpha.rs` へ移る・残差は空行の `-` だけ）。
const POP_BASE_LIB: &str = "fn one() -> u8 {\n    1\n}\n\nfn two() -> u8 {\n    2\n}\n";

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

/// 大きい本文を持つ fn（移動すると diff は本文を 2 度運び、要約は名だけを運ぶ）。
fn big_fn(visibility: &str) -> String {
    let mut body = format!("{visibility}fn big() -> u32 {{\n");
    for number in 0..120 {
        body.push_str(&format!("    let _ = \"line {number:03} of the moved body, long enough to weigh\";\n"));
    }
    body.push_str("    0\n}\n");
    body
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

// ---- 持ち越しの札（`s2-07l.362`・.361 run 2 = base に元から在る `retroactive` の札が item ごと移り新規と読まれた型）----

/// `two` の本文の中に `// flip-check: retroactive <id>` の札を持つ形（base と HEAD の両側に同じ字面で置く）。
fn carried(text: &str, id: &str) -> String {
    let marked = text.replace("    2\n", &format!("    // flip-check: retroactive {id}\n    2\n"));
    assert_ne!(marked, text, "fixture は two の本文を持つ");
    marked
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
        vec![format!("verdict:PASS,account:a2,rules:{}", rules_source(&repo, Some(&rules)))],
        "記帳は判定と起こした口座と読んだ manifest の出所を運ぶ"
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
    let word = rules_source(&repo, Some(&rules.display().to_string()));
    assert_eq!(gated_details(&state, &id), vec![format!("verdict:PASS,rules:{word}")], "detail は判定と読んだ manifest の出所だけ（account を足さない）");
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
    let word = rules_source(&repo, Some(&rules));
    assert_eq!(gated_details(&state, &id), vec![format!("verdict:INCONCLUSIVE,rules:{word}")], "選べなかった周は account を足さない");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "便は Gated のまま（測り直せる側）");
    clean(&[&repo, &state]);
}

/// (d) lens の選定の計測も鮮度つきの 1 本の口（`s2-07l.712`・設計 account-autonomy.md §22 (1)）: 新しい実測の口座は
/// 測り直さず、その実測で選ぶ。
///
/// a1 の偽 curl は本文を持たない（測り直すと読めず Unmeasured）が、今の ts の実測の回が置き場に在り、鮮度の行は
/// [`lifecycle::FRESH_S`]。base（`select_lens_account` が `fleet usage` の口＝`Always` で撃つ）は a1 を測り直して
/// Unmeasured を積み、候補なしの INCONCLUSIVE（lens 0・呼出 1）→ RED。
#[test]
fn pipe_gate_lens_account_fresh_measured_account_is_not_remeasured() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let rules = lifecycle::resume_rules_fresh(&state, &["a1"], lifecycle::FRESH_S);
    lifecycle::put_account(&state, "a1", &[]);
    lifecycle::put_round(&state, &vessel::fleet::cli::now_utc(), "a1");
    let out = gate_with_accounts(&repo, &state, &id, &rules, &argv_lens(&state, "PASS"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(lens_calls(&state), 1, "lens は 1 回起きる");
    assert_eq!(
        lifecycle::argv_account_dir(&lens_argv(&state)),
        Some(state.join("accounts").join("a1").display().to_string()),
        "lens の argv の末尾に新しい実測の口座の credential dir: {:?}",
        lens_argv(&state)
    );
    assert_eq!(lifecycle::curl_calls(&state), 0, "新しい実測の口座は測り直さない");
    assert_eq!(
        gated_details(&state, &id),
        vec![format!("verdict:PASS,account:a1,rules:{}", rules_source(&repo, Some(&rules)))],
        "記帳は判定と起こした口座と読んだ manifest の出所を運ぶ"
    );
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
    assert_eq!(gated_details(&state, &id), vec![format!("verdict:PASS,rules:{}", rules_source(&repo, None))], "Gated の detail は判定と出所だけ");
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

/// 起こされるたびに回数を [`lens_calls`] の置き場へ積み、`subtype` が `error_max_turns` の封筒（`result` に findings と population を
/// 持つ PASS の判定の行を含む）を返す偽 claude を書き、gate の lens の行を**器の lens**（本 binary の `lens` に `{contract}` と
/// `{worktree}` と `--claude` を渡す形）にして返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn max_turns_lens(state: &Path) -> String {
    let spy = lens_spy(state);
    fs::create_dir_all(&spy).expect("偽 claude の置き場を作れる");
    let result = lens_verdict("PASS").replace('"', "\\\"");
    let envelope = format!(
        "{{\"type\":\"result\",\"subtype\":\"error_max_turns\",\"is_error\":false,\"num_turns\":5,\"duration_ms\":6000,\
         \"result\":\"{result}\",\"usage\":{{\"input_tokens\":11,\"cache_creation_input_tokens\":44,\
         \"cache_read_input_tokens\":33,\"output_tokens\":22}},\"total_cost_usd\":0.5}}\n"
    );
    fs::write(spy.join("envelope"), envelope).expect("封筒を書ける");
    let script = format!(
        "#!/bin/sh\ncat >/dev/null\nprintf 'call\\n' >> '{}'\ncat '{}'\n",
        spy.join("calls").display(),
        spy.join("envelope").display()
    );
    let claude = state.join("max-turns-claude.sh");
    fs::write(&claude, script).expect("偽 claude を書ける");
    let mut perm = fs::metadata(&claude).expect("権限を読める").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
    fs::set_permissions(&claude, perm).expect("実行可能にできる");
    format!(
        "{} lens --contract {{contract}} --worktree {{worktree}} --claude {}",
        env!("CARGO_BIN_EXE_scribe2"),
        claude.display()
    )
}

/// (d) gate の lens の行を器の lens と上限で終わる偽 claude にした便は、gate が INCONCLUSIVE（rc 3）で、偽 claude の回数が 2
/// （形の読めない周として同じ gate の中で 1 回だけ撃ち直す・3 回目は無い）、stderr の撃ち直しの行が 1 本で理由に `lens.max_turns` を含む。
/// 封筒の `result` は PASS の判定の行を持つので、上限の読みが無ければ PASS で回数 1 になる。base は PASS で回数 1 なので RED。
#[test]
fn lens_turns_gate_rereads_a_max_turns_round_once_and_stops_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let lens = max_turns_lens(&state);
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "2 回目も上限なら rc 3: {} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(lens_calls(&state), 2, "撃ち直しは 1 回（合計 2 回）");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "{pairs:?}");
    assert!(value_of(&pairs, "evidence").contains("lens.max_turns"), "判定の理由が行を名指す: {pairs:?}");
    let line = reread_line(&out).unwrap_or_default();
    assert!(line.contains("lens.max_turns"), "撃ち直しの行の理由が行を名指す: {}", stderr_of(&out));
    assert_eq!(stderr_of(&out).matches(REREAD_LINE).count(), 1, "撃ち直しの行は 1 本: {}", stderr_of(&out));
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
// verify の行を 1 本も撃たずに INCONCLUSIVE、倍率を十分大きく取った便は従来どおり全段（①②④）を撃つ。呼出回数は
// 共通 verify と契約 verify の stub（`verify-count.sh`）の印（[`verify_calls`]）で数える（検出線は宣言しない・gate は
// ③ を撃たない＝設計 gate-cost.md §44 形 (9)）。

/// 共通 verify と契約 verify の stub の印の行（撃たれた順・[`detection_calls`] と同じ file を段の印で読む）。
fn verify_calls(repo: &Path) -> Vec<String> {
    detection_calls(repo)
}

/// 遮断器の歯の便を Implemented まで通す（共通 verify と契約 verify は印を 1 行ずつ足す stub）。
fn health_run() -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-count.sh common"]"#);
    let design = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-count.sh contract"]"#]);
    let id = implemented(&repo, &state, &design);
    assert!(verify_calls(&repo).is_empty(), "fixture: gate の前は印が無い");
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
    assert!(verify_calls(&repo).is_empty(), "verify の行は 1 本も撃たれない: {:?}", verify_calls(&repo));
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
    assert_eq!(verify_calls(&repo), ["common".to_owned(), "contract".to_owned()], "全段（②④）を 1 回ずつ撃つ");
    assert_eq!(kinds(&verify_rows(&state, &id)), ["write-set", "common", "contract"], "gate の段は ①②④");
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
    let word = |per_core: u64| rules_source(&repo, Some(&state.join(format!("rules-health-{per_core}.toml")).display().to_string()));
    assert_eq!(gated_details(&state, &id), [format!("verdict:INCONCLUSIVE,rules:{}", word(0))], "Gated に留まる");
    let failed = events(&state).into_iter().filter(|event| event.run == id && event.stage == Some(Stage::Failed)).count();
    assert_eq!(failed, 0, "FAIL で終端しない（Failed の event が無い）");
    let calm = health_gate(&repo, &state, &id, HEALTH_PER_CORE_OPEN);
    assert_eq!(calm.status.code(), Some(i32::from(RC_OK)), "撃ち直せて PASS: {} / {}", stdout_of(&calm), stderr_of(&calm));
    assert_eq!(verify_calls(&repo), ["common".to_owned(), "contract".to_owned()], "撃ち直しの周で初めて撃つ");
    assert_eq!(
        gated_details(&state, &id),
        [format!("verdict:INCONCLUSIVE,rules:{}", word(0)), format!("verdict:PASS,rules:{}", word(HEALTH_PER_CORE_OPEN))],
        "測り直しの履歴"
    );
    clean(&[&repo, &state]);
}

// ───── verify の record の `failed=` と落ちた歯ごとの stderr の区間（設計 pipeline.md §35・行 ac・`s2-07l.401`・接頭辞 `pipe_verify_failed_`） ─────

/// nextest 形の stderr を出して rc 100 で終える契約 verify の行（[`write_nextest_red`] が置く script）。
pub(super) const NEXTEST_RED: &str = "sh verify-nextest.sh";

/// 最初に落ちた歯の名（fixture の 1 本目の `FAIL [` の行）。
pub(super) const NEXTEST_FIRST: &str = "toy::alpha::breaks";

/// 1 本目の歯の panic の本文（**cmd にも末尾 20 行にも無い字面**＝区間から来たことだけで写しに載る）。
pub(super) const NEXTEST_PANIC: &str = "alpha-panic-body";

/// nextest 形の stderr の fixture と、それを stderr へ出して rc 100 で終える script を repo へ置く（commit は呼び手の
/// `write_contract` の `add -A` が拾う）。落ちた歯 2 本・1 本目は後ろに PASS の進捗行 30 本（末尾 N 行の外）。
///
/// `once` の周は 1 回目（gate の便の worktree）を緑で通し、2 回目（主実測）から赤い（印は git の共通 dir）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn write_nextest_red(repo: &Path, once: bool) {
    let mut lines: Vec<String> = vec![
        "────────────".to_owned(),
        "    Starting 32 tests across 1 binary".to_owned(),
        format!("        FAIL [   0.003s] (  1/32) toy {NEXTEST_FIRST}"),
        "  stderr ───".to_owned(),
        format!("    thread '{NEXTEST_FIRST}' panicked at src/alpha.rs:9:5:"),
        format!("    assertion failed: {NEXTEST_PANIC}"),
        String::new(),
    ];
    for index in 2..32 {
        lines.push(format!("        PASS [   0.001s] ({index:>3}/32) toy toy::green::t{index}"));
    }
    lines.push("        FAIL [   0.004s] ( 32/32) toy toy::beta::breaks".to_owned());
    lines.push("  stderr ───".to_owned());
    lines.push("    assertion failed: beta-panic-body".to_owned());
    lines.push("────────────".to_owned());
    lines.push("     Summary [   0.004s] 32 tests run: 30 passed, 2 failed, 0 skipped".to_owned());
    lines.push(format!("        FAIL [   0.003s] (  1/32) toy {NEXTEST_FIRST}"));
    lines.push("        FAIL [   0.004s] ( 32/32) toy toy::beta::breaks".to_owned());
    lines.push("error: test run failed".to_owned());
    fs::write(repo.join("nextest-red.txt"), format!("{}\n", lines.join("\n"))).expect("fixture を書ける");
    let gate = if once {
        "seen=\"$(git rev-parse --git-common-dir)/nextest-seen\"\ntest ! -f \"$seen\" && touch \"$seen\" && exit 0\n"
    } else {
        ""
    };
    fs::write(repo.join("verify-nextest.sh"), format!("{gate}cat nextest-red.txt >&2\nexit 100\n"))
        .expect("verify script を書ける");
}

/// gate の `verify.jsonl` の赤い契約行に `failed=<最初の歯>` と区間が載り、`verify.stderr.log` に末尾 N 行の外の
/// panic の本文が残る。`FAIL [` を出さない赤い行（`verify-noisy.sh`）は `failed` を持たない（従来の末尾だけ）。
#[test]
fn pipe_verify_failed_gate_record_names_the_tooth_and_keeps_its_section() {
    let (repo, state) = repo_with_state();
    write_nextest_red(&repo, false);
    let line = format!(r#"verify = ["{NEXTEST_RED}", "sh verify-noisy.sh"]"#);
    let path = write_contract(&repo, &["verify"], &[&line]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1: {}", stderr_of(&out));
    let rows = verify_rows(&state, &id);
    let nextest = rows
        .iter()
        .find(|row| value_of(row, "cmd") == NEXTEST_RED)
        .expect("nextest の行の record が在る");
    assert_eq!(value_of(nextest, "rc"), "100", "rc は従来どおり: {nextest:?}");
    assert_eq!(value_of(nextest, "failed"), NEXTEST_FIRST, "failed= は最初の落ちた歯: {nextest:?}");
    let sections = value_of(nextest, "failed_stderr");
    assert!(sections.contains(NEXTEST_PANIC), "1 本目の区間が record に載る: {sections}");
    assert!(sections.contains("beta-panic-body"), "2 本目の区間も順に載る: {sections}");
    let noisy = rows
        .iter()
        .find(|row| value_of(row, "cmd") == "sh verify-noisy.sh")
        .expect("noisy の行の record が在る");
    assert_eq!(value_of(noisy, "rc"), "3", "赤い行: {noisy:?}");
    assert_eq!(value_of(noisy, "failed"), "", "FAIL [ の無い行は failed を持たない: {noisy:?}");
    assert_eq!(value_of(noisy, "failed_stderr"), "", "区間も持たない: {noisy:?}");
    let log = fs::read_to_string(run_dir(&state, &id).join("verify.stderr.log")).expect("verify.stderr.log を読める");
    assert!(log.contains(NEXTEST_PANIC), "末尾 N 行の外の panic が診断 file に残る: {log}");
    assert!(log.contains("boom"), "FAIL [ の無い行は従来どおり末尾: {log}");
    clean(&[&repo, &state]);
}

// ───── lens に渡す diff の畳み（設計 gate-cost.md §41・行 ah・接頭辞 `pipe_gate_elide_`） ─────
//
// runner が code file を `git mv` し（rename の対）、同じ commit で doc の行の旧 path を新 path へ書き換える。
// 畳むのは `docs/design/` の `.md` の hunk のうち、置換後の一致・各行の効き・1 塊を全部満たすものだけ。

/// rename の対の旧 path（fixture の base に在る code file・item を持たない＝純移動の要約にならない）。
const ELIDE_OLD: &str = "src/old_mod.rs";

/// rename の対の新 path。
const ELIDE_NEW: &str = "src/new_mod.rs";

/// 畳みの面の doc（`docs/design/` 配下の `.md`）。
const ELIDE_NOTES: &str = "docs/design/notes.md";

/// 畳んだ hunk の印の頭。
const ELIDE_MARK_HEAD: &str = "~ rename の置換だけの hunk";

/// path を名指す契約表の row 1 行（改行なし）。
fn elide_row(path: &str) -> String {
    format!("| row names `{path}` in the table |")
}

/// 畳みの歯の便を Implemented まで通す: base の file 群（と [`ELIDE_OLD`]）を commit し、runner は `rename` の周だけ
/// [`ELIDE_OLD`] を [`ELIDE_NEW`] へ `git mv` し、HEAD の file 群を写して commit する（write-set は [`ELIDE_OLD`] と base の
/// file 群・`+` の [`ELIDE_NEW`] は作る周〔`rename`〕だけ宣言する＝段 ① は `+` の file が便の木に在ることを測る・設計
/// pipeline.md §58）。
// flip-check: retroactive s2-07l.697
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn elide_run(base: &[(&str, &str)], head: &[(&str, &str)], rename: bool) -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    fs::write(repo.join(ELIDE_OLD), "// the renamed module\n").expect("rename の元を書ける");
    for (path, body) in base {
        fs::write(repo.join(path), body).expect("base の file を書ける");
    }
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "elide-base"]);
    let staged = state.join("head");
    fs::create_dir_all(&staged).expect("HEAD の写しの dir を作れる");
    let mut steps: Vec<String> = Vec::new();
    if rename {
        steps.push(format!("git mv {ELIDE_OLD} {ELIDE_NEW}"));
    }
    for (index, (path, body)) in head.iter().enumerate() {
        let copy = staged.join(index.to_string());
        fs::write(&copy, body).expect("HEAD の file を書ける");
        steps.push(format!("cp '{}' {path}", copy.display()));
    }
    steps.push("git add -A".to_owned());
    steps.push("git commit -q -m runner".to_owned());
    let mut listed: Vec<String> = vec![format!("\"{ELIDE_OLD}\"")];
    if rename {
        listed.push(format!("\"+{ELIDE_NEW}\""));
    }
    listed.extend(base.iter().map(|(path, _)| format!("\"{path}\"")));
    let write_set = format!("write-set = [{}]", listed.join(", "));
    let design = write_contract(&repo, &["write-set"], &[&write_set]);
    let id = intake(&repo, &state, &design);
    let out = spawn_with(&repo, &state, &id, &steps.join(" && "));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    (repo, state, id)
}

/// 畳みの歯の gate 1 回の観測（判定行・lens が読んだ stdin・生 diff・通知の行）。
struct ElideGate {
    /// toy repo。
    repo: PathBuf,
    /// 置き場。
    state: PathBuf,
    /// 便 id。
    id: String,
    /// gate の stdout の判定行。
    line: String,
    /// lens が読んだ stdin の全文。
    stdin: String,
    /// 便の生 diff（`HEAD~1..HEAD`）。
    raw: String,
    /// `verify.stderr.log` の通知の行。
    notices: Vec<String>,
}

/// [`elide_run`] の便を stdin を写す lens で 1 回 gate する（lens は diff で呼ばれ PASS）。
fn elide_gate(base: &[(&str, &str)], head: &[(&str, &str)], rename: bool) -> ElideGate {
    let (repo, state, id) = elide_run(base, head, rename);
    let seen = state.join("lens-stdin");
    let out = gate_once(&repo, &state, &id, Some(&recording_lens(&seen)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "lens は呼ばれ PASS: {}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(token_of(&line, "lens-input="), "diff", "前提: diff の周: {line}");
    let raw = raw_diff(&repo, &id);
    assert_eq!(raw.contains(&format!("rename from {ELIDE_OLD}\n")), rename, "前提: rename の対の有無: {raw}");
    ElideGate {
        stdin: fs::read_to_string(&seen).unwrap_or_default(),
        notices: notice_lines(&state, &id),
        raw,
        line,
        repo,
        state,
        id,
    }
}

/// 逐語の周の共通 assert: lens の stdin は生 diff そのもの（`kept` の行を含む・印が無い）・`bytes=` は生 diff の byte・
/// 通知は 1 行で `elided=` が無い（従来の字面）。
fn assert_elide_verbatim(gated: &ElideGate, kept: &str, why: &str) {
    assert!(gated.raw.contains(kept), "{why}: 前提: 生 diff が `{kept}` を持つ: {}", gated.raw);
    assert_eq!(gated.stdin, gated.raw, "{why}: lens の stdin は生 diff そのもの");
    assert!(!gated.stdin.contains(ELIDE_MARK_HEAD), "{why}: 印が無い: {}", gated.stdin);
    assert_eq!(token_of(&gated.line, "bytes="), gated.raw.len().to_string(), "{why}: bytes= は生 diff の byte");
    assert_eq!(gated.notices.len(), 1, "{why}: 通知は 1 行: {:?}", gated.notices);
    assert!(
        gated.notices.iter().all(|line| line.starts_with("# lens-input=diff reason=") && !line.contains("elided=")),
        "{why}: 従来の字面（elided= が無い）: {:?}",
        gated.notices
    );
    assert_eq!(value_of(&verdict_pairs(&gated.state, &gated.id), "diff_bytes"), gated.raw.len().to_string(), "{why}");
    clean(&[&gated.repo, &gated.state]);
}

// ───── 段ごとの対と移動で空になった dir の対（設計 gate-cost.md §42・行 ai・接頭辞 `pipe_gate_elide_` のまま） ─────

/// path を名指す 2 本目の row（[`elide_row`] と字面が違う・改行なし）。
fn elide_next_row(path: &str) -> String {
    format!("| next row also points at `{path}` |")
}

/// dir だけを名指す row（改行なし・`dir/` の形）。
fn elide_dir_row(dir: &str) -> String {
    format!("| teeth live under `{dir}/` |")
}

/// 畳まれた周の共通 assert: lens の stdin に `-N/+N` の印が在り、`kept`（置換後の行）が無く、通知は 1 行で
/// `elided=<notice>` で終わり、`bytes=` は lens に渡した本文の byte、`diff_bytes` は生 diff のまま。
fn assert_elide_folded(gated: &ElideGate, kept: &str, mark: &str, notice: &str) {
    assert!(gated.raw.contains(kept), "前提: 生 diff が `{kept}` を持つ: {}", gated.raw);
    assert!(gated.stdin.contains(&format!("\n~ rename の置換だけの hunk（{mark} 行）を省いた\n")), "{}", gated.stdin);
    assert!(!gated.stdin.contains(kept), "置換後の行は lens に渡らない: {}", gated.stdin);
    assert_eq!(gated.notices.len(), 1, "通知は 1 行: {:?}", gated.notices);
    assert!(
        gated.notices.iter().all(|line| line.starts_with("# lens-input=diff reason=") && line.ends_with(notice)),
        "通知に `{notice}`: {:?}",
        gated.notices
    );
    assert_eq!(token_of(&gated.line, "bytes="), gated.stdin.len().to_string(), "bytes= は畳んだ本文の byte");
    assert_eq!(value_of(&verdict_pairs(&gated.state, &gated.id), "diff_bytes"), gated.raw.len().to_string());
    clean(&[&gated.repo, &gated.state]);
}

/// 移動の歯の便を Implemented まで通す: base の file 群と `moves` の旧 path（中身は file ごとに違う）を commit し、runner は
/// `moves` を全部 `git mv` し、HEAD の file 群を写して commit する（write-set は対と base の file 群）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn elide_moves_run(base: &[(&str, &str)], head: &[(&str, &str)], moves: &[(&str, &str)]) -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    let mut written: Vec<(&str, String)> =
        moves.iter().enumerate().map(|(index, (old, _))| (*old, format!("// moved module {index}\n"))).collect();
    written.extend(base.iter().map(|(path, body)| (*path, (*body).to_owned())));
    for (path, body) in &written {
        let file = repo.join(path);
        fs::create_dir_all(file.parent().expect("file の親が在る")).expect("base の dir を作れる");
        fs::write(&file, body).expect("base の file を書ける");
    }
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "elide-moves-base"]);
    let staged = state.join("head");
    fs::create_dir_all(&staged).expect("HEAD の写しの dir を作れる");
    let mut steps: Vec<String> = Vec::new();
    for (old, new) in moves {
        let parent = Path::new(new).parent().expect("移動先の親が在る");
        steps.push(format!("mkdir -p {}", parent.display()));
        steps.push(format!("git mv {old} {new}"));
    }
    for (index, (path, body)) in head.iter().enumerate() {
        let copy = staged.join(index.to_string());
        fs::write(&copy, body).expect("HEAD の file を書ける");
        steps.push(format!("cp '{}' {path}", copy.display()));
    }
    steps.push("git add -A".to_owned());
    steps.push("git commit -q -m runner".to_owned());
    let mut listed: Vec<String> = Vec::new();
    for (old, new) in moves {
        listed.push(format!("\"{old}\""));
        listed.push(format!("\"+{new}\""));
    }
    listed.extend(base.iter().map(|(path, _)| format!("\"{path}\"")));
    let write_set = format!("write-set = [{}]", listed.join(", "));
    let design = write_contract(&repo, &["write-set"], &[&write_set]);
    let id = intake(&repo, &state, &design);
    let out = spawn_with(&repo, &state, &id, &steps.join(" && "));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    (repo, state, id)
}

/// [`elide_moves_run`] の便を stdin を写す lens で 1 回 gate する（lens は diff で呼ばれ PASS・rename の header は全部在る）。
fn elide_moves_gate(base: &[(&str, &str)], head: &[(&str, &str)], moves: &[(&str, &str)]) -> ElideGate {
    let (repo, state, id) = elide_moves_run(base, head, moves);
    let seen = state.join("lens-stdin");
    let out = gate_once(&repo, &state, &id, Some(&recording_lens(&seen)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "lens は呼ばれ PASS: {}", stderr_of(&out));
    let line = stdout_of(&out);
    assert_eq!(token_of(&line, "lens-input="), "diff", "前提: diff の周: {line}");
    let raw = raw_diff(&repo, &id);
    for (old, new) in moves {
        assert!(raw.contains(&format!("rename from {old}\nrename to {new}\n")), "前提: rename の対 {old}: {raw}");
    }
    ElideGate {
        stdin: fs::read_to_string(&seen).unwrap_or_default(),
        notices: notice_lines(&state, &id),
        raw,
        line,
        repo,
        state,
        id,
    }
}

/// 移動の歯の rename: `tests/` 配下の file 1 本を boundary 側の同じ相対 path へ。
const ELIDE_TESTS_MOVE: (&str, &str) = ("tests/gate.rs", "boundary/tests/gate.rs");
