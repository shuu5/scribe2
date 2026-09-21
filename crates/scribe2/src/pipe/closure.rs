//! write-set の閉包（設計 docs/design/contract-source.md §3・ADR-0023 §2.3・ADR-0013 §2.2・SRS FR48）。
//!
//! 契約表の行が `touches` に宣言した閉じた型（`crate::module::Type`）について、その型を**構造として
//! 持つ file** を字面走査で集める。**pure**（I/O は呼び手）で、入力は型名の列と読み込んだ `.rs` の
//! (path, 本文) の列だけである。型の閉包が拾う形は 5 つ（番号は §3 / §16 / §19 の呼び名・第 5 形は外形 pin）:
//!
//! 1. **literal 構築** `Type {`（直前が `struct` / `enum` / `impl` / `for` 等の語か `->` の行は宣言・実装・
//!    戻り型なので除く）
//! 2. **match の arm**（`Type::` を `=>` の左に持つ行）
//! 3. **件数 pin**（その型の const slice `NAME` の `NAME.len()` が整数 literal と `,` / `==` / `!=` で突き合わ
//!    される箇所）。件数を文言へ写すだけの `.len()` は型を足しても壊れないので pin に数えない。`NAME` の出現は
//!    `<module>::NAME` の修飾か、`use` で `<module>::` から同名で取り込んだ file でだけ解く（別の module の同名
//!    const を拾わない）
//! 4. **const slice の宣言 file**（`const NAME: &[Type]`）
//! 6. **variant 構築**（§19・行 s）: 行を最初の `=>` で割った**右側**（`=>` の無い行は全部）に `Type::Variant {` か
//!    `Type::Variant(` を持つ行（`Variant` は大文字始まりの識別子・`{` / `(` の前の空白は任意）。`=>` の右辺で
//!    同名の variant を組み直す file と `Err(…)` の中で組む file が当たる。`=>` の左のパターン側は 2 の面で数えない。
//!    `Self::Variant {` は型名でないので当たらず、小文字始まりの項目（`Type::assoc_fn(`）は呼出しで構築でない。
//!
//! どの形も、先に**その file から `touches` の型が見えているか**（[`sees`]・§3「閉包の同名衝突」）を 1 関数で
//! 判定してから数える: (a) file が型を宣言し path が `touches` の module に当たる (b) `use <module>::Name` で取り込む
//! (c) 本文に `<module>::Name` の修飾が在る。裸の型名だけで照合すると、別 module の同名の型（`hook::vessel::Marker` と
//! `seat::rebrief::Marker`）の file へ閉包が広がり、導出値の偽の交差が並列度を下げる。
//!
//! **下界である**（§3「限界」）: 型の名が別名で現れる形（`use … as`・generic の中）と `Self { … }` の構築、glob
//! （`use m::*` / `use super::*`）越しの取り込みは見ない。上界は構文木が要り A3 の依存になる（却下・§11）。読めない
//! file と型名の形の違いは `Err`（fail-closed・NFR4）。第 6 形は文字列 literal の中や複数行に跨る pattern の 2 行目の
//! 字面にも当たる（上界側へ広がる雑音・§19「着地済み行への波及」）。
//!
//! 閉包の拡張（契約 (g)・§3）も同じ pure な字面走査で持つ: (v) **外形 pin** [`surface_closure`]（`surfaces` の名が
//! 指す外形 snapshot の file と、その名か subcommand の usage 文字列を歯の区間に literal で持つ `.rs`）と
//! **名指しの実在** [`unresolved_names`]（backtick の中身のうち path 形 / 型の path 形 / fn 形だけを名指しと読み、
//! base に解けないものを全件返す）。
//!
//! **write-set の導出**（契約 (h)・§3「write-set の導出」）[`derive_write_set`] は行の欄（`touches` / `verify` /
//! `surfaces` / `creates` / `tests` / `also`）から write-set を**導出値**として作る = 閉包 ∪ 歯の置き場（verify の nextest
//! 行の scope〔旗なし / `--lib` / `--test <name>`・§28〕の中で base の `#[test]` の fn 名が filter 語を含む file）∪ 外形 pin
//! ∪ 新規 file ∪ Rust の外の file。手書きの
//! write-set は [`check_drift`] で導出値との集合一致だけを認める（接頭辞 `+` は剥がして比べる）。
//!
//! **fn 形の touches**（§18・行 r）: `touches` の項目の末尾が小文字始まりの識別子（`crate::pipe::cli::resume`）なら
//! 型でなく fn の名指しで、閉包はその module の段（[`in_module`]）で `fn <識別子>` を宣言する file（[`declares_fn`]・
//! 下界・呼び手は数えない）。宣言する file が 0 の周は [`ClosureError::FnUndeclared`]（空集合に潰さない・C10）。型形の
//! 4 形の判定は不変。

use std::collections::BTreeSet;

mod derive;
mod names;

pub use derive::{check_drift, derive_write_set, weighted_lines, Base, Fields};
pub use names::unresolved_names;
pub(crate) use derive::{declared_teeth, teeth_places, teeth_words};
use names::{declares_fn, holds_word};

/// nextest の行の書き出し（この後ろの語から crate と filter 語を読む）。
const NEXTEST_HEAD: &[&str] = &["cargo", "nextest", "run"];

/// nextest の行で crate を選ぶ flag（次の語が crate の名）。
const PACKAGE_FLAGS: &[&str] = &["-p", "--package"];

/// nextest の行で lib target だけを撃つ scope の旗（§28・置き場は `crates/<crate>/src/` 配下）。
const LIB_FLAG: &str = "--lib";

/// nextest の行で統合 test の target を選ぶ scope の旗（§28・次の語が target の名・置き場は `tests/<name>.rs` とその配下）。
const TEST_FLAG: &str = "--test";

/// nextest の行で target を選ぶが scope に読まない旗（§28・在れば旗なしと同じ広い側＝crate 全体へ倒す・fail-closed）。
const UNREAD_TARGET_FLAGS: &[&str] = &["--bin", "--bins", "--bench", "--benches", "--example", "--examples", "--tests", "--all-targets", "-E"];

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

/// 実装の行の書き出し（`trim_start` 後にこの語で始まる行が impl 経路の照合の口・§26）。
const IMPL_HEAD: &str = "impl";

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

/// variant 構築の口（第 6 形・`Type::Variant {` の `{` と `Type::Variant(` の `(`）。
const VARIANT_OPENS: &[char] = &['{', '('];

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
    /// fn 形の `touches`（`crate::<module>::<snake_ident>`）の fn を宣言する file が base に無い（§18・空集合に潰さない）。
    FnUndeclared {
        /// module の段の字面（`crate::` を除いた `pipe::cli`・`crate` 直下は `crate`）。
        module: String,
        /// fn の名。
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
    /// Declared 行（§20・行 t）の verify の歯の file（base の `#[test]` の fn 名が filter 語を含む file）が行の
    /// write-set に無い。**足りない file を全部**持つ（辞書順）。
    TeethOutsideWriteSet {
        /// write-set に無い歯の file（repo 相対・辞書順）。
        files: Vec<String>,
    },
}

impl ClosureError {
    /// 断る理由の 1 行。
    pub fn reason(&self) -> String {
        match *self {
            Self::TypeForm { ref name } => format!("touches の {name} が crate::module::Type の形でない"),
            Self::FnUndeclared { ref module, ref name } => format!("touches の {module}::{name} を宣言する file が base に無い"),
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
            Self::TeethOutsideWriteSet { ref files } => {
                format!("verify の歯の file が write-set に無い（{}）", listed(files))
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
    test_mark_at(text).and_then(|at| text.get(at..)).unwrap_or_default()
}

/// 本体の区間（[`test_region`] の補・最初の行頭 `#[cfg(test)]` より前・無ければ全体）。path は見ない＝xtask の
/// core-lines（`split_test_src` の src 側・設計 core-boundary.md §2）と同じく印だけで切る。受付の core の余地
/// （[`crate::pipe::declaration::FileLines`]）が R-C4-1 の合計をこの区間で数える。
pub fn src_region(text: &str) -> &str {
    test_mark_at(text).and_then(|at| text.get(..at)).unwrap_or(text)
}

/// 最初の行頭 `#[cfg(test)]` の byte 位置（無ければ `None`）。
fn test_mark_at(text: &str) -> Option<usize> {
    if text.starts_with(TEST_MARK) {
        Some(0)
    } else {
        text.find(&format!("\n{TEST_MARK}")).map(|at| at.saturating_add(1))
    }
}

/// `touches` の 1 項目を読んだもの。
struct Touched<'a> {
    /// 型か fn の名（最後の segment）。
    name: &'a str,
    /// 型か fn を置く module の名（`crate` 直下は `None`）。
    module: Option<&'a str>,
    /// fn 形か（末尾が小文字始まりの識別子・§18）。型形は `false`。
    fn_form: bool,
}

/// `touches` の型 / fn ごとに閉包を求め、和集合を返す（path の辞書順）。
///
/// **読めない file が 1 本でも在れば `Err`**（その file が型を持つかを測れない＝足りない file を見落とす側へ
/// 倒さない）。fn 形で宣言する file が 0 の周も `Err`（[`ClosureError::FnUndeclared`]・空集合に潰さない）。
pub fn closure(types: &[String], sources: &[Source]) -> Result<BTreeSet<String>, ClosureError> {
    let texts = texts_of(sources)?;
    let mut found = BTreeSet::new();
    for raw in types {
        let touched = touched(raw).ok_or_else(|| ClosureError::TypeForm { name: raw.clone() })?;
        let files = files_of(&touched, &texts);
        if touched.fn_form && files.is_empty() {
            return Err(ClosureError::FnUndeclared { module: module_path(raw), name: touched.name.to_owned() });
        }
        found.extend(files);
    }
    Ok(found)
}

/// `crate::module::Type`（型形）か `crate::module::snake_ident`（fn 形）を読む。形が違えば `None`。
fn touched(raw: &str) -> Option<Touched<'_>> {
    let segments: Vec<&str> = raw.split("::").collect();
    let (name, head) = segments.split_last()?;
    let fn_form = name.starts_with(|found: char| found.is_ascii_lowercase());
    let formed = head.first() == Some(&"crate")
        && segments.iter().all(|segment| is_ident(segment))
        && (fn_form || name.starts_with(|found: char| found.is_ascii_uppercase()));
    formed.then(|| Touched { name, module: head.last().copied().filter(|found| *found != "crate"), fn_form })
}

/// `touches` の 1 項目の module の段の字面（`crate::pipe::cli::resume` → `pipe::cli`・`crate` 直下は `crate`）。
fn module_path(raw: &str) -> String {
    raw.strip_prefix(CRATE_ROOT)
        .and_then(|rest| rest.strip_prefix("::"))
        .and_then(|rest| rest.rsplit_once("::"))
        .map_or_else(|| CRATE_ROOT.to_owned(), |(head, _)| head.to_owned())
}

/// 識別子の字面か（ASCII の英数字と `_`・先頭は数字でない）。
fn is_ident(text: &str) -> bool {
    text.starts_with(|found: char| found.is_ascii_alphabetic() || found == '_') && text.chars().all(is_ident_char)
}

/// 識別子を成す文字か。
fn is_ident_char(found: char) -> bool {
    found.is_ascii_alphanumeric() || found == '_'
}

/// 1 つの型の閉包（型が見えている file〔[`sees`]〕のうち 4 形か第 6 形〔variant 構築・[`builds_variant`]〕のどれかを
/// 持つもの）。const slice の宣言 file も見えている file だけ＝別 module の同名 const slice（別の同名の型の slice）を
/// 拾わない。
///
/// fn 形（§18）は module の段（[`in_module`]）で `fn <名>` を宣言する file だけ（呼び手と別 module の同名の fn は
/// 数えない・下界）。
fn files_of(touched: &Touched<'_>, texts: &[(&str, &str)]) -> BTreeSet<String> {
    if touched.fn_form {
        return texts
            .iter()
            .filter(|&&(path, text)| in_module(path, touched.module) && declares_fn(text, touched.name))
            .map(|&(path, _)| path.to_owned())
            .collect();
    }
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
                    || builds_variant(text, touched.name)
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

/// enum の variant 構築（第 6 形・§19）: 行を最初の `=>` で割った右側（`=>` の無い行は行の全部）に `ty::Variant {` か
/// `ty::Variant(` を持つか。`=>` の左のパターン側は [`matches_arm`] の面で数えない。`Self::Variant` は型名でないので
/// 当たらず、小文字始まりの項目（関連 fn の呼出し）は構築でない。
fn builds_variant(text: &str, ty: &str) -> bool {
    let needle = format!("{ty}::");
    text.lines().map(|line| line.split_once("=>").map_or(line, |(_, right)| right)).any(|right| {
        heads(right, &needle)
            .into_iter()
            .any(|at| variant_built(right.get(at.saturating_add(needle.len())..).unwrap_or_default()))
    })
}

/// `ty::` の直後が「大文字始まりの識別子 + 任意の空白 + `{` / `(`」か。
fn variant_built(rest: &str) -> bool {
    rest.starts_with(|found: char| found.is_ascii_uppercase())
        && rest.trim_start_matches(is_ident_char).trim_start().starts_with(VARIANT_OPENS)
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

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.363
    // flip-check: moved s2-07l.458

    use super::{check_drift, closure, sees, surface_closure, touched, ClosureError, Source};
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
    pub(super) fn source(path: &str, body: &str) -> Source {
        Source { path: path.to_owned(), body: Ok(body.to_owned()) }
    }

    /// 型を置く file（`crate::paint`・struct `Swatch` と enum `Hue` と const slice `HUES`）。
    ///
    /// struct / enum / impl の宣言行と戻り型の `-> Swatch {` は literal 構築に数えない・`Self { … }` は見ない（下界）。
    pub(super) const PAINT: &str = "pub struct Swatch {\n    pub hue: u8,\n}\n\nimpl Swatch {\n    pub fn new() -> Swatch {\n        Self { hue: 0 }\n    }\n}\n\npub enum Hue {\n    Red,\n    Blue,\n}\n\npub const HUES: &[Hue] = &[Hue::Red, Hue::Blue];\n";

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
    pub(super) fn set(paths: &[&str]) -> BTreeSet<String> {
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

    /// 第 6 形の fixture（§19）: struct-like と tuple-like の variant を持つ enum `crate::shape::Shape`。宣言 file は
    /// `impl` の中で `Self::Dot { … }` だけを組む（4 形のどれも持たない）。
    const SHAPE: &str = "pub enum Shape {\n    Dot { x: u8 },\n    Line(u8),\n}\n\nimpl Shape {\n    pub fn dot() -> Shape {\n        Self::Dot { x: 0 }\n    }\n\n    pub fn none() -> Option<Shape> {\n        None\n    }\n}\n";

    /// variant 構築（第 6 形・§19 の約束 1 と約束 2 の (a) (b) (d)）: `=>` の右辺で `{` 形を組むだけの file と
    /// `Err(…)` の中で `(` 形を組むだけの file が導出値に入り、`Self::` の構築だけの宣言 file・小文字始まりの項目
    /// （関連 fn の呼出し）だけの file・型が見えていない同名の file（別 module から取り込む）は入らない。
    #[test]
    fn closure_variant_construction_picks_files_that_only_build_a_variant() {
        let sources = vec![
            source("src/shape.rs", SHAPE),
            // `=>` の右辺で struct-like の variant を組む（左のパターンは型を持たない）。
            source("src/right.rs", "use crate::shape::Shape;\n\npub fn of(n: u8) -> Shape {\n    match n {\n        0 => Shape::Dot { x: n },\n        _ => Shape::dot(),\n    }\n}\n"),
            // `Err(…)` の中で tuple-like の variant を組む。
            source("src/err.rs", "use crate::shape::Shape;\n\npub fn of(n: u8) -> Result<u8, Shape> {\n    if n == 0 {\n        return Err(Shape::Line(n));\n    }\n    Ok(n)\n}\n"),
            // 小文字始まりの項目（関連 fn の呼出し）だけ。
            source("src/call.rs", "use crate::shape::Shape;\n\npub fn made() -> bool {\n    let _shape = Shape::dot();\n    Shape::none().is_none()\n}\n"),
            // 別 module の同名の型を組む（`sees` が通さない）。
            source("src/other.rs", "use crate::tone::Shape;\n\npub fn of() -> Shape {\n    Shape::Dot { x: 1 }\n}\n"),
        ];
        assert_eq!(
            of(&["crate::shape::Shape"], &sources),
            set(&["src/err.rs", "src/right.rs"]),
            "variant 構築だけの 2 file が入り、Self:: だけの宣言 file・呼出しだけの file・見えていない同名は入らない"
        );
    }

    /// 第 6 形の述語（§19 の約束 2 の (c)）: `Type::Variant {` / `Type::Variant(` が `=>` の左のパターン側にしか無い本文は
    /// 偽で、同じ出現を `=>` の右側へ移すと真（導出値では `matches_arm` が同じ file を入れるので in-file でだけ測れる）。
    /// `Self::` と小文字始まりの項目・`{` / `(` の続かない出現（`use` 文・doc comment の名指し・unit variant）は偽。
    #[test]
    fn closure_variant_construction_predicate_reads_only_the_right_of_the_arrow() {
        for (body, want) in [
            ("    Shape::Dot { x } => 1,\n", false),
            ("    Shape::Line(n) => n,\n", false),
            ("    1 => Shape::Dot { x },\n", true),
            ("    n => Shape::Line(n),\n", true),
            ("    Shape::Dot { .. } => Shape::Dot { x: 0 },\n", true),
            ("    let made = Shape::Dot { x: 0 };\n", true),
            ("    return Err(Shape::Line (0));\n", true),
            ("    Self::Dot { x: 0 }\n", false),
            ("    let made = Shape::dot();\n", false),
            ("use crate::shape::Shape::Dot;\n", false),
            ("/// [`Shape::Dot`] を組む。\n", false),
            ("    let unit = Shape::Solo;\n", false),
        ] {
            assert_eq!(super::builds_variant(body, "Shape"), want, "{body}");
        }
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
        for name in ["Hue", "paint::Hue", "crate::paint::_hue", "crate::pa-int::Hue", "crate::"] {
            assert_eq!(
                closure(&[name.to_owned()], &fixture()),
                Err(ClosureError::TypeForm { name: name.to_owned() }),
                "{name} は crate::module::Type の形でない"
            );
        }
        let reason = ClosureError::TypeForm { name: "Hue".to_owned() }.reason();
        assert!(reason.contains("Hue") && !reason.contains('\n'), "理由は型名を名乗る 1 行: {reason}");
    }

    /// fn 形の touches（§18）: `crate::<module>::<snake_ident>` は module の段で `fn <名>` を宣言する file だけを持ち
    /// （呼び手・別 module の同名の fn・同 module の別の fn は持たない）、宣言する file が 0 の周は `FnUndeclared`
    /// （module の段と名を名乗る 1 行・`TypeForm` とは別の字面）。`crate` 直下の fn は `lib.rs` / `main.rs` の宣言。
    #[test]
    fn contract_derive_fn_form_collects_declaring_files_in_the_module_only() {
        let sources = vec![
            source("crates/toy/src/pipe/cli.rs", "pub fn resume(state: &str) -> usize {\n    state.len()\n}\n\npub fn stop() {}\n"),
            source("crates/toy/src/pipe/cli/run.rs", "pub(super) fn resume(at: usize) -> usize {\n    at\n}\n"),
            source("crates/toy/src/main.rs", "fn main() {\n    let _ = crate::pipe::cli::resume(\"x\");\n}\n\npub fn boot() {}\n"),
            source("crates/toy/src/tone.rs", "pub fn resume() -> usize {\n    0\n}\n"),
        ];
        assert_eq!(
            of(&["crate::pipe::cli::resume"], &sources),
            set(&["crates/toy/src/pipe/cli.rs", "crates/toy/src/pipe/cli/run.rs"]),
            "module の段で宣言する file だけ（呼び手の main.rs と別 module の tone.rs は入らない）"
        );
        assert_eq!(of(&["crate::boot"], &sources), set(&["crates/toy/src/main.rs"]), "crate 直下の fn は main.rs の宣言");
        let missing = closure(&["crate::pipe::cli::missing".to_owned()], &sources);
        assert_eq!(
            missing,
            Err(ClosureError::FnUndeclared { module: "pipe::cli".to_owned(), name: "missing".to_owned() }),
            "宣言する file が 0 なら typed に断る（空集合に潰さない）"
        );
        assert_eq!(
            closure(&["crate::missing".to_owned()], &sources),
            Err(ClosureError::FnUndeclared { module: "crate".to_owned(), name: "missing".to_owned() }),
            "crate 直下の module の段は crate"
        );
        let reason = missing.map_err(|error| error.reason()).err().unwrap_or_default();
        assert_eq!(reason, "touches の pipe::cli::missing を宣言する file が base に無い");
        assert!(!reason.contains("crate::module::Type の形でない"), "TypeForm とは別の字面: {reason}");
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
