//! 作業記憶（退避物）の schema と出所 pointer の面（設計 docs/design/working-memory.md §2 / §3 / §4・
//! ADR-0018 §2.1 / §2.2・SRS FR23）。
//!
//! 退避物の frontmatter と 3 節の読み書き・節 3 の項目行の文法検査・出所 pointer の分類
//! （[`PointerKind`]）と実在検査（[`Resolution`]）を持つ。file の探索・carry-forward・書込の手順は
//! [`super::externalize`] が持つ（読みと手順を分ける・R-C4-2）。
//!
//! **env を読まない**（憲法 C2.2）: 実在検査の repo root は [`Anchor`] で引数から受ける。器は行の
//! 本文と拘束 SSOT の食い違いを判定しない（C3.3）——形と実在だけを見る。

use super::{FRONTMATTER, SEAT_KEY};
use crate::rules::manifest::Manifest;
use std::path::{Component, Path, PathBuf};

/// frontmatter の schema 版（任意 key・無ければこの版として読む・2 以上は読まない）。
pub const SCHEMA: u64 = 1;
/// 節 1 の見出し（固定・設計 §3）。
pub const HEAD_USER: &str = "## user 直命（verbatim・言い換え禁止）";
/// 節 2 の見出し。
pub const HEAD_PLAN: &str = "## 計画弧・次のステップ";
/// 節 3 の見出し。
pub const HEAD_DIRECTIVES: &str = "## この effort を貫く命令・制約";

/// schema 版の key。
const SCHEMA_KEY: &str = "schema";
/// 出所 pointer の区切り。
const SSOT: &str = "→ SSOT:";
/// 節 1 の状態の区切り。
const STATE_MARK: &str = "→ 状態:";
/// carry から落としてよい節 1 の状態（設計 §3・これ以外は 1 行も落とさない）。
const CLOSED_STATES: &[&str] = &["完了", "user 撤回"];
/// 節 3 の tag（3 語のいずれか）。
const TAGS: &[&str] = &["[auto]", "[confirm]", "[hard候補]"];
/// 節 3 の優先度（並びの位置が数値）。
const PRIORITIES: &[&str] = &["[P0]", "[P1]", "[P2]", "[P3]"];
/// `[P0-P3]` を持たない carry 元の行の優先度（P3・前の版と同じ・落とさない）。
const LOWEST: usize = 3;
/// since の前置き。
const SINCE: &str = "since=";
/// 項目行の前置き。
const BULLET: &str = "- ";
/// HTML コメントの開き。
const COMMENT_OPEN: &str = "<!--";
/// HTML コメントの閉じ。
const COMMENT_CLOSE: &str = "-->";
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

/// 節の 1 項目（先頭行 + 従属行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// 先頭行の物理行番号（1 始まり・入力 file の行）。
    pub line: usize,
    /// 項目の字面（従属行は `\n` で連結・逐語）。
    pub text: String,
}

/// 退避物 1 つの読み。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WmDoc {
    /// frontmatter の `key: value`（書かれた順）。
    pub front: Vec<(String, String)>,
    /// 節 1 の項目。
    pub user: Vec<Item>,
    /// 節 2 の本文（逐語・前後の空行を落とす）。
    pub plan: String,
    /// 節 3 の項目。
    pub directives: Vec<Item>,
}

/// 退避物を読めない理由（**0 件に潰さない**・憲法 C11）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WmError {
    /// 先頭が `---` でない・閉じが無い。
    NoFrontmatter,
    /// `seat:` が無い・空。
    NoSeat,
    /// `schema:` が整数でない・この版より新しい（字面を持つ）。
    Schema(String),
}

/// 読んでいる節。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    /// 見出しの前・未知の見出しの下。
    Outside,
    /// 節 1。
    User,
    /// 節 2。
    Plan,
    /// 節 3。
    Directives,
}

impl WmDoc {
    /// 退避物の全文を読む。`schema` が無ければ 1 相当・2 以上は `Err`・未知 key は許す。
    pub fn parse(text: &str) -> Result<Self, WmError> {
        let (front, body_at) = frontmatter(text)?;
        let seat = front.iter().find(|(key, _)| key == seat_key());
        if seat.is_none_or(|(_, value)| value.is_empty()) {
            return Err(WmError::NoSeat);
        }
        if let Some((_, value)) = front.iter().find(|(key, _)| key == SCHEMA_KEY) {
            match value.parse::<u64>() {
                Ok(found) if found <= SCHEMA => {}
                _ => return Err(WmError::Schema(value.clone())),
            }
        }
        let stripped = strip_comments(text);
        let mut buckets: [Vec<(usize, &str)>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        let mut section = Section::Outside;
        for (at, line) in stripped.lines().enumerate().skip(body_at) {
            section = match line.trim_end() {
                HEAD_USER => Section::User,
                HEAD_PLAN => Section::Plan,
                HEAD_DIRECTIVES => Section::Directives,
                other if other.starts_with("## ") => Section::Outside,
                _ => {
                    let slot = match section {
                        Section::User => buckets.get_mut(0),
                        Section::Plan => buckets.get_mut(1),
                        Section::Directives => buckets.get_mut(2),
                        Section::Outside => None,
                    };
                    if let Some(bucket) = slot {
                        bucket.push((at.saturating_add(1), line));
                    }
                    continue;
                }
            };
        }
        let [user, plan, directives] = buckets;
        Ok(Self {
            front,
            user: items_of(&user),
            plan: plan_of(&plan),
            directives: items_of(&directives),
        })
    }

    /// frontmatter が名乗る席。
    pub fn seat(&self) -> Option<&str> {
        self.value(seat_key())
    }

    /// frontmatter の値。
    pub fn value(&self, key: &str) -> Option<&str> {
        self.front
            .iter()
            .find(|(found, _)| found == key)
            .map(|(_, value)| value.as_str())
    }

    /// 全文を組む（frontmatter → 節 1 → 節 2 → 節 3・見出しは固定の字面）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(FRONTMATTER);
        out.push('\n');
        for (key, value) in &self.front {
            out.push_str(&format!("{key}: {value}\n"));
        }
        out.push_str(FRONTMATTER);
        out.push_str("\n\n");
        push_section(&mut out, HEAD_USER, self.user.iter().map(|item| item.text.as_str()));
        push_section(&mut out, HEAD_PLAN, std::iter::once(self.plan.as_str()).filter(|plan| !plan.is_empty()));
        push_section(&mut out, HEAD_DIRECTIVES, self.directives.iter().map(|item| item.text.as_str()));
        out
    }
}

/// `seat:` の key 部分（`SEAT_KEY` の colon を落とした字面＝名乗りの字は 1 面）。
fn seat_key() -> &'static str {
    SEAT_KEY.strip_suffix(':').unwrap_or(SEAT_KEY)
}

/// 見出しと本文を 1 節ぶん足す。
fn push_section<'a>(out: &mut String, head: &str, bodies: impl Iterator<Item = &'a str>) {
    out.push_str(head);
    out.push('\n');
    for body in bodies {
        out.push_str(body);
        out.push('\n');
    }
    out.push('\n');
}

/// frontmatter の `key: value` と、本文が始まる 0 始まりの行 index。
fn frontmatter(text: &str) -> Result<(Vec<(String, String)>, usize), WmError> {
    let mut lines = text.lines().enumerate();
    if lines.next().is_none_or(|(_, line)| line.trim() != FRONTMATTER) {
        return Err(WmError::NoFrontmatter);
    }
    let mut front = Vec::new();
    for (at, line) in lines {
        if line.trim() == FRONTMATTER {
            return Ok((front, at.saturating_add(1)));
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim().trim_matches('"').to_owned();
            front.push((key.trim().to_owned(), value));
        }
    }
    Err(WmError::NoFrontmatter)
}

/// HTML コメントを捨てる（**改行は残す**＝行番号を動かさない・テンプレの説明文を命令欄へ流さない）。
pub fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once(COMMENT_OPEN) {
        out.push_str(before);
        let (inside, tail) = after.split_once(COMMENT_CLOSE).unwrap_or((after, ""));
        out.extend(inside.chars().filter(|ch| *ch == '\n'));
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// file 全体を項目に分ける（行番号は 1 始まり・HTML コメントは捨てる）。
pub fn items(text: &str) -> Vec<Item> {
    let stripped = strip_comments(text);
    let lines: Vec<(usize, &str)> = stripped
        .lines()
        .enumerate()
        .map(|(at, line)| (at.saturating_add(1), line))
        .collect();
    items_of(&lines)
}

/// 行の列を項目に分ける。**非空で字下げの無い行**が項目を始め、**先頭が空白の行**は直前の項目に
/// 連結する（直前が無ければ自分で始める）。空行は捨てる。
fn items_of(lines: &[(usize, &str)]) -> Vec<Item> {
    let mut found: Vec<Item> = Vec::new();
    for (at, line) in lines {
        if line.trim().is_empty() {
            continue;
        }
        let dependent = line.starts_with(char::is_whitespace);
        match found.last_mut() {
            Some(last) if dependent => {
                last.text.push('\n');
                last.text.push_str(line.trim_end());
            }
            _ => found.push(Item {
                line: *at,
                text: line.trim_end().to_owned(),
            }),
        }
    }
    found
}

/// 節 2 の本文（前後の空行を落とす・中は逐語）。
fn plan_of(lines: &[(usize, &str)]) -> String {
    let body: Vec<&str> = lines.iter().map(|(_, line)| line.trim_end()).collect();
    body.join("\n").trim_matches('\n').to_owned()
}

/// 節 1 の項目が carry から落ちる状態（「完了」「user 撤回」）か。
pub fn is_closed(item: &Item) -> bool {
    item.text
        .rsplit_once(STATE_MARK)
        .is_some_and(|(_, state)| CLOSED_STATES.iter().any(|closed| state.trim_start().starts_with(closed)))
}

/// 節 3 の項目行で欠けた要素（文法検査・`--directives` の新規行にだけ掛ける）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// 先頭が `- ` でない。
    Bullet,
    /// tag `[auto|confirm|hard候補]` が無い。
    Tag,
    /// `[P0-P3]` が無い。
    Priority,
    /// `since=YYYY-MM-DD` が無い。
    Since,
}

impl Missing {
    /// 行に出す語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bullet => "bullet",
            Self::Tag => "tag",
            Self::Priority => "priority",
            Self::Since => "since",
        }
    }
}

/// 項目の先頭行の語。
fn head_words(item: &Item) -> impl Iterator<Item = &str> {
    item.text.lines().next().unwrap_or_default().split_whitespace()
}

/// 節 3 の文法検査。欠けた要素を**全部**返す（空 = 通る）。`→ SSOT:` の有無は見ない（暫定行）。
pub fn grammar(item: &Item) -> Vec<Missing> {
    let mut missing = Vec::new();
    if !item.text.starts_with(BULLET) {
        missing.push(Missing::Bullet);
    }
    if !head_words(item).any(|word| TAGS.contains(&word)) {
        missing.push(Missing::Tag);
    }
    if !head_words(item).any(|word| PRIORITIES.contains(&word)) {
        missing.push(Missing::Priority);
    }
    if !head_words(item).any(|word| word.strip_prefix(SINCE).is_some_and(is_date)) {
        missing.push(Missing::Since);
    }
    missing
}

/// `YYYY-MM-DD` の形か（暦の妥当性は見ない・形だけ）。
fn is_date(text: &str) -> bool {
    text.len() == 10
        && text.bytes().enumerate().all(|(at, byte)| match at {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// 項目の優先度（0〜3・`[P0-P3]` が無ければ P3）。安定 sort の key。
pub fn priority_of(item: &Item) -> usize {
    head_words(item)
        .find_map(|word| PRIORITIES.iter().position(|found| *found == word))
        .unwrap_or(LOWEST)
}

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

/// 項目行の出所 pointer の読み。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    /// 暫定行（`→ SSOT:` が無い・どの参照も分類できない）。
    Provisional,
    /// 分類できた行（最も強い kind と、参照ごとの実在の集約）。
    Pointed {
        /// 行の kind（宣言順で最小）。
        kind: PointerKind,
        /// 1 つでも Unresolved なら Unresolved・次に Resolved・全部未検査なら Unchecked。
        resolution: Resolution,
    },
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

/// 項目 1 つの出所 pointer を読む（分類できない参照は数えない・「user 裁定 <時刻>」だけの行は暫定）。
pub fn pointer_of(item: &Item, anchor: &Anchor) -> Pointer {
    let mut kind: Option<PointerKind> = None;
    let mut unresolved = false;
    let mut resolved = false;
    for reference in references(&item.text) {
        let Some(found) = classify(&reference, anchor.prefixes()) else {
            continue;
        };
        kind = Some(kind.map_or(found, |strongest| strongest.min(found)));
        match anchor.resolve(found, &reference) {
            Resolution::Unresolved => unresolved = true,
            Resolution::Resolved => resolved = true,
            Resolution::Unchecked => {}
        }
    }
    let Some(kind) = kind else {
        return Pointer::Provisional;
    };
    let resolution = if unresolved {
        Resolution::Unresolved
    } else if resolved {
        Resolution::Resolved
    } else {
        Resolution::Unchecked
    };
    Pointer::Pointed { kind, resolution }
}

#[cfg(test)]
mod tests {
    use super::{
        classify, grammar, is_closed, items, priority_of, strip_comments, Item, Missing, PointerKind, WmDoc, WmError,
        ALL, HEAD_DIRECTIVES, HEAD_PLAN, HEAD_USER,
    };
    use crate::order::is_declaration_order;

    /// 台帳 prefix（歯の固定値）。
    fn prefixes() -> Vec<String> {
        vec!["s2".to_owned()]
    }

    /// `ALL` は宣言順（判別子 0, 1, 2, …・ADR-0013 §2.2）で、並びは設計 §4 の表の初期集合。
    #[test]
    fn seat_wm_pointer_all_is_in_declaration_order() {
        assert!(is_declaration_order(ALL, |kind| kind as usize), "ALL は宣言順: {ALL:?}");
        let names: Vec<&str> = ALL.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(
            names,
            ["Constitution", "Adr", "Design", "Manifest", "Ledger", "RepoPath", "Memory", "PullRequest"],
            "宣言順 = 優先順位"
        );
        assert_eq!(PointerKind::Constitution as usize, 0, "最も強い kind の判別子");
        assert_eq!(PointerKind::PullRequest as usize, 7, "最も弱い kind の判別子");
        assert!(PointerKind::Constitution < PointerKind::RepoPath, "順序は宣言順から取る");
    }

    /// 設計 §4 の表の 8 形の例が、それぞれ 1 つの kind にだけ当たる。
    #[test]
    fn seat_wm_pointer_classifies_the_eight_forms_of_the_table() {
        let cases = [
            ("憲法 C11.2", PointerKind::Constitution),
            ("N2", PointerKind::Constitution),
            ("A3", PointerKind::Constitution),
            ("ADR-0018 §2.2", PointerKind::Adr),
            ("ADR-0014§2.1", PointerKind::Adr),
            ("docs/design/working-memory.md §4", PointerKind::Design),
            ("rules 行 seat.wm_directive_cap", PointerKind::Manifest),
            ("R-C4-2", PointerKind::Manifest),
            ("pipe.follow_retries", PointerKind::Manifest),
            ("s2-07l.61 notes 02:01Z", PointerKind::Ledger),
            ("crates/scribe2/src/seat/wm.rs#tests", PointerKind::RepoPath),
            ("auto-memory wm-directives-are-not-rules", PointerKind::Memory),
            ("memory wm-directives-are-not-rules.md", PointerKind::Memory),
            ("PR #128", PointerKind::PullRequest),
            ("#128", PointerKind::PullRequest),
        ];
        for (reference, want) in cases {
            assert_eq!(classify(reference, &prefixes()), Some(want), "{reference}");
            let hits = ALL.iter().filter(|kind| kind.matches(reference, &prefixes())).count();
            assert_eq!(hits, 1, "高々 1 kind: {reference}");
        }
        for reference in ["user 裁定 2026-09-12T02:01Z", "", "C", "ADR-18", "憲法 ADR-0018", "memory slug", "x-1", "CLAUDE.md"] {
            assert_eq!(classify(reference, &prefixes()), None, "分類できない: {reference}");
        }
        assert_eq!(classify("s2-07l.61", &[]), None, "prefix が解けなければ台帳の形は分類しない");
    }

    /// 文法検査は欠けた要素を全部返し、`→ SSOT:` の欠落では止めない。
    #[test]
    fn seat_wm_pointer_grammar_reports_every_missing_element() {
        let item = |text: &str| Item { line: 1, text: text.to_owned() };
        assert!(grammar(&item("- [auto] [P1] since=2026-09-12 本文 → SSOT: N2")).is_empty());
        assert!(grammar(&item("- [hard候補] [P0] since=2026-09-12 暫定")).is_empty(), "SSOT 欠落は通す");
        assert_eq!(grammar(&item("- [P1] since=2026-09-12 x")), vec![Missing::Tag]);
        assert_eq!(grammar(&item("- [confirm] since=2026-09-12 x")), vec![Missing::Priority]);
        assert_eq!(grammar(&item("- [auto] [P2] since=2026-9-1 x")), vec![Missing::Since]);
        assert_eq!(grammar(&item("x")), vec![Missing::Bullet, Missing::Tag, Missing::Priority, Missing::Since]);
        assert_eq!(priority_of(&item("- [auto] [P2] x")), 2);
        assert_eq!(priority_of(&item("- [auto] x")), 3, "無ければ P3");
    }

    /// 従属行は直前の項目に連結し、HTML コメントは行番号を動かさずに捨てる。
    #[test]
    fn seat_wm_pointer_items_join_dependents_and_drop_comments() {
        let text = "<!-- 説明\n説明の続き -->\n- a\n  続き\n\n- b <!-- 注 -->\n";
        let found = items(text);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found.first().map(|item| (item.line, item.text.as_str())), Some((3, "- a\n  続き")));
        assert_eq!(found.get(1).map(|item| (item.line, item.text.as_str())), Some((6, "- b")));
        assert_eq!(strip_comments("a<!--x\ny-->b"), "a\nb");
    }

    /// frontmatter の schema は任意（無ければ 1）・2 以上と seat 無しは読まない・3 節は round-trip。
    #[test]
    fn seat_wm_pointer_doc_parses_schema_and_sections() {
        let body = format!("\n{HEAD_USER}\n- [2026-09-12 10:00] 「x」 → 状態: 完了 s2-1\n{HEAD_PLAN}\n次\n{HEAD_DIRECTIVES}\n- [auto] [P1] since=2026-09-12 y\n");
        let legacy = format!("---\nseat: t:1\nnote: 任意\n---\n{body}");
        let doc = WmDoc::parse(&legacy).unwrap_or_else(|err| panic!("schema 無しも読む: {err:?}"));
        assert_eq!(doc.seat(), Some("t:1"));
        assert_eq!(doc.user.len(), 1);
        assert!(doc.user.first().is_some_and(is_closed), "完了の行");
        assert_eq!(doc.plan, "次");
        assert_eq!(doc.directives.len(), 1);
        let texts = |doc: &WmDoc| {
            let all: Vec<String> = doc.user.iter().chain(&doc.directives).map(|item| item.text.clone()).collect();
            (doc.front.clone(), all, doc.plan.clone())
        };
        let again = WmDoc::parse(&doc.render()).unwrap_or_else(|err| panic!("render した file も読める: {err:?}"));
        assert_eq!(texts(&again), texts(&doc), "render は字面で round-trip（行番号は組み直しで動く）");
        assert_eq!(WmDoc::parse(&format!("---\nschema: 2\nseat: t\n---\n{body}")), Err(WmError::Schema("2".to_owned())));
        assert_eq!(WmDoc::parse(&format!("---\nschema: 1\n---\n{body}")), Err(WmError::NoSeat));
        assert_eq!(WmDoc::parse(&body), Err(WmError::NoFrontmatter));
    }
}
