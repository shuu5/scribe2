//! 入れ子の source の根（vessel 宣言の任意 key `crate-roots`・設計 docs/design/contract-source.md §62・SRS FR34 / FR48 /
//! NFR4）。
//!
//! 固定の根 `crates/` は常に在り、key はそれに**足す**根を並べる（置き換えない＝宣言で器自身の検査の範囲を狭められない）。
//! 各項目は repo 相対の dir で末尾が `/`。根の列と path から「根・crate の名・残り」を返す 1 関数（[`crate_of`]）が、
//! 検出線の面・受付の上限の余地の読み手の共通の入口である。読むのは HEAD の宣言だけ（[`RootsAtHead`]・作業ツリーは
//! 読まない）。

use super::{head_declaration, list_of, DeclError, Raw};
use std::path::Path;

/// 任意 key の名。
pub const KEY: &str = "crate-roots";

/// 固定の根（宣言に依らず常に在る）。
pub const FIXED_ROOT: &str = "crates/";

/// 根の列（固定の根だけ）。
pub fn fixed_roots() -> Vec<String> {
    vec![FIXED_ROOT.to_owned()]
}

/// 固定の根に宣言した根を足した列（固定の根が先頭）。
pub fn with_fixed(declared: &[String]) -> Vec<String> {
    let mut roots = fixed_roots();
    roots.extend(declared.iter().cloned());
    roots
}

/// crate の中の path（根・crate の名・crate の中の残り）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrateFile<'a> {
    /// 根（末尾 `/` を含む）。
    pub root: &'a str,
    /// crate の名（根の直後の 1 段・空でなく `/` を持たない）。
    pub name: &'a str,
    /// crate の中の残り（空でない）。
    pub rest: &'a str,
}

/// repo 相対の path が根のどれかの crate の中なら、根・crate の名・残りを返す（どの根の crate にも入らなければ `None`）。
pub fn crate_of<'a>(roots: &[String], path: &'a str) -> Option<CrateFile<'a>> {
    let root = roots.iter().find(|root| path.starts_with(root.as_str()))?;
    let (root, below) = path.split_at_checked(root.len())?;
    let (name, rest) = below.split_once('/')?;
    (!name.is_empty() && !rest.is_empty()).then_some(CrateFile { root, name, rest })
}

/// HEAD の宣言の根（閉じた 3 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootsAtHead {
    /// key が足した根（固定の根は含まない）。
    Declared(Vec<String>),
    /// 固定の根だけ（宣言 file が HEAD に無い・git を撃てない・key が無い）。
    Fixed,
    /// 読めない（宣言が在って不備）。「固定の根だけ」に倒さない（C10）。
    Unreadable,
}

impl RootsAtHead {
    /// HEAD の宣言から根を読む（読み手は [`head_declaration`] の 1 本・作業ツリーは読まない）。
    pub fn read(repo: &Path) -> Self {
        match head_declaration(repo) {
            None => Self::Fixed,
            Some(Err(_)) => Self::Unreadable,
            Some(Ok(declared)) if declared.crate_roots.is_empty() => Self::Fixed,
            Some(Ok(declared)) => Self::Declared(declared.crate_roots),
        }
    }
}

/// 宣言の本文から key を読む（無ければ空・項目の不備は key と行番号を名指して積む）。
pub(super) fn declared_of(found: &[(String, Raw, u64)], errors: &mut Vec<DeclError>) -> Vec<String> {
    let (items, line) = list_of(found, KEY, errors);
    for (at, item) in items.iter().enumerate() {
        if let Some(why) = item_defect(item) {
            errors.push(DeclError::new(line, format!("{KEY} の項目 {item:?} は{why}")));
        }
        for other in items.iter().skip(at.saturating_add(1)).filter(|other| overlaps(item, other)) {
            errors.push(DeclError::new(line, format!("{KEY} の項目 {item:?} と {other:?} は重なる（1 つの path が 2 つの根に入る）")));
        }
    }
    items
}

/// 項目 1 つの不備（無ければ `None`）。
fn item_defect(item: &str) -> Option<&'static str> {
    if item.trim().is_empty() {
        Some("空である")
    } else if !item.ends_with('/') {
        Some("末尾が / でない")
    } else if item.starts_with('/') {
        Some("絶対 path である")
    } else if item.contains('~') {
        Some("home の短縮記号を持つ")
    } else if item.split('/').any(|part| part == "..") {
        Some(".. の段を持つ")
    } else if overlaps(item, FIXED_ROOT) {
        Some("固定の根 crates/ と重なる")
    } else {
        None
    }
}

/// 同じ項目か一方が他方の接頭辞になるか（末尾 `/` の項目どうしなので path の段で見る）。
fn overlaps(left: &str, right: &str) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

#[cfg(test)]
mod tests {
    use super::{crate_of, fixed_roots, with_fixed, CrateFile, RootsAtHead};
    use crate::pipe::declaration::Declared;
    use crate::pipe::land::scope_touched;

    /// 必須 key だけの宣言（3 行）の後ろに `extra` を足す。
    fn with(extra: &str) -> String {
        format!("schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git diff --quiet\"]\n{extra}")
    }

    /// 宣言だけの 1 commit を持つ使い捨ての git repo（`None` は宣言 file を持たない repo）。
    fn repo_declaring(name: &str, declaration: Option<&str>) -> std::path::PathBuf {
        let repo = crate::pipe::fixture::scratch(name);
        let file = repo.join(crate::pipe::declaration::DECL_FILE);
        let setup: [&[&str]; 4] =
            [&["init", "-q", "-b", "main"], &["config", "user.name", "t"], &["config", "user.email", "t@example.invalid"], &["config", "commit.gpgsign", "false"]];
        for args in setup {
            assert!(crate::pipe::git_ok(&repo, args), "{args:?}");
        }
        assert!(declaration.is_none_or(|text| std::fs::write(&file, text).is_ok()), "宣言を書けた");
        assert!(crate::pipe::git_ok(&repo, &["add", "-A"]), "add");
        assert!(crate::pipe::git_ok(&repo, &["commit", "-q", "--allow-empty", "-m", "seed"]), "commit");
        repo
    }

    /// (a) key が無い宣言は根が空（＝固定の根だけ）、2 項目は 2 つ・足すだけで固定の根は先頭に残る。不備 7 形は key と行番号（4 行目）を名指す。
    #[test]
    fn declaration_crate_roots_reads_the_key_and_names_seven_defects() {
        assert_eq!(Declared::parse(&with("")).map(|found| found.crate_roots), Ok(Vec::new()));
        let two = Declared::parse(&with("crate-roots = [\"nest/crates/\", \"apps/\"]\n")).map(|found| found.crate_roots);
        assert_eq!(two, Ok(vec!["nest/crates/".to_owned(), "apps/".to_owned()]));
        assert_eq!(with_fixed(&two.unwrap_or_default()), ["crates/", "nest/crates/", "apps/"], "足すだけ（固定の根が先頭）");
        let home = format!("[\"{}\"]", concat!("~", "/x/"));
        let bad = ["[\" \"]", "[\"nest\"]", "[\"/nest/\"]", home.as_str(), "[\"a/../b/\"]", "[\"crates/x/\"]", "[\"nest/\", \"nest/deep/\"]"];
        for value in bad {
            let errors = Declared::parse(&with(&format!("crate-roots = {value}\n"))).expect_err(value);
            assert!(errors.iter().any(|error| error.line == 4 && error.reason.contains("crate-roots")), "{value}: {errors:?}");
        }
        let errors = Declared::parse(&with("crate-roots = [\"nest/\", \"apps/\"]\n")).err();
        assert_eq!(errors, None, "重ならない 2 項目は不備でない");
    }

    /// (b) 1 関数の表: 宣言した根の下は根・crate・残り、宣言しない repo では無し、固定の根は宣言に依らず同じ、根の直下の file と crate の名の無い path は無し。
    #[test]
    fn declaration_crate_roots_function_table() {
        let nested = with_fixed(&["nest/crates/".to_owned()]);
        let file = |root, name, rest| Some(CrateFile { root, name, rest });
        let table = [
            (&nested, "nest/crates/toy/src/a.rs", file("nest/crates/", "toy", "src/a.rs")),
            (&fixed_roots(), "nest/crates/toy/src/a.rs", None),
            (&nested, "crates/toy/src/a.rs", file("crates/", "toy", "src/a.rs")),
            (&fixed_roots(), "crates/toy/src/a.rs", file("crates/", "toy", "src/a.rs")),
            (&nested, "nest/crates/README.md", None),
            (&nested, "nest/crates/toy/", None),
            (&nested, "nest/crates//src/a.rs", None),
            (&nested, "nest/cratesx/toy/src/a.rs", None),
        ];
        for (roots, path, want) in table {
            assert_eq!(crate_of(roots, path), want, "{roots:?} {path}");
        }
    }

    /// (c) 面: 宣言した根の下の path だけの列は真、固定の根だけは偽、既存の面の表は根に依らず同じ。空の列は偽。
    #[test]
    fn declaration_crate_roots_scope_adds_the_declared_roots_to_the_fixed_face() {
        let declared = RootsAtHead::Declared(vec!["nest/crates/".to_owned()]);
        let (nested, notes, fixed) = (["nest/crates/toy/src/a.rs"], ["notes/a.md"], ["crates/toy/src/a.rs", "Cargo.lock", "rules/x.toml"]);
        assert!(scope_touched(&declared, &nested), "宣言した根の下は面");
        assert!(!scope_touched(&RootsAtHead::Fixed, &nested), "宣言しない列は面の外");
        assert!(!scope_touched(&declared, &notes), "面の外は外のまま");
        assert!(!scope_touched(&declared, &[]), "空の列は偽");
        for roots in [RootsAtHead::Fixed, declared] {
            assert!(fixed.iter().all(|path| scope_touched(&roots, &[path])), "{roots:?} 既存の面は同じ");
        }
    }

    /// (e) HEAD の口: 宣言 file の無い repo は固定の根だけ・key を持つ宣言は宣言した根・壊した宣言は読めない。
    /// 読めない周は面の外の path だけの列でも「触れる」を返す。
    #[test]
    fn declaration_crate_roots_head_reads_three_values_and_unreadable_touches() {
        let none = repo_declaring("crate-roots-none", None);
        assert_eq!(RootsAtHead::read(&none), RootsAtHead::Fixed);
        let plain = repo_declaring("crate-roots-plain", Some(&with("")));
        assert_eq!(RootsAtHead::read(&plain), RootsAtHead::Fixed);
        let declared = repo_declaring("crate-roots-declared", Some(&with("crate-roots = [\"nest/crates/\"]\n")));
        assert_eq!(RootsAtHead::read(&declared), RootsAtHead::Declared(vec!["nest/crates/".to_owned()]));
        let broken = repo_declaring("crate-roots-broken", Some(&with("crate-roots = [\"/abs/\"]\n")));
        let unreadable = RootsAtHead::read(&broken);
        assert_eq!(unreadable, RootsAtHead::Unreadable);
        assert!(scope_touched(&unreadable, &["notes/a.md"]), "読めない周は面の外の列でも撃つ側");
    }
}
