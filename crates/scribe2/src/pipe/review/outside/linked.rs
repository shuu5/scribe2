//! 外の材料の (g)〜(i): 契約の本文が指す別の設計の § の本文（設計 docs/design/contract-source.md §55・行 bg・memo
//! `s2-07l.710`）。
//!
//! 本文（節の本文・done・約束の行の text）から閉じた 5 形の参照を現れた順に拾い、`行 <id>` と `#<id>` は契約表の行の
//! `section` で § に解く（行の読み手は `find_row`・§ の本文の読み手は review.rs の私有の `section_text` の 1 本）。
//! 自分の § に解けた参照は捨て、同じ § は 1 塊に畳む。並びは (g) 解けない参照の 1 塊 → (h) § の塊（指された順）→
//! (i) 束ねた § の本文だけが名指す名の塊（`mentioned_names` に 1 回だけ渡す・§ の中の参照は辿らない）。
//!
//! doc の字なしの `行 <id>` が自分の doc の表に無ければ、同じ dir の直下の tracked の契約表の置き場（`.md` の区間と
//! `.toml` の全文）を引き、ちょうど 1 置き場に在ればその行に解く（2 つ以上は数を添えた解けない参照）。`.toml` の置き場の
//! 塊は § の本文の代わりに行の goal を本文にする（§56・行 bj・memo `s2-07l.736.1` の型 7）。

use super::super::base::ITEM_HEAD;
use super::super::section_text;
use super::{data_chunks, Tree};
use crate::pipe::closure::{mentioned_names, Mentioned};
use crate::pipe::table::{self, ContractRow, Form};
use std::cell::OnceCell;

/// 参照が指す設計 doc の拡張子（`§N` の doc の字はこれだけ）。
const MD: &str = ".md";

/// 行を指す参照が doc の字として読む導出物の置き場の拡張子（`#<id>` と `行 <id>` の形だけ）。
const TOML: &str = ".toml";

/// 参照の行き先（§ の番号か契約表の行 id）。
enum Target {
    /// `§N` の N。
    Section(String),
    /// `行 <id>` / `#<id>` の id。
    Row(String),
}

/// 本文の中の参照 1 つ（`doc` は書かれた basename・`None` は契約と同じ doc）。
struct Reference {
    /// `<doc>.md` / `<file>.toml` の basename。
    doc: Option<String>,
    /// 行き先。
    target: Target,
}

impl Reference {
    /// 正規化した字面（`<doc>.md §N`・`<doc>.md 行 <id>`・`§N`・`行 <id>`）。
    fn shown(&self) -> String {
        let doc = self.doc.as_ref().map_or_else(String::new, |doc| format!("{doc} "));
        match &self.target {
            Target::Section(number) => format!("{doc}§{number}"),
            Target::Row(id) => format!("{doc}行 {id}"),
        }
    }
}

/// 解けた § 1 つ（塊 1 本）。
struct Group {
    /// 設計 doc の repo 相対 path。
    path: String,
    /// § の番号。
    section: String,
    /// § の本文。
    body: String,
    /// 行を指した参照の (id, done)（指された順・重複なし）。
    rows: Vec<(String, String)>,
}

/// 参照 1 つの解き方。
enum Resolved {
    /// 契約の自分の §（捨てる）。
    Own,
    /// 解けた §（行を指せば `rows` にその行の 1 つ）。
    Found(Group),
    /// 解けない（doc が tracked に無い・§ が無いか空・行 id が表に無い・表を読めない・goal が空）。字面の後ろに添える字
    /// （doc の字なしの行 id が同じ dir の 2 置き場以上に在れば `（同じ dir の <n> 置き場に在る）`・他は空）を持つ。
    Unresolved(String),
}

/// 同じ dir の別の契約表の置き場 1 つ（path・本文・読めた行）。
struct Place {
    /// 置き場の repo 相対 path。
    path: String,
    /// 置き場の本文。
    text: String,
    /// 置き場の契約の行。
    rows: Vec<ContractRow>,
}

/// 参照を解く場: 契約の設計 doc の path と自分の行の section・設計 doc の dir・同じ dir の別の置き場（初めて要る時に 1 回読む）。
struct Scope<'s> {
    /// 束ねる base の木。
    tree: &'s Tree<'s>,
    /// 契約の設計 doc の path。
    path: String,
    /// 契約の自分の行の section。
    section: Option<String>,
    /// 設計 doc の dir（repo の直下なら空）。
    dir: String,
    /// 同じ dir の直下の tracked の契約表の置き場（自分の doc を除く）。
    places: OnceCell<Vec<Place>>,
}

impl Scope<'_> {
    /// doc の字なしの行 id を同じ dir の別の置き場で引く: ちょうど 1 置き場に在れば (path, 本文, 行)、他は在る置き場の数。
    fn elsewhere(&self, id: &str) -> Result<(String, String, ContractRow), usize> {
        let places = self.places.get_or_init(|| self.read_places());
        let holding: Vec<(&Place, &ContractRow)> =
            places.iter().filter_map(|place| place.rows.iter().find(|row| row.id == id).map(|row| (place, row))).collect();
        match holding.as_slice() {
            [(place, row)] => Ok((place.path.clone(), place.text.clone(), (*row).clone())),
            _ => Err(holding.len()),
        }
    }

    /// 同じ dir の直下の tracked の置き場（`form_of` が読む 2 形・自分の doc を除く・読めない置き場は数えない）。
    fn read_places(&self) -> Vec<Place> {
        let candidates = self.tree.tracked.iter().filter(|path| **path != self.path && dir_of(path) == self.dir && table::form_of(path).is_ok());
        candidates
            .filter_map(|path| {
                let text = table::read(self.tree.repo, path).ok()?;
                let rows = table::read_rows(path, &text).ok()?;
                Some(Place { path: path.clone(), text, rows })
            })
            .collect()
    }
}

/// (g) → (h) → (i) の塊。`found` は契約の本文が既に名指した物（(i) から除く）。design が設計 pointer でない周は空。
pub(super) fn linked_chunks(tree: &Tree<'_>, found: &Mentioned, design: &str, bodies: &[&str]) -> Vec<String> {
    let Ok(pointer) = table::parse_pointer(design) else {
        return Vec::new();
    };
    let own = table::read(tree.repo, &pointer.path).ok().and_then(|text| table::find_row(&pointer.path, &text, &pointer.id).ok()).map(|row| row.section);
    let dir = dir_of(&pointer.path).to_owned();
    let scope = Scope { tree, path: pointer.path.clone(), section: own, dir, places: OnceCell::new() };
    let mut groups: Vec<Group> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    for reference in bodies.iter().flat_map(|body| references(body)) {
        match resolve(&scope, &reference) {
            Resolved::Own => {}
            Resolved::Unresolved(note) => {
                let shown = format!("{}{note}", reference.shown());
                if !unresolved.contains(&shown) {
                    unresolved.push(shown);
                }
            }
            Resolved::Found(new) => match groups.iter_mut().find(|group| group.path == new.path && group.section == new.section) {
                Some(group) => {
                    let fresh: Vec<(String, String)> = new.rows.into_iter().filter(|(id, _)| !group.rows.iter().any(|(known, _)| known == id)).collect();
                    group.rows.extend(fresh);
                }
                None => groups.push(new),
            },
        }
    }
    let mut chunks = Vec::new();
    if !unresolved.is_empty() {
        chunks.push(format!("{ITEM_HEAD}解けない参照: {}", unresolved.join(", ")));
    }
    let texts: Vec<String> = groups.iter().map(group_text).collect();
    chunks.extend(groups.iter().zip(&texts).map(|(group, text)| format!("{ITEM_HEAD}{} §{}\n{text}", group.path, group.section)));
    if !texts.is_empty() {
        chunks.extend(named_chunks(tree, found, &pointer.path, &texts));
    }
    chunks
}

/// § の塊の本文（頭の行を除く）: § の本文の各行を 2 字下げ（前後の空行は落とす）、行ごとの done の行を末尾に足す。
fn group_text(group: &Group) -> String {
    let lines: Vec<&str> = group.body.lines().collect();
    let start = lines.iter().position(|line| !line.trim().is_empty()).unwrap_or(lines.len());
    let end = lines.iter().rposition(|line| !line.trim().is_empty()).map_or(start, |at| at.saturating_add(1));
    let body = lines.get(start..end).unwrap_or_default().iter().map(|line| format!("  {line}").trim_end().to_owned());
    let done = group.rows.iter().map(|(id, done)| format!("  行 {id} の done: {}", done.replace('\n', " ")));
    body.chain(done).collect::<Vec<String>>().join("\n")
}

/// (i) 束ねた § の本文を名の照合に 1 回渡し、契約の本文が既に名指した file・dir を除いた残りの data file の鍵の塊だけを
/// 並べる（`doc` は契約の設計 doc＝(c) から除く）。
fn named_chunks(tree: &Tree<'_>, found: &Mentioned, doc: &str, texts: &[String]) -> Vec<String> {
    let bodies: Vec<&str> = texts.iter().map(String::as_str).collect();
    let more = mentioned_names(&bodies, &[], &tree.tracked);
    let rest = |all: Vec<String>, known: &[String]| all.into_iter().filter(|item| !known.contains(item)).collect();
    let rest = Mentioned { names: Vec::new(), files: rest(more.files, &found.files), dirs: rest(more.dirs, &found.dirs) };
    data_chunks(tree, &rest, Some(doc), &bodies.join("\n"))
}

/// 参照 1 つを § に解く。doc の字なしの行 id が自分の doc の表に無ければ同じ dir の別の置き場を引く。`.toml` の置き場は
/// 行を指せばその行の goal、`§N` なら section が N の最初の行の goal を本文にする（見出しを持たない）。
fn resolve(scope: &Scope<'_>, reference: &Reference) -> Resolved {
    let path = match &reference.doc {
        None => scope.path.clone(),
        Some(base) if scope.dir.is_empty() => base.clone(),
        Some(base) => format!("{}/{base}", scope.dir),
    };
    let text = scope.tree.tracked.contains(&path).then(|| table::read(scope.tree.repo, &path).ok()).flatten();
    let (path, text, section, row) = match &reference.target {
        Target::Section(number) => match text {
            Some(text) => (path, text, number.clone(), None),
            None => return Resolved::Unresolved(String::new()),
        },
        Target::Row(id) => match text.and_then(|text| table::find_row(&path, &text, id).ok().map(|row| (text, row))) {
            Some((text, row)) => (path, text, row.section.clone(), Some(row)),
            None if reference.doc.is_some() => return Resolved::Unresolved(String::new()),
            None => match scope.elsewhere(id) {
                Ok((path, text, row)) => (path, text, row.section.clone(), Some(row)),
                Err(0) => return Resolved::Unresolved(String::new()),
                Err(count) => return Resolved::Unresolved(format!("（同じ dir の {count} 置き場に在る）")),
            },
        },
    };
    if path == scope.path && scope.section.as_deref() == Some(section.as_str()) {
        return Resolved::Own;
    }
    let body = match (table::form_of(&path), &row) {
        (Ok(Form::Whole), Some(row)) => row.goal.clone(),
        (Ok(Form::Whole), None) => first_goal(&path, &text, &section),
        _ => section_text(&text, &section),
    };
    if body.trim().is_empty() {
        return Resolved::Unresolved(String::new());
    }
    let rows = row.map(|row| vec![(row.id, row.done)]).unwrap_or_default();
    Resolved::Found(Group { path, section, body, rows })
}

/// `.toml` の置き場で section が `section` の最初の行の goal（行が無いか表を読めなければ空）。
fn first_goal(path: &str, text: &str, section: &str) -> String {
    let rows = table::read_rows(path, text).unwrap_or_default();
    rows.into_iter().find(|row| row.section == section).map(|row| row.goal).unwrap_or_default()
}

/// path の dir（repo の直下なら空）。
fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// 本文から閉じた 5 形の参照を現れた順に拾う（`§` / `行` / `#` の字ごとに形を見る）。
fn references(text: &str) -> Vec<Reference> {
    let split = |at: usize, letter: char| (text.get(..at).unwrap_or_default(), text.get(at.saturating_add(letter.len_utf8())..).unwrap_or_default());
    text.char_indices()
        .filter_map(|(at, letter)| match (letter, split(at, letter)) {
            ('§', (before, after)) => section_at(before, after),
            ('行', (before, after)) => row_at(before, after),
            ('#', (before, after)) => pointer_at(before, after),
            _ => None,
        })
        .collect()
}

/// (A) `<doc>.md §N`・(B) `[…](<path>.md) §N`・(E) `§N`。小節の形（`§2.8`）と、前の path の字の連なりが `.md` で
/// 終わらず英数字を持つ形（`ADR-0045 §2`）は拾わない。
fn section_at(before: &str, after: &str) -> Option<Reference> {
    let number: String = after.chars().take_while(char::is_ascii_digit).collect();
    let mut rest = after.get(number.len()..).unwrap_or_default().chars();
    if number.is_empty() || (rest.next() == Some('.') && rest.next().is_some_and(|next| next.is_ascii_digit())) {
        return None;
    }
    let (prior, target) = (before.strip_suffix(' ').unwrap_or(before), Target::Section(number));
    let run = path_run(prior);
    let dest = link_dest(prior).map(|dest| dest.split('#').next().unwrap_or_default()).filter(|dest| dest.ends_with(MD));
    let head = dest.unwrap_or_else(|| run.split('#').next().unwrap_or_default());
    if head.ends_with(MD) {
        return Some(Reference { doc: basename(head), target });
    }
    (!run.chars().any(|letter| letter.is_ascii_alphanumeric())).then_some(Reference { doc: None, target })
}

/// (D) `<doc>.md 行 <id>` / `<doc>.md の行 <id>`（`.toml` も）・(E) `行 <id>`。
fn row_at(before: &str, after: &str) -> Option<Reference> {
    let id = row_id(after.strip_prefix(' ')?)?;
    let prior = before.strip_suffix('の').unwrap_or(before);
    Some(Reference { doc: basename(path_run(prior.strip_suffix(' ').unwrap_or(prior))), target: Target::Row(id) })
}

/// (C) `<doc>.md#<id>` / `<file>.toml#<id>`。
fn pointer_at(before: &str, after: &str) -> Option<Reference> {
    Some(Reference { doc: Some(basename(path_run(before))?), target: Target::Row(row_id(after)?) })
}

/// 行 id（英小文字で始まり英小文字・数字・`-` が続く・直後に英数字と `_` を持たない）。
fn row_id(text: &str) -> Option<String> {
    let id: String = text.chars().take_while(|letter| letter.is_ascii_lowercase() || letter.is_ascii_digit() || *letter == '-').collect();
    let next = text.get(id.len()..).and_then(|rest| rest.chars().next());
    let starts = id.starts_with(|letter: char| letter.is_ascii_lowercase());
    (starts && !next.is_some_and(|letter| letter.is_ascii_alphanumeric() || letter == '_')).then_some(id)
}

/// `…](<dest>)` で終わる字面のリンクの行き先。
fn link_dest(prior: &str) -> Option<&str> {
    let inner = prior.strip_suffix(')')?;
    let (head, dest) = inner.rsplit_once('(')?;
    head.ends_with(']').then_some(dest)
}

/// 末尾の path の字（英数字と `_ . / - #`）の連なり。
fn path_run(text: &str) -> &str {
    let head = text.trim_end_matches(|letter: char| letter.is_ascii_alphanumeric() || matches!(letter, '_' | '.' | '/' | '-' | '#'));
    text.get(head.len()..).unwrap_or_default()
}

/// path の basename（`.md` か `.toml` の前に字を持つものだけ・`§N` の doc の字は呼び手が `.md` に限る）。
fn basename(path: &str) -> Option<String> {
    let base = path.rsplit('/').next().unwrap_or(path);
    [MD, TOML].iter().any(|ext| base.len() > ext.len() && base.ends_with(ext)).then(|| base.to_owned())
}
