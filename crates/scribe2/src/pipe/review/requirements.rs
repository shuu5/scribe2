//! 要件本文の読み手の群（設計 docs/design/contract-source.md §37・契約表の行 al・`s2-07l.541`）。
//!
//! 要件面（`.html` / `.yaml` / `.md`）から `req` の各 id の本文を読み、審査の材料の `{requirements}` の本文に組む
//! 群である（[`requirements_text`]）。`pipe/review.rs` からの**純移動**で、歯は 1 本も足していない（親に残る
//! in-file の歯が従来どおり測る）。外の呼び手は 0 で、親の材料の組み立てからだけ入る。親の私有 item は
//! `super::` でそのまま見えるので、**親側の可視性は 1 語も上げていない**。親の本体と歯が名指す 6 名だけが
//! `pub(super)` で、残りはこの module に閉じる。

// flip-check: moved s2-07l.541

use super::table;
use std::path::Path;

/// 要件面から 1 つの id を読んだ結果（形ごとの読み手が返す・材料の行の 3 形）。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Found {
    /// id の本文（空白を畳んだ 1 行）。
    Body(String),
    /// id は在るが本文が無い（yaml の裸の `- FR1` / `text:` の無い mapping・md の見出しの下が空）。
    Empty,
    /// id が要件面に無い。
    Absent,
}

/// `req` の各 id の要件本文を要件面から抜く。読み手は要件面の**形ごとに 1 関数**で、呼び分けは id の集合の読み手
/// [`table::requirement_ids`] と同じ**拡張子の 1 match**（`.html` = [`requirement_row`]〔`id="<id>"` の行・tag を剥がした
/// 字面〕・`.yaml` / `.yml` = [`requirement_yaml`]〔id と同じ mapping の `text:` → 無ければ `shall:`〕・`.md` =
/// [`requirement_md`]〔見出しの
/// 下の本文〕・設計 contract-source.md §4・C2）。要件面を読めない周・形を読めない周は理由の 1 行・id が無い周と本文の
/// 無い id はその id の行に明示する（黙って落とさない・NFR4）。
pub(super) fn requirements_text(repo: &Path, path: &str, req: &[String]) -> String {
    let text = match table::read(repo, path) {
        Ok(found) => found,
        Err(reason) => return format!("（要件面を読めない: {reason}）"),
    };
    let reader: fn(&str, &str) -> Found = match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("html") => |text, id| requirement_row(text, id).map_or(Found::Absent, Found::Body),
        Some("yaml" | "yml") => requirement_yaml,
        Some("md") => requirement_md,
        _ => return format!("（要件面 {path} の形を読めない: .html の anchor / .yaml の id + text / .md の見出しだけ）"),
    };
    req.iter()
        .map(|id| match reader(&text, id) {
            Found::Body(body) => format!("{id}: {body}"),
            Found::Empty => format!("{id}: （要件面 {path} の {id} に本文が無い）"),
            Found::Absent => format!("{id}: （要件面 {path} に無い）"),
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// 要件面の中で `id="<id>"` / `id='<id>'` を持つ最初の行の本文（tag を剥がし空白を畳む）。無ければ `None`。
pub(super) fn requirement_row(text: &str, id: &str) -> Option<String> {
    let marks = [format!("id=\"{id}\""), format!("id='{id}'")];
    text.lines()
        .find(|line| marks.iter().any(|mark| line.contains(mark.as_str())))
        .map(strip_tags)
}

/// md の要件面（行頭 `#` の見出しの先頭 token が id・`table/check.rs` の `md_id` と同じ見出しの読み）の本文: その
/// 見出しの次の行から次の見出しの直前までを空白を畳んだ 1 行に。本文の行が無い（空行だけ）id は [`Found::Empty`]。
pub(super) fn requirement_md(text: &str, id: &str) -> Found {
    let mut lines = text.lines();
    if lines.by_ref().all(|line| md_heading(line).and_then(|title| title.split_whitespace().next()) != Some(id)) {
        return Found::Absent;
    }
    let body: Vec<&str> = lines.take_while(|line| md_heading(line).is_none()).flat_map(str::split_whitespace).collect();
    if body.is_empty() {
        Found::Empty
    } else {
        Found::Body(body.join(" "))
    }
}

/// 行頭 `#` の列 + 空白で始まる md の見出しの字面（それ以外は `None`）。
fn md_heading(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('#')?.trim_start_matches('#');
    rest.strip_prefix([' ', '\t']).map(str::trim)
}

/// yaml の要件面の本文: id の行（`- id: FR1` / `id: FR1`）と同じ mapping の **`text:` の値**、無ければ **`shall:` の値**
/// （EARS 形の要件面・設計 contract-source.md §32）。`shall:` を採る周に同じ mapping の `when:` が在れば、本文は
/// 「`when:` の値 + [`BODY_JOIN`] + `shall:` の値」の 1 本にする（条件を落とすと約束の範囲が変わる）。`title:` と
/// `plain:` は読まない。値の続き（`text: |` / `shall: |` の block・桁の深い行）は欄ごとに空白を畳んで繋ぐ。裸の
/// `- FR1` と、`text:` も `shall:` も無い mapping（`when:` だけの mapping を含む）は [`Found::Empty`]。
pub(super) fn requirement_yaml(text: &str, id: &str) -> Found {
    let lines: Vec<&str> = text.lines().collect();
    let hit = lines.iter().enumerate().find_map(|(index, line)| {
        let (column, key, value) = yaml_entry(line)?;
        (value == id && (key.is_empty() || key == "id")).then_some((index, column, key.is_empty()))
    });
    let Some((at, column, bare)) = hit else {
        return Found::Absent;
    };
    if bare {
        return Found::Empty;
    }
    // mapping の始まり: `- id:` の行はそれ自身・`id:` の行は同じ桁の key を上へ辿り、`-` の頭で止まる。
    let member = |index: usize| lines.get(index).map_or(Member::Outside, |line| yaml_member(line, column));
    let mut start = at;
    while member(start) != Member::Head && start > 0 && member(start.saturating_sub(1)) != Member::Outside {
        start = start.saturating_sub(1);
    }
    // 欄は [`BODY_KEYS`] の 3 つだけを集める（他の欄＝`title:` / `plain:` の周は `reading` が `None` で値も続きも捨てる）。
    let (mut parts, mut reading) = ([Vec::new(), Vec::new(), Vec::new()], None);
    for (index, line) in lines.iter().enumerate().skip(start) {
        match yaml_member(line, column) {
            Member::Outside => break,
            Member::Head if index != start => break,
            Member::Head | Member::Key => {
                let (_, key, value) = yaml_entry(line).unwrap_or_default();
                reading = BODY_KEYS.iter().position(|found| *found == key);
                if let Some(part) = reading.and_then(|at| parts.get_mut(at)) {
                    if !matches!(value, "|" | ">" | "|-" | ">-" | "|+" | ">+") {
                        part.push(value);
                    }
                }
            }
            Member::Inner => {
                if let Some(part) = reading.and_then(|at| parts.get_mut(at)) {
                    part.push(unquote(line));
                }
            }
        }
    }
    let [text, when, shall] = parts.map(|part| part.join(" ").split_whitespace().collect::<Vec<&str>>().join(" "));
    // `text:` が在ればそれだけ（`shall:` も `when:` も混ぜない）・無ければ `shall:` に `when:` を前置して 1 本に。
    let joined = match (text.as_str(), when.as_str(), shall.as_str()) {
        ("", _, "") => String::new(),
        ("", "", found) => found.to_owned(),
        ("", condition, found) => format!("{condition}{BODY_JOIN}{found}"),
        (found, _, _) => found.to_owned(),
    };
    if joined.is_empty() {
        Found::Empty
    } else {
        Found::Body(joined)
    }
}

/// [`requirement_yaml`] が読む本文の欄（優先順ではなく**組み立ての並び**: `text:` / `when:` / `shall:`）。欄の名と順序は
/// この 1 か所だけが持つ（`.vessel.toml` にも rules 行にも宣言を足さない・設計 contract-source.md §32・C17）。
const BODY_KEYS: [&str; 3] = ["text", "when", "shall"];

/// [`requirement_yaml`] が `when:` と `shall:` の値を繋ぐ区切り（本文は空白を畳む形なので、空白だけでは境が消える）。
const BODY_JOIN: &str = " — ";

/// yaml の 1 行の (key の桁, key, 値)。`- id: FR1` は key の桁を `-` と空白の後ろに取り、`- FR1` の裸の項目は key が空。
/// 空行と `#` の comment は `None`。値は両端の引用符を剥がす。
fn yaml_entry(line: &str) -> Option<(usize, &str, &str)> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let mut column = line.len().saturating_sub(trimmed.len());
    let item = match trimmed.strip_prefix('-') {
        Some(rest) if rest.is_empty() || rest.starts_with([' ', '\t']) => {
            let body = rest.trim_start();
            column = column.saturating_add(trimmed.len().saturating_sub(body.len()));
            body
        }
        _ => trimmed,
    };
    match item.split_once(':') {
        Some((key, value)) if !key.contains([' ', '"', '\'']) && (value.is_empty() || value.starts_with([' ', '\t'])) => {
            Some((column, key, unquote(value)))
        }
        _ => Some((column, "", unquote(item))),
    }
}

/// 両端の空白と引用符（`"` / `'`）を剥がした字面。
fn unquote(value: &str) -> &str {
    value.trim().trim_matches(|found: char| found == '"' || found == '\'')
}

/// yaml の 1 行が、桁 `column` に key を持つ mapping に対してどこに在るか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Member {
    /// `-` の頭を持つ同じ桁の key（mapping の始まり・次の項目の始まり）。
    Head,
    /// 同じ桁の key（同じ mapping の欄）。
    Key,
    /// 桁の深い行・空行・comment（値の続き）。
    Inner,
    /// 桁の浅い行（mapping の外）。
    Outside,
}

/// [`Member`] の判定（`yaml_entry` の桁と行の字下げから）。
fn yaml_member(line: &str, column: usize) -> Member {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Member::Inner;
    }
    let indent = line.len().saturating_sub(trimmed.len());
    match yaml_entry(line) {
        Some((at, _, _)) if at == column && indent < at => Member::Head,
        Some((at, _, _)) if at == column => Member::Key,
        _ if indent > column => Member::Inner,
        _ => Member::Outside,
    }
}

/// HTML の tag（`<…>`）を剥がし、連続する空白を 1 つに畳む。
pub(super) fn strip_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut inside = false;
    for found in line.chars() {
        match found {
            '<' => inside = true,
            '>' if inside => {
                inside = false;
                out.push(' ');
            }
            _ if inside => {}
            _ => out.push(found),
        }
    }
    out.split_whitespace().collect::<Vec<&str>>().join(" ")
}
