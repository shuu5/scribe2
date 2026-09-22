//! core が **env を読まない**ことを字面で測る（env-reads・憲法 C2.2 / SRS FR17）。
//!
//! 設定の口は manifest の 1 面だけである（設計 rules-manifest.md §5）。C2.2 は「環境変数を直接
//! 読まない・新しい env の seam を作らない」と定めるが、`std::env::var("SCRIBE2_STATE_DIR")` を
//! core に 1 行足しても lint も measure も当たらなかった（監査 2026-09-12 塊 26・`s2-07l.178`）。
//! ここは [`crate::spawn_points`] と同型の字面走査で、core の `src` 配下の `.rs` の**非 test 区間**
//! （最初の行頭 `#[cfg(test)]` より前・名で test の file は丸ごと除く＝[`SourceFile::split_test_src`] と同じ切り方）に在る
//! `std::env::` / `env::` の直後の識別子を数え、[`ALLOWED`] に無いものを全件 `file:line 識別子` で
//! 名指す。
//!
//! 許す集合は閉じた const（閾値ではないので manifest 行を持たない＝C5 非該当・clippy.toml の
//! `disallowed-methods` は規則値を manifest 外の 2 面目に増やすので採らない）: `args` / `args_os` は
//! CLI の入口、`current_dir` は cwd の解決で、どちらも設定の値ではない。`var` / `var_os` / `vars` /
//! `temp_dir`（`TMPDIR` を読む）/ `home_dir` / `set_var` / `current_exe` …＝集合に無いものは全部違反。
//! `env!(` / `option_env!(` は compile 時の macro（実行時の seam ではない）で、`env::` の字面を
//! 持たないので母集団に入らない。行頭 `//` の行（`///` doc を含む）は字面の出所ではないので外す。
//! fact は `env-reads=<違反>/<母集団>` で、母集団（非 test の `env::` 参照の総数）を伏せない（C10）。
//! Rust の parser は足さない（字面で数える・憲法 C13）。

use crate::check::{Layout, Measured, SourceFile};
use crate::workspace::is_named_test_file;

/// 判定行の tag。
const TAG: &str = "env-reads";

/// 参照の字面（`std::env::` もこれを含む）。直前が識別子文字なら別名（`myenv::`）として数えない。
const NEEDLE: &str = "env::";

/// 非 test 区間の終わり（行頭の印・`workspace::TEST_MOD_MARK` と同じ字面）。
const TEST_MOD_MARK: &str = "#[cfg(test)]";

/// 許す識別子（宣言順・閉じた集合）。設定の値を運ばないものだけ。
const ALLOWED: &[&str] = &["args", "args_os", "current_dir"];

/// 参照 1 つの出所。
struct Site {
    /// core の `src/` からの相対 path。
    rel: String,
    /// 1 始まりの行番号。
    line: usize,
    /// `env::` の直後の識別子（`{` 等で読めなければ空）。
    ident: String,
}

/// core の `src/` 配下の非 test 区間を測る。fact は `違反/母集団`。
pub(crate) fn measure(layout: &Layout, files: &[SourceFile]) -> Measured {
    let core_src = layout.core_dir.join("src");
    let mut violations = Vec::new();
    let mut population = 0_usize;
    for file in files {
        if !file.path.starts_with(&core_src) || is_named_test_file(&file.path) {
            continue;
        }
        let rel = file
            .path
            .strip_prefix(&core_src)
            .map(|rel| rel.display().to_string())
            .unwrap_or_else(|_| file.path.display().to_string());
        for site in sites_in(&rel, &file.text) {
            population = population.saturating_add(1);
            if site.ident.is_empty() {
                violations.push(format!(
                    "{TAG}: {}:{} `{NEEDLE}` の直後に識別子が無い（`env::{{…}}` の束ね import 等・読めない形は違反）",
                    site.rel, site.line
                ));
            } else if !ALLOWED.contains(&site.ident.as_str()) {
                violations.push(format!(
                    "{TAG}: {}:{} {} （core は env を読まない・憲法 C2.2 / FR17・許すのは {ALLOWED:?} だけ）",
                    site.rel, site.line, site.ident
                ));
            }
        }
    }
    Measured {
        fact: format!("{TAG}={}/{population}", violations.len()),
        violations,
    }
}

/// 1 file の非 test 区間（最初の行頭 `#[cfg(test)]` より前）から、comment 行を除いて参照を集める。
fn sites_in(rel: &str, text: &str) -> Vec<Site> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.starts_with(TEST_MOD_MARK) {
            break;
        }
        if line.trim_start().starts_with("//") {
            continue;
        }
        for at in occurrences(line) {
            let after = line.get(at.saturating_add(NEEDLE.len())..).unwrap_or_default();
            let ident: String = after.chars().take_while(|ch| is_ident(*ch)).collect();
            found.push(Site {
                rel: rel.to_owned(),
                line: index.saturating_add(1),
                ident,
            });
        }
    }
    found
}

/// 行の中で `env::` が始まる byte 位置（直前が識別子文字のものは別名なので除く）。
fn occurrences(line: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0_usize;
    while let Some(offset) = line.get(from..).and_then(|rest| rest.find(NEEDLE)) {
        let at = from.saturating_add(offset);
        let preceded_by_ident = line
            .get(..at)
            .and_then(|head| head.chars().next_back())
            .is_some_and(is_ident);
        if !preceded_by_ident {
            out.push(at);
        }
        from = at.saturating_add(NEEDLE.len());
    }
    out
}

/// Rust の識別子を成す文字。
fn is_ident(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    // 歯は module と同居させる（案 A′・`s2-07l.101`）: 新 module の別 file `*_tests.rs` は base に
    // 宣言元が無く compile されないので flip-check が測れない。base で RED になる歯は
    // `check_tests.rs` の `SUMMARY_PIN`（fact 列の pin）に置く。
    // fixture は合成した [`SourceFile`] の列（disk を使わない＝measure は収集済みの file だけを見る純関数）。
    use super::measure;
    use crate::check::{Layout, SourceFile};
    use crate::workspace::collect_rs_files;
    use std::path::PathBuf;

    /// 擬似 workspace の core crate 名（実 NAME の字面を持ち込まない別名）。
    const CORE: &str = "demo";

    /// core の `src/` からの相対 path と本文の対から、measure の入力（Layout と file 列）を組む。
    fn workspace(files: &[(&str, &str)]) -> (Layout, Vec<SourceFile>) {
        let root = PathBuf::from("/fixture-root");
        let core_dir = root.join("crates").join(CORE);
        let sources = files
            .iter()
            .map(|(rel, text)| SourceFile {
                path: core_dir.join("src").join(rel),
                text: (*text).to_owned(),
            })
            .collect();
        let layout = Layout {
            root,
            core_dir,
            member_dirs: Vec::new(),
            name: CORE.to_owned(),
        };
        (layout, sources)
    }

    /// 違反 `n` 件で、`needle` を含む行が在ることを表明する。
    fn assert_names(violations: &[String], n: usize, needle: &str) {
        assert_eq!(violations.len(), n, "違反は {n} 件のはず: {violations:?}");
        assert!(
            violations.iter().any(|line| line.contains(needle)),
            "{needle} を名指すはず: {violations:?}"
        );
    }

    /// (a) 現物の core は違反 0 で、母集団は 5 以上（bin 本体〔`main.rs` の `args`〕は境界 crate へ移り core の母集団に
    /// 居ない＝6 → 5・設計 core-boundary.md §3）。母集団の下限を見るのは、走査が空振りして 0/0 の緑に化ける形を塞ぐため（C10）。
    #[test]
    fn env_reads_passes_on_core_with_a_nonempty_population() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let layout = Layout::discover(&root).expect("自 workspace は読める");
        let files = collect_rs_files(&layout.root).expect("crates/*/src を読める");
        let bin = layout.core_dir.join("src").join("main.rs");
        assert!(files.iter().all(|file| file.path != bin), "bin 本体は core の母集団に居ない: {}", bin.display());
        let got = measure(&layout, &files);
        assert!(got.violations.is_empty(), "core は env を読まないはず: {:?}", got.violations);
        let population: usize = got
            .fact
            .strip_prefix("env-reads=0/")
            .and_then(|tail| tail.parse().ok())
            .unwrap_or_else(|| panic!("fact は env-reads=0/<n> の形のはず: {}", got.fact));
        assert!(population >= 5, "母集団は 5 以上のはず: {}", got.fact);
    }

    /// (b) 非 test 区間の `std::env::var("X")` は違反 1 件で、`file:line` と `var` を名指す。
    #[test]
    fn env_reads_reports_var_in_src_region_by_file_line_and_ident() {
        let (layout, files) = workspace(&[(
            "pipe/cli.rs",
            "fn state_dir() -> Option<String> {\n    std::env::var(\"X\").ok()\n}\n",
        )]);
        let got = measure(&layout, &files);
        assert_names(&got.violations, 1, "pipe/cli.rs:2 var ");
        assert_eq!(got.fact, "env-reads=1/1", "違反 1 / 母集団 1");
    }

    /// (c) 同じ字面でも `#[cfg(test)]` 以後は違反でなく、母集団にも入らない。
    #[test]
    fn env_reads_ignores_the_test_region() {
        let (layout, files) = workspace(&[(
            "pipe/cli.rs",
            "fn state_dir() {}\n#[cfg(test)]\nmod tests {\n    fn probe() {\n        let _ = std::env::var(\"X\");\n        let _ = std::env::temp_dir();\n    }\n}\n",
        )]);
        let got = measure(&layout, &files);
        assert!(got.violations.is_empty(), "test 区間は数えない: {:?}", got.violations);
        assert_eq!(got.fact, "env-reads=0/0", "母集団にも入らない");
        // 印より前の同じ字面は数える（区間の切り方は最初の行頭 `#[cfg(test)]`）。
        let (layout, files) = workspace(&[(
            "pipe/cli.rs",
            "fn state_dir() {\n    let _ = std::env::var(\"X\");\n}\n#[cfg(test)]\nmod tests {}\n",
        )]);
        assert_names(&measure(&layout, &files).violations, 1, "pipe/cli.rs:2 var ");
    }

    /// (d) `use std::env;` + `env::var_os(...)` の形（`std::` 無し）も拾う。
    #[test]
    fn env_reads_catches_the_short_form_after_use_std_env() {
        let (layout, files) = workspace(&[(
            "seat/mod.rs",
            "use std::env;\n\nfn home() -> Option<std::ffi::OsString> {\n    env::var_os(\"HOME\")\n}\n",
        )]);
        let got = measure(&layout, &files);
        assert_names(&got.violations, 1, "seat/mod.rs:4 var_os ");
        assert_eq!(got.fact, "env-reads=1/1", "`use std::env;` 自体は `env::` を持たないので母集団 1");
        // 別名の module（`myenv::var`）は `env::` の参照ではない。
        let (layout, files) = workspace(&[("seat/mod.rs", "fn x() {\n    myenv::var(\"H\");\n}\n")]);
        assert_eq!(measure(&layout, &files).fact, "env-reads=0/0", "識別子の続きは数えない");
    }

    /// (e) `env!("CARGO_PKG_VERSION")` だけの file は違反 0・母集団 0（compile 時の macro）。
    #[test]
    fn env_reads_excludes_compile_time_macros() {
        let (layout, files) = workspace(&[(
            "main.rs",
            "const VERSION: &str = env!(\"CARGO_PKG_VERSION\");\nconst SHA: Option<&str> = option_env!(\"GIT_SHA\");\n",
        )]);
        let got = measure(&layout, &files);
        assert!(got.violations.is_empty(), "macro は違反でない: {:?}", got.violations);
        assert_eq!(got.fact, "env-reads=0/0", "母集団にも入らない");
    }

    /// (f) doc comment に `std::env::temp_dir` と書いた file は違反 0（pipe/land.rs の「使わない」の doc）。
    #[test]
    fn env_reads_ignores_comment_and_doc_lines() {
        let (layout, files) = workspace(&[(
            "pipe/land.rs",
            "/// **`std::env::temp_dir` を使わない**（`TMPDIR` を読む）。\n// std::env::var は書かない\n    //! env::home_dir も同じ\nfn place() {}\n",
        )]);
        let got = measure(&layout, &files);
        assert!(got.violations.is_empty(), "comment 行は数えない: {:?}", got.violations);
        assert_eq!(got.fact, "env-reads=0/0", "母集団にも入らない");
    }

    /// (g) `std::env::current_dir()` は許す集合の中＝違反 0・母集団 1。
    #[test]
    fn env_reads_allows_current_dir_and_counts_it() {
        let (layout, files) = workspace(&[(
            "hook/vessel.rs",
            "fn cwd() -> Result<std::path::PathBuf, String> {\n    std::env::current_dir().map_err(|err| format!(\"cwd を解決できない: {err}\"))\n}\n",
        )]);
        let got = measure(&layout, &files);
        assert!(got.violations.is_empty(), "current_dir は許す: {:?}", got.violations);
        assert_eq!(got.fact, "env-reads=0/1", "母集団には数える");
        // 許す集合の外は全部違反（列挙ではなく集合に無いこと）。1 行に 2 つ在れば 2 件。
        let (layout, files) = workspace(&[(
            "hook/vessel.rs",
            "fn x() {\n    let _ = (std::env::temp_dir(), env::current_exe());\n    std::env::set_var(\"A\", \"b\");\n}\n",
        )]);
        let got = measure(&layout, &files);
        assert_eq!(got.violations.len(), 3, "temp_dir / current_exe / set_var の 3 件: {:?}", got.violations);
        for needle in ["hook/vessel.rs:2 temp_dir ", "hook/vessel.rs:2 current_exe ", "hook/vessel.rs:3 set_var "] {
            assert!(got.violations.iter().any(|line| line.contains(needle)), "{needle}: {:?}", got.violations);
        }
        assert_eq!(got.fact, "env-reads=3/3");
        // core の外（task runner 自身の src）は母集団に入らない。
        let (layout, mut files) = workspace(&[("main.rs", "fn m() {\n    std::env::args();\n}\n")]);
        files.push(SourceFile {
            path: layout.root.join("crates/xtask/src/main.rs"),
            text: "fn m() {\n    std::env::var(\"X\");\n}\n".to_owned(),
        });
        assert_eq!(measure(&layout, &files).fact, "env-reads=0/1", "core の外は数えない");
    }
}
