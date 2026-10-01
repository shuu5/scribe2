//! 行の予約（設計 docs/design/row-review.md §7・契約表の行 f・FR102・rules 行 `pipe.reserve_h`）。
//!
//! 落ちた契約 B（直前の便が Landed でない終端）の直前の便の契約の写しの write-set を B の行の予約とし、順序で B より後ろに並ぶ
//! 交差する候補を [`Held`]（待ちの理由 `reserved`）で待たせる。**記帳しない**: 列の周ごとに、周が既に 1 回読んだ台帳と event log と
//! 便の置き場の契約 file と rules 行から導く（新しい event kind を足さない・C17）。解ける契機（B の新しい便の `RunCreated`・
//! B の close・B の memo か問いの label・B への hold の印・期限）は導きの条件の側が持つので、解けの口は別に持たない。

use super::candidates::fallen;
use super::{key_of, Candidate, Input, BLOCKS, OPEN};
use crate::fleet::{epoch_of, replay, Event, Mark, Stage};
use crate::ledger::form::{is_memo, is_question};
use crate::pipe::contract::Contract;
use crate::pipe::contract_path;
use crate::pipe::refuse::overlaps;
use crate::rules::int_row;
use crate::seat::ledger::Issue;
use std::collections::{BTreeMap, BTreeSet};

/// 期限の rules 行の id（時間・値 0 は期限なし）。
const ROW: &str = "pipe.reserve_h";

/// 待ちの理由 `reserved` の値（`dispatch ls` は `reserved:<by>/<files>`・期限の行を読めない周は末尾に `/unset`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// 予約を持つ落ちた契約 B の bead id。
    pub by: String,
    /// 候補の write-set と B の行の予約が交差した file の本数（受付の交差と同じ [`overlaps`] の対の数）。
    pub files: usize,
    /// 期限の行（`pipe.reserve_h`）を読めない周か（読めない周は期限なしで予約を掛ける・測れない期限を掛けないに畳まない・C10）。
    pub unset: bool,
}

impl Held {
    /// 理由の値の字面（`<by>/<files>`・読めない周は末尾に `/unset`）。
    pub fn value(&self) -> String {
        format!("{}/{}{}", self.by, self.files, if self.unset { "/unset" } else { "" })
    }
}

/// 行の予約を持つ契約 1 つ（[`derive`] が 1 周に 1 回導く）。
pub(super) struct Reservation {
    /// 順序の鍵（候補と同じ [`key_of`]・B が列の候補に居ない周も台帳の priority と印で組む）。
    key: (u8, u64, Vec<u64>, String),
    /// B の bead id。
    pub(super) bead: String,
    /// B の直前の便の契約の写しの write-set。
    write_set: Vec<String>,
    /// 期限を測れない周か。
    pub(super) unset: bool,
    /// B が台帳の blocks で待つ祖先（推移・待たせない）。
    pub(super) ancestors: BTreeSet<String>,
    /// B の直前の便の run id（置き場の replay の同じ bead の最後）。
    pub(super) run: String,
    /// その便の段（replay が見た最新）。
    pub(super) stage: Stage,
    /// 終端の段の event の ts の秒（読めない周は無い）。
    pub(super) since: Option<u64>,
}

/// 行の予約を持つ契約を導く（並びの前の B から）。
///
/// B の条件は FR102: 台帳で open・memo の label も問いの label も持たない・最新の介入の印が hold でない・直前の便（置き場の replay の
/// 同じ bead の run id の昇順の最後）が Landed でない終端で、その後の便が無い（後の便は最後の便になる）・期限の内・契約の写しを読める。
pub(super) fn derive(input: &Input<'_>, issues: &[Issue], marks: &BTreeMap<String, (Mark, String)>, events: &[Event], now: u64) -> Vec<Reservation> {
    let state = replay(events);
    let (hours, row) = match int_row(input.manifest, ROW) {
        Ok(found) => (found, true),
        Err(_) => (0, false),
    };
    let mut found: Vec<Reservation> = issues
        .iter()
        .filter(|issue| issue.status == OPEN && !is_memo(issue) && !is_question(issue))
        .filter(|issue| !matches!(marks.get(&issue.id), Some((Mark::Hold, _))))
        .filter_map(|issue| {
            let (id, run) = state.runs.iter().rev().find(|(_, run)| run.bead == issue.id)?;
            if !fallen(input.state_dir, id, run.stage) {
                return None;
            }
            let at = events.iter().rev().find(|event| event.run == *id && event.stage == Some(run.stage)).map_or(run.updated.as_str(), |event| event.ts.as_str());
            let since = epoch_of(at);
            if hours > 0 && since.is_some_and(|since| now.saturating_sub(since) >= hours.saturating_mul(3_600)) {
                return None;
            }
            let contract = Contract::load(&contract_path(input.state_dir, id)).ok()?;
            let candidate = Candidate { bead: issue.id.clone(), priority: issue.priority, mark: marks.get(&issue.id).map(|(mark, _)| *mark), reason: None };
            Some(Reservation {
                key: key_of(&candidate),
                bead: issue.id.clone(),
                write_set: contract.write_set,
                unset: !row || (hours > 0 && since.is_none()),
                ancestors: ancestors_of(issues, &issue.id),
                run: id.clone(),
                stage: run.stage,
                since,
            })
        })
        .collect();
    found.sort_by(|left, right| left.key.cmp(&right.key));
    found
}

/// `start` が台帳の blocks で待つ祖先の集合（推移）。
fn ancestors_of(issues: &[Issue], start: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![start.to_owned()];
    while let Some(id) = stack.pop() {
        for dep in issues.iter().filter(|issue| issue.id == id).flat_map(|issue| &issue.deps).filter(|dep| dep.kind == BLOCKS) {
            if seen.insert(dep.on.clone()) {
                stack.push(dep.on.clone());
            }
        }
    }
    seen
}

/// 候補を待たせる行の予約（無ければ `None`）。順序で B より後ろ（鍵が大きい）・B 自身と B の祖先でない・write-set が交差する B の
/// うち並びの前の 1 つを名指す。交差は受付と同じ [`overlaps`]（tracked は同じ周の材料）。
pub(super) fn held(reserved: &[Reservation], candidate: &Candidate, write_set: &[String], tracked: &[String]) -> Option<Held> {
    let mine = key_of(candidate);
    reserved.iter().filter(|found| found.key < mine && found.bead != candidate.bead && !found.ancestors.contains(&candidate.bead)).find_map(|found| {
        let files = overlaps(write_set, &found.write_set, tracked).len();
        (files > 0).then(|| Held { by: found.bead.clone(), files, unset: found.unset })
    })
}
