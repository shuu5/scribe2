//! land（設計 docs/design/pipeline.md §5.4・FR10 / FR11 / FR12・憲法 N1）。
//!
//! **PASS が無ければ 1 byte も動かさない**。前提は Gated ∧ `verdict.json` が PASS ∧
//! `refs/heads/main` が便の記録した base と同一（CAS の old）で、どれか 1 つでも欠けたら
//! rc 1 で何もしない。
//!
//! main を進めた後の実測が赤でも **auto revert はしない**（MVP 外）。main は進んだまま
//! `Failed detail=main-red` を残して loud に落ちる——黙って巻き戻すと「何が起きたか」が
//! 履歴から消え、赤い main が緑に見える瞬間が生まれるためである。
//!
//! 後始末は **可逆な move**（N1.2）。worktree は `retired/<run>` へ移すだけで削除せず、
//! branch も消さない（squash commit は branch の祖先でないので `-d` は通らず、`-D` は
//! N1 が禁じる形である）。
//!
//! main を進めた後は **anchor（`--repo` の checkout）の index と working tree も新 main に揃える**
//! （`s2-07l.120`・N1）。`update-ref` は ref しか動かさないので、揃えないと anchor の `git status` に
//! landed 変更が staged の逆向きで残り、次の `commit -a` が landed 変更を打ち消す（`.117` 実測）。
//! 揃えるのは HEAD が main を指し tracked な未 commit の変更が無く、landed tree が足す path が anchor に
//! 無い周だけ（[`AnchorPlan`]・fail-closed）。**main の実測が赤 / 測れない周も揃える**（ref は既に
//! 進んでいる＝揃えないと同じ経路が failure exit で開く・lens-120 H1）。
//!
//! **main が動いた便は追随する**（`s2-07l.119`・FR30）。記録した base が main の祖先のまま
//! 置き去りになった周は、worktree の branch を main へ rebase し（効くのは branch だけ・
//! main は 1 byte も動かさない・force 系は使わない）、段を `Implemented` へ戻して gate を
//! **同じ関数で**撃ち直し、PASS なら新しい base で CAS する。衝突は木を戻して
//! `Failed detail=rebase-conflict`（終端・fail-closed）。

use crate::polarity::{OnFailure, Polarity, Timing};
use super::contract::Contract;
use super::declaration::Effective;
use super::gate::{gate, is_unreadable, run_checks, Checks, Gate, Limits, Verdict};
use super::{
    emit, git_bytes, git_line, git_ok, verdict_path, worktree_path, worktrees_dir, Emit,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{self, append_line, LockPolicy};
use crate::fleet::{cli::now_utc, EventKind, Stage, SCHEMA};
use std::path::{Path, PathBuf};

/// 進める ref。設計 §5.4 が名指す 1 本である。
const MAIN_REF: &str = "refs/heads/main";

/// 面 5 の export 先の file 名（ADR-0004 §2.2・**版番号に依らず固定**）。
const VERDICTS_FILE: &str = "verdicts.jsonl";

/// land 済み worktree を寄せる dir 名。
const RETIRED_DIR: &str = "retired";

/// anchor を揃えない理由（判定行 `anchor=skipped:<reason>`・**閉じた enum**・憲法 C11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnchorSkip {
    /// HEAD が main を指さない（別 branch・detached）。通常形ゆえ warning は出さない。
    NotMain,
    /// tracked な未 commit の変更が在る（成果を消さない・N1）。
    Dirty,
    /// landed tree が**足す** path が anchor の working tree に既に在る（untracked・ignored を含む）。
    /// `read-tree -m -u` は ignored な file を黙って上書きする（実測 2026-09-12・lens-120 M1）ので、
    /// 足す path の存在を先に見て 1 file も触らない。
    Collision,
    /// anchor の状態を読めない（読めないを clean に読み替えない）。
    Unreadable,
    /// 揃える git が途中で断った（index.lock 等）。**部分的に更新されている可能性がある**。
    SyncFailed,
}

impl AnchorSkip {
    /// 判定行の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::NotMain => "not-main",
            Self::Dirty => "dirty",
            Self::Collision => "collision",
            Self::Unreadable => "unreadable",
            Self::SyncFailed => "sync-failed",
        }
    }
}

/// main 実測用の tmp worktree を置く dir 名。
///
/// **`std::env::temp_dir` を使わない**（`TMPDIR` を読む＝憲法 C2.2 に反する）。置き場は
/// 便の worktree と同じ repo 配下から導く。run id は `<bead>-<stamp>` なのでこの名と
/// 衝突しない。
const CHECK_DIR: &str = "verify";

/// この境界の極性（`MainCheck`）: main を進めた後に実測し、測れなかった周は `Failed detail=main-unmeasured` で止める（緑に化けさせない）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::PostHoc,
    on_failure: OnFailure::FailClosed,
};

/// main 実測の結果。**「赤かった」と「測れなかった」を混ぜない**。
///
/// gate が「測れなかったを通ったに化けさせない」と決めているのと同じ理由で、land も
/// 「測れなかった」を「赤かった」に化けさせない。実測を 1 行も撃てていないのに
/// `main-red` を記帳すると、event log が事実と違うものを述べる。
enum MainCheck {
    /// verify 全行が rc 0。
    Green,
    /// 1 行以上が rc≠0（**実測した上での赤**）。
    Red(String),
    /// 実測そのものができなかった（tmp worktree を切れない等）。
    Unmeasurable(String),
}

/// retire 1 回の材料（`--pr-cmd` 形の便を merge の後に畳む口・設計 §5.4）。
///
/// **契約を要らない**のが land との違いである。畳むのは worktree という入れ物だけで、
/// 契約の verify も write-set も読まない——読む理由が無い面を材料に数えると、契約が
/// 壊れた便の worktree が永久に畳めなくなる。
pub struct Retire<'a> {
    /// 便 id。
    pub run: &'a str,
    /// 契約の bead id。
    pub bead: &'a str,
    /// 対象 repo。
    pub repo: &'a Path,
    /// 置き場。
    pub state_dir: &'a Path,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// land 1 回の材料。
pub struct Land<'a> {
    /// 便 id。
    pub run: &'a str,
    /// 契約の bead id。
    pub bead: &'a str,
    /// 対象 repo。
    pub repo: &'a Path,
    /// 置き場。
    pub state_dir: &'a Path,
    /// 読み込み済みの契約。
    pub contract: &'a Contract,
    /// PR を作る seam（`--pr-cmd`）。`None` なら squash して main を進める。
    pub pr_cmd: Option<&'a str>,
    /// main が動いた便の追随で gate を撃ち直す周の lens（`--lens`・無ければ `None`＝
    /// 撃ち直しは INCONCLUSIVE へ倒れ land しない）。
    pub lens: Option<&'a str>,
    /// 規則から読んだ線（撃ち直しの gate へ渡す・land 自身は数値を見ない）。
    pub limits: Limits,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// main が動いた便の追随の結果（設計 §5.4）。
enum Follow {
    /// rebase と gate の撃ち直しを通した。stdout に載せる行（`rebase=` と撃ち直しの判定行）。
    Ready(Vec<String>),
    /// 追随できなかった・撃ち直しが PASS でない。呼び手はこの Outcome をそのまま返す。
    Stopped(Outcome),
}

/// 面 5 の export 先。
///
/// dir 名を 2 度書かず event log の隣として導く（`store` が dir を変えたら追随する）。
pub fn verdicts_path(state_dir: &Path) -> PathBuf {
    store::events_path(state_dir).with_file_name(VERDICTS_FILE)
}

/// land 後に worktree を寄せる先。
pub fn retired_path(repo: &Path, id: &str) -> PathBuf {
    worktrees_dir(repo).join(RETIRED_DIR).join(id)
}

/// main 実測用の tmp worktree。
fn check_path(repo: &Path, id: &str) -> PathBuf {
    worktrees_dir(repo).join(CHECK_DIR).join(id)
}

/// land を 1 回通す。
pub fn land(entry: &Land<'_>) -> Outcome {
    let worktree = worktree_path(entry.repo, entry.run);
    let Some(base) = super::base_of_run(entry.state_dir, entry.run) else {
        return refused(format!("run {} に base が無い", entry.run));
    };
    if verdict_of(entry.state_dir, entry.run) != Some(Verdict::Pass) {
        return refused(format!("run {} の verdict が PASS でない", entry.run));
    }
    if let Some(cmd) = entry.pr_cmd {
        return open_pr(entry, &base, cmd);
    }
    let Some(old) = git_line(entry.repo, &["rev-parse", MAIN_REF]) else {
        return refused(format!("{MAIN_REF} を読めない"));
    };
    let mut lines = Vec::new();
    if old != base {
        // **CAS の old が動いている**。base が main の祖先なら追随する（rebase → gate の
        // 撃ち直し・設計 §5.4）。追随の形が無い周はここで断る（何も書かない）。
        match follow_main(entry, &worktree, &base, &old) {
            Follow::Stopped(outcome) => return outcome,
            Follow::Ready(followed) => lines = followed,
        }
    }
    // 撃ち直しの間に main がさらに動いた周は断る。次の land が同じ経路で追随する＝
    // 1 回の land が rebase するのは 1 度だけで、event 列が追随の回数をそのまま語る。
    // どちらの断りも **追随と撃ち直しの判定行は捨てない**（起きたことは event に残り lens も
    // 消費している＝stdout だけが空だと読み手が「何もしなかった」と誤読する）。
    let Some(now) = git_line(entry.repo, &["rev-parse", MAIN_REF]) else {
        return with_lines(lines, refused(format!("{MAIN_REF} を読めない")));
    };
    if now != old {
        return with_lines(
            lines,
            refused(format!("stale base（base={old} main={now}・撃ち直しの間に main が動いた）")),
        );
    }
    // anchor の見立ては **ref を進める前**に読む: 進めた後の `git status` は index の遅れを
    // 「変更」として出すので、人の未 commit と区別できない。
    let plan = anchor_plan(entry.repo);
    let new = match squash(entry, &worktree, &old) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let check = verify_main(entry, &new);
    // **実測の結果に依らず揃える**: ref は既に進んでいるので、赤 / 測れない周に揃えないと
    // anchor が staged の逆向きのまま残り、人が触る failure exit でこそ `.117` の経路が開く。
    let anchor = sync_anchor(entry.repo, &plan, &old, &new);
    let outcome = match check {
        MainCheck::Green => finish(entry, &worktree, &new, &anchor),
        MainCheck::Red(reason) => main_red(entry, &reason, &anchor),
        MainCheck::Unmeasurable(reason) => main_unmeasured(entry, &reason, &anchor),
    };
    with_lines(lines, outcome)
}

/// anchor（`--repo` の checkout）を land の後に新 main へ揃えるかの見立て（`s2-07l.120`）。
///
/// 揃えるのは **HEAD が `refs/heads/main` を指し ∧ tracked な未 commit の変更が無い**周だけ。
/// untracked は数えない（揃える動作は tracked path しか触らず、衝突すれば git が断る＝
/// [`AnchorSkip::SyncFailed`]）。読めない周は clean に読み替えない（fail-closed）。
///
/// 極性一覧の境界（`s2-07l.124`・C11.2 / C16.2）: 隣の [`ANCHOR_POLARITY`] が値を持つ。一覧の pointer は
/// 字面（`pipe::land::AnchorPlan`）で、型は crate の外へ出さない（構築する口が private ゆえ公開しても判定させられない）。
enum AnchorPlan {
    /// 揃える。
    Sync,
    /// 触らない（理由）。
    Skip(AnchorSkip),
}

/// この境界の極性（[`AnchorPlan`]）: 同期の**前**に見立てて止め（in-loop）、読めない周は揃えない（fail-closed）。
/// `status` を読めない周は `anchor=skipped:unreadable`、`symbolic-ref` が答えない周（detached と区別しない）は
/// `skipped:not-main` に落ちる——どちらも揃えない側である（lens-124 M2）。
pub const ANCHOR_POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 見立てを読む。
fn anchor_plan(repo: &Path) -> AnchorPlan {
    if git_line(repo, &["symbolic-ref", "-q", "HEAD"]).is_none_or(|head| head != MAIN_REF) {
        return AnchorPlan::Skip(AnchorSkip::NotMain);
    }
    match git_bytes(repo, &["status", "--porcelain", "--untracked-files=no"]) {
        None => AnchorPlan::Skip(AnchorSkip::Unreadable),
        Some(bytes) if !String::from_utf8_lossy(&bytes).trim().is_empty() => AnchorPlan::Skip(AnchorSkip::Dirty),
        Some(_) => AnchorPlan::Sync,
    }
}

/// 揃えた結果（判定行の `anchor=` token）。
enum AnchorSync {
    /// index と working tree が新 main に揃った。
    Synced,
    /// 触っていない（理由）。
    Skipped(AnchorSkip),
}

impl AnchorSync {
    /// 判定行の token。
    fn token(&self) -> String {
        match self {
            Self::Synced => "anchor=synced".to_owned(),
            Self::Skipped(reason) => format!("anchor=skipped:{}", reason.as_str()),
        }
    }

    /// stderr に出す warning（人の注意が要る周だけ・別 branch は通常形なので黙る）。
    /// **状態を断定しない**: 途中で断られた周は部分的に更新されていることがある（lens-120 M2）。
    fn warning(&self) -> Option<String> {
        match self {
            Self::Synced | Self::Skipped(AnchorSkip::NotMain) => None,
            Self::Skipped(AnchorSkip::SyncFailed) => Some(
                "pipe: anchor を新 main に揃える途中で git が断った（sync-failed）・index と working tree は部分的に更新されている可能性がある＝`commit -a` の前に `git status` で確かめること".to_owned(),
            ),
            Self::Skipped(reason) => Some(format!(
                "pipe: anchor を新 main に揃えていない（{}）・index と working tree は旧 main のまま＝`commit -a` の前に揃えること",
                reason.as_str()
            )),
        }
    }
}

/// landed tree が `old` に対して**足す** path のうち、anchor の working tree に既に在るものが 1 つでも
/// 在るか（untracked・ignored を含む・読めない周は「在る」側＝fail-closed）。
fn anchor_has_collision(repo: &Path, old: &str, new: &str) -> bool {
    let Some(bytes) = git_bytes(repo, &["diff", "--name-only", "--diff-filter=A", "-z", old, new]) else {
        return true;
    };
    bytes
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .any(|path| repo.join(String::from_utf8_lossy(path).as_ref()).symlink_metadata().is_ok())
}

/// anchor の index と working tree を `old` の tree から `new` の tree へ揃える。
///
/// `git read-tree -m -u <old> <new>` の 2-tree merge を使う。`reset --keep <new>` は **ref が
/// 既に `new` を指している**ため差分を 0 と見て working tree を更新しない（index だけが new に
/// なり `commit -a` が landed 変更を巻き戻す形へ悪化する・実測 2026-09-12）。`read-tree -m -u` は
/// old→new で変わった path だけを更新し、局所の変更が在れば "not uptodate" で断る——ただし
/// **ignored な untracked file は黙って上書きする**ので、足す path の衝突は先に見る（[`AnchorSkip::Collision`]）。
fn sync_anchor(repo: &Path, plan: &AnchorPlan, old: &str, new: &str) -> AnchorSync {
    match plan {
        AnchorPlan::Skip(reason) => AnchorSync::Skipped(*reason),
        AnchorPlan::Sync if anchor_has_collision(repo, old, new) => AnchorSync::Skipped(AnchorSkip::Collision),
        AnchorPlan::Sync if git_ok(repo, &["read-tree", "-m", "-u", old, new]) => AnchorSync::Synced,
        AnchorPlan::Sync => AnchorSync::Skipped(AnchorSkip::SyncFailed),
    }
}

/// 追随の行（`rebase=` と撃ち直しの判定行）を Outcome の stdout に前置する。
fn with_lines(mut lines: Vec<String>, mut outcome: Outcome) -> Outcome {
    lines.append(&mut outcome.out);
    outcome.out = lines;
    outcome
}

/// 便の base が main の祖先なら worktree の branch を main へ rebase し、gate を**同じ関数で**
/// 撃ち直す（設計 §5.4・`s2-07l.119`）。**main は 1 byte も動かさない**——rebase が効くのは
/// worktree の branch だけで、force 系は使わない（N1）。
///
/// - 祖先でない（main が巻き戻った / 分岐した）周は追随の形が無いので rc 1 で何もしない。
/// - worktree が clean でない周も rc 1 で何もしない（汚れた木では rebase を走らせない）。
/// - 衝突は `git rebase --abort` で木を戻し `Failed detail=rebase-conflict`（終端・fail-closed）。
/// - rebase で commit が 0 本になった周（同一変更の便が先に land）は gate を撃ち直さず
///   `Failed detail=rebase-empty`（便の変更は既に main に在る＝close してよい合図）。
/// - 追随した事実は `RunStage stage=Implemented detail=rebase:<old>..<new>` で残す（段が
///   `Gated` から `Implemented` へ戻る 1 件＝撃ち直す便の記帳）。base の読み手
///   （[`super::base_of_run`]）はこの行から新しい base を読む。
/// - 撃ち直しが PASS でない周は gate の判定行と rc で止まる（FAIL は `Gated` のまま
///   land しない・INCONCLUSIVE は測り直せる側）。
fn follow_main(entry: &Land<'_>, worktree: &Path, base: &str, main: &str) -> Follow {
    if !git_ok(entry.repo, &["merge-base", "--is-ancestor", base, main]) {
        return Follow::Stopped(refused(format!(
            "stale base（base={base} main={main}・base は main の祖先でない）"
        )));
    }
    let check = WorktreeCheck::judge(worktree);
    if !check.is_clean() {
        return Follow::Stopped(refused(format!(
            "run {} の worktree が clean でない（{}・rebase しない）",
            entry.run,
            check.as_str()
        )));
    }
    if let Err(stopped) = rebase_onto(entry, worktree, base, main) {
        return Follow::Stopped(stopped);
    }
    let rebased = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Implemented),
            seat: None,
            pid: None,
            detail: Some(format!("rebase:{base}..{main}")),
        },
        entry.policy,
    );
    if let Err(err) = rebased {
        return Follow::Stopped(broken(err.to_string()));
    }
    let mut lines = vec![format!("run={} rebase={base}..{main}", entry.run)];
    let regated = gate(&Gate {
        run: entry.run,
        bead: entry.bead,
        repo: entry.repo,
        state_dir: entry.state_dir,
        contract: entry.contract,
        lens: entry.lens,
        limits: entry.limits,
        policy: entry.policy,
    });
    lines.extend(regated.out);
    if regated.rc != RC_OK {
        return Follow::Stopped(Outcome { out: lines, err: regated.err, rc: regated.rc });
    }
    Follow::Ready(lines)
}

/// worktree の branch を main へ rebase する（追随の (iii)・(iii′)）。**main は動かさない**。
///
/// - 衝突は `git rebase --abort` で木を戻し `rebase-conflict` で終端する。戻せない周もその事実は
///   捨てない（段は Failed で retire も通らないので、読み手に届く口は stderr だけ）。
/// - **同一変更の便**: rebase で commit が 0 本になった周は便の変更が既に main に在る（先に land した
///   便と同じ patch）ので gate を撃ち直さず（lens を起動しない）`rebase-empty` で終端する。commit 数を
///   読めない周は 0 に読み替えず、従来どおり撃ち直しの precheck へ流す（fail-closed の向きを変えない・
///   `s2-07l.125`）。
fn rebase_onto(entry: &Land<'_>, worktree: &Path, base: &str, main: &str) -> Result<(), Outcome> {
    if !git_ok(worktree, &["rebase", main]) {
        let restored = git_ok(worktree, &["rebase", "--abort"]);
        let mut stopped = follow_failed(
            entry,
            "rebase-conflict",
            format!("run {} の rebase が衝突した（base={base} main={main}）・main は動かさない", entry.run),
        );
        if !restored {
            stopped
                .err
                .push(format!("pipe: {} は rebase の途中のまま（--abort も失敗）", worktree.display()));
        }
        return Err(stopped);
    }
    if commits_after_rebase(worktree, main) == Some(0) {
        return Err(follow_failed(
            entry,
            "rebase-empty",
            format!(
                "run {} の変更は既に main に在る（rebase で commit が空・base={base} main={main}）・main は動かさない",
                entry.run
            ),
        ));
    }
    Ok(())
}

/// rebase の後に便へ残った commit の数（`<main>..HEAD`）。**読めない周は `None`**（0 に読み替えない）。
fn commits_after_rebase(worktree: &Path, main: &str) -> Option<u64> {
    let range = format!("{main}..HEAD");
    git_line(worktree, &["rev-list", "--count", &range])?.parse().ok()
}

/// 追随の途中で便が終端した周（`rebase-conflict` / `rebase-empty`）。**main は動いていない**。
/// 理由は `Failed` の `detail` に名乗り、stderr の 1 行は呼び手が組む（同じ形・run id と base / main を持つ）。
fn follow_failed(entry: &Land<'_>, detail: &str, reason: String) -> Outcome {
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Failed),
            seat: None,
            pid: None,
            detail: Some(detail.to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => refused(reason),
    }
}

/// PR を作る seam を通す（設計 §5.4 の `--pr-cmd`・**main を動かさない**）。
///
/// **承認 event は前提でない**。自 repo へ branch を push して PR を出す行為は main を
/// 動かさず、branch も PR も閉じられる＝可逆ゆえ、憲法 A4.3（merge・自 repo への
/// dispatch・依存なしの code 変更は A4.2 の目的において可逆）により Ask-first の「出す」
/// に当たらない（ADR-0008）。3 クラスの判定は契約の自己申告（`classes`）だけに効き、
/// seam を使ったことから導出しない。
///
/// **stale base は見ない**。CAS の old が要るのは ref を進める周だけで、この形は ref を
/// 1 本も動かさない——PR が載るかどうかは forge が決める。逆にここで base を縛ると、
/// main が動いた瞬間に PR を出せなくなる（自己ホストの便が最も踏みやすい）。
///
/// **道具の失敗で便を終端させない**（rc 1・event を書かない）。push や PR 作成は network
/// で落ちうるので、`Failed` を焼くと再試行できない便が残る。
fn open_pr(entry: &Land<'_>, base: &str, cmd: &str) -> Outcome {
    // **空の seam を通さない**（使い方の誤り・rc 1・何も書かない）。`sh -c ""` は rc 0 で
    // 終わるので、素通しすると「PR を出した」を記帳しながら **1 行も公開していない**便が
    // 生まれる（何もしていないのに「やった」が永続面に残る——最も避けたい嘘である）。
    if cmd.trim().is_empty() {
        return refused("--pr-cmd が空である".to_owned());
    }
    let branch = super::branch_name(entry.run);
    let line = cmd.replace("{branch}", &branch).replace("{base}", base);
    let ran = std::process::Command::new("sh")
        .arg("-c")
        .arg(&line)
        .current_dir(entry.repo)
        .status();
    match ran {
        Err(err) => return refused(format!("PR の道具を起動できない: {err}")),
        Ok(status) if !status.success() => {
            return refused(format!(
                "PR の道具が rc {} で終わった",
                status.code().unwrap_or(-1)
            ))
        }
        Ok(_) => {}
    }
    // **面 5 へは書かない**: `verdicts.jsonl` は main に載った便の記録で、この形の便は
    // まだ載っていない（merge は人が押す）。worktree も畳まない（PR は生きている）。
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunDone,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Landed),
            seat: None,
            pid: None,
            detail: Some("pr".to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => Outcome::ok_line(format!("run={} landed=pr", entry.run)),
    }
}

/// worktree の tree を 1 commit にして main を CAS で進める。**tree の同一を実測する**。
fn squash(entry: &Land<'_>, worktree: &Path, old: &str) -> Result<String, String> {
    let tree = git_line(worktree, &["rev-parse", "HEAD^{tree}"])
        .ok_or_else(|| format!("{} の tree を読めない", worktree.display()))?;
    let message = format!("{}: {}", entry.bead, entry.contract.goal);
    let new = git_line(entry.repo, &["commit-tree", &tree, "-p", old, "-m", &message])
        .ok_or_else(|| "squash commit を作れない".to_owned())?;
    if !git_ok(entry.repo, &["update-ref", MAIN_REF, &new, old]) {
        return Err(format!("{MAIN_REF} を付け替えられない（CAS が外れた）"));
    }
    let landed = git_line(entry.repo, &["rev-parse", &format!("{new}^{{tree}}")])
        .ok_or_else(|| "land した tree を読めない".to_owned())?;
    if landed != tree {
        return Err(format!("tree が同一でない（{tree} → {landed}）"));
    }
    Ok(new)
}

/// 進めた main を別の worktree で実測する。
fn verify_main(entry: &Land<'_>, new: &str) -> MainCheck {
    let tmp = check_path(entry.repo, entry.run);
    if let Some(parent) = tmp.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            return MainCheck::Unmeasurable(format!("{} を作れない: {err}", parent.display()));
        }
    }
    let path = tmp.display().to_string();
    if !git_ok(entry.repo, &["worktree", "add", "--detach", &path, new]) {
        // **ここで赤を名乗らない**: verify 行を 1 本も撃てていない。
        return MainCheck::Unmeasurable(format!("{} を切れない", tmp.display()));
    }
    // **gate と同じ順序を同じ関数で撃つ**（write-set 照合 → 写しの共通 verify → 契約 verify）。
    // 材料が揃わない周は**赤を名乗らない**——読めなかったを落ちたに化けさせない。
    let materials = materials(entry);
    let (base, common) = match materials {
        Ok(found) => found,
        Err(reason) => {
            let _ = git_ok(entry.repo, &["worktree", "remove", "--force", &path]);
            return MainCheck::Unmeasurable(reason);
        }
    };
    let steps = run_checks(&Checks {
        worktree: &tmp,
        base: &base,
        contract: entry.contract,
        common: &common,
    });
    // 成果は `new` に載っているので、この tmp だけは remove してよい（設計 §5.4）。
    // `--force` は verify が tmp に生んだ中間物ごと畳むためで、履歴・データは触らない。
    let _ = git_ok(entry.repo, &["worktree", "remove", "--force", &path]);
    // 段①を読めなかった周（gate と**同じ 1 本の判定**・rc だけでは見ない）は**赤の集計より先に**
    // 「測れなかった」へ倒す——読めなかったを落ちたに化けさせない（gate §6 と同じ極性・
    // `s2-07l.103`）。Red と同じく main-green にも finish にも進まない（fail-closed）。
    if let Some(step) = steps.iter().find(|step| is_unreadable(step)) {
        return MainCheck::Unmeasurable(format!(
            "main で verify の段を読めない（cmd={} stderr={}）",
            step.cmd,
            step.stderr.lines().next().unwrap_or_default()
        ));
    }
    let red = steps.iter().filter(|step| step.rc != 0).count();
    if red > 0 {
        return MainCheck::Red(format!("main で verify の {red} 行が rc≠0"));
    }
    MainCheck::Green
}

/// main の実測に要る材料（便の base と、写しの共通 verify）を揃える。
///
/// **写しからしか読まない**（repo / worktree の `.vessel.toml` は読み直さない・ADR-0010 §2.4）。
fn materials(entry: &Land<'_>) -> Result<(String, Vec<String>), String> {
    let base = super::base_of_run(entry.state_dir, entry.run)
        .ok_or_else(|| format!("run {} に base が無い", entry.run))?;
    let path = super::vessel_path(entry.state_dir, entry.run);
    let common = Effective::load(&path)
        .map(|found| found.common_verify().to_vec())
        .map_err(|errors| {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            format!("{} を読めない: {}", path.display(), lines.join(" / "))
        })?;
    Ok((base, common))
}

/// export → `Landed` → 後始末。ここまで来た周は land が成立している（anchor は呼び手が揃え済み）。
fn finish(entry: &Land<'_>, worktree: &Path, new: &str, anchor: &AnchorSync) -> Outcome {
    if let Err(reason) = export_verdict(entry, new) {
        return broken(reason);
    }
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunDone,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Landed),
            seat: None,
            pid: None,
            detail: Some(format!("sha:{new}")),
        },
        entry.policy,
    );
    if let Err(err) = emitted {
        return broken(err.to_string());
    }
    // 後始末の失敗は land を取り消さない（**rc 0 のまま stderr 1 行**）。anchor の warning も同じ列。
    let mut err = retire_worktree(entry.repo, entry.run, worktree);
    err.extend(anchor.warning());
    Outcome {
        out: vec![format!("run={} landed={new} {}", entry.run, anchor.token())],
        // 後始末の失敗は land を取り消さない（**rc 0 のまま stderr**）。
        err,
        rc: RC_OK,
    }
}

/// 面 5 の 1 行を `verdicts.jsonl` へ append する（跨版 契約・key 列は固定）。
fn export_verdict(entry: &Land<'_>, new: &str) -> Result<(), String> {
    let evidence = verdict_path(entry.state_dir, entry.run).display().to_string();
    let line = json_lite::write_object(&[
        ("schema", Value::Num(SCHEMA)),
        ("run", Value::Str(entry.run.to_owned())),
        ("bead", Value::Str(entry.bead.to_owned())),
        ("sha", Value::Str(new.to_owned())),
        ("verdict", Value::Str(Verdict::Pass.as_str().to_owned())),
        ("evidence", Value::Str(evidence)),
        ("ts", Value::Str(now_utc())),
    ]);
    append_line(&verdicts_path(entry.state_dir), &line, entry.policy)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

/// worktree を `retired/<run>` へ move する。**削除しない・branch も消さない**（N1.2）。
///
/// 呼び手は 2 つ（squash 形の [`finish`] と `pipe retire`）で、**move の中身は 1 本**である
/// ——2 実装に割ると、一方だけが削除へ寄る余地が生まれる。失敗は stderr 行の列で返し、
/// rc は呼び手が決める（land では 0 のまま・retire では 2）。
pub(crate) fn retire_worktree(repo: &Path, run: &str, worktree: &Path) -> Vec<String> {
    let dest = retired_path(repo, run);
    let Some(parent) = dest.parent() else {
        return vec!["pipe: retired の親 dir を解けない".to_owned()];
    };
    if let Err(err) = std::fs::create_dir_all(parent) {
        return vec![format!("pipe: {} を作れない: {err}", parent.display())];
    }
    let from = worktree.display().to_string();
    let to = dest.display().to_string();
    if git_ok(repo, &["worktree", "move", &from, &to]) {
        return Vec::new();
    }
    vec![format!("pipe: {from} を {to} へ移せなかった")]
}

/// 便の worktree が clean か（**閉じた enum**・`s2-07l.124`・C11.2）。呼び手は 2 つ——
/// [`follow_main`] の rebase の前（汚れた木で rebase を走らせない・`.119`）と [`retire`] の move の前——で、
/// 極性一覧には **判定 enum 1 つ = 行 1 本**（`land-worktree-clean`）として載る（lens-124 M1・bead notes）。
///
/// 「状態を読めなかった」を「汚れていない」に化けさせない（[`Unreadable`](Self::Unreadable) は
/// [`Dirty`](Self::Dirty) と同じく止める）——move は中身ごと運ぶので、未 commit の仕事を持った
/// worktree を畳むと、その仕事の行き先が便の外から読めなくなる。untracked も数える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeCheck {
    /// `status --porcelain` が空。畳める。
    Clean,
    /// 未 commit の変更（untracked を含む）が在る。
    Dirty,
    /// status を読めない（git が断った・repo でない）。
    Unreadable,
}

/// この境界の極性（[`WorktreeCheck`]）: rebase / move の**前**に読んで止め（in-loop）、読めない周は止める（fail-closed）。
pub const WORKTREE_POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

impl WorktreeCheck {
    /// worktree の状態を読む。
    pub fn judge(worktree: &Path) -> Self {
        match git_bytes(worktree, &["status", "--porcelain"]) {
            None => Self::Unreadable,
            Some(bytes) if String::from_utf8_lossy(&bytes).trim().is_empty() => Self::Clean,
            Some(_) => Self::Dirty,
        }
    }

    /// 進めてよいか。**bool はここ 1 本で enum から導く**（読めない周は偽）。
    pub fn is_clean(self) -> bool {
        match self {
            Self::Clean => true,
            Self::Dirty | Self::Unreadable => false,
        }
    }

    /// 断りの理由の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Dirty => "dirty",
            Self::Unreadable => "unreadable",
        }
    }
}

/// `--pr-cmd` 形で終端した便の worktree を、merge の後に畳む（設計 §5.4）。
///
/// **`detail=pr` を前提にしない**。squash 形で move だけが落ちた便（land は rc 0 のまま
/// stderr 1 行で終わる）を後追いで畳む口にもなるので、見るのは永続面の事実——worktree が
/// 在るか・clean か——だけである。**merge 済みかは人が確かめる**（forge へ問い合わせない）。
///
/// 前提違反は **rc 1 + stderr 1 行で何も書かない**（設計 §4 の一般則）。move の失敗だけは
/// 「対象そのものが壊れている」ので rc 2 で、どちらの周も event を 1 件も残さない。
pub fn retire(entry: &Retire<'_>) -> Outcome {
    let worktree = worktree_path(entry.repo, entry.run);
    if !worktree.is_dir() {
        // 2 度目の retire もここで止まる（1 度目が畳んでいるので元の場所に無い）。
        return refused(format!("run {} の worktree {} が無い", entry.run, worktree.display()));
    }
    let check = WorktreeCheck::judge(&worktree);
    if !check.is_clean() {
        return refused(format!("run {} の worktree が clean でない（{}）", entry.run, check.as_str()));
    }
    let failures = retire_worktree(entry.repo, entry.run, &worktree);
    if !failures.is_empty() {
        // **畳めていないのに「畳んだ」を記帳しない**（永続面と event が食い違う）。
        return Outcome { out: Vec::new(), err: failures, rc: RC_BROKEN };
    }
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            // **段は Landed のまま**（終端を動かさない）。畳んだことは detail で残す。
            stage: Some(Stage::Landed),
            seat: None,
            pid: None,
            detail: Some("retired".to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => Outcome::ok_line(format!(
            "run={} retired={}",
            entry.run,
            retired_path(entry.repo, entry.run).display()
        )),
    }
}

/// `verdict.json` から 3 値を読む。読めない周は `None`（＝PASS ではない）。
///
/// **判定の読み手はこの 1 本だけである**。land の前提（PASS か）だけでなく、gate の
/// 測り直し（Gated ∧ INCONCLUSIVE か）と resume の行き先（land か gate か）も同じ値を
/// 見る。読み手を増やすと、同じ JSON の解釈が場所ごとに静かにずれる。
/// 共有先は兄弟 module だけなので、公開面は crate の中に留める。
pub(crate) fn verdict_of(state_dir: &Path, id: &str) -> Option<Verdict> {
    let text = std::fs::read_to_string(verdict_path(state_dir, id)).ok()?;
    let pairs = json_lite::parse_object(text.trim()).ok()?;
    pairs
        .iter()
        .find(|(key, _)| key == "verdict")
        .and_then(|(_, value)| value.as_str())
        .and_then(Verdict::parse)
}

/// main の実測が赤だった周。**auto revert しない**（main は進んだまま・anchor は揃え済み＝stderr に token）。
fn main_red(entry: &Land<'_>, reason: &str, anchor: &AnchorSync) -> Outcome {
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Failed),
            seat: None,
            pid: None,
            detail: Some("main-red".to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => with_anchor(refused(format!("main が赤い（{reason}）・revert しない")), anchor),
    }
}

/// failure exit の stderr に anchor の token（と warning）を足す（ref は進んでいるので黙らない）。
fn with_anchor(mut outcome: Outcome, anchor: &AnchorSync) -> Outcome {
    outcome.err.push(format!("pipe: {}", anchor.token()));
    outcome.err.extend(anchor.warning());
    outcome
}

/// main を実測できなかった周。**赤とは別の名で残す**（rc 2 = 対象が壊れている・anchor は揃え済み）。
fn main_unmeasured(entry: &Land<'_>, reason: &str, anchor: &AnchorSync) -> Outcome {
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Failed),
            seat: None,
            pid: None,
            detail: Some("main-unmeasured".to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => with_anchor(broken(format!("main を実測できない（{reason}）・revert しない")), anchor),
    }
}

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行・何もしない）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}
