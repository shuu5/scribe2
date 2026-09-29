//! 選択式の問いの道具（AskUserQuestion）を器を名乗る repo で例外なく止める 1 関数（設計 docs/design/vessel-hook.md §20・
//! ADR-0084・FR86 / AC56）。
//!
//! 入力は payload の tool 名と、宣言の question-route を読む遅延の読み手の 2 つだけで、rules・pane・host の面・event log・
//! payload の他の欄を読まない。`pre_tool_use` の最初（git dir を解く前・全部の門の前）に撃つ＝席か runner か・誰が開いた
//! session か・skill の文が求めたかに依らず止める。AskUserQuestion でない tool では読み手を呼ばない（git の子 process を足さない）。

use crate::ledger::form::QUESTION_LABEL;
use crate::name::NAME;
use crate::pipe::declaration::QuestionRoute;
use crate::polarity::{OnFailure, Polarity, Timing};

/// 止める tool の名（完全一致だけ）。
pub const TOOL: &str = "AskUserQuestion";

/// 記録の `what`（極性一覧の語と同じ）。
pub const WHAT: &str = "choice-question-deny";

/// この境界の極性: 道具の呼び出しの時点で止め、通す値を持たない（止める側へしか倒れない）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 判定。**通す値の variant を持たない**閉じた 2 値（他の tool は「関係ない」で、後ろの門へ渡す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceQuestionDecision {
    /// 断る。中身は stderr の 1 行。
    Deny(String),
    /// この門と関係ない tool。
    Unrelated,
}

/// tool 名が [`TOOL`] と完全一致の周だけ読み手を 1 回呼んで断りの 1 行を返す。
pub fn decide(tool: &str, route: impl FnOnce() -> QuestionRoute) -> ChoiceQuestionDecision {
    if tool == TOOL {
        ChoiceQuestionDecision::Deny(deny_line(&route()))
    } else {
        ChoiceQuestionDecision::Unrelated
    }
}

/// 断りの 1 行（問いの経路の句は宣言した値・読めない旨・無しの 3 つ）。
fn deny_line(route: &QuestionRoute) -> String {
    let head = format!(
        "{NAME}: deny choice-question tool={TOOL} — 器を名乗る repo では選択式の問いの道具を使わない（FR86・vessel-hook.md §20）。\
         問いは平文の返答で問う。user の裁定が要る問いは台帳の問い（label {QUESTION_LABEL} の bead）として立てる。"
    );
    match route {
        QuestionRoute::Declared(value) => format!("{head}この repo の問いの経路: {value}"),
        QuestionRoute::Unreadable => format!("{head}vessel 宣言を読めないので、この repo の問いの経路の 1 行は告げられない"),
        QuestionRoute::Absent => head,
    }
}

#[cfg(test)]
mod tests {
    use super::{decide, ChoiceQuestionDecision, QuestionRoute, QUESTION_LABEL, TOOL};
    use crate::name::NAME;
    use std::cell::Cell;

    /// 判定と、読み手を呼んだ回数。
    fn judged(tool: &str, route: QuestionRoute) -> (ChoiceQuestionDecision, u32) {
        let calls = Cell::new(0_u32);
        let decided = decide(tool, || {
            calls.set(calls.get().saturating_add(1));
            route
        });
        (decided, calls.get())
    }

    /// (a) AskUserQuestion だけが deny で読み手を 1 回呼び、近い名と他の道具は「関係ない」で読み手を 0 回呼ぶ。
    #[test]
    fn hook_choice_question_denies_only_the_exact_tool_and_reads_the_route_once() {
        let (decided, calls) = judged(TOOL, QuestionRoute::Absent);
        assert!(matches!(decided, ChoiceQuestionDecision::Deny(_)), "{decided:?}");
        assert_eq!(calls, 1, "読み手は 1 回");
        let near = ["Bash", "Edit", "Write", "MultiEdit", "NotebookEdit", "", "askuserquestion", "AskUserQuestionX", "mcp__x__AskUserQuestion"];
        for tool in near {
            assert_eq!(judged(tool, QuestionRoute::Absent), (ChoiceQuestionDecision::Unrelated, 0), "{tool}");
        }
    }

    /// (b) 断りは改行を持たない 1 行で、名乗り・道具・平文・台帳の問いの label を持ち、経路の句は 3 値で分かれる。
    #[test]
    fn hook_choice_question_deny_line_names_the_routes() {
        let line_of = |route| match judged(TOOL, route).0 {
            ChoiceQuestionDecision::Deny(line) => line,
            ChoiceQuestionDecision::Unrelated => String::new(),
        };
        let declared = line_of(QuestionRoute::Declared("ROUTE-X".to_owned()));
        let unreadable = line_of(QuestionRoute::Unreadable);
        let absent = line_of(QuestionRoute::Absent);
        for line in [&declared, &unreadable, &absent] {
            assert!(!line.contains('\n'), "1 行: {line}");
            assert!(line.starts_with(&format!("{NAME}: deny choice-question tool={TOOL} ")), "{line}");
            assert!(line.contains("平文の返答") && line.contains(&format!("label {QUESTION_LABEL}")), "{line}");
        }
        assert!(declared.ends_with("この repo の問いの経路: ROUTE-X"), "{declared}");
        assert!(unreadable.contains("読めない") && !unreadable.contains("ROUTE-X"), "{unreadable}");
        assert!(!absent.contains("読めない") && !absent.contains("問いの経路"), "{absent}");
    }
}
