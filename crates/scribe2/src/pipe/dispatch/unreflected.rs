//! 未反映の裁定の判定・置き場の file・読み手（設計 docs/design/dispatcher.md §38・契約表の行 am・FR84 / FR68 / FR27・AC54・ADR-0083 (6)）。
//!
//! 母集団は閉じた台帳の問い（effect = document）の notes の裁定の行の id。未反映はそのうち main の先端の sha の追跡された file が 1 つも
//! 引かない id（数えは行 ak の [`cited_at`]・ruling-check の有無に依らない）。起こす側の 1 周ごとに置き場（`<state>/pipe/unreflected`・
//! 1 行の JSON）を上書きし、0 件の周も空の列を書く。観測の口は同じ判定を使い file は書かない。
//!
//! 待ちの理由の型と `Turn` は名指さない（写しは親の dispatch.rs だけが書く・台帳の 1 件の型も名指さず、親が素の値を渡す）。

use crate::fleet::json_lite::quote;
use crate::fleet::json_tree::{parse, Tree};
use crate::ledger::citation::{cited_at, prefix_of, scan};
use crate::ledger::close_reason::ruling_row;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// 置き場の下の file（`<state>/pipe/unreflected`）。
const FILE: [&str; 2] = ["pipe", "unreflected"];

/// file の schema 版。
const SCHEMA: &str = "1";

/// 閉じた問い（effect = document）の id と notes（親が台帳から渡す）。
pub struct Question<'a> {
    /// 問いの bead id。
    pub id: &'a str,
    /// 問いの notes（裁定の行を持つ）。
    pub notes: &'a str,
}

/// 問いの notes の裁定の行の id（行の順・判定と置き場の引きが読む 1 か所）。
fn rulings_of(notes: &str, prefix: Option<&str>) -> Vec<String> {
    notes.lines().filter_map(|line| ruling_row(line, prefix)).map(|row| row.id).collect()
}

/// 関わりを測る bead（閉じていない問い以外の 1 件・親が台帳から渡す）。
pub(super) struct Bead<'a> {
    /// bead id。
    pub(super) id: &'a str,
    /// acceptance と notes の字。
    pub(super) text: String,
    /// blocks の依存先の bead id。
    pub(super) blocks: Vec<&'a str>,
}

/// 1 周の判定（置き場の file の中身）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Judged {
    /// 数えた main の先端の sha（40 字）。
    sha: String,
    /// 母集団の裁定 id（字の順）。
    population: Vec<String>,
    /// 未反映の裁定 id（母集団の順）。
    unreflected: Vec<String>,
    /// 関わる契約の表（bead → 未反映の列の順で最初の id）。
    table: BTreeMap<String, String>,
}

impl Judged {
    /// bead を待たせる未反映の id（関わらない bead は `None`）。
    pub(super) fn reason_of(&self, bead: &str) -> Option<&str> {
        self.table.get(bead).map(String::as_str)
    }

    /// file の本文（1 行の JSON）。
    fn body(&self) -> String {
        let list = |ids: &[String]| format!("[{}]", ids.iter().map(|id| quote(id)).collect::<Vec<_>>().join(","));
        let table = self.table.iter().map(|(bead, id)| format!("{}:{}", quote(bead), quote(id))).collect::<Vec<_>>().join(",");
        let (sha, population, unreflected) = (quote(&self.sha), list(&self.population), list(&self.unreflected));
        format!("{{\"schema\":{SCHEMA},\"sha\":{sha},\"population\":{population},\"unreflected\":{unreflected},\"table\":{{{table}}}}}\n")
    }

    /// doctor の行（`unreflected=<n> sha=<7 字> ids=<id>, <id>`）。
    fn line(&self) -> String {
        let sha: String = self.sha.chars().take(7).collect();
        format!("unreflected={} sha={sha} ids={}", self.unreflected.len(), self.unreflected.join(", "))
    }
}

/// 置き場の file の path。
fn path_of(state_dir: &Path) -> PathBuf {
    FILE.iter().fold(state_dir.to_path_buf(), |path, part| path.join(part))
}

/// file の本文から読む（schema 違い・key の欠け・型違いは `None`）。
fn judged_of(text: &str) -> Option<Judged> {
    let tree = parse(text).ok()?;
    if !matches!(tree.get("schema"), Some(Tree::Num(found)) if found == SCHEMA) {
        return None;
    }
    let strings = |key: &str| -> Option<Vec<String>> { tree.get(key)?.as_array()?.iter().map(|node| node.as_str().map(str::to_owned)).collect() };
    let Tree::Object(pairs) = tree.get("table")? else {
        return None;
    };
    let table = pairs.iter().map(|(bead, id)| id.as_str().map(|text| (bead.clone(), text.to_owned()))).collect::<Option<BTreeMap<_, _>>>()?;
    Some(Judged { sha: tree.get("sha")?.as_str()?.to_owned(), population: strings("population")?, unreflected: strings("unreflected")?, table })
}

/// 置き場の file の 3 形（無い周と在るのに読めない周を分ける）。
enum Stored {
    /// file が無い。
    Absent,
    /// file が在って読めない。
    Unreadable,
    /// 読めた。
    File(Judged),
}

/// 置き場の file を読む。
fn stored(state_dir: &Path) -> Stored {
    match fs::read_to_string(path_of(state_dir)) {
        Err(err) if err.kind() == ErrorKind::NotFound => Stored::Absent,
        Err(_) => Stored::Unreadable,
        Ok(text) => judged_of(&text).map_or(Stored::Unreadable, Stored::File),
    }
}

/// 母集団のうち、`sha` の追跡された file が 1 つも引かない id（母集団の順・母集団が空なら git を撃たない・読めない周は `None`）。
fn not_cited(repo: &Path, sha: &str, prefix: Option<&str>, population: &[String]) -> Option<Vec<String>> {
    if population.is_empty() {
        return Some(Vec::new());
    }
    let cited: BTreeSet<String> = cited_at(repo, sha, prefix).ok()?.into_iter().map(|(_, cite)| cite.text).collect();
    Some(population.iter().filter(|id| !cited.contains(*id)).cloned().collect())
}

/// 関わる契約の表: acceptance か notes が未反映の id を引く bead と、その id の問いへ blocks の依存を持つ bead（未反映の列の順で最初の 1 つ）。
fn table_of(prefix: Option<&str>, unreflected: &[String], asked: &[(&str, Vec<String>)], beads: &[Bead<'_>]) -> BTreeMap<String, String> {
    beads
        .iter()
        .filter_map(|bead| {
            let cited: BTreeSet<String> = scan(&bead.text, prefix).into_iter().map(|cite| cite.text).collect();
            let blocked = |id: &String| asked.iter().any(|(question, ids)| bead.blocks.contains(question) && ids.contains(id));
            unreflected.iter().find(|id| cited.contains(*id) || blocked(id)).map(|id| (bead.id.to_owned(), id.clone()))
        })
        .collect()
}

/// 1 周を判じる（`sha` は main の先端・前の file と sha と母集団が同じ周は追跡された file を読み直さず未反映の列を引き継ぐ・
/// 追跡された file を読めない周は `None`）。file は書かない。
pub(super) fn judge(repo: &Path, state_dir: &Path, sha: &str, questions: &[Question<'_>], beads: &[Bead<'_>]) -> Option<Judged> {
    let prefix = prefix_of(repo);
    let prefix = prefix.as_deref();
    let asked: Vec<(&str, Vec<String>)> = questions.iter().map(|question| (question.id, rulings_of(question.notes, prefix))).collect();
    let population: Vec<String> = asked.iter().flat_map(|(_, ids)| ids.iter().cloned()).collect::<BTreeSet<_>>().into_iter().collect();
    let unreflected = match stored(state_dir) {
        Stored::File(old) if old.sha == sha && old.population == population => old.unreflected,
        _ => not_cited(repo, sha, prefix, &population)?,
    };
    let table = if unreflected.is_empty() { BTreeMap::new() } else { table_of(prefix, &unreflected, &asked, beads) };
    Some(Judged { sha: sha.to_owned(), population, unreflected, table })
}

/// 置き場の file を上書きする（temp へ書いてから rename・書けない周は何もしない＝前の file が残る）。
pub(super) fn write(state_dir: &Path, found: &Judged) {
    let path = path_of(state_dir);
    if path.parent().is_none_or(|dir| fs::create_dir_all(dir).is_err()) {
        return;
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    if fs::write(&temporary, found.body()).is_err() || fs::rename(&temporary, &path).is_err() {
        let _ = fs::remove_file(&temporary);
    }
}

/// 関わる契約の表の読み（file が無い周・在るのに読めない周・表を分けて返す・着地の留めが呼ぶ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Table {
    /// 置き場の file が無い。
    Absent,
    /// 置き場の file が在って読めない。
    Unreadable,
    /// 表（bead → 未反映の id・0 件の周は空）。
    Rows(BTreeMap<String, String>),
}

/// 置き場の file を読んで関わる契約の表を返す（file だけを読む・撃たない）。
pub fn involved(state_dir: &Path) -> Table {
    match stored(state_dir) {
        Stored::Absent => Table::Absent,
        Stored::Unreadable => Table::Unreadable,
        Stored::File(found) => Table::Rows(found.table),
    }
}

/// 置き場の裁定 id を問いへ引いた読み（局面の書き手が呼ぶ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// 置き場の file が在って読めない。
    Unreadable,
    /// 未反映の裁定 id の行を notes に持つ問いの id（入力の順・置き場の無い周は空）。
    Ids(Vec<String>),
}

/// 置き場の未反映の裁定 id を、`questions`（閉じた問い）のうち notes の裁定の行にその id を持つ問いの id へ引く（file だけを読む・撃たない）。
pub fn asked(state_dir: &Path, prefix: Option<&str>, questions: &[Question<'_>]) -> Asked {
    match stored(state_dir) {
        Stored::Absent => Asked::Ids(Vec::new()),
        Stored::Unreadable => Asked::Unreadable,
        Stored::File(found) => Asked::Ids(
            questions
                .iter()
                .filter(|question| rulings_of(question.notes, prefix).iter().any(|id| found.unreflected.contains(id)))
                .map(|question| question.id.to_owned())
                .collect(),
        ),
    }
}

/// 置き場の file の未反映の件数（tick の実測・無い周と読めない周は 0）。
pub fn count(state_dir: &Path) -> usize {
    match stored(state_dir) {
        Stored::File(found) => found.unreflected.len(),
        Stored::Absent | Stored::Unreadable => 0,
    }
}

/// doctor の 1 行（未反映が 1 件以上の周だけ・読めない file は `unreflected=unreadable`・0 件と file 無しは行を出さない）。
pub fn doctor_line(state_dir: &Path) -> Option<String> {
    match stored(state_dir) {
        Stored::Absent => None,
        Stored::Unreadable => Some("unreflected=unreadable".to_owned()),
        Stored::File(found) => (!found.unreflected.is_empty()).then(|| found.line()),
    }
}
