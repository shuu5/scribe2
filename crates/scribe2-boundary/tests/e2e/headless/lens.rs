// flip-check: moved s2-07l.675
//! lens の族の歯（接頭辞 `headless_lens_` / `lens_rulings_`・設計 docs/design/carry-prep.md §8 行 f）。
//!
//! 共有の helper と const と外形 snapshot の歯（`headless_lens_prompt_external_form` /
//! `headless_lens_contract_prompt_external_form` / `headless_lens_promise_prompt_external_form`・snapshot 名が
//! module path を含むので動かさない）は親 module（`tests/e2e/headless.rs`）に在り、`use super::*` で使う。
//! 歯の本文は親から**挙動不変で移した**もの（`s2-07l.675`）。

use super::*;

#[test]
fn headless_lens_inconclusive_over_cap_without_calling_claude() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let contract = contract_in(&dir);
    let diff = vec![b'x'; 4096];
    let out = run_lens(&contract, 16, "plan", &claude, &diff);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"INCONCLUSIVE","evidence":"diff exceeds cap"}"#,
        "cap 超過は INCONCLUSIVE"
    );
    // **効果で測る**: cap の意味は「呼ばないこと」なので、呼んでいないことを痕跡で見る。
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");

    // **cap の内側は 128KiB を超えても判定を返す**。prompt を argv で渡すと Linux の
    // 1 引数上限（131072 byte）に当たり、user が裁定した cap 150000 が実質 130KB へ
    // 黙って切り下がる（実測 2026-09-10）。境界の内側で判定が返ることを測る。
    let verdict = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"大きくても読めた\"}\n", false, 0);
    let big = vec![b'x'; 140_000];
    // mode を歯 4 と変えてある。lens 側の permission mode を定数へ固定する変異は、
    // 1 種類しか撃たない歯では捕まらない（実測で生存した）。
    let out = run_lens(&contract, 150_000, "acceptEdits", &verdict, &big);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "cap の内側なので claude を呼ぶ");
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"PASS","evidence":"大きくても読めた"}"#,
        "128KiB 超でも判定を返す"
    );
    let args = slurp(&dir.join("args"));
    assert!(
        args.lines().collect::<Vec<_>>().windows(2).any(|w| {
            w.first() == Some(&"--permission-mode") && w.get(1) == Some(&"acceptEdits")
        }),
        "lens も permission mode を毎回明示する: {args}"
    );

    // **境界ちょうど（diff の byte 数 == cap）は cap の内側**である。`>` を `>=` に
    // すり替える変異は、境界を撃たない歯では捕まらない（実測で生存した）。
    let edge = tmp();
    let at_cap = fake_claude(&edge, "{\"verdict\":\"FAIL\",\"evidence\":\"境界は内側\"}\n", false, 0);
    let edge_contract = contract_in(&edge);
    let out = run_lens(&edge_contract, 64, "plan", &at_cap, &vec![b'y'; 64]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(edge.join("called").exists(), "境界ちょうどでは claude を呼ぶ");
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"FAIL","evidence":"境界は内側"}"#,
        "境界ちょうどは判定を返す"
    );
    clean(&[&edge, &dir]);
}

/// cap は **rules 行 `gate.token_cap` からだけ**読む（`s2-07l.272`・憲法 C1・FR17）。`--cap` を渡さず
/// `--rules` の manifest の値だけで INCONCLUSIVE / 呼出が切り替わる。base は `--cap` が無いと usage の
/// rc 1 で断るので RED。
#[test]
fn headless_lens_reads_cap_from_rules_row() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"manifest の cap の内側\"}\n", false, 0);
    let contract = contract_in(&dir);
    let diff = vec![b'z'; 11];
    // cap 10 byte・diff 11 byte → 超過。claude を呼ばず INCONCLUSIVE。
    let small = rules_with_cap(&dir, 10);
    let out = run_bin_owned(&dir, &lens_args(&contract, &dir, &["--rules", &small.display().to_string()], &claude), &diff);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"INCONCLUSIVE","evidence":"diff exceeds cap"}"#,
        "manifest の cap を超えた周は INCONCLUSIVE"
    );
    assert!(!dir.join("called").exists(), "cap を超えたので claude を 1 度も起動しない");
    // **同じ diff・manifest の値だけ 100 byte へ** → 内側。claude が 1 回呼ばれる＝値は manifest から来ている。
    let wide = rules_with_cap(&dir, 100);
    let out = run_bin_owned(&dir, &lens_args(&contract, &dir, &["--rules", &wide.display().to_string()], &claude), &diff);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "manifest の cap の内側なので claude を呼ぶ");
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"PASS","evidence":"manifest の cap の内側"}"#,
        "判定は claude の最後の JSON 行"
    );
    let args = slurp(&dir.join("args"));
    assert!(!has_arg(&args, "--cap"), "cap は claude へ渡らない: {args}");
    // **`--rules` が無い周は埋め込みの manifest**（`pipe::cli` と同じ規約）。埋め込みの cap は 11 byte より
    // 大きいので claude を呼ぶ＝「`--rules` 無しは cap 0」へ倒す変異を落とす。
    fs::remove_file(dir.join("called")).expect("前の周の印を消せる");
    let out = run_bin_owned(&dir, &lens_args(&contract, &dir, &[], &claude), &diff);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "埋め込みの cap の内側なので claude を呼ぶ");
    clean(&[&dir]);
}

/// 撤去した `--cap` は**未知の引数として断る**（入口の閉包の断りで rc 2・設計 pipeline.md §14 約束 4・usage・claude 未起動）。
/// 黙って読み飛ばすと、手書きの数が残った launcher が効いているように見える（`.265` の drift の再発経路）。base は受理するので RED。
#[test]
fn headless_lens_refuses_cap_flag() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let contract = contract_in(&dir);
    let rules = rules_with_cap(&dir, 4096);
    let out = run_bin_owned(
        &dir,
        &lens_args(&contract, &dir, &["--rules", &rules.display().to_string(), "--cap", "1"], &claude),
        b"--- a\n+++ b\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "--cap は未知の引数: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    let err = stderr_of(&out);
    assert!(err.contains("--cap"), "断った引数を名指す: {err}");
    assert!(err.contains("usage: "), "usage を出す: {err}");
    assert!(!err.contains("--cap BYTES"), "usage に --cap は載らない: {err}");
    assert!(err.contains("[--rules PATH]"), "usage は --rules を載せる: {err}");
    clean(&[&dir]);
}

/// cap の行が解けない周は **claude を呼ばず rc 2** で理由を 1 行（`lens: gate.token_cap …`・pipe の `int_row`
/// と同じ 3 理由 + manifest 自体が読めない周）。上限なしで走らせない（C6）。
#[test]
fn headless_lens_refuses_unreadable_cap_row() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let contract = contract_in(&dir);
    let absent = rules_with_row(
        &dir,
        "absent.toml",
        "id = \"gate.lens_count\"\nkind = \"GateLensCount\"\nvalue = 1\nenabled = true\n",
    );
    let disabled = rules_with_row(
        &dir,
        "disabled.toml",
        "id = \"gate.token_cap\"\nkind = \"GateTokenCap\"\nvalue = 4096\nenabled = false\n",
    );
    // id は同じで kind が散文の行（manifest は id と kind の対応を照合しない）＝値が整数でない形。
    let text = rules_with_row(
        &dir,
        "text.toml",
        "id = \"gate.token_cap\"\nkind = \"MaturityCondition\"\nvalue = \"abc\"\nenabled = true\n",
    );
    let missing = dir.join("no-such-rules.toml");
    for (rules, want) in [
        (&absent, "gate.token_cap が無い"),
        (&disabled, "gate.token_cap は不発効である"),
        (&text, "gate.token_cap が整数でない"),
        (&missing, "rules を読めない"),
    ] {
        let out = run_bin_owned(
            &dir,
            &lens_args(&contract, &dir, &["--rules", &rules.display().to_string()], &claude),
            b"--- a\n+++ b\n",
        );
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{want}: rc 2 / {}", stderr_of(&out));
        assert!(!dir.join("called").exists(), "{want}: claude を 1 度も起動しない");
        let err = stderr_of(&out);
        assert!(err.contains(&format!("lens: {want}")), "理由を 1 行で名乗る: {err}");
        assert_eq!(err.lines().count(), 1, "stderr は理由の 1 行だけ: {err}");
        assert!(stdout_of(&out).is_empty(), "判定の面には何も出さない: {}", stdout_of(&out));
    }
    clean(&[&dir]);
}

/// (b) lens も同じ行の model を claude に毎回渡す（runner と同じ構築点 `build`）: `--rules` の manifest の値ごとに
/// `--model` の対が変わり、`--rules` 無しは埋め込みの行。base は渡さないので RED。
#[test]
fn headless_lens_passes_model_from_rules_row() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"model の歯\"}\n", false, 0);
    let contract = contract_in(&dir);
    for (value, want) in [("opus", "opus"), ("haiku", "haiku"), ("Sonnet", "sonnet")] {
        let rules = rules_with_rows(&dir, &format!("rules-model-{value}.toml"), &[cap_row(4096), model_row(value), effort_row(RUNNER_EFFORT)]);
        let out = run_bin_owned(&dir, &lens_args(&contract, &dir, &["--rules", &rules.display().to_string()], &claude), b"--- a\n+++ b\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{value}: {}", stderr_of(&out));
        assert_eq!(stdout_of(&out).trim(), r#"{"verdict":"PASS","evidence":"model の歯"}"#, "{value}: 判定は claude の行");
        assert_eq!(model_arg(&dir), Some(want.to_owned()), "{value}: 行の値を CLI の別名で渡す: {}", slurp(&dir.join("args")));
        fs::remove_file(dir.join("args")).expect("前の周の写しを消せる");
    }
    let out = run_bin_owned(&dir, &lens_args(&contract, &dir, &[], &claude), b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert_eq!(model_arg(&dir), Some(RUNNER_MODEL.to_owned()), "埋め込みの行: {}", slurp(&dir.join("args")));
    clean(&[&dir]);
}

/// lens の `runner.model` の行も同じ極性（claude を呼ばず rc 2・理由 1 行）。cap の行が解けない周は cap の理由が先
/// （[`headless_lens_refuses_unreadable_cap_row`] の字面は不変）。
#[test]
fn headless_lens_refuses_when_model_row_is_missing() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let contract = contract_in(&dir);
    let absent = rules_with_rows(&dir, "absent.toml", &[cap_row(4096)]);
    let unknown = rules_with_rows(&dir, "unknown.toml", &[cap_row(4096), model_row("Opus 5")]);
    let both_missing = rules_with_row(&dir, "both.toml", "id = \"gate.lens_count\"\nkind = \"GateLensCount\"\nvalue = 1\nenabled = true\n");
    for (rules, want) in [
        (&absent, "runner.model が無い"),
        (&unknown, "runner.model の値 Opus 5 は未知の model"),
        (&both_missing, "gate.token_cap が無い"),
    ] {
        let out = run_bin_owned(&dir, &lens_args(&contract, &dir, &["--rules", &rules.display().to_string()], &claude), b"--- a\n+++ b\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{want}: rc 2 / {}", stderr_of(&out));
        assert!(!dir.join("called").exists(), "{want}: claude を 1 度も起動しない");
        let err = stderr_of(&out);
        assert!(err.contains(&format!("lens: {want}")), "{want}: 理由を 1 行で名乗る: {err}");
        assert_eq!(err.lines().count(), 1, "{want}: stderr は理由の 1 行だけ: {err}");
        assert!(stdout_of(&out).is_empty(), "{want}: 判定の面には何も出さない");
    }
    clean(&[&dir]);
}

/// (b) lens も同じ行の effort を claude に毎回渡す（runner と同じ構築点 `build`）: `--rules` の manifest の値ごとに
/// `--effort` の対が変わり、`--rules` 無しは埋め込みの行。base は渡さないので RED。
#[test]
fn headless_lens_passes_effort_from_rules_row() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"effort の歯\"}\n", false, 0);
    let contract = contract_in(&dir);
    for value in ["high", "medium", "xhigh"] {
        let rules = rules_with_rows(&dir, &format!("rules-effort-{value}.toml"), &[cap_row(4096), model_row(RUNNER_MODEL), effort_row(value)]);
        let out = run_bin_owned(&dir, &lens_args(&contract, &dir, &["--rules", &rules.display().to_string()], &claude), b"--- a\n+++ b\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{value}: {}", stderr_of(&out));
        assert_eq!(stdout_of(&out).trim(), r#"{"verdict":"PASS","evidence":"effort の歯"}"#, "{value}: 判定は claude の行");
        assert_eq!(effort_arg(&dir), Some(value.to_owned()), "{value}: 行の値を渡す: {}", slurp(&dir.join("args")));
        fs::remove_file(dir.join("args")).expect("前の周の写しを消せる");
    }
    let out = run_bin_owned(&dir, &lens_args(&contract, &dir, &[], &claude), b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert_eq!(effort_arg(&dir), Some(RUNNER_EFFORT.to_owned()), "埋め込みの行: {}", slurp(&dir.join("args")));
    clean(&[&dir]);
}

#[test]
fn headless_lens_extracts_last_json_line() {
    let dir = tmp();
    let body = concat!(
        "diff を読んでいます\n",
        "{\"verdict\":\"INCONCLUSIVE\",\"evidence\":\"まだ途中\"}\n",
        "考え直しました\n",
        "{\"verdict\":\"FAIL\",\"evidence\":\"最後の判定\"}\n",
        "おしまい\n"
    );
    let account = tmp();
    let claude = fake_claude(&dir, body, false, 0);
    let contract = contract_in(&dir);
    let rules = rules_with_cap(&dir, 4096);
    let out = run_bin(
        &dir,
        &[
            "lens", "--contract", &contract.display().to_string(),
            "--worktree", &dir.display().to_string(),
            "--rules", &rules.display().to_string(), "--permission-mode", "plan",
            "--account-dir", &account.display().to_string(),
            "--claude", &claude.display().to_string(),
        ],
        b"--- a\n+++ b\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "cap 内なので claude を呼ぶ");
    // lens 側の引数も runner と同じだけ測る（片側だけ測ると、もう片側は自由に壊れる）。
    let args = slurp(&dir.join("args"));
    let lines: Vec<&str> = args.lines().collect();
    assert!(lines.contains(&"-p"), "headless で回す: {args}");
    assert!(pair(&args, "--permission-mode", "plan"), "permission mode を毎回明示する: {args}");
    assert!(
        !lines.iter().any(|line| line.starts_with("--dangerously")),
        "権限を外す flag を渡さない: {args}"
    );
    assert_eq!(slurp(&dir.join("account")), account.display().to_string(), "口座は子の env へ");
    assert!(slurp(&dir.join("stdin")).contains("--- a"), "diff が prompt に載る");
    // **lens は json（1 object の封筒）で呼ぶ**（設計 gate-cost.md §26 形 (2)）。stream-json にすると全行が JSON になり、
    // 「最後の JSON 行」が claude 自身の result record になって判定が取れない。封筒でない出力（この fake）は従来どおり
    // 最後の JSON 行をそのまま読む。
    let args = slurp(&dir.join("args"));
    assert!(pair(&args, "--output-format", "json"), "lens は json で呼ぶ: {args}");
    assert!(!pair(&args, "--output-format", "stream-json"), "stream-json ではない: {args}");
    // 途中の JSON でも末尾の地の文でもなく、**最後の JSON 行**ちょうど 1 行。
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"FAIL","evidence":"最後の判定"}"#,
        "最後の JSON 行を写す"
    );
    assert_eq!(stdout_of(&out).lines().count(), 1, "stdout は 1 行だけ");
    clean(&[&dir, &account]);
}

/// 席の pane の中から `lens` を**単体起動**しても claude の env に `TMUX_PANE` が無く `PATH` は継承される
/// （runner と同じ `wrap_command` の 1 点で外す）。base は RED。
#[test]
fn headless_lens_drops_tmux_pane() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"pane の歯\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "cap 内なので lens は claude を呼ぶ");
    assert_pane_dropped(&dir, "lens");
    clean(&[&dir]);
}

#[test]
fn headless_lens_inconclusive_on_unparsable_output() {
    let dir = tmp();
    let claude = fake_claude(&dir, "判定できませんでした\nもう一度お願いします\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "呼んだ上で読めなかった周である");
    // 読めない出力を握り潰さず、**判定に届かなかった**と名乗る（偽の PASS を作らない）。
    assert!(
        stdout_of(&out).contains(r#""verdict":"INCONCLUSIVE""#),
        "parse 不能は INCONCLUSIVE: {}",
        stdout_of(&out)
    );
    clean(&[&dir]);
}

#[test]
fn headless_lens_prompt_includes_contract_fields() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"契約を読めた\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a/src/lib.rs\n+++ b/src/lib.rs\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "契約が在るので claude を呼ぶ");
    let prompt = slurp(&dir.join("stdin"));
    // **契約の 4 面が prompt に載る**（設計 §6「契約の verify と diff を読み」）。diff だけを
    // 渡して契約適合は問えない——実 lens は「契約が未提供で適合を判定できない」と
    // INCONCLUSIVE を返し、便がそこで止まった（実測 2026-09-10・s2-07l.24 の実 5 便）。
    //
    // **値の存在ではなく「どの見出しの下に在るか」まで測る**。値だけを数えると、goal と
    // done のラベルを入れ替える変異が生き残る（review 2026-09-10 F3）。
    assert!(prompt.contains(&format!("goal: {CONTRACT_GOAL}")), "goal が goal として載る: {prompt}");
    assert!(prompt.contains(&format!("done: {CONTRACT_DONE}")), "done が done として載る: {prompt}");
    // **配列は各行**。1 要素しか測らないと「2 本目以降を捨てる」変異が生き残る。
    for want in [CONTRACT_VERIFY, CONTRACT_VERIFY_2, CONTRACT_WRITE_SET, CONTRACT_WRITE_SET_2] {
        assert!(prompt.contains(&format!("- {want}")), "prompt に契約の {want} が行として載る: {prompt}");
    }
    // **契約 file を丸写ししない**（NFR1・渡すほど cap を食う）。判定の材料にならない面が
    // 載っていないことまで測らないと、丸写しへ戻す変異が生き残る。
    for unwanted in [CONTRACT_OWNER, CONTRACT_DISPOSITION] {
        assert!(!prompt.contains(unwanted), "判定の材料にならない {unwanted} は載せない: {prompt}");
    }
    assert!(prompt.contains("--- a/src/lib.rs"), "diff も従来どおり載る: {prompt}");
    // 穴が埋まらずに残っていたら、lens が読むのは placeholder の字面だけになる。
    assert!(!prompt.contains("{contract}"), "契約の穴が埋まっている: {prompt}");
    assert!(!prompt.contains("{diff}"), "diff の穴が埋まっている: {prompt}");
    clean(&[&dir]);
}

/// lens の prompt は「verify は gate が済ませた・lens は tool を撃てない」前提を伝える
/// （`s2-07l.134`・設計 pipeline.md §5.3 / ADR-0011 §2.1）。前提が無いと実 lens は cargo を
/// 試して token を使い、撃てなかったことを INCONCLUSIVE の理由に混ぜた（.185 run 1 の実測）。
#[test]
fn headless_lens_prompt_states_verify_already_ran() {
    let prompt = lens_prompt_of_fixed_fixture();
    assert_eq!(
        prompt.matches(LENS_PREMISE_HEADING).count(),
        1,
        "前提の節の見出しがちょうど 1 回在る: {prompt}"
    );
    let premise = prompt.find(LENS_PREMISE_HEADING);
    let rubric = prompt.find("## 判定の決め方");
    let contract = prompt.find("## 契約");
    assert!(rubric.is_some() && contract.is_some(), "既存の見出しが在る: {prompt}");
    assert!(rubric < premise, "前提の節は「判定の決め方」より後: {prompt}");
    assert!(premise < contract, "前提の節は「## 契約」より前: {prompt}");
}

/// lens の prompt は審査の材料を契約と diff に限り、契約に名指しされていない検査を
/// 根拠にさせない（`s2-07l.231`・設計 pipeline.md §5.3）。限定が無いと実 lens は rustfmt の
/// 既定を持ち出して INCONCLUSIVE を出した（.223 run 1 の実測）。
///
/// ★文は契約 fixture にも diff fixture にも現れない字面——節を消せば回数が 0 に落ちる。
#[test]
fn headless_lens_scope_prompt_forbids_checks_not_named_by_contract() {
    const SCOPE_RULE: &str =
        "契約に名指しされていない検査（整形・rustfmt・lint の既定 等）を根拠に INCONCLUSIVE / FAIL を出さない。";
    const UNREACHED_RULE: &str = "判定に届かない周は、evidence に「契約のどの行を撃てなかったか」を書く。";
    let prompt = lens_prompt_of_fixed_fixture();
    assert_eq!(prompt.matches(SCOPE_RULE).count(), 1, "契約外の検査を根拠にしない文がちょうど 1 回在る: {prompt}");
    assert_eq!(prompt.matches(UNREACHED_RULE).count(), 1, "撃てなかった行を evidence に書く文が在る: {prompt}");
    let unfired = prompt.find("「verify を自分で撃てなかった」");
    let scope = prompt.find(SCOPE_RULE);
    let contract = prompt.find("## 契約");
    assert!(unfired.is_some() && contract.is_some(), "既存の句と見出しが在る: {prompt}");
    assert!(unfired < scope, "新しい節は「verify を自分で撃てなかった」の句より後: {prompt}");
    assert!(scope < contract, "新しい節は「## 契約」より前: {prompt}");
}

/// 契約の隣に材料の 2 file が在る周は雛形が契約の審査（`lens-contract.txt`）に切り替わる: 契約の各面と設計の節と
/// 要件本文がそれぞれの見出しの下に載り（順は 契約 → 設計の節 → 要件）、観点は 3 つで出力の形は diff の審査の
/// 2 key に理由の型 `kind` と場所 `at` の穴を足したもの（設計 contract-source.md §22・`s2-07l.395`）。
#[test]
fn headless_lens_contract_prompt_places_material_under_its_headings() {
    let prompt = lens_contract_prompt_of_fixed_fixture();
    let heading = |text: &str| prompt.find(text);
    let (design, requirements, contract) = (heading("## 契約が実装する設計の節"), heading("## 契約が満たす要件"), heading("## 契約"));
    assert!(contract.is_some() && design.is_some() && requirements.is_some(), "3 つの見出し: {prompt}");
    assert!(contract < design && design < requirements, "見出しの順は 契約 → 設計の節 → 要件: {prompt}");
    let mark = prompt.find("DESIGN-SECTION-MARK");
    assert!(mark > design && mark < requirements, "設計の節は自分の見出しの下: {prompt}");
    assert!(prompt.find("REQUIREMENT-MARK") > requirements, "要件本文は自分の見出しの下: {prompt}");
    assert!(prompt.contains(&format!("goal: {CONTRACT_GOAL}")) && prompt.contains(&format!("done: {CONTRACT_DONE}")), "契約の面: {prompt}");
    for want in [CONTRACT_VERIFY, CONTRACT_VERIFY_2, CONTRACT_WRITE_SET, CONTRACT_WRITE_SET_2] {
        assert!(prompt.contains(&format!("- {want}")), "契約の {want} が行として載る: {prompt}");
    }
    assert_eq!(prompt.matches("## 審査の観点").count(), 1, "観点の節がちょうど 1 回: {prompt}");
    for point in ["1. **契約と設計の節の適合**", "2. **設計が名指す状態遷移の一周**", "3. **write-set の連鎖**"] {
        assert_eq!(prompt.matches(point).count(), 1, "観点 {point} がちょうど 1 回: {prompt}");
    }
    assert!(
        prompt.contains(r#"{"verdict":"PASS|FAIL|INCONCLUSIVE","evidence":"<根拠を 1 行で>","kind":"<理由の型>","at":"<指した場所>"}"#),
        "出力の形は diff の審査の 2 key に kind と at の穴を足したもの: {prompt}"
    );
    for word in ["teeth-outside-write-set", "goal-done-contradiction", "vacuous-assert", "literal-mismatch", "section-material-missing", "other"] {
        assert_eq!(prompt.matches(&format!("`{word}`")).count(), 1, "kind の語 {word} がちょうど 1 回: {prompt}");
    }
    assert!(!prompt.contains("`unparsed`"), "7 語目 unparsed は器が倒す側で lens の語彙ではない: {prompt}");
}

/// 契約の審査の prompt は diff の節と裁定の節を持たず **stdin は読まれない**（stdin の字面は prompt に載らない）。
/// 穴は 3 つとも埋まり、diff の穴の字面も残らない。
#[test]
fn headless_lens_contract_prompt_ignores_stdin_and_fills_every_hole() {
    let prompt = lens_contract_prompt_of_fixed_fixture();
    assert!(!prompt.contains("## diff") && !prompt.contains("STDIN-MARK"), "diff の節は無く stdin は読まない: {prompt}");
    assert!(!prompt.contains("## 契約への裁定"), "裁定の節は diff の審査だけ: {prompt}");
    for hole in ["{contract}", "{design}", "{requirements}", "{diff}"] {
        assert!(!prompt.contains(hole), "穴 {hole} が埋まっている: {prompt}");
    }
}

/// 契約の隣に約束の行の写しが在る周は、要件の節の後に約束の行の見出しが 1 回載り、その下に 4 欄の写しが逐語で
/// （n の順のまま）載り、kind の限りが 3 語を名指す。穴 `{promises}` は残らない。写しの無い周の prompt は見出しを持たない（対）。
#[test]
fn headless_lens_promise_prompt_places_rows_after_requirements_and_names_three_kinds() {
    let prompt = lens_promise_prompt_of_fixed_fixture();
    let (requirements, promises) = (prompt.find("## 契約が満たす要件"), prompt.find("## 約束の行"));
    assert!(requirements.is_some() && promises > requirements, "約束の行は要件の節の後: {prompt}");
    assert_eq!(prompt.matches("## 約束の行").count(), 1, "見出しはちょうど 1 回: {prompt}");
    assert!(prompt.find(PROMISE_ROWS) > promises, "写しは逐語で見出しの下: {prompt}");
    let limit = "次の 3 語のちょうど 1 つに限る（他の語の FAIL は INCONCLUSIVE に倒される）: `goal-done-contradiction` / `vacuous-assert` / `other`。";
    assert_eq!(prompt.matches(limit).count(), 1, "kind の限りは 3 語: {prompt}");
    assert!(!prompt.contains("{promises}"), "穴は埋まる: {prompt}");
    let plain = lens_contract_prompt_of_fixed_fixture();
    assert!(!plain.contains("## 約束の行") && !plain.contains("{promises}"), "写しの無い周は見出しも穴も無い: {plain}");
    assert!(prompt.starts_with(plain.trim_end()), "写しの無い周の prompt は写しの在る周の頭と同じ字面");
}

/// 写しが在るのに読めない周（dir が置かれている）は claude を呼ばず rc 2（約束の行を落として審査しない）。
#[test]
fn headless_lens_promise_unreadable_rows_are_refused_without_calling_claude() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
    let contract = contract_in(&dir);
    material_in(&dir, Some(CONTRACT_DESIGN), Some(CONTRACT_REQUIREMENTS));
    fs::create_dir_all(dir.join("promises.txt")).expect("dir を作れる");
    let out = run_lens(&contract, 4096, "plan", &claude, CONTRACT_STDIN);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない写しは rc 2: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("promises.txt"), "file を名指す: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "claude を起動しない");
    clean(&[&dir]);
}

/// 材料が片方だけ在る周は壊れた材料として claude を呼ばず rc 2（無い方を名指す）。2 つとも無ければ従来の diff の
/// 審査（stdin が載る・対）。
#[test]
fn headless_lens_contract_half_material_is_refused_without_calling_claude() {
    for (design, requirements, missing) in [
        (Some(CONTRACT_DESIGN), None, "requirements.txt"),
        (None, Some(CONTRACT_REQUIREMENTS), "design.txt"),
    ] {
        let dir = tmp();
        let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
        let contract = contract_in(&dir);
        material_in(&dir, design, requirements);
        let out = run_lens(&contract, 4096, "plan", &claude, CONTRACT_STDIN);
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "片方だけは rc 2: {}", stderr_of(&out));
        assert!(stderr_of(&out).contains(missing), "無い方を名指す: {}", stderr_of(&out));
        assert!(!dir.join("called").exists(), "claude を起動しない");
        clean(&[&dir]);
    }
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &claude, CONTRACT_STDIN);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    assert!(prompt.contains("STDIN-MARK") && prompt.contains("## diff"), "材料が無ければ diff の審査: {prompt}");
    clean(&[&dir]);
}

/// cap は契約 + 節 + 要件の byte で照合する（NFR1・FR9）: 材料が cap を超える周は claude を呼ばず INCONCLUSIVE。
/// 同じ cap で stdin が空の diff の審査は呼ばれる（対＝cap を測っているのは材料の byte）。
#[test]
fn headless_lens_contract_material_over_cap_is_inconclusive_without_calling_claude() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
    let contract = contract_in(&dir);
    material_in(&dir, Some(CONTRACT_DESIGN), Some(CONTRACT_REQUIREMENTS));
    let out = run_lens(&contract, 64, "plan", &claude, b"");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(stdout_of(&out).contains(r#""verdict":"INCONCLUSIVE""#), "cap 超は INCONCLUSIVE: {}", stdout_of(&out));
    assert!(stdout_of(&out).contains("contract material exceeds cap"), "理由は材料の cap 超: {}", stdout_of(&out));
    assert!(!dir.join("called").exists(), "claude を起動しない");
    clean(&[&dir]);
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 64, "plan", &claude, b"");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "材料が無い周は diff（0 byte）が cap の内側＝呼ぶ");
    clean(&[&dir]);
}

/// lens は `{contract}` の path の**同じ dir** の `rulings.txt` を読み、本文をそのまま `{rulings}` の穴へ埋める
/// （`s2-07l.309`・設計 pipeline-question.md）。節は `## 契約` の後・`## diff` の前。base は穴も節も無いので RED。
#[test]
fn lens_rulings_are_filled_into_the_prompt_from_the_sibling_file() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"裁定を読めた\"}\n", false, 0);
    let contract = contract_in(&dir);
    fs::write(dir.join("rulings.txt"), RULINGS_FIXTURE).expect("裁定の file を書ける");
    let out = run_lens(&contract, 4096, "plan", &claude, b"DIFF-BODY-MARKER\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "裁定が在る周も claude を呼ぶ");
    let prompt = slurp(&dir.join("stdin"));
    assert_eq!(prompt.matches(LENS_RULINGS_HEADING).count(), 1, "裁定の節の見出しがちょうど 1 回在る: {prompt}");
    // **本文はそのまま**（見出しの直下・逐語・穴は展開されない）。
    assert!(
        prompt.contains(&format!("{LENS_RULINGS_HEADING}\n{RULINGS_FIXTURE}")),
        "裁定の本文が見出しの直下に逐語で載る: {prompt}"
    );
    assert_eq!(prompt.matches("DIFF-BODY-MARKER").count(), 1, "裁定の中の {{diff}} は展開されない: {prompt}");
    assert!(!prompt.contains("{rulings}"), "裁定の穴が埋まっている: {prompt}");
    assert!(!prompt.contains("（裁定なし）"), "裁定が在る周に「裁定なし」を出さない: {prompt}");
    let contract_at = prompt.find("## 契約\n");
    let rulings_at = prompt.find(LENS_RULINGS_HEADING);
    let diff_at = prompt.find("## diff");
    assert!(contract_at.is_some() && diff_at.is_some(), "既存の見出しが在る: {prompt}");
    assert!(contract_at < rulings_at, "裁定の節は「## 契約」より後: {prompt}");
    assert!(rulings_at < diff_at, "裁定の節は「## diff」より前: {prompt}");
    // 読み方の 1 行は「審査の材料」の節に在り、裁定の節より前。
    const RULING_RULE: &str = "裁定の節に在る逸脱（回答で認めた形）は契約の一部として読む。裁定に無い逸脱だけを契約違反と読む。";
    assert_eq!(prompt.matches(RULING_RULE).count(), 1, "読み方の行がちょうど 1 回在る: {prompt}");
    assert!(prompt.find(RULING_RULE) < contract_at, "読み方の行は「## 契約」より前: {prompt}");
    clean(&[&dir]);
}

/// 裁定の file が無い周は `（裁定なし）` の 1 行を穴へ埋める（「裁定なし」を明示する・C10・空を黙らせない）。
#[test]
fn lens_rulings_absent_reads_as_none() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"裁定なし\"}\n", false, 0);
    let contract = contract_in(&dir);
    assert!(!dir.join("rulings.txt").exists(), "fixture: 裁定の file は無い");
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "裁定が無くても claude を呼ぶ");
    let prompt = slurp(&dir.join("stdin"));
    assert_eq!(prompt.matches(LENS_RULINGS_HEADING).count(), 1, "裁定の節の見出しは在る: {prompt}");
    assert!(
        prompt.contains(&format!("{LENS_RULINGS_HEADING}\n（裁定なし）\n")),
        "見出しの直下に「裁定なし」の 1 行: {prompt}"
    );
    assert_eq!(prompt.matches("（裁定なし）").count(), 1, "「裁定なし」はちょうど 1 回: {prompt}");
    assert!(!prompt.contains("{rulings}"), "裁定の穴が埋まっている: {prompt}");
    clean(&[&dir]);
}

/// 裁定の file が**在るのに読めない**周（file の場所に dir が置かれている・UTF-8 でない）は claude を呼ばず rc 2
/// で理由を 1 行（`lens: 裁定を読めない`）。「無い」と「読めない」で極性を変える＝読めない裁定を「裁定なし」に
/// 倒すと、回答で認めた逸脱が契約違反に読まれる。
#[test]
fn lens_rulings_unreadable_is_broken() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let contract = contract_in(&dir);
    fs::create_dir(dir.join("rulings.txt")).expect("file の場所に dir を置ける");
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない裁定は rc 2: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    assert!(!dir.join("stdin").exists(), "prompt の写しも生成されない");
    assert!(stderr_of(&out).contains("lens: 裁定を読めない"), "理由を名乗る: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("rulings.txt"), "読めなかった path を名指す: {}", stderr_of(&out));
    // UTF-8 でない本文も同じ極性（「在るが読めない」）。
    let bad = tmp();
    let quiet = fake_claude(&bad, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let bad_contract = contract_in(&bad);
    fs::write(bad.join("rulings.txt"), [0xff_u8, 0xfe, 0x00]).expect("壊れた本文を書ける");
    let out = run_lens(&bad_contract, 4096, "plan", &quiet, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "UTF-8 でない裁定も rc 2: {}", stderr_of(&out));
    assert!(!bad.join("called").exists(), "claude を 1 度も起動しない");
    clean(&[&dir, &bad]);
}

#[test]
fn headless_lens_fills_holes_in_one_pass() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
    // **契約は外から来る text である**。goal に穴の字面を書けるので、重ねて replace すると
    // 先に埋めた契約本文の中の `{diff}` が次の走査で展開され、契約に 1 語書くだけで
    // prompt の構造へ触れられる。runner 側と同じ経路を lens でも測る（review 2026-09-10 F2）。
    let contract = dir.join("holes.toml");
    fs::write(&contract, contract_text("穴の字面 {diff} を持つ goal")).expect("契約 file を書ける");
    let out = run_lens(&contract, 4096, "plan", &claude, b"DIFF-BODY-MARKER\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    // 埋めた値を二度と走査しない＝契約に書いた穴の字面は**そのまま残る**。
    assert!(
        prompt.contains("穴の字面 {diff} を持つ goal"),
        "契約本文の穴は展開されない: {prompt}"
    );
    // 展開されていれば diff の本文が契約の中にも現れ、2 か所になる。
    assert_eq!(
        prompt.matches("DIFF-BODY-MARKER").count(),
        1,
        "diff が載るのは 1 か所だけ: {prompt}"
    );
    clean(&[&dir]);
}

/// lens は**渡された worktree で** claude を起こす（憲法を載せる経路は cwd 1 本）。
///
/// worktree は fake の置き場と**別の dir** にする——同じにすると「cwd を渡さず継承した」
/// 実装でも assert が真になり、歯が空虚になる。
#[test]
fn headless_lens_runs_claude_in_the_given_worktree() {
    let dir = tmp();
    let worktree = tmp();
    let contract = contract_in(&dir);
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"ok\"}\n", false, 0);
    let rules = rules_with_cap(&dir, 4096);
    let out = run_bin(
        &dir,
        &[
            "lens",
            "--contract", &contract.display().to_string(),
            "--worktree", &worktree.display().to_string(),
            "--rules", &rules.display().to_string(),
            "--permission-mode", "plan",
            "--claude", &claude.display().to_string(),
        ],
        b"--- a\n+++ b\n",
    );
    assert_eq!(out.status.code(), Some(0), "判定は返る: {}", stderr_of(&out));
    let seen = slurp(&dir.join("cwd"));
    assert_eq!(seen.trim(), worktree.display().to_string(), "cwd は渡された worktree");
    assert_ne!(seen.trim(), dir.display().to_string(), "契約の置き場を cwd にしていない");
    clean(&[&dir, &worktree]);
}

/// `--worktree` が無ければ claude を起こさずに断る（`--contract` と同じ極性）。
#[test]
fn headless_lens_refuses_without_worktree() {
    let dir = tmp();
    let contract = contract_in(&dir);
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let rules = rules_with_cap(&dir, 4096);
    let out = run_bin(
        &dir,
        &[
            "lens",
            "--contract", &contract.display().to_string(),
            "--rules", &rules.display().to_string(), "--permission-mode", "plan",
            "--claude", &claude.display().to_string(),
        ],
        b"--- a\n+++ b\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "worktree が無ければ rc 1");
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    assert!(
        stderr_of(&out).contains("--worktree"),
        "何が要るかを名乗る: {}",
        stderr_of(&out)
    );
    clean(&[&dir]);
}

#[test]
fn headless_lens_refuses_without_contract() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let rules = rules_with_cap(&dir, 4096);
    let out = run_bin(
        &dir,
        &[
            "lens", "--rules", &rules.display().to_string(), "--permission-mode", "plan",
            "--claude", &claude.display().to_string(),
        ],
        b"--- a\n+++ b\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "契約が無ければ rc 1");
    // **効果で測る**: 材料が足りないまま呼べば返るのは INCONCLUSIVE だけで、払った
    // 1 回分が捨て金になる。呼んでいないことを痕跡の不在で見る（cap 超過の歯と同型）。
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    assert!(
        stderr_of(&out).contains("--contract"),
        "何が要るかを名乗る: {}",
        stderr_of(&out)
    );
    // 読めない契約でも claude を呼ばない（「無い」と「壊れている」で極性を変えない）。
    let broken = dir.join("broken.toml");
    fs::write(&broken, "goal = \n").expect("壊れた契約を書ける");
    let out = run_lens(&broken, 4096, "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない契約は rc 2");
    assert!(!dir.join("called").exists(), "読めない契約でも claude を起動しない");
    clean(&[&dir]);
}

/// lens が起こす claude も同じ形で起きる（構築点は `build` 1 つ・ADR-0011 §2.1）。
///
/// lens は `--allowedTools` を渡さない側だが、**settings 由来の allow は権限の口を開ける**ので
/// runner と同じ 5 点を lens でも測る（片方だけ塞ぐ変異を落とす）。
#[test]
fn headless_lens_loads_no_settings_from_account_or_checkout() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"ok\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let args = slurp(&dir.join("args"));
    assert!(pair(&args, "--setting-sources", ""), "空の値を **対**で渡す: {args}");
    for source in ["project", "user", "local"] {
        assert!(!pair(&args, "--setting-sources", source), "{source} の settings を読まない: {args}");
    }
    assert!(has_arg(&args, "--strict-mcp-config"), "MCP も宣言外を拾わない: {args}");
    assert!(!has_arg(&args, "--settings"), "settings を file で渡し直さない: {args}");
    assert!(!has_arg(&args, "--restricted"), "restricted は使わない: {args}");
    clean(&[&dir]);
}

/// lens は **`--allowedTools` を渡さない**（ADR-0011 §2.2: lens には器の hook も allow も載らない）。
///
/// runner 側の「在る」は [`headless_runner_passes_allowed_tools_from_vessel_copy`] が持つので、
/// この歯は lens だけを見る。allow を lens の呼出側に足す変異はどの既存の歯にも当たらず、
/// 権限を持った review が静かに始まる。
#[test]
fn headless_lens_passes_no_allowed_tools_absent_from_argv() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"ok\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let args = slurp(&dir.join("args"));
    assert!(!has_arg(&args, "--allowedTools"), "lens に allow は載らない（ADR-0011 §2.2）: {args}");
    clean(&[&dir]);
}
