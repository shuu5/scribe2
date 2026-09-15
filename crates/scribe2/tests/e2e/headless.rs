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
/// 起動されたら `called` を残し、引数を `args`・cwd を `cwd`・口座 env を `account`・agent view の
/// env を `agent-view` へ写してから `body` を stdout へ出す。
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
         printf '%s' \"$CLAUDE_CODE_DISABLE_AGENT_VIEW\" > \"{d}/agent-view\"\n\
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

/// binary へ渡す親の agent view の env の値（`1` でも空でもない字面）。
///
/// test を撃つ環境が既に `1` を持っていると、子が**継承しただけ**の周も「切れている」に見える（空虚な歯）。
/// 親の値を別の字面に固定し、子の写しが `1` なら器が**設定した**と読める形にする。
const INHERITED_AGENT_VIEW: &str = "inherited-from-parent";

/// binary を 1 回撃つ。stdin には `input` を流す（親の agent view の env は [`INHERITED_AGENT_VIEW`]）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_bin(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(bin())
        .args(args)
        .env("CLAUDE_CODE_DISABLE_AGENT_VIEW", INHERITED_AGENT_VIEW)
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

/// runner / lens の `--rules` に渡す最小の manifest（`rows` の各要素は id / kind / value / enabled の 4 key の
/// `[[rule]]` 1 行）を `name` で書く。
///
/// runner / lens が読むのは `gate.token_cap`（lens）と `runner.model`（両方）だけなので、pipe の歯が使う受付の
/// 10 行は載せない（あちらの helper は `pipe.rs` の private で、こちらへ写すと変更の理由が 2 つの file に割れる）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rules_with_rows(dir: &Path, name: &str, rows: &[String]) -> PathBuf {
    let body: String = rows
        .iter()
        .map(|row| format!("\n[[rule]]\n{row}ruling = \"t\"\nruled_at = \"d\"\n"))
        .collect();
    let path = dir.join(name);
    fs::write(&path, format!("schema = 1\n{body}")).expect("tmp manifest を書ける");
    path
}

/// [`rules_with_rows`] の 1 行の形。
fn rules_with_row(dir: &Path, name: &str, row: &str) -> PathBuf {
    rules_with_rows(dir, name, &[row.to_owned()])
}

/// `gate.token_cap` の行（`cap` byte・発効）。
fn cap_row(cap: u64) -> String {
    format!("id = \"gate.token_cap\"\nkind = \"GateTokenCap\"\nvalue = {cap}\nenabled = true\n")
}

/// `runner.model` の行（値は文字列 `value`・発効）。
fn model_row(value: &str) -> String {
    format!("id = \"runner.model\"\nkind = \"RunnerModel\"\nvalue = \"{value}\"\nenabled = true\n")
}

/// 埋め込み manifest と同じ `runner.model` の値（claude CLI の別名・裁定 id `user 2026-09-14T21:59Z`）。
const RUNNER_MODEL: &str = "opus";

/// `gate.token_cap` を `cap` byte にした manifest（file 名に値を含む＝同じ dir で cap を変えて撃ち直せる）。
/// `runner.model` は埋め込みと同じ値で載せる（lens は cap と model の 2 行を同じ manifest から読む）。
fn rules_with_cap(dir: &Path, cap: u64) -> PathBuf {
    rules_with_rows(dir, &format!("rules-cap-{cap}.toml"), &[cap_row(cap), model_row(RUNNER_MODEL)])
}

/// lens を 1 回撃つ（引数の並びが複数の歯で同じなので畳む）。cap は **`--rules` の manifest** で
/// 渡す（`s2-07l.272`・argv の `--cap` は撤去した＝値の出所は rules 行 `gate.token_cap` 1 つ）。
///
/// `--worktree` は必須だが、この helper を使う歯の関心は cwd ではないので**契約の
/// 置き場**を渡す（引数を 1 本増やすと粒度 lint の上限に当たる）。cwd がその worktree
/// であることは [`headless_lens_runs_claude_in_the_given_worktree`] が**別 dir**で測る。
fn run_lens(contract: &Path, cap: u64, mode: &str, claude: &Path, diff: &[u8]) -> Output {
    let dir = contract.parent().unwrap_or(Path::new("."));
    let rules = rules_with_cap(dir, cap);
    run_bin(
        &[
            "lens",
            "--contract", &contract.display().to_string(),
            "--worktree", &dir.display().to_string(),
            "--rules", &rules.display().to_string(),
            "--permission-mode", mode,
            "--claude", &claude.display().to_string(),
        ],
        diff,
    )
}

/// runner を 1 回撃つための材料（引数の並びが複数の歯で同じなので畳む）。
struct RunnerCall<'a> {
    /// plugin root 兼 fake の置き場（配下に [`PLUGIN_LEAF`] の dir を 1 つ置いて渡す）。
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

/// 既存の runner の歯が root（[`RunnerCall::dir`]）の配下に置く plugin の dir 名。
///
/// runner は root の配下の dir を 1 つずつ `--plugin-dir` に渡し、配下 0 の root では claude を
/// 起こさない（設計 §6・`s2-07l.149`）ので、root には dir が 1 つ要る。root 直下の file
/// （fake・args・body …）は plugin と見られない。
const PLUGIN_LEAF: &str = "vessel-plugin";

/// root の配下に [`PLUGIN_LEAF`] の dir を置き、その path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn plugin_leaf(root: &Path) -> PathBuf {
    let leaf = root.join(PLUGIN_LEAF);
    fs::create_dir_all(&leaf).expect("plugin の dir を作れる");
    leaf
}

/// runner を 1 回撃つ（plugin root = [`RunnerCall::dir`]・配下に [`PLUGIN_LEAF`] を置く）。
fn run_runner(call: &RunnerCall<'_>, input: &[u8]) -> Output {
    plugin_leaf(call.dir);
    run_runner_in(call, call.dir, input)
}

/// runner を 1 回撃つ（plugin root を名指す形・root の中身は呼び手が作る）。
fn run_runner_in(call: &RunnerCall<'_>, root: &Path, input: &[u8]) -> Output {
    let args = runner_args(call, root);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_bin(&refs, input)
}

/// runner を `--rules` の manifest つきで 1 回撃つ（plugin root = [`RunnerCall::dir`]・model の行の歯）。
fn run_runner_with_rules(call: &RunnerCall<'_>, rules: &Path, input: &[u8]) -> Output {
    plugin_leaf(call.dir);
    let mut args = runner_args(call, call.dir);
    args.extend(["--rules".to_owned(), rules.display().to_string()]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_bin(&refs, input)
}

/// runner の argv（plugin root を名指す形）。
fn runner_args(call: &RunnerCall<'_>, root: &Path) -> Vec<String> {
    let mut args = vec![
        "runner".to_owned(),
        "--worktree".to_owned(),
        call.worktree.display().to_string(),
        "--write-set".to_owned(),
        call.write_set.display().to_string(),
        "--vessel".to_owned(),
        call.vessel.display().to_string(),
        "--plugin-dir".to_owned(),
        root.display().to_string(),
        "--permission-mode".to_owned(),
        call.mode.to_owned(),
        "--claude".to_owned(),
        call.claude.display().to_string(),
    ];
    if let Some(found) = call.account {
        args.push("--account-dir".to_owned());
        args.push(found.display().to_string());
    }
    args
}

/// runner が claude へ渡した引数と prompt を測る（歯 1 が mode ごとに 2 度使う）。
fn assert_runner_call(dir: &Path, worktree: &Path, account: &Path, mode: &str) {
    let args = slurp(&dir.join("args"));
    let lines: Vec<&str> = args.lines().collect();
    assert!(pair(&args, "--permission-mode", mode), "permission mode を毎回明示する: {args}");
    assert!(pair(&args, "--model", RUNNER_MODEL), "model も毎回明示する（`--rules` 無しは埋め込みの行）: {args}");
    assert!(pair(&args, "--output-format", "stream-json"), "stream-json で回す: {args}");
    assert!(lines.contains(&"--verbose"), "stream-json には --verbose が要る: {args}");
    // root（`dir`）そのものではなく、配下の dir が 1 本だけ渡る（root 直下の file は plugin でない）。
    assert_eq!(
        plugin_dir_values(&args),
        vec![dir.join(PLUGIN_LEAF).display().to_string()],
        "plugin root の配下の dir を載せる: {args}"
    );
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
    let out = run_lens(&contract, 16, "plan", &claude, &diff);
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
    let out = run_lens(&contract, 150_000, "acceptEdits", &verdict, &big);
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
    let out = run_lens(&edge_contract, 64, "plan", &at_cap, &vec![b'y'; 64]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(edge.join("called").exists(), "境界ちょうどでは claude を呼ぶ");
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"FAIL","evidence":"境界は内側"}"#,
        "境界ちょうどは判定を返す"
    );
    clean(&[&edge, &dir]);
}

/// lens の引数の並び（`--rules` と余分の 1 対を差し込める形・`--claude` は末尾）。
fn lens_args(contract: &Path, worktree: &Path, extra: &[&str], claude: &Path) -> Vec<String> {
    let mut args = vec![
        "lens".to_owned(),
        "--contract".to_owned(),
        contract.display().to_string(),
        "--worktree".to_owned(),
        worktree.display().to_string(),
        "--permission-mode".to_owned(),
        "plan".to_owned(),
    ];
    args.extend(extra.iter().map(|item| (*item).to_owned()));
    args.extend(["--claude".to_owned(), claude.display().to_string()]);
    args
}

/// [`run_bin`] を `Vec<String>` の引数で撃つ。
fn run_bin_owned(args: &[String], input: &[u8]) -> Output {
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_bin(&borrowed, input)
}

/// cap は **rules 行 `gate.token_cap` からだけ**読む（`s2-07l.272`・憲法 C1・FR17）。`--cap` を渡さず
/// `--rules` の manifest の値だけで INCONCLUSIVE / 呼出が切り替わる。base は `--cap` が無いと usage の
/// rc 1 で断るので RED。
#[test]
fn headless_lens_reads_cap_from_rules_row() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"manifest の cap の内側\"}\n", false, 0);
    let contract = contract_in(&dir);
    let diff = vec![b'z'; 11];
    // cap 10 byte・diff 11 byte → 超過。claude を呼ばず INCONCLUSIVE。
    let small = rules_with_cap(&dir, 10);
    let out = run_bin_owned(&lens_args(&contract, &dir, &["--rules", &small.display().to_string()], &claude), &diff);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"INCONCLUSIVE","evidence":"diff exceeds cap"}"#,
        "manifest の cap を超えた周は INCONCLUSIVE"
    );
    assert!(!dir.join("called").exists(), "cap を超えたので claude を 1 度も起動しない");
    // **同じ diff・manifest の値だけ 100 byte へ** → 内側。claude が 1 回呼ばれる＝値は manifest から来ている。
    let wide = rules_with_cap(&dir, 100);
    let out = run_bin_owned(&lens_args(&contract, &dir, &["--rules", &wide.display().to_string()], &claude), &diff);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "manifest の cap の内側なので claude を呼ぶ");
    assert_eq!(
        stdout_of(&out).trim(),
        r#"{"verdict":"PASS","evidence":"manifest の cap の内側"}"#,
        "判定は claude の最後の JSON 行"
    );
    let args = slurp(&dir.join("args"));
    assert!(!has_arg(&args, "--cap"), "cap は claude へ渡らない: {args}");
    // **`--rules` が無い周は埋め込みの manifest**（`pipe::cli` と同じ規約）。埋め込みの cap は 11 byte より
    // 大きいので claude を呼ぶ＝「`--rules` 無しは cap 0」へ倒す変異を落とす。
    fs::remove_file(dir.join("called")).expect("前の周の印を消せる");
    let out = run_bin_owned(&lens_args(&contract, &dir, &[], &claude), &diff);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "埋め込みの cap の内側なので claude を呼ぶ");
    clean(&[&dir]);
}

/// 撤去した `--cap` は**未知の引数として断る**（rc 1・usage・claude 未起動）。黙って読み飛ばすと、手書きの
/// 数が残った launcher が効いているように見える（`.265` の drift の再発経路）。base は受理するので RED。
#[test]
fn headless_lens_refuses_cap_flag() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let contract = contract_in(&dir);
    let rules = rules_with_cap(&dir, 4096);
    let out = run_bin_owned(
        &lens_args(&contract, &dir, &["--rules", &rules.display().to_string(), "--cap", "1"], &claude),
        b"--- a\n+++ b\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "--cap は未知の引数: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    let err = stderr_of(&out);
    assert!(err.contains("--cap"), "断った引数を名指す: {err}");
    assert!(err.contains("usage: "), "usage を出す: {err}");
    assert!(!err.contains("--cap BYTES"), "usage に --cap は載らない: {err}");
    assert!(err.contains("[--rules PATH]"), "usage は --rules を載せる: {err}");
    clean(&[&dir]);
}

/// cap の行が解けない周は **claude を呼ばず rc 2** で理由を 1 行（`lens: gate.token_cap …`・pipe の `int_row`
/// と同じ 3 理由 + manifest 自体が読めない周）。上限なしで走らせない（C6）。
#[test]
fn headless_lens_refuses_unreadable_cap_row() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let contract = contract_in(&dir);
    let absent = rules_with_row(
        &dir,
        "absent.toml",
        "id = \"gate.lens_count\"\nkind = \"GateLensCount\"\nvalue = 1\nenabled = true\n",
    );
    let disabled = rules_with_row(
        &dir,
        "disabled.toml",
        "id = \"gate.token_cap\"\nkind = \"GateTokenCap\"\nvalue = 4096\nenabled = false\n",
    );
    // id は同じで kind が散文の行（manifest は id と kind の対応を照合しない）＝値が整数でない形。
    let text = rules_with_row(
        &dir,
        "text.toml",
        "id = \"gate.token_cap\"\nkind = \"MaturityCondition\"\nvalue = \"abc\"\nenabled = true\n",
    );
    let missing = dir.join("no-such-rules.toml");
    for (rules, want) in [
        (&absent, "gate.token_cap が無い"),
        (&disabled, "gate.token_cap は不発効である"),
        (&text, "gate.token_cap が整数でない"),
        (&missing, "rules を読めない"),
    ] {
        let out = run_bin_owned(
            &lens_args(&contract, &dir, &["--rules", &rules.display().to_string()], &claude),
            b"--- a\n+++ b\n",
        );
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{want}: rc 2 / {}", stderr_of(&out));
        assert!(!dir.join("called").exists(), "{want}: claude を 1 度も起動しない");
        let err = stderr_of(&out);
        assert!(err.contains(&format!("lens: {want}")), "理由を 1 行で名乗る: {err}");
        assert_eq!(err.lines().count(), 1, "stderr は理由の 1 行だけ: {err}");
        assert!(stdout_of(&out).is_empty(), "判定の面には何も出さない: {}", stdout_of(&out));
    }
    clean(&[&dir]);
}

/// runner が claude へ渡す argv に `--model` の対が在るか（値つき・fake が写した argv）。
fn model_arg(dir: &Path) -> Option<String> {
    let args = slurp(&dir.join("args"));
    let lines: Vec<&str> = args.lines().collect();
    lines
        .windows(2)
        .find(|w| w.first() == Some(&"--model"))
        .and_then(|w| w.get(1).map(|value| (*value).to_owned()))
}

/// (a) runner は rules 行 `runner.model` の model を claude に**毎回**渡す（`s2-07l.297`・設計 pipeline.md §6・FR5）:
/// `--rules` の manifest の値が `opus` なら argv に `--model opus` の対・値を `sonnet` に変えると対の値も変わる
/// （値は行から来る＝定数ではない）・表示名 `Opus` で書いた行も CLI の別名 `opus` で渡る（[`Model::parse`] →
/// `alias`）・`--rules` 無しは埋め込みの行（`opus`）。base は `--model` を渡さないので RED。
#[test]
fn headless_runner_passes_model_from_rules_row() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let call = RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None };
    for (value, want) in [("opus", "opus"), ("sonnet", "sonnet"), ("Opus", "opus"), ("Fable", "fable")] {
        let rules = rules_with_rows(&dir, &format!("rules-model-{value}.toml"), &[model_row(value)]);
        let out = run_runner_with_rules(&call, &rules, b"goal = \"x\"\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{value}: {}", stderr_of(&out));
        assert_eq!(model_arg(&dir), Some(want.to_owned()), "{value}: 行の値を CLI の別名で渡す: {}", slurp(&dir.join("args")));
        let args = slurp(&dir.join("args"));
        assert_eq!(args.lines().filter(|line| *line == "--model").count(), 1, "{value}: 対は 1 つ: {args}");
        assert!(!has_arg(&args, "--rules"), "{value}: --rules は claude へ渡らない: {args}");
        fs::remove_file(dir.join("args")).expect("前の周の写しを消せる");
    }
    // `--rules` 無しは埋め込みの manifest の行（`fleet usage` の refresh とは違い、runner は必ず渡す）。
    let out = run_runner(&call, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert_eq!(model_arg(&dir), Some(RUNNER_MODEL.to_owned()), "埋め込みの行: {}", slurp(&dir.join("args")));
    clean(&[&dir, &worktree]);
}

/// (b) lens も同じ行の model を claude に毎回渡す（runner と同じ構築点 `build`）: `--rules` の manifest の値ごとに
/// `--model` の対が変わり、`--rules` 無しは埋め込みの行。base は渡さないので RED。
#[test]
fn headless_lens_passes_model_from_rules_row() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"model の歯\"}\n", false, 0);
    let contract = contract_in(&dir);
    for (value, want) in [("opus", "opus"), ("haiku", "haiku"), ("Sonnet", "sonnet")] {
        let rules = rules_with_rows(&dir, &format!("rules-model-{value}.toml"), &[cap_row(4096), model_row(value)]);
        let out = run_bin_owned(&lens_args(&contract, &dir, &["--rules", &rules.display().to_string()], &claude), b"--- a\n+++ b\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{value}: {}", stderr_of(&out));
        assert_eq!(stdout_of(&out).trim(), r#"{"verdict":"PASS","evidence":"model の歯"}"#, "{value}: 判定は claude の行");
        assert_eq!(model_arg(&dir), Some(want.to_owned()), "{value}: 行の値を CLI の別名で渡す: {}", slurp(&dir.join("args")));
        fs::remove_file(dir.join("args")).expect("前の周の写しを消せる");
    }
    let out = run_bin_owned(&lens_args(&contract, &dir, &[], &claude), b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert_eq!(model_arg(&dir), Some(RUNNER_MODEL.to_owned()), "埋め込みの行: {}", slurp(&dir.join("args")));
    clean(&[&dir]);
}

/// (c) `runner.model` の行が解けない周（無い / 不発効 / 文字列でない / 閉じた表に無い値）は runner が **claude を
/// 呼ばず rc 2** で理由を 1 行（`runner: runner.model …`・lens の cap と同じ極性）。
///
/// base の断りと弁別する: base は `--rules` を知らずに読み飛ばし claude を起こす（argv の写しが**生成される**）。
/// 本歯は写しの不在と stderr の字面（`runner.model が無い`＝「未知の flag」ではない）の両方で測る。
#[test]
fn headless_runner_refuses_when_model_row_is_missing() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let call = RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None };
    let absent = rules_with_rows(&dir, "absent.toml", &[cap_row(4096)]);
    let disabled = rules_with_row(&dir, "disabled.toml", "id = \"runner.model\"\nkind = \"RunnerModel\"\nvalue = \"opus\"\nenabled = false\n");
    // id は同じで kind が整数の行（manifest は id と kind の対応を照合しない）＝値が文字列でない形。
    let int = rules_with_row(&dir, "int.toml", "id = \"runner.model\"\nkind = \"GateTokenCap\"\nvalue = 5\nenabled = true\n");
    let unknown = rules_with_rows(&dir, "unknown.toml", &[model_row("nope")]);
    let missing = dir.join("no-such-rules.toml");
    for (rules, want) in [
        (&absent, "runner.model が無い"),
        (&disabled, "runner.model は不発効である"),
        (&int, "runner.model が文字列でない"),
        (&unknown, "runner.model の値 nope は未知の model"),
        (&missing, "rules を読めない"),
    ] {
        let out = run_runner_with_rules(&call, rules, b"goal = \"x\"\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{want}: rc 2 / {}", stderr_of(&out));
        assert!(!dir.join("called").exists(), "{want}: claude を 1 度も起動しない");
        assert!(!dir.join("args").exists(), "{want}: argv の写しは生成されない");
        let err = stderr_of(&out);
        assert!(err.contains(&format!("runner: {want}")), "{want}: 理由を 1 行で名乗る: {err}");
        assert!(!err.contains("未知の引数") && !err.contains("usage: "), "{want}: 未知の flag の断りではない: {err}");
        assert_eq!(err.lines().count(), 1, "{want}: stderr は理由の 1 行だけ: {err}");
        assert!(stdout_of(&out).is_empty(), "{want}: stdout には何も出さない: {}", stdout_of(&out));
    }
    // `--rules` の値欠けは前提違反（rc 1・usage）。
    let mut args = runner_args(&call, &dir);
    args.push("--rules".to_owned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_bin(&refs, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "値欠け: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("--rules に値が無い"), "{}", stderr_of(&out));
    assert!(stderr_of(&out).contains("[--rules PATH]"), "usage は --rules を載せる: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    clean(&[&dir, &worktree]);
}

/// lens の `runner.model` の行も同じ極性（claude を呼ばず rc 2・理由 1 行）。cap の行が解けない周は cap の理由が先
/// （[`headless_lens_refuses_unreadable_cap_row`] の字面は不変）。
#[test]
fn headless_lens_refuses_when_model_row_is_missing() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let contract = contract_in(&dir);
    let absent = rules_with_rows(&dir, "absent.toml", &[cap_row(4096)]);
    let unknown = rules_with_rows(&dir, "unknown.toml", &[cap_row(4096), model_row("Opus 5")]);
    let both_missing = rules_with_row(&dir, "both.toml", "id = \"gate.lens_count\"\nkind = \"GateLensCount\"\nvalue = 1\nenabled = true\n");
    for (rules, want) in [
        (&absent, "runner.model が無い"),
        (&unknown, "runner.model の値 Opus 5 は未知の model"),
        (&both_missing, "gate.token_cap が無い"),
    ] {
        let out = run_bin_owned(&lens_args(&contract, &dir, &["--rules", &rules.display().to_string()], &claude), b"--- a\n+++ b\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{want}: rc 2 / {}", stderr_of(&out));
        assert!(!dir.join("called").exists(), "{want}: claude を 1 度も起動しない");
        let err = stderr_of(&out);
        assert!(err.contains(&format!("lens: {want}")), "{want}: 理由を 1 行で名乗る: {err}");
        assert_eq!(err.lines().count(), 1, "{want}: stderr は理由の 1 行だけ: {err}");
        assert!(stdout_of(&out).is_empty(), "{want}: 判定の面には何も出さない");
    }
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
    let rules = rules_with_cap(&dir, 4096);
    let out = run_bin(
        &[
            "lens", "--contract", &contract.display().to_string(),
            "--worktree", &dir.display().to_string(),
            "--rules", &rules.display().to_string(), "--permission-mode", "plan",
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

/// runner と lens の子は常に agent view 無しで起きる（`s2-07l.239`・設計 account-autonomy.md §5「agent view の前提」）:
/// `build` が子の env へ `CLAUDE_CODE_DISABLE_AGENT_VIEW=1` を設定し、親の値（[`INHERITED_AGENT_VIEW`]）を継承させない。
/// `--account-dir` を渡さない周でも同じ（口座の env とは別の 1 本）。base は親の値が子へそのまま届く（RED）。
#[test]
fn headless_agent_view_off_env_reaches_runner_and_lens() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert_eq!(slurp(&dir.join("agent-view")), "1", "runner の子で agent view を切る");

    // lens の周の写しを runner の周の残りと取り違えない（呼ばれた印ごと消してから撃つ）。
    fs::remove_file(dir.join("agent-view")).expect("runner の周の写しを消せる");
    fs::remove_file(dir.join("called")).expect("runner の周の印を消せる");
    let verdict = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"x\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &verdict, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "cap 内なので lens も claude を呼ぶ");
    assert_eq!(slurp(&dir.join("agent-view")), "1", "lens の子で agent view を切る");
    clean(&[&dir, &worktree]);
}

#[test]
fn headless_lens_inconclusive_on_unparsable_output() {
    let dir = tmp();
    let claude = fake_claude(&dir, "判定できませんでした\nもう一度お願いします\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
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
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a/src/lib.rs\n+++ b/src/lib.rs\n");
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

/// lens の prompt に載る「審査の前提」の見出し（`s2-07l.134`）。
///
/// ★**契約 fixture にも diff fixture にも現れない字面**を選んである——fixture が同じ字面を
/// 持つと、lens.txt から節を消しても「ちょうど 1 回」が fixture 側で満たされ歯が空虚になる。
const LENS_PREMISE_HEADING: &str = "## 審査の前提（検証は済んでいる）";

/// lens を固定の契約と diff で 1 回撃ち、claude の stdin に渡った prompt を返す。
fn lens_prompt_of_fixed_fixture() -> String {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a/fixture.txt\n+++ b/fixture.txt\n+line\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    clean(&[&dir]);
    prompt
}

/// lens の prompt は「verify は gate が済ませた・lens は tool を撃てない」前提を伝える
/// （`s2-07l.134`・設計 pipeline.md §5.3 / ADR-0011 §2.1）。前提が無いと実 lens は cargo を
/// 試して token を使い、撃てなかったことを INCONCLUSIVE の理由に混ぜた（.185 run 1 の実測）。
#[test]
fn headless_lens_prompt_states_verify_already_ran() {
    let prompt = lens_prompt_of_fixed_fixture();
    assert_eq!(
        prompt.matches(LENS_PREMISE_HEADING).count(),
        1,
        "前提の節の見出しがちょうど 1 回在る: {prompt}"
    );
    let premise = prompt.find(LENS_PREMISE_HEADING);
    let rubric = prompt.find("## 判定の決め方");
    let contract = prompt.find("## 契約");
    assert!(rubric.is_some() && contract.is_some(), "既存の見出しが在る: {prompt}");
    assert!(rubric < premise, "前提の節は「判定の決め方」より後: {prompt}");
    assert!(premise < contract, "前提の節は「## 契約」より前: {prompt}");
}

/// lens の prompt は審査の材料を契約と diff に限り、契約に名指しされていない検査を
/// 根拠にさせない（`s2-07l.231`・設計 pipeline.md §5.3）。限定が無いと実 lens は rustfmt の
/// 既定を持ち出して INCONCLUSIVE を出した（.223 run 1 の実測）。
///
/// ★文は契約 fixture にも diff fixture にも現れない字面——節を消せば回数が 0 に落ちる。
#[test]
fn headless_lens_scope_prompt_forbids_checks_not_named_by_contract() {
    const SCOPE_RULE: &str =
        "契約に名指しされていない検査（整形・rustfmt・lint の既定 等）を根拠に INCONCLUSIVE / FAIL を出さない。";
    const UNREACHED_RULE: &str = "判定に届かない周は、evidence に「契約のどの行を撃てなかったか」を書く。";
    let prompt = lens_prompt_of_fixed_fixture();
    assert_eq!(prompt.matches(SCOPE_RULE).count(), 1, "契約外の検査を根拠にしない文がちょうど 1 回在る: {prompt}");
    assert_eq!(prompt.matches(UNREACHED_RULE).count(), 1, "撃てなかった行を evidence に書く文が在る: {prompt}");
    let unfired = prompt.find("「verify を自分で撃てなかった」");
    let scope = prompt.find(SCOPE_RULE);
    let contract = prompt.find("## 契約");
    assert!(unfired.is_some() && contract.is_some(), "既存の句と見出しが在る: {prompt}");
    assert!(unfired < scope, "新しい節は「verify を自分で撃てなかった」の句より後: {prompt}");
    assert!(scope < contract, "新しい節は「## 契約」より前: {prompt}");
}

/// lens の prompt の外形（契約と diff の fixture を固定・C12.5）。
#[test]
fn headless_lens_prompt_external_form() {
    let prompt = lens_prompt_of_fixed_fixture();
    insta::assert_snapshot!("lens_prompt_external_form", prompt);
}

// ───── 契約の審査の雛形（`s2-07l.241`・設計 contract-source.md §4・FR49・接頭辞 `headless_lens_contract_`） ─────

/// 契約の審査の材料 `{design}`（★契約 fixture にも diff fixture にも lens-contract.txt にも現れない字面）。
const CONTRACT_DESIGN: &str = "docs/design/unlikely.md#z §4\n設計の節の本文 DESIGN-SECTION-MARK";
/// 契約の審査の材料 `{requirements}`（[`CONTRACT_DESIGN`] と同じ理由の字面）。
const CONTRACT_REQUIREMENTS: &str = "FR9: 要件の本文 REQUIREMENT-MARK";
/// 契約の審査の周に stdin へ流す diff（**prompt に載ってはならない**面＝契約の審査は stdin を読まない）。
const CONTRACT_STDIN: &[u8] = b"--- a/stdin-unread.txt\n+++ b/stdin-unread.txt\n+STDIN-MARK\n";

/// 契約の隣に審査の材料の 2 file（`design.txt` / `requirements.txt`・`pipe::review` が置く形）を書く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn material_in(dir: &Path, design: Option<&str>, requirements: Option<&str>) {
    for (name, body) in [("design.txt", design), ("requirements.txt", requirements)] {
        if let Some(text) = body {
            fs::write(dir.join(name), format!("{text}\n")).expect("材料を書ける");
        }
    }
}

/// 契約の審査の lens を固定の契約と材料で 1 回撃ち、claude の stdin に渡った prompt を返す。
fn lens_contract_prompt_of_fixed_fixture() -> String {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
    let contract = contract_in(&dir);
    material_in(&dir, Some(CONTRACT_DESIGN), Some(CONTRACT_REQUIREMENTS));
    let out = run_lens(&contract, 4096, "plan", &claude, CONTRACT_STDIN);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "材料が揃えば claude を呼ぶ");
    let prompt = slurp(&dir.join("stdin"));
    clean(&[&dir]);
    prompt
}

/// 契約の隣に材料の 2 file が在る周は雛形が契約の審査（`lens-contract.txt`）に切り替わる: 契約の各面と設計の節と
/// 要件本文がそれぞれの見出しの下に載り（順は 契約 → 設計の節 → 要件）、観点は 3 つで出力の形は diff の審査と同じ。
#[test]
fn headless_lens_contract_prompt_places_material_under_its_headings() {
    let prompt = lens_contract_prompt_of_fixed_fixture();
    let heading = |text: &str| prompt.find(text);
    let (design, requirements, contract) = (heading("## 契約が実装する設計の節"), heading("## 契約が満たす要件"), heading("## 契約"));
    assert!(contract.is_some() && design.is_some() && requirements.is_some(), "3 つの見出し: {prompt}");
    assert!(contract < design && design < requirements, "見出しの順は 契約 → 設計の節 → 要件: {prompt}");
    let mark = prompt.find("DESIGN-SECTION-MARK");
    assert!(mark > design && mark < requirements, "設計の節は自分の見出しの下: {prompt}");
    assert!(prompt.find("REQUIREMENT-MARK") > requirements, "要件本文は自分の見出しの下: {prompt}");
    assert!(prompt.contains(&format!("goal: {CONTRACT_GOAL}")) && prompt.contains(&format!("done: {CONTRACT_DONE}")), "契約の面: {prompt}");
    for want in [CONTRACT_VERIFY, CONTRACT_VERIFY_2, CONTRACT_WRITE_SET, CONTRACT_WRITE_SET_2] {
        assert!(prompt.contains(&format!("- {want}")), "契約の {want} が行として載る: {prompt}");
    }
    assert_eq!(prompt.matches("## 審査の観点").count(), 1, "観点の節がちょうど 1 回: {prompt}");
    for point in ["1. **契約と設計の節の適合**", "2. **設計が名指す状態遷移の一周**", "3. **write-set の連鎖**"] {
        assert_eq!(prompt.matches(point).count(), 1, "観点 {point} がちょうど 1 回: {prompt}");
    }
    assert!(prompt.contains(r#"{"verdict":"PASS|FAIL|INCONCLUSIVE","evidence":"<根拠を 1 行で>"}"#), "出力の形は同じ: {prompt}");
}

/// 契約の審査の prompt は diff の節と裁定の節を持たず **stdin は読まれない**（stdin の字面は prompt に載らない）。
/// 穴は 3 つとも埋まり、diff の穴の字面も残らない。
#[test]
fn headless_lens_contract_prompt_ignores_stdin_and_fills_every_hole() {
    let prompt = lens_contract_prompt_of_fixed_fixture();
    assert!(!prompt.contains("## diff") && !prompt.contains("STDIN-MARK"), "diff の節は無く stdin は読まない: {prompt}");
    assert!(!prompt.contains("## 契約への裁定"), "裁定の節は diff の審査だけ: {prompt}");
    for hole in ["{contract}", "{design}", "{requirements}", "{diff}"] {
        assert!(!prompt.contains(hole), "穴 {hole} が埋まっている: {prompt}");
    }
}

/// 契約の審査の prompt の外形（契約と材料の fixture を固定・C12.5）。
#[test]
fn headless_lens_contract_prompt_external_form() {
    let prompt = lens_contract_prompt_of_fixed_fixture();
    insta::assert_snapshot!("lens_contract_prompt_external_form", prompt);
}

/// 材料が片方だけ在る周は壊れた材料として claude を呼ばず rc 2（無い方を名指す）。2 つとも無ければ従来の diff の
/// 審査（stdin が載る・対）。
#[test]
fn headless_lens_contract_half_material_is_refused_without_calling_claude() {
    for (design, requirements, missing) in [
        (Some(CONTRACT_DESIGN), None, "requirements.txt"),
        (None, Some(CONTRACT_REQUIREMENTS), "design.txt"),
    ] {
        let dir = tmp();
        let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
        let contract = contract_in(&dir);
        material_in(&dir, design, requirements);
        let out = run_lens(&contract, 4096, "plan", &claude, CONTRACT_STDIN);
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "片方だけは rc 2: {}", stderr_of(&out));
        assert!(stderr_of(&out).contains(missing), "無い方を名指す: {}", stderr_of(&out));
        assert!(!dir.join("called").exists(), "claude を起動しない");
        clean(&[&dir]);
    }
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &claude, CONTRACT_STDIN);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    assert!(prompt.contains("STDIN-MARK") && prompt.contains("## diff"), "材料が無ければ diff の審査: {prompt}");
    clean(&[&dir]);
}

/// cap は契約 + 節 + 要件の byte で照合する（NFR1・FR9）: 材料が cap を超える周は claude を呼ばず INCONCLUSIVE。
/// 同じ cap で stdin が空の diff の審査は呼ばれる（対＝cap を測っているのは材料の byte）。
#[test]
fn headless_lens_contract_material_over_cap_is_inconclusive_without_calling_claude() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
    let contract = contract_in(&dir);
    material_in(&dir, Some(CONTRACT_DESIGN), Some(CONTRACT_REQUIREMENTS));
    let out = run_lens(&contract, 64, "plan", &claude, b"");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(stdout_of(&out).contains(r#""verdict":"INCONCLUSIVE""#), "cap 超は INCONCLUSIVE: {}", stdout_of(&out));
    assert!(stdout_of(&out).contains("contract material exceeds cap"), "理由は材料の cap 超: {}", stdout_of(&out));
    assert!(!dir.join("called").exists(), "claude を起動しない");
    clean(&[&dir]);
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 64, "plan", &claude, b"");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "材料が無い周は diff（0 byte）が cap の内側＝呼ぶ");
    clean(&[&dir]);
}

/// lens の prompt に載る裁定の節の見出し（`s2-07l.309`）。
///
/// ★契約 fixture にも diff fixture にも裁定 fixture にも現れない字面（節を消せば回数が 0 に落ちる）。
const LENS_RULINGS_HEADING: &str = "## 契約への裁定（planner の回答・逐語）";

/// 裁定の file の本文（gate が書く形・1 対 = 3 行）。`{diff}` の字面を持たせ、穴が同じ 1 走査で埋まること
/// （裁定の中の穴は展開されない）も測る。
const RULINGS_FIXTURE: &str = "question: verify 行が矛盾する {diff}\nabout: verify\nanswer: verify は 1 行目だけを撃つ\n";

/// lens は `{contract}` の path の**同じ dir** の `rulings.txt` を読み、本文をそのまま `{rulings}` の穴へ埋める
/// （`s2-07l.309`・設計 pipeline-question.md）。節は `## 契約` の後・`## diff` の前。base は穴も節も無いので RED。
#[test]
fn lens_rulings_are_filled_into_the_prompt_from_the_sibling_file() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"裁定を読めた\"}\n", false, 0);
    let contract = contract_in(&dir);
    fs::write(dir.join("rulings.txt"), RULINGS_FIXTURE).expect("裁定の file を書ける");
    let out = run_lens(&contract, 4096, "plan", &claude, b"DIFF-BODY-MARKER\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "裁定が在る周も claude を呼ぶ");
    let prompt = slurp(&dir.join("stdin"));
    assert_eq!(prompt.matches(LENS_RULINGS_HEADING).count(), 1, "裁定の節の見出しがちょうど 1 回在る: {prompt}");
    // **本文はそのまま**（見出しの直下・逐語・穴は展開されない）。
    assert!(
        prompt.contains(&format!("{LENS_RULINGS_HEADING}\n{RULINGS_FIXTURE}")),
        "裁定の本文が見出しの直下に逐語で載る: {prompt}"
    );
    assert_eq!(prompt.matches("DIFF-BODY-MARKER").count(), 1, "裁定の中の {{diff}} は展開されない: {prompt}");
    assert!(!prompt.contains("{rulings}"), "裁定の穴が埋まっている: {prompt}");
    assert!(!prompt.contains("（裁定なし）"), "裁定が在る周に「裁定なし」を出さない: {prompt}");
    let contract_at = prompt.find("## 契約\n");
    let rulings_at = prompt.find(LENS_RULINGS_HEADING);
    let diff_at = prompt.find("## diff");
    assert!(contract_at.is_some() && diff_at.is_some(), "既存の見出しが在る: {prompt}");
    assert!(contract_at < rulings_at, "裁定の節は「## 契約」より後: {prompt}");
    assert!(rulings_at < diff_at, "裁定の節は「## diff」より前: {prompt}");
    // 読み方の 1 行は「審査の材料」の節に在り、裁定の節より前。
    const RULING_RULE: &str = "裁定の節に在る逸脱（回答で planner が認めた形）は契約の一部として読む。裁定に無い逸脱だけを契約違反と読む。";
    assert_eq!(prompt.matches(RULING_RULE).count(), 1, "読み方の行がちょうど 1 回在る: {prompt}");
    assert!(prompt.find(RULING_RULE) < contract_at, "読み方の行は「## 契約」より前: {prompt}");
    clean(&[&dir]);
}

/// 裁定の file が無い周は `（裁定なし）` の 1 行を穴へ埋める（「裁定なし」を明示する・C10・空を黙らせない）。
#[test]
fn lens_rulings_absent_reads_as_none() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"裁定なし\"}\n", false, 0);
    let contract = contract_in(&dir);
    assert!(!dir.join("rulings.txt").exists(), "fixture: 裁定の file は無い");
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "裁定が無くても claude を呼ぶ");
    let prompt = slurp(&dir.join("stdin"));
    assert_eq!(prompt.matches(LENS_RULINGS_HEADING).count(), 1, "裁定の節の見出しは在る: {prompt}");
    assert!(
        prompt.contains(&format!("{LENS_RULINGS_HEADING}\n（裁定なし）\n")),
        "見出しの直下に「裁定なし」の 1 行: {prompt}"
    );
    assert_eq!(prompt.matches("（裁定なし）").count(), 1, "「裁定なし」はちょうど 1 回: {prompt}");
    assert!(!prompt.contains("{rulings}"), "裁定の穴が埋まっている: {prompt}");
    clean(&[&dir]);
}

/// 裁定の file が**在るのに読めない**周（file の場所に dir が置かれている・UTF-8 でない）は claude を呼ばず rc 2
/// で理由を 1 行（`lens: 裁定を読めない`）。「無い」と「読めない」で極性を変える＝読めない裁定を「裁定なし」に
/// 倒すと、回答で認めた逸脱が契約違反に読まれる。
#[test]
fn lens_rulings_unreadable_is_broken() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let contract = contract_in(&dir);
    fs::create_dir(dir.join("rulings.txt")).expect("file の場所に dir を置ける");
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない裁定は rc 2: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    assert!(!dir.join("stdin").exists(), "prompt の写しも生成されない");
    assert!(stderr_of(&out).contains("lens: 裁定を読めない"), "理由を名乗る: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("rulings.txt"), "読めなかった path を名指す: {}", stderr_of(&out));
    // UTF-8 でない本文も同じ極性（「在るが読めない」）。
    let bad = tmp();
    let quiet = fake_claude(&bad, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let bad_contract = contract_in(&bad);
    fs::write(bad.join("rulings.txt"), [0xff_u8, 0xfe, 0x00]).expect("壊れた本文を書ける");
    let out = run_lens(&bad_contract, 4096, "plan", &quiet, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "UTF-8 でない裁定も rc 2: {}", stderr_of(&out));
    assert!(!bad.join("called").exists(), "claude を 1 度も起動しない");
    clean(&[&dir, &bad]);
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
    let out = run_lens(&contract, 4096, "plan", &claude, b"DIFF-BODY-MARKER\n");
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
    let rules = rules_with_cap(&dir, 4096);
    let out = run_bin(
        &[
            "lens",
            "--contract", &contract.display().to_string(),
            "--worktree", &worktree.display().to_string(),
            "--rules", &rules.display().to_string(),
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
    let rules = rules_with_cap(&dir, 4096);
    let out = run_bin(
        &[
            "lens",
            "--contract", &contract.display().to_string(),
            "--rules", &rules.display().to_string(), "--permission-mode", "plan",
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
    let rules = rules_with_cap(&dir, 4096);
    let out = run_bin(
        &[
            "lens", "--rules", &rules.display().to_string(), "--permission-mode", "plan",
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
    let out = run_lens(&broken, 4096, "plan", &claude, b"--- a\n+++ b\n");
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
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
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
    let seen = run_lens(&contract, 4096, "plan", &lens_claude, b"--- a\n+++ b\n");
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
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
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

/// **上限で止めた周の停止行は stdout に出る**（`s2-07l.190`・設計 account-autonomy.md §2）。
///
/// pipe は runner の stdout だけを捕らえるので、stderr に出すと段の記帳へ status が届かない。
/// 止める側の集合が空である以上 production の binary は止まる側を通らないので、結果を組む口を
/// 直接撃つ（`conclude` はこの 1 本を返す）。
#[test]
fn headless_runner_limited_puts_the_stop_line_on_stdout() {
    use vessel::headless::runner::{limited, stop_line, stop_status};
    let out = limited("allowed_warning");
    assert_eq!(out.out, vec![stop_line("allowed_warning")], "停止行は stdout の 1 行");
    assert!(out.err.is_empty(), "stderr には出さない: {:?}", out.err);
    assert_eq!(out.rc, RC_RATE_LIMIT, "rc は RC_RATE_LIMIT のまま");
    assert_eq!(
        out.out.last().map(String::as_str).and_then(stop_status),
        Some("allowed_warning"),
        "pipe の読み手で往復する"
    );
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
    plugin_leaf(&dir);
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

// ── plugin root の展開（設計 docs/design/pipeline.md §6・s2-07l.149・SRS FR20） ──────

/// fake が残した argv の写しから `--plugin-dir` の値を順に集める。
fn plugin_dir_values(args: &str) -> Vec<String> {
    let lines: Vec<&str> = args.lines().collect();
    lines
        .windows(2)
        .filter(|w| w.first() == Some(&"--plugin-dir"))
        .filter_map(|w| w.get(1).map(|value| (*value).to_owned()))
        .collect()
}

/// root の配下の dir を**名前順に 1 つずつ** `--plugin-dir` へ渡す。root 直下の file と、dir を
/// 指す symlink は plugin と見ない。作る順を名前の逆にする（作った順を写す実装を落とす）。
#[test]
fn headless_plugin_root_passes_each_subdir_in_name_order() {
    let dir = tmp();
    let worktree = tmp();
    let root = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    for name in ["zz-consumer", "aa-vessel"] {
        fs::create_dir(root.join(name)).expect("plugin の dir を作れる");
    }
    fs::write(root.join("mm-file.json"), "{}\n").expect("root 直下に file を置ける");
    std::os::unix::fs::symlink(root.join("aa-vessel"), root.join("bb-link"))
        .expect("root 直下に dir への symlink を置ける");
    let out = run_runner_in(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        &root,
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let args = slurp(&dir.join("args"));
    assert_eq!(
        plugin_dir_values(&args),
        vec![
            root.join("aa-vessel").display().to_string(),
            root.join("zz-consumer").display().to_string(),
        ],
        "配下の dir を名前順に 2 本・file と symlink は渡さない: {args}"
    );
    clean(&[&dir, &worktree, &root]);
}

/// 配下に dir が 0 の root（file と dir への symlink だけ）では **claude を起こさず rc 2**。
/// guard 0 本の claude を起こさない（fail-closed・憲法 C16.2）。
#[test]
fn headless_plugin_root_without_subdirs_does_not_start_claude() {
    let dir = tmp();
    let worktree = tmp();
    let root = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(root.join("plugin.json"), "{}\n").expect("root 直下に file を置ける");
    std::os::unix::fs::symlink(&worktree, root.join("linked")).expect("root 直下に dir への symlink を置ける");
    let out = run_runner_in(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        &root,
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "配下 0 の root は rc 2: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "配下 0 の root では claude を起動しない");
    assert!(stderr_of(&out).contains("plugin root"), "何が壊れたかを名指す: {}", stderr_of(&out));
    clean(&[&dir, &worktree, &root]);
}

/// root が無い周も **claude を起こさず rc 2**（読めない root を空の root として続けない）。
#[test]
fn headless_plugin_root_absent_does_not_start_claude() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    let absent = dir.join("no-such-root");
    let out = run_runner_in(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        &absent,
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "不在の root は rc 2: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "不在の root では claude を起動しない");
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

/// prompt は「追随」節の**読み方**（雛形）を運び、pipeline が stdin へ足した run ごとの節を
/// **そのまま**載せる（設計 pipeline-conflict.md §3・`s2-07l.146`）。節の順序は stdin の順序
/// （契約 → 回答 → 追随）がそのまま prompt の順序になる＝包みは並べ替えない。
#[test]
fn runner_prompt_carries_follow_section_rule_and_keeps_section_order() {
    let dir = tmp();
    let worktree = tmp();
    // pipeline が stdin へ流す形: 契約の写し + 「## 回答」節 + 「## 追随」節。
    let contract = "goal = \"x\"\n\n## 回答\n- 質問: verify 行が矛盾する\n- 回答: verify は 1 行目だけを撃つ\n\n## 追随\n- main が deadbeef へ進んだ\n";
    let out = run_question_runner(&dir, &worktree, "", 0, contract.as_bytes());
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    assert!(prompt.contains("## 追随」節"), "追随節の読み方を運ぶ: {prompt}");
    assert!(prompt.contains("git rebase --continue"), "解き終え方を命じる: {prompt}");
    assert!(prompt.contains("git rebase --abort"), "解けない周の戻し方を命じる: {prompt}");
    assert!(prompt.contains("- main が deadbeef へ進んだ"), "stdin の節がそのまま載る: {prompt}");
    let answer_at = prompt.rfind("- 回答: verify は 1 行目だけを撃つ");
    let follow_at = prompt.rfind("- main が deadbeef へ進んだ");
    assert!(
        matches!((answer_at, follow_at), (Some(a), Some(f)) if a < f),
        "stdin の順序（回答 → 追随）が保たれる: {prompt}"
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

// ── 質問 record の置き場の判定は top-level の key で見る（s2-07l.123・.116 実 run の現物） ──

/// 実 claude 2.1.268 の `result` record の形（.116 の raw stream から必要最小へ写した）: `usage.iterations[]`
/// の入れ子に `"type":"message"` が在り、top-level の `"type":"result"` はその**後ろ**に来る。
const REAL_RESULT_RECORD_HEAD: &str = r#"{"duration_api_ms":15819,"stop_reason":"end_turn","usage":{"input_tokens":4,"iterations":[{"input_tokens":2,"output_tokens":387,"type":"message"}]},"modelUsage":{"m":{"inputTokens":4,"outputTokens":724}},"permission_denials":[],"is_error":false,"num_turns":3,"subtype":"success","api_error_status":null,"result":""#;
const REAL_RESULT_RECORD_TAIL: &str = r#"","ttft_ms":3865,"type":"result","duration_ms":14899,"uuid":"2f7f195c","result_index":0}"#;

/// 実 record の形に `result` の text（escape 済み）を挟む。
fn real_result_record(result_json_text: &str) -> String {
    format!("{REAL_RESULT_RECORD_HEAD}{result_json_text}{REAL_RESULT_RECORD_TAIL}")
}

/// `result_text` は入れ子の `"type"` を種別と読まず、top-level の `"type":"result"` を見る。
#[test]
fn runner_question_toplevel_result_text_reads_real_record_with_nested_type_first() {
    use vessel::headless::runner::result_text;
    let line = real_result_record(r#"契約の done が矛盾する。\n{\"question\":\"どちらの期待値が正しいか\",\"about\":\"done\"}"#);
    assert_eq!(
        result_text(&line).as_deref(),
        Some("契約の done が矛盾する。\n{\"question\":\"どちらの期待値が正しいか\",\"about\":\"done\"}"),
        "入れ子の type:message が先に在っても result record と読む"
    );
    // 偽陽性を塞ぐ: 入れ子だけに type:result を持ち、top-level の種別が別の行は読まない。
    // 入れ子を**先**に置く（top-level を先に置くと最初の対を読む実装でも None になり歯が空虚・lens H1）。
    let nested_only = r#"{"quoted":{"type":"result","result":"inner"},"type":"assistant","result":"outer"}"#;
    assert_eq!(result_text(nested_only), None, "入れ子の type:result は種別ではない");
    // 文字列中の escape された `"` を閉じ引用符と読まない（奇数個の `\"` で走査がずれると top-level の
    // `"type"` を見失う・lens H2）。
    let odd_escape = r#"{"result":"he said \"hi","type":"result"}"#;
    assert_eq!(result_text(odd_escape).as_deref(), Some("he said \"hi"), "escape された引用符は文字列を閉じない");
    // 文字列の中の brace / bracket は深さに数えない（後ろの top-level key を見失わない）。
    let braces_in_string = r#"{"note":"has { and [ inside","type":"result","result":"ok"}"#;
    assert_eq!(result_text(braces_in_string).as_deref(), Some("ok"), "文字列中の brace は深さに数えない");
}

/// 包み経由: 実 record の形で最終行に record → rc 76・stdout 最終行に同じ record（base では rc 0 を写す）。
#[test]
fn runner_question_toplevel_real_record_yields_rc76_through_the_wrapper() {
    let dir = tmp();
    let worktree = tmp();
    let text = r#"契約を読んだ。\n{\"question\":\"verify 行が矛盾する\",\"about\":\"verify\"}"#;
    let body = format!("{{\"type\":\"system\",\"subtype\":\"init\"}}\n{}\n", real_result_record(text));
    let out = run_question_runner(&dir, &worktree, &body, 0, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(vessel::pipe::RC_QUESTION)), "実 record の形でも rc 76: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert_eq!(stdout.lines().last(), Some(QUESTION_RECORD), "最終行は同じ record: {stdout}");
    clean(&[&dir, &worktree]);
}

/// `rate_limit_info` の直下で `status` より**前**に文字列中の brace（`"note":"win {5h}"`）が在る行
/// （`s2-07l.126`・.123 lens MED-4）: 切り出しが文字列の中の `{` で早く終わると status が None になり、
/// 止めるべき便を止めない（fail-open の向き）。文字列と escape を飛ばして**文字列の外**の brace で切る。
#[test]
fn runner_rate_limit_brace_inside_string_before_status_is_still_read() {
    use vessel::headless::runner::rate_limit_status;
    let braces = r#"{"type":"rate_limit_event","rate_limit_info":{"note":"win {5h}","status":"allowed_warning"}}"#;
    assert_eq!(rate_limit_status(braces), Some("allowed_warning"), "文字列中の brace は境界ではない");
    // escape された引用符を含む文字列でも同じ（`\"` で文字列を閉じたと読むと `{` が外に見える）。
    let escaped = r#"{"type":"rate_limit_event","rate_limit_info":{"note":"say \"{\" then }","status":"blocked"}}"#;
    assert_eq!(rate_limit_status(escaped), Some("blocked"), "escape された引用符は文字列を閉じない");
    // 閉じ brace だけが文字列に在る形（早期終端の向きが `}` でも同じ）。
    let closing = r#"{"type":"rate_limit_event","rate_limit_info":{"note":"}","status":"blocked"}}"#;
    assert_eq!(rate_limit_status(closing), Some("blocked"), "文字列中の閉じ brace で切らない");
}

/// 同じ行で status が止める側の集合に在れば `decide` が止める（base では None → Ignore で止まらない）。
#[test]
fn runner_rate_limit_brace_inside_string_still_stops_when_status_is_in_the_set() {
    use vessel::headless::runner::{decide, Decision};
    let braces = r#"{"type":"rate_limit_event","rate_limit_info":{"note":"win {5h}","status":"blocked"}}"#;
    assert_eq!(decide(braces, &["blocked"]), Decision::Stop("blocked".to_owned()), "止める側なら Stop");
    assert_eq!(decide(braces, &[]), Decision::Observed("blocked".to_owned()), "集合に無ければ記録だけ");
}

/// status より前に**入れ子の object**（文字列でない brace）が在っても直下の status を読む（planner 裁定 = 案 P・
/// lens-126 HIGH-1）。入れ子で打ち切る形は key の並び次第で直下の status を取り逃す fail-open の穴だった（`.123` の
/// `usage.iterations[]` と同型・base では None）。入れ子の**中**の status は深さ guard が読まない（既存の歯と同じ向き）。
#[test]
fn runner_rate_limit_brace_nested_object_before_status_still_cuts() {
    use vessel::headless::runner::rate_limit_status;
    let nested_first = r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"status":"other"},"status":"blocked"}}"#;
    assert_eq!(rate_limit_status(nested_first), Some("blocked"), "入れ子が先でも直下の status を読む（base では None）");
    // 入れ子の中にしか status が無ければ読まない（直下の判定は深さ guard・兄弟の入れ子を採らない）。
    let nested_only = r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"status":"other"},"note":"x"}}"#;
    assert_eq!(rate_limit_status(nested_only), None, "入れ子の中の status は直下ではない");
    // 入れ子が status の後ろでも同じ。
    let nested_after = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"blocked","unifiedWindows":{"status":"other"}}}"#;
    assert_eq!(rate_limit_status(nested_after), Some("blocked"), "直下の status は読める");
}

/// 上限 record の読みも同じ走査に乗る: 入れ子の `"type"` が先に在っても `rate_limit_event` を種別と読み、
/// 入れ子だけに `rate_limit_event` を持つ行（別 record の引用）は読まない。
#[test]
fn runner_question_toplevel_rate_limit_status_ignores_nested_type_keys() {
    use vessel::headless::runner::rate_limit_status;
    let nested_first = r#"{"meta":{"type":"noise"},"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","unifiedWindows":{"status":"other"}}}"#;
    assert_eq!(rate_limit_status(nested_first), Some("allowed_warning"), "入れ子の type が先でも top-level を読む");
    let quoted = r#"{"type":"assistant","quoted":{"type":"rate_limit_event","rate_limit_info":{"status":"blocked"}}}"#;
    assert_eq!(rate_limit_status(quoted), None, "引用された上限 record は読まない");
}

// ── 最終 result の観測行（`s2-07l.258`・設計 pipeline.md §6・SRS FR6 / NFR4） ──────────────
//
// claude が `is_error` の result で終わった周、その事実は stream の中にしか無く、stream は捨てられる。
// runner は要約行の**前**に観測行 1 本（`runner: result subtype=… is_error=… text=…`）を出し、
// **最終行は変えない**（pipeline は最終行だけを読む）。

/// 観測行の接頭辞。
const RESULT_LINE_HEAD: &str = "runner: result ";

/// stdout の中の観測行（無ければ `None`・2 本以上は歯が落とす）。
fn result_line_of(out: &Output) -> Option<String> {
    let text = stdout_of(out);
    let found: Vec<&str> = text.lines().filter(|line| line.starts_with(RESULT_LINE_HEAD)).collect();
    assert!(found.len() <= 1, "観測行は 1 本まで: {text}");
    found.first().map(|line| (*line).to_owned())
}

/// (d) `is_error` の result で rc 1 → 観測行が要約行の**前**に在り、最終行は要約行のまま。
#[test]
fn headless_runner_result_line_records_error_result_before_the_summary_on_nonzero_rc() {
    let dir = tmp();
    let worktree = tmp();
    let body = "{\"type\":\"system\",\"subtype\":\"init\"}\n\
                {\"type\":\"result\",\"subtype\":\"error_during_execution\",\"is_error\":true,\"result\":\"boom\\nline2\"}\n";
    let out = run_question_runner(&dir, &worktree, body, 1, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(1), "claude の rc を写す: {}", stderr_of(&out));
    let text = stdout_of(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.last().copied(),
        Some("runner: rc=1 records=2"),
        "最終行は要約行のまま（pipeline が読む面を変えない）: {lines:?}"
    );
    assert_eq!(
        lines.iter().rev().nth(1).copied(),
        Some("runner: result subtype=error_during_execution is_error=true text=boom line2"),
        "観測行は要約行の直前・改行は空白に: {lines:?}"
    );
    clean(&[&dir, &worktree]);
}

/// (e) rc 0 ∧ `subtype=success` → `is_error=false`・最終行は要約行のまま。
#[test]
fn headless_runner_result_line_records_success_and_keeps_the_summary_last() {
    let dir = tmp();
    let worktree = tmp();
    let body = "{\"type\":\"system\",\"subtype\":\"init\"}\n\
                {\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"done\",\"usage\":{\"is_error\":true}}\n";
    let out = run_question_runner(&dir, &worktree, body, 0, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let text = stdout_of(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.last().copied(), Some("runner: rc=0 records=2"), "最終行は要約行のまま: {lines:?}");
    assert_eq!(
        result_line_of(&out).as_deref(),
        Some("runner: result subtype=success is_error=false text=done"),
        "入れ子の is_error は読まない（top-level だけ）: {lines:?}"
    );
    clean(&[&dir, &worktree]);
}

/// (f) result record が無い stream（assistant だけ）→ 観測行が**無い**（「無い」を `-` に化けさせない）。
/// (d) と対で置く（負例だけで RED を主張しない）。
#[test]
fn headless_runner_result_line_is_absent_without_a_result_record() {
    let dir = tmp();
    let worktree = tmp();
    let body = "{\"type\":\"system\",\"subtype\":\"init\"}\n\
                {\"type\":\"assistant\",\"message\":{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false}}\n";
    for want in [0_u8, 1] {
        let out = run_question_runner(&dir, &worktree, body, want, b"goal = \"x\"\n");
        assert_eq!(out.status.code(), Some(i32::from(want)), "{}", stderr_of(&out));
        assert_eq!(result_line_of(&out), None, "record を見ていない周は観測行を出さない: {}", stdout_of(&out));
        assert!(
            stdout_of(&out).lines().last().is_some_and(|line| line == format!("runner: rc={want} records=2")),
            "要約行は変わらない: {}",
            stdout_of(&out)
        );
    }
    clean(&[&dir, &worktree]);
}

/// (g) 質問 record の周（rc 76）→ 観測行 + 要約行 + 質問 record の 3 行で、最終行は質問 record のまま。
#[test]
fn headless_runner_result_line_precedes_the_summary_and_the_question_record() {
    let dir = tmp();
    let worktree = tmp();
    let text = r#"契約を読んだ。\n{\"question\":\"verify 行が矛盾する\",\"about\":\"verify\"}"#;
    let out = run_question_runner(&dir, &worktree, &stream_with_result(text), 0, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(vessel::pipe::RC_QUESTION)), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines,
        [
            "runner: result subtype=- is_error=false text=契約を読んだ。 {\"question\":\"verify 行が矛盾する\",\"about\":\"verify\"}",
            "runner: rc=0 records=2",
            QUESTION_RECORD,
        ],
        "3 行・最終行は質問 record のまま: {stdout}"
    );
    clean(&[&dir, &worktree]);
}

/// (h) text が上限を超える → 先頭 [`vessel::headless::runner::RESULT_TEXT_CHARS`] 字で切れる（`…` は付けない・字数で数える）。
#[test]
fn headless_runner_result_line_cuts_the_text_at_the_char_limit() {
    use vessel::headless::runner::RESULT_TEXT_CHARS;
    let dir = tmp();
    let worktree = tmp();
    // 多 byte 字で埋める＝byte で切る実装は字数が合わない（字数で数えることを測る）。
    let long = "字".repeat(RESULT_TEXT_CHARS + 50);
    let body = format!("{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"{long}\"}}\n");
    let out = run_question_runner(&dir, &worktree, &body, 0, b"goal = \"x\"\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let line = result_line_of(&out).unwrap_or_default();
    let text = line.split_once(" text=").map(|(_, text)| text).unwrap_or_default();
    assert_eq!(text.chars().count(), RESULT_TEXT_CHARS, "先頭 {RESULT_TEXT_CHARS} 字で切る: {line}");
    assert_eq!(text, "字".repeat(RESULT_TEXT_CHARS), "切った後に `…` を付けない: {line}");
    // 上限ちょうどは切らない。
    let exact = "a".repeat(RESULT_TEXT_CHARS);
    let body = format!("{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"{exact}\"}}\n");
    let out = run_question_runner(&dir, &worktree, &body, 0, b"goal = \"x\"\n");
    let line = result_line_of(&out).unwrap_or_default();
    assert!(line.ends_with(&format!(" text={exact}")), "上限ちょうどは丸ごと: {line}");
    clean(&[&dir, &worktree]);
}

/// 純関数の面: 種別は閉じた enum（未知は `unknown` 1 つに潰し字面を残さない）・`is_error` は top-level の bool だけ・
/// 無いものは `-`・tab と改行は空白。
#[test]
fn headless_runner_result_line_pure_readers_and_format() {
    use vessel::headless::runner::{result_is_error, result_line, result_subtype, ResultKind};
    assert_eq!(result_subtype(r#"{"type":"result","subtype":"error_max_turns"}"#), Some(ResultKind::ErrorMaxTurns));
    assert_eq!(result_subtype(r#"{"type":"result","subtype":"something_new"}"#), Some(ResultKind::Unknown), "未知は 1 variant");
    assert_eq!(result_subtype(r#"{"type":"result","is_error":true}"#), None, "subtype 無し");
    assert_eq!(result_subtype(r#"{"type":"assistant","subtype":"success"}"#), None, "result record でない");
    assert_eq!(result_is_error(r#"{"type":"result","is_error":true}"#), Some(true));
    assert_eq!(result_is_error(r#"{"type":"result","is_error": false ,"x":1}"#), Some(false), "空白に寛容");
    assert_eq!(result_is_error(r#"{"type":"result","usage":{"is_error":true}}"#), None, "入れ子は読まない");
    assert_eq!(result_is_error(r#"{"type":"result","is_error":"true"}"#), None, "文字列は bool ではない");
    assert_eq!(
        result_line(Some(ResultKind::Unknown), None, Some("a\tb\r\nc")),
        "runner: result subtype=unknown is_error=- text=a b  c"
    );
    assert_eq!(result_line(None, Some(true), None), "runner: result subtype=- is_error=true text=-");
}
