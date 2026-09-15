//! write-set の閉包（設計 docs/design/contract-source.md §3・ADR-0023 §2.3・ADR-0013 §2.2・SRS FR48）。
//!
//! 契約表の行が `touches` に宣言した閉じた型（`crate::module::Type`）について、その型を**構造として
//! 持つ file** を字面走査で集める。**pure**（I/O は呼び手）で、入力は型名の列と読み込んだ `.rs` の
//! (path, 本文) の列だけである。拾う形は 4 つ:
//!
//! 1. **literal 構築** `Type {`（直前が `struct` / `enum` / `impl` / `for` 等の語か `->` の行は宣言・実装・
//!    戻り型なので除く）
//! 2. **match の arm**（`Type::` を `=>` の左に持つ行）
//! 3. **件数 pin**（その型の const slice `NAME` の `NAME.len()` が整数 literal と `,` / `==` / `!=` で突き合わ
//!    される箇所）。件数を文言へ写すだけの `.len()` は型を足しても壊れないので pin に数えない。`NAME` の出現は
//!    `<module>::NAME` の修飾か、`use` で `<module>::` から同名で取り込んだ file でだけ解く（別の module の同名
//!    const を拾わない）
//! 4. **const slice の宣言 file**（`const NAME: &[Type]`）
//!
//! 4 形はどれも、先に**その file から `touches` の型が見えているか**（[`sees`]・§3「閉包の同名衝突」）を 1 関数で
//! 判定してから数える: (a) file が型を宣言し path が `touches` の module に当たる (b) `use <module>::Name` で取り込む
//! (c) 本文に `<module>::Name` の修飾が在る。裸の型名だけで照合すると、別 module の同名の型（`hook::vessel::Marker` と
//! `seat::rebrief::Marker`）の file へ閉包が広がり、導出値の偽の交差が並列度を下げる。
//!
//! **下界である**（§3「限界」）: 型の名が別名で現れる形（`use … as`・generic の中）と `Self { … }` の構築、glob
//! （`use m::*` / `use super::*`）越しの取り込みは見ない。上界は構文木が要り A3 の依存になる（却下・§11）。読めない
//! file と型名の形の違いは `Err`（fail-closed・NFR4）。
//!
//! 閉包の拡張（契約 (g)・§3）も同じ pure な字面走査で持つ: (v) **外形 pin** [`surface_closure`]（`surfaces` の名が
//! 指す外形 snapshot の file と、その名か subcommand の usage 文字列を歯の区間に literal で持つ `.rs`）と
//! **名指しの実在** [`unresolved_names`]（backtick の中身のうち path 形 / 型の path 形 / fn 形だけを名指しと読み、
//! base に解けないものを全件返す）。
//!
//! **write-set の導出**（契約 (h)・§3「write-set の導出」）[`derive_write_set`] は行の欄（`touches` / `verify` /
//! `surfaces` / `creates` / `tests` / `also`）から write-set を**導出値**として作る = 閉包 ∪ 歯の置き場（base の
//! `#[test]` の fn 名が verify の filter 語を含む file）∪ 外形 pin ∪ 新規 file ∪ Rust の外の file。手書きの
//! write-set は [`check_drift`] で導出値との集合一致だけを認める（接頭辞 `+` は剥がして比べる）。

use super::refuse::{normalize, NEW_FILE};
use std::collections::BTreeSet;

/// nextest の行の書き出し（この後ろの語から crate と filter 語を読む）。
const NEXTEST_HEAD: &[&str] = &["cargo", "nextest", "run"];

/// nextest の行で crate を選ぶ flag（次の語が crate の名）。
const PACKAGE_FLAGS: &[&str] = &["-p", "--package"];

/// 歯の印（この行の直下の `fn` が歯・helper の fn は数えない）。
const TEST_ATTR: &str = "#[test]";

/// crate の置き場（`crates/<crate>/` 配下がその crate の file・上限の余地の `core_of` と同じ規約）。
const CRATES_DIR: &str = "crates/";

/// 外形 snapshot の置き場（repo 相対 path の中の dir・`crates/<c>/src/snapshots/` と `crates/<c>/tests/e2e/snapshots/`）。
const SNAPSHOT_DIRS: &[&str] = &["src/snapshots/", "tests/e2e/snapshots/"];

/// 外形 snapshot の拡張子（insta の `<crate>__<module>__<歯の名>.snap`・名は末尾の `__` の後）。
const SNAPSHOT_EXT: &str = ".snap";

/// usage 行の書き出し（この後ろの名〔`{NAME}` / `{}` の穴を飛ばした最初の語〕が subcommand の名）。
const USAGE_HEAD: &str = "usage: ";

/// src の歯の区間の始まり（行頭・以後 file 末尾まで・xtask の test-src-ratio と同じ印）。
const TEST_MARK: &str = "#[cfg(test)]";

/// 歯の file を置く dir の名（path の段に持てば file 全体が歯の区間）。
const TESTS_DIR: &str = "tests";

/// path 形の名指しに使える文字（英数字と `_ . / -`）。
const PATH_CHARS: &[char] = &['_', '.', '/', '-'];

/// path 形の名指しの拡張子。
const RS: &str = ".rs";

/// `use` 文の書き出し（複数行に跨ぐ `use a::{…};` は `;` まで繋げて 1 文として読む）。
const USE_HEADS: &[&str] = &["use ", "pub use ", "pub(crate) use "];

/// `Type {` の直前にこの語が在る行は宣言・実装の行である（literal 構築ではない）。
const DECLARING: &[&str] = &["struct", "enum", "union", "trait", "impl", "for"];

/// 型の宣言の語（`enum Name` / `struct Name` の行＝[`sees`] の (a)「この file がその型を宣言する」）。
const TYPE_DECLARING: &[&str] = &["enum", "struct"];

/// src の dir 名（[`sees`] の (a) は path をこの段からの相対で module に当てる・`crates/<c>/` の接頭辞は任意）。
const SRC_DIR: &str = "src";

/// `crate` 直下の型（`crate::Type`・module の無い型）の修飾の段の名。
const CRATE_ROOT: &str = "crate";

/// `crate` 直下の型を宣言する file の stem（`lib.rs` / `main.rs`）。
const CRATE_ROOT_STEMS: &[&str] = &["lib", "main"];

/// 親 module の修飾の段（`<module>/` の直下の子 file から `use super::Name` / `super::Name` は `<module>::Name` を指す）。
const SUPER: &str = "super";

/// dir 形の module 自身の file の stem（`<module>/mod.rs`・この file の `super` は親を指す）。
const MOD_STEM: &str = "mod";

/// 件数 pin の突き合わせの口（`assert_eq!(NAME.len(), 3` の `,`・`NAME.len() == 3` の `==`）。
const COMPARES: &[&str] = &[",", "==", "!="];

/// 読み込んだ `.rs` 1 本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// repo 相対 path。
    pub path: String,
    /// 本文。読めなかった周は理由（「その型を持たない」に読み替えない）。
    pub body: Result<String, String>,
}

/// 閉包を求められない理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosureError {
    /// `touches` の項目が `crate::module::Type` の形でない。
    TypeForm {
        /// 書かれていた字面。
        name: String,
    },
    /// `.rs` を読めない。
    Unreadable {
        /// 読めなかった file の repo 相対 path。
        path: String,
        /// 読めなかった理由。
        reason: String,
    },
    /// `surfaces` の名が外形 snapshot の名にも usage を持つ subcommand の名にも無い。
    SurfaceUnknown {
        /// 書かれていた名。
        name: String,
    },
    /// 手書きの write-set が導出値と集合として一致しない（§3「手書きの write-set の扱い」・不足と余分を全部持つ）。
    WriteSetDrift {
        /// 導出値に在って手書きに無い項目（正規化した path・辞書順）。
        missing: Vec<String>,
        /// 手書きに在って導出値に無い項目（正規化した path・辞書順）。
        extra: Vec<String>,
    },
    /// verify の filter 語を含む `#[test]` の fn が base に 1 本も無く、`tests` 欄も無い（歯の置き場を解けない）。
    TeethPlaceUnresolved {
        /// 解けなかった filter 語。
        filter: String,
    },
    /// `also` に `.rs` が書かれた（Rust の面は `touches` と `tests` から導く）。
    AlsoNamesRust {
        /// 書かれていた項目。
        item: String,
    },
    /// `tests` の項目が歯の file（`tests/` 配下か test 区間を持つ `.rs`）でない。
    TestsNotATeethFile {
        /// 書かれていた項目。
        item: String,
    },
    /// `creates` / `tests` / `also` の項目が base に解けない（`creates` は `+` 無しで base に無い path・`tests` /
    /// `also` は base に在る file）。
    ItemUnresolved {
        /// 書かれていた項目。
        item: String,
    },
}

impl ClosureError {
    /// 断る理由の 1 行。
    pub fn reason(&self) -> String {
        match *self {
            Self::TypeForm { ref name } => format!("touches の {name} が crate::module::Type の形でない"),
            Self::Unreadable { ref path, ref reason } => format!("閉包を測る {path} を読めない: {reason}"),
            Self::SurfaceUnknown { ref name } => {
                format!("surfaces の {name} は外形 snapshot の名にも usage を持つ subcommand の名にも無い")
            }
            Self::WriteSetDrift { ref missing, ref extra } => {
                format!("write-set が導出値と一致しない（missing: {} / extra: {}）", listed(missing), listed(extra))
            }
            Self::TeethPlaceUnresolved { ref filter } => {
                format!("verify の filter 語 {filter} を含む #[test] の fn が base に無く tests 欄も無い（歯の置き場を解けない）")
            }
            Self::AlsoNamesRust { ref item } => format!("also の {item} は .rs である（Rust の面は touches と tests から導く）"),
            Self::TestsNotATeethFile { ref item } => {
                format!("tests の {item} は歯の file でない（tests/ 配下か src/ の test 区間を持つ .rs だけ）")
            }
            Self::ItemUnresolved { ref item } => {
                format!("{item} は base に解けない（creates は + 無しで base に無い path・tests / also は base に在る file）")
            }
        }
    }
}

/// 項目の列を 1 行に並べる（空は `-`）。
fn listed(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_owned()
    } else {
        items.join(", ")
    }
}

/// 読める本文の列にする（読めない file が 1 本でも在れば `Err`・fail-closed）。
fn texts_of(sources: &[Source]) -> Result<Vec<(&str, &str)>, ClosureError> {
    sources
        .iter()
        .map(|source| match source.body {
            Ok(ref text) => Ok((source.path.as_str(), text.as_str())),
            Err(ref reason) => Err(ClosureError::Unreadable { path: source.path.clone(), reason: reason.clone() }),
        })
        .collect()
}

/// 外形 pin（第 5 形・§3）: `surfaces` の各名について、外形 snapshot の file と、その snapshot 名か subcommand の
/// usage 文字列を歯の区間に literal で持つ `.rs` を集める（path の辞書順）。
///
/// 名は 2 種: (a) `snapshots` の file 名の末尾（`<crate>__<module>__<名>.snap` の `<名>`）(b) `sources` の
/// `usage: ` の行が名乗る subcommand（`{NAME}` / `{}` の穴を飛ばした最初の語）。どちらにも無い名は
/// [`ClosureError::SurfaceUnknown`]。宣言の無い行は呼ばれない（費用を掛けない）。
pub fn surface_closure(
    surfaces: &[String],
    sources: &[Source],
    snapshots: &[Source],
) -> Result<BTreeSet<String>, ClosureError> {
    let (texts, snaps) = (texts_of(sources)?, texts_of(snapshots)?);
    let usages = usages(&texts);
    let mut found = BTreeSet::new();
    for name in surfaces {
        let named: Vec<&str> = snaps.iter().filter(|(path, _)| snapshot_name(path) == Some(name)).map(|(path, _)| *path).collect();
        let literal = match (named.is_empty(), usages.iter().find(|(sub, _)| sub == name)) {
            (false, _) => name.as_str(),
            (true, Some((_, usage))) => usage.as_str(),
            (true, None) => return Err(ClosureError::SurfaceUnknown { name: name.clone() }),
        };
        found.extend(named.iter().map(|path| (*path).to_owned()));
        found.extend(snaps.iter().filter(|(_, text)| text.contains(literal)).map(|(path, _)| (*path).to_owned()));
        found.extend(
            texts.iter().filter(|(path, text)| test_region(path, text).contains(literal)).map(|(path, _)| (*path).to_owned()),
        );
    }
    Ok(found)
}

/// 外形 snapshot の名（置き場の dir に在る `.snap` の file 名の末尾の `__` の後）。置き場の外は `None`。
fn snapshot_name(path: &str) -> Option<&str> {
    let placed = SNAPSHOT_DIRS.iter().any(|dir| path.contains(dir));
    let stem = path.rsplit('/').next()?.strip_suffix(SNAPSHOT_EXT)?;
    placed.then(|| stem.rsplit("__").next().unwrap_or(stem))
}

/// `usage: ` の行が名乗る (subcommand の名, usage 文字列〔名から literal の終わりまで〕) の列。
fn usages(texts: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (_, text) in texts {
        for line in text.lines() {
            let Some((_, rest)) = line.split_once(USAGE_HEAD) else {
                continue;
            };
            let literal = rest.split('"').next().unwrap_or_default();
            let mut words = literal.split_whitespace();
            let first = words.next().unwrap_or_default();
            let name = if first.starts_with('{') { words.next().unwrap_or_default() } else { first };
            if name.is_empty() || !name.chars().all(|found| is_ident_char(found) || found == '-') {
                continue;
            }
            if let Some(at) = literal.find(name) {
                found.push((name.to_owned(), literal.get(at..).unwrap_or_default().to_owned()));
            }
        }
    }
    found
}

/// 歯の区間: `tests` dir 配下の file は全体・src の file は行頭の `#[cfg(test)]` から末尾（無ければ空）。
fn test_region<'t>(path: &str, text: &'t str) -> &'t str {
    if path.split('/').any(|segment| segment == TESTS_DIR) {
        return text;
    }
    let start = if text.starts_with(TEST_MARK) {
        Some(0)
    } else {
        text.find(&format!("\n{TEST_MARK}")).map(|at| at.saturating_add(1))
    };
    start.and_then(|at| text.get(at..)).unwrap_or_default()
}

/// 名指しの実在（§3）: `texts` の各 (在り処, 本文) の backtick の中身のうち **path 形 / 型の path 形 / fn 形**だけを
/// 名指しと読み、base に解けないものを (名, 在り処) で**全件**返す（書かれていた順）。
///
/// (1) path 形（英数字と `_ . / -` だけ・拡張子 `.rs`）は `tracked` の path と等しいか `/` 区切りの末尾一致、または
/// その行の write-set の `+` 項目（接頭辞を剥がした path）と同じ照合で解ける。(2) 型の path 形（`::` で結んだ識別子の
/// 列）は末尾 2 節「型::項目」が `sources` に現れれば解ける。**`touches` に宣言した型の variant は名指しと読まない**
/// （未来の variant は `touches` が説明する）。(3) fn 形（識別子 + `(`〔`)` は任意〕）は `fn 識別子` の宣言が在れば
/// 解ける。一致しない字面（struct literal・field 付き variant・glob・属性・散文）は名指しではない。別名・generic は
/// 下界の外。
pub fn unresolved_names(
    texts: &[(String, String)],
    touches: &[String],
    write_set: &[String],
    tracked: &[String],
    sources: &[Source],
) -> Result<Vec<(String, String)>, ClosureError> {
    let bodies = texts_of(sources)?;
    let new_files: Vec<&str> = write_set.iter().filter_map(|item| item.strip_prefix('+')).collect();
    let touched: Vec<&str> = touches.iter().filter_map(|raw| raw.rsplit("::").next()).collect();
    let mut found = Vec::new();
    for (at, text) in texts {
        for name in backticked(text) {
            let resolved = match form_of(name, &touched) {
                Form::Path => tracked.iter().map(String::as_str).chain(new_files.iter().copied()).any(|path| path_matches(path, name)),
                Form::Type(tail) => bodies.iter().any(|(_, body)| holds_word(body, &tail)),
                Form::Fn(ident) => bodies.iter().any(|(_, body)| declares_fn(body, &ident)),
                Form::Prose => true,
            };
            if !resolved {
                found.push((name.to_owned(), at.clone()));
            }
        }
    }
    Ok(found)
}

/// backtick の中身の形。
enum Form {
    /// path 形。
    Path,
    /// 型の path 形（末尾 2 節「型::項目」）。
    Type(String),
    /// fn 形（識別子）。
    Fn(String),
    /// 名指しではない字面。
    Prose,
}

/// backtick の中身を 3 形に分ける（`touched` の型の variant は散文扱い）。
fn form_of(name: &str, touched: &[&str]) -> Form {
    let stem = name.rsplit('/').next().unwrap_or(name).strip_suffix(RS);
    if name.chars().all(|found| found.is_ascii_alphanumeric() || PATH_CHARS.contains(&found)) && stem.is_some_and(|stem| !stem.is_empty()) {
        return Form::Path;
    }
    let segments: Vec<&str> = name.split("::").collect();
    if let Some((item, head)) = segments.split_last().filter(|_| segments.iter().all(|segment| is_ident(segment))) {
        if let Some(ty) = head.last() {
            return if touched.contains(ty) { Form::Prose } else { Form::Type(format!("{ty}::{item}")) };
        }
    }
    let ident = name.strip_suffix("()").or_else(|| name.strip_suffix('('));
    match ident {
        Some(ident) if is_ident(ident) => Form::Fn(ident.to_owned()),
        _ => Form::Prose,
    }
}

/// 1 本の本文の backtick の中身（対になった backtick だけ・空は除く）。
fn backticked(text: &str) -> Vec<&str> {
    text.lines()
        .flat_map(|line| {
            let pieces: Vec<&str> = line.split('`').collect();
            let paired = if pieces.len().is_multiple_of(2) { pieces.len().saturating_sub(1) } else { pieces.len() };
            pieces.into_iter().take(paired).skip(1).step_by(2).filter(|piece| !piece.is_empty()).collect::<Vec<&str>>()
        })
        .collect()
}

/// path 形の名指しが tracked の path に解けるか（等しいか `/` 区切りの末尾一致）。
fn path_matches(path: &str, name: &str) -> bool {
    path == name || path.strip_suffix(name).is_some_and(|head| head.ends_with('/'))
}

/// 本文が `word`（`型::項目`）を語の境界で持つか（前が識別子の文字でなく・後ろも識別子の文字でない）。
fn holds_word(body: &str, word: &str) -> bool {
    heads(body, word)
        .into_iter()
        .any(|at| !body.get(at.saturating_add(word.len())..).unwrap_or_default().starts_with(is_ident_char))
}

/// 本文が `fn ident` の宣言を持つか。
fn declares_fn(body: &str, ident: &str) -> bool {
    holds_word(body, &format!("fn {ident}"))
}

/// `touches` の 1 項目を読んだもの。
struct Touched<'a> {
    /// 型の名（最後の segment）。
    name: &'a str,
    /// 型を置く module の名（`crate` 直下の型は `None`）。
    module: Option<&'a str>,
}

/// `touches` の型ごとに閉包を求め、和集合を返す（path の辞書順）。
///
/// **読めない file が 1 本でも在れば `Err`**（その file が型を持つかを測れない＝足りない file を見落とす側へ
/// 倒さない）。
pub fn closure(types: &[String], sources: &[Source]) -> Result<BTreeSet<String>, ClosureError> {
    let texts = texts_of(sources)?;
    let mut found = BTreeSet::new();
    for raw in types {
        let touched = touched(raw).ok_or_else(|| ClosureError::TypeForm { name: raw.clone() })?;
        found.extend(files_of(&touched, &texts));
    }
    Ok(found)
}

/// `crate::module::Type` を読む。形が違えば `None`。
fn touched(raw: &str) -> Option<Touched<'_>> {
    let segments: Vec<&str> = raw.split("::").collect();
    let (name, head) = segments.split_last()?;
    let formed = head.first() == Some(&"crate")
        && segments.iter().all(|segment| is_ident(segment))
        && name.starts_with(|found: char| found.is_ascii_uppercase());
    formed.then(|| Touched { name, module: head.last().copied().filter(|found| *found != "crate") })
}

/// 識別子の字面か（ASCII の英数字と `_`・先頭は数字でない）。
fn is_ident(text: &str) -> bool {
    text.starts_with(|found: char| found.is_ascii_alphabetic() || found == '_') && text.chars().all(is_ident_char)
}

/// 識別子を成す文字か。
fn is_ident_char(found: char) -> bool {
    found.is_ascii_alphanumeric() || found == '_'
}

/// 1 つの型の閉包（型が見えている file〔[`sees`]〕のうち 4 形のどれかを持つもの）。const slice の宣言 file も
/// 見えている file だけ＝別 module の同名 const slice（別の同名の型の slice）を拾わない。
fn files_of(touched: &Touched<'_>, texts: &[(&str, &str)]) -> BTreeSet<String> {
    let mut names: BTreeSet<&str> = BTreeSet::new();
    let mut declaring: BTreeSet<&str> = BTreeSet::new();
    let none = BTreeSet::new();
    for &(path, text) in texts {
        let found = slice_names(text, touched.name);
        if !found.is_empty() && sees(path, text, touched, &none) {
            declaring.insert(path);
            names.extend(found);
        }
    }
    texts
        .iter()
        .filter(|&&(path, text)| {
            sees(path, text, touched, &names)
                && (declaring.contains(path)
                    || constructs(text, touched.name)
                    || matches_arm(text, touched.name)
                    || pins(text, &scopes(path, touched.module), &names))
        })
        .map(|&(path, _)| path.to_owned())
        .collect()
}

/// この file から `touches` の型が見えているか（§3「閉包の同名衝突」・4 形は全部この 1 関数を通ってから数える・C2）:
/// (a) この file が型を宣言し（`enum` / `struct` の宣言行）path が `touches` の module に当たる（[`in_module`]）
/// (b) `use <module>::Name` で取り込む（[`imports`]）(c) 本文に `<module>::Name` の修飾が在る（[`qualifies`]）。
/// `<module>` の段は [`scopes`]（子 file からは `super` も同じ module）。`names` はその型の const slice の名で、(b)(c) は
/// 型名と同じに読む（`crate::paint::HUES.len()` の件数 pin は型を名指さずに型の構造を持つ＝第 3 形）。
fn sees(path: &str, text: &str, touched: &Touched<'_>, names: &BTreeSet<&str>) -> bool {
    let scopes = scopes(path, touched.module);
    (declares_type(text, touched.name) && in_module(path, touched.module))
        || std::iter::once(touched.name)
            .chain(names.iter().copied())
            .any(|name| scopes.iter().any(|scope| imports(text, scope, name) || qualifies(text, scope, name)))
}

/// 本文が `enum ty` / `struct ty` の宣言行を持つか。
fn declares_type(text: &str, ty: &str) -> bool {
    TYPE_DECLARING.iter().any(|word| holds_word(text, &format!("{word} {ty}")))
}

/// path の `src/` からの相対の (file の stem, 直上の dir の名)。`src/` の段が無ければ `None`。
fn placed(path: &str) -> Option<(&str, Option<&str>)> {
    let segments: Vec<&str> = path.split('/').collect();
    let at = segments.iter().position(|segment| *segment == SRC_DIR)?;
    let (file, dirs) = segments.get(at.saturating_add(1)..)?.split_last()?;
    Some((file.strip_suffix(RS).unwrap_or(file), dirs.last().copied()))
}

/// path が module の file か: `src/` からの相対で `<module>.rs` か `<module>/` の直下（多段 module は最後の段で弁別し
/// 親 dir は見ない＝`crate::seat::rebrief::Marker` は `src/seat/rebrief.rs`）・`crate` 直下の型は `lib.rs` / `main.rs`。
fn in_module(path: &str, module: Option<&str>) -> bool {
    match (placed(path), module) {
        (Some((stem, dir)), Some(module)) => stem == module || dir == Some(module),
        (Some((stem, _)), None) => CRATE_ROOT_STEMS.contains(&stem),
        (None, _) => false,
    }
}

/// この file から `touches` の module を指す修飾の段: `<module>`（`crate` 直下の型は `crate`）と、この file が
/// `<module>/` の直下の子 file（`mod.rs` と `lib.rs` / `main.rs` は module 自身）なら `super` も。
fn scopes<'m>(path: &str, module: Option<&'m str>) -> Vec<&'m str> {
    let mut found = vec![module.unwrap_or(CRATE_ROOT)];
    let child = placed(path).is_some_and(|(stem, dir)| dir == module && stem != MOD_STEM && !CRATE_ROOT_STEMS.contains(&stem));
    if child {
        found.push(SUPER);
    }
    found
}

/// 本文が `<scope>::name` の修飾を語の境界で持つか（[`qualified`] と同じ照合）。
fn qualifies(text: &str, scope: &str, name: &str) -> bool {
    heads(text, name).into_iter().any(|at| {
        let (before, rest) = text.split_at(at);
        let after = rest.get(name.len()..).unwrap_or_default();
        !after.starts_with(is_ident_char) && before.strip_suffix("::").is_some_and(|path| qualified(path, scope))
    })
}

/// `::` の前の字面の末尾の段が `scope` か。
fn qualified(path: &str, scope: &str) -> bool {
    path.rsplit(|found: char| !is_ident_char(found)).next() == Some(scope)
}

/// 本文が宣言する `ty` の const slice の名（`const NAME: &[Type]` / `&'static [Type]`）。
fn slice_names<'t>(text: &'t str, ty: &str) -> Vec<&'t str> {
    text.lines()
        .filter_map(|line| {
            let (_, rest) = line.split_once("const ")?;
            let (name, kind) = rest.split_once(':')?;
            let kind = kind.trim_start().strip_prefix('&')?.trim_start();
            let kind = kind.strip_prefix("'static").map_or(kind, str::trim_start);
            let inner = kind.strip_prefix('[')?.split_once(']')?.0;
            let name = name.trim();
            (is_ident(name) && inner.trim() == ty).then_some(name)
        })
        .collect()
}

/// `needle` の出現位置のうち、直前が識別子の文字でないもの（語の頭から始まる出現）。
fn heads(text: &str, needle: &str) -> Vec<usize> {
    text.match_indices(needle)
        .map(|(at, _)| at)
        .filter(|at| !text.get(..*at).unwrap_or_default().ends_with(is_ident_char))
        .collect()
}

/// `ty {` の literal 構築（または分解の pattern）を持つか。宣言・実装・戻り型の行は数えない。
fn constructs(text: &str, ty: &str) -> bool {
    let needle = format!("{ty} {{");
    heads(text, &needle).into_iter().any(|at| !declares(text.get(..at).unwrap_or_default()))
}

/// `Type {` の直前が宣言・実装の語か戻り型の `->` か。
fn declares(before: &str) -> bool {
    let head = before.trim_end();
    head.ends_with("->")
        || DECLARING
            .iter()
            .any(|word| head.strip_suffix(word).is_some_and(|rest| !rest.ends_with(is_ident_char)))
}

/// `ty::` を `=>` の左に持つ行（match の arm）が在るか。
fn matches_arm(text: &str, ty: &str) -> bool {
    let needle = format!("{ty}::");
    text.lines()
        .filter_map(|line| line.split_once("=>"))
        .any(|(left, _)| !heads(left, &needle).is_empty())
}

/// const slice の件数 pin（`NAME.len()` を整数 literal と突き合わせる箇所）を持つか（`scopes` は [`scopes`]）。
fn pins(text: &str, scopes: &[&str], names: &BTreeSet<&str>) -> bool {
    names.iter().any(|name| {
        let needle = format!("{name}.len()");
        heads(text, &needle).into_iter().any(|at| {
            let (before, rest) = text.split_at(at);
            let after = rest.get(needle.len()..).unwrap_or_default();
            resolves(before, text, scopes, name) && compared(before, after)
        })
    })
}

/// `NAME` の 1 出現が目的の module の const を指すか＝[`sees`] の (b)(c) を出現に当てる（`<module>::NAME` の修飾は
/// [`qualified`]・無修飾なら [`imports`] で取り込み済みか）。
fn resolves(before: &str, text: &str, scopes: &[&str], name: &str) -> bool {
    match before.strip_suffix("::") {
        Some(path) => scopes.iter().any(|scope| qualified(path, scope)),
        None => scopes.iter().any(|scope| imports(text, scope, name)),
    }
}

/// `NAME.len()` が整数 literal と突き合わされているか（`len(), 3` / `len() == 3` / `3, NAME.len()`）。
fn compared(before: &str, after: &str) -> bool {
    let lead = before.trim_end_matches(|found: char| is_ident_char(found) || found == ':');
    literal_after(after) || literal_before(lead)
}

/// `.len()` の後ろが「突き合わせの口 + 整数 literal」か（複数行の assert も空白を跨いで読む）。
fn literal_after(after: &str) -> bool {
    let rest = after.trim_start();
    COMPARES
        .iter()
        .filter_map(|op| rest.strip_prefix(op))
        .any(|tail| tail.trim_start().starts_with(|found: char| found.is_ascii_digit()))
}

/// `NAME` の前が「整数 literal + 突き合わせの口」か（`assert_eq!(3, NAME.len())`）。
fn literal_before(lead: &str) -> bool {
    COMPARES.iter().filter_map(|op| lead.trim_end().strip_suffix(op)).any(|head| {
        // 末尾の語を逆順に取る＝逆順の末尾が語の先頭。数字で始まる語だけが literal（`x_2` は変数）。
        let reversed: String = head.trim_end().chars().rev().take_while(|found| is_ident_char(*found)).collect();
        reversed.ends_with(|found: char| found.is_ascii_digit())
    })
}

/// 本文が `<scope>::` から `name` を同名で取り込む `use` 文を持つか（`as` の別名は下界の外）。
fn imports(text: &str, scope: &str, name: &str) -> bool {
    let path = format!("{scope}::");
    use_statements(text)
        .iter()
        .any(|statement| !heads(statement, &path).is_empty() && names_word(statement, name))
}

/// `use` 文が `name` を別名なしの語として持つか。
fn names_word(statement: &str, name: &str) -> bool {
    heads(statement, name).into_iter().any(|at| {
        let after = statement.get(at.saturating_add(name.len())..).unwrap_or_default();
        !after.starts_with(is_ident_char) && !after.trim_start().starts_with("as ")
    })
}

/// `use` 文を 1 文ずつ（複数行の `use a::{…};` は `;` まで繋げる）。
fn use_statements(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut open: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        let current = match open.take() {
            Some(head) => format!("{head} {trimmed}"),
            None if USE_HEADS.iter().any(|head| trimmed.starts_with(head)) => trimmed.to_owned(),
            None => continue,
        };
        if current.contains(';') {
            found.push(current);
        } else {
            open = Some(current);
        }
    }
    found
}

/// 行の数え方（設計 rules-manifest.md §4・`R-C4.line-width`・上限の余地が base の行数を数える式）: 各行を
/// `max(1, ceil(文字数 ÷ width))` と数えた合計。
///
/// 1 行に詰め込んでも余地が増えない形。文字数は `chars` の数・末尾改行の有無で差を出さない・`width = 0` は 1 行 1 と
/// 数える（0 除算の縮退）。xtask の `workspace::weighted_lines` と同じ式（crate は互いに依存しない）で、同じ fixture の
/// 歯が一致を守る。
pub fn weighted_lines(text: &str, width: usize) -> usize {
    text.lines()
        .map(|line| match width {
            0 => 1,
            _ => line.chars().count().div_ceil(width).max(1),
        })
        .sum()
}

/// 行が宣言する導出の材料（契約表の行の欄の字面そのまま・§3「write-set の導出」）。
#[derive(Debug, Clone, Copy)]
pub struct Fields<'a> {
    /// 触る閉じた型（`crate::module::Type`）。
    pub touches: &'a [String],
    /// 触る外形の名。
    pub surfaces: &'a [String],
    /// 検証行（`cargo nextest run` の行から crate と filter 語を読む）。
    pub verify: &'a [String],
    /// 新設する file（`+` を付けずに書く）。
    pub creates: &'a [String],
    /// 歯の置き場（base の歯の file か `creates` の新規 file）。
    pub tests: &'a [String],
    /// Rust の外で触る file（base に実在する非 `.rs`）。
    pub also: &'a [String],
}

/// base の tree の事実（I/O は呼び手が済ませて渡す）。
#[derive(Debug, Clone, Copy)]
pub struct Base<'a> {
    /// 読み込んだ `.rs` の全部。
    pub sources: &'a [Source],
    /// 読み込んだ外形 snapshot（`.snap`）の全部。
    pub snapshots: &'a [Source],
    /// tracked file の repo 相対 path。
    pub tracked: &'a [String],
    /// `-p` の無い nextest 行が指す crate（core の crate の名）。
    pub core_crate: &'a str,
}

/// write-set の導出値（§3・pure）: (i) `touches` の閉包 ∪ (ii) 歯の置き場 ∪ (iii) `surfaces` の外形 pin ∪ (iv) `creates`
/// （`+` を付けた新規 file）∪ (v) `also`。各項は宣言順に 1 関数で、解けない項目は typed に `Err`（最初の 1 件・
/// fail-closed）。読めない file は従来どおり [`ClosureError::Unreadable`]。
pub fn derive_write_set(fields: &Fields<'_>, base: &Base<'_>) -> Result<BTreeSet<String>, ClosureError> {
    let texts = texts_of(base.sources)?;
    let mut found = closure(fields.touches, base.sources)?;
    found.extend(teeth_places(fields, base, &texts)?);
    found.extend(surface_closure(fields.surfaces, base.sources, base.snapshots)?);
    found.extend(created(fields.creates, base.tracked)?);
    found.extend(also_files(fields.also, base.tracked)?);
    Ok(found)
}

/// 手書きの write-set が導出値と**集合として一致**するか（§3「手書きの write-set の扱い」）。照合は正規化した形
/// （`+x` と `x` は同じ項目・`./` と `//` は畳む）で、不足と余分を**全部**名指す（辞書順）。
pub fn check_drift(written: &[String], derived: &BTreeSet<String>) -> Result<(), ClosureError> {
    let wanted: BTreeSet<String> = derived.iter().map(|item| normalize(item)).collect();
    let given: BTreeSet<String> = written.iter().map(|item| normalize(item)).collect();
    let missing: Vec<String> = wanted.difference(&given).cloned().collect();
    let extra: Vec<String> = given.difference(&wanted).cloned().collect();
    if missing.is_empty() && extra.is_empty() {
        Ok(())
    } else {
        Err(ClosureError::WriteSetDrift { missing, extra })
    }
}

/// (ii) 歯の置き場: `verify` の nextest 行ごとに、その crate の歯の区間で `#[test]` の直下の `fn` の名が filter 語を
/// 含む file の全部（nextest の positional filter と同じ「含む」・helper の fn は数えない）。base で 0 本の filter 語
/// （新しい接頭辞）は `tests` 欄が置き場で、`tests` も無ければ [`ClosureError::TeethPlaceUnresolved`]。`tests` の
/// 項目は歯の file だけ（`creates` に在る新規 file は creates の側が write-set に載る）。
fn teeth_places(fields: &Fields<'_>, base: &Base<'_>, texts: &[(&str, &str)]) -> Result<BTreeSet<String>, ClosureError> {
    let mut found = BTreeSet::new();
    for line in fields.verify {
        let Some((krate, filter)) = nextest_filter(line, base.core_crate) else {
            continue;
        };
        let places: Vec<&str> = texts
            .iter()
            .filter(|(path, text)| in_crate(path, krate) && test_fns(test_region(path, text)).iter().any(|name| name.contains(filter)))
            .map(|(path, _)| *path)
            .collect();
        if places.is_empty() && fields.tests.is_empty() {
            return Err(ClosureError::TeethPlaceUnresolved { filter: filter.to_owned() });
        }
        found.extend(places.iter().map(|path| (*path).to_owned()));
    }
    for item in fields.tests {
        found.extend(teeth_file(item, fields.creates, base.tracked, texts)?);
    }
    Ok(found)
}

/// nextest の行から (crate, filter 語) を読む。書き出しが `cargo nextest run` でない行・filter 語（`-` で始まらない
/// 末尾の語）の無い行は `None`。crate は `-p` / `--package` の次の語・無ければ core の crate。
fn nextest_filter<'l>(line: &'l str, core_crate: &'l str) -> Option<(&'l str, &'l str)> {
    let mut words = line.split_whitespace();
    for head in NEXTEST_HEAD {
        if words.next() != Some(*head) {
            return None;
        }
    }
    let (mut krate, mut filter) = (core_crate, None);
    while let Some(word) = words.next() {
        if PACKAGE_FLAGS.contains(&word) {
            krate = words.next()?;
        } else if !word.starts_with('-') {
            filter = Some(word);
        }
    }
    Some((krate, filter?))
}

/// `path` が crate `name` の file か（`crates/<name>/` 配下）。
fn in_crate(path: &str, name: &str) -> bool {
    path.strip_prefix(CRATES_DIR).and_then(|rest| rest.strip_prefix(name)).is_some_and(|rest| rest.starts_with('/'))
}

/// 歯の区間の `#[test]` の直下の `fn` の名（属性行・doc・空行は跨ぐ・他の行が先に来れば歯ではない）。
fn test_fns(region: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut pending = false;
    for line in region.lines() {
        let trimmed = line.trim();
        if trimmed == TEST_ATTR {
            pending = true;
        } else if pending && !(trimmed.is_empty() || trimmed.starts_with("#[") || trimmed.starts_with("//")) {
            pending = false;
            found.extend(fn_name(trimmed));
        }
    }
    found
}

/// `fn ident` の行の ident（`pub fn` / `async fn` も同じ・無ければ `None`）。
fn fn_name(line: &str) -> Option<&str> {
    let (_, rest) = line.split_once("fn ")?;
    let name = rest.split(|found: char| !is_ident_char(found)).next()?;
    is_ident(name).then_some(name)
}

/// `tests` 欄の 1 項目: base に在る歯の file（`tests/` 配下か test 区間を持つ `.rs`）ならその path・`creates` の新規
/// file なら `None`（creates の側が載る）。base に在って歯でない file は [`ClosureError::TestsNotATeethFile`]・base
/// にも `creates` にも無ければ [`ClosureError::ItemUnresolved`]。
fn teeth_file(
    item: &str,
    creates: &[String],
    tracked: &[String],
    texts: &[(&str, &str)],
) -> Result<Option<String>, ClosureError> {
    if creates.iter().any(|found| found == item) {
        return Ok(None);
    }
    let Some((path, text)) = texts.iter().find(|(path, _)| *path == item) else {
        return Err(match tracked.iter().any(|found| found == item) {
            true => ClosureError::TestsNotATeethFile { item: item.to_owned() },
            false => ClosureError::ItemUnresolved { item: item.to_owned() },
        });
    };
    match test_region(path, text).is_empty() {
        true => Err(ClosureError::TestsNotATeethFile { item: item.to_owned() }),
        false => Ok(Some(item.to_owned())),
    }
}

/// (iv) `creates`: `+` を付けずに書いた base に無い path の列を、write-set の形（`+` 付き）で返す。`+` 付き・空・base
/// に在る path は [`ClosureError::ItemUnresolved`]。
fn created(items: &[String], tracked: &[String]) -> Result<BTreeSet<String>, ClosureError> {
    items
        .iter()
        .map(|item| {
            let fresh = !item.is_empty() && !item.starts_with(NEW_FILE) && !tracked.iter().any(|found| found == item);
            fresh.then(|| format!("{NEW_FILE}{item}")).ok_or_else(|| ClosureError::ItemUnresolved { item: item.clone() })
        })
        .collect()
}

/// (v) `also`: base に実在する非 `.rs` の file。`.rs` は [`ClosureError::AlsoNamesRust`]・base に無ければ
/// [`ClosureError::ItemUnresolved`]。
fn also_files(items: &[String], tracked: &[String]) -> Result<BTreeSet<String>, ClosureError> {
    items
        .iter()
        .map(|item| {
            if item.ends_with(RS) {
                return Err(ClosureError::AlsoNamesRust { item: item.clone() });
            }
            match tracked.iter().any(|found| found == item) {
                true => Ok(item.clone()),
                false => Err(ClosureError::ItemUnresolved { item: item.clone() }),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        check_drift, closure, derive_write_set, sees, surface_closure, touched, unresolved_names, weighted_lines, Base,
        ClosureError, Fields, Source,
    };
    use proptest::prelude::*;
    use proptest::test_runner::Config;
    use std::collections::BTreeSet;

    /// 反例の永続化を切り、case 数を 256 に pin する（`pipe::refuse` の歯と同じ形）。
    fn config() -> Config {
        Config {
            cases: 256,
            failure_persistence: None,
            ..Config::default()
        }
    }

    /// 読める `.rs` 1 本。
    fn source(path: &str, body: &str) -> Source {
        Source { path: path.to_owned(), body: Ok(body.to_owned()) }
    }

    /// 型を置く file（`crate::paint`・struct `Swatch` と enum `Hue` と const slice `HUES`）。
    ///
    /// struct / enum / impl の宣言行と戻り型の `-> Swatch {` は literal 構築に数えない・`Self { … }` は見ない（下界）。
    const PAINT: &str = "pub struct Swatch {\n    pub hue: u8,\n}\n\nimpl Swatch {\n    pub fn new() -> Swatch {\n        Self { hue: 0 }\n    }\n}\n\npub enum Hue {\n    Red,\n    Blue,\n}\n\npub const HUES: &[Hue] = &[Hue::Red, Hue::Blue];\n";

    /// 4 形を 1 file ずつ持つ fixture と、どの形も持たない file・別名の file。
    fn fixture() -> Vec<Source> {
        vec![
            // (iv) const slice の宣言 file。
            source("src/paint.rs", PAINT),
            // (i) literal 構築。
            source("src/build.rs", "use crate::paint::Swatch;\n\npub fn make() -> u8 {\n    let made = Swatch { hue: 1 };\n    made.hue\n}\n"),
            // (ii) match の arm。
            source("src/show.rs", "use crate::paint::Hue;\n\npub fn name(hue: Hue) -> &'static str {\n    match hue {\n        Hue::Red => \"red\",\n        _ => \"other\",\n    }\n}\n"),
            // (iii) 件数 pin（複数行の assert の形・`use` で同名に取り込む）。
            source("tests/count.rs", "use crate::paint::{Hue, HUES};\n\n#[test]\nfn count() {\n    assert_eq!(\n        HUES.len(),\n        2,\n        \"母集団\"\n    );\n}\n"),
            // 言及だけ・戻り型・件数を文言へ写すだけの `.len()`・別 module の同名 const の pin。
            source("src/other.rs", "use crate::tone::HUES;\n\n/// Hue の説明。\npub fn swatch() -> Swatch {\n    make()\n}\n\npub fn shown() -> String {\n    format!(\"{}\", crate::paint::HUES.len())\n}\n\npub fn tone_pin() {\n    assert_eq!(HUES.len(), 4);\n}\n"),
            // 別名（`use … as`）で現れる arm と件数 pin。
            source("src/alias.rs", "use crate::paint::Hue as Tint;\nuse crate::paint::{HUES as ALL_HUES};\n\npub fn tint(hue: Tint) -> u8 {\n    match hue {\n        Tint::Red => 1,\n        _ => 0,\n    }\n}\n\npub fn pin() {\n    assert_eq!(ALL_HUES.len(), 2);\n}\n"),
        ]
    }

    /// path の集合。
    fn set(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|found| (*found).to_owned()).collect()
    }

    /// 型名の列の閉包（読める fixture は `Ok` を返す）。
    fn of(types: &[&str], sources: &[Source]) -> BTreeSet<String> {
        let owned: Vec<String> = types.iter().map(|found| (*found).to_owned()).collect();
        closure(&owned, sources).unwrap_or_else(|error| panic!("読める fixture は閉包を返す: {error:?}"))
    }

    /// 4 形（literal 構築 / match の arm / 件数 pin / const slice の宣言）を各 1 file で拾い、宣言・実装・戻り型の行・
    /// 言及だけの file・件数を文言へ写すだけの file・別 module の同名 const は拾わない。
    #[test]
    fn closure_picks_each_of_the_four_forms_from_its_own_file() {
        let sources = fixture();
        assert_eq!(
            of(&["crate::paint::Hue"], &sources),
            set(&["src/paint.rs", "src/show.rs", "tests/count.rs"]),
            "(iv) 宣言 file・(ii) arm・(iii) 件数 pin"
        );
        assert_eq!(
            of(&["crate::paint::Swatch"], &sources),
            set(&["src/build.rs"]),
            "(i) literal 構築だけ（struct / impl の宣言行と戻り型は数えない）"
        );
        assert_eq!(
            of(&["crate::paint::Hue", "crate::paint::Swatch"], &sources),
            set(&["src/build.rs", "src/paint.rs", "src/show.rs", "tests/count.rs"]),
            "型ごとの閉包の和"
        );
        assert!(of(&[], &sources).is_empty(), "型が無ければ空");
    }

    /// 別名で現れる形（`use … as`）は拾わない＝字面走査の**下界**（設計 §3「限界」を歯で残す）。同じ file を
    /// 別名なしに書けば拾う（拾わない理由が別名だけであることの弁別）。
    #[test]
    fn closure_does_not_see_aliases_by_design() {
        assert!(!of(&["crate::paint::Hue"], &fixture()).contains("src/alias.rs"), "別名の arm と件数 pin は拾わない");
        let plain = source(
            "src/alias.rs",
            "use crate::paint::{Hue, HUES};\n\npub fn tint(hue: Hue) -> u8 {\n    match hue {\n        Hue::Red => 1,\n        _ => 0,\n    }\n}\n",
        );
        let found = of(&["crate::paint::Hue"], &[source("src/paint.rs", PAINT), plain]);
        assert!(found.contains("src/alias.rs"), "別名なしなら拾う: {found:?}");
    }

    /// 件数 pin は**整数 literal との突き合わせ**だけで、修飾は型の module で解く（件数を文言へ写す `.len()`・
    /// 不等号・別 module の同名 const は拾わない）。
    #[test]
    fn closure_counts_len_only_when_compared_with_a_literal() {
        for (body, pinned) in [
            ("fn f() {\n    assert_eq!(crate::paint::HUES.len(), 2);\n}\n", true),
            ("fn f() {\n    assert_eq!(2, crate::paint::HUES.len());\n}\n", true),
            ("fn f() {\n    assert!(crate::paint::HUES.len() == 2);\n}\n", true),
            ("fn f() {\n    let n = format!(\"{}\", crate::paint::HUES.len());\n}\n", false),
            ("fn f() {\n    assert!(crate::paint::HUES.len() >= 2);\n}\n", false),
            ("fn f() {\n    assert_eq!(crate::tone::HUES.len(), 2);\n}\n", false),
            ("fn f() {\n    assert_eq!(HUES.len(), 2);\n}\n", false),
        ] {
            let found = of(&["crate::paint::Hue"], &[source("src/paint.rs", PAINT), source("src/pin.rs", body)]);
            assert_eq!(found.contains("src/pin.rs"), pinned, "{body}");
        }
    }

    /// 「その file から型が見えているか」の 1 関数（§3「閉包の同名衝突」）の 4 組: (a) 宣言（自 module の file・
    /// `<module>/` の直下・多段 module は最後の段で弁別・module の path に在るだけの file は宣言が無ければ見えない）/
    /// (b) import（`<module>/` の直下の子 file は `use super::Name` も・`mod.rs` と別 dir の `super` は違う module）/
    /// (c) 修飾のどれかで見え、どれも無い file（別 module の同名の宣言・別 module からの import・別 module の修飾・型名を
    /// 前置きに持つ別の名）は見えない。`crate` 直下の型は `lib.rs` / `main.rs` の宣言と `crate::` の修飾で見える。
    #[test]
    fn contract_closure_ext_same_name_sees_by_declaration_import_or_qualification_only() {
        let none = BTreeSet::new();
        for (ty, path, text, want) in [
            ("crate::paint::Hue", "src/paint.rs", "pub enum Hue {\n    Red,\n}\n", true),
            ("crate::paint::Hue", "crates/toy/src/paint/mod.rs", "pub struct Hue;\n", true),
            ("crate::paint::Hue", "src/paint.rs", "pub fn f() {}\n", false),
            ("crate::paint::Hue", "src/tone.rs", "pub enum Hue {\n    Red,\n}\n", false),
            ("crate::fleet::Stage", "crates/toy/src/fleet/cli.rs", "use super::{replay, Stage};\n", true),
            ("crate::fleet::Stage", "crates/toy/src/fleet/cli.rs", "fn f() -> u8 {\n    super::Stage::Spawned as u8\n}\n", true),
            ("crate::fleet::Stage", "crates/toy/src/fleet/mod.rs", "use super::Stage;\n", false),
            ("crate::fleet::Stage", "crates/toy/src/pipe/cli.rs", "use super::Stage;\n", false),
            ("crate::paint::Hue", "src/a.rs", "use crate::paint::Hue;\n", true),
            ("crate::paint::Hue", "src/a.rs", "use crate::paint::{Hue, HUES};\n", true),
            ("crate::paint::Hue", "src/a.rs", "use crate::tone::Hue;\n", false),
            ("crate::paint::Hue", "src/a.rs", "fn f() -> u8 {\n    crate::paint::Hue::Red as u8\n}\n", true),
            ("crate::paint::Hue", "src/a.rs", "fn f() -> u8 {\n    crate::tone::Hue::Red as u8\n}\n", false),
            ("crate::paint::Hue", "src/a.rs", "fn f() -> usize {\n    crate::paint::Hues::len()\n}\n", false),
            ("crate::paint::Hue", "src/a.rs", "fn f(hue: Hue) -> u8 {\n    match hue {\n        Hue::Red => 1,\n    }\n}\n", false),
            ("crate::seat::rebrief::Marker", "crates/toy/src/seat/rebrief.rs", "pub enum Marker {\n    Sid,\n}\n", true),
            ("crate::seat::rebrief::Marker", "crates/toy/src/hook/vessel.rs", "pub struct Marker {\n    pub n: u32,\n}\n", false),
            ("crate::seat::rebrief::Marker", "tests/e2e/tick.rs", "use vessel::seat::rebrief::Marker;\n", true),
            ("crate::seat::rebrief::Marker", "tests/e2e/hook.rs", "use vessel::hook::vessel::Marker;\n", false),
            ("crate::Mood", "src/lib.rs", "pub enum Mood {\n    Up,\n}\n", true),
            ("crate::Mood", "src/mood.rs", "pub enum Mood {\n    Up,\n}\n", false),
            ("crate::Mood", "src/a.rs", "use crate::Mood;\n", true),
            ("crate::Mood", "src/a.rs", "use super::Mood;\n", true),
            ("crate::Mood", "src/paint/a.rs", "use super::Mood;\n", false),
        ] {
            let target = touched(ty).unwrap_or_else(|| panic!("{ty} は crate::module::Type の形"));
            assert_eq!(sees(path, text, &target, &none), want, "{ty} を {path} から: {text}");
        }
        let hue = touched("crate::paint::Hue").unwrap_or_else(|| panic!("形は正しい"));
        let names: BTreeSet<&str> = ["HUES"].into_iter().collect();
        let pin = "fn f() {\n    assert_eq!(crate::paint::HUES.len(), 2);\n}\n";
        assert!(!sees("src/pin.rs", pin, &hue, &none), "型名だけでは const slice の修飾を見ない");
        assert!(sees("src/pin.rs", pin, &hue, &names), "const slice の名は型名と同じに読む（第 3 形）");
    }

    /// 別 module に同名の型を置いた toy: 閉包は `touches` の module 側（宣言 file・そこから取り込んで構築 / 分岐する
    /// file）だけを持ち、同名の型を宣言し同じ 3 形（`Hue {`・`Hue::` の arm・`const NAME: &[Hue]`）を持つ別 module の
    /// file と、そちらから取り込む file は持たない。const slice の宣言 file も見えている file だけ（別 module の同名
    /// slice `HUES` の宣言も、その `HUES.len()` の pin も拾わない）。
    #[test]
    fn contract_closure_ext_same_name_forms_and_const_slice_stay_on_the_module_side() {
        let tone = "pub enum Hue {\n    Soft,\n}\n\npub struct Swatch {\n    pub hue: u8,\n}\n\npub const HUES: &[Hue] = &[Hue::Soft];\n\npub fn name(hue: Hue) -> u8 {\n    match hue {\n        Hue::Soft => 1,\n    }\n}\n\npub fn make() -> Swatch {\n    Swatch { hue: 0 }\n}\n";
        let sources = vec![
            source("src/paint.rs", PAINT),
            source("src/tone.rs", tone),
            source("src/from_paint.rs", "use crate::paint::{Hue, Swatch};\n\npub fn f(hue: Hue) -> Swatch {\n    match hue {\n        Hue::Red => Swatch { hue: 1 },\n        _ => Swatch { hue: 0 },\n    }\n}\n"),
            source("src/from_tone.rs", "use crate::tone::{Hue, Swatch};\n\npub fn f(hue: Hue) -> Swatch {\n    match hue {\n        Hue::Soft => Swatch { hue: 1 },\n    }\n}\n"),
            source("tests/tone_count.rs", "use crate::tone::HUES;\n\n#[test]\nfn count() {\n    assert_eq!(HUES.len(), 1);\n}\n"),
            source("tests/paint_count.rs", "use crate::paint::HUES;\n\n#[test]\nfn count() {\n    assert_eq!(HUES.len(), 2);\n}\n"),
        ];
        assert_eq!(of(&["crate::paint::Hue"], &sources), set(&["src/from_paint.rs", "src/paint.rs", "tests/paint_count.rs"]));
        assert_eq!(of(&["crate::tone::Hue"], &sources), set(&["src/from_tone.rs", "src/tone.rs", "tests/tone_count.rs"]));
        assert_eq!(of(&["crate::paint::Swatch"], &sources), set(&["src/from_paint.rs"]), "構築点も module 側だけ");
        assert_eq!(of(&["crate::tone::Swatch"], &sources), set(&["src/from_tone.rs", "src/tone.rs"]));
    }

    /// 読めない file は `Err`（fail-closed・その file が型を持つかを測れない）・型名の形が違えば `Err`。
    #[test]
    fn closure_fails_closed_on_unreadable_files_and_malformed_types() {
        let mut sources = fixture();
        sources.push(Source { path: "src/broken.rs".to_owned(), body: Err("invalid utf-8".to_owned()) });
        assert_eq!(
            closure(&["crate::paint::Hue".to_owned()], &sources),
            Err(ClosureError::Unreadable { path: "src/broken.rs".to_owned(), reason: "invalid utf-8".to_owned() })
        );
        for name in ["Hue", "paint::Hue", "crate::paint::hue", "crate::pa-int::Hue", "crate::"] {
            assert_eq!(
                closure(&[name.to_owned()], &fixture()),
                Err(ClosureError::TypeForm { name: name.to_owned() }),
                "{name} は crate::module::Type の形でない"
            );
        }
        let reason = ClosureError::TypeForm { name: "Hue".to_owned() }.reason();
        assert!(reason.contains("Hue") && !reason.contains('\n'), "理由は型名を名乗る 1 行: {reason}");
    }

    /// 外形 pin の fixture: 外形 snapshot 2 枚（doctor / pipe）と usage を持つ subcommand `pipe`・歯の file・src の file。
    fn surface_fixture() -> (Vec<Source>, Vec<Source>) {
        let sources = vec![
            // usage 行を持つ subcommand（`{NAME}` の穴を飛ばした語が名）。
            source("src/pipe/cli.rs", "pub fn usage() -> String {\n    format!(\"usage: {NAME} pipe <intake|show> [--state-dir D]\")\n}\n"),
            // 歯の file（tests 配下は全体が区間）: snapshot 名と usage 文字列を持つ。
            source("tests/e2e/pipe.rs", "#[test]\nfn pipe_external_form() {\n    insta::assert_snapshot!(form);\n}\n"),
            source("tests/e2e/usage.rs", "#[test]\nfn shows_usage() {\n    assert!(err.contains(\"pipe <intake|show> [--state-dir D]\"));\n}\n"),
            // src の file: `#[cfg(test)]` の区間だけが歯。区間の外の snapshot 名は数えない。
            source("src/main.rs", "// doctor_external_form を描く\nfn main() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn doctor_external_form() {}\n}\n"),
            source("src/other.rs", "// doctor_external_form は言及だけ（区間の外）\nfn f() {}\n"),
        ];
        let snapshots = vec![
            source("src/snapshots/toy__tests__doctor_external_form.snap", "---\nsource: src/main.rs\n---\ndoctor: ok\n"),
            source("tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "---\n---\nusage: toy pipe <intake|show> [--state-dir D]\n"),
            // 置き場の外の `.snap` は外形 snapshot ではない。
            source("fixtures/doctor_external_form.snap", "x\n"),
        ];
        (sources, snapshots)
    }

    /// 外形 pin（第 5 形）: snapshot の名は snapshot の file とその名を歯の区間に持つ `.rs`・subcommand の名は usage
    /// 文字列を持つ snapshot と `.rs`。src の区間の外の言及と置き場の外の `.snap` は数えず、宣言なしは空、未知の名は
    /// `SurfaceUnknown`。
    #[test]
    fn closure_surfaces_pin_the_snapshot_and_the_teeth_that_hold_its_name_or_usage() {
        let (sources, snapshots) = surface_fixture();
        let of = |names: &[&str]| {
            let owned: Vec<String> = names.iter().map(|found| (*found).to_owned()).collect();
            surface_closure(&owned, &sources, &snapshots)
        };
        assert_eq!(
            of(&["doctor_external_form"]),
            Ok(set(&["src/main.rs", "src/snapshots/toy__tests__doctor_external_form.snap"])),
            "snapshot の file と、名を歯の区間に持つ src（区間の外の other.rs と置き場の外の .snap は数えない）"
        );
        assert_eq!(
            of(&["pipe"]),
            Ok(set(&["tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "tests/e2e/usage.rs"])),
            "usage 文字列を持つ snapshot と歯（usage() を呼ぶだけの pipe.rs は下界の外）"
        );
        assert_eq!(
            of(&["pipe_external_form"]),
            Ok(set(&["tests/e2e/pipe.rs", "tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"])),
            "snapshot 名で pin する歯"
        );
        assert_eq!(of(&[]), Ok(BTreeSet::new()), "宣言なしは (v) を持たない");
        assert_eq!(
            of(&["nope_external_form"]),
            Err(ClosureError::SurfaceUnknown { name: "nope_external_form".to_owned() }),
            "未知の名は違反"
        );
        let mut broken = snapshots.clone();
        broken.push(Source { path: "src/snapshots/x.snap".to_owned(), body: Err("bad".to_owned()) });
        assert!(matches!(surface_closure(&["pipe".to_owned()], &sources, &broken), Err(ClosureError::Unreadable { .. })));
    }

    /// 名指しの実在の fixture: base の tracked path と `.rs` の本文。
    fn name_fixture() -> (Vec<String>, Vec<Source>) {
        let tracked = ["crates/toy/src/pipe/closure.rs", "crates/toy/src/polarity.rs", "docs/a.md"]
            .iter()
            .map(|found| (*found).to_owned())
            .collect();
        let sources = vec![
            source("crates/toy/src/polarity.rs", "pub enum Guard {\n    Intake,\n}\n\nfn f() -> Guard {\n    Guard::Intake\n}\n"),
            source("crates/toy/src/pipe/closure.rs", "use crate::polarity::Guard;\n\npub fn overlaps(left: &str) -> bool {\n    left.is_empty()\n}\n"),
        ];
        (tracked, sources)
    }

    /// 名指しの 3 形（path / 型の path / fn）を解き、解けないものを在り処付きで全件返す。`+` 宣言の新規 file は解け
    /// （write-set に無い同名は解けない）、`touches` の型の variant と一致しない字面（struct literal・field 付き
    /// variant・glob・属性・散文・単独の語）は名指しと読まない。
    #[test]
    fn closure_names_resolve_the_three_forms_and_name_every_unresolved_one() {
        let (tracked, sources) = name_fixture();
        let texts = |lines: &[(&str, &str)]| -> Vec<(String, String)> {
            lines.iter().map(|(at, text)| ((*at).to_owned(), (*text).to_owned())).collect()
        };
        let resolved = texts(&[
            ("title", "`pipe/closure.rs` と `closure.rs` と `crate::polarity::Guard` の `Guard::Intake`"),
            ("done", "`overlaps(` と `overlaps()` が在る・`Refuse::Nope` は touches の型・`pipe/review.rs` は write-set の + 宣言"),
            ("section 3 line 9", "`Refuse::WriteSetIncomplete { run, missing }`・`tests/e2e/*.rs`・`#[cfg(test)]`・`Type {`・`Type::`・`touches`・`.rs`・`NAME.len()`・`use … as`"),
        ]);
        let write_set = ["crates/toy/src/pipe/closure.rs".to_owned(), "+crates/toy/src/pipe/review.rs".to_owned()];
        let touches = ["crate::pipe::refuse::Refuse".to_owned()];
        assert_eq!(unresolved_names(&resolved, &touches, &write_set, &tracked, &sources), Ok(Vec::new()), "全部解ける");
        let unresolved = texts(&[
            ("title", "`pipe/none.rs` と `Guard::Rules`"),
            ("done", "`nope(` と `pipe/review.rs` は write-set に無い・`Refuse::Nope` は touches に無い・`+x.rs` は字面"),
            ("section 3 line 9", "`crate::fleet::Stage`"),
        ]);
        let found = unresolved_names(&unresolved, &[], &["crates/toy/src/pipe/closure.rs".to_owned()], &tracked, &sources);
        let want: Vec<(String, String)> = [
            ("pipe/none.rs", "title"),
            ("Guard::Rules", "title"),
            ("nope(", "done"),
            ("pipe/review.rs", "done"),
            ("Refuse::Nope", "done"),
            ("crate::fleet::Stage", "section 3 line 9"),
        ]
        .iter()
        .map(|(name, at)| ((*name).to_owned(), (*at).to_owned()))
        .collect();
        assert_eq!(found, Ok(want), "解けないものを全件・在り処付き・書かれた順");
        let mut broken = sources.clone();
        broken.push(Source { path: "crates/toy/src/x.rs".to_owned(), body: Err("bad".to_owned()) });
        assert!(matches!(unresolved_names(&resolved, &touches, &write_set, &tracked, &broken), Err(ClosureError::Unreadable { .. })));
    }

    /// 文字列の列。
    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|found| (*found).to_owned()).collect()
    }

    /// 導出の fixture: crate `toy` の型（`crate::paint::Hue`）と歯（`derive_` の歯が tests と src の区間に 1 本ずつ・
    /// helper 1 本・別接頭辞 1 本）・別 crate の歯・非 `.rs` の面。
    fn derive_base() -> (Vec<Source>, Vec<String>) {
        let sources = vec![
            source("crates/toy/src/paint.rs", PAINT),
            source("crates/toy/src/show.rs", "use crate::paint::Hue;\n\npub fn name(hue: Hue) -> u8 {\n    match hue {\n        Hue::Red => 1,\n        _ => 0,\n    }\n}\n"),
            // src の test 区間の歯（区間の外の `derive_` の fn は数えない）。
            source("crates/toy/src/other.rs", "pub fn derive_outside() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn derive_in_src() {}\n}\n"),
            source("crates/toy/tests/e2e.rs", "#[test]\nfn derive_ok() {}\n"),
            // helper の fn は `#[test]` の直下でない＝数えない。属性行を跨いだ歯は数える。
            source("crates/toy/tests/helper.rs", "fn derive_helper() {}\n\n#[test]\n#[ignore]\nfn other_case() {\n    derive_helper();\n}\n"),
            source("crates/other/tests/e2e.rs", "#[test]\nfn derive_elsewhere() {}\n"),
        ];
        let mut tracked: Vec<String> = sources.iter().map(|found| found.path.clone()).collect();
        tracked.extend(strings(&["rules/manifest.toml", "docs/design/toy.md"]));
        (sources, tracked)
    }

    /// fixture に対して導出する（欄は空を既定に差し替える）。
    fn derive(over: &[(&str, &[&str])], sources: &[Source], tracked: &[String]) -> Result<BTreeSet<String>, ClosureError> {
        let of = |key: &str| strings(over.iter().find(|(name, _)| *name == key).map_or(&[][..], |(_, items)| *items));
        let (touches, surfaces, verify) = (of("touches"), of("surfaces"), of("verify"));
        let (creates, tests, also) = (of("creates"), of("tests"), of("also"));
        let fields = Fields {
            touches: &touches,
            surfaces: &surfaces,
            verify: &verify,
            creates: &creates,
            tests: &tests,
            also: &also,
        };
        derive_write_set(&fields, &Base { sources, snapshots: &[], tracked, core_crate: "toy" })
    }

    /// 導出値 = 閉包 ∪ 歯の置き場（`#[test]` 直下の fn 名が filter 語を含む file・helper と区間の外と別 crate は
    /// 数えない）∪ `creates`（`+` 付き）∪ `tests` ∪ `also`。`-p` の無い行は core の crate・nextest でない行と
    /// filter 語の無い行は置き場を持たず、欄が全部空なら空。
    #[test]
    fn contract_derive_unions_closure_teeth_creates_and_also_in_declaration_order() {
        let (sources, tracked) = derive_base();
        let full = derive(
            &[
                ("touches", &["crate::paint::Hue"]),
                ("verify", &["cargo nextest run -p toy --no-tests=fail derive_", "git status"]),
                ("creates", &["crates/toy/src/new.rs"]),
                ("tests", &["crates/toy/tests/helper.rs"]),
                ("also", &["rules/manifest.toml"]),
            ],
            &sources,
            &tracked,
        );
        let want = set(&[
            "+crates/toy/src/new.rs",
            "crates/toy/src/other.rs",
            "crates/toy/src/paint.rs",
            "crates/toy/src/show.rs",
            "crates/toy/tests/e2e.rs",
            "crates/toy/tests/helper.rs",
            "rules/manifest.toml",
        ]);
        assert_eq!(full, Ok(want), "5 項の和");
        let teeth = |line: &str| derive(&[("verify", &[line])], &sources, &tracked);
        assert_eq!(
            teeth("cargo nextest run --no-tests=fail derive_"),
            Ok(set(&["crates/toy/src/other.rs", "crates/toy/tests/e2e.rs"])),
            "-p の無い行は core の crate（helper と区間の外は数えない）"
        );
        assert_eq!(teeth("cargo nextest run -p other derive_"), Ok(set(&["crates/other/tests/e2e.rs"])), "-p が crate を選ぶ");
        assert_eq!(teeth("cargo nextest run -p toy other_"), Ok(set(&["crates/toy/tests/helper.rs"])), "属性行を跨いだ歯");
        assert_eq!(teeth("cargo nextest run -p toy"), Ok(BTreeSet::new()), "filter 語の無い行は置き場を持たない");
        assert_eq!(teeth("cargo test derive_"), Ok(BTreeSet::new()), "nextest でない行は読まない");
        assert_eq!(derive(&[], &sources, &tracked), Ok(BTreeSet::new()), "欄が全部空なら空");
    }

    /// 解けない欄は typed に断る: `also` の `.rs` / 無い file・`tests` の歯でない file / 無い file・`creates` の `+` 付き /
    /// base に在る file・base で 0 本の filter 語（`tests` が在れば置き場になる・`creates` の新規 file は creates の側
    /// だけに載る）・読めない file。
    #[test]
    fn contract_derive_refuses_each_unresolved_field_typed() {
        let (sources, tracked) = derive_base();
        let item = |value: &str| value.to_owned();
        for (over, want) in [
            (("also", "crates/toy/src/paint.rs"), ClosureError::AlsoNamesRust { item: item("crates/toy/src/paint.rs") }),
            (("also", "rules/none.toml"), ClosureError::ItemUnresolved { item: item("rules/none.toml") }),
            (("tests", "crates/toy/src/paint.rs"), ClosureError::TestsNotATeethFile { item: item("crates/toy/src/paint.rs") }),
            (("tests", "docs/design/toy.md"), ClosureError::TestsNotATeethFile { item: item("docs/design/toy.md") }),
            (("tests", "crates/toy/tests/none.rs"), ClosureError::ItemUnresolved { item: item("crates/toy/tests/none.rs") }),
            (("creates", "+crates/toy/src/new.rs"), ClosureError::ItemUnresolved { item: item("+crates/toy/src/new.rs") }),
            (("creates", "crates/toy/src/paint.rs"), ClosureError::ItemUnresolved { item: item("crates/toy/src/paint.rs") }),
            (("verify", "cargo nextest run -p toy fresh_"), ClosureError::TeethPlaceUnresolved { filter: item("fresh_") }),
        ] {
            let (key, value) = over;
            assert_eq!(derive(&[(key, &[value])], &sources, &tracked), Err(want), "{key} = {value}");
        }
        let placed = derive(
            &[("verify", &["cargo nextest run -p toy fresh_"]), ("creates", &["crates/toy/tests/fresh.rs"]), ("tests", &["crates/toy/tests/fresh.rs"])],
            &sources,
            &tracked,
        );
        assert_eq!(placed, Ok(set(&["+crates/toy/tests/fresh.rs"])), "新しい接頭辞は tests 欄が置き場（creates の側だけに載る）");
        let mut broken = sources.clone();
        broken.push(Source { path: "crates/toy/src/x.rs".to_owned(), body: Err("bad".to_owned()) });
        assert!(matches!(derive(&[], &broken, &tracked), Err(ClosureError::Unreadable { .. })), "読めない file は違反");
    }

    /// 手書きの write-set は導出値との集合一致だけを認める: `+x` と `x`・`./x` は同じ項目、順序は問わない、不足と余分を
    /// 全部（辞書順）名指す。
    #[test]
    fn contract_derive_drift_normalizes_prefixes_and_names_missing_and_extra() {
        let derived = set(&["+crates/toy/src/new.rs", "crates/toy/src/paint.rs", "rules/manifest.toml"]);
        let same = strings(&["rules/manifest.toml", "./crates/toy/src/paint.rs", "crates/toy/src/new.rs"]);
        assert_eq!(check_drift(&same, &derived), Ok(()), "正規化して集合として一致");
        let drifted = strings(&["+crates/toy/src/paint.rs", "src/lib.rs", "docs/a.md"]);
        assert_eq!(
            check_drift(&drifted, &derived),
            Err(ClosureError::WriteSetDrift {
                missing: strings(&["crates/toy/src/new.rs", "rules/manifest.toml"]),
                extra: strings(&["docs/a.md", "src/lib.rs"]),
            }),
            "不足と余分を全部名指す"
        );
        let reason = ClosureError::WriteSetDrift { missing: Vec::new(), extra: strings(&["docs/a.md"]) }.reason();
        assert!(reason.contains("missing: -") && reason.contains("extra: docs/a.md"), "空の側は - で名乗る: {reason}");
    }

    // flip-check: retroactive s2-07l.333

    /// 余分**だけ**（不足 0）でも断る（FR39・触らない file を挙げる偽の交差の根）。`.311` の検出線で生き残った
    /// `&&` → `||` の変異は、不足と余分が両方在る fixture では落ちないので、片側だけの象限を歯で閉じる。
    #[test]
    fn contract_derive_drift_refuses_extra_only() {
        let derived = set(&["+crates/toy/src/new.rs", "crates/toy/src/paint.rs", "rules/manifest.toml"]);
        let extra_only = strings(&["crates/toy/src/new.rs", "crates/toy/src/paint.rs", "rules/manifest.toml", "docs/a.md"]);
        let got = check_drift(&extra_only, &derived);
        assert_eq!(
            got,
            Err(ClosureError::WriteSetDrift { missing: Vec::new(), extra: strings(&["docs/a.md"]) }),
            "余分だけでも断る（不足は空）"
        );
        let reason = got.map_err(|error| error.reason()).err().unwrap_or_default();
        assert!(reason.contains("missing: -") && reason.contains("extra: docs/a.md"), "不足の側は - で名乗る: {reason}");
    }

    /// 不足**だけ**（余分 0）でも断る（FR48・閉包の見落とし）。対は [`contract_derive_drift_refuses_extra_only`]。
    #[test]
    fn contract_derive_drift_refuses_missing_only() {
        let derived = set(&["+crates/toy/src/new.rs", "crates/toy/src/paint.rs", "rules/manifest.toml"]);
        let missing_only = strings(&["crates/toy/src/new.rs", "crates/toy/src/paint.rs"]);
        let got = check_drift(&missing_only, &derived);
        assert_eq!(
            got,
            Err(ClosureError::WriteSetDrift { missing: strings(&["rules/manifest.toml"]), extra: Vec::new() }),
            "不足だけでも断る（余分は空）"
        );
        let reason = got.map_err(|error| error.reason()).err().unwrap_or_default();
        assert!(reason.contains("missing: rules/manifest.toml") && reason.contains("extra: -"), "余分の側は - で名乗る: {reason}");
    }

    /// 幅 10 の fixture と期待値。**xtask の `workspace` の歯と同じ字面・同じ値**（2 crate の式の一致を守る）。
    const WIDTH_FIXTURES: &[(&str, usize)] = &[
        ("ab\ncd\nef\n", 3),
        ("abcdefghijklmnopqrstuvwxy\n", 3),
        ("0123456789\n", 1),
        ("\n", 1),
        ("ab\ncd\nef", 3),
        ("abcdefghijklmnopqrstuvwxy", 3),
        ("あいうえおかきくけこさ\n", 2),
    ];

    /// 幅 10: 短い 3 行 = 3・25 字の 1 行 = 3・10 字ちょうど = 1・空行 = 1・末尾改行の有無で同値・文字数は byte でなく
    /// 文字で数える（11 字の和文 = 2）。
    #[test]
    fn contract_closure_width_weighs_each_line_by_ceil_of_chars_over_width() {
        for (text, want) in WIDTH_FIXTURES {
            assert_eq!(weighted_lines(text, 10), *want, "{text:?}");
        }
        assert_eq!(weighted_lines("ab\ncd\nef", 10), weighted_lines("ab\ncd\nef\n", 10), "末尾改行の有無で差を出さない");
    }

    /// 幅 0 は 1 行 1 と数える（0 除算の縮退）・幅が行より広ければ改行の数と同じ。
    #[test]
    fn contract_closure_width_zero_and_wide_width_count_newlines() {
        assert_eq!(weighted_lines("abcdefghijklmnopqrstuvwxy\nab\n", 0), 2, "幅 0");
        assert_eq!(weighted_lines("abcdefghijklmnopqrstuvwxy\nab\n", 120), 2, "幅 120");
    }

    /// 本文の断片（4 形・別名・無関係な行）。
    const FRAGMENTS: &[&str] = &[
        "pub enum Hue {\n    Red,\n}\n",
        "pub const HUES: &[Hue] = &[Hue::Red];\n",
        "    Hue::Red => 1,\n",
        "let made = Swatch { hue: 1 };\n",
        "use crate::paint::{HUES};\nassert_eq!(HUES.len(), 1);\n",
        "    Tone::Soft => 2,\n",
        "use crate::paint::Hue as Tint;\n",
        "// nothing\n",
    ];

    /// 型名の候補（module 付き・crate 直下）。
    const TYPES: &[&str] = &["crate::paint::Hue", "crate::paint::Swatch", "crate::tone::Tone", "crate::Mood"];

    /// 読める `.rs` の列（0〜5 本・本文は断片の連結）。
    fn sources() -> impl Strategy<Value = Vec<Source>> {
        prop::collection::vec(prop::collection::vec(prop::sample::select(FRAGMENTS), 0..5), 0..6).prop_map(|files| {
            files
                .into_iter()
                .enumerate()
                .map(|(index, parts)| Source { path: format!("src/f{index}.rs"), body: Ok(parts.concat()) })
                .collect()
        })
    }

    /// 型名の列（0〜3 本）。
    fn types() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec(prop::sample::select(TYPES).prop_map(str::to_owned), 0..4)
    }

    /// 導出値の候補（正規化済み・辞書順）。
    const DERIVED_POOL: &[&str] = &["crates/toy/src/a.rs", "crates/toy/src/b.rs", "docs/c.md", "rules/d.toml", "tests/e.rs"];

    /// 足す候補（導出値の候補と交わらない）。
    const EXTRA_POOL: &[&str] = &["docs/x.md", "src/y.rs", "z.toml"];

    /// path の集合（候補から 0〜n 本）。
    fn picked(pool: &'static [&'static str], low: usize) -> impl Strategy<Value = BTreeSet<String>> {
        prop::collection::btree_set(prop::sample::select(pool).prop_map(str::to_owned), low..=pool.len())
    }

    proptest! {
        #![proptest_config(config())]

        /// `check_drift` が `Ok` ⇔ 欠き 0 ∧ 足し 0（片側だけでも `Err`・`missing` は欠いた集合・`extra` は足した集合）。
        #[test]
        fn prop_contract_derive_drift_is_ok_iff_nothing_missing_and_nothing_extra(
            derived in picked(DERIVED_POOL, 1),
            dropped in picked(DERIVED_POOL, 0),
            added in picked(EXTRA_POOL, 0),
        ) {
            let missing: Vec<String> = derived.intersection(&dropped).cloned().collect();
            let extra: Vec<String> = added.iter().cloned().collect();
            let written: Vec<String> = derived.difference(&dropped).chain(added.iter()).cloned().collect();
            let want = if missing.is_empty() && extra.is_empty() {
                Ok(())
            } else {
                Err(ClosureError::WriteSetDrift { missing, extra })
            };
            prop_assert_eq!(check_drift(&written, &derived), want);
        }

        /// 出力の path 集合は入力の path 集合の部分集合である（閉包は入力に無い file を作らない）。
        #[test]
        fn prop_closure_output_is_a_subset_of_the_input_paths(files in sources(), names in types()) {
            let inputs: BTreeSet<String> = files.iter().map(|found| found.path.clone()).collect();
            let found = closure(&names, &files);
            prop_assert!(found.is_ok());
            prop_assert!(found.unwrap_or_default().is_subset(&inputs));
        }

        /// 型を 1 つ足しても既存の型の集合は縮まない（単調）。
        #[test]
        fn prop_closure_is_monotone_in_the_types(files in sources(), names in types(), extra in prop::sample::select(TYPES)) {
            let before = closure(&names, &files).unwrap_or_default();
            let mut more = names.clone();
            more.push(extra.to_owned());
            let after = closure(&more, &files).unwrap_or_default();
            prop_assert!(before.is_subset(&after));
        }
    }
}
