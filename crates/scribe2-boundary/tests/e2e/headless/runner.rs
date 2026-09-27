// flip-check: moved s2-07l.675
//! runner の族の歯（接頭辞 `headless_runner_` / `runner_question_` / `runner_rate_` / `runner_prompt_`・設計
//! docs/design/carry-prep.md §8 行 f）。
//!
//! 共有の helper と const と外形 snapshot の歯（`headless_runner_prompt_external_form`・snapshot 名が module path を
//! 含むので動かさない）は親 module（`tests/e2e/headless.rs`）に在り、`use super::*` で使う。歯の本文は親から
//! **挙動不変で移した**もの（`s2-07l.675`）。

use super::*;

#[test]
fn headless_runner_reads_contract_from_stdin_and_passes_permission_mode_every_time() {
    let dir = tmp();
    let worktree = tmp();
    let account = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\ndocs/\n").expect("write-set を書ける");
    let contract = "goal = \"縦 1 本を通す\"\nverify = [\"true\"]\n";

    // **毎回**明示することを測るので、mode を変えて 2 度撃つ。1 度だけだと「既定が
    // たまたま一致していた」形と区別できない。
    for mode in ["acceptEdits", "plan"] {
        let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode, account: Some(&account) },
        contract.as_bytes(),
    );
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
        assert_runner_call(&dir, &worktree, &account, mode);
    }
    // **契約本文が prompt の構造へ触れられない**（置換は 1 走査）。契約は外から来る text で、
    // 重ねて replace すると契約の中の marker まで後段で展開される。
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        "goal = \"契約の中に {write_set} と書く\"\n".as_bytes(),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    assert!(prompt.contains("{write_set}"), "契約の中の marker はそのまま残る: {prompt}");
    assert_eq!(prompt.matches("src/lib.rs").count(), 1, "write-set の展開は 1 度だけ: {prompt}");

    // 前提違反は断る。**読めなかったものを空として続けない**（空の契約で claude を起こすと、
    // 何を作るのか分からないまま worktree を触らせることになる）。
    let empty = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"   \n",
    );
    assert_eq!(empty.status.code(), Some(i32::from(RC_REFUSED)), "空の契約は断る");
    let absent = dir.join("no-such-file");
    let missing = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &absent, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(missing.status.code(), Some(i32::from(RC_BROKEN)), "write-set を読めない周は rc 2");
    clean(&[&dir, &worktree, &account]);
}

/// **反転した歯**（`s2-07l.77`・[ADR-0012] §2.2）: 入れ子の `rate_limit_error` では**もう止まらない**。
///
/// 以前は record の字面に上限の語彙を当てていたので、この形で rc 75 になっていた。上限の真の
/// 合図は**専用 record（`rate_limit_event`）の構造化 status** で来ると実 run の現物で分かった
/// ため、字面照合は走査ごと撤去した。**歯を緩めたのではなく、測る対象が変わった**——この形は
/// もう「上限」ではないので、claude の rc をそのまま写す。
///
/// 効果でも測る: 以前は record を見た時点で claude を kill していたので `tail-ran` が残らなかった。
/// 止めなくなった今は fake が最後まで走る＝**痕跡が残る**。
///
/// [ADR-0012]: ../../../design-intent/decisions/ADR-0012-rate-limit-detection-reads-dedicated-record.html
#[test]
fn headless_runner_no_longer_stops_on_nested_rate_limit_error() {
    let dir = tmp();
    let worktree = tmp();
    let body = concat!(
        "{\"type\":\"system\",\"subtype\":\"init\"}\n",
        "{\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\"}}\n",
        "{\"type\":\"assistant\",\"text\":\"この先も読んでよい\"}\n"
    );
    let claude = fake_claude(&dir, body, true, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_ne!(out.status.code(), Some(i32::from(RC_RATE_LIMIT)), "字面では止まらない");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "claude の rc を写す: {}", stderr_of(&out));
    assert!(dir.join("tail-ran").exists(), "kill しないので fake は最後まで走る");
    clean(&[&dir, &worktree]);
}

/// (a) runner は rules 行 `runner.model` の model を claude に**毎回**渡す（`s2-07l.297`・設計 pipeline.md §6・FR5）:
/// `--rules` の manifest の値が `opus` なら argv に `--model opus` の対・値を `sonnet` に変えると対の値も変わる
/// （値は行から来る＝定数ではない）・表示名 `Opus` で書いた行も CLI の別名 `opus` で渡る（[`Model::parse`] →
/// `alias`）・`--rules` 無しは埋め込みの行（`opus`）。base は `--model` を渡さないので RED。
#[test]
fn headless_runner_passes_model_from_rules_row() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let call = RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None };
    for (value, want) in [("opus", "opus"), ("sonnet", "sonnet"), ("Opus", "opus"), ("Fable", "fable")] {
        let rules = rules_with_rows(&dir, &format!("rules-model-{value}.toml"), &[model_row(value), effort_row(RUNNER_EFFORT)]);
        let out = run_runner_with_rules(&call, &rules, b"goal = \"x\"\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{value}: {}", stderr_of(&out));
        assert_eq!(model_arg(&dir), Some(want.to_owned()), "{value}: 行の値を CLI の別名で渡す: {}", slurp(&dir.join("args")));
        let args = slurp(&dir.join("args"));
        assert_eq!(args.lines().filter(|line| *line == "--model").count(), 1, "{value}: 対は 1 つ: {args}");
        assert!(!has_arg(&args, "--rules"), "{value}: --rules は claude へ渡らない: {args}");
        fs::remove_file(dir.join("args")).expect("前の周の写しを消せる");
    }
    // `--rules` 無しは埋め込みの manifest の行（`fleet usage` の refresh とは違い、runner は必ず渡す）。
    let out = run_runner(&call, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert_eq!(model_arg(&dir), Some(RUNNER_MODEL.to_owned()), "埋め込みの行: {}", slurp(&dir.join("args")));
    clean(&[&dir, &worktree]);
}

/// (c) `runner.model` の行が解けない周（無い / 不発効 / 文字列でない / 閉じた表に無い値）は runner が **claude を
/// 呼ばず rc 2** で理由を 1 行（`runner: runner.model …`・lens の cap と同じ極性）。
///
/// base の断りと弁別する: base は `--rules` を知らずに読み飛ばし claude を起こす（argv の写しが**生成される**）。
/// 本歯は写しの不在と stderr の字面（`runner.model が無い`＝「未知の flag」ではない）の両方で測る。
#[test]
fn headless_runner_refuses_when_model_row_is_missing() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let call = RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None };
    let absent = rules_with_rows(&dir, "absent.toml", &[cap_row(4096)]);
    let disabled = rules_with_row(&dir, "disabled.toml", "id = \"runner.model\"\nkind = \"RunnerModel\"\nvalue = \"opus\"\nenabled = false\n");
    // id は同じで kind が整数の行（manifest は id と kind の対応を照合しない）＝値が文字列でない形。
    let int = rules_with_row(&dir, "int.toml", "id = \"runner.model\"\nkind = \"GateTokenCap\"\nvalue = 5\nenabled = true\n");
    let unknown = rules_with_rows(&dir, "unknown.toml", &[model_row("nope")]);
    let missing = dir.join("no-such-rules.toml");
    for (rules, want) in [
        (&absent, "runner.model が無い"),
        (&disabled, "runner.model は不発効である"),
        (&int, "runner.model が文字列でない"),
        (&unknown, "runner.model の値 nope は未知の model"),
        (&missing, "rules を読めない"),
    ] {
        let out = run_runner_with_rules(&call, rules, b"goal = \"x\"\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{want}: rc 2 / {}", stderr_of(&out));
        assert!(!dir.join("called").exists(), "{want}: claude を 1 度も起動しない");
        assert!(!dir.join("args").exists(), "{want}: argv の写しは生成されない");
        let err = stderr_of(&out);
        assert!(err.contains(&format!("runner: {want}")), "{want}: 理由を 1 行で名乗る: {err}");
        assert!(!err.contains("未知の引数") && !err.contains("usage: "), "{want}: 未知の flag の断りではない: {err}");
        assert_eq!(err.lines().count(), 1, "{want}: stderr は理由の 1 行だけ: {err}");
        assert!(stdout_of(&out).is_empty(), "{want}: stdout には何も出さない: {}", stdout_of(&out));
    }
    // `--rules` の値欠けは入口の閉包の断り（rc 2・usage・設計 pipeline.md §14 約束 4）。
    let mut args = runner_args(&call, &dir);
    args.push("--rules".to_owned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_bin(&worktree, &refs, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "値欠け: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("--rules に値が無い"), "{}", stderr_of(&out));
    assert!(stderr_of(&out).contains("[--rules PATH]"), "usage は --rules を載せる: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    clean(&[&dir, &worktree]);
}

/// (a) runner は rules 行 `runner.effort` の effort を claude に**毎回**渡す（`s2-07l.322`・設計 pipeline.md §6・FR5）:
/// `--rules` の manifest の値ごとに argv の `--effort` の対の値が変わり（値は行から来る＝定数ではない）、対は 1 つ・
/// 置き場は `--model` の対の直後・`--rules` 無しは埋め込みの行（`high`）。base は `--effort` を渡さないので RED。
#[test]
fn headless_runner_passes_effort_from_rules_row() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let call = RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None };
    for value in ["high", "low", "xhigh", "medium"] {
        let rules = rules_with_rows(&dir, &format!("rules-effort-{value}.toml"), &[model_row(RUNNER_MODEL), effort_row(value)]);
        let out = run_runner_with_rules(&call, &rules, b"goal = \"x\"\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{value}: {}", stderr_of(&out));
        assert_eq!(effort_arg(&dir), Some(value.to_owned()), "{value}: 行の値を渡す: {}", slurp(&dir.join("args")));
        let args = slurp(&dir.join("args"));
        assert_eq!(args.lines().filter(|line| *line == "--effort").count(), 1, "{value}: 対は 1 つ: {args}");
        let lines: Vec<&str> = args.lines().collect();
        let at_model = lines.iter().position(|line| *line == "--model");
        let at_effort = lines.iter().position(|line| *line == "--effort");
        assert_eq!(at_effort, at_model.map(|at| at + 2), "{value}: model の対の直後: {args}");
        fs::remove_file(dir.join("args")).expect("前の周の写しを消せる");
    }
    // `--rules` 無しは埋め込みの manifest の行（裁定 `user 2026-09-15T03:52Z` の high）。
    let out = run_runner(&call, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert_eq!(effort_arg(&dir), Some(RUNNER_EFFORT.to_owned()), "埋め込みの行: {}", slurp(&dir.join("args")));
    clean(&[&dir, &worktree]);
}

/// 席の pane の中から `runner` を**単体起動**（`pipe spawn` を通さない・裁定 (E)）しても claude の env に
/// `TMUX_PANE` が無く `PATH` は継承される（設計 seat-roles.md §4 行 c・ADR-0022 §2.3・FR40 / FR21）。
/// base は `wrap_command` が外さないので [`PARENT_PANE`] が写しに在る（RED）。
#[test]
fn headless_runner_drops_tmux_pane() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "runner は claude を呼ぶ");
    assert_pane_dropped(&dir, "runner");
    clean(&[&dir, &worktree]);
}

#[test]
fn headless_runner_mirrors_claude_rc() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    // **包みが rc を作り替えない**（設計 §6）。呼出側は runner の rc で便の成否を読むので、
    // ここで潰すと失敗した実装便が成功として通る。
    for want in [0_u8, 1, 3] {
        let claude = fake_claude(&dir, "", false, want);
        let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
        assert_eq!(out.status.code(), Some(i32::from(want)), "claude の rc をそのまま写す");
        // 外形の 1 行も測る（C12.5）。rc だけ合っていても、呼出側が読む行が消えては困る。
        assert!(
            stdout_of(&out).contains(&format!("runner: rc={want} records=")),
            "1 行で rc と record 数を名乗る: {}",
            stdout_of(&out)
        );
    }
    clean(&[&dir, &worktree]);
}

#[test]
fn headless_runner_does_not_stop_on_quoted_rate_limit_words() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    // **応答が上限の語を引用しただけ**の record。error を名乗っていないので上限ではない。
    // この歯が無いと、契約の文言を復唱しただけで便が rc 75 で死ぬ（本 bead の契約自身が
    // その文言を含むので、自分で自分を踏む）。
    let body = concat!(
        "{\"type\":\"assistant\",\"message\":{\"text\":\"契約は rate_limit の error record で rc 75 と述べている\"}}\n",
        "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false}\n"
    );
    let claude = fake_claude(&dir, body, false, 0);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_ne!(out.status.code(), Some(i32::from(RC_RATE_LIMIT)), "引用は上限ではない");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "claude の rc を写す: {}", stderr_of(&out));
    clean(&[&dir, &worktree]);
}

/// **反転した歯**（`s2-07l.77`・ADR-0012 §2.2）: error record の本文に上限の語彙が在っても
/// **もう止まらない**。判定の入力は `rate_limit_event` の status だけになった。
#[test]
fn headless_runner_no_longer_stops_on_error_records_with_limit_words() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    for body in [
        "{\"type\":\"result\",\"is_error\":true,\"result\":\"usage limit reached\"}\n",
        "{\"type\":\"error\",\"status\":429}\n",
        "{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"result\":\"overloaded\"}\n",
    ] {
        // **rc を写す**ことを測る（fake の rc を 0 以外にする＝「常に 0」と弁別できる形）。
        let claude = fake_claude(&dir, body, false, 3);
        let out = run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
            b"goal = \"x\"\n",
        );
        assert_eq!(
            out.status.code(),
            Some(3),
            "上限 record 以外は claude の rc を写す: {body}"
        );
    }
    clean(&[&dir, &worktree]);
}

/// **`--vessel` は必須**（lens の `--contract` と同じ極性）。権限の出所が無いまま
/// claude を起こすと、起動口座の settings を継承した席が worktree を触る。
#[test]
fn headless_runner_requires_vessel_flag() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let out = run_bin(
        &worktree,
        &[
            "runner",
            "--worktree",
            &worktree.display().to_string(),
            "--write-set",
            &write_set.display().to_string(),
            "--plugin-dir",
            &dir.display().to_string(),
            "--permission-mode",
            "plan",
            "--claude",
            &claude.display().to_string(),
        ],
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "--vessel 無しは rc 1");
    assert!(stderr_of(&out).contains("--vessel"), "何が要るかを名指す: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "claude を起動しない");

    // **「無い」と「壊れている」で極性を変える**（lens の契約と同じ）。
    let absent = dir.join("no-such-vessel.toml");
    let broken = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &absent, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(broken.status.code(), Some(i32::from(RC_BROKEN)), "読めない写しは rc 2");
    assert!(!dir.join("called").exists(), "読めない写しでも claude を起動しない");
    // 束縛だけの vessel を使う（正常形は次の歯が測る）。
    assert!(vessel.exists(), "写しは在る");
    clean(&[&dir, &worktree]);
}

/// 権限は**便の写しから**組む（manifest も repo の宣言も読まない）。
#[test]
fn headless_runner_passes_allowed_tools_from_vessel_copy() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let args = slurp(&dir.join("args"));
    assert!(pair(&args, "--setting-sources", ""), "settings は 1 つも読まない（ADR-0011 §2.1）: {args}");
    assert!(
        pair(&args, "--allowedTools", "Bash(cargo:*),Bash(git:*)"),
        "写しの allowlist を Bash(<cmd>:*) で与える: {args}"
    );

    // **写しが変われば権限も変わる**＝上限（manifest の cargo / git）を読んでいない。
    let narrow = write_vessel_copy(&dir, r#"["git"]"#);
    let again = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &narrow, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&again));
    let narrowed = slurp(&dir.join("args"));
    assert!(pair(&narrowed, "--allowedTools", "Bash(git:*)"), "狭めた写しに従う: {narrowed}");
    assert!(!narrowed.contains("cargo"), "写しに無い command は与えない: {narrowed}");
    clean(&[&dir, &worktree]);
}

/// runner が起こす claude は **口座の settings も対象 repo の settings も 1 つも読まない**
/// （ADR-0011 §2.1）。
///
/// `--setting-sources project` は「起動口座を継承しない」までしか塞げず、**対象 repo の
/// `.claude/settings.json` の allow 規則が残る**＝便ごとに凍結した allowlist（ADR-0010 §2.4）を
/// 実装対象の repo 側から広げられる（`.56` の lens L4）。空の値は user / project / local の
/// **どれも読まない**の意味である。
///
/// **値まで測る**のが要点で、`--setting-sources` が在ることだけを見る歯は値が `project` でも
/// 緑になる（契約が名指した禁止形）。
#[test]
fn headless_runner_loads_no_settings_from_account_or_checkout() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let vessel = write_vessel_copy(&dir, r#"["cargo"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let args = slurp(&dir.join("args"));
    assert!(pair(&args, "--setting-sources", ""), "空の値を **対**で渡す: {args}");
    // **どの源も名指されていない**（1 つでも残ると、その面の settings が読まれる）。
    for source in ["project", "user", "local"] {
        assert!(!pair(&args, "--setting-sources", source), "{source} の settings を読まない: {args}");
    }
    assert!(has_arg(&args, "--strict-mcp-config"), "MCP も宣言外を拾わない: {args}");
    // 別 seam で settings を戻す形（ADR-0011 §4 (a) / (b)）を渡していない。
    assert!(!has_arg(&args, "--settings"), "settings を file で渡し直さない: {args}");
    assert!(!has_arg(&args, "--restricted"), "restricted は使わない: {args}");
    clean(&[&dir, &worktree]);
}

/// runner も lens も **`--mcp-config` を渡さない**（`.64` の lens LOW-4）。
///
/// 既存の歯は `--strict-mcp-config` の**存在**しか見ないので、宣言 file を足す変異
/// （`--mcp-config <file>`＝strict のまま server を 1 つ載せる形）が緑のまま通る。
/// `--strict-mcp-config` の存在は既存の歯が持つので、ここでは**不在だけ**を測る
/// （分離形と連結形の両方＝[`has_arg`]）。
#[test]
fn headless_runner_and_lens_pass_no_mcp_config_absent_from_argv() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let vessel = write_vessel_copy(&dir, r#"["cargo"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let runner_args = slurp(&dir.join("args"));
    assert!(!has_arg(&runner_args, "--mcp-config"), "runner は MCP の宣言 file を渡さない: {runner_args}");

    let lens_dir = tmp();
    let lens_claude = fake_claude(&lens_dir, "{\"verdict\":\"PASS\",\"evidence\":\"ok\"}\n", false, 0);
    let contract = contract_in(&lens_dir);
    let seen = run_lens(&contract, 4096, "plan", &lens_claude, b"--- a\n+++ b\n");
    assert_eq!(seen.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&seen));
    let lens_args = slurp(&lens_dir.join("args"));
    assert!(!has_arg(&lens_args, "--mcp-config"), "lens は MCP の宣言 file を渡さない: {lens_args}");
    clean(&[&dir, &worktree, &lens_dir]);
}

/// **id の乱数に上限の語が現れただけ**では止まらない（`s2-07l.71`）。
///
/// `.64` の実 run で実際に踏んだ形: 401 authentication_failed の result record は
/// `is_error` を名乗るので種別の絞りは通り、そのうえで claude が振った `session_id` の
/// 16 進に `429` が部分文字列として現れたため、**認証エラーが rc 75「上限」に化けた**
/// （呼出側は `Failed detail=rate-limit` と記帳する＝便の失敗原因が台帳に嘘で残る）。
///
/// **本文 field に上限の語が 1 つも無い**ことが要点で、id 側にだけ置く。
#[test]
fn headless_runner_does_not_stop_on_rate_limit_word_in_session_id() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    for body in [
        // 実 run で観測した形（口座名・API key・path は含まない・id は同形の別値）。
        concat!(
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,",
            "\"result\":\"Failed to authenticate. API Error: 401 API key is invalid.\",",
            "\"session_id\":\"791e7ee4-d00f-4dbc-8429-dd54dcbd20c4\",",
            "\"uuid\":\"c719aad3-e1db-42c9-a654-cae6bf758981\"}\n"
        ),
        // **本文 field を 1 つも持たない** error record は「上限ではない」へ倒す
        // （分からない周を上限と名乗らない＝上限側を狭く取る）。
        "{\"type\":\"error\",\"session_id\":\"00000000-0000-4000-8000-000000000529\"}\n",
    ] {
        // claude 自身の rc は 1（認証で落ちた）。**その rc が写ること**まで測る——
        // 「rc 75 でない」だけだと、包みが独自の rc を作る変異が生き残る。
        let claude = fake_claude(&dir, body, false, 1);
        let out = run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
            b"goal = \"x\"\n",
        );
        assert_ne!(
            out.status.code(),
            Some(i32::from(RC_RATE_LIMIT)),
            "id の乱数に現れた上限語は上限ではない: {body}"
        );
        assert_eq!(out.status.code(), Some(1), "claude の rc を写す: {body} / {}", stderr_of(&out));
    }
    clean(&[&dir, &worktree]);
}

/// **反転した歯**（`s2-07l.77`・ADR-0012 §2.2）: 本文 field に上限の語彙が在っても**もう止まらない**。
///
/// `s2-07l.71` / `.73` で「どこを走査するか」を 2 度狭めた形は、走査ごと撤去された。**狭める
/// 努力が無駄だったのではなく**、狭めても誤爆が残るという事実が「本物の合図を見ていない」ことの
/// 証拠になり、現物を採りに行く判断（`s2-07l.67` の契約）を正当化した。
#[test]
fn headless_runner_no_longer_stops_on_limit_words_in_body_fields() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    for body in [
        "{\"type\":\"result\",\"is_error\":true,\"result\":\"API Error: 429 rate limit\"}\n",
        "{\"type\":\"system\",\"subtype\":\"error\",\"message\":\"429 Too Many Requests\"}\n",
        "{\"type\":\"error\",\"text\":\"upstream returned 429\"}\n",
    ] {
        // **rc を写す**ことを測る（rc 0 だと「常に 0」を返す実装と弁別できない）。
        let claude = fake_claude(&dir, body, false, 3);
        let out = run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
            b"goal = \"x\"\n",
        );
        assert_eq!(out.status.code(), Some(3), "本文の語彙では止まらない（rc を写す）: {body}");
    }
    clean(&[&dir, &worktree]);
}

/// **tool の失敗出力に上限語彙が現れても上限ではない**（`s2-07l.73`）。
///
/// claude は tool の失敗を会話 record（`type=user`）の中の `tool_result` block として流し、その
/// block が `"is_error":true` を持つ。`.71` までは record 種別を見ずに字面で `is_error` を拾って
/// いたので、**cargo や grep の出力に `429` が混じるだけで便が「上限」で死ぬ**（planner と当席が
/// 独立に実 binary で再現・2026-09-11）。とくに実 run で cargo を撃たせる便では、失敗した
/// Bash tool の出力がそのまま「上限で止まった」と記帳される。
#[test]
fn headless_runner_does_not_stop_on_rate_limit_word_in_tool_result() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    for body in [
        // 会話 record が運ぶ tool 出力（上限語彙も is_error も**入れ子の中**に在る）。
        concat!(
            "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"tool_result\",",
            "\"content\":\"error: 429 tests failed; see log\",\"is_error\":true}]}}\n"
        ),
        // **種別を読めない record**（`type` が無い）は「上限ではない」へ倒す＝分からない周を
        // 上限と名乗らない（誤認すると失敗原因が台帳から消える）。
        "{\"is_error\":true,\"result\":\"429 rate limit\"}\n",
    ] {
        // claude の rc は 1。**その rc が写ること**まで測る（rc 75 でないだけでは足りない）。
        let claude = fake_claude(&dir, body, false, 1);
        let out = run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
            b"goal = \"x\"\n",
        );
        assert_ne!(
            out.status.code(),
            Some(i32::from(RC_RATE_LIMIT)),
            "tool 出力の上限語彙は上限ではない: {body}"
        );
        assert_eq!(out.status.code(), Some(1), "claude の rc を写す: {body} / {}", stderr_of(&out));
    }
    clean(&[&dir, &worktree]);
}

/// **反転した歯**（`s2-07l.77`・ADR-0012 §2.2）: 入れ子に終端種別の字面が在る record も、
/// 配列や escape を跨いだ先に種別が在る record も、**もう上限として扱わない**。
///
/// ★契約が挙げた「反転する 3 本」に本 1 本が漏れていた（`s2-07l.73` で**両向き**にした歯で、
/// rc 75 側の 2 形が撤去された走査〔深さ数え〕を測っていたため）。設計の変更に伴う反転であって
/// 歯を緩めたのではない。
#[test]
fn headless_runner_no_longer_stops_on_records_with_nested_type_markers() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    for body in [
        concat!(
            "{\"message\":{\"content\":[{\"type\":\"error\",\"content\":\"429 rate limit\"}],",
            "\"is_error\":true},\"type\":\"user\"}\n"
        ),
        "{\"content\":[\"first\",\"second\"],\"type\":\"error\",\"status\":429}\n",
        "{\"result\":\"said \\\"429 rate limit\\\" once\",\"type\":\"result\",\"is_error\":true}\n",
    ] {
        let claude = fake_claude(&dir, body, false, 1);
        let out = run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
            b"goal = \"x\"\n",
        );
        assert_eq!(out.status.code(), Some(1), "claude の rc を写す: {body}");
    }
    clean(&[&dir, &worktree]);
}

/// prompt は**便の写しの allowlist をそのまま運ぶ**（`s2-07l.67`）。
///
/// 実 run の実測（過去 4 run）では cargo 呼出 9〜12 件が**全件「承認要求」で止まり出力 0 件**
/// だった＝器は一度も cargo を回せていない。`--allowedTools` で allow は与えていたが、**prompt が
/// 「何を撃ってよいか」「1 command で撃つ」を伝えていなかった**ため、実装役が pipe や `&&` で
/// 繋いだ形を撃ち、allow の外として止まっていた。
///
/// **写しの 2 command が両方載ること**を測る（1 つだけ見る歯は、写しの一部が落ちても緑になる）。
#[test]
fn headless_runner_prompt_lists_allowed_commands() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let claude = fake_claude(&dir, "", false, 0);
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    // **2 つとも「1 行 1 command」で載る**（値の出所は写し・ADR-0010 §2.4）。行で測るのが要点で、
    // `contains("- cargo")` だけだと区切りを `, ` へ変える変異が緑のまま通る（lens 2026-09-11 で実測）。
    assert!(
        prompt.contains("- cargo\n- git"),
        "写しの 2 command が 1 行 1 command で並ぶ: {prompt}"
    );
    // 規律を**極性ごと**測る。字面 2 つだけを見る歯は「一覧は参考。他も試してよい」への反転を
    // 素通しする（lens 2026-09-11 で実測）＝**排他の語と、繋がない形の列挙**まで見る。
    assert!(prompt.contains("実行してよい command"), "allowlist の節が在る: {prompt}");
    assert!(
        prompt.contains("**だけ**を実行してよい"),
        "一覧が**排他**であること（参考ではない）を言う: {prompt}"
    );
    assert!(prompt.contains("1 command"), "1 command で撃つ規律を運ぶ: {prompt}");
    // 実 run で唯一 deny された形（`$(…)`）まで名指す。実測で落ちた形が prompt に無いと、
    // 次の run も同じところで止まる。
    // **命令の字面**で測る（`contains("$(")` だけだと、実測の引用に `$(` が残っている限り
    // 命令行を消しても緑になる＝lens 対応で足した assert 自身が空虚だった・実測で確認した）。
    assert!(
        prompt.contains("command 置換"),
        "静的解析できない形（command 置換）を名指して禁じる: {prompt}"
    );
    for form in ["$(", "&&", "|"] {
        assert!(
            prompt.contains(form),
            "繋がない・置換しない形として {form} を名指す: {prompt}"
        );
    }
    // **写しに無い command は現れない**（manifest の上限や repo の宣言を読んでいない）。
    assert!(!prompt.contains("npm"), "写しに無い command 名は載せない: {prompt}");
    // **狭めた写しに従う**（値が固定の字面でなく写し由来であることの対）。
    let narrow = write_vessel_copy(&dir, r#"["git"]"#);
    let again = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &narrow, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&again));
    let narrowed = slurp(&dir.join("stdin"));
    assert!(narrowed.contains("- git"), "狭めた写しの command は載る: {narrowed}");
    assert!(!narrowed.contains("- cargo"), "写しから消えた command は載らない: {narrowed}");
    clean(&[&dir, &worktree]);
}

/// 契約本文の中の `{allowed}` は**展開されない**（`s2-07l.67`・`{write_set}` と同じ極性）。
///
/// 契約は外から来る text なので、重ねて `replace` すると契約に 1 語書くだけで prompt の
/// allowlist 節へ触れられる（自分の権限一覧を自分で書き換えられる）。**3 対を 1 走査**で
/// 埋めることでその経路を塞ぐ。
#[test]
fn headless_runner_prompt_does_not_expand_allowed_placeholder_from_contract() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    // write-set 側にも穴の字面を置く（埋めた値を二度と走査しないことの対）。
    fs::write(&write_set, "src/lib.rs\n{allowed}\n").expect("write-set を書ける");
    let claude = fake_claude(&dir, "", false, 0);
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        // byte string は ASCII だけ（`b"..."` に非 ASCII は置けない）。
        b"goal = \"hole {allowed} stays here\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    // 契約本文と write-set の穴は**そのまま残る**。
    assert!(
        prompt.contains("hole {allowed} stays here"),
        "契約本文の穴は展開されない: {prompt}"
    );
    assert_eq!(prompt.matches("{allowed}").count(), 2, "残る穴は契約と write-set の 2 つだけ: {prompt}");
    // **展開は 1 度だけ**（穴が 2 つ余計に展開されていれば command 名の出現が増える）。
    assert_eq!(prompt.matches("- cargo").count(), 1, "allowlist の展開は 1 度だけ: {prompt}");
    clean(&[&dir, &worktree]);
}

/// 雛形は turn の終端の規律を**逐語の 1 行**で運ぶ（設計 pipeline.md §20 の約束 1 / 2・`s2-07l.275`）。
///
/// `.270` run 1 の実測: runner が「flip-check を背景で回している・完了通知を待つ」と言って rc 0 で turn を
/// 閉じ、背景の task は scope の片付けで殺された（自己申告の done が背景 task の完了を含まない）。
///
/// 外形の `.snap` は入口の flip の test 区間に入らない（snapshot は歯ではない）ので、雛形の RED は
/// **この逐語**を名指すこの歯で測る。規律を薄める改変（「なるべく前面で」等）はここで落ちる。
#[test]
fn headless_runner_prompt_closes_turn_after_the_foreground_verify() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let claude = fake_claude(&dir, "", false, 0);
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    // **逐語で 1 本**（言い換えでも 2 行に割った形でも RED）。
    assert_eq!(
        prompt.matches(TURN_DISCIPLINE).count(),
        1,
        "終端の規律が逐語でちょうど 1 本在る: {prompt}"
    );
    // 規律の節（「守ること」）の中の**独立した 1 行**である（他の行の尾に足した形は落ちる）。
    let line = prompt.lines().find(|line| line.contains(TURN_DISCIPLINE)).unwrap_or_default();
    assert_eq!(line, format!("- {TURN_DISCIPLINE}。"), "節の 1 項目として在る: {prompt}");
    clean(&[&dir, &worktree]);
}

/// **上限 record でも、集合に無い status では止めない**（`s2-07l.77`・ADR-0012 §2.1）。
///
/// 実測で採れた唯一の status は `allowed_warning`（許可されつつ警告）で、これは**止める側では
/// ない**。止める status の集合は**空**なので、器は当面 rc 75 を立てない——ADR の**決定**であって
/// 実装の手抜きではない（未採取の値を推測で足すと ADR §4 案 (A') へ戻る）。
#[test]
fn headless_runner_does_not_stop_on_rate_limit_event_with_an_unlisted_status() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    // 実 run で採取した現物と同じ形（uuid / session_id は別値）。
    let body = concat!(
        "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed_warning\",",
        "\"rateLimitType\":\"seven_day\",\"utilization\":0.97,\"isUsingOverage\":false},",
        "\"uuid\":\"11111111-2222-4333-8444-555555555555\"}\n"
    );
    let claude = fake_claude(&dir, body, false, 0);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_ne!(out.status.code(), Some(i32::from(RC_RATE_LIMIT)), "集合に無い status では止めない");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "claude の rc を写す: {}", stderr_of(&out));
    // **utilization 0.97 でも止めない**（閾値で止めない・ADR §2.3・憲法 C9.2）。
    clean(&[&dir, &worktree]);
}

/// 止める判断は**純関数**で両向きに測る（`s2-07l.77`・ADR-0012 §2.1）。
///
/// 集合が空である以上、production の経路は**止まる側を一度も通らない**。純関数にしないと
/// 「上限で止まる分岐」に歯が 1 本も当たらないので、ここでは**非空の集合**を渡して測る。
#[test]
fn headless_runner_stops_only_on_statuses_in_the_stop_set() {
    let stop = ["blocked", "rejected"];
    assert!(vessel::headless::runner::stops_on("blocked", &stop), "集合の値では止まる");
    assert!(vessel::headless::runner::stops_on("rejected", &stop), "集合の値は 1 つに限らない");
    assert!(!vessel::headless::runner::stops_on("allowed_warning", &stop), "集合に無い値では止まらない");
    // **未知も止めない**（未採取の値を上限へ倒さない＝誤って健全な便を殺さない）。
    assert!(!vessel::headless::runner::stops_on("some_new_status", &stop), "未知の status では止まらない");
    // 空の集合（現行）はどの status でも止めない。
    assert!(!vessel::headless::runner::stops_on("blocked", &[]), "空の集合では止まらない");
}

/// **観測した status は記録に残す**（`s2-07l.77`・ADR-0012 §2.1 末尾）。
///
/// 集合を**実測で育てる唯一の口**である。これが無いと「実測で採れた値だけを入れる」が運用で
/// 回らず、集合は永久に空のままになる。記録は**判定の入力ではない**ので、rc は変わらない。
#[test]
fn headless_runner_records_the_observed_rate_limit_status() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let body = "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed_warning\"}}\n";
    let claude = fake_claude(&dir, body, false, 0);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert!(
        stdout_of(&out).contains("rate-limit-status=allowed_warning"),
        "観測した status を記録面へ載せる: {}",
        stdout_of(&out)
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "記録は rc を変えない");
    // 上限 record が流れない周は記録も出ない（無いものを名乗らない）。
    let quiet = fake_claude(&dir, "{\"type\":\"result\",\"is_error\":false}\n", false, 0);
    let again = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &quiet, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert!(!stdout_of(&again).contains("rate-limit-status="), "観測していない周は書かない: {}", stdout_of(&again));
    clean(&[&dir, &worktree]);
}

/// **`s2-07l.76` の症状が消えたことの裏取り**（`s2-07l.77` の契約 (7)）。
///
/// hook の失敗を伝える `system` record の文面に上限語が混じっても、もう rc 75 にならない。
/// **code の不在を字面で数えず、消えた結果の挙動で測る**。
#[test]
fn headless_runner_no_longer_stops_on_system_record_with_limit_words() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let body = "{\"type\":\"system\",\"subtype\":\"error\",\"message\":\"hook failed: 429 Too Many Requests\"}\n";
    let claude = fake_claude(&dir, body, false, 1);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(1), "claude の rc を写す: {}", stderr_of(&out));
    clean(&[&dir, &worktree]);
}

/// **`s2-07l.74` の症状が消えたことの裏取り**（`s2-07l.77` の契約 (7)）。
///
/// 未閉じの入れ子を大量に持つ病的な 1 行でも、上限 record でなければ**中身を見ない**。
/// **時間は測らない**（環境差で揺れる）——走査が無くなったことは「その record を検査しない」
/// という挙動で表れる。
#[test]
fn headless_runner_no_longer_scans_pathological_records() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    // 未閉じの入れ子 20000 個（旧実装はここで二次の走査に入った）。上限語も混ぜる。
    let pathological = format!(
        "{{\"type\":\"error\",\"is_error\":true,{}\"result\":\"429 rate limit\"\n",
        "\"error\":[".repeat(20000)
    );
    let claude = fake_claude(&dir, &pathological, false, 1);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(1), "上限 record でない行は中身を見ない: {}", stderr_of(&out));
    clean(&[&dir, &worktree]);
}

/// **record の形をしていない行は読まない**（`s2-07l.77`・変異で生存した分岐に歯を当てる）。
///
/// 判定の入力は「claude が出した 1 record」であって「上限 record に言及した文字列」ではない。
/// 行頭が `{` でない行（log の前置きが付いた行・本文が record を引用した行）は、marker を
/// 含んでいても **status を読まない**——これが崩れると、tool の出力が上限 record を引用した
/// 周に器が反応する（集合が空の現在は止まらないが、値を入れた周に誤停止へ育つ）。
#[test]
fn headless_runner_does_not_read_a_rate_limit_record_from_plain_text() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    // 行頭が `{` でない（前置きが付いた）が marker と status を含む行。
    let body = concat!(
        "log: {\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed_warning\"}}\n",
        "{\"type\":\"result\",\"is_error\":false}\n"
    );
    let claude = fake_claude(&dir, body, false, 0);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert!(
        !stdout_of(&out).contains("rate-limit-status="),
        "record の形をしていない行から status を読まない: {}",
        stdout_of(&out)
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc は claude のもの");
    clean(&[&dir, &worktree]);
}

/// **止める側の分岐**を測る（`s2-07l.77`・lens 2026-09-11 H1）。
///
/// 集合が空である以上 production は `Stop` を通らないので、**判定を 3 値の純関数へ切り出して**
/// 非空の集合で測る。これが無いと「上限で止める」側は 1 本も測られない。
#[test]
fn headless_runner_decides_stop_only_for_statuses_in_the_set() {
    use vessel::headless::runner::{decide, stop_line, Decision};
    let event = "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"blocked\"}}";
    // 集合に在る status → **止める**。
    assert_eq!(decide(event, &["blocked"]), Decision::Stop("blocked".to_owned()));
    // 集合に無い status → 記録だけ（止めない）。
    assert_eq!(decide(event, &["exceeded"]), Decision::Observed("blocked".to_owned()));
    // 現行（空の集合）→ 記録だけ。
    assert_eq!(decide(event, &[]), Decision::Observed("blocked".to_owned()));
    // 上限 record でない行 → 何もしない。
    assert_eq!(decide("{\"type\":\"result\",\"is_error\":true}", &["blocked"]), Decision::Ignore);
    // 止めた周の 1 行は **status を載せる**（後から「何で止まったか」を読めるように）。
    assert!(stop_line("blocked").contains("rate-limit-status=blocked"), "止めた理由を記録面と同じ形で載せる");
}

/// **上限で止めた周の停止行は stdout に出る**（`s2-07l.190`・設計 account-autonomy.md §2）。
///
/// pipe は runner の stdout だけを捕らえるので、stderr に出すと段の記帳へ status が届かない。
/// 止める側の集合が空である以上 production の binary は止まる側を通らないので、結果を組む口を
/// 直接撃つ（`conclude` はこの 1 本を返す）。
#[test]
fn headless_runner_limited_puts_the_stop_line_on_stdout() {
    use vessel::headless::runner::{limited, stop_line, stop_status};
    let out = limited("allowed_warning");
    assert_eq!(out.out, vec![stop_line("allowed_warning")], "停止行は stdout の 1 行");
    assert!(out.err.is_empty(), "stderr には出さない: {:?}", out.err);
    assert_eq!(out.rc, RC_RATE_LIMIT, "rc は RC_RATE_LIMIT のまま");
    assert_eq!(
        out.out.last().map(String::as_str).and_then(stop_status),
        Some("allowed_warning"),
        "pipe の読み手で往復する"
    );
}

/// key と colon の間の**空白に寛容**である（`s2-07l.77`・lens 2026-09-11 H2）。
///
/// 実 stream は compact だが（実測）、表記が変わっただけで**記録の口が無音で止まる**形にはしない
/// ——ADR §2.1 の「観測した status を残す」は、読めなければ一度も発火しない。
#[test]
fn headless_runner_reads_the_status_with_spaces_around_colons() {
    use vessel::headless::runner::rate_limit_status;
    let spaced = "{\"type\": \"rate_limit_event\", \"rate_limit_info\": {\"status\": \"allowed_warning\"}}";
    assert_eq!(rate_limit_status(spaced), Some("allowed_warning"), "空白入りでも読む");
    let compact = "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed_warning\"}}";
    assert_eq!(rate_limit_status(compact), Some("allowed_warning"), "compact も読む（実 stream の形）");
}

/// status は **`rate_limit_info` の直下**だけを読む（`s2-07l.77`・lens 2026-09-11 H3）。
///
/// `rate_limit_info` は `unifiedWindows` のような入れ子を持つ。最初に見つけた `"status"` を採る形は
/// **key の並び次第で別の object の値を読む**——記録の口は集合を育てる唯一の入力なので、ここが
/// 汚れると「実測で採れた値だけを入れる」が入口で崩れる。
#[test]
fn headless_runner_reads_the_status_only_from_the_immediate_object() {
    use vessel::headless::runner::rate_limit_status;
    // 入れ子が先に来て、その中に status が在る形（直下には無い）。
    let nested_first = concat!(
        "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{",
        "\"unifiedWindows\":{\"five_hour\":{\"status\":\"blocked\"}},\"utilization\":0.1}}"
    );
    assert_eq!(rate_limit_status(nested_first), None, "入れ子の status は読まない");
    // 直下に在る形（現物と同じ並び）。
    let direct = concat!(
        "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed_warning\",",
        "\"unifiedWindows\":{\"five_hour\":{\"status\":\"blocked\"}}}}"
    );
    assert_eq!(rate_limit_status(direct), Some("allowed_warning"), "直下の status を読む");
}

/// 判定は **`rate_limit_event` 種別に限る**（`s2-07l.77`・ADR-0012 §2.1 の MUST）。
///
/// 種別を見ない実装は、`rate_limit_info` の形さえ持てば**別の record を上限として読む**。
/// 変異（種別の照合を常に真にする）が生存したので歯を当てた（lens 2026-09-11 M1 / 変異 2 周目）。
#[test]
fn headless_runner_reads_the_status_only_from_the_dedicated_record_kind() {
    use vessel::headless::runner::rate_limit_status;
    // 上限 record と同じ形の field を持つが、**種別が違う** record。
    let impostor = "{\"type\":\"result\",\"rate_limit_info\":{\"status\":\"blocked\"}}";
    assert_eq!(rate_limit_status(impostor), None, "種別が違えば読まない");
    // 種別 field 自体が無い record も読まない。
    let typeless = "{\"rate_limit_info\":{\"status\":\"blocked\"}}";
    assert_eq!(rate_limit_status(typeless), None, "種別が無ければ読まない");
    // 対照: 種別が合っていれば読む。
    let real = "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"blocked\"}}";
    assert_eq!(rate_limit_status(real), Some("blocked"), "種別が合えば読む");
}

/// runner が組んだ prompt を**便の作業面へ残す**（`s2-07l.79`・設計 §6）。
///
/// prompt は stdin で渡すので stream には 1 行も出ない（`.67` の限界）。「何を渡したか」を
/// 後から読める唯一の口がこの file で、置き場は **vessel の写しの隣**（= run dir・
/// `<state_dir>/pipe/<run>/`）である。新しい flag も env も足さない（C2.2）。
///
/// 測るのは 3 つ: (i) file の中身が claude の stdin に届いた prompt と**同一**（要約や
/// 別の文面ではない）(ii) claude が起きる**前**に落ちている（起きた後に書く形だと、席が
/// 止まらない周の prompt が読めない）(iii) tracked な面（worktree）へは 1 byte も置かない。
#[test]
fn headless_runner_writes_the_prompt_beside_the_vessel_copy() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let saved = dir.join("prompt.txt");
    // fake は**起動された時点**の prompt file を写す（(ii) を rc でなく痕跡で測る）。
    let script = slurp(&claude).replace(
        "cat > \"",
        &format!("cp \"{}\" \"{}\" 2>/dev/null\ncat > \"", saved.display(), dir.join("prompt-at-call").display()),
    );
    fs::write(&claude, script).expect("fake を書き換えられる");
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        "goal = \"prompt を残す\"\n".as_bytes(),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let sent = slurp(&dir.join("stdin"));
    assert!(sent.contains("goal = \"prompt を残す\""), "fake は prompt を受けている: {sent}");
    assert_eq!(slurp(&saved), sent, "残した prompt は claude へ渡したものと同一");
    assert_eq!(slurp(&dir.join("prompt-at-call")), sent, "claude が起きる前に落ちている");
    assert!(!worktree.join("prompt.txt").exists(), "worktree（tracked 面）には置かない");
    assert!(!stderr_of(&out).contains("prompt を残せない"), "残せた周は欠落の行を出さない: {}", stderr_of(&out));
    clean(&[&dir, &worktree]);
}

/// 残せない周でも便は止めない（極性: 証跡は判定の入力ではない・`s2-07l.79`）。
///
/// 置き場を **dir で塞ぐ**（`prompt.txt` が dir だと write は EISDIR）。claude は起き、
/// rc は claude のものがそのまま写る（0 と 3 の両方で測る＝「たまたま 0」と区別する）。
/// 黙って落とすのではなく stderr へ 1 行残す——証跡の欠落を人が後から読めるように。
#[test]
fn headless_runner_keeps_going_when_the_prompt_cannot_be_saved() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    fs::create_dir_all(dir.join("prompt.txt")).expect("置き場を dir で塞げる");
    for want in [0_u8, 3_u8] {
        let claude = fake_claude(&dir, "", false, want);
        fs::remove_file(dir.join("called")).ok();
        let out = run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
            "goal = \"残せなくても進む\"\n".as_bytes(),
        );
        assert_eq!(out.status.code(), Some(i32::from(want)), "rc は claude のまま: {}", stderr_of(&out));
        assert!(dir.join("called").exists(), "claude は起きる");
        assert!(stdout_of(&out).contains(&format!("runner: rc={want} records=")), "記録行は変わらない");
        assert!(stderr_of(&out).contains("prompt を残せない"), "欠落は stderr に 1 行: {}", stderr_of(&out));
    }
    clean(&[&dir, &worktree]);
}

/// 置き場が解けない写し（裸の `vessel.toml`）では prompt を **cwd へ落とさない**（lens 2026-09-11 M2）。
///
/// `Path::parent` は裸の名に `Some("")` を返すので、素朴に join すると prompt が runner の cwd
/// ＝ pipeline では便の worktree（tracked 面）へ落ちる。「残さない」側へ倒し、便は止めない
/// （rc は claude のまま・stderr に欠落の 1 行）。cwd を tmp に固定して測る＝退行しても repo を汚さない。
#[test]
fn headless_runner_does_not_drop_the_prompt_into_the_cwd_for_a_relative_vessel() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let _ = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    plugin_leaf(&dir);
    // 起動は [`bin_cmd_with_toolbox`]（口 (ii)）で組む——道具箱は worktree の下（plugin root ではない）。
    // flip-check: retroactive s2-07l.504
    let mut child = bin_cmd_with_toolbox(&worktree)
        .args(["runner", "--worktree"])
        .arg(&worktree)
        .arg("--write-set")
        .arg(&write_set)
        .args(["--vessel", "vessel.toml", "--plugin-dir"])
        .arg(&dir)
        .args(["--permission-mode", "acceptEdits", "--claude"])
        .arg(&claude)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary を起動できる");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all("goal = \"相対の写し\"\n".as_bytes());
    }
    let out = child.wait_with_output().expect("binary の出力を読める");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "便は止めない: {}", stderr_of(&out));
    assert!(dir.join("called").exists(), "claude は起きる");
    assert!(!dir.join("prompt.txt").exists(), "cwd（相対 vessel の隣）へは落とさない");
    assert!(stderr_of(&out).contains("prompt を残せない"), "欠落は stderr に 1 行: {}", stderr_of(&out));
    clean(&[&dir, &worktree]);
}

#[test]
fn runner_question_record_on_last_line_yields_rc76_and_echoes_it() {
    let dir = tmp();
    let worktree = tmp();
    // result の text = 本文 1 行 + 最終行の record（JSON 文字列の中なので `"` と改行は escape）。
    let text = r#"契約を読んだ。\n{\"question\":\"verify 行が矛盾する\",\"about\":\"verify\"}"#;
    let out = run_question_runner(&dir, &worktree, &stream_with_result(text), 0, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(vessel::pipe::RC_QUESTION)), "包みは rc 76 で終える: {}", stderr_of(&out));
    let text = stdout_of(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.last().copied(), Some(QUESTION_RECORD), "最終行は同じ record そのもの: {lines:?}");
    assert!(
        lines.iter().rev().nth(1).is_some_and(|line| line.starts_with("runner: rc=0 records=")),
        "観測行は record の前に残る: {lines:?}"
    );
    clean(&[&dir, &worktree]);
}

#[test]
fn runner_question_mirrors_claude_rc_when_record_is_absent_or_unreadable() {
    let dir = tmp();
    let worktree = tmp();
    // key 無し / 壊れた JSON / 入れ子で引用された record / question が空、の 4 形（+ 記録の無い普通の終わり）。
    // `malformed` = JSON らしい最終行が読めない形（FailOpen を**隠さない**＝stderr に理由 1 行）。
    // `Plain`（key 無し・record 無し）は普通の終わり方なので stderr を出さない。
    for (label, text, malformed) in [
        ("key 無し", r#"done\n{\"about\":\"verify\"}"#, false),
        ("壊れた JSON", r#"done\n{\"question\":\"verify"#, true),
        ("入れ子で引用", r#"done\n{\"outer\":{\"question\":\"verify\"}}"#, true),
        ("question が空", r#"done\n{\"question\":\"  \",\"about\":\"verify\"}"#, true),
        ("record 無し", r#"done. see {\"question\":\"x\"} above"#, false),
    ] {
        let out = run_question_runner(&dir, &worktree, &stream_with_result(text), 0, b"goal = \"x\"\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{label}: claude の rc（0）を写す: {}", stderr_of(&out));
        assert!(has_no_record_line(&out), "{label}: stdout に record を写さない: {}", stdout_of(&out));
        assert!(stdout_of(&out).contains("runner: rc=0 records="), "{label}: 観測行は残る");
        assert_eq!(
            stderr_of(&out).contains("質問 record の形でない"),
            malformed,
            "{label}: 読めない形だけ stderr に理由 1 行（FailOpen を隠さない）: {}",
            stderr_of(&out)
        );
    }
    clean(&[&dir, &worktree]);
}

#[test]
fn runner_question_is_judged_only_after_normal_exit() {
    let dir = tmp();
    let worktree = tmp();
    let text = r#"failed\n{\"question\":\"verify 行が矛盾する\",\"about\":\"verify\"}"#;
    for want in [1_u8, 3] {
        let out = run_question_runner(&dir, &worktree, &stream_with_result(text), want, b"goal = \"x\"\n");
        assert_eq!(out.status.code(), Some(i32::from(want)), "非 0 の周は claude の rc を写す（76 にしない）");
        assert!(has_no_record_line(&out), "非 0 の周は最終行を読まない: {}", stdout_of(&out));
    }
    clean(&[&dir, &worktree]);
}

#[test]
fn runner_question_prompt_carries_record_rule_and_answer_section() {
    let dir = tmp();
    let worktree = tmp();
    // pipeline が stdin へ流す形: 契約の写し + 末尾の「## 回答」節（redirect ではなく piped stdin）。
    let contract = "goal = \"x\"\n\n## 回答\n- 質問: verify 行が矛盾する\n- 回答: verify は 1 行目だけを撃つ\n";
    let out = run_question_runner(&dir, &worktree, "", 0, contract.as_bytes());
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    assert!(prompt.contains("{\"question\":\"<"), "質問は最終行の record で、と命じる: {prompt}");
    assert!(prompt.contains("commit を作らない"), "質問の周は commit を作らない: {prompt}");
    assert!(prompt.contains("それ以外の形で人へ問わない"), "record 以外の対話を禁じたまま: {prompt}");
    assert!(!prompt.contains("対話しない。人へ質問を返さず"), "旧い文言（質問を返さず）は消える: {prompt}");
    assert!(prompt.contains("## 回答」節"), "回答節の読み方を運ぶ: {prompt}");
    assert!(
        prompt.contains("- 質問: verify 行が矛盾する") && prompt.contains("- 回答: verify は 1 行目だけを撃つ"),
        "stdin の回答節が prompt に**そのまま**載る（stdin だけを読む）: {prompt}"
    );
    clean(&[&dir, &worktree]);
}

/// prompt は「追随」節の**読み方**（雛形）を運び、pipeline が stdin へ足した run ごとの節を
/// **そのまま**載せる（設計 pipeline-conflict.md §3・`s2-07l.146`）。節の順序は stdin の順序
/// （契約 → 回答 → 追随）がそのまま prompt の順序になる＝包みは並べ替えない。
#[test]
fn runner_prompt_carries_follow_section_rule_and_keeps_section_order() {
    let dir = tmp();
    let worktree = tmp();
    // pipeline が stdin へ流す形: 契約の写し + 「## 回答」節 + 「## 追随」節。
    let contract = "goal = \"x\"\n\n## 回答\n- 質問: verify 行が矛盾する\n- 回答: verify は 1 行目だけを撃つ\n\n## 追随\n- main が deadbeef へ進んだ\n";
    let out = run_question_runner(&dir, &worktree, "", 0, contract.as_bytes());
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    assert!(prompt.contains("## 追随」節"), "追随節の読み方を運ぶ: {prompt}");
    // 追随の指示は `--onto` の 2 sha の形（設計 pipeline.md §38・`s2-07l.449`）: 便の base から先の commit だけを
    // main の上へ運ぶ（素の `git rebase <sha>` の形は消えた commit を運ぶので雛形から落とす）。
    assert!(prompt.contains("`git rebase --onto <main> <base>`"), "--onto の 2 sha の形で追随を命じる: {prompt}");
    assert!(!prompt.contains("`git rebase <sha>`"), "素の rebase の形は命じない: {prompt}");
    assert!(prompt.contains("git rebase --continue"), "解き終え方を命じる: {prompt}");
    assert!(prompt.contains("git rebase --abort"), "解けない周の戻し方を命じる: {prompt}");
    assert!(prompt.contains("- main が deadbeef へ進んだ"), "stdin の節がそのまま載る: {prompt}");
    let answer_at = prompt.rfind("- 回答: verify は 1 行目だけを撃つ");
    let follow_at = prompt.rfind("- main が deadbeef へ進んだ");
    assert!(
        matches!((answer_at, follow_at), (Some(a), Some(f)) if a < f),
        "stdin の順序（回答 → 追随）が保たれる: {prompt}"
    );
    clean(&[&dir, &worktree]);
}

/// 判定の純関数の**受理側**（[`vessel::headless::runner::question_ending`]）。
#[test]
fn runner_question_ending_accepts_records_and_plain_text() {
    use vessel::headless::runner::{question_ending, Ending};
    assert_eq!(question_ending(&format!("done\n{QUESTION_RECORD}")), Ending::Question(QUESTION_RECORD.to_owned()));
    assert_eq!(question_ending(&format!("  {QUESTION_RECORD}  \n")), Ending::Question(QUESTION_RECORD.to_owned()), "前後の空白は剥がす");
    assert_eq!(question_ending("done\n{\"question\":\"only\"}"), Ending::Question("{\"question\":\"only\"}".to_owned()), "about は任意");
    // 最終の `{` 行だけを見る（前の行の record は読まない・後ろの散文は無視）。
    assert_eq!(question_ending(&format!("{QUESTION_RECORD}\nfollow-up text")), Ending::Question(QUESTION_RECORD.to_owned()), "最後の `{{` 行");
    assert_eq!(question_ending("just text"), Ending::Plain);
    assert_eq!(question_ending(""), Ending::Plain);
    assert_eq!(question_ending("{\"about\":\"verify\"}"), Ending::Plain, "question key 無しは record ではない");
}

/// 判定の純関数の**拒否側**（理由の字面まで見る・FailOpen を隠さない）。
#[test]
fn runner_question_ending_rejects_malformed_records_with_reasons() {
    use vessel::headless::runner::{question_ending, Ending};
    let reason_of = |text: &str| match question_ending(text) {
        Ending::Malformed(reason) => reason,
        other => format!("not malformed: {other:?}"),
    };
    assert!(reason_of("{\"question\":\"x").contains("読めない"), "壊れた JSON");
    assert!(reason_of("{\"outer\":{\"question\":\"x\"}}").contains("読めない"), "入れ子は flat parser が断る");
    assert!(reason_of("{\"question\":\"   \"}").contains("空"), "空の question");
    assert!(reason_of("{\"question\":7}").contains("文字列"), "非文字列");
    assert!(reason_of("{\"question\":\"a\\nb\"}").contains("1 行"), "複数行");
}

/// `result` record の text を escape を解いて読む（[`vessel::headless::runner::result_text`]）。
#[test]
fn runner_question_result_text_decodes_escapes_only_from_result_records() {
    use vessel::headless::runner::result_text;
    let line = r#"{"type":"result","is_error":false,"result":"a\n\"q\" \\ é 😀","usage":{"x":1}}"#;
    assert_eq!(result_text(line).as_deref(), Some("a\n\"q\" \\ é 😀"));
    assert_eq!(result_text(r#"{"type":"assistant","result":"x"}"#), None, "種別が違う行は読まない");
    assert_eq!(result_text(r#"{"type":"result","is_error":true}"#), None, "result field が無い");
    assert_eq!(result_text("not json"), None);
    assert_eq!(result_text(r#"{"result":"first","type":"result"}"#).as_deref(), Some("first"), "key の並びに依らない");
}

/// `result_text` は入れ子の `"type"` を種別と読まず、top-level の `"type":"result"` を見る。
#[test]
fn runner_question_toplevel_result_text_reads_real_record_with_nested_type_first() {
    use vessel::headless::runner::result_text;
    let line = real_result_record(r#"契約の done が矛盾する。\n{\"question\":\"どちらの期待値が正しいか\",\"about\":\"done\"}"#);
    assert_eq!(
        result_text(&line).as_deref(),
        Some("契約の done が矛盾する。\n{\"question\":\"どちらの期待値が正しいか\",\"about\":\"done\"}"),
        "入れ子の type:message が先に在っても result record と読む"
    );
    // 偽陽性を塞ぐ: 入れ子だけに type:result を持ち、top-level の種別が別の行は読まない。
    // 入れ子を**先**に置く（top-level を先に置くと最初の対を読む実装でも None になり歯が空虚・lens H1）。
    let nested_only = r#"{"quoted":{"type":"result","result":"inner"},"type":"assistant","result":"outer"}"#;
    assert_eq!(result_text(nested_only), None, "入れ子の type:result は種別ではない");
    // 文字列中の escape された `"` を閉じ引用符と読まない（奇数個の `\"` で走査がずれると top-level の
    // `"type"` を見失う・lens H2）。
    let odd_escape = r#"{"result":"he said \"hi","type":"result"}"#;
    assert_eq!(result_text(odd_escape).as_deref(), Some("he said \"hi"), "escape された引用符は文字列を閉じない");
    // 文字列の中の brace / bracket は深さに数えない（後ろの top-level key を見失わない）。
    let braces_in_string = r#"{"note":"has { and [ inside","type":"result","result":"ok"}"#;
    assert_eq!(result_text(braces_in_string).as_deref(), Some("ok"), "文字列中の brace は深さに数えない");
}

/// 包み経由: 実 record の形で最終行に record → rc 76・stdout 最終行に同じ record（base では rc 0 を写す）。
#[test]
fn runner_question_toplevel_real_record_yields_rc76_through_the_wrapper() {
    let dir = tmp();
    let worktree = tmp();
    let text = r#"契約を読んだ。\n{\"question\":\"verify 行が矛盾する\",\"about\":\"verify\"}"#;
    let body = format!("{{\"type\":\"system\",\"subtype\":\"init\"}}\n{}\n", real_result_record(text));
    let out = run_question_runner(&dir, &worktree, &body, 0, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(vessel::pipe::RC_QUESTION)), "実 record の形でも rc 76: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert_eq!(stdout.lines().last(), Some(QUESTION_RECORD), "最終行は同じ record: {stdout}");
    clean(&[&dir, &worktree]);
}

/// `rate_limit_info` の直下で `status` より**前**に文字列中の brace（`"note":"win {5h}"`）が在る行
/// （`s2-07l.126`・.123 lens MED-4）: 切り出しが文字列の中の `{` で早く終わると status が None になり、
/// 止めるべき便を止めない（fail-open の向き）。文字列と escape を飛ばして**文字列の外**の brace で切る。
#[test]
fn runner_rate_limit_brace_inside_string_before_status_is_still_read() {
    use vessel::headless::runner::rate_limit_status;
    let braces = r#"{"type":"rate_limit_event","rate_limit_info":{"note":"win {5h}","status":"allowed_warning"}}"#;
    assert_eq!(rate_limit_status(braces), Some("allowed_warning"), "文字列中の brace は境界ではない");
    // escape された引用符を含む文字列でも同じ（`\"` で文字列を閉じたと読むと `{` が外に見える）。
    let escaped = r#"{"type":"rate_limit_event","rate_limit_info":{"note":"say \"{\" then }","status":"blocked"}}"#;
    assert_eq!(rate_limit_status(escaped), Some("blocked"), "escape された引用符は文字列を閉じない");
    // 閉じ brace だけが文字列に在る形（早期終端の向きが `}` でも同じ）。
    let closing = r#"{"type":"rate_limit_event","rate_limit_info":{"note":"}","status":"blocked"}}"#;
    assert_eq!(rate_limit_status(closing), Some("blocked"), "文字列中の閉じ brace で切らない");
}

/// 同じ行で status が止める側の集合に在れば `decide` が止める（base では None → Ignore で止まらない）。
#[test]
fn runner_rate_limit_brace_inside_string_still_stops_when_status_is_in_the_set() {
    use vessel::headless::runner::{decide, Decision};
    let braces = r#"{"type":"rate_limit_event","rate_limit_info":{"note":"win {5h}","status":"blocked"}}"#;
    assert_eq!(decide(braces, &["blocked"]), Decision::Stop("blocked".to_owned()), "止める側なら Stop");
    assert_eq!(decide(braces, &[]), Decision::Observed("blocked".to_owned()), "集合に無ければ記録だけ");
}

/// status より前に**入れ子の object**（文字列でない brace）が在っても直下の status を読む（planner 裁定 = 案 P・
/// lens-126 HIGH-1）。入れ子で打ち切る形は key の並び次第で直下の status を取り逃す fail-open の穴だった（`.123` の
/// `usage.iterations[]` と同型・base では None）。入れ子の**中**の status は深さ guard が読まない（既存の歯と同じ向き）。
#[test]
fn runner_rate_limit_brace_nested_object_before_status_still_cuts() {
    use vessel::headless::runner::rate_limit_status;
    let nested_first = r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"status":"other"},"status":"blocked"}}"#;
    assert_eq!(rate_limit_status(nested_first), Some("blocked"), "入れ子が先でも直下の status を読む（base では None）");
    // 入れ子の中にしか status が無ければ読まない（直下の判定は深さ guard・兄弟の入れ子を採らない）。
    let nested_only = r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"status":"other"},"note":"x"}}"#;
    assert_eq!(rate_limit_status(nested_only), None, "入れ子の中の status は直下ではない");
    // 入れ子が status の後ろでも同じ。
    let nested_after = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"blocked","unifiedWindows":{"status":"other"}}}"#;
    assert_eq!(rate_limit_status(nested_after), Some("blocked"), "直下の status は読める");
}

/// 上限 record の読みも同じ走査に乗る: 入れ子の `"type"` が先に在っても `rate_limit_event` を種別と読み、
/// 入れ子だけに `rate_limit_event` を持つ行（別 record の引用）は読まない。
#[test]
fn runner_question_toplevel_rate_limit_status_ignores_nested_type_keys() {
    use vessel::headless::runner::rate_limit_status;
    let nested_first = r#"{"meta":{"type":"noise"},"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","unifiedWindows":{"status":"other"}}}"#;
    assert_eq!(rate_limit_status(nested_first), Some("allowed_warning"), "入れ子の type が先でも top-level を読む");
    let quoted = r#"{"type":"assistant","quoted":{"type":"rate_limit_event","rate_limit_info":{"status":"blocked"}}}"#;
    assert_eq!(rate_limit_status(quoted), None, "引用された上限 record は読まない");
}

/// (d) `is_error` の result で rc 1 → 観測行が要約行の**前**に在り、最終行は要約行のまま。
#[test]
fn headless_runner_result_line_records_error_result_before_the_summary_on_nonzero_rc() {
    let dir = tmp();
    let worktree = tmp();
    let body = "{\"type\":\"system\",\"subtype\":\"init\"}\n\
                {\"type\":\"result\",\"subtype\":\"error_during_execution\",\"is_error\":true,\"result\":\"boom\\nline2\"}\n";
    let out = run_question_runner(&dir, &worktree, body, 1, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(1), "claude の rc を写す: {}", stderr_of(&out));
    let text = stdout_of(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.last().copied(),
        Some("runner: rc=1 records=2"),
        "最終行は要約行のまま（pipeline が読む面を変えない）: {lines:?}"
    );
    assert_eq!(
        lines.iter().rev().nth(1).copied(),
        Some("runner: result subtype=error_during_execution is_error=true text=boom line2"),
        "観測行は要約行の直前・改行は空白に: {lines:?}"
    );
    clean(&[&dir, &worktree]);
}

/// (e) rc 0 ∧ `subtype=success` → `is_error=false`・最終行は要約行のまま。
#[test]
fn headless_runner_result_line_records_success_and_keeps_the_summary_last() {
    let dir = tmp();
    let worktree = tmp();
    let body = "{\"type\":\"system\",\"subtype\":\"init\"}\n\
                {\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"done\",\"usage\":{\"is_error\":true}}\n";
    let out = run_question_runner(&dir, &worktree, body, 0, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let text = stdout_of(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.last().copied(), Some("runner: rc=0 records=2"), "最終行は要約行のまま: {lines:?}");
    assert_eq!(
        result_line_of(&out).as_deref(),
        Some("runner: result subtype=success is_error=false text=done"),
        "入れ子の is_error は読まない（top-level だけ）: {lines:?}"
    );
    clean(&[&dir, &worktree]);
}

/// (f) result record が無い stream（assistant だけ）→ 観測行が**無い**（「無い」を `-` に化けさせない）。
/// (d) と対で置く（負例だけで RED を主張しない）。
#[test]
fn headless_runner_result_line_is_absent_without_a_result_record() {
    let dir = tmp();
    let worktree = tmp();
    let body = "{\"type\":\"system\",\"subtype\":\"init\"}\n\
                {\"type\":\"assistant\",\"message\":{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false}}\n";
    for want in [0_u8, 1] {
        let out = run_question_runner(&dir, &worktree, body, want, b"goal = \"x\"\n");
        assert_eq!(out.status.code(), Some(i32::from(want)), "{}", stderr_of(&out));
        assert_eq!(result_line_of(&out), None, "record を見ていない周は観測行を出さない: {}", stdout_of(&out));
        assert!(
            stdout_of(&out).lines().last().is_some_and(|line| line == format!("runner: rc={want} records=2")),
            "要約行は変わらない: {}",
            stdout_of(&out)
        );
    }
    clean(&[&dir, &worktree]);
}

/// (g) 質問 record の周（rc 76）→ 観測行 + 要約行 + 質問 record の 3 行で、最終行は質問 record のまま。
#[test]
fn headless_runner_result_line_precedes_the_summary_and_the_question_record() {
    let dir = tmp();
    let worktree = tmp();
    let text = r#"契約を読んだ。\n{\"question\":\"verify 行が矛盾する\",\"about\":\"verify\"}"#;
    let out = run_question_runner(&dir, &worktree, &stream_with_result(text), 0, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(vessel::pipe::RC_QUESTION)), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines,
        [
            "runner: result subtype=- is_error=false text=契約を読んだ。 {\"question\":\"verify 行が矛盾する\",\"about\":\"verify\"}",
            "runner: rc=0 records=2",
            QUESTION_RECORD,
        ],
        "3 行・最終行は質問 record のまま: {stdout}"
    );
    clean(&[&dir, &worktree]);
}

/// (h) text が上限を超える → 先頭 [`vessel::headless::runner::RESULT_TEXT_CHARS`] 字で切れる（`…` は付けない・字数で数える）。
#[test]
fn headless_runner_result_line_cuts_the_text_at_the_char_limit() {
    use vessel::headless::runner::RESULT_TEXT_CHARS;
    let dir = tmp();
    let worktree = tmp();
    // 多 byte 字で埋める＝byte で切る実装は字数が合わない（字数で数えることを測る）。
    let long = "字".repeat(RESULT_TEXT_CHARS + 50);
    let body = format!("{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"{long}\"}}\n");
    let out = run_question_runner(&dir, &worktree, &body, 0, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let line = result_line_of(&out).unwrap_or_default();
    let text = line.split_once(" text=").map(|(_, text)| text).unwrap_or_default();
    assert_eq!(text.chars().count(), RESULT_TEXT_CHARS, "先頭 {RESULT_TEXT_CHARS} 字で切る: {line}");
    assert_eq!(text, "字".repeat(RESULT_TEXT_CHARS), "切った後に `…` を付けない: {line}");
    // 上限ちょうどは切らない。
    let exact = "a".repeat(RESULT_TEXT_CHARS);
    let body = format!("{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"{exact}\"}}\n");
    let out = run_question_runner(&dir, &worktree, &body, 0, b"goal = \"x\"\n");
    let line = result_line_of(&out).unwrap_or_default();
    assert!(line.ends_with(&format!(" text={exact}")), "上限ちょうどは丸ごと: {line}");
    clean(&[&dir, &worktree]);
}

/// 純関数の面: 種別は閉じた enum（未知は `unknown` 1 つに潰し字面を残さない）・`is_error` は top-level の bool だけ・
/// 無いものは `-`・tab と改行は空白。
#[test]
fn headless_runner_result_line_pure_readers_and_format() {
    use vessel::headless::runner::{result_is_error, result_line, result_subtype, ResultKind};
    assert_eq!(result_subtype(r#"{"type":"result","subtype":"error_max_turns"}"#), Some(ResultKind::ErrorMaxTurns));
    assert_eq!(result_subtype(r#"{"type":"result","subtype":"something_new"}"#), Some(ResultKind::Unknown), "未知は 1 variant");
    assert_eq!(result_subtype(r#"{"type":"result","is_error":true}"#), None, "subtype 無し");
    assert_eq!(result_subtype(r#"{"type":"assistant","subtype":"success"}"#), None, "result record でない");
    assert_eq!(result_is_error(r#"{"type":"result","is_error":true}"#), Some(true));
    assert_eq!(result_is_error(r#"{"type":"result","is_error": false ,"x":1}"#), Some(false), "空白に寛容");
    assert_eq!(result_is_error(r#"{"type":"result","usage":{"is_error":true}}"#), None, "入れ子は読まない");
    assert_eq!(result_is_error(r#"{"type":"result","is_error":"true"}"#), None, "文字列は bool ではない");
    assert_eq!(
        result_line(Some(ResultKind::Unknown), None, Some("a\tb\r\nc")),
        "runner: result subtype=unknown is_error=- text=a b  c"
    );
    assert_eq!(result_line(None, Some(true), None), "runner: result subtype=- is_error=true text=-");
}

/// **runner が起こす claude の箱は 1 × `gate.job_memory_mb`**・雛形は検出線を撃たない 1 行を「実行してよい
/// command」節に 1 本だけ持つ。base は claude を `MemTotal − host.reserve_memory_mb`（host の箱）で包み、
/// 雛形に行が無い＝どちらの assert でも RED。claude を呼んだことを `called` の印で先に測る。
#[test]
fn headless_runner_box_claude_is_one_job_and_the_prompt_names_the_detector() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let claude = fake_claude(&dir, &format!("{RESULT_RECORD}\n"), false, 0);
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        contract_text(CONTRACT_GOAL).as_bytes(),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "母集団: runner は claude を呼んだ");

    let names = crate::toolbox_record_names(&worktree);
    let record = crate::toolbox_record(&worktree, "-claude-");
    assert!(record.lines().any(|line| line == "--scope"), "claude は包めた（母集団 {names:?}）: {record}");
    assert_eq!(
        record.lines().find_map(|line| line.strip_prefix("MemoryMax=")).unwrap_or_default(),
        format!("{}M", crate::pipe::embedded_int("gate.job_memory_mb")),
        "claude の箱は 1 × gate.job_memory_mb: {record}"
    );

    let prompt = slurp(&dir.join("stdin"));
    assert_eq!(prompt.lines().filter(|line| *line == DETECTOR_LINE).count(), 1, "検出線の行は 1 本: {prompt}");
    let section = prompt.split("## 実行してよい command").nth(1).and_then(|rest| rest.split("\n## ").next()).unwrap_or_default();
    assert!(section.lines().any(|line| line == DETECTOR_LINE), "行は「実行してよい command」節に在る: {section}");
    clean(&[&dir, &worktree]);
}
