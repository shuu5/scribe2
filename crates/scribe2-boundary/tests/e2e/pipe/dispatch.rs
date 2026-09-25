//! 列（`pipe dispatch`）の歯（設計 docs/design/dispatcher.md §8・契約表の行 a）。
//!
//! 偽の台帳（`bd --readonly list --json` の出力を返す script）と偽の live 便（`pipe intake` で起こした本物の
//! 便）で、**列の入力・順序・起動条件・介入の印**を外形から測る。順序の 1 関数と印の畳み込みは pure ゆえ
//! `pipe/dispatch.rs` の in-file の歯が持つ（同じ接頭辞 `pipe_dispatch_`）。

use super::{
    ceiling_rules, clean, commit_rows, design_doc_rows, fake_lens, gate_once, git, implemented, intake_bead,
    kind_count, lens_verdict, question_runner, questioned, repo_with_state, review_lens_pass, row_fields, run_pipe,
    shim_path, stderr_of, run_id_of, stdout_of, value_of, verdict_pairs, worktree_of, write_contract, write_design,
    DESIGN_FILE, HEALTH_PER_CORE_OPEN, IMPLEMENT, RC_BLOCKED,
};
use super::intake::{base_run_repo, cargo_calls, fake_cargo};
use super::run_pipe_with_path;
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

/// (g) 形 7（設計 pipeline.md §56・行 ay）: `deny` を名乗る repo の契約は、偽の cargo が base で緑を返す形でも列の候補の判定で
/// 待たされず（理由 `-`）、偽の cargo は 1 度も撃たれない（列の候補の judge は base の木を撃たない）。
#[test]
fn pipe_dispatch_base_run_deny_contract_is_not_held_and_nothing_is_fired() {
    let (repo, state) = base_run_repo(Some("deny"));
    let (path, log) = fake_cargo(&state, "cargo-green", &[("derive_", "exit 0"), ("other_", "exit 0")]);
    let bd = fake_bd(&state, &[issue("s2-toy.9", 2, "t")]);
    let (repo_arg, state_arg, rules) = (repo.display().to_string(), state.display().to_string(), dispatch_rules(&state));
    let out = run_pipe_with_path(&path, &["dispatch", "ls", "--state-dir", &state_arg, "--repo", &repo_arg, "--rules", &rules, "--bd", &bd]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", told(&out));
    assert_eq!(reason_of(&out, "s2-toy.9"), "-", "deny の契約を待たせない: {}", told(&out));
    assert_eq!(count_of(&out), format!("{COUNT} total=1 ready=1"), "{}", told(&out));
    assert!(cargo_calls(&log).is_empty(), "列は base の木を撃たない: {:?}", cargo_calls(&log));
    clean(&[&repo, &state]);
}

/// **自分の見た cwd を `seen` へ書き出してから**台帳の JSON を吐く偽の `bd`（子の cwd を測る側・設計 §14）。
/// `pwd -P` は物理 path＝`tmp()` の canonical な path と同じ字面になる。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn cwd_recording_bd(state: &Path, issues: &[String], seen: &Path) -> String {
    let json = state.join("ledger-cwd.json");
    fs::write(&json, format!("[{}]\n", issues.join(","))).expect("偽の台帳を書ける");
    script(
        &state.join("bd-cwd"),
        &format!("pwd -P > '{}'\ncat '{}'\n", seen.display(), json.display()),
    )
}

/// `pipe dispatch ls` を **process の cwd を `cwd` にして**撃つ（`--repo` は呼び手の字面のまま渡す＝相対 path も
/// そのまま）。[`ls`] との違いは cwd と `--repo` の字面だけである。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn ls_from(cwd: &Path, repo: &str, state: &Path, bd: &str) -> Output {
    // cwd が主題なので [`super::bin_cmd`] の固定した cwd に自分の `current_dir` を後置する（後の指定が勝つ）。
    super::bin_cmd()
        .args([
            "pipe", "dispatch", "ls",
            "--state-dir", &state.display().to_string(),
            "--repo", repo,
            "--rules", &dispatch_rules(state),
            "--bd", bd,
        ])
        .current_dir(cwd)
        .output()
        .expect("binary を起動できる")
}

/// 子（偽の `bd`）が書き出した cwd の 1 行（無ければ空＝子は起きていない）。
fn seen_cwd(seen: &Path) -> String {
    fs::read_to_string(seen).unwrap_or_default().trim_end().to_owned()
}

/// (§14 (a)) process の cwd を別の dir にしたまま `--repo <toy>` で撃つと、台帳の子 process の見た cwd は
/// **toy repo**である（process の cwd でない）。台帳は読めている（件数の行が出る）。
///
/// base は `read_text` が cwd を名指さない＝子は process の cwd を継ぐ（RED）。
#[test]
fn pipe_dispatch_ledger_cwd_is_the_named_repo_not_the_process_cwd() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let elsewhere = state.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap_or_else(|err| panic!("別の dir を作れる: {err}"));
    let seen = state.join("seen-cwd");
    let bd = cwd_recording_bd(&state, &[issue("s2-toy.2", 2, "b")], &seen);
    let out = ls_from(&elsewhere, &repo.display().to_string(), &state, &bd);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "ls は rc 0: {}", told(&out));
    assert_eq!(count_of(&out), format!("{COUNT} total=1 ready=1"), "台帳は読めている: {}", told(&out));
    assert_eq!(seen_cwd(&seen), repo.display().to_string(), "子の見た cwd は --repo の値");
    assert_ne!(seen_cwd(&seen), elsewhere.display().to_string(), "process の cwd ではない");
    clean(&[&repo, &state]);
}

/// (§14 (b)) `--repo` を**相対 path**で渡した周も、子の見た cwd は絶対 path で撃った周と**同じ絶対 path**である
/// （口が値を絶対にする §12 の形と噛み合う pin）。
#[test]
fn pipe_dispatch_ledger_cwd_relative_repo_resolves_to_the_same_absolute_path() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let seen = state.join("seen-cwd");
    let bd = cwd_recording_bd(&state, &[issue("s2-toy.2", 2, "b")], &seen);
    let absolute = ls_from(&state, &repo.display().to_string(), &state, &bd);
    assert_eq!(absolute.status.code(), Some(i32::from(RC_OK)), "絶対 path の周: {}", told(&absolute));
    let seen_absolute = seen_cwd(&seen);
    assert_eq!(seen_absolute, repo.display().to_string(), "絶対 path の周の子の cwd");
    // 相対 path: toy repo の親 dir を process の cwd にして、leaf 名だけを `--repo` に渡す。
    let parent = repo.parent().unwrap_or_else(|| panic!("toy repo は親 dir を持つ: {}", repo.display()));
    let leaf = repo.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_owned();
    fs::remove_file(&seen).unwrap_or_else(|err| panic!("前の周の書き出しを消せる: {err}"));
    let relative = ls_from(parent, &leaf, &state, &bd);
    assert_eq!(relative.status.code(), Some(i32::from(RC_OK)), "相対 path の周: {}", told(&relative));
    assert_eq!(count_of(&relative), format!("{COUNT} total=1 ready=1"), "相対 path の周も台帳を読める: {}", told(&relative));
    assert_eq!(seen_cwd(&seen), seen_absolute, "相対 path の周の子の cwd は同じ絶対 path（leaf={leaf}）");
    clean(&[&repo, &state]);
}

/// (§14 (c)) 無い dir を `--repo` に渡した周は、子の `spawn` が落ちて列は `UNMEASURED` の行で 0 本である
/// （`[DISPATCH-NONE]` とも件数の行とも融合しない・C10）。偽の台帳 client は起きていない（cwd の書き出しが無い）。
#[test]
fn pipe_dispatch_ledger_cwd_missing_repo_is_unmeasured_not_none() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let seen = state.join("seen-cwd");
    let bd = cwd_recording_bd(&state, &[issue("s2-toy.2", 2, "b")], &seen);
    let missing = state.join("no-such-repo");
    let out = ls(&missing, &state, &bd);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "ls は rc 0: {}", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), UNMEASURED, "読めない周の 1 行だけ: {}", told(&out));
    assert!(!stdout_of(&out).contains(NONE_LINE), "0 件の行と融合しない: {}", told(&out));
    assert!(!stdout_of(&out).contains(COUNT), "件数の行を出さない: {}", told(&out));
    assert!(!seen.exists(), "台帳 client は起きていない（cwd の書き出しが無い）");
    // 同じ台帳 client で在る repo を渡すと読める＝読めなさの根は `--repo` の dir である。
    let readable = ls(&repo, &state, &bd);
    assert_eq!(count_of(&readable), format!("{COUNT} total=1 ready=1"), "在る repo では読める: {}", told(&readable));
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

/// 行 a の便を審査の判定 `judgement`（`review.json` の本文）で `Reviewed` の終端に着ける。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn judged_run(repo: &Path, state: &Path, bead: &str, judgement: &str) -> String {
    let id = intake_bead(repo, state, &format!("{DESIGN_FILE}#a"), bead);
    fs::write(state.join("pipe").join(&id).join(REVIEW_FILE), format!("{judgement}\n")).expect("審査の判定を書ける");
    id
}

/// 審査を測れなかった判定（lens の出力に判定の行が無い周）。
const UNPARSED: &str = "{\"verdict\":\"INCONCLUSIVE\",\"kind\":\"unparsed\"}";

/// (§22 (b)) 審査を測れなかった便（`Reviewed` の INCONCLUSIVE `kind:unparsed`）は `release` で**同じ sha のまま**
/// 列に戻り（`reason=-`・`ready=1`）、起こし直した便が同じ sha でまた unparsed に着けば再び列外（印 1 回で 1 回）。
/// base は `Reviewed` を判定の中身を見ずに戻さない（`settled:…/Reviewed` のまま＝RED）。
#[test]
fn pipe_dispatch_release_unparsed_requeues_an_unmeasured_review_once_at_the_same_sha() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    let first = judged_run(&repo, &state, bead, UNPARSED);
    let (settled, released) = reasons_around_release(&repo, &state, bead, "Reviewed");
    assert_eq!(released, "-", "測れなかった審査は release で同じ sha のまま列に戻る");
    let bd = fake_bd(&state, &[issue(bead, 2, "a")]);
    let back = ls(&repo, &state, &bd);
    assert_eq!(count_of(&back), format!("{COUNT} total=1 ready=1"), "戻った契約は起こせる（{}）", told(&back));
    // **起こし直した便が同じ sha でまた unparsed に着く**（秒を跨いで同じ bead の 2 本目・同じ契約 file）。
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = judged_run(&repo, &state, bead, UNPARSED);
    assert_ne!(second, first, "起こし直した便は新しい run id");
    let again = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&again, bead), settled, "同じ sha でまた unparsed＝再び列外（印は 1 回しか効かない）（{}）", told(&again));
    assert_eq!(count_of(&again), format!("{COUNT} total=1 ready=0"), "2 度目は起こさない");
    clean(&[&repo, &state]);
}

/// (§22 (c)) 審査役が材料を読んで出した INCONCLUSIVE（`kind:section-material-missing`）と、kind が unparsed でも
/// FAIL の便は `release` の後も理由が変わらない（(b) が「INCONCLUSIVE を全部戻す」「unparsed を全部戻す」変異で
/// ないことを測る・FR49）。
#[test]
fn pipe_dispatch_release_unparsed_keeps_other_review_judgements_out() {
    for judgement in [
        "{\"verdict\":\"INCONCLUSIVE\",\"kind\":\"section-material-missing\"}",
        "{\"verdict\":\"FAIL\",\"kind\":\"unparsed\"}",
    ] {
        let (repo, state) = repo_with_state();
        two_rows(&repo);
        let bead = "s2-toy.1";
        judged_run(&repo, &state, bead, judgement);
        let (settled, released) = reasons_around_release(&repo, &state, bead, "Reviewed");
        assert_eq!(released, settled, "{judgement} は release の後も列外のまま（理由も変わらない）");
        clean(&[&repo, &state]);
    }
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

/// toy の設計 doc の § の本文（`design_doc_rows` の固定の 1 行・§ を直す歯はこれを置き換える）。
const SECTION_BODY: &str = "toy repo の縦 1 本を通す節の本文。";

/// 審査の材料の dir に置かれる § の写し（run dir の `review/design.txt`）。
fn section_copy(state: &Path, id: &str) -> std::path::PathBuf {
    state.join("pipe").join(id).join("review").join("design.txt")
}

/// 行 `row` の便を審査の判定 `verdict` で `Reviewed` の終端に着ける（intake は偽 PASS の lens で通し、判定 file を
/// 書き換える＝§ の写しは審査の段が置いたまま）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn reviewed_run(repo: &Path, state: &Path, bead: &str, row: &str, verdict: &str) -> String {
    let id = intake_bead(repo, state, &format!("{DESIGN_FILE}#{row}"), bead);
    fs::write(state.join("pipe").join(&id).join(REVIEW_FILE), format!("{{\"verdict\":\"{verdict}\"}}\n"))
        .expect("審査の判定を書ける");
    assert!(section_copy(state, &id).is_file(), "前提: 審査の段は § の写しを置く");
    id
}

/// 設計 doc の § の本文だけを `from` → `to` で直して commit する（**契約表の行は 1 字も変えない**＝生成される
/// 契約 file は同じ）。`rows` は doc が持つ行の列（[`two_rows`] と同じ形で呼び手が選ぶ）。
fn revise_section(repo: &Path, rows: &[Vec<String>], from: &str, to: &str) {
    let body = design_doc_rows(rows);
    assert!(body.contains(from), "前提: 直す前の § の本文が doc に在る: {body}");
    let revised = body.replace(from, to);
    assert_ne!(revised, body, "§ の本文が動く");
    write_design(repo, &revised);
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "section-revised"]);
}

/// [`two_rows`] が commit する行の列（§ を直す歯が同じ行のまま doc を書き直すのに使う）。
fn two_row_fields() -> Vec<Vec<String>> {
    vec![
        row_fields("a", &["write-set"], &[r#"write-set = ["src/lib.rs"]"#]),
        row_fields("b", &["write-set"], &[r#"write-set = ["src/b.rs"]"#]),
    ]
}

/// (§16 (a)) 審査 INCONCLUSIVE で終端した `Reviewed` の便の契約は、§ の本文を直した後の 1 周で列に戻る
/// （`dispatch ls` の理由が値なしの欄・`ready=1`）。base は契約 file の字しか鍵に持たない（RED）。
#[test]
fn pipe_dispatch_section_key_requeues_an_inconclusive_review_after_the_section_changes() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    reviewed_run(&repo, &state, bead, "a", "INCONCLUSIVE");
    let bd = fake_bd(&state, &[issue(bead, 2, "a")]);
    let before = ls(&repo, &state, &bd);
    let settled = reason_of(&before, bead);
    assert!(settled.starts_with("settled:"), "直す前は列外（{}）", told(&before));
    assert!(settled.ends_with("/Reviewed"), "段は Reviewed: {settled}");
    revise_section(&repo, &two_row_fields(), SECTION_BODY, "toy repo の縦 1 本を通す節の本文（審査役の指摘で直した）。");
    let after = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&after, bead), "-", "§ を直せば列に戻る（{}）", told(&after));
    assert_eq!(count_of(&after), format!("{COUNT} total=1 ready=1"), "戻った契約は起こせる（{}）", told(&after));
    clean(&[&repo, &state]);
}

/// (§16 (b)) § も契約 file も変わっていない周は列外のまま（無限に起こし直さない・理由の字面も同じ）。§ の外の
/// commit（別の file）が積まれても鍵は動かない。
#[test]
fn pipe_dispatch_section_key_keeps_the_run_out_while_nothing_changed() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    reviewed_run(&repo, &state, bead, "a", "INCONCLUSIVE");
    let bd = fake_bd(&state, &[issue(bead, 2, "a")]);
    let first = ls(&repo, &state, &bd);
    let settled = reason_of(&first, bead);
    assert!(settled.starts_with("settled:"), "列外（{}）", told(&first));
    assert!(settled.ends_with("/Reviewed"), "段は Reviewed: {settled}");
    fs::write(repo.join("src").join("b.rs"), "// unrelated\n").expect("§ の外の file を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "unrelated"]);
    let again = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&again, bead), settled, "§ も契約 file も同じなら列外のまま・字面も同じ（{}）", told(&again));
    assert_eq!(count_of(&again), format!("{COUNT} total=1 ready=0"), "起こさない");
    clean(&[&repo, &state]);
}

/// (§16 (c)) 審査 FAIL で終端した便も § を直せば戻る（FAIL と INCONCLUSIVE の弁別は鍵に要らない＝どちらも
/// 「この材料では通らなかった」）。
#[test]
fn pipe_dispatch_section_key_requeues_a_failed_review_after_the_section_changes() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    reviewed_run(&repo, &state, bead, "a", "FAIL");
    let bd = fake_bd(&state, &[issue(bead, 2, "a")]);
    let before = ls(&repo, &state, &bd);
    assert!(reason_of(&before, bead).ends_with("/Reviewed"), "直す前は審査の終端で列外（{}）", told(&before));
    revise_section(&repo, &two_row_fields(), SECTION_BODY, "toy repo の縦 1 本を通す節の本文（FAIL の後に直した）。");
    let after = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&after, bead), "-", "審査 FAIL も § を直せば戻る（{}）", told(&after));
    assert_eq!(count_of(&after), format!("{COUNT} total=1 ready=1"), "戻った契約は起こせる");
    clean(&[&repo, &state]);
}

/// (§16 (d)) § の写しを**持たない**便と、写しが**在るのに読めない**便は契約 file だけの鍵で今までどおり列外
/// （「無い」と「違う」を畳まない）。同じ周に写しが在って読める便は戻る＝**母集団は写しの 3 値**で、鍵が
/// 効いているのに 2 値だけが留まることを 1 周で測る。
#[test]
fn pipe_dispatch_section_key_falls_back_to_the_contract_when_the_copy_is_absent_or_unreadable() {
    let (repo, state) = repo_with_state();
    let rows = vec![
        row_fields("a", &["write-set"], &[r#"write-set = ["src/lib.rs"]"#]),
        row_fields("b", &["write-set"], &[r#"write-set = ["src/b.rs"]"#]),
        row_fields("c", &["write-set"], &[r#"write-set = ["src/c.rs"]"#]),
    ];
    commit_rows(&repo, &rows);
    let absent = reviewed_run(&repo, &state, "s2-toy.1", "a", "INCONCLUSIVE");
    let unreadable = reviewed_run(&repo, &state, "s2-toy.2", "b", "INCONCLUSIVE");
    reviewed_run(&repo, &state, "s2-toy.3", "c", "INCONCLUSIVE");
    fs::remove_file(section_copy(&state, &absent)).expect("写しを消せる");
    // 在るのに読めない写し: 同じ名で dir を置く（file として読めない・root でも読めない形）。
    fs::remove_file(section_copy(&state, &unreadable)).expect("写しを消せる");
    fs::create_dir(section_copy(&state, &unreadable)).expect("同じ名の dir を置ける");
    revise_section(&repo, &rows, SECTION_BODY, "toy repo の縦 1 本を通す節の本文（3 便の後に直した）。");
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a"), issue("s2-toy.2", 2, "b"), issue("s2-toy.3", 2, "c")]);
    let out = ls(&repo, &state, &bd);
    let kept_out = |bead: &str| {
        let reason = reason_of(&out, bead);
        reason.starts_with("settled:") && reason.ends_with("/Reviewed")
    };
    assert!(kept_out("s2-toy.1"), "写しが無い便は契約 file だけの鍵で列外のまま（{}）", told(&out));
    assert!(kept_out("s2-toy.2"), "写しが在るのに読めない便も列外のまま（{}）", told(&out));
    assert_eq!(reason_of(&out, "s2-toy.3"), "-", "写しが在って読める便だけ戻る（{}）", told(&out));
    assert_eq!(count_of(&out), format!("{COUNT} total=3 ready=1"), "3 値のうち起こせるのは 1 本");
    clean(&[&repo, &state]);
}

/// (§16 (e)) `Landed` の便は § を直しても戻らない（済んでいる・起こし直すと同じ変更をもう一度作る）。
/// 母集団 = 終端の段の種類のうち、§ の写しを持つ側（`Reviewed` は上の歯・`Landed` はこの歯）。
#[test]
fn pipe_dispatch_section_key_does_not_requeue_a_landed_run() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    let id = gated_run(&repo, &state, bead, "PASS");
    let landed = super::land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land は rc 0（{}）", told(&landed));
    assert!(section_copy(&state, &id).is_file(), "前提: 着地した便も § の写しを持つ");
    let bd = fake_bd(&state, &[issue(bead, 2, "a")]);
    let before = ls(&repo, &state, &bd);
    let settled = reason_of(&before, bead);
    assert!(settled.ends_with("/Landed"), "着地した便は列外（{}）", told(&before));
    revise_section(&repo, &two_row_fields(), SECTION_BODY, "toy repo の縦 1 本を通す節の本文（着地の後に直した）。");
    let after = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&after, bead), settled, "Landed は § を直しても列外のまま・理由も同じ（{}）", told(&after));
    assert_eq!(count_of(&after), format!("{COUNT} total=1 ready=0"), "起こさない");
    clean(&[&repo, &state]);
}

/// (§16 (f)) § の本文を **1 文字**だけ変えた周も戻る（列が突き合わせる本文が審査の材料と同じ 1 本から出ている
/// pin＝末尾の整えや空白の畳みで差が消えない）。
#[test]
fn pipe_dispatch_section_key_requeues_on_a_single_character_change() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bead = "s2-toy.1";
    reviewed_run(&repo, &state, bead, "a", "INCONCLUSIVE");
    let bd = fake_bd(&state, &[issue(bead, 2, "a")]);
    let before = ls(&repo, &state, &bd);
    assert!(reason_of(&before, bead).ends_with("/Reviewed"), "直す前は列外（{}）", told(&before));
    let one = "toy repo の縦 2 本を通す節の本文。";
    assert_eq!(SECTION_BODY.chars().count(), one.chars().count(), "前提: 字数は同じ");
    assert_eq!(SECTION_BODY.chars().zip(one.chars()).filter(|(a, b)| a != b).count(), 1, "前提: 違いは 1 文字");
    revise_section(&repo, &two_row_fields(), SECTION_BODY, one);
    let after = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&after, bead), "-", "1 文字の差でも戻る（{}）", told(&after));
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
    let measured = super::bin_cmd()
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

// ───── 道具箱の台帳 client の見張り（設計 gate-cost.md §37・行 ad・`s2-07l.484`・接頭辞 `e2e_ledger_tripwire_`） ─────
// flip-check: retroactive s2-07l.484

/// (a) 非空虚の枝: 列の 1 周を `--repo` と `--runner` と台帳の待ち上限の行を持つ写しつきで、`--bd` を**渡さず**撃つと、
/// 見張りの記録が**ちょうど 1 件**在り、その本文が読みの引数（`--readonly` と一覧の語）を持つ。見張りは台帳を
/// 解けない host と同じ形で断るので、1 周は理由を名乗る（0 件の台帳に読み替えない）。
#[test]
fn e2e_ledger_tripwire_default_client_round_leaves_exactly_one_record() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let out = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--runner", "true",
    ]);
    assert_eq!(stdout_of(&out).trim_end(), "dispatch=unmeasured reason=ledger", "見張りは断る（{}）", told(&out));
    let calls = crate::toolbox_ledger_record_names(&state);
    assert_eq!(calls.len(), 1, "見張りの記録はちょうど 1 件（母集団 {calls:?}）");
    let body = calls
        .first()
        .and_then(|name| fs::read_to_string(crate::toolbox_ledger_records(&state).join(name)).ok())
        .unwrap_or_default();
    let argv: Vec<&str> = body.lines().collect();
    assert!(argv.contains(&"--readonly"), "読みの引数を持つ（母集団 {calls:?}）: {argv:?}");
    assert!(argv.contains(&"list"), "一覧の語を持つ（母集団 {calls:?}）: {argv:?}");
    clean(&[&repo, &state]);
}

/// (c) 明示の口と食い合わない: `--bd` に fixture の偽 client の絶対 path を渡した周は、見張りの記録が 0 件のまま、
/// 偽 client 側の log に呼出が残る（同じ置き場に道具箱は組まれている＝見張りが PATH の先頭に在っても通らない）。
#[test]
fn e2e_ledger_tripwire_explicit_client_bypasses_the_tripwire() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let json = state.join("ledger-explicit.json");
    fs::write(&json, "[]\n").expect("偽の台帳を書ける");
    let log = state.join("bd-explicit.log");
    let client = script(
        &state.join("bd-explicit"),
        &format!("printf '%s\\n' \"$*\" >> '{}'\ncat '{}'\n", log.display(), json.display()),
    );
    let out = run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(&state),
        "--bd", &client,
        "--runner", "true",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert!(state.join(crate::TOOLBOX_BIN).join(vessel::seat::ledger::DEFAULT_BD).is_file(), "母集団: 見張りは置かれている");
    let calls = fs::read_to_string(&log).map(|text| text.lines().count()).unwrap_or_default();
    assert!(calls >= 1, "偽 client 側に呼出が残る（{}）", told(&out));
    let tripped = crate::toolbox_ledger_record_names(&state);
    assert!(tripped.is_empty(), "見張りの記録は 0 件（{tripped:?}・偽 client の呼出 {calls} 件）");
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
        .filter_map(|_| super::bin_cmd().args(&args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok())
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

/// path が消えるまで待つ（上限 60s・消えなければ `false`）。
///
/// 継いだ子は toy の gate と land を撃つので、負荷の周は 20 秒を越える（設計 dispatcher.md §24・行 u）。
fn gone(path: &Path) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
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
///
/// 2 語目は**どの process の起動時刻とも一致しない値** `1`（epoch ms）である（設計 dispatcher.md §24・行 u）:
/// 負荷の周に抜けた pid を隣の process が受け取っても、読み手は起動時刻の違いで `Dead` に読む。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_dead_ticket(state: &Path, id: &str) {
    let mut child = Command::new("true").spawn().expect("true を起こせる");
    let pid = child.id();
    child.wait().expect("true を待てる");
    let path = state.join("pipe").join(id).join("driver");
    fs::write(&path, format!("{pid} 1\n")).expect("札を書ける");
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

// ───── 席が測り直して PASS になった Gated の便の再開（行 (l)・設計 §15・`pipe_dispatch_gated_pass_` 接頭辞） ─────
//
// INCONCLUSIVE で正常に抜けた driver は札を外し、席が `pipe gate` で測り直して PASS にしても札は無いままなので、
// §5 の起こし直し（札の所有者が死んだ便だけ）では候補に戻らなかった（実測 2026-09-20: 手動の 1 周で `resumed:0`）。
// 列は `Gated` の便に「verdict が PASS ∧ 札が無いか所有者が死んでいる」の 1 枝を足し、`--drive` 付きの resume で起こす。

/// 便を `Gated` に着ける（intake → 1 commit の偽 runner → 偽 lens の `verdict` で `pipe gate`）。
/// **札は無い**（`pipe gate` は driver の札を置かない＝席が測り直した形そのもの）。
fn gated_without_ticket(repo: &Path, state: &Path, verdict: &str) -> String {
    let contract = write_contract(repo, &[], &[]);
    let id = implemented(repo, state, &contract);
    let lens = fake_lens(&state.join(format!("gated-pass-lens-{verdict}")), &lens_verdict(verdict));
    let out = gate_once(repo, state, &id, Some(&lens));
    assert!(gated_pair_ok(verdict, &out), "gate {verdict}（{}）", told(&out));
    assert_eq!(reached_now(state, &id, "Gated"), 1, "前提: Gated（段の並び: {}）", stages_of(state, &id));
    assert!(!state.join("pipe").join(&id).join("driver").exists(), "前提: 札は無い");
    id
}

/// 行 `row` の便を bead 名つきで `Gated` に着ける（同じ置き場に 2 便を置く歯が使う・偽 runner は [`two_rows`] の
/// 行の write-set の file〔a = `src/lib.rs`・b = `src/b.rs`〕を触る）。
fn gated_bead(repo: &Path, state: &Path, row: &str, bead: &str, verdict: &str) -> String {
    let id = intake_bead(repo, state, &format!("{DESIGN_FILE}#{row}"), bead);
    let file = match row {
        "a" => "src/lib.rs",
        _ => "src/b.rs",
    };
    let runner = format!("echo x >> {file} && git add -A && git commit -q -m runner");
    let spawned = super::spawn_with(repo, state, &id, &runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "{bead} の spawn は rc 0（{}）", told(&spawned));
    let lens = fake_lens(&state.join(format!("gated-bead-lens-{bead}-{verdict}")), &lens_verdict(verdict));
    let out = gate_once(repo, state, &id, Some(&lens));
    assert!(gated_pair_ok(verdict, &out), "{bead} の gate {verdict}（{}）", told(&out));
    id
}

/// (§15 (a)) verdict PASS ∧ 札の無い `Gated` の便は手動の 1 周で **`--drive` 付きの resume** で起こされ（`resumed:1`）、
/// 先の段（`Landed`）へ進む。base は `Gated` の便を札の所有者が死んだものしか候補にしない（RED）。
#[test]
fn pipe_dispatch_gated_pass_without_ticket_is_resumed_with_drive() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "PASS");
    let out = waiting_turn(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "PASS の Gated の便を 1 本起こし直す（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "先の段へ進む（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§15 (b)) verdict INCONCLUSIVE ∧ 札の無い `Gated` の便は起こされない（`resumed:0`・再 gate も起きない＝器が
/// 勝手に 1 周ぶんの費用を払い直さない）。
#[test]
fn pipe_dispatch_gated_pass_inconclusive_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "INCONCLUSIVE");
    let out = waiting_turn(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "INCONCLUSIVE の便は起こさない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "着地しない（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(reached_now(&state, &id, "Gated"), 1, "再 gate も起きない（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§15 (c)) verdict を**読めない** `Gated` の便も起こされない（測れないを「通った」に読み替えない・fail-closed）。
#[test]
fn pipe_dispatch_gated_pass_unreadable_verdict_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "PASS");
    let verdict = state.join("pipe").join(&id).join("verdict.json");
    assert!(verdict.exists(), "前提: 判定 file は在る");
    fs::write(&verdict, "not json\n").unwrap_or_else(|err| panic!("判定 file を壊せる: {err}"));
    let out = waiting_turn(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "読めない verdict の便は起こさない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "着地しない（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(reached_now(&state, &id, "Gated"), 1, "段は動かない（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§15 (d) 札の 4 値) **所有者が生きている**札の PASS の `Gated` の便は触らない（別の driver が駆動している便に
/// 2 本目を立てない・札も奪わない）。
#[test]
fn pipe_dispatch_gated_pass_live_ticket_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "PASS");
    // 歯の process 自身の pid＝確実に生きている所有者。
    put_ticket_body(&state, &id, &format!("{}\n", std::process::id()));
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "生きている所有者の札の便は触らない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "着地しない（段の並び: {}）", stages_of(&state, &id));
    assert!(state.join("pipe").join(&id).join("driver").exists(), "札は奪わない");
    clean(&[&repo, &state]);
}

/// (§15 (d) 札の 4 値) **在るのに読めない**札の PASS の `Gated` の便は触らない（測れないを「居ない」に読み替えない）。
#[test]
fn pipe_dispatch_gated_pass_unreadable_ticket_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "PASS");
    put_ticket_body(&state, &id, "not-a-pid\n");
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "読めない札の便は触らない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "着地しない（段の並び: {}）", stages_of(&state, &id));
    assert!(state.join("pipe").join(&id).join("driver").exists(), "札は触らない");
    clean(&[&repo, &state]);
}

/// (§15 (e)) 札の**所有者が死んでいる** `Gated` の便は verdict に依らず今までどおり起こされる（既存の規則・
/// **母集団 = PASS と INCONCLUSIVE の 2 値**）。起こし直しが**実際に走った**証拠は、死んだ所有者の札を継いだ
/// resume が抜けるときに自分の札を外すこと（数えただけでは撃ったと言えない）。PASS の周は着地まで通り、
/// INCONCLUSIVE の周は resume が `next=gate` で止まる（自動では測り直さない・段は `Gated` のまま）。
#[test]
fn pipe_dispatch_gated_pass_dead_ticket_is_resumed_regardless_of_verdict() {
    for verdict in ["PASS", "INCONCLUSIVE"] {
        let (repo, state) = repo_with_state();
        let id = gated_without_ticket(&repo, &state, verdict);
        put_dead_ticket(&state, &id);
        let ticket = state.join("pipe").join(&id).join("driver");
        let out = waiting_turn(&repo, &state);
        assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "{verdict}: 死んだ所有者の札の便は起こす（{}）", told(&out));
        assert!(gone(&ticket), "{verdict}: 継いだ resume が抜けるときに死んだ札を外す（段の並び: {}）", stages_of(&state, &id));
        let landed = match verdict {
            "PASS" => stage_reached(&state, &id, "Landed"),
            _ => not_reached(&state, &id, "Landed"),
        };
        assert_eq!(landed, usize::from(verdict == "PASS"), "{verdict}: 着地は PASS の周だけ（段の並び: {}）", stages_of(&state, &id));
        clean(&[&repo, &state]);
    }
}

/// (§15 (f)) 段を前へ進めなかった driver の終端の 1 周は、この候補を 1 本も起こさない（空撃ちの連鎖を塞ぐ）。
///
/// 便 B（行 b）を INCONCLUSIVE の `Gated` に置き、INCONCLUSIVE の lens で `--drive` の resume を撃つ（再 gate で
/// 同じ段＝`no-progress`）。その終端の 1 周は、同じ置き場で PASS の `Gated` に在った便 A（行 a・札なし）を起こさない。
/// 正負の対: その後の手動の 1 周（driver でない契機）は A を起こす。
#[test]
fn pipe_dispatch_gated_pass_no_progress_driver_turn_resumes_nothing() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let passed = gated_bead(&repo, &state, "a", "s2-toy.1", "PASS");
    let other = gated_bead(&repo, &state, "b", "s2-toy.2", "INCONCLUSIVE");
    let unsure = fake_lens(&state.join("gated-pass-unsure-lens"), &lens_verdict("INCONCLUSIVE"));
    let out = resume_once(&repo, &state, &other, &unsure, true);
    assert_eq!(
        stdout_of(&out).lines().last(),
        Some(format!("{} drive=no-progress", resumed_line(0)).as_str()),
        "段が動かなかった driver の 1 周は A を起こさない（{}）",
        told(&out)
    );
    assert_eq!(not_reached(&state, &passed, "Landed"), 0, "A は着地しない（段の並び: {}）", stages_of(&state, &passed));
    let manual = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&manual).trim_end(), resumed_line(1), "手動の 1 周は A を起こす（{}）", told(&manual));
    assert_eq!(stage_reached(&state, &passed, "Landed"), 1, "A は自走で着地まで（段の並び: {}）", stages_of(&state, &passed));
    clean(&[&repo, &state]);
}

/// (§15 (h)) **flag の無い** resume が Implemented → `Gated`（PASS）で抜けた直後の自分の終端の 1 周は自分の便を
/// 起こさず（段は `Gated` のまま＝「1 段だけ」を保つ）、その後の手動の 1 周は同じ便を起こす（正負の対）。
///
/// flag の無い周も終端の 1 周は撃つ（`--repo` と `--state-dir` と `--runner` を渡す）。抜けた driver は札を外して
/// いるので、自分の id を列に渡さなければ PASS の枝が自分の便を拾い、着地まで運んでしまう。
#[test]
fn pipe_dispatch_gated_pass_flagless_driver_leaves_its_own_run_for_the_next_turn() {
    let (repo, state) = repo_with_state();
    let contract = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &contract);
    let lens = fake_lens(&state.join("gated-pass-flagless-lens"), &lens_verdict("PASS"));
    let out = resume_once(&repo, &state, &id, &lens, false);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "flag 無しの resume は rc 0（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Gated"), 1, "1 段だけ進む（段の並び: {}）", stages_of(&state, &id));
    assert!(!state.join("pipe").join(&id).join("driver").exists(), "抜けた driver は札を外している");
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "自分の終端の 1 周は自分の便を起こさない（段の並び: {}）", stages_of(&state, &id));
    let manual = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&manual).trim_end(), resumed_line(1), "手動の 1 周は同じ便を起こす（{}）", told(&manual));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "自走で着地まで（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

// ───── regate で戻された便の再開（行 (t)・設計 §23・`pipe_dispatch_regated_` 接頭辞） ─────
//
// `pipe regate` は段を 1 つ戻す記帳だけを書き、札を書かない（driver は正常に抜けて札を外している）ので、§5 の
// 起こし直し（札の所有者が死んだ便だけ）では候補に戻らなかった（実測 2026-09-23: 手動の 1 周で `resumed:0`）。
// 列は「段が戻った ∧ 最新の gate の後に regate の記帳 ∧ 札が無いか所有者が死んでいる」便を `--drive` 付きの
// resume で起こす。段は既存の helper と同じ字面で数える（段の型の変種を名指さない）。

/// 便の `RunStage` のうち段が `stage` で `detail` が `detail` で始まる記帳の件数（いま数えるだけ・空は条件なし）。
fn run_stages(state: &Path, id: &str, stage: &str, detail: &str) -> usize {
    fs::read_to_string(state.join("fleet").join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("\"kind\":\"RunStage\""))
        .filter(|line| line.contains(&format!("\"stage\":\"{stage}\"")))
        .filter(|line| line.contains(&format!("\"run\":\"{id}\"")))
        .filter(|line| detail.is_empty() || line.contains(&format!("\"detail\":\"{detail}")))
        .count()
}

/// gate の判定の `Gated` の記帳の件数（`detail` が `verdict:`・着地の窓の `turn:` の記帳を数えない）。着地の追随が
/// 前周の PASS を引き継いで書く同じ形の記帳は `verify.jsonl` の `"skipped":"regate"` の record 1 本につき 1 件引く。
fn gate_runs(state: &Path, id: &str) -> usize {
    let carried = fs::read_to_string(state.join("pipe").join(id).join("verify.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("\"skipped\":\"regate\""))
        .count();
    run_stages(state, id, "Gated", "verdict:").saturating_sub(carried)
}

/// `pipe regate` を 1 回撃ち、rc 0 の 1 行で段が戻ったことを確かめる。
fn regate_run(repo: &Path, state: &Path, id: &str) {
    let out = run_pipe(&[
        "regate", "--run", id, "--reason", "裁定: 器の一過性の赤（契約の赤でない）",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "regate は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), format!("regate: run={id} from=Gated to=Implemented"), "1 行（{}）", told(&out));
}

/// 判定 FAIL の `Gated` の便を置いて regate で戻す（札は無い）。
fn regated_without_ticket(repo: &Path, state: &Path) -> String {
    let id = gated_without_ticket(repo, state, "FAIL");
    regate_run(repo, state, &id);
    id
}

/// (§23 (a)) 札の無い regate 済みの便は手動の 1 周で `--drive` 付きの resume で起こされ（`resumed:1`）、`Gated` の
/// 記帳が 1 件増えて `Landed` まで進む。base は `resumed:0`（機能不在）。
#[test]
fn pipe_dispatch_regated_run_without_a_ticket_is_resumed_to_landed() {
    let (repo, state) = repo_with_state();
    let id = regated_without_ticket(&repo, &state);
    let gated = gate_runs(&state, &id);
    let out = waiting_turn(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "regate 済みの便を 1 本起こす（{}）", told(&out));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "着地まで（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(gate_runs(&state, &id), gated + 1, "gate をもう 1 周（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§23 (b)) 札の所有者が死んでいる判定 FAIL の `Gated` の便は regate を通って worktree の path・HEAD・判定の
/// verdict が変わらず、続く手動の 1 周は `resumed:1`（二重起動 0）で `Gated` が 1 件だけ増え、死んだ札は外れる。
#[test]
fn pipe_dispatch_regated_dead_ticket_keeps_three_records_and_resumes_once() {
    let (repo, state) = repo_with_state();
    let id = gated_without_ticket(&repo, &state, "FAIL");
    put_dead_ticket(&state, &id);
    let ticket = state.join("pipe").join(&id).join("driver");
    let worktree = worktree_of(&repo, &id);
    let head = git(&worktree, &["rev-parse", "HEAD"]);
    regate_run(&repo, &state, &id);
    assert!(worktree.is_dir(), "worktree の path は同じ");
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head, "worktree の HEAD は動かない");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "FAIL", "判定の verdict は書き換えない");
    let gated = gate_runs(&state, &id);
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(1), "起こすのは 1 本（{}）", told(&out));
    assert!(gone(&ticket), "継いだ resume が死んだ札を外す（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(stage_reached(&state, &id, "Landed"), 1, "着地まで（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(gate_runs(&state, &id), gated + 1, "Gated は 1 件だけ増える（段の並び: {}）", stages_of(&state, &id));
    clean(&[&repo, &state]);
}

/// (§23 (c)) regate の後に PASS の gate を通し、main を進めて `pipe follow` で戻した便（札なし）は起こさない
/// （最新の `Gated` より後ろに regate の記帳が無い）。
#[test]
fn pipe_dispatch_regated_then_gated_and_followed_run_is_left_alone() {
    let (repo, state) = repo_with_state();
    let id = regated_without_ticket(&repo, &state);
    let lens = fake_lens(&state.join("regated-pass-lens"), &lens_verdict("PASS"));
    let passed = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "regate の後の gate は PASS（{}）", told(&passed));
    fs::write(repo.join("moved.txt"), "moved\n").unwrap_or_else(|err| panic!("別便の変更を書ける: {err}"));
    git(&repo, &["add", "moved.txt"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let followed = run_pipe(&[
        "follow", "--run", &id, "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(followed.status.code(), Some(i32::from(RC_OK)), "follow は rc 0（{}）", told(&followed));
    let before = kind_count(&state, &id, vessel::fleet::EventKind::RunStage);
    let out = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "gate を通った後の便は起こさない（{}）", told(&out));
    assert_eq!(not_reached(&state, &id, "Landed"), 0, "着地しない（段の並び: {}）", stages_of(&state, &id));
    assert_eq!(kind_count(&state, &id, vessel::fleet::EventKind::RunStage), before, "段を動かさない");
    clean(&[&repo, &state]);
}

/// (§23 (d)) 札の所有者が生きている regate 済みの便と、札が在るのに読めない regate 済みの便は起こさず札も触らない
/// （母集団 = 札の 2 値・(a)(b) と合わせて 4 値）。
#[test]
fn pipe_dispatch_regated_live_or_unreadable_ticket_is_left_alone() {
    for (form, body) in [("live", format!("{}\n", std::process::id())), ("unreadable", "not-a-pid\n".to_owned())] {
        let (repo, state) = repo_with_state();
        let id = regated_without_ticket(&repo, &state);
        put_ticket_body(&state, &id, &body);
        let before = kind_count(&state, &id, vessel::fleet::EventKind::RunStage);
        let out = waiting_turn(&repo, &state);
        assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "{form}: 起こさない（{}）", told(&out));
        assert_eq!(not_reached(&state, &id, "Gated"), 1, "{form}: gate を撃たない（段の並び: {}）", stages_of(&state, &id));
        assert_eq!(kind_count(&state, &id, vessel::fleet::EventKind::RunStage), before, "{form}: 段を動かさない");
        let kept = fs::read_to_string(state.join("pipe").join(&id).join("driver")).ok();
        assert_eq!(kept.as_deref(), Some(body.as_str()), "{form}: 札は触らない");
        clean(&[&repo, &state]);
    }
}

/// (§23 (e)) 段を前へ進めなかった driver の終端の 1 周は regate 済みの便を起こさず、その後の手動の 1 周は起こす。
///
/// 便 B（行 b）を INCONCLUSIVE の `Gated` に置き、INCONCLUSIVE の lens で `--drive` の resume を撃つ（`no-progress`）。
/// 同じ置き場の便 A（行 a・判定 FAIL を regate で戻した・札なし）は、その終端の 1 周では動かない。
#[test]
fn pipe_dispatch_regated_no_progress_driver_turn_leaves_it_for_the_manual_turn() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let regated = gated_bead(&repo, &state, "a", "s2-toy.1", "FAIL");
    regate_run(&repo, &state, &regated);
    let other = gated_bead(&repo, &state, "b", "s2-toy.2", "INCONCLUSIVE");
    let unsure = fake_lens(&state.join("regated-unsure-lens"), &lens_verdict("INCONCLUSIVE"));
    let out = resume_once(&repo, &state, &other, &unsure, true);
    assert_eq!(
        stdout_of(&out).lines().last(),
        Some(format!("{} drive=no-progress", resumed_line(0)).as_str()),
        "段が動かなかった driver の 1 周は A を起こさない（{}）",
        told(&out)
    );
    assert_eq!(not_reached(&state, &regated, "Gated"), 1, "A は gate を撃たれない（段の並び: {}）", stages_of(&state, &regated));
    let manual = waiting_turn(&repo, &state);
    assert_eq!(stdout_of(&manual).trim_end(), resumed_line(1), "手動の 1 周は A を起こす（{}）", told(&manual));
    assert_eq!(stage_reached(&state, &regated, "Landed"), 1, "A は自走で着地まで（段の並び: {}）", stages_of(&state, &regated));
    clean(&[&repo, &state]);
}

/// (§23 (f)・AC47 の自動の regate 0/K) 判定 FAIL の `Gated` の便（札なし・札の所有者が死んでいる の 2 形）に regate を
/// 撃たずに手動の 1 周を K 回撃っても、どの周も `resumed:0` で `Implemented` の記帳は 1 件も増えない。
#[test]
fn pipe_dispatch_regated_none_without_a_ruling_over_k_turns() {
    const K: usize = 3;
    for form in ["absent", "dead"] {
        let (repo, state) = repo_with_state();
        let id = gated_without_ticket(&repo, &state, "FAIL");
        if form == "dead" {
            put_dead_ticket(&state, &id);
        }
        let implemented_before = run_stages(&state, &id, "Implemented", "");
        for turn in 1..=K {
            let out = waiting_turn(&repo, &state);
            assert_eq!(stdout_of(&out).trim_end(), resumed_line(0), "{form}: 周 {turn}/{K} は起こさない（{}）", told(&out));
        }
        assert_eq!(
            (form, K, run_stages(&state, &id, "Implemented", "")),
            (form, K, implemented_before),
            "{form}: K={K} 周で Implemented の記帳は 0 件増（段の並び: {}）",
            stages_of(&state, &id)
        );
        clean(&[&repo, &state]);
    }
}

/// 起こした事実の印の行の字面（`DispatchMark` の kind と `launched` の値・器の字面を借りない）。
const LAUNCHED_MARK: [&str; 2] = ["\"kind\":\"DispatchMark\"", "\"mark\":\"launched\""];

/// 列の 1 周を撃つ（起こす側・審査は偽 PASS の lens・実装役は呼び手が選ぶ）。
fn launch_turn(repo: &Path, state: &Path, bd: &str, runner: &str) -> Output {
    run_pipe(&[
        "dispatch",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &dispatch_rules(state),
        "--bd", bd,
        "--lens", &review_lens_pass(state),
        "--runner", runner,
    ])
}

/// event log の行のうち `needles` を全部含む最初の行の位置（log を読めない周と該当 0 行は `None`）。
fn line_at(state: &Path, needles: &[&str]) -> Option<usize> {
    fs::read_to_string(state.join("fleet").join("events.jsonl"))
        .ok()?
        .lines()
        .position(|line| needles.iter().all(|needle| line.contains(needle)))
}

/// `bead` の `RunCreated` が `ms` の間に現れるか（**現れないことを測る側**・待たずに数えると遅れた子を見落とす）。
fn run_created_within(state: &Path, bead: &str, ms: u64) -> Option<usize> {
    let needles = ["\"kind\":\"RunCreated\"", &format!("\"bead\":\"{bead}\"")];
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while line_at(state, &needles).is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    line_at(state, &needles)
}

/// 起こした事実の印だけを event log へ直に 1 行書く（子を起こさずに「印だけの周」を作る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_launched(state: &Path, bead: &str, ts: &str) {
    let fleet = state.join("fleet");
    fs::create_dir_all(&fleet).expect("fleet の dir を作れる");
    let line = format!(
        "{{\"schema\":1,\"ts\":\"{ts}\",\"kind\":\"DispatchMark\",\"bead\":\"{bead}\",\"mark\":\"launched\",\
         \"host\":\"h\",\"actor\":\"machine\",\"detail\":\"run\"}}\n"
    );
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(fleet.join("events.jsonl"))
        .expect("event log を開ける");
    std::io::Write::write_all(&mut log, line.as_bytes()).expect("印の行を書ける");
}

/// (§17 起こす前の印) 列が起こした便は、子の `RunCreated` より**前の行**に bead 名義の `DispatchMark mark=launched`
/// （detail = subcommand の 1 語）を持つ。印を書けない周（event log の lock が外せない）は子を起こさず
/// `started:0`、event log を読めず印を測れない周は `dispatch ls` の理由が `admission:mark`（測れない側）になる。
#[test]
fn pipe_dispatch_launched_mark_is_written_before_the_child_is_spawned() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bd = fake_bd(&state, &[issue("s2-toy.2", 2, "b")]);
    let out = launch_turn(&repo, &state, &bd, "true");
    assert_eq!(stdout_of(&out).trim_end(), "dispatch=started:1,resumed:0,waiting:0", "1 本起こす（{}）", told(&out));
    assert_eq!(created(&state, &["s2-toy.2"], 1), 1, "起こした便の RunCreated が 1 件");
    let bead = "\"bead\":\"s2-toy.2\"";
    let mark = line_at(&state, &[LAUNCHED_MARK[0], LAUNCHED_MARK[1], bead, "\"detail\":\"run\""]);
    let run = line_at(&state, &["\"kind\":\"RunCreated\"", bead]);
    assert!(mark.is_some(), "起こした bead の launched の印が在る（mark={mark:?} run={run:?}）");
    assert!(mark < run, "印は RunCreated より前の行（mark={mark:?} run={run:?}）");
    clean(&[&repo, &state]);

    // **印を書けない周は起こさない**: event log の lock を dir にして外せなくする（読みは lock を取らない）。
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bd = fake_bd(&state, &[issue("s2-toy.2", 2, "b")]);
    let lock = state.join("fleet").join("events.jsonl.lock");
    fs::create_dir_all(&lock).expect("lock の位置に dir を置ける");
    let refused = launch_turn(&repo, &state, &bd, "true");
    assert_eq!(
        stdout_of(&refused).trim_end(),
        "dispatch=started:0,resumed:0,waiting:1",
        "印を書けない便は起こさない（{}）",
        told(&refused)
    );
    // lock を外してから待つ: 子が起きていれば lock の待ちを抜けて RunCreated を書く（子が起きない側の測り）。
    fs::remove_dir(&lock).expect("lock の dir を外せる");
    assert_eq!(run_created_within(&state, "s2-toy.2", 5000), None, "子は起きていない");
    assert_eq!(line_at(&state, &LAUNCHED_MARK), None, "印は書かれていない");
    clean(&[&repo, &state]);

    // **印を測れない周**: event log を dir にして読めなくする（読めないを「印が無い」に読み替えない）。
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bd = fake_bd(&state, &[issue("s2-toy.2", 2, "b")]);
    fs::create_dir_all(state.join("fleet").join("events.jsonl")).expect("event log の位置に dir を置ける");
    let listed = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&listed, "s2-toy.2"), "admission:mark", "測れない側の理由（{}）", told(&listed));
    assert_eq!(count_of(&listed), format!("{COUNT} total=1 ready=0"), "列には載るが起こさない");
    let unmeasured = launch_turn(&repo, &state, &bd, "true");
    assert_eq!(
        stdout_of(&unmeasured).trim_end(),
        "dispatch=started:0,resumed:0,waiting:1",
        "測れない周は起こさない（{}）",
        told(&unmeasured)
    );
    clean(&[&repo, &state]);
}

/// (§17 候補の条件) 最新の `launched` の後に `RunCreated` も `release` も無い bead は起こさず理由が `launched:<ts>`、
/// `RunCreated` が来れば従来の判定（live な自分の便との交差＝`overlap`）に戻り、`release` の後の周は起こす。
#[test]
fn pipe_dispatch_launched_bead_is_not_relaunched_until_run_created_or_release() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let ts = "2026-09-21T00:00:00Z";
    put_launched(&state, "s2-toy.1", ts);
    put_launched(&state, "s2-toy.2", ts);
    let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a"), issue("s2-toy.2", 2, "b")]);
    let marked = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&marked, "s2-toy.1"), format!("launched:{ts}"), "印だけの周（{}）", told(&marked));
    assert_eq!(reason_of(&marked, "s2-toy.2"), format!("launched:{ts}"), "印だけの周（{}）", told(&marked));
    assert_eq!(count_of(&marked), format!("{COUNT} total=2 ready=0"), "母集団 2 件・起こせる 0 本");
    let idle = launch_turn(&repo, &state, &bd, "true");
    assert_eq!(stdout_of(&idle).trim_end(), "dispatch=started:0,resumed:0,waiting:2", "起こし直さない（{}）", told(&idle));
    assert_eq!(created(&state, &["s2-toy.1", "s2-toy.2"], 0), 0, "子は 1 本も起きない");
    // 受付に届いた（RunCreated）bead は従来の判定へ戻る＝live な自分の便と交差する。
    let live = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), "s2-toy.1");
    let arrived = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&arrived, "s2-toy.1"), format!("overlap:{live}/1"), "live の側（{}）", told(&arrived));
    assert_eq!(reason_of(&arrived, "s2-toy.2"), format!("launched:{ts}"), "別の bead の印は外れない");
    // `release` の後の周は起こす（印 1 つで起き直る）。
    release(&state, "s2-toy.2");
    let released = ls(&repo, &state, &bd);
    assert_eq!(reason_of(&released, "s2-toy.2"), "-", "release で印が外れる（{}）", told(&released));
    let again = launch_turn(&repo, &state, &bd, "true");
    assert_eq!(stdout_of(&again).trim_end(), "dispatch=started:1,resumed:0,waiting:1", "release の後は起こす（{}）", told(&again));
    assert_eq!(created(&state, &["s2-toy.2"], 1), 1, "起こした便の RunCreated が 1 件");
    clean(&[&repo, &state]);
}

/// (§17 子の stderr) 列が起こした子の stderr は `<state_dir>/pipe/launch.log` に **append** される（前の行を消さない）。
/// file を開けない周も子は起きる（`started:1`・起動を記録の失敗で止めない）。
#[test]
fn pipe_dispatch_launch_log_keeps_the_child_stderr() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bd = fake_bd(&state, &[issue("s2-toy.2", 2, "b")]);
    let log = state.join("pipe").join("launch.log");
    fs::create_dir_all(state.join("pipe")).expect("pipe の dir を作れる");
    fs::write(&log, "earlier-line\n").expect("前の行を置ける");
    let probe = "launch-log-probe";
    let out = launch_turn(&repo, &state, &bd, &format!("echo {probe} >&2; exit 2"));
    assert_eq!(stdout_of(&out).trim_end(), "dispatch=started:1,resumed:0,waiting:0", "1 本起こす（{}）", told(&out));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let text = || fs::read_to_string(&log).unwrap_or_default();
    while !text().contains(probe) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let kept = text();
    assert!(kept.lines().any(|line| line == probe), "実装役の stderr が子を経て残る: {kept}");
    // 子（`pipe run`）自身の断りの 1 行（出力層の `pipe: ` の行）も残る＝子の stderr の全体が行き先である。
    while !text().lines().any(|line| line.starts_with("pipe: ")) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let kept = text();
    assert!(kept.lines().any(|line| line.starts_with("pipe: ")), "子の断りの 1 行が残る: {kept}");
    assert!(kept.starts_with("earlier-line\n"), "append（前の行を消さない）: {kept}");
    clean(&[&repo, &state]);

    // **開けない周も起こす**: launch.log の位置を dir にして開けなくする。
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bd = fake_bd(&state, &[issue("s2-toy.2", 2, "b")]);
    fs::create_dir_all(state.join("pipe").join("launch.log")).expect("launch.log の位置に dir を置ける");
    let blind = launch_turn(&repo, &state, &bd, "true");
    assert_eq!(stdout_of(&blind).trim_end(), "dispatch=started:1,resumed:0,waiting:0", "開けない周も起こす（{}）", told(&blind));
    assert_eq!(created(&state, &["s2-toy.2"], 1), 1, "起こした便の RunCreated が 1 件");
    clean(&[&repo, &state]);
}

/// 列の写しの遮断器の走行可能の行（倍率 `value`・[`dispatch_rules`] の書き方と同じ字面）。
fn runnable_row(value: u64) -> String {
    format!("id = \"host.runnable_per_core\"\nkind = \"HostRunnablePerCore\"\nvalue = {value}\n")
}

/// 規則の写しを選んで列を 1 周撃つ（`runner` が `None` なら観測の `dispatch ls`・`Some` なら起こす周）。
fn ruled_turn(repo: &Path, state: &Path, bd: &str, rules: &str, runner: Option<&str>) -> Output {
    let lens = review_lens_pass(state);
    let (state, repo) = (state.display().to_string(), repo.display().to_string());
    let mut args: Vec<&str> = vec!["dispatch"];
    if runner.is_none() {
        args.push("ls");
    }
    args.extend(["--state-dir", state.as_str(), "--repo", repo.as_str(), "--rules", rules, "--bd", bd]);
    if let Some(runner) = runner {
        args.extend(["--lens", lens.as_str(), "--runner", runner]);
    }
    run_pipe(&args)
}

/// (§18 列の遮断器) 走行可能の倍率を 0（閾値 0 = 走行可能 1 でも混んでいる＝常に `Busy`）にした周は、ready の
/// bead が 1 本在っても `started:0` で `dispatch ls` の理由が `host-busy`、既定の倍率の周は同じ台帳で `started:1`
/// （正負の対・gate と同じ 1 関数の遮断器）。base は列が遮断器を通さず、0 の周も起こす（RED）。
#[test]
fn pipe_dispatch_host_busy_round_launches_nothing() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let bd = fake_bd(&state, &[issue("s2-toy.2", 2, "b")]);
    let open = fs::read_to_string(dispatch_rules(&state)).unwrap_or_else(|err| panic!("列の写しを読める: {err}"));
    let open_row = runnable_row(HEALTH_PER_CORE_OPEN);
    assert!(open.contains(&open_row), "前提: 既定の写しは遮断器が開く倍率の行を 1 つ持つ");
    let busy = state.join("rules-busy.toml");
    fs::write(&busy, open.replace(&open_row, &runnable_row(0))).unwrap_or_else(|err| panic!("写しを書ける: {err}"));
    let busy = busy.display().to_string();
    let listed = ruled_turn(&repo, &state, &bd, &busy, None);
    assert_eq!(reason_of(&listed, "s2-toy.2"), "host-busy", "混んだ周の理由（{}）", told(&listed));
    assert_eq!(count_of(&listed), format!("{COUNT} total=1 ready=0"), "列には載るが起こさない（{}）", told(&listed));
    let held = ruled_turn(&repo, &state, &bd, &busy, Some("true"));
    assert_eq!(
        stdout_of(&held).trim_end(),
        "dispatch=started:0,resumed:0,waiting:1",
        "混んだ周は 1 本も起こさない（{}）",
        told(&held)
    );
    assert_eq!(created(&state, &["s2-toy.2"], 0), 0, "子は 1 本も起きない");
    // 対: 既定の倍率の周は同じ置き場・同じ台帳で起こす（遮断器の外の理由で止まっていない証拠）。
    let out = launch_turn(&repo, &state, &bd, "true");
    assert_eq!(stdout_of(&out).trim_end(), "dispatch=started:1,resumed:0,waiting:0", "既定の周は起こす（{}）", told(&out));
    assert_eq!(created(&state, &["s2-toy.2"], 1), 1, "起こした便の RunCreated が 1 件");
    clean(&[&repo, &state]);
}

/// 行 a を持つ便を 1 本置き、記帳を `RunCreated`（stage=`Intake`）の 1 行だけに剥がして run id を返す
/// （受付の途中で運転手を失った亡骸の形・run dir の契約の写しは残る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn intake_corpse(repo: &Path, state: &Path) -> String {
    let id = intake_bead(repo, state, &format!("{DESIGN_FILE}#a"), "s2-corpse");
    let log = state.join("fleet").join("events.jsonl");
    let text = fs::read_to_string(&log).expect("event log を読める");
    let mine = format!("\"run\":\"{id}\"");
    let kept: Vec<&str> =
        text.lines().filter(|line| !line.contains(&mine) || line.contains("\"kind\":\"RunCreated\"")).collect();
    fs::write(&log, format!("{}\n", kept.join("\n"))).expect("event log を書ける");
    id
}

/// (§18 受付で止まった便) `RunCreated stage=Intake` だけを持つ便（行 a）と同じ write-set の別 bead を候補にし、
/// 運転手の札の 4 形で対照する: **無い・所有者が死んでいる**周は亡骸を live に数えず候補が起きて `started:1`、
/// **所有者が生きている**（歯の自分の pid）周は理由 `overlap:<run>`、**在るのに読めない**周は起こさず理由が
/// 測れない側（`admission:write-set-unreadable`）。base は `Intake` を無条件に live と読む（無い・死んだ周が RED）。
#[test]
fn pipe_dispatch_intake_run_without_a_live_driver_is_not_live() {
    for form in ["absent", "dead", "live", "unreadable"] {
        let (repo, state) = repo_with_state();
        two_rows(&repo);
        let corpse = intake_corpse(&repo, &state);
        let stages = fs::read_to_string(state.join("fleet").join("events.jsonl")).unwrap_or_default();
        let records = stages.lines().filter(|line| line.contains(&format!("\"run\":\"{corpse}\""))).count();
        assert_eq!(records, 1, "{form}: 前提: 亡骸の記帳は RunCreated の 1 行だけ");
        assert!(!state.join("pipe").join(&corpse).join("driver").exists(), "{form}: 前提: 札は無い");
        match form {
            "dead" => put_dead_ticket(&state, &corpse),
            "live" => put_ticket_body(&state, &corpse, &format!("{}\n", std::process::id())),
            "unreadable" => put_ticket_body(&state, &corpse, "not-a-pid\n"),
            _ => {}
        }
        let bd = fake_bd(&state, &[issue("s2-toy.1", 2, "a")]);
        let listed = ls(&repo, &state, &bd);
        let (reason, line) = match form {
            "absent" | "dead" => ("-".to_owned(), "dispatch=started:1,resumed:0,waiting:0"),
            "live" => (format!("overlap:{corpse}/1"), "dispatch=started:0,resumed:0,waiting:1"),
            _ => ("admission:write-set-unreadable".to_owned(), "dispatch=started:0,resumed:0,waiting:1"),
        };
        assert_eq!(reason_of(&listed, "s2-toy.1"), reason, "{form}: ls の理由（{}）", told(&listed));
        let out = launch_turn(&repo, &state, &bd, "true");
        assert_eq!(stdout_of(&out).trim_end(), line, "{form}: 1 周（{}）", told(&out));
        let want = usize::from(line.contains("started:1"));
        assert_eq!(created(&state, &["s2-toy.1"], want), want, "{form}: 候補の RunCreated の件数");
        clean(&[&repo, &state]);
    }
}

// ───── 終端の周の軸（consumer-sync.md §15 形 2 / 3・`pipe_dispatch_vessel_` 接頭辞・`s2-07l.408`） ─────
//
// 偽 git は `[[vessel]] repo` を名指す呼び出しだけを写して答え（`rev-list` は数・`status` は汚れ・`fetch` は rc）、他は
// 実 git へ exec する（列そのものの git は現物で動く）。偽 cargo は argv を写して install 先の行を出す。

/// 偽 git が vessel repo の `rev-parse HEAD` に返す 40 桁（先頭 12 桁が `updated:` の sha）。
const VESSEL_HEAD: &str = "89abcdef0123456789abcdef0123456789abcdef";

/// 空の 1 周の列の行（`dispatch=` の書式は 1 字も変わらない）。
const IDLE_LINE: &str = "dispatch=started:0,resumed:0,waiting:0";

/// 軸の置き場（偽 git / cargo の PATH と argv の写し）。
struct VesselPlace {
    /// 偽 git / cargo を先頭に置いた PATH。
    path: String,
    /// argv の写し。
    log: std::path::PathBuf,
    /// vessel repo の path（写しの中で `[vessel]` に置き換える）。
    vessel: String,
}

impl VesselPlace {
    /// 写った argv（vessel repo の path は `[vessel]`・撃たれなければ空）。
    fn argv(&self) -> Vec<String> {
        fs::read_to_string(&self.log).unwrap_or_default().lines().map(|line| line.replace(&self.vessel, "[vessel]")).collect()
    }
}

/// 置き場を作る: `declared` なら host の面に `[[vessel]] repo` を書き、偽 git（`rev-list` は `count`・`status` は `status_out`・
/// `fetch` は `fetch_rc`）と偽 cargo（rc 0・install 先を stderr へ）を置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn vessel_place(state: &Path, declared: bool, count: u64, status_out: &str, fetch_rc: u8) -> VesselPlace {
    let dir = state.join("vessel-repo");
    fs::create_dir_all(&dir).expect("vessel repo の dir を作れる");
    let vessel = dir.display().to_string();
    if declared {
        fs::write(state.join("host.toml"), format!("schema = 1\n\n[[vessel]]\nrepo = \"{vessel}\"\n")).expect("host の面を書ける");
    }
    let log = state.join("vessel-argv.log");
    let logged = log.display().to_string();
    let git = format!(
        "case \"$2\" in '{vessel}')\n  printf '%s\\n' \"git $*\" >> '{logged}'\n  case \"$3\" in\n    fetch) exit {fetch_rc} ;;\n    rev-list) echo {count} ;;\n    status) printf '{status_out}' ;;\n    rev-parse) echo {VESSEL_HEAD} ;;\n  esac\n  exit 0 ;;\nesac"
    );
    let path = shim_path(state, "vessel-bin", &git);
    script(
        &state.join("vessel-bin").join("cargo"),
        &format!("printf '%s\\n' \"cargo $*\" >> '{logged}'\necho '  Installing /opt/e2e-bin/vessel' >&2\n"),
    );
    VesselPlace { path, log, vessel }
}

/// 手動の 1 周（起こす側・道具つき・台帳は空）を偽 git / cargo の PATH で撃つ（`verb` は `dispatch` の後ろの語）。
fn vessel_turn(repo: &Path, state: &Path, place: &VesselPlace, verb: &[&str]) -> Output {
    let (state_s, repo_s, rules, bd) =
        (state.display().to_string(), repo.display().to_string(), dispatch_rules(state), fake_bd(state, &[]));
    let mut args = vec!["dispatch"];
    args.extend_from_slice(verb);
    args.extend_from_slice(&["--state-dir", &state_s, "--repo", &repo_s, "--rules", &rules, "--bd", &bd, "--runner", "true"]);
    super::run_pipe_with_path(&place.path, &args)
}

/// 置き場の `InstallRecorded` の件数。
fn installs(state: &Path) -> usize {
    let events = vessel::fleet::store::read_all(state).unwrap_or_default();
    events.iter().filter(|event| event.kind == vessel::fleet::EventKind::InstallRecorded).count()
}

/// 終端の周が fetch を 1 回撃ってから差を数える 2 本。
fn fetch_then_count() -> Vec<String> {
    vec!["git -C [vessel] fetch origin".to_owned(), "git -C [vessel] rev-list --count HEAD..origin/main".to_owned()]
}

/// (形 2・3) live 便 0 の周に上流が 2 個先なら、fetch を 1 回撃ってから数え、§5 の口を 1 回呼んで `updated:<sha12>` と
/// `InstallRecorded` 1 件が残る。`vessel=` の行は列の行の前に立ち、最後の行は列の行のまま。base は行も argv も無い（RED）。
#[test]
fn pipe_dispatch_vessel_behind_on_an_idle_round_fires_update_once() {
    let (repo, state) = repo_with_state();
    let place = vessel_place(&state, true, 2, "", 0);
    let out = vessel_turn(&repo, &state, &place, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "1 周は rc 0（{}）", told(&out));
    let sha = VESSEL_HEAD.get(..12).unwrap_or_default();
    assert_eq!(stdout_of(&out), format!("vessel=updated:{sha}\n{IDLE_LINE}\n"), "軸の行 + 列の行（{}）", told(&out));
    let mut want = fetch_then_count();
    want.extend([
        "git -C [vessel] status --porcelain".to_owned(),
        "git -C [vessel] fetch origin".to_owned(),
        "git -C [vessel] merge --ff-only origin/main".to_owned(),
        format!("cargo install --path [vessel]/crates/{} --locked --color never", vessel::name::NAME),
        "git -C [vessel] rev-parse HEAD".to_owned(),
    ]);
    assert_eq!(place.argv(), want, "fetch 1 回 → 数え → §5 の口 1 回（順序は §5 のまま）");
    assert_eq!(installs(&state), 1, "記帳 1 件");
    clean(&[&repo, &state]);
}

/// (形 2・否定の枝) 上流と同じ周は `current` で口を撃たない（argv は fetch と数えの 2 本・記帳 0 件）。
#[test]
fn pipe_dispatch_vessel_current_round_does_not_update() {
    let (repo, state) = repo_with_state();
    let place = vessel_place(&state, true, 0, "", 0);
    let out = vessel_turn(&repo, &state, &place, &[]);
    assert_eq!(stdout_of(&out), format!("vessel=current\n{IDLE_LINE}\n"), "現行（{}）", told(&out));
    assert_eq!(place.argv(), fetch_then_count(), "口の argv は 0 本");
    assert_eq!(installs(&state), 0, "記帳 0 件");
    clean(&[&repo, &state]);
}

/// (形 2・否定の枝) live な便が残る周は fetch も口も撃たず、差を読んで `behind:<n>` を名乗るだけ（次の終端が拾う）。
#[test]
fn pipe_dispatch_vessel_live_round_only_names_the_lead() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let place = vessel_place(&state, true, 3, "", 0);
    let out = vessel_turn(&repo, &state, &place, &[]);
    assert_eq!(stdout_of(&out), format!("vessel=behind:3\n{IDLE_LINE}\n"), "live 便 {id} が残る（{}）", told(&out));
    assert_eq!(place.argv(), ["git -C [vessel] rev-list --count HEAD..origin/main"], "fetch も口も撃たない");
    assert_eq!(installs(&state), 0, "記帳 0 件");
    clean(&[&repo, &state]);
}

/// (形 2・否定の枝) fetch が落ちた周は `unmeasured:fetch-failed` で数えも口も撃たない（0 と融合しない）。
#[test]
fn pipe_dispatch_vessel_fetch_failure_is_unmeasured_and_does_not_update() {
    let (repo, state) = repo_with_state();
    let place = vessel_place(&state, true, 2, "", 1);
    let out = vessel_turn(&repo, &state, &place, &[]);
    assert_eq!(stdout_of(&out), format!("vessel=unmeasured:fetch-failed\n{IDLE_LINE}\n"), "測れない（{}）", told(&out));
    assert_eq!(place.argv(), ["git -C [vessel] fetch origin"], "fetch の後は何も撃たない");
    assert_eq!(installs(&state), 0, "記帳 0 件");
    clean(&[&repo, &state]);
}

/// (形 2・否定の枝) 汚れた vessel repo は §5 の口が `dirty` で断り、`refused:dirty` を名乗って記帳 0 件（口は status で止まる）。
#[test]
fn pipe_dispatch_vessel_dirty_checkout_is_refused_with_the_word() {
    let (repo, state) = repo_with_state();
    let place = vessel_place(&state, true, 2, " M src/lib.rs\\n", 0);
    let out = vessel_turn(&repo, &state, &place, &[]);
    assert_eq!(stdout_of(&out), format!("vessel=refused:dirty\n{IDLE_LINE}\n"), "断りの語（{}）", told(&out));
    let mut want = fetch_then_count();
    want.push("git -C [vessel] status --porcelain".to_owned());
    assert_eq!(place.argv(), want, "口は status で止まる");
    assert_eq!(installs(&state), 0, "記帳 0 件");
    clean(&[&repo, &state]);
}

/// (形 1・3・否定の枝) `[[vessel]]` の無い置き場は読みの 1 本が git を撃たずに `undeclared` を返し、1 周は軸を評価しない
/// （git の argv 0・`vessel=` の行は立たず、stdout は列の行 1 行だけ＝既存の歯の字面は動かない）。
#[test]
fn pipe_dispatch_vessel_undeclared_shoots_no_git() {
    use vessel::hook::vessel::{upstream, Upstream, DEFAULT_BRANCH, DEFAULT_REMOTE};
    let (repo, state) = repo_with_state();
    let place = vessel_place(&state, false, 2, "", 0);
    let out = vessel_turn(&repo, &state, &place, &[]);
    assert_eq!(stdout_of(&out), format!("{IDLE_LINE}\n"), "列の行だけ（{}）", told(&out));
    assert!(place.argv().is_empty(), "git も cargo も撃たない: {:?}", place.argv());
    let read = upstream(None, DEFAULT_REMOTE, DEFAULT_BRANCH);
    assert_eq!(read, Upstream::Undeclared, "宣言なしの値");
    assert_eq!(read.render(), "undeclared");
    clean(&[&repo, &state]);
}

/// (形 2・否定の枝) 見る側の 1 周（`dispatch ls`）は宣言が在って上流が先でも git も cargo も 1 本も撃たず、行に token が載らない。
#[test]
fn pipe_dispatch_vessel_ls_shoots_nothing() {
    let (repo, state) = repo_with_state();
    let place = vessel_place(&state, true, 2, "", 0);
    let out = vessel_turn(&repo, &state, &place, &["ls"]);
    assert_eq!(stdout_of(&out).trim_end(), NONE_LINE, "見る側の行だけ（{}）", told(&out));
    assert!(place.argv().is_empty(), "見るだけで撃たない: {:?}", place.argv());
    assert_eq!(installs(&state), 0, "記帳 0 件");
    clean(&[&repo, &state]);
}

/// (形 2・3) 便の終端（`pipe stop`）の 1 周も同じ軸を撃つ: 最後の live 便が止まった周は live 0 で口を 1 回呼び、終端の
/// stdout に `vessel=` の行が 1 行立つ（`--drive` の無い周なので `dispatch=` の行は出ない＝観測の面は増えない）。
#[test]
fn pipe_dispatch_vessel_terminal_round_updates_after_the_last_live_run() {
    let (repo, state) = repo_with_state();
    two_rows(&repo);
    let live = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), "s2-live");
    let place = vessel_place(&state, true, 1, "", 0);
    let (state_s, repo_s, rules, bd) =
        (state.display().to_string(), repo.display().to_string(), dispatch_rules(&state), fake_bd(&state, &[]));
    let out = super::run_pipe_with_path(
        &place.path,
        &["stop", "--run", &live, "--state-dir", &state_s, "--repo", &repo_s, "--rules", &rules, "--bd", &bd, "--runner", "true"],
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop は rc 0（{}）", told(&out));
    let sha = VESSEL_HEAD.get(..12).unwrap_or_default();
    let stdout = stdout_of(&out);
    let lines: Vec<&str> = stdout.lines().filter(|line| line.starts_with("vessel=")).collect();
    assert_eq!(lines, [format!("vessel=updated:{sha}")], "軸の行 1 行（{}）", told(&out));
    assert!(!stdout.contains("dispatch="), "列の行は出ない（{}）", told(&out));
    assert_eq!(installs(&state), 1, "記帳 1 件");
    clean(&[&repo, &state]);
}

// ───── 群の逼迫の通知（account-lifecycle.md §19 形 2〜4・7・契約表の行 h・`s2-07l.491`・接頭辞 `pipe_dispatch_group_`） ─────
//
// 偽 usage client（stdin の token で口座ごとの本文を返す script・呼ばれた argv を写す）と偽 tmux（`send-keys -l` の payload を
// 偽の pane へ写し Enter で本文へ移す）を置き、群は host の面（`host.toml`）に、席は core の `register` で積む。閾値は
// 埋め込みと同じ値（85 / 95 / 95）を写しの manifest に持つ。群の置き場は 2 つ（[`GROUP_ANCHORS`]）。

/// 群の歯の群の名。
const GROUP: &str = "g";

/// 群の置き場 2 つと、その席の target（偽 tmux は target を読まない）。
const GROUP_ANCHORS: [(&str, &str); 2] = [("/g/one", "gone:0"), ("/g/two", "gtwo:0")];

/// 群の歯の窓が開き直る時刻（遠い未来の番兵＝選定と鮮度が古いと読まない・時限にならない）。
const GROUP_FAR: &str = "2099-01-01T00:00:00Z";

/// 鮮度の外の実測の ts（写しの鮮度 3600 秒より十分古い）。
const GROUP_STALE_TS: &str = "2026-09-12T02:00:00Z";

/// 群の歯の置き場（toy repo・置き場・規則の写し・空の台帳・偽 client）。
struct GroupPlace {
    /// toy repo（列の `--repo`）。
    repo: std::path::PathBuf,
    /// 置き場。
    state: std::path::PathBuf,
    /// 規則の写し（列の行 + 計測の 2 行 + 閾値の 3 行）。
    rules: String,
    /// 空の台帳を返す偽の `bd`。
    bd: String,
    /// 偽 usage client。
    curl: String,
}

/// 口座 1 つの本文（5 時間窓・7 日窓・モデル別窓〔Fable〕の使用率）。
fn group_body(five: u64, seven: u64, model: u64) -> String {
    format!(
        "{{\"five_hour\":{{\"utilization\":{five},\"resets_at\":\"{GROUP_FAR}\"}},\"seven_day\":{{\"utilization\":{seven},\
         \"resets_at\":\"{GROUP_FAR}\"}},\"limits\":[{{\"kind\":\"weekly_scoped\",\"percent\":{model},\"resets_at\":\
         \"{GROUP_FAR}\",\"scope\":{{\"model\":{{\"display_name\":\"Fable\"}}}}}}]}}"
    )
}

/// 群の宣言 1 つ（名・置き場〔[`GROUP_ANCHORS`] の添字の列〕・候補の口座の列）。
type GroupDecl<'a> = (&'a str, &'a [usize], &'a [&'a str]);

/// 群の歯の置き場を作る: 口座ごと（label, 5h, 7d, model）に credential と本文を置き、host の面に口座と群（置き場 =
/// [`GROUP_ANCHORS`]・候補 = `candidates`）を書く（`grouped` が偽なら群の表を書かない＝群 0 の host）。
fn group_place(accounts: &[(&str, u64, u64, u64)], candidates: &[&str], grouped: bool) -> GroupPlace {
    let both: &[usize] = &[0, 1];
    if grouped {
        groups_place(accounts, &[(GROUP, both, candidates)])
    } else {
        groups_place(accounts, &[])
    }
}

/// [`group_place`] の群を宣言の列で渡す形（群 2 つの歯・§20）。規則の写しには起こし直しの確認の 2 行（settle 2 秒・刻み
/// 100 ms）も足す（移動の周の `launch` の 1 本が読む）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn groups_place(accounts: &[(&str, u64, u64, u64)], groups: &[GroupDecl<'_>]) -> GroupPlace {
    let (repo, state) = super::repo_with_state();
    let spy = state.join("spy");
    fs::create_dir_all(&spy).expect("偽 client の dir を作れる");
    let mut host = "schema = 1\n".to_owned();
    for (label, five, seven, model) in accounts {
        host.push_str(&format!("\n[[account]]\nlabel = \"{label}\"\n"));
        let dir = state.join("accounts").join(label);
        fs::create_dir_all(&dir).expect("credential の dir を作れる");
        let credential = format!(
            "{{\"claudeAiOauth\":{{\"accessToken\":\"tok-{label}\",\"refreshToken\":\"r\",\"expiresAt\":4102444800000}}}}"
        );
        fs::write(dir.join(".credentials.json"), credential).expect("credential を書ける");
        fs::write(spy.join(format!("body-tok-{label}")), group_body(*five, *seven, *model)).expect("本文を書ける");
    }
    let quoted = |items: Vec<&str>| items.iter().map(|item| format!("\"{item}\"")).collect::<Vec<String>>().join(", ");
    for (name, anchors, candidates) in groups {
        let anchors: Vec<&str> = anchors.iter().filter_map(|at| GROUP_ANCHORS.get(*at)).map(|(anchor, _)| *anchor).collect();
        host.push_str(&format!(
            "\n[[account-group]]\nname = \"{name}\"\nanchors = [{}]\naccounts = [{}]\n",
            quoted(anchors),
            quoted(candidates.to_vec())
        ));
    }
    fs::write(state.join("host.toml"), host).expect("host の面を書ける");
    let d = spy.display();
    let curl = script(
        &spy.join("curl"),
        &format!(
            "printf '%s\\n' \"$@\" >> \"{d}/args\"\ncfg=$(cat)\nfor f in \"{d}\"/body-*; do\n\
             case \"$cfg\" in *\"Bearer ${{f##*/body-}}\\\"\"*) cat \"$f\" ;; esac\ndone\nprintf '\\n%s' '200'\n"
        ),
    );
    let row = |id: &str, kind: &str, value: u64| {
        format!("[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n")
    };
    let base = fs::read_to_string(dispatch_rules(&state)).expect("列の写しを読める");
    let rules = [
        row("fleet.usage_timeout_s", "UsageTimeoutS", 10),
        row("fleet.usage_fresh_s", "UsageFreshS", 3600),
        row("fleet.group_pressure_5h_pct", "GroupPressure5hPct", 85),
        row("fleet.group_pressure_7d_pct", "GroupPressure7dPct", 95),
        row("fleet.group_pressure_model_pct", "GroupPressureModelPct", 95),
        row("seat.cycle_settle_s", "SeatCycleSettleS", 2),
        row("seat.cycle_poll_ms", "SeatCyclePollMs", 100),
    ]
    .iter()
    .fold(base, |text, found| format!("{text}\n{found}"));
    let path = state.join("rules-group.toml");
    fs::write(&path, rules).expect("群の写しを書ける");
    group_tmux(&state);
    let bd = fake_bd(&state, &[]);
    GroupPlace { repo, state, rules: path.display().to_string(), bd, curl }
}

/// 偽 tmux を道具箱の dir に置く（`send-keys -l` は入力欄の file へ・Enter は入力欄を pane へ移して宛先の席の打刻 file に
/// `UserPromptSubmit` の 1 行を足す＝消費の証拠が窓を待たずに届く・`capture-pane` は pane の本文と prompt 行を返す・どの起動も
/// 引数を `tmux-calls` へ 1 行で足す）。席の打刻 file は空で先に置く（hook の載った席）。
///
/// §20 の移動の歯のために、席の前面を target ごとの file（`spy/front-<target>`・無ければ席＝`claude`）で持つ: 退避の合図
/// （`group: evacuate`）を消費した席は前面が shell（`bash`）に戻り（`spy/stuck-<target>` が在る席は戻らない）、shell の前面は
/// `list-panes` で `bash`・`capture-pane` で `$ ` の prompt を返す。shell の前面へ届いた Enter は起動行と読み、送った行を
/// `spy/launched-<target>` へ・時刻（ns）を `spy/launch-at-<target>` へ・その瞬間の口座の `.claude.json`（在るものを連結）を
/// `spy/claude-at-<target>` へ写して `SessionStart` の打刻を足し、前面を席に戻す（host-init.md §7 の書きと送りの順を外から測る）。
/// 退避の合図を受けた瞬間の時刻（ns）・event log の承認の行数・記録の有無を `spy/{evacuate-at,moved-at-evacuate,record-at-evacuate}-<target>`
/// へ写す（4 手の順を外から測る）。`list-windows` は窓 `0` を返す（[`GROUP_ANCHORS`] の target の窓）。
///
/// §22 の歯のために、席の前面の可視域を target ごとの file（`spy/screen-<target>`）で作り分ける: 在る席の `capture-pane` は
/// prompt 行の代わりにその字面を返し、その席へ届いた Enter は画面を消して前面を shell に戻す（dialog の既定の行の確定）。
/// `spy/dialog-<target>` の在る席は、入力欄が `/exit` の周に届いた Enter で本文を pane へ移さず（echo も打刻も無い＝送達は
/// 未確認）その字面を `screen-<target>` へ写す（`/exit` の確認 dialog が出た席）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn group_tmux(state: &Path) {
    let bin = state.join(crate::TOOLBOX_BIN);
    fs::create_dir_all(&bin).expect("道具箱の dir を作れる");
    let (pane, input, calls) = (state.join("tmux-pane"), state.join("tmux-input"), state.join("tmux-calls"));
    fs::write(&pane, "").expect("偽の pane を作れる");
    fs::write(&input, "").expect("偽の入力欄を作れる");
    for (_, target) in GROUP_ANCHORS {
        let seat = state.join("seat").join(target.replace(':', "_"));
        fs::create_dir_all(&seat).expect("席の置き場を作れる");
        fs::write(seat.join("state.jsonl"), "").expect("打刻 file を作れる");
    }
    let (seats, spy) = (state.join("seat"), state.join("spy"));
    let (events, record) = (state.join("fleet").join("events.jsonl"), groups_dir(state).join(format!("{GROUP}.account")));
    let (pane, input, calls, seats, spy) = (pane.display(), input.display(), calls.display(), seats.display(), spy.display());
    let (events, record, accounts) = (events.display(), record.display(), state.join("accounts"));
    let accounts = accounts.display();
    script(
        &bin.join("tmux"),
        &format!(
            "printf '%s\\n' \"$*\" >> '{calls}'\nt=''; p=''\nfor a in \"$@\"; do [ \"$p\" = '-t' ] && t=\"$a\"; p=\"$a\"; done\n\
             f=$(printf '%s' \"$t\" | tr ':' '_')\nfront='{spy}/front-'\"$f\"\nshell=$(cat \"$front\" 2>/dev/null)\n\
             case \"$1\" in\n\
             list-panes) if [ \"$shell\" = bash ]; then echo bash; else echo claude; fi;;\n\
             list-windows) echo 0;;\n\
             capture-pane) cat '{pane}'; if [ \"$shell\" = bash ]; then printf '$ \\n'; elif [ -f '{spy}/screen-'\"$f\" ]; then \
             cat '{spy}/screen-'\"$f\"; else printf '\\342\\235\\257 '; cat '{input}'; printf '\\n'; fi;;\n\
             send-keys) if [ \"$4\" = \"-l\" ]; then printf '%s' \"$5\" >> '{input}'\n\
             elif [ \"$4\" = \"Enter\" ] && [ \"$shell\" = bash ]; then date +%s%N > '{spy}/launch-at-'\"$f\"\n\
             cat '{accounts}'/*/.claude.json > '{spy}/claude-at-'\"$f\" 2>/dev/null\n\
             cat '{input}' >> '{spy}/launched-'\"$f\"; printf '\\n' >> '{spy}/launched-'\"$f\"; : > '{input}'; echo claude > \"$front\"\n\
             printf '{{\"schema\":1,\"state\":\"idle\",\"event\":\"SessionStart\",\"ts\":%s,\"sid\":\"\"}}\\n' \"$(date +%s)\" \
             >> '{seats}/'\"$f\"'/state.jsonl'\n\
             elif [ \"$4\" = \"Enter\" ] && [ -f '{spy}/screen-'\"$f\" ]; then rm -f '{spy}/screen-'\"$f\"; echo bash > \"$front\"\n\
             elif [ \"$4\" = \"Enter\" ] && [ -f '{spy}/dialog-'\"$f\" ] && [ \"$(cat '{input}')\" = /exit ]; then \
             cp '{spy}/dialog-'\"$f\" '{spy}/screen-'\"$f\"; : > '{input}'\n\
             elif [ \"$4\" = \"Enter\" ]; then\n\
             if grep -q 'group: evacuate' '{input}'; then date +%s%N > '{spy}/evacuate-at-'\"$f\"\n\
             grep -c '\"kind\":\"GroupMoved\"' '{events}' > '{spy}/moved-at-evacuate-'\"$f\"\n\
             if [ -f '{record}' ]; then echo 1 > '{spy}/record-at-evacuate-'\"$f\"; fi\n\
             if [ ! -f '{spy}/stuck-'\"$f\" ]; then echo bash > \"$front\"; fi; fi\n\
             cat '{input}' >> '{pane}'; printf '\\n' >> '{pane}'; : > '{input}'\n\
             printf '{{\"schema\":1,\"state\":\"busy\",\"event\":\"UserPromptSubmit\",\"ts\":%s,\"sid\":\"\"}}\\n' \"$(date +%s)\" \
             >> '{seats}/'\"$f\"'/state.jsonl'; fi;;\n\
             esac\nexit 0\n"
        ),
    );
}

/// host の根の群用 dir（`<置き場の親>/<NAME>-host/groups`・器の字面を借りない）。
fn groups_dir(state: &Path) -> std::path::PathBuf {
    state.parent().unwrap_or(state).join(format!("{}-host", vessel::name::NAME)).join("groups")
}

/// 群の置き場の席の登録 row を積む（口座は置き場ごとに選ぶ）。
fn group_seats(state: &Path, accounts: [&str; 2]) {
    for ((anchor, target), account) in GROUP_ANCHORS.iter().zip(accounts) {
        let row = vessel::fleet::Registration {
            role: vessel::seat::role::Role::Orchestrator,
            anchor: (*anchor).to_owned(),
            target: (*target).to_owned(),
            sid: None,
            account: account.to_owned(),
            launch: String::new(),
            model: None,
        };
        assert!(vessel::seat::role::register(state, row).is_ok(), "登録 row を積める");
    }
}

/// 口座 1 つの実測の回（5h / 7d / model〔Fable〕）を `ts` で置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_group_round(state: &Path, ts: &str, account: &str, (five, seven, model): (u64, u64, u64)) {
    use vessel::fleet::{Allowance, Event, EventKind, Measured, WindowKind};
    let policy = vessel::fleet::store::LockPolicy::embedded().expect("lock の規則を読める");
    for (window, used_pct) in [(WindowKind::FiveHour, five), (WindowKind::SevenDay, seven), (WindowKind::SevenDayModel, model)] {
        let measured = Measured {
            account: account.to_owned(),
            window,
            model: (window == WindowKind::SevenDayModel).then(|| "Fable".to_owned()),
            endpoint: "oauth-usage".to_owned(),
            used_pct,
            resets_at: Some(GROUP_FAR.to_owned()),
        };
        let event = Event {
            schema: vessel::fleet::SCHEMA,
            ts: ts.to_owned(),
            kind: EventKind::AllowanceMeasured,
            run: String::new(),
            bead: String::new(),
            host: "h".to_owned(),
            actor: EventKind::AllowanceMeasured.default_actor().to_owned(),
            stage: None,
            seat: None,
            pid: None,
            detail: None,
            allowance: Some(Allowance::Measured(measured)),
            registration: None,
            mark: None,
            account: None,
            cost: None,
            rule: None,
        };
        vessel::fleet::store::append(state, &event, policy).expect("実測を置ける");
    }
}

/// いまの UTC の ts（鮮度の内側）。
fn group_now() -> String {
    vessel::fleet::cli::now_utc()
}

/// 道具（`--runner`）つきの**終端の周**を 1 回撃つ: live な便 `run` を置いて `pipe stop --run` で止める（台帳は空）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn group_terminal(place: &GroupPlace, run: &str) -> Output {
    let state = place.state.display().to_string();
    let recorded = super::bin_cmd()
        .args(["fleet", "record", "--kind", "RunStage", "--run", run, "--bead", "s2-group.1", "--stage", "Implemented"])
        .args(["--detail", "implemented", "--state-dir", &state])
        .output()
        .expect("binary を起動できる");
    assert_eq!(recorded.status.code(), Some(i32::from(RC_OK)), "live な便を置ける: {}", told(&recorded));
    let repo = place.repo.display().to_string();
    let out = run_pipe(&[
        "stop", "--run", run, "--state-dir", &state, "--repo", &repo, "--rules", &place.rules, "--bd", &place.bd,
        "--curl", &place.curl, "--runner", "true",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop は rc 0（{}）", told(&out));
    out
}

/// 手動の 1 周（`runner` が真なら道具つき・偽なら道具の無い周）か、見る側の 1 周（`ls`）を撃つ。
fn group_turn(place: &GroupPlace, verb: &[&str], runner: bool) -> Output {
    let (state, repo) = (place.state.display().to_string(), place.repo.display().to_string());
    let mut args = vec!["dispatch"];
    args.extend_from_slice(verb);
    args.extend(["--state-dir", state.as_str(), "--repo", repo.as_str(), "--rules", place.rules.as_str()]);
    args.extend(["--bd", place.bd.as_str(), "--curl", place.curl.as_str()]);
    if runner {
        args.extend(["--runner", "true"]);
    }
    run_pipe(&args)
}

/// 偽 tmux が受けた payload の送り（`send-keys -t <target> -l <payload>` の行）。
fn group_sends(state: &Path) -> Vec<String> {
    fs::read_to_string(state.join("tmux-calls"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("send-keys ") && line.contains(" -l "))
        .map(str::to_owned)
        .collect()
}

/// 偽 client の呼出の回数（口座 1 つにつき 1 回）。
fn group_calls(state: &Path) -> usize {
    fs::read_to_string(state.join("spy").join("args")).unwrap_or_default().lines().filter(|arg| *arg == "--max-time").count()
}

/// 置き場の群の逼迫の通知の event（口座 label と本体・log の順）。
fn group_notices(state: &Path) -> Vec<(String, vessel::fleet::Pressure)> {
    vessel::fleet::store::read_all(state)
        .unwrap_or_default()
        .iter()
        .filter(|event| event.kind == vessel::fleet::EventKind::GroupPressureNotified)
        .filter_map(|event| Some((event.account.clone()?, event.pressure()?)))
        .collect()
}

/// 逼迫の 1 行の payload（器の字面を借りない）。
fn group_payload(account: &str, window: &str, used: u64, cap: u64) -> String {
    format!("{} group: pressure group={GROUP} account={account} window={window} used={used} cap={cap}", vessel::name::NAME)
}

/// 閾値未満の種 a0 に鮮度の内側の実測を置く（偽 client を呼ばない）。§20 以後、群の今の口座（種）が逼迫の周は移動の段へ進み
/// §19 の通知を送らないので、通知の歯は今の口座をこの種に置き、席の登録 row の口座を逼迫にする（候補の列の先頭を `a0` にする）。
fn quiet_seed(place: &GroupPlace) {
    put_group_round(&place.state, &group_now(), "a0", (10, 10, 10));
}

/// 2 つの置き場の席へ `payload` がちょうど 1 回ずつ届いたこと（宛先は登録 row の target）。
fn assert_both_seats(sends: &[String], payload: &str) {
    for (_, target) in GROUP_ANCHORS {
        let hits = sends.iter().filter(|line| line.contains(&format!("-t {target} -l ")) && line.ends_with(payload)).count();
        assert_eq!(hits, 1, "{target} へ 1 回: {sends:?}");
    }
}

/// (群 0) 群を宣言しない host は、逼迫の口座と席の登録 row が在っても行 0・event 0・偽 client の呼出 0（群の段は 1 語も
/// 出さない＝既存の終端の周のまま）。
#[test]
fn pipe_dispatch_group_zero_groups_sends_and_records_nothing() {
    let place = group_place(&[("a1", 90, 10, 10)], &["a1"], false);
    group_seats(&place.state, ["a1", "a1"]);
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(group_sends(&place.state), Vec::<String>::new(), "行 0（{}）", told(&out));
    assert_eq!(group_notices(&place.state), Vec::new(), "event 0");
    assert_eq!(group_calls(&place.state), 0, "計測も撃たない");
    assert!(!stdout_of(&out).contains("group="), "stdout に群の語は無い（{}）", told(&out));
    clean(&[&place.repo, &place.state]);
}

/// (5 時間窓) 口座 a1 の 5 時間窓 90（行の値 85 以上）の群は、2 つの置き場の席へ各 1 行・event 1 件（口座 a1・窓 5h・
/// 送り先 2）。道具つきの終端の周で撃つ。今の口座は閾値未満の種 a0（[`quiet_seed`]・§20 以後の通知の形）。
#[test]
fn pipe_dispatch_group_five_hour_seed_pressure_reaches_both_anchor_seats() {
    let place = group_place(&[("a0", 10, 10, 10), ("a1", 90, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&place);
    group_seats(&place.state, ["a1", "a1"]);
    let out = group_terminal(&place, "r-group-1");
    let sends = group_sends(&place.state);
    assert_eq!(sends.len(), 2, "置き場ごとに 1 行（{}）: {sends:?}", told(&out));
    assert_both_seats(&sends, &group_payload("a1", "5h", 90, 85));
    let notices = group_notices(&place.state);
    assert_eq!(notices.len(), 1, "event 1 件: {notices:?}");
    let (account, body) = notices.first().cloned().expect("event が在る");
    assert_eq!((account.as_str(), body.group.as_str(), body.window.short()), ("a1", GROUP, "5h"), "{body:?}");
    assert_eq!((body.used, body.cap, body.sent), (90, 85, 2), "値・行の値・送り先の数");
    clean(&[&place.repo, &place.state]);
}

/// (鮮度) 鮮度の内側の実測（いまの ts・5 時間窓 90）を持つ口座は偽 client を 1 回も起こさずに置いた実測で通知し、鮮度の外
/// （古い ts）の口座は 1 回だけ測って測った値で通知する（今の口座は鮮度の内側の種 a0）。
#[test]
fn pipe_dispatch_group_fresh_account_is_not_remeasured_but_stale_is_once() {
    let fresh = group_place(&[("a0", 10, 10, 10), ("a1", 10, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&fresh);
    group_seats(&fresh.state, ["a1", "a1"]);
    put_group_round(&fresh.state, &group_now(), "a1", (90, 10, 10));
    group_terminal(&fresh, "r-group-1");
    assert_eq!(group_calls(&fresh.state), 0, "鮮度の内側は呼出 0");
    assert_both_seats(&group_sends(&fresh.state), &group_payload("a1", "5h", 90, 85));
    clean(&[&fresh.repo, &fresh.state]);
    let stale = group_place(&[("a0", 10, 10, 10), ("a1", 90, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&stale);
    group_seats(&stale.state, ["a1", "a1"]);
    put_group_round(&stale.state, GROUP_STALE_TS, "a1", (10, 10, 10));
    group_terminal(&stale, "r-group-1");
    assert_eq!(group_calls(&stale.state), 1, "鮮度の外は呼出 1");
    assert_both_seats(&group_sends(&stale.state), &group_payload("a1", "5h", 90, 85));
    clean(&[&stale.repo, &stale.state]);
}

/// (2 度目) 同じ実測のまま 2 周目を撃つと、鮮度の内側なので測らず、前回の通知より新しい実測が無いので送らず記さない
/// （行は 2 のまま・event は 1 のまま・呼出は 1 のまま）。
#[test]
fn pipe_dispatch_group_same_measurement_is_not_notified_twice() {
    let place = group_place(&[("a0", 10, 10, 10), ("a1", 90, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&place);
    group_seats(&place.state, ["a1", "a1"]);
    group_terminal(&place, "r-group-1");
    assert_eq!((group_sends(&place.state).len(), group_notices(&place.state).len()), (2, 1), "1 周目は送って記す");
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(group_sends(&place.state).len(), 2, "2 周目は送らない（{}）", told(&out));
    assert_eq!(group_notices(&place.state).len(), 1, "2 周目は記さない");
    assert_eq!(group_calls(&place.state), 1, "2 周目は測らない（鮮度の内側）");
    clean(&[&place.repo, &place.state]);
}

/// (再通知) 通知の後に**同じ値**の新しい実測が event log に入った周は、値が同じでも再び通知する（log の順序で判じる＝
/// 「通知済みなら永久に送らない」と「値が同じなら送らない」の 2 変異を捕まえる）。
#[test]
fn pipe_dispatch_group_new_measurement_with_the_same_value_notifies_again() {
    let place = group_place(&[("a0", 10, 10, 10), ("a1", 90, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&place);
    group_seats(&place.state, ["a1", "a1"]);
    group_terminal(&place, "r-group-1");
    assert_eq!(group_notices(&place.state).len(), 1, "1 周目は記す");
    put_group_round(&place.state, &group_now(), "a1", (90, 10, 10));
    group_terminal(&place, "r-group-2");
    let notices = group_notices(&place.state);
    assert_eq!(notices.len(), 2, "新しい実測の後は再び記す: {notices:?}");
    assert_eq!(notices.last().map(|(_, body)| body.used), Some(90), "値は同じ 90");
    assert_eq!(group_sends(&place.state).len(), 4, "再び 2 つの席へ");
    assert_eq!(group_calls(&place.state), 1, "2 周目は置いた実測を読む（測らない）");
    clean(&[&place.repo, &place.state]);
}

/// (席の口座) 種 a1 は閾値未満で、席の登録 row の口座 a2 だけが逼迫（5 時間窓 90）の周も (群, a2) の 1 行を送る
/// （種 a1 の行は無い＝群の今の口座だけを測る変異を捕まえる）。
#[test]
fn pipe_dispatch_group_seat_account_pressure_is_notified() {
    let place = group_place(&[("a1", 10, 10, 10), ("a2", 90, 10, 10)], &["a1", "a2"], true);
    group_seats(&place.state, ["a2", "a2"]);
    group_terminal(&place, "r-group-1");
    assert_both_seats(&group_sends(&place.state), &group_payload("a2", "5h", 90, 85));
    let accounts: Vec<String> = group_notices(&place.state).into_iter().map(|(account, _)| account).collect();
    assert_eq!(accounts, vec!["a2".to_owned()], "通知は a2 の 1 件だけ");
    clean(&[&place.repo, &place.state]);
}

/// (種) 席が種と違う閾値未満の口座 a2 に居て、種 a1 だけが逼迫する周も種を測って逼迫と判じる（他の歯は席を種と同じ口座に
/// 置くので、測る集合から種を外す変異はここで落ちる）。§20 以後、今の口座（種 a1）の逼迫は §19 の通知でなく移動の周になる:
/// 群の今の口座の記録は a2（候補の次・3 窓とも閾値未満）・§19 の通知は 0・席は既に a2 に居るので退避の合図も起動も 0。
#[test]
fn pipe_dispatch_group_seed_pressure_is_notified_while_the_seats_sit_elsewhere() {
    let place = group_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], true);
    group_seats(&place.state, ["a2", "a2"]);
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a2"), "種 a1 の逼迫で a2 へ移る（{}）", told(&out));
    assert_eq!(group_notices(&place.state), Vec::new(), "§19 の通知は送らない");
    assert_eq!(group_sends(&place.state), Vec::<String>::new(), "席は既に a2＝退避の合図も起動行も無い");
    assert_eq!(group_calls(&place.state), 2, "種と席の口座を 1 回ずつ測る");
    clean(&[&place.repo, &place.state]);
}

/// (窓ごとの行) 7 日窓 90（5 時間窓の行 85 なら越える値）は通知せず、7 日窓 96 は window=7d cap=95 で通知する。
#[test]
fn pipe_dispatch_group_caps_differ_per_window() {
    for (seven, notified) in [(90, false), (96, true)] {
        let place = group_place(&[("a0", 10, 10, 10), ("a1", 10, seven, 10)], &["a0", "a1"], true);
        quiet_seed(&place);
        group_seats(&place.state, ["a1", "a1"]);
        group_terminal(&place, "r-group-1");
        let sends = group_sends(&place.state);
        match notified {
            false => assert_eq!((sends.len(), group_notices(&place.state).len()), (0, 0), "7 日窓 {seven} は通知しない"),
            true => assert_both_seats(&sends, &group_payload("a1", "7d", seven, 95)),
        }
        clean(&[&place.repo, &place.state]);
    }
}

/// (モデル別窓) 5 時間窓と 7 日窓は閾値未満（10）で、モデル別窓（`fleet.group_pressure_model_pct`・実測行の窓は
/// `SevenDayModel`）だけが 96 の口座も通知し、window は model（3 窓それぞれを別々の歯が pin する）。
#[test]
fn pipe_dispatch_group_model_window_alone_is_notified_as_model() {
    let place = group_place(&[("a0", 10, 10, 10), ("a1", 10, 10, 96)], &["a0", "a1"], true);
    quiet_seed(&place);
    group_seats(&place.state, ["a1", "a1"]);
    group_terminal(&place, "r-group-1");
    assert_both_seats(&group_sends(&place.state), &group_payload("a1", "model", 96, 95));
    let windows: Vec<&str> = group_notices(&place.state).iter().map(|(_, body)| body.window.short()).collect();
    assert_eq!(windows, vec!["model"], "event の窓も model");
    clean(&[&place.repo, &place.state]);
}

/// (見る側) 逼迫の群が在る同じ fixture で `dispatch ls` の周は行 0・event 0・偽 client の呼出 0（群の段は起こす側の
/// 1 周だけが持つ）。対: 同じ置き場の起こす側の手動の 1 周は通知する。
#[test]
fn pipe_dispatch_group_ls_round_sends_nothing() {
    let place = group_place(&[("a0", 10, 10, 10), ("a1", 90, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&place);
    group_seats(&place.state, ["a1", "a1"]);
    let listed = group_turn(&place, &["ls"], false);
    assert_eq!(stdout_of(&listed).trim_end(), NONE_LINE, "見る側の行だけ（{}）", told(&listed));
    assert_eq!(group_sends(&place.state), Vec::<String>::new(), "行 0");
    assert_eq!(group_notices(&place.state), Vec::new(), "event 0");
    assert_eq!(group_calls(&place.state), 0, "呼出 0");
    let fired = group_turn(&place, &[], true);
    assert_eq!(fired.status.code(), Some(i32::from(RC_OK)), "{}", told(&fired));
    assert_eq!(group_notices(&place.state).len(), 1, "起こす側の 1 周は通知する（{}）", told(&fired));
    clean(&[&place.repo, &place.state]);
}

/// (道具なし) `--runner` の無い手動の 1 周（台帳を読まない周・列は `no-runner`）でも群の段は走り、逼迫の種に 1 行ずつ +
/// event 1（他の歯は道具つきの終端の周で撃つ＝道具の有無の両側を別々の歯が pin する）。
#[test]
fn pipe_dispatch_group_round_without_runner_still_notifies() {
    let place = group_place(&[("a0", 10, 10, 10), ("a1", 90, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&place);
    group_seats(&place.state, ["a1", "a1"]);
    let out = group_turn(&place, &[], false);
    assert_eq!(stdout_of(&out).trim_end(), "dispatch=unmeasured reason=no-runner", "列は測らない（{}）", told(&out));
    assert_both_seats(&group_sends(&place.state), &group_payload("a1", "5h", 90, 85));
    assert_eq!(group_notices(&place.state).len(), 1, "event 1");
    clean(&[&place.repo, &place.state]);
}

// ───── 群の自動の移動（account-lifecycle.md §20・契約表の行 i・接頭辞 `pipe_dispatch_group_move_`） ─────
//
// §19 の fixture（偽 usage client・偽 tmux・host の面の群）に、偽 tmux の前面の file（退避の合図で shell へ戻る・shell の前面で
// 受けた Enter を起動行として `spy/launched-<target>` へ写す）を足して撃つ。群の今の口座の記録と移動を頼む記録は host の根の
// 群用 dir（[`groups_dir`]）に在る。

/// 群の今の口座の記録の label（`<群用 dir>/<群>.account` の `account=` の行・無ければ `None`）。
fn move_account(state: &Path, group: &str) -> Option<String> {
    let text = fs::read_to_string(groups_dir(state).join(format!("{group}.account"))).ok()?;
    text.lines().find_map(|line| line.strip_prefix("account=")).map(str::to_owned)
}

/// 置き場の `kind` の event（口座 label と detail・log の順）。
fn move_events(state: &Path, kind: vessel::fleet::EventKind) -> Vec<(String, String)> {
    vessel::fleet::store::read_all(state)
        .unwrap_or_default()
        .into_iter()
        .filter(|event| event.kind == kind)
        .map(|event| (event.account.unwrap_or_default(), event.detail.unwrap_or_default()))
        .collect()
}

/// 群の移動の 3 種の event の件数（承認・断り・保留）。
fn move_counts(state: &Path) -> (usize, usize, usize) {
    use vessel::fleet::EventKind;
    let count = |kind| move_events(state, kind).len();
    (count(EventKind::GroupMoved), count(EventKind::GroupMoveRefused), count(EventKind::GroupMovePending))
}

/// 群用 dir の履歴の file 名（辞書順・dir が無ければ空）。
fn history_names(state: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(groups_dir(state).join("history"))
        .map(|entries| entries.filter_map(Result::ok).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// 群用 dir の直下の file 名のうち拡張子が `ext` のもの。
fn group_files(state: &Path, ext: &str) -> Vec<String> {
    fs::read_dir(groups_dir(state))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(&format!(".{ext}")))
                .collect()
        })
        .unwrap_or_default()
}

/// 偽 tmux の spy の file（`spy/<name>-<target の : を _ に>`）の中身。
fn spy_of(state: &Path, name: &str, target: &str) -> Option<String> {
    fs::read_to_string(state.join("spy").join(format!("{name}-{}", target.replace(':', "_")))).ok()
}

/// shell の前面で受けた起動行（target ごと・受けた順）。
fn launched_lines(state: &Path, target: &str) -> Vec<String> {
    spy_of(state, "launched", target).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// 偽 tmux の前面を書く（`bash` = shell に戻った席・`stuck` は退避の合図を受けても戻らない席の印）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_spy(state: &Path, name: &str, target: &str, body: &str) {
    fs::write(state.join("spy").join(format!("{name}-{}", target.replace(':', "_"))), body).expect("spy の file を書ける");
}

/// 退避の合図の 1 行（器の字面を借りない）。
fn evacuate_payload(group: &str, to: &str) -> String {
    format!("{} group: evacuate group={group} to={to} — 作業記憶を台帳と git に残して /exit", vessel::name::NAME)
}

/// 断りの 1 行。
fn refused_payload(group: &str) -> String {
    format!("{} group: move-refused group={group} reason=no-candidate", vessel::name::NAME)
}

/// 置き場の orchestrator の登録 row の口座（anchor ごと）。
fn seat_account_of(state: &Path, anchor: &str) -> Option<String> {
    let found = vessel::fleet::replay(&vessel::fleet::store::read_all(state).unwrap_or_default());
    vessel::seat::role::registration_of_key(&found, vessel::seat::role::Role::Orchestrator, anchor).map(|row| row.account.clone())
}

/// 口座 `account` を使う live 便を 1 本置く（`RunCreated` + 口座つきの `SeatSpawned`・fleet の inflight に数わる）。
fn put_live_run(state: &Path, account: &str) {
    let state = state.display().to_string();
    for args in [
        vec!["fleet", "record", "--kind", "RunCreated", "--run", "r-live", "--bead", "s2-live.1", "--stage", "Implemented"],
        vec!["fleet", "record", "--kind", "SeatSpawned", "--run", "r-live", "--bead", "s2-live.1", "--seat", "s1", "--account", account],
    ] {
        let out = super::bin_cmd().args(&args).args(["--state-dir", &state]).output();
        assert!(out.is_ok_and(|found| found.status.success()), "live 便を置ける: {args:?}");
    }
}

/// 移動の基本の置き場: 候補 `candidates`（口座ごとの 3 窓は `accounts`）・席は 2 つとも `seat` に居る。
fn move_place(accounts: &[(&str, u64, u64, u64)], candidates: &[&str], seat: &str) -> GroupPlace {
    let place = group_place(accounts, candidates, true);
    group_seats(&place.state, [seat, seat]);
    place
}

/// (移動) 今の口座（種 a1）の 5 時間窓 90 の群は a2 へ移る: 記録 1（account=a2・previous=a1・reason=move）・承認 event 1
/// （account=a2・detail は宣言の行の逐語）・2 つの席へ退避の合図が 1 行ずつ・同じ target へ a2 の口座の起動行が 1 本ずつ・
/// 登録 row は a2・§19 の通知は 0。
#[test]
fn pipe_dispatch_group_move_pressed_group_records_approves_evacuates_and_relaunches() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let out = group_terminal(&place, "r-group-1");
    let record = fs::read_to_string(groups_dir(&place.state).join(format!("{GROUP}.account"))).unwrap_or_default();
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a2"), "記録は a2（{}）", told(&out));
    assert!(record.contains("\nreason=move\nprevious=a1\n"), "理由と前の口座: {record}");
    let moved = move_events(&place.state, vessel::fleet::EventKind::GroupMoved);
    assert_eq!(moved.len(), 1, "承認 event 1: {moved:?}");
    let (account, words) = moved.first().cloned().unwrap_or_default();
    assert_eq!(account, "a2", "account = 移り先");
    assert!(words.contains("host.toml:") && words.contains("[[account-group]]\nname = \"g\""), "宣言の行の逐語: {words}");
    assert_both_seats(&group_sends(&place.state), &evacuate_payload(GROUP, "a2"));
    for (anchor, target) in GROUP_ANCHORS {
        let lines = launched_lines(&place.state, target);
        assert_eq!(lines.len(), 1, "{target} へ起動行 1 本: {lines:?}");
        assert!(lines.iter().all(|line| line.contains("accounts/a2") && !line.contains("accounts/a1")), "a2 の口座で起こす: {lines:?}");
        assert_eq!(seat_account_of(&place.state, anchor).as_deref(), Some("a2"), "{anchor} の登録 row は a2");
    }
    assert_eq!(group_notices(&place.state), Vec::new(), "移動した周は §19 の通知を送らない");
    clean(&[&place.repo, &place.state]);
}

/// (trust・host-init.md §7 / 行 e・接頭辞 `pipe_dispatch_group_trust_`) 群の起こし直しの周は、移り先の口座 a2 の `.claude.json`
/// （無い＝作る）に群の 2 つの anchor の印が置かれ、各 target へ起動行が送られた瞬間の写し（偽 tmux の `claude-at-<target>`）に
/// その target の anchor の印が既に在る（書き → 送りの順）。base は file が作られない（RED）。
#[test]
fn pipe_dispatch_group_trust_marks_the_new_account_before_the_launch_line() {
    use vessel::fleet::json_tree::parse;
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let out = group_terminal(&place, "r-group-trust");
    let marked = |text: &str, anchor: &str| {
        let tree = parse(text).ok();
        tree.as_ref().and_then(|found| found.get("projects")?.get(anchor)?.get("hasTrustDialogAccepted")?.as_bool()) == Some(true)
    };
    let file = fs::read_to_string(place.state.join("accounts").join("a2").join(".claude.json")).unwrap_or_default();
    for (anchor, target) in GROUP_ANCHORS {
        assert!(marked(&file, anchor), "{anchor} の印が a2 に在る（{}）: {file}", told(&out));
        assert_eq!(launched_lines(&place.state, target).len(), 1, "{target} へ起動行 1 本");
        let at = spy_of(&place.state, "claude-at", target).unwrap_or_default();
        assert!(marked(&at, anchor), "{target} へ送った瞬間に {anchor} の印は既に在る: {at:?}");
    }
    assert!(!place.state.join("accounts").join("a1").join(".claude.json").exists(), "移る前の口座には書かない");
    clean(&[&place.repo, &place.state]);
}

/// (候補なし) 候補がどれも逼迫（a1 / a2 とも 5 時間窓 90）の群は移らない: 記録 0・断りの event 1・席の pane への行は群の
/// 置き場ごとに断りの 1 行だけ（全 send の行数 2・§19 の通知の行は 0）・起動行 0。
#[test]
fn pipe_dispatch_group_move_without_candidate_refuses_once_per_anchor() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 90, 10, 10)], &["a1", "a2"], "a1");
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP), None, "記録 0（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (0, 1, 0), "断りの event 1");
    let sends = group_sends(&place.state);
    assert_eq!(sends.len(), 2, "席への行は置き場ごとに 1 行だけ: {sends:?}");
    assert_both_seats(&sends, &refused_payload(GROUP));
    assert_eq!(group_notices(&place.state), Vec::new(), "断った周は §19 の通知を送らない");
    assert!(GROUP_ANCHORS.iter().all(|(_, target)| launched_lines(&place.state, target).is_empty()), "起動行 0");
    clean(&[&place.repo, &place.state]);
}

/// (他の群) 群 h（置き場 2 つ目・候補 [a2, a4]）の今の口座（種 a2）は、群 g（置き場 1 つ目・候補 [a1, a2, a3]）の移り先から
/// 外れて g は a3 へ移る（飛ばす側）。対: h の候補を [a4, a2]（種 a4）にすると g は a2 へ移る（移る側）。
#[test]
fn pipe_dispatch_group_move_skips_another_groups_current_account() {
    let accounts = [("a1", 90, 10, 10), ("a2", 10, 10, 10), ("a3", 10, 10, 10), ("a4", 10, 10, 10)];
    for (other, want) in [(["a2", "a4"], "a3"), (["a4", "a2"], "a2")] {
        let place = groups_place(&accounts, &[(GROUP, &[0], &["a1", "a2", "a3"]), ("h", &[1], &other)]);
        group_seats(&place.state, ["a1", other[0]]);
        let out = group_terminal(&place, "r-group-1");
        assert_eq!(move_account(&place.state, GROUP).as_deref(), Some(want), "h の種 {} で g は {want} へ（{}）", other[0], told(&out));
        assert_eq!(move_account(&place.state, "h"), None, "h は移らない");
        clean(&[&place.repo, &place.state]);
    }
}

/// (同じ周の 2 群) g（候補 [a1, a3, a4]）と h（候補 [a2, a3, a4]）が同じ周に逼迫し候補を共有する: 先の g が a3 へ移り、後の
/// h は g の移り先 a3 を飛ばして a4 へ移る（記録 2 の label が異なる＝周の頭の記録だけを読む変異を捕まえる）。
#[test]
fn pipe_dispatch_group_move_two_groups_in_one_round_take_different_targets() {
    let accounts = [("a1", 90, 10, 10), ("a2", 90, 10, 10), ("a3", 10, 10, 10), ("a4", 10, 10, 10)];
    let place = groups_place(&accounts, &[(GROUP, &[0], &["a1", "a3", "a4"]), ("h", &[1], &["a2", "a3", "a4"])]);
    group_seats(&place.state, ["a1", "a2"]);
    let out = group_terminal(&place, "r-group-1");
    let records = (move_account(&place.state, GROUP), move_account(&place.state, "h"));
    assert_eq!(records, (Some("a3".to_owned()), Some("a4".to_owned())), "先の g は a3・後の h は a4（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (2, 0, 0), "承認 event 2・断り 0");
    clean(&[&place.repo, &place.state]);
}

/// (保留の続き) 2 つ目の置き場の席が退避の合図の後も shell に戻らない周は、1 つ目だけを起こし 2 つ目に保留の event を 1 件
/// 記す。席が shell に戻った後の 2 周目は判定を繰り返さず（承認 event 1 のまま・退避の合図を重ねない・§19 の通知 0・保留を
/// 重ねない）、保留の席だけを同じ target へ a2 で起こす。
#[test]
fn pipe_dispatch_group_move_second_round_only_relaunches_the_pending_seat() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let ((_, one), (anchor, two)) = (GROUP_ANCHORS[0], GROUP_ANCHORS[1]);
    put_spy(&place.state, "stuck", two, "");
    let out = group_terminal(&place, "r-group-1");
    assert_eq!((launched_lines(&place.state, one).len(), launched_lines(&place.state, two).len()), (1, 0), "{}", told(&out));
    assert_eq!(move_counts(&place.state), (1, 0, 1), "承認 1・保留 1");
    let pending = move_events(&place.state, vessel::fleet::EventKind::GroupMovePending);
    assert!(pending.iter().all(|(account, detail)| account == "a2" && detail.contains(&format!("anchor={anchor}"))), "{pending:?}");
    assert_eq!(seat_account_of(&place.state, anchor).as_deref(), Some("a1"), "保留の席の row は古い口座のまま");
    put_spy(&place.state, "front", two, "bash");
    let again = group_terminal(&place, "r-group-2");
    assert_eq!(launched_lines(&place.state, two).len(), 1, "保留の席を起こす（{}）", told(&again));
    assert_eq!(launched_lines(&place.state, one).len(), 1, "起きた席は起こし直さない");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "判定を繰り返さない・保留を重ねない");
    let evacuations = group_sends(&place.state).iter().filter(|line| line.contains(" group: evacuate ")).count();
    assert_eq!(evacuations, 2, "退避の合図は 1 周目の 2 行だけ");
    assert_eq!(group_notices(&place.state), Vec::new(), "続きの周は §19 の通知を送らない");
    assert_eq!(seat_account_of(&place.state, anchor).as_deref(), Some("a2"), "保留の席の row は a2");
    clean(&[&place.repo, &place.state]);
}

/// (移動を頼む記録) 頼みの在る群は今の口座が鮮度の内側でも計測を 1 回撃ち、判定の後に頼みを群用 dir から履歴へ move する
/// （群用 dir の頼み 0・履歴 1）。同じ fixture の 2 周目は頼みが無いので鮮度の内側の計測 0（move せず残す変異を捕まえる）。
#[test]
fn pipe_dispatch_group_move_request_forces_one_measurement_and_moves_to_history() {
    let place = move_place(&[("a1", 10, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    put_group_round(&place.state, &group_now(), "a1", (10, 10, 10));
    fs::create_dir_all(groups_dir(&place.state)).unwrap_or_default();
    fs::write(groups_dir(&place.state).join(format!("{GROUP}.request")), "ts=t\naccount=a1\nwindow=5h\n").unwrap_or_default();
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(group_calls(&place.state), 1, "鮮度の内側でも頼みの在る周は 1 回測る（{}）", told(&out));
    assert_eq!(group_files(&place.state, "request"), Vec::<String>::new(), "群用 dir の頼みは 0");
    let history = history_names(&place.state);
    assert_eq!(history.iter().filter(|name| name.starts_with(&format!("{GROUP}.request."))).count(), 1, "履歴に 1: {history:?}");
    let again = group_terminal(&place, "r-group-2");
    assert_eq!(group_calls(&place.state), 1, "2 周目は頼みが無い＝鮮度の内側は測らない（{}）", told(&again));
    assert_eq!(move_account(&place.state, GROUP), None, "閾値未満は移らない");
    clean(&[&place.repo, &place.state]);
}

/// (4 手の順) 退避の合図を受けた瞬間に記録と承認 event が既に在り（偽 tmux が合図の Enter で測る）、起動行は合図より後に
/// 届く（時刻の並び）。対: 記録を書けない周（一時 file の path が dir）は承認 event 0・合図 0・起動 0（承認を記録より先に
/// 書く変異を捕まえる）。
#[test]
fn pipe_dispatch_group_move_four_steps_run_in_order() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let out = group_terminal(&place, "r-group-1");
    for (_, target) in GROUP_ANCHORS {
        assert_eq!(spy_of(&place.state, "record-at-evacuate", target).as_deref(), Some("1\n"), "合図の時に記録が在る（{}）", told(&out));
        assert_eq!(spy_of(&place.state, "moved-at-evacuate", target).as_deref(), Some("1\n"), "合図の時に承認 event が在る");
        let at = |name| spy_of(&place.state, name, target).and_then(|found| found.trim().parse::<u128>().ok());
        let (evacuated, launched) = (at("evacuate-at"), at("launch-at"));
        assert!(evacuated.is_some() && launched > evacuated, "{target}: 起動は合図より後: {evacuated:?} {launched:?}");
    }
    clean(&[&place.repo, &place.state]);
    let broken = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    fs::create_dir_all(groups_dir(&broken.state).join(format!("{GROUP}.account.tmp"))).unwrap_or_default();
    let out = group_terminal(&broken, "r-group-1");
    assert_eq!(move_account(&broken.state, GROUP), None, "記録を書けない（{}）", told(&out));
    assert_eq!(move_counts(&broken.state), (0, 0, 0), "承認 event 0");
    assert_eq!(group_sends(&broken.state), Vec::<String>::new(), "合図 0");
    assert!(GROUP_ANCHORS.iter().all(|(_, target)| launched_lines(&broken.state, target).is_empty()), "起動 0");
    clean(&[&broken.repo, &broken.state]);
}

/// (群 0) 群を宣言しない host で逼迫の口座と席が在っても、起こす側の周は群用 dir を作らず記録 0・event 0・送り 0・起動 0・
/// 計測 0 で、便の列の rc（終端の周は rc 0）と `dispatch ls` の外形は今のまま。
#[test]
fn pipe_dispatch_group_move_zero_groups_touch_nothing() {
    let place = group_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], false);
    group_seats(&place.state, ["a1", "a1"]);
    let out = group_terminal(&place, "r-group-1");
    assert!(!groups_dir(&place.state).exists(), "群用 dir を作らない（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (0, 0, 0), "event 0");
    assert_eq!(group_notices(&place.state), Vec::new(), "通知 0");
    assert_eq!(group_sends(&place.state), Vec::<String>::new(), "送り 0");
    assert!(GROUP_ANCHORS.iter().all(|(_, target)| launched_lines(&place.state, target).is_empty()), "起動 0");
    assert_eq!(group_calls(&place.state), 0, "計測 0");
    let listed = group_turn(&place, &["ls"], false);
    assert_eq!(listed.status.code(), Some(i32::from(RC_OK)), "{}", told(&listed));
    assert_eq!(stdout_of(&listed).trim_end(), NONE_LINE, "dispatch ls の外形は今のまま");
    assert!(!groups_dir(&place.state).exists(), "見る側の周も作らない");
    clean(&[&place.repo, &place.state]);
}

/// (書き換え) 1 周目に a1 → a2 へ移った群の a2 が 2 周目に逼迫すると、a1（逼迫の実測）を飛ばして a3 へ移り、前の記録
/// （account=a2）は履歴へ move して群用 dir の記録は 1 file のまま。
#[test]
fn pipe_dispatch_group_move_rewrite_moves_the_previous_record_to_history() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10), ("a3", 10, 10, 10)], &["a1", "a2", "a3"], "a1");
    group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a2"), "1 周目は a2");
    put_group_round(&place.state, &group_now(), "a2", (90, 10, 10));
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a3"), "2 周目は a3（{}）", told(&out));
    assert_eq!(group_files(&place.state, "account"), vec![format!("{GROUP}.account")], "記録は 1 file");
    let history: Vec<String> =
        history_names(&place.state).into_iter().filter(|name| name.starts_with(&format!("{GROUP}.account."))).collect();
    assert_eq!(history.len(), 1, "前の記録が履歴に 1: {history:?}");
    let previous = history.first().map(|name| groups_dir(&place.state).join("history").join(name));
    let text = previous.and_then(|path| fs::read_to_string(path).ok()).unwrap_or_default();
    assert!(text.starts_with("account=a2\n"), "履歴は前の記録（a2）: {text}");
    clean(&[&place.repo, &place.state]);
}

/// (live 便) 候補の順で先の a2 を置き場の live 便が使っている周は a2 を飛ばして a3 へ移る（飛ばす側）。対: live 便が無ければ
/// a2 へ移る（移る側）。
#[test]
fn pipe_dispatch_group_move_skips_an_account_used_by_a_live_run() {
    for (live, want) in [(true, "a3"), (false, "a2")] {
        let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10), ("a3", 10, 10, 10)], &["a1", "a2", "a3"], "a1");
        if live {
            put_live_run(&place.state, "a2");
        }
        let out = group_terminal(&place, "r-group-1");
        assert_eq!(move_account(&place.state, GROUP).as_deref(), Some(want), "live={live} は {want} へ（{}）", told(&out));
        clean(&[&place.repo, &place.state]);
    }
}

/// 候補の順で先の a2 の 3 窓が `second`、次の a3 が閾値未満の置き場で 1 周を撃ち、移り先を返す。
fn move_with_second(second: (u64, u64, u64)) -> Option<String> {
    let (five, seven, model) = second;
    let place = move_place(&[("a1", 90, 10, 10), ("a2", five, seven, model), ("a3", 10, 10, 10)], &["a1", "a2", "a3"], "a1");
    group_terminal(&place, "r-group-1");
    let found = move_account(&place.state, GROUP);
    clean(&[&place.repo, &place.state]);
    found
}

/// (5 時間窓) 先の a2 の 5 時間窓だけが 90（行の値 85 以上）なら a2 を飛ばして a3 へ移る。
#[test]
fn pipe_dispatch_group_move_skips_a_candidate_over_the_five_hour_cap() {
    assert_eq!(move_with_second((90, 10, 10)).as_deref(), Some("a3"), "5 時間窓の逼迫は飛ばす");
    assert_eq!(move_with_second((80, 10, 10)).as_deref(), Some("a2"), "閾値未満なら a2 へ");
}

/// (7 日窓) 先の a2 の 7 日窓だけが 96（行の値 95 以上）なら a2 を飛ばして a3 へ移る。
#[test]
fn pipe_dispatch_group_move_skips_a_candidate_over_the_seven_day_cap() {
    assert_eq!(move_with_second((10, 96, 10)).as_deref(), Some("a3"), "7 日窓の逼迫は飛ばす");
    assert_eq!(move_with_second((10, 90, 10)).as_deref(), Some("a2"), "7 日窓 90 は行の値 95 未満＝a2 へ");
}

/// (モデル別窓) 先の a2 のモデル別窓だけが 96 なら a2 を飛ばして a3 へ移る。
#[test]
fn pipe_dispatch_group_move_skips_a_candidate_over_the_model_cap() {
    assert_eq!(move_with_second((10, 10, 96)).as_deref(), Some("a3"), "モデル別窓の逼迫は飛ばす");
    assert_eq!(move_with_second((10, 10, 90)).as_deref(), Some("a2"), "モデル別窓 90 は行の値 95 未満＝a2 へ");
}

/// (lock) 群用 dir に lock の file が残る周は群の段が typed に止まり、記録 0・event 0・送り 0・計測 0 で、便の列の rc は
/// 変わらない（終端の周は rc 0）。lock の file は消さない（他の周のもの）。
#[test]
fn pipe_dispatch_group_move_leftover_lock_stops_the_stage_without_touching_the_queue() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    fs::create_dir_all(groups_dir(&place.state)).unwrap_or_default();
    fs::write(groups_dir(&place.state).join("lock"), "pid=1\n").unwrap_or_default();
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP), None, "記録 0（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (0, 0, 0), "event 0");
    assert_eq!(group_notices(&place.state), Vec::new(), "通知 0");
    assert_eq!(group_sends(&place.state), Vec::<String>::new(), "送り 0");
    assert_eq!(group_calls(&place.state), 0, "計測 0");
    assert!(groups_dir(&place.state).join("lock").is_file(), "lock の file は残る");
    clean(&[&place.repo, &place.state]);
}

// ───── 退避を器が完結させる（account-lifecycle.md §21 形 1・契約表の行 j・接頭辞 `pipe_dispatch_group_exit_`・§20 の fixture） ─────
//
// 2 つ目の置き場の席は退避の合図を受けても shell に戻らない（`spy/stuck-<target>`）。偽 tmux は `/exit` を受けても前面を変えない
// （shell に戻す周は歯が `spy/front-<target>` を書く）。

/// `target` へ送った `/exit` の payload の行（`send-keys -t <target> -l /exit`・退避の合図の末尾の `/exit` とは ` -l ` の直後で
/// 分ける）。
fn exit_sends(state: &Path, target: &str) -> usize {
    group_sends(state).iter().filter(|line| line.contains(&format!("-t {target} ")) && line.ends_with(" -l /exit")).count()
}

/// 移動の周を撃つ: 候補 [a1, a2]・種 a1 が逼迫・2 つ目の置き場の席は shell に戻らない（1 つ目は a2 で起き、2 つ目は保留 1）。
fn exit_place() -> GroupPlace {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    put_spy(&place.state, "stuck", GROUP_ANCHORS[1].1, "");
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "移動の周は承認 1・保留 1（{}）", told(&out));
    place
}

/// (続きの周) 記録 a2 ≠ row a1 の群の続きの周に pane が claude の席へ `/exit` の 1 行を送り、その周は起こさず（起動行 0）、
/// 保留の event を重ねない（保留 1 のまま）。既に a2 に居る席へは送らない。
#[test]
fn pipe_dispatch_group_exit_continuation_round_sends_one_exit_to_the_seat_not_at_a_shell() {
    let place = exit_place();
    let ((_, one), (_, two)) = (GROUP_ANCHORS[0], GROUP_ANCHORS[1]);
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(exit_sends(&place.state, two), 1, "保留の席へ /exit 1 行（{}）: {:?}", told(&out), group_sends(&place.state));
    assert_eq!(exit_sends(&place.state, one), 0, "起きた席へは送らない");
    assert_eq!(launched_lines(&place.state, two).len(), 0, "その周は起こさない");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "判定を繰り返さず保留の event を重ねない");
    clean(&[&place.repo, &place.state]);
}

/// (周ごとに 1 回) 同じ席が次の周も shell に戻らなければ、もう 1 行送る（2 周で 2 行・1 周に 2 行は送らない）。
#[test]
fn pipe_dispatch_group_exit_is_sent_once_per_round_while_the_seat_is_not_at_a_shell() {
    let place = exit_place();
    let two = GROUP_ANCHORS[1].1;
    group_terminal(&place, "r-group-2");
    assert_eq!(exit_sends(&place.state, two), 1, "2 周目は 1 行");
    let out = group_terminal(&place, "r-group-3");
    assert_eq!(exit_sends(&place.state, two), 2, "3 周目にもう 1 行（{}）", told(&out));
    assert_eq!(launched_lines(&place.state, two).len(), 0, "起こさない");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "保留を重ねない");
    clean(&[&place.repo, &place.state]);
}

/// (shell に戻った周) `/exit` の後に席が shell に戻った周は送り 0 で、§20 形 6 のとおり同じ target へ a2 の口座の起動行 1 本。
#[test]
fn pipe_dispatch_group_exit_seat_back_at_a_shell_is_launched_without_another_exit() {
    let place = exit_place();
    let (anchor, two) = GROUP_ANCHORS[1];
    group_terminal(&place, "r-group-2");
    assert_eq!(exit_sends(&place.state, two), 1, "続きの周に 1 行");
    put_spy(&place.state, "front", two, "bash");
    let out = group_terminal(&place, "r-group-3");
    assert_eq!(exit_sends(&place.state, two), 1, "shell に戻った周は送らない（{}）", told(&out));
    let lines = launched_lines(&place.state, two);
    assert_eq!(lines.len(), 1, "起動行 1: {lines:?}");
    assert!(lines.iter().all(|line| line.contains("accounts/a2")), "a2 の口座で起こす: {lines:?}");
    assert_eq!(seat_account_of(&place.state, anchor).as_deref(), Some("a2"), "登録 row は a2");
    clean(&[&place.repo, &place.state]);
}

/// (移動の周) 記録を書いた同じ周は退避の合図の 1 行だけで `/exit` は 0（shell に戻らない席にも送らない＝席が作業記憶を残す番を
/// 1 周ぶん持つ）。
#[test]
fn pipe_dispatch_group_exit_move_round_sends_only_the_evacuation() {
    let place = exit_place();
    for (_, target) in GROUP_ANCHORS {
        assert_eq!(exit_sends(&place.state, target), 0, "{target}: 移動の周は /exit 0");
    }
    assert_both_seats(&group_sends(&place.state), &evacuate_payload(GROUP, "a2"));
    clean(&[&place.repo, &place.state]);
}

// ───── 群の段の起こし直しも会話を運ぶ（seat-heartbeat.md §8・契約表の行 g・接頭辞 `pipe_dispatch_group_carry_`・§21 の fixture） ─────
//
// §21 の移動の周（2 つ目の席は shell に戻らず保留 1）の後、2 つ目の席の打刻 file を歯が書き直して前面を shell に戻し、続きの周で
// 起こす（偽 tmux は退避の合図の Enter に sid の無い打刻を足すので、sid の在る最終行は続きの周の前に書く）。

/// 打刻に書く会話 id（UUID の形）。
const CARRY_SID: &str = "7c1e4b2a-9d3f-4a6b-8e0c-2f5a7b9d1e3c";

/// 打刻の 1 行（`state` / `event` / `sid`・ts は固定＝起動の証拠〔偽 tmux の `SessionStart`〕より前）。
fn carry_stamp(state: &str, event: &str, sid: &str) -> String {
    format!("{{\"schema\":1,\"state\":\"{state}\",\"event\":\"{event}\",\"ts\":1700000000,\"sid\":\"{sid}\"}}\n")
}

/// §21 の移動の周の後、2 つ目の席の打刻 file を `stamps` で置き換え、前面を shell に戻して続きの周を 1 回撃つ。その席の起動行
/// （1 本であることを測る）を返す。
fn carry_relaunch(place: &GroupPlace, stamps: &str) -> String {
    let (_, two) = GROUP_ANCHORS[1];
    let seat = place.state.join("seat").join(two.replace(':', "_"));
    fs::write(seat.join("state.jsonl"), stamps).unwrap_or_default();
    put_spy(&place.state, "front", two, "bash");
    let out = group_terminal(place, "r-group-carry");
    let lines = launched_lines(&place.state, two);
    assert_eq!(lines.len(), 1, "起動行 1 本（{}）: {lines:?}", told(&out));
    assert_eq!(move_counts(&place.state), (1, 0, 1), "承認・断り・保留の event は変わらない");
    lines.first().cloned().unwrap_or_default()
}

/// 置き場の登録 row の全部（log の順）。
fn carry_rows(state: &Path) -> Vec<vessel::fleet::Registration> {
    vessel::fleet::store::read_all(state).unwrap_or_default().into_iter().filter_map(|event| event.registration).collect()
}

/// 起動行の末尾の初手の 1 語（seat-heartbeat.md §10 形 1〜3・単引用で括った `<NAME> seat: relaunch …`・tick の起こしと同じ 1 本）。
fn carry_first_word() -> String {
    let word = vessel::seat::state::relaunch_word();
    assert!(word.starts_with(&format!("'{} seat: relaunch ", vessel::name::NAME)) && word.ends_with('\''), "単引用の初手: {word}");
    word
}

/// (a) 起こす席の打刻の最終行に会話 id が在る → 起動行の末尾が `--resume <sid> '<NAME> seat: relaunch …'`（1 つずつ・a2 の口座の
/// まま）。base は末尾が `--resume <sid>` で初手が無い（RED）。
#[test]
fn pipe_dispatch_group_carry_relaunch_resumes_the_stamped_session() {
    let place = exit_place();
    let line = carry_relaunch(&place, &carry_stamp("idle", "Stop", CARRY_SID));
    assert!(line.ends_with(&format!(" --resume {CARRY_SID} {}", carry_first_word())), "末尾に --resume <sid> と初手: {line}");
    assert_eq!(line.matches("--resume").count(), 1, "--resume は 1 つ: {line}");
    assert_eq!(line.matches("seat: relaunch").count(), 1, "初手は 1 つ: {line}");
    assert!(line.contains("accounts/a2"), "a2 の口座で起こす: {line}");
    clean(&[&place.repo, &place.state]);
}

/// (b) 打刻が無い（空の file）・最終行の sid が会話 id の形でない（前の行の会話 id にも倒れない）→ 起こすが `--resume` は無く、
/// 末尾は初手の 1 語だけ。
#[test]
fn pipe_dispatch_group_carry_without_a_session_id_carries_nothing() {
    let wrong = format!("{}{}", carry_stamp("idle", "Stop", CARRY_SID), carry_stamp("busy", "UserPromptSubmit", "sid-group"));
    for (case, stamps) in [("no-stamp", String::new()), ("not-a-uuid", wrong)] {
        let place = exit_place();
        let line = carry_relaunch(&place, &stamps);
        assert!(line.contains("accounts/a2"), "{case}: 起こす: {line}");
        assert!(!line.contains("--resume") && !line.contains(CARRY_SID), "{case}: --resume 無し: {line}");
        assert!(line.ends_with(&format!(" {}", carry_first_word())), "{case}: 末尾は初手の 1 語: {line}");
        assert_eq!(line.matches("seat: relaunch").count(), 1, "{case}: 初手は 1 つ: {line}");
        clean(&[&place.repo, &place.state]);
    }
}

/// (c) 会話を運んだ周も、登録 row の `launch` は雛形のまま（`--resume` も sid も初手も載らない・起こした row は a2 で launch を持つ）。
#[test]
fn pipe_dispatch_group_carry_leaves_the_row_launch_without_resume() {
    let place = exit_place();
    let (anchor, two) = GROUP_ANCHORS[1];
    let line = carry_relaunch(&place, &carry_stamp("idle", "Stop", CARRY_SID));
    assert!(line.ends_with(&format!(" --resume {CARRY_SID} {}", carry_first_word())), "運んだ周: {line}");
    let rows = carry_rows(&place.state);
    let last = rows.iter().rev().find(|row| row.anchor == anchor && row.target == two);
    assert!(last.is_some_and(|row| row.account == "a2" && !row.launch.is_empty()), "起こした row は a2 で launch を持つ: {rows:?}");
    assert!(rows.iter().all(|row| !row.launch.contains("--resume") && !row.launch.contains(CARRY_SID)), "row の launch は雛形のまま: {rows:?}");
    assert!(rows.iter().all(|row| !row.launch.contains("seat: relaunch")), "row の launch に初手は載らない: {rows:?}");
    clean(&[&place.repo, &place.state]);
}

// ───── 退役した登録 row（account-lifecycle.md §24 形 3・契約表の行 m・接頭辞 `pipe_dispatch_group_retired_`・§20 / §21 の fixture） ─────
//
// 群の段の読み手（`behind`）は 1 字も変えず、`seat retire` の後の replay が row を外すので、退役した row の target には何も送らない。

/// `target` へ送った payload の行の数（退避の合図・`/exit`・逼迫の 1 行の全部）。
fn target_sends(state: &Path, target: &str) -> usize {
    group_sends(state).iter().filter(|line| line.contains(&format!("-t {target} "))).count()
}

/// `seat retire --state-dir <state> --target <target>` を binary で 1 回撃ち、rc 0 を確かめる。
fn retire_seat(state: &Path, target: &str) {
    let state = state.display().to_string();
    let out = super::bin_cmd().args(["seat", "retire", "--state-dir", &state, "--target", target, "--reason", "moved"]).output();
    assert!(out.as_ref().is_ok_and(|found| found.status.success()), "{target} の row を退役できる: {out:?}");
}

/// (移動の周) 2 つ目の置き場の orchestrator の row を退役させてから、種 a1 の逼迫した群を移す周: 退避の合図は残る row の
/// target にだけ 1 行・退役した row の target へは送り 0・起動行 0（base では `retire` が使い方の誤りで row が残り合図が届く＝RED）。
#[test]
fn pipe_dispatch_group_retired_row_target_gets_no_evacuation() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let ((_, one), (anchor, two)) = (GROUP_ANCHORS[0], GROUP_ANCHORS[1]);
    retire_seat(&place.state, two);
    assert_eq!(seat_account_of(&place.state, anchor), None, "退役した row は replay に無い");
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a2"), "群は移る（{}）", told(&out));
    let sends = group_sends(&place.state);
    let evacuate = evacuate_payload(GROUP, "a2");
    assert_eq!(sends.iter().filter(|line| line.contains(&format!("-t {one} ")) && line.ends_with(&evacuate)).count(), 1, "{sends:?}");
    assert_eq!(target_sends(&place.state, two), 0, "退役した row の target へは送らない: {sends:?}");
    assert_eq!(launched_lines(&place.state, two).len(), 0, "起こさない");
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(target_sends(&place.state, two), 0, "続きの周も送らない（{}）", told(&out));
    assert_eq!(exit_sends(&place.state, two), 0, "/exit 0");
    clean(&[&place.repo, &place.state]);
}

/// (続きの周) 移動の周に shell へ戻らず保留になった席の row を退役させると、続きの周は `/exit` も退避の合図も送らず（送り 0）
/// 起こさない（base では row が残り `/exit` が 1 行届く＝RED）。保留の event は重ねない。
#[test]
fn pipe_dispatch_group_retired_pending_seat_gets_no_exit() {
    let place = exit_place();
    let (anchor, two) = GROUP_ANCHORS[1];
    let before = target_sends(&place.state, two);
    assert_eq!(before, 1, "移動の周の退避の合図 1 行だけ: {:?}", group_sends(&place.state));
    retire_seat(&place.state, two);
    assert_eq!(seat_account_of(&place.state, anchor), None, "退役した row は replay に無い");
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(exit_sends(&place.state, two), 0, "/exit 0（{}）: {:?}", told(&out), group_sends(&place.state));
    assert_eq!(target_sends(&place.state, two), before, "送りは増えない");
    assert_eq!(launched_lines(&place.state, two).len(), 0, "起こさない");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "保留の event を重ねない");
    clean(&[&place.repo, &place.state]);
}

// ───── /exit の dialog を器が確定する（account-lifecycle.md §22 形 1〜4・契約表の行 k・接頭辞 `pipe_dispatch_group_exit_dialog_`・
// §21 の fixture） ─────
//
// 2 つ目の置き場の席（保留の席）の可視域を `spy/screen-<target>` で作り分け、`spy/dialog-<target>` で `/exit` を受けた席に dialog
// を出す。器の字面（dialog の既定の行・記録の what）は借りずに写す。

/// `/exit` の確認 dialog の既定の行が最後の `❯` 行に在る可視域。
const DIALOG_SCREEN: &str = "Background work is running\n\u{276f} 1. Exit and stop tasks\n  2. Cancel\n";

/// 既定の行へ Enter を送った周の記録の what。
const DIALOG_WHAT: &str = "enter:exit-dialog";

/// `target` へ送った Enter だけの行（`send-keys -t <target> Enter`・payload の送りの Enter も含む）。
fn enter_sends(state: &Path, target: &str) -> usize {
    let want = format!("send-keys -t {target} Enter");
    fs::read_to_string(state.join("tmux-calls")).unwrap_or_default().lines().filter(|line| *line == want).count()
}

/// `target` へ送った payload の行（`send-keys -t <target> -l …`）。
fn payload_sends(state: &Path, target: &str) -> usize {
    group_sends(state).iter().filter(|line| line.contains(&format!("-t {target} -l "))).count()
}

/// 席 `target` の inject の記録（`seat/<target>/tick.jsonl`）のうち `what` が `what` の行の `who`（記録の順）。
fn record_whos(state: &Path, target: &str, what: &str) -> Vec<String> {
    fs::read_to_string(vessel::seat::inject::tick_path(state, target))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| vessel::fleet::json_lite::parse_object(line).ok())
        .filter(|pairs| pairs.iter().any(|(key, value)| key == "what" && value.as_str() == Some(what)))
        .filter_map(|pairs| pairs.into_iter().find(|(key, _)| key == "who").and_then(|(_, value)| value.as_str().map(str::to_owned)))
        .collect()
}

/// 移動の周を撃つ（[`exit_place`] と同じ置き場）。保留の席は `/exit` を受けると dialog を出し、`screen` が在れば移動の周の前から
/// その可視域を持つ。
fn dialog_place(screen: Option<&str>) -> GroupPlace {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let two = GROUP_ANCHORS[1].1;
    put_spy(&place.state, "stuck", two, "");
    put_spy(&place.state, "dialog", two, DIALOG_SCREEN);
    if let Some(found) = screen {
        put_spy(&place.state, "screen", two, found);
    }
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "移動の周は承認 1・保留 1（{}）", told(&out));
    place
}

/// (形 1) 続きの周の `/exit` は dialog が出て送達を確認できない周（echo が無い）でも inject の記録に 1 行残る（what は `/exit`・
/// who は 1 つ）。送りは 1 行・その周は起こさない・保留の event を重ねない。
#[test]
fn pipe_dispatch_group_exit_dialog_unconfirmed_exit_leaves_one_record() {
    let place = dialog_place(None);
    let two = GROUP_ANCHORS[1].1;
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(exit_sends(&place.state, two), 1, "/exit の送り 1 行（{}）", told(&out));
    let whos = record_whos(&place.state, two, "/exit");
    assert_eq!(whos.len(), 1, "/exit の記録 1 行: {whos:?}");
    assert_eq!(spy_of(&place.state, "screen", two).as_deref(), Some(DIALOG_SCREEN), "dialog が出ている");
    assert_eq!(launched_lines(&place.state, two).len(), 0, "起こさない");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "保留を重ねない");
    clean(&[&place.repo, &place.state]);
}

/// (形 2 / 3) dialog の出た席の次の周は、門の tail が既定の行なので `/exit` を送らず Enter を 1 回だけ送り、記録に
/// `enter:exit-dialog` の 1 行（`/exit` の行と同じ who）。Enter で shell に戻った次の周は同じ target へ a2 の口座の起動行 1 本。
#[test]
fn pipe_dispatch_group_exit_dialog_default_row_gets_one_enter_and_one_record() {
    let place = dialog_place(None);
    let (anchor, two) = GROUP_ANCHORS[1];
    group_terminal(&place, "r-group-2");
    let (exits, enters) = (exit_sends(&place.state, two), enter_sends(&place.state, two));
    let out = group_terminal(&place, "r-group-3");
    assert_eq!(enter_sends(&place.state, two), enters + 1, "Enter 1 回（{}）", told(&out));
    assert_eq!(exit_sends(&place.state, two), exits, "/exit の送り 0");
    let (confirms, exit_whos) = (record_whos(&place.state, two, DIALOG_WHAT), record_whos(&place.state, two, "/exit"));
    assert_eq!(confirms.len(), 1, "{DIALOG_WHAT} の記録 1 行: {confirms:?}");
    assert_eq!(confirms, exit_whos, "/exit と同じ who");
    assert_eq!(launched_lines(&place.state, two).len(), 0, "Enter の周は起こさない");
    let out = group_terminal(&place, "r-group-4");
    let lines = launched_lines(&place.state, two);
    assert_eq!(lines.len(), 1, "shell に戻った周に起動行 1（{}）: {lines:?}", told(&out));
    assert!(lines.iter().all(|line| line.contains("accounts/a2")), "a2 の口座で起こす: {lines:?}");
    assert_eq!(seat_account_of(&place.state, anchor).as_deref(), Some("a2"), "登録 row は a2");
    assert_eq!(record_whos(&place.state, two, DIALOG_WHAT).len(), 1, "起こした周に Enter の記録は増えない");
    clean(&[&place.repo, &place.state]);
}

/// (形 3) dialog が残る周は周ごとに Enter を 1 回ずつ送る（上限を置かない・1 周に 2 回は送らない）。
#[test]
fn pipe_dispatch_group_exit_dialog_remaining_dialog_gets_one_enter_per_round() {
    let place = dialog_place(None);
    let two = GROUP_ANCHORS[1].1;
    group_terminal(&place, "r-group-2");
    let enters = enter_sends(&place.state, two);
    for (at, run) in ["r-group-3", "r-group-4"].into_iter().enumerate() {
        // 前の周の Enter で閉じた dialog を出し直し、前面を席に戻す（dialog が残る席）。
        put_spy(&place.state, "screen", two, DIALOG_SCREEN);
        put_spy(&place.state, "front", two, "claude");
        let out = group_terminal(&place, run);
        assert_eq!(enter_sends(&place.state, two), enters + at + 1, "{run}: 周ごとに Enter 1 回（{}）", told(&out));
    }
    assert_eq!(record_whos(&place.state, two, DIALOG_WHAT).len(), 2, "記録は周ごとに 1 行");
    assert_eq!(exit_sends(&place.state, two), 1, "/exit は続きの 1 周目だけ");
    clean(&[&place.repo, &place.state]);
}

/// (形 2 の fail-closed) 門の tail が既定の行と違う字面（1 字欠け・人の打ちかけ）の席へは 1 key も送らず記録も残さない。
#[test]
fn pipe_dispatch_group_exit_dialog_other_tail_gets_no_key_and_no_record() {
    for screen in ["\u{276f} 1. Exit and stop task\n", "\u{276f} foo\n"] {
        let place = dialog_place(Some(screen));
        let two = GROUP_ANCHORS[1].1;
        let (payloads, enters) = (payload_sends(&place.state, two), enter_sends(&place.state, two));
        let out = group_terminal(&place, "r-group-2");
        assert_eq!(payload_sends(&place.state, two), payloads, "{screen:?}: payload の送り 0（{}）", told(&out));
        assert_eq!(enter_sends(&place.state, two), enters, "{screen:?}: Enter 0");
        assert_eq!(record_whos(&place.state, two, DIALOG_WHAT).len(), 0, "{screen:?}: Enter の記録 0");
        assert_eq!(record_whos(&place.state, two, "/exit").len(), 0, "{screen:?}: /exit の記録 0");
        clean(&[&place.repo, &place.state]);
    }
}

/// (形 4) 移動の周（記録を書いた同じ周）は dialog の既定の行を返す席にも Enter を送らない（退避の合図は門で断られ、`/exit` も
/// Enter も 0・記録 0）。
#[test]
fn pipe_dispatch_group_exit_dialog_move_round_sends_no_enter() {
    let place = dialog_place(Some(DIALOG_SCREEN));
    let two = GROUP_ANCHORS[1].1;
    assert_eq!(enter_sends(&place.state, two), 0, "移動の周の Enter 0");
    assert_eq!(payload_sends(&place.state, two), 0, "移動の周の payload の送り 0");
    assert_eq!(record_whos(&place.state, two, DIALOG_WHAT).len(), 0, "Enter の記録 0");
    clean(&[&place.repo, &place.state]);
}
