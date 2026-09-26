//! `pipe regate`: 契約の赤でない `Gated` の FAIL を**同じ worktree** で再 gate する口（設計 pipeline.md §49・
//! 契約表の行 ar・`s2-07l.240`）。
//!
//! 器は「契約の赤かどうか」を判定しない——それは裁定であって述語ではない（C5）。口は裁定の逐語を
//! `RunStage`（段 = `Implemented`・`detail` = `regate:` + 逐語・actor は human）の 1 件に記帳するだけで、worktree・
//! commit・判定の file には 1 byte も触らない。段が戻れば既存の列（`resume` 等）が同じ便 id の同じ worktree で
//! gate をもう 1 周撃つ。段の生死は `cli` の `live` の 1 本で読む（第 2 の段の読み手を作らない・C2）。段の種別も
//! event の種別も足さない。

use super::cli::{live, refused};
use super::follow::is_conflict;
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
pub(in crate::pipe) fn regated_since_gate(events: &[Event], id: &str) -> bool {
    let own: Vec<&Event> = events.iter().filter(|event| event.run == id && event.kind == EventKind::RunStage).collect();
    let since = own.iter().rposition(|event| event.stage == Some(Stage::Gated)).map_or(0, |at| at.saturating_add(1));
    own.iter()
        .skip(since)
        .any(|event| event.detail.as_deref().is_some_and(|detail| detail.starts_with(PREFIX)))
}

/// 追随の記帳の `detail` の頭（`follow_step` と着地の追随が書く `rebase:<base>..<main>` の字面の写し）。
const FOLLOWED: &str = "rebase:";

/// 便の**最新の `Gated` の `RunStage` より後ろ**に追随の記帳（`rebase:` か衝突の起こし直し〔[`is_conflict`]〕）が在り、
/// その後ろに `Gated` / `Landed` の `RunStage` が無いか（設計 dispatcher.md §25・形は [`regated_since_gate`] と同じ）。
pub(in crate::pipe) fn followed_since_gate(events: &[Event], id: &str) -> bool {
    let own: Vec<&Event> = events.iter().filter(|event| event.run == id && event.kind == EventKind::RunStage).collect();
    let since = own.iter().rposition(|event| matches!(event.stage, Some(Stage::Gated | Stage::Landed)));
    own.iter().skip(since.map_or(0, |at| at.saturating_add(1))).any(|event| {
        event.detail.as_deref().is_some_and(|detail| detail.starts_with(FOLLOWED) || is_conflict(detail))
    })
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
    use super::{admit, followed_since_gate, regate, regated_since_gate, Ticket, PREFIX};
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

    /// 便 `r1` の `RunStage` を 1 件作る（段と `detail` だけ呼び手が選ぶ）。
    fn stage(stage: Stage, detail: &str) -> crate::fleet::Event {
        event("r1", EventKind::RunStage, Some(stage), None, Some(detail))
    }

    /// (§25 形 1) 最新の `Gated` の後ろの追随（`rebase:` と衝突の起こし直しの 2 語）は数える。
    #[test]
    fn followed_since_gate_counts_a_follow_after_the_latest_gated() {
        for detail in ["rebase:a..b", "rebase-conflict:a..b", "rebase-stale-rows:a..b"] {
            let events = [stage(Stage::Gated, "verdict:FAIL"), stage(Stage::Gated, "verdict:PASS"), stage(Stage::Implemented, detail)];
            assert!(followed_since_gate(&events, "r1"), "{detail}: 追随した");
        }
    }

    /// (§25 形 3) 追随の記帳の無い `Implemented`（regate・同一変更の終端・他の便の追随）は数えない。
    #[test]
    fn followed_since_gate_ignores_runs_without_a_follow() {
        let other = event("r2", EventKind::RunStage, Some(Stage::Implemented), None, Some("rebase:a..b"));
        for tail in [stage(Stage::Implemented, "regate:x"), stage(Stage::Implemented, "rebase-empty:a..b"), other] {
            assert!(!followed_since_gate(&[stage(Stage::Gated, "verdict:PASS"), tail.clone()], "r1"), "{:?}", tail.detail);
        }
    }

    /// (§25 形 3) 追随の後ろに `Gated` / `Landed` を経た便は数えない（最新の `Gated` より前の追随は開かない）。
    #[test]
    fn followed_since_gate_closes_after_a_later_gated_or_landed() {
        for later in [Stage::Gated, Stage::Landed] {
            let events = [stage(Stage::Gated, "verdict:PASS"), stage(Stage::Implemented, "rebase:a..b"), stage(later, "x")];
            assert!(!followed_since_gate(&events, "r1"), "{later:?} の後ろに追随は無い");
        }
    }

    /// 形ごとの置き場を 1 つ作り、判定 FAIL の `Gated` の便 `r1` を置いてから `shape` で 1 条件だけ崩す。
    fn form_place(name: &str, shape: impl FnOnce(&std::path::Path)) -> std::path::PathBuf {
        let root = scratch(&format!("regate-form-{name}"));
        let state = root.join("state");
        gated_run(&state, &root.join("repo"), "r1", "FAIL");
        shape(&state);
        root
    }

    /// 形ごとに 1 条件だけ崩す手。
    type Shape = Box<dyn FnOnce(&std::path::Path)>;

    /// 札の本文を置く（`pid` の行）。
    fn put_ticket(state: &std::path::Path, body: &str) {
        let _ = std::fs::write(super::super::driver_path(state, "r1"), body);
    }

    // flip-check: retroactive s2-07l.593
    /// (§23 (i)) 口の本体に通る 2 形（札なし・札の所有者が死んでいる）と断る 6 形（母集団 8）を 1 形ずつ別の置き場で
    /// 渡す: 通る形は rc 0 で記帳ちょうど 1 件、断る形は rc 1 で記帳 0 件・理由の語が形ごとに違う（語は入力の逐語に
    /// 無い字面で測る）。
    #[test]
    fn pipe_regate_forms_body_passes_two_and_refuses_six_with_distinct_reasons() {
        let dead = std::process::Command::new("true").spawn().and_then(|mut child| child.wait().map(|_| child.id()));
        let dead = dead.expect("true を起こして待てる");
        let words = "裁定: 器の欠陥";
        let forms: [(&str, Option<&str>, &str, Shape); 8] = [
            ("absent", None, words, Box::new(|_| {})),
            ("dead", None, words, Box::new(move |state| put_ticket(state, &format!("{dead}\n")))),
            ("stage", Some("戻せるのは"), words, Box::new(|state| append_all(state, &[event("r1", EventKind::RunStage, Some(Stage::Implemented), None, None)]))),
            ("verdict", Some("FAIL でない"), words, Box::new(|state| { let _ = std::fs::write(super::super::verdict_path(state, "r1"), "{\"verdict\":\"PASS\"}\n"); })),
            ("live", Some("運転手が生きている"), words, Box::new(|state| put_ticket(state, &format!("{}\n", std::process::id())))),
            ("empty", Some("逐語が空"), "  ", Box::new(|_| {})),
            ("judged", Some("判定を読めない"), words, Box::new(|state| { let _ = std::fs::write(super::super::verdict_path(state, "r1"), "not json\n"); })),
            ("ticket", Some("札を読めない"), words, Box::new(|state| put_ticket(state, "not-a-pid\n"))),
        ];
        let policy = LockPolicy::embedded().expect("埋め込みの lock 規則");
        let total = forms.len();
        let mut seen: Vec<String> = Vec::new();
        for (name, why, reason, shape) in forms {
            let root = form_place(name, shape);
            let state = root.join("state");
            let before = store::read_all(&state).expect("置き場を読める").len();
            let out = regate(&state, "r1", reason, policy);
            let written = store::read_all(&state).expect("置き場を読める").len() - before;
            let told = out.err.join("\n");
            match why {
                None => assert_eq!((out.rc, written), (RC_OK, 1), "{name}: 通る形は記帳 1 件（{told}）"),
                Some(word) => {
                    assert_eq!((out.rc, written), (RC_REFUSED, 0), "{name}: 断る形は記帳 0 件（{told}）");
                    assert!(told.contains(word) && !reason.contains(word), "{name}: 理由の語 {word:?}（{told}）");
                    assert!(!seen.iter().any(|other| told.contains(other.as_str())), "{name}: 語は形ごとに違う（{told}）");
                    seen.push(word.to_owned());
                }
            }
            let _ = std::fs::remove_dir_all(&root);
        }
        assert_eq!((total, seen.len()), (8, 6), "母集団 8 のうち断る形 6");
    }

    /// (§23 (j)) 通した直後に同じ便へ撃つ 2 度目は、受付の段の条件の語で断り「1 度戻している」の語を持たない
    /// （AC47「理由 = 段」・受付の 4 条件が先）。
    #[test]
    fn pipe_regate_forms_second_right_after_is_refused_by_the_stage() {
        let root = form_place("second", |_| {});
        let state = root.join("state");
        let policy = LockPolicy::embedded().expect("埋め込みの lock 規則");
        assert_eq!(regate(&state, "r1", "一度目", policy).rc, RC_OK, "1 度目は通る");
        let again = regate(&state, "r1", "二度目", policy);
        let told = again.err.join("\n");
        assert_eq!(again.rc, RC_REFUSED, "2 度目は断る（{told}）");
        assert!(told.contains("戻せるのは") && !told.contains("1 度戻している"), "理由は段（{told}）");
        let _ = std::fs::remove_dir_all(&root);
    }
}
