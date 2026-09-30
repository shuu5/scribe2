//! `hook::ledger_guard` の歯。**本体は `ledger_guard.rs`** で、ここには test だけが在る（憲法 C4 の余地を作るための純移動・設計 ledger-form §17）。
// flip-check: moved s2-07l.738.37.9
use super::{create_of, decide, forms_of, judge, judge_write, segments, write_of, LedgerDecision, Refusal, FORMS, ROW};
use crate::rules::manifest::Manifest;

/// 本文の 4 節。
const FULL: &str = "## memo\n### 出所\n- x\n### 観測\n### 候補\n### 昇格条件\n";

/// 引用符・`\`・区切りを解いて語に分ける（引用符の中の `;` は区切りでない）。
#[test]
fn hook_memo_guard_segments_respect_quotes() {
    let found = segments("bd create \"[memo] a; b\" --labels='intake:memo' && echo x\\ y");
    assert_eq!(
        found,
        vec![
            vec!["bd".to_owned(), "create".to_owned(), "[memo] a; b".to_owned(), "--labels=intake:memo".to_owned()],
            vec!["echo".to_owned(), "x y".to_owned()],
        ]
    );
}

/// title・label・body-file・acceptance を両形で読み、値を取る flag の値を title に数えない。
#[test]
fn hook_memo_guard_reads_create_flags() {
    let words = |line: &str| segments(line).into_iter().next().unwrap_or_default();
    let found = create_of(&words("scripts/bdw create --type task --title=x -l a,intake:memo --body-file f --acceptance 'design = d#a'"));
    let Some(create) = found else {
        panic!("bdw create を読む");
    };
    assert_eq!(create.titles, ["x"]);
    assert_eq!(create.labels, ["a", "intake:memo"]);
    assert_eq!(create.body_file.as_deref(), Some("f"));
    assert!(create.is_memo() && create.is_contract());
    assert_eq!(create_of(&words("bd update x --title create")), None, "create 以外の subcommand");
    assert_eq!(create_of(&words("echo bd create")), None, "client でない先頭語");
}

/// 判定の順: 契約 × label → body-file の不在 → 開けない → 4 節の最初の欠け。
#[test]
fn hook_memo_guard_judge_orders_reasons() {
    let words = |line: &str| segments(line).into_iter().next().unwrap_or_default();
    let judged = |line: &str, body: Option<&str>| {
        let create = create_of(&words(line)).unwrap_or_default();
        judge(&create, |_| body.map(str::to_owned))
    };
    assert_eq!(judged("bd create x --labels intake:memo --acceptance 'design = d#a'", Some(FULL)), Some(Refusal::MemoOnContract));
    assert_eq!(judged("bd create '[memo] x'", Some(FULL)), Some(Refusal::NoBodyFile));
    assert_eq!(judged("bd create '[memo] x' --body-file f", None), Some(Refusal::BodyUnreadable));
    assert_eq!(judged("bd create '[memo] x' --body-file f", Some("### 出所\n### 候補\n")), Some(Refusal::MissingSection(1)));
    assert_eq!(judged("bd create '[memo] x' --body-file f", Some(FULL)), None);
    assert_eq!(judged("bd create x --type epic", None), None, "memo でも契約でもない");
    assert_eq!(judged("bd create x --acceptance 'design = d#a'", None), None, "label の無い契約");
}

/// segment 1 つを 4 形の全部に掛ける（`None` は bd / bdw でない）。
fn formed(line: &str) -> Option<Option<Refusal>> {
    let words = segments(line).into_iter().next().unwrap_or_default();
    write_of(&words).map(|write| judge_write(&write, &FORMS))
}

/// 4 形 × 当たる例: bd と bdw のどちらでも `--notes` の両形が notes-replace・記憶の 3 語が memory-subcommand・
/// `--parent` の無い create が create-without-parent・`bd` の書き込みが bd-outside-bdw。
#[test]
fn hook_ledger_write_judge_hits_each_form() {
    for line in ["bd update s2-1 --notes x", "bdw update s2-1 --notes=x", "scripts/bdw update s2-1 --notes x", "bd update s2-1 --notes=x"] {
        assert_eq!(formed(line), Some(Some(Refusal::NotesReplace)), "{line}");
    }
    for line in ["bdw remember x", "bdw recall x", "bdw memories", "bd remember x", "bd recall x", "bd memories"] {
        assert_eq!(formed(line), Some(Some(Refusal::MemorySubcommand)), "{line}");
    }
    for line in ["bdw create x --type task", "X=1 bdw create --title=x", "bd create x"] {
        assert_eq!(formed(line), Some(Some(Refusal::CreateWithoutParent)), "{line}");
    }
    for line in ["bd update s2-1 --status open", "bd close s2-1", "/usr/bin/bd dep add a b", "bd create x --parent=s2-1"] {
        assert_eq!(formed(line), Some(Some(Refusal::BdOutsideBdw)), "{line}");
    }
}

/// 4 形 × 当たらない例（空虚さの柵）: `--append-notes`・`bdw` の書き込み・`--parent` を持つ create・読みの subcommand・
/// bd / bdw でない先頭語。
#[test]
fn hook_ledger_write_judge_passes_the_near_misses() {
    for line in [
        "bdw update s2-1 --append-notes x",
        "bdw update s2-1 --append-notes=--notes",
        "bdw close s2-1 --reason x",
        "bdw create x --parent s2-1",
        "bdw create --parent=s2-1 --title=x",
        "bd list --json",
        "bd show s2-1",
        "bd ready",
        "bdw show s2-1",
    ] {
        assert_eq!(formed(line), Some(None), "{line}");
    }
    for line in ["echo bd update --notes x", "bdwx update --notes x", "mybd close x"] {
        assert_eq!(formed(line), None, "{line}");
    }
}

/// 値の列に載る形だけを断り、行が無い・不発効・値が列でない周は no-row（FailClosed）。
#[test]
fn hook_ledger_write_forms_read_the_row_and_fail_closed() {
    let manifest = |value: &str, enabled: bool| {
        Manifest::parse(&format!(
            "schema = 1\n\n[[rule]]\nid = \"{ROW}\"\nkind = \"LedgerDeniedWrites\"\nvalue = {value}\nenabled = {enabled}\nruling = \"r\"\nruled_at = \"d\"\n"
        ))
        .unwrap_or_else(|errors| panic!("fixture の manifest を読める: {errors:?}"))
    };
    let full = "[\"parent-edge\", \"bd-outside-bdw\", \"create-bypass\", \"notes-replace\", \"memory-subcommand\", \"create-without-parent\"]";
    assert_eq!(forms_of(&manifest(full, true)), Ok(FORMS.to_vec()), "判定の順は FORMS（値の並びに依らない）");
    let one = forms_of(&manifest("[\"notes-replace\", \"unknown\"]", true));
    assert_eq!(one, Ok(vec![Refusal::NotesReplace]), "列に載る形だけ");
    let write = write_of(&segments("bd create x").into_iter().next().unwrap_or_default()).unwrap_or_default();
    assert_eq!(judge_write(&write, &[Refusal::NotesReplace]), None, "列に無い形は断らない");
    assert_eq!(forms_of(&manifest(full, false)), Err(Refusal::NoRow), "不発効");
    let empty = Manifest::parse("schema = 1\n").unwrap_or_else(|errors| panic!("{errors:?}"));
    assert_eq!(forms_of(&empty), Err(Refusal::NoRow), "行が無い");
    let embedded = Manifest::embedded().unwrap_or_else(|errors| panic!("{errors:?}"));
    assert_eq!(forms_of(&embedded), Ok(FORMS.to_vec()), "埋め込みの行は 6 形を全部断る");
}

/// 6 形は既存の 4 形の後ろに create-bypass → parent-edge の順で並び、埋め込みの行はその 6 語を全部断る。
#[test]
fn hook_ledger_edge_forms_are_six_in_order() {
    let want = [
        Refusal::NotesReplace,
        Refusal::MemorySubcommand,
        Refusal::CreateWithoutParent,
        Refusal::BdOutsideBdw,
        Refusal::CreateBypass,
        Refusal::ParentEdge,
    ];
    assert_eq!(FORMS, want, "判定の順");
    let words: Vec<&str> = FORMS.iter().map(|form| form.as_str()).collect();
    let names = ["notes-replace", "memory-subcommand", "create-without-parent", "bd-outside-bdw", "create-bypass", "parent-edge"];
    assert_eq!(words, names, "記録と rules の語");
    let embedded = Manifest::embedded().unwrap_or_else(|errors| panic!("{errors:?}"));
    assert_eq!(forms_of(&embedded), Ok(FORMS.to_vec()), "埋め込みの行は 6 形を全部断る");
    assert_eq!(formed("bdw create x --deps parent-child:y"), Some(Some(Refusal::CreateWithoutParent)), "既存の形が先");
    assert_eq!(formed("bd dep add a b --type parent-child"), Some(Some(Refusal::BdOutsideBdw)), "既存の形が先");
}

/// Write は flag でない語（subcommand を含む・出てきた順）と flag の値（`=` の形と離れた形）を運ぶ。
#[test]
fn hook_ledger_edge_write_carries_words_and_values() {
    let words = segments("X=1 scripts/bdw dep add a b --type parent-child --json -t=blocks").into_iter().next().unwrap_or_default();
    let Some(write) = write_of(&words) else {
        panic!("bdw を読む");
    };
    assert_eq!(write.client, "bdw");
    assert_eq!(write.subcommand, "dep");
    assert_eq!(write.flags, ["--type", "--json", "-t"]);
    assert_eq!(write.words, ["dep", "add", "a", "b", "parent-child"]);
    let values = vec![("--type".to_owned(), "parent-child".to_owned()), ("-t".to_owned(), "blocks".to_owned())];
    assert_eq!(write.values, values, "値を持たない flag（--json の次は flag）は載らない");
}

/// create-bypass: bd と bdw のどちらでも q・create-form・batch と、次の語が add の todo が当たり、todo list と todo done
/// と todo だけは当たらない。
#[test]
fn hook_ledger_edge_create_bypass_hits_the_intake_mouths() {
    for client in ["bd", "bdw", "scripts/bdw"] {
        for rest in ["q x", "q --type task x", "create-form", "batch", "batch --file f", "todo add x", "todo add"] {
            let line = format!("{client} {rest}");
            assert_eq!(formed(&line), Some(Some(Refusal::CreateBypass)), "{line}");
        }
        for rest in ["todo list", "todo done x", "todo", "show q", "list --json"] {
            let line = format!("{client} {rest}");
            assert_eq!(formed(&line), Some(None), "{line}");
        }
    }
}

/// parent-edge: dep add と link の --type・-t・--type= の値 parent-child・dep add の --file・create の --deps の値の
/// parent-child: が当たり、dep add の blocks・link の既定・dep remove は当たらない。
#[test]
fn hook_ledger_edge_parent_edge_hits_the_side_edges() {
    for line in [
        "bdw dep add a b --type parent-child",
        "bdw dep add a b -t parent-child",
        "bdw dep add a b --type=parent-child",
        "bdw dep add --type parent-child a b",
        "bdw link a b --type parent-child",
        "bdw link a b -t=parent-child",
        "bd link a b --type=parent-child",
        "bdw dep add --file edges.jsonl",
        "bdw dep add --file=edges.jsonl",
        "bdw create x --parent e --deps parent-child:y",
        "bdw create x --parent=e --deps=blocks:a,parent-child:b",
    ] {
        assert_eq!(formed(line), Some(Some(Refusal::ParentEdge)), "{line}");
    }
    for line in [
        "bdw dep add a b",
        "bdw dep add a b --type blocks",
        "bdw dep add a b -t=related",
        "bdw link a b",
        "bd link a b",
        "bdw link a b --type blocks",
        "bdw dep remove a b",
        "bdw dep remove a b --type parent-child",
        "bdw dep list a --type parent-child",
        "bdw create x --parent e --deps blocks:y",
        "bdw update a --parent b",
        "bdw show a --type parent-child",
    ] {
        assert_eq!(formed(line), Some(None), "{line}");
    }
}

/// 断り文は既存の形で、create-bypass は create の --parent を、parent-edge は update の --parent を次の一手に持つ。
#[test]
fn hook_ledger_edge_deny_lines_name_the_next_move() {
    let cwd = std::path::Path::new("/nonexistent-ledger-guard-cwd");
    for (line, what, next) in [
        ("bdw q x", "create-bypass", "bdw create <題> --parent <epic>"),
        ("bdw dep add a b --type parent-child", "parent-edge", "bdw update <子> --parent <親>"),
    ] {
        let LedgerDecision::Deny { what: found, line: text } = decide(line, cwd, None) else {
            panic!("{line}: 断る");
        };
        assert_eq!(found, what, "{line}");
        assert_eq!(text.lines().count(), 1, "{line}: 1 行");
        assert!(text.contains(&format!("deny 台帳の write は起票の門が止める reason={what}（")), "{text}");
        let after = text.split_once(" — ").map_or("", |(_, after)| after);
        assert!(after.contains(next), "{line}: 次の一手: {text}");
    }
}

/// decide: memo の理由が先・rules が読めない / 行が無い周は bd / bdw だけを断り、bd / bdw の無い command は通す。
#[test]
fn hook_ledger_write_decide_orders_memo_first_and_fails_closed() {
    let cwd = std::path::Path::new("/nonexistent-ledger-guard-cwd");
    let what = |line: &str, rules: Option<&std::path::Path>| match decide(line, cwd, rules) {
        LedgerDecision::Deny { what, line } => {
            assert_eq!(line.lines().count(), 1, "1 行: {line}");
            assert!(line.contains(&format!("reason={what}（")), "理由を名指す: {line}");
            what
        }
        LedgerDecision::Allow => String::new(),
    };
    assert_eq!(what("bd create '[memo] x'", None), "no-body-file", "memo の理由は 4 形より先");
    assert_eq!(what("bdw update s2-1 --notes x", None), "notes-replace");
    assert_eq!(what("ls && bd close s2-1", None), "bd-outside-bdw", "後ろの segment も読む");
    assert_eq!(what("bdw create x --parent s2-1", None), "");
    let missing = std::path::Path::new("/nonexistent-ledger-guard-rules.toml");
    assert_eq!(what("bdw show s2-1", Some(missing)), "rules-unreadable", "rules が読めない周");
    assert_eq!(what("cargo build", Some(missing)), "", "bd / bdw の無い command は rules を読まない");
}

/// create_of は `-d` と `--description=`（最後の値）・`--metadata`・`--stdin`・`--no-inherit-labels` と `=false` を読む。
#[test]
fn hook_question_form_create_reads_body_metadata_and_flags() {
    let create = |line: &str| create_of(&segments(line).into_iter().next().unwrap_or_default()).unwrap_or_default();
    let found = create("bdw create x -d a --description='b c' --metadata '{\"effect\":1}' --stdin --no-inherit-labels -l intake:question");
    assert_eq!(found.description.as_deref(), Some("b c"), "最後の値");
    assert_eq!(found.metadata.as_deref(), Some("{\"effect\":1}"));
    assert!(found.stdin && found.no_inherit_labels && found.has_question_label());
    assert_eq!(found.titles, ["x"], "値を title に数えない");
    let found = create("bd create x --stdin=false --no-inherit-labels=false");
    assert!(!found.stdin && !found.no_inherit_labels, "=false は持たない");
    assert!(create("bd create x --no-inherit-labels=true").no_inherit_labels);
    assert!(!create("bd create x --no-inherit-labels=yes").no_inherit_labels, "true でない値は継ぐ側");
    assert!(!create("bd create x -d y").has_question_label());
}

/// 段の順: 併せ持ちが継ぐ指定と 4 行より先・継ぐ指定が本文より先・本文を読めない形（--stdin・-・$・backtick・開けない）。
#[test]
fn hook_question_form_stages_run_in_order() {
    let meta = "--metadata '{\"effect\":\"document\",\"asked\":\"seat\"}'";
    let full = "-d '概要 = a\n技術 = b\n理由 = c\n推奨 = d'";
    let what = |line: &str| match decide(line, std::path::Path::new("/nonexistent-question-cwd"), None) {
        LedgerDecision::Deny { what, line } => {
            assert_eq!(line.lines().count(), 1, "1 行: {line}");
            assert!(line.contains("・ledger-form.md §14）"), "{line}");
            what
        }
        LedgerDecision::Allow => String::new(),
    };
    let base = "bdw create x -l intake:question";
    assert_eq!(what(&format!("{base},intake:memo --parent s2-1 -d y")), "question-memo-label", "併せ持ちが先");
    assert_eq!(what(&format!("{base} --parent s2-1 -d y")), "question-inherits-labels", "継ぐ指定が 4 行より先");
    let titled = "bdw create '[memo] x' -l intake:question --parent s2-1 --no-inherit-labels -d y";
    assert_eq!(what(titled), "question-no-summary", "memo の判定を掛けない");
    for body in ["--stdin", "--body-file -", "--body-file", "-d '$X'", "-d '`x`'", "--body-file nope.md"] {
        assert_eq!(what(&format!("{base} --parent s2-1 --no-inherit-labels {meta} {body}")), "question-body-unreadable", "{body}");
    }
    assert_eq!(what(&format!("{base} --parent s2-1 --no-inherit-labels {full} --metadata @nope.json")), "question-metadata-unreadable");
    assert_eq!(what(&format!("{base} --parent s2-1 --no-inherit-labels {full}")), "question-no-effect");
    assert_eq!(what(&format!("{base} --parent s2-1 --no-inherit-labels {full} {meta}")), "");
    assert_eq!(what(&format!("{base} --parent '' {full} {meta}")), "", "空の親は継がない");
}

/// cwd で撃ち、断れば 1 行と §15 の出所を確かめて語を返す（通れば空）。
fn triggered(line: &str, cwd: &std::path::Path) -> String {
    match decide(line, cwd, None) {
        LedgerDecision::Deny { what, line } => {
            assert!(line.lines().count() == 1 && line.ends_with("・ledger-form.md §15）"), "{line}");
            what
        }
        LedgerDecision::Allow => String::new(),
    }
}

/// update の本文の出どころ: `--body-file`・`--body-file=`・`-d` を読み、`--stdin`・値の無い `--body-file`・`-`・開けない
/// file・`$` の `-d` は読めない。本文を書かない update と見出しの無い本文と読める引き金の本文は通る。
#[test]
fn hook_memo_trigger_update_reads_each_body_source() {
    let dir = crate::pipe::fixture::scratch("memo-trigger-update");
    for (name, body) in [("bare.md", "### 昇格条件\n- 散文\n"), ("ok.md", "### 昇格条件\n- 引き金: 再発 1\n"), ("plain.md", "本文\n")] {
        std::fs::write(dir.join(name), body).unwrap_or_else(|error| panic!("{name}: {error}"));
    }
    for body in ["--body-file bare.md", "--body-file=bare.md", "-d '### 昇格条件\n引き金: 再発 3（x）'", "-d '### 昇格条件'"] {
        assert_eq!(triggered(&format!("bdw update s2-1 {body}"), &dir), "update-no-trigger", "{body}");
    }
    for body in ["--stdin", "--body-file", "--body-file -", "--body-file gone.md", "-d '$X'", "--body-file ok.md --stdin"] {
        assert_eq!(triggered(&format!("bdw update s2-1 {body}"), &dir), "update-body-unreadable", "{body}");
    }
    for body in ["--body-file ok.md", "--body-file plain.md", "-d 本文", "--status open", "--stdin=false", "--append-notes '### 昇格条件'"] {
        assert_eq!(triggered(&format!("bdw update s2-1 {body}"), &dir), "", "{body}");
    }
    let _ =std::fs::remove_dir_all(&dir);
}

/// 接頭辞は cwd から上へ辿った最初の `.beads` の dir を持つ dir の設定から解く（外側の台帳より内側が先）。
#[test]
fn hook_memo_trigger_resolves_the_prefix_upward_from_cwd() {
    let dir = crate::pipe::fixture::scratch("memo-trigger-prefix");
    let (outer, inner) = (dir.join("outer"), dir.join("outer").join("repo"));
    for (root, config) in [(&outer, "issue-prefix: far\n"), (&inner, "issue-prefix: \"toy\"\n")] {
        std::fs::create_dir_all(root.join(".beads").join("x")).unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(root.join(".beads").join("config.yaml"), config).unwrap_or_else(|error| panic!("{error}"));
    }
    let deep = inner.join("a").join("b");
    std::fs::create_dir_all(&deep).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!((super::ledger_prefix(&deep), super::ledger_prefix(&outer)), (Some("toy".to_owned()), Some("far".to_owned())));
    assert_eq!(super::ledger_prefix(&dir.join("gone")), None, ".beads の無い木");
    let body = "### 出所\n### 観測\n### 候補\n### 昇格条件\n";
    for (dep, want) in [("toy-1.2", ""), ("far-1", "no-trigger")] {
        std::fs::write(deep.join("m.md"), format!("{body}- 引き金: 依存 {dep}\n")).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(triggered("bdw create '[memo] x' --parent s2-1 --body-file m.md", &deep), want, "{dep}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// 40 桁の 16 進。
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// git を 1 回撃つ（toy repo の組み立て用）。
fn git(dir: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git").arg("-C").arg(dir).args(args).status();
    assert!(status.is_ok_and(|found| found.success()), "git {args:?}");
}

/// toy repo（`declaration` を HEAD に commit・`beads` が真なら `.beads/config.yaml`＝接頭辞 toy を置く）。宣言 `None` は無い。
fn toy(name: &str, declaration: Option<&str>, beads: bool) -> std::path::PathBuf {
    let dir = crate::pipe::fixture::scratch(name);
    git(&dir, &["init", "-q"]);
    git(&dir, &["config", "user.name", "t"]);
    git(&dir, &["config", "user.email", "t@example.invalid"]);
    std::fs::write(dir.join("seed"), "seed\n").unwrap_or_else(|error| panic!("{error}"));
    if let Some(extra) = declaration {
        let text = format!("schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git diff --quiet\"]\n{extra}");
        std::fs::write(dir.join(crate::pipe::declaration::DECL_FILE), text).unwrap_or_else(|error| panic!("{error}"));
    }
    if beads {
        std::fs::create_dir_all(dir.join(".beads")).unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(dir.join(".beads").join("config.yaml"), "issue-prefix: \"toy\"\n").unwrap_or_else(|error| panic!("{error}"));
    }
    git(&dir, &["add", "-A", "."]);
    git(&dir, &["commit", "-q", "-m", "seed"]);
    dir
}

/// close の段で撃ち、断れば 1 行と §16 の出所と `deny bd close` の頭を確かめて語を返す（通れば空）。
fn closed(line: &str, cwd: &std::path::Path) -> (String, String) {
    match decide(line, cwd, None) {
        LedgerDecision::Deny { what, line: text } => {
            assert!(text.lines().count() == 1 && text.ends_with("・ledger-form.md §16）"), "{line}: {text}");
            assert!(text.contains(&format!("deny bd close は起票の門が止める reason={what}（")), "{line}: {text}");
            (what, text)
        }
        LedgerDecision::Allow => (String::new(), String::new()),
    }
}

/// 理由の集め方: `-r`・`-r=`・`--reason=`・`--reason`・`--reason-file`（前後の空白を除く）・`done`・`gate resolve` の理由が読まれ、
/// 値の無い `--reason`・空・`-r<字>` は理由なし、2 つの理由は出てきた順に最初の外れで断る。
#[test]
fn hook_close_reason_collects_every_reason_flag_and_reads_in_order() {
    let dir = toy("close-reason-collect", Some("close-check = true\n"), true);
    std::fs::write(dir.join("ok.txt"), "  完了\n").unwrap_or_else(|error| panic!("{error}"));
    std::fs::write(dir.join("bad.txt"), "done it\n").unwrap_or_else(|error| panic!("{error}"));
    std::fs::write(dir.join("landed.txt"), format!("landed {SHA} ci=success\n")).unwrap_or_else(|error| panic!("{error}"));
    let word = |line: &str| closed(line, &dir).0;
    for line in [
        "bdw close toy-1 -r 完了",
        "bdw close toy-1 -r=完了",
        "bdw close toy-1 --reason=完了",
        "bdw close toy-1 --reason 完了",
        "bdw done toy-1 --reason '取り下げ 要らない'",
        "bdw gate resolve toy-9 -r 完了",
        "scripts/bdw close toy-1 --reason '重複 toy-2'",
        "bdw close toy-1 --reason-file ok.txt",
        "bdw close toy-1 toy-2 --reason 完了 --reason '後継 toy-3'",
        "bdw update toy-1 --status open",
        "bdw show toy-1",
    ] {
        assert_eq!(word(line), "", "{line}");
    }
    for line in ["bdw close", "bdw close toy-1", "bdw close toy-1 --reason", "bdw close toy-1 --reason ''", "bdw close toy-1 -r=", "bdw close toy-1 --reason=", "bdw done toy-1", "bdw gate resolve toy-9", "bdw close toy-1 -r完了"] {
        assert_eq!(word(line), "close-no-reason", "{line}");
    }
    for line in ["bdw close toy-1 --reason 'done it'", "bdw done toy-1 -r 'done it'", "bdw gate resolve toy-9 -r 'done it'", "bdw close toy-1 --reason-file bad.txt", "bdw close toy-1 toy-2 --reason 完了 --reason 'x y'", "bdw close toy-1 --reason nope --reason 'landed x'", "bdw close toy-1 --reason '重複 other-1'", "bdw close toy-1 --reason '昇格済み toy-1,toy-2'", "bdw close toy-1 --reason 'Landed x'"] {
        assert_eq!(word(line), "close-outside-forms", "{line}");
    }
    for line in [format!("bdw close toy-1 --reason 'landed {SHA} ci=success'"), "bdw close toy-1 --reason 'landed x'".to_owned(), "bdw close toy-1 --reason-file landed.txt".to_owned(), "bdw close toy-1 --reason 完了 --reason 'landed'".to_owned()] {
        assert_eq!(word(&line), "close-landed", "{line}");
    }
    for line in ["bdw close toy-1 --reason-file -", "bdw close toy-1 --reason-file", "bdw close toy-1 --reason-file=", "bdw close toy-1 --reason-file gone.txt", "bdw close toy-1 --reason '$X'", "bdw close toy-1 --reason '`x`'", "bdw close toy-1 -r \"$(cat r.txt)\"", "bdw close toy-1 --reason=完了$"] {
        assert_eq!(word(line), "close-reason-unreadable", "{line}");
    }
    let (_, text) = closed("bdw close toy-1 --reason 'landed x'", &dir);
    for named in ["pipe land --run <run> --terminal-only", "pipe retire --run <run>", "settle"] {
        assert!(text.contains(named), "{named}: {text}");
    }
    let (_, text) = closed("bdw close toy-1 --reason 'done it'", &dir);
    assert!(text.contains("「done it」") && text.contains("重複 <bead id>") && text.contains("完了") && !text.contains("landed <"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 3 値ごとの掛け方（pure の判定）: 加わる周は断り、読めない周は同じ判定で当たった語を名指す close-declaration-unreadable、
/// 加わらない周は撃たない。形に合う close はどの値でも通る。
#[test]
fn hook_close_reason_stage_applies_by_the_three_values() {
    use crate::pipe::declaration::CloseCheck;
    let writes = |line: &str| -> Vec<super::Write> { segments(line).iter().filter_map(|words| write_of(words)).collect() };
    let stage = |check: CloseCheck, line: &str| super::close_stage(check, &writes(line), Some("toy"), &|_: &str| None::<String>);
    let what = |found: Option<LedgerDecision>| match found {
        Some(LedgerDecision::Deny { what, line }) => (what, line),
        _ => (String::new(), String::new()),
    };
    assert_eq!(what(stage(CloseCheck::Joins, "bdw close toy-1")).0, "close-no-reason");
    let (word, line) = what(stage(CloseCheck::Unreadable, "bdw close toy-1"));
    assert_eq!(word, "close-declaration-unreadable");
    assert!(line.contains("close-no-reason") && line.contains(".vessel.toml") && line.contains("true か false"), "{line}");
    let (_, line) = what(stage(CloseCheck::Unreadable, "bdw close toy-1 --reason 'landed x'"));
    assert!(line.contains("close-landed"), "{line}");
    assert_eq!(stage(CloseCheck::Exempt, "bdw close toy-1"), None, "加わらない周は撃たない");
    for check in [CloseCheck::Joins, CloseCheck::Unreadable] {
        assert_eq!(stage(check, "bdw close toy-1 --reason '取り下げ x'"), None, "形に合う close は通る");
        assert_eq!(stage(check, "bdw show toy-1"), None, "close の segment が無い");
    }
    assert_eq!(what(stage(CloseCheck::Joins, "bdw show x && bdw done toy-2")).0, "close-no-reason", "後ろの segment も読む");
}

/// 宣言の読みは HEAD だけ・close の segment が在る周だけ: false・key 無し・宣言 file 無し・`.beads` 無し・作業ツリーだけの true は
/// 通し、文字列 yes の宣言は close-declaration-unreadable、加わる repo は語で断る。close の無い command は読めない repo でも通る。
#[test]
fn hook_close_reason_reads_the_head_declaration_once_per_close_and_exempts_the_rest() {
    let bare = "bdw close toy-1";
    let joined = toy("close-reason-joins", Some("close-check = true\n"), true);
    assert_eq!(closed(bare, &joined).0, "close-no-reason");
    assert_eq!(closed(bare, &joined.join(".beads")).0, "close-no-reason", "根は cwd から上へ辿る");
    for (name, declaration, beads) in [
        ("close-reason-false", Some("close-check = false\n"), true),
        ("close-reason-absent", Some(""), true),
        ("close-reason-nodecl", None, true),
        ("close-reason-nobeads", Some("close-check = true\n"), false),
    ] {
        let dir = toy(name, declaration, beads);
        assert_eq!(closed(bare, &dir).0, "", "{name}");
        let _ = std::fs::remove_dir_all(&dir);
    }
    let worktree = toy("close-reason-worktree", Some(""), true);
    std::fs::write(worktree.join(crate::pipe::declaration::DECL_FILE), "schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git diff --quiet\"]\nclose-check = true\n").unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(closed(bare, &worktree).0, "", "作業ツリーの宣言は読まない");
    let broken = toy("close-reason-unreadable", Some("close-check = \"yes\"\n"), true);
    let (word, text) = closed(bare, &broken);
    assert_eq!((word.as_str(), text.contains("close-no-reason")), ("close-declaration-unreadable", true), "{text}");
    assert_eq!(closed("bdw close toy-1 --reason '取り下げ x'", &broken).0, "", "形に合う close は読めない repo でも通る");
    assert_eq!(closed("bdw show toy-1 && sh close.sh", &broken).0, "", "close の segment が無い周は読まない");
    for dir in [&joined, &worktree, &broken] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// 理由を持てない口: `--status` の 4 つの綴り（値 closed）は status-closed、`duplicate`・`supersede`・`epic close-eligible` は
/// implicit-reason で次の一手の理由の形を名指す。`--status` の別の値・`--dry-run`・`epic status`・加わらない repo は通る。
#[test]
fn hook_close_mouth_denies_the_status_and_implicit_reason_forms() {
    let joined = toy("close-mouth-joins", Some("close-check = true\n"), true);
    let plain = toy("close-mouth-plain", Some(""), true);
    let broken = toy("close-mouth-unreadable", Some("close-check = \"yes\"\n"), true);
    // 埋め込みの rules では bd の直の書きは 6 形の語が先なので、bd の経路は e2e が測る。
    for client in ["bdw", "scripts/bdw"] {
        for tail in ["update toy-1 --status closed", "update toy-1 -s closed", "update toy-1 --status=closed", "update toy-1 -s=closed", "update --status closed toy-1 -p 1"] {
            let line = format!("{client} {tail}");
            let (word, text) = closed(&line, &joined);
            assert_eq!(word, "status-closed", "{line}");
            assert!(text.contains("bdw close <id> --reason '<形>'"), "{line}: {text}");
            let (word, text) = closed(&line, &broken);
            assert_eq!(word, "close-declaration-unreadable", "{line}");
            assert!(text.contains("status-closed"), "{line}: {text}");
            assert_eq!(closed(&line, &plain).0, "", "{line}: 加わらない repo");
        }
    }
    for dir in [&joined, &plain, &broken] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// bd が理由を書く口と、通す近い形（`--status` の別の値・`--dry-run`・`epic status`・読みの subcommand）。
#[test]
fn hook_close_mouth_denies_the_implicit_reason_forms_and_passes_the_near_ones() {
    let joined = toy("close-mouth-implicit-joins", Some("close-check = true\n"), true);
    let plain = toy("close-mouth-implicit-plain", Some(""), true);
    let broken = toy("close-mouth-implicit-unreadable", Some("close-check = \"yes\"\n"), true);
    for client in ["bdw", "scripts/bdw"] {
        for (tail, next) in [
            ("duplicate toy-1 --of toy-2", "重複 <id>"),
            ("supersede toy-1 --with toy-2", "後継 <id>"),
            ("epic close-eligible", "完了"),
            ("epic close-eligible --dry-run=false", "完了"),
        ] {
            let line = format!("{client} {tail}");
            let (word, text) = closed(&line, &joined);
            assert_eq!(word, "implicit-reason", "{line}");
            assert!(text.contains(&format!("bdw close <id> --reason '{next}'")), "{line}: {text}");
            assert_eq!(closed(&line, &broken).0, "close-declaration-unreadable", "{line}");
            assert_eq!(closed(&line, &plain).0, "", "{line}: 加わらない repo");
        }
        for tail in [
            "update toy-1 --status pinned",
            "update toy-1 --status open",
            "update toy-1 -s in_progress",
            "update toy-1 --title closed",
            "epic close-eligible --dry-run",
            "epic status",
            "epic",
            "duplicates",
            "show toy-1 --status closed",
            "list --status closed",
        ] {
            let line = format!("{client} {tail}");
            assert_eq!(closed(&line, &joined).0, "", "{line}");
            assert_eq!(closed(&line, &broken).0, "", "{line}");
        }
    }
    for dir in [&joined, &plain, &broken] {
        let _ = std::fs::remove_dir_all(dir);
    }
}
