//! 規則の**種類**を閉じた型で持つ面（憲法 C1「規則はデータ」）。
//!
//! 種類は [`RuleKind`] の閉じた列挙で、値は manifest（`rules/manifest.toml`）が
//! 持つ。`#[non_exhaustive]` は付けない: variant を足したら [`RuleKind::shape`]
//! と [`RuleKind::as_str`] の網羅 `match` が compile error になる形を保つ。
//!
//! 値の型と種類の対応は [`RuleKind::shape`] ただ 1 箇所に置き、wildcard `_` を
//! 書かない（対応の追加漏れを compile 時に落とすため）。

pub mod cli;
pub mod manifest;

use crate::fleet::select::{Model, MODELS};
use crate::headless::{Effort, EFFORTS};
use crate::seat::role::{Capability, Role, ALL as ROLES, CAPABILITIES};
use manifest::{HostManifest, Manifest};
use std::path::{Path, PathBuf};

/// host の面の file 名（`<state_dir>/host.toml`・設計 account-lifecycle.md §2・ADR-0026 §2.1）。
pub const HOST_MANIFEST: &str = "host.toml";

/// host の面の path。**`--state-dir` からだけ解く**（env を読まない・C2.2）。
pub fn host_manifest_path(state_dir: &Path) -> PathBuf {
    state_dir.join(HOST_MANIFEST)
}

/// tracked の面に、state dir が在れば host の面を合わせる（**呼び手が host の面を読む口はこの 1 か所**）。
///
/// state dir を持たない呼び手（hook / polarity 等）は `None` を渡す＝tracked の面だけ（従来どおり）。
pub fn with_state_dir(tracked: Manifest, state_dir: Option<&Path>) -> Result<Manifest, Vec<RuleError>> {
    match state_dir {
        Some(dir) => tracked.with_host(&host_manifest_path(dir)),
        None => Ok(tracked),
    }
}

/// `--rules PATH`（無ければ埋め込み）の tracked の面を読み、[`with_state_dir`] で host の面を合わせる。
pub fn read(rules: Option<&Path>, state_dir: Option<&Path>) -> Result<Manifest, Vec<RuleError>> {
    with_state_dir(rules.map_or_else(Manifest::embedded, Manifest::load)?, state_dir)
}

/// tracked の面の label 列に `<state_dir>/host.toml` の label を足す（[`with_state_dir`] と同じ規則・`seat tick` の口）。
pub fn declared_labels(tracked: &[String], state_dir: &Path) -> Result<Vec<String>, Vec<RuleError>> {
    HostManifest::read(&host_manifest_path(state_dir)).labels_over(tracked)
}

/// 発効した行を 1 つ引く。無い / 不発効の周は理由つきで `Err`（行の無さを既定に倒さない・C1）。
fn enabled_row<'a>(manifest: &'a Manifest, id: &str) -> Result<&'a RuleRow, String> {
    let row = manifest.get(id).ok_or(format!("{id} が無い"))?;
    if !row.enabled {
        return Err(format!("{id} は不発効である"));
    }
    Ok(row)
}

/// 整数の行の値。行が無い / 不発効 / 整数でない周は 3 理由の `Err`（`pipe::cli::int_row` と同じ字面）。
///
/// 読み手は headless（lens の cap）。`pipe::cli` の同形は private で、pub にするには `pipe` 側へ手を入れる
/// ことになる（`s2-07l.272` の柵）——**行の読み手の正本はこちら**で、`pipe` 側は後続で寄せる。
pub fn int_row(manifest: &Manifest, id: &str) -> Result<u64, String> {
    match enabled_row(manifest, id)?.value {
        RuleValue::Int(found) => Ok(found),
        _ => Err(format!("{id} が整数でない")),
    }
}

/// 文字列の行の値。行が無い / 不発効 / 文字列でない周は 3 理由の `Err`（[`int_row`] と同じ極性）。
///
/// 読み手は headless（runner / lens の `runner.model`）と `pipe::ratelimit`（便用の選定の model）の **1 本**。
pub fn str_row<'a>(manifest: &'a Manifest, id: &str) -> Result<&'a str, String> {
    match &enabled_row(manifest, id)?.value {
        RuleValue::Str(found) => Ok(found),
        _ => Err(format!("{id} が文字列でない")),
    }
}

/// 種類が要求する値の形。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueShape {
    /// 閾値（単位は variant の doc コメントが持つ）。
    Int,
    /// 識別子。
    Str,
    /// 散文で書かれた選定規則・検出線の定義。機械は `enabled` だけを読む。
    Policy,
    /// 文字列の列（TOML の string array）。**空は受けない**＝「規則が無い」を
    /// 空 array で表さない（書き間違いを黙って通すと allowlist が空のまま効く）。
    List,
}

/// 規則の種類。憲法 §3 の行と MVP の運用値に 1:1 で対応する。
///
/// variant を足すと [`RuleKind::as_str`] と [`RuleKind::shape`] の網羅 `match` が
/// compile error になるので、種類の追加は必ず手が入る。[`ALL`] の並びが宣言順から
/// ずれた形（並べ替え・重複・**中間**の欠番）は [`crate::order::is_declaration_order`]
/// を通す歯が捕まえる（ADR-0013 §2.2）。ただし **[`ALL`] への足し忘れは機械が検出しない**
/// ——列挙の母集団が `ALL` 自身なので、抜けた variant は parity test の母集団からも消える。
/// 唯一の例外は **manifest 行を伴う**追加で、行の kind は `ALL` を通して解決されるため
/// 未知の kind として `parse` できず落ちる（2026-09-11 実測）。**行を伴わない追加は
/// どの面も受けない**（同日実測: 368/368 が緑のまま）。variant を足したら `ALL` にも
/// 足すこと。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    /// core crate の `src` 配下 `.rs` の総行数の上限（行）。
    CoreLines,
    /// `crates/*/src` 配下 `.rs` 1 file あたりの物理行数の上限（行）。
    ModuleLines,
    /// test 行 / src 行の比の上限（百分率・100 = 比 1.0）。
    TestSrcRatioPct,
    /// 関数 1 本の行数の上限（行）。
    FnLines,
    /// 関数 1 本の認知的複雑度の上限。
    FnComplexity,
    /// 関数 1 本の引数の数の上限。
    FnArgs,
    /// 行の数え方の幅（文字）。これを超える行は ceil(文字数 ÷ 幅) 行に数える（R-C4-1〜3 と上限の余地が同じ式）。
    LineWidth,
    /// 承認の受理面の identity。
    DialogueSurface,
    /// 成熟条件（停止・履歴として残す）。
    MaturityCondition,
    /// session 用の口座選定の閾値（使用率の百分率・未満の口座だけが候補）。便用の規則は閾値を持たない。
    AccountSelection,
    /// 変異生存率の検出線。
    MutationSurvivalLine,
    /// 直接依存の本数の予算（本）。
    DepBudget,
    /// 1 PR で足せる依存の本数（本）。
    DepPerPr,
    /// 依存追加時の増分 check 実測差の許容（ミリ秒）。
    CheckDeltaMs,
    /// compile の決定論的な形。
    CompileShape,
    /// compile 秒数の検出線。
    CompileSeconds,
    /// 1 周の review で回す lens の本数（本）。
    GateLensCount,
    /// 1 周の gate の token 上限（token）。
    GateTokenCap,
    /// hook 1 回の実行予算（ミリ秒）。
    HookBudgetMs,
    /// pipeline の停止猶予（ミリ秒）。
    StopGraceMs,
    /// fleet の lock 再取得間隔（ミリ秒）。
    LockRetryMs,
    /// fleet の lock を stale と見なす経過時間（ミリ秒）。
    LockStaleMs,
    /// hook の timeout（秒）。
    HookTimeoutS,
    /// **宣言が名乗れる上限**（ADR-0010 §2.2）。対象 repo の vessel 宣言
    /// `allowed-commands` はこの部分集合でなければ intake が便を起こさない。
    RunnerAllowedCommands,
    /// **禁じる語列**（ADR-0025 §2.1）。[`Self::RunnerAllowedCommands`] と対で読む**上限側の禁止**で、値は
    /// 空白区切りの語列の配列（先頭語が一致し残りの語をすべて含む command を hook の command guard と intake が
    /// 止める）。vessel 宣言は緩められない（C14）。
    RunnerDeniedCommands,
    /// **tracked な非 Rust 実行物の例外**（ADR-0009 §2.5）。分類器（shebang / 実行 bit /
    /// 拡張子）に当たる path のうち、この列に**完全一致**で載るものだけを `xtask check` が
    /// 通す。定義を緩める代わりに例外を 1 面へ集めるための行である。
    RepoNonRustExecAllow,
    /// 席の cycle が**作り直しと復元を確認する上限**（秒）。超えたら `clear-unconfirmed` /
    /// `restore-unconfirmed` で止まる。
    SeatCycleSettleS,
    /// 席の cycle が**確認を見に行く周期**（ミリ秒）。上限の内でこの刻みで証拠を読み直す。
    SeatCyclePollMs,
    /// 口座残量を聞きに行く子 process の待ち時間の上限（秒）。
    UsageTimeoutS,
    /// land の追随が衝突した便を**起こし直す回数の上限**（回）。値 N = 最大 N 回起こし直す
    /// （N+1 回目の衝突で終端する）。
    FollowRetries,
    /// 変異検査の並列度の**上限**（宣言値）。実効値は受付（設計 gate-cost.md §3.3）が導く。
    GateMutantsJobs,
    /// job 1 つが要る memory の宣言値（MiB）。受付の分母と封じ込めの箱に使う。
    GateJobMemoryMb,
    /// 席と host のために常に残す memory（MiB）。受付はこれを差し引いた空きしか配らない。
    HostReserveMemoryMb,
    /// 受付で枠が空くのを待つ上限（秒）。超えたら並列度 1 で進む（縮退・止めない）。
    GateSlotWaitS,
    /// tmux を立てる歯（e2e の isolated seat）の同時本数（本・設計 gate-cost.md §3.1・`s2-07l.360`）。値の写しは
    /// nextest の test-group `tmux` の `max-threads`（`.config/nextest.toml`）で、`cargo xtask check` が写しの一致と
    /// 配線を manifest と突合する（clippy.toml ↔ R-C4-4.* と同型）。読み手は xtask 側（core は値を消費しない）。
    GateTmuxTestThreads,
    /// 便の scope に付ける CPU の重み（席は既定の重み）。
    GateCpuWeight,
    /// land が着地待ちの列で自分の番を待つ上限（秒）。超えたら待たずに進む（縮退・止めない）。
    PipeLandWaitS,
    /// land の終端が CI の判定を待つ上限（秒・設計 contract-source.md §5）。超えた周は **close しない**
    /// （`unmeasurable` で止める・FailClosed）。
    PipeCiWaitS,
    /// 席の指示文の `{ledger}` が台帳（`bd --readonly`）の子 process を待つ上限（秒）。超えたら数えを返さない。
    LedgerTimeoutS,
    /// 役割ごとの権能（設計 seat-roles.md §3・ADR-0022 §2.2）。値は権能の名の列で、名の集合は
    /// [`crate::seat::role::Capability`] が閉じる（列に無い名は読み込みで拒む）。**1 kind で行が 2 つ**
    /// （id は `role.<役割名>`・役割ごとに 1 行）。
    RoleCapabilities,
    /// 契約の `size` = S の 1 file あたりの増分の見積（行）。契約表の上限の余地（設計 contract-source.md §3）が読む。
    PipeSizeSLines,
    /// 契約の `size` = M の 1 file あたりの増分の見積（行）。
    PipeSizeMLines,
    /// 契約の `size` = L の 1 file あたりの増分の見積（行）。
    PipeSizeLLines,
    /// runner / lens が claude に**毎回**渡す model（設計 pipeline.md §6・`s2-07l.297`）。値は claude CLI の別名
    /// （閉じた表は [`crate::fleet::select::Model`]）。便用の口座選定はこの model のモデル別窓だけを数える。
    RunnerModel,
    /// runner / lens が claude に**毎回**渡す effort（設計 pipeline.md §6・`s2-07l.322`）。値は claude CLI の字面
    /// （閉じた表は [`crate::headless::Effort`]）。省くと口座の設定 dir の `settings.json` の値で決まる。
    RunnerEffort,
    /// 役割ごとの既定の model（設計 seat-roles.md §19・`s2-07l.433`）。値は claude CLI の別名か表示名
    /// （閉じた表は [`crate::fleet::select::Model`]・表に無い字面は読み込みで拒む）。**1 kind で行は役割ごとに
    /// 1 つ**（id は `seat.model.<役割名>`）で、[`Self::RoleEffort`] と対で読む。
    RoleModel,
    /// 役割ごとの既定の effort（設計 seat-roles.md §19・`s2-07l.433`）。値は claude CLI の字面（閉じた表は
    /// [`crate::headless::Effort`]・表に無い字面は読み込みで拒む）。id は `seat.effort.<役割名>`。
    RoleEffort,
}

/// [`RuleKind`] の全 variant。parity test の母集団である。
pub const ALL: &[RuleKind] = &[
    RuleKind::CoreLines,
    RuleKind::ModuleLines,
    RuleKind::TestSrcRatioPct,
    RuleKind::FnLines,
    RuleKind::FnComplexity,
    RuleKind::FnArgs,
    RuleKind::LineWidth,
    RuleKind::DialogueSurface,
    RuleKind::MaturityCondition,
    RuleKind::AccountSelection,
    RuleKind::MutationSurvivalLine,
    RuleKind::DepBudget,
    RuleKind::DepPerPr,
    RuleKind::CheckDeltaMs,
    RuleKind::CompileShape,
    RuleKind::CompileSeconds,
    RuleKind::GateLensCount,
    RuleKind::GateTokenCap,
    RuleKind::HookBudgetMs,
    RuleKind::StopGraceMs,
    RuleKind::LockRetryMs,
    RuleKind::LockStaleMs,
    RuleKind::HookTimeoutS,
    RuleKind::RunnerAllowedCommands,
    RuleKind::RunnerDeniedCommands,
    RuleKind::RepoNonRustExecAllow,
    RuleKind::SeatCycleSettleS,
    RuleKind::SeatCyclePollMs,
    RuleKind::UsageTimeoutS,
    RuleKind::FollowRetries,
    RuleKind::GateMutantsJobs,
    RuleKind::GateJobMemoryMb,
    RuleKind::HostReserveMemoryMb,
    RuleKind::GateSlotWaitS,
    RuleKind::GateTmuxTestThreads,
    RuleKind::GateCpuWeight,
    RuleKind::PipeLandWaitS,
    RuleKind::PipeCiWaitS,
    RuleKind::LedgerTimeoutS,
    RuleKind::RoleCapabilities,
    RuleKind::PipeSizeSLines,
    RuleKind::PipeSizeMLines,
    RuleKind::PipeSizeLLines,
    RuleKind::RunnerModel,
    RuleKind::RunnerEffort,
    RuleKind::RoleModel,
    RuleKind::RoleEffort,
];

impl RuleKind {
    /// manifest の `kind` に書く字面（variant 名と一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CoreLines => "CoreLines",
            Self::ModuleLines => "ModuleLines",
            Self::TestSrcRatioPct => "TestSrcRatioPct",
            Self::FnLines => "FnLines",
            Self::FnComplexity => "FnComplexity",
            Self::FnArgs => "FnArgs",
            Self::LineWidth => "LineWidth",
            Self::DialogueSurface => "DialogueSurface",
            Self::MaturityCondition => "MaturityCondition",
            Self::AccountSelection => "AccountSelection",
            Self::MutationSurvivalLine => "MutationSurvivalLine",
            Self::DepBudget => "DepBudget",
            Self::DepPerPr => "DepPerPr",
            Self::CheckDeltaMs => "CheckDeltaMs",
            Self::CompileShape => "CompileShape",
            Self::CompileSeconds => "CompileSeconds",
            Self::GateLensCount => "GateLensCount",
            Self::GateTokenCap => "GateTokenCap",
            Self::HookBudgetMs => "HookBudgetMs",
            Self::StopGraceMs => "StopGraceMs",
            Self::LockRetryMs => "LockRetryMs",
            Self::LockStaleMs => "LockStaleMs",
            Self::HookTimeoutS => "HookTimeoutS",
            Self::RunnerAllowedCommands => "RunnerAllowedCommands",
            Self::RunnerDeniedCommands => "RunnerDeniedCommands",
            Self::RepoNonRustExecAllow => "RepoNonRustExecAllow",
            Self::SeatCycleSettleS => "SeatCycleSettleS",
            Self::SeatCyclePollMs => "SeatCyclePollMs",
            Self::UsageTimeoutS => "UsageTimeoutS",
            Self::FollowRetries => "FollowRetries",
            Self::GateMutantsJobs => "GateMutantsJobs",
            Self::GateJobMemoryMb => "GateJobMemoryMb",
            Self::HostReserveMemoryMb => "HostReserveMemoryMb",
            Self::GateSlotWaitS => "GateSlotWaitS",
            Self::GateTmuxTestThreads => "GateTmuxTestThreads",
            Self::GateCpuWeight => "GateCpuWeight",
            Self::PipeLandWaitS => "PipeLandWaitS",
            Self::PipeCiWaitS => "PipeCiWaitS",
            Self::LedgerTimeoutS => "LedgerTimeoutS",
            Self::RoleCapabilities => "RoleCapabilities",
            Self::PipeSizeSLines => "PipeSizeSLines",
            Self::PipeSizeMLines => "PipeSizeMLines",
            Self::PipeSizeLLines => "PipeSizeLLines",
            Self::RunnerModel => "RunnerModel",
            Self::RunnerEffort => "RunnerEffort",
            Self::RoleModel => "RoleModel",
            Self::RoleEffort => "RoleEffort",
        }
    }

    /// 種類が要求する値の形。**対応はこの `match` ただ 1 箇所**が持つ。
    pub fn shape(self) -> ValueShape {
        match self {
            Self::CoreLines
            | Self::ModuleLines
            | Self::TestSrcRatioPct
            | Self::FnLines
            | Self::FnComplexity
            | Self::FnArgs
            | Self::LineWidth
            | Self::DepBudget
            | Self::DepPerPr
            | Self::CheckDeltaMs
            | Self::GateLensCount
            | Self::GateTokenCap
            | Self::HookBudgetMs
            | Self::StopGraceMs
            | Self::LockRetryMs
            | Self::LockStaleMs
            | Self::HookTimeoutS
            | Self::SeatCycleSettleS
            | Self::SeatCyclePollMs
            | Self::UsageTimeoutS
            | Self::FollowRetries
            | Self::GateMutantsJobs
            | Self::GateJobMemoryMb
            | Self::HostReserveMemoryMb
            | Self::GateSlotWaitS
            | Self::GateTmuxTestThreads
            | Self::GateCpuWeight
            | Self::PipeLandWaitS
            | Self::PipeCiWaitS
            | Self::LedgerTimeoutS
            | Self::PipeSizeSLines
            | Self::PipeSizeMLines
            | Self::PipeSizeLLines
            | Self::AccountSelection => ValueShape::Int,
            Self::DialogueSurface
            | Self::RunnerModel
            | Self::RunnerEffort
            | Self::RoleModel
            | Self::RoleEffort => ValueShape::Str,
            Self::MaturityCondition
            | Self::MutationSurvivalLine
            | Self::CompileShape
            | Self::CompileSeconds => ValueShape::Policy,
            Self::RunnerAllowedCommands
            | Self::RunnerDeniedCommands
            | Self::RepoNonRustExecAllow
            | Self::RoleCapabilities => ValueShape::List,
        }
    }

    /// manifest の `kind` の字面から種類を引く。未知なら `None`。
    pub fn parse(text: &str) -> Option<Self> {
        ALL.iter().copied().find(|kind| kind.as_str() == text)
    }
}

/// 規則の値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleValue {
    /// 閾値。
    Int(u64),
    /// 識別子。
    Str(String),
    /// 散文の規則本文。
    Policy(String),
    /// 文字列の列（順序は manifest の並びのまま＝機械が読む順序である）。
    List(Vec<String>),
}

impl RuleValue {
    /// この値の形。
    pub fn shape(&self) -> ValueShape {
        match *self {
            Self::Int(_) => ValueShape::Int,
            Self::Str(_) => ValueShape::Str,
            Self::Policy(_) => ValueShape::Policy,
            Self::List(_) => ValueShape::List,
        }
    }

    /// 1 行で表示する形（CLI の `rules get` が使う）。
    pub fn render(&self) -> String {
        match self {
            Self::Int(value) => value.to_string(),
            Self::Str(text) | Self::Policy(text) => text.clone(),
            // **1 行で区切りが読める形**にする（要素を空白で継ぐと、空白を含む
            // 要素〔共通 verify の 1 行〕が何本あるのか読めなくなる）。
            Self::List(items) => {
                let quoted: Vec<String> = items.iter().map(|item| format!("\"{item}\"")).collect();
                format!("[{}]", quoted.join(", "))
            }
        }
    }
}

/// manifest の 1 行。`ruling` / `ruled_at` は全行必須である（憲法 C5）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRow {
    /// 行 id。憲法 §3 の行 id が接頭辞として一致する。
    pub id: String,
    /// 規則の種類。
    pub kind: RuleKind,
    /// 規則の値。
    pub value: RuleValue,
    /// false = 値は写すが機械は効かせない。
    pub enabled: bool,
    /// 裁定 id。
    pub ruling: String,
    /// 裁定の日付。
    pub ruled_at: String,
    /// manifest の中でこの行が始まる物理行番号。
    pub line: u64,
}

/// 規則 1 行が満たすべき性質。
pub trait Rule {
    /// この行の種類。
    fn kind(&self) -> RuleKind;
    /// 値の形と裁定の記入を検査する。
    fn validate(&self) -> Result<(), RuleError>;
}

impl Rule for RuleRow {
    fn kind(&self) -> RuleKind {
        self.kind
    }

    fn validate(&self) -> Result<(), RuleError> {
        let want = self.kind.shape();
        let got = self.value.shape();
        if want != got {
            return Err(RuleError::new(
                self.line,
                format!(
                    "{} の value が kind {} の形と合わない（要 {want:?}・実 {got:?}）",
                    self.id,
                    self.kind.as_str()
                ),
            ));
        }
        if self.ruling.is_empty() || self.ruled_at.is_empty() {
            return Err(RuleError::new(
                self.line,
                format!("{} に ruling / ruled_at が無い", self.id),
            ));
        }
        self.names_are_known()
    }
}

impl RuleRow {
    /// 値が**閉じた名の集合**を指す kind は、名を core の enum で引けることまで検査する（設計
    /// seat-roles.md §3 / §19・ADR-0022 §2.2）: `RoleCapabilities` の列は [`Capability`] の名、`DialogueSurface`
    /// の値は [`Role`] の名、`RoleModel` の値は [`Model`] の字面、`RoleEffort` の値は [`Effort`] の字面。
    /// 綴り違いを黙って「権能なし」「対話面なし」「既定なし」に倒さない（NFR4）。
    /// `RunnerDeniedCommands` の各要素は語を 1 つ以上持つ（空白だけの語列は何にも当たらず黙って効かない・ADR-0025 §2.1）。
    fn names_are_known(&self) -> Result<(), RuleError> {
        let unknown = |what: &str, name: &str, taken: &[&str]| {
            RuleError::new(
                self.line,
                format!("{} の value に未知の{what} {name}（取るのは {}）", self.id, taken.join(" / ")),
            )
        };
        match (self.kind, &self.value) {
            (RuleKind::RoleCapabilities, RuleValue::List(names)) => {
                let taken: Vec<&str> = CAPABILITIES.iter().map(|found| found.as_str()).collect();
                match names.iter().find(|name| Capability::parse(name).is_none()) {
                    Some(name) => Err(unknown("権能", name, &taken)),
                    None => Ok(()),
                }
            }
            (RuleKind::DialogueSurface, RuleValue::Str(name)) if Role::parse(name).is_none() => {
                let taken: Vec<&str> = ROLES.iter().map(|found| found.as_str()).collect();
                Err(unknown("役割", name, &taken))
            }
            (RuleKind::RoleModel, RuleValue::Str(name)) if Model::parse(name).is_none() => {
                let taken: Vec<&str> = MODELS.iter().map(|found| found.alias()).collect();
                Err(unknown("model", name, &taken))
            }
            (RuleKind::RoleEffort, RuleValue::Str(name)) if Effort::parse(name).is_none() => {
                let taken: Vec<&str> = EFFORTS.iter().map(|found| found.alias()).collect();
                Err(unknown("effort", name, &taken))
            }
            (RuleKind::RunnerDeniedCommands, RuleValue::List(sequences)) => {
                match sequences.iter().find(|sequence| sequence.split_whitespace().next().is_none()) {
                    Some(blank) => Err(RuleError::new(
                        self.line,
                        format!("{} の value に語を持たない語列 {blank:?}（各要素は空白区切りの語 1 つ以上）", self.id),
                    )),
                    None => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }
}

/// 1 件 1 行で表示する読み取り error。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleError {
    /// 違反が始まる物理行番号。
    pub line: u64,
    /// 違反の説明。
    pub message: String,
}

impl RuleError {
    /// 行番号と説明から作る。
    pub fn new(line: u64, message: String) -> Self {
        Self { line, message }
    }
}

impl std::fmt::Display for RuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rules: {} line={}", self.message, self.line)
    }
}
