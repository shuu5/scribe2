//! `cargo xtask check` の本体。憲法 C4 の歯（行数 / 比 / manifest parity / lints）を測る。
//!
//! 違反は 1 件 1 行 `<tag>: <本文>` で返し、1 件以上なら CLI 面が rc 1 を返す。
//! 違反 0 のときだけ各 tag の実測値入りサマリを 1 行出す（失敗時にサマリは出さない）。
//! **測る tag の列挙はここに書かない**——列挙の SSOT は判定行を組み立てる実装
//! （[`summary`]）と、それが出す判定行そのものである（ADR-0013 §2.1）。doc へ写した列挙は
//! 腐るので、増減のたびに doc を直す形を採らない。
//!
//! workspace の走査（[`Layout`] / [`SourceFile`] / `read_text` 等）は [`crate::workspace`]、
//! seat-brief の measure は [`crate::seat_brief`]、歯は `check_tests.rs`（`check::tests`）に
//! 在る（`s2-07l.257` で挙動不変に分割）。外の呼び手の path は再 export で保つ。

pub(crate) use crate::workspace::{json_string_field, read_text, Layout, SourceFile};
use crate::limits::Limits;
use crate::workspace::collect_rs_files;
use std::path::Path;

/// paths-clean の母集団から外す tracked file。**ただ 1 本で固定**である
/// （bd が生成し private path 形の例をコメントに持つため）。
///
/// 測定一式は [`crate::paths_clean`] へ移したが、この名前だけは本 file に残す——
/// 免除を測る歯が本 file の test 区間（`check_tests.rs`）に在り、`#[cfg(test)]` を src 区間へ
/// 置くと test-src-ratio がその行から下（本 file の残り全部）を test 区間として数えるからである。
/// 実測値は本 file の行数そのものに依存するのでここへは焼かない（bead s2-07l.33 の notes）。
pub(crate) const PATHS_CLEAN_SKIP: &str = ".beads/config.yaml";

/// check の結果。違反行の列と、違反 0 のときに出す 1 行サマリを持つ。
pub struct Report {
    /// `<tag>: <本文>` 形の違反行。
    pub violations: Vec<String>,
    /// 各 tag の実測値を載せた 1 行サマリ。
    pub summary: String,
}

/// 1 tag の測定結果。
pub(crate) struct Measured {
    /// サマリ行に載せる `tag=値` の断片。
    pub(crate) fact: String,
    /// 違反行（合格なら空）。
    pub(crate) violations: Vec<String>,
}

/// 判定行の接頭辞。この右に各 measure の fact が空白区切りで並ぶ。
const SUMMARY_PREFIX: &str = "xtask check: ok";

/// workspace を測って [`Report`] を返す。
pub fn inspect(root: &Path) -> Report {
    let layout = match Layout::discover(root) {
        Ok(found) => found,
        Err(reason) => return blocked(&reason),
    };
    let files = match collect_rs_files(&layout.root) {
        Ok(found) => found,
        Err(reason) => return blocked(&reason),
    };
    // 閾値は manifest の 1 面から 1 回だけ読む（憲法 C1・SRS FR17）。読めない周は測れないので
    // 全体を止める（不備を 1 つでも黙って既定値で埋めない・FR18）。
    let limits = match read_text(&layout.root.join(RULES_REL)).and_then(|text| Limits::read(&text)) {
        Ok(found) => found,
        Err(reason) => return blocked(&reason),
    };
    let mut measured = vec![
        crate::check_sizes::measure_core_lines(&layout, &files, &limits),
        crate::check_sizes::measure_core_spawn(&layout, &files),
        crate::check_sizes::measure_file_lines(&files, &limits),
        crate::check_sizes::measure_test_src_ratio(&files, &limits),
        crate::check_sizes::measure_name_literal(&layout, &files),
    ];
    measured.extend(crate::check_facts::measure_manifests(&layout));
    measured.extend(crate::check_facts::measure_lints(&layout));
    measured.push(crate::check_facts::measure_deps_empty(&layout));
    measured.push(crate::check_facts::measure_clippy_thresholds(&layout, &limits));
    measured.push(crate::check_facts::measure_nextest_tmux_group(&layout, &limits));
    measured.push(crate::check_facts::measure_dep_budget(&limits));
    measured.push(crate::check_facts::measure_toolchain_pin(&layout));
    measured.push(crate::paths_clean::measure(&layout));
    measured.push(crate::private_clean::measure(&layout));
    measured.push(crate::non_rust_exec::measure(&layout));
    measured.push(crate::non_rust_exec::ci_shell_lines(&layout));
    measured.push(crate::claude_md::measure(&layout));
    measured.push(crate::claude_md::measure_done(&layout));
    measured.push(crate::claude_md::measure_prose(&layout));
    measured.push(crate::enum_slices::measure(&files));
    measured.push(crate::spawn_points::measure(&layout, &files));
    measured.push(crate::env_reads::measure(&layout, &files));
    measured.push(crate::polarity::measure(&layout));
    measured.push(crate::polarity::measure_sites(&layout, &files));
    measured.push(crate::prose_gate::measure(&layout));
    measured.push(crate::seat_brief::measure(&layout));
    measured.push(crate::check_facts::measure_contracts_schema(&layout));
    measured.push(crate::rules_wired::measure(&layout, &files));
    measured.push(crate::rules_parity::measure(&layout));
    fold(measured)
}

/// rules manifest の相対 path（seat-brief の権能の行と歯の fixture が共有する）。
pub(crate) const RULES_REL: &str = "rules/manifest.toml";

/// 純関数面（■D1）。`root` 配下を測り違反行の列を返す。`process::exit` はしない。
pub fn check(root: &Path) -> Vec<String> {
    inspect(root).violations
}

/// 違反 0 のときに出す 1 行サマリ。
pub fn summary(root: &Path) -> String {
    inspect(root).summary
}

/// 判定行を**値を伏せた形**へ写す（tag の名前・並び・値の書式だけを残す）。
///
/// [`SUMMARY_PREFIX`] は字面のまま残し、その右の各 token は `=` より右の英数字の連なりを
/// `<v>` に置き、区切り（`/` `.` `(` 等）はそのまま残す: `a=12/300 b=0.1.0` →
/// `a=<v>/<v> b=<v>.<v>.<v>`。tag は触らない。`=` を持たない token（fact の値が空白を含んで
/// 割れた片割れ）は丸ごと伏せる＝環境で動く値が素通りしない（lens-87 MEDIUM-1）。英数字は
/// Unicode で見る（非 ASCII の値も伏せる）。値は環境で動く（file-lines / paths-clean 等）ので、
/// 外形として pin できるのはこの形までである。ADR-0013 §2.1 が SSOT と定めた判定行を、集合と
/// 順序で測る歯の材料（bd `s2-07l.87`）。
///
/// 呼ぶのは test 区間の pin だけで、runtime に判定行を消費する口は作らない（外形を増やさない）。
/// それでも src に置くのは、base に test 区間だけを写した木で compile error＝flip-check の RED
/// を構造で作るためである。非 test build では未使用になるので、憲法 C11 の口（理由付き expect）
/// で dead_code だけを除く。
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "test 区間の pin だけが呼ぶ（bd s2-07l.87・flip-check の RED を src 配置で作る）")
)]
pub fn shape(summary: &str) -> String {
    let (prefix, facts) = match summary.strip_prefix(SUMMARY_PREFIX) {
        Some(rest) => (SUMMARY_PREFIX, rest),
        None => ("", summary),
    };
    let veiled = facts
        .split(' ')
        .map(|token| match token.split_once('=') {
            Some((tag, value)) => format!("{tag}={}", veil(value)),
            None => veil(token),
        })
        .collect::<Vec<String>>()
        .join(" ");
    format!("{prefix}{veiled}")
}

/// 英数字の連なりを 1 つの `<v>` に畳む（区切り文字は残す）。
fn veil(value: &str) -> String {
    let mut out = String::new();
    let mut in_run = false;
    for ch in value.chars() {
        if ch.is_alphanumeric() {
            if !in_run {
                out.push_str("<v>");
            }
            in_run = true;
        } else {
            out.push(ch);
            in_run = false;
        }
    }
    out
}

/// workspace の形そのものが読めないときの Report。
fn blocked(reason: &str) -> Report {
    Report {
        violations: vec![format!("layout: {reason}")],
        summary: String::new(),
    }
}

/// tag ごとの測定結果を 1 つの Report へ畳む。
fn fold(measured: Vec<Measured>) -> Report {
    let mut violations = Vec::new();
    let mut facts = Vec::new();
    for item in measured {
        violations.extend(item.violations);
        facts.push(item.fact);
    }
    Report {
        summary: format!("{SUMMARY_PREFIX} {}", facts.join(" ")),
        violations,
    }
}

/// 測れなかった tag を違反として立てる。
pub(crate) fn failed(tag: &str, reason: &str) -> Measured {
    Measured {
        fact: format!("{tag}=?"),
        violations: vec![format!("{tag}: {reason}")],
    }
}

#[cfg(test)]
#[path = "check_tests.rs"]
pub(crate) mod tests;
