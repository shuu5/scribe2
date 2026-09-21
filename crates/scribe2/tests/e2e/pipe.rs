// flip-check: moved s2-07l.349
//! 縦 1 本 (a) / (b) の歯（設計 docs/design/pipeline.md §8 (a) / (b)）。
//!
//! tmp の git repo を作り `vessel init --state-dir` で置き場を紐づけてから撃つ。
//! runner は `sh -c` の 1 行の fake で、実 Claude は (d) の手番である。
//! commit には identity が要るので **repo local** の設定を与える（global は触らない）。

use crate::make_tmp_dir;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
use vessel::fleet::{Event, EventKind, Stage};
use vessel::headless::RC_RATE_LIMIT;
use vessel::hook::inject_path;
use vessel::name::NAME;
use vessel::order::is_declaration_order;
use vessel::pipe::approve::RC_BLOCKED;
use vessel::pipe::gate::{CHECKS, RC_INCONCLUSIVE, VERDICTS};
use vessel::rules::manifest::Manifest;
use vessel::rules::RuleValue;

mod dispatch;
mod gate;
mod intake;
mod land;
mod launch_failure;
mod ratelimit;
mod spawn;
mod stop;

// `lifecycle.rs` の跡地（`s2-07l.349` で `ratelimit.rs` / `stop.rs` に割った）: `spawn.rs` / `land.rs` は口座の
// fixture を `super::lifecycle::` の path で引くので、その名を `ratelimit` の別名として残す（呼び手は不変）。
use ratelimit as lifecycle;

/// binary の path。
pub(super) fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_scribe2")
}

/// binary を起こす `Command`（**この module 群が binary を起こす唯一の口**・設計 pipeline.md §28・`s2-07l.381`）。
///
/// cwd を **git repo でない tmp dir**（[`std::env::temp_dir`]）に固定する。nextest の子 process の cwd（crate dir＝便の
/// worktree の中）を継いだまま起こすと、`--state-dir` / `--repo` を読めない変異 binary の下で `pipe` の cwd の fallback
/// が anchor の `<NAME>.stateDir`（本番の置き場）へ届く（2026-09-16 の実測）。repo でない cwd なら、どの変異の下でも
/// fallback は「repo の root を解決できない」で断る（fail-closed）。cwd が主題の歯（相対 `--repo`・台帳の子の cwd）は
/// 返った `Command` に自分の `current_dir` を**後置**する（後の指定が勝つ）。置き場の pin は [`pipe_hermetic_sites_stay_one`]。
pub(super) fn bin_cmd() -> Command {
    let mut cmd = Command::new(bin());
    cmd.current_dir(std::env::temp_dir());
    cmd
}

/// tmp dir を 1 つ作り、symlink を解いた path を返す。
///
/// 包みは歯の thread へ預ける（[`crate::TmpDir::held`]・歯の終わりで消える）——呼び手は `tmp().join(..)` の一時値の形も
/// 持つので、包みを返すと文の終わりで dir が消える。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn tmp() -> PathBuf {
    let dir = make_tmp_dir().expect("tmp dir を作れる");
    dir.canonical().expect("tmp dir の実体 path を解ける").held()
}

/// git を 1 回撃ち、rc 0 を要求して stdout を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git を起動できる");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    out.status.success().then_some(text).expect("git が rc 0 で終わる")
}

/// toy repo の宣言が許す command。`sh` は歯の verify script を撃つためで、上限
/// （`--rules` の写し）にも同じ 3 つが在る。
pub(super) const VESSEL_ALLOWED: &str = r#"["git", "sh"]"#;

/// toy repo の宣言の共通 verify。**撃つのは別便**（.57）で、ここでは形だけを持つ。
pub(super) const VESSEL_COMMON: &str = r#"["git rev-parse --verify {base}"]"#;

/// vessel 宣言を repo の root へ書く（commit は呼び手が行う）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn write_vessel(repo: &Path, allowed: &str, common: &str) {
    let body =
        format!("schema = 1\nallowed-commands = {allowed}\ncommon-verify = {common}\nrequirements = \"{REQS_FILE}\"\n");
    fs::write(repo.join(".vessel.toml"), body).expect("宣言を書ける");
}

/// toy repo の要件面（契約表の `req` の id を持つ・`.md` の見出し形）。
pub(super) const REQS_FILE: &str = "reqs.md";

/// toy repo の設計 doc（契約の正本・repo 相対）。
pub(super) const DESIGN_FILE: &str = "docs/design/toy.md";

/// toy repo の契約表の行 id（[`design_pointer`] が指す 1 本）。
pub(super) const DESIGN_ROW: &str = "a";

/// 宣言を書き換えて commit する（**HEAD の tree が intake の読み面**である）。
pub(super) fn commit_vessel(repo: &Path, allowed: &str, common: &str) {
    write_vessel(repo, allowed, common);
    git(repo, &["add", "-f", ".vessel.toml"]);
    git(repo, &["commit", "-q", "-m", "vessel-decl"]);
}

/// 契約の verify 行が撃つ script を repo へ置く。
///
/// 宣言の allowlist と制御文字の禁止のもとでは、契約の verify 行は **`sh <file>` の
/// argv 1 本**になる（`;` や `|` の連結も、allowlist の外の command も書けない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn write_verify_scripts(repo: &Path) {
    for (name, body) in [
        ("verify-ok.sh", "exit 0\n"),
        ("verify-red.sh", "exit 1\n"),
        // 見出し行は `cmd=` として字面を載せるので、**cmd に無い語**を stderr へ出す。
        ("verify-noisy.sh", "printf 'bo%s\\n' om >&2\nexit 3\n"),
        // 1 回目は緑・2 回目は赤。印は **git の共通 dir**（便の worktree と main 実測の
        // tmp worktree で同じ path になる面）へ置く。
        (
            "verify-once.sh",
            "seen=\"$(git rev-parse --git-common-dir)/verify-seen\"\ntest ! -f \"$seen\" && touch \"$seen\"\n",
        ),
        // detached（= main 実測の tmp）のときだけ中間物を作る。
        (
            "verify-detached.sh",
            "git rev-parse --abbrev-ref HEAD | grep -qx HEAD && touch build-artifact.txt\nexit 0\n",
        ),
        ("verify-out.sh", "test -f docs/out.md\n"),
        // detached（= main 実測の tmp）のときだけ **撃った sh 自身を signal で殺す**（`$PPID` = 行を撃った
        // `sh -c`・dash は単純 command を exec しないので `$$` では inner だけが死んで rc 137 になる）。
        // rc が無い周を器は -1 と記す。
        (
            "verify-kill.sh",
            "git rev-parse --abbrev-ref HEAD | grep -qx HEAD && kill -9 $PPID\nexit 0\n",
        ),
        // `{jobs}` の置換を**撃たれた側**で写す（record の cmd だけを見ると、置換したのか
        // 行に数を書いてあったのかを弁別できない）。印は git の共通 dir へ置く。
        (
            "verify-jobs.sh",
            "printf '%s' \"$1\" > \"$(git rev-parse --absolute-git-dir)/jobs-seen\"\nexit 0\n",
        ),
        // 包みが出す終端行の fixture（`memory.peak` を読めた周の形）。
        ("verify-peak.sh", "printf 'confine-usage peak_bytes=3145728 oom_kill=0\\n'\nexit 0\n"),
        // 箱の中で kernel に殺された周の形（rc は 0 のまま＝**rc では見ない**ことを測る）。
        ("verify-oom.sh", "printf 'confine-usage peak_bytes=4194304 oom_kill=1\\n'\nexit 0\n"),
        // 呼出回数 file に 1 行足す stub（引数 = 段の印）。印は **git の共通 dir**（便の worktree と
        // main 実測の tmp worktree で同じ file になる面）へ置く。
        (
            "verify-count.sh",
            "printf '%s\\n' \"$1\" >> \"$(git rev-parse --git-common-dir)/detection-calls\"\nexit 0\n",
        ),
    ] {
        fs::write(repo.join(name), body).expect("verify script を書ける");
    }
}

/// commit を 1 つ持つ tmp の git repo と、紐づけた置き場を作る。
pub(super) fn repo_with_state() -> (PathBuf, PathBuf) {
    // **置き場は tmp root の 1 段下**（`<tmp>/state`）。host の受付札は `<state_dir の親>` から
    // 導くので、tmp root の直下に置くと全 test が同じ slot dir を共有して flaky になる。
    repo_with_state_in(&tmp().join(STATE_LEAF))
}

/// 置き場の leaf 名（[`repo_with_state`] と [`clean`] が共有する）。
pub(super) const STATE_LEAF: &str = "state";

/// commit を 1 つ持つ tmp の git repo と、**指名した path** に紐づけた置き場を作る。
pub(super) fn repo_with_state_in(state: &Path) -> (PathBuf, PathBuf) {
    repo_with_state_configured(state, &[])
}

/// auto maintenance を止めた config（loose object が pack へ詰まる経路を塞ぐ・`s2-07l.185`）。
pub(super) const NO_AUTO_MAINTENANCE: &[(&str, &str)] = &[
    ("maintenance.auto", "false"),
    ("maintenance.autoDetach", "false"),
    ("gc.auto", "0"),
    ("gc.autoDetach", "false"),
];

/// [`repo_with_state_in`] と同じ repo を、seed の commit を積む**前に** `config` を足して作る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn repo_with_state_configured(state: &Path, config: &[(&str, &str)]) -> (PathBuf, PathBuf) {
    fs::create_dir_all(state).expect("置き場を作れる");
    let repo = tmp();
    // 設計 §5.4 の land は `refs/heads/main` を進める。`git init` の既定 branch 名は
    // 環境依存（多くの host で `master`）なので、**test 側で main を明示する**。
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "e2e"]);
    git(&repo, &["config", "user.email", "e2e@example.invalid"]);
    for (key, value) in config {
        git(&repo, &["config", key, value]);
    }
    fs::create_dir_all(repo.join("src")).expect("src dir を作れる");
    fs::write(repo.join("src").join("lib.rs"), "// seed\n").expect("seed を書ける");
    // 要件面（契約表の `req` の id の出所・`.md` の見出し形・`requirement_ids` が読む）。
    fs::write(repo.join(REQS_FILE), "# toy の要件\n\n## FR4\n\n## FR5\n").expect("要件面を書ける");
    // 設計 doc（契約の正本・intake は **HEAD の tree** の行を読む）。
    write_design(&repo, &design_doc(&[]));
    // **宣言も marker と一緒に commit する**（intake は HEAD の tree から読む＝作業ツリー
    // に置いただけの宣言は無いのと同じ・設計 §8）。
    write_vessel(&repo, VESSEL_ALLOWED, VESSEL_COMMON);
    write_verify_scripts(&repo);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "seed"]);
    let out = bin_cmd()
        .args(["vessel", "init", "--state-dir"])
        .arg(state)
        .arg(&repo)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "vessel init は rc 0");
    (repo, state.to_path_buf())
}

/// 撃つ argv の `--state-dir` の値（持たない周は `None`）。
fn state_dir_of(args: &[&str]) -> Option<PathBuf> {
    args.windows(2)
        .find(|pair| pair.first() == Some(&"--state-dir"))
        .and_then(|pair| pair.get(1))
        .map(PathBuf::from)
}

/// `pipe` の起動を組む（**歯が実 binary を撃つ口 (i)**・設計 gate-cost.md §30 約束 2）。
///
/// 撃つ argv が `--state-dir` を持つ周は、その置き場の下に道具箱（`crate::toolbox_path`）を置き、
/// PATH の先頭に積んで撃つ。持たない周は usage の断りか「置き場が紐づいていない」の断りで段に
/// 届かない（run dir も event も作らない＝scope を 1 本も作れない）ので、host の PATH のまま撃つ。
///
/// 起動を返すのは、`Command` を自分で組む直起動の呼び手（背景で起こす便・pane の env を足す周）も
/// **同じ口を通す**ためである——PATH の組み立てを呼び手ごとに書き直すと、後から書かれた呼出が
/// 実 `systemd-run` へ戻る。
// flip-check: retroactive s2-07l.504
pub(super) fn pipe_cmd(args: &[&str]) -> Command {
    let mut cmd = bin_cmd();
    cmd.arg("pipe").args(args);
    if let Some(state) = state_dir_of(args) {
        cmd.env("PATH", crate::toolbox_path(&state));
    }
    cmd
}

/// `pipe` を binary で 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn run_pipe(args: &[&str]) -> Output {
    pipe_cmd(args).output().expect("binary を起動できる")
}

/// 正しく書けた契約表の行の欄。差し替えたい欄だけ上書きして使う（`owner` / `disposition` / `design` は
/// 行が持たない＝生成の導出値と pointer そのもの・契約 (b)）。
pub(super) fn contract_body() -> Vec<String> {
    [
        r#"id = "a""#,
        r#"title = "縦 1 本を通す""#,
        r#"req = ["FR4"]"#,
        r#"section = "1""#,
        r#"write-set = ["src/lib.rs"]"#,
        r#"verify = ["sh verify-ok.sh"]"#,
        r#"size = "S""#,
        r#"done = "run が Implemented になる""#,
    ]
    .iter()
    .map(|line| (*line).to_owned())
    .collect()
}

/// 契約表の行 1 本を持つ設計 doc の本文（`drop` の欄を落とし `add` の欄を足す）。
///
/// 節は 1 つ（`## 1.`・本文つき）で、行は区間の中に置く。**契約の正本はこの doc の行**である（契約 (b)）。
pub(super) fn design_doc(fields: &[String]) -> String {
    let rows = if fields.is_empty() { contract_body() } else { fields.to_vec() };
    design_doc_rows(&[rows])
}

/// 行を**複数**持つ設計 doc の本文（同じ base から 2 便を起こす歯が使う・commit を 1 回に保つ）。
pub(super) fn design_doc_rows(rows: &[Vec<String>]) -> String {
    let listed: Vec<String> = rows.iter().map(|fields| format!("[[contract]]\n{}", fields.join("\n"))).collect();
    format!(
        "# 設計: toy\n\n## 1. 何を解くか\n\ntoy repo の縦 1 本を通す節の本文。\n\n{}\nschema = 1\n\n{}\n{}\n",
        table_begin(),
        listed.join("\n\n"),
        table_end()
    )
}

/// 欄を差し替えた行の欄の列（`drop` を落とし `add` を足す・`id` は `id` 引数で上書き）。
pub(super) fn row_fields(id: &str, drop: &[&str], add: &[&str]) -> Vec<String> {
    let mut fields: Vec<String> = contract_body()
        .into_iter()
        .filter(|line| !drop.iter().any(|key| line.starts_with(key)) && !line.starts_with("id"))
        .collect();
    fields.insert(0, format!("id = \"{id}\""));
    fields.extend(add.iter().map(|line| (*line).to_owned()));
    fields
}

/// 行を**複数**書いて 1 回だけ commit する（同じ base から複数便を起こす歯の入口）。
pub(super) fn commit_rows(repo: &Path, rows: &[Vec<String>]) {
    let body = design_doc_rows(rows);
    if fs::read_to_string(repo.join(DESIGN_FILE)).is_ok_and(|found| found == body) {
        return;
    }
    for fields in rows {
        seed_write_set(repo, fields);
    }
    write_design(repo, &body);
    if !repo.join(".git").exists() {
        return;
    }
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "design-rows"]);
}

/// 契約表の区間の marker（器の [`vessel::pipe::table::BEGIN`] / `END` と同じ字面を器から借りる）。
fn table_begin() -> &'static str {
    vessel::pipe::table::BEGIN
}

/// 区間の終わりの marker。
fn table_end() -> &'static str {
    vessel::pipe::table::END
}

/// 行の `write-set` が名指す素の path を repo に用意する（**行は base に解けなければならない**・契約 (b)）。
///
/// `+`（新規）・`-`（縮む）・末尾 `/`（dir）の項目は base に在ってはならない / dir なので触らない。既に在る file も
/// 触らない（上限の余地を測る歯が置いた行数を壊さない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn seed_write_set(repo: &Path, fields: &[String]) {
    let Some(line) = fields.iter().find(|line| line.starts_with("write-set")) else {
        return;
    };
    for item in line.split('"').skip(1).step_by(2) {
        if item.starts_with('+') || item.starts_with('-') || item.contains('=') {
            continue;
        }
        let path = repo.join(item.trim_end_matches('/'));
        if path.exists() {
            continue;
        }
        // 末尾 `/` の項目は dir として解ける必要がある（中身が 1 file も無い dir は git が持てないので seed を置く）。
        if item.ends_with('/') {
            fs::create_dir_all(&path).expect("write-set の dir を作れる");
            fs::write(path.join("seed.rs"), "").expect("dir の seed を書ける");
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("write-set の dir を作れる");
        }
        fs::write(&path, "").expect("write-set の file を書ける");
    }
}

/// 設計 doc を repo へ書く（commit は呼び手）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn write_design(repo: &Path, body: &str) {
    let path = repo.join(DESIGN_FILE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("設計 doc の dir を作れる");
    }
    fs::write(&path, body).expect("設計 doc を書ける");
}

/// 契約表の行を書いて **commit し**、`--design` に渡す pointer を返す。`drop` の欄を落とし `add` の欄を足す。
///
/// commit するのは、受付が読むのが**作業木でなく base（`HEAD`）**だからである（契約 (b)・設計 §2「生成」）。
pub(super) fn write_contract(repo: &Path, drop: &[&str], add: &[&str]) -> String {
    let mut fields: Vec<String> = contract_body()
        .into_iter()
        .filter(|line| !drop.iter().any(|key| line.starts_with(key)))
        .collect();
    fields.extend(add.iter().map(|line| (*line).to_owned()));
    commit_row(repo, &fields);
    design_pointer()
}

/// 行を書いて commit する。**seed と同じ本文なら commit しない**（HEAD を動かさない）。
///
/// 受付は base（`HEAD`）の行を読むので行は commit されていなければならないが、seed が既に既定の行を
/// commit している。既定のまま撃つ歯（母集団の大半）で HEAD を動かすと、base の sha を先に控える歯や
/// commit 数を数える歯が、契約とは関係のない理由で落ちる。
pub(super) fn commit_row(repo: &Path, fields: &[String]) {
    let body = design_doc(fields);
    if fs::read_to_string(repo.join(DESIGN_FILE)).is_ok_and(|found| found == body) {
        return;
    }
    seed_write_set(repo, fields);
    write_design(repo, &body);
    // git repo でない dir（「repo でない」を測る歯）は書くだけで commit しない。
    if !repo.join(".git").exists() {
        return;
    }
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "design-row"]);
}

/// toy repo の設計 pointer（`<doc>#<行 id>`）。
pub(super) fn design_pointer() -> String {
    format!("{DESIGN_FILE}#{DESIGN_ROW}")
}

/// stdout の全文。
pub(super) fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// stderr の全文。
pub(super) fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// `run=<id>` の 1 行から id を取る。
pub(super) fn run_id_of(out: &Output) -> String {
    stdout_of(out)
        .lines()
        .find_map(|line| line.strip_prefix("run="))
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_owned)
        .unwrap_or_default()
}

/// intake を 1 回通して run id を返す（審査の段は**偽 PASS の lens**で 1 回通す・FR49・設計 contract-source.md §4
/// 「人の関与 0」＝審査を飛ばす flag は歯にも無い）。
pub(super) fn intake(repo: &Path, state: &Path, design: &str) -> String {
    intake_bead(repo, state, design, "s2-2e5")
}

/// 審査の段（`pipe intake` の直後の lens 1 回）を通す偽 PASS の lens（gate の [`fake_lens`] と同じ作り）。
/// marker は置き場の [`REVIEW_MARKER`]＝「審査の lens が起きた」を効果で測れる（gate の marker とは別名）。
pub(super) fn review_lens_pass(state: &Path) -> String {
    fake_lens(&state.join(REVIEW_MARKER), &lens_verdict("PASS"))
}

/// 審査の偽 lens が置く marker の名（置き場の直下）。
pub(super) const REVIEW_MARKER: &str = "review-lens-ran";

/// bead を選んで intake を 1 回通す。**run id は `<bead>-<秒>`** なので、同じ秒に
/// 2 便を起こす歯は bead を分ける（同 bead だと id が衝突して 2 便目が断られる）。
pub(super) fn intake_bead(repo: &Path, state: &Path, design: &str, bead: &str) -> String {
    let out = intake_raw(repo, state, design, bead);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "intake は rc 0: {}", stderr_of(&out));
    run_id_of(&out)
}

/// 測り終えた便を `stop --run` で外す（rc 0 を要求する）。
///
/// 入口の排他（`s2-07l.145`・ADR-0019 §2.1）が在るので、**終端でない便**が置き場に残ったまま
/// 同じ write-set の 2 本目を intake することはできない。1 つの置き場で 2 便を順に測る歯は、
/// 前の便をこの口で外してから次を起こす。
pub(super) fn stop_run_ok(state: &Path, id: &str) {
    let out = run_pipe(&["stop", "--run", id, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop --run: {}", stderr_of(&out));
}

/// 後片付け。[`STATE_LEAF`] の置き場は tmp root ごと畳む（host の slot dir も同じ root に在る）。
pub(super) fn clean(dirs: &[&Path]) {
    for dir in dirs {
        let root = dir
            .parent()
            .filter(|_| dir.file_name().is_some_and(|name| name == STATE_LEAF))
            .unwrap_or(dir);
        fs::remove_dir_all(root).ok();
    }
}

/// dir の entry 名を昇順で返す（写しの範囲を**集合で**測るための helper）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn dir_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("dir を読める")
        .map(|entry| entry.expect("entry を読める").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// event log の行数（file が無ければ 0）。
pub(super) fn event_count(state: &Path) -> usize {
    fs::read_to_string(state.join("fleet").join("events.jsonl"))
        .map(|text| text.lines().filter(|line| !line.is_empty()).count())
        .unwrap_or(0)
}

/// intake を 1 回撃つ（rc を assert しない形・審査の lens は偽 PASS）。
pub(super) fn intake_raw(repo: &Path, state: &Path, design: &str, bead: &str) -> Output {
    let rules = ceiling_rules(state);
    run_pipe(&[
        "intake", "--design", design, "--bead", bead,
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules, "--lens", &review_lens_pass(state),
    ])
}

/// 便の vessel の写し。
pub(super) fn vessel_copy(state: &Path, id: &str) -> PathBuf {
    state.join("pipe").join(id).join("vessel.toml")
}

/// `/proc/<pid>` が在るか。
pub(super) fn proc_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// `/proc/<pid>/stat` の pgid（読めなければ `None`）。
pub(super) fn proc_pgid(pid: u32) -> Option<u32> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = text.rsplit_once(')')?;
    rest.split_whitespace().nth(2)?.parse().ok()
}

/// 便の event のうち `kind` の件数。
pub(super) fn kind_count(state: &Path, id: &str, kind: EventKind) -> usize {
    events(state).iter().filter(|found| found.run == id && found.kind == kind).count()
}

/// 自分が起こした子孫の process を片付ける（`sleep` の pid だけ・無ければ何もしない）。
pub(super) fn reap_own(pid: u32) {
    let is_sleep = fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|comm| comm.trim() == "sleep");
    if is_sleep {
        Command::new("kill").args(["-KILL", "--", &pid.to_string()]).output().ok();
    }
}

#[test]
fn pipe_external_form() {
    let args = |raw: &[&str]| raw.iter().map(|item| (*item).to_owned()).collect::<Vec<String>>();
    let mut lines = vec![vessel::pipe::cli::usage()];
    lines.extend(vessel::pipe::cli::dispatch(&args(&["show"])).err);
    lines.extend(vessel::pipe::cli::dispatch(&args(&["stop"])).err);
    lines.extend(vessel::pipe::cli::dispatch(&args(&["nope"])).err);
    // 審査の段を飛ばす口は無い（`--no-review` は usage で断る・AC22・設計 contract-source.md §4）。
    lines.extend(vessel::pipe::cli::dispatch(&args(&["run", "--no-review"])).err);
    let form = lines.join("\n");
    insta::assert_snapshot!(form);
}

/// (e) 契約の verify 行に禁じる語列（rules 行 `runner.denied_commands`・ADR-0025 §2.3・`s2-07l.168`）が当たれば intake は
/// rc 1 で断り、何本目の行かと行 id・語列を名指す（event を書かない・run dir も作らない）。判定は hook の command
/// guard と同じ 1 関数＝順序不問（`git branch -D x` も `git branch x -D` も当たる）。先頭語が宣言の allowlist に在る
/// 行でも止まる（先頭語だけの判定では `git` の破壊形が入口を通る＝監査 `.168` の穴）。当たらない行は従来どおり受理。
#[test]
fn pipe_intake_rejects_verify_line_with_denied_sequence() {
    let (repo, state) = repo_with_state();
    for (add, sequence) in [
        (r#"verify = ["git branch -D x"]"#, "git branch -D"),
        (r#"verify = ["git branch x -D"]"#, "git branch -D"),
        (r#"verify = ["sh verify-ok.sh", "git push origin main --force"]"#, "git push --force"),
    ] {
        let path = write_contract(&repo, &["verify"], &[add]);
        let out = intake_raw(&repo, &state, &path, "b");
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{add} は rc 1: {err}");
        // 契約 (b) 以後、verify の面は**行**である（理由は行の欄と doc の位置を名乗る）。
        assert!(err.contains("verify"), "どちらの面かを言う: {err}");
        assert!(err.contains("runner.denied_commands") && err.contains(sequence), "行 id と語列を名指す: {err}");
        assert!(!err.contains("allowed-commands"), "先頭語は宣言の内＝断る理由は語列だけ: {err}");
    }
    let two = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-ok.sh", "git push origin main --force"]"#]);
    let err = stderr_of(&intake_raw(&repo, &state, &two, "b"));
    // 契約 (b) 以後、断るのは**行**なので理由は当該の verify の行そのものを逐語で名乗る（何本目かの序数は持たない）。
    assert!(err.contains("git push origin main --force"), "当たった行を名指す: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    // 対: 当たらない行（`git branch -d x`・語が違う）は受理される。
    let fine = write_contract(&repo, &["verify"], &[r#"verify = ["git branch -d x"]"#]);
    let out = intake_raw(&repo, &state, &fine, "b");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "当たらない行は受理: {}", stderr_of(&out));
    clean(&[&repo, &state]);
}

// ── (b) gate → land → verdict export → e2e（設計 §8 (b)） ──────────────────

/// 便を Implemented まで進める（intake → spawn）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn implemented(repo: &Path, state: &Path, design: &str) -> String {
    let id = intake(repo, state, design);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));
    out.status.code().expect("spawn の rc を読める");
    id
}

/// 呼ばれたら marker を作り、JSON 1 行を返す fake lens（`sh -c` の 1 行）。
///
/// marker で「lens を**起動しなかった**」を測れるようにしてある。判定順の 2 分岐
/// （verify RED / cap 超過）は lens を呼ばないことが契約なので、verdict だけを見ると
/// 「呼んだうえで INCONCLUSIVE を返した」と区別がつかない。
///
/// **末尾の `:` は器が足した語を捨てる口である**（`s2-07l.412`・設計 account-autonomy.md §15）: gate は
/// lens の起動行の末尾に選んだ口座の `--account-dir <dir>` を足すので、`echo` で終わる行のままだと
/// その 2 語まで verdict と同じ行に印字され、JSON 1 行が読めず**全 e2e の gate が静かに INCONCLUSIVE へ
/// 倒れる**。捨てるのに `#` を使わない——行の後ろへ足して使う呼び手（箱の終端行を出す歯）が在り、
/// コメントはその足した分まで覆う（`:` は引数を無視する builtin なので、足す側も足さない側も動く）。
pub(super) fn fake_lens(marker: &Path, body: &str) -> String {
    format!("cat >/dev/null; touch '{}'; echo '{body}'; :", marker.display())
}

/// `--rules` に渡す tmp manifest を書く（gate の 2 行 + lock の 2 行だけ）。
///
/// `enabled` は**必須 key**なので全行に書く（`s2-07l.80`）。この便の test 区間の差は
/// この字面の追加だけで、assert の意味は 1 つも動かない——base の loader は `enabled` を
/// 書いた行も同じ値で読むので、base で新しく赤くなる歯は 1 本も無い。
// flip-check: retroactive s2-07l.80
pub(super) fn write_rules(dir: &Path, name: &str, lens_count: u64, cap: u64) -> PathBuf {
    write_rules_with_retries(dir, name, lens_count, cap, FOLLOW_RETRIES)
}

/// 埋め込み manifest の起こし直しの上限（`pipe.follow_retries`）。写しの既定値をここから
/// 引くのは、**差し替えた周だけが上限の歯である**ことを字面で読めるようにするためである。
pub(super) const FOLLOW_RETRIES: u64 = 2;

/// tmp manifest の受付の待ちの上限（秒・rules 行 `gate.slot_wait_s` の fixture 値）。
pub(super) const SLOT_WAIT_S: u64 = 1;

/// tmp manifest の終端が CI を待つ上限（秒・rules 行 `pipe.ci_wait_s` の fixture 値）。
///
/// **終端は押す先を宣言した repo でしか走らない**ので、この値が効くのは終端の歯だけである
/// （既存の toy repo は `remote` を宣言しない＝`terminal=undeclared` で待たない）。
pub(super) const CI_WAIT_S: u64 = 1;

/// tmp manifest の land の順番を待つ上限（秒・rules 行 `pipe.land_wait_s` の fixture 値）。待ちが
/// 解ける歯だけが [`write_rules_land_wait`] で長い値に振る。
pub(super) const LAND_WAIT_S: u64 = 1;

/// tmp manifest の受付の 3 値（rules 行 `gate.job_memory_mb` / `host.reserve_memory_mb` /
/// `gate.slot_wait_s` の fixture 値）と、同じ待ちの上限を使う器の健康の遮断器の倍率 2 値
/// （rules 行 `host.runnable_per_core` / `host.blocked_per_core`・設計 gate-cost.md §32）。
#[derive(Debug, Clone, Copy)]
pub(super) struct SlotFixture {
    /// job 1 つが要る memory（MiB）。
    job_mb: u64,
    /// 残す memory（MiB）。
    reserve_mb: u64,
    /// 待ちの上限（秒）。
    wait_s: u64,
    /// 遮断器の走行可能の倍率。
    runnable_per_core: u64,
    /// 遮断器の待ちの倍率。
    blocked_per_core: u64,
}

/// 遮断器の倍率の既定の fixture 値（**十分大きく取る**＝並列の歯で混んだ host でも遮断器が閉じない・遮断器の歯
/// だけが [`SlotFixture`] で振る）。
pub(super) const HEALTH_PER_CORE_OPEN: u64 = 1_000_000;

/// 既定の受付 fixture: 容量の 2 行は埋め込みの値を写し（封じ込めの箱と同じ値で測る）、待ちの
/// 上限だけを [`SLOT_WAIT_S`] に縮める（枠の空かない host で歯が 900 秒待たない）。遮断器の倍率は
/// [`HEALTH_PER_CORE_OPEN`]。
pub(super) fn default_slots() -> SlotFixture {
    SlotFixture {
        job_mb: embedded_int("gate.job_memory_mb"),
        reserve_mb: embedded_int("host.reserve_memory_mb"),
        wait_s: SLOT_WAIT_S,
        runnable_per_core: HEALTH_PER_CORE_OPEN,
        blocked_per_core: HEALTH_PER_CORE_OPEN,
    }
}

/// [`write_rules`] に起こし直しの上限を足した形（上限の歯だけが値を振る）。
pub(super) fn write_rules_with_retries(dir: &Path, name: &str, lens_count: u64, cap: u64, retries: u64) -> PathBuf {
    write_rules_full(dir, name, (lens_count, cap), retries, default_slots())
}

/// [`write_rules_with_retries`] に受付の 3 値を足した形（待ちが解ける歯だけが値を振る）。
/// `gate` は `(gate.lens_count, gate.token_cap)`。上限の 2 行（R-C4-1 / R-C4-2）は埋め込みの値を写す。
pub(super) fn write_rules_full(dir: &Path, name: &str, gate: (u64, u64), retries: u64, slots: SlotFixture) -> PathBuf {
    write_rules_capped(dir, name, RulesFixture { gate, retries, slots, caps: default_caps() })
}

/// tmp manifest の値の束（[`write_rules_capped`] の入力）。
#[derive(Debug, Clone, Copy)]
pub(super) struct RulesFixture {
    /// `(gate.lens_count, gate.token_cap)`。
    pub(super) gate: (u64, u64),
    /// 起こし直しの上限（`pipe.follow_retries`）。
    pub(super) retries: u64,
    /// 受付の 3 値。
    pub(super) slots: SlotFixture,
    /// 上限の 2 値。
    pub(super) caps: CapFixture,
}

/// tmp manifest の上限の 2 値（rules 行 `R-C4-1` / `R-C4-2` の fixture 値・上限の余地の歯だけが振る）。
#[derive(Debug, Clone, Copy)]
pub(super) struct CapFixture {
    /// core の総行数の上限（R-C4-1）。
    pub(super) core_lines: u64,
    /// 1 file の行数の上限（R-C4-2）。
    pub(super) file_lines: u64,
}

/// 既定の上限 fixture: 埋め込みの値を写す（余地の歯だけが [`write_rules_capped`] で縮める）。
pub(super) fn default_caps() -> CapFixture {
    CapFixture { core_lines: embedded_int("R-C4-1"), file_lines: embedded_int("R-C4-2") }
}

/// [`write_rules_full`] に上限の 2 値を足した形。size ↔ 行数の 3 行（`pipe.size_<s|m|l>_lines`）は埋め込みの値を写す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn write_rules_capped(dir: &Path, name: &str, fixture: RulesFixture) -> PathBuf {
    let RulesFixture { gate: (lens_count, cap), retries, slots, caps } = fixture;
    let row = |id: &str, kind: &str, value: u64| {
        format!(
            "[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n"
        )
    };
    // **上限の行も載せる**。intake は宣言をこの行と突き合わせるので、上限を持たない
    // manifest を渡した周は「上限が無い」で断られる（`--rules` は全 subcommand に効く）。
    // 禁じる語列の行（`runner.denied_commands`・ADR-0025 §2.3・`s2-07l.168`）も対で載せる＝intake は verify 行に
    // hook の command guard と同じ判定を掛け、行の無い manifest では受付が断られる。
    let ceiling = "[[rule]]\nid = \"runner.allowed_commands\"\nkind = \"RunnerAllowedCommands\"\n\
                   value = [\"cargo\", \"git\", \"sh\"]\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n\n\
                   [[rule]]\nid = \"runner.denied_commands\"\nkind = \"RunnerDeniedCommands\"\n\
                   value = [\"cargo mutants\", \"git push --force\", \"git push -f\", \"git branch -D\"]\n\
                   enabled = true\nruling = \"t\"\nruled_at = \"d\"\n";
    // 受付の 4 行: 並列度の上限は埋め込みの値を写し、残る 3 行は [`SlotFixture`] の値。遮断器の倍率 2 行も
    // [`SlotFixture`] の値（行の無い manifest では gate / land が線を読めず rc 2 で止まる）。
    // 着地の順番の上限は [`LAND_WAIT_S`]（前の便が列に残る歯で 90 分待たない）。
    // 上限の余地の 6 行（設計 contract-source.md §3）: 上限の 2 行は [`CapFixture`]・size の 3 行と行の数え方の幅は
    // 埋め込みの値。同型の審査 FAIL の停止の回数（`review.same_kind_stop`・contract-source.md §23）も埋め込みの値
    // ＝行の無い manifest では受付が rc 2 で断る（`pipe_intake_repeat_` の歯だけが行を落として測る）。
    let body = format!(
        "schema = 1\n\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{ceiling}",
        row("gate.lens_count", "GateLensCount", lens_count),
        row("gate.token_cap", "GateTokenCap", cap),
        row("fleet.lock_retry_ms", "LockRetryMs", 5000),
        row("fleet.lock_stale_ms", "LockStaleMs", 30000),
        row("pipe.follow_retries", "FollowRetries", retries),
        row("gate.mutants_jobs", "GateMutantsJobs", embedded_int("gate.mutants_jobs")),
        row("gate.job_memory_mb", "GateJobMemoryMb", slots.job_mb),
        row("host.reserve_memory_mb", "HostReserveMemoryMb", slots.reserve_mb),
        row("gate.slot_wait_s", "GateSlotWaitS", slots.wait_s),
        row("host.runnable_per_core", "HostRunnablePerCore", slots.runnable_per_core),
        row("host.blocked_per_core", "HostBlockedPerCore", slots.blocked_per_core),
        row("pipe.land_wait_s", "PipeLandWaitS", LAND_WAIT_S),
        row("pipe.ci_wait_s", "PipeCiWaitS", CI_WAIT_S),
        row("R-C4-1", "CoreLines", caps.core_lines),
        row("R-C4-2", "ModuleLines", caps.file_lines),
        row("pipe.size_s_lines", "PipeSizeSLines", embedded_int("pipe.size_s_lines")),
        row("pipe.size_m_lines", "PipeSizeMLines", embedded_int("pipe.size_m_lines")),
        row("pipe.size_l_lines", "PipeSizeLLines", embedded_int("pipe.size_l_lines")),
        row("R-C4.line-width", "LineWidth", embedded_int("R-C4.line-width")),
        row(SAME_KIND_STOP_ROW, "ReviewSameKindStop", embedded_int(SAME_KIND_STOP_ROW)),
    );
    let path = dir.join(name);
    fs::write(&path, body).expect("tmp manifest を書ける");
    path
}

/// 同型の審査 FAIL の停止の回数を持つ rules 行の id（tmp manifest に埋め込みの値で載せる・行を落とす歯が名指す）。
pub(super) const SAME_KIND_STOP_ROW: &str = "review.same_kind_stop";

/// intake が読む上限の manifest（`sh` を足した写し）。置き場の中に 1 本だけ作る。
pub(super) fn ceiling_rules(state: &Path) -> String {
    let path = state.join("rules-ceiling.toml");
    if !path.exists() {
        return write_rules(state, "rules-ceiling.toml", 1, 1_000_000).display().to_string();
    }
    path.display().to_string()
}

/// `--rules` を足して gate を 1 回撃つ。
pub(super) fn gate_with_rules(repo: &Path, state: &Path, id: &str, rules: &Path, lens: &str) -> Output {
    run_pipe(&[
        "gate", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &rules.display().to_string(), "--lens", lens,
    ])
}

/// 偽 lens が出す findings の 8 category（宣言順・**0 件も 0 と書く**・`s2-07l.188`）。
pub(super) const FAKE_FINDINGS: &str =
    "contract-fit:0,teeth-nonvacuous:0,constitution:0,delete:0,stdlib:0,native:0,yagni:0,shrink:0";

/// 偽 lens が出す母集団（**0 でない**＝読んだ・0 は「見ていない」で INCONCLUSIVE へ倒る）。
pub(super) const FAKE_POPULATION: &str = "files:1,lines:1";

/// 3 値を返す fake lens の本文（必須 key の 2 つも出す・`s2-07l.188`）。
pub(super) fn lens_verdict(verdict: &str) -> String {
    format!(
        "{{\"verdict\":\"{verdict}\",\"evidence\":\"fake\",\"findings\":\"{FAKE_FINDINGS}\",\"population\":\"{FAKE_POPULATION}\"}}"
    )
}

/// **偽 lens が出す 2 key は、器が必須とする 2 key と同じ 1 つである**（`s2-07l.188`）。
///
/// [`lens_verdict`] は e2e のほぼ全部が使う lens の本文である。器が `findings` / `population` を
/// 必須にした以上、この helper の字面がそのまま判定の record へ載ることをここで測る——載らない
/// 形（helper と器の要求が割れた周）では、全 e2e の gate が INCONCLUSIVE で静かに止まる。
#[test]
fn pipe_gate_findings_fake_lens_keys_reach_the_verdict_record() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "偽 lens の 2 key で通る: {}", stderr_of(&out));
    assert!(marker.exists(), "lens を起動した周（判定に届いている）");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "findings"), FAKE_FINDINGS, "8 category の件数が record に載る: {pairs:?}");
    assert_eq!(value_of(&pairs, "population"), FAKE_POPULATION, "母集団も同じ record に載る: {pairs:?}");
    clean(&[&repo, &state]);
}

/// **器が lens の起動行に足した語を、偽 lens は verdict の JSON に混ぜない**（`s2-07l.412`・設計
/// account-autonomy.md §15）。
///
/// [`fake_lens`] は e2e のほぼ全部が使う lens である。gate が起動行の末尾に `--account-dir <dir>` を
/// 足すようになると、`echo` で終わる行はその 2 語まで同じ行に印字する＝判定の JSON が読めず、全 e2e の
/// gate が静かに INCONCLUSIVE へ倒れる。宣言口座のある置き場で 1 便を通し、**判定が読めること**
/// （PASS + 必須 2 key）と**器が実際に口座を足したこと**（`Gated` の detail）を対で測る——後者が無いと
/// 「足していない木でも読める」だけの歯になり、fixture の値打ちを測れない。
#[test]
fn pipe_gate_lens_account_fake_lens_ignores_extra_args() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let rules = ratelimit::resume_rules(&state, &["a1"]);
    ratelimit::put_account(&state, "a1", &[ratelimit::windows(40, 10)]);
    let marker = state.join("lens-ran");
    let out = run_pipe(&[
        "gate", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &rules, "--curl", &ratelimit::fake_usage_curl(&state),
        "--lens", &fake_lens(&marker, &lens_verdict("PASS")),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "口座を足した行でも判定は読める: {}", stderr_of(&out));
    assert!(marker.exists(), "lens は起動している");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS", "余分な語が混じると 3 値を読めない: {pairs:?}");
    assert_eq!(value_of(&pairs, "findings"), FAKE_FINDINGS, "必須 2 key も同じ 1 行から読める: {pairs:?}");
    assert_eq!(
        gated_details(&state, &id),
        vec!["verdict:PASS,account:a1".to_owned()],
        "器が選んだ口座を足した周である（足さない木ではこの detail が出ない）"
    );
    clean(&[&repo, &state]);
}

/// 便の `RunStage(Gated)` の detail の列（物理順）。
pub(super) fn gated_details(state: &Path, id: &str) -> Vec<String> {
    stages(state, id)
        .into_iter()
        .filter(|(stage, _)| *stage == Some(Stage::Gated))
        .filter_map(|(_, detail)| detail)
        .collect()
}

/// 便の worktree。
pub(super) fn worktree_of(repo: &Path, id: &str) -> PathBuf {
    repo.join(".worktrees").join("scribe2").join(id)
}

/// `verdict.json` を key/value の並びとして読む。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn verdict_pairs(state: &Path, id: &str) -> Vec<(String, vessel::fleet::json_lite::Value)> {
    let path = state.join("pipe").join(id).join("verdict.json");
    let text = fs::read_to_string(&path).expect("verdict.json を読める");
    vessel::fleet::json_lite::parse_object(text.trim()).expect("verdict.json は 1 行の JSON")
}

/// key/value の並びから 1 つの値を字面で取る。無ければ空。
pub(super) fn value_of(pairs: &[(String, vessel::fleet::json_lite::Value)], key: &str) -> String {
    use vessel::fleet::json_lite::Value;
    pairs
        .iter()
        .find(|(found, _)| found == key)
        .map(|(_, value)| match value {
            Value::Str(text) => text.clone(),
            Value::Num(found) => found.to_string(),
            Value::Bool(found) => found.to_string(),
            Value::Null => "null".to_owned(),
        })
        .unwrap_or_default()
}

/// `pipe show` の 1 行。
pub(super) fn show_line(repo: &Path, state: &Path, id: &str) -> String {
    let out = run_pipe(&[
        "show", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    stdout_of(&out)
}

/// gate を 1 回撃つ（lens は任意）。
pub(super) fn gate_once(repo: &Path, state: &Path, id: &str, lens: Option<&str>) -> Output {
    let mut args: Vec<String> = ["gate", "--run", id]
        .iter()
        .map(|item| (*item).to_owned())
        .collect();
    args.extend([
        "--repo".to_owned(), repo.display().to_string(),
        "--state-dir".to_owned(), state.display().to_string(),
    ]);
    if let Some(cmd) = lens {
        args.extend(["--lens".to_owned(), cmd.to_owned()]);
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_pipe(&borrowed)
}

/// land を 1 回撃つ。
pub(super) fn land_once(repo: &Path, state: &Path, id: &str) -> Output {
    run_pipe(&[
        "land", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ])
}

/// PATH の先頭に「`diff --name-only -z` だけ rc 1 で落とし、他は実 git へ exec する git」を置いて
/// land を 1 回撃つ（段① write-set 照合を **rc -1**＝起動できなかった段にする）。
///
/// verify 行は全部 `sh -c` で撃たれるので、実在しない binary 名は sh の rc 127（実測の赤）で
/// あって rc -1 にならない。rc -1 を作れるのは段①の diff を読めない周だけである。
pub(super) fn land_once_with_unreadable_diff(repo: &Path, state: &Path, id: &str) -> Output {
    land_once_with_git_shim(repo, state, id, " diff --name-only -z ", None)
}

/// PATH の先頭に「引数列に `failing` を含む呼出しだけ rc 1 で落とし、他は実 git へ exec する git」を置いて
/// land を 1 回撃つ（`--lens` は任意）。読めなかった周の極性を測る歯の共通部。
///
/// binary を起こすのは [`run_pipe_with_git_shim`]（verb と `--repo` / `--state-dir` はここで固定する）。
pub(super) fn land_once_with_git_shim(repo: &Path, state: &Path, id: &str, failing: &str, lens: Option<&str>) -> Output {
    let mut args = vec![
        "land".to_owned(), "--run".to_owned(), id.to_owned(),
        "--repo".to_owned(), repo.display().to_string(),
        "--state-dir".to_owned(), state.display().to_string(),
    ];
    if let Some(cmd) = lens {
        args.extend(["--lens".to_owned(), cmd.to_owned()]);
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_pipe_with_git_shim(state, failing, &borrowed)
}

/// PATH の先頭に「引数列に `failing` を含む呼出しだけ rc 1 で落とし、他は実 git へ exec する git」を置いて
/// `pipe` を 1 回撃つ（[`land_once_with_git_shim`] の起こす口・verb は呼び手が選ぶ）。
pub(super) fn run_pipe_with_git_shim(state: &Path, failing: &str, args: &[&str]) -> Output {
    let path = shim_path(state, "shim-bin", &format!("case \"$*\" in *'{failing}'*) exit 1;; esac"));
    run_pipe_with_path(&path, args)
}

/// PATH の先頭に置く偽 git（`script` を先に撃ってから実 git へ exec する）。返すのは PATH の値。
///
/// 器の git の呼び方を**現物で**振る唯一の口である（読めない git・書き込む git・壊す git）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn shim_path(state: &Path, name: &str, script: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = state.join(name);
    fs::create_dir_all(&bin_dir).expect("shim の dir を作れる");
    let real = String::from_utf8_lossy(
        &Command::new("sh").args(["-c", "command -v git"]).output().expect("git を引ける").stdout,
    )
    .trim()
    .to_owned();
    let shim = bin_dir.join("git");
    fs::write(&shim, format!("#!/bin/sh\n{script}\nexec '{real}' \"$@\"\n")).expect("shim を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("shim に実行権を付ける");
    format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default())
}

/// PASS の gate まで通した便を作る。
pub(super) fn gated_pass(repo: &Path, state: &Path, design: &str, marker: &Path) -> String {
    let id = implemented(repo, state, design);
    let lens = fake_lens(marker, &lens_verdict("PASS"));
    let out = gate_once(repo, state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS の gate は rc 0: {}", stderr_of(&out));
    id
}

/// `verify.jsonl` の record を全部読む。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn verify_rows(state: &Path, id: &str) -> Vec<Vec<(String, vessel::fleet::json_lite::Value)>> {
    let log = fs::read_to_string(state.join("pipe").join(id).join("verify.jsonl"))
        .expect("verify.jsonl を読める");
    log.lines()
        .filter_map(|line| vessel::fleet::json_lite::parse_object(line.trim()).ok())
        .collect()
}

/// n 番目（1 始まり）の record の 1 値。
pub(super) fn row_value(rows: &[Vec<(String, vessel::fleet::json_lite::Value)>], n: usize, key: &str) -> String {
    rows.get(n.saturating_sub(1)).map_or_else(String::new, |row| value_of(row, key))
}

/// spawn を 1 回撃つ（runner を選ぶ形）。
pub(super) fn spawn_with(repo: &Path, state: &Path, id: &str, runner: &str) -> Output {
    run_pipe(&[
        "spawn", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ])
}

// ── (c) 承認 Blocked と resume（設計 §8 (c)・FR15 / FR16 / AC5・憲法 A1 / C7.2） ──

/// event log の全行を型で読む（file が無ければ空）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn events(state: &Path) -> Vec<Event> {
    fs::read_to_string(state.join("fleet").join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| Event::from_line(line).expect("event の 1 行を読める"))
        .collect()
}

// ── (e) 到達点の計測（設計 §8 (e)・§9・FR22 / AC1 / AC2・憲法 A1） ──────────

/// `pipe report` を 1 回撃つ。
pub(super) fn report_once(state: &Path) -> Output {
    run_pipe(&["report", "--state-dir", &state.display().to_string()])
}

/// commit を 1 本作る fake runner の 1 行。
pub(super) const TOY_COMMIT: &str = "echo x >> src/lib.rs && git add -A && git commit -q -m runner";

// ── 質問の口 (a)（設計 docs/design/pipeline-question.md §8 (a)・SRS FR31 / FR32） ───────

/// runner が最終行に書く質問 record。
pub(super) const QUESTION_RECORD: &str = r#"{"question":"verify 行が矛盾する","about":"verify"}"#;

/// record を stdout の最終行に書いて rc 76 で終える fake runner（commit は作らない）。
pub(super) fn question_runner() -> String {
    format!("echo before; printf '%s\\n' '{QUESTION_RECORD}'; exit 76")
}

/// intake → spawn で質問に倒した便の id を返す。
pub(super) fn questioned(repo: &Path, state: &Path) -> String {
    let path = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &path);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &question_runner(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "質問は rc 3 で止まる: {}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains(&format!("stage=Questioned question={id}")),
        "判定行に question=<id>: {}",
        stdout_of(&out)
    );
    id
}

/// 便の event を `(kind, stage, detail)` の列にする。
pub(super) fn trail(state: &Path, id: &str) -> Vec<(EventKind, Option<Stage>, Option<String>)> {
    events(state)
        .into_iter()
        .filter(|event| event.run == id)
        .map(|event| (event.kind, event.stage, event.detail))
        .collect()
}

// ────────── 入口の write-set 排他と `stop --run`（設計 pipeline-conflict.md §2・ADR-0019 §2.1） ──────────

/// event log の **byte 列**（行数では追記の中身の差が消えるので byte で比べる）。
pub(super) fn events_bytes(state: &Path) -> Vec<u8> {
    fs::read(state.join("fleet").join("events.jsonl")).unwrap_or_default()
}

/// write-set だけを差し替えた行を id つきで書いて commit し、pointer を返す（1 便 1 行）。
pub(super) fn write_set_contract(repo: &Path, id: &str, entries: &[&str]) -> String {
    let quoted: Vec<String> = entries.iter().map(|item| format!("\"{item}\"")).collect();
    let fields: Vec<String> = contract_body()
        .into_iter()
        .map(|line| match line.split_once(" =").map(|(key, _)| key) {
            Some("write-set") => format!("write-set = [{}]", quoted.join(", ")),
            Some("id") => format!("id = \"{id}\""),
            _ => line,
        })
        .collect();
    commit_row(repo, &fields);
    format!("{DESIGN_FILE}#{id}")
}

/// intake を 1 回撃つ（**rc を測らない**＝断られる周の歯が使う・[`intake_raw`] と同じ形）。
pub(super) fn try_intake(repo: &Path, state: &Path, design: &str, bead: &str) -> Output {
    intake_raw(repo, state, design, bead)
}

/// 便の `RunStage` のうち段が `want` の件数。
pub(super) fn stage_count(state: &Path, id: &str, want: Stage) -> usize {
    stages(state, id).iter().filter(|(stage, _)| *stage == Some(want)).count()
}

// ───── 追随の衝突を runner が解く（`s2-07l.146`・ADR-0019 §2.2 / §2.4 / §2.6・接頭辞 `pipe_follow_`） ─────

/// 偽 runner の置き場（呼出回数と turn ごとの stdin）。
pub(super) fn stub_dir(state: &Path) -> PathBuf {
    state.join("stub")
}

/// 偽 runner が起こされた回数（file が無ければ 0）。**「起こされなかった」を効果で測る**面である。
pub(super) fn stub_calls(state: &Path) -> usize {
    fs::read_to_string(stub_dir(state).join("calls"))
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

/// n turn 目（1 始まり）に渡された stdin の全文（無ければ空）。
pub(super) fn stub_stdin(state: &Path, turn: usize) -> String {
    fs::read_to_string(stub_dir(state).join(format!("stdin-{turn}"))).unwrap_or_default()
}

/// turn 1 の既定の本文: 契約の実装（`src/lib.rs` の末尾へ `x` を足して commit）。
pub(super) const IMPLEMENT: &str = "printf 'x\\n' >> src/lib.rs\ngit add -A\ngit commit -q -m runner\nexit 0";

/// 便の `RunStage` の `(段, detail)` の列。
pub(super) fn stages(state: &Path, id: &str) -> Vec<(Option<Stage>, Option<String>)> {
    trail(state, id)
        .into_iter()
        .filter(|(kind, _, _)| *kind == EventKind::RunStage)
        .map(|(_, stage, detail)| (stage, detail))
        .collect()
}

/// `sh` と `git` だけを引ける PATH（`systemd-run` の**無い** host を作る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn lean_path(state: &Path) -> String {
    let bin_dir = state.join("lean-bin");
    fs::create_dir_all(&bin_dir).expect("lean dir を作れる");
    for name in ["sh", "git"] {
        let found = Command::new("sh")
            .args(["-c", &format!("command -v {name}")])
            .output()
            .expect("command -v を撃てる");
        let real = String::from_utf8_lossy(&found.stdout).trim().to_owned();
        assert!(!real.is_empty(), "{name} を引ける");
        std::os::unix::fs::symlink(&real, bin_dir.join(name)).ok();
    }
    bin_dir.display().to_string()
}

/// 埋め込み manifest の整数 1 行（封じ込めの値は `--rules` の override を通らない）。
#[expect(
    clippy::panic,
    reason = "統合 test の helper。clippy の allow-panic-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn embedded_int(id: &str) -> u64 {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => panic!("埋め込み manifest が拒まれた: {errors:?}"),
    };
    match manifest.get(id).map(|row| row.value.clone()) {
        Some(RuleValue::Int(found)) => found,
        other => panic!("{id} は整数の行のはず: {other:?}"),
    }
}

/// PATH を差し替えて `pipe` を 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn run_pipe_with_path(path: &str, args: &[&str]) -> Output {
    bin_cmd()
        .arg("pipe")
        .args(args)
        .env("PATH", path)
        .output()
        .expect("binary を起動できる")
}

// ───── 歯の cwd を repo の外に固定する（`s2-07l.381`・設計 pipeline.md §28・接頭辞 `pipe_hermetic_`） ─────

/// `--state-dir` も `--repo` も無い `pipe show` の断り: 器は cwd を読まず「`--repo` が要る」で rc 1 になる（設計
/// pipeline.md §15・`s2-07l.310` で cwd の fallback を落とした）。cwd を読む変異は [`bin_cmd`] の cwd（git repo
/// でない）で「repo の root を解決できない」になり、この字面から外れる。
fn assert_show_refused_without_repo(out: &Output, helper: &str) {
    let err = stderr_of(out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{helper}: flag 不在は rc 1 で断る: {err}");
    assert!(err.contains("--repo が要る"), "{helper}: cwd を読まず flag 不在で断る: {err}");
}

/// (2) 親の helper `run_pipe` は cwd を repo の外に固定して起こす。
#[test]
fn pipe_hermetic_run_pipe_refuses_show_without_repo_or_state_dir() {
    assert_show_refused_without_repo(&run_pipe(&["show", "--run", "x"]), "run_pipe");
}

/// (2) 親の helper `run_pipe_with_path` は PATH を差し替えても cwd を repo の外に固定して起こす。
#[test]
fn pipe_hermetic_run_pipe_with_path_refuses_show_without_repo_or_state_dir() {
    let path = std::env::var("PATH").unwrap_or_default();
    assert_show_refused_without_repo(&run_pipe_with_path(&path, &["show", "--run", "x"]), "run_pipe_with_path");
}

/// (2) 親の helper `land_once_with_git_shim` の起こす口（[`run_pipe_with_git_shim`]・偽 git を PATH の先頭に置く）も
/// cwd を repo の外に固定して起こす。verb と `--repo` / `--state-dir` は `land_once_with_git_shim` が固定するので、
/// `show` を flag 無しで撃つのはその 1 段下の同じ口で測る。`failing` は `rev-parse --show-toplevel` に当たらない
/// 字面（当たる字面だと repo の中でも git が落ちて同じ断りになり、歯が空虚になる）。
#[test]
fn pipe_hermetic_land_once_with_git_shim_refuses_show_without_repo_or_state_dir() {
    let state = tmp();
    let out = run_pipe_with_git_shim(&state, " diff --name-only -z ", &["show", "--run", "x"]);
    assert_show_refused_without_repo(&out, "land_once_with_git_shim");
    clean(&[&state]);
}

/// (3) 置き場の pin: tracked の 9 file（この file と `pipe/` 配下）で binary を起こす字面 2 形の出現の合計は
/// **1**（[`bin_cmd`] の中の 1 箇所）。母集団は読んだ file 数 9 と base の 41 site（設計 §28 の census）で、
/// 同時に出す。字面は `concat!` で割って持つ（この歯の本文が自分の母集団に数えられないため）。
#[test]
fn pipe_hermetic_sites_stay_one() {
    const BASE_SITES: usize = 41;
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tracked: Vec<String> = git(&crate_dir, &["ls-files", "--", "tests/e2e/pipe.rs", "tests/e2e/pipe"])
        .lines()
        .map(str::to_owned)
        .collect();
    let needles = [concat!("Command::new(", "bin())"), concat!("Command::new(", "super::bin())")];
    let sites: usize = tracked
        .iter()
        .map(|path| fs::read_to_string(crate_dir.join(path)).expect("tracked の file を読める"))
        .map(|text| needles.iter().map(|needle| text.matches(needle).count()).sum::<usize>())
        .sum();
    assert_eq!(
        (tracked.len(), sites),
        (9, 1),
        "母集団: file {} 本（base 9）・site {sites}（base {BASE_SITES}）・残るのは bin_cmd の 1 箇所: {tracked:?}",
        tracked.len()
    );
}

// ───── e2e の歯の道具箱（設計 gate-cost.md §30・行 v・`s2-07l.504`・接頭辞 `e2e_toolbox_`） ─────

/// (a) 約束 1 と 2(i): [`run_pipe`] で toy repo の gate を 1 本撃つと、**道具箱の記録 dir** に共通 verify の
/// scope の記録が在り、その引数に `--scope` と `MemoryMax=` が在る。
///
/// 母集団は記録 dir の全件（`crate::toolbox_record` が「ちょうど 1 件」を要求する）——件数を確かめずに
/// `contains` すると、probe や別の段の起動の引数で assert が充足する。
#[test]
fn e2e_toolbox_run_pipe_confines_the_common_verify_line() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &design);
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "gate は rc 0: {}", stderr_of(&out));
    let names = crate::toolbox_record_names(&state);
    let record = crate::toolbox_record(&state, "-common-");
    assert!(record.lines().any(|line| line == "--scope"), "scope の包みである（母集団 {names:?}）: {record}");
    assert!(
        record.lines().any(|line| line.starts_with("MemoryMax=")),
        "箱の大きさを渡している（母集団 {names:?}）: {record}"
    );
    assert_eq!(row_value(&verify_rows(&state, &id), 2, "confined"), "true", "共通 verify は包めた周で撃たれた");
    clean(&[&repo, &state]);
}

/// (b) 約束 2(iii): 直起動の 6 か所と同じ形（[`pipe_cmd`] で組んで**子として背景で起こす**便）で撃った周も、
/// 同じ記録 dir に runner の scope の記録が残る。
///
/// 口を `Command` で自分で組む呼び手が PATH を組み直す形だと、この記録が 0 件になる。
#[test]
fn e2e_toolbox_background_child_uses_the_same_toolbox() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &design);
    let child = pipe_cmd(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("背景で起こせる");
    let out = child.wait_with_output().expect("背景の便の出力を読める");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));
    let names = crate::toolbox_record_names(&state);
    let record = crate::toolbox_record(&state, "-runner-");
    assert!(record.lines().any(|line| line == "--scope"), "runner も包めた（母集団 {names:?}）: {record}");
    clean(&[&repo, &state]);
}

/// (d) 約束 5 の否定の枝: [`lean_path`] の PATH（`systemd-run` の**無い** host）で同じ gate を撃つ周は、
/// 道具箱の記録が **1 件も増えず**、record は `confined=false` / `reason=no-systemd-run` のままである。
///
/// 母集団は撃つ前の記録の名の列（`implemented` が既に何件か作っている）で、**差**で測る。
#[test]
fn e2e_toolbox_lean_path_adds_no_record_and_stays_unconfined() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &design);
    let before = crate::toolbox_record_names(&state);
    assert!(!before.is_empty(), "母集団: 道具箱は既に記録を作っている");
    let marker = state.join("lens-ran");
    let out = run_pipe_with_path(&lean_path(&state), &[
        "gate", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--lens", &fake_lens(&marker, &lens_verdict("PASS")),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "包めない host でも便は流れる: {}", stderr_of(&out));
    assert_eq!(crate::toolbox_record_names(&state), before, "明示の口は道具箱を通らない（記録は増えない）");
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 2, "confined"), "false", "包めていない");
    assert_eq!(row_value(&rows, 2, "reason"), "no-systemd-run", "理由は閉じた enum の名");
    clean(&[&repo, &state]);
}
