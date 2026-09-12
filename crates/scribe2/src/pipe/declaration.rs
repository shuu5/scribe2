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

use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::{list, scalar, Scalar};
use std::path::Path;

/// 対象 repo の root に置く宣言 file の名。
pub const DECL_FILE: &str = ".vessel.toml";

/// 上限を持つ rules 行の id。
pub const CEILING_ROW: &str = "runner.allowed_commands";

/// 宣言 file の schema。
const SCHEMA_VERSION: u64 = 1;

/// 宣言が持つ key（この順で報告する）。
const DECLARED_KEYS: &[&str] = &["schema", "allowed-commands", "common-verify", DETECTION_KEY];

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
const OPTIONAL_KEYS: &[&str] = &[DETECTION_KEY];

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

/// 器が持つ上限（rules 行）。
pub struct Ceiling<'a> {
    /// 上限を持つ行の id（出所として写しへ残る）。
    pub row: &'a str,
    /// 許す command の上限。
    pub commands: &'a [String],
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
    Sourced::read(repo, ceiling)?.measure(ceiling.commands, contract_verify)
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

    /// 上限・宣言 allowlist・契約 verify と突き合わせる。
    fn measure(
        self,
        ceiling: &[String],
        contract_verify: &[String],
    ) -> Result<Effective, Vec<DeclError>> {
        let declared = &self.declared;
        let mut errors = Vec::new();
        for command in &declared.allowed {
            if !ceiling.iter().any(|top| top == command) {
                errors.push(DeclError::new(
                    declared.allowed_line,
                    format!(
                        "allowed-commands の {command} が上限 {}（{}）の外である",
                        self.ceiling,
                        ceiling.join(" / ")
                    ),
                ));
            }
        }
        check_lines("common-verify", &declared.common_verify, declared.common_line, declared, &mut errors);
        // **検出線の行にも同じ検査を掛ける**（ADR-0010 §2.3 (2)・ADR-0021 §2.6・lens-132d H1）。
        // 掛けないと、共通 verify で断った迂回行を検出線の側へ置くだけで撃たせられる。
        check_lines(DETECTION_KEY, &declared.detection_verify, declared.detection_line, declared, &mut errors);
        check_contract(contract_verify, declared, &mut errors);
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
fn check_lines(key: &str, lines: &[String], at: u64, declared: &Declared, errors: &mut Vec<DeclError>) {
    for line in lines {
        if let Some(found) = unfit(line, &declared.allowed, Holes::Base) {
            errors.push(DeclError::new(
                at,
                format!("{key} {line:?}: {}", found.reason(&declared.allowed)),
            ));
        }
    }
}

/// 契約の verify を全件見る。**基準は宣言の allowlist**（上限ではない）で、契約行は穴を持てない。
fn check_contract(lines: &[String], declared: &Declared, errors: &mut Vec<DeclError>) {
    for (index, line) in lines.iter().enumerate() {
        if let Some(found) = unfit(line, &declared.allowed, Holes::None) {
            errors.push(DeclError::new(
                0,
                format!(
                    "契約の verify {} 本目 {line:?}: {}",
                    index.saturating_add(1),
                    found.reason(&declared.allowed)
                ),
            ));
        }
    }
}

/// 1 行が **argv 1 本**として撃てるかを見る。**この 1 本が唯一の判定**である。
fn unfit(line: &str, allowed: &[String], holes: Holes) -> Option<Unfit> {
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
    holes_in(line)
        .into_iter()
        .find(|hole| !(holes == Holes::Base && BASE_HOLES.contains(&hole.as_str())))
        .map(Unfit::Hole)
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
            })
        } else {
            Err(errors)
        }
    }
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
        unfit, Declared, Effective, Holes, Sourced, Unfit, BASE_HOLES, BASE_HOLE, CEILING_ROW,
        DECL_FILE, JOBS_HOLE,
    };
    use crate::order::is_declaration_order;

    /// 宣言の共通 verify に置ける穴は**閉じた集合**であり、その外は `Hole` で断る
    /// （ADR-0021 §2.1 が ADR-0010 §2.1 を部分 supersede）。
    ///
    /// 両向きを 1 本で撃つ: 集合を空にする変異も、逆に全部の穴を通す変異も、ここで落ちる。
    #[test]
    fn declaration_accepts_only_the_closed_set_of_holes_in_common_verify() {
        let allowed = ["cargo".to_owned()];
        assert_eq!(BASE_HOLES, [BASE_HOLE, JOBS_HOLE], "集合は 2 つちょうど");
        for hole in BASE_HOLES {
            let line = format!("cargo xtask mutants-diff --base {hole}");
            assert_eq!(unfit(&line, &allowed, Holes::Base), None, "{hole} は共通 verify に置ける");
            assert_eq!(
                unfit(&line, &allowed, Holes::None),
                Some(Unfit::Hole((*hole).to_owned())),
                "{hole} も契約の verify には置けない"
            );
        }
        // 集合の外は**綴り違いでも**断る（`{jobz}` を黙って撃たない）。
        for outside in ["{jobz}", "{job}", "{jobs", "{JOBS}"] {
            let line = format!("cargo xtask mutants-diff --jobs {outside}");
            assert_eq!(
                unfit(&line, &allowed, Holes::Base),
                Some(Unfit::Hole(outside.to_owned())),
                "{outside} は置けない穴である"
            );
        }
        // 2 つを同じ行に置ける（scribe2 自身の宣言の形）。
        assert_eq!(
            unfit("cargo xtask mutants-diff --base {base} --jobs {jobs}", &allowed, Holes::Base),
            None,
            "2 つの穴を同じ行に置ける"
        );
    }

    /// 宣言の本文。
    fn body(allowed: &str, common: &str) -> String {
        format!("schema = 1\nallowed-commands = {allowed}\ncommon-verify = {common}\n")
    }

    /// 上限を通った有効値を **実経路と同じ 3 段**で組む。
    fn effective(allowed: &str, common: &str) -> Effective {
        let declared = Declared::parse(&body(allowed, common)).expect("宣言を読める");
        let ceiling = ["cargo".to_owned(), "git".to_owned()];
        Sourced {
            declared,
            commit: "c0ffee".to_owned(),
            source: DECL_FILE.to_owned(),
            ceiling: CEILING_ROW.to_owned(),
        }
        .measure(&ceiling, &[])
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
        let made = Sourced { declared, commit: "c0ffee".to_owned(), source: DECL_FILE.to_owned(), ceiling: CEILING_ROW.to_owned() }
            .measure(&["cargo".to_owned()], &[])
            .expect("穴 2 つの検出線は通る");
        assert_eq!(made.detection_verify().len(), 1, "検出線を 1 行持つ");
        let read = Effective::parse(&made.render()).expect("写しを読み戻せる");
        assert_eq!(read, made, "検出線ごと round trip する: {}", made.render());

        let empty = format!("{}detection-verify = []\n", body(r#"["cargo"]"#, r#"["cargo xtask check"]"#));
        let errors = Declared::parse(&empty).expect_err("書いた空配列は不備");
        assert!(errors.iter().any(|error| error.reason.contains("配列が空である")), "{errors:?}");
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
        ];
        let order: Vec<&Unfit> = declared.iter().collect();
        assert!(
            is_declaration_order(&order, Unfit::rank),
            "rank は宣言順に 0.. である（並べ替え・重複・中間の欠番を落とす）: {declared:?}"
        );

        // 母集団 = 2 つ以上の理由に同時に当たる行 3 本（当たる理由は fixture が持つ）。
        let allowed = ["cargo".to_owned()];
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
        ] {
            assert!(hit.len() >= 2, "{line:?} は複数の理由に当たる形である: {hit:?}");
            let first = hit.iter().min_by_key(|found| found.rank()).cloned();
            assert_eq!(
                unfit(line, &allowed, holes),
                first,
                "{line:?} が当たる理由 {hit:?} のうち宣言順で最初のものを返す"
            );
        }
    }
}
