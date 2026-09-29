// flip-check: moved s2-07l.264
// flip-check: moved s2-07l.295
// flip-check: retroactive s2-07l.417
//! land の歯: `pipe_land_` / `pipe_order_`（順番待ち）/ `pipe_follow_`（追随）/ `pipe_retire_`。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く（歯の本文は移しただけ・`s2-07l.264`）。
//!
//! 歯は族ごとの子 module にも置く（設計 docs/design/carry-prep.md §10 行 l・`s2-07l.684`）: `follow`（接頭辞 `pipe_follow_`）・
//! `retire`（接頭辞 `pipe_retire_` / `pipe_train_`）・`order`（接頭辞 `pipe_terminal_` / `pipe_order_`）・`rebase`（接頭辞
//! `pipe_land_rebase_` / `pipe_land_onto_`）。この file には共有の helper と const・他の族の歯だけを残す。

mod follow;
mod order;
mod rebase;
mod retire;
// flip-check: moved s2-07l.684

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
    // 末尾の終端の行（`terminal:close:ok`・remote を持たない toy の着地が台帳を閉じた 1 件）は除いて読む。
    let (kind, stage, detail) = trail(state, id)
        .into_iter()
        .rfind(|(_, _, detail)| !detail.as_deref().is_some_and(|found| found.starts_with("terminal:")))
        .expect("便の event が在る");
    assert_eq!((kind, stage), (EventKind::RunDone, Some(Stage::Landed)), "終端の行を除いた末尾は RunDone stage=Landed");
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

/// [`ceiling_rules`] と同じ値の tmp manifest で、終端の CI の 2 行（`pipe.ci_wait_s` / `pipe.ci_poll_s`）だけを
/// 差し替えた別の file（`ci_poll_s` が `None` = 間隔の行を落とす・設計 contract-source.md §50）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_rules_ci(state: &Path, ci_wait_s: u64, ci_poll_s: Option<u64>) -> String {
    let path = write_rules(state, "rules-ci.toml", 1, 1_000_000);
    let text = fs::read_to_string(&path).expect("tmp manifest を読める");
    let block = |id: &str, kind: &str, value: u64| {
        format!("[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n")
    };
    let (wait, poll) = (block("pipe.ci_wait_s", "PipeCiWaitS", CI_WAIT_S), block("pipe.ci_poll_s", "PipeCiPollS", CI_POLL_S));
    assert!(text.contains(&wait) && text.contains(&poll), "既定の 2 行が在る（差し替えが空振りしない）: {text}");
    let replaced = text
        .replace(&wait, &block("pipe.ci_wait_s", "PipeCiWaitS", ci_wait_s))
        .replace(&poll, &ci_poll_s.map(|value| block("pipe.ci_poll_s", "PipeCiPollS", value)).unwrap_or_default());
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

/// 便の `RunDone stage=Landed` の detail（無ければ空・着地後の検出の `detection:` は読み飛ばす＝設計 gate-cost.md §44 形 (4)・
/// 終端の `terminal:` も読み飛ばす）。
fn landed_detail(state: &Path, id: &str) -> String {
    trail(state, id)
        .into_iter()
        .rev()
        .filter(|(kind, stage, _)| *kind == EventKind::RunDone && *stage == Some(Stage::Landed))
        .filter_map(|(_, _, detail)| detail)
        .find(|detail| !detail.starts_with("detection:") && !detail.starts_with("terminal:"))
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

// ───── 揃えなかった周の印（設計 pipeline.md §57・行 az・接頭辞 `pipe_land_anchor_mark_` / `pipe_anchor_sync_`） ─────

/// anchor に置く局所の変更（便と別の tracked な file の中身）。
const LOCAL_EDIT: &str = "# local edit\n";

/// anchor の印の path（`<git-dir>/<NAME>/` の下・器の書き手と同じ 1 本で解く）。
fn mark_of(repo: &Path) -> PathBuf {
    land::mark_path(Path::new(&git(repo, &["rev-parse", "--absolute-git-dir"])))
}

/// 便と別の tracked な file（[`REQS_FILE`]）に局所の変更を置いた anchor へ `write-set` の便を 1 本着地させる
/// （`anchor=skipped:dirty`）。返すのは着地前の main・着地後の main・便 id。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn landed_on_dirty_anchor(repo: &Path, state: &Path, write_set: &str, runner: &str) -> (String, String, String) {
    let path = write_contract(repo, &["write-set"], &[write_set]);
    let id = intake(repo, state, &path);
    spawn_and_gate(repo, state, &id, runner);
    let old = git(repo, &["rev-parse", "refs/heads/main"]);
    fs::write(repo.join(REQS_FILE), LOCAL_EDIT).expect("局所の変更を置ける");
    let out = land_once(repo, state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains(" anchor=skipped:dirty"), "{}", stdout_of(&out));
    (old, git(repo, &["rev-parse", "refs/heads/main"]), id)
}

/// 便を `runner` で実装させ、PASS の gate まで通す（lens の marker は便ごと）。
fn spawn_and_gate(repo: &Path, state: &Path, id: &str, runner: &str) {
    let spawned = run_pipe(&[
        "spawn", "--run", id, "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(), "--runner", runner,
    ]);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&spawned));
    let lens = fake_lens(&state.join(format!("lens-ran-{id}")), &lens_verdict("PASS"));
    let gated = gate_once(repo, state, id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate は rc 0: {}", stderr_of(&gated));
}

/// 既定の便の runner（`src/lib.rs` に 1 行足す）。
const LIB_RUNNER: &str = "echo x >> src/lib.rs && git add -A && git commit -q -m runner";

/// 既定の便を汚れた anchor へ着地させる。
fn lib_landed_on_dirty_anchor(repo: &Path, state: &Path) -> (String, String, String) {
    landed_on_dirty_anchor(repo, state, r#"write-set = ["src/lib.rs"]"#, LIB_RUNNER)
}

/// `pipe anchor-sync --repo R` を 1 回撃つ。
fn anchor_sync_once(repo: &Path) -> Output {
    run_pipe(&["anchor-sync", "--repo", &repo.display().to_string()])
}

/// (a) 便と別の file に局所の変更を置いた anchor で land すると、`Landed` の detail が `anchor=skipped:dirty` で終わり、印が
/// 着地前の main の 1 行を持ち、`pipe land-window` が rc 1 の busy で `anchor=stale` を unpushed の欄の後ろに持つ。
#[test]
fn pipe_land_anchor_mark_dirty_landing_records_the_token_and_closes_the_window() {
    let (repo, state) = repo_with_state();
    let (old, _, id) = lib_landed_on_dirty_anchor(&repo, &state);
    let details = landed_details(&state, &id);
    let landed = details.iter().find(|detail| detail.starts_with("sha:")).expect("Landed の detail が在る");
    assert!(landed.ends_with(" anchor=skipped:dirty"), "detail の末尾に判定行と同じ token: {landed}");
    assert_eq!(fs::read_to_string(mark_of(&repo)).ok(), Some(format!("{old}\n")), "印は着地前の main の 1 行");
    let out = land_window_once(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "古い anchor は窓を閉じる: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim(), "land-window=busy queue=- following=- unpushed=- anchor=stale remote=none");
    clean(&[&repo, &state]);
}

/// (b) 同じ anchor で着地の path を手で `git read-tree -m -u <old> <new>` に揃えると、印が在るまま land-window は開き、
/// 行は `anchor=` を持たない（手で揃えた周は新しい）。
#[test]
fn pipe_land_anchor_mark_hand_synced_anchor_opens_the_window_with_the_mark_in_place() {
    let (repo, state) = repo_with_state();
    let (old, new, _) = lib_landed_on_dirty_anchor(&repo, &state);
    git(&repo, &["read-tree", "-m", "-u", &old, &new]);
    assert!(mark_of(&repo).exists(), "印は残っている");
    let out = land_window_once(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "手で揃えた anchor は窓を閉じない: {}", stdout_of(&out));
    assert_eq!(stdout_of(&out).trim(), "land-window=clear remote=none", "行は 1 字も変わらない");
    clean(&[&repo, &state]);
}

/// (c) 次の便が clean な anchor に synced で着地すると印が消え、その `Landed` の detail は anchor の token を持たない。
#[test]
fn pipe_land_anchor_mark_next_synced_landing_clears_the_mark_and_keeps_the_detail() {
    let (repo, state) = repo_with_state();
    let (old, new, _) = lib_landed_on_dirty_anchor(&repo, &state);
    git(&repo, &["read-tree", "-m", "-u", &old, &new]);
    git(&repo, &["checkout", "--", REQS_FILE]);
    assert_eq!(git(&repo, &["status", "--porcelain", "--untracked-files=no"]), "", "anchor は clean");
    assert!(mark_of(&repo).exists(), "1 本目の着地が残した印が在る（消えるのは 2 本目の synced の周）");
    // 2 本目は別の bead で起こす（run id は bead と秒で決まる＝同じ bead を同じ秒に起こすと断られる）。
    let id = intake_bead(&repo, &state, &design_pointer(), "s2-3ax");
    spawn_and_gate(&repo, &state, &id, LIB_RUNNER);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains(" anchor=synced"), "{}", stdout_of(&out));
    assert!(!mark_of(&repo).exists(), "synced の周は印を消す");
    let details = landed_details(&state, &id);
    let landed = details.iter().find(|detail| detail.starts_with("sha:")).expect("Landed の detail が在る");
    assert!(!landed.contains("anchor="), "synced の周の detail は token を持たない: {landed}");
    clean(&[&repo, &state]);
}

/// (d) (a) の anchor で `pipe anchor-sync` は rc 0 の `anchor-sync=synced paths=1` を返し、index と HEAD の差分が空になり、
/// 局所の変更は残り、印が消え、land-window が clear になる。
#[test]
fn pipe_anchor_sync_restores_landed_paths_and_keeps_the_local_edit() {
    let (repo, state) = repo_with_state();
    lib_landed_on_dirty_anchor(&repo, &state);
    let out = anchor_sync_once(&repo);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim(), "anchor-sync=synced paths=1");
    assert_eq!(git(&repo, &["diff", "--cached", "--name-only", "HEAD"]), "", "index は HEAD に揃う");
    assert_eq!(fs::read_to_string(repo.join(REQS_FILE)).ok().as_deref(), Some(LOCAL_EDIT), "局所の変更は残る");
    let lib = fs::read_to_string(repo.join("src").join("lib.rs")).unwrap_or_default();
    assert!(lib.lines().any(|line| line == "x"), "着地の変更が作業の木に在る: {lib:?}");
    assert!(!mark_of(&repo).exists(), "印を外す");
    let window = land_window_once(&repo, &state);
    assert_eq!(stdout_of(&window).trim(), "land-window=clear remote=none", "窓が開く");
    clean(&[&repo, &state]);
}

/// (e) 着地の path の作業の木を書き換えた anchor では rc 1 でその path を mixed に名指し、中身は変わらず印も残る。
#[test]
fn pipe_anchor_sync_refuses_a_landed_path_the_user_edited() {
    let (repo, state) = repo_with_state();
    lib_landed_on_dirty_anchor(&repo, &state);
    fs::write(repo.join("src").join("lib.rs"), "// user edit\n").expect("利用者の編集を置ける");
    let out = anchor_sync_once(&repo);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "rc 1: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim(), "anchor-sync=refused mixed=src/lib.rs");
    assert_eq!(fs::read_to_string(repo.join("src").join("lib.rs")).ok().as_deref(), Some("// user edit\n"), "編集は触らない");
    assert!(mark_of(&repo).exists(), "印は残す");
    clean(&[&repo, &state]);
}

/// (f) 足された path に untracked の file を置いた anchor では rc 1 でその path を名指し、その file の中身は変わらない
/// （戻せる path は戻す・印は残す）。
#[test]
fn pipe_anchor_sync_refuses_an_added_path_occupied_by_an_untracked_file() {
    let (repo, state) = repo_with_state();
    landed_on_dirty_anchor(
        &repo,
        &state,
        r#"write-set = ["src/lib.rs", "+src/new.rs"]"#,
        "echo n > src/new.rs && echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    );
    fs::write(repo.join("src").join("new.rs"), "// mine\n").expect("untracked の file を置ける");
    let out = anchor_sync_once(&repo);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "rc 1: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim(), "anchor-sync=refused mixed=src/new.rs");
    assert_eq!(fs::read_to_string(repo.join("src").join("new.rs")).ok().as_deref(), Some("// mine\n"), "file は触らない");
    assert_eq!(git(&repo, &["diff", "--cached", "--name-only", "HEAD", "--", "src/lib.rs"]), "", "戻せる path は戻す");
    assert!(mark_of(&repo).exists(), "印は残す");
    clean(&[&repo, &state]);
}

/// (g) 印の無い repo は rc 0 の `none`。古い path の無い印と、中身を壊した印でも tracked の変更が無い anchor は rc 0 の
/// `already` で印が消える。壊した印で tracked の変更が在れば rc 1 の `unreadable` で、land-window は `anchor=unreadable`。
#[test]
fn pipe_anchor_sync_none_already_and_unreadable_marks() {
    let (repo, state) = repo_with_state();
    let out = anchor_sync_once(&repo);
    assert_eq!((out.status.code(), stdout_of(&out).trim().to_owned()), (Some(i32::from(RC_OK)), "anchor-sync=none".to_owned()));
    let head = git(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(land::write_mark(&repo, &head, &"f".repeat(40)), Ok(land::Marked::Written), "境界 crate からも印を置ける");
    let out = anchor_sync_once(&repo);
    assert_eq!((out.status.code(), stdout_of(&out).trim().to_owned()), (Some(i32::from(RC_OK)), "anchor-sync=already".to_owned()));
    assert!(!mark_of(&repo).exists(), "古い path の無い印は消す");
    let mark = mark_of(&repo);
    fs::create_dir_all(mark.parent().expect("印の dir が在る")).expect("印の dir を作れる");
    fs::write(&mark, "garbage\n").expect("壊した印を置ける");
    let out = anchor_sync_once(&repo);
    assert_eq!((out.status.code(), stdout_of(&out).trim().to_owned()), (Some(i32::from(RC_OK)), "anchor-sync=already".to_owned()));
    assert!(!mark.exists(), "tracked の変更が無ければ壊した印も消す");
    fs::write(&mark, "garbage\n").expect("壊した印を置ける");
    fs::write(repo.join(REQS_FILE), LOCAL_EDIT).expect("局所の変更を置ける");
    let out = anchor_sync_once(&repo);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "rc 1: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim(), "anchor-sync=refused unreadable:mark");
    assert!(mark.exists(), "読めない印は残す");
    let window = land_window_once(&repo, &state);
    assert_eq!(window.status.code(), Some(i32::from(RC_REFUSED)), "読めない印は窓を閉じる（fail-closed）");
    assert_eq!(stdout_of(&window).trim(), "land-window=busy queue=- following=- unpushed=- anchor=unreadable remote=none");
    clean(&[&repo, &state]);
}

/// (h) `help pipe` の SUBCOMMANDS に anchor-sync の 1 行が在る。
#[test]
fn pipe_anchor_sync_is_listed_in_help_pipe() {
    let out = bin_cmd().args(["help", "pipe"]).output().expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "help は rc 0: {}", stderr_of(&out));
    let page = stdout_of(&out);
    assert!(page.lines().any(|line| line.trim_start().starts_with("anchor-sync ")), "SUBCOMMANDS に 1 行: {page}");
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

/// 終端の道具一式（設計 contract-source.md §5・`s2-07l.382` の歯）。
struct FakeTerminal {
    /// 偽 remote（bare repo・push の着き先）。
    remote: PathBuf,
    /// 偽 bd が argv を書き出す file（撃たれなければ在らない）。
    bd_log: PathBuf,
    /// 偽 CI が argv を書き出す file（`{sha}` の穴に何が入ったかをここで測る）。
    ci_log: PathBuf,
    /// 偽 CI が呼ばれるたびに 1 行を足す file（照合の回数をここで数える・設計 contract-source.md §50）。
    ci_calls: PathBuf,
}

impl FakeTerminal {
    /// 偽 CI が呼ばれた回数（撃たれなければ 0）。
    fn ci_call_count(&self) -> usize {
        fs::read_to_string(&self.ci_calls).map(|text| text.lines().count()).unwrap_or(0)
    }
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
fn fake_terminal_json(repo: &Path, state: &Path, json: &str) -> FakeTerminal {
    fake_terminal_decl(repo, state, json, true)
}

/// [`fake_terminal_json`] の本体。`declared` が偽の周は git の remote と偽 CI の宣言（`ci-cmd`）だけを足し、宣言に `remote` の行を
/// 書かない＝押す先を宣言していない repo（撃たれれば偽 remote の ref と偽 CI の呼び出しが動く形）を作る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_terminal_decl(repo: &Path, state: &Path, json: &str, declared: bool) -> FakeTerminal {
    let remote = state.join("remote.git");
    git(state, &["init", "--bare", "-q", &remote.display().to_string()]);
    git(repo, &["remote", "add", "fake", &remote.display().to_string()]);
    // 偽 CI: **渡された argv を log へ写し、呼ばれた回数の file に 1 行を足してから** JSON 1 行を返す
    // （`{sha}` の穴に何が入ったか・照合を何回撃ったかを測る）。
    let (ci_log, ci_calls) = (state.join("ci-argv.txt"), state.join("ci-calls.txt"));
    let ci = exec_script(
        &state.join("fake-ci.sh"),
        &format!(
            "printf '%s\\n' \"$@\" > '{}'\necho call >> '{}'\ncat <<'JSON'\n{json}\nJSON\n",
            ci_log.display(),
            ci_calls.display()
        ),
    );
    // 偽 bd: argv をそのまま log へ書いて rc 0（書きは close の 1 種だけ）。
    let bd_log = state.join("bd-argv.txt");
    exec_script(&state.join("fake-bd.sh"), &format!("printf '%s\\n' \"$@\" > '{}'\n", bd_log.display()));
    let body = fs::read_to_string(repo.join(".vessel.toml")).expect("宣言を読める");
    let remote_line = if declared { "remote = \"fake\"\n" } else { "" };
    let added = format!("{body}{remote_line}ci-cmd = \"{ci} {{sha}}\"\n");
    fs::write(repo.join(".vessel.toml"), added).expect("宣言を書ける");
    git(repo, &["add", "-f", ".vessel.toml"]);
    git(repo, &["commit", "-q", "-m", "terminal-decl"]);
    FakeTerminal { remote, bd_log, ci_log, ci_calls }
}

/// 便の `RunDone stage=Landed` の detail を**宣言順に**並べる（終端は段ごとに 1 件記す）。
fn landed_details(state: &Path, id: &str) -> Vec<String> {
    trail(state, id)
        .into_iter()
        .filter(|(kind, stage, _)| *kind == EventKind::RunDone && *stage == Some(Stage::Landed))
        .filter_map(|(_, _, detail)| detail)
        .collect()
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

/// 主実測と候補の木は検出線を撃たない（設計 gate-cost.md §44 形 (9)・契約表の行 ao・接頭辞 `pipe_detection_off_main_`）の
/// (p) の候補の木の側: 検出線を宣言した toy の 3 本（write-set は `crates/` の面の内）を候補の木で着地させると、候補の木は
/// 後続ごとの ③ の段を持たない＝どの便の `verify.jsonl` に足された record も `kind=detection` を持たず（先頭は共通 verify と
/// 契約 verify・後続は契約 verify だけ）、先端の木の主実測（先頭の `verify-main.jsonl` の `landed` の無い分）も ①②④ の
/// 3 本。stub を呼ぶのは着地後の検出の子（便ごとに 1 回・`landed` 付き）だけ。base は後続ごとに ③ を撃ち主実測も ③ を
/// 撃つ＝RED。
#[test]
fn pipe_detection_off_main_train_followers_record_no_detection() {
    let (repo, state) = repo_with_state();
    super::gate::commit_detection_vessel(&repo, super::gate::DETECTION_COUNT);
    let marker = state.join("lens-ran");
    let files = ["crates/toy/a.rs", "crates/toy/b.rs", "crates/toy/c.rs"];
    let ids = train_runs_on(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN], files);
    let before: Vec<usize> = ids.iter().map(|id| verify_rows(&state, id).len()).collect();
    let calls_before = super::gate::detection_calls(&repo).len();
    let rules = write_rules_train(&state, "rules-train.toml", Some(3));
    let out = land_extra(&repo, &state, &ids[0], &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "列の land は rc 0: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={} train=3", ids[0])), "列で着地した: {}", stdout_of(&out));
    for id in &ids {
        super::gate::await_detection_child(&state, id);
    }
    for (index, (id, seen)) in ids.iter().zip(&before).enumerate() {
        let rows = verify_rows(&state, id);
        let added: Vec<String> = rows.iter().skip(*seen).map(|row| value_of(row, "kind")).collect();
        let want: &[&str] = if index == 0 { &["common", "contract"] } else { &["contract"] };
        assert_eq!(added, want, "候補の木が {id} に足した record に ③ は無い: {rows:?}");
        let (main, after) = super::gate::split_landed(super::gate::main_rows(&state, id));
        assert_eq!(after.len(), 1, "{id} の着地後の record は 1 本: {after:?}");
        assert!(main.iter().all(|row| value_of(row, "kind") != "detection"), "{id}: landed の無い kind=detection は 0 本: {main:?}");
    }
    let (head_main, _) = super::gate::split_landed(super::gate::main_rows(&state, &ids[0]));
    assert_eq!(super::gate::kinds(&head_main), ["write-set", "common", "contract"], "先端の木の主実測は ①②④: {head_main:?}");
    let added_calls = super::gate::detection_calls(&repo).split_off(calls_before);
    let marks = added_calls.iter().filter(|call| call.starts_with("detection-")).count();
    assert_eq!(marks, 3, "stub を呼ぶのは着地後の検出の子の 3 回だけ: {added_calls:?}");
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

// ───── remote を持たない repo の終端（設計 contract-source.md §5・FR50・ADR-0094 の経路 (2)・接頭辞 `pipe_terminal_no_remote_`） ─────
//
// 宣言に `remote` の行が無い repo の便は push も CI の照合も撃たず、台帳の close を `landed <sha> ci=none` で撃つ（`terminal=closed:no-ci`・rc 0）。
// fixture は **git の remote と `ci-cmd` は在るが宣言に `remote` を書かない** toy（撃たれれば偽 remote の ref と偽 CI の回数が動く）。
// 台帳 client は既定名（道具箱の見張りが PATH で受ける）＝見張りの記録が close の呼び出しの母集団である。

/// 常に success の偽 CI と偽 remote を持つが宣言に `remote` を書かない toy（[`fake_terminal_decl`]）。
fn fake_no_remote(repo: &Path, state: &Path) -> FakeTerminal {
    fake_terminal_decl(repo, state, r#"[{"status":"completed","conclusion":"success"}]"#, false)
}

/// 台帳 client の見張りが写した呼び出し（1 起動 1 件・1 行 1 引数）を記録の名の昇順で返す。
fn ledger_calls(state: &Path) -> Vec<Vec<String>> {
    crate::toolbox_ledger_record_names(state)
        .iter()
        .map(|name| {
            fs::read_to_string(crate::toolbox_ledger_records(state).join(name))
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .collect()
}

/// 偽 remote に ref が 1 本も無い（push が撃たれていない）。
fn remote_refs(tools: &FakeTerminal) -> String {
    git(&tools.remote, &["for-each-ref"])
}

/// 便の `Landed` の後ろに並ぶ終端の行（`terminal:`）。
fn terminal_lines(state: &Path, id: &str) -> Vec<String> {
    landed_details(state, id).into_iter().filter(|detail| detail.starts_with("terminal:")).collect()
}

/// (a) 単独の着地: rc 0・`terminal=closed:no-ci`・見張りの記録はちょうど 1 件で 1 語目が `close`・理由は `landed <着地した sha> ci=none`
/// （`tip=` を持たない）。偽 CI は 0 回・偽 remote に ref は無く、`Landed` の着地の後ろは `terminal:close:ok` の 1 件だけ。
/// base は remote の無い周に close を撃たない＝記録 0 件で RED。
#[test]
fn pipe_terminal_no_remote_land_closes_with_ci_none() {
    let (repo, state) = repo_with_state();
    let tools = fake_no_remote(&repo, &state);
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "close まで通った land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("terminal=closed:no-ci"), "終端の token: {}", stdout_of(&out));
    let landed = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(landed_sha_of(&state, &id), landed, "fixture: 着地した sha は main の先端");
    let calls = ledger_calls(&state);
    assert_eq!(calls.len(), 1, "見張りの記録はちょうど 1 件: {calls:?}");
    let words: Vec<&str> = calls.first().map(|found| found.iter().map(String::as_str).collect()).unwrap_or_default();
    assert_eq!(words.first().copied(), Some("close"), "1 語目は close: {words:?}");
    assert_eq!(words.get(2).copied(), Some("--reason"), "理由を渡す: {words:?}");
    assert_eq!(words.get(3).copied(), Some(format!("landed {landed} ci=none").as_str()), "理由は ci=none（tip= を持たない）: {words:?}");
    assert_eq!(tools.ci_call_count(), 0, "偽 CI は 1 度も撃たれない");
    assert_eq!(remote_refs(&tools), "", "偽 remote に ref は無い（push しない）");
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec!["terminal:close:ok".to_owned()],
        "Landed の着地の後ろは close の 1 件だけ（走らなかった段の event を積まない・母集団 {} 件）: {details:?}",
        details.len()
    );
    clean(&[&repo, &state]);
}

/// (b) 列（`train=2`）: 先頭と後続がそれぞれ自分の sha で close する（見張りの記録は 2 件・理由は便ごとの `landed <自分の sha> ci=none`・
/// 先端の便が居ても `tip=` を持たない）。stdout は列の便ごとに `terminal=closed:no-ci`。偽 CI と偽 remote は動かない。
#[test]
fn pipe_terminal_no_remote_train_closes_each_run_with_its_own_sha() {
    let (repo, state) = repo_with_state();
    let tools = fake_no_remote(&repo, &state);
    let marker = state.join("lens-ran");
    let [id_a, id_b, _] = train_runs(&repo, &state, &marker, [ALL_GREEN, ALL_GREEN, ALL_GREEN]);
    let rules = write_rules_train(&state, "rules-train.toml", Some(2));
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    let stdout = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "列の land は rc 0: {stdout} / {}", stderr_of(&out));
    assert!(stdout.contains(&format!("run={id_a} train=2")), "先頭と後続の 2 本が列で着地した: {stdout}");
    let (sha_a, sha_b) = (landed_sha_of(&state, &id_a), landed_sha_of(&state, &id_b));
    assert_ne!(sha_a, sha_b, "fixture: 2 本の squash は別の commit");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), sha_b, "fixture: main の先端は後続の b");
    let mut calls = ledger_calls(&state);
    calls.sort();
    let mut want = vec![
        vec!["close".to_owned(), "s2-2e5".to_owned(), "--reason".to_owned(), format!("landed {sha_a} ci=none")],
        vec!["close".to_owned(), "s2-3ax".to_owned(), "--reason".to_owned(), format!("landed {sha_b} ci=none")],
    ];
    want.sort();
    assert_eq!(calls, want, "記録は 2 件・便ごとに自分の sha（tip= なし）");
    for id in [&id_a, &id_b] {
        assert_eq!(terminal_lines(&state, id), ["terminal:close:ok"], "終端は close の 1 件: {id} {:?}", landed_details(&state, id));
        let line = stdout.lines().find(|line| line.starts_with(&format!("run={id} landed="))).map(str::to_owned);
        assert!(line.as_deref().is_some_and(|found| found.ends_with("terminal=closed:no-ci")), "stdout: {id} {stdout}");
    }
    assert_eq!(tools.ci_call_count(), 0, "偽 CI は 1 度も撃たれない");
    assert_eq!(remote_refs(&tools), "", "偽 remote に ref は無い");
    clean(&[&repo, &state]);
}

/// (c) 宣言を読めない周は close しない: `show HEAD:.vessel.toml` を落とす偽 git で撃つ land は rc 1・見張りの記録 0 件・
/// `terminal:unreadable` が 1 件。偽 git を外した `--terminal-only` は rc 0（`run=<id> terminal=closed:no-ci`）で記録が 1 件になる。
#[test]
fn pipe_terminal_no_remote_unreadable_declaration_closes_nothing_until_refired() {
    let (repo, state) = repo_with_state();
    let _tools = fake_no_remote(&repo, &state);
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let out = land_once_with_git_shim(&repo, &state, &id, "show HEAD:.vessel.toml", None);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "宣言を読めない周は rc 1: {} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains("terminal=unreadable"), "終端の token: {}", stdout_of(&out));
    assert!(ledger_calls(&state).is_empty(), "close しない: {:?}", ledger_calls(&state));
    assert_eq!(terminal_lines(&state, &id), ["terminal:unreadable"], "終端の行は unreadable の 1 件");
    let landed = landed_sha_of(&state, &id);
    let refired = land_extra(&repo, &state, &id, &["--rules", &ceiling_rules(&state), "--terminal-only"]);
    assert_eq!(refired.status.code(), Some(i32::from(RC_OK)), "偽 git を外した撃ち直しは rc 0: {}", stderr_of(&refired));
    assert_eq!(stdout_of(&refired).trim(), format!("run={id} terminal=closed:no-ci"), "撃ち直しの stdout");
    let calls = ledger_calls(&state);
    assert_eq!(
        calls,
        vec![vec!["close".to_owned(), "s2-2e5".to_owned(), "--reason".to_owned(), format!("landed {landed} ci=none")]],
        "撃ち直しで記録が 1 件（理由は記録から読んだ着地の sha）"
    );
    clean(&[&repo, &state]);
}

/// (d) PR の形で着地した便（`--pr-cmd`）は終端を撃たない: remote を宣言した repo（`fake_terminal`）でも、`--pr-cmd` の着地と
/// 続く `--terminal-only`（着地した sha の記録が無い＝rc 1 で断る）のどちらも偽 remote の ref・偽 CI・偽 bd・見張りの記録を 1 件も動かさない。
#[test]
fn pipe_terminal_no_remote_pr_landed_run_writes_nothing() {
    let (repo, state) = repo_with_state();
    let tools = fake_terminal(&repo, &state, "success");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let bd = state.join("fake-bd.sh").display().to_string();
    let out = land_extra(&repo, &state, &id, &["--pr-cmd", "true", "--bd", &bd]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PR の形の land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("landed=pr"), "PR の形: {}", stdout_of(&out));
    let before = event_count(&state);
    let refired = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &ceiling_rules(&state), "--terminal-only"]);
    assert_eq!(refired.status.code(), Some(i32::from(RC_REFUSED)), "着地した sha の記録が無い周は rc 1: {}", stdout_of(&refired));
    assert_eq!(event_count(&state), before, "撃ち直しは event を 1 件も書かない");
    assert_eq!(remote_refs(&tools), "", "偽 remote に ref は無い");
    assert_eq!(tools.ci_call_count(), 0, "偽 CI は 1 度も撃たれない");
    assert!(!tools.bd_log.exists(), "偽 bd は 1 度も撃たれない");
    assert!(ledger_calls(&state).is_empty(), "見張りの記録は 0 件: {:?}", ledger_calls(&state));
    clean(&[&repo, &state]);
}
