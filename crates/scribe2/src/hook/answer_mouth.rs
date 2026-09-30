//! 裁定面の答えの口（`seat ruling answer`）を席の道具の呼び出しから撃たせない 1 関数（設計 docs/design/dialogue-surface.md §11・
//! ADR-0087・FR82 / AC52）。
//!
//! 入力は Bash の command の字だけで、役割・pane・rules・台帳・event log を読まない（runner の session でも止める）。
//! 断る形は 2 つ: (a) ある segment に、引用の外の語として `seat` `ruling` `answer` がこの順で並ぶ（binary の名・変数・`cargo run --`
//! の前置きに依らない）。(b) segment の頭の語（前の `VAR=…` を除く）が shell か `eval` で、どれかの語が `seat ruling answer` を
//! 含む。止める側へ倒す（`echo seat ruling answer` のような無害な並びも止める）。

use super::command::BASH;
use super::ledger_guard::segments;
use crate::name::NAME;
use crate::polarity::{OnFailure, Polarity, Timing};

/// 記録の `what`（極性一覧の語と同じ）。
pub const WHAT: &str = "answer-mouth-deny";

/// この境界の極性: 道具の呼び出しの時点で止め、通す値を持たない（止める側へしか倒れない）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 判定。**通す値の variant を持たない**閉じた 2 値（他の command は「関係ない」で、後ろの門へ渡す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerMouthDecision {
    /// 断る。中身は stderr の 1 行。
    Deny(String),
    /// この門と関係ない command。
    Unrelated,
}

/// 並びの 3 語（この順）。
const MOUTH: [&str; 3] = ["seat", "ruling", "answer"];

/// 語の前後から外す括りの字（`$(… answer)` や backtick の中の並びも直の並びに数える）。
const BRACKETS: [char; 5] = ['(', ')', '`', '{', '}'];

/// 並びを語に持つ連続した 3 語の字面（shell の語の中に在るか調べる）。
const MOUTH_TEXT: &str = "seat ruling answer";

/// 頭の語が shell か eval のとき、語の中に並びを探す対象（閉じた 5 語）。
const WRAPPERS: [&str; 5] = ["sh", "bash", "zsh", "dash", "eval"];

/// `VAR=…` の代入の語か（`=` の前が識別子）。
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.chars().next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && name.chars().all(|found| found.is_ascii_alphanumeric() || found == '_')
    })
}

/// 語の終わりの path の最後の要素（`/bin/bash` → `bash`）。
fn base_name(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// 1 segment が答えの口を撃つ形か。
fn fires(segment: &[String]) -> bool {
    let direct = segment.windows(MOUTH.len()).any(|window| window.iter().map(|word| word.trim_matches(BRACKETS)).eq(MOUTH));
    let head = segment.iter().find(|word| !is_assignment(word));
    let wrapped = head.is_some_and(|word| WRAPPERS.contains(&base_name(word)))
        && segment.iter().any(|word| word.contains(MOUTH_TEXT));
    direct || wrapped
}

/// Bash の周だけ command 行を読んで判定する（他の tool では読み手を呼ばない）。
pub fn decide(tool: &str, command: impl FnOnce() -> Option<String>) -> AnswerMouthDecision {
    if tool == BASH && segments(&command().unwrap_or_default()).iter().any(|segment| fires(segment)) {
        AnswerMouthDecision::Deny(deny_line())
    } else {
        AnswerMouthDecision::Unrelated
    }
}

/// 断りの 1 行（答えは結びの口で問いへ結ぶ）。
fn deny_line() -> String {
    format!(
        "{NAME}: deny answer-mouth seat ruling answer は user の画面（裁定面）だけが撃つ口で、席の道具の呼び出しからは撃てない（FR82・dialogue-surface.md §11）。\
         user の答えは記帳された発話の ts を指して `{NAME} seat ruling bind` で問いへ結ぶ。"
    )
}

#[cfg(test)]
mod tests {
    use super::{decide, deny_line, AnswerMouthDecision, BASH};

    /// Bash の command 1 行の判定。
    fn bash(command: &str) -> AnswerMouthDecision {
        decide(BASH, || Some(command.to_owned()))
    }

    /// 止まる形（直の並び・変数の binary・連鎖・shell の -c・eval・前の代入・cargo run の前置き）。
    #[test]
    fn hook_answer_mouth_denies_the_eight_forms() {
        let forms = [
            "scribe2 seat ruling answer --repo . --state-dir s --question q-1",
            "$BIN seat ruling answer --question q-1",
            "cd /tmp && scribe2 seat ruling answer --question q-1",
            "sh -c 'scribe2 seat ruling answer --question q-1'",
            "bash -lc \"scribe2 seat ruling answer --question q-1\"",
            "eval \"scribe2 seat ruling answer --question q-1\"",
            "X=1 bash -c 'scribe2 seat ruling answer --question q-1'",
            "cargo run -- seat ruling answer --question q-1",
        ];
        for form in forms {
            assert!(matches!(bash(form), AnswerMouthDecision::Deny(_)), "{form}");
        }
        assert!(matches!(bash("echo $(scribe2 seat ruling answer)"), AnswerMouthDecision::Deny(_)), "命令置換の中");
    }

    /// 通る形（引用の中の字面を読む grep・bind・ls）と、Bash でない tool（command を読まない）。
    #[test]
    fn hook_answer_mouth_passes_the_three_forms() {
        for form in ["grep -rn \"seat ruling answer\" docs", "scribe2 seat ruling bind --question q-1", "scribe2 seat ruling ls --state-dir s", ""] {
            assert_eq!(bash(form), AnswerMouthDecision::Unrelated, "{form}");
        }
        let other = decide("Edit", || Some("scribe2 seat ruling answer".to_owned()));
        assert_eq!(other, AnswerMouthDecision::Unrelated, "Bash でない tool は読まない");
    }

    /// 断りの 1 行は改行を持たず、結びの口を告げる。
    #[test]
    fn hook_answer_mouth_deny_line_points_at_bind() {
        let line = deny_line();
        assert!(!line.contains('\n') && line.contains("seat ruling bind"), "{line}");
    }
}
