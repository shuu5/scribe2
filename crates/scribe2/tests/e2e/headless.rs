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
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
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
fn fake_claude(dir: &Path, body: &str, lingering: bool, rc: u8) -> PathBuf {
    let d = dir.display().to_string();
    let tail = if lingering { format!("sleep 5\n: > \"{d}/tail-ran\"\n") } else { String::new() };
    fs::write(dir.join("body"), body).expect("body を書ける");
    let script = format!(
        "#!/bin/sh\n\
         : > \"{d}/called\"\n\
         printf '%s\\n' \"$@\" > \"{d}/args\"\n\
         cat > \"{d}/stdin\"\n\
         pwd > \"{d}/cwd\"\n\
         printf '%s' \"$CLAUDE_CONFIG_DIR\" > \"{d}/account\"\n\
         cat \"{d}/body\"\n\
         {tail}exit {rc}\n"
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

/// runner を 1 回撃つための材料（引数の並びが複数の歯で同じなので畳む）。
struct RunnerCall<'a> {
    /// plugin dir 兼 fake の置き場。
    dir: &'a Path,
    /// 実装させる worktree。
    worktree: &'a Path,
    /// write-set の file。
    write_set: &'a Path,
    /// claude の実行 file（fake）。
    claude: &'a Path,
    /// permission mode。
    mode: &'a str,
    /// 口座の設定 dir（渡さない周は親から継承される）。
    account: Option<&'a Path>,
}

/// runner を 1 回撃つ。
fn run_runner(call: &RunnerCall<'_>, input: &[u8]) -> Output {
    let mut args = vec![
        "runner".to_owned(),
        "--worktree".to_owned(),
        call.worktree.display().to_string(),
        "--write-set".to_owned(),
        call.write_set.display().to_string(),
        "--plugin-dir".to_owned(),
        call.dir.display().to_string(),
        "--permission-mode".to_owned(),
        call.mode.to_owned(),
        "--claude".to_owned(),
        call.claude.display().to_string(),
    ];
    if let Some(found) = call.account {
        args.push("--account-dir".to_owned());
        args.push(found.display().to_string());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_bin(&refs, input)
}

/// runner が claude へ渡した引数と prompt を測る（歯 1 が mode ごとに 2 度使う）。
fn assert_runner_call(dir: &Path, worktree: &Path, account: &Path, mode: &str) {
    let args = slurp(&dir.join("args"));
    let lines: Vec<&str> = args.lines().collect();
    // 引数は「flag の直後に値」の対で渡る。字面の混入でなく **対** を測る。
    let pair = |flag: &str, value: &str| {
        lines.windows(2).any(|w| w.first() == Some(&flag) && w.get(1) == Some(&value))
    };
    assert!(pair("--permission-mode", mode), "permission mode を毎回明示する: {args}");
    assert!(pair("--output-format", "stream-json"), "stream-json で回す: {args}");
    assert!(lines.contains(&"--verbose"), "stream-json には --verbose が要る: {args}");
    assert!(pair("--plugin-dir", &dir.display().to_string()), "plugin を載せる: {args}");
    assert!(lines.contains(&"-p"), "headless で回す（-p が要る）: {args}");
    // **在ってはならない flag が無いこと**も測る。在ってほしい flag だけを見ていると、
    // 権限を丸ごと外す flag が黙って混入しても気づけない。
    assert!(
        !lines.iter().any(|line| line.starts_with("--dangerously")),
        "権限を外す flag を渡さない: {args}"
    );
    // **prompt は argv でなく子の stdin へ渡る**（argv だと 1 引数 128KiB の壁に当たり、
    // user 裁定の cap 150000 が実質 130KB へ切り下がる）。
    let prompt = slurp(&dir.join("stdin"));
    assert!(prompt.contains("goal = \"縦 1 本を通す\""), "契約が prompt に載る: {prompt}");
    assert!(prompt.contains("src/lib.rs"), "write-set が prompt に載る: {prompt}");
    assert!(!args.contains("goal = "), "契約を argv では渡さない: {args}");
    // 効果で測る: cwd は worktree・口座は子の環境変数へ書かれている。
    assert_eq!(slurp(&dir.join("cwd")).trim(), worktree.display().to_string(), "cwd は worktree");
    assert_eq!(slurp(&dir.join("account")), account.display().to_string(), "口座は子の env へ");
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
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\ndocs/\n").expect("write-set を書ける");
    let contract = "goal = \"縦 1 本を通す\"\nverify = [\"true\"]\n";

    // **毎回**明示することを測るので、mode を変えて 2 度撃つ。1 度だけだと「既定が
    // たまたま一致していた」形と区別できない。
    for mode in ["acceptEdits", "plan"] {
        let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, claude: &claude, mode, account: Some(&account) },
        contract.as_bytes(),
    );
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
        assert_runner_call(&dir, &worktree, &account, mode);
    }
    // **契約本文が prompt の構造へ触れられない**（置換は 1 走査）。契約は外から来る text で、
    // 重ねて replace すると契約の中の marker まで後段で展開される。
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, claude: &claude, mode: "plan", account: None },
        "goal = \"契約の中に {write_set} と書く\"\n".as_bytes(),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    assert!(prompt.contains("{write_set}"), "契約の中の marker はそのまま残る: {prompt}");
    assert_eq!(prompt.matches("src/lib.rs").count(), 1, "write-set の展開は 1 度だけ: {prompt}");

    // 前提違反は断る。**読めなかったものを空として続けない**（空の契約で claude を起こすと、
    // 何を作るのか分からないまま worktree を触らせることになる）。
    let empty = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, claude: &claude, mode: "plan", account: None },
        b"   \n",
    );
    assert_eq!(empty.status.code(), Some(i32::from(RC_REFUSED)), "空の契約は断る");
    let absent = dir.join("no-such-file");
    let missing = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &absent, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(missing.status.code(), Some(i32::from(RC_BROKEN)), "write-set を読めない周は rc 2");
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
    let claude = fake_claude(&dir, body, true, 0);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, claude: &claude, mode: "acceptEdits", account: None },
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
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
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

    // **cap の内側は 128KiB を超えても判定を返す**。prompt を argv で渡すと Linux の
    // 1 引数上限（131072 byte）に当たり、user が裁定した cap 150000 が実質 130KB へ
    // 黙って切り下がる（実測 2026-09-10）。境界の内側で判定が返ることを測る。
    let verdict = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"大きくても読めた\"}\n", false, 0);
    let big = vec![b'x'; 140_000];
    let out = run_bin(
        &[
            "lens", "--cap", "150000", "--permission-mode", "plan",
            "--claude", &verdict.display().to_string(),
        ],
        &big,
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "cap の内側なので claude を呼ぶ");
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"PASS","evidence":"大きくても読めた"}"#,
        "128KiB 超でも判定を返す"
    );

    // **cap が数でないときは断る**（上限なしで走らせない＝C6）。
    let bad = run_bin(
        &[
            "lens", "--cap", "abc", "--permission-mode", "plan",
            "--claude", &verdict.display().to_string(),
        ],
        b"--- a\n",
    );
    assert_eq!(bad.status.code(), Some(i32::from(RC_REFUSED)), "cap が数でないなら断る");
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
    let account = tmp();
    let claude = fake_claude(&dir, body, false, 0);
    let out = run_bin(
        &[
            "lens", "--cap", "4096", "--permission-mode", "plan",
            "--account-dir", &account.display().to_string(),
            "--claude", &claude.display().to_string(),
        ],
        b"--- a\n+++ b\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "cap 内なので claude を呼ぶ");
    // lens 側の引数も runner と同じだけ測る（片側だけ測ると、もう片側は自由に壊れる）。
    let args = slurp(&dir.join("args"));
    let lines: Vec<&str> = args.lines().collect();
    let pair = |flag: &str, value: &str| {
        lines.windows(2).any(|w| w.first() == Some(&flag) && w.get(1) == Some(&value))
    };
    assert!(lines.contains(&"-p"), "headless で回す: {args}");
    assert!(pair("--permission-mode", "plan"), "permission mode を毎回明示する: {args}");
    assert!(
        !lines.iter().any(|line| line.starts_with("--dangerously")),
        "権限を外す flag を渡さない: {args}"
    );
    assert_eq!(slurp(&dir.join("account")), account.display().to_string(), "口座は子の env へ");
    assert!(slurp(&dir.join("stdin")).contains("--- a"), "diff が prompt に載る");
    // **lens は既定（text）で呼ぶ**。stream-json にすると全行が JSON になり、
    // 「最後の JSON 行」が claude 自身の result record になって判定が取れない。
    let args = slurp(&dir.join("args"));
    assert!(
        !args.lines().any(|line| line == "--output-format"),
        "lens は出力形式を指定しない: {args}"
    );
    // 途中の JSON でも末尾の地の文でもなく、**最後の JSON 行**ちょうど 1 行。
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"FAIL","evidence":"最後の判定"}"#,
        "最後の JSON 行を写す"
    );
    assert_eq!(stdout_of(&out).lines().count(), 1, "stdout は 1 行だけ");
    clean(&[&dir, &account]);
}

#[test]
fn headless_lens_inconclusive_on_unparsable_output() {
    let dir = tmp();
    let claude = fake_claude(&dir, "判定できませんでした\nもう一度お願いします\n", false, 0);
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

#[test]
fn headless_runner_mirrors_claude_rc() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    // **包みが rc を作り替えない**（設計 §6）。呼出側は runner の rc で便の成否を読むので、
    // ここで潰すと失敗した実装便が成功として通る。
    for want in [0_u8, 1, 3] {
        let claude = fake_claude(&dir, "", false, want);
        let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
        assert_eq!(out.status.code(), Some(i32::from(want)), "claude の rc をそのまま写す");
        // 外形の 1 行も測る（C12.5）。rc だけ合っていても、呼出側が読む行が消えては困る。
        assert!(
            stdout_of(&out).contains(&format!("runner: rc={want} records=")),
            "1 行で rc と record 数を名乗る: {}",
            stdout_of(&out)
        );
    }
    clean(&[&dir, &worktree]);
}

#[test]
fn headless_runner_does_not_stop_on_quoted_rate_limit_words() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    // **応答が上限の語を引用しただけ**の record。error を名乗っていないので上限ではない。
    // この歯が無いと、契約の文言を復唱しただけで便が rc 75 で死ぬ（本 bead の契約自身が
    // その文言を含むので、自分で自分を踏む）。
    let body = concat!(
        "{\"type\":\"assistant\",\"message\":{\"text\":\"契約は rate_limit の error record で rc 75 と述べている\"}}\n",
        "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false}\n"
    );
    let claude = fake_claude(&dir, body, false, 0);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_ne!(out.status.code(), Some(i32::from(RC_RATE_LIMIT)), "引用は上限ではない");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "claude の rc を写す: {}", stderr_of(&out));
    clean(&[&dir, &worktree]);
}

#[test]
fn headless_runner_stops_on_error_record_without_rate_limit_literal() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    // 上限は `rate_limit` の字面だけで surface するとは限らない。**error を名乗る record の
    // 中**で上限の語彙を見る形なので、別の言い回しでも止まる。
    for body in [
        "{\"type\":\"result\",\"is_error\":true,\"result\":\"usage limit reached\"}\n",
        "{\"type\":\"error\",\"status\":429}\n",
        "{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"result\":\"overloaded\"}\n",
    ] {
        let claude = fake_claude(&dir, body, false, 0);
        let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
        assert_eq!(
            out.status.code(),
            Some(i32::from(RC_RATE_LIMIT)),
            "上限を名乗る error record では止まる: {body}"
        );
    }
    // **上限でない error は rc 75 にしない**（1 つの数に 2 つの意味を載せない）。
    let other = fake_claude(&dir, "{\"type\":\"result\",\"is_error\":true,\"result\":\"file not found\"}\n", false, 1);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, claude: &other, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(1), "上限でない error は claude の rc を写す");
    clean(&[&dir, &worktree]);
}
