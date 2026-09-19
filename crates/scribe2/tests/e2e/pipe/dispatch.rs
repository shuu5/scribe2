//! 列（`pipe dispatch`）の歯（設計 docs/design/dispatcher.md §8・契約表の行 a）。
//!
//! 偽の台帳（`bd --readonly list --json` の出力を返す script）と偽の live 便（`pipe intake` で起こした本物の
//! 便）で、**列の入力・順序・起動条件・介入の印**を外形から測る。順序の 1 関数と印の畳み込みは pure ゆえ
//! `pipe/dispatch.rs` の in-file の歯が持つ（同じ接頭辞 `pipe_dispatch_`）。

use super::{
    ceiling_rules, clean, commit_rows, design_doc_rows, git, intake_bead, repo_with_state, row_fields, run_pipe,
    stderr_of, stdout_of, write_design, DESIGN_FILE,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Output;
use vessel::cli_outcome::RC_OK;

/// 列の 1 行の書き出し（器の字面を借りない＝外形を測る側は自分で書く）。
const LINE: &str = "[DISPATCH]";

/// 件数の行の書き出し。
const COUNT: &str = "[DISPATCH-COUNT]";

/// 台帳を読めなかった周の行。
const UNMEASURED: &str = "[DISPATCH-UNMEASURED reason=ledger]";

/// 審査の判定 file（run dir の直下）。
const REVIEW_FILE: &str = "review.json";

/// 台帳の 1 件（`bd --readonly list --json` の要素・読み手が読む key だけ）。
fn issue(id: &str, priority: u64, row: &str) -> String {
    format!(
        "{{\"id\":\"{id}\",\"status\":\"open\",\"priority\":{priority},\"labels\":[],\
         \"acceptance_criteria\":\"design = {DESIGN_FILE}#{row}\",\"dependencies\":[]}}"
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
    let row = "[[rule]]\nid = \"seat.ledger_timeout_s\"\nkind = \"LedgerTimeoutS\"\n\
               value = 60\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n";
    let path = state.join("rules-dispatch.toml");
    fs::write(&path, format!("{base}\n{row}")).expect("列の写しを書ける");
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

/// `dispatch ls` の 1 行のうち `bead=<id>` のものの `reason=` の値。
fn reason_of(out: &Output, bead: &str) -> String {
    stdout_of(out)
        .lines()
        .filter(|line| line.starts_with(LINE))
        .find(|line| line.contains(&format!("bead={bead} ")))
        .and_then(|line| line.split("reason=").nth(1).map(str::to_owned))
        .unwrap_or_default()
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
    // 同じ置き場・同じ行で**読める台帳**を渡すと件数が出る＝上の 1 行は「列が空」ではない。
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
    mark("first", "s2-toy.1");
    let first = ls(&repo, &state, &bd);
    assert_eq!(beads(&first), vec!["s2-toy.1", "s2-toy.2"], "first は P0 より先: {}", stdout_of(&first));
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

/// (§8 審査 FAIL の列外) 同じ契約 file の sha で審査に落ちた便が在る契約は `review-failed` で列外になり、
/// 設計 doc の行が変わって sha が動くと列に戻る。
#[test]
fn pipe_dispatch_keeps_a_review_failed_contract_out_until_its_sha_moves() {
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
    assert!(reason.starts_with("review-failed:"), "同じ sha では列外: {}", stdout_of(&out));
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
