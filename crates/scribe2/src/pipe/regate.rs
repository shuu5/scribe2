//! `pipe regate`: 契約の赤でない `Gated` の FAIL を**同じ worktree** で再 gate する口（設計 pipeline.md §49・
//! 契約表の行 ar・`s2-07l.240`）。
//!
//! 器は「契約の赤かどうか」を判定しない——それは裁定であって述語ではない（C5）。口は裁定の逐語を
//! `RunStage`（段 = `Implemented`・`detail` = `regate:` + 逐語・actor は human）の 1 件に記帳するだけで、worktree・
//! commit・判定の file には 1 byte も触らない。段が戻れば既存の列（`resume` 等）が同じ便 id の同じ worktree で
//! gate をもう 1 周撃つ。段の生死は `cli` の `live` の 1 本で読む（第 2 の段の読み手を作らない・C2）。段の種別も
//! event の種別も足さない。

use super::cli::{live, refused};
use super::{driver_ticket, Ticket};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::store::{self, Condition, LockPolicy, StoreError};
use crate::fleet::{self, replay, Event, EventKind, Stage, ACTOR_HUMAN, SCHEMA};
use std::path::Path;

/// 裁定の逐語を受ける flag（`pipe regate` の flag の表と読み手の同じ 1 つ）。
pub(in crate::pipe) const REASON_FLAG: &str = "--reason";

/// 戻しの記帳の `detail` の頭（後ろは裁定の逐語そのもの）。
const PREFIX: &str = "regate:";

/// 受付の 4 条件（設計 §49 形 2）: 段が `Gated` ∧ 判定が FAIL（`live` が `Some(false)`）∧ 運転手の札が無いか
/// 所有者が死んでいる ∧ 逐語が非空。外れた最初の条件を理由 1 行で返す（**測れない `None` も断る**・fail-closed）。
fn admit(stage: Stage, judged: Option<bool>, ticket: Ticket, reason: &str) -> Result<(), String> {
    if stage != Stage::Gated {
        return Err(format!("段は {} である（戻せるのは {} だけ）", stage.as_str(), Stage::Gated.as_str()));
    }
    match judged {
        Some(false) => {}
        Some(true) => return Err("判定が FAIL でない".to_owned()),
        None => return Err("判定を読めない".to_owned()),
    }
    match ticket {
        Ticket::Absent | Ticket::Dead => {}
        Ticket::Live => return Err("運転手が生きている".to_owned()),
        Ticket::Unreadable => return Err("運転手の札を読めない".to_owned()),
    }
    match reason.trim().is_empty() {
        true => Err(format!("{REASON_FLAG} の逐語が空である")),
        false => Ok(()),
    }
}

/// 便の**最新の `Gated` の `RunStage` より後ろ**に戻しの記帳が在るか（設計 §49 形 4・1 つの FAIL につき 1 回）。
///
/// 回数の閾値を値で持たない: もう 1 周の gate が `Gated` を書けば次の 1 回が開く。
fn regated_since_gate(events: &[Event], id: &str) -> bool {
    let own: Vec<&Event> = events.iter().filter(|event| event.run == id && event.kind == EventKind::RunStage).collect();
    let since = own.iter().rposition(|event| event.stage == Some(Stage::Gated)).map_or(0, |at| at.saturating_add(1));
    own.iter()
        .skip(since)
        .any(|event| event.detail.as_deref().is_some_and(|detail| detail.starts_with(PREFIX)))
}

/// `pipe regate --run <id> --reason <逐語>`: 受付を通った周だけ `RunStage` を 1 件書き、`regate: run=<id>
/// from=Gated to=Implemented` の 1 行で rc 0。外れた周は理由 1 行で rc 1 で**何も書かない**。
pub(in crate::pipe) fn regate(state_dir: &Path, id: &str, reason: &str, policy: LockPolicy) -> Outcome {
    let events = match store::read_all(state_dir) {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
    };
    let state = replay(&events);
    let Some(run) = state.runs.get(id) else {
        return refused(format!("run {id} が無い"));
    };
    if let Err(why) = admit(run.stage, live(state_dir, id, run.stage), driver_ticket(state_dir, id), reason) {
        return refused(format!("regate: run {id} を戻さない（{why}）"));
    }
    if regated_since_gate(&events, id) {
        return refused(format!("regate: run {id} は最新の Gated の後に 1 度戻している"));
    }
    let event = Event {
        schema: SCHEMA,
        ts: fleet::cli::now_utc(),
        kind: EventKind::RunStage,
        run: id.to_owned(),
        bead: run.bead.clone(),
        host: fleet::cli::host(),
        // 裁定の逐語を運ぶ記帳ゆえ人由来（設計 §49 形 3）。
        actor: ACTOR_HUMAN.to_owned(),
        stage: Some(Stage::Implemented),
        seat: None,
        pid: None,
        detail: Some(format!("{PREFIX}{reason}")),
        allowance: None,
        registration: None,
        mark: None,
        account: None,
        cost: None,
        rule: None,
    };
    // 段を進める記帳と同じ門を通す（止めた便へ書かない・設計 §39）。
    match store::append_if(state_dir, &event, policy, Condition::NotStopped { run: id }) {
        Err(err) => Outcome::failed_line(RC_BROKEN, format!("pipe: {err}")),
        Ok(_) => Outcome::ok_line(format!(
            "regate: run={id} from={} to={}",
            Stage::Gated.as_str(),
            Stage::Implemented.as_str()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixture::{append_all, event, gated_run, scratch};
    use super::{admit, regate, regated_since_gate, Ticket, PREFIX};
    use crate::cli_outcome::{RC_OK, RC_REFUSED};
    use crate::fleet::store::{self, LockPolicy};
    use crate::fleet::{EventKind, Stage, ACTOR_HUMAN};

    /// (a) 形 2 の核: 4 条件を 1 つずつ外した 4 形と全部満たす 1 形（母集団 5）のうち通るのは 1 形だけで、
    /// 判定を外す形は `live` が `None`（測れない）の周である。
    #[test]
    fn pipe_regate_admit_passes_exactly_one_of_five_forms() {
        let forms = [
            ("all", Stage::Gated, Some(false), Ticket::Absent, "器の欠陥"),
            ("stage", Stage::Implemented, Some(false), Ticket::Absent, "器の欠陥"),
            ("verdict", Stage::Gated, None, Ticket::Absent, "器の欠陥"),
            ("driver", Stage::Gated, Some(false), Ticket::Live, "器の欠陥"),
            ("reason", Stage::Gated, Some(false), Ticket::Absent, "  "),
        ];
        let passed: Vec<&str> = forms
            .iter()
            .filter(|(_, stage, judged, ticket, reason)| admit(*stage, *judged, *ticket, reason).is_ok())
            .map(|(name, ..)| *name)
            .collect();
        assert_eq!((forms.len(), passed), (5, vec!["all"]), "母集団 5 のうち通るのは全部満たす 1 形だけ");
        // 外す側の残りの値も断る（判定が FAIL でない・札を読めない）・札の死んだ便は通る。
        assert!(admit(Stage::Gated, Some(true), Ticket::Absent, "x").is_err(), "判定が FAIL でない");
        assert!(admit(Stage::Gated, Some(false), Ticket::Unreadable, "x").is_err(), "札を読めない");
        assert!(admit(Stage::Gated, Some(false), Ticket::Dead, "x").is_ok(), "札の所有者が死んでいる");
    }

    /// (b) 形 3: 通った周は `RunStage` を**ちょうど 1 件**書き、段は `Implemented`・`detail` は `regate:` + 逐語
    /// （入力と別の字面の逐語で出所を弁別する）・actor は human。判定の file と worktree は 1 byte も変わらない。
    #[test]
    fn pipe_regate_writes_one_run_stage_with_the_verbatim_reason() {
        let root = scratch("regate-one");
        let (state, repo) = (root.join("state"), root.join("repo"));
        gated_run(&state, &repo, "r1", "FAIL");
        let worktree = super::super::worktree_path(&repo, "r1");
        std::fs::write(worktree.join("kept.txt"), "tree\n").expect("worktree に file を置ける");
        let verdict = std::fs::read(super::super::verdict_path(&state, "r1")).expect("判定を読める");
        let before = store::read_all(&state).expect("置き場を読める").len();
        let words = "裁定 2026-09-22: 上限で落ちた（契約の赤でない）  ";
        let policy = LockPolicy::embedded().expect("埋め込みの lock 規則");
        let out = regate(&state, "r1", words, policy);
        assert_eq!(out.rc, RC_OK, "通る周: {:?}", out.err);
        assert_eq!(out.out, vec!["regate: run=r1 from=Gated to=Implemented".to_owned()]);
        let events = store::read_all(&state).expect("置き場を読める");
        assert_eq!(events.len(), before + 1, "書くのはちょうど 1 件");
        let last = events.last().expect("1 件在る");
        assert_eq!((last.kind, last.stage), (EventKind::RunStage, Some(Stage::Implemented)));
        assert_eq!(last.detail.as_deref(), Some(format!("{PREFIX}{words}").as_str()), "逐語をそのまま");
        assert_eq!(last.actor, ACTOR_HUMAN);
        assert_eq!(std::fs::read(super::super::verdict_path(&state, "r1")).ok(), Some(verdict), "判定の file は不変");
        assert_eq!(std::fs::read_to_string(worktree.join("kept.txt")).ok().as_deref(), Some("tree\n"), "worktree は不変");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// (c) 形 4: 同じ便への 2 度目は断られ何も書かず、間に `Gated` の `RunStage` を 1 件挟むと次の 1 回が通る。
    /// 最新の `Gated` より後ろの戻しの記帳は、段が `Gated` に見える周でも断る側である。
    #[test]
    fn pipe_regate_refuses_the_second_until_a_new_gated() {
        let root = scratch("regate-twice");
        let (state, repo) = (root.join("state"), root.join("repo"));
        gated_run(&state, &repo, "r1", "FAIL");
        let policy = LockPolicy::embedded().expect("埋め込みの lock 規則");
        let count = || store::read_all(&state).map(|events| events.len()).unwrap_or(0);
        assert_eq!(regate(&state, "r1", "一度目", policy).rc, RC_OK, "1 度目");
        let after_first = count();
        assert_eq!(regate(&state, "r1", "二度目", policy).rc, RC_REFUSED, "2 度目は断る");
        assert_eq!(count(), after_first, "断った周は何も書かない");
        append_all(&state, &[event("r1", EventKind::RunStage, Some(Stage::Gated), None, Some("verdict:FAIL"))]);
        assert_eq!(regate(&state, "r1", "三度目", policy).rc, RC_OK, "Gated を挟めば次の 1 回が通る");
        assert_eq!(count(), after_first + 2);

        let gated = || event("r1", EventKind::RunStage, Some(Stage::Gated), None, Some("verdict:FAIL"));
        let back = || event("r1", EventKind::RunStage, Some(Stage::Implemented), None, Some("regate:x"));
        let other = event("r2", EventKind::RunStage, Some(Stage::Implemented), None, Some("regate:y"));
        assert!(!regated_since_gate(&[gated()], "r1"), "戻していない");
        assert!(regated_since_gate(&[gated(), back()], "r1"), "Gated の後に 1 度戻した");
        assert!(regated_since_gate(&[gated(), back(), event("r1", EventKind::SeatStopped, Some(Stage::Gated), None, None)], "r1"));
        assert!(!regated_since_gate(&[gated(), back(), gated()], "r1"), "Gated を挟めば開く");
        assert!(!regated_since_gate(&[gated(), other], "r1"), "他の便の記帳は数えない");
        let _ = std::fs::remove_dir_all(&root);
    }
}
