// flip-check: moved s2-07l.264
//! 便を起こす側の歯: `pipe_spawn_` / `pipe_approval_` / `pipe_question_` / `pipe_resume_` / `pipe_report_`・
//! fake runner を toy repo で回す縦 1 本（`pipe_e2e_` / `pipe_five_` / `pipe_guard_`）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く（歯の本文は移しただけ・`s2-07l.264`）。

use super::*;
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
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_pipe_in_pane(args: &[&str]) -> Output {
    Command::new(bin())
        .arg("pipe")
        .args(args)
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

    // consumer の plugin は root の `consumer/` へ写る（器の plugin は `<NAME>/`・root の
    // 形は `pipe_spawn_plugin_` の歯が測る）。
    let consumer = plugin.join(CONSUMER);
    for (dir, name, body) in [
        (".claude-plugin", "plugin.json", PLUGIN_JSON),
        ("hooks", "hooks.json", HOOKS_JSON),
    ] {
        let source = fs::read(worktree.join(dir).join(name)).expect("worktree 側を読める");
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
        vec![".claude-plugin".to_owned(), "hooks".to_owned()],
        "写しは plugin の 2 dir だけ（母集団 {} entry）",
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
    fs::read(root.join(dir).join(name)).expect("tracked の器の plugin を読める")
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
fn repo_with_files(files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
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
        (".claude-plugin/plugin.json", &manifest),
        ("hooks/hooks.json", HOOKS_JSON),
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
    let cases: [(&str, Vec<(&str, &str)>); 4] = [
        ("hooks だけ", vec![("hooks/hooks.json", HOOKS_JSON)]),
        ("plugin.json だけ", vec![(".claude-plugin/plugin.json", PLUGIN_JSON)]),
        (
            "name が入れ子にだけ在る",
            vec![
                (".claude-plugin/plugin.json", "{\"meta\":{\"name\":\"toy-plugin\"}}\n"),
                ("hooks/hooks.json", HOOKS_JSON),
            ],
        ),
        (
            "name が文字列でない",
            vec![(".claude-plugin/plugin.json", "{\"name\":7}\n"), ("hooks/hooks.json", HOOKS_JSON)],
        ),
    ];
    for (label, files) in cases {
        let (repo, state) = repo_with_files(&files);
        let (plugin, _) = spawn_plugin_run(&repo, &state);
        assert!(!plugin.join(CONSUMER).exists(), "{label}: consumer/ を作らない");
        assert_eq!(dir_names(&plugin), vec![NAME.to_owned()], "{label}: root は器の 1 本だけ");
        clean(&[&repo, &state]);
    }
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
    Command::new(bin())
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

/// (a) `pipe run` を Implemented の時点で process group ごと SIGKILL → 中断点は event log に在り、別 process の
/// `resume` 2 回（gate → land）で Landed まで通る。人手の event 0・殺す前の結果（runner の commit）は保たれる（C9）。
#[test]
fn pipe_resume_kill_at_implemented_resumes_to_landed_in_new_process() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let (id, _killed) = killed_at_implemented(&repo, &state, &design);

    // 中断の現在地: Implemented が 1 件・終端の記帳は無い・show も Implemented を名乗る。
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).unwrap_or_default();
    assert_eq!(stage_count(&state, &id, Stage::Implemented), 1, "Implemented は 1 件: {log}");
    assert_eq!(kind_count(&state, &id, EventKind::RunDone), 0, "RunDone は 0 件: {log}");
    assert_eq!(log.matches("\"stage\":\"Landed\"").count(), 0, "Landed の字面は 0 件: {log}");
    let shown = show_line(&repo, &state, &id);
    assert!(shown.contains("stage=Implemented"), "中断点は event log に在る: {shown}");

    // 続き: 別 process が置き場だけを読んで gate → land を引く。
    let marker = state.join("lens-ran-after-kill");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "殺した後の Implemented → gate: {}", stderr_of(&gated));
    assert!(stdout_of(&gated).contains("verdict=PASS"), "{}", stdout_of(&gated));
    let landed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "Gated → land: {}", stderr_of(&landed));
    assert!(stdout_of(&landed).contains("landed="), "{}", stdout_of(&landed));
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    let log = fs::read_to_string(state.join("fleet").join("events.jsonl")).unwrap_or_default();
    assert_eq!(done_count(&state, &id, Stage::Landed), 1, "RunDone stage=Landed が 1 件: {log}");
    assert!(!log.contains("\"actor\":\"human\""), "人手なしで継いだ（C9）: {log}");
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(
        git(&repo, &["rev-list", "--count", &format!("{base}..{new}")]),
        "1",
        "殺す前の結果（runner の commit 1 本）が main に保たれている（C9）"
    );
    clean(&[&repo, &state]);
}

/// (b) 中断が**外されなかった lock**（殺された writer が残す形＝所有者の pid の 10 進 1 行）を残した周からの
/// resume。所有者は死んでいる（前提 assert）。rc 0 で Landed まで通り、lock file は残らない。
///
/// 警告の字面はここでは測らない（pipe の funnel は store の警告を捨てる＝`tests/e2e/fleet.rs` の歯が持つ）。
/// 時刻の窓を race で狙わず、殺した周に器が残しうる状態を構成して撃つ。
#[test]
fn pipe_resume_kill_dead_owner_lock_does_not_block_resume() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let (id, killed) = killed_at_implemented(&repo, &state, &design);
    assert!(!proc_alive(killed), "前提: 殺した pid {killed} は生きていない");
    let lock = state.join("fleet").join("events.jsonl.lock");
    fs::write(&lock, format!("{killed}\n")).expect("死んだ所有者の lock を置ける");

    let marker = state.join("lens-ran-after-kill");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let gated = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--lens", &lens,
    ]);
    assert_eq!(
        gated.status.code(),
        Some(i32::from(RC_OK)),
        "所有者の死んだ lock は resume を止めない: {}",
        stderr_of(&gated)
    );
    assert!(stdout_of(&gated).contains("verdict=PASS"), "{}", stdout_of(&gated));
    assert!(!lock.exists(), "死んだ所有者の lock は外されて残らない");
    let landed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "Gated → land: {}", stderr_of(&landed));
    assert!(stdout_of(&landed).contains("landed="), "{}", stdout_of(&landed));
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "段は Landed");
    assert!(!lock.exists(), "land の後も lock は残らない");
    clean(&[&repo, &state]);
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

/// (a) runner が死んだ `Spawned` の便に `resume --runner` を撃つと rc 0 で、`SeatStopped detail=runner-dead` を 1 件・
/// `Spawned detail=account:a1,resume:runner-dead` を 1 件記帳し、**同じ worktree**（`worktree=` の path が不変）で
/// 2 回目の runner が起きて `Implemented` に至る。1 回目が書いた未 commit の file は worktree に残る（N1）。
#[test]
fn pipe_resume_kill_at_spawned_respawns_in_same_worktree() {
    let (repo, state) = repo_with_state();
    // 行の commit が main を進めるので、base は**便を起こした後**に読む（契約 (b)）。
    let (id, _runner_pid, runner) = killed_at_spawned(&repo, &state, &[IMPLEMENT.to_owned()]);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    let before = show_line(&repo, &state, &id);

    let resumed = resume_dead_runner(&repo, &state, &id, &runner);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    assert!(stdout_of(&resumed).contains(&format!("run={id} next=spawn account=a1")), "{}", stdout_of(&resumed));
    assert!(stdout_of(&resumed).contains(&format!("run={id} stage=Implemented")), "{}", stdout_of(&resumed));
    assert_eq!(runner_dead_count(&state, &id), 1, "runner の死亡を 1 件記帳する");
    assert_eq!(
        spawned_details(&state, &id),
        vec![format!("base:{base}"), "account:a1,resume:runner-dead".to_owned()],
        "起こし直しの記帳は理由を runner-dead と名乗る"
    );
    assert_eq!(stub_calls(&state), 2, "runner を 1 回起こし直した");
    let after = show_line(&repo, &state, &id);
    assert!(after.contains("stage=Implemented"), "2 回目の runner で Implemented: {after}");
    let worktree_of_line = |line: &str| line.split("worktree=").nth(1).map(str::to_owned);
    assert_eq!(worktree_of_line(&before), worktree_of_line(&after), "同じ worktree で起き直る: {before} / {after}");
    let worktree = worktree_of(&repo, &id);
    assert!(worktree.join(WIP_FILE).exists(), "1 回目の未 commit の file を消さない（N1）");
    assert_eq!(git(&worktree, &["rev-list", "--count", "refs/heads/main..HEAD"]), "1", "2 回目の commit が同じ worktree に載る");
    assert!(!events(&state).iter().any(|event| event.actor == "human"), "人手なしで継いだ（C9）");
    clean(&[&repo, &state]);
}

/// (b) (a) の 2 回目の stdin: 「途中再開」節が契約の後に在り、理由の行（runner の死亡）と未 commit の file 名の行
/// （`git status --porcelain` の形）が載る。止まった時刻は `SeatStopped detail=runner-dead` の ts。
#[test]
fn pipe_resume_kill_at_spawned_lists_uncommitted_in_prompt() {
    let (repo, state) = repo_with_state();
    let (id, _runner_pid, runner) = killed_at_spawned(&repo, &state, &[IMPLEMENT.to_owned()]);
    let resumed = resume_dead_runner(&repo, &state, &id, &runner);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    // 2 回目の runner の行儀よい `SeatStopped` が後ろに在るので、理由つきの行を名指して読む。
    let stopped_at = events(&state)
        .into_iter()
        .filter(|event| {
            event.run == id && event.kind == EventKind::SeatStopped && event.detail.as_deref() == Some("runner-dead")
        })
        .map(|event| event.ts)
        .next_back()
        .unwrap_or_default();
    let prompt = stub_stdin(&state, 2);
    let (contract_at, resume_at) = (prompt.find("goal = "), prompt.find("## 途中再開"));
    assert!(matches!((contract_at, resume_at), (Some(c), Some(r)) if c < r), "契約 → 途中再開 の順: {prompt}");
    assert!(!prompt.contains("## 回答") && !prompt.contains("## 追随"), "質問も追随も無い便: {prompt}");
    let reason = format!("- 前の turn は {stopped_at} に runner の死亡で止まった（process が消えた）");
    assert!(!stopped_at.is_empty() && prompt.contains(&reason), "理由の行（SeatStopped の ts）: {prompt}");
    assert!(prompt.contains("未 commit の変更（worktree に在る・消さない・続きから commit する）:"), "一覧の見出し: {prompt}");
    assert!(prompt.contains(&format!("\n  - ?? {WIP_FILE}")), "未 commit の file 名の行: {prompt}");
    assert!(prompt.contains("base からの commit（worktree に在る・やり直さない）: なし"), "commit の無い便は なし: {prompt}");
    assert!(!stub_stdin(&state, 1).contains("## 途中再開"), "初回の turn には節が無い");
    clean(&[&repo, &state]);
}

/// (c) `pipe run` の group だけを殺して runner を生かした周: `resume --runner` は rc 1・判定行 `runner=alive pid=<pid>`・
/// event 0 件（母集団 = 撃つ前後の event 数）で、runner を 2 本にしない。その後 `reap_own` で runner を畳む。
#[test]
fn pipe_resume_kill_at_spawned_refuses_while_runner_alive() {
    let (repo, state) = repo_with_state();
    let contract = write_set_contract(&repo, "wip", &["src/lib.rs", &format!("+{WIP_FILE}")]);
    let pid_file = state.join("wip-sleep.pid");
    let runner = turn_runner(&state, &[wip_then_wait_turn(&pid_file), IMPLEMENT.to_owned()]);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let mut child = spawn_run_child(&repo, &state, &contract, &runner, &lens);
    let (id, runner_pid, sleeper) = wait_for_seat_spawned(&state, &mut child, &pid_file);
    kill_group(child.id());
    child.wait().ok();
    assert!(proc_alive(runner_pid), "前提: runner {runner_pid} は生きている");

    let before = event_count(&state);
    let resumed = resume_dead_runner(&repo, &state, &id, &runner);
    let alive = proc_alive(runner_pid);
    let calls = stub_calls(&state);
    let after = event_count(&state);
    // 片付け: 自分の子孫の `sleep` を止める → runner の script が `wait` から戻って畳まれる。
    reap_own(sleeper);
    wait_gone(runner_pid);

    assert_eq!(resumed.status.code(), Some(i32::from(RC_REFUSED)), "生きている runner の便は断る: {}", stdout_of(&resumed));
    assert!(stdout_of(&resumed).contains(&format!("run={id} runner=alive pid={runner_pid}")), "判定行: {}", stdout_of(&resumed));
    assert!(stderr_of(&resumed).contains("起きている"), "理由を名乗る: {}", stderr_of(&resumed));
    assert!(alive, "runner を殺さない");
    assert_eq!(calls, 1, "runner を 2 本にしない");
    assert_eq!(after, before, "event 0 件");
    assert_eq!(runner_dead_count(&state, &id), 0, "生きている便に runner-dead を書かない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_resume_reports_next_gate_on_inconclusive() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    // 審査が残した lens の写し（`lens.toml`・設計 pipeline.md §26）も外す＝写しも flag も無い世界。
    fs::remove_file(vessel::pipe::run_dir(&state, &id).join("lens.toml")).expect("審査の写しを外せる");
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
        "intake", "--design", &path, "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
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
    let ok = intake_raw(&repo, &state, &listed, "b");
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
        "run", "--design", &path, "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
        "--runner", &runner_cmd(&marker), "--lens", &review_lens_pass(&state),
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
    let design = write_contract(&repo, &[], &[]);
    toy_denied(&repo, &state, &design);
    clean(&[&repo, &state]);
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
        "run", "--design", &path, "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules, "--runner", &question_runner(), "--lens", &review_lens_pass(&state),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "{}", stderr_of(&out));
    let id = run_id_of(&out);
    assert!(stdout_of(&out).contains(&format!("question={id}")), "判定行に question=: {}", stdout_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Questioned"));
    clean(&[&repo, &state]);
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

/// 偽 `systemd-run` の argv を 1 起動 1 行で写す file 名。
const TERMINAL_SCOPE_CALLS: &str = "terminal-systemd-run-calls";

/// **包める周に固定する** PATH（偽 `systemd-run` が argv を写し `--` の後ろを exec する・`gate.rs` の同型）。
/// 偽の包みの中では `/proc/self/cgroup` が unit の scope と一致しないので、包みの終端行は出ない＝kernel の証拠は
/// runner が自分で書いた行だけになる。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn terminal_confined_path(state: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = state.join("terminal-systemd-bin");
    fs::create_dir_all(&bin_dir).expect("stub の dir を作れる");
    let shim = bin_dir.join("systemd-run");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nwhile [ $# -gt 0 ] && [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
        state.join(TERMINAL_SCOPE_CALLS).display()
    );
    fs::write(&shim, script).expect("stub を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("stub に実行権を付ける");
    format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default())
}

/// 包める PATH で `runner` の便を 1 本 spawn し、（便 id・spawn の出力）を返す。前提として runner が包めた周で
/// 起きたことを assert する（包めない周の「oom-kill 0 件」で空虚に充足しない）。
fn spawn_confined(repo: &Path, state: &Path, runner: &str) -> (String, Output) {
    let design = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &design);
    let out = run_pipe_with_path(
        &terminal_confined_path(state),
        &["spawn", "--run", &id, "--repo", &repo.display().to_string(),
          "--state-dir", &state.display().to_string(), "--runner", runner],
    );
    let calls = fs::read_to_string(state.join(TERMINAL_SCOPE_CALLS)).unwrap_or_default();
    assert!(
        calls.lines().any(|line| line.contains("--scope") && line.contains("-runner-")),
        "前提: runner は包めた周で起きた: {calls:?}"
    );
    (id, out)
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
