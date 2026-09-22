//! 審査の材料の 4 本目: 契約の write-set の各項目の base の要約（設計 docs/design/contract-source.md §40・行 ao・
//! `s2-07l.431`）。
//!
//! lens は shell も cargo も撃てないので、既存の file を触る契約の「base の中身がこうだから done が測れる」を読めず、
//! `section-material-missing` の INCONCLUSIVE が往復する。器が base から項目ごとに path・行数の 2 面（全体 / 本体）・
//! 本体の区間の宣言の名・歯の区間の歯の名を測り、材料の dir に [`super::BASE_FILE`] として置く（組むのは
//! [`base_text`] の 1 本・`materials` から 1 回だけ呼ぶ）。区間の読み手は `pipe::closure` の [`src_region`] /
//! [`test_region`]、行数の 2 面は [`FileLines::of`] で、2 本目の読み手を作らない（C2）。
//!
//! 項目の形は 4 つ: `.rs` は行数と宣言の列と歯の列（別の列）・`.rs` でない file は行数だけ・`+` の項目は「新設」の 1 行・
//! 読めない項目は読めなさの 1 行（黙って落とさない・C10）。`-` / `~` の項目は接頭辞を剥がした base の file を読む。
//!
//! cap は新しい閾値を作らない: lens が [`base_block`] で既存の `gate.token_cap` の残りに収まるかを測り、収まらない周は
//! 段ごと落として落とした項目の本数の 1 行を残す（既存の 4 材料だけで越える周の INCONCLUSIVE は lens の側で不変）。

use crate::pipe::closure::{src_region, test_region, TEST_ATTR};
use crate::pipe::declaration::FileLines;
use crate::pipe::refuse::{DELETE_FILE, NEW_FILE, SHRINK_FILE};
use crate::pipe::table;
use std::path::{Component, Path};

/// 行数の幅を持つ rules 行（受付の上限の余地が数える行数と同じ式にする・rules-manifest.md §4）。
const ROW_LINE_WIDTH: &str = "R-C4.line-width";

/// 要約の 1 項目の書き出し（[`base_block`] が落とした本数をこの頭の行で数える）。
const ITEM_HEAD: &str = "- ";

/// 宣言の語（本体の区間でこの語から始まる行が宣言・`impl` / `use` / `let` は名を持つ宣言として数えない）。
const DECL_KEYWORDS: &[&str] = &["fn", "struct", "enum", "union", "trait", "type", "const", "static", "mod"];

/// 宣言の語の前に来てよい修飾の語（`pub(crate)` 等の括弧つきは [`declared_name`] が 1 語として飛ばす）。
const QUALIFIERS: &[&str] = &["pub", "async", "unsafe", "extern", "\"C\"", "default"];

/// `{base}` の穴の見出し（段を落とした周も見出しは残す）。
const HEADING: &str = "\n## write-set の base の要約（器が base から測った事実）\n";

/// 見出しの下の説明の 1 行（段を落とさない周だけ）。
const PREAMBLE: &str =
    "契約の write-set の各項目について、base の行数（全体 / 本体＝最初の行頭 `#[cfg(test)]` より前）と、`.rs` は本体の区間の宣言の名と歯の区間の `#[test]` の fn の名を別の列で並べる。`+` の項目は base に無い新設の file。";

/// 材料の本文を組む（`materials` から 1 回だけ呼ぶ）。幅の rules 行を読めない周は理由の 1 行（C10）。
pub(super) fn base_text(repo: &Path, write_set: &[String]) -> String {
    match crate::seat::int_rule(ROW_LINE_WIDTH) {
        Ok(width) => summary(repo, write_set, width),
        Err(read) => format!("（base の要約を作れない: rules 行 {ROW_LINE_WIDTH} を読めない・{}）", read.as_str()),
    }
}

/// write-set の宣言順に 1 項目ずつ要約する。
fn summary(repo: &Path, write_set: &[String], width: u64) -> String {
    write_set.iter().map(|item| item_text(repo, item, width)).collect::<Vec<String>>().join("\n")
}

/// 1 項目の要約（行の頭は [`ITEM_HEAD`] と契約の字面のままの項目）。
fn item_text(repo: &Path, item: &str, width: u64) -> String {
    if item.starts_with(NEW_FILE) {
        return format!("{ITEM_HEAD}{item}: 新設（base に無い）");
    }
    let path = item.strip_prefix([SHRINK_FILE, DELETE_FILE]).unwrap_or(item);
    if !inside(path) {
        return format!("{ITEM_HEAD}{item}: 読めない（repo の外を指す path）");
    }
    let text = match table::read(repo, path) {
        Ok(found) => found,
        Err(reason) => return format!("{ITEM_HEAD}{item}: 読めない（{reason}）"),
    };
    let lines = FileLines::of(path, &text, width);
    let head = format!("{ITEM_HEAD}{item}: 行数 全体 {} / 本体 {}", lines.total, lines.src);
    if !path.ends_with(".rs") {
        return head;
    }
    let decls: Vec<String> = src_region(&text).lines().filter_map(declared_name).collect();
    let teeth = tooth_names(test_region(path, &text));
    format!("{head}\n  宣言: {}\n  歯: {}", listed(&decls), listed(&teeth))
}

/// repo 相対の path か（絶対 path と `..` の段を持つ path は repo の外を読みうる）。
fn inside(path: &str) -> bool {
    !path.is_empty() && Path::new(path).components().all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
}

/// 名の列を 1 行に並べる（空は「なし」）。
fn listed<T: AsRef<str>>(names: &[T]) -> String {
    if names.is_empty() {
        "なし".to_owned()
    } else {
        names.iter().map(AsRef::as_ref).collect::<Vec<&str>>().join(", ")
    }
}

/// 宣言の行の `<語> <名>`（修飾の語を飛ばした最初の語が [`DECL_KEYWORDS`] で、次の語が識別子の行だけ）。`const fn` は
/// `fn` として読む。
fn declared_name(line: &str) -> Option<String> {
    let mut words = line.split_whitespace().filter(|word| !word.starts_with("pub("));
    let mut word = words.next()?;
    while QUALIFIERS.contains(&word) {
        word = words.next()?;
    }
    if !DECL_KEYWORDS.contains(&word) {
        return None;
    }
    let mut next = words.next()?;
    if word == "const" && QUALIFIERS.iter().chain(["fn"].iter()).any(|found| *found == next) {
        while next != "fn" {
            next = words.next()?;
        }
        word = next;
        next = words.next()?;
    }
    let name = next.split(|found: char| !(found.is_alphanumeric() || found == '_')).next()?;
    (!name.is_empty()).then(|| format!("{word} {name}"))
}

/// 歯の区間の `#[test]` の直下の `fn` の名（属性行・doc・空行は跨ぐ・他の行が先に来れば歯ではない）。
fn tooth_names(region: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = false;
    for line in region.lines() {
        let trimmed = line.trim();
        if trimmed == TEST_ATTR {
            pending = true;
        } else if pending && !(trimmed.is_empty() || trimmed.starts_with("#[") || trimmed.starts_with("//")) {
            pending = false;
            found.extend(declared_name(trimmed).and_then(|decl| decl.strip_prefix("fn ").map(str::to_owned)));
        }
    }
    found
}

/// `{base}` の穴の本文（lens が埋める）: 写しが空なら空文字（雛形は 1 字も変わらない）・見出しと説明と要約が `room`
/// byte に収まれば全部・収まらなければ見出しと落とした項目の本数の 1 行（段ごと落とす・既存 cap の残りで測る）。
pub fn base_block(summary: &str, room: u64) -> String {
    let summary = summary.trim_end();
    if summary.is_empty() {
        return String::new();
    }
    let full = format!("{HEADING}{PREAMBLE}\n\n{summary}");
    if u64::try_from(full.len()).unwrap_or(u64::MAX) <= room {
        return full;
    }
    let dropped = summary.lines().filter(|line| line.starts_with(ITEM_HEAD)).count();
    format!("{HEADING}（要約を足すと cap を越えるので段ごと落とした: 項目 {dropped} 本）")
}

#[cfg(test)]
mod tests {
    use super::{base_block, summary};
    use std::path::PathBuf;

    /// 歯ごとの空の tmp dir。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pipe-review-base-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(dir.join("src"));
        dir
    }

    /// 本体の宣言 5 本と歯 2 本（helper の fn と `mod tests` は歯の列に出ない）。
    const FIXTURE: &str = "//! doc\nuse std::fmt;\n\npub(crate) const fn width() -> u8 {\n    1\n}\n\npub struct Shape {\n    x: u8,\n}\n\nenum Tone {\n    A,\n}\n\nimpl Shape {\n    pub fn dot() -> Self {\n        Self { x: 0 }\n    }\n}\n\nconst LIMIT: u8 = 3;\n\n#[cfg(test)]\nmod tests {\n    fn helper() {}\n\n    /// 1 本目。\n    #[test]\n    fn shape_one() {}\n\n    #[test]\n    #[ignore]\n    fn shape_two() {}\n}\n";

    /// (a) `.rs` 1 本で本体の宣言の名と歯の名が別の列に出る（母集団 = 宣言 5 本と歯 2 本を同じ assert で数える）。
    #[test]
    fn pipe_review_base_rs_lists_declarations_and_teeth_in_separate_columns() {
        let repo = scratch("rs");
        let _ = std::fs::write(repo.join("src/a.rs"), FIXTURE);
        let text = summary(&repo, &["src/a.rs".to_owned()], 120);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.first(), Some(&"- src/a.rs: 行数 全体 35 / 本体 23"), "{text}");
        let column = |head: &str| -> Vec<String> {
            lines
                .iter()
                .find_map(|line| line.strip_prefix(head))
                .map(|rest| rest.split(", ").map(str::to_owned).collect())
                .unwrap_or_default()
        };
        let (decls, teeth) = (column("  宣言: "), column("  歯: "));
        assert_eq!(
            (decls.len(), teeth.len(), lines.len()),
            (5, 2, 3),
            "宣言 5 本・歯 2 本・行は項目 + 2 列: {text}"
        );
        assert_eq!(decls, ["fn width", "struct Shape", "enum Tone", "fn dot", "const LIMIT"], "本体の区間だけ");
        assert_eq!(teeth, ["shape_one", "shape_two"], "helper と mod は歯でない");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// (b) `.rs` でない項目は path と行数だけ（宣言と歯の列を持たない）。
    #[test]
    fn pipe_review_base_non_rs_carries_path_and_lines_only() {
        let repo = scratch("md");
        let _ = std::fs::write(repo.join("notes.md"), "# t\n\nfn not_rust() {}\n#[test]\n");
        let text = summary(&repo, &["notes.md".to_owned()], 120);
        assert_eq!(text, "- notes.md: 行数 全体 4 / 本体 4", "1 行だけ");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// (c) `+` の項目は base を読まず新設の 1 行（同名の file が worktree に在っても読まない）・(d) 読めない項目は
    /// 読めなさの 1 行（無い file・dir・repo の外）。項目の本数は write-set の本数と同じ（黙って落とさない）。
    #[test]
    fn pipe_review_base_new_and_unreadable_items_are_one_line_each() {
        let repo = scratch("odd");
        let _ = std::fs::write(repo.join("src/fresh.rs"), FIXTURE);
        let items: Vec<String> =
            ["+src/fresh.rs", "src/absent.rs", "src/", "../outside.rs", "~src/gone.rs"].iter().map(|item| (*item).to_owned()).collect();
        let text = summary(&repo, &items, 120);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), items.len(), "1 項目 1 行: {text}");
        assert_eq!(lines.first(), Some(&"- +src/fresh.rs: 新設（base に無い）"));
        assert!(lines.get(1).is_some_and(|line| line.starts_with("- src/absent.rs: 読めない（src/absent.rs を読めない: ")), "{text}");
        assert!(lines.get(2).is_some_and(|line| line.starts_with("- src/: 読めない（")), "{text}");
        assert_eq!(lines.get(3), Some(&"- ../outside.rs: 読めない（repo の外を指す path）"));
        assert!(lines.get(4).is_some_and(|line| line.starts_with("- ~src/gone.rs: 読めない（src/gone.rs を読めない: ")), "{text}");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// (e) 収まる周は見出しと説明と要約の全部・収まらない周は段ごと落として落とした項目の本数の 1 行だけが残る
    /// （要約の本文は 1 字も残らない）・空の写しは空文字。
    #[test]
    fn pipe_review_base_block_drops_the_whole_stage_over_cap_and_names_the_count() {
        let summary = "- src/a.rs: 行数 全体 3 / 本体 3\n  宣言: fn a\n  歯: なし\n- docs/b.md: 行数 全体 1 / 本体 1\n- +src/c.rs: 新設（base に無い）\n";
        let full = base_block(summary, u64::MAX);
        assert!(full.ends_with(summary.trim_end()) && full.starts_with("\n## "), "{full}");
        let room = u64::try_from(full.len()).unwrap_or(u64::MAX);
        assert_eq!(base_block(summary, room), full, "ちょうど収まる周は落とさない");
        let dropped = base_block(summary, room.saturating_sub(1));
        assert!(dropped.ends_with("（要約を足すと cap を越えるので段ごと落とした: 項目 3 本）"), "{dropped}");
        assert!(!dropped.contains("src/a.rs") && !dropped.contains("宣言"), "要約の本文は残らない: {dropped}");
        assert_eq!(dropped.lines().filter(|line| !line.is_empty()).count(), 2, "見出しと本数の 1 行: {dropped}");
        assert_eq!(base_block("\n", u64::MAX), "", "空の写しは空文字");
    }
}
