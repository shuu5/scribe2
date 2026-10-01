//! code の索引の読み手と表（設計 docs/design/reverse-index.md §4 形 3・4・5・形 6 の鍵の digest・形 9 の a1 の口）。
//!
//! 外の道具が作った SCIP（[`scip`]）と構文の役の一致（[`read_roles`]）を、呼び手が渡す file の本文で byte の位置へ直して
//! 結び（[`join`]）、平らな表（[`flat`]）にする。**I/O を持たない**（file の本文も呼び手が渡す・口と置き場は行 a2）。

pub mod flat;
pub mod scip;

use crate::fleet::json_tree::{self, Tree};
use crate::hook::vessel::digest::fnv1a_64;
use flat::{descriptor_names, Row, TEXT_ONLY_PREFIX};
use scip::{Document, Occurrence};
use std::collections::{BTreeMap, BTreeSet};

/// 構文の役の語（設計 §5・宣言の順・規則の id は同じ字）。
pub const ROLES: [&str; 9] = ["literal", "pattern", "call", "use", "reexport", "test", "doclink", "capture", "vis"];

/// occurrence を持たないことがある役（捕えた名の字で symbol を借りる）。
const BORROWING: [&str; 3] = ["doclink", "capture", "literal"];

/// 役の一致 1 つ（byte の位置は file の先頭から・終わりは含まない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleMatch {
    /// 役の語（[`ROLES`] のどれか）。
    pub role: &'static str,
    /// 相対 path。
    pub file: String,
    /// 始まりの byte。
    pub start: usize,
    /// 終わりの byte。
    pub end: usize,
    /// 捕えた名の字（`vis` では可視性の字）。
    pub name: Option<String>,
}

/// 役の一致の読みの結果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoleRead {
    /// 9 語の役の一致（入力の順）。
    pub matches: Vec<RoleMatch>,
    /// 9 語の外の ruleId で捨てた行の数。
    pub dropped: usize,
}

/// 読めない役の一致の行の理由（行は 1 始まり）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleError {
    /// 何行目か。
    pub line: usize,
    /// 理由。
    pub reason: String,
}

impl std::fmt::Display for RoleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "roles: {} 行目: {}", self.line, self.reason)
    }
}

/// 木の中の数を byte の位置として読む（数の字面は parse が確かめた形）。
fn position(tree: Option<&Tree>, what: &str) -> Result<usize, String> {
    let text = tree.map(json_tree::render).unwrap_or_default();
    text.parse::<usize>().map_err(|_| format!("{what} が非負整数でない"))
}

/// 1 行の木を役の一致にする（9 語の外の ruleId は `None`）。
fn read_match(tree: &Tree) -> Result<Option<RoleMatch>, String> {
    let id = tree.get("ruleId").and_then(Tree::as_str).ok_or("ruleId が無い")?;
    let Some(role) = ROLES.iter().copied().find(|word| *word == id) else {
        return Ok(None);
    };
    let file = tree.get("file").and_then(Tree::as_str).ok_or("file が無い")?;
    let offset = tree.get("range").and_then(|range| range.get("byteOffset"));
    let start = position(offset.and_then(|found| found.get("start")), "byteOffset.start")?;
    let end = position(offset.and_then(|found| found.get("end")), "byteOffset.end")?;
    let name = tree.get("metaVariables").and_then(|vars| vars.get("single")).and_then(|single| single.get("NAME"));
    let name = name.and_then(|found| found.get("text")).and_then(Tree::as_str).map(str::to_owned);
    Ok(Some(RoleMatch { role, file: file.trim_start_matches("./").to_owned(), start, end, name }))
}

/// 役の一致の stream（1 行 1 件の JSON）を読む。9 語の外の ruleId の行は捨てて数え、JSON でない行は読めない理由にする。
pub fn read_roles(text: &str) -> Result<RoleRead, RoleError> {
    let mut read = RoleRead { matches: Vec::new(), dropped: 0 };
    for (at, line) in text.lines().enumerate().filter(|(_, line)| !line.trim().is_empty()) {
        let fail = |reason: String| RoleError { line: at.saturating_add(1), reason };
        let tree = json_tree::parse(line).map_err(|err| fail(err.to_string()))?;
        match read_match(&tree).map_err(fail)? {
            Some(found) => read.matches.push(found),
            None => read.dropped = read.dropped.saturating_add(1),
        }
    }
    Ok(read)
}

/// 結べない入力の理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinError {
    /// document の相対 path。
    pub path: String,
    /// 理由。
    pub reason: String,
}

impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "join: {}: {}", self.path, self.reason)
    }
}

/// byte の位置へ直した occurrence。
struct Placed<'a> {
    occ: &'a Occurrence,
    start: usize,
    end: usize,
    /// 定義が囲む範囲（byte）。
    scope: Option<(usize, usize)>,
}

/// document 1 つと本文と役の一致。
struct File<'a> {
    path: &'a str,
    body: &'a str,
    /// 行の始まりの byte（先頭は 0）。
    starts: Vec<usize>,
    /// 位置の順。
    placed: Vec<Placed<'a>>,
    matches: Vec<&'a RoleMatch>,
}

/// `line`（0 始まり）・`col`（`encoding` の単位）の byte の位置。行の中で字の途中を指す・行を越える列は `None`。
fn byte_at(body: &str, starts: &[usize], line: u32, col: u32, encoding: u32) -> Option<usize> {
    let from = *starts.get(usize::try_from(line).ok()?)?;
    let text = body.get(from..)?.split('\n').next()?;
    let mut units = 0u32;
    for (offset, ch) in text.char_indices() {
        if units >= col {
            return (units == col).then_some(from.saturating_add(offset));
        }
        units = units.saturating_add(match encoding {
            2 => u32::try_from(ch.len_utf16()).unwrap_or(1),
            3 => 1,
            _ => u32::try_from(ch.len_utf8()).unwrap_or(1),
        });
    }
    (units == col).then_some(from.saturating_add(text.len()))
}

/// 位置（byte）の行と列（どちらも 1 始まり）。
fn line_col(starts: &[usize], at: usize) -> (usize, usize) {
    let line = starts.partition_point(|start| *start <= at);
    let from = line.checked_sub(1).and_then(|index| starts.get(index)).copied().unwrap_or(0);
    (line, at.saturating_sub(from).saturating_add(1))
}

/// document の occurrence を byte の位置へ直す。
fn place<'a>(doc: &'a Document, body: &str, starts: &[usize]) -> Result<Vec<Placed<'a>>, JoinError> {
    let at = |line: u32, col: u32| byte_at(body, starts, line, col, doc.encoding);
    let mut placed = Vec::with_capacity(doc.occurrences.len());
    for occ in &doc.occurrences {
        let span = occ.span;
        let range = at(span.start_line, span.start_col).zip(at(span.end_line, span.end_col));
        let Some((start, end)) = range.filter(|(start, end)| start <= end) else {
            let reason = format!("範囲 {}:{} が本文に収まらない（symbol {}）", span.start_line, span.start_col, occ.symbol);
            return Err(JoinError { path: doc.path.clone(), reason });
        };
        let scope = occ.enclosing.and_then(|sc| at(sc.start_line, sc.start_col).zip(at(sc.end_line, sc.end_col)));
        placed.push(Placed { occ, start, end, scope });
    }
    placed.sort_by_key(|item| (item.start, item.end));
    Ok(placed)
}

/// 本文の中の範囲の字。
fn text_at(body: &str, start: usize, end: usize) -> &str {
    body.get(start..end).unwrap_or_default()
}

impl File<'_> {
    /// 範囲を含む役の一致のうち `keep` が通す物の、最も狭い幅の物（同じ幅は全部・無ければ空）。
    fn tightest(&self, start: usize, end: usize, keep: impl Fn(&str) -> bool) -> Vec<&RoleMatch> {
        let inside: Vec<&RoleMatch> =
            self.matches.iter().copied().filter(|m| keep(m.role) && m.start <= start && end <= m.end).collect();
        let tight = inside.iter().map(|m| m.end.saturating_sub(m.start)).min();
        inside.into_iter().filter(|m| Some(m.end.saturating_sub(m.start)) == tight).collect()
    }

    /// 範囲の役の語（最も内側の一致・同じ幅は全部・[`ROLES`] の順・test と vis は含めない）。
    fn roles_at(&self, start: usize, end: usize) -> Vec<String> {
        let hits = self.tightest(start, end, |role| role != "test" && role != "vis");
        ROLES.iter().filter(|word| hits.iter().any(|m| m.role == **word)).map(|word| (*word).to_owned()).collect()
    }

    /// 範囲が test の役の中か。
    fn in_test(&self, start: usize, end: usize) -> bool {
        self.matches.iter().any(|m| m.role == "test" && m.start <= start && end <= m.end)
    }

    /// 範囲の可視性の字（最も内側の vis の一致の名・空白は 1 つに畳む）。
    fn vis_at(&self, start: usize, end: usize) -> String {
        let hits = self.tightest(start, end, |role| role == "vis");
        let name = hits.first().and_then(|m| m.name.as_deref()).unwrap_or_default();
        name.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// 範囲を囲む定義（最も狭い範囲を持つ定義・`skip` の位置の occurrence 自身は除く）。
    fn enclosing_at(&self, start: usize, end: usize, skip: Option<usize>) -> Option<&Placed<'_>> {
        let candidates = self.placed.iter().enumerate().filter(|(index, item)| Some(*index) != skip && item.occ.definition);
        candidates
            .filter_map(|(_, item)| item.scope.filter(|(from, to)| *from <= start && end <= *to).map(|(from, to)| (to - from, item)))
            .min_by_key(|(width, _)| *width)
            .map(|(_, item)| item)
    }
}

/// 全 document の本文と役の一致を束ねる。
fn files_of<'a>(
    docs: &'a [Document],
    matches: &'a [RoleMatch],
    bodies: &'a BTreeMap<String, String>,
) -> Result<Vec<File<'a>>, JoinError> {
    let mut files = Vec::with_capacity(docs.len());
    for doc in docs {
        let Some(body) = bodies.get(&doc.path) else {
            return Err(JoinError { path: doc.path.clone(), reason: "file の本文が渡されていない".to_owned() });
        };
        let starts: Vec<usize> =
            std::iter::once(0).chain(body.match_indices('\n').map(|(at, _)| at.saturating_add(1))).collect();
        let placed = place(doc, body, &starts)?;
        let matches = matches.iter().filter(|m| m.file == doc.path).collect();
        files.push(File { path: &doc.path, body, starts, placed, matches });
    }
    Ok(files)
}

/// module の宣言（字が名と同じ定義の occurrence・document の位置と symbol と test の役の中か）と、module の file
/// （symbol → file の全体を定義する document の位置）。
type Modules<'a> = (Vec<(usize, &'a str, bool)>, BTreeMap<&'a str, Vec<usize>>);

/// module の定義の occurrence を、宣言と file に分ける（file の先頭の定義は字が空で名と合わない）。
fn modules_of<'a>(files: &'a [File<'a>]) -> Modules<'a> {
    let mut declared = Vec::new();
    let mut module_files: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, file) in files.iter().enumerate() {
        let modules = file.placed.iter().filter(|item| item.occ.definition && item.occ.symbol.ends_with('/'));
        for item in modules {
            let written = Some(text_at(file.body, item.start, item.end));
            if descriptor_names(&item.occ.symbol).last().map(String::as_str) == written {
                declared.push((index, item.occ.symbol.as_str(), file.in_test(item.start, item.end)));
            } else {
                module_files.entry(&item.occ.symbol).or_default().push(index);
            }
        }
    }
    (declared, module_files)
}

/// test の module の file（document の位置の集合）。module を宣言する occurrence が test の役の中か test の file の中に
/// 在る module の file は、全体が test である（宣言の連鎖は不動点まで）。
fn test_files(files: &[File<'_>]) -> BTreeSet<usize> {
    let (declared, module_files) = modules_of(files);
    let mut tests = BTreeSet::new();
    loop {
        let before = tests.len();
        for (index, symbol, in_match) in &declared {
            if *in_match || tests.contains(index) {
                tests.extend(module_files.get(symbol).into_iter().flatten().copied().filter(|other| other != index));
            }
        }
        if tests.len() == before {
            return tests;
        }
    }
}

/// occurrence の行。
fn occurrence_rows(file: &File<'_>, in_test_file: bool) -> Vec<Row> {
    let mut rows = Vec::with_capacity(file.placed.len());
    for (index, item) in file.placed.iter().enumerate() {
        let (line, col) = line_col(&file.starts, item.start);
        rows.push(Row {
            path: file.path.to_owned(),
            line,
            col,
            symbol: item.occ.symbol.clone(),
            definition: item.occ.definition,
            roles: file.roles_at(item.start, item.end),
            test: in_test_file || file.in_test(item.start, item.end),
            enclosing: file.enclosing_at(item.start, item.end, Some(index)).map(|up| up.occ.symbol.clone()).unwrap_or_default(),
            vis: if item.occ.definition { file.vis_at(item.start, item.end) } else { String::new() },
        });
    }
    rows
}

/// occurrence を持たない役の一致の symbol を、囲む定義の中 → 同じ file の順に同じ字の occurrence から借りる。
fn borrow_symbol<'a>(file: &'a File<'_>, found: &RoleMatch, name: &str) -> Option<&'a str> {
    let same = |item: &&Placed<'_>| text_at(file.body, item.start, item.end) == name;
    let scoped = file.enclosing_at(found.start, found.end, None).and_then(|up| up.scope).and_then(|(from, to)| {
        file.placed.iter().filter(|item| from <= item.start && item.end <= to).find(same)
    });
    scoped.or_else(|| file.placed.iter().find(same)).map(|item| item.occ.symbol.as_str())
}

/// occurrence の無い doc の link・取り込み・`Self` の literal の行（借りられなければ字だけの行）。
fn orphan_rows(file: &File<'_>, in_test_file: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    for found in file.matches.iter().filter(|found| BORROWING.contains(&found.role)) {
        let Some(name) = found.name.as_deref().filter(|name| !name.is_empty()) else {
            continue;
        };
        let seen = file.placed.iter().any(|item| {
            found.start <= item.start && item.end <= found.end && text_at(file.body, item.start, item.end) == name
        });
        if seen {
            continue;
        }
        let (line, col) = line_col(&file.starts, found.start);
        rows.push(Row {
            path: file.path.to_owned(),
            line,
            col,
            symbol: borrow_symbol(file, found, name).map_or_else(|| format!("{TEXT_ONLY_PREFIX}{name}"), str::to_owned),
            definition: false,
            roles: vec![found.role.to_owned()],
            test: in_test_file || file.in_test(found.start, found.end),
            enclosing: file.enclosing_at(found.start, found.end, None).map(|up| up.occ.symbol.clone()).unwrap_or_default(),
            vis: String::new(),
        });
    }
    rows
}

/// SCIP の document と役の一致を、呼び手が渡した file の本文（相対 path → 本文）で結んで表の行にする。
///
/// SCIP の列は document の position_encoding の単位から byte に直す。occurrence ごとに範囲を含む最も内側の役を付け、
/// occurrence の無い doc の link・取り込み・`Self` の literal は同じ字の occurrence から symbol を借りる（借りられない
/// 物は symbol が [`TEXT_ONLY_PREFIX`] で始まる字だけの行）。行は（path・行・列・symbol）の順。
pub fn join(docs: &[Document], matches: &[RoleMatch], bodies: &BTreeMap<String, String>) -> Result<Vec<Row>, JoinError> {
    let files = files_of(docs, matches, bodies)?;
    let tests = test_files(&files);
    let mut rows = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let in_test_file = tests.contains(&index);
        rows.extend(occurrence_rows(file, in_test_file));
        rows.extend(orphan_rows(file, in_test_file));
    }
    rows.sort_by(|a, b| (&a.path, a.line, a.col, &a.symbol).cmp(&(&b.path, b.line, b.col, &b.symbol)));
    Ok(rows)
}

/// 索引の鍵の digest（16 桁）。code の木の鍵と宣言 2 key（`index-scip`・`index-roles`）の字を、長さを前置きして
/// 並べた字の FNV-1a 64（[`fnv1a_64`]・2 本目の hash を作らない）。どれかの 1 字の違いが別の字になる。
pub fn key_digest(tree_key: &str, index_scip: &[String], index_roles: &[String]) -> String {
    let mut text = format!("{}:{tree_key}\n", tree_key.len());
    for (key, lines) in [("index-scip", index_scip), ("index-roles", index_roles)] {
        text.push_str(&format!("{key}={}\n", lines.len()));
        for line in lines {
            text.push_str(&format!("{}:{line}\n", line.len()));
        }
    }
    fnv1a_64(text.as_bytes())
}
