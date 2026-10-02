// flip-check: moved s2-07l.264
//! 便を起こす側の歯: `pipe_spawn_` / `pipe_approval_` / `pipe_question_` / `pipe_resume_` / `pipe_report_`・
//! fake runner を toy repo で回す縦 1 本（`pipe_e2e_` / `pipe_five_` / `pipe_guard_`）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く（歯の本文は移しただけ・`s2-07l.264`）。
//!
//! 歯は族ごとの子 module にも置く（設計 docs/design/carry-prep.md §10 行 o・`s2-07l.687`）: `question`（接頭辞
//! `pipe_question_` / `pipe_resume_`）・`approval`（接頭辞 `pipe_approval_` / `run_cost_` / `pipe_report_`）。この file には
//! 子の歯が使う helper と const・use の行・他の族の歯だけを残す。

mod approval;
mod question;
// flip-check: moved s2-07l.687

use super::*;
use vessel::fleet::{Cost, CostSource, Usage};
use vessel::name::PLUGIN_DIR;
use vessel::pipe::land;

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

/// 接頭辞は受付の宣言だけの文法（`s2-07l.287`・設計 contract-source.md §3）: `+new.rs` / `-old.rs` / `dir/` / `plain`
/// の契約を spawn した worktree の policy は接頭辞を剥がした素の 4 行で、dir の末尾 `/` は残る（guard は素の path を読む
/// ＝管理席が契約 file で手剥がしする手順が要らない）。
#[test]
fn pipe_spawn_write_policy_strips_the_item_prefixes() {
    let (repo, state) = repo_with_state();
    let path = write_contract(
        &repo,
        &["write-set"],
        &[r#"write-set = ["+src/new.rs", "-src/lib.rs", "src/", "verify-ok.sh"]"#],
    );
    let id = intake(&repo, &state, &path);
    run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", "true",
    ]);
    let worktree = repo.join(".worktrees").join("scribe2").join(&id);
    let git_dir = PathBuf::from(git(&worktree, &["rev-parse", "--absolute-git-dir"]));
    let body = fs::read_to_string(git_dir.join("scribe2").join("write-set.txt")).expect("policy を読める");
    assert_eq!(body, "src/new.rs\nsrc/lib.rs\nsrc/\nverify-ok.sh\n", "接頭辞が無く末尾 / は残る: {body:?}");
    assert!(body.lines().all(|line| !line.starts_with(['+', '-'])), "接頭辞は 1 行にも写らない: {body:?}");
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

/// 管理席の pane を名乗る値（親 process に置く・`%` 始まりは tmux の pane id の形）。
const ADMIN_PANE: &str = "%99";

/// 親の env に `TMUX_PANE` を置いて `pipe` を 1 回撃つ（管理席の shell から撃つ形）。
///
/// 起動は [`pipe_cmd`]（口 (i)）で組む——道具箱の PATH は撃つ argv の置き場から来る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_pipe_in_pane(args: &[&str]) -> Output {
    pipe_cmd(args)
        .env("TMUX_PANE", ADMIN_PANE)
        .output()
        .expect("binary を起動できる")
}

/// env の写しの変数名の集合。
fn env_keys(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, _)| key.to_owned())
        .collect()
}

/// **runner は起動側の `TMUX_PANE` を継承しない**（設計 seat-roles.md §4・ADR-0022 §2.3）。
///
/// 継承すると runner の hook が `--pane` で管理席の打刻へ書く（他 process の打刻の混入）。
/// base では継承されて在る＝flip の RED。器固有の接頭辞の集合は親と同じまま（外すのは 1 つ）。
#[test]
fn pipe_spawn_drops_tmux_pane_from_runner_env() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let runner = "env > env.txt && git add -A && git commit -q -m runner";
    let out = run_pipe_in_pane(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let env_text = fs::read_to_string(worktree_of(&repo, &id).join("env.txt")).expect("env の写しを読める");
    let child = env_keys(&env_text);
    // 母集団: 写しが空だと「無い」が空虚に通る。親から継承した PATH が在ることを先に測る。
    assert!(child.contains("PATH"), "env の写しは親の env を継承している（母集団 {} 行）", env_text.lines().count());
    assert!(!child.contains("TMUX_PANE"), "runner は TMUX_PANE を継承しない: {child:?}");

    const OURS: &str = "SCRIBE2_";
    let parent: BTreeSet<String> = std::env::vars()
        .map(|(key, _)| key)
        .filter(|key| key.starts_with(OURS))
        .collect();
    let ours: BTreeSet<String> = child.into_iter().filter(|key| key.starts_with(OURS)).collect();
    assert_eq!(ours, parent, "器固有の env は親と同じ（足さない・外さない）");
    clean(&[&repo, &state]);
}

/// **lens も起動側の `TMUX_PANE` を継承しない**（gate の lens cmd も同じ `wrap_line` を通る）。
#[test]
fn pipe_spawn_drops_tmux_pane_from_lens_env() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let seen = state.join("lens-env");
    let lens = format!("cat >/dev/null; env > '{}'; echo '{}'", seen.display(), lens_verdict("PASS"));
    let out = run_pipe_in_pane(&[
        "gate", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let env_text = fs::read_to_string(&seen).expect("lens の env の写しを読める");
    let child = env_keys(&env_text);
    assert!(child.contains("PATH"), "env の写しは親の env を継承している（母集団 {} 行）", env_text.lines().count());
    assert!(!child.contains("TMUX_PANE"), "lens は TMUX_PANE を継承しない: {child:?}");
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

/// plugin（生成 dir [`PLUGIN_DIR`] の下の `.claude-plugin/` と `hooks/`）を持つ toy repo と置き場を作る。
///
/// `README.md` も置くのは、**写しに worktree の他の file が混ざらない**ことを負例で
/// 測るためである（plugin の 2 dir だけを写す、が契約）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn repo_with_plugin() -> (PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    let payload = repo.join(PLUGIN_DIR);
    fs::create_dir_all(payload.join(".claude-plugin")).expect(".claude-plugin を作れる");
    fs::write(payload.join(".claude-plugin").join("plugin.json"), PLUGIN_JSON)
        .expect("plugin.json を書ける");
    fs::create_dir_all(payload.join("hooks")).expect("hooks dir を作れる");
    fs::write(payload.join("hooks").join("hooks.json"), HOOKS_JSON).expect("hooks.json を書ける");
    // plugin dir の**中**の symlink（写してはならない entry）。
    std::os::unix::fs::symlink("../../README.md", payload.join("hooks").join("outside.json"))
        .expect("hooks の中に symlink を置ける");
    fs::write(repo.join("README.md"), "# toy\n").expect("README を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "plugin"]);
    // **commit の後**に anchor 側だけを汚す。便の worktree は base の checkout なので
    // 写しがこの本文になったら、写し元が worktree でなく anchor である証拠になる。
    fs::write(payload.join(".claude-plugin").join("plugin.json"), PLUGIN_JSON_DIRTY)
        .expect("anchor の plugin.json を汚せる");
    (repo, state)
}

/// 生成 dir（[`PLUGIN_DIR`]）の下の repo 相対 path。
fn payload_rel(rel: &str) -> String {
    format!("{PLUGIN_DIR}/{rel}")
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

    // consumer の plugin は root の `consumer/` へ写る（器の plugin は `<NAME>/`・root の
    // 形は `pipe_spawn_plugin_` の歯が測る）。
    let consumer = plugin.join(CONSUMER);
    for (dir, name, body) in [
        (".claude-plugin", "plugin.json", PLUGIN_JSON),
        ("hooks", "hooks.json", HOOKS_JSON),
    ] {
        let source = fs::read(worktree.join(PLUGIN_DIR).join(dir).join(name)).expect("worktree 側を読める");
        let copied = fs::read(consumer.join(dir).join(name)).expect("写しを読める");
        assert_eq!(copied, source, "{dir}/{name} の bytes が worktree と一致する");
        assert_eq!(copied, body.as_bytes(), "{dir}/{name} は toy repo に置いた本文");
    }
    // 写し元は **worktree**（便の base）であって anchor の現在値ではない。
    assert_ne!(
        fs::read(consumer.join(".claude-plugin").join("plugin.json")).expect("写しを読める"),
        PLUGIN_JSON_DIRTY.as_bytes(),
        "anchor の未 commit な plugin.json を載せない"
    );
    // 内側の entry の symlink は写さない（`hooks/outside.json` は repo の README を指す）。
    assert_eq!(
        dir_names(&consumer.join("hooks")),
        vec!["hooks.json".to_owned()],
        "plugin dir の中の symlink を写さない"
    );

    assert!(!consumer.join("README.md").exists(), "写しに worktree の README を入れない");
    assert!(!consumer.join("src").exists(), "写しに worktree の src を入れない");
    let names = dir_names(&consumer);
    assert_eq!(
        names,
        vec![".claude-plugin".to_owned(), "hooks".to_owned(), runner_mark()],
        "写しは plugin の 2 dir と印だけ（母集団 {} entry）",
        names.len()
    );
    // 古い写し（`stale.json`）は root を先に空にしたので残らない。
    let roots = dir_names(&plugin);
    assert_eq!(
        roots,
        vec![CONSUMER.to_owned(), NAME.to_owned()],
        "root は consumer と器の 2 本だけ（母集団 {} entry）",
        roots.len()
    );
    clean(&[&repo, &state]);
}

/// plugin の dir **自体が symlink** の repo では、その dir を写さない。
///
/// `Path::is_dir()` は link を辿るので、判定を `symlink_metadata` にしないと link 先の
/// 木を丸ごと写す（`hooks -> ../..` なら worktree 全体が写しに混ざる）。
#[test]
fn pipe_spawn_skips_plugin_dir_that_is_a_symlink() {
    let (repo, state) = repo_with_state();
    let payload = repo.join(PLUGIN_DIR);
    fs::create_dir_all(payload.join(".claude-plugin")).expect(".claude-plugin を作れる");
    fs::write(payload.join(".claude-plugin").join("plugin.json"), PLUGIN_JSON)
        .expect("plugin.json を書ける");
    fs::create_dir_all(payload.join("real-hooks")).expect("real-hooks を作れる");
    fs::write(payload.join("real-hooks").join("hooks.json"), HOOKS_JSON).expect("hooks.json を書ける");
    std::os::unix::fs::symlink("real-hooks", payload.join("hooks")).expect("hooks を link にできる");
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
    // `hooks` が link の周は `hooks/hooks.json` を link を辿らずに持たない＝consumer の plugin と
    // 見ない（片方だけの周と同じ）。link 先の木は 1 file も写らず、root は器の 1 本だけ。
    let names = dir_names(&plugin);
    assert_eq!(
        names,
        vec![NAME.to_owned()],
        "dir 自体が symlink の面は写さない（母集団 {} entry）",
        names.len()
    );
    clean(&[&repo, &state]);
}

/// consumer の plugin を写す root 配下の subdir 名（設計 §5.2 手順 5 (ii)）。
const CONSUMER: &str = "consumer";

/// repo tracked の器の plugin（`gen-manifest` の生成物＝埋め込みの正本）を読む。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn tracked_plugin(dir: &str, name: &str) -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    fs::read(root.join(PLUGIN_DIR).join(dir).join(name)).expect("tracked の器の plugin を読める")
}

/// root の `<NAME>/` に器の plugin が **tracked と同じ bytes** で在ることを測る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn assert_vessel_plugin(plugin: &Path) {
    let vessel = plugin.join(NAME);
    assert_eq!(
        dir_names(&vessel),
        vec![".claude-plugin".to_owned(), "hooks".to_owned()],
        "器の plugin は 2 dir"
    );
    for (dir, name) in [(".claude-plugin", "plugin.json"), ("hooks", "hooks.json")] {
        let written = fs::read(vessel.join(dir).join(name)).expect("器の plugin を読める");
        assert_eq!(written, tracked_plugin(dir, name), "{dir}/{name} は tracked の生成物と同じ bytes");
    }
}

/// 与えた file（repo 相対 path と本文）を置いて commit した toy repo と置き場を作る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn repo_with_files(files: &[(String, &str)]) -> (PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    for (rel, body) in files {
        let path = repo.join(rel);
        fs::create_dir_all(path.parent().unwrap_or(&repo)).expect("親 dir を作れる");
        fs::write(&path, body).expect("file を書ける");
    }
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "fixture"]);
    (repo, state)
}

/// intake 済みの便を `{plugin_dir}` を写す runner で spawn し、plugin root と runner が受けた値を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn spawn_showing_plugin_dir(repo: &Path, state: &Path, id: &str) -> (PathBuf, String) {
    let out = run_pipe(&[
        "spawn", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", "printf '%s' {plugin_dir} > plugin_dir.txt && git add -A && git commit -q -m runner",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));
    let worktree = repo.join(".worktrees").join(NAME).join(id);
    let shown = fs::read_to_string(worktree.join("plugin_dir.txt")).expect("置換の写しを読める");
    (state.join("pipe").join(id).join("plugin"), shown)
}

/// toy repo を intake して [`spawn_showing_plugin_dir`] で撃つ。
fn spawn_plugin_run(repo: &Path, state: &Path) -> (PathBuf, String) {
    let path = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &path);
    spawn_showing_plugin_dir(repo, state, &id)
}

/// **plugin を持たない repo でも器の plugin が root に載る**（`s2-07l.149` 裁定 (A)・FR20・憲法 C16.2）。
/// base は空 dir を作るだけだった＝consumer repo の便は in-loop guard 0 本で走っていた。
#[test]
fn pipe_spawn_plugin_embeds_vessel_plugin_for_repo_without_plugin() {
    let (repo, state) = repo_with_state();
    let (plugin, shown) = spawn_plugin_run(&repo, &state);
    assert!(
        plugin.join(NAME).join("hooks").join("hooks.json").is_file(),
        "器の hooks.json が root の <NAME>/ に在る: {}",
        plugin.display()
    );
    assert_vessel_plugin(&plugin);
    assert!(!plugin.join(CONSUMER).exists(), "plugin を持たない repo に consumer/ を作らない");
    assert_eq!(dir_names(&plugin), vec![NAME.to_owned()], "root は器の 1 本だけ");
    // `{plugin_dir}` は root（配下の展開は runner が行う・seam は不変）。
    assert_eq!(shown, plugin.display().to_string(), "{{plugin_dir}} は plugin root");
    clean(&[&repo, &state]);
}

/// 別名の plugin を持つ repo では consumer の写しと器の plugin が**並んで**載る。
#[test]
fn pipe_spawn_plugin_puts_consumer_beside_vessel_plugin() {
    let (repo, state) = repo_with_plugin();
    let (plugin, shown) = spawn_plugin_run(&repo, &state);
    assert_eq!(dir_names(&plugin), vec![CONSUMER.to_owned(), NAME.to_owned()], "root は consumer と器");
    assert_vessel_plugin(&plugin);
    assert_eq!(
        fs::read(plugin.join(CONSUMER).join("hooks").join("hooks.json")).expect("consumer の写しを読める"),
        HOOKS_JSON.as_bytes(),
        "consumer の hooks.json は repo の本文"
    );
    assert_eq!(
        dir_names(&plugin.join(CONSUMER).join("hooks")),
        vec!["hooks.json".to_owned()],
        "consumer の中の symlink は写さない"
    );
    assert_eq!(shown, plugin.display().to_string(), "{{plugin_dir}} は plugin root");
    clean(&[&repo, &state]);
}

/// plugin.json の `name` が器と同じ repo（＝器自身の repo）は、version や hooks.json が違っても
/// consumer と見ない＝器の 1 本だけ（同じ hook を 2 度走らせない）。載るのは埋め込みの bytes。
#[test]
fn pipe_spawn_plugin_skips_consumer_named_like_vessel() {
    let manifest = format!("{{\n  \"name\": \"{NAME}\",\n  \"version\": \"9.9.9\"\n}}\n");
    let (repo, state) = repo_with_files(&[
        (payload_rel(".claude-plugin/plugin.json"), &manifest),
        (payload_rel("hooks/hooks.json"), HOOKS_JSON),
    ]);
    let (plugin, _) = spawn_plugin_run(&repo, &state);
    assert!(!plugin.join(CONSUMER).exists(), "器と同名の plugin を consumer として写さない");
    assert_eq!(dir_names(&plugin), vec![NAME.to_owned()], "root は器の 1 本だけ");
    assert_vessel_plugin(&plugin);
    clean(&[&repo, &state]);
}

/// 片方だけ在る repo・`name` を top-level の文字列で読めない repo は consumer の plugin と見ない
/// （写さない・空 dir も作らない）。
#[test]
fn pipe_spawn_plugin_skips_half_or_nameless_consumer() {
    let (manifest, hooks) = (payload_rel(".claude-plugin/plugin.json"), payload_rel("hooks/hooks.json"));
    let cases: [(&str, Vec<(String, &str)>); 4] = [
        ("hooks だけ", vec![(hooks.clone(), HOOKS_JSON)]),
        ("plugin.json だけ", vec![(manifest.clone(), PLUGIN_JSON)]),
        (
            "name が入れ子にだけ在る",
            vec![(manifest.clone(), "{\"meta\":{\"name\":\"toy-plugin\"}}\n"), (hooks.clone(), HOOKS_JSON)],
        ),
        ("name が文字列でない", vec![(manifest, "{\"name\":7}\n"), (hooks, HOOKS_JSON)]),
    ];
    for (label, files) in cases {
        let (repo, state) = repo_with_files(&files);
        let (plugin, _) = spawn_plugin_run(&repo, &state);
        assert!(!plugin.join(CONSUMER).exists(), "{label}: consumer/ を作らない");
        assert_eq!(dir_names(&plugin), vec![NAME.to_owned()], "{label}: root は器の 1 本だけ");
        clean(&[&repo, &state]);
    }
}

/// (d・形 4) 別名の plugin は生成 dir（[`PLUGIN_DIR`]）の下から写り、root 直下の旧 path の本文は写しに載らない。旧 path にしか
/// plugin を持たない worktree は consumer と見ない（**否定の枝**）。どちらの周も run dir の写しに器の 2 file が
/// tracked の生成物（生成 dir の下）と同じ bytes で在り、写し先は `consumer/` の直下（生成 dir を挟まない）。
#[test]
fn plugin_payload_consumer_is_read_from_the_dir_and_old_path_is_not_a_consumer() {
    let old = "{\"name\":\"old-path-plugin\"}\n";
    let (repo, state) = repo_with_files(&[
        (payload_rel(".claude-plugin/plugin.json"), PLUGIN_JSON),
        (payload_rel("hooks/hooks.json"), HOOKS_JSON),
        (".claude-plugin/plugin.json".to_owned(), old),
        ("hooks/hooks.json".to_owned(), old),
    ]);
    let (plugin, _) = spawn_plugin_run(&repo, &state);
    assert_eq!(dir_names(&plugin), vec![CONSUMER.to_owned(), NAME.to_owned()], "root は consumer と器");
    assert_vessel_plugin(&plugin);
    let consumer = plugin.join(CONSUMER);
    assert_eq!(dir_names(&consumer), vec![".claude-plugin".to_owned(), "hooks".to_owned(), runner_mark()], "写し先は consumer/ の直下");
    for (dir, name, body) in [(".claude-plugin", "plugin.json", PLUGIN_JSON), ("hooks", "hooks.json", HOOKS_JSON)] {
        let copied = fs::read(consumer.join(dir).join(name)).unwrap_or_default();
        assert_eq!(copied, body.as_bytes(), "{dir}/{name} は生成 dir の下の本文（旧 path の本文ではない）");
    }
    clean(&[&repo, &state]);

    let (repo, state) = repo_with_files(&[
        (".claude-plugin/plugin.json".to_owned(), PLUGIN_JSON),
        ("hooks/hooks.json".to_owned(), HOOKS_JSON),
    ]);
    let (plugin, _) = spawn_plugin_run(&repo, &state);
    assert!(!plugin.join(CONSUMER).exists(), "旧 path にしか plugin を持たない worktree は consumer と見ない");
    assert_eq!(dir_names(&plugin), vec![NAME.to_owned()], "root は器の 1 本だけ");
    assert_vessel_plugin(&plugin);
    clean(&[&repo, &state]);
}

/// 再走で前の周の `consumer/` が残らない（root を先に空にする）。
#[test]
fn pipe_spawn_plugin_rerun_drops_stale_consumer() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let stale = state.join("pipe").join(&id).join("plugin").join(CONSUMER).join("hooks");
    fs::create_dir_all(&stale).expect("古い consumer の dir を作れる");
    fs::write(stale.join("hooks.json"), HOOKS_JSON).expect("古い consumer の hooks を置ける");
    let (plugin, _) = spawn_showing_plugin_dir(&repo, &state, &id);
    assert!(!plugin.join(CONSUMER).exists(), "古い consumer/ を残さない");
    assert_eq!(dir_names(&plugin), vec![NAME.to_owned()], "root は器の 1 本だけ");
    clean(&[&repo, &state]);
}

/// runner の写しの印の名（[`NAME`] + `-runner`・consumer-sync.md §19 形 3・ADR-0082）。src の const を引かず、跨版の名を
/// 歯が独立に pin する。
fn runner_mark() -> String {
    format!("{NAME}-runner")
}

/// (b・形 3 / 形 4・consumer-sync.md §19) 名の違う plugin を持つ repo の便は、写しの `consumer/` の直下に印の名の空の file
/// （link でない）を持ち、器の plugin の写しは tracked と同じ 2 dir のまま、anchor の下の生成 dir と便の worktree の生成 dir には
/// 印が無い（器が書かない）。続けて撃つ gate の lens の行は `{worktree}` だけが埋まり、`{plugin_dir}` は置換されない字面のまま
/// （lens は plugin の root を受けない）。base は印が無い＝最初の assert で RED。
#[test]
fn runner_mark_is_only_in_the_consumer_copy_and_lens_gets_no_plugin_root() {
    let (repo, state) = repo_with_plugin();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    let plugin = state.join("pipe").join(&id).join("plugin");
    let mark = plugin.join(CONSUMER).join(runner_mark());
    let meta = fs::symlink_metadata(&mark);
    assert!(meta.as_ref().is_ok_and(fs::Metadata::is_file), "consumer の直下に印の file が link でなく在る: {}", mark.display());
    assert_eq!(meta.map(|meta| meta.len()).ok(), Some(0), "印の中身は空");
    assert_vessel_plugin(&plugin);
    for payload in [repo.join(PLUGIN_DIR), worktree_of(&repo, &id).join(PLUGIN_DIR)] {
        // 母集団: 生成 dir に plugin が在る（dir が無いと「印が無い」が空虚に通る）。
        assert!(payload.join(".claude-plugin").join("plugin.json").is_file(), "生成 dir に plugin が在る: {}", payload.display());
        let names = dir_names(&payload);
        assert!(fs::symlink_metadata(payload.join(runner_mark())).is_err(), "器は生成 dir に印を書かない: {names:?}");
    }
    let seen = state.join("lens-holes");
    let lens = format!("cat >/dev/null; printf '%s\\n%s\\n' '{{worktree}}' '{{plugin_dir}}' > '{}'; echo '{}'", seen.display(), lens_verdict("PASS"));
    let out = run_pipe(&[
        "gate", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&out));
    let holes = fs::read_to_string(&seen).expect("lens の穴の写しを読める");
    let lines: Vec<&str> = holes.lines().collect();
    assert!(lines.first().is_some_and(|line| line.ends_with(id.as_str())), "{{worktree}} は便の id で終わる: {holes}");
    assert_eq!(lines.get(1).copied(), Some("{plugin_dir}"), "{{plugin_dir}} は lens の行で置換されない: {holes}");
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
fn pipe_e2e_toy_repo_lands_one_bead_with_fake_runner() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    // intake → spawn → gate → land を **1 process で人手 0** で通す。
    let out = run_pipe(&[
        "run", "--design", &path, "--bead", "s2-41o",
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
    // 着地の 1 件と、remote を持たない toy の終端が台帳を閉じた `terminal:close:ok` の 1 件（着地の後ろ）。
    assert_eq!(landed_events, 2, "RunDone stage=Landed が 2 件（着地 + 終端の close）: {log}");
    clean(&[&repo, &state]);
}

// ───── process を殺してから別 process で resume する（`s2-07l.203`・SRS AC4・接頭辞 `pipe_resume_kill_`） ─────
//
// 現行の resume の歯は**行儀よく終わった spawn の後**から引く。ここは `pipe run` を子 process として起こし、
// `RunStage stage=Implemented` が event log に現れた時点で **process group ごと SIGKILL** し、殺した周が
// 残す物（外されなかった lock・生き残った孫・途中の worktree）の上を別 process の resume が Landed まで
// 通るかを測る（成立を宣言する歯ではなく、成立するかを測る歯）。
//
// **自分が起こした子の process group にしか実 signal を送らない**。撃つ前に pid ≠ 0 / 1 ∧ 子が自分の
// group の leader ∧ 自分の group ではないことを assert し、落ちた周は撃たずに test を落とす。

/// diff を読み切って marker を置き、`sleep` を背景に起こして pid を書き、前景で待ち続ける fake lens
/// （`pipe_stop_group_*` の runner と同型）。gate は lens の stdout の EOF を待つので、この lens が
/// 生きている間 `pipe run` は gate の途中に留まる＝殺す窓を作る。
///
/// 同じ `--lens` は intake 直後の**審査の段**（FR49）にも 1 回撃たれる。その 1 回目（marker が無い周）は
/// 偽 PASS を返して便を spawn へ進め、2 回目（gate）だけが塞ぐ＝審査で塞ぐと `Implemented` に届かない。
fn blocking_lens(marker: &Path, pid_file: &Path) -> String {
    format!(
        "cat >/dev/null; if [ -e '{marker}' ]; then sleep 300 & echo $! > '{pid}'; wait; else touch '{marker}'; echo '{pass}'; fi",
        marker = marker.display(),
        pid = pid_file.display(),
        pass = lens_verdict("PASS"),
    )
}

/// `pipe run` を **自分の process group の leader** として起こす（intake → spawn → gate → land の 1 process）。
///
/// stdin / stdout / stderr は `Stdio::null()`＝読まない pipe で子を詰まらせない（殺す便は stdout を出さない
/// ので run id は event log から取る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn spawn_run_child(repo: &Path, state: &Path, design: &str, runner: &str, lens: &str) -> Child {
    use std::os::unix::process::CommandExt;
    bin_cmd()
        .args([
            "pipe", "run", "--design", design, "--bead", "s2-kill",
            "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
            "--rules", &ceiling_rules(state), "--runner", runner, "--lens", lens,
        ])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("binary を起動できる")
}

/// `events.jsonl` に `RunStage stage=Implemented` が現れるまで 20ms 間隔で読み、その行の run id を返す。
///
/// 上限 60s。その間 `child` が終わっていないことを毎周 assert する（終わっていれば Implemented の
/// 前に落ちた便＝殺す前提を作れていない）。読めない行は飛ばす（書きかけの末尾行で panic しない）。
fn wait_for_implemented(state: &Path, child: &mut Child) -> String {
    let begun = Instant::now();
    loop {
        let found = fs::read_to_string(state.join("fleet").join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| Event::from_line(line).ok())
            .find(|event| event.kind == EventKind::RunStage && event.stage == Some(Stage::Implemented))
            .map(|event| event.run);
        if let Some(id) = found {
            return id;
        }
        assert!(child.try_wait().ok().flatten().is_none(), "pipe run が Implemented の前に終わった");
        assert!(begun.elapsed() < Duration::from_secs(60), "Implemented にならない");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 前提を assert してから **`pid` の process group 1 つだけ**へ SIGKILL を撃つ。
///
/// 前提 = `pid` が 0 / 1 でない ∧ `pid` が自分の group の leader（`pgid == pid`）∧ その group が
/// この test の group でない。1 つでも落ちた周は撃たずに test を落とす（`-1` に化ける形を塞ぐ）。
/// shell を経由せず `kill` の argv へ直に渡す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn kill_group(pid: u32) {
    assert!(pid > 1, "前提: 自分の子の pid は 0 でも 1 でもない: {pid}");
    assert_eq!(proc_pgid(pid), Some(pid), "前提: {pid} は自分の group の leader");
    assert_ne!(proc_pgid(std::process::id()), Some(pid), "前提: 自分の group には撃たない");
    let target = format!("-{pid}");
    let out = Command::new("kill").args(["-KILL", "--", &target]).output().expect("kill を撃てる");
    assert!(out.status.success(), "group {pid} へ撃てる: {}", String::from_utf8_lossy(&out.stderr));
}

/// 便を `pipe run` で起こし、`Implemented` が記帳された時点で process group ごと殺す。
/// 返すのは（便 id・殺した `pipe run` の pid）。group から漏れた lens の `sleep` は [`reap_own`] で片付ける。
fn killed_at_implemented(repo: &Path, state: &Path, design: &str) -> (String, u32) {
    let marker = state.join("lens-ran");
    let pid_file = state.join("lens-sleep.pid");
    let lens = blocking_lens(&marker, &pid_file);
    let mut child = spawn_run_child(repo, state, design, TOY_COMMIT, &lens);
    let pid = child.id();
    let id = wait_for_implemented(state, &mut child);
    kill_group(pid);
    child.wait().ok();
    if let Some(sleeper) = fs::read_to_string(&pid_file).ok().and_then(|text| text.trim().parse::<u32>().ok()) {
        reap_own(sleeper);
    }
    assert!(!proc_alive(pid), "殺した pipe run {pid} は消えている");
    (id, pid)
}

/// 便の `RunDone` のうち段が `stage` の件数（`pipe_e2e_` の歯と同じ「1 便が最後まで載った」の測り方）。
fn done_count(state: &Path, id: &str, stage: Stage) -> usize {
    events(state)
        .iter()
        .filter(|found| found.run == id && found.kind == EventKind::RunDone && found.stage == Some(stage))
        .count()
}

// ───── runner が死んだ便の起こし直し（`s2-07l.323`・設計 account-autonomy.md §4・SRS FR37 / AC39・接頭辞 `pipe_resume_kill_at_spawned_`） ─────
//
// `Spawned` の段で runner の process が消えた便（host の再起動・OOM・kill で `SeatStopped` が書かれないまま死んだ形）に
// `resume --runner` を撃つ。生死は唯一の wait（`Completion::SeatGone`）で測り、死んでいれば `SeatStopped detail=runner-dead`
// を記帳して**同じ worktree** で起こし直す（途中再開の節に未 commit の一覧）。生きていれば typed に断って runner を
// 2 本にしない。実 signal は自分が起こした子の group にだけ送る（上の `kill_group` の前提 assert のまま）。

/// 前の turn が worktree に残す未 commit の file（契約の write-set に `+` で宣言する新規 file）。
const WIP_FILE: &str = "src/wip.rs";

/// turn 1 つ分の本文: write-set の file を 1 つ書き（**commit しない**）、`sleep` を背景に起こして pid を書き、前景で
/// 待つ（作業の途中で止まっている runner＝殺す窓・`blocking_lens` と同型）。pid file は「file を書き終えた」印でもある。
fn wip_then_wait_turn(pid_file: &Path) -> String {
    format!(
        "printf 'wip\\n' > {WIP_FILE}\nsleep 300 </dev/null >/dev/null 2>&1 &\necho $! > '{}'\nwait\n",
        pid_file.display()
    )
}

/// `SeatSpawned` と turn 1 の pid file が揃うまで 20ms 間隔で読む（上限 60s・その間 `child` が終わっていないことを
/// 毎周 assert・読めない行は飛ばす＝[`wait_for_implemented`] と同型）。返すのは（便 id・`SeatSpawned` の pid〔runner の
/// group leader〕・turn 1 の `sleep` の pid）。
fn wait_for_seat_spawned(state: &Path, child: &mut Child, pid_file: &Path) -> (String, u32, u32) {
    let begun = Instant::now();
    loop {
        let seated = fs::read_to_string(state.join("fleet").join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| Event::from_line(line).ok())
            .find(|event| event.kind == EventKind::SeatSpawned)
            .and_then(|event| Some((event.run, u32::try_from(event.pid?).ok()?)));
        let sleeper = fs::read_to_string(pid_file).ok().and_then(|text| text.trim().parse::<u32>().ok());
        if let (Some((id, pid)), Some(sleeper)) = (seated, sleeper) {
            return (id, pid, sleeper);
        }
        assert!(child.try_wait().ok().flatten().is_none(), "pipe run が SeatSpawned の前に終わった");
        assert!(begun.elapsed() < Duration::from_secs(60), "SeatSpawned にならない");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `/proc/<pid>` が消えるまで待つ（上限 10s・親を殺した後の zombie が init に回収されるのを待つ形）。
fn wait_gone(pid: u32) {
    let begun = Instant::now();
    while proc_alive(pid) {
        assert!(begun.elapsed() < Duration::from_secs(10), "pid {pid} が消えない");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 未 commit の file を書いて待つ runner で `pipe run` を子として起こし、`SeatSpawned` が出た時点で **`pipe run` の group →
/// runner の group** の順に SIGKILL する（host の再起動で両方が消えた形）。`pipe run` を先に殺すのは、runner が先に
/// 消えると生きている `pipe run` がその終了を見届けて `SeatStopped` と `Failed` を記帳し、作りたい「`SeatStopped` の無い
/// `Spawned`」にならないため。返すのは（便 id・runner の pid・turn 2 以降の本文を持つ runner cmd）。
pub(super) fn killed_at_spawned(repo: &Path, state: &Path, rest: &[String]) -> (String, u32, String) {
    let contract = write_set_contract(repo, "wip.toml", &["src/lib.rs", &format!("+{WIP_FILE}")]);
    let pid_file = state.join("wip-sleep.pid");
    let mut turns = vec![wip_then_wait_turn(&pid_file)];
    turns.extend(rest.iter().cloned());
    let runner = turn_runner(state, &turns);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let mut child = spawn_run_child(repo, state, &contract, &runner, &lens);
    let (id, runner_pid, sleeper) = wait_for_seat_spawned(state, &mut child, &pid_file);
    kill_group(child.id());
    child.wait().ok();
    kill_group(runner_pid);
    reap_own(sleeper);
    wait_gone(runner_pid);
    assert_eq!(kind_count(state, &id, EventKind::SeatStopped), 0, "前提: SeatStopped の無い Spawned");
    assert!(show_line(repo, state, &id).contains("stage=Spawned"), "前提: 段は Spawned");
    assert!(worktree_of(repo, &id).join(WIP_FILE).exists(), "前提: 未 commit の file が worktree に在る");
    (id, runner_pid, runner)
}

/// 便の `SeatStopped` のうち detail が `runner-dead` の件数。
fn runner_dead_count(state: &Path, id: &str) -> usize {
    events(state)
        .iter()
        .filter(|found| {
            found.run == id && found.kind == EventKind::SeatStopped && found.detail.as_deref() == Some("runner-dead")
        })
        .count()
}

/// runner が死んだ便に口座 a1 の置き場で `resume --runner` を撃つ（計測は偽 curl・選定は §3 の便用の規則）。
fn resume_dead_runner(repo: &Path, state: &Path, id: &str, runner: &str) -> Output {
    let rules = resume_rules(state, &["a1"]);
    put_account(state, "a1", &[windows(30, 30)]);
    run_pipe(&[
        "resume", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
        "--rules", &rules, "--curl", &fake_usage_curl(state),
    ])
}

// ───── 終わりの門の間の印の読み手（設計 pipeline.md §66 形 9・行 bk・接頭辞 `endgate_mark_`） ─────
//
// 書き手（行 bj）が着地するまで印を置く者は居ないので、歯は runner が死んだ `Spawned` の便の run dir へ印を手で置く。

/// 便の run dir の印の path。
fn end_gate_mark_path(state: &Path, id: &str) -> PathBuf {
    state.join("pipe").join(id).join("end-gate.pid")
}

/// 便の event の本数（kind を問わない・「event 0 件」の測り）。
fn event_total(state: &Path, id: &str) -> usize {
    events(state).iter().filter(|found| found.run == id).count()
}

/// 印を置いて `resume --runner` を撃った周（出力・撃つ前の event と `SeatSpawned` の本数・便の材料）。
struct Marked {
    out: Output,
    events_before: usize,
    seats_before: usize,
    id: String,
    repo: PathBuf,
    state: PathBuf,
}

/// 印の本文 `body` を置いて `resume --runner` を撃つ。
fn resume_with_mark(body: &str) -> Marked {
    let (repo, state) = repo_with_state();
    let (id, _runner_pid, runner) = killed_at_spawned(&repo, &state, &[IMPLEMENT.to_owned()]);
    fs::write(end_gate_mark_path(&state, &id), body).unwrap_or_default();
    let events_before = event_total(&state, &id);
    let seats_before = kind_count(&state, &id, EventKind::SeatSpawned);
    let out = resume_dead_runner(&repo, &state, &id, &runner);
    Marked { out, events_before, seats_before, id, repo, state }
}

/// (a) 生きている process の pid を本文にした印の便は、rc 1・判定行 `end-gate=alive pid=<pid>`・event 0 件・runner 起こさず。
#[test]
fn endgate_mark_alive_owner_refuses_without_waking_a_runner() {
    let mut sleeper = Command::new("sleep").arg("300").spawn().unwrap_or_else(|err| panic!("sleep を起こせる: {err}"));
    let pid = sleeper.id();
    let Marked { out, events_before, seats_before, id, repo, state } = resume_with_mark(&format!("{pid}\n"));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={id} end-gate=alive pid={pid}")), "{}", stdout_of(&out));
    assert_eq!(event_total(&state, &id), events_before, "event を足さない");
    assert_eq!(runner_dead_count(&state, &id), 0, "SeatStopped runner-dead を足さない");
    assert_eq!(kind_count(&state, &id, EventKind::SeatSpawned), seats_before, "SeatSpawned を足さない");
    assert_eq!(stub_calls(&state), 1, "偽 runner は起きない（turn 1 の 1 回のまま）");
    sleeper.kill().ok();
    sleeper.wait().ok();
    clean(&[&repo, &state]);
}

/// (b) 読めない本文（10 進でない字）の印の便も、rc 1・判定行 `end-gate=unreadable`・event 0 件・runner 起こさず。
#[test]
fn endgate_mark_unreadable_body_refuses_without_waking_a_runner() {
    let Marked { out, events_before, seats_before, id, repo, state } = resume_with_mark("not-a-pid\n");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={id} end-gate=unreadable")), "{}", stdout_of(&out));
    assert_eq!(event_total(&state, &id), events_before, "event を足さない");
    assert_eq!(kind_count(&state, &id, EventKind::SeatSpawned), seats_before, "SeatSpawned を足さない");
    assert_eq!(stub_calls(&state), 1, "偽 runner は起きない");
    clean(&[&repo, &state]);
}

/// (c) 持ち主の死んだ印の便と印の無い便の resume は、今どおり `runner-dead` を 1 件記帳して runner を起こし直す。
#[test]
fn endgate_mark_dead_owner_and_absent_mark_resume_as_before() {
    let mut gone = Command::new("true").spawn().unwrap_or_else(|err| panic!("true を起こせる: {err}"));
    let pid = gone.id();
    gone.wait().ok();
    for body in [Some(format!("{pid}\n")), None] {
        let (repo, state) = repo_with_state();
        let (id, _runner_pid, runner) = killed_at_spawned(&repo, &state, &[IMPLEMENT.to_owned()]);
        if let Some(body) = body {
            fs::write(end_gate_mark_path(&state, &id), body).unwrap_or_default();
        }
        let out = resume_dead_runner(&repo, &state, &id, &runner);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
        assert_eq!(runner_dead_count(&state, &id), 1, "runner-dead を 1 件記帳する");
        assert_eq!(stub_calls(&state), 2, "runner を起こし直す");
        clean(&[&repo, &state]);
    }
}

// ───── API に届かず止まった runner（`s2-07l.301`・設計 account-autonomy.md §17・SRS FR37 / FR14・接頭辞 `pipe_unreachable_`） ─────
//
// turn 1 は**実 runner**（`<bin> runner`）を偽 claude で撃つ: 偽 claude が `result` record に `is_error` と本文を書いて rc 1 で
// 終わる。runner の弁別（到達不能の閉じた語の集合）が rc を決め、pipe の spawn はその rc だけで段を分ける。

/// 実測の到達不能の本文（host のネット断 2026-09-14 の runner の `result`）。
const UNREACHABLE_TEXT: &str = "API Error: Can't reach the API server (EAI_AGAIN)";

/// 偽 claude（実行 file）: stdin を読み捨て、`is_error` と本文 `text` の `result` record を 1 行書いて rc 1 で終わる。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn result_claude(state: &Path, is_error: bool, text: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let record = format!("{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":{is_error},\"result\":\"{text}\"}}");
    let path = state.join("result-claude");
    fs::write(&path, format!("#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{}'\nexit 1\n", record.replace('\'', "'\\''")))
        .expect("偽 claude を書ける");
    let mut perm = fs::metadata(&path).expect("偽 claude の権限を読める").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("偽 claude を実行可能にできる");
    path
}

/// turn 1 つ分の本文: commit を 1 本作ってから、実 runner を偽 claude `claude` で撃ち、その rc で終わる（stdin は
/// [`turn_runner`] が写した `stdin-<n>`・位置引数は [`unreachable_runner`] が渡す placeholder の 4 つ）。
fn commit_then_real_runner_turn(claude: &Path) -> String {
    format!(
        "printf 'y\\n' >> src/lib.rs\ngit add -A\ngit commit -q -m partial-work\n\
         '{}' runner --worktree \"$1\" --write-set \"$2\" --vessel \"$3\" --plugin-dir \"$4\" \
         --permission-mode acceptEdits --claude '{}' < \"$D/stdin-$N\"\nexit $?",
        bin(),
        claude.display()
    )
}

/// turn 1 = commit + 実 runner（偽 claude の本文は `is_error` / `text`）・turn 2 = [`IMPLEMENT`] の runner cmd。
fn unreachable_runner(state: &Path, is_error: bool, text: &str) -> String {
    let claude = result_claude(state, is_error, text);
    let turns = [commit_then_real_runner_turn(&claude), IMPLEMENT.to_owned()];
    format!("{} {{worktree}} {{write_set}} {{vessel}} {{plugin_dir}}", turn_runner(state, &turns))
}

/// write-set `src/lib.rs` の便を intake → [`unreachable_runner`] で spawn する。返すのは（便 id・runner cmd・spawn の出力）。
fn spawn_real_runner(repo: &Path, state: &Path, is_error: bool, text: &str) -> (String, String, Output) {
    let contract = write_set_contract(repo, "net", &["src/lib.rs"]);
    let id = intake_bead(repo, state, &contract, "s2-net");
    let runner = unreachable_runner(state, is_error, text);
    let out = spawn_with(repo, state, &id, &runner);
    (id, runner, out)
}

/// 便の `SeatStopped` のうち detail が `runner-unreachable` の件数。
fn unreachable_stop_count(state: &Path, id: &str) -> usize {
    events(state)
        .iter()
        .filter(|found| {
            found.run == id && found.kind == EventKind::SeatStopped && found.detail.as_deref() == Some("runner-unreachable")
        })
        .count()
}

/// (a) 到達不能の本文で終わった runner の便は `Spawned` のまま `SeatStopped detail=runner-unreachable` が 1 件・`Failed` は
/// 0・worktree の commit が残る。spawn は rc 3（続きは resume）で、判定行に `halt=unreachable`。
#[test]
fn pipe_unreachable_spawn_keeps_the_run_spawned_with_one_seat_stop() {
    let (repo, state) = repo_with_state();
    let (id, _runner, out) = spawn_real_runner(&repo, &state, true, UNREACHABLE_TEXT);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stdout_of(&out).contains(&format!("run={id} stage=Spawned halt=unreachable")), "{}", stdout_of(&out));
    assert_eq!(stub_calls(&state), 1, "runner は 1 回だけ起きる（連鎖が起こし直さない）");
    assert_eq!(unreachable_stop_count(&state, &id), 1, "到達不能の SeatStopped を 1 件");
    assert_eq!(kind_count(&state, &id, EventKind::SeatStopped), 1, "SeatStopped は理由つきの 1 件だけ");
    assert_eq!(stage_count(&state, &id, Stage::Failed), 0, "Failed に倒さない");
    assert!(show_line(&repo, &state, &id).contains("stage=Spawned"), "段は Spawned のまま live");
    let worktree = worktree_of(&repo, &id);
    assert_eq!(git(&worktree, &["rev-list", "--count", "refs/heads/main..HEAD"]), "1", "turn 1 の commit が残る");
    let kept = fs::read_to_string(state.join("pipe").join(&id).join("runner.stdout.log")).unwrap_or_default();
    assert!(kept.contains(&format!("runner: halt reason=unreachable text={UNREACHABLE_TEXT}")), "停止行: {kept}");
    clean(&[&repo, &state]);
}

/// (b) (a) の便に `resume --runner` を撃つと、生死の計測を飛ばして（`runner-dead` を記帳しない）runner を 1 回起こし直し、
/// `Spawned detail=account:a1,resume:unreachable` を記帳して同じ worktree で `Implemented` に至る。2 回目の stdin の
/// 「途中再開」節に理由の行（`SeatStopped detail=runner-unreachable` の ts）と turn 1 の commit が載る。
#[test]
fn pipe_unreachable_resume_skips_liveness_and_respawns_once() {
    let (repo, state) = repo_with_state();
    let (id, runner, _) = spawn_real_runner(&repo, &state, true, UNREACHABLE_TEXT);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let resumed = resume_dead_runner(&repo, &state, &id, &runner);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    assert!(stdout_of(&resumed).contains(&format!("run={id} stage=Implemented")), "{}", stdout_of(&resumed));
    assert_eq!(stub_calls(&state), 2, "runner を 1 回起こし直した");
    assert_eq!(runner_dead_count(&state, &id), 0, "生死を測らず runner-dead を二重に記帳しない");
    assert_eq!(unreachable_stop_count(&state, &id), 1, "到達不能の記帳は 1 件のまま");
    assert_eq!(
        spawned_details(&state, &id),
        vec![format!("base:{base}"), "account:a1,resume:unreachable".to_owned()],
        "起こし直しの記帳は理由を unreachable と名乗る"
    );
    let stopped_at = events(&state)
        .into_iter()
        .filter(|event| {
            event.run == id && event.kind == EventKind::SeatStopped && event.detail.as_deref() == Some("runner-unreachable")
        })
        .map(|event| event.ts)
        .next_back()
        .unwrap_or_default();
    let prompt = stub_stdin(&state, 2);
    let reason = format!("- 前の turn は {stopped_at} に API に届かず止まった（ネットの断）");
    assert!(!stopped_at.is_empty() && prompt.contains(&reason), "理由の行: {prompt}");
    assert!(prompt.contains("partial-work"), "turn 1 の commit が一覧に載る: {prompt}");
    let worktree = worktree_of(&repo, &id);
    assert_eq!(git(&worktree, &["rev-list", "--count", "refs/heads/main..HEAD"]), "2", "同じ worktree に 2 本目が載る");
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "Implemented に至る");
    clean(&[&repo, &state]);
}

/// (c) 集合に無い `is_error` の本文は従来どおり `Failed detail=runner-rc:1,commits:1`（理由つきの `SeatStopped` は 0）。
#[test]
fn pipe_unreachable_other_error_text_still_fails() {
    let (repo, state) = repo_with_state();
    let (id, _runner, out) = spawn_real_runner(&repo, &state, true, "API Error: 500 Internal server error");
    assert!(stdout_of(&out).contains("stage=Failed"), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(stages(&state, &id).pop(), Some((Some(Stage::Failed), Some("runner-rc:1,commits:1".to_owned()))));
    assert_eq!(unreachable_stop_count(&state, &id), 0, "到達不能と読まない");
    clean(&[&repo, &state]);
}

/// (d) `is_error=false` の本文に到達不能の語が在っても弁別しない（pure）＝`Failed detail=runner-rc:1,commits:1`。
#[test]
fn pipe_unreachable_is_error_false_is_not_discriminated() {
    let (repo, state) = repo_with_state();
    let (id, _runner, out) = spawn_real_runner(&repo, &state, false, UNREACHABLE_TEXT);
    assert!(stdout_of(&out).contains("stage=Failed"), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(stages(&state, &id).pop(), Some((Some(Stage::Failed), Some("runner-rc:1,commits:1".to_owned()))));
    assert_eq!(unreachable_stop_count(&state, &id), 0, "is_error=false は到達不能と読まない");
    clean(&[&repo, &state]);
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
    bin_cmd().args(&args).output().expect("binary を起動できる")
}

/// 3 クラスを名乗る契約で intake → spawn まで撃ち、Blocked で止まった便の
/// id と stdout を返す。stdout は「いまどの段に居るか」の主張なので測る対象である。
pub(super) fn blocked(repo: &Path, state: &Path, marker: &Path, classes: &str) -> (String, String) {
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

/// `fleet record` で人由来の event を 1 件積む（**`pipe approve` を通さない**）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn record_human_stage(state: &Path, id: &str) -> Output {
    bin_cmd()
        .args(["fleet", "record", "--state-dir"])
        .arg(state)
        .args([
            "--kind", "RunStage", "--actor", "human", "--run", id, "--bead", "s2-2e5",
            "--stage", "Intake", "--detail", "手で段を動かした",
        ])
        .output()
        .expect("binary を起動できる")
}

/// 審査 FAIL が 0 件の周の `pipe report` の末尾 2 token（`s2-07l.395`・設計 contract-source.md §22）: 母集団 0 でも
/// `by_kind=` は宣言順に 7 語とも 0 で出る。**字面で pin する**（器の const slice を写すと語の入れ替わりを見逃す）。
pub(super) const NO_REVIEW_FAIL: &str = "review_fail=0 by_kind=teeth-outside-write-set:0,goal-done-contradiction:0,\
                                          vacuous-assert:0,literal-mismatch:0,section-material-missing:0,other:0,unparsed:0";

/// `pipe report` の 1 行目（到達点の行）。2 行目は消費の行（`cost:`・設計 gate-cost.md §26 形 (2)）で、段の秒の和が
/// 周ごとに動くので到達点の歯は 1 行目だけを逐語で pin する（2 行目の形は [`report_cost`] と `run_cost_` の歯が測る）。
pub(super) fn report_head(out: &Output) -> String {
    stdout_of(out).lines().next().unwrap_or_default().to_owned()
}

/// `pipe report` の 2 行目（消費の行）。
pub(super) fn report_cost(out: &Output) -> String {
    stdout_of(out).lines().nth(1).unwrap_or_default().to_owned()
}

/// 便の `Reviewed` を機械の event として 1 件積む（**`kind:` を持たない古い形**の detail・`fleet record` 経由）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn record_legacy_reviewed(state: &Path, id: &str, detail: &str) {
    let out = bin_cmd()
        .args(["fleet", "record", "--state-dir"])
        .arg(state)
        .args(["--kind", "RunStage", "--run", id, "--bead", "s2-legacy", "--stage", "Reviewed", "--detail", detail])
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "record: {}", stderr_of(&out));
}

/// 歯 (3) **数える面**（設計 §22）: `review_fail=` は `RunStage stage=Reviewed` のうち verdict が PASS でないもの全部
/// （PASS は入らない・日付で絞らない）で、`by_kind=` は宣言順に 7 語とも出る（0 も出す）。`kind:` を持たない古い
/// event（FAIL / INCONCLUSIVE）は `unparsed` に数え、既存の 4 token は不変・rc 0。
#[test]
fn pipe_review_kind_report_counts_review_fail_by_kind_in_declaration_order() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let repo_arg = repo.display().to_string();
    let state_arg = state.display().to_string();
    let rules = ceiling_rules(&state);
    // FAIL の 3 便（literal-mismatch × 2・other × 1）と INCONCLUSIVE の 1 便（`--lens` 無し＝unparsed）。
    for (bead, kind) in [("s2-f1", "literal-mismatch"), ("s2-f2", "literal-mismatch"), ("s2-f3", "other")] {
        let line = format!("{{\"verdict\":\"FAIL\",\"evidence\":\"fake\",\"kind\":\"{kind}\",\"at\":\"src/lib.rs\"}}");
        let marker = state.join(format!("lens-ran-{bead}"));
        let out = run_pipe(&[
            "intake", "--design", &path, "--bead", bead, "--repo", &repo_arg, "--state-dir", &state_arg,
            "--rules", &rules, "--lens", &fake_lens(&marker, &line),
        ]);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{bead}: {}", stderr_of(&out));
    }
    let out = run_pipe(&["intake", "--design", &path, "--bead", "s2-i1", "--repo", &repo_arg, "--state-dir", &state_arg, "--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{}", stderr_of(&out));
    // PASS の便は intake の最後に置く（live で write-set を持つので、同じ契約の後続の intake が交差で断られる）。
    let marker = state.join("lens-ran-pass");
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "s2-p1", "--repo", &repo_arg, "--state-dir", &state_arg,
        "--rules", &rules, "--lens", &fake_lens(&marker, &lens_verdict("PASS")),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    // 古い形の event 2 件（FAIL / INCONCLUSIVE・`kind:` 無し）と PASS の古い形 1 件（母集団の外）。intake の後に
    // 積む（`review.json` の無い `Reviewed` の便は受付が live の write-set を読めず断るので、先に積むと intake が通らない）。
    record_legacy_reviewed(&state, "old-fail", "verdict:FAIL");
    record_legacy_reviewed(&state, "old-inc", "verdict:INCONCLUSIVE");
    record_legacy_reviewed(&state, "old-pass", "verdict:PASS");

    let report = report_once(&state);
    assert_eq!(report.status.code(), Some(i32::from(RC_OK)), "report は rc 0 のまま: {}", stderr_of(&report));
    let line = report_head(&report);
    // 母集団 = intake の 4 便（FAIL 3 + INCONCLUSIVE 1）+ 古い形の 2 件（PASS の 2 件は外）= 6。
    assert_eq!(
        line,
        "runs=8 landed=0 human_events=0 human_events_other_than_approval=0 review_fail=6 \
         by_kind=teeth-outside-write-set:0,goal-done-contradiction:0,vacuous-assert:0,literal-mismatch:2,\
         section-material-missing:0,other:1,unparsed:3",
        "既存の 4 token は不変・review_fail は母集団・by_kind は宣言順に 7 語とも（0 も）"
    );
    let by_kind = line.split_once("by_kind=").map(|(_, rest)| rest).unwrap_or_default();
    let words: Vec<&str> = by_kind.split(',').filter_map(|item| item.split_once(':')).map(|(word, _)| word).collect();
    let declared: Vec<&str> = vessel::pipe::review::FINDING_KINDS.iter().map(|kind| kind.as_str()).collect();
    assert_eq!(words, declared, "内訳の並びは器の宣言順そのもの");
    assert_eq!(words.len(), 7, "7 語とも出る");
    clean(&[&repo, &state]);
}

/// 要約行に消費の 3 語を足して終わる偽 runner（commit を 1 本作ってから、runner の要約行と同じ字面を最後に印字する）。
const COST_RUNNER: &str = "echo x >> src/lib.rs && git add -A && git commit -q -m runner && \
                           echo 'runner: rc=0 records=3 usage=in:11,out:22,cache_read:33,cache_create:44 turns=5 wall_ms=6000'";

/// 便ごとの token 消費の検出線の歯の偽 runner（commit を 1 本作り、要約行に 4 値がどれも 0 でない消費を載せる・和 16000000）。
const CEILING_RUNNER: &str = "echo x >> src/lib.rs && git add -A && git commit -q -m runner && \
    echo 'runner: rc=0 records=3 usage=in:1000000,out:2000000,\
    cache_read:10000000,cache_create:3000000 turns=5 wall_ms=6000'";

/// 和が閾値ちょうど（25000000）の便の判定行（埋め込みの行 `R-C6-1`・消費の event 2 件）。
const CEILING_OVER: &str = "cost-ceiling: over total=25000000 limit=25000000 events=2";

/// 偽 lens の判定 object に載せる消費（4 値はどれも 0 でない・和は 9000000 − `short`）。
fn ceiling_lens(marker: &Path, short: u64) -> String {
    let cache_create = 2_000_000_u64.saturating_sub(short);
    let usage = format!(
        r#""usage":"in:1000000,out:1000000,cache_read:5000000,cache_create:{cache_create}","turns":2,"wall_ms":300"#
    );
    let verdict = lens_verdict("PASS");
    fake_lens(marker, &format!("{},{usage}}}", verdict.trim_end_matches('}')))
}

/// 便を intake → spawn（偽 runner の消費）→ gate（偽 lens の消費）→ land まで通す。消費の event は 2 件で、4 値の和は
/// 25000000 − `short`（repo・置き場・便 id）。
fn ceiling_landed(short: u64) -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let spawned = spawn_with(&repo, &state, &id, CEILING_RUNNER);
    assert!(stdout_of(&spawned).contains("stage=Implemented"), "{}", stdout_of(&spawned));
    let gated = gate_once(&repo, &state, &id, Some(&ceiling_lens(&state.join("lens-ran"), short)));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "PASS の gate は rc 0: {}", stderr_of(&gated));
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land は断らない（rc 0）: {}", stderr_of(&landed));
    let costs = events(&state).iter().filter(|event| event.kind == EventKind::RunCost && event.run == id).count();
    assert_eq!(costs, 2, "消費の event は runner と lens の 2 件");
    (repo, state, id)
}

/// `ceiling_rules` の写しに行 `R-C6-1` を**不発効**で足した `--rules` の fixture（置き場の中に 1 本・`pipe show` は lock の
/// 行も読むので、写しの行はそのまま残す）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn disabled_ceiling_rules(state: &Path) -> String {
    let base = fs::read_to_string(ceiling_rules(state)).expect("ceiling_rules を読める");
    let path = state.join("rules-ceiling-disabled.toml");
    let row = "\n[[rule]]\nid = \"R-C6-1\"\nkind = \"RunTokenCeiling\"\nvalue = 25000000\n\
               enabled = false\nruling = \"t\"\nruled_at = \"d\"\n";
    fs::write(&path, format!("{base}{row}")).expect("tmp manifest を書ける");
    path.display().to_string()
}

/// `--rules` を渡して `pipe show --run` を 1 回撃つ。
fn show_with_rules(repo: &Path, state: &Path, id: &str, rules: &str) -> Output {
    run_pipe(&[
        "show", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--rules", rules,
    ])
}

/// `pipe show` の出力のうち検出線の判定行（`cost-ceiling:` で始まる行）。
fn ceiling_lines(shown: &str) -> Vec<&str> {
    shown.lines().filter(|line| line.starts_with("cost-ceiling:")).collect()
}

/// toy repo の 1 便を intake → spawn まで通す（bead を分けて id の衝突を避ける）。
fn toy_spawn(repo: &Path, state: &Path, bead: &str, design: &str, runner: &str) -> (String, Output) {
    let id = intake_bead(repo, state, design, bead);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
    ]);
    (id, out)
}

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
fn toy_land(toy: &Toy<'_>, bead: &str, design: &str, runner: &str) {
    let (repo, state, lens) = (toy.repo, toy.state, toy.lens);
    let (id, spawned) = toy_spawn(repo, state, bead, design, runner);
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
fn toy_denied(repo: &Path, state: &Path, design: &str) {
    let runner = format!(
        "printf '{{\"cwd\":\"%s\",\"tool_name\":\"Write\",\"tool_input\":{{\"file_path\":\"docs/out.md\"}}}}' \"$PWD\" \
         | '{}' hook pre-tool-use; test $? -eq 0 || exit 1; {TOY_COMMIT}",
        bin()
    );
    let (id, stopped) = toy_spawn(repo, state, "toy-guard", design, &runner);
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
fn toy_compliant_refusal(toy: &Toy<'_>, design: &str) {
    let (repo, state) = (toy.repo, toy.state);
    let runner = "git commit -q --allow-empty -m 'goal と write-set が両立しない'";
    let (id, spawned) = toy_spawn(repo, state, "toy-refuse", design, runner);
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
fn toy_gate_fail(toy: &Toy<'_>, design: &str, lens: &str) {
    let (repo, state) = (toy.repo, toy.state);
    let (id, spawned) = toy_spawn(repo, state, "toy-fail", design, TOY_COMMIT);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&spawned));
    let gated = gate_once(repo, state, &id, Some(lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の gate は rc 1");
    let refused = land_once(repo, state, &id);
    assert_eq!(refused.status.code(), Some(i32::from(RC_REFUSED)), "FAIL の便は land しない");
}

/// 3 クラスを名乗る便: spawn の手前で Blocked → approve（逐語）→ resume → gate → land。
fn toy_approved_land(toy: &Toy<'_>, design: &str) {
    let (repo, state, lens) = (toy.repo, toy.state, toy.lens);
    let (id, blocked) = toy_spawn(repo, state, "toy-approve", design, TOY_COMMIT);
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
        report_head(&out),
        format!("runs=5 landed=3 human_events=1 human_events_other_than_approval=0 {NO_REVIEW_FAIL}"),
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
    let design = write_contract(&repo, &[], &[]);
    toy_denied(&repo, &state, &design);
    clean(&[&repo, &state]);
}

// ── 回答後の再開が写しを行から取り直す（設計 docs/design/pipeline-question.md §11・契約表の行 a・`s2-07l.546`） ──

/// 質問で止めて回答を記帳した便の id（resume の手前まで）。
fn answered(repo: &Path, state: &Path) -> String {
    let id = questioned(repo, state);
    let out = run_pipe(&["answer", "--run", &id, "--words", "write-set を広げた", "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    id
}

/// stdin を `copied` へ写して commit を 1 本作る fake runner で resume する。
fn resume_copying(repo: &Path, state: &Path, id: &str, copied: &Path) -> Output {
    let runner = format!("cat > '{}' && {TOY_COMMIT}", copied.display());
    run_pipe(&[
        "resume", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ])
}

/// 段 `Questioned`・detail=contract:refreshed の `RunStage` の件数。
fn refreshed_count(state: &Path, id: &str) -> usize {
    trail(state, id)
        .iter()
        .filter(|(kind, stage, detail)| {
            *kind == EventKind::RunStage
                && *stage == Some(Stage::Questioned)
                && detail.as_deref() == Some("contract:refreshed")
        })
        .count()
}

// ───── 初回の起動も器が口座を選ぶ（`s2-07l.285`・設計 account-autonomy.md §3 / §4・SRS FR36 / FR37・接頭辞 `pipe_spawn_account_`） ─────
//
// 口座の fixture（manifest・credential・偽 curl・argv を写す偽 runner）は `lifecycle.rs` の再開の歯と同じ物を引く
// （初回の起動と再開が**同じ 1 関数**の選定を通ることを、同じ fixture で測る）。

use super::lifecycle::{
    argv_account_dir, assert_lands_without_human, curl_calls, fake_usage_curl, limited_for, put_account,
    register_seat_account, resume_rules, spawned_details, spy_reset, stub_argv, turn_runner, windows,
};

/// (a) 口座 a1 / a2 を宣言し、a1 を席の登録 row に置いた置き場で `pipe run` を撃つと、**初回の turn から**器が便用の
/// 規則で選んだ a2 で runner が起きる: 偽 curl が口座 2 つ分呼ばれ（計測 1 回）・runner の argv に
/// `--account-dir <state>/accounts/a2`・`Spawned detail=base:<sha>,account:a2`・stdout に `next=spawn account=` は
/// **出ない**（初回は判定行を持たない・名乗るのは `RateLimited` の再開だけ）。a1 の方が余裕が大きい（逼迫度で勝つ）
/// ので、a2 が選ばれるのは登録 row の除外が効いた証拠。便はそのまま gate → land まで通る（人由来の event 0）。
#[test]
fn pipe_spawn_account_first_turn_runs_on_the_chosen_free_account() {
    let (repo, state) = repo_with_state();
    // 行の commit が main を進めるので、base は**行を置いた後**に読む（契約 (b)）。
    let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let runner = turn_runner(&state, &[IMPLEMENT.to_owned()]);
    let rules = resume_rules(&state, &["a1", "a2"]);
    put_account(&state, "a1", &[windows(10, 10)]);
    put_account(&state, "a2", &[windows(40, 10)]);
    register_seat_account(&state, &repo, "a1");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "run", "--design", &first, "--bead", "s2-acct",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules, "--curl", &fake_usage_curl(&state), "--runner", &runner, "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    let id = run_id_of(&out);
    let stdout = stdout_of(&out);
    assert!(!stdout.contains("next=spawn account="), "初回は判定行を持たない: {stdout}");
    assert!(!stdout.contains("next=wait"), "候補が在るので待たない: {stdout}");
    // 計測は runner の起動の前と gate の lens の前で 1 回ずつ（`s2-07l.412`・設計 account-autonomy.md §15）。
    assert_eq!(curl_calls(&state), 4, "起動の前と lens の前に FR33 の計測を 1 回ずつ（口座 2 つ × 2）");
    assert_eq!(stub_calls(&state), 1, "runner は 1 回起きる");
    assert_eq!(
        argv_account_dir(&stub_argv(&state, 1)),
        Some(state.join("accounts").join("a2").display().to_string()),
        "初回の turn から選んだ口座の credential dir を渡す: {:?}",
        stub_argv(&state, 1)
    );
    assert_eq!(spawned_details(&state, &id), vec![format!("base:{base},account:a2")], "base と選んだ口座を名乗る");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "1 process で Landed まで: {stdout}");
    assert!(!events(&state).iter().any(|event| event.actor == "human"), "人由来の event は 0");
    clean(&[&repo, &state]);
}

/// 起動行が**既に** `--account-dir` を持つ周は、器は足さずに断る（`s2-07l.411`・設計 account-autonomy.md §16）:
/// (a) と同じ fixture（a1 / a2 を宣言・a1 は登録 row ＝器は a2 を選ぶ）の `--runner` の雛形の末尾に a1 の
/// credential dir を literal で書いて `pipe run` を撃つと、rc 1・stderr に `pipe:` と**行が持っていた** a1 の値・
/// `Spawned` は 0 件・runner は 1 度も起きない。base は末尾に a2 を足して 2 つ並べたまま起こすので、runner の
/// 読み手が最初の値（a1）を採り、記帳（a2）と実行がずれる＝RED。
#[test]
fn headless_flag_duplicate_with_account_refuses_when_already_present() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let present = state.join("accounts").join("a1").display().to_string();
    let runner = format!("{} --account-dir {present}", turn_runner(&state, &[IMPLEMENT.to_owned()]));
    let rules = resume_rules(&state, &["a1", "a2"]);
    put_account(&state, "a1", &[windows(10, 10)]);
    put_account(&state, "a2", &[windows(40, 10)]);
    register_seat_account(&state, &repo, "a1");
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "run", "--design", &first, "--bead", "s2-acct",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules, "--curl", &fake_usage_curl(&state), "--runner", &runner, "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{} / {}", stdout_of(&out), stderr_of(&out));
    let err = stderr_of(&out);
    assert!(err.contains("pipe:"), "断りは器の 1 行: {err}");
    assert!(err.contains(&present), "行が持っていた値を名乗る: {err}");
    let id = run_id_of(&out);
    assert!(!id.is_empty(), "便の id は落ちた周も stdout に出る: {}", stdout_of(&out));
    assert_eq!(spawned_details(&state, &id), Vec::<String>::new(), "Spawned を記帳しない");
    assert_eq!(stub_calls(&state), 0, "runner は 1 度も起きない");
    clean(&[&repo, &state]);
}

/// (b) 口座の宣言が 0 の置き場（既存の fixture のまま）では runner は親の環境を継承する: argv に `--account-dir` 無し・
/// `Spawned detail=base:<sha>`（口座の接尾辞なし）・stderr に継承の 1 行・計測は撃たない。既存の `pipe_spawn_` /
/// `pipe_five_` / `pipe_e2e_` の歯が名を変えず緑＝継承の形は不変。
#[test]
fn pipe_spawn_account_inherits_when_no_account_is_declared() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = intake(&repo, &state, &path);
    let runner = turn_runner(&state, &[IMPLEMENT.to_owned()]);
    let out = spawn_with(&repo, &state, &id, &runner);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert!(stderr_of(&out).contains("口座の宣言が無い＝親の環境を継承"), "継承の 1 行: {}", stderr_of(&out));
    assert_eq!(argv_account_dir(&stub_argv(&state, 1)), None, "宣言 0 は --account-dir を渡さない: {:?}", stub_argv(&state, 1));
    assert_eq!(spawned_details(&state, &id), vec![format!("base:{base}")], "detail は base だけ");
    assert_eq!(curl_calls(&state), 0, "宣言の無い置き場は測らない");
    clean(&[&repo, &state]);
}

/// (c) 宣言あり・全口座が当たっている周は `run=<id> next=wait reset=<ts>` を出して唯一の wait で待ち、reset を過ぎて
/// `Timeout` を受けた周は計測から撃ち直して（偽 curl の 2 回目は a1 に余裕）a1 で起こす
/// （`pipe_ratelimit_resume_waits_for_the_earliest_reset_then_remeasures` と同型・reset は偽 curl の呼ばれた瞬間から相対）。
/// 待ちの間の段は起動前の段（`Reviewed`）のまま＝`RateLimited` 固定の観測なら即「満たされた」になり待たずに選び直し
/// 続ける（計測 2 回で a1 を選ぶが待ち時間 0 ＝ busy loop）。
#[test]
fn pipe_spawn_account_waits_for_the_earliest_reset_then_remeasures() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let runner = turn_runner(&state, &[IMPLEMENT.to_owned()]);
    let rules = resume_rules(&state, &["a1", "a2"]);
    // a1 は計測の 2 秒後に開き直る（最も早い reset）・a2 は 4 秒後。2 回目の計測では a1 に余裕が戻る。
    put_account(&state, "a1", &[limited_for(2), windows(50, 10)]);
    put_account(&state, "a2", &[limited_for(4), limited_for(4)]);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let started = Instant::now();
    let out = run_pipe(&[
        "run", "--design", &first, "--bead", "s2-acct",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules, "--curl", &fake_usage_curl(&state), "--runner", &runner, "--lens", &lens,
    ]);
    let waited = started.elapsed();
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    let id = run_id_of(&out);
    let stdout = stdout_of(&out);
    let soon = spy_reset(&state, "a1", 1);
    assert!(vessel::fleet::epoch_of(&soon).is_some(), "偽 curl が a1 の 1 回目に相対 reset を埋めた: {soon:?}");
    assert!(stdout.contains(&format!("run={id} next=wait reset={soon}")), "最も早い reset を名乗って待つ: {stdout}");
    assert!(!stdout.contains("next=spawn account="), "初回は判定行を持たない: {stdout}");
    assert!(waited >= Duration::from_secs(1), "reset まで待った（{waited:?}）");
    // 起動の前に 2 回（待ち → 撃ち直し）・gate の lens の前に 1 回（`s2-07l.412`・設計 account-autonomy.md §15）。
    assert_eq!(curl_calls(&state), 6, "Timeout の後に計測を撃ち直し、lens の前にもう 1 回（口座 2 つ × 3 回）");
    assert_eq!(stub_calls(&state), 1, "runner は 1 回起きる");
    assert_eq!(
        argv_account_dir(&stub_argv(&state, 1)),
        Some(state.join("accounts").join("a1").display().to_string()),
        "reset の後の計測で a1 を選ぶ: {:?}",
        stub_argv(&state, 1)
    );
    assert_eq!(spawned_details(&state, &id).first().map(|found| found.ends_with(",account:a1")), Some(true));
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "{stdout}");
    clean(&[&repo, &state]);
}

/// (d) `Spawned detail=base:<sha>,account:a2` の便を gate → land まで通す: base の読み手（`base_of_run`）が `,` の
/// 手前までを sha と読む（読めないと land が「base が無い」で断る・sha に接尾辞が残ると CAS が外れる）。
#[test]
fn pipe_spawn_account_base_with_account_suffix_lands() {
    let (repo, state) = repo_with_state();
    // 行の commit が main を進めるので、base は**行を置いた後**に読む（契約 (b)）。
    let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let id = intake_bead(&repo, &state, &first, "s2-acct");
    let runner = turn_runner(&state, &[IMPLEMENT.to_owned()]);
    let rules = resume_rules(&state, &["a2"]);
    put_account(&state, "a2", &[windows(40, 10)]);
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
        "--rules", &rules, "--curl", &fake_usage_curl(&state),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(spawned_details(&state, &id), vec![format!("base:{base},account:a2")]);
    // gate の `{base}` と land の CAS が接尾辞の手前の sha を読む＝そのまま Landed まで通る。
    assert_lands_without_human(&repo, &state, &id);
    assert_eq!(git(&repo, &["rev-list", "--count", &format!("{base}..refs/heads/main")]), "1", "base の上に 1 便が載る");
    clean(&[&repo, &state]);
}

// ───── 箱の中の死の理由は kernel の証拠で分ける（`s2-07l.340`・設計 pipeline.md §23・接頭辞 `pipe_spawn_terminal_reason_` /
// `pipe_spawn_reason_vocabulary_`） ─────

/// 偽 `systemd-run` の argv を写す記録 dir 名（**1 起動 1 file**）。
const TERMINAL_SCOPE_RECORDS: &str = "terminal-scope-args";

/// **包める周に固定する** PATH（偽 `systemd-run` の本体は `crate::write_systemd_run_stub` の 1 つの生成関数
/// から出る・設計 gate-cost.md §30 約束 3）。偽の包みの中では `/proc/self/cgroup` が unit の scope と一致しない
/// ので、包みの終端行は出ない＝kernel の証拠は runner が自分で書いた行だけになる。
// flip-check: retroactive s2-07l.504
fn terminal_confined_path(state: &Path) -> String {
    let bin_dir = state.join("terminal-systemd-bin");
    crate::write_systemd_run_stub(&bin_dir, &state.join(TERMINAL_SCOPE_RECORDS));
    format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default())
}

/// 包める PATH で `runner` の便を 1 本 spawn し、（便 id・spawn の出力）を返す。前提として runner が包めた周で
/// 起きたことを assert する（包めない周の「oom-kill 0 件」で空虚に充足しない）。
///
/// 前提の読みは**記録 dir の走査**で、母集団（全件の名）を assert の本文に出す。
fn spawn_confined(repo: &Path, state: &Path, runner: &str) -> (String, Output) {
    let design = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &design);
    let out = run_pipe_with_path(
        &terminal_confined_path(state),
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", runner],
    );
    let records = state.join(TERMINAL_SCOPE_RECORDS);
    let names = dir_names(&records);
    let confined = names
        .iter()
        .filter(|name| name.contains("-runner-"))
        .filter_map(|name| fs::read_to_string(records.join(name)).ok())
        .any(|body| body.lines().any(|line| line == "--scope"));
    assert!(confined, "前提: runner は包めた周で起きた（母集団 {names:?}）");
    (id, out)
}

/// **runner の箱は 1 × `gate.job_memory_mb`**（設計 gate-cost.md §12・行 c・裁定 id user 2026-09-15T18:2xZ）。
/// base は `MemTotal − host.reserve_memory_mb`（host の箱）を渡す＝RED。runner の記録はちょうど 1 件を読む
/// （母集団を確かめずに `contains` すると別の起動の引数で充足する）。
#[test]
fn pipe_confine_runner_limit_is_one_job_box() {
    let (repo, state) = repo_with_state();
    let (_id, out) = spawn_confined(&repo, &state, TOY_COMMIT);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn: {}", stderr_of(&out));
    let records = state.join(TERMINAL_SCOPE_RECORDS);
    let names = dir_names(&records);
    let runner: Vec<&String> = names.iter().filter(|name| name.contains("-runner-")).collect();
    assert_eq!(runner.len(), 1, "runner の記録はちょうど 1 件（母集団 {names:?}）");
    let body = runner
        .first()
        .and_then(|name| fs::read_to_string(records.join(name)).ok())
        .unwrap_or_default();
    let limit = body.lines().find_map(|line| line.strip_prefix("MemoryMax=")).unwrap_or_default();
    assert_eq!(
        limit,
        format!("{}M", embedded_int("gate.job_memory_mb")),
        "runner の箱は 1 × gate.job_memory_mb: {body}"
    );
    clean(&[&repo, &state]);
}

/// (b) 包めた周で、終端行を出さずに自分を KILL する runner（kernel の証拠が無い signal 死）は `Failed detail=unknown`。
/// base は rc < 0 だけで `oom-kill` を書く。
#[test]
fn pipe_spawn_terminal_reason_no_evidence_is_unknown() {
    let (repo, state) = repo_with_state();
    let (id, out) = spawn_confined(&repo, &state, "kill -KILL $$");
    assert!(stdout_of(&out).contains("stage=Failed"), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last(),
        Some(&(Some(Stage::Failed), Some("unknown".to_owned()))),
        "証拠の無い kill は unknown: {:?}",
        stages(&state, &id)
    );
    assert!(
        !stages(&state, &id).iter().any(|(_, detail)| detail.as_deref() == Some("oom-kill")),
        "oom-kill を書かない: {:?}",
        stages(&state, &id)
    );
    clean(&[&repo, &state]);
}

/// (c) 終端行 `oom_kill=1` を出す runner（rc 0）は従来どおり `Failed detail=oom-kill`（退行の pin）。
#[test]
fn pipe_spawn_terminal_reason_oom_evidence_stays_oom_kill() {
    let (repo, state) = repo_with_state();
    let runner = format!("{TOY_COMMIT}\nprintf 'confine-usage peak_bytes=9437184 oom_kill=1\\n'");
    let (id, out) = spawn_confined(&repo, &state, &runner);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "記帳は通る: {}", stderr_of(&out));
    assert_eq!(
        stages(&state, &id).last(),
        Some(&(Some(Stage::Failed), Some("oom-kill".to_owned()))),
        "kernel の証拠がある周は oom-kill: {:?}",
        stages(&state, &id)
    );
    clean(&[&repo, &state]);
}

// ───── 連鎖の段の通知を捨てない（`s2-07l.293`・設計 pipeline.md §21・接頭辞 `pipe_spawn_notice_`） ─────
//
// `pipe run` は 4 段を 1 process で畳む。畳む口が rc 0 の段の stderr を落としていたので、連鎖で撃った便は
// gate の `lens-input=<kind> reason=<語>` も口座の継承の行も端末に出ず、run dir の log にも残らなかった
// （`.286` の実測: run.stderr 0 byte・rc 0）。**stdout の判定行は 1 字も動かさない**のが対の面である。

/// stderr の行の番号（無ければ `None`）。
fn stderr_index(out: &Output, needle: &str) -> Option<usize> {
    stderr_of(out).lines().position(|line| line.starts_with(needle))
}

/// 段の通知を rc 0 の段も含めて**段の順**で呼び手の stderr に出し、stdout の判定行は変えない。
///
/// 通知を出す段を 2 つ持つ fixture である: gate は純移動でない理由の 1 行（rc 0）、land は汚れた anchor を
/// 揃えなかった warning の 1 行（rc 0・後始末の失敗は land を取り消さない）。base は畳む口が rc 0 の段の
/// stderr を捨てるので、**どちらの行も 1 本も出ない**＝RED。
#[test]
fn pipe_spawn_notice_run_relays_each_stage_stderr_in_order() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    // land の段に通知を作る（未 commit の変更が在る anchor は揃えない・rc 0 のまま stderr 1 行）。
    fs::write(repo.join("src").join("lib.rs"), "// local uncommitted\n").expect("局所の変更を置ける");
    let lens = fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"));
    let out = run_pipe(&[
        "run", "--design", &path, "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state), "--runner", TOY_COMMIT, "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    let id = run_id_of(&out);
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "1 process で Landed まで: {}", stdout_of(&out));

    // (1) rc 0 で終わった段の通知が段の順で出る（gate の段 → land の段）。
    let notice = stderr_index(&out, "pipe: lens-input=diff reason=");
    let anchor = stderr_index(&out, "pipe: anchor を新 main に揃えていない");
    assert!(notice.is_some(), "gate の段の通知が出る（rc 0 でも捨てない）: {}", stderr_of(&out));
    assert!(anchor.is_some(), "land の段の通知も出る: {}", stderr_of(&out));
    assert!(notice < anchor, "段の順で並ぶ: {}", stderr_of(&out));

    // (2) 対: stdout の判定行は base と 1 字も変わらない（理由の語は stdout に漏れない）。
    let stdout = stdout_of(&out);
    let verdict_line = stdout
        .lines()
        .find(|line| line.starts_with(&format!("run={id} verdict=")))
        .unwrap_or_default();
    let tokens: Vec<&str> = verdict_line.split_whitespace().collect();
    assert_eq!(tokens.len(), 4, "判定行の token は 4 つのまま: {verdict_line}");
    assert_eq!(tokens.get(1).copied(), Some("verdict=PASS"), "{verdict_line}");
    assert_eq!(tokens.get(2).copied(), Some("lens-input=diff"), "{verdict_line}");
    assert!(
        tokens.get(3).and_then(|token| token.strip_prefix("bytes=")).is_some_and(|bytes| bytes.parse::<u64>().is_ok()),
        "末尾は byte 数: {verdict_line}"
    );
    assert!(!stdout.contains("reason="), "理由の語は stdout に出さない: {stdout}");
    clean(&[&repo, &state]);
}

/// (d) 器が公開する封じ込めの理由の列（`confine::REASONS` の `as_str`）が**逐語の列で完全一致**する: 母集団 8・
/// 字面の重複 0・宣言順の末尾が `unknown`。variant の名を書かず字面だけで測る（base の木でも compile する）。
#[test]
fn pipe_spawn_reason_vocabulary_closes_over_the_unnamed_kill() {
    let words: Vec<&str> = vessel::pipe::confine::REASONS.iter().map(|reason| reason.as_str()).collect();
    assert_eq!(
        words,
        vec!["no-systemd-run", "no-scope", "no-rules", "manifest-unreadable", "no-room", "oom-kill", "signal", "unknown"],
        "理由の列（宣言順）"
    );
    let unique: BTreeSet<&str> = words.iter().copied().collect();
    assert_eq!(unique.len(), words.len(), "字面の重複 0: {words:?}");
}

// ───── runner の stdin に共通 verify の節（設計 pipeline.md §65・行 bh・接頭辞 `runner_common_section_`） ─────

/// 契約の verify 行（filter 語 `sect_words_` を持つ nextest 行）。語は共通 verify の写しにも雛形にも無い字面。
const SECTION_VERIFY: &str = r#"verify = ["cargo nextest run -p toy --no-tests=fail sect_words_"]"#;

/// 宣言の共通 verify（穴を持つ行 2 本と穴の無い行 1 本）。
const SECTION_COMMON: &str =
    r#"["git rev-parse --verify {base}", "git log -n {jobs} --grep={teeth} --format={threads}", "git status --short"]"#;

/// 共通 verify を宣言し、契約の verify 行に filter 語 `sect_words_` を持たせた便を intake する（repo・置き場・run id）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn section_intake(common: &str) -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, r#"["cargo", "git", "sh"]"#, common);
    // 契約の verify 行の filter 語 `sect_words_` の置き場（受付が歯の置き場を解く）。
    fs::create_dir_all(repo.join("crates/toy/tests")).expect("dir を作れる");
    fs::write(repo.join("crates/toy/tests/sect.rs"), "#[test]\nfn sect_words_one() {}\n").expect("歯を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "sect-tooth"]);
    let design = write_contract(&repo, &["verify", "write-set"], &[SECTION_VERIFY, r#"write-set = ["src/lib.rs", "crates/toy/tests/sect.rs"]"#]);
    let id = intake(&repo, &state, &design);
    (repo, state, id)
}

/// [`section_intake`] の便を質問で止め、回答済みにして 2 turn 目の runner に stdin を写させる（stdin を返す）。
fn section_answered_stdin(repo: &Path, state: &Path, id: &str) -> String {
    let out = run_pipe(&[
        "spawn", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &question_runner(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "質問で止まる: {}", stderr_of(&out));
    let answered = run_pipe(&["answer", "--run", id, "--words", "そのまま進める", "--state-dir", &state.display().to_string()]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&answered));
    let copied = state.join("got-stdin.txt");
    let out = resume_copying(repo, state, id, &copied);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    fs::read_to_string(&copied).unwrap_or_default()
}

/// stdin から「## 共通 verify」節の本文（次の節の見出しの手前まで）を取る。
fn section_body(stdin: &str) -> String {
    let after = stdin.split("\n## 共通 verify\n").nth(1).unwrap_or_default();
    after.split("\n## ").next().unwrap_or_default().to_owned()
}

/// 便の最初の `Spawned` の記録した base の sha。
fn recorded_base(state: &Path, id: &str) -> String {
    let detail = stages(state, id)
        .into_iter()
        .find_map(|(stage, detail)| (stage == Some(Stage::Spawned)).then_some(detail).flatten())
        .unwrap_or_default();
    detail.strip_prefix("base:").map(|rest| rest.split(',').next().unwrap_or_default().to_owned()).unwrap_or_default()
}

/// (a) 回答済みの質問を持つ便の 2 turn 目の stdin は、契約の本文の後・回答の節の前に「## 共通 verify」節を持ち、写しの各行を
/// `{base}` は記録した base の sha・`{jobs}` と `{threads}` は 1・`{teeth}` は契約の verify 行の filter 語で埋めた字で
/// 1 行 1 項目に並べる。穴の字 `{` は節に残らない。
#[test]
fn runner_common_section_lists_the_filled_lines_between_contract_and_answer() {
    let (repo, state, id) = section_intake(SECTION_COMMON);
    let stdin = section_answered_stdin(&repo, &state, &id);
    let (contract_at, section_at, answer_at) = (stdin.find("goal = "), stdin.find("\n## 共通 verify\n"), stdin.find("\n## 回答\n"));
    assert!(
        matches!((contract_at, section_at, answer_at), (Some(c), Some(s), Some(a)) if c < s && s < a),
        "契約 → 共通 verify → 回答 の順: {stdin}"
    );
    let base = recorded_base(&state, &id);
    assert_eq!(base.len(), 40, "記録した base は sha: {base}");
    let want = format!(
        "- git rev-parse --verify {base}\n- git log -n 1 --grep=sect_words_ --format=1\n- git status --short\n"
    );
    assert_eq!(section_body(&stdin), want, "穴を埋めた行が 1 行 1 項目で並ぶ: {stdin}");
    assert!(!section_body(&stdin).contains('{'), "穴の字が残らない: {stdin}");
    clean(&[&repo, &state]);
}

/// (b) 写しを読めない便の節の本文は読めない理由の 1 行で、runner は止まらず段は Implemented まで進む（回答後の resume は
/// 写しを取り直す段で先に断るので、写しを壊すのは初回の spawn の前）。
#[test]
fn runner_common_section_says_why_when_the_copy_is_unreadable() {
    let (repo, state, id) = section_intake(SECTION_COMMON);
    fs::write(vessel_copy(&state, &id), "これは宣言ではない\n").unwrap_or_default();
    let copied = state.join("got-stdin.txt");
    let runner = format!("cat > '{}' && {TOY_COMMIT}", copied.display());
    let out = run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "runner は止まらない: {}", stderr_of(&out));
    let stdin = fs::read_to_string(&copied).unwrap_or_default();
    let body = section_body(&stdin);
    assert!(body.starts_with("（共通 verify の写しを読めない: ") && body.trim_end().ends_with('）'), "理由の 1 行: {stdin}");
    assert_eq!(body.trim_end().lines().count(), 1, "本文は 1 行: {body}");
    assert!(!body.contains("- "), "行の項目は無い: {body}");
    let last = stages(&state, &id).into_iter().next_back().and_then(|(stage, _)| stage);
    assert_eq!(last, Some(Stage::Implemented), "段は Implemented まで進む");
    clean(&[&repo, &state]);
}

/// (c) common-verify が 0 行の写しの節の本文は `なし` の 1 行。宣言も便の写しも 0 行を読み込みで断る（空の配列を許さない）ので
/// 0 行の写しの便は file からは作れない——節の組み立ての関数を直に撃つ。対: 1 行なら `なし` でなく項目になる。
#[test]
fn runner_common_section_says_none_for_an_empty_common_verify() {
    assert_eq!(vessel::pipe::spawn::common_lines(&[], "abc", &[]), "なし\n", "0 行は なし の 1 行");
    let one = vec!["git status".to_owned()];
    assert_eq!(vessel::pipe::spawn::common_lines(&one, "abc", &[]), "- git status\n", "1 行は項目");
}

// ───── runner の stdin にほかの行の touches の節（設計 reverse-index.md §15・行 f・接頭辞 `runner_touches_section_`） ─────

/// 区間を持つ設計 doc（行 `id` ごとに touches の列を持つ）を repo の `rel` へ書く（commit は呼び手）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_touches_doc(repo: &Path, rel: &str, rows: &[(&str, &[&str])]) {
    let fields: Vec<Vec<String>> = rows
        .iter()
        .map(|(id, touches)| {
            let quoted: Vec<String> = touches.iter().map(|item| format!("\"{item}\"")).collect();
            row_fields(id, &[], &[format!("touches = [{}]", quoted.join(", ")).as_str()])
        })
        .collect();
    let path = repo.join(rel);
    fs::create_dir_all(path.parent().expect("doc の dir が在る")).expect("doc の dir を作れる");
    fs::write(&path, design_doc_rows(&fields)).expect("doc を書ける");
}

/// 自分の行（touches は型 1 つ・toy.md の行 a）を持つ repo に、別の doc の 2 行と子の dir の doc の 1 行を commit し、便を受け付けて
/// 返す（repo・置き場・run id）。
fn touches_intake() -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    write_touches_doc(
        &repo,
        "docs/design/other.md",
        &[("b1", &["crate::other::Shared", "crate::other::helper"]), ("b2", &["crate::other::Shared"])],
    );
    write_touches_doc(&repo, "docs/design/child/deep.md", &[("c1", &["crate::child::Hidden"])]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "other-docs"]);
    let design = write_contract(&repo, &[], &[r#"touches = ["crate::pipe::refuse::Refuse"]"#]);
    let id = intake(&repo, &state, &design);
    (repo, state, id)
}

/// runner が stdin を写して commit を 1 本作る周の stdin（runner は止まらず rc 0）。
fn touches_spawn_stdin(repo: &Path, state: &Path, id: &str) -> String {
    let copied = state.join("got-stdin.txt");
    let runner = format!("cat > '{}' && {TOY_COMMIT}", copied.display());
    let out = run_pipe(&[
        "spawn", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "runner は止まらない: {}", stderr_of(&out));
    fs::read_to_string(&copied).unwrap_or_default()
}

/// stdin から「## ほかの行の touches」節の本文（次の節の見出しの手前まで）を取る。
fn touches_body(stdin: &str) -> String {
    let after = stdin.split("\n## ほかの行の touches\n").nth(1).unwrap_or_default();
    after.split("\n## ").next().unwrap_or_default().to_owned()
}

/// (a) 自分の行・別の doc の 2 行・子の dir の doc の行を commit し、anchor の作業木だけに別の doc の行を足した repo の初回の
/// stdin は、「## 共通 verify」節の後に「## ほかの行の touches」節を持つ。本文は 2 項目の 2 行（型は 2 つの pointer・fn は 1 つ）
/// だけで、自分の行の型・子の dir の doc の型・未 commit の行の型を持たない。
#[test]
fn runner_touches_section_lists_other_rows_from_the_base_tree_without_own_row() {
    let (repo, state, id) = touches_intake();
    write_touches_doc(&repo, "docs/design/dirty.md", &[("d1", &["crate::dirty::Wip"])]);
    let stdin = touches_spawn_stdin(&repo, &state, &id);
    let (common_at, section_at) = (stdin.find("\n## 共通 verify\n"), stdin.find("\n## ほかの行の touches\n"));
    assert!(matches!((common_at, section_at), (Some(c), Some(s)) if c < s), "共通 verify の後に節: {stdin}");
    let want = "- crate::other::Shared ← docs/design/other.md#b1, docs/design/other.md#b2\n\
                - crate::other::helper ← docs/design/other.md#b1\n";
    assert_eq!(touches_body(&stdin), want, "2 項目の 2 行だけ: {stdin}");
    for absent in ["Refuse", "Hidden", "Wip"] {
        assert!(!touches_body(&stdin).contains(absent), "{absent} は載らない: {stdin}");
    }
    clean(&[&repo, &state]);
}

/// (g) 宣言の key `contract-tables` で `contracts/` を名乗る toy の、`contracts/t.toml` の行 b の touches の項目が、runner の stdin の
/// 「ほかの行の touches」節に `contracts/t.toml#b` の pointer で載る（置き場は便の base の宣言から読む）。
#[test]
fn spawn_touches_from_declared_tables_lists_a_row_of_the_declared_place() {
    let (repo, state) = repo_with_state();
    let declaration = fs::read_to_string(repo.join(".vessel.toml")).expect("宣言を読める");
    fs::write(repo.join(".vessel.toml"), format!("{declaration}contract-tables = [\"contracts/\"]\n")).expect("宣言を書ける");
    let row = row_fields("b", &[], &[r#"touches = ["crate::declared::Place"]"#]);
    fs::create_dir_all(repo.join("contracts")).expect("置き場の dir を作れる");
    fs::write(repo.join("contracts/t.toml"), format!("schema = 1\n\n[[contract]]\n{}\n", row.join("\n"))).expect("置き場の表を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "declared-place"]);
    let design = write_contract(&repo, &[], &[r#"touches = ["crate::pipe::refuse::Refuse"]"#]);
    let id = intake(&repo, &state, &design);
    let stdin = touches_spawn_stdin(&repo, &state, &id);
    assert_eq!(touches_body(&stdin), "- crate::declared::Place ← contracts/t.toml#b\n", "置き場の行の touches が載る: {stdin}");
    clean(&[&repo, &state]);
}

/// (b) 受付の後・spawn の前に別の doc の区間を壊して commit した便の節の本文は、doc の path を持つ理由の 1 行で、段は
/// Implemented まで進む。
#[test]
fn runner_touches_section_says_why_when_a_doc_region_is_unreadable() {
    let (repo, state, id) = touches_intake();
    let broken = format!("# 壊れた doc\n\n{}\nこれは表ではない\n{}\n", vessel::pipe::table::BEGIN, vessel::pipe::table::END);
    fs::write(repo.join("docs/design/other.md"), broken).unwrap_or_default();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "break-other"]);
    let stdin = touches_spawn_stdin(&repo, &state, &id);
    let body = touches_body(&stdin);
    assert!(
        body.starts_with("（ほかの行の touches を読めない: docs/design/other.md") && body.trim_end().ends_with('）'),
        "doc の path を持つ理由の 1 行: {stdin}"
    );
    assert_eq!(body.trim_end().lines().count(), 1, "本文は 1 行: {body}");
    let last = stages(&state, &id).into_iter().next_back().and_then(|(stage, _)| stage);
    assert_eq!(last, Some(Stage::Implemented), "段は Implemented まで進む");
    clean(&[&repo, &state]);
}

/// (c) 組み立ての関数を直に撃つ。項目 0 は `なし` の 1 行・自分の pointer の行を除く・同じ項目の pointer を 1 行に束ねる
/// （同じ pointer は 1 回・項目は辞書順）。
#[test]
fn runner_touches_section_lines_skip_own_row_and_bundle_pointers() {
    let row = |pointer: &str, touches: &[&str]| {
        (pointer.to_owned(), touches.iter().map(|item| (*item).to_owned()).collect::<Vec<_>>())
    };
    assert_eq!(vessel::pipe::spawn::touches_lines(&[], "d#a"), "なし\n", "項目 0 は なし");
    let rows = [row("d#a", &["X", "Z"]), row("d#b", &["Y", "X", "X"]), row("e#c", &["X"])];
    assert_eq!(vessel::pipe::spawn::touches_lines(&rows, "d#a"), "- X ← d#b, e#c\n- Y ← d#b\n", "自分を除き・束ね・辞書順");
    assert_eq!(vessel::pipe::spawn::touches_lines(rows.get(..1).unwrap_or_default(), "d#a"), "なし\n", "自分の行だけなら項目 0");
}

// ───── runner の終わりの門（設計 pipeline.md §66・行 bj・接頭辞 `end_gate_`） ─────
//
// runner が rc 0 で commit を作って終わった周に、器が gate と同じ行を便の worktree へ撃つ。赤なら同じ worktree で runner を
// 起こし直して赤を渡し、上限の周まで直させてから Implemented にする。偽 runner は turn ごとに stdin を写す（`turn_runner`）。
// 契約の verify 行は `src/lib.rs` の中身で赤と緑が決まる stub で、赤い周に**cmd の字に無い語**を stderr へ出す。

/// 契約の verify 行が撃つ stub（`src/lib.rs` に `green` が在れば緑・無ければ stderr に語を出して rc 1）。
const GATE_STUB: &str = "if grep -q green src/lib.rs; then exit 0; fi\nprintf 'gate-red-%s\\n' word >&2\nexit 1\n";

/// stub を撃つ契約の verify 行。
const GATE_VERIFY: &str = r#"verify = ["sh verify-lib.sh"]"#;

/// turn の本文: `word` を `src/lib.rs` の末尾へ足して commit する（赤 / 緑は `green` を含むかで決まる）。
fn commit_turn(word: &str) -> String {
    format!("printf '{word}\\n' >> src/lib.rs\ngit add -A\ngit commit -q -m '{word}'\nexit 0")
}

/// stub の契約の verify 行を持つ便を intake する（repo・置き場・便 id）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn gate_run_intake() -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    fs::write(repo.join("verify-lib.sh"), GATE_STUB).expect("stub を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "gate-stub"]);
    let design = write_contract(&repo, &["verify"], &[GATE_VERIFY]);
    let id = intake(&repo, &state, &design);
    (repo, state, id)
}

/// 行 `runner.end_gate_rounds` を足した tmp manifest（`rounds` が `None` の周は行を足さない）。受付の fixture は `slots`。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn end_gate_rules(state: &Path, name: &str, slots: SlotFixture, rounds: Option<u64>) -> PathBuf {
    let path = write_rules_full(state, name, (1, 1_000_000), FOLLOW_RETRIES, slots);
    if let Some(rounds) = rounds {
        let body = fs::read_to_string(&path).expect("tmp manifest を読める");
        let row = format!(
            "\n[[rule]]\nid = \"runner.end_gate_rounds\"\nkind = \"RunnerEndGateRounds\"\nvalue = {rounds}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n"
        );
        fs::write(&path, format!("{body}{row}")).expect("tmp manifest を書ける");
    }
    path
}

/// 偽 runner `runner` で spawn を 1 回撃つ（`rules` が在れば `--rules`）。
fn end_gate_spawn(repo: &Path, state: &Path, id: &str, runner: &str, rules: Option<&Path>) -> Output {
    let (repo_arg, state_arg) = (repo.display().to_string(), state.display().to_string());
    let mut args = vec!["spawn", "--run", id, "--repo", &repo_arg, "--state-dir", &state_arg, "--runner", runner];
    let rules_arg = rules.map(|found| found.display().to_string());
    if let Some(found) = &rules_arg {
        args.extend(["--rules", found]);
    }
    run_pipe(&args)
}

/// 便の `Spawned` で始まる段の記帳の列（intake と審査の段は除く）。
fn spawned_trail(state: &Path, id: &str) -> Vec<(Option<Stage>, Option<String>)> {
    stages(state, id).into_iter().skip_while(|(stage, _)| *stage != Some(Stage::Spawned)).collect()
}

/// 便の段の記帳のうち、門の赤（`Spawned` で detail が `end-gate:red:` で始まる）の件数。
fn red_marks(state: &Path, id: &str) -> usize {
    stages(state, id)
        .iter()
        .filter(|(stage, detail)| {
            *stage == Some(Stage::Spawned) && detail.as_deref().is_some_and(|found| found.starts_with("end-gate:red:"))
        })
        .count()
}

/// 門の record の行（要約の行を除く）。
fn end_gate_records(state: &Path, id: &str) -> Vec<String> {
    end_gate_lines(state, id).into_iter().filter(|line| !line.starts_with("{\"end_gate\":")).collect()
}

/// 要約の行のうち語が `unmeasured` の行の `reason` の字。
fn unmeasured_reason(state: &Path, id: &str) -> String {
    end_gate_lines(state, id)
        .iter()
        .find(|line| line.starts_with("{\"end_gate\":") && line.contains("\"result\":\"unmeasured\""))
        .and_then(|line| line.split_once("\"reason\":\"").map(|(_, rest)| rest.to_owned()))
        .unwrap_or_default()
}

/// 2 回目の stdin の「門の赤」節: 周の番号・赤い行の字・`rc=1`・stub の語を持ち、ほかの行の touches の節の後に在る。
fn assert_red_section(second: &str) {
    assert!(second.contains("\n## 門の赤\n"), "2 回目に節が在る: {second}");
    for word in ["sh verify-lib.sh", "rc=1", "gate-red-word", "周 1"] {
        assert!(second.contains(word), "節は {word} を持つ: {second}");
    }
    let touches_at = second.find("\n## ほかの行の touches\n");
    let red_at = second.find("\n## 門の赤\n");
    assert!(matches!((touches_at, red_at), (Some(t), Some(r)) if t < r), "touches → 門の赤 の順: {second}");
}

/// (a) 1 周目に赤い中身を commit した便は、門の赤を渡されて直す: runner が 2 回起き、2 回目の stdin に「門の赤」節と赤い行の字と
/// `rc=1` と stub の語が在り（1 回目には節が無い）、段の記帳の列は Spawned（base）・Spawned（end-gate:red:1:1）・Spawned
/// （end-gate:1）・Implemented（detail なし）。2 回目の木の履歴に 1 回目の commit が在り、要約の語は red・green の順。
#[test]
fn end_gate_red_round_restarts_the_runner_with_the_red_section_and_lands_green() {
    let (repo, state, id) = gate_run_intake();
    let runner = turn_runner(
        &state,
        &[commit_turn("red-one"), format!("git log --format=%s > \"$D/log-2\"\n{}", commit_turn("green"))],
    );
    let out = end_gate_spawn(&repo, &state, &id, &runner, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(stub_calls(&state), 2, "runner は 2 回起きる");
    let first = stub_stdin(&state, 1);
    assert!(!first.contains("## 門の赤"), "1 回目には節が無い: {first}");
    assert_red_section(&stub_stdin(&state, 2));
    let trail = spawned_trail(&state, &id);
    let details: Vec<Option<&str>> = trail.iter().map(|(_, detail)| detail.as_deref()).collect();
    assert_eq!(trail.len(), 4, "段の記帳は 4 件: {trail:?}");
    assert!(details.first().copied().flatten().is_some_and(|found| found.starts_with("base:")), "{trail:?}");
    assert_eq!(details.get(1).copied().flatten(), Some("end-gate:red:1:1"), "{trail:?}");
    assert_eq!(details.get(2).copied().flatten(), Some("end-gate:1"), "{trail:?}");
    assert_eq!(trail.last().cloned(), Some((Some(Stage::Implemented), None)), "Implemented の detail は無い: {trail:?}");
    let log = fs::read_to_string(stub_dir(&state).join("log-2")).unwrap_or_default();
    assert!(log.lines().any(|line| line == "red-one"), "2 回目の木の履歴に 1 回目の commit が在る: {log}");
    assert_eq!(end_gate_words(&state, &id), ["red", "green"], "要約の語");
    clean(&[&repo, &state]);
}

/// (b) 直さない runner の便は runner が 3 回・門の赤の記帳 2 件で Implemented（要約 exhausted）。gate（偽 lens は PASS）は FAIL で、
/// `verify.jsonl` の record の数は同じ契約を行の値 0 の manifest で通した便と同じ。値 0 の便は runner が 1 回・門の赤の記帳 0 件で、
/// `end-gate.jsonl` は 1 周分の record と要約 exhausted を持つ。
#[test]
fn end_gate_unfixed_run_exhausts_after_two_rounds_and_a_zero_value_fires_once() {
    let lens = |state: &Path| fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"));
    let (repo, state, id) = gate_run_intake();
    let runner = turn_runner(&state, &[commit_turn("red-one"), commit_turn("red-two"), commit_turn("red-three")]);
    let out = end_gate_spawn(&repo, &state, &id, &runner, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(stub_calls(&state), 3, "runner は 3 回起きる");
    assert_eq!(red_marks(&state, &id), 2, "門の赤の記帳は 2 件: {:?}", stages(&state, &id));
    assert_eq!(stages(&state, &id).last().cloned(), Some((Some(Stage::Implemented), None)), "{:?}", stages(&state, &id));
    assert_eq!(end_gate_words(&state, &id), ["red", "red", "exhausted"]);
    let gated = gate_once(&repo, &state, &id, Some(&lens(&state)));
    assert_eq!(gated.status.code(), Some(i32::from(RC_REFUSED)), "赤い便の gate は FAIL: {}", stdout_of(&gated));
    let upper = verify_rows(&state, &id).len();
    clean(&[&repo, &state]);

    let (repo, state, id) = gate_run_intake();
    let rules = end_gate_rules(&state, "rules-end-gate-0.toml", default_slots(), Some(0));
    let runner = turn_runner(&state, &[commit_turn("red-one")]);
    let out = end_gate_spawn(&repo, &state, &id, &runner, Some(&rules));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(stub_calls(&state), 1, "値 0 の便は runner が 1 回");
    assert_eq!(red_marks(&state, &id), 0, "門の赤の記帳は 0 件");
    assert_eq!(end_gate_words(&state, &id), ["exhausted"], "要約は exhausted");
    assert_eq!(end_gate_records(&state, &id).len(), 3, "1 周分の record（write-set・共通 verify・契約の verify）");
    assert_eq!(stages(&state, &id).last().cloned(), Some((Some(Stage::Implemented), None)));
    let gated = gate_once(&repo, &state, &id, Some(&lens(&state)));
    assert_eq!(gated.status.code(), Some(i32::from(RC_REFUSED)), "値 0 の便の gate も FAIL: {}", stdout_of(&gated));
    assert_eq!(verify_rows(&state, &id).len(), upper, "verify.jsonl の record の数は同じ");
    clean(&[&repo, &state]);
}

/// 測れなかった周の共通の assert: runner が 1 回・門の赤の記帳が無く Implemented・要約は unmeasured。理由を返す。
fn assert_unmeasured_once(state: &Path, id: &str, out: &Output) -> String {
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(out), stderr_of(out));
    assert_eq!(stub_calls(state), 1, "runner は 1 回");
    assert_eq!(red_marks(state, id), 0, "門の赤の記帳は無い: {:?}", stages(state, id));
    assert_eq!(stages(state, id).last().cloned(), Some((Some(Stage::Implemented), None)), "{:?}", stages(state, id));
    assert_eq!(end_gate_words(state, id), ["unmeasured"], "要約は unmeasured");
    unmeasured_reason(state, id)
}

/// (c) 測れなかった周は赤が在っても起こし直さない: 箱の中で死ぬ行を赤い行と一緒に持つ便・遮断器の閉じる manifest で起こす
/// 赤い行の便・spawn の前に便の写しを読めない字に書き替えた便は、runner が 1 回で Implemented（要約 unmeasured）。
/// 写しの便の理由は写しの file を名指す。
#[test]
fn end_gate_unmeasured_rounds_never_restart_the_runner() {
    // 箱の中で死ぬ行 + 赤い行。
    let (repo, state) = repo_with_state();
    fs::write(repo.join("verify-lib.sh"), GATE_STUB).unwrap_or_default();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "gate-stub"]);
    let design = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-lib.sh", "sh verify-oom.sh"]"#]);
    let id = intake(&repo, &state, &design);
    let bin_dir = state.join("systemd-bin");
    crate::write_systemd_run_stub(&bin_dir, &state.join("scope-args"));
    let path = format!("{}:{}", bin_dir.display(), crate::toolbox_path(&state));
    let runner = turn_runner(&state, &[commit_turn("red-one")]);
    let out = run_pipe_with_path(
        &path,
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(), "--runner", &runner],
    );
    let reason = assert_unmeasured_once(&state, &id, &out);
    assert!(reason.contains("scope の中で死んだ"), "箱の中の死: {reason}");
    clean(&[&repo, &state]);

    // 遮断器の閉じる manifest（走行可能と待ちの倍率 0・待ちの上限 1 秒）+ 赤い行。
    let (repo, state, id) = gate_run_intake();
    let slots = SlotFixture { runnable_per_core: 0, blocked_per_core: 0, ..default_slots() };
    let rules = end_gate_rules(&state, "rules-end-gate-busy.toml", slots, Some(2));
    let runner = turn_runner(&state, &[commit_turn("red-one")]);
    let out = end_gate_spawn(&repo, &state, &id, &runner, Some(&rules));
    let reason = assert_unmeasured_once(&state, &id, &out);
    assert!(reason.contains("host が混んだまま"), "遮断器の閉じた周: {reason}");
    clean(&[&repo, &state]);

    // 便の写しを読めない字に書き替えた便。
    let (repo, state, id) = gate_run_intake();
    fs::write(vessel_copy(&state, &id), "これは宣言ではない\n").unwrap_or_default();
    let runner = turn_runner(&state, &[commit_turn("red-one")]);
    let out = end_gate_spawn(&repo, &state, &id, &runner, None);
    let reason = assert_unmeasured_once(&state, &id, &out);
    assert!(reason.contains("vessel.toml"), "理由は写しの file を名指す: {reason}");
    clean(&[&repo, &state]);
}

/// (d) 全行が緑の便は runner が 1 回で Implemented（detail なし）・要約 green・門の record の数は gate の段の数（write-set の段 1・
/// 共通 verify の行・契約の verify の行）と同じ。
#[test]
fn end_gate_green_run_fires_once_with_the_gate_stage_count() {
    let (repo, state, id) = gate_run_intake();
    let runner = turn_runner(&state, &[commit_turn("green")]);
    let out = end_gate_spawn(&repo, &state, &id, &runner, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(stub_calls(&state), 1, "runner は 1 回");
    assert_eq!(stages(&state, &id).last().cloned(), Some((Some(Stage::Implemented), None)), "{:?}", stages(&state, &id));
    assert_eq!(red_marks(&state, &id), 0);
    assert_eq!(end_gate_words(&state, &id), ["green"]);
    let fired = end_gate_records(&state, &id).len();
    assert_eq!(fired, 3, "門の record は gate の段の数: {:?}", end_gate_lines(&state, &id));
    let lens = fake_lens(&state.join("lens-ran"), &lens_verdict("PASS"));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "{}", stdout_of(&gated));
    assert_eq!(verify_rows(&state, &id).len(), fired, "門の record の数は gate の record の数と同じ");
    assert!(stdout_of(&out).contains(&format!("run={id} stage=Implemented")), "判定行は 1 周の便と同じ形: {}", stdout_of(&out));
    clean(&[&repo, &state]);
}

/// (e) 行の無い manifest で起こす赤い行の便は、門を撃てず runner が 1 回・要約 unmeasured で、理由が行の id を名指す。
#[test]
fn end_gate_manifest_without_the_row_is_unmeasured_naming_the_row() {
    let (repo, state, id) = gate_run_intake();
    let rules = end_gate_rules(&state, "rules-end-gate-absent.toml", default_slots(), None);
    let runner = turn_runner(&state, &[commit_turn("red-one")]);
    let out = end_gate_spawn(&repo, &state, &id, &runner, Some(&rules));
    let reason = assert_unmeasured_once(&state, &id, &out);
    assert!(reason.contains("runner.end_gate_rounds"), "理由は行の id を名指す: {reason}");
    assert!(end_gate_records(&state, &id).is_empty(), "撃たない周は要約の 1 行だけ: {:?}", end_gate_lines(&state, &id));
    clean(&[&repo, &state]);
}

/// 門を撃っている間、解放の file が在るまで待つ stub（撃ち始めの印は git の共通 dir・上限 60 秒）。
const HOLD_STUB: &str = "common=\"$(git rev-parse --git-common-dir)\"\ntouch \"$common/hold-started\"\ni=0\n\
     while [ ! -f \"$common/hold-release\" ] && [ \"$i\" -lt 600 ]; do sleep 0.1; i=$((i+1)); done\nexit 0\n";

/// (f) 門の間は run dir に `end-gate.pid` が在り、その間の `resume --runner` は rc 1 で `end-gate=alive` を持って runner を起こさず
/// （偽 runner の回数は 1 のまま）、門を抜けた spawn は Implemented で印が無い。
#[test]
fn end_gate_mark_stands_while_the_gate_fires_and_refuses_a_resume() {
    let (repo, state) = repo_with_state();
    fs::write(repo.join("verify-hold.sh"), HOLD_STUB).unwrap_or_default();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "hold-stub"]);
    let design = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-hold.sh"]"#]);
    let id = intake(&repo, &state, &design);
    let runner = turn_runner(&state, &[commit_turn("green")]);
    let (repo_arg, state_arg) = (repo.display().to_string(), state.display().to_string());
    let mut child = pipe_cmd(&["spawn", "--run", &id, "--repo", &repo_arg, "--state-dir", &state_arg, "--runner", &runner])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn を起こせる: {err}"));
    let (started, release) = (repo.join(".git").join("hold-started"), repo.join(".git").join("hold-release"));
    let begun = Instant::now();
    while !started.exists() {
        assert!(child.try_wait().ok().flatten().is_none(), "門を撃つ前に spawn が終わった");
        assert!(begun.elapsed() < Duration::from_secs(60), "門が撃ち始めない");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(end_gate_mark_path(&state, &id).exists(), "門の間は印が在る");
    let resumed = run_pipe(&["resume", "--run", &id, "--repo", &repo_arg, "--state-dir", &state_arg, "--runner", &runner]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_REFUSED)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    assert!(stdout_of(&resumed).contains(&format!("run={id} end-gate=alive pid={}", child.id())), "{}", stdout_of(&resumed));
    assert_eq!(stub_calls(&state), 1, "偽 runner は起きない");
    fs::write(&release, "").unwrap_or_default();
    let status = child.wait().unwrap_or_else(|err| panic!("spawn を待てる: {err}"));
    assert!(status.success(), "解放の後の spawn は rc 0");
    assert_eq!(stages(&state, &id).last().cloned(), Some((Some(Stage::Implemented), None)), "{:?}", stages(&state, &id));
    assert!(!end_gate_mark_path(&state, &id).exists(), "門を抜けた spawn は印を外す");
    assert_eq!(stub_calls(&state), 1, "runner は 1 回のまま");
    clean(&[&repo, &state]);
}

// ───── runner の stdin に前の便の gate の FAIL の節（設計 pipeline.md §68・行 bl・接頭辞 `prior_fail_`） ─────
//
// 同じ bead の 1 本目が gate の FAIL で終端した後に、同じ bead で受付を撃ち直して 2 本目を作る（release の後の受付と同じ形）。
// run id は `<bead>-<秒>` なので、2 本目の受付は 1 本目と秒を分ける。偽 runner は turn ごとに stdin を写し（`turn_runner`）、
// 偽 lens は evidence と findings を指定した FAIL を返す。契約の verify 行は緑の stub なので lens まで届く。

/// 1 本目の便の材料（偽 runner は 3 本の便で共有する＝stdin の番号は spawn の通し番号）。
struct PriorRun {
    /// 対象 repo。
    repo: PathBuf,
    /// 置き場。
    state: PathBuf,
    /// 1 本目の run id。
    id: String,
    /// 偽 runner の起動行。
    runner: String,
}

/// 偽 lens の判定の行（FAIL・`evidence` と `findings` を指定）。
fn fail_verdict(evidence: &str, findings: &str) -> String {
    format!(
        "{{\"verdict\":\"FAIL\",\"evidence\":\"{evidence}\",\"findings\":\"{findings}\",\"population\":\"{FAKE_POPULATION}\"}}"
    )
}

/// 1 本目の gate の FAIL の findings（`teeth-nonvacuous` だけ 2・他の 7 観点は 0）。
const PRIOR_FINDINGS: &str =
    "contract-fit:0,teeth-nonvacuous:2,constitution:0,delete:0,stdlib:0,native:0,yagni:0,shrink:0";

/// 1 本目を緑で Implemented まで通し、`evidence` と `findings` を返す偽 lens で gate の FAIL（rc 1）にして終端させる。
/// `second` は偽 runner の 2 回目の turn の本文（3 回目は緑の commit）。
fn prior_first(evidence: &str, findings: &str, second: String) -> PriorRun {
    let (repo, state, id) = gate_run_intake();
    let runner = turn_runner(&state, &[commit_turn("green"), second, commit_turn("green")]);
    let out = end_gate_spawn(&repo, &state, &id, &runner, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    let lens = fake_lens(&state.join("lens-fail"), &fail_verdict(evidence, findings));
    let gated = gate_once(&repo, &state, &id, Some(&lens));
    assert_eq!(gated.status.code(), Some(i32::from(RC_REFUSED)), "1 本目は gate の FAIL: {}", stdout_of(&gated));
    assert!(
        gated_details(&state, &id).iter().any(|detail| detail.starts_with("verdict:FAIL")),
        "Gated の detail は verdict:FAIL: {:?}",
        gated_details(&state, &id)
    );
    PriorRun { repo, state, id, runner }
}

/// 同じ bead で受付を撃ち直した便の run id（run id の秒を前の便から分ける）。
fn prior_intake(prior: &PriorRun) -> String {
    std::thread::sleep(Duration::from_millis(1100));
    intake_bead(&prior.repo, &prior.state, &design_pointer(), "s2-2e5")
}

/// 受付済みの便 `id` を偽 runner で spawn する（rc 0 を要求する）。
fn prior_spawn(prior: &PriorRun, id: &str) {
    let out = end_gate_spawn(&prior.repo, &prior.state, id, &prior.runner, None);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
}

/// 同じ bead で受付を撃ち直して spawn した便の run id。
fn prior_next(prior: &PriorRun) -> String {
    let id = prior_intake(prior);
    prior_spawn(prior, &id);
    id
}

/// stdin から「## 前の便の gate の FAIL」節の本文（次の節の見出しの手前まで）を取る（節が無ければ `None`）。
fn prior_body(stdin: &str) -> Option<String> {
    let after = stdin.split("\n## 前の便の gate の FAIL\n").nth(1)?;
    Some(after.split("\n## ").next().unwrap_or_default().to_owned())
}

/// (a) 1 本目が gate の FAIL（evidence に固有の語・findings は `teeth-nonvacuous:2` で他は 0）で終端した後の、同じ契約の 2 本目の
/// stdin は、ほかの行の touches の節の後に節を持つ。節は 1 本目の run id・evidence の語・`teeth-nonvacuous:2` を持ち、
/// `contract-fit:0` を持たない。1 本目の stdin には節が無い。
#[test]
fn prior_fail_section_carries_the_verdict_of_the_previous_run_after_touches() {
    let prior = prior_first("hole-in-the-teeth-xyzzy", PRIOR_FINDINGS, commit_turn("green"));
    let second = prior_next(&prior);
    assert_ne!(second, prior.id, "2 本目は別の便");
    assert!(prior_body(&stub_stdin(&prior.state, 1)).is_none(), "1 本目の stdin には節が無い");
    let stdin = stub_stdin(&prior.state, 2);
    let body = prior_body(&stdin).unwrap_or_else(|| panic!("2 本目の stdin に節が在る: {stdin}"));
    for word in [prior.id.as_str(), "hole-in-the-teeth-xyzzy", "teeth-nonvacuous:2"] {
        assert!(body.contains(word), "節は {word} を持つ: {body}");
    }
    assert!(!body.contains("contract-fit:0"), "0 件の観点は載らない: {body}");
    assert!(body.contains("findings: teeth-nonvacuous:2\n"), "件数が 0 でない語だけ: {body}");
    let (touches_at, section_at) = (stdin.find("\n## ほかの行の touches\n"), stdin.find("\n## 前の便の gate の FAIL\n"));
    assert!(matches!((touches_at, section_at), (Some(t), Some(s)) if t < s), "touches → 前の便の gate の FAIL の順: {stdin}");
    clean(&[&prior.repo, &prior.state]);
}

/// (b) 1 本目と契約の字が違う（行の title を変えた）2 本目の stdin には節が無い。
#[test]
fn prior_fail_section_is_absent_when_the_contract_text_differs() {
    let prior = prior_first("hole-in-the-teeth-xyzzy", PRIOR_FINDINGS, commit_turn("green"));
    write_contract(&prior.repo, &["verify", "title"], &[GATE_VERIFY, r#"title = "別の題""#]);
    let _second = prior_next(&prior);
    let stdin = stub_stdin(&prior.state, 2);
    assert!(stdin.contains("別の題"), "2 本目は変えた契約を読む: {stdin}");
    assert!(prior_body(&stdin).is_none(), "契約の字が違う便に節は無い: {stdin}");
    clean(&[&prior.repo, &prior.state]);
}

/// (c) 1 本目が gate の FAIL・2 本目が runner の rc 非 0 で Failed の後の、同じ契約の 3 本目の stdin には節が無い（直前の 1 本だけを見る）。
#[test]
fn prior_fail_section_looks_only_at_the_previous_run() {
    let prior = prior_first("hole-in-the-teeth-xyzzy", PRIOR_FINDINGS, "exit 1".to_owned());
    let second = prior_intake(&prior);
    let out = end_gate_spawn(&prior.repo, &prior.state, &second, &prior.runner, None);
    assert!(prior_body(&stub_stdin(&prior.state, 2)).is_some(), "2 本目には節が在る: {}", stub_stdin(&prior.state, 2));
    assert_eq!(
        stages(&prior.state, &second).last().map(|(stage, _)| *stage),
        Some(Some(Stage::Failed)),
        "2 本目は Failed: {} / {}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let _third = prior_next(&prior);
    assert_eq!(stub_calls(&prior.state), 3, "runner は 3 回起きた");
    let stdin = stub_stdin(&prior.state, 3);
    assert!(prior_body(&stdin).is_none(), "直前の便が Failed の 3 本目に節は無い: {stdin}");
    clean(&[&prior.repo, &prior.state]);
}

/// (d) 1 本目の gate の FAIL の後に `verdict.json` を読めない字に書き替えた便の 2 本目の節は、`verdict.json` を名指す理由の 1 行で、
/// evidence の語を持たない。
#[test]
fn prior_fail_section_says_why_when_the_verdict_is_unreadable() {
    let prior = prior_first("hole-in-the-teeth-xyzzy", PRIOR_FINDINGS, commit_turn("green"));
    // 読めない判定の便は live と読まれ受付を断るので、書き替えは 2 本目の受付の後・spawn の前に行う。
    let second = prior_intake(&prior);
    fs::write(prior.state.join("pipe").join(&prior.id).join("verdict.json"), "これは判定ではない\n").unwrap_or_default();
    prior_spawn(&prior, &second);
    let stdin = stub_stdin(&prior.state, 2);
    let body = prior_body(&stdin).unwrap_or_else(|| panic!("読めない判定の周も節は在る: {stdin}"));
    assert!(body.contains("verdict.json を読めない"), "verdict.json を名指す理由: {body}");
    assert!(!body.contains("hole-in-the-teeth-xyzzy") && !body.contains("findings:"), "判定の語は載らない: {body}");
    assert_eq!(body.trim_end().lines().count(), 2, "1 文の行と理由の 1 行: {body}");
    clean(&[&prior.repo, &prior.state]);
}

/// (e) evidence が 3000 字の判定の後の 2 本目の節の evidence は 2000 字で切れ、切った字数の 1 行を持つ。1 本目の後に `verdict.json` を
/// `findings` の無い形に書き替えた便の 2 本目の節は `findings: なし` の 1 行を持つ。
#[test]
fn prior_fail_section_cuts_long_evidence_and_says_none_without_findings() {
    let prior = prior_first(&"z".repeat(3000), PRIOR_FINDINGS, commit_turn("green"));
    let _second = prior_next(&prior);
    let stdin = stub_stdin(&prior.state, 2);
    let body = prior_body(&stdin).unwrap_or_else(|| panic!("2 本目の stdin に節が在る: {stdin}"));
    let line = body.lines().find(|line| line.starts_with("- evidence: ")).unwrap_or_default();
    assert_eq!(line.matches('z').count(), 2000, "evidence は 2000 字で切れる");
    assert!(body.contains("- evidence は 1000 字を切った"), "切った字数の 1 行: {body}");
    clean(&[&prior.repo, &prior.state]);

    let prior = prior_first("hole-in-the-teeth-xyzzy", PRIOR_FINDINGS, commit_turn("green"));
    let verdict = prior.state.join("pipe").join(&prior.id).join("verdict.json");
    fs::write(&verdict, "{\"verdict\":\"FAIL\",\"evidence\":\"no-lens-ran\"}\n").unwrap_or_default();
    let _second = prior_next(&prior);
    let stdin = stub_stdin(&prior.state, 2);
    let body = prior_body(&stdin).unwrap_or_else(|| panic!("2 本目の stdin に節が在る: {stdin}"));
    assert!(body.contains("no-lens-ran"), "書き替えた evidence: {body}");
    assert!(body.contains("- findings: なし\n"), "findings の無い判定は findings: なし: {body}");
    assert!(!body.contains("evidence は"), "短い evidence に切った字数の行は無い: {body}");
    clean(&[&prior.repo, &prior.state]);
}
