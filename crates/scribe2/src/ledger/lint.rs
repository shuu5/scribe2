//! 台帳 lint（設計 docs/design/contract-source.md §6・契約表の行 e・FR51）。
//!
//! doctor の項目 1 行（`ledger: …`）に 3 つの欠陥を**件数と母集団と id** で出す: (i) 契約（acceptance が `design =`
//! で始まる bead）の pointer が解けない（doc が無い・区間に id が無い）(ii) memo（label `intake:memo`）の本文に見出し
//! [`MEMO_HEADING`] が在り `design =` / `research =` の pointer 行が無い (iii) 契約の acceptance が pointer の 1 行を
//! 超える本文を持つ。
//!
//! 判定は [`judge`] の**純関数**（入力は [`Issue`] の列と、解けた pointer の字面の集合）。読みの口は席の側の 1 本
//! （[`crate::seat::ledger::read_ledger`]）を借り、doctor の台帳の 2 行（本行と [`super::form`]）は
//! [`crate::seat::ledger::one_read`] で**同じ 1 回の出力**を分けて読む。読めない周は件数 0 に倒さず測れていない形の
//! 行を出す（[`render_unreadable`]・C10 / NFR4）。極性は増やさない（doctor は読むだけで判定しない・C10.2）。

use crate::pipe::table;
use crate::rules::manifest::Manifest;
use crate::seat::ledger::{self, Issue, LedgerError};
use std::collections::BTreeSet;
use std::path::Path;

/// 行の先頭の字面。
pub const PREFIX: &str = "ledger:";

/// memo の識別の label。
pub const MEMO_LABEL: &str = "intake:memo";

/// memo の本文の機械が読む設計の見出し（固定の 1 つ）。
pub const MEMO_HEADING: &str = "## memo";

/// 契約の acceptance の頭（設計 pointer の行）。
const DESIGN_KEY: &str = "design =";

/// memo の pointer 行の頭（宣言順）。
const MEMO_POINTER_KEYS: [&str; 2] = [DESIGN_KEY, "research ="];

/// 閉じた bead の status（それ以外は open に数える）。
const CLOSED: &str = "closed";

/// 3 つの欠陥の件数と母集団と id（id は台帳の出力の順）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// closed でない bead の件数。
    pub open: usize,
    /// closed でない契約の件数（(i)(iii) の母集団）。
    pub contracts: usize,
    /// (i) pointer の解けない契約。
    pub unresolved: Vec<String>,
    /// (iii) pointer の 1 行を超える本文を持つ契約。
    pub bodied: Vec<String>,
    /// closed でない memo の件数（(ii) の母集団）。
    pub memos: usize,
    /// (ii) 見出しを持ち pointer 行を持たない memo。
    pub unpointed: Vec<String>,
}

/// 契約の pointer の字面（acceptance の先頭行の `design =` の後ろ・trim 済み）。契約でなければ `None`。
pub fn pointer_of(issue: &Issue) -> Option<&str> {
    issue.acceptance.trim_start().lines().next()?.trim().strip_prefix(DESIGN_KEY).map(str::trim)
}

/// memo か（label `intake:memo` を持つ）。
fn is_memo(issue: &Issue) -> bool {
    issue.labels.iter().any(|label| label == MEMO_LABEL)
}

/// acceptance が pointer の 1 行を超えるか（空でない行が 2 行以上）。
fn is_bodied(issue: &Issue) -> bool {
    issue.acceptance.lines().filter(|line| !line.trim().is_empty()).nth(1).is_some()
}

/// 本文（description と notes）と acceptance の行。
fn lines_of(issue: &Issue) -> impl Iterator<Item = &str> {
    issue.description.lines().chain(issue.notes.lines()).chain(issue.acceptance.lines()).map(str::trim)
}

/// memo が見出しを持ち pointer 行を持たないか。
fn is_unpointed(memo: &Issue) -> bool {
    let headed = memo.description.lines().chain(memo.notes.lines()).any(|line| line.trim() == MEMO_HEADING);
    headed && !lines_of(memo).any(|line| MEMO_POINTER_KEYS.iter().any(|key| line.starts_with(key)))
}

/// id の列（入力の順）。
fn ids<'i>(issues: impl Iterator<Item = &'i Issue>) -> Vec<String> {
    issues.map(|issue| issue.id.clone()).collect()
}

/// 3 つの欠陥を数える（**純関数**）。`resolved` は解けた pointer の字面の集合。
pub fn judge(issues: &[Issue], resolved: &BTreeSet<String>) -> Report {
    let open: Vec<&Issue> = issues.iter().filter(|issue| issue.status != CLOSED).collect();
    let contracts: Vec<&Issue> = open.iter().copied().filter(|issue| pointer_of(issue).is_some()).collect();
    let memos: Vec<&Issue> = open.iter().copied().filter(|issue| is_memo(issue)).collect();
    Report {
        open: open.len(),
        contracts: contracts.len(),
        unresolved: ids(contracts.iter().copied().filter(|issue| pointer_of(issue).is_none_or(|text| !resolved.contains(text)))),
        bodied: ids(contracts.iter().copied().filter(|issue| is_bodied(issue))),
        memos: memos.len(),
        unpointed: ids(memos.iter().copied().filter(|memo| is_unpointed(memo))),
    }
}

/// 測れた周の 1 行（件数と母集団が同じ行・欠陥 0 の周も 0 と母集団が出る）。欠陥の id は数の後ろに、欠陥ごとに
/// `<語>:<id>,<id>…` の形で並べる（1 件以上の欠陥だけ）。
pub fn render(report: &Report) -> String {
    let mut line = format!(
        "{PREFIX} open={} contracts={} unresolved={} bodied={} memos={} unpointed={}",
        report.open,
        report.contracts,
        report.unresolved.len(),
        report.bodied.len(),
        report.memos,
        report.unpointed.len(),
    );
    for (word, found) in [("unresolved", &report.unresolved), ("bodied", &report.bodied), ("unpointed", &report.unpointed)] {
        if !found.is_empty() {
            line.push_str(&format!(" {word}:{}", found.join(",")));
        }
    }
    line
}

/// 測れていない周の 1 行（件数を 1 つも出さない＝0 に化けさせない・C10）。
pub fn render_unreadable(reason: &str) -> String {
    format!("{PREFIX} unreadable reason={reason}")
}

/// doctor の台帳の 2 行（本行 → [`super::form`] の行）。台帳は**1 回だけ**読み、2 行が同じ出力を分けて読む。
pub fn doctor_lines(repo: &Path, rules: Option<&str>) -> Vec<String> {
    ledger::one_read(|| vec![doctor_line(repo, rules), super::form::doctor_line(repo, rules)])
}

/// doctor の項目 1 行（`--repo R` の台帳と設計 doc を読み、[`judge`] を撃つ）。`rules` は待ち上限を読む manifest
/// （無ければ埋め込み）。台帳 client は既定の名（PATH 解決）で、子 process の cwd は `repo`。
pub fn doctor_line(repo: &Path, rules: Option<&str>) -> String {
    match measure(repo, rules) {
        Ok(report) => render(&report),
        Err(reason) => render_unreadable(reason),
    }
}

/// 読みの全部（manifest → 台帳 → pointer の先）。読めない周は理由の語。
fn measure(repo: &Path, rules: Option<&str>) -> Result<Report, &'static str> {
    let manifest = rules
        .map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path)))
        .map_err(|_| "rules-unreadable")?;
    let timeout = ledger::timeout_of(&manifest).ok_or("no-rule")?;
    let issues = ledger::read_ledger(ledger::DEFAULT_BD, repo, timeout).map_err(|error| match error {
        LedgerError::Unreadable => "ledger-unreadable",
        LedgerError::Timeout => "ledger-timeout",
    })?;
    let resolved = issues.iter().filter_map(pointer_of).filter(|text| resolves(repo, text)).map(str::to_owned).collect();
    Ok(judge(&issues, &resolved))
}

/// pointer が解けるか（形・doc の在る・区間に行 id が在る）。
fn resolves(repo: &Path, text: &str) -> bool {
    table::parse_pointer(text)
        .ok()
        .and_then(|pointer| table::read(repo, &pointer.path).ok().map(|doc| (pointer, doc)))
        .is_some_and(|(pointer, doc)| table::find_row(&pointer.path, &doc, &pointer.id).is_ok())
}

#[cfg(test)]
mod tests {
    use super::{judge, render, render_unreadable};
    use crate::seat::ledger::issues_of;
    use std::collections::BTreeSet;

    /// 3 つの欠陥の判定（件数を違える・解ける契約と見出しの無い memo は数えない・closed は母集団の外）。
    #[test]
    fn ledger_lint_judge_counts_each_defect_apart() {
        let json = r####"[
            {"id":"u1","status":"open","acceptance_criteria":"design = docs/design/x.md#z"},
            {"id":"b1","status":"open","acceptance_criteria":"design = docs/design/x.md#a\n本文"},
            {"id":"b2","status":"in_progress","acceptance_criteria":"design = docs/design/x.md#a\n\n本文\n"},
            {"id":"ok","status":"open","acceptance_criteria":"design = docs/design/x.md#a\n"},
            {"id":"m1","status":"open","labels":["intake:memo"],"description":"## memo\n本文"},
            {"id":"m2","status":"open","labels":["intake:memo"],"notes":"## memo"},
            {"id":"m3","status":"open","labels":["intake:memo"],"description":"## memo\n本文"},
            {"id":"mr","status":"open","labels":["intake:memo"],"description":"## memo\nresearch = docs/r.md"},
            {"id":"mn","status":"open","labels":["intake:memo"],"description":"見出し無し"},
            {"id":"mc","status":"closed","labels":["intake:memo"],"description":"## memo"},
            {"id":"t1","status":"open","acceptance_criteria":"本文だけ\ndesign = docs/design/x.md#z"}
        ]"####;
        let issues = issues_of(json).unwrap_or_default();
        let resolved: BTreeSet<String> = ["docs/design/x.md#a".to_owned()].into();
        let found = render(&judge(&issues, &resolved));
        assert_eq!(
            found,
            "ledger: open=10 contracts=4 unresolved=1 bodied=2 memos=5 unpointed=3 unresolved:u1 bodied:b1,b2 unpointed:m1,m2,m3"
        );
        assert_eq!(render(&judge(&[], &resolved)), "ledger: open=0 contracts=0 unresolved=0 bodied=0 memos=0 unpointed=0");
        assert_eq!(render_unreadable("ledger-unreadable"), "ledger: unreadable reason=ledger-unreadable");
    }
}
