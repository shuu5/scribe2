//! 契約表（設計 docs/design/contract-source.md §2 / §3 / §8・ADR-0023 §2.1 / §2.3・SRS FR47 / FR48 / FR54 / FR55）。
//!
//! 契約の正本は設計 doc の**契約表 1 行**である。本 module が持つのは 3 つ:
//!
//! 1. 表を読む**薄い層**: `.md` は `<!-- contracts:begin -->` … `<!-- contracts:end -->` の区間を行走査で抜き
//!    （区間 0 = 表なし・2 つ以上 = 違反）、`.toml` は全文を渡す。本文を読む parser は rules manifest の 1 本
//!    だけである（`[[contract]]` の table 種・ADR-0023 §2.1・ADR-0010 §2.1「第 2 の parser を作らない」）。
//! 2. 行の欄の**正本** [`FIELDS`]（C1）: `<NAME> contracts schema` が tracked な生成物 `contracts/schema.toml` へ
//!    描き、`xtask check` が生成物と正本の列の一致を測る。
//! 3. 表の**検査** [`check_table`]: id の一意・`req` の要件面での実在・`section` の節の実在・verify の形・
//!    `depends` の解決と輪・`touches` の閉包と `surfaces` の外形 pin ⊆ `write-set`（[`super::closure`]）・末尾 `/`
//!    無しの dir・write-set の項目の実在（[`super::declaration::read_write_set`]）・名指しの実在
//!    （[`super::closure::unresolved_names`]）。**全件・行番号付き**で返し 1 件目で止めない（FR18 と同じ「黙って
//!    落とさない」）。intake（契約 (b)）は同じ関数を 1 行に撃つ＝1 実装（C2）。上限の余地（§3）は受付時点の事実
//!    なので CI では撃たない（intake の側・[`super::cli`]）。

use super::closure::{closure, surface_closure, unresolved_names, ClosureError, Source};
use super::declaration::{self, read_write_set, Basis, Ceiling};
use super::refuse::{covered, Refuse};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::name::NAME;
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::{contract_rows, Scalar, TableRow, TableValue};
use std::collections::BTreeSet;
use std::path::Path;

/// 区間の始まりの行（CLAUDE.md の憲法区間と同じ marker 形・行全体が marker の行だけを数える）。
pub const BEGIN: &str = "<!-- contracts:begin -->";

/// 区間の終わりの行。
pub const END: &str = "<!-- contracts:end -->";

/// 契約表を置く設計 doc の dir（repo 相対・直下の `*.md` が `contracts check` の母集団）。
pub const DESIGN_DIR: &str = "docs/design/";

/// この境界の極性: 本便では CI の `contracts check` が行為の後に測って落とす post-hoc（in-loop の側は契約 (b) の
/// intake が同じ関数で担う）。読めない doc・区間・要件面・閉包の入力は違反に倒す（NFR4）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::PostHoc,
    on_failure: OnFailure::FailClosed,
};

/// 欄が必須か任意か。任意の列の「無い」は key の省略で表す（空の配列は rules manifest の reader が断る）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// 必ず書く。
    Required,
    /// 書かなくてよい（無ければ空）。
    Optional,
}

impl Need {
    /// 生成物に出す語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
        }
    }
}

/// 欄の値の形（TOML subset の値のうち契約表が使う 2 つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// 空でない文字列。
    Text,
    /// 文字列の配列。
    List,
}

impl Shape {
    /// 生成物に出す語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::List => "list",
        }
    }
}

/// 行の欄 1 つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// key の字面。
    pub name: &'static str,
    /// 必須か任意か。
    pub need: Need,
    /// 値の形。
    pub shape: Shape,
}

/// 契約表の行の欄の全体（**正本**・宣言順が `contracts schema` の描く順）。rules manifest の `[[contract]]` の
/// key 集合もここから引く。1 項目 1 行で書く（`xtask check` の contracts-schema が字面で読む）。
pub const FIELDS: &[Field] = &[
    Field { name: "id", need: Need::Required, shape: Shape::Text },
    Field { name: "title", need: Need::Required, shape: Shape::Text },
    Field { name: "req", need: Need::Required, shape: Shape::List },
    Field { name: "section", need: Need::Required, shape: Shape::Text },
    Field { name: "touches", need: Need::Optional, shape: Shape::List },
    Field { name: "surfaces", need: Need::Optional, shape: Shape::List },
    Field { name: "write-set", need: Need::Required, shape: Shape::List },
    Field { name: "verify", need: Need::Required, shape: Shape::List },
    Field { name: "size", need: Need::Required, shape: Shape::Text },
    Field { name: "done", need: Need::Required, shape: Shape::Text },
    Field { name: "depends", need: Need::Optional, shape: Shape::List },
    Field { name: "classes", need: Need::Optional, shape: Shape::List },
    Field { name: "opens", need: Need::Optional, shape: Shape::List },
];

/// 契約表の 1 行（欄は [`FIELDS`]・任意の列の「無い」は空）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractRow {
    /// 行の見出し（`[[contract]]`）の doc 上の行番号。
    pub line: u64,
    /// doc 内で一意な行 id。
    pub id: String,
    /// 散文の題。
    pub title: String,
    /// 要件 id の列。
    pub req: Vec<String>,
    /// 同じ doc の節番号（`## N.` の N）。
    pub section: String,
    /// 閉じた型の宣言の列（`crate::module::Type`）。
    pub touches: Vec<String>,
    /// 触る外形の名の列（外形 snapshot の名か usage を持つ subcommand の名・§3 の第 5 形・空 = 外形を触らない）。
    pub surfaces: Vec<String>,
    /// 触ってよい path の列。
    pub write_set: Vec<String>,
    /// positional filter 形の検証行の列。
    pub verify: Vec<String>,
    /// 見積の目安。
    pub size: String,
    /// 散文の終わりの条件（1 行）。
    pub done: String,
    /// 同じ doc の行 id の列（順序）。
    pub depends: Vec<String>,
    /// 3 クラスの自己申告。
    pub classes: Vec<String>,
    /// 契約が開く path 種別の名。
    pub opens: Vec<String>,
}

/// 契約表そのものの欠陥（**各 variant が行番号を持つ**・0 は file 全体）。新しい理由は variant を 1 つ足す（C2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    /// 名指した doc に区間が無い（intake が pointer を解く周。`contracts check` では表なしは違反でない）。
    RegionMissing {
        /// 行番号。
        line: u64,
    },
    /// 区間が 2 つ以上在る（2 つ目の begin の行）。
    RegionDuplicate {
        /// 行番号。
        line: u64,
    },
    /// 読めない（file・拡張子・区間の閉じ・TOML subset の欠陥・欄の形・要件面・閉包の入力）。
    Unreadable {
        /// 行番号。
        line: u64,
        /// 読めない理由。
        reason: String,
    },
    /// 行 id が doc の中で重複する（2 本目の行）。
    DuplicateId {
        /// 行番号。
        line: u64,
        /// 重複した id。
        id: String,
    },
    /// pointer の行 id が区間に無い。
    RowMissing {
        /// 行番号。
        line: u64,
        /// 引いた id。
        id: String,
    },
    /// `section` の節（`## N.` の見出しと非空の本文）が同じ doc に無い。
    SectionMissing {
        /// 行番号。
        line: u64,
        /// 行が名指した節番号。
        section: String,
    },
    /// `req` の id が要件面に無い。
    RequirementMissing {
        /// 行番号。
        line: u64,
        /// 無かった要件 id。
        req: String,
    },
    /// verify 行が positional filter 形でない（allowlist の外・`(` や制御文字・穴）。
    VerifyForm {
        /// 行番号。
        line: u64,
        /// verify 行の字面。
        verify: String,
        /// 撃てない理由（宣言の判定の 1 本が返す字面）。
        reason: String,
    },
    /// `depends` の id が同じ doc に無い。
    DependsUnresolved {
        /// 行番号。
        line: u64,
        /// 解けなかった id。
        id: String,
    },
    /// `depends` が輪を成す（輪の中で doc 順が最初の行・輪の id を doc 順に持つ）。
    DependsCycle {
        /// 行番号。
        line: u64,
        /// 輪を成す行 id。
        cycle: Vec<String>,
    },
    /// `surfaces` の名が外形 snapshot の名にも usage を持つ subcommand の名にも無い（§3 の第 5 形）。
    SurfaceUnknown {
        /// 行番号。
        line: u64,
        /// 書かれていた名。
        name: String,
    },
}

impl TableError {
    /// doc 上の行番号（0 は file 全体）。
    pub fn line(&self) -> u64 {
        match *self {
            Self::RegionMissing { line }
            | Self::RegionDuplicate { line }
            | Self::Unreadable { line, .. }
            | Self::DuplicateId { line, .. }
            | Self::RowMissing { line, .. }
            | Self::SectionMissing { line, .. }
            | Self::RequirementMissing { line, .. }
            | Self::VerifyForm { line, .. }
            | Self::DependsUnresolved { line, .. }
            | Self::DependsCycle { line, .. }
            | Self::SurfaceUnknown { line, .. } => line,
        }
    }

    /// 理由の名（kebab・宣言順は歯が pin する）。
    pub fn as_str(&self) -> &'static str {
        match *self {
            Self::RegionMissing { .. } => "region-missing",
            Self::RegionDuplicate { .. } => "region-duplicate",
            Self::Unreadable { .. } => "unreadable",
            Self::DuplicateId { .. } => "duplicate-id",
            Self::RowMissing { .. } => "row-missing",
            Self::SectionMissing { .. } => "section-missing",
            Self::RequirementMissing { .. } => "requirement-missing",
            Self::VerifyForm { .. } => "verify-form",
            Self::DependsUnresolved { .. } => "depends-unresolved",
            Self::DependsCycle { .. } => "depends-cycle",
            Self::SurfaceUnknown { .. } => "surface-unknown",
        }
    }

    /// 断る理由の 1 行。
    pub fn reason(&self) -> String {
        match *self {
            Self::RegionMissing { .. } => format!("区間（{BEGIN} … {END}）が無い"),
            Self::RegionDuplicate { .. } => "区間が 2 つ以上在る（設計 doc 1 本に区間は 0 か 1 つ）".to_owned(),
            Self::Unreadable { ref reason, .. } => reason.clone(),
            Self::DuplicateId { ref id, .. } => format!("行 id {id} が重複する"),
            Self::RowMissing { ref id, .. } => format!("行 id {id} が区間に無い"),
            Self::SectionMissing { ref section, .. } => {
                format!("section {section} の節（## {section}. の見出しと非空の本文）が同じ doc に無い")
            }
            Self::RequirementMissing { ref req, .. } => format!("req {req} が要件面に無い"),
            Self::VerifyForm { ref verify, ref reason, .. } => {
                format!("verify {verify:?} が positional filter 形でない: {reason}")
            }
            Self::DependsUnresolved { ref id, .. } => format!("depends {id} が同じ doc の行 id に無い"),
            Self::DependsCycle { ref cycle, .. } => {
                let back = cycle.first().map_or_else(String::new, |first| format!(" → {first}"));
                format!("depends が輪を成す（{}{back}）", cycle.join(" → "))
            }
            Self::SurfaceUnknown { ref name, .. } => ClosureError::SurfaceUnknown { name: name.clone() }.reason(),
        }
    }

    /// rc（読めない周だけ 2・残りは前提違反の 1・NFR4）。
    pub fn rc(&self) -> u8 {
        match *self {
            Self::Unreadable { .. } => RC_BROKEN,
            _ => RC_REFUSED,
        }
    }
}

/// 読めない 1 件。
fn unreadable(line: u64, reason: &str) -> TableError {
    TableError::Unreadable { line, reason: reason.to_owned() }
}

/// 検査の 1 件（doc 上の行番号と理由）。理由の語彙は契約単位の拒否 [`Refuse`] と同じ enum である（intake と CI が
/// 同じ字面で名指す・C2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// doc 上の行番号（0 は file 全体）。
    pub line: u64,
    /// 理由。
    refuse: Refuse,
}

impl Finding {
    /// 契約表そのものの欠陥の 1 件。
    fn table(error: TableError) -> Self {
        Self { line: error.line(), refuse: Refuse::ContractTable(error) }
    }

    /// `contracts: <doc>:<line> <理由の名>: <理由>` の 1 行。
    pub fn render(&self, doc: &str) -> String {
        format!("contracts: {doc}:{} {}: {}", self.line, self.refuse.label(), self.refuse.reason())
    }

    /// rc（読めない周は 2・残りは 1）。
    pub fn rc(&self) -> u8 {
        self.refuse.rc()
    }
}

/// 表の検査の文脈（repo の側の事実・I/O は呼び手が済ませて渡す）。
pub struct Context<'a> {
    /// 宣言の allowlist（verify 行の先頭語の基準）。
    pub allowed: &'a [String],
    /// 禁じる語列（rules 行 `runner.denied_commands`・verify 行に intake と同じ判定を掛ける・ADR-0025 §2.3）。
    pub denied: &'a [String],
    /// 要件面の id の集合（読めない周は理由）。
    pub requirements: &'a Result<BTreeSet<String>, String>,
    /// 閉包を測る `.rs` の列。
    pub sources: &'a [Source],
    /// tracked file の repo 相対 path（write-set の項目の実在・dir の判定・path 形の名指しに使う）。
    pub tracked: &'a [String],
    /// 外形 snapshot（tracked の `.snap`・`surfaces` の外形 pin に使う）。
    pub snapshots: &'a [Source],
}

/// 契約表の置き場の形（path の拡張子で決める）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// 設計 doc（`.md`）の区間。
    Region,
    /// 導出された `.toml` の全文（folio2 M1 の形）。
    Whole,
}

/// path の拡張子から置き場の形を決める（`.md` = 区間 / `.toml` = 全文・他は typed に断る）。
pub fn form_of(path: &str) -> Result<Form, TableError> {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("md") => Ok(Form::Region),
        Some("toml") => Ok(Form::Whole),
        _ => Err(unreadable(0, &format!("{path} は契約表の置き場の形でない（.md = 区間 / .toml = 全文）"))),
    }
}

/// `.md` の区間（begin の行番号・本文）。区間が無ければ `None`。2 つ以上・閉じない・end だけは `Err`。
fn region(text: &str) -> Result<Option<(u64, String)>, TableError> {
    let (mut open, mut found, mut body) = (None, None, String::new());
    for (index, line) in text.lines().enumerate() {
        let at = (index as u64).saturating_add(1);
        match (line.trim(), open) {
            (BEGIN, Some(_)) => return Err(TableError::RegionDuplicate { line: at }),
            (BEGIN, None) if found.is_some() => return Err(TableError::RegionDuplicate { line: at }),
            (BEGIN, None) => open = Some(at),
            (END, Some(start)) => {
                found = Some((start, std::mem::take(&mut body)));
                open = None;
            }
            (END, None) => return Err(unreadable(at, "区間の end の前に begin が無い")),
            (_, Some(_)) => {
                body.push_str(line);
                body.push('\n');
            }
            (_, None) => {}
        }
    }
    match open {
        Some(start) => Err(unreadable(start, "区間が閉じていない（end が無い）")),
        None => Ok(found),
    }
}

/// 区間の本文の行番号を doc の行番号へ写す（0 = 本文全体 → 区間の始まりの行）。
fn shift(offset: u64, line: u64) -> u64 {
    if line == 0 {
        offset
    } else {
        offset.saturating_add(line)
    }
}

/// 契約表の置き場 1 本を読む（`.md` = 区間 / `.toml` = 全文）。区間の無い `.md` は 0 行（表なしは違反でない）。
/// 欠陥は**全件**・doc 上の行番号付きで返す。
pub fn read_rows(path: &str, text: &str) -> Result<Vec<ContractRow>, Vec<TableError>> {
    let (offset, body) = match form_of(path).map_err(|found| vec![found])? {
        Form::Whole => (0, text.to_owned()),
        Form::Region => match region(text).map_err(|found| vec![found])? {
            Some(found) => found,
            None => return Ok(Vec::new()),
        },
    };
    let raws = contract_rows(&body).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| unreadable(shift(offset, error.line), &error.message))
            .collect::<Vec<TableError>>()
    })?;
    let mut errors = Vec::new();
    let rows: Vec<ContractRow> = raws.iter().filter_map(|raw| typed(raw, offset, &mut errors)).collect();
    if errors.is_empty() {
        Ok(rows)
    } else {
        Err(errors)
    }
}

/// 読めた 1 行を欄の形（[`FIELDS`] の `shape`）で型付けする。形の違いは欄の行番号で積む。
fn typed(raw: &TableRow, offset: u64, errors: &mut Vec<TableError>) -> Option<ContractRow> {
    let before = errors.len();
    let row = ContractRow {
        line: shift(offset, raw.line()),
        id: text_of(raw, "id", offset, errors),
        title: text_of(raw, "title", offset, errors),
        req: list_of(raw, "req", offset, errors),
        section: text_of(raw, "section", offset, errors),
        touches: list_of(raw, "touches", offset, errors),
        surfaces: list_of(raw, "surfaces", offset, errors),
        write_set: list_of(raw, "write-set", offset, errors),
        verify: list_of(raw, "verify", offset, errors),
        size: text_of(raw, "size", offset, errors),
        done: text_of(raw, "done", offset, errors),
        depends: list_of(raw, "depends", offset, errors),
        classes: list_of(raw, "classes", offset, errors),
        opens: list_of(raw, "opens", offset, errors),
    };
    (errors.len() == before).then_some(row)
}

/// 文字列の欄（無ければ空・形が違えば積む）。
fn text_of(raw: &TableRow, key: &str, offset: u64, errors: &mut Vec<TableError>) -> String {
    match raw.value(key) {
        None => String::new(),
        Some((TableValue::One(Scalar::Str(found)), _)) if !found.trim().is_empty() => found.clone(),
        Some((_, line)) => {
            errors.push(unreadable(shift(offset, line), &format!("{key} は空でない文字列でなければならない")));
            String::new()
        }
    }
}

/// 配列の欄（無ければ空・形が違えば積む）。
fn list_of(raw: &TableRow, key: &str, offset: u64, errors: &mut Vec<TableError>) -> Vec<String> {
    match raw.value(key) {
        None => Vec::new(),
        Some((TableValue::List(items), _)) => items.clone(),
        Some((_, line)) => {
            errors.push(unreadable(shift(offset, line), &format!("{key} は文字列の配列でなければならない")));
            Vec::new()
        }
    }
}

/// 設計 pointer の行を 1 つ引く（intake が 1 行に撃つ口・契約 (b)）。区間の無い `.md` は
/// [`TableError::RegionMissing`]・id が無ければ [`TableError::RowMissing`]。
pub fn find_row(path: &str, text: &str, id: &str) -> Result<ContractRow, Vec<TableError>> {
    if form_of(path) == Ok(Form::Region) && region(text) == Ok(None) {
        return Err(vec![TableError::RegionMissing { line: 0 }]);
    }
    read_rows(path, text)?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or_else(|| vec![TableError::RowMissing { line: 0, id: id.to_owned() }])
}

/// 設計 pointer（`docs/design/<題>.md#<id>`）を読んだもの。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pointer {
    /// 置き場の repo 相対 path。
    pub path: String,
    /// 行 id。
    pub id: String,
}

/// 設計 pointer を断る理由（FR54）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerError {
    /// `#` がちょうど 1 つでない。
    Hash,
    /// 置き場の拡張子（`.md` / `.toml`）でない。
    Extension,
    /// 行 id が空か空白を含む。
    Id,
}

impl PointerError {
    /// 断る理由の 1 行。
    pub fn reason(self) -> &'static str {
        match self {
            Self::Hash => "設計 pointer は <path>#<id> の形（# はちょうど 1 つ）",
            Self::Extension => "設計 pointer の path は契約表の置き場（.md の区間 / .toml の全文）",
            Self::Id => "設計 pointer の行 id が空か空白を含む",
        }
    }
}

/// 設計 pointer を読む（拡張子・`#` の 1 つ・空でない id）。
pub fn parse_pointer(text: &str) -> Result<Pointer, PointerError> {
    let mut parts = text.split('#');
    let (Some(path), Some(id), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(PointerError::Hash);
    };
    if form_of(path).is_err() || doc_id(path).is_empty() {
        return Err(PointerError::Extension);
    }
    if id.is_empty() || id.chars().any(char::is_whitespace) {
        return Err(PointerError::Id);
    }
    Ok(Pointer { path: path.to_owned(), id: id.to_owned() })
}

/// 契約 id（`<doc の file 名の stem>#<行 id>`・folio2 の設計ノートと同じ形・doc id は append-only）。
pub fn contract_id(path: &str, id: &str) -> String {
    format!("{}#{id}", doc_id(path))
}

/// doc id（file 名の stem）。
fn doc_id(path: &str) -> String {
    Path::new(path).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default()
}

/// 表の全行を検査する（**全件・行番号の順**・同じ行の中は検査の順）。intake（契約 (b)）は同じ関数を 1 行に撃つ。
pub fn check_table(doc: &str, rows: &[ContractRow], ctx: &Context<'_>) -> Vec<Finding> {
    let numbered = sections(doc);
    let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    let mut found = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if ids.iter().take(index).any(|seen| *seen == row.id) {
            found.push(Finding::table(TableError::DuplicateId { line: row.line, id: row.id.clone() }));
        }
        if !numbered.iter().any(|(number, filled)| *filled && number.as_deref() == Some(row.section.as_str())) {
            found.push(Finding::table(TableError::SectionMissing { line: row.line, section: row.section.clone() }));
        }
        found.extend(requirement_findings(row, ctx.requirements));
        found.extend(verify_findings(row, &Basis { allowed: ctx.allowed, denied: ctx.denied }));
        let unresolved = row.depends.iter().filter(|id| !ids.contains(&id.as_str()));
        found.extend(unresolved.map(|id| Finding::table(TableError::DependsUnresolved { line: row.line, id: id.clone() })));
        found.extend(write_set_findings(row, ctx));
        // 閉包・外形 pin・名指しは `.rs` / `.snap` の本文を読む。1 本でも読めなければ行ごとに 1 件で名指し、
        // 測れない検査は撃たない（読めなさを「足りない file なし」に読み替えない・NFR4）。
        match unreadable_input(ctx) {
            Some(reason) => found.push(Finding::table(unreadable(row.line, &reason))),
            None => {
                found.extend(closure_findings(row, ctx));
                found.extend(name_findings(doc, row, ctx));
            }
        }
    }
    found.extend(cycle_findings(rows));
    found.sort_by_key(|finding| finding.line);
    found
}

/// 節 `number` の本文の (doc 上の行番号, 行)（見出しの次の行から次の `## ` 見出しの前まで・区間と fence の中は除く）。
fn section_lines(text: &str, number: &str) -> Vec<(u64, String)> {
    let mut found = Vec::new();
    let (mut fenced, mut inside, mut open) = (false, false, false);
    for (index, line) in text.lines().enumerate() {
        match line.trim() {
            BEGIN => inside = true,
            END => inside = false,
            _ if inside => {}
            trimmed => {
                if trimmed.starts_with("```") {
                    fenced = !fenced;
                    continue;
                }
                match line.strip_prefix("## ").filter(|_| !fenced) {
                    Some(title) => open = number_of(title).as_deref() == Some(number),
                    None if open && !fenced => found.push(((index as u64).saturating_add(1), line.to_owned())),
                    None => {}
                }
            }
        }
    }
    found
}

/// 名指しの実在（§3）: `title` / `done` と `section` の本文の backtick の中身のうち解けないものを全件（在り処付き）。
/// 閉包の入力を読めない周は `unreadable`（黙って通さない）。
fn name_findings(doc: &str, row: &ContractRow, ctx: &Context<'_>) -> Vec<Finding> {
    let mut texts = vec![("title".to_owned(), row.title.clone()), ("done".to_owned(), row.done.clone())];
    texts.extend(
        section_lines(doc, &row.section).into_iter().map(|(at, line)| (format!("section {} line {at}", row.section), line)),
    );
    match unresolved_names(&texts, &row.touches, &row.write_set, ctx.tracked, ctx.sources) {
        Err(error) => vec![Finding::table(unreadable(row.line, &error.reason()))],
        Ok(names) => names
            .into_iter()
            .map(|(name, at)| Finding { line: row.line, refuse: Refuse::NameUnresolved { name, at } })
            .collect(),
    }
}

/// doc の `## ` 見出しの節番号（`## N.` の N・番号の無い見出しは `None`）と、本文が非空か。区間と code fence の
/// 中の行は見出しに数えず、区間の行は本文にも数えない。
fn sections(text: &str) -> Vec<(Option<String>, bool)> {
    let mut found: Vec<(Option<String>, bool)> = Vec::new();
    let (mut fenced, mut inside) = (false, false);
    for line in text.lines() {
        match line.trim() {
            BEGIN => inside = true,
            END => inside = false,
            _ if inside => {}
            trimmed => {
                if trimmed.starts_with("```") {
                    fenced = !fenced;
                }
                match line.strip_prefix("## ").filter(|_| !fenced) {
                    Some(title) => found.push((number_of(title), false)),
                    None => {
                        if let Some(last) = found.last_mut() {
                            last.1 |= !trimmed.is_empty();
                        }
                    }
                }
            }
        }
    }
    found
}

/// 見出しの字面の先頭の `N.` の N（数字だけ）。
fn number_of(title: &str) -> Option<String> {
    let (head, _) = title.split_once('.')?;
    (!head.is_empty() && head.chars().all(|found| found.is_ascii_digit())).then(|| head.to_owned())
}

/// `req` の各 id が要件面に在るか（要件面を読めない周は行ごとに 1 件・黙って通さない）。
fn requirement_findings(row: &ContractRow, requirements: &Result<BTreeSet<String>, String>) -> Vec<Finding> {
    match *requirements {
        Err(ref reason) => vec![Finding::table(unreadable(row.line, reason))],
        Ok(ref known) => row
            .req
            .iter()
            .filter(|req| !known.contains(*req))
            .map(|req| Finding::table(TableError::RequirementMissing { line: row.line, req: req.clone() }))
            .collect(),
    }
}

/// verify 行の形（判定は宣言の 1 本 [`declaration::verify_unfit`]・契約の行は穴を持てず、禁じる語列にも当たれない）。
fn verify_findings(row: &ContractRow, basis: &Basis<'_>) -> Vec<Finding> {
    row.verify
        .iter()
        .filter_map(|line| {
            let reason = declaration::verify_unfit(line, basis)?;
            Some(Finding::table(TableError::VerifyForm { line: row.line, verify: line.clone(), reason }))
        })
        .collect()
}

/// write-set の項目の 2 検査（base の tracked file だけで測る・§3「項目の実在と展開」）: 末尾 `/` 無しで tracked な
/// dir を指す項目と、base に解けない項目（末尾 `/` 無しの dir として既に名指した項目は重ねて名指さない）。
fn write_set_findings(row: &ContractRow, ctx: &Context<'_>) -> Vec<Finding> {
    let without_slash: Vec<&String> = row
        .write_set
        .iter()
        .filter(|item| !item.ends_with('/') && ctx.tracked.iter().any(|path| declaration::is_under(path, item)))
        .collect();
    let mut found: Vec<Finding> = without_slash
        .iter()
        .map(|item| Finding { line: row.line, refuse: Refuse::WriteSetDirWithoutSlash { path: (*item).clone() } })
        .collect();
    if let Err(items) = read_write_set(&row.write_set, ctx.tracked) {
        found.extend(
            items
                .into_iter()
                .filter(|item| !without_slash.contains(&item))
                .map(|item| Finding { line: row.line, refuse: Refuse::WriteSetItemUnresolved { item } }),
        );
    }
    found
}

/// 閉包の入力（`.rs` と `.snap`）のうち読めない 1 本の理由（全部読めれば `None`・行ごとに 1 件で名指す材料）。
fn unreadable_input(ctx: &Context<'_>) -> Option<String> {
    ctx.sources.iter().chain(ctx.snapshots).find_map(|source| match source.body {
        Ok(_) => None,
        Err(ref reason) => {
            Some(ClosureError::Unreadable { path: source.path.clone(), reason: reason.clone() }.reason())
        }
    })
}

/// `touches` の閉包と `surfaces` の外形 pin（§3 の 4 形 + 第 5 形）のうち write-set に無い file を 1 件に全部。
/// 未知の外形の名は [`TableError::SurfaceUnknown`]。読めない入力は呼び手が先に除く。
fn closure_findings(row: &ContractRow, ctx: &Context<'_>) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut files = BTreeSet::new();
    match closure(&row.touches, ctx.sources) {
        Ok(paths) => files.extend(paths),
        Err(error) => found.push(Finding::table(unreadable(row.line, &error.reason()))),
    }
    match surface_closure(&row.surfaces, ctx.sources, ctx.snapshots) {
        Ok(paths) => files.extend(paths),
        Err(ClosureError::SurfaceUnknown { name }) => {
            found.push(Finding::table(TableError::SurfaceUnknown { line: row.line, name }));
        }
        Err(error) => found.push(Finding::table(unreadable(row.line, &error.reason()))),
    }
    let missing: Vec<String> = files.into_iter().filter(|path| !covered(&row.write_set, path)).collect();
    if !missing.is_empty() {
        found.push(Finding { line: row.line, refuse: Refuse::WriteSetIncomplete { missing } });
    }
    found
}

/// `depends` の輪（輪 1 つにつき 1 件・輪の中で doc 順が最初の行に置く）。解けない id は辿らない（別の 1 件）。
fn cycle_findings(rows: &[ContractRow]) -> Vec<Finding> {
    let reach: Vec<BTreeSet<&str>> = rows.iter().map(|row| reachable(rows, row)).collect();
    let mut named: BTreeSet<&str> = BTreeSet::new();
    let mut found = Vec::new();
    for (row, from) in rows.iter().zip(&reach) {
        if named.contains(row.id.as_str()) || !from.contains(row.id.as_str()) {
            continue;
        }
        let members: Vec<&str> = rows
            .iter()
            .zip(&reach)
            .filter(|(other, back)| from.contains(other.id.as_str()) && back.contains(row.id.as_str()))
            .map(|(other, _)| other.id.as_str())
            .collect();
        named.extend(members.iter().copied());
        let cycle = members.iter().map(|id| (*id).to_owned()).collect();
        found.push(Finding::table(TableError::DependsCycle { line: row.line, cycle }));
    }
    found
}

/// `start` から `depends` を 1 本以上辿って届く id の集合。
fn reachable<'r>(rows: &'r [ContractRow], start: &'r ContractRow) -> BTreeSet<&'r str> {
    let mut found: BTreeSet<&str> = BTreeSet::new();
    let mut stack: Vec<&str> = start.depends.iter().map(String::as_str).collect();
    while let Some(id) = stack.pop() {
        if !found.insert(id) {
            continue;
        }
        if let Some(next) = rows.iter().find(|row| row.id == id) {
            stack.extend(next.depends.iter().map(String::as_str));
        }
    }
    found
}

/// 要件面の id の集合（`.html` = `id="…"` の anchor・`.yaml` / `.yml` = 要件 id の列）。他の拡張子は読まない。
pub fn requirement_ids(path: &str, text: &str) -> Result<BTreeSet<String>, String> {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("html") => Ok(anchors(text)),
        Some("yaml" | "yml") => Ok(text.lines().filter_map(yaml_id).collect()),
        _ => Err(format!("要件面 {path} の形を読めない（.html の anchor / .yaml の列だけ）")),
    }
}

/// `id="…"` / `id='…'` の値のうち要件 id の形のもの。
fn anchors(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find("id=") {
        let tail = rest.get(at.saturating_add(3)..).unwrap_or_default();
        let value = ['"', '\''].iter().find_map(|quote| {
            let body = tail.strip_prefix(*quote)?;
            body.get(..body.find(*quote)?)
        });
        if let Some(id) = value.filter(|id| is_requirement(id)) {
            found.insert(id.to_owned());
        }
        rest = tail;
    }
    found
}

/// yaml の 1 行（`- FR1` / `- "FR1"` / `id: FR1` / `- id: FR1`）から要件 id を取る。
fn yaml_id(line: &str) -> Option<String> {
    let item = line.trim();
    let item = item.strip_prefix('-').map_or(item, str::trim_start);
    let item = item.strip_prefix("id:").map_or(item, str::trim_start);
    let item = item.trim().trim_matches(|found: char| found == '"' || found == '\'');
    is_requirement(item).then(|| item.to_owned())
}

/// 要件 id の形（英大文字の列 + 数字の列・`FR47` / `NFR4` / `AC21`）。
fn is_requirement(text: &str) -> bool {
    let letters = text.trim_end_matches(|found: char| found.is_ascii_digit());
    !letters.is_empty() && letters.len() < text.len() && letters.chars().all(|found| found.is_ascii_uppercase())
}

/// `<NAME> contracts schema` の全出力（tracked な生成物 `contracts/schema.toml` の本文・1 行ずつ）。
pub fn render_schema() -> Vec<String> {
    let mut lines = vec![
        format!("# 契約表の行の欄（生成物: `{NAME} contracts schema` の出力・手で直さない・正本は core の pipe/table.rs の FIELDS）"),
        "schema = 1".to_owned(),
    ];
    for field in FIELDS {
        lines.extend([
            String::new(),
            "[[field]]".to_owned(),
            format!("name = \"{}\"", field.name),
            format!("need = \"{}\"", field.need.as_str()),
            format!("shape = \"{}\"", field.shape.as_str()),
        ]);
    }
    lines
}

/// `<NAME> contracts check --repo R` の本体（設計 §2「表の検査」・FR55）: tracked な `docs/design/*.md` の区間を
/// 全行検査し、findings を `contracts: <file>:<line> …` の 1 行ずつ、末尾に判定行を stdout へ出す。
///
/// rc = 違反 0 → 0 / 違反 ≥ 1 → 1 / 読めない周 → 2（読めない doc・区間・要件面・閉包の入力も 1 件として名指し、
/// 判定行も出す）。tracked file の一覧か宣言を読めない周は判定できないので、理由だけを stderr へ出して rc 2。
pub(crate) fn check_repo(repo: &Path, ceiling: &Ceiling<'_>) -> Outcome {
    let Some(tracked) = tracked_files(repo) else {
        let reason = format!("contracts: {} の tracked file を読めない（git repo でない）", repo.display());
        return Outcome::failed_line(RC_BROKEN, reason);
    };
    let facts = match declaration::table_facts(repo, ceiling) {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect()),
    };
    let sources = read_all(repo, &tracked, ".rs");
    let snapshots = read_all(repo, &tracked, ".snap");
    let requirements = read(repo, &facts.requirements).and_then(|text| requirement_ids(&facts.requirements, &text));
    let ctx = Context {
        allowed: &facts.allowed,
        denied: &facts.denied,
        requirements: &requirements,
        sources: &sources,
        tracked: &tracked,
        snapshots: &snapshots,
    };
    let docs: Vec<&String> = tracked
        .iter()
        .filter(|path| path.strip_prefix(DESIGN_DIR).is_some_and(|rest| !rest.contains('/') && rest.ends_with(".md")))
        .collect();
    let (mut out, mut rows, mut findings, mut rc) = (Vec::new(), 0_usize, 0_usize, RC_OK);
    for doc in &docs {
        let (count, found) = judge_doc(repo, doc, &ctx);
        rows = rows.saturating_add(count);
        findings = findings.saturating_add(found.len());
        rc = found.iter().map(Finding::rc).fold(rc, u8::max);
        out.extend(found.iter().map(|finding| finding.render(doc)));
    }
    out.push(format!("contracts check: docs={} rows={rows} findings={findings}", docs.len()));
    Outcome { out, err: Vec::new(), rc }
}

/// doc 1 本の行数と findings（読めない doc・区間は 1 件ずつ名指す）。
fn judge_doc(repo: &Path, doc: &str, ctx: &Context<'_>) -> (usize, Vec<Finding>) {
    let text = match read(repo, doc) {
        Ok(found) => found,
        Err(reason) => return (0, vec![Finding::table(unreadable(0, &reason))]),
    };
    match read_rows(doc, &text) {
        Ok(rows) => (rows.len(), check_table(&text, &rows, ctx)),
        Err(errors) => (0, errors.into_iter().map(Finding::table).collect()),
    }
}

/// repo 相対の file を読む（読めない理由は path を名乗る 1 行）。
fn read(repo: &Path, path: &str) -> Result<String, String> {
    std::fs::read_to_string(repo.join(path)).map_err(|err| format!("{path} を読めない: {err}"))
}

/// tracked file の repo 相対 path（`git ls-files -z`・git repo でなければ `None`）。`contracts check` と intake の
/// 上限の余地・交差の展開が同じ一覧を読む。
pub(crate) fn tracked_files(repo: &Path) -> Option<Vec<String>> {
    let listed = super::git_bytes(repo, &["ls-files", "-z"])?;
    Some(String::from_utf8_lossy(&listed).split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect())
}

/// tracked のうち拡張子 `ext` の file を全部読む（読めない周は理由を持つ・黙って落とさない）。
pub(crate) fn read_all(repo: &Path, tracked: &[String], ext: &str) -> Vec<Source> {
    tracked
        .iter()
        .filter(|path| path.ends_with(ext))
        .map(|path| Source { path: path.clone(), body: read(repo, path) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        check_table, contract_id, find_row, parse_pointer, read_rows, render_schema, requirement_ids, Context,
        ContractRow, Need, Pointer, PointerError, Shape, TableError, BEGIN, END, FIELDS,
    };
    use crate::cli_outcome::{RC_BROKEN, RC_REFUSED};
    use crate::pipe::closure::Source;
    use crate::pipe::refuse::Refuse;
    use std::collections::BTreeSet;

    /// [`TableError`] の全 variant の名（宣言順）。payload 付きの enum は `as` で判別子へ写せないので、名前の slice
    /// と `as_str` の網羅 match を対にして宣言順を pin する（`pipe::refuse::REFUSALS` と同じ形）。
    const TABLE_ERRORS: &[&str] = &[
        "region-missing",
        "region-duplicate",
        "unreadable",
        "duplicate-id",
        "row-missing",
        "section-missing",
        "requirement-missing",
        "verify-form",
        "depends-unresolved",
        "depends-cycle",
        "surface-unknown",
    ];

    /// 宣言順に 1 つずつ組んだ全 variant（行番号は 1 から順）。
    fn samples() -> Vec<TableError> {
        let text = |value: &str| value.to_owned();
        vec![
            TableError::RegionMissing { line: 1 },
            TableError::RegionDuplicate { line: 2 },
            TableError::Unreadable { line: 3, reason: text("r") },
            TableError::DuplicateId { line: 4, id: text("a") },
            TableError::RowMissing { line: 5, id: text("a") },
            TableError::SectionMissing { line: 6, section: text("9") },
            TableError::RequirementMissing { line: 7, req: text("FR9") },
            TableError::VerifyForm { line: 8, verify: text("v"), reason: text("r") },
            TableError::DependsUnresolved { line: 9, id: text("z") },
            TableError::DependsCycle { line: 10, cycle: vec![text("a"), text("b")] },
            TableError::SurfaceUnknown { line: 11, name: text("nope_external_form") },
        ]
    }

    /// 名前の slice は宣言順で `as_str` と 1 対 1・各 variant は行番号と 1 行の理由を持ち、読めない周だけ rc 2。
    #[test]
    fn table_error_names_are_pinned_in_declaration_order_and_carry_their_line() {
        let found = samples();
        let names: Vec<&str> = found.iter().map(TableError::as_str).collect();
        assert_eq!(names, TABLE_ERRORS, "名前の slice は宣言順（母集団 {} 値）", TABLE_ERRORS.len());
        for (index, error) in found.iter().enumerate() {
            assert_eq!(error.line(), index as u64 + 1, "{} は行番号を持つ", error.as_str());
            assert!(!error.reason().is_empty() && !error.reason().contains('\n'), "{} の理由は 1 行", error.as_str());
            let want = if matches!(error, TableError::Unreadable { .. }) { RC_BROKEN } else { RC_REFUSED };
            assert_eq!(error.rc(), want, "{} の rc", error.as_str());
        }
        let cycle = found.get(9).map(TableError::reason).unwrap_or_default();
        assert!(cycle.contains("a → b → a"), "輪は id を順に名乗り最初へ戻る: {cycle}");
        let surface = found.last().map(TableError::reason).unwrap_or_default();
        assert!(surface.contains("nope_external_form"), "未知の外形の名を名乗る: {surface}");
    }

    /// 全欄を持つ `.toml` の 1 行（`over` の欄だけ値を差し替える）。
    fn full_row(over: &[(&str, &str)]) -> String {
        let mut text = "schema = 1\n\n[[contract]]\n".to_owned();
        for field in FIELDS {
            let default = match field.shape {
                Shape::Text if field.name == "section" => "\"1\"".to_owned(),
                Shape::Text => "\"v\"".to_owned(),
                Shape::List => "[\"v\"]".to_owned(),
            };
            let value = over.iter().find(|(name, _)| *name == field.name).map_or(default, |(_, found)| (*found).to_owned());
            text.push_str(&format!("{} = {value}\n", field.name));
        }
        text
    }

    /// 欄の列は宣言順に 13（必須 8・任意 5）で、`contracts schema` はその順に描く。欄の形は reader が強制する
    /// （文字列の欄に配列・配列の欄に文字列を書くと、その欄を名指して断る）。
    #[test]
    fn table_fields_pin_the_schema_columns_and_the_reader_enforces_their_shapes() {
        let names: Vec<&str> = FIELDS.iter().map(|field| field.name).collect();
        let want = [
            "id", "title", "req", "section", "touches", "surfaces", "write-set", "verify", "size", "done", "depends",
            "classes", "opens",
        ];
        assert_eq!(names, want, "欄の宣言順");
        assert_eq!(FIELDS.iter().filter(|field| field.need == Need::Required).count(), 8, "必須 8・任意 5");
        let rendered = render_schema();
        let listed: Vec<&str> =
            rendered.iter().filter_map(|line| line.strip_prefix("name = \"")?.strip_suffix('"')).collect();
        assert_eq!(listed, names, "生成物は欄の宣言順");
        assert_eq!(rendered.get(1).map(String::as_str), Some("schema = 1"), "生成物も schema = 1 を持つ");
        assert_eq!(read_rows("t.toml", &full_row(&[])).map(|rows| rows.len()), Ok(1), "全欄の行は読める");
        for field in FIELDS {
            let wrong = match field.shape {
                Shape::Text => "[\"x\"]",
                Shape::List => "\"x\"",
            };
            let errors = read_rows("t.toml", &full_row(&[(field.name, wrong)])).expect_err("形の違う欄は断る");
            let named = errors.iter().any(|error| error.reason().starts_with(&format!("{} は", field.name)));
            assert!(named, "{} の形を名指す: {errors:?}", field.name);
        }
    }

    /// `.md` は区間を行走査で抜き（0 = 表なし・2 つ以上・閉じない・end だけは断る）、`.toml` は全文、他の拡張子は
    /// 断る。行番号は doc 上の行（区間の中の行ではない）。TOML subset の欠陥は全件。
    #[test]
    fn table_region_reads_zero_or_one_region_and_maps_lines_to_the_doc() {
        let row = full_row(&[]);
        let doc = format!("# t\n\n{BEGIN}\n{row}{END}\n");
        let rows = read_rows("docs/design/t.md", &doc).expect("区間 1 つは読める");
        assert_eq!(rows.iter().map(|found| found.line).collect::<Vec<u64>>(), vec![6], "見出しの doc 上の行");
        assert_eq!(read_rows("docs/design/t.md", "# t\n本文\n"), Ok(Vec::new()), "区間の無い doc は 0 行");
        let second = doc.lines().count() as u64 + 1;
        let twice = format!("{doc}{BEGIN}\n{row}{END}\n");
        assert_eq!(read_rows("docs/design/t.md", &twice), Err(vec![TableError::RegionDuplicate { line: second }]));
        let unclosed = read_rows("docs/design/t.md", &format!("# t\n{BEGIN}\n{row}")).expect_err("閉じない区間");
        assert!(matches!(unclosed.as_slice(), [TableError::Unreadable { line: 2, .. }]), "{unclosed:?}");
        let stray = read_rows("docs/design/t.md", &format!("# t\n{END}\n")).expect_err("end だけ");
        assert!(matches!(stray.as_slice(), [TableError::Unreadable { line: 2, .. }]), "{stray:?}");
        let other = read_rows("t.txt", &row).expect_err("他の拡張子");
        assert!(matches!(other.as_slice(), [TableError::Unreadable { line: 0, .. }]), "{other:?}");
        let broken_row = full_row(&[("depends", "[]")]).replace("[[contract]]\n", "[[contract]]\ncolor = \"red\"\n");
        let broken = read_rows("docs/design/t.md", &format!("# t\n\n{BEGIN}\n{broken_row}{END}\n")).expect_err("欠陥");
        assert_eq!(broken.len(), 2, "未知 key と空の配列の 2 件: {broken:?}");
        assert!(broken.iter().all(|error| error.line() > 3), "doc 上の行番号: {broken:?}");
    }

    /// pointer は `#` ちょうど 1 つ・置き場の拡張子・空白の無い id を要り、契約 id は `<stem>#<id>`。区間の無い doc と
    /// 区間に無い id は typed に断る。
    #[test]
    fn table_pointer_and_row_lookup_refuse_malformed_and_missing_targets() {
        let path = "docs/design/contract-source.md";
        assert_eq!(parse_pointer(&format!("{path}#a")), Ok(Pointer { path: path.to_owned(), id: "a".to_owned() }));
        assert_eq!(contract_id(path, "a"), "contract-source#a");
        for (text, want) in [
            ("docs/design/x.md", PointerError::Hash),
            ("docs/design/x.md#a#b", PointerError::Hash),
            ("docs/design/x.txt#a", PointerError::Extension),
            ("#a", PointerError::Extension),
            ("docs/design/x.md#", PointerError::Id),
            ("docs/design/x.md#a b", PointerError::Id),
        ] {
            assert_eq!(parse_pointer(text), Err(want), "{text}: {}", want.reason());
        }
        let doc = format!("# t\n\n{BEGIN}\n{}{END}\n", full_row(&[("id", "\"a\"")]));
        assert_eq!(find_row("docs/design/t.md", &doc, "a").map(|row| row.id), Ok("a".to_owned()));
        assert_eq!(find_row("docs/design/t.md", &doc, "b"), Err(vec![TableError::RowMissing { line: 0, id: "b".to_owned() }]));
        assert_eq!(find_row("docs/design/t.md", "# t\n", "a"), Err(vec![TableError::RegionMissing { line: 0 }]));
    }

    /// 要件面は拡張子で読み手を分ける（`.html` の要件 id の形の anchor・`.yaml` の列）。他の形は読まない。
    #[test]
    fn table_requirement_faces_read_html_anchors_and_yaml_lists_by_extension() {
        let set = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<BTreeSet<String>>();
        let html = "<p id=\"FR47\">x</p><p id='NFR4'>y</p><div id=\"toc\">z</div>";
        assert_eq!(requirement_ids("spec/srs.html", html), Ok(set(&["FR47", "NFR4"])), "要件 id の形の anchor だけ");
        let yaml = "requirements:\n  - FR1\n  - \"AC2\"\n  - id: FR3\n";
        assert_eq!(requirement_ids("spec/reqs.yaml", yaml), Ok(set(&["AC2", "FR1", "FR3"])));
        assert!(requirement_ids("spec/reqs.json", "{}").is_err(), "他の拡張子は読まない");
    }

    /// 検査に要る欄だけを選ぶ 1 行（残りは適合する既定）。
    fn row(line: u64, id: &str) -> ContractRow {
        let one = |value: &str| vec![value.to_owned()];
        ContractRow {
            line,
            id: id.to_owned(),
            title: "t".to_owned(),
            req: one("FR1"),
            section: "1".to_owned(),
            touches: Vec::new(),
            surfaces: Vec::new(),
            write_set: one("src/kind.rs"),
            verify: one("git status"),
            size: "S".to_owned(),
            done: "d".to_owned(),
            depends: Vec::new(),
            classes: Vec::new(),
            opens: Vec::new(),
        }
    }

    /// 節の fixture（§1 = 本文あり / §2 = 本文なし / fence の中の `## 4.` は見出しでない）。
    const DOC: &str = "# t\n\n## 1. 本文の在る節\n\n本文。\n\n## 2. 空の節\n\n## 3. fence\n\n```\n## 4. 見出しではない\n```\n";

    /// 閉包の fixture（`crate::kind::Kind` の宣言 file と arm を持つ file）。
    fn sources() -> Vec<Source> {
        let source = |path: &str, body: &str| Source { path: path.to_owned(), body: Ok(body.to_owned()) };
        vec![
            source("src/kind.rs", "pub enum Kind {\n    A,\n}\n\npub const KINDS: &[Kind] = &[Kind::A];\n"),
            source("src/use.rs", "fn f(kind: Kind) -> u8 {\n    match kind {\n        Kind::A => 1,\n    }\n}\n"),
        ]
    }

    /// 欠陥を 1 つずつ持つ行を、行番号の順に**全件**名指す（1 件目で止めない・適合する行は名指さない）。
    #[test]
    fn table_check_names_every_defect_with_its_row_line() {
        let requirements = Ok(["FR1".to_owned()].into_iter().collect::<BTreeSet<String>>());
        let (allowed, sources) = (["git".to_owned()], sources());
        let tracked = ["src/kind.rs".to_owned(), "src/use.rs".to_owned()];
        let ctx = Context {
            allowed: &allowed,
            denied: &[],
            requirements: &requirements,
            sources: &sources,
            tracked: &tracked,
            snapshots: &[],
        };
        let mut rows: Vec<ContractRow> = ["a", "a", "c", "d", "e", "f", "g", "h", "i"]
            .iter()
            .enumerate()
            .map(|(index, id)| row((index as u64 + 1) * 10, id))
            .collect();
        rows[1].req.push("FR9".to_owned());
        rows[2].section = "2".to_owned();
        rows[3].section = "4".to_owned();
        rows[4].verify = vec!["git log (x)".to_owned()];
        rows[5].depends = vec!["zz".to_owned()];
        rows[6].depends = vec!["h".to_owned()];
        rows[7].depends = vec!["g".to_owned()];
        rows[8].touches = vec!["crate::kind::Kind".to_owned()];
        rows[8].write_set = vec!["src/kind.rs".to_owned(), "src".to_owned()];
        let found = check_table(DOC, &rows, &ctx);
        let shown: Vec<(u64, String)> = found.iter().map(|finding| (finding.line, finding.refuse.label())).collect();
        let want: Vec<(u64, &str)> = vec![
            (20, "contract-table:duplicate-id"),
            (20, "contract-table:requirement-missing"),
            (30, "contract-table:section-missing"),
            (40, "contract-table:section-missing"),
            (50, "contract-table:verify-form"),
            (60, "contract-table:depends-unresolved"),
            (70, "contract-table:depends-cycle"),
            (90, "write-set-dir-without-slash"),
            (90, "write-set-incomplete"),
        ];
        assert_eq!(shown, want.iter().map(|(line, label)| (*line, (*label).to_owned())).collect::<Vec<_>>());
        let missing = Refuse::WriteSetIncomplete { missing: vec!["src/use.rs".to_owned()] };
        assert_eq!(found.last().map(|finding| &finding.refuse), Some(&missing), "足りない file だけを名指す");
        let cycle = found.iter().find(|finding| finding.line == 70).map(|finding| finding.refuse.reason());
        assert_eq!(cycle.as_deref(), Some("depends が輪を成す（g → h → g）"), "輪は 1 件で 2 行を名乗る");
    }

    /// 契約表の verify 行にも intake と同じ禁じる語列の判定が掛かる（ADR-0025 §2.3・FR55「intake と同じ検査を表の全行に」）:
    /// 先頭語が allowlist に在っても `runner.denied_commands` の語列に当たる行は `verify-form` で行番号付きに名指し、
    /// 理由は行 id と語列を持つ。語列を持たない文脈（`denied = []`）では同じ行が通る＝判定の出所は行の値である。
    #[test]
    fn table_check_names_a_verify_line_that_hits_a_denied_sequence() {
        let requirements = Ok(["FR1".to_owned()].into_iter().collect::<BTreeSet<String>>());
        let (allowed, sources) = (["git".to_owned()], sources());
        let denied = ["git push --force".to_owned()];
        let tracked = ["src/kind.rs".to_owned(), "src/use.rs".to_owned()];
        let mut forced = row(10, "a");
        forced.verify = vec!["git push origin main --force".to_owned()];
        let closed = Context {
            allowed: &allowed,
            denied: &denied,
            requirements: &requirements,
            sources: &sources,
            tracked: &tracked,
            snapshots: &[],
        };
        let found = check_table(DOC, &[forced.clone()], &closed);
        let labels: Vec<String> = found.iter().map(|finding| finding.refuse.label()).collect();
        assert_eq!(labels, vec!["contract-table:verify-form".to_owned()], "禁じる語列の行は verify-form の 1 件: {labels:?}");
        let rendered = found.first().map(|finding| finding.render("docs/design/t.md")).unwrap_or_default();
        assert!(rendered.starts_with("contracts: docs/design/t.md:10 "), "行番号付き: {rendered}");
        assert!(rendered.contains("runner.denied_commands") && rendered.contains("git push --force"), "行 id と語列: {rendered}");
        let open = Context { denied: &[], ..closed };
        assert!(check_table(DOC, &[forced], &open).is_empty(), "語列の無い文脈では通る（判定の出所は行の値）");
    }

    /// 要件面を読めない周・閉包の入力を読めない周は、黙って通さず行ごとに `unreadable`（rc 2）で名指す。
    #[test]
    fn table_check_fails_closed_when_the_requirement_face_or_a_source_is_unreadable() {
        let requirements: Result<BTreeSet<String>, String> = Err("srs を読めない".to_owned());
        let allowed = ["git".to_owned()];
        let mut sources = sources();
        sources.push(Source { path: "src/broken.rs".to_owned(), body: Err("invalid utf-8".to_owned()) });
        let tracked = ["src/kind.rs".to_owned()];
        let ctx = Context {
            allowed: &allowed,
            denied: &[],
            requirements: &requirements,
            sources: &sources,
            tracked: &tracked,
            snapshots: &[],
        };
        let mut touched = row(10, "a");
        touched.touches = vec!["crate::kind::Kind".to_owned()];
        let found = check_table(DOC, &[touched], &ctx);
        let rendered: Vec<String> = found.iter().map(|finding| finding.render("docs/design/t.md")).collect();
        assert_eq!(found.len(), 2, "要件面と閉包の入力の 2 件: {rendered:?}");
        assert!(found.iter().all(|finding| finding.rc() == RC_BROKEN), "読めない周は rc 2: {rendered:?}");
        assert!(rendered.iter().any(|line| line.starts_with("contracts: docs/design/t.md:10 contract-table:unreadable: srs")));
        assert!(rendered.iter().any(|line| line.contains("src/broken.rs を読めない")), "{rendered:?}");
    }
}
