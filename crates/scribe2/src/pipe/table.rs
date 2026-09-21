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
//!    落とさない」）。intake（契約 (b)）は同じ関数を 1 行に撃つ＝1 実装（C2）。`depends` の解決の母集団（同じ doc の
//!    全行の id）は引数で渡す＝1 行に撃つ intake でも相手が別の行に在る `depends` が解ける（§30・行 ad）。上限の余地
//!    （§3）は受付時点の事実なので CI では撃たない（intake の側・[`super::cli`]）。
//!
//! 1 と 3 の群は子 module（`table/parse.rs` = 区間の抜き出しと TOML の型付け・`table/check.rs` = 表の検査と要件面と
//! CLI の駆動）に置き、本 file は 2 と findings の語彙（[`TableError`] / [`Finding`] / [`Context`]）を持つ。呼び手の
//! `use` は下の再 export を通る（`s2-07l.374`・設計 §15）。
//!
//! 約束の行 `[[promise]]`（設計 §33・行 af）は契約の行の子行で、欄の正本は [`PROMISE_FIELDS`]（9 欄）・型は
//! [`PromiseRow`]。区間の中の約束の行は parse の段で契約の行と分けて読み（rules manifest の面は `[[contract]]` だけを
//! 受けるので、約束の行の区間は契約の本文から抜いて値の層〔`scalar` / `list`〕だけを共有する）、`of` の親の行の実在と
//! `n` の連番は表の検査の段が [`TableError::PromiseOrphan`] / [`TableError::PromiseNumber`] で名指す。

use super::closure::{ClosureError, Source};
use super::refuse::Refuse;
use crate::cli_outcome::{RC_BROKEN, RC_REFUSED};
use crate::name::NAME;
use crate::polarity::{OnFailure, Polarity, Timing};
use std::collections::BTreeSet;

mod check;
mod parse;

pub use check::{check_promises, check_table, requirement_ids};
pub use parse::{
    contract_id, find_row, form_of, parse_pointer, promises_of, read_rows, read_table, Form, Pointer, PointerError,
};
pub(crate) use check::{check_repo, read, read_all, tracked_files};

/// 区間の始まりの行（CLAUDE.md の憲法区間と同じ marker 形・行全体が marker の行だけを数える）。
pub const BEGIN: &str = "<!-- contracts:begin -->";

/// 区間の終わりの行。
pub const END: &str = "<!-- contracts:end -->";

/// 約束の行の見出し（契約の行の子行・top-level の array of tables・設計 §33）。
pub const PROMISE: &str = "[[promise]]";

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
    /// 約束の行（`[[promise]]`）を持たない行では必須・持つ行では任意（設計 §33 の「必須の緩み」）。約束の行を持つ
    /// 行（Promised）の `done` / `verify` は器が約束の行から生成するので、書かない形が正しい。rules manifest の必須
    /// key の検査は [`Need::Required`] だけを数え、この値の欠けは区間の parse（`table/parse.rs`）が約束の行を `of` で
    /// 数えてから名指す。
    Conditional,
}

impl Need {
    /// 生成物に出す語（variant の名を小文字にした 1 語＝xtask の contracts-schema の導出と同じ形）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::Conditional => "conditional",
        }
    }
}

/// 欄の値の形（TOML subset の値のうち契約表が使う 3 つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// 空でない文字列。
    Text,
    /// 文字列の配列。
    List,
    /// 1 以上の整数（約束の行の `n`）。
    Number,
}

impl Shape {
    /// 生成物に出す語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::List => "list",
            Self::Number => "number",
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
    Field { name: "write-set", need: Need::Optional, shape: Shape::List },
    Field { name: "creates", need: Need::Optional, shape: Shape::List },
    Field { name: "tests", need: Need::Optional, shape: Shape::List },
    Field { name: "also", need: Need::Optional, shape: Shape::List },
    Field { name: "verify", need: Need::Conditional, shape: Shape::List },
    Field { name: "size", need: Need::Required, shape: Shape::Text },
    Field { name: "done", need: Need::Conditional, shape: Shape::Text },
    Field { name: "depends", need: Need::Optional, shape: Shape::List },
    Field { name: "classes", need: Need::Optional, shape: Shape::List },
    Field { name: "opens", need: Need::Optional, shape: Shape::List },
];

/// 約束の行 `[[promise]]` の欄の全体（**正本**・宣言順が `contracts schema` の描く順・設計 §33 の 9 欄）。`place` は
/// base に無い歯の名の周だけ要る（受付の側の判定）ので、表の形としては任意。
pub const PROMISE_FIELDS: &[Field] = &[
    Field { name: "of", need: Need::Required, shape: Shape::Text },
    Field { name: "n", need: Need::Required, shape: Shape::Number },
    Field { name: "text", need: Need::Required, shape: Shape::Text },
    Field { name: "files", need: Need::Required, shape: Shape::List },
    Field { name: "symbols", need: Need::Optional, shape: Shape::List },
    Field { name: "teeth", need: Need::Required, shape: Shape::List },
    Field { name: "place", need: Need::Optional, shape: Shape::Text },
    Field { name: "fixture", need: Need::Required, shape: Shape::Text },
    Field { name: "expect", need: Need::Required, shape: Shape::Text },
];

/// 約束の行 1 つ（欄は [`PROMISE_FIELDS`]・任意の欄の「無い」は空）。親の行は `of` の行 id で引く（[`promises_of`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromiseRow {
    /// 行の見出し（`[[promise]]`）の doc 上の行番号。
    pub line: u64,
    /// 親の行 id（同じ doc の `[[contract]]` の `id`）。
    pub of: String,
    /// 親の行の中の番号（1 から連番）。
    pub n: u64,
    /// 約束の 1 文。
    pub text: String,
    /// 触る file の列（`+` `-` `~` の接頭辞は write-set の項目と同じ）。
    pub files: Vec<String>,
    /// 名指す識別子の列（base に無い新設は `+` を前置）。
    pub symbols: Vec<String>,
    /// 歯の完全名の列。
    pub teeth: Vec<String>,
    /// 歯の置き場の file（空 = base の歯の名で解く）。
    pub place: String,
    /// 歯の fixture の形の 1 文。
    pub fixture: String,
    /// 歯が観測する結果の 1 文。
    pub expect: String,
}

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
    /// 触ってよい path の列（§3「write-set の導出」以後は任意: 無い行は受付が導出値を write-set にする）。
    pub write_set: Vec<String>,
    /// 新設する file の列（`+` を付けずに書く・§3 (iv)・空 = 新設しない）。
    pub creates: Vec<String>,
    /// 歯の新しい置き場の列（base の歯の file か `creates` の新規 file・§3 (ii)・空 = base の歯の名で解く）。
    pub tests: Vec<String>,
    /// Rust の外で触る file の列（base に実在する非 `.rs`・§3 (v)・空 = 触らない）。
    pub also: Vec<String>,
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
    /// 約束の行の `of` が同じ doc の行 id に無い（約束の行の見出しの行）。
    PromiseOrphan {
        /// 行番号。
        line: u64,
        /// 書かれていた親の行 id。
        of: String,
    },
    /// 約束の行の `n` が親の行の中で重複するか欠ける（1 から連番でない）。重複は 2 本目の約束の行・欠番は親の行の
    /// 最初の約束の行に置く。
    PromiseNumber {
        /// 行番号。
        line: u64,
        /// 親の行 id。
        of: String,
        /// 重複した番号か、欠けた番号。
        n: u64,
        /// 重複（真）か欠番（偽）か。
        duplicate: bool,
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
            | Self::SurfaceUnknown { line, .. }
            | Self::PromiseOrphan { line, .. }
            | Self::PromiseNumber { line, .. } => line,
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
            Self::PromiseOrphan { .. } => "promise-orphan",
            Self::PromiseNumber { .. } => "promise-number",
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
            Self::PromiseOrphan { ref of, .. } => format!("{PROMISE} の of {of} が同じ doc の行 id に無い"),
            Self::PromiseNumber { ref of, n, duplicate: true, .. } => format!("行 {of} の約束の n {n} が重複する"),
            Self::PromiseNumber { ref of, n, duplicate: false, .. } => {
                format!("行 {of} の約束の n {n} が欠ける（n は 1 から連番）")
            }
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
    // 約束の行の欄は別の表・別の key で描く（`[[field]]` の `name` / `need` / `shape` は契約の行の欄だけ＝xtask の
    // contracts-schema と FIELDS の照合の母集団を変えない）。
    lines.extend([String::new(), format!("# 約束の行 {PROMISE} の欄（正本は同じ file の PROMISE_FIELDS）")]);
    for field in PROMISE_FIELDS {
        lines.extend([
            String::new(),
            "[[promise-field]]".to_owned(),
            format!("promise-name = \"{}\"", field.name),
            format!("promise-need = \"{}\"", field.need.as_str()),
            format!("promise-shape = \"{}\"", field.shape.as_str()),
        ]);
    }
    lines
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.374

    use super::{read_rows, read_table, render_schema, Need, Shape, TableError, FIELDS, PROMISE_FIELDS};
    use crate::cli_outcome::{RC_BROKEN, RC_REFUSED};

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
        "promise-orphan",
        "promise-number",
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
            TableError::PromiseOrphan { line: 12, of: text("zz") },
            TableError::PromiseNumber { line: 13, of: text("a"), n: 2, duplicate: false },
        ]
    }

    /// 名前の slice は宣言順で `as_str` と 1 対 1・各 variant は行番号と 1 行の理由を持ち、読めない周だけ rc 2。
    #[test]
    fn table_error_names_are_pinned_in_declaration_order_and_carry_their_line() {
        let found = samples();
        let names: Vec<&str> = found.iter().map(TableError::as_str).collect();
        assert_eq!(names, TABLE_ERRORS, "名前の slice は宣言順（母集団 {} 値）", TABLE_ERRORS.len());
        assert_eq!(TABLE_ERRORS.len(), 13, "母集団は 13 値");
        for (index, error) in found.iter().enumerate() {
            assert_eq!(error.line(), index as u64 + 1, "{} は行番号を持つ", error.as_str());
            assert!(!error.reason().is_empty() && !error.reason().contains('\n'), "{} の理由は 1 行", error.as_str());
            let want = if matches!(error, TableError::Unreadable { .. }) { RC_BROKEN } else { RC_REFUSED };
            assert_eq!(error.rc(), want, "{} の rc", error.as_str());
        }
        let cycle = found.get(9).map(TableError::reason).unwrap_or_default();
        assert!(cycle.contains("a → b → a"), "輪は id を順に名乗り最初へ戻る: {cycle}");
        let surface = found.get(10).map(TableError::reason).unwrap_or_default();
        assert!(surface.contains("nope_external_form"), "未知の外形の名を名乗る: {surface}");
        let orphan = found.get(11).map(TableError::reason).unwrap_or_default();
        assert!(orphan.contains("of zz"), "親の無い of を名乗る: {orphan}");
        let gap = found.get(12).map(TableError::reason).unwrap_or_default();
        assert!(gap.contains("行 a の約束の n 2 が欠ける"), "欠番は親の行 id と番号を名乗る: {gap}");
        let twice = TableError::PromiseNumber { line: 1, of: "a".to_owned(), n: 1, duplicate: true }.reason();
        assert!(twice.contains("行 a の約束の n 1 が重複する"), "重複は欠番と別の字面: {twice}");
    }

    /// 全欄を持つ `.toml` の 1 行（`over` の欄だけ値を差し替える）。
    /// 子 module の歯（`table/parse.rs`）も同じ fixture を `super::super::tests::full_row` で読む（複製しない）ので
    /// 可視性は `pub(super)`（`s2-07l.374`・設計 §15）。
    pub(super) fn full_row(over: &[(&str, &str)]) -> String {
        let mut text = "schema = 1\n\n[[contract]]\n".to_owned();
        for field in FIELDS {
            let default = match field.shape {
                Shape::Text if field.name == "section" => "\"1\"".to_owned(),
                Shape::Text => "\"v\"".to_owned(),
                Shape::List => "[\"v\"]".to_owned(),
                Shape::Number => "1".to_owned(),
            };
            let value = over.iter().find(|(name, _)| *name == field.name).map_or(default, |(_, found)| (*found).to_owned());
            text.push_str(&format!("{} = {value}\n", field.name));
        }
        text
    }

    /// 全欄を持つ約束の行 1 つ（`of` = `of`・`n` = `n`・`over` の欄だけ値を差し替え、値が空の字面の欄は書かない）。
    /// 子 module の歯も同じ fixture を読む。
    pub(super) fn full_promise(of: &str, n: u64, over: &[(&str, &str)]) -> String {
        let mut text = "\n[[promise]]\n".to_owned();
        for field in PROMISE_FIELDS {
            let default = match (field.name, field.shape) {
                ("of", _) => format!("\"{of}\""),
                ("n", _) => n.to_string(),
                (_, Shape::Text) => format!("\"{} の値\"", field.name),
                (_, Shape::List) => format!("[\"{} の値\"]", field.name),
                (_, Shape::Number) => "1".to_owned(),
            };
            let value = over.iter().find(|(name, _)| *name == field.name).map_or(default, |(_, found)| (*found).to_owned());
            if !value.is_empty() {
                text.push_str(&format!("{} = {value}\n", field.name));
            }
        }
        text
    }

    /// 欄の列は宣言順に 16（必須 5・条件付き 2・任意 9）で、`contracts schema` はその順に描く。欄の形は reader が強制する
    /// （文字列の欄に配列・配列の欄に文字列を書くと、その欄を名指して断る）。`write-set` は任意（契約 (h)・§3
    /// 「write-set の導出」: 無い行は受付が導出値を写す）。約束の行の欄は別の列 9（必須 7・任意 2）で、生成物は
    /// 契約の行の欄の後に別の表・別の key で描く（`[[field]]` の母集団は 16 のまま）。
    #[test]
    fn table_fields_pin_the_schema_columns_and_the_reader_enforces_their_shapes() {
        let names: Vec<&str> = FIELDS.iter().map(|field| field.name).collect();
        let want = [
            "id", "title", "req", "section", "touches", "surfaces", "write-set", "creates", "tests", "also", "verify",
            "size", "done", "depends", "classes", "opens",
        ];
        assert_eq!(names, want, "欄の宣言順");
        assert_eq!(FIELDS.iter().filter(|field| field.need == Need::Required).count(), 5, "必須 5・条件付き 2・任意 9");
        let optional = |name: &str| FIELDS.iter().any(|field| field.name == name && field.need == Need::Optional);
        assert!(["write-set", "creates", "tests", "also"].iter().all(|name| optional(name)), "導出の 4 欄は任意");
        let rendered = render_schema();
        let listed: Vec<&str> =
            rendered.iter().filter_map(|line| line.strip_prefix("name = \"")?.strip_suffix('"')).collect();
        assert_eq!(listed, names, "生成物は欄の宣言順");
        assert_eq!(FIELDS.len(), 16, "契約の行の欄は 16");
        assert_eq!(rendered.get(1).map(String::as_str), Some("schema = 1"), "生成物も schema = 1 を持つ");
        assert_eq!(read_rows("t.toml", &full_row(&[])).map(|rows| rows.len()), Ok(1), "全欄の行は読める");
        for field in FIELDS {
            let errors = read_rows("t.toml", &full_row(&[(field.name, wrong(field.shape))])).expect_err("形の違う欄は断る");
            let named = errors.iter().any(|error| error.reason().starts_with(&format!("{} は", field.name)));
            assert!(named, "{} の形を名指す: {errors:?}", field.name);
        }
        promise_fields_are_pinned(&rendered);
    }

    // flip-check: s2-07l.512

    /// §33 (f) の母集団: `FIELDS` の `need` は必須 5・条件付き 2（`verify` と `done`・宣言順）・任意 9 の和 16 で、生成物の
    /// `need` の列は `FIELDS` と同じ順に `conditional` を 2 欄（`verify` / `done`）で載せる（xtask の contracts-schema は
    /// variant の名を小文字にした語で照合する＝同じ語）。
    #[test]
    fn contract_promise_need_conditional_is_two_fields_in_the_schema() {
        let count = |need: Need| FIELDS.iter().filter(|field| field.need == need).count();
        assert_eq!((count(Need::Required), count(Need::Conditional), count(Need::Optional)), (5, 2, 9), "必須 5・条件付き 2・任意 9");
        assert_eq!(FIELDS.len(), 16, "母集団 16");
        let conditional: Vec<&str> =
            FIELDS.iter().filter(|field| field.need == Need::Conditional).map(|field| field.name).collect();
        assert_eq!(conditional, ["verify", "done"], "条件付きは verify と done");
        let rendered = render_schema();
        let needs: Vec<&str> =
            rendered.iter().filter_map(|line| line.strip_prefix("need = \"")?.strip_suffix('"')).collect();
        let want: Vec<&str> = FIELDS.iter().map(|field| field.need.as_str()).collect();
        assert_eq!(needs, want, "生成物の need の列は FIELDS の順");
        assert_eq!(needs.iter().filter(|need| **need == "conditional").count(), 2, "conditional は 2 欄");
        assert_eq!(Need::Conditional.as_str(), format!("{:?}", Need::Conditional).to_lowercase(), "variant 名の小文字");
    }

    /// 欄の形に合わない値の字面（文字列と数の欄に配列・配列の欄に文字列）。
    fn wrong(shape: Shape) -> &'static str {
        match shape {
            Shape::Text | Shape::Number => "[\"x\"]",
            Shape::List => "\"x\"",
        }
    }

    /// 約束の行の欄は宣言順に 9（必須 7・任意 2）で、生成物は契約の行の欄の後に `[[promise-field]]` の表で描く。
    /// 欄の形は reader が強制する（契約の行と同じ字面で欄を名指す）。
    fn promise_fields_are_pinned(rendered: &[String]) {
        let promised: Vec<&str> = PROMISE_FIELDS.iter().map(|field| field.name).collect();
        let want = ["of", "n", "text", "files", "symbols", "teeth", "place", "fixture", "expect"];
        assert_eq!(promised, want, "約束の行の欄の宣言順（9）");
        assert_eq!(PROMISE_FIELDS.iter().filter(|field| field.need == Need::Required).count(), 7, "必須 7・任意 2");
        let promise_optional: Vec<&str> =
            PROMISE_FIELDS.iter().filter(|field| field.need == Need::Optional).map(|field| field.name).collect();
        assert_eq!(promise_optional, ["symbols", "place"], "任意は symbols と place");
        let columns: Vec<String> = rendered
            .iter()
            .filter_map(|line| line.strip_prefix("promise-name = \"")?.strip_suffix('"'))
            .map(str::to_owned)
            .collect();
        assert_eq!(columns, want, "生成物に約束の行の 9 欄が宣言順で載る");
        let shapes: Vec<&str> =
            rendered.iter().filter_map(|line| line.strip_prefix("promise-shape = \"")?.strip_suffix('"')).collect();
        assert_eq!(shapes.get(1), Some(&"number"), "n は数の形");
        let last_field = rendered.iter().rposition(|line| line == "[[field]]");
        let first_promise = rendered.iter().position(|line| line == "[[promise-field]]");
        assert!(last_field < first_promise, "約束の行の欄は契約の行の欄の後");
        let base = full_row(&[("id", "\"a\"")]);
        assert_eq!(read_table("t.toml", &format!("{base}{}", full_promise("a", 1, &[]))).map(|(_, found)| found.len()), Ok(1));
        for field in PROMISE_FIELDS {
            let errors = read_table("t.toml", &format!("{base}{}", full_promise("a", 1, &[(field.name, wrong(field.shape))])))
                .expect_err("形の違う約束の欄は断る");
            let named = errors.iter().any(|error| error.reason().starts_with(&format!("{} は", field.name)));
            assert!(named, "約束の行の {} の形を名指す: {errors:?}", field.name);
        }
    }
}
