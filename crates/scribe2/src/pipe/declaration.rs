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

/// 対象 repo の root に置く宣言 file の名。
pub const DECL_FILE: &str = ".vessel.toml";

/// 上限を持つ rules 行の id。
pub const CEILING_ROW: &str = "runner.allowed_commands";

/// 禁じる語列を持つ rules 行の id（上限と**対**で読む・ADR-0025 §2.1・判定は hook の command guard と同じ 1 関数）。
pub const DENIED_ROW: &str = crate::hook::command::ROW;

/// 宣言 file の schema。
const SCHEMA_VERSION: u64 = 1;

/// 宣言が持つ key（この順で報告する）。
const DECLARED_KEYS: &[&str] = &["schema", "allowed-commands", "common-verify", DETECTION_KEY, REQUIREMENTS_KEY];

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
const OPTIONAL_KEYS: &[&str] = &[DETECTION_KEY, REQUIREMENTS_KEY];

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

/// **宣言の共通 verify に置ける穴の閉じた集合**。
///
/// ADR-0010 §2.1 は穴を `{base}` 1 つと定めたが、ADR-0021 §2.1 がそれを部分 supersede して
/// 集合にした。集合をここ 1 本に閉じるのは、[`unfit`] の判定と gate の置換が**同じ列**を
/// 見るためである——片方だけに穴を足すと、intake を通った行が gate で置換されないまま
/// 撃たれる（`{jobs}` という語をそのまま `--jobs` へ渡す）。
pub const BASE_HOLES: &[&str] = &[BASE_HOLE, JOBS_HOLE];

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

impl Sourced {
    /// HEAD commit の tree から宣言を読む。**作業ツリーは読まない**。
    fn read(repo: &Path, ceiling: &Ceiling<'_>) -> Result<Self, Vec<DeclError>> {
        let commit = super::head_of(repo).ok_or_else(|| {
            vec![DeclError::new(0, format!("{} の HEAD を読めない", repo.display()))]
        })?;
        let spec = format!("HEAD:{DECL_FILE}");
        let bytes = super::git_bytes(repo, &["show", &spec]).ok_or_else(|| {
            vec![DeclError::new(
                0,
                format!("HEAD の tree に {DECL_FILE} が無い（作業ツリーの宣言は読まない＝commit されていない宣言は無いのと同じ）"),
            )]
        })?;
        let declared = Declared::parse(&String::from_utf8_lossy(&bytes))?;
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

/// 契約の verify 1 行が argv 1 本として撃てない理由（撃てれば `None`）。判定は [`unfit`] の 1 本で、契約の行は
/// 穴を持てない（intake の契約 verify と同じ字面で断る・禁じる語列も同じ）。
pub fn verify_unfit(line: &str, basis: &Basis<'_>) -> Option<String> {
    unfit(line, basis, Holes::None).map(|found| found.reason(basis.allowed))
}

// ───────── write-set の項目の読み（設計 contract-source.md §3「項目の実在と展開」「上限の余地」・pure） ─────────

/// write-set の 1 項目を base（tracked file の一覧）に対して読んだもの。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteSetItem {
    /// base に実在する file（repo 相対 path）。
    File(String),
    /// 末尾 `/` の dir。base の配下の file を**展開した**列で持つ（辞書順）。
    Dir(Vec<String>),
    /// `+` 接頭辞で宣言した新規 file（base に無い・接頭辞を剥がした path）。
    New(String),
    /// `-` 接頭辞で宣言した**縮む面**（base に実在する file・接頭辞を剥がした path・設計 contract-source.md §3）。
    /// 増分は負なので上限の余地を求めず、core の見積の本数にも数えない。閉包・交差・guard は [`Self::File`] と同じ
    /// 素の path として読む。
    Shrink(String),
}

/// write-set の各項目を base に対して読む。**解けない項目は全件**（1 件目で止めない）。
///
/// 解ける形は 4 つだけ: base に実在する file / 末尾 `/` で base に配下の file を持つ dir / `+` 接頭辞で base に**無い**
/// 新規 file / `-` 接頭辞で base に**在る**縮む file。それ以外（無い file・空の dir・base に在る file への `+`・base に
/// 無い file への `-`）は `Err` に項目の字面で積む。
pub fn read_write_set(write_set: &[String], tracked: &[String]) -> Result<Vec<WriteSetItem>, Vec<String>> {
    let (mut items, mut unresolved) = (Vec::new(), Vec::new());
    for item in write_set {
        match read_item(item, tracked) {
            Some(found) => items.push(found),
            None => unresolved.push(item.clone()),
        }
    }
    if unresolved.is_empty() {
        Ok(items)
    } else {
        Err(unresolved)
    }
}

/// 1 項目を読む（解けなければ `None`）。
fn read_item(item: &str, tracked: &[String]) -> Option<WriteSetItem> {
    if let Some(dir) = item.strip_suffix('/') {
        let under: Vec<String> = tracked.iter().filter(|path| is_under(path, dir)).cloned().collect();
        return (!under.is_empty()).then_some(WriteSetItem::Dir(under));
    }
    if let Some(new) = item.strip_prefix(super::refuse::NEW_FILE) {
        let absent = !new.is_empty() && !tracked.iter().any(|path| path == new);
        return absent.then(|| WriteSetItem::New(new.to_owned()));
    }
    if let Some(old) = item.strip_prefix(super::refuse::SHRINK_FILE) {
        let present = !old.is_empty() && tracked.iter().any(|path| path == old);
        return present.then(|| WriteSetItem::Shrink(old.to_owned()));
    }
    tracked.iter().any(|path| path == item).then(|| WriteSetItem::File(item.to_owned()))
}

/// `path` が dir `dir`（末尾 `/` 無しの字面）の配下か。
pub(crate) fn is_under(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

/// 上限の余地の入力（rules 行の値・設計 contract-source.md §3「上限の余地」）。数は manifest から読む（C1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// 1 file の行数の上限（R-C4-2）。
    pub file_lines: u64,
    /// core の総行数の上限（R-C4-1）。
    pub core_lines: u64,
    /// `size` 1 段の 1 file あたりの見積（行・`pipe.size_<s|m|l>_lines` のうち契約の size の行）。
    pub size_lines: u64,
}

/// 余地の足りない 1 件（file は repo 相対・core の合計は [`CORE`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headroom {
    /// 余地の足りない file。
    pub file: String,
    /// 残っている行数。
    pub headroom: u64,
}

/// core の合計を名指す `file` の字面。
pub const CORE: &str = "core";

/// 行数（xtask check の file-lines / core-lines と同じ式 = 幅 `width` で正規化した行数・[`super::closure::weighted_lines`]）。
pub fn line_count(text: &str, width: u64) -> u64 {
    let width = usize::try_from(width).unwrap_or(usize::MAX);
    u64::try_from(super::closure::weighted_lines(text, width)).unwrap_or(u64::MAX)
}

/// 上限の余地を測る（**受付だけが撃つ**・pure・I/O は呼び手）。
///
/// `lines` は base の tracked `.rs` の (path, 行数)。write-set の `.rs`（dir は展開した配下・新規 file は 0 行）の
/// うち R-C4-2 の測定範囲（`crates/<c>/src/` 配下＝[`core_of`] が `Some`）のそれぞれについて `file_lines − 行数` を
/// 余地とし、`size_lines` が余地を超える file を名指す（範囲外の `tests/` 等は門の対象外で測らない）。core（write-set
/// の `.rs` が在る `crates/<c>/src/` の総行数）は `size_lines × その core に属する write-set の .rs 本数` を見積として
/// 同じ式で 1 回（母集団は file の余地と同じ [`core_of`] が `Some` の集合＝`tests/` の歯は本数に入れない・C10）。
/// **縮む面（`-`）は増分が負**なので、file の余地も求めず core の本数にも数えない（満杯の file を割る便を受付が
/// 断って満杯が固定される型を塞ぐ・§3「上限の余地」）。
pub fn headroom_shortfalls(items: &[WriteSetItem], lines: &[(String, u64)], caps: Caps) -> Vec<Headroom> {
    let files: Vec<&str> = items
        .iter()
        .flat_map(|item| match *item {
            WriteSetItem::File(ref path) | WriteSetItem::New(ref path) => vec![path.as_str()],
            WriteSetItem::Dir(ref under) => under.iter().map(String::as_str).collect(),
            WriteSetItem::Shrink(_) => Vec::new(),
        })
        .filter(|path| path.ends_with(".rs"))
        .collect();
    let lines_of = |path: &str| lines.iter().find(|(found, _)| found == path).map_or(0, |(_, count)| *count);
    let mut found: Vec<Headroom> = files
        .iter()
        .filter(|path| core_of(path).is_some())
        .filter_map(|path| {
            let headroom = caps.file_lines.saturating_sub(lines_of(path));
            (caps.size_lines > headroom).then(|| Headroom { file: (*path).to_owned(), headroom })
        })
        .collect();
    let mut cores: Vec<&str> = files.iter().filter_map(|path| core_of(path)).collect();
    cores.sort_unstable();
    cores.dedup();
    for core in cores {
        let members = files.iter().filter(|path| core_of(path) == Some(core)).count();
        let estimate = caps.size_lines.saturating_mul(u64::try_from(members).unwrap_or(u64::MAX));
        let total: u64 = lines.iter().filter(|(path, _)| core_of(path) == Some(core)).map(|(_, count)| *count).sum();
        let headroom = caps.core_lines.saturating_sub(total);
        if estimate > headroom {
            found.push(Headroom { file: CORE.to_owned(), headroom });
        }
    }
    found
}

/// `.rs` の path が属する core（`crates/<c>/src/…` の `crates/<c>/src`）。その形でなければ `None`。
fn core_of(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("crates/")?;
    let (crate_name, tail) = rest.split_once("/src/")?;
    let head = path.len().checked_sub(tail.len().saturating_add(1))?;
    (!crate_name.is_empty() && !crate_name.contains('/')).then(|| path.get(..head)).flatten()
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
    use super::{
        headroom_shortfalls, line_count, read_write_set, unfit, Basis, Caps, Ceiling, Declared, Effective, Headroom,
        Holes, Sourced, Unfit, WriteSetItem, BASE_HOLES, BASE_HOLE, CEILING_ROW, CORE, DECL_FILE, DENIED_ROW, JOBS_HOLE,
    };
    use crate::order::is_declaration_order;

    /// 文字列の列。
    fn strings(items: &[&str]) -> Vec<String> {
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

    /// base の tracked file（write-set の項目の fixture）。
    fn base() -> Vec<String> {
        strings(&["crates/toy/src/a.rs", "crates/toy/src/b.rs", "snap/x.snap", "docs/d.md", "crates/toy/tests/t.rs"])
    }

    /// write-set の項目は 4 形（実在する file / 末尾 `/` で配下を持つ dir〔展開される〕/ `+` の新規 file〔base に無い〕/
    /// `-` の縮む file〔base に在る〕）だけが解け、それ以外は**全件**項目の字面で返る（設計 contract-source.md §3
    /// 「項目の実在と展開」）。
    #[test]
    fn declaration_write_set_items_resolve_only_the_three_forms_and_name_every_unresolved_item() {
        let read = read_write_set(
            &strings(&["crates/toy/src/a.rs", "snap/", "+crates/toy/src/new.rs", "-crates/toy/src/b.rs"]),
            &base(),
        );
        assert_eq!(
            read,
            Ok(vec![
                WriteSetItem::File("crates/toy/src/a.rs".to_owned()),
                WriteSetItem::Dir(vec!["snap/x.snap".to_owned()]),
                WriteSetItem::New("crates/toy/src/new.rs".to_owned()),
                WriteSetItem::Shrink("crates/toy/src/b.rs".to_owned()),
            ]),
            "4 形が解け dir は配下に展開され、- は接頭辞を剥がした path で持つ"
        );
        let unresolved = read_write_set(
            &strings(&[
                "crates/toy/src/none.rs",
                "empty/",
                "+crates/toy/src/a.rs",
                "+",
                "snap",
                "-crates/toy/src/none.rs",
                "-",
                "docs/d.md",
            ]),
            &base(),
        );
        assert_eq!(
            unresolved,
            Err(strings(&["crates/toy/src/none.rs", "empty/", "+crates/toy/src/a.rs", "+", "snap", "-crates/toy/src/none.rs", "-"])),
            "無い file・空の dir・base に在る file への +・空の +・base に無い file への -・空の - は解けない（末尾 / 無しの dir も file としては無い）"
        );
    }

    /// 上限の余地: write-set の `.rs` ごとに `file_lines − 行数` を余地とし、size の見積が超える file を名指す。core
    /// （`crates/<c>/src/` の合計）は `見積 × .rs 本数` で 1 回。`.rs` でない項目と別 crate の行は数えない。
    #[test]
    fn declaration_headroom_names_the_file_and_the_core_whose_room_is_short() {
        let lines = vec![
            ("crates/toy/src/a.rs".to_owned(), 1_400),
            ("crates/toy/src/b.rs".to_owned(), 100),
            ("crates/other/src/z.rs".to_owned(), 5_000),
            ("crates/toy/tests/t.rs".to_owned(), 900),
        ];
        let items = read_write_set(&strings(&["crates/toy/src/a.rs", "snap/", "+crates/toy/src/new.rs"]), &base())
            .unwrap_or_default();
        let caps = |size_lines: u64, core_lines: u64| Caps { file_lines: 1_500, core_lines, size_lines };
        assert_eq!(
            headroom_shortfalls(&items, &lines, caps(300, 40_000)),
            vec![Headroom { file: "crates/toy/src/a.rs".to_owned(), headroom: 100 }],
            "M（300）は余地 100 の a.rs に入らない・新規 file は余地いっぱい・core は余裕"
        );
        assert!(headroom_shortfalls(&items, &lines, caps(100, 40_000)).is_empty(), "S（100）は余地 100 に入る");
        // core: 合計 1500（tests/ と別 crate は数えない）・見積 = 100 × 2 本 = 200 > 余地 100。
        assert_eq!(
            headroom_shortfalls(&items, &lines, caps(100, 1_600)),
            vec![Headroom { file: CORE.to_owned(), headroom: 100 }],
            "core の余地は crates/<c>/src/ の合計で 1 回"
        );
        let only_b = read_write_set(&strings(&["crates/toy/src/b.rs", "docs/d.md"]), &base()).unwrap_or_default();
        assert!(headroom_shortfalls(&only_b, &lines, caps(300, 40_000)).is_empty(), "余地の無い file を持たない行は通る");
        // 縮む面（`-`）: 満杯の a.rs を減らす便は file の余地を求めず、core の見積の本数にも数えない（新規 1 本だけ）。
        let shrink = read_write_set(&strings(&["-crates/toy/src/a.rs", "+crates/toy/src/new.rs"]), &base()).unwrap_or_default();
        assert!(headroom_shortfalls(&shrink, &lines, caps(300, 40_000)).is_empty(), "- の a.rs は余地 100 でも M を通す");
        assert!(headroom_shortfalls(&shrink, &lines, caps(100, 1_600)).is_empty(), "core の見積は 100 × 1 本 = 100 ≤ 余地 100");
        assert_eq!(
            headroom_shortfalls(&shrink, &lines, caps(101, 1_600)),
            vec![Headroom { file: CORE.to_owned(), headroom: 100 }],
            "新規 1 本の見積 101 は core の余地 100 を超える（- を数えないだけで core は測る）"
        );
        assert_eq!(line_count("a\nb\n", 120), 2, "幅に収まる行は改行で区切った行の数");
        assert_eq!(line_count("a\nb", 120), 2, "末尾改行の有無で差を出さない");
        assert_eq!(line_count(&format!("{}\nb\n", "a".repeat(250)), 120), 4, "幅を超える行は ceil(250 ÷ 120) = 3 行");
    }

    /// 門の範囲の外（R-C4-2 は `crates/<c>/src/` 配下だけ）の fixture: 余地 50 の src・余地 0 の tests・`.rs` でない doc。
    fn outside_the_gate_range() -> (Vec<WriteSetItem>, Vec<(String, u64)>) {
        let items = vec![
            WriteSetItem::File("crates/toy/src/a.rs".to_owned()),
            WriteSetItem::File("crates/toy/tests/e2e/t.rs".to_owned()),
            WriteSetItem::File("docs/d.md".to_owned()),
        ];
        let lines = vec![("crates/toy/src/a.rs".to_owned(), 1_450), ("crates/toy/tests/e2e/t.rs".to_owned(), 2_000)];
        (items, lines)
    }

    /// 受付の余地は門（R-C4-2）と同じ範囲だけを測る: `tests/` の歯は行数が上限を超えていても名指さない。
    #[test]
    fn declaration_headroom_ignores_files_outside_the_gate_range() {
        let (items, lines) = outside_the_gate_range();
        let caps = Caps { file_lines: 1_500, core_lines: 40_000, size_lines: 100 };
        assert_eq!(
            headroom_shortfalls(&items, &lines, caps),
            vec![Headroom { file: "crates/toy/src/a.rs".to_owned(), headroom: 50 }],
            "名指すのは src の a.rs だけ（tests/e2e/t.rs は門の対象外）"
        );
    }

    /// core の見積の本数も門の範囲だけ: src 1 本 + tests 1 本 + doc の write-set は `size_lines × 1`（tests/ の歯を
    /// 本数に入れると 2 本で余地を超え、src だけなら通る便を受付が断る＝.303 run 3 の型）。
    #[test]
    fn declaration_headroom_core_estimate_counts_only_files_in_the_gate_range() {
        let (items, _) = outside_the_gate_range();
        let lines = vec![("crates/toy/src/a.rs".to_owned(), 100), ("crates/toy/tests/e2e/t.rs".to_owned(), 2_000)];
        let caps = |core_lines: u64| Caps { file_lines: 1_500, core_lines, size_lines: 100 };
        assert!(
            headroom_shortfalls(&items, &lines, caps(250)).is_empty(),
            "core の見積は src 1 本 × 100 = 100 ≤ 余地 150（tests/e2e/t.rs は本数に入れない）"
        );
        assert_eq!(
            headroom_shortfalls(&items, &lines, caps(150)),
            vec![Headroom { file: CORE.to_owned(), headroom: 50 }],
            "余地 50 では src 1 本の見積 100 が超える（見積が 0 に潰れていない）"
        );
    }

    /// 余地を測る file の集合 = `core_of` が `Some` の file の集合（xtask の file-lines と同じ範囲）。
    #[test]
    fn declaration_headroom_range_matches_the_gate_predicate() {
        let (items, lines) = outside_the_gate_range();
        // 余地を必ず超える見積で、測られた file が全部名指される形にする（core は余裕）。
        let caps = Caps { file_lines: 1_500, core_lines: u64::MAX, size_lines: 1_501 };
        let measured: Vec<String> =
            headroom_shortfalls(&items, &lines, caps).into_iter().map(|found| found.file).collect();
        let in_range: Vec<String> = items
            .iter()
            .filter_map(|item| match *item {
                WriteSetItem::File(ref path) | WriteSetItem::New(ref path) => {
                    super::core_of(path).map(|_| path.clone())
                }
                WriteSetItem::Dir(_) | WriteSetItem::Shrink(_) => None,
            })
            .collect();
        assert_eq!(measured, in_range, "余地を測る範囲は core_of の述語と同じ");
        assert_eq!(measured, strings(&["crates/toy/src/a.rs"]), "範囲は空でない（空虚な一致を断つ）");
    }

    /// 宣言の共通 verify に置ける穴は**閉じた集合**であり、その外は `Hole` で断る
    /// （ADR-0021 §2.1 が ADR-0010 §2.1 を部分 supersede）。
    ///
    /// 両向きを 1 本で撃つ: 集合を空にする変異も、逆に全部の穴を通す変異も、ここで落ちる。
    #[test]
    fn declaration_accepts_only_the_closed_set_of_holes_in_common_verify() {
        let (allowed, denied) = (strings(&["cargo"]), denied());
        let basis = Basis { allowed: &allowed, denied: &denied };
        assert_eq!(BASE_HOLES, [BASE_HOLE, JOBS_HOLE], "集合は 2 つちょうど");
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
        // 2 つを同じ行に置ける（scribe2 自身の宣言の形）。
        assert_eq!(
            unfit("cargo xtask mutants-diff --base {base} --jobs {jobs}", &basis, Holes::Base),
            None,
            "2 つの穴を同じ行に置ける"
        );
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
        let made = effective(r#"["cargo", "git"]"#, r#"["cargo xtask check"]"#);
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
        let absent = effective(r#"["cargo"]"#, r#"["cargo xtask check"]"#);
        assert!(absent.detection_verify().is_empty(), "無い key は空");
        assert!(!absent.render().contains("detection-verify"), "空の周は key ごと書かない: {}", absent.render());

        let text = format!("{}detection-verify = [\"cargo xtask mutants-diff --base {{base}} --jobs {{jobs}}\"]\n", body(r#"["cargo"]"#, r#"["cargo xtask check"]"#));
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
}
