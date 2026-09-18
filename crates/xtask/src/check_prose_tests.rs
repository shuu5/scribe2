//! `check` の歯（散文の門・`CLAUDE.md` の区間）: `prose_gate_*` と `claude_md_*` を測る。
//! 共通の helper（`check_fixture` / `write_at` / `write_healthy` 等）は親 module（`check_tests.rs`）に在り、
//! 専用の fixture（`done_claude_md` / `prose_claude_md`）はこの file が持つ。
//! 純粋な移動（`check_tests.rs` から歯を足さずに写した・s2-07l.370）。札は file ごとに要る。

// flip-check: moved s2-07l.370

use super::*;

/// 判定行に散文の門の fact が載る（設計 contract-source.md §12・`s2-07l.202`）。
#[test]
fn prose_gate_fact_is_in_summary() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let line = summary(&root);
    assert!(line.contains(" prose-gate="), "判定行に prose-gate の fact が在るはず: {line}");
}

/// fact は `prose-gate=<違反数>/<母集団>` の形で、現物の `docs/design` は違反 0（母集団は空でない）。
///
/// 分岐は `.git` の有無（flip-check の展開木では git を要する他の fact と同じく測れない形）。
#[test]
fn prose_gate_fact_counts_zero_violations_on_workspace() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let line = summary(&root);
    let value = line
        .split(' ')
        .find_map(|token| token.strip_prefix("prose-gate="))
        .unwrap_or_default();
    if root.join(".git").exists() {
        let counts: Vec<usize> = value.split('/').filter_map(|part| part.parse().ok()).collect();
        assert_eq!(counts.len(), 2, "<n>/<m> の形のはず: {line}");
        assert_eq!(counts.first(), Some(&0), "現物の設計 doc は違反 0 のはず: {line}");
        assert!(counts.get(1).is_some_and(|marked| *marked >= 1), "母集団は空でないはず: {line}");
    } else {
        assert!(value.starts_with("n/a(") || value == "?", ".git の無い木では測れない形のはず: {line}");
    }
}

/// 印を持つ文が pointer を失った設計 doc は prose-gate だけで落ち、file:line と理由を名指す。
#[test]
fn prose_gate_names_violating_design_doc_in_check() {
    let violations = check_fixture(|dir| {
        write_at(dir, "docs/design/probe-8w.md", "# 設計\n席は lock を確保しなければならない。\n");
    });
    assert_single(&violations, "prose-gate");
    let head = violations.first().map(String::as_str).unwrap_or_default();
    assert!(head.contains("docs/design/probe-8w.md:2: no-pointer"), "{head}");
}

/// tracked な設計 doc が 0 本の木は違反（0 本の緑にしない）・健全な木の fact は `0/<母集団>`。
#[test]
fn prose_gate_fails_closed_without_design_docs() {
    let missing = check_fixture(|dir| {
        let _ = fs::remove_file(dir.join(PROSE_DOC_REL));
    });
    assert_single(&missing, "prose-gate");
    let dir = make_tmp_dir();
    write_healthy(&dir);
    git_track_all(&dir);
    let line = summary(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert!(line.contains(" prose-gate=0/1"), "健全な木は違反 0 / 母集団 1: {line}");
}

/// done 区間の fixture の ci.yml の相対 path。
const CI_FIXTURE_REL: &str = ".github/workflows/ci.yml";

/// done 区間の fixture の ci.yml（`run:` 3 行・穴 1 つ・`if:` 付きの job と `uses:` の行を挟む）。
const DONE_CI: &str = concat!(
    "jobs:\n",
    "  alpha:\n    steps:\n      - uses: actions/checkout@v0\n      - run: cargo frobnicate --all\n",
    "  beta:\n    if: github.event_name == 'pull_request'\n    steps:\n",
    "      - run: cargo quuxify --base \"${{ github.event.pull_request.base.sha }}\"\n",
    "  gamma:\n    steps:\n      - name: z\n        run: cargo zorkify check\n",
);

/// [`DONE_CI`] から出るべき done 区間の本文（job の宣言順・穴は引用符ごと `<base>`）。
const DONE_EXPECTED: &str = "```\ncargo frobnicate --all\ncargo quuxify --base <base>\ncargo zorkify check\n```";

/// done 区間に `body` を持つ `CLAUDE.md`。
fn done_claude_md(body: &str) -> String {
    format!("# fixture\n<!-- done:begin -->\n{body}\n<!-- done:end -->\n")
}

/// ci.yml の `run: cargo …` 行が job の宣言順で done 区間の本文になる。
#[test]
fn claude_md_done_renders_ci_run_lines_in_job_order() {
    use crate::claude_md::done_body;
    assert_eq!(done_body(DONE_CI), Ok((DONE_EXPECTED.to_owned(), 3)));
    // cargo でない run 行は写さない・単引用符の穴も引用符ごと・引用符の無い穴は穴だけ。
    let mixed = "- run: echo skipped-omega\n- run: cargo a --b '${{ x }}'\n- run: cargo c ${{ y }}/z\n";
    assert_eq!(done_body(mixed), Ok(("```\ncargo a --b <base>\ncargo c <base>/z\n```".to_owned(), 2)));
    assert!(done_body("- run: cargo a ${{ x\n").is_err(), "閉じない穴は断る");
    assert!(done_body("- run: echo only\n").is_err(), "cargo の行が 0 本の ci.yml は断る");
}

/// 生成と一致する done 区間は通り、判定行に行数が出る。
#[test]
fn claude_md_done_accepts_generated_region_in_check() {
    let dir = make_tmp_dir();
    write_healthy(&dir);
    write_at(&dir, CI_FIXTURE_REL, DONE_CI);
    write_at(&dir, "CLAUDE.md", &done_claude_md(DONE_EXPECTED));
    git_track_all(&dir);
    let report = inspect(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert!(report.violations.is_empty(), "違反: {:?}", report.violations);
    assert!(report.summary.contains(" claude-md-done=3 "), "{}", report.summary);
}

/// tracked と 1 行違う done 区間は claude-md-done だけで落ち、違う行を両側の字面で名指す。
#[test]
fn claude_md_done_names_drifted_line_in_check() {
    let drifted = DONE_EXPECTED.replace("cargo zorkify check", "cargo zorkify check --stale-psi");
    let violations = check_fixture(|dir| {
        write_at(dir, CI_FIXTURE_REL, DONE_CI);
        write_at(dir, "CLAUDE.md", &done_claude_md(&drifted));
    });
    assert_single(&violations, "claude-md-done");
    let head = violations.first().map(String::as_str).unwrap_or_default();
    assert!(head.contains("区間の 5 行目"), "違う行の番号: {head}");
    assert!(head.contains("tracked=`cargo zorkify check --stale-psi`"), "tracked 側: {head}");
    assert!(head.contains("生成=`cargo zorkify check`"), "生成側: {head}");
}

/// done 区間の印が無い / 2 つ在る `CLAUDE.md` は型で断り、check は claude-md-done だけで落ちる。
#[test]
fn claude_md_done_refuses_missing_and_duplicated_markers() {
    use crate::claude_md::{region_between, RegionError, DONE};
    let twice = format!("{}{}", done_claude_md("a"), done_claude_md("b"));
    assert_eq!(
        region_between("# fixture\n", DONE),
        Err(RegionError::Missing("CLAUDE.md", "<!-- done:begin -->")),
        "断る理由は印の在る file を名乗る"
    );
    assert_eq!(region_between(&twice, DONE), Err(RegionError::Duplicated("CLAUDE.md", "<!-- done:begin -->", 2)));
    for text in ["# fixture\n".to_owned(), twice] {
        let violations = check_fixture(|dir| {
            write_at(dir, CI_FIXTURE_REL, DONE_CI);
            write_at(dir, "CLAUDE.md", &text);
        });
        assert_single(&violations, "claude-md-done");
    }
}

/// ci.yml の無い木に done 区間だけが残ると deny・どちらも無い木は `n/a`。
#[test]
fn claude_md_done_denies_orphan_region_without_ci() {
    let violations = check_fixture(|dir| write_at(dir, "CLAUDE.md", &done_claude_md(DONE_EXPECTED)));
    assert_single(&violations, "claude-md-done");
    let dir = make_tmp_dir();
    write_healthy(&dir);
    git_track_all(&dir);
    let line = summary(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert!(line.contains(" claude-md-done=n/a(no-ci) "), "{line}");
    assert!(line.contains(" claude-md-prose=n/a(no-claude-md) "), "{line}");
}

/// 3 面の現物から「`cargo nextest run` で始まる行」を字面で 1 本ずつ取る（`claude_md.rs` の
/// 読み手は使わない＝独立の pin）。`prefix` は行頭の飾り（yaml の `- run: ` 等）。
fn nextest_line_of(text: &str, prefix: &str) -> Option<String> {
    let mut found = text
        .lines()
        .map(str::trim_start)
        .filter_map(|line| line.strip_prefix(prefix))
        .filter(|line| line.starts_with("cargo nextest run "));
    let line = found.next()?;
    assert!(found.next().is_none(), "nextest の行は 1 面に 1 本のはず");
    Some(line.to_owned())
}

/// `.vessel.toml` の `common-verify` の nextest 行・`ci.yml` の `run:` の nextest 行・`CLAUDE.md`
/// の done 区間の nextest 行が **3 面同文**で `--no-fail-fast` を持つ（設計 gate-cost.md §19・
/// 憲法 C10）。`xtask check` の `claude-md-done` は ci.yml ↔ CLAUDE.md の一致しか見ないので、
/// `.vessel.toml` を含む 3 面と flag の在ることは、ここが現物を字面で読んで pin する。
#[test]
fn no_fail_fast_done_line_is_the_same_on_three_faces() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let read = |rel: &str| fs::read_to_string(root.join(rel)).unwrap_or_else(|err| panic!("{rel} を読める: {err}"));
    // `.vessel.toml`: `common-verify = [ "…", "cargo nextest run …", … ]` の要素を引用符で切る。
    let vessel = read(".vessel.toml");
    let common = vessel
        .lines()
        .find_map(|line| line.strip_prefix("common-verify = ["))
        .expect(".vessel.toml に common-verify が在る");
    let vessel_line = common
        .split('"')
        .find(|item| item.starts_with("cargo nextest run "))
        .expect("common-verify に nextest の行が在る")
        .to_owned();
    let ci_line = nextest_line_of(&read(".github/workflows/ci.yml"), "- run: ").expect("ci.yml に run: の nextest 行が在る");
    let claude_md = read("CLAUDE.md");
    let (_, after_begin) = claude_md.split_once("<!-- done:begin -->").expect("CLAUDE.md に done:begin が在る");
    let (region, _) = after_begin.split_once("<!-- done:end -->").expect("CLAUDE.md に done:end が在る");
    let claude_line = nextest_line_of(region, "").expect("done 区間に nextest の行が在る");
    assert_eq!(vessel_line, ci_line, ".vessel.toml と ci.yml は同文");
    assert_eq!(ci_line, claude_line, "ci.yml と CLAUDE.md は同文");
    let tokens: Vec<&str> = ci_line.split(' ').collect();
    assert_eq!(tokens.iter().filter(|t| **t == "--no-fail-fast").count(), 1, "--no-fail-fast を 1 つ持つ: {ci_line}");
    assert!(tokens.contains(&"--no-tests=fail"), "--no-tests=fail は落とさない: {ci_line}");
    assert!(tokens.contains(&"--workspace"), "--workspace は落とさない: {ci_line}");
}

/// 区間外の 5 行（印 2 行・pointer 付きの印 1 行・印なし 2 行）。
const PROSE_OUTSIDE: [&str; 5] = [
    "器は席を確保しなければならない。",
    "器は台帳を削除してはならない。",
    "器は失敗を記録しなければならない（C1）。",
    "器は判定行を読む。",
    "lens が差分を見る。",
];

/// 区間外の 5 行の間に、印を持つ行を中に持つ 2 つの区間と空行を挟んだ `CLAUDE.md`。
fn prose_claude_md() -> String {
    let [first, second, third, fourth, fifth] = PROSE_OUTSIDE;
    [
        first,
        "<!-- constitution:begin -->",
        "X1: scribe2 SHALL hold INSIDE-ALPHA.",
        "<!-- constitution:end -->",
        second,
        third,
        "<!-- done:begin -->",
        "```",
        "器は区間の中でも確保しなければならない。",
        "```",
        "<!-- done:end -->",
        fourth,
        "",
        fifth,
    ]
    .join("\n")
}

/// 区間外の規範行は `<pointer の無い印の行>/<区間外の非空行>` = 2/5（区間の中と印の行は数えない）。
#[test]
fn claude_md_prose_counts_unpointered_marked_lines_outside_regions() {
    use crate::claude_md::{prose_count, RegionError};
    assert_eq!(prose_count(&prose_claude_md()), Ok((2, 5)));
    assert_eq!(prose_count(&PROSE_OUTSIDE.join("\n")), Ok((2, 5)), "区間の無い本文も同じ数");
    let broken = format!("{}\n<!-- done:begin -->\n", prose_claude_md());
    assert_eq!(
        prose_count(&broken),
        Err(RegionError::Duplicated("CLAUDE.md", "<!-- done:begin -->", 2)),
        "壊れた印は型で断る"
    );
}

/// `claude-md-prose` は検出線: 判定行に値が出て、違反行は立たない（rc を変えない）。
#[test]
fn claude_md_prose_is_a_detection_line_in_check() {
    let dir = make_tmp_dir();
    write_healthy(&dir);
    write_at(&dir, "CLAUDE.md", &PROSE_OUTSIDE.join("\n"));
    git_track_all(&dir);
    let report = inspect(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert!(report.violations.is_empty(), "検出線は rc を変えない: {:?}", report.violations);
    assert!(report.summary.contains(" claude-md-prose=2/5 "), "{}", report.summary);
}

/// 憲法の生成区間は `CLAUDE.md` でなく生成 file `docs/constitution.md` に在り、`CLAUDE.md` に
/// 残るのはその file を名指す pointer 1 行だけである（ADR-0046・`claude_md.rs` の読み手は
/// 使わない＝現物を字面で読む独立の pin）。
///
/// 測るのは 3 つ: 区間の印が `CLAUDE.md` から消え生成 file に 1 本ずつ在ること・規範文の字面
/// （`scribe2 SHALL`）が `CLAUDE.md` に 1 つも無く生成 file に在ること・done の区間は
/// `CLAUDE.md` に残ること（移したのは憲法の区間だけである）。
#[test]
fn constitution_region_lives_in_the_generated_file_not_claude_md() {
    const GENERATED_REL: &str = "docs/constitution.md";
    const NORMATIVE: &str = "scribe2 SHALL";
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let read = |rel: &str| fs::read_to_string(root.join(rel)).unwrap_or_else(|err| panic!("{rel} を読める: {err}"));
    let claude_md = read("CLAUDE.md");
    let generated = read(GENERATED_REL);
    for marker in ["<!-- constitution:begin -->", "<!-- constitution:end -->"] {
        assert_eq!(claude_md.matches(marker).count(), 0, "CLAUDE.md に生成区間の印が残った: {marker}");
        assert_eq!(generated.matches(marker).count(), 1, "{GENERATED_REL} の印は 1 本: {marker}");
    }
    assert_eq!(claude_md.matches(NORMATIVE).count(), 0, "CLAUDE.md に規範文が残った");
    let (_, after_begin) = generated
        .split_once("<!-- constitution:begin -->")
        .expect("生成 file に constitution:begin が在る");
    let (region, _) = after_begin
        .split_once("<!-- constitution:end -->")
        .expect("生成 file に constitution:end が在る");
    assert!(region.contains(NORMATIVE), "生成 file の区間に規範文が無い");
    let pointers = claude_md.lines().filter(|line| line.contains(GENERATED_REL)).count();
    assert_eq!(pointers, 1, "CLAUDE.md が生成 file を名指す行は 1 本");
    assert!(claude_md.contains("<!-- done:begin -->"), "done の区間は CLAUDE.md に残る");
}
