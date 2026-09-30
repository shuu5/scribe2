//! main の側の部品と着地の commit の misfit（設計 docs/design/case-lifecycle.md §10・FR90・FR92）。
//!
//! 純関数 1 本（[`derive`]）が、切り替えの線より後の first-parent の commit（呼び手が渡す・受けた commit は全部判じる）・線の時刻・
//! event log の run id・台帳の読み・契約表の行・要件 id を受け、commit・row・requirement の部品と、器の便の commit の結び
//! （便の run id と契約の bead id ごとの sha・書き手が `links.commits` に写す）を返す。I/O も時計も持たない。
//!
//! 窓は掛けない（終わりの閉じた部品への窓は書き手）。`owned` は持たない（数えは書き手）。要件の列か契約表の行が `None`（無いか
//! 読めない形）の周は、その種類の部品を 1 件も出さず `unmeasured` に名指す（0 件と書かない）。要件の局面は行の `req` から導くので、
//! 表が読めない周は requirement も `table-unreadable` で名指す。

use crate::case::{turn_of, Extra, Kind, Links, Misfit, Part, Phase, Turn};
use crate::fleet::cli::format_utc;
use crate::fleet::epoch_of;
use crate::ledger::close_reason::{self, Form, LandedTail};
use crate::ledger::form::pointer_text;
use crate::seat::ledger::Issue;
use std::collections::BTreeMap;

/// 閉じた bead の status。
const CLOSED: &str = "closed";

/// `unmeasured` の理由: 要件の面が無いか読める形でない。
pub const UNMEASURED_SRS: &str = "srs-unreadable";

/// `unmeasured` の理由: 契約表が無いか読める形でない。
pub const UNMEASURED_TABLE: &str = "table-unreadable";

/// main の commit 1 本（呼び手が trailer を読んで渡す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// 40 桁の sha。
    pub sha: String,
    /// commit の時刻（UNIX 秒）。
    pub at: u64,
    /// 発端の trailer の id の列（無ければ空）。
    pub sources: Vec<String>,
    /// 器の便の trailer `run:` の値（無ければ `None`）。
    pub run: Option<String>,
    /// 契約の trailer の bead id の列（無ければ空）。
    pub contracts: Vec<String>,
}

/// 契約表の行 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// 便の `--design` の pointer の字のまま（`.md#<行 id>` か `.toml#<行 id>`）。
    pub pointer: String,
    /// 行の `req` の要件 id。
    pub req: Vec<String>,
}

/// [`derive`] の入力（全部を呼び手が集めて渡す）。
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// 切り替えの線より後の commit。
    pub commits: &'a [Commit],
    /// 切り替えの線の時刻（UNIX 秒・row の閉じの読みに使う）。
    pub cutover: u64,
    /// event log の run id の集合。
    pub runs: &'a [String],
    /// 台帳の読み（閉じた bead を含む全部）。
    pub issues: &'a [Issue],
    /// 契約表の行（無いか読めない形なら `None`）。
    pub rows: Option<&'a [Row]>,
    /// 要件 id の列（無いか読めない形なら `None`）。
    pub requirements: Option<&'a [String]>,
}

/// 器の便の commit の結び（sha の列・入力の順）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ties {
    /// 便の run id ごとの sha。
    pub runs: BTreeMap<String, Vec<String>>,
    /// 契約の bead id ごとの sha。
    pub beads: BTreeMap<String, Vec<String>>,
}

/// [`derive`] の出力。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// 部品（commit・row・requirement の順）。`overdue` は書き手が決めるので `None`。
    pub parts: Vec<Part>,
    /// 器の便の commit の結び。
    pub ties: Ties,
    /// 測れなかった種類と理由の語。
    pub unmeasured: Vec<(Kind, &'static str)>,
}

/// main の側の部品を導く（**純関数**・§10）。
pub fn derive(input: &Input<'_>) -> Output {
    let mut parts: Vec<Part> = input.commits.iter().filter_map(|commit| commit_part(input, commit)).collect();
    let mut unmeasured = Vec::new();
    match input.rows {
        Some(rows) => parts.extend(rows.iter().map(|row| row_part(input, row))),
        None => unmeasured.push((Kind::Row, UNMEASURED_TABLE)),
    }
    match (input.requirements, input.rows) {
        (None, _) => unmeasured.push((Kind::Requirement, UNMEASURED_SRS)),
        (Some(_), None) => unmeasured.push((Kind::Requirement, UNMEASURED_TABLE)),
        (Some(ids), Some(rows)) => parts.extend(ids.iter().map(|id| requirement_part(id, rows))),
    }
    Output { parts, ties: ties_of(input), unmeasured }
}

/// 器の便の commit（`run:` が event log に在る）の結び。
fn ties_of(input: &Input<'_>) -> Ties {
    let mut ties = Ties::default();
    for commit in input.commits.iter().filter(|commit| is_vessel(input, commit)) {
        let Some(run) = &commit.run else { continue };
        ties.runs.entry(run.clone()).or_default().push(commit.sha.clone());
        let mut beads: Vec<&String> = commit.contracts.iter().collect();
        beads.dedup();
        for bead in beads {
            ties.beads.entry(bead.clone()).or_default().push(commit.sha.clone());
        }
    }
    ties
}

/// `run:` が event log に在る commit（器の便の commit）か。
fn is_vessel(input: &Input<'_>, commit: &Commit) -> bool {
    commit.run.as_ref().is_some_and(|run| input.runs.contains(run))
}

/// commit の部品（器の便の commit は `None`・先の語は run-trailer-unknown → commit-no-trailer か source-unresolved）。
fn commit_part(input: &Input<'_>, commit: &Commit) -> Option<Part> {
    let known = |id: &String| input.issues.iter().any(|issue| issue.id == *id);
    let word = match &commit.run {
        Some(_) if is_vessel(input, commit) => return None,
        Some(_) => Some(Misfit::RunTrailerUnknown),
        None if commit.sources.is_empty() => Some(Misfit::CommitNoTrailer),
        None if !commit.sources.iter().all(known) => Some(Misfit::SourceUnresolved),
        None => None,
    };
    let closed = commit.contracts.iter().any(|id| input.issues.iter().any(|issue| issue.id == *id && is_closed(issue)));
    let links = Links { source: commit.sources.clone(), ..Links::default() };
    Some(match word {
        Some(word) => part(Kind::Commit, &commit.sha, (Phase::Misfit, Some(word.as_str())), None, (closed, links)),
        None => part(Kind::Commit, &commit.sha, (Phase::CommitLanded, None), Some(commit.at), (closed, links)),
    })
}

/// row の部品（開いた bead が在れば beaded・着地が在れば landed・どちらも無ければ unbeaded）。
fn row_part(input: &Input<'_>, row: &Row) -> Part {
    let beads: Vec<&Issue> = input.issues.iter().filter(|issue| pointer_text(&issue.acceptance) == Some(row.pointer.as_str())).collect();
    let mut open = beads.iter().filter(|issue| !is_closed(issue)).peekable();
    let (phase, since) = if open.peek().is_some() {
        (Phase::RowBeaded, open.filter_map(|issue| time_of(issue.created_at.as_deref())).max())
    } else {
        let closed = beads.iter().filter(|issue| landed_close(issue, input.cutover)).map(|issue| time_of(issue.closed_at.as_deref()));
        let named = input.commits.iter().filter(|commit| beads.iter().any(|issue| commit.contracts.contains(&issue.id))).map(|commit| Some(commit.at));
        let times: Vec<Option<u64>> = closed.chain(named).collect();
        match times.is_empty() {
            true => (Phase::RowUnbeaded, None),
            false => (Phase::RowLanded, times.into_iter().flatten().max()),
        }
    };
    part(Kind::Row, &row.pointer, (phase, None), since, (phase == Phase::RowLanded, Links::default()))
}

/// 閉じた bead が着地と読めるか（線より前の閉じは形を問わず着地・線より後と閉じた時刻が読めない閉じは着地の形だけ）。
fn landed_close(issue: &Issue, cutover: u64) -> bool {
    let landed = matches!(close_reason::read(&issue.close_reason, None), Ok(Form::Landed { tail, .. }) if !matches!(tail, LandedTail::Unreadable(_)));
    is_closed(issue) && (landed || time_of(issue.closed_at.as_deref()).is_some_and(|at| at <= cutover))
}

/// requirement の部品（どの行の `req` にも無ければ unrowed）。
fn requirement_part(id: &str, rows: &[Row]) -> Part {
    let phase = if rows.iter().any(|row| row.req.iter().any(|req| req == id)) { Phase::RequirementRowed } else { Phase::RequirementUnrowed };
    part(Kind::Requirement, id, (phase, None), None, (false, Links::default()))
}

/// 部品 1 つ（手番は表から引く・この行の語は全部表に在る）。
fn part(kind: Kind, id: &str, (phase, reason): (Phase, Option<&str>), since: Option<u64>, (closed, links): (bool, Links)) -> Part {
    Part {
        part: kind,
        id: id.to_owned(),
        phase,
        turn: turn_of(phase.as_str(), reason).unwrap_or(Turn::Seat),
        since: since.map(format_utc),
        reason: reason.map(str::to_owned),
        closed,
        overdue: None,
        links,
        extra: Extra::None,
    }
}

/// bead が閉じたか。
fn is_closed(issue: &Issue) -> bool {
    issue.status == CLOSED
}

/// 台帳の時刻の字（秒の小数を持つ形も許す）を UNIX 秒へ（無いか読めなければ `None`）。
fn time_of(text: Option<&str>) -> Option<u64> {
    let text = text?;
    epoch_of(text).or_else(|| {
        let (head, tail) = text.split_once('.')?;
        let digits = tail.strip_suffix('Z')?;
        (!digits.is_empty() && digits.bytes().all(|found| found.is_ascii_digit())).then(|| epoch_of(&format!("{head}Z")))?
    })
}

#[cfg(test)]
mod tests {
    use super::{derive, Commit, Input, Output, Row, UNMEASURED_SRS, UNMEASURED_TABLE};
    use crate::case::Kind;
    use crate::fleet::epoch_of;
    use crate::seat::ledger::{issues_of, Issue};

    /// 切り替えの線（09-20）と、線より後の時刻。
    const LINE: &str = "2026-09-20T00:00:00Z";
    const LATE: &str = "2026-09-28T00:00:00Z";

    /// 台帳を JSON の字から作る。
    fn ledger(items: &[String]) -> Vec<Issue> {
        issues_of(&format!("[{}]", items.join(","))).expect("fixture の JSON を読める")
    }

    /// 設計 pointer を持つ bead の JSON（`extra` は `,` で始まる追加の key）。
    fn bead(id: &str, status: &str, pointer: &str, extra: &str) -> String {
        format!(r#"{{"id":"{id}","status":"{status}","acceptance_criteria":"design = {pointer}"{extra}}}"#)
    }

    /// 閉じた bead の JSON（理由と閉じた時刻つき）。
    fn closed(id: &str, pointer: &str, reason: &str, when: &str) -> String {
        bead(id, "closed", pointer, &format!(r#","close_reason":"{reason}","closed_at":"{when}""#))
    }

    fn commit(sha: &str, run: Option<&str>, sources: &[&str], contracts: &[&str]) -> Commit {
        let owned = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect();
        Commit { sha: sha.to_owned(), at: epoch_of(LATE).expect("時刻"), sources: owned(sources), run: run.map(str::to_owned), contracts: owned(contracts) }
    }

    fn row(pointer: &str, req: &[&str]) -> Row {
        Row { pointer: pointer.to_owned(), req: req.iter().map(|id| (*id).to_owned()).collect() }
    }

    /// 周の入力（線 09-20・event log の run は `r-known`・要件 FR1 と FR2）で導く。
    fn run(issues: &[Issue], commits: &[Commit], rows: Option<&[Row]>, reqs: Option<&[&str]>) -> Output {
        let runs = ["r-known".to_owned()];
        let reqs: Option<Vec<String>> = reqs.map(|ids| ids.iter().map(|id| (*id).to_owned()).collect());
        derive(&Input { commits, cutover: epoch_of(LINE).expect("線"), runs: &runs, issues, rows, requirements: reqs.as_deref() })
    }

    /// (id・局面・理由・手番) の列。
    fn shape(out: &Output) -> Vec<(String, &'static str, Option<String>, &'static str)> {
        out.parts.iter().map(|part| (part.id.clone(), part.phase.as_str(), part.reason.clone(), part.turn.as_str())).collect()
    }

    fn want(items: &[(&str, &'static str, Option<&str>, &'static str)]) -> Vec<(String, &'static str, Option<String>, &'static str)> {
        items.iter().map(|(id, phase, reason, turn)| ((*id).to_owned(), *phase, reason.map(str::to_owned), *turn)).collect()
    }

    /// commit の 4 語（各 1 fixture）。`run:` が無く発端も台帳に無い commit は run-trailer-unknown（先の語）。器の便の commit は部品にならない。
    #[test]
    fn phase_main_commit_words_and_the_first_word_wins() {
        let issues = ledger(&[bead("s2-a", "open", "docs/design/x.md#z", "")]);
        let commits = [
            commit("c1", None, &["s2-a"], &[]),
            commit("c2", None, &["s2-a", "s2-gone"], &[]),
            commit("c3", None, &[], &[]),
            commit("c4", Some("r-unknown"), &["s2-gone"], &[]),
            commit("c5", Some("r-known"), &[], &[]),
        ];
        let out = run(&issues, &commits, Some(&[]), Some(&[]));
        let expected = want(&[
            ("c1", "commit-landed", None, "none"),
            ("c2", "misfit", Some("source-unresolved"), "seat"),
            ("c3", "misfit", Some("commit-no-trailer"), "seat"),
            ("c4", "misfit", Some("run-trailer-unknown"), "seat"),
        ]);
        assert_eq!(shape(&out), expected, "器の便 c5 は部品にしない");
        assert_eq!(out.parts[0].since.as_deref(), Some(LATE), "commit-landed の since は commit の時刻");
        assert_eq!(out.parts[0].links.source, ["s2-a"], "commit-landed の links.source は発端");
        assert_eq!(out.parts[1].links.source, ["s2-a", "s2-gone"], "misfit も発端の id を結びに残す");
        assert_eq!(out.parts[3].since, None, "misfit は since を持たない");
    }

    /// 器の便の commit は結びの便の run id と契約の bead id の両方に載る（結びを捨てる実装と便の id だけに結ぶ実装を落とす）。
    #[test]
    fn phase_main_ties_carry_both_the_run_id_and_the_bead_ids() {
        let issues = ledger(&[bead("s2-a", "closed", "docs/design/x.md#z", "")]);
        let commits = [commit("v1", Some("r-known"), &[], &["s2-a", "s2-b"]), commit("v2", Some("r-known"), &[], &["s2-a"]), commit("u1", Some("r-other"), &[], &["s2-a"])];
        let out = run(&issues, &commits, Some(&[]), Some(&[]));
        let sha = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<String>>();
        assert_eq!(out.ties.runs.get("r-known"), Some(&sha(&["v1", "v2"])), "便の run id に載る");
        assert_eq!(out.ties.beads.get("s2-a"), Some(&sha(&["v1", "v2"])), "契約の bead id に載る（event log に無い便 u1 は結ばない）");
        assert_eq!(out.ties.beads.get("s2-b"), Some(&sha(&["v1"])), "台帳に無い bead id にも載る");
        assert_eq!(out.ties.runs.len(), 1, "結びは event log に在る便だけ");
        assert_eq!(out.parts.len(), 1, "u1 だけが部品（run-trailer-unknown）");
    }

    /// row の 3 局面（`.md` と `.toml` の pointer）・row-beaded の since は bead の `created_at`。
    #[test]
    fn phase_main_row_three_phases_for_md_and_toml() {
        let issues = ledger(&[
            bead("s2-md", "open", "docs/design/x.md#a", r#","created_at":"2026-09-27T01:02:03Z""#),
            closed("s2-toml", "docs/design/y.toml#b", "landed 0123456789abcdef0123456789abcdef01234567 ci=success", LATE),
        ]);
        let rows = [row("docs/design/x.md#a", &[]), row("docs/design/y.toml#b", &[]), row("docs/design/z.md#c", &[])];
        let out = run(&issues, &[], Some(&rows), Some(&[]));
        let expected = want(&[
            ("docs/design/x.md#a", "row-beaded", None, "none"),
            ("docs/design/y.toml#b", "row-landed", None, "none"),
            ("docs/design/z.md#c", "row-unbeaded", None, "seat"),
        ]);
        assert_eq!(shape(&out), expected);
        assert_eq!(out.parts[0].since.as_deref(), Some("2026-09-27T01:02:03Z"), "row-beaded の since は bead の created_at");
    }

    /// trailer の経路: 線より後に取り下げで閉じた bead を main の commit の契約の trailer が名指す行は row-landed、trailer の無い同じ行は row-unbeaded。
    #[test]
    fn phase_main_row_lands_through_the_trailer() {
        let issues = ledger(&[closed("s2-w", "docs/design/x.md#a", "取り下げ 不要", LATE)]);
        let rows = [row("docs/design/x.md#a", &[])];
        let with = run(&issues, &[commit("c1", None, &["s2-w"], &["s2-w"])], Some(&rows), Some(&[]));
        assert_eq!(shape(&with)[1], ("docs/design/x.md#a".to_owned(), "row-landed", None, "none"), "trailer が名指せば着地");
        assert_eq!(with.parts[1].since.as_deref(), Some(LATE), "since は着地の時刻");
        let other = run(&issues, &[commit("c1", None, &["s2-w"], &["s2-other"])], Some(&rows), Some(&[]));
        assert_eq!(other.parts[1].phase.as_str(), "row-unbeaded", "別の bead を名指す trailer は着地にしない");
        let without = run(&issues, &[], Some(&rows), Some(&[]));
        assert_eq!(without.parts[0].phase.as_str(), "row-unbeaded", "trailer の無い同じ行は unbeaded");
    }

    /// 線の前後の対: 線より前の取り下げの閉じは row-landed・線より後は row-unbeaded・線より後でも着地の形の閉じは row-landed。
    #[test]
    fn phase_main_row_line_splits_withdrawn_closes() {
        let sha = "landed 0123456789abcdef0123456789abcdef01234567 ci=success";
        let issues = ledger(&[
            closed("s2-old", "docs/design/x.md#old", "取り下げ 不要", "2026-09-10T00:00:00Z"),
            closed("s2-new", "docs/design/x.md#new", "取り下げ 不要", LATE),
            closed("s2-land", "docs/design/x.md#land", sha, LATE),
            closed("s2-badtail", "docs/design/x.md#bad", "landed 0123456789abcdef0123456789abcdef01234567 nonsense", LATE),
        ]);
        let rows = ["old", "new", "land", "bad"].map(|id| row(&format!("docs/design/x.md#{id}"), &[]));
        let out = run(&issues, &[], Some(&rows), Some(&[]));
        let phases: Vec<&str> = out.parts.iter().map(|part| part.phase.as_str()).collect();
        assert_eq!(phases, ["row-landed", "row-unbeaded", "row-landed", "row-unbeaded"]);
    }

    /// requirement の 2 局面（どの行の req にも無い要件は unrowed・手番 seat・在るなら rowed）。
    #[test]
    fn phase_main_requirement_two_phases() {
        let out = run(&[], &[], Some(&[row("docs/design/x.md#a", &["FR1"])]), Some(&["FR1", "FR2"]));
        let expected = want(&[
            ("docs/design/x.md#a", "row-unbeaded", None, "seat"),
            ("FR1", "requirement-rowed", None, "none"),
            ("FR2", "requirement-unrowed", None, "seat"),
        ]);
        assert_eq!(shape(&out), expected);
        assert!(out.unmeasured.is_empty(), "読めた周は unmeasured を持たない");
    }

    /// 読めない面は unmeasured に名指し、その種類の部品を 1 件も出さない（ほかの種類は出す）。
    #[test]
    fn phase_main_unmeasured_surfaces_emit_no_parts() {
        let issues = ledger(&[bead("s2-a", "open", "docs/design/x.md#a", "")]);
        let commits = [commit("c1", None, &["s2-a"], &[])];
        let rows = [row("docs/design/x.md#a", &["FR1"])];
        let srs = run(&issues, &commits, Some(&rows), None);
        assert_eq!(srs.unmeasured, [(Kind::Requirement, UNMEASURED_SRS)]);
        assert!(srs.parts.iter().all(|part| part.part != Kind::Requirement) && srs.parts.len() == 2, "row と commit は出す");
        let table = run(&issues, &commits, None, Some(&["FR1"]));
        assert_eq!(table.unmeasured, [(Kind::Row, UNMEASURED_TABLE), (Kind::Requirement, UNMEASURED_TABLE)]);
        assert_eq!(table.parts.len(), 1, "commit だけ出す（表が読めなければ要件も row も測れない）");
        let both = run(&issues, &commits, None, None);
        assert_eq!(both.unmeasured, [(Kind::Row, UNMEASURED_TABLE), (Kind::Requirement, UNMEASURED_SRS)]);
        assert!(both.parts.iter().all(|part| part.part == Kind::Commit), "row と requirement は 0 件");
    }

    /// `closed` の 5 形: 閉じた bead を名指す commit-landed は真・開いた bead か無い bead か trailer の無い commit は偽・row-landed だけ真・requirement は偽。
    #[test]
    fn phase_main_closed_five_shapes() {
        let sha = "landed 0123456789abcdef0123456789abcdef01234567 ci=success";
        let issues = ledger(&[
            closed("s2-c", "docs/design/x.md#a", sha, LATE),
            bead("s2-o", "open", "docs/design/x.md#b", ""),
            bead("s2-k", "open", "docs/design/x.md#k", ""),
        ]);
        let commits = [
            commit("named-closed", None, &["s2-c"], &["s2-c"]),
            commit("named-open", None, &["s2-c"], &["s2-o"]),
            commit("named-none", None, &["s2-c"], &["s2-gone"]),
            commit("no-trailer", None, &[], &[]),
            commit("misfit-named-closed", None, &["s2-gone"], &["s2-c"]),
        ];
        let rows = [row("docs/design/x.md#a", &["FR1"]), row("docs/design/x.md#b", &[]), row("docs/design/x.md#c", &[])];
        let out = run(&issues, &commits, Some(&rows), Some(&["FR1", "FR9"]));
        let flags: Vec<(&str, bool)> = out.parts.iter().map(|part| (part.phase.as_str(), part.closed)).collect();
        let expected = [
            ("commit-landed", true),
            ("commit-landed", false),
            ("commit-landed", false),
            ("misfit", false),
            ("misfit", true),
            ("row-landed", true),
            ("row-beaded", false),
            ("row-unbeaded", false),
            ("requirement-rowed", false),
            ("requirement-unrowed", false),
        ];
        assert_eq!(flags, expected);
    }
}
