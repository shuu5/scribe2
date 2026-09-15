// flip-check: moved s2-07l.264
// flip-check: moved s2-07l.295
//! land の歯: `pipe_land_` / `pipe_order_`（順番待ち）/ `pipe_follow_`（追随）/ `pipe_retire_`。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く（歯の本文は移しただけ・`s2-07l.264`）。

use super::*;
use vessel::pipe::land;

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
    let goal = format!("goal = \"{LONG_GOAL}\"");
    let path = write_contract(&repo, &["goal"], &[&goal]);
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
    // 落とさない側。**本文は goal 全文を逐語で持ち**、最終行は run へ辿る trailer である。
    let body = git(&repo, &["log", "-1", "--format=%b", "refs/heads/main"]);
    assert!(body.contains(LONG_GOAL), "本文に goal 全文が逐語で在る: {body}");
    let trailer = format!("run: {id}");
    assert_eq!(body.lines().last(), Some(trailer.as_str()), "最終行は run trailer: {body}");
    clean(&[&repo, &state]);
}

/// 負例: goal が 1 文で 72 文字以内なら件名は**その文そのもの**で、切った印（`…`）は付かない
/// （「。」の手前までが先頭の文である）。
#[test]
fn pipe_land_subject_keeps_short_single_sentence_whole() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["goal"], &[r#"goal = "短い 1 文の goal は件名にそのまま載る。""#]);
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

#[test]
fn pipe_land_rebase_without_lens_stops_inconclusive_and_keeps_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    // gate の後に main が別便で進む。CAS の old が動いた＝そのままでは land できない。
    fs::write(repo.join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    // `--lens` 無しの land: 追随（rebase）は済むが撃ち直しの gate は lens を得られず
    // INCONCLUSIVE＝**land しない**（測れなかったを通ったに化けさせない・FR14 で測り直せる）。
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
    let before = event_count(&state);
    fs::remove_file(&marker).expect("lens の marker を消せる");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "汚れた木は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("clean でない"), "理由: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before, "何も書かない");
    assert!(!marker.exists(), "gate を撃ち直さない（lens は走らない）");
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head_before, "rebase を撃たない（HEAD 不変）");
    assert!(worktree.join("dirty.txt").exists(), "未 commit の仕事は残る");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated のまま（回復可能）");
    clean(&[&repo, &state]);
}

/// 撃ち直しの間に main がさらに動いた周は **rc 1 `stale base` で squash しない**（1 回の land が
/// rebase するのは 1 度だけ）。次の land が同じ経路で追随する。
#[test]
fn pipe_land_rebase_refuses_when_main_moves_during_regate() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    fs::write(repo.join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    // 撃ち直しの lens が走っている間に **さらに別便が main を進める**（lens の中で commit する）。
    let racing = format!(
        "cat >/dev/null; git -C '{}' commit -q --allow-empty -m racing; echo '{}'",
        repo.display(),
        lens_verdict("PASS")
    );
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &racing,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "撃ち直し中に動いた main は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("stale base"), "理由: {}", stderr_of(&out));
    // **追随と撃ち直しは実際に起きた**（event に残り lens を 1 回消費した）ので、判定行も残す。
    let stdout = stdout_of(&out);
    assert!(stdout.contains("rebase=") && stdout.contains(&format!("..{moved}")), "追随の行が残る: {stdout}");
    assert!(stdout.contains("verdict=PASS"), "撃ち直しの判定行が残る: {stdout}");
    let raced = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(raced, moved, "lens の中で main が進んでいる");
    assert_eq!(git(&repo, &["rev-parse", &format!("{raced}^")]), moved, "main に載ったのは racing の 1 commit だけ（squash していない）");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated（撃ち直しは PASS）");
    // 次の land が同じ経路で追随して載る。
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let again = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "次の land は追随して載る: {}", stderr_of(&again));
    assert!(stdout_of(&again).contains(&format!("rebase={moved}..{raced}")), "2 度目の追随: {}", stdout_of(&again));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-parse", &format!("{new}^")]), raced, "racing の上に載る");
    clean(&[&repo, &state]);
}

/// 同じ base から 2 便を PASS の gate まで通す（1 本目 = `src/lib.rs`・2 本目 = `src/b.rs`＝
/// write-set は交わらない）。追随の歯の材料。
fn two_gated_runs(repo: &Path, state: &Path, marker: &Path) -> (String, String) {
    let contract_a = write_contract(repo, &[], &[]);
    let id_a = gated_pass(repo, state, &contract_a, marker);
    let contract_b = write_contract(repo, &["write-set"], &[r#"write-set = ["src/b.rs"]"#]);
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
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    // 1 本目が land して main が動く（2 本目の base は置き去り）。
    let first = land_once(&repo, &state, &id_a);
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
    assert!(git(&repo, &["show", &format!("{new}:src/lib.rs")]).contains('x'), "1 本目の仕事も残る");
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
    let contract = write_contract(repo, &[], &[]);
    let id = gated_pass(repo, state, &contract, marker);
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
    let before = event_count(&state);
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
    assert_eq!(event_count(&state), before + 1, "残す event は Failed の 1 本だけ");
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
    let first = land_once(&repo, &state, &id_a);
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
    let first = land_once(&repo, &state, &id_a);
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

/// 便の `Gated` の記帳の件数（gate 1 周 = 1 件）。
fn gate_count(state: &Path, id: &str) -> usize {
    stages(state, id).into_iter().filter(|(stage, _)| *stage == Some(Stage::Gated)).count()
}

/// land の stdout の `landed=` の値（squash commit の sha・無ければ空）。
fn landed_token(out: &Output) -> String {
    stdout_of(out)
        .split_whitespace()
        .find_map(|word| word.strip_prefix("landed="))
        .map(str::to_owned)
        .unwrap_or_default()
}

/// land を背景で撃つ（列で待つ歯の材料・stdout / stderr は `wait_with_output` で読む）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn land_in_background(repo: &Path, state: &Path, id: &str, rules: &str, lens: &str) -> Child {
    Command::new(bin())
        .args(["pipe", "land", "--run", id, "--repo", &repo.display().to_string()])
        .args(["--state-dir", &state.display().to_string(), "--rules", rules, "--lens", lens])
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

/// 撃ち直しが FAIL になった便（`Gated` のまま verdict が FAIL）は列から外れ、後続は待たずに進む
/// （`order=first`・上限の 30 秒を待たない）。
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
    let started = Instant::now();
    let out = land_extra(&repo, &state, &id_c, &["--rules", &rules, "--lens", &pass]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "c の land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "first", "FAIL の b は列に居ない: {}", stdout_of(&out));
    assert!(started.elapsed() < Duration::from_secs(30), "上限まで待たない");
    assert!(show_line(&repo, &state, &id_c).contains("stage=Landed"), "c は Landed");
    clean(&[&repo, &state]);
}

/// `--pr-cmd` の形は列を見ない（main を動かさない）: 前の便が列に居ても待たず、`order=` を出さない。
#[test]
fn pipe_order_pr_cmd_does_not_look_at_the_queue() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (_id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    let started = Instant::now();
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules, "--pr-cmd", "true"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PR の口: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("landed=pr"), "{}", stdout_of(&out));
    assert!(!stdout_of(&out).contains("order="), "列を見ない形は order= を出さない: {}", stdout_of(&out));
    assert!(started.elapsed() < Duration::from_secs(30), "上限まで待たない");
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
    fs::write(repo.join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
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
    let before = event_count(&state);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "祖先でない base は rc 1");
    assert!(stderr_of(&out).contains("stale base"), "理由: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("祖先でない"), "理由の弁別: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before, "何も書かない");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), root, "main は動かない");
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head_before, "worktree も動かない（rebase を撃たない）");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated のまま");
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
        vec!["schema", "run", "bead", "sha", "verdict", "evidence", "ts", "order", "size", "files", "lines", "pub_symbols"],
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
        Some(&["order", "size", "files", "lines", "pub_symbols"][..]),
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
    assert_eq!(keys.last(), Some(&"order"), "面 5 の行は在り order で終わる: {keys:?}");
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

    // 値欠け（SRS NFR4「黙って落とさない」）。
    let missing = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--pr-cmd",
    ]);
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "値欠けは rc 1");
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
fn landed_pr(repo: &Path, state: &Path, contract: &Path, marker: &Path) -> String {
    let id = gated_pass(repo, state, contract, marker);
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

/// 負例: `Failed` でも畳める理由（`rebase-empty` / `rebase-conflict`）でない便——ここでは
/// **main の実測が赤かった便**（`main-red`）——は畳まない（rc 1・worktree 不動・event 0 増）。
/// 木は clean のままなので、この rc 1 は **clean 検査ではなく終端の理由**を見ている
/// （「断ってから解いて通す」形は上の歯が担保する）。
#[test]
fn pipe_retire_rebase_empty_refuses_other_failed_reasons() {
    let (repo, state) = repo_with_state();
    // 1 回目（worktree）は緑・2 回目（main の実測）は赤になる verify 行＝`main-red` で終端する。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-once.sh"]"#]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let red = land_once(&repo, &state, &id);
    assert_eq!(red.status.code(), Some(i32::from(RC_REFUSED)), "main が赤い land は rc 1: {}", stderr_of(&red));
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "終端の段は Failed");
    let live = worktree_of(&repo, &id);
    assert!(git(&live, &["status", "--porcelain"]).is_empty(), "木は clean のまま");

    let before = event_count(&state);
    let out = retire_once(&repo, &state, &id);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_REFUSED)),
        "rebase-conflict の便は畳まない: {}",
        stdout_of(&out)
    );
    assert!(live.exists(), "断った周は worktree を動かさない");
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(!retired.exists(), "retired/<run> を作らない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
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
    assert!(stdout_of(&out).contains(&format!("landed={new} anchor=synced")), "判定行に anchor=synced: {}", stdout_of(&out));
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
    assert!(stdout_of(&out).contains(&format!("landed={new} anchor=skipped:dirty")), "{}", stdout_of(&out));
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
        assert!(stdout_of(&out).contains(&format!("landed={new} anchor=skipped:not-main")), "{label}: {}", stdout_of(&out));
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
        &[r#"write-set = ["src/lib.rs", "src/new.rs"]"#],
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
    assert!(!record.exists(), "gate（branch の worktree）は record を書かない＝印は実測の周のもの");

    let out = land_once(&repo, &state, &id);

    // 5. 既存の挙動は不変（Landed・`anchor=synced`・main は新 sha）。
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(new, base, "main は進む");
    assert!(stdout_of(&out).contains(&format!("landed={new} anchor=synced")), "判定行: {}", stdout_of(&out));
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
/// 名指す main の sha が入る（節が無い周は空＝rebase が落ちて歯が赤くなる＝空虚にならない）。
fn stub_runner(state: &Path, second: &str) -> String {
    stub_runner_turns(state, IMPLEMENT, second)
}

/// [`stub_runner`] の turn 1 の本文も振る形（`first` = turn 1・`second` = turn 2 以降）。
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
         cat > \"$D/stdin-$N\"\nif [ \"$N\" = 1 ]; then\n{first}\nfi\n\
         SHA=$(sed -n 's/^- main が \\(.*\\) へ進んだ$/\\1/p' \"$D/stdin-$N\" | head -1)\n{second}\n",
        dir.display()
    );
    fs::write(&path, body).expect("stub を書ける");
    format!("sh {}", path.display())
}

/// turn 2 の本文: 衝突を write-set の中で解いて `git rebase --continue` で終える。
const RESOLVE: &str = "if git rebase \"$SHA\"; then exit 0; fi\nprintf '// seed\\ny\\nx\\n' > src/lib.rs\ngit add src/lib.rs\nGIT_EDITOR=true git rebase --continue";

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
    let out = Command::new(bin())
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

/// 回数を**読めない**周は起こし直さず `Failed detail=follow-unmeasured` + rc 2 で終端する
/// （上限到達の rc 1 と分ける・NFR4）。読めなさは、rebase の呼出しに合わせて event log へ
/// 壊れた行を混ぜる偽 git で作る。
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
    let out = Command::new(bin())
        .args(["pipe", "land", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--runner", &runner])
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない回数は rc 2: {}", stderr_of(&out));
    assert_eq!(stub_calls(&state), 1, "起こし直さない");
    let log = fs::read_to_string(&events).expect("event log");
    let last = log.lines().rfind(|line| !line.is_empty()).unwrap_or_default();
    assert!(
        last.contains("\"stage\":\"Failed\"") && last.contains("follow-unmeasured"),
        "終端の理由は上限到達と分ける: {last}"
    );
    assert!(!last.contains("\"detail\":\"rebase-conflict\""), "上限到達を名乗らない: {last}");
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
    let out = Command::new(bin())
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
