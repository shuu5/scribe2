//! 契約表の置き場の抜き出しと TOML subset の型付け（設計 docs/design/contract-source.md §15・SRS FR47 / FR54・pure）。
//!
//! `.md` は `<!-- contracts:begin -->` … `<!-- contracts:end -->` の区間を行走査で抜き、`.toml` は全文を [`read_rows`]
//! が rules manifest の 1 本の parser へ渡して、欄の形（[`super::FIELDS`] の `shape`）で [`super::ContractRow`] へ
//! 型付けする。設計 pointer（`<path>#<id>`）の読みと行 1 つの引き（[`find_row`]）も同じ群。findings の語彙
//! （[`super::TableError`] / `Finding` / `Context`）は親 module `table.rs`・表の検査は兄弟 `table/check.rs` に
//! 置いたまま。呼び手（`pipe/cli/intake.rs`・`pipe/review.rs`・歯）の `use` は親の再 export を通る。

use super::{unreadable, ContractRow, Need, PromiseRow, TableError, BEGIN, END, FIELDS, PROMISE, PROMISE_FIELDS};
use crate::rules::manifest::{contract_rows, list, scalar, Scalar, TableRow, TableValue};
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
/// 欠陥は**全件**・doc 上の行番号付きで返す。約束の行も読んで欠陥を名指すが、返すのは契約の行だけ（約束の行も
/// 要る呼び手は [`read_table`]）。
pub fn read_rows(path: &str, text: &str) -> Result<Vec<ContractRow>, Vec<TableError>> {
    read_table(path, text).map(|(rows, _)| rows)
}

/// 契約表の置き場 1 本の契約の行と約束の行 `[[promise]]` を読む（[`read_rows`] と同じ置き場・同じ欠陥の積み方）。
/// 約束の行の欄は [`super::PROMISE_FIELDS`] の形で型付けし、欠けた必須欄・空の欄・未知の key を doc 上の行番号で
/// 積む。`of` の親の行の実在と `n` の連番は表の検査（`check_promises`）の段で測る。
pub fn read_table(path: &str, text: &str) -> Result<(Vec<ContractRow>, Vec<PromiseRow>), Vec<TableError>> {
    let (offset, body) = match form_of(path).map_err(|found| vec![found])? {
        Form::Whole => (0, text.to_owned()),
        Form::Region => match region(text).map_err(|found| vec![found])? {
            Some(found) => found,
            None => return Ok((Vec::new(), Vec::new())),
        },
    };
    let mut late = Vec::new();
    let (contracts, raw_promises) = split_promises(&body, offset, &mut late);
    let promises: Vec<PromiseRow> = raw_promises.iter().filter_map(|raw| typed_promise(raw, &mut late)).collect();
    let raws = contract_rows(contracts.as_deref().unwrap_or(&body)).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| unreadable(shift(offset, error.line), &error.message))
            .chain(late.iter().cloned())
            .collect::<Vec<TableError>>()
    })?;
    let mut errors = Vec::new();
    let rows: Vec<ContractRow> = raws.iter().filter_map(|raw| typed(raw, offset, &mut errors)).collect();
    errors.extend(conditional_missing(&raws, offset, &raw_promises));
    errors.extend(late);
    if errors.is_empty() {
        Ok((rows, promises))
    } else {
        Err(errors)
    }
}

/// 条件付きの欄（[`Need::Conditional`]・`done` / `verify`）の欠け: 約束の行を 1 つも持たない行（`of` がその行の id を
/// 名指す約束の行が無い）に欄が無ければ、rules manifest の必須 key と同じ字面で行の見出しに名指す（設計 §33 の
/// 「必須の緩み」）。約束の行は [`split_promises`] が抜いた生の行の `of` で数える（型付けに落ちた約束の行も親を持つ）。
fn conditional_missing(raws: &[TableRow], offset: u64, promises: &[RawPromise]) -> Vec<TableError> {
    let parents: Vec<&str> = promises
        .iter()
        .filter_map(|raw| match raw.value("of") {
            Some((TableValue::One(Scalar::Str(of)), _)) => Some(of.as_str()),
            _ => None,
        })
        .collect();
    let mut found = Vec::new();
    for raw in raws {
        let promised = match raw.value("id") {
            Some((TableValue::One(Scalar::Str(id)), _)) => parents.contains(&id.as_str()),
            _ => false,
        };
        let missing = FIELDS.iter().filter(|field| field.need == Need::Conditional && raw.value(field.name).is_none());
        if !promised {
            found.extend(missing.map(|field| unreadable(shift(offset, raw.line()), &format!("必須 key {} が無い", field.name))));
        }
    }
    found
}

/// 親の行 id `id` の約束の行（doc 順）。
pub fn promises_of<'p>(promises: &'p [PromiseRow], id: &str) -> Vec<&'p PromiseRow> {
    promises.iter().filter(|promise| promise.of == id).collect()
}

/// 約束の行 1 つの生の欄（key・値・書かれていた doc 上の行番号）。
struct RawPromise {
    /// 見出しの doc 上の行番号。
    line: u64,
    /// 書かれた順の欄。
    fields: Vec<(String, TableValue, u64)>,
}

impl RawPromise {
    /// key の値と、その key が書かれていた doc 上の行番号。
    fn value(&self, key: &str) -> Option<(&TableValue, u64)> {
        self.fields.iter().find(|(found, _, _)| found == key).map(|(_, value, line)| (value, *line))
    }
}

/// 区間の本文から約束の行（`[[promise]]` の見出しから次の見出しの前まで）を抜く。約束の行が 1 つも無い本文は
/// 契約の本文を `None`（本文をそのまま読む＝約束の行を持たない表の読みは 1 字も変わらない）、在れば約束の行の区間を
/// 空行に置き換えた本文（行番号を保つ）を返す。rules manifest の面は `[[contract]]` だけを受けるので、約束の行は
/// ここで抜いて値の層（[`scalar`] / [`list`]・受理集合の唯一の定義）だけを共有する（第 2 の値 parser を作らない）。
fn split_promises(body: &str, offset: u64, errors: &mut Vec<TableError>) -> (Option<String>, Vec<RawPromise>) {
    let (mut rest, mut raws, mut inside) = (String::new(), Vec::<RawPromise>::new(), false);
    for (index, line) in body.lines().enumerate() {
        let at = shift(offset, (index as u64).saturating_add(1));
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == PROMISE;
            if inside {
                raws.push(RawPromise { line: at, fields: Vec::new() });
                rest.push('\n');
                continue;
            }
        }
        if !inside {
            rest.push_str(line);
            rest.push('\n');
            continue;
        }
        rest.push('\n');
        let Some(raw) = raws.last_mut() else {
            continue;
        };
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match promise_pair(trimmed) {
            Ok((key, value)) => raw.fields.push((key, value, at)),
            Err(reason) => errors.push(unreadable(at, &reason)),
        }
    }
    let contracts = (!raws.is_empty()).then_some(rest);
    (contracts, raws)
}

/// 約束の行の `key = value` 1 行（値は rules manifest と同じ [`scalar`] / [`list`] で読む・字面も同じ）。
fn promise_pair(trimmed: &str) -> Result<(String, TableValue), String> {
    let Some((key, raw)) = trimmed.split_once('=') else {
        return Err(format!("key = value の形でない: {trimmed}"));
    };
    let (key, raw) = (key.trim().to_owned(), raw.trim());
    let value = if raw.starts_with('[') {
        TableValue::List(list(raw).map_err(|reason| format!("{key} の {reason}"))?)
    } else {
        let found = scalar(raw).ok_or_else(|| {
            format!("{key} の value が TOML subset の形でない（string / integer / bool / 文字列の配列のみ）")
        })?;
        TableValue::One(found)
    };
    Ok((key, value))
}

/// 約束の行 1 つを欄の形（[`super::PROMISE_FIELDS`] の `shape`）で型付けする。未知 key・重複 key・必須 key の欠落
/// （rules manifest と同じ字面）と形の違い（契約の行と同じ字面）を積む。
fn typed_promise(raw: &RawPromise, errors: &mut Vec<TableError>) -> Option<PromiseRow> {
    let before = errors.len();
    for (index, (key, _, line)) in raw.fields.iter().enumerate() {
        if !PROMISE_FIELDS.iter().any(|field| field.name == key) {
            errors.push(unreadable(*line, &format!("未知の key {key}（{PROMISE} の欄でない）")));
        }
        if raw.fields.iter().take(index).any(|(earlier, _, _)| earlier == key) {
            errors.push(unreadable(*line, &format!("key {key} が重複する")));
        }
    }
    for field in PROMISE_FIELDS.iter().filter(|field| field.need == Need::Required) {
        if raw.value(field.name).is_none() {
            errors.push(unreadable(raw.line, &format!("必須 key {} が無い（{PROMISE}）", field.name)));
        }
    }
    let row = PromiseRow {
        line: raw.line,
        of: text_in(raw.value("of"), "of", errors),
        n: number_in(raw.value("n"), "n", errors),
        text: text_in(raw.value("text"), "text", errors),
        files: list_in(raw.value("files"), "files", errors),
        symbols: list_in(raw.value("symbols"), "symbols", errors),
        teeth: list_in(raw.value("teeth"), "teeth", errors),
        place: text_in(raw.value("place"), "place", errors),
        fixture: text_in(raw.value("fixture"), "fixture", errors),
        expect: text_in(raw.value("expect"), "expect", errors),
    };
    (errors.len() == before).then_some(row)
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
    text_in(raw.value(key).map(|(value, line)| (value, shift(offset, line))), key, errors)
}

/// 配列の欄（無ければ空・形が違えば積む）。
fn list_of(raw: &TableRow, key: &str, offset: u64, errors: &mut Vec<TableError>) -> Vec<String> {
    list_in(raw.value(key).map(|(value, line)| (value, shift(offset, line))), key, errors)
}

/// 文字列の欄の値（`found` は値と doc 上の行番号・無ければ空・空の文字列か形が違えば積む）。
fn text_in(found: Option<(&TableValue, u64)>, key: &str, errors: &mut Vec<TableError>) -> String {
    match found {
        None => String::new(),
        Some((TableValue::One(Scalar::Str(found)), _)) if !found.trim().is_empty() => found.clone(),
        Some((_, line)) => {
            errors.push(unreadable(line, &format!("{key} は空でない文字列でなければならない")));
            String::new()
        }
    }
}

/// 配列の欄の値（`found` は値と doc 上の行番号・無ければ空・形が違えば積む）。
fn list_in(found: Option<(&TableValue, u64)>, key: &str, errors: &mut Vec<TableError>) -> Vec<String> {
    match found {
        None => Vec::new(),
        Some((TableValue::List(items), _)) => items.clone(),
        Some((_, line)) => {
            errors.push(unreadable(line, &format!("{key} は文字列の配列でなければならない")));
            Vec::new()
        }
    }
}

/// 数の欄の値（`found` は値と doc 上の行番号・無ければ 0・1 未満か形が違えば積む）。
fn number_in(found: Option<(&TableValue, u64)>, key: &str, errors: &mut Vec<TableError>) -> u64 {
    match found {
        None => 0,
        Some((TableValue::One(Scalar::Int(n)), _)) if *n >= 1 => *n,
        Some((_, line)) => {
            errors.push(unreadable(line, &format!("{key} は 1 以上の整数でなければならない")));
            0
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

    use super::super::tests::{full_promise, full_row};
    use super::{
        contract_id, find_row, parse_pointer, promises_of, read_rows, read_table, Pointer, PointerError, TableError, BEGIN,
        END,
    };

    /// doc 上で `[[promise]]` の見出しが在る行番号（1 始まり・doc 順）。
    fn promise_lines(doc: &str) -> Vec<u64> {
        doc.lines().enumerate().filter(|(_, line)| *line == "[[promise]]").map(|(index, _)| index as u64 + 1).collect()
    }

    /// 約束の行は 9 欄（宣言順）で読まれて契約の行と別の列に入り、親の行 id で引ける。任意の 2 欄（`symbols` /
    /// `place`）は省けば空。約束の行を足しても契約の行の読み（行番号を含む）は 1 字も変わらない。
    #[test]
    fn contract_promise_parse_reads_nine_fields_and_looks_up_by_the_parent_id() {
        let row = full_row(&[("id", "\"a\"")]);
        let plain = format!("# t\n\n{BEGIN}\n{row}{END}\n");
        let (first, second) = (full_promise("a", 1, &[]), full_promise("a", 2, &[("symbols", ""), ("place", "")]));
        let doc = format!("# t\n\n{BEGIN}\n{row}{first}{second}{END}\n");
        let (rows, promises) = read_table("docs/design/t.md", &doc).unwrap_or_else(|errors| panic!("読める: {errors:?}"));
        assert_eq!(read_rows("docs/design/t.md", &plain), Ok(rows.clone()), "契約の行の読みは約束の行の有無で変わらない");
        assert_eq!(read_rows("docs/design/t.md", &doc), Ok(rows), "read_rows は契約の行だけを返す");
        assert_eq!(read_table("docs/design/t.md", &plain).map(|(_, found)| found.len()), Ok(0), "約束の行の無い表は 0");
        let own = promises_of(&promises, "a");
        assert_eq!(own.iter().map(|promise| promise.line).collect::<Vec<u64>>(), promise_lines(&doc), "見出しの doc 上の行");
        assert_eq!(own.iter().map(|promise| promise.n).collect::<Vec<u64>>(), vec![1, 2], "n は doc 順");
        let one = own.first().copied().unwrap_or_else(|| panic!("約束の行 1"));
        let text = |name: &str| format!("{name} の値");
        assert_eq!(
            (one.of.as_str(), one.text.clone(), one.place.clone(), one.fixture.clone(), one.expect.clone()),
            ("a", text("text"), text("place"), text("fixture"), text("expect")),
            "文字列の 5 欄"
        );
        assert_eq!(
            (one.files.clone(), one.symbols.clone(), one.teeth.clone()),
            (vec![text("files")], vec![text("symbols")], vec![text("teeth")]),
            "列の 3 欄"
        );
        let two = own.get(1).copied().unwrap_or_else(|| panic!("約束の行 2"));
        assert!(two.symbols.is_empty() && two.place.is_empty(), "任意の欄は省けば空: {two:?}");
        assert!(promises_of(&promises, "b").is_empty(), "親の違う約束の行は引かない");
        let whole = format!("{row}{first}");
        assert_eq!(read_table("t.toml", &whole).map(|(_, found)| found.len()), Ok(1), ".toml の全文でも読む");
    }

    /// 約束の行の欠陥（空の必須欄・欠けた必須欄・空の配列・1 未満の n・未知の key・重複 key）は、それぞれ doc 上の
    /// 行番号付きの `unreadable` で名指され、契約の行だけを返す [`read_rows`] も同じ欠陥で断る。
    #[test]
    fn contract_promise_parse_names_empty_missing_and_unknown_fields_with_their_doc_line() {
        let row = full_row(&[("id", "\"a\"")]);
        /// 差し替える欄・名指される行の字面の頭・理由の字面。
        type Case<'c> = (&'c [(&'c str, &'c str)], &'c str, &'c str);
        let cases: [Case<'_>; 6] = [
            (&[("text", "\"\"")], "text = ", "text は空でない文字列でなければならない"),
            (&[("expect", "\"  \"")], "expect = ", "expect は空でない文字列でなければならない"),
            (&[("fixture", "")], "[[promise]]", "必須 key fixture が無い"),
            (&[("teeth", "[]")], "teeth = ", "teeth の 配列が空である"),
            (&[("n", "0")], "n = ", "n は 1 以上の整数でなければならない"),
            (&[("files", "[\"a.rs\"]\nfiles = [\"b.rs\"]")], "files = [\"b.rs\"]", "key files が重複する"),
        ];
        for (over, at, want) in cases {
            let doc = format!("# t\n\n{BEGIN}\n{row}{}{END}\n", full_promise("a", 1, over));
            let lines: Vec<&str> = doc.lines().collect();
            let line = lines.iter().rposition(|found| found.starts_with(at)).map_or(0, |index| index as u64 + 1);
            let errors = read_table("docs/design/t.md", &doc).expect_err(want);
            let named = errors
                .iter()
                .any(|error| matches!(error, TableError::Unreadable { .. }) && error.line() == line && error.reason().contains(want));
            assert!(named, "{want} を行 {line} で名指す: {errors:?}");
            assert_eq!(read_rows("docs/design/t.md", &doc).err(), Some(errors), "read_rows も同じ欠陥で断る");
        }
        let unknown = full_promise("a", 1, &[]).replace("[[promise]]\n", "[[promise]]\ncolor = \"red\"\n");
        let doc = format!("# t\n\n{BEGIN}\n{row}{unknown}{END}\n");
        let errors = read_table("docs/design/t.md", &doc).expect_err("未知の key");
        let line = promise_lines(&doc).first().map_or(0, |found| found + 1);
        assert!(
            errors.iter().any(|error| error.line() == line && error.reason().contains("未知の key color")),
            "未知の key を名指す: {errors:?}"
        );
    }

    // flip-check: s2-07l.512

    /// §33 (f): `done` と `verify` を持たない行は、約束の行が 1 つでも在れば parse を通り（2 欄は空）、約束の行が無ければ
    /// 行の見出しの行番号で「必須 key done が無い」「必須 key verify が無い」の 2 件を名指される（他の行の約束の行では
    /// 緩まない）。`read_rows` も同じ判定（1 実装）。
    #[test]
    fn contract_promise_need_relaxes_done_and_verify_only_for_a_row_with_promises() {
        let bare = |id: &str| {
            full_row(&[("id", &format!("\"{id}\""))])
                .lines()
                .filter(|line| !line.starts_with("done =") && !line.starts_with("verify ="))
                .map(|line| format!("{line}\n"))
                .collect::<String>()
                .replace("schema = 1\n\n", "")
        };
        let promised = format!("# t\n\n{BEGIN}\nschema = 1\n\n{}{}{END}\n", bare("a"), full_promise("a", 1, &[]));
        let (rows, promises) = read_table("docs/design/t.md", &promised).unwrap_or_else(|errors| panic!("通る: {errors:?}"));
        let row = rows.first().unwrap_or_else(|| panic!("行 a"));
        assert!(row.done.is_empty() && row.verify.is_empty(), "約束の行を持つ行の 2 欄は空: {row:?}");
        assert_eq!(promises_of(&promises, "a").len(), 1, "約束の行 1");
        assert_eq!(read_rows("docs/design/t.md", &promised).map(|found| found.len()), Ok(1), "read_rows も通す");
        let orphan = format!("# t\n\n{BEGIN}\nschema = 1\n\n{}{}{}{END}\n", bare("b"), bare("a"), full_promise("a", 1, &[]));
        let line = orphan.lines().position(|found| found == "id = \"b\"").map_or(0, |index| index as u64);
        let errors = read_table("docs/design/t.md", &orphan).expect_err("約束の行を持たない行は断る");
        let named: Vec<(u64, String)> = errors.iter().map(|error| (error.line(), error.reason())).collect();
        assert_eq!(
            named,
            [(line, "必須 key verify が無い".to_owned()), (line, "必須 key done が無い".to_owned())],
            "行 b の見出しで 2 欄を名指す（行 a は緩む）"
        );
        assert!(errors.iter().all(|error| matches!(error, TableError::Unreadable { .. })), "TableError の必須 key の欠け");
        assert_eq!(read_rows("docs/design/t.md", &orphan).err(), Some(errors), "read_rows も同じ欠陥で断る");
    }

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
