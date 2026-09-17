//! 名指しの実在（設計 docs/design/contract-source.md §3「名指しの実在」・§26・SRS FR48）。
//!
//! 型の閉包の 4 形（[`super::sees`] と親 module `closure.rs`）と外形 pin（[`super::surface_closure`]）に依らない閉じた
//! 群で、契約の散文（`title` / `done` / § の本文）の backtick の中身のうち **path 形 / 型の path 形 / fn 形**だけを名指しと
//! 読み、base に解けないものを [`unresolved_names`] が全件返す。呼び手は `pipe::table::check` の 1 か所で、親の
//! `pub use` を通るので import は不変である。
//!
//! 親の私有 item（[`super::texts_of`] / [`super::heads`] / [`super::is_ident`] / [`super::is_ident_char`] と const 群）は
//! 子孫として `super::` でそのまま引く（可視性を上げない）。逆向きに、親の 4 形の判定が使う [`holds_word`] /
//! [`declares_fn`] と、`closure::derive` が引く [`backticked`] は `pub(super)`（＝`pipe::closure` の中だけ）に留める。

use super::{heads, is_ident, is_ident_char, texts_of, ClosureError, Source};
use super::{IMPL_HEAD, PATH_CHARS, RS};

/// 名指しの実在（§3）: `texts` の各 (在り処, 本文) の backtick の中身のうち **path 形 / 型の path 形 / fn 形**だけを
/// 名指しと読み、base に解けないものを (名, 在り処) で**全件**返す（書かれていた順）。
///
/// (1) path 形（英数字と `_ . / -` だけ・拡張子 `.rs`）は `tracked` の path と等しいか `/` 区切りの末尾一致、または
/// その行の write-set の `+` 項目（接頭辞を剥がした path）と同じ照合で解ける。(2) 型の path 形（`::` で結んだ識別子の
/// 列）は末尾 2 節の「型」と「項目」を [`resolves_type`] の 2 経路（字面 / impl）で解く（§26）。**`touches` に宣言した
/// 型の variant は名指しと読まない**
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
                Form::Type { ty, item } => resolves_type(&bodies, &ty, &item),
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
    body.lines()
        .filter(|line| line.trim_start().strip_prefix(IMPL_HEAD).is_some_and(|rest| !rest.starts_with(is_ident_char)))
        .any(|line| holds_word(line, ty))
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
        _ => Form::Prose,
    }
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

/// 本文が `word`（`型::項目`）を語の境界で持つか（前が識別子の文字でなく・後ろも識別子の文字でない）。
pub(super) fn holds_word(body: &str, word: &str) -> bool {
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
}
