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
//! 読めない項目は読めなさの 1 行（黙って落とさない・C10）。`-` / `~` / `=` の項目は接頭辞を剥がした base の file を読み
//! （剥がすのは `pipe::refuse` の [`normalize`] の 1 本・§44）、`=` の項目は行数の後ろに置き場だけの 1 語を添える。
//!
//! cap は新しい閾値を作らない: lens が [`base_block`] で既存の `gate.token_cap` の残りに収まるかを測り、収まらない周は
//! 段ごと落として落とした項目の本数の 1 行を残す（既存の 4 材料だけで越える周の INCONCLUSIVE は lens の側で不変）。

use crate::pipe::closure::{src_region, test_region, TEST_ATTR};
use crate::pipe::declaration::FileLines;
use crate::pipe::refuse::{normalize, NEW_FILE, PLACE_ONLY_FILE};
use crate::pipe::table;
use std::path::{Component, Path};

/// 行数の幅を持つ rules 行（受付の上限の余地が数える行数と同じ式にする・rules-manifest.md §4）。
pub(super) const ROW_LINE_WIDTH: &str = "R-C4.line-width";

/// 要約の 1 項目の書き出し（[`base_block`] が落とした本数をこの頭の行で数える・外の材料〔§51〕の塊の頭も同じ字面）。
pub(super) const ITEM_HEAD: &str = "- ";

/// 宣言の語（本体の区間でこの語から始まる行が宣言・`impl` / `use` / `let` は名を持つ宣言として数えない）。
const DECL_KEYWORDS: &[&str] = &["fn", "struct", "enum", "union", "trait", "type", "const", "static", "mod"];

/// 宣言の語の前に来てよい修飾の語（`pub(crate)` / `pub(in <path>)` の括弧つきは [`declared_name`] が閉じ括弧まで 1 まとまりとして飛ばす）。
const QUALIFIERS: &[&str] = &["pub", "async", "unsafe", "extern", "\"C\"", "default"];

/// 置き場だけの印（`=`）の項目の行数の後ろに添える 1 語（lens が印の意味を要約から読める・§44）。
const PLACE_ONLY_NOTE: &str = "・置き場だけ（中身は変えない）";

/// `{base}` の穴の見出し（段を落とした周も見出しは残す）。
const HEADING: &str = "\n## write-set の base の要約（器が base から測った事実）\n";

/// 見出しの下の説明の 1 行（段を落とさない周だけ）。
const PREAMBLE: &str =
    "write-set の各項目の base の行数（全体 / 本体＝最初の行頭 `#[cfg(test)]` より前）と、`.rs` は本体の宣言の名と歯の `#[test]` の fn の名を別の列で並べる（`+` は新設）。行数は字数 ÷ 幅の切り上げ（最小 1・受付の上限の余地と同じ）で、生の行と違う file は括弧に生の行。宣言は可視性の字（`pub` / `pub(crate)` / `pub(super)` / `pub(in …)`）を前に持ち、字の無い宣言と欄は私有（その module と子孫から見える）。私有でない struct は名の後ろの括弧の中に欄の可視性と名を `; ` で区切って並べ（tuple の欄の名は位置の番号・型は渡さない）、trait の impl の中の fn は字を持たず trait に従う。";

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

/// 1 項目の要約（行の頭は [`ITEM_HEAD`] と契約の字面のままの項目）。外の材料（§51 形 3 (b)）の名指された `.rs` も同じ 1 本。
pub(super) fn item_text(repo: &Path, item: &str, width: u64) -> String {
    if item.starts_with(NEW_FILE) {
        return format!("{ITEM_HEAD}{item}: 新設（base に無い）");
    }
    // 外の断りは字面（絶対 path・`..` の段）と剥がして畳んだ path（`=../x` 等）の両方で測る。
    let path = normalize(item);
    if !(inside(item) && inside(&path)) {
        return format!("{ITEM_HEAD}{item}: 読めない（repo の外を指す path）");
    }
    let text = match table::read(repo, &path) {
        Ok(found) => found,
        Err(reason) => return format!("{ITEM_HEAD}{item}: 読めない（{reason}）"),
    };
    let lines = FileLines::of(&path, &text, width);
    // 畳んだ数と生の行が違う file だけ生の行を括弧で添える（等しい file の見出しは不変・§55 形 8）。
    let raw = text.lines().count();
    let folded = if u64::try_from(raw).ok() == Some(lines.total) { String::new() } else { format!("（幅 {width} で畳んだ数・生の行 {raw}）") };
    let space = if folded.is_empty() { " " } else { "" };
    let mut head = format!("{ITEM_HEAD}{item}: 行数 全体 {}{folded}{space}/ 本体 {}", lines.total, lines.src);
    if item.starts_with(PLACE_ONLY_FILE) {
        head.push_str(PLACE_ONLY_NOTE);
    }
    if !path.ends_with(".rs") {
        return head;
    }
    let src: Vec<&str> = src_region(&text).lines().collect();
    let decls: Vec<String> = src.iter().enumerate().filter_map(|(at, line)| listed_item(&src, at, line)).collect();
    let teeth = tooth_names(test_region(&path, &text));
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
/// `fn` として読む。括弧つきの可視性（`pub(crate)` / `pub(in crate::pipe)`）は閉じ括弧を含む語までを 1 まとまりとして
/// 飛ばし、閉じ括弧の無い行は宣言と読まない（§49）。外の材料（§51）の候補の名と所在もこの 1 本で読む。
pub(super) fn declared_name(line: &str) -> Option<String> {
    declared(line).map(|(_, decl)| decl)
}

/// 宣言の行の可視性の字面（行頭の `pub` / `pub(…)`・無ければ空）と [`declared_name`] の `<語> <名>`（宣言の語の切りは
/// この 1 本・§56）。
fn declared(line: &str) -> Option<(String, String)> {
    let mut kept = Vec::new();
    let mut visibility = String::new();
    let mut raw = line.split_whitespace();
    let mut first = true;
    while let Some(word) = raw.next() {
        if word.starts_with("pub(") {
            let mut group = vec![word];
            let mut closing = word;
            while !closing.contains(')') {
                closing = raw.next()?;
                group.push(closing);
            }
            if first {
                visibility = group.join(" ");
            }
        } else {
            if first && word == "pub" {
                visibility = word.to_owned();
            }
            kept.push(word);
        }
        first = false;
    }
    let mut words = kept.into_iter();
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
    (!name.is_empty()).then(|| (visibility, format!("{word} {name}")))
}

/// 宣言の列の 1 項目（`<可視性> <語> <名>`・私有は字なし・私有でない struct は名の後ろに [`fields`] の列）。
fn listed_item(lines: &[&str], at: usize, line: &str) -> Option<String> {
    let (visibility, decl) = declared(line)?;
    if visibility.is_empty() {
        return Some(decl);
    }
    let fields = if decl.starts_with("struct ") { fields(lines, at) } else { String::new() };
    Some(format!("{visibility} {decl}{fields}"))
}

/// struct の欄の列（名前つきは ` { <可視性> <欄の名>; … }`・tuple は `(<可視性> 0; …)`・欄が無ければ空・型は渡さない）。
/// 宣言の行から [`block_end`] までの doc 行・注釈・属性の行を除き、各行の行末の `//` から後ろを落として繋いで読む。名の
/// 直後（generic の `<…>` は飛ばす）が `(` なら tuple、そうでなければ最初の `{` の中（`{` より先に `;` が来れば欄なし）。
fn fields(lines: &[&str], at: usize) -> String {
    let kept: Vec<&str> = lines
        .get(at..=block_end(lines, at))
        .unwrap_or_default()
        .iter()
        .map(|line| line.trim())
        .filter(|line| !(line.starts_with("//") || line.starts_with("#[")))
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect();
    let joined = kept.join(" ");
    let after = joined.split_once("struct ").map_or("", |(_, after)| after);
    let mut rest = after.trim_start().trim_start_matches(|found: char| found.is_alphanumeric() || found == '_').trim_start();
    if let Some(generic) = rest.strip_prefix('<') {
        rest = generic.get(group(generic).1.saturating_add(1)..).unwrap_or_default().trim_start();
    }
    let (tuple, pieces) = if let Some(inner) = rest.strip_prefix('(') {
        (true, group(inner).0)
    } else {
        match (rest.find('{'), rest.find(';')) {
            (Some(brace), semi) if semi.is_none_or(|end| brace < end) => (false, group(rest.get(brace.saturating_add(1)..).unwrap_or_default()).0),
            _ => return String::new(),
        }
    };
    let named: Vec<String> =
        pieces.iter().map(|piece| piece.trim()).filter(|piece| !piece.is_empty()).enumerate().map(|(index, piece)| field(piece, index, tuple)).collect();
    match (named.is_empty(), tuple) {
        (true, _) => String::new(),
        (false, true) => format!("({})", named.join("; ")),
        (false, false) => format!(" {{ {} }}", named.join("; ")),
    }
}

/// 欄 1 つの `<可視性> <欄の名>`（私有は名だけ・tuple の名は位置の番号）。
fn field(piece: &str, index: usize, tuple: bool) -> String {
    let (visibility, rest) = match piece.find(')').filter(|_| piece.starts_with("pub(")) {
        Some(end) => piece.split_at_checked(end.saturating_add(1)).unwrap_or((piece, "")),
        None => piece.strip_prefix("pub ").map_or(("", piece), |rest| ("pub", rest)),
    };
    let name = if tuple { index.to_string() } else { rest.split(':').next().unwrap_or_default().trim().to_owned() };
    if visibility.is_empty() {
        name
    } else {
        format!("{visibility} {name}")
    }
}

/// 開き括弧の直後からの字面を、同じ深さの閉じ括弧の前まで深さ 0 の `,` で割る（`(` `[` `{` `<` で深く・閉じで浅く・
/// `->` の `>` は数えない）。返すのは割った欄と閉じ括弧の byte 位置（閉じなければ末尾）。
fn group(text: &str) -> (Vec<&str>, usize) {
    let mut depth = 0_usize;
    let mut pieces = Vec::new();
    let mut start = 0_usize;
    let mut previous = ' ';
    for (at, letter) in text.char_indices() {
        match letter {
            '(' | '[' | '{' | '<' => depth = depth.saturating_add(1),
            '>' if previous == '-' => {}
            ')' | ']' | '}' | '>' if depth == 0 => {
                pieces.push(text.get(start..at).unwrap_or_default());
                return (pieces, at);
            }
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                pieces.push(text.get(start..at).unwrap_or_default());
                start = at.saturating_add(1);
            }
            _ => {}
        }
        previous = letter;
    }
    pieces.push(text.get(start..).unwrap_or_default());
    (pieces, text.len())
}

/// 宣言の行から本体の閉じ括弧の行まで（括弧を開かず `;` で終わる行はその行・閉じなければ file の末尾）。外の材料の item
/// の行（§51）も同じ 1 本で数える。
pub(super) fn block_end(lines: &[&str], at: usize) -> usize {
    let mut depth = 0_usize;
    let mut opened = false;
    for (index, line) in lines.iter().enumerate().skip(at) {
        let opens = line.matches('{').count();
        depth = depth.saturating_add(opens).saturating_sub(line.matches('}').count());
        opened = opened || opens > 0;
        if (opened && depth == 0) || (!opened && line.trim_end().ends_with(';')) {
            return index;
        }
    }
    lines.len().saturating_sub(1)
}

/// 歯の区間の `#[test]` の直下の `fn` の名（属性行・doc・空行は跨ぐ・他の行が先に来れば歯ではない・§51 の候補も同じ 1 本）。
pub(super) fn tooth_names(region: &str) -> Vec<String> {
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
        assert_eq!(decls, ["pub(crate) fn width", "pub struct Shape { x }", "enum Tone", "pub fn dot", "const LIMIT"], "本体の区間だけ");
        assert_eq!(teeth, ["shape_one", "shape_two"], "helper と mod は歯でない");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// 1 本の `.rs` を要約し、宣言の列の行を返す（列が無ければ空）。
    fn declared_column(name: &str, body: &str) -> String {
        let repo = scratch(name);
        let _ = std::fs::write(repo.join("src/v.rs"), body);
        let text = summary(&repo, &["src/v.rs".to_owned()], 120);
        let _ = std::fs::remove_dir_all(&repo);
        text.lines().find_map(|line| line.strip_prefix("  宣言: ")).unwrap_or_default().to_owned()
    }

    /// §49 (1) 正例: `pub(in <path>)` の可視性は閉じ括弧まで 1 まとまりとして飛ばし、struct / fn / const の 3 形が
    /// 宣言の列に載る（母集団 = 3 本を同じ assert で数える）。
    #[test]
    fn pipe_review_base_pub_in_path_visibility_declarations_are_listed() {
        let body = "pub(in crate::pipe) struct Materials {\n    x: u8,\n}\n\npub(in crate::pipe) fn design_material() -> u8 {\n    1\n}\n\npub(in super::super) const EDGE: u8 = 2;\n";
        assert_eq!(
            declared_column("pub-in-yes", body),
            "pub(in crate::pipe) struct Materials { x }, pub(in crate::pipe) fn design_material, pub(in super::super) const EDGE"
        );
    }

    /// §49 (1) 負例: 閉じ括弧の無い `pub(in` の行は宣言と読まない（次の 2 語目で切り上げない・後続の正しい行は載る）。
    #[test]
    fn pipe_review_base_pub_in_without_closing_paren_is_not_a_declaration() {
        let body = "pub(in crate::pipe struct Broken {\n}\n\npub(in crate::pipe) struct Kept;\n";
        assert_eq!(declared_column("pub-in-no", body), "pub(in crate::pipe) struct Kept");
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

    /// §44: `+` / `-` / `~` / `=` の 4 形を同じ木で要約する（母集団 = 4 形の行数を同じ assert で数える）。`=` の項目は
    /// 読めないを持たず、行の頭は `=` を含む字面のまま・行数の後ろに置き場だけの 1 語・宣言と歯の 2 列を持つ。`+` の
    /// 1 行と `-` / `~` の行は 1 字も変わらない。
    #[test]
    fn pipe_review_base_place_only_item_is_read_and_marked_while_other_forms_stay() {
        let repo = scratch("place");
        let _ = std::fs::write(repo.join("src/a.rs"), FIXTURE);
        let items: Vec<String> =
            ["+src/fresh.rs", "-src/a.rs", "~src/a.rs", "=src/a.rs"].iter().map(|item| (*item).to_owned()).collect();
        let text = summary(&repo, &items, 120);
        let lines: Vec<&str> = text.lines().collect();
        let decls = "  宣言: pub(crate) fn width, pub struct Shape { x }, enum Tone, pub fn dot, const LIMIT";
        let teeth = "  歯: shape_one, shape_two";
        assert_eq!(
            lines,
            [
                "- +src/fresh.rs: 新設（base に無い）",
                "- -src/a.rs: 行数 全体 35 / 本体 23",
                decls,
                teeth,
                "- ~src/a.rs: 行数 全体 35 / 本体 23",
                decls,
                teeth,
                "- =src/a.rs: 行数 全体 35 / 本体 23・置き場だけ（中身は変えない）",
                decls,
                teeth,
            ],
            "4 形 = 1 + 3 + 3 + 3 行: {text}"
        );
        assert!(!text.contains("読めない"), "{text}");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// §44: `=` の `.rs` でない項目も本文を読んで行数と置き場だけの 1 語を持ち、`=` の後が repo の外なら従来の断り。
    #[test]
    fn pipe_review_base_place_only_non_rs_and_outside_items() {
        let repo = scratch("place-odd");
        let _ = std::fs::write(repo.join("notes.md"), "# t\n\nx\n");
        let items: Vec<String> = ["=notes.md", "=../outside.rs"].iter().map(|item| (*item).to_owned()).collect();
        let text = summary(&repo, &items, 120);
        assert_eq!(
            text.lines().collect::<Vec<&str>>(),
            ["- =notes.md: 行数 全体 3 / 本体 3・置き場だけ（中身は変えない）", "- =../outside.rs: 読めない（repo の外を指す path）"],
            "{text}"
        );
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

    /// §55 形 7 / 8: 幅 120 を越える 250 字の行を持つ `.rs` の見出しは全体の畳んだ数の直後に幅と生の行の括弧を持ち、
    /// 越えない `.rs` の見出しは不変（母集団 = 2 項目の見出しを同じ assert で数える）。base の説明の 1 行は畳む式と括弧の
    /// 意味を、外の材料の説明の 1 行は生の行だけを名乗る（畳んだ数の句は持たない）。
    #[test]
    fn review_base_lines_folded_count_names_the_raw_lines_only_when_they_differ() {
        let repo = scratch("lines");
        let _ = std::fs::write(repo.join("src/a.rs"), FIXTURE);
        let _ = std::fs::write(repo.join("src/w.rs"), format!("//! w\n// {}\npub fn wide() {{}}\n", "y".repeat(247)));
        let text = summary(&repo, &["src/w.rs".to_owned(), "src/a.rs".to_owned()], 120);
        let heads: Vec<&str> = text.lines().filter(|line| line.starts_with("- ")).collect();
        assert_eq!(
            heads,
            ["- src/w.rs: 行数 全体 5（幅 120 で畳んだ数・生の行 3）/ 本体 5", "- src/a.rs: 行数 全体 35 / 本体 23"],
            "{text}"
        );
        let preamble = |block: String| block.lines().find(|line| !line.is_empty() && !line.starts_with("## ")).map(str::to_owned);
        let base = preamble(base_block(&text, u64::MAX)).unwrap_or_default();
        assert!(base.contains("字数 ÷ 幅の切り上げ（最小 1・受付の上限の余地と同じ）") && base.contains("生の行と違う file は括弧に生の行"), "{base}");
        let outside = preamble(super::super::outside::outside_block("- x.json: 行数 1 / byte 2", u64::MAX)).unwrap_or_default();
        assert!(outside.contains("生の行（wc -l と同じ）") && !outside.contains("幅で畳んだ数"), "{outside}");
        let _ = std::fs::remove_dir_all(&repo);
    }
}
