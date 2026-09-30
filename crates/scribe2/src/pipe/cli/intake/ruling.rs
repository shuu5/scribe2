//! 受付の裁定 id の引用の判定（設計 docs/design/dispatcher.md §37・契約表の行 al・FR83 / AC53・数えは §36 の
//! [`crate::ledger::citation`] の 1 本）。
//!
//! 契約の設計の節の本文と契約表の行の欄の値から引用を拾い、`ruling-fixtures` の完全一致で外し、台帳の裁定の行に解けるか
//! （線より後の時刻の形は解けない）を測る。返すのは素の値（[`Ruled`]・名指す id の列と置き場）だけで、断りの組み立ては親が
//! 書く——この子は断りと証拠の型も台帳の 1 件の型も表の行の型も名指さない（ほかの行の閉包に入らないため・設計 §37 約束 7）。
//!
//! 宣言（`ruling-check` と `ruling-fixtures`）・台帳・opt-in の線は材料 1 つにつき 1 回だけ読む（[`Rulings`] が持つ）。
//! 台帳は引用を 1 件でも持つ契約が出たときに初めて読む（引用の無い周は台帳の client を起こさない）。

use crate::ledger::citation::{before_line, is_fixture, line_commit, prefix_of, scan, verdict, Cite, Form, Index, Notes, Verdict};
use crate::ledger::form::is_question;
use crate::pipe::contract::Contract;
use crate::pipe::declaration::ruling_keys_at;
use crate::pipe::review::section_text;
use crate::pipe::table;
use crate::seat::ledger::read_ledger;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

/// 宣言を読む tree（受付の base）。
const HEAD: &str = "HEAD";

/// 閉じた bead の status の字面。
const CLOSED: &str = "closed";

/// 台帳を読めない周の語（doctor の行の閉じた 3 語のうち台帳）。
const LEDGER: &str = "ledger";

/// 宣言の 2 key から決めた、この材料の引用の規則。
#[derive(Clone)]
struct Setup {
    /// `ruling-fixtures`（完全一致で外す字面）。
    fixtures: Vec<String>,
    /// 台帳の接頭辞（解けない周は `None`＝問い id の形は拾わない）。
    prefix: Option<String>,
}

/// 台帳の索引と opt-in の線（引用を持つ契約が出たときに 1 回だけ読む）。
#[derive(Clone)]
struct Facts {
    /// 裁定の行から作った、解けるの索引。
    index: Index,
    /// opt-in の線の commit。
    line: String,
}

/// 引用の材料（台帳の client と、1 周に 1 回だけ読む宣言・台帳・線）。
#[derive(Clone)]
pub(super) struct Rulings {
    /// 台帳の client（`--bd` か既定の名）。
    bd: String,
    /// `ruling-check` が true なら引用の規則（true でない repo と宣言を読めない repo は `None`）。
    setup: OnceLock<Option<Setup>>,
    /// 台帳と線の読み（読めない周は語）。
    facts: OnceLock<Result<Facts, &'static str>>,
}

/// 引用を拾う母集団（設計の節の本文と契約表の行の欄の値）。
pub(super) struct Cited<'a> {
    /// 母集団の doc の path（線より前の判定が当てる file）。
    pub(super) doc: &'a str,
    /// 設計の節の本文。
    pub(super) section: &'a str,
    /// 契約表の行の欄の値。
    pub(super) fields: &'a [String],
}

/// 判定の結果（素の値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Ruled {
    /// 断る引用が無い（key の無い repo・引用の無い契約を含む）。
    Clear,
    /// 解けない引用（置き場ごとの字面・昇順・重複を除く）。
    Unresolved {
        /// 設計の節の引用。
        section: Vec<String>,
        /// 契約表の行の引用。
        row: Vec<String>,
    },
    /// 台帳か線を読めない（語は doctor の行の閉じた語）。
    Unmeasured(&'static str),
}

/// 引用の置き場。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// 設計の節の本文。
    Section,
    /// 契約表の行の欄。
    Row,
}

impl Rulings {
    /// 台帳の client を名指して材料を作る（何も読まない）。
    pub(super) fn new(bd: &str) -> Self {
        Self { bd: bd.to_owned(), setup: OnceLock::new(), facts: OnceLock::new() }
    }

    /// `repo` の base の宣言の `ruling-check` が true か（母集団を読む前の門・宣言は 1 回だけ読む）。
    pub(super) fn checks(&self, repo: &Path) -> bool {
        self.setup(repo).is_some()
    }

    /// 宣言の 2 key（false の repo と読めない宣言は `None`＝何もしない・読めない宣言は宣言の凍結が断る）。
    fn setup(&self, repo: &Path) -> Option<&Setup> {
        self.setup
            .get_or_init(|| {
                let keys = ruling_keys_at(repo, HEAD).ok().filter(|found| found.check)?;
                Some(Setup { fixtures: keys.fixtures, prefix: prefix_of(repo) })
            })
            .as_ref()
    }

    /// 契約の母集団（設計の節の本文と、契約の欄の値＝行の欄の値）の引用を測る。ruling-check が true でない repo は母集団を読まない。
    pub(super) fn judge_contract(&self, repo: &Path, timeout: Option<Duration>, contract: &Contract) -> Ruled {
        if !self.checks(repo) {
            return Ruled::Clear;
        }
        let (doc, section) = section_of(repo, &contract.design);
        let fields: Vec<String> = [&contract.goal, &contract.done, &contract.size, &contract.owner, &contract.disposition, &contract.design]
            .into_iter()
            .chain([&contract.write_set, &contract.verify, &contract.req, &contract.classes, &contract.opens, &contract.touches, &contract.growth].into_iter().flatten())
            .cloned()
            .collect();
        self.judge(repo, timeout, &Cited { doc: &doc, section: &section, fields: &fields })
    }

    /// 母集団の引用を測る。`timeout` は台帳の待ち上限（rules 行の値・無ければ台帳を読めない周）。
    fn judge(&self, repo: &Path, timeout: Option<Duration>, cited: &Cited<'_>) -> Ruled {
        let Some(setup) = self.setup(repo) else {
            return Ruled::Clear;
        };
        let prefix = setup.prefix.as_deref();
        let mut found: Vec<(Place, Cite)> = scan(cited.section, prefix).into_iter().map(|cite| (Place::Section, cite)).collect();
        found.extend(cited.fields.iter().flat_map(|field| scan(field, prefix)).map(|cite| (Place::Row, cite)));
        // 一覧に載せた字面は台帳にも線にも依らずに外れる（線より前の時刻の形は解けるので、載せていても断る側には入らない）。
        found.retain(|(_, cite)| !is_fixture(cite, &setup.fixtures, false));
        if found.is_empty() {
            return Ruled::Clear;
        }
        let facts = match self.facts.get_or_init(|| load(&self.bd, repo, timeout, prefix)) {
            Ok(facts) => facts,
            Err(word) => return Ruled::Unmeasured(word),
        };
        let times: BTreeSet<(String, String)> =
            found.iter().filter(|(_, cite)| cite.form == Form::Time).map(|(_, cite)| (cited.doc.to_owned(), cite.text.clone())).collect();
        let before = match before_line(repo, &facts.line, &times) {
            Ok(found) => found,
            Err(reason) => return Ruled::Unmeasured(reason.as_str()),
        };
        let (mut section, mut row) = (BTreeSet::new(), BTreeSet::new());
        for (place, cite) in &found {
            let resolved = match cite.form {
                Form::Time => before.contains(&(cited.doc.to_owned(), cite.text.clone())),
                Form::Question | Form::Batch | Form::Policy => verdict(Some(&facts.index), cite) == Verdict::Resolved,
            };
            if !resolved {
                let seen = if *place == Place::Section { &mut section } else { &mut row };
                seen.insert(cite.text.clone());
            }
        }
        if section.is_empty() && row.is_empty() {
            Ruled::Clear
        } else {
            Ruled::Unresolved { section: section.into_iter().collect(), row: row.into_iter().collect() }
        }
    }
}

/// 契約の `design` が指す設計の節の本文（base の doc から [`section_text`] で読む）と、その doc の path。設計 pointer でない `design` と
/// 読めない doc は節が空（読めなさは受付の行の読みが別に断る）で、path は `design` の字面のまま。
fn section_of(repo: &Path, design: &str) -> (String, String) {
    let Ok(pointer) = table::parse_pointer(design) else {
        return (design.to_owned(), String::new());
    };
    let section = table::read(repo, &pointer.path)
        .ok()
        .and_then(|text| table::find_row(&pointer.path, &text, &pointer.id).ok().map(|row| section_text(&text, &row.section)))
        .unwrap_or_default();
    (pointer.path, section)
}

/// 台帳を 1 回読んで索引を作り、opt-in の線を引く（読めない周は語・台帳 → 線の順に最初に読めなかった 1 つ）。
fn load(bd: &str, repo: &Path, timeout: Option<Duration>, prefix: Option<&str>) -> Result<Facts, &'static str> {
    let timeout = timeout.ok_or(LEDGER)?;
    let issues = read_ledger(bd, repo, timeout).map_err(|_| LEDGER)?;
    let closed_questions = issues.iter().filter(|found| found.status == CLOSED && is_question(found)).map(|found| found.notes.clone()).collect();
    let notes = Notes { closed_questions, every: issues.iter().map(|found| found.notes.clone()).collect() };
    let line = line_commit(repo).map_err(|reason| reason.as_str())?.ok_or("git")?;
    Ok(Facts { index: Index::new(&notes, prefix), line })
}
