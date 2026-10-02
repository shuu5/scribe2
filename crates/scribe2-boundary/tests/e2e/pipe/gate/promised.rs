//! 段 ① が契約の約束を便の木で測る族の歯（接頭辞 `pipe_gate_promised_`・設計 docs/design/pipeline.md §58 行 ba）。
//!
//! 共有の helper と const は親 module（`tests/e2e/pipe/gate.rs`）と `tests/e2e/pipe.rs` に在り、`use super::*` で使う。
//! runner は `sh -c` の偽 runner で、`+` の file や約束の名を作る周と作らない周を対で撃つ。

use super::*;

/// `+` の新規 file の項目（toy repo の base に無い）。
const FRESH_FILE: &str = "src/fresh.rs";

/// `+` の項目の外れの見出し（段 ① の stderr・器の字面と同じ）。
const ABSENT_HEAD: &str = "契約の write-set の + の file が便の木に無い:";

/// 約束の名の外れの見出し。
const UNRESOLVED_HEAD: &str = "約束の行の + の名が便の木で解けない:";

/// 段 ① の診断 file（gate の `verify.stderr.log`・無ければ空）。
fn gate_log(state: &Path, id: &str) -> String {
    fs::read_to_string(run_dir(state, id).join("verify.stderr.log")).unwrap_or_default()
}

/// `+` の file を宣言した契約の便を、`runner` で Implemented まで通す（toy repo・write-set は `src/lib.rs` と `+src/fresh.rs`）。
fn fresh_run(runner: &str) -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &["write-set"], &[&format!("write-set = [\"src/lib.rs\", \"+{FRESH_FILE}\"]")]);
    let id = intake(&repo, &state, &design);
    let out = spawn_with(&repo, &state, &id, runner);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    (repo, state, id)
}

/// (1) 契約が `+` の file を宣言し、偽 runner がそれを作らずに commit した便の gate は FAIL で、段 ① の record が rc 1、診断が
/// 見出しの下にその path を名指す（lens は起動しない）。作った便は PASS のまま段 ① が緑。
#[test]
fn pipe_gate_promised_plus_file_not_created_fails_stage_one() {
    let (repo, state, id) = fresh_run(TOY_COMMIT);
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "作らなかった `+` は FAIL: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=FAIL"), "{}", stdout_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 1, "cmd"), "write-set", "段 ① は先頭（母集団 {} record）", rows.len());
    assert_eq!(row_value(&rows, 1, "rc"), "1", "約束の外れは段 ① の rc 1");
    assert!(!marker.exists(), "段 ① が赤い周は lens を起動しない");
    let log = gate_log(&state, &id);
    assert!(log.contains(&format!("{ABSENT_HEAD}\n{FRESH_FILE}")), "見出しの下に path を名指す: {log}");
    assert!(!log.contains("write-set の外へ出た"), "diff は write-set の内（外れは約束だけ）: {log}");
    clean(&[&repo, &state]);

    let made = format!("echo x >> src/lib.rs && echo new > {FRESH_FILE} && git add -A && git commit -q -m runner");
    let (repo, state, id) = fresh_run(&made);
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "作った便は PASS: {}", stderr_of(&out));
    assert_eq!(row_value(&verify_rows(&state, &id), 1, "rc"), "0", "段 ① が緑");
    clean(&[&repo, &state]);
}

/// 約束の行の新設 file（`+` の files）。
const PROMISED_FILE: &str = "crates/toy/src/fresh.rs";

/// 約束の行の新設の名（`symbols` の `+` の名・fn 形）。
const PROMISED_NAME: &str = "fresh_helper(";

/// 約束の行を 1 つ持つ行 `a` の設計 doc（受付の約束の行の歯と同じ形: 行は `write-set` / `verify` / `done` を持たず、約束の行が
/// `+` の file・`+` の fn の名・base の歯 `derive_ok` を持つ）。
fn promised_doc() -> String {
    let row: String = table_row("a", &[])
        .lines()
        .filter(|line| !["write-set =", "verify =", "done ="].iter().any(|head| line.starts_with(head)))
        .map(|line| format!("{line}\n"))
        .collect();
    let promise = [
        "[[promise]]".to_owned(),
        "of = \"a\"".to_owned(),
        "n = 1".to_owned(),
        "text = \"約束 1\"".to_owned(),
        format!("files = [\"+{PROMISED_FILE}\"]"),
        format!("symbols = [\"+{PROMISED_NAME}\"]"),
        "teeth = [\"derive_ok\"]".to_owned(),
        "fixture = \"toy の repo\"".to_owned(),
        "expect = \"tests の歯が緑\"".to_owned(),
    ];
    table_doc(&table_region(&[format!("{row}\n{}\n", promise.join("\n"))]))
}

/// 契約の verify（約束の行が生成する `cargo nextest` の行）を rc 0 で返す偽 `cargo` を道具箱の前に置いて gate を 1 回撃つ
/// （toy repo は crate を持たない＝段 ④ を緑にして、判定を段 ① だけで分ける）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn gate_with_cargo_stub(repo: &Path, state: &Path, id: &str, lens: &str) -> Output {
    use std::os::unix::fs::PermissionsExt;
    let dir = state.join("cargo-stub");
    fs::create_dir_all(&dir).expect("偽 cargo の dir を作れる");
    let stub = dir.join("cargo");
    fs::write(&stub, "#!/bin/sh\nexit 0\n").expect("偽 cargo を書ける");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("偽 cargo に実行権を付ける");
    let path = format!("{}:{}", dir.display(), crate::toolbox_path(state));
    run_pipe_with_path(
        &path,
        &[
            "gate", "--run", id, "--repo", &repo.display().to_string(),
            "--state-dir", &state.display().to_string(), "--lens", lens,
        ],
    )
}

/// 約束の行の便を `body` を `+` の file に書く偽 runner で Implemented まで通し、偽 `cargo` の下で gate を 1 回撃つ。
fn promised_gate(body: &str) -> (PathBuf, PathBuf, String, Output) {
    let (repo, state) = derive_repo(&promised_doc());
    let id = intake(&repo, &state, "docs/design/toy.md#a");
    let copied = copied_write_set(&state, &id);
    assert!(copied.contains(&format!("+{PROMISED_FILE}")), "前提: 写しの write-set は約束の `+` の file を持つ: {copied:?}");
    let runner = format!("echo '{body}' > {PROMISED_FILE} && git add -A && git commit -q -m runner");
    let out = spawn_without_gate(&repo, &state, &id, &runner);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    let gated = gate_with_cargo_stub(&repo, &state, &id, &fake_lens(&state.join("lens-ran"), &lens_verdict("PASS")));
    (repo, state, id, gated)
}

/// (2) 約束の行を持つ toy の設計 doc で、偽 runner が `+` の file は作るがその名を宣言しない便の gate は FAIL で、段 ① の
/// record が rc 1、診断が見出しの下にその名を名指す（file は在るので `+` の file の外れは出ない）。宣言した便は PASS。
#[test]
fn pipe_gate_promised_new_name_not_declared_fails_stage_one() {
    let (repo, state, id, out) = promised_gate("pub fn other_helper() {}");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "宣言しなかった名は FAIL: {}", stderr_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 1, "cmd"), "write-set", "段 ① は先頭（母集団 {} record）", rows.len());
    assert_eq!(row_value(&rows, 1, "rc"), "1", "名の外れは段 ① の rc 1");
    assert!(rows.iter().skip(1).all(|row| value_of(row, "rc") == "0"), "他の段は緑（判定は段 ① だけで分かれる）: {rows:?}");
    let log = gate_log(&state, &id);
    assert!(log.contains(&format!("{UNRESOLVED_HEAD}\n{PROMISED_NAME}")), "見出しの下に名を名指す: {log}");
    assert!(!log.contains(ABSENT_HEAD), "file は作った（`+` の file の外れは出ない）: {log}");
    clean(&[&repo, &state]);

    let (repo, state, id, out) = promised_gate("pub fn fresh_helper() {}");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "宣言した便は PASS: {}", stderr_of(&out));
    assert_eq!(row_value(&verify_rows(&state, &id), 1, "rc"), "0", "段 ① が緑");
    clean(&[&repo, &state]);
}

/// 写しの write-set に足す、便の木に無い `+` の項目。
const LAND_ABSENT: &str = "src/promised_absent.rs";

/// (3) PASS した便の写し（`contract_path` の file）の write-set に便の木に無い `+` の項目を 1 つ足し、主実測を撃たせる
/// （[`super::super::land::make_tree_differ`]）と、land は main-red（rc 1）で終わり、`verify-main.jsonl` の段 ① の record が
/// rc 1、同じ stem の診断 file がその path を名指す。
#[test]
fn pipe_gate_promised_land_main_measure_fails_on_absent_plus_item() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let copy = vessel::pipe::contract_path(&state, &id);
    let text = fs::read_to_string(&copy).unwrap_or_default();
    let written = "write-set = [\"src/lib.rs\"]\n";
    assert!(text.contains(written), "前提: 写しの write-set の字面: {text}");
    let widened = text.replace(written, &format!("write-set = [\"src/lib.rs\", \"+{LAND_ABSENT}\"]\n"));
    fs::write(&copy, widened).expect("写しを書き換えられる");
    super::super::land::make_tree_differ(&repo, &state, &id, "refs/heads/main");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "主実測の段 ① が赤い land は rc 1: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("main-red".to_owned()))),
        "終端は main-red: {:?}",
        stages(&state, &id)
    );
    let rows = main_rows(&state, &id);
    let first = rows.iter().find(|row| value_of(row, "cmd") == "write-set").expect("主実測の段 ① の record が在る");
    assert_eq!(value_of(first, "rc"), "1", "主実測の段 ① が rc 1: {first:?}");
    let log = fs::read_to_string(run_dir(&state, &id).join("verify-main.stderr.log")).unwrap_or_default();
    assert!(log.contains(&format!("{ABSENT_HEAD}\n{LAND_ABSENT}")), "主実測の診断が path を名指す: {log}");
    clean(&[&repo, &state]);
}
