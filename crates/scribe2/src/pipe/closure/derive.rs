//! write-set の導出（設計 docs/design/contract-source.md §3「write-set の導出」・契約 (h)・SRS FR48 / FR39）。
//!
//! 型の閉包（[`super::closure`]）・外形 pin（[`super::surface_closure`]）と同じ **pure** な字面走査で、契約表の行の
//! 欄（`touches` / `verify` / `surfaces` / `creates` / `tests` / `also`）から write-set を**導出値**として作る
//! [`derive_write_set`] = 閉包 ∪ 歯の置き場（verify の nextest 行の scope〔[`Scope`]・§28〕の中で base の `#[test]` の
//! fn 名が filter 語を含む file）∪ 外形 pin ∪ 新規 file ∪ Rust の外の file。手書きの write-set は [`check_drift`] で
//! 導出値との集合一致だけを認める（接頭辞 `+` は剥がして比べる）。行の数え方 [`weighted_lines`] も上限の余地の式として
//! ここに置く。
//!
//! **Declared 行の歯の置き場の門**（§20・行 t）[`declared_teeth`]: 導出も drift も撃たない Declared 行でも、`verify` の
//! nextest 行の歯の file は同じ [`teeth_places`] で解き、行の write-set に無い file を全部名指して断る
//! （[`check_teeth_cover`]・照合は [`check_drift`] と同じ正規化・dir 項目は配下）。
//!
//! **契約の散文の閉包**（§27・行 aa）[`prose_closure`]: 契約 file の `goal` / `done` の backtick 字面から (a) 既存の歯の
//! 名指しと (b) 判定行 token の pin（第 7 形）を読む 1 本の pure 関数。行の欄ではなく契約 file の散文を入力にするので
//! [`Fields`] は広げず別の引数で受ける。(a) の被覆は [`nextest_filter`]・(a) の歯の file の門は §20 と同じ
//! [`check_teeth_cover`]・(b) の門は対の [`check_pin_cover`] で、撃つ場所は受付（`cli::intake` の `settle_write_set`）だけ。
//!
//! 型の閉包の字面走査（4 形と [`super::sees`]）は親 module `closure.rs` に置いたまま（1 関数の判定で結ばれる）。

use super::names::backticked;
use super::super::refuse::{covered, normalize, NEW_FILE};
use super::{closure, is_ident, is_ident_char, surface_closure, test_region, texts_of, ClosureError, Source};
use super::{CRATES_DIR, LIB_FLAG, NEXTEST_HEAD, PACKAGE_FLAGS, RS, SRC_DIR, TESTS_DIR, TEST_ATTR, TEST_FLAG, UNREAD_TARGET_FLAGS};
use std::collections::BTreeSet;

/// 歯の名指しの末尾に付いてよい形（§27 (a)・`derive_ok()` / `derive_ok(` も同じ名指し・長い側から剥がす）。
const CALL_TAILS: &[&str] = &["()", "("];

/// 判定行 token の key と value の区切り（§27 (b)）。
const TOKEN_EQ: char = '=';

/// 判定行 token の key に使える記号（英数字に加えて・§27 (b)）。
const KEY_SYMBOLS: &[char] = &['_', '-'];

/// value が具体でない印（`<…>` の穴と `a|b` の選択肢・§27 (b)・placeholder は `<key>=` までを literal にする）。
const PLACEHOLDERS: &[char] = &['<', '|'];

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

/// 散文が pin する判定行 token の file `pins` が write-set `written` に全部含まれるか（§27 (b)・照合は
/// [`check_teeth_cover`] と**同じ 1 本**[`outside`]・理由だけが別の variant）。無い file を**全部**名指す（辞書順）。
pub(crate) fn check_pin_cover(written: &[String], pins: &BTreeSet<String>) -> Result<(), ClosureError> {
    let files = outside(written, pins);
    if files.is_empty() {
        Ok(())
    } else {
        Err(ClosureError::PinsOutsideWriteSet { files })
    }
}

/// `places` のうち write-set `written` に含まれない file（正規化した形・辞書順・2 つの門が同じ畳み方で数える）。
fn outside(written: &[String], places: &BTreeSet<String>) -> Vec<String> {
    places.iter().filter(|path| !covered(written, path)).map(|path| normalize(path)).collect()
}

/// 契約の散文（`goal` / `done`）が名指すもの（§27・行 aa・[`prose_closure`] の出力・閉じた struct）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Prose {
    /// 名指した**既存の**歯の名（base の `#[test]` の fn 名に等しいもの・辞書順・一意）。
    pub(crate) teeth: Vec<String>,
    /// そのうち `verify` のどの nextest 行の filter 語も含まない名（辞書順・全部）。
    pub(crate) uncovered: Vec<String>,
    /// 名指した歯を宣言する file（repo 相対・辞書順）。
    pub(crate) files: BTreeSet<String>,
    /// 判定行 token の literal を歯の区間か外形 snapshot に持つ file（repo 相対・辞書順）。
    pub(crate) pins: BTreeSet<String>,
}

/// 契約の散文の閉包（§27・行 aa・**pure**）: `prose`（契約 file の `goal` / `done` の本文）の backtick 字面から
/// (a) 既存の歯の名指しと (b) 判定行 token の pin を読む。
///
/// (a) 識別子形（[`named`]）で base の `#[test]` の fn 名（[`test_fns`] × [`super::test_region`]）に**等しい**名だけを
/// 既存の歯の名指しと読み（base に無い名は判定しない＝下界・新しい歯は §20 の門と FR20 の guard が拾う）、`verify` の
/// nextest 行の filter 語（[`filters_of`]）を 1 つも含まない名を [`Prose::uncovered`] に、その歯を宣言する file を
/// [`Prose::files`] に置く。
///
/// (b) 判定行 token の literal（[`pinned`]）を歯の区間か外形 snapshot に持つ file を [`Prose::pins`] に置く
/// （[`pinned_files`]・src の区間の外の同じ字面は数えない）。
///
/// 名も token も無い散文は base を走査しない（費用を掛けない・[`surface_closure`] と同じ）。読めない file が 1 本でも
/// 在れば `Err`（fail-closed・NFR4）。
pub(crate) fn prose_closure(prose: &[&str], verify: &[String], base: &Base<'_>) -> Result<Prose, ClosureError> {
    let (names, literals) = (named(prose), pinned(prose));
    if names.is_empty() && literals.is_empty() {
        return Ok(Prose::default());
    }
    let (texts, snapshots) = (texts_of(base.sources)?, texts_of(base.snapshots)?);
    let (mut teeth, mut files) = (BTreeSet::new(), BTreeSet::new());
    for &(path, text) in &texts {
        let fns = test_fns(test_region(path, text));
        let mine: Vec<&str> = names.iter().copied().filter(|name| fns.contains(name)).collect();
        if !mine.is_empty() {
            files.insert(path.to_owned());
            teeth.extend(mine);
        }
    }
    let filters = filters_of(verify, base.core_crate);
    let uncovered: Vec<String> = teeth
        .iter()
        .filter(|name| !filters.iter().any(|filter| name.contains(filter)))
        .map(|name| (*name).to_owned())
        .collect();
    let pins = pinned_files(&literals, &texts, &snapshots);
    Ok(Prose { teeth: teeth.into_iter().map(str::to_owned).collect(), uncovered, files, pins })
}

/// 散文の backtick 字面のうち歯の名の候補（§27 (a)・識別子形＝小文字始まりの英数字と `_`・末尾の `(` / `()` は剥がす）。
/// 大文字始まり・`::` を持つ形・`-` を含む形は名指しと読まない（`NameUnresolved` の領分と重ねない）。
fn named<'p>(prose: &[&'p str]) -> Vec<&'p str> {
    let mut found = Vec::new();
    for &text in prose {
        for piece in backticked(text) {
            let ident = CALL_TAILS.iter().find_map(|tail| piece.strip_suffix(tail)).unwrap_or(piece);
            if ident.starts_with(|first: char| first.is_ascii_lowercase()) && is_ident(ident) {
                found.push(ident);
            }
        }
    }
    found
}

/// 散文の backtick 字面の中の判定行 token の literal（§27 (b)・字面は空白で語に割ってから読む）。
fn pinned<'p>(prose: &[&'p str]) -> Vec<&'p str> {
    let mut found = Vec::new();
    for &text in prose {
        for piece in backticked(text) {
            found.extend(piece.split_whitespace().filter_map(token_literal));
        }
    }
    found
}

/// 1 語が `<key>=<value>` の token なら pin する literal（value が具体なら語の全体・placeholder〔`<…>` / `a|b`〕と空なら
/// `<key>=` まで）。key の形（小文字始まりの英数字と `_` `-`）でない語は `None`。
fn token_literal(word: &str) -> Option<&str> {
    let (key, value) = word.split_once(TOKEN_EQ)?;
    let formed = key.starts_with(|first: char| first.is_ascii_lowercase())
        && key.chars().all(|found| found.is_ascii_alphanumeric() || KEY_SYMBOLS.contains(&found));
    if !formed {
        return None;
    }
    if value.is_empty() || value.contains(PLACEHOLDERS) {
        // `=` は 1 byte なので key の長さの次が value の頭である。
        return word.get(..key.len().saturating_add(1));
    }
    Some(word)
}

/// 判定行 token の literal を持つ file（歯の区間の `.rs` と外形 snapshot の全文・§27 (b)）。
fn pinned_files(literals: &[&str], texts: &[(&str, &str)], snapshots: &[(&str, &str)]) -> BTreeSet<String> {
    let holds = |text: &str| literals.iter().any(|literal| text.contains(*literal));
    let mut found: BTreeSet<String> = texts
        .iter()
        .filter(|(path, text)| holds(test_region(path, text)))
        .map(|(path, _)| (*path).to_owned())
        .collect();
    found.extend(snapshots.iter().filter(|(_, text)| holds(text)).map(|(path, _)| (*path).to_owned()));
    found
}

/// `verify` の nextest 行の filter 語（読みは歯の置き場と**同じ 1 本**[`nextest_filter`]・nextest 形でない行は落ちる）。
fn filters_of<'v>(verify: &'v [String], core_crate: &'v str) -> Vec<&'v str> {
    verify.iter().filter_map(|line| nextest_filter(line, core_crate).map(|(_, filter, _)| filter)).collect()
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

/// nextest 行の scope（§28・閉じた 3 値・宣言順 = 旗なし / `--lib` / `--test <name>`）＝その行が走らせる target。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope<'l> {
    /// 旗なし＝その crate の全 file。読めない旗（[`UNREAD_TARGET_FLAGS`]）と旗が 2 つ以上の行もここへ倒す（fail-closed）。
    Crate,
    /// `--lib`＝`crates/<crate>/src/` 配下。
    Lib,
    /// `--test <name>`＝`crates/<crate>/tests/<name>.rs` とその配下 `tests/<name>/`。
    Test(&'l str),
}

/// nextest の行から (crate, filter 語, scope) を読む。書き出しが `cargo nextest run` でない行・filter 語（`-` で
/// 始まらない末尾の語）の無い行は `None`。crate は `-p` / `--package` の次の語・無ければ core の crate。scope の旗
/// （`--lib` / `--test <name>`）が丁度 1 つで読めない旗が無い行だけ狭く読み、他は [`Scope::Crate`]。
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
        } else if UNREAD_TARGET_FLAGS.contains(&word) {
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
    use super::{check_drift, check_teeth_cover, declared_teeth, derive_write_set, weighted_lines, Base, ClosureError, Fields, Source};
    use super::{prose_closure, Prose};
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
            let fields = Fields { touches: &[], surfaces: &[], verify: &verify, creates: &[], tests: &[], also: &[] };
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
        let fields = Fields { touches: &[], surfaces: &[], verify: &verify, creates: &[], tests: &[], also: &[] };
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

    // flip-check: s2-07l.429

    /// 散文の閉包の fixture（§27・行 aa）: [`derive_base`] の歯に、判定行 token を歯の区間に持つ file（`tests/` 配下と
    /// src の `#[cfg(test)]` の区間）と、同じ字面を区間の**外**に持つ file を足す。
    fn prose_sources() -> Vec<Source> {
        let (mut sources, _) = derive_base();
        sources.push(source("crates/toy/tests/pin.rs", "#[test]\nfn pin_case() {\n    assert!(line.contains(\"mode=fast\"));\n}\n"));
        sources.push(source(
            "crates/toy/src/inline.rs",
            "pub fn f() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn pin_inline() {\n        assert!(line.contains(\"mode=fast\"));\n    }\n}\n",
        ));
        sources.push(source("crates/toy/src/outside.rs", "pub fn shown() -> &'static str {\n    \"mode=fast\"\n}\n"));
        sources
    }

    /// 判定行 token を持つ外形 snapshot（`.snap` は全文が pin の探索域）。
    fn prose_snapshots() -> Vec<Source> {
        vec![source(PROSE_SNAP, "---\n---\nrun=r-1 mode=fast\n")]
    }

    /// 外形 snapshot の path（pin の期待値が名指す）。
    const PROSE_SNAP: &str = "crates/toy/tests/e2e/snapshots/e2e__pipe__form.snap";

    /// 散文の閉包を求める（`texts` = `goal` / `done` の本文・読める fixture は `Ok`）。
    fn prose(texts: &[&str], verify: &[&str]) -> Prose {
        let (sources, snapshots) = (prose_sources(), prose_snapshots());
        let lines = strings(verify);
        let base = Base { sources: &sources, snapshots: &snapshots, tracked: &[], core_crate: "toy" };
        prose_closure(texts, &lines, &base).unwrap_or_else(|error| panic!("読める fixture は散文の閉包を返す: {error:?}"))
    }

    /// (a) 歯の名指しは**識別子形で base の `#[test]` の fn 名に等しいもの**だけ: `derive_ok` / `derive_ok()` / `derive_ok(`
    /// は同じ 1 本で、大文字始まり・型の path 形・`-` を含む形・`#[test]` でない fn・base に無い名は名指しと読まない
    /// （下界・§27 の限界）。`goal` と `done` の両方から拾い、名は辞書順・一意で file は宣言する歯の file。
    #[test]
    fn prose_closure_reads_identifier_forms_only() {
        let line = "cargo nextest run -p toy derive_";
        for text in ["`derive_ok`", "`derive_ok()`", "`derive_ok(`"] {
            let found = prose(&[text, ""], &[line]);
            assert_eq!(found.teeth, ["derive_ok"], "{text}");
            assert_eq!(found.files, set(&["crates/toy/tests/e2e.rs"]), "{text}");
        }
        let plain = prose(&["`Derive_ok` `Tint::derive_ok` `derive-ok` `derive_outside` `derive_new`", ""], &[line]);
        assert!(plain.teeth.is_empty() && plain.files.is_empty(), "識別子形で base の歯の名でなければ読まない: {plain:?}");
        let both = prose(&["`derive_ok`", "`derive_in_src` と `other_case` と `derive_ok`"], &[line]);
        assert_eq!(both.teeth, ["derive_in_src", "derive_ok", "other_case"], "辞書順・一意");
        assert_eq!(both.files, set(&["crates/toy/src/other.rs", "crates/toy/tests/e2e.rs", "crates/toy/tests/helper.rs"]));
    }

    /// (b) 判定行 token の literal は value が**具体**なら語の全体・placeholder（`<…>` / `a|b`）と空なら `<key>=` まで:
    /// `mode=fast` は fixture の 3 file を pin し、具体で base に無い `mode=slow` は 0 file（判定しない）・`mode=<a|b>` は
    /// `mode=` までなので同じ 3 file を pin する。key の形でない語は token と読まない。
    #[test]
    fn prose_closure_token_literal_is_whole_when_concrete_and_key_when_placeholder() {
        let pinned = set(&["crates/toy/src/inline.rs", PROSE_SNAP, "crates/toy/tests/pin.rs"]);
        assert_eq!(prose(&["", "`mode=fast`"], &[]).pins, pinned, "具体な value は語の全体で当てる");
        assert!(prose(&["", "`mode=slow`"], &[]).pins.is_empty(), "具体で base に無い語は 0 file");
        assert_eq!(prose(&["", "`mode=<a|b>`"], &[]).pins, pinned, "placeholder は mode= までを literal にする");
        assert_eq!(prose(&["", "`mode=`"], &[]).pins, pinned, "空の value も key まで");
        for text in ["`Mode=fast`", "`=fast`", "`mode`"] {
            assert!(prose(&["", text], &[]).pins.is_empty(), "{text} は判定行 token でない");
        }
    }

    /// (b) の探索域は**歯の区間と外形 snapshot だけ**: src の `#[cfg(test)]` の区間の中の字面は数え、同じ字面を区間の外に
    /// 持つ file は数えない。(a) の名指しも同じ区間で読む（区間の外の `fn` は歯でない）。
    #[test]
    fn prose_closure_searches_test_regions_and_snapshots_only() {
        let found = prose(&["", "`mode=fast` と `derive_outside`"], &["cargo nextest run -p toy derive_"]);
        assert!(found.pins.contains("crates/toy/src/inline.rs"), "src の test 区間は数える: {found:?}");
        assert!(found.pins.contains(PROSE_SNAP), "外形 snapshot も数える: {found:?}");
        assert!(!found.pins.contains("crates/toy/src/outside.rs"), "区間の外の同じ字面は数えない: {found:?}");
        assert!(found.teeth.is_empty(), "区間の外の fn は歯の名指しでない: {found:?}");
    }

    /// (a1) 被覆は nextest の positional filter と**同じ「含む」**: `derive_` の行は `derive_ok` を被覆し `other_case` を
    /// 被覆しない（被覆されない名は全部・辞書順）。2 行のどちらかが被覆すれば足り、nextest 形でない行と filter 語の無い行は
    /// 被覆を持たない。
    #[test]
    fn prose_closure_uncovered_uses_nextest_contains() {
        let named = ["", "`derive_ok` と `other_case`"];
        let derive_line = "cargo nextest run -p toy --no-tests=fail derive_";
        let found = prose(&named, &[derive_line]);
        assert_eq!(found.teeth, ["derive_ok", "other_case"], "名指しは 2 本");
        assert_eq!(found.uncovered, ["other_case"], "filter 語 derive_ を含まない名だけ");
        let covered = prose(&named, &[derive_line, "cargo nextest run -p toy other_"]);
        assert!(covered.uncovered.is_empty(), "2 行のどちらかで足りる: {covered:?}");
        let unread = prose(&named, &["git status", "cargo nextest run -p toy"]);
        assert_eq!(unread.uncovered, ["derive_ok", "other_case"], "nextest 形でない行と filter 語の無い行は被覆を持たない");
        assert_eq!(prose(&named, &["cargo nextest run -p toy derive_ok"]).uncovered, ["other_case"], "filter 語は名の全体でもよい");
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
