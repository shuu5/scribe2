//! 外の材料の (g)〜(i): 契約の本文が指す別の設計の § の本文（設計 docs/design/contract-source.md §55・行 bg・memo
//! `s2-07l.710`）。
//!
//! 本文（節の本文・done・約束の行の text）から閉じた 5 形の参照を現れた順に拾い、`行 <id>` と `#<id>` は契約表の行の
//! `section` で § に解く（行の読み手は `find_row`・§ の本文の読み手は review.rs の私有の `section_text` の 1 本）。
//! 自分の § に解けた参照は捨て、同じ § は 1 塊に畳む。並びは (g) 解けない参照の 1 塊 → (h) § の塊（指された順）→
//! (i) 束ねた § の本文だけが名指す名の塊（`mentioned_names` に 1 回だけ渡す・§ の中の参照は辿らない）。

use super::super::base::ITEM_HEAD;
use super::super::section_text;
use super::{data_chunks, item_chunk, rs_chunks, Decl, Tree};
use crate::pipe::closure::{mentioned_names, Mentioned};
use crate::pipe::table;

/// 参照が指す doc の拡張子（設計 doc は markdown だけ）。
const MD: &str = ".md";

/// 参照の行き先（§ の番号か契約表の行 id）。
enum Target {
    /// `§N` の N。
    Section(String),
    /// `行 <id>` / `#<id>` の id。
    Row(String),
}

/// 本文の中の参照 1 つ（`doc` は書かれた basename・`None` は契約と同じ doc）。
struct Reference {
    /// `<doc>.md` の basename。
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
    /// 解けない（doc が tracked に無い・§ が無いか空・行 id が表に無い・表を読めない）。
    Unresolved,
}

/// (g) → (h) → (i) の塊。`found` は契約の本文が既に名指した物（(i) から除く）。design が設計 pointer でない周は空。
pub(super) fn linked_chunks(tree: &Tree<'_>, names: (&[Decl], &[&str]), found: &Mentioned, design: &str, bodies: &[&str]) -> Vec<String> {
    let Ok(pointer) = table::parse_pointer(design) else {
        return Vec::new();
    };
    let own = table::read(tree.repo, &pointer.path).ok().and_then(|text| table::find_row(&pointer.path, &text, &pointer.id).ok()).map(|row| row.section);
    let dir = pointer.path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut groups: Vec<Group> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    for reference in bodies.iter().flat_map(|body| references(body)) {
        match resolve(tree, (&pointer.path, own.as_deref()), dir, &reference) {
            Resolved::Own => {}
            Resolved::Unresolved => {
                let shown = reference.shown();
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
        chunks.extend(named_chunks(tree, names, found, &pointer.path, &texts));
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

/// (i) 束ねた § の本文を名の照合に 1 回渡し、契約の本文が既に名指した名・file・dir を除いた残りを §51 形 3 の
/// (a) → (b) → (c) の形で並べる（`doc` は契約の設計 doc＝(c) から除く）。
fn named_chunks(tree: &Tree<'_>, names: (&[Decl], &[&str]), found: &Mentioned, doc: &str, texts: &[String]) -> Vec<String> {
    let bodies: Vec<&str> = texts.iter().map(String::as_str).collect();
    let more = mentioned_names(&bodies, names.1, &tree.tracked);
    let rest = |all: Vec<String>, known: &[String]| all.into_iter().filter(|item| !known.contains(item)).collect();
    let rest = Mentioned { names: rest(more.names, &found.names), files: rest(more.files, &found.files), dirs: rest(more.dirs, &found.dirs) };
    let mut chunks: Vec<String> = rest.names.iter().filter_map(|name| item_chunk(tree, names.0, name)).collect();
    chunks.extend(rs_chunks(tree, &rest));
    chunks.extend(data_chunks(tree, &rest, Some(doc), &bodies.join("\n")));
    chunks
}

/// 参照 1 つを § に解く（`own` は契約の設計 doc の path と自分の行の section・`dir` は設計 doc の dir）。
fn resolve(tree: &Tree<'_>, own: (&str, Option<&str>), dir: &str, reference: &Reference) -> Resolved {
    let path = match &reference.doc {
        None => own.0.to_owned(),
        Some(base) if dir.is_empty() => base.clone(),
        Some(base) => format!("{dir}/{base}"),
    };
    let Some(text) = tree.tracked.contains(&path).then(|| table::read(tree.repo, &path).ok()).flatten() else {
        return Resolved::Unresolved;
    };
    let (section, rows) = match &reference.target {
        Target::Section(number) => (number.clone(), Vec::new()),
        Target::Row(id) => match table::find_row(&path, &text, id) {
            Ok(found) => (found.section, vec![(id.clone(), found.done)]),
            Err(_) => return Resolved::Unresolved,
        },
    };
    if path == own.0 && own.1 == Some(section.as_str()) {
        return Resolved::Own;
    }
    let body = section_text(&text, &section);
    if body.trim().is_empty() {
        return Resolved::Unresolved;
    }
    Resolved::Found(Group { path, section, body, rows })
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

/// (D) `<doc>.md 行 <id>` / `<doc>.md の行 <id>`・(E) `行 <id>`。
fn row_at(before: &str, after: &str) -> Option<Reference> {
    let id = row_id(after.strip_prefix(' ')?)?;
    let prior = before.strip_suffix('の').unwrap_or(before);
    Some(Reference { doc: basename(path_run(prior.strip_suffix(' ').unwrap_or(prior))), target: Target::Row(id) })
}

/// (C) `<doc>.md#<id>`。
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

/// path の basename（`.md` の前に字を持つものだけ）。
fn basename(path: &str) -> Option<String> {
    let base = path.rsplit('/').next().unwrap_or(path);
    (base.len() > MD.len() && base.ends_with(MD)).then(|| base.to_owned())
}
