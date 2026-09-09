//! `pipe report`（設計 docs/design/pipeline.md §5.8・FR22）。
//!
//! 到達点の主張「人由来の event が **approval 以外に 0 件**」を機械が数えて示す面である
//! （AC1）。人手 0 は「そう見えた」ではなく **log に残った actor** で決める。
//!
//! **便の数と land の数は replay から、人由来の event は生の行から数える**。replay は
//! 便ごとに最後の段しか残さないので、途中に挟まった人の手は replay からは読めない
//! （承認の後に手で段を動かした周が、replay 上は機械だけで進んだ便に見える）。

use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::store::{self, StoreError};
use crate::fleet::{replay, Event, EventKind, Stage, ACTOR_HUMAN};
use std::path::Path;

/// 数えた 4 つ。**合算値だけを出さない**（内訳が到達点の主張そのものである）。
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
}

impl Counted {
    /// stdout の 1 行（設計 §5.8 の字面）。
    pub fn line(self) -> String {
        format!(
            "runs={} landed={} human_events={} human_events_other_than_approval={}",
            self.runs, self.landed, self.human_events, self.human_events_other_than_approval
        )
    }
}

/// event 列から 4 つを数える。
///
/// 承認だけを人の手として許す判定は **kind で行う**（actor の字面ではない）——`actor` は
/// 「誰が起こしたか」、`kind` は「何が起きたか」で、到達点が言う例外は後者だからである。
pub fn count(events: &[Event]) -> Counted {
    let state = replay(events);
    let human: Vec<&Event> = events
        .iter()
        .filter(|event| event.actor == ACTOR_HUMAN)
        .collect();
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
    }
}

/// `pipe report`。読むだけで、event を 1 件も書かない。
pub fn report(state_dir: &Path) -> Outcome {
    match store::read_all(state_dir) {
        // 読めない行が在る周は数を出さない（**数えられなかったを 0 に化けさせない**）。
        Err(errors) => Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
        Ok(events) => Outcome::ok_line(count(&events).line()),
    }
}
