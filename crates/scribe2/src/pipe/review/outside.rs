//! 審査の材料の 5 本目: 契約が名指す write-set の外の物（設計 docs/design/contract-source.md §51・行 bc・`s2-07l.430`）。
//!
//! lens は shell も cargo も撃てないので、write-set の外に在る型・data file の鍵・crate の依存・親 module の宣言・depends の
//! 相手の着地を読めず、`section-material-missing` の INCONCLUSIVE が往復する。器が base から名ごとの塊を束ね、材料の dir に
//! [`super::OUTSIDE_FILE`] として置く（組むのは [`outside_text`] の 1 本・`materials` から 1 回だけ呼ぶ・本文が空なら置かない）。
//!
//! 名の照合は名指しの読み手の口 [`mentioned_names`]（`pipe::closure`）、候補の名と所在は base の要約と同じ `declared_name` /
//! `tooth_names`、`.rs` の要約は同じ `item_text` で読む（2 本目の読み手を作らない・C2）。塊は行頭の `- ` と名で始まり、続きの
//! 行は 2 字下げる。並びは (f) depends の相手の行 → (d) crate の依存の表 → (e) 親 module の宣言 → (a) `.rs` の item → (b)
//! 名指された `.rs` の要約 → (c) data file の鍵の行（小さい構造の材料が先）。
//!
//! cap は新しい閾値を作らない: lens が [`outside_block`] で既存の `gate.token_cap` の残り（既存 4 材料と base の段の後）に
//! 名ごとに収め、収まらない名は切り詰めの 1 行、それも収まらない名は最後に落とした本数の 1 行に数える（黙って落とさない）。
//!
//! 本文が指す別の設計の § の本文（§55・行 bg）は子 module `linked` が既存の塊の後ろに束ねる。

mod linked;

use super::base::{declared_name, item_text, tooth_names, ITEM_HEAD, ROW_LINE_WIDTH};
use crate::pipe::closure::{holds_word, mentioned_names, src_region, test_region, Mentioned, Source};
use crate::pipe::contract::Contract;
use crate::pipe::refuse::{normalize, NEW_FILE};
use crate::pipe::table;
use std::collections::BTreeSet;
use std::path::Path;

/// `{outside}` の穴の見出し（名を落とした周も見出しは残す）。
const HEADING: &str = "\n## write-set の外の材料（契約が名指す物を器が base から束ねた事実）\n";

/// 見出しの下の説明の 1 行。
const PREAMBLE: &str = "契約の本文（節・done・約束の行）が名指す write-set の外の物を、名ごとの塊（行頭の `- ` と名）で depends の相手の行・crate の依存の表・親 module の宣言・`.rs` の item・`.rs` の要約・data file の鍵の行の順に並べる。data file は全文でなく、行数と byte 数と鍵の列と、本文に在る鍵を持つ最初の 1 行だけ。その後ろに、本文が指す別の設計の § のうち解けない参照の 1 塊、解けた § の塊（指された順・§ の見出しの次の行から次の見出しの前まで・行を指せばその行の done つき）、束ねた § の本文だけが名指す名の塊（1 段だけ・§ の中の参照は辿らない）の順に並べる。data file と dir の配下の file の行数は改行で数えた生の行（wc -l と同じ）、`.rs` の要約の行数は base の要約と同じ幅で畳んだ数。";

/// crate の manifest の file 名。
const MANIFEST: &str = "Cargo.toml";

/// 依存の表の見出しの末尾（`[dependencies]` / `[dev-dependencies]` / `[target.….dependencies]`）。
const DEPENDENCIES: &str = "dependencies";

/// Rust の file の拡張子。
const RS: &str = ".rs";

/// module の木を持つ dir の名（path がこのどれかの段を持つ `.rs` だけ親を辿る）。
const MODULE_DIRS: &[&str] = &["src", "tests", "benches", "examples"];

/// 直下の `.rs` が crate の根になる dir の名（統合 test・bench・example・bin）。
const ROOT_DIRS: &[&str] = &["tests", "benches", "examples", "bin"];

/// crate の根の file の stem。
const ROOT_STEMS: &[&str] = &["lib", "main"];

/// dir 形の module 自身の file の stem。
const MOD_STEM: &str = "mod";

/// 閉じ括弧までを本体として渡す宣言の語（field と variant の列）。
const BLOCK_KINDS: &[&str] = &["struct", "enum", "union", "trait"];

/// 束ねる base の木（tracked の path・`.rs` の本文・接頭辞を剥がした write-set）。
struct Tree<'a> {
    /// 対象 repo（data file と manifest を読む base）。
    repo: &'a Path,
    /// tracked の repo 相対 path。
    tracked: Vec<String>,
    /// tracked の `.rs` の本文。
    sources: Vec<Source>,
    /// write-set の項目（[`normalize`] で剥がした path・dir は末尾 `/`）。
    write_set: Vec<String>,
}

impl Tree<'_> {
    /// base に在るか（tracked の file か、末尾 `/` の dir の配下に tracked の file を持つ）。
    fn has(&self, path: &str) -> bool {
        self.tracked.iter().any(|found| found == path || (path.ends_with('/') && found.starts_with(path)))
    }

    /// write-set の中か（同じ項目か dir 項目の配下）。
    fn owned(&self, path: &str) -> bool {
        self.write_set.iter().any(|item| item == path || (item.ends_with('/') && path.starts_with(item.as_str())))
    }

    /// `.rs` の本文（tracked の `.rs` でなければ `None`）。
    fn body(&self, path: &str) -> Option<&Result<String, String>> {
        self.sources.iter().find(|source| source.path == path).map(|source| &source.body)
    }
}

/// base の `.rs` の宣言 1 つ（候補の名と所在）。
struct Decl {
    /// 名。
    name: String,
    /// 宣言の語（`fn` / `struct` / …）。
    kind: String,
    /// 宣言する file。
    path: String,
    /// 宣言の行の番号（0 始まり）。
    line: usize,
    /// 歯（歯の区間の `#[test]` の fn）か。
    tooth: bool,
}

/// 材料の本文を組む（`materials` から 1 回だけ呼ぶ）。`bodies` は節の本文・done・約束の行の text。名指しの無い契約は空。
pub(super) fn outside_text(repo: &Path, contract: &Contract, bodies: &[&str]) -> String {
    let Some(tracked) = table::tracked_files(repo) else {
        return "（外の材料を作れない: tracked の file を読めない）".to_owned();
    };
    let sources = table::read_all(repo, &tracked, RS);
    let write_set = contract.write_set.iter().map(|item| normalize(item)).collect();
    bundle(&Tree { repo, tracked, sources, write_set }, &contract.design, bodies)
}

/// 木と本文から塊を (f) → (d) → (e) → (a) → (b) → (c) の順に並べ、別の設計の § の (g) → (h) → (i) を後ろに足す（§55）。
fn bundle(tree: &Tree<'_>, design: &str, bodies: &[&str]) -> String {
    let decls: Vec<Decl> = tree.sources.iter().flat_map(declarations).collect();
    let candidates: Vec<&str> = decls.iter().map(|decl| decl.name.as_str()).collect::<BTreeSet<&str>>().into_iter().collect();
    let found = mentioned_names(bodies, &candidates, &tree.tracked);
    let doc = table::parse_pointer(design).ok().map(|pointer| pointer.path);
    let mut chunks = depends_chunks(tree, design);
    chunks.extend(manifest_chunks(tree, &found));
    chunks.extend(tree.write_set.iter().filter(|item| item.ends_with(RS)).filter_map(|item| parent_chunk(tree, item)));
    chunks.extend(found.names.iter().filter_map(|name| item_chunk(tree, &decls, name)));
    chunks.extend(rs_chunks(tree, &found));
    chunks.extend(data_chunks(tree, &found, doc.as_deref(), &bodies.join("\n")));
    chunks.extend(linked::linked_chunks(tree, (&decls, &candidates), &found, design, bodies));
    chunks.join("\n")
}

/// 1 本の `.rs` の宣言（本体の区間の [`declared_name`] と歯の区間の [`tooth_names`]＝base の要約と同じ読み）。
fn declarations(source: &Source) -> Vec<Decl> {
    let Ok(text) = &source.body else {
        return Vec::new();
    };
    let region = test_region(&source.path, text);
    let first_tooth = text.get(..text.len().saturating_sub(region.len())).unwrap_or_default().matches('\n').count();
    let teeth = tooth_names(region);
    let src_lines = src_region(text).lines().count();
    let mut found = Vec::new();
    for (line, raw) in text.lines().enumerate() {
        let Some((kind, name)) = declared_name(raw).and_then(|decl| decl.split_once(' ').map(|(k, n)| (k.to_owned(), n.to_owned())))
        else {
            continue;
        };
        let tooth = line >= first_tooth && kind == "fn" && teeth.contains(&name);
        if tooth || line < src_lines {
            found.push(Decl { name, kind, path: source.path.clone(), line, tooth });
        }
    }
    found
}

/// (f) depends の相手の行: 契約表の行の depends の各 id の title と、その行の `+` の項目（と creates）ごとの base の在否。
fn depends_chunks(tree: &Tree<'_>, design: &str) -> Vec<String> {
    let Ok(pointer) = table::parse_pointer(design) else {
        return Vec::new();
    };
    let Ok(text) = table::read(tree.repo, &pointer.path) else {
        return Vec::new();
    };
    let Ok(row) = table::find_row(&pointer.path, &text, &pointer.id) else {
        return Vec::new();
    };
    row.depends.iter().map(|id| depends_chunk(tree, (&pointer.path, &text), id)).collect()
}

/// depends の相手の行 1 つの塊（`doc` は設計 doc の path と本文）。
fn depends_chunk(tree: &Tree<'_>, doc: (&str, &str), id: &str) -> String {
    let row = match table::find_row(doc.0, doc.1, id) {
        Ok(found) => found,
        Err(errors) => {
            let reasons: Vec<String> = errors.iter().map(table::TableError::reason).collect();
            return format!("{ITEM_HEAD}depends {id}: 読めない（{}）", reasons.join(" / "));
        }
    };
    let fresh = row.write_set.iter().filter_map(|item| item.strip_prefix(NEW_FILE)).map(str::to_owned).chain(row.creates);
    let lines: Vec<String> =
        fresh.map(|path| format!("  +{path}: {}", if tree.has(&path) { "base に在る" } else { "base に無い" })).collect();
    let head = format!("{ITEM_HEAD}depends {id}: {}", row.title);
    std::iter::once(head).chain(lines).collect::<Vec<String>>().join("\n")
}

/// (d) crate の依存: write-set の各項目の最も近い祖先の Cargo.toml と名指された Cargo.toml（write-set の中は除く）。
fn manifest_chunks(tree: &Tree<'_>, found: &Mentioned) -> Vec<String> {
    let nearest = tree.write_set.iter().filter_map(|item| nearest_manifest(tree, item));
    let named = found.files.iter().filter(|path| is_manifest(path)).cloned();
    let mut manifests: Vec<String> = Vec::new();
    for path in nearest.chain(named) {
        if !manifests.contains(&path) && !tree.owned(&path) {
            manifests.push(path);
        }
    }
    manifests.iter().map(|path| manifest_chunk(tree, path)).collect()
}

/// 項目の祖先の dir を下から辿って最初に在る tracked の Cargo.toml。
fn nearest_manifest(tree: &Tree<'_>, item: &str) -> Option<String> {
    let mut rest = item.trim_end_matches('/');
    while let Some((up, _)) = rest.rsplit_once('/') {
        let path = format!("{up}/{MANIFEST}");
        if tree.has(&path) {
            return Some(path);
        }
        rest = up;
    }
    tree.has(MANIFEST).then(|| MANIFEST.to_owned())
}

/// path が crate の manifest か。
fn is_manifest(path: &str) -> bool {
    path.rsplit('/').next() == Some(MANIFEST)
}

/// manifest 1 本の塊: 依存の表（見出しが dependencies で終わる表）の見出しと行・無ければその 1 行。
fn manifest_chunk(tree: &Tree<'_>, path: &str) -> String {
    let text = match table::read(tree.repo, path) {
        Ok(found) => found,
        Err(reason) => return format!("{ITEM_HEAD}{path}: 読めない（{reason}）"),
    };
    let mut lines = Vec::new();
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line.trim_start_matches('[').trim_end_matches(']').trim().ends_with(DEPENDENCIES);
        }
        if inside && !line.is_empty() && !line.starts_with('#') {
            lines.push(format!("  {line}"));
        }
    }
    if lines.is_empty() {
        return format!("{ITEM_HEAD}{path}: 依存の表なし");
    }
    format!("{ITEM_HEAD}{path}: 依存の表\n{}", lines.join("\n"))
}

/// (e) 親 module の宣言: write-set の `.rs` 1 本から crate の根まで、write-set の外の親 file の mod の行か「宣言なし」。
fn parent_chunk(tree: &Tree<'_>, item: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut current = item.to_owned();
    while let Some((module, candidates)) = parent_of(&current) {
        let Some(parent) = candidates.into_iter().find(|path| tree.has(path) || tree.write_set.contains(path)) else {
            lines.push(format!("  mod {module} の親 file が base に無い（宣言なし）"));
            break;
        };
        if !tree.owned(&parent) {
            lines.push(mod_line(tree, &parent, &module));
        }
        current = parent;
    }
    (!lines.is_empty()).then(|| format!("{ITEM_HEAD}{item} の親 module: {} 段\n{}", lines.len(), lines.join("\n")))
}

/// `.rs` の module の名と親 file の候補（`D/x.rs` は D の子・`D/mod.rs` は D そのもの）。crate の根（`lib.rs` / `main.rs`・
/// 統合 test の直下）と module の木の外の file は `None`。`#[path]` 属性は解かない（下界）。
fn parent_of(path: &str) -> Option<(String, Vec<String>)> {
    let (dir, file) = path.rsplit_once('/')?;
    let stem = file.strip_suffix(RS)?;
    let segments: Vec<&str> = dir.split('/').collect();
    let rooted = segments.last().is_some_and(|last| ROOT_DIRS.contains(last)) && stem != MOD_STEM;
    if ROOT_STEMS.contains(&stem) || rooted || !segments.iter().any(|segment| MODULE_DIRS.contains(segment)) {
        return None;
    }
    let (home, module) = if stem == MOD_STEM { dir.rsplit_once('/')? } else { (dir, stem) };
    let mut candidates = vec![format!("{home}/{MOD_STEM}{RS}")];
    candidates.extend(home.rsplit_once('/').map(|(up, name)| format!("{up}/{name}{RS}")));
    candidates.extend(ROOT_STEMS.iter().map(|root| format!("{home}/{root}{RS}")));
    Some((module.to_owned(), candidates))
}

/// 親 file の `mod <module>` の行の字面（可視性を含む）か、無ければ「宣言なし」。
fn mod_line(tree: &Tree<'_>, parent: &str, module: &str) -> String {
    let want = format!("mod {module}");
    match tree.body(parent) {
        Some(Ok(text)) => text
            .lines()
            .enumerate()
            .find(|(_, line)| declared_name(line).as_deref() == Some(want.as_str()))
            .map_or_else(|| format!("  {parent}: {want} の宣言なし"), |(at, line)| format!("  {parent}:{}: {}", at.saturating_add(1), line.trim())),
        Some(Err(reason)) => format!("  {parent}: 読めない（{reason}）"),
        None => format!("  {parent}: 読めない（base の .rs に無い）"),
    }
}

/// (a) `.rs` の item: write-set の中に宣言が在る名は出さず、外の 2 file 以上に在る名は所在の 1 行だけ。
fn item_chunk(tree: &Tree<'_>, decls: &[Decl], name: &str) -> Option<String> {
    let named: Vec<&Decl> = decls.iter().filter(|decl| decl.name == name).collect();
    if named.is_empty() || named.iter().any(|decl| tree.owned(&decl.path)) {
        return None;
    }
    let places: Vec<String> = named.iter().map(|decl| format!("{}:{}", decl.path, decl.line.saturating_add(1))).collect();
    let files: BTreeSet<&str> = named.iter().map(|decl| decl.path.as_str()).collect();
    if files.len() >= 2 {
        return Some(format!("{ITEM_HEAD}{name}: 宣言 {} か所（{}）", places.len(), places.join(", ")));
    }
    let lines: Vec<String> = named.iter().flat_map(|decl| item_lines(tree, decl)).collect();
    Some(format!("{ITEM_HEAD}{name}: {}\n{}", places.join(", "), lines.join("\n")))
}

/// 宣言 1 つの行: 直上の doc 行と属性行・宣言の行・struct / enum / union / trait と歯は閉じ括弧まで・fn は署名まで。
fn item_lines(tree: &Tree<'_>, decl: &Decl) -> Vec<String> {
    let Some(Ok(text)) = tree.body(&decl.path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut start = decl.line;
    while let Some(above) = start.checked_sub(1).and_then(|at| lines.get(at)).map(|line| line.trim_start()) {
        if !(above.starts_with("///") || above.starts_with("#[")) {
            break;
        }
        start = start.saturating_sub(1);
    }
    let end = if decl.tooth || BLOCK_KINDS.contains(&decl.kind.as_str()) {
        block_end(&lines, decl.line)
    } else if decl.kind == "fn" {
        lines.iter().enumerate().skip(decl.line).find(|(_, line)| line.contains('{') || line.trim_end().ends_with(';')).map_or(decl.line, |(at, _)| at)
    } else {
        decl.line
    };
    lines.get(start..=end).unwrap_or_default().iter().map(|line| format!("  {}", line.trim_end())).collect()
}

/// 宣言の行から本体の閉じ括弧の行まで（括弧を開かず `;` で終わる行はその行・閉じなければ file の末尾）。
fn block_end(lines: &[&str], at: usize) -> usize {
    let mut depth = 0_usize;
    let mut opened = false;
    for (index, line) in lines.iter().enumerate().skip(at) {
        let opens = line.matches('{').count();
        depth = depth.saturating_add(opens).saturating_sub(line.matches('}').count());
        opened = opened || opens > 0;
        if (opened && depth == 0) || (!opened && line.trim_end().ends_with(';')) {
            return index;
        }
    }
    lines.len().saturating_sub(1)
}

/// (b) 名指された `.rs`（write-set の外）: base の要約と同じ 1 項目。
fn rs_chunks(tree: &Tree<'_>, found: &Mentioned) -> Vec<String> {
    let named = found.files.iter().filter(|path| path.ends_with(RS) && !tree.owned(path));
    match crate::seat::int_rule(ROW_LINE_WIDTH) {
        Ok(width) => named.map(|path| item_text(tree.repo, path, width)).collect(),
        Err(read) => named.map(|path| format!("{ITEM_HEAD}{path}: 読めない（rules 行 {ROW_LINE_WIDTH} を読めない・{}）", read.as_str())).collect(),
    }
}

/// (c) data file（名指された `.rs` と manifest でない tracked の file・write-set の中と契約の設計 doc は除く）と dir。
fn data_chunks(tree: &Tree<'_>, found: &Mentioned, doc: Option<&str>, body: &str) -> Vec<String> {
    let files = found.files.iter().filter(|path| !(path.ends_with(RS) || is_manifest(path) || tree.owned(path) || doc == Some(path.as_str())));
    let mut chunks: Vec<String> = files.map(|path| file_chunk(tree, path, body)).collect();
    chunks.extend(found.dirs.iter().filter(|dir| !tree.owned(&format!("{dir}/"))).map(|dir| dir_chunk(tree, dir)));
    chunks
}

/// data file 1 本の塊: 行数と byte 数・拡張子ごとの鍵の列・本文に語の境界で在る鍵ごとにその鍵を持つ最初の 1 行（全文は渡さない）。
fn file_chunk(tree: &Tree<'_>, path: &str, body: &str) -> String {
    let bytes = match std::fs::read(tree.repo.join(path)) {
        Ok(found) => found,
        Err(err) => return format!("{ITEM_HEAD}{path}: 読めない（{err}）"),
    };
    let text = String::from_utf8_lossy(&bytes);
    let head = format!("{ITEM_HEAD}{path}: 行数 {} / byte {}", text.lines().count(), bytes.len());
    let Some(keys) = keys_of(path, &text) else {
        return head;
    };
    let shown: Vec<&str> = keys.iter().map(|key| key.0.as_str()).collect();
    let mut lines = vec![head, format!("  鍵: {}", if shown.is_empty() { "なし".to_owned() } else { shown.join(", ") })];
    for (shown, word) in keys.iter().filter(|(_, word)| holds_word(body, word)) {
        let first = |needle: &str| text.lines().enumerate().find(|(_, line)| holds_word(line, needle));
        if let Some((at, line)) = first(shown).or_else(|| first(word)) {
            lines.push(format!("  行 {}: {}", at.saturating_add(1), line.trim()));
        }
    }
    lines.join("\n")
}

/// 拡張子ごとの鍵の列（(見せる字面, 本文と照合する語) の対・重複なし）。鍵の読み手を持たない拡張子は `None`。
fn keys_of(path: &str, text: &str) -> Option<Vec<(String, String)>> {
    let (_, ext) = path.rsplit('/').next()?.rsplit_once('.')?;
    let keys = match ext {
        "json" => json_keys(text),
        "css" => css_keys(text),
        "yaml" | "yml" => yaml_keys(text),
        "toml" => toml_keys(text),
        _ => return None,
    };
    let mut unique: Vec<(String, String)> = Vec::new();
    for key in keys.into_iter().filter(|(_, word)| !word.is_empty()) {
        if !unique.contains(&key) {
            unique.push(key);
        }
    }
    Some(unique)
}

/// `.json` の `"鍵":` の鍵。
fn json_keys(text: &str) -> Vec<(String, String)> {
    let pieces: Vec<&str> = text.split('"').collect();
    let pairs = pieces.iter().zip(pieces.iter().skip(1)).skip(1).step_by(2);
    pairs.filter(|(_, after)| after.trim_start().starts_with(':')).map(|(key, _)| (format!("\"{key}\""), (*key).to_owned())).collect()
}

/// `.css` の選択子（`{` の前の字面）の `.class`。
fn css_keys(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut prelude = String::new();
    for letter in text.chars() {
        match letter {
            '{' => {
                found.extend(prelude.split('.').skip(1).filter_map(class_of));
                prelude.clear();
            }
            '}' | ';' => prelude.clear(),
            _ => prelude.push(letter),
        }
    }
    found
}

/// 選択子の `.` の後ろの class 名（英字か `_` / `-` で始まる）。
fn class_of(rest: &str) -> Option<(String, String)> {
    let name: String = rest.chars().take_while(|found| found.is_ascii_alphanumeric() || *found == '_' || *found == '-').collect();
    name.starts_with(|found: char| found.is_ascii_alphabetic() || found == '_' || found == '-').then(|| (format!(".{name}"), name))
}

/// `.yaml` / `.yml` の `id:` の値と行頭の鍵。
fn yaml_keys(text: &str) -> Vec<(String, String)> {
    let key = |word: &str| (word.to_owned(), word.to_owned());
    text.lines()
        .filter_map(|line| {
            let item = line.trim_start().trim_start_matches("- ").trim_start();
            if let Some(value) = item.strip_prefix("id:") {
                return Some(key(value.trim().trim_matches(['"', '\''])));
            }
            let head = !line.starts_with(|found: char| found.is_whitespace() || found == '-' || found == '#');
            line.split_once(':').filter(|_| head).map(|(name, _)| key(name.trim()))
        })
        .collect()
}

/// `.toml` の表の見出しと行頭の鍵。
fn toml_keys(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            if line.starts_with('[') {
                let name = line.trim().trim_start_matches('[').trim_end_matches(']').trim();
                return Some((line.trim().to_owned(), name.to_owned()));
            }
            let head = !line.starts_with(|found: char| found.is_whitespace() || found == '#');
            line.split_once('=').filter(|_| head).map(|(name, _)| (name.trim().to_owned(), name.trim().to_owned()))
        })
        .collect()
}

/// 名指しが解けた tracked の dir の塊: 配下の file ごとに path・行数・byte 数の 1 行。
fn dir_chunk(tree: &Tree<'_>, dir: &str) -> String {
    let prefix = format!("{dir}/");
    let files: Vec<&String> = tree.tracked.iter().filter(|path| path.starts_with(&prefix)).collect();
    let lines = files.iter().map(|path| match std::fs::read(tree.repo.join(path)) {
        Ok(bytes) => format!("  {path}: 行数 {} / byte {}", String::from_utf8_lossy(&bytes).lines().count(), bytes.len()),
        Err(err) => format!("  {path}: 読めない（{err}）"),
    });
    let head = format!("{ITEM_HEAD}{prefix}: 配下 {} file", files.len());
    std::iter::once(head).chain(lines).collect::<Vec<String>>().join("\n")
}

/// `{outside}` の穴の本文（lens が埋める）: 写しが空なら空文字（雛形は 1 字も変わらない）。見出しと説明の後に塊を順に
/// `room` byte の残りへ収め、収まらない名は切り詰めの 1 行（名と塊の byte 数）、それも収まらない名は数えて最後に本数の
/// 1 行を残す（既存 cap の残りで測る・新しい閾値を作らない）。
pub fn outside_block(copy: &str, room: u64) -> String {
    let copy = copy.trim_end();
    if copy.is_empty() {
        return String::new();
    }
    let fits = |bytes: usize| u64::try_from(bytes).unwrap_or(u64::MAX) <= room;
    let mut block = format!("{HEADING}{PREAMBLE}\n");
    let mut dropped = 0_usize;
    for chunk in chunks(copy) {
        let whole = format!("\n{chunk}");
        let name = chunk.lines().next().unwrap_or_default().trim_start_matches(ITEM_HEAD);
        let name = name.split_once(": ").map_or(name, |(head, _)| head);
        let short = format!("\n{ITEM_HEAD}{name}: 切り詰めた（塊 {} byte が cap の残りに収まらない）", chunk.len());
        if fits(block.len().saturating_add(whole.len())) {
            block.push_str(&whole);
        } else if fits(block.len().saturating_add(short.len())) {
            block.push_str(&short);
        } else {
            dropped = dropped.saturating_add(1);
        }
    }
    if dropped > 0 {
        block.push_str(&format!("\n（cap の残りに収まらず落とした名: {dropped} 本）"));
    }
    block
}

/// 写しを塊に割る（行頭の [`ITEM_HEAD`] が塊の頭・続きの行は字下げ）。
fn chunks(copy: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in copy.lines() {
        match found.last_mut() {
            Some(open) if !line.starts_with(ITEM_HEAD) => {
                open.push('\n');
                open.push_str(line);
            }
            _ => found.push(line.to_owned()),
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{bundle, chunks, outside_block, Tree, HEADING, PREAMBLE};
    use crate::pipe::refuse::normalize;
    use crate::pipe::table;
    use std::path::{Path, PathBuf};

    /// 隣の project の実例の型を起こした toy の木: 別 crate の山 2 つの struct・2 file に在る struct・write-set の中と外の
    /// 同名・json / css / yaml の data file・crate の manifest・親 file・depends の相手の行を持つ設計 doc。
    const FILES: &[(&str, &str)] = &[
        (
            "crates/other/src/shape.rs",
            "//! 形。\n\n/// 形の 1 つ。\n#[derive(Debug)]\npub struct ZqShape {\n    pub zq_width: u8,\n    pub zq_height: u8,\n}\n\npub fn zq_area(shape: &ZqShape) -> u8 {\n    shape.zq_width\n}\n",
        ),
        ("crates/other/src/twin_a.rs", "pub struct ZqTwin;\n"),
        ("crates/other/src/twin_b.rs", "pub struct ZqTwin;\n"),
        ("crates/other/src/inner_copy.rs", "pub struct ZqInner {\n    pub y: u8,\n}\n"),
        ("crates/toy/src/inner.rs", "pub struct ZqInner {\n    pub x: u8,\n}\n"),
        ("crates/toy/src/lib.rs", "//! toy。\n\npub(crate) mod inner;\n"),
        ("crates/toy/src/landed.rs", "pub fn landed() {}\n"),
        ("crates/toy/fixtures/data.json", "{\n  \"zq_key\": 1,\n  \"other_key\": 2\n}\n"),
        ("crates/toy/assets/look.css", ".zq-fold {\n  color: red;\n}\n.zq-open .zq-leaf {\n  margin: 0.5em;\n}\n"),
        ("crates/toy/fixtures/reqs.yaml", "requirements:\n  - id: ZQ-1\n    text: one\n  - id: ZQ-2\n    text: two\n"),
        ("crates/toy/Cargo.toml", "[package]\nname = \"toy\"\n\n[dependencies]\nzq-dep = \"1\"\n\n[dev-dependencies]\nzq-dev = \"2\"\n"),
        ("docs/design/t.md", DOC),
    ];

    /// 設計 doc: 行 a は行 b に depends し、行 b は base に在る `+` と無い `+` を 1 つずつ持つ。行 g は depends を持たない。
    const DOC: &str = "# t\n\n## 1. 節\n\n本文。\n\n<!-- contracts:begin -->\nschema = 1\n\n[[contract]]\nid = \"a\"\ntitle = \"行 a\"\nreq = [\"FR1\"]\nsection = \"1\"\nwrite-set = [\"crates/toy/src/inner.rs\"]\nverify = [\"git status\"]\nsize = \"S\"\ndone = \"a\"\ndepends = [\"b\"]\n\n[[contract]]\nid = \"b\"\ntitle = \"行 b の題\"\nreq = [\"FR1\"]\nsection = \"1\"\nwrite-set = [\"+crates/toy/src/landed.rs\", \"+crates/toy/src/pending.rs\"]\nverify = [\"git status\"]\nsize = \"S\"\ndone = \"b\"\n\n[[contract]]\nid = \"g\"\ntitle = \"行 g\"\nreq = [\"FR1\"]\nsection = \"1\"\nwrite-set = [\"crates/toy/src/lib.rs\"]\nverify = [\"git status\"]\nsize = \"S\"\ndone = \"g\"\n<!-- contracts:end -->\n";

    /// 節の本文（名は backtick の外・data file は basename だけ）。
    const BODY: &str = "節は ZqShape と ZqTwin と ZqInner を読み、data.json の zq_key と look.css の zq-fold と reqs.yaml の ZQ-2 を名指す。";

    /// 歯ごとの toy の木（file を書いた tmp dir と tracked の列）。
    fn scratch(name: &str) -> (PathBuf, Vec<String>) {
        let dir = std::env::temp_dir().join(format!("pipe-review-outside-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, body) in FILES {
            let target = dir.join(path);
            let _ = std::fs::create_dir_all(target.parent().unwrap_or(&dir));
            let _ = std::fs::write(&target, body);
        }
        (dir, FILES.iter().map(|(path, _)| (*path).to_owned()).collect())
    }

    /// 木と write-set と本文から材料を組む。
    fn text_of(repo: &Path, tracked: &[String], write_set: &[&str], design: &str, bodies: &[&str]) -> String {
        let sources = table::read_all(repo, tracked, ".rs");
        let write_set = write_set.iter().map(|item| normalize(item)).collect();
        bundle(&Tree { repo, tracked: tracked.to_vec(), sources, write_set }, design, bodies)
    }

    /// 名 `head` の塊（行頭の `- <head>` から次の塊の前まで）。
    fn chunk_of(text: &str, head: &str) -> String {
        chunks(text).into_iter().find(|chunk| chunk.starts_with(&format!("- {head}"))).unwrap_or_default()
    }

    /// (a) 別 crate の山 2 つの struct を backtick の外で名指すと所在・doc 行と属性行・可視性を含む宣言の行・field の列が出て、
    /// write-set の中にも宣言が在る名（`ZqInner`）は出ず、2 file に在る名（`ZqTwin`）は所在の 1 行だけ。
    #[test]
    fn pipe_review_outside_names_an_outside_struct_with_its_fields() {
        let (repo, tracked) = scratch("struct");
        let text = text_of(&repo, &tracked, &["crates/toy/src/inner.rs"], "docs/design/t.md#g", &[BODY]);
        assert_eq!(
            chunk_of(&text, "ZqShape").lines().collect::<Vec<&str>>(),
            [
                "- ZqShape: crates/other/src/shape.rs:5",
                "  /// 形の 1 つ。",
                "  #[derive(Debug)]",
                "  pub struct ZqShape {",
                "      pub zq_width: u8,",
                "      pub zq_height: u8,",
                "  }",
            ],
            "{text}"
        );
        assert_eq!(chunk_of(&text, "ZqTwin"), "- ZqTwin: 宣言 2 か所（crates/other/src/twin_a.rs:1, crates/other/src/twin_b.rs:1）", "{text}");
        assert!(!text.contains("ZqInner"), "write-set の中の同名は出ない: {text}");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// (b) 名指された json・stylesheet・yaml が行数と byte 数と鍵の列を持ち、本文に在る鍵（json の鍵・class・id）を持つ行
    /// だけが出て前後の行は出ず、basename だけの名指しが一意に解ける（母集団 = data file 3 本を同じ assert で数える）。
    #[test]
    fn pipe_review_outside_data_files_carry_keys_and_only_the_key_lines() {
        let (repo, tracked) = scratch("data");
        let text = text_of(&repo, &tracked, &["crates/toy/src/inner.rs"], "docs/design/t.md#g", &[BODY]);
        let json = chunk_of(&text, "crates/toy/fixtures/data.json");
        let css = chunk_of(&text, "crates/toy/assets/look.css");
        let yaml = chunk_of(&text, "crates/toy/fixtures/reqs.yaml");
        let bytes = |path: &str| FILES.iter().find(|(found, _)| *found == path).map_or(0, |(_, body)| body.len());
        assert_eq!(
            [json, css, yaml],
            [
                format!(
                    "- crates/toy/fixtures/data.json: 行数 4 / byte {}\n  鍵: \"zq_key\", \"other_key\"\n  行 2: \"zq_key\": 1,",
                    bytes("crates/toy/fixtures/data.json")
                ),
                format!(
                    "- crates/toy/assets/look.css: 行数 6 / byte {}\n  鍵: .zq-fold, .zq-open, .zq-leaf\n  行 1: .zq-fold {{",
                    bytes("crates/toy/assets/look.css")
                ),
                format!(
                    "- crates/toy/fixtures/reqs.yaml: 行数 5 / byte {}\n  鍵: requirements, ZQ-1, ZQ-2\n  行 4: - id: ZQ-2",
                    bytes("crates/toy/fixtures/reqs.yaml")
                ),
            ],
            "{text}"
        );
        assert!(!text.contains("text: two") && !text.contains("color: red"), "前後の行は出ない: {text}");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// (c) write-set の `.rs` の crate の Cargo.toml の依存の表の見出しと行が出て（`[package]` は出ない）、Cargo.toml が
    /// write-set に在る周は出ない。
    #[test]
    fn pipe_review_outside_manifest_dependencies_unless_the_manifest_is_in_the_write_set() {
        let (repo, tracked) = scratch("manifest");
        let text = text_of(&repo, &tracked, &["crates/toy/src/inner.rs"], "docs/design/t.md#g", &[]);
        assert_eq!(
            chunk_of(&text, "crates/toy/Cargo.toml"),
            "- crates/toy/Cargo.toml: 依存の表\n  [dependencies]\n  zq-dep = \"1\"\n  [dev-dependencies]\n  zq-dev = \"2\"",
            "{text}"
        );
        let owned = text_of(&repo, &tracked, &["crates/toy/src/inner.rs", "crates/toy/Cargo.toml"], "docs/design/t.md#g", &[]);
        assert!(!owned.contains("Cargo.toml"), "{owned}");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// (d) 親 file の mod の行が可視性つきで出て、親に宣言の無い `+` の file は「宣言なし」。
    #[test]
    fn pipe_review_outside_parent_module_lines_or_no_declaration() {
        let (repo, tracked) = scratch("parent");
        let text = text_of(&repo, &tracked, &["crates/toy/src/inner.rs", "+crates/toy/src/fresh.rs"], "docs/design/t.md#g", &[]);
        assert_eq!(
            chunk_of(&text, "crates/toy/src/inner.rs の親 module"),
            "- crates/toy/src/inner.rs の親 module: 1 段\n  crates/toy/src/lib.rs:3: pub(crate) mod inner;",
            "{text}"
        );
        assert_eq!(
            chunk_of(&text, "crates/toy/src/fresh.rs の親 module"),
            "- crates/toy/src/fresh.rs の親 module: 1 段\n  crates/toy/src/lib.rs: mod fresh の宣言なし",
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// (e) depends の相手の行の title と、その行の `+` の項目ごとの base の在否。塊の並びは depends → 依存の表 → 親 module →
    /// item → data file（構造の材料が先）。
    #[test]
    fn pipe_review_outside_depends_rows_title_and_plus_items_presence() {
        let (repo, tracked) = scratch("depends");
        let text = text_of(&repo, &tracked, &["crates/toy/src/inner.rs"], "docs/design/t.md#a", &[BODY]);
        assert_eq!(
            chunk_of(&text, "depends b"),
            "- depends b: 行 b の題\n  +crates/toy/src/landed.rs: base に在る\n  +crates/toy/src/pending.rs: base に無い",
            "{text}"
        );
        let heads: Vec<String> = chunks(&text).iter().map(|chunk| chunk.split(':').next().unwrap_or_default().to_owned()).collect();
        let at = |head: &str| heads.iter().position(|found| found.starts_with(head)).unwrap_or(usize::MAX);
        assert!(at("- depends b") < at("- crates/toy/Cargo.toml"), "{heads:?}");
        assert!(at("- crates/toy/Cargo.toml") < at("- crates/toy/src/inner.rs の親"), "{heads:?}");
        assert!(at("- crates/toy/src/inner.rs の親") < at("- ZqShape"), "{heads:?}");
        assert!(at("- ZqShape") < at("- crates/toy/fixtures/data.json"), "{heads:?}");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// (f) 残りが足りない周は名ごとの切り詰めの行、全く足りない周は落とした本数の 1 行（母集団 = 塊 3 本を同じ assert で
    /// 数える）・全部収まる周は写しの全部・空の写しは空文字。
    #[test]
    fn pipe_review_outside_block_truncates_per_name_then_counts_the_dropped() {
        let long = "x".repeat(400);
        let copy = format!("- ZqA: a.rs:1\n  pub struct ZqA {{\n  {long}\n  }}\n- ZqB: b.rs:1\n  {long}\n- c.json: 行数 1 / byte 2\n  {long}\n");
        let pieces = chunks(&copy);
        assert_eq!(pieces.len(), 3, "母集団");
        let full = outside_block(&copy, u64::MAX);
        assert!(full.starts_with("\n## ") && full.ends_with(copy.trim_end()), "{full}");
        let head = u64::try_from(format!("{HEADING}{PREAMBLE}\n").len()).unwrap_or(u64::MAX);
        let short = outside_block(&copy, head.saturating_add(300));
        assert_eq!(short.lines().filter(|line| line.contains("切り詰めた")).count(), pieces.len(), "名ごとに切り詰めの 1 行: {short}");
        let first = pieces.first().map_or(0, String::len);
        assert!(short.contains(&format!("- ZqA: 切り詰めた（塊 {first} byte が cap の残りに収まらない）")), "{short}");
        assert!(!short.contains(&long) && !short.contains("落とした名"), "{short}");
        let none = outside_block(&copy, head);
        assert!(none.ends_with(&format!("（cap の残りに収まらず落とした名: {} 本）", pieces.len())), "{none}");
        assert!(!none.contains("ZqA") && !none.contains("c.json"), "{none}");
        assert_eq!(outside_block("\n", u64::MAX), "", "空の写しは空文字");
    }

    /// (g) 名指しの無い契約（素の 1 語 `inner` / `shape` だけを持つ契約を含む・write-set は crate の根で manifest の無い木）は
    /// 材料が空。
    #[test]
    fn pipe_review_outside_no_mention_is_empty() {
        let (repo, tracked) = scratch("none");
        let bare: Vec<String> = tracked.iter().filter(|path| !path.ends_with("Cargo.toml")).cloned().collect();
        let text = text_of(&repo, &bare, &["crates/toy/src/lib.rs"], "docs/design/t.md#g", &["inner と shape と area だけを読む"]);
        assert_eq!(text, "", "名指しが無ければ空");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// §55 の長い § の本文（§ 4 の 1 行）。
    fn long_line() -> String {
        "x".repeat(2000)
    }

    /// §55 の toy の木: 既存の木に設計 doc 2 本（自分の doc s.md に § 4 つと行 p〔§1＝自分の §〕・q〔§3〕、別の doc o.md に
    /// § 2 つと行 r〔§1〕）を足す。s.md §2 は `ZqShape` と `ZqTwin` と o.md §2 を、o.md §2 は `zq_area` を名指す。
    fn linked_scratch(name: &str) -> (PathBuf, Vec<String>) {
        let (repo, mut tracked) = scratch(name);
        let row = |id: &str, section: &str| {
            format!("\n[[contract]]\nid = \"{id}\"\ntitle = \"t{id}\"\nreq = [\"FR1\"]\nsection = \"{section}\"\nwrite-set = [\"crates/toy/src/lib.rs\"]\nverify = [\"git status\"]\nsize = \"S\"\ndone = \"{id} の done\"\n")
        };
        let table = |rows: String| format!("<!-- contracts:begin -->\nschema = 1\n{rows}<!-- contracts:end -->\n");
        let own = format!(
            "# s\n\n## 1. 自分の節\n\n自分の本文。\n\n## 2. 二の節\n\n- 二の本文は `ZqShape` と `ZqTwin` と o.md §2 を名指す。\n  二の 2 行目。\n\n## 3. 三の節\n\n三の本文。\n\n## 4. 四の節\n\n{}\n\n{}",
            long_line(),
            table(format!("{}{}", row("p", "1"), row("q", "3")))
        );
        let other = format!("# o\n\n## 1. 一の節\n\n一の本文。\n\n## 2. 二の節\n\n他の二は `zq_area` を名指す。\n\n{}", table(row("r", "1")));
        for (path, body) in [("docs/design/s.md", own), ("docs/design/o.md", other)] {
            let _ = std::fs::write(repo.join(path), body);
            tracked.push(path.to_owned());
        }
        (repo, tracked)
    }

    /// § の塊の頭（`- <doc の path> §N` の 1 行だけの頭）の列。
    fn section_heads(text: &str) -> Vec<String> {
        chunks(text).iter().filter_map(|chunk| chunk.lines().next()).filter(|head| head.contains(".md §") && !head.contains(':')).map(str::to_owned).collect()
    }

    /// §55 (a) 5 形の全部を持つ本文で § の塊が指された順に 4 本（母集団 = 塊の頭を同じ assert で数える）・2 字下げの本文・
    /// 行ごとの done の行。自分の § と、同じ § の 2 形と、`ADR-0001 §4` と `§4.1` は塊を作らず、節の本文の先頭の 1 行と
    /// 自分の § だけを指す本文は外の材料を 1 字も変えない。
    #[test]
    fn linked_section_material_five_forms_bundle_sections_in_pointed_order() {
        let (repo, tracked) = linked_scratch("linked-forms");
        let ws = ["crates/toy/src/inner.rs"];
        let body = "docs/design/s.md#p §1\n本文は §2 と o.md §2 と [他](o.md) §2 と s.md#q と 行 q と o.md の行 r と [o.md §1](o.md) を指し、§1 と 行 p と s.md 行 p と ADR-0001 §4 と §4.1 も書く。";
        let text = text_of(&repo, &tracked, &ws, "docs/design/s.md#p", &[body]);
        assert_eq!(
            section_heads(&text),
            ["- docs/design/s.md §2", "- docs/design/o.md §2", "- docs/design/s.md §3", "- docs/design/o.md §1"],
            "{text}"
        );
        assert_eq!(
            chunk_of(&text, "docs/design/s.md §2"),
            "- docs/design/s.md §2\n  - 二の本文は `ZqShape` と `ZqTwin` と o.md §2 を名指す。\n    二の 2 行目。",
            "{text}"
        );
        assert_eq!(chunk_of(&text, "docs/design/s.md §3"), "- docs/design/s.md §3\n  三の本文。\n  行 q の done: q の done", "{text}");
        assert_eq!(chunk_of(&text, "docs/design/o.md §1"), "- docs/design/o.md §1\n  一の本文。\n  行 r の done: r の done", "{text}");
        assert!(!text.contains("解けない参照") && !text.contains(&long_line()), "{text}");
        let own = text_of(&repo, &tracked, &ws, "docs/design/s.md#p", &["docs/design/s.md#p §1\n本文は §1 と 行 p だけを指す。"]);
        assert_eq!(own, text_of(&repo, &tracked, &ws, "docs/design/s.md#p", &[]), "自分の § だけなら不変");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// §55 (b) tracked に無い doc の §・無い §・表に無い行 id（同じ doc と別の doc）は、正規化した字面を現れた順に重複なく
    /// 並べた 1 塊になり、既存の塊の後ろ・§ の塊の前に置かれる。
    #[test]
    fn linked_section_material_unresolved_references_form_one_chunk_before_sections() {
        let (repo, tracked) = linked_scratch("linked-unresolved");
        let ws = ["crates/toy/src/inner.rs"];
        let body = "x.md §1 と §9 と 行 zz と o.md 行 yy と §9 と o.md §1 を指す。";
        let text = text_of(&repo, &tracked, &ws, "docs/design/s.md#p", &[body]);
        let base = text_of(&repo, &tracked, &ws, "docs/design/s.md#p", &["o.md"]);
        assert_eq!(text, format!("{base}\n- 解けない参照: x.md §1, §9, 行 zz, o.md 行 yy\n- docs/design/o.md §1\n  一の本文。"), "{text}");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// §55 (c) 1 段: 束ねた § だけが名指す名（`ZqShape`）の塊は § の塊の後ろ、契約の本文も名指す `ZqTwin` の塊は既存の
    /// 位置に 1 本だけで、束ねた § が指す別の doc の §2 の塊とその本文だけが名指す `zq_area` の塊は無い。
    #[test]
    fn linked_section_material_names_in_bundled_sections_go_one_level_deep() {
        let (repo, tracked) = linked_scratch("linked-names");
        let text = text_of(&repo, &tracked, &["crates/toy/src/inner.rs"], "docs/design/s.md#p", &["docs/design/s.md#p §1\n本文は §2 を指し ZqTwin を読む。"]);
        let heads: Vec<String> = chunks(&text).iter().map(|chunk| chunk.lines().next().unwrap_or_default().to_owned()).collect();
        let at = |head: &str| heads.iter().position(|found| found.starts_with(head)).unwrap_or(usize::MAX);
        let section = at("- docs/design/s.md §2");
        assert!(section < heads.len() && section < at("- ZqShape: crates/other/src/shape.rs:5"), "{heads:?}");
        assert!(at("- ZqShape") < heads.len(), "{heads:?}");
        assert_eq!(heads.iter().filter(|head| head.starts_with("- ZqTwin")).count(), 1, "{heads:?}");
        assert!(at("- ZqTwin") < section, "{heads:?}");
        assert!(at("- docs/design/o.md §2") == usize::MAX && at("- zq_area") == usize::MAX, "{heads:?}");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// §55 (d) 残りが足りない周は長い § の塊が切り詰めの 1 行になり、解けない参照の塊と小さい § の塊は残り、並びは
    /// 解けない参照 → 切り詰めの行 → 小さい § の塊。説明の 1 行が (g) → (h) → (i) の並びを名乗る。
    #[test]
    fn linked_section_material_long_section_is_truncated_while_small_chunks_stay() {
        let (repo, tracked) = linked_scratch("linked-cap");
        let text = text_of(&repo, &tracked, &["crates/toy/src/inner.rs"], "docs/design/s.md#p", &["x.md §1 と §4 と o.md §1 を指す。"]);
        let long = chunk_of(&text, "docs/design/s.md §4");
        assert!(long.contains(&long_line()), "{text}");
        let full = outside_block(&text, u64::MAX);
        let room = u64::try_from(full.len()).unwrap_or(u64::MAX).saturating_sub(1000);
        let block = outside_block(&text, room);
        let lines: Vec<&str> = block.lines().collect();
        let at = |head: &str| lines.iter().position(|line| line.starts_with(head)).unwrap_or(usize::MAX);
        let short = format!("- docs/design/s.md §4: 切り詰めた（塊 {} byte が cap の残りに収まらない）", long.len());
        assert!(block.contains(&short) && block.contains(&chunk_of(&text, "docs/design/o.md §1")), "{block}");
        assert!(at("- 解けない参照: x.md §1") < at(&short) && at(&short) < at("- docs/design/o.md §1"), "{block}");
        assert!(at("- docs/design/o.md §1") < lines.len() && !block.contains(&long_line()) && !block.contains("落とした名"), "{block}");
        assert_eq!(lines.get(2), Some(&PREAMBLE), "{block}");
        let order: Vec<usize> = ["解けない参照の 1 塊", "解けた § の塊", "名指す名の塊"].iter().filter_map(|word| PREAMBLE.find(word)).collect();
        assert!(order.len() == 3 && order.windows(2).all(|pair| pair.first() < pair.get(1)), "{PREAMBLE}");
        let _ = std::fs::remove_dir_all(&repo);
    }
}
