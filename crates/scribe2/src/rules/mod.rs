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
    /// 承認の受理面の identity。
    DialogueSurface,
    /// 成熟条件（停止・履歴として残す）。
    MaturityCondition,
    /// 口座の選定規則。
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
    /// 席の context 使用率の上限（百分率）。超えると退避以外の編集を止める。
    SeatContextCapPct,
    /// 席の context 窓の宣言値（token）。使用率の分母である。
    SeatContextWindowTokens,
    /// 席の tick を stale と見なす経過時間（秒）。これを超えた席にだけ tick を撃つ。
    SeatTickStaleS,
    /// 席の cycle lock を live と見なす経過時間（秒）。超えた lock は residue として取り直す。
    SeatCycleLockTtlS,
    /// **宣言が名乗れる上限**（ADR-0010 §2.2）。対象 repo の vessel 宣言
    /// `allowed-commands` はこの部分集合でなければ intake が便を起こさない。
    RunnerAllowedCommands,
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
}

/// [`RuleKind`] の全 variant。parity test の母集団である。
pub const ALL: &[RuleKind] = &[
    RuleKind::CoreLines,
    RuleKind::ModuleLines,
    RuleKind::TestSrcRatioPct,
    RuleKind::FnLines,
    RuleKind::FnComplexity,
    RuleKind::FnArgs,
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
    RuleKind::SeatContextCapPct,
    RuleKind::SeatContextWindowTokens,
    RuleKind::SeatTickStaleS,
    RuleKind::SeatCycleLockTtlS,
    RuleKind::RunnerAllowedCommands,
    RuleKind::RepoNonRustExecAllow,
    RuleKind::SeatCycleSettleS,
    RuleKind::SeatCyclePollMs,
    RuleKind::UsageTimeoutS,
    RuleKind::FollowRetries,
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
            Self::SeatContextCapPct => "SeatContextCapPct",
            Self::SeatContextWindowTokens => "SeatContextWindowTokens",
            Self::SeatTickStaleS => "SeatTickStaleS",
            Self::SeatCycleLockTtlS => "SeatCycleLockTtlS",
            Self::RunnerAllowedCommands => "RunnerAllowedCommands",
            Self::RepoNonRustExecAllow => "RepoNonRustExecAllow",
            Self::SeatCycleSettleS => "SeatCycleSettleS",
            Self::SeatCyclePollMs => "SeatCyclePollMs",
            Self::UsageTimeoutS => "UsageTimeoutS",
            Self::FollowRetries => "FollowRetries",
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
            | Self::SeatContextCapPct
            | Self::SeatContextWindowTokens
            | Self::SeatTickStaleS
            | Self::SeatCycleLockTtlS
            | Self::SeatCycleSettleS
            | Self::SeatCyclePollMs
            | Self::UsageTimeoutS
            | Self::FollowRetries => ValueShape::Int,
            Self::DialogueSurface => ValueShape::Str,
            Self::MaturityCondition
            | Self::AccountSelection
            | Self::MutationSurvivalLine
            | Self::CompileShape
            | Self::CompileSeconds => ValueShape::Policy,
            Self::RunnerAllowedCommands | Self::RepoNonRustExecAllow => ValueShape::List,
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
        Ok(())
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
