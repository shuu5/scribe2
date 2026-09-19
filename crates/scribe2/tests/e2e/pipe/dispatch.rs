//! 列（`pipe dispatch`）の歯（設計 docs/design/dispatcher.md §8・契約表の行 a）。
//!
//! 偽の台帳（`bd --readonly list --json` の出力を返す script）と偽の live 便（`pipe intake` で起こした本物の
//! 便）で、**列の入力・順序・起動条件・介入の印**を外形から測る。順序の 1 関数と印の畳み込みは pure ゆえ
//! `pipe/dispatch.rs` の in-file の歯が持つ（同じ接頭辞 `pipe_dispatch_`）。

use super::{
    ceiling_rules, clean, commit_rows, design_doc_rows, fake_lens, gate_once, git, implemented, intake_bead,
    kind_count, lens_verdict, question_runner, questioned, repo_with_state, review_lens_pass, row_fields, run_pipe,
    shim_path, stderr_of, run_id_of, stdout_of, write_contract, write_design, DESIGN_FILE, IMPLEMENT, RC_BLOCKED,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use vessel::cli_outcome::RC_OK;

/// 列の 1 行の書き出し（器の字面を借りない＝外形を測る側は自分で書く）。
const LINE: &str = "[DISPATCH]";

/// 件数の行の書き出し。
const COUNT: &str = "[DISPATCH-COUNT]";

/// 台帳を読めなかった周の行。
const UNMEASURED: &str = "[DISPATCH-UNMEASURED reason=ledger]";

/// 列が空の周の行（読めなかった周と**別の行**である・C10）。
const NONE_LINE: &str = "[DISPATCH-NONE]";

/// 審査の判定 file（run dir の直下）。
const REVIEW_FILE: &str = "review.json";

/// 台帳の 1 件（`bd --readonly list --all --json` の要素・**key の字面は現物から採る**）。
fn issue(id: &str, priority: u64, row: &str) -> String {
    listed(id, "open", priority, &format!("design = {DESIGN_FILE}#{row}"), &[])
}

/// 台帳の 1 件（status と依存も呼び手が選ぶ形）。依存の要素は現物と同じ key
/// （`issue_id` / `depends_on_id` / `type`）で、**依存先の status は持たない**。
fn listed(id: &str, status: &str, priority: u64, acceptance: &str, deps: &[(&str, &str)]) -> String {
    let listed: Vec<String> = deps
        .iter()
        .map(|(on, kind)| {
            format!("{{\"issue_id\":\"{id}\",\"depends_on_id\":\"{on}\",\"type\":\"{kind}\"}}")
        })
        .collect();
    format!(
        "{{\"id\":\"{id}\",\"status\":\"{status}\",\"priority\":{priority},\"labels\":[],\
         \"acceptance_criteria\":\"{acceptance}\",\"dependencies\":[{}]}}",
        listed.join(",")
    )
}

/// 台帳の JSON を返す偽の `bd`（引数は読み飛ばす）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_bd(state: &Path, issues: &[String]) -> String {
    let json = state.join("ledger.json");
    fs::write(&json, format!("[{}]\n", issues.join(","))).expect("偽の台帳を書ける");
    script(&state.join("bd"), &format!("cat '{}'\n", json.display()))
}

/// 実行権つきの `/bin/sh` script を書き、その path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn script(path: &Path, body: &str) -> String {
    fs::write(path, format!("#!/bin/sh\n{body}")).expect("script を書ける");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("script に実行権を付ける");
    path.display().to_string()
}

/// 渡された manifest の写しに**台帳の待ち上限の行**を足す（列はこれが無いと `no-rule` で止まる）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn with_ledger_row(state: &Path, name: &str, base: &str) -> String {
    let body = fs::read_to_string(base).expect("元の写しを読める");
    let row = "[[rule]]\nid = \"seat.ledger_timeout_s\"\nkind = \"LedgerTimeoutS\"\n\
               value = 60\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n";
    let path = state.join(name);
    fs::write(&path, format!("{body}\n{row}")).expect("写しを書ける");
    path.display().to_string()
}

/// 列が読む manifest（受付の上限の写しに台帳の待ち上限の行を足す）。
///
/// `ceiling_rules` の写しは受付の行しか持たないので、そのまま渡すと列は `no-rule` で止まる
/// （**測れないを 0 件に読み替えない**側に倒れる・C10）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn dispatch_rules(state: &Path) -> String {
    let base = fs::read_to_string(ceiling_rules(state)).expect("受付の写しを読める");
    let row = |id: &str, kind: &str, value: u64| {
        format!("[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n")
    };
    // 台帳の待ち上限（列が読む）と、終端の口が読む行（`pipe stop`）を足す＝**同じ 1 本の manifest で
    // 終端も列も動く**（終端の 1 周が別の写しを読むと、測っているものがずれる）。
    let rows = format!(
        "{}\n{}",
        row("seat.ledger_timeout_s", "LedgerTimeoutS", 60),
        row("pipe.stop_grace_ms", "StopGraceMs", super::embedded_int("pipe.stop_grace_ms")),
    );
    let path = state.join("rules-dispatch.toml");
    fs::write(&path, format!("{base}\n{rows}")).expect("列の写しを書ける");
    path.display().to_string()
}

/// `pipe dispatch ls` を 1 回撃つ。
fn ls(repo: &Path, state: &Path, bd: &str) -> Output {
    run_pipe(&[
        "dispatch", "ls",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(state),
        "--bd", bd,
    ])
}

/// 落ちた周に写す 1 行（**rc と stdout と stderr**）。stdout だけを写すと、断りで早返りした周の理由が
/// 見えない（`s2-07l.486` の main の赤で、rc を見るために歯を patch する羽目になった）。
fn told(out: &Output) -> String {
    format!(
        "rc={:?} out={} err={}",
        out.status.code(),
        stdout_of(out).replace('\n', " / ").trim_end(),
        stderr_of(out).replace('\n', " / ").trim_end()
    )
}

/// `dispatch ls` の 1 行のうち `bead=<id>` のものの `reason=` の値。
fn reason_of(out: &Output, bead: &str) -> String {
    stdout_of(out)
        .lines()
        .filter(|line| line.starts_with(LINE))
        .find(|line| line.contains(&format!("bead={bead} ")))
        .and_then(|line| line.split("reason=").nth(1).map(str::to_owned))
        .unwrap_or_default()
}

/// `bead` が `beads` のどれかである `RunCreated` の件数（起こした便の効果・上限まで待つ）。
///
/// 起こすのは子 process なので、記帳は親の終端の**後**に来る。待たずに数えると「起こしていない」と
/// 読み違える（測れていないを 0 件に読み替えない・C10）。
fn created(state: &Path, beads: &[&str], want: usize) -> usize {
    let log = state.join("fleet").join("events.jsonl");
    let count = || -> usize {
        let Ok(text) = fs::read_to_string(&log) else {
            return 0;
        };
        text.lines()
            .filter(|line| line.contains("\"kind\":\"RunCreated\""))
            .filter(|line| beads.iter().any(|bead| line.contains(&format!("\"bead\":\"{bead}\""))))
            .count()
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while count() < want && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // **0 を待たずに数えない**: 起こすのは子 process なので、`want` が 0 の周も少し待ってから数える
    // （遅れて来た 1 件を「起こしていない」と読み違えない・C10）。
    if want == 0 {
        std::thread::sleep(std::time::Duration::from_millis(800));
    }
    count()
}

/// `[DISPATCH-COUNT] total=<n> ready=<k>` の行。
fn count_of(out: &Output) -> String {
    stdout_of(out).lines().find(|line| line.starts_with(COUNT)).unwrap_or_default().to_owned()
}

/// 交差する行と交差しない行を 1 回で commit する（同じ base から 2 便を測る）。
fn two_rows(repo: &Path) {
    commit_rows(
        repo,
        &[
            row_fields("a", &["write-set"], &[r#"write-set = ["src/lib.rs"]"#]),
            row_fields("b", &["write-set"], &[r#"write-set = ["src/b.rs"]"#]),
        ],
    );
}

/// (§8 起動条件) live 便と write-set が交差する契約は `overlap` で待ち、交差しない契約だけが起動の構築点へ届く。
///
/// base には **`pipe dispatch` の口が無い**（RED）。
#[test]
fn pipe_dispatch_holds_the_crossing_contract_and_readies_only_the_disjoint_one() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    // 行 a（`src/lib.rs`）を持つ live な便を 1 本置く。
    let live = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), "s2-live");
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a"), issue("s2-toy.2", 2, "b")]);
    let out = ls(&repo, &state, &bd);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "ls は rc 0: {}", stderr_of(&out));
    assert_eq!(
        reason_of(&out, "s2-toy.1"),
        format!("overlap:{live}/1"),
        "行 a は live 便と 1 file 交差する: {}",
        stdout_of(&out)
    );
    assert_eq!(reason_of(&out, "s2-toy.2"), "-", "行 b は交差しない: {}", stdout_of(&out));
    assert_eq!(count_of(&out), format!("{COUNT} total=2 ready=1"), "母集団 2 件のうち起こすのは 1 本");
    clean(&[&repo, &state]);
}

/// (§8 台帳が読めない周) 台帳の子 process が落ちた周は `UNMEASURED` で、**件数の行を出さない**
/// （`0 件`と融合しない・C10・NFR4）。
#[test]
fn pipe_dispatch_unmeasured_ledger_starts_nothing_and_is_not_zero() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let broken = script(&state.join("bd-broken"), "exit 1\n");
    let out = ls(&repo, &state, &broken);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "ls は rc 0: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out).trim_end(), UNMEASURED, "読めない周の 1 行だけ");
    assert!(!stdout_of(&out).contains(COUNT), "件数の行を出さない（0 件に読み替えない）");
    // 同じ置き場で**読める空の台帳**は `[DISPATCH-NONE]`＝「0 件」と「読めない」は別の行である。
    let empty = fake_bd(&state, &[]);
    let none = ls(&repo, &state, &empty);
    assert_eq!(stdout_of(&none).trim_end(), NONE_LINE, "0 件の行は読めない周と別");
    // 読める非空の台帳では件数が出る＝上の 2 行はどちらも「列が空」の 1 形ではない。
    let bd = fake_bd(&state, &[issue("s2-toy.2", 2, "b")]);
    let measured = ls(&repo, &state, &bd);
    assert_eq!(count_of(&measured), format!("{COUNT} total=1 ready=1"), "読めた周は件数が出る");
    clean(&[&repo, &state]);
}

/// (§8 介入) `first` は priority より先に来て、`hold` は起こさず、`release` で戻る（event log の往復）。
#[test]
fn pipe_dispatch_first_outranks_priority_and_hold_stops_the_start() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a"), issue("s2-toy.2", 0, "b")]);
    let beads = |out: &Output| -> Vec<String> {
        stdout_of(out)
            .lines()
            .filter(|line| line.starts_with(LINE))
            .filter_map(|line| line.split("bead=").nth(1)?.split_whitespace().next().map(str::to_owned))
            .collect()
    };
    let plain = ls(&repo, &state, &bd);
    assert_eq!(beads(&plain), vec!["s2-toy.2", "s2-toy.1"], "印が無ければ P0 が先: {}", stdout_of(&plain));
    assert_eq!(count_of(&plain), format!("{COUNT} total=2 ready=2"), "交差しない 2 本は両方起こせる");
    let mark = |verb: &str, bead: &str| {
        let out = run_pipe(&["dispatch", verb, bead, "--state-dir", &state.display().to_string()]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{verb}: {}", stderr_of(&out));
    };
    let marked = run_pipe(&["dispatch", "first", "s2-toy.1", "--state-dir", &state.display().to_string()]);
    assert_eq!(marked.status.code(), Some(i32::from(RC_OK)), "first: {}", stderr_of(&marked));
    // **`first` の記録の直後にも 1 周撃つ**（§5）。`--repo` を渡していないので材料は解けず、1 周を
    // 撃った事実だけが `unmeasured reason=args` の行で残る（撃っていなければ行が無い）。
    assert_eq!(
        stdout_of(&marked).lines().last(),
        Some("dispatch=unmeasured reason=args"),
        "first の直後に 1 周: {}",
        stdout_of(&marked)
    );
    let first = ls(&repo, &state, &bd);
    assert_eq!(beads(&first), vec!["s2-toy.1", "s2-toy.2"], "first は P0 より先: {}", stdout_of(&first));
    assert!(
        stdout_of(&first).contains(&format!("{LINE} bead=s2-toy.1 prio=2 mark=first reason=-")),
        "印は行にも出る: {}",
        stdout_of(&first)
    );
    mark("hold", "s2-toy.1");
    let held = ls(&repo, &state, &bd);
    assert!(reason_of(&held, "s2-toy.1").starts_with("hold:"), "hold の理由: {}", stdout_of(&held));
    assert_eq!(count_of(&held), format!("{COUNT} total=2 ready=1"), "hold は列に残して起こさない");
    mark("release", "s2-toy.1");
    let released = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&released, "s2-toy.1"), "-", "release で戻る: {}", stdout_of(&released));
    assert_eq!(count_of(&released), format!("{COUNT} total=2 ready=2"), "印が外れて 2 本とも起こせる");
    clean(&[&repo, &state]);
}

/// (§2 終端の便は列外) 同じ契約 file の sha で**終端に着いた**便が在る契約は `settled` で列外になり、
/// 設計 doc の行が変わって sha が動くと列に戻る。
#[test]
fn pipe_dispatch_keeps_a_settled_contract_out_until_its_sha_moves() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let failed = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), "s2-toy.1");
    let review = state.join("pipe").join(&failed).join(REVIEW_FILE);
    fs::write(&review, "{\"verdict\":\"FAIL\"}\n").expect("審査の判定を書ける");
    // FAIL の判定を書いた時点でこの便は終端である（`live` は verdict を読む）ので、交差の相手には入らない
    // ——測るのは審査の列外であって交差ではない。
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a")]);
    let out = ls(&repo, &state, &bd);
    let reason = reason_of(&out, "s2-toy.1");
    assert!(reason.starts_with("settled:"), "同じ sha では列外: {}", stdout_of(&out));
    assert_eq!(count_of(&out), format!("{COUNT} total=1 ready=0"), "列には載るが起こさない");
    // 行の本文を変える＝生成される契約 file の sha が動く。
    write_design(
        &repo,
        &design_doc_rows(&[
            row_fields("a", &["write-set", "done"], &[r#"write-set = ["src/lib.rs"]"#, r#"done = "改訂した""#]),
            row_fields("b", &["write-set"], &[r#"write-set = ["src/b.rs"]"#]),
        ]),
    );
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "design-row-revised"]);
    let again = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&again, "s2-toy.1"), "-", "sha が動けば列に戻る: {}", stdout_of(&again));
    assert_eq!(count_of(&again), format!("{COUNT} total=1 ready=1"), "改訂した契約は起こせる");
    clean(&[&repo, &state]);
}

/// (§2 終端の便は列外・裏側) 判定の鍵は**現在の契約 file の sha**である: 同じ sha の直前の便が live なら、
/// それより**古い**同じ sha の終端は列を塞がない。
///
/// `run id = <bead>-<UTC の秒>` なので 2 便は別の秒に起こす（同じ秒だと id が衝突して 2 本目が断られる）。
#[test]
fn pipe_dispatch_settled_reads_the_run_just_before_the_current_sha() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let design = format!("{DESIGN_FILE}#a");
    let older = intake_bead(&repo, &state, &design, "s2-toy.1");
    fs::write(state.join("pipe").join(&older).join(REVIEW_FILE), "{\"verdict\":\"FAIL\"}\n")
        .expect("古い便の審査の判定を書ける");
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a")]);
    let blocked = ls(&repo, &state, &bd);
    assert!(
        reason_of(&blocked, "s2-toy.1").starts_with("settled:"),
        "古い便しか無い周は列外: {}",
        stdout_of(&blocked)
    );
    // 同じ契約でもう 1 便（秒を跨ぐ）。こちらは審査 PASS のまま **live** にしておく。
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let newer = intake_bead(&repo, &state, &design, "s2-toy.1");
    assert_ne!(newer, older, "2 便は別の run id");
    let out = ls(&repo, &state, &bd);
    // **直前の便が live なら古い終端は塞がない**＝理由は交差（live な自分の便）であって `settled` ではない。
    assert_eq!(
        reason_of(&out, "s2-toy.1"),
        format!("overlap:{newer}/1"),
        "直前の便が live なら古い終端は塞がない: {}",
        stdout_of(&out)
    );
    assert_eq!(count_of(&out), format!("{COUNT} total=1 ready=0"), "live な便と交差して 0 本");
    clean(&[&repo, &state]);
}

/// `release` の印を 1 つ打つ（`--repo` を渡さないので直後の 1 周は `unmeasured reason=args`＝便は起きない）。
fn release(state: &Path, bead: &str) {
    let out = run_pipe(&["dispatch", "release", bead, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "release は rc 0（{}）", told(&out));
}

/// 行 a の便を `Failed` に着ける（intake → rc 2・commit 0 の偽 runner）。
fn failed_run(repo: &Path, state: &Path, bead: &str) -> String {
    let id = intake_bead(repo, state, &format!("{DESIGN_FILE}#a"), bead);
    fail_run(repo, state, &id);
    id
}

/// 便を rc 2・commit 0 の偽 runner で `Failed` に着ける（spawn 自体の rc は 0＝段は stdout の行で測る）。
fn fail_run(repo: &Path, state: &Path, id: &str) {
    let out = super::spawn_with(repo, state, id, "exit 2");
    assert!(stdout_of(&out).contains("stage=Failed"), "rc 2 の runner は Failed（{}）", told(&out));
}

/// 行 a の便を gate の判定（偽 lens の verdict）まで通す（intake → 1 commit の偽 runner → gate）。
fn gated_run(repo: &Path, state: &Path, bead: &str, verdict: &str) -> String {
    let id = intake_bead(repo, state, &format!("{DESIGN_FILE}#a"), bead);
    let spawned = super::spawn_with(repo, state, &id, super::TOY_COMMIT);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0（{}）", told(&spawned));
    let lens = fake_lens(&state.join(format!("gate-lens-{verdict}")), &lens_verdict(verdict));
    let out = gate_once(repo, state, &id, Some(&lens));
    assert!(gated_pair_ok(verdict, &out), "gate {verdict}（{}）", told(&out));
    id
}

/// gate の rc が verdict と噛み合うか（PASS は rc 0・FAIL は rc 非 0）。
fn gated_pair_ok(verdict: &str, out: &Output) -> bool {
    (out.status.code() == Some(i32::from(RC_OK))) == (verdict == "PASS")
}

/// 終端の便を 1 本置いた置き場で `release` の前後の `dispatch ls` の理由を測る（**同じ sha のまま**）。
///
/// 返すのは `(release の前, release の後)` の `reason=` の値。前は必ず `settled:<sha>/<段>` で、後が
/// `-` に戻るか `settled:` のままかを呼び手が段ごとに判じる（母集団 = 終端の段の種類）。
fn reasons_around_release(repo: &Path, state: &Path, bead: &str, stage: &str) -> (String, String) {
    let bd = fake_bd(state, &[issue(bead, 2, "a")]);
    let before = ls(repo, state, &bd);
    let settled = reason_of(&before, bead);
    assert!(settled.starts_with("settled:"), "release の前は列外（{}）", told(&before));
    assert!(settled.ends_with(&format!("/{stage}")), "段は {stage}: {settled}");
    assert_eq!(count_of(&before), format!("{COUNT} total=1 ready=0"), "列には載るが起こさない");
    release(state, bead);
    let after = ls(repo, state, &bd);
    (settled, reason_of(&after, bead))
}

/// (§12 列へ戻す印) `Failed` で終端した便の bead は `settled` で列外だが、その後の `release` で**同じ sha の
/// まま**列に戻り（`reason=-`・`ready=1`）、起こし直した便が同じ sha でまた終端に着くと再び `settled` になる
/// （**印 1 回で起き直るのは 1 回**＝§2 の無限再起動を開け直さない）。
///
/// 起こし直した便は run dir の fixture で作る（同じ bead で秒を跨いで intake → rc 2 の runner）。起こす
/// 効果そのものは印の直後の 1 周の歯（`..._marks_fire_without_children`）と手動の 1 周の歯が測る。
#[test]
fn pipe_dispatch_release_requeues_a_failed_run_once_at_the_same_sha() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    let first = failed_run(&repo, &state, bead);
    let (settled, released) = reasons_around_release(&repo, &state, bead, "Failed");
    assert_eq!(released, "-", "release で同じ sha のまま列に戻る");
    let bd = fake_bd(&state, &[issue(bead, 2, "a")]);
    let back = ls(&repo, &state, &bd);
    assert_eq!(count_of(&back), format!("{COUNT} total=1 ready=1"), "戻った契約は起こせる（{}）", told(&back));
    // **起こし直した便が同じ sha でまた終端に着く**（秒を跨いで同じ bead の 2 本目・同じ契約 file）。
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = failed_run(&repo, &state, bead);
    assert_ne!(second, first, "起こし直した便は新しい run id");
    let again = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&again, bead), settled, "同じ sha でまた終端＝再び settled（印は 1 回しか効かない）（{}）", told(&again));
    assert_eq!(count_of(&again), format!("{COUNT} total=1 ready=0"), "2 度目は起こさない");
    // 2 度目の `release` はまた 1 回だけ戻す（印ごとに 1 回）。
    release(&state, bead);
    let twice = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&twice, bead), "-", "2 度目の release でまた戻る（{}）", told(&twice));
    clean(&[&repo, &state]);
}

/// (§12 列へ戻す印) 終端より**前**の `release` は効かない——印は便の最後の記帳より後に在る 1 件だけを見る。
#[test]
fn pipe_dispatch_release_requeues_nothing_when_the_mark_precedes_the_terminal() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    let id = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), bead);
    // live な便のうちに印を打つ（この時点では列外でなく、自分の便との交差で待つ）。
    release(&state, bead);
    let bd = fake_bd(&state, &[issue(bead, 2, "a")]);
    let live = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&live, bead), format!("overlap:{id}/1"), "終端の前は交差で待つ（{}）", told(&live));
    // その後に終端へ着く（rc 2 の runner）。
    fail_run(&repo, &state, &id);
    let after = ls(&repo, &state, &bd);
    let reason = reason_of(&after, bead);
    assert!(reason.starts_with("settled:"), "終端より前の release は効かない＝列外のまま（{}）", told(&after));
    assert!(reason.ends_with("/Failed"), "段は Failed: {reason}");
    assert_eq!(count_of(&after), format!("{COUNT} total=1 ready=0"), "起こさない");
    clean(&[&repo, &state]);
}

/// (§12 戻さない段) `Landed` の便は `release` の後も `settled` のまま（済んでいる・起こし直すと同じ変更を
/// もう一度作る）。
#[test]
fn pipe_dispatch_release_requeues_not_a_landed_run() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    let id = gated_run(&repo, &state, bead, "PASS");
    let landed = super::land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land は rc 0（{}）", told(&landed));
    let (settled, released) = reasons_around_release(&repo, &state, bead, "Landed");
    assert_eq!(released, settled, "Landed は release の後も列外のまま（理由も変わらない）");
    clean(&[&repo, &state]);
}

/// (§12 戻さない段) 審査 FAIL（`Reviewed` で終端）の便は `release` の後も `settled` のまま
/// （FR49「中身が変わるまで列に入らない」）。
#[test]
fn pipe_dispatch_release_requeues_not_a_review_failed_run() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    let id = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), bead);
    fs::write(state.join("pipe").join(&id).join(REVIEW_FILE), "{\"verdict\":\"FAIL\"}\n")
        .expect("審査の判定を書ける");
    let (settled, released) = reasons_around_release(&repo, &state, bead, "Reviewed");
    assert_eq!(released, settled, "審査 FAIL は release の後も列外のまま（理由も変わらない）");
    clean(&[&repo, &state]);
}

/// (§12 戻す段) `Stopped` の便（人が止めた）は `release` で列に戻る。
#[test]
fn pipe_dispatch_release_requeues_a_stopped_run() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    let id = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), bead);
    super::stop_run_ok(&state, &id);
    let (_, released) = reasons_around_release(&repo, &state, bead, "Stopped");
    assert_eq!(released, "-", "Stopped は release で戻る");
    clean(&[&repo, &state]);
}

/// (§12 戻す段) gate の判定で終端になった便（`Gated` の verdict FAIL）は `release` で列に戻る——gate の
/// FAIL には flaky な歯で落ちた周が含まれ、契約の字を変えずに測り直す口が他に無い。
#[test]
fn pipe_dispatch_release_requeues_a_gate_failed_run() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    gated_run(&repo, &state, bead, "FAIL");
    let (_, released) = reasons_around_release(&repo, &state, bead, "Gated");
    assert_eq!(released, "-", "gate FAIL は release で戻る");
    clean(&[&repo, &state]);
}

/// (§3 起動条件) 閉じていない `blocks` の依存は `dependency` で待ち、依存先が closed になると起こせる。
/// 所属（`parent-child`）は順序ではないので待たせない（`.beads/PRIME.md` R2）。
///
/// 依存の要素は**依存先の status を持たない**ので、判定は同じ一覧（`--all`）の中で引く＝この歯は key の
/// 字面（`depends_on_id` / `type`）まで固定する（key を読み違えると依存が黙って「全部閉じた」に化ける）。
#[test]
fn pipe_dispatch_waits_for_an_open_blocks_dependency_and_ignores_the_parent() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let acceptance = format!("design = {DESIGN_FILE}#a");
    let waiting = listed("s2-toy.1", "open", 2, &acceptance, &[("s2-dep", "blocks"), ("s2-epic", "parent-child")]);
    let open_dep = listed("s2-dep", "open", 2, "memo", &[]);
    let epic = listed("s2-epic", "open", 2, "memo", &[]);
    let blocked = fake_bd(&state, &[waiting.clone(), open_dep, epic.clone()]);
    let out = ls(&repo, &state, &blocked);
    assert_eq!(reason_of(&out, "s2-toy.1"), "dependency:s2-dep", "閉じていない blocks 1 本: {}", stdout_of(&out));
    assert_eq!(count_of(&out), format!("{COUNT} total=3 ready=0"), "母集団 3 件・起こすのは 0 本");
    // 依存先を closed にすると起こせる（親の epic は open のままでも待たせない）。
    let closed_dep = listed("s2-dep", "closed", 2, "memo", &[]);
    let freed = fake_bd(&state, &[waiting, closed_dep, epic]);
    let again = ls(&repo, &state, &freed);
    assert_eq!(reason_of(&again, "s2-toy.1"), "-", "blocks が閉じれば起こせる: {}", stdout_of(&again));
    assert_eq!(count_of(&again), format!("{COUNT} total=2 ready=1"), "closed の bead は列の入力に入らない");
    clean(&[&repo, &state]);
}

/// (§5 材料の読み) 候補 N 件の 1 周で **repo の走査は 1 回**（候補ごとに `generated` / `judge` で読み直さない）。
///
/// 数えるのは PATH の偽 git が記録した `git ls-files` の回数である（走査の入口は必ずここを通る）。
/// base は候補の数だけ走査するので RED になる。母集団は `[DISPATCH-COUNT]` の `total=` で同時に出す。
#[test]
fn pipe_terminal_dispatch_reads_the_repo_materials_once_for_every_candidate() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    // 行 a と行 b を指す候補を 4 件（同じ 2 行を指す＝走査の回数だけが候補数に依る）。
    let issues: Vec<String> = ["s2-toy.1", "s2-toy.2", "s2-toy.3", "s2-toy.4"]
        .iter()
        .enumerate()
        .map(|(at, id)| issue(id, 2, if at % 2 == 0 { "a" } else { "b" }))
        .collect();
    let bd = fake_bd(&state, &issues);
    let log = state.join("ls-files.log");
    let script = format!("case \"$*\" in *ls-files*) printf 'x\\n' >> '{}';; esac", log.display());
    let path = shim_path(&state, "git-count", &script);
    let measured = Command::new(super::bin())
        .args(["pipe", "dispatch", "ls"])
        .args(["--state-dir", &state.display().to_string()])
        .args(["--repo", &repo.display().to_string()])
        .args(["--rules", &dispatch_rules(&state)])
        .args(["--bd", &bd])
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    assert_eq!(measured.status.code(), Some(i32::from(RC_OK)), "ls は rc 0: {}", stderr_of(&measured));
    assert_eq!(count_of(&measured), format!("{COUNT} total=4 ready=2"), "母集団 4 件: {}", stdout_of(&measured));
    let scans = fs::read_to_string(&log).map(|text| text.lines().count()).unwrap_or_default();
    assert_eq!(scans, 1, "候補 4 件の 1 周で repo の走査は 1 回（母集団 4 件）");
    clean(&[&repo, &state]);
}

/// (§5 手動の 1 周) subcommand の無い `pipe dispatch` は 1 周を撃ってその結果を 1 行で返し、通る便を
/// **起こす**（`RunCreated` が増える）。測れない周は件数でなく理由を名乗る（C10）。
///
/// 印の直後の 1 周は**便を起こさない歯**（`..._marks_fire_without_children`・行 g）が測る——ここで
/// 続けて測ると、起こした子 process が走っている最中の状態に依存する（`s2-07l.487`）。
#[test]
fn pipe_terminal_dispatch_manual_turn_starts_the_runs_it_can() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a"), issue("s2-toy.2", 0, "b")]);
    let turn = || -> Output {
        let args: Vec<String> = vec![
            "dispatch".to_owned(),
            "--state-dir".to_owned(),
            state.display().to_string(),
            "--repo".to_owned(),
            repo.display().to_string(),
            "--rules".to_owned(),
            dispatch_rules(&state),
            "--bd".to_owned(),
            bd.clone(),
            // 起こした便の審査は**偽 PASS の lens**で通す（既定の lens は実 claude を起こす）。
            "--lens".to_owned(),
            review_lens_pass(&state),
            // 実装役は偽の 1 行（器は runner の既定を持たない）。
            "--runner".to_owned(),
            "true".to_owned(),
        ];
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        run_pipe(&borrowed)
    };
    let manual = turn();
    assert_eq!(manual.status.code(), Some(i32::from(RC_OK)), "手動の 1 周は rc 0（{}）", told(&manual));
    assert_eq!(
        stdout_of(&manual).trim_end(),
        "dispatch=started:2,resumed:0,waiting:0",
        "交差しない 2 本は両方起こせる（{}）",
        told(&manual)
    );
    // **起こした効果**: 起こした 2 本ぶんの `RunCreated` が置き場に積まれる（構築点で止まらない・裁定 (A)）。
    assert_eq!(created(&state, &["s2-toy.1", "s2-toy.2"], 2), 2, "起こした便の RunCreated が 2 件");
    // 台帳を読めない周は件数でなく理由を名乗る（0 件と融合しない・C10）。
    let broken = script(&state.join("bd-broken2"), "exit 1\n");
    let unmeasured = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &broken,
        "--runner", "true",
    ]);
    assert_eq!(stdout_of(&unmeasured).trim_end(), "dispatch=unmeasured reason=ledger", "読めない周は理由（{}）", told(&unmeasured));
    // **実装役の口が無い周は台帳も読まない**（起こせないと分かっている＝理由が別の値・C10）。
    let no_runner = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &bd,
    ]);
    assert_eq!(
        stdout_of(&no_runner).trim_end(),
        "dispatch=unmeasured reason=no-runner",
        "runner が無い周（{}）",
        told(&no_runner)
    );
    clean(&[&repo, &state]);
}

/// (§5 便の終端) `pipe stop` の終端の記帳の直後に列が 1 周撃たれ、**交差の解けた便が起こされる**
/// （`RunCreated` が増える）。台帳を読めない周でも終端の rc は変わらない。
#[test]
fn pipe_terminal_dispatch_stop_starts_the_contract_whose_overlap_just_cleared() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    // 行 a を持つ live な便＝同じ行を指す候補と交差する。
    let live = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), "s2-live");
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a")]);
    let blocked = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&blocked, "s2-toy.1"), format!("overlap:{live}/1"), "止める前は交差で待つ");
    assert_eq!(count_of(&blocked), format!("{COUNT} total=1 ready=0"), "起こせる便は 0 本");
    let terminal = |client: &str| -> Output {
        run_pipe(&[
            "stop", "--run", &live,
            "--state-dir", &state.display().to_string(),
            "--repo", &repo.display().to_string(),
            "--rules", &dispatch_rules(&state),
            "--bd", client,
            "--lens", &review_lens_pass(&state),
            "--runner", "true",
        ])
    };
    let stopped = terminal(&bd);
    assert_eq!(stopped.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", stderr_of(&stopped));
    // **観測の面は増えない**（設計 §6）: 終端の stdout に列の行は出ず、効果だけが残る。
    assert!(!stdout_of(&stopped).contains("dispatch="), "終端は列の行を出さない: {}", stdout_of(&stopped));
    assert_eq!(created(&state, &["s2-toy.1"], 1), 1, "交差が解けた便が起こされる（RunCreated 1 件）");
    clean(&[&repo, &state]);
}

/// (§5 便の終端) 終端の中の 1 周が**失敗しても終端の rc は変わらない**（台帳を読めない周）。
#[test]
fn pipe_terminal_dispatch_keeps_the_terminal_rc_when_the_round_cannot_measure() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let live = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), "s2-live");
    let broken = script(&state.join("bd-dead"), "exit 1\n");
    let stopped = run_pipe(&[
        "stop", "--run", &live,
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &broken,
        "--runner", "true",
    ]);
    assert_eq!(stopped.status.code(), Some(i32::from(RC_OK)), "読めない台帳でも stop は rc 0: {}", stderr_of(&stopped));
    assert_eq!(created(&state, &["s2-toy.1"], 0), 0, "読めない周は 1 本も起こさない");
    // 終端そのものは通っている（便は止まっている）＝1 周の失敗が終端を巻き込んでいない。
    let again = run_pipe(&["stop", "--run", &live, "--state-dir", &state.display().to_string()]);
    assert_ne!(again.status.code(), Some(i32::from(RC_OK)), "既に終端ゆえ 2 度目は断られる");
    clean(&[&repo, &state]);
}

/// (§5 便の終端) `pipe run` の終端の 1 周は**その process 自身の道具**（`--rules` / `--lens` / `--runner`）で
/// 便を起こす。driver は自分の道具を知っているので、列に渡し直さなくても起こせる。
#[test]
fn pipe_terminal_dispatch_run_uses_its_own_tools_for_the_round() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    // 台帳の候補は行 a。`pipe run` が起こすのは行 b（交差しない）＝終端の 1 周で行 a が起きる。
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a")]);
    let ran = run_pipe(&[
        "run", "--design", &format!("{DESIGN_FILE}#b"), "--bead", "s2-own",
        "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &bd,
        "--lens", &review_lens_pass(&state),
        "--runner", "true",
    ]);
    // `pipe run` 自体の rc は問わない（toy の runner は何もしないので後段で止まる）。測るのは**終端の 1 周**である。
    assert!(!stdout_of(&ran).contains("dispatch="), "終端は列の行を出さない: {}", stdout_of(&ran));
    assert_eq!(created(&state, &["s2-toy.1"], 1), 1, "自分の道具で台帳の候補 1 本を起こす");
    clean(&[&repo, &state]);
}

/// (§5 便の終端 + §2 終端の便は列外) `pipe land` の終端の直後に列が 1 周撃たれ、**交差の外の候補が起こされる**
/// 一方で、**着地した便の bead は起こし直されない**（`settled`）。契約の行を改訂して sha が動けば列に戻る。
///
/// 着地から台帳を閉じるまでの間、終端が来るたびに同じ契約が起こし直される穴（`s2-07l.366` の自己レビュー）を
/// 両側で測る。母集団は `[DISPATCH-COUNT]` の `total=` で同時に出す。
#[test]
fn pipe_terminal_dispatch_land_starts_the_queue_without_restarting_the_landed_bead() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let design_a = format!("{DESIGN_FILE}#a");
    // 行 a の便を PASS の gate まで通す（bead は台帳の候補と同じ id にする）。
    let landing = intake_bead(&repo, &state, &design_a, "s2-toy.1");
    let spawned = run_pipe(&[
        "spawn", "--run", &landing, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&spawned));
    let lens = fake_lens(&state.join("gate-lens-ran"), &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &landing, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate は rc 0: {}", stderr_of(&gated));
    // 台帳の候補は 2 件: 着地する bead（行 a）と、交差しない bead（行 b）。
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a"), issue("s2-toy.2", 2, "b")]);
    let landed = run_pipe(&[
        "land", "--run", &landing, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &bd,
        "--lens", &review_lens_pass(&state),
        "--runner", "true",
    ]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&landed));
    // 終端の 1 周が撃たれた（行 b の候補が起きる）。
    assert_eq!(created(&state, &["s2-toy.2"], 1), 1, "land の終端の直後に交差の外の 1 本が起きる");
    // **着地した bead は起こし直されない**（RunCreated は着地した便の 1 件だけ）。
    assert_eq!(created(&state, &["s2-toy.1"], 0), 1, "着地した bead の便は 1 件のまま（起こし直さない）");
    let after = ls(&repo, &state, &bd);
    let reason = reason_of(&after, "s2-toy.1");
    assert!(reason.starts_with("settled:"), "着地した便の sha で列外: {}", stdout_of(&after));
    assert!(reason.ends_with("/Landed"), "段は Landed: {reason}");
    assert_eq!(count_of(&after), format!("{COUNT} total=2 ready=0"), "母集団 2 件・起こせる 0 本");
    // **契約の行を改訂して sha が動けば列に戻る**（着地した便は塞ぎ続けない）。
    write_design(
        &repo,
        &design_doc_rows(&[
            row_fields("a", &["write-set", "done"], &[r#"write-set = ["src/lib.rs"]"#, r#"done = "改訂した""#]),
            row_fields("b", &["write-set"], &[r#"write-set = ["src/b.rs"]"#]),
        ]),
    );
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "design-row-revised"]);
    let revised = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&revised, "s2-toy.1"), "-", "sha が動けば列に戻る: {}", stdout_of(&revised));
    clean(&[&repo, &state]);
}

/// (§5 起こす便へ渡す道具) 起こした子（`pipe run`）**自身も終端で 1 周撃つ**ので、列に渡した台帳 client も
/// そのまま渡る。落とすと子の 1 周が既定の台帳（PATH の `bd`）を読み、1 hop で列と食い違う。
///
/// 測り方: 偽の台帳を「呼ばれたら印を置く」形にし、**列の 1 周（1 回）と子の 1 周（1 回）で 2 回**呼ばれる
/// ことを見る。落ちていれば 1 回で止まる（子は PATH の `bd` を読む）。
#[test]
fn pipe_terminal_dispatch_hands_the_ledger_client_to_the_run_it_starts() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let json = state.join("ledger-counted.json");
    fs::write(&json, format!("[{}]\n", issue("s2-toy.2", 2, "b"))).expect("偽の台帳を書ける");
    let calls = state.join("bd-calls.log");
    let counted = script(
        &state.join("bd-counted"),
        &format!("printf 'x\\n' >> '{}'\ncat '{}'\n", calls.display(), json.display()),
    );
    let out = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &counted,
        "--lens", &review_lens_pass(&state),
        "--runner", "true",
    ]);
    assert_eq!(stdout_of(&out).trim_end(), "dispatch=started:1,resumed:0,waiting:0", "1 本起こす: {}", stderr_of(&out));
    assert_eq!(created(&state, &["s2-toy.2"], 1), 1, "起こした便の RunCreated");
    // 子の終端の 1 周が**同じ台帳**を読む＝印が 2 つ（列の 1 周 + 子の 1 周）。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let count = || fs::read_to_string(&calls).map(|text| text.lines().count()).unwrap_or_default();
    while count() < 2 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(count(), 2, "列の 1 周と子の 1 周で偽の台帳が 2 回呼ばれる（母集団 = 1 周 × 2 段）");
    clean(&[&repo, &state]);
}

/// (§5 二重起動) **契機が重なっても同じ bead は 1 本しか起きない**（SRS AC38・母集団 = 起動試行 2 回）。
///
/// 1 周は lock を取らない（子を起こす間じゅう着地の列と同じ lock を握らないため）ので、同時に来た 2 つの
/// 契機は同じ候補をどちらも起こそうとする。落とすのは**受付**である——同じ秒なら run id の衝突
/// （`DuplicateRun`）、秒を跨げば先の便と write-set が交差（`WriteSetOverlap`）。
#[test]
fn pipe_terminal_dispatch_two_overlapping_rounds_start_the_bead_once() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bd = fake_bd(&state, &[issue("s2-toy.2", 2, "b")]);
    let args: Vec<String> = [
        "pipe", "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &bd,
        "--lens", &review_lens_pass(&state),
        "--runner", "true",
    ]
    .iter()
    .map(|found| (*found).to_owned())
    .collect();
    // **同時に撃つ**（片方を待ってから撃つと 2 本目は交差で落ちるだけで、重なりを測れない）。
    let spawned: Vec<_> = (0..2)
        .filter_map(|_| Command::new(super::bin()).args(&args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok())
        .collect();
    assert_eq!(spawned.len(), 2, "2 つの契機を同時に撃つ");
    let started: usize = spawned
        .into_iter()
        .filter_map(|child| child.wait_with_output().ok())
        .filter(|out| stdout_of(out).contains("dispatch=started:1,resumed:0"))
        .count();
    assert!(started >= 1, "少なくとも一方の 1 周は起こす側に立つ（母集団 2 回）");
    assert_eq!(created(&state, &["s2-toy.2"], 1), 1, "起動試行 2 回でも便は 1 本（母集団 2 回）");
    clean(&[&repo, &state]);
}

/// (§5 driver の死亡 + 便の自走) `pipe run` の process を殺すと **driver の札が残り**、列の 1 周が
/// `pipe resume` で起こし直す（record token の `resumed:1`）。起こし直しが**実際に走った**ことは、その便に
/// `SeatStopped detail=runner-dead` が 1 件積まれることで測る（数えただけでは撃ったと言えない）。
///
/// **列が起こす便は `--drive` を持つ**（行 (e)・`s2-07l.485`）ので、起こし直しは 1 段で終わらず
/// `Landed` まで続く（AC38・`s2-07l.482` の実測で `Implemented` で止まった件の直し）。
#[test]
fn pipe_dispatch_drive_revives_a_dead_driver_all_the_way_to_landed() {
    let (repo, state) = repo_with_state();
    // `pipe run` を子として起こし、`SeatSpawned` の時点で group ごと殺す（host の再起動と同じ形）。
    let (id, _runner_pid, runner) = super::spawn::killed_at_spawned(&repo, &state, &[IMPLEMENT.to_owned()]);
    let ticket = state.join("pipe").join(&id).join("driver");
    assert!(ticket.exists(), "殺した driver の札が残る");
    // 台帳は空でよい（測るのは起こし直しであって列の入力ではない）。起こし直した便が Landed まで通る
    // ように、**列に渡す道具は toy の一式**にする（列はそれをそのまま `pipe resume` へ渡す）。
    let bd = fake_bd(&state, &[]);
    let rules = with_ledger_row(&state, "rules-driver.toml", &super::ratelimit::resume_rules(&state, &["a1"]));
    super::ratelimit::put_account(&state, "a1", &[super::ratelimit::windows(30, 30)]);
    let out = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &rules,
        "--bd", &bd,
        "--lens", &fake_lens(&state.join("driver-lens-ran"), &lens_verdict("PASS")),
        "--curl", &super::ratelimit::fake_usage_curl(&state),
        "--runner", &runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0: {}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out).trim_end(),
        "dispatch=started:0,resumed:1,waiting:0",
        "record token は起こし直しを 1 件数える: {}",
        stdout_of(&out)
    );
    // **数えただけでは撃ったと言えない**: 起こし直しが実際に走ると、死んだ runner の始末が記帳される。
    assert_eq!(dead_runners(&state, &id, 1), 1, "pipe resume が 1 回走る（SeatStopped detail=runner-dead）");
    // **1 段では終わらない**: 起こし直した driver は自分の便を次の driver へ渡し、便は着地まで進む。
    assert_eq!(stage_reached(&state, &id, "Implemented"), 1, "1 段目（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "自走で着地まで（段の並び: {}）", stages_of(&state, &id));
    // 便が着地したら、継いだ driver も抜けるときに**自分の札を外す**（札の寿命は変えていない）。
    assert!(gone(&ticket), "継ぎの driver も抜けるときに自分の札を外す");
    clean(&[&repo, &state]);
}

/// path が消えるまで待つ（上限 20s・消えなければ `false`）。
fn gone(path: &Path) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while path.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    !path.exists()
}

/// (§5 driver の死亡) **札の無い live 便は触らない**（`pipe intake` + `pipe spawn` で起こした便は driver の
/// 札を持たない＝`pipe run` / `pipe resume` の process が居ない）。測れないを「死んだ」に読み替えない。
#[test]
fn pipe_dispatch_driver_live_run_without_a_ticket_is_left_alone() {
    let (repo, state) = repo_with_state();
    let contract = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &contract);
    assert!(!state.join("pipe").join(&id).join("driver").exists(), "前提: 札を持たない live 便");
    let before = kind_count(&state, &id, vessel::fleet::EventKind::RunStage);
    let bd = fake_bd(&state, &[]);
    let out = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &bd,
        "--lens", &review_lens_pass(&state),
        "--runner", "true",
    ]);
    assert_eq!(
        stdout_of(&out).trim_end(),
        "dispatch=started:0,resumed:0,waiting:0",
        "札の無い便は起こし直さない: {}",
        stdout_of(&out)
    );
    assert_eq!(kind_count(&state, &id, vessel::fleet::EventKind::RunStage), before, "段を 1 つも動かさない");
    clean(&[&repo, &state]);
}

/// (§5 driver の死亡) `pipe run` は入口で札を置き、**終端で消す**（生きて終わった便の札は残らない＝
/// 残っている札は死んだ driver のものだけである）。
#[test]
fn pipe_dispatch_driver_ticket_is_removed_when_the_run_ends() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let ran = run_pipe(&[
        "run", "--design", &format!("{DESIGN_FILE}#a"), "--bead", "s2-ticket",
        "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--lens", &review_lens_pass(&state),
        "--runner", "true",
    ]);
    let id = stdout_of(&ran)
        .lines()
        .find_map(|line| line.strip_prefix("run="))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_default()
        .to_owned();
    assert!(!id.is_empty(), "run id を読める: {}", stdout_of(&ran));
    assert!(state.join("pipe").join(&id).exists(), "前提: run dir は在る");
    assert!(!state.join("pipe").join(&id).join("driver").exists(), "終端で札が消える");
    clean(&[&repo, &state]);
}

/// 便の段の並び（診断の 1 行）。
fn stages_of(state: &Path, id: &str) -> String {
    let found: Vec<String> = fs::read_to_string(state.join("fleet").join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains(&format!("\"run\":\"{id}\"")))
        .filter_map(|line| line.split("\"stage\":\"").nth(1).and_then(|rest| rest.split('"').next()).map(str::to_owned))
        .collect();
    found.join(" → ")
}

/// 便が `stage` の段に達した件数（いま数えるだけ・待たない）。
fn reached_now(state: &Path, id: &str, stage: &str) -> usize {
    fs::read_to_string(state.join("fleet").join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains(&format!("\"stage\":\"{stage}\"")))
        .filter(|line| line.contains(&format!("\"run\":\"{id}\"")))
        .count()
}

/// 便が `stage` の段に達した件数（達するまで待つ・起こし直しは子 process ゆえ遅れて来る）。
fn stage_reached(state: &Path, id: &str, stage: &str) -> usize {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    while reached_now(state, id, stage) == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    reached_now(state, id, stage).min(1)
}

/// 便が `stage` の段に**達していない**ことを測る（**少し待ってから数える**）。
///
/// 達するのを待つ [`stage_reached`] をそのまま使うと上限いっぱい（90 秒）待ってから 0 を返す。
/// かといって即座に数えると、遅れて来た 1 件を「来なかった」と読み違える（C10）。
fn not_reached(state: &Path, id: &str, stage: &str) -> usize {
    std::thread::sleep(std::time::Duration::from_millis(2000));
    reached_now(state, id, stage)
}

/// 便の `SeatStopped detail=runner-dead` の件数（`want` 件まで待つ・起こし直しは子 process ゆえ遅れて来る）。
fn dead_runners(state: &Path, id: &str, want: usize) -> usize {
    let log = state.join("fleet").join("events.jsonl");
    let count = || -> usize {
        fs::read_to_string(&log).unwrap_or_default()
            .lines()
            .filter(|line| line.contains("\"kind\":\"SeatStopped\"") && line.contains("\"detail\":\"runner-dead\""))
            .filter(|line| line.contains(&format!("\"run\":\"{id}\"")))
            .count()
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while count() < want && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    count()
}

/// (§5 driver の死亡) **人の手を待つ段は起こし直しの候補から段で外す**（`Blocked` は `pipe resume` が
/// rc 3 で何もしない段なので、起こし直すと空撃ちになる）。札は**残す**——消すと、承認が記帳された後に
/// driver の居ない live 便が「札の無い便＝触らない」に落ちて二度と自走しない（planner 裁定 2026-09-19）。
#[test]
fn pipe_dispatch_driver_blocked_run_is_excluded_by_stage_and_keeps_its_ticket() {
    let (repo, state) = repo_with_state();
    let marker = state.join("blocked-runner-ran");
    let (id, _said) = super::spawn::blocked(&repo, &state, &marker, r#"classes = ["publish"]"#);
    // 承認待ちの便に**死んだ所有者の札**を置く（`pipe run` が承認待ちまで進めて抜けた形）。
    put_dead_ticket(&state, &id);
    let ticket = state.join("pipe").join(&id).join("driver");
    let bd = fake_bd(&state, &[]);
    let out = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &bd,
        "--lens", &review_lens_pass(&state),
        "--runner", "true",
    ]);
    assert_eq!(
        stdout_of(&out).trim_end(),
        "dispatch=started:0,resumed:0,waiting:0",
        "承認待ちの便は起こし直さない: {}",
        stdout_of(&out)
    );
    assert!(ticket.exists(), "札は残る（承認の後に自走へ戻るため）");
    assert_eq!(stages_of(&state, &id), "Intake → Reviewed → Blocked", "段を 1 つも動かさない");
    clean(&[&repo, &state]);
}

/// 便に**死んだ所有者の札**を置く（`true` を起こして待ち、その pid を書く＝確実に居ない process）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_dead_ticket(state: &Path, id: &str) {
    let mut child = Command::new("true").spawn().expect("true を起こせる");
    let pid = child.id();
    child.wait().expect("true を待てる");
    let path = state.join("pipe").join(id).join("driver");
    fs::write(&path, format!("{pid}\n")).expect("札を書ける");
}


/// (§5 印の直後) `first` / `release` の記録の直後にも 1 周が撃たれ、`hold` は撃たない。
///
/// **便を 1 本も起こさずに測る**（`s2-07l.487`・行 g）: 台帳の候補が live な便と交差する形にすれば、
/// 1 周は必ず `started:0` で、子 process が 1 つも生まれない。起こしてから印を打つ形（`.366` の元の歯）
/// は、**走っている子の状態に依存**する——`s2-07l.486` の merge sha で CI が 1 度赤くなり、同じ sha の
/// 再走では緑・ローカルの負荷では両側 0/20 で、原因を特定できなかった（台帳 notes）。
#[test]
fn pipe_terminal_dispatch_marks_fire_without_children() {
    // flip-check: retroactive s2-07l.487
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    // 行 a を持つ live な便を 1 本置く＝台帳の候補（行 a）は必ず交差して起こせない。
    let live = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), "s2-live");
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a")]);
    let turn = |extra: &[&str]| -> Output {
        let mut args: Vec<String> = vec!["dispatch".to_owned()];
        args.extend(extra.iter().map(|found| (*found).to_owned()));
        args.extend([
            "--state-dir".to_owned(),
            state.display().to_string(),
            "--repo".to_owned(),
            repo.display().to_string(),
            "--rules".to_owned(),
            dispatch_rules(&state),
            "--bd".to_owned(),
            bd.clone(),
            "--runner".to_owned(),
            "true".to_owned(),
        ]);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        run_pipe(&borrowed)
    };
    let waiting = "dispatch=started:0,resumed:0,waiting:1";
    let manual = turn(&[]);
    assert_eq!(stdout_of(&manual).trim_end(), waiting, "交差する候補は起こせない（{}）", told(&manual));
    // `first` の記録の直後に 1 周（印の行 → 1 周の行の 2 行）。
    let first = turn(&["first", "s2-toy.1"]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "first は rc 0（{}）", told(&first));
    assert_eq!(stdout_of(&first).lines().last(), Some(waiting), "first の直後に 1 周（{}）", told(&first));
    // `hold` は起こす側を増やさないので 1 周を撃たない（印の行だけ）。
    let held = turn(&["hold", "s2-toy.1"]);
    assert_eq!(stdout_of(&held).lines().count(), 1, "hold は印の行だけ（{}）", told(&held));
    assert!(!stdout_of(&held).contains("dispatch="), "hold は 1 周を撃たない（{}）", told(&held));
    // `release` の記録の直後にも 1 周。
    let released = turn(&["release", "s2-toy.1"]);
    assert_eq!(released.status.code(), Some(i32::from(RC_OK)), "release は rc 0（{}）", told(&released));
    assert_eq!(stdout_of(&released).lines().last(), Some(waiting), "release の直後に 1 周（{}）", told(&released));
    // **子 process は 1 つも生まれない**（母集団 = 撃った 1 周 4 回）。
    assert_eq!(created(&state, &["s2-toy.1"], 0), 0, "便を 1 本も起こさない（1 周 4 回）");
    assert_eq!(kind_count(&state, &live, vessel::fleet::EventKind::RunCreated), 1, "live な便は元の 1 件のまま");
    clean(&[&repo, &state]);
}

/// 自走の 1 行（`dispatch=…` の行のうち `drive=` を持つ最後の 1 本の `drive=` の値）。
fn drive_of(out: &Output) -> String {
    stdout_of(out)
        .lines()
        .filter(|line| line.starts_with("dispatch="))
        .filter_map(|line| line.split("drive=").nth(1).map(str::to_owned))
        .next_back()
        .unwrap_or_default()
}

/// (§5 便の自走) `--drive` を持つ `pipe run` は toy repo の契約 1 本を偽 runner と偽 lens で
/// **人の手なしに** `Landed` まで通す。
#[test]
fn pipe_dispatch_drive_run_lands_a_toy_contract_without_hands() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let lens = fake_lens(&state.join("drive-lens-ran"), &lens_verdict("PASS"));
    let bd = fake_bd(&state, &[]);
    let out = run_pipe(&[
        "run",
        "--design", &format!("{DESIGN_FILE}#a"),
        "--bead", "s2-toy.1",
        "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &bd,
        "--lens", &lens,
        "--runner", IMPLEMENT,
        "--drive",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "自走の run は rc 0（{}）", told(&out));
    let id = run_id_of(&out);
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "段の列: {}", stages_of(&state, &id));
    assert_eq!(drive_of(&out), "settled", "着地した周は渡さない（{}）", told(&out));
    clean(&[&repo, &state]);
}

/// `pipe resume` を 1 回撃つ（道具は toy の一式・`--drive` は呼び手が選ぶ）。
fn resume_once(repo: &Path, state: &Path, id: &str, lens: &str, drive: bool) -> Output {
    let mut args: Vec<String> = ["resume", "--run", id]
        .iter()
        .map(|item| (*item).to_owned())
        .collect();
    args.extend([
        "--repo".to_owned(), repo.display().to_string(),
        "--state-dir".to_owned(), state.display().to_string(),
        "--rules".to_owned(), dispatch_rules(state),
        "--bd".to_owned(), fake_bd(state, &[]),
        "--lens".to_owned(), lens.to_owned(),
        "--runner".to_owned(), IMPLEMENT.to_owned(),
    ]);
    if drive {
        args.push("--drive".to_owned());
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_pipe(&borrowed)
}

/// (§5 便の自走) `--drive` を持つ `pipe resume` は 1 段進めた後に**自分の便を次の driver へ渡し**、
/// 便は `Landed` まで通る。**flag の無い周は今までどおり 1 段で止まり、行に `drive=` は載らない**
/// （段を手で 1 つずつ進める既存の歯の行は 1 byte も変わらない）。
///
/// 2 つの置き場で同じ段から A/B する（同じ置き場だと 1 本目の着地が 2 本目の交差を動かす）。
#[test]
fn pipe_dispatch_drive_resume_hands_off_only_with_the_flag() {
    let stopped = {
        let (repo, state) = repo_with_state();
        let contract = write_contract(&repo, &[], &[]);
        let id = implemented(&repo, &state, &contract);
        let lens = fake_lens(&state.join("drive-off-lens"), &lens_verdict("PASS"));
        let out = resume_once(&repo, &state, &id, &lens, false);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "flag 無しの resume は rc 0（{}）", told(&out));
        assert!(!stdout_of(&out).contains("drive="), "flag の無い周に token は載らない（{}）", told(&out));
        assert_eq!(stage_reached(&state, &id, "Gated"), 1, "1 段だけ進む（段の並び: {}）", stages_of(&state, &id));
        let landed = not_reached(&state, &id, "Landed");
        clean(&[&repo, &state]);
        landed
    };
    assert_eq!(stopped, 0, "flag の無い周は渡さない＝着地しない");
    let (repo, state) = repo_with_state();
    let contract = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &contract);
    let lens = fake_lens(&state.join("drive-on-lens"), &lens_verdict("PASS"));
    let out = resume_once(&repo, &state, &id, &lens, true);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "自走の resume は rc 0（{}）", told(&out));
    assert_eq!(drive_of(&out), "pass", "1 段進めた周は渡す（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "継ぎの子が着地させる（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§5 渡す周と渡さない周) 渡さなかった周は**理由を名乗る**（C10・黙って止まらない）。
///
/// `waiting` = 人の手を待つ段に着いた周（承認待ち）・`no-progress` = 段が動かなかった周
/// （`Gated` の INCONCLUSIVE は測り直さずに止まる）。`settled` は着地の歯が測る。
#[test]
fn pipe_dispatch_drive_names_why_it_did_not_hand_off() {
    let waiting = {
        let (repo, state) = repo_with_state();
        commit_rows(&repo, &[row_fields("a", &["write-set"], &[r#"write-set = ["src/lib.rs"]"#, r#"classes = ["publish"]"#])]);
        let out = run_pipe(&[
            "run",
            "--design", &format!("{DESIGN_FILE}#a"),
            "--bead", "s2-toy.1",
            "--repo", &repo.display().to_string(),
            "--state-dir", &state.display().to_string(),
            "--rules", &dispatch_rules(&state),
            "--bd", &fake_bd(&state, &[]),
            "--lens", &fake_lens(&state.join("wait-lens"), &lens_verdict("PASS")),
            "--runner", IMPLEMENT,
            "--drive",
        ]);
        let id = run_id_of(&out);
        assert_eq!(reached_now(&state, &id, "Blocked"), 1, "承認待ちで止まる（段の並び: {}）", stages_of(&state, &id));
        let drive = drive_of(&out);
        assert_eq!(not_reached(&state, &id, "Gated"), 0, "渡していないので先へ進まない（{}）", told(&out));
        clean(&[&repo, &state]);
        drive
    };
    assert_eq!(waiting, "waiting", "人の手を待つ段に着いた周は渡さない");
    let (repo, state) = repo_with_state();
    let contract = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &contract);
    let unsure = fake_lens(&state.join("unsure-lens"), &lens_verdict("INCONCLUSIVE"));
    let gated = gate_once(&repo, &state, &id, Some(&unsure));
    assert_eq!(reached_now(&state, &id, "Gated"), 1, "前提: Gated（{}）", told(&gated));
    let out = resume_once(&repo, &state, &id, &unsure, true);
    assert_eq!(drive_of(&out), "no-progress", "段が動かなかった周は渡さない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "渡していないので着地しない（{}）", told(&out));
    clean(&[&repo, &state]);
}

// ───── 関門が開いた待ちの便の再開（行 (j)・設計 §13・`pipe_dispatch_waiting_gate_` 接頭辞） ─────
//
// 回答済みの `Questioned` と承認済みの `Blocked` は段が待ちのままで札も無い（正常に抜けた driver は札を外す）ので、
// §5 の起こし直しでは候補に戻らなかった（実測 2026-09-20: 回答の後に手動の 1 周を撃っても `resumed:0`）。
// 列は関門の判定を resume の入口と同じ述語 1 本で撃ち、driver が居ないと測れた便を `--drive` 付きの resume で起こす。

/// 列と回答・承認に渡す道具の一式（toy の偽 runner と偽 lens・起こし直した便が `Landed` まで通る形）。
fn toy_tools(repo: &Path, state: &Path) -> Vec<String> {
    vec![
        "--repo".to_owned(), repo.display().to_string(),
        "--state-dir".to_owned(), state.display().to_string(),
        "--rules".to_owned(), dispatch_rules(state),
        "--bd".to_owned(), fake_bd(state, &[]),
        "--lens".to_owned(), fake_lens(&state.join("waiting-lens-ran"), &lens_verdict("PASS")),
        "--runner".to_owned(), IMPLEMENT.to_owned(),
    ]
}

/// `pipe <verb> …` を道具付きで撃つ（`head` は道具の前に置く引数）。
fn with_tools(head: &[&str], tools: &[String]) -> Output {
    let mut args: Vec<String> = head.iter().map(|item| (*item).to_owned()).collect();
    args.extend(tools.iter().cloned());
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_pipe(&borrowed)
}

/// 手動の 1 周（権能なしの口・道具は toy の一式）。
fn waiting_turn(repo: &Path, state: &Path) -> Output {
    with_tools(&["dispatch"], &toy_tools(repo, state))
}

/// 回答を 1 回撃つ（`extra` は置き場の後ろに足す引数＝道具の有無を呼び手が選ぶ）。
fn answer(state: &Path, id: &str, words: &str, extra: &[String]) -> Output {
    let mut head: Vec<String> = ["answer", "--run", id, "--words", words, "--state-dir"]
        .iter()
        .map(|item| (*item).to_owned())
        .collect();
    head.push(state.display().to_string());
    head.extend(extra.iter().cloned());
    let borrowed: Vec<&str> = head.iter().map(String::as_str).collect();
    run_pipe(&borrowed)
}

/// 行 `row` の便を bead 名つきで質問に倒す（`questioned` の bead を選べる形・同じ置き場に 2 便を置く歯が使う）。
fn questioned_bead(repo: &Path, state: &Path, row: &str, bead: &str) -> String {
    let id = intake_bead(repo, state, &format!("{DESIGN_FILE}#{row}"), bead);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &question_runner(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "質問は rc 3 で止まる（{}）", told(&out));
    id
}

/// 便の driver の札に本文を書く（dir は run dir・在る前提）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_ticket_body(state: &Path, id: &str, body: &str) {
    fs::write(state.join("pipe").join(id).join("driver"), body).expect("札を書ける");
}

/// 便の `Spawned` の記帳の件数（いま数えるだけ）。質問で止まった便は 1 件で、起こし直されると 2 件になる。
fn spawned_now(state: &Path, id: &str) -> usize {
    reached_now(state, id, "Spawned")
}

/// 1 周の行の期待値（起こし直しの本数だけ呼び手が選ぶ）。
fn resumed_line(count: usize) -> String {
    format!("dispatch=started:0,resumed:{count},waiting:0")
}

/// 便の `QuestionAnswered` が 1 件在るか（記帳が成った証拠）。
fn answered_once(state: &Path, id: &str) -> bool {
    kind_count(state, id, vessel::fleet::EventKind::QuestionAnswered) == 1
}

/// (§13) 回答済みの `Questioned` の便（driver の札なし）は手動の 1 周で **`--drive` 付きの resume** で起こされ
/// （`resumed:1`）、先の段へ進んで人の手なしに `Landed` まで通る。base は札の無い便を触らない（RED）。
#[test]
fn pipe_dispatch_waiting_gate_answered_question_is_resumed_with_drive() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    assert!(!state.join("pipe").join(&id).join("driver").exists(), "前提: 正常に抜けた driver は札を外している");
    let answered = answer(&state, &id, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&answered));
    let out = waiting_turn(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "回答済みの便を 1 本起こし直す（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Implemented"), 1, "先の段へ進む（段の並び: {}）", stages_of(&state, &id));
    // **`--drive` 付き**である証拠: 1 段で止まらず、継ぎの driver が着地まで通す。
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "自走で着地まで（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§13) 未回答の `Questioned` の便は起こされない（`resumed:0`・段は 1 つも動かない）。
#[test]
fn pipe_dispatch_waiting_gate_unanswered_question_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let out = waiting_turn(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "関門が閉じた便は起こさない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Implemented"), 0, "段は動かない（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(spawned_now(&state, &id), 1, "起こし直していない（Spawned は初回の 1 件）");
    clean(&[&repo, &state]);
}

/// (§13) 古い質問に回答が在っても**最新の**質問が未回答なら関門は閉じている（`resumed:0`）。
#[test]
fn pipe_dispatch_waiting_gate_newest_question_unanswered_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let answered = answer(&state, &id, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "1 つ目の回答は rc 0（{}）", told(&answered));
    // 2 つ目の質問で止まる runner で手で resume する（`--drive` は無い＝1 段で止まる）。
    let second = "printf '%s\\n' '{\"question\":\"write-set の外を触ってよいか\"}'; exit 76";
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", second,
        "--bd", &fake_bd(&state, &[]),
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_BLOCKED)), "2 つ目の質問で止まる（{}）", told(&resumed));
    assert_eq!(reached_now(&state, &id, "Questioned"), 2, "前提: 質問は 2 件（段の並び: {}）", stages_of(&state, &id));
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "最新の質問が未回答なら起こさない（{}）", told(&out));
    assert_eq!(spawned_now(&state, &id), 2, "起こし直していない（Spawned は手の 2 件のまま）");
    clean(&[&repo, &state]);
}

/// (§13) 承認済みの `Blocked` の便も同じく起こされ、先の段へ進む（`resumed:1`）。
#[test]
fn pipe_dispatch_waiting_gate_approved_blocked_is_resumed() {
    let (repo, state) = repo_with_state();
    let marker = state.join("blocked-runner-ran");
    let (id, _said) = super::spawn::blocked(&repo, &state, &marker, r#"classes = ["publish"]"#);
    let approved = run_pipe(&["approve", "--run", &id, "--words", "出してよい", "--state-dir", &state.display().to_string()]);
    assert_eq!(approved.status.code(), Some(i32::from(RC_OK)), "承認は rc 0（{}）", told(&approved));
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "承認済みの便を 1 本起こし直す（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Implemented"), 1, "先の段へ進む（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "自走で着地まで（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§13 札の 4 値) **所有者が死んでいる**札の便は起こす（`pipe run` が回答待ちまで進めて死んだ形）。
#[test]
fn pipe_dispatch_waiting_gate_dead_ticket_is_resumed() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let answered = answer(&state, &id, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&answered));
    put_dead_ticket(&state, &id);
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "死んだ所有者の札の便は起こす（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "自走で着地まで（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§13 札の 4 値) **所有者が生きている**札の便は触らない（別の driver が駆動している便に 2 本目を立てない）。
#[test]
fn pipe_dispatch_waiting_gate_live_ticket_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let answered = answer(&state, &id, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&answered));
    // 歯の process 自身の pid＝確実に生きている所有者。
    put_ticket_body(&state, &id, &format!("{}\n", std::process::id()));
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "生きている所有者の札の便は触らない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Implemented"), 0, "段は動かない（段の並び: {}）", stages_of(&state, &id));
    assert!(state.join("pipe").join(&id).join("driver").exists(), "札は奪わない");
    clean(&[&repo, &state]);
}

/// (§13 札の 4 値) **在るのに読めない**札の便は触らない（測れないを「居ない」に読み替えない・fail-closed）。
#[test]
fn pipe_dispatch_waiting_gate_unreadable_ticket_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let answered = answer(&state, &id, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&answered));
    put_ticket_body(&state, &id, "not-a-pid\n");
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "読めない札の便は触らない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Implemented"), 0, "段は動かない（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(spawned_now(&state, &id), 1, "起こし直していない");
    clean(&[&repo, &state]);
}

/// (§13 契機) 道具を渡した `pipe answer` は記帳の直後に同じ 1 周を撃ち、便が進む。**stdout は記帳の 1 行だけ**
/// （1 周の行は足さない・終端の 1 周と同じ黙る形）。
#[test]
fn pipe_dispatch_waiting_gate_answer_with_tools_fires_a_turn_silently() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let out = answer(&state, &id, "verify は 1 行目だけを撃つ", &toy_tools(&repo, &state));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out), format!("run={id} answered=true\n"), "stdout は記帳の 1 行だけ（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Implemented"), 1, "記帳の直後の 1 周が便を進める（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "自走で着地まで（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§13 契機) 道具を渡した `pipe approve` も記帳の直後に同じ 1 周を撃ち、便が進む（stdout は記帳の 1 行だけ）。
#[test]
fn pipe_dispatch_waiting_gate_approve_with_tools_fires_a_turn_silently() {
    let (repo, state) = repo_with_state();
    let marker = state.join("blocked-runner-ran");
    let (id, _said) = super::spawn::blocked(&repo, &state, &marker, r#"classes = ["publish"]"#);
    let out = with_tools(&["approve", "--run", &id, "--words", "出してよい"], &toy_tools(&repo, &state));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "承認は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out), format!("run={id} approved=true\n"), "stdout は記帳の 1 行だけ（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Implemented"), 1, "記帳の直後の 1 周が便を進める（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "自走で着地まで（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§13 契機) 道具を渡さない `pipe answer` は今までどおり記帳だけで rc 0（便は次の契機まで待つ）。
#[test]
fn pipe_dispatch_waiting_gate_answer_without_tools_only_records() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let before = kind_count(&state, &id, vessel::fleet::EventKind::RunStage);
    let out = answer(&state, &id, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out), format!("run={id} answered=true\n"), "stdout は記帳の 1 行だけ（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Implemented"), 0, "便は進まない（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(kind_count(&state, &id, vessel::fleet::EventKind::RunStage), before, "段の記帳は増えない");
    assert!(answered_once(&state, &id), "記帳は成っている");
    clean(&[&repo, &state]);
}

/// (§13 契機) 記帳の直後の 1 周が失敗しても（台帳を読めない周）回答の rc は変わらず、stdout も記帳の 1 行だけ。
#[test]
fn pipe_dispatch_waiting_gate_failed_turn_keeps_the_answer_rc() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    // 台帳 client を rc 1 で落ちる script に差し替える＝1 周は `unmeasured` で 1 本も起こさない。
    let broken = script(&state.join("bd-broken"), "exit 1\n");
    let tools: Vec<String> = toy_tools(&repo, &state)
        .into_iter()
        .map(|item| if item.ends_with("/bd") { broken.clone() } else { item })
        .collect();
    let out = answer(&state, &id, "verify は 1 行目だけを撃つ", &tools);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周が失敗しても回答は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out), format!("run={id} answered=true\n"), "stdout は記帳の 1 行だけ（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Implemented"), 0, "測れない周は起こさない（段の並び: {}）", stages_of(&state, &id));
    assert!(answered_once(&state, &id), "記帳は成っている（回答の逐語が残る）");
    clean(&[&repo, &state]);
}

/// (§13) 段を前へ進めた driver の終端の 1 周は、別の回答済みの便を起こす（`resumed:1`）。
///
/// 便 B を `Gated` まで手で進め、`--drive` の resume で着地させる（`Gated` → `Landed` は前進・自分の便は
/// 終端ゆえ渡さない）。その終端の 1 周が、同じ置き場で回答を待っていた便 A を起こし直す。
#[test]
fn pipe_dispatch_waiting_gate_forward_driver_turn_resumes_another_answered_run() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    // A: 行 a（`src/lib.rs`）の便を質問で止めて回答する（道具なし＝記帳だけ・札は無い）。
    let asked = questioned_bead(&repo, &state, "a", "s2-toy.1");
    let answered = answer(&state, &asked, "verify は 1 行目だけを撃つ", &[]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "回答は rc 0（{}）", told(&answered));
    // B: 行 b（`src/b.rs`・A と交差しない）の便を Gated まで人の手で進める。
    let other = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#b"), "s2-toy.2");
    let spawned = run_pipe(&[
        "spawn", "--run", &other, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/b.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "B の spawn は rc 0（{}）", told(&spawned));
    let lens = fake_lens(&state.join("forward-lens-ran"), &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &other, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "B の gate は rc 0（{}）", told(&gated));
    assert_eq!(spawned_now(&state, &asked), 1, "前提: A はまだ起こし直されていない");
    // B の driver（`--drive`）が Gated → Landed と段を前へ進め、終端の 1 周で A を起こす。
    let mut tools = toy_tools(&repo, &state);
    tools.push("--drive".to_owned());
    let out = with_tools(&["resume", "--run", &other], &tools);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "B の resume は rc 0（{}）", told(&out));
    assert_eq!(
        stdout_of(&out).lines().last(),
        Some(format!("{} drive=settled", resumed_line(1)).as_str()),
        "終端の 1 周が A を 1 本起こす（{}）",
        told(&out)
    );
    assert_eq!(stage_reached(&state, &other, "Landed"), 1, "B は着地（段の並び: {}）", stages_of(&state, &other));
    assert_eq!(stage_reached(&state, &asked, "Implemented"), 1, "A が先の段へ進む（段の並び: {}）", stages_of(&state, &asked));
    assert_eq!(stage_reached(&state, &asked, "Landed"), 1, "A も自走で着地まで（段の並び: {}）", stages_of(&state, &asked));
    clean(&[&repo, &state]);
}
