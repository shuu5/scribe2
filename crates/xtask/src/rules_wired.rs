//! `enabled = true` の rules 行に**読み手が在るか**を字面で測る（rules-wired・憲法 C10.3 / C14.2）。
//!
//! manifest の行は機械に効いて初めて規則である（C1・設計 rules-manifest.md §4）。全体監査
//! 2026-09-12 塊 1（`s2-07l.160`）: enabled な行のうち src から 1 度も読まれない行が在り、lint も
//! measure も当たらなかった。ここは [`crate::env_reads`] と同型の字面走査で、`crates/*/src` 配下の
//! `.rs` の**非 test 区間**（最初の行頭 `#[cfg(test)]` より前・`*_tests.rs` は丸ごと test）から
//! `const NAME: &str = "<id>"` の宣言を集め、その const 名が宣言以外の行で ≥ 1 回使われる id を
//! 「読み手あり」とする。宣言側の 4 file（[`DECLARING`]・manifest を読む / 生成する / 突合する側）と
//! 行頭 `//` の行は出所ではない。
//!
//! 却下案（契約）: 字面 grep だけ（doc コメント 1 行で充足＝空虚）・`RuleKind` に消費点の table
//! （table に書けば読まれたことになる＝同じ空虚）。const の値 + const の使用の 2 条件で測る。
//!
//! **検出線**（`Measured` の極性＝記録・違反にしない）: fact は
//! `rules-wired=<読み手の無い行>/<enabled 行> ids=<列>`（列は manifest 順の `,` 区切り・0 本は `none`）。
//! `cargo xtask check` の rc は変えない。deny 化（`0/<n>` を要求）は rules 行 + 裁定 id で後の便
//! （C5・C12.4）。`enabled = false` の行は母集団外（止めた事実は manifest に在る・C1）。

use crate::check::{failed, read_text, Layout, Measured, SourceFile, RULES_REL};
use crate::toml_lite::quoted;

/// 判定行の tag。
const TAG: &str = "rules-wired";

/// 宣言側の file（path の末尾で当てる）。ここに在る `"<id>"` は読み手ではなく manifest の側である。
const DECLARING: &[&str] = &["/rules/mod.rs", "/rules/manifest.rs", "/genmanifest.rs", "/rules_diff.rs"];

/// 名前で丸ごと test と見なす file の末尾（flip-check の写し方と同じ規約）。
const TEST_FILE_TAIL: &str = "_tests.rs";

/// 非 test 区間の終わり（行頭の印・`workspace::TEST_MOD_MARK` と同じ字面）。
const TEST_MOD_MARK: &str = "#[cfg(test)]";

/// 読み手の無い行が 0 本の周の `ids=` の値（空文字だと「無い」と「書いていない」が同じ字面）。
const NONE: &str = "none";

/// `const NAME: &str = "<値>"` の宣言 1 つ。
struct Decl {
    name: String,
    value: String,
    /// `files` の index と 0 始まりの行番号（使用を数えるとき宣言行自身を除く）。
    at: (usize, usize),
}

/// layout が指す tracked の manifest を読み、収集済みの file 列を測る。違反は立てない（検出線）。
pub(crate) fn measure(layout: &Layout, files: &[SourceFile]) -> Measured {
    let manifest = match read_text(&layout.root.join(RULES_REL)) {
        Ok(text) => text,
        Err(reason) => return failed(TAG, &format!("rules manifest を読めない: {reason}")),
    };
    let enabled: Vec<String> = match crate::rules_diff::rows(&manifest) {
        Ok(rows) => rows.into_iter().filter(|row| row.enabled == "true").map(|row| row.id).collect(),
        Err(reason) => return failed(TAG, &format!("rules manifest を行に分けられない: {reason}")),
    };
    let unwired = unwired(&enabled, files);
    let ids = if unwired.is_empty() { NONE.to_owned() } else { unwired.join(",") };
    Measured {
        fact: format!("{TAG}={}/{} ids={ids}", unwired.len(), enabled.len()),
        violations: Vec::new(),
    }
}

/// enabled な行 id のうち読み手の無いものを manifest 順で返す。
fn unwired(enabled: &[String], files: &[SourceFile]) -> Vec<String> {
    let lines = reader_lines(files);
    let wired: Vec<String> = declarations(&lines)
        .into_iter()
        .filter(|decl| is_used(decl, &lines))
        .map(|decl| decl.value)
        .collect();
    enabled.iter().filter(|id| !wired.contains(id)).cloned().collect()
}

/// 読み手になりうる行（宣言側の file・test file・test 区間・行頭コメントを除く）を `(file, line, 本文)` で回す。
fn reader_lines(files: &[SourceFile]) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let path = file.path.display().to_string();
        if path.ends_with(TEST_FILE_TAIL) || DECLARING.iter().any(|tail| path.ends_with(tail)) {
            continue;
        }
        for (at, line) in file.text.lines().enumerate() {
            if line.starts_with(TEST_MOD_MARK) {
                break;
            }
            if !line.trim_start().starts_with("//") {
                out.push((index, at, line));
            }
        }
    }
    out
}

/// `const NAME: &str = "<値>"` の宣言を集める（`pub` 系の前置きは任意）。
fn declarations(lines: &[(usize, usize, &str)]) -> Vec<Decl> {
    lines
        .iter()
        .filter_map(|(index, at, line)| {
            let (name, value) = parse_decl(line)?;
            Some(Decl { name, value, at: (*index, *at) })
        })
        .collect()
}

/// 1 行を `const NAME: &str = "<値>"` として読む（型は `&str` / `&'static str`）。
fn parse_decl(line: &str) -> Option<(String, String)> {
    let (head, rest) = line.trim_start().split_once("const ")?;
    if !(head.is_empty() || head.starts_with("pub")) {
        return None;
    }
    let (name, rest) = rest.split_once(':')?;
    let (kind, value) = rest.split_once('=')?;
    if !matches!(kind.trim(), "&str" | "&'static str") {
        return None;
    }
    let name = name.trim();
    if name.is_empty() || !name.chars().all(is_ident) {
        return None;
    }
    Some((name.to_owned(), quoted(value)?))
}

/// const 名が宣言行以外の行に識別子として現れるか。
fn is_used(decl: &Decl, lines: &[(usize, usize, &str)]) -> bool {
    lines
        .iter()
        .any(|(index, at, line)| (*index, *at) != decl.at && has_ident(line, &decl.name))
}

/// `line` に `name` が識別子の単位で現れるか（前後が識別子文字なら別名）。
fn has_ident(line: &str, name: &str) -> bool {
    let mut from = 0_usize;
    while let Some(offset) = line.get(from..).and_then(|rest| rest.find(name)) {
        let at = from.saturating_add(offset);
        let end = at.saturating_add(name.len());
        let before = line.get(..at).and_then(|head| head.chars().next_back()).is_some_and(is_ident);
        let after = line.get(end..).and_then(|tail| tail.chars().next()).is_some_and(is_ident);
        if !before && !after {
            return true;
        }
        from = end;
    }
    false
}

/// Rust の識別子を成す文字。
fn is_ident(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    // 歯は module と同居させる（案 A′・`s2-07l.101`）: 新 module の別 file は base に宣言元が無く
    // flip-check が測れない。歯は **measure の入口**（layout が指す manifest の path）から通す——絞った
    // 列を内部関数へ渡す形は enabled の filter を通らない（filter を外す変異が全歯を通る）。
    use super::measure;
    use crate::check::tests::{make_tmp_dir, write_at};
    use crate::check::{Layout, Measured, SourceFile, RULES_REL};
    use crate::workspace::collect_rs_files;
    use std::path::PathBuf;

    /// enabled 3 本（alpha / beta / gamma）と disabled 1 本（delta）の manifest。id は実在しない字面。
    const MANIFEST: &str = "schema = 1\n\n[[rule]]\nid = \"probe.alpha\"\nvalue = 1\nenabled = true\nruling = \"r\"\n\n\
        [[rule]]\nid = \"probe.beta\"\nvalue = 2\nenabled = true\nruling = \"r\"\n\n\
        [[rule]]\nid = \"probe.gamma\"\nvalue = 3\nenabled = true\nruling = \"r\"\n\n\
        [[rule]]\nid = \"probe.delta\"\nvalue = 4\nenabled = false\nruling = \"r\"\n";

    /// tmp の root に `manifest` を `rules/manifest.toml` として置き、`crates/demo/src/<rel>` の file 列を
    /// 測る（measure は収集済みの file だけを見るので src は disk に書かない）。
    fn measured(manifest: &str, files: &[(&str, &str)]) -> Measured {
        let root = make_tmp_dir();
        write_at(&root, RULES_REL, manifest);
        let core_dir = root.join("crates").join("demo");
        let sources: Vec<SourceFile> = files
            .iter()
            .map(|(rel, text)| SourceFile { path: core_dir.join("src").join(rel), text: (*text).to_owned() })
            .collect();
        let layout = Layout { root: root.clone(), core_dir, member_dirs: Vec::new(), name: "demo".to_owned() };
        let got = measure(&layout, &sources);
        std::fs::remove_dir_all(&root).expect("tmp dir を消せる");
        got
    }

    /// (a) const + 使用 = 読み手あり・const だけ = 無し・doc コメントの字面だけ = 無し・disabled は母集団外。
    #[test]
    fn rules_wired_names_enabled_rows_without_a_reader() {
        let got = measured(
            MANIFEST,
            &[
                ("a.rs", "const ROW_ALPHA: &str = \"probe.alpha\";\nfn f(m: &str) -> bool {\n    m.contains(ROW_ALPHA)\n}\n"),
                ("b.rs", "pub const ROW_BETA: &str = \"probe.beta\";\n/// 行 \"probe.gamma\" を読む（doc の字面だけ）\nfn g() {}\n"),
                // disabled の行は読み手が無くても列に並ばない（filter を外すと `3/4 … probe.delta` に化ける）。
                ("c.rs", "// const ROW_DELTA: &str = \"probe.delta\";\n"),
            ],
        );
        assert_eq!(got.fact, "rules-wired=2/3 ids=probe.beta,probe.gamma", "読み手の無い enabled 行だけを manifest 順で");
        // disabled の行に読み手を足しても母集団は 3 のまま（enabled の filter を反転すると 0/1 に化ける）。
        let delta = measured(MANIFEST, &[("d.rs", "const ROW_DELTA: &str = \"probe.delta\";\nfn d() -> &'static str {\n    ROW_DELTA\n}\n")]);
        assert_eq!(delta.fact, "rules-wired=3/3 ids=probe.alpha,probe.beta,probe.gamma");
    }

    /// 読み手に数えない 5 形: 宣言側の file・test 区間の使用・`*_tests.rs`・識別子の続き・`format!` の合成。
    #[test]
    fn rules_wired_does_not_count_declaring_files_tests_or_partial_idents() {
        let all = "rules-wired=3/3 ids=probe.alpha,probe.beta,probe.gamma";
        let use_alpha = "const ROW_ALPHA: &str = \"probe.alpha\";\nfn f() -> &'static str {\n    ROW_ALPHA\n}\n";
        assert_eq!(measured(MANIFEST, &[("rules/mod.rs", use_alpha)]).fact, all, "宣言側の file は読み手でない");
        assert_eq!(measured(MANIFEST, &[("seat_tests.rs", use_alpha)]).fact, all, "test file は読み手でない");
        let in_tests = "const ROW_ALPHA: &str = \"probe.alpha\";\n#[cfg(test)]\nmod t {\n    fn f() -> &'static str { super::ROW_ALPHA }\n}\n";
        assert_eq!(measured(MANIFEST, &[("a.rs", in_tests)]).fact, all, "test 区間の使用は読み手でない");
        let partial = "const ROW_ALPHA: &str = \"probe.alpha\";\nconst ROW_ALPHA_X: u64 = 1;\nfn f() -> u64 { ROW_ALPHA_X }\n";
        assert_eq!(measured(MANIFEST, &[("a.rs", partial)]).fact, all, "識別子の続きは別名");
        let composed = "fn id(k: &str) -> String {\n    format!(\"probe.{k}\")\n}\n";
        assert_eq!(measured(MANIFEST, &[("a.rs", composed)]).fact, all, "合成した字面は const でない");
        // `pub(crate)` の前置き・`&'static str`・別 file からの使用は読み手。
        let across = [
            ("a.rs", "pub(crate) const ROW_ALPHA: &'static str = \"probe.alpha\";\n"),
            ("b.rs", "fn f() -> &'static str {\n    crate::a::ROW_ALPHA\n}\n"),
        ];
        assert_eq!(measured(MANIFEST, &across).fact, "rules-wired=2/3 ids=probe.beta,probe.gamma");
    }

    /// 字下げした `#[cfg(test)]`（行頭でない・fn の中の属性）は test 区間の印に数えない（module doc の
    /// 「行頭の印」を pin・`s2-07l.160` の A/B で `starts_with` → `contains` が生存した）。印の後の使用は
    /// 読み手のまま。行頭の印はその後を切る。
    // flip-check: retroactive s2-07l.350
    #[test]
    fn rules_wired_indented_cfg_test_is_not_a_test_mod_mark() {
        let indented = "const ROW_ALPHA: &str = \"probe.alpha\";\nfn f() {\n    #[cfg(test)]\n    fn inner() {}\n}\n\
                        fn g() -> &'static str {\n    ROW_ALPHA\n}\n";
        assert_eq!(
            measured(MANIFEST, &[("a.rs", indented)]).fact,
            "rules-wired=2/3 ids=probe.beta,probe.gamma",
            "字下げした印は区間を切らない（contains だと使用が test 区間に落ちて 3/3 に化ける）"
        );
        let at_head = "const ROW_ALPHA: &str = \"probe.alpha\";\nfn f() {\n    #[cfg(test)]\n    fn inner() {}\n}\n\
                       #[cfg(test)]\nfn g() -> &'static str {\n    ROW_ALPHA\n}\n";
        assert_eq!(
            measured(MANIFEST, &[("a.rs", at_head)]).fact,
            "rules-wired=3/3 ids=probe.alpha,probe.beta,probe.gamma",
            "行頭の印はその後を切る"
        );
    }

    /// (b) 検出線: 読み手の無い行が在っても違反は 0（rc に効かない）。全行に読み手が在れば `ids=none`。
    /// 読めない / 行に分けられない manifest だけが違反（数に化けない）。
    #[test]
    fn rules_wired_is_a_detection_line() {
        let got = measured(MANIFEST, &[("a.rs", "fn f() {}\n")]);
        assert_eq!(got.fact, "rules-wired=3/3 ids=probe.alpha,probe.beta,probe.gamma");
        assert!(got.violations.is_empty(), "検出線は違反を立てない: {:?}", got.violations);
        let reads = "pub const A: &str = \"probe.alpha\";\npub const B: &str = \"probe.beta\";\n\
                     pub const G: &str = \"probe.gamma\";\nfn f() -> [&'static str; 3] {\n    [A, B, G]\n}\n";
        assert_eq!(measured(MANIFEST, &[("a.rs", reads)]).fact, "rules-wired=0/3 ids=none", "0 本は none");
        let broken = measured("[[rule]]\nid = \"probe.alpha\"\n", &[]);
        assert_eq!(broken.fact, "rules-wired=?", "行に分けられない manifest は ?");
        assert_eq!(broken.violations.len(), 1, "測れない周だけ違反: {:?}", broken.violations);
    }

    /// (c) 現物の repo: fact が数で出て、母集団は enabled 行の数（40 以上）・違反 0・列は本数と対。
    #[test]
    fn rules_wired_measures_the_workspace_with_a_nonempty_population() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let layout = Layout::discover(&root).expect("自 workspace は読める");
        let files = collect_rs_files(&layout.root).expect("crates/*/src を読める");
        let got = measure(&layout, &files);
        assert!(got.violations.is_empty(), "検出線は違反を立てない: {:?}", got.violations);
        let (counts, ids) = got
            .fact
            .strip_prefix("rules-wired=")
            .and_then(|tail| tail.split_once(" ids="))
            .unwrap_or_else(|| panic!("fact は rules-wired=<n>/<m> ids=<列> の形のはず: {}", got.fact));
        let (unwired, population) = counts
            .split_once('/')
            .and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)))
            .unwrap_or_else(|| panic!("<n>/<m> の形のはず: {}", got.fact));
        assert!(population >= 40, "母集団は enabled 行の数のはず: {}", got.fact);
        let listed = if ids == "none" { 0 } else { ids.split(',').count() };
        assert_eq!(listed, unwired, "ids= の列は本数と対: {}", got.fact);
    }
}
