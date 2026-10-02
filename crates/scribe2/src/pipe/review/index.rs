//! 逆引きの表と審査の材料 index.txt（設計 docs/design/reverse-index.md §6・§7 (a)・契約表の行 c・FR49 / FR108）。
//!
//! 外の道具が作った索引（[`crate::pipe::index`] の表）と契約表から、項目ごとに 7 列（refs・callers・literals・patterns・teeth・vis・
//! rows）の件数と site と write-set の外の印と母集団（`text=`・`indexed=`・`outside-index=`）を数え（[`count`]）、1 つの字に描く
//! （[`render`]）。口 `pipe index show`（[`show`]）・審査の材料の [`super::INDEX_FILE`]（[`keep`]）・lens の収め（[`index_block`]）が
//! この 2 本を共用する（2 本目の数えを作らない）。項目は symbol へ a1 の問い（`flat::query`）で解き、解けない（0 件）・複数
//! （`ambiguous:<n>`）を名指す。行の節の名指しは名指しの読み手の口 [`section_symbols`] の 1 本で読む（読み手を写さない）。
//!
//! 契約表の行・表の検査の断り・審査の材料の型は組まない: 表の行は `read_table` の返りを field で読む。

use super::base::ITEM_HEAD;
use super::{material_file, section_text, INDEX_FILE};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK};
use crate::fleet::store::LockPolicy;
use crate::invocation::Invocation;
use crate::pipe::cli::{broken, flag, refused, repo_of, state_dir_of};
use crate::pipe::closure::section_symbols;
use crate::pipe::declaration::{TablePlaces, DECL_FILE};
use crate::pipe::dispatch::index_build::{assemble, status, Assembled, How, Status};
use crate::pipe::git_bytes;
use crate::pipe::index::flat::{descriptor_names, query, Resolution, Row, Site};
use crate::pipe::table::{design_docs, read_table};
use crate::rules::manifest::Manifest;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// `{index}` の穴の見出し（項目を落とした周も見出しは残す）。
const HEADING: &str = "\n## 逆引きの表（index.txt・器が外の道具の索引から組んだ事実）\n";

/// 見出しの下の 1 文。
const PREAMBLE: &str = "index.txt は器が外の道具の索引から組んだ事実で、件数の横の母集団（`text=`・`indexed=`・`outside-index=`）を合わせて読む。site は読みの道具で開ける。";

/// 外の印。
const OUTSIDE: &str = "外";

/// 契約表の 1 行（touches・write-set・節だけを読む）。
struct Line {
    /// `<doc>#<行 id>`。
    pointer: String,
    /// doc の path。
    doc: String,
    /// 閉じた型の宣言の列。
    touches: Vec<String>,
    /// 欄 write-set（無い行は空）。
    write_set: Vec<String>,
    /// 節の番号。
    section: String,
    /// 導出物の行の節の逐語（無ければ空）。
    goal: String,
}

/// ref の木の契約表の全行（置き場の宣言と列挙の 1 関数 `design_docs` で絞った doc の行・doc の本文つき・既定は行 0 本＝rows 列が
/// 空の表で、行 e の測りが rows 列を読まずに数える）。
#[derive(Default)]
pub(in crate::pipe) struct Tables {
    /// 全行（doc の順と行の順）。
    lines: Vec<Line>,
    /// doc の path → 本文。
    docs: BTreeMap<String, String>,
}

impl Tables {
    /// ref の木から読む（置き場の宣言を読めない・doc を読めない周は理由）。
    pub(in crate::pipe) fn load(repo: &Path, sha: &str) -> Result<Self, String> {
        let listed = git_bytes(repo, &["ls-tree", "-r", "-z", "--full-tree", "--name-only", sha]).ok_or_else(|| format!("{sha} の木を読めない"))?;
        let tracked: Vec<String> = String::from_utf8_lossy(&listed).split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect();
        let places = TablePlaces::at(repo, sha);
        let items = places.items().ok_or_else(|| format!("{sha} の {DECL_FILE} を読めない（契約表の置き場）"))?;
        let (mut lines, mut docs) = (Vec::new(), BTreeMap::new());
        for doc in design_docs(&tracked, items) {
            let shown = git_bytes(repo, &["show", &format!("{sha}:{doc}")]).and_then(|bytes| String::from_utf8(bytes).ok());
            let shown = shown.ok_or_else(|| format!("{doc} を {sha} から読めない"))?;
            let (found, _) = read_table(doc, &shown).map_err(|errors| {
                format!("{doc} の区間を読めない: {}", errors.first().map(|error| error.reason()).unwrap_or_default())
            })?;
            for row in found {
                let pointer = format!("{doc}#{}", row.id);
                lines.push(Line { pointer, doc: doc.clone(), touches: row.touches, write_set: row.write_set, section: row.section, goal: row.goal });
            }
            docs.insert(doc.clone(), shown);
        }
        Ok(Self { lines, docs })
    }

    /// 行 `pointer` の節の散文（導出物の行は goal・無ければ節の番号の本文）。
    fn prose(&self, line: &Line) -> String {
        if !line.goal.is_empty() {
            return line.goal.clone();
        }
        self.docs.get(&line.doc).map(|text| section_text(text, &line.section)).unwrap_or_default()
    }
}

/// 数えの入力（読めた表・repo と commit・契約表）。
pub(in crate::pipe) struct Ctx<'a> {
    /// 読めた索引の表。
    pub rows: &'a [Row],
    /// 対象 repo。
    pub repo: &'a Path,
    /// 索引の commit の sha。
    pub sha: &'a str,
    /// ref の木の契約表。
    pub tables: &'a Tables,
}

/// 外の印の入力（`own` は名指した行の `<doc>#<行 id>`・無ければ空・`marks` は行の欄 write-set で、`None` は印を付けない＝
/// `--item` と欄 write-set を持たない行）。
pub(in crate::pipe) struct Scope<'a> {
    /// 名指した行。
    pub own: &'a str,
    /// 行の write-set。
    pub marks: Option<&'a [String]>,
}

/// site 1 つ（列の中の 1 件）。
struct Hit {
    /// file。
    file: String,
    /// 行（1 始まり）。
    line: usize,
    /// test の中か。
    test: bool,
    /// 添える字（役の語か囲む定義の名）。
    note: String,
    /// 行の write-set の外か。
    outside: bool,
}

/// 項目が解けた形。
enum State {
    /// symbol が 1 つ。
    One(String),
    /// 解けない。
    Unresolved,
    /// 複数（symbol と定義の site）。
    Many(Vec<(String, Vec<String>)>),
}

/// 母集団（項目の最後の節の名が tracked file に語の境界で現れる行の数・うち索引が解いた数・索引の外の file の行の数と path）。
struct Population {
    /// 現れる行の数。
    text: usize,
    /// うち索引の表が項目の symbol を持つ行の数。
    indexed: usize,
    /// 表に 1 行も無い file に現れる行の数。
    outside: usize,
    /// その file（昇順）。
    paths: Vec<String>,
}

/// 項目 1 つの数え。
pub(in crate::pipe) struct Counted {
    /// 項目の字。
    item: String,
    /// 解けた形。
    state: State,
    /// 外の印を付けたか。
    marked: bool,
    /// 6 列（symbol が 1 つに解けない周は空・解けない周は件数 0 の列）。
    columns: Vec<(&'static str, Vec<Hit>)>,
    /// 定義の可視性と親 module の可視性（解けた周だけ）。
    vis: String,
    /// 項目を touches に持つほかの行（`<doc>#<行 id>`・その行の write-set の外に在る site の file＝その行の閉包を広げる file・
    /// 欄を持たない行は `None`）。
    rows: Vec<(String, Option<Vec<String>>)>,
    /// 母集団（git を撃てない周は `None`）。
    population: Option<Population>,
}

/// write-set の項目（`+` `-` `~` `=` の接頭辞つき・末尾 `/` の dir）が path を含むか。
fn in_write_set(set: &[String], path: &str) -> bool {
    set.iter().any(|raw| {
        let item = raw.strip_prefix(['+', '-', '~', '=']).unwrap_or(raw);
        match item.strip_suffix('/') {
            Some(dir) => path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/')),
            None => item == path,
        }
    })
}

/// `crate::` の頭を落とした字（touches と項目の突き合わせ）。
fn bare(item: &str) -> &str {
    item.strip_prefix("crate::").unwrap_or(item)
}

/// symbol の末尾の節の名（囲む定義の名・空は `-`）。
fn last_name(symbol: &str) -> String {
    descriptor_names(symbol).pop().unwrap_or_else(|| "-".to_owned())
}

/// 列の site を作る。
fn hit(site: &Site, note: String, marks: Option<&[String]>) -> Hit {
    let outside = marks.is_some_and(|set| !in_write_set(set, &site.path));
    Hit { file: site.path.clone(), line: site.line, test: site.test, note, outside }
}

/// 同じ（囲む定義・test か）の site を 1 つに畳む（先に現れた物を残す）。
fn distinct(sites: Vec<(&Site, String)>, marks: Option<&[String]>) -> Vec<Hit> {
    let mut seen: BTreeSet<(String, bool)> = BTreeSet::new();
    sites.into_iter().filter(|(site, name)| seen.insert((name.clone(), site.test))).map(|(site, name)| hit(site, name, marks)).collect()
}

/// 6 列（refs・callers・literals・patterns・teeth・vis の reexport）の site。
fn columns_of(sites: &[Site], marks: Option<&[String]>) -> Vec<(&'static str, Vec<Hit>)> {
    let uses: Vec<&Site> = sites.iter().filter(|site| !site.definition).collect();
    let by_role = |word: &str| -> Vec<Hit> {
        uses.iter().filter(|site| site.roles.iter().any(|role| role == word)).map(|site| hit(site, word.to_owned(), marks)).collect()
    };
    let named = |keep: &dyn Fn(&Site) -> bool| -> Vec<Hit> {
        distinct(uses.iter().filter(|site| keep(site)).map(|site| (*site, last_name(&site.enclosing))).collect(), marks)
    };
    let refs = uses.iter().map(|site| hit(site, site.roles.join(","), marks)).collect();
    let callers = named(&|site| site.roles.iter().any(|role| role == "call"));
    let teeth = named(&|site| site.test);
    vec![("refs", refs), ("callers", callers), ("literals", by_role("literal")), ("patterns", by_role("pattern")), ("teeth", teeth), ("vis", by_role("reexport"))]
}

/// 定義の可視性と、親 module を crate の根まで辿った各段の可視性（定義の行を持たない段は `?`）。
fn vis_chain(rows: &[Row], symbol: &str) -> String {
    let defs: BTreeMap<&str, &str> = rows.iter().filter(|row| row.definition).map(|row| (row.symbol.as_str(), row.vis.as_str())).collect();
    let shown = |vis: &str| if vis.is_empty() { "private".to_owned() } else { vis.to_owned() };
    let mut text = format!("定義 {}", defs.get(symbol).map_or_else(|| "?".to_owned(), |vis| shown(vis)));
    let mut at = symbol;
    while let Some(parent) = defs.keys().filter(|key| key.ends_with('/') && at.starts_with(**key) && **key != at).max_by_key(|key| key.len()) {
        text.push_str(&format!(" ← {}={}", descriptor_names(parent).join("::"), shown(defs.get(parent).copied().unwrap_or("?"))));
        at = parent;
    }
    text
}

/// 項目を touches に持つほかの行（`own` を除く）と、その行の write-set の外に在る項目の site の file。
fn other_rows(tables: &Tables, item: &str, own: &str, sites: &[Site]) -> Vec<(String, Option<Vec<String>>)> {
    let mut found = Vec::new();
    for line in tables.lines.iter().filter(|line| line.pointer != own && line.touches.iter().any(|touch| bare(touch) == bare(item))) {
        let outside = (!line.write_set.is_empty()).then(|| {
            let files: BTreeSet<&str> = sites.iter().map(|site| site.path.as_str()).filter(|file| !in_write_set(&line.write_set, file)).collect();
            files.into_iter().map(str::to_owned).collect()
        });
        found.push((line.pointer.clone(), outside));
    }
    found
}

/// 名が語の境界で現れる行（`git grep -w`・rc 1 は 0 件・撃てない周は `None`）の（path・行）。
fn grep_lines(repo: &Path, sha: &str, name: &str) -> Option<Vec<(String, usize)>> {
    let output = Invocation::new("git")
        .arg("-C")
        .arg(repo)
        .args(["grep", "-n", "-w", "-F", "-I", "-z", "-e", name, sha, "--"])
        .output()
        .ok()?;
    match output.status.code() {
        Some(1) => return Some(Vec::new()),
        Some(0) => {}
        _ => return None,
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let prefix = format!("{sha}:");
    let mut found = Vec::new();
    for record in text.split('\n').filter(|record| !record.is_empty()) {
        let mut parts = record.splitn(3, '\0');
        let path = parts.next().and_then(|head| head.strip_prefix(&prefix))?;
        found.push((path.to_owned(), parts.next()?.parse::<usize>().ok()?));
    }
    Some(found)
}

/// 母集団を測る（`symbols` は項目が解けた symbol の全部）。
fn population(ctx: &Ctx<'_>, item: &str, symbols: &BTreeSet<&str>) -> Option<Population> {
    let name = item.rsplit("::").next().map(str::trim).filter(|name| !name.is_empty())?;
    let lines = grep_lines(ctx.repo, ctx.sha, name)?;
    let covered: BTreeSet<(&str, usize)> = ctx.rows.iter().filter(|row| symbols.contains(row.symbol.as_str())).map(|row| (row.path.as_str(), row.line)).collect();
    let indexed_files: BTreeSet<&str> = ctx.rows.iter().map(|row| row.path.as_str()).collect();
    let indexed = lines.iter().filter(|(path, line)| covered.contains(&(path.as_str(), *line))).count();
    let beyond: Vec<&(String, usize)> = lines.iter().filter(|(path, _)| !indexed_files.contains(path.as_str())).collect();
    let paths: BTreeSet<&str> = beyond.iter().map(|(path, _)| path.as_str()).collect();
    Some(Population { text: lines.len(), indexed, outside: beyond.len(), paths: paths.into_iter().map(str::to_owned).collect() })
}

/// 項目 1 つの 7 列の件数と site と write-set の外の印と母集団を数える（show・index.txt・行 e の測りが同じこの 1 本を撃つ）。
pub(in crate::pipe) fn count(ctx: &Ctx<'_>, item: &str, scope: &Scope<'_>) -> Counted {
    let (state, sites) = match query(ctx.rows, item) {
        Resolution::Unresolved => (State::Unresolved, Vec::new()),
        Resolution::One(found) => (State::One(found.symbol), found.sites),
        Resolution::Ambiguous(found) => {
            let many = found.iter().map(|one| (one.symbol.clone(), defined(&one.sites))).collect();
            (State::Many(many), Vec::new())
        }
    };
    let symbols: BTreeSet<&str> = match &state {
        State::One(symbol) => BTreeSet::from([symbol.as_str()]),
        State::Many(many) => many.iter().map(|(symbol, _)| symbol.as_str()).collect(),
        State::Unresolved => BTreeSet::new(),
    };
    let columns = if matches!(state, State::Many(_)) { Vec::new() } else { columns_of(&sites, scope.marks) };
    let vis = if let State::One(symbol) = &state { vis_chain(ctx.rows, symbol) } else { String::new() };
    let rows = other_rows(ctx.tables, item, scope.own, &sites);
    let population = population(ctx, item, &symbols);
    Counted { item: item.to_owned(), state, marked: scope.marks.is_some(), columns, vis, rows, population }
}

/// 数えの読み口（欄 `code-facts` の測り〔行 e〕が読む・描きの [`render`] と同じ値を返し、数えの本体と字は変えない）。
impl Counted {
    /// 列 `name` の site（`<path>:<行>`・数えの順）。symbol が複数に解けた周と列の名が無い周は `None`・解けない周は空。
    pub(in crate::pipe) fn sites(&self, name: &str) -> Option<Vec<String>> {
        let (_, hits) = self.columns.iter().find(|(word, _)| *word == name)?;
        Some(hits.iter().map(|found| format!("{}:{}", found.file, found.line)).collect())
    }

    /// 定義の可視性の字（`pub`・`pub(crate)`・`pub(in <path>)`・`private` など・解けない周と複数に解けた周は `None`）。
    pub(in crate::pipe) fn definition_vis(&self) -> Option<String> {
        let chain = self.vis.strip_prefix("定義 ")?;
        chain.split(" ← ").next().filter(|word| *word != "?").map(str::to_owned)
    }

    /// symbol が複数に解けた周の候補の数。
    pub(in crate::pipe) fn ambiguous(&self) -> Option<usize> {
        match &self.state {
            State::Many(many) => Some(many.len()),
            State::One(_) | State::Unresolved => None,
        }
    }

    /// 母集団の `text=`（項目の最後の節の名が語の境界で現れる行の数・git を撃てない周は `None`）。
    pub(in crate::pipe) fn text(&self) -> Option<usize> {
        self.population.as_ref().map(|pop| pop.text)
    }
}

/// 候補の定義の site（`<path>:<行>`）。
fn defined(sites: &[Site]) -> Vec<String> {
    sites.iter().filter(|site| site.definition).map(|site| format!("{}:{}", site.path, site.line)).collect()
}

/// 列の頭の 1 語（`refs=5` か外の印つきの `refs=5(外 2)`）。
fn tally(name: &str, hits: &[Hit], marked: bool) -> String {
    let outside = hits.iter().filter(|found| found.outside).count();
    if marked { format!("{name}={}({OUTSIDE}{outside})", hits.len()) } else { format!("{name}={}", hits.len()) }
}

/// 列の詳細（本体と test の別・file の数・site の列）。
fn detail(name: &str, hits: &[Hit], marked: bool) -> Vec<String> {
    let test = hits.iter().filter(|found| found.test).count();
    let files: BTreeSet<&str> = hits.iter().map(|found| found.file.as_str()).collect();
    let outside = hits.iter().filter(|found| found.outside).count();
    let tail = if marked { format!("・{OUTSIDE} {outside}") } else { String::new() };
    let mut lines = vec![format!("  {name}: 本体 {}・test {test}・file {}{tail}", hits.len().saturating_sub(test), files.len())];
    for found in hits {
        let (place, mark) = (if found.test { " test" } else { "" }, if found.outside { format!(" {OUTSIDE}") } else { String::new() });
        lines.push(format!("    {}:{}{place} {}{mark}", found.file, found.line, found.note));
    }
    lines
}

/// 母集団の語（測れない周は `-`）。
fn census(found: &Option<Population>) -> String {
    match found {
        Some(pop) => format!("text={} indexed={} outside-index={}", pop.text, pop.indexed, pop.outside),
        None => "text=- indexed=- outside-index=-".to_owned(),
    }
}

/// 項目 1 つの塊に描く（頭の 1 行は件数だけ・続きは 2 字下げ・show と index.txt が同じこの 1 本を使う）。
pub(in crate::pipe) fn render(found: &Counted) -> String {
    let state = match &found.state {
        State::One(_) => "resolved".to_owned(),
        State::Unresolved => "unresolved".to_owned(),
        State::Many(many) => format!("ambiguous:{}", many.len()),
    };
    let mut parts = vec![state];
    parts.extend(found.columns.iter().map(|(name, hits)| tally(name, hits, found.marked)));
    if !found.columns.is_empty() {
        parts.push(format!("rows={}", found.rows.len()));
    }
    parts.push(census(&found.population));
    let mut lines = vec![format!("{ITEM_HEAD}{}: {}", found.item, parts.join(" "))];
    match &found.state {
        State::One(symbol) => lines.push(format!("  symbol: {symbol}")),
        State::Many(many) => lines.extend(many.iter().map(|(symbol, sites)| format!("  候補: {symbol} {}", sites.join(", ")))),
        State::Unresolved => {}
    }
    for (name, hits) in &found.columns {
        lines.extend(detail(name, hits, found.marked));
    }
    if !found.vis.is_empty() {
        lines.push(format!("  vis: {}", found.vis));
    }
    if !found.columns.is_empty() {
        lines.push(format!("  rows: {}", found.rows.len()));
        lines.extend(found.rows.iter().map(|(pointer, outside)| match outside {
            Some(files) if !files.is_empty() => format!("    {pointer} 広げる: {}", files.join(", ")),
            Some(_) => format!("    {pointer}"),
            None => format!("    {pointer} write-set=derived"),
        }));
    }
    if let Some(pop) = found.population.as_ref().filter(|pop| pop.outside > 0) {
        lines.push(format!("  outside-index: {} {}", pop.outside, pop.paths.join(", ")));
    }
    lines.join("\n")
}

/// 行 `pointer` の表（touches と節の名指しを項目にし・欄 write-set で外の印を付ける・欄の無い行は項目の前に `write-set=derived`）。
pub(in crate::pipe) fn row_report(ctx: &Ctx<'_>, pointer: &str) -> Result<String, String> {
    let line = ctx.tables.lines.iter().find(|line| line.pointer == pointer).ok_or_else(|| format!("行 {pointer} を契約表から引けない"))?;
    let prose = ctx.tables.prose(line);
    let mut items = line.touches.clone();
    for name in section_symbols(&[prose.as_str()], &line.touches) {
        if !items.contains(&name) {
            items.push(name);
        }
    }
    let marks = (!line.write_set.is_empty()).then_some(line.write_set.as_slice());
    let scope = Scope { own: pointer, marks };
    let mut blocks: Vec<String> = marks.is_none().then(|| "write-set=derived".to_owned()).into_iter().collect();
    blocks.extend(items.iter().map(|item| render(&count(ctx, item, &scope))));
    Ok(blocks.join("\n"))
}

/// 索引の状態の語（ready と undeclared は語を持たない）。
fn word_of(state: &Status) -> Option<String> {
    match state {
        Status::Undeclared | Status::Ready(_) => None,
        Status::Half(_) => Some("half".to_owned()),
        Status::Absent => Some("absent".to_owned()),
        Status::Building => Some("building".to_owned()),
        Status::Failed(word) => Some(word.clone()),
    }
}

/// 審査の材料 index.txt の本文（undeclared の repo は `None`＝file を置かない・ready でない周は `index=unavailable:<語>` の 1 行）。
fn material(state_dir: &Path, repo: &Path, head: Option<&str>, design: &str) -> Option<String> {
    let Some(sha) = head else {
        return Some("index=unavailable:tree".to_owned());
    };
    let rows = match status(state_dir, repo, sha) {
        Status::Undeclared => return None,
        Status::Ready(rows) => rows,
        other => return Some(format!("index=unavailable:{}", word_of(&other).unwrap_or_default())),
    };
    let Ok(tables) = Tables::load(repo, sha) else {
        return Some("index=unavailable:table".to_owned());
    };
    let ctx = Ctx { rows: &rows, repo, sha, tables: &tables };
    Some(row_report(&ctx, design).unwrap_or_else(|_| "index=unavailable:row".to_owned()))
}

/// 契約の審査が材料の dir に index.txt を置く（既存の材料を置いた後・`design` は契約の design の pointer・`head` は審査の木の commit）。
pub(in crate::pipe) fn keep(dir: &Path, place: (&Path, &Path), head: Option<&str>, design: &str) -> Result<(), String> {
    let Some(body) = material(place.0, place.1, head, design) else {
        return Ok(());
    };
    let path = dir.join(INDEX_FILE);
    std::fs::write(&path, material_file(&body)).map_err(|err| format!("{} を書けない: {err}", path.display()))
}

/// 写しの先頭の字（最初の項目の頭の前・`index=unavailable:…` の 1 行や `write-set=derived` の 1 行）と、項目ごとの塊。
fn split(copy: &str) -> (String, Vec<String>) {
    let mut preface: Vec<&str> = Vec::new();
    let mut items: Vec<String> = Vec::new();
    for line in copy.lines() {
        match items.last_mut() {
            Some(open) if !line.starts_with(ITEM_HEAD) => {
                open.push('\n');
                open.push_str(line);
            }
            _ if line.starts_with(ITEM_HEAD) => items.push(line.to_owned()),
            _ => preface.push(line),
        }
    }
    (preface.join("\n"), items)
}

/// `{index}` の穴の本文（lens が埋める）: 写しが空なら空文字（雛形は 1 字も変わらない）。見出しと 1 文の後ろに先頭の字と項目の塊を
/// `room` byte の残りへ収める（全部が収まる項目は全部・収まらない項目は件数だけの頭の 1 行・それも収まらない項目は落とす）。落とした
/// 項目の数（落とした項目だけ）を最後の 1 行に数える（既存 cap の残りで測る・新しい閾値を作らない）。
pub fn index_block(copy: &str, room: u64) -> String {
    let copy = copy.trim_end();
    if copy.is_empty() {
        return String::new();
    }
    let fits = |bytes: usize| u64::try_from(bytes).unwrap_or(u64::MAX) <= room;
    let (preface, chunks) = split(copy);
    let mut block = format!("{HEADING}{PREAMBLE}\n");
    if !preface.is_empty() {
        block.push_str(&format!("\n{preface}"));
    }
    let mut dropped = 0_usize;
    for chunk in chunks {
        let whole = format!("\n{chunk}");
        let short = format!("\n{}", chunk.lines().next().unwrap_or_default());
        if fits(block.len().saturating_add(whole.len())) {
            block.push_str(&whole);
        } else if fits(block.len().saturating_add(short.len())) {
            block.push_str(&short);
        } else {
            dropped = dropped.saturating_add(1);
        }
    }
    if dropped > 0 {
        block.push_str(&format!("\n（cap の残りに収まらず落とした項目: {dropped} 個）"));
    }
    block
}

/// argv の順の `--row` と `--item` の値（`(行か項目か・値)`・dispatch が値の有無を閉包の検査で断った後に読む）。
fn asked(args: &[String]) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == "--row" || word == "--item" {
            if let Some(value) = words.next() {
                found.push((word.as_str(), value.as_str()));
            }
        }
    }
    found
}

/// 索引の宣言の不備（key の名と行番号）。
fn half(errors: &[crate::pipe::declaration::DeclError]) -> Outcome {
    let mut err = vec!["pipe: 索引の宣言を読めない（index-scip と index-roles は両方を宣言する）".to_owned()];
    err.extend(errors.iter().map(|error| format!("pipe: {error}")));
    Outcome { out: Vec::new(), err, rc: RC_BROKEN }
}

/// 項目ごとの表を引数の順に描く（引けない行は rc 2）。
fn tables_of(ctx: &Ctx<'_>, asks: &[(&str, &str)]) -> Outcome {
    let mut out = Vec::new();
    for (kind, value) in asks {
        let text = if *kind == "--row" {
            match row_report(ctx, value) {
                Ok(found) => found,
                Err(reason) => return broken(reason),
            }
        } else {
            render(&count(ctx, value, &Scope { own: "", marks: None }))
        };
        out.extend(text.lines().map(str::to_owned));
    }
    Outcome { out, err: Vec::new(), rc: RC_OK }
}

/// `<NAME> pipe index show --repo R --state-dir S [--ref <sha>] (--row <doc>#<行 id> | --item <path>)…`: ref（既定は HEAD）の commit の
/// 索引を組み立ての 1 本で得て（無ければ組み・撃ち中の持ち主が生きていれば終わりを待つ）、項目ごとの表を stdout に出す。組み立てが
/// 失敗した周は `index=unavailable:<語>`・宣言が 2 key を持たない repo は `index=unavailable:undeclared` の 1 行だけで rc 0。
/// 片方だけの宣言・引けない行・解けない ref・置き場の宣言を読めない周は rc 2。
pub(in crate::pipe) fn show(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let (state_dir, repo) = match (state_dir_of(args), repo_of(args)) {
        (Ok(state_dir), Ok(repo)) => (state_dir, repo),
        (Err(reason), _) | (_, Err(reason)) => return refused(reason),
    };
    let rev = match flag(args, "--ref") {
        Ok(found) => found.unwrap_or("HEAD"),
        Err(reason) => return refused(reason),
    };
    let asks = asked(args);
    if asks.is_empty() {
        return refused("index show は --row か --item を 1 つ以上要る（pipe index show --repo R --state-dir S [--ref SHA] --row DOC#ID --item PATH）".to_owned());
    }
    let Some(sha) = crate::pipe::git_line(&repo, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")]) else {
        return broken(format!("{rev} を解けない（repo でないか ref が無い）"));
    };
    let unavailable = |word: &str| Outcome::ok_line(format!("index=unavailable:{word}"));
    match assemble(&state_dir, &repo, &sha, manifest, policy) {
        Assembled::Undeclared => unavailable("undeclared"),
        Assembled::Half(errors) => half(&errors),
        Assembled::Made(made) => match (&made.how, status(&state_dir, &repo, &sha)) {
            (How::Failed(word), _) => unavailable(word),
            (_, Status::Ready(rows)) => match Tables::load(&repo, &sha) {
                Ok(tables) => tables_of(&Ctx { rows: &rows, repo: &repo, sha: &sha, tables: &tables }, &asks),
                Err(reason) => broken(reason),
            },
            (_, other) => unavailable(&word_of(&other).unwrap_or_default()),
        },
    }
}
