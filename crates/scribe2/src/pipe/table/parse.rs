//! 契約表の置き場の抜き出しと TOML subset の型付け（設計 docs/design/contract-source.md §15・SRS FR47 / FR54・pure）。
//!
//! `.md` は `<!-- contracts:begin -->` … `<!-- contracts:end -->` の区間を行走査で抜き、`.toml` は全文を [`read_rows`]
//! が rules manifest の 1 本の parser へ渡して、欄の形（[`super::FIELDS`] の `shape`）で [`super::ContractRow`] へ
//! 型付けする。設計 pointer（`<path>#<id>`）の読みと行 1 つの引き（[`find_row`]）も同じ群。findings の語彙
//! （[`super::TableError`] / `Finding` / `Context`）は親 module `table.rs`・表の検査は兄弟 `table/check.rs` に
//! 置いたまま。呼び手（`pipe/cli/intake.rs`・`pipe/review.rs`・歯）の `use` は親の再 export を通る。

use super::{unreadable, ContractRow, TableError, BEGIN, END};
use crate::rules::manifest::{contract_rows, Scalar, TableRow, TableValue};
use std::path::Path;

/// 契約表の置き場の形（path の拡張子で決める）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// 設計 doc（`.md`）の区間。
    Region,
    /// 導出された `.toml` の全文（folio2 M1 の形）。
    Whole,
}

/// path の拡張子から置き場の形を決める（`.md` = 区間 / `.toml` = 全文・他は typed に断る）。
pub fn form_of(path: &str) -> Result<Form, TableError> {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("md") => Ok(Form::Region),
        Some("toml") => Ok(Form::Whole),
        _ => Err(unreadable(0, &format!("{path} は契約表の置き場の形でない（.md = 区間 / .toml = 全文）"))),
    }
}

/// `.md` の区間（begin の行番号・本文）。区間が無ければ `None`。2 つ以上・閉じない・end だけは `Err`。
fn region(text: &str) -> Result<Option<(u64, String)>, TableError> {
    let (mut open, mut found, mut body) = (None, None, String::new());
    for (index, line) in text.lines().enumerate() {
        let at = (index as u64).saturating_add(1);
        match (line.trim(), open) {
            (BEGIN, Some(_)) => return Err(TableError::RegionDuplicate { line: at }),
            (BEGIN, None) if found.is_some() => return Err(TableError::RegionDuplicate { line: at }),
            (BEGIN, None) => open = Some(at),
            (END, Some(start)) => {
                found = Some((start, std::mem::take(&mut body)));
                open = None;
            }
            (END, None) => return Err(unreadable(at, "区間の end の前に begin が無い")),
            (_, Some(_)) => {
                body.push_str(line);
                body.push('\n');
            }
            (_, None) => {}
        }
    }
    match open {
        Some(start) => Err(unreadable(start, "区間が閉じていない（end が無い）")),
        None => Ok(found),
    }
}

/// 区間の本文の行番号を doc の行番号へ写す（0 = 本文全体 → 区間の始まりの行）。
fn shift(offset: u64, line: u64) -> u64 {
    if line == 0 {
        offset
    } else {
        offset.saturating_add(line)
    }
}

/// 契約表の置き場 1 本を読む（`.md` = 区間 / `.toml` = 全文）。区間の無い `.md` は 0 行（表なしは違反でない）。
/// 欠陥は**全件**・doc 上の行番号付きで返す。
pub fn read_rows(path: &str, text: &str) -> Result<Vec<ContractRow>, Vec<TableError>> {
    let (offset, body) = match form_of(path).map_err(|found| vec![found])? {
        Form::Whole => (0, text.to_owned()),
        Form::Region => match region(text).map_err(|found| vec![found])? {
            Some(found) => found,
            None => return Ok(Vec::new()),
        },
    };
    let raws = contract_rows(&body).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| unreadable(shift(offset, error.line), &error.message))
            .collect::<Vec<TableError>>()
    })?;
    let mut errors = Vec::new();
    let rows: Vec<ContractRow> = raws.iter().filter_map(|raw| typed(raw, offset, &mut errors)).collect();
    if errors.is_empty() {
        Ok(rows)
    } else {
        Err(errors)
    }
}

/// 読めた 1 行を欄の形（[`super::FIELDS`] の `shape`）で型付けする。形の違いは欄の行番号で積む。
fn typed(raw: &TableRow, offset: u64, errors: &mut Vec<TableError>) -> Option<ContractRow> {
    let before = errors.len();
    let row = ContractRow {
        line: shift(offset, raw.line()),
        id: text_of(raw, "id", offset, errors),
        title: text_of(raw, "title", offset, errors),
        req: list_of(raw, "req", offset, errors),
        section: text_of(raw, "section", offset, errors),
        touches: list_of(raw, "touches", offset, errors),
        surfaces: list_of(raw, "surfaces", offset, errors),
        write_set: list_of(raw, "write-set", offset, errors),
        creates: list_of(raw, "creates", offset, errors),
        tests: list_of(raw, "tests", offset, errors),
        also: list_of(raw, "also", offset, errors),
        verify: list_of(raw, "verify", offset, errors),
        size: text_of(raw, "size", offset, errors),
        done: text_of(raw, "done", offset, errors),
        depends: list_of(raw, "depends", offset, errors),
        classes: list_of(raw, "classes", offset, errors),
        opens: list_of(raw, "opens", offset, errors),
    };
    (errors.len() == before).then_some(row)
}

/// 文字列の欄（無ければ空・形が違えば積む）。
fn text_of(raw: &TableRow, key: &str, offset: u64, errors: &mut Vec<TableError>) -> String {
    match raw.value(key) {
        None => String::new(),
        Some((TableValue::One(Scalar::Str(found)), _)) if !found.trim().is_empty() => found.clone(),
        Some((_, line)) => {
            errors.push(unreadable(shift(offset, line), &format!("{key} は空でない文字列でなければならない")));
            String::new()
        }
    }
}

/// 配列の欄（無ければ空・形が違えば積む）。
fn list_of(raw: &TableRow, key: &str, offset: u64, errors: &mut Vec<TableError>) -> Vec<String> {
    match raw.value(key) {
        None => Vec::new(),
        Some((TableValue::List(items), _)) => items.clone(),
        Some((_, line)) => {
            errors.push(unreadable(shift(offset, line), &format!("{key} は文字列の配列でなければならない")));
            Vec::new()
        }
    }
}

/// 設計 pointer の行を 1 つ引く（intake が 1 行に撃つ口・契約 (b)）。区間の無い `.md` は
/// [`TableError::RegionMissing`]・id が無ければ [`TableError::RowMissing`]。
pub fn find_row(path: &str, text: &str, id: &str) -> Result<ContractRow, Vec<TableError>> {
    if form_of(path) == Ok(Form::Region) && region(text) == Ok(None) {
        return Err(vec![TableError::RegionMissing { line: 0 }]);
    }
    read_rows(path, text)?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or_else(|| vec![TableError::RowMissing { line: 0, id: id.to_owned() }])
}

/// 設計 pointer（`docs/design/<題>.md#<id>`）を読んだもの。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pointer {
    /// 置き場の repo 相対 path。
    pub path: String,
    /// 行 id。
    pub id: String,
}

/// 設計 pointer を断る理由（FR54）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerError {
    /// `#` がちょうど 1 つでない。
    Hash,
    /// 置き場の拡張子（`.md` / `.toml`）でない。
    Extension,
    /// 行 id が空か空白を含む。
    Id,
}

impl PointerError {
    /// 断る理由の 1 行。
    pub fn reason(self) -> &'static str {
        match self {
            Self::Hash => "設計 pointer は <path>#<id> の形（# はちょうど 1 つ）",
            Self::Extension => "設計 pointer の path は契約表の置き場（.md の区間 / .toml の全文）",
            Self::Id => "設計 pointer の行 id が空か空白を含む",
        }
    }
}

/// 設計 pointer を読む（拡張子・`#` の 1 つ・空でない id）。
pub fn parse_pointer(text: &str) -> Result<Pointer, PointerError> {
    let mut parts = text.split('#');
    let (Some(path), Some(id), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(PointerError::Hash);
    };
    if form_of(path).is_err() || doc_id(path).is_empty() {
        return Err(PointerError::Extension);
    }
    if id.is_empty() || id.chars().any(char::is_whitespace) {
        return Err(PointerError::Id);
    }
    Ok(Pointer { path: path.to_owned(), id: id.to_owned() })
}

/// 契約 id（`<doc の file 名の stem>#<行 id>`・folio2 の設計ノートと同じ形・doc id は append-only）。
pub fn contract_id(path: &str, id: &str) -> String {
    format!("{}#{id}", doc_id(path))
}

/// doc id（file 名の stem）。
fn doc_id(path: &str) -> String {
    Path::new(path).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.374

    use super::super::tests::full_row;
    use super::{contract_id, find_row, parse_pointer, read_rows, Pointer, PointerError, TableError, BEGIN, END};

    /// `.md` は区間を行走査で抜き（0 = 表なし・2 つ以上・閉じない・end だけは断る）、`.toml` は全文、他の拡張子は
    /// 断る。行番号は doc 上の行（区間の中の行ではない）。TOML subset の欠陥は全件。
    #[test]
    fn table_region_reads_zero_or_one_region_and_maps_lines_to_the_doc() {
        let row = full_row(&[]);
        let doc = format!("# t\n\n{BEGIN}\n{row}{END}\n");
        let rows = read_rows("docs/design/t.md", &doc).expect("区間 1 つは読める");
        assert_eq!(rows.iter().map(|found| found.line).collect::<Vec<u64>>(), vec![6], "見出しの doc 上の行");
        assert_eq!(read_rows("docs/design/t.md", "# t\n本文\n"), Ok(Vec::new()), "区間の無い doc は 0 行");
        let second = doc.lines().count() as u64 + 1;
        let twice = format!("{doc}{BEGIN}\n{row}{END}\n");
        assert_eq!(read_rows("docs/design/t.md", &twice), Err(vec![TableError::RegionDuplicate { line: second }]));
        let unclosed = read_rows("docs/design/t.md", &format!("# t\n{BEGIN}\n{row}")).expect_err("閉じない区間");
        assert!(matches!(unclosed.as_slice(), [TableError::Unreadable { line: 2, .. }]), "{unclosed:?}");
        let stray = read_rows("docs/design/t.md", &format!("# t\n{END}\n")).expect_err("end だけ");
        assert!(matches!(stray.as_slice(), [TableError::Unreadable { line: 2, .. }]), "{stray:?}");
        let other = read_rows("t.txt", &row).expect_err("他の拡張子");
        assert!(matches!(other.as_slice(), [TableError::Unreadable { line: 0, .. }]), "{other:?}");
        let broken_row = full_row(&[("depends", "[]")]).replace("[[contract]]\n", "[[contract]]\ncolor = \"red\"\n");
        let broken = read_rows("docs/design/t.md", &format!("# t\n\n{BEGIN}\n{broken_row}{END}\n")).expect_err("欠陥");
        assert_eq!(broken.len(), 2, "未知 key と空の配列の 2 件: {broken:?}");
        assert!(broken.iter().all(|error| error.line() > 3), "doc 上の行番号: {broken:?}");
    }

    /// pointer は `#` ちょうど 1 つ・置き場の拡張子・空白の無い id を要り、契約 id は `<stem>#<id>`。区間の無い doc と
    /// 区間に無い id は typed に断る。
    #[test]
    fn table_pointer_and_row_lookup_refuse_malformed_and_missing_targets() {
        let path = "docs/design/contract-source.md";
        assert_eq!(parse_pointer(&format!("{path}#a")), Ok(Pointer { path: path.to_owned(), id: "a".to_owned() }));
        assert_eq!(contract_id(path, "a"), "contract-source#a");
        for (text, want) in [
            ("docs/design/x.md", PointerError::Hash),
            ("docs/design/x.md#a#b", PointerError::Hash),
            ("docs/design/x.txt#a", PointerError::Extension),
            ("#a", PointerError::Extension),
            ("docs/design/x.md#", PointerError::Id),
            ("docs/design/x.md#a b", PointerError::Id),
        ] {
            assert_eq!(parse_pointer(text), Err(want), "{text}: {}", want.reason());
        }
        let doc = format!("# t\n\n{BEGIN}\n{}{END}\n", full_row(&[("id", "\"a\"")]));
        assert_eq!(find_row("docs/design/t.md", &doc, "a").map(|row| row.id), Ok("a".to_owned()));
        assert_eq!(find_row("docs/design/t.md", &doc, "b"), Err(vec![TableError::RowMissing { line: 0, id: "b".to_owned() }]));
        assert_eq!(find_row("docs/design/t.md", "# t\n", "a"), Err(vec![TableError::RegionMissing { line: 0 }]));
    }
}
