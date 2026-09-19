//! 列（`pipe dispatch`）の歯（設計 docs/design/dispatcher.md §8・契約表の行 a）。
//!
//! 偽の台帳（`bd --readonly list --json` の出力を返す script）と偽の live 便（`pipe intake` で起こした本物の
//! 便）で、**列の入力・順序・起動条件・介入の印**を外形から測る。順序の 1 関数と印の畳み込みは pure ゆえ
//! `pipe/dispatch.rs` の in-file の歯が持つ（同じ接頭辞 `pipe_dispatch_`）。

use super::{
    ceiling_rules, clean, commit_rows, design_doc_rows, git, intake_bead, repo_with_state, review_lens_pass,
    row_fields, run_pipe, shim_path, stderr_of, stdout_of, stop_run_ok, write_design, DESIGN_FILE,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
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
    mark("first", "s2-toy.1");
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

/// (§2 審査 FAIL の列外・裏側) 判定の鍵は**現在の契約 file の sha**である: 同じ sha の直前の便が PASS で
/// 終わっていれば、それより**古い**同じ sha の FAIL は列を塞がない。
///
/// `run id = <bead>-<UTC の秒>` なので 2 便は別の秒に起こす（同じ秒だと id が衝突して 2 本目が断られる）。
#[test]
fn pipe_dispatch_review_failed_reads_the_run_just_before_the_current_sha() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let design = format!("{DESIGN_FILE}#a");
    let older = intake_bead(&repo, &state, &design, "s2-toy.1");
    fs::write(state.join("pipe").join(&older).join(REVIEW_FILE), "{\"verdict\":\"FAIL\"}\n")
        .expect("古い便の審査の判定を書ける");
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a")]);
    let blocked = ls(&repo, &state, &bd);
    assert!(
        reason_of(&blocked, "s2-toy.1").starts_with("review-failed:"),
        "古い便しか無い周は列外: {}",
        stdout_of(&blocked)
    );
    // 同じ契約でもう 1 便（秒を跨ぐ）。こちらは審査 PASS のまま終端にする。
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let newer = intake_bead(&repo, &state, &design, "s2-toy.1");
    assert_ne!(newer, older, "2 便は別の run id");
    stop_run_ok(&state, &newer);
    let out = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&out, "s2-toy.1"), "-", "直前の便が PASS なら古い FAIL は塞がない: {}", stdout_of(&out));
    assert_eq!(count_of(&out), format!("{COUNT} total=1 ready=1"), "起こせる 1 本");
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

/// (§5 手動の 1 周・印の直後) subcommand の無い `pipe dispatch` は 1 周を撃ってその結果を 1 行で返し、
/// `first` / `release` の記録の直後にも同じ 1 周が撃たれる。`hold` は起こす側を増やさないので撃たない。
#[test]
fn pipe_terminal_dispatch_manual_turn_and_marks_fire_the_same_round() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a"), issue("s2-toy.2", 0, "b")]);
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
            // 起こした便の審査は**偽 PASS の lens**で通す（既定の lens は実 claude を起こす）。
            "--lens".to_owned(),
            review_lens_pass(&state),
            // 実装役は偽の 1 行（器は runner の既定を持たない）。
            "--runner".to_owned(),
            "true".to_owned(),
        ]);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        run_pipe(&borrowed)
    };
    let manual = turn(&[]);
    assert_eq!(manual.status.code(), Some(i32::from(RC_OK)), "手動の 1 周は rc 0: {}", stderr_of(&manual));
    assert_eq!(stdout_of(&manual).trim_end(), "dispatch=started:2,waiting:0", "交差しない 2 本は両方起こせる");
    // **起こした効果**: 起こした 2 本ぶんの `RunCreated` が置き場に積まれる（構築点で止まらない・裁定 (A)）。
    assert_eq!(created(&state, &["s2-toy.1", "s2-toy.2"], 2), 2, "起こした便の RunCreated が 2 件");
    // `hold` は 1 周を撃たない（印の行だけ）。
    let held = turn(&["hold", "s2-toy.1"]);
    assert_eq!(stdout_of(&held).lines().count(), 1, "hold は印の行だけ: {}", stdout_of(&held));
    assert!(!stdout_of(&held).contains("dispatch=started"), "hold は 1 周を撃たない");
    // `release` は印を外した直後に 1 周を撃つ＝2 行目に結果が出る。**起こす本数は 0 でよい**
    // （最初の 1 周で起こした 2 便が live で、同じ契約は自分の便と交差する）＝測るのは「撃たれたか」である。
    let released = turn(&["release", "s2-toy.1"]);
    assert_eq!(
        stdout_of(&released).lines().last(),
        Some("dispatch=started:0,waiting:2"),
        "release の直後に 1 周（起こした 2 便と交差して 0 本）: {}",
        stdout_of(&released)
    );
    assert_eq!(created(&state, &["s2-toy.1", "s2-toy.2"], 2), 2, "2 周目は便を増やさない");
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
    assert_eq!(stdout_of(&unmeasured).trim_end(), "dispatch=unmeasured reason=ledger", "読めない周は理由");
    // **実装役の口が無い周は台帳も読まない**（起こせないと分かっている＝理由が別の値・C10）。
    let no_runner = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &bd,
    ]);
    assert_eq!(stdout_of(&no_runner).trim_end(), "dispatch=unmeasured reason=no-runner", "runner が無い周");
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
