//! `pipe follow`: gate の段の便が base を追随する口（設計 pipeline.md §52・契約表の行 au・`s2-07l.470`）。
//!
//! 動かすのは**便の木だけ**である。木を main の先端へ載せ替え、段を `Implemented` へ戻す `RunStage` を 1 件書く
//! （`detail` は着地の追随と同じ字面 `rebase:<base>..<main>`＝base の読み手 [`super::base_of_run`] が新しい側を読む）。
//! gate は撃たない・main は 1 byte も動かさない・PR も押さない・runner も起こさない。衝突した周は木を撃つ前の姿へ
//! 戻して断る（着地側の起こし直しと上限は通らない）。段の種別も event の種別も足さない。
//!
//! 木の載せ替えの 1 段（[`rebase`]）は着地の追随（`land.rs` の `rebase_onto`）と共有する 1 本である（C3.4）。

use super::cli::{live, refused};
use super::follow::Ancestry;
use super::land::{WorktreeCheck, MAIN_REF};
use super::{base_of_run, driver_ticket, emit, git_line, git_ok, repo_of_run, worktree_path, Base, Emit, Ticket};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::store::{self, LockPolicy, StoreError};
use crate::fleet::{replay, EventKind, Stage};
use std::path::Path;

/// 木の載せ替えの 1 段: 便が `base` の上に積んだ commit だけを `main` の上へ運ぶ（`git rebase --onto <main> <base>`）。
///
/// **木の branch だけ**が動き、main は動かさない（force 系は使わない・N1）。衝突の後始末は呼び手が持つ（着地は
/// 起こし直し・この口は中止して断る）＝ここは撃って通ったかだけを返す。
pub(in crate::pipe) fn rebase(worktree: &Path, base: &str, main: &str) -> bool {
    git_ok(worktree, &["rebase", "--onto", main, base])
}

/// 受付の材料（設計 §52 形 2 の 5 条件を測った値）。
struct Facts {
    /// 便が在るか。
    exists: bool,
    /// 段が生きているか（`live` の答え・`None` は測れない）。
    live: Option<bool>,
    /// 運転手の札。
    ticket: Ticket,
    /// 木の状態。
    tree: WorktreeCheck,
    /// 記録した base と main の関係（base か main を読めない周は `None`）。
    ancestry: Option<Ancestry>,
}

/// 受付の 5 条件（設計 §52 形 2）: 便が在る ∧ 段が終端でない ∧ 運転手の札が無いか所有者が死んでいる ∧ 木が clean ∧
/// base が main の祖先。外れた最初の条件を理由 1 行で返す（**測れない周も断る**・fail-closed）。
fn admit(facts: &Facts) -> Result<(), String> {
    if !facts.exists {
        return Err("便が無い".to_owned());
    }
    match facts.live {
        Some(true) => {}
        Some(false) => return Err("段が終端である".to_owned()),
        None => return Err("段を測れない".to_owned()),
    }
    match facts.ticket {
        Ticket::Absent | Ticket::Dead => {}
        Ticket::Live => return Err("運転手が生きている".to_owned()),
        Ticket::Unreadable => return Err("運転手の札を読めない".to_owned()),
    }
    if !facts.tree.is_clean() {
        return Err(format!("木が clean でない（{}）", facts.tree.as_str()));
    }
    match facts.ancestry {
        Some(Ancestry::Ancestor) => Ok(()),
        Some(Ancestry::Diverged(_) | Ancestry::Unrelated) => Err("base が main の祖先でない".to_owned()),
        None => Err("base か main を読めない".to_owned()),
    }
}

/// `pipe follow --run <id>`: 受付を通った周だけ木を main の先端へ載せ替え、`RunStage stage=Implemented
/// detail=rebase:<base>..<main>` を 1 件書き、`follow: run=<id> rebase=<base>..<main>` の 1 行で rc 0。
/// 外れた周・衝突した周は理由 1 行で rc 1 で**何も書かない**（衝突は木を撃つ前の先端へ戻す）。
pub(in crate::pipe) fn follow(state_dir: &Path, id: &str, policy: LockPolicy) -> Outcome {
    let events = match store::read_all(state_dir) {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
    };
    let state = replay(&events);
    let run = state.runs.get(id);
    let repo = repo_of_run(state_dir, id);
    let worktree = repo.as_deref().map(|found| worktree_path(found, id));
    let base = match base_of_run(state_dir, id) {
        Base::Known(found) => Some(found),
        Base::Absent | Base::Unreadable => None,
    };
    let main = repo.as_deref().and_then(|found| git_line(found, &["rev-parse", MAIN_REF]));
    let facts = Facts {
        exists: run.is_some() && worktree.is_some(),
        live: run.and_then(|found| live(state_dir, id, found.stage)),
        ticket: driver_ticket(state_dir, id),
        tree: worktree.as_deref().map_or(WorktreeCheck::Unreadable, WorktreeCheck::judge),
        ancestry: match (repo.as_deref(), base.as_deref(), main.as_deref()) {
            (Some(root), Some(from), Some(to)) => Some(Ancestry::judge(root, from, to)),
            _ => None,
        },
    };
    if let Err(why) = admit(&facts) {
        return refused(format!("follow: run {id} を追随しない（{why}）"));
    }
    let (Some(found), Some(tree), Some(base), Some(main)) = (run, worktree, base, main) else {
        return refused(format!("follow: run {id} を追随しない（材料を解けない）"));
    };
    let Some(before) = git_line(&tree, &["rev-parse", "HEAD"]) else {
        return refused(format!("follow: run {id} の木の先端を読めない"));
    };
    if !rebase(&tree, &base, &main) {
        // 中止して撃つ前の先端へ戻す（着地の起こし直しと上限は通らない・設計 §52 形 4）。
        let aborted = git_ok(&tree, &["rebase", "--abort"]);
        let restored = git_line(&tree, &["rev-parse", "HEAD"]).is_some_and(|head| head == before);
        if !(aborted && restored) {
            return Outcome::failed_line(
                RC_BROKEN,
                format!("pipe: follow: run {id} の載せ替えが衝突し、木を {before} へ戻せなかった"),
            );
        }
        return refused(format!("follow: run {id} の載せ替えが衝突した（base={base} main={main}・木は {before} のまま）"));
    }
    let recorded = emit(
        state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: id,
            bead: &found.bead,
            stage: Some(Stage::Implemented),
            seat: None,
            pid: None,
            // 着地の追随と同じ字面（`land.rs` の `follow_main`）。
            detail: Some(format!("rebase:{base}..{main}")),
        },
        policy,
    );
    match recorded {
        Err(err) => Outcome::failed_line(RC_BROKEN, format!("pipe: {err}")),
        Ok(()) => Outcome::ok_line(format!("follow: run={id} rebase={base}..{main}")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixture::{append_all, event, scratch};
    use super::super::land::WorktreeCheck;
    use super::super::{base_of_run, branch_name, repo_path, run_dir, verdict_path, worktree_path, Base, Ticket};
    use super::{admit, follow, Ancestry, Facts};
    use crate::cli_outcome::{RC_OK, RC_REFUSED};
    use crate::fleet::store::{self, LockPolicy};
    use crate::fleet::{EventKind, Stage};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// git を 1 回撃ち、stdout の 1 行を返す（失敗は読み手の assert が落とす）。
    fn git(dir: &Path, args: &[&str]) -> String {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .unwrap_or_default()
    }

    /// main に 1 file の commit を積み、進んだ sha を返す。
    fn commit(dir: &Path, path: &str, body: &str) -> String {
        let _ = std::fs::write(dir.join(path), body);
        git(dir, &["add", path]);
        git(dir, &["commit", "-q", "-m", path]);
        git(dir, &["rev-parse", "HEAD"])
    }

    /// base の上に 1 commit を積んだ便の木と、その後に進んだ main（`(root, repo, state, base, main)`）。
    /// 便は `Gated` の PASS で、`Spawned` の `base:<sha>` を持つ。
    fn gated_behind(name: &str, main_file: &str) -> (PathBuf, PathBuf, PathBuf, String, String) {
        let root = scratch(name);
        let (repo, state) = (root.join("repo"), root.join("state"));
        let _ = std::fs::create_dir_all(&repo);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "mutant"]);
        git(&repo, &["config", "user.email", "mutant@example.invalid"]);
        let base = commit(&repo, "seed.txt", "seed\n");
        let tree = worktree_path(&repo, "r1");
        git(&repo, &["worktree", "add", "-q", "-b", &branch_name("r1"), &tree.display().to_string(), &base]);
        commit(&tree, "run.txt", "run\n");
        let main = commit(&repo, main_file, "main\n");
        let spawned = format!("base:{base}");
        append_all(
            &state,
            &[
                event("r1", EventKind::RunStage, Some(Stage::Spawned), None, Some(&spawned)),
                event("r1", EventKind::RunStage, Some(Stage::Implemented), None, None),
                event("r1", EventKind::RunStage, Some(Stage::Gated), None, Some("verdict:PASS")),
            ],
        );
        let _ = std::fs::create_dir_all(run_dir(&state, "r1"));
        let _ = std::fs::write(repo_path(&state, "r1"), format!("{}\n", repo.display()));
        let _ = std::fs::write(verdict_path(&state, "r1"), "{\"verdict\":\"PASS\"}\n");
        (root, repo, state, base, main)
    }

    /// (a) 形 2 の核: 5 条件を 1 つずつ外した 5 形と全部満たす 1 形（母集団 6）のうち通るのは 1 形だけで、
    /// 段を外す形は `live` が `None`（測れない）の周である。
    #[test]
    fn pipe_follow_step_admit_passes_exactly_one_of_six_forms() {
        let all = || Facts {
            exists: true,
            live: Some(true),
            ticket: Ticket::Absent,
            tree: WorktreeCheck::Clean,
            ancestry: Some(Ancestry::Ancestor),
        };
        let forms = [
            ("all", all()),
            ("exists", Facts { exists: false, ..all() }),
            ("stage", Facts { live: None, ..all() }),
            ("driver", Facts { ticket: Ticket::Live, ..all() }),
            ("tree", Facts { tree: WorktreeCheck::Dirty, ..all() }),
            ("ancestor", Facts { ancestry: Some(Ancestry::Diverged("m".to_owned())), ..all() }),
        ];
        let passed: Vec<&str> = forms.iter().filter(|(_, facts)| admit(facts).is_ok()).map(|(name, _)| *name).collect();
        assert_eq!((forms.len(), passed), (6, vec!["all"]), "母集団 6 のうち通るのは全部満たす 1 形だけ");
        // 外す側の残りの値も断る・札の死んだ便は通る。
        assert!(admit(&Facts { live: Some(false), ..all() }).is_err(), "終端の段");
        assert!(admit(&Facts { ticket: Ticket::Unreadable, ..all() }).is_err(), "札を読めない");
        assert!(admit(&Facts { tree: WorktreeCheck::Unreadable, ..all() }).is_err(), "木を読めない");
        assert!(admit(&Facts { ancestry: Some(Ancestry::Unrelated), ..all() }).is_err(), "無関係な歴史");
        assert!(admit(&Facts { ancestry: None, ..all() }).is_err(), "base を読めない");
        assert!(admit(&Facts { ticket: Ticket::Dead, ..all() }).is_ok(), "札の所有者が死んでいる");
    }

    /// (b) 形 3: 通った周は `RunStage` を**ちょうど 1 件**書き、段は `Implemented`・`detail` は `rebase:` の後ろに
    /// 撃つ前の base と main の 2 sha。木の base は main の先端になり、main の sha は 1 字も変わらない。
    #[test]
    fn pipe_follow_step_writes_one_rebase_event_and_keeps_main() {
        let (root, repo, state, base, main) = gated_behind("follow-one", "main.txt");
        let tree = worktree_path(&repo, "r1");
        let before = store::read_all(&state).expect("置き場を読める").len();
        let policy = LockPolicy::embedded().expect("埋め込みの lock 規則");
        let out = follow(&state, "r1", policy);
        assert_eq!(out.rc, RC_OK, "通る周: {:?}", out.err);
        assert_eq!(out.out, vec![format!("follow: run=r1 rebase={base}..{main}")]);
        let events = store::read_all(&state).expect("置き場を読める");
        assert_eq!(events.len(), before + 1, "書くのはちょうど 1 件");
        let last = events.last().expect("1 件在る");
        assert_eq!((last.kind, last.stage), (EventKind::RunStage, Some(Stage::Implemented)));
        assert_eq!(last.detail.as_deref(), Some(format!("rebase:{base}..{main}").as_str()), "着地の追随と同じ字面");
        assert_eq!(base_of_run(&state, "r1"), Base::Known(main.clone()), "base の読み手は新しい側を読む");
        assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main, "main は動かない");
        assert_eq!(git(&tree, &["rev-parse", "HEAD~1"]), main, "木の base は main の先端");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// (c) 形 4: 載せ替えが衝突した周は rc 1 で、木の先端は撃つ前と同じ sha に戻り、events は 1 件も増えない
    /// （着地側の `rebase-conflict:` の起こし直しの記帳も無い）。
    #[test]
    fn pipe_follow_step_conflict_restores_tree_and_writes_nothing() {
        let (root, repo, state, _, main) = gated_behind("follow-conflict", "run.txt");
        let tree = worktree_path(&repo, "r1");
        let head = git(&tree, &["rev-parse", "HEAD"]);
        let before = store::read_all(&state).expect("置き場を読める");
        let policy = LockPolicy::embedded().expect("埋め込みの lock 規則");
        let out = follow(&state, "r1", policy);
        assert_eq!(out.rc, RC_REFUSED, "衝突は rc 1: {:?}", out.err);
        assert!(out.err.iter().any(|line| line.contains("衝突")), "理由 1 行: {:?}", out.err);
        assert_eq!(git(&tree, &["rev-parse", "HEAD"]), head, "木の先端は撃つ前のまま");
        assert_eq!(git(&tree, &["status", "--porcelain"]), "", "木は clean（rebase の途中で残さない）");
        assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main, "main は動かない");
        let after = store::read_all(&state).expect("置き場を読める");
        assert_eq!(after.len(), before.len(), "events は 1 件も増えない");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 形 2 の実物: 木が汚れた周・判定 FAIL の `Gated`（終端）の周は rc 1 で何も書かず、木も動かない。
    #[test]
    fn pipe_follow_step_refuses_dirty_tree_and_terminal_stage() {
        let (root, repo, state, _, _) = gated_behind("follow-refuse", "main.txt");
        let tree = worktree_path(&repo, "r1");
        let head = git(&tree, &["rev-parse", "HEAD"]);
        let policy = LockPolicy::embedded().expect("埋め込みの lock 規則");
        let count = || store::read_all(&state).map(|events| events.len()).unwrap_or(0);
        let before = count();
        let _ = std::fs::write(tree.join("dirty.txt"), "x\n");
        assert_eq!(follow(&state, "r1", policy).rc, RC_REFUSED, "汚れた木");
        let _ = std::fs::remove_file(tree.join("dirty.txt"));
        let _ = std::fs::write(verdict_path(&state, "r1"), "{\"verdict\":\"FAIL\"}\n");
        assert_eq!(follow(&state, "r1", policy).rc, RC_REFUSED, "判定 FAIL の Gated は終端");
        assert_eq!(follow(&state, "nope", policy).rc, RC_REFUSED, "便が無い");
        assert_eq!((count(), git(&tree, &["rev-parse", "HEAD"])), (before, head), "何も書かず木も動かない");
        let _ = std::fs::remove_dir_all(&root);
    }
}
