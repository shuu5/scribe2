//! memo の notes の昇格の行の読み手（設計 docs/design/case-lifecycle.md §6・FR91・ADR-0089）。
//!
//! 純関数 1 本（[`read`]）だけを置く: 入力は notes の字と台帳の接頭辞で、I/O も時計も持たない。notes の行頭（空白を除く）
//! `昇格:` の行を出てきた順に読み、**最後の行**の読みを返す（前の行へは倒れない）。形は `昇格: 全部 <契約 id の列>` と
//! `昇格: 一部 <契約 id の列>`。id は空白で割り（`,` は区切りでない）、1 本以上で、同じ台帳の bead id の形。局面の関数と
//! close の理由の照合（promoted-list-mismatch）が同じ読み手を引く。

use crate::ledger::form::is_bead_id;

/// 昇格の行の頭（ASCII の colon・行頭に在る行だけが昇格の行）。
pub const HEAD: &str = "昇格:";

/// 昇格の範囲の語（全部の引き金が満ちて全部を昇格 / 一部だけを昇格）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `全部`。
    All,
    /// `一部`。
    Part,
}

impl Scope {
    /// 行の 1 語目の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Part => "一部",
        }
    }
}

/// 読めた昇格の行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Promotion {
    /// 範囲。
    pub scope: Scope,
    /// 契約 id の列（行の順・1 本以上）。
    pub ids: Vec<String>,
}

/// 昇格の行を読めない理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// 語が足りない（範囲の語と id が 2 語以上無い）。
    Words,
    /// 1 語目が `全部` / `一部` の外。
    Scope,
    /// id が同じ台帳の bead id の形でない。
    Id,
}

impl Reason {
    /// 理由の語（改行を持たない）。
    pub fn word(self) -> &'static str {
        match self {
            Self::Words => "words",
            Self::Scope => "scope",
            Self::Id => "id",
        }
    }
}

/// 昇格の行 1 本の読み。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// 読めた行。
    Readable(Promotion),
    /// 読めない行（行の字は行頭の空白を除いた字）。
    Unreadable {
        /// 行の字。
        text: String,
        /// 理由。
        reason: Reason,
    },
}

/// notes の最後の昇格の行を読む（**純関数**）。昇格の行が 1 本も無ければ `None`。`prefix` は台帳の接頭辞。
pub fn read(notes: &str, prefix: &str) -> Option<Line> {
    notes
        .lines()
        .map(str::trim_start)
        .rfind(|line| line.starts_with(HEAD))
        .map(|line| line_of(line, line.strip_prefix(HEAD).unwrap_or_default(), prefix))
}

/// 昇格の行 1 本を読む（頭の後ろを空白で割る）。
fn line_of(text: &str, rest: &str, prefix: &str) -> Line {
    let words: Vec<&str> = rest.split_whitespace().collect();
    let found = match words.as_slice() {
        [] | [_] => Err(Reason::Words),
        [scope, ids @ ..] => [Scope::All, Scope::Part]
            .into_iter()
            .find(|found| found.as_str() == *scope)
            .ok_or(Reason::Scope)
            .and_then(|scope| {
                ids.iter()
                    .all(|id| is_bead_id(id, prefix))
                    .then(|| Promotion { scope, ids: ids.iter().map(|id| (*id).to_owned()).collect() })
                    .ok_or(Reason::Id)
            }),
    };
    found.map_or_else(|reason| Line::Unreadable { text: text.to_owned(), reason }, Line::Readable)
}

#[cfg(test)]
mod tests {
    use super::{read, Line, Promotion, Reason, Scope};

    /// 読めた行（範囲と id の列）。
    fn readable(scope: Scope, ids: &[&str]) -> Option<Line> {
        Some(Line::Readable(Promotion { scope, ids: ids.iter().map(|id| (*id).to_owned()).collect() }))
    }

    /// 読めない行（行の字と理由）。
    fn unreadable(text: &str, reason: Reason) -> Option<Line> {
        Some(Line::Unreadable { text: text.to_owned(), reason })
    }

    /// `全部` と `一部` の 2 形を読む（id は空白で割った列・行の順）。
    #[test]
    fn promotion_line_reads_all_and_part() {
        assert_eq!(read("昇格: 全部 s2-a s2-b.1\n", "s2"), readable(Scope::All, &["s2-a", "s2-b.1"]));
        assert_eq!(read("昇格: 一部 s2-c\n", "s2"), readable(Scope::Part, &["s2-c"]));
        assert_eq!(read("", "s2"), None, "昇格の行が無い notes");
    }

    /// 2 行在れば最後の行が勝つ。
    #[test]
    fn promotion_line_the_last_line_wins() {
        assert_eq!(read("昇格: 一部 s2-a\n別の行\n昇格: 全部 s2-a s2-b\n", "s2"), readable(Scope::All, &["s2-a", "s2-b"]));
        assert_eq!(read("昇格: 全部 s2-a s2-b\n昇格: 一部 s2-a\n", "s2"), readable(Scope::Part, &["s2-a"]));
    }

    /// 最後の行が読めず前の行が読める notes は読めない（前の行へ倒れない）。
    #[test]
    fn promotion_line_unreadable_last_line_does_not_fall_back() {
        assert_eq!(read("昇格: 全部 s2-a\n昇格: 全部 x-1\n", "s2"), unreadable("昇格: 全部 x-1", Reason::Id));
        assert_eq!(read("昇格: 全部 x-1\n昇格: 全部 s2-a\n", "s2"), readable(Scope::All, &["s2-a"]), "逆順は最後が読める");
    }

    /// 読めない 3 形（語の欠け・全部 / 一部の外・id の形）のそれぞれの理由の語と行の字。
    #[test]
    fn promotion_line_each_unreadable_form_carries_its_reason() {
        assert_eq!(read("  昇格: 全部\n", "s2"), unreadable("昇格: 全部", Reason::Words));
        assert_eq!(read("昇格:\n", "s2"), unreadable("昇格:", Reason::Words));
        assert_eq!(read("昇格: 半分 s2-a\n", "s2"), unreadable("昇格: 半分 s2-a", Reason::Scope));
        assert_eq!(read("昇格: 一部 s2-a s3-b\n", "s2"), unreadable("昇格: 一部 s2-a s3-b", Reason::Id));
        assert_eq!(read("昇格: 全部 s2-\n", "s2"), unreadable("昇格: 全部 s2-", Reason::Id));
        let words: Vec<&str> = [Reason::Words, Reason::Scope, Reason::Id].map(Reason::word).to_vec();
        assert_eq!(words, ["words", "scope", "id"]);
    }

    /// 行頭でない `昇格:`（行の途中・全角の colon・箇条の印つき）は昇格の行でなく読まない。
    #[test]
    fn promotion_line_mid_line_head_is_not_a_promotion_line() {
        assert_eq!(read("メモ 昇格: 全部 s2-a\n", "s2"), None);
        assert_eq!(read("昇格： 全部 s2-a\n", "s2"), None);
        assert_eq!(read("- 昇格: 全部 s2-a\n", "s2"), None);
        assert_eq!(read("メモ 昇格: 全部 s2-a\n昇格: 一部 s2-b\n", "s2"), readable(Scope::Part, &["s2-b"]), "行頭の行だけを読む");
    }

    /// `,` は区切りでない（1 語の中の `,` は id の形に合わず読めない）。
    #[test]
    fn promotion_line_comma_is_not_a_separator() {
        assert_eq!(read("昇格: 全部 s2-a,s2-b\n", "s2"), unreadable("昇格: 全部 s2-a,s2-b", Reason::Id));
        assert_eq!(read("昇格: 全部 s2-a, s2-b\n", "s2"), unreadable("昇格: 全部 s2-a, s2-b", Reason::Id));
        assert_eq!(read("昇格: 全部 s2-a s2-b\n", "s2"), readable(Scope::All, &["s2-a", "s2-b"]));
    }

    /// 範囲の語の字面（`全部` / `一部`）。
    #[test]
    fn promotion_line_scope_words() {
        assert_eq!([Scope::All, Scope::Part].map(Scope::as_str), ["全部", "一部"]);
        assert_eq!(read("昇格: 全部\u{3000}s2-a\n", "s2"), readable(Scope::All, &["s2-a"]), "Unicode の空白で割る");
    }
}
