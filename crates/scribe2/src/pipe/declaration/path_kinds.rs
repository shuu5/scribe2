//! path の種別に属する prefix の集合を対象 repo の vessel 宣言が名乗る（設計 docs/design/seat-roles.md §24・
//! ADR-0047・SRS FR45 / FR41 / NFR4）。
//!
//! 宣言の任意 key 3 本（[`KEYS`]）は repo 相対の prefix の**配列**で、末尾が `/` の項目はその dir の下の全 file、
//! `/` で終わらない項目はその path と完全一致の 1 file。書かれた key はその種別の固定の判定を**置き換え**
//! （足し合わせない）、書かれていない key の種別は固定の判定のまま＝3 本とも無い宣言と宣言 file を持たない
//! repo は今と 1 行も変わらない（[`PathKinds::Default`]）。
//!
//! 不正な項目（`..` の段・絶対 path・空文字・種別の間で重なる項目）が 1 件でも在る周と、宣言 file が在るのに
//! 読めない周は [`PathKinds::Invalid`]（理由は閉じた enum [`Invalid`]・宣言順 = 検査順）で、guard は repo 内の
//! 全 file を code の種別として扱う（fail-closed・黙って固定値へ戻さない・憲法 C10）。
//!
//! 読むのは anchor の **HEAD の tree**（親 module の読み手と同じ 1 本・作業ツリーは読まない＝commit されていない
//! 宣言は無いのと同じ）。値の受理集合と配列の層は既存の key と共有する（第 2 の parser を作らない）。

use super::{head_declaration, list_of, DeclError, Raw};
use std::path::Path;

/// `design-intent` の種別の prefix の key。
pub const DESIGN_INTENT_KEY: &str = "design-intent-paths";

/// `design-doc` の種別の prefix の key。
pub const DESIGN_DOC_KEY: &str = "design-doc-paths";

/// `tests` の種別の prefix の key。
pub const TESTS_KEY: &str = "tests-paths";

/// 任意 key 3 本（種別の宣言順）。
pub const KEYS: [&str; 3] = [DESIGN_INTENT_KEY, DESIGN_DOC_KEY, TESTS_KEY];

/// 書かれていた 3 本の key の値。無い key は `None`（書いた周の空配列は配列の層が先に断る＝不備）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeclaredPaths {
    /// `design-intent-paths` の項目。
    pub design_intent: Option<Vec<String>>,
    /// `design-doc-paths` の項目。
    pub design_doc: Option<Vec<String>>,
    /// `tests-paths` の項目。
    pub tests: Option<Vec<String>>,
}

impl DeclaredPaths {
    /// key の順の (key, 値)。
    pub fn entries(&self) -> [(&'static str, Option<&[String]>); 3] {
        [
            (DESIGN_INTENT_KEY, self.design_intent.as_deref()),
            (DESIGN_DOC_KEY, self.design_doc.as_deref()),
            (TESTS_KEY, self.tests.as_deref()),
        ]
    }

    /// 書かれた key の数（doctor の `declared:<数>`）。
    pub fn written(&self) -> usize {
        self.entries().iter().filter(|(_, items)| items.is_some()).count()
    }
}

/// 宣言が不正な理由（closed enum・**宣言順 = 検査順**・deny の行と doctor の欄に載る字面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Invalid {
    /// 項目が `..` の段を含む。
    ParentSegment,
    /// 項目が絶対 path（`/` で始まる）。
    Absolute,
    /// 項目が空文字（空白だけ）。
    Empty,
    /// 同じ項目か一方が他方の prefix になる項目が 2 つの種別にまたがる。
    Overlap,
    /// 宣言 file が在るのに読めない（parser の不備が 1 件でも在る）。
    Unreadable,
}

/// [`Invalid`] の全 variant（宣言順）。
pub const INVALID_REASONS: [Invalid; 5] =
    [Invalid::ParentSegment, Invalid::Absolute, Invalid::Empty, Invalid::Overlap, Invalid::Unreadable];

impl Invalid {
    /// deny の行と doctor の欄に載る字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ParentSegment => "parent-segment",
            Self::Absolute => "absolute",
            Self::Empty => "empty",
            Self::Overlap => "overlap",
            Self::Unreadable => "unreadable",
        }
    }
}

/// anchor の HEAD の宣言から解いた、種別ごとの prefix の集合の出所（doctor の 3 つの state と同じ形）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathKinds {
    /// 3 本とも書かれていない（宣言 file を持たない repo も同じ）＝全種別が固定の判定。
    Default,
    /// 1 本以上が書かれている。書かれた種別だけが宣言で決まり、残りは固定の判定のまま。
    Declared(DeclaredPaths),
    /// 不正な宣言＝repo 内の全 file が code の種別（repo の外の判定は宣言に依らない）。
    Invalid(Invalid),
}

impl PathKinds {
    /// 書かれた値を検査して state にする（pure）。不正は宣言順で最初の理由 1 つ。
    pub fn resolve(declared: &DeclaredPaths) -> Self {
        if declared.written() == 0 {
            return Self::Default;
        }
        let entries = declared.entries();
        let items: Vec<&[String]> = entries.iter().map(|(_, items)| items.unwrap_or_default()).collect();
        if let Some(found) = items.iter().flat_map(|found| found.iter()).find_map(|item| item_defect(item)) {
            return Self::Invalid(found);
        }
        let pairs = items.iter().enumerate().flat_map(|(at, mine)| items.iter().skip(at.saturating_add(1)).map(move |other| (*mine, *other)));
        if pairs.into_iter().any(|(mine, other)| overlaps(mine, other)) {
            return Self::Invalid(Invalid::Overlap);
        }
        Self::Declared(declared.clone())
    }

    /// anchor の HEAD の tree の宣言を読んで解く（git の子 process 1 回・作業ツリーは読まない）。宣言 file が
    /// 無い（HEAD に無い・git を撃てない）周は [`Self::Default`]、在って読めない周は `invalid:unreadable`。
    pub fn read_at_head(repo: &Path) -> Self {
        match head_declaration(repo) {
            None => Self::Default,
            Some(Err(_)) => Self::Invalid(Invalid::Unreadable),
            Some(Ok(declared)) => Self::resolve(&declared.path_kinds),
        }
    }

    /// doctor の欄と deny の行の字面（`default` / `declared:<書かれた key の数>` / `invalid:<理由>`）。
    pub fn render(&self) -> String {
        match self {
            Self::Default => "default".to_owned(),
            Self::Declared(declared) => format!("declared:{}", declared.written()),
            Self::Invalid(reason) => format!("invalid:{}", reason.as_str()),
        }
    }

    /// 不正な周の理由（それ以外は `None`）。
    pub fn invalid(&self) -> Option<Invalid> {
        match self {
            Self::Invalid(reason) => Some(*reason),
            _ => None,
        }
    }
}

/// 項目 1 つの不備（宣言順で最初の理由・重なりは項目の対で見るので別）。
fn item_defect(item: &str) -> Option<Invalid> {
    if item.split('/').any(|part| part == "..") {
        Some(Invalid::ParentSegment)
    } else if item.starts_with('/') {
        Some(Invalid::Absolute)
    } else if item.trim().is_empty() {
        Some(Invalid::Empty)
    } else {
        None
    }
}

/// 2 つの種別の項目が重なるか（同じ項目・一方が他方の prefix＝字面の先頭一致で見る・fail-closed の側）。
fn overlaps(mine: &[String], other: &[String]) -> bool {
    mine.iter().any(|item| other.iter().any(|found| item.starts_with(found.as_str()) || found.starts_with(item.as_str())))
}

/// repo 相対 path が項目に当たるか: 末尾 `/` の項目はその dir 自身とその下の全 file・それ以外は完全一致の 1 file。
pub fn matches(item: &str, rel: &str) -> bool {
    match item.strip_suffix('/') {
        Some(dir) => rel == dir || rel.starts_with(item),
        None => rel == item,
    }
}

/// repo 相対 path が項目のどれかに当たるか。
pub fn under_any(items: &[String], rel: &str) -> bool {
    items.iter().any(|item| matches(item, rel))
}

/// 宣言の本文から 3 本の key を読む（無い key は `None`・型違いは親の配列の層が積む）。
pub(super) fn declared_of(found: &[(String, Raw, u64)], errors: &mut Vec<DeclError>) -> DeclaredPaths {
    let mut read = |key: &str| {
        let (items, line) = list_of(found, key, errors);
        (line != 0).then_some(items)
    };
    let design_intent = read(DESIGN_INTENT_KEY);
    let design_doc = read(DESIGN_DOC_KEY);
    let tests = read(TESTS_KEY);
    DeclaredPaths { design_intent, design_doc, tests }
}

#[cfg(test)]
mod tests {
    use super::{matches, DeclaredPaths, Invalid, PathKinds, INVALID_REASONS, KEYS};
    use crate::pipe::declaration::Declared;

    /// 文字列の列。
    fn strings(items: &[&str]) -> Option<Vec<String>> {
        Some(items.iter().map(|item| (*item).to_owned()).collect())
    }

    /// 宣言の本文（必須 key + 追加の行）。
    fn body(extra: &str) -> String {
        format!("schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git status\"]\n{extra}")
    }

    /// 3 本の key は任意で、無い key は `None`・書いた key は配列のまま読める・書いた空配列は従来どおり不備。
    #[test]
    fn hook_role_paths_keys_are_optional_arrays() {
        let none = Declared::parse(&body("")).unwrap_or_else(|errors| panic!("{errors:?}"));
        assert_eq!(none.path_kinds, DeclaredPaths::default(), "無い key は None");
        assert_eq!(PathKinds::resolve(&none.path_kinds), PathKinds::Default, "3 本とも無い宣言は固定の判定");
        let two = Declared::parse(&body("design-intent-paths = [\"spec/\"]\ntests-paths = [\"t/\", \"check.py\"]\n"))
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        assert_eq!(two.path_kinds.design_intent, strings(&["spec/"]));
        assert_eq!(two.path_kinds.design_doc, None);
        assert_eq!(two.path_kinds.tests, strings(&["t/", "check.py"]));
        assert_eq!(two.path_kinds.written(), 2);
        assert_eq!(PathKinds::resolve(&two.path_kinds).render(), "declared:2");
        for key in KEYS {
            let errors = Declared::parse(&body(&format!("{key} = []\n"))).expect_err("書いた空配列は不備");
            assert!(errors.iter().any(|error| error.reason.contains("配列が空である")), "{key}: {errors:?}");
            let errors = Declared::parse(&body(&format!("{key} = \"spec/\"\n"))).expect_err("文字列は不備");
            assert!(errors.iter().any(|error| error.reason.contains(&format!("{key} は配列である"))), "{key}: {errors:?}");
        }
    }

    /// 2 種別の宣言（空の側は無い key）。
    fn declared(intent: &[&str], doc: &[&str]) -> DeclaredPaths {
        DeclaredPaths {
            design_intent: (!intent.is_empty()).then(|| strings(intent)).flatten(),
            design_doc: (!doc.is_empty()).then(|| strings(doc)).flatten(),
            tests: None,
        }
    }

    /// 不正の理由は宣言順で最初の 1 つ: `..` の段 → 絶対 path → 空文字 → 種別の間の重なり。項目の不備は重なりより先。
    #[test]
    fn hook_role_paths_resolve_names_the_first_defect_in_declaration_order() {
        assert_eq!(PathKinds::resolve(&declared(&["a/../b/"], &[])).invalid(), Some(Invalid::ParentSegment));
        assert_eq!(PathKinds::resolve(&declared(&["/a/../b/"], &[])).invalid(), Some(Invalid::ParentSegment), "先の理由が勝つ");
        assert_eq!(PathKinds::resolve(&declared(&["/spec/"], &[])).invalid(), Some(Invalid::Absolute));
        assert_eq!(PathKinds::resolve(&declared(&[" "], &[])).invalid(), Some(Invalid::Empty));
        assert_eq!(PathKinds::resolve(&declared(&["spec/"], &["/x"])).invalid(), Some(Invalid::Absolute), "項目の不備が重なりより先");
        let words: Vec<&str> = INVALID_REASONS.iter().map(|reason| reason.as_str()).collect();
        assert_eq!(words, ["parent-segment", "absolute", "empty", "overlap", "unreadable"], "字面は宣言順");
        for pair in INVALID_REASONS.windows(2) {
            assert!(pair.first() < pair.get(1), "判別子順に並ぶ: {pair:?}");
        }
        assert_eq!(PathKinds::Invalid(Invalid::Overlap).render(), "invalid:overlap");
        assert_eq!(PathKinds::Default.render(), "default");
    }

    /// 種別の間の重なりは同じ項目と一方が他方の prefix になる項目（両向き）。同じ種別の中の重複と似た名の兄弟は
    /// 不正ではなく、`unreadable` は読み手の側の理由である（`resolve` は返さない）。
    #[test]
    fn hook_role_paths_resolve_names_overlap_across_kinds_only() {
        assert_eq!(PathKinds::resolve(&declared(&["spec/"], &["spec/"])).invalid(), Some(Invalid::Overlap), "同じ項目");
        assert_eq!(PathKinds::resolve(&declared(&["spec/"], &["spec/design/"])).invalid(), Some(Invalid::Overlap), "prefix");
        assert_eq!(PathKinds::resolve(&declared(&["spec/design/"], &["spec/"])).invalid(), Some(Invalid::Overlap), "逆向き");
        assert_eq!(PathKinds::resolve(&declared(&["spec/", "spec/"], &["docs/"])).invalid(), None, "同じ種別の重複は不正でない");
        assert_eq!(PathKinds::resolve(&declared(&["spec/"], &["specs/"])).invalid(), None, "似た名の兄弟は重ならない");
        let three = DeclaredPaths { tests: strings(&["docs/t/"]), ..declared(&["spec/"], &["docs/"]) };
        assert_eq!(PathKinds::resolve(&three).invalid(), Some(Invalid::Overlap), "2 本目と 3 本目の間の重なりも見る");
        assert_eq!(PathKinds::resolve(&declared(&["spec/"], &["docs/"])).render(), "declared:2");
    }

    /// 項目の当たり方: 末尾 `/` はその dir 自身と下の全 file・`/` で終わらない項目は完全一致の 1 file だけ。
    #[test]
    fn hook_role_paths_item_matches_dir_prefix_or_exact_file() {
        assert!(matches("spec/", "spec/srs.yaml"));
        assert!(matches("spec/", "spec/deep/x.md"));
        assert!(matches("spec/", "spec"), "dir 自身");
        assert!(!matches("spec/", "specs/x.md"), "似た名の兄弟");
        assert!(!matches("spec/", "src/spec/x.md"), "途中の段");
        assert!(matches("DESIGN.md", "DESIGN.md"));
        assert!(!matches("DESIGN.md", "DESIGN.md.bak"), "同じ名で始まる別の file");
        assert!(!matches("DESIGN.md", "DESIGN.md/x"), "同じ名の dir の下");
        assert!(!matches("DESIGN.md", "docs/DESIGN.md"), "別の段の同じ名");
    }
}
