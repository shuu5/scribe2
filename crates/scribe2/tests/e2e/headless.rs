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

/// lens へ渡す契約の各面（歯が prompt の中で照合する値）。
///
/// **値を歯の中で直書きしない**。契約 file と assert が同じ定数を見ることで、
/// 「prompt に載った」と「契約に書いた」が同じものを指す。
///
/// ★**diff にも lens.txt にも現れない字面を選ぶ**。`src/lib.rs` のような値を使うと、
/// 歯が渡す diff の `--- a/src/lib.rs` が同じ字面を持ち、`state()` が write-set を
/// 1 文字も出さなくても assert が真になる（review 2026-09-10 F1・実測で再現した）。
const CONTRACT_GOAL: &str = "縦 1 本を通す";
/// 契約の done（[`CONTRACT_GOAL`] と対）。
const CONTRACT_DONE: &str = "run が Implemented になる";
/// 契約の verify 1 行目（[`CONTRACT_GOAL`] と対）。
const CONTRACT_VERIFY: &str = "cargo nextest run --no-tests=fail";
/// 契約の verify 2 行目。**2 要素にする**——1 要素だと「2 本目以降を捨てる」変異が生き残る。
const CONTRACT_VERIFY_2: &str = "cargo deny check bans";
/// 契約の write-set 1 行目（[`CONTRACT_GOAL`] と対）。
const CONTRACT_WRITE_SET: &str = "crates/vessel-unlikely/src/only-here.rs";
/// 契約の write-set 2 行目（[`CONTRACT_VERIFY_2`] と同じ理由で 2 要素）。
const CONTRACT_WRITE_SET_2: &str = "crates/vessel-unlikely/src/second-only.rs";
/// 契約の owner。**prompt に載ってはならない**面（判定の材料にならない）。
const CONTRACT_OWNER: &str = "s2-07l-owner-marker";
/// 契約の disposition。[`CONTRACT_OWNER`] と同じく載ってはならない面。
const CONTRACT_DISPOSITION: &str = "A-now";

/// 契約 file の本文を組む（`goal` だけ差し替えられる）。
fn contract_text(goal: &str) -> String {
    format!(
        "goal = \"{goal}\"\n\
         done = \"{CONTRACT_DONE}\"\n\
         size = \"S\"\n\
         owner = \"{CONTRACT_OWNER}\"\n\
         disposition = \"{CONTRACT_DISPOSITION}\"\n\
         write-set = [\"{CONTRACT_WRITE_SET}\", \"{CONTRACT_WRITE_SET_2}\"]\n\
         verify = [\"{CONTRACT_VERIFY}\", \"{CONTRACT_VERIFY_2}\"]\n\
         req = [\"FR9\"]\n\
         design = \"docs/design/pipeline.md\"\n"
    )
}

/// lens へ渡す契約 file を 1 本書く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn contract_in(dir: &Path) -> PathBuf {
    let path = dir.join("contract.toml");
    fs::write(&path, contract_text(CONTRACT_GOAL)).expect("契約 file を書ける");
    path
}

/// lens を 1 回撃つ（引数の並びが複数の歯で同じなので畳む）。
///
/// `--worktree` は必須だが、この helper を使う歯の関心は cwd ではないので**契約の
/// 置き場**を渡す（引数を 1 本増やすと粒度 lint の上限に当たる）。cwd がその worktree
/// であることは [`headless_lens_runs_claude_in_the_given_worktree`] が**別 dir**で測る。
fn run_lens(contract: &Path, cap: &str, mode: &str, claude: &Path, diff: &[u8]) -> Output {
    let worktree = contract.parent().unwrap_or(Path::new(".")).display().to_string();
    run_bin(
        &[
            "lens",
            "--contract", &contract.display().to_string(),
            "--worktree", &worktree,
            "--cap", cap,
            "--permission-mode", mode,
            "--claude", &claude.display().to_string(),
        ],
        diff,
    )
}

/// runner を 1 回撃つための材料（引数の並びが複数の歯で同じなので畳む）。
struct RunnerCall<'a> {
    /// plugin dir 兼 fake の置き場。
    dir: &'a Path,
    /// 実装させる worktree。
    worktree: &'a Path,
    /// write-set の file。
    write_set: &'a Path,
    /// 便ごとに凍結した vessel の写し（**権限の出所**）。
    vessel: &'a Path,
    /// claude の実行 file（fake）。
    claude: &'a Path,
    /// permission mode。
    mode: &'a str,
    /// 口座の設定 dir（渡さない周は親から継承される）。
    account: Option<&'a Path>,
}

/// runner が読む vessel の写し（intake が凍結する形と同じ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_vessel_copy(dir: &Path, allowed: &str) -> PathBuf {
    let body = format!(
        "schema = 1\nallowed-commands = {allowed}\ncommon-verify = [\"cargo xtask check\"]\n\
         commit = \"c0ffee\"\nsource = \".vessel.toml\"\nceiling = \"runner.allowed_commands\"\n"
    );
    let path = dir.join("vessel.toml");
    fs::write(&path, body).expect("vessel の写しを書ける");
    path
}

/// runner を 1 回撃つ。
fn run_runner(call: &RunnerCall<'_>, input: &[u8]) -> Output {
    let mut args = vec![
        "runner".to_owned(),
        "--worktree".to_owned(),
        call.worktree.display().to_string(),
        "--write-set".to_owned(),
        call.write_set.display().to_string(),
        "--vessel".to_owned(),
        call.vessel.display().to_string(),
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
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\ndocs/\n").expect("write-set を書ける");
    let contract = "goal = \"縦 1 本を通す\"\nverify = [\"true\"]\n";

    // **毎回**明示することを測るので、mode を変えて 2 度撃つ。1 度だけだと「既定が
    // たまたま一致していた」形と区別できない。
    for mode in ["acceptEdits", "plan"] {
        let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode, account: Some(&account) },
        contract.as_bytes(),
    );
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
        assert_runner_call(&dir, &worktree, &account, mode);
    }
    // **契約本文が prompt の構造へ触れられない**（置換は 1 走査）。契約は外から来る text で、
    // 重ねて replace すると契約の中の marker まで後段で展開される。
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        "goal = \"契約の中に {write_set} と書く\"\n".as_bytes(),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    assert!(prompt.contains("{write_set}"), "契約の中の marker はそのまま残る: {prompt}");
    assert_eq!(prompt.matches("src/lib.rs").count(), 1, "write-set の展開は 1 度だけ: {prompt}");

    // 前提違反は断る。**読めなかったものを空として続けない**（空の契約で claude を起こすと、
    // 何を作るのか分からないまま worktree を触らせることになる）。
    let empty = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"   \n",
    );
    assert_eq!(empty.status.code(), Some(i32::from(RC_REFUSED)), "空の契約は断る");
    let absent = dir.join("no-such-file");
    let missing = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &absent, vessel: &vessel, claude: &claude, mode: "plan", account: None },
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
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
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
    let contract = contract_in(&dir);
    let diff = vec![b'x'; 4096];
    let out = run_lens(&contract, "16", "plan", &claude, &diff);
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
    // mode を歯 4 と変えてある。lens 側の permission mode を定数へ固定する変異は、
    // 1 種類しか撃たない歯では捕まらない（実測で生存した）。
    let out = run_lens(&contract, "150000", "acceptEdits", &verdict, &big);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "cap の内側なので claude を呼ぶ");
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"PASS","evidence":"大きくても読めた"}"#,
        "128KiB 超でも判定を返す"
    );
    let args = slurp(&dir.join("args"));
    assert!(
        args.lines().collect::<Vec<_>>().windows(2).any(|w| {
            w.first() == Some(&"--permission-mode") && w.get(1) == Some(&"acceptEdits")
        }),
        "lens も permission mode を毎回明示する: {args}"
    );

    // **境界ちょうど（diff の byte 数 == cap）は cap の内側**である。`>` を `>=` に
    // すり替える変異は、境界を撃たない歯では捕まらない（実測で生存した）。
    let edge = tmp();
    let at_cap = fake_claude(&edge, "{\"verdict\":\"FAIL\",\"evidence\":\"境界は内側\"}\n", false, 0);
    let edge_contract = contract_in(&edge);
    let out = run_lens(&edge_contract, "64", "plan", &at_cap, &vec![b'y'; 64]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(edge.join("called").exists(), "境界ちょうどでは claude を呼ぶ");
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"FAIL","evidence":"境界は内側"}"#,
        "境界ちょうどは判定を返す"
    );
    clean(&[&edge]);

    // **cap が数でないときは断る**（上限なしで走らせない＝C6）。
    let bad = run_lens(&contract, "abc", "plan", &verdict, b"--- a\n");
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
    let contract = contract_in(&dir);
    let out = run_bin(
        &[
            "lens", "--contract", &contract.display().to_string(),
            "--worktree", &dir.display().to_string(),
            "--cap", "4096", "--permission-mode", "plan",
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
    let contract = contract_in(&dir);
    let out = run_lens(&contract, "4096", "plan", &claude, b"--- a\n+++ b\n");
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
fn headless_lens_prompt_includes_contract_fields() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"契約を読めた\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, "4096", "plan", &claude, b"--- a/src/lib.rs\n+++ b/src/lib.rs\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "契約が在るので claude を呼ぶ");
    let prompt = slurp(&dir.join("stdin"));
    // **契約の 4 面が prompt に載る**（設計 §6「契約の verify と diff を読み」）。diff だけを
    // 渡して契約適合は問えない——実 lens は「契約が未提供で適合を判定できない」と
    // INCONCLUSIVE を返し、便がそこで止まった（実測 2026-09-10・s2-07l.24 の実 5 便）。
    //
    // **値の存在ではなく「どの見出しの下に在るか」まで測る**。値だけを数えると、goal と
    // done のラベルを入れ替える変異が生き残る（review 2026-09-10 F3）。
    assert!(prompt.contains(&format!("goal: {CONTRACT_GOAL}")), "goal が goal として載る: {prompt}");
    assert!(prompt.contains(&format!("done: {CONTRACT_DONE}")), "done が done として載る: {prompt}");
    // **配列は各行**。1 要素しか測らないと「2 本目以降を捨てる」変異が生き残る。
    for want in [CONTRACT_VERIFY, CONTRACT_VERIFY_2, CONTRACT_WRITE_SET, CONTRACT_WRITE_SET_2] {
        assert!(prompt.contains(&format!("- {want}")), "prompt に契約の {want} が行として載る: {prompt}");
    }
    // **契約 file を丸写ししない**（NFR1・渡すほど cap を食う）。判定の材料にならない面が
    // 載っていないことまで測らないと、丸写しへ戻す変異が生き残る。
    for unwanted in [CONTRACT_OWNER, CONTRACT_DISPOSITION] {
        assert!(!prompt.contains(unwanted), "判定の材料にならない {unwanted} は載せない: {prompt}");
    }
    assert!(prompt.contains("--- a/src/lib.rs"), "diff も従来どおり載る: {prompt}");
    // 穴が埋まらずに残っていたら、lens が読むのは placeholder の字面だけになる。
    assert!(!prompt.contains("{contract}"), "契約の穴が埋まっている: {prompt}");
    assert!(!prompt.contains("{diff}"), "diff の穴が埋まっている: {prompt}");
    clean(&[&dir]);
}

#[test]
fn headless_lens_fills_holes_in_one_pass() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
    // **契約は外から来る text である**。goal に穴の字面を書けるので、重ねて replace すると
    // 先に埋めた契約本文の中の `{diff}` が次の走査で展開され、契約に 1 語書くだけで
    // prompt の構造へ触れられる。runner 側と同じ経路を lens でも測る（review 2026-09-10 F2）。
    let contract = dir.join("holes.toml");
    fs::write(&contract, contract_text("穴の字面 {diff} を持つ goal")).expect("契約 file を書ける");
    let out = run_lens(&contract, "4096", "plan", &claude, b"DIFF-BODY-MARKER\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    // 埋めた値を二度と走査しない＝契約に書いた穴の字面は**そのまま残る**。
    assert!(
        prompt.contains("穴の字面 {diff} を持つ goal"),
        "契約本文の穴は展開されない: {prompt}"
    );
    // 展開されていれば diff の本文が契約の中にも現れ、2 か所になる。
    assert_eq!(
        prompt.matches("DIFF-BODY-MARKER").count(),
        1,
        "diff が載るのは 1 か所だけ: {prompt}"
    );
    clean(&[&dir]);
}

/// lens は**渡された worktree で** claude を起こす（憲法を載せる経路は cwd 1 本）。
///
/// worktree は fake の置き場と**別の dir** にする——同じにすると「cwd を渡さず継承した」
/// 実装でも assert が真になり、歯が空虚になる。
#[test]
fn headless_lens_runs_claude_in_the_given_worktree() {
    let dir = tmp();
    let worktree = tmp();
    let contract = contract_in(&dir);
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"ok\"}\n", false, 0);
    let out = run_bin(
        &[
            "lens",
            "--contract", &contract.display().to_string(),
            "--worktree", &worktree.display().to_string(),
            "--cap", "4096",
            "--permission-mode", "plan",
            "--claude", &claude.display().to_string(),
        ],
        b"--- a\n+++ b\n",
    );
    assert_eq!(out.status.code(), Some(0), "判定は返る: {}", stderr_of(&out));
    let seen = slurp(&dir.join("cwd"));
    assert_eq!(seen.trim(), worktree.display().to_string(), "cwd は渡された worktree");
    assert_ne!(seen.trim(), dir.display().to_string(), "契約の置き場を cwd にしていない");
    clean(&[&dir, &worktree]);
}

/// `--worktree` が無ければ claude を起こさずに断る（`--contract` と同じ極性）。
#[test]
fn headless_lens_refuses_without_worktree() {
    let dir = tmp();
    let contract = contract_in(&dir);
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let out = run_bin(
        &[
            "lens",
            "--contract", &contract.display().to_string(),
            "--cap", "4096", "--permission-mode", "plan",
            "--claude", &claude.display().to_string(),
        ],
        b"--- a\n+++ b\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "worktree が無ければ rc 1");
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    assert!(
        stderr_of(&out).contains("--worktree"),
        "何が要るかを名乗る: {}",
        stderr_of(&out)
    );
    clean(&[&dir]);
}

#[test]
fn headless_lens_refuses_without_contract() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let out = run_bin(
        &[
            "lens", "--cap", "4096", "--permission-mode", "plan",
            "--claude", &claude.display().to_string(),
        ],
        b"--- a\n+++ b\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "契約が無ければ rc 1");
    // **効果で測る**: 材料が足りないまま呼べば返るのは INCONCLUSIVE だけで、払った
    // 1 回分が捨て金になる。呼んでいないことを痕跡の不在で見る（cap 超過の歯と同型）。
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    assert!(
        stderr_of(&out).contains("--contract"),
        "何が要るかを名乗る: {}",
        stderr_of(&out)
    );
    // 読めない契約でも claude を呼ばない（「無い」と「壊れている」で極性を変えない）。
    let broken = dir.join("broken.toml");
    fs::write(&broken, "goal = \n").expect("壊れた契約を書ける");
    let out = run_lens(&broken, "4096", "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない契約は rc 2");
    assert!(!dir.join("called").exists(), "読めない契約でも claude を起動しない");
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
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    // **包みが rc を作り替えない**（設計 §6）。呼出側は runner の rc で便の成否を読むので、
    // ここで潰すと失敗した実装便が成功として通る。
    for want in [0_u8, 1, 3] {
        let claude = fake_claude(&dir, "", false, want);
        let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
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
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
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
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
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
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
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
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
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
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &other, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(1), "上限でない error は claude の rc を写す");
    clean(&[&dir, &worktree]);
}

/// **`--vessel` は必須**（lens の `--contract` と同じ極性）。権限の出所が無いまま
/// claude を起こすと、起動口座の settings を継承した席が worktree を触る。
#[test]
fn headless_runner_requires_vessel_flag() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let out = run_bin(
        &[
            "runner",
            "--worktree",
            &worktree.display().to_string(),
            "--write-set",
            &write_set.display().to_string(),
            "--plugin-dir",
            &dir.display().to_string(),
            "--permission-mode",
            "plan",
            "--claude",
            &claude.display().to_string(),
        ],
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "--vessel 無しは rc 1");
    assert!(stderr_of(&out).contains("--vessel"), "何が要るかを名指す: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "claude を起動しない");

    // **「無い」と「壊れている」で極性を変える**（lens の契約と同じ）。
    let absent = dir.join("no-such-vessel.toml");
    let broken = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &absent, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(broken.status.code(), Some(i32::from(RC_BROKEN)), "読めない写しは rc 2");
    assert!(!dir.join("called").exists(), "読めない写しでも claude を起動しない");
    // 束縛だけの vessel を使う（正常形は次の歯が測る）。
    assert!(vessel.exists(), "写しは在る");
    clean(&[&dir, &worktree]);
}

/// 権限は**便の写しから**組む（manifest も repo の宣言も読まない）。
#[test]
fn headless_runner_passes_allowed_tools_from_vessel_copy() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let pair = |args: &str, flag: &str, value: &str| {
        let lines: Vec<&str> = args.lines().collect();
        lines.windows(2).any(|w| w.first() == Some(&flag) && w.get(1) == Some(&value))
    };

    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let args = slurp(&dir.join("args"));
    assert!(pair(&args, "--setting-sources", "project"), "起動口座の settings を継承しない: {args}");
    assert!(
        pair(&args, "--allowedTools", "Bash(cargo:*),Bash(git:*)"),
        "写しの allowlist を Bash(<cmd>:*) で与える: {args}"
    );

    // **写しが変われば権限も変わる**＝上限（manifest の cargo / git）を読んでいない。
    let narrow = write_vessel_copy(&dir, r#"["git"]"#);
    let again = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &narrow, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&again));
    let narrowed = slurp(&dir.join("args"));
    assert!(pair(&narrowed, "--allowedTools", "Bash(git:*)"), "狭めた写しに従う: {narrowed}");
    assert!(!narrowed.contains("cargo"), "写しに無い command は与えない: {narrowed}");
    clean(&[&dir, &worktree]);
}
