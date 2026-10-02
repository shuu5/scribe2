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

/// 審査の lens が写した 4 行 `holes`（`{contract}`・`{worktree}`・cwd・木の中の `git rev-parse HEAD`）が、便 `id` の審査の木を指す:
/// `{worktree}` は run dir の直下の `<head>.tree`・cwd は同じ path・木の HEAD は審査の前の repo の HEAD `head`・`review.json` の `tree` も
/// その sha・審査の後に木の dir も worktree の登録も無い。
fn assert_reviewed_in_its_tree(repo: &Path, state: &Path, id: &str, head: &str, holes: &str) {
    let tree = state.join("pipe").join(id).join(format!("{head}.tree"));
    let physical = fs::canonicalize(state.join("pipe").join(id)).map(|found| found.join(format!("{head}.tree")));
    assert_eq!(
        holes.lines().collect::<Vec<&str>>(),
        [
            review_dir(state, id).join("contract.toml").display().to_string(),
            tree.display().to_string(),
            physical.map(|found| found.display().to_string()).unwrap_or_default(),
            head.to_owned(),
        ],
        "{{contract}} は審査の写し・{{worktree}} と cwd は審査の木・その木の HEAD は審査の前の repo の HEAD"
    );
    assert_ne!(tree, repo, "審査の木は anchor の repo と違う path");
    assert_eq!(value_of(&review_pairs(state, id), "tree"), head, "review.json の tree は木の sha");
    assert!(!tree.exists(), "審査の後に木の dir は無い");
    assert_eq!(git(repo, &["worktree", "list"]).lines().count(), 1, "git worktree list は anchor だけ");
}

/// (b) PASS の契約は `Reviewed(PASS)` を経て Spawned へ進み、verdict が便の記録（`review.json` と event）に残る（AC22）。
/// 審査の材料は run dir の `review/`（契約の写し・設計の節・要件本文）に置かれ、lens の `{contract}` はその写しの
/// path・`{worktree}` は審査の木（run dir の直下の `<sha>.tree`・審査の前の repo の HEAD に detach した worktree）で埋まり、lens の cwd も
/// 同じ木で、その木の HEAD は審査の前の repo の HEAD と同じ・`review.json` の `tree` がその sha・審査の後に木は無く `git worktree list` は
/// anchor だけ（(f)・設計 pipeline.md §64 形 3）。pointer でない `design` と読めない要件面は材料の本文に明示される。
// flip-check: retroactive s2-07l.736.33.1
#[test]
fn pipe_review_pass_spawns() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let head = git(&repo, &["rev-parse", "HEAD"]).trim().to_owned();
    let seen = state.join("lens-holes");
    let lens = format!(
        "cat >/dev/null; printf '%s\\n' '{{contract}}' '{{worktree}}' \"$(pwd -P)\" \"$(git rev-parse HEAD)\" > '{}'; echo '{}'",
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
    assert_reviewed_in_its_tree(&repo, &state, &id, &head, &fs::read_to_string(&seen).unwrap_or_default());
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
// flip-check: moved s2-07l.736.33.10
// 先撃ちの私有の 4 本（materialize・declare・requirements_of・digest）は pipe の review の子 module `tree` へ純移動した（歯は足さない）。
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
/// base に在り（空の F が tracked）、一時の worktree は lens が終わった後の次の周の頭に外れ、置き場にも `git worktree list` にも残らない。
// flip-check: retroactive s2-07l.736.33.1
#[test]
fn pipe_prelens_declared_ancestor_places_empty_files_in_a_temporary_tree() {
    let rows = [prelens_row("a", r#"["+src/fresh.rs"]"#, &[]), prelens_row("b", r#"["src/fresh.rs"]"#, &[r#"depends = ["a"]"#])];
    let doc = design_doc_rows(&rows).replace("節の本文。", "節の本文。`src/fresh.rs` を読む。");
    let (repo, state) = prelens_repo(&doc, &[]);
    let bd = prelens_ledger(&state, &[prelens_issue(PRELENS_A, "open", "a", &[]), prelens_issue(PRELENS_B, "open", "b", &[PRELENS_A])]);
    prelens_hold(&state, &[PRELENS_A]);
    let place = Prelens { rules: prelens_rules(&state, Some(1)), repo, state, bd };
    let seen = place.state.join("prelens-seen");
    let lens = prelens_lens(&place.state, &prelens_copy(&seen));
    prelens_turn(&place, &lens);
    assert_eq!(prelens_word(&place.state, PRELENS_B), "clean", "{}", prelens_result(&place.state, PRELENS_B));
    assert!(prelens_out(&place.state, PRELENS_B), "偽 lens が終わる");
    let design = fs::read_to_string(seen.join("design.txt")).unwrap_or_default();
    let note ="予想の base: 次の file は未着地の祖先の宣言で、本文を空で置いた\n- src/fresh.rs\n";
    assert!(design.ends_with(note), "設計の材料の末尾に予想の 1 行と F: {design}");
    let base = fs::read_to_string(seen.join("base.txt")).unwrap_or_default();
    assert!(base.contains("- src/fresh.rs: 行数 全体 0 / "), "空の F: {base}");
    let outside = fs::read_to_string(seen.join("outside.txt")).unwrap_or_default();
    assert!(outside.contains("  +src/fresh.rs: base に在る"), "空の F が tracked: {outside}");
    prelens_turn(&place, &lens);
    assert!(!prelens_dir(&place.state, PRELENS_B).join("tree").exists(), "lens が終わった後の次の周の後に worktree は置き場に残らない");
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
/// `firm:1,provisional:0` のままで finding は `new=false`、`[DISPATCH-BUNDLE]` の行は commit の前と同じ。`--lens` 無しで撃った周も
/// 同じ（撃たない周も写し直す）。値 0 の周は持ち越さない（`pipe_prelens_off_` の (c)・設計 row-review.md §6）。
#[test]
fn pipe_prelens_same_material_keeps_the_finding_across_a_main_move() {
    let (place, lens) = prelens_crossing();
    let before = prelens_bundles(&place);
    let rounds: [(&str, &dyn Fn()); 2] = [
        ("--lens の周", &|| {
            prelens_turn(&place, &lens);
        }),
        ("--lens 無しの周", &|| {
            prelens_round(&place, None);
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

// ───── 先撃ちの退役の段 1（設計 row-review.md §6・行 d・接頭辞 `pipe_prelens_off_`） ─────
// 値 0 の周は片付けの後で返る＝材料の組み直しも一時の worktree も前の判定の写し直しも撃たない。base は値 0 の周も組み直して写し直す。

/// (a) 値 0 の写しの周は、clean の依存待ちの行 B の置き場に材料の dir も key も一時の worktree も作らず、worktree の登録も増やさない。
/// 値 1 の同じ fixture の周は作る（対照）。
#[test]
fn pipe_prelens_off_builds_no_material_at_value_zero_but_builds_at_value_one() {
    let pass = lens_verdict("PASS");
    for (limit, built) in [(0, false), (1, true)] {
        let place = prelens_place(Some(limit), &[PRELENS_B]);
        prelens_turn(&place, &prelens_lens(&place.state, &format!("echo '{pass}'")));
        prelens_settle(&place.state, usize::from(built));
        let dir = prelens_dir(&place.state, PRELENS_B);
        assert_eq!(dir.join("key").exists(), built, "値 {limit}: key");
        assert_eq!(dir.join("review").exists(), built, "値 {limit}: 材料の dir");
        let list = git(&place.repo, &["worktree", "list", "--porcelain"]);
        assert!(built || !dir.join("tree").exists(), "値 {limit}: 一時の worktree の dir");
        assert!(built || !list.contains("precheck/lens"), "値 {limit}: worktree の登録: {list}");
        assert_eq!(prelens_word(&place.state, PRELENS_B), "clean", "値 {limit}: 事前審査は clean");
        prelens_clean(&place, &[]);
    }
}

/// (b) 値 0 の周も、母集団を出た bead の置き場と前の周の木（lens が終わった行の一時の worktree）を外す。
#[test]
fn pipe_prelens_off_still_prunes_the_gone_places_and_the_leftover_trees() {
    let place = prelens_place(Some(1), &[PRELENS_B]);
    let pass = lens_verdict("PASS");
    prelens_turn(&place, &prelens_lens(&place.state, &format!("echo '{pass}'")));
    assert!(prelens_out(&place.state, PRELENS_B), "B の偽 lens が終わる");
    let gone = prelens_dir(&place.state, "s2-pre.gone");
    fs::create_dir_all(&gone).expect("母集団を出た bead の置き場を作れる");
    fs::write(gone.join("key"), "x\n").expect("置き場に file を書ける");
    let tree = prelens_dir(&place.state, PRELENS_B).join("tree");
    assert!(tree.join(".git").is_file(), "周の前は B の木が在る");
    prelens_rules(&place.state, Some(0));
    prelens_round(&place, None);
    assert!(!gone.exists(), "母集団を出た bead の置き場は外れる");
    assert!(!tree.exists(), "前の周の木は外れる");
    let list = git(&place.repo, &["worktree", "list", "--porcelain"]);
    assert!(!list.contains("precheck/lens"), "worktree の登録も外れる: {list}");
    prelens_clean(&place, &[]);
}

/// (c) 値 1 の写しで B が FAIL の確定を写し終え C の lens が撃ち中の置き場で、値 0 に書き換えて材料に入らない file を main に commit した
/// 次の周の後、B の置き場の key は周の前と同じ字、B の結果は clean で finding の行を持たず、束の行は 0 本（確定は持ち越さない）。
#[test]
fn pipe_prelens_off_neither_rebuilds_nor_carries_a_finding_over() {
    let (place, lens) = prelens_crossing();
    let key = || fs::read_to_string(prelens_dir(&place.state, PRELENS_B).join("key")).unwrap_or_default();
    let before = key();
    assert!(!before.is_empty() && !prelens_bundles(&place).is_empty(), "周の前は key と束が在る");
    prelens_rules(&place.state, Some(0));
    prelens_unrelated(&place.repo, "unrelated-off.txt");
    prelens_turn(&place, &lens);
    assert_eq!(key(), before, "B の key は周の前と同じ字");
    let result = prelens_result(&place.state, PRELENS_B);
    assert_eq!(prelens_word(&place.state, PRELENS_B), "clean", "B の結果は clean: {result}");
    assert!(!result.lines().any(|found| found.starts_with("finding=")), "finding の行を持たない: {result}");
    assert!(prelens_bundles(&place).is_empty(), "束の行は 0 本: {:?}", prelens_bundles(&place));
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
/// 周は ` prelens=unbuilt` で終わらずに偽 lens を 1 回起こし、lens が終わった後の次の周の後の `git worktree list --porcelain` がその path を
/// 持たない（残りの木を外して組み直した木は、撃った lens が cwd に読む間は残る）。
// flip-check: retroactive s2-07l.736.33.1
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
        let lens = prelens_lens(&place.state, &format!("echo '{}'", lens_verdict("PASS")));
        prelens_turn(&place, &lens);
        assert_eq!(prelens_settle(&place.state, 1), 1, "{label}: 残りを外して撃つ");
        let line = prelens_line(&place, PRELENS_B);
        assert!(!line.ends_with(" prelens=unbuilt"), "{label}: ls の行: {line}");
        assert!(prelens_out(&place.state, PRELENS_B), "{label}: 偽 lens が終わる");
        prelens_turn(&place, &lens);
        let list = git(&place.repo, &["worktree", "list", "--porcelain"]);
        assert!(!list.contains(&suffix), "{label}: lens が終わった後の次の周の後に残りは無い: {list}");
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

/// 置き場の木の印 `tree.sha` の本文を `text` の 1 行に書き換える（[`reuse_actual`] と (k) が使う）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn reuse_mark(place: &Prelens, text: &str) {
    fs::write(prelens_dir(&place.state, PRELENS_B).join("tree.sha"), format!("{text}\n")).expect("木の印を書き換えられる");
}

/// 置き場の木の印を `<着地後の main の HEAD> actual` に書き換える（祖先の層の無い予想の置き場を模す・§64 形 5）。HEAD を返す。
fn reuse_actual(place: &Prelens) -> String {
    let head = git(&place.repo, &["rev-parse", "HEAD"]).trim().to_owned();
    reuse_mark(place, &format!("{head} actual"));
    head
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
/// 撃たず（回数 1 のまま）判定を写し、detail が ` prelens:reused` で終わり、審査の消費の event を書かない。置き場の木の印は審査の前に
/// `<着地後の HEAD> actual` に書き換える（層の無い予想の置き場を模す・§64 形 5）。
// flip-check: retroactive s2-07l.736.33.1
#[test]
fn pipe_review_reuse_same_material_and_lens_copies_the_verdict_without_firing() {
    let (place, id, lens) = reuse_gated(&format!("echo '{}'", reuse_pass()));
    reuse_land(&place, Some(&id), None, &lens);
    reuse_actual(&place);
    let (detail, costs) = reuse_review(&place, &lens);
    assert_eq!(prelens_count(&place.state), 1, "審査は偽 lens を撃たない: {detail}");
    assert_eq!(detail, "verdict:PASS prelens:reused", "先撃ちの判定を写し末尾に語");
    assert_eq!(costs, 0, "使い回した審査は消費の event を書かない");
    prelens_clean(&place, &[]);
}

/// (g) 行 b の done が `(1) 甲 (2) 乙` で、先撃ちの偽 lens が PASS と表 `1:-,2:t` を返した周の審査は、偽 lens を撃たず（回数 1）使い回した
/// 判定を倒しの 1 本に通し、detail が `verdict:FAIL kind:vacuous-assert prelens:reused`・review.json の at が `done(1)`（置き場の木の印は
/// 審査の前に `<着地後の HEAD> actual` に書き換える・§64 形 5）。
// flip-check: retroactive s2-07l.736.33.1
#[test]
fn pipe_review_done_items_reused_prelens_verdict_goes_through_the_same_fall() {
    let (place, id, lens) = reuse_gated_with(&format!("echo '{}'", table_pass("1:-,2:t")), Some("(1) 甲 (2) 乙"));
    reuse_land(&place, Some(&id), None, &lens);
    reuse_actual(&place);
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
/// （回数 1）。base は model を比べず全部の周で使い回すので RED。置き場の木の印は審査の前に `<着地後の HEAD> actual` に書き換える。
// flip-check: retroactive s2-07l.736.33.1
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
        reuse_actual(&place);
        prelens_rules_with(&place.state, Some(1), models);
        let (detail, costs) = reuse_review(&place, &lens);
        let (count, want) = if reused { (1, "verdict:PASS prelens:reused") } else { (2, "verdict:PASS") };
        assert_eq!(prelens_count(&place.state), count, "{models:?}: 審査の偽 lens の回数: {detail}");
        assert_eq!(detail, want, "{models:?}: detail");
        assert_eq!(costs, count - 1, "{models:?}: 撃った審査だけが消費の event を書く");
        prelens_clean(&place, &[]);
    }
}

// ───── 審査役は読みの道具だけで起きる（設計 pipeline.md §64・契約表の行 bg・接頭辞 `lens_read_`） ─────
//
// 契約の審査の lens は審査の前の repo の HEAD に detach した木（run dir の直下の `<sha>.tree`）を cwd と `{worktree}` に読み、先撃ちの lens は
// 置き場の木（祖先の層を当てた予想の木）を読む。先撃ちの判定は置き場の木の印 `tree.sha` が `<審査の木の sha> actual` の周だけ Reviewed に写る。

/// 受付だけ済んだ便 `id`（`RunCreated` 1 件・bead `bead`）を組む: 写し面（契約・vessel・repo）は `seeded` の便から写す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn read_staged_run(state: &Path, seeded: &str, id: &str, bead: &str) {
    let dir = state.join("pipe").join(id);
    fs::create_dir_all(&dir).expect("run dir を作れる");
    for name in ["contract.toml", "vessel.toml", "repo"] {
        fs::copy(state.join("pipe").join(seeded).join(name), dir.join(name)).expect("写しを置ける");
    }
    let out = bin_cmd()
        .args(["fleet", "record", "--kind", "RunCreated", "--stage", "Intake", "--run", id, "--bead", bead, "--state-dir"])
        .arg(state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fleet record: {}", stderr_of(&out));
}

/// 受付だけ済んだ便 `id` を `resume` し、先に審査させる（runner は印を作るだけの偽）。
fn read_resume(repo: &Path, state: &Path, rules: &str, id: &str, lens: &str) -> Output {
    let marker = state.join(format!("runner-ran-{id}"));
    run_pipe(&[
        "resume", "--run", id, "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", rules, "--runner", &marker_runner(&marker), "--lens", lens,
    ])
}

/// 審査の木の path が決まる置き場: 受付だけ済んだ便 `id` と、その run dir の直下の `<審査の前の HEAD>.tree`。
fn read_staged_place(id: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let seeded = intake(&repo, &state, &path);
    stop_run_ok(&state, &seeded);
    read_staged_run(&state, &seeded, id, "s2-live");
    let head = git(&repo, &["rev-parse", "HEAD"]);
    let tree = state.join("pipe").join(id).join(format!("{}.tree", head.trim()));
    (repo, state, tree)
}

/// (g) 審査の木の path を worktree でない file が塞ぐ周は、lens を撃たず INCONCLUSIVE で、evidence が審査の木とその path を名指す
/// （anchor の作業木へは倒さない）。塞いだ file は消さない。
#[test]
fn lens_read_tree_path_blocked_by_a_file_is_inconclusive_and_never_fires_the_lens() {
    let (repo, state, tree) = read_staged_place("s2-blk-1");
    fs::write(&tree, "not a tree\n").expect("木の path に file を置ける");
    let marker = state.join("blocked-lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = read_resume(&repo, &state, &ceiling_rules(&state), "s2-blk-1", &lens);
    let pairs = review_pairs(&state, "s2-blk-1");
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "{pairs:?} / {}", stderr_of(&out));
    let evidence = value_of(&pairs, "evidence");
    assert!(evidence.contains("審査の木") && evidence.contains(&tree.display().to_string()), "審査の木を名指す: {evidence}");
    assert!(!marker.exists(), "偽の lens は撃たれない");
    assert!(tree.is_file(), "塞いだ file は消さない");
    clean(&[&repo, &state]);
}

/// (g2) 同じ path に前の周の登録済みの worktree が残る周は、それを外して審査が進む: 偽 lens の印が在り、判定は PASS で、審査の後に
/// その dir も worktree の登録も無い。
#[test]
fn lens_read_leftover_registered_tree_at_the_path_is_removed_and_the_review_goes_on() {
    let (repo, state, tree) = read_staged_place("s2-left-1");
    git(&repo, &["worktree", "add", "-q", "--detach", &tree.display().to_string(), "HEAD"]);
    assert!(tree.join(".git").is_file(), "前提: 前の周の登録済みの worktree が在る");
    let marker = state.join("left-lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = read_resume(&repo, &state, &ceiling_rules(&state), "s2-left-1", &lens);
    let pairs = review_pairs(&state, "s2-left-1");
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "{pairs:?} / {}", stderr_of(&out));
    assert!(marker.exists(), "偽 lens が撃たれる");
    assert!(!tree.exists(), "審査の後に dir は無い");
    let list = git(&repo, &["worktree", "list", "--porcelain"]);
    assert!(!list.contains(&tree.display().to_string()), "worktree の登録も無い（PASS の後の spawn の便の worktree は別の path）: {list}");
    clean(&[&repo, &state]);
}

/// (g3) 使い回せる先撃ち（置き場の `tree.sha` が `<HEAD> actual`）を持つ周は木を作らない: 審査の木の path に file を置いても INCONCLUSIVE
/// でなく使い回しの判定になり（detail が ` prelens:reused` で終わる）、偽 lens は撃たれず（回数 1）file は残る。
#[test]
fn lens_read_reusable_prelens_verdict_needs_no_tree_even_when_the_path_is_blocked() {
    let (place, id, lens) = reuse_gated(&format!("echo '{}'", reuse_pass()));
    reuse_land(&place, Some(&id), None, &lens);
    let head = reuse_actual(&place);
    let out = run_pipe(&[
        "intake", "--design", &format!("{DESIGN_FILE}#b"), "--bead", PRELENS_B, "--repo", &place.repo.display().to_string(),
        "--state-dir", &place.state.display().to_string(), "--rules", &place.rules, "--lens", &lens,
    ]);
    let seeded = run_id_of(&out);
    assert!(!seeded.is_empty(), "前提: 写しを取る便が審査まで届く: {} / {}", stdout_of(&out), stderr_of(&out));
    stop_run_ok(&place.state, &seeded);
    read_staged_run(&place.state, &seeded, "s2-g3-1", PRELENS_B);
    let tree = place.state.join("pipe").join("s2-g3-1").join(format!("{head}.tree"));
    fs::write(&tree, "not a tree\n").expect("木の path に file を置ける");
    let resumed = read_resume(&place.repo, &place.state, &place.rules, "s2-g3-1", &lens);
    assert_eq!(reviewed_detail(&place.state, "s2-g3-1"), "verdict:PASS prelens:reused", "{}", stderr_of(&resumed));
    assert_eq!(prelens_count(&place.state), 1, "偽 lens は撃たれない");
    assert!(tree.is_file(), "木を作らないので file は残る");
    prelens_clean(&place, &[]);
}

/// (i) 先撃ちの lens の cwd と `{worktree}` は置き場の `tree` で、そこに祖先の層の file が在る（宣言だけの祖先の `+` の file が空で在る）。
#[test]
fn lens_read_prelens_lens_reads_the_placed_tree_that_carries_the_ancestor_layer() {
    let rows = [prelens_row("a", r#"["+src/fresh.rs"]"#, &[]), prelens_row("b", r#"["src/fresh.rs"]"#, &[r#"depends = ["a"]"#])];
    let (repo, state) = prelens_repo(&design_doc_rows(&rows), &[]);
    let bd = prelens_ledger(&state, &[prelens_issue(PRELENS_A, "open", "a", &[]), prelens_issue(PRELENS_B, "open", "b", &[PRELENS_A])]);
    prelens_hold(&state, &[PRELENS_A]);
    let place = Prelens { rules: prelens_rules(&state, Some(1)), repo, state, bd };
    let (cwd, hole, layer) = (place.state.join("read-cwd"), place.state.join("read-hole"), place.state.join("read-layer"));
    let body = format!(
        "pwd -P > '{}'; printf '%s' '{{worktree}}' > '{}'; if [ -f src/fresh.rs ]; then echo yes > '{}'; fi; echo '{}'",
        cwd.display(),
        hole.display(),
        layer.display(),
        lens_verdict("PASS")
    );
    prelens_turn(&place, &prelens_lens(&place.state, &body));
    assert!(prelens_out(&place.state, PRELENS_B), "偽 lens が終わる: {}", prelens_result(&place.state, PRELENS_B));
    let dir = prelens_dir(&place.state, PRELENS_B);
    assert_eq!(fs::read_to_string(&hole).unwrap_or_default(), dir.join("tree").display().to_string(), "{{worktree}} は置き場の tree");
    let physical = fs::canonicalize(&dir).map(|found| found.join("tree").display().to_string()).unwrap_or_default();
    assert_eq!(fs::read_to_string(&cwd).unwrap_or_default().trim_end(), physical, "cwd は置き場の tree");
    assert_eq!(fs::read_to_string(&layer).unwrap_or_default().trim_end(), "yes", "祖先の層の file が木に在る");
    let mark = fs::read_to_string(dir.join("tree.sha")).unwrap_or_default();
    assert!(mark.trim_end().ends_with(" forecast"), "層を当てた予想の印: {mark:?}");
    prelens_clean(&place, &[]);
}

/// (j) 撃ち中の偽 lens（印の file が出るまで待つ）の間に撃った次の周の後も、置き場の `tree` が在り `git worktree list` に載る。lens を
/// 終わらせた後の周の後は、置き場にも `git worktree list` にも無い。
#[test]
fn lens_read_flying_prelens_keeps_its_tree_and_the_ended_one_goes_at_the_next_round() {
    let place = prelens_place(Some(1), &[PRELENS_B]);
    let release = place.state.join("read-release");
    let body = format!("while [ ! -e '{}' ]; do sleep 0.05; done; echo '{}'", release.display(), lens_verdict("PASS"));
    let lens = prelens_lens(&place.state, &body);
    prelens_turn(&place, &lens);
    assert_eq!(prelens_settle(&place.state, 1), 1, "1 周目に 1 本");
    let tree = prelens_dir(&place.state, PRELENS_B).join("tree");
    let suffix = format!("precheck/lens/{PRELENS_B}/tree");
    assert!(tree.join(".git").is_file(), "撃ち中は木が在る");
    prelens_turn(&place, &lens);
    assert!(tree.join(".git").is_file(), "撃ち中の間の周の後も木は外れない");
    assert!(git(&place.repo, &["worktree", "list", "--porcelain"]).contains(&suffix), "登録も残る");
    fs::write(&release, "").expect("印を置ける");
    assert!(prelens_out(&place.state, PRELENS_B), "偽 lens が終わる");
    prelens_turn(&place, &lens);
    assert!(!tree.exists(), "lens が終わった後の周の後に置き場に木は無い");
    let list = git(&place.repo, &["worktree", "list", "--porcelain"]);
    assert!(!list.contains(&suffix) && git(&place.repo, &["worktree", "list"]).lines().count() == 1, "登録も無い: {list}");
    prelens_clean(&place, &[]);
}

/// (k) 先撃ちの判定は、置き場の木の印が `<審査の木の sha> actual` の周だけ Reviewed に写る: Gated の祖先を着地させた後の審査は、印が
/// `forecast` のままの周に偽 lens を撃ち（回数 2・detail に語が無い）、`<着地後の HEAD> actual` に書き換えた周は撃たずに写し（回数 1）、
/// `<別の sha> actual` に書き換えた周は撃つ（回数 2）。
#[test]
fn lens_read_prelens_verdict_reaches_reviewed_only_when_it_read_the_landed_tree() {
    let other = "0123456789abcdef0123456789abcdef01234567";
    for (label, mark, reused) in [("forecast のまま", None, false), ("今の HEAD の actual", Some(false), true), ("別の sha の actual", Some(true), false)] {
        let (place, id, lens) = reuse_gated(&format!("echo '{}'", reuse_pass()));
        reuse_land(&place, Some(&id), None, &lens);
        let natural = fs::read_to_string(prelens_dir(&place.state, PRELENS_B).join("tree.sha")).unwrap_or_default();
        assert!(natural.trim_end().ends_with(" forecast"), "{label}: 層を当てた予想の印は forecast: {natural:?}");
        match mark {
            None => {}
            Some(false) => {
                reuse_actual(&place);
            }
            Some(true) => reuse_mark(&place, &format!("{other} actual")),
        }
        let (detail, _) = reuse_review(&place, &lens);
        let (count, want) = if reused { (1, "verdict:PASS prelens:reused") } else { (2, "verdict:PASS") };
        assert_eq!(prelens_count(&place.state), count, "{label}: 審査の偽 lens の回数: {detail}");
        assert_eq!(detail, want, "{label}: detail");
        prelens_clean(&place, &[]);
    }
}

/// 祖先の層の無い待ち行 B の置き場（B は open の blocks の相手 X〔契約の行でない bead〕を待つ・B は hold）。
fn read_layerless_place() -> Prelens {
    let rows = [prelens_row("b", r#"["src/b.rs"]"#, &[])];
    let (repo, state) = prelens_repo(&design_doc_rows(&rows), &[("src/b.rs", "pub fn b() {}\n")]);
    let blocker = r#"{"id":"s2-pre.x","status":"open","priority":2,"labels":[],"acceptance_criteria":"","dependencies":[]}"#.to_owned();
    let bd = prelens_ledger(&state, &[blocker, prelens_issue(PRELENS_B, "open", "b", &["s2-pre.x"])]);
    prelens_hold(&state, &[PRELENS_B]);
    Prelens { rules: prelens_rules(&state, Some(1)), repo, state, bd }
}

/// (l) 先撃ちが判定を撃った後に main が動いて組み直した置き場は、判定と `tree.sha` が撃った木を指し続け、今の HEAD の契約の審査は lens を
/// 撃つ（回数 2・語が無い）。置き場の `tree.sha` は `build`・`rebuild`・`fire` の経路だけで作り、手で書かない。対照: 組み直し後の `tree.sha`
/// を今の HEAD に貼り替えた置き場は同じ材料・判定で使い回す（回数 1）＝撃った周が材料の食い違いでなく木の印で撃ったと分かる。
#[test]
fn lens_read_prelens_verdict_stays_on_the_tree_it_read_across_a_main_move() {
    for (label, pasted) in [("組み直しのまま", false), ("今の HEAD へ貼り替えた対照", true)] {
        let place = read_layerless_place();
        let lens = prelens_lens(&place.state, &format!("echo '{}'", reuse_pass()));
        reuse_fire(&place, &lens);
        let dir = prelens_dir(&place.state, PRELENS_B);
        let fired_on = fs::read_to_string(dir.join("tree.sha")).unwrap_or_default();
        let old_head = git(&place.repo, &["rev-parse", "HEAD"]).trim().to_owned();
        assert_eq!(fired_on, format!("{old_head} actual\n"), "{label}: 層の無い予想の印は撃った時の HEAD の actual");
        fs::write(place.repo.join("src").join("unrelated.rs"), "pub fn u() {}\n").expect("file を書ける");
        git(&place.repo, &["add", "-A"]);
        git(&place.repo, &["commit", "-q", "-m", "code-moved"]);
        prelens_turn(&place, &lens);
        assert_eq!(prelens_count(&place.state), 1, "{label}: 材料が同じなので撃ち直さない");
        assert!(dir.join("out").exists(), "{label}: 判定は残る");
        assert_eq!(fs::read_to_string(dir.join("tree.sha")).unwrap_or_default(), fired_on, "{label}: 組み直しでも印は撃った木のまま");
        if pasted {
            reuse_actual(&place);
        }
        let (detail, _) = reuse_review(&place, &lens);
        let (count, want) = if pasted { (1, "verdict:PASS prelens:reused") } else { (2, "verdict:PASS") };
        assert_eq!(prelens_count(&place.state), count, "{label}: 審査の偽 lens の回数: {detail}");
        assert_eq!(detail, want, "{label}: detail");
        prelens_clean(&place, &[]);
    }
}

// ───── 行の審査の記録の読み手と鍵の口（設計 docs/design/row-review.md §3・§9・行 a1・接頭辞 `pipe_review_record_`） ─────
//
// 口 (A)〜(F) と (H) を e2e が直に呼ぶ（`vessel::pipe::row_review`）。記録は §9 の形で手で書く（書き手は行 a の歯が測る）。
// 口 (G) は事前審査の予想の外形（`pipe_dispatch_precheck_table_depends_`）が測る。

use vessel::pipe::gate::Verdict;
use vessel::pipe::review::FindingKind;
use vessel::pipe::row_review::{self, Basis, Listed, Parts, RefResult};

/// 40 桁の 16 進の sha（字は 1 字で埋める）。
fn rr_sha(digit: char) -> String {
    digit.to_string().repeat(40)
}

/// 置き場の行の審査の根（§9: state dir の pipe の下の row-review）。
fn rr_root(state: &Path) -> PathBuf {
    state.join("pipe").join("row-review")
}

/// 空の置き場（tmp の下の `state`）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rr_state() -> PathBuf {
    let state = tmp().join(STATE_LEAF);
    fs::create_dir_all(rr_root(&state).join("ref")).expect("置き場を作れる");
    state
}

/// ref の記録の file（本文をそのまま置く）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rr_ref(state: &Path, sha: &str, body: &str) {
    fs::write(rr_root(state).join("ref").join(sha), body).expect("ref の記録を書ける");
}

/// 撃ち中の印（`<pid> <起動時刻>`）を置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rr_mark(state: &Path, sha: &str, pid: u32) {
    let started = vessel::fleet::store::started_ms(pid).started();
    let body = started.map_or_else(|| format!("{pid} 1\n"), |at| format!("{pid} {at}\n"));
    fs::write(rr_root(state).join("ref").join(format!("{sha}.pid")), body).expect("印を書ける");
}

/// 終わって回収した子の pid（今は無い process・印の持ち主が死んだ形）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rr_dead_pid() -> u32 {
    let mut child = Command::new("true").spawn().expect("子を起こせる");
    let pid = child.id();
    child.wait().expect("子を待てる");
    pid
}

/// 済んだ ref の記録の本文（`result` の行は呼び手が足す）。
fn rr_finished(result: Option<&str>) -> String {
    let rows = "row=docs/design/toy.md#x digest=0000000000000001 id=0000000000000002 verdict=PASS basis=actual\n";
    let tail = result.map_or_else(String::new, |word| format!("result={word}\n"));
    format!("schema=1\nbase={}\ntables=docs/design/toy.md,docs/design/other.md\n{rows}{tail}", rr_sha('b'))
}

/// (a) 口 (A): result=pass は pass と記録の merge-base の sha と契約表の file の列、result=fail は fail、生きた印は pending、死んだ pid の印と
/// result の行の無い記録は stale、file が無い・schema の違う file・dir で置いた file・result の行も印も無い file と 40 桁でない sha は missing。
#[test]
fn pipe_review_record_ref_result_is_one_of_five_values() {
    let state = rr_state();
    let (pass, fail, pending, stale) = (rr_sha('1'), rr_sha('2'), rr_sha('3'), rr_sha('4'));
    rr_ref(&state, &pass, &rr_finished(Some("pass")));
    rr_ref(&state, &fail, &rr_finished(Some("fail")));
    rr_ref(&state, &pending, &rr_finished(None));
    rr_mark(&state, &pending, std::process::id());
    rr_ref(&state, &stale, &rr_finished(None));
    rr_mark(&state, &stale, rr_dead_pid());
    let tables = vec!["docs/design/toy.md".to_owned(), "docs/design/other.md".to_owned()];
    assert_eq!(row_review::read_ref(&state, &pass), RefResult::Pass { base: rr_sha('b'), tables }, "pass");
    assert_eq!(row_review::read_ref(&state, &fail), RefResult::Fail, "fail");
    assert_eq!(row_review::read_ref(&state, &pending), RefResult::Pending, "生きた印は pending");
    assert_eq!(row_review::read_ref(&state, &stale), RefResult::Stale, "死んだ印で result の行が無い記録は stale");
    // missing の 4 形（+ sha の形でない字）。
    let (old_schema, as_dir, bare) = (rr_sha('5'), rr_sha('6'), rr_sha('7'));
    rr_ref(&state, &old_schema, &rr_finished(Some("pass")).replacen("schema=1", "schema=2", 1));
    fs::create_dir_all(rr_root(&state).join("ref").join(&as_dir)).expect("dir を置ける");
    rr_ref(&state, &bare, &rr_finished(None));
    for (label, sha) in [("file が無い", rr_sha('8')), ("schema の違う file", old_schema), ("dir で置いた file", as_dir), ("result の行も印も無い file", bare), ("sha の形でない字", "../ref".to_owned())] {
        assert_eq!(row_review::read_ref(&state, &sha), RefResult::Missing, "{label}は missing");
    }
    clean(&[&state]);
}

/// (b) 口 (B): 16 桁の小文字の 16 進。同じ 2 つの字の 2 回は同じ値で、契約 file の字か節の本文の字の 1 byte を変えた 2 形はそれぞれ違い、
/// 2 つの字の切れ目だけを動かした形（ab と c・a と bc）も違う。
#[test]
fn pipe_review_record_row_digest_is_sixteen_hex_and_keeps_the_seam() {
    let digest = row_review::row_digest("contract text", "section text");
    assert_eq!(digest.len(), 16, "16 桁: {digest}");
    assert!(digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)), "小文字の 16 進: {digest}");
    assert_eq!(digest, row_review::row_digest("contract text", "section text"), "同じ入力の 2 回は同じ値");
    assert_ne!(digest, row_review::row_digest("contract texu", "section text"), "契約 file の 1 byte");
    assert_ne!(digest, row_review::row_digest("contract text", "section texu"), "節の本文の 1 byte");
    assert_ne!(row_review::row_digest("ab", "c"), row_review::row_digest("a", "bc"), "切れ目だけを動かした形");
}

/// (c) 口 (C): toy repo で、契約表の file だけを変えた commit は鍵が動かず、code の file を 1 つ変えた commit は動く。無い sha は理由を持つ Err。
#[test]
fn pipe_review_record_tree_key_moves_with_code_and_not_with_the_table() {
    let (repo, state) = repo_with_state();
    let first = git(&repo, &["rev-parse", "HEAD"]);
    let before = row_review::tree_key(&repo, &first).expect("seed の木の鍵を読める");
    assert_eq!(before.len(), 16, "16 桁: {before}");
    let moved_table = design_doc(&row_fields("a", &["done"], &["done = \"別の done\""]));
    write_design(&repo, &moved_table);
    git(&repo, &["add", DESIGN_FILE]);
    git(&repo, &["commit", "-q", "-m", "table-only"]);
    let table_only = git(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(row_review::tree_key(&repo, &table_only), Ok(before.clone()), "契約表だけを変えた commit は動かない");
    fs::write(repo.join("src").join("lib.rs"), "// moved\n").expect("code を書ける");
    git(&repo, &["add", "src/lib.rs"]);
    git(&repo, &["commit", "-q", "-m", "code"]);
    let code = git(&repo, &["rev-parse", "HEAD"]);
    let after = row_review::tree_key(&repo, &code).expect("code の木の鍵を読める");
    assert_ne!(after, before, "code の file を変えた commit は動く");
    let missing = row_review::tree_key(&repo, &rr_sha('0')).expect_err("無い sha は Err");
    assert!(missing.contains(&rr_sha('0')), "理由は sha を名指す: {missing}");
    clean(&[&repo, &state]);
}

/// 判定の鍵の 6 材料の基準。
fn rr_parts<'a>(ancestors: &'a [String]) -> Parts<'a> {
    Parts { digest: "0000000000000001", materials: "0000000000000002", tree: "0000000000000003", basis: Basis::Actual, ancestors, version: "lens-version model=opus" }
}

/// (d) 口 (H): 6 つの材料のどれか 1 つを変えた 6 形でそれぞれ判定の鍵と記録の dir の名が変わり、同じ材料の 2 回は同じ対（16 桁）を返す。
#[test]
fn pipe_review_record_judgement_key_moves_with_each_of_the_six_materials() {
    let row = "docs/design/toy.md#x";
    let none: Vec<String> = Vec::new();
    let base = row_review::judgement(row, &rr_parts(&none));
    assert_eq!(base, row_review::judgement(row, &rr_parts(&none)), "同じ材料の 2 回は同じ対");
    assert!(base.0.len() == 16 && base.1.len() == 16, "16 桁の対: {base:?}");
    let landed = vec!["docs/design/toy.md#a:landed".to_owned()];
    let variants = [
        ("行の digest", Parts { digest: "0000000000000009", ..rr_parts(&none) }),
        ("材料の鍵", Parts { materials: "0000000000000009", ..rr_parts(&none) }),
        ("code の木の鍵", Parts { tree: "0000000000000009", ..rr_parts(&none) }),
        ("basis", Parts { basis: Basis::Forecast, ..rr_parts(&none) }),
        ("祖先の状態の語", rr_parts(&landed)),
        ("lens の版", Parts { version: "lens-version model=sonnet", ..rr_parts(&none) }),
    ];
    for (label, parts) in variants {
        let moved = row_review::judgement(row, &parts);
        assert_ne!(moved.0, base.0, "{label}: 判定の鍵が変わる");
        assert_ne!(moved.1, base.1, "{label}: 記録の dir の名が変わる");
    }
    let declared = vec!["docs/design/toy.md#a:declared".to_owned()];
    assert_ne!(row_review::judgement(row, &rr_parts(&landed)).0, row_review::judgement(row, &rr_parts(&declared)).0, "祖先の状態の語だけが違う 2 形");
}

/// 行の記録 1 つ（§9 の key の列を手で書く・dir の名は口 (H) が返す名）。
struct RrRecord<'a> {
    /// `<doc>#<行 id>`。
    row: &'a str,
    /// 行の digest。
    digest: &'a str,
    /// 判定の字。
    verdict: &'a str,
    /// 理由の型の字（`-` か 7 語）。
    kind: &'a str,
    /// basis。
    basis: Basis,
    /// 祖先ごとの `<行>:<状態>`。
    ancestors: &'a [String],
    /// 材料の鍵・code の木の鍵・lens の版。
    keys: [&'a str; 3],
    /// UTC の秒。
    at: u64,
}

/// 行の記録を置き、dir の名を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rr_record(state: &Path, record: &RrRecord<'_>) -> String {
    let [materials, tree, version] = record.keys;
    let parts = Parts { digest: record.digest, materials, tree, basis: record.basis, ancestors: record.ancestors, version };
    let (key, name) = row_review::judgement(record.row, &parts);
    let (basis, ancestors) = (record.basis.as_str(), row_review::ancestors_word(record.ancestors));
    let (row, digest, verdict, kind, at) = (record.row, record.digest, record.verdict, record.kind, record.at);
    let body = format!(
        "schema=1\nrow={row}\ndigest={digest}\nkey={key}\nbasis={basis}\nancestors={ancestors}\nmech=clean\nverdict={verdict}\nkind={kind}\nmaterials={materials}\ntree={tree}\nversion={version}\nref={}\nat={at}\nusage=-\n",
        rr_sha('a')
    );
    let dir = rr_root(state).join(&name);
    fs::create_dir_all(&dir).expect("記録の dir を作れる");
    fs::write(dir.join("record"), body).expect("記録を書ける");
    name
}

/// 基準の行の記録（PASS・actual・祖先なし）。
fn rr_pass<'a>(ancestors: &'a [String]) -> RrRecord<'a> {
    RrRecord {
        row: "docs/design/toy.md#x",
        digest: "0000000000000001",
        verdict: "PASS",
        kind: "-",
        basis: Basis::Actual,
        ancestors,
        keys: ["0000000000000002", "0000000000000003", "lens-version model=opus"],
        at: 1_000,
    }
}

/// (e) 口 (D): 行と 4 つの鍵の材料が同じで PASS・actual・祖先が全部 landed か祖先なしの記録の dir の名だけを返す。FAIL・forecast・祖先に tree を
/// 持つ記録と、行の digest・材料の鍵・code の木の鍵・lens の版の 1 つだけが違う 4 形は `None`。
#[test]
fn pipe_review_record_reusable_returns_only_a_landed_actual_pass_of_the_same_keys() {
    let row = "docs/design/toy.md#x";
    let (digest, keys) = ("0000000000000001", ["0000000000000002", "0000000000000003", "lens-version model=opus"]);
    let ask = |state: &Path, digest: &str, keys: [&str; 3]| row_review::reusable(state, row, [digest, keys[0], keys[1], keys[2]]);
    let landed = vec!["docs/design/toy.md#a:landed".to_owned(), "docs/design/toy.md#b:landed".to_owned()];
    let none: Vec<String> = Vec::new();
    // 返す 2 形: 祖先なしと、祖先が全部 landed。
    for (label, ancestors) in [("祖先なし", &none), ("祖先が全部 landed", &landed)] {
        let state = rr_state();
        let name = rr_record(&state, &rr_pass(ancestors));
        assert_eq!(ask(&state, digest, keys), Some(name), "{label}: 写せる記録の dir の名");
        clean(&[&state]);
    }
    // 返さない 3 形（記録の側が違う）: FAIL・forecast・祖先に tree。
    let tree = vec!["docs/design/toy.md#a:landed".to_owned(), "docs/design/toy.md#c:tree".to_owned()];
    let declared = vec!["docs/design/toy.md#a:declared".to_owned()];
    let fail = RrRecord { verdict: "FAIL", kind: "other", ..rr_pass(&none) };
    let forecast = RrRecord { basis: Basis::Forecast, ..rr_pass(&declared) };
    let beneath = rr_pass(&tree);
    for (label, record) in [("FAIL", fail), ("forecast", forecast), ("祖先に tree", beneath)] {
        let state = rr_state();
        rr_record(&state, &record);
        assert_eq!(ask(&state, digest, keys), None, "{label}の記録は写さない");
        clean(&[&state]);
    }
    // 返さない 4 形（問いの側が違う）: 4 つの鍵の材料の 1 つだけが違う。
    let state = rr_state();
    rr_record(&state, &rr_pass(&none));
    let asked = [
        ("行の digest", ask(&state, "00000000000000ff", keys)),
        ("材料の鍵", ask(&state, digest, ["00000000000000ff", keys[1], keys[2]])),
        ("code の木の鍵", ask(&state, digest, [keys[0], "00000000000000ff", keys[2]])),
        ("lens の版", ask(&state, digest, [keys[0], keys[1], "lens-version model=sonnet"])),
    ];
    for (label, found) in asked {
        assert_eq!(found, None, "{label}だけが違う問いは None");
    }
    clean(&[&state]);
}

/// 変えた行 `row` を載せた ref の記録の本文（`rows` は (行, digest)）。
fn rr_with_rows(rows: &[(&str, &str)]) -> String {
    let listed: String = rows.iter().map(|(row, digest)| format!("row={row} digest={digest} id=0000000000000009 verdict=PASS basis=actual\n")).collect();
    format!("schema=1\nbase={}\ntables=docs/design/toy.md\n{listed}result=pass\n", rr_sha('b'))
}

/// (f) 口 (E): その行をその digest で載せた ref の記録の他の行を字の順で重複なく返し、同じ行を違う digest で載せた ref の記録の行は返さず、
/// 読めない ref の記録の file（dir・schema が違う）の数を別に返す。
#[test]
fn pipe_review_record_siblings_union_the_refs_that_carry_the_row_at_its_digest() {
    let state = rr_state();
    let (x, y, z, w, q) = ("docs/design/toy.md#x", "docs/design/toy.md#y", "docs/design/toy.md#z", "docs/design/toy.md#w", "docs/design/toy.md#q");
    rr_ref(&state, &rr_sha('1'), &rr_with_rows(&[(x, "d1"), (z, "d3"), (y, "d2")]));
    rr_ref(&state, &rr_sha('2'), &rr_with_rows(&[(x, "d1"), (w, "d9"), (y, "d2")]));
    rr_ref(&state, &rr_sha('3'), &rr_with_rows(&[(x, "dX"), (q, "d4")]));
    rr_ref(&state, &rr_sha('4'), &rr_with_rows(&[(x, "d1")]).replacen("schema=1", "schema=2", 1));
    fs::create_dir_all(rr_root(&state).join("ref").join(rr_sha('5'))).expect("dir を置ける");
    let (found, unreadable) = row_review::siblings(&state, x, "d1");
    assert_eq!(found, [w, y, z], "d1 で x を載せた 2 本の ref の記録の他の行（字の順・y は 1 回）");
    assert_eq!(unreadable, 2, "読めない ref の記録: schema の違う file と dir");
    let (other, _) = row_review::siblings(&state, x, "dX");
    assert_eq!(other, [q], "違う digest で載せた ref の記録の行はその digest の問いにだけ返る");
    clean(&[&state]);
}

/// (g) 口 (F): その行とその digest の行の記録ごとの判定・basis・理由の型・at を at の順に返し、読めない記録は数だけを返す。
#[test]
fn pipe_review_record_listed_returns_each_record_of_the_row_and_counts_the_unreadable() {
    let state = rr_state();
    let none: Vec<String> = Vec::new();
    let declared = vec!["docs/design/toy.md#a:declared".to_owned()];
    rr_record(&state, &RrRecord { at: 200, verdict: "FAIL", kind: "other", basis: Basis::Forecast, ancestors: &declared, ..rr_pass(&none) });
    rr_record(&state, &RrRecord { at: 100, ..rr_pass(&none) });
    rr_record(&state, &RrRecord { digest: "00000000000000ff", ..rr_pass(&none) });
    rr_record(&state, &RrRecord { row: "docs/design/toy.md#other", ..rr_pass(&none) });
    fs::create_dir_all(rr_root(&state).join("0123456789abcdef")).expect("記録の無い dir を置ける");
    let broken = rr_root(&state).join("fedcba9876543210");
    fs::create_dir_all(&broken).expect("dir を置ける");
    fs::write(broken.join("record"), "schema=2\nrow=docs/design/toy.md#x\n").expect("schema の違う記録を書ける");
    let (found, unreadable) = row_review::listed(&state, "docs/design/toy.md#x", "0000000000000001");
    let want = [
        Listed { verdict: Verdict::Pass, basis: Basis::Actual, kind: None, at: 100 },
        Listed { verdict: Verdict::Fail, basis: Basis::Forecast, kind: Some(FindingKind::Other), at: 200 },
    ];
    assert_eq!(found, want, "行と digest が同じ 2 本（at の順）");
    assert_eq!(unreadable, 2, "読めない記録は数だけ: file の無い dir と schema の違う記録");
    clean(&[&state]);
}

// ───── 行の審査の口（設計 docs/design/row-review.md §3・§9・行 a・接頭辞 `pipe_review_ref_`） ─────
//
// `pipe review --ref SHA` を実 binary で撃つ。偽の `bd`（PATH の先頭）と偽の lens（起動のたびに log へ 1 行・版の flag には版の 1 行を返す）で、
// 変わった行・祖先の層・機械の検査・使い回し・結果の語・記録の形を外形から測る。設計の PR の commit は anchor の repo から切った別の木で作る
// （anchor の作業の木は main のまま置く）。口 (A)〜(F) は書いた記録を直に読んで測る（書き手と読み手の形の一致）。

/// 行の審査の 1 つの置き場。
struct Rv {
    /// anchor の repo（作業の木は main）。
    repo: PathBuf,
    /// 置き場。
    state: PathBuf,
    /// PATH（偽の `bd` を先頭に積む）。
    path: String,
    /// `--lens` の cmd。
    lens: String,
    /// `--rules` の写し。
    rules: String,
    /// main の sha（`origin/main` の先端）。
    main: String,
}

/// 行 `id`（節 `section`）の欄。`over` の欄（key と TOML の値の字面）は行の既定を置き換える。
fn rv_row(id: &str, section: u32, over: &[(&str, &str)]) -> Vec<String> {
    let mut drop: Vec<&str> = over.iter().map(|(key, _)| *key).collect();
    drop.push("section");
    let mut add = vec![format!("section = \"{section}\"")];
    add.extend(over.iter().map(|(key, value)| format!("{key} = {value}")));
    row_fields(id, &drop, &add.iter().map(String::as_str).collect::<Vec<&str>>())
}

/// 設計 doc（節 n の本文は `bodies[n-1]`・行は区間の中）。
fn rv_doc(bodies: &[&str], rows: &[Vec<String>]) -> String {
    let sections: String = bodies.iter().enumerate().map(|(n, body)| format!("## {}. 節 {}\n\n{body}\n\n", n + 1, n + 1)).collect();
    let listed: Vec<String> = rows.iter().map(|fields| format!("[[contract]]\n{}", fields.join("\n"))).collect();
    let (begin, end) = (vessel::pipe::table::BEGIN, vessel::pipe::table::END);
    format!("# 設計: toy\n\n{sections}{begin}\nschema = 1\n\n{}\n{end}\n", listed.join("\n\n"))
}

/// 実行権つきの `/bin/sh` script を書き、その path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rv_script(path: &Path, body: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, format!("#!/bin/sh\n{body}")).expect("script を書ける");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("script に実行権を付ける");
    path.display().to_string()
}

/// 偽 lens の script（`--print-version` の周は版の撃ちの log `rv-vlog` に 1 行を足し、版の 1 行〔`rv-version` の中身・無ければ既定〕を返し、
/// `rv-version-rc` が在ればその rc で終わる）。
/// それ以外の周は log `rv-log` に 1 行（行・cwd・HEAD・審査の木の file の有無・撃ち中の受付札）を足し、`rv-out.<行 id>` か `rv-out` の中身
/// （無ければ PASS）を返す。
fn rv_lens_script(state: &Path) -> String {
    let (s, slots) = (state.display(), vessel::seat::host_slots_dir(state).display().to_string());
    let body = format!(
        r#"for a in "$@"; do
  if [ "$a" = "--print-version" ]; then
    echo v >> '{s}/rv-vlog'
    if [ -f '{s}/rv-version-rc' ]; then exit "$(cat '{s}/rv-version-rc')"; fi
    if [ -f '{s}/rv-version' ]; then cat '{s}/rv-version'; else echo 'lens-version fake=1'; fi
    exit 0
  fi
done
cat >/dev/null
row=$(sed -n 's/^design = "\(.*\)"$/\1/p' "$1")
id=${{row##*#}}
fresh=no; [ -e src/fresh.rs ] && fresh=yes
added=no; [ -e src/added.rs ] && added=yes
printf 'row=%s cwd=%s head=%s fresh=%s added=%s slots=%s\n' "$row" "$(pwd -P)" "$(git rev-parse HEAD)" "$fresh" "$added" "$(cat '{slots}'/*.slot 2>/dev/null | tr '\n' ' ')" >> '{s}/rv-log'
if [ -f '{s}/rv-out.'"$id" ]; then cat '{s}/rv-out.'"$id"; elif [ -f '{s}/rv-out' ]; then cat '{s}/rv-out'; else echo '{{"verdict":"PASS","evidence":"fake"}}'; fi
"#
    );
    format!("{} {{contract}} {{worktree}}", rv_script(&state.join("rv-lens.sh"), &body))
}

/// 口が読む manifest（受付の上限の写しに台帳の待ち上限の行を足す）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rv_rules(state: &Path) -> String {
    let base = fs::read_to_string(ceiling_rules(state)).expect("受付の写しを読める");
    let row = "[[rule]]\nid = \"seat.ledger_timeout_s\"\nkind = \"LedgerTimeoutS\"\nvalue = 60\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n";
    let path = state.join("rules-rv.toml");
    fs::write(&path, format!("{base}\n{row}")).expect("rules の写しを書ける");
    path.display().to_string()
}

/// 偽の `bd`（PATH の先頭に置く 1 本）が返す台帳を書き換える（`rc` が `Some` の周は台帳を返さずその rc で終わる）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rv_ledger(place: &Rv, issues: &[String], rc: Option<u8>) {
    let json = place.state.join("rv-ledger.json");
    fs::write(&json, format!("[{}]\n", issues.join(","))).expect("偽の台帳を書ける");
    let body = rc.map_or_else(|| format!("cat '{}'\n", json.display()), |code| format!("exit {code}\n"));
    rv_script(&place.state.join("rv-bin").join("bd"), &body);
}

/// 行の審査の置き場: main に設計 doc と file を commit し、`origin/main` を main の先端に置く（偽の台帳は空）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rv_place(bodies: &[&str], rows: &[Vec<String>], files: &[(&str, String)]) -> Rv {
    let (repo, state) = repo_with_state();
    write_design(&repo, &rv_doc(bodies, rows));
    let mut all = vec![(".vessel.toml", PRELENS_VESSEL.to_owned())];
    all.extend(files.iter().cloned());
    for (path, body) in &all {
        let target = repo.join(path);
        fs::create_dir_all(target.parent().expect("親 dir が在る")).expect("dir を作れる");
        fs::write(&target, body).expect("file を書ける");
    }
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "rv-main"]);
    let main = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", &main]);
    fs::create_dir_all(state.join("rv-bin")).expect("偽 bd の dir を作れる");
    let (lens, rules) = (rv_lens_script(&state), rv_rules(&state));
    let path = format!("{}:{}", state.join("rv-bin").display(), crate::toolbox_path(&state));
    let place = Rv { repo, state, path, lens, rules, main };
    rv_ledger(&place, &[], None);
    place
}

/// 設計の PR の commit を作る（anchor の repo から切った detach の別の木で `edits` を書いて commit し、木を外す・空の commit も作る）。
/// `parent` が `None` なら main の上。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rv_commit(place: &Rv, parent: Option<&str>, edits: &[(&str, String)]) -> String {
    let tree = tmp().join("rv-tree");
    let text = tree.display().to_string();
    git(&place.repo, &["worktree", "add", "-q", "--detach", &text, parent.unwrap_or(&place.main)]);
    for (path, body) in edits {
        let target = tree.join(path);
        fs::create_dir_all(target.parent().expect("親 dir が在る")).expect("dir を作れる");
        fs::write(&target, body).expect("file を書ける");
    }
    git(&tree, &["add", "-A"]);
    git(&tree, &["commit", "-q", "--allow-empty", "-m", "rv-ref"]);
    let sha = git(&tree, &["rev-parse", "HEAD"]);
    git(&place.repo, &["worktree", "remove", "--force", &text]);
    sha
}

/// `pipe review --ref <sha>` を撃つ。
fn rv_review(place: &Rv, sha: &str) -> Output {
    run_pipe_with_path(
        &place.path,
        &[
            "review", "--ref", sha, "--repo", &place.repo.display().to_string(), "--state-dir", &place.state.display().to_string(),
            "--lens", &place.lens, "--rules", &place.rules,
        ],
    )
}

/// 偽 lens の log（起動ごとに 1 行）。
fn rv_log(place: &Rv) -> Vec<String> {
    fs::read_to_string(place.state.join("rv-log")).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// log の行の `key=` の値（`slots` は行の残り全部・他は次の空白まで）。
fn rv_field(line: &str, key: &str) -> String {
    let rest = line.split_once(&format!("{key}=")).map_or("", |(_, rest)| rest);
    if key == "slots" { rest.to_owned() } else { rest.split(' ').next().unwrap_or_default().to_owned() }
}

/// 偽 lens が撃たれた行 id（字の順・重複なし）。
fn rv_fired(place: &Rv) -> Vec<String> {
    let ids: BTreeSet<String> = rv_log(place).iter().map(|line| rv_field(line, "row").rsplit('#').next().unwrap_or_default().to_owned()).collect();
    ids.into_iter().collect()
}

/// ref の記録の全文（無ければ空）。
fn rv_ref_text(place: &Rv, sha: &str) -> String {
    fs::read_to_string(rr_root(&place.state).join("ref").join(sha)).unwrap_or_default()
}

/// `key=value` の語の対（空白で割る）。
fn rv_words(line: &str) -> std::collections::BTreeMap<String, String> {
    line.split(' ').filter_map(|word| word.split_once('=')).map(|(key, value)| (key.to_owned(), value.to_owned())).collect()
}

/// ref の記録の row の行（`row` の語が `#<id>` で終わる行）の語の対。
fn rv_row_line(text: &str, id: &str) -> std::collections::BTreeMap<String, String> {
    let found = text.lines().filter(|line| line.starts_with("row=")).map(rv_words).find(|words| words.get("row").is_some_and(|row| row.ends_with(&format!("#{id}"))));
    found.unwrap_or_default()
}

/// ref の記録の row の行の id（`#` の後ろ・記録の順）。
fn rv_row_ids(text: &str) -> Vec<String> {
    text.lines().filter(|line| line.starts_with("row=")).filter_map(|line| Some(rv_words(line).get("row")?.rsplit('#').next()?.to_owned())).collect()
}

/// 行の記録（`<根>/<名>/record`・1 行目 schema の後ろの `key=value`）。
fn rv_record(place: &Rv, name: &str) -> std::collections::BTreeMap<String, String> {
    let text = fs::read_to_string(rr_root(&place.state).join(name).join("record")).unwrap_or_default();
    text.lines().skip(1).filter_map(|line| line.split_once('=')).map(|(key, value)| (key.to_owned(), value.to_owned())).collect()
}

/// stdout の `[ROW-REVIEW]` の行。
fn rv_lines(out: &Output) -> Vec<String> {
    stdout_of(out).lines().map(str::to_owned).collect()
}

/// (a) help の頁と usage が `review` を 1 行ずつ載せ、args が許す flag は `--ref` `--repo` `--state-dir` `--lens` `--rules` の 5 つだけ
/// （`--bd` など 5 つの外は未知の引数で断る）。
#[test]
fn pipe_review_ref_help_and_usage_name_the_subcommand_and_five_flags() {
    let page = stdout_of(&bin_cmd().args(["help", "pipe"]).output().expect("help を撃てる"));
    let line = page.lines().find(|line| line.trim_start().starts_with("review ")).unwrap_or_default();
    assert!(line.contains("design PR"), "help の頁の review の 1 行: {page}");
    let usage = vessel::pipe::cli::usage();
    assert!(usage.contains("pipe review --ref SHA --repo R --state-dir S --lens CMD [--rules PATH]"), "usage: {usage}");
    let dir = tmp().display().to_string();
    let args = ["--ref", "x", "--repo", &dir, "--state-dir", &dir, "--lens", "x", "--rules", "/nonexistent"];
    let known = run_pipe(&[&["review"][..], &args[..]].concat());
    assert!(!stderr_of(&known).contains("未知の引数"), "5 つの flag は閉包を通る: {}", stderr_of(&known));
    for outside in ["--bd", "--runner", "--run", "--curl"] {
        let out = run_pipe(&[&["review"][..], &args[..], &[outside, "x"][..]].concat());
        assert!(stderr_of(&out).contains(&format!("未知の引数 {outside}")), "{outside} は閉包の外で名指して断る: {}", stderr_of(&out));
    }
}

/// (b) 変わった行: 契約の欄を変えた行 b・節の本文だけを変えた行 c・新しい行 f・bead の無い変えた行 e は撃たれ、変わらない行 a と指す bead が
/// 全部 closed の行 d は撃たれない（偽 lens の呼び出しの行 id の集合と母集団の行数・ref の記録の row の行が同じ 4 本）。全部 PASS で rc 0。
#[test]
fn pipe_review_ref_fires_only_the_rows_whose_digest_changed() {
    let bodies = ["本文 1", "本文 2", "本文 3", "本文 4", "本文 5"];
    let base = [rv_row("a", 1, &[]), rv_row("b", 2, &[]), rv_row("c", 3, &[]), rv_row("d", 4, &[]), rv_row("e", 5, &[])];
    let place = rv_place(&bodies, &base, &[]);
    let issues = [("s2-rv.1", "open", "a"), ("s2-rv.2", "open", "b"), ("s2-rv.3", "open", "c"), ("s2-rv.4", "closed", "d")];
    rv_ledger(&place, &issues.map(|(id, status, row)| prelens_issue(id, status, row, &[])), None);
    let new_done = ("done", "\"done を変えた\"");
    let rows = [rv_row("a", 1, &[]), rv_row("b", 2, &[new_done]), rv_row("c", 3, &[]), rv_row("d", 4, &[new_done]), rv_row("e", 5, &[new_done]), rv_row("f", 1, &[])];
    let edited = ["本文 1", "本文 2", "本文 3 を変えた", "本文 4", "本文 5"];
    let sha = rv_commit(&place, None, &[(DESIGN_FILE, rv_doc(&edited, &rows))]);
    let out = rv_review(&place, &sha);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "全部 PASS は rc 0: {} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(rv_fired(&place), ["b", "c", "e", "f"], "撃たれた行 id（a は不変・d は着地済み）");
    assert_eq!(rv_log(&place).len(), 4, "母集団は 4 行（偽 lens の呼び出しの数）");
    assert_eq!(rv_row_ids(&rv_ref_text(&place, &sha)), ["b", "c", "e", "f"], "ref の記録の row の行も同じ 4 本");
    clean(&[&place.repo, &place.state]);
}

/// 歯の置き場を持つ検証行（nextest の filter 語 `pre_<x>_`・歯の file は `crates/toy/tests/<x>.rs`）。
fn rv_tooth_row(id: &str) -> Vec<String> {
    let verify = format!("[\"cargo nextest run -p toy --no-tests=fail pre_{id}_\"]");
    row_fields(id, &["write-set", "verify"], &[&format!("verify = {verify}")])
}

/// 歯の file（`#[test]` の直下の fn の名が `pre_<x>_` を含む）。
fn rv_tooth_file(name: &str) -> String {
    format!("#[test]\nfn pre_{name}_one() {{}}\n")
}

/// (b)・code の file だけを変えた commit（表と節は main と字で同じ）: verify の接頭辞に当たる歯を別の file に足して歯の置き場の導出が変わる行 p は
/// 撃たれ、導出が変わらない行 q は撃たれない。受付の生成が断る行（要件面に無い id）は表と節と断りの字が main と同じなら撃たれず、表のその行を
/// 変えると（断りの字が変わると）撃たれる。
#[test]
fn pipe_review_ref_code_only_commit_fires_the_rows_whose_derivation_moved_and_a_refusal_follows_its_text() {
    let bodies = ["本文 1"];
    let base = [rv_tooth_row("p"), rv_tooth_row("q"), rv_row("g", 1, &[("req", "[\"FR99\"]")])];
    let files = [("crates/toy/tests/p.rs", rv_tooth_file("p")), ("crates/toy/tests/q.rs", rv_tooth_file("q"))];
    let place = rv_place(&bodies, &base, &files);
    let table = rv_doc(&bodies, &base);
    let moved = rv_commit(&place, None, &[("crates/toy/tests/p2.rs", rv_tooth_file("p"))]);
    assert_eq!(git(&place.repo, &["show", &format!("{moved}:{DESIGN_FILE}")]), table.trim_end(), "表と節は main と字で同じ");
    let out = rv_review(&place, &moved);
    assert!(out.status.code().is_some_and(|rc| rc == i32::from(RC_OK) || rc == i32::from(RC_REFUSED)), "{} / {}", stdout_of(&out), stderr_of(&out));
    let text = rv_ref_text(&place, &moved);
    assert_eq!(rv_row_ids(&text), ["p"], "歯の置き場の導出が変わった行だけ（q は不変・g は断りの字が同じ）: {text}");
    let edited = [base[0].clone(), base[1].clone(), rv_row("g", 1, &[("req", "[\"FR98\"]")])];
    let changed = rv_commit(&place, None, &[(DESIGN_FILE, rv_doc(&bodies, &edited))]);
    rv_review(&place, &changed);
    assert_eq!(rv_row_ids(&rv_ref_text(&place, &changed)), ["g"], "断りの字が変わった行は撃たれる");
    clean(&[&place.repo, &place.state]);
}

/// (c) 変わった行が 0 本の sha（空の commit）は偽 lens 0 回で rc 0・ref の記録は row の行を持たず result=pass で、stdout は result の 1 行だけ
/// （行ごとの行も notify の行も無い）。
#[test]
fn pipe_review_ref_without_changed_rows_writes_a_pass_record_and_fires_nothing() {
    let place = rv_place(&["本文 1"], &[rv_row("a", 1, &[])], &[]);
    let sha = rv_commit(&place, None, &[]);
    let out = rv_review(&place, &sha);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(rv_lines(&out), [format!("[ROW-REVIEW] result=pass ref={sha}")], "result の 1 行だけ");
    assert!(rv_log(&place).is_empty(), "偽 lens は 0 回");
    let want = format!("schema=1\nbase={}\ntables={DESIGN_FILE}\nresult=pass\n", place.main);
    assert_eq!(rv_ref_text(&place, &sha), want, "row の行を持たない ref の記録");
    clean(&[&place.repo, &place.state]);
}

/// (d) 偽の `bd` が rc 1 の周は偽 lens 0 回・rc 2 で台帳を読めない理由を名指し、ref の記録に result の行を書かず（口 (A) は missing）、撃ち中の印も残らない。
#[test]
fn pipe_review_ref_unreadable_ledger_is_rc_two_and_fires_nothing() {
    let place = rv_place(&["本文 1"], &[rv_row("a", 1, &[])], &[]);
    rv_ledger(&place, &[], Some(1));
    let sha = rv_commit(&place, None, &[(DESIGN_FILE, rv_doc(&["本文 1"], &[rv_row("a", 1, &[("done", "\"変えた\"")])]))]);
    let out = rv_review(&place, &sha);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stderr_of(&out).contains("台帳を読めない"), "理由を名指す: {}", stderr_of(&out));
    assert!(rv_log(&place).is_empty(), "偽 lens は 0 回");
    assert!(!rv_ref_text(&place, &sha).contains("result="), "result の行を書かない");
    assert!(matches!(row_review::read_ref(&place.state, &sha), RefResult::Missing), "口 (A) は missing（印も残らない）");
    clean(&[&place.repo, &place.state]);
}

/// Reviewed の材料の file 名（材料の dir はこの外の file を持たない・§9）。
const RV_MATERIAL_FILES: [&str; 7] = ["contract.toml", "design.txt", "requirements.txt", "base.txt", "promises.txt", "outside.txt", "items.txt"];

/// stdout の row の行（`<行 id>` の行）の語の対（`verdict` `basis` `record`）。
fn rv_row_out(out: &Output, id: &str) -> std::collections::BTreeMap<String, String> {
    let found = rv_lines(out).into_iter().filter(|line| line.starts_with("[ROW-REVIEW] row=")).map(|line| rv_words(&line)).find(|words| words.get("row").is_some_and(|row| row.ends_with(&format!("#{id}"))));
    found.unwrap_or_default()
}

/// 行の記録の dir の材料の file（`review/` の中・名の順）。
fn rv_material_names(place: &Rv, name: &str) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(rr_root(&place.state).join(name).join("review")).map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    found.sort();
    found
}

/// 行の記録の dir の design.txt。
fn rv_design(place: &Rv, name: &str) -> String {
    fs::read_to_string(rr_root(&place.state).join(name).join("review").join("design.txt")).unwrap_or_default()
}

/// (e) forecast: anchor の HEAD の表に行 x も祖先 y も無く（`--ref` の commit だけが足した行）、表の depends だけで未着地の祖先 y に繋がる行 x は
/// basis forecast（y は祖先なしの partial）。x の材料の dir は Reviewed の file 名の外の file を持たず、design.txt の末尾に祖先 y の行の TOML の写しと
/// 節の本文を持つ。祖先の `+` の file は審査の木に無い。
#[test]
fn pipe_review_ref_forecast_carries_the_declared_ancestor_in_the_design_material() {
    let bodies = ["本文 1", "本文 2"];
    let place = rv_place(&bodies, &[rv_row("m", 1, &[])], &[]);
    let y = rv_row("y", 2, &[("write-set", r#"["+src/fresh.rs"]"#)]);
    let x = rv_row("x", 1, &[("write-set", r#"["src/fresh.rs"]"#), ("depends", r#"["y"]"#)]);
    let sha = rv_commit(&place, None, &[(DESIGN_FILE, rv_doc(&bodies, &[rv_row("m", 1, &[]), y.clone(), x]))]);
    let head = git(&place.repo, &["show", &format!("HEAD:{DESIGN_FILE}")]);
    assert!(!head.contains("id = \"x\"") && !head.contains("id = \"y\""), "前提: anchor の HEAD の契約表に行 x も y も無い");
    assert_eq!(git(&place.repo, &["rev-parse", "HEAD"]), place.main, "anchor の作業の木は main のまま");
    let out = rv_review(&place, &sha);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    let (rx, ry) = (rv_row_out(&out, "x"), rv_row_out(&out, "y"));
    assert_eq!((rx.get("basis").map(String::as_str), ry.get("basis").map(String::as_str)), (Some("forecast"), Some("partial")), "{}", stdout_of(&out));
    let name = rx.get("record").cloned().unwrap_or_default();
    let files = rv_material_names(&place, &name);
    assert!(!files.is_empty() && files.iter().all(|file| RV_MATERIAL_FILES.contains(&file.as_str())), "材料の dir の file 名: {files:?}");
    let want = format!("{}\n[[contract]]\n{}\n{DESIGN_FILE}#y §2\n\n本文 2\n", "次の行は未着地の祖先で、その write-set の file はこの祖先が作る・変える", y.join("\n"));
    assert!(rv_design(&place, &name).ends_with(&want), "design.txt の末尾に祖先の行の写しと節の本文: {}", rv_design(&place, &name));
    let seen = rv_log(&place).into_iter().find(|line| rv_field(line, "row").ends_with("#x")).unwrap_or_default();
    assert_eq!((rv_field(&seen, "fresh"), rv_field(&seen, "head")), ("no".to_owned(), sha.clone()), "祖先の + の file は審査の木に無い: {seen}");
    assert_eq!(rv_record(&place, &name).get("ancestors").map(String::as_str), Some("docs/design/toy.md#y:declared"));
    clean(&[&place.repo, &place.state]);
}

/// 本文を畳む歯の 1 周（§14）: main は行 m だけの doc、`--ref` の commit が行 `rows` を足す。口は rc 0・前提は HEAD が main のまま・HEAD の表に足した行が無い・
/// `--ref` の doc が各印の字を数えどおり持つ・行 `last` の basis が forecast・記録の ancestors が `declared` を declared で持つ。返りは行 `last` の design.txt。
fn ancestor_body_once_case(bodies: &[&str], rows: &[Vec<String>], last: &str, marks: &[(&str, usize)], declared: &[&str]) -> String {
    let place = rv_place(bodies, &[rv_row("m", 1, &[])], &[]);
    let doc = rv_doc(bodies, &[vec![rv_row("m", 1, &[])], rows.to_vec()].concat());
    let sha = rv_commit(&place, None, &[(DESIGN_FILE, doc.clone())]);
    let head = git(&place.repo, &["show", &format!("HEAD:{DESIGN_FILE}")]);
    assert!(rows.iter().filter_map(|fields| fields.first()).all(|id_line| !head.contains(id_line.as_str())), "前提: HEAD の契約表に足した行が無い");
    assert_eq!(git(&place.repo, &["rev-parse", "HEAD"]), place.main, "anchor の作業の木は main のまま");
    for (mark, count) in marks {
        assert_eq!(doc.matches(mark).count(), *count, "前提: --ref の doc が {mark} を {count} 回持つ");
    }
    let out = rv_review(&place, &sha);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    let row = rv_row_out(&out, last);
    assert_eq!(row.get("basis").map(String::as_str), Some("forecast"), "前提: 行 {last} の basis: {}", stdout_of(&out));
    let name = row.get("record").cloned().unwrap_or_default();
    let want = declared.iter().map(|id| format!("{DESIGN_FILE}#{id}:declared")).collect::<Vec<_>>().join(",");
    assert_eq!(rv_record(&place, &name).get("ancestors").map(String::as_str), Some(want.as_str()), "前提: 記録の ancestors");
    let design = rv_design(&place, &name);
    clean(&[&place.repo, &place.state]);
    design
}

/// design.txt のうち、見出しの行 `head` の直後の行。
fn ancestor_body_once_after(design: &str, head: &str) -> String {
    let lines: Vec<&str> = design.lines().collect();
    lines.iter().position(|line| *line == head).and_then(|at| lines.get(at + 1)).map(|line| (*line).to_owned()).unwrap_or_default()
}

/// 畳んだ本文の 1 行（設計 §14 の字）。
const ANCESTOR_BODY_SAME: &str = "（節の本文は上と同じ）";

/// (a) §14: 節 1 つの doc で行 a・b・c（c の depends が b・b の depends が a）を足した口の行 c の design.txt は、節の本文の印の字を 1 回・未着地の祖先の頭の
/// 1 行を 2 回・行 a と b の TOML の写しの id の行を 1 回ずつ持ち、見出しの行 b §1 と a §1 の直後の行がどちらも「（節の本文は上と同じ）」。
#[test]
fn ancestor_body_once_same_section_chain_folds_each_ancestor_body_into_one_line() {
    let rows = [rv_row("a", 1, &[]), rv_row("b", 1, &[("depends", r#"["a"]"#)]), rv_row("c", 1, &[("depends", r#"["b"]"#)])];
    let design = ancestor_body_once_case(&["ZZMARK-ONE"], &rows, "c", &[("ZZMARK-ONE", 1)], &["a", "b"]);
    assert_eq!(design.matches("ZZMARK-ONE").count(), 1, "印の字は 1 回: {design}");
    let note = "次の行は未着地の祖先で、その write-set の file はこの祖先が作る・変える";
    assert_eq!(design.matches(note).count(), 2, "祖先の頭の 1 行は 2 回: {design}");
    for id in ["a", "b"] {
        assert_eq!(design.lines().filter(|line| *line == format!("id = \"{id}\"")).count(), 1, "行 {id} の TOML の写し: {design}");
        let after = ancestor_body_once_after(&design, &format!("{DESIGN_FILE}#{id} §1"));
        assert_eq!(after, ANCESTOR_BODY_SAME, "見出しの行 {id} の直後: {design}");
    }
}

/// (b) §14: 節 2 つ（印は別の字）の doc で節 1 の行 a・b（b の depends が a）と節 2 の行 d（depends が b）を足した口の行 d の design.txt は、節 1 の印を 1 回・
/// 節 2 の印を 1 回・「（節の本文は上と同じ）」の行を 1 回持つ。
#[test]
fn ancestor_body_once_different_section_body_is_written_once_and_the_second_same_one_folds() {
    let rows = [rv_row("a", 1, &[]), rv_row("b", 1, &[("depends", r#"["a"]"#)]), rv_row("d", 2, &[("depends", r#"["b"]"#)])];
    let design = ancestor_body_once_case(&["ZZMARK-ONE", "ZZMARK-TWO"], &rows, "d", &[("ZZMARK-ONE", 1), ("ZZMARK-TWO", 1)], &["a", "b"]);
    assert_eq!(design.matches("ZZMARK-ONE").count(), 1, "節 1 の印: {design}");
    assert_eq!(design.matches("ZZMARK-TWO").count(), 1, "節 2 の印: {design}");
    assert_eq!(design.lines().filter(|line| *line == ANCESTOR_BODY_SAME).count(), 1, "上と同じの行: {design}");
}

/// (c) §14: 節 1 と節 2 の本文が同じ印の字の doc で節 1 の行 a と節 2 の行 e（depends が a）を足した口の行 e の design.txt は、印の字を 1 回持ち、
/// 見出しの行 a §1 の直後の行が「（節の本文は上と同じ）」（節の番号では比べない）。
#[test]
fn ancestor_body_once_compares_the_body_bytes_not_the_section() {
    let rows = [rv_row("a", 1, &[]), rv_row("e", 2, &[("depends", r#"["a"]"#)])];
    let design = ancestor_body_once_case(&["ZZMARK-ONE", "ZZMARK-ONE"], &rows, "e", &[("ZZMARK-ONE", 2)], &["a"]);
    assert_eq!(design.matches("ZZMARK-ONE").count(), 1, "印の字は 1 回: {design}");
    assert_eq!(ancestor_body_once_after(&design, &format!("{DESIGN_FILE}#a §1")), ANCESTOR_BODY_SAME, "見出しの行 a の直後: {design}");
}

/// 祖先 y が Gated PASS の便を持つ置き場: anchor の main に行 y（`+src/added.rs`）と行 x（表の depends が y）・台帳は y と x の bead・y の便は
/// Gated PASS（worktree は anchor の便の worktree の置き場）。行 x の done を変えた設計の PR の commit まで作る。返りは (置き場, y の便の id, commit)。
fn rv_actual() -> (Rv, String, String) {
    let bodies = ["本文 1", "本文 2"];
    let base = [rv_row("y", 1, &[("write-set", r#"["+src/added.rs"]"#)]), rv_row("x", 2, &[("depends", r#"["y"]"#)])];
    let place = rv_place(&bodies, &base, &[]);
    let id = intake_bead(&place.repo, &place.state, &format!("{DESIGN_FILE}#y"), "s2-rv.1");
    let runner = "echo 'pub fn added() {}' > src/added.rs && git add -A && git commit -q -m runner";
    let spawned = spawn_with(&place.repo, &place.state, &id, runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));
    let gated = gate_once(&place.repo, &place.state, &id, Some(&fake_lens(&place.state.join("gate-lens"), &lens_verdict("PASS"))));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate は PASS: {}", stderr_of(&gated));
    rv_ledger(&place, &[prelens_issue("s2-rv.1", "open", "y", &[]), prelens_issue("s2-rv.2", "open", "x", &["s2-rv.1"])], None);
    let rows = [base[0].clone(), rv_row("x", 2, &[("depends", r#"["y"]"#), ("done", "\"x を変えた\"")])];
    let sha = rv_commit(&place, None, &[(DESIGN_FILE, rv_doc(&bodies, &rows))]);
    (place, id, sha)
}

/// (e) actual: Gated PASS の便を持つ祖先 y の add の file は審査の木に写り（偽 lens が審査の木で見る）、bead を持つ行 x は basis actual・
/// 祖先の状態の語は tree。
#[test]
fn pipe_review_ref_gated_ancestor_copies_its_added_file_and_the_row_is_actual() {
    let (place, _, sha) = rv_actual();
    let out = rv_review(&place, &sha);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    let row = rv_row_out(&out, "x");
    assert_eq!(row.get("basis").map(String::as_str), Some("actual"), "{}", stdout_of(&out));
    let seen = rv_log(&place).into_iter().next().unwrap_or_default();
    assert_eq!((rv_field(&seen, "added"), rv_field(&seen, "head")), ("yes".to_owned(), sha), "実物の祖先の add の file が審査の木に在る: {seen}");
    let record = rv_record(&place, row.get("record").map_or("", String::as_str));
    assert_eq!(record.get("ancestors").map(String::as_str), Some("docs/design/toy.md#y:tree"));
    clean(&[&place.repo, &place.state]);
}

/// (p) 祖先の層を組めない行（Gated PASS の便の worktree を消した）が在る周は、偽 lens 0 回・rc 2 でその祖先の行と理由を名指し、撃ち中の印を外して
/// ref の記録に result の行を書かない（口 (A) は missing）。
#[test]
fn pipe_review_ref_unbuildable_ancestor_layer_is_rc_two_naming_the_ancestor() {
    let (place, id, sha) = rv_actual();
    git(&place.repo, &["worktree", "remove", "--force", &vessel::pipe::worktree_path(&place.repo, &id).display().to_string()]);
    let out = rv_review(&place, &sha);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{} / {}", stdout_of(&out), stderr_of(&out));
    let err = stderr_of(&out);
    assert!(err.contains(&format!("{DESIGN_FILE}#x")) && err.contains(&format!("{DESIGN_FILE}#y")) && err.contains("層を決められない"), "行と祖先と理由を名指す: {err}");
    assert!(rv_log(&place).is_empty(), "偽 lens は 0 回");
    assert!(!rv_ref_text(&place, &sha).contains("result="), "result の行を書かない");
    assert!(matches!(row_review::read_ref(&place.state, &sha), RefResult::Missing), "口 (A) は missing（印も残らない）");
    clean(&[&place.repo, &place.state]);
}

/// (f) 機械の検査は祖先の層を重ねた予想の上で撃つ: 祖先 a の `+src/fresh.rs` を素で持ち depends を持たない行 b は偽 lens 0 回で verdict FAIL・
/// mech が firm:write-set-item-unresolved・kind が -。同じ write-set で表の depends だけで a に繋がる行 c は firm にならず偽 lens が 1 回。祖先 e の
/// write-set と交わる file の上限の余地が足りない行 d は firm にならず偽 lens が 1 回撃たれ、design.txt の末尾が暫定の finding の名 cap-headroom と在り処を持つ。
#[test]
fn pipe_review_ref_mechanical_check_runs_on_the_ancestors_layers_and_hands_on_provisional_findings() {
    let full = "x\n".repeat(usize::try_from(embedded_int("R-C4-2")).unwrap_or_default());
    let place = rv_place(&["本文 1"], &[rv_row("m", 1, &[])], &[("crates/toy/src/big.rs", full)]);
    let (fresh, big) = (r#"["src/fresh.rs"]"#, r#"["crates/toy/src/big.rs"]"#);
    let rows = [
        rv_row("m", 1, &[]),
        rv_row("a", 1, &[("write-set", r#"["+src/fresh.rs"]"#)]),
        rv_row("b", 1, &[("write-set", fresh)]),
        rv_row("c", 1, &[("write-set", fresh), ("depends", r#"["a"]"#)]),
        rv_row("e", 1, &[("write-set", big)]),
        rv_row("d", 1, &[("write-set", big), ("depends", r#"["e"]"#)]),
    ];
    let sha = rv_commit(&place, None, &[(DESIGN_FILE, rv_doc(&["本文 1"], &rows))]);
    let out = rv_review(&place, &sha);
    assert_eq!(rv_fired(&place), ["a", "c", "d"], "確定の機械の検査を持つ行 b と e は偽 lens を撃たない: {} / {}", stdout_of(&out), stderr_of(&out));
    let firm = rv_row_out(&out, "b");
    assert_eq!(firm.get("verdict").map(String::as_str), Some("FAIL"), "{}", stdout_of(&out));
    let record = rv_record(&place, firm.get("record").map_or("", String::as_str));
    assert_eq!((record.get("mech").map(String::as_str), record.get("kind").map(String::as_str)), (Some("firm:write-set-item-unresolved"), Some("-")));
    let (c, d) = (rv_row_out(&out, "c"), rv_row_out(&out, "d"));
    assert_eq!((c.get("basis").map(String::as_str), d.get("basis").map(String::as_str)), (Some("forecast"), Some("forecast")), "{}", stdout_of(&out));
    let design = rv_design(&place, d.get("record").map_or("", String::as_str));
    assert!(design.lines().any(|line| line.contains("cap-headroom") && line.contains("files:crates/toy/src/big.rs")), "暫定の finding の名と在り処: {design}");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の行が在る周は fail");
    clean(&[&place.repo, &place.state]);
}

/// 行 a（bead 無し・節 1）を 1 本だけ変えた設計の PR の置き場と commit（偽 lens の出力は呼び手が [`rv_out`] で選ぶ）。
fn rv_one() -> (Rv, String) {
    let place = rv_place(&["本文 1"], &[rv_row("a", 1, &[])], &[]);
    let sha = rv_commit(&place, None, &[(DESIGN_FILE, rv_doc(&["本文 1"], &[rv_row("a", 1, &[("done", "\"変えた\"")])]))]);
    (place, sha)
}

/// 偽 lens の出力を選ぶ（`id` が空なら全行・でなければその行 id だけ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rv_out(place: &Rv, id: &str, body: &str) {
    let name = if id.is_empty() { "rv-out".to_owned() } else { format!("rv-out.{id}") };
    fs::write(place.state.join(name), format!("{body}\n")).expect("偽 lens の出力を書ける");
}

/// (g) 同じ sha の 2 回目の口は偽 lens 0 回で同じ記録の dir を名指し、要件の本文を 1 行変えた commit・code の file を 1 つ変えた commit・偽 lens の版の行を
/// 変えた回の 2 回目はそれぞれ撃つ（撃たない 1 + 撃つ 3・撃った回の記録の dir の名は互いに違う）。
#[test]
fn pipe_review_ref_second_run_reuses_the_record_until_a_key_material_moves() {
    let (place, first) = rv_one();
    let name = |out: &Output| rv_row_out(out, "a").get("record").cloned().unwrap_or_default();
    let named = name(&rv_review(&place, &first));
    assert_eq!(rv_log(&place).len(), 1, "1 回目は撃つ");
    let again = rv_review(&place, &first);
    assert_eq!((rv_log(&place).len(), name(&again)), (1, named.clone()), "同じ sha の 2 回目は撃たず同じ記録の dir を名指す: {}", stdout_of(&again));
    let reqs = rv_commit(&place, Some(&first), &[("reqs.md", "# toy の要件\n\n## FR4\n\n本文を 1 行足す\n\n## FR5\n".to_owned())]);
    let by_reqs = name(&rv_review(&place, &reqs));
    assert_eq!(rv_log(&place).len(), 2, "要件の本文を変えた commit は撃つ");
    let code = rv_commit(&place, Some(&first), &[("src/extra.rs", "pub fn extra() {}\n".to_owned())]);
    let by_code = name(&rv_review(&place, &code));
    assert_eq!(rv_log(&place).len(), 3, "code の file を変えた commit は撃つ");
    fs::write(place.state.join("rv-version"), "lens-version fake=2\n").expect("版を書き換えられる");
    let by_version = name(&rv_review(&place, &first));
    assert_eq!(rv_log(&place).len(), 4, "版の行を変えた回は撃つ");
    let names = BTreeSet::from([named, by_reqs, by_code, by_version]);
    assert_eq!(names.len(), 4, "鍵が違えば記録の dir の名も違う: {names:?}");
    clean(&[&place.repo, &place.state]);
}

/// (h) 理由の型が unparsed の記録を持つ行は同じ鍵でも撃ち直される（kind の無い FAIL を 2 回・PASS に変えて 1 回撃ち、PASS の記録は撃ち直さない）。
#[test]
fn pipe_review_ref_unparsed_record_is_fired_again_under_the_same_key() {
    let (place, sha) = rv_one();
    rv_out(&place, "", r#"{"verdict":"FAIL","evidence":"e"}"#);
    let out = rv_review(&place, &sha);
    let record = rv_record(&place, rv_row_out(&out, "a").get("record").map_or("", String::as_str));
    assert_eq!((record.get("verdict").map(String::as_str), record.get("kind").map(String::as_str)), (Some("FAIL"), Some("unparsed")));
    rv_review(&place, &sha);
    assert_eq!(rv_log(&place).len(), 2, "unparsed の記録は同じ鍵でも撃ち直す");
    rv_out(&place, "", r#"{"verdict":"PASS","evidence":"e"}"#);
    let third = rv_review(&place, &sha);
    assert_eq!((rv_log(&place).len(), rv_row_out(&third, "a").get("verdict").cloned()), (3, Some("PASS".to_owned())), "PASS に変えて撃ち直す");
    rv_review(&place, &sha);
    assert_eq!(rv_log(&place).len(), 3, "PASS の記録は撃ち直さない");
    clean(&[&place.repo, &place.state]);
}

/// 結果の語の 1 形: main に行 m だけの置き場へ、`rows` を足した設計の PR の commit を `issues` の台帳・行ごとの偽 lens の出力 `outs` で審査し、
/// （最後の行・rc・行 id ごとの basis）を返す。
fn rv_case(rows: &[Vec<String>], issues: &[String], outs: &[(&str, String)]) -> (String, Option<i32>, Vec<(String, String)>) {
    let place = rv_place(&["本文 1"], &[rv_row("m", 1, &[])], &[]);
    rv_ledger(&place, issues, None);
    for (id, body) in outs {
        rv_out(&place, id, body);
    }
    let mut all = vec![rv_row("m", 1, &[])];
    all.extend(rows.iter().cloned());
    let sha = rv_commit(&place, None, &[(DESIGN_FILE, rv_doc(&["本文 1"], &all))]);
    let out = rv_review(&place, &sha);
    let bases = rv_row_ids(&rv_ref_text(&place, &sha)).into_iter().map(|id| (id.clone(), rv_row_out(&out, &id).get("basis").cloned().unwrap_or_default())).collect();
    let last = rv_lines(&out).last().cloned().unwrap_or_default();
    clean(&[&place.repo, &place.state]);
    (last, out.status.code(), bases)
}

/// (i) ref の結果の 5 形: 全行 PASS・forecast と partial の unparsed でない INCONCLUSIVE だけは pass（rc 0）、actual の INCONCLUSIVE・FAIL・
/// unparsed の行を 1 本持つ sha は fail（rc 1）。
#[test]
fn pipe_review_ref_result_word_follows_verdict_basis_and_kind() {
    let open = lens_finding("INCONCLUSIVE", Some("other"), None);
    let fresh = |id: &str, extra: &[(&str, &str)]| rv_row(id, 1, extra);
    let (pass, ok) = (|line: &str| line.contains("result=pass"), Some(i32::from(RC_OK)));
    let (last, rc, _) = rv_case(&[fresh("p1", &[])], &[], &[]);
    assert!(pass(&last) && rc == ok, "全行 PASS: {last}");
    let ancestor = fresh("anc", &[("write-set", r#"["+src/fresh.rs"]"#)]);
    let child = fresh("fc", &[("write-set", r#"["src/fresh.rs"]"#), ("depends", r#"["anc"]"#)]);
    let outs = [("fc", open.clone()), ("p2", open.clone())];
    let (last, rc, bases) = rv_case(&[ancestor, child, fresh("p2", &[])], &[], &outs);
    assert!(pass(&last) && rc == ok, "forecast と partial の INCONCLUSIVE だけは pass: {last}");
    assert!(bases.contains(&("fc".to_owned(), "forecast".to_owned())) && bases.contains(&("p2".to_owned(), "partial".to_owned())), "{bases:?}");
    let (last, rc, bases) = rv_case(&[fresh("ac", &[])], &[prelens_issue("s2-rv.9", "open", "ac", &[])], &[("ac", open)]);
    assert!(!pass(&last) && rc == Some(i32::from(RC_REFUSED)) && bases == [("ac".to_owned(), "actual".to_owned())], "actual の INCONCLUSIVE は fail: {last} {bases:?}");
    let failed = lens_finding("FAIL", Some("other"), None);
    let (last, rc, _) = rv_case(&[fresh("p3", &[])], &[], &[("p3", failed)]);
    assert!(!pass(&last) && rc == Some(i32::from(RC_REFUSED)), "FAIL は fail: {last}");
    let (last, rc, _) = rv_case(&[fresh("p4", &[])], &[], &[("p4", lens_finding("INCONCLUSIVE", None, None))]);
    assert!(!pass(&last) && rc == Some(i32::from(RC_REFUSED)), "partial でも理由の型が unparsed の INCONCLUSIVE は fail: {last}");
}

/// (j) 同じ sha に生きた撃ち中の印を置いた周は偽 lens 0 回で result=pending の 1 行と rc 1 で待たずに終わり（印は残る）、死んだ pid の印を置いた周は
/// 印を外して撃ち直す（撃ち終えた後に印は無い）。
#[test]
fn pipe_review_ref_live_mark_is_pending_and_a_dead_mark_is_fired_over() {
    let (place, sha) = rv_one();
    fs::create_dir_all(rr_root(&place.state).join("ref")).expect("ref の dir を作れる");
    rr_mark(&place.state, &sha, std::process::id());
    let pending = rv_review(&place, &sha);
    assert_eq!((pending.status.code(), rv_lines(&pending)), (Some(i32::from(RC_REFUSED)), vec![format!("[ROW-REVIEW] result=pending ref={sha}")]), "{}", stderr_of(&pending));
    assert!(rv_log(&place).is_empty() && row_review::mark_path(&place.state, &sha).exists(), "撃たず、生きた印は外さない");
    assert!(matches!(row_review::read_ref(&place.state, &sha), RefResult::Pending), "口 (A) は pending");
    rr_mark(&place.state, &sha, rr_dead_pid());
    let fired = rv_review(&place, &sha);
    assert_eq!(fired.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&fired), stderr_of(&fired));
    assert_eq!(rv_log(&place).len(), 1, "死んだ印は外して撃ち直す");
    assert!(!row_review::mark_path(&place.state, &sha).exists(), "撃ち終えて result の行を書いた後に印は無い");
    clean(&[&place.repo, &place.state]);
}

/// (k) 偽 lens は審査の木の中で起き（cwd の HEAD が --ref の sha・anchor の木でない）、撃たれている間に受付札の置き場に run が
/// row-review-<ref の 12 桁>-<行 id> の札が 1 枚在り、撃ち終えた後に審査の木と表の木と merge-base の木の worktree の登録も札も一時の dir も残らない。
#[test]
fn pipe_review_ref_lens_runs_in_the_review_tree_under_its_own_ticket_and_leaves_nothing() {
    let (place, sha) = rv_one();
    let out = rv_review(&place, &sha);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    let seen = rv_log(&place).into_iter().next().unwrap_or_default();
    assert_eq!(rv_field(&seen, "head"), sha, "偽 lens が見る HEAD は --ref の sha: {seen}");
    let cwd = rv_field(&seen, "cwd");
    assert!(cwd.ends_with(&format!("{sha}.tree")) && !cwd.starts_with(&place.repo.display().to_string()), "cwd は審査の木: {cwd}");
    let ticket = format!("\"run\":\"row-review-{}-a\"", sha.chars().take(12).collect::<String>());
    assert_eq!(rv_field(&seen, "slots").matches(&ticket).count(), 1, "撃たれている間の札は 1 枚: {seen}");
    let tickets = fs::read_dir(vessel::seat::host_slots_dir(&place.state)).map(|entries| entries.flatten().count()).unwrap_or_default();
    assert_eq!(tickets, 0, "撃ち終えた後に札は残らない");
    let worktrees = git(&place.repo, &["worktree", "list", "--porcelain"]);
    assert_eq!(worktrees.lines().filter(|line| line.starts_with("worktree ")).count(), 1, "worktree の登録が残らない: {worktrees}");
    assert!(!rr_root(&place.state).join("ref").join(format!("{sha}.work")).exists(), "一時の dir も残らない");
    clean(&[&place.repo, &place.state]);
}

/// 行の記録の key の列（§9 の 14 key・この順）。
const RV_KEYS: [&str; 14] = [
    "row", "digest", "key", "basis", "ancestors", "mech", "verdict", "kind", "materials", "tree", "version", "ref", "at", "usage",
];

/// (l) 行の記録は 1 行目 schema=1 と §9 の 14 key をこの順に持ち、dir の名は口 (H) を同じ材料で直に呼んだ名と等しい。ref の記録は schema・base
/// （merge-base の sha）・tables（契約表の file の列）・row の行（digest の欄を含む）・result を持つ。stdout は [ROW-REVIEW] の行だけで notify の行は無い。
#[test]
fn pipe_review_ref_writes_the_record_shape_of_section_nine() {
    let (place, sha) = rv_one();
    let out = rv_review(&place, &sha);
    assert!(rv_lines(&out).iter().all(|line| line.starts_with("[ROW-REVIEW]") && !line.contains("notify")), "stdout は [ROW-REVIEW] の行だけ: {}", stdout_of(&out));
    let name = rv_row_out(&out, "a").get("record").cloned().unwrap_or_default();
    let text = fs::read_to_string(rr_root(&place.state).join(&name).join("record")).unwrap_or_default();
    let keys: Vec<&str> = text.lines().skip(1).filter_map(|line| line.split_once('=').map(|(key, _)| key)).collect();
    assert!(text.starts_with("schema=1\n") && keys == RV_KEYS, "1 行目 schema=1 と 14 key の列: {text}");
    let record = rv_record(&place, &name);
    let key = format!("{DESIGN_FILE}#a");
    let parts = Parts { digest: &record["digest"], materials: &record["materials"], tree: &record["tree"], basis: Basis::Partial, ancestors: &[], version: &record["version"] };
    assert_eq!(row_review::judgement(&key, &parts), (record["key"].clone(), name.clone()), "dir の名は口 (H) の返す名");
    assert_eq!((record["ref"].as_str(), record["mech"].as_str(), record["verdict"].as_str(), record["usage"].as_str()), (sha.as_str(), "clean", "PASS", "-"));
    let refs = rv_ref_text(&place, &sha);
    let want = format!("schema=1\nbase={}\ntables={DESIGN_FILE}\nrow={key} digest={} id={name} verdict=PASS basis=partial\nresult=pass\n", place.main, record["digest"]);
    assert_eq!(refs, want, "ref の記録の形");
    clean(&[&place.repo, &place.state]);
}

/// (m) 口が書いた記録を行 a1 の口が読む（書き手と読み手の形の一致）: 全行 PASS の sha で口 (A) が pass と merge-base の sha と契約表の file の列を、FAIL の行を
/// 持つ sha で fail を返し、PASS・actual の行で口 (D) が記録の dir の名を返し（FAIL の行は返さない）、口 (E) が互いの行を、口 (F) が撃った行の記録の判定と
/// basis を返す。口 (B) に便の置き場の写し（末尾の改行を 1 つ除いた字）を渡した値は記録の digest と同じで、口 (C) を --ref の sha で呼んだ値は記録の code の木の鍵と同じ。
#[test]
fn pipe_review_ref_record_is_read_back_by_the_row_review_mouths() {
    let place = rv_place(&["本文 1"], &[rv_row("r1", 1, &[]), rv_row("r2", 1, &[])], &[]);
    rv_ledger(&place, &[prelens_issue("s2-rv.1", "open", "r1", &[]), prelens_issue("s2-rv.2", "open", "r2", &[])], None);
    let rows = |done: &str| rv_doc(&["本文 1"], &[rv_row("r1", 1, &[("done", done)]), rv_row("r2", 1, &[("done", done)])]);
    let (first, second) = (rv_commit(&place, None, &[(DESIGN_FILE, rows("\"一\""))]), rv_commit(&place, None, &[(DESIGN_FILE, rows("\"二\""))]));
    let passed = rv_review(&place, &first);
    rv_out(&place, "r2", &lens_finding("FAIL", Some("other"), None));
    rv_review(&place, &second);
    let want = RefResult::Pass { base: place.main.clone(), tables: vec![DESIGN_FILE.to_owned()] };
    assert_eq!((row_review::read_ref(&place.state, &first), row_review::read_ref(&place.state, &second)), (want, RefResult::Fail), "口 (A)");
    let (key1, key2) = (format!("{DESIGN_FILE}#r1"), format!("{DESIGN_FILE}#r2"));
    let name = rv_row_out(&passed, "r1").get("record").cloned().unwrap_or_default();
    let record = rv_record(&place, &name);
    let keys = [record["digest"].as_str(), record["materials"].as_str(), record["tree"].as_str(), record["version"].as_str()];
    assert_eq!(row_review::reusable(&place.state, &key1, keys), Some(name.clone()), "口 (D): PASS・actual の行の記録の dir の名");
    let failed = rv_record(&place, &rv_row_line(&rv_ref_text(&place, &second), "r2")["id"]);
    let failed_keys = [failed["digest"].as_str(), failed["materials"].as_str(), failed["tree"].as_str(), failed["version"].as_str()];
    assert_eq!(row_review::reusable(&place.state, &key2, failed_keys), None, "口 (D): FAIL の記録は返さない");
    assert_eq!(row_review::siblings(&place.state, &key1, &record["digest"]), (vec![key2.clone()], 0), "口 (E): 互いの行");
    let (listed, unreadable) = row_review::listed(&place.state, &key1, &record["digest"]);
    let at = record["at"].parse().unwrap_or_default();
    assert_eq!((listed, unreadable), (vec![Listed { verdict: Verdict::Pass, basis: Basis::Actual, kind: None, at }], 0), "口 (F)");
    let copy = |file: &str| fs::read_to_string(rr_root(&place.state).join(&name).join("review").join(file)).unwrap_or_default();
    let trimmed = |text: String| text.strip_suffix('\n').map(str::to_owned).unwrap_or(text);
    assert_eq!(row_review::row_digest(&trimmed(copy("contract.toml")), &trimmed(copy("design.txt"))), record["digest"], "口 (B)");
    assert_eq!(row_review::tree_key(&place.repo, &first), Ok(record["tree"].clone()), "口 (C)");
    clean(&[&place.repo, &place.state]);
}

/// (n) 版の flag で rc 1 を返す偽 lens の行は lens を撃たずに INCONCLUSIVE・unparsed（版の分からない判定を鍵に入れない）で、ref の結果は fail。
#[test]
fn pipe_review_ref_unreadable_lens_version_is_unparsed_without_firing() {
    let (place, sha) = rv_one();
    fs::write(place.state.join("rv-version-rc"), "1\n").expect("版の rc を書ける");
    let out = rv_review(&place, &sha);
    assert!(rv_log(&place).is_empty(), "lens を撃たない");
    let row = rv_row_out(&out, "a");
    assert_eq!(row.get("verdict").map(String::as_str), Some("INCONCLUSIVE"), "{}", stdout_of(&out));
    assert_eq!(rv_record(&place, row.get("record").map_or("", String::as_str)).get("kind").map(String::as_str), Some("unparsed"));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "unparsed の行は fail: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("lens の版"), "理由を stderr に残す: {}", stderr_of(&out));
    clean(&[&place.repo, &place.state]);
}

/// (o) 行の lens の判定は Reviewed と同じ読みの 2 本を 1 つの口で通る: 番号の付いた 3 項目の done を持つ行で偽 lens が PASS と歯の無い項目（-）を持つ対応の表を
/// 返すと行の記録は FAIL・vacuous-assert（ref の結果は fail）、揃った表の PASS は PASS のまま。約束の行を持つ行で偽 lens が 3 語の外の kind の FAIL を返すと
/// 行の記録は INCONCLUSIVE で kind は lens の値のまま。
#[test]
fn pipe_review_ref_reads_the_lens_through_the_done_table_and_the_promised_narrowing() {
    let other = "pub fn derive_outside() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn derive_in_src() {}\n}\n";
    let place = rv_place(&["本文 1"], &[rv_row("m", 1, &[])], &[("crates/toy/src/other.rs", other.to_owned())]);
    let done = ("done", "\"(1) 甲を作る (2) 乙を測る (3) 丙を足す\"");
    let mut promised = row_fields("pr", &["write-set", "verify", "done"], &[]);
    let promise = ["", "[[promise]]", "of = \"pr\"", "n = 1", "text = \"約束 1\"", "files = [\"crates/toy/src/other.rs\"]", "teeth = [\"derive_in_src\"]", "fixture = \"toy の repo\"", "expect = \"src の歯が緑\""];
    promised.extend(promise.map(str::to_owned));
    rv_out(&place, "i1", &table_pass("1:pipe_x_,2:-,3:-"));
    rv_out(&place, "i2", &table_pass("1:a,2:b,3:c"));
    rv_out(&place, "pr", &lens_finding("FAIL", Some("teeth-outside-write-set"), Some("x")));
    let rows = [rv_row("m", 1, &[]), rv_row("i1", 1, &[done]), rv_row("i2", 1, &[done]), promised];
    let sha = rv_commit(&place, None, &[(DESIGN_FILE, rv_doc(&["本文 1"], &rows))]);
    let out = rv_review(&place, &sha);
    let kind = |id: &str| {
        let record = rv_record(&place, rv_row_out(&out, id).get("record").map_or("", String::as_str));
        (record.get("verdict").cloned().unwrap_or_default(), record.get("kind").cloned().unwrap_or_default())
    };
    assert_eq!(kind("i1"), ("FAIL".to_owned(), "vacuous-assert".to_owned()), "歯の無い項目を持つ表の PASS は倒れる: {} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(kind("i2"), ("PASS".to_owned(), "-".to_owned()), "揃った表の PASS は PASS のまま");
    assert_eq!(kind("pr"), ("INCONCLUSIVE".to_owned(), "teeth-outside-write-set".to_owned()), "約束の行の 3 語の外の kind は INCONCLUSIVE・kind は lens の値のまま");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の行が在る周は fail");
    clean(&[&place.repo, &place.state]);
}

// ───── Reviewed の段の行の審査の記録の使い回し（設計 docs/design/row-review.md §5・行 c・接頭辞 `pipe_review_row_reuse_`） ─────
//
// 設計の PR の commit を `pipe review --ref` で審査して actual の PASS の記録を作り、その commit を main にして `pipe intake` の Reviewed を撃つ。
// 偽 lens は行の審査と Reviewed で同じ log に 1 行を足す（版の flag の撃ちは別の log `rv-vlog`）。base には読み口が無い＝Reviewed は必ず偽 lens を
// 撃つ。各形は先に、手を入れない記録と同じ main で写す（撃ち 0 回）ことを測ってから、鍵か記録を 1 つ動かして撃つ側を測る。

/// 6 値を運ぶ FAIL の判定の行（撃った周は審査の消費の event を 1 件書く・写した周との差を測る）。
fn rrr_fail() -> String {
    let usage = r#""usage":"in:7,out:8,cache_read:9,cache_create:10","turns":2,"wall_ms":300"#;
    format!("{},{usage}}}", lens_finding("FAIL", Some("other"), None).trim_end_matches('}'))
}

/// 行 a（bead 有り・祖先なし＝basis actual）の設計の PR の commit を行の審査で撃って PASS の記録を作り、その commit を anchor の main にした置き場。
/// 返りは (置き場, commit, 行の記録の dir の名)。偽 lens の出力は消費つきの PASS。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rrr_ready() -> (Rv, String, String) {
    let place = rv_place(&["本文 1"], &[rv_row("a", 1, &[])], &[]);
    rv_ledger(&place, &[prelens_issue("s2-rv.1", "open", "a", &[])], None);
    rv_out(&place, "", &reuse_pass());
    let sha = rv_commit(&place, None, &[(DESIGN_FILE, rv_doc(&["本文 1"], &[rv_row("a", 1, &[("done", "\"変えた\"")])]))]);
    let out = rv_review(&place, &sha);
    let row = rv_row_out(&out, "a");
    assert_eq!((row.get("verdict").map(String::as_str), row.get("basis").map(String::as_str)), (Some("PASS"), Some("actual")), "{}", stdout_of(&out));
    assert_eq!(rv_log(&place).len(), 1, "行の審査は偽 lens を 1 回撃つ");
    git(&place.repo, &["merge", "-q", "--ff-only", &sha]);
    let name = row.get("record").cloned().expect("行の記録の dir の名");
    (place, sha, name)
}

/// 版の flag の撃ちの回数。
fn rrr_vlog(place: &Rv) -> usize {
    fs::read_to_string(place.state.join("rv-vlog")).unwrap_or_default().lines().count()
}

/// Reviewed の 1 回の結果。
struct Shot {
    /// 便の id。
    id: String,
    /// Reviewed の detail。
    detail: String,
    /// 偽 lens の撃ち（版の flag を除く）の増分。
    fired: usize,
    /// 版の flag の撃ちの増分。
    versions: usize,
    /// 審査の消費の event の増分。
    costs: usize,
}

/// 行 a の `pipe intake` を anchor の main で 1 回撃つ（bead は `bead`）。
fn rrr_intake(place: &Rv, bead: &str) -> Shot {
    let costs = || events(&place.state).iter().filter(|event| event.kind == EventKind::RunCost).count();
    let (fired, versions, spent) = (rv_log(place).len(), rrr_vlog(place), costs());
    let out = run_pipe_with_path(
        &place.path,
        &[
            "intake", "--design", &format!("{DESIGN_FILE}#a"), "--bead", bead, "--repo", &place.repo.display().to_string(),
            "--state-dir", &place.state.display().to_string(), "--rules", &ceiling_rules(&place.state), "--lens", &place.lens,
        ],
    );
    let id = run_id_of(&out);
    assert!(!id.is_empty(), "審査まで届く: {} / {}", stdout_of(&out), stderr_of(&out));
    Shot { detail: reviewed_detail(&place.state, &id), id, fired: rv_log(place).len() - fired, versions: rrr_vlog(place) - versions, costs: costs() - spent }
}

/// 手を入れない記録を同じ main で写す（偽 lens 0 回・消費 0 件・detail が `verdict:PASS row-review:reused`）ことを測り、便を外す（各形の前提）。
fn rrr_copies(place: &Rv, bead: &str) {
    let shot = rrr_intake(place, bead);
    assert_eq!((shot.fired, shot.detail.as_str(), shot.costs), (0, "verdict:PASS row-review:reused", 0), "手を入れない記録は写す");
    stop_run_ok(&place.state, &shot.id);
}

/// 行の記録の `key` の欄の値を `value` に書き換える（dir の名と他の欄は変えない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rrr_edit(place: &Rv, name: &str, key: &str, value: &str) {
    let path = rr_root(&place.state).join(name).join("record");
    let text = fs::read_to_string(&path).expect("行の記録を読める");
    let edited: String = text
        .lines()
        .map(|line| match line.split_once('=') {
            Some((found, _)) if found == key => format!("{key}={value}\n"),
            _ => format!("{line}\n"),
        })
        .collect();
    assert_ne!(text, edited, "{key} の欄を書き換えられる");
    fs::write(&path, edited).expect("行の記録を書ける");
}

/// 撃つ側の確認: 偽 lens を 1 回撃ち、判定は撃った lens の値（FAIL）で、語は付かず、消費の event を 1 件書く。
fn rrr_fires(shot: &Shot, why: &str) {
    assert_eq!((shot.fired, shot.detail.as_str(), shot.costs), (1, "verdict:FAIL kind:other", 1), "{why}");
}

/// (a) 同じ commit を main にして intake した便は、Reviewed の段で偽 lens を撃たず（版の flag の撃ちは 1 回）、記録の判定 PASS を写して detail が
/// `verdict:PASS row-review:reused`・review.json の verdict は PASS で、審査の消費の event を書かない（撃った周の 1 件との対は (b) が測る）。
#[test]
fn pipe_review_row_reuse_copies_the_actual_pass_record_without_firing() {
    let (place, _, _) = rrr_ready();
    let shot = rrr_intake(&place, "s2-rr.1");
    assert_eq!((shot.fired, shot.versions, shot.costs), (0, 1, 0), "偽 lens は撃たず版を 1 回読み、消費は書かない: {}", shot.detail);
    assert_eq!(shot.detail, "verdict:PASS row-review:reused");
    assert_eq!(value_of(&review_pairs(&place.state, &shot.id), "verdict"), "PASS");
    clean(&[&place.repo, &place.state]);
}

/// (b) 鍵が外れる 6 形は偽 lens を 1 回撃ち、判定は撃った lens の値（FAIL）で、消費の event を 1 件書く: 記録の digest・材料の鍵・code の木の鍵・lens の版の欄を
/// 手で書き換えた記録の 4 形と、code の file を 1 つ変えた main・版の 1 行を変えた偽 lens の 2 形。各形は先に手を入れない記録と同じ main で写す。
#[test]
fn pipe_review_row_reuse_fires_the_lens_when_any_of_the_four_keys_moves() {
    for field in ["digest", "materials", "tree", "version"] {
        let (place, _, name) = rrr_ready();
        rrr_copies(&place, "s2-rr.1");
        rrr_edit(&place, &name, field, "edited");
        rv_out(&place, "", &rrr_fail());
        rrr_fires(&rrr_intake(&place, "s2-rr.2"), &format!("{field} の欄を書き換えた記録は写さない"));
        clean(&[&place.repo, &place.state]);
    }
    let (place, sha, _) = rrr_ready();
    rrr_copies(&place, "s2-rr.1");
    let moved = rv_commit(&place, Some(&sha), &[("src/extra.rs", "pub fn extra() {}\n".to_owned())]);
    git(&place.repo, &["merge", "-q", "--ff-only", &moved]);
    rv_out(&place, "", &rrr_fail());
    rrr_fires(&rrr_intake(&place, "s2-rr.2"), "code の file を変えた main は写さない");
    clean(&[&place.repo, &place.state]);
    let (place, _, _) = rrr_ready();
    rrr_copies(&place, "s2-rr.1");
    fs::write(place.state.join("rv-version"), "lens-version fake=2\n").expect("版を書き換えられる");
    rv_out(&place, "", &rrr_fail());
    rrr_fires(&rrr_intake(&place, "s2-rr.2"), "版の 1 行を変えた lens は写さない");
    clean(&[&place.repo, &place.state]);
}

/// (c) 写さない 5 形は偽 lens を 1 回撃つ: 判定の欄を FAIL・basis の欄を forecast・basis の欄を partial・祖先の欄を tree に書き換えた記録と、dir の名を変えた記録。
/// 各形は先に手を入れない記録と同じ main で写す。
#[test]
fn pipe_review_row_reuse_never_copies_a_record_that_is_not_an_actual_landed_pass_under_its_own_name() {
    for (key, value) in [("verdict", "FAIL"), ("basis", "forecast"), ("basis", "partial"), ("ancestors", "docs/design/toy.md#y:tree")] {
        let (place, _, name) = rrr_ready();
        rrr_copies(&place, "s2-rr.1");
        rrr_edit(&place, &name, key, value);
        rv_out(&place, "", &rrr_fail());
        rrr_fires(&rrr_intake(&place, "s2-rr.2"), &format!("{key}={value} の記録は写さない"));
        clean(&[&place.repo, &place.state]);
    }
    let (place, _, name) = rrr_ready();
    rrr_copies(&place, "s2-rr.1");
    let root = rr_root(&place.state);
    fs::rename(root.join(&name), root.join(format!("{name}0"))).expect("dir の名を変えられる");
    rv_out(&place, "", &rrr_fail());
    rrr_fires(&rrr_intake(&place, "s2-rr.2"), "dir の名を変えた記録は写さない");
    clean(&[&place.repo, &place.state]);
}

/// (d) 版の flag が版の 1 行を返す lens で写せた記録を、版の flag で rc 1 を返す lens の cmd の便は写さずに撃ち、判定は撃った lens の値（FAIL）。
#[test]
fn pipe_review_row_reuse_never_copies_when_the_lens_cannot_name_its_version() {
    let (place, _, _) = rrr_ready();
    rrr_copies(&place, "s2-rr.1");
    fs::write(place.state.join("rv-version-rc"), "1\n").expect("版の rc を書ける");
    rv_out(&place, "", &rrr_fail());
    rrr_fires(&rrr_intake(&place, "s2-rr.2"), "版を読めない lens の便は写さない");
    clean(&[&place.repo, &place.state]);
}

/// (e) 版を撃つのは同じ行と digest の PASS・actual の記録が在る周だけ: 記録の無い便と digest の欄を書き換えた記録の便は版の撃ち 0 回、材料の鍵の欄を書き換えた
/// 記録の便は 1 回（どれも偽 lens は 1 回撃つ・手を入れない記録の便は版を 1 回撃って写す）。
#[test]
fn pipe_review_row_reuse_reads_the_version_only_when_a_same_digest_actual_pass_exists() {
    let place = rv_place(&["本文 1"], &[rv_row("a", 1, &[])], &[]);
    rv_out(&place, "", &rrr_fail());
    let none = rrr_intake(&place, "s2-rr.1");
    rrr_fires(&none, "記録の無い便は撃つ");
    assert_eq!(none.versions, 0, "記録の無い便は版を撃たない");
    clean(&[&place.repo, &place.state]);
    for (field, versions) in [("digest", 0), ("materials", 1)] {
        let (place, _, name) = rrr_ready();
        rrr_edit(&place, &name, field, "edited");
        rv_out(&place, "", &rrr_fail());
        let shot = rrr_intake(&place, "s2-rr.1");
        rrr_fires(&shot, &format!("{field} の欄を書き換えた記録の便は撃つ"));
        assert_eq!(shot.versions, versions, "{field} の欄を書き換えた記録の便の版の撃ち");
        clean(&[&place.repo, &place.state]);
    }
}

// ───── 逆引きの表と審査の材料 index.txt（設計 reverse-index.md §6・§7 (a)・契約表の行 c・接頭辞 `pipe_index_show_`） ─────
//
// 小さな crate の fixture（別名の取り込み・`Self` の literal・glob の取り込み・`pub use` の再輸出・test の module の file・doc の link・文字列の
// 取り込み・別 module の同名の型・toml の名指し）を、行 a1 の歯の file（pipe.rs）の SCIP の書き手で SCIP と一致の列にし、行 a2 の歯の偽の宣言
// （`IdxPlace`）が写す。撃つ口は実 binary の `pipe index show` と `pipe intake`（審査の材料）。

/// 本体の file（型 `Gadget`・doc の link・`Self` の literal・文字列の取り込み・test の module の宣言）。
const GX_SHAPE_RS: &str = "crates/toy/src/shape.rs";
const GX_SHAPE: &str = "/// 見る: [`Gadget`] の話\npub struct Gadget { pub x: u8 }\nimpl Gadget {\n    pub fn new() -> Self {\n        Self { x: 0 }\n    }\n}\npub fn make() -> Gadget {\n    Gadget { x: 1 }\n}\nfn describe() -> String { format!(\"{Gadget}\") }\n#[cfg(test)]\nmod tests;\n";

/// test の module の file（`#[cfg(test)] mod tests;` の先）。
const GX_TESTS_RS: &str = "crates/toy/src/shape/tests.rs";
const GX_TESTS: &str = "fn builds() { let _ = crate::shape::make(); let _ = crate::shape::Gadget { x: 2 }; }\n";

/// 別名の取り込み・glob の取り込み・match の pattern・呼び出しを持つ file（`run` は別 module にも在る）。
const GX_USER_RS: &str = "crates/toy/src/user.rs";
const GX_USER: &str = "use crate::shape::Gadget as Widget;\nuse crate::shape::*;\npub fn assemble() -> Widget { let _ = make(); Widget { x: 3 } }\npub fn route(g: Gadget) -> u8 {\n    match g {\n        Gadget { x } => x,\n    }\n}\npub fn run() {}\n";

/// `pub use` の再輸出と module の宣言。
const GX_LIB_RS: &str = "crates/toy/src/lib.rs";
const GX_LIB: &str = "pub use crate::shape::Gadget;\npub(crate) mod shape;\nmod user;\nmod other;\n";

/// 別 module の同名の型と同名の fn。
const GX_OTHER_RS: &str = "crates/toy/src/other.rs";
const GX_OTHER: &str = "pub struct Gadget;\nfn other(_: Gadget) {}\npub fn run() {}\n";

/// toml に現れる名（索引の外）。
const GX_TOML_PATH: &str = "config/gadget.toml";
const GX_TOML: &str = "[shape]\nkind = \"Gadget\"\n";

/// fixture の file（相対 path と本文）。
const GX_FILES: [(&str, &str); 6] = [(GX_SHAPE_RS, GX_SHAPE), (GX_TESTS_RS, GX_TESTS), (GX_USER_RS, GX_USER), (GX_LIB_RS, GX_LIB), (GX_OTHER_RS, GX_OTHER), (GX_TOML_PATH, GX_TOML)];

const GX_GADGET: &str = "rust-analyzer cargo toy 0.1.0 shape/Gadget#";
const GX_NEW: &str = "rust-analyzer cargo toy 0.1.0 shape/Gadget#new().";
const GX_MAKE: &str = "rust-analyzer cargo toy 0.1.0 shape/make().";
const GX_DESCRIBE: &str = "rust-analyzer cargo toy 0.1.0 shape/describe().";
const GX_SHAPE_TESTS: &str = "rust-analyzer cargo toy 0.1.0 shape/tests/";
const GX_BUILDS: &str = "rust-analyzer cargo toy 0.1.0 shape/tests/builds().";
const GX_MOD_SHAPE: &str = "rust-analyzer cargo toy 0.1.0 shape/";
const GX_MOD_USER: &str = "rust-analyzer cargo toy 0.1.0 user/";
const GX_MOD_OTHER: &str = "rust-analyzer cargo toy 0.1.0 other/";
const GX_ASSEMBLE: &str = "rust-analyzer cargo toy 0.1.0 user/assemble().";
const GX_ROUTE: &str = "rust-analyzer cargo toy 0.1.0 user/route().";
const GX_RUN_USER: &str = "rust-analyzer cargo toy 0.1.0 user/run().";
const GX_OTHER_GADGET: &str = "rust-analyzer cargo toy 0.1.0 other/Gadget#";
const GX_OTHER_FN: &str = "rust-analyzer cargo toy 0.1.0 other/other().";
const GX_RUN_OTHER: &str = "rust-analyzer cargo toy 0.1.0 other/run().";

/// occurrence 1 つ（`at` は（文脈・文脈の中の字）・`scope` は定義の囲む範囲の字〔無ければ参照〕）。
fn gx_occ(body: &str, at: (&str, &str), symbol: &str, scope: Option<&str>) -> Vec<u8> {
    let range = scip_range(body, span_in(body, at.0, at.1), false);
    let enclosing = scope.map(|whole| scip_range(body, span_of(body, whole, 0), false)).unwrap_or_default();
    scip_occ(&range, symbol, scope.is_some(), &enclosing)
}

/// module を宣言する occurrence（囲む範囲を持たない定義・`declared` は `mod <名>;`）。
fn gx_module(body: &str, declared: &str, symbol: &str) -> Vec<u8> {
    let name = declared.trim_start_matches("mod ").trim_end_matches(';');
    scip_occ(&scip_range(body, span_in(body, declared, name), false), symbol, true, &[])
}

/// fixture の SCIP の bytes。
fn gx_scip_bytes() -> Vec<u8> {
    let shape = vec![
        gx_occ(GX_SHAPE, ("pub struct Gadget", "Gadget"), GX_GADGET, Some("pub struct Gadget { pub x: u8 }")),
        gx_occ(GX_SHAPE, ("impl Gadget", "Gadget"), GX_GADGET, None),
        gx_occ(GX_SHAPE, ("fn new", "new"), GX_NEW, Some("pub fn new() -> Self {\n        Self { x: 0 }\n    }")),
        gx_occ(GX_SHAPE, ("-> Self", "Self"), GX_GADGET, None),
        gx_occ(GX_SHAPE, ("Self { x: 0 }", "Self"), GX_GADGET, None),
        gx_occ(GX_SHAPE, ("fn make", "make"), GX_MAKE, Some("pub fn make() -> Gadget {\n    Gadget { x: 1 }\n}")),
        gx_occ(GX_SHAPE, ("-> Gadget", "Gadget"), GX_GADGET, None),
        gx_occ(GX_SHAPE, ("Gadget { x: 1 }", "Gadget"), GX_GADGET, None),
        gx_occ(GX_SHAPE, ("fn describe", "describe"), GX_DESCRIBE, Some("fn describe() -> String { format!(\"{Gadget}\") }")),
        gx_module(GX_SHAPE, "mod tests;", GX_SHAPE_TESTS),
    ];
    let tests = vec![
        scip_occ(&[0, 0, 0], GX_SHAPE_TESTS, true, &[]),
        gx_occ(GX_TESTS, ("fn builds", "builds"), GX_BUILDS, Some("fn builds() { let _ = crate::shape::make(); let _ = crate::shape::Gadget { x: 2 }; }")),
        gx_occ(GX_TESTS, ("shape::make", "make"), GX_MAKE, None),
        gx_occ(GX_TESTS, ("shape::Gadget", "Gadget"), GX_GADGET, None),
    ];
    let user = vec![
        gx_occ(GX_USER, ("shape::Gadget as", "Gadget"), GX_GADGET, None),
        gx_occ(GX_USER, ("fn assemble", "assemble"), GX_ASSEMBLE, Some("pub fn assemble() -> Widget { let _ = make(); Widget { x: 3 } }")),
        gx_occ(GX_USER, ("-> Widget", "Widget"), GX_GADGET, None),
        gx_occ(GX_USER, ("make()", "make"), GX_MAKE, None),
        gx_occ(GX_USER, ("Widget { x: 3 }", "Widget"), GX_GADGET, None),
        gx_occ(GX_USER, ("fn route", "route"), GX_ROUTE, Some("pub fn route(g: Gadget) -> u8 {\n    match g {\n        Gadget { x } => x,\n    }\n}")),
        gx_occ(GX_USER, ("g: Gadget", "Gadget"), GX_GADGET, None),
        gx_occ(GX_USER, ("Gadget { x } =>", "Gadget"), GX_GADGET, None),
        gx_occ(GX_USER, ("fn run", "run"), GX_RUN_USER, Some("pub fn run() {}")),
    ];
    let lib = vec![
        gx_occ(GX_LIB, ("shape::Gadget;", "Gadget"), GX_GADGET, None),
        gx_module(GX_LIB, "mod shape;", GX_MOD_SHAPE),
        gx_module(GX_LIB, "mod user;", GX_MOD_USER),
        gx_module(GX_LIB, "mod other;", GX_MOD_OTHER),
    ];
    let other = vec![
        gx_occ(GX_OTHER, ("struct Gadget", "Gadget"), GX_OTHER_GADGET, Some("pub struct Gadget;")),
        gx_occ(GX_OTHER, ("fn other", "other"), GX_OTHER_FN, Some("fn other(_: Gadget) {}")),
        gx_occ(GX_OTHER, ("_: Gadget", "Gadget"), GX_OTHER_GADGET, None),
        gx_occ(GX_OTHER, ("fn run", "run"), GX_RUN_OTHER, Some("pub fn run() {}")),
    ];
    scip_index(
        &[
            scip_document(GX_SHAPE_RS, 1, &shape, &[]),
            scip_document(GX_TESTS_RS, 1, &tests, &[]),
            scip_document(GX_USER_RS, 1, &user, &[]),
            scip_document(GX_LIB_RS, 1, &lib, &[]),
            scip_document(GX_OTHER_RS, 1, &other, &[]),
        ],
        &[],
    )
}

/// fixture の役の一致の stream。
fn gx_roles_text() -> String {
    let role = |rule: &str, (file, body): (&str, &str), needle: &str, name: Option<&str>| idx_role_line(rule, file, span_of(body, needle, 0), name);
    let (shape, tests, user, lib) = ((GX_SHAPE_RS, GX_SHAPE), (GX_TESTS_RS, GX_TESTS), (GX_USER_RS, GX_USER), (GX_LIB_RS, GX_LIB));
    [
        role("doclink", shape, "[`Gadget`]", Some("Gadget")),
        role("literal", shape, "Self { x: 0 }", Some("Self")),
        role("literal", shape, "Gadget { x: 1 }", Some("Gadget")),
        role("capture", shape, "\"{Gadget}\"", Some("Gadget")),
        role("test", shape, "#[cfg(test)]\nmod tests;", None),
        role("vis", shape, "pub struct Gadget { pub x: u8 }", Some("pub")),
        role("vis", shape, "pub fn make() -> Gadget {\n    Gadget { x: 1 }\n}", Some("pub")),
        role("literal", tests, "Gadget { x: 2 }", Some("Gadget")),
        role("call", tests, "make()", None),
        role("use", user, "use crate::shape::Gadget as Widget;", None),
        role("use", user, "use crate::shape::*;", None),
        role("call", user, "make()", None),
        role("literal", user, "Widget { x: 3 }", Some("Widget")),
        role("pattern", user, "Gadget { x }", Some("Gadget")),
        role("use", lib, "pub use crate::shape::Gadget;", None),
        role("reexport", lib, "pub use crate::shape::Gadget;", None),
        role("vis", lib, "pub(crate) mod shape;", Some("pub(crate)")),
    ]
    .join("\n")
        + "\n"
}

/// 節 1 の散文: touches の型・型の path 形（`crate::` の有無）・fn 形・解けない fn 形・同名が 2 つの fn 形・touches の型の variant・散文（可視性・
/// 引数付きの予約語の呼び出し・大文字始まりの tuple variant の構築・末尾 `::`・glob）。
const GX_PROSE: &str = "型 `crate::shape::Gadget` を使う。`crate::shape::make` を呼び、`shape::Gadget` と `describe()` と `ghost()` と `run()` を見る。`Gadget::new` は touches の型の variant で、`pub(crate)`・`if (a)`・`match(x)`・`Some(x)`・`crate::shape::`・`crate::shape::*` は散文。";

/// 節 2 の散文（審査の材料の歯が使う・名は全部解ける）。
const GX_PROSE_TWO: &str = "型 `crate::shape::Gadget` と `describe()` を読む。";

/// 設計 doc の本文（節 2 つと契約表の行）。
fn gx_doc(rows: &[Vec<String>]) -> String {
    let listed: Vec<String> = rows.iter().map(|fields| format!("[[contract]]\n{}", fields.join("\n"))).collect();
    format!(
        "# 設計: toy\n\n## 1. 何を解くか\n\n{GX_PROSE}\n\n## 2. 読むだけ\n\n{GX_PROSE_TWO}\n\n{}\nschema = 1\n\n{}\n{}\n",
        table_begin(),
        listed.join("\n\n"),
        table_end()
    )
}

/// 表の行 1 本の欄（`derived` は write-set の字か `tests` の欄の字・節は 1）。
fn gx_row(id: &str, touches: bool, derived: &str) -> Vec<String> {
    let mut add = vec![derived];
    if touches {
        add.push(r#"touches = ["crate::shape::Gadget"]"#);
    }
    row_fields(id, &["write-set"], &add)
}

/// 逆引きの歯の置き場（偽の宣言を持つか持たない toy repo・表の行 a・b・c・e・f と `contracts/t.toml` の行 t1・偽の command）。
fn gx_place(declared: bool) -> IdxPlace {
    gx_place_with(if declared { IDXB_DECL } else { "" })
}

/// [`gx_place`] の宣言の本文を選ぶ形（`index` は宣言に足す索引の 2 key の行）。
fn gx_place_with(index: &str) -> IdxPlace {
    let (repo, state) = repo_with_state();
    let put = |path: &str, body: &str| {
        let target = repo.join(path);
        assert!(target.parent().is_some_and(|parent| fs::create_dir_all(parent).is_ok()), "{path} の dir を作れる");
        assert!(fs::write(&target, body).is_ok(), "{path} を書ける");
    };
    for (path, body) in GX_FILES {
        put(path, body);
    }
    let four = r#"write-set = ["crates/toy/src/shape.rs", "crates/toy/src/shape/tests.rs", "crates/toy/src/user.rs", "crates/toy/src/lib.rs"]"#;
    let rows = [
        gx_row("a", true, r#"write-set = ["crates/toy/src/shape.rs", "crates/toy/src/shape/tests.rs"]"#),
        gx_row("b", true, r#"write-set = ["crates/toy/src/user.rs"]"#),
        gx_row("c", true, r#"tests = ["crates/toy/src/shape/tests.rs"]"#),
        row_fields("e", &["write-set", "section"], &[r#"section = "2""#, four]),
        row_fields("f", &["write-set", "section"], &[r#"section = "2""#, r#"creates = ["crates/toy/tests/gadget.rs"]"#, r#"tests = ["crates/toy/tests/gadget.rs"]"#]),
    ];
    put(DESIGN_FILE, &gx_doc(&rows));
    put("contracts/t.toml", &format!("schema = 1\n\n[[contract]]\n{}\n", gx_row("t1", true, r#"write-set = ["crates/toy/src/lib.rs"]"#).join("\n")));
    let declaration = fs::read_to_string(repo.join(".vessel.toml")).unwrap_or_default();
    put(".vessel.toml", &format!("{declaration}contract-tables = [\"contracts/\"]\n{index}"));
    git(&repo, &["add", "-A"]);
    git(&repo, &["add", "-f", ".vessel.toml"]);
    git(&repo, &["commit", "-q", "-m", "gadget"]);
    let place = IdxPlace { repo, state };
    assert!(fs::write(place.state.join("idx.scip"), gx_scip_bytes()).is_ok(), "SCIP の fixture を書ける");
    assert!(fs::write(place.state.join("idx.roles"), gx_roles_text()).is_ok(), "一致の fixture を書ける");
    assert!(fs::create_dir_all(place.bin()).is_ok(), "偽の command の dir を作れる");
    idxb_script(&place, (IDXB_SCIP, "scip"), &format!("cp '{}' \"$2\"\n", place.state.join("idx.scip").display()));
    idxb_script(&place, (IDXB_ROLES, "roles"), &format!("cat '{}'\n", place.state.join("idx.roles").display()));
    place
}

impl IdxPlace {
    /// `pipe index show` を撃つ（`extra` は `--row` と `--item` などの引数）。
    fn show(&self, extra: &[&str]) -> Output {
        let (state, repo) = (self.state.display().to_string(), self.repo.display().to_string());
        let mut args = vec!["index", "show", "--state-dir", state.as_str(), "--repo", repo.as_str()];
        args.extend(extra);
        run_pipe_with_path(&self.path(), &args)
    }
}

/// 項目 `item` の塊（頭の行から次の項目の頭の前まで）。
fn gx_block(text: &str, item: &str) -> Vec<String> {
    let head = format!("- {item}: ");
    let mut lines = text.lines().skip_while(|line| !line.starts_with(&head));
    lines.next().map(str::to_owned).into_iter().chain(lines.take_while(|line| !line.starts_with("- ")).map(str::to_owned)).collect()
}

/// 塊の頭の行の `name=<値>` の値（`refs=14(外6)` は `14(外6)`）。
fn gx_field(block: &[String], name: &str) -> String {
    let head = block.first().cloned().unwrap_or_default();
    head.split_whitespace().find_map(|word| word.strip_prefix(&format!("{name}="))).unwrap_or_default().to_owned()
}

/// 塊の中の列 `name` の site の行（4 字下げ・先頭の空白を落とし昇順）。
fn gx_sites(block: &[String], name: &str) -> Vec<String> {
    let head = format!("  {name}:");
    let mut sites: Vec<String> = block
        .iter()
        .skip_while(|line| !line.starts_with(&head))
        .skip(1)
        .take_while(|line| line.starts_with("    "))
        .map(|line| line.trim().to_owned())
        .collect();
    sites.sort();
    sites
}

/// site の行の先頭の `<file>:<行>`（昇順）。
fn gx_places_of(block: &[String], name: &str) -> Vec<String> {
    gx_sites(block, name).iter().filter_map(|line| line.split(' ').next()).map(str::to_owned).collect()
}

/// 項目の見出し（`- <項目>: ` の頭の行の項目・出力の順）。
fn gx_items(text: &str) -> Vec<String> {
    text.lines().filter_map(|line| line.strip_prefix("- ")).filter_map(|rest| rest.split_once(": ")).map(|(item, _)| item.to_owned()).collect()
}

/// 行（1 始まり）の番号の列を `<file>:<行>` にする。
fn gx_places(file: &str, lines: &[usize]) -> Vec<String> {
    lines.iter().map(|line| format!("{file}:{line}")).collect()
}

/// 本文の中で語 `word` を語の境界で含む行の数。
fn gx_word_lines(body: &str, word: &str) -> usize {
    let word_char = |found: char| found.is_alphanumeric() || found == '_';
    let holds = |line: &str| {
        line.match_indices(word).any(|(at, _)| {
            let before = line.get(..at).and_then(|head| head.chars().next_back());
            let after = line.get(at + word.len()..).and_then(|tail| tail.chars().next());
            !before.is_some_and(word_char) && !after.is_some_and(word_char)
        })
    };
    body.lines().filter(|line| holds(line)).count()
}

/// 偽の宣言で組んだ索引の上の `show` の stdout（rc 0 でない周は落とす）。
fn gx_shown(place: &IdxPlace, extra: &[&str]) -> String {
    let out = place.show(extra);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "show は rc 0: {}", told_index(&out));
    stdout_of(&out)
}

/// 昇順に並べ直した列。
fn gx_sorted(mut found: Vec<String>) -> Vec<String> {
    found.sort();
    found
}

/// (a) `--item` は項目を symbol に解き、7 列の件数と site（本体と test の別・file の数）と母集団の 3 値を出す: 別名・`Self` の literal・glob・
/// 再輸出・doc の link・文字列の取り込み越しの site を含み、別 module の同名の型の site を数えず、toml に現れる名を outside-index に数える。
/// 解けない項目は 0 件・同名が 2 つの fn は ambiguous:2 と候補の定義の site。
#[test]
fn pipe_index_show_item_counts_seven_columns_and_the_population_of_a_resolved_symbol() {
    let place = gx_place(true);
    let shown = gx_shown(&place, &["--item", "crate::shape::Gadget", "--item", "crate::other::Gadget", "--item", "crate::shape::make", "--item", "crate::nothing::Here", "--item", "run"]);
    let gadget = gx_block(&shown, "crate::shape::Gadget");
    gx_check_sites(&gadget);
    gx_check_census(&place, &gadget);
    let other = gx_block(&shown, "crate::other::Gadget");
    assert_eq!(gx_places_of(&other, "refs"), gx_places(GX_OTHER_RS, &[2]), "別 module の同名の型は自分の site だけ: {shown}");
    let make = gx_block(&shown, "crate::shape::make");
    assert_eq!(gx_sites(&make, "callers"), [format!("{GX_TESTS_RS}:1 test builds"), format!("{GX_USER_RS}:3 assemble")], "callers は call の site を囲む定義: {make:?}");
    assert!(make.iter().any(|line| line.starts_with("  callers: 本体 1・test 1")), "{make:?}");
    let none = gx_block(&shown, "crate::nothing::Here");
    assert!(none.first().is_some_and(|line| line.contains("unresolved refs=0 callers=0 literals=0 patterns=0 teeth=0 vis=0 ")), "解けない項目は 0 件: {none:?}");
    gx_check_ambiguous(&gx_block(&shown, "run"));
    clean(&[&place.repo, &place.state]);
}

/// (a) の 6 列の site の集合と 7 列の件数。
fn gx_check_sites(gadget: &[String]) {
    let want = [gx_places(GX_SHAPE_RS, &[1, 3, 4, 5, 8, 9, 11]), gx_places(GX_TESTS_RS, &[1]), gx_places(GX_USER_RS, &[1, 3, 3, 4, 6]), gx_places(GX_LIB_RS, &[1])].concat();
    assert_eq!(gx_places_of(gadget, "refs"), gx_sorted(want), "refs の site の集合（別名・Self・glob・再輸出・doc の link・文字列の取り込み越し・別 module の同名の型は無い）: {gadget:?}");
    assert!(gadget.iter().any(|line| line.starts_with("  refs: 本体 13・test 1・file 4")), "本体と test の別と file の数: {gadget:?}");
    let literals = [gx_places(GX_SHAPE_RS, &[5, 9]), gx_places(GX_TESTS_RS, &[1]), gx_places(GX_USER_RS, &[3])].concat();
    assert_eq!(gx_places_of(gadget, "literals"), gx_sorted(literals), "Self と別名の literal を含む: {gadget:?}");
    assert_eq!(gx_places_of(gadget, "patterns"), gx_places(GX_USER_RS, &[6]), "pattern: {gadget:?}");
    assert_eq!(gx_places_of(gadget, "vis"), gx_places(GX_LIB_RS, &[1]), "再輸出の site: {gadget:?}");
    assert_eq!(gx_sites(gadget, "teeth"), [format!("{GX_TESTS_RS}:1 test builds")], "teeth は test の関数の名と site: {gadget:?}");
    assert!(gadget.iter().any(|line| line == "  vis: 定義 pub ← shape=pub(crate)"), "可視性と親 module の可視性: {gadget:?}");
    let counts: Vec<String> = ["refs", "callers", "literals", "patterns", "teeth", "vis", "rows"].iter().map(|name| gx_field(gadget, name)).collect();
    assert_eq!(counts, ["14", "0", "4", "1", "1", "1", "4"], "7 列の件数（rows は a・b・c と contracts/t.toml の t1）: {gadget:?}");
}

/// (a) の母集団の 3 値（text・indexed・outside-index と path）。
fn gx_check_census(place: &IdxPlace, gadget: &[String]) {
    let doc = fs::read_to_string(place.repo.join(DESIGN_FILE)).unwrap_or_default();
    let toml = fs::read_to_string(place.repo.join("contracts/t.toml")).unwrap_or_default();
    let beyond = gx_word_lines(GX_TOML, "Gadget") + gx_word_lines(&doc, "Gadget") + gx_word_lines(&toml, "Gadget");
    let inside: usize = [GX_SHAPE, GX_TESTS, GX_USER, GX_LIB, GX_OTHER].iter().map(|body| gx_word_lines(body, "Gadget")).sum();
    assert_eq!(inside, 13, "索引の中の file で語が現れる行（別 module の同名の型の 2 行を含む）");
    let census = (gx_field(gadget, "text"), gx_field(gadget, "indexed"), gx_field(gadget, "outside-index"));
    assert_eq!(census, ((inside + beyond).to_string(), "11".to_owned(), beyond.to_string()), "母集団の 3 値: {gadget:?}");
    let outside = gadget.iter().find(|line| line.starts_with("  outside-index: ")).cloned().unwrap_or_default();
    for path in [GX_TOML_PATH, "contracts/t.toml", DESIGN_FILE] {
        assert!(outside.contains(path), "索引の外の file の path: {outside}");
    }
    assert!(!outside.contains(".rs"), "索引の中の file は外に数えない: {outside}");
}

/// (a) の同名が 2 つの fn（ambiguous:2 と候補の定義の site）。
fn gx_check_ambiguous(many: &[String]) {
    assert!(many.first().is_some_and(|line| line.starts_with("- run: ambiguous:2 ")), "同名が 2 つの fn: {many:?}");
    let candidates: Vec<&String> = many.iter().filter(|line| line.starts_with("  候補: ")).collect();
    let names = |place: String| candidates.iter().any(|line| line.contains(&place));
    assert!(candidates.len() == 2 && names(format!("{GX_USER_RS}:9")) && names(format!("{GX_OTHER_RS}:3")), "候補の定義の site: {many:?}");
}

/// 項目の頭の行の 6 列の値（`refs`・`callers`・`literals`・`patterns`・`teeth`・`vis`）。
fn gx_six(block: &[String]) -> Vec<String> {
    ["refs", "callers", "literals", "patterns", "teeth", "vis"].iter().map(|name| gx_field(block, name)).collect()
}

/// (b) `--row` は行の touches と節の散文の名指しを項目にし（見出しの列は touches と `section_symbols` の列と集合で等しい・散文の可視性・
/// 引数付きの予約語の呼び出し・tuple variant の構築・touches の型の variant は項目にならず、`crate::` の頭の無い型の path 形と fn 形は項目になる）、
/// site の file が行の欄 write-set の外なら `外` を付けて列ごとに外の件数を出し、ほかの行の閉包を広げる file を rows 列に出す。`key` で
/// `contracts/` を名乗る toy の `contracts/t.toml` の行も引け rows 列に数える。欄 write-set を持たず tests の欄で導く行は `外` を 1 つも付けず
/// `write-set=derived` の 1 行を持つ（同じ site が欄 write-set を持つ行では `外` を持つことを先に確かめる）。
#[test]
fn pipe_index_show_row_marks_outside_sites_and_derived_rows_carry_no_mark() {
    let place = gx_place(true);
    let shown = gx_shown(&place, &["--row", "docs/design/toy.md#a"]);
    let touches = vec!["crate::shape::Gadget".to_owned()];
    let mut want = touches.clone();
    for name in vessel::pipe::closure::section_symbols(&[GX_PROSE], &touches) {
        if !want.contains(&name) {
            want.push(name);
        }
    }
    let got = gx_items(&shown);
    assert_eq!(gx_sorted(got.clone()), gx_sorted(want), "見出しの列は touches と section_symbols の列と集合で等しい: {shown}");
    assert_eq!(got, ["crate::shape::Gadget", "crate::shape::make", "shape::Gadget", "describe", "ghost", "run"], "touches が先・節の名指しは本文の順: {shown}");
    for prose in ["pub(crate)", "if (a)", "match(x)", "Some(x)", "crate::shape::", "crate::shape::*", "Gadget::new"] {
        assert!(!got.iter().any(|item| item == prose), "{prose} は項目にならない: {got:?}");
    }
    gx_check_marks(&shown);
    assert!(!shown.starts_with("write-set=derived"), "欄を持つ行に derived の行は無い");
    // `contracts/t.toml` の行も引け、rows 列は a・b・c を数える。
    let declared = gx_shown(&place, &["--row", "contracts/t.toml#t1"]);
    let one = gx_block(&declared, "crate::shape::Gadget");
    assert_eq!((gx_field(&one, "rows"), gx_six(&one)), ("3".to_owned(), ["14(外13)", "0(外0)", "4(外4)", "1(外1)", "1(外1)", "1(外0)"].map(str::to_owned).to_vec()), "{one:?}");
    // 欄 write-set を持たず tests の欄で導く行（c）は外を 1 つも付けず derived の 1 行を持つ。
    let derived = gx_shown(&place, &["--row", "docs/design/toy.md#c"]);
    assert_eq!(derived.lines().next(), Some("write-set=derived"), "項目の前に derived の 1 行: {derived}");
    let block = gx_block(&derived, "crate::shape::Gadget");
    assert_eq!(gx_six(&block), ["14", "0", "4", "1", "1", "1"], "外の件数を出さない: {block:?}");
    assert!(gx_sites(&block, "refs").iter().all(|line| !line.ends_with(" 外")), "外の印を付けない: {block:?}");
    assert!(block.iter().any(|line| line.starts_with("  refs: 本体 13・test 1・file 4") && !line.contains('外')), "{block:?}");
    assert_eq!(gx_items(&derived), got, "項目は同じ節の名指し: {derived}");
    clean(&[&place.repo, &place.state]);
}

/// (b) の外の印と列ごとの外の件数と rows 列（行 a の write-set は shape.rs と shape/tests.rs）。
fn gx_check_marks(shown: &str) {
    let gadget = gx_block(shown, "crate::shape::Gadget");
    assert_eq!(gx_six(&gadget), ["14(外6)", "0(外0)", "4(外1)", "1(外1)", "1(外0)", "1(外1)"], "列ごとの外の件数: {gadget:?}");
    let marked: Vec<String> = gx_sites(&gadget, "refs").into_iter().filter(|line| line.ends_with(" 外")).filter_map(|line| line.split(' ').next().map(str::to_owned)).collect();
    assert_eq!(marked, gx_sorted([gx_places(GX_USER_RS, &[1, 3, 3, 4, 6]), gx_places(GX_LIB_RS, &[1])].concat()), "外の印は write-set の外の file の site だけ: {gadget:?}");
    assert!(gadget.iter().any(|line| line.starts_with("  refs: 本体 13・test 1・file 4・外 6")), "{gadget:?}");
    assert_eq!(gx_field(&gadget, "rows"), "3", "rows 列は自分を除く b・c と contracts/t.toml の t1: {gadget:?}");
    let rows: Vec<&str> = gadget.iter().filter(|line| line.starts_with("    ") && line.contains('#')).map(|line| line.trim()).collect();
    let listed = [
        "contracts/t.toml#t1 広げる: crates/toy/src/shape.rs, crates/toy/src/shape/tests.rs, crates/toy/src/user.rs",
        "docs/design/toy.md#b 広げる: crates/toy/src/lib.rs, crates/toy/src/shape.rs, crates/toy/src/shape/tests.rs",
        "docs/design/toy.md#c write-set=derived",
    ];
    assert_eq!(rows, listed, "ほかの行の閉包を広げる file（欄を持たない行は derived）: {gadget:?}");
    let make = gx_block(shown, "crate::shape::make");
    assert_eq!(gx_six(&make), ["2(外1)", "2(外1)", "0(外0)", "0(外0)", "1(外0)", "0(外0)"], "{make:?}");
    assert!(gx_block(shown, "ghost").first().is_some_and(|line| line.contains("unresolved refs=0(外0) ")), "解けない項目は 0 件: {shown}");
    assert!(gx_block(shown, "run").first().is_some_and(|line| line.starts_with("- run: ambiguous:2 ")), "{shown}");
}

/// (c) 索引の無い周の show は偽の宣言を 1 回撃って表を出し（2 回目は撃たない）、生きた持ち主の印の周は command を撃たず子の終わりまで待ち
/// （経過が子の sleep 以上）失敗の記録を `index=unavailable:<語>` で出し、rc 1 の偽の宣言の周は `index=unavailable:rc`・2 key の無い repo は
/// `index=unavailable:undeclared` で、どれも rc 0。片方の key だけの repo と引けない行・解けない ref は rc 2、build に `--item` か `--row` を渡すと
/// stderr が未知の引数を名乗って rc 2。
#[test]
fn pipe_index_show_assembles_waits_for_the_owner_and_names_what_it_cannot_show() {
    let place = gx_place(true);
    let first = gx_shown(&place, &["--item", "crate::shape::Gadget"]);
    assert!(first.starts_with("- crate::shape::Gadget: resolved refs=14 "), "索引の無い周も表を出す: {first}");
    assert_eq!((place.calls("scip"), place.calls("roles")), (1, 1), "偽の宣言を 1 回撃つ");
    assert_eq!(gx_shown(&place, &["--item", "crate::shape::Gadget"]), first, "2 回目は撃たず同じ表");
    assert_eq!(place.calls("scip"), 1, "撃たない");
    let key = place.names().iter().find_map(|name| name.strip_suffix(".tsv").map(str::to_owned)).unwrap_or_default();
    for suffix in ["tsv", "rec"] {
        assert!(fs::remove_file(place.dir().join(format!("{key}.{suffix}"))).is_ok(), "{suffix} を外す");
    }
    assert!(fs::write(place.dir().join(format!("{key}.rec")), "schema=1\nfailed=rc\n").is_ok(), "失敗の記録を置く");
    let started = Instant::now();
    assert!(fs::write(place.dir().join(format!("{key}.lock")), format!("{}\n", idxb_sleeper())).is_ok(), "子の印を置く");
    let waited = place.show(&["--item", "crate::shape::Gadget"]);
    assert!(started.elapsed() >= Duration::from_secs(1), "子の終わりまで待つ: {:?}", started.elapsed());
    assert_eq!((waited.status.code(), stdout_of(&waited)), (Some(i32::from(RC_OK)), "index=unavailable:rc\n".to_owned()), "{}", told_index(&waited));
    assert_eq!(place.calls("scip"), 1, "待つ周は撃たない（回数は増えない）");
    clean(&[&place.repo, &place.state]);
    gx_check_unavailable();
    gx_check_refusals();
}

/// (c) の組めない周（rc 1 の偽の宣言）と 2 key の無い repo は `index=unavailable:<語>` の 1 行で rc 0。
fn gx_check_unavailable() {
    let failing = gx_place(true);
    idxb_script(&failing, (IDXB_SCIP, "scip"), "exit 1\n");
    let failed = failing.show(&["--item", "crate::shape::Gadget"]);
    assert_eq!((failed.status.code(), stdout_of(&failed)), (Some(i32::from(RC_OK)), "index=unavailable:rc\n".to_owned()), "{}", told_index(&failed));
    clean(&[&failing.repo, &failing.state]);
    let bare = gx_place(false);
    let undeclared = bare.show(&["--item", "crate::shape::Gadget", "--row", "docs/design/toy.md#a"]);
    assert_eq!((undeclared.status.code(), stdout_of(&undeclared)), (Some(i32::from(RC_OK)), "index=unavailable:undeclared\n".to_owned()), "{}", told_index(&undeclared));
    assert_eq!((bare.calls("scip"), bare.calls("roles"), bare.dir().exists()), (0, 0, false), "撃たず置き場も作らない");
    clean(&[&bare.repo, &bare.state]);
}

/// (c) の rc 2（片方の key だけの宣言・引けない行・解けない ref）と build に渡した `--item` / `--row` の使い方の誤り。
fn gx_check_refusals() {
    let decl ="index-scip = [\"index-fake-scip {tree} {out}\"]\n";
    let one = gx_place_with(decl);
    let line = fs::read_to_string(one.repo.join(".vessel.toml")).unwrap_or_default().lines().position(|text| text.starts_with("index-scip")).map_or(0, |at| at + 1);
    let half = one.show(&["--item", "crate::shape::Gadget"]);
    assert_eq!(half.status.code(), Some(i32::from(RC_BROKEN)), "片方の key だけは rc 2: {}", told_index(&half));
    assert!(stderr_of(&half).contains("index-roles") && stderr_of(&half).contains(&format!("line={line}")), "key の名と行番号: {}", stderr_of(&half));
    assert_eq!(one.calls("scip"), 0, "撃たない");
    clean(&[&one.repo, &one.state]);
    let place = gx_place(true);
    let missing = place.show(&["--row", "docs/design/toy.md#zz"]);
    assert_eq!(missing.status.code(), Some(i32::from(RC_BROKEN)), "引けない行は rc 2: {}", told_index(&missing));
    assert!(stderr_of(&missing).contains("docs/design/toy.md#zz"), "{}", stderr_of(&missing));
    let no_ref = place.show(&["--ref", "no-such-ref", "--item", "x"]);
    assert_eq!(no_ref.status.code(), Some(i32::from(RC_BROKEN)), "解けない ref は rc 2: {}", told_index(&no_ref));
    let nothing = place.show(&[]);
    assert_eq!(nothing.status.code(), Some(i32::from(RC_REFUSED)), "項目が 1 つも無い周は断る: {}", told_index(&nothing));
    for flag in ["--item", "--row"] {
        let build = place.build(None, &[flag, "crate::shape::Gadget"]);
        assert_eq!(build.status.code(), Some(i32::from(RC_BROKEN)), "build に {flag} は使い方の誤り: {}", told_index(&build));
        assert!(stderr_of(&build).contains(&format!("未知の引数 {flag}")), "未知の引数を名乗る: {}", stderr_of(&build));
    }
    clean(&[&place.repo, &place.state]);
}

/// 審査の材料の dir（偽 PASS の lens で受付から審査まで通した便・行 `row` の pointer は節 2 の行）。
fn gx_review_dir(place: &IdxPlace, row: &str) -> PathBuf {
    let out = intake_raw(&place.repo, &place.state, &format!("docs/design/toy.md#{row}"), &format!("s2-{row}"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "受付から審査まで通る: {}", told_index(&out));
    review_dir(&place.state, &run_id_of(&out))
}

/// 材料の dir の file の名の列から index.txt を除いた列。
fn gx_others(dir: &Path) -> Vec<String> {
    dir_names(dir).into_iter().filter(|name| name != "index.txt").collect()
}

/// (d) 偽の lens で受付から審査まで通した便の材料の dir の index.txt は、同じ ref の `show --row` の出力と byte で等しい（write-set は表の行の欄から
/// 読む＝欄を持たず tests の欄で導く行は契約の写しが導出値の write-set を持っても `write-set=derived` の 1 行）。索引を作らない周は
/// `index=unavailable:absent` の 1 行・撃ち中の持ち主が生きている周は `building`・宣言の無い repo は file を置かず、材料の file の列は今のまま。
#[test]
fn pipe_index_show_material_equals_the_show_row_output_and_says_why_when_unavailable() {
    let place = gx_place(true);
    let built = place.build(None, &[]);
    assert_eq!(idxb_word(&idxb_line(&built)), "built", "{}", told_index(&built));
    let dir = gx_review_dir(&place, "e");
    let written = fs::read(dir.join("index.txt")).unwrap_or_default();
    let shown = place.show(&["--row", "docs/design/toy.md#e"]);
    assert_eq!(shown.status.code(), Some(i32::from(RC_OK)), "{}", told_index(&shown));
    assert_eq!(written, shown.stdout, "index.txt は show --row と byte で等しい: {}", String::from_utf8_lossy(&written));
    assert_eq!(gx_items(&stdout_of(&shown)), ["crate::shape::Gadget", "describe"], "touches の無い行は節の名指しだけ: {}", stdout_of(&shown));
    let names = gx_others(&dir);
    let derived = gx_review_dir(&place, "f");
    let text = fs::read_to_string(derived.join("index.txt")).unwrap_or_default();
    assert_eq!(text.lines().next(), Some("write-set=derived"), "契約の写しの write-set は読まない: {text}");
    assert!(!text.contains('外'), "derived の行は外を付けない: {text}");
    clean(&[&place.repo, &place.state]);
    gx_check_material_unavailable(&names);
}

/// (d) の索引を作らない周（absent）・撃ち中の持ち主が生きている周（building）・宣言の無い repo（file を置かず材料の列は今のまま）。
fn gx_check_material_unavailable(names: &[String]) {
    let absent = gx_place(true);
    let dir_absent = gx_review_dir(&absent, "e");
    assert_eq!(fs::read_to_string(dir_absent.join("index.txt")).unwrap_or_default(), "index=unavailable:absent\n", "索引を作らない周");
    assert_eq!(absent.calls("scip"), 0, "審査は索引を撃たない");
    assert_eq!(gx_others(&dir_absent), names, "索引を名乗る周も index.txt のほかの列は同じ");
    clean(&[&absent.repo, &absent.state]);
    let busy = gx_place(true);
    let key = idxb_field(&idxb_line(&busy.build(None, &[])), "key");
    for suffix in ["tsv", "rec"] {
        assert!(fs::remove_file(busy.dir().join(format!("{key}.{suffix}"))).is_ok(), "{suffix} を外す");
    }
    let spawned = Command::new("sleep").arg("20").spawn();
    assert!(spawned.is_ok(), "sleep を起こせる");
    let Ok(mut child) = spawned else {
        return;
    };
    assert!(fs::write(busy.dir().join(format!("{key}.lock")), format!("{}\n", child.id())).is_ok(), "生きた持ち主の印を置く");
    let dir_busy = gx_review_dir(&busy, "e");
    assert!(child.kill().is_ok() && child.wait().is_ok(), "子を止める");
    assert_eq!(fs::read_to_string(dir_busy.join("index.txt")).unwrap_or_default(), "index=unavailable:building\n", "作り中の周");
    clean(&[&busy.repo, &busy.state]);
    let plain = gx_place(false);
    let dir_plain = gx_review_dir(&plain, "e");
    assert!(!dir_plain.join("index.txt").exists(), "宣言の無い repo は file を置かない");
    assert_eq!(dir_names(&dir_plain), names, "材料の file の列は今のまま");
    clean(&[&plain.repo, &plain.state]);
}
