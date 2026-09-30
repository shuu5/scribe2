//! 台帳のグラフの形（設計 docs/design/ledger-form.md §10・契約表の行 f・FR51）。
//!
//! doctor の項目 1 行（`ledger-graph: …`）に、epic の木の崩れを**件数と id** で出す: 根の epic に着かない bead（open と
//! closed）・鎖の終わりの非 epic（top）・2 つ目の親・親の輪・直下の open の子が上限 N を越える親・子が全部 closed の
//! open な epic。blocks の輪は数えない（bd が書きの時点で断る・§10 の決定はしご）。
//!
//! 判定は [`judge`] の**純関数**（入力は [`Issue`] の列と上限 N だけ）。親は `deps` の parent-child の最初の 1 本、根は
//! 親を持たない epic で、付け先が根に着くか・top・直下の open の子の数・子孫かの問いは [`Graph`] が外へ見せる（起票の門
//! §12 が同じ問いを引く）。上限 N は rules 行 [`ROW`] を id で引いて整数だけを読む。読めない台帳・読めない rules・行の
//! 無い rules は件数 0 に倒さず測れていない形の行を出す（[`render_unreadable`]・C10 / NFR4）。台帳は読むだけ（C15）。

use crate::rules::int_row;
use crate::rules::manifest::Manifest;
use crate::seat::ledger::{self, Issue, LedgerError};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// 行の先頭の字面。
pub const PREFIX: &str = "ledger-graph:";

/// 直下の open の子の上限を宣言する rules 行の id（**値は code に焼かない**・C5）。
pub const ROW: &str = "ledger.open_children_max";

/// 親子の辺の種別。
const PARENT_CHILD: &str = "parent-child";

/// 根になれる型。
const EPIC: &str = "epic";

/// 閉じた bead の status。
const CLOSED: &str = "closed";

/// 常設・閉じない bead の status（直下の open の子に数えない）。
const PINNED: &str = "pinned";

/// 違反が 1 つ以上の周に行の末尾へ 1 回添える直す形。
const FIX: &str = "top は bdw update <top> --parent <epic> か --type epic・親 2 つと親の輪は bdw update <子> --parent \
                   <epic> で 1 本に置き換える・溢れは bdw create <題> --type epic --parent <親> の子 epic へ付け替える・\
                   close-eligible は bdw close <epic> --reason 完了";

/// 親をたどった行き先。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach<'a> {
    /// 根の epic（親を持たない epic）に着いた（値は根の id）。
    Root(&'a str),
    /// 親を持たない非 epic で止まった（値は鎖の終わり＝top の id）。
    Top(&'a str),
    /// 親が台帳に無い（始点が台帳に無い周も）。
    Missing,
    /// 親の輪に入った。
    Loop,
}

impl Reach<'_> {
    /// 根に着いたか。
    pub fn is_rooted(self) -> bool {
        matches!(self, Self::Root(_))
    }
}

/// 台帳の親子の索引（id → bead・id → 直下の子）。
#[derive(Debug, Clone, Default)]
pub struct Graph<'a> {
    /// id → bead。
    by_id: BTreeMap<&'a str, &'a Issue>,
    /// 親の id → 直下の子（親は最初の parent-child）。
    children: BTreeMap<&'a str, Vec<&'a Issue>>,
}

/// 親（`deps` の parent-child の最初の 1 本）。
pub fn parent_of(issue: &Issue) -> Option<&str> {
    issue.deps.iter().find(|dep| dep.kind == PARENT_CHILD).map(|dep| dep.on.as_str())
}

/// 直下の open の子に数えるか（closed でも pinned でもない）。
fn is_live(issue: &Issue) -> bool {
    issue.status != CLOSED && issue.status != PINNED
}

impl<'a> Graph<'a> {
    /// 台帳の列から索引を作る。
    pub fn of(issues: &'a [Issue]) -> Self {
        let mut graph = Self::default();
        for issue in issues {
            graph.by_id.insert(issue.id.as_str(), issue);
            if let Some(parent) = parent_of(issue) {
                graph.children.entry(parent).or_default().push(issue);
            }
        }
        graph
    }

    /// `id` から親をたどった行き先。
    pub fn reach(&self, id: &str) -> Reach<'a> {
        let mut seen = BTreeSet::new();
        let mut at = id;
        loop {
            let Some(issue) = self.by_id.get(at).copied() else {
                return Reach::Missing;
            };
            if !seen.insert(issue.id.as_str()) {
                return Reach::Loop;
            }
            match parent_of(issue) {
                Some(parent) => at = parent,
                None if issue.kind == EPIC => return Reach::Root(issue.id.as_str()),
                None => return Reach::Top(issue.id.as_str()),
            }
        }
    }

    /// 付け先 `id` が根に着くか。
    pub fn is_rooted(&self, id: &str) -> bool {
        self.reach(id).is_rooted()
    }

    /// 根に着かない鎖の top（親を持たない非 epic で止まった周だけ）。
    pub fn top(&self, id: &str) -> Option<&'a str> {
        match self.reach(id) {
            Reach::Top(found) => Some(found),
            _ => None,
        }
    }

    /// `id` の直下の open の子（closed でも pinned でもない子）の数。
    pub fn open_children(&self, id: &str) -> usize {
        self.children.get(id).map_or(0, |found| found.iter().filter(|child| is_live(child)).count())
    }

    /// `id` が `ancestor` の子孫か（`id` の親から上へたどって `ancestor` に会う・`id` 自身は数えない）。
    pub fn is_descendant(&self, id: &str, ancestor: &str) -> bool {
        let mut seen = BTreeSet::new();
        let mut at = self.by_id.get(id).and_then(|issue| parent_of(issue));
        while let Some(parent) = at {
            if parent == ancestor {
                return true;
            }
            if !seen.insert(parent) {
                return false;
            }
            at = self.by_id.get(parent).and_then(|issue| parent_of(issue));
        }
        false
    }
}

/// 数えた崩れ（id は昇順）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// bead の件数。
    pub beads: usize,
    /// closed でない bead の件数。
    pub open: usize,
    /// 直下の open の子の上限（0 は over を数えない）。
    pub max: u64,
    /// 根に着かない closed でない bead の件数。
    pub unrooted: usize,
    /// 根に着かない closed の bead の件数。
    pub unrooted_closed: usize,
    /// open を含む鎖の top。
    pub tops: Vec<String>,
    /// closed だけの鎖の top の件数。
    pub tops_closed: usize,
    /// 親を 2 つ以上持つ bead。
    pub two_parents: Vec<String>,
    /// 親の輪に乗る bead。
    pub parent_loops: Vec<String>,
    /// 直下の open の子が上限を越える親（`<id>/<子の数>`・上限 0 の周は `None`）。
    pub over: Option<Vec<String>>,
    /// 子を 1 本以上持ち、子が全部 closed の open な epic。
    pub close_eligible: Vec<String>,
}

impl Report {
    /// 違反が 1 つ以上在るか。
    fn has_violation(&self) -> bool {
        self.unrooted > 0
            || self.unrooted_closed > 0
            || !self.two_parents.is_empty()
            || !self.parent_loops.is_empty()
            || self.over.as_ref().is_some_and(|found| !found.is_empty())
            || !self.close_eligible.is_empty()
    }
}

/// 台帳のグラフの崩れを数える（**純関数**・§10 形 2）。`max` は直下の open の子の上限（0 は数えない）。
pub fn judge(issues: &[Issue], max: u64) -> Report {
    let graph = Graph::of(issues);
    let mut chains: BTreeMap<&str, bool> = BTreeMap::new();
    let (mut unrooted, mut unrooted_closed) = (0, 0);
    for issue in issues {
        let reach = graph.reach(&issue.id);
        if reach.is_rooted() {
            continue;
        }
        let open = issue.status != CLOSED;
        if open { unrooted += 1 } else { unrooted_closed += 1 }
        if let Reach::Top(top) = reach {
            *chains.entry(top).or_default() |= open;
        }
    }
    let over = (max > 0).then(|| {
        graph
            .by_id
            .keys()
            .map(|id| (*id, graph.open_children(id)))
            .filter(|(_, count)| u64::try_from(*count).is_ok_and(|count| count > max))
            .map(|(id, count)| format!("{id}/{count}"))
            .collect()
    });
    Report {
        beads: issues.len(),
        open: issues.iter().filter(|issue| issue.status != CLOSED).count(),
        max,
        unrooted,
        unrooted_closed,
        tops: chains.iter().filter(|(_, open)| **open).map(|(top, _)| (*top).to_owned()).collect(),
        tops_closed: chains.values().filter(|open| !**open).count(),
        two_parents: ids(issues.iter().filter(|issue| parents(issue) > 1)),
        parent_loops: ids(issues.iter().filter(|issue| graph.is_descendant(&issue.id, &issue.id))),
        over,
        close_eligible: ids(issues.iter().filter(|issue| is_close_eligible(&graph, issue))),
    }
}

/// 違う親の数（parent-child の辺の先の id の種類）。
fn parents(issue: &Issue) -> usize {
    issue.deps.iter().filter(|dep| dep.kind == PARENT_CHILD).map(|dep| dep.on.as_str()).collect::<BTreeSet<_>>().len()
}

/// 子を 1 本以上持ち、子が全部 closed の open な epic か。
fn is_close_eligible(graph: &Graph<'_>, issue: &Issue) -> bool {
    issue.kind == EPIC
        && is_live(issue)
        && graph
            .children
            .get(issue.id.as_str())
            .is_some_and(|found| !found.is_empty() && found.iter().all(|child| child.status == CLOSED))
}

/// id の列（昇順）。
fn ids<'i>(issues: impl Iterator<Item = &'i Issue>) -> Vec<String> {
    let mut found: Vec<String> = issues.map(|issue| issue.id.clone()).collect();
    found.sort();
    found
}

/// id の欄（`<語>=<件数>` と、1 件以上なら `:<id>,<id>…`）。
fn field(word: &str, found: &[String]) -> String {
    if found.is_empty() {
        format!("{word}=0")
    } else {
        format!("{word}={}:{}", found.len(), found.join(","))
    }
}

/// 測れた周の 1 行（違反 0 の周も行が出る・違反が 1 つ以上の周だけ末尾に ` — ` と直す形を 1 回）。
pub fn render(report: &Report) -> String {
    let over = report.over.as_deref().map_or_else(|| "over=-".to_owned(), |found| field("over", found));
    let mut line = format!(
        "{PREFIX} beads={} open={} max={} unrooted={} unrooted-closed={} {} tops-closed={} {} {} {over} {}",
        report.beads,
        report.open,
        report.max,
        report.unrooted,
        report.unrooted_closed,
        field("tops", &report.tops),
        report.tops_closed,
        field("two-parents", &report.two_parents),
        field("parent-loops", &report.parent_loops),
        field("close-eligible", &report.close_eligible),
    );
    if report.has_violation() {
        line.push_str(" — ");
        line.push_str(FIX);
    }
    line
}

/// 測れていない周の 1 行（件数を 1 つも出さない＝0 に化けさせない・C10）。
pub fn render_unreadable(reason: &str) -> String {
    format!("{PREFIX} unreadable reason={reason}")
}

/// doctor の項目 1 行（`--repo R` の台帳を読み、[`judge`] を撃つ）。`rules` は待ち上限と上限 N を読む manifest（無ければ
/// 埋め込み）。台帳 client は既定の名（PATH 解決）で、子 process の cwd は `repo`。
pub fn doctor_line(repo: &Path, rules: Option<&str>) -> String {
    match measure(repo, rules) {
        Ok(report) => render(&report),
        Err(reason) => render_unreadable(reason),
    }
}

/// 読みの全部（manifest → 待ち上限と上限 N → 台帳）。読めない周は理由の語。
fn measure(repo: &Path, rules: Option<&str>) -> Result<Report, &'static str> {
    let manifest = rules
        .map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path)))
        .map_err(|_| "rules-unreadable")?;
    let timeout = ledger::timeout_of(&manifest).ok_or("no-rule")?;
    let max = int_row(&manifest, ROW).map_err(|_| "no-rule")?;
    let issues = ledger::read_ledger(ledger::DEFAULT_BD, repo, timeout).map_err(|error| match error {
        LedgerError::Unreadable => "ledger-unreadable",
        LedgerError::Timeout => "ledger-timeout",
    })?;
    Ok(judge(&issues, max))
}

#[cfg(test)]
mod tests {
    use super::{judge, render, Graph, Reach};
    use crate::seat::ledger::issues_of;

    /// 1 件の JSON（`parents` は parent-child の辺の先・順のまま）。
    fn bead(id: &str, status: &str, kind: &str, parents: &[&str]) -> String {
        let deps: Vec<String> = parents
            .iter()
            .map(|parent| format!("{{\"issue_id\":\"{id}\",\"depends_on_id\":\"{parent}\",\"type\":\"parent-child\"}}"))
            .collect();
        format!("{{\"id\":\"{id}\",\"status\":\"{status}\",\"issue_type\":\"{kind}\",\"dependencies\":[{}]}}", deps.join(","))
    }

    /// 行き先は根・top・台帳に無い親・輪の 4 つで、子孫は親をたどって測り、pinned と closed の子は数えない。
    #[test]
    fn ledger_graph_reach_names_root_top_missing_and_loop() {
        let json = format!(
            "[{}]",
            [
                bead("r", "open", "epic", &[]),
                bead("r.1", "open", "task", &["r"]),
                bead("r.2", "pinned", "task", &["r"]),
                bead("r.3", "closed", "task", &["r"]),
                bead("r.1.1", "open", "task", &["r.1"]),
                bead("f", "open", "feature", &[]),
                bead("f.1", "open", "task", &["f"]),
                bead("g", "open", "task", &["gone"]),
                bead("l1", "open", "task", &["l2"]),
                bead("l2", "open", "task", &["l1"]),
                bead("l3", "open", "task", &["l1"]),
            ]
            .join(",")
        );
        let issues = issues_of(&json).unwrap_or_default();
        assert_eq!(issues.len(), 11, "fixture を読める");
        let graph = Graph::of(&issues);
        assert_eq!(graph.reach("r.1.1"), Reach::Root("r"));
        assert_eq!(graph.reach("f.1"), Reach::Top("f"));
        assert_eq!(graph.top("f.1"), Some("f"));
        assert_eq!(graph.reach("g"), Reach::Missing);
        assert_eq!(graph.reach("nothing"), Reach::Missing);
        assert_eq!(graph.reach("l3"), Reach::Loop);
        assert!(graph.is_rooted("r.1") && !graph.is_rooted("f"));
        assert_eq!(graph.open_children("r"), 1, "pinned と closed の子は数えない");
        assert!(graph.is_descendant("r.1.1", "r") && !graph.is_descendant("r", "r.1.1"));
        assert!(graph.is_descendant("l1", "l1") && !graph.is_descendant("l3", "l3"), "輪に乗るのは l1 と l2 だけ");
    }

    /// 上限 0 は over を数えず `-`、違反 0 の周は直す形を持たない。
    #[test]
    fn ledger_graph_render_marks_zero_max_and_clean_ledger() {
        let json = format!("[{},{}]", bead("r", "open", "epic", &[]), bead("r.1", "open", "task", &["r"]));
        let issues = issues_of(&json).unwrap_or_default();
        let clean = "ledger-graph: beads=2 open=2 max=1 unrooted=0 unrooted-closed=0 tops=0 tops-closed=0 \
                     two-parents=0 parent-loops=0 over=0 close-eligible=0";
        assert_eq!(render(&judge(&issues, 1)), clean);
        assert_eq!(render(&judge(&issues, 0)), clean.replace("max=1", "max=0").replace("over=0", "over=-"));
    }
}
