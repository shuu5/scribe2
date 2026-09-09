//! flat な JSON object だけを std で読み書きする面（設計 §3）。
//!
//! 扱うのは **1 段の object**（値は string / u64 / bool / null）だけである。入れ子・
//! 配列・浮動小数は受理しない（受理すると schema の形が緩み、読み側が黙って別物を
//! 通してしまう）。`"` `\` と制御文字は escape する。

/// flat object が持てる値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// 文字列。
    Str(String),
    /// 非負整数。
    Num(u64),
    /// 真偽。
    Bool(bool),
    /// 値なし。
    Null,
}

impl Value {
    /// 文字列なら中身を借りる。
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text),
            _ => None,
        }
    }

    /// 整数なら値を返す。
    pub fn as_num(&self) -> Option<u64> {
        match *self {
            Self::Num(found) => Some(found),
            _ => None,
        }
    }
}

/// key と値の並びを 1 行の JSON object に書く。
pub fn write_object(pairs: &[(&str, Value)]) -> String {
    let body: Vec<String> = pairs
        .iter()
        .map(|(key, value)| format!("{}:{}", quote(key), render(value)))
        .collect();
    format!("{{{}}}", body.join(","))
}

/// 値 1 つを書く。
fn render(value: &Value) -> String {
    match value {
        Value::Str(text) => quote(text),
        Value::Num(found) => found.to_string(),
        Value::Bool(found) => found.to_string(),
        Value::Null => "null".to_owned(),
    }
}

/// 文字列を escape して `"` で囲む。
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", other as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// 1 行の flat object を読む。入れ子・配列・余分な後続は error。
pub fn parse_object(text: &str) -> Result<Vec<(String, Value)>, String> {
    let mut chars = text.trim().chars().peekable();
    take(&mut chars, '{')?;
    let mut pairs = Vec::new();
    skip_ws(&mut chars);
    if chars.peek() == Some(&'}') {
        chars.next();
        return tail_is_empty(&mut chars).map(|()| pairs);
    }
    loop {
        skip_ws(&mut chars);
        let key = parse_string(&mut chars)?;
        skip_ws(&mut chars);
        take(&mut chars, ':')?;
        skip_ws(&mut chars);
        if pairs.iter().any(|(found, _)| *found == key) {
            // 先勝ちで後の値が黙って消えるのを塞ぐ（重複は形の壊れである）。
            return Err(format!("key {key} が重複する"));
        }
        pairs.push((key, parse_value(&mut chars)?));
        skip_ws(&mut chars);
        match chars.next() {
            Some(',') => {}
            Some('}') => break,
            other => return Err(format!("',' か '}}' が要る（{other:?}）")),
        }
    }
    tail_is_empty(&mut chars).map(|()| pairs)
}

/// 読み進める型の別名（引数の型を短く保つ）。
type Cursor<'a> = std::iter::Peekable<std::str::Chars<'a>>;

/// object の後ろに余分が無いことを確かめる。
fn tail_is_empty(chars: &mut Cursor<'_>) -> Result<(), String> {
    skip_ws(chars);
    match chars.next() {
        None => Ok(()),
        Some(found) => Err(format!("object の後ろに余分がある（{found:?}）")),
    }
}

/// 空白を読み飛ばす。
fn skip_ws(chars: &mut Cursor<'_>) {
    while chars.peek().is_some_and(|ch| ch.is_ascii_whitespace()) {
        chars.next();
    }
}

/// 1 文字を確かめて読み進める。
fn take(chars: &mut Cursor<'_>, want: char) -> Result<(), String> {
    match chars.next() {
        Some(found) if found == want => Ok(()),
        other => Err(format!("{want:?} が要る（{other:?}）")),
    }
}

/// 値 1 つを読む。
fn parse_value(chars: &mut Cursor<'_>) -> Result<Value, String> {
    match chars.peek() {
        Some('"') => parse_string(chars).map(Value::Str),
        Some('t') | Some('f') => parse_bool(chars),
        Some('n') => parse_null(chars),
        Some(found) if found.is_ascii_digit() => parse_num(chars),
        other => Err(format!("値の形でない（{other:?}）")),
    }
}

/// `true` / `false` を読む。
fn parse_bool(chars: &mut Cursor<'_>) -> Result<Value, String> {
    let word = word_of(chars);
    match word.as_str() {
        "true" => Ok(Value::Bool(true)),
        "false" => Ok(Value::Bool(false)),
        other => Err(format!("bool でない（{other:?}）")),
    }
}

/// `null` を読む。
fn parse_null(chars: &mut Cursor<'_>) -> Result<Value, String> {
    let word = word_of(chars);
    if word == "null" {
        Ok(Value::Null)
    } else {
        Err(format!("null でない（{word:?}）"))
    }
}

/// 非負整数を読む。
fn parse_num(chars: &mut Cursor<'_>) -> Result<Value, String> {
    let mut digits = String::new();
    while chars.peek().is_some_and(char::is_ascii_digit) {
        if let Some(found) = chars.next() {
            digits.push(found);
        }
    }
    digits
        .parse::<u64>()
        .map(Value::Num)
        .map_err(|err| format!("整数でない（{digits:?}・{err}）"))
}

/// 英小文字の連なりを読む（`true` / `false` / `null` の判別に使う）。
fn word_of(chars: &mut Cursor<'_>) -> String {
    let mut word = String::new();
    while chars.peek().is_some_and(char::is_ascii_alphabetic) {
        if let Some(found) = chars.next() {
            word.push(found);
        }
    }
    word
}

/// escape を解いて文字列を読む。
fn parse_string(chars: &mut Cursor<'_>) -> Result<String, String> {
    take(chars, '"')?;
    let mut out = String::new();
    loop {
        match chars.next() {
            None => return Err("文字列が閉じていない".to_owned()),
            Some('"') => return Ok(out),
            Some('\\') => out.push(parse_escape(chars)?),
            Some(found) if (found as u32) < 0x20 => {
                return Err(format!("生の制御文字が在る（{:#06x}）", found as u32));
            }
            Some(found) => out.push(found),
        }
    }
}

/// `\` の次の 1 文字（または `\uXXXX`）を解く。
fn parse_escape(chars: &mut Cursor<'_>) -> Result<char, String> {
    match chars.next() {
        Some('"') => Ok('"'),
        Some('\\') => Ok('\\'),
        Some('/') => Ok('/'),
        Some('n') => Ok('\n'),
        Some('r') => Ok('\r'),
        Some('t') => Ok('\t'),
        Some('b') => Ok('\u{0008}'),
        Some('f') => Ok('\u{000c}'),
        Some('u') => parse_unicode(chars),
        other => Err(format!("未知の escape（{other:?}）")),
    }
}

/// `\uXXXX` の 4 桁を解く。
fn parse_unicode(chars: &mut Cursor<'_>) -> Result<char, String> {
    let mut hex = String::new();
    for _ in 0..4 {
        match chars.next() {
            Some(found) => hex.push(found),
            None => return Err("\\u の 4 桁が足りない".to_owned()),
        }
    }
    let code = u32::from_str_radix(&hex, 16).map_err(|err| format!("16 進でない（{hex:?}・{err}）"))?;
    char::from_u32(code).ok_or_else(|| format!("符号位置が文字にならない（{hex:?}）"))
}
