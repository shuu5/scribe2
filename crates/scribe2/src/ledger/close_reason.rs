//! close の理由の読み手（設計 docs/design/ledger-form.md §16・契約表の行 l・FR81 (d)・FR91・ADR-0089・ADR-0094・ADR-0097）。
//!
//! 純関数 1 本（[`read`]）だけを置く: 入力は理由の字と台帳の接頭辞（解けない周は `None`）で、I/O も時計も持たない。前後の空白を
//! 除き、Unicode の空白（全角の空白を含む）で割った 1 語目を 9 つの頭（[`Head`]）と完全一致で照合し（大文字小文字を区別する）、
//! 頭ごとの値の形で読んで、形（[`Form`]）か閉じた欠陥（[`Defect`]）を返す。門の判定・種類と形の食い違い・閉じた時点の条件は
//! 持たない。局面の関数も同じ関数で読む（自前の読みを持たない・C2）。bead id は [`is_bead_id`] 1 本、裁定 id の時刻は
//! [`epoch_of`] に秒 `:00` を足した字で読み、裁定 id の形の判定は [`is_ruling_id`] が外へ見せる。

use crate::fleet::epoch_of;
use crate::ledger::form::is_bead_id;

/// 着地 commit id の桁数（40 桁の 16 進）。
const COMMIT_LEN: usize = 40;

/// 尾の語 `ci=success`。
const CI_SUCCESS: &str = "ci=success";

/// 尾の語 `ci=none`。
const CI_NONE: &str = "ci=none";

/// 尾の先端の語の頭 `tip=`。
const TIP_KEY: &str = "tip=";

/// 裁定 id の頭 `batch:`。
const RULING_BATCH: &str = "batch:";

/// 裁定 id の頭 `policy:`。
const RULING_POLICY: &str = "policy:";

/// 理由の頭の 9 語（宣言順）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Head {
    /// `landed <着地 commit id> <尾>`。
    Landed,
    /// `重複 <bead id>`。
    Duplicate,
    /// `後継 <bead id>`。
    Successor,
    /// `取り下げ <理由>`。
    Withdrawn,
    /// `裁定 <裁定 id>`。
    Ruling,
    /// `昇格済み <契約 id の列>`。
    Promoted,
    /// `まとめた <memo id>`。
    Merged,
    /// `見送り <裁定 id>`。
    Deferred,
    /// `完了`。
    Done,
}

impl Head {
    /// 9 語（宣言順）。
    pub const ALL: [Self; 9] = [
        Self::Landed,
        Self::Duplicate,
        Self::Successor,
        Self::Withdrawn,
        Self::Ruling,
        Self::Promoted,
        Self::Merged,
        Self::Deferred,
        Self::Done,
    ];

    /// 理由の 1 語目の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Landed => "landed",
            Self::Duplicate => "重複",
            Self::Successor => "後継",
            Self::Withdrawn => "取り下げ",
            Self::Ruling => "裁定",
            Self::Promoted => "昇格済み",
            Self::Merged => "まとめた",
            Self::Deferred => "見送り",
            Self::Done => "完了",
        }
    }
}

/// landed の尾（閉じた 3 形か読めない尾）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandedTail {
    /// `ci=success`。
    Success,
    /// `ci=success tip=<40 桁の 16 進>`（先端の commit id を運ぶ）。
    SuccessTip(String),
    /// `ci=none`。
    None,
    /// 3 形のどれでもない尾（空白を 1 つに揃えた字・尾が無いなら空）。
    Unreadable(String),
}

/// 読めた理由の形（頭と値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Form {
    /// 着地（着地 commit id と尾）。
    Landed {
        /// 40 桁の 16 進の着地 commit id（書かれた字のまま）。
        commit: String,
        /// 尾。
        tail: LandedTail,
    },
    /// 重複（同じ台帳の bead id）。
    Duplicate(String),
    /// 後継（同じ台帳の bead id）。
    Successor(String),
    /// 取り下げ（頭の後ろの字の全部・前後の空白を除く）。
    Withdrawn(String),
    /// 裁定（裁定 id）。
    Ruling(String),
    /// 昇格済み（同じ台帳の bead id の列・1 つ以上）。
    Promoted(Vec<String>),
    /// まとめた（同じ台帳の bead id）。
    Merged(String),
    /// 見送り（裁定 id）。
    Deferred(String),
    /// 完了（値なし）。
    Done,
}

impl Form {
    /// 頭。
    pub fn head(&self) -> Head {
        match self {
            Self::Landed { .. } => Head::Landed,
            Self::Duplicate(_) => Head::Duplicate,
            Self::Successor(_) => Head::Successor,
            Self::Withdrawn(_) => Head::Withdrawn,
            Self::Ruling(_) => Head::Ruling,
            Self::Promoted(_) => Head::Promoted,
            Self::Merged(_) => Head::Merged,
            Self::Deferred(_) => Head::Deferred,
            Self::Done => Head::Done,
        }
    }
}

/// 理由を読めない欠陥（閉じた 4 つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defect {
    /// 空（空白だけを含む）。
    Empty,
    /// 1 語目が 9 つの頭の外。
    UnknownHead,
    /// 値が頭の値の形に合わない（値の欠けを含む）。
    Value(Head),
    /// 値を判じる台帳の接頭辞が解けず、bead id を読めない。
    NoPrefix,
}

/// close の理由を読む（**純関数**・§16 の文法）。`prefix` は台帳の接頭辞（解けない周は `None`＝bead id を値に持つ形が読めない）。
pub fn read(reason: &str, prefix: Option<&str>) -> Result<Form, Defect> {
    let text = reason.trim();
    let word = text.split_whitespace().next().ok_or(Defect::Empty)?;
    let head = Head::ALL.into_iter().find(|found| found.as_str() == word).ok_or(Defect::UnknownHead)?;
    let rest = text.get(word.len()..).unwrap_or_default().trim();
    let values: Vec<&str> = rest.split_whitespace().collect();
    let bad = Defect::Value(head);
    match head {
        Head::Landed => landed(&values).ok_or(bad),
        Head::Duplicate => one_bead(&values, prefix, head).map(Form::Duplicate),
        Head::Successor => one_bead(&values, prefix, head).map(Form::Successor),
        Head::Merged => one_bead(&values, prefix, head).map(Form::Merged),
        Head::Withdrawn => (!rest.is_empty()).then(|| Form::Withdrawn(rest.to_owned())).ok_or(bad),
        Head::Ruling => one_ruling(&values, prefix, head).map(Form::Ruling),
        Head::Deferred => one_ruling(&values, prefix, head).map(Form::Deferred),
        Head::Promoted => promoted(&values, prefix),
        Head::Done => Ok(Form::Done),
    }
}

/// 裁定 id の形か（閉じた 3 形・§16）: `<問い id>:<YYYYMMDDTHHMMZ>-<n>`（問い id は同じ台帳の bead id・時刻は [`epoch_of`] が
/// 読める範囲・n は ASCII の数字だけの 1 以上の整数）・`batch:<字>`・`policy:<字>`（字は 1 字以上）。接頭辞が解けない周
/// （`None`）は問い id の形を読めず、`batch:` と `policy:` の形だけが真になる。
pub fn is_ruling_id(text: &str, prefix: Option<&str>) -> bool {
    if let Some(rest) = text.strip_prefix(RULING_BATCH).or_else(|| text.strip_prefix(RULING_POLICY)) {
        return !rest.is_empty();
    }
    prefix.is_some_and(|prefix| question_form(text, prefix))
}

/// 裁定の行（bead の notes の 1 行）の 5 欄（設計 dispatcher.md §36 約束 4・fleet-event-log.md 行 h が書く行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulingRow {
    /// 裁定 id（先頭の欄）。
    pub id: String,
    /// 問い id。
    pub question: String,
    /// 発話の ts。
    pub ts: String,
    /// 経路（4 欄の古い行は [`ROUTE_CHAT`]）。
    pub route: String,
    /// 逐語。
    pub verbatim: String,
}

/// 4 欄の古い行が持つと読む経路。
pub const ROUTE_CHAT: &str = "chat";

/// notes の 1 行を裁定の行として読む（`|` で割り、欄ごとに前後の空白を剥ぐ）。先頭の欄が [`is_ruling_id`] で真なら 5 欄
/// （4 欄の古い行は経路を [`ROUTE_CHAT`] と読む）を返し、ほかの行（先頭の欄が裁定 id でない・欄が 4 でも 5 でもない）は
/// `None`。`prefix` は台帳の接頭辞（解けない周は `None`＝問い id の形の裁定 id は読めない）。
pub fn ruling_row(line: &str, prefix: Option<&str>) -> Option<RulingRow> {
    let fields: Vec<&str> = line.split('|').map(str::trim).collect();
    let own = |text: &str| text.to_owned();
    let (id, question, ts, route, verbatim) = match fields.as_slice() {
        [id, question, ts, route, verbatim] => (id, question, ts, *route, verbatim),
        [id, question, ts, verbatim] => (id, question, ts, ROUTE_CHAT, verbatim),
        _ => return None,
    };
    is_ruling_id(id, prefix).then(|| RulingRow { id: own(id), question: own(question), ts: own(ts), route: own(route), verbatim: own(verbatim) })
}

/// 着地の形 `<40 桁の 16 進> <尾>` を読む（値の語が無いか id が 40 桁の 16 進でなければ `None`）。
fn landed(values: &[&str]) -> Option<Form> {
    let (commit, tail) = values.split_first()?;
    is_commit_id(commit).then(|| Form::Landed { commit: (*commit).to_owned(), tail: tail_of(tail) })
}

/// 尾の閉じた 3 形（ほかは読めない尾）。
fn tail_of(words: &[&str]) -> LandedTail {
    match words {
        [CI_SUCCESS] => LandedTail::Success,
        [CI_NONE] => LandedTail::None,
        [CI_SUCCESS, tip] => match tip.strip_prefix(TIP_KEY).filter(|id| is_commit_id(id)) {
            Some(id) => LandedTail::SuccessTip(id.to_owned()),
            None => LandedTail::Unreadable(words.join(" ")),
        },
        _ => LandedTail::Unreadable(words.join(" ")),
    }
}

/// 40 桁の 16 進か（大文字小文字を問わない）。
fn is_commit_id(text: &str) -> bool {
    text.len() == COMMIT_LEN && text.bytes().all(|found| found.is_ascii_hexdigit())
}

/// 2 語目が同じ台帳の bead id の形（値の語が無い周は値の形の外・接頭辞が解けない周は `NoPrefix`）。
fn one_bead(values: &[&str], prefix: Option<&str>, head: Head) -> Result<String, Defect> {
    let value = values.first().ok_or(Defect::Value(head))?;
    let prefix = prefix.ok_or(Defect::NoPrefix)?;
    if is_bead_id(value, prefix) {
        Ok((*value).to_owned())
    } else {
        Err(Defect::Value(head))
    }
}

/// 2 語目が裁定 id の形（値の語が無い周は値の形の外・問い id の形を判じる接頭辞が解けない周は `NoPrefix`）。
fn one_ruling(values: &[&str], prefix: Option<&str>, head: Head) -> Result<String, Defect> {
    let value = values.first().ok_or(Defect::Value(head))?;
    if is_ruling_id(value, prefix) {
        Ok((*value).to_owned())
    } else if prefix.is_none() {
        Err(Defect::NoPrefix)
    } else {
        Err(Defect::Value(head))
    }
}

/// 頭の後ろの語の全部が同じ台帳の bead id（1 つ以上・`,` は区切りでない）。
fn promoted(values: &[&str], prefix: Option<&str>) -> Result<Form, Defect> {
    if values.is_empty() {
        return Err(Defect::Value(Head::Promoted));
    }
    let prefix = prefix.ok_or(Defect::NoPrefix)?;
    if values.iter().all(|value| is_bead_id(value, prefix)) {
        Ok(Form::Promoted(values.iter().map(|value| (*value).to_owned()).collect()))
    } else {
        Err(Defect::Value(Head::Promoted))
    }
}

/// 問い id の形の裁定 id（`<bead id>:<YYYYMMDDTHHMMZ>-<n>`）か。
fn question_form(text: &str, prefix: &str) -> bool {
    let Some((id, rest)) = text.split_once(':') else {
        return false;
    };
    let Some((time, count)) = rest.split_once('-') else {
        return false;
    };
    is_bead_id(id, prefix) && is_minute(time) && is_count(count)
}

/// `YYYYMMDDTHHMMZ` で、秒 `:00` を足した字を [`epoch_of`] が読めるか。
fn is_minute(time: &str) -> bool {
    let Some((date, minute)) = time.strip_suffix('Z').and_then(|stem| stem.split_once('T')) else {
        return false;
    };
    let (Some(year), Some(month), Some(day), Some(hour), Some(min)) =
        (date.get(..4), date.get(4..6), date.get(6..8), minute.get(..2), minute.get(2..4))
    else {
        return false;
    };
    date.len() == 8 && minute.len() == 4 && epoch_of(&format!("{year}-{month}-{day}T{hour}:{min}:00Z")).is_some()
}

/// ASCII の数字だけの 1 以上の整数か。
fn is_count(text: &str) -> bool {
    text.bytes().all(|found| found.is_ascii_digit()) && text.parse::<u64>().is_ok_and(|count| count >= 1)
}

#[cfg(test)]
mod tests {
    use super::{is_ruling_id, read, ruling_row, Defect, Form, Head, LandedTail, RulingRow, ROUTE_CHAT};

    /// 40 桁の 16 進（小文字）。
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    /// 40 桁の 16 進（大文字）。
    const UPPER: &str = "0123456789ABCDEF0123456789ABCDEF01234567";

    /// 台帳の接頭辞つきで読む。
    fn ok(reason: &str) -> Result<Form, Defect> {
        read(reason, Some("s2"))
    }

    /// 9 つの頭 × 読める値。
    #[test]
    fn ledger_close_reason_reads_each_head_with_a_readable_value() {
        let landed = |tail| Form::Landed { commit: SHA.to_owned(), tail };
        for (reason, want) in [
            (format!("landed {SHA} ci=success"), landed(LandedTail::Success)),
            ("重複 s2-07l.738".to_owned(), Form::Duplicate("s2-07l.738".to_owned())),
            ("後継 s2-9".to_owned(), Form::Successor("s2-9".to_owned())),
            ("取り下げ 不要になった".to_owned(), Form::Withdrawn("不要になった".to_owned())),
            ("裁定 batch:x".to_owned(), Form::Ruling("batch:x".to_owned())),
            ("昇格済み s2-1 s2-2.3".to_owned(), Form::Promoted(vec!["s2-1".to_owned(), "s2-2.3".to_owned()])),
            ("まとめた s2-5".to_owned(), Form::Merged("s2-5".to_owned())),
            ("見送り policy:p".to_owned(), Form::Deferred("policy:p".to_owned())),
            ("完了".to_owned(), Form::Done),
        ] {
            let found = ok(&reason);
            assert_eq!(found.as_ref(), Ok(&want), "{reason}");
            assert_eq!(reason.split_whitespace().next(), found.ok().map(|form| form.head().as_str()), "{reason}: 頭の字面");
        }
        assert_eq!(Head::ALL.map(Head::as_str), ["landed", "重複", "後継", "取り下げ", "裁定", "昇格済み", "まとめた", "見送り", "完了"]);
    }

    /// 9 つの頭 × 読めない値（値の欠け・別の台帳・語尾の記号・空の値）。
    #[test]
    fn ledger_close_reason_refuses_each_head_with_an_unreadable_value() {
        for (reason, head) in [
            ("landed".to_owned(), Head::Landed),
            ("landed abc ci=success".to_owned(), Head::Landed),
            (format!("landed {SHA}0 ci=success"), Head::Landed),
            (format!("landed {} ci=success", SHA.trim_end_matches('7')), Head::Landed),
            (format!("landed {} ci=success", SHA.replace('7', "g")), Head::Landed),
            ("重複".to_owned(), Head::Duplicate),
            ("重複 tz-1".to_owned(), Head::Duplicate),
            ("重複 s2-1、".to_owned(), Head::Duplicate),
            ("後継 s2".to_owned(), Head::Successor),
            ("取り下げ".to_owned(), Head::Withdrawn),
            ("取り下げ \u{3000} ".to_owned(), Head::Withdrawn),
            ("裁定".to_owned(), Head::Ruling),
            ("裁定 batch:".to_owned(), Head::Ruling),
            ("裁定 s2-1".to_owned(), Head::Ruling),
            ("昇格済み".to_owned(), Head::Promoted),
            ("昇格済み s2-1, s2-2".to_owned(), Head::Promoted),
            ("昇格済み s2-1 tz-2".to_owned(), Head::Promoted),
            ("まとめた".to_owned(), Head::Merged),
            ("まとめた s2-1.".to_owned(), Head::Merged),
            ("見送り".to_owned(), Head::Deferred),
            ("見送り policy:".to_owned(), Head::Deferred),
        ] {
            assert_eq!(ok(&reason), Err(Defect::Value(head)), "{reason}");
        }
    }

    /// 空・空白だけ・頭の外（大文字の Landed・末尾が `:` の昇格済み・頭に字が続く完了・別の頭）は閉じた欠陥。
    #[test]
    fn ledger_close_reason_refuses_empty_and_heads_outside_the_nine() {
        for reason in ["", "   ", "\u{3000}\t", "\n"] {
            assert_eq!(ok(reason), Err(Defect::Empty), "{reason:?}");
        }
        for reason in [
            format!("Landed {SHA} ci=success"),
            "昇格済み: s2-1".to_owned(),
            "完了した".to_owned(),
            "done".to_owned(),
            "fixed s2-1".to_owned(),
            "重複s2-1".to_owned(),
        ] {
            assert_eq!(ok(&reason), Err(Defect::UnknownHead), "{reason}");
        }
    }

    /// 前後の空白は除き、全角の空白と tab で割り、値の後ろの語は読まない（取り下げ・昇格済み・着地の尾を除く）。
    #[test]
    fn ledger_close_reason_splits_on_unicode_whitespace_and_ignores_trailing_words() {
        assert_eq!(ok("  重複\u{3000}s2-1  "), Ok(Form::Duplicate("s2-1".to_owned())));
        assert_eq!(ok("重複\ts2-1 と同じ"), Ok(Form::Duplicate("s2-1".to_owned())));
        assert_eq!(ok("完了 後ろの字は読まない"), Ok(Form::Done));
        assert_eq!(ok("取り下げ\u{3000}前提が  変わった "), Ok(Form::Withdrawn("前提が  変わった".to_owned())));
        assert_eq!(ok("昇格済み\u{3000}s2-1\u{3000}s2-2"), Ok(Form::Promoted(vec!["s2-1".to_owned(), "s2-2".to_owned()])));
    }

    /// landed の尾の閉じた 3 形と読めない尾（着地の形かは頭と id で決まる）・大文字の 16 進の id。
    #[test]
    fn ledger_close_reason_reads_the_landed_tail_forms() {
        let landed = |commit: &str, tail| Ok(Form::Landed { commit: commit.to_owned(), tail });
        assert_eq!(ok(&format!("landed {SHA} ci=success")), landed(SHA, LandedTail::Success));
        assert_eq!(ok(&format!("landed {SHA} ci=success tip={UPPER}")), landed(SHA, LandedTail::SuccessTip(UPPER.to_owned())));
        assert_eq!(ok(&format!("landed {SHA} ci=none")), landed(SHA, LandedTail::None));
        assert_eq!(ok(&format!("landed {UPPER} ci=none")), landed(UPPER, LandedTail::None));
        for (tail, want) in [
            ("", ""),
            ("ci=failure", "ci=failure"),
            ("ci=none tip=abc", "ci=none tip=abc"),
            ("ci=success tip=abc", "ci=success tip=abc"),
            ("ci=success  extra", "ci=success extra"),
            ("tip=0123456789abcdef0123456789abcdef01234567 ci=success", "tip=0123456789abcdef0123456789abcdef01234567 ci=success"),
        ] {
            let reason = format!("landed {SHA} {tail}");
            assert_eq!(ok(&reason), landed(SHA, LandedTail::Unreadable(want.to_owned())), "{reason}");
        }
    }

    /// 接頭辞が解けない周は bead id を値に持つ形（重複・後継・まとめた・昇格済み・問い id の裁定 id）が読めず、
    /// 値の要らない形と batch: / policy: の裁定と着地は読める。
    #[test]
    fn ledger_close_reason_without_a_prefix_cannot_read_bead_ids() {
        let none = |reason: &str| read(reason, None);
        for reason in ["重複 s2-1", "後継 s2-1", "まとめた s2-1", "昇格済み s2-1", "裁定 s2-1:20260928T1347Z-1", "見送り s2-1:20260928T1347Z-1"] {
            assert_eq!(none(reason), Err(Defect::NoPrefix), "{reason}");
        }
        assert_eq!(none("完了"), Ok(Form::Done));
        assert_eq!(none("取り下げ x"), Ok(Form::Withdrawn("x".to_owned())));
        assert_eq!(none("裁定 batch:x"), Ok(Form::Ruling("batch:x".to_owned())));
        assert_eq!(none("見送り policy:x"), Ok(Form::Deferred("policy:x".to_owned())));
        assert!(matches!(none(&format!("landed {SHA} ci=none")), Ok(Form::Landed { .. })));
        assert_eq!(none("重複"), Err(Defect::Value(Head::Duplicate)), "値の欠けは接頭辞より先");
    }

    /// 裁定 id の 3 形と、n が 0・月 13・日 0・時 24・分 60・桁の欠け・別の台帳・区切りの違い・空の字。
    #[test]
    fn ledger_close_reason_ruling_id_has_three_closed_forms() {
        let yes = |text: &str| is_ruling_id(text, Some("s2"));
        for text in [
            "s2-07l.739.1:20260928T1347Z-1",
            "s2-1:20260928T0000Z-12",
            "s2-1:20261231T2359Z-007",
            "batch:x",
            "batch:2026-09-28 の束",
            "policy:p",
        ] {
            assert!(yes(text), "{text}");
        }
        for text in [
            "s2-1:20260928T1347Z-0",
            "s2-1:20260928T1347Z-",
            "s2-1:20260928T1347Z-a",
            "s2-1:20260928T1347Z-+1",
            "s2-1:20261328T1347Z-1",
            "s2-1:20260900T1347Z-1",
            "s2-1:20260928T2447Z-1",
            "s2-1:20260928T1360Z-1",
            "s2-1:20260928T134Z-1",
            "s2-1:2026092T1347Z-1",
            "s2-1:20260928T13477Z-1",
            "s2-1:20260928t1347Z-1",
            "s2-1:20260928T1347-1",
            "s2-1:20260928T1347Z_1",
            "s2-1:２０２６0928T1347Z-1",
            "tz-1:20260928T1347Z-1",
            "s2-1",
            "batch:",
            "policy:",
            "Batch:x",
            "",
        ] {
            assert!(!yes(text), "{text}");
        }
        assert!(!is_ruling_id("s2-1:20260928T1347Z-1", None), "接頭辞が無ければ問い id の形は読めない");
        assert!(is_ruling_id("batch:x", None) && is_ruling_id("policy:x", None));
    }

    /// 裁定の行の 5 欄と期待の欄の組（裁定 id・問い id・発話の ts・経路・逐語）。
    fn row(id: &str, question: &str, ts: &str, route: &str, verbatim: &str) -> Option<RulingRow> {
        let own = |text: &str| text.to_owned();
        Some(RulingRow { id: own(id), question: own(question), ts: own(ts), route: own(route), verbatim: own(verbatim) })
    }

    /// 5 欄の行は 5 欄をそのまま返し、4 欄の古い行は経路を chat と読む（発話の ts と逐語は 4 欄の位置のまま）。
    #[test]
    fn ruling_row_reads_five_fields_and_four_fields_as_chat() {
        let five = "s2-1:20260930T0000Z-1 | s2-1 | 2026-09-30T00:00Z | chat | 逐語の字";
        assert_eq!(ruling_row(five, Some("s2")), row("s2-1:20260930T0000Z-1", "s2-1", "2026-09-30T00:00Z", "chat", "逐語の字"));
        let route = "s2-1:20260930T0000Z-1 | s2-1 | 2026-09-30T00:00Z | seat | 逐語";
        assert_eq!(ruling_row(route, Some("s2")).map(|found| found.route), Some("seat".to_owned()), "5 欄の経路は書かれたまま");
        let four = "batch:m2 | s2-1 | 2026-09-30T00:00Z | 逐語";
        assert_eq!(ruling_row(four, Some("s2")), row("batch:m2", "s2-1", "2026-09-30T00:00Z", ROUTE_CHAT, "逐語"));
        assert_eq!(ROUTE_CHAT, "chat");
    }

    /// 欄ごとに前後の空白（全角を含む）を剥ぎ、欄の中の空白は残す。
    #[test]
    fn ruling_row_trims_each_field() {
        let line = "  policy:p \u{3000}|\t s2-1 | 2026-09-30T00:00Z |chat|  逐語  の 字  ";
        assert_eq!(ruling_row(line, Some("s2")), row("policy:p", "s2-1", "2026-09-30T00:00Z", "chat", "逐語  の 字"));
    }

    /// 先頭の欄が裁定 id でない行（逐語の欄にだけ id を持つ行・ほかの欄が id の行・地の文）と、欄が 4 でも 5 でもない行は無し。
    #[test]
    fn ruling_row_refuses_rows_whose_first_field_is_not_a_ruling_id() {
        let id = "s2-1:20260930T0000Z-1";
        for line in [
            format!("s2-1 | {id} | 2026-09-30T00:00Z | chat | 逐語"),
            format!("メモ | s2-1 | 2026-09-30T00:00Z | chat | {id}"),
            format!("{id} を引く | s2-1 | 2026-09-30T00:00Z | chat | 逐語"),
            String::new(),
            "batch:".to_owned(),
            format!("{id} | s2-1 | 2026-09-30T00:00Z"),
            format!("{id} | s2-1 | 2026-09-30T00:00Z | chat | 逐語 | 余り"),
            id.to_owned(),
        ] {
            assert_eq!(ruling_row(&line, Some("s2")), None, "{line}");
        }
    }

    /// 接頭辞が違う問い id の形の行は無し（batch: / policy: の裁定 id の行は接頭辞に依らず読める）。接頭辞が解けない周も同じ。
    #[test]
    fn ruling_row_reads_the_question_form_only_under_the_ledger_prefix() {
        let other = "tz-1:20260930T0000Z-1 | tz-1 | 2026-09-30T00:00Z | chat | 逐語";
        assert_eq!(ruling_row(other, Some("s2")), None, "接頭辞違い");
        assert_eq!(ruling_row(other, None), None, "接頭辞が解けない周");
        assert!(ruling_row(other, Some("tz")).is_some());
        let named = "batch:2026-09-28 | s2-1 | 2026-09-30T00:00Z | chat | 逐語";
        assert!(ruling_row(named, Some("tz")).is_some() && ruling_row(named, None).is_some(), "batch: は接頭辞に依らない");
        assert_eq!(ruling_row("policy:batch:x | s2-1 | 2026-09-30T00:00Z | 逐語", None).map(|found| found.id), Some("policy:batch:x".to_owned()));
    }
}
