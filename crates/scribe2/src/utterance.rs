//! 発話の仕分けの口（設計 docs/design/dialogue-surface.md §10・ADR-0087・SRS FR88 / FR65）。
//!
//! user の発話 1 件ごとに「要望（開いた memo へ）」か「会話」の札を [`EventKind::UtteranceSorted`] 1 件で記帳する（[`sort`]・台帳は
//! 書かない）。逐語を返す口は [`show`] だけで、席が ts を名指したときに限る。発話がどう仕分け済みかを決める関数は [`sorted_of`] の
//! 1 本だけで、IO を持たない（turn の終わりの止めも局面の出力もこの関数に同じ答えを出させる）。どちらの口も event log を
//! [`store::read_all`] で読む（人が撃つ 1 回・末尾の窓だけを読まない）。

pub mod cli;

use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{cli as fleet_cli, Case, Event, EventKind, Sorting, SCHEMA};
use crate::ledger::{self, form::MEMO_LABEL};
use std::collections::BTreeSet;
use std::path::Path;

/// 発話の仕分けの状態（**閉じた 3 値**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// 要望も答えも会話の札も無い。
    Unsorted,
    /// 会話の札だけが在る。
    ChatOnly,
    /// 要望か答えが在る（会話の札は数えない）。memo の id と裁定 id はそれぞれ整列・重なりなし。
    Linked { memos: Vec<String>, rulings: Vec<String> },
}

/// 発話 `ts` の仕分け済みかを決める（pure・**この 1 本だけ**）。`events` のうち `ts` を指す仕分け（[`Case::Sorted`]）と結びの裁定
/// （[`Case::Ruling`]）だけを読む。要望か答えが 1 つでも在れば [`Standing::Linked`]（会話の札は外れる・札の並びに依らない）、
/// 会話だけなら [`Standing::ChatOnly`]、どれも無ければ [`Standing::Unsorted`]。
pub fn sorted_of(ts: &str, events: &[Event]) -> Standing {
    let (mut memos, mut rulings, mut chat) = (BTreeSet::new(), BTreeSet::new(), false);
    for event in events {
        match &event.case {
            Some(Case::Sorted { utterance, sorting }) if utterance == ts => match sorting {
                Sorting::Request => {
                    memos.insert(event.bead.clone());
                }
                Sorting::Chat => chat = true,
            },
            Some(Case::Ruling { utterance, ruling, .. }) if utterance == ts => {
                rulings.insert(ruling.clone());
            }
            _ => {}
        }
    }
    if memos.is_empty() && rulings.is_empty() {
        return if chat { Standing::ChatOnly } else { Standing::Unsorted };
    }
    Standing::Linked { memos: memos.into_iter().collect(), rulings: rulings.into_iter().collect() }
}

/// 断る理由（**閉じた 4 語**・判定の順・断る周は何も書かない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// その ts の発話 event が無い。
    NoUtterance,
    /// 要望か答えを持つ発話へ会話を付けようとした。
    Linked,
    /// 台帳を読めない。
    LedgerUnreadable,
    /// 名指しが開いた memo でない（無い・閉じた・label `intake:memo` が無い）。
    NotMemo,
}

/// [`Refusal`] の全 variant（判定の順・`enum-slices` が集合完全性を測る）。
pub const REFUSALS: &[Refusal] = &[Refusal::NoUtterance, Refusal::Linked, Refusal::LedgerUnreadable, Refusal::NotMemo];

impl Refusal {
    /// 行に出す理由の 1 語。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoUtterance => "no-utterance",
            Self::Linked => "linked",
            Self::LedgerUnreadable => "ledger-unreadable",
            Self::NotMemo => "not-memo",
        }
    }
}

/// 仕分けの口が通らなかった形（rc と行は呼び手が決める）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// 閉じた 4 語の断り（何も書いていない）。
    Refused(Refusal),
    /// event log を読めない（何も書いていない・理由の本文）。
    LogUnreadable(Vec<String>),
    /// 仕分けの event を書けなかった。
    WriteFailed,
}

/// 仕分けの周の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Done {
    /// 仕分けの event を 1 件書いた。
    Written,
    /// 同じ札が既に在り、何も書かなかった。
    Already,
}

/// 仕分けの向き。
pub enum Sort<'a> {
    /// 開いた memo への要望（台帳を `bd` で 1 回読む）。
    Request { repo: &'a Path, memo: &'a str, bd: &'a str },
    /// 会話（台帳を読まない）。
    Chat,
}

/// 開いた bead の status。
const OPEN: &str = "open";

/// 発話 `ts` を仕分ける。断りは no-utterance → linked → ledger-unreadable → not-memo の順で、台帳を読むのは要望の周だけ。
/// 同じ発話と同じ memo の要望・会話の札が在る発話への会話は何も書かず [`Done::Already`]（要望の周は台帳より先に見る）。
pub fn sort(state_dir: &Path, ts: &str, how: &Sort<'_>) -> Result<Done, Failure> {
    let events = read(state_dir)?;
    if !events.iter().any(|event| event.ts == ts && matches!(event.case, Some(Case::Utterance { .. }))) {
        return Err(Failure::Refused(Refusal::NoUtterance));
    }
    let (standing, memo) = (sorted_of(ts, &events), if let Sort::Request { memo, .. } = how { Some(*memo) } else { None });
    match (&standing, memo) {
        (Standing::Linked { .. }, None) => return Err(Failure::Refused(Refusal::Linked)),
        (Standing::ChatOnly, None) => return Ok(Done::Already),
        (Standing::Linked { memos, .. }, Some(named)) if memos.iter().any(|found| found == named) => return Ok(Done::Already),
        _ => {}
    }
    if let Sort::Request { repo, memo, bd } = how {
        let bead = ledger::show(bd, repo, memo).map_err(|_| Failure::Refused(Refusal::LedgerUnreadable))?;
        if !bead.is_some_and(|found| found.status == OPEN && found.labels.iter().any(|label| label == MEMO_LABEL)) {
            return Err(Failure::Refused(Refusal::NotMemo));
        }
    }
    let sorting = if memo.is_some() { Sorting::Request } else { Sorting::Chat };
    let event = sorted_event(ts, sorting, memo.unwrap_or_default());
    let policy = LockPolicy::embedded().map_err(|_| Failure::WriteFailed)?;
    store::append(state_dir, &event, policy).map_err(|_| Failure::WriteFailed)?;
    Ok(Done::Written)
}

/// 発話 `ts` の逐語（発話 event の detail をそのまま・無い ts は [`Refusal::NoUtterance`]）。
pub fn show(state_dir: &Path, ts: &str) -> Result<String, Failure> {
    let events = read(state_dir)?;
    let said = events.into_iter().find(|event| event.ts == ts && matches!(event.case, Some(Case::Utterance { .. })));
    said.map(|event| event.detail.unwrap_or_default()).ok_or(Failure::Refused(Refusal::NoUtterance))
}

/// event log を全部読む（読めない log は 0 件に潰さない）。
fn read(state_dir: &Path) -> Result<Vec<Event>, Failure> {
    store::read_all(state_dir).map_err(|errors| Failure::LogUnreadable(errors.iter().map(ToString::to_string).collect()))
}

/// 仕分けの event（[`Case::Sorted`]・要望の行だけ `bead` に memo の id・会話は空）。
fn sorted_event(ts: &str, sorting: Sorting, memo: &str) -> Event {
    Event {
        schema: SCHEMA,
        ts: fleet_cli::now_utc(),
        kind: EventKind::UtteranceSorted,
        run: String::new(),
        bead: memo.to_owned(),
        host: fleet_cli::host(),
        actor: EventKind::UtteranceSorted.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: None,
        allowance: None,
        registration: None,
        mark: None,
        account: None,
        cost: None,
        rule: None,
        case: Some(Case::Sorted { utterance: ts.to_owned(), sorting }),
    }
}

#[cfg(test)]
mod tests {
    use super::{sorted_of, Standing};
    use crate::fleet::{Case, Channel, Event, EventKind, Sorting, ACTOR_HUMAN, SCHEMA};

    const TS: &str = "2026-09-30T07:05:09.123Z";

    /// event 1 件（kind と bead と本体だけ選ぶ）。
    fn event(kind: EventKind, bead: &str, case: Option<Case>) -> Event {
        Event {
            schema: SCHEMA,
            ts: "2026-09-30T08:00:00Z".to_owned(),
            kind,
            run: String::new(),
            bead: bead.to_owned(),
            host: "h".to_owned(),
            actor: ACTOR_HUMAN.to_owned(),
            stage: None,
            seat: None,
            pid: None,
            detail: None,
            allowance: None,
            registration: None,
            mark: None,
            account: None,
            cost: None,
            rule: None,
            case,
        }
    }

    /// `utterance` の ts を指す仕分けの札。
    fn sorted(utterance: &str, sorting: Sorting, memo: &str) -> Event {
        event(EventKind::UtteranceSorted, memo, Some(Case::Sorted { utterance: utterance.to_owned(), sorting }))
    }

    /// `utterance` の ts を指す答え（結びの裁定）。
    fn answer(utterance: &str, ruling: &str) -> Event {
        let case = Case::Ruling {
            ruling: ruling.to_owned(),
            utterance: utterance.to_owned(),
            channel: Channel::Chat,
            question_ts: "2026-09-30T06:00:00Z".to_owned(),
            asked: None,
        };
        event(EventKind::RulingReceived, "s2-q1", Some(case))
    }

    fn linked(memos: &[&str], rulings: &[&str]) -> Standing {
        let owned = |list: &[&str]| list.iter().map(|word| (*word).to_owned()).collect();
        Standing::Linked { memos: owned(memos), rulings: owned(rulings) }
    }

    /// (f) 3 値の表: 無し・会話だけ・要望・答え・会話の後の要望・会話の後の答え・承認に使った発話を会話にした形（承認 event は読まない）・
    /// 要望の後の会話。他の発話を指す札と case の無い古い形の裁定は数えない。結びありは札の並びに依らない。
    #[test]
    fn utterance_sorted_of_decides_the_three_values_and_drops_chat_behind_a_link() {
        let chat = || sorted(TS, Sorting::Chat, "");
        let request = |memo: &str| sorted(TS, Sorting::Request, memo);
        let approval = || event(EventKind::ApprovalReceived, "s2-x.1", None);
        let table: Vec<(&str, Vec<Event>, Standing)> = vec![
            ("無し", vec![], Standing::Unsorted),
            ("会話だけ", vec![chat()], Standing::ChatOnly),
            ("要望", vec![request("s2-m1")], linked(&["s2-m1"], &[])),
            ("答え", vec![answer(TS, "s2-q1:1")], linked(&[], &["s2-q1:1"])),
            ("会話の後の要望", vec![chat(), request("s2-m1")], linked(&["s2-m1"], &[])),
            ("会話の後の答え", vec![chat(), answer(TS, "s2-q1:1")], linked(&[], &["s2-q1:1"])),
            ("承認に使った発話を会話にした形", vec![approval(), chat(), approval()], Standing::ChatOnly),
            ("要望の後の会話", vec![request("s2-m1"), chat()], linked(&["s2-m1"], &[])),
            ("要望と答え", vec![request("s2-m2"), answer(TS, "s2-q1:1"), request("s2-m1")], linked(&["s2-m1", "s2-m2"], &["s2-q1:1"])),
            ("他の発話を指す札", vec![sorted("2026-09-30T07:05:10Z", Sorting::Request, "s2-m9"), answer("x", "s2-q9:1")], Standing::Unsorted),
            ("case の無い古い形の裁定", vec![event(EventKind::RulingReceived, "s2-x.1", None)], Standing::Unsorted),
        ];
        for (name, events, expected) in table {
            assert_eq!(sorted_of(TS, &events), expected, "{name}");
            let reversed: Vec<Event> = events.into_iter().rev().collect();
            assert_eq!(sorted_of(TS, &reversed), expected, "{name}: 並びを逆にしても同じ");
        }
    }

    /// (g) 同じ入力を 2 回渡すと同じ結果になる（重なる札は 1 つに数える）。
    #[test]
    fn utterance_sorted_of_gives_the_same_result_for_the_same_input() {
        let events = [sorted(TS, Sorting::Request, "s2-m1"), sorted(TS, Sorting::Request, "s2-m1"), sorted(TS, Sorting::Chat, "")];
        let first = sorted_of(TS, &events);
        assert_eq!(first, linked(&["s2-m1"], &[]), "同じ memo の札は 1 つ");
        assert_eq!(sorted_of(TS, &events), first, "同じ入力は同じ結果");
    }
}
