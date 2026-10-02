//! 上限の許可の記帳の本体・読み手・効きの純関数（設計 limit-permit.md §18・ADR-0106）。
//!
//! 許可は event log の 1 kind（`LimitPermitted`）の `detail` 1 行で、本体は [`Record`] の閉じた 2 値（許可と取り消し）である。
//! 「この bead のこの行に今効いている許可があるか」は [`permitted`] が event の列と今の時刻だけから答える（manifest の宣言値は
//! 呼び手が渡す・C10: 宣言値と実行時の記録を別の型で持つ）。event の列を読めない周は呼び手が止め、この module は許可へ倒す枝を持たない。
//!
//! 口はどれも `pub`（後の行が呼ぶまで dead_code の札を要らなくし、純関数の歯を e2e に置くため）。

use crate::fleet::{epoch_of, Event, EventKind, Stage};

/// 記帳 1 件の本体（閉じた 2 値・`detail` の 1 行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    /// 許可（`rule=<行 id> value=<整数> until=<YYYY-MM-DDTHH:MM:SSZ> ruling=<裁定 id>`）。
    Permit {
        /// 許可の対象の rules 行の id。
        rule: String,
        /// 許可の値（manifest の値より大きい整数）。
        value: u64,
        /// 期限（UTC の秒の形）。
        until: String,
        /// 結んだ裁定の id。
        ruling: String,
    },
    /// 取り消し（`rule=<行 id> revoked`）。
    Revoke {
        /// 取り消す rules 行の id。
        rule: String,
    },
}

impl Record {
    /// `detail` に書く 1 行。
    pub fn render(&self) -> String {
        match self {
            Self::Permit { rule, value, until, ruling } => format!("rule={rule} value={value} until={until} ruling={ruling}"),
            Self::Revoke { rule } => format!("rule={rule} revoked"),
        }
    }

    /// [`Self::render`] の字面から読む。key の順違い・語の欠けと余り・数でない値・空の値・秒の形でない期限・綴り違いは `None`。
    pub fn parse(text: &str) -> Option<Self> {
        let mut words = text.split(' ');
        let rule = word(words.next()?, "rule=")?;
        let record = match words.next()? {
            "revoked" => Self::Revoke { rule: rule.to_owned() },
            value => {
                let value = word(value, "value=").filter(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))?.parse().ok()?;
                let until = word(words.next()?, "until=").filter(|until| epoch_of(until).is_some())?;
                let ruling = word(words.next()?, "ruling=")?;
                Self::Permit { rule: rule.to_owned(), value, until: until.to_owned(), ruling: ruling.to_owned() }
            }
        };
        words.next().is_none().then_some(record)
    }

    /// 記帳が指す rules 行の id。
    fn rule(&self) -> &str {
        match self {
            Self::Permit { rule, .. } | Self::Revoke { rule } => rule,
        }
    }
}

/// `<key>` の頭を外した空でない値。
fn word<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.strip_prefix(key).filter(|value| !value.is_empty())
}

/// 同じ bead と行の記帳を、列の順（＝記帳の順）のまま返す（許可も取り消しも）。ほかの kind の行は `detail` の字が同じでも数えない。
pub fn records(bead: &str, rule: &str, events: &[Event]) -> Vec<Record> {
    events
        .iter()
        .filter(|event| event.kind == EventKind::LimitPermitted && event.bead == bead)
        .filter_map(|event| event.detail.as_deref().and_then(Record::parse))
        .filter(|record| record.rule() == rule)
        .collect()
}

/// bead の便が `Landed` に達した最初の行の便 id（無ければ `None`）。便の着地は戻らないので、記帳の前か後かを問わない。
pub fn landed(bead: &str, events: &[Event]) -> Option<String> {
    events.iter().find(|event| event.bead == bead && event.stage == Some(Stage::Landed)).map(|event| event.run.clone())
}

/// 今が期限以後か、期限を読めない周に `true`（期限ちょうどは切れている・読めない期限は切れている）。
pub fn expired(until: &str, now: u64) -> bool {
    epoch_of(until).is_none_or(|at| now >= at)
}

/// 効く cap の出所（閉じた 2 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// manifest の値のまま。
    Declared,
    /// 許可の値で読む。
    Permitted {
        /// 許可の値。
        value: u64,
        /// 結んだ裁定の id。
        ruling: String,
        /// 期限（UTC の秒の形）。
        until: String,
    },
}

/// bead のこの行に今効いている許可（`now` は UNIX 秒・`declared` は呼び手が manifest から読んだ値）。
///
/// 上から照らし、最初に外れた所で [`Effect::Declared`]: (a) 同じ bead と行の最新の記帳が許可（無い・取り消しは外れ）
/// (b) bead の便が着地していない (c) 期限が切れていない (d) 許可の値が `declared` より大きい。最新の記帳だけを見るので、
/// 新しい許可が先に切れても古い許可へ戻らない。
pub fn permitted(rule: &str, declared: u64, bead: &str, events: &[Event], now: u64) -> Effect {
    let Some(Record::Permit { value, until, ruling, .. }) = records(bead, rule, events).pop() else {
        return Effect::Declared;
    };
    if landed(bead, events).is_some() || expired(&until, now) || value <= declared {
        return Effect::Declared;
    }
    Effect::Permitted { value, ruling, until }
}
