//! 局面の出力の入力の印と古さの印の file（設計 docs/design/case-lifecycle.md §5.2・§5.3・§12 約束 1・5・FR90・ADR-0088）。
//!
//! 入力の印は 3 つ: 台帳（[`Ledger`]・`noms` は manifest の 1 file だけ・`files` は `issues.jsonl` の長さと更新時刻）・event log
//! （[`Events`]・長さと 1 行目の ts）・main（`refs/remotes/origin/main` の sha・git を撃たずに loose の ref → packed-refs → worktree の
//! common dir の順に読む）。読みは stat と小さい読みだけで、bd も git も撃たない。順の比べ（[`ledger_order`]・[`events_order`]・
//! [`main_order`]）は「順を持たない」組を `None` で返し、捨てる判定と Coalesced は「古くない」と読み、印を消す判定は
//! gen か head が違えば「新しい」と読む。
//!
//! 古さの印の file（`lifecycle.stale`）の読み書きもこの file の 1 本が持つ: 種類は閉じた 3 つ（[`Kind`]）・種類ごとに 1 つで後の
//! 印が置き換え・`lifecycle.stale.lock` を死んだ所有者だけ外す取り方で短く取り・一時 file からの rename で置き換える。
//!
//! 台帳の印の隣に journal の長さの読み（[`read_journal_len`]）を持つ（先読みの口の store の鍵・設計 ledger-form.md §20）。
//!
//! 全部の書き直しの入力の読み（契約表・SRS・main の commit の trailer・event log の便と断りと結び）も末尾の区間に置く。

use super::json_tree::{self, Tree};
use super::phase::{Latest, Refused};
use super::store::{acquire_with, events_path, LockPolicy, Reclaim};
use super::wait::epoch_of;
use super::{replay, Case, Event, State};
use crate::case::Kind as Part;
use crate::hook::vessel::digest::fnv1a_64;
use crate::hook::vessel::state_dir as named_state_dir;
use crate::ledger::form::{is_memo, is_question, pointer_text};
use crate::ledger::phase_main::{Commit, Row};
use crate::pipe::declaration::{requirements_at_sha, TablePlaces};
use crate::pipe::dispatch::memo::Word;
use crate::pipe::dispatch::unreflected::{asked, Asked, Question};
use crate::pipe::land::{contract_key, source_key, RUN_TRAILER, TERMINAL_TOKENS};
use crate::pipe::table::{design_docs, read_table as table_rows, requirement_ids};
use crate::pipe::{git_bytes, live_driver};
use crate::seat::ledger::Issue;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// 出力の file 名（読み手が見る 2 つの名の 1 つ）。
pub const JSON_FILE: &str = "lifecycle.json";

/// 古さの印の file 名（読み手が見る 2 つの名のもう 1 つ）。
pub const STALE_FILE: &str = "lifecycle.stale";

/// 書き手の lock の名。
pub const JSON_LOCK: &str = "lifecycle.lock";

/// 古さの印の lock の名。
pub const STALE_LOCK: &str = "lifecycle.stale.lock";

/// main の印の ref（器の land が基にする anchor・`crate::pipe::queue` の私有の const と同じ字・読むだけで fetch しない）。
pub const MAIN_REF: &str = "refs/remotes/origin/main";

/// 台帳の dir（`--repo` の下）。
const LEDGER_DIR: &str = ".beads";

/// `files` の形の台帳の file。
const ISSUES_FILE: &str = "issues.jsonl";

/// 台帳の印の 1 つ（§5.3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ledger {
    /// embedded の store（manifest の 1 file から組む）。
    Noms {
        /// manifest の root の字。
        root: String,
        /// gc の世代と journal を除く file 名の列の digest（16 字の 16 進）。
        generation: String,
        /// chunk 数の和。
        chunks: u64,
    },
    /// store が無い台帳（`issues.jsonl` の長さと更新時刻）。
    Files {
        /// byte 長。
        len: u64,
        /// 更新時刻（ns）。
        mtime_ns: u64,
    },
}

/// event log の印（§5.3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Events {
    /// byte 長。
    pub len: u64,
    /// 1 行目の ts（log が空なら `None`）。
    pub head: Option<String>,
}

/// 入力の印 3 つ（main は sha だけ持つ・ref は [`MAIN_REF`] の 1 つ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marks {
    /// 台帳。
    pub ledger: Ledger,
    /// event log。
    pub events: Events,
    /// main の sha（40 桁の小文字の 16 進）。
    pub main: String,
}

/// 台帳の印を読む（`<repo>/.beads` の下の file だけ・子 process を撃たない・読めなければ `None`）。
pub fn read_ledger(repo: &Path) -> Option<Ledger> {
    let dir = repo.join(LEDGER_DIR);
    match store_of(&dir.join("metadata.json")) {
        Store::Embedded(database) => noms_of(&noms_dir(&dir, &database).join("manifest")),
        Store::Other => files_of(&dir.join(ISSUES_FILE)),
        Store::Broken => None,
    }
}

/// embedded の store の noms の dir（manifest と journal の file の置き場）。
fn noms_dir(ledger: &Path, database: &str) -> PathBuf {
    ledger.join("embeddeddolt").join(database).join(".dolt").join("noms")
}

/// journal（manifest の組のうち名が全部 `v` の file）の byte 長の和を読む（[`read_ledger`] の隣・設計 ledger-form.md §20 約束 1）。
/// embedded の store でない・manifest を読めない・journal の組か file が無い周は `None`。子 process を撃たない。
pub fn read_journal_len(repo: &Path) -> Option<u64> {
    let dir = repo.join(LEDGER_DIR);
    let Store::Embedded(database) = store_of(&dir.join("metadata.json")) else { return None };
    let noms = noms_dir(&dir, &database);
    let text = fs::read_to_string(noms.join("manifest")).ok()?;
    let fields: Vec<&str> = text.trim_end_matches('\n').split(':').collect();
    let specs = fields.get(5..)?;
    if !specs.len().is_multiple_of(2) {
        return None;
    }
    let mut total: Option<u64> = None;
    for pair in specs.chunks(2) {
        let name = pair.first()?;
        if is_journal(name) {
            total = Some(total.unwrap_or(0).checked_add(fs::metadata(noms.join(name)).ok()?.len())?);
        }
    }
    total
}

/// `metadata.json` が示す store。
enum Store {
    /// `dolt_mode` が `embedded`（値は `dolt_database`）。
    Embedded(String),
    /// file が無いか embedded でない。
    Other,
    /// file が在って読めない。
    Broken,
}

/// `metadata.json` を読む（無ければ [`Store::Other`]・在って読めないか embedded で db の名が無ければ [`Store::Broken`]）。
fn store_of(path: &Path) -> Store {
    let text = match fs::read_to_string(path) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Store::Other,
        Err(_) => return Store::Broken,
    };
    let Ok(tree) = json_tree::parse(&text) else { return Store::Broken };
    if tree.get("dolt_mode").and_then(Tree::as_str) != Some("embedded") {
        return Store::Other;
    }
    match tree.get("dolt_database").and_then(Tree::as_str) {
        Some(database) if !database.is_empty() => Store::Embedded(database.to_owned()),
        _ => Store::Broken,
    }
}

/// manifest（`:` で割った 1 行・2 つ目が `__DOLT__`・4 つ目が root・5 つ目が gc の世代・6 つ目から「file 名:chunk 数」の組）から [`Ledger::Noms`]。
fn noms_of(path: &Path) -> Option<Ledger> {
    let text = fs::read_to_string(path).ok()?;
    let fields: Vec<&str> = text.trim_end_matches('\n').split(':').collect();
    if fields.get(1) != Some(&"__DOLT__") {
        return None;
    }
    let (root, collected) = (fields.get(3)?, fields.get(4)?);
    let specs = fields.get(5..)?;
    if root.is_empty() || !specs.len().is_multiple_of(2) {
        return None;
    }
    let mut keyed = vec![*collected];
    let mut chunks: u64 = 0;
    for pair in specs.chunks(2) {
        let (name, count) = (pair.first()?, pair.get(1)?);
        chunks = chunks.checked_add(count.parse::<u64>().ok()?)?;
        if !is_journal(name) {
            keyed.push(name);
        }
    }
    Some(Ledger::Noms { root: (*root).to_owned(), generation: fnv1a_64(keyed.join("\n").as_bytes()), chunks })
}

/// journal の file 名（名が全部 `v`）か。
fn is_journal(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|found| found == 'v')
}

/// `issues.jsonl` の長さと更新時刻（ns）から [`Ledger::Files`]。
fn files_of(path: &Path) -> Option<Ledger> {
    let meta = fs::metadata(path).ok()?;
    let since = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(Ledger::Files { len: meta.len(), mtime_ns: u64::try_from(since.as_nanos()).ok()? })
}

/// event log の印を読む（log が無ければ長さ 0 と head なし・読めなければ `None`）。
pub fn read_events(state_dir: &Path) -> Option<Events> {
    let path = events_path(state_dir);
    let file = match fs::File::open(&path) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Some(Events { len: 0, head: None }),
        Err(_) => return None,
    };
    let len = file.metadata().ok()?.len();
    let mut first = String::new();
    std::io::BufReader::new(file).read_line(&mut first).ok()?;
    let head = json_tree::parse(first.trim_end()).ok().and_then(|tree| tree.get("ts").and_then(Tree::as_str).map(str::to_owned));
    Some(Events { len, head })
}

/// main の sha を読む（git を撃たない・loose の ref → packed-refs の順・worktree は `.git` の file が指す common dir を読む）。
pub fn read_main(repo: &Path) -> Option<String> {
    let common = common_dir(repo)?;
    match fs::read_to_string(common.join(MAIN_REF)) {
        Ok(text) => sha_of(text.trim()).map(str::to_owned),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => packed_sha(&common),
        Err(_) => None,
    }
}

/// common dir（`.git` が dir ならそれ・file なら `gitdir:` の指す dir と、その `commondir` が指す先）。
fn common_dir(repo: &Path) -> Option<PathBuf> {
    let dotgit = repo.join(".git");
    if dotgit.is_dir() {
        return Some(dotgit);
    }
    let text = fs::read_to_string(&dotgit).ok()?;
    let target = text.lines().find_map(|line| line.strip_prefix("gitdir:"))?.trim();
    let gitdir = resolved(repo, target);
    Some(match fs::read_to_string(gitdir.join("commondir")) {
        Ok(relative) => resolved(&gitdir, relative.trim()),
        Err(_) => gitdir,
    })
}

/// `base` からの path（絶対なら `text` のまま）。
fn resolved(base: &Path, text: &str) -> PathBuf {
    let path = Path::new(text);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

/// `packed-refs` の [`MAIN_REF`] の行の sha（コメントと peeled の行は読まない）。
fn packed_sha(common: &Path) -> Option<String> {
    let text = fs::read_to_string(common.join("packed-refs")).ok()?;
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with('^'))
        .find_map(|line| line.split_once(' ').filter(|(_, name)| *name == MAIN_REF).and_then(|(sha, _)| sha_of(sha)))
        .map(str::to_owned)
}

/// 40 桁の小文字の 16 進ならそのまま。
fn sha_of(text: &str) -> Option<&str> {
    (text.len() == 40 && text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))).then_some(text)
}

/// 3 つの印を読む（読めなかった最初の入力の語を `Err` に返す・読む順は `ledger`・`events`・`main`）。
pub fn read_marks(state_dir: &Path, repo: &Path) -> Result<Marks, &'static str> {
    let ledger = read_ledger(repo).ok_or("ledger")?;
    let events = read_events(state_dir).ok_or("events")?;
    let main = read_main(repo).ok_or("main")?;
    Ok(Marks { ledger, events, main })
}

/// 台帳の印の順（`current` が `mine` に対して）。gen が違う組・同じ chunks で root が違う組・形が違う組は順を持たない（`None`）。
pub fn ledger_order(current: &Ledger, mine: &Ledger) -> Option<Ordering> {
    match (current, mine) {
        (
            Ledger::Noms { root, generation, chunks },
            Ledger::Noms { root: other_root, generation: other_generation, chunks: other_chunks },
        ) if generation == other_generation => match chunks.cmp(other_chunks) {
            Ordering::Equal => (root == other_root).then_some(Ordering::Equal),
            unequal => Some(unequal),
        },
        (Ledger::Files { len, mtime_ns }, Ledger::Files { len: other_len, mtime_ns: other_mtime }) => {
            Some(len.cmp(other_len).then(mtime_ns.cmp(other_mtime)))
        }
        _ => None,
    }
}

/// event log の印の順（`current` が `mine` に対して）。head が違う組は順を持たない。
pub fn events_order(current: &Events, mine: &Events) -> Option<Ordering> {
    (current.head == mine.head).then(|| current.len.cmp(&mine.len))
}

/// main の sha の順（git の祖先の関係・`current` が `mine` に対して・判じるのは全部の書き直しだけ）。祖先の関係に無い組と git を
/// 撃てない周は順を持たない。
pub fn main_order(repo: &Path, current: &str, mine: &str) -> Option<Ordering> {
    if current == mine {
        return Some(Ordering::Equal);
    }
    if is_ancestor(repo, mine, current) {
        return Some(Ordering::Greater);
    }
    is_ancestor(repo, current, mine).then_some(Ordering::Less)
}

/// `ancestor` が `descendant` の祖先か（`git merge-base --is-ancestor`・撃てない周は偽）。
fn is_ancestor(repo: &Path, ancestor: &str, descendant: &str) -> bool {
    crate::pipe::git_ok(repo, &["merge-base", "--is-ancestor", ancestor, descendant])
}

/// 印を消す判定の「新しい」台帳: `read` が `mark` より新しい（gen が違えば新しい・同じ gen なら chunks が多い・files は順が大きい）。
pub fn ledger_is_newer(read: &Ledger, mark: &Ledger) -> bool {
    match (read, mark) {
        (Ledger::Noms { generation, .. }, Ledger::Noms { generation: other, .. }) if generation != other => true,
        _ => ledger_order(read, mark) == Some(Ordering::Greater),
    }
}

/// 印を消す判定の「進んだ」main: `read` が `mark` の真の子孫。
pub fn main_is_descendant(repo: &Path, read: &str, mark: &str) -> bool {
    read != mark && is_ancestor(repo, mark, read)
}

/// 印の種類（閉じた 3 つ・宣言順）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 起票の門（台帳の書きを通した）。
    LedgerGate,
    /// merge の門（main へ入る書きを通した）。
    MergeGate,
    /// 読めない周。
    Unreadable,
}

impl Kind {
    /// 全種類（宣言順）。
    pub const ALL: [Self; 3] = [Self::LedgerGate, Self::MergeGate, Self::Unreadable];

    /// 出力の字。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LedgerGate => "ledger-gate",
            Self::MergeGate => "merge-gate",
            Self::Unreadable => "unreadable",
        }
    }

    /// 字から種類。
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// 印の値（種類が決める: 起票の門は台帳の印・merge の門は main の sha・読めない周は理由の 1 語）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// 台帳の印。
    Ledger(Ledger),
    /// main の sha。
    Main(String),
    /// 読めない理由の語。
    Reason(String),
}

/// 古さの印 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    /// 種類。
    pub kind: Kind,
    /// 付けた時刻（UTC の秒まで）。
    pub at: String,
    /// 値。
    pub value: Value,
}

impl Mark {
    /// 印を付けた時刻（UNIX 秒・読めなければ `None`）。
    pub fn at_secs(&self) -> Option<u64> {
        epoch_of(&self.at)
    }
}

/// 古さの印の file の読み（閉じた 3 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stale {
    /// file が無い。
    Absent,
    /// file が在って読める形でない。
    Unreadable,
    /// 読めた印（file の順）。
    Marks(Vec<Mark>),
}

/// `<state_dir>/fleet`（出力と lock と一時 file の置き場）。
pub fn fleet_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("fleet")
}

/// 古さの印の file を読む（読み手が見る名は [`STALE_FILE`] だけ）。
pub fn read_stale(state_dir: &Path) -> Stale {
    let text = match fs::read_to_string(fleet_dir(state_dir).join(STALE_FILE)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Stale::Absent,
        Err(_) => return Stale::Unreadable,
    };
    match marks_of(&text) {
        Some(marks) => Stale::Marks(marks),
        None => Stale::Unreadable,
    }
}

/// 古さの印の本文を読む（版が 1 でない・形が違う・種類と値が食い違う・同じ種類が 2 つ在る周は `None`）。
fn marks_of(text: &str) -> Option<Vec<Mark>> {
    let tree = json_tree::parse(text).ok()?;
    if !matches!(tree.get("version"), Some(Tree::Num(version)) if version == "1") {
        return None;
    }
    let marks: Vec<Mark> = tree.get("marks")?.as_array()?.iter().map(mark_of).collect::<Option<_>>()?;
    let distinct = marks.iter().enumerate().all(|(at, mark)| marks.iter().skip(at + 1).all(|other| other.kind != mark.kind));
    distinct.then_some(marks)
}

/// 印 1 つを読む。
fn mark_of(node: &Tree) -> Option<Mark> {
    let kind = Kind::parse(node.get("kind")?.as_str()?)?;
    let at = node.get("at")?.as_str()?.to_owned();
    let inputs = node.get("inputs")?;
    let value = match kind {
        Kind::LedgerGate => Value::Ledger(ledger_of(inputs.get("ledger")?)?),
        Kind::MergeGate => Value::Main(main_of(inputs.get("main")?)?),
        Kind::Unreadable => Value::Reason(node.get("reason")?.as_str()?.to_owned()),
    };
    Some(Mark { kind, at, value })
}

/// 数の欄。
pub fn num(value: u64) -> Tree {
    Tree::Num(value.to_string())
}

/// 台帳の印の木（§5.2）。
pub fn ledger_tree(ledger: &Ledger) -> Tree {
    let pairs = match ledger {
        Ledger::Noms { root, generation, chunks } => vec![
            ("form".to_owned(), Tree::Str("noms".to_owned())),
            ("root".to_owned(), Tree::Str(root.clone())),
            ("gen".to_owned(), Tree::Str(generation.clone())),
            ("chunks".to_owned(), num(*chunks)),
        ],
        Ledger::Files { len, mtime_ns } => {
            vec![("form".to_owned(), Tree::Str("files".to_owned())), ("len".to_owned(), num(*len)), ("mtime_ns".to_owned(), num(*mtime_ns))]
        }
    };
    Tree::Object(pairs)
}

/// 数の欄を読む。
pub fn num_of(node: &Tree) -> Option<u64> {
    match node {
        Tree::Num(digits) => digits.parse().ok(),
        _ => None,
    }
}

/// 台帳の印を木から読む。
pub fn ledger_of(node: &Tree) -> Option<Ledger> {
    match node.get("form")?.as_str()? {
        "noms" => Some(Ledger::Noms {
            root: node.get("root")?.as_str()?.to_owned(),
            generation: node.get("gen")?.as_str()?.to_owned(),
            chunks: num_of(node.get("chunks")?)?,
        }),
        "files" => Some(Ledger::Files { len: num_of(node.get("len")?)?, mtime_ns: num_of(node.get("mtime_ns")?)? }),
        _ => None,
    }
}

/// event log の印の木。
pub fn events_tree(events: &Events) -> Tree {
    let head = events.head.clone().map_or(Tree::Null, Tree::Str);
    Tree::Object(vec![("len".to_owned(), num(events.len)), ("head".to_owned(), head)])
}

/// event log の印を木から読む。
pub fn events_of(node: &Tree) -> Option<Events> {
    let head = match node.get("head")? {
        Tree::Null => None,
        Tree::Str(text) => Some(text.clone()),
        _ => return None,
    };
    Some(Events { len: num_of(node.get("len")?)?, head })
}

/// main の印の木。
pub fn main_tree(sha: &str) -> Tree {
    Tree::Object(vec![("ref".to_owned(), Tree::Str(MAIN_REF.to_owned())), ("sha".to_owned(), Tree::Str(sha.to_owned()))])
}

/// main の印を木から読む（ref が [`MAIN_REF`] で sha が 40 桁の小文字の 16 進）。
pub fn main_of(node: &Tree) -> Option<String> {
    (node.get("ref")?.as_str()? == MAIN_REF).then_some(())?;
    sha_of(node.get("sha")?.as_str()?).map(str::to_owned)
}

/// 古さの印 1 つの木。
fn mark_tree(mark: &Mark) -> Tree {
    let (inputs, reason) = match &mark.value {
        Value::Ledger(ledger) => (Tree::Object(vec![("ledger".to_owned(), ledger_tree(ledger))]), Tree::Null),
        Value::Main(sha) => (Tree::Object(vec![("main".to_owned(), main_tree(sha))]), Tree::Null),
        Value::Reason(word) => (Tree::Null, Tree::Str(word.clone())),
    };
    Tree::Object(vec![
        ("kind".to_owned(), Tree::Str(mark.kind.as_str().to_owned())),
        ("at".to_owned(), Tree::Str(mark.at.clone())),
        ("inputs".to_owned(), inputs),
        ("reason".to_owned(), reason),
    ])
}

/// 古さの印の file の本文（末尾に改行）。
fn stale_text(marks: &[Mark]) -> String {
    let tree = Tree::Object(vec![
        ("version".to_owned(), num(1)),
        ("marks".to_owned(), Tree::Array(marks.iter().map(mark_tree).collect())),
    ]);
    format!("{}\n", json_tree::render(&tree))
}

/// 一時 file に書いて fsync し、`name` へ rename する（読み手は `name` だけを見る・書きかけは `name` に見えない）。rename が落ちた周は
/// 一時 file を消して `Err`（既存の `name` は動かない）。
pub fn publish(dir: &Path, name: &str, text: &str) -> std::io::Result<()> {
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let written = fs::File::create(&tmp).and_then(|mut file| file.write_all(text.as_bytes()).and_then(|()| file.sync_all()));
    let renamed = written.and_then(|()| fs::rename(&tmp, dir.join(name)));
    if renamed.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    renamed
}

/// 取った lock（drop で外す）。
pub struct Held {
    /// lock の path。
    path: PathBuf,
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// `dir` の lock を死んだ所有者だけ外す取り方で取る（取れなければ `None`・生きた所有者は古くても外さない）。
pub fn hold(dir: &Path, name: &str, policy: LockPolicy) -> Option<Held> {
    let _ = fs::create_dir_all(dir);
    let path = dir.join(name);
    acquire_with(&path, policy, Reclaim::DeadOnly).ok().map(|_| Held { path })
}

/// 印を足す関数の返り（閉じた 4 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    /// 付けた。
    Added,
    /// 出力（`lifecycle.json`）が無い。
    NoOutput,
    /// `lifecycle.stale.lock` を取れない。
    Busy,
    /// `lifecycle.stale` が読める形でない。
    Unreadable,
}

/// 印を 1 つ足す（hook の module から呼べる・種類ごとに 1 つで同じ種類の前の印を置き換える・`lifecycle.json` が無ければ何も書かない）。
pub fn add_mark(state_dir: &Path, mark: &Mark, policy: LockPolicy) -> Added {
    let dir = fleet_dir(state_dir);
    if !dir.join(JSON_FILE).is_file() {
        return Added::NoOutput;
    }
    let Some(_held) = hold(&dir, STALE_LOCK, policy) else { return Added::Busy };
    let mut marks = match read_stale(state_dir) {
        Stale::Absent => Vec::new(),
        Stale::Marks(found) => found,
        Stale::Unreadable => return Added::Unreadable,
    };
    marks.retain(|kept| kept.kind != mark.kind);
    marks.push(mark.clone());
    marks.sort_by_key(|kept| Kind::ALL.iter().position(|kind| *kind == kept.kind));
    match publish(&dir, STALE_FILE, &stale_text(&marks)) {
        Ok(()) => Added::Added,
        Err(_) => Added::Unreadable,
    }
}

/// 全部の書き直しが json の rename の後に古さの印を整える返り。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// 整えた（file が無ければ空の marks を作り、消す印だけを消した）。
    Done,
    /// lock を取れなかった（印は動かない）。
    Busy,
    /// file が読める形でない（触らない）。
    Unreadable,
}

/// 古さの印の file を整える（`lifecycle.stale.lock` の内側で読み・`gone` が真の印を消し・一時 file からの rename で置き換える）。
/// file が無ければ `{"version":1,"marks":[]}` を作る（最初の全部の書き直しで作り、それからは消さない）。
pub fn settle(state_dir: &Path, gone: impl Fn(&Mark) -> bool, policy: LockPolicy) -> Settled {
    let dir = fleet_dir(state_dir);
    let Some(_held) = hold(&dir, STALE_LOCK, policy) else { return Settled::Busy };
    let marks = match read_stale(state_dir) {
        Stale::Absent => Vec::new(),
        Stale::Marks(found) => found,
        Stale::Unreadable => return Settled::Unreadable,
    };
    let kept: Vec<Mark> = marks.iter().filter(|mark| !gone(mark)).cloned().collect();
    let absent = !dir.join(STALE_FILE).is_file();
    if (absent || kept.len() != marks.len()) && publish(&dir, STALE_FILE, &stale_text(&kept)).is_err() {
        return Settled::Unreadable;
    }
    Settled::Done
}

/// 読みの結果（読めた・無いか読める形でない・読みそのものが落ちた）。
pub enum Face<T> {
    /// 読めた。
    Found(T),
    /// 無いか器の読める形でない（`unmeasured` で書き直す）。
    Missing,
    /// 読みそのものが落ちた（git が撃てない・file を読めない・書き直さない）。
    Fault,
}

/// 契約表の行と、pointer ごとの write-set。
pub type Rows = (Vec<Row>, Vec<(String, Vec<String>)>);

/// `git ls-tree` の出力の path の列（`-z`）。
fn listed(repo: &Path, args: &[&str]) -> Option<Vec<String>> {
    let out = git_bytes(repo, args)?;
    Some(String::from_utf8_lossy(&out).split('\0').filter(|name| !name.is_empty()).map(str::to_owned).collect())
}

/// 契約表の行（main の sha の tree の宣言が名乗る置き場〔既定の `docs/design/` 直下の `.md` と宣言の `contract-tables` の項目〕を、
/// sha の tree の全 path から [`design_docs`] の 1 本で絞って読む・行 id は `<置き場の path>#<id>` の pointer の字）と、pointer ごとの
/// write-set。sha の宣言を読めない周は表が在っても `Missing`（既定の置き場に倒さない・設計 contract-source.md §69 行 cc）。
pub fn read_rows(repo: &Path, sha: &str) -> Face<Rows> {
    let places = TablePlaces::at(repo, sha);
    let Some(items) = places.items() else { return Face::Missing };
    let Some(names) = listed(repo, &["ls-tree", "-r", "--name-only", "-z", sha]) else { return Face::Fault };
    let docs = design_docs(&names, items);
    if docs.is_empty() {
        return Face::Missing;
    }
    let (mut rows, mut write_sets) = (Vec::new(), Vec::new());
    for doc in docs {
        let Some(body) = git_bytes(repo, &["show", &format!("{sha}:{doc}")]) else { return Face::Fault };
        let Ok((found, _)) = table_rows(doc, &String::from_utf8_lossy(&body)) else { return Face::Missing };
        for row in found {
            let pointer = format!("{doc}#{}", row.id);
            write_sets.push((pointer.clone(), row.write_set));
            rows.push(Row { pointer, req: row.req });
        }
    }
    Face::Found((rows, write_sets))
}

/// 要件 id（main の sha の tree の宣言が名指す要件面か既定の path・`.html` の anchor など拡張子で読み手が分かれる）。
pub fn read_srs(repo: &Path, sha: &str) -> Face<Vec<String>> {
    let path = requirements_at_sha(repo, sha);
    let Some(entries) = listed(repo, &["ls-tree", "--name-only", "-z", sha, "--", &path]) else { return Face::Fault };
    if entries.is_empty() {
        return Face::Missing;
    }
    let Some(body) = git_bytes(repo, &["show", &format!("{sha}:{path}")]) else { return Face::Fault };
    match requirement_ids(&path, &String::from_utf8_lossy(&body)) {
        Ok(ids) => Face::Found(ids.into_iter().collect()),
        Err(_) => Face::Missing,
    }
}

/// 開いた契約か（開いていて・問いでも memo でも epic でもない）。
pub fn is_open_contract(issue: &Issue) -> bool {
    issue.status != "closed" && !is_question(issue) && issue.kind != "decision" && !is_memo(issue) && issue.kind != "epic"
}

/// 開いた契約の設計 pointer が指す行の write-set の項目（`+` `-` `=` の印つきの字のまま）。
pub fn open_write_set(issues: &[Issue], write_sets: &[(String, Vec<String>)]) -> Vec<String> {
    let pointers: Vec<&str> = issues.iter().filter(|issue| is_open_contract(issue)).filter_map(|issue| pointer_text(&issue.acceptance)).collect();
    write_sets.iter().filter(|(pointer, _)| pointers.contains(&pointer.as_str())).flat_map(|(_, items)| items.clone()).collect()
}

/// 設計 pointer から bead を引く（開いた bead が先・無ければ閉じた時刻が最も新しい bead）。
fn bead_of<'i>(issues: &'i [Issue], pointer: &str) -> Option<&'i Issue> {
    let mut named: Vec<&Issue> = issues.iter().filter(|issue| pointer_text(&issue.acceptance) == Some(pointer)).collect();
    named.sort_by_key(|issue| (issue.status == "closed", std::cmp::Reverse(issue.closed_at.clone())));
    named.first().copied()
}

/// main の commit（切り替えの線の main の sha の次から先端まで・first-parent・古い順）を trailer つきで読む。
pub fn read_commits(repo: &Path, from: &str, to: &str, issues: &[Issue]) -> Option<Vec<Commit>> {
    let range = format!("{from}..{to}");
    let raw = git_bytes(repo, &["log", "--first-parent", "--format=%H%x1f%ct%x1f%B%x1e", &range])?;
    let body = String::from_utf8_lossy(&raw);
    let mut commits: Vec<Commit> = body.split('\x1e').filter_map(|record| commit_of(record.trim_start_matches('\n'), issues)).collect();
    commits.reverse();
    Some(commits)
}

/// commit 1 本（`run:`・発端・契約の 3 つの trailer を読む・契約の trailer は pointer を bead id へ引き直し、引けない pointer は渡さない）。
fn commit_of(record: &str, issues: &[Issue]) -> Option<Commit> {
    let mut fields = record.splitn(3, '\x1f');
    let sha = fields.next()?.trim().to_owned();
    let at = fields.next()?.trim().parse().ok()?;
    let (source, contract) = (source_key(), contract_key());
    let mut found = Commit { sha, at, sources: Vec::new(), run: None, contracts: Vec::new() };
    for line in fields.next().unwrap_or_default().lines().map(str::trim_end) {
        if let Some(run) = line.strip_prefix(RUN_TRAILER).map(str::trim).filter(|run| !run.is_empty()) {
            found.run.get_or_insert_with(|| run.to_owned());
        } else if let Some(ids) = line.strip_prefix(source.as_str()) {
            found.sources.extend(ids.split_whitespace().map(str::to_owned));
        } else if let Some(pointer) = line.strip_prefix(contract.as_str()) {
            found.contracts.extend(bead_of(issues, pointer.trim()).map(|issue| issue.id.clone()));
        }
    }
    (!found.sha.is_empty()).then_some(found)
}

/// `verdict:<語>` の語（`,account:<label>` などの後ろは読まない）。
fn verdict_word(detail: &str) -> Option<String> {
    let word = detail.split_whitespace().find_map(|token| token.strip_prefix("verdict:"))?;
    word.split(',').next().map(str::to_owned)
}

/// detail の頭（`:`・`,`・空白の前・例 `rebase-conflict`）。
fn detail_head(detail: &str) -> Option<String> {
    let head = detail.split(|found: char| matches!(found, ':' | ',') || found.is_whitespace()).next().unwrap_or_default();
    (!head.is_empty()).then(|| head.to_owned())
}

/// bead ごとの最新の便（最初の event が最も後の便）。段と時刻と detail は便の表と段の行から、札の生死は運転手の札から読む。
pub fn latest_runs(state_dir: &Path, events: &[Event], state: &State) -> Vec<Latest> {
    let first = |run: &str| events.iter().position(|event| event.run == run).unwrap_or(0);
    let mut newest: BTreeMap<&str, (usize, &str)> = BTreeMap::new();
    for (id, run) in &state.runs {
        let at = first(id);
        if newest.get(run.bead.as_str()).is_none_or(|(best, _)| *best < at) {
            newest.insert(run.bead.as_str(), (at, id.as_str()));
        }
    }
    newest.values().filter_map(|(_, id)| state.runs.get(*id)).map(|run| latest_of(state_dir, events, run)).collect()
}

/// 便 1 本の最新の現在地。
fn latest_of(state_dir: &Path, events: &[Event], run: &super::Run) -> Latest {
    let at_stage = events.iter().rev().find(|event| event.run == run.id && event.stage == Some(run.stage));
    let detail = at_stage.and_then(|event| event.detail.clone());
    let landed = run.stage.as_str() == "Landed";
    let terminal = detail.as_deref().filter(|_| landed).map(|found| {
        TERMINAL_TOKENS.iter().find(|stem| found.starts_with(**stem)).map_or_else(|| found.to_owned(), |stem| (*stem).to_owned())
    });
    Latest {
        run: run.id.clone(),
        bead: run.bead.clone(),
        stage: run.stage,
        verdict: detail.as_deref().and_then(verdict_word),
        detail_head: run.detail.as_deref().and_then(detail_head),
        terminal,
        alive: live_driver(state_dir, &run.id).is_some(),
        ts: at_stage.map_or_else(|| run.updated.clone(), |event| event.ts.clone()),
    }
}

/// 受付の断りの最新（bead ごと・断りの後に便の起動が在れば `run_after`）。
pub fn refusals_of(events: &[Event]) -> Vec<Refused> {
    let mut latest: BTreeMap<&str, usize> = BTreeMap::new();
    for (at, event) in events.iter().enumerate() {
        if matches!(event.case, Some(Case::Refused { .. })) && !event.bead.is_empty() {
            latest.insert(event.bead.as_str(), at);
        }
    }
    latest
        .into_iter()
        .filter_map(|(bead, at)| {
            let event = events.get(at)?;
            let Some(Case::Refused { refuse }) = &event.case else { return None };
            let run_after = events.iter().skip(at + 1).any(|later| later.kind.as_str() == "RunCreated" && later.bead == bead);
            Some(Refused { bead: bead.to_owned(), name: refuse.clone(), ts: event.ts.clone(), run_after })
        })
        .collect()
}

/// 裁定 event の結び（問い id と裁定 id の組）。
pub fn bindings_of(events: &[Event]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|event| match &event.case {
            Some(Case::Ruling { ruling, .. }) if !event.bead.is_empty() => Some((event.bead.clone(), ruling.clone())),
            _ => None,
        })
        .collect()
}

/// 時刻の字（秒も ms も）を UNIX 秒へ。
pub fn secs_of(ts: &str) -> Option<u64> {
    epoch_of(ts).or_else(|| super::epoch_ms_of(ts).map(|ms| ms / 1_000))
}

/// `unmeasured` の理由: 未反映の裁定の置き場が在って読めない（部品の種類は問い）。
pub const UNMEASURED_UNREFLECTED: &str = "unreflected-unreadable";

/// `unmeasured` の理由: 置き場の anchor が 2 つ以上（設計 §20）。
pub const UNMEASURED_MULTI_ANCHOR: &str = "multi-anchor";

/// [`UNMEASURED_MULTI_ANCHOR`] で名指す部品の種類（§2 の種類の順・発話は置き場の event log 全部から作るので外す）。
pub const MULTI_ANCHOR_PARTS: [Part; 8] =
    [Part::Question, Part::Memo, Part::Contract, Part::Run, Part::Row, Part::Requirement, Part::Epic, Part::Commit];

/// 置き場の anchor の数え（閉じた 3 値・設計 §20）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnchorCensus {
    /// 1 つ。
    One,
    /// 2 つ以上。
    Many,
    /// 書き直す repo が別の置き場を名乗る。
    Foreign,
}

/// path の正規化した字（正規化できない path は字のまま）。
fn canon(path: &Path) -> String {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()).display().to_string()
}

/// `root` の設定が、正規化した字が `mine` と違う在る dir を名乗るか（設定が無い・読めない・在らない path は偽）。
fn names_other(root: &Path, mine: &str) -> bool {
    named_state_dir(root).and_then(|named| fs::canonicalize(named).ok()).is_some_and(|named| named.display().to_string() != mine)
}

/// 置き場の anchor を数える（`events` の登録 row の anchor から別の置き場を名乗る anchor を除き、`repo` と合わせて 2 つ以上なら
/// [`AnchorCensus::Many`]・1 つで `repo` が別の置き場を名乗れば [`AnchorCensus::Foreign`]・ほかは [`AnchorCensus::One`]）。
pub(crate) fn census_anchors(state_dir: &Path, repo: &Path, events: &[Event]) -> AnchorCensus {
    let mine = canon(state_dir);
    let mut set: BTreeSet<String> = BTreeSet::new();
    for latest in replay(events).registrations.values() {
        let anchor = Path::new(&latest.registration.anchor);
        if !names_other(anchor, &mine) {
            set.insert(canon(anchor));
        }
    }
    set.insert(canon(repo));
    if set.len() >= 2 {
        AnchorCensus::Many
    } else if names_other(repo, &mine) {
        AnchorCensus::Foreign
    } else {
        AnchorCensus::One
    }
}

/// 閉じて未反映の裁定を持つ問いの bead id（置き場・台帳の接頭辞・台帳の全件から引く・置き場の無い周は空・読めない周は `Unreadable`）。
pub(crate) fn unreflected_questions(state_dir: &Path, prefix: Option<&str>, issues: &[Issue]) -> Asked {
    let closed: Vec<Question<'_>> =
        issues.iter().filter(|issue| issue.status == "closed" && is_question(issue)).map(|issue| Question { id: &issue.id, notes: &issue.notes }).collect();
    asked(state_dir, prefix, &closed)
}

/// 処置の無い判定を持つ memo の bead id（台帳の順）: 開いた memo のうち、event log の最後の判定の語が promote か close で、
/// 台帳の updated_at がその判定の ts より後でないもの（updated_at か ts が読めない memo は渡す）。
pub(crate) fn verdict_unhandled(issues: &[Issue], events: &[Event]) -> Vec<String> {
    let mut last: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    for event in events.iter().filter(|event| matches!(event.case, Some(Case::Judged))) {
        last.insert(&event.bead, (event.detail.as_deref().unwrap_or_default(), &event.ts));
    }
    let handled = |issue: &Issue, ts: &str| match (issue.updated_at.as_deref().and_then(secs_of), secs_of(ts)) {
        (Some(updated), Some(judged)) => updated > judged,
        _ => false,
    };
    issues
        .iter()
        .filter(|issue| issue.status != "closed" && is_memo(issue))
        .filter(|issue| last.get(issue.id.as_str()).is_some_and(|(word, ts)| [Word::Promote, Word::Close].iter().any(|actionable| actionable.as_str() == *word) && !handled(issue, ts)))
        .map(|issue| issue.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        add_mark, census_anchors, events_order, ledger_is_newer, ledger_order, main_is_descendant, main_order, read_events, read_ledger, read_main, read_marks,
        read_stale, settle, unreflected_questions, verdict_unhandled, Added, AnchorCensus, Events, Kind, Ledger, Mark, Settled, Stale, Value, JSON_FILE, MAIN_REF,
        STALE_FILE,
    };
    use crate::fleet::store::LockPolicy;
    use crate::fleet::Event;
    use crate::pipe::dispatch::unreflected::Asked;
    use crate::seat::ledger::{issues_of, Issue};
    use std::cmp::Ordering;
    use std::path::{Path, PathBuf};

    /// 1 つ目の sha。
    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// もう 1 つの sha。
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    /// 使い捨ての dir。
    fn scratch(name: &str) -> PathBuf {
        crate::pipe::fixture::scratch(&format!("lifecycle-mark-{name}"))
    }

    /// 書く（親 dir も作る）。
    fn put(path: &Path, text: &str) {
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(path));
        assert!(std::fs::write(path, text).is_ok(), "{} を書けた", path.display());
    }

    /// embedded の台帳の fixture（metadata.json と manifest）。
    fn noms_repo(name: &str, manifest: &str) -> PathBuf {
        let repo = scratch(name);
        put(&repo.join(".beads/metadata.json"), r#"{"dolt_mode":"embedded","dolt_database":"beads"}"#);
        put(&repo.join(".beads/embeddeddolt/beads/.dolt/noms/manifest"), manifest);
        repo
    }

    /// 実物の bd の manifest の 1 行（`5:<second>:<lock>:<root>:<gc の世代>:` の後に `pairs`）。
    fn real_manifest(second: &str, root: &str, collected: &str, pairs: &str) -> String {
        format!("5:{second}:{}:{root}:{collected}:{pairs}", "l".repeat(32))
    }

    /// 32 桁の root。
    fn root_of_32() -> String {
        "0123456789abcdef0123456789abcdef".to_owned()
    }

    /// 32 桁の table の file 名（`seed` の字を並べる・journal の名 `v…` と違う）。
    fn table_name(seed: char) -> String {
        seed.to_string().repeat(32)
    }

    /// 実物の形の組（table 2 つと journal 1 つ）。
    fn real_pairs() -> String {
        format!("{}:10:{}:5:{}:7", table_name('a'), table_name('b'), "v".repeat(32))
    }

    /// 実物の bd の manifest（2 つ目が `__DOLT__`・root は 4 つ目・gc の世代は 5 つ目・6 つ目から組）を読める。
    #[test]
    fn lifecycle_mark_ledger_reads_the_real_bd_manifest_form() {
        let zeros = "0".repeat(32);
        let manifest = real_manifest("__DOLT__", &root_of_32(), &zeros, &real_pairs());
        let Some(Ledger::Noms { root, generation, chunks }) = read_ledger(&noms_repo("real-form", &manifest)) else {
            panic!("実物の形の manifest を読める");
        };
        assert_eq!(root, root_of_32(), "root は 4 つ目の字");
        assert_eq!(chunks, 22, "chunks は組の数の和");
        let keyed = format!("{zeros}\n{}\n{}", table_name('a'), table_name('b'));
        assert_eq!(generation, crate::hook::vessel::digest::fnv1a_64(keyed.as_bytes()), "gen は gc の世代と journal でない file 名の digest");
    }

    /// journal だけ・table つき・gc の世代違いの 3 形は同じ root を返し、gen と chunks は材料の通りに決まる。
    #[test]
    fn lifecycle_mark_ledger_noms_reads_the_manifest_of_three_shapes() {
        let zeros = "0".repeat(32);
        let journal = format!("{}:7", "v".repeat(32));
        let Some(Ledger::Noms { root, generation, chunks }) = read_ledger(&noms_repo("noms-journal", &real_manifest("__DOLT__", &root_of_32(), &zeros, &journal))) else {
            panic!("journal だけの manifest を読める");
        };
        assert_eq!((root.as_str(), chunks), (root_of_32().as_str(), 7), "root と chunks");
        assert_eq!(generation, crate::hook::vessel::digest::fnv1a_64(zeros.as_bytes()), "gen は gc の世代だけの digest（journal を除く）");
        let Some(Ledger::Noms { root: table_root, generation: table_gen, chunks: table_chunks }) =
            read_ledger(&noms_repo("noms-table", &real_manifest("__DOLT__", &root_of_32(), &zeros, &real_pairs())))
        else {
            panic!("table file つきの manifest を読める");
        };
        assert_eq!((table_root, table_chunks), (root_of_32(), 22), "root は同じで chunks は journal を含む和");
        let keyed = format!("{zeros}\n{}\n{}", table_name('a'), table_name('b'));
        assert_eq!(table_gen, crate::hook::vessel::digest::fnv1a_64(keyed.as_bytes()), "gen は gc の世代と journal を除く名を改行でつないだ digest");
        let Some(Ledger::Noms { root: next_root, generation: next_gen, chunks: next_chunks }) =
            read_ledger(&noms_repo("noms-gc", &real_manifest("__DOLT__", &root_of_32(), &"1".repeat(32), &real_pairs())))
        else {
            panic!("gc の世代違いの manifest を読める");
        };
        assert_eq!((next_root, next_chunks), (root_of_32(), 22), "root と chunks は同じ");
        assert_ne!(next_gen, table_gen, "gc の世代が違えば gen が違う");
    }

    /// manifest が読めない形（2 つ目が `__DOLT__` でない・欄が足りない・組が奇数・数でない・root が空）と metadata の崩れは読めない（`None`）。
    #[test]
    fn lifecycle_mark_ledger_refuses_broken_manifests_and_metadata() {
        let zeros = "0".repeat(32);
        let readable = real_manifest("__DOLT__", &root_of_32(), &zeros, &real_pairs());
        assert!(matches!(read_ledger(&noms_repo("readable", &readable)), Some(Ledger::Noms { .. })), "対照: 2 つ目が __DOLT__ なら読める");
        let ld = real_manifest("__LD_1__", &root_of_32(), &zeros, &real_pairs());
        let odd = real_manifest("__DOLT__", &root_of_32(), &zeros, &format!("{}:10:{}", table_name('a'), table_name('b')));
        let nan = real_manifest("__DOLT__", &root_of_32(), &zeros, &format!("{}:many", table_name('a')));
        let noroot = real_manifest("__DOLT__", "", &zeros, &real_pairs());
        for (name, manifest) in [("ld", ld.as_str()), ("short", "5:__DOLT__:lock"), ("odd", odd.as_str()), ("nan", nan.as_str()), ("noroot", noroot.as_str())] {
            assert_eq!(read_ledger(&noms_repo(name, manifest)), None, "{name}");
        }
        let repo = noms_repo("broken-meta", &readable);
        put(&repo.join(".beads/metadata.json"), "{");
        assert_eq!(read_ledger(&repo), None, "metadata.json が読めない");
        put(&repo.join(".beads/metadata.json"), r#"{"dolt_mode":"embedded"}"#);
        assert_eq!(read_ledger(&repo), None, "embedded で db の名が無い");
        assert_eq!(read_ledger(&scratch("no-ledger")), None, "台帳が無い");
    }

    /// store が無い台帳は `issues.jsonl` の長さと更新時刻の files の形・順は長さ、同じ長さは更新時刻。
    #[test]
    fn lifecycle_mark_ledger_files_form_orders_by_len_then_mtime() {
        let repo = scratch("files");
        put(&repo.join(".beads/issues.jsonl"), "abc");
        let Some(Ledger::Files { len, mtime_ns }) = read_ledger(&repo) else { panic!("files の形を読める") };
        assert_eq!(len, 3, "byte 長");
        assert!(mtime_ns > 0, "更新時刻の ns");
        let files = |len, mtime_ns| Ledger::Files { len, mtime_ns };
        assert_eq!(ledger_order(&files(4, 1), &files(3, 9)), Some(Ordering::Greater), "長さが先");
        assert_eq!(ledger_order(&files(3, 2), &files(3, 9)), Some(Ordering::Less), "同じ長さは更新時刻");
        assert_eq!(ledger_order(&files(3, 9), &files(3, 9)), Some(Ordering::Equal), "同じ");
        let noms = Ledger::Noms { root: "r".to_owned(), generation: "g".to_owned(), chunks: 1 };
        assert_eq!(ledger_order(&files(3, 9), &noms), None, "形が違う組は順を持たない");
    }

    /// 台帳の順: 同じ gen は chunks の大小・同じ chunks で root が違う組と gen の違う組は順を持たない（捨てる判定では古くないと読む）。
    #[test]
    fn lifecycle_mark_ledger_order_is_chunks_within_one_generation() {
        let noms = |root: &str, generation: &str, chunks| Ledger::Noms { root: root.to_owned(), generation: generation.to_owned(), chunks };
        assert_eq!(ledger_order(&noms("a", "g", 5), &noms("b", "g", 4)), Some(Ordering::Greater), "同じ gen は chunks の大小");
        assert_eq!(ledger_order(&noms("a", "g", 3), &noms("b", "g", 4)), Some(Ordering::Less));
        assert_eq!(ledger_order(&noms("a", "g", 4), &noms("b", "g", 4)), None, "同じ chunks で root が違う組は順を持たない");
        assert_eq!(ledger_order(&noms("a", "g", 4), &noms("a", "g", 4)), Some(Ordering::Equal));
        assert_eq!(ledger_order(&noms("a", "g1", 9), &noms("a", "g2", 1)), None, "gen が違う組は順を持たない");
        assert!(ledger_is_newer(&noms("a", "g1", 1), &noms("a", "g2", 9)), "印を消す判定は gen が違えば新しい");
        assert!(ledger_is_newer(&noms("a", "g", 5), &noms("a", "g", 4)), "同じ gen は chunks が多ければ新しい");
        assert!(!ledger_is_newer(&noms("b", "g", 4), &noms("a", "g", 4)), "同じ gen で同じ chunks は新しくない");
    }

    /// event log の印は長さと 1 行目の ts・無い log は長さ 0 と head なし・順は head が同じときだけ長さの大小。
    #[test]
    fn lifecycle_mark_events_reads_len_and_head_and_orders_by_len() {
        let state = scratch("events");
        assert_eq!(read_events(&state), Some(Events { len: 0, head: None }), "無い log は長さ 0");
        let first = "{\"ts\":\"2026-09-30T00:00:00Z\",\"kind\":\"RunCreated\"}\n";
        put(&state.join("fleet/events.jsonl"), &format!("{first}{{\"ts\":\"2026-09-30T00:00:01Z\"}}\n"));
        let read = read_events(&state).unwrap_or(Events { len: 0, head: None });
        assert_eq!(read.head.as_deref(), Some("2026-09-30T00:00:00Z"), "1 行目の ts");
        assert_eq!(read.len, (first.len() + "{\"ts\":\"2026-09-30T00:00:01Z\"}\n".len()) as u64, "byte 長");
        let events = |len, head: &str| Events { len, head: Some(head.to_owned()) };
        assert_eq!(events_order(&events(9, "h"), &events(5, "h")), Some(Ordering::Greater), "同じ head は長さの大小");
        assert_eq!(events_order(&events(5, "h"), &events(5, "h")), Some(Ordering::Equal));
        assert_eq!(events_order(&events(1, "畳み後"), &events(99, "h")), None, "head が違えば別の log（畳みで縮んだ周を含む）");
    }

    /// `.git` が ref の file と packed-refs と gitfile だけ（HEAD も object も無い）でも main を読める（git を撃つ実装は落ちる）。
    #[test]
    fn lifecycle_mark_main_reads_loose_ref_and_packed_refs_without_git() {
        let repo = scratch("main-loose");
        put(&repo.join(".git/refs/remotes/origin/main"), &format!("{SHA_A}\n"));
        assert_eq!(read_main(&repo).as_deref(), Some(SHA_A), "loose の ref");
        put(&repo.join(".git/packed-refs"), &format!("# pack-refs\n{SHA_B} refs/remotes/origin/other\n{SHA_B} {MAIN_REF}\n^{SHA_A}\n"));
        assert_eq!(read_main(&repo).as_deref(), Some(SHA_A), "loose が先");
        assert!(std::fs::remove_file(repo.join(".git/refs/remotes/origin/main")).is_ok());
        assert_eq!(read_main(&repo).as_deref(), Some(SHA_B), "loose が無ければ packed-refs");
        put(&repo.join(".git/packed-refs"), &format!("{SHA_A} refs/remotes/origin/other\n"));
        assert_eq!(read_main(&repo), None, "どちらにも無い");
        put(&repo.join(".git/refs/remotes/origin/main"), "not-a-sha\n");
        assert_eq!(read_main(&repo), None, "loose が 40 桁の 16 進でない");
    }

    /// worktree の gitfile: `.git` の file が指す dir の `commondir` が指す common dir の ref を読む（`commondir` が無ければ指す dir 自身）。
    #[test]
    fn lifecycle_mark_main_follows_a_worktree_gitfile_to_the_common_dir() {
        let root = scratch("main-worktree");
        put(&root.join("common/refs/remotes/origin/main"), &format!("{SHA_B}\n"));
        put(&root.join("common/worktrees/w/commondir"), "../..\n");
        put(&root.join("wt/.git"), &format!("gitdir: {}\n", root.join("common/worktrees/w").display()));
        assert_eq!(read_main(&root.join("wt")).as_deref(), Some(SHA_B), "commondir の相対 path を解く");
        put(&root.join("bare/refs/remotes/origin/main"), &format!("{SHA_A}\n"));
        put(&root.join("wt2/.git"), &format!("gitdir: {}\n", root.join("bare").display()));
        assert_eq!(read_main(&root.join("wt2")).as_deref(), Some(SHA_A), "commondir の無い gitdir はそのまま common dir");
        put(&root.join("wt3/.git"), "garbage\n");
        assert_eq!(read_main(&root.join("wt3")), None, "gitfile が読めない");
    }

    /// 3 つの印は読む順に最初に読めなかった入力の語を返す（ledger → events → main）。
    #[test]
    fn lifecycle_mark_reads_three_marks_and_names_the_first_unreadable() {
        let state = scratch("marks-state");
        let repo = scratch("marks-repo");
        assert_eq!(read_marks(&state, &repo), Err("ledger"), "台帳も main も読めない周は先の語");
        put(&repo.join(".beads/issues.jsonl"), "[]");
        assert_eq!(read_marks(&state, &repo), Err("main"), "台帳と event log が読めて main が無い");
        put(&repo.join(".git/refs/remotes/origin/main"), SHA_A);
        let marks = read_marks(&state, &repo).unwrap_or_else(|word| panic!("3 つとも読める: {word}"));
        assert_eq!((marks.main.as_str(), marks.events.len), (SHA_A, 0));
        put(&state.join("fleet/events.jsonl"), "x\n");
        let events_dir = state.join("fleet/events.jsonl");
        assert!(std::fs::remove_file(&events_dir).is_ok() && std::fs::create_dir_all(&events_dir).is_ok(), "log を dir に置き換える");
        assert_eq!(read_marks(&state, &repo), Err("events"), "event log が読めない周");
    }

    /// main の順は git の祖先の関係（同じ sha は同じ・祖先の関係に無い組と git を撃てない周は順を持たない）。
    #[test]
    fn lifecycle_mark_main_order_follows_git_ancestry() {
        let repo = crate::pipe::fixture::scratch("lifecycle-mark-ancestry");
        for args in [&["init", "-q", "-b", "main"][..], &["config", "user.name", "t"], &["config", "user.email", "t@example.invalid"], &["config", "commit.gpgsign", "false"]] {
            assert!(crate::pipe::git_ok(&repo, args), "{args:?}");
        }
        let commit = |message: &str| {
            assert!(crate::pipe::git_ok(&repo, &["commit", "-q", "--allow-empty", "-m", message]), "commit");
            crate::pipe::git_bytes(&repo, &["rev-parse", "HEAD"]).map(|out| String::from_utf8_lossy(&out).trim().to_owned()).unwrap_or_default()
        };
        let (base, child) = (commit("base"), commit("child"));
        assert!(crate::pipe::git_ok(&repo, &["checkout", "-q", "-b", "side", &base]), "side");
        let side = commit("side");
        assert_eq!(main_order(&repo, &child, &base), Some(Ordering::Greater), "子孫は新しい");
        assert_eq!(main_order(&repo, &base, &child), Some(Ordering::Less), "祖先は古い");
        assert_eq!(main_order(&repo, &child, &child), Some(Ordering::Equal), "同じ sha");
        assert_eq!(main_order(&repo, &side, &child), None, "祖先の関係に無い組は順を持たない");
        assert!(main_is_descendant(&repo, &child, &base), "真の子孫");
        assert!(!main_is_descendant(&repo, &base, &base), "同じ sha は真の子孫でない");
        assert!(!main_is_descendant(&repo, &side, &child), "別の枝は子孫でない");
        assert_eq!(main_order(Path::new("/nonexistent-lifecycle-mark-dir"), &side, &child), None, "git を撃てない周は順を持たない");
    }

    /// 古さの印の 3 種類の字・種類ごとに 1 つ・後の印が置き換え・出力が無い周は何も書かない。
    #[test]
    fn lifecycle_mark_stale_adds_one_per_kind_and_replaces_with_the_later_mark() {
        let state = scratch("stale-add");
        let policy = LockPolicy { retry_ms: 100, stale_ms: 600_000 };
        let ledger = |chunks| Ledger::Noms { root: "r".to_owned(), generation: "g".to_owned(), chunks };
        let mark = |kind, value| Mark { kind, at: "2026-09-30T00:00:00Z".to_owned(), value };
        assert_eq!(add_mark(&state, &mark(Kind::LedgerGate, Value::Ledger(ledger(1))), policy), Added::NoOutput, "出力が無い");
        assert_eq!(read_stale(&state), Stale::Absent, "何も書かない");
        put(&state.join("fleet").join(JSON_FILE), "{}");
        assert_eq!(add_mark(&state, &mark(Kind::LedgerGate, Value::Ledger(ledger(1))), policy), Added::Added);
        assert_eq!(add_mark(&state, &mark(Kind::MergeGate, Value::Main(SHA_A.to_owned())), policy), Added::Added);
        assert_eq!(add_mark(&state, &mark(Kind::Unreadable, Value::Reason("main".to_owned())), policy), Added::Added);
        assert_eq!(add_mark(&state, &mark(Kind::LedgerGate, Value::Ledger(ledger(2))), policy), Added::Added, "同じ種類は置き換え");
        let Stale::Marks(marks) = read_stale(&state) else { panic!("読める") };
        assert_eq!(marks.iter().map(|found| found.kind.as_str()).collect::<Vec<_>>(), ["ledger-gate", "merge-gate", "unreadable"], "種類ごとに 1 つ・宣言順");
        assert_eq!(marks.first().map(|found| found.value.clone()), Some(Value::Ledger(ledger(2))), "後の印が前の印を置き換える");
        assert_eq!(marks.get(2).map(|found| found.value.clone()), Some(Value::Reason("main".to_owned())));
        assert_eq!(marks.first().and_then(Mark::at_secs), Some(1_790_726_400), "at の秒");
    }

    /// 読めない古さの印の file と取れない lock は `Unreadable` と `Busy`・settle は file が無ければ空の marks を作り、消す印だけを消す。
    #[test]
    fn lifecycle_mark_stale_settle_creates_empty_marks_and_removes_only_gone_marks() {
        let state = scratch("stale-settle");
        let policy = LockPolicy { retry_ms: 50, stale_ms: 600_000 };
        assert_eq!(settle(&state, |_| true, policy), Settled::Done, "無ければ作る");
        assert_eq!(read_stale(&state), Stale::Marks(Vec::new()), "空の marks");
        put(&state.join("fleet").join(JSON_FILE), "{}");
        let at = "2026-09-30T00:00:00Z".to_owned();
        for kind in [Kind::MergeGate, Kind::Unreadable] {
            let value = if kind == Kind::MergeGate { Value::Main(SHA_A.to_owned()) } else { Value::Reason("table".to_owned()) };
            assert_eq!(add_mark(&state, &Mark { kind, at: at.clone(), value }, policy), Added::Added);
        }
        assert_eq!(settle(&state, |mark| mark.kind == Kind::MergeGate, policy), Settled::Done);
        let Stale::Marks(rest) = read_stale(&state) else { panic!("読める") };
        assert_eq!(rest.iter().map(|found| found.kind).collect::<Vec<_>>(), [Kind::Unreadable], "条件に当たる印だけ消える");
        assert_eq!(settle(&state, |_| true, policy), Settled::Done);
        assert_eq!(read_stale(&state), Stale::Marks(Vec::new()), "消しきっても file は残る（無いのと 0 件を見分ける）");
        put(&state.join("fleet").join(STALE_FILE), "{\"version\":2,\"marks\":[]}");
        assert_eq!(read_stale(&state), Stale::Unreadable, "版が違う");
        assert_eq!(settle(&state, |_| true, policy), Settled::Unreadable, "読めない file は触らない");
        let mark = Mark { kind: Kind::Unreadable, at, value: Value::Reason("ledger".to_owned()) };
        assert_eq!(add_mark(&state, &mark, policy), Added::Unreadable);
    }

    /// 生きた所有者が持つ `lifecycle.stale.lock` は取れず `Busy`（印は動かない）・死んだ所有者の lock は外して取る。
    #[test]
    fn lifecycle_mark_stale_lock_is_busy_for_a_live_owner_and_reclaimed_from_a_dead_one() {
        let state = scratch("stale-lock");
        let policy = LockPolicy { retry_ms: 30, stale_ms: 1 };
        put(&state.join("fleet").join(JSON_FILE), "{}");
        let lock = state.join("fleet").join(super::STALE_LOCK);
        put(&lock, &format!("{}\n", std::process::id()));
        let mark = Mark { kind: Kind::Unreadable, at: "2026-09-30T00:00:00Z".to_owned(), value: Value::Reason("ledger".to_owned()) };
        assert_eq!(add_mark(&state, &mark, policy), Added::Busy, "生きた所有者は古くても外さない");
        assert_eq!(read_stale(&state), Stale::Absent, "印は動かない");
        put(&lock, "4194300 1\n");
        assert_eq!(add_mark(&state, &mark, policy), Added::Added, "死んだ所有者の lock は外して取る");
        assert!(!lock.exists(), "取った lock は外される");
    }

    /// bd の JSON の字から台帳を読む（歯の fixture は `Issue` を literal で組まない）。
    fn ledger_of_text(text: &str) -> Vec<Issue> {
        issues_of(text).unwrap_or_else(|| panic!("台帳を読める: {text}"))
    }

    /// 台帳の bead 1 本の JSON（`label` は空なら無し・`updated` は無ければ欄ごと無い）。
    fn bead_json(id: &str, status: &str, label: &str, notes: &str, updated: Option<&str>) -> String {
        let labels = if label.is_empty() { String::new() } else { format!(",\"labels\":[\"{label}\"]") };
        let updated = updated.map(|at| format!(",\"updated_at\":\"{at}\"")).unwrap_or_default();
        format!("{{\"id\":\"{id}\",\"status\":\"{status}\"{labels},\"notes\":\"{notes}\"{updated}}}")
    }

    /// 台帳の JSON の配列。
    fn ledger_json(beads: &[String]) -> Vec<Issue> {
        ledger_of_text(&format!("[{}]", beads.join(",")))
    }

    /// 裁定の行 1 本（notes の字・行の区切りは `\n`）。
    fn ruling(id: &str) -> String {
        format!("{id} | s2-q | 2026-09-30T00:00Z | chat | 逐語")
    }

    /// 未反映の置き場の file（`unreflected` の id だけ・母集団は `population`）。
    fn put_store(state: &Path, population: &[&str], unreflected: &[&str]) {
        let list = |ids: &[&str]| format!("[{}]", ids.iter().map(|id| format!("\"{id}\"")).collect::<Vec<_>>().join(","));
        let body = format!("{{\"schema\":1,\"sha\":\"{SHA_A}\",\"population\":{},\"unreflected\":{},\"table\":{{}}}}\n", list(population), list(unreflected));
        put(&state.join("pipe").join("unreflected"), &body);
    }

    /// 閉じた問いと散文だけの問いと開いた問いと label の無い bead の台帳で、置き場の無い周は空・読めない周は `Unreadable`・
    /// 読める周は未反映の id の行を持つ閉じた問いだけが入力の順で載る。
    #[test]
    fn unreflected_questions_pick_only_closed_questions_with_an_unreflected_ruling_row() {
        let question = "intake:question";
        let issues = ledger_json(&[
            bead_json("s2-open", "closed", question, &format!("{}\\n{}", ruling("batch:old"), ruling("batch:new")), None),
            bead_json("s2-written", "closed", question, &ruling("batch:old"), None),
            bead_json("s2-prose", "closed", question, "batch:new を後で写す", None),
            bead_json("s2-live", "open", question, &ruling("batch:new"), None),
            bead_json("s2-plain", "closed", "", &ruling("batch:new"), None),
            bead_json("s2-second", "closed", question, &ruling("batch:new"), None),
        ]);
        let state = scratch("unreflected-questions");
        assert_eq!(unreflected_questions(&state, Some("s2"), &issues), Asked::Ids(Vec::new()), "置き場の無い周は空");
        put(&state.join("pipe").join("unreflected"), "これは JSON でない");
        assert_eq!(unreflected_questions(&state, Some("s2"), &issues), Asked::Unreadable, "読めない置き場");
        put_store(&state, &["batch:new", "batch:old"], &["batch:new"]);
        assert_eq!(
            unreflected_questions(&state, Some("s2"), &issues),
            Asked::Ids(vec!["s2-open".to_owned(), "s2-second".to_owned()]),
            "写った id の問い・散文だけの問い・開いた問い・label の無い bead は載らない"
        );
        put_store(&state, &["batch:old"], &[]);
        assert_eq!(unreflected_questions(&state, Some("s2"), &issues), Asked::Ids(Vec::new()), "未反映が 0 件の置き場は空");
    }

    /// event の 1 行（MemoJudged・bead は memo の id・detail は判定の語）。
    fn judged_line(ts: &str, memo: &str, word: &str) -> Event {
        let line = format!("{{\"schema\":1,\"ts\":\"{ts}\",\"kind\":\"MemoJudged\",\"bead\":\"{memo}\",\"detail\":\"{word}\",\"host\":\"h\",\"actor\":\"machine\"}}");
        Event::from_line(&line).unwrap_or_else(|why| panic!("{line}: {why}"))
    }

    /// 最後の判定が promote か close の開いた memo だけが載り、keep と unparsed と判定の行の無い memo と閉じた memo と memo でない bead は載らない。
    #[test]
    fn verdict_unhandled_takes_the_last_word_of_an_open_memo() {
        let memo = "intake:memo";
        let issues = ledger_json(&[
            bead_json("m-promote", "open", memo, "", None),
            bead_json("m-close", "open", memo, "", None),
            bead_json("m-keep", "open", memo, "", None),
            bead_json("m-unparsed", "open", memo, "", None),
            bead_json("m-none", "open", memo, "", None),
            bead_json("m-closed", "closed", memo, "", None),
            bead_json("c-plain", "open", "", "", None),
        ]);
        let ts = "2026-10-01T00:00:00Z";
        let events: Vec<Event> = [("m-promote", "promote"), ("m-close", "close"), ("m-keep", "keep"), ("m-unparsed", "unparsed"), ("m-closed", "promote"), ("c-plain", "promote")]
            .iter()
            .map(|(bead, word)| judged_line(ts, bead, word))
            .collect();
        assert_eq!(verdict_unhandled(&issues, &events), ["m-promote".to_owned(), "m-close".to_owned()]);
    }

    /// 同じ memo の判定は最後の行が勝つ（promote → keep は無く、keep → promote は在る）。
    #[test]
    fn verdict_unhandled_follows_the_order_of_the_judgements() {
        let issues = ledger_json(&[bead_json("m-a", "open", "intake:memo", "", None), bead_json("m-b", "open", "intake:memo", "", None)]);
        let ts = "2026-10-01T00:00:00Z";
        let events = [
            judged_line(ts, "m-a", "promote"),
            judged_line(ts, "m-b", "keep"),
            judged_line(ts, "m-a", "keep"),
            judged_line(ts, "m-b", "promote"),
        ];
        assert_eq!(verdict_unhandled(&issues, &events), ["m-b".to_owned()], "後の行が勝つ");
    }

    /// updated_at が判定の ts と同じ memo は載り、1 秒後の memo は載らない（対）。updated_at の無い memo と読めない memo は載る。
    #[test]
    fn verdict_unhandled_drops_a_memo_written_after_the_judgement() {
        let memo = "intake:memo";
        let issues = ledger_json(&[
            bead_json("m-same", "open", memo, "", Some("2026-10-01T00:00:00Z")),
            bead_json("m-after", "open", memo, "", Some("2026-10-01T00:00:01Z")),
            bead_json("m-before", "open", memo, "", Some("2026-09-30T23:59:59Z")),
            bead_json("m-absent", "open", memo, "", None),
            bead_json("m-garbled", "open", memo, "", Some("昨日")),
        ]);
        let ts = "2026-10-01T00:00:00Z";
        let events: Vec<Event> = ["m-same", "m-after", "m-before", "m-absent", "m-garbled"].iter().map(|bead| judged_line(ts, bead, "close")).collect();
        let expected = ["m-same", "m-before", "m-absent", "m-garbled"].map(str::to_owned);
        assert_eq!(verdict_unhandled(&issues, &events), expected);
    }

    /// git の repo（`stateDir` を名乗る設定は `named` の字・無ければ設定しない）。
    fn census_repo(name: &str, named: Option<&str>) -> PathBuf {
        let repo = scratch(&format!("anchor-census-{name}"));
        assert!(crate::pipe::git_ok(&repo, &["init", "-q", "-b", "main"]), "git init");
        if let Some(value) = named {
            assert!(crate::pipe::git_ok(&repo, &["config", &format!("{}.stateDir", crate::name::NAME), value]), "設定を書ける");
        }
        repo
    }

    /// 登録（か退役）の event の 1 行（役割は orchestrator）。
    fn census_event(kind: &str, anchor: &Path) -> Event {
        let line = format!(
            "{{\"schema\":1,\"ts\":\"2026-10-02T00:00:00Z\",\"kind\":\"{kind}\",\"role\":\"orchestrator\",\"anchor\":\"{}\",\"target\":\"s:w\",\"account\":\"a\",\"launch\":\"l\",\"host\":\"h\",\"actor\":\"machine\"}}",
            anchor.display()
        );
        Event::from_line(&line).unwrap_or_else(|why| panic!("{line}: {why}"))
    }

    /// 登録が無い周・R だけの周・`..` を含む R の path は One で、設定の無い B を足すと Many。
    #[test]
    fn anchor_census_counts_the_registered_anchors_of_the_place() {
        let (state, repo, other) = (scratch("anchor-census-place"), census_repo("a-r", None), census_repo("a-b", None));
        assert_eq!(census_anchors(&state, &repo, &[]), AnchorCensus::One, "登録が無い");
        let registered = [census_event("SeatRegistered", &repo)];
        assert_eq!(census_anchors(&state, &repo, &registered), AnchorCensus::One, "R だけ");
        let leaf = repo.file_name().unwrap_or_default();
        assert_eq!(census_anchors(&state, &repo.join("..").join(leaf), &registered), AnchorCensus::One, ".. を含む R の path");
        let both = [registered[0].clone(), census_event("SeatRegistered", &other)];
        assert_eq!(census_anchors(&state, &repo, &both), AnchorCensus::Many, "設定の無い B を足す");
    }

    /// B の設定が別の在る dir なら One・置き場なら Many（対）・在らない path の C は Many・B の退役で One に戻る。
    #[test]
    fn anchor_census_leaves_out_the_anchors_of_other_places() {
        let (state, away) = (scratch("anchor-census-mine"), scratch("anchor-census-away"));
        let repo = census_repo("b-r", None);
        let other = census_repo("b-b", Some(&away.display().to_string()));
        let events = [census_event("SeatRegistered", &repo), census_event("SeatRegistered", &other)];
        assert_eq!(census_anchors(&state, &repo, &events), AnchorCensus::One, "B は別の置き場");
        assert!(crate::pipe::git_ok(&other, &["config", &format!("{}.stateDir", crate::name::NAME), &state.display().to_string()]), "設定を書き直せる");
        assert_eq!(census_anchors(&state, &repo, &events), AnchorCensus::Many, "B は同じ置き場");
        let gone = census_repo("b-c", Some(&away.join("absent").display().to_string()));
        assert_eq!(census_anchors(&state, &repo, &[events[0].clone(), census_event("SeatRegistered", &gone)]), AnchorCensus::Many, "在らない path の C");
        let retired = [census_event("SeatRegistered", &repo), census_event("SeatRegistered", &other), census_event("SeatRetired", &other)];
        assert_eq!(census_anchors(&state, &repo, &retired), AnchorCensus::One, "B の退役");
    }

    /// R が別の在る dir を名乗り登録が R だけなら Foreign で、設定の無い B を足すと Many（先に Many を判じる）。
    #[test]
    fn anchor_census_names_a_repo_of_another_place_foreign() {
        let (state, away) = (scratch("anchor-census-home"), scratch("anchor-census-elsewhere"));
        let repo = census_repo("c-r", Some(&away.display().to_string()));
        let registered = [census_event("SeatRegistered", &repo)];
        assert_eq!(census_anchors(&state, &repo, &registered), AnchorCensus::Foreign, "R だけ");
        let both = [registered[0].clone(), census_event("SeatRegistered", &census_repo("c-b", None))];
        assert_eq!(census_anchors(&state, &repo, &both), AnchorCensus::Many, "設定の無い B を足す");
    }

    /// 契約表の行 1 つの全文（`.toml` の置き場の本文・`.md` の区間の本文のどちらにも使う）。
    fn table_row(id: &str, write_set: &str) -> String {
        format!(
            "[[contract]]\nid = \"{id}\"\ntitle = \"t\"\nreq = [\"FR1\"]\nsection = \"1\"\nwrite-set = [\"{write_set}\"]\nsize = \"S\"\nverify = [\"cargo test\"]\ndone = \"d\"\n"
        )
    }

    /// 現在の HEAD の sha。
    fn head_of(repo: &Path) -> String {
        crate::pipe::git_bytes(repo, &["rev-parse", "HEAD"]).map(|out| String::from_utf8_lossy(&out).trim().to_owned()).unwrap_or_default()
    }

    /// 木を `files`（path と本文）と宣言の追加行で組んで commit し、その sha を返す（前の commit の file は消す）。
    fn commit_tree(repo: &Path, key: &str, files: &[(&str, String)]) -> String {
        for dir in ["contracts", "docs"] {
            let _ = std::fs::remove_dir_all(repo.join(dir));
        }
        let declaration = format!("schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git status\"]\n{key}");
        put(&repo.join(".vessel.toml"), &declaration);
        for (path, body) in files {
            put(&repo.join(path), body);
        }
        assert!(crate::pipe::git_ok(repo, &["add", "-A"]) && crate::pipe::git_ok(repo, &["commit", "-q", "--allow-empty", "-m", "c"]), "commit");
        head_of(repo)
    }

    /// 行の write-set を pointer ごとに引く（`Found` でなければ `None`）。
    fn write_sets(found: super::Face<super::Rows>) -> Option<Vec<(String, Vec<String>)>> {
        match found {
            super::Face::Found((_, sets)) => Some(sets),
            super::Face::Missing | super::Face::Fault => None,
        }
    }

    /// (a) key で `contracts/` を名乗り表を `contracts/x.toml` だけに置いた commit は `contracts/x.toml#a` と行の write-set を返す・
    /// (b) key を消した commit は Missing で同じ repo の (a) の sha は (a) と同じ行・(c) key の値を壊した commit は `contracts/x.toml` と
    /// `docs/design/y.md` の両方に表が在っても Missing（読めない宣言を既定の置き場や Fixed に倒さない）・
    /// (d) key の無い宣言で `docs/design/y.md` に表を置いた commit は y.md の行を返す。
    #[test]
    fn lifecycle_declared_tables_reads_the_places_the_sha_declaration_names() {
        let repo = crate::pipe::fixture::scratch("lifecycle-mark-declared-tables");
        for args in [&["init", "-q", "-b", "main"][..], &["config", "user.name", "t"], &["config", "user.email", "t@example.invalid"], &["config", "commit.gpgsign", "false"]] {
            assert!(crate::pipe::git_ok(&repo, args), "{args:?}");
        }
        let region = |id: &str, write_set: &str| {
            format!("# y\n\n{}\nschema = 1\n\n{}{}\n", crate::pipe::table::BEGIN, table_row(id, write_set), crate::pipe::table::END)
        };
        let whole = |id: &str, write_set: &str| format!("schema = 1\n\n{}", table_row(id, write_set));
        let expected_a = Some(vec![("contracts/x.toml#a".to_owned(), vec!["src/a.rs".to_owned()])]);

        let a = commit_tree(&repo, "contract-tables = [\"contracts/\"]\n", &[("contracts/x.toml", whole("a", "src/a.rs"))]);
        assert!(!repo.join("docs/design").exists(), "docs/design/ に file が無い");
        assert_eq!(write_sets(super::read_rows(&repo, &a)), expected_a, "(a) key で名乗った置き場の行");
        let rows = match super::read_rows(&repo, &a) {
            super::Face::Found((rows, _)) => rows.into_iter().map(|row| row.pointer).collect::<Vec<_>>(),
            super::Face::Missing | super::Face::Fault => Vec::new(),
        };
        assert_eq!(rows, ["contracts/x.toml#a"], "pointer は <置き場の path>#<行 id>");

        let b = commit_tree(&repo, "", &[("contracts/x.toml", whole("a", "src/a.rs"))]);
        assert!(matches!(super::read_rows(&repo, &b), super::Face::Missing), "(b) key を消した commit は Missing");
        assert_eq!(write_sets(super::read_rows(&repo, &a)), expected_a, "(b) 古い sha を名指すと (a) と同じ行");

        let broken = "contract-tables = \"contracts/\"\n";
        let c = commit_tree(&repo, broken, &[("contracts/x.toml", whole("a", "src/a.rs")), ("docs/design/y.md", region("y", "src/y.rs"))]);
        assert!(matches!(super::read_rows(&repo, &c), super::Face::Missing), "(c) 読めない宣言は表が在っても Missing");

        let d = commit_tree(&repo, "", &[("docs/design/y.md", region("y", "src/y.rs"))]);
        let expected_d = Some(vec![("docs/design/y.md#y".to_owned(), vec!["src/y.rs".to_owned()])]);
        assert_eq!(write_sets(super::read_rows(&repo, &d)), expected_d, "(d) key の無い宣言は既定の置き場の行");
    }
}
