//! 台帳の問い（label [`QUESTION_LABEL`]）の create の形の判定（設計 docs/design/ledger-form.md §14・契約表の行 j・
//! FR81 (a)・FR89・ADR-0083・ADR-0087）。
//!
//! 純関数だけを置く: 本文の字・metadata の字・label の列（と、起票の門が command 行から解いた継ぐ親の id）を読み、
//! 台帳も file も読まない。読めない字（開けない file・`--stdin`）は呼び手が `None` で渡す。判定の順は
//! 併せ持ち → 継がない指定 → 本文を読めるか → 4 行 → metadata を読めるか → effect → asked で、最初の欠け 1 つを
//! 閉じた理由（[`Gap`]・12 語）で返す。JSON は [`crate::fleet::json_tree::parse`] で読む（依存を足さない・NFR3）。

use crate::fleet::json_tree::{parse, Tree};
use crate::ledger::form::{MEMO_LABEL, QUESTION_LABEL};

/// 本文の 4 行の語（宣言順＝欠けの報告の順）。
pub const LINES: [&str; 4] = ["概要", "技術", "理由", "推奨"];

/// metadata の key effect の閉じた 2 値。
pub const EFFECTS: [&str; 2] = ["document", "operation"];

/// metadata の key asked の閉じた 2 値。
pub const ASKED: [&str; 2] = ["seat", "user"];

/// metadata の key（答えを文書へ写すか）。
const EFFECT: &str = "effect";

/// metadata の key（誰のきっかけの問いか）。
const ASKED_KEY: &str = "asked";

/// 次の一手に置く metadata の字。
const METADATA_HINT: &str = "--metadata '{\"effect\":\"document\",\"asked\":\"seat\"}'";

/// 問いの create の欠け（1 周に 1 つ・宣言順で最初のもの）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gap {
    /// label intake:memo を併せ持つ。
    MemoLabel,
    /// `--parent` を名指して継がない指定（`--no-inherit-labels`）が無い（値は継ぐ親の id）。
    InheritsLabels(String),
    /// 本文を読めない（`--stdin`・値が `-` か無い `--body-file`・開けない file・`$` か backtick を含む `-d`）。
    BodyUnreadable,
    /// 本文に 4 行の 1 つが無い（[`LINES`] の添字）。
    MissingLine(usize),
    /// metadata が JSON の object でない・開けない。
    MetadataUnreadable,
    /// metadata に effect が無い。
    NoEffect,
    /// effect の値が [`EFFECTS`] の外（値は外れた字）。
    BadEffect(String),
    /// metadata に asked が無い。
    NoAsked,
    /// asked の値が [`ASKED`] の外（値は外れた字）。
    BadAsked(String),
}

impl Gap {
    /// 記録と deny 文に出す理由の 1 語。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MemoLabel => "question-memo-label",
            Self::InheritsLabels(_) => "question-inherits-labels",
            Self::BodyUnreadable => "question-body-unreadable",
            Self::MissingLine(0) => "question-no-summary",
            Self::MissingLine(1) => "question-no-technical",
            Self::MissingLine(2) => "question-no-reason",
            Self::MissingLine(_) => "question-no-recommendation",
            Self::MetadataUnreadable => "question-metadata-unreadable",
            Self::NoEffect => "question-no-effect",
            Self::BadEffect(_) => "question-bad-effect",
            Self::NoAsked => "question-no-asked",
            Self::BadAsked(_) => "question-bad-asked",
        }
    }

    /// 理由の説明と次の一手（deny 文の後半・改行を持たない）。
    pub fn guidance(&self) -> String {
        let lines = LINES.map(|word| format!("{word} = …")).join(" / ");
        match self {
            Self::MemoLabel => format!(
                "問いは label {MEMO_LABEL} を併せ持てない — label {MEMO_LABEL} を外す（memo なら label {QUESTION_LABEL} を外す）"
            ),
            Self::InheritsLabels(parent) => format!(
                "親 {} の label を継ぐ（memo の子なら {MEMO_LABEL} を併せ持つ） — --no-inherit-labels を足す",
                parent.escape_debug()
            ),
            Self::BodyUnreadable => "問いの本文を読めない（--stdin・--body-file の - か値なし・開けない file・$ か backtick を含む -d） — 本文を file に書いて --body-file で名指すか -d に字で書く".to_owned(),
            Self::MissingLine(at) => format!(
                "本文に {} の行が無い — 4 行（{lines}）を置く",
                LINES.get(*at).copied().unwrap_or_default()
            ),
            Self::MetadataUnreadable => format!("--metadata が JSON の object でない・開けない — {METADATA_HINT} で渡す"),
            Self::NoEffect => format!("metadata に {EFFECT} が無い — {METADATA_HINT} の形で {} のどちらかを置く", EFFECTS.join(" / ")),
            Self::BadEffect(value) => format!(
                "metadata の {EFFECT} が {} — {} のどちらか（{METADATA_HINT}）",
                value.escape_debug(),
                EFFECTS.join(" / ")
            ),
            Self::NoAsked => format!("metadata に {ASKED_KEY} が無い — {METADATA_HINT} の形で {} のどちらかを置く", ASKED.join(" / ")),
            Self::BadAsked(value) => format!(
                "metadata の {ASKED_KEY} が {} — {} のどちらか（{METADATA_HINT}）",
                value.escape_debug(),
                ASKED.join(" / ")
            ),
        }
    }
}

/// metadata の字の状態（`--metadata` の最後の値・`@<path>` は呼び手が読んだ字）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metadata<'a> {
    /// `--metadata` が無い（effect と asked の欠け）。
    Absent,
    /// `@<path>` の file を開けない。
    Unreadable,
    /// JSON の字。
    Text(&'a str),
}

/// 問いの create 1 つの判定（pure）。`inherits` は継ぐ親の id（継がない指定の無い周だけ `Some`）・`body` は読めない周
/// `None`。通すなら `None`。
pub fn judge(labels: &[String], inherits: Option<&str>, body: Option<&str>, metadata: Metadata<'_>) -> Option<Gap> {
    if labels.iter().any(|label| label == MEMO_LABEL) {
        return Some(Gap::MemoLabel);
    }
    if let Some(parent) = inherits {
        return Some(Gap::InheritsLabels(parent.to_owned()));
    }
    let Some(body) = body else {
        return Some(Gap::BodyUnreadable);
    };
    if let Some(at) = missing_line(body) {
        return Some(Gap::MissingLine(at));
    }
    metadata_gap(metadata)
}

/// 本文に無い 4 行の最初の添字（[`LINES`] の宣言順・全部在れば `None`）。
pub fn missing_line(body: &str) -> Option<usize> {
    LINES.iter().position(|word| !body.lines().any(|line| starts_line(line, word)))
}

/// 行が語の行か: 行頭の空白と任意の `-` を除いて語で始まり、語の直後が行末・空白・`=`・`:`・`：`。
fn starts_line(line: &str, word: &str) -> bool {
    let line = line.trim_start();
    let line = line.strip_prefix('-').map_or(line, str::trim_start);
    line.strip_prefix(word)
        .is_some_and(|rest| rest.chars().next().is_none_or(|next| next.is_whitespace() || matches!(next, '=' | ':' | '：')))
}

/// metadata の欠け（object でない → effect → asked の順・揃えば `None`）。
pub fn metadata_gap(metadata: Metadata<'_>) -> Option<Gap> {
    let text = match metadata {
        Metadata::Absent => return Some(Gap::NoEffect),
        Metadata::Unreadable => return Some(Gap::MetadataUnreadable),
        Metadata::Text(text) => text,
    };
    let Ok(tree @ Tree::Object(_)) = parse(text.trim()) else {
        return Some(Gap::MetadataUnreadable);
    };
    closed(&tree, EFFECT, &EFFECTS, Gap::NoEffect, Gap::BadEffect)
        .or_else(|| closed(&tree, ASKED_KEY, &ASKED, Gap::NoAsked, Gap::BadAsked))
}

/// object の key の値が閉じた値の列に在るか（無ければ `absent`・外れれば `bad` に外れた字）。
fn closed(tree: &Tree, key: &str, values: &[&str], absent: Gap, bad: fn(String) -> Gap) -> Option<Gap> {
    let Some(found) = tree.get(key) else {
        return Some(absent);
    };
    match found.as_str() {
        Some(value) if values.contains(&value) => None,
        Some(value) => Some(bad(value.to_owned())),
        None => Some(bad("文字列でない値".to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::{judge, metadata_gap, missing_line, Gap, Metadata};

    /// 揃った 4 行。
    const FULL: &str = "概要 = a\n技術 = b\n理由 = c\n推奨 = d\n";

    /// 揃った metadata。
    const META: &str = "{\"effect\":\"document\",\"asked\":\"seat\"}";

    /// 語の直後の字: `=`・`:`・`：`・空白・行末は当たり、`技術的` は当たらない。行頭の空白と `- ` を除く。
    #[test]
    fn ledger_question_form_line_word_boundary() {
        for body in ["概要=a\n技術: b\n理由：c\n推奨 d\n", "概要\n技術\n理由\n推奨\n", "  - 概要 = a\n- 技術 = b\n\t理由 = c\n-推奨\n"] {
            assert_eq!(missing_line(body), None, "{body}");
        }
        assert_eq!(missing_line("概要 = a\n技術的な話\n理由 = c\n推奨 = d\n"), Some(1), "技術的 は技術の行でない");
        assert_eq!(missing_line("概要 = a\n x 技術 = b\n理由 = c\n推奨 = d\n"), Some(1), "行の途中の語は数えない");
        assert_eq!(missing_line("推奨 = d\n理由 = c\n技術 = b\n概要 = a\n"), None, "順は問わない");
    }

    /// 宣言順で最初の欠けの行（空の本文は概要の欠け）。
    #[test]
    fn ledger_question_form_first_missing_line_in_declared_order() {
        for (body, at) in [("推奨 = d\n理由 = c\n", 0), ("概要 = a\n推奨 = d\n", 1), ("概要\n技術\n推奨\n", 2), ("概要\n技術\n理由\n", 3), ("", 0)] {
            assert_eq!(missing_line(body), Some(at), "{body}");
        }
    }

    /// metadata: object でない JSON・開けない → 読めない、effect が asked より先、文字列でない値と値の外、ほかの key は読まない。
    #[test]
    fn ledger_question_form_metadata_gaps_in_declared_order() {
        for text in ["[]", "\"document\"", "1", "{", "", "null"] {
            assert_eq!(metadata_gap(Metadata::Text(text)), Some(Gap::MetadataUnreadable), "{text}");
        }
        assert_eq!(metadata_gap(Metadata::Unreadable), Some(Gap::MetadataUnreadable));
        assert_eq!(metadata_gap(Metadata::Absent), Some(Gap::NoEffect));
        let other = "文字列でない値".to_owned();
        for (text, gap) in [
            ("{}", Some(Gap::NoEffect)),
            ("{\"effect\":1,\"asked\":\"x\"}", Some(Gap::BadEffect(other.clone()))),
            ("{\"effect\":\"Document\"}", Some(Gap::BadEffect("Document".to_owned()))),
            ("{\"effect\":\"operation\"}", Some(Gap::NoAsked)),
            ("{\"effect\":\"operation\",\"asked\":[\"seat\"]}", Some(Gap::BadAsked(other))),
            ("{\"asked\":\"planner\",\"effect\":\"document\",\"x\":{}}", Some(Gap::BadAsked("planner".to_owned()))),
            (META, None),
            ("{\"effect\":\"operation\",\"asked\":\"user\",\"other\":[1]}", None),
        ] {
            assert_eq!(metadata_gap(Metadata::Text(text)), gap, "{text}");
        }
    }

    /// 段の順: 併せ持ち → 継ぐ親 → 本文を読めない → 4 行 → metadata。12 語は閉じていて重ならない。
    #[test]
    fn ledger_question_form_judge_orders_the_stages() {
        let labels = |list: &[&str]| list.iter().map(|label| (*label).to_owned()).collect::<Vec<_>>();
        let question = labels(&["intake:question"]);
        let both = labels(&["intake:question", "intake:memo"]);
        assert_eq!(judge(&both, Some("s2-1"), None, Metadata::Absent), Some(Gap::MemoLabel));
        assert_eq!(judge(&question, Some("s2-1"), None, Metadata::Absent), Some(Gap::InheritsLabels("s2-1".to_owned())));
        assert_eq!(judge(&question, None, None, Metadata::Absent), Some(Gap::BodyUnreadable));
        assert_eq!(judge(&question, None, Some("概要\n"), Metadata::Unreadable), Some(Gap::MissingLine(1)));
        assert_eq!(judge(&question, None, Some(FULL), Metadata::Unreadable), Some(Gap::MetadataUnreadable));
        assert_eq!(judge(&question, None, Some(FULL), Metadata::Text(META)), None);
        let all = [
            Gap::MemoLabel,
            Gap::InheritsLabels("p".to_owned()),
            Gap::BodyUnreadable,
            Gap::MissingLine(0),
            Gap::MissingLine(1),
            Gap::MissingLine(2),
            Gap::MissingLine(3),
            Gap::MetadataUnreadable,
            Gap::NoEffect,
            Gap::BadEffect("x".to_owned()),
            Gap::NoAsked,
            Gap::BadAsked("x".to_owned()),
        ];
        let mut words: Vec<&str> = all.iter().map(Gap::as_str).collect();
        words.sort_unstable();
        words.dedup();
        assert_eq!(words.len(), 12, "閉じた 12 語");
        assert!(all.iter().all(|gap| gap.as_str().starts_with("question-") && !gap.guidance().contains('\n')));
        assert!(Gap::BadEffect("a\nb".to_owned()).guidance().lines().count() == 1, "外れた値の改行は escape する");
    }
}
