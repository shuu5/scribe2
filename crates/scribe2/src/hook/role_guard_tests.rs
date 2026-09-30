//! `hook::role_guard` の歯。**本体は `role_guard.rs`** で、ここには test だけが在る。
//!
//! 分けたのは憲法 C4（1 file の上限）である——`role_guard.rs` が 1213 行まで育ち、同じ file へ
//! 行を足す契約（行 z）を受けられなくなった（`s2-07l.737.22`）。`#[path]` で `role_guard` の子 module として
//! 取り込むので、module path は `hook::role_guard::tests` のまま＝歯の名前は 1 つも変わらない。

// 純粋な移動（`role_guard.rs` の test 区間から歯を足さずに写した・s2-07l.737.22）。
// flip-check: moved s2-07l.737.22

use super::{
    capabilities_of, is_self, judge, refused, subject, unanchored_line, Invalid, Operation, PathKind, PathKinds,
    RefuseReason, RoleDecision, Subject, CAPABILITY_COMMANDS, DECL_FILE, PATH_KINDS, SETTLE_HINT, STOP_HINT,
};
use crate::name::NAME;
use crate::pipe::declaration::path_kinds::{DeclaredPaths, INVALID_REASONS};
use crate::rules::manifest::Manifest;
use crate::seat::role::{Capability, Role, CAPABILITIES};
use proptest::prelude::*;
use proptest::test_runner::Config;
use std::path::{Path, PathBuf};

/// 固定の判定（宣言なし）で編集先を解く。
fn locate(root: Option<&Path>, cwd: &Path, target: &str) -> super::Located {
    super::locate(root, cwd, target, &PathKinds::Default)
}

/// 宣言の無い周の編集先の種別（`invalid` 無し）。
fn path(kind: PathKind, opened: bool) -> Subject {
    Subject::Path { kind, opened, invalid: None }
}

/// 反例の永続化を切り、case 数を 256 に pin する（`tests/e2e/prop.rs` と同じ形）。
fn config() -> Config {
    Config { cases: 256, failure_persistence: None, ..Config::default() }
}

/// `role.orchestrator` が `held` を持つ manifest。空の列は行を置けない（loader が空の配列を拒む）ので、
/// 行の無い manifest にする（＝権能なし＝`NoRow` の枝）。
fn manifest_with(held: &[Capability]) -> Manifest {
    let names: Vec<String> = held.iter().map(|cap| format!("\"{}\"", cap.as_str())).collect();
    let row = if names.is_empty() {
        String::new()
    } else {
        format!(
            "\n[[rule]]\nid = \"role.orchestrator\"\nkind = \"RoleCapabilities\"\nvalue = [{}]\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n",
            names.join(", ")
        )
    };
    Manifest::parse(&format!("schema = 1\n{row}"))
        .unwrap_or_else(|errors| panic!("fixture の manifest を読める: {errors:?}"))
}

/// 分類は **prefix だけ**（`..`・root ちょうど・似た名の兄弟 dir・段の深さ）。
#[test]
fn role_guard_path_kind_classifies_by_prefix_only() {
    let root = Path::new("/repo");
    for (target, want) in [
        ("design-intent/spec/srs.html", PathKind::DesignIntent),
        ("design-intent", PathKind::DesignIntent),
        ("design-intents/x.html", PathKind::Code),
        ("docs/design/seat-roles.md", PathKind::DesignDoc),
        ("docs/design", PathKind::DesignDoc),
        ("docs/designs/x.md", PathKind::Code),
        ("docs/x.md", PathKind::Code),
        ("src/design-intent/x.rs", PathKind::Code),
        ("README.md", PathKind::Code),
        ("", PathKind::Code),
        ("src/../docs/design/x.md", PathKind::DesignDoc),
        ("../outside.rs", PathKind::Outside),
        ("src/../../outside.rs", PathKind::Outside),
        ("/elsewhere/x.rs", PathKind::Outside),
    ] {
        let found = locate(Some(root), root, target);
        assert_eq!(found.kind, want, "{target}");
        assert_eq!(found.run, None, "{target} は便の worktree の外");
    }
    // 相対 path の基準は cwd（subdir から見た `design-intent/` は repo の `sub/design-intent/`＝Code）。
    assert_eq!(locate(Some(root), &root.join("sub"), "design-intent/x.html").kind, PathKind::Code);
    // root を解けない周は root の外として扱う（fail-closed）。
    assert_eq!(locate(None, root, "src/lib.rs").kind, PathKind::Outside);
}

/// 便の worktree の中は worktree 相対で分類し、run id と相対 path を返す。
#[test]
fn role_guard_locates_bead_worktree_paths_by_run_id() {
    let root = PathBuf::from("/repo");
    let inside = format!(".worktrees/{NAME}/run-1/docs/design/x.md");
    let found = locate(Some(&root), &root, &inside);
    assert_eq!(found.kind, PathKind::DesignDoc, "worktree 相対で分類する");
    assert_eq!(found.run, Some(("run-1".to_owned(), PathBuf::from("docs/design/x.md"))));
    let code = locate(Some(&root), &root, &format!(".worktrees/{NAME}/run-2/src/lib.rs"));
    assert_eq!(code.kind, PathKind::Code);
    assert_eq!(code.run.map(|(run, _)| run), Some("run-2".to_owned()));
    // worktree の集合 dir そのもの・別の器の worktree は便の中ではない。
    assert_eq!(locate(Some(&root), &root, &format!(".worktrees/{NAME}")).run, None);
    assert_eq!(locate(Some(&root), &root, ".worktrees/other/run-1/src/lib.rs").run, None);
}

/// 便の器でない worktree（`.worktrees/<name>/`・planner の docs PR 用の木）は repo の写しとして worktree 相対で
/// 分類し、便の印は開かない（`run: None`）。`.worktrees/<name>` そのものは `Code`・便の worktree は不変。
#[test]
fn hook_role_worktree_repo_copy_is_classified_worktree_relative() {
    let root = PathBuf::from("/repo");
    for (target, want) in [
        (".worktrees/planner-x/design-intent/spec/srs.html".to_owned(), PathKind::DesignIntent),
        (".worktrees/planner-x/docs/design/a.md".to_owned(), PathKind::DesignDoc),
        ("/repo/.worktrees/planner-x/design-intent/decisions/x.html".to_owned(), PathKind::DesignIntent),
        (".worktrees/planner-x/src/lib.rs".to_owned(), PathKind::Code),
        (".worktrees/planner-x".to_owned(), PathKind::Code),
        (".worktrees".to_owned(), PathKind::Code),
        (".worktrees/planner-x/../../../outside.rs".to_owned(), PathKind::Outside),
    ] {
        let found = locate(Some(&root), &root, &target);
        assert_eq!(found.kind, want, "{target}");
        assert_eq!(found.run, None, "{target} は便の worktree ではない");
    }
    // 便の worktree の分類と run id は不変。
    let bead = locate(Some(&root), &root, &format!(".worktrees/{NAME}/run-1/design-intent/x.html"));
    assert_eq!(bead.kind, PathKind::DesignIntent);
    assert_eq!(bead.run, Some(("run-1".to_owned(), PathBuf::from("design-intent/x.html"))));
    assert_eq!(locate(Some(&root), &root, &format!(".worktrees/{NAME}/run-1/src/lib.rs")).kind, PathKind::Code);
}

/// 照合は `<NAME> <sub> <sub2>` の並び: binary の名は末尾でもよく、1 行に複数在れば全部・重複は畳む・
/// 器の口でない command（`gh pr merge`・`pipe show`）と `${..._BIN}` の形は見ない。行末の名指しの停止は
/// `Stop`（ADR-0048・§25 約束 6 の「停止の 1 件だけ期待値が変わる」）。
#[test]
fn role_guard_capability_commands_match_the_three_word_sequence() {
    let answer = format!("{NAME} pipe answer --run r --words x");
    assert_eq!(capabilities_of(&answer), vec![Capability::Answer]);
    let by_path = format!("target/debug/{NAME} pipe land --run r");
    assert_eq!(capabilities_of(&by_path), vec![Capability::Merge]);
    let two = format!("{NAME} pipe answer --run r; {NAME} pipe run --run r && {NAME} pipe stop --run r");
    assert_eq!(capabilities_of(&two), vec![Capability::Answer, Capability::Launch, Capability::Stop], "宣言順・重複なし");
    let tight = format!("{NAME} pipe approve; ls");
    assert_eq!(capabilities_of(&tight), vec![Capability::Approve], "`;` の直付けでも 2 語目が読める");
    for silent in [
        "ls -la".to_owned(),
        format!("{NAME} pipe show --run r"),
        format!("{NAME} pipe"),
        format!("gh pr merge 1 && echo {NAME}"),
        format!("\"${{{}_BIN:-{NAME}}}\" pipe answer", NAME.to_uppercase()),
        format!("not{NAME} pipe answer"),
        format!("{NAME}x/pipe answer"),
    ] {
        assert!(capabilities_of(&silent).is_empty(), "権能付きでない: {silent}");
    }
    // 表は subcommand の名を 2 語で持ち、`pipe` の口だけである。
    for (name, _) in CAPABILITY_COMMANDS {
        assert_eq!(name.split(' ').count(), 2, "{name}");
        assert!(name.starts_with("pipe "), "{name}");
    }
}

/// 器の口の basename の形（§27 の歯の母集団 8 形）: `NAME` / `…/NAME` / `NAME.bin` / `…/NAME-pipe.bin` は器の口、
/// `NAMEctl` / `…/NAMEx` / 別名 / 空 は違う。同じ token で便を起こす行を組むと、器の口の 4 形だけが `Launch` を要る。
#[test]
fn role_guard_self_reads_the_basename_and_its_dot_or_dash_copies() {
    let forms: [(String, bool); 8] = [
        (NAME.to_owned(), true),
        (format!("target/debug/{NAME}"), true),
        (format!("{NAME}.bin"), true),
        (format!("/tmp/cache/{NAME}-pipe.bin"), true),
        (format!("{NAME}ctl"), false),
        (format!("target/debug/{NAME}x"), false),
        ("cargo".to_owned(), false),
        (String::new(), false),
    ];
    for (token, want) in &forms {
        assert_eq!(is_self(token), *want, "{token:?}");
        let line = format!("{token} pipe run --run r");
        let caps = if *want { vec![Capability::Launch] } else { Vec::new() };
        assert_eq!(capabilities_of(&line), caps, "{line}");
    }
    assert_eq!(forms.iter().filter(|(_, want)| *want).count(), 4, "器の口は 4 形");
}

/// 停止の呼び出しの窓（2 語の後ろ）の **9 つの形**（§25 の歯の母集団）: 通る形 1 つ（`--run` と値）と起動の権能へ
/// 降りる形 8 つ（`--all`／`--run` 無し／`--run` の値無し／`--run` と `--all`／`--run=<id>` の 1 語／列の道具の
/// flag つき／置き場の値の途中に pipe と別の `--run`／`$(` を含む形）。
const STOP_WINDOWS: [(&str, Capability); 9] = [
    ("--run r", Capability::Stop),
    ("--all", Capability::Launch),
    ("", Capability::Launch),
    ("--run", Capability::Launch),
    ("--run r --all", Capability::Launch),
    ("--run=r", Capability::Launch),
    ("--run r --runner x", Capability::Launch),
    ("--run r --state-dir /s|--run x", Capability::Launch),
    ("--run $(cat id)", Capability::Launch),
];

/// 約束 3 / 4（ADR-0048 §2）: guard の表は停止の口を `stop` に結び、便 1 本を名指す形（`--run` と値・置き場と
/// repo と rules の flag を足した形も・binary の名は path の末尾でも）だけが `Stop`、母集団の残り 8 形は `Launch`
/// へ降りる。`--run` が 2 回・値が `-` で始まる形も降りる側。
#[test]
fn role_guard_stop_named_run_is_stop_and_the_other_forms_fall_to_launch() {
    for (window, want) in STOP_WINDOWS {
        let line = format!("{NAME} pipe stop {window}");
        assert_eq!(capabilities_of(&line), vec![want], "{line}");
    }
    assert_eq!(STOP_WINDOWS.iter().filter(|(_, cap)| *cap == Capability::Stop).count(), 1, "通る形は 1 つ");
    assert!(CAPABILITY_COMMANDS.contains(&("pipe stop", Capability::Stop)), "表は停止の口を stop に結ぶ");
    assert!(CAPABILITY_COMMANDS.iter().all(|(name, cap)| (*name == "pipe stop") == (*cap == Capability::Stop)), "stop は停止の口だけ");
    let full = format!("target/debug/{NAME} pipe stop --state-dir /s --run r --repo . --rules /r.toml");
    assert_eq!(capabilities_of(&full), vec![Capability::Stop], "置き場・repo・rules の flag を足した形も通る");
    for window in ["--run r --run r2", "--run --state-dir /s", "--run r --state-dir", "--run 'r'", "--run r --repo \"$HOME\""] {
        let line = format!("{NAME} pipe stop {window}");
        assert_eq!(capabilities_of(&line), vec![Capability::Launch], "{line}");
    }
}

/// Bash 面の入口 [`subject`](super::subject) で command 行を解く（停止の窓が降りたかの欄も入口が計算する）。
fn bash(line: &str) -> Subject {
    let op = Operation { tool: "Bash", command: Some(line), path: None, root: None, cwd: Path::new("/") };
    subject(&op, None).unwrap_or_else(|| panic!("権能付きの行: {line}"))
}

/// 約束 3 / 7: 名指しの停止は行が `stop` を持てば Allow・持たなければ deny（deny 文は `stop` と行 id を名指し
/// `hint=` を持たない）。降りた形の停止は行が `stop` を持っていても `launch` を要り、deny 文は `launch` を名指して
/// 末尾に通る名指しの形の 1 句（`hint=`・§29）を持つ。窓の無い素の起動の deny 文は `hint=` を持たない。
#[test]
fn role_guard_stop_judge_requires_the_stop_capability_from_the_row() {
    let named = bash(&format!("{NAME} pipe stop --run r"));
    assert_eq!(named, Subject::Capabilities(vec![Capability::Stop], Vec::new()));
    let with_stop = manifest_with(&[Capability::Answer, Capability::Stop]);
    assert_eq!(judge(&named, Role::Orchestrator, &with_stop), RoleDecision::Allow, "行が stop を持てば通る");
    let without_stop = manifest_with(&[Capability::Answer, Capability::EditTests]);
    let RoleDecision::Deny(line) = judge(&named, Role::Orchestrator, &without_stop) else {
        panic!("stop を持たない行では名指しの停止も deny");
    };
    assert!(line.contains("（stop）") && line.contains("role.orchestrator"), "欠けた権能と行 id: {line}");
    assert!(!line.contains("hint="), "名指しの停止は窓が降りていない: {line}");
    let all = bash(&format!("{NAME} pipe stop --all"));
    assert_eq!(all, Subject::Capabilities(vec![Capability::Launch], vec![Capability::Stop]));
    let RoleDecision::Deny(line) = judge(&all, Role::Orchestrator, &with_stop) else {
        panic!("--all は stop を持つ行でも deny（launch を要る）");
    };
    assert!(
        line.contains("（launch）") && line.contains("hint=") && line.contains("stop の権能で通る"),
        "launch を名指し通る形の 1 句を添える: {line}"
    );
    assert_eq!(line.matches("hint=").count(), 1, "句は 1 回: {line}");
    let head = format!("{NAME}: この操作（launch）は席の権能でない（rules 行 role.orchestrator）＝orchestrator 席では止める");
    assert_eq!(line, format!("{head} {STOP_HINT}"), "今の行の末尾に半角空白 1 つと句");
    let run = bash(&format!("{NAME} pipe run --run r --repo ."));
    assert_eq!(run, Subject::Capabilities(vec![Capability::Launch], Vec::new()));
    let RoleDecision::Deny(line) = judge(&run, Role::Orchestrator, &with_stop) else {
        panic!("素の起動は deny");
    };
    assert_eq!(line, head, "窓の無い素の起動の行は変わらない");
}

/// 約束 5: 名指しの停止の窓は行の末尾までなので、後ろに別の呼び出しが続く行は `Launch` へ降りる（`&&`・`;` の直付け・
/// `stop;` の形）。停止の前に別の口（回答）が在る行は両方の権能を要り、deny 文は欠けた側だけを名指す。
#[test]
fn role_guard_stop_window_runs_to_the_end_of_the_line() {
    let trailing = format!("{NAME} pipe stop --run r && ls");
    assert_eq!(capabilities_of(&trailing), vec![Capability::Launch], "後ろに別の command");
    let chained = format!("{NAME} pipe stop --run r; {NAME} pipe answer --run r --words x");
    assert_eq!(capabilities_of(&chained), vec![Capability::Answer, Capability::Launch], "後ろに別の口");
    let split = format!("{NAME} pipe stop; --run r");
    assert_eq!(capabilities_of(&split), vec![Capability::Launch], "2 語目に区切りが直付け");
    let both = format!("{NAME} pipe answer --run r --words x && {NAME} pipe stop --run r");
    let subject = bash(&both);
    assert_eq!(subject, Subject::Capabilities(vec![Capability::Answer, Capability::Stop], Vec::new()), "前の口と名指しの停止");
    assert_eq!(judge(&subject, Role::Orchestrator, &manifest_with(&[Capability::Answer, Capability::Stop])), RoleDecision::Allow);
    let RoleDecision::Deny(line) = judge(&subject, Role::Orchestrator, &manifest_with(&[Capability::Stop])) else {
        panic!("answer を持たない行は deny");
    };
    assert!(line.contains("（answer）"), "欠けた answer だけを名指す: {line}");
    let RoleDecision::Deny(line) = judge(&subject, Role::Orchestrator, &manifest_with(&[Capability::Answer])) else {
        panic!("stop を持たない行は deny");
    };
    assert!(line.contains("（stop）"), "欠けた stop だけを名指す: {line}");
}

/// 決着の歯の anchor（repo root と cwd）と置き場（hook が解いた値）。
const SETTLE_ANCHOR: &str = "/repo";
const SETTLE_PLACE: &str = "/state";

/// 決着の入口: root と cwd は anchor・置き場は `place`（`None` は anchor を解けない周）。Bash 面は git を撃たない。
fn settle_subject(line: &str, place: Option<&str>) -> Subject {
    let anchor = Path::new(SETTLE_ANCHOR);
    let op = Operation { tool: "Bash", command: Some(line), path: None, root: Some(anchor), cwd: anchor };
    subject(&op, place.map(Path::new)).unwrap_or_else(|| panic!("権能付きの行: {line}"))
}

/// 名指しの決着の 6 形（land 2・retire 4）。
fn settle_named() -> Vec<String> {
    let (place, anchor) = (SETTLE_PLACE, SETTLE_ANCHOR);
    [
        "pipe land --run r --terminal-only".to_owned(),
        format!("pipe land --terminal-only --run r --state-dir {place} --repo {anchor}"),
        "pipe retire --run r".to_owned(),
        format!("pipe retire --state-dir {place} --run r --repo {anchor}"),
        "pipe retire --run r --fold-only".to_owned(),
        "pipe retire --fold-only --run r --repo .".to_owned(),
    ]
    .map(|window| format!("{NAME} {window}"))
    .to_vec()
}

/// 名指しでない land 10 形（どれも merge・降りた列は settle）。
fn settle_unnamed_land() -> Vec<String> {
    [
        "--run r",
        "--terminal-only",
        "--terminal-only --terminal-only --run r",
        "--run=r --terminal-only",
        "--run r --terminal-only --detection-only",
        "--run r --terminal-only --bd b",
        "--run r --run r2 --terminal-only",
        "--run r --terminal-only 2>&1 | tail -3",
        "--run r --terminal-only --rules f",
        "--run r --terminal-only --state-dir /elsewhere",
    ]
    .map(|window| format!("{NAME} pipe land {window}"))
    .to_vec()
}

/// 名指しでない retire 10 形（どれも launch・降りた列は settle）。
fn settle_unnamed_retire() -> Vec<String> {
    [
        "",
        "--fold-only",
        "--run=r",
        "--run r --bd b",
        "--run r --run r2",
        "--run r --fold-only --fold-only",
        "--run r --fold-only x",
        "--run r --fold-only=x",
        "--run r 2>&1 | tail -3",
        "--run r --repo /other",
    ]
    .map(|window| format!("{NAME} pipe retire {window}"))
    .to_vec()
}

/// 約束 3 / 4 / 5: 名指しの 6 形は settle で降りた列は空。名指しでない land 10 形は merge・retire 10 形は launch で、
/// どれも降りた列は settle。停止の窓の判定は別で、決着の窓に `--rules` は通らない。
#[test]
fn role_guard_settle_named_windows_are_settle_and_the_others_fall_to_merge_or_launch() {
    for line in settle_named() {
        let want = Subject::Capabilities(vec![Capability::Settle], Vec::new());
        assert_eq!(settle_subject(&line, Some(SETTLE_PLACE)), want, "{line}");
    }
    for (lines, cap) in [(settle_unnamed_land(), Capability::Merge), (settle_unnamed_retire(), Capability::Launch)] {
        assert_eq!(lines.len(), 10, "名指しでない形は 10");
        for line in lines {
            let want = Subject::Capabilities(vec![cap], vec![Capability::Settle]);
            assert_eq!(settle_subject(&line, Some(SETTLE_PLACE)), want, "{line}");
        }
    }
    let rules = format!("{NAME} pipe retire --run r --rules /r.toml");
    assert_eq!(capabilities_of(&rules), vec![Capability::Launch], "--rules を持つ形は名指しでない");
    let stop = settle_subject(&format!("{NAME} pipe stop --run r --rules /r.toml"), Some(SETTLE_PLACE));
    assert_eq!(stop, Subject::Capabilities(vec![Capability::Stop], Vec::new()), "停止の窓は不変");
}

/// 約束 3 / 4: 置き場と anchor は `Path` の成分で比べる（末尾の `/`・`.` は同じ・`..` と相対で外れる形は違う）。
/// 置き場を渡さない `subject` と公開の `capabilities_of` は `--state-dir` か `--repo` を持つ 3 形を名指しと読まない。
#[test]
fn role_guard_settle_place_and_repo_are_compared_by_components() {
    for window in ["--state-dir /state/", "--state-dir /state/.", "--repo /repo/", "--repo ./", "--state-dir /state --repo ."] {
        let line = format!("{NAME} pipe retire --run r {window}");
        let want = Subject::Capabilities(vec![Capability::Settle], Vec::new());
        assert_eq!(settle_subject(&line, Some(SETTLE_PLACE)), want, "{line}");
    }
    for window in ["--state-dir /state/../state", "--state-dir state", "--repo ..", "--repo /repo/../repo", "--state-dir /repo"] {
        let line = format!("{NAME} pipe retire --run r {window}");
        let want = Subject::Capabilities(vec![Capability::Launch], vec![Capability::Settle]);
        assert_eq!(settle_subject(&line, Some(SETTLE_PLACE)), want, "{line}");
    }
    let named = settle_named();
    assert_eq!(named.iter().filter(|line| line.contains("--state-dir") || line.contains("--repo")).count(), 3);
    for line in &named {
        let carries = line.contains("--state-dir") || line.contains("--repo");
        let (found, demoted) = if carries { (vec![want_cap(line)], vec![Capability::Settle]) } else { (vec![Capability::Settle], Vec::new()) };
        assert_eq!(settle_subject(line, None), Subject::Capabilities(found.clone(), demoted), "置き場を渡さない: {line}");
        assert_eq!(capabilities_of(line), found, "公開の読み: {line}");
    }
}

/// 名指しでない形の権能（land は merge・retire は launch）。
fn want_cap(line: &str) -> Capability {
    if line.contains(" land ") {
        Capability::Merge
    } else {
        Capability::Launch
    }
}

/// 約束 6 / 9: settle を持つ行は名指しの 6 形を通し、持たない行は括弧つきの `（settle）` で断って句を持たない。
/// 名指しでない形の断り文は今の行の末尾に半角空白 1 つと決着の句で、名指しでない停止と退役を並べた行は 2 つの句を
/// この順に持ち、素の起動は句を持たない。
#[test]
fn role_guard_settle_judge_holds_the_row_and_appends_the_hint() {
    let with_settle = manifest_with(&[Capability::Answer, Capability::Settle]);
    let without = manifest_with(&[Capability::Answer, Capability::Stop]);
    for line in settle_named() {
        let named = settle_subject(&line, Some(SETTLE_PLACE));
        assert_eq!(judge(&named, Role::Orchestrator, &with_settle), RoleDecision::Allow, "{line}");
        let RoleDecision::Deny(text) = judge(&named, Role::Orchestrator, &without) else {
            panic!("settle を持たない行は断る: {line}");
        };
        assert!(text.contains("（settle）") && text.contains("role.orchestrator"), "{text}");
        assert!(!text.contains("hint="), "{text}");
    }
    for (line, name) in settle_unnamed_land().into_iter().map(|line| (line, "（merge）"))
        .chain(settle_unnamed_retire().into_iter().map(|line| (line, "（launch）")))
    {
        let RoleDecision::Deny(text) = judge(&settle_subject(&line, Some(SETTLE_PLACE)), Role::Orchestrator, &with_settle) else {
            panic!("名指しでない形は settle を持つ行でも断る: {line}");
        };
        let head = format!("{NAME}: この操作（{}）は席の権能でない（rules 行 role.orchestrator）＝orchestrator 席では止める", name.trim_matches(['（', '）']));
        assert!(text.contains(name), "{line}: {text}");
        assert_eq!(text, format!("{head} {SETTLE_HINT}"), "{line}");
    }
    let both = format!("{NAME} pipe stop --all && {NAME} pipe retire --run r --repo /other");
    let RoleDecision::Deny(text) = judge(&settle_subject(&both, Some(SETTLE_PLACE)), Role::Orchestrator, &with_settle) else {
        panic!("名指しでない停止と退役は断る");
    };
    assert!(text.ends_with(&format!(" {STOP_HINT} {SETTLE_HINT}")), "2 つの句をこの順に: {text}");
    assert_eq!(text.matches("hint=").count(), 2, "{text}");
    let run = settle_subject(&format!("{NAME} pipe run --run r"), Some(SETTLE_PLACE));
    let RoleDecision::Deny(text) = judge(&run, Role::Orchestrator, &with_settle) else {
        panic!("素の起動は断る");
    };
    assert!(text.contains("（launch）") && !text.contains("hint="), "素の起動は句を持たない: {text}");
}

/// 判定は行の値だけを読む: 持てば Allow・欠けば Deny（deny 文は欠けた権能と行 id）・行の無い周は
/// 権能なし・印で開いた path は種別の権能が無くても Allow。
#[test]
fn role_guard_judge_reads_the_role_row() {
    let manifest = manifest_with(&[Capability::Answer, Capability::EditDesignDoc]);
    let answer = Subject::Capabilities(vec![Capability::Answer], Vec::new());
    assert_eq!(judge(&answer, Role::Orchestrator, &manifest), RoleDecision::Allow);
    let launch = Subject::Capabilities(vec![Capability::Answer, Capability::Launch], Vec::new());
    let RoleDecision::Deny(line) = judge(&launch, Role::Orchestrator, &manifest) else {
        panic!("欠けた権能は deny");
    };
    assert!(line.starts_with(&format!("{NAME}: ")), "器が名乗る: {line}");
    assert!(line.contains("launch") && !line.contains("answer"), "欠けた権能だけを名指す: {line}");
    assert!(line.contains("role.orchestrator"), "行 id を名指す: {line}");
    let RoleDecision::Deny(line) = judge(&answer, Role::Orchestrator, &manifest_with(&[])) else {
        panic!("行の無い manifest は権能なし");
    };
    assert!(line.contains("reason=no-row role.orchestrator"), "{line}");
    let doc = path(PathKind::DesignDoc, false);
    assert_eq!(judge(&doc, Role::Orchestrator, &manifest), RoleDecision::Allow);
    let code = path(PathKind::Code, false);
    let RoleDecision::Deny(line) = judge(&code, Role::Orchestrator, &manifest) else {
        panic!("種別の権能が無ければ deny");
    };
    assert!(line.contains("edit-code"), "{line}");
    assert!(!line.contains("paths="), "宣言の不正でない周は paths= を持たない: {line}");
    let opened = path(PathKind::Code, true);
    assert_eq!(judge(&opened, Role::Orchestrator, &manifest), RoleDecision::Allow, "印で開いた path は通る");
}

/// 不正な宣言の周の deny 文は末尾に `paths=invalid:<理由>`（doctor の欄と同じ字面・§24）を持ち、理由の 5 種の
/// それぞれが字面で出る。印で開いた path は不正の周も通る（便の印の扱いは §4 のまま）。
#[test]
fn hook_role_paths_deny_line_carries_the_invalid_reason() {
    let manifest = manifest_with(&[Capability::EditDesignDoc]);
    for reason in INVALID_REASONS {
        let subject = Subject::Path { kind: PathKind::Code, opened: false, invalid: Some(reason) };
        let RoleDecision::Deny(line) = judge(&subject, Role::Orchestrator, &manifest) else {
            panic!("{reason:?}: code の編集は deny");
        };
        let tail = format!(" paths=invalid:{}", reason.as_str());
        assert!(line.ends_with(&tail), "{reason:?}: 末尾に理由の字面: {line}");
        assert_eq!(line.matches("paths=").count(), 1, "{line}");
        assert!(line.contains("edit-code") && line.contains("role.orchestrator"), "前半は不変: {line}");
        let opened = Subject::Path { kind: PathKind::Code, opened: true, invalid: Some(reason) };
        assert_eq!(judge(&opened, Role::Orchestrator, &manifest), RoleDecision::Allow, "印で開いた path は通る");
    }
}

/// 宣言で分類する（§24）: 書かれた key はその種別の固定値を置き換え（`design-intent/` は code に落ちる）、
/// 書かれていない key の種別は固定の判定のまま、`/` で終わらない項目は完全一致の 1 file だけ、宣言 file 自身は
/// 宣言が名指しても `Code`、不正な宣言は repo 内の全 file が `Code` で `..` の外は `Outside` のまま。便の
/// worktree と便の器でない worktree の中も同じ宣言で分類する。
#[test]
fn hook_role_paths_classify_reads_the_declared_prefixes() {
    let root = PathBuf::from("/repo");
    let declared = PathKinds::Declared(DeclaredPaths {
        design_intent: Some(vec!["spec/".to_owned(), DECL_FILE.to_owned()]),
        design_doc: Some(vec!["DESIGN.md".to_owned()]),
        tests: None,
    });
    for (target, want) in [
        ("spec/srs.yaml", PathKind::DesignIntent),
        ("spec", PathKind::DesignIntent),
        ("specs/x.yaml", PathKind::Code),
        ("design-intent/spec/srs.html", PathKind::Code),
        ("DESIGN.md", PathKind::DesignDoc),
        ("DESIGN.md.bak", PathKind::Code),
        ("docs/design/x.md", PathKind::Code),
        ("crates/x/tests/y.rs", PathKind::Tests),
        (DECL_FILE, PathKind::Code),
        ("src/lib.rs", PathKind::Code),
    ] {
        assert_eq!(super::locate(Some(&root), &root, target, &declared).kind, want, "{target}");
        let bead = format!(".worktrees/{NAME}/run-1/{target}");
        assert_eq!(super::locate(Some(&root), &root, &bead, &declared).kind, want, "便の worktree: {bead}");
        let copy = format!(".worktrees/planner-x/{target}");
        assert_eq!(super::locate(Some(&root), &root, &copy, &declared).kind, want, "repo の写し: {copy}");
    }
    assert_eq!(super::locate(Some(&root), &root, "../outside.rs", &declared).kind, PathKind::Outside, "外は宣言に依らない");
    assert_eq!(super::locate(Some(&root), &root, "spec/x.yaml", &declared).run, None, "repo 本体は便の外");
}

/// 1 本だけ書いた宣言はその種別だけが宣言で決まり残りは固定のまま、不正な宣言（5 つの理由のどれでも）は repo 内の
/// 全 file が `Code` で `..` の外は `Outside` のまま、宣言の無い周は固定の判定と 1 file も違わない。
#[test]
fn hook_role_paths_classify_one_key_invalid_and_default() {
    let one = PathKinds::Declared(DeclaredPaths { design_intent: None, design_doc: None, tests: Some(vec!["t/".to_owned()]) });
    assert_eq!(PathKind::classify(Path::new("t/x_test.py"), &one), PathKind::Tests, "書いた種別は宣言で");
    assert_eq!(PathKind::classify(Path::new("crates/x/tests/y.rs"), &one), PathKind::Code, "固定値は置き換わる");
    assert_eq!(PathKind::classify(Path::new("design-intent/x.html"), &one), PathKind::DesignIntent, "残りは固定のまま");
    assert_eq!(PathKind::classify(Path::new("docs/design/x.md"), &one), PathKind::DesignDoc);
    for reason in INVALID_REASONS {
        let invalid = PathKinds::Invalid(reason);
        for target in ["design-intent/x.html", "docs/design/x.md", "crates/x/tests/y.rs", "spec/x", "src/lib.rs"] {
            assert_eq!(PathKind::classify(Path::new(target), &invalid), PathKind::Code, "{reason:?}: {target}");
        }
        assert_eq!(PathKind::classify(Path::new("../x"), &invalid), PathKind::Outside, "{reason:?}: 外は宣言に依らない");
    }
    assert_eq!(Invalid::Overlap.as_str(), "overlap");
    // 宣言の無い周は固定の判定と同じ。
    for target in ["design-intent/x.html", "docs/design/x.md", "crates/x/tests/y.rs", "src/lib.rs", DECL_FILE, "../x"] {
        let rel = Path::new(target);
        assert_eq!(PathKind::classify(rel, &PathKinds::Default), PathKind::of_relative(rel), "{target}");
    }
}

/// 歯の段（`crates/<crate>/tests/`）と src の段は**別の権能**である（ADR-0045 §2 (1)）: orchestrator の行は
/// `edit-tests` を持ち `edit-code` を持たないので、歯の編集は通り src の編集は止まり、`design-intent/` は通る。
///
/// 分類は path の段だけで決まる（字面の語彙で判定しない）＝`crates/x/tests/…` は `Tests`・`crates/x/src/…`
/// と `tests/…`（crate の外）は `Code` である。
#[test]
fn role_guard_orchestrator_may_edit_teeth_but_not_src() {
    let row = manifest_with(&[Capability::EditTests, Capability::EditDesignIntent, Capability::EditOutside]);
    let judged = |rel: &str| {
        let kind = PathKind::of_relative(Path::new(rel));
        (kind, judge(&path(kind, false), Role::Orchestrator, &row))
    };
    assert_eq!(judged("crates/scribe2/tests/e2e/hook.rs"), (PathKind::Tests, RoleDecision::Allow), "歯は通る");
    assert_eq!(
        judged("design-intent/decisions/ADR-0045.html"),
        (PathKind::DesignIntent, RoleDecision::Allow),
        "design-intent は通る"
    );
    for rel in ["crates/scribe2/src/hook/role_guard.rs", "tests/e2e/hook.rs", "crates/scribe2/tests.rs"] {
        let (kind, decision) = judged(rel);
        assert_eq!(kind, PathKind::Code, "{rel} は src の段");
        let RoleDecision::Deny(line) = decision else {
            panic!("{rel}: src の編集は止まる");
        };
        assert!(line.contains("edit-code"), "{rel}: 欠けた権能を名指す: {line}");
    }
}

/// 理由の字面 6 種（現行のまま・宣言順）。
const REASON_WORDS: [&str; 6] =
    ["target-unresolved", "registry-unreadable", "unregistered", "rules-unreadable", "no-row", "no-anchor"];

/// (b) `RefuseReason::ALL` は 6 variant を判別子順（宣言順 = 解く順）に持ち、字面は 6 種で重複しない。
#[test]
fn hook_role_guard_route_all_pins_discriminant_order() {
    assert_eq!(RefuseReason::ALL.len(), REASON_WORDS.len(), "断りの理由は 6 種");
    let words: Vec<&str> = RefuseReason::ALL.iter().map(|reason| reason.as_str()).collect();
    assert_eq!(words, REASON_WORDS, "字面は宣言順");
    for pair in RefuseReason::ALL.windows(2) {
        assert!(pair[0] < pair[1], "判別子順に並ぶ: {pair:?}");
    }
    assert_eq!(RefuseReason::ALL[0], RefuseReason::TargetUnresolved, "先頭は target の段");
    assert_eq!(RefuseReason::ALL[5], RefuseReason::NoAnchor, "末尾は anchor の段");
    assert_eq!(RefuseReason::NoRow(Role::Orchestrator).render(), "no-row role.orchestrator", "行 id を伴う");
    assert_eq!(RefuseReason::Unregistered.render(), "unregistered", "行 id を伴わない");
}

/// (c) 各 variant の route は非空で器の subcommand の形（先頭は subcommand の名・散文でない）・理由に応じた口
/// （未登録 = `seat register`・anchor = `vessel`・読めない周 = `doctor`・行なし = `rules`）を名指す。
#[test]
fn hook_role_guard_route_every_variant_is_non_empty() {
    for reason in RefuseReason::ALL {
        let route = reason.route();
        assert!(!route.trim().is_empty(), "{reason:?} の route は非空");
        assert!(!route.starts_with(NAME), "{reason:?}: 器の名は deny 文が前置する: {route}");
        assert_eq!(route.lines().count(), 1, "{reason:?}: route は 1 行: {route}");
        let head = route.split(' ').next().unwrap_or_default();
        assert!(["seat", "doctor", "rules", "vessel"].contains(&head), "{reason:?}: subcommand の形: {route}");
    }
    assert!(RefuseReason::Unregistered.route().starts_with("seat register "), "{}", RefuseReason::Unregistered.route());
    for flag in ["--state-dir", "--target", "--role"] {
        assert!(RefuseReason::Unregistered.route().contains(flag), "登録の口の引数 {flag}");
    }
    assert!(RefuseReason::NoAnchor.route().starts_with("vessel init "), "{}", RefuseReason::NoAnchor.route());
    assert!(RefuseReason::RegistryUnreadable.route().starts_with("doctor "), "{}", RefuseReason::RegistryUnreadable.route());
    assert!(RefuseReason::RulesUnreadable.route().starts_with("doctor "), "{}", RefuseReason::RulesUnreadable.route());
    assert!(
        RefuseReason::NoRow(Role::Orchestrator).route().starts_with("rules get "),
        "{}",
        RefuseReason::NoRow(Role::Orchestrator).route()
    );
    assert!(RefuseReason::TargetUnresolved.route().starts_with("seat launch "), "{}", RefuseReason::TargetUnresolved.route());
}

/// (d) deny 文は 1 形: 前半（器の名・種別・`reason=<字面>`・括弧の句）は不変で、末尾に `route=<NAME> <1 行>` を持つ。
/// `unanchored_line` は `NoAnchor` の同じ形・`judge` の行なしは `no-row <row>` の同じ形。
#[test]
fn hook_role_guard_route_deny_line_ends_with_route() {
    let subject = Subject::Capabilities(vec![Capability::Answer], Vec::new());
    for (reason, word) in RefuseReason::ALL.into_iter().zip(REASON_WORDS) {
        let line = refused(&subject, reason);
        assert_eq!(line.lines().count(), 1, "deny 文は 1 行: {line}");
        let head = format!("{NAME}: この操作（capability=answer）は権能なし reason={}（席の登録 row と rules 行から権能を解けない）", reason.render());
        assert!(line.starts_with(&head), "前半は不変: {line}");
        assert!(line.contains(&format!("reason={word}")), "理由の字面 {word}: {line}");
        let tail = format!(" route={NAME} {}", reason.route());
        assert!(line.ends_with(&tail), "末尾に代替ルート: {line}");
        assert_eq!(line.matches("route=").count(), 1, "route は 1 句: {line}");
        assert_eq!(line, format!("{head}{tail}"), "前半と route の間に他の句を持たない");
    }
    assert_eq!(unanchored_line(&subject), refused(&subject, RefuseReason::NoAnchor));
    assert!(unanchored_line(&subject).contains("reason=no-anchor（"), "{}", unanchored_line(&subject));
    let RoleDecision::Deny(line) = judge(&subject, Role::Orchestrator, &manifest_with(&[])) else {
        panic!("行の無い manifest は権能なし");
    };
    assert_eq!(line, refused(&subject, RefuseReason::NoRow(Role::Orchestrator)));
    assert!(line.contains("reason=no-row role.orchestrator（"), "{line}");
    assert!(
        line.ends_with(&format!(" route={NAME} {}", RefuseReason::NoRow(Role::Orchestrator).route())),
        "{line}"
    );
}

/// 記録の種別の字面。
#[test]
fn role_guard_subject_renders_capability_or_path_kind() {
    let caps = Subject::Capabilities(vec![Capability::Answer, Capability::Launch], vec![Capability::Stop]);
    assert_eq!(caps.render(), "capability=answer+launch");
    assert_eq!(path(PathKind::Code, false).render(), "path=code");
    assert_eq!(path(PathKind::Code, true).render(), "path=code opened");
    let invalid = Subject::Path { kind: PathKind::Code, opened: false, invalid: Some(Invalid::Unreadable) };
    assert_eq!(invalid.render(), "path=code", "記録の種別は宣言の不正で変わらない（理由は deny の行）");
    assert_eq!(PATH_KINDS.len(), 5, "path 種別は 5 つ（歯の段を含む）");
}

/// 権能の名か表に無い語。
fn word() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        NAME.to_owned(),
        format!("bin/{NAME}"),
        "pipe".to_owned(),
        "answer".to_owned(),
        "approve".to_owned(),
        "run".to_owned(),
        "land".to_owned(),
        "show".to_owned(),
        "ls".to_owned(),
        "&&".to_owned(),
        "--run".to_owned(),
    ])
}

/// 任意の command 行（表の語の並び）。
fn command() -> impl Strategy<Value = String> {
    prop::collection::vec(word(), 0..12).prop_map(|words| words.join(" "))
}

/// 権能付き subcommand を少なくとも 1 つ含む command 行（前後は任意の語）。
fn armed_command() -> impl Strategy<Value = String> {
    (command(), 0..CAPABILITY_COMMANDS.len(), command()).prop_map(|(head, at, tail)| {
        let (name, _) = CAPABILITY_COMMANDS.get(at).copied().unwrap_or(("pipe answer", Capability::Answer));
        format!("{head} {NAME} {name} {tail}")
    })
}

/// 権能の任意の部分集合。
fn capability_set() -> impl Strategy<Value = Vec<Capability>> {
    prop::collection::vec(any::<bool>(), CAPABILITIES.len()).prop_map(|mask| {
        CAPABILITIES.iter().copied().zip(mask).filter(|(_, keep)| *keep).map(|(cap, _)| cap).collect()
    })
}

proptest! {
    #![proptest_config(config())]

    /// `Capability` / `PathKind` の `as_str` ↔ `parse` は往復し、列に無い名は必ず `None`。
    #[test]
    fn prop_role_names_round_trip_and_reject_unknown(name in "[a-z-]{0,20}") {
        for cap in CAPABILITIES {
            prop_assert_eq!(Capability::parse(cap.as_str()), Some(*cap));
        }
        for kind in PATH_KINDS {
            prop_assert_eq!(PathKind::parse(kind.as_str()), Some(*kind));
        }
        let known_cap = CAPABILITIES.iter().any(|cap| cap.as_str() == name);
        prop_assert_eq!(Capability::parse(&name).is_some(), known_cap);
        let known_kind = PATH_KINDS.iter().any(|kind| kind.as_str() == name);
        prop_assert_eq!(PathKind::parse(&name).is_some(), known_kind);
    }

    /// 任意の command 行で「照合された権能 ⊆ 行の値」なら Allow、1 つでも欠ければ Deny。
    #[test]
    fn prop_role_bash_face_allows_iff_matched_capabilities_are_held(line in armed_command(), extra in capability_set()) {
        let matched = capabilities_of(&line);
        prop_assert!(!matched.is_empty(), "{line}");
        let subject = Subject::Capabilities(matched.clone(), Vec::new());
        let mut held = matched.clone();
        held.extend(extra.iter().copied());
        prop_assert_eq!(judge(&subject, Role::Orchestrator, &manifest_with(&held)), RoleDecision::Allow);
        let short: Vec<Capability> = held.iter().copied().filter(|cap| Some(cap) != matched.first()).collect();
        prop_assert!(matches!(judge(&subject, Role::Orchestrator, &manifest_with(&short)), RoleDecision::Deny(_)));
    }

    /// 照合は行の語の並びだけを見る＝行の中の `<NAME> <sub> <sub2>` の 3 語窓が表に無ければ空。
    #[test]
    fn prop_role_capabilities_are_a_subset_of_the_table(line in command()) {
        let found = capabilities_of(&line);
        for cap in &found {
            prop_assert!(CAPABILITY_COMMANDS.iter().any(|(_, listed)| listed == cap));
        }
        let mut unique = found.clone();
        unique.dedup();
        prop_assert_eq!(unique.len(), found.len());
    }
}
