//! `ledger-plan`: 全設計 doc の契約表の**未着地の行**から `bd create --graph` の plan JSON を出す
//! （設計 docs/design/ledger-form.md §3 の 5・行 b・SRS FR47）。
//!
//! 未着地 = 行が新設を宣言した file（write-set の `+`・`creates`・約束の行の `files` の `+`・
//! `symbols` の `+` の path 形）のどれかが tracked に無い行。新設の宣言を 1 つも持たない行は
//! 着地を字面で判じられないので plan に載せない（台帳を読まずに「未着地」と言わない）。
//! plan の 1 件は title・引数の epic id の parent・`doc:` / `size:` の label を行から写し、`depends` を
//! `blocks` の edge に、§ の本文が名指す bead の id（epic id と同じ prefix の字面）を `discovered-from` の
//! edge に写す。node と edge の key は bd 1.1.0 の graph schema の field（[`NODE_KEYS`] / [`EDGE_KEYS`]・§9）。
//! schema に無い acceptance の pointer 行は、plan の次の行から「plan の key TAB pointer 行」の対応表で出す。
//! `--skip` が名指した契約 id の行は plan から外す（plan に無い id は断る）。
//!
//! **台帳は 1 度も読まず書かない**（bd を起こさない・`.beads/` を開かない）。起こす子 process は
//! tracked の列挙の `git ls-files` 1 本だけ。apply は席の手番（bdw で撃つ）で、台帳側の drift は
//! doctor の lint（行 a の (vii)）が測る。出力は標準出力の 1 回の書き（1 行目が plan の JSON・2 行目以降が対応表）。

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// 使い方の 1 行（epic id は必須・既定に倒さない）。
const USAGE: &str = "usage: cargo xtask ledger-plan --epic <epic id> [--skip <contract id>[,<contract id>...]]... [ROOT]";
/// plan の node が持つ key（bd 1.1.0 の graph schema の node の field の部分集合・閉じた集合）。
const NODE_KEYS: [&str; 5] = ["key", "title", "type", "labels", "parent_id"];
/// plan の edge が持つ key（`to_key` = plan の中の行・`to_id` = 台帳の既存 bead のどちらか 1 つ）。
const EDGE_KEYS: [&str; 4] = ["from_key", "to_key", "to_id", "type"];
/// 設計 doc の置き場（直下の `.md` だけを読む）。
const DESIGN_DIR: &str = "docs/design/";
/// 契約表の区間の始まり（core の `pipe/table.rs` と同じ字面）。
const BEGIN: &str = "<!-- contracts:begin -->";
/// 契約表の区間の終わり。
const END: &str = "<!-- contracts:end -->";

/// 契約表の 1 行のうち plan が読む欄。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Row {
    /// 行 id。
    id: String,
    /// 行の title（bead の title へ写す）。
    title: String,
    /// 節番号（`## N.` の N）。
    section: String,
    /// S / M / L。
    size: String,
    /// 同 doc の行 id の列。
    depends: Vec<String>,
    /// 新設を宣言した repo 相対 path（tracked の path と等しければ在る）。
    new_files: Vec<String>,
    /// `symbols` の `+` の path 形（tracked の path の末尾に `/` 区切りで在れば在る）。
    new_names: Vec<String>,
}

/// plan の 1 件（bead 1 つ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Node {
    /// plan の中の key（契約 id `<doc id>#<行 id>`）。
    pub(crate) key: String,
    /// bead の title。
    pub(crate) title: String,
    /// acceptance の pointer 行（`design = <path>#<id>`・graph schema に無いので JSON に載せず対応表に出す）。
    pub(crate) pointer: String,
    /// label の列（`doc:<doc id>`・`size:<size>`）。
    pub(crate) labels: Vec<String>,
}

/// plan の edge 1 本（`from` が `to` に依る）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Edge {
    /// 依る側の key。
    pub(crate) from: String,
    /// 依られる側（plan の key か台帳の bead id の字面）。
    pub(crate) to: String,
    /// `blocks` か `discovered-from`。
    pub(crate) kind: &'static str,
}

/// `bd create --graph` へ渡す plan。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    /// 全件の parent（引数の epic id）。
    pub(crate) epic: String,
    /// 未着地の行の bead（doc の順・行の順）。
    pub(crate) nodes: Vec<Node>,
    /// edge（node の順）。
    pub(crate) edges: Vec<Edge>,
}

/// 口の引数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Args {
    /// 全件の parent（必須）。
    pub(crate) epic: String,
    /// plan から外す契約 id（`<doc id>#<行 id>`・既定は空）。
    pub(crate) skip: Vec<String>,
    /// repo の root（省略時は cwd）。
    pub(crate) root: Option<String>,
}

/// 引数を読む: `--epic <id>`（必須）・`--skip <契約 id の , 区切り>`（0 回以上）・任意の `ROOT` 1 つ。
pub(crate) fn parse_args(args: &[String]) -> Result<Args, String> {
    let (mut epic, mut root, mut skip) = (None, None, Vec::new());
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--epic" => match rest.next() {
                Some(value) if !value.starts_with("--") && !value.trim().is_empty() => epic = Some(value.clone()),
                _ => return Err(format!("--epic に値が無い\n{USAGE}")),
            },
            "--skip" => match rest.next() {
                Some(value) if !value.starts_with("--") && value.split(',').all(|id| !id.trim().is_empty()) => {
                    skip.extend(value.split(',').map(|id| id.trim().to_owned()));
                }
                _ => return Err(format!("--skip に値が無い\n{USAGE}")),
            },
            other if other.starts_with("--") || root.is_some() => return Err(format!("知らない引数 {other}\n{USAGE}")),
            other => root = Some(other.to_owned()),
        }
    }
    let epic = epic.ok_or_else(|| format!("epic id の引数が無い（plan を出さない）\n{USAGE}"))?;
    id_prefix(&epic).ok_or_else(|| format!("epic id {epic} が bead id の形（<prefix>-<hash>）でない\n{USAGE}"))?;
    Ok(Args { epic, skip, root })
}

/// `ledger-plan` subcommand。plan の JSON と対応表を標準出力へ 1 回で出す。
pub(crate) fn run(args: &[String]) -> Result<ExitCode, String> {
    let args = parse_args(args)?;
    let root = crate::resolve_root(args.root.as_deref())?;
    let plan = skip(plan_at(&root, &args.epic, None)?, &args.skip)?;
    crate::emit(&render(&plan));
    Ok(ExitCode::SUCCESS)
}

/// `ids` の契約 id の行と、その行に触れる edge を plan から外す。plan に無い id が 1 つでも在れば
/// 何も外さず Err（rc 1・知らない id を黙って読み捨てない）。
pub(crate) fn skip(mut plan: Plan, ids: &[String]) -> Result<Plan, String> {
    if let Some(id) = ids.iter().find(|id| !plan.nodes.iter().any(|node| &node.key == *id)) {
        return Err(format!("--skip の {id} が plan に無い（未着地の行の契約 id でない）\n{USAGE}"));
    }
    plan.nodes.retain(|node| !ids.contains(&node.key));
    plan.edges.retain(|edge| !ids.contains(&edge.from) && !ids.contains(&edge.to));
    Ok(plan)
}

/// repo `root` の tracked な設計 doc を読んで plan を組む。`path_var` は子 process（git）の `PATH`
/// （`None` = 継承）。
pub(crate) fn plan_at(root: &Path, epic: &str, path_var: Option<&OsStr>) -> Result<Plan, String> {
    let tracked = tracked_files(root, path_var)?;
    let mut docs = Vec::new();
    for rel in tracked.iter().filter(|rel| is_design_doc(rel)) {
        let full: PathBuf = root.join(rel);
        let text = std::fs::read_to_string(&full).map_err(|err| format!("{rel} を読めない: {err}"))?;
        docs.push((rel.clone(), text));
    }
    build(&docs, &tracked, epic)
}

/// `docs/design/` 直下の `.md` か。
fn is_design_doc(rel: &str) -> bool {
    rel.strip_prefix(DESIGN_DIR).is_some_and(|name| !name.contains('/') && name.ends_with(".md"))
}

/// tracked の path の集合（`git ls-files -z`・読めない周は Err で止める＝0 件に倒さない）。
fn tracked_files(root: &Path, path_var: Option<&OsStr>) -> Result<BTreeSet<String>, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(["ls-files", "-z"]);
    if let Some(path) = path_var {
        command.env("PATH", path);
    }
    let output = command.output().map_err(|err| format!("git ls-files を起動できない: {err}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files が rc≠0: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect())
}

/// doc の列（repo 相対 path・本文）と tracked の集合から plan を組む（pure）。
pub(crate) fn build(docs: &[(String, String)], tracked: &BTreeSet<String>, epic: &str) -> Result<Plan, String> {
    let prefix = id_prefix(epic).ok_or_else(|| format!("epic id {epic} が bead id の形でない"))?;
    let mut plan = Plan { epic: epic.to_owned(), nodes: Vec::new(), edges: Vec::new() };
    let mut pending = Vec::new();
    for (path, text) in docs {
        let doc = doc_id(path);
        for row in rows_of(text).map_err(|reason| format!("{path}: {reason}"))? {
            if landed(&row, tracked) != Some(false) {
                continue;
            }
            let body = section_body(text, &row.section)
                .ok_or_else(|| format!("{path}#{}: § {} の見出しが無い", row.id, row.section))?;
            let key = format!("{doc}#{}", row.id);
            let mut labels = vec![format!("doc:{doc}")];
            if !row.size.is_empty() {
                labels.push(format!("size:{}", row.size));
            }
            plan.nodes.push(Node {
                key: key.clone(),
                title: row.title.clone(),
                pointer: format!("design = {path}#{}", row.id),
                labels,
            });
            pending.push((key, doc.clone(), row.depends.clone(), bead_ids(&body, prefix)));
        }
    }
    let keys: BTreeSet<String> = plan.nodes.iter().map(|node| node.key.clone()).collect();
    for (key, doc, depends, named) in pending {
        // 着地済みの行への depends は既に閉じた bead への依存なので edge にしない（plan の外の key を作らない）。
        for dep in depends.iter().map(|dep| format!("{doc}#{dep}")).filter(|dep| keys.contains(dep)) {
            plan.edges.push(Edge { from: key.clone(), to: dep, kind: "blocks" });
        }
        for id in named.into_iter().filter(|id| id != epic) {
            plan.edges.push(Edge { from: key.clone(), to: id, kind: "discovered-from" });
        }
    }
    Ok(plan)
}

/// doc id（file 名の stem）。
fn doc_id(path: &str) -> String {
    Path::new(path).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default()
}

/// bead id の prefix（`s2-07l.515` → `s2`）。`-` の無い字面は `None`。
fn id_prefix(epic: &str) -> Option<&str> {
    let head = epic.split('.').next().unwrap_or(epic);
    let (prefix, hash) = head.rsplit_once('-')?;
    (!prefix.is_empty() && !hash.is_empty()).then_some(prefix)
}

/// 行が着地したか。新設の宣言が 1 つも無い行は `None`（判じられない）・1 つでも tracked に無ければ `Some(false)`。
fn landed(row: &Row, tracked: &BTreeSet<String>) -> Option<bool> {
    if row.new_files.is_empty() && row.new_names.is_empty() {
        return None;
    }
    let files = row.new_files.iter().all(|file| tracked.contains(file));
    let names = row
        .new_names
        .iter()
        .all(|name| tracked.iter().any(|path| path == name || path.ends_with(&format!("/{name}"))));
    Some(files && names)
}

/// 契約表の区間を抜く（区間が無い doc は `None`）。
fn region(text: &str) -> Option<String> {
    let mut body: Option<String> = None;
    for line in text.lines() {
        match (line.trim(), body.as_mut()) {
            (BEGIN, None) => body = Some(String::new()),
            (END, Some(_)) => return body,
            (_, Some(found)) => {
                found.push_str(line);
                found.push('\n');
            }
            (_, None) => {}
        }
    }
    None
}

/// 契約表の行の header。
const CONTRACT: &str = "[[contract]]";
/// 約束の行の header（contract-source.md §33）。
const PROMISE: &str = "[[promise]]";

/// 区間を header（[`CONTRACT`] / [`PROMISE`]）ごとの `key = value` の列に束ねる（header より前の行は捨てる）。
fn blocks(body: &str) -> Vec<(&str, Vec<(&str, &str)>)> {
    let mut found: Vec<(&str, Vec<(&str, &str)>)> = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed == CONTRACT || trimmed == PROMISE {
            found.push((trimmed, Vec::new()));
            continue;
        }
        if let (Some(pair), Some((_, pairs))) = (crate::toml_lite::key_value(line), found.last_mut()) {
            pairs.push(pair);
        }
    }
    found
}

/// 区間の `[[contract]]` と `[[promise]]` を読み、約束の行の新設の宣言を親の行へ畳む。
fn rows_of(text: &str) -> Result<Vec<Row>, String> {
    let Some(body) = region(text) else {
        return Ok(Vec::new());
    };
    let found = blocks(&body);
    let mut rows: Vec<Row> =
        found.iter().filter(|(kind, _)| *kind == CONTRACT).map(|(_, pairs)| contract_row(pairs)).collect();
    for (_, pairs) in found.iter().filter(|(kind, _)| *kind == PROMISE) {
        fold_promise(&mut rows, pairs)?;
    }
    if let Some(row) = rows.iter().find(|row| row.id.is_empty() || row.title.is_empty()) {
        return Err(format!("id か title の無い行が在る（id = {:?}）", row.id));
    }
    Ok(rows)
}

/// `[[contract]]` の 1 行を読む。
fn contract_row(pairs: &[(&str, &str)]) -> Row {
    let mut row = Row::default();
    for (key, value) in pairs {
        let text = || crate::toml_lite::quoted(value).unwrap_or_default();
        match *key {
            "id" => row.id = text(),
            "title" => row.title = text(),
            "section" => row.section = text(),
            "size" => row.size = text(),
            "depends" => row.depends = strings(value),
            "write-set" => row.new_files.extend(plus_items(&strings(value))),
            "creates" => row.new_files.extend(strings(value)),
            _ => {}
        }
    }
    row
}

/// `[[promise]]` の `files` の `+` と `symbols` の `+` の path 形を、`of` の親の行の新設の宣言へ足す。
fn fold_promise(rows: &mut [Row], pairs: &[(&str, &str)]) -> Result<(), String> {
    let of = pairs
        .iter()
        .find(|(key, _)| *key == "of")
        .and_then(|(_, value)| crate::toml_lite::quoted(value))
        .unwrap_or_default();
    let row = rows
        .iter_mut()
        .find(|row| row.id == of)
        .ok_or_else(|| format!("約束の行の of = {of:?} が同じ doc の行に無い"))?;
    for (key, value) in pairs {
        match *key {
            "files" => row.new_files.extend(plus_items(&strings(value))),
            "symbols" => row.new_names.extend(plus_items(&strings(value)).into_iter().filter(|name| is_path_form(name))),
            _ => {}
        }
    }
    Ok(())
}

/// `["a", "b"]` の各要素（`"` の対の中身・`,` を含む要素も割らない）。
fn strings(value: &str) -> Vec<String> {
    value.split('"').skip(1).step_by(2).map(str::to_owned).collect()
}

/// `+` の項目だけを接頭辞を剥がして返す。
fn plus_items(items: &[String]) -> Vec<String> {
    items.iter().filter_map(|item| item.strip_prefix('+')).map(str::to_owned).collect()
}

/// 名指しの path 形（英数字と `_ . / -` だけで `.rs` で終わる）か。
fn is_path_form(name: &str) -> bool {
    name.ends_with(".rs") && name.chars().all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c))
}

/// 節 `## N.` の本文（次の `## ` の見出しか契約表の区間の手前まで）。見出しが無ければ `None`。
fn section_body(text: &str, section: &str) -> Option<String> {
    let heading = format!("## {section}.");
    let mut lines = text.lines().skip_while(|line| !line.starts_with(&heading));
    lines.next()?;
    let body: Vec<&str> = lines.take_while(|line| !line.starts_with("## ") && line.trim() != BEGIN).collect();
    Some(body.join("\n"))
}

/// 本文が名指す bead id の字面（`<prefix>-<英数字>` に `.<数字>` の続き・前後が識別子の字でない）を出現順に重複なく。
fn bead_ids(text: &str, prefix: &str) -> Vec<String> {
    let needle = format!("{prefix}-");
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.';
    let mut found: Vec<String> = Vec::new();
    for (at, _) in text.match_indices(&needle) {
        if text.get(..at).and_then(|head| head.chars().next_back()).is_some_and(is_word) {
            continue;
        }
        let rest = text.get(at + needle.len()..).unwrap_or_default();
        let Some(tail) = id_tail_len(rest).and_then(|end| rest.get(..end)) else {
            continue;
        };
        let id = format!("{needle}{tail}");
        if !found.contains(&id) {
            found.push(id);
        }
    }
    found
}

/// prefix の後ろの id の長さ（英数字の hash に `.<数字>` の子の番号が続き、直後が識別子の字でない）。
fn id_tail_len(rest: &str) -> Option<usize> {
    let hash = rest.chars().take_while(char::is_ascii_alphanumeric).count();
    if hash == 0 {
        return None;
    }
    let mut end = hash;
    // 文末の `.` は読まない（数字が続く `.` だけが子の番号）。
    while let Some(tail) = rest.get(end..).and_then(|tail| tail.strip_prefix('.')) {
        let digits = tail.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            break;
        }
        end += 1 + digits;
    }
    let next = rest.get(end..).and_then(|tail| tail.chars().next());
    (!next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(end)
}

/// JSON の文字列 literal。
fn json_str(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// JSON の object（key は字面の順・値は描き済みの JSON）。
fn json_object(pairs: &[(&str, String)]) -> String {
    let fields: Vec<String> = pairs.iter().map(|(key, value)| format!("{}:{value}", json_str(key))).collect();
    format!("{{{}}}", fields.join(","))
}

/// plan を描く: 1 行目が `bd create --graph` の JSON 1 つ、2 行目以降が node ごとの対応表
/// （`<plan の key>\t<acceptance の pointer 行>`・node の順）。
pub(crate) fn render(plan: &Plan) -> String {
    let [key, title, kind, labels, parent_id] = NODE_KEYS;
    let nodes: Vec<String> = plan
        .nodes
        .iter()
        .map(|node| {
            let label_list: Vec<String> = node.labels.iter().map(|label| json_str(label)).collect();
            json_object(&[
                (key, json_str(&node.key)),
                (title, json_str(&node.title)),
                (kind, json_str("task")),
                (labels, format!("[{}]", label_list.join(","))),
                (parent_id, json_str(&plan.epic)),
            ])
        })
        .collect();
    let [from_key, to_key, to_id, edge_kind] = EDGE_KEYS;
    let edges: Vec<String> = plan
        .edges
        .iter()
        .map(|edge| {
            // 依られる側が plan の中の行なら key で、それ以外（台帳の既存 bead の id の字面）は id で指す。
            let to = if plan.nodes.iter().any(|node| node.key == edge.to) { to_key } else { to_id };
            json_object(&[(from_key, json_str(&edge.from)), (to, json_str(&edge.to)), (edge_kind, json_str(edge.kind))])
        })
        .collect();
    let mut out = format!("{{\"nodes\":[{}],\"edges\":[{}]}}", nodes.join(","), edges.join(","));
    for node in &plan.nodes {
        out.push_str(&format!("\n{}\t{}", node.key, node.pointer));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{build, parse_args, plan_at, render, skip, Args, Edge, USAGE};
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// 実在しない prefix の epic（fixture の字面が実台帳の id と衝突しない）。
    const EPIC: &str = "zq9-e7k";

    /// 契約表の行 1 つ。
    fn row(id: &str, section: &str, write_set: &str, extra: &str) -> String {
        format!(
            "[[contract]]\nid = \"{id}\"\ntitle = \"probe-title-{id}, with comma\"\nreq = [\"FR1\"]\nsection = \"{section}\"\nwrite-set = [{write_set}]\nverify = [\"cargo nextest run -p x probe_\"]\nsize = \"M\"\ndone = \"(1) d\"\n{extra}\n"
        )
    }

    /// 節 2 つと契約表を持つ toy の設計 doc。
    fn doc(rows: &[String]) -> String {
        format!(
            "# t\n\n## 1. one\n本文は zq9-e7k.41 と `zq9-e7k.7` と zq9-abc を名指す。親は zq9-e7k。xzq9-e7k.9 と zq9-e7k.5x は名指しでない。\n\n## 2. two\n名指し無し。\n\n## 12. twelve\nzq9-e7k.99 は § 1 の本文ではない。\n\n<!-- contracts:begin -->\nschema = 1\n\n{}<!-- contracts:end -->\n",
            rows.concat()
        )
    }

    fn tracked(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    /// plan の key の列（組めない周は理由 1 つの列＝期待の key と一致しない）。
    fn keys(docs: &[(String, String)], present: &BTreeSet<String>) -> Vec<String> {
        build(docs, present, EPIC)
            .map(|plan| plan.nodes.into_iter().map(|node| node.key).collect())
            .unwrap_or_else(|reason| vec![reason])
    }

    /// (a) `+` の file が tracked に無い行だけが載り、在る行（着地済み）と新設の宣言が無い行は載らない。
    /// `creates` と約束の行の `files` / `symbols` の `+` も同じ読み。
    #[test]
    fn ledger_plan_lists_only_rows_whose_new_files_are_untracked() {
        let rows = vec![
            row("a", "1", "\"+src/landed.rs\", \"src/main.rs\"", ""),
            row("b", "1", "\"+src/fresh.rs\", \"src/main.rs\"", ""),
            row("c", "2", "\"src/main.rs\"", ""),
            row("d", "2", "\"+src/landed.rs\", \"+src/half.rs\"", ""),
            row("e", "2", "\"src/main.rs\"", "creates = [\"src/made.rs\"]"),
            row("f", "2", "\"src/main.rs\"", "[[promise]]\nof = \"f\"\nfiles = [\"+src/promised.rs\"]\nsymbols = [\"+deep/named.rs\", \"+crate::x::Y\"]"),
        ];
        let docs = vec![("docs/design/probe.md".to_owned(), doc(&rows))];
        let present = tracked(&["src/landed.rs", "src/main.rs", "docs/design/probe.md"]);
        assert_eq!(keys(&docs, &present), ["probe#b", "probe#d", "probe#e", "probe#f"], "未着地の行だけ・doc の順");
        // 着地すると載らない（行 e の creates・行 f の files と symbols の path 形は tracked の末尾で解ける）。
        let landed = tracked(&["src/landed.rs", "src/main.rs", "src/fresh.rs", "src/half.rs", "src/made.rs", "src/promised.rs", "crates/k/src/deep/named.rs"]);
        assert!(keys(&docs, &landed).is_empty(), "全部着地した表は plan 0 件");
        // symbols の path 形だけが欠けた行 f は未着地。
        let unnamed = tracked(&["src/landed.rs", "src/main.rs", "src/fresh.rs", "src/half.rs", "src/made.rs", "src/promised.rs"]);
        assert_eq!(keys(&docs, &unnamed), ["probe#f"]);
    }

    /// (b) `depends` が `blocks` に・§ の本文が名指す bead id が `discovered-from` に写り、名指しの無い行は edge 0。
    #[test]
    fn ledger_plan_maps_depends_to_blocks_and_named_ids_to_discovered_from() {
        let rows = vec![
            row("a", "2", "\"+src/landed.rs\"", ""),
            row("b", "1", "\"+src/b.rs\"", "depends = [\"c\", \"a\"]"),
            row("c", "2", "\"+src/c.rs\"", ""),
        ];
        let docs = vec![("docs/design/probe.md".to_owned(), doc(&rows))];
        let plan = build(&docs, &tracked(&["src/landed.rs"]), EPIC).expect("plan");
        let edge = |from: &str, to: &str, kind: &'static str| Edge { from: from.to_owned(), to: to.to_owned(), kind };
        assert_eq!(
            plan.edges,
            [
                edge("probe#b", "probe#c", "blocks"),
                edge("probe#b", "zq9-e7k.41", "discovered-from"),
                edge("probe#b", "zq9-e7k.7", "discovered-from"),
                edge("probe#b", "zq9-abc", "discovered-from"),
            ],
            "着地済みの a への depends・epic 自身・§ 12 の id・識別子の途中の字面は edge にならない"
        );
        assert!(plan.edges.iter().all(|edge| edge.from != "probe#c"), "名指しの無い行 c は edge 0");
    }

    /// (c) title・parent・label が行から写り、1 行目は JSON 1 つ・pointer 行は 2 行目の対応表。
    #[test]
    fn ledger_plan_copies_title_pointer_parent_and_labels_into_one_json() {
        let rows = vec![row("b", "2", "\"+src/b.rs\"", "")];
        let docs = vec![("docs/design/probe-doc.md".to_owned(), doc(&rows))];
        let plan = build(&docs, &tracked(&[]), EPIC).expect("plan");
        let out = render(&plan);
        let json = out.lines().next().unwrap_or_default();
        assert_eq!(
            out,
            "{\"nodes\":[{\"key\":\"probe-doc#b\",\"title\":\"probe-title-b, with comma\",\"type\":\"task\",\"labels\":[\"doc:probe-doc\",\"size:M\"],\"parent_id\":\"zq9-e7k\"}],\"edges\":[]}\nprobe-doc#b\tdesign = docs/design/probe-doc.md#b"
        );
        assert!(one_json_value(json), "JSON の値 1 つ: {json}");
        let quoted = build(&[("docs/design/q.md".to_owned(), doc(&[row("b", "2", "\"+x.rs\"", "").replace("with comma", "with \\\\ back")]))], &tracked(&[]), EPIC).expect("plan");
        assert!(render(&quoted).contains("with \\\\\\\\ back"), "文字列は escape される: {}", render(&quoted));
    }

    /// (d) epic id の引数が無い・値が無い・bead id の形でない周は plan を出さず rc 1。
    #[test]
    fn ledger_plan_refuses_without_epic() {
        let args = |line: &str| -> Vec<String> { line.split_whitespace().map(str::to_owned).collect() };
        assert!(parse_args(&args("")).is_err(), "引数なし");
        assert!(parse_args(&args("/tmp/root")).is_err(), "ROOT だけ");
        assert!(parse_args(&args("--epic")).is_err(), "値なし");
        assert!(parse_args(&args("--epic --x")).is_err(), "次の flag を値に読まない");
        assert!(parse_args(&args("--epic nohyphen")).is_err(), "bead id の形でない");
        assert_eq!(
            parse_args(&args("--epic zq9-e7k /r")),
            Ok(Args { epic: "zq9-e7k".to_owned(), skip: Vec::new(), root: Some("/r".to_owned()) })
        );
        assert_eq!(super::run(&args("")).map(|_| ()).map_err(|reason| reason.contains("epic")), Err(true), "Err = rc 1");
        assert!(build(&[], &tracked(&[]), "nohyphen").is_err(), "build も epic の形を要る");
    }

    /// (e) 台帳を 1 度も読まない: PATH の先頭の偽の bd は 1 回も呼ばれず、toy repo の plan が出る。
    #[test]
    fn ledger_plan_never_runs_bd_even_when_it_is_first_on_path() {
        let base = tmp_dir();
        let repo = base.join("repo");
        let bin = base.join("bin");
        let marker = base.join("bd-called");
        fs::create_dir_all(repo.join("docs/design")).expect("mkdir");
        fs::create_dir_all(&bin).expect("mkdir");
        let fake = bin.join("bd");
        fs::write(&fake, format!("#!/bin/sh\necho called >> '{}'\n", marker.display())).expect("fake bd");
        assert!(make_executable(&fake), "chmod");
        fs::write(repo.join("docs/design/probe.md"), doc(&[row("b", "1", "\"+src/b.rs\"", "")])).expect("doc");
        fs::write(repo.join("docs/design/notes.txt"), doc(&[row("z", "1", "\"+src/z.rs\"", "")])).expect("非 md");
        assert!(git(&repo, &["init", "-q"]) && git(&repo, &["add", "."]), "toy repo");
        let path = std::env::join_paths(std::iter::once(bin.clone()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())))
            .expect("PATH");
        let plan = plan_at(&repo, EPIC, Some(&path)).expect("plan");
        assert_eq!(plan.nodes.iter().map(|node| node.key.as_str()).collect::<Vec<_>>(), ["probe#b"], "tracked の設計 doc だけ");
        assert!(!marker.exists(), "偽の bd が呼ばれた");
        // 偽の bd 自体は PATH から起こせる（marker が書ける）ことを確かめ、上の不在が空虚でないことを示す。
        let called = Command::new("bd").env("PATH", &path).status().map(|status| status.success()).unwrap_or(false);
        assert!(called && marker.exists(), "偽の bd は PATH の先頭で解ける");
        let _ = fs::remove_dir_all(&base);
    }

    /// §9 の歯の fixture: 行 b（c に依り § 1 の id を名指す）・c・d（§ 2・edge 無し）が未着地。
    fn schema_plan() -> super::Plan {
        let rows = vec![
            row("b", "1", "\"+src/b.rs\"", "depends = [\"c\"]"),
            row("c", "2", "\"+src/c.rs\"", ""),
            row("d", "2", "\"+src/d.rs\"", ""),
        ];
        build(&[("docs/design/probe.md".to_owned(), doc(&rows))], &tracked(&[]), EPIC).expect("plan")
    }

    /// JSON の各 object の key の集合（閉じた順・文字列の値は key に数えない）。
    fn object_keys(json: &str) -> Vec<BTreeSet<String>> {
        let (mut stack, mut found): (Vec<BTreeSet<String>>, Vec<BTreeSet<String>>) = (Vec::new(), Vec::new());
        let (mut in_str, mut escaped, mut current, mut last_str) = (false, false, String::new(), None::<String>);
        for c in json.chars() {
            if in_str {
                match (escaped, c) {
                    (true, _) => {
                        escaped = false;
                        current.push(c);
                    }
                    (false, '\\') => escaped = true,
                    (false, '"') => {
                        in_str = false;
                        last_str = Some(std::mem::take(&mut current));
                    }
                    _ => current.push(c),
                }
                continue;
            }
            match c {
                '"' => in_str = true,
                ':' => {
                    if let (Some(key), Some(top)) = (last_str.take(), stack.last_mut()) {
                        top.insert(key);
                    }
                }
                '{' => stack.push(BTreeSet::new()),
                '}' => found.extend(stack.pop()),
                _ if c.is_whitespace() => {}
                _ => last_str = None,
            }
        }
        found
    }

    /// §9 形 1: node の key は key / title / type / description / labels / parent_id に、edge の key は
    /// from_key と to_key か to_id と type に閉じ、acceptance / parent / from / to は key としてどこにも無い。
    #[test]
    fn ledger_plan_renders_the_bd_graph_schema_field_names() {
        let plan = schema_plan();
        let out = render(&plan);
        let json = out.lines().next().unwrap_or_default();
        let objects = object_keys(json);
        let set = |keys: &[&str]| -> BTreeSet<String> { keys.iter().map(|key| (*key).to_owned()).collect() };
        let node_allowed = set(&["key", "title", "type", "description", "labels", "parent_id"]);
        let (nodes, rest): (Vec<_>, Vec<_>) = objects.iter().partition(|keys| keys.contains("title"));
        let (top, edges): (Vec<_>, Vec<_>) = rest.into_iter().partition(|keys| keys.contains("nodes"));
        assert_eq!(top, [&set(&["nodes", "edges"])], "外側は nodes と edges だけ: {json}");
        assert_eq!(nodes.len(), plan.nodes.len(), "node の object の数: {json}");
        assert_eq!(edges.len(), plan.edges.len(), "edge の object の数: {json}");
        for keys in &nodes {
            assert!(keys.is_subset(&node_allowed) && keys.contains("parent_id") && keys.contains("key"), "node の key: {keys:?}");
        }
        let to_key = set(&["from_key", "to_key", "type"]);
        let to_id = set(&["from_key", "to_id", "type"]);
        assert!(edges.iter().all(|keys| **keys == to_key || **keys == to_id), "edge の key: {edges:?}");
        assert!(edges.contains(&&to_key) && edges.contains(&&to_id), "plan の中の行は to_key・台帳の id は to_id: {json}");
        for banned in ["acceptance", "parent", "from", "to"] {
            assert!(objects.iter().all(|keys| !keys.contains(banned)), "{banned} が key に在る: {json}");
        }
        // 向きと相手の弁別: b → c（plan の中）は to_key、b → 名指しの id は to_id。
        assert!(json.contains("{\"from_key\":\"probe#b\",\"to_key\":\"probe#c\",\"type\":\"blocks\"}"), "{json}");
        assert!(json.contains("{\"from_key\":\"probe#b\",\"to_id\":\"zq9-e7k.41\",\"type\":\"discovered-from\"}"), "{json}");
    }

    /// §9 形 2: 2 行目以降が node と同じ本数の「plan の key TAB pointer 行」で、pointer 行は JSON に無く、
    /// stdout へ書く呼び出しは本体に 1 回だけ。
    #[test]
    fn ledger_plan_emits_the_pointer_line_table_after_the_plan() {
        let plan = schema_plan();
        let out = render(&plan);
        let lines: Vec<&str> = out.lines().collect();
        let table: Vec<Vec<&str>> = lines.iter().skip(1).map(|line| line.split('\t').collect()).collect();
        assert_eq!(
            table,
            [
                ["probe#b", "design = docs/design/probe.md#b"],
                ["probe#c", "design = docs/design/probe.md#c"],
                ["probe#d", "design = docs/design/probe.md#d"],
            ],
            "node と同じ本数・同じ順: {out}"
        );
        assert_eq!(table.len(), plan.nodes.len());
        assert!(!lines.first().is_some_and(|json| json.contains("design = ")), "pointer 行は JSON に載らない: {out}");
        assert!(render(&skip(plan, &["probe#b".to_owned(), "probe#c".to_owned(), "probe#d".to_owned()]).expect("skip")).lines().count() == 1, "node 0 件は JSON の 1 行だけ");
        // stdout の口は emit の 1 関数・呼び出しは run の 1 回（本体の区間で数える）。
        let source = include_str!("ledger_plan.rs");
        let body = source.split(["#[cfg(", "test)]"].concat().as_str()).next().unwrap_or_default();
        assert_eq!(body.matches(["crate::", "emit("].concat().as_str()).count(), 1, "stdout へ書く呼び出しは 1 回");
        assert!(!body.contains(["print", "ln!"].concat().as_str()) && !body.contains(["print", "!("].concat().as_str()), "emit の外で書かない");
    }

    /// §9 形 3: --skip が名指した行だけが消え、残りの行と edge は不変・plan に無い id は rc 1・usage が --skip を写す。
    #[test]
    fn ledger_plan_skips_the_rows_named_by_the_skip_argument() {
        let plan = schema_plan();
        let skipped = skip(plan.clone(), &["probe#d".to_owned()]).expect("skip");
        assert_eq!(skipped.nodes, plan.nodes.iter().filter(|node| node.key != "probe#d").cloned().collect::<Vec<_>>(), "d だけが消える");
        assert_eq!(skipped.edges, plan.edges, "d に触れない edge は不変");
        // 依られる行を外すと、その行に触れる edge だけが消える（plan の外の key を指す edge を残さない）。
        let no_c = skip(plan.clone(), &["probe#c".to_owned()]).expect("skip");
        assert_eq!(no_c.edges, plan.edges.iter().filter(|edge| edge.to != "probe#c").cloned().collect::<Vec<_>>());
        assert_eq!(no_c.nodes.len(), 2);
        assert!(skip(plan.clone(), &[]).is_ok_and(|same| same == plan), "既定は空＝不変");
        assert!(skip(plan.clone(), &["probe#d".to_owned(), "probe#zz".to_owned()]).is_err(), "plan に無い id は断る");
        assert!(skip(plan.clone(), &["zq9-e7k.41".to_owned()]).is_err(), "edge の相手の bead id は契約 id でない");
        let args = |line: &str| -> Vec<String> { line.split_whitespace().map(str::to_owned).collect() };
        assert_eq!(
            parse_args(&args("--epic zq9-e7k --skip a#b,c#d --skip e#f /r")).map(|parsed| parsed.skip),
            Ok(vec!["a#b".to_owned(), "c#d".to_owned(), "e#f".to_owned()])
        );
        assert!(parse_args(&args("--epic zq9-e7k --skip")).is_err(), "値なし");
        assert!(parse_args(&args("--epic zq9-e7k --skip --x")).is_err(), "次の flag を値に読まない");
        assert!(parse_args(&args("--epic zq9-e7k --skip a#b,")).is_err(), "空の id");
        assert!(USAGE.lines().count() == 1 && USAGE.contains("--skip"), "usage の 1 行が --skip を写す");
        run_refuses_unknown_skip_id();
    }

    /// 口の全体: toy repo で plan に無い id は Err（= rc 1）・在る id は通る。
    fn run_refuses_unknown_skip_id() {
        let args = |line: &str| -> Vec<String> { line.split_whitespace().map(str::to_owned).collect() };
        let base = tmp_dir();
        let repo = base.join("repo");
        fs::create_dir_all(repo.join("docs/design")).expect("mkdir");
        fs::write(repo.join("docs/design/probe.md"), doc(&[row("b", "1", "\"+src/b.rs\"", "")])).expect("doc");
        assert!(git(&repo, &["init", "-q"]) && git(&repo, &["add", "."]), "toy repo");
        let root = repo.display().to_string();
        let refused = super::run(&args(&format!("--epic {EPIC} --skip probe#nope {root}")));
        assert!(refused.is_err_and(|reason| reason.contains("probe#nope")), "plan に無い id は rc 1");
        assert!(skip(plan_at(&repo, EPIC, None).expect("plan"), &["probe#b".to_owned()]).is_ok_and(|left| left.nodes.is_empty()));
        let _ = fs::remove_dir_all(&base);
    }

    /// JSON の値がちょうど 1 つか（括弧の対・文字列の中は数えない）。
    fn one_json_value(text: &str) -> bool {
        let (mut depth, mut in_str, mut escaped, mut closed_at) = (0_i64, false, false, None);
        for (at, c) in text.char_indices() {
            if in_str {
                match (escaped, c) {
                    (true, _) => escaped = false,
                    (false, '\\') => escaped = true,
                    (false, '"') => in_str = false,
                    _ => {}
                }
                continue;
            }
            match c {
                '"' => in_str = true,
                '{' | '[' => depth += 1,
                '}' | ']' => {
                    depth -= 1;
                    if depth == 0 && closed_at.is_none() {
                        closed_at = Some(at);
                    }
                }
                _ => {}
            }
        }
        text.starts_with('{') && depth == 0 && !in_str && closed_at == Some(text.len() - 1)
    }

    fn tmp_dir() -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|since| since.subsec_nanos()).unwrap_or(0);
        std::env::temp_dir().join(format!("xtask-ledger-plan-{}-{nanos}", std::process::id()))
    }

    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git").arg("-C").arg(dir).args(args).output().map(|out| out.status.success()).unwrap_or(false)
    }

    fn make_executable(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).is_ok()
    }
}
