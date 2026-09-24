// flip-check: moved s2-07l.264
// flip-check: moved s2-07l.295
// flip-check: retroactive s2-07l.417
//! land の歯: `pipe_land_` / `pipe_order_`（順番待ち）/ `pipe_follow_`（追随）/ `pipe_retire_`。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く（歯の本文は移しただけ・`s2-07l.264`）。

use super::*;
use vessel::pipe::land;
use vessel::pipe::review::REVIEW_FILE;
use vessel::pipe::{contract_path, run_dir};

/// 審査が run dir に残す lens の cmd の写し（設計 pipeline.md §26・名は字面で持つ＝写しの名の変化も歯が測る）。
fn lens_record_of(state: &Path, id: &str) -> PathBuf {
    run_dir(state, id).join("lens.toml")
}

#[test]
fn pipe_land_refuses_without_pass() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("FAIL"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert!(stdout_of(&gated).contains("verdict=FAIL"), "{}", stdout_of(&gated));
    let before = git(&repo, &["rev-parse", "refs/heads/main"]);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "PASS 以外は rc 1");
    assert!(stderr_of(&out).contains("PASS でない"), "理由: {}", stderr_of(&out));
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        before,
        "**何もしない**（main は 1 byte も動かない）"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_squashes_one_commit_with_identical_tree() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let tree = git(&worktree_of(&repo, &id), &["rev-parse", "HEAD^{tree}"]);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert!(stdout_of(&out).contains(&format!("landed={new}")), "{}", stdout_of(&out));
    assert_eq!(
        git(&repo, &["rev-list", "--count", &format!("{base}..{new}")]),
        "1",
        "**squash は 1 commit**"
    );
    assert_eq!(
        git(&repo, &["rev-parse", &format!("{new}^{{tree}}")]),
        tree,
        "tree は同一（lossless）"
    );
    clean(&[&repo, &state]);
}

/// 件名の歯の goal（**文が複数・200 字超**で、先頭の文が 72 文字より長い）。契約 file は
/// 1 行 1 値ゆえ改行を置けない（「改行を保つ」側は `pipe::land` の in-file の歯が測る）。
const LONG_GOAL: &str = "件名の要旨は goal の先頭の文を 72 文字で切って組む・この 1 文目は 72 文字より長いので末尾に印が付く・切った側と落とさない側を 1 本の message の中で持つのがこの便の主題である。2 文目はここから始まり件名には載らないが本文には逐語で載る。3 文目も同じで、契約の中身は message の本文からそのまま辿れる。";

/// squash の message は **3 部**（件名 / 空行 / 本文 = goal 全文 + `run:` trailer・`s2-07l.130`・
/// 設計 §5.4 手順 1）。件名は goal の**先頭の文**を 72 文字で切った要旨で、切って落ちた中身は
/// 本文に逐語で残る——`git log --oneline` が読めて、便の現物へは trailer から辿れる形である。
#[test]
fn pipe_land_subject_cuts_first_sentence_and_keeps_goal_in_body() {
    let (repo, state) = repo_with_state();
    // 契約 (b) 以後、`goal` は**行の `title`** である（planner 裁定 2026-09-19）。
    let goal = format!("title = \"{LONG_GOAL}\"");
    let path = write_contract(&repo, &["title"], &[&goal]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let subject = git(&repo, &["log", "-1", "--format=%s", "refs/heads/main"]);
    assert!(
        subject.starts_with("s2-2e5: 件名の要旨は goal の先頭の文を 72 文字で切って組む"),
        "件名は `<bead>: ` + goal の先頭の文で始まる: {subject}"
    );
    assert!(
        subject.chars().count() <= "s2-2e5: ".chars().count() + 72 + 1,
        "件名は 72 文字 + `…` 以内（base は goal 全文を載せるので落ちる）: {subject}"
    );
    assert!(subject.ends_with('…'), "切った周は印が付く: {subject}");
    // 落とさない側。**本文は goal 全文を逐語で持ち**、末尾に run へ辿る trailer の組が並ぶ。
    let body = git(&repo, &["log", "-1", "--format=%b", "refs/heads/main"]);
    assert!(body.contains(LONG_GOAL), "本文に goal 全文が逐語で在る: {body}");
    let trailer = format!("run: {id}");
    let tail: Vec<&str> = body.lines().rev().take(3).collect();
    assert_eq!(tail.len(), 3, "trailer の組は 3 行: {body}");
    assert!(tail.contains(&trailer.as_str()), "run trailer が在る: {body}");
    // **契約と要件の trailer**（設計 contract-source.md §5 手順 5・`s2-07l.382`）は run の後ろに並ぶ。
    assert!(tail.iter().any(|line| line.ends_with("#a")), "契約の trailer が設計 pointer を名指す: {body}");
    assert!(tail.iter().any(|line| line.ends_with("FR4")), "要件の trailer が req を名指す: {body}");
    clean(&[&repo, &state]);
}

/// 負例: goal が 1 文で 72 文字以内なら件名は**その文そのもの**で、切った印（`…`）は付かない
/// （「。」の手前までが先頭の文である）。
#[test]
fn pipe_land_subject_keeps_short_single_sentence_whole() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["title"], &[r#"title = "短い 1 文の goal は件名にそのまま載る。""#]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert_eq!(
        git(&repo, &["log", "-1", "--format=%s", "refs/heads/main"]),
        "s2-2e5: 短い 1 文の goal は件名にそのまま載る",
        "72 文字以内の 1 文は逐語（`…` は付かない）"
    );
    clean(&[&repo, &state]);
}

/// 別便の 1 commit で main を **面の内**（`crates/` 配下＝検出線の [`DETECTION_SCOPE`]）へ進める。
/// 返すのは動いた後の main の sha。
///
/// 面の**外**で進んだ周の追随は再 gate を撃たずに前周の判定を引き継ぐ（設計 §33）ので、撃ち直し
/// そのものを測る歯は面の内で main を動かす——測る対象（lens の再走・撃ち直しの判定）は不変である。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn commit_other_in_scope(repo: &Path) -> String {
    fs::create_dir_all(repo.join("crates")).expect("面の内の dir を作れる");
    fs::write(repo.join("crates").join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "other"]);
    git(repo, &["rev-parse", "refs/heads/main"])
}

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

// ───── lens の cmd の写し（設計 pipeline.md §26・`s2-07l.378`・接頭辞 `pipe_lens_record_`） ─────

/// 審査（`pipe intake … --lens <cmd>`）は受けた cmd を `<run_dir>/lens.toml` に `schema = 1` / `cmd = "<逐語>"` の
/// 2 行で写す。cmd の中の `"` と `'` は escape しない（素通し）。`--lens` の無い審査は写しを残さない（INCONCLUSIVE）。
#[test]
fn pipe_lens_record_review_writes_lens_toml() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    // 対: `--lens` の無い intake は審査が INCONCLUSIVE（終端）で写しを残さない。
    let bare = run_pipe(&[
        "intake", "--design", &path, "--bead", "b-bare",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
    ]);
    assert_eq!(bare.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "lens 無しの審査は rc 3: {}", stderr_of(&bare));
    let bare_id = run_id_of(&bare);
    assert!(!lens_record_of(&state, &bare_id).exists(), "`--lens` の無い周は写しを書かない");
    // 本命: `--lens` 付きの intake（helper は審査の偽 PASS lens を渡す）。
    let id = intake(&repo, &state, &path);
    let cmd = review_lens_pass(&state);
    assert!(cmd.contains('"') && cmd.contains('\''), "fixture の cmd は両方の引用符を含む（escape しないを測る）: {cmd}");
    let text = fs::read_to_string(lens_record_of(&state, &id)).expect("lens.toml が在る");
    assert_eq!(text, format!("schema = 1\ncmd = \"{cmd}\"\n"), "2 行・二重引用符・逐語");
    clean(&[&repo, &state]);
}

/// main が動いた便を `--lens` 無しで land → 写しの lens（審査の偽 PASS）で再 gate が起動して着地する
/// （base は「lens が要るのに --lens が無い」で INCONCLUSIVE＝RED）。
#[test]
fn pipe_lens_record_land_regate_reads_it_when_flag_absent() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let moved = commit_other_in_scope(&repo);
    // 2 つの marker を外して「どの lens が起きたか」を効果で測る: gate の flag の lens（`lens-ran`）は起きず、
    // 審査が写した lens（[`REVIEW_MARKER`]）が起きる。
    fs::remove_file(&marker).expect("gate の marker を消せる");
    fs::remove_file(state.join(REVIEW_MARKER)).expect("審査の marker を消せる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "写しの lens で再 gate が通り land する: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("rebase={base}..{moved}")), "追随は済む: {stdout}");
    assert!(stdout.contains("verdict=PASS"), "撃ち直しの判定行: {stdout}");
    assert!(stdout.contains("landed="), "land した: {stdout}");
    assert!(state.join(REVIEW_MARKER).exists(), "起きたのは写しの lens");
    assert!(!marker.exists(), "前の gate の flag の lens は起きない（record に無い）");
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(new, moved, "main が便の squash で進む");
    assert_eq!(git(&repo, &["rev-parse", &format!("{new}^")]), moved, "squash の親は動いた main");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 同じく gate: `--lens` 無しの gate が写しの lens を起動して PASS（base は INCONCLUSIVE＝RED）。`resume`
/// （`Implemented` → 同じ gate の関数）も同じ写しを読む。
#[test]
fn pipe_lens_record_gate_reads_it_when_flag_absent() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    fs::remove_file(state.join(REVIEW_MARKER)).expect("審査の marker を消せる");
    let out = gate_once(&repo, &state, &id, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "写しの lens で PASS: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=PASS"), "{}", stdout_of(&out));
    assert!(state.join(REVIEW_MARKER).exists(), "起きたのは写しの lens");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS");
    assert!(!value_of(&pairs, "evidence").contains("--lens が無い"), "理由は「無い」ではない: {}", value_of(&pairs, "evidence"));
    // 対: `resume` も同じ関数を通る（`Implemented` の 2 便目を `--lens` 無しで resume → 写しの lens で PASS → land）。
    stop_run_ok(&state, &id);
    let second = intake_bead(&repo, &state, &path, "b-two");
    let spawned = spawn_with(&repo, &state, &second, TOY_COMMIT);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&spawned));
    fs::remove_file(state.join(REVIEW_MARKER)).expect("審査の marker を消せる");
    let resumed = run_pipe(&[
        "resume", "--run", &second, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "resume も写しの lens で gate を通す: {}", stderr_of(&resumed));
    assert!(stdout_of(&resumed).contains("verdict=PASS"), "{}", stdout_of(&resumed));
    assert!(state.join(REVIEW_MARKER).exists(), "resume が起こしたのも写しの lens");
    clean(&[&repo, &state]);
}

/// `--lens` が在れば flag の cmd が起動する（写しは別の cmd＝審査の偽 PASS）。判定も flag の lens のもの（FAIL）。
#[test]
fn pipe_lens_record_flag_overrides_the_record() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    assert!(lens_record_of(&state, &id).exists(), "写しは在る（審査が書いた）");
    fs::remove_file(state.join(REVIEW_MARKER)).expect("審査の marker を消せる");
    let marker = state.join("lens-flag");
    let lens = fake_lens(&marker, &lens_verdict("FAIL"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "flag の lens の FAIL: {}", stderr_of(&out));
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "FAIL", "判定は flag の lens のもの");
    assert!(marker.exists(), "flag の lens が起きた");
    assert!(!state.join(REVIEW_MARKER).exists(), "写しの lens は起きない（flag が勝つ）");
    clean(&[&repo, &state]);
}

/// 壊れた `lens.toml` → INCONCLUSIVE の理由が写しの path と読めなかった理由を持ち、「--lens が無い」ではない
/// （読めなさを「無い」に潰さない・C10）。lens は起きない。land の再 gate も同じ 3 値で倒れ main を動かさない。
#[test]
fn pipe_lens_record_unreadable_is_not_absent() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let record = lens_record_of(&state, &id);
    fs::write(&record, "schema = 1\ncmd = 3\n").expect("壊れた写しを書ける");
    fs::remove_file(state.join(REVIEW_MARKER)).expect("審査の marker を消せる");
    let out = gate_once(&repo, &state, &id, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "読めない写しは rc 3: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    let evidence = value_of(&verdict_pairs(&state, &id), "evidence");
    assert!(evidence.contains(&record.display().to_string()), "理由は写しの path を持つ: {evidence}");
    assert!(evidence.contains("cmd が文字列でない"), "理由は読めなかった訳を持つ: {evidence}");
    assert!(!evidence.contains("--lens が無い"), "「無い」に潰さない: {evidence}");
    assert!(!state.join(REVIEW_MARKER).exists(), "lens は起きない");
    // 対: 写しを直せば `--lens` 無しの測り直しが通る（読めない周は終端でなく Gated＝測り直せる側）。
    fs::write(&record, format!("schema = 1\ncmd = \"{}\"\n", review_lens_pass(&state))).expect("写しを直せる");
    let again = gate_once(&repo, &state, &id, None);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "直した写しで PASS: {}", stderr_of(&again));
    assert!(state.join(REVIEW_MARKER).exists(), "直した写しの lens が起きた");
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

// ───── 撃ち直しの間に main が動いた便の追随し直し（設計 §5.4 (vi) / §18・`s2-07l.335`・接頭辞 `pipe_land_stale_`） ─────

/// 撃ち直しの lens が走っている間に **別便が main を面の内へ進める** fake lens（PASS を返す）。
///
/// `every` が偽なら**最初の 1 回だけ**進め（2 周目の撃ち直しは main を動かさない＝land できる）、真なら
/// 呼ばれるたびに進める（何周追随し直しても stale＝上限で終端する形）。呼出回数は `calls` に 1 行ずつ写す。
fn racing_lens(repo: &Path, state: &Path, every: bool) -> String {
    let calls = state.join("racing-calls");
    let once = state.join("racing-once");
    let advance = format!(
        "printf 'x\\n' >> '{repo}/crates/racing.txt'; git -C '{repo}' add -A; git -C '{repo}' commit -q -m racing",
        repo = repo.display()
    );
    let guarded = if every {
        advance
    } else {
        format!("if [ ! -e '{once}' ]; then touch '{once}'; {advance}; fi", once = once.display())
    };
    format!(
        "cat >/dev/null; printf 'call\\n' >> '{}'; {guarded}; echo '{}'; :",
        calls.display(),
        lens_verdict("PASS")
    )
}

/// [`racing_lens`] が呼ばれた回数。
fn racing_calls(state: &Path) -> usize {
    fs::read_to_string(state.join("racing-calls")).map(|text| text.lines().count()).unwrap_or(0)
}

/// stale の記帳（`Gated detail=stale:<range>`）の件数。
fn stale_count(state: &Path, id: &str) -> usize {
    stages(state, id)
        .into_iter()
        .filter(|(stage, detail)| {
            *stage == Some(Stage::Gated) && detail.as_deref().is_some_and(|found| found.starts_with("stale:"))
        })
        .count()
}

/// 追随の記帳（`Implemented detail=rebase:<range>`）の件数。
fn rebase_count(state: &Path, id: &str) -> usize {
    stages(state, id)
        .into_iter()
        .filter(|(stage, detail)| {
            *stage == Some(Stage::Implemented) && detail.as_deref().is_some_and(|found| found.starts_with("rebase:"))
        })
        .count()
}

/// 撃ち直しの間に main がさらに動いた周は **同じ land の中で追随し直して Landed** する（設計 §18・`--runner` は
/// 要らない）。`RunStage stage=Gated detail=stale:<old>..<now>` を 1 件記帳し、(iii) の rebase から再 gate を
/// 通し直して新しい main の上に squash が載る。stdout は 1 周目の追随と撃ち直しの判定行も捨てない。
/// base（rc 1 `stale base`・段は Gated・event 0 増）では stale の記帳も 2 度目の追随も無い＝RED。
#[test]
fn pipe_land_stale_follows_again_in_the_same_land_and_lands() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let moved = commit_other_in_scope(&repo);
    let lens = racing_lens(&repo, &state, false);
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "追随し直した land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    let raced = git(&repo, &["rev-parse", &format!("{new}^")]);
    assert_eq!(git(&repo, &["log", "-1", "--format=%s", &raced]), "racing", "squash は lens の中で進んだ main の上に載る");
    assert_eq!(git(&repo, &["rev-parse", &format!("{raced}^")]), moved, "racing は 1 周目の追随先の上に 1 commit");
    assert_eq!(git(&repo, &["rev-list", "--count", &format!("{raced}..{new}")]), "1", "squash は 1 commit");
    assert_eq!(racing_calls(&state), 2, "撃ち直しは 2 周（1 周目で main が動き・2 周目で載る）");
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("run={id} rebase={base}..{moved}")), "1 周目の追随の行: {stdout}");
    assert!(stdout.contains(&format!("run={id} stale={moved}..{raced}")), "stale の判定行: {stdout}");
    assert!(stdout.contains(&format!("run={id} rebase={moved}..{raced}")), "2 周目の追随の行: {stdout}");
    assert_eq!(stdout.matches("verdict=PASS").count(), 2, "撃ち直しの判定行は 2 周分: {stdout}");
    assert!(stdout.contains(&format!("landed={new}")), "landed=: {stdout}");
    assert!(!stderr_of(&out).contains("stale base"), "断らない: {}", stderr_of(&out));
    assert_stale_trail(&state, &id, &moved, &raced);
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 追随し直して載った便の event 列: stale の記帳 1 件（段は `Gated`・old と now を名乗る）→ 2 周目の追随の
/// 記帳の順で、追随は 2 件・`Failed` は 0 件。
fn assert_stale_trail(state: &Path, id: &str, moved: &str, raced: &str) {
    let trail = stages(state, id);
    assert_eq!(stale_count(state, id), 1, "stale の記帳は 1 件: {trail:?}");
    assert!(
        trail.contains(&(Some(Stage::Gated), Some(format!("stale:{moved}..{raced}")))),
        "stale の記帳は old と now を名乗り段は Gated のまま: {trail:?}"
    );
    assert_eq!(rebase_count(state, id), 2, "追随の記帳は 2 件（周ごとに 1 件）: {trail:?}");
    let stale_at = trail.iter().position(|(_, detail)| detail.as_deref().is_some_and(|found| found.starts_with("stale:")));
    let second_at = trail.iter().position(|(_, detail)| detail.as_deref() == Some(format!("rebase:{moved}..{raced}").as_str()));
    assert!(matches!((stale_at, second_at), (Some(s), Some(r)) if s < r), "順序は stale → 2 周目の追随: {trail:?}");
    assert!(!trail.iter().any(|(stage, _)| *stage == Some(Stage::Failed)), "終端しない: {trail:?}");
}

/// 何周追随し直しても main が動く周は **上限（`pipe.follow_retries`＝fixture で 1）で typed に終端する**
/// （`Failed detail=rebase-conflict` + rc 1・新しい終端の理由を増やさない・main は squash を載せない）。
/// 値 1 ＝最大 1 回追随し直す: stale 2 件目で終端し、lens は 2 周分だけ走る。`--runner` は要らない。
#[test]
fn pipe_land_stale_stops_at_the_limit_with_typed_failed() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let moved = commit_other_in_scope(&repo);
    let rules = write_rules_with_retries(&state, "rules-stale-1.toml", 1, 1_000_000, 1);
    let lens = racing_lens(&repo, &state, true);
    let out = land_extra(&repo, &state, &id, &["--lens", &lens, "--rules", &rules.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "上限に達した周は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("上限"), "理由は上限を名乗る: {}", stderr_of(&out));
    assert!(!stdout_of(&out).contains("landed="), "land していない: {}", stdout_of(&out));
    let trail = stages(&state, &id);
    assert_eq!(
        trail.last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-conflict".to_owned()))),
        "終端の理由は既存の型: {trail:?}"
    );
    assert_eq!(stale_count(&state, &id), 2, "stale の記帳は 2 件（1 回追随し直し・2 件目で終端）: {trail:?}");
    assert_eq!(rebase_count(&state, &id), 2, "追随は 2 周: {trail:?}");
    assert_eq!(racing_calls(&state), 2, "3 周目は撃たない");
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["log", "-1", "--format=%s", &main]), "racing", "main の先頭は lens が進めた commit（squash は無い）");
    assert_eq!(git(&repo, &["rev-list", "--count", &format!("{moved}..{main}")]), "2", "lens が 2 回進めただけ");
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "段は Failed");
    clean(&[&repo, &state]);
}

/// 回数は衝突の起こし直しと **1 つの上限に合算**する（設計 §18）: 衝突の記帳を 1 件持つ便は、上限 1 の下で
/// 最初の stale で終端する（別々に数えると stale 0 回＝追随し直して Landed に化ける）。
#[test]
fn pipe_land_stale_shares_the_limit_with_conflict_retries() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let moved = commit_other_in_scope(&repo);
    // 衝突の記帳を手で 1 件積む（段は Implemented に戻るので gate を撃ち直して Gated PASS へ）。
    record_conflict(&state, &id, &format!("{base}..{moved}"));
    let rules = write_rules_with_retries(&state, "rules-stale-shared.toml", 1, 1_000_000, 1);
    fs::remove_file(&marker).ok();
    let regate = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_with_rules(&repo, &state, &id, &rules, &regate);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "撃ち直しの gate: {}", stderr_of(&gated));
    // main は 1 度だけ動く（合算しなければ 2 周目で載る形）。
    let lens = racing_lens(&repo, &state, false);
    let out = land_extra(&repo, &state, &id, &["--lens", &lens, "--rules", &rules.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "合算で上限に達した周は rc 1: {}", stderr_of(&out));
    let trail = stages(&state, &id);
    assert_eq!(
        trail.last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-conflict".to_owned()))),
        "終端の理由: {trail:?}"
    );
    assert_eq!(stale_count(&state, &id), 1, "stale は 1 件で尽きる: {trail:?}");
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は手で積んだ 1 件のまま: {trail:?}");
    assert_eq!(racing_calls(&state), 1, "2 周目は撃たない");
    assert!(!stdout_of(&out).contains("landed="), "land していない: {}", stdout_of(&out));
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["log", "-1", "--format=%s", &main]), "racing", "squash は載らない");
    clean(&[&repo, &state]);
}

/// 同じ base から 2 便を PASS の gate まで通す（1 本目 = `crates/toy/a.rs`・2 本目 = `src/b.rs`＝
/// write-set は交わらない）。追随の歯の材料。
///
/// 1 本目が **面の内**（`crates/` 配下）を書くのは、1 本目の着地で動いた main に 2 本目が追随する周が
/// 従来どおり再 gate を撃つ形だからである（面の外だけが動いた周は引き継ぐ・設計 §33）。
fn two_gated_runs(repo: &Path, state: &Path, marker: &Path) -> (String, String) {
    // **2 行を 1 回で commit する**（行ごとに commit すると 2 便の base が別になる・契約 (b)）。
    commit_rows(
        repo,
        &[
            row_fields("a", &["write-set"], &[r#"write-set = ["crates/toy/a.rs"]"#]),
            row_fields("b", &["write-set"], &[r#"write-set = ["src/b.rs"]"#]),
        ],
    );
    let contract_a = format!("{DESIGN_FILE}#a");
    let id_a = intake_bead(repo, state, &contract_a, "s2-2e5");
    let spawned_a = run_pipe(&[
        "spawn", "--run", &id_a, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "mkdir -p crates/toy && echo a > crates/toy/a.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(spawned_a.status.code(), Some(i32::from(RC_OK)), "1 本目の spawn: {}", stderr_of(&spawned_a));
    let lens_a = fake_lens(marker, &lens_verdict("PASS"));
    let gated_a = gate_once(repo, state, &id_a, Some(&lens_a));
    assert_eq!(gated_a.status.code(), Some(i32::from(RC_OK)), "1 本目の gate: {}", stderr_of(&gated_a));
    let contract_b = format!("{DESIGN_FILE}#b");
    let id_b = intake_bead(repo, state, &contract_b, "s2-3ax");
    let spawned = run_pipe(&[
        "spawn", "--run", &id_b, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo b > src/b.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "2 本目の spawn: {}", stderr_of(&spawned));
    let lens = fake_lens(marker, &lens_verdict("PASS"));
    let gated = gate_once(repo, state, &id_b, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "2 本目の gate: {}", stderr_of(&gated));
    (id_a, id_b)
}

/// 列を積まない land（tmp manifest に `land.train_max` の行が無い＝先頭だけ・設計 §40）。1 本目だけを載せて 2 本目の
/// 追随を測る歯の入口——埋め込み manifest の上限で撃つと、2 本目も同じ周に候補の木で載る。
fn land_solo(repo: &Path, state: &Path, id: &str) -> Output {
    let rules = write_rules(state, "rules-solo.toml", 1, 1_000_000);
    land_extra(repo, state, id, &["--rules", &rules.display().to_string()])
}

/// 追随した便の event 列が **Implemented(rebase:) → Gated(PASS) → Landed** の順で replay できるか。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn assert_follow_events(state: &Path, base: &str, moved: &str, new: &str) {
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let rebased = log.find(&format!("rebase:{base}..{moved}")).expect("追随の event が在る");
    let regated = log.rfind("verdict:PASS").expect("撃ち直しの判定 event が在る");
    let landed = log.rfind(&format!("sha:{new}")).expect("Landed の event が在る");
    assert!(rebased < regated && regated < landed, "順序: rebase → 撃ち直し → Landed\n{log}");
    let follow = log
        .lines()
        .find(|line| line.contains(&format!("rebase:{base}..{moved}")))
        .expect("追随の event の行が在る");
    assert!(follow.contains("\"stage\":\"Implemented\""), "追随の event は段を Implemented へ戻す: {follow}");
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

/// 便 1 本を PASS の gate まで通し、**同じ変更を先に main へ載せる**（1 本目が land した後と
/// 同じ状態）。返すのは便の id と、そのときの main の sha。
///
/// `.145` までは 2 便を同時に live にして 1 本目を land する形だった。入口の排他
/// （`s2-07l.145`・ADR-0019 §2.1）が在る今、同じ write-set の 2 便は**同時に live にできない**
/// ——main が動いた事実だけを器の外で作り、rebase で patch が空になる便を 1 本で測る
/// （land / retire が測る対象は 1 つも変えていない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn gated_run_whose_change_is_already_on_main(repo: &Path, state: &Path, marker: &Path) -> (String, String) {
    let design = write_contract(repo, &[], &[]);
    let id = gated_pass(repo, state, &design, marker);
    // 便の runner（`echo x >> src/lib.rs`）と**同じ 1 行**を main へ載せる。
    let lib = repo.join("src").join("lib.rs");
    let text = fs::read_to_string(&lib).expect("seed を読める");
    fs::write(&lib, format!("{text}x\n")).expect("main 側に同じ変更を書ける");
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "same-change-from-elsewhere"]);
    let landed = git(repo, &["rev-parse", "refs/heads/main"]);
    (id, landed)
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

// ───── 既に main に自分の squash が在る便の land（設計 §29・`s2-07l.389`・FR50・接頭辞 `pipe_land_already_landed_`） ─────

/// 既着地の fixture: 便 A を PASS の gate まで通し、A の worktree の HEAD の tree から
/// `git commit-tree <tree> -p <main> -m "<件名>\n\n<goal>\n\n<body>"` で squash を作って `refs/heads/main` を進める
/// （前の周が CAS の後・実測の前に死んだ形を再現＝A の段は Gated のまま・`verify-main.jsonl` は無い）。
/// `body` は A の id から本文の末尾（trailer の行）を組む——**trailer の有無と字面は歯が選ぶ**。
/// 返すのは A の id と作った squash の sha。
fn gated_run_squashed_on_main(repo: &Path, state: &Path, marker: &Path, body: impl Fn(&str) -> String) -> (String, String) {
    let design = write_contract(repo, &[], &[]);
    let id = gated_pass(repo, state, &design, marker);
    let tree = git(&worktree_of(repo, &id), &["rev-parse", "HEAD^{tree}"]);
    let old = git(repo, &["rev-parse", "refs/heads/main"]);
    let message = format!("s2-2e5: 縦 1 本を通す\n\n縦 1 本を通す\n\n{}", body(&id));
    let squash = git(repo, &["commit-tree", &tree, "-p", &old, "-m", &message]);
    git(repo, &["update-ref", "refs/heads/main", &squash, &old]);
    assert_eq!(git(repo, &["rev-parse", "refs/heads/main"]), squash, "fixture: main は作った squash を指す");
    assert!(show_line(repo, state, &id).contains("stage=Gated"), "fixture: 便の段は Gated のまま");
    (id, squash)
}

/// 便の trailer の 1 行（[`vessel::pipe::land`] の `squash_message` が本文の末尾に置く字面）。
fn run_trailer(id: &str) -> String {
    format!("run: {id}")
}

/// run id の**末尾 1 字**だけを変えた id（別の便の trailer の形・接頭辞は A と同じ）。
fn altered_id(id: &str) -> String {
    let mut chars: Vec<char> = id.chars().collect();
    let last = chars.pop().unwrap_or('0');
    chars.push(if last == '0' { '1' } else { '0' });
    chars.into_iter().collect()
}

/// (a) 自分の trailer を持つ squash が main に在る便の land: **main を動かさず**（sha も `rev-list --count` も同じ）、
/// 主実測を撃って（`verify-main.jsonl`）Landed で終端し、stdout に `landed=<sha>` / `main=<sha>` / `already-landed=1`、
/// `RunDone stage=Landed` の detail に `sha:<sha>` と `already-landed`、verdicts.jsonl に A の行（`sha` = 見つけた sha・
/// key 列は従来）、A の worktree は退役する。gate は撃ち直さない（lens は走らない）。
#[test]
fn pipe_land_already_landed_finishes_without_moving_main() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, squash) = gated_run_squashed_on_main(&repo, &state, &marker, run_trailer);
    fs::remove_file(&marker).expect("lens の marker を消せる");
    let count_before = git(&repo, &["rev-list", "--count", "refs/heads/main"]);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "既着地の land は rc 0: {}", stderr_of(&out));
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), squash, "main は撃つ前と同じ sha（squash を作らない）");
    assert_eq!(
        git(&repo, &["rev-list", "--count", "refs/heads/main"]),
        count_before,
        "main の commit 数も同じ（母集団＝1 commit も足していない）"
    );
    assert!(!marker.exists(), "gate を撃ち直さない（lens は走らない）");
    assert_already_landed_terminal(&state, &id, &squash, &out);
    assert_already_landed_side_effects(&repo, &state, &id, &squash);
    clean(&[&repo, &state]);
}

/// (a) の終端の字面: stdout の `landed=` / `main=` / 後置の `already-landed=1` と、末尾 event の
/// `RunDone stage=Landed detail=sha:<found> main:<実測> already-landed`。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn assert_already_landed_terminal(state: &Path, id: &str, squash: &str, out: &Output) {
    let stdout = stdout_of(out);
    assert_eq!(landed_token(out), squash, "`landed=` は見つけた squash の sha: {stdout}");
    assert_eq!(main_token(out), squash, "`main=` は終端で実測した main（= 作った sha）: {stdout}");
    assert!(
        stdout.split_whitespace().any(|word| word == "already-landed=1"),
        "stdout の末尾に `already-landed=1` を後置する: {stdout}"
    );
    let (kind, stage, detail) = trail(state, id).pop().expect("便の event が在る");
    assert_eq!((kind, stage), (EventKind::RunDone, Some(Stage::Landed)), "events の末尾は RunDone stage=Landed");
    assert_eq!(
        detail.unwrap_or_default(),
        format!("sha:{squash} main:{squash} already-landed"),
        "detail は `sha:<found> main:<実測> already-landed`（§27 の `main:` の後ろ・空白区切り）"
    );
}

/// (a) の永続面: 面 5 の行（従来の key 列・`sha` = 見つけた squash）・worktree の退役・主実測の記録・段。
fn assert_already_landed_side_effects(repo: &Path, state: &Path, id: &str, squash: &str) {
    let pairs = exported_pairs(state, id);
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["schema", "run", "bead", "sha", "verdict", "evidence", "ts", "order", "generation", "size", "files", "lines", "pub_symbols"],
        "verdicts.jsonl の行は従来の key 列（任意 field を足さない）"
    );
    assert_eq!(value_of(&pairs, "sha"), squash, "面 5 の `sha` は見つけた squash");
    assert_eq!(value_of(&pairs, "verdict"), "PASS");
    assert!(!worktree_of(repo, id).exists(), "A の worktree は退役する（元の場所に残らない）");
    assert!(
        repo.join(".worktrees").join("scribe2").join("retired").join(id).exists(),
        "retired/ へ move されている"
    );
    assert!(
        state.join("pipe").join(id).join("verify-main.jsonl").exists(),
        "主実測を撃った証拠（verify-main.jsonl）が在る"
    );
    assert!(show_line(repo, state, id).contains("stage=Landed"), "段は Landed");
}

/// squash の本文の末尾（trailer の行）を A の id から組む形。
type TrailerBody = fn(&str) -> String;

/// (b) 負例: squash の trailer が**別の run id**（A の id の末尾 1 字違い）の周と、trailer の行そのものを
/// **持たない**周は、どちらも従来の `Failed detail=rebase-empty`（main は不変・`already-landed` は出ない）。
/// 母集団 = 2 回の land の rc と detail。
#[test]
fn pipe_land_already_landed_needs_the_exact_trailer() {
    let bodies: [(&str, TrailerBody); 2] = [
        ("別の便の trailer", |id| run_trailer(&altered_id(id))),
        ("trailer の行が無い", |_| String::new()),
    ];
    for (name, body) in bodies {
        let (repo, state) = repo_with_state();
        let marker = state.join("lens-ran");
        let (id, squash) = gated_run_squashed_on_main(&repo, &state, &marker, body);
        let out = land_once(&repo, &state, &id);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{name}: 従来どおり rc 1: {}", stderr_of(&out));
        assert!(stderr_of(&out).contains("既に main に在る"), "{name}: 理由は rebase-empty の字面: {}", stderr_of(&out));
        assert!(!stdout_of(&out).contains("already-landed"), "{name}: `already-landed` は出ない: {}", stdout_of(&out));
        assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), squash, "{name}: main は不変");
        let (kind, stage, detail) = trail(&state, &id).pop().expect("便の event が在る");
        assert_eq!((kind, stage), (EventKind::RunStage, Some(Stage::Failed)), "{name}: 末尾は Failed");
        assert_eq!(detail.as_deref(), Some("rebase-empty"), "{name}: detail は従来の rebase-empty");
        assert!(!land::verdicts_path(&state).exists(), "{name}: 面 5 へ書かない");
        assert!(worktree_of(&repo, &id).exists(), "{name}: worktree は退役しない");
        clean(&[&repo, &state]);
    }
}

/// (c) (a) の形で main の**共通 verify の写し**を赤（`sh verify-red.sh`）にする: 従来の `main_red` の終端
/// （Landed にならない・stdout に `already-landed` は無い・面 5 へ書かない）＝**主実測を飛ばしていない**証拠。
/// main は見つけた sha のまま（この land は動かしていないので revert の対象も無い）。
#[test]
fn pipe_land_already_landed_red_main_is_not_landed() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, squash) = gated_run_squashed_on_main(&repo, &state, &marker, run_trailer);
    make_tree_differ(&repo, &state, &id, &format!("{squash}^"));
    // 主実測は**写しからしか読まない**（ADR-0010 §2.4）ので、gate の後に写しの共通 verify だけを赤へ差し替える。
    let frozen = vessel::pipe::vessel_path(&state, &id);
    let text = fs::read_to_string(&frozen).expect("宣言の写しを読める");
    let common = format!("common-verify = {VESSEL_COMMON}\n");
    assert!(text.contains(&common), "fixture: 写しに既定の共通 verify が在る: {text}");
    fs::write(&frozen, text.replace(&common, "common-verify = [\"sh verify-red.sh\"]\n")).expect("写しを書ける");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "main が赤ければ rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("main が赤い"), "理由: {}", stderr_of(&out));
    assert!(!stdout_of(&out).contains("already-landed"), "赤い周に `already-landed` は出ない: {}", stdout_of(&out));
    let (kind, stage, detail) = trail(&state, &id).pop().expect("便の event が在る");
    assert_eq!((kind, stage), (EventKind::RunStage, Some(Stage::Failed)), "末尾は Failed（Landed にならない）");
    assert_eq!(detail.as_deref(), Some("main-red"), "detail は従来の main-red");
    assert!(
        state.join("pipe").join(&id).join("verify-main.jsonl").exists(),
        "主実測は撃っている（verify-main.jsonl が在る）"
    );
    assert!(!land::verdicts_path(&state).exists(), "赤い周は面 5 へ export しない");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), squash, "main は見つけた sha のまま");
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "段は Failed");
    clean(&[&repo, &state]);
}

/// [`write_rules`] の `pipe.land_wait_s` の行だけを差し替えた tmp manifest（`None` = 行を落とす）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_rules_land_wait(dir: &Path, name: &str, land_wait_s: Option<u64>) -> String {
    let path = write_rules(dir, name, 1, 1_000_000);
    let text = fs::read_to_string(&path).expect("tmp manifest を読める");
    let block = |value: u64| {
        format!(
            "[[rule]]\nid = \"pipe.land_wait_s\"\nkind = \"PipeLandWaitS\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n"
        )
    };
    let default = block(LAND_WAIT_S);
    assert!(text.contains(&default), "既定の行が在る（差し替えが空振りしない）: {text}");
    let replaced = text.replace(&default, &land_wait_s.map(block).unwrap_or_default());
    fs::write(&path, replaced).expect("tmp manifest を書ける");
    path.display().to_string()
}

/// land の stdout の `order=` の値（無ければ空）。
fn order_token(out: &Output) -> String {
    stdout_of(out)
        .split_whitespace()
        .find_map(|word| word.strip_prefix("order="))
        .map(str::to_owned)
        .unwrap_or_default()
}

/// 面 5（`verdicts.jsonl`）の便の行の `order` の値（行も field も無ければ空）。
fn exported_order(state: &Path, id: &str) -> String {
    let text = fs::read_to_string(land::verdicts_path(state)).unwrap_or_default();
    text.lines()
        .filter_map(|line| vessel::fleet::json_lite::parse_object(line.trim()).ok())
        .find(|pairs| value_of(pairs, "run") == id)
        .map(|pairs| value_of(&pairs, "order"))
        .unwrap_or_default()
}

/// 便の追随の記帳（`Implemented detail=rebase:<range>`）の件数。
fn follow_count(state: &Path, id: &str) -> usize {
    stages(state, id)
        .into_iter()
        .filter(|(stage, detail)| {
            *stage == Some(Stage::Implemented) && detail.as_deref().is_some_and(|found| found.starts_with("rebase:"))
        })
        .count()
}

/// 便の gate の周の件数（`Gated detail=verdict:<…>` だけを数える＝番の記帳 `turn:taken` は gate の周でない・設計 §22）。
fn gate_count(state: &Path, id: &str) -> usize {
    stages(state, id)
        .into_iter()
        .filter(|(stage, detail)| *stage == Some(Stage::Gated) && detail.as_deref().is_some_and(|found| found.starts_with("verdict:")))
        .count()
}

/// land の stdout の `landed=` の値（squash commit の sha・無ければ空）。
fn landed_token(out: &Output) -> String {
    stdout_of(out)
        .split_whitespace()
        .find_map(|word| word.strip_prefix("landed="))
        .map(str::to_owned)
        .unwrap_or_default()
}

/// land の stdout の `main=` の値（終端で実測した `refs/heads/main`・無ければ空）。
fn main_token(out: &Output) -> String {
    stdout_of(out)
        .split_whitespace()
        .find_map(|word| word.strip_prefix("main="))
        .map(str::to_owned)
        .unwrap_or_default()
}

/// verdict の `tree` を `rev` の木（実在する別の木）に差し替え、主実測を「木が違う周」の経路にする fixture
/// （設計 gate-cost.md §27・`s2-07l.464`: 同じ木の周は主実測を 1 本も撃たないので、主実測の赤 / 測れない /
/// verify の副作用を測る歯は land の前にこれを挟む。`gate.rs` の `verdict_tree_to_base` と同じ型）。
/// 差分は便の `src/lib.rs` だけ＝面の外なので、③ は従来どおり省き ①②④ を撃つ＝歯の期待は不変。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn make_tree_differ(repo: &Path, state: &Path, id: &str, rev: &str) {
    let verdict = state.join("pipe").join(id).join("verdict.json");
    let text = fs::read_to_string(&verdict).expect("verdict.json を読める");
    let tree = value_of(&verdict_pairs(state, id), "tree");
    assert!(!tree.is_empty(), "差し替える前の verdict は tree を持つ: {text}");
    let other = git(repo, &["rev-parse", &format!("{rev}^{{tree}}")]);
    assert_ne!(other, tree, "fixture: 差し替え先は別の木");
    let swapped = text.replace(&format!("\"tree\":\"{tree}\""), &format!("\"tree\":\"{other}\""));
    fs::write(&verdict, swapped).expect("verdict.json を差し替えられる");
}

/// 便の `RunDone stage=Landed` の detail（無ければ空・着地後の検出の `detection:` は読み飛ばす＝設計 gate-cost.md §44 形 (4)）。
fn landed_detail(state: &Path, id: &str) -> String {
    trail(state, id)
        .into_iter()
        .rev()
        .filter(|(kind, stage, _)| *kind == EventKind::RunDone && *stage == Some(Stage::Landed))
        .filter_map(|(_, _, detail)| detail)
        .find(|detail| !detail.starts_with("detection:"))
        .unwrap_or_default()
}

/// toy repo の `reference-transaction` hook: `committed` の段で `refs/heads/main` を旧 sha へ戻す
/// （**1 度だけ**・自分の `update-ref` で再帰しない印を `marker` に置く）。CAS の後に main が
/// 動いた周（追随の chain・別の便・手の操作）を、land の外の手で作る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn install_main_rewind_hook(repo: &Path, marker: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let hook = repo.join(".git").join("hooks").join("reference-transaction");
    fs::create_dir_all(hook.parent().expect("hooks の親 dir")).expect("hooks dir を作れる");
    let body = format!(
        "#!/bin/sh\n[ \"$1\" = committed ] || exit 0\n[ -f '{marker}' ] && exit 0\n\
         while read -r old new ref; do\n  if [ \"$ref\" = refs/heads/main ]; then\n    touch '{marker}'\n    \
         git update-ref refs/heads/main \"$old\"\n    exit 0\n  fi\ndone\nexit 0\n",
        marker = marker.display()
    );
    fs::write(&hook, body).expect("hook を書ける");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("hook に実行権を付ける");
}

/// 通常の land（main は CAS の後に動かない）: stdout の `main=` は `landed=` と等しく、`Landed` の detail は
/// `sha:<new> main:<new>`（一致する周も**省かない**・C10 の実測値・設計 §27）。
#[test]
fn pipe_land_main_measured_matches_landed_when_main_is_still() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(landed_token(&out), new, "`landed=` は squash の sha: {}", stdout_of(&out));
    assert_eq!(main_token(&out), new, "`main=` は終端で実測した main: {}", stdout_of(&out));
    assert_eq!(
        landed_detail(&state, &id),
        format!("sha:{new} main:{new}"),
        "`Landed` の detail は宣言値と実測値を空白区切りで並べる"
    );
    assert!(
        !stderr_of(&out).contains("読めない"),
        "読めた周は stderr に理由を出さない: {}",
        stderr_of(&out)
    );
    clean(&[&repo, &state]);
}

/// CAS の後に hook が main を旧 sha へ戻す周: land は rc 0 のまま（終端を偽らない）、`main=` は実測の
/// 旧 sha で `landed=` と**違う**——宣言値と実測値が別の列に在るから見分けられる。
#[test]
fn pipe_land_main_measured_differs_when_a_hook_moves_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let old = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    install_main_rewind_hook(&repo, &state.join("main-rewound"));
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "main が動いても land は成立: {}", stderr_of(&out));
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), old, "hook が main を旧 sha へ戻している");
    let landed = landed_token(&out);
    assert!(!landed.is_empty() && landed != old, "`landed=` は squash の sha のまま: {}", stdout_of(&out));
    assert_eq!(main_token(&out), old, "`main=` は実測の旧 sha: {}", stdout_of(&out));
    assert_eq!(
        landed_detail(&state, &id),
        format!("sha:{landed} main:{old}"),
        "detail の `main:` も実測の旧 sha"
    );
    clean(&[&repo, &state]);
}

/// `--pr-cmd` の形は main を動かさない＝実測の列を持たない（stdout に `main=` が無い・detail は `pr` のまま）。
#[test]
fn pipe_land_main_measured_is_absent_for_pr_cmd() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_extra(&repo, &state, &id, &["--pr-cmd", "true"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PR 形の land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("landed=pr"), "{}", stdout_of(&out));
    assert!(main_token(&out).is_empty(), "PR 形は `main=` を持たない: {}", stdout_of(&out));
    assert_eq!(landed_detail(&state, &id), "pr", "PR 形の detail は `pr` のまま");
    clean(&[&repo, &state]);
}

/// land を背景で撃つ（列で待つ歯の材料・stdout / stderr は `wait_with_output` で読む）。
///
/// 起動は [`pipe_cmd`]（口 (i)）で組む——道具箱の PATH は撃つ argv の置き場から来る。
// flip-check: retroactive s2-07l.504
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn land_in_background(repo: &Path, state: &Path, id: &str, rules: &str, lens: &str) -> Child {
    pipe_cmd(&[
        "land", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--rules", rules, "--lens", lens,
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("binary を起動できる")
}

/// 同じ base から 3 便を PASS の gate まで通す（Gated の ts 順 a < b < c・3 本目 = `src/c.rs`＝
/// write-set は交わらない）。bead も同じ順（`s2-2e5` < `s2-3ax` < `s2-4cz`）なので、同じ秒に
/// Gated になっても列の順（同時刻は run id の辞書順）は変わらない。
fn three_gated_runs(repo: &Path, state: &Path, marker: &Path) -> (String, String, String) {
    let (id_a, id_b) = two_gated_runs(repo, state, marker);
    let contract_c = write_contract(repo, &["write-set"], &[r#"write-set = ["src/c.rs"]"#]);
    let id_c = intake_bead(repo, state, &contract_c, "s2-4cz");
    let spawned = run_pipe(&[
        "spawn", "--run", &id_c, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo c > src/c.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "3 本目の spawn: {}", stderr_of(&spawned));
    let lens = fake_lens(marker, &lens_verdict("PASS"));
    let gated = gate_once(repo, state, &id_c, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "3 本目の gate: {}", stderr_of(&gated));
    (id_a, id_b, id_c)
}

/// 着地の順番（設計 gate-cost.md §6）: 後から Gated になった便は前の便が列に居る間は待ち、上限
/// （fixture 1 秒）で**待たずに進む**（`order=degraded`・断らない・止めない）。main は動いていないので
/// 追随せずに Landed。
#[test]
fn pipe_order_later_run_degrades_at_the_limit_and_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(LAND_WAIT_S));
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "上限で進んで land する: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "degraded", "stdout の land 行: {}", stdout_of(&out));
    assert_eq!(exported_order(&state, &id_b), "degraded", "面 5 の record");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "段は Landed");
    assert!(show_line(&repo, &state, &id_a).contains("stage=Gated"), "前の便は列に残ったまま");
    clean(&[&repo, &state]);
}

/// 先に Gated になった便は待たない（`order=first`）。
#[test]
fn pipe_order_oldest_run_lands_first_without_waiting() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, _id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "first", "stdout の land 行: {}", stdout_of(&out));
    assert_eq!(exported_order(&state, &id_a), "first", "面 5 の record");
    clean(&[&repo, &state]);
}

/// 待っている便は、前の便が land して列を空けた時点で**上限を待たずに**進み（`order=waited:<n>`・n < 上限）、
/// 追随 1 回・gate の撃ち直し 1 回で Landed（撃ち直しの間は main が動かない＝(vi) が起きない）。
#[test]
fn pipe_order_waiting_run_lands_after_the_front_with_one_follow() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let mut waiting = land_in_background(&repo, &state, &id_b, &rules, &lens);
    std::thread::sleep(Duration::from_secs(2));
    assert!(waiting.try_wait().expect("子の状態を読める").is_none(), "後の便は列の前が空くまで待っている");
    let first = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "前の便の land: {}", stderr_of(&first));
    assert_eq!(order_token(&first), "first", "前の便は待たない: {}", stdout_of(&first));
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);

    let out = waiting.wait_with_output().expect("待っていた land が終わる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "待っていた便も land する: {}", stderr_of(&out));
    let token = order_token(&out);
    let waited: u64 = token.strip_prefix("waited:").and_then(|secs| secs.parse().ok()).unwrap_or(u64::MAX);
    assert!(waited < 30, "上限を待たずに進んだ（order={token}）: {}", stdout_of(&out));
    assert_eq!(exported_order(&state, &id_b), token, "面 5 の record も同じ値");
    assert_eq!(follow_count(&state, &id_b), 1, "追随は 1 回: {:?}", stages(&state, &id_b));
    assert_eq!(gate_count(&state, &id_b), 2, "gate は初回 + 撃ち直し 1 回: {:?}", stages(&state, &id_b));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-parse", &format!("{new}^")]), moved, "前の便の上に載る");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 前の便の worktree が在らない（retire 済み＝move 済み）なら列から外れ、後の便は待たない。
#[test]
fn pipe_order_front_run_without_worktree_leaves_the_queue() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let front = worktree_of(&repo, &id_a);
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id_a);
    fs::create_dir_all(retired.parent().unwrap_or(&repo)).expect("retired の親を作れる");
    git(&repo, &["worktree", "move", &front.display().to_string(), &retired.display().to_string()]);
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "first", "列の前が空: {}", stdout_of(&out));
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 3 便（Gated の ts 順 a < b < c）: a を land した後に b と c の land を背景で撃つと、b が追随 →
/// 撃ち直しの間も b は列の先頭に残り c は待つ（`order=waited:<n>`）。追随は b・c とも **1 回ずつ**
/// （2 回の便が 0＝c が (vi) の `stale base` を踏まない）で、main には a → b → c の順に載る
/// （lens の指摘 2026-09-13T04:05Z の形・撃ち直し中の便が列から外れると c が b と並行に撃ち直す）。
#[test]
fn pipe_order_three_runs_follow_once_each_and_land_in_gated_order() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b, id_c) = three_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let first = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "a の land: {}", stderr_of(&first));
    assert_eq!(order_token(&first), "first", "a は待たない: {}", stdout_of(&first));
    let second = land_in_background(&repo, &state, &id_b, &rules, &lens);
    let third = land_in_background(&repo, &state, &id_c, &rules, &lens);
    let out_b = second.wait_with_output().expect("b の land が終わる");
    let out_c = third.wait_with_output().expect("c の land が終わる");
    assert_eq!(out_b.status.code(), Some(i32::from(RC_OK)), "b の land: {}", stderr_of(&out_b));
    assert_eq!(out_c.status.code(), Some(i32::from(RC_OK)), "c の land: {}", stderr_of(&out_c));
    assert_eq!(order_token(&out_b), "first", "a の着地後の b は列の先頭: {}", stdout_of(&out_b));
    let token = order_token(&out_c);
    let waited: u64 = token.strip_prefix("waited:").and_then(|secs| secs.parse().ok()).unwrap_or(u64::MAX);
    assert!(waited < 30, "c は b が列を空けるまで待ち、上限は待たない（order={token}）: {}", stdout_of(&out_c));
    assert_eq!(exported_order(&state, &id_c), token, "面 5 の record も同じ値");
    assert_eq!(follow_count(&state, &id_b), 1, "b の追随は 1 回: {:?}", stages(&state, &id_b));
    assert_eq!(follow_count(&state, &id_c), 1, "c の追随は 1 回: {:?}", stages(&state, &id_c));
    let (sha_a, sha_b, sha_c) = (landed_token(&first), landed_token(&out_b), landed_token(&out_c));
    assert_eq!(git(&repo, &["rev-parse", &format!("{sha_b}^")]), sha_a, "b は a の上に載る");
    assert_eq!(git(&repo, &["rev-parse", &format!("{sha_c}^")]), sha_b, "c は b の上に載る");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), sha_c, "main の先頭は c");
    clean(&[&repo, &state]);
}

/// 撃ち直しが FAIL になった便（`Gated` のまま verdict が FAIL）は列から外れ、後続は待たずに進む。
/// 「待たなかった」は**待ちの record** で pin する（設計 gate-cost.md §23 形 (1)）: stdout の `order=first` と
/// 面 5（`verdicts.jsonl`）の `order` = `first`（待った周は `waited:<s>`）。壁時計は測らない——負荷下では
/// land 自体（rebase + 再 gate + 主実測）が上限を超えて偽に落ちる。
#[test]
fn pipe_order_regate_fail_leaves_the_queue() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b, id_c) = three_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let first = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "a の land: {}", stderr_of(&first));
    let fail = fake_lens(&marker, &lens_verdict("FAIL"));
    let failed = land_extra(&repo, &state, &id_b, &["--rules", &rules, "--lens", &fail]);
    assert_ne!(failed.status.code(), Some(i32::from(RC_OK)), "撃ち直しが FAIL なら land しない: {}", stdout_of(&failed));
    assert_eq!(follow_count(&state, &id_b), 1, "b は追随して撃ち直した: {:?}", stages(&state, &id_b));
    assert!(show_line(&repo, &state, &id_b).contains("stage=Gated"), "b は Gated(FAIL) のまま");
    let pass = fake_lens(&marker, &lens_verdict("PASS"));
    let out = land_extra(&repo, &state, &id_c, &["--rules", &rules, "--lens", &pass]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "c の land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "first", "FAIL の b は列に居ない: {}", stdout_of(&out));
    assert_eq!(exported_order(&state, &id_c), "first", "面 5 の record も `first`（待った周は waited:<s>）");
    assert!(show_line(&repo, &state, &id_c).contains("stage=Landed"), "c は Landed");
    clean(&[&repo, &state]);
}

/// `--pr-cmd` の形は列を見ない（main を動かさない）: 前の便が列に居ても待たず、`order=` を出さない。
/// 列を見ない形は面 5 へも `order` を書かないので、「待たなかった」の pin は stdout に `order=` が無いこと
/// だけである（設計 gate-cost.md §23 形 (1)・記録を書かない面に空文字の pin を置いても RED を作れない）。
/// 壁時計は測らない（負荷下で偽に落ちる）。
#[test]
fn pipe_order_pr_cmd_does_not_look_at_the_queue() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (_id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules, "--pr-cmd", "true"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PR の口: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("landed=pr"), "{}", stdout_of(&out));
    assert!(!stdout_of(&out).contains("order="), "列を見ない形は order= を出さない: {}", stdout_of(&out));
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main, "main は動かない");
    clean(&[&repo, &state]);
}

/// 列を導けない周（worktree 在りの `Gated` の便の判定が読めない）は `order=unmeasured` で**進む**
/// （rc 2 にしない＝待ちは deny の関門でない・main 実測の「測れなかった」とは別の極性）。
#[test]
fn pipe_order_unreadable_front_verdict_is_unmeasured_and_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    fs::write(state.join("pipe").join(&id_a).join("verdict.json"), "{broken\n").expect("判定を壊せる");
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "読めない周も進む: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "unmeasured", "stdout の land 行: {}", stdout_of(&out));
    assert_eq!(exported_order(&state, &id_b), "unmeasured", "面 5 の record");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 便の driver の札に pid を書く（本文は 10 進 1 行・dir は run dir で在る前提）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_driver_pid(state: &Path, id: &str, pid: u32) {
    fs::write(state.join("pipe").join(id).join("driver"), format!("{pid}\n")).expect("札を書ける");
}

/// 確実に居ない pid（`sh -c true` を起こして wait した pid）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn dead_pid() -> u32 {
    let mut child = std::process::Command::new("sh").args(["-c", "true"]).spawn().expect("sh を起こせる");
    let pid = child.id();
    child.wait().expect("sh を待てる");
    pid
}

/// 面 5 の便の行の `skipped_dead` の値（行も field も無ければ空）。
fn exported_skipped_dead(state: &Path, id: &str) -> String {
    value_of(&exported_pairs(state, id), "skipped_dead")
}

/// 先頭の便の札が死んだ pid（設計 pipeline.md §36）: 後続の land は待たずに `first` で進み、stdout の `order=` の直後に
/// `skipped-dead=1`、面 5 の `skipped_dead` に先頭の便 id を載せる。外すだけで先頭の段と worktree は動かない。
#[test]
fn pipe_order_dead_front_driver_is_skipped_and_named() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    put_driver_pid(&state, &id_a, dead_pid());
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "first", "死んだ先頭は数えない: {}", stdout_of(&out));
    assert!(stdout_of(&out).contains(" order=first skipped-dead=1 "), "order= の直後に外した本数: {}", stdout_of(&out));
    assert_eq!(exported_order(&state, &id_b), "first", "面 5 の order");
    assert_eq!(exported_skipped_dead(&state, &id_b), id_a, "面 5 の skipped_dead は外した便 id");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "後続は Landed");
    assert!(show_line(&repo, &state, &id_a).contains("stage=Gated"), "死んだ便の段は動かない");
    assert!(worktree_of(&repo, &id_a).is_dir(), "死んだ便の worktree は残る");
    clean(&[&repo, &state]);
}

/// 先頭の便の札が生きている（所有者 = この test の process）周は従来どおり待つ（上限 1 秒で `degraded`・外した便は
/// 名指さない）。
#[test]
fn pipe_order_dead_live_front_driver_still_waits() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    put_driver_pid(&state, &id_a, std::process::id());
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(LAND_WAIT_S));
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "degraded", "生きている先頭を待つ: {}", stdout_of(&out));
    assert!(!stdout_of(&out).contains("skipped-dead="), "外した便は無い: {}", stdout_of(&out));
    assert_eq!(exported_skipped_dead(&state, &id_b), "", "面 5 に skipped_dead を書かない");
    assert!(show_line(&repo, &state, &id_a).contains("stage=Gated"), "先頭は列に残ったまま");
    clean(&[&repo, &state]);
}

/// 先頭の便に札が無い（`pipe spawn` で起こした便）周も従来どおり待つ（札の無いを「死んだ」に読み替えない）。
#[test]
fn pipe_order_dead_absent_front_driver_still_waits() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    assert!(!state.join("pipe").join(&id_a).join("driver").exists(), "前提: 先頭は札を持たない");
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(LAND_WAIT_S));
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "degraded", "札の無い先頭を待つ: {}", stdout_of(&out));
    assert!(!stdout_of(&out).contains("skipped-dead="), "外した便は無い: {}", stdout_of(&out));
    assert_eq!(exported_skipped_dead(&state, &id_b), "", "面 5 に skipped_dead を書かない");
    clean(&[&repo, &state]);
}

/// 番を取った記帳（設計 pipeline.md §22）の detail。
const TURN_TAKEN: &str = "turn:taken";

/// 便の番を取った記帳の `(kind, stage)` の列（detail が [`TURN_TAKEN`] の event 全部・kind を問わない）。
fn turn_taken_rows(state: &Path, id: &str) -> Vec<(EventKind, Option<Stage>)> {
    trail(state, id)
        .into_iter()
        .filter(|(_, _, detail)| detail.as_deref() == Some(TURN_TAKEN))
        .map(|(kind, stage, _)| (kind, stage))
        .collect()
}

/// 置き場の event のうち番の記帳（[`TURN_TAKEN`]）でない件数（番を取った後に断る周の「何も書かない」を測る）。
fn count_but_turn(state: &Path) -> usize {
    events(state).into_iter().filter(|event| event.detail.as_deref() != Some(TURN_TAKEN)).count()
}

/// (1) 待たずに番を取った周（`order=first`）は `RunStage stage=Gated detail=turn:taken` がちょうど 1 行増える
/// （既存の kind と段の組・新しい `EventKind` は無い）。
#[test]
fn pipe_land_turn_taken_first_records_one_gated_row() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, _id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    assert!(turn_taken_rows(&state, &id_a).is_empty(), "land の前は番の記帳が無い");
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "first", "stdout の land 行: {}", stdout_of(&out));
    assert_eq!(turn_taken_rows(&state, &id_a), vec![(EventKind::RunStage, Some(Stage::Gated))], "番の記帳は 1 行");
    clean(&[&repo, &state]);
}

/// (1) 待って番を取った周（`order=waited:<n>`）も 1 行だけ（待ちの途中で読み直した回数に依らない）。先に着地した
/// 前の便も自分の 1 行だけを持つ。
#[test]
fn pipe_land_turn_taken_after_waiting_records_one_gated_row() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let mut waiting = land_in_background(&repo, &state, &id_b, &rules, &lens);
    std::thread::sleep(Duration::from_secs(2));
    assert!(waiting.try_wait().expect("子の状態を読める").is_none(), "後の便は待っている");
    assert!(turn_taken_rows(&state, &id_b).is_empty(), "待っている間は番を取っていない");
    let first = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "前の便の land: {}", stderr_of(&first));
    let out = waiting.wait_with_output().expect("待っていた land が終わる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "待っていた便も land する: {}", stderr_of(&out));
    assert!(order_token(&out).starts_with("waited:"), "待って番を取った: {}", stdout_of(&out));
    let row = vec![(EventKind::RunStage, Some(Stage::Gated))];
    assert_eq!(turn_taken_rows(&state, &id_b), row, "待った便の番の記帳は 1 行");
    assert_eq!(turn_taken_rows(&state, &id_a), row, "前の便の番の記帳も 1 行");
    clean(&[&repo, &state]);
}

/// (2) 上限で縮退した周（`order=degraded`）は番を取っていない＝1 行も書かない。
#[test]
fn pipe_land_turn_not_taken_when_degraded() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (_id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(LAND_WAIT_S));
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "上限で進んで land する: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "degraded", "stdout の land 行: {}", stdout_of(&out));
    assert!(turn_taken_rows(&state, &id_b).is_empty(), "縮退の周は番を記さない: {:?}", stages(&state, &id_b));
    clean(&[&repo, &state]);
}

/// (2) 列を導けなかった周（`order=unmeasured`）も 1 行も書かない。
#[test]
fn pipe_land_turn_not_taken_when_unmeasured() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    fs::write(state.join("pipe").join(&id_a).join("verdict.json"), "{broken\n").expect("判定を壊せる");
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "読めない周も進む: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "unmeasured", "stdout の land 行: {}", stdout_of(&out));
    assert!(turn_taken_rows(&state, &id_b).is_empty(), "測れなかった周は番を記さない: {:?}", stages(&state, &id_b));
    clean(&[&repo, &state]);
}

/// `pipe.land_wait_s` の行が無い manifest は land を 1 byte も動かさない（rc 2・event 0 増・main 不変）。
#[test]
fn pipe_order_missing_land_wait_row_moves_nothing() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let path = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", None);
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    let before = event_count(&state);
    let out = land_extra(&repo, &state, &id, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "行の欠落は rc 2: {}", stdout_of(&out));
    assert!(stderr_of(&out).contains("pipe.land_wait_s が無い"), "行を名指す: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main, "main は動かない");
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

// ───── 追随の形が無い便（base が main の祖先でない）を merge-base からの rebase --onto で追随する（設計 pipeline.md §38・`s2-07l.449`・接頭辞 `pipe_land_onto_`） ─────

/// main を 1 commit 進める（path と本文）。返すのは動いた後の main の sha。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn commit_main_file(repo: &Path, path: &str, body: &str) -> String {
    let file = repo.join(path);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).expect("commit の dir を作れる");
    }
    fs::write(&file, body).expect("別便の変更を書ける");
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", path]);
    git(repo, &["rev-parse", "refs/heads/main"])
}

/// main を `onto` の commit へ巻き戻してから別の commit で進める（`.449` の 2 面目の型: 便の base が消えた commit の上に
/// 居て main の祖先でなくなり、merge-base は `onto`）。anchor の checkout ごと戻す（便の worktree は別 branch なので
/// 触らない）。返すのは動いた後の main の sha。
fn rewrite_main_from(repo: &Path, onto: &str, path: &str, body: &str) -> String {
    git(repo, &["reset", "-q", "--hard", onto]);
    commit_main_file(repo, path, body)
}

/// 便の base が main の祖先でなく merge-base が `fork` であることを現物で確かめる（fixture が狙いの形か）。
fn assert_diverged(repo: &Path, base: &str, main: &str, fork: &str) {
    assert_eq!(git(repo, &["merge-base", base, main]), fork, "merge-base は巻き戻した先");
    assert_eq!(git(repo, &["rev-list", "--count", &format!("{main}..{base}")]), "1", "base は main の祖先でない（消えた commit 1 本の上）");
}

/// 追随が**必ず衝突し**かつ base が main の祖先でない便を 1 本作る（[`conflicting_run`] の型で、便の base は seed の上の
/// 1 commit〔面の外〕・gate PASS の後に main を seed へ巻き戻して便が触った行の隣へ進める）。返すのは 便の id・便の base・
/// 動いた main の sha。
fn diverged_conflicting_run(repo: &Path, state: &Path, marker: &Path, runner: &str) -> (String, String, String) {
    let seed = git(repo, &["rev-parse", "refs/heads/main"]);
    let base = commit_main_file(repo, "notes/pre.txt", "pre\n");
    let path = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &path);
    let spawned = spawn_with(repo, state, &id, runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "turn 1 の spawn: {}", stderr_of(&spawned));
    let lens = fake_lens(marker, &lens_verdict("PASS"));
    let gated = gate_once(repo, state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate: {}", stderr_of(&gated));
    git(repo, &["reset", "-q", "--hard", &seed]);
    let moved = move_main_into_conflict(repo);
    assert_diverged(repo, &base, &moved, &seed);
    (id, base, moved)
}

/// 起こし直しの stdin の「追随」節が main と便の base の 2 sha を名指し、`--onto` の形で命じる（1 行目は不変・素の
/// `git rebase <main>` の形は無い）。
fn assert_follow_section_names_both(stdin: &str, main: &str, base: &str) {
    assert!(stdin.contains("## 追随"), "起こし直しの turn に節が付く: {stdin}");
    assert!(stdin.contains(&format!("- main が {main} へ進んだ")), "1 行目は不変（main の sha）: {stdin}");
    assert!(stdin.contains(&format!("- 便の base は {base}")), "便の base の sha も名指す: {stdin}");
    assert!(stdin.contains(&format!("`git rebase --onto {main} {base}`")), "指示は --onto の 2 sha の形: {stdin}");
    assert!(!stdin.contains(&format!("`git rebase {main}`")), "素の rebase の形は命じない: {stdin}");
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

#[test]
fn pipe_land_reruns_verify_on_main_and_fails_loud() {
    let (repo, state) = repo_with_state();
    // 1 回目（worktree）は緑・2 回目（main の実測）は赤になる verify 行。印の置き場は
    // script の中で **git の共通 dir** から解く（行に絶対 path は書けない）。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-once.sh"]"#]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    make_tree_differ(&repo, &state, &id, "refs/heads/main");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "main が赤ければ rc 1");
    assert!(stderr_of(&out).contains("main が赤い"), "理由: {}", stderr_of(&out));
    // **auto revert しない**: main は進んだまま loud に落ちる。
    assert_ne!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        base,
        "main は進んだまま（revert しない）"
    );
    assert!(
        show_line(&repo, &state, &id).contains("stage=Failed"),
        "Failed detail=main-red で残る: {}",
        show_line(&repo, &state, &id)
    );
    assert!(
        !land::verdicts_path(&state).exists(),
        "赤い周は面 5 へ export しない"
    );
    clean(&[&repo, &state]);
}

/// **land の main 実測も写しの共通 verify を撃つ**（gate と同じ順序・同じ関数）。
///
/// 契約の verify だけを撃つ実装だと、main で初めて赤くなる共通の検証（repo 共通の lint /
/// 依存監査）を素通しして便が載る。1 回目（便の worktree）は緑・2 回目（main の実測）は
/// 赤になる行で、**撃った回数**から弁別する。
#[test]
fn pipe_land_reruns_common_verify_from_vessel_copy_on_main() {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-once.sh"]"#);
    // 契約の verify（`sh verify-ok.sh`）は main でも緑＝赤いのは**写しの共通 verify** である。
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    make_tree_differ(&repo, &state, &id, "refs/heads/main");
    let out = land_once(&repo, &state, &id);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_REFUSED)),
        "main で共通 verify が赤ければ rc 1: {}",
        stderr_of(&out)
    );
    assert!(stderr_of(&out).contains("main が赤い"), "理由: {}", stderr_of(&out));
    assert!(
        show_line(&repo, &state, &id).contains("stage=Failed"),
        "Failed detail=main-red で残る: {}",
        show_line(&repo, &state, &id)
    );
    // **auto revert しない**（main は進んだまま loud に落ちる）。
    assert_ne!(git(&repo, &["rev-parse", "refs/heads/main"]), base, "main は進んだまま");
    // gate の周は緑だった＝1 回目と 2 回目で結果が変わる行を、両方の面が撃っている。
    assert_eq!(row_value(&verify_rows(&state, &id), 2, "rc"), "0", "gate では同じ行が緑");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_exports_verdict_schema1() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let exported = fs::read_to_string(land::verdicts_path(&state)).expect("verdicts.jsonl を読める");
    assert_eq!(exported.lines().count(), 1, "land ごとに 1 行: {exported}");
    let line = exported.lines().next().unwrap_or_default();
    let pairs = vessel::fleet::json_lite::parse_object(line).expect("1 行の JSON");
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    // `order` は schema 1 のまま末尾に足した任意 field（ADR-0021 §2.6 (iv)・設計 gate-cost.md §6）。
    // 既存の 7 key の並びは動かない。便の規模の 4 field（gate-cost.md §5.1・`s2-07l.189`）は `order` の後ろ。
    assert_eq!(
        keys,
        vec!["schema", "run", "bead", "sha", "verdict", "evidence", "ts", "order", "generation", "size", "files", "lines", "pub_symbols"],
        "面 5 の key 列（ADR-0004 §2.2・版番号に依らず固定）"
    );
    assert_eq!(value_of(&pairs, "schema"), "1");
    assert_eq!(value_of(&pairs, "run"), id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS");
    assert_eq!(
        value_of(&pairs, "sha"),
        git(&repo, &["rev-parse", "refs/heads/main"]),
        "sha は land した commit"
    );
    assert!(
        value_of(&pairs, "evidence").ends_with("verdict.json"),
        "evidence は verdict.json の path: {}",
        value_of(&pairs, "evidence")
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 便の規模の歯の runner（`s2-07l.189`・設計 gate-cost.md §5.1）: `src/lib.rs` の seed 1 行を 3 行へ置き換え
/// （行頭が `pub ` の行・字下げした `pub ` の行・`pub(crate)` の行）、binary の `src/blob.bin` を足す。
/// base..new の実数は files=2・lines=3/1（binary の `-\t-` は数えない）・pub_symbols=2（`pub(crate)` は数えない）。
const SIZE_RUNNER: &str = "printf 'pub fn a() {}\\n    pub fn b() {}\\npub(crate) fn c() {}\\n' > src/lib.rs \
                           && printf '\\000\\001\\002' > src/blob.bin && git add -A && git commit -q -m runner";

/// [`SIZE_RUNNER`] の便を PASS の gate まで通す（契約の size は `M`＝fixture の既定 `S` と弁別する）。
fn gated_size_run(repo: &Path, state: &Path) -> String {
    let path = write_contract(
        repo,
        &["size", "write-set"],
        &[r#"size = "M""#, r#"write-set = ["src/lib.rs", "src/blob.bin"]"#],
    );
    let id = intake(repo, state, &path);
    let spawned = spawn_with(repo, state, &id, SIZE_RUNNER);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&spawned));
    let lens = fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"));
    let gated = gate_once(repo, state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate は rc 0: {}", stderr_of(&gated));
    id
}

/// 面 5 の便の行を key/value の並びで読む（行が無ければ空）。
fn exported_pairs(state: &Path, id: &str) -> Vec<(String, vessel::fleet::json_lite::Value)> {
    let text = fs::read_to_string(land::verdicts_path(state)).unwrap_or_default();
    text.lines()
        .filter_map(|line| vessel::fleet::json_lite::parse_object(line.trim()).ok())
        .find(|pairs| value_of(pairs, "run") == id)
        .unwrap_or_default()
}

/// 面 5 の行に便の規模の 4 field が **`order` の後ろ**にこの順で載り、値が fixture の diff の実数と一致する
/// （`s2-07l.189`・設計 gate-cost.md §5.1）。size は契約の字面（`M`）で、stdout の land 行は変えない。
#[test]
fn pipe_land_size_fields_follow_order_and_match_the_diff() {
    let (repo, state) = repo_with_state();
    let id = gated_size_run(&repo, &state);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    // fixture の前提: blob は binary として `-\t-` で出る（text に化けると lines が 4/1 になり歯が別の理由で落ちる）。
    let numstat = git(&repo, &["diff", "--numstat", &format!("{new}^..{new}")]);
    assert_eq!(numstat, "-\t-\tsrc/blob.bin\n3\t1\tsrc/lib.rs", "fixture の diff の実数: {numstat}");
    let pairs = exported_pairs(&state, &id);
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys.get(7..),
        Some(&["order", "generation", "size", "files", "lines", "pub_symbols"][..]),
        "4 field は order の後ろにこの順: {keys:?}"
    );
    for (key, want) in [("size", "M"), ("files", "2"), ("lines", "3/1"), ("pub_symbols", "2")] {
        assert_eq!(value_of(&pairs, key), want, "{key} の値");
    }
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("landed={new}")), "land 行は在る: {stdout}");
    for token in ["size=", "files=", "lines=", "pub_symbols="] {
        assert!(!stdout.contains(token), "stdout の land 行は変えない（{token}）: {stdout}");
    }
    clean(&[&repo, &state]);
}

/// 負例: `git diff --numstat` を読めない周は 4 field を**全部欠く**（0 と書かない）・land は rc 0 のまま
/// （測れないことは land を止める理由ではない）。
#[test]
fn pipe_land_size_fields_are_absent_when_git_cannot_be_read() {
    let (repo, state) = repo_with_state();
    let id = gated_size_run(&repo, &state);
    let out = land_once_with_git_shim(&repo, &state, &id, " diff --numstat ", None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let pairs = exported_pairs(&state, &id);
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys.last(), Some(&"generation"), "面 5 の行は在り generation で終わる（`.382` で order の後ろに 1 つ足した）: {keys:?}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_retires_worktree_by_move_and_keeps_branch() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let live = worktree_of(&repo, &id);
    assert!(live.exists(), "land の前は便の worktree が在る");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(retired.exists(), "retired/ へ move する");
    assert!(!live.exists(), "元の場所には残らない");
    // **削除しない**（N1.2）: 中身が move で運ばれている。
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    let branches = git(&repo, &["branch", "--list", &format!("scribe2/{id}")]);
    assert!(!branches.trim().is_empty(), "branch は消さない: {branches}");
    // main 実測用の tmp worktree だけは畳む。
    assert!(
        !repo.join(".worktrees").join("scribe2").join("verify").join(&id).exists(),
        "main 実測の tmp worktree は remove する"
    );
    clean(&[&repo, &state]);
}

/// main 確認で**段①（write-set 照合）を読めなかった周**（Step の rc -1）は赤（main-red）
/// でなく `main-unmeasured` に倒す（gate §6 の INCONCLUSIVE と同じ極性・`.65` lens M3）。
/// **赤の段と同時に在っても**測れなかったが先に効く（写しの共通 verify は main で赤くなる
/// `verify-once.sh`）。fail-closed: finish（verdict export・Landed）にも main-green にも進まない。
/// main は squash で進んだまま（red と同じく auto revert しない・設計 §5.4）。
#[test]
fn pipe_land_turns_unstartable_verify_step_into_unmeasured() {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-once.sh"]"#);
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    make_tree_differ(&repo, &state, &id, "refs/heads/main");

    let out = land_once_with_unreadable_diff(&repo, &state, &id);

    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "測れない周は rc 2: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("実測できない"), "赤ではなく測れないと名乗る: {}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("cmd=write-set") && stderr_of(&out).contains("stderr=diff の path を読めない"),
        "理由に段の名と stderr の 1 行が写る: {}",
        stderr_of(&out)
    );
    assert!(!stderr_of(&out).contains("赤い"), "赤を名乗らない: {}", stderr_of(&out));
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let last = log.lines().last().unwrap_or_default();
    assert!(last.contains("\"kind\":\"RunStage\""), "最終行は RunStage: {last}");
    assert!(last.contains("\"detail\":\"main-unmeasured\""), "最終行の detail は main-unmeasured: {last}");
    assert!(!log.contains("\"detail\":\"main-red\""), "main-red は書かない: {log}");
    assert!(!log.contains("\"stage\":\"Landed\""), "Landed へ進まない（fail-closed）: {log}");
    assert!(!land::verdicts_path(&state).exists(), "面 5 へ export しない");
    // squash は verify の前に済んでいる（設計 §5.4 の順序）＝main は進んだまま・revert しない。
    let now = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(now, base, "main は squash で進んだまま（red と同じ極性）");
    // 負例（同じ便を実 git で撃ち直すと、段① が読めて赤は無い＝この歯の理由は shim だけ）:
    // 2 度目の land は前提（verdict / stale base）で断られるので、ここでは segment の弁別だけ
    // 既存の pipe_land_reruns_common_verify_from_vessel_copy_on_main（rc≠0 → main-red）に委ねる。
    clean(&[&repo, &state]);
}

/// 負例: 走って **signal で死んだ** verify 行（`code()` が無く器は -1 と記す）は「読めなかった」
/// ではなく実測の赤＝従来どおり main-red（rc 1）。rc -1 の全数を測れなかったへ倒す実装は
/// ここで落ちる（gate と同じく**段の名と rc**で見る）。
#[test]
fn pipe_land_keeps_signal_killed_verify_line_as_red() {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-kill.sh"]"#);
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    make_tree_differ(&repo, &state, &id, "refs/heads/main");

    let out = land_once(&repo, &state, &id);

    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "走って死んだ赤は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("main が赤い"), "赤と名乗る: {}", stderr_of(&out));
    assert!(!stderr_of(&out).contains("実測できない"), "測れなかったと名乗らない: {}", stderr_of(&out));
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(log.contains("\"detail\":\"main-red\""), "main-red で残る: {log}");
    assert!(!log.contains("\"detail\":\"main-unmeasured\""), "main-unmeasured は書かない: {log}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_reports_unmeasured_main_apart_from_red() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    make_tree_differ(&repo, &state, &id, "refs/heads/main");
    // main 実測用の tmp の置き場を塞ぐ＝**verify を 1 行も撃てない**。
    let blocked = repo.join(".worktrees").join("scribe2").join("verify").join(&id);
    fs::create_dir_all(&blocked).expect("tmp の置き場を塞げる");
    fs::write(blocked.join("occupied"), "x\n").expect("塞げる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "測れない周は rc 2");
    assert!(
        stderr_of(&out).contains("実測できない"),
        "**赤ではなく測れない**と名乗る: {}",
        stderr_of(&out)
    );
    assert!(!stderr_of(&out).contains("赤い"), "赤を名乗らない: {}", stderr_of(&out));
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(log.contains("\"detail\":\"main-unmeasured\""), "別の名で残す: {log}");
    assert!(!log.contains("\"detail\":\"main-red\""), "main-red は書かない: {log}");
    assert!(!land::verdicts_path(&state).exists(), "面 5 へ export しない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_removes_dirty_tmp_worktree() {
    let (repo, state) = repo_with_state();
    // detached（= main 実測の tmp）のときだけ中間物を作る verify 行。便の worktree は
    // branch 上なので clean のままで、retire の move が塞がれない。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-detached.sh"]"#]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    // **中間物で dirty になった tmp を leak させない**（設計 §5.4 の `--force`）。
    assert!(
        !repo.join(".worktrees").join("scribe2").join("verify").join(&id).exists(),
        "dirty な tmp worktree も畳む"
    );
    let listed = git(&repo, &["worktree", "list"]);
    assert!(!listed.contains("/verify/"), "worktree の登録も残らない: {listed}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_pr_cmd_runs_without_approval_event() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let before = git(&repo, &["rev-parse", "refs/heads/main"]);
    let sent = state.join("pr-args");
    // **自 repo への PR は「出す」ではない**（憲法 A4.3・ADR-0008）。main を動かさず
    // branch も PR も閉じられる＝可逆ゆえ、承認 event を積まない周でも道具は起動する。
    // 3 クラスの判定は契約の自己申告（`classes`）だけに効き、seam を使ったことから
    // 導出しない（publish を名乗る契約は従来どおり spawn の手前で Blocked＝別の歯が守る）。
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--pr-cmd", &format!("printf '%s %s' {{branch}} {{base}} > '{}'", sent.display()),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_OK)),
        "承認 event 無しでも rc 0: {}",
        stderr_of(&out)
    );
    // **道具が起動したことを file で測る**（器は道具の中身を知らない）。
    assert_eq!(
        fs::read_to_string(&sent).expect("seam へ渡した引数を読める"),
        format!("scribe2/{id} {before}"),
        "`{{branch}}` と `{{base}}` を置換して渡す"
    );
    // **main は動かさない**（merge は人が押す・A4.3 の可逆はここに乗っている）。
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        before,
        "main は 1 byte も動かない"
    );
    assert!(
        show_line(&repo, &state, &id).contains("stage=Landed"),
        "段は Landed へ進む: {}",
        show_line(&repo, &state, &id)
    );
    assert!(
        stdout_of(&out).contains("landed=pr"),
        "PR を出した形だと名乗る: {}",
        stdout_of(&out)
    );
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(
        log.contains("\"stage\":\"Landed\"") && log.contains("\"detail\":\"pr\""),
        "Landed detail=pr で終える: {log}"
    );
    assert!(
        !land::verdicts_path(&state).exists(),
        "面 5 は main へ載った便の記録ゆえ、PR の段階では書かない"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_pr_cmd_pushes_branch_without_moving_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);

    // **道具の失敗で便を終端させない**: push も PR 作成も network で落ちうるので、
    // `Failed` を焼くと再試行できない便が残る。rc 1 で何も書かず段も動かさない——だから
    // この後そのまま成功へ進める（この 2 段で「何も書かない」を測っている）。
    let broken_seam = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--pr-cmd", "exit 7",
    ]);
    assert_eq!(broken_seam.status.code(), Some(i32::from(RC_REFUSED)), "道具が落ちた周は rc 1");
    assert!(
        stderr_of(&broken_seam).contains("rc 7"),
        "道具の rc を理由に写す: {}",
        stderr_of(&broken_seam)
    );
    assert!(
        !show_line(&repo, &state, &id).contains("stage=Landed"),
        "段も動かない: {}",
        show_line(&repo, &state, &id)
    );

    // seam は `{branch}` `{base}` を置換して `sh -c` する。**道具の中身は器が知らない**
    // ので、置換の結果を file へ写して測る。
    let sent = state.join("pr-args");
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--pr-cmd", &format!("printf '%s %s' {{branch}} {{base}} > '{}'", sent.display()),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "承認 event 無しでも rc 0: {}", stderr_of(&out));
    let args = fs::read_to_string(&sent).expect("seam へ渡した引数を読める");
    assert_eq!(
        args,
        format!("scribe2/{id} {base}"),
        "`{{branch}}` と `{{base}}` を置換して渡す"
    );
    // **main は動かさない**（merge は人が押す・憲法 A4.3）。
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        base,
        "この形では main を進めない"
    );
    assert!(
        stdout_of(&out).contains("landed=pr"),
        "PR を出した形だと名乗る: {}",
        stdout_of(&out)
    );
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(
        log.contains("\"stage\":\"Landed\"") && log.contains("\"detail\":\"pr\""),
        "Landed detail=pr で終える: {log}"
    );
    // 便の worktree は畳まない（**merge は人が押すまで終わっていない**）。
    assert!(worktree_of(&repo, &id).exists(), "PR 待ちの worktree は残す");
    assert!(
        !land::verdicts_path(&state).exists(),
        "面 5 は main へ載った便の記録ゆえ、PR の段階では書かない"
    );

    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_pr_cmd_ignores_moved_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);

    // **別便が main を進めた状況**を作る。squash の口はここで stale base を理由に断るが、
    // PR の口は ref を 1 本も動かさないので CAS の old が要らない——ここで base を縛ると
    // main が動いた瞬間に PR を出せなくなる（自己ホストの便が最も踏む）。
    fs::write(repo.join("other.txt"), "x\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(moved, base, "main が進んでいる");

    let sent = state.join("pr-base");
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--pr-cmd", &format!("printf '%s' {{base}} > '{}'", sent.display()),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_OK)),
        "main が動いていても PR は出せる: {}",
        stderr_of(&out)
    );
    // **`{base}` は便が記録した base**（いまの main ではない）＝PR の比較先は便の出発点。
    assert_eq!(
        fs::read_to_string(&sent).expect("seam へ渡した base を読める"),
        base,
        "便の base を渡す（現在の main へ滑らせない）"
    );
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        moved,
        "main は 1 byte も動かさない"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_pr_cmd_refuses_empty_or_missing_value() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let before = event_count(&state);

    // 値欠け（SRS NFR4「黙って落とさない」・入口の閉包の断りで rc 2・設計 pipeline.md §14 約束 4）。
    let missing = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--pr-cmd",
    ]);
    assert_eq!(missing.status.code(), Some(i32::from(RC_BROKEN)), "値欠けは rc 2");
    assert!(
        stderr_of(&missing).contains("値が無い"),
        "値欠けだと名乗る（squash 経路へ滑らせない）: {}",
        stderr_of(&missing)
    );

    // **空文字**。`sh -c ""` は rc 0 で終わるので、素通しすると 1 行も公開していないのに
    // 「PR を出した」を記帳する（何もしていないのに「やった」が残る）。
    let empty = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--pr-cmd", "",
    ]);
    assert_eq!(empty.status.code(), Some(i32::from(RC_REFUSED)), "空の seam は rc 1");
    assert!(
        !show_line(&repo, &state, &id).contains("stage=Landed"),
        "段も動かない: {}",
        show_line(&repo, &state, &id)
    );
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    clean(&[&repo, &state]);
}

/// Gated の便を `--pr-cmd` 形で land する（main は動かず worktree も残る）。
fn land_pr(repo: &Path, state: &Path, id: &str) {
    let sent = state.join("pr-args");
    let out = run_pipe(&[
        "land", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--pr-cmd", &format!("printf '%s' {{branch}} > '{}'", sent.display()),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PR 形の land は rc 0: {}", stderr_of(&out));
}

/// `--pr-cmd` 形で land した便の id。
fn landed_pr(repo: &Path, state: &Path, design: &str, marker: &Path) -> String {
    let id = gated_pass(repo, state, design, marker);
    land_pr(repo, state, &id);
    id
}

/// retire を 1 回撃つ。
fn retire_once(repo: &Path, state: &Path, id: &str) -> Output {
    run_pipe(&[
        "retire", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ])
}

#[test]
fn pipe_retire_moves_pr_landed_worktree_and_keeps_branch() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = landed_pr(&repo, &state, &path, &marker);
    let live = worktree_of(&repo, &id);
    // `--pr-cmd` 形は worktree を畳まない（merge は人が押す）＝retire の入口の前提である。
    assert!(live.exists(), "PR 形の land の後も便の worktree は在る");

    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "retire は rc 0: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(
        stdout_of(&out).contains(&format!("retired={}", retired.display())),
        "畳んだ先を名乗る: {}",
        stdout_of(&out)
    );
    // **削除しない**（N1.2）: 中身が move で運ばれている。
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    assert!(!live.exists(), "元の場所には残らない");
    let branches = git(&repo, &["branch", "--list", &format!("scribe2/{id}")]);
    assert!(!branches.trim().is_empty(), "branch は消さない: {branches}");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let last = log.lines().rfind(|line| !line.is_empty()).unwrap_or_default();
    assert!(
        last.contains("\"stage\":\"Landed\"") && last.contains("\"detail\":\"retired\""),
        "最終行は Landed detail=retired（段は Landed のまま）: {last}"
    );
    let after = event_count(&state);

    // 2 度目は前提（worktree が在る）を満たさない＝**rc 1 で何も書かない**。畳んだ先へ
    // 2 周目の move を当てると、retired/<id>/<id> のような入れ子が静かに生まれる。
    let again = retire_once(&repo, &state, &id);
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "2 度目は rc 1");
    assert_eq!(event_count(&state), after, "前提違反は event を 1 件も書かない");
    assert!(retired.exists(), "畳んだ先は在るまま");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_retire_refuses_unless_landed_and_clean() {
    // **2 つの前提は「断ってから解いて通す」で測る**。rc 1 だけを見ると、subcommand を
    // 持っていない器でも同じ rc 1 が返るので歯が空虚になる（stderr の文言は pin しない）。
    //
    // (a) 段が Gated のまま＝**終端していない便の worktree は畳まない**。
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let before = event_count(&state);
    let early = retire_once(&repo, &state, &id);
    assert_eq!(early.status.code(), Some(i32::from(RC_REFUSED)), "Landed 以外は rc 1");
    assert!(worktree_of(&repo, &id).exists(), "断った周は worktree を動かさない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    // 段だけを解くと同じ便が通る＝上の rc 1 は**段**を理由にしている。
    land_pr(&repo, &state, &id);
    let landed = retire_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "Landed なら通る: {}", stderr_of(&landed));
    clean(&[&repo, &state]);

    // (b) Landed でも worktree が dirty なら畳まない（fail-closed・untracked も数える）。
    // move は中身ごと運ぶので、未 commit の仕事を持った worktree を黙って動かすと
    // 「どこへ行ったか」が便の外から読めなくなる。
    let (dirty_repo, dirty_state) = repo_with_state();
    let dirty_path = write_contract(&dirty_repo, &[], &[]);
    let dirty_marker = dirty_state.join("lens-ran");
    let dirty_id = landed_pr(&dirty_repo, &dirty_state, &dirty_path, &dirty_marker);
    let live = worktree_of(&dirty_repo, &dirty_id);
    let stray = live.join("dirty.txt");
    fs::write(&stray, "x\n").expect("worktree を汚せる");
    let dirty_before = event_count(&dirty_state);
    let out = retire_once(&dirty_repo, &dirty_state, &dirty_id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "dirty な worktree は rc 1");
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert!(stray.exists(), "汚れもそのまま残す（掃除しない）");
    assert_eq!(event_count(&dirty_state), dirty_before, "event を 1 件も書かない");
    let dirty_retired = dirty_repo.join(".worktrees").join("scribe2").join("retired").join(&dirty_id);
    assert!(!dirty_retired.exists(), "retired/<id> を作らない");
    // 汚れだけを拭うと同じ便が通る＝上の rc 1 は**clean**を理由にしている。
    fs::remove_file(&stray).expect("汚れを拭える");
    let cleaned = retire_once(&dirty_repo, &dirty_state, &dirty_id);
    assert_eq!(cleaned.status.code(), Some(i32::from(RC_OK)), "clean なら通る: {}", stderr_of(&cleaned));
    assert!(dirty_retired.exists(), "畳んだ先が出来る");
    clean(&[&dirty_repo, &dirty_state]);
}

/// 同一変更の 2 便: 2 本目が `Failed detail=rebase-empty` で終端した後、その worktree を
/// `pipe retire` が畳む（`s2-07l.128`）。成果は既に main に在り**入れ物だけが残る**形は
/// `--pr-cmd` 形の `Landed` と同じで、畳み方も同じ 1 本（move・branch は残す・main 不変）。
/// 残す event の段は **`Failed` のまま**＝retire は終端を動かさない。
#[test]
fn pipe_retire_rebase_empty_folds_failed_run_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_b, landed) = gated_run_whose_change_is_already_on_main(&repo, &state, &marker);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let empty = run_pipe(&[
        "land", "--run", &id_b, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(
        empty.status.code(),
        Some(i32::from(RC_REFUSED)),
        "2 本目は rebase-empty で終端する: {}",
        stderr_of(&empty)
    );
    assert!(show_line(&repo, &state, &id_b).contains("stage=Failed"), "終端の段は Failed");
    let live = worktree_of(&repo, &id_b);
    assert!(live.exists(), "終端した便の worktree は残る（retire の入口の前提）");

    let out = retire_once(&repo, &state, &id_b);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rebase-empty の便も畳める: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id_b);
    assert!(
        stdout_of(&out).contains(&format!("retired={}", retired.display())),
        "畳んだ先を名乗る: {}",
        stdout_of(&out)
    );
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    assert!(!live.exists(), "元の場所が空く");
    let branches = git(&repo, &["branch", "--list", &format!("scribe2/{id_b}")]);
    assert!(!branches.trim().is_empty(), "branch は消さない: {branches}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), landed, "main は 1 byte も動かない");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let last = log.lines().rfind(|line| !line.is_empty()).unwrap_or_default();
    assert!(
        last.contains("\"stage\":\"Failed\"") && last.contains("\"detail\":\"retired\""),
        "最終行は Failed detail=retired（段を Landed へ動かさない）: {last}"
    );
    assert!(show_line(&repo, &state, &id_b).contains("stage=Failed"), "畳んだ後も段は Failed");
    clean(&[&repo, &state]);
}

// ─── `Failed` は detail を問わず畳める（設計 pipeline.md §24・契約表の行 r・接頭辞 `pipe_retire_failed_any_detail_`） ───
//
// 母集団の割れ方: base が畳めたのは 2 つの detail——`REBASE_EMPTY`（`rebase-empty`・変更が既に main に
// 在る）と `follow::EXHAUSTED`（`rebase-conflict`・起こし直しの上限に達した）——だけで、この 2 つは
// `pipe_retire_rebase_empty_folds_failed_run_and_keeps_stage` と
// `pipe_follow_retire_folds_an_exhausted_run_and_keeps_the_stage` が引き続き pin する（歯が字面で持つ
// 終端の理由の出所はこの 2 つの定数である）。下の 4 本は **base が断っていた側**の detail を 1 つずつ名指す。

/// 終端した便を 1 本畳み、畳んだ後の面を全部測る（`pipe_retire_failed_any_detail_` の 4 本が共有する）:
/// rc 0・畳んだ先を名乗る・`retired/` へ**中身ごと**運ぶ（可逆 move・N1.2）・元の場所が空く・
/// branch は消さない・残す `RunStage` は**段そのまま**で `detail=retired`（終端を動かさない）。
fn folds_and_keeps_stage(repo: &Path, state: &Path, id: &str, stage: Stage) {
    let live = worktree_of(repo, id);
    assert!(live.exists(), "畳む前の便の worktree は在る（retire の入口の前提）");
    let out = retire_once(repo, state, id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "終端した便は畳める: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(id);
    assert!(
        stdout_of(&out).contains(&format!("retired={}", retired.display())),
        "畳んだ先を名乗る: {}",
        stdout_of(&out)
    );
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない＝可逆）");
    assert!(!live.exists(), "元の場所が空く");
    let branches = git(repo, &["branch", "--list", &format!("scribe2/{id}")]);
    assert!(!branches.trim().is_empty(), "branch は消さない: {branches}");
    assert_eq!(
        stages(state, id).last().cloned(),
        Some((Some(stage), Some("retired".to_owned()))),
        "残す event は段そのままで detail=retired: {:?}",
        stages(state, id)
    );
}

/// `Failed` の便は **detail を問わず**畳める（設計 §24 の約束 1 / 2）。base が断っていた 4 つの
/// detail を 1 つずつ名指す 4 本の 1 本目——**main の実測が赤かった便**（`main-red`）。
///
/// 段の弁別から detail を外しても **clean の検査は残る**ので、汚れた木で断ってから拭って通す
/// 対で測る（上の rc 1 は clean を理由にしており、detail ではない）。
#[test]
fn pipe_retire_failed_any_detail_main_red_folds_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    // 1 回目（worktree）は緑・2 回目（main の実測）は赤になる verify 行＝`main-red` で終端する。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-once.sh"]"#]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    make_tree_differ(&repo, &state, &id, "refs/heads/main");
    let red = land_once(&repo, &state, &id);
    assert_eq!(red.status.code(), Some(i32::from(RC_REFUSED)), "main が赤い land は rc 1: {}", stderr_of(&red));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("main-red".to_owned()))),
        "終端の理由は main-red: {:?}",
        stages(&state, &id)
    );

    // 対の負例: 汚れた木は畳まない（rc 1・worktree 不動・event 0 増）。
    let live = worktree_of(&repo, &id);
    let stray = live.join("dirty.txt");
    fs::write(&stray, "x\n").expect("worktree を汚せる");
    let before = event_count(&state);
    let dirty = retire_once(&repo, &state, &id);
    assert_eq!(dirty.status.code(), Some(i32::from(RC_REFUSED)), "dirty な worktree は rc 1: {}", stdout_of(&dirty));
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert!(
        !repo.join(".worktrees").join("scribe2").join("retired").join(&id).exists(),
        "retired/<run> を作らない"
    );
    assert_eq!(event_count(&state), before, "前提違反は event を 1 件も書かない");
    fs::remove_file(&stray).expect("汚れを拭える");

    folds_and_keeps_stage(&repo, &state, &id, Stage::Failed);
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "畳んだ後も段は Failed");
    clean(&[&repo, &state]);
}

/// 2 本目: main を**実測できなかった**便（`main-unmeasured`・rc 2 で終端する側）。
#[test]
fn pipe_retire_failed_any_detail_main_unmeasured_folds_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    make_tree_differ(&repo, &state, &id, "refs/heads/main");
    // main 実測用の tmp の置き場を塞ぐ＝**verify を 1 行も撃てない**
    // （`pipe_land_reports_unmeasured_main_apart_from_red` と同じ fixture）。
    let blocked = repo.join(".worktrees").join("scribe2").join("verify").join(&id);
    fs::create_dir_all(&blocked).expect("tmp の置き場を塞げる");
    fs::write(blocked.join("occupied"), "x\n").expect("塞げる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "測れない周は rc 2: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("main-unmeasured".to_owned()))),
        "終端の理由は main-unmeasured: {:?}",
        stages(&state, &id)
    );
    folds_and_keeps_stage(&repo, &state, &id, Stage::Failed);
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "畳んだ後も段は Failed");
    clean(&[&repo, &state]);
}

/// 3 本目: 追随の rebase の**途中で** turn が終わった便（`rebase-dirty`）。木が rebase の途中＝
/// dirty なので retire は clean の検査で断り、木を戻すと同じ便が通る——この対で「断りは clean・
/// 段の弁別は `Failed` を detail ごと通す」の両方が測れる。
#[test]
fn pipe_retire_failed_any_detail_rebase_dirty_folds_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, LEAVE_MID_REBASE);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "rebase の途中は rc 1: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-dirty".to_owned()))),
        "終端の理由は rebase-dirty: {:?}",
        stages(&state, &id)
    );
    assert!(mid_rebase(&repo, &id), "木は rebase の途中のまま（器は触らない）");

    let live = worktree_of(&repo, &id);
    let before = event_count(&state);
    let dirty = retire_once(&repo, &state, &id);
    assert_eq!(dirty.status.code(), Some(i32::from(RC_REFUSED)), "rebase の途中の木は rc 1: {}", stdout_of(&dirty));
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert_eq!(event_count(&state), before, "前提違反は event を 1 件も書かない");
    // 木を戻すと同じ便が通る＝上の rc 1 は clean を理由にしている（終端の理由ではない）。
    git(&live, &["rebase", "--abort"]);
    folds_and_keeps_stage(&repo, &state, &id, Stage::Failed);
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は 1 byte も動かない");
    clean(&[&repo, &state]);
}

/// 4 本目: gate の前提違反で終端した便（`precheck:…`）。commit を 1 本も持たない木は gate が
/// `Failed detail=precheck:…` で残す（木は clean のまま）＝ここでの rc 0 は **detail の弁別が
/// 無い**ことだけを測る。
#[test]
fn pipe_retire_failed_any_detail_precheck_folds_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 便の commit を捨てる＝precheck が「commit が 1 本も無い」で落ちる（木は clean のまま）。
    let live = worktree_of(&repo, &id);
    git(&live, &["reset", "--hard", "refs/heads/main"]);
    assert!(git(&live, &["status", "--porcelain"]).is_empty(), "木は clean のまま");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "前提違反は rc 1: {}", stderr_of(&out));
    assert!(!marker.exists(), "**lens を起動しない**（precheck で止まる周）");
    let terminal = stages(&state, &id).last().cloned();
    assert!(
        matches!(&terminal, Some((Some(Stage::Failed), Some(detail))) if detail.starts_with("precheck:")),
        "終端の理由は precheck:…: {terminal:?}"
    );
    folds_and_keeps_stage(&repo, &state, &id, Stage::Failed);
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "畳んだ後も段は Failed");
    clean(&[&repo, &state]);
}

/// 退行の pin（設計 §24 の約束 4）: **非終端**（`Spawned` / `Implemented`）と `Gated(PASS)` は
/// 畳まない。`Failed` から detail の弁別を外しても `allowed` の列は不変で、段の一般則は効き
/// 続ける——`Spawned` は終端させると同じ便が通る＝その rc 1 は**段**を理由にしている。
#[test]
fn pipe_retire_failed_any_detail_still_refuses_live_runs_and_gated_pass() {
    // (a) `Spawned`（runner が生きている便）。
    let (repo, state) = repo_with_state();
    let id = spawned_run(&repo, &state);
    let live = worktree_of(&repo, &id);
    let before = event_count(&state);
    let spawned = retire_once(&repo, &state, &id);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_REFUSED)), "Spawned は rc 1: {}", stdout_of(&spawned));
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    // 段を終端（`Stopped`）へ解くと同じ便が通る＝`allowed` の列は不変である。
    stop_run_ok(&state, &id);
    let stopped = retire_once(&repo, &state, &id);
    assert_eq!(stopped.status.code(), Some(i32::from(RC_OK)), "Stopped なら通る: {}", stderr_of(&stopped));
    clean(&[&repo, &state]);

    // (b) `Implemented`（gate を待つ便）→ (c) `Gated(PASS)`（land が残っている便）。
    let (other, other_state) = repo_with_state();
    let path = write_contract(&other, &[], &[]);
    let waiting_id = implemented(&other, &other_state, &path);
    let waiting_tree = worktree_of(&other, &waiting_id);
    let waiting_before = event_count(&other_state);
    let waiting = retire_once(&other, &other_state, &waiting_id);
    assert_eq!(waiting.status.code(), Some(i32::from(RC_REFUSED)), "Implemented は rc 1: {}", stdout_of(&waiting));
    assert!(waiting_tree.exists(), "断った周は worktree を動かさない");
    assert_eq!(event_count(&other_state), waiting_before, "event を 1 件も書かない");

    let marker = other_state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&other, &other_state, &waiting_id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate は rc 0: {}", stderr_of(&gated));
    let pass_before = event_count(&other_state);
    let pass = retire_once(&other, &other_state, &waiting_id);
    assert_eq!(pass.status.code(), Some(i32::from(RC_REFUSED)), "Gated(PASS) は rc 1: {}", stdout_of(&pass));
    assert!(waiting_tree.exists(), "断った周は worktree を動かさない");
    assert!(
        !other.join(".worktrees").join("scribe2").join("retired").join(&waiting_id).exists(),
        "retired/<run> を作らない"
    );
    assert_eq!(event_count(&other_state), pass_before, "event を 1 件も書かない");
    clean(&[&other, &other_state]);
}

/// runner が**生きている**便（段 `Spawned`・commit 0・clean の worktree）の id。
///
/// 偽 runner は commit を 1 本も作らず前景で眠るだけなので、spawn を**背景で**起こして席が Live に
/// なるまで待つ（`lifecycle.rs` の `pipe_stop_all_terminates_live_runner` と同じ「生きた席を止める」形）。
/// spawn の process はここで外す——外さないと runner の終了を見届けた spawn が自分の記帳を足し、
/// 次の段（stop など）の event と数が混ざる。runner は席の group ごと stop が止める。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn spawned_run(repo: &Path, state: &Path) -> String {
    let design = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &design);
    let mut spawner = pipe_cmd(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", "sleep 300",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("binary を起動できる");
    let begun = Instant::now();
    while kind_count(state, &id, EventKind::SeatSpawned) < 1 {
        assert!(spawner.try_wait().ok().flatten().is_none(), "spawn が席を立てる前に終わった");
        assert!(begun.elapsed() < Duration::from_secs(60), "席が Live にならない");
        std::thread::sleep(Duration::from_millis(20));
    }
    spawner.kill().ok();
    spawner.wait().ok();
    assert!(show_line(repo, state, &id).contains("stage=Spawned"), "席が立った便の段は Spawned");
    id
}

/// `pipe stop --run` で終端した便（段 `Stopped`・commit 0・clean の worktree が残る形）の id。
/// 生きた席（[`spawned_run`]）を 1 本止めるだけで、止め方も畳み方も器の口に委ねる。
fn stopped_run(repo: &Path, state: &Path) -> String {
    let id = spawned_run(repo, state);
    stop_run_ok(state, &id);
    assert!(show_line(repo, state, &id).contains("stage=Stopped"), "終端の段は Stopped");
    let live = worktree_of(repo, &id);
    assert!(live.is_dir(), "止めた便の worktree は残る（retire の入口の前提）");
    assert!(git(&live, &["status", "--porcelain"]).trim().is_empty(), "commit 0 の木は clean のまま");
    id
}

/// `Stopped` の便を `pipe retire` が畳む（`s2-07l.284`・設計 pipeline-conflict.md §5）。stop は
/// 畳まない（C2）ので、止めた便の commit 0・clean の worktree はこの口でしか動かせない。畳み方は
/// 他の終端と同じ 1 本（move・branch は残す・main 不変）で、残す event の段は **`Stopped` のまま**。
#[test]
fn pipe_retire_stopped_folds_a_clean_stopped_run_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let main_before = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = stopped_run(&repo, &state);
    let live = worktree_of(&repo, &id);

    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "Stopped の便も畳める: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(
        stdout_of(&out).contains(&format!("retired={}", retired.display())),
        "畳んだ先を名乗る: {}",
        stdout_of(&out)
    );
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    assert!(!live.exists(), "元の場所が空く");
    let branches = git(&repo, &["branch", "--list", &format!("scribe2/{id}")]);
    assert!(!branches.trim().is_empty(), "branch は消さない: {branches}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main_before, "main は 1 byte も動かない");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let last = log.lines().rfind(|line| !line.is_empty()).unwrap_or_default();
    assert!(
        last.contains("\"stage\":\"Stopped\"") && last.contains("\"detail\":\"retired\""),
        "最終行は Stopped detail=retired（段を Landed へ動かさない）: {last}"
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Stopped"), "畳んだ後も段は Stopped");
    let after = event_count(&state);

    // 2 度目は前提（worktree が在る）を満たさない＝rc 1 で何も書かない。
    let again = retire_once(&repo, &state, &id);
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "2 度目は rc 1");
    assert_eq!(event_count(&state), after, "前提違反は event を 1 件も書かない");
    assert!(retired.exists(), "畳んだ先は在るまま");
    clean(&[&repo, &state]);
}

/// 負例: `Stopped` でも worktree が dirty なら畳まない（rc 1・worktree 不動・event 0 増）。
/// 負例を**clean 検査で断る位置**に置く＝段の検査は通っている（上の歯との対で、`Stopped` に
/// `Extra::Retire` の clean 検査が同じく効くことを担保する・untracked も数える）。
#[test]
fn pipe_retire_stopped_refuses_a_dirty_worktree() {
    let (repo, state) = repo_with_state();
    let id = stopped_run(&repo, &state);
    let live = worktree_of(&repo, &id);
    let stray = live.join("dirty.txt");
    fs::write(&stray, "x\n").expect("worktree を汚せる");
    let before = event_count(&state);

    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "dirty な worktree は rc 1: {}", stdout_of(&out));
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert!(stray.exists(), "汚れもそのまま残す（掃除しない）");
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(!retired.exists(), "retired/<run> を作らない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Stopped"), "段は Stopped のまま");
    // 汚れだけを拭うと同じ便が通る＝上の rc 1 は**clean**を理由にしている（段ではない）。
    fs::remove_file(&stray).expect("汚れを拭える");
    let cleaned = retire_once(&repo, &state, &id);
    assert_eq!(cleaned.status.code(), Some(i32::from(RC_OK)), "clean なら通る: {}", stderr_of(&cleaned));
    assert!(retired.exists(), "畳んだ先が出来る");
    clean(&[&repo, &state]);
}

// ─── 審査の段の終端も畳める（契約表の行 i・設計 pipeline.md §12・接頭辞 `pipe_retire_reviewed_`） ───
//
// 出所: 審査（FR49）が足した終端 `Reviewed(FAIL / INCONCLUSIVE)` は live を持たないのに retire の入口の
// 段の列に無く、前の周が残した worktree を畳めなかった＝再開（FR14）の続きの段が別の worktree に割れる。
//
// 審査の段そのものは worktree を作らない（`pipe_review_fail_stops_before_spawn` が「worktree を作らない」を
// pin する）ので、この 4 本が畳む / 断る入れ物は **前の周が残した worktree**——spawn と同じ形（branch
// `scribe2/<run>`）で [`reviewed_with_worktree`] が置く。

/// 審査の判定 file（run dir の `review.json`）を `body` の字面へ差し替える。段の event は動かさない
/// （`Reviewed` のまま）＝判定だけを振って入口の弁別を測れる。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_review_verdict(state: &Path, id: &str, body: &str) {
    fs::write(run_dir(state, id).join(REVIEW_FILE), body).expect("審査の判定を書ける");
}

/// 審査の判定 file の本文（3 値の字面をそのまま持つ＝3 値の外も書ける）。
fn review_body(verdict: &str) -> String {
    format!("{{\"verdict\":\"{verdict}\"}}\n")
}

/// 段が `Reviewed` で、判定が `body`・**前の周の worktree が残っている**便の id。
///
/// intake は偽 PASS の lens で 1 回通り（審査を飛ばす口は無い・FR49）、判定 file だけを後から差し替える。
/// worktree は spawn と同じ形（`scribe2/<run>` の branch を切って base の main から）で置く。
fn reviewed_with_worktree(repo: &Path, state: &Path, body: &str) -> String {
    let path = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &path);
    assert!(show_line(repo, state, &id).contains("stage=Reviewed"), "受付の直後の段は Reviewed");
    write_review_verdict(state, &id, body);
    let live = worktree_of(repo, &id);
    git(repo, &[
        "worktree", "add", "-q", "-b", &format!("scribe2/{id}"),
        &live.display().to_string(), "refs/heads/main",
    ]);
    assert!(live.join("src").join("lib.rs").exists(), "前の周の worktree は中身ごと残っている");
    id
}

/// 審査が **FAIL** で終端した `Reviewed` の便を畳む（判定に届いた終端・段の列に `Reviewed` が在る）。
/// 畳み方は他の終端と同じ 1 本（move・branch は残す・main 不変）で、残す event の段は `Reviewed` のまま。
#[test]
fn pipe_retire_reviewed_fail_folds_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let id = reviewed_with_worktree(&repo, &state, &review_body("FAIL"));
    let main_before = git(&repo, &["rev-parse", "refs/heads/main"]);
    folds_and_keeps_stage(&repo, &state, &id, Stage::Reviewed);
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main_before, "main は 1 byte も動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Reviewed"), "畳んだ後も段は Reviewed");
    let after = event_count(&state);

    // 2 度目は前提（worktree が在る）を満たさない＝rc 1 で何も書かない（入れ子の retired/<run>/<run> を作らない）。
    let again = retire_once(&repo, &state, &id);
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "2 度目は rc 1");
    assert_eq!(event_count(&state), after, "前提違反は event を 1 件も書かない");
    clean(&[&repo, &state]);
}

/// 審査が **INCONCLUSIVE** で終端した便も同じく畳める（FAIL との弁別は入口に無い＝どちらも「この材料では
/// 通らなかった」終端）。残す event の段は `Reviewed` のまま・`detail=retired`。
#[test]
fn pipe_retire_reviewed_inconclusive_folds_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let id = reviewed_with_worktree(&repo, &state, &review_body("INCONCLUSIVE"));
    let main_before = git(&repo, &["rev-parse", "refs/heads/main"]);
    folds_and_keeps_stage(&repo, &state, &id, Stage::Reviewed);
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main_before, "main は 1 byte も動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Reviewed"), "畳んだ後も段は Reviewed");
    clean(&[&repo, &state]);
}

/// 負例 (1): 審査が **PASS** の便は畳まない——これから起こす側（live）で、入れ物は次の段が使う。
///
/// 断りは**段だけでなく判定も名乗る**。字面は 1 行丸ごとで測る＝段違いの一般則の字面（`run <id> の段は
/// Reviewed である`・verdict の括弧を持たない）では通らず、括弧の語は `ReviewCheck::as_str` が `Passed` に
/// 返す `PASS` である。一般則そのものの形は末尾で `Implemented` の便から実測して対に置く。
#[test]
fn pipe_retire_reviewed_pass_refused_and_names_the_verdict() {
    let (repo, state) = repo_with_state();
    let id = reviewed_with_worktree(&repo, &state, &review_body("PASS"));
    let live = worktree_of(&repo, &id);
    let before = event_count(&state);

    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "Reviewed(PASS) は rc 1: {}", stdout_of(&out));
    assert_eq!(
        stderr_of(&out).trim(),
        format!("pipe: run {id} の段は Reviewed である（verdict=PASS）"),
        "断りは判定の語まで名乗る"
    );
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert!(
        !repo.join(".worktrees").join("scribe2").join("retired").join(&id).exists(),
        "retired/<run> を作らない"
    );
    assert_eq!(event_count(&state), before, "前提違反は event を 1 件も書かない");
    // 判定だけを終端の側へ解くと同じ便が通る＝上の rc 1 は **verdict** を理由にしている（段ではない）。
    write_review_verdict(&state, &id, &review_body("FAIL"));
    let folded = retire_once(&repo, &state, &id);
    assert_eq!(folded.status.code(), Some(i32::from(RC_OK)), "FAIL なら通る: {}", stderr_of(&folded));
    clean(&[&repo, &state]);

    // 対の実測: 段の列に無い段（`Implemented`）の断りは **一般則のまま**で、verdict の括弧を持たない。
    let (other, other_state) = repo_with_state();
    let other_path = write_contract(&other, &[], &[]);
    let waiting = implemented(&other, &other_state, &other_path);
    let general = retire_once(&other, &other_state, &waiting);
    assert_eq!(general.status.code(), Some(i32::from(RC_REFUSED)), "Implemented は rc 1: {}", stdout_of(&general));
    assert_eq!(
        stderr_of(&general).trim(),
        format!("pipe: run {waiting} の段は Implemented である"),
        "一般則は段だけを名乗る"
    );
    clean(&[&other, &other_state]);
}

/// 負例 (2): 判定を**読めない**便も畳まない（fail-closed・読めない判定を終端に読み替えない）。母集団は
/// 「JSON でない本文」と「3 値の外」の 2 つで、どちらも同じ 1 行（括弧の語は `Unreadable` の `読めない`）。
#[test]
fn pipe_retire_reviewed_unreadable_refused_and_names_the_verdict() {
    let (repo, state) = repo_with_state();
    let id = reviewed_with_worktree(&repo, &state, "not json\n");
    let live = worktree_of(&repo, &id);
    let before = event_count(&state);
    let reason = format!("pipe: run {id} の段は Reviewed である（verdict=読めない）");

    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "読めない判定は rc 1: {}", stdout_of(&out));
    assert_eq!(stderr_of(&out).trim(), reason, "断りは「読めない」を名乗る");
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert!(
        !repo.join(".worktrees").join("scribe2").join("retired").join(&id).exists(),
        "retired/<run> を作らない"
    );
    assert_eq!(event_count(&state), before, "前提違反は event を 1 件も書かない");
    // 3 値の外も同じ断り（PASS でない字面を「終端」に読み替えない）。
    write_review_verdict(&state, &id, &review_body("MAYBE"));
    let outside = retire_once(&repo, &state, &id);
    assert_eq!(outside.status.code(), Some(i32::from(RC_REFUSED)), "3 値の外も rc 1: {}", stdout_of(&outside));
    assert_eq!(stderr_of(&outside).trim(), reason, "3 値の外も「読めない」");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    // 判定を読める終端へ直すと同じ便が通る＝上の rc 1 は**判定の読めなさ**を理由にしている（段ではない）。
    write_review_verdict(&state, &id, &review_body("INCONCLUSIVE"));
    let folded = retire_once(&repo, &state, &id);
    assert_eq!(folded.status.code(), Some(i32::from(RC_OK)), "読める終端なら通る: {}", stderr_of(&folded));
    clean(&[&repo, &state]);
}

// ─────────────────── land の後の anchor 同期（`s2-07l.120`・N1・接頭辞 `pipe_land_anchor_`） ───────────────────

/// land（squash 形）の後、anchor（`--repo`）の HEAD が main を指す checkout なら **index と working tree を
/// 新 main に揃える**（`.117` 実測: base は `git update-ref` だけで index が旧のまま＝`git status` に
/// `M  src/lib.rs` が残り、次の `commit -a` で landed 変更が消える経路・N1）。判定行に `anchor=synced`。
#[test]
fn pipe_land_anchor_syncs_index_and_working_tree_to_new_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    assert_eq!(git(&repo, &["status", "--porcelain", "--untracked-files=no"]).trim(), "", "land の前の anchor は clean");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert!(stdout_of(&out).contains(&format!("landed={new} main={new} anchor=synced")), "判定行に anchor=synced: {}", stdout_of(&out));
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), new, "anchor の HEAD は新 main");
    assert_eq!(
        git(&repo, &["status", "--porcelain", "--untracked-files=no"]).trim(),
        "",
        "index と working tree が新 main に揃う（M が残らない）"
    );
    let lib = fs::read_to_string(repo.join("src").join("lib.rs")).unwrap_or_default();
    assert!(lib.lines().any(|line| line == "x"), "landed 変更が anchor の working tree に在る: {lib:?}");
    clean(&[&repo, &state]);
}

/// anchor に**未 commit の変更**が在る周は触らない（成果を消さない・fail-closed）: main の ref は進めるが
/// index / working tree は揃えず、判定行に `anchor=skipped:dirty` と stderr の warning 1 行。局所の変更は
/// そのまま残る。
#[test]
fn pipe_land_anchor_skips_dirty_anchor_and_keeps_local_change() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    // 便の変更と同じ file に、commit していない局所の変更を置く（揃えると消える形）。
    fs::write(repo.join("src").join("lib.rs"), "// local uncommitted\n").expect("局所の変更を置ける");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land 自体は成立（rc 0）: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert!(stdout_of(&out).contains(&format!("landed={new} main={new} anchor=skipped:dirty")), "{}", stdout_of(&out));
    assert!(stderr_of(&out).contains("anchor"), "warning の 1 行を stderr に出す: {}", stderr_of(&out));
    assert_eq!(
        fs::read_to_string(repo.join("src").join("lib.rs")).unwrap_or_default(),
        "// local uncommitted\n",
        "未 commit の変更を消さない"
    );
    clean(&[&repo, &state]);
}

/// anchor の HEAD が main を指さない周（別 branch・detached）は触らない: `anchor=skipped:not-main`。
/// 他 branch の HEAD と working tree は不変。
#[test]
fn pipe_land_anchor_skips_when_head_is_not_main() {
    for (label, args) in [("other-branch", vec!["checkout", "-q", "-b", "other"]), ("detached", vec!["checkout", "-q", "--detach"])] {
        let (repo, state) = repo_with_state();
        let path = write_contract(&repo, &[], &[]);
        let marker = state.join("lens-ran");
        let id = gated_pass(&repo, &state, &path, &marker);
        let before = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &args);
        let out = land_once(&repo, &state, &id);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{label}: land は rc 0: {}", stderr_of(&out));
        let new = git(&repo, &["rev-parse", "refs/heads/main"]);
        assert_ne!(new, before, "{label}: main は進む");
        assert!(stdout_of(&out).contains(&format!("landed={new} main={new} anchor=skipped:not-main")), "{label}: {}", stdout_of(&out));
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), before, "{label}: anchor の HEAD は動かない");
        assert_eq!(git(&repo, &["status", "--porcelain", "--untracked-files=no"]).trim(), "", "{label}: working tree は不変で clean");
        clean(&[&repo, &state]);
    }
}

/// main の実測が**赤**でも ref は進んでいるので anchor は揃える（揃えないと failure exit で `.117` の
/// 経路が開く・lens-120 H1）。rc 1 のまま stderr に `anchor=synced` を足し、anchor は clean・HEAD == 新 main。
#[test]
fn pipe_land_anchor_syncs_even_when_main_verify_is_red() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-once.sh"]"#]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    make_tree_differ(&repo, &state, &id, "refs/heads/main");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "main が赤ければ rc 1");
    assert!(stderr_of(&out).contains("main が赤い"), "理由: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("pipe: anchor=synced"), "赤でも anchor は揃える（token を stderr に）: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), new, "anchor の HEAD は新 main");
    assert_eq!(git(&repo, &["status", "--porcelain", "--untracked-files=no"]).trim(), "", "赤でも staged の逆向きを残さない");
    clean(&[&repo, &state]);
}

/// landed tree が**足す** path が anchor に untracked（ここでは **ignored**）で在る周は触らない: `read-tree -m -u` は
/// ignored な file を黙って上書きする（実測・lens-120 M1）ので、足す path の衝突を先に見て
/// `anchor=skipped:collision`。局所の file は不変・main は進む。
#[test]
fn pipe_land_anchor_skips_when_landed_tree_adds_a_path_that_exists_ignored_in_anchor() {
    let (repo, state) = repo_with_state();
    let path = write_contract(
        &repo,
        &["write-set"],
        // base に無い file は `+` で宣言する（契約 (b) の行の形）＝anchor の ignored な同名 file は tracked でない。
        &[r#"write-set = ["src/lib.rs", "+src/new.rs"]"#],
    );
    let marker = state.join("lens-ran");
    let id = intake(&repo, &state, &path);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo n > src/new.rs && echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS の gate は rc 0: {}", stderr_of(&out));
    // anchor に ignored な同名 file を置く（`.gitignore` は untracked でも効く・tracked 変更ではない）。
    fs::write(repo.join(".gitignore"), "src/new.rs\n").expect(".gitignore を置ける");
    fs::write(repo.join("src").join("new.rs"), "// local ignored\n").expect("ignored な file を置ける");
    assert_eq!(git(&repo, &["status", "--porcelain", "--untracked-files=no"]).trim(), "", "tracked 変更は無い");

    let out = land_once(&repo, &state, &id);

    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains(" anchor=skipped:collision"), "{}", stdout_of(&out));
    assert!(stderr_of(&out).contains("collision"), "warning: {}", stderr_of(&out));
    assert_eq!(
        fs::read_to_string(repo.join("src").join("new.rs")).unwrap_or_default(),
        "// local ignored\n",
        "ignored な file を上書きしない"
    );
    assert_eq!(
        fs::read_to_string(repo.join("src").join("lib.rs")).unwrap_or_default(),
        "// seed\n",
        "衝突の周は 1 file も触らない（lib.rs も旧のまま）"
    );
    clean(&[&repo, &state]);
}

/// anchor の状態を**読めない**周（index が壊れている＝`git status` が fatal）は clean に読み替えず
/// `anchor=skipped:unreadable`（fail-closed・lens-120 M3）。main は進む。
#[test]
fn pipe_land_anchor_skips_when_status_is_unreadable() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    fs::write(repo.join(".git").join("index"), b"garbage").expect("index を壊せる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains(" anchor=skipped:unreadable"), "{}", stdout_of(&out));
    assert!(stderr_of(&out).contains("unreadable"), "warning: {}", stderr_of(&out));
    assert_ne!(git(&repo, &["rev-parse", "refs/heads/main"]), base, "main は進む");
    assert_eq!(
        fs::read_to_string(repo.join("src").join("lib.rs")).unwrap_or_default(),
        "// seed\n",
        "読めない周は working tree に触らない"
    );
    clean(&[&repo, &state]);
}

/// 見立ては Sync でも git が**途中で**断った周（`index.lock` が在る）は `anchor=skipped:sync-failed` で、
/// warning は「部分的に更新されている可能性」を名指す（状態を「旧のまま」と断定しない・lens-120 M2 / M3）。
#[test]
fn pipe_land_anchor_reports_sync_failed_when_git_refuses_midway() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    fs::write(repo.join(".git").join("index.lock"), b"").expect("index.lock を置ける");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains(" anchor=skipped:sync-failed"), "{}", stdout_of(&out));
    assert!(stderr_of(&out).contains("部分的に更新されている可能性"), "warning は状態を断定しない: {}", stderr_of(&out));
    fs::remove_file(repo.join(".git").join("index.lock")).ok();
    clean(&[&repo, &state]);
}

/// 同期は **squash の直後・main 実測の前**に走る（`s2-07l.131`・窓を秒単位へ）。
///
/// 「揃えた」だけでは順序を測れない（実測の後に揃えても最後は clean になる）ので、**実測の最中に
/// anchor がどう見えるか**を現物で採る: 契約の verify が main 実測の tmp worktree（detached）で
/// 撃たれたときだけ、置き場の record file へ印 1 行と anchor の `git status --porcelain` を落とす。
/// base（実測 → 同期）では index が旧 main のままなので `M  src/lib.rs`（staged の逆向き・`.117` の形）が
/// 記録され、head（同期 → 実測）では空になる。
///
/// **record の不在を clean に読み替えない**（lens FAIL 2026-09-12）: file が在ること・先頭行が印である
/// ことを先に要求し、その後の porcelain 部分だけを空と照合する。script は `set -e` で書き、status を
/// 読めない周は verify が赤くなって land が `main-red` で終端する（この歯はそれも落とす）。
#[test]
fn pipe_land_anchor_before_verify_records_clean_anchor_during_main_check() {
    let (repo, state) = repo_with_state();
    let record = state.join("anchor-status");
    // `--untracked-files=no`: anchor には便の `.worktrees/` が常に untracked で在る（器が作る入れ物で
    // あって「揃っていない」の合図ではない）＝器の見立て（`anchor_plan`）と同じ面を読む。
    let script = format!(
        "set -e\n\
         if [ \"$(git rev-parse --abbrev-ref HEAD)\" = HEAD ]; then\n\
         printf 'anchor-observed\\n' > '{record}'\n\
         git -C '{repo}' status --porcelain --untracked-files=no >> '{record}'\n\
         fi\n",
        record = record.display(),
        repo = repo.display(),
    );
    fs::write(repo.join("verify-probe.sh"), script).expect("probe script を書ける");
    git(&repo, &["add", "--", "verify-probe.sh"]);
    git(&repo, &["commit", "-q", "-m", "probe"]);
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-probe.sh"]"#]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    make_tree_differ(&repo, &state, &id, "refs/heads/main");
    assert!(!record.exists(), "gate（branch の worktree）は record を書かない＝印は実測の周のもの");

    let out = land_once(&repo, &state, &id);

    // 5. 既存の挙動は不変（Landed・`anchor=synced`・main は新 sha）。
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(new, base, "main は進む");
    assert!(stdout_of(&out).contains(&format!("landed={new} main={new} anchor=synced")), "判定行: {}", stdout_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "{}", show_line(&repo, &state, &id));
    // 1. record が**在る**（無いを clean に化けさせない）。
    let text = fs::read_to_string(&record).expect("main 実測の verify が record を書いている");
    let mut lines = text.lines();
    // 2. 先頭行は固定の印＝script が本当に走って書いた証拠。
    assert_eq!(lines.next(), Some("anchor-observed"), "印が先頭行: {text:?}");
    // 3. 印の後の porcelain は空＝実測の**最中に** anchor が既に新 main へ揃っている
    //    （base ではここが `M  src/lib.rs`）。
    assert_eq!(
        lines.collect::<Vec<&str>>(),
        Vec::<&str>::new(),
        "実測の最中の anchor は揃っている（同期が先）: {text:?}"
    );
    clean(&[&repo, &state]);
}

/// 偽 runner（実行 file）の runner cmd。
///
/// turn 1 は契約の実装（`src/lib.rs` の末尾へ `x` を足して commit）で、turn 2 以降は `second` の
/// 本文＝**追随の解き方をここで振る**。どの turn も呼出回数と stdin を置き場へ写すので、
/// 「起こされたか」「何を渡されたか」を rc でなく効果で測れる。`$SHA` には stdin の「追随」節が
/// 名指す main の sha、`$BASE` には同じ節が名指す便の base の sha が入る（節が無い周は空＝rebase が
/// 落ちて歯が赤くなる＝空虚にならない）。
fn stub_runner(state: &Path, second: &str) -> String {
    stub_runner_turns(state, IMPLEMENT, second)
}

/// [`stub_runner`] の turn 1 の本文も振る形（`first` = turn 1・`second` = turn 2 以降）。argv も turn ごとに
/// `argv-<n>`（1 行 1 引数）へ写す（どの口座で起こされたかを `lifecycle::stub_argv` で読む）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn stub_runner_turns(state: &Path, first: &str, second: &str) -> String {
    let dir = stub_dir(state);
    fs::create_dir_all(&dir).expect("stub の置き場を作れる");
    let path = state.join("stub-runner.sh");
    let body = format!(
        "#!/bin/sh\nD='{}'\nprintf 'call\\n' >> \"$D/calls\"\nN=$(wc -l < \"$D/calls\" | tr -d ' ')\n\
         printf '%s\\n' \"$@\" > \"$D/argv-$N\"\n\
         cat > \"$D/stdin-$N\"\nif [ \"$N\" = 1 ]; then\n{first}\nfi\n\
         SHA=$(sed -n 's/^- main が \\(.*\\) へ進んだ$/\\1/p' \"$D/stdin-$N\" | head -1)\n\
         BASE=$(sed -n 's/^- 便の base は \\(.*\\)$/\\1/p' \"$D/stdin-$N\" | head -1)\n{second}\n",
        dir.display()
    );
    fs::write(&path, body).expect("stub を書ける");
    format!("sh {}", path.display())
}

/// turn 2 の本文: 衝突を write-set の中で解いて `git rebase --continue` で終える。
const RESOLVE: &str = "if git rebase \"$SHA\"; then exit 0; fi\nprintf '// seed\\ny\\nx\\n' > src/lib.rs\ngit add src/lib.rs\nGIT_EDITOR=true git rebase --continue";

/// turn 2 の本文: 節の名指す 2 sha で `git rebase --onto <main> <base>` を撃ち（雛形の指示どおり・設計 §38）、衝突を
/// write-set の中で解いて `git rebase --continue` で終える。`$BASE` が空なら `--onto` が落ちて歯が赤くなる（空虚にならない）。
const RESOLVE_ONTO: &str = "if git rebase --onto \"$SHA\" \"$BASE\"; then exit 0; fi\nprintf '// seed\\ny\\nx\\n' > src/lib.rs\ngit add src/lib.rs\nGIT_EDITOR=true git rebase --continue";

/// turn 2 の本文: 解かずに木を戻して終わる（次の land でも同じ衝突が起きる）。
const KEEP_CONFLICT: &str = "git rebase \"$SHA\" || git rebase --abort\nexit 0";

/// turn 2 の本文: 木を戻して質問 record で止まる（**commit を作らない**）。
const ABORT_AND_ASK: &str = "git rebase \"$SHA\" || git rebase --abort\nprintf '%s\\n' '{\"question\":\"追随の衝突を解けない\",\"about\":\"write-set\"}'\nexit 76";

/// turn 2 の本文: commit を作ってから質問 record を出す（質問ではなく実装の失敗）。
const COMMIT_THEN_ASK: &str = "git rebase \"$SHA\" || git rebase --abort\nprintf 'z\\n' >> src/lib.rs\ngit add -A\ngit commit -q -m extra\nprintf '%s\\n' '{\"question\":\"追随の衝突を解けない\",\"about\":\"write-set\"}'\nexit 76";

/// turn 2 の本文: rebase の途中のまま turn を終える（木が clean でない）。
const LEAVE_MID_REBASE: &str = "git rebase \"$SHA\" || true\nexit 0";

/// main を**便が触った行の隣**へ進める（追随が必ず衝突する形）。返すのは動いた後の main。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn move_main_into_conflict(repo: &Path) -> String {
    let lib = repo.join("src").join("lib.rs");
    let mut text = fs::read_to_string(&lib).expect("seed を読める");
    text.push_str("y\n");
    fs::write(&lib, text).expect("別便の変更を書ける");
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "other"]);
    git(repo, &["rev-parse", "refs/heads/main"])
}

/// 追随が**必ず衝突する**便を 1 本作る（偽 runner の turn 1 で実装 → PASS の gate → main が
/// 同じ行の隣へ進む）。返すのは 便の id・便の base・動いた main の sha。
fn conflicting_run(repo: &Path, state: &Path, marker: &Path, runner: &str) -> (String, String, String) {
    let base = git(repo, &["rev-parse", "refs/heads/main"]);
    let path = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &path);
    let spawned = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ]);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "turn 1 の spawn: {}", stderr_of(&spawned));
    let lens = fake_lens(marker, &lens_verdict("PASS"));
    let gated = gate_once(repo, state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate: {}", stderr_of(&gated));
    let moved = move_main_into_conflict(repo);
    (id, base, moved)
}

/// land を 1 回撃つ（`extra` で `--runner` / `--lens` / `--rules` を足す）。
fn land_extra(repo: &Path, state: &Path, id: &str, extra: &[&str]) -> Output {
    let mut args: Vec<String> = ["land", "--run", id].iter().map(|item| (*item).to_owned()).collect();
    args.extend([
        "--repo".to_owned(), repo.display().to_string(),
        "--state-dir".to_owned(), state.display().to_string(),
    ]);
    args.extend(extra.iter().map(|item| (*item).to_owned()));
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_pipe(&borrowed)
}

/// 便の worktree が rebase の途中か（`rebase-merge` / `rebase-apply` の有無を現物で見る）。
fn mid_rebase(repo: &Path, id: &str) -> bool {
    let worktree = worktree_of(repo, id);
    let git_dir = PathBuf::from(git(&worktree, &["rev-parse", "--absolute-git-dir"]));
    git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
}

/// 衝突の記帳（`Implemented detail=rebase-conflict:<range>`）の件数。
fn conflict_count(state: &Path, id: &str) -> usize {
    stages(state, id)
        .into_iter()
        .filter(|(stage, detail)| {
            *stage == Some(Stage::Implemented)
                && detail.as_deref().is_some_and(|found| found.starts_with("rebase-conflict:"))
        })
        .count()
}

/// 衝突の記帳を**手で 1 件積む**（回数が replay の導出であることを測る fixture）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn record_conflict(state: &Path, id: &str, range: &str) {
    let out = bin_cmd()
        .args(["fleet", "record", "--kind", "RunStage", "--stage", "Implemented", "--run", id,
               "--bead", "s2-2e5", "--detail", &format!("rebase-conflict:{range}"), "--state-dir"])
        .arg(state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fleet record: {}", stderr_of(&out));
}

/// 衝突は**便を終端にしない**（設計 pipeline-conflict.md §3 手順 1〜2）。木を戻して
/// `RunStage Implemented detail=rebase-conflict:<base>..<main>` を **1 件**記帳し、`Failed` は
/// 1 件も書かない。main は 1 byte も動かず、worktree は rebase の途中でなく clean である。
#[test]
fn pipe_follow_records_the_conflict_without_failing_the_run() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_INCONCLUSIVE)),
        "起こし直した周は rc 3（次の段を名乗って止まる）: {}",
        stderr_of(&out)
    );
    assert!(stdout_of(&out).contains(&format!("run={id} next=gate")), "次に撃つ段: {}", stdout_of(&out));
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は 1 件: {:?}", stages(&state, &id));
    assert!(
        stages(&state, &id).iter().any(|(stage, detail)| *stage == Some(Stage::Implemented)
            && detail.as_deref() == Some(format!("rebase-conflict:{base}..{moved}").as_str())),
        "detail は base と main を名乗る: {:?}",
        stages(&state, &id)
    );
    assert!(
        !stages(&state, &id).iter().any(|(stage, _)| *stage == Some(Stage::Failed)),
        "便を終端にしない: {:?}",
        stages(&state, &id)
    );
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は 1 byte も動かない");
    assert!(!mid_rebase(&repo, &id), "木は rebase の途中でない");
    assert!(git(&worktree_of(&repo, &id), &["status", "--porcelain"]).is_empty(), "木は clean");
    assert_eq!(stub_calls(&state), 2, "実装役を 1 回起こし直した");
    clean(&[&repo, &state]);
}

/// (e) 衝突の起こし直しの周も初回の起動と同じ選定を通る（`s2-07l.285`・設計 account-autonomy.md §4「初回の起動も
/// 同じ選定を通す」の列挙 = 衝突の起こし直し・FR36）: 口座 a1 / a2 を宣言し a1 を席の登録 row に置いた置き場で
/// `--runner` 付きの land が衝突を起こし直すと、起こし直しの stub の argv に a2 の `--account-dir` が渡り、
/// `Spawned` の detail が `base:<sha>,account:a2` を持つ（turn 1 は宣言 0 の spawn＝`base:<sha>`・`Inherit` の
/// ままなら detail に `account:` が無く argv にも `--account-dir` が無い＝RED）。偽 curl は起こし直しの直前に
/// 口座 2 つ分呼ばれる。
#[test]
fn pipe_spawn_account_conflict_retry_runs_on_the_chosen_account() {
    use super::lifecycle::{argv_account_dir, curl_calls, fake_usage_curl, put_account, register_seat_account, resume_rules, spawned_details, stub_argv, windows};
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let (id, base, _moved) = conflicting_run(&repo, &state, &marker, &runner);
    let rules = resume_rules(&state, &["a1", "a2"]);
    put_account(&state, "a1", &[windows(10, 10)]);
    put_account(&state, "a2", &[windows(40, 10)]);
    register_seat_account(&state, &repo, "a1");
    let out = land_extra(&repo, &state, &id, &["--runner", &runner, "--rules", &rules, "--curl", &fake_usage_curl(&state)]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "起こし直した周は rc 3: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={id} next=gate")), "次に撃つ段: {}", stdout_of(&out));
    assert!(!stdout_of(&out).contains("next=spawn account="), "起こし直しは判定行を持たない: {}", stdout_of(&out));
    assert_eq!(stub_calls(&state), 2, "実装役を 1 回起こし直した");
    assert_eq!(curl_calls(&state), 2, "起こし直しの直前に計測を 1 回（口座 2 つ）");
    assert_eq!(argv_account_dir(&stub_argv(&state, 1)), None, "turn 1 は宣言 0 の spawn＝継承");
    assert_eq!(
        argv_account_dir(&stub_argv(&state, 2)),
        Some(state.join("accounts").join("a2").display().to_string()),
        "起こし直しは登録 row の a1 を除いた a2 で起きる: {:?}",
        stub_argv(&state, 2)
    );
    assert_eq!(
        spawned_details(&state, &id),
        vec![format!("base:{base}"), format!("base:{base},account:a2")],
        "起こし直しの記帳は base と選んだ口座を名乗る"
    );
    clean(&[&repo, &state]);
}

/// 起こし直しの turn の stdin は、契約の写しの**後ろ**に「## 追随」節を持ち、main の sha を
/// 名指す（設計 §3 手順 4）。turn 1 の stdin には節が無い（不在が既定）。
#[test]
fn pipe_follow_second_turn_receives_the_follow_section() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    let first = stub_stdin(&state, 1);
    assert!(first.contains("goal = "), "turn 1 も契約の本文を受ける: {first}");
    assert!(!first.contains("## 追随"), "追随の無い turn には節が付かない: {first}");
    let second = stub_stdin(&state, 2);
    assert!(second.contains("## 追随"), "起こし直しの turn に節が付く: {second}");
    assert!(second.contains(&moved), "節は main の sha を名指す: {second}");
    let contract_at = second.find("goal = ");
    let follow_at = second.find("## 追随");
    assert!(
        matches!((contract_at, follow_at), (Some(c), Some(f)) if c < f),
        "順序は 契約 → 追随: {second}"
    );
    clean(&[&repo, &state]);
}

/// 回答済みの質問を持つ便の起こし直しは、stdin に「回答」と「追随」を**この順**で持つ
/// （設計 §3 手順 4 の「契約 → 回答 → 追随」）。
#[test]
fn pipe_follow_answered_question_comes_before_the_follow_section() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let id = questioned(&repo, &state);
    let answered = run_pipe(&[
        "answer", "--run", &id, "--words", "verify は 1 行目だけを撃つ",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&answered));
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "回答の後の turn: {}", stderr_of(&resumed));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate: {}", stderr_of(&gated));
    move_main_into_conflict(&repo);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    let second = stub_stdin(&state, 2);
    let answer_at = second.find("## 回答");
    let follow_at = second.find("## 追随");
    assert!(
        matches!((answer_at, follow_at), (Some(a), Some(f)) if a < f),
        "順序は 回答 → 追随: {second}"
    );
    assert!(second.contains("verify は 1 行目だけを撃つ"), "回答の逐語も運ぶ: {second}");
    clean(&[&repo, &state]);
}

/// 実装役が衝突を解いた周: 器が**実測した merge-base**で base を進め（`rebase:<old>..<new>`）、
/// 続きの gate が新しい base で PASS（先着便の file が write-set の外に載らない）→ land で
/// `Landed`。起こし直しは 1 回だけ（3 turn 目は起こされない）。
#[test]
fn pipe_follow_resolved_conflict_advances_the_base_and_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "起こし直した周は rc 3: {}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains(&format!("run={id} rebase={base}..{moved}")),
        "新しい base を名乗る: {}",
        stdout_of(&out)
    );
    assert!(
        stages(&state, &id).iter().any(|(stage, detail)| *stage == Some(Stage::Implemented)
            && detail.as_deref() == Some(format!("rebase:{base}..{moved}").as_str())),
        "器が base を進めた記帳: {:?}",
        stages(&state, &id)
    );
    assert!(!mid_rebase(&repo, &id), "木は rebase の途中でない");
    // 続きは gate から（新しい base の 2 点 diff は write-set の中だけ）。
    fs::remove_file(&marker).ok();
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "新しい base の gate は PASS: {}", stderr_of(&gated));
    let landed = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(
        git(&repo, &["show", &format!("{new}:src/lib.rs")]),
        "// seed\ny\nx",
        "先着便の行と便の行が両方 main に載る"
    );
    let landings = trail(&state, &id)
        .into_iter()
        .filter(|(kind, stage, _)| *kind == EventKind::RunDone && *stage == Some(Stage::Landed))
        .count();
    assert_eq!(landings, 1, "Landed は 1 件");
    assert_eq!(stub_calls(&state), 2, "起こし直しは 1 回だけ");
    clean(&[&repo, &state]);
}

/// turn の間に main がさらに進んだ周でも、**書く値は実測した merge-base**であって現在の main
/// ではない（2 点 diff に main の新しい commit の逆向きを載せない・設計 §3 手順 5）。
#[test]
fn pipe_follow_records_the_measured_merge_base_not_the_moving_main() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let racing = format!("{RESOLVE}\ngit -C '{}' commit -q --allow-empty -m racing", repo.display());
    let runner = stub_runner(&state, &racing);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    let raced = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(raced, moved, "turn の中で main がさらに進んでいる");
    let details: Vec<String> = stages(&state, &id)
        .into_iter()
        .filter_map(|(_, detail)| detail)
        .filter(|detail| detail.starts_with("rebase:"))
        .collect();
    assert_eq!(
        details,
        vec![format!("rebase:{base}..{moved}")],
        "書く値は merge-base（現在の main {raced} でない）"
    );
    clean(&[&repo, &state]);
}

/// 上限（fixture の rules で 1 回）まで起こし直し、2 回目の衝突で終端した便を作る。
/// 返すのは 便の id・動いた main の sha・偽 runner の cmd。
fn exhausted_run(repo: &Path, state: &Path, marker: &Path) -> (String, String) {
    let runner = stub_runner(state, KEEP_CONFLICT);
    let (id, _base, moved) = conflicting_run(repo, state, marker, &runner);
    let rules = write_rules_with_retries(state, "rules-retry-1.toml", 1, 1_000_000, 1);
    let rules_arg = rules.display().to_string();
    let first = land_extra(repo, state, &id, &["--runner", &runner, "--rules", &rules_arg]);
    assert_eq!(
        first.status.code(),
        Some(i32::from(RC_INCONCLUSIVE)),
        "上限の内は起こし直す: {}",
        stderr_of(&first)
    );
    fs::remove_file(marker).ok();
    let lens = fake_lens(marker, &lens_verdict("PASS"));
    let gated = gate_with_rules(repo, state, &id, &rules, &lens);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "撃ち直しの gate: {}", stderr_of(&gated));
    let second = land_extra(repo, state, &id, &["--runner", &runner, "--rules", &rules_arg]);
    assert_eq!(
        second.status.code(),
        Some(i32::from(RC_REFUSED)),
        "上限に達した周は rc 1: {}",
        stderr_of(&second)
    );
    (id, moved)
}

/// 上限の歯（値 1 ＝最大 1 回起こし直す）: 2 回目の衝突で `Failed detail=rebase-conflict` になり、
/// runner は 3 turn 目に起こされない。main は動かない。
#[test]
fn pipe_follow_stops_retrying_at_the_limit() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, moved) = exhausted_run(&repo, &state, &marker);
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-conflict".to_owned()))),
        "終端の理由: {:?}",
        stages(&state, &id)
    );
    assert_eq!(conflict_count(&state, &id), 2, "衝突は 2 件記帳された: {:?}", stages(&state, &id));
    assert_eq!(stub_calls(&state), 2, "3 turn 目は起こされない");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "段は Failed");
    clean(&[&repo, &state]);
}

/// 上限に達して終端した便（`Failed detail=rebase-conflict`）は `pipe retire` で畳める
/// （move・元 dir 不在・retired/ に在る・**残す event の段は終端のまま**・設計 §5）。
#[test]
fn pipe_follow_retire_folds_an_exhausted_run_and_keeps_the_stage() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, moved) = exhausted_run(&repo, &state, &marker);
    let live = worktree_of(&repo, &id);
    assert!(live.exists(), "終端した便の worktree は残る（retire の入口の前提）");
    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rebase-conflict の便も畳める: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    assert!(!live.exists(), "元の場所が空く");
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("retired".to_owned()))),
        "残す event の段は終端のまま: {:?}",
        stages(&state, &id)
    );
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は 1 byte も動かない");
    clean(&[&repo, &state]);
}

/// `Gated` で verdict が FAIL の便（判定に届いた終端・`.132` の memo）も畳める。残す event の
/// 段は `Gated` のままで、`Landed` へ動かさない。
#[test]
fn pipe_follow_retire_folds_a_gated_fail_run_and_keeps_the_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = implemented(&repo, &state, &path);
    let lens = fake_lens(&marker, &lens_verdict("FAIL"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の gate は rc 1: {}", stderr_of(&gated));
    let live = worktree_of(&repo, &id);
    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "Gated(FAIL) の便も畳める: {}", stderr_of(&out));
    assert!(!live.exists(), "元の場所が空く");
    assert!(
        repo.join(".worktrees").join("scribe2").join("retired").join(&id).exists(),
        "retired/ に在る"
    );
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Gated), Some("retired".to_owned()))),
        "段は Gated のまま: {:?}",
        stages(&state, &id)
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "畳んだ後も段は Gated");
    clean(&[&repo, &state]);
}

/// `resume` は「最後の段が `Implemented` ∧ 最後の detail が `rebase-conflict:` ∧ runner が
/// 起きていない」周に**同じ起こし直し**を撃つ。`--runner` の無い周は rc 1 で events.jsonl が
/// byte 不変（衝突の記帳は land の時点で済んでいる）。detail が `rebase-conflict:` でない
/// `Implemented` が従来どおり gate へ行くことは
/// `pipe_resume_continues_from_implemented_in_new_process` が測る。
#[test]
fn pipe_follow_resume_needs_a_runner_and_continues_the_retry() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    // `--runner` を渡さない land は衝突を記帳して断る（起こし直せない）。
    let bare_land = land_extra(&repo, &state, &id, &[]);
    assert_eq!(bare_land.status.code(), Some(i32::from(RC_REFUSED)), "起こし直せない land は rc 1");
    assert!(stderr_of(&bare_land).contains("--runner が要る"), "理由: {}", stderr_of(&bare_land));
    assert_eq!(stub_calls(&state), 1, "起こし直していない");
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は残る");
    // `--runner` の無い resume は 1 byte も書かない。
    let before = events_bytes(&state);
    let bare = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(bare.status.code(), Some(i32::from(RC_REFUSED)), "--runner 無しの resume は rc 1");
    assert_eq!(events_bytes(&state), before, "events.jsonl は byte 不変");
    assert_eq!(stub_calls(&state), 1, "runner を起こさない");
    // `--runner` 付きの resume は land の衝突と同じ turn を撃つ。
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "resume の起こし直し: {}", stderr_of(&out));
    assert_eq!(stub_calls(&state), 2, "resume が実装役を起こした");
    assert!(
        stdout_of(&out).contains(&format!("run={id} rebase={base}..{moved}")),
        "resume の turn も base を進める: {}",
        stdout_of(&out)
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段は Implemented（次は gate）");
    clean(&[&repo, &state]);
}

/// 追随を解けずに**木を戻して質問 record で止まった** turn は `Questioned` である
/// （ADR-0019 §2.6: 判定は turn 開始時の tip 基準＝便が base から持つ commit を数えない）。
#[test]
fn pipe_follow_question_after_abort_stops_at_questioned() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, ABORT_AND_ASK);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "質問は rc 3: {}", stderr_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Questioned"), "段は Questioned");
    let worktree = worktree_of(&repo, &id);
    assert!(git(&worktree, &["status", "--porcelain"]).is_empty(), "木は clean");
    assert!(!mid_rebase(&repo, &id), "木は rebase の途中でない");
    assert_eq!(
        git(&worktree, &["rev-list", "--count", &format!("{base}..HEAD")]),
        "1",
        "便が base から持つ commit は在る（turn で増えていないだけ）"
    );
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(
        trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised),
        "質問の逐語が残る"
    );
    clean(&[&repo, &state]);
}

/// 負例: 起こし直しの turn で **commit を作ってから**質問 record を出した周は質問ではなく
/// 実装の失敗である（`Failed detail=runner-rc:76,commits:1`）。
#[test]
fn pipe_follow_commit_before_question_is_a_failure() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, COMMIT_THEN_ASK);
    let (id, _base, _moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "land はしない: {}", stdout_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("runner-rc:76,commits:1".to_owned()))),
        "turn で作った commit を数える: {:?}",
        stages(&state, &id)
    );
    assert!(
        !trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised),
        "質問は記帳しない"
    );
    clean(&[&repo, &state]);
}

// ───── 便の commit を数えるとき main に在る commit を除く（`s2-07l.546`・設計 §11・接頭辞 `pipe_follow_main_`） ─────

/// turn 2 の本文の頭: 節の 2 sha で `--onto` の rebase を解いて終え、解いた便の commit を 1 つ戻す
/// （HEAD が新しい main に一致する＝main の commit だけが HEAD に載り、便の commit は無い）。
const ABSORB_MAIN: &str = "if ! git rebase --onto \"$SHA\" \"$BASE\"; then\nprintf '// seed\\ny\\nx\\n' > src/lib.rs\ngit add src/lib.rs\nGIT_EDITOR=true git rebase --continue\nfi\ngit reset -q --hard HEAD~1";

/// 質問 record を出して rc 76 で終える尾。
const ASK: &str = "printf '%s\\n' '{\"question\":\"追随の衝突を解けない\",\"about\":\"write-set\"}'\nexit 76";

/// 便の worktree の HEAD が main に一致することを現物で測る（本文が main の commit を HEAD に載せた）。
fn assert_head_is_main(repo: &Path, id: &str, moved: &str) {
    assert_eq!(git(&worktree_of(repo, id), &["rev-parse", "HEAD"]), moved, "HEAD は新しい main に一致する");
    assert_eq!(git(repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
}

/// (a) 追随の rebase で main の commit を HEAD に載せ、**自分の commit は作らずに**質問 record で止まった
/// turn は `Questioned` である（main の commit は便の commit に数えない）。
#[test]
fn pipe_follow_main_absorbed_then_question_stops_at_questioned() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, &format!("{ABSORB_MAIN}\n{ASK}"));
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "質問は rc 3: {} {:?}", stderr_of(&out), stages(&state, &id));
    assert!(show_line(&repo, &state, &id).contains("stage=Questioned"), "段は Questioned: {:?}", stages(&state, &id));
    assert!(
        trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised),
        "質問の逐語が残る"
    );
    assert_head_is_main(&repo, &id, &moved);
    clean(&[&repo, &state]);
}

/// (b) 負例: 同じ木で**自分の commit を 1 本作ってから**質問 record を出した turn は従来どおり実装の失敗で、
/// detail は `runner-rc:76,commits:1`（除外は main の commit だけ＝質問を無条件に通さない）。
#[test]
fn pipe_follow_main_own_commit_then_question_is_a_failure() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let body = format!("{ABSORB_MAIN}\nprintf 'z\\n' >> src/lib.rs\ngit add -A\ngit commit -q -m extra\n{ASK}");
    let runner = stub_runner(&state, &body);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "land はしない: {}", stdout_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("runner-rc:76,commits:1".to_owned()))),
        "turn で作った commit だけを数える: {:?}",
        stages(&state, &id)
    );
    assert!(
        !trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised),
        "質問は記帳しない"
    );
    assert_eq!(
        git(&worktree_of(&repo, &id), &["rev-parse", "HEAD~1"]),
        moved,
        "自分の commit は main の真上に 1 本"
    );
    clean(&[&repo, &state]);
}

/// (c) 追随で main を取り込んだだけで rc 0 で終わった turn は `Failed detail=runner-rc:0,commits:0`
/// （完了の判定も base から数え、同じ除外を受ける）。
#[test]
fn pipe_follow_main_absorbed_only_with_rc_zero_is_not_implemented() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, &format!("{ABSORB_MAIN}\nexit 0"));
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "land はしない: {}", stdout_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("runner-rc:0,commits:0".to_owned()))),
        "main の commit は完了の数にも入らない: {:?}",
        stages(&state, &id)
    );
    assert_head_is_main(&repo, &id, &moved);
    clean(&[&repo, &state]);
}

/// (d) main を**読めない**周は除外なしの従来の数え方に落ちる: (a) と同じ木でも、数え手の main の読みだけを
/// 落とす偽 git の下では `Failed detail=runner-rc:76,commits:1`（読めなさを 0 に倒して質問へ通さない）。
#[test]
fn pipe_follow_main_unreadable_main_counts_without_exclusion() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, &format!("{ABSORB_MAIN}\n{ASK}"));
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let path = shim_path(&state, "main-bin", "case \"$*\" in *'rev-parse --verify -q refs/heads/main'*) exit 1;; esac");
    let out = bin_cmd()
        .args(["pipe", "land", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--runner", &runner])
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "land はしない: {}", stdout_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("runner-rc:76,commits:1".to_owned()))),
        "除外なしで数える: {:?}",
        stages(&state, &id)
    );
    assert_head_is_main(&repo, &id, &moved);
    clean(&[&repo, &state]);
}

/// runner が **rebase の途中で** turn を終えた周は `Failed detail=rebase-dirty` で終端する
/// （clean 前提を守る・fail-closed・設計 §3 手順 6）。
#[test]
fn pipe_follow_mid_rebase_turn_fails_dirty() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, LEAVE_MID_REBASE);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "rebase の途中は rc 1: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-dirty".to_owned()))),
        "終端の理由: {:?}",
        stages(&state, &id)
    );
    assert!(mid_rebase(&repo, &id), "木は rebase の途中のまま（器は触らない）");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    clean(&[&repo, &state]);
}

/// 回数は **replay の導出値**である（別の状態 file を持たない・C3）。衝突の `RunStage` を手で
/// 2 件積んだ便は、上限 2（埋め込みの値）の下で次の衝突が終端になり、置き場に新しい file は
/// 1 つも増えない。
#[test]
fn pipe_follow_counts_the_retries_from_the_event_log() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    for _ in 0..2 {
        record_conflict(&state, &id, &format!("{base}..{moved}"));
    }
    // 手で積んだ段は `Implemented` なので、land の前に gate を撃ち直す。
    fs::remove_file(&marker).ok();
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "撃ち直しの gate: {}", stderr_of(&gated));
    let before = dir_names(&state.join("pipe").join(&id));
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "上限に達した周は rc 1: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-conflict".to_owned()))),
        "終端の理由: {:?}",
        stages(&state, &id)
    );
    assert_eq!(stub_calls(&state), 1, "起こし直さない（回数は log が持つ）");
    assert_eq!(dir_names(&state.join("pipe").join(&id)), before, "置き場に新しい file を作らない");
    clean(&[&repo, &state]);
}

/// 回数を**読めない**周は起こし直さず rc 2 で止まる（上限到達の rc 1 と分ける・NFR4）。読めなさは、rebase の
/// 呼出しに合わせて event log へ壊れた行を混ぜる偽 git で作る。
///
/// 段を進める記帳は記帳の門（設計 pipeline.md §39・行 ag）を通り、門は読めない log に書かない（fail-closed）ので、
/// `Failed detail=follow-unmeasured` は書かれず、最後の行は壊れた行のままである。
#[test]
fn pipe_follow_unreadable_retry_count_fails_closed_with_rc_two() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let events = state.join("fleet").join("events.jsonl");
    let path = shim_path(
        &state,
        "poison-bin",
        &format!("case \"$*\" in *' rebase '*) printf 'not-json\\n' >> '{}' ;; esac", events.display()),
    );
    let out = bin_cmd()
        .args(["pipe", "land", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--runner", &runner])
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない回数は rc 2: {}", stderr_of(&out));
    assert_eq!(stub_calls(&state), 1, "起こし直さない");
    let log = fs::read_to_string(&events).expect("event log");
    let last = log.lines().rfind(|line| !line.is_empty()).unwrap_or_default();
    assert_eq!(last, "not-json", "読めない log に段を書かない（Failed も上限到達も書かれない）: {log}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    clean(&[&repo, &state]);
}

/// 起こし直しの spawn も **Budget を要る口だけ**を通る（`pipe_spawn_measures_repo_before_launching`
/// と同型の観測）。repo の HEAD を読めない git を前に置くと、起こし直しは Precheck の段で断られ、
/// runner は起こされない。
#[test]
fn pipe_follow_retry_measures_the_repo_before_launching() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE);
    let (id, _base, _moved) = conflicting_run(&repo, &state, &marker, &runner);
    let failing = format!("-C {} rev-parse HEAD", repo.display());
    let path = shim_path(&state, "measure-bin", &format!("case \"$*\" in *'{failing}'*) exit 1;; esac"));
    let out = bin_cmd()
        .args(["pipe", "land", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--runner", &runner])
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "測れない repo は rc 1: {}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("git repo でない"),
        "測る段で断る（起動関数へ入る前）: {}",
        stderr_of(&out)
    );
    assert_eq!(stub_calls(&state), 1, "起こし直しの runner は起きない");
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は残る（resume で続けられる）");
    clean(&[&repo, &state]);
}

// ───── 追随節の無い turn で runner が自ら rebase した周（`s2-07l.213`・設計 §3 手順 5・接頭辞 `pipe_follow_self_rebase_`） ─────

/// turn 1 の本文（**追随節なし**）: main を**別 file の 1 commit**で進め、頼まれていない
/// `git rebase` を自分で撃ってから便の commit を作る（.203 run 20260913T122002Z の実測の型）。
fn self_rebase_body(repo: &Path) -> String {
    format!(
        "printf 'o\\n' > '{repo}/other.txt'\ngit -C '{repo}' add other.txt\ngit -C '{repo}' commit -q -m other\n\
         git rebase refs/heads/main\n{IMPLEMENT}",
        repo = repo.display()
    )
}

/// turn 1 の本文（追随節なし）: main を別 file の 1 commit で進めるが、**rebase はしない**。
fn no_rebase_body(repo: &Path) -> String {
    format!(
        "printf 'o\\n' > '{repo}/other.txt'\ngit -C '{repo}' add other.txt\ngit -C '{repo}' commit -q -m other\n{IMPLEMENT}",
        repo = repo.display()
    )
}

/// turn 1 の本文（追随節なし）: 便の commit を作った後で main を**同じ行の隣**へ進め、
/// 自分で撃った `git rebase` の途中のまま turn を終える。
fn mid_rebase_body(repo: &Path) -> String {
    format!(
        "printf 'x\\n' >> src/lib.rs\ngit add -A\ngit commit -q -m runner\n\
         printf 'y\\n' >> '{repo}/src/lib.rs'\ngit -C '{repo}' add src/lib.rs\ngit -C '{repo}' commit -q -m other\n\
         git rebase refs/heads/main || true\nexit 0",
        repo = repo.display()
    )
}

/// 便の base を進めた記帳（`Implemented detail=rebase:<old>..<new>`）の detail の列。
fn rebase_details(state: &Path, id: &str) -> Vec<String> {
    stages(state, id)
        .into_iter()
        .filter(|(stage, _)| *stage == Some(Stage::Implemented))
        .filter_map(|(_, detail)| detail)
        .filter(|detail| detail.starts_with("rebase:"))
        .collect()
}

/// **追随節の無い turn**（spawn 時 main = base）で runner が自ら `git rebase` を撃った周も、
/// 器は turn の終了後に merge-base を実測して base を進める（設計 §3 手順 5・節の有無で経路を
/// 分けない）。記帳は `rebase:<base>..<new main>` の 1 件・`base_of_run` は new main・続きの
/// gate は PASS（main 側の `other.txt` を write-set の外れと数えない）。
#[test]
fn pipe_follow_self_rebase_advances_the_base_without_a_follow_section() {
    let (repo, state) = repo_with_state();
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let runner = stub_runner_turns(&state, &self_rebase_body(&repo), KEEP_CONFLICT);
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let spawned = spawn_with(&repo, &state, &id, &runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "turn 1 の spawn: {}", stderr_of(&spawned));
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(moved, base, "turn の中で main が進んだ");
    assert!(!stub_stdin(&state, 1).contains("## 追随"), "spawn 時は main = base ゆえ節は無い: {}", stub_stdin(&state, 1));
    assert_eq!(stub_calls(&state), 1, "turn は 1 回");
    assert_eq!(
        rebase_details(&state, &id),
        vec![format!("rebase:{base}..{moved}")],
        "器が base を進めた記帳は 1 件: {:?}",
        stages(&state, &id)
    );
    assert!(
        stdout_of(&spawned).contains(&format!("run={id} rebase={base}..{moved}")),
        "新しい base を名乗る: {}",
        stdout_of(&spawned)
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段は Implemented（次は gate）");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(
        gated.status.code(),
        Some(i32::from(RC_OK)),
        "新しい base の gate は PASS（main 側の other.txt を外れと数えない）: {}",
        stderr_of(&gated)
    );
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 1, "cmd"), "write-set", "段①");
    assert_eq!(row_value(&rows, 1, "rc"), "0", "段①（write-set）が緑");
    assert_eq!(
        row_value(&rows, 2, "cmd"),
        format!("git rev-parse --verify {moved}"),
        "gate が読む base（`base_of_run`）は new main"
    );
    clean(&[&repo, &state]);
}

/// 負例: 追随節なしで main が進んでも、runner が rebase しなければ **`rebase:` の記帳は無く**
/// `Implemented` の detail は空のまま（merge-base は記録済みの base と同じ＝進んでいない）。
#[test]
fn pipe_follow_self_rebase_records_nothing_when_the_runner_does_not_rebase() {
    let (repo, state) = repo_with_state();
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let runner = stub_runner_turns(&state, &no_rebase_body(&repo), KEEP_CONFLICT);
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let spawned = spawn_with(&repo, &state, &id, &runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "turn 1 の spawn: {}", stderr_of(&spawned));
    assert_ne!(git(&repo, &["rev-parse", "refs/heads/main"]), base, "turn の中で main が進んだ");
    assert!(rebase_details(&state, &id).is_empty(), "base は進めない: {:?}", stages(&state, &id));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Implemented), None)),
        "Implemented の detail は空のまま: {:?}",
        stages(&state, &id)
    );
    assert!(!stdout_of(&spawned).contains("rebase="), "base を名乗らない: {}", stdout_of(&spawned));
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "元の base の gate は PASS: {}", stderr_of(&gated));
    assert_eq!(
        row_value(&verify_rows(&state, &id), 2, "cmd"),
        format!("git rev-parse --verify {base}"),
        "gate が読む base は記録済みのまま"
    );
    clean(&[&repo, &state]);
}

/// 追随節の無い turn でも、木が rebase の途中のまま終わった周は `Failed detail=rebase-dirty`
/// で終端する（`mid_rebase` の検査が節の有無に依らない・fail-closed・設計 §3 手順 6）。
#[test]
fn pipe_follow_self_rebase_mid_rebase_turn_fails_dirty_without_a_follow_section() {
    let (repo, state) = repo_with_state();
    let runner = stub_runner_turns(&state, &mid_rebase_body(&repo), KEEP_CONFLICT);
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let spawned = spawn_with(&repo, &state, &id, &runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_REFUSED)), "rebase の途中は rc 1: {}", stderr_of(&spawned));
    assert!(!stub_stdin(&state, 1).contains("## 追随"), "節は無い: {}", stub_stdin(&state, 1));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-dirty".to_owned()))),
        "終端の理由: {:?}",
        stages(&state, &id)
    );
    assert!(mid_rebase(&repo, &id), "木は rebase の途中のまま（器は触らない）");
    assert!(rebase_details(&state, &id).is_empty(), "途中の木では base を進めない: {:?}", stages(&state, &id));
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "段は Failed");
    clean(&[&repo, &state]);
}

// ───── 検出線の面（設計 pipeline.md §30 / §33・`s2-07l.397` / `s2-07l.416`・FR46 / FR14 / FR34） ─────
//
// 追随の側。面の内が動いた周・diff を読めない周が従来どおり全段を撃ち直すのは接頭辞 `pipe_detection_scope_`、
// 面の外だけが動いた周が再 gate を丸ごと省いて前周の判定を引き継ぐのは接頭辞 `pipe_follow_docs_only_` の歯。
// 主実測の側（same-tree / outside-scope / 読めない周）は `gate.rs` の `pipe_detection_scope_` の歯が持つ。

/// 検出線を持つ便を Gated PASS まで通し、呼出行の母集団と base を返す（[`super::gate::detection_repo`] の型）。
fn detection_gated() -> (PathBuf, PathBuf, String, String, usize) {
    let (repo, state, design) = super::gate::detection_repo(super::gate::DETECTION_COUNT);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let before = super::gate::detection_calls(&repo).len();
    (repo, state, id, base, before)
}

/// main を **1 file だけ**の commit で進める（`git add <path>`＝契約 file や他の untracked を混ぜない）。
/// 返すのは進んだ main の sha。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn advance_main_with(repo: &Path, path: &str) -> String {
    let file = repo.join(path);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).expect("別便の dir を作れる");
    }
    fs::write(&file, "moved\n").expect("別便の変更を書ける");
    git(repo, &["add", path]);
    git(repo, &["commit", "-q", "-m", "other"]);
    let moved = git(repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(
        git(repo, &["diff", "--name-only", &format!("{moved}^..{moved}")]),
        path,
        "fixture: main が進んだ差分はその 1 path だけ"
    );
    moved
}

/// (a) docs だけで main が動いた便の追随（設計 §33）: **再 gate を丸ごと撃たず**（lens も起こさず）
/// 前周の Gated PASS を新しい base へ引き継いで着地する。撃つのは主実測の ②④ だけ（③ は木の差が
/// docs だけ＝面の外なので従来どおり省く）で、`verify.jsonl` に足されるのは引き継ぎの 1 本だけ。
///
/// base は再 gate を撃つので、呼出が ②④ の 2 行 + record 3 本ぶん多く、偽 lens の marker も立つ＝RED。
#[test]
fn pipe_follow_docs_only_carries_gated_pass_without_regate() {
    let (repo, state, id, base, before) = detection_gated();
    let moved = advance_main_with(&repo, "docs/design/toy.md");
    // 1 度目の gate が立てた marker を外す＝「land の中で lens が起きたか」だけを効果で測る。
    let marker = state.join("lens-ran");
    fs::remove_file(&marker).expect("1 度目の gate の marker を消せる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("rebase={base}..{moved}")), "追随は済む: {stdout}");
    assert!(stdout.contains("verdict=PASS"), "引き継いだ判定行: {stdout}");
    assert!(!marker.exists(), "再 gate を撃たない＝lens は 1 度も起きない: {stdout}");
    // 着地後の検出の子（設計 gate-cost.md §44 形 (11)）は着地した diff が面の外（toy の `src/lib.rs`）＝stub を撃たず
    // `landed` 付きの skip record 1 本だけを足す。子の終わりを待ってから測る。
    super::gate::await_detection_child(&state, &id);
    let added = super::gate::detection_calls(&repo).split_off(before);
    assert_eq!(added, ["common", "contract"], "撃つのは主実測の ②④ だけ（母集団 = 前 {before} 行）");
    assert_regate_skip_record(&verify_rows(&state, &id));
    // 主実測は別 file（`verify-main.jsonl`）に従来どおり ②・③ の skip・④ の 3 本。木は gate を撃った周と
    // 違う（docs の 1 file ぶん進んでいる）ので、③ を省く理由は木の一致ではなく面の外である。
    let (main, after) = super::gate::split_landed(super::gate::main_rows(&state, &id));
    assert_eq!(after.len(), 1, "着地後の record は 1 本: {after:?}");
    assert_eq!(
        super::gate::kinds(&main),
        ["write-set", "common", "detection", "contract"],
        "主実測は ①②④ を撃ち ③ の位置に record を置く: {main:?}"
    );
    assert_eq!(row_value(&main, 3, "skipped"), "detection", "主実測も ③ を省く: {main:?}");
    assert_eq!(row_value(&main, 3, "reason"), "outside-scope", "省いた理由は面の外");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed: {}", show_line(&repo, &state, &id));
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS", "引き継いだ verdict");
    assert_follow_events(&state, &base, &moved, &git(&repo, &["rev-parse", "refs/heads/main"]));
    clean(&[&repo, &state]);
}

/// (a) の `verify.jsonl`: 1 度目の gate の 3 本（①②④・gate は ③ を撃たない＝設計 gate-cost.md §44 形 (9)）の後ろに、
/// **引き継ぎの 1 本**（`kind=gate` / `skipped=regate` / `reason=outside-scope`・`n` は通し・木は持たない）だけが足される。
fn assert_regate_skip_record(rows: &[Vec<(String, vessel::fleet::json_lite::Value)>]) {
    assert_eq!(
        super::gate::kinds(rows),
        ["write-set", "common", "contract", "gate"],
        "1 度目の gate の 3 本 + 引き継ぎの 1 本（撃ち直しの 3 本は無い）: {rows:?}"
    );
    let skips = super::gate::skip_rows(rows);
    assert_eq!(skips.len(), 1, "skip record は引き継ぎの 1 本だけ（1 度目の gate は撃っている）: {rows:?}");
    let skip = skips.first().copied().cloned().unwrap_or_default();
    assert_eq!(value_of(&skip, "kind"), "gate", "省いたのは段ではなく gate 1 周");
    assert_eq!(value_of(&skip, "skipped"), "regate");
    assert_eq!(value_of(&skip, "reason"), "outside-scope", "理由は面の外");
    assert!(skip.iter().all(|(key, _)| key != "tree"), "引き継ぎの record は木を持たない: {skip:?}");
    assert_eq!(row_value(rows, 4, "n"), "4", "`n` は 1 度目の gate の 3 本からの通し");
    assert_eq!(row_value(rows, 3, "rc"), "0", "1 度目の ④ は撃って緑（引き継ぐ根）");
}

/// 対: docs だけで main が動いても `<base>..<main>` の diff を**読めない**周は面の外として省かない（fail-closed・
/// 設計 §30）＝再 gate を撃つ（前周の record の持ち越しは無い・設計 gate-cost.md §44 形 (8)・gate は ③ を撃たない＝
/// 形 (9)）。面の外の skip record は無い。偽 git はその range の `diff --name-only -z` だけを rc 1 で落とす（段①の
/// `<base>..HEAD` は落とさない）。
#[test]
fn pipe_detection_scope_unreadable_follow_diff_does_not_skip_outside_scope() {
    let (repo, state, id, base, before) = detection_gated();
    let moved = advance_main_with(&repo, "docs/design/toy.md");
    let out = land_once_with_git_shim(&repo, &state, &id, &format!(" diff --name-only -z {base}..{moved}"), None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("rebase={base}..{moved}")), "追随は済む: {}", stdout_of(&out));
    // 着地後の検出の子は面の外の着地で stub を撃たない（子の終わりを待ってから呼出を数える）。
    super::gate::await_detection_child(&state, &id);
    let added = super::gate::detection_calls(&repo).split_off(before);
    assert_eq!(added, ["common".to_owned(), "contract".to_owned()], "再 gate は ②④ を撃つ・主実測は撃たない: {added:?}");
    let rows = verify_rows(&state, &id);
    assert_eq!(rows.len(), 6, "1 度目 3 本 + 再 gate 3 本（引き継ぎの 1 本ではない）: {rows:?}");
    assert!(super::gate::skip_rows(&rows).is_empty(), "面の外の skip record は無い: {rows:?}");
    assert_eq!(row_value(&rows, 6, "kind"), "contract", "再 gate の ④ は撃った record: {rows:?}");
    assert_eq!(row_value(&rows, 4, "rc"), "0", "再 gate の段①（`<base>..HEAD`）は読めている: {rows:?}");
    clean(&[&repo, &state]);
}

/// 終端の道具一式（設計 contract-source.md §5・`s2-07l.382` の歯）。
struct FakeTerminal {
    /// 偽 remote（bare repo・push の着き先）。
    remote: PathBuf,
    /// 偽 bd が argv を書き出す file（撃たれなければ在らない）。
    bd_log: PathBuf,
    /// 偽 CI が argv を書き出す file（`{sha}` の穴に何が入ったかをここで測る）。
    ci_log: PathBuf,
}

/// 実行権つきの `/bin/sh` script を書き、その path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn exec_script(path: &Path, body: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, format!("#!/bin/sh\n{body}")).expect("script を書ける");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("script に実行権を付ける");
    path.display().to_string()
}

/// 偽 remote（bare repo）・偽 CI（`conclusion` を返す 1 行）・偽 bd を用意し、宣言に `remote` と
/// `ci-cmd` を足して commit する。**`.vessel.toml` は HEAD の tree が読み面**なので commit まで行う。
fn fake_terminal(repo: &Path, state: &Path, conclusion: &str) -> FakeTerminal {
    let body = format!("[{{\"status\":\"completed\",\"conclusion\":\"{conclusion}\"}}]");
    fake_terminal_json(repo, state, &body)
}

/// [`fake_terminal`] の一般形（偽 CI が返す JSON を呼び手が選ぶ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_terminal_json(repo: &Path, state: &Path, json: &str) -> FakeTerminal {
    let remote = state.join("remote.git");
    git(state, &["init", "--bare", "-q", &remote.display().to_string()]);
    git(repo, &["remote", "add", "fake", &remote.display().to_string()]);
    // 偽 CI: **渡された argv を log へ写してから** JSON 1 行を返す（`{sha}` の穴に何が入ったかを測る）。
    let ci_log = state.join("ci-argv.txt");
    let ci = exec_script(
        &state.join("fake-ci.sh"),
        &format!("printf '%s\\n' \"$@\" > '{}'\ncat <<'JSON'\n{json}\nJSON\n", ci_log.display()),
    );
    // 偽 bd: argv をそのまま log へ書いて rc 0（書きは close の 1 種だけ）。
    let bd_log = state.join("bd-argv.txt");
    exec_script(&state.join("fake-bd.sh"), &format!("printf '%s\\n' \"$@\" > '{}'\n", bd_log.display()));
    let body = fs::read_to_string(repo.join(".vessel.toml")).expect("宣言を読める");
    let added = format!("{body}remote = \"fake\"\nci-cmd = \"{ci} {{sha}}\"\n");
    fs::write(repo.join(".vessel.toml"), added).expect("宣言を書ける");
    git(repo, &["add", "-f", ".vessel.toml"]);
    git(repo, &["commit", "-q", "-m", "terminal-decl"]);
    FakeTerminal { remote, bd_log, ci_log }
}

/// 便の `RunDone stage=Landed` の detail を**宣言順に**並べる（終端は段ごとに 1 件記す）。
fn landed_details(state: &Path, id: &str) -> Vec<String> {
    trail(state, id)
        .into_iter()
        .filter(|(kind, stage, _)| *kind == EventKind::RunDone && *stage == Some(Stage::Landed))
        .filter_map(|(_, _, detail)| detail)
        .collect()
}

/// (§5 land の終端) 偽 remote + 偽 CI（success）+ 偽 adapter で、`Landed` の後ろに **push → CI → close の
/// 3 event**が並び、bead が閉じられる。押した先の main は着地した sha を指す。
#[test]
fn pipe_terminal_land_pushes_checks_ci_and_closes_the_bead() {
    let (repo, state) = repo_with_state();
    let tools = fake_terminal(&repo, &state, "success");
    let marker = state.join("lens-ran");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &marker);
    let bd = state.join("fake-bd.sh").display().to_string();
    // **上限は fixture の manifest から渡す**（埋め込みの 900 s を待たない）: CI を測れなくする変異は
    // ここで速やかに落ちる側に倒れる＝timeout でなく撃墜として数えられる。
    let rules = ceiling_rules(&state);
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "終端まで通った land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("terminal=closed"), "終端の token: {}", stdout_of(&out));
    let landed = git(&repo, &["rev-parse", "refs/heads/main"]);
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec!["terminal:push:fake".to_owned(), "terminal:ci:success".to_owned(), "terminal:close:ok".to_owned()],
        "Landed の後ろに段ごとの 3 件（母集団 {} 件）: {details:?}",
        details.len()
    );
    // **押した先が動いている**（数えただけでは撃ったと言えない）。
    assert_eq!(git(&tools.remote, &["rev-parse", "refs/heads/main"]), landed, "偽 remote の main は着地した sha");
    // **`{sha}` の穴が埋まっている**: CI の行は着地した **40 桁の** sha を名指して撃たれる（短縮 sha だと
    // forge の CLI は完了済みの run でも空を返し続ける）。
    let ci_argv = fs::read_to_string(&tools.ci_log).expect("偽 CI が撃たれた");
    let words: Vec<&str> = ci_argv.lines().collect();
    assert!(words.contains(&landed.as_str()), "argv に着地した sha が入る: {words:?}");
    assert_eq!(landed.len(), 40, "穴に入るのは 40 桁の sha: {landed}");
    assert!(!ci_argv.contains("{sha}"), "穴の字面が残らない: {ci_argv}");
    // **台帳は close の 1 種だけで撃たれる**（起票も acceptance も撃たない）。
    let argv = fs::read_to_string(&tools.bd_log).expect("偽 bd が撃たれた");
    let words: Vec<&str> = argv.lines().collect();
    assert_eq!(words.first().copied(), Some("close"), "subcommand は close: {words:?}");
    assert_eq!(words.get(2).copied(), Some("--reason"), "理由を渡す: {words:?}");
    assert!(words.get(3).is_some_and(|line| line.contains(&landed) && line.ends_with("ci=success")), "理由の中身: {words:?}");
    clean(&[&repo, &state]);
}

/// (設計 pipeline.md 行 ap) 運転手の cwd が**消えた dir**でも、台帳の close は repo を cwd にして撃たれ
/// `terminal:close:ok` まで進む。
///
/// 偽 bd は本物と同じく**台帳を cwd から探す**（`.vessel.toml` が cwd に無ければ rc 1 で断る）うえで cwd を
/// 記録に残す。運転手の cwd を継ぐ実装では、消えた dir から撃たれて `close:failed` で止まる。
#[test]
fn pipe_terminal_land_close_cwd_survives_a_vanished_driver_cwd() {
    let (repo, state) = repo_with_state();
    let _tools = fake_terminal(&repo, &state, "success");
    let cwd_log = state.join("bd-cwd.txt");
    let bd = exec_script(
        &state.join("fake-bd-cwd.sh"),
        &format!(
            "pwd -P > '{}' 2>&1\ntest -e .vessel.toml || {{ echo 'no ledger found from cwd' >&2; exit 1; }}\n",
            cwd_log.display()
        ),
    );
    let marker = state.join("lens-ran");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &marker);
    let rules = ceiling_rules(&state);
    let gone = state.join("vanishing-cwd");
    fs::create_dir_all(&gone).expect("消える dir を作れる");
    let (repo_arg, state_arg) = (repo.display().to_string(), state.display().to_string());
    let args = [
        "land", "--run", id.as_str(), "--repo", repo_arg.as_str(), "--state-dir", state_arg.as_str(),
        "--bd", bd.as_str(), "--rules", rules.as_str(),
    ];
    let mut cmd = pipe_cmd(&args);
    // **後置の cwd が勝つ**（[`bin_cmd`] の tmp dir を上書き）。起こした直後に dir を消す＝子の cwd は消えた dir。
    let child = cmd.current_dir(&gone).spawn().expect("binary を起動できる");
    fs::remove_dir(&gone).expect("子の cwd を消せる");
    assert!(!gone.exists(), "fixture: 子の cwd は消えている");
    let out = child.wait_with_output().expect("子の終わりを待てる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "終端まで通った land は rc 0: {}", stderr_of(&out));
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec!["terminal:push:fake".to_owned(), "terminal:ci:success".to_owned(), "terminal:close:ok".to_owned()],
        "消えた cwd からでも close まで進む（母集団 {} 件）: {details:?}",
        details.len()
    );
    let written = fs::read_to_string(&cwd_log).expect("偽 bd が撃たれた");
    assert_eq!(written.trim_end(), repo.display().to_string(), "偽 bd の cwd は repo");
    clean(&[&repo, &state]);
}

/// (§5 land の終端) CI が **failure** の周は**台帳を閉じない**（rc 1・記録は `ci:failure` で終わる）。
///
/// 着地そのものは取り消さない（main は進んだまま）——止めるのは close であって着地ではない。
#[test]
fn pipe_terminal_land_ci_failure_does_not_close_the_bead() {
    let (repo, state) = repo_with_state();
    let tools = fake_terminal(&repo, &state, "failure");
    let marker = state.join("lens-ran");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &marker);
    let before = git(&repo, &["rev-parse", "refs/heads/main"]);
    let bd = state.join("fake-bd.sh").display().to_string();
    let rules = ceiling_rules(&state);
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert_eq!(out.status.code(), Some(1), "close しなかった周は rc 1: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("terminal=ci:failure"), "終端の token: {}", stdout_of(&out));
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec!["terminal:push:fake".to_owned(), "terminal:ci:failure".to_owned()],
        "close の段は記さない（母集団 {} 件）: {details:?}",
        details.len()
    );
    assert!(!tools.bd_log.exists(), "台帳 client は 1 度も撃たれない");
    // 着地は取り消さない（main は進んだまま・押した先も動いている）。
    assert_ne!(git(&repo, &["rev-parse", "refs/heads/main"]), before, "main は進んだまま");
    clean(&[&repo, &state]);
}

/// (§5 手順 3) `pipe land --terminal-only` は**着地をやり直さず終端だけ**を撃ち直す（冪等）。
///
/// CI が確定しなかった便（`ci:failure`）を、CI を直してから継ぐ。main は 1 mm も動かない——
/// 着地は既に成立していて、やり直すのは終端の 3 段だけである。
#[test]
fn pipe_terminal_land_only_replays_the_terminal_without_relanding() {
    let (repo, state) = repo_with_state();
    let tools = fake_terminal(&repo, &state, "failure");
    let marker = state.join("lens-ran");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &marker);
    let bd = state.join("fake-bd.sh").display().to_string();
    let rules = ceiling_rules(&state);
    let first = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert_eq!(first.status.code(), Some(1), "1 周目は close しない: {}", stderr_of(&first));
    let landed = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert!(!tools.bd_log.exists(), "前提: 台帳はまだ閉じていない");
    // **別の便が main を進める**（この歯の要）: 以後 HEAD ≠ 着地した sha なので、終端が「記録の sha」を
    // 読むのか「HEAD の今の sha」を読むのかが弁別できる。同じ fixture で両方が等しいままだと、HEAD を
    // 読む実装でも通ってしまう（空虚）。
    fs::write(repo.join("unrelated.md"), "別の便
").expect("別の便の file を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "another-run"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(moved, landed, "前提: HEAD は着地した sha から動いた");
    // CI を直す（宣言は同じ path を指したまま・行は 1 byte も変えない）。
    exec_script(&state.join("fake-ci.sh"), &format!("printf '%s\\n' \"$@\" > '{}'\nprintf '[{{\"status\":\"completed\",\"conclusion\":\"success\"}}]\\n'\n", tools.ci_log.display()));
    let again = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules, "--terminal-only"]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "継いだ終端は rc 0: {}", stderr_of(&again));
    assert_eq!(stdout_of(&again).trim(), format!("run={id} terminal=closed"), "終端だけの 1 行");
    // **着地はやり直さない**: main は別の便が進めた位置のままで、器は 1 mm も動かさない。
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は器が動かさない");
    // **照合したのは記録の sha である**（HEAD の今の sha ではない）。
    let ci_argv = fs::read_to_string(&tools.ci_log).expect("偽 CI が撃たれた");
    let words: Vec<&str> = ci_argv.lines().collect();
    assert!(words.contains(&landed.as_str()), "CI の argv は**着地した sha**: {words:?}");
    assert!(!words.contains(&moved.as_str()), "HEAD の今の sha では照合しない: {words:?}");
    // 記録は 1 周目の 2 件に 2 周目の 3 件が続く（段ごとに 1 件・やり直した段も残る）。
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec![
            "terminal:push:fake".to_owned(),
            "terminal:ci:failure".to_owned(),
            "terminal:push:fake".to_owned(),
            "terminal:ci:success".to_owned(),
            "terminal:close:ok".to_owned(),
        ],
        "母集団 {} 件: {details:?}",
        details.len()
    );
    let argv = fs::read_to_string(&tools.bd_log).expect("2 周目で台帳が閉じられた");
    assert!(argv.contains(&landed), "理由は**1 周目に着地した sha**を名指す: {argv}");
    assert!(!argv.contains(&moved), "HEAD の今の sha は理由に載らない: {argv}");
    clean(&[&repo, &state]);
}

/// (§5 手順 4) record の `generation` は **binary の build 元 commit**（`--version` の括弧の中身と同じ 1 本）で、
/// 同じ行の `sha`（着地した commit）とは**別の値**である。
///
/// 同値の欄を 2 つ並べると、読み手はどちらを版の比較（§12）に使うのか判じられない（C10）。
#[test]
fn pipe_terminal_land_generation_is_the_binary_build_commit_not_the_landed_sha() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &marker);
    let out = land_extra(&repo, &state, &id, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let pairs = exported_pairs(&state, &id);
    let landed = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(value_of(&pairs, "sha"), landed, "sha は着地した commit: {pairs:?}");
    // `--version` の括弧の中身を現物から採る（器の字面を借りずに外形から測る）。
    let version = String::from_utf8_lossy(&bin_cmd().arg("--version").output().expect("--version").stdout)
        .trim()
        .to_owned();
    let generation = version
        .rsplit_once('(')
        .and_then(|(_, tail)| tail.strip_suffix(')'))
        .unwrap_or_default()
        .to_owned();
    assert!(!generation.is_empty(), "--version の括弧の中身を読める: {version}");
    assert_eq!(value_of(&pairs, "generation"), generation, "generation は build 元 commit: {pairs:?}");
    assert_ne!(value_of(&pairs, "generation"), value_of(&pairs, "sha"), "同値の欄を 2 つ並べない: {pairs:?}");
    clean(&[&repo, &state]);
}

/// (§5 手順 2) **落ちた run が 1 本在れば、別の run が走っていても `ci:failure`** である。
///
/// 実 CI では複数の workflow が並ぶので「1 本が落ちた後も別の 1 本が走っている」が常態である。
/// 未完了を先に見る実装は、**測って落ちた事実**を上限いっぱい待った末の `ci:unmeasurable` に化けさせる
/// （C10 の反転）。落ちたと分かった時点で待つ理由は無い。
#[test]
fn pipe_terminal_land_ci_failure_wins_over_a_still_running_workflow() {
    let (repo, state) = repo_with_state();
    let json = "[{\"status\":\"completed\",\"conclusion\":\"failure\"},{\"status\":\"in_progress\",\"conclusion\":null}]";
    let tools = fake_terminal_json(&repo, &state, json);
    let marker = state.join("lens-ran");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &marker);
    let bd = state.join("fake-bd.sh").display().to_string();
    // **上限は fixture の manifest から渡す**（埋め込みの 900 s を待たない）。落ちた run を先に見ない
    // 実装はここで上限まで待ってから `ci:unmeasurable` を名乗る＝この歯はその差で落ちる。
    let rules = ceiling_rules(&state);
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert_eq!(out.status.code(), Some(1), "close しなかった周は rc 1: {}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains("terminal=ci:failure"),
        "走っている run が同居しても **failure** を名乗る（unmeasurable に化けない）: {}",
        stdout_of(&out)
    );
    assert!(!tools.bd_log.exists(), "台帳 client は 1 度も撃たれない");
    // 負例の対: 同じ形で落ちた run を外すと（走っている run だけ）測れない側へ倒れる。
    // **上限は fixture の manifest から渡す**（埋め込みの 900 s を待たない＝測れない周だけが待つ側である）。
    clean(&[&repo, &state]);
    let (repo, state) = repo_with_state();
    let running = "[{\"status\":\"in_progress\",\"conclusion\":null}]";
    let tools = fake_terminal_json(&repo, &state, running);
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let bd = state.join("fake-bd.sh").display().to_string();
    let rules = ceiling_rules(&state);
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert!(
        stdout_of(&out).contains("terminal=ci:unmeasurable"),
        "走っている run だけの周は測れない側: {}",
        stdout_of(&out)
    );
    assert!(!tools.bd_log.exists(), "測れない周も台帳は閉じない");
    clean(&[&repo, &state]);
}

/// (pipeline.md §46) 同じ sha で cron の run（`event=schedule`）が走っていても、**それを待たずに**
/// push の run の success で終端が close まで進む。
///
/// cron の run を数える実装は、走っている schedule の run の完了を上限いっぱい待って `ci:unmeasurable` に倒れる。
#[test]
fn pipe_terminal_land_ci_ignores_scheduled_runs() {
    let (repo, state) = repo_with_state();
    let json = "[{\"status\":\"completed\",\"conclusion\":\"success\",\"event\":\"push\"},{\"status\":\"in_progress\",\"conclusion\":null,\"event\":\"schedule\"}]";
    let tools = fake_terminal_json(&repo, &state, json);
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let bd = state.join("fake-bd.sh").display().to_string();
    // **上限は fixture の manifest から渡す**（埋め込みの 900 s を待たない）。
    let rules = ceiling_rules(&state);
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "close まで通った land は rc 0: {}", stderr_of(&out));
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec!["terminal:push:fake".to_owned(), "terminal:ci:success".to_owned(), "terminal:close:ok".to_owned()],
        "schedule の run を待たない（母集団 {} 件）: {details:?}",
        details.len()
    );
    assert!(tools.bd_log.exists(), "bead が閉じられた");
    clean(&[&repo, &state]);
}

/// (pipeline.md §46) run が cron（`event=schedule`）だけの周は、外した後に 0 本＝**測れない**。
///
/// 外した後の空を success に倒すと、着地した commit の CI を 1 本も見ずに bead を閉じる（C10 の反転）。
#[test]
fn pipe_terminal_land_ci_only_scheduled_runs_is_unmeasurable() {
    let (repo, state) = repo_with_state();
    let json = "[{\"status\":\"completed\",\"conclusion\":\"success\",\"event\":\"schedule\"}]";
    let tools = fake_terminal_json(&repo, &state, json);
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let bd = state.join("fake-bd.sh").display().to_string();
    let rules = ceiling_rules(&state);
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert_eq!(out.status.code(), Some(1), "close しなかった周は rc 1: {}", stderr_of(&out));
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec!["terminal:push:fake".to_owned(), "terminal:ci:unmeasurable".to_owned()],
        "schedule の run だけでは測れない（母集団 {} 件）: {details:?}",
        details.len()
    );
    assert!(!tools.bd_log.exists(), "測れない周は台帳を閉じない");
    clean(&[&repo, &state]);
}

/// `pipe land-window` を 1 回撃つ（上限は既定の 0 秒＝1 周だけ観測する）。
fn land_window_once(repo: &Path, state: &Path) -> Output {
    run_pipe(&["land-window", "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string()])
}

/// 着地列の窓（設計 pipeline.md §19）: 列が空で origin の無い周は rc 0 の `clear remote=none`・origin が local main を
/// 含む周は rc 0 の `clear`。窓の口は merge も fetch も撃たない（main と origin の ref は動かない）。
#[test]
fn pipe_land_window_clear_when_the_queue_is_empty_and_main_is_pushed() {
    let (repo, state) = repo_with_state();
    let out = land_window_once(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "開いた窓は rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim(), "land-window=clear remote=none", "origin の無い周");
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", &main]);
    let out = land_window_once(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "push 済みも rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim(), "land-window=clear", "origin が local main を含む周");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main, "main は動かない");
    assert_eq!(git(&repo, &["rev-parse", "refs/remotes/origin/main"]), main, "origin の ref は動かない");
    clean(&[&repo, &state]);
}

/// 列に PASS の便が居る周は rc 1 の `busy` で列の便を名指す（`unpushed=-`＝(a) で閉じた）。上限を渡しても前の便が
/// 居るまま待ちが切れれば rc 1。
#[test]
fn pipe_land_window_busy_names_the_queued_runs() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let out = land_window_once(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "閉じた窓は rc 1: {}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out).trim(),
        format!("land-window=busy queue={id_a},{id_b} following=- unpushed=- remote=none"),
        "列の便を名指す"
    );
    let waited = run_pipe(&[
        "land-window", "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(), "--wait-s", "1",
    ]);
    assert_eq!(waited.status.code(), Some(i32::from(RC_REFUSED)), "待ちが切れても rc 1: {}", stderr_of(&waited));
    clean(&[&repo, &state]);
}

/// local main に未 push の commit が在る周は rc 1 の `busy` で `unpushed=<local main の sha>`（(c) で閉じた）・local main を
/// 読めない周は `unpushed=unreadable`。
#[test]
fn pipe_land_window_busy_names_the_unpushed_main() {
    let (repo, state) = repo_with_state();
    let pushed = git(&repo, &["rev-parse", "refs/heads/main"]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", &pushed]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "squash"]);
    let local = git(&repo, &["rev-parse", "refs/heads/main"]);
    let out = land_window_once(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "未 push は rc 1: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim(), format!("land-window=busy queue=- following=- unpushed={local}"));
    git(&repo, &["branch", "-q", "-m", "main", "trunk"]);
    let out = land_window_once(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "local main を読めない周も rc 1: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim(), "land-window=busy queue=- following=- unpushed=unreadable");
    clean(&[&repo, &state]);
}

// ───── 追随で入った契約表の行が便の消した path を名指す周（設計 pipeline.md §34・`s2-07l.400`・接頭辞 `pipe_follow_stale_rows_`） ─────

/// main が docs の commit で足す行の置き場（便の契約の doc `toy.md` とは別＝追記した項目がどの doc かを弁別できる）。
const STALE_DOC: &str = "docs/design/other.md";

/// 便の turn 1 の本文: 契約の実装に加えて `src/gone.rs` を消す（便自身の diff の `D`）。
const IMPLEMENT_AND_DELETE: &str = "git rm -q src/gone.rs\nprintf 'x\\n' >> src/lib.rs\ngit add -A\ngit commit -q -m runner\nexit 0";

/// turn 2 の本文: 名指された行を write-set の中（追記された設計 doc）で直して commit する。
const FIX_ROW: &str = "sed -i 's#\"src/gone.rs\"#\"src/lib.rs\"#' docs/design/other.md\ngit add -A\ngit commit -q -m fix-row\nexit 0";

/// `src/gone.rs` を消す便を PASS の gate まで通し、main を「`named` を write-set に持つ行 z」を [`STALE_DOC`] に足す
/// docs だけの commit で進める。便の行 a は消す file を `~`（着地で消える）で名指す＝便自身の行は解ける。
/// 返すのは 便の id・便の base・動いた main・偽 runner の cmd（turn 2 以降は `second`）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn stale_rows_run(repo: &Path, state: &Path, marker: &Path, second: &str, named: &str) -> (String, String, String, String) {
    fs::write(repo.join("src").join("gone.rs"), "// gone\n").expect("消す file を書ける");
    let own = row_fields("a", &["write-set"], &[r#"write-set = ["src/lib.rs", "~src/gone.rs"]"#]);
    write_design(repo, &design_doc(&own));
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "base-with-gone"]);
    let base = git(repo, &["rev-parse", "refs/heads/main"]);
    let runner = stub_runner_turns(state, IMPLEMENT_AND_DELETE, second);
    let id = intake(repo, state, &design_pointer());
    let spawned = spawn_with(repo, state, &id, &runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "turn 1 の spawn: {}", stderr_of(&spawned));
    let lens = fake_lens(marker, &lens_verdict("PASS"));
    let gated = gate_once(repo, state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate: {}", stderr_of(&gated));
    let other = row_fields("z", &["write-set"], &[&format!("write-set = [\"{named}\"]")]);
    fs::write(repo.join(STALE_DOC), design_doc_rows(&[other])).expect("別の設計 doc を書ける");
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "docs-row"]);
    let moved = git(repo, &["rev-parse", "refs/heads/main"]);
    fs::remove_file(marker).expect("1 度目の gate の marker を外せる");
    (id, base, moved, runner)
}

/// 契約表の行の起こし直しの記帳（`Implemented detail=rebase-stale-rows:<range>`）の件数。
fn stale_rows_count(state: &Path, id: &str) -> usize {
    stages(state, id)
        .into_iter()
        .filter(|(stage, detail)| {
            *stage == Some(Stage::Implemented)
                && detail.as_deref().is_some_and(|found| found.starts_with("rebase-stale-rows:"))
        })
        .count()
}

/// 契約表の行の起こし直しの記帳は 1 件で `<range>` を名乗り、便は終端しない（`Failed` 0 件）。
fn assert_stale_rows_record(state: &Path, id: &str, range: &str) {
    let trail = stages(state, id);
    assert!(
        trail.contains(&(Some(Stage::Implemented), Some(format!("rebase-stale-rows:{range}")))),
        "記帳は base と main を名乗る: {trail:?}"
    );
    assert_eq!(stale_rows_count(state, id), 1, "記帳は 1 件: {trail:?}");
    assert!(!trail.iter().any(|(stage, _)| *stage == Some(Stage::Failed)), "終端しない: {trail:?}");
}

/// 写し（run dir の `contract.toml`）の `write-set` の行（無ければ空）。
fn write_set_line(state: &Path, id: &str) -> String {
    fs::read_to_string(contract_path(state, id))
        .unwrap_or_default()
        .lines()
        .find(|line| line.starts_with("write-set"))
        .map(str::to_owned)
        .unwrap_or_default()
}

/// (a) 便が消した path を名指す行が追随で入った周は `Implemented detail=rebase-stale-rows:<base>..<main>` を記帳して
/// runner を 1 回起こし直し、写しの write-set の**末尾に 1 項目**だけ行の設計 doc を足す（既存の項目は字面も順序も不変）。
/// 再 gate は撃たない（偽 lens の marker 0）。起こし直しの stdin の「追随」節は行を `<doc>#<id>: <項目>` で名指す。
/// base（従来どおり docs だけの追随＝Gated PASS を引き継いで着地・runner は 1 回）では記帳も起こし直しも無い＝RED。
#[test]
fn pipe_follow_stale_rows_restarts_the_runner_and_appends_the_design_doc() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, base, moved, runner) = stale_rows_run(&repo, &state, &marker, FIX_ROW, "src/gone.rs");
    let before = write_set_line(&state, &id);
    assert!(before.ends_with(']') && !before.contains(STALE_DOC), "fixture: 写しの write-set の行: {before}");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = land_extra(&repo, &state, &id, &["--runner", &runner, "--lens", &lens]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "起こし直した周は rc 3: {} / {}", stdout_of(&out), stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("run={id} next=gate")), "次に撃つ段: {stdout}");
    assert!(!stdout.contains("landed="), "land しない: {stdout}");
    assert_eq!(stub_calls(&state), 2, "runner を 1 回起こし直した");
    assert!(!marker.exists(), "再 gate は撃たれない（偽 lens の写し 0）");
    assert_stale_rows_record(&state, &id, &format!("{base}..{moved}"));
    let widened = format!("{}, \"{STALE_DOC}\"]", before.strip_suffix(']').unwrap_or_default());
    assert_eq!(write_set_line(&state, &id), widened, "末尾へ 1 項目だけ追記（既存の項目は不変）");
    assert!(stderr_of(&out).contains(STALE_DOC), "追記した項目を stderr に写す: {}", stderr_of(&out));
    let second = stub_stdin(&state, 2);
    assert!(second.contains("## 追随"), "起こし直しの turn に節が付く: {second}");
    assert!(second.contains(&format!("{STALE_DOC}#z: src/gone.rs")), "節は行と未解決の項目を名指す: {second}");
    assert!(stdout.contains(&format!("run={id} rebase={base}..{moved}")), "turn の後に base が進む: {stdout}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段は Implemented（次は gate）");
    clean(&[&repo, &state]);
}

/// (b) 行が便と無関係の path（便が消していない・base にも無い）を名指す周は起こし直さず、従来どおりの追随（docs だけ＝
/// Gated PASS を引き継ぐ）で着地する。記帳 0・runner は turn 1 の 1 回だけ・写しの write-set は不変。
#[test]
fn pipe_follow_stale_rows_unrelated_path_goes_on_as_before() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, base, moved, runner) = stale_rows_run(&repo, &state, &marker, FIX_ROW, "src/never.rs");
    let before = write_set_line(&state, &id);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = land_extra(&repo, &state, &id, &["--runner", &runner, "--lens", &lens]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "従来どおり着地: {} / {}", stdout_of(&out), stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("rebase={base}..{moved}")), "追随は済む: {stdout}");
    assert!(stdout.contains("landed="), "着地する: {stdout}");
    assert_eq!(stub_calls(&state), 1, "起こし直さない");
    assert_eq!(stale_rows_count(&state, &id), 0, "記帳しない: {:?}", stages(&state, &id));
    assert_eq!(write_set_line(&state, &id), before, "写しの write-set は不変");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// (c) 回数は衝突と**同じ 1 つの上限**（`pipe.follow_retries`＝fixture で 1）: 衝突の記帳を 1 件持つ便は、最初の
/// 契約表の行の周で上限に達して `Failed detail=rebase-stale-rows` + rc 1 で終端する（runner は起こさない・main は不動）。
#[test]
fn pipe_follow_stale_rows_exhausted_fails_typed_under_the_conflict_limit() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, base, moved, runner) = stale_rows_run(&repo, &state, &marker, FIX_ROW, "src/gone.rs");
    record_conflict(&state, &id, &format!("{base}..{moved}"));
    let rules = write_rules_with_retries(&state, "rules-stale-rows-1.toml", 1, 1_000_000, 1);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_with_rules(&repo, &state, &id, &rules, &lens);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "撃ち直しの gate: {}", stderr_of(&gated));
    fs::remove_file(&marker).ok();
    let out = land_extra(&repo, &state, &id, &["--runner", &runner, "--lens", &lens, "--rules", &rules.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "上限に達した周は rc 1: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stderr_of(&out).contains("上限"), "理由は上限を名乗る: {}", stderr_of(&out));
    let trail = stages(&state, &id);
    assert_eq!(
        trail.last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-stale-rows".to_owned()))),
        "終端の理由: {trail:?}"
    );
    assert_eq!(stale_rows_count(&state, &id), 1, "記帳は 1 件: {trail:?}");
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は手で積んだ 1 件のまま: {trail:?}");
    assert_eq!(stub_calls(&state), 1, "runner を起こさない");
    assert!(!marker.exists(), "再 gate は撃たれない");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "段は Failed");
    clean(&[&repo, &state]);
}

/// (d) `--runner` の無い land は記帳（と写しの追記）を残して rc 1 で止まり（`--runner が要る`・段は `Implemented`）、
/// `pipe resume --runner` が同じ起こし直しを続ける（衝突と同じ弁別の 1 本）。resume の turn も行の一覧を受ける。
#[test]
fn pipe_follow_stale_rows_no_runner_records_and_resume_continues() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, base, moved, runner) = stale_rows_run(&repo, &state, &marker, FIX_ROW, "src/gone.rs");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = land_extra(&repo, &state, &id, &["--lens", &lens]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "起こし直せない land は rc 1: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stderr_of(&out).contains("--runner が要る"), "理由: {}", stderr_of(&out));
    assert_eq!(stale_rows_count(&state, &id), 1, "記帳は残る: {:?}", stages(&state, &id));
    assert_eq!(stub_calls(&state), 1, "起こし直していない");
    assert!(write_set_line(&state, &id).ends_with(&format!(", \"{STALE_DOC}\"]")), "追記は残る: {}", write_set_line(&state, &id));
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段は Implemented（続けられる側）");
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "resume の起こし直し: {} / {}", stdout_of(&resumed), stderr_of(&resumed));
    assert_eq!(stub_calls(&state), 2, "resume が runner を起こした");
    assert!(
        stub_stdin(&state, 2).contains(&format!("{STALE_DOC}#z: src/gone.rs")),
        "resume の turn も行を名指す: {}",
        stub_stdin(&state, 2)
    );
    assert!(stdout_of(&resumed).contains(&format!("run={id} rebase={base}..{moved}")), "base が進む: {}", stdout_of(&resumed));
    assert!(!marker.exists(), "再 gate は撃たれない");
    clean(&[&repo, &state]);
}

// ───── 着地の列（merge train・設計 pipeline.md §40・契約表の行 ah・接頭辞 `pipe_train_`） ─────

/// [`write_rules_land_wait`]（待ちの上限 30 秒）に `land.train_max` の行を足した tmp manifest（`None` = 行を置かない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_rules_train(dir: &Path, name: &str, train_max: Option<u64>) -> String {
    let path = write_rules_land_wait(dir, name, Some(30));
    if let Some(value) = train_max {
        let text = fs::read_to_string(&path).expect("tmp manifest を読める");
        let block = format!(
            "\n[[rule]]\nid = \"land.train_max\"\nkind = \"LandTrainMax\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n"
        );
        fs::write(&path, format!("{text}{block}")).expect("tmp manifest を書ける");
    }
    path
}

/// 候補の木（`<worktrees>/train/<run>`）の中でだけ赤い契約 verify（gate の便の worktree と主実測の `verify` では緑）。
const TRAIN_RED: &str = "sh verify-train-red.sh";

/// 主実測の tmp（`<worktrees>/verify/<run>`）の中でだけ赤い契約 verify（gate と候補の木では緑）。
const MAIN_RED: &str = "sh verify-main-red.sh";

/// 契約 verify の既定（どこでも緑）。
const ALL_GREEN: &str = "sh verify-ok.sh";

/// 同じ base から 3 便を PASS の gate まで通す（行は **1 回の commit** で置く＝3 便の base は main の先端と同じ）。
/// write-set は `crates/toy/a.rs` / `src/b.rs` / `src/c.rs` で交わらず、Gated の ts と bead は a < b < c（列の順）。
/// `verify` は便ごとの契約 verify の 1 行（[`TRAIN_RED`] / [`MAIN_RED`] で赤い場所を選ぶ）。
fn train_runs(repo: &Path, state: &Path, marker: &Path, verify: [&str; 3]) -> [String; 3] {
    train_runs_on(repo, state, marker, verify, ["crates/toy/a.rs", "src/b.rs", "src/c.rs"])
}

/// [`train_runs`] の本体（便ごとの write-set の 1 file を `files` で選ぶ・交わらないこと）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn train_runs_on(repo: &Path, state: &Path, marker: &Path, verify: [&str; 3], files: [&str; 3]) -> [String; 3] {
    for (name, face) in [("verify-train-red.sh", "*/train/*"), ("verify-main-red.sh", "*/verify/*")] {
        let body = format!("case \"$(git rev-parse --show-toplevel)\" in {face}) exit 1;; esac\nexit 0\n");
        fs::write(repo.join(name), body).expect("verify script を書ける");
    }
    let rows: Vec<Vec<String>> = ["a", "b", "c"]
        .iter()
        .zip(files)
        .zip(verify)
        .map(|((row, file), line)| {
            row_fields(row, &["write-set", "verify"], &[&format!("write-set = [\"{file}\"]"), &format!("verify = [\"{line}\"]")])
        })
        .collect();
    commit_rows(repo, &rows);
    let mut ids = Vec::new();
    for ((row, bead), file) in ["a", "b", "c"].iter().zip(["s2-2e5", "s2-3ax", "s2-4cz"]).zip(files) {
        let id = intake_bead(repo, state, &format!("{DESIGN_FILE}#{row}"), bead);
        let runner = format!("mkdir -p crates/toy && echo {row} > {file} && git add -A && git commit -q -m runner");
        let spawned = spawn_with(repo, state, &id, &runner);
        assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "{row} の spawn: {}", stderr_of(&spawned));
        let gated = gate_once(repo, state, &id, Some(&fake_lens(marker, &lens_verdict("PASS"))));
        assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "{row} の gate: {}", stderr_of(&gated));
        ids.push(id);
    }
    ids.try_into().expect("3 便")
}

/// 便の `Landed` の detail の `sha:`（無ければ空）。
fn landed_sha_of(state: &Path, id: &str) -> String {
    landed_detail(state, id)
        .split_whitespace()
        .find_map(|word| word.strip_prefix("sha:"))
        .map(str::to_owned)
        .unwrap_or_default()
}

/// 面 5（`verdicts.jsonl`）の行数。
fn verdict_lines(state: &Path) -> usize {
    fs::read_to_string(land::verdicts_path(state)).map(|text| text.lines().filter(|line| !line.trim().is_empty()).count()).unwrap_or(0)
}

/// 便の `RunDone stage=Landed` の件数（着地そのものの行だけ・終端の `terminal:` は数えない）。
fn landed_count(state: &Path, id: &str) -> usize {
    trail(state, id)
        .into_iter()
        .filter(|(kind, stage, detail)| {
            *kind == EventKind::RunDone
                && *stage == Some(Stage::Landed)
                && detail.as_deref().is_some_and(|found| found.starts_with("sha:"))
        })
        .count()
}

/// 列の 3 本が列の順に載った形: 親の連鎖（base → a → b → c）・main の先端は c・仕事が載る・`Landed` は便ごとに 1 件・
/// 段は Landed・追随は 0 回・面 5 は 3 行で先頭は自分の番（`first`）・後続は `train`。返すのは main の先端。
fn assert_train_chain(repo: &Path, state: &Path, base: &str, ids: [&String; 3]) -> String {
    let [id_a, id_b, id_c] = ids;
    let (sha_a, sha_b, sha_c) = (landed_sha_of(state, id_a), landed_sha_of(state, id_b), landed_sha_of(state, id_c));
    assert_eq!(git(repo, &["rev-parse", &format!("{sha_a}^")]), base, "a は base の上");
    assert_eq!(git(repo, &["rev-parse", &format!("{sha_b}^")]), sha_a, "b は a の上");
    assert_eq!(git(repo, &["rev-parse", &format!("{sha_c}^")]), sha_b, "c は b の上");
    let main = git(repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(main, sha_c, "main の先端は c");
    assert_eq!(git(repo, &["show", &format!("{main}:src/c.rs")]), "c", "c の仕事が main に載る");
    assert_eq!(git(repo, &["show", &format!("{main}:crates/toy/a.rs")]), "a", "a の仕事も載る");
    for id in ids {
        assert_eq!(landed_count(state, id), 1, "Landed は便ごとに 1 件: {:?}", trail(state, id));
        assert!(show_line(repo, state, id).contains("stage=Landed"), "段は Landed: {id}");
        assert_eq!(follow_count(state, id), 0, "追随は 0 回: {:?}", stages(state, id));
    }
    assert_eq!(verdict_lines(state), 3, "面 5 は 3 行");
    let orders: Vec<String> = ids.iter().map(|id| exported_order(state, id)).collect();
    assert_eq!(orders, vec!["first", "train", "train"], "先頭は自分の番・後続は train");
    main
}

/// 候補の木の record: 先頭の `verify.jsonl` に共通 verify が `train=3` で 1 組、後続に足されたのは契約 verify だけで
/// `train=` を持たない（`before` は land の前の record 数）。
fn assert_train_records(state: &Path, ids: [&String; 3], before: &[usize]) {
    let head_rows = verify_rows(state, ids[0]);
    let trained: Vec<String> = head_rows.iter().filter(|row| value_of(row, "train") == "3").map(|row| value_of(row, "kind")).collect();
    assert_eq!(trained, vec!["common".to_owned()], "先頭に共通 verify が train=3 で 1 組: {head_rows:?}");
    for (id, seen) in ids.iter().zip(before).skip(1) {
        let rows = verify_rows(state, id);
        let added: Vec<String> = rows.iter().skip(*seen).map(|row| value_of(row, "kind")).collect();
        assert_eq!(added, vec!["contract".to_owned()], "後続に足されたのは契約 verify だけ: {rows:?}");
        assert!(rows.iter().all(|row| value_of(row, "train").is_empty()), "後続は train= を持たない: {rows:?}");
    }
}

/// (a) 上限 3 で先頭を land すると 3 本が**列の順に**着地する: 親の連鎖（base → a → b → c）・main の先端は c・`Landed`
/// 3 件・面 5 は 3 行で後続 2 本が `order=train`・後続の追随は 0 回。先頭の `verify.jsonl` に共通 verify が `train=3` で
/// 1 組、後続の `verify.jsonl` に足された record は契約 verify だけ（検出線は写しに無い）。着地済みの便の land は
/// rc 0 で main も event も動かさない。base（`train=` の無い経路）は a だけが載り b / c は Gated のまま＝RED。
#[test]
fn pipe_train_three_runs_land_in_queue_order_with_one_candidate_check() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let [id_a, id_b, id_c] = train_runs(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let before: Vec<usize> = [&id_a, &id_b, &id_c].iter().map(|id| verify_rows(&state, id).len()).collect();
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "列の land は rc 0: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={id_a} train=3")), "stdout に train=3: {}", stdout_of(&out));
    let main = assert_train_chain(&repo, &state, &base, [&id_a, &id_b, &id_c]);
    assert_train_records(&state, [&id_a, &id_b, &id_c], &before);
    let events = event_count(&state);
    let again = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "着地済みの便の land は rc 0: {}", stderr_of(&again));
    assert!(stdout_of(&again).contains("already-landed"), "already-landed を名乗る: {}", stdout_of(&again));
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main, "main は不変");
    assert_eq!(event_count(&state), events, "event は増えない");
    clean(&[&repo, &state]);
}

/// (a) 番待ちで待っている 2 本目の land（子 process・`pipe.land_wait_s` の窓）と並行に先頭が列で着地すると、2 本目は
/// 起きた後に rc 0 の `already-landed` で終端し event を 1 件も足さない（追随へ進まない）。
#[test]
fn pipe_train_waiting_second_run_wakes_already_landed() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let [id_a, id_b, _id_c] = train_runs(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN]);
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let mut waiting = land_in_background(&repo, &state, &id_b, &rules, &lens);
    std::thread::sleep(Duration::from_secs(2));
    assert!(waiting.try_wait().expect("子の状態を読める").is_none(), "2 本目は列の前が空くまで待っている");
    let first = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "先頭の land: {} / {}", stdout_of(&first), stderr_of(&first));
    assert!(stdout_of(&first).contains("train=3"), "列で着地した: {}", stdout_of(&first));
    let events = event_count(&state);
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    let out = waiting.wait_with_output().expect("待っていた land が終わる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "起きた 2 本目は rc 0: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains("already-landed"), "already-landed で終端: {}", stdout_of(&out));
    assert_eq!(event_count(&state), events, "event は増えない");
    assert_eq!(follow_count(&state, &id_b), 0, "追随しない: {:?}", stages(&state, &id_b));
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main, "main は動かない");
    clean(&[&repo, &state]);
}

/// (b) 3 本目の契約 verify が候補の木で赤なら列を解き、先頭だけが既存の経路で着地する。後続 2 本は Gated PASS の
/// まま列に残り、stdout に `dissolved`。
#[test]
fn pipe_train_red_contract_dissolves_and_only_the_front_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let [id_a, id_b, id_c] = train_runs(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, TRAIN_RED]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "先頭は着地する: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={id_a} train=3 dissolved")), "列を解いた: {}", stdout_of(&out));
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(landed_sha_of(&state, &id_a), main, "先頭だけが載る");
    assert_eq!(git(&repo, &["rev-list", "--count", &format!("{base}..{main}")]), "1", "main は 1 本だけ進む");
    for id in [&id_b, &id_c] {
        assert!(show_line(&repo, &state, id).contains("stage=Gated"), "後続は Gated のまま: {id}");
        assert_eq!(value_of(&verdict_pairs(&state, id), "verdict"), "PASS", "後続の判定は PASS のまま: {id}");
        assert!(worktree_of(&repo, id).is_dir(), "後続の worktree は列に残る: {id}");
        assert_eq!(landed_count(&state, id), 0, "後続は着地しない: {id}");
    }
    assert_eq!(verdict_lines(&state), 1, "面 5 は先頭の 1 行");
    clean(&[&repo, &state]);
}

/// (c) 2 本目が先頭と衝突する周（gate の後に 2 本目の branch が先頭と同じ file を足した形）は 2 本目を候補から外して
/// 1 本目と 3 本目が着地する。2 本目の worktree は clean で HEAD も動かず、event は 1 件も増えない。
#[test]
fn pipe_train_conflicting_second_is_left_out_and_the_rest_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let [id_a, id_b, id_c] = train_runs(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN]);
    let second = worktree_of(&repo, &id_b);
    fs::create_dir_all(second.join("crates").join("toy")).expect("衝突の dir を作れる");
    fs::write(second.join("crates").join("toy").join("a.rs"), "conflict\n").expect("衝突の file を書ける");
    git(&second, &["add", "-A"]);
    git(&second, &["commit", "-q", "-m", "conflict"]);
    let head_before = git(&second, &["rev-parse", "HEAD"]);
    let trail_before = trail(&state, &id_b).len();
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "列の land は rc 0: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={id_a} train=2")), "積んだのは 2 本: {}", stdout_of(&out));
    let (sha_a, sha_c) = (landed_sha_of(&state, &id_a), landed_sha_of(&state, &id_c));
    assert_eq!(git(&repo, &["rev-parse", &format!("{sha_c}^")]), sha_a, "c は a の上に詰めて載る");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), sha_c, "main の先端は c");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Gated"), "2 本目は Gated のまま");
    assert_eq!(trail(&state, &id_b).len(), trail_before, "2 本目の event は増えない");
    assert_eq!(git(&second, &["rev-parse", "HEAD"]), head_before, "2 本目の HEAD は動かない");
    assert!(git(&second, &["status", "--porcelain"]).is_empty(), "2 本目の worktree は clean");
    assert_eq!(exported_order(&state, &id_c), "train", "3 本目は train");
    clean(&[&repo, &state]);
}

/// (d) 上限 1 と行の不在は先頭だけが着地し（`train=` を出さない）、後続は自分の land で従来どおり追随 1 回。
#[test]
fn pipe_train_limit_one_and_absent_row_land_only_the_front() {
    for (name, limit) in [("limit-1", Some(1)), ("absent", None)] {
        let (repo, state) = repo_with_state();
        let marker = state.join("lens-ran");
        let [id_a, id_b, _id_c] = train_runs(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN]);
        let rules = write_rules_train(&state, "rules-train.toml", limit);
        let first = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
        assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "{name}: 先頭の land: {}", stderr_of(&first));
        assert!(!stdout_of(&first).contains("train="), "{name}: 列を積まない: {}", stdout_of(&first));
        assert!(show_line(&repo, &state, &id_b).contains("stage=Gated"), "{name}: 2 本目は Gated のまま");
        let lens = fake_lens(&marker, &lens_verdict("PASS"));
        let second = land_extra(&repo, &state, &id_b, &["--rules", &rules, "--lens", &lens]);
        assert_eq!(second.status.code(), Some(i32::from(RC_OK)), "{name}: 2 本目の land: {}", stderr_of(&second));
        assert_eq!(follow_count(&state, &id_b), 1, "{name}: 追随は 1 回: {:?}", stages(&state, &id_b));
        assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "{name}: 2 本目は Landed");
        clean(&[&repo, &state]);
    }
}

/// (g) 先端の木の主実測が赤の周は列の便すべてが `Failed detail=main-red` で `Landed` 0 件・main は 3 本ぶん進んだまま
/// （巻き戻さない・既存の極性・どの便が赤かは帰属しない）。
#[test]
fn pipe_train_red_main_fails_every_run_and_keeps_main_advanced() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let [id_a, id_b, id_c] = train_runs(&repo, &state, &marker, [MAIN_RED, ALL_GREEN, ALL_GREEN]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "main が赤い周は rc 1: {} / {}", stdout_of(&out), stderr_of(&out));
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-list", "--count", &format!("{base}..{main}")]), "3", "main は 3 本ぶん進んだまま");
    for id in [&id_a, &id_b, &id_c] {
        assert_eq!(
            stages(&state, id).last().cloned(),
            Some((Some(Stage::Failed), Some("main-red".to_owned()))),
            "列の便はすべて main-red: {id}"
        );
        assert_eq!(landed_count(&state, id), 0, "Landed は 0 件: {id}");
    }
    assert_eq!(verdict_lines(&state), 0, "面 5 へ書かない");
    clean(&[&repo, &state]);
}

/// 着地後の検出（設計 gate-cost.md §44 行 am・接頭辞 `pipe_detection_after_landing_`）の (o): 検出線を宣言した toy の 3 本
/// （write-set は `crates/` の面の内）を候補の木で着地させると、後続も自分の `finish` から口を起こす＝後続は自分の run dir の
/// `verify-main.jsonl` に `landed=<自分の squash>` の record を 1 本持ち、その `{base}` は自分の squash commit の親。base の
/// land は子を起こさない＝`landed` を持つ record が 0 本で RED。
#[test]
fn pipe_detection_after_landing_train_follower_records_in_its_own_run_dir() {
    let (repo, state) = repo_with_state();
    super::gate::commit_detection_vessel(&repo, super::gate::DETECTION_COUNT);
    let marker = state.join("lens-ran");
    let files = ["crates/toy/a.rs", "crates/toy/b.rs", "crates/toy/c.rs"];
    let [id_a, id_b, id_c] = train_runs_on(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN], files);
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "列の land は rc 0: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={id_a} train=3")), "列で着地した: {}", stdout_of(&out));
    for id in [&id_a, &id_b, &id_c] {
        super::gate::await_detection_child(&state, id);
    }
    let (sha_a, sha_b, sha_c) = (landed_sha_of(&state, &id_a), landed_sha_of(&state, &id_b), landed_sha_of(&state, &id_c));
    let calls = super::gate::detection_calls(&repo);
    for (id, sha, parent) in [(&id_b, &sha_b, &sha_a), (&id_c, &sha_c, &sha_b)] {
        assert_eq!(git(&repo, &["rev-parse", &format!("{sha}^")]), *parent, "fixture: 後続の squash の親は前の便の squash");
        let (_, after) = super::gate::split_landed(super::gate::main_rows(&state, id));
        assert_eq!(after.len(), 1, "後続 {id} の run dir に landed を持つ record は 1 本: {after:?}");
        let row = after.first().cloned().unwrap_or_default();
        assert_eq!(value_of(&row, "landed"), *sha, "landed=<後続自身の squash>: {row:?}");
        assert_eq!(value_of(&row, "cmd"), format!("sh verify-count.sh detection-{parent}"), "{{base}} は自分の squash の親: {row:?}");
        assert!(calls.contains(&format!("detection-{parent}")), "stub は自分の squash の親で呼ばれた: {calls:?}");
    }
    clean(&[&repo, &state]);
}

/// 着地が動かしうる面の写し（main の ref・偽 remote の ref・event log の bytes・worktree の一覧と便の木の HEAD / 状態）。
#[derive(Debug, PartialEq, Eq)]
struct LandFaces {
    main: String,
    remote: String,
    events: Vec<u8>,
    worktrees: String,
    head: String,
    status: String,
}

/// [`LandFaces`] を測る。
fn land_faces(repo: &Path, state: &Path, remote: &Path, id: &str) -> LandFaces {
    let worktree = worktree_of(repo, id);
    LandFaces {
        main: git(repo, &["rev-parse", "refs/heads/main"]),
        remote: git(remote, &["for-each-ref", "--format=%(refname) %(objectname)"]),
        events: fs::read(state.join("fleet").join("events.jsonl")).unwrap_or_default(),
        worktrees: git(repo, &["worktree", "list", "--porcelain"]),
        head: git(&worktree, &["rev-parse", "HEAD"]),
        status: git(&worktree, &["status", "--porcelain"]),
    }
}

/// 偽 remote の toy repo に Gated(PASS) の便を 1 本立て、land の引数（`--bd` / `--rules` 込み）と面の写しを返す。
fn land_args_fixture() -> (PathBuf, PathBuf, PathBuf, String, Vec<String>) {
    let (repo, state) = repo_with_state();
    let tools = fake_terminal(&repo, &state, "success");
    let marker = state.join("lens-ran");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &marker);
    let bd = state.join("fake-bd.sh").display().to_string();
    let rules = ceiling_rules(&state);
    let (repo_arg, state_arg) = (repo.display().to_string(), state.display().to_string());
    let args = ["land", "--run", id.as_str(), "--repo", repo_arg.as_str(), "--state-dir", state_arg.as_str(), "--bd", bd.as_str()]
        .into_iter()
        .chain(["--rules", rules.as_str()])
        .map(str::to_owned)
        .collect();
    (repo, state, tools.remote, id, args)
}

/// argv の後ろに `extra` を足して `pipe` を撃つ。
fn pipe_with(args: &[String], extra: &[&str]) -> Output {
    let mut all: Vec<&str> = args.iter().map(String::as_str).collect();
    all.extend_from_slice(extra);
    run_pipe(&all)
}

/// (6) 偽 remote の toy repo で `pipe land` に**未知の flag** を渡すと、main の ref・偽 remote・event log・worktree が 1 つも
/// 動かず rc 2 で断る（理由の 1 行が flag を名指し、usage を添える・stdout 0 byte）。flag を外した同じ argv は着地する
/// （断ったのが閉包の検査であって、材料の欠けではない）。
#[test]
fn pipe_land_args_unknown_flag_moves_nothing_and_refuses_with_rc_2() {
    let (repo, state, remote, id, args) = land_args_fixture();
    let before = land_faces(&repo, &state, &remote, &id);
    for extra in [&["--bogus"][..], &["--bogus", "x"], &["--no-such-flag", "--terminal-only"], &["-x"]] {
        let out = pipe_with(&args, extra);
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{extra:?}: rc 2: {}", stderr_of(&out));
        assert!(out.stdout.is_empty(), "{extra:?}: stdout 0 byte: {}", stdout_of(&out));
        let err = stderr_of(&out);
        let named = format!("pipe: 未知の引数 {}", extra.first().copied().unwrap_or_default());
        assert_eq!(err.lines().next(), Some(named.as_str()), "{extra:?}: 理由の 1 行: {err}");
        assert!(err.contains(&vessel::pipe::cli::usage()), "{extra:?}: usage を添える: {err}");
        assert_eq!(land_faces(&repo, &state, &remote, &id), before, "{extra:?}: 何も動かない");
    }
    let landed = pipe_with(&args, &[]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "対照: flag を外せば着地する: {}", stderr_of(&landed));
    assert_ne!(git(&repo, &["rev-parse", "refs/heads/main"]), before.main, "対照: main が進む");
    clean(&[&repo, &state]);
}

/// (7) 同じ toy repo で `pipe land --run <id> --help`（`-h` も）は usage を stdout に出して rc 0 で終わり、main の ref・偽 remote・
/// event log・worktree が 1 つも動かない（**2026-09-15 の回帰そのもの**＝base は squash して `Landed` まで走る）。
#[test]
fn pipe_land_args_help_prints_usage_with_rc_0_and_moves_nothing() {
    let (repo, state, remote, id, args) = land_args_fixture();
    let before = land_faces(&repo, &state, &remote, &id);
    for extra in [&["--help"][..], &["-h"], &["--bogus", "--help"]] {
        let out = pipe_with(&args, extra);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{extra:?}: rc 0: {}", stderr_of(&out));
        assert_eq!(stdout_of(&out), format!("{}\n", vessel::pipe::cli::usage()), "{extra:?}: usage だけを stdout へ");
        assert!(out.stderr.is_empty(), "{extra:?}: stderr 0 byte: {}", stderr_of(&out));
        assert_eq!(land_faces(&repo, &state, &remote, &id), before, "{extra:?}: 何も動かない");
    }
    let landed = pipe_with(&args, &[]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "対照: --help を外せば着地する: {}", stderr_of(&landed));
    assert_ne!(git(&repo, &["rev-parse", "refs/heads/main"]), before.main, "対照: main が進む");
    clean(&[&repo, &state]);
}

// ───── 主実測の record の `failed=` と診断 file（設計 pipeline.md §35 (3)・行 ac・`s2-07l.401`・接頭辞 `pipe_verify_failed_`） ─────

/// main-red の便（gate では緑・主実測で nextest 形の赤）の `verify-main.jsonl` に `failed=<最初の歯>` と区間が載り、
/// 同じ dir に同じ stem の `verify-main.stderr.log` が末尾 N 行の外の panic の本文を持つ。終端は従来の `main-red`。
#[test]
fn pipe_verify_failed_main_red_records_the_tooth_and_keeps_the_stderr_log() {
    let (repo, state) = repo_with_state();
    super::gate::write_nextest_red(&repo, true);
    let line = format!(r#"verify = ["{}"]"#, super::gate::NEXTEST_RED);
    let path = write_contract(&repo, &["verify"], &[&line]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let log = run_dir(&state, &id).join("verify-main.stderr.log");
    assert!(!log.exists(), "前提: gate の周（緑）は主実測の診断 file を作らない");
    make_tree_differ(&repo, &state, &id, "refs/heads/main");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "main が赤い land は rc 1: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("main-red".to_owned()))),
        "終端は従来の main-red: {:?}",
        stages(&state, &id)
    );
    let rows = super::gate::main_rows(&state, &id);
    let red = rows
        .iter()
        .find(|row| value_of(row, "cmd") == super::gate::NEXTEST_RED)
        .expect("主実測の nextest の行の record が在る");
    assert_eq!(value_of(red, "rc"), "100", "rc は従来どおり: {red:?}");
    assert_eq!(value_of(red, "failed"), super::gate::NEXTEST_FIRST, "failed= は最初の落ちた歯: {red:?}");
    assert!(
        value_of(red, "failed_stderr").contains(super::gate::NEXTEST_PANIC),
        "区間が verify-main.jsonl に載る: {red:?}"
    );
    let text = fs::read_to_string(&log).expect("verify-main.jsonl と同じ dir に verify-main.stderr.log が在る");
    assert!(text.contains(super::gate::NEXTEST_PANIC), "末尾 N 行の外の panic が残る: {text}");
    assert!(
        run_dir(&state, &id).join("verify-main.jsonl").exists(),
        "同じ dir に record が在る"
    );
    clean(&[&repo, &state]);
}
