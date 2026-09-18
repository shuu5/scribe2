//! seat-brief（席の指示文の雛形）の measure（設計 seat-roles.md §5・憲法 C14.2・AC17）。
//!
//! `check.rs` から**挙動不変で切り出した**（`s2-07l.257`・憲法 C4）。fact の字面 `seat-brief=…`
//! と違反行の形は不変で、`check::inspect` の measure の列の同じ位置から [`measure`] を呼ぶ。
//! 共通の fixture helper は `check::tests`（`check_tests.rs`）に在る。

use crate::check::{failed, Layout, Measured, RULES_REL};
use crate::toml_lite::{quoted, string_array};
use crate::workspace::{read_dir_sorted, read_text};
use std::path::PathBuf;

/// seat-brief（設計 seat-roles.md §5・憲法 C14.2・AC17）の tag。
const BRIEF_TAG: &str = "seat-brief";
/// 席の指示文の雛形の置き場（core の `src` 相対・`<役割名>.txt` が 1 枚ずつ）。
const BRIEF_DIR: &str = "seat/brief";
/// 権能の穴（rules 行の値の列で埋める）。
const BRIEF_CAPABILITIES: &str = "{capabilities}";
/// 雛形の定義済みの穴（core の `seat::brief::HOLES` と同じ列＝自 workspace の check が drift を捕まえる）。
const BRIEF_HOLES: &[&str] = &[BRIEF_CAPABILITIES, "{target}", "{anchor}", "{role}", "{ledger}"];
/// 雛形の行の出所 pointer の区切り（退避物の命令行と同じ字面・この後ろの参照だけを pointer と読む）。
const BRIEF_SSOT: &str = "→ SSOT:";
/// 役割の rules 行 id の前置き（`role.<役割名>`）。
const ROLE_ROW: &str = "role.";

/// 席の指示文の雛形を測る（C14.2・AC17）: (i) 穴 ⊆ 定義済み・(ii) pointer を持たない行 0（空行と穴だけの行を除く）・
/// (iii) rules 行 `role.<役割>` の値に在って生成文に無い権能 0。雛形 0 枚・行の無い役割・雛形の無い行も違反（fail-closed）。
pub(crate) fn measure(layout: &Layout) -> Measured {
    let dir = layout.core_dir.join("src").join(BRIEF_DIR);
    let (manifest, entries) = match (read_text(&layout.root.join(RULES_REL)), read_dir_sorted(&dir)) {
        (Ok(manifest), Ok(entries)) => (manifest, entries),
        (Err(reason), _) | (_, Err(reason)) => return failed(BRIEF_TAG, &reason),
    };
    let rows = role_rows(&manifest);
    let stem = |path: &PathBuf| path.file_stem().map(|found| found.to_string_lossy().into_owned()).unwrap_or_default();
    let txt = entries.iter().filter(|path| path.extension().is_some_and(|ext| ext == "txt"));
    let templates: Vec<(String, &PathBuf)> = txt.map(|path| (stem(path), path)).collect();
    let orphan = rows.iter().filter(|(role, _)| !templates.iter().any(|(name, _)| name == role));
    let missing = |role: &String| format!("{BRIEF_TAG}: rules 行 {ROLE_ROW}{role} の雛形 {}/{role}.txt が無い", dir.display());
    let mut violations: Vec<String> = orphan.map(|(role, _)| missing(role)).collect();
    if templates.is_empty() {
        violations.push(format!("{BRIEF_TAG}: {} に雛形が 0 枚である", dir.display()));
    }
    for (role, path) in &templates {
        let rel = path.strip_prefix(&layout.root).unwrap_or(path).display().to_string();
        let Ok(text) = read_text(path).map_err(|reason| violations.push(format!("{BRIEF_TAG}: {reason}"))) else {
            continue;
        };
        violations.extend(brief_line_violations(&rel, &text));
        match rows.iter().find(|(name, _)| name == role) {
            None => violations.push(format!("{BRIEF_TAG}: {rel}: rules 行 {ROLE_ROW}{role} が無い")),
            Some((_, names)) => violations.extend(brief_missing_capabilities(&rel, &text, names)),
        }
    }
    let fact = if violations.is_empty() { format!("{BRIEF_TAG}=ok") } else { format!("{BRIEF_TAG}={}", violations.len()) };
    Measured { fact, violations }
}

/// manifest の `role.<役割>` の行（役割名と値の列・出現順・`enabled` は見ない＝不発効の行も雛形を要る）。
fn role_rows(manifest: &str) -> Vec<(String, Vec<String>)> {
    let rows = crate::toml_lite::sections(manifest).into_iter().filter(|(header, _)| *header == "[rule");
    rows.filter_map(|(_, pairs)| {
        let field = |key: &str| pairs.iter().find(|(found, _)| *found == key).map(|(_, value)| *value);
        let id = field("id").and_then(quoted)?;
        let role = id.strip_prefix(ROLE_ROW)?.to_owned();
        Some((role, field("value").map(string_array).unwrap_or_default()))
    })
    .collect()
}

/// 雛形の行の違反（(i) 未知の穴・(ii) pointer を持たない行・`<rel>:<line>` 付き）。空行と穴だけの行は母集団の外で、
/// 未知の穴は pointer の有無より先に見る（core の `seat::brief::classify_line` と同じ順）。
fn brief_line_violations(rel: &str, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at, line) in text.lines().enumerate() {
        let stripped = BRIEF_HOLES.iter().fold(line.to_owned(), |text, hole| text.replace(hole, ""));
        let brace = stripped.find('{').and_then(|open| stripped.get(open..));
        let unknown = brace.and_then(|tail| tail.get(..=tail.find('}')?));
        let tail = line.rsplit_once(BRIEF_SSOT).map(|(_, tail)| tail);
        if let Some(hole) = unknown {
            found.push(format!("{BRIEF_TAG}: {rel}:{}: unknown-hole {hole}", at.saturating_add(1)));
        } else if !stripped.trim().is_empty() && tail.is_none_or(|tail| crate::prose_gate::pointer_spans(tail).is_empty()) {
            let head: String = line.trim().chars().take(40).collect();
            found.push(format!("{BRIEF_TAG}: {rel}:{}: no-pointer {head}", at.saturating_add(1)));
        }
    }
    found
}

/// (iii) 権能の穴を rules 行の値で埋めた生成文に現れない権能の名（`<rel>` 付き）。
fn brief_missing_capabilities(rel: &str, text: &str, names: &[String]) -> Vec<String> {
    let rendered = text.replace(BRIEF_CAPABILITIES, &names.join("・"));
    let dropped = names.iter().filter(|name| !rendered.contains(name.as_str()));
    dropped.map(|name| format!("{BRIEF_TAG}: {rel}: 権能 {name} が生成文に無い（rules 行の値に在る）")).collect()
}

#[cfg(test)]
mod tests {
    // 純粋な移動（`check.rs` の test 区間から歯を足さずに写した・s2-07l.257）。
    // flip-check: moved s2-07l.257
    use crate::check::tests::{assert_single, brief_rel, check_fixture, summary_fixture, write_at};
    use std::fs;

    /// 雛形の 3 違反（AC17・C14.2・`s2-07l.248`）: pointer の無い行・未知の穴・権能を消した雛形はそれぞれ seat-brief
    /// だけで落ち、行の違反は file:line を名指す。雛形を消すと行の無い役割 + 雛形 0 枚の 2 件。健全な木は `seat-brief=ok`。
    #[test]
    fn seat_brief_rejects_bare_lines_unknown_holes_and_dropped_capabilities() {
        let bare = check_fixture(|dir| write_at(dir, &brief_rel(), "{capabilities}\n席は lock を確保する\n"));
        assert_single(&bare, "seat-brief");
        assert!(bare.first().is_some_and(|line| line.contains(&format!("{}:2: no-pointer", brief_rel()))), "file:line 付き: {bare:?}");
        let unknown = check_fixture(|dir| write_at(dir, &brief_rel(), "{capabilities} {model} → SSOT: N2\n"));
        assert_single(&unknown, "seat-brief");
        assert!(unknown.first().is_some_and(|line| line.contains(":1: unknown-hole {model}")), "{unknown:?}");
        let dropped = check_fixture(|dir| write_at(dir, &brief_rel(), "権能は answer だけ → SSOT: ADR-0022 §2.2\n"));
        assert_single(&dropped, "seat-brief");
        assert!(dropped.first().is_some_and(|line| line.contains("権能 merge が生成文に無い")), "{dropped:?}");
        let orphan = check_fixture(|dir| drop(fs::remove_file(dir.join(brief_rel()))));
        assert_eq!(orphan.iter().filter(|line| line.starts_with("seat-brief: ")).count(), 2, "{orphan:?}");
        let ok = summary_fixture(|_| {});
        assert!(ok.contains(" seat-brief=ok"), "健全な木: {ok}");
    }
}
