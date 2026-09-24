//! `hook::host_guard` の歯。**本体は `host_guard.rs`** で、ここには test だけが在る。
//!
//! 分けたのは憲法 C4（1 file の上限）である——`host_guard.rs` が 1388 行まで育ち、同じ file へ
//! 行を足す契約を受けられなくなった（`s2-07l.592`）。`#[path]` で `host_guard` の子 module として
//! 取り込むので、module path は `hook::host_guard::tests` のまま＝歯の名前は 1 つも変わらない。

// 純粋な移動（`host_guard.rs` の test 区間から歯を足さずに写した・s2-07l.592）。
// flip-check: moved s2-07l.592

use super::{
    decide, judge, HostGuardDecision, Kind, Protected, Scene, Unreadable, KINDS, LEDGER_ROW, PROTECTED, RM_ROW, TMUX_ROW,
    WORD_ROWS,
};
use crate::hook::command::{self, denied_in, CommandDecision};
use crate::hook::ledger_guard;
use crate::name::NAME;
use crate::order::is_declaration_order;
use crate::rules::manifest::Manifest;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// fixture の裁定 id。
const RULING: &str = "user 2026-09-19T15:28Z";

/// `runner.denied_commands` の fixture の語列（git の語列は host_guard.git と重複・tmux の語列は持たない）。
const RUNNER: &[&str] = &["git push --force", "cargo mutants"];

/// 1 行の本文。
fn row(id: &str, kind: &str, value: &[&str], enabled: bool) -> String {
    let quoted: Vec<String> = value.iter().map(|item| format!("\"{item}\"")).collect();
    format!(
        "\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = [{}]\nenabled = {enabled}\nruling = \"{RULING}\"\nruled_at = \"2026-09-19\"\n",
        quoted.join(", ")
    )
}

/// `runner.denied_commands` と host_guard の語列の 3 行と `extra` の本文を持つ manifest（host_guard.tmux の enabled は
/// 引数・`drop` の行は持たない）。
fn manifest_with(tmux_enabled: bool, drop: Option<&str>, extra: &str) -> Manifest {
    let mut text = format!("schema = 1\n{}", row(command::ROW, "RunnerDeniedCommands", RUNNER, true));
    for (id, value, enabled) in [
        (super::GIT_ROW, &["git push --force", "git reset --hard"][..], true),
        (TMUX_ROW, &["tmux kill-server"][..], tmux_enabled),
        (LEDGER_ROW, &["bd delete"][..], true),
    ] {
        if drop != Some(id) {
            text.push_str(&row(id, "HostGuardDeniedCommands", value, enabled));
        }
    }
    text.push_str(extra);
    Manifest::parse(&text).unwrap_or_else(|errors| panic!("fixture の manifest を読める: {errors:?}"))
}

/// 語列の 3 行だけの manifest（rm の行を持たない＝rm の segment が無い command の判定は行 b と同じ）。
fn manifest(tmux_enabled: bool, drop: Option<&str>) -> Manifest {
    manifest_with(tmux_enabled, drop, "")
}

/// 語列の判定の場（rm の segment を持たない command には効かない）。
fn nowhere() -> Scene<'static> {
    Scene { cwd: Path::new("/nonexistent"), state_dir: Path::new("/nonexistent/state"), git: Path::new("git") }
}

/// Bash の判定の (what, line)。Allow なら `None`。
fn denied(command: &str, manifest: &Manifest) -> Option<(String, String)> {
    match judge("Bash", command, manifest, &nowhere()) {
        HostGuardDecision::Deny { what, line } => Some((what, line)),
        HostGuardDecision::Allow => None,
    }
}

/// Bash の payload（command は JSON の escape を通す）。
fn bash(command: &str) -> String {
    format!("{{\"cwd\":\"/tmp\",\"tool_name\":\"Bash\",\"tool_input\":{{\"command\":{}}}}}", crate::fleet::json_lite::quote(command))
}

/// 種類は閉じた 5 値で、const slice は宣言順に 5 本（git → rm → tmux → 台帳 → 自身の設定）。行を持たないのは自身の設定だけ。
#[test]
fn host_guard_kind_slice_is_the_five_kinds_in_declaration_order() {
    assert!(is_declaration_order(KINDS, |kind| kind as usize), "KINDS は宣言順: {KINDS:?}");
    let words: Vec<&str> = KINDS.iter().map(|kind| kind.as_str()).collect();
    assert_eq!(words, ["git", "rm", "tmux", "ledger", "settings"], "5 値の語");
    let rows: Vec<Option<&str>> = KINDS.iter().map(|kind| kind.row()).collect();
    assert_eq!(rows, [Some(super::GIT_ROW), Some(RM_ROW), Some(TMUX_ROW), Some(LEDGER_ROW), None], "行 id");
    assert!(!is_declaration_order(&[Kind::Rm, Kind::Git], |kind| kind as usize), "述語は並べ替えを落とす");
}

/// 引用符の中に区切りを含む command で 2 つの口の判定が割れる（分割だけが 2 本・照合は同じ 1 関数）: command guard の
/// 従来の分割は引用符の中の `;` で切って当て、host-guard は引用符を解くので当てない。逆に引用符で包んだ flag は
/// host-guard だけが解いて当てる。
#[test]
fn host_guard_kind_quoted_separator_splits_the_two_mouths() {
    let manifest = manifest(true, None);
    let runner: Vec<String> = RUNNER.iter().map(|item| (*item).to_owned()).collect();
    let quoted = "echo \"x; git push --force origin\"";
    assert!(denied_in(quoted, &runner).is_some(), "command guard の分割は引用符の中の ; で切る");
    assert_eq!(denied(quoted, &manifest), None, "host-guard は引用符の中を 1 語に読む");
    let wrapped = "git push \"--force\" origin main";
    assert!(denied_in(wrapped, &runner).is_none(), "command guard は引用符を解かない");
    let (what, _) = denied(wrapped, &manifest).unwrap_or_else(|| panic!("host-guard は引用符を解いて当てる"));
    assert_eq!(what, "host-guard-deny git");
}

/// `NAME=value` の前置きを読み飛ばして当てる（前置きが 2 つでも）。
#[test]
fn host_guard_kind_skips_env_assignment_prefix() {
    let manifest = manifest(true, None);
    for (command, what) in [
        ("NAME=1 git push --force origin main", "host-guard-deny git"),
        ("A=1 B=x tmux kill-server", "host-guard-deny tmux"),
        ("cd /tmp && GIT_DIR=x git reset --hard", "host-guard-deny git"),
    ] {
        let found = denied(command, &manifest).map(|(found, _)| found);
        assert_eq!(found.as_deref(), Some(what), "{command}");
    }
}

/// git・tmux・台帳の語列がそれぞれ自分の行の id と裁定 id を名指し、断りの 1 行は 5 欄（kind / hit / row / ruling / 代わりの
/// 経路）を持つ。
#[test]
fn host_guard_kind_each_word_kind_names_its_own_row() {
    let manifest = manifest(true, None);
    let (what, line) = denied("git push origin main --force", &manifest).unwrap_or_else(|| panic!("git は断る"));
    assert_eq!(what, "host-guard-deny git");
    assert_eq!(
        line,
        format!("{NAME}: host-guard deny kind=git hit=git push --force row=host_guard.git ruling={RULING} — {}", Kind::Git.route())
    );
    for (command, kind, id) in [("tmux -L x kill-server", Kind::Tmux, TMUX_ROW), ("bd delete s2-1", Kind::Ledger, LEDGER_ROW)] {
        let (what, line) = denied(command, &manifest).unwrap_or_else(|| panic!("{command} は断る"));
        assert_eq!(what, format!("host-guard-deny {}", kind.as_str()), "{command}");
        assert!(line.contains(&format!(" kind={} ", kind.as_str())) && line.contains(&format!(" row={id} ")), "{line}");
        assert!(line.contains(&format!(" ruling={RULING} — ")) && line.ends_with(kind.route()), "{line}");
        assert_eq!(line.lines().count(), 1, "1 行: {line}");
    }
    assert_eq!(denied("git push --force-with-lease origin x", &manifest), None, "語が違う flag は通す");
}

/// host_guard.tmux の `enabled = false` で、その行にだけ在る語列を host-guard は通す（他の種類は動く・同じ manifest で
/// 行が発効なら断る＝切ったことだけが効く）。git の語列は runner.denied_commands と重複するので enabled の性質は tmux の
/// 行で測る。
#[test]
fn host_guard_kind_disabled_tmux_row_passes_host_guard() {
    let off = manifest(false, None);
    assert_eq!(denied("tmux kill-server", &off), None, "切った行の語列は host-guard が通す");
    assert!(denied("git push --force", &off).is_some(), "他の種類は動く");
    let on = denied("tmux kill-server", &manifest(true, None)).map(|(what, _)| what);
    assert_eq!(on.as_deref(), Some("host-guard-deny tmux"), "発効の行なら断る");
}

/// 同じ `enabled = false` の fixture で、command guard の `denied_of` は host_guard.tmux の語列を行 id つきで返し、`judge`
/// は断って行 id host_guard.tmux を名指す（enabled を見ない・disabled の行を捨てる変異で赤）。
#[test]
fn host_guard_kind_disabled_tmux_row_is_still_read_by_command_guard() {
    let manifest = manifest(false, None);
    let sources = command::denied_of(&manifest).unwrap_or_else(|| panic!("行が揃う"));
    assert!(sources.contains(&(TMUX_ROW, vec!["tmux kill-server".to_owned()])), "{sources:?}");
    let CommandDecision::Deny { what, line } = command::judge("tmux kill-server", &manifest) else {
        panic!("command guard は enabled を見ずに断る");
    };
    assert_eq!(what, "tmux kill-server");
    assert!(line.contains(&format!("rules 行 {TMUX_ROW} が禁じる")), "{line}");
}

/// 語列の行が無い manifest は、その行の種類の語で断る（FailClosed・当たらない command でも）。空の manifest は宣言順で
/// 先の git の種類で断る。
#[test]
fn host_guard_kind_missing_row_fails_closed_with_the_kind_word() {
    for (id, kind) in WORD_ROWS.iter().zip(["git", "tmux", "ledger"]) {
        let (what, line) = denied("ls", &manifest(true, Some(*id))).unwrap_or_else(|| panic!("{id} が無い周は断る"));
        assert_eq!(what, format!("host-guard-deny {kind}"), "{id}");
        assert!(line.contains(&format!(" hit=no-row row={id} ruling=- — ")), "{line}");
    }
    let empty = Manifest::parse("schema = 1\n").unwrap_or_else(|errors| panic!("{errors:?}"));
    assert_eq!(denied("ls", &empty).map(|(what, _)| what).as_deref(), Some("host-guard-deny git"));
    assert_eq!(judge("Edit", "", &empty, &nowhere()), HostGuardDecision::Allow, "編集系には語列の種類が掛からない");
}

/// host_guard.tmux にだけ在る語列を command guard が断り、deny 文が行 id host_guard.tmux を名指す（runner の id を固定で
/// 書かない）。runner と重複する語列は行の並びで先の runner.denied_commands を名指す。
#[test]
fn host_guard_kind_command_guard_names_the_tmux_row() {
    let manifest = manifest(true, None);
    let CommandDecision::Deny { what, line } = command::judge("tmux kill-server", &manifest) else {
        panic!("tmux の行の語列を command guard が断る");
    };
    assert_eq!(what, "tmux kill-server", "記録の what は当たった語列");
    assert!(line.starts_with(&format!("{NAME}: deny tmux kill-server は rules 行 {TMUX_ROW} が禁じる")), "{line}");
    assert!(!line.contains(command::ROW), "runner の id を名乗らない: {line}");
    let CommandDecision::Deny { line, .. } = command::judge("bd delete x", &manifest) else {
        panic!("台帳の行の語列も断る");
    };
    assert!(line.contains(&format!("rules 行 {LEDGER_ROW} が禁じる")), "{line}");
    let CommandDecision::Deny { line, .. } = command::judge("git push --force", &manifest) else {
        panic!("runner の語列は断る");
    };
    assert!(line.contains(&format!("rules 行 {} が禁じる", command::ROW)), "{line}");
}

/// runner.denied_commands は在るが host_guard の 1 行を欠く manifest では、command guard は当たらない command も
/// `no-row <欠けた行の id>` で断る（`denied_of` は `None`・欠けた行を読み飛ばす変異で赤）。
#[test]
fn host_guard_kind_command_guard_fails_closed_without_a_host_row() {
    for id in WORD_ROWS {
        let manifest = manifest(true, Some(id));
        assert_eq!(command::denied_of(&manifest), None, "{id} を欠く周は揃わない");
        let CommandDecision::Deny { what, line } = command::judge("ls", &manifest) else {
            panic!("{id} が無い周は当たらない command も断る");
        };
        assert_eq!(what, format!("reason=no-row {id}"));
        assert!(line.contains(&format!("rules 行 {id} を読めない")), "{line}");
    }
    assert_eq!(command::judge("ls", &manifest(true, None)), CommandDecision::Allow, "揃えば当たらない command は通す");
}

/// git と tmux の語列を両方含む 1 command は宣言順で先の git だけを断り、deny は 1 行。
#[test]
fn host_guard_kind_first_kind_in_declaration_order_wins() {
    let manifest = manifest(true, None);
    for command in ["tmux kill-server; git push --force origin main", "git push --force && tmux kill-server"] {
        let (what, line) = denied(command, &manifest).unwrap_or_else(|| panic!("{command} は断る"));
        assert_eq!(what, "host-guard-deny git", "{command}");
        assert!(!line.contains("kind=tmux") && line.lines().count() == 1, "{line}");
    }
}

/// 置き場の実体が無い state dir で判定する（埋め込みか `--rules` の rules）。
fn decided(payload: &str, rules: Option<&str>) -> Result<HostGuardDecision, Unreadable> {
    decide(payload, rules, Path::new("/nonexistent/state"))
}

/// 台帳を持たない cwd の台帳の形と、見張り自身の設定の arm は Allow: bdw を経ない台帳の write・settings.json への Write が
/// 通る（実体の無い path の rm も通る＝rm の判定は行 c の `host_guard_rm_`・台帳の形は行 f の `host_guard_ledger_` の歯）。
#[test]
fn host_guard_kind_rm_ledger_arm_and_settings_arms_allow_in_row_b() {
    for command in ["rm -rf /tmp/state-dir", "bd close s2-1", "bd update s2-1 --notes x"] {
        assert_eq!(decided(&bash(command), None), Ok(HostGuardDecision::Allow), "{command}");
    }
    let write = "{\"cwd\":\"/tmp\",\"tool_name\":\"Write\",\"tool_input\":{\"file_path\":\"/tmp/acct/settings.json\"}}";
    assert_eq!(decided(write, None), Ok(HostGuardDecision::Allow), "settings.json への Write");
    assert!(matches!(decided(&bash("git push --force"), None), Ok(HostGuardDecision::Deny { .. })), "埋め込みの行で git は断る");
}

/// payload が JSON でない・tool_name が無い・Bash の command が無い・rules が読めない周は理由つきで断り、判定に載らない
/// tool は rules を読まずに通す。
#[test]
fn host_guard_kind_unreadable_payload_fails_closed() {
    assert_eq!(decided("not json", None), Err(Unreadable::PayloadUnreadable));
    assert_eq!(decided("{\"cwd\":\"/tmp\"}", None), Err(Unreadable::NoToolName));
    assert_eq!(decided("{\"tool_name\":\"Bash\",\"tool_input\":{}}", None), Err(Unreadable::NoCommand));
    assert_eq!(decided(&bash("ls"), Some("/nonexistent/rules.toml")), Err(Unreadable::RulesUnreadable));
    assert_eq!(decided("{\"tool_name\":\"Read\"}", Some("/nonexistent/rules.toml")), Ok(HostGuardDecision::Allow));
}

// ─── rm の種類（行 c・接頭辞 `host_guard_rm_`） ───

/// 守る集合の 3 記号を持つ rm の行（値は引数・`enabled` も引数）。
fn rm_row(symbols: &[&str], enabled: bool) -> String {
    row(RM_ROW, "HostGuardRmProtected", symbols, enabled)
}

/// 語列の 3 行と rm の行（3 記号・発効）を持つ manifest。
fn rm_manifest() -> Manifest {
    manifest_with(true, None, &rm_row(&["state-dir", "repo-tracked", "repo-git"], true))
}

/// 歯の置き場: `base/state`（host.toml と accounts）・`base/work/repo`（`src/lib.rs` だけ tracked・`notes.txt` と
/// `.gitkeep` と `build/out.o` は untracked）・`base/work/other`（repo の外・file `x`）。
struct Place {
    base: PathBuf,
    state: PathBuf,
    repo: PathBuf,
    other: PathBuf,
}

impl Place {
    /// 歯ごとに作り直す（名は歯ごとに一意・pid つき）。
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!("host-guard-rm-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let _ = fs::create_dir_all(&base);
        let base = fs::canonicalize(&base).unwrap_or(base);
        let (state, repo, other) = (base.join("state"), base.join("work/repo"), base.join("work/other"));
        for dir in [state.join("accounts"), repo.join("src"), repo.join("build"), other.clone()] {
            let _ = fs::create_dir_all(dir);
        }
        for file in ["state/host.toml", "work/repo/src/lib.rs", "work/repo/notes.txt", "work/repo/.gitkeep"] {
            let _ = fs::write(base.join(file), "x\n");
        }
        let _ = fs::write(repo.join("build/out.o"), "x\n");
        let _ = fs::write(other.join("x"), "x\n");
        git(&repo, &["init", "-q"]);
        git(&repo, &["add", "src/lib.rs"]);
        Self { base, state, repo, other }
    }

    /// 本物の git で判定し、断る周の hit（` hit=` と ` row=` の間）を返す。通す周は `None`。
    fn hit(&self, command: &str, cwd: &Path) -> Option<String> {
        self.hit_in(command, cwd, &rm_manifest(), Path::new("git"))
    }

    /// manifest と git の program を指定して判定する。
    fn hit_in(&self, command: &str, cwd: &Path, manifest: &Manifest, git: &Path) -> Option<String> {
        let scene = Scene { cwd, state_dir: &self.state, git };
        match judge("Bash", command, manifest, &scene) {
            HostGuardDecision::Deny { line, .. } => Some(line.split(" hit=").nth(1)?.split(" row=").next()?.to_owned()),
            HostGuardDecision::Allow => None,
        }
    }

    /// 置き場を片付ける。
    fn clean(self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// git を 1 回撃つ（失敗は読み手の assert が落とす）。
fn git(dir: &Path, args: &[&str]) {
    let _ = Command::new("git").arg("-C").arg(dir).args(args).output();
}

/// 記号は閉じた 3 値の宣言順で、字面から引け、rules 行の値の綴り違いは読み込みで拒み 3 記号は受理される。
#[test]
fn host_guard_rm_symbols_are_three_in_declaration_order() {
    assert!(is_declaration_order(PROTECTED, |symbol| symbol as usize), "PROTECTED は宣言順: {PROTECTED:?}");
    let words: Vec<&str> = PROTECTED.iter().map(|symbol| symbol.as_str()).collect();
    assert_eq!(words, ["state-dir", "repo-tracked", "repo-git"], "3 記号の字面");
    assert!(PROTECTED.iter().all(|symbol| Protected::parse(symbol.as_str()) == Some(*symbol)), "字面から引ける");
    assert_eq!(Protected::parse("state_dir"), None, "綴り違いは引けない");
    let _accepted = rm_manifest();
    let text = format!("schema = 1\n{}", rm_row(&["state-dir", "repo_git"], true));
    let errors = Manifest::parse(&text).err().unwrap_or_default();
    assert!(errors.iter().any(|error| error.message.contains("未知の守る集合の記号 repo_git")), "{errors:?}");
}

/// launcher（sudo・env・timeout・command・exec・nice・`\` の逃がし）を剥いだ先頭語の basename が rm の segment だけを
/// 判定し、rmdir・git rm・rm を引数に持つだけの command は通す。
#[test]
fn host_guard_rm_verb_is_identified_after_launchers() {
    let place = Place::new("verb");
    let target = place.state.join("host.toml").display().to_string();
    for head in [
        "/bin/rm", "\\rm", "sudo rm", "sudo -u root rm", "env X=1 rm", "env -u Y rm", "timeout 5 rm", "timeout -s KILL 5 rm",
        "command rm", "exec rm", "nice -n 5 rm", "sudo -- env rm", "ls && rm", "sudo -E rm", "env - rm", "nice -5 rm",
    ] {
        let hit = place.hit(&format!("{head} {target}"), &place.other);
        assert_eq!(hit, Some(format!("state-dir:{target}")), "{head}");
    }
    for head in ["rmdir", "git rm", "echo rm", "timeout rm"] {
        assert_eq!(place.hit(&format!("{head} {target}"), &place.other), None, "{head} は rm でない");
    }
    place.clean();
}

/// `-` で始まる語は flag として読み飛ばし、`--` の後ろは全部 path（`-f` という名の file が守る dir に在っても flag は
/// path に読まない）。
#[test]
fn host_guard_rm_flags_are_skipped_and_double_dash_ends_them() {
    let place = Place::new("flags");
    let _ = fs::write(place.state.join("-f"), "x\n");
    let _ = fs::write(place.state.join("-"), "x\n");
    assert_eq!(place.hit("rm -f nope", &place.state), None, "flag は path でない");
    let want = format!("state-dir:{}", place.state.join("-f").display());
    assert_eq!(place.hit("rm -- -f", &place.state), Some(want.clone()), "-- の後ろは path");
    assert_eq!(place.hit("rm -r -- -f --", &place.state), Some(want), "2 つ目の -- も path");
    let dash = format!("state-dir:{}", place.state.join("-").display());
    assert_eq!(place.hit("rm -", &place.state), Some(dash), "- 1 字は path");
    place.clean();
}

/// 実体の無い path は守る dir の配下の字面でも通す。
#[test]
fn host_guard_rm_missing_path_passes() {
    let place = Place::new("missing");
    for path in [place.state.join("nope"), place.repo.join("src/nope.rs"), place.repo.join(".git/nope")] {
        assert_eq!(place.hit(&format!("rm -rf {}", path.display()), &place.other), None, "{}", path.display());
    }
    assert!(place.hit(&format!("rm {}", place.state.join("host.toml").display()), &place.other).is_some(), "在れば断る");
    place.clean();
}

/// 相対 path は payload の cwd を基準に `.` と `..` を畳む（cwd が repo の root なら `.` も断られる）。
#[test]
fn host_guard_rm_relative_path_folds_against_cwd() {
    let place = Place::new("fold");
    let (src, repo) = (place.repo.join("src"), place.repo.display().to_string());
    assert_eq!(place.hit("rm ../src/lib.rs", &src), Some(format!("repo-tracked:{repo}/src/lib.rs")), "..");
    assert_eq!(place.hit("rm ../notes.txt", &src), None, "untracked は通る");
    assert_eq!(place.hit("rm -rf .", &place.repo), Some(format!("repo-tracked:{repo}")), "root の .");
    assert_eq!(place.hit("rm -rf build/..", &place.repo), Some(format!("repo-tracked:{repo}")), "畳むと root");
    assert_eq!(place.hit("rm -rf ./build", &place.repo), None, "untracked の dir");
    place.clean();
}

/// `$` の変数展開は解かずに断り（引用符を解いた語が hit）、同じ字面でも rm でない先頭語は通す。
#[test]
fn host_guard_rm_unresolved_variable_is_denied() {
    let place = Place::new("variable");
    assert_eq!(place.hit("rm \"$X\"", &place.other), Some("unresolved:$X".to_owned()));
    assert_eq!(place.hit("rm -rf ${HOME}", &place.other), Some("unresolved:${HOME}".to_owned()));
    assert_eq!(place.hit("echo \"$X\"", &place.other), None, "rm でない");
    place.clean();
}

/// `` ` `` と `$(` の command 置換は解かずに断り、同じ字面でも rm でない先頭語は通す。
#[test]
fn host_guard_rm_unresolved_command_substitution_is_denied() {
    let place = Place::new("substitution");
    assert_eq!(place.hit("rm `cat list`", &place.other), Some("unresolved:`cat".to_owned()));
    assert_eq!(place.hit("rm $(cat list)", &place.other), Some("unresolved:$(cat".to_owned()));
    assert_eq!(place.hit("echo `cat list`", &place.other), None, "rm でない");
    place.clean();
}

/// `~` 始まりの path は HOME を読まずに断り、同じ字面でも rm でない先頭語は通す（home の短縮の字面は paths-clean が
/// 数えるので `concat!` で組む）。
#[test]
fn host_guard_rm_unresolved_tilde_is_denied() {
    const HOME_X: &str = concat!("~", "/x");
    let place = Place::new("tilde");
    assert_eq!(place.hit(&format!("rm -rf {HOME_X}"), &place.other), Some(format!("unresolved:{HOME_X}")));
    assert_eq!(place.hit("rm ~other/x", &place.other), Some("unresolved:~other/x".to_owned()));
    assert_eq!(place.hit(&format!("ls {HOME_X}"), &place.other), None, "rm でない");
    assert_eq!(place.hit("rm x~", &place.other), None, "~ で始まらない語は解く（実体が無い）");
    place.clean();
}

/// brace の `{` `}` を含む語は展開せずに断り、同じ字面でも rm でない先頭語は通す。
#[test]
fn host_guard_rm_unresolved_brace_is_denied() {
    let place = Place::new("brace");
    assert_eq!(place.hit("rm {a,b}.txt", &place.other), Some("unresolved:{a,b}.txt".to_owned()));
    assert_eq!(place.hit("rm a}", &place.other), Some("unresolved:a}".to_owned()));
    assert_eq!(place.hit("echo {a,b}.txt", &place.other), None, "rm でない");
    place.clean();
}

/// 同じ command の cd / pushd より後ろの segment の相対 path は解かずに断り、絶対 path は解いて判定する（cd より前の
/// rm と、cwd が絶対 path でない周の相対 path も同じ向き）。
#[test]
fn host_guard_rm_unresolved_relative_after_cd_is_denied() {
    let place = Place::new("cd");
    assert_eq!(place.hit("cd sub && rm x", &place.other), Some("unresolved:x".to_owned()), "cd");
    assert_eq!(place.hit("pushd sub; rm -f y", &place.other), Some("unresolved:y".to_owned()), "pushd");
    assert_eq!(place.hit("cd sub && ls x", &place.other), None, "rm でない");
    assert_eq!(place.hit("rm nope && cd sub", &place.other), None, "cd より前の rm は解く");
    let tracked = place.repo.join("src/lib.rs").display().to_string();
    let outside = place.other.join("x").display().to_string();
    assert_eq!(place.hit(&format!("cd sub && rm {outside}"), &place.other), None, "cd の後ろの絶対 path は解く");
    assert_eq!(place.hit(&format!("cd / && rm {tracked}"), &place.repo), Some(format!("repo-tracked:{tracked}")));
    assert_eq!(place.hit("rm x", Path::new("")), Some("unresolved:x".to_owned()), "cwd が解けない周");
    place.clean();
}

/// glob の語は glob の字より前の literal な接頭の dir を解き、守る path と当たる関係に在れば断る（一時 dir の下と
/// untracked の dir の下は通り、repo の root の配下と state dir の下は断る）。
#[test]
fn host_guard_rm_glob_is_judged_by_its_literal_prefix_dir() {
    let place = Place::new("glob");
    let repo = place.repo.display().to_string();
    assert_eq!(place.hit("rm *.bak", &place.other), None, "一時 dir の下");
    assert_eq!(place.hit("rm -f build/*.o", &place.repo), None, "untracked の dir の下");
    assert_eq!(place.hit("rm *.bak", &place.repo), Some(format!("repo-tracked:{repo}/*.bak")), "root の配下");
    assert_eq!(place.hit("rm src/l?b.rs", &place.repo), Some(format!("repo-tracked:{repo}/src/l?b.rs")), "?");
    assert_eq!(place.hit("rm bu[i]ld/x", &place.repo), Some(format!("repo-tracked:{repo}/bu[i]ld/x")), "[");
    let state = format!("{}/*", place.state.display());
    assert_eq!(place.hit(&format!("rm -rf {state}"), &place.other), Some(format!("state-dir:{state}")), "state dir");
    place.clean();
}

/// `rm <path>` を cwd で判定した hit（path は絶対 path）。
fn hit_at(place: &Place, path: &Path, cwd: &Path) -> Option<String> {
    place.hit(&format!("rm -rf {}", path.display()), cwd)
}

/// 一致: state dir そのものは state-dir に当たり、兄弟の dir は通る。
#[test]
fn host_guard_rm_state_dir_itself_is_hit() {
    let place = Place::new("state-itself");
    let want = format!("state-dir:{}", place.state.display());
    assert_eq!(hit_at(&place, &place.state, &place.other), Some(want), "state dir そのもの");
    assert_eq!(hit_at(&place, &place.other, &place.other), None, "兄弟の dir");
    place.clean();
}

/// 配下: state dir の下の host.toml と accounts は state-dir に当たり、state dir の外の file は通る。
#[test]
fn host_guard_rm_state_dir_children_are_hit() {
    let place = Place::new("state-children");
    for child in ["host.toml", "accounts"] {
        let path = place.state.join(child);
        assert_eq!(hit_at(&place, &path, &place.other), Some(format!("state-dir:{}", path.display())), "{child}");
    }
    assert_eq!(hit_at(&place, &place.other.join("x"), &place.other), None, "外の file");
    place.clean();
}

/// 祖先: state dir の親 dir は state-dir に当たり、祖先でない兄弟 dir は通る。
#[test]
fn host_guard_rm_state_dir_parent_is_hit() {
    let place = Place::new("state-parent");
    let want = format!("state-dir:{}", place.base.display());
    assert_eq!(hit_at(&place, &place.base, &place.other), Some(want), "親 dir");
    assert_eq!(hit_at(&place, &place.other, &place.other), None, "祖先でない兄弟 dir");
    place.clean();
}

/// 一致: tracked file は repo-tracked に当たり、untracked の file は通る。
#[test]
fn host_guard_rm_tracked_file_is_hit() {
    let place = Place::new("tracked");
    let path = place.repo.join("src/lib.rs");
    assert_eq!(place.hit("rm src/lib.rs", &place.repo), Some(format!("repo-tracked:{}", path.display())), "tracked");
    assert_eq!(place.hit("rm notes.txt", &place.repo), None, "untracked");
    place.clean();
}

/// 祖先: tracked file を配下に持つ dir は repo-tracked に当たり、untracked だけの dir は通る。
#[test]
fn host_guard_rm_dir_holding_tracked_is_hit() {
    let place = Place::new("holding");
    let want = format!("repo-tracked:{}", place.repo.join("src").display());
    assert_eq!(place.hit("rm -rf src", &place.repo), Some(want), "tracked を持つ dir");
    assert_eq!(place.hit("rm -rf build", &place.repo), None, "untracked だけの dir");
    place.clean();
}

/// 配下: `.git` の下は repo-git に当たり、名が `.git` で始まるだけの兄弟 file は通る。
#[test]
fn host_guard_rm_git_dir_children_are_hit() {
    let place = Place::new("git-children");
    let want = format!("repo-git:{}", place.repo.join(".git/index").display());
    assert_eq!(place.hit("rm .git/index", &place.repo), Some(want), ".git の配下");
    assert_eq!(place.hit("rm .gitkeep", &place.repo), None, "兄弟の file");
    place.clean();
}

/// 祖先: repo の root の親 dir は当たり（tracked の祖先）、祖先でない兄弟 dir は通る。
#[test]
fn host_guard_rm_repo_root_parent_is_hit() {
    let place = Place::new("root-parent");
    let work = place.base.join("work");
    assert_eq!(hit_at(&place, &work, &place.repo), Some(format!("repo-tracked:{}", work.display())), "root の親");
    assert_eq!(hit_at(&place, &place.other, &place.repo), None, "祖先でない兄弟 dir");
    place.clean();
}

/// worktree: root は `.git` の file でも fs で解け、その file と `gitdir:` が指す本体の common dir が repo-git に当たる
/// （worktree の cwd から本体の親を消す rm は当たる）。本体と関係の無い dir は通る。
#[test]
fn host_guard_rm_worktree_git_file_and_common_dir_are_hit() {
    let place = Place::new("worktree");
    let config = ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"];
    git(&place.repo, &[&config[..], &["commit", "-q", "-m", "seed"]].concat());
    let tree = place.base.join("tree");
    git(&place.repo, &["worktree", "add", "-q", &tree.display().to_string()]);
    let _ = fs::create_dir_all(place.base.join("spare"));
    let dot = format!("repo-git:{}", tree.join(".git").display());
    assert_eq!(place.hit("rm .git", &tree), Some(dot), "worktree の .git の file");
    let lib = format!("repo-tracked:{}", tree.join("src/lib.rs").display());
    assert_eq!(place.hit("rm src/lib.rs", &tree), Some(lib), "root は .git の file から解ける");
    let work = place.base.join("work");
    assert_eq!(hit_at(&place, &work, &tree), Some(format!("repo-git:{}", work.display())), "本体の親");
    let objects = place.repo.join(".git/objects");
    let want = format!("repo-git:{}", objects.display());
    assert_eq!(hit_at(&place, &objects, &tree), Some(want), "common dir の配下（gitdir の外）");
    assert_eq!(hit_at(&place, &place.base.join("spare"), &tree), None, "関係の無い dir");
    place.clean();
}

/// 守る file を指す symlink の rm は link 自身の path で見て通り、link の親が守る dir なら当たり、state dir を指す
/// dir の link を経た path は realpath で当たる（末尾 `/` は link の先を指す）。
#[test]
fn host_guard_rm_symlink_is_judged_by_the_link_itself() {
    let place = Place::new("symlink");
    let _ = std::os::unix::fs::symlink(place.state.join("host.toml"), place.other.join("link"));
    let _ = std::os::unix::fs::symlink(place.other.join("x"), place.state.join("inner"));
    let _ = std::os::unix::fs::symlink(&place.state, place.other.join("st"));
    assert_eq!(place.hit("rm link", &place.other), None, "守る file を指す link");
    let inner = format!("state-dir:{}", place.state.join("inner").display());
    assert_eq!(hit_at(&place, &place.state.join("inner"), &place.other), Some(inner), "親が守る dir");
    let via = format!("state-dir:{}", place.other.join("st/accounts").display());
    assert_eq!(place.hit("rm -rf st/accounts", &place.other), Some(via), "realpath で当たる");
    assert_eq!(place.hit("rm st", &place.other), None, "dir の link そのもの");
    assert!(place.hit("rm -rf st/", &place.other).is_some(), "末尾 / は link の先");
    let link = place.other.join("st");
    let scene = Scene { cwd: &place.other, state_dir: &link, git: Path::new("git") };
    let command = format!("rm {}", place.state.join("host.toml").display());
    let found = judge("Bash", &command, &rm_manifest(), &scene);
    assert!(matches!(found, HostGuardDecision::Deny { .. }), "link で渡した state dir も実体で守る: {found:?}");
    place.clean();
}

/// git の子 process は rm の segment が在り root が解けた周の `git ls-files` 1 回だけ（偽の git で数える）。rm の
/// segment が無い周・root が解けない周・repo-tracked を値に持たない周は 0 回で、root が解けない周は repo の 2 記号が空。
#[test]
fn host_guard_rm_runs_git_once_only_with_rm_and_root() {
    use std::os::unix::fs::PermissionsExt;
    let place = Place::new("git-once");
    let (fake, count) = (place.base.join("fake-git"), place.base.join("count"));
    let _ = fs::write(&fake, format!("#!/bin/sh\necho \"$*\" >> '{}'\nexec git \"$@\"\n", count.display()));
    let _ = fs::set_permissions(&fake, fs::Permissions::from_mode(0o755));
    let calls = || fs::read_to_string(&count).unwrap_or_default().lines().map(str::to_owned).collect::<Vec<_>>();
    let hit = place.hit_in("rm notes.txt; rm -f build/out.o && rm src/lib.rs", &place.repo, &rm_manifest(), &fake);
    assert_eq!(hit.as_deref().map(|found| found.starts_with("repo-tracked:")), Some(true), "{hit:?}");
    assert_eq!(calls(), ["-C ".to_owned() + &place.repo.display().to_string() + " ls-files -z"], "1 回");
    for (command, cwd) in [("ls src", &place.repo), ("git status", &place.repo), ("rm x", &place.other)] {
        assert_eq!(place.hit_in(command, cwd, &rm_manifest(), &fake), None, "{command}");
    }
    let state_only = manifest_with(true, None, &rm_row(&["state-dir", "repo-git"], true));
    assert!(place.hit_in("rm .git/index", &place.repo, &state_only, &fake).is_some(), "repo-git は git を撃たない");
    assert_eq!(calls().len(), 1, "増えない: {:?}", calls());
    let broken = place.base.join("broken-git");
    let _ = fs::write(&broken, "#!/bin/sh\nexit 1\n");
    let _ = fs::set_permissions(&broken, fs::Permissions::from_mode(0o755));
    let hit = place.hit_in("rm notes.txt", &place.repo, &rm_manifest(), &broken);
    let want = format!("repo-tracked:{}", place.repo.join("notes.txt").display());
    assert_eq!(hit, Some(want), "ls-files を読めない周は root 全体を守る");
    place.clean();
}

/// 行の値に無い記号は守らない（state-dir を外した値で state dir の配下の rm が通り、他の記号は動く）。
#[test]
fn host_guard_rm_symbol_missing_from_value_is_not_guarded() {
    let place = Place::new("value");
    let without = manifest_with(true, None, &rm_row(&["repo-tracked", "repo-git"], true));
    let command = format!("rm {}", place.state.join("host.toml").display());
    assert_eq!(place.hit_in(&command, &place.other, &without, Path::new("git")), None, "値に無い state-dir");
    assert!(place.hit(&command, &place.other).is_some(), "値に在れば断る");
    assert!(place.hit_in("rm src/lib.rs", &place.repo, &without, Path::new("git")).is_some(), "他の記号は動く");
    place.clean();
}

/// 行が無い・列でない周は rm の segment だけを `no-row` で断り（rm でない command は通る）、`enabled = false` は rm の
/// 種類だけを切る（git の語列は動く）。
#[test]
fn host_guard_rm_missing_row_fails_closed_and_disabled_row_passes() {
    let place = Place::new("row");
    let target = format!("rm {}", place.state.join("host.toml").display());
    let not_list = "\n[[rule]]\nid = \"host_guard.rm\"\nkind = \"CoreLines\"\nvalue = 1\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n";
    for fixture in [manifest(true, None), manifest_with(true, None, not_list)] {
        let scene = Scene { cwd: &place.other, state_dir: &place.state, git: Path::new("git") };
        let HostGuardDecision::Deny { what, line } = judge("Bash", "rm nope", &fixture, &scene) else {
            panic!("行が読めない周は rm を断る");
        };
        assert_eq!(what, "host-guard-deny rm");
        assert!(line.contains(" kind=rm hit=no-row row=host_guard.rm ruling=- — "), "{line}");
        assert_eq!(judge("Bash", "ls", &fixture, &scene), HostGuardDecision::Allow, "rm でない command は通る");
    }
    let off = manifest_with(true, None, &rm_row(&["state-dir", "repo-tracked", "repo-git"], false));
    assert_eq!(place.hit_in(&target, &place.other, &off, Path::new("git")), None, "切った rm の種類");
    let git = place.hit_in("git push --force", &place.other, &off, Path::new("git"));
    assert_eq!(git.as_deref(), Some("git push --force"), "他の種類は動く");
    place.clean();
}

// ─── 台帳の形（行 f・接頭辞 `host_guard_ledger_`） ───

/// 語列の 3 行（host_guard.ledger の enabled は引数）と、`writes` が `Some(enabled)` なら 4 形を持つ ledger.denied_writes
/// の行を持つ manifest（`None` は行を持たない）。
fn ledger_manifest(ledger_on: bool, writes: Option<bool>) -> Manifest {
    let mut text = format!("schema = 1\n{}", row(command::ROW, "RunnerDeniedCommands", RUNNER, true));
    for (id, value, enabled) in [
        (super::GIT_ROW, "git push --force", true),
        (TMUX_ROW, "tmux kill-server", true),
        (LEDGER_ROW, "bd delete", ledger_on),
    ] {
        text.push_str(&row(id, "HostGuardDeniedCommands", &[value], enabled));
    }
    if let Some(enabled) = writes {
        let forms: Vec<&str> = ledger_guard::FORMS.iter().map(|form| form.as_str()).collect();
        text.push_str(&row(ledger_guard::ROW, "LedgerDeniedWrites", &forms, enabled));
    }
    Manifest::parse(&text).unwrap_or_else(|errors| panic!("fixture の manifest を読める: {errors:?}"))
}

/// 台帳の印（`.beads` の dir と `scripts/bdw` の file）のうち引数が true のものだけを持つ root（`.git` は dir で作る＝fs の辿り
/// だけで root が解ける）。名は歯ごとに一意・pid つき。
fn ledger_root(name: &str, beads: bool, bdw: bool) -> PathBuf {
    let root = std::env::temp_dir().join(format!("host-guard-ledger-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let _ = fs::create_dir_all(root.join(".git"));
    let _ = fs::create_dir_all(root.join("scripts"));
    if beads {
        let _ = fs::create_dir_all(root.join(".beads"));
    }
    if bdw {
        let _ = fs::write(root.join("scripts/bdw"), "#!/bin/sh\n");
    }
    root
}

/// root の下の dir を cwd にして判定し、断る周の (what, hit) を返す。通す周は `None`。
fn ledger_hit(command: &str, root: &Path, manifest: &Manifest) -> Option<(String, String)> {
    let scene = Scene { cwd: root, state_dir: Path::new("/nonexistent/state"), git: Path::new("git") };
    match judge("Bash", command, manifest, &scene) {
        HostGuardDecision::Deny { what, line } => Some((what, line.split(" hit=").nth(1)?.split(" row=").next()?.to_owned())),
        HostGuardDecision::Allow => None,
    }
}

/// 形が掛かるのは root が `.beads` の dir と `scripts/bdw` の file を両方持つ周だけ（片方だけ・どちらも無い root は
/// 0 語）。root は cwd から `.git` を辿って解く（root の下の dir からでも同じ）。
#[test]
fn host_guard_ledger_applies_only_under_a_root_with_both_marks() {
    let manifest = ledger_manifest(true, Some(true));
    let command = "bd update s2-1 --notes x";
    for (name, beads, bdw) in [("beads-only", true, false), ("bdw-only", false, true), ("neither", false, false)] {
        let root = ledger_root(name, beads, bdw);
        assert_eq!(ledger_hit(command, &root, &manifest), None, "{name}: 片方だけの root は読まない");
        let _ = fs::remove_dir_all(&root);
    }
    let root = ledger_root("both", true, true);
    let _ = fs::create_dir_all(root.join("sub"));
    let want = Some(("host-guard-deny ledger".to_owned(), "notes-replace".to_owned()));
    assert_eq!(ledger_hit(command, &root, &manifest), want.clone(), "両方を持つ root");
    assert_eq!(ledger_hit(command, &root.join("sub"), &manifest), want, "root の下の dir からも辿る");
    let _ = fs::remove_dir_all(&root);
}

/// 4 形が起票の門と同じ 1 語で当たり（行 id は ledger.denied_writes）、`--append-notes` と bdw の書き込みと読みは通る。
#[test]
fn host_guard_ledger_four_forms_hit_with_the_gate_words() {
    let manifest = ledger_manifest(true, Some(true));
    let root = ledger_root("forms", true, true);
    for (command, hit) in [
        ("bd update s2-1 --notes x", "notes-replace"),
        ("scripts/bdw update s2-1 --notes=x", "notes-replace"),
        ("bdw remember x", "memory-subcommand"),
        ("bdw create x --type task", "create-without-parent"),
        ("ls && bd close s2-1", "bd-outside-bdw"),
    ] {
        let found = ledger_hit(command, &root, &manifest);
        assert_eq!(found, Some(("host-guard-deny ledger".to_owned(), hit.to_owned())), "{command}");
        assert_eq!(ledger_guard::FORMS.iter().filter(|form| form.as_str() == hit).count(), 1, "起票の門の語: {hit}");
    }
    let scene = Scene { cwd: &root, state_dir: Path::new("/nonexistent/state"), git: Path::new("git") };
    let HostGuardDecision::Deny { line, .. } = judge("Bash", "bd close s2-1", &manifest, &scene) else {
        panic!("bd の書き込みは断る");
    };
    assert!(line.contains(&format!(" row={} ruling={RULING} — ", ledger_guard::ROW)), "{line}");
    assert!(line.ends_with(Kind::Ledger.route()), "{line}");
    for command in [
        "bdw update s2-1 --append-notes x",
        "bdw close s2-1 --reason x",
        "bdw create x --parent s2-1",
        "bd list --json",
        "bd show s2-1",
    ] {
        assert_eq!(ledger_hit(command, &root, &manifest), None, "{command}");
    }
    let _ = fs::remove_dir_all(&root);
}

/// ledger.denied_writes が無い・不発効の周は、host_guard.ledger が on なら書き込みの subcommand（`bd close x`・bdw も）を
/// 全部 `no-row` で断り、読みの subcommand（`bd list`）は通し、同じ command の git の語列の判定は動く。
#[test]
fn host_guard_ledger_without_the_writes_row_denies_every_write_subcommand() {
    let root = ledger_root("no-writes", true, true);
    for (manifest, why) in [(ledger_manifest(true, None), "行が無い"), (ledger_manifest(true, Some(false)), "不発効")] {
        for command in ["bd close x", "bdw close x", "bdw update x --append-notes y"] {
            let found = ledger_hit(command, &root, &manifest);
            assert_eq!(found, Some(("host-guard-deny ledger".to_owned(), "no-row".to_owned())), "{why}: {command}");
        }
        for command in ["bd list", "bdw show x"] {
            assert_eq!(ledger_hit(command, &root, &manifest), None, "{why}: {command}");
        }
        let git = ledger_hit("bd list && git push --force", &root, &manifest);
        assert_eq!(git, Some(("host-guard-deny git".to_owned(), "git push --force".to_owned())), "{why}: git は動く");
    }
    let _ = fs::remove_dir_all(&root);
}

/// 同じ fixture で host_guard.ledger を off にすると、台帳の語列（`bd delete`）も形も書き込みの全断りも通る（on なら
/// 断る＝切ったことだけが効く）。git の種類は動く。
#[test]
fn host_guard_ledger_disabled_row_cuts_sequences_and_forms() {
    let root = ledger_root("off", true, true);
    for (writes, command) in [(Some(true), "bd delete x"), (Some(true), "bd update x --notes y"), (None, "bd close x")] {
        assert!(ledger_hit(command, &root, &ledger_manifest(true, writes)).is_some(), "on は断る: {command}");
        assert_eq!(ledger_hit(command, &root, &ledger_manifest(false, writes)), None, "off は通す: {command}");
    }
    let git = ledger_hit("git push --force", &root, &ledger_manifest(false, None)).map(|(what, _)| what);
    assert_eq!(git.as_deref(), Some("host-guard-deny git"), "他の種類は動く");
    let _ = fs::remove_dir_all(&root);
}
