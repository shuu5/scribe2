//! 打刻の合図の組み立ての群（設計 docs/design/seat-heartbeat.md §19・契約表の行 w・`s2-07l.737.4`）。
//!
//! 梯子の形（[`Pace`]）・梯子の記録の 1 行の字面（[`Ladder`]）・段の候補（[`candidate`]）・基準（[`settle`]）・段の待ち
//! （[`pointer_of`]）・合図の文面（[`signal`]）・alarm の語と上げの秒（`idle_alarm`）・段の上げ（[`raise`]）の群である。
//! `seat/tick.rs` からの**純移動**で、歯は 1 本も足していない（親に残る in-file の歯と e2e が従来どおり測る）。
//! 外から見える名と path は親の `pub use` が保ち、上げた可視性は `idle_alarm` の `pub(super)` 1 語だけである。

// flip-check: moved s2-07l.737.4

use super::{Pointer, Rows, ROW_LADDER};
use crate::fleet::json_lite::{self, Value};
use crate::name::NAME;
use crate::pipe::dispatch::facts::{Fact, Facts};
use crate::seat::state::{Event, Stamp};

/// 梯子の記録の schema 版。
const LADDER_SCHEMA: u64 = 1;

/// 行 2 本の値（黙りの閾値と梯子の列）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pace {
    /// `seat.tick_stale_s`（黙りの閾値＝settle の基準・Busy の古さ）。
    pub stale_s: u64,
    /// `seat.pointer_ladder_s`（段 n の待ちは n 番目・非空・狭義に昇順）。
    pub ladder: Vec<u64>,
}

impl Pace {
    /// 列の字面を読む。要素が数でない・狭義に昇順でない・空の列は `Err`（既定に倒さない・C1）。
    pub fn of(stale_s: u64, items: &[String]) -> Result<Self, String> {
        let ladder = items.iter().map(|item| item.parse::<u64>().map_err(|_| format!("{ROW_LADDER} の要素 {item:?} が数でない")));
        let ladder = ladder.collect::<Result<Vec<u64>, String>>()?;
        if ladder.is_empty() || ladder.iter().zip(ladder.iter().skip(1)).any(|(before, after)| before >= after) {
            return Err(format!("{ROW_LADDER} が非空の狭義の昇順でない"));
        }
        Ok(Self { stale_s, ladder })
    }

    /// 段 `step` の待ち（秒・列の `step` 番目）。列を越えた段は `None`（打ち切り）。
    pub fn wait_of(&self, step: u32) -> Option<u64> {
        usize::try_from(step).ok().and_then(|at| self.ladder.get(at)).copied()
    }
}

/// 梯子の記録（1 行 JSON・`schema` / `sent_at` / `step` / `digest`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ladder {
    /// 送った UTC 秒。
    pub sent_at: u64,
    /// 送った段（0 始まり）。
    pub step: u32,
    /// 基準の ts（未確定は `None`＝`null`）。
    pub digest: Option<u64>,
}

impl Ladder {
    /// 1 行の flat JSON にする。
    pub fn to_line(&self) -> String {
        json_lite::write_object(&[
            ("schema", Value::Num(LADDER_SCHEMA)),
            ("sent_at", Value::Num(self.sent_at)),
            ("step", Value::Num(u64::from(self.step))),
            ("digest", self.digest.map_or(Value::Null, Value::Num)),
        ])
    }

    /// file の本文を読む（ちょうど 1 行・key の欠け・型違い・schema 違い・段が u32 に収まらない形は `None`）。
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let pairs = json_lite::parse_object(lines.next()?).ok()?;
        if lines.next().is_some() {
            return None;
        }
        let value = |key: &str| pairs.iter().find(|(found, _)| found == key).map(|(_, value)| value);
        if value("schema")?.as_num()? != LADDER_SCHEMA {
            return None;
        }
        let digest = match value("digest")? {
            Value::Null => None,
            other => Some(other.as_num()?),
        };
        Some(Self { sent_at: value("sent_at")?.as_num()?, step: u32::try_from(value("step")?.as_num()?).ok()?, digest })
    }
}

/// 段の候補（pure）: 記録が無い・基準と今の digest が違う（変化）→ 0、同じ → 記録の段 + 1。基準の無い記録は 0（呼び手は
/// settle の後にだけ撃つ）。
pub fn candidate(record: Option<&Ladder>, digest: u64) -> u32 {
    match record.map(|found| (found.step, found.digest)) {
        Some((step, Some(base))) if base == digest => step.saturating_add(1),
        _ => 0,
    }
}

/// 基準の確定（pure）: `sent_at` より後の Stop の打刻の最後の ts（合図に応えた turn の終わり）、無ければ `sent_at` から
/// `stale_s` を過ぎた周の今の digest（応えない席＝梯子が登る側）、どちらでもなければ `None`（settling）。
pub fn settle(record: &Ladder, stamps: &[Stamp], digest: u64, now: u64, stale_s: u64) -> Option<u64> {
    let answered = stamps.iter().rev().find(|stamp| stamp.event == Event::Stop && stamp.ts > record.sent_at).map(|stamp| stamp.ts);
    answered.or_else(|| (now.saturating_sub(record.sent_at) >= stale_s).then_some(digest))
}

/// 段の候補の打ち切りと床（pure）: 段が列を越えれば [`Pointer::Stopped`]、記録の `sent_at` から待ちが経っていなければ
/// [`Pointer::Wait`]（残り秒）、それ以外（記録なしを含む）は [`Pointer::Open`]。
pub fn pointer_of(pace: &Pace, sent_at: Option<u64>, step: u32, now: u64) -> Pointer {
    let Some(wait) = pace.wait_of(step) else {
        return Pointer::Stopped;
    };
    match sent_at.map(|at| now.saturating_sub(at)) {
        Some(elapsed) if elapsed < wait => Pointer::Wait(wait.saturating_sub(elapsed)),
        _ => Pointer::Open,
    }
}

/// 打刻の合図の文面（**正本はこの 1 関数**・先頭の `<NAME> tick:` が器自身の目印・次の待ちは列から引き、最後の段は次が無い）。
pub fn signal(step: u32, pace: &Pace) -> String {
    let next = pace.wait_of(step.saturating_add(1)).map_or_else(|| "次の合図は無い・打ち切り".to_owned(), |wait| format!("次の合図は {wait} 秒後"));
    format!("{NAME} tick: heartbeat step={step} — 台帳の現在地（bd --readonly ready --limit 0）から続きを進める（変化が無ければ{next}）")
}

/// 段の上げの判定（設計 §17 形 1 / 2）: 行が無い・読めない周は上げず語 `idle-unset`、値が正で live が 0 と測れ 0 本の分数 × 60 が
/// 値以上の周だけ上げて語 `idle`、他（値 0・測れない・値なし）は上げず語なし。事前審査の本数を持つ周（dispatcher.md §27 形 4）は
/// 続けて、`seat.precheck_alarm_s` の行が無い・読めない周は上げず語 `precheck-unset`、値が正で最も古い束の初めて見た時刻から
/// 値の秒数以上経った周は上げて語 `precheck`。返り値は（上げた周の値の小さい方, `alarm=` の語の列〔u の語が先〕）。
pub(super) fn idle_alarm(rows: &Rows, found: &Facts, now: u64) -> (Option<u64>, Vec<&'static str>) {
    let idle = match (rows.idle_alarm_s, &found.live, &found.idle) {
        (None, _, _) => (None, Some("idle-unset")),
        (Some(value), Fact::Value(0), Fact::Value(minutes)) if value > 0 && minutes.saturating_mul(60) >= value => (Some(value), Some("idle")),
        _ => (None, None),
    };
    let precheck = found.precheck.as_ref().map_or((None, None), |tally| match (rows.precheck_alarm_s, tally.oldest) {
        (None, _) => (None, Some("precheck-unset")),
        (Some(value), Some(first)) if value > 0 && now.saturating_sub(first) >= value => (Some(value), Some("precheck")),
        _ => (None, None),
    });
    // 床の検査の不合格は語 `floor` だけ足し、上げの秒は黙りの閾値を縮めない最大値で渡す（段だけ 0 に戻る・設計 §35 約束 5）。
    let floor = found.floor.map_or((None, None), |_| (Some(u64::MAX), Some("floor")));
    // 未反映の裁定は 1 件以上の周に語 `unreflected` だけ足し、段は上げない（precheck の後・floor の前・設計 §38 約束 7）。
    let unreflected = (found.unreflected > 0).then_some("unreflected");
    ([idle.0, precheck.0, floor.0].into_iter().flatten().min(), [idle.1, precheck.1, unreflected, floor.1].into_iter().flatten().collect())
}

/// 段の上げ（pure・設計 §17 形 3 (a)(b)・**1 本**）: 上げた周は（黙りの閾値, 段）を（`seat.tick_stale_s` と値の小さい方, 0）に。
pub fn raise(pace: &Pace, step: u32, alarm_s: Option<u64>) -> (u64, u32) {
    alarm_s.map_or((pace.stale_s, step), |value| (pace.stale_s.min(value), 0))
}
