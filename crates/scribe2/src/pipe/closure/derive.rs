//! write-set の導出（設計 docs/design/contract-source.md §3「write-set の導出」・契約 (h)・SRS FR48 / FR39）。
//!
//! 型の閉包（[`super::closure`]）・外形 pin（[`super::surface_closure`]）と同じ **pure** な字面走査で、契約表の行の
//! 欄（`touches` / `verify` / `surfaces` / `creates` / `tests` / `also`）から write-set を**導出値**として作る
//! [`derive_write_set`] = 閉包 ∪ 歯の置き場（base の `#[test]` の fn 名が verify の filter 語を含む file）∪ 外形
//! pin ∪ 新規 file ∪ Rust の外の file。手書きの write-set は [`check_drift`] で導出値との集合一致だけを認める
//! （接頭辞 `+` は剥がして比べる）。行の数え方 [`weighted_lines`] も上限の余地の式としてここに置く。
//!
//! 型の閉包の字面走査（4 形と [`super::sees`]）は親 module `closure.rs` に置いたまま（1 関数の判定で結ばれる）。

use super::super::refuse::{normalize, NEW_FILE};
use super::{closure, is_ident, is_ident_char, surface_closure, test_region, texts_of, ClosureError, Source};
use super::{CRATES_DIR, NEXTEST_HEAD, PACKAGE_FLAGS, RS, TEST_ATTR};
use std::collections::BTreeSet;

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
    // flip-check: moved s2-07l.363

    use super::super::tests::{set, source, PAINT};
    use super::{check_drift, derive_write_set, weighted_lines, Base, ClosureError, Fields, Source};
    use std::collections::BTreeSet;

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
}
