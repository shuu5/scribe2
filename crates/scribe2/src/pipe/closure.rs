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
//!    される箇所）。件数を文言へ写すだけの `.len()` は型を足しても壊れないので pin に数えない。`NAME` は
//!    `<module>::NAME` の修飾か、宣言 file か、`use` で `<module>::` から同名で取り込んだ file でだけ解く
//!    （別の module の同名 const を拾わない）
//! 4. **const slice の宣言 file**（`const NAME: &[Type]`）
//!
//! **下界である**（§3「限界」）: 型の名が別名で現れる形（`use … as`・generic の中）と `Self { … }` の構築は
//! 見ない。上界は構文木が要り A3 の依存になる（却下・§11）。読めない file と型名の形の違いは `Err`
//! （fail-closed・NFR4）。

use std::collections::BTreeSet;

/// `use` 文の書き出し（複数行に跨ぐ `use a::{…};` は `;` まで繋げて 1 文として読む）。
const USE_HEADS: &[&str] = &["use ", "pub use ", "pub(crate) use "];

/// `Type {` の直前にこの語が在る行は宣言・実装の行である（literal 構築ではない）。
const DECLARING: &[&str] = &["struct", "enum", "union", "trait", "impl", "for"];

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
}

impl ClosureError {
    /// 断る理由の 1 行。
    pub fn reason(&self) -> String {
        match *self {
            Self::TypeForm { ref name } => format!("touches の {name} が crate::module::Type の形でない"),
            Self::Unreadable { ref path, ref reason } => format!("閉包を測る {path} を読めない: {reason}"),
        }
    }
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
    let mut texts: Vec<(&str, &str)> = Vec::new();
    for source in sources {
        match source.body {
            Ok(ref text) => texts.push((source.path.as_str(), text.as_str())),
            Err(ref reason) => {
                return Err(ClosureError::Unreadable { path: source.path.clone(), reason: reason.clone() })
            }
        }
    }
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

/// 1 つの型の閉包（4 形のどれかを持つ file）。
fn files_of(touched: &Touched<'_>, texts: &[(&str, &str)]) -> BTreeSet<String> {
    let mut names: BTreeSet<&str> = BTreeSet::new();
    let mut declaring: BTreeSet<&str> = BTreeSet::new();
    for &(path, text) in texts {
        let found = slice_names(text, touched.name);
        if !found.is_empty() {
            declaring.insert(path);
            names.extend(found);
        }
    }
    texts
        .iter()
        .filter(|&&(path, text)| {
            declaring.contains(path)
                || constructs(text, touched.name)
                || matches_arm(text, touched.name)
                || pins(text, touched, &names, declaring.contains(path))
        })
        .map(|&(path, _)| path.to_owned())
        .collect()
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

/// const slice の件数 pin（`NAME.len()` を整数 literal と突き合わせる箇所）を持つか。
fn pins(text: &str, touched: &Touched<'_>, names: &BTreeSet<&str>, declaring: bool) -> bool {
    names.iter().any(|name| {
        let imported = declaring || imports(text, touched.module, name);
        let needle = format!("{name}.len()");
        heads(text, &needle).into_iter().any(|at| {
            let (before, rest) = text.split_at(at);
            let after = rest.get(needle.len()..).unwrap_or_default();
            resolves(before, touched.module, imported) && compared(before, after)
        })
    })
}

/// `NAME` の出現が目的の module の const を指すか（`<module>::NAME` の修飾・無修飾なら取り込み済みか）。
fn resolves(before: &str, module: Option<&str>, imported: bool) -> bool {
    match before.strip_suffix("::") {
        Some(path) => module.is_some_and(|found| path.rsplit(|c: char| !is_ident_char(c)).next() == Some(found)),
        None => imported,
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

/// 本文が `<module>::` から `name` を同名で取り込む `use` 文を持つか（`as` の別名は下界の外）。
fn imports(text: &str, module: Option<&str>, name: &str) -> bool {
    let Some(module) = module else {
        return false;
    };
    let path = format!("{module}::");
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
    use super::{closure, ClosureError, Source};
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

    proptest! {
        #![proptest_config(config())]

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
