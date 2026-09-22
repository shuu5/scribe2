//! 決定の索引と語彙を突合する（decisions-index・憲法 C8.3 / C14.2 / N4）。
//!
//! 監査 2026-09-12 塊 7（`s2-07l.165`・設計 rules-manifest.md §17）: 決定（ADR）を足した便が索引
//! `design-intent/decisions/README.html` と語彙 `design-intent/vocabulary.yaml` を更新し忘れても、
//! 着地の時点で拾う面が無かった。3 面を集合にし、片側にしか無いものを判定行へ名指す。
//!
//! - file 側: 決定 dir 直下の `ADR-` で始まる `.html`（名の昇順）。
//! - 索引側: 索引の `<a href>` のうち、決定 dir 直下の決定 file を指すもの（`./` を剥いだ file 名・
//!   `#` / `?` より後ろは落とす）。HTML の読み手は [`crate::claude_md`] の tag 読み 4 本を呼ぶ
//!   （2 本目の parser を作らない・C2）。
//! - 語彙側: 本文に現れる `ADR-` + 4 桁の id（yaml として parse しない＝id の字面だけが要る）。
//!   file 名の頭の id の集合に解けないものを名指す。
//!
//! **検出線**（`Measured` の極性＝記録・違反にしない）: fact は
//! `decisions-index=<file-only>/<index-only> file-only=<n> ids=<列> index-only=<n> ids=<列>
//! vocab-unresolved=<n> ids=<列> population=<file 数>/<索引の link 数>/<語彙の id 数>`（列は出現順の
//! `,` 区切り・0 本は `-`・母集団は distinct）。`cargo xtask check` の rc は変えない。deny 化は rules 行 +
//! 裁定 id で後の便（C5）。決定 dir を持たない木は `n/a`・索引か語彙を読めない周は `?` + 違反
//! （測れないを 0 に化けさせない・NFR4）。

use crate::check::{failed, read_text, Layout, Measured};
use crate::claude_md::{attr, read_tag, skip_ignorable, skip_raw, Tag, RAW};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// 判定行の tag。
const TAG: &str = "decisions-index";

/// 決定 dir（repo root 相対）。
pub(crate) const DECISIONS_REL: &str = "design-intent/decisions";

/// 索引 file（repo root 相対）。
pub(crate) const INDEX_REL: &str = "design-intent/decisions/README.html";

/// 語彙 file（repo root 相対）。
pub(crate) const VOCABULARY_REL: &str = "design-intent/vocabulary.yaml";

/// 決定 file の名と id の頭。
const ID_HEAD: &str = "ADR-";

/// 決定 id の数字の桁数（`ADR-0001`）。
const ID_DIGITS: usize = 4;

/// 決定 file の名の尾。
const FILE_TAIL: &str = ".html";

/// 索引の link を運ぶ要素。
const LINK_ELEMENT: &str = "a";

/// link の行き先を運ぶ属性。
const HREF_ATTR: &str = "href";

/// 同じ dir を指す href の頭（剥いで file 名にする）。
const SAME_DIR: &str = "./";

/// 片側だけの列が 0 本の周の `ids=` の値（空文字だと「無い」と「書いていない」が同じ字面）。
const NONE: &str = "-";

/// 3 面の突合の結果。
struct Index {
    /// 決定 dir に在って索引に無い file 名（名の昇順）。
    file_only: Vec<String>,
    /// 索引に在って決定 dir に無い file 名（索引の出現順）。
    index_only: Vec<String>,
    /// 語彙が名指すが決定 file に解けない id（語彙の出現順）。
    unresolved: Vec<String>,
    /// 決定 file の数。
    files: usize,
    /// 索引の決定 file への link の数（distinct）。
    links: usize,
    /// 語彙が名指す決定 id の数（distinct）。
    vocab: usize,
}

/// `cargo xtask check` の検出線。決定 dir が **NotFound** の木だけ `n/a`、他の読めない形は `?` + 違反。
pub(crate) fn measure(layout: &Layout) -> Measured {
    let files = match decision_files(&layout.root.join(DECISIONS_REL)) {
        Ok(Some(found)) => found,
        Ok(None) => {
            return Measured {
                fact: format!("{TAG}=n/a"),
                violations: Vec::new(),
            }
        }
        Err(reason) => return failed(TAG, &reason),
    };
    let html = match read_text(&layout.root.join(INDEX_REL)) {
        Ok(text) => text,
        Err(reason) => return failed(TAG, &format!("{INDEX_REL} を読めない: {reason}")),
    };
    let vocabulary = match read_text(&layout.root.join(VOCABULARY_REL)) {
        Ok(text) => text,
        Err(reason) => return failed(TAG, &format!("{VOCABULARY_REL} を読めない: {reason}")),
    };
    match index_of(&files, &html, &vocabulary) {
        Ok(found) => Measured {
            fact: fact_of(&found),
            violations: Vec::new(),
        },
        Err(reason) => failed(TAG, &reason),
    }
}

/// 決定 dir 直下の決定 file の名（昇順）。dir が無ければ `None`。
fn decision_files(dir: &Path) -> Result<Option<Vec<String>>, String> {
    let entries = match fs::read_dir(dir) {
        Ok(found) => found,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("{DECISIONS_REL} を読めない: {err}")),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("{DECISIONS_REL} の entry を読めない: {err}"))?;
        let kind = entry
            .file_type()
            .map_err(|err| format!("{DECISIONS_REL} の entry の種別を読めない: {err}"))?;
        if !kind.is_file() {
            continue;
        }
        if let Some(name) = entry.file_name().to_str().filter(|name| is_decision_file(name)) {
            out.push(name.to_owned());
        }
    }
    out.sort();
    Ok(Some(out))
}

/// 3 面を集合にして突合する。
fn index_of(files: &[String], html: &str, vocabulary: &str) -> Result<Index, String> {
    let links = distinct(&index_links(html)?);
    let vocab = distinct(&vocabulary_ids(vocabulary));
    let file_ids: Vec<&str> = files.iter().filter_map(|name| id_at(name)).collect();
    Ok(Index {
        file_only: files.iter().filter(|name| !links.contains(name)).cloned().collect(),
        index_only: links.iter().filter(|name| !files.contains(name)).cloned().collect(),
        unresolved: vocab.iter().filter(|id| !file_ids.contains(&id.as_str())).cloned().collect(),
        files: files.len(),
        links: links.len(),
        vocab: vocab.len(),
    })
}

/// 判定行の fact を組む。
fn fact_of(found: &Index) -> String {
    format!(
        "{TAG}={}/{} file-only={} ids={} index-only={} ids={} vocab-unresolved={} ids={} population={}/{}/{}",
        found.file_only.len(),
        found.index_only.len(),
        found.file_only.len(),
        listed(&found.file_only),
        found.index_only.len(),
        listed(&found.index_only),
        found.unresolved.len(),
        listed(&found.unresolved),
        found.files,
        found.links,
        found.vocab,
    )
}

/// 名の列を `,` 区切りにする（0 本は [`NONE`]）。
fn listed(names: &[String]) -> String {
    if names.is_empty() {
        NONE.to_owned()
    } else {
        names.join(",")
    }
}

/// 出現順を保って重複を落とす。
fn distinct(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        if !out.contains(name) {
            out.push(name.clone());
        }
    }
    out
}

/// 名が決定 file の形（`ADR-` で始まり `.html` で終わる）か。
fn is_decision_file(name: &str) -> bool {
    name.starts_with(ID_HEAD) && name.ends_with(FILE_TAIL)
}

/// 索引の `<a href>` のうち決定 dir 直下の決定 file を指すものの file 名を出現順に集める。
///
/// tag の切り出しは [`crate::claude_md`] の読み手を呼ぶ（comment / doctype は飛ばし、`script` /
/// `style` の中身は tag として読まない）。
fn index_links(html: &str) -> Result<Vec<String>, String> {
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
        if let Some(name) = link_target(&tag) {
            out.push(name);
        }
        if RAW.contains(&tag.name) && !tag.self_closing {
            rest = skip_raw(rest, tag.name)?;
        }
    }
    Ok(out)
}

/// 開き tag が決定 file を指す `<a href>` なら file 名を返す（他の dir を指す href は外す）。
fn link_target(tag: &Tag<'_>) -> Option<String> {
    if tag.name != LINK_ELEMENT {
        return None;
    }
    let href = attr(tag.attrs, HREF_ATTR)?;
    let path = href.split(['#', '?']).next().unwrap_or_default();
    let name = path.strip_prefix(SAME_DIR).unwrap_or(path);
    (!name.contains('/') && is_decision_file(name)).then(|| name.to_owned())
}

/// 語彙の本文に現れる決定 id（`ADR-` + 4 桁・直後に数字が続かない）を出現順に集める。
fn vocabulary_ids(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(ID_HEAD) {
        rest = rest.get(at..).unwrap_or_default();
        if let Some(id) = id_at(rest) {
            out.push(id.to_owned());
        }
        rest = rest.get(ID_HEAD.len()..).unwrap_or_default();
    }
    out
}

/// `text` の頭が決定 id（`ADR-` + 4 桁・直後に数字が続かない）ならその id を返す。
fn id_at(text: &str) -> Option<&str> {
    let digits = text.strip_prefix(ID_HEAD)?;
    let run = digits.bytes().take_while(u8::is_ascii_digit).count();
    (run == ID_DIGITS).then(|| text.get(..ID_HEAD.len().saturating_add(ID_DIGITS))).flatten()
}
