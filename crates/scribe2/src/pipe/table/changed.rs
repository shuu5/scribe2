//! 変わった行の読みと要否（設計 docs/design/contract-source.md §66 形 3・契約表の行 by・SRS FR105 / FR55）。
//!
//! `contracts check --base <sha>` の周だけが使う。変わった行（base の同じ doc に同じ id の行が無い行と、在って done の字が base と違う
//! 行）の読み [`changed`]、変わった行に求める要否 [`gaps`]、base の commit から doc の行と歯の区間を読む口 [`BaseAt`]。要否の外れは
//! 本 module の型 [`Gap`] で返し、表の検査の 1 種への包みは `table/check.rs` が持つ（ほかの行の touches の型を本 module は名指さない）。

use super::super::closure::Source;
use super::super::git_bytes;
use super::super::review::done_items;
use std::cell::OnceCell;
use std::path::Path;

/// 変わった行に求める 2 つの要否の外れ（宣言 `teeth-check` が true の周だけ・1 行が両方を持つこともある）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gap {
    /// done が番号つきの項目を 1 つも持たない。
    Unnumbered,
    /// 欄 done-teeth を持たない。
    Missing,
}

/// 変わった行の添字（`now` の順）。`now` は (行 id, done の字・表の読みの後の値) の列、`before` は base の同じ doc の同じ組の列で
/// 無い doc は `None`（全行が足された行）。base に同じ id の行が無い行と、在って done の字が違う行を数える。done 以外の欄と § の散文
/// だけを変えた行は数えない（字が同じ）。
pub(crate) fn changed(now: &[(&str, &str)], before: Option<&[(String, String)]>) -> Vec<usize> {
    let same = |id: &str, done: &str| before.is_some_and(|rows| rows.iter().find(|(seen, _)| seen == id).is_some_and(|(_, was)| was == done));
    now.iter().enumerate().filter(|(_, (id, done))| !same(id, done)).map(|(at, _)| at).collect()
}

/// 変わった行 1 つの要否の外れ（done を持たない行〔約束の行を持つ行〕は求めない）。
pub(crate) fn gaps(done: &str, elements: &[String]) -> Vec<Gap> {
    let mut found = Vec::new();
    if done.is_empty() {
        return found;
    }
    if done_items(done).is_empty() {
        found.push(Gap::Unnumbered);
    }
    if elements.is_empty() {
        found.push(Gap::Missing);
    }
    found
}

/// base の tracked な path の列と `.rs` の本文（歯の区間の在りかの照らしが読む）。
type Tree = (Vec<String>, Vec<Source>);

/// base の commit（読めると確かめた sha）から、doc の行と木を読む口。木は在りかの照らしが要る周だけ 1 回読む。
pub(crate) struct BaseAt<'a> {
    /// repo の root。
    repo: &'a Path,
    /// base の commit の sha。
    sha: &'a str,
    /// base の木（初めて要る周に読む・読めなければ理由）。
    tree: OnceCell<Result<Tree, String>>,
}

impl<'a> BaseAt<'a> {
    /// base の commit を読めると確かめて組む（読めなければ理由の 1 行・`-` で始まる字は旗に読まれるので断る）。
    pub(crate) fn open(repo: &'a Path, sha: &'a str) -> Result<Self, String> {
        let spec = format!("{sha}^{{commit}}");
        let readable = !sha.starts_with('-') && git_bytes(repo, &["rev-parse", "--verify", "--quiet", &spec]).is_some();
        let found = Self { repo, sha, tree: OnceCell::new() };
        readable.then_some(found).ok_or_else(|| format!("--base {sha} の commit を {} から読めない", repo.display()))
    }

    /// base の doc の (行 id, done の字) の列。doc が base に無ければ `None`（読めない doc は理由）。
    pub(crate) fn rows(&self, doc: &str) -> Result<Option<Vec<(String, String)>>, String> {
        let spec = format!("{}:{doc}", self.sha);
        let Some(bytes) = git_bytes(self.repo, &["show", &spec]) else {
            return Ok(None);
        };
        let read = super::read_rows(doc, &String::from_utf8_lossy(&bytes));
        let rows = read.map_err(|errors| {
            let first = errors.iter().map(|error| error.reason()).next().unwrap_or_default();
            format!("base {} の {doc} を読めない: {first}", self.sha)
        })?;
        Ok(Some(rows.into_iter().map(|row| (row.id, row.done)).collect()))
    }

    /// base の木（tracked の path の列と `.rs` の本文）。1 本でも読めなければ理由（「歯が無い」に読み替えない）。
    pub(crate) fn tree(&self) -> Result<(&[String], &[Source]), &str> {
        let read = self.tree.get_or_init(|| self.read_tree());
        read.as_ref().map(|(tracked, sources)| (tracked.as_slice(), sources.as_slice())).map_err(String::as_str)
    }

    /// base の木を読む（`git ls-tree` の path の列と、そのうち `.rs` の `git show`）。
    fn read_tree(&self) -> Result<Tree, String> {
        let listed = git_bytes(self.repo, &["ls-tree", "-r", "-z", "--name-only", self.sha])
            .ok_or_else(|| format!("base {} の木を読めない", self.sha))?;
        let tracked: Vec<String> = String::from_utf8_lossy(&listed).split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect();
        let mut sources = Vec::new();
        for path in tracked.iter().filter(|path| path.ends_with(".rs")) {
            let spec = format!("{}:{path}", self.sha);
            let body = git_bytes(self.repo, &["show", &spec]).ok_or_else(|| format!("base {} の {path} を読めない", self.sha))?;
            sources.push(Source { path: path.clone(), body: Ok(String::from_utf8_lossy(&body).into_owned()) });
        }
        Ok((tracked, sources))
    }
}
