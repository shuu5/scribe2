//! (d) headless の runner と lens の歯（設計 docs/design/pipeline.md §8 (d)）。
//!
//! claude は **fake の実行 file**（`--claude` の seam）で、引数・cwd・口座 env を file へ
//! 写し、決めた body を stdout へ出す。**CI で実 claude は呼ばない**。

use crate::make_tmp_dir;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use vessel::cli_outcome::RC_OK;
use vessel::headless::RC_RATE_LIMIT;

/// binary の path。
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_scribe2")
}

/// tmp dir を 1 つ作り、symlink を解いた path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn tmp() -> PathBuf {
    let dir = make_tmp_dir().expect("tmp dir を作れる");
    dir.canonicalize().expect("tmp dir の実体 path を解ける")
}

/// fake claude を 1 本作る。
///
/// 起動されたら `called` を残し、引数を `args`・cwd を `cwd`・口座 env を `account` へ
/// 写してから `body` を stdout へ出す。
///
/// `lingering` が真のときだけ、body の後に**眠ってから** `tail-ran` を残す。呼び手が
/// 途中で殺したかどうかを rc でなく**痕跡の不在**で測るための印で、要る歯は 1 本だけ
/// である（全 fake に眠らせると、他の歯が待つだけの秒を払う）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_claude(dir: &Path, body: &str, lingering: bool) -> PathBuf {
    let d = dir.display().to_string();
    let tail = if lingering { format!("sleep 5\n: > \"{d}/tail-ran\"\n") } else { String::new() };
    fs::write(dir.join("body"), body).expect("body を書ける");
    let script = format!(
        "#!/bin/sh\n\
         : > \"{d}/called\"\n\
         printf '%s\\n' \"$@\" > \"{d}/args\"\n\
         pwd > \"{d}/cwd\"\n\
         printf '%s' \"$CLAUDE_CONFIG_DIR\" > \"{d}/account\"\n\
         cat \"{d}/body\"\n\
         {tail}"
    );
    let path = dir.join("fake-claude");
    fs::write(&path, script).expect("fake を書ける");
    let mut perm = fs::metadata(&path).expect("fake の権限を読める").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("fake を実行可能にできる");
    path
}

/// binary を 1 回撃つ。stdin には `input` を流す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_bin(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary を起動できる");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input);
    }
    child.wait_with_output().expect("binary の出力を読める")
}

/// stdout の全文。
fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// stderr の全文。
fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// file の中身（無ければ空）。
fn slurp(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// 後片付け。
fn clean(dirs: &[&Path]) {
    for dir in dirs {
        fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn headless_runner_reads_contract_from_stdin_and_passes_permission_mode_every_time() {
    let dir = tmp();
    let worktree = tmp();
    let account = tmp();
    let claude = fake_claude(&dir, "", false);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\ndocs/\n").expect("write-set を書ける");
    let contract = "goal = \"縦 1 本を通す\"\nverify = [\"true\"]\n";

    // **毎回**明示することを測るので、mode を変えて 2 度撃つ。1 度だけだと「既定が
    // たまたま一致していた」形と区別できない。
    for mode in ["acceptEdits", "plan"] {
        let out = run_bin(
            &[
                "runner", "--worktree", &worktree.display().to_string(),
                "--write-set", &write_set.display().to_string(),
                "--plugin-dir", &dir.display().to_string(),
                "--permission-mode", mode,
                "--account-dir", &account.display().to_string(),
                "--claude", &claude.display().to_string(),
            ],
            contract.as_bytes(),
        );
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
        let args = slurp(&dir.join("args"));
        let lines: Vec<&str> = args.lines().collect();
        // 引数は「flag の直後に値」の対で渡る。字面の混入でなく **対** を測る。
        let pair = |flag: &str, value: &str| {
            lines.windows(2).any(|w| w.first() == Some(&flag) && w.get(1) == Some(&value))
        };
        assert!(pair("--permission-mode", mode), "permission mode を毎回明示する: {args}");
        assert!(pair("--output-format", "stream-json"), "stream-json で回す: {args}");
        assert!(pair("--plugin-dir", &dir.display().to_string()), "plugin を載せる: {args}");
        // **契約は stdin から来て prompt に載る**（argv で契約を渡さない形の裏返し）。
        assert!(args.contains("goal = \"縦 1 本を通す\""), "契約が prompt に載る: {args}");
        assert!(args.contains("src/lib.rs"), "write-set が prompt に載る: {args}");
        // 効果で測る: cwd は worktree・口座は子の環境変数へ書かれている。
        assert_eq!(slurp(&dir.join("cwd")).trim(), worktree.display().to_string(), "cwd は worktree");
        assert_eq!(slurp(&dir.join("account")), account.display().to_string(), "口座は子の env へ");
    }
    clean(&[&dir, &worktree, &account]);
}

#[test]
fn headless_runner_stops_on_rate_limit_record() {
    let dir = tmp();
    let worktree = tmp();
    let body = concat!(
        "{\"type\":\"system\",\"subtype\":\"init\"}\n",
        "{\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\"}}\n",
        "{\"type\":\"assistant\",\"text\":\"この先は読まれてはならない\"}\n"
    );
    let claude = fake_claude(&dir, body, true);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let out = run_bin(
        &[
            "runner", "--worktree", &worktree.display().to_string(),
            "--write-set", &write_set.display().to_string(),
            "--plugin-dir", &dir.display().to_string(),
            "--permission-mode", "acceptEdits",
            "--claude", &claude.display().to_string(),
        ],
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_RATE_LIMIT)), "rate limit は rc 75");
    // **効果で測る**: 名乗るだけでなく、実際にその場で止めている（fake は body の後に
    // 5 秒眠ってから痕跡を残すので、残っていれば最後まで走らせてしまった証拠になる）。
    assert!(!dir.join("tail-ran").exists(), "record を見た時点で止める");
    clean(&[&dir, &worktree]);
}

#[test]
fn headless_lens_inconclusive_over_cap_without_calling_claude() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false);
    let diff = vec![b'x'; 4096];
    let out = run_bin(
        &[
            "lens", "--cap", "16", "--permission-mode", "plan",
            "--claude", &claude.display().to_string(),
        ],
        &diff,
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"INCONCLUSIVE","evidence":"diff exceeds cap"}"#,
        "cap 超過は INCONCLUSIVE"
    );
    // **効果で測る**: cap の意味は「呼ばないこと」なので、呼んでいないことを痕跡で見る。
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    clean(&[&dir]);
}

#[test]
fn headless_lens_extracts_last_json_line() {
    let dir = tmp();
    let body = concat!(
        "diff を読んでいます\n",
        "{\"verdict\":\"INCONCLUSIVE\",\"evidence\":\"まだ途中\"}\n",
        "考え直しました\n",
        "{\"verdict\":\"FAIL\",\"evidence\":\"最後の判定\"}\n",
        "おしまい\n"
    );
    let claude = fake_claude(&dir, body, false);
    let out = run_bin(
        &[
            "lens", "--cap", "4096", "--permission-mode", "plan",
            "--claude", &claude.display().to_string(),
        ],
        b"--- a\n+++ b\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "cap 内なので claude を呼ぶ");
    // 途中の JSON でも末尾の地の文でもなく、**最後の JSON 行**ちょうど 1 行。
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"FAIL","evidence":"最後の判定"}"#,
        "最後の JSON 行を写す"
    );
    assert_eq!(stdout_of(&out).lines().count(), 1, "stdout は 1 行だけ");
    clean(&[&dir]);
}

#[test]
fn headless_lens_inconclusive_on_unparsable_output() {
    let dir = tmp();
    let claude = fake_claude(&dir, "判定できませんでした\nもう一度お願いします\n", false);
    let out = run_bin(
        &[
            "lens", "--cap", "4096", "--permission-mode", "plan",
            "--claude", &claude.display().to_string(),
        ],
        b"--- a\n+++ b\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "呼んだ上で読めなかった周である");
    // 読めない出力を握り潰さず、**判定に届かなかった**と名乗る（偽の PASS を作らない）。
    assert!(
        stdout_of(&out).contains(r#""verdict":"INCONCLUSIVE""#),
        "parse 不能は INCONCLUSIVE: {}",
        stdout_of(&out)
    );
    clean(&[&dir]);
}

#[test]
fn headless_external_form() {
    let args = |raw: &[&str]| raw.iter().map(|item| (*item).to_owned()).collect::<Vec<String>>();
    let mut lines = vec![vessel::headless::runner::usage(), vessel::headless::lens::usage()];
    lines.extend(vessel::headless::runner::dispatch(&args(&[])).err);
    lines.extend(vessel::headless::lens::dispatch(&args(&[])).err);
    let form = lines.join("\n");
    insta::assert_snapshot!(form);
}
