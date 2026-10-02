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

use super::closure::{Base, ClosureError, Source};
use super::contract::{Class, CLASS_ROW};
use super::declaration::{TablePlaces, DECL_FILE};
use super::refuse::{Evidence, Refuse};
use crate::cli_outcome::{RC_BROKEN, RC_REFUSED};
use crate::name::NAME;
use crate::polarity::{OnFailure, Polarity, Timing};
use std::collections::BTreeSet;
use std::path::Path;

mod changed;
mod check;
mod parse;
mod teeth;

pub use check::{check_promises, check_table, requirement_ids};
pub use parse::{
    contract_id, find_row, form_of, parse_pointer, promises_of, read_rows, read_table, Form, Pointer, PointerError,
};
pub(crate) use check::{check_repo, declared_files, design_docs, read, read_all, repo_findings, tracked_files, Located};
/// 区間の始まりの行（CLAUDE.md の憲法区間と同じ marker 形・行全体が marker の行だけを数える）。
pub const BEGIN: &str = "<!-- contracts:begin -->";

/// 区間の終わりの行。
pub const END: &str = "<!-- contracts:end -->";

/// 約束の行の見出し（契約の行の子行・top-level の array of tables・設計 §33）。
pub const PROMISE: &str = "[[promise]]";

/// 導出物（`.toml` の全文）の行だけが持つ欄の名（節の本文の逐語・設計 contract-source.md §47・行 ay）。[`FIELDS`] と
/// 生成物には載せない（`.md` の区間の行は節の本文を `section` が指す＝同じ本文を 2 面に持たない）。rules manifest の
/// 契約表の key 集合はこの分だけ広く、`.md` の区間の行が持てば parse の段が未知の key として断る。
pub const DERIVED_GOAL: &str = "goal";

/// 導出物（`.toml` の全文）の先頭の版の宣言の字面（folio2 の ADR-3 決定 (4)・値は rules manifest の schema の版と同じ・
/// 設計 contract-source.md §47 の 3）。空行と `#` の行を除いた最初の行がこの字面でなければ、その行番号で断る。
pub const WHOLE_HEAD: &str = "schema = 1";

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
    Field { name: "targets", need: Need::Optional, shape: Shape::List },
    Field { name: "growth", need: Need::Optional, shape: Shape::List },
    Field { name: "done-teeth", need: Need::Optional, shape: Shape::List },
    Field { name: "code-facts", need: Need::Optional, shape: Shape::List },
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
    /// 検出線の的（`<file>:<行>:<変異の名>` の列・空 = diff の追加行を母集団にする従来の経路・設計 gate-cost.md §16）。
    pub targets: Vec<String>,
    /// file ごとの見込み行数（`<path>:<行数>` の列・空 = 全 file が `size` の見込み・設計 contract-source.md §46）。
    pub growth: Vec<String>,
    /// done の番号つき項目ごとの歯の対応（`<番号>:<歯>` の列・空 = 欄を持たない行・設計 contract-source.md §66 形 1・行 bw）。
    pub done_teeth: Vec<String>,
    /// 節の本文の逐語（導出物の行だけの欄 [`DERIVED_GOAL`]・空 = `section` の節を doc から読む・設計 contract-source.md §47）。
    pub goal: String,
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
    /// `targets` の値が的の形（`<file>:<行>:<変異の名>`）でない（設計 gate-cost.md §16）。
    TargetForm {
        /// 行番号。
        line: u64,
        /// 的の字面。
        target: String,
        /// 形に合わない理由（契約 file の読みと同じ判定の 1 本が返す字面）。
        reason: String,
    },
    /// Declared 行の verify の filter 語が解く歯の file が write-set の外に在る（行の見出しの行・設計 contract-source.md
    /// §45・行 aw）。
    TeethOutsideWriteSet {
        /// 行番号。
        line: u64,
        /// 行 id。
        id: String,
        /// write-set の外の歯の file（辞書順）。
        files: Vec<String>,
    },
    /// `growth` の項目が形を崩す（path と行数の形でない・write-set の file の項目に無い・`.rs` でない・`-` / `~` / `=` の
    /// 項目・同じ path が 2 回・行の見出しの行・設計 contract-source.md §46・行 ax）。
    GrowthForm {
        /// 行番号。
        line: u64,
        /// growth の項目の字面。
        item: String,
        /// 崩れの理由（growth の読み手の 1 本が返す字面）。
        reason: String,
    },
    /// verify 行がクラスの語列表の要素の語列に当たり（禁じる語列と同じ照合）、導いたクラスが行の `classes` に無い（行の
    /// 見出しの行・当たり 1 つにつき 1 件・設計 contract-source.md §48 の 4・行 az）。
    ClassUndeclared {
        /// 行番号。
        line: u64,
        /// 当たった verify 行の字面。
        verify: String,
        /// 当たった要素の語列。
        sequence: String,
        /// 導いたクラス。
        class: Class,
    },
    /// 宣言した契約表の置き場の項目が tracked の path に 1 つも当たらない（dir 項目は直下に `form_of` が読める tracked file が 0 本・
    /// file 項目は tracked に無い・`.vessel.toml` の key の行・設計 contract-source.md §69 形 5・行 cf）。
    PlaceEmpty {
        /// 行番号（key が書かれていた行）。
        line: u64,
        /// 当たらなかった項目の字面。
        item: String,
    },
    /// 列挙した doc の file 名の stem が重なる（列挙の順で 2 本目以降の doc を名指す・行 0・行 cf）。契約 id は stem と行 id の組で、
    /// doc id は一意でなければならない。
    DocIdDuplicate {
        /// 行番号（file 全体の 0）。
        line: u64,
        /// 重なった 2 本目以降の doc（repo 相対）。
        doc: String,
        /// 先に列挙された相手の doc（repo 相対）。
        other: String,
    },
    /// 欄 `done-teeth` の要素が外れる（形・覆い・検証行の番号・選ばれ・仕組みの測る物・base の在りか・行の見出しの行・1 件に 1 要素・
    /// 設計 contract-source.md §66 形 2・行 bw）。
    DoneTeeth {
        /// 行番号。
        line: u64,
        /// 外れた要素の字（覆いの欠けは `<番号>:`）。
        element: String,
        /// 外れの理由。
        reason: String,
    },
    /// 変わった行（base から足された行・done の字が変わった行）の done が番号つきの項目を 1 つも持たない（宣言 `teeth-check` が true の
    /// repo で `--base` を渡した周だけ・行の見出しの行・設計 contract-source.md §66 形 3・行 by）。
    DoneUnnumbered {
        /// 行番号。
        line: u64,
    },
    /// 変わった行が欄 `done-teeth` を持たない（宣言 `teeth-check` が true の repo で `--base` を渡した周だけ・行の見出しの行・形 3・行 by）。
    DoneTeethMissing {
        /// 行番号。
        line: u64,
    },
    /// 受付の pointer の path が受付の材料の宣言の置き場（既定の `docs/design/` 直下の `.md` と宣言 `contract-tables` の項目）に入らない
    /// （doc を読む前に断る・行 0・設計 contract-source.md §69 形 8・行 cg）。
    PlaceOutside {
        /// 行番号（file 全体の 0）。
        line: u64,
        /// 置き場の外を指していた pointer の path。
        path: String,
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
            | Self::PromiseNumber { line, .. }
            | Self::TargetForm { line, .. }
            | Self::TeethOutsideWriteSet { line, .. }
            | Self::GrowthForm { line, .. }
            | Self::ClassUndeclared { line, .. }
            | Self::PlaceEmpty { line, .. }
            | Self::DocIdDuplicate { line, .. }
            | Self::DoneTeeth { line, .. }
            | Self::DoneUnnumbered { line }
            | Self::DoneTeethMissing { line }
            | Self::PlaceOutside { line, .. } => line,
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
            Self::TargetForm { .. } => "target-form",
            Self::TeethOutsideWriteSet { .. } => "teeth-outside-write-set",
            Self::GrowthForm { .. } => "growth-form",
            Self::ClassUndeclared { .. } => "class-undeclared",
            Self::PlaceEmpty { .. } => "place-empty",
            Self::DocIdDuplicate { .. } => "doc-id-duplicate",
            Self::DoneTeeth { .. } => "done-teeth",
            Self::DoneUnnumbered { .. } => "done-unnumbered",
            Self::DoneTeethMissing { .. } => "done-teeth-missing",
            Self::PlaceOutside { .. } => "place-outside",
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
            Self::TargetForm { ref target, ref reason, .. } => {
                format!("targets {target:?} が <file>:<行>:<変異の名> の形でない: {reason}")
            }
            Self::TeethOutsideWriteSet { ref id, ref files, .. } => {
                format!("行 {id} の歯の file が write-set の外: {}", files.join(", "))
            }
            Self::GrowthForm { ref item, ref reason, .. } => format!("growth {item:?} が崩れている: {reason}"),
            Self::ClassUndeclared { ref verify, ref sequence, class, .. } => format!(
                "verify {verify:?} が rules 行 {CLASS_ROW} の語列 {sequence} に当たりクラス {} を導くが、行の classes に無い",
                class.as_str()
            ),
            Self::PlaceEmpty { ref item, .. } => {
                format!("契約表の置き場 {item} が tracked の path に当たらない（dir は直下に表と読める file が 0 本・file は tracked に無い）")
            }
            Self::DocIdDuplicate { ref doc, ref other, .. } => {
                format!("{doc} の file 名の stem が {other} と重なる（doc id は一意でなければならない）")
            }
            Self::DoneTeeth { ref element, ref reason, .. } => format!("done-teeth {element:?} が外れている: {reason}"),
            Self::DoneUnnumbered { .. } => DONE_UNNUMBERED.to_owned(),
            Self::DoneTeethMissing { .. } => DONE_TEETH_MISSING.to_owned(),
            Self::PlaceOutside { ref path, .. } => format!(
                "{path} は契約表の置き場の外（受付の pointer の置き場は既定の {DESIGN_DIR} 直下の .md と、宣言 {DECL_FILE} の key contract-tables の項目だけ）"
            ),
        }
    }

    /// rc（読めない周だけ 2・残りは前提違反の 1・NFR4）。
    pub fn rc(&self) -> u8 {
        match *self {
            Self::Unreadable { .. } => RC_BROKEN,
            _ => RC_REFUSED,
        }
    }

    /// 証拠の在り処（**網羅の match 1 本**・設計 docs/design/dispatcher.md §27 形 3）: 表の区間・欄の形・id・節・要件・verify の形・
    /// 依存・約束の行・的・見込み・クラスは行の字と規則だけで決まり、外形の名と歯の置き場は本文の読み手が解き、読めない周は
    /// 測れない（置き場と同じ側に倒す）。置き場の欠陥は名指した path の列（place-empty は項目・doc-id-duplicate は 2 本の doc）。
    pub(crate) fn evidence(&self) -> Evidence {
        match *self {
            Self::SurfaceUnknown { .. } | Self::TeethOutsideWriteSet { .. } | Self::DoneTeeth { .. } => Evidence::Name,
            Self::PlaceEmpty { ref item, .. } => Evidence::Files(vec![item.clone()]),
            Self::PlaceOutside { .. } => Evidence::Files(vec![DECL_FILE.to_owned()]),
            Self::DocIdDuplicate { ref doc, ref other, .. } => Evidence::Files(vec![doc.clone(), other.clone()]),
            Self::Unreadable { .. } => Evidence::Place,
            Self::RegionMissing { .. }
            | Self::RegionDuplicate { .. }
            | Self::DuplicateId { .. }
            | Self::RowMissing { .. }
            | Self::SectionMissing { .. }
            | Self::RequirementMissing { .. }
            | Self::VerifyForm { .. }
            | Self::DependsUnresolved { .. }
            | Self::DependsCycle { .. }
            | Self::PromiseOrphan { .. }
            | Self::PromiseNumber { .. }
            | Self::TargetForm { .. }
            | Self::GrowthForm { .. }
            | Self::ClassUndeclared { .. }
            | Self::DoneUnnumbered { .. }
            | Self::DoneTeethMissing { .. } => Evidence::Row,
        }
    }
}

/// 行の欄 `done-teeth` の形の照らし（設計 contract-source.md §66 形 2 の (a)(b)(c)(d)(f)・[`teeth::done_teeth_misses`] の外れを
/// [`TableError::DoneTeeth`] の 1 件ずつに包む・欄を持たない行は 0 件）。表の検査（[`check_table`]）が行ごとに撃つ。
fn done_teeth_findings(row: &ContractRow) -> Vec<Finding> {
    let given = teeth::Given { done: &row.done, verify: &row.verify, write_set: &row.write_set, creates: &row.creates, elements: &row.done_teeth };
    done_teeth_wrapped(row.line, teeth::done_teeth_misses(&given, NAME))
}

/// 行の欄 `done-teeth` の在りかの照らし（§66 形 2 の (e)・base を渡す口だけが撃つ＝受付と preflight・表の検査の本体は撃たない）。
pub(crate) fn done_teeth_located_findings(row: &ContractRow, base: &Base<'_>) -> Vec<Finding> {
    done_teeth_wrapped(row.line, teeth::done_teeth_located(&row.done_teeth, &row.verify, base))
}

/// 照らしの外れ（teeth の型 `Miss`）を行 `line` の [`TableError::DoneTeeth`] の 1 件ずつに包む。
fn done_teeth_wrapped(line: u64, misses: Vec<teeth::Miss>) -> Vec<Finding> {
    let error = |miss: teeth::Miss| TableError::DoneTeeth { line, element: miss.element, reason: miss.reason };
    misses.into_iter().map(|miss| Finding::table(error(miss))).collect()
}

/// 変わった行の done が番号つきの項目を持たない断りの理由（[`TableError::DoneUnnumbered`]）。
const DONE_UNNUMBERED: &str = "変わった行（base から足された行か done の字が変わった行）の done は番号つきの項目 (1) を 1 つ以上持つ（宣言 teeth-check）";

/// 変わった行が欄 done-teeth を持たない断りの理由（[`TableError::DoneTeethMissing`]）。
const DONE_TEETH_MISSING: &str = "変わった行（base から足された行か done の字が変わった行）は done の番号つきの項目ごとの歯を欄 done-teeth で名指す（宣言 teeth-check）";

/// 置き場の項目 `item` が path を含むか（末尾 `/` は dir の直下で `form_of` が読める path・ほかは等しい path）。契約表の doc の
/// 列（[`design_docs`]）と置き場の検査（[`place_defects`]）が同じこの 1 本で測る。
fn in_item(path: &str, item: &str) -> bool {
    if item.ends_with('/') {
        path.strip_prefix(item).is_some_and(|rest| !rest.contains('/')) && form_of(path).is_ok()
    } else {
        path == item
    }
}

/// 契約表の置き場の検査（設計 contract-source.md §69 形 5・行 cf）: HEAD の宣言の項目のうち tracked の path に 1 つも当たらない項目
/// を `.vessel.toml` の key の行で 1 件ずつ、列挙した doc の file 名の stem の重なりを列挙の順で 2 本目以降の doc の行 0 で名指す
/// （既定の置き場の 0 本は名指さない・受付の検査はこれを撃たない＝repo 全体の事実）。宣言を読めない周は項目 0 と同じ（読めなさは
/// 呼び手が先に名指している）。
fn place_defects(repo: &Path, tracked: &[String], docs: &[&String]) -> Vec<(String, Finding)> {
    let mut found = Vec::new();
    if let TablePlaces::Declared { items, line } = TablePlaces::at(repo, "HEAD") {
        for item in items.iter().filter(|item| !tracked.iter().any(|path| in_item(path, item))) {
            let error = TableError::PlaceEmpty { line, item: item.clone() };
            found.push((DECL_FILE.to_owned(), Finding::table(error)));
        }
    }
    let stem = |doc: &str| Path::new(doc).file_stem().map(std::ffi::OsStr::to_owned);
    for (at, doc) in docs.iter().enumerate() {
        let earlier = docs.iter().take(at).find(|other| stem(other) == stem(doc));
        if let Some(other) = earlier {
            let error = TableError::DocIdDuplicate { line: 0, doc: (*doc).clone(), other: (*other).clone() };
            found.push(((*doc).clone(), Finding::table(error)));
        }
    }
    found
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
    pub(crate) fn table(error: TableError) -> Self {
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

    /// 型の断り（受付が findings を [`crate::pipe::cli::Denial`] の型の断りの列に写す口・設計 dispatcher.md §27 形 3）。
    pub(crate) fn refuse(&self) -> &Refuse {
        &self.refuse
    }

    /// write-set の項目の未解決（`write-set-item-unresolved`）ならその項目の字面、他の理由は `None`（設計 pipeline.md
    /// §34・追随で入った行が便の消した path を名指すかを呼び手が読む口・検査と `contracts check` の字面は変えない）。
    pub fn unresolved_item(&self) -> Option<&str> {
        match self.refuse {
            Refuse::WriteSetItemUnresolved { ref item } => Some(item),
            _ => None,
        }
    }
}

/// 表の検査の文脈（repo の側の事実・I/O は呼び手が済ませて渡す）。
pub struct Context<'a> {
    /// 宣言の allowlist（verify 行の先頭語の基準）。
    pub allowed: &'a [String],
    /// 禁じる語列（rules 行 `runner.denied_commands`・verify 行に intake と同じ判定を掛ける・ADR-0025 §2.3）。
    pub denied: &'a [String],
    /// クラスの語列表（rules 行 [`CLASS_ROW`] の値・verify 行から 3 クラスを導く・設計 contract-source.md §48）。
    pub classes: &'a [String],
    /// 要件面の id の集合（読めない周は理由）。
    pub requirements: &'a Result<BTreeSet<String>, String>,
    /// 閉包を測る `.rs` の列。
    pub sources: &'a [Source],
    /// tracked file の repo 相対 path（write-set の項目の実在・dir の判定・path 形の名指しに使う）。
    pub tracked: &'a [String],
    /// 外形 snapshot（tracked の `.snap`・`surfaces` の外形 pin に使う）。
    pub snapshots: &'a [Source],
    /// 宣言済みの新規 file（repo の全 doc の行の write-set の `+` 項目と `creates`・印は剥がす・名指しの解に足す）。
    /// 区間を読めない doc が在る周は理由（母集団を縮めて通さない・設計 contract-source.md §39）。
    pub declared: &'a Result<Vec<String>, String>,
    /// crate の根の列（固定の根 + 宣言した根・§62 の 1 関数が歯の置き場と module の段を読む）。
    pub crate_roots: &'a [String],
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

    use super::changed::{changed, gaps, Gap};
    use super::teeth::{done_teeth_located, done_teeth_misses, Given};
    use super::{
        declared_files, design_docs, read_rows, read_table, render_schema, tracked_files, Base, Class, Need, Shape, Source,
        TableError, DERIVED_GOAL, FIELDS, PROMISE_FIELDS, WHOLE_HEAD,
    };
    use crate::cli_outcome::{RC_BROKEN, RC_REFUSED};

    /// (c) 契約表の doc の列の 1 関数の表: 項目 0 は既定（`docs/design/` 直下の `.md`）だけ、dir 項目は直下で `form_of` が読める path、
    /// file 項目は等しい path だけ、既定と重なる項目は同じ path を 2 度返さない。返りは入力の順。
    #[test]
    fn table_places_docs_enumerates_defaults_and_items_once_in_input_order() {
        let paths: Vec<String> = [
            "contracts/g.txt",
            "docs/design/b.toml",
            "contracts/e.md",
            "docs/design/a.md",
            "contracts/sub/f.toml",
            "docs/design/sub/c.md",
            "tables/one.toml.bak",
            "contracts/d.toml",
            "tables/one.toml",
        ]
        .iter()
        .map(|path| (*path).to_owned())
        .collect();
        let pick = |items: &[&str]| -> Vec<&str> {
            let items: Vec<String> = items.iter().map(|item| (*item).to_owned()).collect();
            design_docs(&paths, &items).into_iter().map(String::as_str).collect()
        };
        assert_eq!(pick(&[]), ["docs/design/a.md"], "項目 0 は既定だけ");
        assert_eq!(pick(&["contracts/"]), ["contracts/e.md", "docs/design/a.md", "contracts/d.toml"], "直下の .md と .toml だけ・.txt と下の dir は返さない");
        assert_eq!(pick(&["tables/one.toml"]), ["docs/design/a.md", "tables/one.toml"], "file 項目は等しい path だけ");
        assert_eq!(pick(&["docs/design/"]), ["docs/design/b.toml", "docs/design/a.md"], "既定と重なる a.md は 1 度");
    }

    /// (d) 宣言済みの新規 file は HEAD の宣言の項目の列を読む: key で `contracts/` を名乗る commit は `contracts/t.toml` の行の `+` の file と
    /// `creates` を返し、key の無い commit は返さず、key の値を壊した commit は理由を返す。
    #[test]
    fn table_places_declared_files_reads_the_head_declaration() {
        let repo = crate::pipe::fixture::scratch("table-places-declared");
        for args in [&["init", "-q", "-b", "main"][..], &["config", "user.name", "t"], &["config", "user.email", "t@example.invalid"], &["config", "commit.gpgsign", "false"]] {
            assert!(crate::pipe::git_ok(&repo, args), "{args:?}");
        }
        let row = full_row(&[("id", "\"a\""), ("write-set", "[\"+src/new.rs\"]"), ("creates", "[\"made.rs\"]")]);
        assert!(std::fs::create_dir_all(repo.join("contracts")).is_ok());
        assert!(std::fs::write(repo.join("contracts/t.toml"), row).is_ok());
        let commit = |key: &str| {
            let declaration = format!("schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git status\"]\n{key}");
            assert!(std::fs::write(repo.join(".vessel.toml"), declaration).is_ok(), "宣言を書ける");
            assert!(crate::pipe::git_ok(&repo, &["add", "-A"]) && crate::pipe::git_ok(&repo, &["commit", "-q", "-m", "c"]), "commit");
            tracked_files(&repo).unwrap_or_default()
        };
        let tracked = commit("contract-tables = [\"contracts/\"]\n");
        assert_eq!(declared_files(&repo, &tracked), Ok(vec!["made.rs".to_owned(), "src/new.rs".to_owned()]), "key で名乗った置き場の行");
        let tracked = commit("");
        assert_eq!(declared_files(&repo, &tracked), Ok(Vec::new()), "key の無い commit は返さない");
        let tracked = commit("contract-tables = \"contracts/\"\n");
        let reason = declared_files(&repo, &tracked).expect_err("key の値を壊した commit は理由を返す");
        assert!(reason.contains("contract-tables"), "key を名乗る理由: {reason}");
    }

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
        "target-form",
        "teeth-outside-write-set",
        "growth-form",
        "class-undeclared",
        "place-empty",
        "doc-id-duplicate",
        "done-teeth",
        "done-unnumbered",
        "done-teeth-missing",
        "place-outside",
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
            TableError::TargetForm { line: 14, target: text("src/a.rs"), reason: text("r") },
            TableError::TeethOutsideWriteSet { line: 15, id: text("out"), files: vec![text("tests/a.rs"), text("tests/b.rs")] },
            TableError::GrowthForm { line: 16, item: text("src/a.md:5"), reason: text("r") },
            TableError::ClassUndeclared { line: 17, verify: text("git push o m"), sequence: text("git push"), class: Class::Publish },
            TableError::PlaceEmpty { line: 18, item: text("tables/none.toml") },
            TableError::DocIdDuplicate { line: 19, doc: text("docs/design/toy.md"), other: text("contracts/toy.toml") },
            TableError::DoneTeeth { line: 20, element: text("2:x_tooth"), reason: text("r") },
            TableError::DoneUnnumbered { line: 21 },
            TableError::DoneTeethMissing { line: 22 },
            TableError::PlaceOutside { line: 23, path: text("contracts/t.toml") },
        ]
    }

    /// 名前の slice は宣言順で `as_str` と 1 対 1・各 variant は行番号と 1 行の理由を持ち、読めない周だけ rc 2。
    #[test]
    fn table_error_names_are_pinned_in_declaration_order_and_carry_their_line() {
        let found = samples();
        let names: Vec<&str> = found.iter().map(TableError::as_str).collect();
        assert_eq!(names, TABLE_ERRORS, "名前の slice は宣言順（母集団 {} 値）", TABLE_ERRORS.len());
        assert_eq!(TABLE_ERRORS.len(), 23, "母集団は 23 値");
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
        let target = found.get(13).map(TableError::reason).unwrap_or_default();
        assert!(target.contains("\"src/a.rs\"") && target.ends_with(": r"), "的の字面と理由を名乗る: {target}");
    }

    /// 置き場の欠陥の 2 語の理由は、当たらなかった項目と重なった 2 本の doc の path を名乗る。
    #[test]
    fn table_place_defects_name_their_paths_in_the_reason() {
        let found = samples();
        let place = found.get(17).map(TableError::reason).unwrap_or_default();
        assert!(place.contains("tables/none.toml"), "当たらない項目を名乗る: {place}");
        let twice = found.get(18).map(TableError::reason).unwrap_or_default();
        assert!(twice.contains("docs/design/toy.md") && twice.contains("contracts/toy.toml"), "重なった 2 本の doc を名乗る: {twice}");
    }

    /// 在り処は全 variant の母集団（23）で 1 つずつ決まり（設計 dispatcher.md §27 形 3・宣言順）、契約表の欠陥の断りは同じ値を
    /// 受付の側（`Refuse::ContractTable`）へ渡す。本文の読み手が解くのは外形の名と歯の置き場と欄 done-teeth の在りかの 3 つ・読めない周は測れない。
    /// 変わった行の要否の 2 語（done-unnumbered・done-teeth-missing）は行の字だけで決まる。
    #[test]
    fn pipe_table_evidence_is_decided_once_for_every_variant() {
        use crate::pipe::refuse::Refuse;
        let found: Vec<&str> = samples().iter().map(|error| error.evidence().as_str()).collect();
        let want = [
            "row", "row", "place", "row", "row", "row", "row", "row", "row", "row", "name", "row", "row", "row", "name", "row",
            "row", "files", "files", "name", "row", "row", "files",
        ];
        assert_eq!(found, want, "母集団 {} variant の在り処（宣言順）", TABLE_ERRORS.len());
        assert_eq!(found.len(), TABLE_ERRORS.len(), "全 variant に 1 つ");
        for error in samples() {
            assert_eq!(Refuse::ContractTable(error.clone()).evidence(), error.evidence(), "{} は受付の側も同じ値", error.as_str());
        }
    }

    /// 全欄を持つ `.toml` の 1 行（`over` の欄だけ値を差し替える）。
    /// 子 module の歯（`table/parse.rs`）も同じ fixture を `super::super::tests::full_row` で読む（複製しない）ので
    /// 可視性は `pub(super)`（`s2-07l.374`・設計 §15）。
    pub(super) fn full_row(over: &[(&str, &str)]) -> String {
        let mut text = "schema = 1\n\n[[contract]]\n".to_owned();
        for field in FIELDS {
            let default = match field.shape {
                Shape::Text if field.name == "section" => "\"1\"".to_owned(),
                Shape::List if field.name == "targets" => "[\"src/v.rs:1:v\"]".to_owned(),
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

    /// 欄の列は宣言順に 20（必須 5・条件付き 2・任意 13・`targets` は設計 gate-cost.md §16・`growth` は §46・`done-teeth` と `code-facts` は §67）で、
    /// `contracts schema` はその順に描く。欄の形は reader が強制する（文字列の欄に配列・配列の欄に文字列を書くと、その欄を
    /// 名指して断る）。`write-set` は任意（契約 (h)・§3「write-set の導出」: 無い行は受付が導出値を写す）。約束の行の欄は
    /// 別の列 9（必須 7・任意 2）で、生成物は契約の行の欄の後に別の表・別の key で描く（`[[field]]` の母集団は 20）。
    #[test]
    fn table_fields_pin_the_schema_columns_and_the_reader_enforces_their_shapes() {
        let names: Vec<&str> = FIELDS.iter().map(|field| field.name).collect();
        let want = [
            "id", "title", "req", "section", "touches", "surfaces", "write-set", "creates", "tests", "also", "verify",
            "size", "done", "depends", "classes", "opens", "targets", "growth", "done-teeth", "code-facts",
        ];
        assert_eq!(names, want, "欄の宣言順");
        assert_eq!(FIELDS.iter().filter(|field| field.need == Need::Required).count(), 5, "必須 5・条件付き 2・任意 13");
        let optional = |name: &str| FIELDS.iter().any(|field| field.name == name && field.need == Need::Optional);
        assert!(["write-set", "creates", "tests", "also"].iter().all(|name| optional(name)), "導出の 4 欄は任意");
        assert!(optional("targets"), "的の欄は任意（無い行は従来の経路）");
        assert!(optional("growth"), "見込みの欄は任意（無い行は全 file が size の見込み）");        let rendered = render_schema();
        let listed: Vec<&str> =
            rendered.iter().filter_map(|line| line.strip_prefix("name = \"")?.strip_suffix('"')).collect();
        assert_eq!(listed, names, "生成物は欄の宣言順");
        assert_eq!(FIELDS.len(), 20, "契約の行の欄は 20");
        assert_eq!(rendered.get(1).map(String::as_str), Some("schema = 1"), "生成物も schema = 1 を持つ");
        assert_eq!(read_rows("t.toml", &full_row(&[])).map(|rows| rows.len()), Ok(1), "全欄の行は読める");
        for field in FIELDS {
            let errors = read_rows("t.toml", &full_row(&[(field.name, wrong(field.shape))])).expect_err("形の違う欄は断る");
            let named = errors.iter().any(|error| error.reason().starts_with(&format!("{} は", field.name)));
            assert!(named, "{} の形を名指す: {errors:?}", field.name);
        }
        let aimed = read_rows("t.toml", &full_row(&[("targets", "[\"src/v.rs:1:v\", \"src/v.rs:0:v\"]")]));
        let errors = aimed.expect_err("的の形でない値は断る");
        assert!(
            matches!(errors.as_slice(), [found @ TableError::TargetForm { .. }] if found.rc() == RC_REFUSED),
            "形の外れた 1 本だけを target-form（rc 1）で名指す: {errors:?}"
        );
        promise_fields_are_pinned(&rendered);
    }

    // flip-check: s2-07l.512

    /// §33 (f) の母集団: `FIELDS` の `need` は必須 5・条件付き 2（`verify` と `done`・宣言順）・任意 13 の和 20 で（§46 の
    /// `growth` と §67 の 2 欄で任意が増えた）、生成物の `need` の列は `FIELDS` と同じ順に `conditional` を 2 欄（`verify` / `done`）で
    /// 載せる（xtask の contracts-schema は variant の名を小文字にした語で照合する＝同じ語）。
    #[test]
    fn contract_promise_need_conditional_is_two_fields_in_the_schema() {
        let count = |need: Need| FIELDS.iter().filter(|field| field.need == need).count();
        assert_eq!((count(Need::Required), count(Need::Conditional), count(Need::Optional)), (5, 2, 13), "必須 5・条件付き 2・任意 13");
        assert_eq!(FIELDS.len(), 20, "母集団 20");
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

    /// §47 の 1 と 3: 導出物の先頭の版の宣言の字面は folio2 の ADR-3 決定 (4) の `schema = 1` と一致し（drift の歯・台帳
    /// `s2-07l.214` の (3)）、rules manifest の版と同じ値である。導出物だけの欄の名は goal で、欄の正本 `FIELDS`（20）と
    /// 生成物に載らない。
    #[test]
    fn contract_whole_goal_head_pins_the_folio2_schema_and_the_goal_stays_off_the_fields() {
        assert_eq!(WHOLE_HEAD, "schema = 1", "folio2 の ADR-3 決定 (4) の字面");
        assert_eq!(WHOLE_HEAD, format!("schema = {}", crate::rules::manifest::SCHEMA), "rules manifest の版と同じ値");
        assert_eq!(DERIVED_GOAL, "goal", "導出物だけの欄の名");
        assert!(FIELDS.iter().all(|field| field.name != DERIVED_GOAL), "FIELDS に goal は無い");
        assert_eq!(FIELDS.len(), 20, "欄の正本は 20（goal は載らない）");
        let rendered = render_schema();
        assert!(!rendered.iter().any(|line| line.contains(DERIVED_GOAL)), "生成物に goal は無い: {rendered:?}");
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

    /// 項目 3 つの done（欄 done-teeth の照らしの fixture）。
    const DONE3: &str = "(1) a (2) b (3) c";

    /// 歯の名の接頭辞 `tooth_` を部分一致で選ぶ検証行。
    const TOOTH_LINE: &str = "cargo nextest run -p toy --no-tests=fail tooth_";

    /// 文字列の列。
    fn own(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    /// 欄 done-teeth の形の照らしを撃ち、外れを (要素, 理由) の列で返す（core の crate は `toy`）。
    fn teeth_misses(done: &str, verify: &[&str], write_set: &[&str], creates: &[&str], elements: &[&str]) -> Vec<(String, String)> {
        let (verify, write_set, creates, elements) = (own(verify), own(write_set), own(creates), own(elements));
        let given = Given { done, verify: &verify, write_set: &write_set, creates: &creates, elements: &elements };
        done_teeth_misses(&given, "toy").into_iter().map(|miss| (miss.element, miss.reason)).collect()
    }

    /// 外れた要素の字の列。
    fn elements_of(found: &[(String, String)]) -> Vec<&str> {
        found.iter().map(|(element, _)| element.as_str()).collect()
    }

    /// (1) 形（§66 形 2 (a)）: 4 形の要素は外れず、空白か , を含む要素・: の無い要素・番号が数字でない要素・4 形の外の歯・同じ要素の重なりを
    /// 名指す（番号を読めた要素は覆いに数えるので、形の外れだけが並ぶ）。
    #[test]
    fn done_teeth_table_shape_names_bad_elements() {
        let good = ["1:tooth_a", "2:=tooth_b", "3:@1", "3:!write-set"];
        assert!(teeth_misses(DONE3, &[TOOTH_LINE], &[], &[], &good).is_empty(), "4 形すべてが適合なら外れ 0");
        let bad = ["1:tooth_a", "2:tooth b", "2:a,b", "3", "x:tooth_c", "3:path::tooth_c", "3:@", "3:=", "3:!bogus", "1:tooth_a"];
        let found = teeth_misses(DONE3, &[TOOTH_LINE], &[], &[], &bad);
        let want = ["2:tooth b", "2:a,b", "3", "x:tooth_c", "3:path::tooth_c", "3:@", "3:=", "3:!bogus", "1:tooth_a"];
        assert_eq!(elements_of(&found), want, "外れた要素を全件・要素の順に: {found:?}");
        let reason = |element: &str| found.iter().find(|(found, _)| found == element).map(|(_, reason)| reason.as_str()).unwrap_or_default();
        assert!(reason("1:tooth_a").contains("重なる"), "重なりは 2 つ目: {found:?}");
        assert!(reason("2:tooth b").contains("空白"), "{found:?}");
        assert!(reason("x:tooth_c").contains("数字"), "{found:?}");
        assert!(reason("3:!bogus").contains("bogus"), "仕組みの名を名乗る: {found:?}");
        assert!(teeth_misses(DONE3, &[TOOTH_LINE], &[], &[], &[]).is_empty(), "欄を持たない行（要素が空）は外れ 0");
    }

    /// (2) 覆い（(b)）: 番号の集合が done の項目 1〜K とちょうど等しいこと。無い番号と余る番号（0 を含む）を名指し、K が 0 の行は欄を持てない。
    #[test]
    fn done_teeth_table_cover_names_missing_and_extra_numbers() {
        let found = teeth_misses(DONE3, &[TOOTH_LINE], &[], &[], &["1:tooth_a", "3:tooth_c", "5:tooth_e", "0:tooth_z"]);
        assert_eq!(elements_of(&found), ["2:", "5:tooth_e", "0:tooth_z"], "無い番号が先・余る番号が要素の順: {found:?}");
        assert!(found.iter().all(|(_, reason)| reason.contains("1〜3")), "項目の範囲を名乗る: {found:?}");
        let all = ["1:tooth_a", "2:tooth_b", "3:tooth_c", "3:@1"];
        assert!(teeth_misses(DONE3, &[TOOTH_LINE], &[], &[], &all).is_empty(), "番号が 1〜3 を覆えば（2 本以上の歯も）外れ 0");
        let plain = teeth_misses("番号の無い done", &[TOOTH_LINE], &[], &[], &["1:tooth_a"]);
        assert_eq!(elements_of(&plain), ["done-teeth"], "K が 0 の行が欄を持つことを名指す: {plain:?}");
    }

    /// (3) 検証行の番号（(c)）: @ の番号が 1〜検証行の本数に在ること（0 と本数より大きい番号を名指す）。
    #[test]
    fn done_teeth_table_line_names_numbers_past_the_verify_lines() {
        let verify = [TOOTH_LINE, "cargo xtask check"];
        let found = teeth_misses(DONE3, &verify, &[], &[], &["1:@2", "2:@3", "3:@0"]);
        assert_eq!(elements_of(&found), ["2:@3", "3:@0"], "本数 2 の外の番号だけ: {found:?}");
        assert!(found.iter().all(|(_, reason)| reason.contains("1〜2")), "検証行の範囲を名乗る: {found:?}");
    }

    /// (4) 選ばれ（(d)）: 名の歯と既存の歯が、検証行のどれか 1 本の filter 語に行の一致の型で当たること。部分一致の行は名が語を含めば選ばれ、
    /// `--exact` の行は等しい名だけが選ばれる（語を名の部分に持つだけの名は選ばれない）。
    #[test]
    fn done_teeth_table_select_names_unselected_teeth() {
        let verify = [TOOTH_LINE, "cargo nextest run -p toy --no-tests=fail -- --exact one_exact", "cargo xtask check"];
        let done = "(1) a (2) b (3) c (4) d (5) e";
        let elements = ["1:tooth_a", "2:one_exact", "3:one_exact_more", "4:=unrelated", "5:@3"];
        let found = teeth_misses(done, &verify, &[], &[], &elements);
        assert_eq!(elements_of(&found), ["3:one_exact_more", "4:=unrelated"], "--exact の行の部分一致と、どの行にも当たらない名: {found:?}");
        assert!(found.iter().all(|(_, reason)| reason.contains("撃たれない歯")), "撃たれない歯として名指す: {found:?}");
    }

    /// (5) 仕組み（(f)）: ! の後ろが 4 語のどれかで、place-only は = の項目・new-file は + か ~ の項目か creates・closure は契約表の検査を撃つ検証行を
    /// 行が持つこと（write-set は測る物を要らない）。
    #[test]
    fn done_teeth_table_mechanism_needs_its_measure() {
        let done = "(1) a (2) b (3) c (4) d (5) e";
        let elements = ["1:!write-set", "2:!place-only", "3:!new-file", "4:!closure", "5:!bogus"];
        let bare = teeth_misses(done, &["cargo xtask check"], &["src/a.rs"], &[], &elements);
        assert_eq!(elements_of(&bare), ["2:!place-only", "3:!new-file", "4:!closure", "5:!bogus"], "測る物の無い 3 語と 4 語の外: {bare:?}");
        let check = "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo .";
        let full = teeth_misses(done, &[check], &["=src/a.rs", "+src/new.rs"], &[], &elements);
        assert_eq!(elements_of(&full), ["5:!bogus"], "測る物が在れば 4 語は外れない: {full:?}");
        let moved = teeth_misses("(1) a", &[check], &["~src/old.rs"], &[], &["1:!new-file"]);
        assert!(moved.is_empty(), "~ の項目も new-file の測る物: {moved:?}");
        let created = teeth_misses("(1) a", &[check], &["src/a.rs"], &["src/made.rs"], &["1:!new-file"]);
        assert!(created.is_empty(), "creates も new-file の測る物: {created:?}");
        let word = teeth_misses("(1) a", &["cargo xtask check contracts"], &[], &[], &["1:!closure"]);
        assert_eq!(elements_of(&word), ["1:!closure"], "contracts check の並びでない行は測る物でない: {word:?}");
    }

    /// (6) 在りか（(e)）: base を渡す口（`done_teeth_located`）は、既存の歯の 0 か所（無い歯）と 2 か所（2 か所の名）と、名の歯の 2 か所を名指し、
    /// base に無い名の歯（新しい歯）と 1 か所の歯は名指さない。scope（`--lib`）は数える所を狭める。base を渡さない形の照らし（表の検査の本体）は
    /// 同じ要素を名指さない。
    #[test]
    fn done_teeth_table_located_names_absent_and_twice_placed_teeth() {
        let source = |path: &str, body: &str| Source { path: path.to_owned(), body: Ok(body.to_owned()) };
        let sources = vec![
            source("crates/toy/src/a.rs", "pub fn plain() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn tooth_once() {}\n\n    #[test]\n    fn tooth_twice() {}\n}\n"),
            source("crates/toy/tests/e2e.rs", "#[test]\nfn tooth_twice() {}\n"),
        ];
        let roots = crate::pipe::declaration::fixed_roots();
        let base = Base { sources: &sources, snapshots: &[], tracked: &[], core_crate: "toy", roots: &roots };
        let elements = own(&["1:=tooth_once", "2:=tooth_gone", "3:=tooth_twice", "4:tooth_twice", "5:tooth_fresh", "6:tooth_once"]);
        let found = done_teeth_located(&elements, &own(&[TOOTH_LINE]), &base);
        let named: Vec<&str> = found.iter().map(|miss| miss.element.as_str()).collect();
        assert_eq!(named, ["2:=tooth_gone", "3:=tooth_twice", "4:tooth_twice"], "0 か所の既存の歯と 2 か所の既存の歯・名の歯: {found:?}");
        assert!(found.first().is_some_and(|miss| miss.reason.contains("無い歯")), "0 か所は無い歯: {found:?}");
        let twice = found.get(1).map(|miss| miss.reason.as_str()).unwrap_or_default();
        assert!(twice.contains("2 か所の名") && twice.contains("crates/toy/src/a.rs") && twice.contains("crates/toy/tests/e2e.rs"), "2 か所の path を名乗る: {twice}");
        let lib = own(&["cargo nextest run -p toy --lib --no-tests=fail tooth_"]);
        let narrowed: Vec<String> = done_teeth_located(&elements, &lib, &base).into_iter().map(|miss| miss.element).collect();
        assert_eq!(narrowed, ["2:=tooth_gone"], "--lib は src の 1 か所だけを数える");
        let blind = teeth_misses("(1) a (2) b (3) c (4) d (5) e (6) f", &[TOOTH_LINE], &[], &[], &["1:=tooth_once", "2:=tooth_gone", "3:=tooth_twice", "4:tooth_twice", "5:tooth_fresh", "6:tooth_once"]);
        assert!(blind.is_empty(), "base を渡さない口は在りかを名指さない: {blind:?}");
    }

    /// (2) 変わった行の読み（§66 形 3）: base の同じ doc に同じ id の行が無い行（足された行）と、在って done の字が違う行だけを数える。done が同じ行
    /// （表の読みの後の値）は write-set などの欄を変えても数えず、base に doc が無い周は全行が足された行。
    #[test]
    fn done_teeth_base_changed_counts_added_rows_and_done_changes_only() {
        let before: Vec<(String, String)> = [("a", "(1) x"), ("b", "(1) y"), ("c", "(1) z")].iter().map(|(id, done)| ((*id).to_owned(), (*done).to_owned())).collect();
        let now = [("a", "(1) x"), ("b", "(1) y が変わる"), ("d", "(1) 足した"), ("c", "(1) z")];
        assert_eq!(changed(&now, Some(&before)), [1, 2], "done の字が変わった b と足された d だけ");
        assert_eq!(changed(&now, None), [0, 1, 2, 3], "base に doc が無ければ全行が足された行");
        assert!(changed(&[], Some(&before)).is_empty(), "行の無い doc は変わった行を持たない（消した行は数えない）");
        let row = |over: &[(&str, &str)]| read_rows("t.toml", &full_row(over)).map(|rows| rows.into_iter().map(|row| (row.id, row.done)).collect::<Vec<_>>());
        let (plain, moved) = (row(&[]), row(&[("write-set", "[\"w.rs\"]"), ("verify", "[\"git status\"]"), ("done-teeth", "[\"1:t\"]")]));
        let (plain, moved) = (plain.unwrap_or_default(), moved.unwrap_or_default());
        let borrowed: Vec<(&str, &str)> = moved.iter().map(|(id, done)| (id.as_str(), done.as_str())).collect();
        assert!(changed(&borrowed, Some(&plain)).is_empty(), "done 以外の欄だけを変えた行は数えない: {plain:?} {moved:?}");
    }

    /// (3) 要否（§66 形 3）: 変わった行の done が番号つきの項目を持たなければ done-unnumbered、欄を持たなければ done-teeth-missing（1 行が両方を持つ
    /// こともある）。done を持たない行（約束の行を持つ行）には求めず、全角の番号は項目と読まない。
    #[test]
    fn done_teeth_base_gaps_name_unnumbered_and_missing_rows() {
        let teeth = own(&["1:tooth_a"]);
        assert!(gaps("(1) a (2) b", &own(&["1:tooth_a", "2:tooth_b"])).is_empty(), "番号つきの項目と欄が揃えば外れ 0");
        assert_eq!(gaps("(1) a", &[]), [Gap::Missing], "欄が無い");
        assert_eq!(gaps("番号の無い done", &teeth), [Gap::Unnumbered], "番号が無い");
        assert_eq!(gaps("番号の無い done", &[]), [Gap::Unnumbered, Gap::Missing], "両方");
        assert_eq!(gaps("（１）全角の番号", &[]), [Gap::Unnumbered, Gap::Missing], "全角の番号は項目でない");
        assert!(gaps("", &[]).is_empty(), "done を持たない行（Promised）には求めない");
    }
}
