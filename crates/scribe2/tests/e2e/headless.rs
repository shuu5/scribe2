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

/// fake が残した argv の写し（1 行 1 引数）に `flag` と `value` が **対**で在るか。
///
/// **flag の存在だけを見ない**——`args.contains("--setting-sources")` は値が `project` でも
/// 真になり、「settings を 1 つも読まない」を測ったことにならない（空虚な歯・ADR-0011 §2.1）。
fn pair(args: &str, flag: &str, value: &str) -> bool {
    let lines: Vec<&str> = args.lines().collect();
    lines.windows(2).any(|w| w.first() == Some(&flag) && w.get(1) == Some(&value))
}

/// argv の写しに `flag` が在るか。**値の連結形（`--flag=値`）も同じ 1 本として数える**。
///
/// 部分一致（`args.contains`）では数えない——値や別 flag の中の同じ字面まで拾い、不在の assert が
/// 偽陽性になる。一方で完全一致だけにすると **`--settings=/path` が「渡していない」に化ける**
/// （lens 2026-09-10 LENS-1・実測で再現: `--settings=` 形を足しても新しい歯 2 本が緑のまま通った）。
/// `--settings` は ADR-0011 §2.1 が名指しで禁じた flag で、この不在 assert が却下案 (b)
/// 「settings を消さずに足す」へ戻る経路の唯一の柵ゆえ、**分離形と連結形の両方**で見る。
fn has_arg(args: &str, flag: &str) -> bool {
    let joined = format!("{flag}=");
    args.lines().any(|line| line == flag || line.starts_with(&joined))
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
    assert!(pair(&args, "--permission-mode", mode), "permission mode を毎回明示する: {args}");
    assert!(pair(&args, "--output-format", "stream-json"), "stream-json で回す: {args}");
    assert!(lines.contains(&"--verbose"), "stream-json には --verbose が要る: {args}");
    assert!(pair(&args, "--plugin-dir", &dir.display().to_string()), "plugin を載せる: {args}");
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

/// **反転した歯**（`s2-07l.77`・[ADR-0012] §2.2）: 入れ子の `rate_limit_error` では**もう止まらない**。
///
/// 以前は record の字面に上限の語彙を当てていたので、この形で rc 75 になっていた。上限の真の
/// 合図は**専用 record（`rate_limit_event`）の構造化 status** で来ると実 run の現物で分かった
/// ため、字面照合は走査ごと撤去した。**歯を緩めたのではなく、測る対象が変わった**——この形は
/// もう「上限」ではないので、claude の rc をそのまま写す。
///
/// 効果でも測る: 以前は record を見た時点で claude を kill していたので `tail-ran` が残らなかった。
/// 止めなくなった今は fake が最後まで走る＝**痕跡が残る**。
///
/// [ADR-0012]: ../../../design-intent/decisions/ADR-0012-rate-limit-detection-reads-dedicated-record.html
#[test]
fn headless_runner_no_longer_stops_on_nested_rate_limit_error() {
    let dir = tmp();
    let worktree = tmp();
    let body = concat!(
        "{\"type\":\"system\",\"subtype\":\"init\"}\n",
        "{\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\"}}\n",
        "{\"type\":\"assistant\",\"text\":\"この先も読んでよい\"}\n"
    );
    let claude = fake_claude(&dir, body, true, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_ne!(out.status.code(), Some(i32::from(RC_RATE_LIMIT)), "字面では止まらない");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "claude の rc を写す: {}", stderr_of(&out));
    assert!(dir.join("tail-ran").exists(), "kill しないので fake は最後まで走る");
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
    assert!(lines.contains(&"-p"), "headless で回す: {args}");
    assert!(pair(&args, "--permission-mode", "plan"), "permission mode を毎回明示する: {args}");
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

/// **反転した歯**（`s2-07l.77`・ADR-0012 §2.2）: error record の本文に上限の語彙が在っても
/// **もう止まらない**。判定の入力は `rate_limit_event` の status だけになった。
#[test]
fn headless_runner_no_longer_stops_on_error_records_with_limit_words() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    for body in [
        "{\"type\":\"result\",\"is_error\":true,\"result\":\"usage limit reached\"}\n",
        "{\"type\":\"error\",\"status\":429}\n",
        "{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"result\":\"overloaded\"}\n",
    ] {
        // **rc を写す**ことを測る（fake の rc を 0 以外にする＝「常に 0」と弁別できる形）。
        let claude = fake_claude(&dir, body, false, 3);
        let out = run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
            b"goal = \"x\"\n",
        );
        assert_eq!(
            out.status.code(),
            Some(3),
            "上限 record 以外は claude の rc を写す: {body}"
        );
    }
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
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let args = slurp(&dir.join("args"));
    assert!(pair(&args, "--setting-sources", ""), "settings は 1 つも読まない（ADR-0011 §2.1）: {args}");
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

/// runner が起こす claude は **口座の settings も対象 repo の settings も 1 つも読まない**
/// （ADR-0011 §2.1）。
///
/// `--setting-sources project` は「起動口座を継承しない」までしか塞げず、**対象 repo の
/// `.claude/settings.json` の allow 規則が残る**＝便ごとに凍結した allowlist（ADR-0010 §2.4）を
/// 実装対象の repo 側から広げられる（`.56` の lens L4）。空の値は user / project / local の
/// **どれも読まない**の意味である。
///
/// **値まで測る**のが要点で、`--setting-sources` が在ることだけを見る歯は値が `project` でも
/// 緑になる（契約が名指した禁止形）。
#[test]
fn headless_runner_loads_no_settings_from_account_or_checkout() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let vessel = write_vessel_copy(&dir, r#"["cargo"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let args = slurp(&dir.join("args"));
    assert!(pair(&args, "--setting-sources", ""), "空の値を **対**で渡す: {args}");
    // **どの源も名指されていない**（1 つでも残ると、その面の settings が読まれる）。
    for source in ["project", "user", "local"] {
        assert!(!pair(&args, "--setting-sources", source), "{source} の settings を読まない: {args}");
    }
    assert!(has_arg(&args, "--strict-mcp-config"), "MCP も宣言外を拾わない: {args}");
    // 別 seam で settings を戻す形（ADR-0011 §4 (a) / (b)）を渡していない。
    assert!(!has_arg(&args, "--settings"), "settings を file で渡し直さない: {args}");
    assert!(!has_arg(&args, "--restricted"), "restricted は使わない: {args}");
    clean(&[&dir, &worktree]);
}

/// lens が起こす claude も同じ形で起きる（構築点は `build` 1 つ・ADR-0011 §2.1）。
///
/// lens は `--allowedTools` を渡さない側だが、**settings 由来の allow は権限の口を開ける**ので
/// runner と同じ 5 点を lens でも測る（片方だけ塞ぐ変異を落とす）。
#[test]
fn headless_lens_loads_no_settings_from_account_or_checkout() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"ok\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, "4096", "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let args = slurp(&dir.join("args"));
    assert!(pair(&args, "--setting-sources", ""), "空の値を **対**で渡す: {args}");
    for source in ["project", "user", "local"] {
        assert!(!pair(&args, "--setting-sources", source), "{source} の settings を読まない: {args}");
    }
    assert!(has_arg(&args, "--strict-mcp-config"), "MCP も宣言外を拾わない: {args}");
    assert!(!has_arg(&args, "--settings"), "settings を file で渡し直さない: {args}");
    assert!(!has_arg(&args, "--restricted"), "restricted は使わない: {args}");
    clean(&[&dir]);
}

// 既に land した起動形（`build` 1 つ・ADR-0011 §2.1 / §2.2）へ後から足す**不在**の歯 2 本。
// 実装は 1 byte も触らないので base で緑になる＝逃がしは下の 1 行で明示し、非空虚性は
// 変異 2 本（`build` に `--mcp-config` を足す / lens の呼出側に `--allowedTools` を足す）で示す。
// flip-check: retroactive s2-07l.72

/// runner も lens も **`--mcp-config` を渡さない**（`.64` の lens LOW-4）。
///
/// 既存の歯は `--strict-mcp-config` の**存在**しか見ないので、宣言 file を足す変異
/// （`--mcp-config <file>`＝strict のまま server を 1 つ載せる形）が緑のまま通る。
/// `--strict-mcp-config` の存在は既存の歯が持つので、ここでは**不在だけ**を測る
/// （分離形と連結形の両方＝[`has_arg`]）。
#[test]
fn headless_runner_and_lens_pass_no_mcp_config_absent_from_argv() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let vessel = write_vessel_copy(&dir, r#"["cargo"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let runner_args = slurp(&dir.join("args"));
    assert!(!has_arg(&runner_args, "--mcp-config"), "runner は MCP の宣言 file を渡さない: {runner_args}");

    let lens_dir = tmp();
    let lens_claude = fake_claude(&lens_dir, "{\"verdict\":\"PASS\",\"evidence\":\"ok\"}\n", false, 0);
    let contract = contract_in(&lens_dir);
    let seen = run_lens(&contract, "4096", "plan", &lens_claude, b"--- a\n+++ b\n");
    assert_eq!(seen.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&seen));
    let lens_args = slurp(&lens_dir.join("args"));
    assert!(!has_arg(&lens_args, "--mcp-config"), "lens は MCP の宣言 file を渡さない: {lens_args}");
    clean(&[&dir, &worktree, &lens_dir]);
}

/// lens は **`--allowedTools` を渡さない**（ADR-0011 §2.2: lens には器の hook も allow も載らない）。
///
/// runner 側の「在る」は [`headless_runner_passes_allowed_tools_from_vessel_copy`] が持つので、
/// この歯は lens だけを見る。allow を lens の呼出側に足す変異はどの既存の歯にも当たらず、
/// 権限を持った review が静かに始まる。
#[test]
fn headless_lens_passes_no_allowed_tools_absent_from_argv() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"ok\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, "4096", "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let args = slurp(&dir.join("args"));
    assert!(!has_arg(&args, "--allowedTools"), "lens に allow は載らない（ADR-0011 §2.2）: {args}");
    clean(&[&dir]);
}

/// **id の乱数に上限の語が現れただけ**では止まらない（`s2-07l.71`）。
///
/// `.64` の実 run で実際に踏んだ形: 401 authentication_failed の result record は
/// `is_error` を名乗るので種別の絞りは通り、そのうえで claude が振った `session_id` の
/// 16 進に `429` が部分文字列として現れたため、**認証エラーが rc 75「上限」に化けた**
/// （呼出側は `Failed detail=rate-limit` と記帳する＝便の失敗原因が台帳に嘘で残る）。
///
/// **本文 field に上限の語が 1 つも無い**ことが要点で、id 側にだけ置く。
#[test]
fn headless_runner_does_not_stop_on_rate_limit_word_in_session_id() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    for body in [
        // 実 run で観測した形（口座名・API key・path は含まない・id は同形の別値）。
        concat!(
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,",
            "\"result\":\"Failed to authenticate. API Error: 401 API key is invalid.\",",
            "\"session_id\":\"791e7ee4-d00f-4dbc-8429-dd54dcbd20c4\",",
            "\"uuid\":\"c719aad3-e1db-42c9-a654-cae6bf758981\"}\n"
        ),
        // **本文 field を 1 つも持たない** error record は「上限ではない」へ倒す
        // （分からない周を上限と名乗らない＝上限側を狭く取る）。
        "{\"type\":\"error\",\"session_id\":\"00000000-0000-4000-8000-000000000529\"}\n",
    ] {
        // claude 自身の rc は 1（認証で落ちた）。**その rc が写ること**まで測る——
        // 「rc 75 でない」だけだと、包みが独自の rc を作る変異が生き残る。
        let claude = fake_claude(&dir, body, false, 1);
        let out = run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
            b"goal = \"x\"\n",
        );
        assert_ne!(
            out.status.code(),
            Some(i32::from(RC_RATE_LIMIT)),
            "id の乱数に現れた上限語は上限ではない: {body}"
        );
        assert_eq!(out.status.code(), Some(1), "claude の rc を写す: {body} / {}", stderr_of(&out));
    }
    clean(&[&dir, &worktree]);
}

/// **反転した歯**（`s2-07l.77`・ADR-0012 §2.2）: 本文 field に上限の語彙が在っても**もう止まらない**。
///
/// `s2-07l.71` / `.73` で「どこを走査するか」を 2 度狭めた形は、走査ごと撤去された。**狭める
/// 努力が無駄だったのではなく**、狭めても誤爆が残るという事実が「本物の合図を見ていない」ことの
/// 証拠になり、現物を採りに行く判断（`s2-07l.67` の契約）を正当化した。
#[test]
fn headless_runner_no_longer_stops_on_limit_words_in_body_fields() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    for body in [
        "{\"type\":\"result\",\"is_error\":true,\"result\":\"API Error: 429 rate limit\"}\n",
        "{\"type\":\"system\",\"subtype\":\"error\",\"message\":\"429 Too Many Requests\"}\n",
        "{\"type\":\"error\",\"text\":\"upstream returned 429\"}\n",
    ] {
        // **rc を写す**ことを測る（rc 0 だと「常に 0」を返す実装と弁別できない）。
        let claude = fake_claude(&dir, body, false, 3);
        let out = run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
            b"goal = \"x\"\n",
        );
        assert_eq!(out.status.code(), Some(3), "本文の語彙では止まらない（rc を写す）: {body}");
    }
    clean(&[&dir, &worktree]);
}

/// **tool の失敗出力に上限語彙が現れても上限ではない**（`s2-07l.73`）。
///
/// claude は tool の失敗を会話 record（`type=user`）の中の `tool_result` block として流し、その
/// block が `"is_error":true` を持つ。`.71` までは record 種別を見ずに字面で `is_error` を拾って
/// いたので、**cargo や grep の出力に `429` が混じるだけで便が「上限」で死ぬ**（planner と当席が
/// 独立に実 binary で再現・2026-09-11）。とくに実 run で cargo を撃たせる便では、失敗した
/// Bash tool の出力がそのまま「上限で止まった」と記帳される。
#[test]
fn headless_runner_does_not_stop_on_rate_limit_word_in_tool_result() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    for body in [
        // 会話 record が運ぶ tool 出力（上限語彙も is_error も**入れ子の中**に在る）。
        concat!(
            "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"tool_result\",",
            "\"content\":\"error: 429 tests failed; see log\",\"is_error\":true}]}}\n"
        ),
        // **種別を読めない record**（`type` が無い）は「上限ではない」へ倒す＝分からない周を
        // 上限と名乗らない（誤認すると失敗原因が台帳から消える）。
        "{\"is_error\":true,\"result\":\"429 rate limit\"}\n",
    ] {
        // claude の rc は 1。**その rc が写ること**まで測る（rc 75 でないだけでは足りない）。
        let claude = fake_claude(&dir, body, false, 1);
        let out = run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
            b"goal = \"x\"\n",
        );
        assert_ne!(
            out.status.code(),
            Some(i32::from(RC_RATE_LIMIT)),
            "tool 出力の上限語彙は上限ではない: {body}"
        );
        assert_eq!(out.status.code(), Some(1), "claude の rc を写す: {body} / {}", stderr_of(&out));
    }
    clean(&[&dir, &worktree]);
}

/// **反転した歯**（`s2-07l.77`・ADR-0012 §2.2）: 入れ子に終端種別の字面が在る record も、
/// 配列や escape を跨いだ先に種別が在る record も、**もう上限として扱わない**。
///
/// ★契約が挙げた「反転する 3 本」に本 1 本が漏れていた（`s2-07l.73` で**両向き**にした歯で、
/// rc 75 側の 2 形が撤去された走査〔深さ数え〕を測っていたため）。設計の変更に伴う反転であって
/// 歯を緩めたのではない。
#[test]
fn headless_runner_no_longer_stops_on_records_with_nested_type_markers() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    for body in [
        concat!(
            "{\"message\":{\"content\":[{\"type\":\"error\",\"content\":\"429 rate limit\"}],",
            "\"is_error\":true},\"type\":\"user\"}\n"
        ),
        "{\"content\":[\"first\",\"second\"],\"type\":\"error\",\"status\":429}\n",
        "{\"result\":\"said \\\"429 rate limit\\\" once\",\"type\":\"result\",\"is_error\":true}\n",
    ] {
        let claude = fake_claude(&dir, body, false, 1);
        let out = run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
            b"goal = \"x\"\n",
        );
        assert_eq!(out.status.code(), Some(1), "claude の rc を写す: {body}");
    }
    clean(&[&dir, &worktree]);
}

/// prompt は**便の写しの allowlist をそのまま運ぶ**（`s2-07l.67`）。
///
/// 実 run の実測（過去 4 run）では cargo 呼出 9〜12 件が**全件「承認要求」で止まり出力 0 件**
/// だった＝器は一度も cargo を回せていない。`--allowedTools` で allow は与えていたが、**prompt が
/// 「何を撃ってよいか」「1 command で撃つ」を伝えていなかった**ため、実装役が pipe や `&&` で
/// 繋いだ形を撃ち、allow の外として止まっていた。
///
/// **写しの 2 command が両方載ること**を測る（1 つだけ見る歯は、写しの一部が落ちても緑になる）。
#[test]
fn headless_runner_prompt_lists_allowed_commands() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let claude = fake_claude(&dir, "", false, 0);
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    // **2 つとも「1 行 1 command」で載る**（値の出所は写し・ADR-0010 §2.4）。行で測るのが要点で、
    // `contains("- cargo")` だけだと区切りを `, ` へ変える変異が緑のまま通る（lens 2026-09-11 で実測）。
    assert!(
        prompt.contains("- cargo\n- git"),
        "写しの 2 command が 1 行 1 command で並ぶ: {prompt}"
    );
    // 規律を**極性ごと**測る。字面 2 つだけを見る歯は「一覧は参考。他も試してよい」への反転を
    // 素通しする（lens 2026-09-11 で実測）＝**排他の語と、繋がない形の列挙**まで見る。
    assert!(prompt.contains("実行してよい command"), "allowlist の節が在る: {prompt}");
    assert!(
        prompt.contains("**だけ**を実行してよい"),
        "一覧が**排他**であること（参考ではない）を言う: {prompt}"
    );
    assert!(prompt.contains("1 command"), "1 command で撃つ規律を運ぶ: {prompt}");
    // 実 run で唯一 deny された形（`$(…)`）まで名指す。実測で落ちた形が prompt に無いと、
    // 次の run も同じところで止まる。
    // **命令の字面**で測る（`contains("$(")` だけだと、実測の引用に `$(` が残っている限り
    // 命令行を消しても緑になる＝lens 対応で足した assert 自身が空虚だった・実測で確認した）。
    assert!(
        prompt.contains("command 置換"),
        "静的解析できない形（command 置換）を名指して禁じる: {prompt}"
    );
    for form in ["$(", "&&", "|"] {
        assert!(
            prompt.contains(form),
            "繋がない・置換しない形として {form} を名指す: {prompt}"
        );
    }
    // **写しに無い command は現れない**（manifest の上限や repo の宣言を読んでいない）。
    assert!(!prompt.contains("npm"), "写しに無い command 名は載せない: {prompt}");
    // **狭めた写しに従う**（値が固定の字面でなく写し由来であることの対）。
    let narrow = write_vessel_copy(&dir, r#"["git"]"#);
    let again = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &narrow, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&again));
    let narrowed = slurp(&dir.join("stdin"));
    assert!(narrowed.contains("- git"), "狭めた写しの command は載る: {narrowed}");
    assert!(!narrowed.contains("- cargo"), "写しから消えた command は載らない: {narrowed}");
    clean(&[&dir, &worktree]);
}

/// 契約本文の中の `{allowed}` は**展開されない**（`s2-07l.67`・`{write_set}` と同じ極性）。
///
/// 契約は外から来る text なので、重ねて `replace` すると契約に 1 語書くだけで prompt の
/// allowlist 節へ触れられる（自分の権限一覧を自分で書き換えられる）。**3 対を 1 走査**で
/// 埋めることでその経路を塞ぐ。
#[test]
fn headless_runner_prompt_does_not_expand_allowed_placeholder_from_contract() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    // write-set 側にも穴の字面を置く（埋めた値を二度と走査しないことの対）。
    fs::write(&write_set, "src/lib.rs\n{allowed}\n").expect("write-set を書ける");
    let claude = fake_claude(&dir, "", false, 0);
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        // byte string は ASCII だけ（`b"..."` に非 ASCII は置けない）。
        b"goal = \"hole {allowed} stays here\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    // 契約本文と write-set の穴は**そのまま残る**。
    assert!(
        prompt.contains("hole {allowed} stays here"),
        "契約本文の穴は展開されない: {prompt}"
    );
    assert_eq!(prompt.matches("{allowed}").count(), 2, "残る穴は契約と write-set の 2 つだけ: {prompt}");
    // **展開は 1 度だけ**（穴が 2 つ余計に展開されていれば command 名の出現が増える）。
    assert_eq!(prompt.matches("- cargo").count(), 1, "allowlist の展開は 1 度だけ: {prompt}");
    clean(&[&dir, &worktree]);
}

/// **上限 record でも、集合に無い status では止めない**（`s2-07l.77`・ADR-0012 §2.1）。
///
/// 実測で採れた唯一の status は `allowed_warning`（許可されつつ警告）で、これは**止める側では
/// ない**。止める status の集合は**空**なので、器は当面 rc 75 を立てない——ADR の**決定**であって
/// 実装の手抜きではない（未採取の値を推測で足すと ADR §4 案 (A') へ戻る）。
#[test]
fn headless_runner_does_not_stop_on_rate_limit_event_with_an_unlisted_status() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    // 実 run で採取した現物と同じ形（uuid / session_id は別値）。
    let body = concat!(
        "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed_warning\",",
        "\"rateLimitType\":\"seven_day\",\"utilization\":0.97,\"isUsingOverage\":false},",
        "\"uuid\":\"11111111-2222-4333-8444-555555555555\"}\n"
    );
    let claude = fake_claude(&dir, body, false, 0);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_ne!(out.status.code(), Some(i32::from(RC_RATE_LIMIT)), "集合に無い status では止めない");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "claude の rc を写す: {}", stderr_of(&out));
    // **utilization 0.97 でも止めない**（閾値で止めない・ADR §2.3・憲法 C9.2）。
    clean(&[&dir, &worktree]);
}

/// 止める判断は**純関数**で両向きに測る（`s2-07l.77`・ADR-0012 §2.1）。
///
/// 集合が空である以上、production の経路は**止まる側を一度も通らない**。純関数にしないと
/// 「上限で止まる分岐」に歯が 1 本も当たらないので、ここでは**非空の集合**を渡して測る。
#[test]
fn headless_runner_stops_only_on_statuses_in_the_stop_set() {
    let stop = ["blocked", "rejected"];
    assert!(vessel::headless::runner::stops_on("blocked", &stop), "集合の値では止まる");
    assert!(vessel::headless::runner::stops_on("rejected", &stop), "集合の値は 1 つに限らない");
    assert!(!vessel::headless::runner::stops_on("allowed_warning", &stop), "集合に無い値では止まらない");
    // **未知も止めない**（未採取の値を上限へ倒さない＝誤って健全な便を殺さない）。
    assert!(!vessel::headless::runner::stops_on("some_new_status", &stop), "未知の status では止まらない");
    // 空の集合（現行）はどの status でも止めない。
    assert!(!vessel::headless::runner::stops_on("blocked", &[]), "空の集合では止まらない");
}

/// **観測した status は記録に残す**（`s2-07l.77`・ADR-0012 §2.1 末尾）。
///
/// 集合を**実測で育てる唯一の口**である。これが無いと「実測で採れた値だけを入れる」が運用で
/// 回らず、集合は永久に空のままになる。記録は**判定の入力ではない**ので、rc は変わらない。
#[test]
fn headless_runner_records_the_observed_rate_limit_status() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let body = "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed_warning\"}}\n";
    let claude = fake_claude(&dir, body, false, 0);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert!(
        stdout_of(&out).contains("rate-limit-status=allowed_warning"),
        "観測した status を記録面へ載せる: {}",
        stdout_of(&out)
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "記録は rc を変えない");
    // 上限 record が流れない周は記録も出ない（無いものを名乗らない）。
    let quiet = fake_claude(&dir, "{\"type\":\"result\",\"is_error\":false}\n", false, 0);
    let again = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &quiet, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert!(!stdout_of(&again).contains("rate-limit-status="), "観測していない周は書かない: {}", stdout_of(&again));
    clean(&[&dir, &worktree]);
}

/// **`s2-07l.76` の症状が消えたことの裏取り**（`s2-07l.77` の契約 (7)）。
///
/// hook の失敗を伝える `system` record の文面に上限語が混じっても、もう rc 75 にならない。
/// **code の不在を字面で数えず、消えた結果の挙動で測る**。
#[test]
fn headless_runner_no_longer_stops_on_system_record_with_limit_words() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let body = "{\"type\":\"system\",\"subtype\":\"error\",\"message\":\"hook failed: 429 Too Many Requests\"}\n";
    let claude = fake_claude(&dir, body, false, 1);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(1), "claude の rc を写す: {}", stderr_of(&out));
    clean(&[&dir, &worktree]);
}

/// **`s2-07l.74` の症状が消えたことの裏取り**（`s2-07l.77` の契約 (7)）。
///
/// 未閉じの入れ子を大量に持つ病的な 1 行でも、上限 record でなければ**中身を見ない**。
/// **時間は測らない**（環境差で揺れる）——走査が無くなったことは「その record を検査しない」
/// という挙動で表れる。
#[test]
fn headless_runner_no_longer_scans_pathological_records() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    // 未閉じの入れ子 20000 個（旧実装はここで二次の走査に入った）。上限語も混ぜる。
    let pathological = format!(
        "{{\"type\":\"error\",\"is_error\":true,{}\"result\":\"429 rate limit\"\n",
        "\"error\":[".repeat(20000)
    );
    let claude = fake_claude(&dir, &pathological, false, 1);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(1), "上限 record でない行は中身を見ない: {}", stderr_of(&out));
    clean(&[&dir, &worktree]);
}

/// **record の形をしていない行は読まない**（`s2-07l.77`・変異で生存した分岐に歯を当てる）。
///
/// 判定の入力は「claude が出した 1 record」であって「上限 record に言及した文字列」ではない。
/// 行頭が `{` でない行（log の前置きが付いた行・本文が record を引用した行）は、marker を
/// 含んでいても **status を読まない**——これが崩れると、tool の出力が上限 record を引用した
/// 周に器が反応する（集合が空の現在は止まらないが、値を入れた周に誤停止へ育つ）。
#[test]
fn headless_runner_does_not_read_a_rate_limit_record_from_plain_text() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    // 行頭が `{` でない（前置きが付いた）が marker と status を含む行。
    let body = concat!(
        "log: {\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed_warning\"}}\n",
        "{\"type\":\"result\",\"is_error\":false}\n"
    );
    let claude = fake_claude(&dir, body, false, 0);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert!(
        !stdout_of(&out).contains("rate-limit-status="),
        "record の形をしていない行から status を読まない: {}",
        stdout_of(&out)
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc は claude のもの");
    clean(&[&dir, &worktree]);
}

/// **止める側の分岐**を測る（`s2-07l.77`・lens 2026-09-11 H1）。
///
/// 集合が空である以上 production は `Stop` を通らないので、**判定を 3 値の純関数へ切り出して**
/// 非空の集合で測る。これが無いと「上限で止める」側は 1 本も測られない。
#[test]
fn headless_runner_decides_stop_only_for_statuses_in_the_set() {
    use vessel::headless::runner::{decide, stop_line, Decision};
    let event = "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"blocked\"}}";
    // 集合に在る status → **止める**。
    assert_eq!(decide(event, &["blocked"]), Decision::Stop("blocked".to_owned()));
    // 集合に無い status → 記録だけ（止めない）。
    assert_eq!(decide(event, &["exceeded"]), Decision::Observed("blocked".to_owned()));
    // 現行（空の集合）→ 記録だけ。
    assert_eq!(decide(event, &[]), Decision::Observed("blocked".to_owned()));
    // 上限 record でない行 → 何もしない。
    assert_eq!(decide("{\"type\":\"result\",\"is_error\":true}", &["blocked"]), Decision::Ignore);
    // 止めた周の 1 行は **status を載せる**（後から「何で止まったか」を読めるように）。
    assert!(stop_line("blocked").contains("rate-limit-status=blocked"), "止めた理由を記録面と同じ形で載せる");
}

/// key と colon の間の**空白に寛容**である（`s2-07l.77`・lens 2026-09-11 H2）。
///
/// 実 stream は compact だが（実測）、表記が変わっただけで**記録の口が無音で止まる**形にはしない
/// ——ADR §2.1 の「観測した status を残す」は、読めなければ一度も発火しない。
#[test]
fn headless_runner_reads_the_status_with_spaces_around_colons() {
    use vessel::headless::runner::rate_limit_status;
    let spaced = "{\"type\": \"rate_limit_event\", \"rate_limit_info\": {\"status\": \"allowed_warning\"}}";
    assert_eq!(rate_limit_status(spaced), Some("allowed_warning"), "空白入りでも読む");
    let compact = "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed_warning\"}}";
    assert_eq!(rate_limit_status(compact), Some("allowed_warning"), "compact も読む（実 stream の形）");
}

/// status は **`rate_limit_info` の直下**だけを読む（`s2-07l.77`・lens 2026-09-11 H3）。
///
/// `rate_limit_info` は `unifiedWindows` のような入れ子を持つ。最初に見つけた `"status"` を採る形は
/// **key の並び次第で別の object の値を読む**——記録の口は集合を育てる唯一の入力なので、ここが
/// 汚れると「実測で採れた値だけを入れる」が入口で崩れる。
#[test]
fn headless_runner_reads_the_status_only_from_the_immediate_object() {
    use vessel::headless::runner::rate_limit_status;
    // 入れ子が先に来て、その中に status が在る形（直下には無い）。
    let nested_first = concat!(
        "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{",
        "\"unifiedWindows\":{\"five_hour\":{\"status\":\"blocked\"}},\"utilization\":0.1}}"
    );
    assert_eq!(rate_limit_status(nested_first), None, "入れ子の status は読まない");
    // 直下に在る形（現物と同じ並び）。
    let direct = concat!(
        "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed_warning\",",
        "\"unifiedWindows\":{\"five_hour\":{\"status\":\"blocked\"}}}}"
    );
    assert_eq!(rate_limit_status(direct), Some("allowed_warning"), "直下の status を読む");
}

/// 判定は **`rate_limit_event` 種別に限る**（`s2-07l.77`・ADR-0012 §2.1 の MUST）。
///
/// 種別を見ない実装は、`rate_limit_info` の形さえ持てば**別の record を上限として読む**。
/// 変異（種別の照合を常に真にする）が生存したので歯を当てた（lens 2026-09-11 M1 / 変異 2 周目）。
#[test]
fn headless_runner_reads_the_status_only_from_the_dedicated_record_kind() {
    use vessel::headless::runner::rate_limit_status;
    // 上限 record と同じ形の field を持つが、**種別が違う** record。
    let impostor = "{\"type\":\"result\",\"rate_limit_info\":{\"status\":\"blocked\"}}";
    assert_eq!(rate_limit_status(impostor), None, "種別が違えば読まない");
    // 種別 field 自体が無い record も読まない。
    let typeless = "{\"rate_limit_info\":{\"status\":\"blocked\"}}";
    assert_eq!(rate_limit_status(typeless), None, "種別が無ければ読まない");
    // 対照: 種別が合っていれば読む。
    let real = "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"blocked\"}}";
    assert_eq!(rate_limit_status(real), Some("blocked"), "種別が合えば読む");
}

/// runner が組んだ prompt を**便の作業面へ残す**（`s2-07l.79`・設計 §6）。
///
/// prompt は stdin で渡すので stream には 1 行も出ない（`.67` の限界）。「何を渡したか」を
/// 後から読める唯一の口がこの file で、置き場は **vessel の写しの隣**（= run dir・
/// `<state_dir>/pipe/<run>/`）である。新しい flag も env も足さない（C2.2）。
///
/// 測るのは 3 つ: (i) file の中身が claude の stdin に届いた prompt と**同一**（要約や
/// 別の文面ではない）(ii) claude が起きる**前**に落ちている（起きた後に書く形だと、席が
/// 止まらない周の prompt が読めない）(iii) tracked な面（worktree）へは 1 byte も置かない。
#[test]
fn headless_runner_writes_the_prompt_beside_the_vessel_copy() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let saved = dir.join("prompt.txt");
    // fake は**起動された時点**の prompt file を写す（(ii) を rc でなく痕跡で測る）。
    let script = slurp(&claude).replace(
        "cat > \"",
        &format!("cp \"{}\" \"{}\" 2>/dev/null\ncat > \"", saved.display(), dir.join("prompt-at-call").display()),
    );
    fs::write(&claude, script).expect("fake を書き換えられる");
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        "goal = \"prompt を残す\"\n".as_bytes(),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let sent = slurp(&dir.join("stdin"));
    assert!(sent.contains("goal = \"prompt を残す\""), "fake は prompt を受けている: {sent}");
    assert_eq!(slurp(&saved), sent, "残した prompt は claude へ渡したものと同一");
    assert_eq!(slurp(&dir.join("prompt-at-call")), sent, "claude が起きる前に落ちている");
    assert!(!worktree.join("prompt.txt").exists(), "worktree（tracked 面）には置かない");
    assert!(!stderr_of(&out).contains("prompt を残せない"), "残せた周は欠落の行を出さない: {}", stderr_of(&out));
    clean(&[&dir, &worktree]);
}

/// 残せない周でも便は止めない（極性: 証跡は判定の入力ではない・`s2-07l.79`）。
///
/// 置き場を **dir で塞ぐ**（`prompt.txt` が dir だと write は EISDIR）。claude は起き、
/// rc は claude のものがそのまま写る（0 と 3 の両方で測る＝「たまたま 0」と区別する）。
/// 黙って落とすのではなく stderr へ 1 行残す——証跡の欠落を人が後から読めるように。
#[test]
fn headless_runner_keeps_going_when_the_prompt_cannot_be_saved() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    fs::create_dir_all(dir.join("prompt.txt")).expect("置き場を dir で塞げる");
    for want in [0_u8, 3_u8] {
        let claude = fake_claude(&dir, "", false, want);
        fs::remove_file(dir.join("called")).ok();
        let out = run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
            "goal = \"残せなくても進む\"\n".as_bytes(),
        );
        assert_eq!(out.status.code(), Some(i32::from(want)), "rc は claude のまま: {}", stderr_of(&out));
        assert!(dir.join("called").exists(), "claude は起きる");
        assert!(stdout_of(&out).contains(&format!("runner: rc={want} records=")), "記録行は変わらない");
        assert!(stderr_of(&out).contains("prompt を残せない"), "欠落は stderr に 1 行: {}", stderr_of(&out));
    }
    clean(&[&dir, &worktree]);
}

/// 置き場が解けない写し（裸の `vessel.toml`）では prompt を **cwd へ落とさない**（lens 2026-09-11 M2）。
///
/// `Path::parent` は裸の名に `Some("")` を返すので、素朴に join すると prompt が runner の cwd
/// ＝ pipeline では便の worktree（tracked 面）へ落ちる。「残さない」側へ倒し、便は止めない
/// （rc は claude のまま・stderr に欠落の 1 行）。cwd を tmp に固定して測る＝退行しても repo を汚さない。
#[test]
fn headless_runner_does_not_drop_the_prompt_into_the_cwd_for_a_relative_vessel() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let _ = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let mut child = Command::new(bin())
        .args(["runner", "--worktree"])
        .arg(&worktree)
        .arg("--write-set")
        .arg(&write_set)
        .args(["--vessel", "vessel.toml", "--plugin-dir"])
        .arg(&dir)
        .args(["--permission-mode", "acceptEdits", "--claude"])
        .arg(&claude)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary を起動できる");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all("goal = \"相対の写し\"\n".as_bytes());
    }
    let out = child.wait_with_output().expect("binary の出力を読める");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "便は止めない: {}", stderr_of(&out));
    assert!(dir.join("called").exists(), "claude は起きる");
    assert!(!dir.join("prompt.txt").exists(), "cwd（相対 vessel の隣）へは落とさない");
    assert!(stderr_of(&out).contains("prompt を残せない"), "欠落は stderr に 1 行: {}", stderr_of(&out));
    clean(&[&dir, &worktree]);
}

// ── 質問の口 (b) 包み（設計 docs/design/pipeline-question.md §3 / §8 (b)・SRS FR31） ──────

/// runner が最終行に書く質問 record（写す用の字面そのもの）。
const QUESTION_RECORD: &str = r#"{"question":"verify 行が矛盾する","about":"verify"}"#;

/// fake claude の stream（system 1 行 + 最終 result 1 行・`result` の text は escape 済み）。
fn stream_with_result(result_json_text: &str) -> String {
    format!(
        "{{\"type\":\"system\",\"subtype\":\"init\"}}\n{{\"type\":\"result\",\"is_error\":false,\"result\":\"{result_json_text}\",\"usage\":{{\"input_tokens\":1}}}}\n"
    )
}

/// runner を 1 回撃つ（契約は stdin・fake claude の rc を指定）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_question_runner(dir: &Path, worktree: &Path, body: &str, rc: u8, contract: &[u8]) -> Output {
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let vessel = write_vessel_copy(dir, r#"["cargo", "git"]"#);
    let claude = fake_claude(dir, body, false, rc);
    run_runner(
        &RunnerCall { dir, worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        contract,
    )
}

/// stdout に `{` で始まる行が無い（record を写していない）。
fn has_no_record_line(out: &Output) -> bool {
    !stdout_of(out).lines().any(|line| line.trim_start().starts_with('{'))
}

#[test]
fn runner_question_record_on_last_line_yields_rc76_and_echoes_it() {
    let dir = tmp();
    let worktree = tmp();
    // result の text = 本文 1 行 + 最終行の record（JSON 文字列の中なので `"` と改行は escape）。
    let text = r#"契約を読んだ。\n{\"question\":\"verify 行が矛盾する\",\"about\":\"verify\"}"#;
    let out = run_question_runner(&dir, &worktree, &stream_with_result(text), 0, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(vessel::pipe::RC_QUESTION)), "包みは rc 76 で終える: {}", stderr_of(&out));
    let text = stdout_of(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.last().copied(), Some(QUESTION_RECORD), "最終行は同じ record そのもの: {lines:?}");
    assert!(
        lines.iter().rev().nth(1).is_some_and(|line| line.starts_with("runner: rc=0 records=")),
        "観測行は record の前に残る: {lines:?}"
    );
    clean(&[&dir, &worktree]);
}

#[test]
fn runner_question_mirrors_claude_rc_when_record_is_absent_or_unreadable() {
    let dir = tmp();
    let worktree = tmp();
    // key 無し / 壊れた JSON / 入れ子で引用された record / question が空、の 4 形（+ 記録の無い普通の終わり）。
    // `malformed` = JSON らしい最終行が読めない形（FailOpen を**隠さない**＝stderr に理由 1 行）。
    // `Plain`（key 無し・record 無し）は普通の終わり方なので stderr を出さない。
    for (label, text, malformed) in [
        ("key 無し", r#"done\n{\"about\":\"verify\"}"#, false),
        ("壊れた JSON", r#"done\n{\"question\":\"verify"#, true),
        ("入れ子で引用", r#"done\n{\"outer\":{\"question\":\"verify\"}}"#, true),
        ("question が空", r#"done\n{\"question\":\"  \",\"about\":\"verify\"}"#, true),
        ("record 無し", r#"done. see {\"question\":\"x\"} above"#, false),
    ] {
        let out = run_question_runner(&dir, &worktree, &stream_with_result(text), 0, b"goal = \"x\"\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{label}: claude の rc（0）を写す: {}", stderr_of(&out));
        assert!(has_no_record_line(&out), "{label}: stdout に record を写さない: {}", stdout_of(&out));
        assert!(stdout_of(&out).contains("runner: rc=0 records="), "{label}: 観測行は残る");
        assert_eq!(
            stderr_of(&out).contains("質問 record の形でない"),
            malformed,
            "{label}: 読めない形だけ stderr に理由 1 行（FailOpen を隠さない）: {}",
            stderr_of(&out)
        );
    }
    clean(&[&dir, &worktree]);
}

#[test]
fn runner_question_is_judged_only_after_normal_exit() {
    let dir = tmp();
    let worktree = tmp();
    let text = r#"failed\n{\"question\":\"verify 行が矛盾する\",\"about\":\"verify\"}"#;
    for want in [1_u8, 3] {
        let out = run_question_runner(&dir, &worktree, &stream_with_result(text), want, b"goal = \"x\"\n");
        assert_eq!(out.status.code(), Some(i32::from(want)), "非 0 の周は claude の rc を写す（76 にしない）");
        assert!(has_no_record_line(&out), "非 0 の周は最終行を読まない: {}", stdout_of(&out));
    }
    clean(&[&dir, &worktree]);
}

#[test]
fn runner_question_prompt_carries_record_rule_and_answer_section() {
    let dir = tmp();
    let worktree = tmp();
    // pipeline が stdin へ流す形: 契約の写し + 末尾の「## 回答」節（redirect ではなく piped stdin）。
    let contract = "goal = \"x\"\n\n## 回答\n- 質問: verify 行が矛盾する\n- 回答: verify は 1 行目だけを撃つ\n";
    let out = run_question_runner(&dir, &worktree, "", 0, contract.as_bytes());
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    assert!(prompt.contains("{\"question\":\"<"), "質問は最終行の record で、と命じる: {prompt}");
    assert!(prompt.contains("commit を作らない"), "質問の周は commit を作らない: {prompt}");
    assert!(prompt.contains("それ以外の形で人へ問わない"), "record 以外の対話を禁じたまま: {prompt}");
    assert!(!prompt.contains("対話しない。人へ質問を返さず"), "旧い文言（質問を返さず）は消える: {prompt}");
    assert!(prompt.contains("## 回答」節"), "回答節の読み方を運ぶ: {prompt}");
    assert!(
        prompt.contains("- 質問: verify 行が矛盾する") && prompt.contains("- 回答: verify は 1 行目だけを撃つ"),
        "stdin の回答節が prompt に**そのまま**載る（stdin だけを読む）: {prompt}"
    );
    clean(&[&dir, &worktree]);
}

/// 判定の純関数の**受理側**（[`vessel::headless::runner::question_ending`]）。
#[test]
fn runner_question_ending_accepts_records_and_plain_text() {
    use vessel::headless::runner::{question_ending, Ending};
    assert_eq!(question_ending(&format!("done\n{QUESTION_RECORD}")), Ending::Question(QUESTION_RECORD.to_owned()));
    assert_eq!(question_ending(&format!("  {QUESTION_RECORD}  \n")), Ending::Question(QUESTION_RECORD.to_owned()), "前後の空白は剥がす");
    assert_eq!(question_ending("done\n{\"question\":\"only\"}"), Ending::Question("{\"question\":\"only\"}".to_owned()), "about は任意");
    // 最終の `{` 行だけを見る（前の行の record は読まない・後ろの散文は無視）。
    assert_eq!(question_ending(&format!("{QUESTION_RECORD}\nfollow-up text")), Ending::Question(QUESTION_RECORD.to_owned()), "最後の `{{` 行");
    assert_eq!(question_ending("just text"), Ending::Plain);
    assert_eq!(question_ending(""), Ending::Plain);
    assert_eq!(question_ending("{\"about\":\"verify\"}"), Ending::Plain, "question key 無しは record ではない");
}

/// 判定の純関数の**拒否側**（理由の字面まで見る・FailOpen を隠さない）。
#[test]
fn runner_question_ending_rejects_malformed_records_with_reasons() {
    use vessel::headless::runner::{question_ending, Ending};
    let reason_of = |text: &str| match question_ending(text) {
        Ending::Malformed(reason) => reason,
        other => format!("not malformed: {other:?}"),
    };
    assert!(reason_of("{\"question\":\"x").contains("読めない"), "壊れた JSON");
    assert!(reason_of("{\"outer\":{\"question\":\"x\"}}").contains("読めない"), "入れ子は flat parser が断る");
    assert!(reason_of("{\"question\":\"   \"}").contains("空"), "空の question");
    assert!(reason_of("{\"question\":7}").contains("文字列"), "非文字列");
    assert!(reason_of("{\"question\":\"a\\nb\"}").contains("1 行"), "複数行");
}

/// `result` record の text を escape を解いて読む（[`vessel::headless::runner::result_text`]）。
#[test]
fn runner_question_result_text_decodes_escapes_only_from_result_records() {
    use vessel::headless::runner::result_text;
    let line = r#"{"type":"result","is_error":false,"result":"a\n\"q\" \\ é 😀","usage":{"x":1}}"#;
    assert_eq!(result_text(line).as_deref(), Some("a\n\"q\" \\ é 😀"));
    assert_eq!(result_text(r#"{"type":"assistant","result":"x"}"#), None, "種別が違う行は読まない");
    assert_eq!(result_text(r#"{"type":"result","is_error":true}"#), None, "result field が無い");
    assert_eq!(result_text("not json"), None);
    assert_eq!(result_text(r#"{"result":"first","type":"result"}"#).as_deref(), Some("first"), "key の並びに依らない");
}
