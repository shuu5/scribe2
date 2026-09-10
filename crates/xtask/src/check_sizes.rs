//! `cargo xtask check` の**大きさ系 measure**（core-lines / file-lines / test-src-ratio /
//! name-literal）。母集団は `collect_rs_files` が読んだ `.rs` の列である。
//!
//! `check.rs` から分けたのは憲法 C4（1 file の上限）のためで、**測る内容は 1 つも変えていない**
//! （`s2-07l.84`・純粋な移動）。判定行の名前・順序・値の書式は不変である。

use crate::check::{Layout, Measured, SourceFile};
use crate::limits::{MAX_CORE_LINES, MAX_FILE_LINES};
use std::path::PathBuf;

/// core crate の `src` 配下の総行数（core-lines）。
pub(crate) fn measure_core_lines(layout: &Layout, files: &[SourceFile]) -> Measured {
    let core_src = layout.core_dir.join("src");
    let total: usize = files
        .iter()
        .filter(|file| file.path.starts_with(&core_src))
        .map(SourceFile::lines)
        .sum();
    let mut violations = Vec::new();
    if total > MAX_CORE_LINES {
        violations.push(format!(
            "core-lines: core crate の src が {total} 行で上限 {MAX_CORE_LINES} 行を超える"
        ));
    }
    Measured {
        fact: format!("core-lines={total}/{MAX_CORE_LINES}"),
        violations,
    }
}

/// `crates/*/src` 配下 `.rs` の 1 file 行数（file-lines）。
pub(crate) fn measure_file_lines(files: &[SourceFile]) -> Measured {
    let mut violations = Vec::new();
    let mut worst = 0;
    for file in files {
        let lines = file.lines();
        worst = worst.max(lines);
        if lines > MAX_FILE_LINES {
            violations.push(format!(
                "file-lines: {} が {lines} 行で上限 {MAX_FILE_LINES} 行を超える",
                file.path.display()
            ));
        }
    }
    Measured {
        fact: format!("file-lines={worst}/{MAX_FILE_LINES}"),
        violations,
    }
}

/// test 行と src 行の比（test-src-ratio）。整数比較で `Σtest <= Σsrc` を見る。
pub(crate) fn measure_test_src_ratio(files: &[SourceFile]) -> Measured {
    let mut test_total = 0;
    let mut src_total = 0;
    for file in files {
        let (test, src) = file.split_test_src();
        test_total += test;
        src_total += src;
    }
    let mut violations = Vec::new();
    if src_total > 0 && test_total > src_total {
        violations.push(format!(
            "test-src-ratio: test {test_total} 行 > src {src_total} 行（比の上限は 1.0）"
        ));
    }
    Measured {
        fact: format!("test-src-ratio={test_total}/{src_total}"),
        violations,
    }
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

