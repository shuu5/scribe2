//! 憲法 §3 の行 id と manifest の `R-*` 行を**双方向**に突合する（rules-parity・憲法 C14.2 / C1.2）。
//!
//! 全体監査 2026-09-12 塊 6（`s2-07l.164`）: 憲法 `design-intent/spec/constitution.html` の
//! `<tr id="r-…">` の行と `rules/manifest.toml` の `R-…` 行は 2 面に手で写され、片側だけの id
//! （現物: `R-C6-1` は文書だけ・`R-C4.line-width` は manifest だけ）を CI が拾わなかった。
//! 設計 rules-manifest.md §4 の畳み方で 2 面を集合にし、片側にしか無い id を判定行へ名指す。
//!
//! - 文書側: `<tr>` の `id` 属性のうち `r-` で始まるものを大文字化して `R-…` にする。HTML の
//!   読み手は [`crate::claude_md`] の tag 読み 1 本を共有する（2 本目の parser を作らない）。
//! - manifest 側: `[[rule]]` の id のうち `R-` で始まるものだけが母集団（運用行 `gate.*` /
//!   `seat.*` / `hook.*` / `fleet.*` / `pipe.*` / `runner.*` / `repo.*` 等は C 条由来でないので外）。
//!   §3 の行 id の形 `R-<条>-<番号>`（`^R-C\d+(\.\d+)?-\d+`）に一致する接頭辞だけを畳む
//!   （compound 行 `R-C4-4.fn-lines` → `R-C4-4`）。その形を持たない `R-…` 行（`R-C4.line-width`）は
//!   **自身の id のまま**置く（`R-C4` に畳まない・0 に潰さない）。
//!
//! **検出線**（`Measured` の極性＝記録・違反にしない）: fact は
//! `rules-parity=<doc-only>/<manifest-only> doc-only=<n> ids=<列> manifest-only=<n> ids=<列>
//! population=<文書側の行数>/<manifest 側 R-* の行数>`（列は出現順の `,` 区切り・0 本は `-`）。
//! `cargo xtask check` の rc は変えない。deny 化（両方向 0 を要求）は rules 行 + 裁定 id で後の便
//! （C5・C12.4・`.160` の `rules-wired` と同じ順）。憲法を持たない木（xtask の歯が組む骨格だけの
//! 木）は `n/a(no-constitution)`・読めない / 行に分けられない周は `?` + 違反（測れないを 0 に化けさせない）。

use crate::check::{failed, read_text, Layout, Measured, RULES_REL};
use crate::claude_md::{attr, read_tag, skip_ignorable, skip_raw, Tag, RAW, SOURCE_REL};
use std::fs;
use std::io::ErrorKind;

/// 判定行の tag。
const TAG: &str = "rules-parity";

/// 文書側の行を運ぶ要素。
const ROW_ELEMENT: &str = "tr";

/// 行 id を運ぶ属性。
const ID_ATTR: &str = "id";

/// 母集団に採る id の頭（文書側は小文字 `r-` で書かれ、大文字化してこの形に揃える）。
const ID_HEAD: &str = "R-";

/// 条の頭（`R-` の直後・`^R-C\d+(\.\d+)?-\d+` の `C`）。
const ARTICLE_HEAD: char = 'C';

/// compound 行の枝分かれ（`R-C4-4.fn-lines` の `.`）。
const COMPOUND_SEP: char = '.';

/// 条と番号の区切り（`R-C4-1` の 2 つ目の `-`）。
const NUMBER_SEP: char = '-';

/// 片側だけの id が 0 本の周の `ids=` の値（空文字だと「無い」と「書いていない」が同じ字面）。
const NONE: &str = "-";

/// 2 面の突合の結果。
struct Parity {
    /// 文書に在って manifest に無い id（文書の出現順）。
    doc_only: Vec<String>,
    /// manifest に在って文書に無い id（manifest の出現順・畳んだ後）。
    manifest_only: Vec<String>,
    /// 文書側の行数。
    doc_rows: usize,
    /// manifest 側の `R-*` の行数（畳む前）。
    manifest_rows: usize,
}

/// `cargo xtask check` の検出線。憲法が **NotFound** の木だけ `n/a`、他の読めない形は `?` + 違反。
pub(crate) fn measure(layout: &Layout) -> Measured {
    let html = match fs::read_to_string(layout.root.join(SOURCE_REL)) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Measured {
                fact: format!("{TAG}=n/a(no-constitution)"),
                violations: Vec::new(),
            }
        }
        Err(err) => return failed(TAG, &format!("{SOURCE_REL} を読めない: {err}")),
    };
    let manifest = match read_text(&layout.root.join(RULES_REL)) {
        Ok(text) => text,
        Err(reason) => return failed(TAG, &format!("rules manifest を読めない: {reason}")),
    };
    match parity(&html, &manifest) {
        Ok(found) => Measured {
            fact: fact_of(&found),
            violations: Vec::new(),
        },
        Err(reason) => failed(TAG, &reason),
    }
}

/// 2 面を集合にして双方向に突合する。
fn parity(html: &str, manifest: &str) -> Result<Parity, String> {
    let doc = doc_ids(html)?;
    let manifest_ids: Vec<String> = crate::rules_diff::rows(manifest)
        .map_err(|reason| format!("rules manifest を行に分けられない: {reason}"))?
        .into_iter()
        .filter(|row| row.id.starts_with(ID_HEAD))
        .map(|row| fold(&row.id))
        .collect();
    let doc_set = distinct(&doc);
    let manifest_set = distinct(&manifest_ids);
    Ok(Parity {
        doc_only: doc_set.iter().filter(|id| !manifest_set.contains(id)).cloned().collect(),
        manifest_only: manifest_set.iter().filter(|id| !doc_set.contains(id)).cloned().collect(),
        doc_rows: doc.len(),
        manifest_rows: manifest_ids.len(),
    })
}

/// 判定行の fact を組む。
fn fact_of(found: &Parity) -> String {
    format!(
        "{TAG}={}/{} doc-only={} ids={} manifest-only={} ids={} population={}/{}",
        found.doc_only.len(),
        found.manifest_only.len(),
        found.doc_only.len(),
        listed(&found.doc_only),
        found.manifest_only.len(),
        listed(&found.manifest_only),
        found.doc_rows,
        found.manifest_rows,
    )
}

/// id の列を `,` 区切りにする（0 本は [`NONE`]）。
fn listed(ids: &[String]) -> String {
    if ids.is_empty() {
        NONE.to_owned()
    } else {
        ids.join(",")
    }
}

/// 出現順を保って重複を落とす。
fn distinct(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

/// 憲法 HTML の `<tr id="r-…">` の id を出現順に集め、大文字の `R-…` に揃える。
///
/// tag の切り出しは [`crate::claude_md`] の読み手を呼ぶ（comment / doctype は飛ばし、`script` /
/// `style` の中身は tag として読まない＝憲法の script 本文に在る `<` で落ちない）。
fn doc_ids(html: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find('<') {
        rest = rest.get(at..).unwrap_or_default();
        if let Some(after) = skip_ignorable(rest)? {
            rest = after;
            continue;
        }
        let (tag, after) = read_tag(rest)?;
        rest = after;
        if tag.closing {
            continue;
        }
        if let Some(id) = row_id(&tag) {
            out.push(id);
        }
        if RAW.contains(&tag.name) && !tag.self_closing {
            rest = skip_raw(rest, tag.name)?;
        }
    }
    Ok(out)
}

/// 開き tag が `<tr id="r-…">` なら大文字化した id を返す。
fn row_id(tag: &Tag<'_>) -> Option<String> {
    if tag.name != ROW_ELEMENT {
        return None;
    }
    let id = attr(tag.attrs, ID_ATTR)?.to_ascii_uppercase();
    id.starts_with(ID_HEAD).then_some(id)
}

/// manifest の id を §3 の行 id へ畳む。
///
/// `^R-C\d+(\.\d+)?-\d+` に一致する接頭辞を持ち、その後ろが空か `.`（compound の枝）なら接頭辞へ
/// 畳む。それ以外（`R-C4.line-width`・`R-C4-` 等）は自身の id のまま返す。
fn fold(id: &str) -> String {
    match section_prefix_len(id) {
        Some(len) if id.get(len..).is_some_and(|tail| tail.is_empty() || tail.starts_with(COMPOUND_SEP)) => {
            id.get(..len).unwrap_or(id).to_owned()
        }
        _ => id.to_owned(),
    }
}

/// `^R-C\d+(\.\d+)?-\d+` に一致する接頭辞の byte 長（一致しなければ `None`）。
fn section_prefix_len(id: &str) -> Option<usize> {
    let mut rest = id.strip_prefix(ID_HEAD)?.strip_prefix(ARTICLE_HEAD)?;
    let mut len = ID_HEAD.len().saturating_add(ARTICLE_HEAD.len_utf8());
    let article = digits_len(rest);
    if article == 0 {
        return None;
    }
    len = len.saturating_add(article);
    rest = rest.get(article..)?;
    if let Some(sub) = rest.strip_prefix(COMPOUND_SEP) {
        let sub_len = digits_len(sub);
        if sub_len > 0 {
            len = len.saturating_add(COMPOUND_SEP.len_utf8()).saturating_add(sub_len);
            rest = sub.get(sub_len..)?;
        }
    }
    let after_sep = rest.strip_prefix(NUMBER_SEP)?;
    let number = digits_len(after_sep);
    if number == 0 {
        return None;
    }
    Some(len.saturating_add(NUMBER_SEP.len_utf8()).saturating_add(number))
}

/// 先頭の ASCII 数字の連なりの長さ。
fn digits_len(text: &str) -> usize {
    text.bytes().take_while(u8::is_ascii_digit).count()
}
