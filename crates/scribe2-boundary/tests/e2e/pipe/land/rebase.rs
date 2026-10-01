// flip-check: moved s2-07l.684
//! rebase と onto の族の歯（接頭辞 `pipe_land_rebase_` / `pipe_land_onto_`・設計 docs/design/carry-prep.md §10 行 l・親 `tests/e2e/pipe/land.rs` の helper を `use super::*` で使う）。

use super::*;

#[test]
fn pipe_land_rebase_without_lens_stops_inconclusive_and_keeps_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    // gate の後に main が別便で進む。CAS の old が動いた＝そのままでは land できない。
    let moved = commit_other_in_scope(&repo);
    // `--lens` 無しの land（審査が残した写し `lens.toml` も外す＝写しも flag も無い世界・§26）: 追随（rebase）は
    // 済むが撃ち直しの gate は lens を得られず INCONCLUSIVE＝**land しない**（測れなかったを通ったに化けさせない・
    // FR14 で測り直せる）。
    fs::remove_file(lens_record_of(&state, &id)).expect("審査の写しを外せる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "lens 無しの撃ち直しは rc 3: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("rebase={base}..{moved}")), "追随は済む: {stdout}");
    assert!(stdout.contains("verdict=INCONCLUSIVE"), "撃ち直しの判定行: {stdout}");
    assert!(!stdout.contains("landed="), "land していない: {stdout}");
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        moved,
        "断った周は main を動かさない"
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated（測り直せる側）");
    clean(&[&repo, &state]);
}

/// worktree が clean でない周は **rebase を撃たずに rc 1 で何も書かない**（設計 §5.4 (ii)）。
/// 汚れた木で rebase すると撃ち直しの precheck が `Failed` で終端し、回復可能だった便が閉じる。
#[test]
fn pipe_land_rebase_refuses_dirty_worktree_without_rebase() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let worktree = worktree_of(&repo, &id);
    let head_before = git(&worktree, &["rev-parse", "HEAD"]);
    fs::write(repo.join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    // gate の後に木が汚れた（未 commit の仕事が在る）。
    fs::write(worktree.join("dirty.txt"), "x\n").expect("汚せる");
    let before = count_but_turn(&state);
    fs::remove_file(&marker).expect("lens の marker を消せる");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "汚れた木は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("clean でない"), "理由: {}", stderr_of(&out));
    assert_eq!(count_but_turn(&state), before, "番の記帳のほかは何も書かない");
    assert!(!marker.exists(), "gate を撃ち直さない（lens は走らない）");
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head_before, "rebase を撃たない（HEAD 不変）");
    assert!(worktree.join("dirty.txt").exists(), "未 commit の仕事は残る");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated のまま（回復可能）");
    clean(&[&repo, &state]);
}

/// main が動いた便の追随（設計 §5.4・`s2-07l.119`）: **2 便を同じ base から起こし、1 本目を
/// land して main を動かした後、2 本目の land が rebase → gate の撃ち直し → 新 base で CAS**。
#[test]
fn pipe_land_rebase_follows_landed_sibling_and_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    // 行の commit が main を進めうるので、base は**行を置いた後**に読む（契約 (b)）。
    // 行は `two_gated_runs` が 1 回で commit するので、base はその後に読む（2 便で同じ 1 つ）。
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    // 1 本目が land して main が動く（2 本目の base は置き去り）。
    let first = land_solo(&repo, &state, &id_a);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "1 本目の land: {}", stderr_of(&first));
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(moved, base, "main が動いている");
    fs::remove_file(&marker).expect("撃ち直しの前に lens の marker を消せる");

    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id_b, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "追随した land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(new, moved, "2 本目も land した");
    assert_eq!(git(&repo, &["rev-parse", &format!("{new}^")]), moved, "squash は動いた main の上に載る");
    assert_eq!(git(&repo, &["rev-list", "--count", &format!("{moved}..{new}")]), "1", "squash は 1 commit");
    assert_eq!(git(&repo, &["show", &format!("{new}:src/b.rs")]), "b", "2 本目の仕事が main に載る");
    assert_eq!(git(&repo, &["show", &format!("{new}:crates/toy/a.rs")]), "a", "1 本目の仕事も残る");
    assert!(marker.exists(), "gate を撃ち直した（lens が再び走った）");
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("run={id_b} rebase={base}..{moved}")), "rebase= token: {stdout}");
    assert!(stdout.contains("verdict=PASS"), "撃ち直しの判定行: {stdout}");
    assert!(stdout.contains(&format!("landed={new}")), "landed=: {stdout}");
    assert_follow_events(&state, &base, &moved, &new);
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 便の `Gated` の detail のうち `verdict:` で始まるもの（物理順・番の記帳 `turn:taken` は除く）。
fn verdict_details(state: &Path, id: &str) -> Vec<String> {
    gated_details(state, id).into_iter().filter(|detail| detail.starts_with("verdict:")).collect()
}

/// (j) 兄弟の便の着地で main が動いた便を `--rules` つきの land で追随させると、`verdict:` で始まる `Gated` の detail は最初の gate の
/// `rules:embedded` と、再 gate の `rules:<その file の blob id>`（land が受けた `--rules` を gate へ渡す）の 2 件。検出線の面に触れない
/// commit で main が動いた便の引き継ぎ（`regate=skipped`）の detail は `verdict:PASS` のまま（`rules:` の項を持たない・manifest を読んで
/// 判じた周ではない）。base は再 gate の detail が `verdict:PASS` だけ → RED。
#[test]
fn pipe_land_rules_source_regate_carries_the_blob_id_and_the_skip_carries_none() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let first = land_solo(&repo, &state, &id_a);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "1 本目の land: {}", stderr_of(&first));
    let rules = write_rules(&state, "rules-follow.toml", 1, 1_000_000).display().to_string();
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules, "--lens", &lens]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "追随した land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=PASS") && !stdout_of(&out).contains("regate=skipped"), "再 gate を撃った: {}", stdout_of(&out));
    let blob = rules_source(&repo, Some(&rules));
    assert_eq!(
        verdict_details(&state, &id_b),
        ["verdict:PASS,rules:embedded".to_owned(), format!("verdict:PASS,rules:{blob}")],
        "最初の gate は embedded・再 gate は land が受けた file の blob id"
    );
    clean(&[&repo, &state]);

    // 面の外（`notes/` だけ）で main が動いた便は再 gate を省いて前周の PASS を引き継ぐ（`--rules` を渡しても項を足さない）。
    let (repo, state) = repo_with_state();
    let seed = git(&repo, &["rev-parse", "refs/heads/main"]);
    let path = write_contract(&repo, &[], &[]);
    let base = commit_main_file(&repo, "notes/pre.txt", "pre\n");
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let moved = rewrite_main_from(&repo, &seed, "notes/other.txt", "other\n");
    assert_diverged(&repo, &base, &moved, &seed);
    let rules = write_rules(&state, "rules-skip.toml", 1, 1_000_000).display().to_string();
    let out = land_extra(&repo, &state, &id, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "面の外の追随は再 gate 無しで land: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("regate=skipped"), "再 gate を省いて引き継いだ: {}", stdout_of(&out));
    assert_eq!(
        verdict_details(&state, &id),
        ["verdict:PASS,rules:embedded".to_owned(), "verdict:PASS".to_owned()],
        "引き継ぎの記帳の字は不変（rules: を持たない）"
    );
    clean(&[&repo, &state]);
}

/// 同一変更の 2 便: 1 本目が land した後の 2 本目は rebase で commit が 0 本になり、
/// **gate を撃ち直さず `Failed detail=rebase-empty`**（main は 1 本目の sha のまま・`s2-07l.125`）。
#[test]
fn pipe_land_rebase_empty_fails_closed_without_regate() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_b, landed) = gated_run_whose_change_is_already_on_main(&repo, &state, &marker);
    fs::remove_file(&marker).expect("lens の marker を消せる");
    let before = count_but_turn(&state);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id_b, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "空になった便は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("既に main に在る"), "理由: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(stderr.contains(&id_b) && stderr.contains("base=") && stderr.contains("main="), "run / base / main を名乗る: {stderr}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), landed, "main は 1 本目の sha のまま");
    assert!(!marker.exists(), "gate を撃ち直さない（lens は走らない）");
    assert_eq!(count_but_turn(&state), before + 1, "番の記帳のほかに残す event は Failed の 1 本だけ");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let last = log.lines().last().unwrap_or_default();
    assert!(last.contains("\"stage\":\"Failed\"") && last.contains("rebase-empty"), "末尾: {last}");
    assert!(!log.contains("rebase:"), "追随の event は書かない（追随の先が無い）\n{log}");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Failed"), "段は Failed");
    clean(&[&repo, &state]);
}

/// 読めない周は 0 に読み替えない: rebase の後の `rev-list --count` が落ちる周は `rebase-empty` に**倒さず**
/// 従来どおり追随の event を残して撃ち直しの precheck へ流す（fail-closed の向きは不変）。
#[test]
fn pipe_land_rebase_empty_does_not_treat_unreadable_count_as_zero() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let first = land_solo(&repo, &state, &id_a);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "1 本目の land: {}", stderr_of(&first));
    let landed = git(&repo, &["rev-parse", "refs/heads/main"]);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // `rev-list --count` だけを落とす git を前に置く。追随の rebase 自体は通る。
    let out = land_once_with_git_shim(&repo, &state, &id_b, " rev-list --count ", Some(&lens));
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "読めない周に land はしない: {}", stdout_of(&out));
    assert!(!stderr_of(&out).contains("既に main に在る"), "読めないを 0 に読み替えない: {}", stderr_of(&out));
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(!log.contains("rebase-empty"), "rebase-empty を名乗らない\n{log}");
    assert!(log.contains("rebase:"), "追随の event は残る（従来の経路へ流れた）\n{log}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), landed, "main は動かない");
    clean(&[&repo, &state]);
}

/// 負例: 変更が **異なる** 2 便は従来どおり追随して Landed（`rebase-empty` に倒れない）。
#[test]
fn pipe_land_rebase_empty_does_not_fire_for_distinct_changes() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let first = land_solo(&repo, &state, &id_a);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "1 本目の land: {}", stderr_of(&first));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id_b, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "異なる変更は追随して land: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("landed="), "{}", stdout_of(&out));
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(!log.contains("rebase-empty"), "rebase-empty は出ない\n{log}");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 追随の rebase が衝突した周は木を戻し、**便を終端にせず**衝突を記帳して止まる（`s2-07l.146`・
/// ADR-0019 §2.2）。この歯が pin するのは「**main は 1 byte も動かない**・木は衝突前へ戻る」で、
/// 起こし直しそのものは `pipe_follow_` の歯が測る。
///
/// `--runner` を渡さない `pipe land` は起こし直せないので rc 1 で断る——衝突の記帳
/// （`Implemented detail=rebase-conflict:…`）だけは残り、`pipe resume --runner` で続けられる。
#[test]
fn pipe_land_rebase_conflict_fails_closed_and_keeps_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let worktree = worktree_of(&repo, &id);
    let head_before = git(&worktree, &["rev-parse", "HEAD"]);
    let branch_before = git(&worktree, &["rev-parse", "--abbrev-ref", "HEAD"]);
    // 別便が **同じ file の同じ末尾** へ別の行を足す（runner は `echo x >> src/lib.rs`）。
    let lib = repo.join("src").join("lib.rs");
    let mut text = fs::read_to_string(&lib).expect("seed を読める");
    text.push_str("y\n");
    fs::write(&lib, text).expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "起こし直せない周は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("--runner が要る"), "理由: {}", stderr_of(&out));
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は 1 byte も動かない");
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head_before, "木は衝突前へ戻る");
    assert_eq!(git(&worktree, &["rev-parse", "--abbrev-ref", "HEAD"]), branch_before, "branch に居る（rebase 途中で detach していない）");
    assert!(git(&worktree, &["status", "--porcelain"]).is_empty(), "衝突の残骸が無い");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(log.contains(&format!("rebase-conflict:{base}..{moved}")), "衝突が event に残る\n{log}");
    assert!(!log.contains("\"Failed\""), "便を終端にしない\n{log}");
    assert!(!log.contains("\"detail\":\"rebase:"), "追随の event は書かない（追随できていない）\n{log}");
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段は Implemented（起こし直せる側）");
    clean(&[&repo, &state]);
}

/// 追随した後の gate の撃ち直しが FAIL なら **land しない**（`Gated` のまま・main は動いたまま）。
#[test]
fn pipe_land_rebase_regate_fail_keeps_gated_and_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let moved = commit_other_in_scope(&repo);
    let lens = fake_lens(&marker, &lens_verdict("FAIL"));
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "撃ち直し FAIL は gate の rc 1: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("rebase={base}..{moved}")), "追随は済んでいる: {stdout}");
    assert!(stdout.contains("verdict=FAIL"), "撃ち直しの判定行: {stdout}");
    assert!(!stdout.contains("landed="), "land していない: {stdout}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は撃ち直しの前のまま");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "FAIL", "verdict.json は撃ち直しの値");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated のまま");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(!log.contains("\"Landed\""), "Landed の event は無い\n{log}");
    // 同じ便をもう一度 land しても PASS でないので断る（撃ち直しの FAIL は終端）。
    let again = land_once(&repo, &state, &id);
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "FAIL のまま land はできない");
    assert!(stderr_of(&again).contains("PASS でない"), "理由: {}", stderr_of(&again));
    clean(&[&repo, &state]);
}

/// `resume` の Gated(PASS) → land も同じ経路で追随する（別口を作らない）。
#[test]
fn pipe_land_rebase_resume_from_gated_follows_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    fs::write(repo.join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "resume の追随は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-parse", &format!("{new}^")]), moved, "動いた main の上に載る");
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("rebase={base}..{moved}")), "rebase= token: {stdout}");
    assert!(stdout.contains(&format!("landed={new}")), "landed=: {stdout}");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// base が main の祖先でない周（main が巻き戻った / 分岐した）は追随の形が無い＝**rc 1 で
/// 何も書かない**（rebase も撃たない）。
#[test]
fn pipe_land_rebase_refuses_when_base_is_not_ancestor_of_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let worktree = worktree_of(&repo, &id);
    let head_before = git(&worktree, &["rev-parse", "HEAD"]);
    // 親を持たない commit を main に据える（便の base はその祖先でない）。
    let tree = git(&repo, &["rev-parse", "refs/heads/main^{tree}"]);
    let root = git(&repo, &["commit-tree", &tree, "-m", "diverged"]);
    git(&repo, &["update-ref", "refs/heads/main", &root]);
    let before = count_but_turn(&state);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "祖先でない base は rc 1");
    assert!(stderr_of(&out).contains("stale base"), "理由: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("祖先でない"), "理由の弁別: {}", stderr_of(&out));
    assert_eq!(count_but_turn(&state), before, "番の記帳のほかは何も書かない");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), root, "main は動かない");
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head_before, "worktree も動かない（rebase を撃たない）");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated のまま");
    clean(&[&repo, &state]);
}

/// (a) base が main の祖先でなく merge-base が在る便の land は `rebase --onto` で追随して `Landed`（設計 §38 (2)）:
/// squash の tree は便の commit **だけ**を運び（消えた commit の file は無い・新しい main の file は在る）、記帳は従来の
/// `rebase:<base>..<main>`、面の内で動いた main なので再 gate（偽 lens）も従来どおり撃つ。base（rc 1 `stale base`・段は
/// Gated・event 0 増）では RED。
#[test]
fn pipe_land_onto_follows_diverged_main_and_lands() {
    let (repo, state) = repo_with_state();
    let seed = git(&repo, &["rev-parse", "refs/heads/main"]);
    let path = write_contract(&repo, &[], &[]);
    // 便の base = seed の上の 1 commit（面の内）。この commit は後で main から消える。
    let base = commit_main_file(&repo, "crates/pre.txt", "pre\n");
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let moved = rewrite_main_from(&repo, &seed, "crates/other.txt", "other\n");
    assert_diverged(&repo, &base, &moved, &seed);
    // `--lens` 無しの land の再 gate は審査の写し（`lens.toml`・§26）の lens を起こす＝その marker で「走ったか」を測る。
    fs::remove_file(state.join(REVIEW_MARKER)).expect("審査の marker を消せる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "--onto で追随した land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-parse", &format!("{new}^")]), moved, "squash の親は新しい main");
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("run={id} rebase={base}..{moved}")), "追随の行は従来の形（範囲の 2 sha）: {stdout}");
    assert!(stdout.contains(&format!("landed={new}")), "landed=: {stdout}");
    let files = git(&repo, &["ls-tree", "-r", "--name-only", &new]);
    assert!(!files.contains("crates/pre.txt"), "消えた commit の file は運ばない: {files}");
    assert!(files.contains("crates/other.txt"), "新しい main の file は在る: {files}");
    assert_eq!(git(&repo, &["show", &format!("{new}:src/lib.rs")]), "// seed\nx", "便の変更だけが載る");
    assert!(
        stages(&state, &id).iter().any(|(stage, detail)| *stage == Some(Stage::Implemented)
            && detail.as_deref() == Some(format!("rebase:{base}..{moved}").as_str())),
        "記帳は従来の `rebase:<base>..<main>`: {:?}",
        stages(&state, &id)
    );
    assert!(state.join(REVIEW_MARKER).exists(), "面の内で動いた main の追随は再 gate を撃つ（写しの lens が走る）");
    assert!(stdout.contains("verdict=PASS") && !stdout.contains("regate=skipped"), "撃ち直しの判定行: {stdout}");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// (b) 同じ形で衝突する周は既存の `rebase-conflict:` の記帳と起こし直しの経路（字面不変）へ合流し、起こし直しの stdin の
/// 「追随」節は main と base の **2 sha** を持つ。`--onto <main> <base>` で rebase を通す stub の runner は消えた commit を
/// 運ばずに base を進め（`rebase:<base>..<main>`）、続きの gate → land で `Landed`。
#[test]
fn pipe_land_onto_conflict_restarts_the_runner_with_two_shas_and_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE_ONTO);
    let (id, base, moved) = diverged_conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "起こし直した周は rc 3: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={id} next=gate")), "次に撃つ段: {}", stdout_of(&out));
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は 1 件（字面不変）: {:?}", stages(&state, &id));
    assert!(
        stages(&state, &id).iter().any(|(stage, detail)| *stage == Some(Stage::Implemented)
            && detail.as_deref() == Some(format!("rebase-conflict:{base}..{moved}").as_str())),
        "衝突の detail は base と main を名乗る: {:?}",
        stages(&state, &id)
    );
    assert_follow_section_names_both(&stub_stdin(&state, 2), &moved, &base);
    assert!(
        stdout_of(&out).contains(&format!("run={id} rebase={base}..{moved}")),
        "runner が --onto で解いた木の base を器が main へ進める: {}",
        stdout_of(&out)
    );
    assert!(!mid_rebase(&repo, &id), "木は rebase の途中でない");
    fs::remove_file(&marker).ok();
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "新しい base の gate は PASS: {}", stderr_of(&gated));
    let landed = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-parse", &format!("{new}^")]), moved, "squash の親は新しい main");
    assert_eq!(git(&repo, &["show", &format!("{new}:src/lib.rs")]), "// seed\ny\nx", "新しい main の行と便の行が両方載る");
    let files = git(&repo, &["ls-tree", "-r", "--name-only", &new]);
    assert!(!files.contains("notes/pre.txt"), "消えた commit の file は運ばない: {files}");
    assert_eq!(stub_calls(&state), 2, "起こし直しは 1 回だけ");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// (b′) `--runner` の無い周は既存の rc 1（衝突の記帳だけ残り `resume --runner` で続く・木は衝突前へ戻る・main は動かない）。
#[test]
fn pipe_land_onto_conflict_without_runner_records_and_stops() {
    let (repo, state) = repo_with_state();
    let seed = git(&repo, &["rev-parse", "refs/heads/main"]);
    let path = write_contract(&repo, &[], &[]);
    let base = commit_main_file(&repo, "notes/pre.txt", "pre\n");
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let worktree = worktree_of(&repo, &id);
    let head_before = git(&worktree, &["rev-parse", "HEAD"]);
    git(&repo, &["reset", "-q", "--hard", &seed]);
    let moved = move_main_into_conflict(&repo);
    assert_diverged(&repo, &base, &moved, &seed);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = land_extra(&repo, &state, &id, &["--lens", &lens]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "起こし直せない周は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("--runner が要る"), "理由: {}", stderr_of(&out));
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は 1 件: {:?}", stages(&state, &id));
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は 1 byte も動かない");
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head_before, "木は衝突前へ戻る");
    assert!(git(&worktree, &["status", "--porcelain"]).is_empty(), "衝突の残骸が無い");
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段は Implemented（起こし直せる側）");
    clean(&[&repo, &state]);
}

/// (c) merge-base の無い main（親を持たない commit＝無関係な歴史）は従来どおり `stale base` の rc 1・event 0 増・rebase も
/// 撃たない（極性不変・fail-closed＝無関係な歴史へ便の commit を運ばない）。
#[test]
fn pipe_land_onto_unrelated_history_is_still_stale_base() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let worktree = worktree_of(&repo, &id);
    let head_before = git(&worktree, &["rev-parse", "HEAD"]);
    let tree = git(&repo, &["rev-parse", "refs/heads/main^{tree}"]);
    let root = git(&repo, &["commit-tree", &tree, "-m", "unrelated"]);
    git(&repo, &["update-ref", "refs/heads/main", &root]);
    assert_eq!(git(&repo, &["rev-list", "--count", &root]), "1", "main は親を持たない commit（merge-base は無い）");
    let before = count_but_turn(&state);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = land_extra(&repo, &state, &id, &["--lens", &lens]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "merge-base の無い main は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("stale base"), "断りの字面は従来どおり: {}", stderr_of(&out));
    assert_eq!(count_but_turn(&state), before, "番の記帳のほかは何も書かない（event 0 増）");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), root, "main は動かない");
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head_before, "worktree も動かない（rebase を撃たない）");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated のまま");
    clean(&[&repo, &state]);
}

/// (d) `--onto` の追随の後の `base_of_run` は main を返し（記帳の新しい側＝merge-base ではない）、再 gate の要否は §30 の判定の
/// まま——main との差分が検出線の面の外（`notes/` だけ）なら再 gate を省いて前周の PASS を引き継ぐ（lens は走らない・
/// `regate=skipped`）。
#[test]
fn pipe_land_onto_records_main_as_the_new_base_and_skips_regate_outside_scope() {
    use vessel::pipe::{base_of_run, Base};
    let (repo, state) = repo_with_state();
    let seed = git(&repo, &["rev-parse", "refs/heads/main"]);
    let path = write_contract(&repo, &[], &[]);
    let base = commit_main_file(&repo, "notes/pre.txt", "pre\n");
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let moved = rewrite_main_from(&repo, &seed, "notes/other.txt", "other\n");
    assert_diverged(&repo, &base, &moved, &seed);
    // gate の flag の lens と審査の写しの lens の marker を両方外す（どちらも起きない＝再 gate そのものが無い）。
    fs::remove_file(&marker).expect("lens の marker を消せる");
    fs::remove_file(state.join(REVIEW_MARKER)).expect("審査の marker を消せる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "面の外の追随は再 gate 無しで land: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("run={id} rebase={base}..{moved}")), "追随の行: {stdout}");
    assert!(stdout.contains("regate=skipped"), "再 gate を省いて引き継いだ: {stdout}");
    assert!(!marker.exists() && !state.join(REVIEW_MARKER).exists(), "lens は走らない");
    assert_eq!(base_of_run(&state, &id), Base::Known(moved.clone()), "新しい base は main（merge-base {seed} ではない）");
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-parse", &format!("{new}^")]), moved, "squash の親は新しい main");
    let files = git(&repo, &["ls-tree", "-r", "--name-only", &new]);
    assert!(!files.contains("notes/pre.txt") && files.contains("notes/other.txt"), "便の commit だけを運ぶ: {files}");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 宣言に key `crate-roots = ["nest/crates/"]` を足して commit する（`declared` の repo だけ・便を起こす前に呼ぶ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn commit_crate_roots(repo: &Path) {
    let text = fs::read_to_string(repo.join(".vessel.toml")).expect("宣言を読める");
    fs::write(repo.join(".vessel.toml"), format!("{text}crate-roots = [\"nest/crates/\"]\n")).expect("宣言を書ける");
    git(repo, &["add", "-f", ".vessel.toml"]);
    git(repo, &["commit", "-q", "-m", "crate-roots"]);
}

/// gate の後に main が `nest/crates/toy/src/other.rs` だけ動いた便を追随で land し、(stdout・lens の写しが走ったか) を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn land_after_nested_main_move(declared: bool) -> (String, bool) {
    let (repo, state) = repo_with_state();
    if declared {
        commit_crate_roots(&repo);
    }
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    commit_main_file(&repo, "nest/crates/toy/src/other.rs", "other\n");
    fs::remove_file(&marker).expect("lens の marker を消せる");
    fs::remove_file(state.join(REVIEW_MARKER)).expect("審査の marker を消せる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "追随した land は rc 0: {}", stderr_of(&out));
    let seen = (stdout_of(&out), state.join(REVIEW_MARKER).exists());
    clean(&[&repo, &state]);
    seen
}

/// 宣言した根の下だけ main が動いた追随は再 gate を撃つ（写しの lens が走り・`regate=skipped` が無い）。
#[test]
fn pipe_land_crate_roots_declared_root_move_fires_the_regate() {
    let (stdout, lens_ran) = land_after_nested_main_move(true);
    assert!(lens_ran, "宣言した根の下は面の内＝写しの lens が走る: {stdout}");
    assert!(stdout.contains("verdict=PASS") && !stdout.contains("regate=skipped"), "撃ち直しの判定行: {stdout}");
}

/// key の無い repo の同じ動きは今どおり `regate=skipped` で前周の PASS を引き継ぐ（対照）。
#[test]
fn pipe_land_crate_roots_without_the_key_the_same_move_skips_the_regate() {
    let (stdout, lens_ran) = land_after_nested_main_move(false);
    assert!(!lens_ran, "面の外＝lens は走らない: {stdout}");
    assert!(stdout.contains("regate=skipped"), "再 gate を省いて引き継いだ: {stdout}");
}

/// (e) 祖先である周（従来の追随）の起こし直しの「追随」節も同じ 2 sha の形で `--onto <main> <base>` を命じる（経路を 2 本に
/// しない）。既存の追随の歯（`$SHA` で素の rebase を撃つ stub）の期待は変えない＝節の 1 行目は不変。
#[test]
fn pipe_land_onto_ancestor_follow_section_names_both_shas() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    // fixture の形の確認: base は main の祖先（`git` helper は rc 0 を要求する）。
    git(&repo, &["merge-base", "--is-ancestor", &base, &moved]);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    assert_follow_section_names_both(&stub_stdin(&state, 2), &moved, &base);
    clean(&[&repo, &state]);
}
