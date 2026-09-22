//! `pipe report`（設計 docs/design/pipeline.md §5.8・FR22）。
//!
//! 到達点の主張「人由来の event が **approval 以外に 0 件**」を機械が数えて示す面である
//! （AC1）。人手 0 は「そう見えた」ではなく **log に残った actor** で決める。
//!
//! **便の数と land の数は replay から、人由来の event は生の行から数える**。replay は
//! 便ごとに最後の段しか残さないので、途中に挟まった人の手は replay からは読めない
//! （承認の後に手で段を動かした周が、replay 上は機械だけで進んだ便に見える）。
//!
//! **審査の理由の型も生の行から数える**（設計 contract-source.md §22・`s2-07l.395`）: `RunStage stage=Reviewed`
//! の detail（`verdict:<V> kind:<k>`）のうち verdict が PASS でないものが母集団で、`kind:` を持たない古い
//! event は `unparsed` に数える。内訳は [`FINDING_KINDS`] の宣言順に 7 語とも出す（0 も出す）——「潰す」は
//! kind ごとの内訳が 0 に落ちたことを機械で見ることである。
//!
//! **2 行目は消費**（設計 gate-cost.md §26 形 (2) (d)・[`Spent`]）: 消費の event（`RunCost`）の token の和を、それを持つ
//! 便の数（母集団）と同じ行に出し、gate の record の段の秒の和を添える。

use super::review::{read_detail, FindingKind, FINDING_KINDS};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::json_lite;
use crate::fleet::store::{self, StoreError};
use crate::fleet::{replay, Cost, Event, EventKind, Stage, ACTOR_HUMAN};
use crate::pipe::gate::Verdict;
use crate::pipe::verify_log_path;
use std::collections::BTreeSet;
use std::path::Path;

/// 数えた 6 つ。**合算値だけを出さない**（内訳が到達点の主張そのものである）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counted {
    /// 置き場に在る便の数。
    pub runs: usize,
    /// 終端（`Landed`）まで通った便の数。
    ///
    /// **「main に載った数」ではない**: `--pr-cmd` の便は main を動かさず `Landed
    /// detail=pr` で終わるが、便としては終端に達しているのでここに入る。main へ載った
    /// 数だけを見たいときは面 5（`verdicts.jsonl`）の行数を読む（あちらは squash した
    /// 便だけを書く）。
    pub landed: usize,
    /// 人由来の event の数。
    pub human_events: usize,
    /// 人由来のうち承認でないものの数（**到達点はここが 0**・承認は [`is_approval`] の 2 kind）。
    pub human_events_other_than_approval: usize,
    /// 審査で PASS にならなかった `Reviewed` の event の数（`by_kind` の母集団・`runs` と同じ範囲で日付では絞らない）。
    pub review_fail: usize,
    /// `review_fail` の理由の型ごとの内訳（[`FINDING_KINDS`] の宣言順・合計は `review_fail`）。
    pub by_kind: [usize; FINDING_KINDS.len()],
    /// run 無しの裁定（[`EventKind::RulingReceived`]）の数（設計 fleet-event-log.md §9 (3)・kind で数える）。
    pub rulings: usize,
}

/// 人の手として許す「承認」の kind か（設計 fleet-event-log.md §9 (3)・FR22）: 便の承認と run 無しの裁定の 2 つ。
pub fn is_approval(kind: EventKind) -> bool {
    kind == EventKind::ApprovalReceived || kind == EventKind::RulingReceived
}

impl Counted {
    /// stdout の 1 行（設計 §5.8 の字面 + §22 の `review_fail=` / `by_kind=`・既存の token は不変）。
    ///
    /// `rulings=<n>`（fleet-event-log.md §9 (3)）は `human_events_other_than_approval=` の直後に**裁定が 1 件以上在る周だけ**
    /// 出す——裁定の無い置き場の到達点の行は従来の字面のまま（読み手が行を逐語で突き合わせる面を動かさない）。
    pub fn line(self) -> String {
        let by_kind: Vec<String> = FINDING_KINDS
            .iter()
            .zip(self.by_kind)
            .map(|(kind, count)| format!("{}:{count}", kind.as_str()))
            .collect();
        let rulings = if self.rulings > 0 { format!(" rulings={}", self.rulings) } else { String::new() };
        format!(
            "runs={} landed={} human_events={} human_events_other_than_approval={}{rulings} review_fail={} by_kind={}",
            self.runs,
            self.landed,
            self.human_events,
            self.human_events_other_than_approval,
            self.review_fail,
            by_kind.join(",")
        )
    }
}

/// event 列から数える。
///
/// 承認だけを人の手として許す判定は **kind で行う**（actor の字面ではない）——`actor` は
/// 「誰が起こしたか」、`kind` は「何が起きたか」で、到達点が言う例外は後者だからである。承認の kind は
/// [`is_approval`] の 2 つ（run 無しの裁定も承認 event・ADR-0037）。
pub fn count(events: &[Event]) -> Counted {
    let state = replay(events);
    let human: Vec<&Event> = events
        .iter()
        .filter(|event| event.actor == ACTOR_HUMAN)
        .collect();
    let mut by_kind = [0_usize; FINDING_KINDS.len()];
    for kind in events.iter().filter_map(review_failure) {
        if let Some(slot) = FINDING_KINDS.iter().position(|found| *found == kind).and_then(|at| by_kind.get_mut(at)) {
            *slot = slot.saturating_add(1);
        }
    }
    Counted {
        runs: state.runs.len(),
        landed: state
            .runs
            .values()
            .filter(|run| run.stage == Stage::Landed)
            .count(),
        human_events: human.len(),
        human_events_other_than_approval: human
            .iter()
            .filter(|event| !is_approval(event.kind))
            .count(),
        review_fail: by_kind.iter().sum(),
        by_kind,
        rulings: events.iter().filter(|event| event.kind == EventKind::RulingReceived).count(),
    }
}

/// `RunStage stage=Reviewed` で verdict が PASS でない event の理由の型（母集団の外は `None`）。
///
/// detail が無い・verdict を読めない `Reviewed` も母集団に入れる（PASS と読めた周だけが外＝壊れた detail を
/// PASS に化けさせない・C10）。理由の型の読みは審査の書き手の対 [`read_detail`] 1 本。
fn review_failure(event: &Event) -> Option<FindingKind> {
    if event.kind != EventKind::RunStage || event.stage != Some(Stage::Reviewed) {
        return None;
    }
    let (verdict, kind) = read_detail(event.detail.as_deref().unwrap_or_default());
    (verdict != Some(Verdict::Pass)).then_some(kind)
}

/// 消費の集計（`pipe report` の 2 行目・設計 gate-cost.md §26 形 (2) (d)）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spent {
    /// 消費の event を 1 件以上持つ便の数（**母集団**・token の和はこの便たちのもの）。
    pub with_usage: usize,
    /// 全消費の event の `output_tokens` の和。
    pub out: u64,
    /// 全消費の event の `cache_read_input_tokens` の和。
    pub cache_read: u64,
    /// 置き場の全便の gate の record（`verify.jsonl`）の `secs=` の和（秒を持たない record は足さない・C10）。
    pub gate_secs: u64,
}

impl Spent {
    /// stdout の 2 行目（`cost: with_usage=<便数> out=<token> cache_read=<token> gate_secs=<秒の和>`）。
    pub fn line(self) -> String {
        format!(
            "cost: with_usage={} out={} cache_read={} gate_secs={}",
            self.with_usage, self.out, self.cache_read, self.gate_secs
        )
    }
}

/// event 列と置き場の gate の record から消費を数える（token は event・秒は record の写し）。
pub fn spend(state_dir: &Path, events: &[Event]) -> Spent {
    let costs: Vec<(&str, Cost)> = events
        .iter()
        .filter(|event| event.kind == EventKind::RunCost)
        .filter_map(|event| event.cost.map(|cost| (event.run.as_str(), cost)))
        .collect();
    let runs: BTreeSet<&str> = costs.iter().map(|(run, _)| *run).collect();
    Spent {
        with_usage: runs.len(),
        out: costs.iter().fold(0_u64, |sum, (_, cost)| sum.saturating_add(cost.usage.output)),
        cache_read: costs.iter().fold(0_u64, |sum, (_, cost)| sum.saturating_add(cost.usage.cache_read)),
        gate_secs: replay(events).runs.keys().fold(0_u64, |sum, id| sum.saturating_add(gate_secs_of(state_dir, id))),
    }
}

/// 便 1 本の `verify.jsonl` の `secs` の和（file が無い・行が読めない・`secs` を持たない record は 0 を足す）。
fn gate_secs_of(state_dir: &Path, id: &str) -> u64 {
    let text = std::fs::read_to_string(verify_log_path(state_dir, id)).unwrap_or_default();
    text.lines()
        .filter_map(|line| json_lite::parse_object(line).ok())
        .filter_map(|pairs| pairs.into_iter().find(|(key, _)| key == "secs").and_then(|(_, value)| value.as_num()))
        .fold(0_u64, u64::saturating_add)
}

/// `pipe report`。読むだけで、event を 1 件も書かない。
pub fn report(state_dir: &Path) -> Outcome {
    match store::read_all(state_dir) {
        // 読めない行が在る周は数を出さない（**数えられなかったを 0 に化けさせない**）。
        Err(errors) => Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
        Ok(events) => Outcome::ok(vec![count(&events).line(), spend(state_dir, &events).line()]),
    }
}
