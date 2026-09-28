//! 行の新しい歯の接頭辞が他の Declared 行の verify の filter 語を含み、その行の歯の置き場を write-set の外へ広げる衝突の
//! 予想（設計 docs/design/contract-source.md §54・契約表の行 bf・memo `s2-07l.708`）。
//!
//! 着地の前の行（write-set の `+` か `creates` に base の tracked に無い path を持つ行）の verify の nextest 行ごとに最後の
//! filter 語を読み、base の `.rs` の本文の全体で 0 本の語（新しい語）の置き場の候補を組み、候補の全部が他の Declared 行の
//! write-set の外に解ける衝突を [`Predicted`] で返す。読み手は [`teeth_places`] と親の `teeth_outside` の 2 本だけで、
//! 仮の本文を渡して撃つ（2 本目の読み手を作らない）。[`predict`] は親の `judge_repo` が全 doc から 1 回だけ撃ち、当たりは
//! 置き場の検出線（親の `Places`）が既存の `teeth-outside-write-set` の 1 件に [`merge`] で併せる。

use super::super::{read_table, Context, ContractRow};
use super::{is_declared, read, teeth_outside};
use crate::name::NAME;
use crate::pipe::closure::{teeth_places, teeth_words, test_region, Base, ClosureError, Fields};
use crate::pipe::refuse::{normalize, NEW_FILE, SHRINK_FILE};
use std::collections::BTreeSet;
use std::path::Path;

/// src の歯の区間の印（`test_region` が読む行頭の印）。新しい語の読みは各 file の本文の前にこれを置いた写しで撃ち
/// （`#[path]` の子 module の歯の file の既存の歯も数える・§54 形 1）、仮の歯の本文も同じ印から始める。
const TEST_MARK: &str = "#[cfg(test)]";

/// crate の file の置き場（`crates/<crate>/`）。
const CRATES_DIR: &str = "crates/";

/// module の path が始まる crate の dir（`src/` と `tests/` の後ろの段・§54 形 3）。
const MODULE_ROOTS: &[&str] = &["src/", "tests/"];

/// module の path の段に数えない名（crate の root と dir の module の file）。
const ROOT_SEGMENTS: &[&str] = &["mod", "lib", "main"];

/// Rust の file の拡張子。
const RS: &str = ".rs";

/// 予想の当たり 1 件（他の Declared 行 1 本 × 新しい語 1 つ）。
pub(super) struct Predicted {
    /// 当たった行を持つ設計 doc（repo 相対）。
    doc: String,
    /// 当たった行の見出しの行番号。
    line: u64,
    /// 置き場の候補（全部がその行の write-set の外に解けた file・辞書順）。
    files: Vec<String>,
    /// `--verbose` の知らせの行の末尾（予想の出所と直し方）。
    tail: String,
}

/// 予想の材料（base の事実・本文の列・全 doc の行）。
struct Ground<'g> {
    /// base の tree の事実（core の crate は器の名）。
    base: Base<'g>,
    /// base の `.rs` の本文。
    texts: &'g [(&'g str, &'g str)],
    /// base の `.rs` の本文の前に歯の区間の印を置いた写し（新しい語の読み）。
    whole: &'g [(&'g str, &'g str)],
    /// (doc, 区間の行) の列（doc 順・区間を読めない doc は数えない）。
    tables: &'g [(&'g str, Vec<ContractRow>)],
}

/// 新しい語の出所（語を持つ行の doc・行 id・語）。
struct Origin<'o> {
    /// 語を持つ行の設計 doc。
    doc: &'o str,
    /// 語を持つ行の id。
    id: &'o str,
    /// 新しい語。
    word: &'o str,
}

/// 全 doc の行から衝突を予想する（§54 形 1〜4・doc 順・行の順）。閉包の入力を読めない周は 0 件（置き場の検出線が
/// `?` を名乗る）。
pub(super) fn predict(repo: &Path, docs: &[&String], ctx: &Context<'_>) -> Vec<Predicted> {
    let read_texts: Option<Vec<(&str, &str)>> =
        ctx.sources.iter().map(|source| source.body.as_deref().ok().map(|body| (source.path.as_str(), body))).collect();
    let Some(texts) = read_texts else {
        return Vec::new();
    };
    let marked: Vec<(&str, String)> = texts.iter().map(|(path, text)| (*path, format!("{TEST_MARK}\n{text}"))).collect();
    let whole: Vec<(&str, &str)> = marked.iter().map(|(path, text)| (*path, text.as_str())).collect();
    let tables: Vec<(&str, Vec<ContractRow>)> = docs
        .iter()
        .filter_map(|doc| {
            let text = read(repo, doc).ok()?;
            read_table(doc, &text).ok().map(|(rows, _)| (doc.as_str(), rows))
        })
        .collect();
    let base = Base { sources: ctx.sources, snapshots: ctx.snapshots, tracked: ctx.tracked, core_crate: NAME };
    let ground = Ground { base, texts: &texts, whole: &whole, tables: &tables };
    let mut found = Vec::new();
    for (doc, rows) in &tables {
        for row in rows.iter().filter(|row| fresh(row, ctx.tracked)) {
            for line in &row.verify {
                found.extend(from_line(&ground, doc, row, std::slice::from_ref(line)));
            }
        }
    }
    found
}

/// 当たった行（doc・見出しの行番号）の予想を `files` に併せ、知らせの行の末尾を返す（当たりが無ければ空＝出力は不変）。
pub(super) fn merge(predicted: &[Predicted], doc: &str, line: u64, files: &mut BTreeSet<String>) -> String {
    let mut tail = String::new();
    for hit in predicted.iter().filter(|hit| hit.doc == doc && hit.line == line) {
        files.extend(hit.files.iter().cloned());
        if !tail.contains(&hit.tail) {
            tail.push_str(&hit.tail);
        }
    }
    tail
}

/// 着地の前の行か（write-set の `+` の項目か `creates` のうち base の tracked に無い path を 1 つ以上持つ・§54 形 1）。
fn fresh(row: &ContractRow, tracked: &[String]) -> bool {
    let plus = row.write_set.iter().filter_map(|item| item.strip_prefix(NEW_FILE));
    plus.chain(row.creates.iter().map(String::as_str)).any(|path| !tracked.contains(&normalize(path)))
}

/// verify の 1 行 `one` の新しい語から、他の Declared 行（doc を跨ぐ・自分の行を除く）への当たりを組む。
fn from_line(ground: &Ground<'_>, doc: &str, row: &ContractRow, one: &[String]) -> Vec<Predicted> {
    let Some(word) = new_word(one, ground) else {
        return Vec::new();
    };
    let places = candidates(row, one, word, ground);
    if places.is_empty() {
        return Vec::new();
    }
    let origin = Origin { doc, id: &row.id, word };
    let mut found = Vec::new();
    for (at, others) in ground.tables {
        for other in others.iter().filter(|other| is_declared(other) && !(*at == doc && other.id == row.id)) {
            let Some(filters) = collides(other, word, &places, &ground.base) else {
                continue;
            };
            let tail = tail(&origin, other, &places, &filters, ground.base.tracked);
            found.push(Predicted { doc: (*at).to_owned(), line: other.line, files: places.clone(), tail });
        }
    }
    found
}

/// 行 `one` の最後の filter 語（`teeth_places` と同じ読み）が識別子の形で、その行の scope の base の本文の全体に語を
/// 含む `#[test]` の fn が 0 本なら語（§54 形 1）。
fn new_word<'l>(one: &'l [String], ground: &Ground<'_>) -> Option<&'l str> {
    let word = teeth_words(one).first().copied()?;
    let unresolved = matches!(teeth_places(&only(one), &ground.base, ground.whole), Err(ClosureError::TeethPlaceUnresolved { .. }));
    (is_ident(word) && unresolved).then_some(word)
}

/// 置き場の候補（§54 形 2・形 3）: `tests` 欄が在ればその項目、無ければ write-set（`-` の項目を除く・印は剥がす）と
/// `creates` の `.rs` のうち base に無い新規 file か base の歯の区間が空でない file で、`one` の scope に入るものを
/// module の path の語で絞る。scope は各候補に語の名の歯 1 本を置いた仮の本文を [`teeth_places`] に渡して解く。
fn candidates(row: &ContractRow, one: &[String], word: &str, ground: &Ground<'_>) -> Vec<String> {
    if !row.tests.is_empty() {
        return row.tests.clone();
    }
    let items = row.write_set.iter().filter(|item| !item.starts_with(SHRINK_FILE)).chain(&row.creates);
    let placeable: BTreeSet<String> = items
        .map(|item| normalize(item))
        .filter(|path| path.ends_with(RS))
        .filter(|path| {
            !ground.base.tracked.contains(path)
                || ground.texts.iter().any(|(at, text)| *at == path.as_str() &&!test_region(at, text).is_empty())
        })
        .collect();
    let body = tooth(word);
    let texts: Vec<(&str, &str)> = placeable.iter().map(|path| (path.as_str(), body.as_str())).collect();
    let scoped = teeth_places(&only(one), &ground.base, &texts).unwrap_or_default();
    narrow(placeable.into_iter().filter(|path| scoped.contains(path)).collect(), word)
}

/// module の path の語で絞る（§54 形 3）: 一致の段数が最も多い file だけを残し、一致が無ければ全部を残す。
fn narrow(files: Vec<String>, word: &str) -> Vec<String> {
    let best = files.iter().map(|path| depth(path, word)).max().unwrap_or(0);
    if best == 0 {
        return files;
    }
    files.into_iter().filter(|path| depth(path, word) == best).collect()
}

/// `path` の module の path の末尾の段の連なりを `_` で繋いだ字面が、語と等しいか語の先頭に `_` の境で一致する最大の
/// 段数（一致が無ければ 0）。
fn depth(path: &str, word: &str) -> usize {
    let segments = module_path(path);
    (1..=segments.len())
        .filter(|count| {
            let joined = segments.get(segments.len().saturating_sub(*count)..).unwrap_or_default().join("_");
            word == joined || word.strip_prefix(joined.as_str()).is_some_and(|rest| rest.starts_with('_'))
        })
        .max()
        .unwrap_or(0)
}

/// module の path の段（`crates/<crate>/src/` か `crates/<crate>/tests/` より後ろ・末尾の `.rs` を落とし `mod` / `lib` /
/// `main` の段は数えない）。crate の外の file は空。
fn module_path(path: &str) -> Vec<&str> {
    let Some((_, rest)) = path.strip_prefix(CRATES_DIR).and_then(|rest| rest.split_once('/')) else {
        return Vec::new();
    };
    let Some(inner) = MODULE_ROOTS.iter().find_map(|root| rest.strip_prefix(root)) else {
        return Vec::new();
    };
    inner.strip_suffix(RS).unwrap_or(inner).split('/').filter(|segment| !ROOT_SEGMENTS.contains(segment)).collect()
}

/// 他の Declared 行 `other` に、残った候補 1 つずつに語の名の歯 1 本だけを持つ仮の本文を `teeth_outside` で解き、全部の
/// 候補が write-set の外に解けた周だけ当てた filter 語の集合（§54 形 4）。1 つでも外に解けなければ `None`。
fn collides(other: &ContractRow, word: &str, places: &[String], base: &Base<'_>) -> Option<BTreeSet<String>> {
    let body = tooth(word);
    let mut filters = BTreeSet::new();
    for place in places {
        let texts = [(place.as_str(), body.as_str())];
        if teeth_outside(other, base, &texts).is_empty() {
            return None;
        }
        for line in &other.verify {
            let single = ContractRow { verify: vec![line.clone()], ..other.clone() };
            if !teeth_outside(&single, base, &texts).is_empty() {
                filters.extend(teeth_words(std::slice::from_ref(line)).into_iter().map(str::to_owned));
            }
        }
    }
    Some(filters)
}

/// 知らせの行の末尾（§54 形 5）: 予想の出所（doc・行 id・語）と直し方（当たった行の write-set に file を足す〔base に
/// 無い file は `+`〕か、語を当たった filter 語を含まない語に変える）。
fn tail(origin: &Origin<'_>, other: &ContractRow, files: &[String], filters: &BTreeSet<String>, tracked: &[String]) -> String {
    let items: Vec<String> =
        files.iter().map(|file| if tracked.contains(file) { file.clone() } else { format!("{NEW_FILE}{file}") }).collect();
    let filters: Vec<&str> = filters.iter().map(String::as_str).collect();
    format!(
        " — 予想: {} 行 {} の新しい語 {}・直し方: 行 {} の write-set に {} を足すか、語を {} を含まない語に変える",
        origin.doc,
        origin.id,
        origin.word,
        other.id,
        items.join(", "),
        filters.join(" / ")
    )
}

/// verify の 1 行だけを持つ導出の材料（他の欄は空＝`tests` 欄で解けたことにしない）。
fn only(one: &[String]) -> Fields<'_> {
    Fields { touches: &[], surfaces: &[], verify: one, creates: &[], tests: &[], also: &[], files: &[] }
}

/// 語の名の歯 1 本だけを持つ仮の本文（src の file でも歯の区間に入るよう印から始める）。
fn tooth(word: &str) -> String {
    format!("{TEST_MARK}\n#[test]\nfn {word}() {{}}\n")
}

/// 識別子の形（英字か `_` で始まり、英数字と `_` だけ）。`::` を含む module path の filter は数えない。
fn is_ident(word: &str) -> bool {
    word.chars().next().is_some_and(|head| head.is_ascii_alphabetic() || head == '_')
        && word.chars().all(|found| found.is_ascii_alphanumeric() || found == '_')
}
