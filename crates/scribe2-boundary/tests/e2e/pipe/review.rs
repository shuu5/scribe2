// flip-check: moved s2-07l.351
//! 審査の段の歯: `pipe_review_`（契約の審査の段・審査の理由の閉じた型・要件の読み）。
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
        Some(&["- =src/zq_place.rs: 行数 全体 7 / 本体 2・置き場だけ（中身は変えない）", "  宣言: fn placed", "  歯: zq_tooth"][..]),
        "{summary}"
    );
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
