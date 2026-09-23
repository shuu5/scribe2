// flip-check: moved s2-07l.351
//! 契約表と閉包の歯: `contract_` / `pipe_contract_`（契約表の検査・閉包の拡張・write-set の導出・Declared 行の歯の置き場）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引き、受付の口の歯と共有する helper は
//! `use super::intake::{…}` で引く（歯の本文は `intake.rs` から移しただけ・`s2-07l.351`）。

use super::*;
use super::intake::{
    capped_rules, contracts_check, declared_teeth_row, findings_of, intake_tokens, intake_with_rules, repo_with_big_file,
    sized_contract, table_repo, SUBCOMMAND_FILES, TABLE_VESSEL,
};

/// 契約の印 `opens`（`s2-07l.201`・設計 seat-roles.md §3「契約が開く例外」・AC16）: `classes` と同じ optional list
/// の形で、値は `PathKind` の名の列。intake の写し `contract.toml` にそのまま乗り、`Contract::load` が印を typed
/// に返す。印の無い便は空（従来どおり）。
#[test]
fn pipe_contract_opens_is_an_optional_list_of_path_kinds_copied_by_intake() {
    use vessel::hook::role_guard::{PathKind, PATH_KINDS};
    use vessel::pipe::contract::Contract;
    let (repo, state) = repo_with_state();
    let marked = write_contract(&repo, &[], &[r#"opens = ["code", "design-doc"]"#]);
    let id = intake(&repo, &state, &marked);
    let copied = Contract::load(&state.join("pipe").join(&id).join("contract.toml")).unwrap_or_else(|errors| panic!("{errors:?}"));
    assert_eq!(copied.opens, vec!["code".to_owned(), "design-doc".to_owned()], "写しに印が乗る（書いた順）");
    assert_eq!(copied.opened_kinds(), vec![PathKind::Code, PathKind::DesignDoc], "印は PathKind へ引ける");
    assert!(copied.classes.is_empty(), "classes は別の field のまま");
    stop_run_ok(&state, &id);

    let bare = intake_bead(&repo, &state, &write_contract(&repo, &[], &[]), "s2-bare");
    let plain = Contract::load(&state.join("pipe").join(&bare).join("contract.toml"))
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    stop_run_ok(&state, &bare);
    assert!(plain.opens.is_empty() && plain.opened_kinds().is_empty(), "印の無い便は空");
    // 取る名は PathKind の全数で、variant 名の字面・空の配列・重複 key は受けない。
    let all: Vec<String> = PATH_KINDS.iter().map(|kind| format!("\"{}\"", kind.as_str())).collect();
    let every_id = intake_bead(&repo, &state, &write_contract(&repo, &[], &[&format!("opens = [{}]", all.join(", "))]), "s2-every");
    let every = Contract::load(&state.join("pipe").join(&every_id).join("contract.toml"))
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    stop_run_ok(&state, &every_id);
    assert_eq!(every.opened_kinds(), PATH_KINDS.to_vec(), "全種別を開ける");
    // 名簿に無い名・配列でない値は**行**の側で断られる（契約 file は器が作るので手書きの不備は入口に無い）。
    for (add, want) in [(r#"opens = ["Code"]"#, "Code"), (r#"opens = "code""#, "opens")] {
        let out = intake_raw(&repo, &state, &write_contract(&repo, &[], &[add]), "s2-bad");
        let err = stderr_of(&out);
        assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "{add} を通さない: {err}");
        assert!(err.contains(want), "{add}: {err}");
    }
    clean(&[&repo, &state]);
}

// ─────────────────── 契約表（設計 docs/design/contract-source.md §2 / §3 / §9・`s2-07l.208`・接頭辞 `contract_`） ───────────────────

/// (1) 区間 1 つ・3 行（適合 / 型の構築点を write-set が欠く / 節が無い）の doc で、findings 2 件を `file:line` 付きで
/// 名指し rc 1・判定行 `docs=1 rows=3 findings=2`。適合だけの doc は rc 0（AC21 の表側）。
#[test]
fn contract_check_names_the_incomplete_write_set_and_the_missing_section_with_file_line() {
    let touches = ("touches", "[\"crate::tint::Tint\"]");
    let rows = [
        table_row("a", &[]),
        table_row("b", &[("section", "\"2\""), ("write-set", "[\"src/tint.rs\"]"), touches]),
        table_row("c", &[("section", "\"9\"")]),
    ];
    let doc = table_doc(&table_region(&rows));
    let repo = table_repo(&doc, &[]);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "違反 ≥ 1 は rc 1: {text}{}", stderr_of(&out));
    assert_eq!(found.len(), 2, "2 件ちょうど: {text}");
    let head = |id: &str| format!("contracts: docs/design/toy.md:{} ", table_line(&doc, id));
    let incomplete = found.iter().find(|line| line.starts_with(&head("b"))).cloned().unwrap_or_default();
    assert!(incomplete.contains("write-set-incomplete") && incomplete.contains("src/show.rs"), "閉包の足りない file: {text}");
    assert!(!incomplete.contains("src/tint.rs"), "write-set に在る file は名指さない: {incomplete}");
    let section = found.iter().find(|line| line.starts_with(&head("c"))).cloned().unwrap_or_default();
    assert!(section.contains("contract-table:section-missing"), "節の無い行を名指す: {text}");
    assert_eq!(text.lines().last(), Some("contracts check: docs=1 rows=3 untracked=0 findings=2 place-out=0/3"), "判定行: {text}");
    // 適合だけの doc は rc 0（write-set が閉包を覆えば touches を持つ行も通る）。
    let covering = ("write-set", "[\"src/tint.rs\", \"src/show.rs\"]");
    let good = table_repo(&table_doc(&table_region(&[table_row("a", &[]), table_row("b", &[covering, touches])])), &[]);
    let passed = contracts_check(&good);
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "適合だけの doc は rc 0: {}", stdout_of(&passed));
    assert_eq!(stdout_of(&passed).lines().collect::<Vec<&str>>(), ["contracts check: docs=1 rows=2 untracked=0 findings=0 place-out=0/2"]);
    clean(&[&repo, &good]);
}

/// (2) 区間の無い doc は findings 0・rows=0（表なしは違反ではない）・区間 2 つは `region-duplicate`（rc 1）・読めない
/// doc は `unreadable` を行番号 0 で名指して rc 2（黙って飛ばさない・NFR4）。
#[test]
fn contract_check_treats_a_doc_without_region_as_zero_rows_and_fails_closed_on_unreadable_docs() {
    let plain = table_repo(&table_doc(""), &[]);
    let out = contracts_check(&plain);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "表なしは違反でない: {}", stdout_of(&out));
    assert_eq!(stdout_of(&out).trim_end(), "contracts check: docs=1 rows=0 untracked=0 findings=0 place-out=0/0");
    let region = table_region(&[table_row("a", &[])]);
    let twice = table_repo(&table_doc(&format!("{region}\n{region}")), &[]);
    let out = contracts_check(&twice);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "区間 2 つは違反: {}", stdout_of(&out));
    assert!(findings_of(&out).iter().any(|line| line.contains("contract-table:region-duplicate")), "{}", stdout_of(&out));
    fs::write(plain.join("docs/design/bad.md"), [0xff, 0xfe, b'\n']).expect("非 UTF-8 の doc を書ける");
    git(&plain, &["add", "-A"]);
    git(&plain, &["commit", "-q", "-m", "bad"]);
    let out = contracts_check(&plain);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない doc は rc 2: {text}");
    assert!(text.contains("contracts: docs/design/bad.md:0 contract-table:unreadable"), "読めない doc を名指す: {text}");
    assert_eq!(text.lines().last(), Some("contracts check: docs=2 rows=0 untracked=0 findings=1 place-out=0/0"), "母集団は tracked 設計 doc の全数: {text}");
    clean(&[&plain, &twice]);
}

/// 未追跡の設計 doc（設計 contract-source.md §43 (3)・行 at）: 未追跡の `.md` を 1 本置いた木の判定行は
/// `untracked=1` を持ち、判定行の前に path を名乗る知らせが 1 行出て rc 0・findings 0 のまま（母集団に入らない＝
/// doc 数は 1）。同じ file を追跡すると `untracked=0` で知らせが消え、doc 数が 1 増える（同じ木の 2 回の判定行の対）。
#[test]
fn contracts_untracked_doc_is_noticed_without_counting_and_joins_the_population_once_tracked() {
    let repo = table_repo(&table_doc(&table_region(&[table_row("a", &[])])), &[]);
    fs::write(repo.join("docs/design/draft.md"), "# 下書き\n").expect("未追跡の doc を書ける");
    let out = contracts_check(&repo);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "未追跡は rc に数えない: {text}{}", stderr_of(&out));
    assert_eq!(
        text.lines().collect::<Vec<&str>>(),
        [
            "contracts untracked-doc: docs/design/draft.md は未追跡の設計 doc（検査の母集団に入らない）",
            "contracts check: docs=1 rows=1 untracked=1 findings=0 place-out=0/1",
        ],
        "知らせ 1 行 + 判定行: {text}"
    );
    assert!(findings_of(&out).is_empty(), "知らせは findings の行でない: {text}");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "draft"]);
    let out = contracts_check(&repo);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{text}{}", stderr_of(&out));
    assert_eq!(text.lines().collect::<Vec<&str>>(), ["contracts check: docs=2 rows=1 untracked=0 findings=0 place-out=0/1"], "{text}");
    clean(&[&repo]);
}

/// (3) `req` が要件面に無い行・`depends` が解決しない行・輪を持つ 2 行・`verify` に `(` を持つ行・末尾 `/` 無しの
/// dir を指す行を、各 1 件ずつ行番号付きで名指す（全件・1 件目で止めない・輪は 2 行で 1 件）。
#[test]
fn contract_check_names_each_row_defect_once_with_its_line() {
    let rows = [
        table_row("a", &[("req", "[\"FR1\", \"FR9\"]")]),
        table_row("b", &[("depends", "[\"zz\"]")]),
        table_row("c", &[("depends", "[\"d\"]")]),
        table_row("d", &[("depends", "[\"c\"]")]),
        table_row("e", &[("verify", "[\"git log (x)\"]")]),
        table_row("f", &[("write-set", "[\"src\"]")]),
    ];
    let doc = table_doc(&table_region(&rows));
    let repo = table_repo(&doc, &[]);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{text}");
    for (id, label, needle) in [
        ("a", "contract-table:requirement-missing", "FR9"),
        ("b", "contract-table:depends-unresolved", "zz"),
        ("c", "contract-table:depends-cycle", "c → d → c"),
        ("e", "contract-table:verify-form", "'('"),
        ("f", "write-set-dir-without-slash", "src/"),
    ] {
        let head = format!("contracts: docs/design/toy.md:{} {label}: ", table_line(&doc, id));
        let hits: Vec<&String> = found.iter().filter(|line| line.starts_with(&head)).collect();
        assert_eq!(hits.len(), 1, "行 {id} の {label} を 1 件: {text}");
        assert!(hits.iter().all(|line| line.contains(needle)), "{needle} を名乗る: {text}");
    }
    assert_eq!(found.len(), 5, "他の行は名指さない: {text}");
    assert_eq!(text.lines().last(), Some("contracts check: docs=1 rows=6 untracked=0 findings=5 place-out=0/6"), "{text}");
    clean(&[&repo]);
}

/// (4) `contracts schema` の出力は tracked の `contracts/schema.toml` と byte で一致し（差分 0）、欄の列は core の
/// const slice（`FIELDS`）と同じ順。余りの引数は断る。
#[test]
fn contract_schema_matches_the_tracked_file_and_the_field_slice() {
    let out = bin_cmd().args(["contracts", "schema"]).output().expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(out.stderr.is_empty(), "stderr は 0 byte: {}", stderr_of(&out));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("contracts").join("schema.toml");
    let tracked = fs::read_to_string(&path).expect("tracked の生成物を読める");
    assert_eq!(stdout_of(&out), tracked, "render と tracked の差分 0（`{NAME} contracts schema` で描き直す）");
    let names: Vec<&str> =
        tracked.lines().filter_map(|line| line.strip_prefix("name = \"")?.strip_suffix('"')).collect();
    let fields: Vec<&str> = vessel::pipe::table::FIELDS.iter().map(|field| field.name).collect();
    assert_eq!(names, fields, "欄の列は const slice と同じ順");
    let extra = bin_cmd().args(["contracts", "schema", "x"]).output().expect("binary を起動できる");
    assert_eq!(extra.status.code(), Some(i32::from(RC_REFUSED)), "余りの引数は断る");
}

/// (6) 宣言 `requirements` が指す要件面（`.yaml`）で req の実在を測り、key 無しの repo は既定の `.html` を読む。
/// 宣言が指す要件面が無い周は rc 2（既定へ黙って倒さない）。
#[test]
fn contract_check_reads_requirements_from_the_declared_face() {
    let doc = table_doc(&table_region(&[table_row("a", &[("req", "[\"FR7\"]")])]));
    let with_key = |face: &str| format!("{TABLE_VESSEL}requirements = \"{face}\"\n");
    let yaml = "requirements:\n  - FR7\n  - id: FR8\n";
    let declared = table_repo(&doc, &[(".vessel.toml", &with_key("spec/reqs.yaml")), ("spec/reqs.yaml", yaml)]);
    let out = contracts_check(&declared);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "宣言の要件面に在る req は通る: {}", stdout_of(&out));
    // 既定の要件面（srs.html）に FR7 は無い＝同じ行が名指される（宣言の path で測っていたことの弁別）。
    let fallback = table_repo(&doc, &[]);
    let out = contracts_check(&fallback);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{}", stdout_of(&out));
    let named = findings_of(&out).iter().any(|line| line.contains("requirement-missing") && line.contains("FR7"));
    assert!(named, "既定の要件面で測る: {}", stdout_of(&out));
    let missing = table_repo(&doc, &[(".vessel.toml", &with_key("spec/none.yaml"))]);
    let out = contracts_check(&missing);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "宣言が指す要件面が無い周は rc 2: {}", stdout_of(&out));
    assert!(stdout_of(&out).contains("spec/none.yaml を読めない"), "{}", stdout_of(&out));
    clean(&[&declared, &fallback, &missing]);
}

/// (6') 要件面が `.md` の周は行頭 `#` の見出しの先頭 token を要件 id に読む（設計 contract-source.md §4・行 k・
/// `s2-07l.354`）: `## FR1 …` / `## FR2 …` を持つ面で `req = ["FR1"]` の行は通り、`["FR9"]` は「要件面に無い」で
/// 落ちる。base は `.md` を「形を読めない」で断る（rc 2・RED）。
#[test]
fn contract_check_reads_requirements_from_md_headings() {
    let with_key = format!("{TABLE_VESSEL}requirements = \"spec/reqs.md\"\n");
    let md = "# 要件\n\n## FR1 便の起動\n\n便を起こす。FR9 は本文の字面。\n\n## FR2 審査\n\n審査する。\n";
    let passing = table_doc(&table_region(&[table_row("a", &[("req", "[\"FR1\", \"FR2\"]")])]));
    let declared = table_repo(&passing, &[(".vessel.toml", &with_key), ("spec/reqs.md", md)]);
    let out = contracts_check(&declared);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "md の見出しの id は通る: {}", stdout_of(&out));
    assert_eq!(stdout_of(&out).lines().last(), Some("contracts check: docs=1 rows=1 untracked=0 findings=0 place-out=0/1"), "{}", stdout_of(&out));
    let failing = table_doc(&table_region(&[table_row("a", &[("req", "[\"FR9\"]")])]));
    let missing = table_repo(&failing, &[(".vessel.toml", &with_key), ("spec/reqs.md", md)]);
    let out = contracts_check(&missing);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "本文の字面は id でない: {}", stdout_of(&out));
    let named = findings_of(&out).iter().any(|line| line.contains("requirement-missing") && line.contains("FR9"));
    assert!(named, "要件面に無い id を名指す: {}", stdout_of(&out));
    clean(&[&declared, &missing]);
}

/// (e) 入口の flip の名乗り（設計 pipeline.md §54 形 5・行 aw・`s2-07l.554`）: `cargo` の 2 行だけで名乗りを持つ宣言の
/// repo は rc 0 で判定行が ` entrance=unmeasured` で終わり、名乗りを外した同じ repo は `NoEntranceRed` で rc 2・名乗りと
/// 入口の flip の行を同居させた repo も矛盾で rc 2。宣言が Rust でない（TABLE_VESSEL の）repo の判定行は既存の 4 欄の
/// まま（名乗りの無い周は 1 字も変わらない）。
#[test]
fn contract_check_entrance_unmeasured_is_named_on_the_judgement_line() {
    let doc = table_doc(&table_region(&[table_row("a", &[])]));
    let cargo = "schema = 1\nallowed-commands = [\"cargo\", \"git\"]\ncommon-verify = [\"cargo nextest run\", \"cargo clippy --all-targets\"]\n";
    let named = format!("{cargo}entrance-flip = \"unmeasured\"\n");
    let declared = table_repo(&doc, &[(".vessel.toml", &named)]);
    let out = contracts_check(&declared);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "名乗りの宣言は通る: {}{}", stdout_of(&out), stderr_of(&out));
    let plain = table_repo(&doc, &[]);
    let bare = contracts_check(&plain);
    assert_eq!(bare.status.code(), Some(i32::from(RC_OK)), "{}{}", stdout_of(&bare), stderr_of(&bare));
    let lines = [stdout_of(&out).lines().last().map(str::to_owned), stdout_of(&bare).lines().last().map(str::to_owned)];
    assert_eq!(
        lines,
        [
            Some("contracts check: docs=1 rows=1 untracked=0 findings=0 entrance=unmeasured place-out=0/1".to_owned()),
            Some("contracts check: docs=1 rows=1 untracked=0 findings=0 place-out=0/1".to_owned()),
        ],
        "名乗りの周だけ末尾に欄を持ち、名乗りの無い周は既存の 4 欄のまま"
    );
    let unnamed = table_repo(&doc, &[(".vessel.toml", cargo)]);
    let refused = contracts_check(&unnamed);
    assert_eq!(refused.status.code(), Some(i32::from(RC_BROKEN)), "名乗りを外すと断る: {}", stdout_of(&refused));
    assert!(stderr_of(&refused).contains("NoEntranceRed"), "{}", stderr_of(&refused));
    assert!(!stdout_of(&refused).contains("entrance="), "断った周は欄を出さない: {}", stdout_of(&refused));
    let both = "schema = 1\nallowed-commands = [\"cargo\"]\ncommon-verify = [\"cargo xtask flip-check --base {base}\", \"cargo nextest run\"]\nentrance-flip = \"unmeasured\"\n";
    let clash = table_repo(&doc, &[(".vessel.toml", both)]);
    let clashed = contracts_check(&clash);
    assert_eq!(clashed.status.code(), Some(i32::from(RC_BROKEN)), "名乗りと flip の行の同居は断る: {}", stdout_of(&clashed));
    assert!(stderr_of(&clashed).contains("UnmeasuredWithEntranceFlip"), "{}", stderr_of(&clashed));
    let wrong = table_repo(&doc, &[(".vessel.toml", &format!("{cargo}entrance-flip = \"measured\"\n"))]);
    let misread = contracts_check(&wrong);
    assert_eq!(misread.status.code(), Some(i32::from(RC_BROKEN)), "語の誤りは断る: {}", stdout_of(&misread));
    assert!(stderr_of(&misread).contains("entrance-flip"), "理由が key の名を持つ: {}", stderr_of(&misread));
    clean(&[&declared, &plain, &unnamed, &clash, &wrong]);
}

/// 使い方の誤りは rc 1（stderr に理由）・git repo でない `--repo` は判定できないので rc 2（判定行を出さない）。
#[test]
fn contract_check_refuses_usage_errors_and_non_repositories() {
    let bare = bin_cmd().args(["contracts", "check"]).output().expect("binary を起動できる");
    assert_eq!(bare.status.code(), Some(i32::from(RC_REFUSED)), "--repo 無しは rc 1");
    assert!(stderr_of(&bare).contains("--repo が要る"), "{}", stderr_of(&bare));
    let none = bin_cmd().arg("contracts").output().expect("binary を起動できる");
    assert_eq!(none.status.code(), Some(i32::from(RC_REFUSED)), "subcommand 無しは rc 1");
    assert!(stderr_of(&none).contains("contracts <check"), "使い方を出す: {}", stderr_of(&none));
    let dir = tmp();
    let out = contracts_check(&dir);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "git repo でない: {}", stderr_of(&out));
    assert!(out.stdout.is_empty() && stderr_of(&out).contains("git repo でない"), "{}", stderr_of(&out));
    clean(&[&dir]);
}

/// Declared 行の歯の置き場の検出線（設計 contract-source.md §45・行 av）: 歯が write-set の内 / 外 / base に 0 本で解けない
/// Declared 行 3 本の repo で、`--verbose` の周は外の行の 1 行（doc・行 id・file）が判定行の前に出て、旗の無い周は 0 行・
/// 判定行はどちらも `place-out=1/3`。外の行は旗に依らず findings の 1 件（`contract-table:teeth-outside-write-set`・行 aw）
/// で rc 1。引数の無い周の使い方の文字列に `--verbose` が載る。
#[test]
fn contract_check_place_verbose_names_the_outside_row_only_with_the_flag() {
    let rows = [
        declared_teeth_row("in", "derive_ok", "[\"crates/toy/src/tint.rs\", \"crates/toy/tests/e2e.rs\"]"),
        declared_teeth_row("out", "derive_ok", "[\"crates/toy/src/tint.rs\"]"),
        declared_teeth_row("none", "fresh_", "[\"crates/toy/src/tint.rs\"]"),
    ];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    let rules = ceiling_rules(&state);
    let check = |extra: &[&str]| {
        let args = ["contracts", "check", "--rules", rules.as_str(), "--repo"];
        bin_cmd().args(args).arg(&repo).args(extra).output().expect("binary を起動できる")
    };
    let (quiet, loud) = (check(&[]), check(&["--verbose"]));
    let judgement = "contracts check: docs=1 rows=3 untracked=0 findings=1 place-out=1/3";
    let hit = "contracts place-out: docs/design/toy.md 行 out の歯の file が write-set の外: crates/toy/tests/e2e.rs";
    let quiet_text = stdout_of(&quiet);
    let finding = quiet_text.lines().next().unwrap_or_default();
    assert!(
        finding.starts_with("contracts: docs/design/toy.md:")
            && finding.ends_with(" contract-table:teeth-outside-write-set: 行 out の歯の file が write-set の外: crates/toy/tests/e2e.rs"),
        "findings の 1 件は外の行: {quiet_text}"
    );
    assert_eq!(quiet.status.code(), Some(i32::from(RC_REFUSED)), "{quiet_text}{}", stderr_of(&quiet));
    assert_eq!(quiet_text.lines().collect::<Vec<&str>>(), [finding, judgement], "旗の無い周は当たった行 0 行");
    assert_eq!(loud.status.code(), Some(i32::from(RC_REFUSED)), "{}{}", stdout_of(&loud), stderr_of(&loud));
    assert_eq!(stdout_of(&loud).lines().collect::<Vec<&str>>(), [finding, hit, judgement], "旗の周は外の行 1 行 + 判定行");
    let usage = bin_cmd().arg("contracts").output().expect("binary を起動できる");
    assert!(stderr_of(&usage).contains("--verbose"), "使い方の文字列に旗が載る: {}", stderr_of(&usage));
    clean(&[&repo, &state]);
}

// ─────── 閉包の拡張（設計 docs/design/contract-source.md §3 の 4 点・§9・契約 (g)・`s2-07l.249`・接頭辞 `contract_closure_ext_`） ───────

/// (1) `surfaces`（第 5 形）: snapshot の名を宣言した行は snapshot の file とその名を持つ歯が write-set に無いと
/// `write-set-incomplete` で両方を名指し、subcommand の名を宣言した行は usage 文字列を持つ歯を名指す。未知の名は
/// `contract-table:surface-unknown`。宣言なしの行と write-set が覆う行は名指さない。base は `surfaces` を読めない（RED）。
#[test]
fn contract_closure_ext_surfaces_name_the_snapshot_and_the_teeth_that_pin_it() {
    let rows = [
        table_row("a", &[("surfaces", "[\"doctor_external_form\"]")]),
        table_row("b", &[("surfaces", "[\"tint\"]")]),
        table_row("c", &[("surfaces", "[\"nope_external_form\"]")]),
        table_row("d", &[]),
        table_row(
            "e",
            &[
                ("surfaces", "[\"doctor_external_form\", \"tint\"]"),
                ("write-set", "[\"src/tint.rs\", \"src/snapshots/\", \"tests/e2e/\"]"),
            ],
        ),
    ];
    let doc = table_doc(&table_region(&rows));
    let repo = table_repo(&doc, SURFACE_FILES);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{text}{}", stderr_of(&out));
    let snapshot = findings_for(&found, &doc, "a", "write-set-incomplete");
    assert_eq!(snapshot.len(), 1, "行 a は 1 件に全部: {text}");
    for path in ["src/snapshots/toy__tests__doctor_external_form.snap", "tests/e2e/doctor.rs"] {
        assert!(snapshot.iter().all(|line| line.contains(path)), "snapshot の file と pin する歯 {path} を名指す: {text}");
    }
    assert!(snapshot.iter().all(|line| !line.contains("usage.rs") && !line.contains("cli.rs")), "外形の外は名指さない: {text}");
    let usage = findings_for(&found, &doc, "b", "write-set-incomplete");
    assert_eq!(usage.len(), 1, "行 b: {text}");
    assert!(usage.iter().all(|line| line.contains("tests/e2e/usage.rs") && !line.contains("doctor")), "usage 文字列を持つ歯: {text}");
    let unknown = findings_for(&found, &doc, "c", "contract-table:surface-unknown");
    assert_eq!(unknown.len(), 1, "未知の名: {text}");
    assert!(unknown.iter().all(|line| line.contains("nope_external_form")), "{text}");
    assert_eq!(found.len(), 3, "宣言なしの行 d と覆う行 e は名指さない: {text}");
    assert_eq!(text.lines().last(), Some("contracts check: docs=1 rows=5 untracked=0 findings=3 place-out=0/5"), "{text}");
    clean(&[&repo]);
}

/// (b) 肯定側: 素の名と `-` を含む名の usage 行はどちらも解け、その usage 文字列を持つ歯が導出値に入る（内側の
/// 論理和を積にすると素の名が、`-` との等値を非等値にすると `-` の名が落ちて `surface-unknown` で断られる）。
#[test]
fn contract_closure_ext_survivor_b_name_plain_and_dashed_usage_names_resolve() {
    // flip-check: retroactive s2-07l.277
    let (repo, state, out) = usage_name_intake("a", "[\"paint\", \"re-paint\"]");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "2 つの名が解けて通る: {}", stderr_of(&out));
    let copied = copied_write_set(&state, &run_id_of(&out));
    let want = ["crates/toy/tests/paint.rs", "crates/toy/tests/repaint.rs"];
    let hits = want.iter().filter(|path| copied.iter().any(|item| item == *path)).count();
    assert_eq!(hits, want.len(), "usage 文字列を持つ歯が導出値に入る（{hits} 件 / 母集団 名 {} 個・導出値 {copied:?}）", want.len());
    stop_run_ok(&state, &run_id_of(&out));
    clean(&[&repo, &state]);
}

/// (b) 否定側: 識別子の文字でも `-` でもない文字（`.`）を含む名の usage 行は捨てられ、その名は `surface-unknown` で
/// 断られる（外側の論理和を積にすると、`-` との等値を非等値にするとその名が解けて通る）。
#[test]
fn contract_closure_ext_survivor_b_match_name_with_other_chars_is_surface_unknown() {
    // flip-check: retroactive s2-07l.277
    let (repo, state, out) = usage_name_intake("b", "[\"pa.int\"]");
    let err = stderr_of(&out);
    let named = err.lines().filter(|line| line.contains("surface-unknown") && line.contains("pa.int")).count();
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "未知の名で断る: {err}");
    assert_eq!(named, 1, "pa.int を surface-unknown で名指す（{named} 件 / 母集団 stderr {} 行）: {err}", err.lines().count());
    assert_eq!(run_dirs(&state).len(), 0, "run を作らない");
    clean(&[&repo, &state]);
}

/// (2) 項目の実在と dir の展開: 無い file・空の dir は `write-set-item-unresolved` で 1 項目 1 件（実在する file・
/// 配下を持つ dir・base に無い `+`・**base に在る file への `+`〔契約表の検査は land 済みの実在 file と読む・
/// `s2-07l.346`〕** は通る）。intake の交差は dir を base の file に展開して数える＝`src/` の live な便と
/// `+src/new.rs` の便は交差 0 で通り、`src/lib.rs` の便は交差で断られる。
#[test]
fn contract_closure_ext_dir_items_expand_and_unresolved_items_are_named() {
    let write_set = "[\"src/none.rs\", \"empty/\", \"+src/tint.rs\", \"src/tint.rs\", \"src/\", \"+src/new.rs\"]";
    let doc = table_doc(&table_region(&[table_row("a", &[("write-set", write_set)])]));
    let repo = table_repo(&doc, &[]);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{text}{}", stderr_of(&out));
    let unresolved = findings_for(&found, &doc, "a", "write-set-item-unresolved");
    let named: Vec<&str> = ["src/none.rs", "empty/"]
        .into_iter()
        .filter(|item| unresolved.iter().any(|line| line.contains(&format!("write-set の {item} は"))))
        .collect();
    assert_eq!(named.len(), 2, "解けない 2 項目を名指す: {text}");
    assert_eq!(unresolved.len(), 2, "1 項目 1 件（解ける 4 項目は名指さない）: {text}");
    assert_eq!(found.len(), 2, "他の理由は出ない: {text}");
    clean(&[&repo]);

    let (repo, state) = repo_with_state();
    let dir_run = write_set_contract(&repo, "dir", &["src/"]);
    let id = intake_bead(&repo, &state, &dir_run, "s2-live");
    let fresh = write_set_contract(&repo, "fresh", &["+src/new.rs"]);
    let passed = try_intake(&repo, &state, &fresh, "s2-fresh");
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "新規 file は dir と交差しない: {}", stderr_of(&passed));
    let existing = write_set_contract(&repo, "existing", &["src/lib.rs"]);
    let refused = try_intake(&repo, &state, &existing, "s2-old");
    assert_eq!(refused.status.code(), Some(i32::from(RC_REFUSED)), "base の file は dir に展開されて交差する");
    assert!(stderr_of(&refused).contains(&id) && stderr_of(&refused).contains("src/lib.rs"), "{}", stderr_of(&refused));
    clean(&[&repo, &state]);
}

/// (3) 上限の余地（受付だけ）: base の 1399 行の `.rs`（上限 1500・余地 101）を write-set に持つ size M（300）の契約
/// は `cap-headroom` の理由で file と余地と size を名指して断られ、run dir も event も作らない。size S（100）は
/// 通り、余地の無い file を write-set に持たない M の契約も通る。core（`crates/toy/src/` の合計 1399・上限 1500）は
/// 新規 file だけの M でも見積 300 が余地 101 を超えて `core` を名指す。数は `--rules` の manifest から読む。
#[test]
fn contract_closure_ext_cap_headroom_refuses_a_size_that_does_not_fit_the_file_or_the_core() {
    let (repo, state) = repo_with_big_file();
    let caps = |core_lines: u64| CapFixture { core_lines, file_lines: 1_500 };
    let rules = |name: &str, core_lines: u64| {
        let fixture =
            RulesFixture { gate: (1, 1_000_000), retries: FOLLOW_RETRIES, slots: default_slots(), caps: caps(core_lines) };
        write_rules_capped(&state, name, fixture).display().to_string()
    };
    let roomy = rules("rules-roomy.toml", 40_000);
    let intake = |design: &str, bead: &str, rules: &str| intake_with_rules(&repo, &state, design, bead, rules);
    let sized = |id: &str, size: &str, write_set: &str| sized_contract(&repo, id, size, write_set);
    let big = "\"crates/toy/src/big.rs\"";
    let out = intake(&sized("m", "M", big), "s2-m", &roomy);
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "余地 101 に M（300）は入らない: {err}");
    assert!(err.contains("crates/toy/src/big.rs") && err.contains(" 101 ") && err.contains("size M"), "file と余地と size: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    let small = intake(&sized("s", "S", big), "s2-s", &roomy);
    assert_eq!(small.status.code(), Some(i32::from(RC_OK)), "S（100）は余地 101 に入る: {}", stderr_of(&small));
    stop_run_ok(&state, &run_id_of(&small));
    let other = intake(&sized("other", "M", "\"src/lib.rs\""), "s2-o", &roomy);
    assert_eq!(other.status.code(), Some(i32::from(RC_OK)), "余地の無い file を持たない行は通る: {}", stderr_of(&other));
    stop_run_ok(&state, &run_id_of(&other));
    // core の形: 上限 1500 に対し合計 1399（余地 101）・新規 file 1 本の M の見積 300 が超える（file の余地は 1500）。
    let tight = rules("rules-tight.toml", 1_500);
    let core = intake(&sized("core", "M", "\"+crates/toy/src/new.rs\""), "s2-c", &tight);
    let err = stderr_of(&core);
    assert_eq!(core.status.code(), Some(i32::from(RC_REFUSED)), "core の余地 101 に 300 は入らない: {err}");
    assert!(err.contains("core の上限の余地が 101 行") && !err.contains("big.rs"), "core を名指す: {err}");
    let fits = intake(&sized("fits", "S", "\"+crates/toy/src/new.rs\""), "s2-f", &tight);
    assert_eq!(fits.status.code(), Some(i32::from(RC_OK)), "S の見積 100 は core の余地 101 に入る: {}", stderr_of(&fits));
    clean(&[&repo, &state]);
}

/// (6) 縮む面（`s2-07l.287`・設計 contract-source.md §3・接頭辞 `contract_closure_ext_shrink_`）: 余地 101 の 1399 行の
/// `.rs` を `-` で持つ size M（300）の契約は受付を**通り**（run dir と event 1 件）、同じ file を素の path で持つ M は
/// 従来どおり `cap-headroom`（対で測る＝`-` が効いた証拠）。
#[test]
fn contract_closure_ext_shrink_item_is_exempt_from_the_file_headroom() {
    let (repo, state) = repo_with_big_file();
    let roomy = capped_rules(&state, "rules-roomy.toml", 40_000);
    let plain = intake_with_rules(&repo, &state, &sized_contract(&repo, "m.toml", "M", "\"crates/toy/src/big.rs\""), "s2-m", &roomy);
    let err = stderr_of(&plain);
    assert_eq!(plain.status.code(), Some(i32::from(RC_REFUSED)), "素の path の M は余地 101 に入らない: {err}");
    assert!(err.contains("crates/toy/src/big.rs の上限の余地が 101 行") && err.contains("size M"), "cap-headroom: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    let shrink = intake_with_rules(&repo, &state, &sized_contract(&repo, "shrink.toml", "M", "\"-crates/toy/src/big.rs\""), "s2-sh", &roomy);
    assert_eq!(shrink.status.code(), Some(i32::from(RC_OK)), "- の big.rs は余地を求めない: {}", stderr_of(&shrink));
    let id = run_id_of(&shrink);
    assert!(state.join("pipe").join(&id).is_dir(), "run dir が作られる: {id}");
    assert_eq!(event_count(&state), 2, "RunCreated と審査の段（Reviewed）の 2 件");
    clean(&[&repo, &state]);
}

/// (6) `-` の先が base に無い項目は **`pipe intake` で** `write-set-item-unresolved` として項目の字面（`-` 込み）を
/// 名指して断り、run dir も event も作らない（落として測ると「余地を求めない」宣言が静かに消える）。
#[test]
fn contract_closure_ext_shrink_item_absent_from_base_is_refused_at_intake() {
    let (repo, state) = repo_with_big_file();
    let roomy = capped_rules(&state, "rules-roomy.toml", 40_000);
    let contract = sized_contract(&repo, "none.toml", "M", "\"-crates/toy/src/none.rs\"");
    let out = intake_with_rules(&repo, &state, &contract, "s2-n", &roomy);
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "base に無い file への - は解けない: {err}");
    assert!(err.contains("write-set の -crates/toy/src/none.rs は base に解けない"), "項目の字面（- 込み）を名指す: {err}");
    assert!(!err.contains("cap-headroom") && !err.contains("上限の余地"), "理由は解けない項目の 1 つ: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&repo, &state]);
}

/// (6) core の見積: core の余地を「縮む面を数えると超え・数えなければ入る」150（上限 1549・合計 1399）に置く。size S
/// （100）で `-big.rs` + `+new.rs` は見積 100 × 1 本で通り、素の `big.rs` + `+new.rs` は 100 × 2 本 = 200 が超えて
/// `core` を名指して断られる（file の余地 101 は S に足りるので、名指すのは core だけ）。
#[test]
fn contract_closure_ext_shrink_item_is_not_counted_in_the_core_estimate() {
    let (repo, state) = repo_with_big_file();
    let tight = capped_rules(&state, "rules-tight.toml", 1_549);
    let both = "\"crates/toy/src/big.rs\", \"+crates/toy/src/new.rs\"";
    let plain = intake_with_rules(&repo, &state, &sized_contract(&repo, "plain.toml", "S", both), "s2-p", &tight);
    let err = stderr_of(&plain);
    assert_eq!(plain.status.code(), Some(i32::from(RC_REFUSED)), "2 本の見積 200 は core の余地 150 に入らない: {err}");
    assert!(err.contains("core の上限の余地が 150 行") && !err.contains("big.rs の上限"), "core を名指す: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    let shrunk = "\"-crates/toy/src/big.rs\", \"+crates/toy/src/new.rs\"";
    let shrink = intake_with_rules(&repo, &state, &sized_contract(&repo, "shrink.toml", "S", shrunk), "s2-sc", &tight);
    assert_eq!(shrink.status.code(), Some(i32::from(RC_OK)), "- を数えない 1 本の見積 100 は余地 150 に入る: {}", stderr_of(&shrink));
    assert_eq!(event_count(&state), 2, "RunCreated と審査の段（Reviewed）の 2 件");
    clean(&[&repo, &state]);
}

// ── 着地で消える file の宣言（`~`・設計 contract-source.md §24・行 x・`s2-07l.405`・接頭辞 `contract_closure_ext_delete_`） ──

/// `~` の項目が base に在る行は受付を通り（run dir と event 1 件）、受付はその項目を**接頭辞を剥がした素の path**で
/// 読む（剥がす規則は `normalize` の 1 本＝交差の照合も同じ 1 本を通る）: 素の path を書いた 2 本目は live な `~` の
/// 便と交差して断られ、別の file の便は交差しない（base は `~` を剥がさないので交差 0 で通る＝RED）。
#[test]
fn contract_closure_ext_delete_intake_accepts_existing_and_strips_prefix() {
    let (repo, state) = repo_with_state();
    let doomed = write_set_contract(&repo, "delete", &["~src/lib.rs"]);
    let out = try_intake(&repo, &state, &doomed, "s2-del");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "base に在る file への ~ は受付を通る: {}", stderr_of(&out));
    let id = run_id_of(&out);
    assert!(state.join("pipe").join(&id).is_dir(), "run dir が作られる: {id}");
    assert_eq!(event_count(&state), 2, "RunCreated と審査の段（Reviewed）の 2 件");
    let plain = write_set_contract(&repo, "plain", &["src/lib.rs"]);
    let crossed = try_intake(&repo, &state, &plain, "s2-plain");
    let err = stderr_of(&crossed);
    assert_eq!(crossed.status.code(), Some(i32::from(RC_REFUSED)), "~ の便と素の path の便は同じ面を触る: {err}");
    assert!(err.contains(&id) && err.contains("src/lib.rs"), "1 本目の run id と交差した path を名乗る: {err}");
    let apart = try_intake(&repo, &state, &write_set_contract(&repo, "other", &["+src/new.rs"]), "s2-apart");
    assert_eq!(apart.status.code(), Some(i32::from(RC_OK)), "別の file の便は交差しない: {}", stderr_of(&apart));
    clean(&[&repo, &state]);
}

/// `~` の先が base に無い項目は **`pipe intake` で** `write-set-item-unresolved` として項目の字面（`~` 込み）を名指して
/// 断り、run dir も event も作らない（消す予定の file が無い＝宣言の誤りを入口で止める・落として測ると黙って通る）。
#[test]
fn contract_closure_ext_delete_intake_refuses_missing_file() {
    let (repo, state) = repo_with_state();
    let out = try_intake(&repo, &state, &write_set_contract(&repo, "none", &["~src/none.rs"]), "s2-none");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "base に無い file への ~ は解けない: {err}");
    assert!(err.contains("write-set の ~src/none.rs は base に解けない"), "項目の字面（~ 込み）を名指す: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&repo, &state]);
}

/// 契約表の検査（`MayBeLanded` の場面）は `~` の項目を **2 分岐とも**解く: tracked から消した後（着地の後）も、まだ
/// 消していない周も findings 0・rc 0（着地済みの行が `write-set-item-unresolved` で永久に赤くなる型を塞ぐ）。
#[test]
fn contract_closure_ext_delete_check_passes_after_landing() {
    let doc = table_doc(&table_region(&[table_row("a", &[("write-set", "[\"src/tint.rs\", \"~src/old.rs\"]")])]));
    let present = table_repo(&doc, &[("src/old.rs", "// old\n")]);
    let out = contracts_check(&present);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "まだ消していない周の ~ も解ける: {}", stdout_of(&out));
    assert_eq!(stdout_of(&out).trim_end(), "contracts check: docs=1 rows=1 untracked=0 findings=0 place-out=0/1");
    let landed = table_repo(&doc, &[("src/old.rs", "// old\n")]);
    git(&landed, &["rm", "-q", "src/old.rs"]);
    git(&landed, &["commit", "-q", "-m", "land"]);
    let out = contracts_check(&landed);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "着地で消えた ~ も解ける: {}", stdout_of(&out));
    assert_eq!(stdout_of(&out).trim_end(), "contracts check: docs=1 rows=1 untracked=0 findings=0 place-out=0/1");
    clean(&[&present, &landed]);
}

/// 名指しの実在（§24 (3)）: 節の本文が行の `~` の項目と等しい path 形を backtick で名指しても、着地（tracked から
/// 消えた後）に `name-unresolved` にならない。除外の無い別の path（`src/gone.rs`）は名指されたまま＝空虚でない対。
#[test]
fn contract_closure_ext_delete_name_in_section_resolves_after_landing() {
    let body = "本文。`src/old.rs` は行の消える file・`src/gone.rs` は write-set に無い。";
    let doc = table_doc(&table_region(&[table_row("a", &[("write-set", "[\"src/tint.rs\", \"~src/old.rs\"]")])]))
        .replace("## 1. 何を解くか\n\n本文。", &format!("## 1. 何を解くか\n\n{body}"));
    let repo = table_repo(&doc, &[("src/old.rs", "// old\n")]);
    git(&repo, &["rm", "-q", "src/old.rs"]);
    git(&repo, &["commit", "-q", "-m", "land"]);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{text}{}", stderr_of(&out));
    let names = findings_for(&found, &doc, "a", "name-unresolved");
    assert!(names.iter().all(|line| !line.contains("src/old.rs")), "~ の項目と等しい名指しは着地の後も解ける: {text}");
    assert_eq!(names.len(), 1, "解けない名指しは 1 件だけ: {text}");
    assert!(names.iter().all(|line| line.contains("名指し src/gone.rs が base に無い")), "除外の無い path は名指したまま: {text}");
    assert_eq!(found.len(), 1, "他の理由は出ない（~ の項目は write-set-item-unresolved にならない）: {text}");
    clean(&[&repo]);
}

/// 上限の余地: `~` の項目は増分が負なので **file の余地も core の見積の本数も**求めない（`-` と同じ扱い）。余地 101 の
/// 1399 行の `.rs` を `~` で持つ size M（300）の契約は受付を**通り**（run dir と event 1 件）、同じ file を素の path で
/// 持つ M は従来どおり `cap-headroom`。core も対で測る: 余地 150 に `~big.rs` + `+new.rs` の見積 100 × 1 本は入り、素の
/// 2 本 200 は超えて `core` を名指して断られる（`~` を `File` / `New` と同じ本数に数えると後段が赤くなる）。
#[test]
fn contract_closure_ext_delete_does_not_count_headroom() {
    let (repo, state) = repo_with_big_file();
    let roomy = capped_rules(&state, "rules-roomy.toml", 40_000);
    let plain = intake_with_rules(&repo, &state, &sized_contract(&repo, "m.toml", "M", "\"crates/toy/src/big.rs\""), "s2-mb", &roomy);
    let err = stderr_of(&plain);
    assert_eq!(plain.status.code(), Some(i32::from(RC_REFUSED)), "素の path の M は余地 101 に入らない: {err}");
    assert!(err.contains("crates/toy/src/big.rs の上限の余地が 101 行") && err.contains("size M"), "cap-headroom: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    let doomed = intake_with_rules(&repo, &state, &sized_contract(&repo, "del.toml", "M", "\"~crates/toy/src/big.rs\""), "s2-db", &roomy);
    assert_eq!(doomed.status.code(), Some(i32::from(RC_OK)), "~ の big.rs は余地を求めない: {}", stderr_of(&doomed));
    let id = run_id_of(&doomed);
    assert!(state.join("pipe").join(&id).is_dir(), "run dir が作られる: {id}");
    assert_eq!(event_count(&state), 2, "RunCreated と審査の段（Reviewed）の 2 件");
    stop_run_ok(&state, &id);
    // core の見積の本数（上限 1549・合計 1399＝余地 150・size S の見積は 1 本 100 行）。
    let tight = capped_rules(&state, "rules-tight.toml", 1_549);
    let both = "\"crates/toy/src/big.rs\", \"+crates/toy/src/new.rs\"";
    let counted = intake_with_rules(&repo, &state, &sized_contract(&repo, "both.toml", "S", both), "s2-bo", &tight);
    let err = stderr_of(&counted);
    assert_eq!(counted.status.code(), Some(i32::from(RC_REFUSED)), "2 本の見積 200 は core の余地 150 に入らない: {err}");
    assert!(err.contains("core の上限の余地が 150 行") && !err.contains("big.rs の上限"), "core を名指す: {err}");
    let gone = "\"~crates/toy/src/big.rs\", \"+crates/toy/src/new.rs\"";
    let apart = intake_with_rules(&repo, &state, &sized_contract(&repo, "gone.toml", "S", gone), "s2-go", &tight);
    assert_eq!(apart.status.code(), Some(i32::from(RC_OK)), "~ を数えない 1 本の見積 100 は余地 150 に入る: {}", stderr_of(&apart));
    assert!(state.join("pipe").join(run_id_of(&apart)).is_dir(), "run dir が作られる");
    clean(&[&repo, &state]);
}

/// 置き場だけの `=`（§43 (1)・行 ar・接頭辞 `contract_place_only_`）: 余地 101 の 1399 行の `.rs` を `=` で持つ size M
/// （300）の行は受付を**通り** run dir が出来る。同じ行から印だけを外すと従来の `cap-headroom` の字面で rc 1・run dir も
/// event も作らない（母集団 = 2 回の受付の rc を対で見る）。
#[test]
fn contract_place_only_item_passes_intake_and_unmarked_is_refused_by_headroom() {
    let (repo, state) = repo_with_big_file();
    let roomy = capped_rules(&state, "rules-roomy.toml", 40_000);
    let plain = intake_with_rules(&repo, &state, &sized_contract(&repo, "m.toml", "M", "\"crates/toy/src/big.rs\""), "s2-pm", &roomy);
    let err = stderr_of(&plain);
    assert_eq!(plain.status.code(), Some(i32::from(RC_REFUSED)), "印の無い M は余地 101 に入らない: {err}");
    assert!(err.contains("crates/toy/src/big.rs の上限の余地が 101 行") && err.contains("size M"), "cap-headroom: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    let place = intake_with_rules(&repo, &state, &sized_contract(&repo, "place.toml", "M", "\"=crates/toy/src/big.rs\""), "s2-pp", &roomy);
    assert_eq!(place.status.code(), Some(i32::from(RC_OK)), "= の big.rs は余地を求めない: {}", stderr_of(&place));
    let id = run_id_of(&place);
    assert!(state.join("pipe").join(&id).is_dir(), "run dir が作られる: {id}");
    assert_eq!(event_count(&state), 2, "RunCreated と審査の段（Reviewed）の 2 件");
    clean(&[&repo, &state]);
}

/// (5) 行の数え方（`s2-07l.254`・設計 rules-manifest.md §4・接頭辞 `contract_closure_ext_width_`）: base の `.rs` が短い
/// 1399 行と 2000 字を詰めた 1 行を持つとき、余地は改行の数（1400 行 → 100）でなく幅（`--rules` の `R-C4.line-width`）で
/// 正規化した行数で出て、改行の数なら入る size S（100）が `cap-headroom` で断られる（詰め込みで余地が増えない）。
#[test]
fn contract_closure_ext_width_packed_line_does_not_widen_the_headroom() {
    let (repo, state) = repo_with_state();
    let dir = repo.join("crates").join("toy").join("src");
    fs::create_dir_all(&dir).expect("core の dir を作れる");
    fs::write(dir.join("packed.rs"), format!("{}{}\n", "// x\n".repeat(1399), "x".repeat(2000))).expect("file を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "packed"]);
    let width = embedded_int("R-C4.line-width");
    let headroom = 1_500 - 1_399 - 2_000_u64.div_ceil(width);
    assert!(headroom < 100, "幅で数えた余地は改行の数の余地（100）より小さい: {headroom}");
    let fixture = RulesFixture {
        gate: (1, 1_000_000),
        retries: FOLLOW_RETRIES,
        slots: default_slots(),
        caps: CapFixture { core_lines: 40_000, file_lines: 1_500 },
    };
    let rules = write_rules_capped(&state, "rules-width.toml", fixture).display().to_string();
    let design = sized_contract(&repo, "s", "S", "\"crates/toy/src/packed.rs\"");
    let out = run_pipe(&[
        "intake", "--design", &design, "--bead", "s2-w",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(), "--rules", &rules,
    ]);
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "詰め込んだ 1 行は幅で数える: {err}");
    let named = format!("crates/toy/src/packed.rs の上限の余地が {headroom} 行");
    assert!(err.contains(&named) && err.contains("size S"), "file と幅で数えた余地と size: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// (4) 名指しの実在: `title` / `done` / 節の本文の backtick の中身のうち path 形・型の path 形・fn 形が base に解けない
/// ものを `name-unresolved` で全件・在り処付き（行番号は行の見出し）。`touches` の型の variant・write-set の `+`
/// 宣言の新規 file（別の行の宣言も含む・§39）・一致しない字面は名指さない。
#[test]
fn contract_closure_ext_unresolved_names_are_named_with_their_place() {
    let body = "本文。`crate::tint::Tint` と `show(` は在る。`Nope::Thing` と `src/nope.rs` は無い。`Tint::Warm => 1` は字面。";
    let doc = table_doc(&table_region(&[
        table_row("a", &[("title", "\"`src/none.rs` を直す\""), ("done", "\"`Tint::Hot` と `frob(` が通る\"")]),
        table_row("b", &[("done", "\"`Tint::Hot` は touches の型・`src/new.rs` は + 宣言\""), ("touches", "[\"crate::tint::Tint\"]"), ("write-set", "[\"src/tint.rs\", \"src/show.rs\", \"+src/new.rs\"]")]),
        table_row("c", &[("done", "\"`src/new.rs` は write-set に無い\"")]),
    ]))
    .replace("## 1. 何を解くか\n\n本文。", &format!("## 1. 何を解くか\n\n{body}"));
    let repo = table_repo(&doc, &[]);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{text}{}", stderr_of(&out));
    let body_line = doc.lines().position(|line| line.starts_with("本文。`crate")).unwrap_or_default() + 1;
    let row_a = findings_for(&found, &doc, "a", "name-unresolved");
    let want_a = [
        ("src/none.rs", "title".to_owned()),
        ("Tint::Hot", "done".to_owned()),
        ("frob(", "done".to_owned()),
        ("Nope::Thing", format!("section 1 line {body_line}")),
        ("src/nope.rs", format!("section 1 line {body_line}")),
    ];
    assert_eq!(row_a.len(), want_a.len(), "行 a は解けない名指しを全件: {text}");
    for ((name, at), line) in want_a.iter().zip(&row_a) {
        assert!(line.contains(&format!("名指し {name} が base に無い（{at}）")), "{name} を {at} で名指す: {line}");
    }
    let row_b = findings_for(&found, &doc, "b", "name-unresolved");
    let named_b: Vec<&String> = row_b.iter().filter(|line| line.contains("Tint::Hot") || line.contains("src/new.rs")).collect();
    assert!(named_b.is_empty(), "touches の型の variant と + 宣言の新規 file は名指さない: {row_b:?}");
    assert_eq!(row_b.len(), 2, "行 b も節の本文の 2 件は持つ（Nope::Thing / src/nope.rs）: {text}");
    let row_c = findings_for(&found, &doc, "c", "name-unresolved");
    assert!(!row_c.iter().any(|line| line.contains("src/new.rs")), "別の行が + で宣言した新規 file は解ける（§39）: {text}");
    assert!(!text.contains("Tint::Warm"), "一致しない字面（arm の断片）は名指さない: {text}");
    clean(&[&repo]);
}

/// (5) 現物の契約表（本 repo の `docs/design/*.md`）は 4 つの拡張（外形 pin・項目の実在と展開・名指しの実在・
/// 余地は CI で撃たない）を含めて違反 0・rc 0（設計 §9「現物の契約表で 4 つとも違反 0」）。
#[test]
fn contract_closure_ext_real_table_has_zero_findings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo の root を解ける");
    let out = contracts_check(root);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "現物の契約表は違反 0: {text}{}", stderr_of(&out));
    let last = text.lines().last().unwrap_or_default();
    let tokens: Vec<&str> = last.split_whitespace().collect();
    assert!(last.starts_with("contracts check: docs=") && tokens.contains(&"findings=0"), "判定行: {last}");
    assert!(tokens.last().is_some_and(|token| token.starts_with("place-out=")), "末尾は置き場の検出線の欄: {last}");
    let rows: u64 = last.split_whitespace().find_map(|token| token.strip_prefix("rows=")?.parse().ok()).unwrap_or_default();
    assert!(rows >= 8, "母集団は現物の契約表の行（contract-source.md の 8 行以上・空の表で 0 件を名乗らない）: {last}");
}

// ─────── 名指しの実在の impl 経路（設計 docs/design/contract-source.md §26・§3 (2)・`s2-07l.432`・接頭辞 `contract_names_impl_`） ───────

/// impl 経路（§26）: base が宣言する method / 関連 fn の「型::項目」は `contracts check` で解け、同じ 1 語の形で
/// 並ぶ実在しない `Report::nope` だけが `name-unresolved` で名指される（done と § 本文の 2 か所ぶんの 2 行）・rc 1。
/// base（字面の経路だけ）では実在の 2 語も名指されて findings が 6 件になる（偽陽性・C16）。
#[test]
fn contract_names_impl_method_is_not_named_by_contracts_check() {
    let named = "`Report::violation` と `Wide::width` は在る。`Report::nope` は無い。";
    let doc = table_doc(&table_region(&[table_row("a", &[("done", &format!("\"{named}\""))])]))
        .replace("## 1. 何を解くか\n\n本文。", &format!("## 1. 何を解くか\n\n{named}"));
    let repo = table_repo(&doc, IMPL_FILES);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "実在しない 1 語で rc 1: {text}{}", stderr_of(&out));
    let body_line = doc.lines().position(|line| line.starts_with("`Report::violation`")).unwrap_or_default() + 1;
    let row_a = findings_for(&found, &doc, "a", "name-unresolved");
    let want = [("Report::nope", "done".to_owned()), ("Report::nope", format!("section 1 line {body_line}"))];
    assert_eq!(row_a.len(), want.len(), "名指すのは実在しない 1 語の 2 か所だけ: {text}");
    for ((name, at), line) in want.iter().zip(&row_a) {
        assert!(line.contains(&format!("名指し {name} が base に無い（{at}）")), "{name} を {at} で名指す: {line}");
    }
    for resolved in ["Report::violation", "Wide::width"] {
        assert!(!text.contains(resolved), "base が impl で宣言する {resolved} は名指さない: {text}");
    }
    assert_eq!(text.lines().last(), Some("contracts check: docs=1 rows=1 untracked=0 findings=2 place-out=0/1"), "判定行: {text}");
    clean(&[&repo]);
}

// ─────── 宣言済みの新規 file（設計 docs/design/contract-source.md §39・行 an・`s2-07l.475`・接頭辞 `contract_names_declared_`） ───────

/// 宣言の側の doc（`docs/design/other.md`）: 行 `x` が write-set の `+` で `fresh` を、行 `y` が `creates` で `made` を
/// 宣言する（どちらも base に無い）。
fn declaring_doc(fresh: &str, made: &str) -> String {
    let plus = format!("[\"src/tint.rs\", \"+{fresh}\"]");
    let creates = format!("[\"{made}\"]");
    table_doc(&table_region(&[
        table_row("x", &[("write-set", &plus)]),
        table_row("y", &[("write-set", "[\"src/tint.rs\"]"), ("creates", &creates)]),
    ]))
}

/// (a) doc A（other.md）の行が宣言した新規 file を doc B（toy.md）の行の done が backtick で名指すと findings 0・rc 0
/// （base は行 1 本の write-set だけを解に読むので `name-unresolved` が 2 件 → RED）。`+` の項目と `creates` の欄の両方。
#[test]
fn contract_names_declared_in_another_doc_resolve() {
    let doc = table_doc(&table_region(&[table_row("b", &[("done", "\"`src/fresh.rs` と `src/made.rs` が通る\"")])]));
    let repo = table_repo(&doc, &[("docs/design/other.md", &declaring_doc("src/fresh.rs", "src/made.rs"))]);
    let out = contracts_check(&repo);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "別の doc の宣言で解ける: {text}{}", stderr_of(&out));
    assert!(!text.contains("name-unresolved"), "名指さない: {text}");
    assert_eq!(text.lines().last(), Some("contracts check: docs=2 rows=3 untracked=0 findings=0 place-out=0/2"), "判定行: {text}");
    clean(&[&repo]);
}

/// (b) どの行も宣言していない名は従来どおり `name-unresolved` の 1 件・rc 1（母集団は宣言の印の在る項目だけ＝印の
/// 無い write-set の項目〔`src/tint.rs`〕や他の名まで無条件に広がらない）。
#[test]
fn contract_names_declared_undeclared_name_is_still_named_once() {
    let doc = table_doc(&table_region(&[table_row("b", &[("done", "\"`src/fresh.rs` と `src/ghost.rs`\"")])]));
    let repo = table_repo(&doc, &[("docs/design/other.md", &declaring_doc("src/fresh.rs", "src/made.rs"))]);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "宣言の無い名は断る: {text}{}", stderr_of(&out));
    let row_b = findings_for(&found, &doc, "b", "name-unresolved");
    assert_eq!(row_b.len(), 1, "宣言の無い 1 語だけ: {text}");
    assert!(row_b.iter().all(|line| line.contains("名指し src/ghost.rs が base に無い（done）")), "{row_b:?}");
    assert_eq!(text.lines().last(), Some("contracts check: docs=2 rows=3 untracked=0 findings=1 place-out=0/2"), "判定行: {text}");
    clean(&[&repo]);
}

/// (c) 同じ doc の別の行の宣言でも解ける（`+` の項目と `creates` の欄）。
#[test]
fn contract_names_declared_in_another_row_of_the_same_doc_resolve() {
    let region = table_region(&[
        table_row("x", &[("write-set", "[\"src/tint.rs\", \"+src/fresh.rs\"]")]),
        table_row("y", &[("write-set", "[\"src/tint.rs\"]"), ("creates", "[\"src/made.rs\"]")]),
        table_row("b", &[("done", "\"`src/fresh.rs` と `src/made.rs` が通る\"")]),
    ]);
    let repo = table_repo(&table_doc(&region), &[]);
    let out = contracts_check(&repo);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "同じ doc の別の行の宣言で解ける: {text}{}", stderr_of(&out));
    assert_eq!(text.lines().last(), Some("contracts check: docs=1 rows=3 untracked=0 findings=0 place-out=0/2"), "判定行: {text}");
    clean(&[&repo]);
}

/// (d) 受付でも同じ母集団: doc B の行を pointer に受付を撃つと run dir と event が作られる（base は `name-unresolved`
/// で断る → RED）。対: 宣言の無い名を名指す行は従来どおり断られ、run dir も event も作らない。
#[test]
fn contract_names_declared_in_another_doc_pass_intake() {
    let row = |done: &str| {
        let done = format!("\"{done}\"");
        table_doc(&table_region(&[table_row("b", &[("write-set", "[\"crates/toy/src/tint.rs\"]"), ("done", &done)])]))
    };
    let other = ("docs/design/other.md", declaring_doc("crates/toy/src/fresh.rs", "crates/toy/src/made.rs"));
    let (repo, state) = derive_repo_with(&row("`crates/toy/src/fresh.rs` が通る"), &[(other.0, &other.1)]);
    let out = intake_raw(&repo, &state, "docs/design/toy.md#b", "s2-declared");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "別の doc の宣言で受付を通る: {}", stderr_of(&out));
    let id = run_id_of(&out);
    assert!(state.join("pipe").join(&id).join("contract.toml").is_file(), "run dir を作る");
    assert!(event_count(&state) >= 1, "event を書く");
    stop_run_ok(&state, &id);
    clean(&[&repo, &state]);
    let (repo, state) = derive_repo_with(&row("`crates/toy/src/ghost.rs` が通る"), &[(other.0, &other.1)]);
    let out = intake_raw(&repo, &state, "docs/design/toy.md#b", "s2-ghost");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "宣言の無い名は受付で断る: {err}");
    assert!(err.contains("名指し crates/toy/src/ghost.rs が base に無い（done）"), "従来の字面: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&repo, &state]);
}

/// (4) 区間を読めない doc が在る周は宣言の母集団を縮めたまま通さない: 読めない doc の従来の 1 件（行番号 0）に加え、
/// 名指しを測る行にも `contract-table:unreadable` が出て rc 2・その行に `name-unresolved` は出ない。
#[test]
fn contract_names_declared_unreadable_doc_fails_closed() {
    let doc = table_doc(&table_region(&[table_row("b", &[("done", "\"`src/fresh.rs` が通る\"")])]));
    let repo = table_repo(&doc, &[("docs/design/other.md", &declaring_doc("src/fresh.rs", "src/made.rs"))]);
    fs::write(repo.join("docs/design/bad.md"), [0xff, 0xfe, b'\n']).expect("非 UTF-8 の doc を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "bad"]);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない doc は rc 2: {text}");
    assert!(text.contains("contracts: docs/design/bad.md:0 contract-table:unreadable"), "読めない doc を名指す: {text}");
    let row_b = findings_for(&found, &doc, "b", "contract-table:unreadable");
    assert_eq!(row_b.len(), 1, "名指しを測る行に読めなさの 1 件: {text}");
    assert!(row_b.iter().all(|line| line.contains("docs/design/bad.md")), "読めない doc を名乗る: {row_b:?}");
    assert!(findings_for(&found, &doc, "b", "name-unresolved").is_empty(), "縮めた母集団で名指さない: {text}");
    clean(&[&repo]);
}

/// (e) 現物の契約表（本 repo の `docs/design/*.md`）は宣言済みの母集団を足しても findings 0・rc 0。
#[test]
fn contract_names_declared_real_table_has_zero_findings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo の root を解ける");
    let out = contracts_check(root);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "現物の契約表は違反 0: {text}{}", stderr_of(&out));
    let last = text.lines().last().unwrap_or_default();
    let tokens: Vec<&str> = last.split_whitespace().collect();
    assert!(last.starts_with("contracts check: docs=") && tokens.contains(&"findings=0"), "判定行: {last}");
    assert!(tokens.last().is_some_and(|token| token.starts_with("place-out=")), "末尾は置き場の検出線の欄: {last}");
}

// ─────── land 済みの `+`（設計 docs/design/contract-source.md §3・契約 (i)・`s2-07l.346`・接頭辞 `contract_table_landed_plus_`） ───────

/// (a) 契約表の行が `+crates/toy/src/new.rs` を持ち、その file を commit した base（land 後の main の形）で `contracts check`
/// を撃つと findings 0・rc 0（base は `write-set-item-unresolved` 1 件 → RED）。対: 同じ行で file がまだ無い base も 0
/// （新規 file の宣言として解ける＝land の前後で行の字面を変えずに緑）。
#[test]
fn contract_table_landed_plus_item_resolves_as_file() {
    let landed = table_repo(&landed_plus_doc(), &[("crates/toy/src/new.rs", "pub fn landed() {}\n")]);
    let out = contracts_check(&landed);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land 済みの + は実在 file と読む: {text}{}", stderr_of(&out));
    assert!(!text.contains("write-set-item-unresolved"), "解けない項目として名指さない: {text}");
    assert_eq!(text.trim_end(), "contracts check: docs=1 rows=1 untracked=0 findings=0 place-out=0/1", "判定行: {text}");
    let fresh = table_repo(&landed_plus_doc(), &[]);
    let out = contracts_check(&fresh);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land 前の + は新規 file として解ける: {}", stdout_of(&out));
    assert_eq!(stdout_of(&out).trim_end(), "contracts check: docs=1 rows=1 untracked=0 findings=0 place-out=0/1");
    clean(&[&landed, &fresh]);
}

/// (b) 同じ契約（行 `i` を指し `+crates/toy/src/new.rs` を write-set に持つ）を、その file が base に在る repo の intake に
/// 出すと `write-set-item-unresolved` で項目の字面（`+` 込み）を名指して断り、run dir も event も作らない（intake は
/// `MustBeAbsent`・契約表の検査を緩めても入口は緩まない＝退行の pin）。対: file の無い base では通る。
#[test]
fn contract_table_landed_plus_intake_still_refuses() {
    let (landed, state) = derive_repo_with(&landed_plus_doc(), &[("crates/toy/src/new.rs", "pub fn landed() {}\n")]);
    let out = intake_raw(&landed, &state, &landed_plus_contract(&landed), "s2-landed");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "base に在る file への + は受付で断る: {err}");
    assert!(err.contains(&format!("write-set の {LANDED_PLUS_ITEM} は base に解けない")), "項目の字面（+ 込み）を名指す: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&landed, &state]);
    let (fresh, state) = derive_repo(&landed_plus_doc());
    let out = intake_raw(&fresh, &state, &landed_plus_contract(&fresh), "s2-fresh");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "base に無い file への + は通る: {}", stderr_of(&out));
    stop_run_ok(&state, &run_id_of(&out));
    clean(&[&fresh, &state]);
}

// ─────── write-set の導出（設計 docs/design/contract-source.md §3「write-set の導出」・契約 (h)・`s2-07l.311`・接頭辞 `contract_derive_`） ───────

/// (a) `write-set` の無い行（`touches` + `verify` + `creates` + `tests` + `also`）は intake を通り、写しの契約の write-set に
/// 導出値（閉包 ∪ 歯の置き場 ∪ tests ∪ creates〔`+` 付き〕∪ also・辞書順）が載る・判定行 `write-set=derived files=<N>`・
/// 他の欄は逐語。base は `write-set` 必須で断る（RED）。
#[test]
fn contract_derive_fills_write_set_for_a_row_without_one() {
    let row = derive_row(
        "a",
        &[
            ("touches", "[\"crate::tint::Tint\"]"),
            ("verify", "[\"cargo nextest run -p toy --no-tests=fail derive_\"]"),
            ("creates", "[\"crates/toy/src/new.rs\"]"),
            ("tests", "[\"crates/toy/tests/helper.rs\"]"),
            ("also", "[\"rules/manifest.toml\"]"),
        ],
    );
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "a.toml", "a"), "s2-a");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "導出値で通る: {}", stderr_of(&out));
    let tokens = intake_tokens(&out);
    assert!(tokens.first().is_some_and(|token| token.starts_with("run=")), "既存 token が先頭のまま: {tokens:?}");
    assert!(tokens.contains(&"write-set=derived".to_owned()) && tokens.contains(&"files=7".to_owned()), "判定行: {tokens:?}");
    let id = run_id_of(&out);
    let want = [
        "+crates/toy/src/new.rs",
        "crates/toy/src/other.rs",
        "crates/toy/src/show.rs",
        "crates/toy/src/tint.rs",
        "crates/toy/tests/e2e.rs",
        "crates/toy/tests/helper.rs",
        "rules/manifest.toml",
    ];
    assert_eq!(copied_write_set(&state, &id), want, "写しの write-set は導出値（閉包 ∪ 歯の置き場 ∪ tests ∪ creates ∪ also）");
    let copied = fs::read_to_string(state.join("pipe").join(&id).join("contract.toml")).unwrap_or_default();
    // 契約 (b) 以後、写しの欄は**行**から来る（`verify` は行の逐語・新欄 `creates` / `tests` / `also` は写さない）。
    assert!(
        copied.contains("verify = [\"cargo nextest run -p toy --no-tests=fail derive_\"]"),
        "verify は行の逐語: {copied}"
    );
    for skipped in ["creates", "tests =", "also"] {
        assert!(!copied.contains(skipped), "新欄 {skipped} は写さない: {copied}");
    }
    assert_eq!(event_count(&state), 2, "RunCreated と審査の段（Reviewed）の 2 件");
    clean(&[&repo, &state]);
}

/// (b) 手書きの `write-set` が導出値とずれた行は `write-set-drift` で**余分**（src/lib.rs）を名指して断られ、run dir も
/// event も作らない。`+x` と `x` は同じ項目（new.rs は名指さない）。対: 一致する行は通り、写しの write-set は行の逐語。
///
/// **不足の側は表の検査が先に断る**（契約 (b)・`write-set-incomplete`＝閉包の file が write-set に無い）ので、
/// drift に届くのは余分だけである（不足は `contract_closure_ext_` の族が測る）。
#[test]
fn contract_derive_refuses_drift_naming_missing_and_extra() {
    let touches = ("touches", "[\"crate::tint::Tint\"]");
    let creates = ("creates", "[\"crates/toy/src/new.rs\"]");
    let rows = [
        derive_row("b", &[touches, creates, ("write-set", "[\"crates/toy/src/tint.rs\", \"crates/toy/src/show.rs\", \"+crates/toy/src/new.rs\", \"src/lib.rs\"]")]),
        derive_row("c", &[touches, creates, ("write-set", "[\"+crates/toy/src/new.rs\", \"crates/toy/src/show.rs\", \"crates/toy/src/tint.rs\"]")]),
    ];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "b.toml", "b"), "s2-b");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "ずれは rc 1: {err}");
    assert!(err.contains("write-set が導出値と一致しない"), "理由: {err}");
    assert!(err.contains("extra: src/lib.rs"), "余分を名指す: {err}");
    assert!(!err.contains("new.rs") && !err.contains("tint.rs"), "一致する項目（+ の有無は同じ）は名指さない: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    let passed = intake_raw(&repo, &state, &pointed_contract(&repo, "c.toml", "c"), "s2-c");
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "一致する手書きは通る: {}", stderr_of(&passed));
    let tokens = intake_tokens(&passed);
    assert!(tokens.contains(&"write-set=derived".to_owned()) && tokens.contains(&"files=3".to_owned()), "{tokens:?}");
    // 契約 (b) 以後、写しの write-set は**行の逐語**である（手書きの行は導出値で差し替えない）。
    assert_eq!(
        copied_write_set(&state, &run_id_of(&passed)),
        ["+crates/toy/src/new.rs", "crates/toy/src/show.rs", "crates/toy/src/tint.rs"],
        "手書きの在る行は写しを差し替えない"
    );
    clean(&[&repo, &state]);
}

/// (c) `also` の `.rs`・`tests` の歯でない file・新しい接頭辞で `tests` 無し、はそれぞれ typed に断られ run を作らない。
/// 新しい接頭辞は `tests` 欄（`creates` の新規 file）が置き場で、write-set には creates の側（`+` 付き）だけが載る。
#[test]
fn contract_derive_refuses_also_rs_and_tests_non_teeth_and_unresolved_filter() {
    let fresh = ("verify", "[\"cargo nextest run -p toy --no-tests=fail fresh_\"]");
    let rows = [
        derive_row("r", &[("also", "[\"crates/toy/src/tint.rs\"]")]),
        derive_row("t", &[("tests", "[\"crates/toy/src/tint.rs\"]")]),
        derive_row("f", &[fresh]),
        derive_row("p", &[fresh, ("creates", "[\"crates/toy/tests/fresh.rs\"]"), ("tests", "[\"crates/toy/tests/fresh.rs\"]")]),
    ];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    for (id, want) in [
        ("r", "also の crates/toy/src/tint.rs は .rs である"),
        ("t", "tests の crates/toy/src/tint.rs は歯の file でない"),
        ("f", "filter 語 fresh_ を含む #[test] の fn が base に無く tests 欄も無い"),
    ] {
        let out = intake_raw(&repo, &state, &pointed_contract(&repo, &format!("{id}.toml"), id), &format!("s2-{id}"));
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "行 {id} は rc 1: {err}");
        assert!(err.contains(want), "行 {id} の理由: {err}");
        assert_eq!(event_count(&state), 0, "断った周は event を書かない");
        assert!(!state.join("pipe").exists(), "run dir も作らない");
    }
    let placed = intake_raw(&repo, &state, &pointed_contract(&repo, "p.toml", "p"), "s2-p");
    assert_eq!(placed.status.code(), Some(i32::from(RC_OK)), "tests 欄が置き場になる: {}", stderr_of(&placed));
    assert_eq!(copied_write_set(&state, &run_id_of(&placed)), ["+crates/toy/tests/fresh.rs"], "creates の側だけが載る");
    clean(&[&repo, &state]);
}

/// (d) 新欄なし + `write-set` あり = `Declared`: 導出も drift も撃たず (g) までの検査だけで通り（判定行 `write-set=declared files=<N>`・写しは行の write-set のまま）。解けない pointer
/// （区間に無い行 id）は契約表の欠陥として断る。
#[test]
fn contract_derive_declared_rows_skip_derivation() {
    // 契約 (b) 以後、**Declared の行も閉包 ⊆ write-set** を表の検査が要る（導出と drift を撃たないだけ）。
    let row = table_row(
        "d",
        &[("touches", "[\"crate::tint::Tint\"]"), ("write-set", "[\"crates/toy/src/tint.rs\", \"crates/toy/src/show.rs\"]")],
    );
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "d.toml", "d"), "s2-d");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "Declared は導出しない: {}", stderr_of(&out));
    let tokens = intake_tokens(&out);
    assert!(tokens.contains(&"write-set=declared".to_owned()) && tokens.contains(&"files=2".to_owned()), "{tokens:?}");
    let id = run_id_of(&out);
    // 契約 (b) 以後、写しの write-set は**行の逐語**である（Declared は導出しない＝行がそのまま載る）。
    assert_eq!(copied_write_set(&state, &id), ["crates/toy/src/tint.rs", "crates/toy/src/show.rs"], "写しは行の write-set のまま");
    stop_run_ok(&state, &id);
    // 「pointer でない design」の周は**もう無い**（受付は pointer しか受けない・契約 (b)）。
    let missing = intake_raw(&repo, &state, &pointed_contract(&repo, "z.toml", "zz"), "s2-z");
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "区間に無い行 id は rc 1: {}", stderr_of(&missing));
    assert!(stderr_of(&missing).contains("行 id zz が区間に無い"), "{}", stderr_of(&missing));
    clean(&[&repo, &state]);
}

// ───── Declared 行の歯の置き場の門（設計 docs/design/contract-source.md §20・行 t・`s2-07l.391`・接頭辞 `contract_declared_teeth_`） ─────

/// (a) Declared 行の `verify` の歯（`derive_` = other.rs と e2e.rs）が `write-set`（tint.rs だけ）の外に在る契約は受付が
/// `teeth-outside-write-set`（rc 1）で**両方**を辞書順に名指して断り（helper の fn だけの helper.rs は出ない）、run dir は
/// 撃つ前と同数（便を作らない）。base は Declared を導出も門も無しで通す（rc 0 → RED）。
#[test]
fn contract_declared_teeth_outside_write_set_is_refused() {
    let row = declared_teeth_row("t", "derive_", "[\"crates/toy/src/tint.rs\"]");
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let before = run_dirs(&state);
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "t.toml", "t"), "s2-t");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "write-set の外の歯は rc 1: {err}");
    assert!(err.contains("verify の歯の file が write-set に無い"), "teeth-outside-write-set の理由: {err}");
    assert!(
        err.contains("crates/toy/src/other.rs ← filter 語 derive_, crates/toy/tests/e2e.rs ← filter 語 derive_"),
        "両方を辞書順に filter 語と対で名指す: {err}"
    );
    assert!(!err.contains("helper.rs"), "helper の fn だけの file は歯の file でない: {err}");
    assert!(!err.contains("write-set が導出値と一致しない"), "drift は撃たない: {err}");
    assert_eq!(run_dirs(&state), before, "便を作らない（run dir は撃つ前と同数）");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

// ───── 歯の置き場の門の断りの出所（設計 docs/design/contract-source.md §41・行 ap・`s2-07l.474`・接頭辞 `contract_teeth_origin_`） ─────

/// Declared 行の verify 2 行（`derive_ok` は e2e.rs だけ・`derive_in` は other.rs だけ）が write-set（tint.rs だけ）の外の
/// 歯を解く契約は、受付の stderr が file ごとに**それを解いた行**の filter 語を対で名乗り、rc 1 で run dir を作らない。
#[test]
fn contract_teeth_origin_intake_names_file_and_its_line_filter() {
    let verify = "[\"cargo nextest run -p toy --no-tests=fail derive_ok\", \"cargo nextest run -p toy --no-tests=fail derive_in\"]";
    let row = table_row("o", &[("write-set", "[\"crates/toy/src/tint.rs\"]"), ("verify", verify)]);
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let before = run_dirs(&state);
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "o.toml", "o"), "s2-o");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "rc 1: {err}");
    assert!(err.contains("verify の歯の file が write-set に無い"), "teeth-outside-write-set の理由: {err}");
    assert!(
        err.contains("crates/toy/src/other.rs ← filter 語 derive_in, crates/toy/tests/e2e.rs ← filter 語 derive_ok"),
        "file と filter 語を対で名乗る: {err}"
    );
    assert_eq!(run_dirs(&state), before, "run dir を作らない");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// (b) 同じ行の `write-set` に歯の 2 file を足した形（3 項目）は通る: rc 0・判定行 `write-set=declared` ∧ `files=3`・
/// 写しの write-set は契約 file のまま（Declared は差し替えない）。
#[test]
fn contract_declared_teeth_inside_write_set_passes() {
    let write_set = "[\"crates/toy/src/tint.rs\", \"crates/toy/src/other.rs\", \"crates/toy/tests/e2e.rs\"]";
    let row = declared_teeth_row("u", "derive_", write_set);
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "u.toml", "u"), "s2-u");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "歯が write-set の中なら通る: {}", stderr_of(&out));
    let tokens = intake_tokens(&out);
    assert!(tokens.contains(&"write-set=declared".to_owned()) && tokens.contains(&"files=3".to_owned()), "{tokens:?}");
    // 契約 (b) 以後、写しの write-set は行の逐語である。
    assert_eq!(
        copied_write_set(&state, &run_id_of(&out)),
        ["crates/toy/src/tint.rs", "crates/toy/src/other.rs", "crates/toy/tests/e2e.rs"],
        "写しは行の write-set のまま"
    );
    clean(&[&repo, &state]);
}

/// (c) base で 0 本の filter 語（`fresh_`）: Declared 行に `tests` 欄は無いので、`write-set` が歯の file を 1 つも持たなければ
/// 従来の `teeth-place-unresolved`（rc 1・字面不変）で断り、歯の file（e2e.rs）を足せばそれを置き場と読んで通る（rc 0）。
/// 母集団 = 2 回の intake の rc。
#[test]
fn contract_declared_teeth_new_filter_needs_a_teeth_file_in_write_set() {
    let rows = [
        declared_teeth_row("v", "fresh_", "[\"crates/toy/src/tint.rs\"]"),
        declared_teeth_row("w", "fresh_", "[\"crates/toy/src/tint.rs\", \"crates/toy/tests/e2e.rs\"]"),
    ];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    let bare = intake_raw(&repo, &state, &pointed_contract(&repo, "v.toml", "v"), "s2-v");
    let err = stderr_of(&bare);
    assert_eq!(bare.status.code(), Some(i32::from(RC_REFUSED)), "歯の file の無い write-set は rc 1: {err}");
    assert!(err.contains("filter 語 fresh_ を含む #[test] の fn が base に無く tests 欄も無い"), "teeth-place-unresolved の字面のまま: {err}");
    assert!(!state.join("pipe").exists(), "run dir を作らない");
    let placed = intake_raw(&repo, &state, &pointed_contract(&repo, "w.toml", "w"), "s2-w");
    assert_eq!(placed.status.code(), Some(i32::from(RC_OK)), "write-set の歯の file が置き場: {}", stderr_of(&placed));
    assert!(intake_tokens(&placed).contains(&"write-set=declared".to_owned()), "{}", stdout_of(&placed));
    clean(&[&repo, &state]);
}

// flip-check: s2-07l.481

/// §42（行 aq・接頭辞 `contract_declared_place_new_`）: base で 0 本の filter 語（`fresh_`）の行が `+` の新規の歯の file だけを
/// 持てば、受付はそれを path だけで置き場と読んで通り（rc 0・run dir を作る）、同じ行から `+` の項目を外すと従来の
/// `teeth-place-unresolved` の字面で断る（rc 1・run dir を作らない）。母集団 = 2 回の受付の rc。
#[test]
fn contract_declared_place_new_plus_teeth_file_passes_intake() {
    let rows = [
        declared_teeth_row("x", "fresh_", "[\"crates/toy/src/tint.rs\"]"),
        declared_teeth_row("y", "fresh_", "[\"crates/toy/src/tint.rs\", \"+crates/toy/tests/fresh.rs\"]"),
    ];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    let bare = intake_raw(&repo, &state, &pointed_contract(&repo, "x.toml", "x"), "s2-x");
    let err = stderr_of(&bare);
    assert_eq!(bare.status.code(), Some(i32::from(RC_REFUSED)), "+ を外した write-set は rc 1: {err}");
    assert!(err.contains("filter 語 fresh_ を含む #[test] の fn が base に無く tests 欄も無い"), "teeth-place-unresolved の字面のまま: {err}");
    assert!(!state.join("pipe").exists(), "run dir を作らない");
    let placed = intake_raw(&repo, &state, &pointed_contract(&repo, "y.toml", "y"), "s2-y");
    assert_eq!(placed.status.code(), Some(i32::from(RC_OK)), "+ の新規の歯の file が置き場: {}", stderr_of(&placed));
    assert!(intake_tokens(&placed).contains(&"write-set=declared".to_owned()), "{}", stdout_of(&placed));
    assert_eq!(run_dirs(&state), [run_id_of(&placed)], "run dir を 1 つ作る");
    clean(&[&repo, &state]);
}

// flip-check: s2-07l.550

/// §43 (2)（行 as・接頭辞 `contract_teeth_exact_`）: base に `derive_ok`（e2e.rs）と、それを substring に持つ `derive_ok_more`
/// （wide.rs）が在る。`-- --exact` の行（名の全体 `e2e::derive_ok` と段 1 つの `derive_ok`）を verify に持つ行は wide.rs を
/// write-set に持たないまま受付を通り（rc 0・run dir を作る）、同じ write-set で `--` の無い部分一致の行は wide.rs を
/// write-set の外の歯として断る（rc 1・run dir を作らない）。verify 行は括弧も引用符も持たないので宣言の形の門をそのまま
/// 通る。母集団 = 2 回の受付の rc。
#[test]
fn contract_teeth_exact_full_name_line_passes_intake_without_the_substring_file() {
    let write_set = "[\"crates/toy/src/tint.rs\", \"crates/toy/tests/e2e.rs\"]";
    let exact = "[\"cargo nextest run -p toy --no-tests=fail -- --exact e2e::derive_ok\", \"cargo nextest run -p toy --no-tests=fail -- --exact derive_ok\"]";
    let rows = [
        table_row("ea", &[("write-set", write_set), ("verify", exact)]),
        declared_teeth_row("eb", "derive_ok", write_set),
    ];
    let wide = [("crates/toy/tests/wide.rs", "#[test]\nfn derive_ok_more() {}\n")];
    let (repo, state) = derive_repo_with(&table_doc(&table_region(&rows)), &wide);
    let placed = intake_raw(&repo, &state, &pointed_contract(&repo, "ea.toml", "ea"), "s2-ea");
    assert_eq!(placed.status.code(), Some(i32::from(RC_OK)), "完全一致は e2e.rs だけ: {}", stderr_of(&placed));
    assert!(intake_tokens(&placed).contains(&"write-set=declared".to_owned()), "{}", stdout_of(&placed));
    assert_eq!(run_dirs(&state), [run_id_of(&placed)], "run dir を 1 つ作る");
    let loose = intake_raw(&repo, &state, &pointed_contract(&repo, "eb.toml", "eb"), "s2-eb");
    let err = stderr_of(&loose);
    assert_eq!(loose.status.code(), Some(i32::from(RC_REFUSED)), "部分一致は wide.rs も要る: {err}");
    assert!(err.contains("crates/toy/tests/wide.rs ← filter 語 derive_ok"), "wide.rs を名指す: {err}");
    assert_eq!(run_dirs(&state), [run_id_of(&placed)], "断った周は run dir を足さない");
    clean(&[&repo, &state]);
}

/// (e) `contracts schema` の出力は tracked の `contracts/schema.toml` と一致し、`creates` / `tests` / `also` を任意の list として
/// 持ち `write-set` は任意である。表の読み手も同じ: `write-set` の無い行を持つ doc は `contracts check` で違反 0（base は
/// 必須の欠落で断る → RED）。
#[test]
fn contract_schema_lists_creates_tests_also_and_write_set_is_optional() {
    let out = bin_cmd().args(["contracts", "schema"]).output().expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("contracts").join("schema.toml");
    let tracked = fs::read_to_string(&path).expect("tracked の生成物を読める");
    assert_eq!(stdout_of(&out), tracked, "render と tracked の差分 0");
    let mut fields: Vec<(String, String, String)> = Vec::new();
    for line in tracked.lines() {
        if line == "[[field]]" {
            fields.push((String::new(), String::new(), String::new()));
        }
        let Some(last) = fields.last_mut() else {
            continue;
        };
        if let Some(name) = line.strip_prefix("name = ") {
            last.0 = name.trim_matches('"').to_owned();
        } else if let Some(need) = line.strip_prefix("need = ") {
            last.1 = need.trim_matches('"').to_owned();
        } else if let Some(shape) = line.strip_prefix("shape = ") {
            last.2 = shape.trim_matches('"').to_owned();
        }
    }
    let field = |name: &str| fields.iter().find(|(found, _, _)| found == name).cloned().unwrap_or_default();
    for name in ["write-set", "creates", "tests", "also"] {
        assert_eq!(field(name), (name.to_owned(), "optional".to_owned(), "list".to_owned()), "{name} は任意の list");
    }
    // §33（行 ag）以後、verify は約束の行を持たない行でだけ必須（条件付き）。
    assert_eq!(field("verify").1, "conditional", "verify は条件付き");
    let doc = table_doc(&table_region(&[derive_row("a", &[("creates", "[\"src/new.rs\"]")])]));
    let repo = table_repo(&doc, &[]);
    let checked = contracts_check(&repo);
    assert_eq!(checked.status.code(), Some(i32::from(RC_OK)), "write-set の無い行は読める: {}", stdout_of(&checked));
    assert_eq!(stdout_of(&checked).trim_end(), "contracts check: docs=1 rows=1 untracked=0 findings=0 place-out=0/0");
    clean(&[&repo]);
}

/// (f) 歯の置き場は base の `#[test]` の直下の fn 名で解く: `derive_` は tests の歯（e2e.rs）と src の test 区間の歯（other.rs）
/// の 2 file で、helper の fn（`derive_helper`）を持つ file は数えない・`other_` は fn 名（`other_case`）で helper.rs に解け、
/// file 名（other.rs）では解かない。
#[test]
fn contract_derive_teeth_place_uses_base_test_names() {
    let rows = [
        derive_row("a", &[("verify", "[\"cargo nextest run -p toy --no-tests=fail derive_\"]")]),
        derive_row("b", &[("verify", "[\"cargo nextest run -p toy --no-tests=fail other_\"]")]),
    ];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "a.toml", "a"), "s2-a");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let id = run_id_of(&out);
    assert_eq!(copied_write_set(&state, &id), ["crates/toy/src/other.rs", "crates/toy/tests/e2e.rs"], "helper は数えない");
    assert!(intake_tokens(&out).contains(&"files=2".to_owned()), "{}", stdout_of(&out));
    stop_run_ok(&state, &id);
    let other = intake_raw(&repo, &state, &pointed_contract(&repo, "b.toml", "b"), "s2-b");
    assert_eq!(other.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&other));
    assert_eq!(copied_write_set(&state, &run_id_of(&other)), ["crates/toy/tests/helper.rs"], "fn 名で解く（file 名ではない）");
    clean(&[&repo, &state]);
}

// ───── fn 形の touches（設計 docs/design/contract-source.md §18・行 r・`s2-07l.358`・接頭辞 `contract_derive_fn_`） ─────

/// (a) `touches = ["crate::pipe::cli::resume"]`（toy の `src/pipe/cli.rs` が `fn resume(` を宣言）の行は intake を通り、
/// 導出値に宣言する file が入る。呼び手（`main.rs`）と別 module の同名の fn（`tone.rs`）は入らない（下界）。base は
/// 型の形でないと断る（RED）。
#[test]
fn contract_derive_fn_touches_names_the_declaring_file() {
    let (repo, state, out) = fn_form_intake("a", &["crate::pipe::cli::resume"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fn 形の行は導出値で通る: {}", stderr_of(&out));
    let tokens = intake_tokens(&out);
    assert!(tokens.contains(&"write-set=derived".to_owned()) && tokens.contains(&"files=1".to_owned()), "判定行: {tokens:?}");
    let found = copied_write_set(&state, &run_id_of(&out));
    assert_eq!(found, ["crates/toy/src/pipe/cli.rs"], "宣言する file だけ（呼び手の main.rs・別 module の tone.rs は入らない）");
    clean(&[&repo, &state]);
}

/// (b) `touches = ["crate::pipe::cli::missing"]`（どの file も `fn missing(` を宣言しない）は受付で断られ（rc 1）、
/// 断りの字面は新 variant のもの（`TypeForm` の「crate::module::Type の形でない」ではない）。run dir も
/// event も作らない＝導出値を空集合に潰さない。
#[test]
fn contract_derive_fn_refuses_when_no_file_declares_it() {
    let (repo, state, out) = fn_form_intake("b", &["crate::pipe::cli::missing"]);
    let err = stderr_of(&out);
    // 契約 (b) 以後、行の欠陥の rc は**表の検査の分類**が持つ（`unreadable` は rc 2）。断ることと理由が要点。
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "宣言する file が無い周は断る: {err}");
    assert!(err.contains("touches の pipe::cli::missing を宣言する file が base に無い"), "新 variant の字面で断る: {err}");
    assert!(!err.contains("crate::module::Type の形でない"), "TypeForm の字面ではない: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない（導出値を空集合にしない）");
    clean(&[&repo, &state]);
}

/// (c) 型を持つ同じ toy で、型形の行 `touches = ["crate::paint::Hue"]` の導出値は宣言 file（paint.rs）と arm の file
/// （arm.rs）の 2 つのまま（fn 形の追加で型形が動かない退行の pin）で、fn 形と型形を同じ行に並べた `touches` の導出値は
/// 両者の和集合。
#[test]
fn contract_derive_fn_keeps_type_closure_unchanged() {
    let (repo, state, out) = fn_form_intake("c", &["crate::paint::Hue"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "型形の行は通る: {}", stderr_of(&out));
    let typed = copied_write_set(&state, &run_id_of(&out));
    assert_eq!(typed, ["crates/toy/src/arm.rs", "crates/toy/src/paint.rs"], "型形の閉包は宣言 file と arm の file の 2 つのまま");
    clean(&[&repo, &state]);
    let (repo, state, out) = fn_form_intake("d", &["crate::paint::Hue", "crate::pipe::cli::resume"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "型形と fn 形を並べた行は通る: {}", stderr_of(&out));
    let both = copied_write_set(&state, &run_id_of(&out));
    assert_eq!(
        both,
        ["crates/toy/src/arm.rs", "crates/toy/src/paint.rs", "crates/toy/src/pipe/cli.rs"],
        "型形の閉包 ∪ fn 形の宣言 file"
    );
    clean(&[&repo, &state]);
}

// ───── creates の親 mod と subcommand の閉じた enum（設計 docs/design/contract-source.md §17・行 q・`s2-07l.337`・接頭辞 `contract_derive_creates_parent_` / `contract_derive_subcommand_enum_`） ─────

/// (vi-1) / (vi-2): 新設 file の dir の module を dir の中の `mod.rs` で宣言する周はその `mod.rs`、dir の隣の `<dir>.rs`
/// で宣言する周はその `<dir>.rs` が導出値に入る（base は `creates` の新規 file だけ → RED）。
#[test]
fn contract_derive_creates_parent_mod_rs_and_sibling_rs_are_added() {
    let files = [("crates/toy/src/pipe/mod.rs", "pub mod old;\n"), ("crates/toy/src/pipe/old.rs", "\n")];
    let found = creates_write_set("a", &["crates/toy/src/pipe/new.rs"], &files);
    assert_eq!(found, ["+crates/toy/src/pipe/new.rs", "crates/toy/src/pipe/mod.rs"], "(vi-1) dir の中の mod.rs");
    let files = [("crates/toy/src/seat.rs", "pub mod old;\n"), ("crates/toy/src/seat/old.rs", "\n")];
    let found = creates_write_set("b", &["crates/toy/src/seat/new.rs"], &files);
    assert_eq!(found, ["+crates/toy/src/seat/new.rs", "crates/toy/src/seat.rs"], "(vi-2) dir の隣の <dir>.rs");
}

/// (vi-3): `crates/<c>/src` の直下に新設する周は、同じ dir の `lib.rs` と `main.rs` のうち tracked な方が全部入る
/// （両方 tracked の crate は 2 面・`main.rs` だけの crate は 1 面・別の crate の根は入らない）。(vi-1) / (vi-2) だけの
/// 実装では `crates/<c>/src/mod.rs` も `crates/<c>/src.rs` も無いので導出値が新規 file だけになって落ちる。
#[test]
fn contract_derive_creates_parent_crate_root_takes_every_tracked_root() {
    let files = [
        ("crates/toy/src/lib.rs", "pub mod tint;\n"),
        ("crates/toy/src/main.rs", "fn main() {}\n"),
        ("crates/tool/src/main.rs", "fn main() {}\n"),
    ];
    let found = creates_write_set("a", &["crates/toy/src/fresh.rs"], &files);
    assert_eq!(found, ["+crates/toy/src/fresh.rs", "crates/toy/src/lib.rs", "crates/toy/src/main.rs"], "両方 tracked の crate は 2 面");
    let found = creates_write_set("b", &["crates/tool/src/fresh.rs"], &files);
    assert_eq!(found, ["+crates/tool/src/fresh.rs", "crates/tool/src/main.rs"], "main.rs だけの crate は 1 面");
}

/// 否定の枝: 親の候補がどれも base に無い周（dir ごと新設・同じ crate の根は入らない）・`.rs` でない項目・dir を持たない
/// 項目は 1 面も足さない（導出値は `creates` の新規 file だけ）。
#[test]
fn contract_derive_creates_parent_adds_nothing_without_a_tracked_candidate() {
    let files = [
        ("crates/toy/src/lib.rs", "pub mod pipe;\n"),
        ("crates/toy/src/pipe/mod.rs", "\n"),
        ("mod.rs", "\n"),
        ("lib.rs", "\n"),
    ];
    let found = creates_write_set("a", &["crates/toy/src/brand/new.rs"], &files);
    assert_eq!(found, ["+crates/toy/src/brand/new.rs"], "dir ごと新設する周は親を足さない");
    let found = creates_write_set("b", &["crates/toy/src/pipe/notes.md"], &files);
    assert_eq!(found, ["+crates/toy/src/pipe/notes.md"], ".rs でない項目は親を持たない");
    let found = creates_write_set("c", &["fresh.rs"], &files);
    assert_eq!(found, ["+fresh.rs"], "dir を持たない項目は親を持たない");
}

/// (vii): `touches` に cli の閉じた enum を宣言した行の導出値は cli.rs（宣言・const slice・match の arm）と自分の件数 pin
/// の歯の file だけで、もう一方の cli の歯の file は入らない（const slice の名を分けた効き目＝同名なら互いを拾って落ちる）。
/// base の字面には型も const slice も無いので、この導出値にならない（RED）。
#[test]
fn contract_derive_subcommand_enum_closure_is_its_cli_and_its_own_count_pin() {
    let seat = derived_write_set("a", ("touches", "[\"crate::seat::cli::SeatCommand\"]"), SUBCOMMAND_FILES);
    assert_eq!(seat, ["crates/scribe2/src/seat/cli.rs", "crates/scribe2/tests/e2e/seat.rs"], "seat の cli と seat の歯だけ");
    let pipe = derived_write_set("b", ("touches", "[\"crate::pipe::cli::PipeCommand\"]"), SUBCOMMAND_FILES);
    assert_eq!(pipe, ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/tests/e2e/pipe.rs"], "pipe の cli と pipe の歯だけ");
}

// ───── 閉包の同名衝突（設計 docs/design/contract-source.md §3「閉包の同名衝突」・行 j・`s2-07l.347`・接頭辞 `contract_closure_ext_same_name_`） ─────

/// (a) `crate::a::Marker` と `crate::b::Marker`（同名・同じ 3 形）を置いた toy で、`touches = ["crate::a::Marker"]` の行の
/// 導出値は `a` 側（宣言 file と、そこから取り込んで構築する file）だけを持つ（base は裸の型名で照合し `b` 側と
/// 同名の `rebrief` / `vessel` / `tick` も入る → RED）。
#[test]
fn contract_closure_ext_same_name_type_in_another_module_is_not_widened() {
    let found = same_name_write_set("a", "crate::a::Marker");
    assert_eq!(found, ["src/a.rs", "src/build.rs"], "a 側だけ（b・同名の他 module は入らない）");
}

/// (b) `use crate::a::Marker` で取り込んで `Marker {` を構築する file は導出値に入ったまま（退行の pin・`sees` の (b)）。
/// 対: `b` 側の行は `b` の宣言 file だけを持ち、`a` から取り込む `build.rs` を持たない。
#[test]
fn contract_closure_ext_same_name_import_still_widens() {
    let found = same_name_write_set("a", "crate::a::Marker");
    assert!(found.iter().any(|path| path == "src/build.rs"), "取り込んで構築する file は入る: {found:?}");
    let other = same_name_write_set("b", "crate::b::Marker");
    assert_eq!(other, ["src/b.rs"], "b 側は宣言 file だけ（a から取り込む build.rs は入らない）");
}

/// (d) 多段 module: `crate::seat::rebrief::Marker`（`src/seat/rebrief.rs`・enum）と同名の `crate::hook::vessel::Marker`
/// （`src/hook/vessel.rs`・struct・同じ 3 形）と `rebrief` から取り込む `src/seat/tick.rs` を置いた toy で、`touches =
/// ["crate::seat::rebrief::Marker"]` の行の導出値は `rebrief.rs` と `tick.rs` を持ち `vessel.rs` を持たない（goal の実物と
/// 同じ 2 段の形・module は最後の段で弁別する）。
#[test]
fn contract_closure_ext_same_name_in_nested_module_keeps_the_declaring_file() {
    let found = same_name_write_set("d", "crate::seat::rebrief::Marker");
    assert_eq!(found, ["src/seat/rebrief.rs", "src/seat/tick.rs"], "宣言 file と取り込む file だけ（同名の vessel.rs は入らない）");
}
