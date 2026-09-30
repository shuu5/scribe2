//! 裁定の閉じの misfit 3 語（設計 docs/design/case-lifecycle.md §8・FR90・FR91・FR83）。
//!
//! 純関数 1 本（[`derive`]）が、台帳の読み（[`Issue`] の全件）・台帳の接頭辞・2 つの線の時刻・裁定 event の結び（問い id と裁定 id の組の列）を
//! 受け、misfit の (bead id・語) の列を返す。I/O も時計も持たない（呼び手が集めて渡す）。閉じの理由は [`close_reason::read`] で、
//! notes の裁定の行は [`ruling_row`] で読み、裁定 id の解け方と行の読みの写しを持たない。
//!
//! 閉じた問いの裁定と閉じた memo の見送りだけを判じる（種類と頭の食い違う閉じは局面の側の `close-kind-mismatch` が持つ）。1 つの閉じに
//! 返す語は 1 つまでで、表の順（`close-ruling-unresolved` → `close-ruling-not-bound` → `deferred-not-child-ruling`）の先の語。線の規則は
//! 局面の側と同じ（2 つの線がどちらも在り、閉じた時刻が両方より後の閉じだけ）で、接頭辞が解けない周は空の列を返す。

use crate::case::Misfit;
use crate::fleet::epoch_of;
use crate::ledger::close_reason::{self, ruling_row, Defect, Form, Head};
use crate::ledger::form::{is_memo, is_question};
use crate::ledger::phase::Lines;
use crate::seat::ledger::Issue;

/// 閉じた bead の status。
const CLOSED: &str = "closed";

/// 問いに数える型（label を持たない decision）。
const DECISION: &str = "decision";

/// 子から親へ張る edge の種別。
const PARENT_CHILD: &str = "parent-child";

/// [`derive`] の入力（全部を呼び手が集めて渡す）。
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// 台帳の読み（閉じた bead を含む全部）。
    pub issues: &'a [Issue],
    /// 台帳の接頭辞（解けない周は `None`）。
    pub prefix: Option<&'a str>,
    /// 2 つの線。
    pub lines: Lines,
    /// 裁定 event の結び（問い id と裁定 id の組の列）。
    pub bound: &'a [(String, String)],
}

/// 裁定の閉じの misfit を導く（**純関数**・§8）。
pub fn derive(input: &Input<'_>) -> Vec<(String, Misfit)> {
    let (Some(prefix), Some(cutover), Some(check)) = (input.prefix, input.lines.cutover, input.lines.close_check) else {
        return Vec::new();
    };
    let line = cutover.max(check);
    let after = |issue: &&Issue| issue.status == CLOSED && closed_at(issue).is_some_and(|closed| closed > line);
    input.issues.iter().filter(after).filter_map(|issue| word(input, prefix, issue).map(|found| (issue.id.clone(), found))).collect()
}

/// 閉じた bead 1 本の語（問いは裁定・memo は見送りだけ）。
fn word(input: &Input<'_>, prefix: &str, issue: &Issue) -> Option<Misfit> {
    let read = close_reason::read(&issue.close_reason, Some(prefix));
    match (is_ruling_question(issue), is_memo(issue), read) {
        (true, _, Err(Defect::Value(Head::Ruling))) | (false, true, Err(Defect::Value(Head::Deferred))) => Some(Misfit::CloseRulingUnresolved),
        (true, _, Ok(Form::Ruling(id))) => (!rulings_of(input, issue).contains(&id)).then_some(Misfit::CloseRulingNotBound),
        (false, true, Ok(Form::Deferred(id))) => {
            let mut child_rulings = input.issues.iter().filter(|child| is_child_of(child, issue)).flat_map(|child| rulings_of(input, child));
            (!child_rulings.any(|found| found == id)).then_some(Misfit::DeferredNotChildRuling)
        }
        _ => None,
    }
}

/// 問いの種類か（label か decision の型・§2 の順で memo より先）。
fn is_ruling_question(issue: &Issue) -> bool {
    is_question(issue) || issue.kind == DECISION
}

/// `child` が `memo` の子の問い（`parent-child` でその memo を親に持つ問い）か。
fn is_child_of(child: &Issue, memo: &Issue) -> bool {
    is_ruling_question(child) && child.deps.iter().any(|dep| dep.kind == PARENT_CHILD && dep.on == memo.id)
}

/// 問いに結んだ裁定 id（自分の notes の裁定の行と、裁定 event の結びの両方）。
fn rulings_of(input: &Input<'_>, question: &Issue) -> Vec<String> {
    let rows = question.notes.lines().filter_map(|line| ruling_row(line, input.prefix)).map(|row| row.id);
    let events = input.bound.iter().filter(|(bead, _)| *bead == question.id).map(|(_, ruling)| ruling.clone());
    rows.chain(events).collect()
}

/// 閉じた時刻（無いか読めなければ `None`・秒の小数を持つ形も許す）。
fn closed_at(issue: &Issue) -> Option<u64> {
    let text = issue.closed_at.as_deref()?;
    epoch_of(text).or_else(|| {
        let (head, tail) = text.split_once('.')?;
        let digits = tail.strip_suffix('Z')?;
        (!digits.is_empty() && digits.bytes().all(|found| found.is_ascii_digit())).then(|| epoch_of(&format!("{head}Z")))?
    })
}

#[cfg(test)]
mod tests {
    use super::{derive, Input};
    use crate::fleet::epoch_of;
    use crate::ledger::phase::Lines;
    use crate::seat::ledger::issues_of;

    /// 2 つの線より後の閉じた時刻（切り替え 09-20・close-check 09-25）。
    const LATE: &str = "2026-09-28T00:00:00Z";

    /// 問いの label。
    const QUESTION: &str = r#""labels":["intake:question"]"#;

    /// memo の label。
    const MEMO: &str = r#""labels":["intake:memo"]"#;

    /// 閉じた bead の JSON（`label` は label の字・`extra` は `,` で始まる追加の key）。
    fn closed(id: &str, label: &str, reason: &str, when: &str, extra: &str) -> String {
        format!(r#"{{"id":"{id}","status":"closed",{label},"close_reason":"{reason}","closed_at":"{when}"{extra}}}"#)
    }

    /// 閉じた問い。
    fn question(id: &str, reason: &str) -> String {
        closed(id, QUESTION, reason, LATE, "")
    }

    /// 閉じた memo。
    fn memo(id: &str, reason: &str) -> String {
        closed(id, MEMO, reason, LATE, "")
    }

    /// notes に裁定の行（5 欄）を 1 行持つ追加の key。
    fn row_of(ruling: &str, question: &str) -> String {
        format!(r#","notes":"{ruling} | {question} | 2026-09-30T00:00Z | seat | 逐語""#)
    }

    /// memo の子（または発見元）の問い。`edge` は依存の種別・`extra` は追加の key。
    fn child(id: &str, memo: &str, edge: &str, extra: &str) -> String {
        format!(r#"{{"id":"{id}","status":"open",{QUESTION},"dependencies":[{{"depends_on_id":"{memo}","type":"{edge}"}}]{extra}}}"#)
    }

    /// 周の入力（切り替え 09-20・close-check 09-25・接頭辞 s2・結び無し）を既定に、台帳を JSON の字から作って語を返す。
    fn run(items: &[String], prefix: Option<&str>, lines: (Option<&str>, Option<&str>), bound: &[(&str, &str)]) -> Vec<(String, String)> {
        let issues = issues_of(&format!("[{}]", items.join(","))).expect("fixture の JSON を読める");
        let at = |text: Option<&str>| text.map(|found| epoch_of(found).expect("時刻の字を読める"));
        let bound: Vec<(String, String)> = bound.iter().map(|(bead, ruling)| ((*bead).to_owned(), (*ruling).to_owned())).collect();
        let input = Input { issues: &issues, prefix, lines: Lines { cutover: at(lines.0), close_check: at(lines.1) }, bound: &bound };
        derive(&input).into_iter().map(|(id, word)| (id, word.as_str().to_owned())).collect()
    }

    /// 既定の線。
    const LINES: (Option<&str>, Option<&str>) = (Some("2026-09-20T00:00:00Z"), Some("2026-09-25T00:00:00Z"));

    /// (bead id・語) の期待。
    fn want(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(id, word)| ((*id).to_owned(), (*word).to_owned())).collect()
    }

    /// 解けない値と古い形（`<裁定 id>・束 …`）は問いも memo も close-ruling-unresolved。解けて結ばれた対照は 0 件。
    #[test]
    fn phase_ruling_unresolved_value_and_old_form_count() {
        let old = "裁定 s2-q3:20260930T0000Z-1・束 batch:x";
        let items = [
            question("s2-q1", "裁定 nonsense"),
            question("s2-q2", "裁定"),
            question("s2-q3", old),
            memo("s2-m1", "見送り nonsense"),
            memo("s2-m2", "見送り s2-m2:20260930T0000Z-1・束 batch:x"),
            closed("s2-ok", QUESTION, "裁定 batch:x", LATE, &row_of("batch:x", "s2-ok")),
        ];
        let got = run(&items, Some("s2"), LINES, &[]);
        let word = "close-ruling-unresolved";
        assert_eq!(got, want(&[("s2-q1", word), ("s2-q2", word), ("s2-q3", word), ("s2-m1", word), ("s2-m2", word)]), "解けた対照 s2-ok は 0 件");
    }

    /// 種類と頭の食い違う閉じ（裁定で閉じた memo・見送りで閉じた問い）は 0 件。対照の解けない裁定の問いは 1 件。
    #[test]
    fn phase_ruling_mismatched_head_is_not_counted_twice() {
        let items = [memo("s2-m", "裁定 nonsense"), question("s2-q", "見送り nonsense"), question("s2-ctl", "裁定 nonsense")];
        assert_eq!(run(&items, Some("s2"), LINES, &[]), want(&[("s2-ctl", "close-ruling-unresolved")]));
    }

    /// 裁定 id が notes の行にも event にも無い問いは not-bound。notes の行だけ・event だけの問いは 0 件（片方の経路だけを読む実装を落とす）。
    #[test]
    fn phase_ruling_not_bound_reads_the_notes_row_and_the_event() {
        let items = [
            question("s2-none", "裁定 batch:x"),
            closed("s2-other-row", QUESTION, "裁定 batch:x", LATE, &row_of("batch:y", "s2-other-row")),
            closed("s2-notes", QUESTION, "裁定 batch:x", LATE, &row_of("batch:x", "s2-notes")),
            question("s2-event", "裁定 batch:x"),
            question("s2-wrong-event", "裁定 batch:x"),
        ];
        let bound = [("s2-event", "batch:x"), ("s2-wrong-event", "batch:y"), ("s2-elsewhere", "batch:x")];
        let got = run(&items, Some("s2"), LINES, &bound);
        let word = "close-ruling-not-bound";
        assert_eq!(got, want(&[("s2-none", word), ("s2-other-row", word), ("s2-wrong-event", word)]));
    }

    /// `discovered-from` で結ぶ問いの裁定で閉じた見送りは deferred-not-child-ruling。`parent-child` の子の問いが（notes か event で）持つ見送りは 0 件。
    #[test]
    fn phase_ruling_deferred_counts_only_child_question_rulings() {
        let items = [
            memo("s2-ma", "見送り batch:x"),
            child("s2-qa", "s2-ma", "discovered-from", &row_of("batch:x", "s2-qa")),
            memo("s2-mb", "見送り batch:y"),
            child("s2-qb", "s2-mb", "parent-child", &row_of("batch:y", "s2-qb")),
            memo("s2-mc", "見送り batch:z"),
            child("s2-qc", "s2-mc", "parent-child", ""),
            memo("s2-md", "見送り batch:w"),
            child("s2-qd", "s2-md", "parent-child", &row_of("batch:v", "s2-qd")),
        ];
        let bound = [("s2-qc", "batch:z"), ("s2-qa", "batch:w")];
        let got = run(&items, Some("s2"), LINES, &bound);
        let word = "deferred-not-child-ruling";
        assert_eq!(got, want(&[("s2-ma", word), ("s2-md", word)]), "discovered-from の問いの結びは数えない・別の裁定の子も数えない");
    }

    /// 線の対: close-check の線・切り替えの線・接頭辞の有無。各対は同じ閉じで片方だけ 0 件・もう片方は 1 件。
    #[test]
    fn phase_ruling_lines_and_prefix_gate_the_count() {
        let at = |when: &str| vec![closed("s2-q", QUESTION, "裁定 nonsense", when, "")];
        let one = want(&[("s2-q", "close-ruling-unresolved")]);
        assert_eq!(run(&at("2026-09-24T00:00:00Z"), Some("s2"), LINES, &[]), want(&[]), "close-check の線より前は数えない");
        assert_eq!(run(&at("2026-09-26T00:00:00Z"), Some("s2"), LINES, &[]), one, "同じ閉じを線より後へ動かすと 1 件");
        let cutover_late = (Some("2026-09-27T00:00:00Z"), Some("2026-09-25T00:00:00Z"));
        assert_eq!(run(&at("2026-09-26T00:00:00Z"), Some("s2"), cutover_late, &[]), want(&[]), "切り替えの線より前は数えない");
        assert_eq!(run(&at(LATE), Some("s2"), (LINES.0, None), &[]), want(&[]), "close-check が None の周は数えない");
        assert_eq!(run(&at(LATE), Some("s2"), (None, LINES.1), &[]), want(&[]), "切り替えが None の周は数えない");
        assert_eq!(run(&at(LATE), Some("s2"), LINES, &[]), one, "同じ閉じに線を渡すと 1 件");
        assert_eq!(run(&at(LATE), None, LINES, &[]), want(&[]), "接頭辞が None の周は空の列");
    }
}
