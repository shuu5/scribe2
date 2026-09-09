//! 契約 file の読み取り（設計 §3・FR1 / FR2・ADR-0004 §2.3）。
//!
//! section を持たない flat な TOML subset である。値は文字列と**文字列の配列**だけで、
//! 値そのものの受理集合は [`rules::manifest::scalar`] と共有する（第 2 の値 parser を
//! 作らない・憲法 C6）。配列は `key = ["a", "b"]` の形で **1 行に収まる**こと。
//!
//! **検査は 1 か所に閉じ、見つけた不備は全件集めて返す**（C2 / FR1）。1 件目で止めると
//! 直すたびに次の 1 件が出る形になり、契約を書き切れない。

use crate::rules::manifest::{scalar, Scalar};
use std::path::Path;

/// 必ず在る key（この順で報告する）。
const REQUIRED: &[&str] = &[
    "goal",
    "done",
    "size",
    "owner",
    "disposition",
    "write-set",
    "verify",
    "req",
    "design",
];

/// 任意の key。
const OPTIONAL: &[&str] = &["classes"];

/// 3 クラスの自己申告が取れる値（FR15）。
pub const CLASSES: &[&str] = &["delete", "publish", "consume"];

/// 契約 file が読めない理由。**行番号を必ず持つ**（0 は file 全体を指す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError {
    /// 何行目か（1 始まり・0 は file 全体）。
    pub line: u64,
    /// なぜ読めないか。
    pub reason: String,
}

impl ContractError {
    /// 1 件を組む。
    fn new(line: u64, reason: String) -> Self {
        Self { line, reason }
    }
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "contract: {} line={}", self.reason, self.line)
    }
}

/// 読み込み済みの契約。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    /// 1 行の目的。
    pub goal: String,
    /// 何ができたら終わりか。
    pub done: String,
    /// 見積の目安。
    pub size: String,
    /// bead id（文字列として持つだけ・台帳は読まない）。
    pub owner: String,
    /// 処分。
    pub disposition: String,
    /// 触ってよい file / dir。
    pub write_set: Vec<String>,
    /// 検証コマンド（各要素は 1 行で完結する）。
    pub verify: Vec<String>,
    /// SRS の要件 id。
    pub req: Vec<String>,
    /// 設計 doc の repo 相対 path。
    pub design: String,
    /// 3 クラスの自己申告（既定 空）。
    pub classes: Vec<String>,
}

/// 走査中の 1 key の値。
enum Raw {
    /// 文字列。
    Text(String),
    /// 文字列の配列。
    List(Vec<String>),
}

impl Contract {
    /// file から読む。読めない file 自体も 1 件の error にする。
    pub fn load(path: &Path) -> Result<Self, Vec<ContractError>> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(err) => Err(vec![ContractError::new(
                0,
                format!("{} を読めない: {err}", path.display()),
            )]),
        }
    }

    /// 本文から読む。**不備は全件集めて返す**。
    pub fn parse(text: &str) -> Result<Self, Vec<ContractError>> {
        let mut errors = Vec::new();
        let found = scan(text, &mut errors);
        check_required(&found, &mut errors);
        let built = build(&found, &mut errors);
        match built {
            Some(contract) if errors.is_empty() => Ok(contract),
            _ => Err(errors),
        }
    }
}

/// 1 行ずつ読み、key → 値 を集める。行の形の不備はここで全件積む。
fn scan(text: &str, errors: &mut Vec<ContractError>) -> Vec<(String, Raw, u64)> {
    let mut found: Vec<(String, Raw, u64)> = Vec::new();
    for (index, raw_line) in text.lines().enumerate() {
        let line = index as u64 + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = trimmed.split_once('=') else {
            errors.push(ContractError::new(
                line,
                format!("key = value の形でない: {trimmed}"),
            ));
            continue;
        };
        let key = key.trim().to_owned();
        if !REQUIRED.contains(&key.as_str()) && !OPTIONAL.contains(&key.as_str()) {
            errors.push(ContractError::new(line, format!("未知の key {key}")));
            continue;
        }
        if found.iter().any(|(seen, _, _)| *seen == key) {
            errors.push(ContractError::new(line, format!("key {key} が重複する")));
            continue;
        }
        if let Some(value) = value_of(&key, raw_value.trim(), line, errors) {
            found.push((key, value, line));
        }
    }
    found
}

/// 1 つの値を読む。配列は 1 行に収まっていること。
fn value_of(key: &str, raw: &str, line: u64, errors: &mut Vec<ContractError>) -> Option<Raw> {
    if !raw.starts_with('[') {
        return match scalar(raw) {
            Some(Scalar::Str(text)) => Some(Raw::Text(text)),
            _ => {
                errors.push(ContractError::new(
                    line,
                    format!("{key} の value が文字列でない"),
                ));
                None
            }
        };
    }
    if !raw.ends_with(']') {
        errors.push(ContractError::new(
            line,
            format!("{key} の配列が同じ行で閉じていない（1 行で完結すること）"),
        ));
        return None;
    }
    let Some(parts) = elements(raw) else {
        errors.push(ContractError::new(line, format!("{key} の配列の形でない")));
        return None;
    };
    let mut list = Vec::new();
    for part in parts {
        match scalar(part.trim()) {
            Some(Scalar::Str(text)) => list.push(text),
            _ => errors.push(ContractError::new(
                line,
                format!("{key} の要素が文字列でない: {}", part.trim()),
            )),
        }
    }
    Some(Raw::List(list))
}

/// `["a", "b"]` を要素へ切る。要素の中の `,` は quote の内側として扱う。
fn elements(raw: &str) -> Option<Vec<String>> {
    let body = raw.strip_prefix('[')?.strip_suffix(']')?;
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut inside = false;
    for ch in body.chars() {
        match ch {
            '"' => {
                inside = !inside;
                current.push(ch);
            }
            ',' if !inside => parts.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    if inside {
        return None;
    }
    parts.push(current);
    if parts.last().is_some_and(|last| last.trim().is_empty()) {
        parts.pop();
    }
    Some(parts)
}

/// 必須 key の欠落を全件積む。
fn check_required(found: &[(String, Raw, u64)], errors: &mut Vec<ContractError>) {
    for key in REQUIRED {
        if !found.iter().any(|(seen, _, _)| seen == key) {
            errors.push(ContractError::new(0, format!("必須の key {key} が無い")));
        }
    }
}

/// 文字列 key を取り出す。型違いはここで積む。
fn text_of(found: &[(String, Raw, u64)], key: &str, errors: &mut Vec<ContractError>) -> String {
    match found.iter().find(|(seen, _, _)| seen == key) {
        None => String::new(),
        Some((_, Raw::Text(text), _)) => text.clone(),
        Some((_, Raw::List(_), line)) => {
            errors.push(ContractError::new(
                *line,
                format!("{key} は文字列である（配列でない）"),
            ));
            String::new()
        }
    }
}

/// 配列 key を取り出す。`least` 本に満たなければ積む。
fn list_of(
    found: &[(String, Raw, u64)],
    key: &str,
    least: usize,
    errors: &mut Vec<ContractError>,
) -> Vec<String> {
    match found.iter().find(|(seen, _, _)| seen == key) {
        None => Vec::new(),
        Some((_, Raw::Text(_), line)) => {
            errors.push(ContractError::new(
                *line,
                format!("{key} は配列である（文字列でない）"),
            ));
            Vec::new()
        }
        Some((_, Raw::List(list), line)) => {
            if list.len() < least {
                errors.push(ContractError::new(
                    *line,
                    format!("{key} は 1 本以上要る（実 {} 本）", list.len()),
                ));
            }
            list.clone()
        }
    }
}

/// 集めた key から契約を組み、値の不備を全件積む。
fn build(found: &[(String, Raw, u64)], errors: &mut Vec<ContractError>) -> Option<Contract> {
    let verify = list_of(found, "verify", 1, errors);
    let line = found
        .iter()
        .find(|(key, _, _)| key == "verify")
        .map_or(0, |(_, _, at)| *at);
    for item in &verify {
        if item.contains('\n') || item.contains('\r') {
            errors.push(ContractError::new(
                line,
                format!("verify の要素に改行が在る: {item}"),
            ));
        }
    }
    let classes = list_of(found, "classes", 0, errors);
    let at = found
        .iter()
        .find(|(key, _, _)| key == "classes")
        .map_or(0, |(_, _, found_at)| *found_at);
    for item in &classes {
        if !CLASSES.contains(&item.as_str()) {
            errors.push(ContractError::new(
                at,
                format!("未知の classes 値 {item}（取るのは {}）", CLASSES.join(" / ")),
            ));
        }
    }
    Some(Contract {
        goal: text_of(found, "goal", errors),
        done: text_of(found, "done", errors),
        size: text_of(found, "size", errors),
        owner: text_of(found, "owner", errors),
        disposition: text_of(found, "disposition", errors),
        write_set: list_of(found, "write-set", 1, errors),
        verify,
        req: list_of(found, "req", 1, errors),
        design: text_of(found, "design", errors),
        classes,
    })
}
