//! 平らな表の描きと読みと表の問い（設計 docs/design/reverse-index.md §4 形 5・形 9）。
//!
//! 表は頭の行 `schema=1` と、1 行 1 occurrence の tab 区切りの 9 列（path・行・列・symbol・定義か・役の列・test か・
//! 囲む定義の symbol・可視性の字）である。読みは描きの逆で、schema の違う表は「無い」と読み、形の崩れた行は
//! [`TableError`] にする（導出値なので古い表は作り直す）。行・列は 1 始まり、列は行の先頭からの byte。

use super::ROLES;
use std::collections::BTreeMap;
use std::iter::Peekable;
use std::str::Chars;

/// 表の schema 版。
pub const SCHEMA: u32 = 1;
/// 字だけの行（symbol を借りられなかった役の一致）の symbol の接頭辞（SCIP の symbol はこの形にならない）。
pub const TEXT_ONLY_PREFIX: &str = "text:";
/// 列の数。
const COLUMNS: usize = 9;

/// 表の 1 行（1 occurrence）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// repo の root からの相対 path。
    pub path: String,
    /// 行（1 始まり）。
    pub line: usize,
    /// 列（1 始まり・行の先頭からの byte）。
    pub col: usize,
    /// symbol の字（借りられなかった物は [`TEXT_ONLY_PREFIX`] と捕えた名）。
    pub symbol: String,
    /// 定義の occurrence か。
    pub definition: bool,
    /// 役の語（[`ROLES`] の順・test と vis は持たない）。
    pub roles: Vec<String>,
    /// test の役の中（か test の module の file）か。
    pub test: bool,
    /// 囲む定義の symbol（無ければ空）。
    pub enclosing: String,
    /// 定義の source の可視性の字（無ければ空）。
    pub vis: String,
}

impl Row {
    /// symbol を借りられなかった字だけの行か。
    pub fn text_only(&self) -> bool {
        self.symbol.starts_with(TEXT_ONLY_PREFIX)
    }
}

/// 読めない表の行の理由（行は 1 始まりの物理行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableError {
    /// 何行目か。
    pub line: usize,
    /// 理由。
    pub reason: String,
}

impl std::fmt::Display for TableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "table: {} 行目: {}", self.line, self.reason)
    }
}

/// 真偽の列の字。
fn flag(on: bool) -> &'static str {
    if on {
        "1"
    } else {
        "0"
    }
}

/// 表を描く（頭の行と 1 行 1 occurrence・末尾は改行）。
pub fn render(rows: &[Row]) -> String {
    let mut out = format!("schema={SCHEMA}\n");
    for row in rows {
        let cols = [
            row.path.as_str(),
            &row.line.to_string(),
            &row.col.to_string(),
            &row.symbol,
            flag(row.definition),
            &row.roles.join(","),
            flag(row.test),
            &row.enclosing,
            &row.vis,
        ];
        out.push_str(&cols.join("\t"));
        out.push('\n');
    }
    out
}

/// 真偽の列を読む。
fn read_flag(text: &str) -> Result<bool, String> {
    match text {
        "1" => Ok(true),
        "0" => Ok(false),
        other => Err(format!("真偽の列が 0 か 1 でない: {other:?}")),
    }
}

/// 数の列を読む。
fn read_number(text: &str) -> Result<usize, String> {
    text.parse::<usize>().map_err(|_| format!("数の列でない: {text:?}"))
}

/// 役の列を読む（空は役なし・9 語の外は形の崩れ）。
fn read_roles_column(text: &str) -> Result<Vec<String>, String> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let words: Vec<&str> = text.split(',').collect();
    match words.iter().find(|word| !ROLES.contains(word)) {
        Some(word) => Err(format!("役の語でない: {word:?}")),
        None => Ok(words.into_iter().map(str::to_owned).collect()),
    }
}

/// 1 行を読む。
fn read_row(line: &str) -> Result<Row, String> {
    let cols: Vec<&str> = line.split('\t').collect();
    let found = cols.len();
    let Ok([path, row_line, col, symbol, definition, roles, test, enclosing, vis]) = <[&str; COLUMNS]>::try_from(cols)
    else {
        return Err(format!("列が {COLUMNS} つでない（{found} 列）"));
    };
    Ok(Row {
        path: path.to_owned(),
        line: read_number(row_line)?,
        col: read_number(col)?,
        symbol: symbol.to_owned(),
        definition: read_flag(definition)?,
        roles: read_roles_column(roles)?,
        test: read_flag(test)?,
        enclosing: enclosing.to_owned(),
        vis: vis.to_owned(),
    })
}

/// 表を読む。schema の違う表は `Ok(None)`（無いと読む）・頭の行が `schema=` でない形と形の崩れた行は [`TableError`]。
pub fn read_table(text: &str) -> Result<Option<Vec<Row>>, TableError> {
    let mut lines = text.lines();
    let head = lines.next().and_then(|head| head.strip_prefix("schema="));
    let Some(version) = head else {
        return Err(TableError { line: 1, reason: "頭の行が schema= でない".to_owned() });
    };
    if version != SCHEMA.to_string() {
        return Ok(None);
    }
    let mut rows = Vec::new();
    for (at, line) in lines.enumerate() {
        rows.push(read_row(line).map_err(|reason| TableError { line: at.saturating_add(2), reason })?);
    }
    Ok(Some(rows))
}

/// symbol の 4 つの成分（scheme・manager・package・version）を飛ばした残り（空白 2 つは成分の中の空白）。
fn descriptors_of(symbol: &str) -> Option<&str> {
    let mut rest = symbol;
    for _ in 0..4 {
        let bytes = rest.as_bytes();
        let mut at = 0;
        loop {
            match (bytes.get(at), bytes.get(at.saturating_add(1))) {
                (Some(b' '), Some(b' ')) => at = at.saturating_add(2),
                (Some(b' '), _) => break,
                (Some(_), _) => at = at.saturating_add(1),
                (None, _) => return None,
            }
        }
        rest = rest.get(at.saturating_add(1)..)?;
    }
    Some(rest)
}

/// backtick で囲まれた名（二重の backtick は 1 字の backtick）。
fn escaped_name(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut name = String::new();
    while let Some(ch) = chars.next() {
        if ch != '`' {
            name.push(ch);
        } else if chars.next_if_eq(&'`').is_some() {
            name.push('`');
        } else {
            break;
        }
    }
    name
}

/// `end` までの名（`end` も食う）。
fn name_until(chars: &mut Peekable<Chars<'_>>, end: char) -> String {
    let mut name = String::new();
    for ch in chars.by_ref() {
        if ch == end {
            break;
        }
        name.push(ch);
    }
    name
}

/// 句読点（`/ # . : ! (`）の手前までの名。
fn plain_name(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut name = String::new();
    while let Some(ch) = chars.next_if(|ch| !"/#.:!(".contains(*ch)) {
        name.push(ch);
    }
    name
}

/// symbol の descriptor の名の列（namespace・type・term・method・macro・type parameter・parameter の名を前から）。
pub fn descriptor_names(symbol: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut chars = descriptors_of(symbol).unwrap_or_default().chars().peekable();
    while let Some(first) = chars.peek().copied() {
        let name = match first {
            '[' | '(' => {
                chars.next();
                name_until(&mut chars, if first == '[' { ']' } else { ')' })
            }
            _ => {
                let name = if chars.next_if_eq(&'`').is_some() { escaped_name(&mut chars) } else { plain_name(&mut chars) };
                if chars.next_if_eq(&'(').is_some() {
                    name_until(&mut chars, ')');
                }
                chars.next();
                name
            }
        };
        if !name.is_empty() {
            names.push(name);
        }
    }
    names
}

/// 問いの答えの site 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// 相対 path。
    pub path: String,
    /// 行（1 始まり）。
    pub line: usize,
    /// 役の語（[`ROLES`] の宣言の順）。
    pub roles: Vec<String>,
    /// test の中か。
    pub test: bool,
    /// 囲む定義の symbol（無ければ空）。
    pub enclosing: String,
    /// 定義の site か。
    pub definition: bool,
}

/// 解けた symbol 1 つとその site の列（表の順）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// symbol の字。
    pub symbol: String,
    /// site の列。
    pub sites: Vec<Site>,
}

/// 項目の path が解けた数（0・1・複数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// 解けない。
    Unresolved,
    /// symbol が 1 つ。
    One(Resolved),
    /// symbol が複数（symbol の字の順）。
    Ambiguous(Vec<Resolved>),
}

/// 行から site を作る（役は [`ROLES`] の宣言の順に並べ直す）。
fn site_of(row: &Row) -> Site {
    let roles = ROLES.iter().filter(|word| row.roles.iter().any(|role| role.as_str() == **word)).map(|word| (*word).to_owned());
    Site {
        path: row.path.clone(),
        line: row.line,
        roles: roles.collect(),
        test: row.test,
        enclosing: row.enclosing.clone(),
        definition: row.definition,
    }
}

/// 項目の path（`crate::a::B`・先頭の `crate` は落とす）を、読めた表の symbol へ descriptor の名の列の末尾一致で
/// 解く。symbol ごとに site の列を返す（字だけの行は symbol を持たないので答えに入らない）。
pub fn query(rows: &[Row], item: &str) -> Resolution {
    let segments: Vec<&str> = item.split("::").map(str::trim).filter(|segment| !segment.is_empty()).collect();
    let want = match segments.split_first() {
        Some((&"crate", rest)) => rest,
        _ => segments.as_slice(),
    };
    if want.is_empty() {
        return Resolution::Unresolved;
    }
    let mut by_symbol: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for row in rows.iter().filter(|row| !row.symbol.is_empty() && !row.text_only()) {
        by_symbol.entry(&row.symbol).or_default().push(row);
    }
    let ends_with_want = |symbol: &str| {
        let names = descriptor_names(symbol);
        names.iter().rev().map(String::as_str).take(want.len()).eq(want.iter().rev().copied())
    };
    let mut found: Vec<Resolved> = by_symbol
        .into_iter()
        .filter(|(symbol, _)| ends_with_want(symbol))
        .map(|(symbol, hits)| Resolved { symbol: symbol.to_owned(), sites: hits.into_iter().map(site_of).collect() })
        .collect();
    match found.len() {
        0 => Resolution::Unresolved,
        1 => found.pop().map_or(Resolution::Unresolved, Resolution::One),
        _ => Resolution::Ambiguous(found),
    }
}
