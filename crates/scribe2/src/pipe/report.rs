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

use super::review::{read_detail, FindingKind, FINDING_KINDS};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::store::{self, StoreError};
use crate::fleet::{replay, Event, EventKind, Stage, ACTOR_HUMAN};
use crate::pipe::gate::Verdict;
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
    /// 人由来のうち承認でないものの数（**到達点はここが 0**）。
    pub human_events_other_than_approval: usize,
    /// 審査で PASS にならなかった `Reviewed` の event の数（`by_kind` の母集団・`runs` と同じ範囲で日付では絞らない）。
    pub review_fail: usize,
    /// `review_fail` の理由の型ごとの内訳（[`FINDING_KINDS`] の宣言順・合計は `review_fail`）。
    pub by_kind: [usize; FINDING_KINDS.len()],
}

impl Counted {
    /// stdout の 1 行（設計 §5.8 の字面 + §22 の `review_fail=` / `by_kind=`・既存の token は不変）。
    pub fn line(self) -> String {
        let by_kind: Vec<String> = FINDING_KINDS
            .iter()
            .zip(self.by_kind)
            .map(|(kind, count)| format!("{}:{count}", kind.as_str()))
            .collect();
        format!(
            "runs={} landed={} human_events={} human_events_other_than_approval={} review_fail={} by_kind={}",
            self.runs,
            self.landed,
            self.human_events,
            self.human_events_other_than_approval,
            self.review_fail,
            by_kind.join(",")
        )
    }
}

/// event 列から 6 つを数える。
///
/// 承認だけを人の手として許す判定は **kind で行う**（actor の字面ではない）——`actor` は
/// 「誰が起こしたか」、`kind` は「何が起きたか」で、到達点が言う例外は後者だからである。
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
            .filter(|event| event.kind != EventKind::ApprovalReceived)
            .count(),
        review_fail: by_kind.iter().sum(),
        by_kind,
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

/// `pipe report`。読むだけで、event を 1 件も書かない。
pub fn report(state_dir: &Path) -> Outcome {
    match store::read_all(state_dir) {
        // 読めない行が在る周は数を出さない（**数えられなかったを 0 に化けさせない**）。
        Err(errors) => Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
        Ok(events) => Outcome::ok_line(count(&events).line()),
    }
}
