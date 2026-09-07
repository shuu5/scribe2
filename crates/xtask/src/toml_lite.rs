//! `Cargo.toml` / `rust-toolchain.toml` を std だけで読むための最小 scanner。
//!
//! file 全体を行 grep してはならない。`panic` は clippy の lint 名であると同時に
//! `[profile.*]` の正規キーでもあり、骨格の root manifest は同じ file に
//! `[profile.dev]` を持つ。ゆえに「いま自分がどの section に居るか」を追える
//! だけの section 対応が要る。

/// 行頭 `[...]` から section 名を取り出す。section 行でなければ `None`。
pub fn section_header(line: &str) -> Option<&str> {
    let rest = line.trim_end().strip_prefix('[')?;
    let (name, _) = rest.split_once(']')?;
    Some(name.trim())
}

/// `key = value` 行を分解する。空行と `#` 始まりのコメント行は `None`。
pub fn key_value(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let (key, value) = trimmed.split_once('=')?;
    Some((key.trim(), value.trim()))
}

/// `"..."` で囲まれた最初の文字列の中身を返す。
pub fn quoted(text: &str) -> Option<String> {
    let rest = text.trim().strip_prefix('"')?;
    rest.split_once('"').map(|(value, _)| value.to_owned())
}

/// lint の level を取り出す。文字列形 `"deny"` と inline table 形
/// `{ level = "deny", priority = -1 }` の両方を受理する。
pub fn lint_level(value: &str) -> Option<String> {
    if value.starts_with('"') {
        return quoted(value);
    }
    let inner = value.trim().strip_prefix('{')?.strip_suffix('}')?;
    inner.split(',').find_map(|part| {
        let (key, level) = part.split_once('=')?;
        if key.trim() != "level" {
            return None;
        }
        quoted(level)
    })
}

/// `["a", "b"]` 形の文字列配列を分解する（要素は同一行に在る前提でよい）。
pub fn string_array(value: &str) -> Vec<String> {
    let inner = value.trim().trim_start_matches('[').trim_end_matches(']');
    inner.split(',').filter_map(quoted).collect()
}

/// `section` の内側に在る `key = value` を順に返す。
pub fn entries_in<'a>(text: &'a str, section: &str) -> Vec<(&'a str, &'a str)> {
    let mut current = "";
    let mut found = Vec::new();
    for line in text.lines() {
        if let Some(header) = section_header(line) {
            current = header;
            continue;
        }
        if current != section {
            continue;
        }
        if let Some(pair) = key_value(line) {
            found.push(pair);
        }
    }
    found
}
