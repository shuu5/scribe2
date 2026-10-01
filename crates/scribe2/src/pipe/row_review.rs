//! 行の審査の記録の読み手と鍵の口（設計 docs/design/row-review.md §3 の口 (A)〜(F)・(H) と §9・契約表の行 a1）。
//!
//! 置き場は state dir の `pipe/row-review/`: 行の記録は `<名>/record`（名は [`judgement`] の 16 桁）、ref の記録は
//! `ref/<40 桁の sha>`、撃ち中の印は `ref/<sha>.pid`（`<pid> <起動時刻>`）。書き手は行 a が足す。歯は e2e の `pipe_review_record_`。

use super::gate::Verdict;
use super::review::FindingKind;
use super::table::{form_of, read_rows, BEGIN};
use super::{git_bytes, DIR};
use crate::fleet::store::{lock_owner, started_ms, Owner};
use crate::hook::vessel::digest::fnv1a_64;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// ref の記録を置く dir の名（置き場の下・40 桁の sha の名で 1 file）。
pub const REF_DIR: &str = "ref";

/// 記録の 1 行目（跨版で読める約束はこの 1 形だけ・形を変える版は schema を上げる）。
pub const SCHEMA_LINE: &str = "schema=1";

/// 置き場の根（`<state_dir>/pipe/row-review`）。
pub fn root_of(state_dir: &Path) -> PathBuf {
    state_dir.join(DIR).join("row-review")
}

/// ref の記録の file（`<根>/ref/<sha>`）。
pub fn ref_path(state_dir: &Path, sha: &str) -> PathBuf {
    root_of(state_dir).join(REF_DIR).join(sha)
}

/// ref の撃ち中の印の file（`<根>/ref/<sha>.pid`）。
pub fn mark_path(state_dir: &Path, sha: &str) -> PathBuf {
    root_of(state_dir).join(REF_DIR).join(format!("{sha}.pid"))
}

/// 審査の basis（行の祖先を何で測ったか・設計 §3 形 3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// 行を指す bead が在り、祖先が全部着地か実物。
    Actual,
    /// 宣言の祖先を 1 つ以上持つ。
    Forecast,
    /// 行を指す bead が無く、同じ doc の祖先は全部着地か実物。
    Partial,
}

impl Basis {
    /// 記録に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Actual => "actual",
            Self::Forecast => "forecast",
            Self::Partial => "partial",
        }
    }

    /// 字面から引く（3 語の外は `None`）。
    pub fn parse(text: &str) -> Option<Self> {
        [Self::Actual, Self::Forecast, Self::Partial].into_iter().find(|found| found.as_str() == text)
    }
}

/// ref の結果（口 (A) の返り・閉じた 5 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefResult {
    /// 全行が通った（記録の merge-base の sha と、判定した契約表の file の列）。
    Pass {
        /// 記録の merge-base の sha。
        base: String,
        /// 判定した契約表の file の列。
        tables: Vec<String>,
    },
    /// 落ちた行が在る。
    Fail,
    /// 撃ち中（印の持ち主が生きている・読めない印も待つ側＝fail-closed）。
    Pending,
    /// 印の持ち主が死んで撃ち終えていない。
    Stale,
    /// 記録が無い・読めない・schema が違う・result の行も印も無い。
    Missing,
}

/// 1 行 1 key の本文（1 行目が [`SCHEMA_LINE`] でない本文は `None`）の `key=value` の列。
fn fields(text: &str) -> Option<Vec<(&str, &str)>> {
    let mut lines = text.lines();
    (lines.next() == Some(SCHEMA_LINE)).then(|| lines.filter_map(|line| line.split_once('=')).collect())
}

/// 欄の値（先に出た行が勝つ）。
fn field<'a>(found: &[(&str, &'a str)], key: &str) -> Option<&'a str> {
    found.iter().find(|(name, _)| *name == key).map(|(_, value)| *value)
}

/// 40 桁の 16 進か（path を組む前の検査＝hook が渡す字で置き場の外へ出ない）。
fn is_sha(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// (A) ref の結果の読み手: result の行が在れば pass か fail、無ければ撃ち中の印の持ち主（`lock_owner` の 1 本）が生きていれば
/// pending・死んでいれば stale・印も無ければ missing。file が無い周は印だけで決め、読めない（dir・UTF-8 でない）・schema が違う・
/// pass なのに base の行が無い file は印に依らず missing（読めない記録を通さない・C10）。
pub fn read_ref(state_dir: &Path, sha: &str) -> RefResult {
    if !is_sha(sha) {
        return RefResult::Missing;
    }
    let owner = std::fs::read_to_string(mark_path(state_dir, sha)).ok().map(|body| lock_owner(&body, started_ms));
    let by_mark = || match owner {
        Some(Owner::Dead) => RefResult::Stale,
        Some(_) => RefResult::Pending,
        None => RefResult::Missing,
    };
    let text = match std::fs::read_to_string(ref_path(state_dir, sha)) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return by_mark(),
        Err(_) => return RefResult::Missing,
    };
    let Some(found) = fields(&text) else {
        return RefResult::Missing;
    };
    match (field(&found, "result"), field(&found, "base")) {
        (Some("fail"), _) => RefResult::Fail,
        (Some("pass"), Some(base)) if !base.is_empty() => {
            let tables = field(&found, "tables").unwrap_or_default().split(',').filter(|file| !file.is_empty());
            RefResult::Pass { base: base.to_owned(), tables: tables.map(str::to_owned).collect() }
        }
        (None, _) => by_mark(),
        _ => RefResult::Missing,
    }
}

/// (B) 行の digest の口（pure）: 契約 file の字と設計の節の本文の字を、この順に NUL で区切った byte の FNV-1a 64 の 16 桁。
/// 切れ目だけを動かした 2 形（ab と c・a と bc）は違う値になる。
pub fn row_digest(contract: &str, section: &str) -> String {
    fnv1a_64(&[contract.as_bytes(), &[0], section.as_bytes()].concat())
}

/// 表の file か（受付と同じ表の読み手 [`read_rows`] が行を返す file・表の欠陥で読めない file も表を持つ側＝鍵に入れない）。
fn bears_table(path: &str, body: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(body) else {
        return false;
    };
    let marker = if path.ends_with(".md") { BEGIN } else { "[[contract]]" };
    text.contains(marker) && read_rows(path, text).map_or(true, |rows| !rows.is_empty())
}

/// (C) code の木の鍵の口: `sha` の tree から契約表を持つ file を除いた全 file の path と blob の hash の列の digest（16 桁）。
/// 契約表だけを変えた commit では動かず、code の file を変えた commit で動く。git を撃てない・tree を読めない周は理由を返す。
pub fn tree_key(repo: &Path, sha: &str) -> Result<String, String> {
    let listed = git_bytes(repo, &["ls-tree", "-r", "-z", "--full-tree", sha]).ok_or_else(|| format!("{sha} の tree を読めない"))?;
    let text = String::from_utf8(listed).map_err(|_| format!("{sha} の tree の path が UTF-8 でない"))?;
    let mut bytes = Vec::new();
    for entry in text.split('\0').filter(|entry| !entry.is_empty()) {
        let (meta, path) = entry.split_once('\t').ok_or_else(|| format!("{sha} の tree の行を読めない: {entry}"))?;
        let blob = meta.split_whitespace().nth(2).ok_or_else(|| format!("{sha} の tree の行を読めない: {entry}"))?;
        let body = form_of(path).ok().and_then(|_| git_bytes(repo, &["show", &format!("{sha}:{path}")]));
        if !body.is_some_and(|body| bears_table(path, &body)) {
            bytes.extend_from_slice(format!("{path}\0{blob}\n").as_bytes());
        }
    }
    Ok(fnv1a_64(&bytes))
}

/// (H) 判定の鍵の 6 材料（設計 §9・記録の key の元）。
pub struct Parts<'a> {
    /// 行の digest（[`row_digest`]）。
    pub digest: &'a str,
    /// 材料の鍵（材料の dir の digest）。
    pub materials: &'a str,
    /// code の木の鍵（[`tree_key`]）。
    pub tree: &'a str,
    /// 審査の basis。
    pub basis: Basis,
    /// 祖先ごとの `<行>:<landed|tree|declared>` の列（祖先なしは空）。
    pub ancestors: &'a [String],
    /// lens の版の 1 行。
    pub version: &'a str,
}

/// 祖先の列の記録の字（コンマで並べる・祖先なしは `-`）。
pub fn ancestors_word(ancestors: &[String]) -> String {
    if ancestors.is_empty() { "-".to_owned() } else { ancestors.join(",") }
}

/// (H) 判定の鍵と行の記録の dir の名の口（pure）: 6 材料を 1 行ずつ並べた字の digest が判定の鍵、`<行>@<判定の鍵>` の字の
/// digest が dir の名（どちらも 16 桁・束の id と同じ形）。同じ材料の 2 回は同じ対を返す。
pub fn judgement(row: &str, parts: &Parts<'_>) -> (String, String) {
    let ancestors = ancestors_word(parts.ancestors);
    let lines = [parts.digest, parts.materials, parts.tree, parts.basis.as_str(), ancestors.as_str(), parts.version];
    let key = fnv1a_64(format!("{}\n", lines.join("\n")).as_bytes());
    let name = fnv1a_64(format!("{row}@{key}").as_bytes());
    (key, name)
}

/// 行の記録 1 つ（判定・basis・理由の型・at が読めない記録は作らない）。
struct Record {
    /// 記録の dir の名。
    name: String,
    /// 記録の本文。
    text: String,
    /// 判定。
    verdict: Verdict,
    /// basis。
    basis: Basis,
    /// 理由の型（`-` は `None`）。
    kind: Option<FindingKind>,
    /// UTC の秒。
    at: u64,
}

impl Record {
    /// 欄の値。
    fn get(&self, key: &str) -> Option<&str> {
        field(&fields(&self.text)?, key)
    }

    /// 記録の dir を読む（file が無い・UTF-8 でない・schema が違う・必須の欄が読めない周は `None`）。
    fn read(dir: &Path, name: &str) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join("record")).ok()?;
        let found = fields(&text)?;
        let kind = match field(&found, "kind")? {
            "-" => None,
            word => Some(FindingKind::parse(word)?),
        };
        let (verdict, basis) = (Verdict::parse(field(&found, "verdict")?)?, Basis::parse(field(&found, "basis")?)?);
        let at = field(&found, "at")?.parse().ok()?;
        Some(Self { name: name.to_owned(), text, verdict, basis, kind, at })
    }
}

/// 置き場の行の記録の全部（`ref` の dir と dir でない entry を除く・名の順）と、読めない記録の数。
fn records(state_dir: &Path) -> (Vec<Record>, usize) {
    let entries = std::fs::read_dir(root_of(state_dir)).map(|found| found.flatten().collect()).unwrap_or_else(|_| Vec::new());
    let mut names: Vec<(String, PathBuf)> = entries.iter().filter(|entry| entry.path().is_dir()).map(|entry| (entry.file_name().to_string_lossy().into_owned(), entry.path())).collect();
    names.retain(|(name, _)| name != REF_DIR);
    names.sort();
    let read: Vec<Record> = names.iter().filter_map(|(name, dir)| Record::read(dir, name)).collect();
    let unreadable = names.len() - read.len();
    (read, unreadable)
}

/// (D) 写せる記録を引く口: 行と 4 つの鍵の材料 `keys`（行の digest・材料の鍵・code の木の鍵・lens の版の順）が同じで、basis が
/// actual・祖先の状態の語が全部 landed か祖先なし・判定が PASS の行の記録の dir の名（at の新しい記録・無い・読めない・PASS でない周は
/// `None`）。dir の名が記録の判定の鍵から [`judgement`] の規則で決まる名と違う記録は写さない。
pub fn reusable(state_dir: &Path, row: &str, keys: [&str; 4]) -> Option<String> {
    let [digest, materials, tree, version] = keys;
    let landed = |word: &str| word == "-" || word.split(',').all(|entry| entry.rsplit_once(':').is_some_and(|(_, state)| state == "landed"));
    let same = |found: &&Record| {
        let named = found.get("key").is_some_and(|key| fnv1a_64(format!("{row}@{key}").as_bytes()) == found.name);
        let keys = [("row", row), ("digest", digest), ("materials", materials), ("tree", tree), ("version", version)];
        named && found.get("ancestors").is_some_and(landed) && keys.iter().all(|(key, want)| found.get(key) == Some(want))
    };
    let (read, _) = records(state_dir);
    let found = read.iter().filter(same).filter(|found| found.basis == Basis::Actual && found.verdict == Verdict::Pass);
    found.max_by_key(|found| found.at).map(|found| found.name.clone())
}

/// ref の記録の row の行（`row=<行> digest=<digest> …`）の (行, digest)（schema が違う本文は `None`）。
fn row_entries(text: &str) -> Option<Vec<(String, String)>> {
    let mut lines = text.lines();
    (lines.next() == Some(SCHEMA_LINE)).then(|| {
        let entry = |line: &str| {
            let mut words = line.split(' ');
            Some((words.next()?.strip_prefix("row=")?.to_owned(), words.next()?.strip_prefix("digest=")?.to_owned()))
        };
        lines.filter_map(entry).collect()
    })
}

/// (E) 兄弟の読み手: その行をその digest で載せた ref の記録（1 本でも）に載る、その行以外の `<行>` の列（重複なし・字の順）と、
/// 読めない ref の記録の file（dir・UTF-8 でない・schema が違う）の数。
pub fn siblings(state_dir: &Path, row: &str, digest: &str) -> (Vec<String>, usize) {
    let dir = root_of(state_dir).join(REF_DIR);
    let entries = std::fs::read_dir(&dir).map(|found| found.flatten().collect()).unwrap_or_else(|_| Vec::new());
    let (mut found, mut unreadable) = (BTreeSet::new(), 0);
    for name in entries.iter().map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| is_sha(name)) {
        let Some(rows) = std::fs::read_to_string(dir.join(name)).ok().and_then(|text| row_entries(&text)) else {
            unreadable += 1;
            continue;
        };
        if rows.iter().any(|(id, seen)| id == row && seen == digest) {
            found.extend(rows.into_iter().map(|(id, _)| id).filter(|id| id != row));
        }
    }
    (found.into_iter().collect(), unreadable)
}

/// 行の記録 1 つの一覧の行（口 (F) の返り）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// 判定。
    pub verdict: Verdict,
    /// basis。
    pub basis: Basis,
    /// 理由の型（`-` は `None`）。
    pub kind: Option<FindingKind>,
    /// 記録の時刻（UTC の秒）。
    pub at: u64,
}

/// (F) 行の記録の一覧: その行とその digest の行の記録ごとの（判定・basis・理由の型・at）を at の順に返し、読めない記録は数だけを
/// 別に返す。
pub fn listed(state_dir: &Path, row: &str, digest: &str) -> (Vec<Listed>, usize) {
    let (read, unreadable) = records(state_dir);
    let mut found: Vec<&Record> = read.iter().filter(|found| found.get("row") == Some(row) && found.get("digest") == Some(digest)).collect();
    found.sort_by_key(|found| (found.at, found.name.as_str()));
    let rows = found.into_iter().map(|found| Listed { verdict: found.verdict, basis: found.basis, kind: found.kind, at: found.at });
    (rows.collect(), unreadable)
}
