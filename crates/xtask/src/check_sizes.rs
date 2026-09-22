//! `cargo xtask check` の**大きさ系 measure**（core-lines / core-spawn / file-lines / test-src-ratio /
//! name-literal）。母集団は `collect_rs_files` が読んだ `.rs` の列である。
//!
//! `check.rs` から分けたのは憲法 C4（1 file の上限）のためで、分けた周は**測る内容は 1 つも変えていない**
//! （`s2-07l.84`・純粋な移動）。判定行の名前・順序・値の書式は不変である。閾値は const でなく
//! manifest から読んだ [`Limits`] で受ける（`s2-07l.163`・憲法 C1）。
//!
//! core-lines の母集団は core crate の `src` の**本体**（各 file の最初の行頭 `#[cfg(test)]` より前＝
//! [`SourceFile::split_test_src`] の src 側・in-file の歯は R-C4-3 が数える側で二重計上しない・user 裁定
//! 2026-09-15・ADR-0033・設計 core-boundary.md §2）。core-spawn は core の `src` で `Command::new` を含む行の
//! **検出線**（数だけ出す・値を持たない・deny へ倒すのは境界 crate への移動が終わる便・同 §5）。

use crate::check::{Layout, Measured, SourceFile};
use crate::limits::Limits;
use std::path::PathBuf;

/// 子 process を起こす字面（core-spawn の母集団・設計 core-boundary.md §4「規則は 1 つ」）。
const SPAWN_NEEDLE: &str = "Command::new";

/// 行数（usize）を manifest の値（u64）と同じ型へ（64 bit では損失なし）。
fn as_u64(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

/// 行の数え方の幅（manifest の `R-C4.line-width`）を `SourceFile` の数え方が受ける型へ。
fn width_of(limits: &Limits) -> usize {
    usize::try_from(limits.line_width).unwrap_or(usize::MAX)
}

/// core crate の `src` 配下の**本体**の総行数（core-lines・幅で正規化・in-file の歯〔行頭 `#[cfg(test)]` から
/// file 末尾〕と名で test の file〔丸ごと〕は数えない＝[`SourceFile::split_test_src`] の src 側の合計・設計
/// core-boundary.md §2・rules-manifest.md §16）。
///
/// 受付の core の余地（core の `pipe::declaration` の `FileLines`）も同じ切り方で数える（crate は互いに依存
/// しないので式は 2 か所・同じ fixture の歯が一致を守る）。
pub(crate) fn measure_core_lines(layout: &Layout, files: &[SourceFile], limits: &Limits) -> Measured {
    let core_src = layout.core_dir.join("src");
    let width = width_of(limits);
    let total: usize = files
        .iter()
        .filter(|file| file.path.starts_with(&core_src))
        .map(|file| file.split_test_src(width).1)
        .sum();
    let max = limits.core_lines;
    let mut violations = Vec::new();
    if as_u64(total) > max {
        violations.push(format!(
            "core-lines: core crate の src の本体が {total} 行で上限 {max} 行を超える"
        ));
    }
    Measured {
        fact: format!("core-lines={total}/{max}"),
        violations,
    }
}

/// core crate の `src` 配下で `Command::new` を含む行の件数と、それを持つ file の数（core-spawn・検出線・
/// 設計 core-boundary.md §5）。fact は `core-spawn=<件数>/<file 数>` で、**違反は立てない**（値を持たない＝
/// `R-C12-1` と同型の検出線・純移動の便が件数を N → N−k へ下げる形を pin する材料・deny へ倒すのは最後の
/// 移動の便）。母集団は file 全体（in-file の歯の中の起動も core に在る限り数える＝module と一緒に動く）。
pub(crate) fn measure_core_spawn(layout: &Layout, files: &[SourceFile]) -> Measured {
    let core_src = layout.core_dir.join("src");
    let (mut sites, mut holders) = (0_usize, 0_usize);
    for file in files.iter().filter(|file| file.path.starts_with(&core_src)) {
        let found = file.text.lines().filter(|line| line.contains(SPAWN_NEEDLE)).count();
        if found > 0 {
            holders = holders.saturating_add(1);
            sites = sites.saturating_add(found);
        }
    }
    Measured {
        fact: format!("core-spawn={sites}/{holders}"),
        violations: Vec::new(),
    }
}

/// `crates/*/src` 配下 `.rs` の 1 file 行数（file-lines・幅で正規化）。
pub(crate) fn measure_file_lines(files: &[SourceFile], limits: &Limits) -> Measured {
    let max = limits.file_lines;
    let width = width_of(limits);
    let mut violations = Vec::new();
    let mut worst = 0;
    for file in files {
        let lines = file.lines(width);
        worst = worst.max(lines);
        if as_u64(lines) > max {
            violations.push(format!(
                "file-lines: {} が {lines} 行で上限 {max} 行を超える",
                file.path.display()
            ));
        }
    }
    Measured {
        fact: format!("file-lines={worst}/{max}"),
        violations,
    }
}

/// test 行と src 行の比（test-src-ratio・幅で正規化）。整数比較で `Σtest × 100 <= Σsrc × pct` を見る。
pub(crate) fn measure_test_src_ratio(files: &[SourceFile], limits: &Limits) -> Measured {
    let width = width_of(limits);
    let mut test_total = 0;
    let mut src_total = 0;
    for file in files {
        let (test, src) = file.split_test_src(width);
        test_total += test;
        src_total += src;
    }
    Measured {
        fact: format!("test-src-ratio={test_total}/{src_total}"),
        violations: ratio_violations(test_total, src_total, limits.test_src_ratio_pct),
    }
}

/// 比の判定（整数演算のみ）。src 0 行は測れないので違反にしない（従来と同じ）。
fn ratio_violations(test_total: usize, src_total: usize, pct: u64) -> Vec<String> {
    let over = src_total > 0 && as_u64(test_total).saturating_mul(100) > as_u64(src_total).saturating_mul(pct);
    if !over {
        return Vec::new();
    }
    vec![format!(
        "test-src-ratio: test {test_total} 行 > src {src_total} 行（比の上限は {pct}%）"
    )]
}

/// NAME の字面を持つ `.rs` が core crate の `name.rs` ただ 1 本であること（name-literal）。
pub(crate) fn measure_name_literal(layout: &Layout, files: &[SourceFile]) -> Measured {
    let needle = format!("\"{}\"", layout.name);
    let allowed = layout.core_dir.join("src").join("name.rs");
    let holders: Vec<&PathBuf> = files
        .iter()
        .filter(|file| file.text.contains(&needle))
        .map(|file| &file.path)
        .collect();
    let mut violations = Vec::new();
    for path in &holders {
        if **path != allowed {
            violations.push(format!(
                "name-literal: {} が NAME の字面を持つ（name.rs だけが持てる）",
                path.display()
            ));
        }
    }
    if !holders.iter().any(|path| **path == allowed) {
        violations.push(format!(
            "name-literal: {} に NAME の字面が無い",
            allowed.display()
        ));
    }
    Measured {
        fact: format!("name-literal={}", holders.len()),
        violations,
    }
}

#[cfg(test)]
mod tests {
    use super::{measure_core_lines, measure_core_spawn, measure_test_src_ratio, ratio_violations};
    use crate::check::{Layout, SourceFile};
    use crate::limits::Limits;
    use std::path::PathBuf;

    /// 擬似 workspace の core crate 名（実 NAME の字面を持ち込まない別名・`env_reads::tests` と同形）。
    const CORE: &str = "demo";

    /// 歯と本体を持つ file の fixture。**core の `pipe::declaration::write_set` の歯と同じ字面・同じ値**
    /// （幅 10 で test 6 / src 4・`workspace::tests` の split の歯とも同じ）＝2 crate の切り方の一致を守る。
    const SPLIT_FIXTURE: &str = "fn a() {}\nabcdefghijklmnopqrstuvwxy\n#[cfg(test)]\nmod t {}\nabcdefghijklmnopqrstuvwxy\n";

    /// 歯を持たない file の fixture（幅 10 で 3 行・全部 src）。
    const BARE_FIXTURE: &str = "abcdefghijklmnopqrstuvwxy\n";

    /// (member, `src/` からの相対 path, 本文) の列から measure の入力（Layout と file 列）を組む。
    fn workspace(files: &[(&str, &str, &str)]) -> (Layout, Vec<SourceFile>) {
        let root = PathBuf::from("/fixture-root");
        let core_dir = root.join("crates").join(CORE);
        let sources = files
            .iter()
            .map(|(member, rel, text)| SourceFile {
                path: root.join("crates").join(member).join("src").join(rel),
                text: (*text).to_owned(),
            })
            .collect();
        let layout = Layout { root, core_dir, member_dirs: Vec::new(), name: CORE.to_owned() };
        (layout, sources)
    }

    /// core-lines の上限と幅だけを振った閾値（他は測らないので 1）。
    fn limits(core_lines: u64, line_width: u64) -> Limits {
        Limits {
            core_lines,
            file_lines: 1,
            test_src_ratio_pct: 1,
            fn_lines: 1,
            fn_complexity: 1,
            fn_args: 1,
            line_width,
            dep_budget: 1,
            dep_per_pr: 1,
            check_delta_ms: 1,
            tmux_test_threads: 1,
        }
    }

    /// core-lines は in-file の歯（行頭 `#[cfg(test)]` から file 末尾）を数えない: fixture 2 本の本体は幅 10 で
    /// 4 + 3 = 7 行（file 全体なら 10 + 3 = 13）。上限 7 は通り 6 は落ちる（値が 7 であることの両側の歯）。別 member の
    /// file は母集団外。file 全体を数える実装は 13 で両方落ちる。
    #[test]
    fn sizes_core_lines_exclude_in_file_tests() {
        let (layout, files) = workspace(&[
            (CORE, "heavy.rs", SPLIT_FIXTURE),
            (CORE, "bare.rs", BARE_FIXTURE),
            ("xtask", "other.rs", SPLIT_FIXTURE),
        ]);
        let fits = measure_core_lines(&layout, &files, &limits(7, 10));
        assert_eq!(fits.fact, "core-lines=7/7", "本体の合計 = 4 + 3");
        assert_eq!(fits.violations, Vec::<String>::new(), "上限ちょうどは通る");
        let over = measure_core_lines(&layout, &files, &limits(6, 10));
        assert_eq!(over.fact, "core-lines=7/6");
        assert_eq!(over.violations.len(), 1, "{:?}", over.violations);
        assert!(
            over.violations.first().is_some_and(|line| line.starts_with("core-lines: ") && line.contains(" 7 行")),
            "本体の行数を名指す: {:?}",
            over.violations
        );
        // 幅を広げれば改行の数（本体 2 + 1 = 3・file 全体なら 5 + 1 = 6）。
        assert_eq!(measure_core_lines(&layout, &files, &limits(3, 120)).fact, "core-lines=3/3");
    }

    /// 名で test の file（`#[path]` で外出しした歯・行頭 `#[cfg(test)]` を持たない）は test 側に載る: 本体 1 本
    /// （幅 10 で 3 行）と `select_tests.rs`（同じ本文で 3 行）の toy workspace で、test-src-ratio は 3/3・core-lines
    /// は本体の 3 だけ（名を見ない実装は分子 0 の 0/6・core-lines 6）。
    #[test]
    fn sizes_ratio_counts_named_test_files_on_the_test_side() {
        let (layout, files) = workspace(&[(CORE, "select.rs", BARE_FIXTURE), (CORE, "select_tests.rs", BARE_FIXTURE)]);
        let ratio = measure_test_src_ratio(&files, &limits(100, 10));
        assert_eq!(ratio.fact, "test-src-ratio=3/3", "分子が名で test の file を数える");
        assert_eq!(measure_core_lines(&layout, &files, &limits(100, 10)).fact, "core-lines=3/100", "本体だけ");
    }

    /// core-spawn は core の `src` で `Command::new` を含む行の件数と file 数を出し、**違反は立てない**（検出線）:
    /// 2 行持つ file + 歯の区間に 1 行持つ file = 3 件 / 2 file。持たない file と別 member の file は数えない。
    /// 起動が 0 の木は `0/0`。
    #[test]
    fn sizes_core_spawn_counts_lines_and_files_without_denying() {
        let (layout, files) = workspace(&[
            (CORE, "a.rs", "use std::process::Command;\n\nfn f() {\n    let _ = Command::new(\"git\");\n    let _ = std::process::Command::new(\"tmux\");\n}\n"),
            (CORE, "b.rs", "fn g() {}\n\n#[cfg(test)]\nmod tests {\n    fn h() {\n        let _ = std::process::Command::new(\"sh\");\n    }\n}\n"),
            (CORE, "c.rs", "fn pure() {}\n"),
            ("xtask", "d.rs", "fn f() {\n    let _ = std::process::Command::new(\"cargo\");\n}\n"),
        ]);
        let got = measure_core_spawn(&layout, &files);
        assert_eq!(got.fact, "core-spawn=3/2", "件数 / file 数");
        assert_eq!(got.violations, Vec::<String>::new(), "検出線は違反を立てない");
        let (layout, files) = workspace(&[(CORE, "c.rs", "fn pure() {}\n")]);
        let none = measure_core_spawn(&layout, &files);
        assert_eq!(none.fact, "core-spawn=0/0");
        assert_eq!(none.violations, Vec::<String>::new());
    }

    /// 境界は manifest の pct の側で動く: pct = 100 で test 101 / src 100 は違反、
    /// pct = 150 で test 149 / src 100 は非違反（比 1.0 を焼いた実装は後者で落ちる）。
    #[test]
    fn sizes_ratio_boundary_moves_with_the_manifest_pct() {
        let over = ratio_violations(101, 100, 100);
        assert_eq!(over.len(), 1, "{over:?}");
        assert!(
            over.first().is_some_and(|line| line.starts_with("test-src-ratio: ") && line.contains("100%")),
            "違反文が pct を名指す: {over:?}"
        );
        assert_eq!(ratio_violations(100, 100, 100), Vec::<String>::new(), "等しいは通る");
        assert_eq!(ratio_violations(149, 100, 150), Vec::<String>::new(), "pct = 150 は 1.49 を通す");
        assert_eq!(ratio_violations(151, 100, 150).len(), 1, "pct = 150 は 1.51 を落とす");
        assert_eq!(ratio_violations(5, 0, 100), Vec::<String>::new(), "src 0 行は測れない（違反にしない）");
    }
}

