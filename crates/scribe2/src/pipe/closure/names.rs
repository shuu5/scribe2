//! 名指しの実在（設計 docs/design/contract-source.md §3「名指しの実在」・§26・SRS FR48）。
//!
//! 型の閉包の 4 形（[`super::sees`] と親 module `closure.rs`）と外形 pin（[`super::surface_closure`]）に依らない閉じた
//! 群で、契約の散文（`title` / `done` / § の本文）の backtick の中身のうち **path 形 / 型の path 形 / fn 形**だけを名指しと
//! 読み、base に解けないものを [`unresolved_names`] が全件返す。呼び手は `pipe::table::check` の 1 か所で、親の
//! `pub use` を通るので import は不変である。
//!
//! 親の私有 item（[`super::texts_of`] / [`super::heads`] / [`super::is_ident`] / [`super::is_ident_char`] と const 群）は
//! 子孫として `super::` でそのまま引く（可視性を上げない）。逆向きに、親の 4 形の判定が使う [`holds_word`]（審査の外の
//! 材料も引くので `pub(crate)`）/ [`declares_fn`] と、`closure::derive` が引く [`backticked`] / [`closed_type`] は `pub(super)`（＝`pipe::closure` の中だけ）に留める。
//!
//! 審査の材料の名指し（設計 §51 形 2・行 bc）は同じ読み手の 2 本目の口 [`mentioned_names`] で、backtick の中身の先頭の
//! token と、backtick の外の識別子の形の語と、`/` か拡張子を持つ path 形の連なりを拾う（[`unresolved_names`] の判定は不変）。
//! 本文の塊を渡す名指しの 3 形（§56・行 bi）は 3 本目の口 [`named_items`] で、impl は [`impl_line`] で読む。

use super::{declares_type, heads, in_module, is_ident, is_ident_char, texts_of, touched, ClosureError, Source};
use super::{IMPL_HEAD, KEYWORDS, PATH_CHARS, RS};
use std::collections::BTreeSet;

/// 名指しの実在（§3）: `texts` の各 (在り処, 本文) の backtick の中身のうち **path 形 / 型の path 形 / fn 形**だけを
/// 名指しと読み、base に解けないものを (名, 在り処) で**全件**返す（書かれていた順）。
///
/// (1) path 形（英数字と `_ . / -` だけ・拡張子 `.rs`）は `tracked` の path と等しいか `/` 区切りの末尾一致、または
/// その行の write-set の `+`（新規 file）/ `~`（着地で消える file・§24 (3)）の項目（接頭辞を剥がした path）と同じ
/// 照合で解ける（`~` の除外が無いと、着地で消した file を名指す本文が着地の後に全部赤くなる）。(2) 型の path 形（`::` で結んだ識別子の
/// 列）は末尾 2 節の「型」と「項目」を [`resolves_type`] の 2 経路（字面 / impl）で解く（§26）。**`touches` に宣言した
/// 型の variant は名指しと読まない**
/// （未来の variant は `touches` が説明する）。(3) fn 形（識別子 + `(`〔`)` は任意〕）は `fn 識別子` の宣言が在れば
/// 解ける。(2) と (3) は中身の**先頭の token**（[`head_of`]・§25）で読む＝field 付き variant の literal と引数付きの呼出しも
/// 名指しである。一致しない字面（大文字始まりの tuple variant の構築・予約語・末尾 `::` の module path・glob・属性・散文）は
/// 名指しではない。別名・generic は下界の外。
pub fn unresolved_names(
    texts: &[(String, String)],
    touches: &[String],
    write_set: &[String],
    tracked: &[String],
    sources: &[Source],
) -> Result<Vec<(String, String)>, ClosureError> {
    let bodies = texts_of(sources)?;
    // base に無くてよい項目（`+` の新規 file と `~` の着地で消える file）を接頭辞を剥がして path 形の解に足す。
    let off_base = write_set.iter().filter_map(|item| item.strip_prefix(['+', '~']));
    let paths: Vec<&str> = tracked.iter().map(String::as_str).chain(off_base).collect();
    let touched: Vec<&str> = touches.iter().filter_map(|raw| raw.rsplit("::").next()).collect();
    let mut found = Vec::new();
    for (at, text) in texts {
        for name in backticked(text) {
            if resolved(name, &touched, &paths, &bodies) == Some(false) {
                found.push((name.to_owned(), at.clone()));
            }
        }
    }
    Ok(found)
}

/// 約束の行の `symbols` の各名が base に在るか（設計 §33 項 5・[`unresolved_names`] と**同じ読み手**・名は backtick の
/// 中身と同じ字面で渡す）。3 形のどれでもない字面は `None`（名指しでない＝測れない・下界）。
pub fn symbols_in_base(names: &[&str], tracked: &[String], sources: &[Source]) -> Result<Vec<Option<bool>>, ClosureError> {
    let bodies = texts_of(sources)?;
    let paths: Vec<&str> = tracked.iter().map(String::as_str).collect();
    Ok(names.iter().map(|name| resolved(name, &[], &paths, &bodies)).collect())
}

/// 審査の材料の名指し（§51 形 2・[`mentioned_names`] が返す）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mentioned {
    /// 候補の名のうち本文が名指すもの（辞書順・重複なし）。
    pub names: Vec<String>,
    /// path 形の連なりが解けた tracked の file（辞書順・重複なし・同じ末尾の file は全部）。
    pub files: Vec<String>,
    /// file に解けず tracked の dir に解けた連なりの dir（末尾 `/` 無し・辞書順・重複なし）。
    pub dirs: Vec<String>,
}

/// 審査の材料の名の照合の口（§51 形 2・名指しの読み手の 1 本）。候補の名は (a) backtick の中身の先頭の token
/// （[`head_of`]）の `::` の節のどれかに等しいか、(b) backtick の外に語の境界（[`holds_word`]）で現れ字面が識別子の形
/// （[`shaped`]）のものだけを拾う。path 形の文字（[`PATH_CHARS`]）の連なりは `/` か拡張子を持つものだけを
/// [`path_matches`] で tracked の file（拡張子を問わない）に、file に解けなければ tracked の dir に解く。backtick の外の
/// 素の 1 語は、候補に在っても tracked の dir と同じ名でも読まない（数の閾値を持たない構造の規則）。
pub fn mentioned_names(texts: &[&str], candidates: &[&str], tracked: &[String]) -> Mentioned {
    let mut segments: BTreeSet<&str> = BTreeSet::new();
    let mut outside = String::new();
    for text in texts {
        segments.extend(backticked(text).into_iter().flat_map(|name| head_of(name).0.split("::")));
        outside.push_str(&unticked(text));
        outside.push('\n');
    }
    let names: BTreeSet<String> = candidates
        .iter()
        .filter(|name| segments.contains(**name) || (shaped(name) && holds_word(&outside, name)))
        .map(|name| (*name).to_owned())
        .collect();
    let (mut files, mut dirs) = (BTreeSet::new(), BTreeSet::new());
    for run in texts.iter().flat_map(|text| path_runs(text)) {
        let hit: Vec<&String> = tracked.iter().filter(|path| path_matches(path, run)).collect();
        if hit.is_empty() {
            dirs.extend(tracked.iter().filter_map(|path| dir_of(path, run)).map(str::to_owned));
        }
        files.extend(hit.into_iter().cloned());
    }
    Mentioned { names: names.into_iter().collect(), files: files.into_iter().collect(), dirs: dirs.into_iter().collect() }
}

/// 本文の塊を求める名指し 1 つ（§56 行 bi・[`named_items`] が返す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    /// (P)(F) は探し先に解けた path・(N) は `None`（探し先の全部を探す）。
    pub path: Option<String>,
    /// 宣言の語（(F) の語・(N) は `fn`・(P) は `None`＝語を問わない）。
    pub kind: Option<String>,
    /// 名。
    pub name: String,
}

/// 本文の塊を渡す名指しの 3 形の口（§56 行 bi・審査の外の材料の (j)）: (P) `<path>.rs#<名>`（backtick の内外を問わない）・
/// (F) `<path>.rs` の直後（backtick・空白・`の` を飛ばす）の `<語> <名>`・(N) backtick の中身の fn 形（[`form_of`]）。(P)(F) の
/// path は探し先 `paths` に [`path_matches`] で解いた最初の path（解けない名指しは返さない）。並びは本文の順・1 本の中は現れた順。
pub fn named_items(texts: &[&str], paths: &[String]) -> Vec<Named> {
    let spots = |text: &str| -> Vec<(usize, Named)> {
        let offset = |name: &str| name.as_ptr().addr().saturating_sub(text.as_ptr().addr());
        let fns = backticked(text).into_iter().filter_map(|name| match form_of(name, &[]) {
            Form::Fn(ident) => Some((offset(name), Named { path: None, kind: Some("fn".to_owned()), name: ident })),
            _ => None,
        });
        let mut hits: Vec<(usize, Named)> = text.match_indices(RS).filter_map(|(at, _)| Some((at, rs_named(text, at, paths)?))).chain(fns).collect();
        hits.sort_by_key(|(at, _)| *at);
        hits
    };
    texts.iter().flat_map(|text| spots(text)).map(|(_, named)| named).collect()
}

/// `at` の `.rs` で終わる path の連なりの (P) か (F) の名指し（path が探し先に解けなければ `None`・(F) の語は宣言の語か impl）。
fn rs_named(text: &str, at: usize, paths: &[String]) -> Option<Named> {
    let path_char = |found: char| found.is_ascii_alphanumeric() || PATH_CHARS.contains(&found);
    let (head, rest) = text.split_at_checked(at.saturating_add(RS.len()))?;
    let run = head.get(head.get(..at)?.trim_end_matches(path_char).len()..)?.trim_start_matches('-');
    let path = paths.iter().find(|path| path_matches(path, run))?.clone();
    let ident = |tail: &str| Some(tail.split(|found: char| !is_ident_char(found)).next()?.to_owned()).filter(|name| is_ident(name));
    if let Some(tail) = rest.strip_prefix('#') {
        return Some(Named { path: Some(path), kind: None, name: ident(tail)? });
    }
    let (word, tail) = rest.trim_start_matches(|found: char| found == '`' || found == 'の' || found.is_whitespace()).split_once(char::is_whitespace)?;
    let name = ident(tail.trim_start()).filter(|_| !rest.starts_with(path_char) && ["fn", "struct", "enum", "union", "trait", "type", "const", "static", "mod", IMPL_HEAD].contains(&word))?;
    Some(Named { path: Some(path), kind: Some(word.to_owned()), name })
}

/// 本文の backtick の外（対になった backtick の中身を空白に替えた字面・対にならない末尾の backtick の後ろは外）。
fn unticked(text: &str) -> String {
    let lines = text.lines().map(|line| {
        let pieces: Vec<&str> = line.split('`').collect();
        let paired = if pieces.len().is_multiple_of(2) { pieces.len().saturating_sub(1) } else { pieces.len() };
        let kept = pieces.iter().enumerate().filter(|(at, _)| at.is_multiple_of(2) || *at >= paired).map(|(_, piece)| *piece);
        kept.collect::<Vec<&str>>().join(" ")
    });
    lines.collect::<Vec<String>>().join("\n")
}

/// 字面が識別子の形か（語の中に `_` を持つか、大文字で始まる山〔先頭か小文字・数字の直後の大文字〕が 2 つ以上）。
fn shaped(name: &str) -> bool {
    let mut humps = 0_usize;
    let mut prev: Option<char> = None;
    for found in name.chars() {
        if found.is_ascii_uppercase() && prev.is_none_or(|before| before.is_ascii_lowercase() || before.is_ascii_digit()) {
            humps = humps.saturating_add(1);
        }
        prev = Some(found);
    }
    name.contains('_') || humps >= 2
}

/// 本文の path 形の連なりのうち `/` か拡張子（末尾の `.` と英字）を持つもの（前の `-` と後ろの `.` `-` は文の区切り
/// として剥がし、末尾の `/` は照合の前に剥がす）。
fn path_runs(text: &str) -> Vec<&str> {
    text.split(|found: char| !(found.is_ascii_alphanumeric() || PATH_CHARS.contains(&found)))
        .map(|run| run.trim_start_matches('-').trim_end_matches(['.', '-']))
        .filter(|run| run.contains('/') || has_extension(run))
        .map(|run| run.trim_end_matches('/'))
        .filter(|run| !run.is_empty())
        .collect()
}

/// 最後の段が拡張子（末尾の `.` の後ろが 1 字以上の英字だけ）を持つか。
fn has_extension(run: &str) -> bool {
    run.rsplit('/')
        .next()
        .and_then(|last| last.rsplit_once('.'))
        .is_some_and(|(_, ext)| !ext.is_empty() && ext.chars().all(|found| found.is_ascii_alphabetic()))
}

/// tracked の path の `/` 区切りの頭のうち、連なりに [`path_matches`] で解ける最初の dir。
fn dir_of<'p>(path: &'p str, run: &str) -> Option<&'p str> {
    path.match_indices('/').filter_map(|(at, _)| path.get(..at)).find(|head| path_matches(head, run))
}

/// 名指し 1 つが base に解けるか（path 形は `paths` の path・型の path 形は [`resolves_type`]・fn 形は宣言）。名指しの
/// 形でない字面（`touched` の型の variant を含む）は `None`。`crate::<module>::<Type>` の形（§34）は閉じた型の読み手
/// [`closed_type`]（`closure()` の `touches` と同じ 1 本）でも解き、従来の末尾 2 節の読みとの OR をとる（解けていた名を
/// 解けなくしない）。
fn resolved(name: &str, touched: &[&str], paths: &[&str], bodies: &[(&str, &str)]) -> Option<bool> {
    match form_of(name, touched) {
        Form::Path => Some(paths.iter().any(|path| path_matches(path, name))),
        Form::Type { ty, item } => Some(closed_type(head_of(name).0, bodies) || resolves_type(bodies, &ty, &item)),
        Form::Fn(ident) => Some(bodies.iter().any(|(_, body)| declares_fn(body, &ident))),
        Form::Prose => None,
    }
}

/// `symbol` が base で閉じた型か（`crate::module::Type` の型形で、module の file が `enum` / `struct` を宣言する＝
/// [`super::closure`] が読む型）。fn 形・`crate::` の無い字面・宣言の無い名は閉じた型でない。約束の行の導出
/// （`touches`・`closure::derive`）と受付の名指し（[`resolved`]）が同じ 1 本で読む（§34）。
pub(super) fn closed_type(symbol: &str, texts: &[(&str, &str)]) -> bool {
    touched(symbol).is_some_and(|found| {
        !found.fn_form && texts.iter().any(|(path, text)| declares_type(text, found.name) && in_module(path, found.module))
    })
}

/// backtick の中身の形。
enum Form {
    /// path 形。
    Path,
    /// 型の path 形（末尾 2 節の「型」と「項目」を別に持つ＝2 経路の解決に両方が要る・§26）。
    Type {
        /// 末尾から 2 番目の節（型の名）。
        ty: String,
        /// 末尾の節（variant / method / 関連 fn の名）。
        item: String,
    },
    /// fn 形（識別子）。
    Fn(String),
    /// 名指しではない字面。
    Prose,
}

/// 型の path 形が base に解けるか（§26 の 2 経路の OR）:
///
/// (a) **字面**＝「型::項目」が語の境界で現れる file が在る（variant と `Self::` を持たない呼び出しの形）。
/// (b) **impl 経路**＝「型」を語に持つ impl 行（[`impls_type`]）と `fn <項目>` の宣言（[`declares_fn`]）を
///     **同じ file** が持つ（method / 関連 fn の呼び手は「値.項目(」「Self::項目」で字面が現物に無い）。
///
/// **同じ file** に限るのは、別 module の同名の fn で解けてしまわないためである（§3「閉包の同名衝突」と同じ向き）。
fn resolves_type(bodies: &[(&str, &str)], ty: &str, item: &str) -> bool {
    let word = format!("{ty}::{item}");
    bodies.iter().any(|&(_, body)| holds_word(body, &word) || (impls_type(body, ty) && declares_fn(body, item)))
}

/// 本文が `ty` を語に持つ impl 行を持つか（行頭〔`trim_start` 後〕が `impl`・素の impl `impl Ty {`・generic impl
/// `impl<T> Ty<T> {`・trait impl `impl Tr for Ty {` を同じ照合で拾う）。
fn impls_type(body: &str, ty: &str) -> bool {
    body.lines().any(|line| impl_line(line, ty))
}

/// 1 行が `ty` を語に持つ impl 行か（[`impls_type`] の照合の行 1 本・審査の外の材料の (j) の impl も同じ 1 本で読む）。
pub fn impl_line(line: &str, ty: &str) -> bool {
    line.trim_start().strip_prefix(IMPL_HEAD).is_some_and(|rest| !rest.starts_with(is_ident_char)) && holds_word(line, ty)
}

/// backtick の中身を 3 形に分ける（`touched` の型の variant は散文扱い）。path 形は中身全体で、型の path 形と
/// 引数付きの fn 形は先頭の token（[`head_of`]）で読む（§25）。引数付きの fn 形は小文字始まりの識別子で予約語でない
/// 周だけ（`Some(…)` は tuple variant の構築・`pub(crate)` は可視性＝散文）。
fn form_of(name: &str, touched: &[&str]) -> Form {
    let stem = name.rsplit('/').next().unwrap_or(name).strip_suffix(RS);
    if name.chars().all(|found| found.is_ascii_alphanumeric() || PATH_CHARS.contains(&found)) && stem.is_some_and(|stem| !stem.is_empty()) {
        return Form::Path;
    }
    let (head, rest) = head_of(name);
    let segments: Vec<&str> = head.split("::").collect();
    if let Some((item, head)) = segments.split_last().filter(|_| segments.iter().all(|segment| is_ident(segment))) {
        if let Some(ty) = head.last() {
            return if touched.contains(ty) {
                Form::Prose
            } else {
                Form::Type { ty: (*ty).to_owned(), item: (*item).to_owned() }
            };
        }
    }
    let ident = name.strip_suffix("()").or_else(|| name.strip_suffix('('));
    match ident {
        Some(ident) if is_ident(ident) => Form::Fn(ident.to_owned()),
        _ if rest.starts_with('(')
            && is_ident(head)
            && head.starts_with(|found: char| found.is_ascii_lowercase())
            && !KEYWORDS.contains(&head) =>
        {
            Form::Fn(head.to_owned())
        }
        _ => Form::Prose,
    }
}

/// backtick の中身の先頭の token（最初の `{` / `(` / 空白の手前まで・末尾の `::` は落とさない）と残り（§25）。
fn head_of(name: &str) -> (&str, &str) {
    name.split_at(name.find(|found: char| found == '{' || found == '(' || found.is_whitespace()).unwrap_or(name.len()))
}

/// 1 本の本文の backtick の中身（対になった backtick だけ・空は除く）。
pub(super) fn backticked(text: &str) -> Vec<&str> {
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

/// 本文が `word`（`型::項目`）を語の境界で持つか（前が識別子の文字でなく・後ろも識別子の文字でない）。審査の外の材料
/// （§51 形 3 (c) の data file の鍵）も同じ 1 本で測るので `pub(crate)`。
pub(crate) fn holds_word(body: &str, word: &str) -> bool {
    heads(body, word)
        .into_iter()
        .any(|at| !body.get(at.saturating_add(word.len())..).unwrap_or_default().starts_with(is_ident_char))
}

/// 本文が `fn ident` の宣言を持つか。
pub(super) fn declares_fn(body: &str, ident: &str) -> bool {
    holds_word(body, &format!("fn {ident}"))
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.458

    use super::super::tests::source;
    use super::{unresolved_names, ClosureError, Source};

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

    /// 名指しの 3 形（path / 型の path / fn）を解き、解けないものを在り処付きで全件返す。`+` 宣言の新規 file と
    /// `~` 宣言の消える file は解け（write-set に無い同名は解けない）、`touches` の型の variant（field 付きの literal を
    /// 含む）と一致しない字面（struct literal・glob・属性・散文・単独の語）は名指しと読まない。
    #[test]
    fn closure_names_resolve_the_three_forms_and_name_every_unresolved_one() {
        let (tracked, sources) = name_fixture();
        let texts = |lines: &[(&str, &str)]| -> Vec<(String, String)> {
            lines.iter().map(|(at, text)| ((*at).to_owned(), (*text).to_owned())).collect()
        };
        let resolved = texts(&[
            ("title", "`pipe/closure.rs` と `closure.rs` と `crate::polarity::Guard` の `Guard::Intake`"),
            ("done", "`overlaps(` と `overlaps()` が在る・`Refuse::Nope` は touches の型・`pipe/review.rs` は write-set の + 宣言・`pipe/old.rs` は ~ 宣言"),
            ("section 3 line 9", "`Refuse::WriteSetIncomplete { run, missing }`・`tests/e2e/*.rs`・`#[cfg(test)]`・`Type {`・`Type::`・`touches`・`.rs`・`NAME.len()`・`use … as`"),
        ]);
        let write_set = [
            "crates/toy/src/pipe/closure.rs".to_owned(),
            "+crates/toy/src/pipe/review.rs".to_owned(),
            // 着地で消える file（§24 (3)）は base（tracked）に無いが、`+` と同じ除外で解ける。
            "~crates/toy/src/pipe/old.rs".to_owned(),
        ];
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

    /// 名指しを持つ (在り処, 本文) の列。
    fn named_texts(lines: &[(&str, &str)]) -> Vec<(String, String)> {
        lines.iter().map(|(at, text)| ((*at).to_owned(), (*text).to_owned())).collect()
    }

    /// impl 経路の fixture: 素の impl（`Report` の method `violation`）・generic impl（`Wide` の `width`）・
    /// trait impl（`Shown` の `show`）を各 1 file で持つ。どの file も「型::項目」の字面は持たない。
    fn impl_fixture() -> Vec<Source> {
        vec![
            source("crates/toy/src/report.rs", "pub struct Report {\n    pub at: u8,\n}\n\nimpl Report {\n    pub fn violation(&self) -> u8 {\n        self.at\n    }\n}\n"),
            source("crates/toy/src/wide.rs", "pub struct Wide<T> {\n    pub inner: T,\n}\n\nimpl<T: Copy> Wide<T> {\n    pub fn width(&self) -> usize {\n        0\n    }\n}\n"),
            source("crates/toy/src/shown.rs", "pub struct Shown;\n\nimpl Render for Shown {\n    fn show(&self) -> String {\n        String::new()\n    }\n}\n"),
        ]
    }

    /// impl 経路（§26 の (b)）: 型の path 形の「項目」が method / 関連 fn の周は、「型」を語に持つ impl 行と
    /// `fn <項目>` の宣言を**同じ file** が持てば解ける（素の impl・generic impl・trait impl の 3 形とも）。呼び手は
    /// 「値.項目(」「Self::項目」なので (a) の字面「型::項目」は現物に無い＝base では 3 形とも name-unresolved に倒れる。
    #[test]
    fn closure_names_impl_method_resolves_through_the_impl_block() {
        let sources = impl_fixture();
        let literal = |word: &str| sources.iter().any(|found| found.body.as_deref().unwrap_or_default().contains(word));
        for word in ["Report::violation", "Wide::width", "Shown::show"] {
            assert!(!literal(word), "fixture は {word} の字面を持たない（(a) の経路では解けない）");
        }
        let texts = named_texts(&[
            ("title", "`Report::violation` を直す"),
            ("done", "`Wide::width` と `Shown::show` が通る"),
        ]);
        assert_eq!(unresolved_names(&texts, &[], &[], &[], &sources), Ok(Vec::new()), "3 形とも impl 経路で解ける");
    }

    /// impl 経路は**同じ file** の中だけで結ぶ（impl 行と `fn` が別 file の周は解けない）。fn の無い項目（variant
    /// `Guard::Rules`・impl 行は在るが `fn Rules` が無い）も解けず、variant `Tint::Warm` は (a) の字面で解けたまま。
    #[test]
    fn closure_names_impl_route_stays_in_the_same_file_and_keeps_missing_items_unresolved() {
        let sources = vec![
            // impl 行は持つが `fn violation` は別 file（結ばない）。
            source("crates/toy/src/report.rs", "pub struct Report;\n\nimpl Report {\n    pub fn other(&self) -> u8 {\n        0\n    }\n}\n"),
            source("crates/toy/src/free.rs", "pub fn violation() -> u8 {\n    1\n}\n"),
            // impl 行は在るが項目は fn でない（`fn rules` は語が違う）。`impl` を literal の頭に置くのは、この file
            // 自身が閉包の母集団だからである（`\n` の escape の後ろの `impl Guard {` は第 1 形の literal 構築に読まれ、
            // 現物の `crate::polarity::Guard` の閉包がこの file へ偽に広がる）。
            source("crates/toy/src/polarity.rs", "impl Guard {\n    pub fn rules(&self) -> u8 {\n        2\n    }\n}\n\npub enum Guard {\n    Rules,\n}\n"),
            // variant の字面（(a) の経路）。
            source("crates/toy/src/tint.rs", "pub enum Tint {\n    Warm,\n}\n\npub const TINTS: &[Tint] = &[Tint::Warm];\n"),
        ];
        let texts = named_texts(&[
            ("done", "`Report::violation` は impl 行の無い file の fn・`Guard::Rules` は fn でない"),
            ("section 3 line 9", "`Tint::Warm` は字面で解ける"),
        ]);
        let want: Vec<(String, String)> = [("Report::violation", "done"), ("Guard::Rules", "done")]
            .iter()
            .map(|(name, at)| ((*name).to_owned(), (*at).to_owned()))
            .collect();
        assert_eq!(unresolved_names(&texts, &[], &[], &[], &sources), Ok(want), "解けない名を書かれた順・在り処付き");
    }

    /// 先頭の token の fixture（§25）: 既存の enum `Tint`（variant `Warm`）と既存 fn `parse_pointer`。`Tide` は無い。
    /// 型の名は現物の型と重ねない（この file 自身が閉包の母集団で、現物の型の literal を書くと行の閉包が広がる）。
    fn head_fixture() -> Vec<Source> {
        vec![
            source("crates/toy/src/tint.rs", "pub enum Tint {\n    Warm { at: u8 },\n}\n\nfn f() -> Tint {\n    Tint::Warm { at: 0 }\n}\n"),
            source("crates/toy/src/pipe/table/parse.rs", "pub fn parse_pointer(text: &str) -> bool {\n    text.is_empty()\n}\n"),
        ]
    }

    /// struct-like variant の literal（型::項目 に `{ 欄: 値 }` が続く字面）は先頭の token の型の path 形に読まれ、base に
    /// 無ければ name-unresolved・在れば解ける・`touches` の型なら従来どおり名指しでない（§25）。
    #[test]
    fn contract_name_form_struct_like_variant_literal_reads_the_type_path_head() {
        let sources = head_fixture();
        let texts = named_texts(&[("section 25 line 3", "`Tide::Ebb { run: 1 }` と `Tint::Warm { at: 0 }`")]);
        let want = vec![("Tide::Ebb { run: 1 }".to_owned(), "section 25 line 3".to_owned())];
        assert_eq!(unresolved_names(&texts, &[], &[], &[], &sources), Ok(want), "未 land の型の literal だけが name-unresolved");
        let touches = ["crate::tide::Tide".to_owned()];
        assert_eq!(unresolved_names(&texts, &touches, &[], &[], &sources), Ok(Vec::new()), "touches の型の variant は名指しでない");
        assert_eq!(super::symbols_in_base(&["Tide::Ebb(1)", "Tint::Warm(x)"], &[], &sources), Ok(vec![Some(false), Some(true)]));
    }

    /// 引数付きの呼出し形（識別子(引数)）は先頭の token の fn 形に読まれる: 既存 fn は解け、無い識別子は解けない（§25）。
    #[test]
    fn contract_name_form_call_with_arguments_reads_the_fn_head() {
        let sources = head_fixture();
        let names = ["parse_pointer(text)", "parse_pointer(&row.design)", "nope_pointer(text)", "parse_pointer("];
        assert_eq!(super::symbols_in_base(&names, &[], &sources), Ok(vec![Some(true), Some(true), Some(false), Some(true)]));
        let texts = named_texts(&[("done", "`parse_pointer(text)` が解け `nope_pointer(x, y)` は解けない")]);
        let want = vec![("nope_pointer(x, y)".to_owned(), "done".to_owned())];
        assert_eq!(unresolved_names(&texts, &[], &[], &[], &sources), Ok(want));
    }

    /// 先頭の token がどの形にも合わない字面は従来どおり散文: 大文字始まりの tuple variant の構築・予約語（可視性）・
    /// 末尾 `::` の module path・glob の use・属性・method 呼出し・空白で続く散文（§25）。
    #[test]
    fn contract_name_form_tuple_variant_keyword_module_path_glob_and_attribute_stay_prose() {
        let sources = head_fixture();
        let names = [
            "Some(x)",
            "Err(e)",
            "Gated(FAIL)",
            "pub(crate)",
            "pub(super) fn x",
            "if (a)",
            "seat::account::",
            "seat::account:: の下",
            "use crate::tint::*;",
            "crate::tint::*",
            "#[cfg(test)]",
            "NAME.len()",
            "row.find(x)",
            "cargo nextest run",
            "Type {",
        ];
        let want: Vec<Option<bool>> = names.iter().map(|_| None).collect();
        assert_eq!(super::symbols_in_base(&names, &[], &sources), Ok(want));
    }

    /// 文字列の列。
    fn owned(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    /// §51 形 2 の名: backtick の中身の先頭の token の節（`overlaps(` の `overlaps`・`crate::pipe::review::Material` の各節）と、
    /// backtick の外の `_` を持つ名・山 2 つの CamelCase の名を拾い、外の素の 1 語（`shape` / `Shape` / `parse`）は候補に在っても
    /// 拾わない。長い名の途中に在る短い名（`ZqShapeKindWide` の `ShapeKind`・`zq_wide_max` の `zq_wide`）も拾わない。同じ素の語を
    /// backtick に入れると拾う（母集団 = 候補 10 個を同じ assert で数える）。
    #[test]
    fn closure_names_mentioned_picks_backticked_heads_and_shaped_words_but_not_bare_words() {
        let candidates = ["ZqShape", "zq_width", "Shape", "shape", "overlaps", "parse", "Material", "review", "ShapeKind", "zq_wide"];
        let text = "節は ZqShape と zq_width を読む。shape と Shape と parse は素の語・`overlaps(` と `crate::pipe::review::Material` を名指す・ZqShapeKindWide と zq_wide_max";
        let found = super::mentioned_names(&[text], &candidates, &[]);
        assert_eq!(candidates.len(), 10, "母集団");
        assert_eq!(found.names, owned(&["Material", "ZqShape", "overlaps", "review", "zq_width"]), "{found:?}");
        let ticked = super::mentioned_names(&["`shape` と `Shape::Dot { x: 1 }` と parse"], &candidates, &[]);
        assert_eq!(ticked.names, owned(&["Shape", "shape"]), "backtick の中なら素の語も拾う: {ticked:?}");
        assert!(found.files.is_empty() && found.dirs.is_empty(), "path は無い: {found:?}");
    }

    /// §51 形 2 の path: `/` か拡張子を持つ連なりだけが解け（素の 1 語 `pipe` / `fixtures` は tracked の dir と同じ名でも解けない）、
    /// 等しいか `/` 区切りの末尾一致で解け、同じ末尾が 2 file に在れば両方を返し、`.rs` でない path（json・md）も解け、file に
    /// 解けない連なりは dir に解ける。
    #[test]
    fn closure_names_mentioned_resolves_path_runs_with_a_slash_or_an_extension_only() {
        let tracked = owned(&[
            "crates/a/src/pipe/review.rs",
            "crates/b/src/pipe/review.rs",
            "crates/a/fixtures/data.json",
            "crates/a/fixtures/deep/b.yaml",
            "docs/a.md",
        ]);
        let text = "pipe/review.rs と data.json と `fixtures/` と docs/a.md を読む。pipe と fixtures は素の語・none/x.json は無い。";
        let found = super::mentioned_names(&[text], &[], &tracked);
        assert_eq!(
            found.files,
            owned(&["crates/a/fixtures/data.json", "crates/a/src/pipe/review.rs", "crates/b/src/pipe/review.rs", "docs/a.md"]),
            "{found:?}"
        );
        assert_eq!(found.dirs, owned(&["crates/a/fixtures"]), "{found:?}");
        let bare = super::mentioned_names(&["pipe と fixtures と review"], &[], &tracked);
        assert_eq!((bare.files.len(), bare.dirs.len()), (0, 0), "素の 1 語は path にも dir にも解けない: {bare:?}");
    }
}
