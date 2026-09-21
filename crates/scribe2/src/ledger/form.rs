//! 台帳の形の lint（設計 docs/design/ledger-form.md §3 の 4・契約表の行 a・FR51）。
//!
//! doctor の項目 1 行（`ledger-form: …`）に 5 つの欠陥を**件数と母集団と id** で出す:
//! (iv) memo の 4 節（[`MEMO_SECTIONS`]）のどれかが無い bead・(v) field の 4 象限（label `intake:memo` ×
//! acceptance の設計 pointer 行）の違反 2 形・(vi) § か本文が memo の id を名指すのに `discovered-from` の無い
//! 契約・(vii) 契約表の未着地の行のうち pointer を持つ open の bead が無い行（drift）・(viii) `discovered-from` で
//! 辿れる契約が全部 closed の open な memo。
//!
//! 判定は [`judge`] の**純関数**で、読むのは [`Issue`] の label / acceptance / description / notes / dependencies と、
//! 型（epic と裁定を 4 象限の母集団から外す）と、[`Docs`]（pointer の先の § の本文・契約表の行・tracked な file の
//! 集合）だけである。読みの口は席の側の 1 本（[`crate::seat::ledger::read_ledger`]）を借り、**台帳の書きの口は
//! 増えない**（書きは親 module の `close` の 1 種のまま・C15）。読めない周は件数 0 に倒さず測れていない形の行を
//! 出す（[`render_unreadable`]・C10 / NFR4）。極性は増やさない（doctor は読むだけで判定しない）。

use crate::pipe::table;
use crate::rules::manifest::Manifest;
use crate::seat::ledger::{self, Issue, LedgerError};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// 行の先頭の字面。
pub const PREFIX: &str = "ledger-form:";

/// memo の識別の label（§3 の 1・型を足さない）。
pub const MEMO_LABEL: &str = "intake:memo";

/// memo の本文の 4 節の見出し（§3 の 2・宣言順）。
pub const MEMO_SECTIONS: [&str; 4] = ["### 出所", "### 観測", "### 候補", "### 昇格条件"];

/// 4 節の欠けの語（[`MEMO_SECTIONS`] と同じ順）。
const MISSING_WORDS: [&str; 4] = ["no-source", "no-observation", "no-candidate", "no-promotion"];

/// 4 象限の母集団から外す型（epic と裁定）。
const EXEMPT_KINDS: [&str; 2] = ["epic", "decision"];

/// acceptance の設計 pointer 行の頭（行 e と同じ字面）。
const DESIGN_KEY: &str = "design =";

/// memo から契約へ張る edge の種別。
const DISCOVERED_FROM: &str = "discovered-from";

/// 閉じた bead の status。
const CLOSED: &str = "closed";

/// 契約表の 1 行のうち lint が読む分（doc の repo 相対 path・行 id・write-set の `+` の path）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// 契約表を持つ設計 doc の repo 相対 path。
    pub doc: String,
    /// 行 id。
    pub id: String,
    /// write-set の `+` の path（`+` を剥いだ字面・無ければ空）。
    pub plus: Vec<String>,
}

/// 台帳の外の入力（pointer の先の § の本文・契約表の行・tracked な file の集合）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Docs {
    /// 設計 pointer の字面（`<doc>#<id>`）→ その行の `section` の § の本文（解けない pointer は key が無い）。
    pub sections: BTreeMap<String, String>,
    /// 全設計 doc の契約表の行。
    pub rows: Vec<Row>,
    /// tracked な file の repo 相対 path（未着地の行の弁別・§3 の 5 と同じ読み）。
    pub tracked: BTreeSet<String>,
}

/// 5 つの欠陥の件数と母集団と id（id は昇順）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// closed でない bead の件数。
    pub open: usize,
    /// closed でない memo の件数（(iv) と (viii) の母集団）。
    pub memos: usize,
    /// (iv) 4 節ごとの、その節を持たない memo の id（[`MEMO_SECTIONS`] の順）。
    pub missing: [Vec<String>; 4],
    /// 4 象限の母集団（closed でない bead から epic と裁定を外した件数）。
    pub shaped: usize,
    /// (v) label と pointer 行の両方を持つ bead。
    pub both: Vec<String>,
    /// (v) label も pointer 行も持たない bead。
    pub neither: Vec<String>,
    /// closed でない契約の件数（(vi) の母集団）。
    pub contracts: usize,
    /// (vi) memo を名指すのに `discovered-from` の無い契約。
    pub undiscovered: Vec<String>,
    /// 契約表の未着地の行の件数（(vii) の母集団）。
    pub unlanded: usize,
    /// (vii) pointer を持つ open の bead が無い未着地の行（契約 id `<doc の stem>#<行 id>`）。
    pub drift: Vec<String>,
    /// (viii) 辿れる契約が全部 closed の open な memo。
    pub settled: Vec<String>,
}

/// acceptance の設計 pointer 行の値（`design =` の後ろ・trim 済み）。無ければ `None`。
pub fn pointer_text(acceptance: &str) -> Option<&str> {
    acceptance.lines().map(str::trim).find_map(|line| line.strip_prefix(DESIGN_KEY)).map(str::trim)
}

/// memo か（label `intake:memo` を持つ）。
fn is_memo(issue: &Issue) -> bool {
    issue.labels.iter().any(|label| label == MEMO_LABEL)
}

/// 契約か（label を持たず pointer 行を持つ＝4 象限の契約）。
fn is_contract(issue: &Issue) -> bool {
    !is_memo(issue) && pointer_text(&issue.acceptance).is_some()
}

/// 本文（description と notes）に見出し `heading` の行が在るか。
fn has_heading(issue: &Issue, heading: &str) -> bool {
    issue.description.lines().chain(issue.notes.lines()).any(|line| line.trim() == heading)
}

/// id の字面を作る文字か。
fn is_id_char(found: char) -> bool {
    found.is_alphanumeric() || found == '-' || found == '_'
}

/// `text` が id `id` を**字面で**名指すか（前後が id の続きでない出現だけ・`s2-x.7` は `s2-x.70` に当たらない）。
pub fn names(text: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    text.match_indices(id).any(|(at, _)| {
        let before = text.get(..at).and_then(|head| head.chars().next_back());
        let mut after = text.get(at.saturating_add(id.len())..).unwrap_or_default().chars();
        let bounded_before = before.is_none_or(|found| !is_id_char(found) && found != '.');
        let bounded_after = match after.next() {
            None => true,
            Some('.') => after.next().is_none_or(|found| !is_id_char(found)),
            Some(found) => !is_id_char(found),
        };
        bounded_before && bounded_after
    })
}

/// 契約が名指す memo のうち `discovered-from` の edge が無いものが在るか（§ の本文と description と notes）。
fn undiscovered(contract: &Issue, memo_ids: &[&str], docs: &Docs) -> bool {
    let section = pointer_text(&contract.acceptance).and_then(|text| docs.sections.get(text)).map_or("", String::as_str);
    let texts = [section, contract.description.as_str(), contract.notes.as_str()];
    memo_ids.iter().filter(|memo| **memo != contract.id).any(|memo| {
        texts.iter().any(|text| names(text, memo))
            && !contract.deps.iter().any(|dep| dep.kind == DISCOVERED_FROM && dep.on == *memo)
    })
}

/// 行が未着地か（write-set の `+` の path のどれかが tracked に無い・`+` の無い行は未着地に数えない）。
fn is_unlanded(row: &Row, tracked: &BTreeSet<String>) -> bool {
    row.plus.iter().any(|path| match path.strip_suffix('/') {
        Some(_) => !tracked.iter().any(|found| found.starts_with(path.as_str())),
        None => !tracked.contains(path),
    })
}

/// bead の設計 pointer がこの行を指すか。
fn points_at(issue: &Issue, row: &Row) -> bool {
    pointer_text(&issue.acceptance)
        .and_then(|text| table::parse_pointer(text).ok())
        .is_some_and(|pointer| pointer.path == row.doc && pointer.id == row.id)
}

/// memo から `discovered-from` で辿れる契約が在り、全部 closed か。
fn is_settled(memo: &Issue, issues: &[Issue]) -> bool {
    let mut reached = issues
        .iter()
        .filter(|issue| pointer_text(&issue.acceptance).is_some() && !is_memo(issue))
        .filter(|issue| issue.deps.iter().any(|dep| dep.kind == DISCOVERED_FROM && dep.on == memo.id))
        .peekable();
    reached.peek().is_some() && reached.all(|issue| issue.status == CLOSED)
}

/// id の列（昇順）。
fn ids<'i>(issues: impl Iterator<Item = &'i Issue>) -> Vec<String> {
    let mut found: Vec<String> = issues.map(|issue| issue.id.clone()).collect();
    found.sort();
    found
}

/// 5 つの欠陥を数える（**純関数**・§3 の 4）。
pub fn judge(issues: &[Issue], docs: &Docs) -> Report {
    let open: Vec<&Issue> = issues.iter().filter(|issue| issue.status != CLOSED).collect();
    let memos: Vec<&Issue> = open.iter().copied().filter(|issue| is_memo(issue)).collect();
    let memo_ids: Vec<&str> = issues.iter().filter(|issue| is_memo(issue)).map(|issue| issue.id.as_str()).collect();
    let shaped: Vec<&Issue> = open.iter().copied().filter(|issue| !EXEMPT_KINDS.contains(&issue.kind.as_str())).collect();
    let contracts: Vec<&Issue> = open.iter().copied().filter(|issue| is_contract(issue)).collect();
    let unlanded: Vec<&Row> = docs.rows.iter().filter(|row| is_unlanded(row, &docs.tracked)).collect();
    let mut drift: Vec<String> = unlanded
        .iter()
        .filter(|row| !open.iter().any(|issue| points_at(issue, row)))
        .map(|row| table::contract_id(&row.doc, &row.id))
        .collect();
    drift.sort();
    Report {
        open: open.len(),
        memos: memos.len(),
        missing: MEMO_SECTIONS.map(|heading| ids(memos.iter().copied().filter(|memo| !has_heading(memo, heading)))),
        shaped: shaped.len(),
        both: ids(shaped.iter().copied().filter(|issue| is_memo(issue) && pointer_text(&issue.acceptance).is_some())),
        neither: ids(shaped.iter().copied().filter(|issue| !is_memo(issue) && pointer_text(&issue.acceptance).is_none())),
        contracts: contracts.len(),
        undiscovered: ids(contracts.iter().copied().filter(|issue| undiscovered(issue, &memo_ids, docs))),
        unlanded: unlanded.len(),
        drift,
        settled: ids(memos.iter().copied().filter(|memo| is_settled(memo, issues))),
    }
}

/// 欠陥 1 種の欄（`<語>=<件数>` と、1 件以上なら `:<id>,<id>…`）。
fn field(word: &str, found: &[String]) -> String {
    if found.is_empty() {
        format!("{word}=0")
    } else {
        format!("{word}={}:{}", found.len(), found.join(","))
    }
}

/// 測れた周の 1 行（件数と母集団と id が同じ行・欠陥 0 の周も 0 と母集団が出る）。
pub fn render(report: &Report) -> String {
    let missing: Vec<String> =
        MISSING_WORDS.iter().zip(report.missing.iter()).map(|(word, found)| field(word, found)).collect();
    format!(
        "{PREFIX} open={} memos={} {} shaped={} {} {} contracts={} {} unlanded={} {} {}",
        report.open,
        report.memos,
        missing.join(" "),
        report.shaped,
        field("both", &report.both),
        field("neither", &report.neither),
        report.contracts,
        field("undiscovered", &report.undiscovered),
        report.unlanded,
        field("drift", &report.drift),
        field("settled", &report.settled),
    )
}

/// 測れていない周の 1 行（件数を 1 つも出さない＝0 に化けさせない・C10）。
pub fn render_unreadable(reason: &str) -> String {
    format!("{PREFIX} unreadable reason={reason}")
}

/// doctor の項目 1 行（`--repo R` の台帳と設計 doc を読み、[`judge`] を撃つ）。`rules` は待ち上限を読む manifest
/// （無ければ埋め込み）。台帳 client は既定の名（PATH 解決）で、子 process の cwd は `repo`。
pub fn doctor_line(repo: &Path, rules: Option<&str>) -> String {
    match measure(repo, rules) {
        Ok(report) => render(&report),
        Err(reason) => render_unreadable(reason),
    }
}

/// 読みの全部（manifest → 台帳 → tracked → 契約表 → § の本文）。読めない周は理由の語。
fn measure(repo: &Path, rules: Option<&str>) -> Result<Report, &'static str> {
    let manifest = rules
        .map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path)))
        .map_err(|_| "rules-unreadable")?;
    let timeout = ledger::timeout_of(&manifest).ok_or("no-rule")?;
    let issues = ledger::read_ledger(ledger::DEFAULT_BD, repo, timeout).map_err(|error| match error {
        LedgerError::Unreadable => "ledger-unreadable",
        LedgerError::Timeout => "ledger-timeout",
    })?;
    let docs = docs_of(repo, &issues)?;
    Ok(judge(&issues, &docs))
}

/// 設計 doc の直下の `*.md` か（`contracts check` と同じ母集団）。
fn is_design_doc(path: &str) -> bool {
    path.strip_prefix(table::DESIGN_DIR).is_some_and(|rest| !rest.contains('/') && rest.ends_with(".md"))
}

/// 台帳の外の入力を読む（tracked を読めない・契約表を読めない周は理由の語）。
fn docs_of(repo: &Path, issues: &[Issue]) -> Result<Docs, &'static str> {
    let tracked: BTreeSet<String> = table::tracked_files(repo).ok_or("tracked-unreadable")?.into_iter().collect();
    let mut rows = Vec::new();
    for doc in tracked.iter().filter(|path| is_design_doc(path)) {
        let text = table::read(repo, doc).map_err(|_| "table-unreadable")?;
        let found = table::read_rows(doc, &text).map_err(|_| "table-unreadable")?;
        rows.extend(found.into_iter().map(|row| Row {
            doc: doc.clone(),
            id: row.id,
            plus: row.write_set.iter().filter_map(|path| path.strip_prefix('+')).map(str::to_owned).collect(),
        }));
    }
    let sections = issues
        .iter()
        .filter_map(|issue| pointer_text(&issue.acceptance))
        .filter_map(|text| section_of(repo, text).map(|body| (text.to_owned(), body)))
        .collect();
    Ok(Docs { sections, rows, tracked })
}

/// pointer の先の行の `section` の § の本文（解けない周は `None`＝行 e が数える側）。
fn section_of(repo: &Path, text: &str) -> Option<String> {
    let pointer = table::parse_pointer(text).ok()?;
    let doc = table::read(repo, &pointer.path).ok()?;
    let row = table::find_row(&pointer.path, &doc, &pointer.id).ok()?;
    Some(section_text(&doc, &row.section))
}

/// 節 `number` の本文（`## N.` の見出しの次の行から次の `## ` 見出しの前まで・契約表の区間と fence の中の `## `
/// は見出しに数えない・審査の材料の § の読みと同じ形）。無ければ空。
fn section_text(doc: &str, number: &str) -> String {
    let mut found: Vec<&str> = Vec::new();
    let (mut fenced, mut inside, mut open) = (false, false, false);
    for line in doc.lines() {
        let trimmed = line.trim();
        if trimmed == table::BEGIN || trimmed == table::END {
            inside = trimmed == table::BEGIN;
            continue;
        }
        if inside {
            continue;
        }
        if trimmed.starts_with("```") {
            fenced = !fenced;
        }
        match line.strip_prefix("## ").filter(|_| !fenced) {
            Some(title) => open = section_number(title).as_deref() == Some(number),
            None if open => found.push(line),
            None => {}
        }
    }
    found.join("\n")
}

/// `## N. …` の N（数字の列だけ・それ以外は `None`）。
fn section_number(title: &str) -> Option<String> {
    let (head, _) = title.split_once('.')?;
    (!head.is_empty() && head.chars().all(|found| found.is_ascii_digit())).then(|| head.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{names, section_text};

    /// 名指しは id の字面の境界で測る（`s2-x.7` は `s2-x.70` にも `s2-x.7.1` にも当たらず、文末の `.` には当たる）。
    #[test]
    fn ledger_form_names_respects_id_boundaries() {
        assert!(names("見た s2-x.7 の件", "s2-x.7"));
        assert!(names("s2-x.7.", "s2-x.7"), "文末の点");
        assert!(!names("s2-x.70", "s2-x.7"));
        assert!(!names("s2-x.7.1", "s2-x.7"));
        assert!(!names("as2-x.7", "s2-x.7"));
        assert!(!names("anything", ""));
    }

    /// § の本文は見出しの次から次の `## ` の前まで（契約表の区間は数えない）。
    #[test]
    fn ledger_form_section_text_reads_one_section() {
        let doc = "# t\n\n## 1. a\n\none\n\n## 2. b\n\ntwo\n<!-- contracts:begin -->\n## 1. x\n<!-- contracts:end -->\n";
        assert_eq!(section_text(doc, "1"), "\none\n");
        assert_eq!(section_text(doc, "2"), "\ntwo");
        assert_eq!(section_text(doc, "3"), "");
    }
}
