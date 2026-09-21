//! write-set の導出（設計 docs/design/contract-source.md §3「write-set の導出」・契約 (h)・SRS FR48 / FR39）。
//!
//! 型の閉包（[`super::closure`]）・外形 pin（[`super::surface_closure`]）と同じ **pure** な字面走査で、契約表の行の
//! 欄（`touches` / `verify` / `surfaces` / `creates` / `tests` / `also`）から write-set を**導出値**として作る
//! [`derive_write_set`] = 閉包 ∪ 歯の置き場（verify の nextest 行の scope〔[`Scope`]・§28〕の中で base の `#[test]` の
//! fn 名が filter 語を含む file）∪ 外形 pin ∪ 新規 file とその親の宣言 file（§17 の形 (vi)・[`parents`]）∪ Rust の外の file。手書きの write-set は [`check_drift`] で
//! 導出値との集合一致だけを認める（接頭辞 `+` は剥がして比べる）。行の数え方 [`weighted_lines`] も上限の余地の式として
//! ここに置く。
//!
//! **Declared 行の歯の置き場の門**（§20・行 t）[`declared_teeth`]: 導出も drift も撃たない Declared 行でも、`verify` の
//! nextest 行の歯の file は同じ [`teeth_places`] で解き、行の write-set に無い file を全部名指して断る
//! （[`check_teeth_cover`]・照合は [`check_drift`] と同じ正規化・dir 項目は配下）。
//!
//! 型の閉包の字面走査（4 形と [`super::sees`]）は親 module `closure.rs` に置いたまま（1 関数の判定で結ばれる）。

use super::super::refuse::{covered, normalize, NEW_FILE};
use super::super::table::PromiseRow;
use super::names::closed_type;
use super::{closure, is_ident, is_ident_char, snapshot_name, surface_closure, test_region};
use super::{texts_of, ClosureError, Source};
use super::{CRATES_DIR, CRATE_ROOT_STEMS, LIB_FLAG, MOD_STEM, NEXTEST_HEAD, PACKAGE_FLAGS, RS, SRC_DIR, TESTS_DIR, TEST_ATTR};
use super::{TEST_FLAG, UNREAD_ARG_TARGET_FLAGS, UNREAD_BARE_TARGET_FLAGS};
use std::collections::BTreeSet;

/// 生成する nextest 行の旗（歯が 0 本の行を緑にしない＝契約表の既存の行と同じ形）。
const NO_TESTS_FLAG: &str = "--no-tests=fail";

/// 名付き snapshot の口（`assert_snapshot!(` の直後が文字列 literal と `,` の形＝第 1 引数が snapshot の名）。
const SNAPSHOT_CALL: &str = "snapshot!(";

/// 外形 snapshot を持つ歯の名の末尾（§16 の外形 pin と同じ読み＝snapshot の名は歯の fn 名）。
const EXTERNAL_FORM: &str = "_external_form";

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
    /// 約束の行の `files` の `+` 無しの `.rs`（base に実在・§34）。Declared / Derived の行は空。
    pub files: &'a [String],
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
/// （`+` を付けた新規 file）とその親の宣言 file（§17 の形 (vi)・[`parents`]）∪ (v) `also` ∪ (vi) `files`（約束の行の
/// 既存の `.rs`・§34）。各項は宣言順に 1 関数で、解けない項目は typed に `Err`（最初の 1 件・fail-closed）。読めない
/// file は従来どおり [`ClosureError::Unreadable`]。
pub fn derive_write_set(fields: &Fields<'_>, base: &Base<'_>) -> Result<BTreeSet<String>, ClosureError> {
    let texts = texts_of(base.sources)?;
    let mut found = closure(fields.touches, base.sources)?;
    found.extend(teeth_places(fields, base, &texts)?);
    found.extend(surface_closure(fields.surfaces, base.sources, base.snapshots)?);
    found.extend(created(fields.creates, base.tracked)?);
    found.extend(parents(fields.creates, base.tracked));
    found.extend(also_files(fields.also, base.tracked)?);
    found.extend(listed_files(fields.files, base.tracked)?);
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

/// Declared 行の歯の置き場の門（§20・pure）: `verify` の nextest 行ごとに base の歯の file を [`teeth_places`] と
/// **同じ 1 関数**で解き（`tests` 欄は行のまま空・nextest 形でない行は読み飛ばす）、解けた file が行の write-set
/// `written` に全部含まれるかを [`check_teeth_cover`] で測る。base で 0 本の filter 語（新しい接頭辞）は、Declared
/// 行に `tests` 欄が無いので write-set の歯の file（[`teeth_file`] と同じ弁別 = 歯の区間が空でない `.rs`・dir 項目は
/// 配下）を置き場と読む: 1 つも無ければ従来の [`ClosureError::TeethPlaceUnresolved`]（字面不変）。行ごとに解くのは、
/// 先に在る新しい接頭辞の行で止まると後の行の歯の file を測り落とすからである。
pub(crate) fn declared_teeth(fields: &Fields<'_>, base: &Base<'_>, written: &[String]) -> Result<(), ClosureError> {
    let texts = texts_of(base.sources)?;
    let placed = texts.iter().any(|(path, text)| covered(written, path) && !test_region(path, text).is_empty());
    let mut places = BTreeSet::new();
    for line in fields.verify {
        let one = Fields { verify: std::slice::from_ref(line), ..*fields };
        let found = teeth_places(&one, base, &texts);
        // 新しい接頭辞で write-set に歯の file が在る周だけ読み飛ばす（他の理由はそのまま断る）。
        if placed && matches!(found, Err(ClosureError::TeethPlaceUnresolved { .. })) {
            continue;
        }
        places.extend(found?);
    }
    check_teeth_cover(written, &places)
}

/// 解けた歯の file `places` が write-set `written` に全部含まれるか（照合は [`check_drift`] と同じ正規化・dir 項目は
/// その配下・[`covered`]）。無い file を**全部**名指す（辞書順）。
pub(crate) fn check_teeth_cover(written: &[String], places: &BTreeSet<String>) -> Result<(), ClosureError> {
    let files = outside(written, places);
    if files.is_empty() {
        Ok(())
    } else {
        Err(ClosureError::TeethOutsideWriteSet { files })
    }
}

/// `places` のうち write-set `written` に含まれない file（正規化した形・辞書順）。
fn outside(written: &[String], places: &BTreeSet<String>) -> Vec<String> {
    places.iter().filter(|path| !covered(written, path)).map(|path| normalize(path)).collect()
}

/// (ii) 歯の置き場: `verify` の nextest 行ごとに、その crate のその行の scope（[`Scope`]・§28）の歯の区間で `#[test]`
/// の直下の `fn` の名が filter 語を含む file の全部（nextest の positional filter と同じ「含む」・helper の fn は
/// 数えない）。scope で 0 本の filter 語（新しい接頭辞）は `tests` 欄が置き場で、`tests` も無ければ
/// [`ClosureError::TeethPlaceUnresolved`]。`tests` の項目は歯の file だけ（`creates` に在る新規 file は creates の側が
/// write-set に載る）。Declared 行の門（[`declared_teeth`]）も同じ 1 関数で読む（2 本目の読み手を作らない）。
pub(crate) fn teeth_places(fields: &Fields<'_>, base: &Base<'_>, texts: &[(&str, &str)]) -> Result<BTreeSet<String>, ClosureError> {
    let mut found = BTreeSet::new();
    for line in fields.verify {
        let Some((krate, filter, scope)) = nextest_filter(line, base.core_crate) else {
            continue;
        };
        let places: Vec<&str> = texts
            .iter()
            .filter(|(path, text)| {
                in_crate(path, krate) && in_scope(path, krate, scope) && test_fns(test_region(path, text)).iter().any(|name| name.contains(filter))
            })
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

/// verify 行の列の filter 語（宣言順・filter を持たない行と nextest でない行は飛ばす）。gate と land の主実測が検出線の
/// `{teeth}` に置く語の正本で、置き場の導出（[`teeth_places`]・preflight の `teeth=` 行）と同じ [`nextest_filter`] を通す
/// （設計 gate-cost.md §34 約束 5・導出の正本を増やさない）。crate の名は語に効かないので core の crate を問わない。
pub(crate) fn teeth_words(verify: &[String]) -> Vec<&str> {
    verify.iter().filter_map(|line| nextest_filter(line, "").map(|(_, filter, _)| filter)).collect()
}

/// nextest 行の scope（§28・閉じた 3 値・宣言順 = 旗なし / `--lib` / `--test <name>`）＝その行が走らせる target。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope<'l> {
    /// 旗なし＝その crate の全 file。読めない旗（[`UNREAD_ARG_TARGET_FLAGS`] / [`UNREAD_BARE_TARGET_FLAGS`]）と旗が 2 つ以上の
    /// 行もここへ倒す（fail-closed）。
    Crate,
    /// `--lib`＝`crates/<crate>/src/` 配下。
    Lib,
    /// `--test <name>`＝`crates/<crate>/tests/<name>.rs` とその配下 `tests/<name>/`。
    Test(&'l str),
}

/// nextest の行から (crate, filter 語, scope) を読む。書き出しが `cargo nextest run` でない行・filter 語（`-` で
/// 始まらない末尾の語）の無い行は `None`。crate は `-p` / `--package` の次の語・無ければ core の crate。scope の旗
/// （`--lib` / `--test <name>`）が丁度 1 つで読めない旗が無い行だけ狭く読み、他は [`Scope::Crate`]。引数を取る旗
/// （`--test` と [`UNREAD_ARG_TARGET_FLAGS`]）は次の 1 語を消費し（filter 語に数えない）、行末なら `None`。
fn nextest_filter<'l>(line: &'l str, core_crate: &'l str) -> Option<(&'l str, &'l str, Scope<'l>)> {
    let mut words = line.split_whitespace();
    for head in NEXTEST_HEAD {
        if words.next() != Some(*head) {
            return None;
        }
    }
    let (mut krate, mut filter, mut scopes) = (core_crate, None, Vec::new());
    while let Some(word) = words.next() {
        if PACKAGE_FLAGS.contains(&word) {
            krate = words.next()?;
        } else if word == LIB_FLAG {
            scopes.push(Scope::Lib);
        } else if word == TEST_FLAG {
            scopes.push(Scope::Test(words.next()?));
        } else if UNREAD_ARG_TARGET_FLAGS.contains(&word) {
            // 引数を取る読めない旗は次の 1 語を消費する（filter 語に数えない・行末なら読めないと断る＝`--test` と同じ極性）。
            words.next()?;
            scopes.push(Scope::Crate);
        } else if UNREAD_BARE_TARGET_FLAGS.contains(&word) {
            // 読めない旗は「広い側の旗」として数える＝単独でも scope の旗と並んでも Crate へ倒れる。
            scopes.push(Scope::Crate);
        } else if !word.starts_with('-') {
            filter = Some(word);
        }
    }
    let scope = match scopes.as_slice() {
        [one] => *one,
        _ => Scope::Crate,
    };
    Some((krate, filter?, scope))
}

/// `path` が crate `name` の file か（`crates/<name>/` 配下）。
fn in_crate(path: &str, name: &str) -> bool {
    crate_relative(path, name).is_some()
}

/// `path` が行の scope の中か（§28・[`in_crate`] の後段の 1 述語・網羅 match）: 旗なし = crate の全 file / `--lib` =
/// `src/` 配下 / `--test <name>` = `tests/<name>.rs` とその配下 `tests/<name>/`。
fn in_scope(path: &str, krate: &str, scope: Scope<'_>) -> bool {
    crate_relative(path, krate).is_some_and(|rest| match scope {
        Scope::Crate => true,
        Scope::Lib => rest.split('/').next() == Some(SRC_DIR),
        Scope::Test(name) => rest
            .strip_prefix(TESTS_DIR)
            .and_then(|tail| tail.strip_prefix('/'))
            .and_then(|tail| tail.strip_prefix(name))
            .is_some_and(|tail| tail == RS || tail.starts_with('/')),
    })
}

/// `crates/<name>/` を剥がした残り（crate の外は `None`）。
fn crate_relative<'p>(path: &'p str, name: &str) -> Option<&'p str> {
    path.strip_prefix(CRATES_DIR).and_then(|rest| rest.strip_prefix(name)).and_then(|rest| rest.strip_prefix('/'))
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

/// `creates` の親の宣言 file（§17 の形 (vi)・path だけで組む下界・字面は読まない）: 各 `.rs` の項目の dir から候補
/// (vi-1) `<dir>/mod.rs`・(vi-2) `<dir>.rs`・(vi-3) dir が crate の src の根（`crates/<c>/src`）なら同じ dir の `lib.rs`
/// と `main.rs` を組み、base の tracked に在るものを**全部**返す（(vi-3) の 2 つは並び立ち、どちらが `pub mod` を受けるかは
/// 導出で決まらない）。候補がどれも tracked に無い周（dir ごと新設）・`.rs` でない項目・dir を持たない項目は何も足さない。
fn parents(creates: &[String], tracked: &[String]) -> BTreeSet<String> {
    creates
        .iter()
        .filter(|item| item.ends_with(RS))
        .filter_map(|item| item.rsplit_once('/').map(|(dir, _)| dir))
        .flat_map(parent_candidates)
        .filter(|candidate| tracked.contains(candidate))
        .collect()
}

/// dir `dir` の module を宣言しうる file の候補（[`parents`] の 3 形）。
fn parent_candidates(dir: &str) -> Vec<String> {
    let mut found = vec![format!("{dir}/{MOD_STEM}{RS}"), format!("{dir}{RS}")];
    let crate_dir = dir.strip_prefix(CRATES_DIR).and_then(|rest| rest.split_once('/'));
    let src_root = crate_dir.is_some_and(|(krate, rest)| !krate.is_empty() && rest == SRC_DIR);
    if src_root {
        found.extend(CRATE_ROOT_STEMS.iter().map(|stem| format!("{dir}/{stem}{RS}")));
    }
    found
}

/// 約束の行から組んだ導出の入力（設計 §33 項 3・§34・[`Fields`] の 7 欄を所有する形）と、歯ごとの置き場。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Promised {
    /// `symbols` のうち base で閉じた型（enum / struct の宣言が module に在る `crate::module::Type`）に解ける名。
    pub touches: Vec<String>,
    /// `_external_form` の歯の snapshot の名と、歯の本文の名付き snapshot の名（base に snapshot が在るものだけ）。
    pub surfaces: Vec<String>,
    /// 歯 1 本 1 行の nextest 行（導出 (ii) の入力＝filter 語は fn 名・契約 file の `verify` は束ねた別の行）。
    pub verify: Vec<String>,
    /// `files` の `+` の項目（`+` を剥がす）。
    pub creates: Vec<String>,
    /// `place`（`+` を剥がす）。
    pub tests: Vec<String>,
    /// `files` の `+` 無しで `.rs` でない項目。
    pub also: Vec<String>,
    /// `files` の `+` 無しの `.rs`（§34・write-set にそのまま載る）。
    pub files: Vec<String>,
    /// 歯の (完全名, 置き場の file)（`n` の順・約束の行の中は書かれた順）。
    pub teeth: Vec<(String, String)>,
}

impl Promised {
    /// [`derive_write_set`] に渡す形。
    pub fn fields(&self) -> Fields<'_> {
        Fields {
            touches: &self.touches,
            surfaces: &self.surfaces,
            verify: &self.verify,
            creates: &self.creates,
            tests: &self.tests,
            also: &self.also,
            files: &self.files,
        }
    }
}

/// Promised の行の write-set（§33 項 3・pure）: 約束の行から [`Promised`] を組み（[`promised_inputs`]）、§3 の
/// [`derive_write_set`] を**そのまま**撃った値（導出の 1 本は増やさない）。入力も返す（生成の `verify` が歯の置き場を読む）。
pub fn derive_promised(promises: &[&PromiseRow], base: &Base<'_>) -> Result<(BTreeSet<String>, Promised), ClosureError> {
    let inputs = promised_inputs(promises, base)?;
    let found = derive_write_set(&inputs.fields(), base)?;
    Ok((found, inputs))
}

/// 約束の行（`n` の順に並べ直す）から導出の入力を組む。歯の置き場は `place`（在れば）か base で `#[test]` の直下の fn 名が
/// 歯の名の末尾の段と等しい file で、どちらも無ければ [`ClosureError::TeethPlaceUnresolved`]（filter は歯の完全名）。
pub fn promised_inputs(promises: &[&PromiseRow], base: &Base<'_>) -> Result<Promised, ClosureError> {
    let texts = texts_of(base.sources)?;
    let mut ordered = promises.to_vec();
    ordered.sort_by_key(|promise| promise.n);
    let mut out = Promised::default();
    for promise in ordered {
        let symbols = promise.symbols.iter().filter(|symbol| !symbol.starts_with(NEW_FILE) && closed_type(symbol, &texts));
        symbols.for_each(|symbol| push_new(&mut out.touches, symbol));
        for file in &promise.files {
            match file.strip_prefix(NEW_FILE) {
                Some(fresh) => push_new(&mut out.creates, fresh),
                None if !file.ends_with(RS) => push_new(&mut out.also, file),
                None => push_new(&mut out.files, file),
            }
        }
        let place = promise.place.strip_prefix(NEW_FILE).unwrap_or(&promise.place);
        if !place.is_empty() {
            push_new(&mut out.tests, place);
        }
        for tooth in &promise.teeth {
            let file = tooth_file(tooth, place, &texts)?;
            let name = fn_of(tooth);
            push_new(&mut out.verify, &nextest_line(&file, &[name]));
            for surface in tooth_surfaces(name, &file, &texts, base.snapshots) {
                push_new(&mut out.surfaces, &surface);
            }
            out.teeth.push((tooth.clone(), file));
        }
    }
    Ok(out)
}

/// 列に無ければ足す（宣言順を保つ集合）。
fn push_new(list: &mut Vec<String>, item: &str) {
    if !list.iter().any(|found| found == item) {
        list.push(item.to_owned());
    }
}

/// 歯の完全名の末尾の段（fn 名）。
fn fn_of(tooth: &str) -> &str {
    tooth.rsplit("::").next().unwrap_or(tooth)
}

/// 歯の置き場: `place` が在ればそれ・無ければ base の歯の区間で `#[test]` の直下の fn 名が歯の fn 名と等しい最初の file。
fn tooth_file(tooth: &str, place: &str, texts: &[(&str, &str)]) -> Result<String, ClosureError> {
    if !place.is_empty() {
        return Ok(place.to_owned());
    }
    let name = fn_of(tooth);
    texts
        .iter()
        .find(|(path, text)| test_fns(test_region(path, text)).contains(&name))
        .map(|(path, _)| (*path).to_owned())
        .ok_or_else(|| ClosureError::TeethPlaceUnresolved { filter: tooth.to_owned() })
}

/// 歯の外形の名: fn 名が `_external_form` で終わればその名・置き場の本文の歯の fn の中の名付き snapshot の名。どちらも
/// base に snapshot が在る名だけ（無い名は [`surface_closure`] が解けない＝新設の snapshot は置き場の側が持つ）。
fn tooth_surfaces(name: &str, file: &str, texts: &[(&str, &str)], snapshots: &[Source]) -> Vec<String> {
    let mut found: Vec<&str> = Vec::new();
    if name.ends_with(EXTERNAL_FORM) {
        found.push(name);
    }
    if let Some((_, text)) = texts.iter().find(|(path, _)| *path == file) {
        found.extend(named_snapshots(fn_body(test_region(file, text), name)));
    }
    let known = |candidate: &&str| snapshots.iter().any(|snap| snapshot_name(&snap.path) == Some(*candidate));
    found.into_iter().filter(known).map(str::to_owned).collect()
}

/// 歯の区間の中の `fn name(` から次の `#[test]` の行（無ければ末尾）まで（無ければ空）。
fn fn_body<'t>(region: &'t str, name: &str) -> &'t str {
    let head = format!("fn {name}(");
    let Some(at) = region.find(&head) else {
        return "";
    };
    let rest = region.get(at..).unwrap_or_default();
    let end = rest.find(&format!("\n{TEST_ATTR}")).or_else(|| rest.find(&format!("    {TEST_ATTR}"))).unwrap_or(rest.len());
    rest.get(..end).unwrap_or(rest)
}

/// 本文の名付き snapshot の名（`snapshot!(` の直後が `"名"` と `,` の形・値だけの literal は名でない）。
fn named_snapshots(body: &str) -> Vec<&str> {
    body.match_indices(SNAPSHOT_CALL)
        .filter_map(|(at, _)| {
            let rest = body.get(at.saturating_add(SNAPSHOT_CALL.len())..)?.trim_start().strip_prefix('"')?;
            let (name, tail) = rest.split_once('"')?;
            tail.trim_start().starts_with(',').then_some(name)
        })
        .collect()
}

/// 置き場の file の歯を `filters` で撃つ nextest 行（§28 の scope を file の path から読む・[`in_scope`] の逆）:
/// `crates/<c>/src/` は `-p <c> --lib`・`crates/<c>/tests/<name>[.rs|/…]` は `-p <c> --test <name>`・crate の他の file は
/// `-p <c>`・`crates/` の外は旗なし（core の crate）。末尾に `--no-tests=fail` と filter 語を空白で並べる。
pub(crate) fn nextest_line(file: &str, filters: &[&str]) -> String {
    let (krate, scope) = target_of(file);
    let mut words: Vec<String> = NEXTEST_HEAD.iter().map(|word| (*word).to_owned()).collect();
    if let (Some(krate), Some(flag)) = (krate, PACKAGE_FLAGS.first()) {
        words.extend([(*flag).to_owned(), krate.to_owned()]);
    }
    match scope {
        Scope::Crate => {}
        Scope::Lib => words.push(LIB_FLAG.to_owned()),
        Scope::Test(name) => words.extend([TEST_FLAG.to_owned(), name.to_owned()]),
    }
    words.push(NO_TESTS_FLAG.to_owned());
    words.extend(filters.iter().map(|filter| (*filter).to_owned()));
    words.join(" ")
}

/// file の path の (crate, scope)（§28 の 3 値・`crates/` の外は crate 無しの [`Scope::Crate`]）。
fn target_of(file: &str) -> (Option<&str>, Scope<'_>) {
    let Some((krate, rest)) = file.strip_prefix(CRATES_DIR).and_then(|rest| rest.split_once('/')) else {
        return (None, Scope::Crate);
    };
    let mut segments = rest.split('/');
    let scope = match (segments.next(), segments.next()) {
        (Some(SRC_DIR), _) => Scope::Lib,
        (Some(TESTS_DIR), Some(next)) => Scope::Test(next.strip_suffix(RS).unwrap_or(next)),
        _ => Scope::Crate,
    };
    (Some(krate), scope)
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

/// (vi) `files`（§34）: 約束の行の `files` の `+` 無しの `.rs` を**そのまま**載せる。base に無ければ
/// [`ClosureError::ItemUnresolved`]（黙って捨てない）。
fn listed_files(items: &[String], tracked: &[String]) -> Result<BTreeSet<String>, ClosureError> {
    items
        .iter()
        .map(|item| match tracked.iter().any(|found| found == item) {
            true => Ok(item.clone()),
            false => Err(ClosureError::ItemUnresolved { item: item.clone() }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.363

    use super::super::tests::{set, source, PAINT};
    use super::{check_drift, check_teeth_cover, declared_teeth, derive_write_set, weighted_lines, Base, ClosureError, Fields, Source};
    use super::{derive_promised, in_crate, in_scope, nextest_filter, nextest_line, PromiseRow};
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
        let (creates, tests, also, files) = (of("creates"), of("tests"), of("also"), of("files"));
        let fields = Fields {
            touches: &touches,
            surfaces: &surfaces,
            verify: &verify,
            creates: &creates,
            tests: &tests,
            also: &also,
            files: &files,
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

    // flip-check: s2-07l.391

    /// Declared 行の門（§20・(d)）: 解けた歯の file {e2e.rs, other.rs} に対し write-set {tint.rs, other.rs} は e2e.rs を
    /// 名指して断り・両方を持てば通り・dir 項目 `crates/toy/tests/` は配下の e2e.rs を含むと読む（3 分岐）。
    #[test]
    fn contract_declared_teeth_gate_names_missing_files_sorted() {
        let places = set(&["crates/toy/tests/e2e.rs", "crates/toy/src/other.rs"]);
        let short = strings(&["crates/toy/src/tint.rs", "crates/toy/src/other.rs"]);
        assert_eq!(
            check_teeth_cover(&short, &places),
            Err(ClosureError::TeethOutsideWriteSet { files: strings(&["crates/toy/tests/e2e.rs"]) }),
            "write-set に無い歯の file を名指す"
        );
        let full = strings(&["crates/toy/src/tint.rs", "./crates/toy/tests/e2e.rs", "crates/toy/src/other.rs"]);
        assert_eq!(check_teeth_cover(&full, &places), Ok(()), "両方を持てば通る（正規化して比べる）");
        let dir = strings(&["crates/toy/tests/", "crates/toy/src/other.rs"]);
        assert_eq!(check_teeth_cover(&dir, &places), Ok(()), "dir 項目は配下を含む");
        let none = strings(&["crates/toy/src/tint.rs"]);
        let reason = check_teeth_cover(&none, &places).map_err(|error| error.reason()).err().unwrap_or_default();
        assert!(reason.contains("crates/toy/src/other.rs, crates/toy/tests/e2e.rs"), "全部を辞書順で名乗る: {reason}");
    }

    /// Declared 行の門の入口（§20・(a)〜(c) の pure な対）: `derive_` の歯（other.rs / e2e.rs）を write-set が欠けば
    /// 両方を名指し・持てば通り・nextest 形でない行は読み飛ばす。base で 0 本の `fresh_` は write-set に歯の file が
    /// 無ければ従来の `TeethPlaceUnresolved`・在れば通る（後ろに並ぶ `derive_` の行の歯は落とさない）。
    #[test]
    fn contract_declared_teeth_resolves_each_line_and_reads_a_written_teeth_file_as_the_place() {
        let (sources, tracked) = derive_base();
        let base = Base { sources: &sources, snapshots: &[], tracked: &tracked, core_crate: "toy" };
        let gate = |lines: &[&str], written: &[&str]| {
            let verify = strings(lines);
            let fields = Fields { touches: &[], surfaces: &[], verify: &verify, creates: &[], tests: &[], also: &[], files: &[] };
            declared_teeth(&fields, &base, &strings(written))
        };
        let derive_line = "cargo nextest run -p toy --no-tests=fail derive_";
        assert_eq!(
            gate(&[derive_line, "git status"], &["crates/toy/src/tint.rs"]),
            Err(ClosureError::TeethOutsideWriteSet { files: strings(&["crates/toy/src/other.rs", "crates/toy/tests/e2e.rs"]) }),
            "歯の file を欠く write-set は両方を名指す（helper.rs は歯の file でない）"
        );
        assert_eq!(gate(&[derive_line], &["crates/toy/src/tint.rs", "crates/toy/src/other.rs", "crates/toy/tests/e2e.rs"]), Ok(()));
        assert_eq!(gate(&["git status"], &["crates/toy/src/tint.rs"]), Ok(()), "nextest 形でない行は読み飛ばす");
        let fresh_line = "cargo nextest run -p toy --no-tests=fail fresh_";
        assert_eq!(
            gate(&[fresh_line], &["crates/toy/src/tint.rs"]),
            Err(ClosureError::TeethPlaceUnresolved { filter: "fresh_".to_owned() }),
            "write-set に歯の file が無ければ従来の理由"
        );
        assert_eq!(gate(&[fresh_line], &["crates/toy/src/tint.rs", "crates/toy/tests/e2e.rs"]), Ok(()), "歯の file が置き場");
        assert_eq!(
            gate(&[fresh_line, derive_line], &["crates/toy/tests/e2e.rs"]),
            Err(ClosureError::TeethOutsideWriteSet { files: strings(&["crates/toy/src/other.rs"]) }),
            "先の行が新しい接頭辞でも後の行の歯は測る"
        );
    }

    // flip-check: s2-07l.451

    /// §28 (a): `--test e2e` の行の置き場は `tests/e2e.rs` とその配下 `tests/e2e/nested.rs` の 2 本に等しい（同じ filter 語に
    /// 当たる src の in-file の歯 `other.rs` は返さない）。`--test helper` は `helper.rs` だけ・別 crate の `other` の歯は
    /// どちらも返さない（`in_crate` の側は不変）。(e) Declared 行の門も同じ 1 関数で scope を読む＝同じ行に target の 2 file
    /// だけの write-set を渡すと通る（crate 全体を読めば `other.rs` を名指して断る）。
    #[test]
    fn closure_scope_test_target_keeps_only_that_targets_files() {
        let (mut sources, tracked) = derive_base();
        sources.push(source("crates/toy/tests/e2e/nested.rs", "#[test]\nfn derive_nested() {}\n"));
        let e2e_line = "cargo nextest run -p toy --test e2e --no-tests=fail derive_";
        let target = set(&["crates/toy/tests/e2e.rs", "crates/toy/tests/e2e/nested.rs"]);
        let teeth = |line: &str| derive(&[("verify", &[line])], &sources, &tracked);
        assert_eq!(teeth(e2e_line), Ok(target.clone()), "--test e2e は target の file とその配下だけ");
        assert_eq!(teeth("cargo nextest run -p toy --test helper other_"), Ok(set(&["crates/toy/tests/helper.rs"])), "--test helper");
        let base = Base { sources: &sources, snapshots: &[], tracked: &tracked, core_crate: "toy" };
        let verify = strings(&[e2e_line]);
        let fields = Fields { touches: &[], surfaces: &[], verify: &verify, creates: &[], tests: &[], also: &[], files: &[] };
        let written: Vec<String> = target.into_iter().collect();
        assert_eq!(declared_teeth(&fields, &base, &written), Ok(()), "(e) 門は src の歯の file を要求しない");
    }

    /// §28 (b): `--lib` の行の置き場は `src/` の歯の file `other.rs` の 1 本だけ（`tests/e2e.rs` は返さない）。
    #[test]
    fn closure_scope_lib_keeps_only_src_side_teeth() {
        let (sources, tracked) = derive_base();
        let got = derive(&[("verify", &["cargo nextest run -p toy --lib --no-tests=fail derive_"])], &sources, &tracked);
        assert_eq!(got, Ok(set(&["crates/toy/src/other.rs"])), "--lib は src 配下だけ");
    }

    /// §28 (c): 旗なし・読めない旗（`--bin toy`）・scope の旗が 2 つ（`--lib --test e2e`）の行は 3 つとも crate 全体
    /// （src も tests も）＝広い側へ倒す。返りの集合を等値で測る（空で通らない）。
    #[test]
    fn closure_scope_unknown_or_repeated_flags_stay_crate_wide() {
        let (sources, tracked) = derive_base();
        let wide = set(&["crates/toy/src/other.rs", "crates/toy/tests/e2e.rs"]);
        for line in [
            "cargo nextest run -p toy --no-tests=fail derive_",
            "cargo nextest run -p toy --bin toy --no-tests=fail derive_",
            "cargo nextest run -p toy --lib --test e2e --no-tests=fail derive_",
        ] {
            assert_eq!(derive(&[("verify", &[line])], &sources, &tracked), Ok(wide.clone()), "{line}");
        }
    }

    /// §28 (d): `--test e2e` の scope に filter 語 `in_src` の歯が無く（`e2e.rs` / `e2e/nested.rs`）src の `other.rs` にだけ
    /// 在る周は、`tests` 欄が無ければ従来どおり `TeethPlaceUnresolved`（字面不変・crate 全体を読んで解いてしまわない）。
    #[test]
    fn closure_scope_zero_files_without_tests_is_still_unresolved() {
        let (mut sources, tracked) = derive_base();
        sources.push(source("crates/toy/tests/e2e/nested.rs", "#[test]\nfn derive_nested() {}\n"));
        let got = derive(&[("verify", &["cargo nextest run -p toy --test e2e --no-tests=fail in_src"])], &sources, &tracked);
        assert_eq!(got, Err(ClosureError::TeethPlaceUnresolved { filter: "in_src".to_owned() }), "scope の外の歯では解かない");
    }

    // flip-check: s2-07l.512

    /// 約束の行 1 つ（`of` = `ag`・`text` / `fixture` / `expect` は固定）。
    fn promise(n: u64, symbols: &[&str], files: &[&str], teeth: &[&str], place: &str) -> PromiseRow {
        PromiseRow {
            line: n,
            of: "ag".to_owned(),
            n,
            text: format!("約束 {n}"),
            files: strings(files),
            symbols: strings(symbols),
            teeth: strings(teeth),
            place: place.to_owned(),
            fixture: "fixture".to_owned(),
            expect: "expect".to_owned(),
        }
    }

    /// 約束の行の導出の fixture: crate `toy` の型（`crate::paint::Hue`）と arm・src の区間の歯・tests の歯 3 本（外形の歯
    /// `toy_external_form`・名付き snapshot の歯 `lens_case`〔`assert_snapshot!("<名>", 値)` の形〕・値だけの literal の歯
    /// `doctor_case`）と 2 本の snapshot・非 `.rs` の面。
    fn promise_base() -> (Vec<Source>, Vec<Source>, Vec<String>) {
        let e2e = "#[test]\nfn derive_ok() {}\n\n#[test]\nfn toy_external_form() {\n    insta::assert_snapshot!(form);\n}\n\n#[test]\nfn lens_case() {\n    insta::assert_snapshot!(\"lens_prompt_external_form\", prompt);\n}\n\n#[test]\nfn doctor_case() {\n    insta::assert_snapshot!(\"doctor: ok\");\n}\n";
        let sources = vec![
            source("crates/toy/src/paint.rs", PAINT),
            source("crates/toy/src/show.rs", "use crate::paint::Hue;\n\npub fn name(hue: Hue) -> u8 {\n    match hue {\n        Hue::Red => 1,\n        _ => 0,\n    }\n}\n"),
            source("crates/toy/src/other.rs", "pub fn plain() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn derive_in_src() {}\n}\n"),
            source("crates/toy/tests/e2e.rs", e2e),
        ];
        let snapshots = vec![
            source("crates/toy/tests/e2e/snapshots/e2e__toy_external_form.snap", "---\nsource: e2e\n---\nform\n"),
            source("crates/toy/tests/e2e/snapshots/e2e__lens_prompt_external_form.snap", "---\nsource: e2e\n---\nprompt\n"),
        ];
        let mut tracked: Vec<String> = sources.iter().chain(&snapshots).map(|found| found.path.clone()).collect();
        tracked.extend(strings(&["rules/manifest.toml", "docs/design/toy.md"]));
        (sources, snapshots, tracked)
    }

    /// §33 (b): 約束の行 3 つ（書いた順は n = 3, 1, 2）から `symbols` の閉じた型だけが `touches` に（`+` の名と `crate::` の無い
    /// 字面は載らない）・`+` の file が `creates` に・`.rs` でない file が `also` に・`place` が `tests` に・`_external_form` の
    /// 歯と名付き snapshot の歯の名が `surfaces` に（値だけの literal は名でない）写り、歯は n の順に置き場と組になる。
    /// write-set は §3 の [`derive_write_set`] を同じ入力で撃った値と一致する（導出の 1 本は増えない）。
    #[test]
    fn contract_promise_derive_maps_promises_to_the_six_fields_and_the_same_write_set() {
        let (sources, snapshots, tracked) = promise_base();
        let base = Base { sources: &sources, snapshots: &snapshots, tracked: &tracked, core_crate: "toy" };
        let first = promise(
            1,
            &["crate::paint::Hue", "+crate::paint::Fresh", "Hue::Red"],
            &["crates/toy/src/paint.rs", "+crates/toy/src/new.rs", "rules/manifest.toml"],
            &["derive_ok", "toy_external_form"],
            "",
        );
        let second = promise(2, &[], &["crates/toy/src/other.rs"], &["derive_in_src", "lens_case", "doctor_case"], "");
        let third = promise(3, &[], &["+crates/toy/tests/fresh.rs"], &["fresh::fresh_case"], "+crates/toy/tests/fresh.rs");
        let (found, inputs) = derive_promised(&[&third, &first, &second], &base).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(inputs.touches, strings(&["crate::paint::Hue"]), "閉じた型だけ");
        assert_eq!(inputs.creates, strings(&["crates/toy/src/new.rs", "crates/toy/tests/fresh.rs"]), "+ の file");
        assert_eq!(inputs.also, strings(&["rules/manifest.toml"]), ".rs でない file");
        assert_eq!(inputs.tests, strings(&["crates/toy/tests/fresh.rs"]), "place");
        assert_eq!(inputs.surfaces, strings(&["toy_external_form", "lens_prompt_external_form"]), "外形の歯と名付き snapshot");
        let (e2e, other, fresh) = ("crates/toy/tests/e2e.rs", "crates/toy/src/other.rs", "crates/toy/tests/fresh.rs");
        let teeth: Vec<(&str, &str)> = inputs.teeth.iter().map(|(name, file)| (name.as_str(), file.as_str())).collect();
        let want_teeth = [
            ("derive_ok", e2e),
            ("toy_external_form", e2e),
            ("derive_in_src", other),
            ("lens_case", e2e),
            ("doctor_case", e2e),
            ("fresh::fresh_case", fresh),
        ];
        assert_eq!(teeth, want_teeth, "歯は n の順に置き場と組");
        assert_eq!(found, derive_write_set(&inputs.fields(), &base).unwrap_or_default(), "§3 の関数の値と一致");
        let want = set(&[
            "+crates/toy/src/new.rs",
            "+crates/toy/tests/fresh.rs",
            "crates/toy/src/other.rs",
            "crates/toy/src/paint.rs",
            "crates/toy/src/show.rs",
            "crates/toy/tests/e2e.rs",
            "crates/toy/tests/e2e/snapshots/e2e__lens_prompt_external_form.snap",
            "crates/toy/tests/e2e/snapshots/e2e__toy_external_form.snap",
            "rules/manifest.toml",
        ]);
        assert_eq!(found, want, "閉包 ∪ 歯の置き場 ∪ 外形 pin ∪ creates ∪ also");
    }

    /// §33 (b) の負の枝: base に無い歯で `place` の無い約束の行は `TeethPlaceUnresolved`（filter は歯の完全名）で、同じ歯に
    /// `place` を足せば置き場になる。`place` の在る他の約束の行があっても、`place` の無い約束の行の歯は緩まない。
    #[test]
    fn contract_promise_derive_refuses_a_tooth_without_base_or_place() {
        let (sources, snapshots, tracked) = promise_base();
        let base = Base { sources: &sources, snapshots: &snapshots, tracked: &tracked, core_crate: "toy" };
        let placed = promise(1, &[], &[], &["fresh_case"], "crates/toy/tests/e2e.rs");
        let bare = promise(2, &[], &[], &["pipe::nope_case"], "");
        assert_eq!(
            derive_promised(&[&placed, &bare], &base).map(|(found, _)| found),
            Err(ClosureError::TeethPlaceUnresolved { filter: "pipe::nope_case".to_owned() }),
            "place も base の歯も無い"
        );
        let found = derive_promised(&[&placed], &base).map(|(found, _)| found);
        assert_eq!(found, Ok(set(&["crates/toy/tests/e2e.rs"])), "place が置き場");
    }

    /// 生成する nextest 行の scope は置き場の path の（crate・scope）で、同じ行を §28 の読み手（[`super::nextest_filter`]）で
    /// 読み戻すと、置き場の file は行の crate と scope の中に在る（`src/` = `--lib`・`tests/<name>` = `--test <name>`・他は旗なし）。
    #[test]
    fn contract_promise_derive_nextest_line_reads_back_into_the_same_scope() {
        for (file, want) in [
            ("crates/toy/src/a/b.rs", "cargo nextest run -p toy --lib --no-tests=fail x"),
            ("crates/toy/tests/e2e.rs", "cargo nextest run -p toy --test e2e --no-tests=fail x"),
            ("crates/toy/tests/e2e/pipe/intake.rs", "cargo nextest run -p toy --test e2e --no-tests=fail x"),
            ("crates/toy/benches/z.rs", "cargo nextest run -p toy --no-tests=fail x"),
        ] {
            let line = nextest_line(file, &["x"]);
            assert_eq!(line, want, "{file}");
            let (krate, filter, scope) = nextest_filter(&line, "core").unwrap_or_else(|| panic!("読み戻せる: {line}"));
            assert!(in_crate(file, krate) && in_scope(file, krate, scope) && filter == "x", "{file} は行の scope の中");
        }
        assert_eq!(nextest_line("src/lib.rs", &["x", "y"]), "cargo nextest run --no-tests=fail x y", "crates/ の外は core の crate");
    }

    // flip-check: s2-07l.528

    /// §34 (a): `files` の `+` 無しの `.rs`（base に実在・閉包にも歯の置き場にも無い `show.rs`）は `files` の欄に写り、
    /// write-set にそのまま載る（base は黙って捨てる → RED）。`.rs` でない file は従来どおり `also` で `files` に混ざらない。
    #[test]
    fn contract_promise_files_existing_rs_lands_in_the_write_set_as_is() {
        let (sources, snapshots, tracked) = promise_base();
        let base = Base { sources: &sources, snapshots: &snapshots, tracked: &tracked, core_crate: "toy" };
        let one = promise(1, &[], &["crates/toy/src/show.rs", "rules/manifest.toml"], &["derive_ok"], "");
        let (found, inputs) = derive_promised(&[&one], &base).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(inputs.files, strings(&["crates/toy/src/show.rs"]), "+ 無しの .rs だけ");
        assert_eq!(inputs.also, strings(&["rules/manifest.toml"]), ".rs でない file は also のまま");
        let want = set(&["crates/toy/src/show.rs", "crates/toy/tests/e2e.rs", "rules/manifest.toml"]);
        assert_eq!(found, want, "歯の置き場 ∪ also ∪ files");
        assert_eq!(found, derive_write_set(&inputs.fields(), &base).unwrap_or_default(), "§3 の関数の値と一致");
    }

    /// §34 (b): base に無い `.rs` を `+` 無しで書いた約束の行は `ItemUnresolved`（base は黙って捨てて通す → RED）。§3 の
    /// 関数に `files` を直に渡しても同じ理由で断る。
    #[test]
    fn contract_promise_files_missing_rs_is_item_unresolved() {
        let (sources, snapshots, tracked) = promise_base();
        let base = Base { sources: &sources, snapshots: &snapshots, tracked: &tracked, core_crate: "toy" };
        let one = promise(1, &[], &["crates/toy/src/none.rs"], &["derive_ok"], "");
        let want = ClosureError::ItemUnresolved { item: "crates/toy/src/none.rs".to_owned() };
        assert_eq!(derive_promised(&[&one], &base).map(|(found, _)| found), Err(want.clone()), "base に無い .rs");
        let (sources, tracked) = derive_base();
        assert_eq!(derive(&[("files", &["crates/toy/src/none.rs"])], &sources, &tracked), Err(want), "§3 の関数も同じ");
    }

    /// §34 (c)(d): `crate::paint::Hue` は `paint::Hue` の字面を持たない base（宣言 file だけ）でも module の宣言で解ける
    /// （base は末尾 2 節の字面を探して解けない → RED）。宣言の無い `crate::paint::Fresh` と別 module の `crate::show::Hue`
    /// は解けない。末尾 2 節の `Hue::Red` の形と fn 形は従来どおり解ける。
    #[test]
    fn contract_promise_files_crate_type_path_resolves_by_module_declaration() {
        let sources = vec![source("crates/toy/src/paint.rs", PAINT), source("crates/toy/src/show.rs", "pub fn name() -> u8 {\n    0\n}\n")];
        let literal = sources.iter().any(|found| found.body.as_deref().unwrap_or_default().contains("paint::Hue"));
        assert!(!literal, "fixture は paint::Hue の字面を持たない");
        let names = ["crate::paint::Hue", "crate::paint::Fresh", "crate::show::Hue", "Hue::Red", "name("];
        let found = super::super::symbols_in_base(&names, &[], &sources);
        assert_eq!(found, Ok(vec![Some(true), Some(false), Some(false), Some(true), Some(true)]), "{names:?}");
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

    // flip-check: s2-07l.538

    /// 行 ak (a)(b)(d)(e): 引数を取る旗（`--bin` / `--bench` / `--example` / `-E`）は次の 1 語を消費して filter 語に数えず、
    /// 取らない旗は従来どおり。scope はどちらも `Crate`（scope の旗と並んでも広い側）。
    #[test]
    fn contract_derive_target_flag_consumes_its_argument_and_falls_to_crate() {
        for (line, filter) in [
            ("cargo nextest run -p x --bin x foo_", "foo_"),
            ("cargo nextest run -p x --bin x --test face foo_", "foo_"),
            ("cargo nextest run -p x --bins foo_", "foo_"),
            ("cargo nextest run -p x -E expr bar_", "bar_"),
            ("cargo nextest run -p x foo_ --bin x", "foo_"),
            ("cargo nextest run -p x foo_ --bench b", "foo_"),
            ("cargo nextest run -p x foo_ --example e", "foo_"),
            ("cargo nextest run -p x --lib foo_ -E expr", "foo_"),
        ] {
            assert_eq!(nextest_filter(line, "core"), Some(("x", filter, super::Scope::Crate)), "{line}");
        }
    }

    /// 行 ak (c): 引数を取る旗が行末（次の語が無い）なら読めないと断る（`None`）。引数を取らない旗の後ろの filter 語は残る。
    #[test]
    fn contract_derive_target_flag_at_line_end_is_unreadable() {
        for line in ["cargo nextest run -p x --bin", "cargo nextest run -p x foo_ --bin", "cargo nextest run -p x foo_ -E"] {
            assert_eq!(nextest_filter(line, "core"), None, "{line}");
        }
        assert_eq!(
            nextest_filter("cargo nextest run -p x foo_ --all-targets", "core"),
            Some(("x", "foo_", super::Scope::Crate)),
            "取らない旗は語を消費しない"
        );
    }
}
