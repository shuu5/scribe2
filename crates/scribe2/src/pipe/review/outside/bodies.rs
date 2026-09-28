//! 外の材料の (j): 契約の本文が名指しの 3 形で指す write-set の中の item の本文（設計 docs/design/contract-source.md §56・
//! 行 bi・memo `s2-07l.736.1` の型 2・4）。
//!
//! 名指しの読み手と path の照合は `pipe::closure` の [`named_items`] の 1 本。探し先は write-set の `.rs` の項目のうち base に
//! 本文が在るもの（接頭辞は見ない・`+` の新設は base に無いので外れる）。宣言は本文の全行を [`declared_name`] で、impl は
//! [`impl_line`] で読み、本文は直上の doc 行と属性行から [`block_end`] まで。塊は名指し 1 つが 1 本（同じ path と名は 1 本）で、
//! (N) の名が探し先の 2 file 以上に在れば所在の 1 行だけ（§51 形 3 (a) と同じ字面）。解けない名指しは何も足さない。

use super::super::base::{block_end, declared_name, ITEM_HEAD};
use super::{doc_start, Tree, RS};
use crate::pipe::closure::{impl_line, named_items, Named};

/// impl の語（[`declared_name`] は impl を名を持つ宣言と読まないので [`impl_line`] で読む）。
const IMPL: &str = "impl";

/// 探し先の宣言 1 つ（path・本文・宣言の行の番号〔0 始まり〕）。
type Site<'t> = (&'t str, &'t str, usize);

/// (j) の塊（名指しの順・同じ path と名は最初の 1 本）。
pub(super) fn item_bodies(tree: &Tree<'_>, bodies: &[&str]) -> Vec<String> {
    let targets: Vec<(&str, &str)> = tree
        .write_set
        .iter()
        .filter(|item| item.ends_with(RS))
        .filter_map(|item| match tree.body(item) {
            Some(Ok(text)) => Some((item.as_str(), text.as_str())),
            _ => None,
        })
        .collect();
    let paths: Vec<String> = targets.iter().map(|(path, _)| (*path).to_owned()).collect();
    let mut keys: Vec<String> = Vec::new();
    let mut chunks = Vec::new();
    for chunk in named_items(bodies, &paths).iter().filter_map(|named| chunk_of(&targets, named)) {
        let head = chunk.lines().next().unwrap_or_default();
        let key = head.split_once(": ").map_or(head, |(key, _)| key).to_owned();
        if !keys.contains(&key) {
            keys.push(key);
            chunks.push(chunk);
        }
    }
    chunks
}

/// 名指し 1 つの塊: 宣言が 1 file なら頭 `- <path>#<名>: <所在>` と全部の本文、2 file 以上なら所在の 1 行（宣言が無ければ `None`）。
fn chunk_of(targets: &[(&str, &str)], named: &Named) -> Option<String> {
    let searched = targets.iter().filter(|(path, _)| named.path.as_deref().is_none_or(|want| want == *path));
    let sites: Vec<Site<'_>> = searched
        .flat_map(|&(path, text)| text.lines().enumerate().filter(|(_, line)| declares(line, named)).map(move |(at, _)| (path, text, at)))
        .collect();
    let &(first, _, _) = sites.first()?;
    let places: Vec<String> = sites.iter().map(|(path, _, at)| format!("{path}:{}", at.saturating_add(1))).collect();
    if sites.iter().any(|(path, _, _)| *path != first) {
        return Some(format!("{ITEM_HEAD}{}: 宣言 {} か所（{}）", named.name, places.len(), places.join(", ")));
    }
    let head = format!("{ITEM_HEAD}{first}#{}: {}", named.name, places.join(", "));
    let lines = sites.iter().flat_map(|&(_, text, at)| body_lines(text, at));
    Some(std::iter::once(head).chain(lines).collect::<Vec<String>>().join("\n"))
}

/// 1 行が名指しの宣言か（(F) と (N) は語と名が一致・impl は名を語に持つ impl 行・(P) は語を問わない）。
fn declares(line: &str, named: &Named) -> bool {
    let decl = || declared_name(line).and_then(|decl| decl.split_once(' ').map(|(kind, name)| (kind.to_owned(), name.to_owned())));
    match named.kind.as_deref() {
        Some(IMPL) => impl_line(line, &named.name),
        Some(kind) => decl().is_some_and(|(found, name)| found == kind && name == named.name),
        None => impl_line(line, &named.name) || decl().is_some_and(|(_, name)| name == named.name),
    }
}

/// 宣言 1 つの本文の行（直上の doc 行と属性行から閉じ括弧か `;` まで・2 字下げ）。
fn body_lines(text: &str, at: usize) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let range = lines.get(doc_start(&lines, at)..=block_end(&lines, at)).unwrap_or_default();
    range.iter().map(|line| format!("  {}", line.trim_end())).collect()
}
