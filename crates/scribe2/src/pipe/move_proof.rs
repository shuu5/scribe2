//! 純移動の機械証明（設計 docs/design/pipeline.md §5.3・`s2-07l.266`・FR9 / NFR1 / FR5）。
//!
//! 便の diff が**純移動**——(名, 本文の hash) の多重集合が base と HEAD で一致し、移動した item が
//! 1 つ以上在り、残差分が宣言と札とコメントと空行だけ——なら、lens へ diff の代わりに**要約**
//! （[`MoveSummary`]）を渡す。分割便の diff は本文を 2 度（`-` と `+`）運ぶので予算の cap に当たるが、
//! 移動の事実は要約の方が小さく正確に運べる（cap を一時的に上げた `s2-07l.265` の恒久解）。
//!
//! **判定は純関数**である。file の読み（git）は gate 側が閉じた口 [`Side`] の closure で担い、
//! ここは I/O を持たない（`lens-input.txt` の書き出しの helper [`keep`] だけが fs に触る）。
//!
//! **下界**（C13・構文木を持たない・字面走査）: 列 0 から始まる宣言単位だけを item と数え、macro が
//! 生む item・1 行に複数の item・入れ子の item（`impl {}` の中の fn・inline `mod tests {}` の歯）は外側の
//! item 1 本に畳む。読めない形は**純移動でない側**へ倒す——誤判定は lens から diff を奪う側
//! （FailOpen・PostHoc・[`POLARITY`]）なので、迷った周は従来どおり diff を渡す。

use crate::polarity::{OnFailure, Polarity, Timing};
use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// 純移動の証明の極性（[`LensInput`]）: 実装の後に測り、誤判定は lens から diff を奪う側（通す側）へ倒れる。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::PostHoc,
    on_failure: OnFailure::FailOpen,
};

/// run dir に残す lens の入力の写し（純移動の周だけ・事後に読める・NFR4）。
pub const LENS_INPUT_FILE: &str = "lens-input.txt";

/// 要約の先頭行（雛形 `lens.txt` の `{diff}` の穴に入る本文が diff でないことを名乗る）。
const HEADLINE: &str = "これは diff ではなく純移動の要約である";

/// 純移動の札（残差分に許す `// flip-check:` の形は **moved** だけ・lens v2 medium）。
const MOVED_MARK: &str = "// flip-check: moved ";

/// `// flip-check:` で始まる札の共通の頭。
const FLIP_MARK: &str = "// flip-check:";

/// lens への入力（閉じた型・C3.3「判定入力を自由文にしない」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LensInput {
    /// 従来の diff（純移動でない周・理由を伴う）。
    Diff(NotPure),
    /// 純移動の要約（本文は雛形の `{diff}` の穴へそのまま入る）。
    Summary(MoveSummary),
}

impl LensInput {
    /// 判定行の `lens-input=` の語。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Diff(_) => "diff",
            Self::Summary(_) => "summary",
        }
    }

    /// lens の stdin へ渡す本文（予算の照合もこの byte で行う・FR9）。
    pub fn body<'a>(&'a self, diff: &'a [u8]) -> &'a [u8] {
        match self {
            Self::Diff(_) => diff,
            Self::Summary(summary) => summary.text.as_bytes(),
        }
    }

    /// 純移動でない周に gate の stderr へ出す 1 行（typed・自由文にしない）。
    pub fn notice(&self) -> Option<String> {
        match self {
            Self::Diff(why) => Some(format!("pipe: lens-input=diff reason={}", why.as_str())),
            Self::Summary(_) => None,
        }
    }
}

/// 純移動でない理由（閉じた enum・(1e)）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotPure {
    /// diff の形か、diff に現れる file の本文を読めない。
    Unreadable,
    /// (名, 本文の hash) の多重集合が base と HEAD で一致しない（追加・削除・本文差）。
    ItemsDiffer,
    /// 移動した item が 0（宣言だけの便）。
    NothingMoved,
    /// 残差分に宣言と札とコメント以外の行が在る。
    ResidualLine,
    /// `// flip-check:` の札のうち `moved` 以外（`retroactive` 等）が残差分に在る。
    ForeignMarker,
}

/// [`NotPure`] の全 variant。
pub const NOT_PURE: &[NotPure] = &[
    NotPure::Unreadable,
    NotPure::ItemsDiffer,
    NotPure::NothingMoved,
    NotPure::ResidualLine,
    NotPure::ForeignMarker,
];

impl NotPure {
    /// stderr の判定行に載せる名（kebab）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unreadable => "unreadable",
            Self::ItemsDiffer => "items-differ",
            Self::NothingMoved => "nothing-moved",
            Self::ResidualLine => "residual-line",
            Self::ForeignMarker => "foreign-marker",
        }
    }
}

/// gate が読む側（closure の第 1 引数・rev の解決は gate が持つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    /// 便の base。
    Base,
    /// 便の HEAD。
    Head,
}

/// 純移動の要約（型は閉じ、本文 `text` は雛形の穴へそのまま入る）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveSummary {
    /// 描いた本文（先頭行は [`HEADLINE`]）。
    text: String,
}

impl MoveSummary {
    /// 本文。
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// 要約の本文を run dir の [`LENS_INPUT_FILE`] へ残す。
pub fn keep(run_dir: &Path, summary: &MoveSummary) -> Result<(), String> {
    let path = run_dir.join(LENS_INPUT_FILE);
    std::fs::write(&path, summary.text()).map_err(|err| format!("{} を書けない: {err}", path.display()))
}

/// diff を判定して lens への入力を決める（**純関数**・file の本文は `read` が返す）。
///
/// `read` は (側, repo 相対 path) → 本文（読めない周は `None`）。diff が新規 file と言う path の base 側・
/// 削除 file の HEAD 側は読まない（空として扱う）。
pub fn judge(diff: &str, read: &dyn Fn(Side, &str) -> Option<String>) -> LensInput {
    match prove(diff, read) {
        Ok(summary) => LensInput::Summary(summary),
        Err(why) => LensInput::Diff(why),
    }
}

/// 判定の本体（4 条件を順に見る・(1d)）。
fn prove(diff: &str, read: &dyn Fn(Side, &str) -> Option<String>) -> Result<MoveSummary, NotPure> {
    let files = parse_diff(diff)?;
    let mut base: Vec<Located> = Vec::new();
    let mut head: Vec<Located> = Vec::new();
    let mut spans: BTreeMap<(Side, String), Vec<(usize, usize)>> = BTreeMap::new();
    for file in &files {
        for (side, path) in [(Side::Base, &file.base), (Side::Head, &file.head)] {
            let Some(path) = path else { continue };
            let text = read(side, path).ok_or(NotPure::Unreadable)?;
            let items = if path.ends_with(".rs") { items_of(&text) } else { Vec::new() };
            let located = items.into_iter().map(|item| Located { file: path.clone(), item });
            match side {
                Side::Base => base.extend(located),
                Side::Head => head.extend(located),
            }
            spans.insert((side, path.clone()), use_spans(&text));
        }
    }
    let matched = pair_items(&base, &head)?;
    if matched.moved == 0 {
        return Err(NotPure::NothingMoved);
    }
    let residual = residual_lines(&files, &base, &head, &spans)?;
    Ok(render(&matched, &residual, base.len()))
}

// ───────── diff の読み ─────────

/// diff に現れる file 1 本（`-` 行は base の行番号・`+` 行は HEAD の行番号を持つ）。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct FileDiff {
    /// base 側の path（新規 file は `None`）。
    base: Option<String>,
    /// HEAD 側の path（削除 file は `None`）。
    head: Option<String>,
    /// `-` 行（base の行番号・本文）。
    removed: Vec<(usize, String)>,
    /// `+` 行（HEAD の行番号・本文）。
    added: Vec<(usize, String)>,
}

/// hunk の外で読める見出し（これ以外の行は読めない形＝純移動でない側へ）。
const HEADERS: &[&str] = &["index ", "new file mode ", "deleted file mode ", "similarity index ", "rename from ", "rename to "];

/// `git diff` の本文を file ごとに読む。読めない見出し（binary・mode の変更・未知の形）は `Unreadable`。
fn parse_diff(diff: &str) -> Result<Vec<FileDiff>, NotPure> {
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
fn split_git_paths(rest: &str) -> Option<(String, String)> {
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
struct Declaration {
    /// 種類。
    kind: Kind,
    /// 名（`fn foo` / `impl Foo for Bar` の形）。
    name: String,
    /// 可視性の prefix（無ければ空）。
    visibility: String,
    /// 可視性を剥がした宣言行。
    stripped: String,
}

/// 可視性の prefix を剥がす（`pub` / `pub(crate)` / `pub(super)` / `pub(in …)`）。
fn strip_visibility(line: &str) -> (String, &str) {
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
fn declaration_of(line: &str) -> Option<Declaration> {
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
struct Item {
    /// 名（`fn foo` / `impl Foo for Bar` の形）。
    name: String,
    /// 可視性の prefix（無ければ空）。
    visibility: String,
    /// 正規化した本文の hash（可視性を剥がし・行頭の indent を落とし・末尾の空行を除く）。
    hash: u64,
    /// 行の区間（1 始まり・両端含む）。
    lines: (usize, usize),
}

/// file に置かれた item。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Located {
    /// repo 相対 path。
    file: String,
    /// item。
    item: Item,
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
fn items_of(text: &str) -> Vec<Item> {
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

/// 区間から item を組む（本文の正規化 = 宣言行の可視性の剥がし + 行頭の indent の除去）。
fn build_item(lines: &[&str], span: (usize, usize), decl_index: usize, decl: &Declaration) -> Item {
    let mut hasher = DefaultHasher::new();
    for (offset, line) in lines.iter().enumerate().take(span.1.saturating_add(1)).skip(span.0) {
        let shown = if offset == decl_index { decl.stripped.as_str() } else { line.trim_start() };
        shown.hash(&mut hasher);
        '\n'.hash(&mut hasher);
    }
    Item {
        name: decl.name.clone(),
        visibility: decl.visibility.clone(),
        hash: hasher.finish(),
        lines: (span.0.saturating_add(1), span.1.saturating_add(1)),
    }
}

/// `use` の宣言のうち複数行に渡る区間（列 0 の `use …{` から `;` で終わる行まで・1 始まり・両端含む）。
fn use_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut open: Option<usize> = None;
    for (index, line) in text.lines().enumerate() {
        let number = index.saturating_add(1);
        match open {
            None if is_use_head(line) && !line.trim_end().ends_with(';') => open = Some(number),
            Some(start) if line.trim_end().ends_with(';') => {
                spans.push((start, number));
                open = None;
            }
            _ => {}
        }
    }
    spans
}

/// `use` の宣言の頭か（`pub` 系の可視性を許す）。
fn is_use_head(line: &str) -> bool {
    strip_visibility(line).1.starts_with("use ")
}

/// `mod x;` の宣言か（inline の `mod x {` は item の側）。
fn is_mod_declaration(line: &str) -> bool {
    let body = strip_visibility(line).1;
    body.starts_with("mod ") && body.trim_end().ends_with(';')
}

// ───────── 判定 ─────────

/// file の間を動いた item の束（元 → 先ごと）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Move {
    /// 移動元。
    from: String,
    /// 移動先。
    to: String,
    /// 動いた item の名。
    names: Vec<String>,
    /// 動いた行数（HEAD 側の区間の合計）。
    lines: usize,
}

/// 可視性が変わった item。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Visibility {
    /// HEAD 側の file。
    file: String,
    /// 名。
    name: String,
    /// 前（base）。
    before: String,
    /// 後（HEAD）。
    after: String,
}

/// 多重集合の照合の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Matched {
    /// 移動（元 → 先の順）。
    moves: Vec<Move>,
    /// 可視性の変化（HEAD の file・行の順）。
    visibility: Vec<Visibility>,
    /// 動いた item の本数。
    moved: usize,
}

/// (名, hash) の多重集合を突き合わせ、同じ file の対を先に取り、残りを移動と数える。
fn pair_items(base: &[Located], head: &[Located]) -> Result<Matched, NotPure> {
    let mut keyed: BTreeMap<(String, u64), (Vec<usize>, Vec<usize>)> = BTreeMap::new();
    for (index, found) in base.iter().enumerate() {
        keyed.entry((found.item.name.clone(), found.item.hash)).or_default().0.push(index);
    }
    for (index, found) in head.iter().enumerate() {
        keyed.entry((found.item.name.clone(), found.item.hash)).or_default().1.push(index);
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (_, (mut olds, mut news)) in keyed {
        if olds.len() != news.len() {
            return Err(NotPure::ItemsDiffer);
        }
        // 同じ file の対を先に取る（動いていない item を移動と数えない）。
        let mut index = 0;
        while let Some(old) = olds.get(index).copied() {
            let same = news.iter().position(|new| base.get(old).map(|found| &found.file) == head.get(*new).map(|found| &found.file));
            match same {
                Some(at) => {
                    pairs.push((old, news.remove(at)));
                    olds.remove(index);
                }
                None => index = index.saturating_add(1),
            }
        }
        pairs.extend(olds.into_iter().zip(news));
    }
    pairs.sort_by_key(|(_, new)| *new);
    Ok(matched_of(&pairs, base, head))
}

/// 対の列から移動と可視性の変化を集める。
fn matched_of(pairs: &[(usize, usize)], base: &[Located], head: &[Located]) -> Matched {
    let mut moves: Vec<Move> = Vec::new();
    let mut visibility = Vec::new();
    let mut moved: usize = 0;
    for (old, new) in pairs {
        let (Some(from), Some(to)) = (base.get(*old), head.get(*new)) else { continue };
        if from.item.visibility != to.item.visibility {
            visibility.push(Visibility {
                file: to.file.clone(),
                name: to.item.name.clone(),
                before: from.item.visibility.clone(),
                after: to.item.visibility.clone(),
            });
        }
        if from.file == to.file {
            continue;
        }
        moved = moved.saturating_add(1);
        let lines = to.item.lines.1.saturating_sub(to.item.lines.0).saturating_add(1);
        match moves.iter_mut().find(|found| found.from == from.file && found.to == to.file) {
            Some(found) => {
                found.names.push(to.item.name.clone());
                found.lines = found.lines.saturating_add(lines);
            }
            None => moves.push(Move { from: from.file.clone(), to: to.file.clone(), names: vec![to.item.name.clone()], lines }),
        }
    }
    Matched { moves, visibility, moved }
}

/// 残差分（どの item の区間にも入らない diff 行）を file ごとに集め、許されない行が在れば理由を返す。
fn residual_lines(
    files: &[FileDiff],
    base: &[Located],
    head: &[Located],
    spans: &BTreeMap<(Side, String), Vec<(usize, usize)>>,
) -> Result<Vec<(String, Vec<String>)>, NotPure> {
    let mut residual = Vec::new();
    for file in files {
        let mut lines = Vec::new();
        let sides = [(Side::Base, &file.base, &file.removed, '-', base), (Side::Head, &file.head, &file.added, '+', head)];
        for (side, path, changed, sign, items) in sides {
            let Some(path) = path else { continue };
            let in_use = spans.get(&(side, path.clone())).map(Vec::as_slice).unwrap_or_default();
            for (number, text) in changed {
                if items.iter().any(|found| found.file == *path && covers(found.item.lines, *number)) {
                    continue;
                }
                residual_allowed(text, in_use.iter().any(|span| covers(*span, *number)))?;
                lines.push(format!("{sign}{text}"));
            }
        }
        if !lines.is_empty() {
            let shown = file.head.clone().or_else(|| file.base.clone()).unwrap_or_default();
            residual.push((shown, lines));
        }
    }
    Ok(residual)
}

/// 区間（両端含む）が行番号を含むか。
fn covers(span: (usize, usize), number: usize) -> bool {
    span.0 <= number && number <= span.1
}

/// 残差分の 1 行が許される形か（宣言 / 札 / item に付かない裸のコメント / 空行）。
fn residual_allowed(text: &str, in_use_span: bool) -> Result<(), NotPure> {
    if text.trim().is_empty() || in_use_span {
        return Ok(());
    }
    if text.starts_with(FLIP_MARK) {
        return text
            .strip_prefix(MOVED_MARK)
            .is_some_and(|id| !id.trim().is_empty())
            .then_some(())
            .ok_or(NotPure::ForeignMarker);
    }
    let declaration = is_use_head(text) || is_mod_declaration(text) || text.starts_with("#[path") || text == "#[cfg(test)]";
    (text.starts_with("//") || declaration).then_some(()).ok_or(NotPure::ResidualLine)
}

// ───────── 要約 ─────────

/// 可視性の字面（無しは `private`）。
fn shown_visibility(visibility: &str) -> &str {
    if visibility.is_empty() {
        "private"
    } else {
        visibility
    }
}

/// 要約の本文を描く（外形は snapshot・C12.5）。
fn render(matched: &Matched, residual: &[(String, Vec<String>)], total: usize) -> MoveSummary {
    let mut lines = vec![HEADLINE.to_owned(), "## 移動（元 -> 先: 本数 / 行数）".to_owned()];
    for found in &matched.moves {
        lines.push(format!("{} -> {}: items={} lines={}", found.from, found.to, found.names.len(), found.lines));
        lines.extend(found.names.iter().map(|name| format!("  {name}")));
    }
    lines.push("## 可視性（名: 前 -> 後）".to_owned());
    for found in &matched.visibility {
        lines.push(format!(
            "{} {}: {} -> {}",
            found.file,
            found.name,
            shown_visibility(&found.before),
            shown_visibility(&found.after)
        ));
    }
    lines.push("## 残差分（逐語）".to_owned());
    for (file, changed) in residual {
        lines.push(file.clone());
        lines.extend(changed.iter().cloned());
    }
    lines.push(format!(
        "判定: 名 + 本文の多重集合が一致 items={total} moved={} visibility={}",
        matched.moved,
        matched.visibility.len()
    ));
    let mut text = lines.join("\n");
    text.push('\n');
    MoveSummary { text }
}

#[cfg(test)]
mod tests {
    use super::{declaration_of, items_of, judge, residual_allowed, strip_visibility, use_spans, LensInput, NotPure, Side};
    use proptest::prelude::*;
    use proptest::test_runner::Config;

    /// 反例の永続化を切り、case 数を 256 に pin する（`pipe::closure` の歯と同じ形）。
    fn config() -> Config {
        Config {
            cases: 256,
            failure_persistence: None,
            ..Config::default()
        }
    }

    /// 可視性の剥がし: 3 形を剥がして記録し、`pub` で始まる識別子（`pubsub`）と `pub(` の閉じない形は剥がさない。
    #[test]
    fn move_proof_strips_the_three_visibility_prefixes_only() {
        assert_eq!(strip_visibility("pub fn a() {"), ("pub".to_owned(), "fn a() {"));
        assert_eq!(strip_visibility("pub(crate) fn a() {"), ("pub(crate)".to_owned(), "fn a() {"));
        assert_eq!(strip_visibility("pub(super) struct A;"), ("pub(super)".to_owned(), "struct A;"));
        assert_eq!(strip_visibility("fn a() {"), (String::new(), "fn a() {"));
        assert_eq!(strip_visibility("pubsub fn a() {"), (String::new(), "pubsub fn a() {"));
        assert_eq!(strip_visibility("pub(crate fn a() {"), (String::new(), "pub(crate fn a() {"));
    }

    /// 宣言の読み: 9 種の keyword・修飾（`unsafe` / `const fn`）・`mod x;` は宣言でない・字下げ行は読まない。
    #[test]
    fn move_proof_reads_column_zero_declarations_only() {
        let named: &[(&str, &str)] = &[
            ("fn one() -> u8 {", "fn one"),
            ("pub(crate) const fn two() {", "fn two"),
            ("pub unsafe fn three() {", "fn three"),
            ("struct Pair;", "struct Pair"),
            ("pub enum Hue {", "enum Hue"),
            ("impl<T> Foo<T> for Bar {", "impl <T> Foo<T> for Bar"),
            ("impl Foo {}", "impl Foo"),
            ("trait Tr {", "trait Tr"),
            ("const CAP: u8 = 1;", "const CAP"),
            ("static ONCE: u8 = 1;", "static ONCE"),
            ("type Alias = u8;", "type Alias"),
            ("mod tests {", "mod tests"),
        ];
        for (line, want) in named {
            assert_eq!(declaration_of(line).map(|decl| decl.name).as_deref(), Some(*want), "{line}");
        }
        // `mod x;` は宣言（item ではない）・字下げ行は読まない・keyword の後ろは空白。
        for line in ["mod tests;", "use std::fs;", "    fn nested() {", "// fn commented() {", "fnord() {"] {
            assert_eq!(declaration_of(line), None, "{line}");
        }
    }

    /// 入れ子の切り出し: `impl {}` の中の fn と inline `mod tests {}` の歯は外側の 1 本に畳む。属性と doc は
    /// item に付き、`;` で終わる const は 1 行、複数行の const は `];` まで、複数行の署名の fn は列 0 の `)` で
    /// 切れず `}` まで、末尾の空行は含めない。
    #[test]
    fn move_proof_folds_nested_items_into_the_outer_item() {
        let text = "//! doc\n\nuse std::fs;\n\n/// d\n#[derive(Debug)]\npub struct A {\n    b: u8,\n}\n\nimpl A {\n    fn m(&self) {}\n    pub fn n(&self) {}\n}\n\nconst ONE: u8 = 1;\n\nconst MANY: &[u8] = &[\n    1,\n];\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n\nfn wide(\n    a: u8,\n) -> u8 {\n    a\n}\n";
        let items = items_of(text);
        let shown: Vec<(&str, (usize, usize))> = items.iter().map(|item| (item.name.as_str(), item.lines)).collect();
        assert_eq!(
            shown,
            [
                ("struct A", (5, 9)),
                ("impl A", (11, 14)),
                ("const ONE", (16, 16)),
                ("const MANY", (18, 20)),
                ("mod tests", (22, 26)),
                ("fn wide", (28, 32)),
            ],
            "列 0 の宣言単位だけ（入れ子は外側に畳む）"
        );
        assert_eq!(items.first().map(|item| item.visibility.as_str()), Some("pub"));
    }

    /// indent の正規化と可視性: 同じ本文を字下げ・可視性だけ変えても hash は同じで、本文が 1 字違えば異なる。
    #[test]
    fn move_proof_hash_ignores_indent_and_visibility_but_not_the_body() {
        let plain = items_of("fn a() {\n    1\n}\n");
        let shifted = items_of("pub(super) fn a() {\n        1\n}\n");
        let changed = items_of("fn a() {\n    2\n}\n");
        let hash = |items: &[super::Item]| items.first().map(|item| item.hash);
        assert!(hash(&plain).is_some(), "母集団 1 本");
        assert_eq!(hash(&plain), hash(&shifted), "indent と可視性は hash に入らない");
        assert_ne!(hash(&plain), hash(&changed), "本文の差は hash に出る");
        assert_eq!(shifted.first().map(|item| item.visibility.as_str()), Some("pub(super)"));
    }

    /// 残差分の弁別: 宣言と札とコメントと空行だけを許し、`moved` 以外の札は `ForeignMarker`、他は `ResidualLine`。
    #[test]
    fn move_proof_residual_lines_are_declarations_markers_comments_or_blank() {
        for line in ["", "   ", "mod alpha;", "pub mod alpha;", "pub(crate) mod b;", "use super::*;", "pub use x::Y;", "#[path = \"x.rs\"]", "#[cfg(test)]", "//! module doc", "/// stray doc", "// ── section ──", "// flip-check: moved s2-07l.261"] {
            assert_eq!(residual_allowed(line, false), Ok(()), "許す: {line:?}");
        }
        assert_eq!(residual_allowed("    Foo,", true), Ok(()), "複数行の use の中の行");
        assert_eq!(residual_allowed("// flip-check: retroactive s2-07l.1", false), Err(NotPure::ForeignMarker));
        assert_eq!(residual_allowed("// flip-check: moved ", false), Err(NotPure::ForeignMarker), "id の無い札");
        for line in ["x", "#![allow(dead_code)]", "    Foo,", "mod alpha {", "let a = 1;", "#[derive(Debug)]"] {
            assert_eq!(residual_allowed(line, false), Err(NotPure::ResidualLine), "許さない: {line:?}");
        }
        assert_eq!(use_spans("use a::{\n    b,\n};\nuse c;\n"), [(1, 3)], "複数行の use の区間だけ");
    }

    /// `judge` の 4 条件を 1 fixture ずつ: 純移動 → Summary・本文差 → ItemsDiffer・移動 0 → NothingMoved・
    /// 読めない file → Unreadable・binary の見出し → Unreadable。
    #[test]
    fn move_proof_judge_pins_each_reason() {
        let base_lib = "fn a() {\n    1\n}\n\nfn b() {\n    2\n}\n";
        let head_lib = "mod m;\n\nfn a() {\n    1\n}\n";
        let head_m = "pub(super) fn b() {\n    2\n}\n";
        let diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,7 +1,5 @@\n+mod m;\n+\n fn a() {\n     1\n }\n-\n-fn b() {\n-    2\n-}\ndiff --git a/src/m.rs b/src/m.rs\nnew file mode 100644\n--- /dev/null\n+++ b/src/m.rs\n@@ -0,0 +1,3 @@\n+pub(super) fn b() {\n+    2\n+}\n";
        let read = |side: Side, path: &str| -> Option<String> {
            match (side, path) {
                (Side::Base, "src/lib.rs") => Some(base_lib.to_owned()),
                (Side::Head, "src/lib.rs") => Some(head_lib.to_owned()),
                (Side::Head, "src/m.rs") => Some(head_m.to_owned()),
                _ => None,
            }
        };
        let LensInput::Summary(summary) = judge(diff, &read) else {
            panic!("純移動は Summary");
        };
        assert!(summary.text().starts_with("これは diff ではなく純移動の要約である\n"), "{}", summary.text());
        assert!(summary.text().contains("src/lib.rs -> src/m.rs: items=1 lines=3\n  fn b\n"), "{}", summary.text());
        assert!(summary.text().contains("src/m.rs fn b: private -> pub(super)\n"), "{}", summary.text());
        assert!(summary.text().contains("\n+mod m;\n"), "残差分は逐語: {}", summary.text());
        assert!(summary.text().ends_with("判定: 名 + 本文の多重集合が一致 items=2 moved=1 visibility=1\n"), "{}", summary.text());

        let changed = |side: Side, path: &str| read(side, path).map(|text| if side == Side::Head { text.replace("    2", "    3") } else { text });
        assert_eq!(judge(diff, &changed), LensInput::Diff(NotPure::ItemsDiffer), "本文が 1 行違う");
        let stayed = |side: Side, path: &str| match (side, path) {
            (Side::Head, "src/lib.rs") => Some(format!("mod m;\n\n{base_lib}")),
            (Side::Head, "src/m.rs") => Some("//! m\n".to_owned()),
            _ => read(side, path),
        };
        assert_eq!(judge(diff, &stayed), LensInput::Diff(NotPure::NothingMoved), "宣言だけ");
        let unreadable = |side: Side, path: &str| if path == "src/m.rs" { None } else { read(side, path) };
        assert_eq!(judge(diff, &unreadable), LensInput::Diff(NotPure::Unreadable), "file を読めない");
        let binary = "diff --git a/x.png b/x.png\nBinary files a/x.png and b/x.png differ\n";
        assert_eq!(judge(binary, &read), LensInput::Diff(NotPure::Unreadable), "binary の見出し");
        assert_eq!(judge("", &read), LensInput::Diff(NotPure::NothingMoved), "空の diff は移動 0");
    }

    /// 本文の断片（列 0 の宣言・入れ子・宣言でない行）。
    const FRAGMENTS: &[&str] = &[
        "fn a() {\n    1\n}\n",
        "pub(crate) fn a() {\n    1\n}\n",
        "/// d\n#[derive(Debug)]\npub struct S {\n    x: u8,\n}\n",
        "impl S {\n    fn m(&self) {}\n}\n",
        "const C: u8 = 1;\n",
        "const M: &[u8] = &[\n    1,\n];\n",
        "mod tests {\n    #[test]\n    fn t() {}\n}\n",
        "fn wide(\n    a: u8,\n) -> u8 {\n    a\n}\n",
        "use std::fs;\n",
        "mod x;\n",
        "// ── section ──\n",
        "\n",
        "    stray();\n",
    ];

    /// 断片の連結（0〜8 本）。
    fn bodies() -> impl Strategy<Value = String> {
        prop::collection::vec(prop::sample::select(FRAGMENTS), 0..9).prop_map(|parts| parts.concat())
    }

    proptest! {
        #![proptest_config(config())]

        /// item の区間は互いに重ならず file の中に収まり、hash は同じ本文で決定的・可視性を剥がしても不変。
        #[test]
        fn prop_move_proof_items_are_disjoint_and_hash_is_deterministic(text in bodies()) {
            let items = items_of(&text);
            let total = text.lines().count();
            let mut last_end = 0;
            for item in &items {
                prop_assert!(item.lines.0 > last_end);
                prop_assert!(item.lines.0 <= item.lines.1);
                prop_assert!(item.lines.1 <= total);
                last_end = item.lines.1;
            }
            let again = items_of(&text);
            prop_assert_eq!(&items, &again);
            let widened = text.replace("pub(crate) fn a()", "fn a()");
            let hashes: Vec<(String, u64)> = items.iter().map(|item| (item.name.clone(), item.hash)).collect();
            let widened_hashes: Vec<(String, u64)> = items_of(&widened).iter().map(|item| (item.name.clone(), item.hash)).collect();
            prop_assert_eq!(hashes, widened_hashes);
        }
    }
}
