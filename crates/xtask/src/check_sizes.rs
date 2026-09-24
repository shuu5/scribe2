//! `cargo xtask check` の**大きさ系 measure**（core-lines / core-spawn / boundary-spawn / boundary-lines / file-lines /
//! test-src-ratio / name-literal）。母集団は `collect_rs_files` が読んだ `.rs` の列である。
//!
//! `check.rs` から分けたのは憲法 C4（1 file の上限）のためで、分けた周は**測る内容は 1 つも変えていない**
//! （`s2-07l.84`・純粋な移動）。判定行の名前・順序・値の書式は不変である。閾値は const でなく
//! manifest から読んだ [`Limits`] で受ける（`s2-07l.163`・憲法 C1）。
//!
//! core-lines の母集団は core crate の `src` の**本体**（各 file の最初の行頭 `#[cfg(test)]` より前＝
//! [`SourceFile::split_test_src`] の src 側・in-file の歯は R-C4-3 が数える側で二重計上しない・user 裁定
//! 2026-09-15・ADR-0033・設計 core-boundary.md §2）。core-spawn は core の `src` の本体で `Command::new` を含む行を
//! 数え 1 以上を deny、boundary-spawn は境界 crate の `src` の本体で同じ字面を持つ file が 2 本以上を deny、
//! boundary-lines は境界 crate の `src` の本体を R-C4-5 で縛る（設計 core-boundary.md §9 行 i・ADR-0062）。

use crate::check::{Layout, Measured, SourceFile};
use crate::limits::Limits;
use crate::workspace::is_named_test_file;
use std::path::{Path, PathBuf};

/// 子 process を起こす字面（core-spawn の母集団・設計 core-boundary.md §4「規則は 1 つ」）。
const SPAWN_NEEDLE: &str = "Command::new";

/// in-file の歯の始まりの行頭の印（[`SourceFile::split_test_src`] と同じ字面・本体と歯の区間の切れ目）。
const TEST_MOD_MARK: &str = "#[cfg(test)]";

/// 境界 crate の `src` の本体で `Command::new` を持ってよい file の本数（std の Command の構築は 1 か所・ADR-0062）。
const BOUNDARY_SPAWN_HOLDERS: usize = 1;

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

/// file の**本体**の行（core-lines と同じ切り方: 最初の行頭 `#[cfg(test)]` より前・名で test の file は 0 行）。
fn body_lines(file: &SourceFile) -> impl Iterator<Item = &str> {
    let whole_test = is_named_test_file(&file.path);
    file.text.lines().take_while(move |line| !whole_test && !line.starts_with(TEST_MOD_MARK))
}

/// `src` 配下の file ごとの、本体で `Command::new` を含む行の件数（0 の file は載せない・path 順）。
fn spawn_holders<'a>(src: &Path, files: &'a [SourceFile]) -> Vec<(&'a PathBuf, usize)> {
    files
        .iter()
        .filter(|file| file.path.starts_with(src))
        .map(|file| (&file.path, body_lines(file).filter(|line| line.contains(SPAWN_NEEDLE)).count()))
        .filter(|(_, found)| *found > 0)
        .collect()
}

/// 起動の site の件数と file 数の fact（`<tag>=<件数>/<file 数>`）。
fn spawn_fact(tag: &str, holders: &[(&PathBuf, usize)]) -> String {
    let sites: usize = holders.iter().map(|(_, found)| found).sum();
    format!("{tag}={sites}/{}", holders.len())
}

/// core crate の `src` の**本体**で `Command::new` を含む行の件数と、それを持つ file の数（core-spawn・設計
/// core-boundary.md §9 採る形 9）。fact は `core-spawn=<件数>/<file 数>` で、1 件以上は file ごとに違反を立てる
/// （core は起動の記述を差し替え口へ渡し、std の Command を構築しない・ADR-0062）。歯の区間の実物と fixture の
/// 起動は数えない（量は R-C4-3 が縛る）。
pub(crate) fn measure_core_spawn(layout: &Layout, files: &[SourceFile]) -> Measured {
    let holders = spawn_holders(&layout.core_dir.join("src"), files);
    let violations = holders
        .iter()
        .map(|(path, found)| {
            format!(
                "core-spawn: {} の本体が Command::new を {found} 行持つ（core は起動の記述を差し替え口へ渡す・ADR-0062）",
                path.display()
            )
        })
        .collect();
    Measured {
        fact: spawn_fact("core-spawn", &holders),
        violations,
    }
}

/// 境界 crate の `src` の**本体**で `Command::new` を含む行の件数と file 数（boundary-spawn・設計 core-boundary.md
/// §9 採る形 9）。境界 crate の dir が無い木（[`Layout::boundary_dir`] が `None`）は measure を出さない。
pub(crate) fn measure_boundary_spawn(layout: &Layout, files: &[SourceFile]) -> Option<Measured> {
    layout.boundary_dir().map(|dir| boundary_spawn_in(&dir.join("src"), files))
}

/// boundary-spawn の判定（`src` を引数で受ける＝dir の実在を見ない fixture の歯が撃てる形）。fact は
/// `boundary-spawn=<件数>/<file 数>` で、file 数が [`BOUNDARY_SPAWN_HOLDERS`] を超えると持つ file を名指す違反を立てる
/// （値を持たない shape 検査で rules 行は持たない）。
fn boundary_spawn_in(src: &Path, files: &[SourceFile]) -> Measured {
    let holders = spawn_holders(src, files);
    let mut violations = Vec::new();
    if holders.len() > BOUNDARY_SPAWN_HOLDERS {
        let names: Vec<String> = holders.iter().map(|(path, _)| path.display().to_string()).collect();
        violations.push(format!(
            "boundary-spawn: 境界 crate の src の本体で Command::new を持つ file が {} 本（{BOUNDARY_SPAWN_HOLDERS} 本まで・ADR-0062）: {}",
            holders.len(),
            names.join(" ")
        ));
    }
    Measured {
        fact: spawn_fact("boundary-spawn", &holders),
        violations,
    }
}

/// 境界 crate の `src` の**本体**の総行数（boundary-lines・core-lines と同じ切り方と幅・上限は R-C4-5・設計
/// core-boundary.md §3 / §9）。境界 crate の dir が無い木は measure を出さない。
pub(crate) fn measure_boundary_lines(layout: &Layout, files: &[SourceFile], limits: &Limits, max: u64) -> Option<Measured> {
    layout.boundary_dir().map(|dir| boundary_lines_in(&dir.join("src"), files, width_of(limits), max))
}

/// boundary-lines の判定（`src` を引数で受ける・[`boundary_spawn_in`] と同じ理由）。
fn boundary_lines_in(src: &Path, files: &[SourceFile], width: usize, max: u64) -> Measured {
    let total: usize = files
        .iter()
        .filter(|file| file.path.starts_with(src))
        .map(|file| file.split_test_src(width).1)
        .sum();
    let mut violations = Vec::new();
    if as_u64(total) > max {
        violations.push(format!(
            "boundary-lines: 境界 crate の src の本体が {total} 行で上限 {max} 行（R-C4-5）を超える"
        ));
    }
    Measured {
        fact: format!("boundary-lines={total}/{max}"),
        violations,
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
    use super::{
        boundary_lines_in, boundary_spawn_in, measure_boundary_lines, measure_boundary_spawn, measure_core_lines, measure_core_spawn,
        measure_test_src_ratio, ratio_violations,
    };
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

    /// 本体で 2 行持つ file（`a.rs`）の fixture 本文。
    const TWO_SPAWNS: &str =
        "use std::process::Command;\n\nfn f() {\n    let _ = Command::new(\"git\");\n    let _ = std::process::Command::new(\"tmux\");\n}\n";

    /// 歯の区間（行頭 `#[cfg(test)]` の後ろ）にだけ 1 行持つ file の fixture 本文。
    const TEST_SECTION_SPAWN: &str =
        "fn g() {}\n\n#[cfg(test)]\nmod tests {\n    fn h() {\n        let _ = std::process::Command::new(\"sh\");\n    }\n}\n";

    /// 本体で 1 行持つ file の fixture 本文。
    const ONE_SPAWN: &str = "fn f() {\n    let _ = std::process::Command::new(\"cargo\");\n}\n";

    /// core-spawn は core の `src` の**本体**で `Command::new` を含む行の件数と file 数を出し、1 件以上を file ごとに
    /// deny する: 本体に 2 行の `a.rs` だけを数えて `2/1`。歯の区間にだけ持つ `b.rs`・名で test の `e_tests.rs`・持たない
    /// `c.rs`・別 member の `d.rs` は数えない（file 全体を数える実装は `4/3` で落ちる）。歯の区間と test の file にだけ
    /// 持つ木は `0/0` で違反 0（本体で 0 と読む）。
    #[test]
    fn sizes_core_spawn_denies_src_body_sites() {
        let (layout, files) = workspace(&[
            (CORE, "a.rs", TWO_SPAWNS),
            (CORE, "b.rs", TEST_SECTION_SPAWN),
            (CORE, "c.rs", "fn pure() {}\n"),
            (CORE, "e_tests.rs", ONE_SPAWN),
            ("xtask", "d.rs", ONE_SPAWN),
        ]);
        let got = measure_core_spawn(&layout, &files);
        assert_eq!(got.fact, "core-spawn=2/1", "本体の件数 / file 数");
        assert_eq!(got.violations.len(), 1, "本体に持つ file ごとに 1 件: {:?}", got.violations);
        assert!(
            got.violations.first().is_some_and(|line| line.starts_with("core-spawn: ") && line.contains("a.rs") && line.contains(" 2 行")),
            "持つ file と件数を名指す: {:?}",
            got.violations
        );
        let (layout, files) = workspace(&[(CORE, "b.rs", TEST_SECTION_SPAWN), (CORE, "e_tests.rs", ONE_SPAWN)]);
        let none = measure_core_spawn(&layout, &files);
        assert_eq!(none.fact, "core-spawn=0/0", "歯の区間と test の file は数えない");
        assert_eq!(none.violations, Vec::<String>::new(), "本体 0 は通る");
    }

    /// boundary-spawn は境界 crate の `src` の本体で `Command::new` を持つ file が 2 本以上を deny する: 本体に持つ file
    /// 2 本（2 行 + 1 行）は `3/2` で持つ 2 file を名指す違反 1 件。1 本と歯の区間だけの file は `2/1` で通り、歯の区間だけの
    /// 木は `0/0` で通る。core の file は境界 crate の母集団に入らない。境界 crate の dir が無い木は measure を出さない。
    #[test]
    fn sizes_boundary_spawn_denies_a_second_holder_file() {
        let (layout, files) = workspace(&[
            ("demo-boundary", "spawner.rs", TWO_SPAWNS),
            ("demo-boundary", "main.rs", ONE_SPAWN),
            ("demo-boundary", "lib.rs", TEST_SECTION_SPAWN),
            (CORE, "a.rs", ONE_SPAWN),
        ]);
        let src = layout.root.join("crates").join("demo-boundary").join("src");
        let two = boundary_spawn_in(&src, &files);
        assert_eq!(two.fact, "boundary-spawn=3/2", "本体の件数 / file 数");
        assert_eq!(two.violations.len(), 1, "{:?}", two.violations);
        assert!(
            two.violations.first().is_some_and(|line| {
                line.starts_with("boundary-spawn: ") && line.contains("main.rs") && line.contains("spawner.rs") && !line.contains("lib.rs")
            }),
            "本体に持つ 2 file を名指す: {:?}",
            two.violations
        );
        let one: Vec<SourceFile> = files.into_iter().filter(|file| !file.path.ends_with("main.rs")).collect();
        let fits = boundary_spawn_in(&src, &one);
        assert_eq!(fits.fact, "boundary-spawn=2/1", "1 本は通る");
        assert_eq!(fits.violations, Vec::<String>::new());
        let (_, tests_only) = workspace(&[("demo-boundary", "lib.rs", TEST_SECTION_SPAWN)]);
        let bare = boundary_spawn_in(&src, &tests_only);
        assert_eq!((bare.fact.as_str(), bare.violations.len()), ("boundary-spawn=0/0", 0), "歯の区間だけは数えない");
        assert!(measure_boundary_spawn(&layout, &one).is_none(), "境界 crate の dir が無い木は出さない");
    }

    /// boundary-lines は境界 crate の `src` の本体（core-lines と同じ切り方・幅 10 で `SPLIT_FIXTURE` 4 + `BARE_FIXTURE` 3 =
    /// 7 行）を上限と比べる: 上限 7 は通り 6 は落ちる（両側の歯・file 全体を数える実装は 13 で両方落ちる）。core の file は
    /// 数えない。境界 crate の dir が無い木は measure を出さない。
    #[test]
    fn sizes_boundary_lines_over_the_limit_is_denied() {
        let (layout, files) = workspace(&[
            ("demo-boundary", "main.rs", SPLIT_FIXTURE),
            ("demo-boundary", "lib.rs", BARE_FIXTURE),
            (CORE, "heavy.rs", SPLIT_FIXTURE),
        ]);
        let src = layout.root.join("crates").join("demo-boundary").join("src");
        let fits = boundary_lines_in(&src, &files, 10, 7);
        assert_eq!(fits.fact, "boundary-lines=7/7", "本体の合計 = 4 + 3");
        assert_eq!(fits.violations, Vec::<String>::new(), "上限ちょうどは通る");
        let over = boundary_lines_in(&src, &files, 10, 6);
        assert_eq!(over.fact, "boundary-lines=7/6");
        assert_eq!(over.violations.len(), 1, "{:?}", over.violations);
        assert!(
            over.violations.first().is_some_and(|line| line.starts_with("boundary-lines: ") && line.contains(" 7 行") && line.contains("R-C4-5")),
            "本体の行数と行 id を名指す: {:?}",
            over.violations
        );
        assert!(measure_boundary_lines(&layout, &files, &limits(1, 10), 6).is_none(), "境界 crate の dir が無い木は出さない");
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

