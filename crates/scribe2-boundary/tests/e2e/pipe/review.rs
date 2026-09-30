// flip-check: moved s2-07l.351
//! 審査の段の歯: `pipe_review_`（契約の審査の段・審査の理由の閉じた型・要件の読み）と、同じ組み手で事前審査の待ち行に
//! lens を先に撃つ先撃ちの歯 `pipe_prelens_`（設計 dispatcher.md §27 形 aa・行 aa）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引き、受付の口の歯と共有する helper は
//! `use super::intake::{…}` で引く（歯の本文は `intake.rs` から移しただけ・`s2-07l.351`）。

use super::*;
use super::intake::{lens_finding, review_has, review_pairs, reviewed_detail};

// ───── 契約の審査の段（`s2-07l.241`・設計 contract-source.md §4・SRS FR49 / FR9 / AC22・接頭辞 `pipe_review_`） ─────

/// (a) 偽 lens が FAIL を返す契約は `Reviewed(FAIL)` で止まり **runner は 1 度も起きない**（構築点の呼出 0・AC22）:
/// `pipe run` は rc 1 で intake の判定行と `stage=Reviewed verdict=FAIL` を出し、trail は `RunCreated(Intake)` →
/// `RunStage(Reviewed, verdict:FAIL kind:unparsed)` で終わる（Spawned 無し・worktree 無し・`kind` を書かない偽 lens は
/// 7 語目・`s2-07l.395`）。`review.json` に verdict と evidence が残り、`show` は `Reviewed` を名乗る。
#[test]
fn pipe_review_fail_stops_before_spawn() {
    let (repo, state) = repo_with_state();
    let (id, _) = reviewed_fail(&repo, &state);
    assert_eq!(
        trail(&state, &id),
        vec![
            (EventKind::RunCreated, Some(Stage::Intake), Some("classes:".to_owned())),
            (EventKind::RunStage, Some(Stage::Reviewed), Some("verdict:FAIL kind:unparsed".to_owned())),
        ],
        "Reviewed(FAIL) が終端（Spawned 無し）"
    );
    assert!(!worktree_of(&repo, &id).exists(), "worktree を作らない");
    let pairs = review_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "FAIL", "review.json の verdict");
    assert_eq!(value_of(&pairs, "evidence"), "fake", "lens の evidence を写す");
    assert_eq!(value_of(&pairs, "run"), id, "run id を持つ");
    assert!(show_line(&repo, &state, &id).contains("stage=Reviewed"), "段は Reviewed");
    clean(&[&repo, &state]);
}

/// (a'') `Reviewed(FAIL)` は終端: `spawn` / `resume` は段違いとして rc 1 で何も書かず runner を起こさず、`stop` は
/// 終端として断る。終端ゆえ同じ write-set の 2 本目の intake は交差で断られない（`live` は偽）。
#[test]
fn pipe_review_fail_is_terminal_for_spawn_resume_stop_and_overlap() {
    let (repo, state) = repo_with_state();
    let (id, runner_marker) = reviewed_fail(&repo, &state);
    let before = event_count(&state);
    let spawned = spawn_with(&repo, &state, &id, &marker_runner(&runner_marker));
    assert_eq!(spawned.status.code(), Some(i32::from(RC_REFUSED)), "spawn は段違い: {}", stderr_of(&spawned));
    assert!(stderr_of(&spawned).contains("Reviewed") && stderr_of(&spawned).contains("verdict=FAIL"), "{}", stderr_of(&spawned));
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &marker_runner(&runner_marker),
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_REFUSED)), "resume も起こさない: {}", stderr_of(&resumed));
    assert!(!runner_marker.exists(), "spawn / resume のどちらでも runner は起きない");
    assert_eq!(event_count(&state), before, "段違いは event を 1 件も書かない");
    let stopped = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(stopped.status.code(), Some(i32::from(RC_REFUSED)), "終端の便は stop で断る: {}", stderr_of(&stopped));
    assert!(stderr_of(&stopped).contains("終端"), "{}", stderr_of(&stopped));
    // 終端ゆえ交差の母集団に入らない（`live` は偽）。
    let next = write_set_contract(&repo, "next", &["src/lib.rs"]);
    let again = try_intake(&repo, &state, &next, "s2-next");
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "FAIL の便と交差しても受理: {}", stderr_of(&again));
    clean(&[&repo, &state]);
}

/// (a') lens が無い・出力を読めない周は INCONCLUSIVE（FR9・偽の PASS を作らない）で、終端として spawn を断る。
/// `--lens` 無しの intake は rc 3 で `verdict:INCONCLUSIVE` を記帳し evidence が `--lens` を名指す。
#[test]
fn pipe_review_inconclusive_without_lens_or_unreadable_output_is_terminal() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let rules = ceiling_rules(&state);
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "s2-none",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(), "--rules", &rules,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "lens 無しは rc 3: {}", stderr_of(&out));
    let id = run_id_of(&out);
    assert_eq!(stages(&state, &id), vec![(Some(Stage::Reviewed), Some("verdict:INCONCLUSIVE kind:unparsed".to_owned()))]);
    assert!(value_of(&review_pairs(&state, &id), "evidence").contains("--lens"), "{:?}", review_pairs(&state, &id));
    let marker = state.join("runner-ran");
    let spawned = spawn_with(&repo, &state, &id, &marker_runner(&marker));
    assert_eq!(spawned.status.code(), Some(i32::from(RC_REFUSED)), "INCONCLUSIVE は起こさない: {}", stderr_of(&spawned));
    assert!(stderr_of(&spawned).contains("verdict=INCONCLUSIVE"), "{}", stderr_of(&spawned));
    assert!(!marker.exists(), "runner は起きない");
    // 読めない出力（JSON 行が無い）・3 値の外・rc≠0 の lens も INCONCLUSIVE。
    for (bead, lens, want) in [
        ("s2-nojson", "cat >/dev/null; echo not-json".to_owned(), "JSON 行が無い"),
        ("s2-3v", format!("cat >/dev/null; echo '{}'", lens_verdict("MAYBE")), "3 値でない"),
        ("s2-rc", "cat >/dev/null; exit 7".to_owned(), "rc 7"),
    ] {
        let out = run_pipe(&[
            "intake", "--design", &path, "--bead", bead,
            "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
            "--rules", &rules, "--lens", &lens,
        ]);
        assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{bead}: {}", stderr_of(&out));
        let pairs = review_pairs(&state, &run_id_of(&out));
        assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "{bead}");
        assert!(value_of(&pairs, "evidence").contains(want), "{bead}: {pairs:?}");
    }
    clean(&[&repo, &state]);
}

/// (b) PASS の契約は `Reviewed(PASS)` を経て Spawned へ進み、verdict が便の記録（`review.json` と event）に残る（AC22）。
/// 審査の材料は run dir の `review/`（契約の写し・設計の節・要件本文）に置かれ、lens の `{contract}` はその写しの
/// path・`{worktree}` は base の repo で埋まる。pointer でない `design` と読めない要件面は材料の本文に明示される。
#[test]
fn pipe_review_pass_spawns() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let seen = state.join("lens-holes");
    let lens = format!(
        "cat >/dev/null; printf '%s\\n' '{{contract}}' '{{worktree}}' > '{}'; echo '{}'",
        seen.display(),
        lens_verdict("PASS")
    );
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS は rc 0: {}", stderr_of(&out));
    let id = run_id_of(&out);
    assert!(stdout_of(&out).contains(&format!("run={id} stage=Reviewed verdict=PASS")), "{}", stdout_of(&out));
    assert_eq!(stages(&state, &id), vec![(Some(Stage::Reviewed), Some("verdict:PASS".to_owned()))]);
    assert_eq!(event_count(&state), 2, "RunCreated + Reviewed");
    assert_eq!(value_of(&review_pairs(&state, &id), "verdict"), "PASS");
    let dir = review_dir(&state, &id);
    let holes = fs::read_to_string(&seen).unwrap_or_default();
    assert_eq!(
        holes.lines().collect::<Vec<&str>>(),
        [dir.join("contract.toml").display().to_string(), repo.display().to_string()],
        "{{contract}} は審査の写し・{{worktree}} は base の repo"
    );
    assert_eq!(
        dir_names(&dir),
        ["base.txt", "contract.toml", "design.txt", "requirements.txt"],
        "材料の 3 file と base の要約（§40）"
    );
    assert_eq!(
        fs::read(dir.join("contract.toml")).ok(),
        fs::read(state.join("pipe").join(&id).join("contract.toml")).ok(),
        "契約の写しは byte で同じ"
    );
    // 契約 (b) 以後、`design` は**必ず**設計 pointer である（契約 file は行から作られる）＝材料は節の
    // 出所と本文をそのまま持つ（「pointer でない」形は入口から消えた）。
    let design = fs::read_to_string(dir.join("design.txt")).unwrap_or_default();
    assert!(design.starts_with(&format!("{} §1\n", design_pointer())), "節の出所を名乗る: {design}");
    assert!(design.contains("節の本文"), "節の本文を写す: {design}");
    let requirements = fs::read_to_string(dir.join("requirements.txt")).unwrap_or_default();
    assert!(requirements.contains("FR4"), "行の req を材料に載せる: {requirements}");
    // PASS の便だけが起こせる。
    let spawned = spawn_with(&repo, &state, &id, TOY_COMMIT);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "PASS → Spawned: {}", stderr_of(&spawned));
    let listed: Vec<Option<Stage>> = stages(&state, &id).into_iter().map(|(stage, _)| stage).collect();
    assert_eq!(listed, vec![Some(Stage::Reviewed), Some(Stage::Spawned), Some(Stage::Implemented)], "Reviewed → Spawned → Implemented");
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"));
    clean(&[&repo, &state]);
}

// ───── write-set の base の要約（`s2-07l.431`・設計 contract-source.md §40・接頭辞 `pipe_review_base_`） ─────

/// (f) 受付から審査まで通した run の材料の dir に base の要約の file が在り、write-set の各項目の path を宣言順に 1 項目
/// ずつ持つ（base に在る `.rs` は行数と宣言と歯の列・`+` の項目は新設の 1 行）。既存の 3 材料の file も並んで在る。
#[test]
fn pipe_review_base_summary_file_names_every_write_set_item() {
    let (repo, state) = repo_with_state();
    let items = ["src/lib.rs", "src/zq_base.rs", "+src/zq_fresh.rs"];
    let path = write_set_contract(&repo, "base", &items);
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "s2-base",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state), "--lens", &format!("cat >/dev/null; echo '{}'", lens_verdict("PASS")),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS は rc 0: {}", stderr_of(&out));
    let dir = review_dir(&state, &run_id_of(&out));
    assert_eq!(dir_names(&dir), ["base.txt", "contract.toml", "design.txt", "requirements.txt"], "要約の file が 1 本増える");
    let summary = fs::read_to_string(dir.join("base.txt")).unwrap_or_default();
    let heads: Vec<&str> =
        summary.lines().filter_map(|line| line.strip_prefix("- ")).filter_map(|line| line.split(": ").next()).collect();
    assert_eq!(heads, items, "write-set の各項目の path を宣言順に 1 項目ずつ: {summary}");
    assert!(summary.contains("- src/zq_base.rs: 行数 全体 "), "base に在る file は行数を持つ: {summary}");
    assert!(summary.contains("\n  宣言: ") && summary.contains("\n  歯: "), ".rs は宣言と歯の列を持つ: {summary}");
    assert!(summary.contains("- +src/zq_fresh.rs: 新設（base に無い）"), "{summary}");
    clean(&[&repo, &state]);
}

/// (g) 置き場だけの印（`=`）の項目を持つ契約の審査の材料 `base.txt` は「読めない」を 1 行も持たず、その項目の行は
/// 契約の字面のまま本文を読んで行数と置き場だけの 1 語を持ち、宣言と歯の列が続く（§44・行 au）。
#[test]
fn pipe_review_base_place_only_item_is_read_in_base_txt() {
    let (repo, state) = repo_with_state();
    let _ = fs::write(repo.join("src").join("zq_place.rs"), "pub fn placed() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn zq_tooth() {}\n}\n");
    let items = ["src/lib.rs", "=src/zq_place.rs"];
    let path = write_set_contract(&repo, "place", &items);
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "s2-place",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state), "--lens", &format!("cat >/dev/null; echo '{}'", lens_verdict("PASS")),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS は rc 0: {}", stderr_of(&out));
    let summary = fs::read_to_string(review_dir(&state, &run_id_of(&out)).join("base.txt")).unwrap_or_default();
    assert_eq!(summary.lines().filter(|line| line.contains("読めない")).count(), 0, "{summary}");
    let lines: Vec<&str> = summary.lines().collect();
    let at = lines.iter().position(|line| line.starts_with("- =src/zq_place.rs: ")).unwrap_or(lines.len());
    assert_eq!(
        lines.get(at..at.saturating_add(3)),
        Some(&["- =src/zq_place.rs: 行数 全体 7 / 本体 2・置き場だけ（中身は変えない）", "  宣言: pub fn placed", "  歯: zq_tooth"][..]),
        "{summary}"
    );
    clean(&[&repo, &state]);
}

// ───── write-set の外の材料（`s2-07l.430`・設計 contract-source.md §51・行 bc・接頭辞 `pipe_review_outside_`） ─────

/// 受付から審査まで通した run の材料の dir に外の材料の file が在り、§ が backtick の外で名指した write-set の外の struct の
/// 所在と可視性つきの宣言の行と field を持つ。名指しの無い既存の契約の材料の dir は 4 本のまま（歯
/// `pipe_review_base_summary_file_names_every_write_set_item`）。
#[test]
fn pipe_review_outside_material_carries_the_named_outside_struct() {
    let row = derive_row("o", &[("write-set", "[\"crates/toy/src/tint.rs\"]"), ("section", "\"2\""), ("req", "[\"FR2\"]")]);
    let doc = table_doc(&table_region(&[row])).replace("## 2. 型\n\n本文。", "## 2. 型\n\n節は ZqOuterShape の field を読む。");
    let shape = "/// 外の形。\npub(crate) struct ZqOuterShape {\n    pub zq_width: u8,\n}\n";
    let (repo, state) = derive_repo_with(&doc, &[("crates/other/src/zq_shape.rs", shape)]);
    let out = intake_raw(&repo, &state, "docs/design/toy.md#o", "s2-o");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let dir = review_dir(&state, &run_id_of(&out));
    assert_eq!(
        dir_names(&dir),
        ["base.txt", "contract.toml", "design.txt", "outside.txt", "requirements.txt"],
        "外の材料の file が 1 本増える"
    );
    let outside = fs::read_to_string(dir.join("outside.txt")).unwrap_or_default();
    assert!(outside.contains("- ZqOuterShape: crates/other/src/zq_shape.rs:2\n  /// 外の形。\n"), "所在と doc 行: {outside}");
    assert!(outside.contains("\n  pub(crate) struct ZqOuterShape {\n      pub zq_width: u8,\n  }"), "宣言の行と field: {outside}");
    clean(&[&repo, &state]);
}

// ───── 審査の理由の閉じた型（`s2-07l.395`・設計 contract-source.md §22・SRS FR49・接頭辞 `pipe_review_kind_`） ─────

/// 歯 (1) **読みと書き**: FAIL の周に lens の `kind` と `at` が `review.json` の任意 field に逐語で残り、同じ周の event の
/// detail は `verdict:FAIL kind:<k>` の **2 語だけ**（`at` は event に載せない）。6 語をそれぞれ書いた周でその語が
/// 両面に残り、INCONCLUSIVE の周も同じ形。verdict の 3 値と rc は不変（FAIL は rc 1・INCONCLUSIVE は rc 3）。
#[test]
fn pipe_review_kind_fail_keeps_kind_and_at_in_review_json_and_two_word_detail() {
    let (repo, state) = repo_with_state();
    for (index, word) in LENS_KINDS.iter().enumerate() {
        let bead = format!("s2-k{index}");
        let out = intake_with_lens(&repo, &state, &bead, &lens_finding("FAIL", Some(word), Some(LENS_AT)));
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{word}: FAIL は rc 1 のまま: {}", stderr_of(&out));
        let id = run_id_of(&out);
        let pairs = review_pairs(&state, &id);
        assert_eq!(value_of(&pairs, "verdict"), "FAIL", "{word}: verdict は lens の値");
        assert_eq!(value_of(&pairs, "kind"), *word, "{word}: kind が逐語で残る: {pairs:?}");
        assert_eq!(value_of(&pairs, "at"), LENS_AT, "{word}: at が逐語で残る: {pairs:?}");
        assert_eq!(value_of(&pairs, "evidence"), "fake", "{word}: 既存 key は不変");
        assert_eq!(value_of(&pairs, "schema"), "1", "{word}: schema は 1 のまま");
        let detail = reviewed_detail(&state, &id);
        assert_eq!(detail, format!("verdict:FAIL kind:{word}"), "{word}: detail は 2 語");
        assert_eq!(detail.split_whitespace().count(), 2, "{word}: at は event に載せない: {detail}");
        assert!(!detail.contains(LENS_AT) && !detail.contains("§2"), "{word}: {detail}");
    }
    let out = intake_with_lens(&repo, &state, "s2-kinc", &lens_finding("INCONCLUSIVE", Some("other"), Some("x")));
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "INCONCLUSIVE は rc 3 のまま: {}", stderr_of(&out));
    let id = run_id_of(&out);
    let pairs = review_pairs(&state, &id);
    assert_eq!((value_of(&pairs, "verdict"), value_of(&pairs, "kind"), value_of(&pairs, "at")), ("INCONCLUSIVE".to_owned(), "other".to_owned(), "x".to_owned()));
    assert_eq!(reviewed_detail(&state, &id), "verdict:INCONCLUSIVE kind:other");
    clean(&[&repo, &state]);
}

/// 歯 (1) の否定の枝: PASS の周は `review.json` に `kind` も `at` も持たず detail は `verdict:PASS` のまま——lens が PASS に
/// `kind` / `at` を書いても持たない（PASS に理由の型は無い・空の値を作らない）。
#[test]
fn pipe_review_kind_pass_carries_neither_kind_nor_at() {
    let (repo, state) = repo_with_state();
    let out = intake_with_lens(&repo, &state, "s2-kpass", &lens_finding("PASS", Some("other"), Some(LENS_AT)));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS は rc 0: {}", stderr_of(&out));
    let id = run_id_of(&out);
    assert_eq!(value_of(&review_pairs(&state, &id), "verdict"), "PASS");
    assert!(!review_has(&state, &id, "kind"), "PASS は kind を持たない: {:?}", review_pairs(&state, &id));
    assert!(!review_has(&state, &id, "at"), "PASS は at を持たない: {:?}", review_pairs(&state, &id));
    assert_eq!(reviewed_detail(&state, &id), "verdict:PASS", "detail は従来どおり 1 語");
    clean(&[&repo, &state]);
}

/// 歯 (2) **7 語目へ倒す枝**:FAIL / INCONCLUSIVE で `kind` が無い周・語でない周は `unparsed` になり **verdict は lens の値
/// のまま**（`other` にも INCONCLUSIVE にも化けない・C10）。JSON が読めない周・3 値でない周・rc≠0 の周・`--lens` 無しの
/// 周（器が作る INCONCLUSIVE）も `unparsed`。どの周も `at` を持たない。
#[test]
fn pipe_review_kind_missing_or_unknown_or_unreadable_falls_to_unparsed_without_moving_the_verdict() {
    let (repo, state) = repo_with_state();
    for (bead, line, verdict, rc) in [
        ("s2-u1", lens_finding("FAIL", None, None), "FAIL", RC_REFUSED),
        ("s2-u2", lens_finding("FAIL", Some("bogus-word"), None), "FAIL", RC_REFUSED),
        ("s2-u3", lens_finding("INCONCLUSIVE", None, None), "INCONCLUSIVE", RC_INCONCLUSIVE),
        ("s2-u4", lens_finding("INCONCLUSIVE", Some("Other"), None), "INCONCLUSIVE", RC_INCONCLUSIVE),
        ("s2-u5", lens_finding("FAIL", Some("unparsed"), None), "FAIL", RC_REFUSED),
        ("s2-u6", "not-json".to_owned(), "INCONCLUSIVE", RC_INCONCLUSIVE),
        ("s2-u7", lens_finding("MAYBE", Some("other"), None), "INCONCLUSIVE", RC_INCONCLUSIVE),
    ] {
        let out = intake_with_lens(&repo, &state, bead, &line);
        assert_eq!(out.status.code(), Some(i32::from(rc)), "{bead}: rc は不変: {}", stderr_of(&out));
        let id = run_id_of(&out);
        let pairs = review_pairs(&state, &id);
        assert_eq!(value_of(&pairs, "verdict"), verdict, "{bead}: verdict は動かない: {pairs:?}");
        assert_eq!(value_of(&pairs, "kind"), "unparsed", "{bead}: 7 語目: {pairs:?}");
        assert!(!review_has(&state, &id, "at"), "{bead}: at は無い: {pairs:?}");
        assert_eq!(reviewed_detail(&state, &id), format!("verdict:{verdict} kind:unparsed"), "{bead}");
    }
    // 出力を読めない（rc≠0）・`--lens` 無し（器が作る INCONCLUSIVE の 2 形・残る 3 形は stop.rs / ratelimit.rs と in-crate）。
    let path = write_contract(&repo, &[], &[]);
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "s2-u8",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state), "--lens", "cat >/dev/null; exit 7",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    let id = run_id_of(&out);
    assert_eq!(value_of(&review_pairs(&state, &id), "kind"), "unparsed", "rc 7 は 7 語目");
    assert_eq!(reviewed_detail(&state, &id), "verdict:INCONCLUSIVE kind:unparsed");
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "s2-u9",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    let id = run_id_of(&out);
    let pairs = review_pairs(&state, &id);
    assert!(value_of(&pairs, "evidence").contains("--lens"), "理由は lens 無しのまま: {pairs:?}");
    assert_eq!(value_of(&pairs, "kind"), "unparsed", "--lens 無しは 7 語目: {pairs:?}");
    assert!(!review_has(&state, &id, "at"), "{pairs:?}");
    assert_eq!(reviewed_detail(&state, &id), "verdict:INCONCLUSIVE kind:unparsed");
    clean(&[&repo, &state]);
}

// ───── done の項目ごとの歯の対応の表（設計 contract-source.md §64・行 bs・接頭辞 `pipe_review_done_items_`） ─────

/// (c)〜(f) が撃つ同じ done（順の外の印 (2) を 1 つ持つ 3 項目・書き手と読みの数え方が違うと (d)〜(f) が落ちる）。
const ITEMS_DONE: &str = "(1) 甲を作る (2) 乙を測る 形 (2) の字 (3) 丙を足す";

/// 偽 lens の最終行: [`lens_finding`] に key done を足す（`table` は JSON の値の字面・`None` は key を書かない）。
fn table_finding(verdict: &str, kind: Option<&str>, at: Option<&str>, table: Option<&str>) -> String {
    let line = lens_finding(verdict, kind, at);
    table.map_or_else(|| line.clone(), |value| format!("{},\"done\":{value}}}", line.trim_end_matches('}')))
}

/// 表が `table`（文字列の値）の PASS の最終行。
fn table_pass(table: &str) -> String {
    table_finding("PASS", None, None, Some(&format!("\"{table}\"")))
}

/// done が `done` の行を受付から審査まで通す（`lens` は偽 lens の全文・`None` は `--lens` 無し）。
fn done_intake(repo: &Path, state: &Path, bead: &str, done: &str, lens: Option<&str>) -> Output {
    let path = write_contract(repo, &["done"], &[&format!("done = \"{done}\"")]);
    let (repo, state_dir, rules) = (repo.display().to_string(), state.display().to_string(), ceiling_rules(state));
    let mut args: Vec<&str> = vec!["intake", "--design", &path, "--bead", bead, "--repo", &repo, "--state-dir", &state_dir, "--rules", &rules];
    if let Some(found) = lens {
        args.extend(["--lens", found]);
    }
    run_pipe(&args)
}

/// 偽 lens が `line` を最終行に書く周の全文（rc 0）。
fn says(state: &Path, line: &str) -> String {
    fake_lens(&state.join("lens-ran"), line)
}

/// 審査の判定の 1 行: `rc|verdict|kind|at|evidence`（無い key は `<無し>`）。
fn judged(state: &Path, out: &Output) -> String {
    let id = run_id_of(out);
    let pairs = review_pairs(state, &id);
    let field = |key: &str| if review_has(state, &id, key) { value_of(&pairs, key) } else { "<無し>".to_owned() };
    format!("{}|{}|{}|{}|{}", out.status.code().unwrap_or(-1), field("verdict"), field("kind"), field("at"), field("evidence"))
}

/// (c) 材料: 順の外の印を持つ 3 項目の行は材料の dir が 5 本（items.txt が増える）で、items.txt が見出し（3 個）・表の形の指示
/// （`1:<歯>,2:<歯>,3:<歯>`）・項目 3 行をこの順に持つ。番号を持たない done の行は items.txt を置かず、偽 lens が PASS と表 `1:-` を
/// 返しても PASS のまま（kind も at も無い）。
#[test]
fn pipe_review_done_items_material_lists_the_items_and_a_plain_done_places_none() {
    let (repo, state) = repo_with_state();
    let out = done_intake(&repo, &state, "s2-ic", ITEMS_DONE, Some(&says(&state, &table_pass("1:a,2:b,3:c"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let (first, dir) = (run_id_of(&out), review_dir(&state, &run_id_of(&out)));
    assert_eq!(dir_names(&dir), ["base.txt", "contract.toml", "design.txt", "items.txt", "requirements.txt"], "items.txt が 1 本増える");
    stop_run_ok(&state, &first);
    let text = fs::read_to_string(dir.join("items.txt")).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.first().is_some_and(|head| head.starts_with("## done の項目（3 個")), "見出しの行: {text}");
    assert!(lines.iter().any(|line| line.contains("1:<歯>,2:<歯>,3:<歯>")), "表の形の指示: {text}");
    let at = |want: &str| lines.iter().position(|line| *line == want);
    let found = [at("(1) 甲を作る"), at("(2) 乙を測る 形 (2) の字"), at("(3) 丙を足す")];
    assert!(found.iter().all(Option::is_some) && found.windows(2).all(|pair| pair.first() < pair.get(1)), "項目 3 行をこの順に: {text}");
    assert_eq!(lines.len(), found.last().copied().flatten().map_or(0, |last| last + 1), "項目の行が末尾: {text}");
    let plain = done_intake(&repo, &state, "s2-ip", "d-plain", Some(&says(&state, &table_pass("1:-"))));
    assert_eq!(judged(&state, &plain), "0|PASS|<無し>|<無し>|fake", "番号を持たない行は表を読まない: {}", stderr_of(&plain));
    assert_eq!(dir_names(&review_dir(&state, &run_id_of(&plain))), ["base.txt", "contract.toml", "design.txt", "requirements.txt"]);
    clean(&[&repo, &state]);
}

/// (d) PASS の倒し: 歯の無い項目（`-`）を持つ揃った表の PASS は FAIL・vacuous-assert（at は `-` の番号の `done(n)` の列・evidence は
/// 歯の無い項目と lens の evidence・rc 1・detail は倒した後の値）。空白と末尾の `,` を持つ揃った表の PASS は PASS のまま。
#[test]
fn pipe_review_done_items_pass_with_a_toothless_item_falls_to_vacuous_assert() {
    let (repo, state) = repo_with_state();
    let out = done_intake(&repo, &state, "s2-id1", ITEMS_DONE, Some(&says(&state, &table_pass("1:pipe_x_,2:-,3:-"))));
    assert_eq!(
        judged(&state, &out),
        "1|FAIL|vacuous-assert|done(2),done(3)|歯の無い done の項目 (2)(3)（lens の対応の表）: fake",
        "{}",
        stderr_of(&out)
    );
    assert_eq!(reviewed_detail(&state, &run_id_of(&out)), "verdict:FAIL kind:vacuous-assert");
    let clean_table = done_intake(&repo, &state, "s2-id2", ITEMS_DONE, Some(&says(&state, &table_pass("1:a, 2:b ,3:c,"))));
    assert_eq!(judged(&state, &clean_table), "0|PASS|<無し>|<無し>|fake", "空白と末尾の , を剥がして揃う: {}", stderr_of(&clean_table));
    assert_eq!(reviewed_detail(&state, &run_id_of(&clean_table)), "verdict:PASS");
    clean(&[&repo, &state]);
}

/// (e) FAIL と INCONCLUSIVE への足し: verdict と kind と rc は lens の値のまま、at の末尾に lens の at に無い `done(n)` だけを番号の順に
/// 足し（重ねない）、evidence の末尾に歯の無い項目の全部を足す。`-` の無い表は at も evidence も lens の値のまま。
#[test]
fn pipe_review_done_items_fail_and_inconclusive_gain_the_toothless_items_without_repeating() {
    let (repo, state) = repo_with_state();
    let fail = table_finding("FAIL", Some("literal-mismatch"), Some("§2,done(2)"), Some("\"1:-,2:-,3:t\""));
    let out = done_intake(&repo, &state, "s2-ie1", ITEMS_DONE, Some(&says(&state, &fail)));
    assert_eq!(judged(&state, &out), "1|FAIL|literal-mismatch|§2,done(2),done(1)|fake・歯の無い done の項目 (1)(2)", "{}", stderr_of(&out));
    assert_eq!(reviewed_detail(&state, &run_id_of(&out)), "verdict:FAIL kind:literal-mismatch");
    let open = table_finding("INCONCLUSIVE", Some("other"), None, Some("\"1:t,2:t,3:-\""));
    let out = done_intake(&repo, &state, "s2-ie2", ITEMS_DONE, Some(&says(&state, &open)));
    assert_eq!(judged(&state, &out), "3|INCONCLUSIVE|other|done(3)|fake・歯の無い done の項目 (3)", "{}", stderr_of(&out));
    let full = table_finding("FAIL", Some("literal-mismatch"), Some(LENS_AT), Some("\"1:a,2:b,3:c\""));
    let out = done_intake(&repo, &state, "s2-ie3", ITEMS_DONE, Some(&says(&state, &full)));
    assert_eq!(judged(&state, &out), format!("1|FAIL|literal-mismatch|{LENS_AT}|fake"), "`-` の無い表は不変: {}", stderr_of(&out));
    clean(&[&repo, &state]);
}

/// (f) 表の欠け: 揃わない 7 形の PASS は INCONCLUSIVE・unparsed・at 無しで evidence の頭に理由、欠けた FAIL は at を lens の値のまま
/// 残す。器が作る INCONCLUSIVE の 4 形（rc 7・JSON 無し・verdict が 3 値の外・`--lens` 無し）は表を読まない（rc 7 の周の stdout は `-` を
/// 持つ揃った表の PASS・MAYBE の周も `-` を持つ揃った表を持つ＝外しを飛ばす実装は rc 7 を FAIL に・MAYBE に歯の無い項目を足す）。
#[test]
fn pipe_review_done_items_missing_table_falls_to_unparsed_and_vessel_inconclusives_skip_the_table() {
    let (repo, state) = repo_with_state();
    for (index, (value, reason)) in [
        (None, "key done が無い"),
        (Some("\"1:a,3:b\""), "無い番号 (2)"),
        (Some("\"1:a,2:b,3:c,4:d\""), "余る番号 (4)"),
        (Some("\"1:a,1:b,2:c,3:d\""), "重なる番号 (1)"),
        (Some("\"1:a,2:,3:c\""), "無い番号 (2)・形の合わない項目 1 件"),
        (Some("\"1:a,x:b,2:c,3:d\""), "形の合わない項目 1 件"),
        (Some("7"), "key done が文字列でない"),
    ]
    .into_iter()
    .enumerate()
    {
        let line = table_finding("PASS", None, None, value);
        let out = done_intake(&repo, &state, &format!("s2-if{index}"), ITEMS_DONE, Some(&says(&state, &line)));
        let want = format!("3|INCONCLUSIVE|unparsed|<無し>|done の対応の表が欠ける（{reason}）: fake");
        assert_eq!(judged(&state, &out), want, "{reason}: {}", stderr_of(&out));
    }
    let lost = table_finding("FAIL", Some("literal-mismatch"), Some("§2"), None);
    let out = done_intake(&repo, &state, "s2-ifa", ITEMS_DONE, Some(&says(&state, &lost)));
    let want = "3|INCONCLUSIVE|unparsed|§2|done の対応の表が欠ける（key done が無い）: fake";
    assert_eq!(judged(&state, &out), want, "欠けた FAIL は at を残す: {}", stderr_of(&out));
    let row = table_pass("1:-,2:t,3:t");
    let cases = [
        ("s2-ifr", format!("cat >/dev/null; echo '{row}'; exit 7"), "lens が rc 7 で終わった"),
        ("s2-ifj", says(&state, "not-json"), "lens の出力に JSON 行が無い"),
        ("s2-ifm", says(&state, &table_finding("MAYBE", Some("other"), None, Some("\"1:-,2:t,3:t\""))), "lens の verdict が 3 値でない"),
    ];
    for (bead, lens, reason) in cases {
        let out = done_intake(&repo, &state, bead, ITEMS_DONE, Some(&lens));
        assert_eq!(judged(&state, &out), format!("3|INCONCLUSIVE|unparsed|<無し>|{reason}"), "{bead}: {}", stderr_of(&out));
    }
    let out = done_intake(&repo, &state, "s2-ifn", ITEMS_DONE, None);
    let text = judged(&state, &out);
    assert!(text.starts_with("3|INCONCLUSIVE|unparsed|<無し>|") && text.contains("--lens"), "{text}");
    assert!(!text.contains("done の対応の表") && !text.contains("歯の無い"), "--lens 無しは表を求めない: {text}");
    clean(&[&repo, &state]);
}

/// (b') 設計 pointer の契約は base の設計 doc からその行の `section` の節の本文を、要件面（`.html` の `id=`）から `req` の
/// 各 id の本文（tag 無し）を材料に写す。無い id はその旨を行に明示する（§4「順序」: 生成 (b) の前でも穴の出所は行の pointer）。
#[test]
fn pipe_review_reads_design_section_and_requirements_from_base() {
    // 契約 (b) 以後、行の `req` は表の検査が要件面と突き合わせる＝面に在る id だけを置く。
    let row = derive_row("a", &[("write-set", "[\"crates/toy/src/tint.rs\"]"), ("section", "\"2\""), ("req", "[\"FR2\"]")]);
    let doc = table_doc(&table_region(&[row])).replace("## 2. 型\n\n本文。", "## 2. 型\n\n節二の本文 SECTION-TWO-MARK。");
    let (repo, state) = derive_repo(&doc);
    let out = intake_raw(&repo, &state, "docs/design/toy.md#a", "s2-a");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let dir = review_dir(&state, &run_id_of(&out));
    let design = fs::read_to_string(dir.join("design.txt")).unwrap_or_default();
    assert!(design.starts_with("docs/design/toy.md#a §2\n"), "節の出所を名乗る: {design}");
    assert!(design.contains("SECTION-TWO-MARK"), "節 2 の本文: {design}");
    assert!(!design.contains("何を解くか") && !design.contains("[[contract]]"), "他の節と契約表は写さない: {design}");
    let requirements = fs::read_to_string(dir.join("requirements.txt")).unwrap_or_default();
    assert_eq!(requirements.trim_end(), "FR2: 2", "{requirements}");
    clean(&[&repo, &state]);
}

/// (b'-47) 導出物（`.toml`）の行を指す pointer の受付が通り、審査の材料 design.txt が出所の 1 行と行の goal（二重引用符と
/// backtick を含む単一行）を持ち、契約 file の goal が行の goal と等しい（設計 contract-source.md §47 の 6 / 7・同じ形の
/// `.md` の行は (b') の歯が不変で測る）。
#[test]
fn pipe_review_contract_whole_goal_reads_the_goal_from_a_derived_toml() {
    let goal = "節の \"本文\" GOAL-MARK と `crates/toy/src/tint.rs` の逐語";
    let quoted = format!("\"{goal}\"");
    let row = derive_row(
        "g",
        &[("write-set", "[\"crates/toy/src/tint.rs\"]"), ("section", "\"47\""), ("req", "[\"FR2\"]"), ("goal", &quoted)],
    );
    let derived = format!("schema = 1\n\n{row}");
    let (repo, state) = derive_repo_with(&table_doc(""), &[("docs/design/derived.toml", &derived)]);
    let out = intake_raw(&repo, &state, "docs/design/derived.toml#g", "s2-g");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "導出物の行の受付は通る: {}", stderr_of(&out));
    let id = run_id_of(&out);
    let design = fs::read_to_string(review_dir(&state, &id).join("design.txt")).unwrap_or_default();
    assert_eq!(design, format!("docs/design/derived.toml#g §47\n{goal}\n"), "出所の 1 行と goal");
    let contract = vessel::pipe::contract::Contract::load(&state.join("pipe").join(&id).join("contract.toml"))
        .unwrap_or_else(|errors| panic!("写しを読める: {errors:?}"));
    assert_eq!(contract.goal, goal, "契約 file の goal は行の goal");
    clean(&[&repo, &state]);
}

/// (b'') 要件面が `.yaml` の周は `- id: FR1` と同じ mapping の `text:` の値が材料に載る（設計 §4「yaml の `id` + `text`」・
/// `s2-07l.354`）。`title:` は本文にしない。base は `id="…"` の字面だけを探すので「要件面に無い」になる（RED）。
#[test]
fn pipe_review_reads_requirements_text_from_yaml() {
    let yaml = "requirements:\n  - id: FR1\n    title: 起動\n    text: 便を起こす YAML-TEXT-MARK\n  - id: FR2\n    text: 審査する\n";
    // 契約 (b) 以後、行の `req` は受付の表の検査が要件面と突き合わせる＝面に無い id は intake が断る。
    // ここは**面に在る id** で材料の描画を測る（面に無い id の断りは別の歯）。
    let (repo, state) = faced_repo_with_req("spec/reqs.yaml", yaml, &["FR1", "FR2"]);
    let requirements = reviewed_requirements(&repo, &state);
    assert_eq!(requirements, "FR1: 便を起こす YAML-TEXT-MARK\nFR2: 審査する", "{requirements}");
    assert!(!requirements.contains("起動"), "title は本文にしない: {requirements}");
    clean(&[&repo, &state]);
}

/// (c) 要件面が `.md` の周は `## FR1 …` の見出しの下の本文（次の見出しの直前まで・空白を畳んだ 1 行）が材料に載る。
/// 無い id は「要件面に無い」。base は `.md` を読めず「要件面に無い」になる（RED）。
#[test]
fn pipe_review_reads_requirements_text_from_md() {
    let md = "# 要件\n\n## FR1 便の起動\n\n便を\n起こす MD-TEXT-MARK。\n\n## FR2 審査\n\n審査する。\n";
    let (repo, state) = faced_repo_with_req("spec/reqs.md", md, &["FR1"]);
    let requirements = reviewed_requirements(&repo, &state);
    assert_eq!(requirements, "FR1: 便を 起こす MD-TEXT-MARK。", "{requirements}");
    assert!(!requirements.contains("審査する"), "次の見出しの下は写さない: {requirements}");
    clean(&[&repo, &state]);
}

/// (d) 裸の `- FR1` の yaml は id の検査は通るが本文が無い＝「（要件面 <path> の FR1 に本文が無い）」の理由が材料に
/// 載る（黙って空にしない・NFR4）。
#[test]
fn pipe_review_reads_requirements_reason_for_bare_yaml_id() {
    let (repo, state) = faced_repo_with_req("spec/reqs.yaml", "requirements:\n  - FR1\n  - FR2\n", &["FR1"]);
    let requirements = reviewed_requirements(&repo, &state);
    assert_eq!(requirements, "FR1: （要件面 spec/reqs.yaml の FR1 に本文が無い）", "{requirements}");
    clean(&[&repo, &state]);
}

/// (e) `## FR1` の直下が空行だけで次の見出しに続く md は (d) と同じ形の理由が載る（本文の無い id の理由は形を問わない）。
#[test]
fn pipe_review_reads_requirements_reason_for_empty_md_heading() {
    let (repo, state) =
        faced_repo_with_req("spec/reqs.md", "# 要件\n\n## FR1\n\n\n## FR2 審査\n\n審査する。\n", &["FR1", "FR2"]);
    let requirements = reviewed_requirements(&repo, &state);
    assert_eq!(requirements, "FR1: （要件面 spec/reqs.md の FR1 に本文が無い）\nFR2: 審査する。", "{requirements}");
    clean(&[&repo, &state]);
}

/// (c) 審査を飛ばす口は無い: `--no-review` は `intake` / `run` / `resume` のどれでも usage で断り（rc 1）、event も
/// run dir も作らない（AC22・C16）。
#[test]
fn pipe_review_has_no_skip_flag() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let rules = ceiling_rules(&state);
    let (contract, repo_text, state_text) = (path.clone(), repo.display().to_string(), state.display().to_string());
    let common: [&str; 12] = [
        "--design", &contract, "--bead", "s2-2e5", "--repo", &repo_text,
        "--state-dir", &state_text, "--rules", &rules, "--runner", TOY_COMMIT,
    ];
    for head in ["intake", "run", "resume"] {
        let mut args = vec![head, "--no-review"];
        args.extend(common.iter().copied());
        let out = run_pipe(&args);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{head} --no-review は rc 1: {}", stderr_of(&out));
        let err = stderr_of(&out);
        assert!(err.contains("--no-review"), "{head}: 断った引数を名指す: {err}");
        assert!(err.contains("usage: "), "{head}: usage を出す: {err}");
        assert!(out.stdout.is_empty(), "{head}: stdout 0 byte");
    }
    assert_eq!(event_count(&state), 0, "event を 1 件も書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&repo, &state]);
}

/// (d) `Stage::Reviewed` は `Intake` の直後（母集団 11 段・字面が往復する）・`Guard::Review` は `IntakeRefuse` の直後
/// （in-loop / fail-closed・境界は `pipe::review::ReviewCheck`）で、極性一覧の行に載る（C2 / C11.2 / C16.2）。
#[test]
fn pipe_review_stage_and_guard_are_pinned_in_declaration_order() {
    use vessel::fleet::STAGES;
    use vessel::polarity::{Guard, OnFailure, Timing, ALL};
    let at = |want: Stage| STAGES.iter().position(|stage| *stage == want);
    assert_eq!(STAGES.len(), 11, "段は 11 個: {STAGES:?}");
    assert_eq!(at(Stage::Reviewed), at(Stage::Intake).map(|found| found + 1), "Reviewed は Intake の直後");
    assert!(is_declaration_order(STAGES, |stage| stage as usize), "STAGES は宣言順");
    assert_eq!(Stage::Reviewed.as_str(), "Reviewed");
    assert_eq!(Stage::parse("Reviewed"), Some(Stage::Reviewed), "as_str ↔ parse の往復");
    let guard_at = |want: Guard| ALL.iter().position(|guard| *guard == want);
    assert_eq!(guard_at(Guard::Review), guard_at(Guard::IntakeRefuse).map(|found| found + 1), "Review は IntakeRefuse の直後");
    assert!(is_declaration_order(ALL, |guard| guard as usize), "ALL は宣言順");
    assert_eq!(Guard::Review.polarity().timing, Timing::InLoop, "spawn の前に止める");
    assert_eq!(Guard::Review.polarity().on_failure, OnFailure::FailClosed, "読めない判定は起こさない");
    assert_eq!(Guard::Review.boundary(), "pipe::review::ReviewCheck");
    assert_eq!(Guard::Review.line(), "guard=review-gate timing=in-loop on-failure=fail-closed boundary=pipe::review::ReviewCheck");
    let listed = bin_cmd().arg("polarity").output().expect("binary を起動できる");
    assert!(stdout_of(&listed).lines().any(|line| line == Guard::Review.line()), "極性一覧に載る: {}", stdout_of(&listed));
}

/// (e) `Intake` で止まった便（`RunCreated` の直後に process が落ちた形）の `resume` は**先に審査**し、PASS なら
/// 起こし・FAIL なら起こさない（同じ形で 2 便を対で測る）。
#[test]
fn pipe_review_resume_from_intake_reviews_before_spawning() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let seeded = intake(&repo, &state, &path);
    stop_run_ok(&state, &seeded);
    for (id, verdict, want_rc, spawned) in [("s2-pass-1", "PASS", RC_OK, true), ("s2-fail-1", "FAIL", RC_REFUSED, false)] {
        // 受付だけ済んだ便を組む: 写し面（契約・vessel・repo）は seed の便から写し、event は `RunCreated` 1 件。
        let dir = state.join("pipe").join(id);
        fs::create_dir_all(&dir).expect("run dir を作れる");
        for name in ["contract.toml", "vessel.toml", "repo"] {
            fs::copy(state.join("pipe").join(&seeded).join(name), dir.join(name)).expect("写しを置ける");
        }
        let out = bin_cmd()
            .args(["fleet", "record", "--kind", "RunCreated", "--stage", "Intake", "--run", id, "--bead", "s2-live", "--state-dir"])
            .arg(&state)
            .output()
            .expect("binary を起動できる");
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fleet record: {}", stderr_of(&out));
        assert!(show_line(&repo, &state, id).contains("stage=Intake"), "前提: 審査前の便");
        let marker = state.join(format!("runner-ran-{id}"));
        let out = run_pipe(&[
            "resume", "--run", id, "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
            "--rules", &ceiling_rules(&state), "--runner", &marker_runner(&marker),
            "--lens", &fake_lens(&state.join(format!("lens-{id}")), &lens_verdict(verdict)),
        ]);
        assert_eq!(out.status.code(), Some(i32::from(want_rc)), "{verdict}: {}", stderr_of(&out));
        assert_eq!(stage_count(&state, id, Stage::Reviewed), 1, "{verdict}: 審査の段が 1 件");
        assert_eq!(marker.exists(), spawned, "{verdict}: runner が起きたか");
        assert_eq!(stage_count(&state, id, Stage::Spawned), usize::from(spawned), "{verdict}: Spawned の件数");
        assert_eq!(value_of(&review_pairs(&state, id), "verdict"), verdict);
    }
    clean(&[&repo, &state]);
}

// ───── 事前審査の先撃ち（設計 dispatcher.md §27 形 aa・行 aa・`s2-07l.718`・接頭辞 `pipe_prelens_`） ─────
//
// 依存 A（行 a・held）を blocks で待つ clean な行 b / c に、起こす側の周（`pipe dispatch`）が lens を裏で先に撃つ。偽 lens は起動の
// たびに回数の file へ 1 行を足す。base には先撃ちが無い＝偽 lens は 1 回も起きず置き場も無い（RED）。

/// 先撃ちの toy repo の宣言（`cargo` を許す・要件面は reqs.md）。
const PRELENS_VESSEL: &str =
    "schema = 1\nallowed-commands = [\"git\", \"sh\", \"cargo\"]\ncommon-verify = [\"git rev-parse --verify {base}\"]\nrequirements = \"reqs.md\"\n";

/// 祖先 A の bead（行 a・held）。
const PRELENS_A: &str = "s2-pre.1";

/// 待ち行 B の bead（行 b）。
const PRELENS_B: &str = "s2-pre.2";

/// 待ち行 C の bead（行 c）。
const PRELENS_C: &str = "s2-pre.3";

/// 30 秒眠る偽 lens の本文（撃ち中を作る）。
const PRELENS_SLEEP: &str = "sleep 30";

/// 先撃ちの 1 つの置き場（toy repo・置き場・偽の台帳・列の manifest の写し）。
struct Prelens {
    /// toy repo。
    repo: PathBuf,
    /// 置き場。
    state: PathBuf,
    /// 偽の `bd`。
    bd: String,
    /// 列の manifest の写し。
    rules: String,
}

/// 行 `id` の欄（write-set を差し替え、`extra` の欄を足す）。
fn prelens_row(id: &str, write_set: &str, extra: &[&str]) -> Vec<String> {
    let set = format!("write-set = {write_set}");
    let mut add = vec![set.as_str()];
    add.extend(extra.iter().copied());
    row_fields(id, &["write-set"], &add)
}

/// 設計 doc と file を**そのまま** 1 回 commit した toy repo（行の素の項目を seed しない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn prelens_repo(doc: &str, files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    write_design(&repo, doc);
    for (path, body) in [(".vessel.toml", PRELENS_VESSEL)].iter().chain(files) {
        let target = repo.join(path);
        fs::create_dir_all(target.parent().expect("親 dir が在る")).expect("dir を作れる");
        fs::write(&target, body).expect("file を書ける");
    }
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "prelens-rows"]);
    (repo, state)
}

/// 台帳の 1 件（行 `row` を指す bead・blocks の依存の列）。
fn prelens_issue(id: &str, status: &str, row: &str, blocks: &[&str]) -> String {
    let deps: Vec<String> =
        blocks.iter().map(|on| format!("{{\"issue_id\":\"{id}\",\"depends_on_id\":\"{on}\",\"type\":\"blocks\"}}")).collect();
    format!(
        "{{\"id\":\"{id}\",\"status\":\"{status}\",\"priority\":2,\"labels\":[],\
         \"acceptance_criteria\":\"design = {DESIGN_FILE}#{row}\",\"dependencies\":[{}]}}",
        deps.join(",")
    )
}

/// 偽の `bd` の台帳を書き換える（script は 1 本・JSON だけを差し替える）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn prelens_ledger(state: &Path, issues: &[String]) -> String {
    use std::os::unix::fs::PermissionsExt;
    let json = state.join("bd-prelens.json");
    fs::write(&json, format!("[{}]\n", issues.join(","))).expect("偽の台帳を書ける");
    let path = state.join("bd-prelens");
    fs::write(&path, format!("#!/bin/sh\ncat '{}'\n", json.display())).expect("偽の bd を書ける");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("偽の bd に実行権を付ける");
    path.display().to_string()
}

/// 列の manifest の写し（受付の上限に台帳の待ち上限・終端の猶予・先撃ちの上限の行を足す・`None` は上限の行を持たない）。審査と
/// 先撃ちの lens の model の 2 行（`lens.model` / `pipe.precheck_lens_model`）は同じ値で持つ（使い回しの既存の歯の前提・§61 形 5）。
fn prelens_rules(state: &Path, limit: Option<u64>) -> String {
    prelens_rules_with(state, limit, (Some("opus"), Some("opus")))
}

/// [`prelens_rules`] の model の 2 行（審査の `lens.model`・先撃ちの `pipe.precheck_lens_model`・`None` は行を持たない）を選ぶ形。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn prelens_rules_with(state: &Path, limit: Option<u64>, models: (Option<&str>, Option<&str>)) -> String {
    let base = fs::read_to_string(ceiling_rules(state)).expect("受付の写しを読める");
    let row = |id: &str, kind: &str, value: u64| {
        format!("[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n")
    };
    let model = |id: &str, kind: &str, value: &str| {
        format!("[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = \"{value}\"\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n")
    };
    let mut rows = vec![
        row("seat.ledger_timeout_s", "LedgerTimeoutS", 60),
        row("pipe.stop_grace_ms", "StopGraceMs", embedded_int("pipe.stop_grace_ms")),
    ];
    rows.extend(limit.map(|value| row("pipe.precheck_lens_per_round", "PipePrecheckLensPerRound", value)));
    rows.extend(models.0.map(|value| model("lens.model", "LensModel", value)));
    rows.extend(models.1.map(|value| model("pipe.precheck_lens_model", "PipePrecheckLensModel", value)));
    let path = state.join("rules-prelens.toml");
    fs::write(&path, format!("{base}\n{}", rows.join("\n"))).expect("列の写しを書ける");
    path.display().to_string()
}

/// 行 a（`+src/fresh.rs`）と、a を blocks で待つ行 b（base の `src/b.rs`）/ c（base の `src/c.rs`）の置き場。`waiting` の bead
/// だけを台帳に載せ、A を hold する（依存を持たない A を列が起こさない）。
fn prelens_place(limit: Option<u64>, waiting: &[&str]) -> Prelens {
    let rows = [
        prelens_row("a", r#"["+src/fresh.rs"]"#, &[]),
        prelens_row("b", r#"["src/b.rs"]"#, &[]),
        prelens_row("c", r#"["src/c.rs"]"#, &[]),
    ];
    let files = [("src/b.rs", "pub fn b() {}\n"), ("src/c.rs", "pub fn c() {}\n")];
    let (repo, state) = prelens_repo(&design_doc_rows(&rows), &files);
    let mut issues = vec![prelens_issue(PRELENS_A, "open", "a", &[])];
    for (bead, row) in [(PRELENS_B, "b"), (PRELENS_C, "c")] {
        if waiting.contains(&bead) {
            issues.push(prelens_issue(bead, "open", row, &[PRELENS_A]));
        }
    }
    let bd = prelens_ledger(&state, &issues);
    prelens_hold(&state, &[PRELENS_A]);
    let rules = prelens_rules(&state, limit);
    Prelens { repo, state, bd, rules }
}

/// 介入 `hold` を打つ（`hold` は 1 周を撃たない）。
fn prelens_hold(state: &Path, beads: &[&str]) {
    for &bead in beads {
        let out = run_pipe(&["dispatch", "hold", bead, "--state-dir", &state.display().to_string()]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "hold は rc 0: {}", stderr_of(&out));
    }
}

/// 起こす側の手動の 1 周の argv（道具つき・runner は偽の 1 行・`lens` が `None` の周は `--lens` を持たない）。
fn prelens_args(place: &Prelens, lens: Option<&str>) -> Vec<String> {
    let mut args = [
        "dispatch", "--state-dir", &place.state.display().to_string(), "--repo", &place.repo.display().to_string(),
        "--rules", &place.rules, "--bd", &place.bd, "--runner", "true",
    ]
    .map(str::to_owned)
    .to_vec();
    args.extend(lens.into_iter().flat_map(|found| ["--lens".to_owned(), found.to_owned()]));
    args
}

/// 起こす側の手動の 1 周（rc 0 を要求する・`lens` が `None` の周は `--lens` 無し）。
fn prelens_round(place: &Prelens, lens: Option<&str>) -> Output {
    let args = prelens_args(place, lens);
    let out = run_pipe(&args.iter().map(String::as_str).collect::<Vec<&str>>());
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0: {} / {}", stdout_of(&out), stderr_of(&out));
    out
}

/// 起こす側の手動の 1 周（`--lens` つき）。
fn prelens_turn(place: &Prelens, lens: &str) -> Output {
    prelens_round(place, Some(lens))
}

/// `dispatch ls`（`--lens` を持たない）の `[DISPATCH-PRECHECK]` の行のうち `bead` の行。
fn prelens_line(place: &Prelens, bead: &str) -> String {
    let out = run_pipe(&[
        "dispatch", "ls", "--state-dir", &place.state.display().to_string(), "--repo", &place.repo.display().to_string(),
        "--rules", &place.rules, "--bd", &place.bd,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "ls は rc 0: {}", stderr_of(&out));
    let want = format!("[DISPATCH-PRECHECK] bead={bead} ");
    stdout_of(&out).lines().find(|line| line.starts_with(&want)).unwrap_or_default().to_owned()
}

/// `dispatch ls` の `[DISPATCH-BUNDLE]` の行。
fn prelens_bundles(place: &Prelens) -> Vec<String> {
    let out = run_pipe(&[
        "dispatch", "ls", "--state-dir", &place.state.display().to_string(), "--repo", &place.repo.display().to_string(),
        "--rules", &place.rules, "--bd", &place.bd,
    ]);
    stdout_of(&out).lines().filter(|line| line.starts_with("[DISPATCH-BUNDLE]")).map(str::to_owned).collect()
}

/// 起動のたびに回数の file へ 1 行を足し、`body` を撃つ偽 lens の 1 行。末尾の `:` は器が先撃ちの行の末尾に足す `--stage prelens`
/// （設計 pipeline.md §61 形 4）を引数ごと捨てる（`body` の最後の command の引数に化けない・[`fake_lens`] と同じ形）。
fn prelens_lens(state: &Path, body: &str) -> String {
    format!("printf 'x\\n' >> '{}'; {body}; :", state.join("prelens-count").display())
}

/// 偽 lens の起動の回数。
fn prelens_count(state: &Path) -> usize {
    fs::read_to_string(state.join("prelens-count")).map(|text| text.lines().count()).unwrap_or_default()
}

/// 回数が `want` に届くまで待ち（上限 20 秒）、さらに少し待ってから数える（遅れて来た起動を「起こしていない」と読み違えない）。
fn prelens_settle(state: &Path, want: usize) -> usize {
    let deadline = Instant::now() + Duration::from_secs(20);
    while prelens_count(state) < want && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(800));
    prelens_count(state)
}

/// bead の置き場（事前審査の dir の下の `lens/<bead>`）。
fn prelens_dir(state: &Path, bead: &str) -> PathBuf {
    state.join("pipe").join("precheck").join("lens").join(bead)
}

/// 置き場の `out` が在るまで待つ（上限 20 秒）。
fn prelens_out(state: &Path, bead: &str) -> bool {
    let out = prelens_dir(state, bead).join("out");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !out.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    out.exists()
}

/// 事前審査の結果の file の本文（無ければ空）。
fn prelens_result(state: &Path, bead: &str) -> String {
    fs::read_to_string(state.join("pipe").join("precheck").join(bead)).unwrap_or_default()
}

/// 結果の file の `result=` の値。
fn prelens_word(state: &Path, bead: &str) -> String {
    prelens_result(state, bead).lines().find_map(|line| line.strip_prefix("result=")).unwrap_or_default().to_owned()
}

/// 撃ち中の印の本文。
fn prelens_mark(state: &Path, bead: &str) -> String {
    fs::read_to_string(prelens_dir(state, bead).join("pid")).unwrap_or_default()
}

/// 印の pid（1 語目）。
fn prelens_pid(state: &Path, bead: &str) -> Option<u32> {
    prelens_mark(state, bead).split_whitespace().next()?.parse().ok()
}

/// 印の group を SIGKILL で殺し、pid が消えるまで待つ（上限 5 秒）。
fn prelens_kill(pid: u32) {
    Command::new("kill").args(["-KILL", "--", &format!("-{pid}")]).output().ok();
    let deadline = Instant::now() + Duration::from_secs(5);
    while proc_alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 置き場の印の group を全部殺して片付ける。
fn prelens_clean(place: &Prelens, pids: &[u32]) {
    for bead in [PRELENS_B, PRELENS_C] {
        if let Some(pid) = prelens_pid(&place.state, bead) {
            prelens_kill(pid);
        }
    }
    for &pid in pids {
        prelens_kill(pid);
    }
    clean(&[&place.repo, &place.state]);
}

/// (a) 形 aa 1 の上限: 上限 1 で clean の待ち行 2 本の 1 周目は 1 本だけ起こし、その lens が生きている間の 2 周目も残りを起こさず
/// （撃ち中を上限に数える）、印の group を殺した後の 3 周目に 1 本だけ起こす。値 0 の周は 1 本も起こさない。
#[test]
fn pipe_prelens_limit_counts_the_flying_lens_and_zero_fires_nothing() {
    let place = prelens_place(Some(1), &[PRELENS_B, PRELENS_C]);
    let lens = prelens_lens(&place.state, PRELENS_SLEEP);
    prelens_turn(&place, &lens);
    assert_eq!(prelens_settle(&place.state, 1), 1, "1 周目は 1 本だけ（{}）", prelens_result(&place.state, PRELENS_B));
    prelens_turn(&place, &lens);
    assert_eq!(prelens_settle(&place.state, 1), 1, "撃ち中の間の 2 周目は起こさない");
    let pid = prelens_pid(&place.state, PRELENS_B).unwrap_or_default();
    assert!(pid > 0 && proc_alive(pid), "列の先頭の行の印が生きている: {:?}", prelens_mark(&place.state, PRELENS_B));
    prelens_kill(pid);
    prelens_turn(&place, &lens);
    assert_eq!(prelens_settle(&place.state, 2), 2, "group を殺した後の 3 周目は 1 本だけ");
    prelens_clean(&place, &[pid]);
    let zero = prelens_place(Some(0), &[PRELENS_B, PRELENS_C]);
    prelens_turn(&zero, &prelens_lens(&zero.state, PRELENS_SLEEP));
    assert_eq!(prelens_settle(&zero.state, 0), 0, "値 0 の周は 1 本も起こさない");
    prelens_clean(&zero, &[]);
}

/// (b) 行の無い写しの周は 1 本も起こさず、`--lens` を持たない `dispatch ls` の `[DISPATCH-PRECHECK]` の行が ` prelens=unset` で終わる。
#[test]
fn pipe_prelens_without_the_row_fires_nothing_and_ls_says_unset() {
    let place = prelens_place(None, &[PRELENS_B]);
    prelens_turn(&place, &prelens_lens(&place.state, PRELENS_SLEEP));
    assert_eq!(prelens_settle(&place.state, 0), 0, "行の無い周は撃たない");
    assert_eq!(prelens_word(&place.state, PRELENS_B), "clean", "事前審査は clean: {}", prelens_result(&place.state, PRELENS_B));
    let line = prelens_line(&place, PRELENS_B);
    assert!(line.ends_with(" base=current prelens=unset"), "ls の行の末尾: {line}");
    prelens_clean(&place, &[]);
}

/// (c) 口座は選ばず起こす側の環境を継承し、席の pane の変数だけを外す。
#[test]
fn pipe_prelens_lens_inherits_the_env_but_not_the_pane() {
    let place = prelens_place(Some(1), &[PRELENS_B]);
    let seen = place.state.join("prelens-env");
    let body = format!("printf '%s %s\\n' \"${{TMUX_PANE-unset}}\" \"${{PRELENS_PROBE-unset}}\" > '{}'", seen.display());
    let lens = prelens_lens(&place.state, &body);
    let args = prelens_args(&place, Some(&lens));
    let out = pipe_cmd(&args.iter().map(String::as_str).collect::<Vec<&str>>())
        .env("TMUX_PANE", "%97")
        .env("PRELENS_PROBE", "inherited")
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0: {}", stderr_of(&out));
    assert_eq!(prelens_settle(&place.state, 1), 1, "1 本起こす");
    assert_eq!(fs::read_to_string(&seen).unwrap_or_default(), "unset inherited\n", "TMUX_PANE だけを外し他は継承する");
    prelens_clean(&place, &[]);
}

/// 偽 lens が受けた材料の dir を `seen` へ写す本文（その後に PASS を返す）。
fn prelens_copy(seen: &Path) -> String {
    format!("cp -r \"$(dirname '{{contract}}')\" '{}'; echo '{}'", seen.display(), lens_verdict("PASS"))
}

/// (d) 実体化（宣言）: 宣言だけの祖先 A の `+` の F を素で持ち、設計の節が F を backtick で名指す待ち行 B の先撃ちで、偽 lens が受けた
/// 設計の材料の末尾に予想の base の 1 行と F の path が在り、base の要約の F が `行数 全体 0`、外の材料の depends の相手の行の F が
/// base に在り（空の F が tracked）、一時の worktree は置き場にも `git worktree list` にも残らない。
#[test]
fn pipe_prelens_declared_ancestor_places_empty_files_in_a_temporary_tree() {
    let rows = [prelens_row("a", r#"["+src/fresh.rs"]"#, &[]), prelens_row("b", r#"["src/fresh.rs"]"#, &[r#"depends = ["a"]"#])];
    let doc = design_doc_rows(&rows).replace("節の本文。", "節の本文。`src/fresh.rs` を読む。");
    let (repo, state) = prelens_repo(&doc, &[]);
    let bd = prelens_ledger(&state, &[prelens_issue(PRELENS_A, "open", "a", &[]), prelens_issue(PRELENS_B, "open", "b", &[PRELENS_A])]);
    prelens_hold(&state, &[PRELENS_A]);
    let place = Prelens { rules: prelens_rules(&state, Some(1)), repo, state, bd };
    let seen = place.state.join("prelens-seen");
    prelens_turn(&place, &prelens_lens(&place.state, &prelens_copy(&seen)));
    assert_eq!(prelens_word(&place.state, PRELENS_B), "clean", "{}", prelens_result(&place.state, PRELENS_B));
    assert!(prelens_out(&place.state, PRELENS_B), "偽 lens が終わる");
    let design = fs::read_to_string(seen.join("design.txt")).unwrap_or_default();
    let note = "予想の base: 次の file は未着地の祖先の宣言で、本文を空で置いた\n- src/fresh.rs\n";
    assert!(design.ends_with(note), "設計の材料の末尾に予想の 1 行と F: {design}");
    let base = fs::read_to_string(seen.join("base.txt")).unwrap_or_default();
    assert!(base.contains("- src/fresh.rs: 行数 全体 0 / "), "空の F: {base}");
    let outside = fs::read_to_string(seen.join("outside.txt")).unwrap_or_default();
    assert!(outside.contains("  +src/fresh.rs: base に在る"), "空の F が tracked: {outside}");
    assert!(!prelens_dir(&place.state, PRELENS_B).join("tree").exists(), "worktree は置き場に残らない");
    assert_eq!(git(&place.repo, &["worktree", "list"]).lines().count(), 1, "git worktree list は anchor だけ");
    prelens_clean(&place, &[]);
}

/// (e) 実体化（実物の木）: 祖先 A が Gated PASS で A の木が `.md` の G に 2 行を書いた周に、G を素で持つ待ち行 B の先撃ちの base の
/// 要約の G が `行数 全体 2` で、設計の材料に予想の印が無い。
#[test]
fn pipe_prelens_gated_ancestor_copies_the_tree_file_whatever_its_extension() {
    let rows = [prelens_row("a", r#"["+docs/g.md"]"#, &[]), prelens_row("b", r#"["docs/g.md"]"#, &[])];
    let (repo, state) = prelens_repo(&design_doc_rows(&rows), &[]);
    let bd = prelens_ledger(&state, &[prelens_issue(PRELENS_A, "open", "a", &[]), prelens_issue(PRELENS_B, "open", "b", &[PRELENS_A])]);
    prelens_hold(&state, &[PRELENS_A]);
    let id = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), PRELENS_A);
    let runner = "printf 'one\\ntwo\\n' > docs/g.md && git add -A && git commit -q -m runner";
    let spawned = spawn_with(&repo, &state, &id, runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&spawned));
    let gated = gate_once(&repo, &state, &id, Some(&fake_lens(&state.join("prelens-gate-lens"), &lens_verdict("PASS"))));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate は PASS: {}", stderr_of(&gated));
    fs::write(state.join("pipe").join(&id).join("driver"), "not-a-pid\n").expect("札を書ける");
    let place = Prelens { rules: prelens_rules(&state, Some(1)), repo, state, bd };
    let seen = place.state.join("prelens-seen");
    prelens_turn(&place, &prelens_lens(&place.state, &prelens_copy(&seen)));
    assert_eq!(prelens_word(&place.state, PRELENS_B), "clean", "{}", prelens_result(&place.state, PRELENS_B));
    assert!(prelens_out(&place.state, PRELENS_B), "偽 lens が終わる");
    let base = fs::read_to_string(seen.join("base.txt")).unwrap_or_default();
    assert!(base.contains("- docs/g.md: 行数 全体 2 / "), "A の木の G の 2 行: {base}");
    let design = fs::read_to_string(seen.join("design.txt")).unwrap_or_default();
    assert!(!design.is_empty() && !design.contains("予想の base"), "実物の木に予想の印は無い: {design}");
    prelens_clean(&place, &[]);
}

/// (f) 組めない周: 置き場の `tree` の path に file が在って worktree を作れない周は撃たず、ls の行が ` prelens=unbuilt` で終わる。
/// その file を外した次の周は撃ち、` prelens=unbuilt` は消える。
#[test]
fn pipe_prelens_unbuilt_tree_is_named_and_refired_after_it_clears() {
    let place = prelens_place(Some(1), &[PRELENS_B]);
    let blocker = prelens_dir(&place.state, PRELENS_B).join("tree");
    fs::create_dir_all(prelens_dir(&place.state, PRELENS_B)).expect("置き場を作れる");
    fs::write(&blocker, "not a tree\n").expect("tree の path に file を置ける");
    let lens = prelens_lens(&place.state, &format!("echo '{}'", lens_verdict("PASS")));
    prelens_turn(&place, &lens);
    assert_eq!(prelens_settle(&place.state, 0), 0, "組めない周は撃たない");
    let line = prelens_line(&place, PRELENS_B);
    assert!(line.ends_with(" prelens=unbuilt"), "ls の行の末尾: {line}");
    fs::remove_file(&blocker).expect("file を外せる");
    prelens_turn(&place, &lens);
    assert_eq!(prelens_settle(&place.state, 1), 1, "組めた周に撃つ");
    let line = prelens_line(&place, PRELENS_B);
    assert!(line.ends_with(" base=current"), "unbuilt の語は消える: {line}");
    prelens_clean(&place, &[]);
}

/// (g) 起こした周は待たない: 30 秒眠る偽 lens を起こした周が返った時に、置き場に `out` が無く、印の本文が 2 語で、印の pid が生きている。
#[test]
fn pipe_prelens_fired_round_returns_without_waiting() {
    let place = prelens_place(Some(1), &[PRELENS_B]);
    let started = Instant::now();
    prelens_turn(&place, &prelens_lens(&place.state, PRELENS_SLEEP));
    assert!(started.elapsed() < Duration::from_secs(25), "周は lens の終わりを待たない: {:?}", started.elapsed());
    assert!(!prelens_dir(&place.state, PRELENS_B).join("out").exists(), "out は無い");
    let mark = prelens_mark(&place.state, PRELENS_B);
    let words: Vec<&str> = mark.split_whitespace().collect();
    assert!(words.len() == 2 && words.iter().all(|word| word.bytes().all(|byte| byte.is_ascii_digit())), "印は 2 語: {mark:?}");
    let pid = prelens_pid(&place.state, PRELENS_B).unwrap_or_default();
    assert!(proc_alive(pid), "印の pid が生きている: {mark:?}");
    prelens_clean(&place, &[]);
}

/// (h) 印の弁別: 2 語目を別の数に書き換えた印（pid は生きている）は死んだと判じて撃ち直し、数でない字に壊した印（読めない）は
/// 撃ち中に数えて撃ち直さない。
#[test]
fn pipe_prelens_mark_with_a_foreign_start_refires_but_an_unreadable_one_does_not() {
    let place = prelens_place(Some(1), &[PRELENS_B]);
    let lens = prelens_lens(&place.state, PRELENS_SLEEP);
    prelens_turn(&place, &lens);
    assert_eq!(prelens_settle(&place.state, 1), 1, "1 周目に 1 本");
    let first = prelens_pid(&place.state, PRELENS_B).unwrap_or_default();
    let mark = prelens_dir(&place.state, PRELENS_B).join("pid");
    fs::write(&mark, format!("{first} 1\n")).expect("印を書き換えられる");
    prelens_turn(&place, &lens);
    assert_eq!(prelens_settle(&place.state, 2), 2, "起動時刻の違う印は死んだと判じて撃ち直す");
    let second = prelens_pid(&place.state, PRELENS_B).unwrap_or_default();
    fs::write(&mark, "not-a-pid\n").expect("印を壊せる");
    prelens_turn(&place, &lens);
    assert_eq!(prelens_settle(&place.state, 2), 2, "読めない印は撃ち中に数えて撃ち直さない");
    prelens_clean(&place, &[first, second]);
}

/// 1 行 `bead` に `judgement` を返す偽 lens を撃ち、終わった後の次の周までを回す（上限 1・待ち行 B だけ）。
fn prelens_judged(judgement: &str) -> Prelens {
    let place = prelens_place(Some(1), &[PRELENS_B]);
    let lens = prelens_lens(&place.state, judgement);
    prelens_turn(&place, &lens);
    assert!(prelens_out(&place.state, PRELENS_B), "偽 lens が終わる");
    prelens_turn(&place, &lens);
    place
}

/// (i) 形 aa 4 の確定: FAIL（vacuous-assert）と INCONCLUSIVE（section-material-missing）の次の周に事前審査の結果が
/// `firm:1,provisional:0` で、束の行の根が理由の型、finding の在り処が `prelens`。rc 1 で終わる偽 lens の周は clean のまま。
#[test]
fn pipe_prelens_fail_and_inconclusive_become_firm_findings_but_rc_one_does_not() {
    for (verdict, kind) in [("FAIL", "vacuous-assert"), ("INCONCLUSIVE", "section-material-missing")] {
        let place = prelens_judged(&format!("echo '{}'", lens_finding(verdict, Some(kind), None)));
        let result = prelens_result(&place.state, PRELENS_B);
        assert_eq!(prelens_word(&place.state, PRELENS_B), "firm:1,provisional:0", "{verdict}: {result}");
        let line = prelens_line(&place, PRELENS_B);
        assert!(line.contains(" result=firm:1,provisional:0 "), "{verdict}: ls の行: {line}");
        let bundles = prelens_bundles(&place);
        assert!(bundles.len() == 1 && bundles.iter().all(|found| found.contains(&format!(" root={kind} "))), "{verdict}: {bundles:?}");
        let finding = result.lines().find(|found| found.starts_with("finding=")).unwrap_or_default();
        assert!(finding.starts_with(&format!("finding=firm name={kind} at=prelens new=true ")), "{verdict}: {result}");
        prelens_clean(&place, &[]);
    }
    let place = prelens_judged("exit 1");
    assert_eq!(prelens_word(&place.state, PRELENS_B), "clean", "rc 1 は写さない: {}", prelens_result(&place.state, PRELENS_B));
    prelens_clean(&place, &[]);
}

/// (j)(k) の置き場: 上限 1 で、待ち行 B は FAIL の判定を写し終え、待ち行 C は 30 秒眠る偽 lens が撃ち中（2 周を回した後）。
fn prelens_crossing() -> (Prelens, String) {
    let place = prelens_place(Some(1), &[PRELENS_B, PRELENS_C]);
    let fail = lens_finding("FAIL", Some("vacuous-assert"), None);
    let lens = prelens_lens(&place.state, &format!("case '{{contract}}' in *{PRELENS_C}*) {PRELENS_SLEEP};; esac; echo '{fail}'"));
    prelens_turn(&place, &lens);
    assert!(prelens_out(&place.state, PRELENS_B), "B の偽 lens が終わる");
    prelens_turn(&place, &lens);
    assert_eq!(prelens_settle(&place.state, 2), 2, "2 周目に C を起こす");
    assert_eq!(prelens_word(&place.state, PRELENS_B), "firm:1,provisional:0", "{}", prelens_result(&place.state, PRELENS_B));
    assert!(proc_alive(prelens_pid(&place.state, PRELENS_C).unwrap_or_default()), "C は撃ち中");
    (place, lens)
}

/// 材料に入らない file `name` を main に 1 つ commit する（事前審査の鍵の HEAD が動く）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn prelens_unrelated(repo: &Path, name: &str) {
    fs::write(repo.join(name), "x\n").expect("file を書ける");
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", name]);
}

/// (j) 周またぎ（材料が同じ）: 別の行の lens が撃ち中の周に材料に入らない file を main に commit した次の周も、B は
/// `firm:1,provisional:0` のままで finding は `new=false`、`[DISPATCH-BUNDLE]` の行は commit の前と同じ。`--lens` 無しで撃った周と
/// 値 0 の写しで撃った周も同じ（撃たない周も写し直す）。
#[test]
fn pipe_prelens_same_material_keeps_the_finding_across_a_main_move() {
    let (place, lens) = prelens_crossing();
    let before = prelens_bundles(&place);
    let rounds: [(&str, &dyn Fn()); 3] = [
        ("--lens の周", &|| {
            prelens_turn(&place, &lens);
        }),
        ("--lens 無しの周", &|| {
            prelens_round(&place, None);
        }),
        ("値 0 の周", &|| {
            prelens_rules(&place.state, Some(0));
            prelens_turn(&place, &lens);
        }),
    ];
    for (index, (label, round)) in rounds.iter().enumerate() {
        prelens_unrelated(&place.repo, &format!("unrelated-{index}.txt"));
        round();
        let result = prelens_result(&place.state, PRELENS_B);
        assert_eq!(prelens_word(&place.state, PRELENS_B), "firm:1,provisional:0", "{label}: {result}");
        assert!(prelens_line(&place, PRELENS_B).contains(" base=current"), "{label}: 事前審査は main の動きで書き直された");
        let finding = result.lines().find(|found| found.starts_with("finding=")).unwrap_or_default();
        assert!(finding.contains(" at=prelens new=false "), "{label}: 前の結果に在った finding は new=false: {result}");
        assert_eq!(prelens_bundles(&place), before, "{label}: 束の行は変わらない");
    }
    prelens_clean(&place, &[]);
}

/// 材料の dir の file の (名, 本文) の列（名の順）。
fn prelens_material(state: &Path, bead: &str) -> Vec<(String, String)> {
    let dir = prelens_dir(state, bead).join("review");
    let mut found: Vec<(String, String)> = fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| (entry.file_name().to_string_lossy().into_owned(), fs::read_to_string(entry.path()).unwrap_or_default()))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

/// (k) 周またぎ（材料が変わる）: B の write-set の素の path の file の本文を main で変えた次の周に、B の結果は clean で置き場に
/// `out` と `fired` が無く、撃ち中の C の材料の dir は変わらない。
#[test]
fn pipe_prelens_changed_material_drops_the_verdict_and_keeps_the_flying_material() {
    let (place, lens) = prelens_crossing();
    let flying = prelens_material(&place.state, PRELENS_C);
    assert!(!flying.is_empty(), "C の材料が在る");
    fs::write(place.repo.join("src").join("b.rs"), "pub fn b() {}\n\npub fn b2() {}\n").expect("file を書ける");
    git(&place.repo, &["add", "-A"]);
    git(&place.repo, &["commit", "-q", "-m", "b-changed"]);
    prelens_turn(&place, &lens);
    assert_eq!(prelens_word(&place.state, PRELENS_B), "clean", "{}", prelens_result(&place.state, PRELENS_B));
    let dir = prelens_dir(&place.state, PRELENS_B);
    assert!(!dir.join("out").exists() && !dir.join("fired").exists(), "別の材料の判定を写さない");
    assert_eq!(prelens_material(&place.state, PRELENS_C), flying, "撃ち中の行の材料は組み直さない");
    prelens_clean(&place, &[]);
}

/// (l) 置き場の寿命: 依存を閉じて依存待ちを抜けた B の置き場は残り（母集団に居る）、B の bead を閉じた周の頭に消える。
#[test]
fn pipe_prelens_place_lives_while_the_row_is_open() {
    let place = prelens_place(Some(1), &[PRELENS_B]);
    prelens_hold(&place.state, &[PRELENS_B]);
    let lens = prelens_lens(&place.state, &format!("echo '{}'", lens_verdict("PASS")));
    prelens_turn(&place, &lens);
    assert!(prelens_out(&place.state, PRELENS_B), "偽 lens が終わる");
    let a_closed = prelens_issue(PRELENS_A, "closed", "a", &[]);
    prelens_ledger(&place.state, &[a_closed.clone(), prelens_issue(PRELENS_B, "open", "b", &[PRELENS_A])]);
    prelens_turn(&place, &lens);
    assert!(prelens_dir(&place.state, PRELENS_B).is_dir(), "依存待ちを抜けても母集団に居る間は残る");
    prelens_ledger(&place.state, &[a_closed, prelens_issue(PRELENS_B, "closed", "b", &[PRELENS_A])]);
    prelens_turn(&place, &lens);
    assert!(!prelens_dir(&place.state, PRELENS_B).exists(), "閉じた周の頭に外れる");
    prelens_clean(&place, &[]);
}

/// (m) 測れなかった判定の撃ち直し: JSON の行の無い stdout（`unparsed`）を返した先撃ちの後、main を動かさない周は撃ち直さず結果は
/// clean のまま。材料に入らない file を main に commit した後の 2 周のうちに 1 度だけ撃ち直し、main を動かさない次の周も撃ち直さない。
#[test]
fn pipe_prelens_unparsed_verdict_refires_once_after_main_moves() {
    let place = prelens_place(Some(1), &[PRELENS_B]);
    let lens = prelens_lens(&place.state, "echo not-json");
    prelens_turn(&place, &lens);
    assert!(prelens_out(&place.state, PRELENS_B), "偽 lens が終わる");
    prelens_turn(&place, &lens);
    assert_eq!(prelens_settle(&place.state, 1), 1, "main の動かない周は撃ち直さない");
    assert_eq!(prelens_word(&place.state, PRELENS_B), "clean", "unparsed は写さない: {}", prelens_result(&place.state, PRELENS_B));
    prelens_unrelated(&place.repo, "unrelated.txt");
    for _ in 0..2 {
        prelens_turn(&place, &lens);
        prelens_settle(&place.state, 2);
    }
    assert_eq!(prelens_count(&place.state), 2, "main が動いた後の 2 周のうちに撃ち直す");
    assert!(prelens_out(&place.state, PRELENS_B), "撃ち直した偽 lens が終わる");
    prelens_turn(&place, &lens);
    assert_eq!(prelens_settle(&place.state, 2), 2, "main の動かない次の周は撃ち直さない");
    assert_eq!(prelens_word(&place.state, PRELENS_B), "clean", "{}", prelens_result(&place.state, PRELENS_B));
    prelens_clean(&place, &[]);
}

/// (n) 残りの片付け: 置き場の `tree` に一時の worktree を残した置き場と、`tree` の dir を消して登録だけを残した置き場のそれぞれの
/// 周は ` prelens=unbuilt` で終わらずに偽 lens を 1 回起こし、周の後の `git worktree list --porcelain` がその path を持たない。
#[test]
fn pipe_prelens_leftover_tree_and_registration_are_removed_at_the_round_head() {
    for (label, registered_only) in [("worktree の残り", false), ("登録だけの残り", true)] {
        let place = prelens_place(Some(1), &[PRELENS_B]);
        let tree = prelens_dir(&place.state, PRELENS_B).join("tree");
        fs::create_dir_all(prelens_dir(&place.state, PRELENS_B)).expect("置き場を作れる");
        git(&place.repo, &["worktree", "add", "-q", "--detach", &tree.display().to_string(), "HEAD"]);
        if registered_only {
            fs::remove_dir_all(&tree).expect("tree の dir を消せる");
        }
        let suffix = format!("precheck/lens/{PRELENS_B}/tree");
        assert!(git(&place.repo, &["worktree", "list", "--porcelain"]).contains(&suffix), "{label}: 周の前は登録が在る");
        prelens_turn(&place, &prelens_lens(&place.state, &format!("echo '{}'", lens_verdict("PASS"))));
        assert_eq!(prelens_settle(&place.state, 1), 1, "{label}: 残りを外して撃つ");
        let line = prelens_line(&place, PRELENS_B);
        assert!(!line.ends_with(" prelens=unbuilt"), "{label}: ls の行: {line}");
        let list = git(&place.repo, &["worktree", "list", "--porcelain"]);
        assert!(!list.contains(&suffix), "{label}: 周の後に残りは無い: {list}");
        prelens_clean(&place, &[]);
    }
}

// ───── Reviewed の段の使い回し（設計 dispatcher.md §27 形 ac・行 ac・接頭辞 `pipe_review_reuse_`） ─────
//
// 祖先 A（行 a・`+docs/g.md`）を待つ行 b（`docs/g.md` を素で持つ）へ先撃ちし、A を着地させて起こす側の周を 1 回撃った後に B の審査
// （`pipe intake` の Reviewed）を撃つ。偽 lens は先撃ちと審査で同じ回数の file に 1 行を足す。base には使い回しが無い＝審査は必ず
// 偽 lens を撃つ（(a) が RED）。

/// 6 値を運ぶ PASS の判定の行（lens を撃った審査は消費の event を 1 件書く）。
fn reuse_pass() -> String {
    let usage = r#""usage":"in:7,out:8,cache_read:9,cache_create:10","turns":2,"wall_ms":300"#;
    format!("{},{usage}}}", lens_verdict("PASS").trim_end_matches('}'))
}

/// 行 a（`+docs/g.md`）と a を blocks で待つ行 b（`docs/g.md`）の置き場（A と B を hold する＝列は起こさない）。
fn reuse_place() -> Prelens {
    reuse_place_with(None)
}

/// [`reuse_place`] の行 b の done を `done_b` にした置き場（`None` は既定の done）。
fn reuse_place_with(done_b: Option<&str>) -> Prelens {
    let row_b = match done_b {
        Some(text) => row_fields("b", &["write-set", "done"], &[r#"write-set = ["docs/g.md"]"#, &format!("done = \"{text}\"")]),
        None => prelens_row("b", r#"["docs/g.md"]"#, &[]),
    };
    let rows = [prelens_row("a", r#"["+docs/g.md"]"#, &[]), row_b];
    let (repo, state) = prelens_repo(&design_doc_rows(&rows), &[]);
    let bd = prelens_ledger(&state, &[prelens_issue(PRELENS_A, "open", "a", &[]), prelens_issue(PRELENS_B, "open", "b", &[PRELENS_A])]);
    prelens_hold(&state, &[PRELENS_A, PRELENS_B]);
    Prelens { rules: prelens_rules(&state, Some(1)), repo, state, bd }
}

/// 先撃ちを 1 回撃ち、終わるまで待つ（偽 lens の回数 1）。
fn reuse_fire(place: &Prelens, lens: &str) {
    prelens_turn(place, lens);
    assert!(prelens_out(&place.state, PRELENS_B), "先撃ちの偽 lens が終わる");
    assert_eq!(prelens_settle(&place.state, 1), 1, "先撃ちは 1 回: {}", prelens_result(&place.state, PRELENS_B));
}

/// A を Gated PASS にし（A の木が G に 2 行を書く）、B へ `body` を撃つ偽 lens で先撃ちする。(置き場, A の便, 偽 lens) を返す。
fn reuse_gated(body: &str) -> (Prelens, String, String) {
    reuse_gated_with(body, None)
}

/// [`reuse_gated`] の行 b の done を `done_b` にした形。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn reuse_gated_with(body: &str, done_b: Option<&str>) -> (Prelens, String, String) {
    let place = reuse_place_with(done_b);
    let id = intake_bead(&place.repo, &place.state, &format!("{DESIGN_FILE}#a"), PRELENS_A);
    let runner = "printf 'one\\ntwo\\n' > docs/g.md && git add -A && git commit -q -m runner";
    let spawned = spawn_with(&place.repo, &place.state, &id, runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&spawned));
    let gate_lens = fake_lens(&place.state.join("reuse-gate-lens"), &lens_verdict("PASS"));
    let gated = gate_once(&place.repo, &place.state, &id, Some(&gate_lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate は PASS: {}", stderr_of(&gated));
    fs::write(place.state.join("pipe").join(&id).join("driver"), "not-a-pid\n").expect("札を書ける");
    let lens = prelens_lens(&place.state, body);
    reuse_fire(&place, &lens);
    (place, id, lens)
}

/// A を着地させる: `body` が `None` なら A の木の HEAD へ main を早送りし、`Some` なら G をその本文で main に commit する。A の便を
/// 外し（`run` が在れば）、台帳の A を閉じ、起こす側の周を 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn reuse_land(place: &Prelens, run: Option<&str>, body: Option<&str>, lens: &str) {
    match (run, body) {
        (Some(id), None) => {
            let head = git(&worktree_of(&place.repo, id), &["rev-parse", "HEAD"]);
            git(&place.repo, &["merge", "-q", "--ff-only", head.trim()]);
        }
        (_, text) => {
            fs::write(place.repo.join("docs").join("g.md"), text.unwrap_or_default()).expect("G を書ける");
            git(&place.repo, &["add", "-A"]);
            git(&place.repo, &["commit", "-q", "-m", "landed"]);
        }
    }
    if let Some(id) = run {
        stop_run_ok(&place.state, id);
    }
    prelens_ledger(&place.state, &[prelens_issue(PRELENS_A, "closed", "a", &[]), prelens_issue(PRELENS_B, "open", "b", &[PRELENS_A])]);
    prelens_turn(place, lens);
}

/// B の審査を `lens` で 1 回撃ち、(Reviewed の detail, 審査の消費の event の増分) を返す。
fn reuse_review(place: &Prelens, lens: &str) -> (String, usize) {
    let costs = || events(&place.state).iter().filter(|event| event.kind == EventKind::RunCost).count();
    let before = costs();
    let out = run_pipe(&[
        "intake", "--design", &format!("{DESIGN_FILE}#b"), "--bead", PRELENS_B, "--repo", &place.repo.display().to_string(),
        "--state-dir", &place.state.display().to_string(), "--rules", &place.rules, "--lens", lens,
    ]);
    let id = run_id_of(&out);
    assert!(!id.is_empty(), "審査まで届く: {} / {}", stdout_of(&out), stderr_of(&out));
    (reviewed_detail(&place.state, &id), costs().saturating_sub(before))
}

/// (a) A が Gated の木のまま着地し、起こす側の周を 1 回撃った後の B の審査は、材料の鍵・判定・lens の字が先撃ちと同じなので偽 lens を
/// 撃たず（回数 1 のまま）判定を写し、detail が ` prelens:reused` で終わり、審査の消費の event を書かない。
#[test]
fn pipe_review_reuse_same_material_and_lens_copies_the_verdict_without_firing() {
    let (place, id, lens) = reuse_gated(&format!("echo '{}'", reuse_pass()));
    reuse_land(&place, Some(&id), None, &lens);
    let (detail, costs) = reuse_review(&place, &lens);
    assert_eq!(prelens_count(&place.state), 1, "審査は偽 lens を撃たない: {detail}");
    assert_eq!(detail, "verdict:PASS prelens:reused", "先撃ちの判定を写し末尾に語");
    assert_eq!(costs, 0, "使い回した審査は消費の event を書かない");
    prelens_clean(&place, &[]);
}

/// (g) 行 b の done が `(1) 甲 (2) 乙` で、先撃ちの偽 lens が PASS と表 `1:-,2:t` を返した周の審査は、偽 lens を撃たず（回数 1）使い回した
/// 判定を倒しの 1 本に通し、detail が `verdict:FAIL kind:vacuous-assert prelens:reused`・review.json の at が `done(1)`。
#[test]
fn pipe_review_done_items_reused_prelens_verdict_goes_through_the_same_fall() {
    let (place, id, lens) = reuse_gated_with(&format!("echo '{}'", table_pass("1:-,2:t")), Some("(1) 甲 (2) 乙"));
    reuse_land(&place, Some(&id), None, &lens);
    let (detail, _) = reuse_review(&place, &lens);
    assert_eq!(prelens_count(&place.state), 1, "審査は偽 lens を撃たない: {detail}");
    assert_eq!(detail, "verdict:FAIL kind:vacuous-assert prelens:reused");
    let reviewed = run_dirs(&place.state).into_iter().find(|run| *run != id && review_has(&place.state, run, "verdict")).unwrap_or_default();
    assert_eq!(value_of(&review_pairs(&place.state, &reviewed), "at"), "done(1)", "倒しの at");
    prelens_clean(&place, &[]);
}

/// (b) G が Gated の木（2 行）と違う 3 行で着地した周の審査は材料の鍵が外れて偽 lens を撃ち（回数 2）、detail に語が無く、撃った
/// 審査の消費の event を 1 件書く（(a) の 0 件の対）。
#[test]
fn pipe_review_reuse_landed_body_unlike_the_gated_tree_fires_the_lens() {
    let (place, id, lens) = reuse_gated(&format!("echo '{}'", reuse_pass()));
    stop_run_ok(&place.state, &id);
    reuse_land(&place, None, Some("one\ntwo\nthree\n"), &lens);
    let (detail, costs) = reuse_review(&place, &lens);
    assert_eq!(prelens_count(&place.state), 2, "審査は偽 lens を撃つ: {detail}");
    assert_eq!(detail, "verdict:PASS", "語は無い");
    assert_eq!(costs, 1, "撃った審査は消費の event を 1 件書く");
    prelens_clean(&place, &[]);
}

/// (c) 宣言だけの祖先を持つ行（予想の印を持つ）は、祖先が宣言どおり空の G で着地した後の審査でも偽 lens を撃つ。
#[test]
fn pipe_review_reuse_forecast_mark_never_matches_the_landed_base() {
    let place = reuse_place();
    let lens = prelens_lens(&place.state, &format!("echo '{}'", reuse_pass()));
    reuse_fire(&place, &lens);
    reuse_land(&place, None, Some(""), &lens);
    let (detail, _) = reuse_review(&place, &lens);
    assert_eq!(prelens_count(&place.state), 2, "予想の印を持つ材料は使い回さない: {detail}");
    assert!(!detail.contains("prelens:reused"), "{detail}");
    prelens_clean(&place, &[]);
}

/// (d) 先撃ちが rc 1（`unparsed`）で終わった行は、材料と lens の字が同じでも審査で偽 lens を撃つ。
#[test]
fn pipe_review_reuse_unparsed_prelens_verdict_fires_the_lens() {
    let (place, id, lens) = reuse_gated(&format!("echo '{}'; exit 1", reuse_pass()));
    reuse_land(&place, Some(&id), None, &lens);
    let (detail, _) = reuse_review(&place, &lens);
    assert_eq!(prelens_count(&place.state), 2, "測れなかった判定は使い回さない: {detail}");
    assert!(!detail.contains("prelens:reused"), "{detail}");
    prelens_clean(&place, &[]);
}

/// (e) 起こす側の lens の cmd の字と便の lens の cmd の字が違う周（末尾の空白 1 つ）の審査は偽 lens を撃つ。
#[test]
fn pipe_review_reuse_other_lens_cmd_fires_the_lens() {
    let (place, id, lens) = reuse_gated(&format!("echo '{}'", reuse_pass()));
    reuse_land(&place, Some(&id), None, &lens);
    let (detail, _) = reuse_review(&place, &format!("{lens} "));
    assert_eq!(prelens_count(&place.state), 2, "lens の字が違えば使い回さない: {detail}");
    assert!(!detail.contains("prelens:reused"), "{detail}");
    prelens_clean(&place, &[]);
}

// ───── lens の model の分け（設計 pipeline.md §61・契約表の行 bd・接頭辞 `model_split_`） ─────

/// (h) 先撃ちは穴を埋めた lens の行の末尾に `--stage prelens` を足して起こす（§61 形 4）: 引数を写す偽 lens の argv の末尾 2 語が
/// `--stage prelens` で、先頭は穴を埋めた `--contract <材料の契約>`。置き場の `lens` の字は穴を埋める前の cmd のまま（足した flag を
/// 含まない）。base は flag を足さないので末尾が `--worktree` の値になり RED。
#[test]
fn model_split_prelens_fires_the_lens_with_the_stage_as_the_last_two_words() {
    use std::os::unix::fs::PermissionsExt;
    let place = prelens_place(Some(1), &[PRELENS_B]);
    let (argv, script) = (place.state.join("prelens-argv"), place.state.join("prelens-argv-lens"));
    let body = format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\necho '{}'\n", argv.display(), lens_verdict("PASS"));
    fs::write(&script, body).expect("偽 lens を書ける");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("偽 lens に実行権を付ける");
    let lens = format!("'{}' --contract '{{contract}}' --worktree '{{worktree}}'", script.display());
    prelens_turn(&place, &lens);
    assert!(prelens_out(&place.state, PRELENS_B), "先撃ちの偽 lens が終わる: {}", prelens_result(&place.state, PRELENS_B));
    let text = fs::read_to_string(&argv).unwrap_or_default();
    let words: Vec<&str> = text.lines().collect();
    assert_eq!(words.len(), 6, "argv は --contract / --worktree の 2 対と段の 2 語: {words:?}");
    assert_eq!(words.get(4..), Some(&["--stage", "prelens"][..]), "末尾 2 語は段の flag: {words:?}");
    assert_eq!(words.first(), Some(&"--contract"), "{words:?}");
    assert!(words.get(1).is_some_and(|path| path.ends_with("contract.toml") && !path.contains('{')), "穴は埋まる: {words:?}");
    let kept = fs::read_to_string(prelens_dir(&place.state, PRELENS_B).join("lens")).unwrap_or_default();
    assert_eq!(kept, lens, "置き場の lens の字は穴を埋める前の cmd のまま");
    prelens_clean(&place, &[]);
}

/// (i) 形 ac 1 の使い回しは審査が読む manifest の `lens.model` と `pipe.precheck_lens_model` が両方読めて同じ `Model` に解ける周だけ
/// （§61 形 5）: 2 行が違う manifest と片方の行が無い manifest では、材料・判定・lens の字が同じでも Reviewed の段が偽 lens を撃ち
/// （回数 2）detail に ` prelens:reused` が無い。字面は違っても同じ model に解ける 2 行（`sonnet` と表示名 `Sonnet`）は今どおり使い回す
/// （回数 1）。base は model を比べず全部の周で使い回すので RED。
#[test]
fn model_split_review_reuses_only_when_both_model_rows_resolve_to_one_model() {
    let cases = [
        ((Some("opus"), Some("sonnet")), false),
        ((Some("opus"), None), false),
        ((None, Some("opus")), false),
        ((Some("sonnet"), Some("Sonnet")), true),
    ];
    for (models, reused) in cases {
        let (place, id, lens) = reuse_gated(&format!("echo '{}'", reuse_pass()));
        reuse_land(&place, Some(&id), None, &lens);
        prelens_rules_with(&place.state, Some(1), models);
        let (detail, costs) = reuse_review(&place, &lens);
        let (count, want) = if reused { (1, "verdict:PASS prelens:reused") } else { (2, "verdict:PASS") };
        assert_eq!(prelens_count(&place.state), count, "{models:?}: 審査の偽 lens の回数: {detail}");
        assert_eq!(detail, want, "{models:?}: detail");
        assert_eq!(costs, count - 1, "{models:?}: 撃った審査だけが消費の event を書く");
        prelens_clean(&place, &[]);
    }
}
