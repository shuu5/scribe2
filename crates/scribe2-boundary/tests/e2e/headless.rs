//! (d) headless の runner と lens の歯（設計 docs/design/pipeline.md §8 (d)）。
//!
//! claude は **fake の実行 file**（`--claude` の seam）で、引数・cwd・口座 env を file へ
//! 写し、決めた body を stdout へ出す。**CI で実 claude は呼ばない**。
//!
//! 歯は族ごとの子 module に置く（設計 docs/design/carry-prep.md §8 行 f・`s2-07l.675`）: `runner`（接頭辞
//! `headless_runner_` / `runner_question_` / `runner_rate_` / `runner_prompt_`）・`lens`（接頭辞 `headless_lens_` /
//! `lens_rulings_`）。この file には共有の helper と const・外形 snapshot の歯（snapshot 名が module path を含む
//! ので動かさない）・他の族の歯だけを残す。

mod lens;
mod runner;
// flip-check: moved s2-07l.675

use crate::{make_tmp_dir, TmpDir};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
use vessel::headless::{RC_RATE_LIMIT, RC_UNREACHABLE};

/// binary の path。
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_scribe2")
}

/// tmp dir を 1 つ作り、symlink を解いた path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn tmp() -> TmpDir {
    let dir = make_tmp_dir().expect("tmp dir を作れる");
    dir.canonical().expect("tmp dir の実体 path を解ける")
}

/// fake claude を 1 本作る。
///
/// 起動されたら `called` を残し、引数を `args`・cwd を `cwd`・口座 env を `account`・agent view の
/// env を `agent-view`・env の全行を `env` へ写してから `body` を stdout へ出す。
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
         env > \"{d}/env\"\n\
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

/// binary へ渡す親の自動 memory の env の値（`1` でも空でもない字面・[`INHERITED_AGENT_VIEW`] と同じ理由）。
const INHERITED_AUTO_MEMORY: &str = "inherited-memory-from-parent";

/// binary を撃つ側（席の pane の中）の `TMUX_PANE`。
///
/// 席の管理席が `pipe` の外で `runner` / `lens` を単体起動する周を作る。子（claude）に届くと hook が
/// `--pane` でこの席の打刻へ書く（他 process の打刻の混入・設計 seat-roles.md §4）。
const PARENT_PANE: &str = "%99";

/// binary の起動を組む（**歯が実 binary を撃つ口 (ii)**・設計 gate-cost.md §30 約束 2）。
///
/// 道具箱（偽 `systemd-run` と偽 `systemctl`）は**呼び手が既に持つ fixture の dir** `place` の下に置き、
/// PATH の先頭に積む。`place` に plugin の root を渡さない——root の配下の dir は `--plugin-dir` として
/// 1 つずつ claude へ渡るので、道具箱の dir が plugin に化ける（設計 §30.1 の errata）。
// flip-check: retroactive s2-07l.504
fn bin_cmd_with_toolbox(place: &Path) -> Command {
    let mut cmd = Command::new(bin());
    cmd.env("PATH", crate::toolbox_path(place))
        .env("CLAUDE_CODE_DISABLE_AGENT_VIEW", INHERITED_AGENT_VIEW)
        .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", INHERITED_AUTO_MEMORY)
        .env("TMUX_PANE", PARENT_PANE);
    cmd
}

/// binary を 1 回撃つ。stdin には `input` を流す（親の agent view の env は [`INHERITED_AGENT_VIEW`]・
/// 親の pane は [`PARENT_PANE`]・道具箱の置き場は `place`）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_bin(place: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = bin_cmd_with_toolbox(place)
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

/// `lens.model` の行（`--stage` の無い lens が読む・値は文字列 `value`・発効・設計 pipeline.md §61）。
fn lens_model_row(value: &str) -> String {
    format!("id = \"lens.model\"\nkind = \"LensModel\"\nvalue = \"{value}\"\nenabled = true\n")
}

/// `pipe.precheck_lens_model` の行（`--stage prelens` の lens が読む・値は文字列 `value`・発効）。
fn prelens_model_row(value: &str) -> String {
    format!("id = \"pipe.precheck_lens_model\"\nkind = \"PipePrecheckLensModel\"\nvalue = \"{value}\"\nenabled = true\n")
}

/// 埋め込み manifest と同じ `runner.model` の値（claude CLI の別名・裁定 id `user 2026-09-29T07:44Z`）。
const RUNNER_MODEL: &str = "sonnet";

/// 埋め込み manifest と同じ `lens.model` の値（同じ裁定）。
const LENS_MODEL: &str = "opus";

/// `runner.effort` の行（値は文字列 `value`・発効・`s2-07l.322`）。
fn effort_row(value: &str) -> String {
    format!("id = \"runner.effort\"\nkind = \"RunnerEffort\"\nvalue = \"{value}\"\nenabled = true\n")
}

/// 埋め込み manifest と同じ `runner.effort` の値（claude CLI の字面・裁定 id `user 2026-09-15T03:52Z`）。
const RUNNER_EFFORT: &str = "high";

/// `gate.token_cap` を `cap` byte にした manifest（file 名に値を含む＝同じ dir で cap を変えて撃ち直せる）。
/// `runner.model` / `lens.model` / `runner.effort` は埋め込みと同じ値で載せる（lens は cap と model と effort の 3 行を
/// 同じ manifest から読む）。
fn rules_with_cap(dir: &Path, cap: u64) -> PathBuf {
    rules_with_rows(
        dir,
        &format!("rules-cap-{cap}.toml"),
        &[cap_row(cap), model_row(RUNNER_MODEL), lens_model_row(LENS_MODEL), effort_row(RUNNER_EFFORT)],
    )
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
        dir,
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
///
/// 道具箱の置き場は **worktree**（plugin root ではない・設計 gate-cost.md §30.1）。
fn run_runner_in(call: &RunnerCall<'_>, root: &Path, input: &[u8]) -> Output {
    let args = runner_args(call, root);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_bin(call.worktree, &refs, input)
}

/// runner を `--rules` の manifest つきで 1 回撃つ（plugin root = [`RunnerCall::dir`]・model の行の歯）。
fn run_runner_with_rules(call: &RunnerCall<'_>, rules: &Path, input: &[u8]) -> Output {
    plugin_leaf(call.dir);
    let mut args = runner_args(call, call.dir);
    args.extend(["--rules".to_owned(), rules.display().to_string()]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_bin(call.worktree, &refs, input)
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
fn run_bin_owned(place: &Path, args: &[String], input: &[u8]) -> Output {
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_bin(place, &borrowed, input)
}

/// (8) headless の口: runner と lens の分岐の直後の閉包の検査が**未知の flag** を rc 2 で断り（理由の 1 行が flag を名指し
/// usage を添える・claude を 1 度も起こさない）、`--help` は usage を stdout へ出して rc 0（設計 pipeline.md §14 約束 3 / 4 / 8）。
/// base の runner は未知の flag を読み飛ばして claude を起こす＝RED。
#[test]
fn headless_args_unknown_flag_is_refused_with_rc_2_on_runner_and_lens() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let call = RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None };
    let contract = contract_in(&dir);
    let mouths = [
        ("runner", runner_args(&call, &dir), vessel::headless::runner::usage()),
        ("lens", lens_args(&contract, &worktree, &[], &claude), vessel::headless::lens::usage()),
    ];
    for (mouth, args, usage) in mouths {
        for (extra, rc, out_want, err_want) in [
            (&["--bogus", "x"][..], RC_BROKEN, String::new(), format!("{mouth}: 未知の引数 --bogus\n{usage}\n")),
            (&["--help"][..], RC_OK, format!("{usage}\n"), String::new()),
        ] {
            let mut all = args.clone();
            all.extend(extra.iter().map(|item| (*item).to_owned()));
            let out = run_bin_owned(&worktree, &all, b"goal = \"x\"\n");
            assert_eq!(out.status.code(), Some(i32::from(rc)), "{mouth} {extra:?}: {}", stderr_of(&out));
            assert_eq!(stdout_of(&out), out_want, "{mouth} {extra:?}: stdout");
            assert_eq!(stderr_of(&out), err_want, "{mouth} {extra:?}: stderr");
            assert!(!dir.join("called").exists(), "{mouth} {extra:?}: claude を 1 度も起動しない");
        }
    }
    clean(&[&dir, &worktree]);
}

/// runner / lens が claude へ渡した argv の `flag` の対の値（値つき・fake が写した argv）。
fn arg_value(dir: &Path, flag: &str) -> Option<String> {
    let args = slurp(&dir.join("args"));
    let lines: Vec<&str> = args.lines().collect();
    lines
        .windows(2)
        .find(|w| w.first() == Some(&flag))
        .and_then(|w| w.get(1).map(|value| (*value).to_owned()))
}

/// runner が claude へ渡す argv に `--model` の対が在るか（値つき・fake が写した argv）。
fn model_arg(dir: &Path) -> Option<String> {
    arg_value(dir, "--model")
}

/// 同じく `--effort` の対（rules 行 `runner.effort`・`s2-07l.322`）。
fn effort_arg(dir: &Path) -> Option<String> {
    arg_value(dir, "--effort")
}

/// (c) `runner.effort` の行が解けない周（無い / 不発効 / 文字列でない）は runner が **claude を呼ばず rc 2** で理由を
/// 1 行（`runner: runner.effort …`・model と同じ極性）。読む順は model → effort＝両方欠けた manifest では model の
/// 理由だけが出る（既存の歯の字面は不変）。base は行が無くても claude を起こす（argv の写しが生成される）ので RED。
/// 名の接頭辞 `headless_effort_row_refuses_` は新歯だけに当たる（`headless_runner_refuses_` は既存の歯 3 本に当たり
/// base が rc 4 にならない・planner 2026-09-16 10:4xZ）。
// 名の改めは run 1（5279b90・main に在る）が land した挙動の歯を後から動かす便＝base で緑になる。逃がしは下の 1 行で
// 明示し、非空虚性は run 1 の RED-on-base（効く歯 4 本）が示す。
// flip-check: retroactive s2-07l.322
#[test]
fn headless_effort_row_refuses_when_missing() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let call = RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None };
    let absent = rules_with_rows(&dir, "effort-absent.toml", &[model_row(RUNNER_MODEL)]);
    let disabled = rules_with_rows(
        &dir,
        "effort-disabled.toml",
        &[model_row(RUNNER_MODEL), "id = \"runner.effort\"\nkind = \"RunnerEffort\"\nvalue = \"high\"\nenabled = false\n".to_owned()],
    );
    // id は同じで kind が整数の行（manifest は id と kind の対応を照合しない）＝値が文字列でない形。
    let int = rules_with_rows(
        &dir,
        "effort-int.toml",
        &[model_row(RUNNER_MODEL), "id = \"runner.effort\"\nkind = \"GateTokenCap\"\nvalue = 5\nenabled = true\n".to_owned()],
    );
    // model も effort も無い manifest（順は model が先）。
    let neither = rules_with_rows(&dir, "effort-neither.toml", &[cap_row(4096)]);
    for (rules, want) in [
        (&absent, "runner.effort が無い"),
        (&disabled, "runner.effort は不発効である"),
        (&int, "runner.effort が文字列でない"),
        (&neither, "runner.model が無い"),
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
    clean(&[&dir, &worktree]);
}

/// (d) 閉じた表に無い `runner.effort` の値（`max` / `High` / 空）は runner が claude を呼ばず rc 2 で、理由に行 id と
/// 値と**取る 4 つの字面**を名指す（case-fold しない＝`High` も未知）。lens も同じ極性。base は行を読まないので RED。
#[test]
fn headless_effort_row_refuses_unknown_value() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let call = RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None };
    for value in ["max", "High", ""] {
        let rules = rules_with_rows(&dir, &format!("bad-effort-{value}.toml"), &[model_row(RUNNER_MODEL), effort_row(value)]);
        let out = run_runner_with_rules(&call, &rules, b"goal = \"x\"\n");
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{value}: rc 2 / {}", stderr_of(&out));
        assert!(!dir.join("called").exists(), "{value}: claude を 1 度も起動しない");
        assert!(!dir.join("args").exists(), "{value}: argv の写しは生成されない");
        let err = stderr_of(&out);
        assert!(err.contains(&format!("runner: runner.effort の値 {value} は未知の effort")), "{value}: 行 id と値: {err}");
        assert!(err.contains("low / medium / high / xhigh"), "{value}: 取る字面を全部名指す: {err}");
        assert_eq!(err.lines().count(), 1, "{value}: stderr は理由の 1 行だけ: {err}");
    }
    // lens も同じ極性（cap と model は解ける manifest で effort だけが表に無い）。
    let contract = contract_in(&dir);
    let lens_claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let rules = rules_with_rows(&dir, "lens-bad-effort.toml", &[cap_row(4096), lens_model_row(LENS_MODEL), effort_row("max")]);
    let out = run_bin_owned(&dir, &lens_args(&contract, &dir, &["--rules", &rules.display().to_string()], &lens_claude), b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "lens: rc 2 / {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "lens: claude を 1 度も起動しない");
    assert!(stderr_of(&out).contains("lens: runner.effort の値 max は未知の effort"), "lens: {}", stderr_of(&out));
    assert!(stdout_of(&out).is_empty(), "lens: 判定の面には何も出さない: {}", stdout_of(&out));
    clean(&[&dir, &worktree]);
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

/// fake が写した claude の env の `key` の値（写しに行が無ければ `None`）。
fn env_value(dir: &Path, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    slurp(&dir.join("env")).lines().find_map(|line| line.strip_prefix(&prefix)).map(str::to_owned)
}

/// (d) runner と lens の子は口座の自動 memory を読まずに起きる（設計 pipeline.md §64 形 2・agent view の歯と同じ形）: `build` が子の
/// env の `CLAUDE_CODE_DISABLE_AUTO_MEMORY` を 1 に設定し、親の値（[`INHERITED_AUTO_MEMORY`]）を継承させない。`--account-dir` を
/// 渡さない runner の周と、渡す lens の周のどちらも同じ。base は設定せず親の値がそのまま届く（RED）。
#[test]
fn lens_read_auto_memory_env_is_one_for_runner_and_lens_and_never_inherited() {
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
    assert_eq!(env_value(&dir, "CLAUDE_CODE_DISABLE_AUTO_MEMORY").as_deref(), Some("1"), "runner の子で自動 memory を切る");

    // lens の周の写しを runner の周の残りと取り違えない（呼ばれた印ごと消してから撃つ）。
    fs::remove_file(dir.join("env")).expect("runner の周の写しを消せる");
    fs::remove_file(dir.join("called")).expect("runner の周の印を消せる");
    let verdict = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"x\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &verdict, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "cap 内なので lens も claude を呼ぶ");
    assert_eq!(env_value(&dir, "CLAUDE_CODE_DISABLE_AUTO_MEMORY").as_deref(), Some("1"), "lens の子で自動 memory を切る");
    clean(&[&dir, &worktree]);
}

/// (e) runner の argv は今のまま: `--tools` を 1 本も持たず、`--allowedTools` の allowlist と渡した permission mode（acceptEdits）を
/// そのまま持つ（道具の列を持つのは lens だけ・設計 pipeline.md §64 形 1）。
#[test]
fn lens_read_runner_argv_has_no_tools_flag_and_keeps_allowlist_and_mode() {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let args = slurp(&dir.join("args"));
    assert!(!has_arg(&args, "--tools"), "runner は道具の列を渡さない: {args}");
    assert!(has_arg(&args, "--allowedTools"), "allowlist は今のまま: {args}");
    assert!(pair(&args, "--permission-mode", "acceptEdits"), "渡した permission mode は今のまま: {args}");
    clean(&[&dir, &worktree]);
}

/// fake が写した claude の env に起動側の [`PARENT_PANE`] が**無い**。
///
/// 母集団を先に測る——写しが空なら「無い」は空虚に通るので、行数 > 0 と親から継承した `PATH` を見る。
fn assert_pane_dropped(dir: &Path, who: &str) {
    let text = slurp(&dir.join("env"));
    let keys: Vec<&str> = text.lines().filter_map(|line| line.split_once('=')).map(|(key, _)| key).collect();
    assert!(!keys.is_empty(), "{who}: claude の env の写しが空（母集団 0）");
    assert!(keys.contains(&"PATH"), "{who}: 写しは親の env を継承している（{} 行）", keys.len());
    assert!(!keys.contains(&"TMUX_PANE"), "{who}: TMUX_PANE を継承しない: {keys:?}");
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

/// 契約の審査の prompt の外形（契約と材料の fixture を固定・C12.5）。
#[test]
fn headless_lens_contract_prompt_external_form() {
    let prompt = lens_contract_prompt_of_fixed_fixture();
    insta::assert_snapshot!("lens_contract_prompt_external_form", prompt);
}

// ───── Promised の行の審査の雛形（`s2-07l.513`・設計 contract-source.md §33 行 ah・接頭辞 `headless_lens_promise_`） ─────

/// 約束の行の写し（`pipe::review` が `promises.txt` に置く形・★契約・材料・雛形のどれにも現れない字面）。
const PROMISE_ROWS: &str = "- n: 1\n  text: 約束の本文 PROMISE-TEXT-MARK\n  fixture: 歯の fixture PROMISE-FIXTURE-MARK\n  expect: 歯の観測 PROMISE-EXPECT-MARK\n- n: 2\n  text: 二つ目の約束\n  fixture: 二つ目の fixture\n  expect: 二つ目の観測";

/// 契約の審査の lens を約束の行の写し付きで 1 回撃ち、claude の stdin に渡った prompt を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn lens_promise_prompt_of_fixed_fixture() -> String {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"fake\"}\n", false, 0);
    let contract = contract_in(&dir);
    material_in(&dir, Some(CONTRACT_DESIGN), Some(CONTRACT_REQUIREMENTS));
    fs::write(dir.join("promises.txt"), format!("{PROMISE_ROWS}\n")).expect("写しを書ける");
    let out = run_lens(&contract, 4096, "plan", &claude, CONTRACT_STDIN);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    clean(&[&dir]);
    prompt
}

/// Promised の行の契約の審査の prompt の外形（契約と材料と約束の行の写しの fixture を固定・C12.5）。
#[test]
fn headless_lens_promise_prompt_external_form() {
    let prompt = lens_promise_prompt_of_fixed_fixture();
    insta::assert_snapshot!("lens_promise_prompt_external_form", prompt);
}

/// (c) 契約の審査の雛形・約束の行つきの雛形・gate の雛形（diff の審査）で組んだ prompt は「読みの道具（Read・Grep・Glob）」を持ち、
/// 「tool が渡されていない」を 1 件も持たない（設計 pipeline.md §64 形 4）。base の雛形は 1 行目を持たず 2 行目を 1 件ずつ持つ。
#[test]
fn lens_read_prompts_name_the_read_tools_and_say_no_tool_is_missing() {
    let prompts = [
        ("gate の雛形", lens_prompt_of_fixed_fixture()),
        ("契約の審査の雛形", lens_contract_prompt_of_fixed_fixture()),
        ("約束の行つきの雛形", lens_promise_prompt_of_fixed_fixture()),
    ];
    for (label, prompt) in prompts {
        assert!(!prompt.is_empty(), "{label}: prompt の写しが空（母集団 0）");
        assert_eq!(prompt.matches("読みの道具（Read・Grep・Glob）").count(), 1, "{label}: 読みの道具を名乗る: {prompt}");
        assert_eq!(prompt.matches("tool が渡されていない").count(), 0, "{label}: 「tool が渡されていない」は消える: {prompt}");
        assert_eq!(prompt.matches("読むだけで決める").count(), 0, "{label}: 「読むだけで決める」は消える: {prompt}");
    }
}

/// lens の prompt に載る裁定の節の見出し（`s2-07l.309`）。
///
/// ★契約 fixture にも diff fixture にも裁定 fixture にも現れない字面（節を消せば回数が 0 に落ちる）。
const LENS_RULINGS_HEADING: &str = "## 契約への裁定（便の質問への回答・逐語）";

/// 裁定の file の本文（gate が書く形・1 対 = 3 行）。`{diff}` の字面を持たせ、穴が同じ 1 走査で埋まること
/// （裁定の中の穴は展開されない）も測る。
const RULINGS_FIXTURE: &str = "question: verify 行が矛盾する {diff}\nabout: verify\nanswer: verify は 1 行目だけを撃つ\n";

#[test]
fn headless_external_form() {
    let args = |raw: &[&str]| raw.iter().map(|item| (*item).to_owned()).collect::<Vec<String>>();
    let mut lines = vec![vessel::headless::runner::usage(), vessel::headless::lens::usage()];
    lines.extend(vessel::headless::runner::dispatch(&args(&[])).err);
    lines.extend(vessel::headless::lens::dispatch(&args(&[])).err);
    let form = lines.join("\n");
    insta::assert_snapshot!(form);
}

/// **API に届かず止まった周**（`s2-07l.301`・設計 account-autonomy.md §17 (1)）: 偽 claude が `is_error:true` と到達不能の
/// 本文の `result` を書いて rc 1 で終わると、runner は rc [`RC_UNREACHABLE`] で終わり stdout の最終行に停止行を出す。
/// 同じ本文で `is_error:false` の周と、集合に無い error の本文の周は claude の rc（1）を写す（従来どおり）。
#[test]
fn pipe_unreachable_runner_exits_with_its_rc_only_on_unreachable_error_results() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let text = "API Error: Can't reach the API server (EAI_AGAIN)";
    let record = |is_error: bool, body: &str| {
        format!("{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":{is_error},\"result\":\"{body}\"}}\n")
    };
    let run = |body: &str| {
        let claude = fake_claude(&dir, body, false, 1);
        run_runner(
            &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
            b"goal = \"x\"\n",
        )
    };
    let halted = run(&record(true, text));
    let copied = run(&record(false, text));
    let other = run(&record(true, "API Error: 500 Internal server error"));
    clean(&[&dir, &worktree]);
    assert_eq!(halted.status.code(), Some(i32::from(RC_UNREACHABLE)), "{}", stderr_of(&halted));
    assert_eq!(
        stdout_of(&halted).lines().last(),
        Some(format!("runner: halt reason=unreachable text={text}").as_str()),
        "停止行は stdout の最終行: {}",
        stdout_of(&halted)
    );
    assert_eq!(copied.status.code(), Some(1), "is_error=false は弁別しない: {}", stdout_of(&copied));
    assert_eq!(other.status.code(), Some(1), "集合に無い error は claude の rc: {}", stdout_of(&other));
    assert!(!stdout_of(&other).contains("reason=unreachable"), "停止行を出さない: {}", stdout_of(&other));
}

// 既に land した起動形（`build` 1 つ・ADR-0011 §2.1 / §2.2）へ後から足す**不在**の歯 2 本。
// 実装は 1 byte も触らないので base で緑になる＝逃がしは下の 1 行で明示し、非空虚性は
// 変異 2 本（`build` に `--mcp-config` を足す / lens の呼出側に `--allowedTools` を足す）で示す。
// flip-check: retroactive s2-07l.72

/// 雛形が運ぶ **turn の終端の規律**の逐語（正本は設計 pipeline.md §20 の約束 1・`s2-07l.275`）。
const TURN_DISCIPLINE: &str =
    "検証は前面で完走させてから turn を閉じる（背景実行を残して終えない・残した task は片付けで止められ done に数えない）";

/// runner の prompt の外形（契約 / write-set / 写しの fixture を固定・C12.5・`s2-07l.176`・設計 §6）。
///
/// 上の 2 本は断片（allowlist の節・穴の不展開）を見る歯で、本文の他の節の改変は素通しする
/// （部分集合の罠・全体監査 2026-09-12 塊 23）。**全文**を pin し、prompt の 1 字の変更を `.snap` の
/// 差分として PR に出す（隣の `headless_lens_prompt_external_form` と同じ型）。
#[test]
fn headless_runner_prompt_external_form() {
    let dir = tmp();
    let worktree = tmp();
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, format!("{CONTRACT_WRITE_SET}\n{CONTRACT_WRITE_SET_2}\n")).expect("write-set を書ける");
    let claude = fake_claude(&dir, "", false, 0);
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "acceptEdits", account: None },
        contract_text(CONTRACT_GOAL).as_bytes(),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let prompt = slurp(&dir.join("stdin"));
    clean(&[&dir, &worktree]);
    insta::assert_snapshot!(prompt);
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

// ── 質問 record の置き場の判定は top-level の key で見る（s2-07l.123・.116 実 run の現物） ──

/// 実 claude 2.1.268 の `result` record の形（.116 の raw stream から必要最小へ写した）: `usage.iterations[]`
/// の入れ子に `"type":"message"` が在り、top-level の `"type":"result"` はその**後ろ**に来る。
const REAL_RESULT_RECORD_HEAD: &str = r#"{"duration_api_ms":15819,"stop_reason":"end_turn","usage":{"input_tokens":4,"iterations":[{"input_tokens":2,"output_tokens":387,"type":"message"}]},"modelUsage":{"m":{"inputTokens":4,"outputTokens":724}},"permission_denials":[],"is_error":false,"num_turns":3,"subtype":"success","api_error_status":null,"result":""#;
const REAL_RESULT_RECORD_TAIL: &str = r#"","ttft_ms":3865,"type":"result","duration_ms":14899,"uuid":"2f7f195c","result_index":0}"#;

/// 実 record の形に `result` の text（escape 済み）を挟む。
fn real_result_record(result_json_text: &str) -> String {
    format!("{REAL_RESULT_RECORD_HEAD}{result_json_text}{REAL_RESULT_RECORD_TAIL}")
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

// ---- claude の scope の peak を走行中に sample する（設計 gate-cost.md §13・s2-07l.273）------------------

/// 偽 `systemctl show` が返す fixture の ControlGroup（実 systemd と同じ `/` 始まりの絶対形＝root を捨てない
/// 側の材料）。unit 名は pid と通し番号で動くので、歯が**起動前に** `memory.peak` を置ける固定の名にする。
const PEAK_CONTROL_GROUP: &str = "/user.slice/scribe2-peak-fixture.scope";

/// 歯が起動前に `memory.peak` へ書く値（planner の実測 5006 MB に寄せた byte・0 でも 1 でもない）。
const PEAK_BYTES: &str = "5006000000";

/// 偽 `systemctl show` の答え: fixture の ControlGroup を 1 行（rc 0）。
const SHOW_FOUND: &str = "printf '%s\\n' '/user.slice/scribe2-peak-fixture.scope'\nexit 0";

/// 偽 `systemctl show` の答え: rc 1 で空（解けない周）。
const SHOW_FAILED: &str = "exit 1";

/// 偽 `systemctl kill` の答え: 殺した（rc 0 = `killed`）。
const KILL_KILLED: &str = "exit 0";

/// 偽 `systemctl kill` の答え: unit が無い（実 systemctl の字面・rc 1 = `gone`＝最後の process の終了で消えた正常系）。
const KILL_GONE: &str = "printf 'Failed to kill unit %s: Unit %s not loaded.\\n' \"$5\" \"$5\" >&2\nexit 1";

/// 偽 `systemctl` が撃たれた argv（1 呼出 1 行）を追記する file 名。
const PEAK_CALLS: &str = "systemctl-calls";

/// stream の init record（runner の fake が 1 行目に出す）。
const INIT_RECORD: &str = r#"{"type":"system","subtype":"init"}"#;

/// stream の result record（runner の fake が終端に出す）。
const RESULT_RECORD: &str = r#"{"type":"result","subtype":"success","is_error":false,"result":"done"}"#;

/// lens の fake が終端に出す verdict。
const PEAK_VERDICT: &str = r#"{"verdict":"PASS","evidence":"peak の歯"}"#;

/// 偽 `systemd-run` の argv を写す記録 dir 名（**1 起動 1 file**）。
const PEAK_SCOPE_RECORDS: &str = "shim-scope-args";

/// 偽 `systemd-run`（本体は `crate::write_systemd_run_stub` の 1 つの生成関数から出る・設計 gate-cost.md
/// §30 約束 3）と偽 `systemctl`（`show` は `show`・`kill` は `kill` の答え・argv は [`PEAK_CALLS`] へ写す）を
/// `dir/shim-bin` に置き、それを先頭にした PATH を返す。
///
/// **偽 `systemctl` の答えが歯ごとに変わる形はこの口のまま**である（本行が揃えるのは `systemd-run` の側だけ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn peak_shims(dir: &Path, show: &str, kill: &str) -> String {
    let bin_dir = dir.join("shim-bin");
    crate::write_systemd_run_stub(&bin_dir, &dir.join(PEAK_SCOPE_RECORDS));
    let systemctl = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$2\" in\nshow) {show};;\nkill) {kill};;\nesac\nexit 1\n",
        dir.join(PEAK_CALLS).display()
    );
    let shim = bin_dir.join("systemctl");
    fs::write(&shim, systemctl).expect("shim を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("shim に実行権を付ける");
    format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default())
}

/// `<root>/<ControlGroup>/memory.peak` に [`PEAK_BYTES`] を**起動前に**書き、その scope の dir を返す
/// （fake claude が終端の前に `rm -r` する相手）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn peak_fixture(root: &Path) -> PathBuf {
    let scope = root.join(PEAK_CONTROL_GROUP.trim_start_matches('/'));
    fs::create_dir_all(&scope).expect("fixture の cgroup dir を作れる");
    fs::write(scope.join("memory.peak"), format!("{PEAK_BYTES}\n")).expect("memory.peak を書ける");
    scope
}

/// 偽 claude（[`fake_claude`] の形を写した歯内の script）: `called` を残してから `body` を撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn peak_claude(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("fake-claude");
    fs::write(&path, format!("#!/bin/sh\n: > \"{}/called\"\n{body}", dir.display())).expect("fake を書ける");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("fake を実行可能にできる");
    path
}

/// binary を PATH を差し替えて起こす（stdin に `input` を流す・env の残りは [`run_bin`] と同じ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn spawn_bin(args: &[String], input: &[u8], path: &str) -> Child {
    let mut child = Command::new(bin())
        .args(args)
        .env("PATH", path)
        .env("CLAUDE_CODE_DISABLE_AGENT_VIEW", INHERITED_AGENT_VIEW)
        .env("TMUX_PANE", PARENT_PANE)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary を起動できる");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input);
    }
    child
}

/// [`run_bin`] の PATH 差し替え変種。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_bin_with_path(args: &[String], input: &[u8], path: &str) -> Output {
    spawn_bin(args, input, path).wait_with_output().expect("binary の出力を読める")
}

/// `limit` の上限つきで binary の終了を待つ。超えたら kill して落とす（poll 中に stdout を読まない実装は
/// 子の pipe が詰まって永久に回る＝上限で RED）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn wait_bounded(mut child: Child, limit: Duration) -> Output {
    let started = Instant::now();
    while child.try_wait().expect("binary の状態を読める").is_none() {
        let timed_out = started.elapsed() > limit;
        if timed_out {
            let _ = child.kill();
            let _ = child.wait();
        }
        assert!(!timed_out, "{limit:?} 内に終わらない（poll 中に stdout を読み切らず pipe が詰まった形）");
        std::thread::sleep(Duration::from_millis(100));
    }
    child.wait_with_output().expect("binary の出力を読める")
}

/// runner を `--cgroup-root root` つきで撃つ（write-set / vessel / plugin leaf は `dir` に置く）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_runner_peak(dir: &Path, worktree: &Path, claude: &Path, root: &Path, path: &str) -> Output {
    let write_set = dir.join("write-set.txt");
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let vessel = write_vessel_copy(dir, r#"["cargo"]"#);
    plugin_leaf(dir);
    let call = RunnerCall { dir, worktree, write_set: &write_set, vessel: &vessel, claude, mode: "plan", account: None };
    let mut args = runner_args(&call, dir);
    args.extend(["--cgroup-root".to_owned(), root.display().to_string()]);
    run_bin_with_path(&args, b"goal = \"x\"\n", path)
}

/// lens の argv（`--rules` の cap と `--cgroup-root root` つき・契約は `dir` に置く）。
fn lens_peak_args(dir: &Path, claude: &Path, root: &Path) -> Vec<String> {
    let contract = contract_in(dir);
    let rules = rules_with_cap(dir, 4096);
    let extra = ["--rules", &rules.display().to_string(), "--cgroup-root", &root.display().to_string()];
    lens_args(&contract, dir, &extra, claude)
}

/// stderr の `<who>: scope=` で始まる行（母集団ごと返す）。
fn scope_lines<'a>(err: &'a str, who: &str) -> Vec<&'a str> {
    let head = format!("{who}: scope=");
    err.lines().filter(|line| line.starts_with(&head)).collect()
}

/// (a) runner: scope の peak は**走行中の sample**で残る。偽 claude は stream の record を 1 行出し → `sleep 3` →
/// fixture の cgroup dir を `rm -r`（最後の process の終了で scope が消えることを模す）→ result record → rc 0。
/// 偽 `kill` は「not loaded」（`gone`）。期待: `runner: scope=gone` で始まり ` claude_peak_bytes=5006000000` を
/// 含む行が**ちょうど 1 本**・rc 0・stdout の最終行は現物と同じ。終端で読む実装は dir が無く `-`・`Gone` を
/// `None` に落とす実装は行そのものが無い→どちらも RED。`show` の argv も pin する（unit 名は `systemd-run` が
/// 受けた claude の scope・`-p ControlGroup --value`）。
#[test]
fn headless_claude_peak_is_sampled_before_the_scope_vanishes() {
    let dir = tmp();
    let worktree = tmp();
    let root = tmp();
    let path = peak_shims(&dir, SHOW_FOUND, KILL_GONE);
    let scope = peak_fixture(&root);
    let body = format!(
        "printf '%s\\n' '{INIT_RECORD}'\nsleep 3\nrm -r '{}'\nprintf '%s\\n' '{RESULT_RECORD}'\nexit 0\n",
        scope.display()
    );
    let claude = peak_claude(&dir, &body);
    let out = run_runner_peak(&dir, &worktree, &claude, &root, &path);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(!scope.exists(), "fake は終端の前に scope の dir を消している（母集団）");
    let err = stderr_of(&out);
    let lines = scope_lines(&err, "runner");
    assert_eq!(lines.len(), 1, "scope= 行はちょうど 1 本: {err}");
    let line = lines.first().copied().unwrap_or_default();
    assert!(line.starts_with("runner: scope=gone"), "gone の周も行を出す: {line}");
    assert!(line.contains(&format!(" claude_peak_bytes={PEAK_BYTES}")), "走行中に読んだ値が残る: {line}");
    // 孤児の数は fixture に `cgroup.procs` を置かないので `-`（0 と融合しない・設計 pipeline.md §20）。
    assert!(line.ends_with(" orphans=-"), "数えられない周は - で終わる: {line}");
    let stdout = stdout_of(&out);
    assert_eq!(stdout.lines().last(), Some("runner: rc=0 records=2"), "stdout の最終行は不変: {stdout}");
    let calls = slurp(&dir.join(PEAK_CALLS));
    let shown: Vec<&str> = calls.lines().filter(|call| call.starts_with("--user show ")).collect();
    // **2 回**（走行中の sample が 1 回だけ解く + 終端で孤児を数える前に 1 回）。stream の record は 2 本
    // なので、周ごとに解き直す実装は 3 回以上になる＝この数は「周期で撃たない」を保ったままである。
    assert_eq!(shown.len(), 2, "show は sample の 1 回と終端の 1 回: {calls}");
    for show in &shown {
        assert!(show.starts_with("--user show scribe2-runner-claude-"), "claude の scope の unit を引く: {show}");
        assert!(show.ends_with(".scope -p ControlGroup --value"), "ControlGroup の値だけを引く: {show}");
    }
    clean(&[&dir, &worktree, &root]);
}

/// (b) lens: `wait_with_output` を poll に替え、各周で sample する。偽 claude は `sleep 3` → dir を `rm -r` →
/// verdict 1 行で rc 0。期待: `lens: scope=gone claude_peak_bytes=5006000000` の行が 1 本・stdout の最終行は
/// verdict・rc 0。終端で読む実装は `-` → RED。
#[test]
fn headless_claude_peak_lens_samples_by_poll() {
    let dir = tmp();
    let root = tmp();
    let path = peak_shims(&dir, SHOW_FOUND, KILL_GONE);
    let scope = peak_fixture(&root);
    let body = format!("sleep 3\nrm -r '{}'\nprintf '%s\\n' '{PEAK_VERDICT}'\nexit 0\n", scope.display());
    let claude = peak_claude(&dir, &body);
    let out = run_bin_with_path(&lens_peak_args(&dir, &claude, &root), b"--- a\n+++ b\n", &path);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(dir.join("called").exists(), "lens は claude を呼ぶ（母集団）");
    let err = stderr_of(&out);
    let want = format!("lens: scope=gone claude_peak_bytes={PEAK_BYTES} orphans=-");
    assert_eq!(scope_lines(&err, "lens"), vec![want.as_str()], "{err}");
    let stdout = stdout_of(&out);
    assert_eq!(stdout.lines().last(), Some(PEAK_VERDICT), "stdout の最終行は verdict: {stdout}");
    clean(&[&dir, &root]);
}

/// (c) runner・読めない周は `-`（0 でない・C10）: ① `memory.peak` を置かない ② 偽 `show` が rc 1 で空。
/// 偽 `kill` は rc 0（`killed`）。
#[test]
fn headless_claude_peak_unreadable_is_dash() {
    let body = format!("printf '%s\\n' '{RESULT_RECORD}'\nexit 0\n");
    for (show, with_fixture) in [(SHOW_FOUND, false), (SHOW_FAILED, true)] {
        let dir = tmp();
        let worktree = tmp();
        let root = tmp();
        let path = peak_shims(&dir, show, KILL_KILLED);
        if with_fixture {
            peak_fixture(&root);
        }
        let claude = peak_claude(&dir, &body);
        let out = run_runner_peak(&dir, &worktree, &claude, &root, &path);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
        let err = stderr_of(&out);
        assert_eq!(
            scope_lines(&err, "runner"),
            vec!["runner: scope=killed claude_peak_bytes=- orphans=-"],
            "fixture={with_fixture} show={show:?}: {err}"
        );
        clean(&[&dir, &worktree, &root]);
    }
}

/// (d) runner・systemd 無しの host（PATH = 空 dir 1 つ・builtin だけの偽 claude）: 行に `claude_peak_bytes` も
/// `scope=` も出ない。母集団を先に測る（rc 0 ∧ stdout の行数 > 0 ∧ `called` の印）。
#[test]
fn headless_claude_peak_absent_without_systemd() {
    let dir = tmp();
    let worktree = tmp();
    let root = tmp();
    let empty = tmp();
    let claude = peak_claude(&dir, &format!("printf '%s\\n' '{RESULT_RECORD}'\nexit 0\n"));
    let out = run_runner_peak(&dir, &worktree, &claude, &root, &empty.display().to_string());
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.lines().count() > 0, "stdout の行が在る（母集団）");
    assert!(dir.join("called").exists(), "claude は呼ばれた（母集団）");
    let err = stderr_of(&out);
    assert!(!err.contains("claude_peak_bytes"), "systemd 無しでは語を出さない: {err}");
    assert!(!err.contains("scope="), "scope= も出ない（現物と同じ）: {err}");
    clean(&[&dir, &worktree, &root, &empty]);
}

/// (e) lens・poll の間も stdout を読み切る: 偽 claude が 256 KiB の filler を出してから verdict（sleep 無し・
/// 偽 `kill` rc 0）。30 秒の上限つきで待ち、超えたら binary を kill して落とす。期待: 上限内に rc 0 ∧ 最終行が
/// verdict ∧ `lens: scope=killed claude_peak_bytes=` の語。
#[test]
fn headless_claude_peak_lens_drains_stdout_while_polling() {
    let dir = tmp();
    let root = tmp();
    let path = peak_shims(&dir, SHOW_FOUND, KILL_KILLED);
    peak_fixture(&root);
    let body = format!("head -c 262144 /dev/zero | tr '\\0' x\nprintf '\\n%s\\n' '{PEAK_VERDICT}'\nexit 0\n");
    let claude = peak_claude(&dir, &body);
    let child = spawn_bin(&lens_peak_args(&dir, &claude, &root), b"--- a\n+++ b\n", &path);
    let out = wait_bounded(child, Duration::from_secs(30));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert_eq!(stdout.lines().last(), Some(PEAK_VERDICT), "filler の後の verdict を読む: {stdout}");
    let err = stderr_of(&out);
    assert!(err.contains("lens: scope=killed claude_peak_bytes="), "行は killed で出る: {err}");
    clean(&[&dir, &root]);
}

// ───── e2e の歯の道具箱（設計 gate-cost.md §30・行 v・`s2-07l.504`・接頭辞 `e2e_toolbox_`） ─────

/// (c) 約束 2(ii): [`run_bin`] で lens を 1 回撃つと、呼び手の fixture の dir の下の道具箱の記録 dir に
/// **claude の scope の記録**が残り、その引数に `--scope` と `MemoryMax=` が在る。
///
/// 母集団は記録 dir の全件（`crate::toolbox_record` が「ちょうど 1 件」を要求する）。lens が claude を
/// 呼んだことは `called` の印で先に測る（呼んでいない周の「記録が無い」で空虚に充足しない）。
#[test]
fn e2e_toolbox_run_bin_confines_the_lens_claude() {
    let dir = tmp();
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"道具箱の歯\"}\n", false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "判定は返る: {}", stderr_of(&out));
    assert!(dir.join("called").exists(), "母集団: lens は claude を呼んだ");
    let names = crate::toolbox_record_names(&dir);
    let record = crate::toolbox_record(&dir, "-claude-");
    assert!(record.lines().any(|line| line == "--scope"), "claude も包めた（母集団 {names:?}）: {record}");
    assert!(
        record.lines().any(|line| line.starts_with("MemoryMax=")),
        "箱の大きさを渡している（母集団 {names:?}）: {record}"
    );
    clean(&[&dir]);
}

// ───── runner / lens / claude の箱を 1 × gate.job_memory_mb に揃える（`s2-07l.230`・設計 gate-cost.md §12・行 c・
// 接頭辞 `headless_runner_box_`） ─────

/// runner の雛形の「実行してよい command」節に足した 1 行（検出線は着地の後に器が撃つ・設計 gate-cost.md §44 形 (12)）。
const DETECTOR_LINE: &str = "- 検出線（`cargo mutants`）は着地の後に器が撃つ。runner は撃たない（禁じる語列で止まる）。";

// ───── 同名の flag が 2 つ在る argv は claude を起こさずに断る（`s2-07l.411`・設計 account-autonomy.md §16・
// 接頭辞 `headless_flag_duplicate_`） ─────
//
// 読み手は `headless::flag` の**1 関数**で、runner も lens も・必須（`need`）も任意も同じ経路を通る。
// 最初の出現を採る実装では、器が足した口座の後ろに散文で書かれた値（やその逆）が黙って捨てられ、
// 記帳した口座と実際に走る口座がずれる——どちらが正かは器に分からない（C10）ので断る。

/// 断りの手前まで材料が揃った契約（parse で断るので中身は読まれない＝「材料不足で落ちた」と区別する）。
const DUPLICATE_CONTRACT: &str = "goal = \"縦 1 本を通す\"\nverify = [\"true\"]\n";

/// (a) runner の argv に `--account-dir` が 2 つ在る周は rc 1 で断り、claude を 1 度も起こさない。
/// base は最初の値で claude を起こす（`called` が在り `account` が最初の dir）＝RED。
#[test]
fn headless_flag_duplicate_account_dir_refuses_runner_before_claude() {
    let dir = tmp();
    let worktree = tmp();
    let first = tmp();
    let second = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    plugin_leaf(&dir);
    // **値は別の dir** にする——同じ値だと「2 つ在る」でなく「値が一致する」で通る実装と区別できない。
    let call = RunnerCall {
        dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel,
        claude: &claude, mode: "plan", account: Some(&first),
    };
    let mut args = runner_args(&call, &dir);
    args.push("--account-dir".to_owned());
    args.push(second.display().to_string());
    let out = run_bin_owned(&worktree, &args, DUPLICATE_CONTRACT.as_bytes());
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "同名 2 つは rc 1: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    let err = stderr_of(&out);
    assert!(err.contains("--account-dir"), "何が 2 つ在るかを名乗る: {err}");
    assert!(err.contains(&first.display().to_string()), "1 つ目の値を名乗る: {err}");
    assert!(err.contains(&second.display().to_string()), "2 つ目の値も名乗る: {err}");
    assert!(err.contains("usage:"), "usage も併記する: {err}");
    clean(&[&dir, &worktree, &first, &second]);
}

/// (b) lens も同じ 1 経路で断る（`--account-dir` は `KNOWN_FLAGS` に在るので `unknown_arg` は通す）。
/// base は最初の値で claude を起こす＝RED。
#[test]
fn headless_flag_duplicate_account_dir_refuses_lens_before_claude() {
    let dir = tmp();
    let first = tmp();
    let second = tmp();
    let contract = contract_in(&dir);
    let claude = fake_claude(&dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let one = first.display().to_string();
    let two = second.display().to_string();
    let args = lens_args(&contract, &dir, &["--account-dir", &one, "--account-dir", &two], &claude);
    let out = run_bin_owned(&dir, &args, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "同名 2 つは rc 1: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "claude を 1 度も起動しない");
    let err = stderr_of(&out);
    assert!(err.contains("--account-dir"), "何が 2 つ在るかを名乗る: {err}");
    assert!(err.contains(&one) && err.contains(&two), "両方の値を名乗る: {err}");
    assert!(err.contains("usage:"), "usage も併記する: {err}");
    clean(&[&dir, &first, &second]);
}

/// (d) 必須の flag（`need` → `flag` の経路）でも同じ断りに届く: runner と lens の argv のそれぞれに
/// `--worktree` が 2 つ在る周は、どちらも rc 1・claude を起こさず・両方の dir を名乗る。base は最初の
/// dir で claude を起こす＝RED。`headless_lens_refuses_without_worktree`（**不在**の断り）は不変。
#[test]
fn headless_flag_duplicate_worktree_refuses_runner_and_lens_before_claude() {
    let dir = tmp();
    let worktree = tmp();
    let other = tmp();
    let claude = fake_claude(&dir, "", false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    plugin_leaf(&dir);
    let call = RunnerCall {
        dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel,
        claude: &claude, mode: "plan", account: None,
    };
    let mut args = runner_args(&call, &dir);
    args.push("--worktree".to_owned());
    args.push(other.display().to_string());
    let out = run_bin_owned(&worktree, &args, DUPLICATE_CONTRACT.as_bytes());
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "runner も rc 1: {}", stderr_of(&out));
    assert!(!dir.join("called").exists(), "runner は claude を 1 度も起動しない");
    let err = stderr_of(&out);
    assert!(err.contains("--worktree"), "何が 2 つ在るかを名乗る: {err}");
    assert!(err.contains(&worktree.display().to_string()), "1 つ目の dir を名乗る: {err}");
    assert!(err.contains(&other.display().to_string()), "2 つ目の dir も名乗る: {err}");

    let lens_dir = tmp();
    let contract = contract_in(&lens_dir);
    let lens_claude = fake_claude(&lens_dir, "{\"verdict\":\"PASS\",\"evidence\":\"呼ばれてはならない\"}\n", false, 0);
    let second = other.display().to_string();
    let args = lens_args(&contract, &worktree, &["--worktree", &second], &lens_claude);
    let out = run_bin_owned(&lens_dir, &args, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "lens も rc 1: {}", stderr_of(&out));
    assert!(!lens_dir.join("called").exists(), "lens も claude を 1 度も起動しない");
    let err = stderr_of(&out);
    assert!(err.contains("--worktree"), "何が 2 つ在るかを名乗る: {err}");
    assert!(err.contains(&worktree.display().to_string()) && err.contains(&second), "両方の dir を名乗る: {err}");
    clean(&[&dir, &worktree, &other, &lens_dir]);
}

// ---- claude の消費の 6 値（設計 gate-cost.md §26 形 (2)・接頭辞 `run_cost_`）----
//
// 偽 claude が usage 付きの result record を出し、runner の要約行と lens の判定 object に 6 値が載ることを測る。
// 6 値は互いに違う数（取り違えは字面で落ちる）で、`usage.iterations[]` の中の数（901）に釣られないことも同じ行で見る。

/// text を JSON の文字列 literal にする（`result` の値に判定の JSON 行を埋めるため）。
fn json_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
}

/// 偽 claude の result record（stream-json の 1 行と json の封筒は同じ形）。`turns` は `"num_turns":5,` の対の字面
/// （空で欠く）・`wall` は `duration_ms` の値の字面・`result` は text。
fn cost_record(turns: &str, wall: &str, result: &str) -> String {
    format!(
        "{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,{turns}\"duration_ms\":{wall},\"result\":{},\
         \"usage\":{{\"iterations\":[{{\"input_tokens\":901}}],\"input_tokens\":11,\"cache_creation_input_tokens\":44,\
         \"cache_read_input_tokens\":33,\"output_tokens\":22}},\"total_cost_usd\":0.5}}\n",
        json_string(result)
    )
}

/// 6 値が揃った周に要約行と判定 object へ載る字面（fixture の数そのまま）。
const COST_WORDS: &str = "usage=in:11,out:22,cache_read:33,cache_create:44 turns=5 wall_ms=6000";

/// 偽 lens の判定（`findings` / `population` を持つ flat な 1 行・`result` の text の最後の JSON 行に置く）。
const COST_VERDICT: &str = r#"{"verdict":"PASS","evidence":"ok","findings":"a:0","population":"files:1,lines:1"}"#;

/// runner を偽 claude の `body` で 1 回撃ち、stdout を返す（rc は 0 であること）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn runner_stdout_with(body: &str) -> String {
    let dir = tmp();
    let worktree = tmp();
    let claude = fake_claude(&dir, body, false, 0);
    let write_set = dir.join("write-set.txt");
    let vessel = write_vessel_copy(&dir, r#"["cargo", "git"]"#);
    fs::write(&write_set, "src/lib.rs\n").expect("write-set を書ける");
    let out = run_runner(
        &RunnerCall { dir: &dir, worktree: &worktree, write_set: &write_set, vessel: &vessel, claude: &claude, mode: "plan", account: None },
        b"goal = \"x\"\n",
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc は claude のまま 0: {}", stderr_of(&out));
    clean(&[&dir, &worktree]);
    stdout_of(&out)
}

/// lens を偽 claude の `body` で 1 回撃ち、stdout を返す（rc は 0 であること）。
fn lens_stdout_with(body: &str) -> String {
    let dir = tmp();
    let claude = fake_claude(&dir, body, false, 0);
    let contract = contract_in(&dir);
    let out = run_lens(&contract, 4096, "plan", &claude, b"--- a\n+++ b\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc 0: {}", stderr_of(&out));
    assert!(dir.join("called").exists(), "cap 内なので claude を呼ぶ");
    clean(&[&dir]);
    stdout_of(&out)
}

/// (d) runner の要約行に `usage=` の 3 語が載る（6 値は result record の値そのまま・`iterations[]` の 901 は載らない）。
#[test]
fn run_cost_runner_summary_line_carries_usage_from_the_result_record() {
    let out = runner_stdout_with(&cost_record("\"num_turns\":5,", "6000", "done"));
    let summary = out.lines().last().unwrap_or_default();
    assert_eq!(summary, format!("runner: rc=0 records=1 {COST_WORDS}"), "要約行（最終行）: {out}");
    assert!(!out.contains("901"), "入れ子の数に釣られない: {out}");
}

/// (d) lens は json の封筒の `result` の text の最後の JSON 行を判定に読み、封筒の 6 値を判定 object へ足して 1 行で写す。
#[test]
fn run_cost_lens_verdict_object_carries_usage_from_the_json_envelope() {
    let result = format!("読みました\n{{\"verdict\":\"FAIL\",\"evidence\":\"途中\"}}\n{COST_VERDICT}\nおしまい");
    let out = lens_stdout_with(&cost_record("\"num_turns\":5,", "6000", &result));
    let head = COST_VERDICT.strip_suffix('}').unwrap_or_default();
    assert_eq!(
        out.trim(),
        format!("{head},\"usage\":\"in:11,out:22,cache_read:33,cache_create:44\",\"turns\":5,\"wall_ms\":6000}}"),
        "最後の判定 + 6 値"
    );
    assert_eq!(out.lines().count(), 1, "stdout は 1 行だけ");
}

/// (g) 部分欠けと数でない値: `num_turns` だけ欠く周・`duration_ms` が数でない周は、要約行に `usage=` が載らず
/// 判定 object に `usage` が無い（rc は 0 のまま・判定は同じ行）。同じ歯の中で 6 値揃い＝載るを対に並べる。
#[test]
fn run_cost_partial_or_non_number_usage_is_carried_nowhere() {
    let full = runner_stdout_with(&cost_record("\"num_turns\":5,", "6000", "done"));
    assert!(full.contains(COST_WORDS), "6 値揃い＝載る: {full}");
    for (turns, wall, why) in [("", "6000", "num_turns だけ欠く"), ("\"num_turns\":5,", "\"6000\"", "duration_ms が数でない")] {
        let out = runner_stdout_with(&cost_record(turns, wall, "done"));
        assert_eq!(out.lines().last().unwrap_or_default(), "runner: rc=0 records=1", "{why}: 要約行は従来の字面: {out}");
        assert!(!out.contains("usage="), "{why}: {out}");
        let verdict = lens_stdout_with(&cost_record(turns, wall, COST_VERDICT));
        assert_eq!(verdict.trim(), COST_VERDICT, "{why}: 判定は 1 字も変わらない");
    }
    let verdict = lens_stdout_with(&cost_record("\"num_turns\":5,", "6000", COST_VERDICT));
    assert!(verdict.contains("\"usage\":"), "6 値揃い＝判定 object に載る: {verdict}");
}
