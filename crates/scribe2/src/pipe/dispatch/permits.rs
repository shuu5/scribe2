//! 上限の許可の見え方（設計 docs/design/limit-permit.md §21・契約表の行 e・FR111・AC85・NFR4）。
//!
//! `pipe show --run` が bead の許可を状態の語つきで 1 件 1 行に並べ、`pipe dispatch ls` が効いている許可だけを 1 行ずつ並べる。
//! 着地と期限と効きは行 b の公開の口（[`permit`]）を呼ぶだけで写しを持たない（C2）。何も書かない。

use super::{Input, Read};
use crate::fleet::Event;
use crate::pipe::permit::{self, Effect, Record};
use crate::rules::int_row;
use std::collections::BTreeSet;

/// 許可 1 件の状態の語（閉じた 6 値・宣言順は先に当たる順・FR111）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::pipe) enum Word {
    /// bead の便が着地した。
    Landed,
    /// 同じ bead と行の次の記帳が取り消し。
    Revoked,
    /// 同じ bead と行の次の記帳が新しい許可。
    Superseded,
    /// 期限の後。
    Expired,
    /// 値が manifest の値以下。
    Overtaken,
    /// どれにも当たらない（効いている）。
    Active,
}

impl Word {
    /// 行に書く字面（FR111 の 6 語）。
    pub(in crate::pipe) fn as_str(self) -> &'static str {
        match self {
            Self::Landed => "landed",
            Self::Revoked => "revoked",
            Self::Superseded => "superseded",
            Self::Expired => "expired",
            Self::Overtaken => "overtaken",
            Self::Active => "active",
        }
    }
}

/// 同じ bead と行の許可を記帳の順に、先に当たる語とともに返す（`now` は UNIX 秒・`declared` は manifest の値）。
///
/// 語の `Active` は高々 1 つで、在れば [`permit::permitted`] が返す許可と値・裁定 id・期限が同じ。
pub(in crate::pipe) fn judged(rule: &str, declared: u64, bead: &str, events: &[Event], now: u64) -> Vec<(Record, Word)> {
    let records = permit::records(bead, rule, events);
    let landed = permit::landed(bead, events).is_some();
    let word_of = |at: usize, value: u64, until: &str| match (landed, records.get(at + 1)) {
        (true, _) => Word::Landed,
        (false, Some(Record::Revoke { .. })) => Word::Revoked,
        (false, Some(Record::Permit { .. })) => Word::Superseded,
        (false, None) if permit::expired(until, now) => Word::Expired,
        (false, None) if value <= declared => Word::Overtaken,
        (false, None) => Word::Active,
    };
    records
        .iter()
        .enumerate()
        .filter_map(|(at, record)| match record {
            Record::Permit { value, until, .. } => Some((record.clone(), word_of(at, *value, until))),
            Record::Revoke { .. } => None,
        })
        .collect()
}

/// 許可の記帳を持つ（bead・行 id）の組を、bead と行 id の字の順に重ねずに返す（取り消しだけの組は返さない）。
pub(in crate::pipe) fn held(events: &[Event]) -> Vec<(String, String)> {
    let named: BTreeSet<(String, String)> = events
        .iter()
        .filter_map(|event| match event.detail.as_deref().and_then(Record::parse)? {
            Record::Permit { rule, .. } | Record::Revoke { rule } => Some((event.bead.clone(), rule)),
        })
        .collect();
    named
        .into_iter()
        .filter(|(bead, rule)| permit::records(bead, rule, events).iter().any(|record| matches!(record, Record::Permit { .. })))
        .collect()
}

/// `pipe show --run` が足す bead の許可の行（行 id の字の順・記帳の順）。
///
/// `declared` は行 id の manifest の値の読み手（呼ばれるのは許可の記帳を持つ行 id だけ）。値を読めない行 id は語を出さず
/// `permit: unmeasured` の 1 行にする（読めないのに語を言わない・C10）。
pub(in crate::pipe) fn show_lines(bead: &str, events: &[Event], now: u64, declared: &dyn Fn(&str) -> Result<u64, String>) -> Vec<String> {
    let mut lines = Vec::new();
    for (_, rule) in held(events).into_iter().filter(|(found, _)| found == bead) {
        let Ok(value) = declared(&rule) else {
            let records = permit::records(bead, &rule, events).iter().filter(|record| matches!(record, Record::Permit { .. })).count();
            lines.push(format!("permit: unmeasured rule={rule} records={records}"));
            continue;
        };
        for (record, word) in judged(&rule, value, bead, events, now) {
            if let Record::Permit { value: set, until, ruling, .. } = record {
                lines.push(format!("permit: rule={rule} value={set} declared={value} until={until} ruling={ruling} state={}", word.as_str()));
            }
        }
    }
    lines
}

/// `pipe dispatch ls` が memo の行の後ろに足す、効いている許可の行（bead と行 id の字の順・同じ周の 1 回の event の読みを借りる）。
///
/// event log を読めない周と、値を読めない行 id は理由を名乗る（効いている許可 0 件と融合しない・C10・NFR4）。
pub(super) fn lines(input: &Input<'_>, read: &Read, now: u64) -> Vec<String> {
    let Some(events) = read.events.as_deref() else {
        return vec!["[DISPATCH-PERMIT-UNMEASURED reason=events]".to_owned()];
    };
    let mut unread = BTreeSet::new();
    held(events)
        .into_iter()
        .filter_map(|(bead, rule)| match int_row(input.manifest, &rule) {
            Err(_) => unread.insert(rule.clone()).then(|| format!("[DISPATCH-PERMIT-UNMEASURED reason=rules rule={rule}]")),
            Ok(declared) => match permit::permitted(&rule, declared, &bead, events, now) {
                Effect::Declared => None,
                Effect::Permitted { value, ruling, until } => {
                    Some(format!("[DISPATCH-PERMIT] bead={bead} rule={rule} value={value} declared={declared} until={until} ruling={ruling}"))
                }
            },
        })
        .collect()
}
