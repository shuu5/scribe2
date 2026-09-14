//! `cargo xtask check` の**大きさ系 measure**（core-lines / file-lines / test-src-ratio /
//! name-literal）。母集団は `collect_rs_files` が読んだ `.rs` の列である。
//!
//! `check.rs` から分けたのは憲法 C4（1 file の上限）のためで、**測る内容は 1 つも変えていない**
//! （`s2-07l.84`・純粋な移動）。判定行の名前・順序・値の書式は不変である。閾値は const でなく
//! manifest から読んだ [`Limits`] で受ける（`s2-07l.163`・憲法 C1）。

use crate::check::{Layout, Measured, SourceFile};
use crate::limits::Limits;
use std::path::PathBuf;

/// 行数（usize）を manifest の値（u64）と同じ型へ（64 bit では損失なし）。
fn as_u64(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

/// core crate の `src` 配下の総行数（core-lines）。
pub(crate) fn measure_core_lines(layout: &Layout, files: &[SourceFile], limits: &Limits) -> Measured {
    let core_src = layout.core_dir.join("src");
    let total: usize = files
        .iter()
        .filter(|file| file.path.starts_with(&core_src))
        .map(SourceFile::lines)
        .sum();
    let max = limits.core_lines;
    let mut violations = Vec::new();
    if as_u64(total) > max {
        violations.push(format!(
            "core-lines: core crate の src が {total} 行で上限 {max} 行を超える"
        ));
    }
    Measured {
        fact: format!("core-lines={total}/{max}"),
        violations,
    }
}

/// `crates/*/src` 配下 `.rs` の 1 file 行数（file-lines）。
pub(crate) fn measure_file_lines(files: &[SourceFile], limits: &Limits) -> Measured {
    let max = limits.file_lines;
    let mut violations = Vec::new();
    let mut worst = 0;
    for file in files {
        let lines = file.lines();
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

/// test 行と src 行の比（test-src-ratio）。整数比較で `Σtest × 100 <= Σsrc × pct` を見る。
pub(crate) fn measure_test_src_ratio(files: &[SourceFile], limits: &Limits) -> Measured {
    let mut test_total = 0;
    let mut src_total = 0;
    for file in files {
        let (test, src) = file.split_test_src();
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
    use super::ratio_violations;

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

