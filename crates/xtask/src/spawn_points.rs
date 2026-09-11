//! claude を起こす**構築点が `headless/` 配下で 1 つ**であることを字面で測る（claude-spawn-points）。
//!
//! ADR-0011 §2.1 の保証（settings を 1 つも読まない・宣言外の MCP を拾わない）は、runner と lens が
//! 同じ 1 関数 `headless::build` で claude を起こすことに依存する（憲法 C2）。呼出側へ
//! `Command::new` や flag の字面を複製する変異は argv が同型になるため、argv を比べる歯では
//! 原理的に落ちない（`.64` の lens LENS-3）。ここは `.88`（enum-slices）と同じ構造検査で、
//! (a) core の `headless/` 配下にある `Command::new(` の site と (b) core 全体の
//! `"--setting-sources"` / `"--strict-mcp-config"` の字面を持つ site を数え、どちらも
//! `headless/mod.rs` に**ちょうど 1 か所**であることを求める。
//!
//! **読めない形・見失った形は違反に倒す**（fail-closed）: `headless/` に file が無い、`mod.rs` に
//! `Command::new(` が 0 か所、flag の字面が core に 0 か所——どれも「黙って 1」にしない。
//! 他 dir の `Command::new`（git / tmux / sh を起こす pipe / seat / hook）は数えない＝**射程は (a) が
//! `headless/` 配下・(b) が core 全体**で、`headless/` の外で flag の字面を持たずに claude を起こす形は
//! 本 measure の外である（契約どおり・lens-101 MEDIUM-1 の記録）。
//! 行頭が `//` の行（doc / 行コメント）は字面の出所ではないので数えない。Rust の parser は
//! 足さない（字面で数える・憲法 C13）。閾値ではなく不変条件なので manifest 行を持たない
//! （name-literal / enum-slices と同じ型）。

use crate::check::{Layout, Measured, SourceFile};
use std::path::Path;

/// 判定行の tag。
const TAG: &str = "claude-spawn-points";

/// 構築点を持つ file（core の `src/` からの相対）。
const BUILD_REL: &str = "headless/mod.rs";

/// 構築点が在るべき dir（core の `src/` からの相対）。
const HEADLESS_REL: &str = "headless";

/// (a) claude を起こす呼出の字面。
const SPAWN_NEEDLE: &str = "Command::new(";

/// (b) 構築点だけが持つ flag の字面（quoted literal の形で数える）。
const FLAG_NEEDLES: &[&str] = &["\"--setting-sources\"", "\"--strict-mcp-config\""];

/// 字面 1 つの出所（core の `src/` からの相対 path と 1 始まりの行番号）。
struct Site {
    rel: String,
    line: usize,
}

/// core の `src/` 配下を測る。fact は (a) の site 数（healthy なら 1）。
pub(crate) fn measure(layout: &Layout, files: &[SourceFile]) -> Measured {
    let core_src = layout.core_dir.join("src");
    let headless = core_src.join(HEADLESS_REL);
    let mut violations = Vec::new();
    let spawns = sites_of(files, &core_src, SPAWN_NEEDLE, |path| path.starts_with(&headless));
    let mut in_build = 0_usize;
    for site in &spawns {
        if site.rel == BUILD_REL {
            in_build = in_build.saturating_add(1);
        } else {
            violations.push(format!(
                "{TAG}: {}:{}: `{SPAWN_NEEDLE}` が構築点（{BUILD_REL}）の外に在る",
                site.rel, site.line
            ));
        }
    }
    match in_build {
        0 => violations.push(format!(
            "{TAG}: {BUILD_REL} に `{SPAWN_NEEDLE}` が無い（構築点を見失った・{HEADLESS_REL}/ を読めない形も同じ）"
        )),
        1 => {}
        many => violations.push(format!(
            "{TAG}: {BUILD_REL} に `{SPAWN_NEEDLE}` が {many} か所（構築点は 1 つのはず）"
        )),
    }
    for needle in FLAG_NEEDLES {
        let found = sites_of(files, &core_src, needle, |_| true);
        let mut here = 0_usize;
        for site in &found {
            if site.rel == BUILD_REL {
                here = here.saturating_add(1);
            } else {
                violations.push(format!(
                    "{TAG}: {}:{}: {needle} の字面が構築点（{BUILD_REL}）の外に在る",
                    site.rel, site.line
                ));
            }
        }
        match here {
            0 => violations.push(format!(
                "{TAG}: {BUILD_REL} に {needle} の字面が無い（build が渡していない・読めない形も同じ）"
            )),
            1 => {}
            many => violations.push(format!(
                "{TAG}: {BUILD_REL} に {needle} の字面が {many} か所（1 つのはず）"
            )),
        }
    }
    Measured {
        fact: format!("{TAG}={}", spawns.len()),
        violations,
    }
}

/// `core_src` 配下で `keep` が真の file から、`needle` を含む行（行頭 `//` を除く）を集める。
fn sites_of(
    files: &[SourceFile],
    core_src: &Path,
    needle: &str,
    keep: impl Fn(&Path) -> bool,
) -> Vec<Site> {
    let mut found = Vec::new();
    for file in files {
        if !file.path.starts_with(core_src) || !keep(&file.path) {
            continue;
        }
        let rel = file
            .path
            .strip_prefix(core_src)
            .map(|rel| rel.display().to_string())
            .unwrap_or_else(|_| file.path.display().to_string());
        for (index, line) in file.text.lines().enumerate() {
            if line.trim_start().starts_with("//") || !line.contains(needle) {
                continue;
            }
            found.push(Site {
                rel: rel.clone(),
                line: index.saturating_add(1),
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    // 歯は module と同居させる（planner 裁定 案 A′・`s2-07l.101`）: 新 module の別 file `*_tests.rs` は
    // base に宣言元が無く compile されないので flip-check が測れない（flipcheck.rs の M4 注記）。
    // fixture は合成した [`SourceFile`] の列（disk を使わない＝measure は収集済みの file だけを見る純関数）。
    use super::measure;
    use crate::check::{Layout, SourceFile};
    use std::path::PathBuf;

    /// 擬似 workspace の core crate 名（実 NAME の字面を持ち込まない別名）。
    const CORE: &str = "demo";

    /// 健全な構築点（`Command::new(` 1 か所 + flag 2 つ）の `headless/mod.rs`。
    const BUILD_OK: &str = "pub fn build(call: &Call<'_>) -> Command {\n    let mut cmd = Command::new(call.claude);\n    cmd.arg(\"--setting-sources\").arg(\"\").arg(\"--strict-mcp-config\");\n    cmd\n}\n";

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

    /// 違反が `n` 件で、`needle` を含む行が在ることを表明する。
    fn assert_names(violations: &[String], n: usize, needle: &str) {
        assert_eq!(violations.len(), n, "違反は {n} 件のはず: {violations:?}");
        assert!(
            violations.iter().any(|line| line.contains(needle)),
            "{needle} を名指すはず: {violations:?}"
        );
    }

    /// 構築点の複製（runner.rs にも `Command::new(`）は、argv が同型でも字面で落ちる（`.64` LENS-3）。
    #[test]
    fn spawn_points_reports_command_new_copied_into_runner() {
        let (layout, files) = workspace(&[("headless/mod.rs", BUILD_OK)]);
        let ok = measure(&layout, &files);
        assert!(ok.violations.is_empty(), "健全な構築点は通る: {:?}", ok.violations);
        assert_eq!(ok.fact, "claude-spawn-points=1", "構築点を 1 つ数える");

        let (layout, files) = workspace(&[
            ("headless/mod.rs", BUILD_OK),
            ("headless/runner.rs", "fn launch(call: &Call<'_>) {\n    let _probe = Command::new(call.claude);\n}\n"),
        ]);
        let got = measure(&layout, &files);
        assert_names(&got.violations, 1, "headless/runner.rs:2");
        assert_eq!(got.fact, "claude-spawn-points=2", "複製した site も数に出る");
        // path 付きの呼び方（`std::process::Command::new(`）も同じ 1 本。
        let (layout, files) = workspace(&[
            ("headless/mod.rs", BUILD_OK),
            ("headless/lens.rs", "fn ask() {\n    std::process::Command::new(\"x\");\n}\n"),
        ]);
        assert_names(&measure(&layout, &files).violations, 1, "headless/lens.rs:2");
    }

    /// flag の字面を呼出側へ複製する形（lens が `"--strict-mcp-config"` を自前で足す）も落ちる。
    /// 構築点の外に在る字面は core のどの dir でも数える（headless/ に限らない）。
    #[test]
    fn spawn_points_reports_flag_literal_outside_build() {
        let (layout, files) = workspace(&[
            ("headless/mod.rs", BUILD_OK),
            ("headless/lens.rs", "fn ask(call: &Call<'_>) {\n    let mut cmd = build(call);\n    cmd.arg(\"--strict-mcp-config\");\n}\n"),
        ]);
        assert_names(&measure(&layout, &files).violations, 1, "headless/lens.rs:3");
        let (layout, files) = workspace(&[
            ("headless/mod.rs", BUILD_OK),
            ("pipe/gate.rs", "fn go() {\n    let _ = \"--setting-sources\";\n}\n"),
        ]);
        assert_names(&measure(&layout, &files).violations, 1, "pipe/gate.rs:2");
        // 構築点の中で 2 度渡す形も 1 つではない。
        let (layout, files) = workspace(&[(
            "headless/mod.rs",
            "fn build() {\n    let mut cmd = Command::new(\"c\");\n    cmd.arg(\"--setting-sources\").arg(\"\");\n    cmd.arg(\"--setting-sources\").arg(\"\");\n    cmd.arg(\"--strict-mcp-config\");\n}\n",
        )]);
        assert_names(&measure(&layout, &files).violations, 1, "\"--setting-sources\" の字面が 2 か所");
    }

    /// **見失った形は違反**（fail-closed）: `mod.rs` に `Command::new(` が無い・headless/ に file が
    /// 1 つも無い・flag の字面が無い、のどれも「1 つ」に化けさせない。
    #[test]
    fn spawn_points_fails_closed_when_the_build_point_is_missing() {
        let (layout, files) = workspace(&[(
            "headless/mod.rs",
            "fn build() {\n    let mut cmd = other();\n    cmd.arg(\"--setting-sources\").arg(\"\").arg(\"--strict-mcp-config\");\n}\n",
        )]);
        let got = measure(&layout, &files);
        assert_names(&got.violations, 1, "構築点を見失った");
        assert_eq!(got.fact, "claude-spawn-points=0", "0 を 0 と出す（伏せない）");

        let (layout, files) = workspace(&[("pipe/mod.rs", "fn x() {}\n")]);
        let got = measure(&layout, &files);
        assert_eq!(got.violations.len(), 3, "構築点 0 + flag 2 つ分が違反: {:?}", got.violations);
        for needle in ["構築点を見失った", "\"--setting-sources\" の字面が無い", "\"--strict-mcp-config\" の字面が無い"] {
            assert!(got.violations.iter().any(|line| line.contains(needle)), "{needle} を名指す: {:?}", got.violations);
        }
        assert_eq!(got.fact, "claude-spawn-points=0", "headless/ に file が無い木も 0 を 0 と出す");

        let (layout, files) = workspace(&[(
            "headless/mod.rs",
            "fn build() {\n    let mut cmd = Command::new(\"c\");\n    cmd.arg(\"--setting-sources\").arg(\"\");\n}\n",
        )]);
        assert_names(&measure(&layout, &files).violations, 1, "\"--strict-mcp-config\" の字面が無い");
        // 2 つ目の構築点は違反（1 つのはず）。
        let (layout, files) = workspace(&[(
            "headless/mod.rs",
            "fn build() {\n    let a = Command::new(\"c\");\n    let b = Command::new(\"d\");\n    let _ = (\"--setting-sources\", \"--strict-mcp-config\");\n}\n",
        )]);
        assert_names(&measure(&layout, &files).violations, 1, "`Command::new(` が 2 か所");
    }

    /// 他 dir の `Command::new`（git / tmux / sh）とコメント行は数えない（偽陽性を出さない）。
    #[test]
    fn spawn_points_ignores_other_dirs_and_comment_lines() {
        let (layout, files) = workspace(&[
            ("headless/mod.rs", "// Command::new( を書く場所はここだけ\n/// `\"--setting-sources\"` は build が渡す\nfn build() {\n    let mut cmd = Command::new(\"c\");\n    cmd.arg(\"--setting-sources\").arg(\"\").arg(\"--strict-mcp-config\");\n}\n"),
            ("pipe/mod.rs", "fn git() {\n    std::process::Command::new(\"git\");\n}\n"),
            ("seat/mod.rs", "fn tmux() {\n    Command::new(\"tmux\");\n}\n"),
        ]);
        let got = measure(&layout, &files);
        assert!(got.violations.is_empty(), "他 dir とコメントは数えない: {:?}", got.violations);
        assert_eq!(got.fact, "claude-spawn-points=1");
        // core の外（task runner 自身の src）は母集団に入らない。
        let (layout, mut files) = workspace(&[("headless/mod.rs", BUILD_OK)]);
        files.push(SourceFile {
            path: layout.root.join("crates/xtask/src/probe.rs"),
            text: "fn probe() {\n    Command::new(\"c\");\n    let _ = \"--strict-mcp-config\";\n}\n".to_owned(),
        });
        assert!(measure(&layout, &files).violations.is_empty(), "core の外は数えない");
    }
}
