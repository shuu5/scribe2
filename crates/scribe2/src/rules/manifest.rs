//! `rules/manifest.toml` を std だけで読む面（ADR-0004 §2.3・SRS NFR3）。
//!
//! 受理するのは TOML の部分集合である: 先頭の `schema = 1`・`[[rule]]` の
//! array-of-tables・値は string / integer / bool と**文字列の配列**（1 行で閉じる）。
//! **最初の 1 件で止めず**違反を全件集めて返す（silent drop 禁止・SRS NFR4）。

use super::{Rule, RuleError, RuleKind, RuleRow, RuleValue, ValueShape};
use std::path::Path;

/// build 時に binary へ埋め込む manifest の本文。
///
/// 別 repo の worktree で走る便でも path に依存せず同じ規則を読むための形である
/// （憲法 C1・単一 static binary の向き）。
const EMBEDDED: &str = include_str!("../../../../rules/manifest.toml");

/// manifest が要求する schema 版。
const SCHEMA: u64 = 1;

/// 行が持てる key の全体。ここに無い key は拒む。
const KNOWN_KEYS: &[&str] = &["id", "kind", "value", "enabled", "ruling", "ruled_at"];

/// 行に必ず要る key。
const REQUIRED_KEYS: &[&str] = &["id", "kind", "value", "ruling", "ruled_at"];

/// TOML subset が受理する値。
///
/// `pipe` の契約 file も同じ subset の値を持つので、この型と [`scalar`] を器の中で
/// 共有する（第 2 の値 parser を作らない・憲法 C6）。**受理集合はここが唯一の定義**で、
/// 配列の層も同じ module の [`list`] / [`elements`] / [`quoted_once`] が持つ
/// （rules manifest も契約 file も同じ切り方で読む・s2-07l.55）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scalar {
    /// 非負整数。
    Int(u64),
    /// 文字列。
    Str(String),
    /// 真偽。
    Bool(bool),
}

/// 1 つの key が持てる生の値。**配列の層はここが唯一の定義**である。
///
/// `pipe` の契約 file も同じ切り方（[`elements`] / [`quoted_once`]）を使う
/// ——配列を読む実装が 2 本あると、書いた本数と通る本数の食い違いが片側だけ直る。
#[derive(Debug, Clone, PartialEq, Eq)]
enum RawValue {
    /// 単一の値。
    One(Scalar),
    /// 文字列の列。
    List(Vec<String>),
    /// 読めなかった値。**scan の時点で 1 件報告済み**なので、以降の段はこの値に
    /// ついて何も言わない——同じ欠陥を 2 行にしないためであり、とりわけ
    /// 「必須 key value が無い」と**嘘をつかない**ため（key は在って値が壊れている）。
    Broken,
}

/// `[[rule]]` 1 つ分の生の key/value。
struct RawRow {
    line: u64,
    fields: Vec<(String, RawValue, u64)>,
}

/// 読み込み済みの manifest。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    rows: Vec<RuleRow>,
}

impl Manifest {
    /// binary に埋め込んだ manifest を読む。
    pub fn embedded() -> Result<Self, Vec<RuleError>> {
        Self::parse(EMBEDDED)
    }

    /// file から読む（`--rules PATH` の override）。
    pub fn load(path: &Path) -> Result<Self, Vec<RuleError>> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(err) => Err(vec![RuleError::new(
                0,
                format!("{} を読めない: {err}", path.display()),
            )]),
        }
    }

    /// 本文を読む。違反は全件集めて返す。
    pub fn parse(text: &str) -> Result<Self, Vec<RuleError>> {
        let mut errors = Vec::new();
        let (schema, raws) = scan(text, &mut errors);
        check_schema(schema, &mut errors);
        let mut rows = Vec::new();
        for raw in &raws {
            if let Some(row) = build_row(raw, &mut errors) {
                rows.push(row);
            }
        }
        check_duplicate_ids(&rows, &mut errors);
        if errors.is_empty() {
            Ok(Self { rows })
        } else {
            errors.sort_by_key(|error| error.line);
            Err(errors)
        }
    }

    /// 行 id で引く。
    pub fn get(&self, id: &str) -> Option<&RuleRow> {
        self.rows.iter().find(|row| row.id == id)
    }

    /// 全行。
    pub fn rows(&self) -> &[RuleRow] {
        &self.rows
    }
}

/// 本文を走査して top-level の `schema` と `[[rule]]` の生 field を集める。
fn scan(text: &str, errors: &mut Vec<RuleError>) -> (Option<(u64, Scalar)>, Vec<RawRow>) {
    let mut schema = None;
    let mut raws: Vec<RawRow> = Vec::new();
    for (index, raw_line) in text.lines().enumerate() {
        let line = index as u64 + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            if trimmed == "[[rule]]" {
                raws.push(RawRow {
                    line,
                    fields: Vec::new(),
                });
            } else {
                errors.push(RuleError::new(
                    line,
                    format!("未知の section {trimmed}（受理するのは [[rule]] だけ）"),
                ));
            }
            continue;
        }
        scan_pair(trimmed, line, &mut schema, raws.last_mut(), errors);
    }
    (schema, raws)
}

/// `key = value` 1 行を、直前の section に応じて振り分ける。
fn scan_pair(
    trimmed: &str,
    line: u64,
    schema: &mut Option<(u64, Scalar)>,
    current: Option<&mut RawRow>,
    errors: &mut Vec<RuleError>,
) {
    let Some((key, raw_value)) = trimmed.split_once('=') else {
        errors.push(RuleError::new(line, format!("key = value の形でない: {trimmed}")));
        return;
    };
    let key = key.trim().to_owned();
    let raw = raw_value.trim();
    let value = if raw.starts_with('[') {
        match list(raw) {
            Ok(items) => RawValue::List(items),
            Err(reason) => {
                errors.push(RuleError::new(line, format!("{key} の {reason}")));
                RawValue::Broken
            }
        }
    } else {
        match scalar(raw) {
            Some(found) => RawValue::One(found),
            None => {
                errors.push(RuleError::new(
                    line,
                    format!(
                        "{key} の value が TOML subset の形でない（string / integer / bool / 文字列の配列のみ）"
                    ),
                ));
                RawValue::Broken
            }
        }
    };
    match current {
        Some(row) => row.fields.push((key, value, line)),
        None if key == "schema" && schema.is_some() => {
            errors.push(RuleError::new(line, "schema が重複する".to_owned()));
        }
        None if key == "schema" => match value {
            RawValue::One(found) => *schema = Some((line, found)),
            RawValue::List(_) => {
                errors.push(RuleError::new(line, "schema は配列でない".to_owned()));
            }
            // 読めなかった値は scan が報告済み（`schema` は未設定のまま＝
            // `check_schema` が「無い」と言う。値の欠陥と schema の不在は別件である）。
            RawValue::Broken => {}
        },
        None => errors.push(RuleError::new(
            line,
            format!("[[rule]] の外に未知の key {key} が在る"),
        )),
    }
}

/// TOML の**文字列の配列**を要素へ切る（1 行で閉じること）。
///
/// **空の配列は受けない**——「規則が無い」を空 array で表せてしまうと、書き間違いの
/// `value = []` が allowlist を空のまま効かせる（空の口は「成功」に化ける）。
/// 要素は [`scalar`] で読み、引用符 1 組ちょうどでなければ受けない
/// （`["a" "b"]` の区切り忘れを 1 本の壊れた文字列として黙って通さない・NFR4）。
/// **空文字の要素も受けない**——command 名や verify 行として空を通すと、何もしない口が
/// 規則の顔で並ぶ。
pub fn list(raw: &str) -> Result<Vec<String>, String> {
    if !raw.ends_with(']') {
        return Err("配列が同じ行で閉じていない（要素に改行は置けない）".to_owned());
    }
    let Some(parts) = elements(raw) else {
        return Err("配列の引用符が閉じていない".to_owned());
    };
    if parts.is_empty() {
        return Err("配列が空である（規則が無いことを空の配列で表さない）".to_owned());
    }
    let mut items = Vec::new();
    for part in parts {
        let trimmed = part.trim();
        match scalar(trimmed).filter(|_| quoted_once(trimmed)) {
            Some(Scalar::Str(text)) if !text.is_empty() => items.push(text),
            Some(Scalar::Str(_)) => return Err(format!("配列の要素が空文字である: {trimmed}")),
            _ => return Err(format!("配列の要素が引用符 1 組の文字列でない: {trimmed}")),
        }
    }
    Ok(items)
}

/// 要素が引用符 1 組ちょうどか（中に裸の `"` を含まない）。
///
/// **配列を読む面はここと [`elements`] の 1 組だけ**である（`pipe` の契約 file も
/// これを呼ぶ）。
pub fn quoted_once(text: &str) -> bool {
    text.trim()
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .is_some_and(|body| !body.contains('"'))
}

/// `["a", "b"]` を要素へ切る。要素の中の `,` は quote の内側として扱う。
pub fn elements(raw: &str) -> Option<Vec<String>> {
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

/// 値 1 つを読む。受理するのは `"..."` / 整数 / `true` / `false` だけ。
pub fn scalar(raw: &str) -> Option<Scalar> {
    if let Some(rest) = raw.strip_prefix('"') {
        return rest.strip_suffix('"').map(|text| Scalar::Str(text.to_owned()));
    }
    match raw {
        "true" => return Some(Scalar::Bool(true)),
        "false" => return Some(Scalar::Bool(false)),
        _ => {}
    }
    let digits: String = raw.chars().filter(|c| *c != '_').collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u64>().ok().map(Scalar::Int)
}

/// `schema = 1` が在ることを確かめる。
fn check_schema(schema: Option<(u64, Scalar)>, errors: &mut Vec<RuleError>) {
    match schema {
        Some((_, Scalar::Int(found))) if found == SCHEMA => {}
        Some((line, found)) => errors.push(RuleError::new(
            line,
            format!("schema が {SCHEMA} でない（実 {found:?}）"),
        )),
        None => errors.push(RuleError::new(0, format!("schema = {SCHEMA} が無い"))),
    }
}

/// 生 field から 1 行を組む。欠けや未知 key は全件 `errors` へ積む。
fn build_row(raw: &RawRow, errors: &mut Vec<RuleError>) -> Option<RuleRow> {
    let before = errors.len();
    check_keys(raw, errors);
    // **読めなかった値が 1 つでもあれば、この行はここで打ち切る**。scan が既に
    // 「value が TOML subset の形でない」を 1 件報告しており、続けると同じ欠陥が
    // 「必須 key が無い」「ruling / ruled_at が無い」「id が空である」という
    // **事実でない 2 行目**に化ける（key は在って値が壊れている）。
    if raw
        .fields
        .iter()
        .any(|(_, value, _)| matches!(value, RawValue::Broken))
    {
        return None;
    }
    let id = text_field(raw, "id", errors).unwrap_or_default();
    if id.is_empty() && raw.fields.iter().any(|(key, _, _)| key == "id") {
        errors.push(RuleError::new(raw.line, "id が空である".to_owned()));
    }
    let kind = kind_field(raw, &id, errors);
    let enabled = bool_field(raw, "enabled", errors).unwrap_or(true);
    let ruling = text_field(raw, "ruling", errors).unwrap_or_default();
    let ruled_at = text_field(raw, "ruled_at", errors).unwrap_or_default();
    let value = kind.and_then(|found| value_field(raw, found, &id, errors));
    let (Some(kind), Some(value)) = (kind, value) else {
        return None;
    };
    let row = RuleRow {
        id,
        kind,
        value,
        enabled,
        ruling,
        ruled_at,
        line: raw.line,
    };
    if errors.len() > before {
        // key の欠けや未知 key は既に 1 件として報告済みである。ここで validate を
        // 重ねると同じ欠陥が 2 行になるので、報告済みの行はここで打ち切る。
        return None;
    }
    if let Err(error) = row.validate() {
        errors.push(error);
        return None;
    }
    Some(row)
}

/// 未知 key・重複 key・必須 key の欠落を集める。
///
/// 重複を拒むのは、同じ key を 2 度書いたとき先勝ちで後の行が**黙って消える**のを
/// 塞ぐためである（SRS AC6「黙って落とす入力 0 件」・NFR4）。とりわけ
/// `enabled = true` の次に `enabled = false` を書くと、不発効の行が有効なまま
/// 機械に読まれてしまう。
fn check_keys(raw: &RawRow, errors: &mut Vec<RuleError>) {
    for (index, (key, _, line)) in raw.fields.iter().enumerate() {
        if !KNOWN_KEYS.contains(&key.as_str()) {
            errors.push(RuleError::new(*line, format!("未知の key {key}")));
        }
        if raw
            .fields
            .iter()
            .take(index)
            .any(|(earlier, _, _)| earlier == key)
        {
            errors.push(RuleError::new(*line, format!("key {key} が重複する")));
        }
    }
    for want in REQUIRED_KEYS {
        if !raw.fields.iter().any(|(key, _, _)| key == want) {
            errors.push(RuleError::new(raw.line, format!("必須 key {want} が無い")));
        }
    }
}

/// 文字列 field を取り出す。型違いは error にする。
fn text_field(raw: &RawRow, key: &str, errors: &mut Vec<RuleError>) -> Option<String> {
    let (_, value, line) = raw.fields.iter().find(|(found, _, _)| found == key)?;
    match value {
        RawValue::One(Scalar::Str(text)) => Some(text.clone()),
        other => {
            errors.push(RuleError::new(
                *line,
                format!("{key} は文字列でなければならない（実 {other:?}）"),
            ));
            None
        }
    }
}

/// bool field を取り出す。型違いは error にする。
fn bool_field(raw: &RawRow, key: &str, errors: &mut Vec<RuleError>) -> Option<bool> {
    let (_, value, line) = raw.fields.iter().find(|(found, _, _)| found == key)?;
    match value {
        RawValue::One(Scalar::Bool(found)) => Some(*found),
        other => {
            errors.push(RuleError::new(
                *line,
                format!("{key} は bool でなければならない（実 {other:?}）"),
            ));
            None
        }
    }
}

/// `kind` を種類へ引く。未知の字面は error にする。
fn kind_field(raw: &RawRow, id: &str, errors: &mut Vec<RuleError>) -> Option<RuleKind> {
    let text = text_field(raw, "kind", errors)?;
    match RuleKind::parse(&text) {
        Some(kind) => Some(kind),
        None => {
            errors.push(RuleError::new(
                raw.line,
                format!("{id} の kind {text} は未知である"),
            ));
            None
        }
    }
}

/// `value` を種類に応じた値へ写す。bool は value に置けない。
fn value_field(
    raw: &RawRow,
    kind: RuleKind,
    id: &str,
    errors: &mut Vec<RuleError>,
) -> Option<RuleValue> {
    let (_, scalar, line) = raw.fields.iter().find(|(key, _, _)| key == "value")?;
    match scalar {
        // **形の照合はここでしない**（`RuleRow::validate` の 1 箇所が持つ）。ここで
        // 弾くと、kind と value の対応を測る面が 2 つになる。
        RawValue::List(items) => Some(RuleValue::List(items.clone())),
        RawValue::One(Scalar::Int(found)) => Some(RuleValue::Int(*found)),
        RawValue::One(Scalar::Str(text)) => Some(match kind.shape() {
            ValueShape::Policy => RuleValue::Policy(text.clone()),
            ValueShape::Int | ValueShape::Str | ValueShape::List => RuleValue::Str(text.clone()),
        }),
        RawValue::One(Scalar::Bool(_)) => {
            errors.push(RuleError::new(
                *line,
                format!("{id} の value は integer か string でなければならない"),
            ));
            None
        }
        // 網羅のための枝。**到達しない**——読めなかった値を持つ行は
        // [`build_row`] が先に打ち切る（Broken を見る場所は 1 か所である）。
        RawValue::Broken => None,
    }
}

/// 行 id の重複を集める。
fn check_duplicate_ids(rows: &[RuleRow], errors: &mut Vec<RuleError>) {
    for (index, row) in rows.iter().enumerate() {
        let seen = rows
            .iter()
            .take(index)
            .any(|earlier| earlier.id == row.id);
        if seen {
            errors.push(RuleError::new(row.line, format!("id {} が重複する", row.id)));
        }
    }
}
