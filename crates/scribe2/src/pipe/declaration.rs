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
const DECLARED_KEYS: &[&str] = &["schema", "allowed-commands", "common-verify"];

/// 便の写しが持つ key（宣言の 3 つ + 出所 3 つ）。
const EFFECTIVE_KEYS: &[&str] = &[
    "schema",
    "allowed-commands",
    "common-verify",
    "commit",
    "source",
    "ceiling",
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

/// 器が持つ上限（rules 行）。
pub struct Ceiling<'a> {
    /// 上限を持つ行の id（出所として写しへ残る）。
    pub row: &'a str,
    /// 許す command の上限。
    pub commands: &'a [String],
}

/// 行が置ける穴。**穴の可否だけが宣言の共通 verify と契約の verify の違い**である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holes {
    /// `{base}` だけ置ける（宣言の共通 verify）。
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
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unfit {
    /// 語が 1 つも無い。
    Empty,
    /// 先頭語が宣言の allowlist に無い。
    Command(String),
    /// shell が意味を変える文字を含む。
    Metachar(char),
    /// repo の外を指す語を含む（絶対 path・home の短縮記号・`..` で遡る path）。
    Outside(String),
    /// 置けない穴を含む。
    Hole(String),
}

impl Unfit {
    /// 断る理由の 1 行。
    fn reason(&self, allowed: &[String]) -> String {
        match *self {
            Self::Empty => "verify 行が空である".to_owned(),
            Self::Command(ref head) => {
                format!("先頭 command {head} が宣言の allowed-commands（{}）に無い", allowed.join(" / "))
            }
            Self::Metachar(found) => format!(
                "shell の制御文字 {found:?} を含む（1 行 1 command・包みも連結も書けない）"
            ),
            Self::Outside(ref word) => {
                format!("repo の外を指す語 {word} を含む（絶対 path・home の短縮記号・.. で遡る path）")
            }
            Self::Hole(ref hole) => format!("置けない穴 {hole} を含む"),
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
    /// `allowed-commands` が書かれていた行。
    allowed_line: u64,
    /// `common-verify` が書かれていた行。
    common_line: u64,
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
        check_lines(&declared.common_verify, declared, Holes::Base, &mut errors);
        check_contract(contract_verify, declared, &mut errors);
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Effective {
            allowed: declared.allowed.clone(),
            common_verify: declared.common_verify.clone(),
            commit: self.commit,
            source: self.source,
            ceiling: self.ceiling,
        })
    }
}

/// 宣言の共通 verify を全件見る。
fn check_lines(lines: &[String], declared: &Declared, holes: Holes, errors: &mut Vec<DeclError>) {
    for line in lines {
        if let Some(found) = unfit(line, &declared.allowed, holes) {
            errors.push(DeclError::new(
                declared.common_line,
                format!("common-verify {line:?}: {}", found.reason(&declared.allowed)),
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
    holes_in(line)
        .into_iter()
        .find(|hole| !(holes == Holes::Base && hole == "{base}"))
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
        if schema != Some(SCHEMA_VERSION) {
            errors.push(DeclError::new(
                0,
                format!("schema は {SCHEMA_VERSION} である（実 {schema:?}）"),
            ));
        }
        if errors.is_empty() {
            Ok(Self { allowed, common_verify, allowed_line, common_line })
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

    /// 便の写しの本文（**同じ reader で読み戻せる**形）。
    pub fn render(&self) -> String {
        format!(
            "schema = {SCHEMA_VERSION}\nallowed-commands = {}\ncommon-verify = {}\ncommit = \"{}\"\nsource = \"{}\"\nceiling = \"{}\"\n",
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
        let commit = text_of(&found, "commit", &mut errors);
        let source = text_of(&found, "source", &mut errors);
        let ceiling = text_of(&found, "ceiling", &mut errors);
        if schema != Some(SCHEMA_VERSION) {
            errors.push(DeclError::new(0, format!("schema は {SCHEMA_VERSION} である")));
        }
        if errors.is_empty() {
            Ok(Self { allowed, common_verify, commit, source, ceiling })
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
        if !seen.iter().any(|name| name == key) {
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
    use super::{Declared, Effective, Sourced, CEILING_ROW, DECL_FILE};

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
}
