//! 出所 pointer の形と解決（席の指示文の雛形の規律・ADR-0022 §2.4・憲法 N2）。
//!
//! `seat/wm.rs` から**挙動不変で移した**もの（`s2-07l.479.2`）: 退避物の命令行は ADR-0045 §2 (2) で
//! 消えたが、**席の指示文の行は「穴か出所 pointer を持つ行」に限る**という規律は残る（[`super::classify_line`]
//! が読み、`xtask check` の seat-brief の measure が同じ規律を雛形 file に対して測る）。判定は字面の
//! 語彙でなく**形**で行い、解を anchor の実在（[`Anchor::resolve`]）で確かめる。
// flip-check: moved s2-07l.479.2

use crate::rules::manifest::Manifest;
use std::path::{Component, Path, PathBuf};

/// 出所 pointer の区切り。
const SSOT: &str = "→ SSOT:";

/// 憲法の置き場（anchor 相対）。
const CONSTITUTION: &str = "design-intent/spec/constitution.html";

/// ADR の置き場（anchor 相対）。
const DECISIONS: &str = "design-intent/decisions";

/// 設計 doc の置き場（anchor 相対）。
const DESIGN_DIR: &str = "docs/design/";

/// rules manifest の置き場（anchor 相対）。
const MANIFEST: &str = "rules/manifest.toml";

/// 台帳の設定（anchor 相対・`issue-prefix:`）。
const LEDGER_CONFIG: &str = ".beads/config.yaml";

/// 台帳の metadata（anchor 相対・`dolt_database`）。
const LEDGER_METADATA: &str = ".beads/metadata.json";

/// 出所 pointer の形。**宣言順 = 優先順位**（C2・散文の注記を持たない・正本は [`ALL`] と外形 snapshot）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PointerKind {
    /// 憲法の条（`憲法 C11.2` / `N2`）。
    Constitution,
    /// ADR（`ADR-0018 §2.2`）。
    Adr,
    /// 設計 doc（`docs/design/<名>.md`）。
    Design,
    /// rules manifest の行 id（`rules 行 seat.wm_directive_cap` / `R-C4-2`）。
    Manifest,
    /// 台帳の id（`<prefix>-<id>`・prefix は `.beads` から解く）。
    Ledger,
    /// repo 内の相対 path（`/` を含む・`#節` を許す）。
    RepoPath,
    /// auto-memory（`auto-memory <slug>` / `memory <slug>.md`）。
    Memory,
    /// PR 番号（`PR #<n>` / `#<n>`）。
    PullRequest,
}

/// [`PointerKind`] の全 variant（宣言順）。
pub const ALL: &[PointerKind] = &[
    PointerKind::Constitution,
    PointerKind::Adr,
    PointerKind::Design,
    PointerKind::Manifest,
    PointerKind::Ledger,
    PointerKind::RepoPath,
    PointerKind::Memory,
    PointerKind::PullRequest,
];

/// 参照の前置き語（1 つの参照は高々 1 つを持つ）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lead {
    /// 前置き無し。
    Bare,
    /// `憲法 `。
    Constitution,
    /// `rules 行 `。
    Rules,
    /// `auto-memory `。
    AutoMemory,
    /// `memory `。
    MemoryFile,
    /// `PR `。
    Pr,
}

/// 前置き語の字面と種類。
const LEADS: [(&str, Lead); 5] = [
    ("憲法 ", Lead::Constitution),
    ("rules 行 ", Lead::Rules),
    ("auto-memory ", Lead::AutoMemory),
    ("memory ", Lead::MemoryFile),
    ("PR ", Lead::Pr),
];

/// 参照を前置き語と先頭の語（`§` 以降を落とす）に分ける。
fn split_lead(reference: &str) -> (Lead, &str) {
    let trimmed = reference.trim();
    let (lead, rest) = LEADS
        .iter()
        .find_map(|(text, lead)| trimmed.strip_prefix(text).map(|rest| (*lead, rest)))
        .unwrap_or((Lead::Bare, trimmed));
    let word = rest.split_whitespace().next().unwrap_or_default();
    (lead, word.split('§').next().unwrap_or_default())
}

impl PointerKind {
    /// DATA と snapshot に出す名前（variant 名）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Constitution => "Constitution",
            Self::Adr => "Adr",
            Self::Design => "Design",
            Self::Manifest => "Manifest",
            Self::Ledger => "Ledger",
            Self::RepoPath => "RepoPath",
            Self::Memory => "Memory",
            Self::PullRequest => "PullRequest",
        }
    }

    /// 参照 1 つがこの kind の形か。**kind 同士の形は互いに素**（前置き語が 1 つに決まり、前置き無しの
    /// 先頭語は字種で分かれる）＝任意の参照は高々 1 つの kind に当たる。
    pub fn matches(self, reference: &str, prefixes: &[String]) -> bool {
        let (lead, head) = split_lead(reference);
        match self {
            Self::Constitution => matches!(lead, Lead::Bare | Lead::Constitution) && is_constitution_id(head),
            Self::Adr => lead == Lead::Bare && adr_number(head).is_some(),
            Self::Design => lead == Lead::Bare && is_design(head),
            Self::Manifest => matches!(lead, Lead::Bare | Lead::Rules) && is_manifest_id(head),
            Self::Ledger => lead == Lead::Bare && is_ledger(head, prefixes),
            Self::RepoPath => lead == Lead::Bare && head.contains('/') && !is_design(head),
            Self::Memory => match lead {
                Lead::AutoMemory => is_slug(head),
                Lead::MemoryFile => head.strip_suffix(".md").is_some_and(is_slug),
                _ => false,
            },
            Self::PullRequest => matches!(lead, Lead::Bare | Lead::Pr) && is_pr_number(head),
        }
    }
}

/// 参照 1 つを分類する（宣言順で最初に当たる kind・形が互いに素なので順序は結果を変えない）。
pub fn classify(reference: &str, prefixes: &[String]) -> Option<PointerKind> {
    ALL.iter().copied().find(|kind| kind.matches(reference, prefixes))
}

/// `C\d+(\.\d+)?` / `A\d(\.\d)?` / `N\d(\.\d)?`。
fn is_constitution_id(head: &str) -> bool {
    let Some((letter, rest)) = head.split_at_checked(1) else {
        return false;
    };
    let (major, minor) = match rest.split_once('.') {
        Some((major, minor)) => (major, Some(minor)),
        None => (rest, None),
    };
    let digits = |text: &str, one: bool| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) && (!one || text.len() == 1);
    let one = match letter {
        "C" => false,
        "A" | "N" => true,
        _ => return false,
    };
    digits(major, one) && minor.is_none_or(|minor| digits(minor, one))
}

/// `ADR-\d{4}` の番号。
fn adr_number(head: &str) -> Option<&str> {
    head.strip_prefix("ADR-")
        .filter(|number| number.len() == 4 && number.bytes().all(|b| b.is_ascii_digit()))
}

/// `docs/design/<名>.md`（`#節` を許す）。
fn is_design(head: &str) -> bool {
    path_part(head)
        .strip_prefix(DESIGN_DIR)
        .and_then(|name| name.strip_suffix(".md"))
        .is_some_and(|name| !name.is_empty() && !name.contains('/'))
}

/// `R-…`（`[A-Za-z0-9.-]`）/ `<領域>.<名>`（`[a-z]+` `.` `[a-z][a-z0-9_]*`）。
fn is_manifest_id(head: &str) -> bool {
    if let Some(rest) = head.strip_prefix("R-") {
        return !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    }
    let Some((area, name)) = head.split_once('.') else {
        return false;
    };
    !area.is_empty()
        && area.bytes().all(|b| b.is_ascii_lowercase())
        && name.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// `<prefix>-<id>`（id は `[A-Za-z0-9][A-Za-z0-9.]*`・prefix は `[a-z0-9]+` のものだけ）。
fn is_ledger(head: &str, prefixes: &[String]) -> bool {
    prefixes.iter().filter(|prefix| is_ledger_prefix(prefix)).any(|prefix| {
        head.strip_prefix(prefix.as_str())
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(|id| {
                id.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
                    && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
            })
    })
}

/// 台帳 prefix として受ける字面（小文字英数だけ＝`R-` / `ADR-` の形と交わらない）。
fn is_ledger_prefix(prefix: &str) -> bool {
    !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// memory の slug（`[A-Za-z0-9_-]+`）。
fn is_slug(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `#<n>`。
fn is_pr_number(head: &str) -> bool {
    head.strip_prefix('#')
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// path の `#節` を落とした部分。
fn path_part(head: &str) -> &str {
    head.split('#').next().unwrap_or_default()
}

/// 実在検査の 3 値（**融合しない**・ADR-0018 §2.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// 参照先が在る。
    Resolved,
    /// 形は当たるが参照先が無い（矢印は在るが先が無い）。
    Unresolved,
    /// 器が到達しない kind（台帳・memory・PR）。
    Unchecked,
}

impl Resolution {
    /// DATA に出す名前。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Unresolved => "unresolved",
            Self::Unchecked => "unchecked",
        }
    }
}

/// 実在検査の root（repo の anchor）と台帳 prefix。**env を読まない**（`--anchor` で受ける）。
pub struct Anchor {
    /// repo root。
    root: PathBuf,
    /// 台帳 prefix（`.beads` から解いたもの）。
    prefixes: Vec<String>,
}

impl Anchor {
    /// dir を anchor として開く（dir でなければ `None`）。台帳 prefix は `.beads/config.yaml` の
    /// `issue-prefix:`、無ければ `.beads/metadata.json` の `dolt_database` から解く（無ければ空＝台帳の
    /// 形は分類できず、その参照だけの行は暫定行になる）。
    pub fn open(root: &Path) -> Option<Self> {
        if !root.is_dir() {
            return None;
        }
        let prefixes = ledger_prefix(root).into_iter().collect();
        Some(Self::with_prefixes(root, prefixes))
    }

    /// prefix を明示して作る（歯と rebrief が同じ root を使い回す口）。
    pub fn with_prefixes(root: &Path, prefixes: Vec<String>) -> Self {
        Self {
            root: root.to_path_buf(),
            prefixes,
        }
    }

    /// 台帳 prefix。
    pub fn prefixes(&self) -> &[String] {
        &self.prefixes
    }

    /// 参照 1 つの実在を検査する（`kind` は [`classify`] の結果）。
    pub fn resolve(&self, kind: PointerKind, reference: &str) -> Resolution {
        let (_, head) = split_lead(reference);
        let found = match kind {
            PointerKind::Ledger | PointerKind::Memory | PointerKind::PullRequest => {
                return Resolution::Unchecked;
            }
            PointerKind::Constitution => self.has_constitution_id(head),
            PointerKind::Adr => adr_number(head).is_some_and(|number| self.has_adr(number)),
            PointerKind::Design | PointerKind::RepoPath => self.has_file(path_part(head)),
            PointerKind::Manifest => Manifest::load(&self.root.join(MANIFEST))
                .is_ok_and(|manifest| manifest.get(head).is_some()),
        };
        if found {
            Resolution::Resolved
        } else {
            Resolution::Unresolved
        }
    }

    /// 憲法に `id="<小文字 id>"` が在るか。条の下位（`C11.2`）は anchor を持たない版があるので、
    /// 下位の id が無ければ**親の条**の id で読む（親も無ければ Unresolved）。
    fn has_constitution_id(&self, head: &str) -> bool {
        let Ok(text) = std::fs::read_to_string(self.root.join(CONSTITUTION)) else {
            return false;
        };
        let id = head.to_ascii_lowercase();
        let has = |id: &str| text.contains(&format!("id=\"{id}\""));
        has(&id) || id.split_once('.').is_some_and(|(parent, _)| has(parent))
    }

    /// `ADR-<n>-*.html` が在るか。
    fn has_adr(&self, number: &str) -> bool {
        let Ok(entries) = std::fs::read_dir(self.root.join(DECISIONS)) else {
            return false;
        };
        let lead = format!("ADR-{number}-");
        entries.flatten().any(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with(&lead) && name.ends_with(".html")
        })
    }

    /// anchor 内の file が在るか（絶対 path・`..` は anchor の外＝Unresolved）。
    fn has_file(&self, relative: &str) -> bool {
        let path = Path::new(relative);
        let inside = !relative.is_empty()
            && path
                .components()
                .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
        inside && self.root.join(path).is_file()
    }
}

/// `.beads` から台帳 prefix を 1 つ解く。
fn ledger_prefix(root: &Path) -> Option<String> {
    let clean = |value: &str| value.trim().trim_matches(|ch| ch == '"' || ch == '\'' || ch == ',').trim().to_owned();
    let configured = std::fs::read_to_string(root.join(LEDGER_CONFIG)).ok().and_then(|text| {
        text.lines()
            .find_map(|line| line.strip_prefix("issue-prefix:"))
            .map(clean)
            .filter(|prefix| !prefix.is_empty())
    });
    configured.or_else(|| {
        std::fs::read_to_string(root.join(LEDGER_METADATA)).ok().and_then(|text| {
            text.lines()
                .find_map(|line| line.trim().strip_prefix("\"dolt_database\"")?.trim_start().strip_prefix(':').map(clean))
                .filter(|prefix| !prefix.is_empty())
        })
    })
}

/// 項目の `→ SSOT:` 以降の参照の列（` / ` `、` `・` `,` `;` で区切る・従属行も含める）。
pub fn references(text: &str) -> Vec<String> {
    let Some((_, tail)) = text.rsplit_once(SSOT) else {
        return Vec::new();
    };
    tail.replace(" / ", "\n")
        .split(['\n', '、', '・', ',', ';'])
        .map(str::trim)
        .filter(|reference| !reference.is_empty())
        .map(str::to_owned)
        .collect()
}
