//! diff を読んで item の列にする群（設計 docs/design/pipeline.md §44・`s2-07l.499`）。
//!
//! 責務は 3 段で、**(1) diff の字面を読む → (2) 宣言を同定する → (3) item の列に切る**である。親
//! （`pipe/move_proof.rs`）の後段（突き合わせと要約）はここの [`Located`] と [`Item`] を受け取るだけで、
//! ここは後段の item を 1 つも呼ばない（呼び手の向きは片道・§44「現物」）。`pipe/move_proof.rs` からの
//! **純移動**で、歯は 1 本も足していない（親の `mod tests` に残る 19 本が従来どおり測る）。
//!
//! 可視性: 親（後段と歯）が呼ぶ item だけが `pub(super)` で、残りはこの module に閉じる。逆向き
//! （子 → 親）は `super::` でそのまま見える（Rust の可視性＝子孫は祖先の私有を見る）ので、**親側の
//! 可視性は 1 語も上げていない**。

// flip-check: moved s2-07l.499

use super::NotPure;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

// ───────── diff の読み ─────────

/// diff に現れる file 1 本（`-` 行は base の行番号・`+` 行は HEAD の行番号を持つ）。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct FileDiff {
    /// base 側の path（新規 file は `None`）。
    pub(super) base: Option<String>,
    /// HEAD 側の path（削除 file は `None`）。
    pub(super) head: Option<String>,
    /// `-` 行（base の行番号・本文）。
    pub(super) removed: Vec<(usize, String)>,
    /// `+` 行（HEAD の行番号・本文）。
    pub(super) added: Vec<(usize, String)>,
}

/// hunk の外で読める見出し（これ以外の行は読めない形＝純移動でない側へ）。
const HEADERS: &[&str] = &["index ", "new file mode ", "deleted file mode ", "similarity index ", "rename from ", "rename to "];

/// `git diff` の本文を file ごとに読む。読めない見出し（binary・mode の変更・未知の形）は `Unreadable`。
pub(super) fn parse_diff(diff: &str) -> Result<Vec<FileDiff>, NotPure> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut cursor: Option<(usize, usize)> = None;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let (base, head) = split_git_paths(rest).ok_or(NotPure::Unreadable)?;
            files.push(FileDiff { base: Some(base), head: Some(head), ..FileDiff::default() });
            cursor = None;
            continue;
        }
        let file = files.last_mut().ok_or(NotPure::Unreadable)?;
        if let Some(rest) = line.strip_prefix("@@ ") {
            cursor = Some(hunk_start(rest).ok_or(NotPure::Unreadable)?);
            continue;
        }
        match cursor.as_mut() {
            Some(at) => hunk_line(file, at, line),
            None => header_line(file, line)?,
        }
    }
    Ok(files)
}

/// `diff --git a/X b/Y` の 2 つの path（`a/` / `b/` の接頭辞を落とす・空白入りは `Y` を末尾から取る）。
pub(super) fn split_git_paths(rest: &str) -> Option<(String, String)> {
    let (base, head) = rest.split_once(" b/")?;
    let base = base.strip_prefix("a/").unwrap_or(base);
    (!base.is_empty() && !head.is_empty()).then(|| (base.to_owned(), head.to_owned()))
}

/// hunk 見出し `-l[,n] +l[,n] @@ …` の 2 つの開始行番号。
fn hunk_start(rest: &str) -> Option<(usize, usize)> {
    let (range, _) = rest.split_once(" @@")?;
    let (old, new) = range.split_once(' ')?;
    let start = |token: &str, sign: char| -> Option<usize> {
        let body = token.strip_prefix(sign)?;
        body.split(',').next()?.parse().ok()
    };
    Some((start(old, '-')?, start(new, '+')?))
}

/// hunk の中の 1 行（`-` / `+` / context・`\ No newline` は無視）。
fn hunk_line(file: &mut FileDiff, at: &mut (usize, usize), line: &str) {
    if let Some(text) = line.strip_prefix('-') {
        file.removed.push((at.0, text.to_owned()));
        at.0 = at.0.saturating_add(1);
    } else if let Some(text) = line.strip_prefix('+') {
        file.added.push((at.1, text.to_owned()));
        at.1 = at.1.saturating_add(1);
    } else if !line.starts_with('\\') {
        at.0 = at.0.saturating_add(1);
        at.1 = at.1.saturating_add(1);
    }
}

/// hunk の外の 1 行（file の見出し）。`---` / `+++` の `/dev/null` が新規 / 削除を名乗る。
fn header_line(file: &mut FileDiff, line: &str) -> Result<(), NotPure> {
    if let Some(path) = line.strip_prefix("--- ") {
        file.base = (path != "/dev/null").then(|| path.strip_prefix("a/").unwrap_or(path).to_owned());
        return Ok(());
    }
    if let Some(path) = line.strip_prefix("+++ ") {
        file.head = (path != "/dev/null").then(|| path.strip_prefix("b/").unwrap_or(path).to_owned());
        return Ok(());
    }
    if HEADERS.iter().any(|head| line.starts_with(head)) {
        return Ok(());
    }
    Err(NotPure::Unreadable)
}

// ───────── item の切り出し ─────────

/// item の種類（列 0 の宣言 keyword・閉じた集合）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `fn`。
    Fn,
    /// `struct`。
    Struct,
    /// `enum`。
    Enum,
    /// `impl`。
    Impl,
    /// `trait`。
    Trait,
    /// `const`。
    Const,
    /// `static`。
    Static,
    /// `type`。
    Type,
    /// `mod <name> {`（inline の module だけ・`mod x;` は宣言）。
    Mod,
}

/// [`Kind`] の全 variant。
const KINDS: &[Kind] = &[
    Kind::Fn,
    Kind::Struct,
    Kind::Enum,
    Kind::Impl,
    Kind::Trait,
    Kind::Const,
    Kind::Static,
    Kind::Type,
    Kind::Mod,
];

impl Kind {
    /// 宣言の keyword。
    fn keyword(self) -> &'static str {
        match self {
            Self::Fn => "fn",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Impl => "impl",
            Self::Trait => "trait",
            Self::Const => "const",
            Self::Static => "static",
            Self::Type => "type",
            Self::Mod => "mod",
        }
    }

    /// 本文が必ず `{ … }` で閉じる種類（`;` で終わる形を持たない）。
    fn braced(self) -> bool {
        matches!(self, Self::Fn | Self::Impl | Self::Trait | Self::Mod)
    }
}

/// 列 0 の宣言行の読み。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Declaration {
    /// 種類。
    kind: Kind,
    /// 名（`fn foo` / `impl Foo for Bar` の形）。
    pub(super) name: String,
    /// 可視性の prefix（無ければ空）。
    visibility: String,
    /// 可視性を剥がした宣言行。
    stripped: String,
}

/// 可視性の prefix を剥がす（`pub` / `pub(crate)` / `pub(super)` / `pub(in …)`）。
pub(super) fn strip_visibility(line: &str) -> (String, &str) {
    let Some(rest) = line.strip_prefix("pub") else {
        return (String::new(), line);
    };
    if let Some(after) = rest.strip_prefix(' ') {
        return ("pub".to_owned(), after.trim_start());
    }
    let Some(inner) = rest.strip_prefix('(') else {
        return (String::new(), line);
    };
    match inner.split_once(')') {
        Some((scope, after)) if after.starts_with(' ') && is_scope(scope) => {
            (format!("pub({scope})"), after.trim_start())
        }
        _ => (String::new(), line),
    }
}

/// `pub(…)` の中身として読める形（`crate` / `super` / `self` / `in <path>`）。
fn is_scope(scope: &str) -> bool {
    matches!(scope, "crate" | "super" | "self")
        || scope.strip_prefix("in ").is_some_and(|path| !path.is_empty() && !path.contains(['(', ' ']))
}

/// 宣言の keyword の前に来てよい修飾（`const fn` の `const` は keyword の判定で扱う）。
const QUALIFIERS: &[&str] = &["unsafe ", "async ", "default ", "extern \"C\" ", "extern \"system\" "];

/// 列 0 の行を宣言として読む（読めない行は `None`＝item ではない）。
pub(super) fn declaration_of(line: &str) -> Option<Declaration> {
    if line.starts_with(char::is_whitespace) || line.is_empty() {
        return None;
    }
    let (visibility, stripped) = strip_visibility(line);
    let mut body = stripped;
    while let Some(next) = QUALIFIERS.iter().find_map(|word| body.strip_prefix(word)) {
        body = next;
    }
    if let Some(after) = body.strip_prefix("const fn ") {
        return Some(named(Kind::Fn, after, visibility, stripped));
    }
    let kind = KINDS.iter().copied().find(|kind| {
        let after = body.strip_prefix(kind.keyword());
        after.is_some_and(|after| after.starts_with(' ') || (*kind == Kind::Impl && after.starts_with('<')))
    })?;
    let after = body.get(kind.keyword().len()..).unwrap_or_default();
    if kind == Kind::Mod && !after.contains('{') {
        return None;
    }
    Some(named(kind, after, visibility, stripped))
}

/// keyword の後ろから名を切り出して宣言を組む。
fn named(kind: Kind, after: &str, visibility: String, stripped: &str) -> Declaration {
    let name = match kind {
        Kind::Impl => format!("impl {}", after.split('{').next().unwrap_or_default().trim()),
        _ => {
            let ident: String = after
                .trim_start()
                .chars()
                .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                .collect();
            format!("{} {ident}", kind.keyword())
        }
    };
    Declaration { kind, name, visibility, stripped: stripped.to_owned() }
}

/// file の中の item 1 本（列 0 から始まる宣言単位・設計 §5.3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Item {
    /// 名（`fn foo` / `impl Foo for Bar` の形）。
    pub(super) name: String,
    /// 可視性の prefix（無ければ空）。
    pub(super) visibility: String,
    /// 正規化した本文の hash（可視性を剥がし・行頭の indent を落とし・コメント行を除き・末尾の空行を除く）。
    pub(super) hash: u64,
    /// 行の区間（1 始まり・両端含む）。
    pub(super) lines: (usize, usize),
    /// hash から除いたコメント行（indent を落とした字面・区間の順・要約の「コメント行の差」の元）。
    pub(super) comments: Vec<String>,
}

/// file に置かれた item。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Located {
    /// repo 相対 path。
    pub(super) file: String,
    /// item。
    pub(super) item: Item,
}

/// item の前に連なる行か（属性・doc コメント・列 0）。
fn is_prefix(line: &str) -> bool {
    line.starts_with("#[") || line.starts_with("///")
}

/// 属性の続きの行か（複数行の `#[expect(\n …\n)]`・prefix が在る周だけ見る）。
fn is_continuation(line: &str) -> bool {
    line.starts_with(char::is_whitespace) || line.starts_with(')') || line.starts_with(']')
}

/// 列 0 で次の item（の prefix か宣言）が始まる行か。
fn starts_item(line: &str) -> bool {
    is_prefix(line) || declaration_of(line).is_some()
}

/// 本文の item を全部切り出す（列 0 の宣言単位・入れ子は外側に畳む）。
pub(super) fn items_of(text: &str) -> Vec<Item> {
    let lines: Vec<&str> = text.lines().collect();
    let mut items = Vec::new();
    let mut prefix: Option<usize> = None;
    let mut index = 0;
    while let Some(line) = lines.get(index).copied() {
        if is_prefix(line) {
            if prefix.is_none() {
                prefix = Some(index);
            }
        } else if prefix.is_some() && is_continuation(line) {
            // 属性の続き（`)]` まで）。
        } else if let Some(decl) = declaration_of(line) {
            let start = prefix.take().unwrap_or(index);
            let end = item_end(&lines, index, &decl);
            items.push(build_item(&lines, (start, end), index, &decl));
            index = end.saturating_add(1);
            continue;
        } else {
            prefix = None;
        }
        index = index.saturating_add(1);
    }
    items
}

/// item の終端の index（両端含む・末尾の空行は含めない）。
///
/// 宣言行が `{` を持てば列 0 の `}` の行まで（同じ行で釣り合えばその行）、持たなければ `;` で終わる行まで
/// （`fn` / `impl` / `trait` / `mod` は署名が複数行でも列 0 の `}` まで＝列 0 の `)` は終端でない）。
/// 次の item が先に始まればその直前で切る。
fn item_end(lines: &[&str], decl_index: usize, decl: &Declaration) -> usize {
    let opens = decl.stripped.matches('{').count();
    if opens > 0 && opens == decl.stripped.matches('}').count() {
        return decl_index;
    }
    let by_brace = opens > 0 || decl.kind.braced();
    if !by_brace && decl.stripped.trim_end().ends_with(';') {
        return decl_index;
    }
    let mut end = decl_index;
    for (offset, line) in lines.iter().enumerate().skip(decl_index.saturating_add(1)) {
        if starts_item(line) {
            break;
        }
        end = offset;
        let closes = if by_brace { line.starts_with('}') } else { line.trim_end().ends_with(';') };
        if closes {
            return offset;
        }
    }
    while end > decl_index && lines.get(end).is_some_and(|line| line.trim().is_empty()) {
        end = end.saturating_sub(1);
    }
    end
}

/// 区間から item を組む（本文の正規化 = 宣言行の可視性の剥がし + 行頭の indent の除去 + コメント行の除外）。
///
/// コメント行（`//` / `///` / `//!`・indent の後）は挙動を持たないので hash に入れず [`Item::comments`] へ写す
/// （module を跨ぐ移動で常に要る doc の intra-doc link の path 書き換えを本文差と読まない・`s2-07l.294`）。
/// 札（`// flip-check:`）の行もここでは hash に入れず、[`super::residual_lines`] が item の中の札を残差と同じ規則で
/// 検査する（札を先に hash へ入れると `ItemsDiffer` が `ForeignMarker` を隠す）。文字列 literal の行（`"// …"`）は
/// `"` で始まるので本文。
fn build_item(lines: &[&str], span: (usize, usize), decl_index: usize, decl: &Declaration) -> Item {
    let mut hasher = DefaultHasher::new();
    let mut comments = Vec::new();
    for (offset, line) in lines.iter().enumerate().take(span.1.saturating_add(1)).skip(span.0) {
        let shown = if offset == decl_index { decl.stripped.as_str() } else { line.trim_start() };
        if offset != decl_index && shown.starts_with("//") {
            comments.push(shown.to_owned());
            continue;
        }
        shown.hash(&mut hasher);
        '\n'.hash(&mut hasher);
    }
    Item {
        name: decl.name.clone(),
        visibility: decl.visibility.clone(),
        hash: hasher.finish(),
        lines: (span.0.saturating_add(1), span.1.saturating_add(1)),
        comments,
    }
}
