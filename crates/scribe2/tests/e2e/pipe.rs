//! 縦 1 本 (a) / (b) の歯（設計 docs/design/pipeline.md §8 (a) / (b)）。
//!
//! tmp の git repo を作り `vessel init --state-dir` で置き場を紐づけてから撃つ。
//! runner は `sh -c` の 1 行の fake で、実 Claude は (d) の手番である。
//! commit には identity が要るので **repo local** の設定を与える（global は触らない）。

use crate::make_tmp_dir;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
use vessel::fleet::{Event, EventKind, Stage};
use vessel::hook::inject_path;
use vessel::order::is_declaration_order;
use vessel::pipe::approve::RC_BLOCKED;
use vessel::pipe::gate::{CHECKS, RC_INCONCLUSIVE, VERDICTS};
use vessel::pipe::land;
use vessel::rules::manifest::Manifest;
use vessel::rules::RuleValue;

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

/// git を 1 回撃ち、rc 0 を要求して stdout を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn git(dir: &Path, args: &[&str]) -> String {
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
const VESSEL_ALLOWED: &str = r#"["git", "sh"]"#;

/// toy repo の宣言の共通 verify。**撃つのは別便**（.57）で、ここでは形だけを持つ。
const VESSEL_COMMON: &str = r#"["git rev-parse --verify {base}"]"#;

/// vessel 宣言を repo の root へ書く（commit は呼び手が行う）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_vessel(repo: &Path, allowed: &str, common: &str) {
    let body = format!("schema = 1\nallowed-commands = {allowed}\ncommon-verify = {common}\n");
    fs::write(repo.join(".vessel.toml"), body).expect("宣言を書ける");
}

/// 宣言を書き換えて commit する（**HEAD の tree が intake の読み面**である）。
fn commit_vessel(repo: &Path, allowed: &str, common: &str) {
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
fn write_verify_scripts(repo: &Path) {
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
fn repo_with_state() -> (PathBuf, PathBuf) {
    // **置き場は tmp root の 1 段下**（`<tmp>/state`）。host の受付札は `<state_dir の親>` から
    // 導くので、tmp root の直下に置くと全 test が同じ slot dir を共有して flaky になる。
    repo_with_state_in(&tmp().join(STATE_LEAF))
}

/// 置き場の leaf 名（[`repo_with_state`] と [`clean`] が共有する）。
const STATE_LEAF: &str = "state";

/// commit を 1 つ持つ tmp の git repo と、**指名した path** に紐づけた置き場を作る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn repo_with_state_in(state: &Path) -> (PathBuf, PathBuf) {
    fs::create_dir_all(state).expect("置き場を作れる");
    let repo = tmp();
    // 設計 §5.4 の land は `refs/heads/main` を進める。`git init` の既定 branch 名は
    // 環境依存（多くの host で `master`）なので、**test 側で main を明示する**。
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "e2e"]);
    git(&repo, &["config", "user.email", "e2e@example.invalid"]);
    fs::create_dir_all(repo.join("src")).expect("src dir を作れる");
    fs::write(repo.join("src").join("lib.rs"), "// seed\n").expect("seed を書ける");
    // **宣言も marker と一緒に commit する**（intake は HEAD の tree から読む＝作業ツリー
    // に置いただけの宣言は無いのと同じ・設計 §8）。
    write_vessel(&repo, VESSEL_ALLOWED, VESSEL_COMMON);
    write_verify_scripts(&repo);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "seed"]);
    let out = Command::new(bin())
        .args(["vessel", "init", "--state-dir"])
        .arg(state)
        .arg(&repo)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "vessel init は rc 0");
    (repo, state.to_path_buf())
}

/// `pipe` を binary で 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_pipe(args: &[&str]) -> Output {
    Command::new(bin())
        .arg("pipe")
        .args(args)
        .output()
        .expect("binary を起動できる")
}

/// 正しく書けた契約 file の本文。差し替えたい行だけ上書きして使う。
fn contract_body() -> Vec<String> {
    [
        r#"goal = "縦 1 本を通す""#,
        r#"done = "run が Implemented になる""#,
        r#"size = "S""#,
        r#"owner = "s2-2e5""#,
        r#"disposition = "A-now""#,
        r#"write-set = ["src/lib.rs"]"#,
        r#"verify = ["sh verify-ok.sh"]"#,
        r#"req = ["FR4"]"#,
        r#"design = "docs/design/pipeline.md""#,
    ]
    .iter()
    .map(|line| (*line).to_owned())
    .collect()
}

/// 契約 file を書き、その path を返す。`drop` の行を落とし `add` の行を足す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_contract(dir: &Path, drop: &[&str], add: &[&str]) -> PathBuf {
    let mut lines: Vec<String> = contract_body()
        .into_iter()
        .filter(|line| !drop.iter().any(|key| line.starts_with(key)))
        .collect();
    lines.extend(add.iter().map(|line| (*line).to_owned()));
    let path = dir.join("contract.toml");
    fs::write(&path, format!("{}\n", lines.join("\n"))).expect("契約 file を書ける");
    path
}

/// stdout の全文。
fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// stderr の全文。
fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// `run=<id>` の 1 行から id を取る。
fn run_id_of(out: &Output) -> String {
    stdout_of(out)
        .lines()
        .find_map(|line| line.strip_prefix("run="))
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_owned)
        .unwrap_or_default()
}

/// intake を 1 回通して run id を返す。
fn intake(repo: &Path, state: &Path, contract: &Path) -> String {
    intake_bead(repo, state, contract, "s2-2e5")
}

/// bead を選んで intake を 1 回通す。**run id は `<bead>-<秒>`** なので、同じ秒に
/// 2 便を起こす歯は bead を分ける（同 bead だと id が衝突して 2 便目が断られる）。
fn intake_bead(repo: &Path, state: &Path, contract: &Path, bead: &str) -> String {
    let rules = ceiling_rules(state);
    let out = run_pipe(&[
        "intake",
        "--contract",
        &contract.display().to_string(),
        "--bead",
        bead,
        "--repo",
        &repo.display().to_string(),
        "--state-dir",
        &state.display().to_string(),
        "--rules",
        &rules,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "intake は rc 0: {}", stderr_of(&out));
    run_id_of(&out)
}

/// 測り終えた便を `stop --run` で外す（rc 0 を要求する）。
///
/// 入口の排他（`s2-07l.145`・ADR-0019 §2.1）が在るので、**終端でない便**が置き場に残ったまま
/// 同じ write-set の 2 本目を intake することはできない。1 つの置き場で 2 便を順に測る歯は、
/// 前の便をこの口で外してから次を起こす。
fn stop_run_ok(state: &Path, id: &str) {
    let out = run_pipe(&["stop", "--run", id, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop --run: {}", stderr_of(&out));
}

/// 後片付け。[`STATE_LEAF`] の置き場は tmp root ごと畳む（host の slot dir も同じ root に在る）。
fn clean(dirs: &[&Path]) {
    for dir in dirs {
        let root = dir
            .parent()
            .filter(|_| dir.file_name().is_some_and(|name| name == STATE_LEAF))
            .unwrap_or(dir);
        fs::remove_dir_all(root).ok();
    }
}

#[test]
fn pipe_intake_rejects_missing_field() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["goal", "size"], &[]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "欠落を通さない");
    let err = stderr_of(&out);
    // **全件集めて返す**（1 件目で止めない）。落とした 2 本がどちらも出る。
    assert!(err.contains("goal"), "goal の欠落: {err}");
    assert!(err.contains("size"), "size の欠落: {err}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_rejects_multiline_verify() {
    let (repo, state) = repo_with_state();
    let path = repo.join("multiline.toml");
    let mut lines = contract_body()
        .into_iter()
        .filter(|line| !line.starts_with("verify"))
        .collect::<Vec<String>>();
    lines.push("verify = [".to_owned());
    lines.push(r#"  "true","#.to_owned());
    lines.push("]".to_owned());
    fs::write(&path, format!("{}\n", lines.join("\n"))).expect("契約 file を書ける");
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "1 行で完結しない verify を通さない");
    assert!(
        stderr_of(&out).contains("1 行で完結"),
        "理由は行を跨いだことである: {}",
        stderr_of(&out)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_rejects_contract_without_req_or_design() {
    let (repo, state) = repo_with_state();
    for (drop, add, want) in [
        (vec!["req"], vec![], "req"),
        (vec!["design"], vec![], "design"),
        (vec!["req"], vec![r#"req = []"#], "req"),
    ] {
        let path = write_contract(&repo, &drop, &add);
        let out = run_pipe(&[
            "intake", "--contract", &path.display().to_string(), "--bead", "b",
            "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        ]);
        assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "{want} が無い契約を通さない");
        assert!(stderr_of(&out).contains(want), "{want}: {}", stderr_of(&out));
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_records_run_in_fleet() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let stamp = id.strip_prefix("s2-2e5-").unwrap_or_default();
    assert!(!stamp.is_empty(), "run id は <bead>-<stamp>: {id}");
    // id は dir 名と branch 名になるので、stamp 側に : も - も残っていない。
    assert!(!stamp.contains(':'), "stamp に : が残る: {id}");
    assert!(!stamp.contains('-'), "stamp に - が残る: {id}");

    // 契約 file の写しが置き場に在る（process 間で持ち越す面）。
    assert!(
        state.join("pipe").join(&id).join("contract.toml").exists(),
        "契約の写しが置き場に在る"
    );
    // 現在地は event log から読める。
    let out = run_pipe(&["show", "--run", &id, "--state-dir", &state.display().to_string(),
                         "--repo", &repo.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "show は rc 0");
    assert!(stdout_of(&out).contains("stage=Intake"), "{}", stdout_of(&out));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_spawn_creates_worktree_and_records_implemented() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("stage=Implemented"), "{}", stdout_of(&out));
    assert!(
        repo.join(".worktrees").join("scribe2").join(&id).exists(),
        "worktree を切る"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_spawn_marks_failed_when_runner_makes_no_commit() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // rc 0 でも commit が 0 本なら完了ではない。
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", "true",
    ]);
    assert!(stdout_of(&out).contains("stage=Failed"), "{}", stdout_of(&out));
    let shown = run_pipe(&["show", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert!(stdout_of(&shown).contains("stage=Failed"), "永続面にも Failed が残る");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_spawn_writes_write_set_into_git_dir() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["write-set"], &[r#"write-set = ["src/lib.rs", "docs/"]"#]);
    let id = intake(&repo, &state, &path);
    run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", "true",
    ]);
    let worktree = repo.join(".worktrees").join("scribe2").join(&id);
    let git_dir = PathBuf::from(git(&worktree, &["rev-parse", "--absolute-git-dir"]));
    let policy = git_dir.join("scribe2").join("write-set.txt");
    let body = fs::read_to_string(&policy).expect("policy を読める");
    assert_eq!(body, "src/lib.rs\ndocs/\n", "1 行 1 path で guard が読む形: {body:?}");
    // tracked 面を汚さない。
    assert!(
        git(&worktree, &["status", "--porcelain"]).is_empty(),
        "policy は git status に出ない"
    );
    clean(&[&repo, &state]);
}

// flip-check: retroactive s2-07l.49
#[test]
fn pipe_spawn_substitutes_placeholders_and_adds_no_env() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let runner = "env > env.txt && printf '%s\\n%s\\n%s\\n%s\\n' \
                  {run} {base} {contract} {write_set} > subst.txt && \
                  git add -A && git commit -q -m runner";
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let worktree = repo.join(".worktrees").join("scribe2").join(&id);

    // 器が足した env は**親（この test process）の env との差分**で測る。母集団を
    // 「env.txt の当該接頭辞の行」だけに取ると、親が既に持っていた変数（器の binary を
    // env で指した shell から撃つ周）を器が足したものと弁別できず、**歯が親の環境で
    // 落ちる**——測っているのは「器が足したか」であって「その名の変数が在るか」ではない。
    const OURS: &str = "SCRIBE2_";
    let env_text = fs::read_to_string(worktree.join("env.txt")).expect("env の写しを読める");
    let parent: BTreeSet<String> = std::env::vars()
        .map(|(key, _)| key)
        .filter(|key| key.starts_with(OURS))
        .collect();
    let child: BTreeSet<String> = env_text
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, _)| key.to_owned())
        .filter(|key| key.starts_with(OURS))
        .collect();
    assert_eq!(
        child, parent,
        "器固有の env を 1 つも足さない（母集団 {} 行 / 親 {parent:?} / 子 {child:?}）",
        env_text.lines().count()
    );

    let subst = fs::read_to_string(worktree.join("subst.txt")).expect("置換の写しを読める");
    let lines: Vec<&str> = subst.lines().collect();
    assert_eq!(lines.first().copied(), Some(id.as_str()), "{{run}}: {subst}");
    assert_eq!(lines.get(1).copied(), Some(base.as_str()), "{{base}}: {subst}");
    assert!(
        lines.get(2).is_some_and(|line| line.ends_with("contract.toml")),
        "{{contract}}: {subst}"
    );
    assert!(
        lines.get(3).is_some_and(|line| line.ends_with("write-set.txt")),
        "{{write_set}}: {subst}"
    );
    clean(&[&repo, &state]);
}

/// 写しの照合に使う plugin manifest の本文。
const PLUGIN_JSON: &str = "{\"name\":\"toy-plugin\"}\n";

/// 写しの照合に使う hooks の本文。**plugin.json と字面を変える**のは、片方だけを
/// 写す実装でも bytes 一致が通ってしまうのを防ぐためである。
const HOOKS_JSON: &str = "{\"hooks\":{\"PreToolUse\":[]}}\n";

/// commit **後**に anchor の working tree だけを書き換える本文。
///
/// 写し元が worktree（＝便の base）か anchor の現在値かを弁別する negative である。
/// これが無いと、写し元を anchor に差し替える退行が歯を素通りする。
const PLUGIN_JSON_DIRTY: &str = "{\"name\":\"dirty-anchor\"}\n";

/// plugin（`.claude-plugin/` と `hooks/`）を持つ toy repo と置き場を作る。
///
/// `README.md` も置くのは、**写しに worktree の他の file が混ざらない**ことを負例で
/// 測るためである（plugin の 2 dir だけを写す、が契約）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn repo_with_plugin() -> (PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    fs::create_dir_all(repo.join(".claude-plugin")).expect(".claude-plugin を作れる");
    fs::write(repo.join(".claude-plugin").join("plugin.json"), PLUGIN_JSON)
        .expect("plugin.json を書ける");
    fs::create_dir_all(repo.join("hooks")).expect("hooks dir を作れる");
    fs::write(repo.join("hooks").join("hooks.json"), HOOKS_JSON).expect("hooks.json を書ける");
    // plugin dir の**中**の symlink（写してはならない entry）。
    std::os::unix::fs::symlink("../README.md", repo.join("hooks").join("outside.json"))
        .expect("hooks の中に symlink を置ける");
    fs::write(repo.join("README.md"), "# toy\n").expect("README を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "plugin"]);
    // **commit の後**に anchor 側だけを汚す。便の worktree は base の checkout なので
    // 写しがこの本文になったら、写し元が worktree でなく anchor である証拠になる。
    fs::write(repo.join(".claude-plugin").join("plugin.json"), PLUGIN_JSON_DIRTY)
        .expect("anchor の plugin.json を汚せる");
    (repo, state)
}

#[test]
fn pipe_spawn_copies_plugin_outside_worktree_and_substitutes_plugin_dir() {
    let (repo, state) = repo_with_plugin();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // 前の周の写しが残っている状態を作る（run dir は intake が作る）。写し先を先に
    // 空にしないと、この file が **古い plugin** として runner に載ったままになる。
    let stale = state.join("pipe").join(&id).join("plugin");
    fs::create_dir_all(&stale).expect("古い写しの dir を作れる");
    fs::write(stale.join("stale.json"), "{}\n").expect("古い写しを置ける");
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "printf '%s' {plugin_dir} > plugin_dir.txt && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));

    let worktree = repo.join(".worktrees").join("scribe2").join(&id);
    let plugin = state.join("pipe").join(&id).join("plugin");
    let shown = fs::read_to_string(worktree.join("plugin_dir.txt")).expect("置換の写しを読める");
    // **これが本題**: runner が受け取った plugin dir が repo の外に在るから、worktree の
    // file が Claude Code の sensitive 判定（plugin dir 配下）に掛からない。**先に本題を
    // 測る**——先に path の一致を測ると、worktree の path を渡す退行も「一致しない」でしか
    // 落ちず、何が壊れたのかが読めない。
    assert!(
        !Path::new(&shown).starts_with(&repo),
        "runner が受けた plugin dir は repo の配下でない: shown={shown} repo={}",
        repo.display()
    );
    assert_eq!(shown, plugin.display().to_string(), "{{plugin_dir}} は run dir 配下の写し");

    for (dir, name, body) in [
        (".claude-plugin", "plugin.json", PLUGIN_JSON),
        ("hooks", "hooks.json", HOOKS_JSON),
    ] {
        let source = fs::read(worktree.join(dir).join(name)).expect("worktree 側を読める");
        let copied = fs::read(plugin.join(dir).join(name)).expect("写しを読める");
        assert_eq!(copied, source, "{dir}/{name} の bytes が worktree と一致する");
        assert_eq!(copied, body.as_bytes(), "{dir}/{name} は toy repo に置いた本文");
    }
    // 写し元は **worktree**（便の base）であって anchor の現在値ではない。
    assert_ne!(
        fs::read(plugin.join(".claude-plugin").join("plugin.json")).expect("写しを読める"),
        PLUGIN_JSON_DIRTY.as_bytes(),
        "anchor の未 commit な plugin.json を載せない"
    );
    // 内側の entry の symlink は写さない（`hooks/outside.json` は repo の README を指す）。
    assert_eq!(
        dir_names(&plugin.join("hooks")),
        vec!["hooks.json".to_owned()],
        "plugin dir の中の symlink を写さない"
    );

    assert!(!plugin.join("README.md").exists(), "写しに worktree の README を入れない");
    assert!(!plugin.join("src").exists(), "写しに worktree の src を入れない");
    let names = dir_names(&plugin);
    assert_eq!(
        names,
        vec![".claude-plugin".to_owned(), "hooks".to_owned()],
        "写しは plugin の 2 dir だけ（母集団 {} entry）",
        names.len()
    );
    clean(&[&repo, &state]);
}

/// dir の entry 名を昇順で返す（写しの範囲を**集合で**測るための helper）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn dir_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("dir を読める")
        .map(|entry| entry.expect("entry を読める").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// plugin の dir **自体が symlink** の repo では、その dir を写さない。
///
/// `Path::is_dir()` は link を辿るので、判定を `symlink_metadata` にしないと link 先の
/// 木を丸ごと写す（`hooks -> ../..` なら worktree 全体が写しに混ざる）。
#[test]
fn pipe_spawn_skips_plugin_dir_that_is_a_symlink() {
    let (repo, state) = repo_with_state();
    fs::create_dir_all(repo.join(".claude-plugin")).expect(".claude-plugin を作れる");
    fs::write(repo.join(".claude-plugin").join("plugin.json"), PLUGIN_JSON)
        .expect("plugin.json を書ける");
    fs::create_dir_all(repo.join("real-hooks")).expect("real-hooks を作れる");
    fs::write(repo.join("real-hooks").join("hooks.json"), HOOKS_JSON).expect("hooks.json を書ける");
    std::os::unix::fs::symlink("real-hooks", repo.join("hooks")).expect("hooks を link にできる");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "plugin-link"]);

    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));
    let plugin = state.join("pipe").join(&id).join("plugin");
    let names = dir_names(&plugin);
    assert_eq!(
        names,
        vec![".claude-plugin".to_owned()],
        "dir 自体が symlink の面は写さない（母集団 {} entry）",
        names.len()
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_spawn_makes_empty_plugin_dir_when_repo_has_none() {
    // plugin を持たない repo（toy repo の既定）でも便を止めない。
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));
    let plugin = state.join("pipe").join(&id).join("plugin");
    assert!(plugin.is_dir(), "空の plugin dir を作る: {}", plugin.display());
    let entries: Vec<String> = fs::read_dir(&plugin)
        .expect("写しの dir を読める")
        .map(|entry| entry.expect("entry を読める").file_name().to_string_lossy().into_owned())
        .collect();
    assert!(entries.is_empty(), "写すものが無ければ空（母集団 {} entry: {entries:?}）", entries.len());
    clean(&[&repo, &state]);
}

#[test]
fn pipe_spawn_refuses_wrong_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let one = run_pipe(&["spawn", "--run", &id, "--repo", &repo.display().to_string(),
                         "--state-dir", &state.display().to_string(), "--runner", "true"]);
    assert!(stdout_of(&one).contains("stage=Failed"), "1 回目で段が動く");
    // 段が Intake でなくなったので 2 回目は何もせず rc 1。
    let two = run_pipe(&["spawn", "--run", &id, "--repo", &repo.display().to_string(),
                         "--state-dir", &state.display().to_string(), "--runner", "true"]);
    assert_eq!(two.status.code(), Some(i32::from(RC_REFUSED)), "前提違反は rc 1");
    assert!(two.stdout.is_empty(), "前提違反では stdout 0 byte");
    assert_eq!(stderr_of(&two).lines().count(), 1, "stderr は 1 行: {}", stderr_of(&two));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_state_survives_process_restart() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    // 置き場を **--state-dir なしで** 解く＝repo に紐づいた git 設定から読む。
    let out = Command::new(bin())
        .args(["pipe", "intake", "--contract"])
        .arg(&path)
        .args(["--bead", "s2-2e5", "--repo"])
        .arg(&repo)
        .args(["--rules", &ceiling_rules(&state)])
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let id = run_id_of(&out);
    // **別 process** が同じ現在地を読む（process の記憶に何も置いていない）。
    let shown = Command::new(bin())
        .args(["pipe", "show", "--run", &id, "--repo"])
        .arg(&repo)
        .output()
        .expect("binary を起動できる");
    assert_eq!(shown.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&shown));
    assert!(stdout_of(&shown).contains("stage=Intake"), "{}", stdout_of(&shown));
    assert!(stdout_of(&shown).contains(&id), "{}", stdout_of(&shown));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_stop_all_rc0_when_nothing_to_stop() {
    let (repo, state) = repo_with_state();
    for _ in 0..2 {
        let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "対象なしは rc 0（冪等）");
        assert!(stdout_of(&out).contains("seats=0 stopped=0"), "{}", stdout_of(&out));
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_stop_all_terminates_live_runner() {
    let (repo, state) = repo_with_state();
    // **孫**として起こす。test process の子のままだと kill 後に zombie が残り、
    // `/proc/<pid>` が在るせいで「まだ生きている」と読めてしまう（実運用の席は
    // 別 process が起こすので init に引き取られる）。
    let spawned = Command::new("sh")
        .arg("-c")
        .arg("sleep 60 >/dev/null 2>&1 & echo $!")
        .output()
        .expect("fake runner を起こせる");
    let pid: u32 = String::from_utf8_lossy(&spawned.stdout)
        .trim()
        .parse()
        .expect("pid を読める");
    assert!(
        Path::new(&format!("/proc/{pid}")).exists(),
        "fake runner が動いている"
    );
    // 生きた席を event log に置く（spawn は runner の終了まで待つので、席が Live な
    // 周を作るには log 側から組む）。
    let record = Command::new(bin())
        .args([
            "fleet", "record", "--kind", "SeatSpawned", "--run", "r1", "--bead", "s2-2e5",
            "--seat", "r1", "--pid", &pid.to_string(), "--state-dir",
        ])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));

    let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "全部止まれば rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("seats=1 stopped=1"), "{}", stdout_of(&out));
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "runner の process は消えている"
    );
    // 2 回目は対象が無く rc 0（冪等）。
    let again = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "冪等");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_stop_returns_rc2_on_malformed_store() {
    let (repo, state) = repo_with_state();
    let events = state.join("fleet").join("events.jsonl");
    fs::create_dir_all(state.join("fleet")).expect("dir を作れる");
    fs::write(&events, "こわれ\n").expect("壊れた行を書ける");
    let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "state が読めない周は rc 2（rc 語彙の 3 値目）"
    );
    assert!(out.stdout.is_empty(), "rc 2 でも stdout は 0 byte");
    clean(&[&repo, &state]);
}

/// event log の行数（file が無ければ 0）。
fn event_count(state: &Path) -> usize {
    fs::read_to_string(state.join("fleet").join("events.jsonl"))
        .map(|text| text.lines().filter(|line| !line.is_empty()).count())
        .unwrap_or(0)
}

#[test]
fn pipe_refuses_without_writing_events() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let before = event_count(&state);
    // 段を進めてから、同じ段の前提を要る操作をもう一度撃つ。
    run_pipe(&["spawn", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--runner", "true"]);
    let settled = event_count(&state);
    let out = run_pipe(&["spawn", "--run", &id, "--repo", &repo.display().to_string(),
                         "--state-dir", &state.display().to_string(), "--runner", "true"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "前提違反は rc 1");
    assert_eq!(
        event_count(&state),
        settled,
        "**前提違反は event を 1 件も書かない**（intake 後 {before} → 実行後 {settled}）"
    );
    // 存在しない run も同じ（何も書かずに断る）。
    let missing = run_pipe(&["spawn", "--run", "no-such-run", "--repo", &repo.display().to_string(),
                             "--state-dir", &state.display().to_string(), "--runner", "true"]);
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "無い run は rc 1");
    assert_eq!(event_count(&state), settled, "無い run でも 1 件も書かない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_refuses_duplicate_run_id() {
    let (repo, state) = repo_with_state();
    let first = write_contract(&repo, &["write-set"], &[r#"write-set = ["src/A.rs"]"#]);
    let id = intake(&repo, &state, &first);
    // stamp は秒までなので、同じ秒の再 intake は id が衝突する。**黙って上書きしない**。
    let second = write_contract(&repo, &["write-set"], &[r#"write-set = ["src/B.rs"]"#]);
    let out = run_pipe(&[
        "intake", "--contract", &second.display().to_string(), "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
    ]);
    if out.status.code() == Some(i32::from(RC_OK)) {
        // 秒をまたいだ周は id が違う＝衝突していない。そのときは上書きが起きていない
        // ことだけを測る（時計に依存して flaky にしない）。
        assert_ne!(run_id_of(&out), id, "id が違うなら衝突していない");
    } else {
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "衝突は rc 1 で断る");
    }
    let kept = fs::read_to_string(state.join("pipe").join(&id).join("contract.toml"))
        .expect("最初の契約を読める");
    assert!(
        kept.contains("src/A.rs"),
        "最初の便の契約が別物に化けていない: {kept}"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_rejects_broken_arrays_and_unknown_keys() {
    let (repo, state) = repo_with_state();
    for (drop, add, want) in [
        // 区切り忘れを 1 本の壊れた文字列として受理しない。
        (vec!["verify"], vec![r#"verify = ["a" "b"]"#], "引用符 1 組の文字列でない"),
        (vec!["write-set"], vec![r#"write-set = []"#], "write-set は 1 本以上"),
        (vec!["verify"], vec![r#"verify = []"#], "verify は 1 本以上"),
        (vec![], vec![r#"nonsense = "x""#], "未知の key nonsense"),
        (vec![], vec![r#"classes = ["publish", "bogus"]"#], "未知の classes 値 bogus"),
    ] {
        let path = write_contract(&repo, &drop, &add);
        let out = run_pipe(&[
            "intake", "--contract", &path.display().to_string(), "--bead", "b",
            "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        ]);
        let err = stderr_of(&out);
        assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "{want} を通さない: {err}");
        assert!(err.contains(want), "理由に {want} が出る: {err}");
        assert!(err.contains("line="), "行番号を持つ: {err}");
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_reports_broken_value_without_claiming_absence() {
    let (repo, state) = repo_with_state();
    // 値が壊れているだけで key は書かれている。「無い」と二重に言わない。
    // 文字列 key を壊すと値が 1 つも取れない＝「書かれていた」を別に覚えていないと
    // 欠落として二重に報告される。
    let path = write_contract(&repo, &["goal"], &[r#"goal = 1"#]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    let err = stderr_of(&out);
    assert!(err.contains("goal の value が文字列でない"), "値の不備を言う: {err}");
    assert!(
        !err.contains("必須の key goal が無い"),
        "書かれている key を「無い」とは言わない: {err}"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_refuses_non_git_repo() {
    let (_repo, state) = repo_with_state();
    let bare = tmp();
    let path = write_contract(&bare, &[], &[]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &bare.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "git repo でなければ intake の時点で断る");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&bare, &state]);
}

/// intake を 1 回撃つ（rc を assert しない形）。
fn intake_raw(repo: &Path, state: &Path, contract: &Path, bead: &str) -> Output {
    let rules = ceiling_rules(state);
    run_pipe(&[
        "intake", "--contract", &contract.display().to_string(), "--bead", bead,
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules,
    ])
}

/// 便の vessel の写し。
fn vessel_copy(state: &Path, id: &str) -> PathBuf {
    state.join("pipe").join(id).join("vessel.toml")
}

/// 宣言が無い repo では便を起こさない（ADR-0010 §2.3）。**作業ツリーに置いただけの
/// 宣言も無いのと同じ**である——宣言の変更は対象 repo の PR として review を通る。
#[test]
fn pipe_intake_refuses_without_vessel_declaration() {
    let (repo, state) = repo_with_state();
    git(&repo, &["rm", "-q", ".vessel.toml"]);
    git(&repo, &["commit", "-q", "-m", "drop-vessel"]);
    let path = write_contract(&repo, &[], &[]);
    let out = intake_raw(&repo, &state, &path, "b");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "宣言が無ければ rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains(".vessel.toml"), "何が無いかを名指す: {}", stderr_of(&out));
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "断った周は run dir も作らない");

    // **未 commit の宣言は読まない**（作業ツリーへ置くだけでは効かない）。
    write_vessel(&repo, VESSEL_ALLOWED, VESSEL_COMMON);
    let again = intake_raw(&repo, &state, &path, "b");
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "untracked の宣言は無いのと同じ: {}", stderr_of(&again));
    assert_eq!(event_count(&state), 0, "こちらも記帳しない");
    clean(&[&repo, &state]);
}

/// 宣言の allowlist は器の上限（`runner.allowed_commands`）の部分集合でなければならない。
#[test]
fn pipe_intake_refuses_declaration_outside_ceiling() {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, r#"["git", "sh", "curl"]"#, VESSEL_COMMON);
    let path = write_contract(&repo, &[], &[]);
    let out = intake_raw(&repo, &state, &path, "b");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "上限の外は rc 1: {err}");
    assert!(err.contains("allowed-commands の curl が上限"), "外れた command を理由の形で名指す: {err}");
    assert!(err.contains("runner.allowed_commands"), "上限の出所を名指す: {err}");
    assert!(err.contains("line="), "行番号を持つ: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&repo, &state]);
}

/// 共通 verify の先頭語の基準は**宣言の** allowlist である（上限ではない）。
#[test]
fn pipe_intake_refuses_common_verify_outside_declared_allowlist() {
    let (repo, state) = repo_with_state();
    // `cargo` は上限には在るが、この repo の宣言には無い。
    commit_vessel(&repo, r#"["git"]"#, r#"["cargo xtask check"]"#);
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["git status"]"#]);
    let out = intake_raw(&repo, &state, &path, "b");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "宣言の外は rc 1: {err}");
    assert!(err.contains("先頭 command cargo が"), "外れた先頭語を理由の形で名指す: {err}");
    assert!(err.contains("allowed-commands"), "基準が宣言であることを言う: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// repo の外を指す語（絶対 path・home の短縮記号・`..` で遡る path）を持つ行は撃たせない。
#[test]
fn pipe_intake_refuses_common_verify_with_absolute_path() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    // home の短縮記号は **2 文字を組み立てて**書く（paths-clean は tracked file の本文に
    // その字面が残ることを違反として数えるので、literal では書けない）。
    let home = format!("{}{}", '~', '/');
    for common in [
        r#"["git rev-parse --verify /etc/passwd"]"#.to_owned(),
        format!("[\"git config --file {home}gitconfig list\"]"),
        // 先頭語が allowlist の内でも、引数が repo の外へ遡れば同じ穴である。
        r#"["git rev-parse --git-dir ../../../etc"]"#.to_owned(),
    ] {
        commit_vessel(&repo, VESSEL_ALLOWED, &common);
        let out = intake_raw(&repo, &state, &path, "b");
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{common} は rc 1: {err}");
        assert!(err.contains("repo の外"), "理由を名指す: {err}");
    }
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// 置ける穴は `{base}` だけ（閉じない `{` も穴として断る）。
#[test]
fn pipe_intake_refuses_common_verify_with_unknown_placeholder() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    for common in [
        r#"["git rev-parse --verify {head}"]"#,
        r#"["git rev-parse --verify {base"]"#,
    ] {
        commit_vessel(&repo, VESSEL_ALLOWED, common);
        let out = intake_raw(&repo, &state, &path, "b");
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{common} は rc 1: {err}");
        assert!(err.contains("置けない穴"), "理由を名指す: {err}");
    }
    // 弁別: **`{base}` は通る**（穴を丸ごと禁じているのではない）。
    commit_vessel(&repo, VESSEL_ALLOWED, VESSEL_COMMON);
    let ok = intake_raw(&repo, &state, &path, "b");
    assert_eq!(ok.status.code(), Some(i32::from(RC_OK)), "{{base}} は置ける: {}", stderr_of(&ok));
    clean(&[&repo, &state]);
}

/// **先頭語だけでは境界にならない**——gate と land は行を `sh -c` で撃つ（ADR-0010 §2.3）。
#[test]
fn pipe_intake_refuses_common_verify_with_shell_metachar() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    for common in [
        r#"["git --version; env"]"#.to_owned(),
        r#"["git --version | tee out"]"#.to_owned(),
        r#"["git --version && env"]"#.to_owned(),
        r#"["git $(env) --version"]"#.to_owned(),
        // 目に見えない制御文字も同じ扱い（改行を含む語彙は 1 行の値には書けない）。
        "[\"git --version\u{7}\"]".to_owned(),
    ] {
        commit_vessel(&repo, VESSEL_ALLOWED, &common);
        let out = intake_raw(&repo, &state, &path, "b");
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{common} は rc 1: {err}");
        assert!(err.contains("制御文字"), "理由を名指す: {err}");
    }
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// 読むのは **HEAD commit の tree** で、作業ツリーではない。
#[test]
fn pipe_intake_reads_declaration_from_head_not_worktree() {
    let (repo, state) = repo_with_state();
    let head = git(&repo, &["rev-parse", "HEAD"]);
    // 作業ツリーの宣言だけを**上限の外**へ広げる（commit しない）。
    write_vessel(&repo, r#"["git", "sh", "curl"]"#, VESSEL_COMMON);
    let path = write_contract(&repo, &[], &[]);
    let id = intake_bead(&repo, &state, &path, "s2-2e5");
    let copy = fs::read_to_string(vessel_copy(&state, &id)).expect("写しを読める");
    assert!(!copy.contains("curl"), "作業ツリーの宣言は読まない: {copy}");
    assert!(copy.contains(&format!("commit = \"{head}\"")), "出所は読んだ commit: {copy}");
    clean(&[&repo, &state]);
}

/// 契約の verify 行は穴を持てない（`{base}` も置けない）。
#[test]
fn pipe_intake_refuses_contract_verify_with_placeholder() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-ok.sh {base}"]"#]);
    let out = intake_raw(&repo, &state, &path, "b");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "契約行の穴は rc 1: {err}");
    assert!(err.contains("契約の verify"), "どちらの面かを言う: {err}");
    assert!(err.contains("置けない穴"), "理由を名指す: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&repo, &state]);
}

/// 契約の verify 行も**宣言の** allowlist で測る。空の行も撃てる形とは認めない。
#[test]
fn pipe_intake_refuses_contract_verify_outside_declared_allowlist() {
    let (repo, state) = repo_with_state();
    for (add, want) in [
        // `cargo` は上限には在るが toy repo の宣言には無い。
        (r#"verify = ["cargo xtask check"]"#, "allowed-commands"),
        // **完全一致**である（prefix 一致へ緩めると `gitk` が `git` で通る）。
        (r#"verify = ["gitk --all"]"#, "先頭 command gitk"),
        (r#"verify = [""]"#, "空"),
    ] {
        let path = write_contract(&repo, &["verify"], &[add]);
        let out = intake_raw(&repo, &state, &path, "b");
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{add} は rc 1: {err}");
        assert!(err.contains(want), "理由に {want} が出る: {err}");
    }
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// 有効値は便ごとに凍結され、以後 repo の宣言が動いても写しは変わらない（ADR-0010 §2.4）。
#[test]
fn pipe_intake_freezes_effective_vessel_copy() {
    let (repo, state) = repo_with_state();
    let head = git(&repo, &["rev-parse", "HEAD"]);
    let path = write_contract(&repo, &[], &[]);
    let id = intake_bead(&repo, &state, &path, "s2-2e5");
    let copy = vessel_copy(&state, &id);
    let before = fs::read_to_string(&copy).expect("写しを読める");
    assert!(before.contains(r#"allowed-commands = ["git", "sh"]"#), "宣言の値: {before}");
    assert!(before.contains(r#"common-verify = ["git rev-parse --verify {base}"]"#), "共通 verify: {before}");
    assert!(before.contains(&format!("commit = \"{head}\"")), "出所の commit: {before}");
    assert!(before.contains(r#"source = ".vessel.toml""#), "出所の path: {before}");
    assert!(before.contains(r#"ceiling = "runner.allowed_commands""#), "出所の上限行: {before}");
    // **便の後に宣言を commit ごと書き換えても写しは動かない**（自己拡張の閉塞）。
    commit_vessel(&repo, r#"["git"]"#, r#"["git status"]"#);
    assert_eq!(fs::read_to_string(&copy).expect("写しを読める"), before, "写しは凍結されている");
    clean(&[&repo, &state]);
}

/// **本 repo 自身の宣言**が、埋め込みの上限（`--rules` を渡さない周の manifest）の内側に在る。
///
/// suite の他の歯はすべて `--rules` の tmp manifest で上限を広げて通しているので、この 1 本が
/// 無いと「自己ホストの宣言が壊れた」周も CI は緑のまま通る（lens M3・2026-09-10）。
/// 併せて manifest の**上限の行**（`runner.allowed_commands`）を宣言が名乗ることを pin する
/// （共通 verify の行は `s2-07l.57` で廃止済みゆえ 2 面同文の pin は畳んだ）。
#[test]
fn pipe_intake_accepts_self_hosted_declaration_under_embedded_ceiling() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo の root を解ける");
    let declared = fs::read_to_string(root.join(".vessel.toml")).expect("自己ホストの宣言を読める");
    let (repo, state) = repo_with_state();
    fs::write(repo.join(".vessel.toml"), &declared).expect("宣言を写せる");
    git(&repo, &["add", "-f", ".vessel.toml"]);
    git(&repo, &["commit", "-q", "-m", "self-hosted"]);
    // **`--rules` を渡さない**＝埋め込みの上限で測る。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["git status"]"#]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_OK)),
        "自己ホストの宣言は埋め込みの上限の内側: {}",
        stderr_of(&out)
    );

    // 上限の行は残る（共通 verify の行は ADR-0010 §2.2 で廃止済み＝2 面同文の pin は畳んだ）。
    let manifest = Manifest::embedded().expect("埋め込み manifest を読める");
    let row = manifest.get("runner.allowed_commands").expect("上限の行が在る");
    let ceiling = match row.value {
        RuleValue::List(ref found) => found.clone(),
        _ => Vec::new(),
    };
    assert!(!ceiling.is_empty(), "母集団は上限の行の値（実 {} 本）", ceiling.len());
    // **`allowed-commands` の行だけに当てる**——宣言の本文には共通 verify（`cargo …`）も
    // 在るので、file 全体へ `contains` すると allowlist が空でも通る（字面衝突）。
    let line = declared
        .lines()
        .find(|line| line.trim_start().starts_with("allowed-commands"))
        .unwrap_or_default();
    for command in &ceiling {
        assert!(
            line.contains(command.as_str()),
            "自己ホストの宣言は上限の {command:?} を名乗る（母集団 {} 本）: {line}",
            ceiling.len()
        );
    }
    clean(&[&repo, &state]);
}

/// `{vessel}` は便の写しを指す（runner はこれだけを読む）。
#[test]
fn pipe_spawn_substitutes_vessel_placeholder() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let runner = "printf '%s\\n' {vessel} > vessel-arg.txt && git add -A && git commit -q -m runner";
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let worktree = repo.join(".worktrees").join("scribe2").join(&id);
    let handed = fs::read_to_string(worktree.join("vessel-arg.txt")).expect("置換の写しを読める");
    assert_eq!(
        handed.trim(),
        vessel_copy(&state, &id).display().to_string(),
        "{{vessel}} は便の写しを指す"
    );
    let read_back = fs::read_to_string(handed.trim()).expect("渡された path から写しを読める");
    assert!(read_back.contains("allowed-commands"), "写しは宣言の値を持つ: {read_back}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_spawn_measures_repo_before_launching() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let bare = tmp();
    // 起動口の手前（Precheck）で断る。ここを外すと同じ rc 1 でも **別の理由**
    // （起動関数の中で HEAD を読めない）になるので、理由まで見て弁別する。
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &bare.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", "true",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "git repo でなければ rc 1");
    assert!(
        stderr_of(&out).contains("git repo でない"),
        "測る段で断る（起動関数へ入る前）: {}",
        stderr_of(&out)
    );
    assert!(
        !bare.join(".worktrees").exists(),
        "断った周は worktree を作らない"
    );
    clean(&[&repo, &state, &bare]);
}

#[test]
fn pipe_stop_counts_seat_without_pid() {
    let (repo, state) = repo_with_state();
    // pid の無い Live 席（`fleet record` の --pid は任意）。
    let record = Command::new(bin())
        .args(["fleet", "record", "--kind", "SeatSpawned", "--run", "r9", "--bead", "b",
               "--seat", "s9", "--state-dir"])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));
    let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert!(
        stdout_of(&out).contains("seats=1"),
        "pid の無い Live 席も母集団に数える: {}",
        stdout_of(&out)
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_REFUSED)),
        "止められない席が残るので rc 1（「対象なし rc 0」に化けない）"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_stop_keeps_unstoppable_seat_live() {
    let (repo, state) = repo_with_state();
    // pid 1 は殺せない。止めていない席を終端にしない（偽の全クリアを作らない）。
    let record = Command::new(bin())
        .args(["fleet", "record", "--kind", "SeatSpawned", "--run", "r8", "--bead", "b",
               "--seat", "s8", "--pid", "1", "--state-dir"])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));
    for round in 1..=2 {
        let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
        assert_eq!(
            out.status.code(),
            Some(i32::from(RC_REFUSED)),
            "{round} 回目も rc 1（止めていないのに rc 0 を返さない）: {}",
            stdout_of(&out)
        );
        assert!(stdout_of(&out).contains("seats=1"), "{round} 回目: {}", stdout_of(&out));
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_show_reads_repo_from_state() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // **--repo を渡さずに** 撃つ。cwd（この test を走らせている repo）でなく、
    // intake が書き留めた repo から worktree の path が組まれる。
    let out = run_pipe(&["show", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let line = stdout_of(&out);
    assert!(
        line.contains(&repo.display().to_string()),
        "worktree は便に紐づいた repo から組む: {line}"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_external_form() {
    let args = |raw: &[&str]| raw.iter().map(|item| (*item).to_owned()).collect::<Vec<String>>();
    let mut lines = vec![vessel::pipe::cli::usage()];
    lines.extend(vessel::pipe::cli::dispatch(&args(&["show"])).err);
    lines.extend(vessel::pipe::cli::dispatch(&args(&["stop"])).err);
    lines.extend(vessel::pipe::cli::dispatch(&args(&["nope"])).err);
    let form = lines.join("\n");
    insta::assert_snapshot!(form);
}

// ── (b) gate → land → verdict export → e2e（設計 §8 (b)） ──────────────────

/// 便を Implemented まで進める（intake → spawn）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn implemented(repo: &Path, state: &Path, contract: &Path) -> String {
    let id = intake(repo, state, contract);
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
fn fake_lens(marker: &Path, body: &str) -> String {
    format!("cat >/dev/null; touch '{}'; echo '{body}'", marker.display())
}

/// `--rules` に渡す tmp manifest を書く（gate の 2 行 + lock の 2 行だけ）。
///
/// `enabled` は**必須 key**なので全行に書く（`s2-07l.80`）。この便の test 区間の差は
/// この字面の追加だけで、assert の意味は 1 つも動かない——base の loader は `enabled` を
/// 書いた行も同じ値で読むので、base で新しく赤くなる歯は 1 本も無い。
// flip-check: retroactive s2-07l.80
fn write_rules(dir: &Path, name: &str, lens_count: u64, cap: u64) -> PathBuf {
    write_rules_with_retries(dir, name, lens_count, cap, FOLLOW_RETRIES)
}

/// 埋め込み manifest の起こし直しの上限（`pipe.follow_retries`）。写しの既定値をここから
/// 引くのは、**差し替えた周だけが上限の歯である**ことを字面で読めるようにするためである。
const FOLLOW_RETRIES: u64 = 2;

/// tmp manifest の受付の待ちの上限（秒・rules 行 `gate.slot_wait_s` の fixture 値）。
const SLOT_WAIT_S: u64 = 1;

/// tmp manifest の受付の 3 値（rules 行 `gate.job_memory_mb` / `host.reserve_memory_mb` /
/// `gate.slot_wait_s` の fixture 値）。
#[derive(Debug, Clone, Copy)]
struct SlotFixture {
    /// job 1 つが要る memory（MiB）。
    job_mb: u64,
    /// 残す memory（MiB）。
    reserve_mb: u64,
    /// 待ちの上限（秒）。
    wait_s: u64,
}

/// 既定の受付 fixture: 容量の 2 行は埋め込みの値を写し（封じ込めの箱と同じ値で測る）、待ちの
/// 上限だけを [`SLOT_WAIT_S`] に縮める（枠の空かない host で歯が 900 秒待たない）。
fn default_slots() -> SlotFixture {
    SlotFixture {
        job_mb: embedded_int("gate.job_memory_mb"),
        reserve_mb: embedded_int("host.reserve_memory_mb"),
        wait_s: SLOT_WAIT_S,
    }
}

/// [`write_rules`] に起こし直しの上限を足した形（上限の歯だけが値を振る）。
fn write_rules_with_retries(dir: &Path, name: &str, lens_count: u64, cap: u64, retries: u64) -> PathBuf {
    write_rules_full(dir, name, (lens_count, cap), retries, default_slots())
}

/// [`write_rules_with_retries`] に受付の 3 値を足した形（待ちが解ける歯だけが値を振る）。
/// `gate` は `(gate.lens_count, gate.token_cap)`。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_rules_full(dir: &Path, name: &str, gate: (u64, u64), retries: u64, slots: SlotFixture) -> PathBuf {
    let (lens_count, cap) = gate;
    let row = |id: &str, kind: &str, value: u64| {
        format!(
            "[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n"
        )
    };
    // **上限の行も載せる**。intake は宣言をこの行と突き合わせるので、上限を持たない
    // manifest を渡した周は「上限が無い」で断られる（`--rules` は全 subcommand に効く）。
    let ceiling = "[[rule]]\nid = \"runner.allowed_commands\"\nkind = \"RunnerAllowedCommands\"\n\
                   value = [\"cargo\", \"git\", \"sh\"]\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n";
    // 受付の 4 行: 並列度の上限は埋め込みの値を写し、残る 3 行は [`SlotFixture`] の値。
    let body = format!(
        "schema = 1\n\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{ceiling}",
        row("gate.lens_count", "GateLensCount", lens_count),
        row("gate.token_cap", "GateTokenCap", cap),
        row("fleet.lock_retry_ms", "LockRetryMs", 5000),
        row("fleet.lock_stale_ms", "LockStaleMs", 30000),
        row("pipe.follow_retries", "FollowRetries", retries),
        row("gate.mutants_jobs", "GateMutantsJobs", embedded_int("gate.mutants_jobs")),
        row("gate.job_memory_mb", "GateJobMemoryMb", slots.job_mb),
        row("host.reserve_memory_mb", "HostReserveMemoryMb", slots.reserve_mb),
        row("gate.slot_wait_s", "GateSlotWaitS", slots.wait_s),
    );
    let path = dir.join(name);
    fs::write(&path, body).expect("tmp manifest を書ける");
    path
}

/// intake が読む上限の manifest（`sh` を足した写し）。置き場の中に 1 本だけ作る。
fn ceiling_rules(state: &Path) -> String {
    let path = state.join("rules-ceiling.toml");
    if !path.exists() {
        return write_rules(state, "rules-ceiling.toml", 1, 1_000_000).display().to_string();
    }
    path.display().to_string()
}

/// `--rules` を足して gate を 1 回撃つ。
fn gate_with_rules(repo: &Path, state: &Path, id: &str, rules: &Path, lens: &str) -> Output {
    run_pipe(&[
        "gate", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &rules.display().to_string(), "--lens", lens,
    ])
}

/// 3 値を返す fake lens の本文。
fn lens_verdict(verdict: &str) -> String {
    format!("{{\"verdict\":\"{verdict}\",\"evidence\":\"fake\"}}")
}

/// 便の worktree。
fn worktree_of(repo: &Path, id: &str) -> PathBuf {
    repo.join(".worktrees").join("scribe2").join(id)
}

/// `verdict.json` を key/value の並びとして読む。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn verdict_pairs(state: &Path, id: &str) -> Vec<(String, vessel::fleet::json_lite::Value)> {
    let path = state.join("pipe").join(id).join("verdict.json");
    let text = fs::read_to_string(&path).expect("verdict.json を読める");
    vessel::fleet::json_lite::parse_object(text.trim()).expect("verdict.json は 1 行の JSON")
}

/// key/value の並びから 1 つの値を字面で取る。無ければ空。
fn value_of(pairs: &[(String, vessel::fleet::json_lite::Value)], key: &str) -> String {
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
fn show_line(repo: &Path, state: &Path, id: &str) -> String {
    let out = run_pipe(&[
        "show", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    stdout_of(&out)
}

/// gate を 1 回撃つ（lens は任意）。
fn gate_once(repo: &Path, state: &Path, id: &str, lens: Option<&str>) -> Output {
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
fn land_once(repo: &Path, state: &Path, id: &str) -> Output {
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
fn land_once_with_unreadable_diff(repo: &Path, state: &Path, id: &str) -> Output {
    land_once_with_git_shim(repo, state, id, " diff --name-only -z ", None)
}

/// PATH の先頭に「引数列に `failing` を含む呼出しだけ rc 1 で落とし、他は実 git へ exec する git」を置いて
/// land を 1 回撃つ（`--lens` は任意）。読めなかった周の極性を測る歯の共通部。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn land_once_with_git_shim(repo: &Path, state: &Path, id: &str, failing: &str, lens: Option<&str>) -> Output {
    let path = shim_path(state, "shim-bin", &format!("case \"$*\" in *'{failing}'*) exit 1;; esac"));
    let mut args = vec![
        "pipe".to_owned(), "land".to_owned(), "--run".to_owned(), id.to_owned(),
        "--repo".to_owned(), repo.display().to_string(),
        "--state-dir".to_owned(), state.display().to_string(),
    ];
    if let Some(cmd) = lens {
        args.extend(["--lens".to_owned(), cmd.to_owned()]);
    }
    Command::new(bin()).args(args).env("PATH", path).output().expect("binary を起動できる")
}

/// PATH の先頭に置く偽 git（`script` を先に撃ってから実 git へ exec する）。返すのは PATH の値。
///
/// 器の git の呼び方を**現物で**振る唯一の口である（読めない git・書き込む git・壊す git）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn shim_path(state: &Path, name: &str, script: &str) -> String {
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
fn gated_pass(repo: &Path, state: &Path, contract: &Path, marker: &Path) -> String {
    let id = implemented(repo, state, contract);
    let lens = fake_lens(marker, &lens_verdict("PASS"));
    let out = gate_once(repo, state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS の gate は rc 0: {}", stderr_of(&out));
    id
}

#[test]
fn pipe_gate_refuses_dirty_worktree() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 実装後に worktree を汚す。gate は前提を満たさない。
    fs::write(worktree_of(&repo, &id).join("dirty.txt"), "x\n").expect("汚せる");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "前提違反は rc 1");
    assert!(stderr_of(&out).contains("clean でない"), "理由: {}", stderr_of(&out));
    assert!(!marker.exists(), "**lens を起動しない**（前提違反の周）");
    assert!(
        show_line(&repo, &state, &id).contains("stage=Failed"),
        "precheck 違反は Failed で残る: {}",
        show_line(&repo, &state, &id)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_fails_on_red_verify_line() {
    let (repo, state) = repo_with_state();
    // 2 行目が rc≠0。**逐条**で残るので 2 行とも verify.jsonl に出る。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-ok.sh", "sh verify-red.sh"]"#]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1");
    assert!(stdout_of(&out).contains("verdict=FAIL"), "{}", stdout_of(&out));
    // **lens は呼ばない**: verify が赤い周は lens の verdict に上書きされない。
    assert!(!marker.exists(), "verify RED の周は lens を起動しない");
    let log = fs::read_to_string(state.join("pipe").join(&id).join("verify.jsonl"))
        .expect("verify.jsonl を読める");
    // 母集団 = 4 record（write-set 照合 1 + 写しの共通 verify 1 + 契約 2 本）。
    assert_eq!(log.lines().count(), 4, "verify は逐条で残る: {log}");
    assert!(log.contains("\"n\":3,\"rc\":0,\"cmd\":\"sh verify-ok.sh\""), "契約 1 本目は緑: {log}");
    assert!(log.contains("\"n\":4,\"rc\":1,\"cmd\":\"sh verify-red.sh\""), "契約 2 本目が赤: {log}");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "1", "赤は 1 本");
    clean(&[&repo, &state]);
}

/// `verify.jsonl` の record を全部読む。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn verify_rows(state: &Path, id: &str) -> Vec<Vec<(String, vessel::fleet::json_lite::Value)>> {
    let log = fs::read_to_string(state.join("pipe").join(id).join("verify.jsonl"))
        .expect("verify.jsonl を読める");
    log.lines()
        .filter_map(|line| vessel::fleet::json_lite::parse_object(line.trim()).ok())
        .collect()
}

/// n 番目（1 始まり）の record の 1 値。
fn row_value(rows: &[Vec<(String, vessel::fleet::json_lite::Value)>], n: usize, key: &str) -> String {
    rows.get(n.saturating_sub(1)).map_or_else(String::new, |row| value_of(row, key))
}

/// spawn を 1 回撃つ（runner を選ぶ形）。
fn spawn_with(repo: &Path, state: &Path, id: &str, runner: &str) -> Output {
    run_pipe(&[
        "spawn", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ])
}

/// **write-set の外へ出た便は gate が落とす**（ADR-0009 §2.4・段①）。
///
/// guard（hook）は misbehave した runner のための backstop で、`sh -c` の runner には
/// 効かない。**器の側で数える**面がここである。
#[test]
fn pipe_gate_fails_when_diff_leaves_write_set() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // 契約の write-set は `src/lib.rs` だけ。runner が別 file も足す。
    // `src/lib.rs.bak` は **write-set の entry の接頭辞だが segment 境界で外れる** path。
    // これが無いと「`/` を 1 文字落とす」変異（`starts_with(trimmed)`）が生き残る。
    let runner = "echo x >> src/lib.rs && echo y > stray.md && echo z > src/lib.rs.bak \
                  && git add -A && git commit -q -m runner";
    let spawned = spawn_with(&repo, &state, &id, runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&spawned));
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "write-set の外は FAIL: {}", stderr_of(&out));

    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 1, "cmd"), "write-set", "段①は先頭（母集団 {} record）", rows.len());
    assert_eq!(row_value(&rows, 1, "rc"), "1", "外れた便は段①が赤い");
    // **lens は呼ばない**（赤い周は lens の顔色で通らない）。
    assert!(!marker.exists(), "段①が赤い周は lens を起動しない");
    let tail = fs::read_to_string(state.join("pipe").join(&id).join("verify.stderr.log"))
        .expect("verify.stderr.log を読める");
    assert!(tail.contains("stray.md"), "外れた path を列挙する: {tail}");
    assert!(tail.contains("src/lib.rs.bak"), "接頭辞が一致しても segment 境界で外れる: {tail}");
    // **内に収まる path は列挙しない**（`src/lib.rs.bak` を含む行を除いて数える＝字面の
    // 包含関係で assert が空虚にならないようにする）。
    assert!(
        !tail.lines().any(|line| line.contains("src/lib.rs") && !line.contains("src/lib.rs.bak")),
        "内に収まる path は列挙しない: {tail}"
    );

    // **弁別**: 同じ契約でも write-set の内に収まる便は段①が緑になる（`stray.md` を足さない）。
    // **bead を分ける**（run id は `<bead>-<秒>` なので、同じ秒の 2 便目は id が衝突する）。
    let second = write_contract(&repo, &[], &[]);
    let inside = intake_bead(&repo, &state, &second, "s2-41o");
    let ran = spawn_with(&repo, &state, &inside, TOY_COMMIT);
    assert_eq!(ran.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&ran));
    let ok = gate_once(&repo, &state, &inside, Some(&fake_lens(&state.join("lens-2"), &lens_verdict("PASS"))));
    assert_eq!(ok.status.code(), Some(i32::from(RC_OK)), "内に収まる便は通る: {}", stderr_of(&ok));
    assert_eq!(row_value(&verify_rows(&state, &inside), 1, "rc"), "0", "段①が緑");
    clean(&[&repo, &state]);
}

/// **共通 verify は便の写しから撃ち、契約の verify より前に来る**（段②→段③）。
///
/// `{base}` は共通 verify の行だけが置ける穴で、契約の行には置換しない（.56 の intake が
/// 契約行の穴を断っているので、契約側に穴は在り得ない）。
#[test]
fn pipe_gate_runs_common_verify_from_vessel_copy_before_contract() {
    let (repo, state) = repo_with_state();
    // git だけで rc 0 / rc≠0 になる 2 行（宣言の allowlist は git / sh）。
    commit_vessel(
        &repo,
        VESSEL_ALLOWED,
        r#"["git rev-parse --verify {base}", "git cat-file -e {base}:no-such-file"]"#,
    );
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "共通 verify が赤ければ FAIL: {}", stderr_of(&out));

    let rows = verify_rows(&state, &id);
    assert_eq!(rows.len(), 4, "母集団 = write-set 1 + 共通 2 + 契約 1");
    assert_eq!(row_value(&rows, 1, "cmd"), "write-set", "段①");
    assert_eq!(row_value(&rows, 2, "rc"), "0", "共通の 1 本目は緑");
    assert_ne!(row_value(&rows, 3, "rc"), "0", "共通の 2 本目は赤");
    assert_eq!(row_value(&rows, 4, "cmd"), "sh verify-ok.sh", "契約の行は共通の後ろ");
    // **`{base}` は置換されている**（穴のまま撃つと `git rev-parse --verify {base}` は赤い）。
    assert_eq!(
        row_value(&rows, 2, "cmd"),
        format!("git rev-parse --verify {base}"),
        "共通の行の穴は便の base へ置換される"
    );
    // 「契約の行には置換しない」は **intake が契約行の穴を断つ**ので gate では観測できない
    // （穴を持つ契約は run にならない）。その保証は
    // `pipe_intake_refuses_contract_verify_with_placeholder` が持つ＝ここでは測らない。
    assert!(!marker.exists(), "段②が赤い周も lens を起動しない");
    clean(&[&repo, &state]);
}

/// **便の実装が worktree の宣言を書き換えても、gate は写しの行を撃つ**（ADR-0010 §2.4）。
///
/// 読み直す実装だと、便が自分の検証を消して通れる（自己拡張）。
#[test]
fn pipe_gate_ignores_worktree_vessel_declaration() {
    let (repo, state) = repo_with_state();
    // 宣言も write-set に入れる＝段①は緑のまま段②だけを測る。
    let path = write_contract(&repo, &["write-set"], &[r#"write-set = ["src/lib.rs", ".vessel.toml"]"#]);
    let id = intake(&repo, &state, &path);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    // runner が worktree の宣言を「必ず赤くなる共通 verify」へ書き換えて commit する。
    let runner = "printf 'schema = 1\\nallowed-commands = [\"git\"]\\n\
                  common-verify = [\"git cat-file -e HEAD:no-such-file\"]\\n' > .vessel.toml \
                  && echo x >> src/lib.rs && git add -A && git commit -q -m runner";
    let spawned = spawn_with(&repo, &state, &id, runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&spawned));
    // **repo 側の宣言も intake の後に赤い行へ差し替える**＝写しからしか読まないことを
    // worktree 面と repo 面の 2 面で縛る（main が進んだ周に凍っていない行を撃たない）。
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["git cat-file -e HEAD:no-such-file"]"#);
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "写しの行は緑なので通る: {}", stderr_of(&out));

    let rows = verify_rows(&state, &id);
    assert_eq!(
        row_value(&rows, 2, "cmd"),
        format!("git rev-parse --verify {base}"),
        "撃つのは**写し**の行"
    );
    let log = fs::read_to_string(state.join("pipe").join(&id).join("verify.jsonl"))
        .expect("verify.jsonl を読める");
    assert!(!log.contains("no-such-file"), "worktree の宣言は読み直さない: {log}");
    // worktree 側の宣言が実際に書き換わっていることも測る（**測っていない**を「通った」に
    // 化けさせないため＝runner が何もしていなければこの歯は空虚になる）。
    let changed = fs::read_to_string(worktree_of(&repo, &id).join(".vessel.toml"))
        .expect("worktree の宣言を読める");
    assert!(changed.contains("no-such-file"), "runner は宣言を書き換えている: {changed}");
    clean(&[&repo, &state]);
}

/// 赤い verify 行の stderr の末尾が診断 file に残る（緑の行は残さない）。
///
/// `verify.jsonl` の rc だけでは「何がどう赤いか」が便の外から読めない。**stderr の
/// 本文で測る**——見出し行にも `cmd=` として字面が載るので、cmd に**無い**字面
/// （`boom` は `printf` が組み立てる）で「写しが空でない」を弁別する。
#[test]
fn pipe_gate_keeps_stderr_tail_of_red_verify_lines() {
    let (repo, state) = repo_with_state();
    // 2 行目が rc 3 で stderr に 1 行出す。cmd の字面には `boom` が無い。
    let path = write_contract(
        &repo,
        &["verify"],
        &[r#"verify = ["sh verify-ok.sh", "sh verify-noisy.sh"]"#],
    );
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1");
    assert!(stdout_of(&out).contains("verdict=FAIL"), "{}", stdout_of(&out));

    // record の形は変えない（schema 不変）。rc は逐条のまま `verify.jsonl` に在る。
    let log = fs::read_to_string(state.join("pipe").join(&id).join("verify.jsonl"))
        .expect("verify.jsonl を読める");
    let rows: Vec<&str> = log.lines().collect();
    // 母集団 = 4 record（write-set 照合 1 + 写しの共通 verify 1 + 契約 2 本）。
    assert_eq!(rows.len(), 4, "verify は逐条で残る: {log}");
    let second =
        vessel::fleet::json_lite::parse_object(rows.get(3).copied().unwrap_or_default().trim())
            .expect("4 行目は 1 行の JSON");
    assert_eq!(value_of(&second, "n"), "4", "赤いのは契約の 2 本目: {log}");
    assert_eq!(value_of(&second, "rc"), "3", "赤い契約行の rc: {log}");

    let tail = fs::read_to_string(state.join("pipe").join(&id).join("verify.stderr.log"))
        .expect("verify.stderr.log を読める");
    let heads: Vec<&str> = tail.lines().filter(|line| line.starts_with("## ")).collect();
    assert_eq!(
        heads.len(),
        1,
        "見出しは赤い行の分だけ（母集団 {} 行）: {tail}",
        tail.lines().count()
    );
    let head = heads.first().copied().unwrap_or_default();
    assert!(head.contains("n=4 rc=3"), "見出しは赤い行を名指す: {head}");
    assert!(!head.contains("boom"), "見出しの cmd= に boom の字面は無い: {head}");
    assert!(!tail.contains("## n=3"), "緑の行は見出しを残さない: {tail}");
    assert!(tail.contains("boom"), "stderr の本文が残る: {tail}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_without_lens_when_required() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 規則は lens を 1 本要る（gate.lens_count = 1）が `--lens` が無い。
    let out = gate_once(&repo, &state, &id, None);
    assert_eq!(out.status.code(), Some(3), "判定できない周の rc は 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE");
    assert!(
        value_of(&pairs, "evidence").contains("--lens が無い"),
        "理由が残る: {}",
        value_of(&pairs, "evidence")
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_when_diff_exceeds_cap() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // cap を 1 byte にした manifest を渡す（**数値は規則から来る**ことを測る）。
    let rules = write_rules(&repo, "tight.toml", 1, 1);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "gate", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &rules.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(3), "cap 超過の rc は 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    // **lens を呼ばない**のが cap の意味である（呼んでから捨てるのでは予算を守れない）。
    assert!(!marker.exists(), "cap 超過の周は lens を起動しない");
    let pairs = verdict_pairs(&state, &id);
    assert!(
        value_of(&pairs, "evidence").contains("cap 1"),
        "cap の値は規則から来る: {}",
        value_of(&pairs, "evidence")
    );
    let bytes: u64 = value_of(&pairs, "diff_bytes").parse().unwrap_or(0);
    assert!(bytes > 1, "diff の byte 数を実測して比べている: {bytes}");

    // **境界**: 設計は「diff byte > cap → INCONCLUSIVE」＝等号は超えていない。
    // 同じ内容の別便を cap = ちょうどその byte 数で撃ち、PASS 側に残ることを測る。
    // INCONCLUSIVE は終端でない＝同じ write-set の便と交差する（`s2-07l.145`）ので、
    // 測り終えた 1 本目を `stop --run` で外してから双子を起こす。
    stop_run_ok(&state, &id);
    let twin = intake_bead(&repo, &state, &path, "s2-edge");
    let out = run_pipe(&[
        "spawn", "--run", &twin, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    let exact = write_rules(&repo, "exact.toml", 1, bytes);
    let edge = gate_with_rules(&repo, &state, &twin, &exact, &lens);
    assert_eq!(
        edge.status.code(),
        Some(i32::from(RC_OK)),
        "cap ちょうどは超えていない（> であって >= でない）: {}",
        stdout_of(&edge)
    );
    assert_eq!(
        value_of(&verdict_pairs(&state, &twin), "diff_bytes"),
        bytes.to_string(),
        "同じ内容の便なので diff の byte 数も同じ"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_records_structured_verdict() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS の rc は 0: {}", stderr_of(&out));
    assert!(marker.exists(), "判定に届いた周は lens を起動する");
    let pairs = verdict_pairs(&state, &id);
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["schema", "run", "verdict", "evidence", "verify_red", "diff_bytes", "tree", "ts"],
        "verdict.json の key 列（設計 §5.3）"
    );
    assert_eq!(value_of(&pairs, "schema"), "1");
    assert_eq!(value_of(&pairs, "run"), id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS");
    assert_eq!(value_of(&pairs, "evidence"), "fake", "lens の evidence を写す");
    assert_eq!(value_of(&pairs, "verify_red"), "0");
    assert!(
        show_line(&repo, &state, &id).contains("stage=Gated"),
        "段が Gated へ動く: {}",
        show_line(&repo, &state, &id)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_refuses_without_pass() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("FAIL"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert!(stdout_of(&gated).contains("verdict=FAIL"), "{}", stdout_of(&gated));
    let before = git(&repo, &["rev-parse", "refs/heads/main"]);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "PASS 以外は rc 1");
    assert!(stderr_of(&out).contains("PASS でない"), "理由: {}", stderr_of(&out));
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        before,
        "**何もしない**（main は 1 byte も動かない）"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_squashes_one_commit_with_identical_tree() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let tree = git(&worktree_of(&repo, &id), &["rev-parse", "HEAD^{tree}"]);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert!(stdout_of(&out).contains(&format!("landed={new}")), "{}", stdout_of(&out));
    assert_eq!(
        git(&repo, &["rev-list", "--count", &format!("{base}..{new}")]),
        "1",
        "**squash は 1 commit**"
    );
    assert_eq!(
        git(&repo, &["rev-parse", &format!("{new}^{{tree}}")]),
        tree,
        "tree は同一（lossless）"
    );
    clean(&[&repo, &state]);
}

/// 件名の歯の goal（**文が複数・200 字超**で、先頭の文が 72 文字より長い）。契約 file は
/// 1 行 1 値ゆえ改行を置けない（「改行を保つ」側は `pipe::land` の in-file の歯が測る）。
const LONG_GOAL: &str = "件名の要旨は goal の先頭の文を 72 文字で切って組む・この 1 文目は 72 文字より長いので末尾に印が付く・切った側と落とさない側を 1 本の message の中で持つのがこの便の主題である。2 文目はここから始まり件名には載らないが本文には逐語で載る。3 文目も同じで、契約の中身は message の本文からそのまま辿れる。";

/// squash の message は **3 部**（件名 / 空行 / 本文 = goal 全文 + `run:` trailer・`s2-07l.130`・
/// 設計 §5.4 手順 1）。件名は goal の**先頭の文**を 72 文字で切った要旨で、切って落ちた中身は
/// 本文に逐語で残る——`git log --oneline` が読めて、便の現物へは trailer から辿れる形である。
#[test]
fn pipe_land_subject_cuts_first_sentence_and_keeps_goal_in_body() {
    let (repo, state) = repo_with_state();
    let goal = format!("goal = \"{LONG_GOAL}\"");
    let path = write_contract(&repo, &["goal"], &[&goal]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let subject = git(&repo, &["log", "-1", "--format=%s", "refs/heads/main"]);
    assert!(
        subject.starts_with("s2-2e5: 件名の要旨は goal の先頭の文を 72 文字で切って組む"),
        "件名は `<bead>: ` + goal の先頭の文で始まる: {subject}"
    );
    assert!(
        subject.chars().count() <= "s2-2e5: ".chars().count() + 72 + 1,
        "件名は 72 文字 + `…` 以内（base は goal 全文を載せるので落ちる）: {subject}"
    );
    assert!(subject.ends_with('…'), "切った周は印が付く: {subject}");
    // 落とさない側。**本文は goal 全文を逐語で持ち**、最終行は run へ辿る trailer である。
    let body = git(&repo, &["log", "-1", "--format=%b", "refs/heads/main"]);
    assert!(body.contains(LONG_GOAL), "本文に goal 全文が逐語で在る: {body}");
    let trailer = format!("run: {id}");
    assert_eq!(body.lines().last(), Some(trailer.as_str()), "最終行は run trailer: {body}");
    clean(&[&repo, &state]);
}

/// 負例: goal が 1 文で 72 文字以内なら件名は**その文そのもの**で、切った印（`…`）は付かない
/// （「。」の手前までが先頭の文である）。
#[test]
fn pipe_land_subject_keeps_short_single_sentence_whole() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["goal"], &[r#"goal = "短い 1 文の goal は件名にそのまま載る。""#]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert_eq!(
        git(&repo, &["log", "-1", "--format=%s", "refs/heads/main"]),
        "s2-2e5: 短い 1 文の goal は件名にそのまま載る",
        "72 文字以内の 1 文は逐語（`…` は付かない）"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_rebase_without_lens_stops_inconclusive_and_keeps_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    // gate の後に main が別便で進む。CAS の old が動いた＝そのままでは land できない。
    fs::write(repo.join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    // `--lens` 無しの land: 追随（rebase）は済むが撃ち直しの gate は lens を得られず
    // INCONCLUSIVE＝**land しない**（測れなかったを通ったに化けさせない・FR14 で測り直せる）。
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "lens 無しの撃ち直しは rc 3: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("rebase={base}..{moved}")), "追随は済む: {stdout}");
    assert!(stdout.contains("verdict=INCONCLUSIVE"), "撃ち直しの判定行: {stdout}");
    assert!(!stdout.contains("landed="), "land していない: {stdout}");
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        moved,
        "断った周は main を動かさない"
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated（測り直せる側）");
    clean(&[&repo, &state]);
}

/// worktree が clean でない周は **rebase を撃たずに rc 1 で何も書かない**（設計 §5.4 (ii)）。
/// 汚れた木で rebase すると撃ち直しの precheck が `Failed` で終端し、回復可能だった便が閉じる。
#[test]
fn pipe_land_rebase_refuses_dirty_worktree_without_rebase() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let worktree = worktree_of(&repo, &id);
    let head_before = git(&worktree, &["rev-parse", "HEAD"]);
    fs::write(repo.join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    // gate の後に木が汚れた（未 commit の仕事が在る）。
    fs::write(worktree.join("dirty.txt"), "x\n").expect("汚せる");
    let before = event_count(&state);
    fs::remove_file(&marker).expect("lens の marker を消せる");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "汚れた木は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("clean でない"), "理由: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before, "何も書かない");
    assert!(!marker.exists(), "gate を撃ち直さない（lens は走らない）");
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head_before, "rebase を撃たない（HEAD 不変）");
    assert!(worktree.join("dirty.txt").exists(), "未 commit の仕事は残る");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated のまま（回復可能）");
    clean(&[&repo, &state]);
}

/// 撃ち直しの間に main がさらに動いた周は **rc 1 `stale base` で squash しない**（1 回の land が
/// rebase するのは 1 度だけ）。次の land が同じ経路で追随する。
#[test]
fn pipe_land_rebase_refuses_when_main_moves_during_regate() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    fs::write(repo.join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    // 撃ち直しの lens が走っている間に **さらに別便が main を進める**（lens の中で commit する）。
    let racing = format!(
        "cat >/dev/null; git -C '{}' commit -q --allow-empty -m racing; echo '{}'",
        repo.display(),
        lens_verdict("PASS")
    );
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &racing,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "撃ち直し中に動いた main は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("stale base"), "理由: {}", stderr_of(&out));
    // **追随と撃ち直しは実際に起きた**（event に残り lens を 1 回消費した）ので、判定行も残す。
    let stdout = stdout_of(&out);
    assert!(stdout.contains("rebase=") && stdout.contains(&format!("..{moved}")), "追随の行が残る: {stdout}");
    assert!(stdout.contains("verdict=PASS"), "撃ち直しの判定行が残る: {stdout}");
    let raced = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(raced, moved, "lens の中で main が進んでいる");
    assert_eq!(git(&repo, &["rev-parse", &format!("{raced}^")]), moved, "main に載ったのは racing の 1 commit だけ（squash していない）");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated（撃ち直しは PASS）");
    // 次の land が同じ経路で追随して載る。
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let again = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "次の land は追随して載る: {}", stderr_of(&again));
    assert!(stdout_of(&again).contains(&format!("rebase={moved}..{raced}")), "2 度目の追随: {}", stdout_of(&again));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-parse", &format!("{new}^")]), raced, "racing の上に載る");
    clean(&[&repo, &state]);
}

/// 同じ base から 2 便を PASS の gate まで通す（1 本目 = `src/lib.rs`・2 本目 = `src/b.rs`＝
/// write-set は交わらない）。追随の歯の材料。
fn two_gated_runs(repo: &Path, state: &Path, marker: &Path) -> (String, String) {
    let contract_a = write_contract(repo, &[], &[]);
    let id_a = gated_pass(repo, state, &contract_a, marker);
    let contract_b = write_contract(repo, &["write-set"], &[r#"write-set = ["src/b.rs"]"#]);
    let id_b = intake_bead(repo, state, &contract_b, "s2-3ax");
    let spawned = run_pipe(&[
        "spawn", "--run", &id_b, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo b > src/b.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "2 本目の spawn: {}", stderr_of(&spawned));
    let lens = fake_lens(marker, &lens_verdict("PASS"));
    let gated = gate_once(repo, state, &id_b, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "2 本目の gate: {}", stderr_of(&gated));
    (id_a, id_b)
}

/// 追随した便の event 列が **Implemented(rebase:) → Gated(PASS) → Landed** の順で replay できるか。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn assert_follow_events(state: &Path, base: &str, moved: &str, new: &str) {
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let rebased = log.find(&format!("rebase:{base}..{moved}")).expect("追随の event が在る");
    let regated = log.rfind("verdict:PASS").expect("撃ち直しの判定 event が在る");
    let landed = log.rfind(&format!("sha:{new}")).expect("Landed の event が在る");
    assert!(rebased < regated && regated < landed, "順序: rebase → 撃ち直し → Landed\n{log}");
    let follow = log
        .lines()
        .find(|line| line.contains(&format!("rebase:{base}..{moved}")))
        .expect("追随の event の行が在る");
    assert!(follow.contains("\"stage\":\"Implemented\""), "追随の event は段を Implemented へ戻す: {follow}");
}

/// main が動いた便の追随（設計 §5.4・`s2-07l.119`）: **2 便を同じ base から起こし、1 本目を
/// land して main を動かした後、2 本目の land が rebase → gate の撃ち直し → 新 base で CAS**。
#[test]
fn pipe_land_rebase_follows_landed_sibling_and_lands() {
    let (repo, state) = repo_with_state();
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    // 1 本目が land して main が動く（2 本目の base は置き去り）。
    let first = land_once(&repo, &state, &id_a);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "1 本目の land: {}", stderr_of(&first));
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(moved, base, "main が動いている");
    fs::remove_file(&marker).expect("撃ち直しの前に lens の marker を消せる");

    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id_b, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "追随した land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(new, moved, "2 本目も land した");
    assert_eq!(git(&repo, &["rev-parse", &format!("{new}^")]), moved, "squash は動いた main の上に載る");
    assert_eq!(git(&repo, &["rev-list", "--count", &format!("{moved}..{new}")]), "1", "squash は 1 commit");
    assert_eq!(git(&repo, &["show", &format!("{new}:src/b.rs")]), "b", "2 本目の仕事が main に載る");
    assert!(git(&repo, &["show", &format!("{new}:src/lib.rs")]).contains('x'), "1 本目の仕事も残る");
    assert!(marker.exists(), "gate を撃ち直した（lens が再び走った）");
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("run={id_b} rebase={base}..{moved}")), "rebase= token: {stdout}");
    assert!(stdout.contains("verdict=PASS"), "撃ち直しの判定行: {stdout}");
    assert!(stdout.contains(&format!("landed={new}")), "landed=: {stdout}");
    assert_follow_events(&state, &base, &moved, &new);
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 便 1 本を PASS の gate まで通し、**同じ変更を先に main へ載せる**（1 本目が land した後と
/// 同じ状態）。返すのは便の id と、そのときの main の sha。
///
/// `.145` までは 2 便を同時に live にして 1 本目を land する形だった。入口の排他
/// （`s2-07l.145`・ADR-0019 §2.1）が在る今、同じ write-set の 2 便は**同時に live にできない**
/// ——main が動いた事実だけを器の外で作り、rebase で patch が空になる便を 1 本で測る
/// （land / retire が測る対象は 1 つも変えていない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn gated_run_whose_change_is_already_on_main(repo: &Path, state: &Path, marker: &Path) -> (String, String) {
    let contract = write_contract(repo, &[], &[]);
    let id = gated_pass(repo, state, &contract, marker);
    // 便の runner（`echo x >> src/lib.rs`）と**同じ 1 行**を main へ載せる。
    let lib = repo.join("src").join("lib.rs");
    let text = fs::read_to_string(&lib).expect("seed を読める");
    fs::write(&lib, format!("{text}x\n")).expect("main 側に同じ変更を書ける");
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "same-change-from-elsewhere"]);
    let landed = git(repo, &["rev-parse", "refs/heads/main"]);
    (id, landed)
}

/// 同一変更の 2 便: 1 本目が land した後の 2 本目は rebase で commit が 0 本になり、
/// **gate を撃ち直さず `Failed detail=rebase-empty`**（main は 1 本目の sha のまま・`s2-07l.125`）。
#[test]
fn pipe_land_rebase_empty_fails_closed_without_regate() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_b, landed) = gated_run_whose_change_is_already_on_main(&repo, &state, &marker);
    fs::remove_file(&marker).expect("lens の marker を消せる");
    let before = event_count(&state);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id_b, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "空になった便は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("既に main に在る"), "理由: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(stderr.contains(&id_b) && stderr.contains("base=") && stderr.contains("main="), "run / base / main を名乗る: {stderr}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), landed, "main は 1 本目の sha のまま");
    assert!(!marker.exists(), "gate を撃ち直さない（lens は走らない）");
    assert_eq!(event_count(&state), before + 1, "残す event は Failed の 1 本だけ");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let last = log.lines().last().unwrap_or_default();
    assert!(last.contains("\"stage\":\"Failed\"") && last.contains("rebase-empty"), "末尾: {last}");
    assert!(!log.contains("rebase:"), "追随の event は書かない（追随の先が無い）\n{log}");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Failed"), "段は Failed");
    clean(&[&repo, &state]);
}

/// 読めない周は 0 に読み替えない: rebase の後の `rev-list --count` が落ちる周は `rebase-empty` に**倒さず**
/// 従来どおり追随の event を残して撃ち直しの precheck へ流す（fail-closed の向きは不変）。
#[test]
fn pipe_land_rebase_empty_does_not_treat_unreadable_count_as_zero() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let first = land_once(&repo, &state, &id_a);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "1 本目の land: {}", stderr_of(&first));
    let landed = git(&repo, &["rev-parse", "refs/heads/main"]);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // `rev-list --count` だけを落とす git を前に置く。追随の rebase 自体は通る。
    let out = land_once_with_git_shim(&repo, &state, &id_b, " rev-list --count ", Some(&lens));
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "読めない周に land はしない: {}", stdout_of(&out));
    assert!(!stderr_of(&out).contains("既に main に在る"), "読めないを 0 に読み替えない: {}", stderr_of(&out));
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(!log.contains("rebase-empty"), "rebase-empty を名乗らない\n{log}");
    assert!(log.contains("rebase:"), "追随の event は残る（従来の経路へ流れた）\n{log}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), landed, "main は動かない");
    clean(&[&repo, &state]);
}

/// 負例: 変更が **異なる** 2 便は従来どおり追随して Landed（`rebase-empty` に倒れない）。
#[test]
fn pipe_land_rebase_empty_does_not_fire_for_distinct_changes() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let first = land_once(&repo, &state, &id_a);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "1 本目の land: {}", stderr_of(&first));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id_b, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "異なる変更は追随して land: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("landed="), "{}", stdout_of(&out));
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(!log.contains("rebase-empty"), "rebase-empty は出ない\n{log}");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 追随の rebase が衝突した周は木を戻し、**便を終端にせず**衝突を記帳して止まる（`s2-07l.146`・
/// ADR-0019 §2.2）。この歯が pin するのは「**main は 1 byte も動かない**・木は衝突前へ戻る」で、
/// 起こし直しそのものは `pipe_follow_` の歯が測る。
///
/// `--runner` を渡さない `pipe land` は起こし直せないので rc 1 で断る——衝突の記帳
/// （`Implemented detail=rebase-conflict:…`）だけは残り、`pipe resume --runner` で続けられる。
#[test]
fn pipe_land_rebase_conflict_fails_closed_and_keeps_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let worktree = worktree_of(&repo, &id);
    let head_before = git(&worktree, &["rev-parse", "HEAD"]);
    let branch_before = git(&worktree, &["rev-parse", "--abbrev-ref", "HEAD"]);
    // 別便が **同じ file の同じ末尾** へ別の行を足す（runner は `echo x >> src/lib.rs`）。
    let lib = repo.join("src").join("lib.rs");
    let mut text = fs::read_to_string(&lib).expect("seed を読める");
    text.push_str("y\n");
    fs::write(&lib, text).expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "起こし直せない周は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("--runner が要る"), "理由: {}", stderr_of(&out));
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は 1 byte も動かない");
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head_before, "木は衝突前へ戻る");
    assert_eq!(git(&worktree, &["rev-parse", "--abbrev-ref", "HEAD"]), branch_before, "branch に居る（rebase 途中で detach していない）");
    assert!(git(&worktree, &["status", "--porcelain"]).is_empty(), "衝突の残骸が無い");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(log.contains(&format!("rebase-conflict:{base}..{moved}")), "衝突が event に残る\n{log}");
    assert!(!log.contains("\"Failed\""), "便を終端にしない\n{log}");
    assert!(!log.contains("\"detail\":\"rebase:"), "追随の event は書かない（追随できていない）\n{log}");
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段は Implemented（起こし直せる側）");
    clean(&[&repo, &state]);
}

/// 追随した後の gate の撃ち直しが FAIL なら **land しない**（`Gated` のまま・main は動いたまま）。
#[test]
fn pipe_land_rebase_regate_fail_keeps_gated_and_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    fs::write(repo.join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    let lens = fake_lens(&marker, &lens_verdict("FAIL"));
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "撃ち直し FAIL は gate の rc 1: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("rebase={base}..{moved}")), "追随は済んでいる: {stdout}");
    assert!(stdout.contains("verdict=FAIL"), "撃ち直しの判定行: {stdout}");
    assert!(!stdout.contains("landed="), "land していない: {stdout}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は撃ち直しの前のまま");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "FAIL", "verdict.json は撃ち直しの値");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated のまま");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(!log.contains("\"Landed\""), "Landed の event は無い\n{log}");
    // 同じ便をもう一度 land しても PASS でないので断る（撃ち直しの FAIL は終端）。
    let again = land_once(&repo, &state, &id);
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "FAIL のまま land はできない");
    assert!(stderr_of(&again).contains("PASS でない"), "理由: {}", stderr_of(&again));
    clean(&[&repo, &state]);
}

/// `resume` の Gated(PASS) → land も同じ経路で追随する（別口を作らない）。
#[test]
fn pipe_land_rebase_resume_from_gated_follows_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    fs::write(repo.join("other.txt"), "other\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "resume の追随は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-parse", &format!("{new}^")]), moved, "動いた main の上に載る");
    let stdout = stdout_of(&out);
    assert!(stdout.contains(&format!("rebase={base}..{moved}")), "rebase= token: {stdout}");
    assert!(stdout.contains(&format!("landed={new}")), "landed=: {stdout}");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// base が main の祖先でない周（main が巻き戻った / 分岐した）は追随の形が無い＝**rc 1 で
/// 何も書かない**（rebase も撃たない）。
#[test]
fn pipe_land_rebase_refuses_when_base_is_not_ancestor_of_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let worktree = worktree_of(&repo, &id);
    let head_before = git(&worktree, &["rev-parse", "HEAD"]);
    // 親を持たない commit を main に据える（便の base はその祖先でない）。
    let tree = git(&repo, &["rev-parse", "refs/heads/main^{tree}"]);
    let root = git(&repo, &["commit-tree", &tree, "-m", "diverged"]);
    git(&repo, &["update-ref", "refs/heads/main", &root]);
    let before = event_count(&state);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "祖先でない base は rc 1");
    assert!(stderr_of(&out).contains("stale base"), "理由: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("祖先でない"), "理由の弁別: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before, "何も書かない");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), root, "main は動かない");
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), head_before, "worktree も動かない（rebase を撃たない）");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated のまま");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_reruns_verify_on_main_and_fails_loud() {
    let (repo, state) = repo_with_state();
    // 1 回目（worktree）は緑・2 回目（main の実測）は赤になる verify 行。印の置き場は
    // script の中で **git の共通 dir** から解く（行に絶対 path は書けない）。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-once.sh"]"#]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "main が赤ければ rc 1");
    assert!(stderr_of(&out).contains("main が赤い"), "理由: {}", stderr_of(&out));
    // **auto revert しない**: main は進んだまま loud に落ちる。
    assert_ne!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        base,
        "main は進んだまま（revert しない）"
    );
    assert!(
        show_line(&repo, &state, &id).contains("stage=Failed"),
        "Failed detail=main-red で残る: {}",
        show_line(&repo, &state, &id)
    );
    assert!(
        !land::verdicts_path(&state).exists(),
        "赤い周は面 5 へ export しない"
    );
    clean(&[&repo, &state]);
}

/// **land の main 実測も写しの共通 verify を撃つ**（gate と同じ順序・同じ関数）。
///
/// 契約の verify だけを撃つ実装だと、main で初めて赤くなる共通の検証（repo 共通の lint /
/// 依存監査）を素通しして便が載る。1 回目（便の worktree）は緑・2 回目（main の実測）は
/// 赤になる行で、**撃った回数**から弁別する。
#[test]
fn pipe_land_reruns_common_verify_from_vessel_copy_on_main() {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-once.sh"]"#);
    // 契約の verify（`sh verify-ok.sh`）は main でも緑＝赤いのは**写しの共通 verify** である。
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_REFUSED)),
        "main で共通 verify が赤ければ rc 1: {}",
        stderr_of(&out)
    );
    assert!(stderr_of(&out).contains("main が赤い"), "理由: {}", stderr_of(&out));
    assert!(
        show_line(&repo, &state, &id).contains("stage=Failed"),
        "Failed detail=main-red で残る: {}",
        show_line(&repo, &state, &id)
    );
    // **auto revert しない**（main は進んだまま loud に落ちる）。
    assert_ne!(git(&repo, &["rev-parse", "refs/heads/main"]), base, "main は進んだまま");
    // gate の周は緑だった＝1 回目と 2 回目で結果が変わる行を、両方の面が撃っている。
    assert_eq!(row_value(&verify_rows(&state, &id), 2, "rc"), "0", "gate では同じ行が緑");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_exports_verdict_schema1() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let exported = fs::read_to_string(land::verdicts_path(&state)).expect("verdicts.jsonl を読める");
    assert_eq!(exported.lines().count(), 1, "land ごとに 1 行: {exported}");
    let line = exported.lines().next().unwrap_or_default();
    let pairs = vessel::fleet::json_lite::parse_object(line).expect("1 行の JSON");
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["schema", "run", "bead", "sha", "verdict", "evidence", "ts"],
        "面 5 の key 列（ADR-0004 §2.2・版番号に依らず固定）"
    );
    assert_eq!(value_of(&pairs, "schema"), "1");
    assert_eq!(value_of(&pairs, "run"), id);
    assert_eq!(value_of(&pairs, "verdict"), "PASS");
    assert_eq!(
        value_of(&pairs, "sha"),
        git(&repo, &["rev-parse", "refs/heads/main"]),
        "sha は land した commit"
    );
    assert!(
        value_of(&pairs, "evidence").ends_with("verdict.json"),
        "evidence は verdict.json の path: {}",
        value_of(&pairs, "evidence")
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_retires_worktree_by_move_and_keeps_branch() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let live = worktree_of(&repo, &id);
    assert!(live.exists(), "land の前は便の worktree が在る");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(retired.exists(), "retired/ へ move する");
    assert!(!live.exists(), "元の場所には残らない");
    // **削除しない**（N1.2）: 中身が move で運ばれている。
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    let branches = git(&repo, &["branch", "--list", &format!("scribe2/{id}")]);
    assert!(!branches.trim().is_empty(), "branch は消さない: {branches}");
    // main 実測用の tmp worktree だけは畳む。
    assert!(
        !repo.join(".worktrees").join("scribe2").join("verify").join(&id).exists(),
        "main 実測の tmp worktree は remove する"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_e2e_toy_repo_lands_one_bead_with_fake_runner() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    // intake → spawn → gate → land を **1 process で人手 0** で通す。
    let out = run_pipe(&[
        "run", "--contract", &path.display().to_string(), "--bead", "s2-41o",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
        "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "run は rc 0: {}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(text.contains("verdict=PASS"), "gate まで通る: {text}");
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert!(text.contains(&format!("landed={new}")), "land まで通る: {text}");
    assert_eq!(
        git(&repo, &["rev-list", "--count", &format!("{base}..{new}")]),
        "1",
        "toy repo に 1 便が載る"
    );
    let id = run_id_of(&out);
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    // 人由来の event は 1 件も無い（承認の要らない契約ゆえ）。**この行だけでは (b) の
    // code を測れない**（human を書くのは (c) の `ApprovalReceived` だけ）ので、
    // 「1 便が最後まで載った」ことを event の側からも測る。
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(!log.contains("\"actor\":\"human\""), "人手 0 で通る: {log}");
    let landed_events = log
        .lines()
        .filter(|line| line.contains("\"kind\":\"RunDone\"") && line.contains("\"stage\":\"Landed\""))
        .count();
    assert_eq!(landed_events, 1, "RunDone stage=Landed が 1 件: {log}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_resume_continues_from_implemented_in_new_process() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    // spawn までで process が終わる（＝gate の手前で落ちた便と同じ現在地）。
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // **別 process** が置き場だけを読んで続きを引く。
    let gated = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "Implemented → gate: {}", stderr_of(&gated));
    assert!(stdout_of(&gated).contains("verdict=PASS"), "{}", stdout_of(&gated));
    // もう一度 resume すると Gated(PASS) → land へ進む。
    let landed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "Gated → land: {}", stderr_of(&landed));
    assert!(stdout_of(&landed).contains("landed="), "{}", stdout_of(&landed));
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_refuses_run_without_commits() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // commit を作らない runner。spawn は Failed で終える（commit 0 は完了ではない）。
    run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", "true",
    ]);
    // 段だけを Implemented へ書き換える（＝台帳が壊れている / 手で進めた周）。
    // worktree は在って clean なので、**commits の検査だけ**が gate を止める。
    let forced = Command::new(bin())
        .args(["fleet", "record", "--state-dir"])
        .arg(&state)
        .args(["--kind", "RunStage", "--stage", "Implemented", "--run", &id, "--bead", "s2-2e5"])
        .output()
        .expect("binary を起動できる");
    assert_eq!(forced.status.code(), Some(i32::from(RC_OK)), "record は rc 0");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "commit 0 は前提違反で rc 1");
    assert!(
        stderr_of(&out).contains("commit が 1 本も無い"),
        "理由は commits（dirty ではない）: {}",
        stderr_of(&out)
    );
    assert!(!marker.exists(), "前提違反の周は lens を起動しない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_on_unlisted_lens_verdict() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    // 3 値の外を名乗る lens。**PASS へ倒さない**（AC3「偽の PASS 0 件」）。
    let lens = fake_lens(&marker, &lens_verdict("OK"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(3), "3 値外は rc 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    assert!(marker.exists(), "lens 自体は呼んでいる（判定に届いた周）");
    assert_eq!(
        value_of(&verdict_pairs(&state, &id), "verdict"),
        "INCONCLUSIVE",
        "未知の verdict を通さない"
    );
    clean(&[&repo, &state]);
}

// ── lens review（2026-09-09）で「測っていない」と名指しされた経路を塞ぐ歯 ──────

#[test]
fn pipe_gate_inconclusive_when_lens_count_is_not_one() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // 0 本（lens を呼ばずに通す）も 2 本（1 本で足りたことにする）も**判定できていない**。
    // 規則 1 行で gate が飾りになる形を塞ぐ（AC3「偽の PASS 0 件」）。
    for (count, bead) in [(0_u64, "s2-zero"), (2, "s2-two")] {
        let id = intake_bead(&repo, &state, &path, bead);
        let out = run_pipe(&[
            "spawn", "--run", &id, "--repo", &repo.display().to_string(),
            "--state-dir", &state.display().to_string(),
            "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
        ]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
        let rules = write_rules(&repo, &format!("lens{count}.toml"), count, 150_000);
        let gated = gate_with_rules(&repo, &state, &id, &rules, &lens);
        assert_eq!(gated.status.code(), Some(3), "lens {count} 本は判定不能で rc 3");
        assert!(
            stdout_of(&gated).contains("verdict=INCONCLUSIVE"),
            "lens {count} 本: {}",
            stdout_of(&gated)
        );
        // **land まで行かせない**（面 5 に PASS を残さない）。
        let landed = land_once(&repo, &state, &id);
        assert_eq!(landed.status.code(), Some(i32::from(RC_REFUSED)), "PASS でなければ land しない");
        // INCONCLUSIVE は終端でない＝次の便と write-set が交差する（`s2-07l.145`）。測り終えた
        // 便を `stop --run` で外してから次の周を回す（測る内容は 1 つも変えていない）。
        stop_run_ok(&state, &id);
    }
    assert!(!marker.exists(), "0 本の周は lens を起動しない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_when_lens_exits_nonzero() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // JSON は正しく吐くが rc≠0 で終える lens。**出力を信じて PASS へ倒さない**。
    let lens = format!("cat >/dev/null; echo '{}'; exit 7", lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(3), "lens が rc≠0 なら rc 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    assert!(
        value_of(&verdict_pairs(&state, &id), "evidence").contains("rc 7"),
        "理由に lens の rc が残る: {}",
        value_of(&verdict_pairs(&state, &id), "evidence")
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_inconclusive_when_lens_output_is_not_json() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // rc 0 だが JSON 行が無い lens。**読めなかったを通ったに化けさせない**。
    let lens = "cat >/dev/null; echo looks-fine".to_owned();
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(3), "parse 不能なら rc 3");
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    assert!(
        value_of(&verdict_pairs(&state, &id), "evidence").contains("JSON 行が無い"),
        "理由: {}",
        value_of(&verdict_pairs(&state, &id), "evidence")
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_passes_diff_to_lens_on_stdin() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let seen = state.join("stdin-bytes");
    // lens が **実際に受け取った byte 数**を書き出す（設計 §5.3 / FR9 の中心）。
    let lens = format!("wc -c > '{}'; echo '{}'", seen.display(), lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let received = fs::read_to_string(&seen).expect("lens が受けた byte 数を読める");
    let received = received.trim().to_owned();
    assert_ne!(received, "0", "diff を渡さずに lens を呼んでいない");
    assert_eq!(
        received,
        value_of(&verdict_pairs(&state, &id), "diff_bytes"),
        "lens が受けた byte 数と verdict.json の diff_bytes は同じ diff を指す"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_substitutes_contract_placeholder_in_lens_cmd() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let seen = state.join("lens-contract-arg");
    // fake lens が **受け取った argv**（置換後の cmd に埋まった path）を写す。
    // 置換していなければ `{contract}` の字面がそのまま残る。
    // **穴は 2 つ置く**。1 つだけだと `replacen(_, _, 1)` へ縮める変異が生き残る
    // （review 2026-09-10）。2 つ目が置換されなければ字面のまま file に残る。
    let lens = format!(
        "printf '%s\\n%s' '{{contract}}' '{{contract}}' > '{}'; cat >/dev/null; echo '{}'",
        seen.display(),
        lens_verdict("PASS")
    );
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let handed = fs::read_to_string(&seen).expect("lens が受けた値を読める");
    let handed: Vec<&str> = handed.lines().collect();
    assert_eq!(handed.len(), 2, "穴 2 つ分が渡る: {handed:?}");
    assert!(
        !handed.iter().any(|line| line.contains("{contract}")),
        "placeholder が 1 つでも置換されずに渡っている: {handed:?}"
    );
    let handed = handed.first().copied().unwrap_or_default().trim().to_owned();
    // **path であって本文ではない**（cmd は `sh -c` の 1 行なので、本文を埋めると
    // 契約の中の引用符 1 つで cmd の構造が変わる）。
    let handed = PathBuf::from(handed.trim());
    assert!(handed.is_absolute(), "契約 copy の絶対 path が渡る: {}", handed.display());
    let body = fs::read_to_string(&handed).expect("lens は渡された path から契約を読める");
    // 渡ったのが **この便の契約 copy** であること（別の file を指していない）。
    assert!(body.contains("縦 1 本を通す"), "契約の goal が読める: {body}");
    assert_eq!(
        handed,
        state.join("pipe").join(&id).join("contract.toml"),
        "run の契約 copy を指す"
    );
    clean(&[&repo, &state]);
}

/// gate は `--lens` の cmd の `{worktree}` へ **便の worktree** を埋める。
///
/// 本契約（`s2-07l.60`）の実利は「lens の context に憲法を載せる」ことで、その唯一の
/// 経路が gate → `{worktree}` → lens の `cwd` である。`substitute()` という純関数の中
/// だけを測る歯では、**呼び手が別の path を穴へ入れる退行**を捕まえられない——実測
/// 2026-09-10: `ask_lens(&substitute(cmd, &contract, worktree), …)` の第 3 引数を
/// `entry.state_dir` へ差し替えても workspace の 297 本が 1 本も落ちなかった。
/// ゆえに**穴の中身が正しいか**をここで測る（`{contract}` 側と対称にする）。
#[test]
fn pipe_gate_substitutes_worktree_placeholder_in_lens_cmd() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let seen = state.join("lens-worktree-arg");
    // fake lens が **受け取った argv** を写す。置換していなければ `{worktree}` の字面が残る。
    let lens = format!(
        "printf '%s' '{{worktree}}' > '{}'; cat >/dev/null; echo '{}'",
        seen.display(),
        lens_verdict("PASS")
    );
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let handed = fs::read_to_string(&seen).expect("lens が受けた値を読める");
    assert!(
        !handed.contains("{worktree}"),
        "placeholder が置換されずに渡っている: {handed}"
    );
    let handed = PathBuf::from(handed.trim());
    assert!(handed.is_absolute(), "絶対 path が渡る: {}", handed.display());
    // **便の worktree ちょうど**を指す。
    assert_eq!(
        handed,
        repo.join(".worktrees").join("scribe2").join(&id),
        "run の worktree を指す"
    );
    // 近い path を渡す退行を負例で外す（置き場も anchor の repo も worktree ではない）。
    assert_ne!(handed, state, "置き場を渡していない");
    assert_ne!(handed, repo, "anchor の repo を渡していない");
    // 憲法が載る経路である＝渡った dir は便の base の checkout である。
    assert!(
        handed.join(".git").exists(),
        "worktree の checkout を指す: {}",
        handed.display()
    );
    clean(&[&repo, &state]);
}


#[test]
fn pipe_gate_refuses_wrong_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let before = event_count(&state);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // Intake の便に gate は掛からない。**段違いは何もせず rc 1**（Failed で終端させない）。
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "段違いは rc 1");
    assert!(stderr_of(&out).contains("段は Intake である"), "理由: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before, "段違いは event を 1 件も書かない");
    assert!(!marker.exists(), "lens を起動しない");
    // 終端していないので、正しい段まで進めれば通る（resume できる）。
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    // 逆向きも同じ: Implemented の便に land は掛からない。
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_REFUSED)), "gate 前の land は rc 1");
    assert!(
        stderr_of(&landed).contains("段は Implemented である"),
        "理由: {}",
        stderr_of(&landed)
    );
    // **判定が済んだ**便に gate は 2 度掛からない（測り直せるのは INCONCLUSIVE の
    // 周だけ・設計 §5.3。ここは PASS ゆえ終端側である）。
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "1 度目は通る: {}", stderr_of(&gated));
    let again = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "2 度目の gate は rc 1");
    assert!(stderr_of(&again).contains("段は Gated である"), "理由: {}", stderr_of(&again));
    clean(&[&repo, &state]);
}

/// main 確認で**段①（write-set 照合）を読めなかった周**（Step の rc -1）は赤（main-red）
/// でなく `main-unmeasured` に倒す（gate §6 の INCONCLUSIVE と同じ極性・`.65` lens M3）。
/// **赤の段と同時に在っても**測れなかったが先に効く（写しの共通 verify は main で赤くなる
/// `verify-once.sh`）。fail-closed: finish（verdict export・Landed）にも main-green にも進まない。
/// main は squash で進んだまま（red と同じく auto revert しない・設計 §5.4）。
#[test]
fn pipe_land_turns_unstartable_verify_step_into_unmeasured() {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-once.sh"]"#);
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);

    let out = land_once_with_unreadable_diff(&repo, &state, &id);

    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "測れない周は rc 2: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("実測できない"), "赤ではなく測れないと名乗る: {}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("cmd=write-set") && stderr_of(&out).contains("stderr=diff の path を読めない"),
        "理由に段の名と stderr の 1 行が写る: {}",
        stderr_of(&out)
    );
    assert!(!stderr_of(&out).contains("赤い"), "赤を名乗らない: {}", stderr_of(&out));
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let last = log.lines().last().unwrap_or_default();
    assert!(last.contains("\"kind\":\"RunStage\""), "最終行は RunStage: {last}");
    assert!(last.contains("\"detail\":\"main-unmeasured\""), "最終行の detail は main-unmeasured: {last}");
    assert!(!log.contains("\"detail\":\"main-red\""), "main-red は書かない: {log}");
    assert!(!log.contains("\"stage\":\"Landed\""), "Landed へ進まない（fail-closed）: {log}");
    assert!(!land::verdicts_path(&state).exists(), "面 5 へ export しない");
    // squash は verify の前に済んでいる（設計 §5.4 の順序）＝main は進んだまま・revert しない。
    let now = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(now, base, "main は squash で進んだまま（red と同じ極性）");
    // 負例（同じ便を実 git で撃ち直すと、段① が読めて赤は無い＝この歯の理由は shim だけ）:
    // 2 度目の land は前提（verdict / stale base）で断られるので、ここでは segment の弁別だけ
    // 既存の pipe_land_reruns_common_verify_from_vessel_copy_on_main（rc≠0 → main-red）に委ねる。
    clean(&[&repo, &state]);
}

/// 負例: 走って **signal で死んだ** verify 行（`code()` が無く器は -1 と記す）は「読めなかった」
/// ではなく実測の赤＝従来どおり main-red（rc 1）。rc -1 の全数を測れなかったへ倒す実装は
/// ここで落ちる（gate と同じく**段の名と rc**で見る）。
#[test]
fn pipe_land_keeps_signal_killed_verify_line_as_red() {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-kill.sh"]"#);
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);

    let out = land_once(&repo, &state, &id);

    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "走って死んだ赤は rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("main が赤い"), "赤と名乗る: {}", stderr_of(&out));
    assert!(!stderr_of(&out).contains("実測できない"), "測れなかったと名乗らない: {}", stderr_of(&out));
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(log.contains("\"detail\":\"main-red\""), "main-red で残る: {log}");
    assert!(!log.contains("\"detail\":\"main-unmeasured\""), "main-unmeasured は書かない: {log}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_reports_unmeasured_main_apart_from_red() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    // main 実測用の tmp の置き場を塞ぐ＝**verify を 1 行も撃てない**。
    let blocked = repo.join(".worktrees").join("scribe2").join("verify").join(&id);
    fs::create_dir_all(&blocked).expect("tmp の置き場を塞げる");
    fs::write(blocked.join("occupied"), "x\n").expect("塞げる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "測れない周は rc 2");
    assert!(
        stderr_of(&out).contains("実測できない"),
        "**赤ではなく測れない**と名乗る: {}",
        stderr_of(&out)
    );
    assert!(!stderr_of(&out).contains("赤い"), "赤を名乗らない: {}", stderr_of(&out));
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(log.contains("\"detail\":\"main-unmeasured\""), "別の名で残す: {log}");
    assert!(!log.contains("\"detail\":\"main-red\""), "main-red は書かない: {log}");
    assert!(!land::verdicts_path(&state).exists(), "面 5 へ export しない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_removes_dirty_tmp_worktree() {
    let (repo, state) = repo_with_state();
    // detached（= main 実測の tmp）のときだけ中間物を作る verify 行。便の worktree は
    // branch 上なので clean のままで、retire の move が塞がれない。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-detached.sh"]"#]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    // **中間物で dirty になった tmp を leak させない**（設計 §5.4 の `--force`）。
    assert!(
        !repo.join(".worktrees").join("scribe2").join("verify").join(&id).exists(),
        "dirty な tmp worktree も畳む"
    );
    let listed = git(&repo, &["worktree", "list"]);
    assert!(!listed.contains("/verify/"), "worktree の登録も残らない: {listed}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_regates_after_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 道具が足りない周（`--lens` を渡し忘れた便）。INCONCLUSIVE は「測れなかった」で
    // あって「落ちた」ではないので、**ここで終端しない**。
    let first = gate_once(&repo, &state, &id, None);
    assert_eq!(first.status.code(), Some(3), "測れなかった周の rc は 3");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "INCONCLUSIVE");

    // 道具を揃えて撃ち直す。**段が Gated でも通る**のが本便で広げた 1 分岐である。
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let second = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(
        second.status.code(),
        Some(i32::from(RC_OK)),
        "道具を揃えた測り直しは通る: {}",
        stderr_of(&second)
    );
    assert!(marker.exists(), "2 度目は lens を起動する");
    assert_eq!(
        value_of(&verdict_pairs(&state, &id), "verdict"),
        "PASS",
        "verdict.json は新しい判定で上書きされる"
    );

    // **前の INCONCLUSIVE は event に残る**（append-only・書き換えない）。
    let gated: Vec<String> = events(&state)
        .into_iter()
        .filter(|event| event.run == id && event.stage == Some(Stage::Gated))
        .filter_map(|event| event.detail)
        .collect();
    assert_eq!(
        gated,
        vec!["verdict:INCONCLUSIVE".to_owned(), "verdict:PASS".to_owned()],
        "測り直しは 2 件目を追記する（1 件目を書き換えない）"
    );
    // 吸収状態が解けている＝そのまま land まで進む。
    let landed = land_once(&repo, &state, &id);
    assert_eq!(
        landed.status.code(),
        Some(i32::from(RC_OK)),
        "測り直した便は land できる: {}",
        stderr_of(&landed)
    );
    // **land 済みの便は測り直せない**。ここを開けると Gated を書き直して land を 2 度
    // 通す口になる（測り直しが開くのは Gated の 1 段だけ）。
    let after_land = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(
        after_land.status.code(),
        Some(i32::from(RC_REFUSED)),
        "Landed からの再 gate は rc 1"
    );
    assert!(
        stderr_of(&after_land).contains("段は Landed である"),
        "理由は段である: {}",
        stderr_of(&after_land)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_regates_after_relaxing_cap() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // **cap 超過も「測れなかった」**（lens を呼べていない）。契約が名指す測り直しの
    // 主用途はここで、`--lens` の渡し忘れだけを測っていると `--rules` 経由の周が丸ごと
    // 素通りする。
    let tight = write_rules(&repo, "tight.toml", 1, 1);
    let first = gate_with_rules(&repo, &state, &id, &tight, &lens);
    assert_eq!(first.status.code(), Some(3), "cap 超過の rc は 3");
    assert!(!marker.exists(), "cap 超過の周は lens を起動しない");
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE");
    let bytes: u64 = value_of(&pairs, "diff_bytes").parse().unwrap_or(0);

    // **道具を揃えずに撃ち直しても通らない**（測り直しは検査を素通りする口ではない）。
    let retight = gate_with_rules(&repo, &state, &id, &tight, &lens);
    assert_eq!(retight.status.code(), Some(3), "cap のままなら 2 度目も 3");
    assert!(!marker.exists(), "2 度目も cap 超過なら lens を起動しない");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "INCONCLUSIVE");

    // 予算を実測値まで緩めて撃ち直す（道具を揃えるのは人の手番）。
    let relaxed = write_rules(&repo, "relaxed.toml", 1, bytes);
    let second = gate_with_rules(&repo, &state, &id, &relaxed, &lens);
    assert_eq!(
        second.status.code(),
        Some(i32::from(RC_OK)),
        "cap を緩めた測り直しは通る: {}",
        stderr_of(&second)
    );
    assert!(marker.exists(), "2 度目は lens を起動する");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS");
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_regates_after_fixing_lens_count() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // 規則が lens 2 本を定める周は「1 本で足りたことにしない」ため INCONCLUSIVE。
    // **規則の側の不備も道具の不足**なので、直せば同じ便を測り直せる。
    let two = write_rules(&repo, "two-lenses.toml", 2, 100_000);
    let first = gate_with_rules(&repo, &state, &id, &two, &lens);
    assert_eq!(first.status.code(), Some(3), "lens_count≠1 の rc は 3");
    assert!(!marker.exists(), "本数が合わない周は lens を起動しない");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "INCONCLUSIVE");

    // **規則を直さずに撃ち直しても通らない**（測り直しは本数照合を外す口ではない）。
    let retry = gate_with_rules(&repo, &state, &id, &two, &lens);
    assert_eq!(retry.status.code(), Some(3), "規則がそのままなら 2 度目も 3");
    assert!(!marker.exists(), "本数が合わないままなら lens を起動しない");

    let one = write_rules(&repo, "one-lens.toml", 1, 100_000);
    let second = gate_with_rules(&repo, &state, &id, &one, &lens);
    assert_eq!(
        second.status.code(),
        Some(i32::from(RC_OK)),
        "規則を直した測り直しは通る: {}",
        stderr_of(&second)
    );
    assert!(marker.exists(), "2 度目は lens を起動する");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "PASS");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_fails_regate_on_dirty_worktree() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let first = gate_once(&repo, &state, &id, None);
    assert_eq!(first.status.code(), Some(3), "測れなかった周の rc は 3");

    // 道具（lens・cap）を揃える過程で worktree を汚した周。**測り直しに来た便でも
    // worktree の事実の違反は Failed で終端する**——precheck の極性を段で変えると
    // 「段の検査は cli / worktree の事実は gate」の分離が濁るため（planner 裁定
    // 2026-09-10 Q3 案B）。運用は「道具を揃える前に worktree を clean へ戻す」。
    fs::write(worktree_of(&repo, &id).join("dirty.txt"), "x\n").expect("汚せる");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "前提違反は rc 1");
    assert!(stderr_of(&out).contains("clean でない"), "理由: {}", stderr_of(&out));
    assert!(!marker.exists(), "lens を起動しない");
    assert!(
        show_line(&repo, &state, &id).contains("stage=Failed"),
        "測り直しの周も precheck 違反は終端する: {}",
        show_line(&repo, &state, &id)
    );

    // **掃除しても引けない**（Failed は終端）。この非対称は測って残す。
    fs::remove_file(worktree_of(&repo, &id).join("dirty.txt")).expect("掃除できる");
    let again = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "Failed からは撃ち直せない");
    assert!(
        stderr_of(&again).contains("段は Failed である"),
        "理由: {}",
        stderr_of(&again)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_refuses_regate_after_fail() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let failed_marker = state.join("lens-fail");
    let failing = fake_lens(&failed_marker, &lens_verdict("FAIL"));
    let first = gate_once(&repo, &state, &id, Some(&failing));
    assert_eq!(first.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verdict"), "FAIL");

    // **FAIL は終端のまま**。契約の verify が赤い便は測り直しても赤いので、撃ち直す口を
    // 開けない（開けると「壊れたまま進む」経路になる）。**段違いの一般則どおり何もしない**。
    let before = event_count(&state);
    let passing_marker = state.join("lens-pass");
    let passing = fake_lens(&passing_marker, &lens_verdict("PASS"));
    let second = gate_once(&repo, &state, &id, Some(&passing));
    assert_eq!(second.status.code(), Some(i32::from(RC_REFUSED)), "FAIL からの再 gate は rc 1");
    assert!(
        stderr_of(&second).contains("FAIL"),
        "断る理由は「段は Gated」ではなく判定である: {}",
        stderr_of(&second)
    );
    assert!(!passing_marker.exists(), "lens を起動しない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    assert_eq!(
        value_of(&verdict_pairs(&state, &id), "verdict"),
        "FAIL",
        "判定は書き換わらない（PASS の lens を渡しても）"
    );
    // resume も FAIL を測り直しへ案内しない（`next=` を名乗るのは INCONCLUSIVE だけ）。
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の resume は rc 1");
    assert!(
        !stdout_of(&resumed).contains("next="),
        "次の一手を名乗らない（終端である）: {}",
        stdout_of(&resumed)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_gate_refuses_regate_without_readable_verdict() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let first = gate_once(&repo, &state, &id, None);
    assert_eq!(first.status.code(), Some(3), "測れなかった周の rc は 3");

    // **判定が読めない周は測り直さない**（fail-closed・C11.2）。前提は「Gated ∧ verdict が
    // INCONCLUSIVE」であって「Gated ∧ PASS でも FAIL でもない」ではない——後者だと
    // verdict.json が消えた / 壊れた便まで撃ち直せてしまい、「測れなかった」ではなく
    // **判定の記録が無い**便が gate を通る（自前の変異 M04 が生き延びた経路）。
    let judged = state.join("pipe").join(&id).join("verdict.json");
    let before = event_count(&state);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    // `verdict_of` は 3 つの経路を等しく「読めない」へ畳む。**3 つとも測る**——1 つだけ
    // 測ると、残り 2 つを INCONCLUSIVE へ倒す実装（＝測れなかった便を測り直してよい便に
    // 化けさせる）が素通りする。
    for (label, body) in [
        ("file が無い", None),
        ("JSON が壊れている", Some("{ 壊れた\n")),
        (
            "verdict が 3 値の外",
            Some(r#"{"schema":1,"run":"x","verdict":"MAYBE","evidence":"","verify_red":0,"diff_bytes":0,"ts":"t"}"#),
        ),
    ] {
        match body {
            None => {
                fs::remove_file(&judged).ok();
            }
            Some(text) => fs::write(&judged, text).expect("verdict.json を書ける"),
        }
        let out = gate_once(&repo, &state, &id, Some(&lens));
        assert_eq!(
            out.status.code(),
            Some(i32::from(RC_REFUSED)),
            "{label}: 読めない周は rc 1"
        );
        assert!(
            stderr_of(&out).contains("読めない"),
            "{label}: **判定が読めない**と名乗る（INCONCLUSIVE と同じ扱いにしない）: {}",
            stderr_of(&out)
        );
        assert!(!marker.exists(), "{label}: lens を起動しない");
        assert_eq!(event_count(&state), before, "{label}: event を 1 件も書かない");
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_resume_reports_next_gate_on_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let first = gate_once(&repo, &state, &id, None);
    assert_eq!(first.status.code(), Some(3), "測れなかった周の rc は 3");

    // resume は **自動で測り直さない**（道具の不足は人が直す）。`--lens` を渡してあっても
    // 撃たず、次に何をすればよいかだけを名乗って止まる。
    let before = event_count(&state);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(resumed.status.code(), Some(3), "測れていない便の resume は rc 3");
    assert!(
        stdout_of(&resumed).contains("next=gate"),
        "次の一手を名乗る: {}",
        stdout_of(&resumed)
    );
    assert!(!marker.exists(), "resume は lens を起こさない（撃ち直すのは人）");
    assert_eq!(event_count(&state), before, "何も書かない");
    // **land を試して断られる形（吸収状態）に戻っていない**。
    assert!(
        !stderr_of(&resumed).contains("PASS でない"),
        "land を試さない: {}",
        stderr_of(&resumed)
    );
    clean(&[&repo, &state]);
}

// ── (c) 承認 Blocked と resume（設計 §8 (c)・FR15 / FR16 / AC5・憲法 A1 / C7.2） ──

/// event log の全行を型で読む（file が無ければ空）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn events(state: &Path) -> Vec<Event> {
    fs::read_to_string(state.join("fleet").join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| Event::from_line(line).expect("event の 1 行を読める"))
        .collect()
}

/// commit を 1 本作る fake runner。**起動されたら marker を残す**——「起きていない」を
/// rc でなく効果で測るための痕跡である。
fn runner_cmd(marker: &Path) -> String {
    format!(
        "touch {} && echo x >> src/lib.rs && git add -A && git commit -q -m runner",
        marker.display()
    )
}

/// `fleet record` で `ApprovalReceived` を 1 件直接積む。**`pipe approve` を通さない**
/// 経路で、書き手側の逐語検査を素通りした event が関門を開けないことを測るのに使う。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn record_approval(state: &Path, id: &str, extra: &[&str]) -> Output {
    let mut args = vec!["fleet".to_owned(), "record".to_owned(), "--state-dir".to_owned()];
    args.push(state.display().to_string());
    args.extend(
        ["--kind", "ApprovalReceived", "--run", id, "--bead", "s2-2e5"]
            .iter()
            .map(|item| (*item).to_owned()),
    );
    args.extend(extra.iter().map(|item| (*item).to_owned()));
    Command::new(bin()).args(&args).output().expect("binary を起動できる")
}

/// 3 クラスを名乗る契約で intake → spawn まで撃ち、Blocked で止まった便の
/// id と stdout を返す。stdout は「いまどの段に居るか」の主張なので測る対象である。
fn blocked(repo: &Path, state: &Path, marker: &Path, classes: &str) -> (String, String) {
    let path = write_contract(repo, &[], &[classes]);
    let id = intake(repo, state, &path);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", &runner_cmd(marker),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BLOCKED)),
        "承認が要る便は rc 3 で止まる: {}",
        stderr_of(&out)
    );
    (id, stdout_of(&out))
}

#[test]
fn pipe_approval_blocks_before_spawn_when_contract_declares_class() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    // **2 クラス**で撃つ。1 クラスだと detail の連結が恒等になり、区切りを測れない。
    let (id, said) = blocked(&repo, &state, &marker, r#"classes = ["delete", "publish"]"#);
    assert!(said.contains(&format!("run={id} stage=Blocked")), "止まった先を stdout で名乗る: {said}");
    // **数字そのものが約束である**（設計 §5.5）。定数を辿るだけの assert は、定数が
    // 動いたときに歯も黙って追随する——外形の 3 はここで literal に留める。
    assert_eq!(RC_BLOCKED, 3, "人の手番で止まっている周の rc は 3");
    // **効果で測る**: 止めたと名乗るだけでなく、runner が 1 度も起きていない。A1 の
    // 「実行前」は、消す / 出す / 使うが**起きた後**に聞くのでは意味が無い。
    assert!(!marker.exists(), "runner を起こさない");
    assert!(
        !repo.join(".worktrees").join("scribe2").join(&id).exists(),
        "worktree も切らない"
    );
    let mine: Vec<Event> = events(&state).into_iter().filter(|found| found.run == id).collect();
    let requested = mine
        .iter()
        .find(|found| found.kind == EventKind::ApprovalRequested)
        .expect("ApprovalRequested を記帳する");
    assert_eq!(
        requested.detail.as_deref(),
        Some("delete+publish"),
        "何のクラスで止めたかを名指す（複数なら全部・区切りは +）"
    );
    assert_eq!(requested.actor, "machine", "止めたのは機械であって人の event ではない");
    assert!(
        mine.iter().any(|found| found.stage == Some(Stage::Blocked)),
        "段は Blocked に落ちる"
    );
    let line = show_line(&repo, &state, &id);
    assert!(line.contains("stage=Blocked"), "{line}");
    assert!(line.contains("approved=false"), "{line}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_records_verbatim_as_human_event() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let (id, _) = blocked(&repo, &state, &marker, r#"classes = ["publish"]"#);
    // 引用符も全角も入った 1 行を **要約せずそのまま** 通す（C7.2）。前後の空白と
    // 大文字を混ぜてあるのは、正規化（trim / 小文字化）を「そのまま」と言い張れない
    // ようにするためである——fixture が綺麗だと歯は正規化を見逃す。
    let words = r#"  OK：出してよい（user 逐語 2026-09-09）："推奨で進めて"  "#;
    let out = run_pipe(&[
        "approve", "--run", &id, "--words", words,
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let human: Vec<Event> =
        events(&state).into_iter().filter(|found| found.actor == "human").collect();
    assert_eq!(human.len(), 1, "人由来の event は承認の 1 件だけである（FR22 の計測面）");
    let received = human.first().expect("承認 event が 1 件在る");
    assert_eq!(received.kind, EventKind::ApprovalReceived, "種類は ApprovalReceived");
    assert_eq!(received.detail.as_deref(), Some(words), "逐語をそのまま持つ");
    assert_eq!(received.bead, "s2-2e5", "どの契約への承認かを持つ");
    let line = show_line(&repo, &state, &id);
    assert!(line.contains("approved=true"), "{line}");
    // 承認は「許し」であって「前進」ではない——段を動かすのは resume である。
    assert!(line.contains("stage=Blocked"), "{line}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_refuses_empty_words() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let (id, _) = blocked(&repo, &state, &marker, r#"classes = ["consume"]"#);
    let before = event_count(&state);
    // 空も空白だけも承認ではない。「聞いた形」だけが残る記録を作らない。
    for words in ["", "   "] {
        let out = run_pipe(&[
            "approve", "--run", &id, "--words", words,
            "--state-dir", &state.display().to_string(),
        ]);
        assert_eq!(
            out.status.code(),
            Some(i32::from(RC_REFUSED)),
            "空の逐語を承認にしない: {words:?}"
        );
    }
    assert_eq!(event_count(&state), before, "1 byte も書かない");
    assert!(show_line(&repo, &state, &id).contains("approved=false"), "承認は立たない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_resume_spawns_after_received() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let (id, _) = blocked(&repo, &state, &marker, r#"classes = ["delete"]"#);
    let approved = run_pipe(&[
        "approve", "--run", &id, "--words", "消してよい",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(approved.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&approved));
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", &runner_cmd(&marker),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    // 効果で測る: 承認の後は runner が実際に起き、便が先の段へ進む。
    assert!(marker.exists(), "承認の後は runner が起きる");
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段が進む");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_resume_stays_blocked_without_received() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let (id, _) = blocked(&repo, &state, &marker, r#"classes = ["publish"]"#);
    let before = event_count(&state);
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", &runner_cmd(&marker),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "未承認の resume は rc 3");
    assert!(!marker.exists(), "runner を起こさない");
    // 待っている事実は Blocked が既に持っている。resume のたびに積むと
    // 「何回聞いたか」が事実と食い違う。
    assert_eq!(event_count(&state), before, "何も書かない");

    // **資格の無い `ApprovalReceived` では関門は開かない**（憲法 C7.2・planner 裁定 Q1）。
    // `fleet record` は公開の口なので、書き手（`pipe approve`）の逐語検査だけでは
    // 「承認は event に残った逐語だけ」を守れない——読み手が資格を見る。
    for disqualified in [
        vec!["--actor", "machine"],
        vec!["--actor", "human"],
        vec!["--actor", "human", "--detail", "   "],
        vec!["--actor", "machine", "--detail", "出してよい"],
    ] {
        let wrote = record_approval(&state, &id, &disqualified);
        assert_eq!(
            wrote.status.code(),
            Some(i32::from(RC_OK)),
            "event を積むこと自体はできる（塞ぐのは読み手である）: {}",
            stderr_of(&wrote)
        );
        assert!(
            show_line(&repo, &state, &id).contains("approved=false"),
            "資格の無い承認で関門は開かない: {disqualified:?}"
        );
        let again = run_pipe(&[
            "resume", "--run", &id, "--repo", &repo.display().to_string(),
            "--state-dir", &state.display().to_string(),
            "--runner", &runner_cmd(&marker),
        ]);
        assert_eq!(
            again.status.code(),
            Some(i32::from(RC_BLOCKED)),
            "資格の無い承認の後も rc 3 のまま: {disqualified:?}"
        );
        assert!(!marker.exists(), "runner を起こさない: {disqualified:?}");
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_unlisted_class_value_is_rejected_at_intake() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[r#"classes = ["deploy"]"#]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "名簿に無いクラスを通さない");
    let err = stderr_of(&out);
    assert!(err.contains("deploy"), "断る値を名指す: {err}");
    for listed in ["delete", "publish", "consume"] {
        assert!(err.contains(listed), "取れる値を全部見せる: {err}");
    }
    assert_eq!(event_count(&state), 0, "断った便は 1 行も記帳しない");
    // **弁別**: 断っているのは「classes が在ること」ではなく **値**である。
    let listed = write_contract(&repo, &[], &[r#"classes = ["publish"]"#]);
    let ok = run_pipe(&[
        "intake", "--contract", &listed.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
    ]);
    assert_eq!(ok.status.code(), Some(i32::from(RC_OK)), "名簿に在る値は通す: {}", stderr_of(&ok));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_blocks_in_one_shot_run() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let path = write_contract(&repo, &[], &[r#"classes = ["publish"]"#]);
    // **一発経路**（intake → spawn → gate → land を 1 process で通す）でも関門は効く。
    // 段ごとの口だけを測ると、この経路だけ素通りする実装に気づけない——関門は唯一の
    // 起動口 `spawn()` に在るという主張を、経路の側から裏書きする歯である。
    let out = run_pipe(&[
        "run", "--contract", &path.display().to_string(), "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
        "--runner", &runner_cmd(&marker),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BLOCKED)),
        "一発経路でも spawn の手前で止まる: {}",
        stderr_of(&out)
    );
    assert!(!marker.exists(), "runner を起こさない");
    let id = run_id_of(&out);
    assert!(!id.is_empty(), "止まった周も run id を出す: {}", stdout_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Blocked"), "段は Blocked に落ちる");
    clean(&[&repo, &state]);
}

// ── (e) 到達点の計測（設計 §8 (e)・§9・FR22 / AC1 / AC2・憲法 A1） ──────────

/// `pipe report` を 1 回撃つ。
fn report_once(state: &Path) -> Output {
    run_pipe(&["report", "--state-dir", &state.display().to_string()])
}

/// `fleet record` で人由来の event を 1 件積む（**`pipe approve` を通さない**）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn record_human_stage(state: &Path, id: &str) -> Output {
    Command::new(bin())
        .args(["fleet", "record", "--state-dir"])
        .arg(state)
        .args([
            "--kind", "RunStage", "--actor", "human", "--run", id, "--bead", "s2-2e5",
            "--stage", "Intake", "--detail", "手で段を動かした",
        ])
        .output()
        .expect("binary を起動できる")
}

#[test]
fn pipe_report_counts_human_events() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    // 1 便を land まで通す（機械だけで進む便）。
    let landed_id = gated_pass(&repo, &state, &path, &marker);
    let landed = land_once(&repo, &state, &landed_id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    // もう 1 便は intake で止める（`landed` に数えない側）。
    let open_id = intake_bead(&repo, &state, &path, "s2-open");
    let approved = run_pipe(&[
        "approve", "--run", &open_id, "--words", "推奨で進めて",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(approved.status.code(), Some(i32::from(RC_OK)), "approve: {}", stderr_of(&approved));

    let out = report_once(&state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "report は rc 0: {}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out).trim(),
        "runs=2 landed=1 human_events=1 human_events_other_than_approval=0",
        "到達点の 1 行（設計 §5.8）"
    );

    // **approval 以外の人由来 event は別に数える**——ここが 0 であることが到達点の主張
    // なので、0 のままにしか動かない数え方だと主張を測れない。
    let recorded = record_human_stage(&state, &open_id);
    assert_eq!(recorded.status.code(), Some(i32::from(RC_OK)), "record: {}", stderr_of(&recorded));
    let after = report_once(&state);
    assert_eq!(
        stdout_of(&after).trim(),
        "runs=2 landed=1 human_events=2 human_events_other_than_approval=1",
        "approval 以外の人由来 event を数える"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_report_returns_rc2_on_malformed_store() {
    let (repo, state) = repo_with_state();
    let events = state.join("fleet").join("events.jsonl");
    fs::create_dir_all(state.join("fleet")).expect("dir を作れる");
    fs::write(&events, "こわれ\n").expect("壊れた行を書ける");
    // **数えられなかったを 0 に化けさせない**（C11.2）。到達点の 1 行は「人手 0」を
    // 主張する面なので、読めない台帳から 0 を出すと**偽の全クリア**そのものになる。
    let out = report_once(&state);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない台帳は rc 2");
    assert!(out.stdout.is_empty(), "rc 2 でも数を出さない");
    assert!(
        !stdout_of(&out).contains("human_events_other_than_approval=0"),
        "0 を名乗らない: {}",
        stdout_of(&out)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_pr_cmd_runs_without_approval_event() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let before = git(&repo, &["rev-parse", "refs/heads/main"]);
    let sent = state.join("pr-args");
    // **自 repo への PR は「出す」ではない**（憲法 A4.3・ADR-0008）。main を動かさず
    // branch も PR も閉じられる＝可逆ゆえ、承認 event を積まない周でも道具は起動する。
    // 3 クラスの判定は契約の自己申告（`classes`）だけに効き、seam を使ったことから
    // 導出しない（publish を名乗る契約は従来どおり spawn の手前で Blocked＝別の歯が守る）。
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--pr-cmd", &format!("printf '%s %s' {{branch}} {{base}} > '{}'", sent.display()),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_OK)),
        "承認 event 無しでも rc 0: {}",
        stderr_of(&out)
    );
    // **道具が起動したことを file で測る**（器は道具の中身を知らない）。
    assert_eq!(
        fs::read_to_string(&sent).expect("seam へ渡した引数を読める"),
        format!("scribe2/{id} {before}"),
        "`{{branch}}` と `{{base}}` を置換して渡す"
    );
    // **main は動かさない**（merge は人が押す・A4.3 の可逆はここに乗っている）。
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        before,
        "main は 1 byte も動かない"
    );
    assert!(
        show_line(&repo, &state, &id).contains("stage=Landed"),
        "段は Landed へ進む: {}",
        show_line(&repo, &state, &id)
    );
    assert!(
        stdout_of(&out).contains("landed=pr"),
        "PR を出した形だと名乗る: {}",
        stdout_of(&out)
    );
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(
        log.contains("\"stage\":\"Landed\"") && log.contains("\"detail\":\"pr\""),
        "Landed detail=pr で終える: {log}"
    );
    assert!(
        !land::verdicts_path(&state).exists(),
        "面 5 は main へ載った便の記録ゆえ、PR の段階では書かない"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_pr_cmd_pushes_branch_without_moving_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);

    // **道具の失敗で便を終端させない**: push も PR 作成も network で落ちうるので、
    // `Failed` を焼くと再試行できない便が残る。rc 1 で何も書かず段も動かさない——だから
    // この後そのまま成功へ進める（この 2 段で「何も書かない」を測っている）。
    let broken_seam = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--pr-cmd", "exit 7",
    ]);
    assert_eq!(broken_seam.status.code(), Some(i32::from(RC_REFUSED)), "道具が落ちた周は rc 1");
    assert!(
        stderr_of(&broken_seam).contains("rc 7"),
        "道具の rc を理由に写す: {}",
        stderr_of(&broken_seam)
    );
    assert!(
        !show_line(&repo, &state, &id).contains("stage=Landed"),
        "段も動かない: {}",
        show_line(&repo, &state, &id)
    );

    // seam は `{branch}` `{base}` を置換して `sh -c` する。**道具の中身は器が知らない**
    // ので、置換の結果を file へ写して測る。
    let sent = state.join("pr-args");
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--pr-cmd", &format!("printf '%s %s' {{branch}} {{base}} > '{}'", sent.display()),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "承認 event 無しでも rc 0: {}", stderr_of(&out));
    let args = fs::read_to_string(&sent).expect("seam へ渡した引数を読める");
    assert_eq!(
        args,
        format!("scribe2/{id} {base}"),
        "`{{branch}}` と `{{base}}` を置換して渡す"
    );
    // **main は動かさない**（merge は人が押す・憲法 A4.3）。
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        base,
        "この形では main を進めない"
    );
    assert!(
        stdout_of(&out).contains("landed=pr"),
        "PR を出した形だと名乗る: {}",
        stdout_of(&out)
    );
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    assert!(
        log.contains("\"stage\":\"Landed\"") && log.contains("\"detail\":\"pr\""),
        "Landed detail=pr で終える: {log}"
    );
    // 便の worktree は畳まない（**merge は人が押すまで終わっていない**）。
    assert!(worktree_of(&repo, &id).exists(), "PR 待ちの worktree は残す");
    assert!(
        !land::verdicts_path(&state).exists(),
        "面 5 は main へ載った便の記録ゆえ、PR の段階では書かない"
    );

    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_pr_cmd_ignores_moved_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);

    // **別便が main を進めた状況**を作る。squash の口はここで stale base を理由に断るが、
    // PR の口は ref を 1 本も動かさないので CAS の old が要らない——ここで base を縛ると
    // main が動いた瞬間に PR を出せなくなる（自己ホストの便が最も踏む）。
    fs::write(repo.join("other.txt"), "x\n").expect("別便の変更を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(moved, base, "main が進んでいる");

    let sent = state.join("pr-base");
    let out = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--pr-cmd", &format!("printf '%s' {{base}} > '{}'", sent.display()),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_OK)),
        "main が動いていても PR は出せる: {}",
        stderr_of(&out)
    );
    // **`{base}` は便が記録した base**（いまの main ではない）＝PR の比較先は便の出発点。
    assert_eq!(
        fs::read_to_string(&sent).expect("seam へ渡した base を読める"),
        base,
        "便の base を渡す（現在の main へ滑らせない）"
    );
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        moved,
        "main は 1 byte も動かさない"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_land_pr_cmd_refuses_empty_or_missing_value() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let before = event_count(&state);

    // 値欠け（SRS NFR4「黙って落とさない」）。
    let missing = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--pr-cmd",
    ]);
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "値欠けは rc 1");
    assert!(
        stderr_of(&missing).contains("値が無い"),
        "値欠けだと名乗る（squash 経路へ滑らせない）: {}",
        stderr_of(&missing)
    );

    // **空文字**。`sh -c ""` は rc 0 で終わるので、素通しすると 1 行も公開していないのに
    // 「PR を出した」を記帳する（何もしていないのに「やった」が残る）。
    let empty = run_pipe(&[
        "land", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--pr-cmd", "",
    ]);
    assert_eq!(empty.status.code(), Some(i32::from(RC_REFUSED)), "空の seam は rc 1");
    assert!(
        !show_line(&repo, &state, &id).contains("stage=Landed"),
        "段も動かない: {}",
        show_line(&repo, &state, &id)
    );
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    clean(&[&repo, &state]);
}

/// Gated の便を `--pr-cmd` 形で land する（main は動かず worktree も残る）。
fn land_pr(repo: &Path, state: &Path, id: &str) {
    let sent = state.join("pr-args");
    let out = run_pipe(&[
        "land", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--pr-cmd", &format!("printf '%s' {{branch}} > '{}'", sent.display()),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PR 形の land は rc 0: {}", stderr_of(&out));
}

/// `--pr-cmd` 形で land した便の id。
fn landed_pr(repo: &Path, state: &Path, contract: &Path, marker: &Path) -> String {
    let id = gated_pass(repo, state, contract, marker);
    land_pr(repo, state, &id);
    id
}

/// retire を 1 回撃つ。
fn retire_once(repo: &Path, state: &Path, id: &str) -> Output {
    run_pipe(&[
        "retire", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ])
}

#[test]
fn pipe_retire_moves_pr_landed_worktree_and_keeps_branch() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = landed_pr(&repo, &state, &path, &marker);
    let live = worktree_of(&repo, &id);
    // `--pr-cmd` 形は worktree を畳まない（merge は人が押す）＝retire の入口の前提である。
    assert!(live.exists(), "PR 形の land の後も便の worktree は在る");

    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "retire は rc 0: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(
        stdout_of(&out).contains(&format!("retired={}", retired.display())),
        "畳んだ先を名乗る: {}",
        stdout_of(&out)
    );
    // **削除しない**（N1.2）: 中身が move で運ばれている。
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    assert!(!live.exists(), "元の場所には残らない");
    let branches = git(&repo, &["branch", "--list", &format!("scribe2/{id}")]);
    assert!(!branches.trim().is_empty(), "branch は消さない: {branches}");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let last = log.lines().rfind(|line| !line.is_empty()).unwrap_or_default();
    assert!(
        last.contains("\"stage\":\"Landed\"") && last.contains("\"detail\":\"retired\""),
        "最終行は Landed detail=retired（段は Landed のまま）: {last}"
    );
    let after = event_count(&state);

    // 2 度目は前提（worktree が在る）を満たさない＝**rc 1 で何も書かない**。畳んだ先へ
    // 2 周目の move を当てると、retired/<id>/<id> のような入れ子が静かに生まれる。
    let again = retire_once(&repo, &state, &id);
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "2 度目は rc 1");
    assert_eq!(event_count(&state), after, "前提違反は event を 1 件も書かない");
    assert!(retired.exists(), "畳んだ先は在るまま");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_retire_refuses_unless_landed_and_clean() {
    // **2 つの前提は「断ってから解いて通す」で測る**。rc 1 だけを見ると、subcommand を
    // 持っていない器でも同じ rc 1 が返るので歯が空虚になる（stderr の文言は pin しない）。
    //
    // (a) 段が Gated のまま＝**終端していない便の worktree は畳まない**。
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let before = event_count(&state);
    let early = retire_once(&repo, &state, &id);
    assert_eq!(early.status.code(), Some(i32::from(RC_REFUSED)), "Landed 以外は rc 1");
    assert!(worktree_of(&repo, &id).exists(), "断った周は worktree を動かさない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    // 段だけを解くと同じ便が通る＝上の rc 1 は**段**を理由にしている。
    land_pr(&repo, &state, &id);
    let landed = retire_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "Landed なら通る: {}", stderr_of(&landed));
    clean(&[&repo, &state]);

    // (b) Landed でも worktree が dirty なら畳まない（fail-closed・untracked も数える）。
    // move は中身ごと運ぶので、未 commit の仕事を持った worktree を黙って動かすと
    // 「どこへ行ったか」が便の外から読めなくなる。
    let (dirty_repo, dirty_state) = repo_with_state();
    let dirty_path = write_contract(&dirty_repo, &[], &[]);
    let dirty_marker = dirty_state.join("lens-ran");
    let dirty_id = landed_pr(&dirty_repo, &dirty_state, &dirty_path, &dirty_marker);
    let live = worktree_of(&dirty_repo, &dirty_id);
    let stray = live.join("dirty.txt");
    fs::write(&stray, "x\n").expect("worktree を汚せる");
    let dirty_before = event_count(&dirty_state);
    let out = retire_once(&dirty_repo, &dirty_state, &dirty_id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "dirty な worktree は rc 1");
    assert!(live.exists(), "断った周は worktree を動かさない");
    assert!(stray.exists(), "汚れもそのまま残す（掃除しない）");
    assert_eq!(event_count(&dirty_state), dirty_before, "event を 1 件も書かない");
    let dirty_retired = dirty_repo.join(".worktrees").join("scribe2").join("retired").join(&dirty_id);
    assert!(!dirty_retired.exists(), "retired/<id> を作らない");
    // 汚れだけを拭うと同じ便が通る＝上の rc 1 は**clean**を理由にしている。
    fs::remove_file(&stray).expect("汚れを拭える");
    let cleaned = retire_once(&dirty_repo, &dirty_state, &dirty_id);
    assert_eq!(cleaned.status.code(), Some(i32::from(RC_OK)), "clean なら通る: {}", stderr_of(&cleaned));
    assert!(dirty_retired.exists(), "畳んだ先が出来る");
    clean(&[&dirty_repo, &dirty_state]);
}

/// 同一変更の 2 便: 2 本目が `Failed detail=rebase-empty` で終端した後、その worktree を
/// `pipe retire` が畳む（`s2-07l.128`）。成果は既に main に在り**入れ物だけが残る**形は
/// `--pr-cmd` 形の `Landed` と同じで、畳み方も同じ 1 本（move・branch は残す・main 不変）。
/// 残す event の段は **`Failed` のまま**＝retire は終端を動かさない。
#[test]
fn pipe_retire_rebase_empty_folds_failed_run_and_keeps_stage() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_b, landed) = gated_run_whose_change_is_already_on_main(&repo, &state, &marker);
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let empty = run_pipe(&[
        "land", "--run", &id_b, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(
        empty.status.code(),
        Some(i32::from(RC_REFUSED)),
        "2 本目は rebase-empty で終端する: {}",
        stderr_of(&empty)
    );
    assert!(show_line(&repo, &state, &id_b).contains("stage=Failed"), "終端の段は Failed");
    let live = worktree_of(&repo, &id_b);
    assert!(live.exists(), "終端した便の worktree は残る（retire の入口の前提）");

    let out = retire_once(&repo, &state, &id_b);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rebase-empty の便も畳める: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id_b);
    assert!(
        stdout_of(&out).contains(&format!("retired={}", retired.display())),
        "畳んだ先を名乗る: {}",
        stdout_of(&out)
    );
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    assert!(!live.exists(), "元の場所が空く");
    let branches = git(&repo, &["branch", "--list", &format!("scribe2/{id_b}")]);
    assert!(!branches.trim().is_empty(), "branch は消さない: {branches}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), landed, "main は 1 byte も動かない");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).expect("event log");
    let last = log.lines().rfind(|line| !line.is_empty()).unwrap_or_default();
    assert!(
        last.contains("\"stage\":\"Failed\"") && last.contains("\"detail\":\"retired\""),
        "最終行は Failed detail=retired（段を Landed へ動かさない）: {last}"
    );
    assert!(show_line(&repo, &state, &id_b).contains("stage=Failed"), "畳んだ後も段は Failed");
    clean(&[&repo, &state]);
}

/// 負例: `Failed` でも畳める理由（`rebase-empty` / `rebase-conflict`）でない便——ここでは
/// **main の実測が赤かった便**（`main-red`）——は畳まない（rc 1・worktree 不動・event 0 増）。
/// 木は clean のままなので、この rc 1 は **clean 検査ではなく終端の理由**を見ている
/// （「断ってから解いて通す」形は上の歯が担保する）。
#[test]
fn pipe_retire_rebase_empty_refuses_other_failed_reasons() {
    let (repo, state) = repo_with_state();
    // 1 回目（worktree）は緑・2 回目（main の実測）は赤になる verify 行＝`main-red` で終端する。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-once.sh"]"#]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let red = land_once(&repo, &state, &id);
    assert_eq!(red.status.code(), Some(i32::from(RC_REFUSED)), "main が赤い land は rc 1: {}", stderr_of(&red));
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "終端の段は Failed");
    let live = worktree_of(&repo, &id);
    assert!(git(&live, &["status", "--porcelain"]).is_empty(), "木は clean のまま");

    let before = event_count(&state);
    let out = retire_once(&repo, &state, &id);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_REFUSED)),
        "rebase-conflict の便は畳まない: {}",
        stdout_of(&out)
    );
    assert!(live.exists(), "断った周は worktree を動かさない");
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(!retired.exists(), "retired/<run> を作らない");
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_report_counts_landed_runs_not_landed_events() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    // 同じ便へ `Landed` の event をもう 1 件積む（手で積んだ / 台帳が壊れた周）。
    // **replay は便を数える**ので landed は 1 のまま——生の行を数える実装だと 2 になる。
    let doubled = Command::new(bin())
        .args(["fleet", "record", "--state-dir"])
        .arg(&state)
        .args(["--kind", "RunDone", "--stage", "Landed", "--run", &id, "--bead", "s2-2e5"])
        .output()
        .expect("binary を起動できる");
    assert_eq!(doubled.status.code(), Some(i32::from(RC_OK)), "record: {}", stderr_of(&doubled));
    let out = report_once(&state);
    assert_eq!(
        stdout_of(&out).trim(),
        "runs=1 landed=1 human_events=0 human_events_other_than_approval=0",
        "landed は便の数であって event の数ではない"
    );
    clean(&[&repo, &state]);
}

/// toy repo の 1 便を intake → spawn まで通す（bead を分けて id の衝突を避ける）。
fn toy_spawn(repo: &Path, state: &Path, bead: &str, contract: &Path, runner: &str) -> (String, Output) {
    let id = intake_bead(repo, state, contract, bead);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ]);
    (id, out)
}

/// commit を 1 本作る fake runner の 1 行。
const TOY_COMMIT: &str = "echo x >> src/lib.rs && git add -A && git commit -q -m runner";

/// **marker を tracked にする**。便の worktree は base の checkout なので、`.vessel` が
/// commit されていない repo では worktree に marker が無く `served()` は Absent＝guard は
/// 黙る。本 repo の root へ `.vessel` を置く理由がこれである（設計 §9・AC2）。
fn track_marker(repo: &Path) {
    git(repo, &["add", "-f", ".vessel"]);
    git(repo, &["commit", "-q", "-m", "vessel"]);
}

/// toy repo の 5 便が共有する材料（引数の本数を線の内へ収める）。
struct Toy<'a> {
    /// 対象 repo。
    repo: &'a Path,
    /// 置き場。
    state: &'a Path,
    /// PASS を返す fake lens。
    lens: &'a str,
}

/// 1 便を intake → spawn → gate(PASS) → land まで通す（正常形）。
fn toy_land(toy: &Toy<'_>, bead: &str, contract: &Path, runner: &str) {
    let (repo, state, lens) = (toy.repo, toy.state, toy.lens);
    let (id, spawned) = toy_spawn(repo, state, bead, contract, runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "{bead} spawn: {}", stderr_of(&spawned));
    let gated = gate_once(repo, state, &id, Some(lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "{bead} gate: {}", stderr_of(&gated));
    let landed = land_once(repo, state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "{bead} land: {}", stderr_of(&landed));
}

/// write-set の外を編集しようとして guard に止まる便。**止めたのが guard であることまで測る**
/// （runner が別の理由で落ちた周と弁別する）。
///
/// この経路は **misbehave した runner** のためのもので、五便（compliant な runner）では
/// 発火しない。ゆえに呼び手は `pipe_guard_is_backstop_for_misbehaving_runner` の 1 本だけ
/// である（`Toy` を受けないのは、使わない lens を組み立てさせないため）。
fn toy_denied(repo: &Path, state: &Path, contract: &Path) {
    let runner = format!(
        "printf '{{\"cwd\":\"%s\",\"tool_name\":\"Write\",\"tool_input\":{{\"file_path\":\"docs/out.md\"}}}}' \"$PWD\" \
         | '{}' hook pre-tool-use; test $? -eq 0 || exit 1; {TOY_COMMIT}",
        bin()
    );
    let (id, stopped) = toy_spawn(repo, state, "toy-guard", contract, &runner);
    // **spawn の rc は 0 のまま**（段が結果を運ぶ・設計 §5.2）。便の終わり方は段で読む。
    assert!(stdout_of(&stopped).contains("stage=Failed"), "guard に止まった便は Failed: {}", stdout_of(&stopped));
    assert!(
        show_line(repo, state, &id).contains("stage=Failed"),
        "永続面にも Failed が残る: {}",
        show_line(repo, state, &id)
    );
    let injected = fs::read_to_string(inject_path(state)).unwrap_or_default();
    let denies = injected.lines().filter(|line| line.contains("\"what\":\"deny\"")).count();
    assert_eq!(
        denies,
        1,
        "write-set の外への Write が 1 件 deny されている（母集団 {} 行）",
        injected.lines().count()
    );
}

/// **compliant な runner** の便②: 契約の goal が求める file が write-set の外にあるとき、
/// 実 runner は fence の外を書きに行かず「両立しない」と述べて空 commit を打つ
/// （実測・`s2-07l.24` の AC1 再走）。便は commit 1 本ゆえ `Implemented` まで進み、
/// **gate の verify で止まる**——guard は 1 件も発火しない（backstop であって関門ではない）。
fn toy_compliant_refusal(toy: &Toy<'_>, contract: &Path) {
    let (repo, state) = (toy.repo, toy.state);
    let runner = "git commit -q --allow-empty -m 'goal と write-set が両立しない'";
    let (id, spawned) = toy_spawn(repo, state, "toy-refuse", contract, runner);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));
    // **空 commit も commit 1 本**＝FR6 の完了判定（commit 0 は完了ではない）は通る。
    assert!(
        stdout_of(&spawned).contains("stage=Implemented"),
        "空 commit 1 本で Implemented: {}",
        stdout_of(&spawned)
    );

    // **便②専用の marker を持つ lens** を渡す（五便が共有する PASS lens の marker は
    // 便①の gate で既に作られており、「呼ばれなかった」を測れない）。
    let marker = state.join("lens-refusal");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(repo, state, &id, Some(&lens));
    assert_eq!(
        gated.status.code(),
        Some(i32::from(RC_REFUSED)),
        "verify が赤い便の gate は rc 1: {}",
        stderr_of(&gated)
    );
    let log = fs::read_to_string(state.join("pipe").join(&id).join("verify.jsonl")).unwrap_or_default();
    // 赤いのは**契約の**行である（n=1 の write-set 照合と n=2 の共通 verify は緑）。
    // 段が増えたので「1 行目」ではなく **cmd の字面**で当てる。
    let row = log
        .lines()
        .filter_map(|line| vessel::fleet::json_lite::parse_object(line.trim()).ok())
        .find(|found: &Vec<(String, vessel::fleet::json_lite::Value)>| {
            value_of(found, "cmd") == "sh verify-out.sh"
        })
        .unwrap_or_default();
    assert_eq!(value_of(&row, "n"), "3", "契約の行は共通 verify の後ろ: {log}");
    // **数値で測る**: `value_of` は key が無いと空文字を返すので、字面の `!= "0"` だと
    // `rc` が消えた・改名された退行まで真になってしまう（fail-open）。
    let rc: u64 = value_of(&row, "rc").parse().unwrap_or_default();
    assert!(
        rc > 0,
        "goal が求める docs/out.md は fence の外＝verify が赤い（rc={rc}）: {log}"
    );
    // 判定順どおり、verify が赤い周は lens を**呼ばない**（PASS を返す lens を渡しても
    // 便は通らない＝gate が lens の顔色で通す形になっていないことまで測る）。
    assert!(!marker.exists(), "verify が赤い周は lens を起動しない（marker 不在）");
    // guard は misbehave した runner のための backstop＝この経路では 1 件も発火しない。
    let injected = fs::read_to_string(inject_path(state)).unwrap_or_default();
    let denies = injected.lines().filter(|line| line.contains("\"what\":\"deny\"")).count();
    assert_eq!(
        denies,
        0,
        "compliant な runner は fence の外を書きに行かない（母集団 {} 行）",
        injected.lines().count()
    );

    let refused = land_once(repo, state, &id);
    assert_eq!(refused.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の便は land しない");
}

/// gate が FAIL する便（land しない）。
fn toy_gate_fail(toy: &Toy<'_>, contract: &Path, lens: &str) {
    let (repo, state) = (toy.repo, toy.state);
    let (id, spawned) = toy_spawn(repo, state, "toy-fail", contract, TOY_COMMIT);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));
    let gated = gate_once(repo, state, &id, Some(lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の gate は rc 1");
    let refused = land_once(repo, state, &id);
    assert_eq!(refused.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の便は land しない");
}

/// 3 クラスを名乗る便: spawn の手前で Blocked → approve（逐語）→ resume → gate → land。
fn toy_approved_land(toy: &Toy<'_>, contract: &Path) {
    let (repo, state, lens) = (toy.repo, toy.state, toy.lens);
    let (id, blocked) = toy_spawn(repo, state, "toy-approve", contract, TOY_COMMIT);
    assert_eq!(blocked.status.code(), Some(i32::from(RC_BLOCKED)), "承認待ちは rc 3");
    let approved = run_pipe(&[
        "approve", "--run", &id, "--words", "この便は出してよい",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(approved.status.code(), Some(i32::from(RC_OK)), "approve: {}", stderr_of(&approved));
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT,
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "resume: {}", stderr_of(&resumed));
    let gated = gate_once(repo, state, &id, Some(lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let landed = land_once(repo, state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
}

// flip-check: retroactive s2-07l.40
#[test]
fn pipe_five_contracts_land_with_fake_runner_in_toy_repo() {
    let (repo, state) = repo_with_state();
    let pass = fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"));
    let fail = fake_lens(&state.join("lens-fail"), &lens_verdict("FAIL"));
    let plain = write_contract(&repo, &[], &[]);
    // `Command::output` は stdin を /dev/null にする＝**人の入力を待つ余地が無い**形で
    // 5 便を通す（設計 §8 (e)）。
    track_marker(&repo);

    let toy = Toy { repo: &repo, state: &state, lens: &pass };
    toy_land(&toy, "toy-ok", &plain, TOY_COMMIT);
    // 便②: goal（`docs/out.md`）が write-set（`src/lib.rs`）の外にある契約。
    let refusal = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-out.sh"]"#]);
    toy_compliant_refusal(&toy, &refusal);
    let with_tests = write_contract(&repo, &["write-set"], &[r#"write-set = ["src/lib.rs", "tests/"]"#]);
    let add_test = "mkdir -p tests && echo '#[test] fn t() {}' > tests/new.rs \
                    && git add -A && git commit -q -m test";
    toy_land(&toy, "toy-test", &with_tests, add_test);
    toy_gate_fail(&toy, &plain, &fail);
    let publish = write_contract(&repo, &[], &[r#"classes = ["publish"]"#]);
    toy_approved_land(&toy, &publish);

    // **到達点**: 5 便のうち 3 便が main に載り、人由来の event は承認の 1 件だけ。
    let out = report_once(&state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "report: {}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out).trim(),
        "runs=5 landed=3 human_events=1 human_events_other_than_approval=0",
        "5 便の到達点（AC1 の形・人手は承認 1 件だけ）"
    );
    let exported = fs::read_to_string(land::verdicts_path(&state)).expect("面 5 を読める");
    assert_eq!(exported.lines().count(), 3, "main に載った便だけが面 5 に出る: {exported}");
    clean(&[&repo, &state]);
}

/// guard は **misbehave した runner のための backstop**。
///
/// 五便（compliant な runner）ではこの経路は発火しないので、極性はここで独立に測る
/// ——「発火しない」だけを測ると、guard が壊れて**常に**黙る退行が素通りする。
#[test]
fn pipe_guard_is_backstop_for_misbehaving_runner() {
    let (repo, state) = repo_with_state();
    track_marker(&repo);
    let contract = write_contract(&repo, &[], &[]);
    toy_denied(&repo, &state, &contract);
    clean(&[&repo, &state]);
}

/// `CHECKS` の並びが**宣言順**と一致する（ADR-0013 D2）。**この並びが適用順序である**ので、
/// 乖離は段の実行順が静かに変わることを意味する。
#[test]
fn gate_checks_follow_declaration_order() {
    assert!(
        is_declaration_order(CHECKS, |check| check as usize),
        "CHECKS の並びが宣言順と乖離している（母集団 {} 段）",
        CHECKS.len()
    );
}

/// `VERDICTS` の並びが**宣言順**と一致する（ADR-0013 D2）。
#[test]
fn gate_verdicts_follow_declaration_order() {
    assert!(
        is_declaration_order(VERDICTS, |verdict| verdict as usize),
        "VERDICTS の並びが宣言順と乖離している（母集団 {} 値）",
        VERDICTS.len()
    );
}

/// `--rules` で上限を差し替えて intake を通した周は、その事実が stdout に残る（`s2-07l.65`・
/// `.56` lens M1）。`--rules` は test の seam で、上限を無条件に差し替える——差し替えた周が
/// 通常の周と同じ 1 行しか出さないと、review は「埋め込みの上限で通った便」と区別できない。
/// 値は渡した path の字面そのもの（加工しない）。**差し替えていない周は出さない**（不在が既定・負例）。
#[test]
fn pipe_intake_names_ceiling_override_in_stdout() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let rules = ceiling_rules(&state);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let line = stdout_of(&out).lines().next().unwrap_or_default().to_owned();
    assert!(line.starts_with("run="), "既存 token が先頭のまま: {line}");
    assert!(
        line.split_whitespace().any(|token| token == format!("ceiling-overridden={rules}")),
        "差し替えた path が対で載る: {line}"
    );
    assert!(!run_id_of(&out).is_empty(), "run id は取れる: {line}");
    // 負例: `--rules` を渡さない（自己ホストの宣言 = 埋め込みの上限の内側）。
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo の root を解ける");
    let declared = fs::read_to_string(root.join(".vessel.toml")).expect("自己ホストの宣言を読める");
    let (plain, plain_state) = repo_with_state();
    fs::write(plain.join(".vessel.toml"), &declared).expect("宣言を写せる");
    git(&plain, &["add", "-f", ".vessel.toml"]);
    git(&plain, &["commit", "-q", "-m", "self-hosted"]);
    let contract = write_contract(&plain, &["verify"], &[r#"verify = ["git status"]"#]);
    let out = run_pipe(&[
        "intake", "--contract", &contract.display().to_string(), "--bead", "s2-2e5",
        "--repo", &plain.display().to_string(), "--state-dir", &plain_state.display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(!stdout_of(&out).contains("ceiling-overridden"), "差し替えていない周は出さない: {}", stdout_of(&out));
    clean(&[&repo, &state, &plain, &plain_state]);
}

/// gate の段①（write-set 照合）で diff を**読めない**周は「測れなかった」であって赤ではない
/// （`s2-07l.65`・`.57` 申し送り・land の `MainCheck::Unmeasurable` と同じ極性）。
///
/// 読めない状態は **base commit の tree object を消して**作る（実測: `git status` と
/// `git rev-list --count` は通り、`git diff --name-only -z <base>..HEAD` だけが `unable to read tree`
/// で落ちる＝precheck を抜けて段①に届く）。verdict は INCONCLUSIVE（既存 3 値の内側・rc 3）で、
/// lens は呼ばれず（判定に届いていない）、verify.jsonl の段①の行は残る（現物を消さない）。
#[test]
fn pipe_gate_turns_unreadable_diff_into_inconclusive() {
    let (repo, state) = repo_with_state();
    // 契約行に**赤い行を 1 本**混ぜる: 「測れなかったは赤より先」（判定順）と「段①の -1 は赤に
    // 数えないが ②③ の赤は数える」（`verify_red` = 1）を同じ便で測る。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-ok.sh", "sh verify-red.sh"]"#]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let id = implemented(&repo, &state, &path);
    let tree = git(&repo, &["rev-parse", &format!("{base}^{{tree}}")]);
    let (head, tail) = tree.split_at(2);
    let object = repo.join(".git").join("objects").join(head).join(tail);
    fs::remove_file(&object).expect("base の tree object を消せる");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(3), "測れなかった周は INCONCLUSIVE の rc: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&out));
    let pairs = verdict_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "赤い契約行が在っても、測れなかったが先");
    assert_eq!(value_of(&pairs, "verify_red"), "1", "段①の -1 は赤に数えず ②③ の赤（1 本）だけを数える");
    assert!(value_of(&pairs, "evidence").contains("読めない"), "理由が残る: {}", value_of(&pairs, "evidence"));
    assert!(!marker.exists(), "判定に届いていないので lens は呼ばない");
    let log = fs::read_to_string(vessel::pipe::verify_log_path(&state, &id)).expect("verify.jsonl を読める");
    let first = log.lines().next().unwrap_or_default();
    assert!(first.contains("\"n\":1") && first.contains("\"cmd\":\"write-set\""), "段①の record は残る: {first}");
    assert!(first.contains("\"rc\":255"), "段①の rc -1 は u64 の記録形 255 で残る（schema 不変）: {first}");
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "段は Gated へ進む（測り直せる）");
    clean(&[&repo, &state]);
}

// ── 質問の口 (a)（設計 docs/design/pipeline-question.md §8 (a)・SRS FR31 / FR32） ───────

/// runner が最終行に書く質問 record。
const QUESTION_RECORD: &str = r#"{"question":"verify 行が矛盾する","about":"verify"}"#;

/// record を stdout の最終行に書いて rc 76 で終える fake runner（commit は作らない）。
fn question_runner() -> String {
    format!("echo before; printf '%s\\n' '{QUESTION_RECORD}'; exit 76")
}

/// intake → spawn で質問に倒した便の id を返す。
fn questioned(repo: &Path, state: &Path) -> String {
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
fn trail(state: &Path, id: &str) -> Vec<(EventKind, Option<Stage>, Option<String>)> {
    events(state)
        .into_iter()
        .filter(|event| event.run == id)
        .map(|event| (event.kind, event.stage, event.detail))
        .collect()
}

#[test]
fn pipe_question_spawn_records_questioned_in_order() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let tail: Vec<_> = trail(&state, &id).into_iter().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();
    assert_eq!(
        tail,
        vec![
            (EventKind::SeatStopped, None, None),
            (EventKind::QuestionRaised, None, Some("verify 行が矛盾する".to_owned())),
            (EventKind::RunStage, Some(Stage::Questioned), Some("about:verify".to_owned())),
        ],
        "SeatStopped → QuestionRaised(逐語) → RunStage(Questioned) の順"
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Questioned"), "永続面に Questioned が残る");
    // 質問で止まった便に Live 席は無い（`pipe stop --all` の母集団に入らない）。
    let stopped = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert!(stdout_of(&stopped).contains("seats=0"), "{}", stdout_of(&stopped));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_rc76_without_record_fails_closed() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    // record が無い / 壊れた JSON / question が空 / 非文字列 / 複数行 / key 無し、の各形。
    for (bead, last_line, reason) in [
        ("s2-none", "not a record", "JSON 行が無い"),
        ("s2-broken", r#"{"question":"verify"#, "読めない"),
        ("s2-empty", r#"{"question":"  "}"#, "無いか空"),
        ("s2-num", r#"{"question":1}"#, "無いか空"),
        ("s2-multi", r#"{"question":"a\nb"}"#, "1 行でない"),
        ("s2-nokey", r#"{"about":"verify"}"#, "無いか空"),
    ] {
        let id = intake_bead(&repo, &state, &path, bead);
        let runner = format!("printf '%s\\n' '{last_line}'; exit 76");
        let out = run_pipe(&[
            "spawn", "--run", &id, "--repo", &repo.display().to_string(),
            "--state-dir", &state.display().to_string(), "--runner", &runner,
        ]);
        assert!(stdout_of(&out).contains("stage=Failed"), "{bead}: {}", stdout_of(&out));
        let last = trail(&state, &id).pop();
        assert!(
            matches!(&last, Some((EventKind::RunStage, Some(Stage::Failed), Some(detail)))
                if detail.starts_with("question-record-missing:") && detail.contains(reason) && detail.ends_with(",commits:0")),
            "{bead}: 理由 question-record-missing:{reason}: {last:?}"
        );
        assert!(
            !trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised),
            "{bead}: 質問は記帳しない"
        );
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_runner_stdout_is_kept_in_run_dir() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // stdout を捕らえても、包みの観測行（rate-limit status の集合を育てる口）は残る。
    let runner = "echo 'runner: rc=0 records=3 observed=allowed_warning'; echo x >> src/lib.rs && git add -A && git commit -q -m runner";
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let kept = fs::read_to_string(state.join("pipe").join(&id).join("runner.stdout.log")).unwrap_or_default();
    assert!(kept.contains("observed=allowed_warning"), "観測行が残る: {kept}");
    assert!(kept.lines().next().is_some_and(|head| head.starts_with("## ") && head.ends_with(" rc=0")), "見出し行: {kept}");
    // stdout を出さない runner では file を作らない。**測り終えた便は `stop --run` で外す**
    // ——`Implemented` は終端でないので、同じ write-set の 2 本目は交差で断られる（`s2-07l.145`）。
    //
    // **包めない host で撃つ**（[`lean_path`]）。封じ込めが効く host では包みが終端行
    // （`confine-usage …`）を出すので stdout は空にならず、この面は host ごとに違う答えを
    // 出してしまう——測っているのは「**runner が**何も言わなかった周」である。
    stop_run_ok(&state, &id);
    let id2 = intake_bead(&repo, &state, &path, "s2-quiet");
    let quiet = run_pipe_with_path(
        &lean_path(&state),
        &["spawn", "--run", &id2, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", "true"],
    );
    assert!(stdout_of(&quiet).contains("stage=Failed"));
    assert!(!state.join("pipe").join(&id2).join("runner.stdout.log").exists(), "空の周は書かない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_rc0_does_not_read_record_line() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // 76 でない rc では最終行を読まない＝record を書いても従来どおり Implemented。
    let runner = format!(
        "echo x >> src/lib.rs && git add -A && git commit -q -m runner; printf '%s\\n' '{QUESTION_RECORD}'; exit 0"
    );
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(stdout_of(&out).contains("stage=Implemented"), "{}", stdout_of(&out));
    assert!(!trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised));
    // record と commit が同時の周は質問ではなく実装の失敗（rc 76 でも Failed）。
    // 測り終えた 1 本目は `stop --run` で外す（入口の排他・`s2-07l.145`）。
    stop_run_ok(&state, &id);
    let path2 = write_contract(&repo, &[], &[]);
    let id2 = intake_bead(&repo, &state, &path2, "s2-both");
    let both = format!("echo y >> src/lib.rs && git add -A && git commit -q -m r; printf '%s\\n' '{QUESTION_RECORD}'; exit 76");
    let out = run_pipe(&[
        "spawn", "--run", &id2, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &both,
    ]);
    assert!(stdout_of(&out).contains("stage=Failed"), "{}", stdout_of(&out));
    assert!(matches!(trail(&state, &id2).pop(), Some((_, Some(Stage::Failed), Some(d))) if d.starts_with("runner-rc:76,commits:1")));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_resume_waits_for_answer_without_writing() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let marker = state.join("runner-ran");
    let before = event_count(&state);
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner_cmd(&marker),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "回答の無い resume は rc 3: {}", stderr_of(&out));
    assert!(!marker.exists(), "runner を起こさない");
    assert_eq!(event_count(&state), before, "1 行も書かない");
    // 空の回答は書かない（rc 1）。
    let empty = run_pipe(&["answer", "--run", &id, "--words", "  ", "--state-dir", &state.display().to_string()]);
    assert_eq!(empty.status.code(), Some(i32::from(RC_REFUSED)), "{}", stderr_of(&empty));
    assert_eq!(event_count(&state), before, "空の回答は 1 byte も書かない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_answer_refuses_run_that_is_not_questioned() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let before = event_count(&state);
    let out = run_pipe(&["answer", "--run", &id, "--words", "verify は 1 行目だけ", "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "Questioned 以外は rc 3: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before, "何も書かない");
    let missing = run_pipe(&["answer", "--run", "nope", "--words", "x", "--state-dir", &state.display().to_string()]);
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "無い run は rc 1");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_answer_then_resume_respawns_with_answer_section() {
    let (repo, state) = repo_with_state();
    let id = questioned(&repo, &state);
    let answered = run_pipe(&[
        "answer", "--run", &id, "--words", "verify は 1 行目だけを撃つ",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&answered));
    assert!(stdout_of(&answered).contains("answered=true"), "{}", stdout_of(&answered));
    let copied = state.join("got-stdin.txt");
    let runner = format!(
        "cat > '{}' && echo x >> src/lib.rs && git add -A && git commit -q -m runner",
        copied.display()
    );
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "回答の後は同じ便が進む: {}", stderr_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "{}", show_line(&repo, &state, &id));
    let stdin = fs::read_to_string(&copied).unwrap_or_default();
    assert!(stdin.contains("## 回答"), "stdin に回答節: {stdin}");
    assert!(stdin.contains("verify 行が矛盾する") && stdin.contains("verify は 1 行目だけを撃つ"), "質問と回答の逐語: {stdin}");
    assert!(stdin.contains("goal = "), "契約の本文も流す: {stdin}");
    // 同じ run が Spawned を通り直し、base は初回の記録と同じ。
    let stages: Vec<Option<Stage>> = trail(&state, &id).into_iter().map(|(_, stage, _)| stage).collect();
    assert_eq!(stages.iter().filter(|stage| **stage == Some(Stage::Spawned)).count(), 2, "Spawned を 2 回通る");
    let bases: BTreeSet<String> = trail(&state, &id)
        .into_iter()
        .filter_map(|(_, stage, detail)| (stage == Some(Stage::Spawned)).then_some(detail).flatten())
        .collect();
    assert_eq!(bases.len(), 1, "base は 1 つ: {bases:?}");
    // 回答は machine 由来（FR22 不変）。
    let report = report_once(&state);
    assert!(
        stdout_of(&report).contains("human_events=0 human_events_other_than_approval=0"),
        "{}",
        stdout_of(&report)
    );
    assert!(
        trail(&state, &id).iter().any(|(kind, _, detail)| *kind == EventKind::QuestionAnswered && detail.as_deref() == Some("verify は 1 行目だけを撃つ")),
        "回答の逐語が残る"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_question_run_stops_with_question_token() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let rules = ceiling_rules(&state);
    let out = run_pipe(&[
        "run", "--contract", &path.display().to_string(), "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules, "--runner", &question_runner(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "{}", stderr_of(&out));
    let id = run_id_of(&out);
    assert!(stdout_of(&out).contains(&format!("question={id}")), "判定行に question=: {}", stdout_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Questioned"));
    clean(&[&repo, &state]);
}

// ─────────────────── land の後の anchor 同期（`s2-07l.120`・N1・接頭辞 `pipe_land_anchor_`） ───────────────────

/// land（squash 形）の後、anchor（`--repo`）の HEAD が main を指す checkout なら **index と working tree を
/// 新 main に揃える**（`.117` 実測: base は `git update-ref` だけで index が旧のまま＝`git status` に
/// `M  src/lib.rs` が残り、次の `commit -a` で landed 変更が消える経路・N1）。判定行に `anchor=synced`。
#[test]
fn pipe_land_anchor_syncs_index_and_working_tree_to_new_main() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    assert_eq!(git(&repo, &["status", "--porcelain", "--untracked-files=no"]).trim(), "", "land の前の anchor は clean");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert!(stdout_of(&out).contains(&format!("landed={new} anchor=synced")), "判定行に anchor=synced: {}", stdout_of(&out));
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), new, "anchor の HEAD は新 main");
    assert_eq!(
        git(&repo, &["status", "--porcelain", "--untracked-files=no"]).trim(),
        "",
        "index と working tree が新 main に揃う（M が残らない）"
    );
    let lib = fs::read_to_string(repo.join("src").join("lib.rs")).unwrap_or_default();
    assert!(lib.lines().any(|line| line == "x"), "landed 変更が anchor の working tree に在る: {lib:?}");
    clean(&[&repo, &state]);
}

/// anchor に**未 commit の変更**が在る周は触らない（成果を消さない・fail-closed）: main の ref は進めるが
/// index / working tree は揃えず、判定行に `anchor=skipped:dirty` と stderr の warning 1 行。局所の変更は
/// そのまま残る。
#[test]
fn pipe_land_anchor_skips_dirty_anchor_and_keeps_local_change() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    // 便の変更と同じ file に、commit していない局所の変更を置く（揃えると消える形）。
    fs::write(repo.join("src").join("lib.rs"), "// local uncommitted\n").expect("局所の変更を置ける");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land 自体は成立（rc 0）: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert!(stdout_of(&out).contains(&format!("landed={new} anchor=skipped:dirty")), "{}", stdout_of(&out));
    assert!(stderr_of(&out).contains("anchor"), "warning の 1 行を stderr に出す: {}", stderr_of(&out));
    assert_eq!(
        fs::read_to_string(repo.join("src").join("lib.rs")).unwrap_or_default(),
        "// local uncommitted\n",
        "未 commit の変更を消さない"
    );
    clean(&[&repo, &state]);
}

/// anchor の HEAD が main を指さない周（別 branch・detached）は触らない: `anchor=skipped:not-main`。
/// 他 branch の HEAD と working tree は不変。
#[test]
fn pipe_land_anchor_skips_when_head_is_not_main() {
    for (label, args) in [("other-branch", vec!["checkout", "-q", "-b", "other"]), ("detached", vec!["checkout", "-q", "--detach"])] {
        let (repo, state) = repo_with_state();
        let path = write_contract(&repo, &[], &[]);
        let marker = state.join("lens-ran");
        let id = gated_pass(&repo, &state, &path, &marker);
        let before = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &args);
        let out = land_once(&repo, &state, &id);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{label}: land は rc 0: {}", stderr_of(&out));
        let new = git(&repo, &["rev-parse", "refs/heads/main"]);
        assert_ne!(new, before, "{label}: main は進む");
        assert!(stdout_of(&out).contains(&format!("landed={new} anchor=skipped:not-main")), "{label}: {}", stdout_of(&out));
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), before, "{label}: anchor の HEAD は動かない");
        assert_eq!(git(&repo, &["status", "--porcelain", "--untracked-files=no"]).trim(), "", "{label}: working tree は不変で clean");
        clean(&[&repo, &state]);
    }
}

/// main の実測が**赤**でも ref は進んでいるので anchor は揃える（揃えないと failure exit で `.117` の
/// 経路が開く・lens-120 H1）。rc 1 のまま stderr に `anchor=synced` を足し、anchor は clean・HEAD == 新 main。
#[test]
fn pipe_land_anchor_syncs_even_when_main_verify_is_red() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-once.sh"]"#]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "main が赤ければ rc 1");
    assert!(stderr_of(&out).contains("main が赤い"), "理由: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains("pipe: anchor=synced"), "赤でも anchor は揃える（token を stderr に）: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), new, "anchor の HEAD は新 main");
    assert_eq!(git(&repo, &["status", "--porcelain", "--untracked-files=no"]).trim(), "", "赤でも staged の逆向きを残さない");
    clean(&[&repo, &state]);
}

/// landed tree が**足す** path が anchor に untracked（ここでは **ignored**）で在る周は触らない: `read-tree -m -u` は
/// ignored な file を黙って上書きする（実測・lens-120 M1）ので、足す path の衝突を先に見て
/// `anchor=skipped:collision`。局所の file は不変・main は進む。
#[test]
fn pipe_land_anchor_skips_when_landed_tree_adds_a_path_that_exists_ignored_in_anchor() {
    let (repo, state) = repo_with_state();
    let path = write_contract(
        &repo,
        &["write-set"],
        &[r#"write-set = ["src/lib.rs", "src/new.rs"]"#],
    );
    let marker = state.join("lens-ran");
    let id = intake(&repo, &state, &path);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "echo n > src/new.rs && echo x >> src/lib.rs && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS の gate は rc 0: {}", stderr_of(&out));
    // anchor に ignored な同名 file を置く（`.gitignore` は untracked でも効く・tracked 変更ではない）。
    fs::write(repo.join(".gitignore"), "src/new.rs\n").expect(".gitignore を置ける");
    fs::write(repo.join("src").join("new.rs"), "// local ignored\n").expect("ignored な file を置ける");
    assert_eq!(git(&repo, &["status", "--porcelain", "--untracked-files=no"]).trim(), "", "tracked 変更は無い");

    let out = land_once(&repo, &state, &id);

    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains(" anchor=skipped:collision"), "{}", stdout_of(&out));
    assert!(stderr_of(&out).contains("collision"), "warning: {}", stderr_of(&out));
    assert_eq!(
        fs::read_to_string(repo.join("src").join("new.rs")).unwrap_or_default(),
        "// local ignored\n",
        "ignored な file を上書きしない"
    );
    assert_eq!(
        fs::read_to_string(repo.join("src").join("lib.rs")).unwrap_or_default(),
        "// seed\n",
        "衝突の周は 1 file も触らない（lib.rs も旧のまま）"
    );
    clean(&[&repo, &state]);
}

/// anchor の状態を**読めない**周（index が壊れている＝`git status` が fatal）は clean に読み替えず
/// `anchor=skipped:unreadable`（fail-closed・lens-120 M3）。main は進む。
#[test]
fn pipe_land_anchor_skips_when_status_is_unreadable() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    fs::write(repo.join(".git").join("index"), b"garbage").expect("index を壊せる");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains(" anchor=skipped:unreadable"), "{}", stdout_of(&out));
    assert!(stderr_of(&out).contains("unreadable"), "warning: {}", stderr_of(&out));
    assert_ne!(git(&repo, &["rev-parse", "refs/heads/main"]), base, "main は進む");
    assert_eq!(
        fs::read_to_string(repo.join("src").join("lib.rs")).unwrap_or_default(),
        "// seed\n",
        "読めない周は working tree に触らない"
    );
    clean(&[&repo, &state]);
}

/// 見立ては Sync でも git が**途中で**断った周（`index.lock` が在る）は `anchor=skipped:sync-failed` で、
/// warning は「部分的に更新されている可能性」を名指す（状態を「旧のまま」と断定しない・lens-120 M2 / M3）。
#[test]
fn pipe_land_anchor_reports_sync_failed_when_git_refuses_midway() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    fs::write(repo.join(".git").join("index.lock"), b"").expect("index.lock を置ける");
    let out = land_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains(" anchor=skipped:sync-failed"), "{}", stdout_of(&out));
    assert!(stderr_of(&out).contains("部分的に更新されている可能性"), "warning は状態を断定しない: {}", stderr_of(&out));
    fs::remove_file(repo.join(".git").join("index.lock")).ok();
    clean(&[&repo, &state]);
}

/// 同期は **squash の直後・main 実測の前**に走る（`s2-07l.131`・窓を秒単位へ）。
///
/// 「揃えた」だけでは順序を測れない（実測の後に揃えても最後は clean になる）ので、**実測の最中に
/// anchor がどう見えるか**を現物で採る: 契約の verify が main 実測の tmp worktree（detached）で
/// 撃たれたときだけ、置き場の record file へ印 1 行と anchor の `git status --porcelain` を落とす。
/// base（実測 → 同期）では index が旧 main のままなので `M  src/lib.rs`（staged の逆向き・`.117` の形）が
/// 記録され、head（同期 → 実測）では空になる。
///
/// **record の不在を clean に読み替えない**（lens FAIL 2026-09-12）: file が在ること・先頭行が印である
/// ことを先に要求し、その後の porcelain 部分だけを空と照合する。script は `set -e` で書き、status を
/// 読めない周は verify が赤くなって land が `main-red` で終端する（この歯はそれも落とす）。
#[test]
fn pipe_land_anchor_before_verify_records_clean_anchor_during_main_check() {
    let (repo, state) = repo_with_state();
    let record = state.join("anchor-status");
    // `--untracked-files=no`: anchor には便の `.worktrees/` が常に untracked で在る（器が作る入れ物で
    // あって「揃っていない」の合図ではない）＝器の見立て（`anchor_plan`）と同じ面を読む。
    let script = format!(
        "set -e\n\
         if [ \"$(git rev-parse --abbrev-ref HEAD)\" = HEAD ]; then\n\
         printf 'anchor-observed\\n' > '{record}'\n\
         git -C '{repo}' status --porcelain --untracked-files=no >> '{record}'\n\
         fi\n",
        record = record.display(),
        repo = repo.display(),
    );
    fs::write(repo.join("verify-probe.sh"), script).expect("probe script を書ける");
    git(&repo, &["add", "--", "verify-probe.sh"]);
    git(&repo, &["commit", "-q", "-m", "probe"]);
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-probe.sh"]"#]);
    let marker = state.join("lens-ran");
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = gated_pass(&repo, &state, &path, &marker);
    assert!(!record.exists(), "gate（branch の worktree）は record を書かない＝印は実測の周のもの");

    let out = land_once(&repo, &state, &id);

    // 5. 既存の挙動は不変（Landed・`anchor=synced`・main は新 sha）。
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(new, base, "main は進む");
    assert!(stdout_of(&out).contains(&format!("landed={new} anchor=synced")), "判定行: {}", stdout_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "{}", show_line(&repo, &state, &id));
    // 1. record が**在る**（無いを clean に化けさせない）。
    let text = fs::read_to_string(&record).expect("main 実測の verify が record を書いている");
    let mut lines = text.lines();
    // 2. 先頭行は固定の印＝script が本当に走って書いた証拠。
    assert_eq!(lines.next(), Some("anchor-observed"), "印が先頭行: {text:?}");
    // 3. 印の後の porcelain は空＝実測の**最中に** anchor が既に新 main へ揃っている
    //    （base ではここが `M  src/lib.rs`）。
    assert_eq!(
        lines.collect::<Vec<&str>>(),
        Vec::<&str>::new(),
        "実測の最中の anchor は揃っている（同期が先）: {text:?}"
    );
    clean(&[&repo, &state]);
}

// ────────── 入口の write-set 排他と `stop --run`（設計 pipeline-conflict.md §2・ADR-0019 §2.1） ──────────

/// event log の **byte 列**（行数では追記の中身の差が消えるので byte で比べる）。
fn events_bytes(state: &Path) -> Vec<u8> {
    fs::read(state.join("fleet").join("events.jsonl")).unwrap_or_default()
}

/// 置き場に在る run dir の名（「run を作らない」を数で測る）。
fn run_dirs(state: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(state.join("pipe")) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .filter_map(|entry| entry.ok().map(|found| found.file_name().to_string_lossy().into_owned()))
        .collect();
    found.sort();
    found
}

/// write-set だけを差し替えた契約 file を名前つきで書く（1 便 1 file＝写しの取り違えを作らない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_set_contract(dir: &Path, name: &str, entries: &[&str]) -> PathBuf {
    let quoted: Vec<String> = entries.iter().map(|item| format!("\"{item}\"")).collect();
    let mut lines: Vec<String> = contract_body()
        .into_iter()
        .filter(|line| !line.starts_with("write-set"))
        .collect();
    lines.push(format!("write-set = [{}]", quoted.join(", ")));
    let path = dir.join(name);
    fs::write(&path, format!("{}\n", lines.join("\n"))).expect("契約 file を書ける");
    path
}

/// intake を 1 回撃つ（**rc を測らない**＝断られる周の歯が使う）。
fn try_intake(repo: &Path, state: &Path, contract: &Path, bead: &str) -> Output {
    run_pipe(&[
        "intake", "--contract", &contract.display().to_string(), "--bead", bead,
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(state),
    ])
}

/// fixture の event で run の段を動かす。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn record_stage(state: &Path, id: &str, stage: &str) {
    let out = Command::new(bin())
        .args(["fleet", "record", "--kind", "RunStage", "--stage", stage, "--run", id,
               "--bead", "s2-live", "--state-dir"])
        .arg(state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fleet record: {}", stderr_of(&out));
}

/// fixture の `verdict.json`（gate の判定を手で置く）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_verdict(state: &Path, id: &str, verdict: &str) {
    let path = state.join("pipe").join(id).join("verdict.json");
    fs::write(&path, format!("{{\"schema\":1,\"run\":\"{id}\",\"verdict\":\"{verdict}\"}}\n"))
        .expect("verdict.json を書ける");
}

/// live な便（intake だけ通した段 `Intake`）と **write-set が交差する 2 本目**は受け付けない
/// （ADR-0019 §2.1）。断った周は run dir も event も作らず、stderr が 1 本目の run id と
/// 交差した path を名乗る。**base はこの 2 本目を受理する**（run dir が 2 つできる）。
#[test]
fn pipe_refuse_intake_refuses_a_contract_that_overlaps_a_live_run() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first.toml", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-live");
    let before = events_bytes(&state);
    let dirs = run_dirs(&state);
    let second = write_set_contract(&repo, "second.toml", &["src/lib.rs"]);
    let out = try_intake(&repo, &state, &second, "s2-next");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "交差は rc 1: {}", stdout_of(&out));
    let err = stderr_of(&out);
    assert!(err.contains(&id), "1 本目の run id を名乗る: {err}");
    assert!(err.contains("src/lib.rs"), "交差した path を名乗る: {err}");
    assert!(out.stdout.is_empty(), "断った周は stdout に 1 byte も書かない");
    assert_eq!(run_dirs(&state), dirs, "run dir を作らない（母集団 {} 本）", dirs.len());
    assert_eq!(events_bytes(&state), before, "events.jsonl は byte 不変");
    clean(&[&repo, &state]);
}

/// dir と file の交差の表（設計 §2）を **intake の受理 / 拒否**で測る。正規化は write-set
/// guard と同じ規則（先頭の `./`・連続する `/`・`..` の畳み）で、dir `a/` は `a/…` を含み
/// `ab/` は含まない。
#[test]
fn pipe_refuse_intake_measures_dir_and_file_overlap() {
    for (live_entry, next_entry, refused) in [
        ("a/", "a/b.rs", true),
        ("a/", "ab/", false),
        ("a", "a/", true),
        ("./a/b.rs", "a/b.rs", true),
        ("a//b.rs", "a/b.rs", true),
        ("src/../src/x.rs", "src/x.rs", true),
    ] {
        let (repo, state) = repo_with_state();
        let first = write_set_contract(&repo, "first.toml", &[live_entry]);
        intake_bead(&repo, &state, &first, "s2-live");
        let second = write_set_contract(&repo, "second.toml", &[next_entry]);
        let out = try_intake(&repo, &state, &second, "s2-next");
        let want = if refused { RC_REFUSED } else { RC_OK };
        assert_eq!(
            out.status.code(),
            Some(i32::from(want)),
            "{live_entry} × {next_entry} は交差={refused}: {}",
            stderr_of(&out)
        );
        clean(&[&repo, &state]);
    }
}

/// **終端した便とは交差しない**（段が `Landed` / `Failed` / `Stopped`・`Gated` で verdict が
/// FAIL）。`Gated` の PASS / INCONCLUSIVE は終端でないので交差する（pipeline.md §4「FAIL は終端」）。
/// 契約を改訂して流し直す経路（本番 `.129` / `.131` の型）を塞がないことを測る。
#[test]
fn pipe_refuse_intake_ignores_terminal_runs() {
    for (stage, verdict, refused) in [
        ("Landed", None, false),
        ("Failed", None, false),
        ("Stopped", None, false),
        ("Gated", Some("FAIL"), false),
        ("Gated", Some("PASS"), true),
        ("Gated", Some("INCONCLUSIVE"), true),
    ] {
        let (repo, state) = repo_with_state();
        let first = write_set_contract(&repo, "first.toml", &["src/lib.rs"]);
        let id = intake_bead(&repo, &state, &first, "s2-live");
        if let Some(found) = verdict {
            write_verdict(&state, &id, found);
        }
        record_stage(&state, &id, stage);
        let second = write_set_contract(&repo, "second.toml", &["src/lib.rs"]);
        let out = try_intake(&repo, &state, &second, "s2-next");
        let want = if refused { RC_REFUSED } else { RC_OK };
        assert_eq!(
            out.status.code(),
            Some(i32::from(want)),
            "段 {stage} verdict {verdict:?} は交差={refused}: {}",
            stderr_of(&out)
        );
        clean(&[&repo, &state]);
    }
}

/// live な便の契約の写しを読めない周は **rc 2**（壊れた store・NFR4）で、run dir も event も
/// 作らない。`Gated` の判定を読めない周も同じ「読めない」側である（fail-closed＝読めなさを
/// 「交差なし」に読み替えない）。
#[test]
fn pipe_refuse_intake_is_broken_when_a_live_copy_is_unreadable() {
    for damage in ["remove", "garble", "verdict"] {
        let (repo, state) = repo_with_state();
        let first = write_set_contract(&repo, "first.toml", &["src/lib.rs"]);
        let id = intake_bead(&repo, &state, &first, "s2-live");
        let copied = state.join("pipe").join(&id).join("contract.toml");
        match damage {
            "remove" => {
                fs::remove_file(&copied).ok();
            }
            "garble" => {
                fs::write(&copied, "こわれ\n").ok();
            }
            // 段は Gated だが verdict.json が無い＝終端かを測れない。
            _ => record_stage(&state, &id, "Gated"),
        }
        let before = events_bytes(&state);
        let dirs = run_dirs(&state);
        let second = write_set_contract(&repo, "second.toml", &["src/lib.rs"]);
        let out = try_intake(&repo, &state, &second, "s2-next");
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{damage}: 読めない周は rc 2");
        let err = stderr_of(&out);
        assert!(err.contains(&id), "{damage}: 読めない run を名指す: {err}");
        assert!(err.contains("読めない"), "{damage}: 理由は読めないこと: {err}");
        assert_eq!(run_dirs(&state), dirs, "{damage}: run dir を作らない");
        assert_eq!(events_bytes(&state), before, "{damage}: events.jsonl は byte 不変");
        clean(&[&repo, &state]);
    }
}

/// **同じ bead の 2 本目も特別扱いしない**: write-set が同じなら交差で断られる（owner が同じ
/// ことに意味を持たせない＝自然に掛かる）。
#[test]
fn pipe_refuse_intake_refuses_the_second_run_of_the_same_bead() {
    let (repo, state) = repo_with_state();
    let path = write_set_contract(&repo, "first.toml", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &path, "s2-same");
    let out = try_intake(&repo, &state, &path, "s2-same");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "同 bead の 2 本目も断る");
    let err = stderr_of(&out);
    assert!(err.contains("交差"), "断る理由は id の衝突でなく交差: {err}");
    assert!(err.contains(&id), "交差した相手を名乗る: {err}");
    clean(&[&repo, &state]);
}

/// 交差が 2 組以上の周は **stderr に全組が 1 組 1 行**で並び、理由の 1 行は先頭の 1 組を名乗る。
#[test]
fn pipe_refuse_intake_lists_every_overlapping_pair() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first.toml", &["src/a.rs", "src/b.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-live");
    let second = write_set_contract(&repo, "second.toml", &["src/a.rs", "src/b.rs"]);
    let out = try_intake(&repo, &state, &second, "s2-next");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "交差は rc 1");
    let err = stderr_of(&out);
    let pairs: Vec<&str> = err.lines().filter(|line| line.contains("overlap ")).collect();
    assert_eq!(pairs.len(), 2, "交差した全組が並ぶ（母集団 {} 行）: {err}", err.lines().count());
    for entry in ["src/a.rs", "src/b.rs"] {
        assert!(
            pairs.iter().any(|line| line.contains(entry) && line.contains(&id)),
            "{entry} の組が run id つきで並ぶ: {err}"
        );
    }
    let head = err.lines().next().unwrap_or_default();
    assert!(head.contains("src/a.rs"), "理由の 1 行は先頭の 1 組: {head}");
    assert!(!head.contains("src/b.rs"), "理由の 1 行は 1 組だけ: {head}");
    clean(&[&repo, &state]);
}

/// `pipe stop --run <id>`: 終端でない便 1 本に `RunStopped` を **1 件だけ**書き、その後は同じ
/// write-set の契約が通る。終端した便には何も書かず rc 1（書込は冪等・rc は冪等でない）。
#[test]
fn pipe_refuse_stop_run_releases_the_write_set_of_a_live_run() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first.toml", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-live");
    let second = write_set_contract(&repo, "second.toml", &["src/lib.rs"]);
    let blocked = try_intake(&repo, &state, &second, "s2-next");
    assert_eq!(blocked.status.code(), Some(i32::from(RC_REFUSED)), "止める前は交差で断られる");
    let before = event_count(&state);
    let out = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "非終端の便は止まる: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before + 1, "RunStopped を 1 件だけ書く");
    let last = events(&state).into_iter().rfind(|found| found.run == id);
    assert!(
        matches!(&last, Some(found) if found.kind == EventKind::RunStopped && found.stage == Some(Stage::Stopped)),
        "書くのは RunStopped stage=Stopped: {last:?}"
    );
    // 2 回撃っても 2 件目を書かない（終端した便は rc 1）。
    let again = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "終端の便は rc 1");
    assert_eq!(event_count(&state), before + 1, "2 件目を書かない");
    // 外れた便とは交差しない＝同じ write-set の契約が通る。
    let passed = try_intake(&repo, &state, &second, "s2-third");
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "止めた後は通る: {}", stderr_of(&passed));
    // 無い便は rc 1 で何も書かない。
    let missing = run_pipe(&["stop", "--run", "no-such-run", "--state-dir", &state.display().to_string()]);
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "無い便は rc 1");
    clean(&[&repo, &state]);
}

/// `stop --run` は便の **Live 席も止める**（`--all` と同じ関数を通る）。席を持つ便を外す口が
/// 席を残すと、止めたはずの便の runner が走り続ける。
#[test]
fn pipe_refuse_stop_run_stops_the_live_seat_of_the_run() {
    let (repo, state) = repo_with_state();
    let path = write_set_contract(&repo, "first.toml", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &path, "s2-live");
    // **孫**として起こす（test process の子のままだと zombie が /proc に残る）。
    let spawned = Command::new("sh")
        .arg("-c")
        .arg("sleep 60 >/dev/null 2>&1 & echo $!")
        .output()
        .expect("fake runner を起こせる");
    let pid: u32 = String::from_utf8_lossy(&spawned.stdout).trim().parse().expect("pid を読める");
    let record = Command::new(bin())
        .args(["fleet", "record", "--kind", "SeatSpawned", "--run", &id, "--bead", "s2-live",
               "--seat", "seat-1", "--pid", &pid.to_string(), "--state-dir"])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));
    let out = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "席ごと止まる: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("seats=1 stopped=1"), "席を数える: {}", stdout_of(&out));
    assert!(!Path::new(&format!("/proc/{pid}")).exists(), "runner の process は消えている");
    let kinds: Vec<EventKind> = events(&state)
        .into_iter()
        .filter(|found| found.run == id)
        .map(|found| found.kind)
        .collect();
    assert!(kinds.contains(&EventKind::SeatStopped), "席にも記帳する: {kinds:?}");
    assert!(kinds.contains(&EventKind::RunStopped), "便にも記帳する: {kinds:?}");
    clean(&[&repo, &state]);
}

// ───── 追随の衝突を runner が解く（`s2-07l.146`・ADR-0019 §2.2 / §2.4 / §2.6・接頭辞 `pipe_follow_`） ─────

/// 偽 runner の置き場（呼出回数と turn ごとの stdin）。
fn stub_dir(state: &Path) -> PathBuf {
    state.join("stub")
}

/// 偽 runner が起こされた回数（file が無ければ 0）。**「起こされなかった」を効果で測る**面である。
fn stub_calls(state: &Path) -> usize {
    fs::read_to_string(stub_dir(state).join("calls"))
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

/// n turn 目（1 始まり）に渡された stdin の全文（無ければ空）。
fn stub_stdin(state: &Path, turn: usize) -> String {
    fs::read_to_string(stub_dir(state).join(format!("stdin-{turn}"))).unwrap_or_default()
}

/// 偽 runner（実行 file）の runner cmd。
///
/// turn 1 は契約の実装（`src/lib.rs` の末尾へ `x` を足して commit）で、turn 2 以降は `second` の
/// 本文＝**追随の解き方をここで振る**。どの turn も呼出回数と stdin を置き場へ写すので、
/// 「起こされたか」「何を渡されたか」を rc でなく効果で測れる。`$SHA` には stdin の「追随」節が
/// 名指す main の sha が入る（節が無い周は空＝rebase が落ちて歯が赤くなる＝空虚にならない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn stub_runner(state: &Path, second: &str) -> String {
    let dir = stub_dir(state);
    fs::create_dir_all(&dir).expect("stub の置き場を作れる");
    let path = state.join("stub-runner.sh");
    let body = format!(
        "#!/bin/sh\nD='{}'\nprintf 'call\\n' >> \"$D/calls\"\nN=$(wc -l < \"$D/calls\" | tr -d ' ')\n\
         cat > \"$D/stdin-$N\"\nif [ \"$N\" = 1 ]; then\nprintf 'x\\n' >> src/lib.rs\ngit add -A\n\
         git commit -q -m runner\nexit 0\nfi\n\
         SHA=$(sed -n 's/^- main が \\(.*\\) へ進んだ$/\\1/p' \"$D/stdin-$N\" | head -1)\n{second}\n",
        dir.display()
    );
    fs::write(&path, body).expect("stub を書ける");
    format!("sh {}", path.display())
}

/// turn 2 の本文: 衝突を write-set の中で解いて `git rebase --continue` で終える。
const RESOLVE: &str = "if git rebase \"$SHA\"; then exit 0; fi\nprintf '// seed\\ny\\nx\\n' > src/lib.rs\ngit add src/lib.rs\nGIT_EDITOR=true git rebase --continue";

/// turn 2 の本文: 解かずに木を戻して終わる（次の land でも同じ衝突が起きる）。
const KEEP_CONFLICT: &str = "git rebase \"$SHA\" || git rebase --abort\nexit 0";

/// turn 2 の本文: 木を戻して質問 record で止まる（**commit を作らない**）。
const ABORT_AND_ASK: &str = "git rebase \"$SHA\" || git rebase --abort\nprintf '%s\\n' '{\"question\":\"追随の衝突を解けない\",\"about\":\"write-set\"}'\nexit 76";

/// turn 2 の本文: commit を作ってから質問 record を出す（質問ではなく実装の失敗）。
const COMMIT_THEN_ASK: &str = "git rebase \"$SHA\" || git rebase --abort\nprintf 'z\\n' >> src/lib.rs\ngit add -A\ngit commit -q -m extra\nprintf '%s\\n' '{\"question\":\"追随の衝突を解けない\",\"about\":\"write-set\"}'\nexit 76";

/// turn 2 の本文: rebase の途中のまま turn を終える（木が clean でない）。
const LEAVE_MID_REBASE: &str = "git rebase \"$SHA\" || true\nexit 0";

/// main を**便が触った行の隣**へ進める（追随が必ず衝突する形）。返すのは動いた後の main。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn move_main_into_conflict(repo: &Path) -> String {
    let lib = repo.join("src").join("lib.rs");
    let mut text = fs::read_to_string(&lib).expect("seed を読める");
    text.push_str("y\n");
    fs::write(&lib, text).expect("別便の変更を書ける");
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "other"]);
    git(repo, &["rev-parse", "refs/heads/main"])
}

/// 追随が**必ず衝突する**便を 1 本作る（偽 runner の turn 1 で実装 → PASS の gate → main が
/// 同じ行の隣へ進む）。返すのは 便の id・便の base・動いた main の sha。
fn conflicting_run(repo: &Path, state: &Path, marker: &Path, runner: &str) -> (String, String, String) {
    let base = git(repo, &["rev-parse", "refs/heads/main"]);
    let path = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &path);
    let spawned = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ]);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "turn 1 の spawn: {}", stderr_of(&spawned));
    let lens = fake_lens(marker, &lens_verdict("PASS"));
    let gated = gate_once(repo, state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate: {}", stderr_of(&gated));
    let moved = move_main_into_conflict(repo);
    (id, base, moved)
}

/// land を 1 回撃つ（`extra` で `--runner` / `--lens` / `--rules` を足す）。
fn land_extra(repo: &Path, state: &Path, id: &str, extra: &[&str]) -> Output {
    let mut args: Vec<String> = ["land", "--run", id].iter().map(|item| (*item).to_owned()).collect();
    args.extend([
        "--repo".to_owned(), repo.display().to_string(),
        "--state-dir".to_owned(), state.display().to_string(),
    ]);
    args.extend(extra.iter().map(|item| (*item).to_owned()));
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_pipe(&borrowed)
}

/// 便の worktree が rebase の途中か（`rebase-merge` / `rebase-apply` の有無を現物で見る）。
fn mid_rebase(repo: &Path, id: &str) -> bool {
    let worktree = worktree_of(repo, id);
    let git_dir = PathBuf::from(git(&worktree, &["rev-parse", "--absolute-git-dir"]));
    git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
}

/// 便の `RunStage` の `(段, detail)` の列。
fn stages(state: &Path, id: &str) -> Vec<(Option<Stage>, Option<String>)> {
    trail(state, id)
        .into_iter()
        .filter(|(kind, _, _)| *kind == EventKind::RunStage)
        .map(|(_, stage, detail)| (stage, detail))
        .collect()
}

/// 衝突の記帳（`Implemented detail=rebase-conflict:<range>`）の件数。
fn conflict_count(state: &Path, id: &str) -> usize {
    stages(state, id)
        .into_iter()
        .filter(|(stage, detail)| {
            *stage == Some(Stage::Implemented)
                && detail.as_deref().is_some_and(|found| found.starts_with("rebase-conflict:"))
        })
        .count()
}

/// 衝突の記帳を**手で 1 件積む**（回数が replay の導出であることを測る fixture）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn record_conflict(state: &Path, id: &str, range: &str) {
    let out = Command::new(bin())
        .args(["fleet", "record", "--kind", "RunStage", "--stage", "Implemented", "--run", id,
               "--bead", "s2-2e5", "--detail", &format!("rebase-conflict:{range}"), "--state-dir"])
        .arg(state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fleet record: {}", stderr_of(&out));
}

/// 衝突は**便を終端にしない**（設計 pipeline-conflict.md §3 手順 1〜2）。木を戻して
/// `RunStage Implemented detail=rebase-conflict:<base>..<main>` を **1 件**記帳し、`Failed` は
/// 1 件も書かない。main は 1 byte も動かず、worktree は rebase の途中でなく clean である。
#[test]
fn pipe_follow_records_the_conflict_without_failing_the_run() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_INCONCLUSIVE)),
        "起こし直した周は rc 3（次の段を名乗って止まる）: {}",
        stderr_of(&out)
    );
    assert!(stdout_of(&out).contains(&format!("run={id} next=gate")), "次に撃つ段: {}", stdout_of(&out));
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は 1 件: {:?}", stages(&state, &id));
    assert!(
        stages(&state, &id).iter().any(|(stage, detail)| *stage == Some(Stage::Implemented)
            && detail.as_deref() == Some(format!("rebase-conflict:{base}..{moved}").as_str())),
        "detail は base と main を名乗る: {:?}",
        stages(&state, &id)
    );
    assert!(
        !stages(&state, &id).iter().any(|(stage, _)| *stage == Some(Stage::Failed)),
        "便を終端にしない: {:?}",
        stages(&state, &id)
    );
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は 1 byte も動かない");
    assert!(!mid_rebase(&repo, &id), "木は rebase の途中でない");
    assert!(git(&worktree_of(&repo, &id), &["status", "--porcelain"]).is_empty(), "木は clean");
    assert_eq!(stub_calls(&state), 2, "実装役を 1 回起こし直した");
    clean(&[&repo, &state]);
}

/// 起こし直しの turn の stdin は、契約の写しの**後ろ**に「## 追随」節を持ち、main の sha を
/// 名指す（設計 §3 手順 4）。turn 1 の stdin には節が無い（不在が既定）。
#[test]
fn pipe_follow_second_turn_receives_the_follow_section() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    let first = stub_stdin(&state, 1);
    assert!(first.contains("goal = "), "turn 1 も契約の本文を受ける: {first}");
    assert!(!first.contains("## 追随"), "追随の無い turn には節が付かない: {first}");
    let second = stub_stdin(&state, 2);
    assert!(second.contains("## 追随"), "起こし直しの turn に節が付く: {second}");
    assert!(second.contains(&moved), "節は main の sha を名指す: {second}");
    let contract_at = second.find("goal = ");
    let follow_at = second.find("## 追随");
    assert!(
        matches!((contract_at, follow_at), (Some(c), Some(f)) if c < f),
        "順序は 契約 → 追随: {second}"
    );
    clean(&[&repo, &state]);
}

/// 回答済みの質問を持つ便の起こし直しは、stdin に「回答」と「追随」を**この順**で持つ
/// （設計 §3 手順 4 の「契約 → 回答 → 追随」）。
#[test]
fn pipe_follow_answered_question_comes_before_the_follow_section() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let id = questioned(&repo, &state);
    let answered = run_pipe(&[
        "answer", "--run", &id, "--words", "verify は 1 行目だけを撃つ",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&answered));
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "回答の後の turn: {}", stderr_of(&resumed));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate: {}", stderr_of(&gated));
    move_main_into_conflict(&repo);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    let second = stub_stdin(&state, 2);
    let answer_at = second.find("## 回答");
    let follow_at = second.find("## 追随");
    assert!(
        matches!((answer_at, follow_at), (Some(a), Some(f)) if a < f),
        "順序は 回答 → 追随: {second}"
    );
    assert!(second.contains("verify は 1 行目だけを撃つ"), "回答の逐語も運ぶ: {second}");
    clean(&[&repo, &state]);
}

/// 実装役が衝突を解いた周: 器が**実測した merge-base**で base を進め（`rebase:<old>..<new>`）、
/// 続きの gate が新しい base で PASS（先着便の file が write-set の外に載らない）→ land で
/// `Landed`。起こし直しは 1 回だけ（3 turn 目は起こされない）。
#[test]
fn pipe_follow_resolved_conflict_advances_the_base_and_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "起こし直した周は rc 3: {}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains(&format!("run={id} rebase={base}..{moved}")),
        "新しい base を名乗る: {}",
        stdout_of(&out)
    );
    assert!(
        stages(&state, &id).iter().any(|(stage, detail)| *stage == Some(Stage::Implemented)
            && detail.as_deref() == Some(format!("rebase:{base}..{moved}").as_str())),
        "器が base を進めた記帳: {:?}",
        stages(&state, &id)
    );
    assert!(!mid_rebase(&repo, &id), "木は rebase の途中でない");
    // 続きは gate から（新しい base の 2 点 diff は write-set の中だけ）。
    fs::remove_file(&marker).ok();
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "新しい base の gate は PASS: {}", stderr_of(&gated));
    let landed = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(
        git(&repo, &["show", &format!("{new}:src/lib.rs")]),
        "// seed\ny\nx",
        "先着便の行と便の行が両方 main に載る"
    );
    let landings = trail(&state, &id)
        .into_iter()
        .filter(|(kind, stage, _)| *kind == EventKind::RunDone && *stage == Some(Stage::Landed))
        .count();
    assert_eq!(landings, 1, "Landed は 1 件");
    assert_eq!(stub_calls(&state), 2, "起こし直しは 1 回だけ");
    clean(&[&repo, &state]);
}

/// turn の間に main がさらに進んだ周でも、**書く値は実測した merge-base**であって現在の main
/// ではない（2 点 diff に main の新しい commit の逆向きを載せない・設計 §3 手順 5）。
#[test]
fn pipe_follow_records_the_measured_merge_base_not_the_moving_main() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let racing = format!("{RESOLVE}\ngit -C '{}' commit -q --allow-empty -m racing", repo.display());
    let runner = stub_runner(&state, &racing);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    let raced = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(raced, moved, "turn の中で main がさらに進んでいる");
    let details: Vec<String> = stages(&state, &id)
        .into_iter()
        .filter_map(|(_, detail)| detail)
        .filter(|detail| detail.starts_with("rebase:"))
        .collect();
    assert_eq!(
        details,
        vec![format!("rebase:{base}..{moved}")],
        "書く値は merge-base（現在の main {raced} でない）"
    );
    clean(&[&repo, &state]);
}

/// 上限（fixture の rules で 1 回）まで起こし直し、2 回目の衝突で終端した便を作る。
/// 返すのは 便の id・動いた main の sha・偽 runner の cmd。
fn exhausted_run(repo: &Path, state: &Path, marker: &Path) -> (String, String) {
    let runner = stub_runner(state, KEEP_CONFLICT);
    let (id, _base, moved) = conflicting_run(repo, state, marker, &runner);
    let rules = write_rules_with_retries(state, "rules-retry-1.toml", 1, 1_000_000, 1);
    let rules_arg = rules.display().to_string();
    let first = land_extra(repo, state, &id, &["--runner", &runner, "--rules", &rules_arg]);
    assert_eq!(
        first.status.code(),
        Some(i32::from(RC_INCONCLUSIVE)),
        "上限の内は起こし直す: {}",
        stderr_of(&first)
    );
    fs::remove_file(marker).ok();
    let lens = fake_lens(marker, &lens_verdict("PASS"));
    let gated = gate_with_rules(repo, state, &id, &rules, &lens);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "撃ち直しの gate: {}", stderr_of(&gated));
    let second = land_extra(repo, state, &id, &["--runner", &runner, "--rules", &rules_arg]);
    assert_eq!(
        second.status.code(),
        Some(i32::from(RC_REFUSED)),
        "上限に達した周は rc 1: {}",
        stderr_of(&second)
    );
    (id, moved)
}

/// 上限の歯（値 1 ＝最大 1 回起こし直す）: 2 回目の衝突で `Failed detail=rebase-conflict` になり、
/// runner は 3 turn 目に起こされない。main は動かない。
#[test]
fn pipe_follow_stops_retrying_at_the_limit() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, moved) = exhausted_run(&repo, &state, &marker);
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-conflict".to_owned()))),
        "終端の理由: {:?}",
        stages(&state, &id)
    );
    assert_eq!(conflict_count(&state, &id), 2, "衝突は 2 件記帳された: {:?}", stages(&state, &id));
    assert_eq!(stub_calls(&state), 2, "3 turn 目は起こされない");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(show_line(&repo, &state, &id).contains("stage=Failed"), "段は Failed");
    clean(&[&repo, &state]);
}

/// 上限に達して終端した便（`Failed detail=rebase-conflict`）は `pipe retire` で畳める
/// （move・元 dir 不在・retired/ に在る・**残す event の段は終端のまま**・設計 §5）。
#[test]
fn pipe_follow_retire_folds_an_exhausted_run_and_keeps_the_stage() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id, moved) = exhausted_run(&repo, &state, &marker);
    let live = worktree_of(&repo, &id);
    assert!(live.exists(), "終端した便の worktree は残る（retire の入口の前提）");
    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rebase-conflict の便も畳める: {}", stderr_of(&out));
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id);
    assert!(retired.join("src").join("lib.rs").exists(), "中身ごと運ぶ（消さない）");
    assert!(!live.exists(), "元の場所が空く");
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("retired".to_owned()))),
        "残す event の段は終端のまま: {:?}",
        stages(&state, &id)
    );
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は 1 byte も動かない");
    clean(&[&repo, &state]);
}

/// `Gated` で verdict が FAIL の便（判定に届いた終端・`.132` の memo）も畳める。残す event の
/// 段は `Gated` のままで、`Landed` へ動かさない。
#[test]
fn pipe_follow_retire_folds_a_gated_fail_run_and_keeps_the_stage() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = implemented(&repo, &state, &path);
    let lens = fake_lens(&marker, &lens_verdict("FAIL"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の gate は rc 1: {}", stderr_of(&gated));
    let live = worktree_of(&repo, &id);
    let out = retire_once(&repo, &state, &id);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "Gated(FAIL) の便も畳める: {}", stderr_of(&out));
    assert!(!live.exists(), "元の場所が空く");
    assert!(
        repo.join(".worktrees").join("scribe2").join("retired").join(&id).exists(),
        "retired/ に在る"
    );
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Gated), Some("retired".to_owned()))),
        "段は Gated のまま: {:?}",
        stages(&state, &id)
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Gated"), "畳んだ後も段は Gated");
    clean(&[&repo, &state]);
}

/// `resume` は「最後の段が `Implemented` ∧ 最後の detail が `rebase-conflict:` ∧ runner が
/// 起きていない」周に**同じ起こし直し**を撃つ。`--runner` の無い周は rc 1 で events.jsonl が
/// byte 不変（衝突の記帳は land の時点で済んでいる）。detail が `rebase-conflict:` でない
/// `Implemented` が従来どおり gate へ行くことは
/// `pipe_resume_continues_from_implemented_in_new_process` が測る。
#[test]
fn pipe_follow_resume_needs_a_runner_and_continues_the_retry() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    // `--runner` を渡さない land は衝突を記帳して断る（起こし直せない）。
    let bare_land = land_extra(&repo, &state, &id, &[]);
    assert_eq!(bare_land.status.code(), Some(i32::from(RC_REFUSED)), "起こし直せない land は rc 1");
    assert!(stderr_of(&bare_land).contains("--runner が要る"), "理由: {}", stderr_of(&bare_land));
    assert_eq!(stub_calls(&state), 1, "起こし直していない");
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は残る");
    // `--runner` の無い resume は 1 byte も書かない。
    let before = events_bytes(&state);
    let bare = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(bare.status.code(), Some(i32::from(RC_REFUSED)), "--runner 無しの resume は rc 1");
    assert_eq!(events_bytes(&state), before, "events.jsonl は byte 不変");
    assert_eq!(stub_calls(&state), 1, "runner を起こさない");
    // `--runner` 付きの resume は land の衝突と同じ turn を撃つ。
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "resume の起こし直し: {}", stderr_of(&out));
    assert_eq!(stub_calls(&state), 2, "resume が実装役を起こした");
    assert!(
        stdout_of(&out).contains(&format!("run={id} rebase={base}..{moved}")),
        "resume の turn も base を進める: {}",
        stdout_of(&out)
    );
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段は Implemented（次は gate）");
    clean(&[&repo, &state]);
}

/// 追随を解けずに**木を戻して質問 record で止まった** turn は `Questioned` である
/// （ADR-0019 §2.6: 判定は turn 開始時の tip 基準＝便が base から持つ commit を数えない）。
#[test]
fn pipe_follow_question_after_abort_stops_at_questioned() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, ABORT_AND_ASK);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "質問は rc 3: {}", stderr_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Questioned"), "段は Questioned");
    let worktree = worktree_of(&repo, &id);
    assert!(git(&worktree, &["status", "--porcelain"]).is_empty(), "木は clean");
    assert!(!mid_rebase(&repo, &id), "木は rebase の途中でない");
    assert_eq!(
        git(&worktree, &["rev-list", "--count", &format!("{base}..HEAD")]),
        "1",
        "便が base から持つ commit は在る（turn で増えていないだけ）"
    );
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    assert!(
        trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised),
        "質問の逐語が残る"
    );
    clean(&[&repo, &state]);
}

/// 負例: 起こし直しの turn で **commit を作ってから**質問 record を出した周は質問ではなく
/// 実装の失敗である（`Failed detail=runner-rc:76,commits:1`）。
#[test]
fn pipe_follow_commit_before_question_is_a_failure() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, COMMIT_THEN_ASK);
    let (id, _base, _moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "land はしない: {}", stdout_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("runner-rc:76,commits:1".to_owned()))),
        "turn で作った commit を数える: {:?}",
        stages(&state, &id)
    );
    assert!(
        !trail(&state, &id).iter().any(|(kind, _, _)| *kind == EventKind::QuestionRaised),
        "質問は記帳しない"
    );
    clean(&[&repo, &state]);
}

/// runner が **rebase の途中で** turn を終えた周は `Failed detail=rebase-dirty` で終端する
/// （clean 前提を守る・fail-closed・設計 §3 手順 6）。
#[test]
fn pipe_follow_mid_rebase_turn_fails_dirty() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, LEAVE_MID_REBASE);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "rebase の途中は rc 1: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-dirty".to_owned()))),
        "終端の理由: {:?}",
        stages(&state, &id)
    );
    assert!(mid_rebase(&repo, &id), "木は rebase の途中のまま（器は触らない）");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    clean(&[&repo, &state]);
}

/// 回数は **replay の導出値**である（別の状態 file を持たない・C3）。衝突の `RunStage` を手で
/// 2 件積んだ便は、上限 2（埋め込みの値）の下で次の衝突が終端になり、置き場に新しい file は
/// 1 つも増えない。
#[test]
fn pipe_follow_counts_the_retries_from_the_event_log() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, KEEP_CONFLICT);
    let (id, base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    for _ in 0..2 {
        record_conflict(&state, &id, &format!("{base}..{moved}"));
    }
    // 手で積んだ段は `Implemented` なので、land の前に gate を撃ち直す。
    fs::remove_file(&marker).ok();
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "撃ち直しの gate: {}", stderr_of(&gated));
    let before = dir_names(&state.join("pipe").join(&id));
    let out = land_extra(&repo, &state, &id, &["--runner", &runner]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "上限に達した周は rc 1: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last().cloned(),
        Some((Some(Stage::Failed), Some("rebase-conflict".to_owned()))),
        "終端の理由: {:?}",
        stages(&state, &id)
    );
    assert_eq!(stub_calls(&state), 1, "起こし直さない（回数は log が持つ）");
    assert_eq!(dir_names(&state.join("pipe").join(&id)), before, "置き場に新しい file を作らない");
    clean(&[&repo, &state]);
}

/// 回数を**読めない**周は起こし直さず `Failed detail=follow-unmeasured` + rc 2 で終端する
/// （上限到達の rc 1 と分ける・NFR4）。読めなさは、rebase の呼出しに合わせて event log へ
/// 壊れた行を混ぜる偽 git で作る。
#[test]
fn pipe_follow_unreadable_retry_count_fails_closed_with_rc_two() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE);
    let (id, _base, moved) = conflicting_run(&repo, &state, &marker, &runner);
    let events = state.join("fleet").join("events.jsonl");
    let path = shim_path(
        &state,
        "poison-bin",
        &format!("case \"$*\" in *' rebase '*) printf 'not-json\\n' >> '{}' ;; esac", events.display()),
    );
    let out = Command::new(bin())
        .args(["pipe", "land", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--runner", &runner])
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない回数は rc 2: {}", stderr_of(&out));
    assert_eq!(stub_calls(&state), 1, "起こし直さない");
    let log = fs::read_to_string(&events).expect("event log");
    let last = log.lines().rfind(|line| !line.is_empty()).unwrap_or_default();
    assert!(
        last.contains("\"stage\":\"Failed\"") && last.contains("follow-unmeasured"),
        "終端の理由は上限到達と分ける: {last}"
    );
    assert!(!last.contains("\"detail\":\"rebase-conflict\""), "上限到達を名乗らない: {last}");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は動かない");
    clean(&[&repo, &state]);
}

/// 起こし直しの spawn も **Budget を要る口だけ**を通る（`pipe_spawn_measures_repo_before_launching`
/// と同型の観測）。repo の HEAD を読めない git を前に置くと、起こし直しは Precheck の段で断られ、
/// runner は起こされない。
#[test]
fn pipe_follow_retry_measures_the_repo_before_launching() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let runner = stub_runner(&state, RESOLVE);
    let (id, _base, _moved) = conflicting_run(&repo, &state, &marker, &runner);
    let failing = format!("-C {} rev-parse HEAD", repo.display());
    let path = shim_path(&state, "measure-bin", &format!("case \"$*\" in *'{failing}'*) exit 1;; esac"));
    let out = Command::new(bin())
        .args(["pipe", "land", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--runner", &runner])
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "測れない repo は rc 1: {}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("git repo でない"),
        "測る段で断る（起動関数へ入る前）: {}",
        stderr_of(&out)
    );
    assert_eq!(stub_calls(&state), 1, "起こし直しの runner は起きない");
    assert_eq!(conflict_count(&state, &id), 1, "衝突の記帳は残る（resume で続けられる）");
    clean(&[&repo, &state]);
}

// ---- 封じ込め（設計 docs/design/gate-cost.md §4・ADR-0021 §2.2）--------------------------

/// 偽 `systemd-run` の記録を置く dir 名（**起動ごとに 1 file**）。
const SCOPE_RECORDS: &str = "scope-args";

/// PATH の先頭に置く偽 `systemd-run`（返すのは PATH の値）。
///
/// argv を写してから `--` の後ろを exec する＝**包みの中身は実際に撃たれる**。
///
/// 記録は **`<unit>.args` の 1 起動 1 file** である。1 file へ追記する形は、probe の記録や
/// 別の行の記録まで同じ母集団に入り、`contains` の assert が**撃っていない起動の引数**で
/// 充足する（run 1 の実測: `limit_of` を常に `PerJob` にする変異で 21/21 が緑だった）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn systemd_stub(state: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = state.join("systemd-bin");
    let records = state.join(SCOPE_RECORDS);
    fs::create_dir_all(&bin_dir).expect("stub の dir を作れる");
    fs::create_dir_all(&records).expect("記録の dir を作れる");
    let shim = bin_dir.join("systemd-run");
    let script = format!(
        "#!/bin/sh\n\
         __unit=no-unit\n\
         for __a in \"$@\"; do case \"$__a\" in --unit=*) __unit=${{__a#--unit=}};; esac; done\n\
         printf '%s\\n' \"$@\" > '{}'/\"$__unit\".args\n\
         while [ $# -gt 0 ] && [ \"$1\" != \"--\" ]; do shift; done\n\
         shift\n\
         exec \"$@\"\n",
        records.display()
    );
    fs::write(&shim, script).expect("stub を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("stub に実行権を付ける");
    format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default())
}

/// `sh` と `git` だけを引ける PATH（`systemd-run` の**無い** host を作る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn lean_path(state: &Path) -> String {
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

/// `needle` を名に含む scope 記録の**ちょうど 1 件**の本文（1 行 1 引数）。
///
/// 0 件も 2 件以上も `panic` にするのは、母集団を確かめずに `contains` すると、別の起動の
/// 引数で assert が充足するからである（fixture 衝突）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn scope_record(state: &Path, needle: &str) -> String {
    let dir = state.join(SCOPE_RECORDS);
    let names = dir_names(&dir);
    let hits: Vec<&String> = names.iter().filter(|name| name.contains(needle)).collect();
    assert_eq!(hits.len(), 1, "{needle} の記録はちょうど 1 件（母集団 {names:?}）");
    let name = hits.first().expect("1 件在る");
    fs::read_to_string(dir.join(name)).expect("記録を読める")
}

/// scope 記録の `-p <KEY>=<値>` の値（無ければ空）。
fn scope_prop(record: &str, key: &str) -> String {
    let head = format!("{key}=");
    record
        .lines()
        .find_map(|line| line.strip_prefix(&head))
        .unwrap_or_default()
        .to_owned()
}

/// 埋め込み manifest の整数 1 行（封じ込めの値は `--rules` の override を通らない）。
#[expect(
    clippy::panic,
    reason = "統合 test の helper。clippy の allow-panic-in-tests は #[test] 関数の中だけに効く"
)]
fn embedded_int(id: &str) -> u64 {
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
fn run_pipe_with_path(path: &str, args: &[&str]) -> Output {
    Command::new(bin())
        .arg("pipe")
        .args(args)
        .env("PATH", path)
        .output()
        .expect("binary を起動できる")
}

/// PATH を差し替えて spawn → gate まで通す（gate の rc と便 id を返す）。
fn confined_run(repo: &Path, state: &Path, path: &str, lens: &str) -> (String, Output) {
    let contract = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &contract);
    let spawned = run_pipe_with_path(
        path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT],
    );
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));
    // `--rules` の tmp manifest で撃つ（受付の待ちの上限を fixture の値にする）。
    let gated = run_pipe_with_path(
        path,
        &["gate", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--lens", lens,
          "--rules", &ceiling_rules(state)],
    );
    (id, gated)
}

/// **`{jobs}` を持つ行は実効値へ置換され、job の箱で撃たれる**（設計 §3.3 / §4.2）。
///
/// base の gate はこの宣言を intake で断る（`{jobs}` は置けない穴）ので、この歯は base で
/// 落ちる＝flip の RED である。
#[test]
fn pipe_confine_fills_the_jobs_hole_and_uses_the_job_box() {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, VESSEL_ALLOWED, r#"["sh verify-jobs.sh {jobs}"]"#);
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));

    // **撃たれた側**が受け取った値（record の cmd だけでは置換したことを測れない）。
    // 値は受付の実測で決まる（host 依存）ので、record の `jobs=` と突き合わせる。
    let rows = verify_rows(&state, &id);
    let jobs = row_value(&rows, 2, "jobs");
    let count: u64 = jobs.parse().unwrap_or(0);
    assert!(
        (1..=embedded_int("gate.mutants_jobs")).contains(&count),
        "実効 jobs は 1 以上・上限以下: {jobs}"
    );
    let git_dir = git(&worktree_of(&repo, &id), &["rev-parse", "--absolute-git-dir"]);
    assert_eq!(
        fs::read_to_string(Path::new(&git_dir).join("jobs-seen")).unwrap_or_default(),
        jobs,
        "撃たれた側は record と同じ実効 jobs を受け取る"
    );
    assert_eq!(row_value(&rows, 2, "cmd"), format!("sh verify-jobs.sh {jobs}"), "record の cmd も置換後である");
    assert_eq!(row_value(&rows, 2, "confined"), "true", "包めている");

    // 箱は `実効 jobs × gate.job_memory_mb`・重みは rules 行そのもの（値は manifest が持つ・C1）。
    let record = scope_record(&state, "-common-2-");
    assert_eq!(
        scope_prop(&record, "MemoryMax"),
        format!("{}M", count * embedded_int("gate.job_memory_mb")),
        "job の箱: {record}"
    );
    assert_eq!(
        scope_prop(&record, "CPUWeight"),
        embedded_int("gate.cpu_weight").to_string(),
        "CPU の重み: {record}"
    );
    assert!(!record.contains("MemoryHigh"), "MemoryHigh は付けない（設計 §4.2）: {record}");
    clean(&[&repo, &state]);
}

/// **箱は 2 種で、同じ gate の中で互いに違う値になる**（設計 §4.2）。
///
/// 1 本ずつ別の unit の記録を読む——`{jobs}` 行と非 `{jobs}` 行の記録を混ぜると、
/// `limit_of` を片方へ潰した実装でも両方の assert が通る（fixture 衝突）。
#[test]
fn pipe_confine_uses_two_distinct_boxes_in_one_gate() {
    let (repo, state) = repo_with_state();
    commit_vessel(
        &repo,
        VESSEL_ALLOWED,
        r#"["sh verify-jobs.sh {jobs}", "sh verify-ok.sh"]"#,
    );
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));

    let jobs: u64 = row_value(&verify_rows(&state, &id), 2, "jobs").parse().unwrap_or(0);
    let job_box = scope_prop(&scope_record(&state, "-common-2-"), "MemoryMax");
    let host_box = scope_prop(&scope_record(&state, "-common-3-"), "MemoryMax");
    assert_eq!(
        job_box,
        format!("{}M", jobs * embedded_int("gate.job_memory_mb")),
        "{{jobs}} を持つ行は job の箱（実効 jobs {jobs}）"
    );
    let want_host = vessel::pipe::confine::mem_total_mb(&fs::read_to_string("/proc/meminfo").unwrap_or_default())
        .and_then(|total| total.checked_sub(embedded_int("host.reserve_memory_mb")))
        .filter(|mb| *mb > 0);
    assert_eq!(
        Some(host_box.clone()),
        want_host.map(|mb| format!("{mb}M")),
        "{{jobs}} を持たない行は host の箱（MemTotal − reserve）"
    );
    assert_ne!(job_box, host_box, "2 つの箱は互いに違う値である");
    for unit in ["-common-2-", "-common-3-"] {
        let record = scope_record(&state, unit);
        assert!(
            record.lines().any(|line| line == "OOMPolicy=continue"),
            "{unit} の包みを systemd の OOM 停止から外す: {record}"
        );
    }
    clean(&[&repo, &state]);
}

/// **包めない host では素の `sh -c` で撃ち、record に理由を残す**（止めない・設計 §4.2）。
#[test]
fn pipe_confine_falls_back_to_the_plain_shell_without_the_tool() {
    let (repo, state) = repo_with_state();
    let path = lean_path(&state);
    let marker = state.join("lens-ran");
    let (id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(
        gated.status.code(),
        Some(i32::from(RC_OK)),
        "systemd-run の無い host でも便は流れる: {}",
        stderr_of(&gated)
    );
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "confined"), "false", "包めていない");
    assert_eq!(row_value(&rows, 3, "reason"), "no-systemd-run", "理由は閉じた enum の名");
    assert_eq!(row_value(&rows, 3, "peak_mb"), "-", "測れない peak は 0 と書かない");
    assert_eq!(row_value(&rows, 3, "rc"), "0", "行そのものは撃たれている");
    clean(&[&repo, &state]);
}

/// **peak は包みの終端行から読む**（設計 §4.3）。終端行の無い行は `-` である。
#[test]
fn pipe_confine_reads_the_peak_from_the_trailing_line() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let contract = write_contract(
        &repo,
        &["verify"],
        &[r#"verify = ["sh verify-peak.sh", "sh verify-ok.sh"]"#],
    );
    let id = intake(&repo, &state, &contract);
    let spawned = run_pipe_with_path(
        &path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT],
    );
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));
    let marker = state.join("lens-ran");
    let gated = run_pipe_with_path(
        &path,
        &["gate", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(),
          "--lens", &fake_lens(&marker, &lens_verdict("PASS"))],
    );
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "peak_mb"), "3", "3145728 byte は 3 MiB");
    assert_eq!(row_value(&rows, 4, "peak_mb"), "-", "終端行の無い行は不明（0 ではない）");
    clean(&[&repo, &state]);
}

/// **runner と lens の起動も同じ包みを通る**（設計 §4.1 の 2 つ目と 3 つ目）。
#[test]
fn pipe_confine_wraps_the_runner_and_the_lens() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let (_id, gated) = confined_run(&repo, &state, &path, &fake_lens(&marker, &lens_verdict("PASS")));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    assert!(marker.exists(), "lens は実際に撃たれている（包みは行を殺さない）");
    for stage in ["-runner-1-", "-lens-1-"] {
        let record = scope_record(&state, stage);
        assert!(record.lines().any(|line| line == "--scope"), "{stage} は scope である: {record}");
        assert!(
            record.lines().any(|line| line == "OOMPolicy=continue"),
            "{stage} も OOM 停止から外す: {record}"
        );
    }
    clean(&[&repo, &state]);
}

/// **箱の中で殺された verify 行は赤ではなく「測れなかった」**（設計 §4.2）。
///
/// rc は 0 のままの fixture で撃つ＝根拠が rc ではなく終端行の `oom_kill` であることを測る。
#[test]
fn pipe_confine_oom_verify_line_is_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let contract = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-oom.sh"]"#]);
    let id = intake(&repo, &state, &contract);
    let spawned = run_pipe_with_path(
        &path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT],
    );
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));
    let marker = state.join("lens-ran");
    let gated = run_pipe_with_path(
        &path,
        &["gate", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(),
          "--lens", &fake_lens(&marker, &lens_verdict("PASS"))],
    );
    assert_eq!(gated.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stdout_of(&gated));
    assert!(stdout_of(&gated).contains("verdict=INCONCLUSIVE"), "{}", stdout_of(&gated));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "rc"), "0", "rc は 0 のまま（rc では見ていない）");
    assert_eq!(row_value(&rows, 3, "reason"), "oom-kill", "外からの kill と弁別する");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "0", "赤には数えない");
    assert!(!marker.exists(), "測れなかった周は lens を起動しない");
    clean(&[&repo, &state]);
}

/// **runner の箱が溢れた便は `Failed detail=oom-kill` で終端する**（設計 §4.2・理由 1 つ）。
#[test]
fn pipe_confine_oom_runner_fails_the_run() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let contract = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &contract);
    let runner = format!("{TOY_COMMIT}\nprintf 'confine-usage peak_bytes=9437184 oom_kill=1\\n'");
    let out = run_pipe_with_path(
        &path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", &runner],
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "記帳は通る: {}", stderr_of(&out));
    let seen = trail(&state, &id);
    assert!(
        seen.contains(&(EventKind::RunStage, Some(Stage::Failed), Some("oom-kill".to_owned()))),
        "閉じた理由 1 つで終端する: {seen:?}"
    );
    clean(&[&repo, &state]);
}

/// **lens の箱が溢れた周は INCONCLUSIVE**（FR9 の既存極性のまま・便は終端しない・設計 §4.2）。
#[test]
fn pipe_confine_oom_lens_is_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let body = lens_verdict("PASS");
    let lens = format!(
        "{}; printf 'confine-usage peak_bytes=9437184 oom_kill=1\\n'",
        fake_lens(&marker, &body)
    );
    let (id, gated) = confined_run(&repo, &state, &path, &lens);
    assert_eq!(gated.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stdout_of(&gated));
    assert!(marker.exists(), "lens は起動されている（判定だけが届かない）");
    assert!(
        value_of(&verdict_pairs(&state, &id), "evidence").contains("oom-kill"),
        "理由が verdict に残る: {:?}",
        verdict_pairs(&state, &id)
    );
    assert!(
        show_line(&repo, &state, &id).contains("stage=Gated"),
        "便は終端しない（測り直せる）: {}",
        show_line(&repo, &state, &id)
    );
    clean(&[&repo, &state]);
}

/// host の受付札の置き場（`<state_dir の親>/scribe2-host/slots`・設計 gate-cost.md §3.2）。
///
/// **lib の関数を通さず字面で組む**——同じ関数で置き場を引くと、導き方を変えた実装でも歯が
/// 追随して通る（置き場の字面そのものを pin する）。
fn host_slots(state: &Path) -> PathBuf {
    state.parent().unwrap_or(state).join("scribe2-host").join("slots")
}

/// 存在しない pid（`pid_max` の上限 4194304 を超える）。
const DEAD_PID: u64 = 4_000_000_000;

/// 受付札を 1 枚置く（歯が置く札・中身は 1 行 JSON）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn plant_ticket(state: &Path, pid: u64, jobs: u64) -> PathBuf {
    let dir = host_slots(state);
    fs::create_dir_all(&dir).expect("slot dir を作れる");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX));
    let path = dir.join(format!("{pid}-planted.slot"));
    let body = format!("{{\"schema\":1,\"pid\":{pid},\"run\":\"planted\",\"jobs\":{jobs},\"ts\":{ts}}}\n");
    fs::write(&path, body).expect("札を置ける");
    path
}

/// 受付の歯の宣言: `{jobs}` の無い行 → `{jobs}` の行の順に、**撃たれた側で** slot dir を写す。
///
/// 札は行の終了で消えるので、外から gate の後に見ても「在った」ことは測れない。行の中から
/// dir の中身と札の本文を git の dir へ写す（script の本文は宣言の検査の外）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn commit_slot_vessel(repo: &Path, state: &Path) {
    let slots = host_slots(state);
    let dir = slots.display();
    let seen = "\"$(git rev-parse --absolute-git-dir)\"";
    let plain = format!("ls -A '{dir}' > {seen}/slots-plain 2>/dev/null\nexit 0\n");
    let jobs = format!(
        "printf '%s' \"$1\" > {seen}/jobs-seen\nls -A '{dir}' > {seen}/slots-during 2>/dev/null\n\
         cat '{dir}'/*.slot > {seen}/slots-body 2>/dev/null\nexit 0\n"
    );
    fs::write(repo.join("verify-slot-plain.sh"), plain).expect("script を書ける");
    fs::write(repo.join("verify-slot.sh"), jobs).expect("script を書ける");
    git(repo, &["add", "verify-slot-plain.sh", "verify-slot.sh"]);
    commit_vessel(repo, VESSEL_ALLOWED, r#"["sh verify-slot-plain.sh", "sh verify-slot.sh {jobs}"]"#);
}

/// 受付の歯の 1 便（stub の `systemd-run` で包める host を作り、`--rules` の fixture で gate）。
fn slot_gate(repo: &Path, state: &Path) -> (String, Output) {
    commit_slot_vessel(repo, state);
    let path = systemd_stub(state);
    let marker = state.join("lens-ran");
    confined_run(repo, state, &path, &fake_lens(&marker, &lens_verdict("PASS")))
}

/// `slot=` を持つ record（**ちょうど 1 件**・母集団を確かめてから読む）。
fn slot_row(
    rows: &[Vec<(String, vessel::fleet::json_lite::Value)>],
) -> Vec<(String, vessel::fleet::json_lite::Value)> {
    let hits: Vec<&Vec<(String, vessel::fleet::json_lite::Value)>> =
        rows.iter().filter(|row| row.iter().any(|(key, _)| key == "slot")).collect();
    assert_eq!(hits.len(), 1, "受付を通った record はちょうど 1 件: {rows:?}");
    hits.first().map(|row| (*row).clone()).unwrap_or_default()
}

/// slot dir の `.slot` の名（dir が無ければ空）。
fn slot_names(state: &Path) -> Vec<String> {
    fs::read_dir(host_slots(state))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".slot"))
                .collect()
        })
        .unwrap_or_default()
}

/// record の `slot` が回収 1 枚を含む（`reclaimed:1` か `degraded,reclaimed:1`）。
///
/// 枠を配れたかは host の空き memory に依る（MemAvailable が `reserve + job` 未満の host では
/// 縮退する）ので pin しない——e2e は実 host の meminfo に依らない（設計 §7）。
fn assert_reclaimed_one(slot: &str, why: &str) {
    assert!(
        slot == "reclaimed:1" || slot == "degraded,reclaimed:1",
        "{why}: slot は回収 1 枚を含む: {slot}"
    );
}

/// **project 2 つの gate が同じ host の slot dir を見て、死んだ札を回収する**（設計 §3.2・歯 (1) (3)）。
///
/// 札は置き場ごとではなく `<state_dir の親>` から導く＝同じ tmp root の 2 つの置き場で、片方の
/// gate が回収した後にもう片方の gate が**同じ dir に置き直した**札を回収する。
#[test]
fn pipe_slots_two_projects_share_one_dir_and_reclaim_dead_tickets() {
    let root = tmp();
    let (repo_a, state_a) = repo_with_state_in(&root.join("state-a"));
    let (repo_b, state_b) = repo_with_state_in(&root.join("state-b"));
    assert_eq!(host_slots(&state_a), host_slots(&state_b), "2 つの置き場の親は同じ");

    let dead = plant_ticket(&state_a, DEAD_PID, 1);
    let (id_a, gated_a) = slot_gate(&repo_a, &state_a);
    assert_eq!(gated_a.status.code(), Some(i32::from(RC_OK)), "gate a: {}", stderr_of(&gated_a));
    assert_reclaimed_one(&value_of(&slot_row(&verify_rows(&state_a, &id_a)), "slot"), "a が回収した");
    assert!(!dead.exists(), "死んだ札は削除された");

    // **b の置き場からは札を置かない**（a の親に置いた札を b の gate が拾う＝親が一致する）。
    let again = plant_ticket(&state_a, DEAD_PID, 1);
    let (id_b, gated_b) = slot_gate(&repo_b, &state_b);
    assert_eq!(gated_b.status.code(), Some(i32::from(RC_OK)), "gate b: {}", stderr_of(&gated_b));
    assert_reclaimed_one(&value_of(&slot_row(&verify_rows(&state_b, &id_b)), "slot"), "b も同じ dir を回収した");
    assert!(!again.exists(), "置き直した札も削除された");
    clean(&[&repo_a, &repo_b, &root]);
}

/// **生きている札が枠を食い尽くすと待ち、上限を超えたら並列度 1 で進む**（歯 (2)）。
///
/// 札の pid はこの歯の process（gate の間ずっと生きている）で、jobs を host の総量より大きく
/// 置いて `by_token` を 0 にする。待ちの上限は rules fixture の `slot_wait_s = 1`。
#[test]
fn pipe_slots_live_ticket_waits_then_degrades_to_one_job() {
    let (repo, state) = repo_with_state();
    let live = plant_ticket(&state, u64::from(std::process::id()), 1_000_000);
    let started = std::time::Instant::now();
    let (id, gated) = slot_gate(&repo, &state);
    let took = started.elapsed();
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "縮退しても便は流れる: {}", stderr_of(&gated));
    let row = slot_row(&verify_rows(&state, &id));
    assert_eq!(value_of(&row, "slot"), "degraded", "上限を超えた: {row:?}");
    assert_eq!(value_of(&row, "jobs"), "1", "並列度 1 で進む（0 で走らせない）");
    assert_eq!(value_of(&row, "cmd"), "sh verify-slot.sh 1", "置換後の cmd に 1 が載る");
    assert!(took >= std::time::Duration::from_secs(SLOT_WAIT_S), "待った: {took:?}");
    assert!(live.exists(), "生きている札は回収しない");
    clean(&[&repo, &state]);
}

/// 待ちが解ける歯の待ちの上限（秒）。歯が札を消すまでの時間より十分に長く置く。
const SLOT_WAIT_LONG_S: u64 = 60;

/// **待ちの途中で塞いでいた札が消えると、上限を待たずに枠を配って進む**（歯 (9)）。
///
/// - 容量の 2 線を fixture で最小（job 1 MiB・reserve 0）にし、枠を配れるかを host の空き memory に
///   依らせない（設計 §7）。塞ぐのは自 pid の札（jobs を host の総量より大きく置く）。
/// - 待ちの始まりは**先に置いた死んだ札が 1 周目の受付で回収される**ことで知る（壁時計に頼らない）。
/// - 待ちの間に死んだ札をもう 1 枚置き、**回収されずに残る**ことを測る（待ちの観測は lock も回収も
///   持たない＝観測を常に「空いた」と読む実装は受付を回し続けて札を回収する）。
/// - 観測を常に「空かない」と読む実装は上限まで待って `degraded` になる。
#[test]
fn pipe_slots_wait_ends_early_when_the_blocking_ticket_goes() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let (repo, state) = repo_with_state();
    commit_slot_vessel(&repo, &state);
    let path = systemd_stub(&state);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let slots = SlotFixture { job_mb: 1, reserve_mb: 0, wait_s: SLOT_WAIT_LONG_S };
    let rules = write_rules_full(&state, "rules-slot-long.toml", (1, 1_000_000), FOLLOW_RETRIES, slots);
    let contract = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &contract);
    let spawned = run_pipe_with_path(
        &path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", TOY_COMMIT],
    );
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));

    let first_dead = plant_ticket(&state, DEAD_PID, 1);
    let live = plant_ticket(&state, u64::from(std::process::id()), 1_000_000_000_000);
    let mut child = Command::new(bin())
        .arg("pipe")
        .args(["gate", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--lens", &lens,
               "--rules", &rules.display().to_string()])
        .env("PATH", &path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary を起動できる");

    // 1 周目の受付が死んだ札を回収した＝枠が 0 で待ちに入った（塞ぐ札は生きている）。
    let begun = Instant::now();
    while first_dead.exists() {
        assert!(child.try_wait().ok().flatten().is_none(), "gate が受付の前に終わった");
        assert!(begun.elapsed() < Duration::from_secs(120), "1 周目の受付が来ない");
        std::thread::sleep(Duration::from_millis(10));
    }
    let second_dead = plant_ticket(&state, DEAD_PID, 1);
    std::thread::sleep(Duration::from_millis(500));
    assert!(second_dead.exists(), "待ちの間は札を回収しない（観測は受付を回さない）");
    fs::remove_file(&second_dead).expect("札を消せる");

    let freed = Instant::now();
    fs::remove_file(&live).expect("塞いでいた札を消せる");
    let out = child.wait_with_output().expect("gate を待てる");
    let took = freed.elapsed();
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&out));
    let row = slot_row(&verify_rows(&state, &id));
    // Granted の字面（回収 1 枚＝1 周目の死んだ札・`slot_detail` の合成）。
    assert_eq!(value_of(&row, "slot"), "reclaimed:1", "上限を待たずに枠を配った: {row:?}");
    assert!(took < Duration::from_secs(SLOT_WAIT_LONG_S / 2), "上限を待っていない: {took:?}");
    assert!(slot_names(&state).is_empty(), "終了で札が消える: {:?}", slot_names(&state));
    clean(&[&repo, &state]);
}

/// **`{jobs}` の行だけが札を置き、終了で消し、置換後の cmd に実効 jobs が載る**（歯 (4) (5) (6)）。
#[test]
fn pipe_slots_ticket_lives_only_during_the_jobs_line() {
    let (repo, state) = repo_with_state();
    let (id, gated) = slot_gate(&repo, &state);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let rows = verify_rows(&state, &id);
    let row = slot_row(&rows);
    assert_ne!(value_of(&row, "slot"), "unmeasured", "meminfo の在る host では測れる: {row:?}");
    let jobs = value_of(&row, "jobs");
    let count: u64 = jobs.parse().unwrap_or(0);
    assert!((1..=embedded_int("gate.mutants_jobs")).contains(&count), "実効 jobs は 1..=上限: {jobs}");
    assert_eq!(value_of(&row, "cmd"), format!("sh verify-slot.sh {jobs}"), "(6) record の cmd は置換後");

    let git_dir = PathBuf::from(git(&worktree_of(&repo, &id), &["rev-parse", "--absolute-git-dir"]));
    let read = |name: &str| fs::read_to_string(git_dir.join(name)).unwrap_or_default();
    assert_eq!(read("jobs-seen"), jobs, "(6) 撃たれた側も同じ実効 jobs");
    // (4) `{jobs}` の無い行の間は札が無い（先に撃つ行・札を置く前）。
    assert!(!read("slots-plain").contains(".slot"), "(4) {{jobs}} の無い行は札を作らない: {}", read("slots-plain"));
    // (5) `{jobs}` の行の間は自便の札がちょうど 1 枚在り、本文の jobs が実効 jobs と一致する。
    let during: Vec<String> = read("slots-during")
        .lines()
        .filter(|name| name.ends_with(".slot"))
        .map(str::to_owned)
        .collect();
    assert_eq!(during.len(), 1, "(5) 行の間は札 1 枚: {during:?}");
    assert!(during.iter().all(|name| name.ends_with(&format!("-{id}.slot"))), "札の名は <pid>-<run>.slot: {during:?}");
    let body = vessel::fleet::json_lite::parse_object(read("slots-body").trim()).unwrap_or_default();
    assert_eq!(value_of(&body, "jobs"), jobs, "札の jobs は実効 jobs: {}", read("slots-body"));
    assert_eq!(value_of(&body, "run"), id, "札の run は便 id");
    // (5) 終了で札は消える。
    assert!(slot_names(&state).is_empty(), "(5) 終了で札が消える: {:?}", slot_names(&state));
    clean(&[&repo, &state]);
}

/// 検出線の stub の行（`{base}` を印に埋める＝置換されたことを撃たれた側で読める）。
const DETECTION_COUNT: &str = r#"["sh verify-count.sh detection-{base}"]"#;

/// `detection-verify` を持つ宣言を commit する（共通 verify も stub・`allowed-commands` は toy のまま）。
fn commit_detection_vessel(repo: &Path, detection: &str) {
    write_vessel(repo, VESSEL_ALLOWED, r#"["sh verify-count.sh common"]"#);
    let path = repo.join(".vessel.toml");
    let body = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, format!("{body}detection-verify = {detection}\n")).ok();
    git(repo, &["add", "-f", ".vessel.toml"]);
    git(repo, &["commit", "-q", "-m", "vessel-detection"]);
}

/// 検出線を持つ toy repo と、契約 verify も stub にした契約 file。
fn detection_repo(detection: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    commit_detection_vessel(&repo, detection);
    let contract = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-count.sh contract"]"#]);
    (repo, state, contract)
}

/// 呼出回数 file の行（撃たれた順）。
fn detection_calls(repo: &Path) -> Vec<String> {
    fs::read_to_string(repo.join(".git").join("detection-calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// land の main 実測の record を全部読む。
fn main_rows(state: &Path, id: &str) -> Vec<Vec<(String, vessel::fleet::json_lite::Value)>> {
    fs::read_to_string(state.join("pipe").join(id).join("verify-main.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| vessel::fleet::json_lite::parse_object(line.trim()).ok())
        .collect()
}

/// record 列の `kind` の並び。
fn kinds(rows: &[Vec<(String, vessel::fleet::json_lite::Value)>]) -> Vec<String> {
    rows.iter().map(|row| value_of(row, "kind")).collect()
}

/// [`detection_land`] の結果。
struct DetectionLand {
    /// **land が足した**呼出行（撃たれた順）。
    added: Vec<String>,
    /// main 実測の record。
    rows: Vec<Vec<(String, vessel::fleet::json_lite::Value)>>,
    /// land の出力。
    out: Output,
    /// 対象 repo。
    repo: PathBuf,
    /// 置き場。
    state: PathBuf,
    /// 便 id。
    id: String,
}

/// PASS まで通し、`verdict.json` を `edit(本文, tree)` で差し替えてから land する。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn detection_land(edit: fn(&str, &str) -> String) -> DetectionLand {
    let (repo, state, contract) = detection_repo(DETECTION_COUNT);
    let id = gated_pass(&repo, &state, &contract, &state.join("lens-ran"));
    let before = detection_calls(&repo).len();
    let verdict = state.join("pipe").join(&id).join("verdict.json");
    let text = fs::read_to_string(&verdict).expect("verdict.json を読める");
    let tree = value_of(&verdict_pairs(&state, &id), "tree");
    assert!(!tree.is_empty(), "差し替える前の verdict は tree を持つ: {text}");
    fs::write(&verdict, edit(&text, &tree)).expect("verdict.json を差し替えられる");
    let out = land_once(&repo, &state, &id);
    let added = detection_calls(&repo).split_off(before);
    let rows = main_rows(&state, &id);
    DetectionLand { added, rows, out, repo, state, id }
}

/// (1) `detection-verify` が読めて、gate が **① write-set → ② common → ③ detection → ④ 契約** の順で撃つ。
#[test]
fn pipe_detection_verify_fires_third_in_gate() {
    let (repo, state, contract) = detection_repo(DETECTION_COUNT);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let id = gated_pass(&repo, &state, &contract, &state.join("lens-ran"));
    let rows = verify_rows(&state, &id);
    assert_eq!(kinds(&rows), ["write-set", "common", "detection", "contract"], "段の順序と kind: {rows:?}");
    assert_eq!(row_value(&rows, 3, "cmd"), format!("sh verify-count.sh detection-{base}"), "③ の穴は置換される");
    assert_eq!(
        detection_calls(&repo),
        ["common".to_owned(), format!("detection-{base}"), "contract".to_owned()],
        "撃たれた側の順序も ②③④"
    );
    clean(&[&repo, &state]);
}

/// (2) `verdict.json` の `tree` は **gate を撃った HEAD の木**（base の木ではない）。
#[test]
fn pipe_detection_verdict_carries_tree_of_gated_head() {
    let (repo, state, contract) = detection_repo(DETECTION_COUNT);
    let id = gated_pass(&repo, &state, &contract, &state.join("lens-ran"));
    let tree = value_of(&verdict_pairs(&state, &id), "tree");
    assert!(!tree.is_empty(), "tree が在る");
    assert_eq!(tree, git(&worktree_of(&repo, &id), &["rev-parse", "HEAD^{tree}"]), "HEAD の木と一致");
    assert_ne!(tree, git(&repo, &["rev-parse", "HEAD^{tree}"]), "base の木ではない（runner が commit した後の木）");
    clean(&[&repo, &state]);
}

/// (3) 木が gate と同じ main 実測は **③ だけを撃たず** `skipped=detection tree=<sha>` を記す（②④は撃つ）。
#[test]
fn pipe_detection_land_skips_detection_when_tree_matches() {
    let landed = detection_land(|text, _| text.to_owned());
    let (rows, out) = (&landed.rows, &landed.out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(out));
    assert_eq!(landed.added, ["common", "contract"], "main 実測は ②④ だけを撃つ");
    assert_eq!(kinds(rows), ["write-set", "common", "detection", "contract"], "省いた段も位置に record が在る: {rows:?}");
    assert_eq!(row_value(rows, 3, "skipped"), "detection", "skipped=detection");
    assert_eq!(row_value(rows, 3, "tree"), git(&landed.repo, &["rev-parse", "refs/heads/main^{tree}"]), "tree=<land した木>");
    assert_eq!(row_value(rows, 4, "rc"), "0", "④ は撃って緑");
    clean(&[&landed.repo, &landed.state]);
}

/// ③ を撃った main 実測の共通 assert（(4) / (5)）。
fn assert_detection_fired(landed: &DetectionLand) {
    let (added, rows) = (&landed.added, &landed.rows);
    assert_eq!(landed.out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&landed.out));
    assert_eq!(added.len(), 3, "②③④ を全部撃つ: {added:?}");
    assert!(added.get(1).is_some_and(|call| call.starts_with("detection-")), "③ が呼ばれる: {added:?}");
    assert_eq!(kinds(rows), ["write-set", "common", "detection", "contract"], "③ は撃った record: {rows:?}");
    assert!(rows.iter().all(|row| value_of(row, "skipped").is_empty()), "省いた record は無い: {rows:?}");
}

/// (4) verdict の `tree` が land した木と違えば **③ も撃つ**。
#[test]
fn pipe_detection_land_fires_detection_when_tree_differs() {
    let landed = detection_land(|text, tree| {
        text.replace(&format!("\"tree\":\"{tree}\""), "\"tree\":\"0000000000000000000000000000000000000000\"")
    });
    let verdict = verdict_pairs(&landed.state, &landed.id);
    assert_eq!(value_of(&verdict, "tree"), "0".repeat(40), "fixture は壊した tree");
    assert_detection_fired(&landed);
    clean(&[&landed.repo, &landed.state]);
}

/// (5) `tree` の無い verdict（旧 gate の形）でも **③ を撃つ**。
#[test]
fn pipe_detection_land_fires_detection_when_verdict_has_no_tree() {
    let landed = detection_land(|text, tree| text.replace(&format!(",\"tree\":\"{tree}\""), ""));
    let verdict = verdict_pairs(&landed.state, &landed.id);
    assert!(verdict.iter().all(|(key, _)| key != "tree"), "fixture は tree の無い旧形: {verdict:?}");
    assert_eq!(value_of(&verdict, "verdict"), "PASS", "fixture の verdict は読める形のまま");
    assert_detection_fired(&landed);
    clean(&[&landed.repo, &landed.state]);
}

/// (6) 検出線の rc≠0 は **従来どおり gate FAIL**（測れなかったを通ったに化けさせない・lens を呼ばない）。
#[test]
fn pipe_detection_red_line_fails_gate() {
    let (repo, state, contract) = detection_repo(r#"["sh verify-red.sh"]"#);
    let id = implemented(&repo, &state, &contract);
    let marker = state.join("lens-ran");
    let out = gate_once(&repo, &state, &id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の rc は 1: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("verdict=FAIL"), "{}", stdout_of(&out));
    let rows = verify_rows(&state, &id);
    assert_eq!(row_value(&rows, 3, "kind"), "detection", "③ の record");
    assert_eq!(row_value(&rows, 3, "rc"), "1", "③ が赤");
    assert_eq!(value_of(&verdict_pairs(&state, &id), "verify_red"), "1", "赤は ③ の 1 本");
    assert!(!marker.exists(), "赤い周は lens を起動しない");
    clean(&[&repo, &state]);
}

/// (7) `detection-verify` の行にも共通 verify と**同じ検査**を掛け、同じ理由の字面で rc 1 に断る。
#[test]
fn pipe_detection_intake_refuses_unfit_lines() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    for (detection, want) in [
        // `cargo` は上限には在るが、この repo の宣言の allowlist には無い。
        (r#"["cargo xtask mutants-diff --base {base}"]"#.to_owned(), "先頭 command cargo が"),
        ("[\"git --version\u{7}\"]".to_owned(), "制御文字"),
        (r#"["git rev-parse --git-dir ../../../etc"]"#.to_owned(), "repo の外"),
    ] {
        commit_detection_vessel(&repo, &detection);
        let out = intake_raw(&repo, &state, &path, "b");
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{detection} は rc 1: {err}");
        assert!(err.contains(want), "{detection} の理由は共通 verify と同じ字面 {want}: {err}");
        assert!(err.contains("detection-verify"), "どの key の行かを名指す: {err}");
    }
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    // 弁別: 検査を通る行なら同じ宣言の形で便が起きる（key そのものを断っているのではない）。
    commit_detection_vessel(&repo, DETECTION_COUNT);
    let ok = intake_raw(&repo, &state, &path, "b");
    assert_eq!(ok.status.code(), Some(i32::from(RC_OK)), "検査を通る検出線は読める: {}", stderr_of(&ok));
    clean(&[&repo, &state]);
}
