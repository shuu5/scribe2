//! vessel 宣言（対象 repo の root の `.vessel.toml`）を読み、器の上限と突き合わせ、便ごとに
//! 凍結する（設計 docs/design/pipeline.md §5.1・ADR-0010 §2.1 / §2.3 / §2.4）。
//!
//! **読むのは HEAD commit の tree** で、作業ツリーは読まない——未 commit の宣言は存在しない
//! のと同じである。宣言の変更が対象 repo の PR として review を通る形を、読み口の側で強制する。
//!
//! 値は 3 段で持つ（憲法 C10）: [`Declared`]（書かれていた値）→ [`Sourced`]（出所つき＝読んだ
//! commit・宣言 path・上限行 id）→ [`Effective`]（上限と突き合わせて有効になった値）。
//! `Effective` は `Sourced` を消費してしか作れない＝宣言は**実測を経てしか効かない**。
//! `pipe::gate` の `Measured`（1 便の verify の実測量）とは別物である。
//!
//! 値の受理集合と配列の層は [`crate::rules::manifest`] と共有する（第 2 の parser を作らない）。

use crate::hook::command::denied_in;
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::{list, scalar, Scalar};
use std::path::Path;

pub mod path_kinds;
mod write_set;

pub use write_set::{headroom_shortfalls, line_count, read_write_set, Caps, FileLines, Headroom, NewFilePolicy, WriteSetItem, CORE};
pub(crate) use write_set::is_under;

/// 対象 repo の root に置く宣言 file の名。
pub const DECL_FILE: &str = ".vessel.toml";

/// 上限を持つ rules 行の id。
pub const CEILING_ROW: &str = "runner.allowed_commands";

/// 禁じる語列を持つ rules 行の id（上限と**対**で読む・ADR-0025 §2.1・判定は hook の command guard と同じ 1 関数）。
pub const DENIED_ROW: &str = crate::hook::command::ROW;

/// 宣言 file の schema。
const SCHEMA_VERSION: u64 = 1;

/// 宣言が持つ key（この順で報告する）。path の種別の任意 key 3 本（[`path_kinds::KEYS`]・ADR-0047）は
/// 既存の任意 key と同じ読み口で読む（schema は 1 のまま・key の追加と不在＝既定は版を上げない）。
const DECLARED_KEYS: &[&str] = &[
    "schema",
    "allowed-commands",
    "common-verify",
    DETECTION_KEY,
    REQUIREMENTS_KEY,
    REMOTE_KEY,
    CI_CMD_KEY,
    path_kinds::DESIGN_INTENT_KEY,
    path_kinds::DESIGN_DOC_KEY,
    path_kinds::TESTS_KEY,
];

/// **push 先の remote の名**の key（任意・設計 contract-source.md §5「land の終端」）。
///
/// **既定は持たない**。push は repo の外へ出す行為（憲法 A1 の「出す」）なので、押す先を宣言していない
/// repo に器が勝手な既定で押すことはしない——宣言の無い repo の便は終端を持たない（`--pr-cmd` 形と同じ）。
const REMOTE_KEY: &str = "remote";

/// **CI の判定を読む 1 行**の key（任意・設計 contract-source.md §5）。書かない宣言は [`DEFAULT_CI_CMD`] を撃つ。
const CI_CMD_KEY: &str = "ci-cmd";

/// CI の判定を読む行の既定（forge の CLI・`{sha}` に着地した sha が入る）。`event` は読み手が
/// `schedule` の run を母集団から外すための欄（設計 pipeline.md §46）。
pub const DEFAULT_CI_CMD: &str = "gh run list --commit {sha} --json status,conclusion,event";

/// CI の行が必ず持つ穴（**着地した commit を名指さない行は撃てない**・別の commit の判定を読むことになる）。
pub const CI_SHA_HOLE: &str = "{sha}";

/// **要件面の path** の key（任意・設計 contract-source.md §2「表の検査」）。契約表の `req` の id をこの file で
/// 測る。書かない宣言は [`DEFAULT_REQUIREMENTS`] を読む（既存の宣言を 1 行も変えさせない）。
const REQUIREMENTS_KEY: &str = "requirements";

/// 要件面の既定 path（宣言 `requirements` が無い周）。
pub const DEFAULT_REQUIREMENTS: &str = "design-intent/spec/srs.html";

/// 便の写しが持つ key（宣言の 4 つ + 出所 3 つ）。
const EFFECTIVE_KEYS: &[&str] = &[
    "schema",
    "allowed-commands",
    "common-verify",
    DETECTION_KEY,
    "commit",
    "source",
    "ceiling",
];

/// **検出線の行の列**の key（設計 gate-cost.md §5・ADR-0021 §2.4）。
///
/// 検出線（C12.4）は落ちても deny しない行で、木が gate と同じ main 実測では撃ち直さない。
/// deny する行（`common-verify`）と置き場を分けるのはそのためである。
const DETECTION_KEY: &str = "detection-verify";

/// **書かなくてよい** key（無ければ空）。書いた周の空配列は従来どおり不備である（ADR-0010 §2.1）。
///
/// 任意にするのは、検出線を持たない consumer（toy repo 等）の宣言を 1 行も変えさせないためである。
const OPTIONAL_KEYS: &[&str] = &[
    DETECTION_KEY,
    REQUIREMENTS_KEY,
    REMOTE_KEY,
    CI_CMD_KEY,
    path_kinds::DESIGN_INTENT_KEY,
    path_kinds::DESIGN_DOC_KEY,
    path_kinds::TESTS_KEY,
];

/// shell が意味を変える文字。**1 行 1 command の粒度**はここで守る——gate と land は行を
/// `sh -c` で撃つので、先頭語だけを見ても包みや連結を止められない（ADR-0010 §2.3）。
const METACHARS: &[char] = &[';', '&', '|', '`', '$', '(', ')', '<', '>', '"', '\''];

/// 宣言が読めない / 撃てない理由。**行番号を必ず持つ**（0 は file 全体）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclError {
    /// 何行目か（1 始まり・0 は file 全体）。
    pub line: u64,
    /// なぜ読めない / 撃てないか。
    pub reason: String,
}

impl DeclError {
    /// 1 件を組む。
    fn new(line: u64, reason: String) -> Self {
        Self { line, reason }
    }
}

impl std::fmt::Display for DeclError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "vessel: {} line={}", self.reason, self.line)
    }
}

/// 器が持つ上限（rules 行）。**組み立ては `pipe::cli` の 2 か所だけ**（intake の `freeze` と `contracts check`）＝値は
/// `--rules` の経路で読んだ manifest から来る（C2.2・埋め込みを直に読まない）。
pub struct Ceiling<'a> {
    /// 上限を持つ行の id（出所として写しへ残る）。
    pub row: &'a str,
    /// 許す command の上限。
    pub commands: &'a [String],
    /// 禁じる語列（rules 行 [`DENIED_ROW`]・上限と対で読む）。
    pub denied: &'a [String],
}

/// verify 行を測る基準（宣言の allowlist と禁じる語列）。
pub struct Basis<'a> {
    /// 先頭語の基準（**宣言の** allowlist・上限ではない）。
    pub allowed: &'a [String],
    /// 禁じる語列（rules 行 [`DENIED_ROW`]）。
    pub denied: &'a [String],
}

/// 便の base を置く穴。
pub const BASE_HOLE: &str = "{base}";

/// **実効 jobs を置く穴**（設計 gate-cost.md §3.3・ADR-0021 §2.1）。並列度は env で渡さない
/// （C2.2 の精神・折り返しの裏口を作らない）ので、宣言の行に穴として書く。
pub const JOBS_HOLE: &str = "{jobs}";

/// **job 1 つに許す thread を置く穴**（設計 gate-cost.md §31・ADR-0050）。値段は器の受付が cores の
/// 実測と `gate.mutants_jobs` から決め（[`crate::pipe::admission::Cpu`]）、道具は受けた値をそのまま
/// test 走行へ渡すだけである（cores からの導出を道具に持たせない＝導出の正本は器の 1 か所）。
pub const THREADS_HOLE: &str = "{threads}";

/// **契約の verify 行が名指した歯の filter 語を置く穴**（設計 gate-cost.md §34 約束 4 / 5・ADR-0052）。語は器が
/// 契約の verify 行から導き `,` で結んで置く（0 本は `-`）。道具は契約を読まない（導出の正本は器の 1 か所・C2.2）。
pub const TEETH_HOLE: &str = "{teeth}";

/// **宣言の共通 verify に置ける穴の閉じた集合**（4 つちょうど・末尾が [`TEETH_HOLE`]）。
///
/// ADR-0010 §2.1 は穴を `{base}` 1 つと定めたが、ADR-0021 §2.1 がそれを部分 supersede して
/// 集合にした。集合をここ 1 本に閉じるのは、[`unfit`] の判定と gate の置換が**同じ列**を
/// 見るためである——片方だけに穴を足すと、intake を通った行が gate で置換されないまま
/// 撃たれる（`{jobs}` という語をそのまま `--jobs` へ渡す）。
pub const BASE_HOLES: &[&str] = &[BASE_HOLE, JOBS_HOLE, THREADS_HOLE, TEETH_HOLE];

/// 行が置ける穴。**穴の可否だけが宣言の共通 verify と契約の verify の違い**である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holes {
    /// [`BASE_HOLES`] の穴だけ置ける（宣言の共通 verify）。
    Base,
    /// 穴を置けない（契約の verify）。
    None,
}

/// この境界の極性: intake（起動の前）で断り、宣言を読めない周は断る側へ倒す（未 commit の宣言は存在しないのと同じ）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 行が argv 1 本として撃てない理由。**新しい理由は variant を 1 つ足す**（憲法 C2）。
///
/// **この並びが [`unfit`] の適用順序であり、唯一の権威**である（同じ行が複数の理由に当たる
/// 周は、この並びで最初の理由を返す）。並びと適用順が一致することは
/// [`crate::order::is_declaration_order`] を [`Self::rank`] へ通す歯が測る（ADR-0013 §2.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unfit {
    /// shell が意味を変える文字を含む。
    Metachar(char),
    /// 語が 1 つも無い。
    Empty,
    /// 先頭語が宣言の allowlist に無い。
    Command(String),
    /// repo の外を指す語を含む（絶対 path・home の短縮記号・`..` で遡る path）。
    Outside(String),
    /// 置けない穴を含む。
    Hole(String),
    /// 禁じる語列（rules 行 [`DENIED_ROW`]）に当たる（ADR-0025 §2.3・hook の command guard と同じ判定）。
    Denied(String),
}

impl Unfit {
    /// 断る理由の 1 行。
    fn reason(&self, allowed: &[String]) -> String {
        match *self {
            Self::Metachar(found) => format!(
                "shell の制御文字 {found:?} を含む（1 行 1 command・包みも連結も書けない）"
            ),
            Self::Empty => "verify 行が空である".to_owned(),
            Self::Command(ref head) => {
                format!("先頭 command {head} が宣言の allowed-commands（{}）に無い", allowed.join(" / "))
            }
            Self::Outside(ref word) => {
                format!("repo の外を指す語 {word} を含む（絶対 path・home の短縮記号・.. で遡る path）")
            }
            Self::Hole(ref hole) => format!("置けない穴 {hole} を含む"),
            Self::Denied(ref sequence) => {
                format!("禁じる語列 {sequence} に当たる（rules 行 {DENIED_ROW}・N1 / C16・gate と land が sh -c で実走する行）")
            }
        }
    }

    /// 宣言順の位置（0 始まり）。判別子を `as` で取れない payload つき enum の
    /// [`crate::order::is_declaration_order`] 用の写像で、外形には出さない。
    #[cfg(test)]
    fn rank(&self) -> usize {
        match *self {
            Self::Metachar(_) => 0,
            Self::Empty => 1,
            Self::Command(_) => 2,
            Self::Outside(_) => 3,
            Self::Hole(_) => 4,
            Self::Denied(_) => 5,
        }
    }
}

/// 入口の flip を撃つ行の先頭語列（設計 pipeline.md §7・本 repo の `.vessel.toml` の 1 本目の行の頭）。
const ENTRANCE_FLIP_WORDS: &[&str] = &["cargo", "xtask", "flip-check"];

/// 宣言の `common-verify` の 1 行の種類。**先頭語列だけ**で閉じる（設計 pipeline.md §7 約束 4・`s2-07l.170`）。
///
/// 並びは分類の適用順（先に当たる側が勝つ）: 入口の flip は先頭語 `cargo` の行でもあるので先に取る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyKind {
    /// 入口の flip（先頭語列 [`ENTRANCE_FLIP_WORDS`]）。
    EntranceFlip,
    /// 先頭語 `cargo` の他の行（Rust の repo の検証）。
    Cargo,
    /// 先頭語が `cargo` でない行（`sh` / `git` 等＝Rust 固有の検査を内蔵しない側）。
    Other,
}

impl VerifyKind {
    /// 1 行を先頭語列で分類する（語は空白で割る・[`unfit`] が 1 行 1 command を先に保証する）。
    pub fn of(line: &str) -> Self {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.starts_with(ENTRANCE_FLIP_WORDS) {
            Self::EntranceFlip
        } else if words.first() == ENTRANCE_FLIP_WORDS.first() {
            Self::Cargo
        } else {
            Self::Other
        }
    }
}

/// 宣言の行の**分類**が断る理由（行ごとの [`Unfit`] とは別の面＝行の列の全体で決まる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindGap {
    /// 先頭語 `cargo` の行を持ちながら、入口の flip を撃つ行を持たない（TDD の flip を測らない Rust の宣言）。
    NoEntranceRed,
}

impl KindGap {
    /// 断る理由の 1 行（variant の名を先頭に置く）。
    fn reason(self) -> String {
        match self {
            Self::NoEntranceRed => format!(
                "NoEntranceRed: common-verify に先頭語 cargo の行が在るのに入口の flip（{} --base {BASE_HOLE}）を撃つ行が無い",
                ENTRANCE_FLIP_WORDS.join(" ")
            ),
        }
    }
}

/// `common-verify` の行の列の分類が断る理由。先頭語 `cargo` の行を 1 本も持たない宣言（Rust でない toy repo）は
/// 分類だけで断らない（§7「Rust 固有の検査を内蔵しない」のまま）。
pub fn kind_gap(lines: &[String]) -> Option<KindGap> {
    let kinds: Vec<VerifyKind> = lines.iter().map(|line| VerifyKind::of(line)).collect();
    let rust = kinds.iter().any(|kind| *kind != VerifyKind::Other);
    (rust && !kinds.contains(&VerifyKind::EntranceFlip)).then_some(KindGap::NoEntranceRed)
}

/// 書かれていた宣言の値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    /// 許してよい command。
    allowed: Vec<String>,
    /// どの便でも撃つ検証行。
    common_verify: Vec<String>,
    /// 検出線の行（任意・無ければ空）。
    detection_verify: Vec<String>,
    /// `allowed-commands` が書かれていた行。
    allowed_line: u64,
    /// `common-verify` が書かれていた行。
    common_line: u64,
    /// `detection-verify` が書かれていた行（無ければ 0）。
    detection_line: u64,
    /// 要件面の repo 相対 path（任意・無ければ `None`）。
    requirements: Option<String>,
    /// push 先の remote の名（任意・無ければ `None`）。
    remote: Option<String>,
    /// CI の判定を読む 1 行（任意・無ければ `None`）。
    ci_cmd: Option<String>,
    /// path の種別の prefix（任意 key 3 本・無い key は `None`・設計 seat-roles.md §24）。
    path_kinds: path_kinds::DeclaredPaths,
}

/// 出所つきの宣言。**[`Effective`] はこれを消費してしか作れない**（C10）。
pub struct Sourced {
    /// 書かれていた値。
    declared: Declared,
    /// 読んだ commit の sha。
    commit: String,
    /// 宣言 file の repo 相対 path。
    source: String,
    /// 突き合わせる上限行の id。
    ceiling: String,
}

/// 上限と突き合わせて有効になった値。便ごとに凍結され、以後の段はこれだけを読む。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    /// 許す command。
    allowed: Vec<String>,
    /// どの便でも撃つ検証行。
    common_verify: Vec<String>,
    /// 検出線の行（無ければ空）。
    detection_verify: Vec<String>,
    /// 読んだ commit の sha。
    commit: String,
    /// 宣言 file の repo 相対 path。
    source: String,
    /// 突き合わせた上限行の id。
    ceiling: String,
}

/// 走査中の 1 key の値。
enum Raw {
    /// 文字列。
    Text(String),
    /// 非負整数。
    Int(u64),
    /// 文字列の列。
    List(Vec<String>),
}

/// repo の HEAD から宣言を読み、上限・宣言 allowlist・契約 verify と突き合わせる。
///
/// **不備は全件返す**（1 件目で止めると直すたびに次の 1 件が出る）。
pub fn measure(
    repo: &Path,
    ceiling: &Ceiling<'_>,
    contract_verify: &[String],
) -> Result<Effective, Vec<DeclError>> {
    Sourced::read(repo, ceiling)?.measure(ceiling, contract_verify)
}

/// HEAD commit の tree の宣言を読む（**読み手は 1 本**・作業ツリーは読まない＝commit されていない宣言は
/// 無いのと同じ）。宣言 file が無い（HEAD に無い・git を撃てない）周は `None`、在って読めない周は `Some(Err)`
/// （path の種別の読み手 [`path_kinds::PathKinds::read_at_head`] はこの 2 つを別の state にする・§24）。
fn head_declaration(repo: &Path) -> Option<Result<Declared, Vec<DeclError>>> {
    let spec = format!("HEAD:{DECL_FILE}");
    let bytes = super::git_bytes(repo, &["show", &spec])?;
    Some(Declared::parse(&String::from_utf8_lossy(&bytes)))
}

/// [`head_declaration`] の、宣言 file が無い周も不備として返す形。上限と突き合わせる [`Sourced::read`] と、終端が
/// 読む [`terminal_facts`] が共有する。
fn declared_at_head(repo: &Path) -> Result<Declared, Vec<DeclError>> {
    head_declaration(repo).unwrap_or_else(|| {
        Err(vec![DeclError::new(
            0,
            format!("HEAD の tree に {DECL_FILE} が無い（作業ツリーの宣言は読まない＝commit されていない宣言は無いのと同じ）"),
        )])
    })
}

impl Sourced {
    /// HEAD commit の tree から宣言を読む。**作業ツリーは読まない**。
    fn read(repo: &Path, ceiling: &Ceiling<'_>) -> Result<Self, Vec<DeclError>> {
        let commit = super::head_of(repo).ok_or_else(|| {
            vec![DeclError::new(0, format!("{} の HEAD を読めない", repo.display()))]
        })?;
        let declared = declared_at_head(repo)?;
        Ok(Self {
            declared,
            commit,
            source: DECL_FILE.to_owned(),
            ceiling: ceiling.row.to_owned(),
        })
    }

    /// 上限・宣言 allowlist・禁じる語列・契約 verify と突き合わせる。
    fn measure(
        self,
        ceiling: &Ceiling<'_>,
        contract_verify: &[String],
    ) -> Result<Effective, Vec<DeclError>> {
        let declared = &self.declared;
        let mut errors = Vec::new();
        for command in &declared.allowed {
            if !ceiling.commands.iter().any(|top| top == command) {
                errors.push(DeclError::new(
                    declared.allowed_line,
                    format!(
                        "allowed-commands の {command} が上限 {}（{}）の外である",
                        self.ceiling,
                        ceiling.commands.join(" / ")
                    ),
                ));
            }
        }
        let basis = Basis { allowed: &declared.allowed, denied: ceiling.denied };
        check_lines("common-verify", &declared.common_verify, declared.common_line, &basis, &mut errors);
        // **入口の flip の不在**は行ごとの検査では見えない（どの行も撃てる形のまま、flip の行だけが無い）。
        if let Some(gap) = kind_gap(&declared.common_verify) {
            errors.push(DeclError::new(declared.common_line, gap.reason()));
        }
        // **検出線の行にも同じ検査を掛ける**（ADR-0010 §2.3 (2)・ADR-0021 §2.6・lens-132d H1）。
        // 掛けないと、共通 verify で断った迂回行を検出線の側へ置くだけで撃たせられる。
        check_lines(DETECTION_KEY, &declared.detection_verify, declared.detection_line, &basis, &mut errors);
        check_contract(contract_verify, &basis, &mut errors);
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Effective {
            allowed: declared.allowed.clone(),
            common_verify: declared.common_verify.clone(),
            detection_verify: declared.detection_verify.clone(),
            commit: self.commit,
            source: self.source,
            ceiling: self.ceiling,
        })
    }
}

/// 宣言の行の列（共通 verify・検出線）を全件見る。**どちらも [`BASE_HOLES`] の穴を置ける**。
fn check_lines(key: &str, lines: &[String], at: u64, basis: &Basis<'_>, errors: &mut Vec<DeclError>) {
    for line in lines {
        if let Some(found) = unfit(line, basis, Holes::Base) {
            errors.push(DeclError::new(
                at,
                format!("{key} {line:?}: {}", found.reason(basis.allowed)),
            ));
        }
    }
}

/// 契約の verify を全件見る。**基準は宣言の allowlist**（上限ではない）で、契約行は穴を持てない。
fn check_contract(lines: &[String], basis: &Basis<'_>, errors: &mut Vec<DeclError>) {
    for (index, line) in lines.iter().enumerate() {
        if let Some(found) = unfit(line, basis, Holes::None) {
            errors.push(DeclError::new(
                0,
                format!(
                    "契約の verify {} 本目 {line:?}: {}",
                    index.saturating_add(1),
                    found.reason(basis.allowed)
                ),
            ));
        }
    }
}

/// 1 行が **argv 1 本**として撃てるかを見る。**この 1 本が唯一の判定**である。
fn unfit(line: &str, basis: &Basis<'_>, holes: Holes) -> Option<Unfit> {
    let allowed = basis.allowed;
    if let Some(found) = line
        .chars()
        .find(|found| METACHARS.contains(found) || found.is_ascii_control())
    {
        return Some(Unfit::Metachar(found));
    }
    // **空の行を「撃てる」に化けさせない**（`sh -c ""` は rc 0 で終わる＝何もしていない
    // のに緑を名乗る）。宣言の側は配列の層が空要素を弾くが、契約の verify は弾かない。
    let Some(head) = line.split_whitespace().next() else {
        return Some(Unfit::Empty);
    };
    if !allowed.iter().any(|command| command == head) {
        return Some(Unfit::Command(head.to_owned()));
    }
    // **`..` も外である**。error 文言が「repo の外」を名乗る以上、絶対 path と `~` だけを
    // 見る形は名乗りに届かない（`git rev-parse --git-dir ../../../etc` が通っていた・実測
    // 2026-09-10）。ADR-0010 §2.3 の括弧の列挙より厳しい側なので、条とは衝突しない。
    if let Some(word) = line.split_whitespace().find(|word| {
        word.starts_with('/') || word.contains('~') || word.split('/').any(|part| part == "..")
    }) {
        return Some(Unfit::Outside(word.to_owned()));
    }
    // **置ける穴は閉じた集合**（[`BASE_HOLES`]）であって `{base}` 1 つではない（ADR-0021 §2.1）。
    if let Some(hole) = holes_in(line)
        .into_iter()
        .find(|hole| !(holes == Holes::Base && BASE_HOLES.contains(&hole.as_str())))
    {
        return Some(Unfit::Hole(hole));
    }
    // **禁じる語列**（ADR-0025 §2.3）: hook の command guard と同じ 1 関数。verify 行は argv 1 本（制御文字なし）
    // なので segment は 1 つである。
    denied_in(line, basis.denied).map(|hit| Unfit::Denied(hit.sequence))
}

/// 行の中の穴を全部拾う。**閉じない `{` も穴として拾う**（黙って通さない）。
fn holes_in(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find('{') {
        let tail = rest.get(at..).unwrap_or_default();
        match tail.find('}') {
            Some(end) => {
                found.push(tail.get(..=end).unwrap_or_default().to_owned());
                rest = tail.get(end.saturating_add(1)..).unwrap_or_default();
            }
            None => {
                found.push(tail.to_owned());
                break;
            }
        }
    }
    found
}

impl Declared {
    /// 宣言の本文から読む。**不備は全件集めて返す**。
    fn parse(text: &str) -> Result<Self, Vec<DeclError>> {
        let mut errors = Vec::new();
        let found = fields(text, DECLARED_KEYS, &mut errors);
        let schema = int_of(&found, "schema", &mut errors);
        let (allowed, allowed_line) = list_of(&found, "allowed-commands", &mut errors);
        let (common_verify, common_line) = list_of(&found, "common-verify", &mut errors);
        let (detection_verify, detection_line) = list_of(&found, DETECTION_KEY, &mut errors);
        let requirements = requirements_of(&found, &mut errors);
        let remote = remote_of(&found, &mut errors);
        let ci_cmd = ci_cmd_of(&found, &mut errors);
        let path_kinds = path_kinds::declared_of(&found, &mut errors);
        if schema != Some(SCHEMA_VERSION) {
            errors.push(DeclError::new(
                0,
                format!("schema は {SCHEMA_VERSION} である（実 {schema:?}）"),
            ));
        }
        if errors.is_empty() {
            Ok(Self {
                allowed,
                common_verify,
                detection_verify,
                allowed_line,
                common_line,
                detection_line,
                requirements,
                remote,
                ci_cmd,
                path_kinds,
            })
        } else {
            Err(errors)
        }
    }
}

/// 要件面の path（任意）。書いた周は repo 相対の path の文字列だけを受ける（repo の外を読まない）。
fn requirements_of(found: &[(String, Raw, u64)], errors: &mut Vec<DeclError>) -> Option<String> {
    let (_, value, line) = found.iter().find(|(seen, _, _)| seen == REQUIREMENTS_KEY)?;
    match value {
        Raw::Text(path) if repo_relative(path) => Some(path.clone()),
        _ => {
            errors.push(DeclError::new(
                *line,
                format!("{REQUIREMENTS_KEY} は repo 相対の path の文字列である（空・絶対 path・home の短縮記号・.. は書けない）"),
            ));
            None
        }
    }
}

/// push 先の remote の名（任意）。**1 語だけ**を受ける——空白を含む値は `git push <remote> main:main` の
/// 引数が 2 つに割れ、別の ref を押すことになる。
fn remote_of(found: &[(String, Raw, u64)], errors: &mut Vec<DeclError>) -> Option<String> {
    let (_, value, line) = found.iter().find(|(seen, _, _)| seen == REMOTE_KEY)?;
    match value {
        Raw::Text(name) if !name.trim().is_empty() && !name.split_whitespace().nth(1).is_some() => {
            Some(name.trim().to_owned())
        }
        _ => {
            errors.push(DeclError::new(*line, format!("{REMOTE_KEY} は空白を含まない 1 語の remote の名である")));
            None
        }
    }
}

/// CI の判定を読む 1 行（任意）。**`{sha}` の穴を必ず持つ**——穴の無い行は着地した commit を名指さず、
/// 別の commit の判定を読んで success と言いうる（測っていないものを測ったことにしない・C10）。
fn ci_cmd_of(found: &[(String, Raw, u64)], errors: &mut Vec<DeclError>) -> Option<String> {
    let (_, value, line) = found.iter().find(|(seen, _, _)| seen == CI_CMD_KEY)?;
    match value {
        Raw::Text(cmd) if !cmd.trim().is_empty() && cmd.contains(CI_SHA_HOLE) => Some(cmd.trim().to_owned()),
        _ => {
            errors.push(DeclError::new(
                *line,
                format!("{CI_CMD_KEY} は {CI_SHA_HOLE} の穴を持つ 1 行である（着地した commit を名指さない行は撃てない）"),
            ));
            None
        }
    }
}

/// repo 相対の path か（空でない・絶対 path でない・home の短縮記号も `..` の段も持たない）。
fn repo_relative(path: &str) -> bool {
    !path.trim().is_empty() && !path.starts_with('/') && !path.contains('~') && !path.split('/').any(|part| part == "..")
}

/// 契約表の検査（`contracts check`）が読む宣言の事実（設計 contract-source.md §2「表の検査」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableFacts {
    /// 上限と突き合わせた allowlist（verify 行の先頭語の基準）。
    pub allowed: Vec<String>,
    /// 禁じる語列（rules 行 [`DENIED_ROW`]・verify 行に intake と同じ判定を掛ける基準）。
    pub denied: Vec<String>,
    /// 要件面の repo 相対 path（宣言 `requirements`・無ければ [`DEFAULT_REQUIREMENTS`]）。
    pub requirements: String,
}

/// HEAD の宣言を読み、上限と突き合わせて契約表の検査の事実にする（intake と同じ読み口・作業ツリーは読まない）。
pub fn table_facts(repo: &Path, ceiling: &Ceiling<'_>) -> Result<TableFacts, Vec<DeclError>> {
    let sourced = Sourced::read(repo, ceiling)?;
    let requirements = sourced.declared.requirements.clone().unwrap_or_else(|| DEFAULT_REQUIREMENTS.to_owned());
    let effective = sourced.measure(ceiling, &[])?;
    Ok(TableFacts { allowed: effective.allowed, denied: ceiling.denied.to_vec(), requirements })
}

/// land の終端が読む宣言の事実（設計 contract-source.md §5・push 先と CI の行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalFacts {
    /// push 先の remote の名（宣言 `remote`・**無ければ `None`＝終端を持たない**）。
    pub remote: Option<String>,
    /// CI の判定を読む 1 行（宣言 `ci-cmd`・無ければ [`DEFAULT_CI_CMD`]・`{sha}` の穴を持つ）。
    pub ci_cmd: String,
}

/// HEAD の宣言から終端の事実を解く（上限は読まない＝終端は allowlist と突き合わせない）。
///
/// **宣言が無い周も断る**（`Err`）。終端は「どこへ push し、どの行で CI を読むか」を宣言から受ける口で、
/// 宣言そのものが無い repo に既定で push するのは「測っていない先へ出す」ことになる（A1 の「出す」・C10）。
pub fn terminal_facts(repo: &Path) -> Result<TerminalFacts, Vec<DeclError>> {
    let declared = declared_at_head(repo)?;
    Ok(TerminalFacts {
        remote: declared.remote,
        ci_cmd: declared.ci_cmd.unwrap_or_else(|| DEFAULT_CI_CMD.to_owned()),
    })
}

/// 契約の verify 1 行が argv 1 本として撃てない理由（撃てれば `None`）。判定は [`unfit`] の 1 本で、契約の行は
/// 穴を持てない（intake の契約 verify と同じ字面で断る・禁じる語列も同じ）。
pub fn verify_unfit(line: &str, basis: &Basis<'_>) -> Option<String> {
    unfit(line, basis, Holes::None).map(|found| found.reason(basis.allowed))
}

impl Effective {
    /// 許す command。
    pub fn allowed(&self) -> &[String] {
        &self.allowed
    }

    /// どの便でも撃つ検証行。**gate と land はここからしか読まない**（repo / worktree の
    /// 宣言を読み直さない＝便の中で検証が動かない・ADR-0010 §2.4）。
    pub fn common_verify(&self) -> &[String] {
        &self.common_verify
    }

    /// 検出線の行（無ければ空）。**gate と land はここからしか読まない**（[`Self::common_verify`] と同じ）。
    pub fn detection_verify(&self) -> &[String] {
        &self.detection_verify
    }

    /// 便の写しの本文（**同じ reader で読み戻せる**形）。
    ///
    /// 検出線が空の周は key ごと書かない——空配列は reader が不備として断る形であり、
    /// 任意 key の「無い」は key の不在で表す。
    pub fn render(&self) -> String {
        let detection = if self.detection_verify.is_empty() {
            String::new()
        } else {
            format!("{DETECTION_KEY} = {}\n", array(&self.detection_verify))
        };
        format!(
            "schema = {SCHEMA_VERSION}\nallowed-commands = {}\ncommon-verify = {}\n{detection}commit = \"{}\"\nsource = \"{}\"\nceiling = \"{}\"\n",
            array(&self.allowed),
            array(&self.common_verify),
            self.commit,
            self.source,
            self.ceiling,
        )
    }

    /// 便の写しを読み戻す。
    pub fn load(path: &Path) -> Result<Self, Vec<DeclError>> {
        let text = std::fs::read_to_string(path).map_err(|err| {
            vec![DeclError::new(0, format!("{} を読めない: {err}", path.display()))]
        })?;
        Self::parse(&text)
    }

    /// 写しの本文から読む（[`Self::render`] の逆・**同じ reader** で読み戻せることが要件）。
    fn parse(text: &str) -> Result<Self, Vec<DeclError>> {
        let mut errors = Vec::new();
        let found = fields(text, EFFECTIVE_KEYS, &mut errors);
        let schema = int_of(&found, "schema", &mut errors);
        let (allowed, _) = list_of(&found, "allowed-commands", &mut errors);
        let (common_verify, _) = list_of(&found, "common-verify", &mut errors);
        let (detection_verify, _) = list_of(&found, DETECTION_KEY, &mut errors);
        let commit = text_of(&found, "commit", &mut errors);
        let source = text_of(&found, "source", &mut errors);
        let ceiling = text_of(&found, "ceiling", &mut errors);
        if schema != Some(SCHEMA_VERSION) {
            errors.push(DeclError::new(0, format!("schema は {SCHEMA_VERSION} である")));
        }
        if errors.is_empty() {
            Ok(Self { allowed, common_verify, detection_verify, commit, source, ceiling })
        } else {
            Err(errors)
        }
    }
}

/// 配列 1 つを写す（要素は制御文字も引用符も持てないので、そのまま囲める）。
fn array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| format!("\"{item}\"")).collect();
    format!("[{}]", quoted.join(", "))
}

/// 1 行ずつ読み、key → 値 を集める。**未知 key・重複・必須の欠落は全件積む**。
fn fields(text: &str, known: &[&str], errors: &mut Vec<DeclError>) -> Vec<(String, Raw, u64)> {
    let mut found: Vec<(String, Raw, u64)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (index, raw_line) in text.lines().enumerate() {
        let line = (index as u64).saturating_add(1);
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = trimmed.split_once('=') else {
            errors.push(DeclError::new(line, format!("key = value の形でない: {trimmed}")));
            continue;
        };
        let key = key.trim().to_owned();
        if !known.contains(&key.as_str()) {
            errors.push(DeclError::new(line, format!("未知の key {key}")));
            continue;
        }
        if seen.contains(&key) {
            errors.push(DeclError::new(line, format!("key {key} が重複する")));
            continue;
        }
        seen.push(key.clone());
        if let Some(value) = value_of(&key, raw_value.trim(), line, errors) {
            found.push((key, value, line));
        }
    }
    for key in known {
        if !OPTIONAL_KEYS.contains(key) && !seen.iter().any(|name| name == key) {
            errors.push(DeclError::new(0, format!("必須の key {key} が無い")));
        }
    }
    found
}

/// 1 つの値を読む。配列の層（空・空要素・引用符 1 組）は manifest と共有する。
fn value_of(key: &str, raw: &str, line: u64, errors: &mut Vec<DeclError>) -> Option<Raw> {
    if raw.starts_with('[') {
        return match list(raw) {
            Ok(items) => Some(Raw::List(items)),
            Err(reason) => {
                errors.push(DeclError::new(line, format!("{key} の {reason}")));
                None
            }
        };
    }
    match scalar(raw) {
        Some(Scalar::Int(found)) => Some(Raw::Int(found)),
        Some(Scalar::Str(found)) => Some(Raw::Text(found)),
        _ => {
            errors.push(DeclError::new(line, format!("{key} の value を読めない: {raw}")));
            None
        }
    }
}

/// 整数 key を取り出す。型違いはここで積む。
fn int_of(found: &[(String, Raw, u64)], key: &str, errors: &mut Vec<DeclError>) -> Option<u64> {
    match found.iter().find(|(seen, _, _)| seen == key) {
        Some((_, Raw::Int(value), _)) => Some(*value),
        Some((_, _, line)) => {
            errors.push(DeclError::new(*line, format!("{key} は整数である")));
            None
        }
        None => None,
    }
}

/// 文字列 key を取り出す。型違いはここで積む。
fn text_of(found: &[(String, Raw, u64)], key: &str, errors: &mut Vec<DeclError>) -> String {
    match found.iter().find(|(seen, _, _)| seen == key) {
        Some((_, Raw::Text(value), _)) => value.clone(),
        Some((_, _, line)) => {
            errors.push(DeclError::new(*line, format!("{key} は文字列である")));
            String::new()
        }
        None => String::new(),
    }
}

/// 配列 key を取り出す。**書かれていた行番号も返す**（違反をその行で名指すため）。
fn list_of(
    found: &[(String, Raw, u64)],
    key: &str,
    errors: &mut Vec<DeclError>,
) -> (Vec<String>, u64) {
    match found.iter().find(|(seen, _, _)| seen == key) {
        Some((_, Raw::List(items), line)) => (items.clone(), *line),
        Some((_, _, line)) => {
            errors.push(DeclError::new(*line, format!("{key} は配列である")));
            (Vec::new(), *line)
        }
        None => (Vec::new(), 0),
    }
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.373

    use super::{
        kind_gap, unfit, Basis, Ceiling, Declared, Effective, Holes, KindGap, Sourced, Unfit, VerifyKind, BASE_HOLES,
        BASE_HOLE, CEILING_ROW, DECLARED_KEYS, DECL_FILE, DEFAULT_CI_CMD, DENIED_ROW, CI_SHA_HOLE, JOBS_HOLE,
        SCHEMA_VERSION, TEETH_HOLE, THREADS_HOLE,
    };
    use crate::order::is_declaration_order;

    /// 文字列の列。
    pub(super) fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    /// 禁じる語列の fixture（ADR-0025 §2.1 の初期値の一部）。
    fn denied() -> Vec<String> {
        strings(&["cargo mutants", "git push --force", "git branch -D"])
    }

    /// 上限の fixture（`cargo` / `git` を許し [`denied`] を禁じる）。
    fn ceiling<'a>(commands: &'a [String], denied: &'a [String]) -> Ceiling<'a> {
        Ceiling { row: CEILING_ROW, commands, denied }
    }

    /// 宣言の共通 verify に置ける穴は**閉じた集合**であり、その外は `Hole` で断る
    /// （ADR-0021 §2.1 が ADR-0010 §2.1 を部分 supersede）。
    ///
    /// 両向きを 1 本で撃つ: 集合を空にする変異も、逆に全部の穴を通す変異も、ここで落ちる。
    #[test]
    fn declaration_accepts_only_the_closed_set_of_holes_in_common_verify() {
        let (allowed, denied) = (strings(&["cargo"]), denied());
        let basis = Basis { allowed: &allowed, denied: &denied };
        assert_eq!(BASE_HOLES, [BASE_HOLE, JOBS_HOLE, THREADS_HOLE, TEETH_HOLE], "集合は 4 つちょうど");
        for hole in BASE_HOLES {
            let line = format!("cargo xtask mutants-diff --base {hole}");
            assert_eq!(unfit(&line, &basis, Holes::Base), None, "{hole} は共通 verify に置ける");
            assert_eq!(
                unfit(&line, &basis, Holes::None),
                Some(Unfit::Hole((*hole).to_owned())),
                "{hole} も契約の verify には置けない"
            );
        }
        // 集合の外は**綴り違いでも**断る（`{jobz}` を黙って撃たない）。
        for outside in ["{jobz}", "{job}", "{jobs", "{JOBS}"] {
            let line = format!("cargo xtask mutants-diff --jobs {outside}");
            assert_eq!(
                unfit(&line, &basis, Holes::Base),
                Some(Unfit::Hole(outside.to_owned())),
                "{outside} は置けない穴である"
            );
        }
        // 2 つを同じ行に置ける（`{threads}` を持たない consumer の宣言の形）。
        assert_eq!(
            unfit("cargo xtask mutants-diff --base {base} --jobs {jobs}", &basis, Holes::Base),
            None,
            "2 つの穴を同じ行に置ける"
        );
    }

    /// 宣言の置ける穴の 3 つ目は `{threads}` で、それを持つ検出線の行は intake を通り、
    /// 集合の外の穴は従来どおり不適合に落ちる（設計 gate-cost.md §31 約束 6・4 つ目の `{teeth}` は §34）。
    ///
    /// 3 つ目を足したことと集合を広げすぎていないことを同じ歯で pin する（集合を 2 つに戻す変異は 1 本目で、
    /// 穴を全部通す変異は綴り違いの列で落ちる）。
    #[test]
    fn declaration_threads_hole_is_the_third_and_last_hole() {
        let (allowed, denied) = (strings(&["cargo"]), denied());
        let basis = Basis { allowed: &allowed, denied: &denied };
        assert_eq!(BASE_HOLES.len(), 4, "穴は 4 つちょうど: {BASE_HOLES:?}");
        assert_eq!(BASE_HOLES.get(2).copied(), Some(THREADS_HOLE), "3 つ目が {THREADS_HOLE}");
        assert_eq!(THREADS_HOLE, "{threads}", "穴の字面は固定（宣言 file に書く語）");
        // scribe2 自身の検出線の形（3 つの穴を同じ行に置ける）。
        let line = "cargo xtask mutants-diff --base {base} --jobs {jobs} --threads {threads}";
        assert_eq!(unfit(line, &basis, Holes::Base), None, "3 つの穴の行は intake を通る");
        assert_eq!(
            unfit(line, &basis, Holes::None),
            Some(Unfit::Hole(BASE_HOLE.to_owned())),
            "契約の verify には 1 つも置けない（最初の穴で断る）"
        );
        assert_eq!(
            unfit("cargo xtask mutants-diff --threads {threads}", &basis, Holes::None),
            Some(Unfit::Hole(THREADS_HOLE.to_owned())),
            "{THREADS_HOLE} も契約の verify には置けない"
        );
        // 集合の外は綴り違いでも断る。
        for outside in ["{thread}", "{threadz}", "{test-threads}", "{THREADS}", "{cores}", "{price}"] {
            let line = format!("cargo xtask mutants-diff --threads {outside}");
            assert_eq!(
                unfit(&line, &basis, Holes::Base),
                Some(Unfit::Hole(outside.to_owned())),
                "{outside} は置けない穴である"
            );
        }
    }

    /// (a) 穴の閉じた集合は **4 つちょうど**で末尾が `{teeth}`（設計 gate-cost.md §34 約束 4・ADR-0052）。
    #[test]
    fn declaration_teeth_hole_is_the_fourth_and_last_hole() {
        assert_eq!(BASE_HOLES.len(), 4, "穴は 4 つちょうど: {BASE_HOLES:?}");
        assert_eq!(BASE_HOLES.last().copied(), Some(TEETH_HOLE), "末尾が {TEETH_HOLE}");
        assert_eq!(TEETH_HOLE, "{teeth}", "穴の字面は固定（宣言 file に書く語）");
        assert_eq!(BASE_HOLES, ["{base}", "{jobs}", "{threads}", "{teeth}"], "4 つの字面と順");
    }

    /// (b) 検出線の行の `{teeth}` は `unfit` を通り、契約の verify 行の `{teeth}` は断られる（既存の `{jobs}` と同じ対）。
    /// 集合の外の綴り違いは宣言の側でも断る（4 つ目を足しても 5 つ目は無い）。
    #[test]
    fn declaration_teeth_hole_passes_detection_line_and_is_refused_in_contract_verify() {
        let (allowed, denied) = (strings(&["cargo"]), denied());
        let basis = Basis { allowed: &allowed, denied: &denied };
        let line = "cargo xtask mutants-diff --base {base} --jobs {jobs} --threads {threads} --teeth {teeth}";
        assert_eq!(unfit(line, &basis, Holes::Base), None, "4 つの穴の検出線は intake を通る");
        assert_eq!(
            unfit("cargo xtask mutants-diff --teeth {teeth}", &basis, Holes::Base),
            None,
            "{TEETH_HOLE} だけの行も宣言には置ける"
        );
        assert_eq!(
            unfit("cargo xtask mutants-diff --teeth {teeth}", &basis, Holes::None),
            Some(Unfit::Hole(TEETH_HOLE.to_owned())),
            "{TEETH_HOLE} は契約の verify には置けない"
        );
        assert_eq!(
            unfit("cargo xtask mutants-diff --jobs {jobs}", &basis, Holes::None),
            Some(Unfit::Hole(JOBS_HOLE.to_owned())),
            "{JOBS_HOLE} と同じ対"
        );
        for outside in ["{tooth}", "{teethz}", "{TEETH}", "{filter}", "{words}"] {
            let line = format!("cargo xtask mutants-diff --teeth {outside}");
            assert_eq!(unfit(&line, &basis, Holes::Base), Some(Unfit::Hole(outside.to_owned())), "{outside} は置けない穴である");
        }
    }

    /// 禁じる語列（ADR-0025 §2.3）は unfit の 6 つ目の理由: 先頭語が allowlist に在っても、rules 行 `runner.denied_commands`
    /// の語列（先頭語一致 + 残りの語の包含・順序不問）に当たる行は `Denied` で断り、理由は行 id と語列を名指す。
    /// 語列を 1 つも持たない基準（`denied = []`）では同じ行が通る（判定の出所は行の値である）。
    #[test]
    fn declaration_unfit_names_the_denied_sequence_from_the_rules_row() {
        let (allowed, denied) = (strings(&["cargo", "git"]), denied());
        let basis = Basis { allowed: &allowed, denied: &denied };
        for (line, sequence) in [
            ("cargo mutants --in-diff x", "cargo mutants"),
            ("git push origin main --force", "git push --force"),
            ("git branch -D feat", "git branch -D"),
        ] {
            let found = unfit(line, &basis, Holes::None);
            assert_eq!(found, Some(Unfit::Denied(sequence.to_owned())), "{line}");
            let reason = found.map(|found| found.reason(&allowed)).unwrap_or_default();
            assert!(reason.contains(DENIED_ROW) && reason.contains(sequence), "行 id と語列を名指す: {reason}");
        }
        assert_eq!(unfit("cargo nextest run -p x", &basis, Holes::None), None, "当たらない行は通る");
        assert_eq!(unfit("git push origin feat/x", &basis, Holes::Base), None, "共通 verify も同じ判定");
        let none: Vec<String> = Vec::new();
        let open = Basis { allowed: &allowed, denied: &none };
        assert_eq!(unfit("cargo mutants --in-diff x", &open, Holes::None), None, "語列の無い基準では通る");
    }

    /// 入口の flip と Rust の検証の 2 行（`cargo` の行を持つ宣言は入口の flip を要る・`s2-07l.170`）。
    const FLIP_AND_CHECK: &str = r#"["cargo xtask flip-check --base {base}", "cargo xtask check"]"#;

    /// 宣言の本文。
    fn body(allowed: &str, common: &str) -> String {
        format!("schema = 1\nallowed-commands = {allowed}\ncommon-verify = {common}\n")
    }

    /// 上限を通った有効値を **実経路と同じ 3 段**で組む。
    fn effective(allowed: &str, common: &str) -> Effective {
        let declared = Declared::parse(&body(allowed, common)).expect("宣言を読める");
        let (commands, denied) = (strings(&["cargo", "git"]), denied());
        Sourced {
            declared,
            commit: "c0ffee".to_owned(),
            source: DECL_FILE.to_owned(),
            ceiling: CEILING_ROW.to_owned(),
        }
        .measure(&ceiling(&commands, &denied), &[])
        .expect("上限の内側の宣言は通る")
    }

    /// 便の写しは **同じ reader で読み戻せる**（出所ごと round trip する）。
    ///
    /// 出所を落とした写しは「どの commit の宣言で走ったか」を後から言えない＝C10 の
    /// 「実測を経た値」が名ばかりになる。
    #[test]
    fn declaration_copy_round_trips_through_the_same_reader() {
        let made = effective(r#"["cargo", "git"]"#, FLIP_AND_CHECK);
        let text = made.render();
        let read = Effective::parse(&text).expect("写しを読み戻せる");
        assert_eq!(read, made, "写しは出所ごと round trip する: {text}");
        assert!(text.contains("commit = \"c0ffee\""), "読んだ commit が写しに載る: {text}");
        assert!(text.contains("ceiling = \"runner.allowed_commands\""), "上限行の id が載る: {text}");
    }

    /// `detection-verify` は**任意 key**: 無い宣言は通り（写しにも key を書かない）、在る宣言は
    /// 写しを round trip し、書いた空配列は従来どおり不備である（ADR-0010 §2.1・ADR-0021 §2.4）。
    #[test]
    fn declaration_detection_verify_is_optional_and_round_trips() {
        let absent = effective(r#"["cargo"]"#, FLIP_AND_CHECK);
        assert!(absent.detection_verify().is_empty(), "無い key は空");
        assert!(!absent.render().contains("detection-verify"), "空の周は key ごと書かない: {}", absent.render());

        let text = format!("{}detection-verify = [\"cargo xtask mutants-diff --base {{base}} --jobs {{jobs}}\"]\n", body(r#"["cargo"]"#, FLIP_AND_CHECK));
        let declared = Declared::parse(&text).expect("検出線の在る宣言を読める");
        let (commands, denied) = (strings(&["cargo"]), denied());
        let made = Sourced { declared, commit: "c0ffee".to_owned(), source: DECL_FILE.to_owned(), ceiling: CEILING_ROW.to_owned() }
            .measure(&ceiling(&commands, &denied), &[])
            .expect("穴 2 つの検出線は通る");
        assert_eq!(made.detection_verify().len(), 1, "検出線を 1 行持つ");
        let read = Effective::parse(&made.render()).expect("写しを読み戻せる");
        assert_eq!(read, made, "検出線ごと round trip する: {}", made.render());

        let empty = format!("{}detection-verify = []\n", body(r#"["cargo"]"#, r#"["cargo xtask check"]"#));
        let errors = Declared::parse(&empty).expect_err("書いた空配列は不備");
        assert!(errors.iter().any(|error| error.reason.contains("配列が空である")), "{errors:?}");
    }

    /// `requirements` は**任意 key**（無い宣言は通り既定の要件面を読む）で、書いた周は repo 相対の path だけを
    /// 受ける（設計 contract-source.md §2）。綴り違いの key は従来どおり未知 key として断る。
    #[test]
    fn declaration_requirements_is_an_optional_repo_relative_path() {
        let base = body(r#"["cargo"]"#, r#"["cargo xtask check"]"#);
        assert_eq!(Declared::parse(&base).expect("key 無しは通る").requirements, None, "無い key は None");
        let set = Declared::parse(&format!("{base}requirements = \"spec/reqs.yaml\"\n")).expect("path は通る");
        assert_eq!(set.requirements.as_deref(), Some("spec/reqs.yaml"), "書いた path");
        for bad in ["\"\"", "\"/etc/reqs.yaml\"", "\"../up/srs.html\"", "\"a/~x.html\"", "[\"a\"]", "1"] {
            let errors = Declared::parse(&format!("{base}requirements = {bad}\n")).expect_err("repo 相対の path でない");
            assert!(errors.iter().any(|error| error.reason.contains("requirements") && error.line == 4), "{bad}: {errors:?}");
        }
        let typo = Declared::parse(&format!("{base}requirement = \"x.html\"\n")).expect_err("綴り違いは未知 key");
        assert!(typo.iter().any(|error| error.reason.contains("未知の key requirement")), "{typo:?}");
    }

    /// `remote` は**任意 key**で、書いた周は**空白を含まない 1 語**だけを受ける（設計 contract-source.md §5）。
    ///
    /// 空白を含む値は `git push <remote> main:main` の引数が 2 つに割れ、**別の ref を押す**。
    #[test]
    fn pipe_terminal_land_remote_is_an_optional_single_word() {
        let base = body(r#"["cargo"]"#, r#"["cargo xtask check"]"#);
        assert_eq!(Declared::parse(&base).expect("key 無しは通る").remote, None, "無い key は None（既定を持たない）");
        let set = Declared::parse(&format!("{base}remote = \"upstream\"\n")).expect("1 語は通る");
        assert_eq!(set.remote.as_deref(), Some("upstream"), "書いた remote の名");
        for bad in ["\"\"", "\"   \"", "\"origin main\"", "\"origin +refs/heads/x\"", "[\"origin\"]", "1"] {
            let errors = Declared::parse(&format!("{base}remote = {bad}\n")).expect_err("1 語でない");
            assert!(errors.iter().any(|error| error.reason.contains("remote") && error.line == 4), "{bad}: {errors:?}");
        }
    }

    /// `ci-cmd` は**任意 key**で、書いた周は **`{sha}` の穴を持つ 1 行**だけを受ける（同 §5）。
    ///
    /// 穴の無い行は着地した commit を名指さず、**別の commit の判定を読んで success と言いうる**（C10）。
    #[test]
    fn pipe_terminal_land_ci_cmd_must_carry_the_sha_hole() {
        let base = body(r#"["cargo"]"#, r#"["cargo xtask check"]"#);
        assert_eq!(Declared::parse(&base).expect("key 無しは通る").ci_cmd, None, "無い key は None（既定は呼び手が埋める）");
        let line = format!("forge runs --commit {CI_SHA_HOLE} --json status");
        let set = Declared::parse(&format!("{base}ci-cmd = \"{line}\"\n")).expect("穴を持つ行は通る");
        assert_eq!(set.ci_cmd.as_deref(), Some(line.as_str()), "書いた行");
        for bad in ["\"\"", "\"forge runs --commit HEAD\"", "\"forge runs --commit {run}\"", "[\"a\"]", "1"] {
            let errors = Declared::parse(&format!("{base}ci-cmd = {bad}\n")).expect_err("穴が無い");
            assert!(errors.iter().any(|error| error.reason.contains("ci-cmd") && error.line == 4), "{bad}: {errors:?}");
        }
        // 既定の 1 行も同じ条件を満たす（器が埋める既定が自分の規則を破らない）。
        assert!(DEFAULT_CI_CMD.contains(CI_SHA_HOLE), "既定の行も穴を持つ: {DEFAULT_CI_CMD}");
    }

    /// (§46) 既定の 1 行は `--json` の欄の列に `event` を持ち、`{sha}` の穴は 1 つのまま。
    #[test]
    fn pipe_declaration_default_ci_cmd_carries_the_event_field() {
        let words: Vec<&str> = DEFAULT_CI_CMD.split_whitespace().collect();
        let fields = words
            .iter()
            .position(|word| *word == "--json")
            .and_then(|at| words.get(at + 1))
            .expect("既定の行は --json の欄の列を持つ");
        let fields: Vec<&str> = fields.split(',').collect();
        assert!(fields.contains(&"event"), "欄の列に event が在る: {fields:?}");
        assert!(fields.contains(&"status") && fields.contains(&"conclusion"), "従来の欄は残る: {fields:?}");
        assert_eq!(DEFAULT_CI_CMD.matches(CI_SHA_HOLE).count(), 1, "穴は 1 つのまま: {DEFAULT_CI_CMD}");
    }

    /// schema は 1 だけ。**整数でない schema も断る**（型の取り違えを黙って通さない）。
    #[test]
    fn declaration_refuses_other_schema_versions() {
        for text in [
            "schema = 2\nallowed-commands = [\"git\"]\ncommon-verify = [\"git status\"]\n",
            "schema = \"1\"\nallowed-commands = [\"git\"]\ncommon-verify = [\"git status\"]\n",
        ] {
            let errors = Declared::parse(text).expect_err("schema が違えば断る");
            assert!(
                errors.iter().any(|error| error.reason.contains("schema")),
                "理由に schema が出る: {errors:?}"
            );
        }
    }

    /// 欠けた必須 key は **全件**名指す（1 件目で止めると直すたびに次の 1 件が出る）。
    #[test]
    fn declaration_names_every_missing_key() {
        let errors = Declared::parse("schema = 1\n").expect_err("2 key が欠けている");
        for key in ["allowed-commands", "common-verify"] {
            assert!(
                errors.iter().any(|error| error.reason.contains(&format!("必須の key {key}"))),
                "{key} の欠落を名指す: {errors:?}"
            );
        }
    }

    /// 不備は **全件・行番号つき**で名指す（1 件目で止めると直すたびに次の 1 件が出る）。
    ///
    /// 母集団 = `fields` / `value_of` / `list_of` が押す 6 分岐（schema 違いと必須欠落は
    /// 別の歯が持つ）。
    #[test]
    fn declaration_names_every_malformed_line() {
        let text = "schema = 1\nallowed-commands = [\"git\"]\nallowed-commands = [\"sh\"]\n\
                    common-verify = [\"git status\"]\nnonsense = \"x\"\nbare\n";
        let errors = Declared::parse(text).expect_err("3 つの不備が在る");
        let shown = format!("{errors:?}");
        for (want, line) in [("重複", 3_u64), ("未知の key nonsense", 5), ("key = value の形でない", 6)] {
            let found = errors
                .iter()
                .find(|error| error.reason.contains(want))
                .unwrap_or_else(|| panic!("{want} を名指す: {shown}"));
            assert_eq!(found.line, line, "{want} の行番号: {shown}");
        }

        // 値の形の不備も同じ面で断る（型違い・配列の壊れ・読めない scalar）。
        for (schema, allowed, want) in [
            ("1", "\"git\"", "allowed-commands は配列である"),
            ("1", "[\"a\" \"b\"]", "引用符 1 組の文字列でない"),
            ("1", "[]", "配列が空である"),
            ("xyz", "[\"git\"]", "value を読めない"),
        ] {
            let text = format!(
                "schema = {schema}\nallowed-commands = {allowed}\ncommon-verify = [\"git status\"]\n"
            );
            let errors = Declared::parse(&text).expect_err("値の形の不備は断る");
            assert!(
                errors.iter().any(|error| error.reason.contains(want)),
                "{allowed} の理由に {want} が出る: {errors:?}"
            );
        }
    }

    /// 空行と `#` のコメント行は読み飛ばす（宣言 file は人が読む面でもある）。
    #[test]
    fn declaration_skips_blank_lines_and_comments() {
        let text = "# 何のための file か\n\nschema = 1\n\n\
                    # 許してよい command\nallowed-commands = [\"git\"]\n\
                    common-verify = [\"git status\"]\n\n";
        let parsed = Declared::parse(text).expect("空行とコメントは飛ばす");
        assert_eq!(parsed.allowed, vec!["git".to_owned()], "値は読めている");
        assert_eq!(parsed.allowed_line, 6, "行番号は物理行のまま（飛ばした行も数える）");
    }

    /// 複数の理由に同時に当たる行は、**宣言順で最初の理由**で断る（憲法 C2: 適用順は
    /// 宣言順だけから取る）。
    ///
    /// `unfit` の検査順は手で書くので、宣言順から静かにずれうる——ずれても compile は
    /// 通り、順序の注記を散文で持たない以上、機械が測らなければ誰も気づかない。
    #[test]
    fn unfit_order_takes_the_first_reason_in_declaration_order() {
        let declared = [
            Unfit::Metachar(';'),
            Unfit::Empty,
            Unfit::Command(String::new()),
            Unfit::Outside(String::new()),
            Unfit::Hole(String::new()),
            Unfit::Denied(String::new()),
        ];
        let order: Vec<&Unfit> = declared.iter().collect();
        assert!(
            is_declaration_order(&order, Unfit::rank),
            "rank は宣言順に 0.. である（並べ替え・重複・中間の欠番を落とす）: {declared:?}"
        );

        // 母集団 = 2 つ以上の理由に同時に当たる行 4 本（当たる理由は fixture が持つ）。
        let (allowed, denied) = (strings(&["cargo"]), denied());
        let basis = Basis { allowed: &allowed, denied: &denied };
        for (line, holes, hit) in [
            (
                "rm -rf; echo",
                Holes::Base,
                vec![Unfit::Metachar(';'), Unfit::Command("rm".to_owned())],
            ),
            (
                "rm /etc/passwd",
                Holes::Base,
                vec![Unfit::Command("rm".to_owned()), Unfit::Outside("/etc/passwd".to_owned())],
            ),
            (
                "cargo test ../up {base}",
                Holes::None,
                vec![Unfit::Outside("../up".to_owned()), Unfit::Hole("{base}".to_owned())],
            ),
            (
                "cargo mutants --baseline {base}",
                Holes::None,
                vec![Unfit::Hole("{base}".to_owned()), Unfit::Denied("cargo mutants".to_owned())],
            ),
        ] {
            assert!(hit.len() >= 2, "{line:?} は複数の理由に当たる形である: {hit:?}");
            let first = hit.iter().min_by_key(|found| found.rank()).cloned();
            assert_eq!(
                unfit(line, &basis, holes),
                first,
                "{line:?} が当たる理由 {hit:?} のうち宣言順で最初のものを返す"
            );
        }
    }

    // ---- 行の分類と入口の flip の不在（設計 pipeline.md §7 約束 4・`s2-07l.170`・接頭辞 `declaration_kind_`）----

    /// 宣言を上限（`cargo` / `git` / `sh`）と突き合わせる（intake と同じ `Sourced::measure` の経路）。
    fn measured(allowed: &str, common: &str) -> Result<Effective, Vec<super::DeclError>> {
        let declared = Declared::parse(&body(allowed, common)).expect("宣言を読める");
        let (commands, denied) = (strings(&["cargo", "git", "sh"]), denied());
        Sourced { declared, commit: "c0ffee".to_owned(), source: DECL_FILE.to_owned(), ceiling: CEILING_ROW.to_owned() }
            .measure(&ceiling(&commands, &denied), &[])
    }

    /// 各行は先頭語列で閉じた 3 種に分かれる: 入口の flip（`cargo xtask flip-check`）・他の `cargo` の行・それ以外。
    /// 語の途中や 2 語目以降の `cargo` / `flip-check` は分類に効かない（先頭語列だけで見る）。
    #[test]
    fn declaration_kind_classifies_each_line_by_its_head_words() {
        for (line, want) in [
            ("cargo xtask flip-check --base {base}", VerifyKind::EntranceFlip),
            ("cargo  xtask   flip-check --base {base}", VerifyKind::EntranceFlip),
            ("cargo xtask check", VerifyKind::Cargo),
            ("cargo xtask flip-checks --base {base}", VerifyKind::Cargo),
            ("cargo nextest run --workspace", VerifyKind::Cargo),
            ("sh verify.sh cargo xtask flip-check", VerifyKind::Other),
            ("git rev-parse --verify {base}", VerifyKind::Other),
            ("cargox xtask flip-check", VerifyKind::Other),
        ] {
            assert_eq!(VerifyKind::of(line), want, "{line:?}");
        }
    }

    /// 先頭語 `cargo` の行を持ちながら入口の flip の行を持たない宣言は intake の経路（`Sourced::measure`）が
    /// `NoEntranceRed` で断り、理由は `common-verify` の行番号と variant の名を持つ。flip の行を足せば同じ宣言が通る。
    #[test]
    fn declaration_kind_refuses_cargo_lines_without_the_entrance_flip() {
        let errors = measured(r#"["cargo", "git"]"#, r#"["cargo xtask check", "git status"]"#)
            .expect_err("入口の flip の無い Rust の宣言は断る");
        let found: Vec<&super::DeclError> =
            errors.iter().filter(|error| error.reason.starts_with("NoEntranceRed")).collect();
        assert_eq!(found.len(), 1, "NoEntranceRed を 1 件: {errors:?}");
        assert_eq!(found.first().map(|error| error.line), Some(3), "common-verify の行を名指す: {errors:?}");
        assert!(found.iter().any(|error| error.reason.contains("cargo xtask flip-check --base {base}")), "{errors:?}");
        assert_eq!(kind_gap(&strings(&["cargo nextest run"])), Some(KindGap::NoEntranceRed), "cargo の行 1 本でも断る");
        assert!(measured(r#"["cargo", "git"]"#, FLIP_AND_CHECK).is_ok(), "入口の flip を足せば通る");
        assert_eq!(kind_gap(&strings(&["cargo xtask flip-check --base {base}"])), None, "flip だけの宣言も通る");
    }

    /// 先頭語 `cargo` の行を持たない宣言（`sh` / `git` だけの toy repo）は分類だけで断らない（§7「Rust 固有の検査を
    /// 内蔵しない」のまま）。宣言 file の schema は不変（版 1・key の列も同じ 10 本）。
    #[test]
    fn declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema() {
        assert!(measured(r#"["git", "sh"]"#, r#"["git rev-parse --verify {base}", "sh verify.sh"]"#).is_ok(), "sh / git だけは通る");
        assert_eq!(kind_gap(&strings(&["git status", "sh verify.sh"])), None);
        assert_eq!(SCHEMA_VERSION, 1, "宣言 file の版は動かない");
        assert_eq!(
            DECLARED_KEYS,
            [
                "schema",
                "allowed-commands",
                "common-verify",
                "detection-verify",
                "requirements",
                "remote",
                "ci-cmd",
                "design-intent-paths",
                "design-doc-paths",
                "tests-paths",
            ],
            "宣言 file の key の列は動かない"
        );
    }
}
