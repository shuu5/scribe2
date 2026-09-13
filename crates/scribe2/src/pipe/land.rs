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
//! **同じ関数で**撃ち直し、PASS なら新しい base で CAS する。衝突は木を戻して**実装役を
//! 起こし直す**（[`super::follow`]・便は終端にしない・終端するのは上限に達した周だけ）。
//!
//! **着地は gate 済みの便を先に通す**（`s2-07l.147`・設計 gate-cost.md §6）。前提検査の直後・追随の
//! 前に、同じ置き場の着地待ちの列（event log の replay から導く・別の状態 file を持たない）を見て、
//! 自分より前の便が居る間は待つ。待ちは完了 enum の variant 1 つ（[`Completion::LandTurn`]）で唯一の
//! wait 実装を通り、上限（rules 行 `pipe.land_wait_s`）を超えた周と列を導けない周は**待たずに進む**
//! （断らない・止めない）。どの周だったかは land の record と stdout の `order=` が残す。

use crate::polarity::{OnFailure, Polarity, Timing};
use super::contract::Contract;
use super::declaration::Effective;
use super::follow::{self, Conflict};
use super::gate::{
    gate, is_unreadable, run_checks, step_record, Check, Checks, Gate, Limits, Step, Verdict,
};
use super::{
    emit, git_bytes, git_line, git_ok, verdict_path, worktree_path, worktrees_dir, Emit,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{self, append_line, LockPolicy};
use crate::fleet::{cli::now_utc, replay, Completion, Event, EventKind, Stage, Timeout, SCHEMA};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 進める ref。設計 §5.4 が名指す 1 本である（追随の相手を読む [`super::follow`] も同じ字面を使う）。
pub(crate) const MAIN_REF: &str = "refs/heads/main";

/// 面 5 の export 先の file 名（ADR-0004 §2.2・**版番号に依らず固定**）。
const VERDICTS_FILE: &str = "verdicts.jsonl";

/// land 済み worktree を寄せる dir 名。
const RETIRED_DIR: &str = "retired";

/// 追随の rebase で便の commit が 0 本になった周の終端の理由（`Failed` の `detail`）。
///
/// **書き手（[`rebase_onto`]）と読み手（`pipe retire` の入口）で字面を 2 度書かない**——
/// 片方だけを直すと、畳める便の集合が静かにずれる。
pub(crate) const REBASE_EMPTY: &str = "rebase-empty";

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

/// squash commit の件名に載せる要旨の長さ（**char 単位**・byte でない・`s2-07l.130`）。
///
/// git の慣習（件名は短く 1 行）に合わせて切るが、**切った goal は本文に逐語で残す**——
/// 要旨だけを残すと契約の中身が履歴から落ちる。
const SUBJECT_CHARS: usize = 72;

/// 要旨を切ったことを示す印（件名の末尾に 1 文字だけ足す）。
const ELLIPSIS: char = '…';

/// 本文の最後に置く trailer の key（読み手が fleet の記録へ辿る鍵）。
const RUN_TRAILER: &str = "run: ";

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
    /// **その便の現在の終端の段**（`Landed` か `Failed`・呼び手が replay から解いたもの）。
    /// 畳んだ事実を残す event はこの段のままで、retire は段を 1 つも動かさない（`s2-07l.128`）。
    pub stage: Stage,
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
    /// 追随が衝突した周に runner を起こし直すコマンド（`--runner`）。**無い周は起こし直さない**。
    pub runner: Option<&'a str>,
    /// 起こし直しの上限（rules 行 `pipe.follow_retries`・land 自身は数値を見ない）。
    pub retries: u64,
    /// 着地待ちの列で自分の番を待つ上限（秒・rules 行 `pipe.land_wait_s`）。超えた周は待たずに進む。
    pub land_wait_s: u64,
    /// 承認 event が在るか（起こし直しも A1 の関門を通る・replay の導出値）。
    pub approved: bool,
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
    // **着地の順番**（設計 gate-cost.md §6）: 前提検査の直後・追随の前に列を見て待つ。`--pr-cmd` の形は
    // 上で返っている＝main を動かさないので列を見ない（stale base を見ないのと同じ理由）。
    let order = await_turn(entry);
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
    // **squash の直後に揃える**（`s2-07l.131`）。ref を進めてから anchor を揃えるまでの窓——
    // `git status` に landed 変更が staged の逆向きで見える時間——は、実測の後に揃えると
    // **main 実測の長さだけ**開く（人が anchor を触れば `.117` の経路がその間ずっと開いている）。
    // 実測の前に揃えれば窓は秒単位に縮む。「**実測の結果に依らず揃える**」（`s2-07l.120`・lens-120 H1）は
    // この順序でこそ自明である——同期が先なら、そもそも結果を見ていない。
    //
    // 同期が `Skipped(SyncFailed)` の周も実測は続ける（ref は既に進んでいる＝同期の失敗で land を
    // 止めない・極性は不変）。結果は従来どおり [`finish`] / [`main_red`] / [`main_unmeasured`] へ渡す。
    let anchor = sync_anchor(entry.repo, &plan, &old, &new);
    let check = verify_main(entry, &new);
    let outcome = match check {
        MainCheck::Green => finish(entry, &worktree, &new, &anchor, order),
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
/// - 衝突は [`super::follow::on_conflict`] へ委ねる（設計 pipeline-conflict.md §3）。器は木を戻し、
///   衝突を `Implemented detail=rebase-conflict:<base>..<main>` で記帳して**実装役を起こし直す**
///   ——便を終端にするのは上限に達した周だけである。どの形でも land はここで止まり、続きは
///   `gate` から撃ち直す（起こし直した turn の後は、次の land の追随で再び rebase が走る）。
/// - **同一変更の便**: rebase で commit が 0 本になった周は便の変更が既に main に在る（先に land した
///   便と同じ patch）ので gate を撃ち直さず（lens を起動しない）`rebase-empty` で終端する。commit 数を
///   読めない周は 0 に読み替えず、従来どおり撃ち直しの precheck へ流す（fail-closed の向きを変えない・
///   `s2-07l.125`）。
fn rebase_onto(entry: &Land<'_>, worktree: &Path, base: &str, main: &str) -> Result<(), Outcome> {
    if !git_ok(worktree, &["rebase", main]) {
        return Err(follow::on_conflict(&Conflict {
            turn: turn_of(entry),
            base,
            main,
            limit: entry.retries,
        }));
    }
    if commits_after_rebase(worktree, main) == Some(0) {
        return Err(follow_failed(
            entry,
            REBASE_EMPTY,
            format!(
                "run {} の変更は既に main に在る（rebase で commit が空・base={base} main={main}）・main は動かさない",
                entry.run
            ),
        ));
    }
    Ok(())
}

/// 起こし直しの材料（land が持つ面から組む・**組み立てはこの 1 本**）。
fn turn_of<'a>(entry: &'a Land<'a>) -> follow::Turn<'a> {
    follow::Turn {
        run: entry.run,
        bead: entry.bead,
        repo: entry.repo,
        state_dir: entry.state_dir,
        contract: entry.contract,
        runner: entry.runner,
        approved: entry.approved,
        policy: entry.policy,
    }
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
    let message = squash_message(entry.bead, &entry.contract.goal, entry.run);
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

/// squash commit の message（設計 §5.4 手順 1・**3 部**・`s2-07l.130`）。
///
/// (1) 件名 `<bead>: <要旨>` (2) 空行 (3) 本文 = goal 全文（**逐語・改行を保つ**）+ 空行 +
/// `run: <run id>` の 1 行（trailer）。
///
/// 件名は goal の先頭の文を [`SUBJECT_CHARS`] 文字で切った要約ゆえ中身が落ちる——だから
/// **同じ message の中に落とさない側（本文の goal 全文）を必ず持つ**。`git log --oneline` は
/// 件名だけを読み、便の現物を追う人は本文と trailer から fleet の記録へ辿る。
fn squash_message(bead: &str, goal: &str, run: &str) -> String {
    format!("{}\n\n{goal}\n\n{RUN_TRAILER}{run}\n", subject_of(bead, goal))
}

/// 件名。要旨が空の周は **`<bead>` だけ**にして落とさない（契約の検査で goal は非空のはずで、
/// 件名を組めないことは land を止める理由ではない＝ここを fail-closed に倒すと、message の
/// 形の不備で main に載らない便が生まれる）。
fn subject_of(bead: &str, goal: &str) -> String {
    let gist = gist_of(goal);
    if gist.is_empty() {
        return bead.to_owned();
    }
    format!("{bead}: {gist}")
}

/// goal の先頭の文（最初の改行または「。」の手前まで・前後の空白と markdown の見出し記号 `#` を除く）
/// を [`SUBJECT_CHARS`] 文字で切る。切った周だけ末尾に [`ELLIPSIS`] を足す。
///
/// 切るのは **char 単位**である（byte で切ると UTF-8 の途中で割れる＝slice 禁止・C11）。
fn gist_of(goal: &str) -> String {
    let head = goal.split(['\n', '。']).next().unwrap_or_default();
    let sentence = head.trim().trim_start_matches('#').trim();
    let cut: String = sentence.chars().take(SUBJECT_CHARS).collect();
    if sentence.chars().count() > SUBJECT_CHARS {
        return format!("{cut}{ELLIPSIS}");
    }
    cut
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
    // **gate と同じ順序を同じ関数で撃つ**（write-set 照合 → 写しの共通 verify → 検出線 → 契約 verify）。
    // 材料が揃わない周は**赤を名乗らない**——読めなかったを落ちたに化けさせない。
    let materials = materials(entry);
    let (base, frozen) = match materials {
        Ok(found) => found,
        Err(reason) => {
            let _ = git_ok(entry.repo, &["worktree", "remove", "--force", &path]);
            return MainCheck::Unmeasurable(reason);
        }
    };
    let skipped = same_tree(entry, new);
    let steps = run_checks(&Checks {
        worktree: &tmp,
        base: &base,
        contract: entry.contract,
        common: frozen.common_verify(),
        detection: if skipped.is_some() { &[] } else { frozen.detection_verify() },
    });
    // 成果は `new` に載っているので、この tmp だけは remove してよい（設計 §5.4）。
    // `--force` は verify が tmp に生んだ中間物ごと畳むためで、履歴・データは触らない。
    let _ = git_ok(entry.repo, &["worktree", "remove", "--force", &path]);
    if let Err(reason) = record_main(entry, &steps, skipped.as_deref()) {
        return MainCheck::Unmeasurable(reason);
    }
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

/// main の実測に要る材料（便の base と、写しの共通 verify・検出線）を揃える。
///
/// **写しからしか読まない**（repo / worktree の `.vessel.toml` は読み直さない・ADR-0010 §2.4）。
fn materials(entry: &Land<'_>) -> Result<(String, Effective), String> {
    let base = super::base_of_run(entry.state_dir, entry.run)
        .ok_or_else(|| format!("run {} に base が無い", entry.run))?;
    let path = super::vessel_path(entry.state_dir, entry.run);
    let frozen = Effective::load(&path).map_err(|errors| {
        let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
        format!("{} を読めない: {}", path.display(), lines.join(" / "))
    })?;
    Ok((base, frozen))
}

/// main 実測の record を書く file の名（gate の `verify.jsonl` と同じ dir・同じ record 形・別 file）。
///
/// 別 file にするのは、gate の周の `n` と main 実測の `n` を重ねないためである（設計 gate-cost.md §5）。
const VERIFY_MAIN_FILE: &str = "verify-main.jsonl";

/// 検出線を省いた段の名（record の `skipped=`）。
const SKIPPED_DETECTION: &str = "detection";

/// land した木が **gate を撃った木と同じ**なら、その sha を返す（検出線を撃ち直さない周）。
///
/// `tree` の無い verdict（旧 gate）・読めない木・不一致はどれも `None`＝全段を撃つ側へ倒す
/// （省く側へ倒すと、測っていない検出線を main で通したことになる・ADR-0021 §2.4）。
fn same_tree(entry: &Land<'_>, new: &str) -> Option<String> {
    let gated = verdict_field(entry.state_dir, entry.run, "tree")?;
    let landed = git_line(entry.repo, &["rev-parse", &format!("{new}^{{tree}}")])?;
    (landed == gated).then_some(landed)
}

/// main 実測の段を `verify-main.jsonl` へ逐条で残す。検出線を省いた周は、その段の位置に
/// `skipped=detection tree=<sha>` の record を 1 件置く（**撃たなかった事実を黙って落とさない**）。
fn record_main(entry: &Land<'_>, steps: &[Step], skipped: Option<&str>) -> Result<(), String> {
    let path = super::verify_log_path(entry.state_dir, entry.run).with_file_name(VERIFY_MAIN_FILE);
    let mut records = Vec::new();
    let mut pending = skipped;
    for step in steps {
        if step.stage == Check::Contract {
            if let Some(tree) = pending.take() {
                records.push(skip_record(records.len(), tree));
            }
        }
        records.push(step_record(next_number(records.len()), step));
    }
    if let Some(tree) = pending {
        records.push(skip_record(records.len(), tree));
    }
    for record in &records {
        append_line(&path, record, entry.policy).map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// 既に積んだ record 数から次の `n`（1 始まり）。
fn next_number(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX).saturating_add(1)
}

/// 検出線を省いた段の record。
fn skip_record(len: usize, tree: &str) -> String {
    json_lite::write_object(&[
        ("schema", Value::Num(SCHEMA)),
        ("n", Value::Num(next_number(len))),
        ("kind", Value::Str(Check::Detection.as_str().to_owned())),
        ("skipped", Value::Str(SKIPPED_DETECTION.to_owned())),
        ("tree", Value::Str(tree.to_owned())),
    ])
}

/// export → `Landed` → 後始末。ここまで来た周は land が成立している（anchor は呼び手が揃え済み）。
fn finish(entry: &Land<'_>, worktree: &Path, new: &str, anchor: &AnchorSync, order: Order) -> Outcome {
    if let Err(reason) = export_verdict(entry, new, order) {
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
        out: vec![format!("run={} landed={new} {} order={}", entry.run, anchor.token(), order.as_value())],
        // 後始末の失敗は land を取り消さない（**rc 0 のまま stderr**）。
        err,
        rc: RC_OK,
    }
}

/// 面 5 の 1 行を `verdicts.jsonl` へ append する（跨版 契約・key 列は固定）。
///
/// `order` は schema 1 のまま足した**任意 field**（ADR-0021 §2.6 (iv)・古い読み手は無視する）で、
/// 列の後ろに置く（既存の 7 key の並びは動かさない）。
fn export_verdict(entry: &Land<'_>, new: &str, order: Order) -> Result<(), String> {
    let evidence = verdict_path(entry.state_dir, entry.run).display().to_string();
    let line = json_lite::write_object(&[
        ("schema", Value::Num(SCHEMA)),
        ("run", Value::Str(entry.run.to_owned())),
        ("bead", Value::Str(entry.bead.to_owned())),
        ("sha", Value::Str(new.to_owned())),
        ("verdict", Value::Str(Verdict::Pass.as_str().to_owned())),
        ("evidence", Value::Str(evidence)),
        ("ts", Value::Str(now_utc())),
        ("order", Value::Str(order.as_value())),
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
/// **段を動かさない**（`s2-07l.128`）。畳める便は `Landed`・`Failed detail=rebase-empty`
/// （変更が既に main に在る）・`Failed detail=rebase-conflict`（起こし直しの上限に達した）・
/// `Gated` で verdict が FAIL（判定に届いた終端）の 4 通りで、どの周も残す event の段は
/// [`Retire::stage`] のまま＝`Landed` に決め打ちしない。畳む動作そのものは 1 本で、
/// 段の弁別は入口（`pipe::cli`）が持つ。
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
            // **段は入口が解いた終端のまま**（終端を動かさない）。畳んだことは detail で残す。
            stage: Some(entry.stage),
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
    verdict_field(state_dir, id, "verdict").as_deref().and_then(Verdict::parse)
}

/// `verdict.json` の文字列 field を 1 つ読む（**JSON の読み手はこの 1 本**・読めない周は `None`）。
fn verdict_field(state_dir: &Path, id: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(verdict_path(state_dir, id)).ok()?;
    let pairs = json_lite::parse_object(text.trim()).ok()?;
    pairs
        .iter()
        .find(|(found, _)| found == key)
        .and_then(|(_, value)| value.as_str())
        .map(str::to_owned)
}

/// 着地待ちの列を導く材料 1 本（設計 gate-cost.md §6・**replay の導出値**＝別の状態 file を持たない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    /// 便 id。
    pub run: String,
    /// replay が見た最新の段。
    pub stage: Stage,
    /// `verdict.json` の 3 値＝**最新の判定**（**読めない周は `None`**・列に入りうる便〔終端でない ∧
    /// `Gated` を 1 度でも通った〕でなければ読まない＝`None`）。追随の撃ち直しの間は前の周の判定のまま
    /// （gate は判定の確定時にだけ上書きする）。
    pub verdict: Option<Verdict>,
    /// **最初の** `Gated` event の ts（列の鍵・撃ち直しで `Gated` が増えても動かない・`Gated` を
    /// 1 度も通っていない便は `None`）。
    pub gated_at: Option<String>,
    /// 便の worktree が実在するか（retire 済み・move 済みは偽）。
    pub worktree: bool,
}

/// 自分の land の番（**閉じた 3 値**・設計 gate-cost.md §6）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Turn {
    /// 列の前に誰も居ない。
    First,
    /// 列の前に居る最古の便（この便が列を空けるまで待つ）。
    After(String),
    /// 列を導けない（store / 判定を読めない・自分の `Gated` の ts が無い）。**`First` に読み替えない**。
    Unmeasurable,
}

/// 列の中の自分の番を判じる（**pure**・判定はこの 1 本）。
///
/// 列 = 終端でない ∧ `Gated` を 1 度でも通った ∧ worktree が実在 ∧ 最新の verdict が PASS の便。
/// **追随して段が `Implemented` へ戻り撃ち直している便も列に残る**（撃ち直しの間に後続が番を得ない・
/// 撃ち直しが FAIL なら verdict で外れる）。順序は**最初の** `Gated` の ts（同時刻は run id の辞書順）
/// ＝全順序ゆえ待ちは循環しない。列に入りうる便で verdict を読めない便が 1 本でも在る周は列を導けない
/// ——PASS かを測れない便を列から外すと、読めないを「列なし」に読み替えることになる。
pub fn turn_in(queue: Option<&[Queued]>, me: &str) -> Turn {
    let Some(entries) = queue else {
        return Turn::Unmeasurable;
    };
    let Some(mine) = entries
        .iter()
        .find(|found| found.run == me)
        .and_then(|found| found.gated_at.as_deref())
    else {
        return Turn::Unmeasurable;
    };
    let mut ahead: Option<(&str, &str)> = None;
    for entry in entries.iter().filter(|found| found.run != me && may_queue(found.stage, found.gated_at.is_some()) && found.worktree) {
        let (Some(verdict), Some(ts)) = (entry.verdict, entry.gated_at.as_deref()) else {
            return Turn::Unmeasurable;
        };
        let key = (ts, entry.run.as_str());
        if verdict == Verdict::Pass && key < (mine, me) && ahead.is_none_or(|found| key < found) {
            ahead = Some(key);
        }
    }
    ahead.map_or(Turn::First, |(_, run)| Turn::After(run.to_owned()))
}

/// いまの番（[`Completion::LandTurn`] の観測もこの 1 本を通る）。
pub(crate) fn turn_now(state_dir: &Path, run: &str) -> Turn {
    turn_in(queue_of(state_dir).as_deref(), run)
}

/// 列に入りうる段か（**終端でない ∧ `Gated` を 1 度でも通った**）。verdict と worktree の条件は呼び手が
/// 重ねる（読めない判定を列から黙って外さないため・[`turn_in`]）。
fn may_queue(stage: Stage, gated: bool) -> bool {
    gated && !matches!(stage, Stage::Landed | Stage::Failed | Stage::Stopped)
}

/// 便ごとの**最初の** `Gated` event の ts（列の鍵・**pure**）。追記だけの log を先頭から読み、1 度目だけを
/// 採る＝追随の撃ち直しが `Gated` を足しても鍵は動かない（最新の ts を鍵にすると、撃ち直した便が
/// 後から Gated になった便の後ろへ回る）。
fn first_gated_at(events: &[Event]) -> BTreeMap<&str, &str> {
    let mut first: BTreeMap<&str, &str> = BTreeMap::new();
    for event in events.iter().filter(|event| event.stage == Some(Stage::Gated)) {
        first.entry(event.run.as_str()).or_insert(event.ts.as_str());
    }
    first
}

/// 同じ置き場の便を replay して列の材料を組む。**store を読めない周は `None`**（空の列に読み替えない）。
///
/// 段は replay（[`replay`]）、判定は [`verdict_of`] の 1 本、`Gated` の ts は追記だけの log の原本から
/// 読む（[`first_gated_at`]・replay の `Run::updated` は `Gated` の後の自由文でも動く）。判定と worktree は
/// 列に入りうる便（[`may_queue`]）だけ読み、worktree の実在は便の写し面の repo から導く——repo を
/// 読めない便が在る周も `None` へ倒す。
fn queue_of(state_dir: &Path) -> Option<Vec<Queued>> {
    let events = store::read_all(state_dir).ok()?;
    let gated_at = first_gated_at(&events);
    let state = replay(&events);
    let mut queue = Vec::new();
    for (id, run) in &state.runs {
        let first = gated_at.get(id.as_str()).map(|ts| (*ts).to_owned());
        let open = may_queue(run.stage, first.is_some());
        let worktree = match open {
            true => worktree_path(&super::repo_of_run(state_dir, id)?, id).is_dir(),
            false => false,
        };
        queue.push(Queued {
            run: id.clone(),
            stage: run.stage,
            verdict: open.then(|| verdict_of(state_dir, id)).flatten(),
            gated_at: first,
            worktree,
        });
    }
    Some(queue)
}

/// land の record と stdout の `order=`（設計 gate-cost.md §6・**閉じた 4 値**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    /// 待ち無し。
    First,
    /// 列の前が空くのを待った（秒）。
    Waited(u64),
    /// 上限（rules 行 `pipe.land_wait_s`）で待つのを止めて進んだ。
    Degraded,
    /// 列を導けなかった（読めないを「列なし」に読み替えず、その事実を残して進む）。
    Unmeasured,
}

impl Order {
    /// `order=` の値の字面。
    fn as_value(self) -> String {
        match self {
            Self::First => "first".to_owned(),
            Self::Waited(secs) => format!("waited:{secs}"),
            Self::Degraded => "degraded".to_owned(),
            Self::Unmeasured => "unmeasured".to_owned(),
        }
    }
}

/// 待ちが解けた周の次の手（[`after_wake`] の出力・**閉じた 2 値**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    /// 前の便がまだ居る（残りの上限で同じ完了 enum を待ち直す）。
    KeepWaiting,
    /// 進む（`order=` の値）。
    Proceed(Order),
}

/// 待ちが解けた周に**番を再評価する**（**pure**・run 2 bba45bd の追随 gate FAIL・lens 2026-09-13T04:35Z）。
///
/// [`Completion::LandTurn`] は `After` でない周に満たされる＝`Unmeasurable` の瞬間（前の便が撃ち直しで
/// 判定を書き直す瞬間）でも解ける。解けた後の番を読み直さずに進むと、前の便が列に居るのに進む。
fn after_wake(turn: &Turn, waited_s: u64) -> Next {
    match turn {
        Turn::After(_) => Next::KeepWaiting,
        Turn::First => Next::Proceed(Order::Waited(waited_s)),
        Turn::Unmeasurable => Next::Proceed(Order::Unmeasured),
    }
}

/// 列を見て、自分の番まで待つ（**待ちは唯一の wait 実装を通る**・C3.4・第 2 の poll loop を書かない）。
///
/// 上限を超えた周・列を導けない周は**待たずに進む**（断らない・止めない＝受付の縮退と同じ極性:
/// 詰まって止まるより stale 1 回の費用を払う側に倒す）。待ちは deny の関門ではないので rc を変えない。
/// 待ちが解けた周は [`after_wake`] で番を読み直し、`After` なら**残りの上限で** wait へ再投入する。
fn await_turn(entry: &Land<'_>) -> Order {
    match turn_now(entry.state_dir, entry.run) {
        Turn::First => return Order::First,
        Turn::Unmeasurable => return Order::Unmeasured,
        Turn::After(_) => {}
    }
    let started = Instant::now();
    let limit = Duration::from_secs(entry.land_wait_s);
    loop {
        let turn = Completion::LandTurn {
            state_dir: entry.state_dir.to_path_buf(),
            run: entry.run.to_owned(),
        };
        if crate::fleet::wait(turn, limit.saturating_sub(started.elapsed())) == Err(Timeout) {
            return Order::Degraded;
        }
        match after_wake(&turn_now(entry.state_dir, entry.run), started.elapsed().as_secs()) {
            Next::Proceed(order) => return order,
            Next::KeepWaiting if started.elapsed() >= limit => return Order::Degraded,
            Next::KeepWaiting => {}
        }
    }
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

/// message の 3 部（`s2-07l.130`）を **goal に改行が在る形**で測る歯。
///
/// 契約 file の parser は 1 行 1 値で escape を解かない（`pipe::contract`）ので、e2e の
/// 契約からは改行入りの goal を作れない——「改行を保つ」の側はここで測る。
#[cfg(test)]
mod tests {
    use super::{
        after_wake, first_gated_at, squash_message, subject_of, turn_in, Next, Order, Queued, Turn, SUBJECT_CHARS,
    };
    use crate::fleet::{wait, Completion, Event, EventKind, Stage};
    use crate::pipe::gate::Verdict;
    use std::path::PathBuf;
    use std::time::Duration;

    /// 早い方の `Gated` の ts。
    const EARLY: &str = "2026-09-13T01:00:00Z";
    /// 間の `Gated` の ts。
    const MID: &str = "2026-09-13T01:30:00Z";
    /// 遅い方の `Gated` の ts。
    const LATE: &str = "2026-09-13T02:00:00Z";

    /// 列の fixture 1 本（`ts` が空なら `Gated` を 1 度も通っていない便）。
    fn queued(run: &str, stage: Stage, verdict: Option<Verdict>, ts: &str, worktree: bool) -> Queued {
        Queued {
            run: run.to_owned(),
            stage,
            verdict,
            gated_at: (!ts.is_empty()).then(|| ts.to_owned()),
            worktree,
        }
    }

    /// 自分が最古の `Gated(PASS)` なら `First`・後から `Gated` になった便は `After(自分)`。
    #[test]
    fn pipe_order_first_when_self_is_the_oldest_gated_pass() {
        let queue = [
            queued("b", Stage::Gated, Some(Verdict::Pass), LATE, true),
            queued("a", Stage::Gated, Some(Verdict::Pass), EARLY, true),
        ];
        assert_eq!(turn_in(Some(&queue), "a"), Turn::First, "最古の便は待たない");
        assert_eq!(turn_in(Some(&queue), "b"), Turn::After("a".to_owned()), "後の便は前の便を待つ");
    }

    /// 前に 2 本居る周は**最古の 1 本**を名指す（列の順序は ts の全順序）。
    #[test]
    fn pipe_order_after_names_the_oldest_run_ahead() {
        let queue = [
            queued("me", Stage::Gated, Some(Verdict::Pass), LATE, true),
            queued("mid", Stage::Gated, Some(Verdict::Pass), MID, true),
            queued("early", Stage::Gated, Some(Verdict::Pass), EARLY, true),
        ];
        assert_eq!(turn_in(Some(&queue), "me"), Turn::After("early".to_owned()));
        assert_eq!(turn_in(Some(&queue), "mid"), Turn::After("early".to_owned()));
    }

    /// 列に入らない便: `Gated(FAIL)` / `Gated(INCONCLUSIVE)` / `Gated` を 1 度も通っていない便 /
    /// worktree 不在（retire 済み・判定を読めなくても外れる）/ 終端（Landed / Failed / Stopped・判定が
    /// PASS のまま worktree が在っても外れる）。どれも自分より古くても待たない。
    #[test]
    fn pipe_order_leaves_out_fail_ungated_retired_and_terminal_runs() {
        let queue = [
            queued("fail", Stage::Gated, Some(Verdict::Fail), EARLY, true),
            queued("inconclusive", Stage::Gated, Some(Verdict::Inconclusive), EARLY, true),
            queued("ungated", Stage::Implemented, None, "", true),
            queued("retired", Stage::Gated, Some(Verdict::Pass), EARLY, false),
            queued("retired-unreadable", Stage::Gated, None, EARLY, false),
            queued("landed", Stage::Landed, Some(Verdict::Pass), EARLY, true),
            queued("failed", Stage::Failed, Some(Verdict::Pass), EARLY, true),
            queued("stopped", Stage::Stopped, None, EARLY, true),
            queued("me", Stage::Gated, Some(Verdict::Pass), LATE, true),
        ];
        assert_eq!(turn_in(Some(&queue), "me"), Turn::First);
        // 負例の対: 同じ列に古い `Gated(PASS)` を 1 本足すと待つ（上の First が空虚でない）。
        let mut with_front = queue.to_vec();
        with_front.push(queued("front", Stage::Gated, Some(Verdict::Pass), MID, true));
        assert_eq!(turn_in(Some(&with_front), "me"), Turn::After("front".to_owned()));
    }

    /// 追随して段が `Implemented` へ戻り撃ち直している便（前の周の PASS が残る）は**列に残る**＝撃ち直しの
    /// 間に後続が番を得ない（lens の指摘 2026-09-13T04:05Z）。撃ち直しが FAIL なら外れる。
    #[test]
    fn pipe_order_regating_run_stays_in_the_queue_until_it_fails() {
        let regating = [
            queued("front", Stage::Implemented, Some(Verdict::Pass), EARLY, true),
            queued("me", Stage::Gated, Some(Verdict::Pass), LATE, true),
        ];
        assert_eq!(turn_in(Some(&regating), "me"), Turn::After("front".to_owned()), "撃ち直し中の前の便を待つ");
        let failed = [
            queued("front", Stage::Gated, Some(Verdict::Fail), EARLY, true),
            queued("me", Stage::Gated, Some(Verdict::Pass), LATE, true),
        ];
        assert_eq!(turn_in(Some(&failed), "me"), Turn::First, "撃ち直しが FAIL の便は列に居ない");
    }

    /// log の 1 行の fixture（`RunStage` の段と ts だけを持つ）。
    fn event(run: &str, stage: Stage, ts: &str) -> Event {
        Event {
            schema: 1,
            ts: ts.to_owned(),
            kind: EventKind::RunStage,
            run: run.to_owned(),
            bead: "b".to_owned(),
            host: "h".to_owned(),
            actor: "machine".to_owned(),
            stage: Some(stage),
            seat: None,
            pid: None,
            detail: None,
            allowance: None,
        }
    }

    /// 列の鍵は**最初の** `Gated` の ts（撃ち直しで `Gated` が増えても動かない）。`Gated` を通っていない便は鍵を持たない。
    #[test]
    fn pipe_order_key_is_the_first_gated_ts() {
        let events = [
            event("front", Stage::Gated, EARLY),
            event("me", Stage::Gated, MID),
            event("front", Stage::Implemented, LATE),
            event("front", Stage::Gated, LATE),
            event("never", Stage::Implemented, EARLY),
        ];
        let first = first_gated_at(&events);
        assert_eq!(first.get("front").copied(), Some(EARLY), "撃ち直しの Gated（LATE）で鍵が動かない");
        assert_eq!(first.get("me").copied(), Some(MID));
        assert_eq!(first.get("never"), None, "Gated を通っていない便");
    }

    /// 同時刻は run id の辞書順（全順序＝2 本が互いを待つ循環が起きない）。
    #[test]
    fn pipe_order_same_second_breaks_by_run_id() {
        let queue = [
            queued("s2-3ax-1", Stage::Gated, Some(Verdict::Pass), EARLY, true),
            queued("s2-2e5-1", Stage::Gated, Some(Verdict::Pass), EARLY, true),
        ];
        assert_eq!(turn_in(Some(&queue), "s2-2e5-1"), Turn::First);
        assert_eq!(turn_in(Some(&queue), "s2-3ax-1"), Turn::After("s2-2e5-1".to_owned()));
    }

    /// 読めない周は `Unmeasurable`（**`First` に読み替えない**）: store を読めない・自分が列の材料に
    /// 無い・自分の `Gated` の ts が無い・worktree 在りの `Gated` の便の判定を読めない。
    #[test]
    fn pipe_order_unreadable_is_unmeasurable_not_first() {
        assert_eq!(turn_in(None, "me"), Turn::Unmeasurable, "store を読めない");
        let other = [queued("other", Stage::Landed, None, EARLY, false)];
        assert_eq!(turn_in(Some(&other), "me"), Turn::Unmeasurable, "自分が居ない");
        let no_ts = [queued("me", Stage::Gated, Some(Verdict::Pass), "", true)];
        assert_eq!(turn_in(Some(&no_ts), "me"), Turn::Unmeasurable, "自分の Gated の ts が無い");
        let unreadable = [
            queued("front", Stage::Gated, None, EARLY, true),
            queued("me", Stage::Gated, Some(Verdict::Pass), LATE, true),
        ];
        assert_eq!(turn_in(Some(&unreadable), "me"), Turn::Unmeasurable, "前の便の判定を読めない");
    }

    /// 待ちが解けた周の再評価（lens 2026-09-13T04:35Z）: `After` は待ち直す（`Unmeasurable` の瞬間に
    /// 解けても進まない）・`First` は待った秒・`Unmeasurable` は `unmeasured`（`First` に読み替えない）。
    #[test]
    fn pipe_order_after_wake_rechecks_the_turn() {
        assert_eq!(after_wake(&Turn::After("front".to_owned()), 3), Next::KeepWaiting, "前の便が居れば待ち直す");
        assert_eq!(after_wake(&Turn::First, 3), Next::Proceed(Order::Waited(3)), "番が来た");
        assert_eq!(after_wake(&Turn::Unmeasurable, 3), Next::Proceed(Order::Unmeasured), "列を導けない");
    }

    /// record と stdout の `order=` は閉じた 4 値の字面。
    #[test]
    fn pipe_order_record_values_are_the_closed_four() {
        assert_eq!(Order::First.as_value(), "first");
        assert_eq!(Order::Waited(7).as_value(), "waited:7");
        assert_eq!(Order::Degraded.as_value(), "degraded");
        assert_eq!(Order::Unmeasured.as_value(), "unmeasured");
    }

    /// `Completion::LandTurn` は pid を見張らず（0）、列を導けない周は**満たされた側**（待たずに進む）。
    #[test]
    fn pipe_order_land_turn_is_met_when_the_queue_cannot_be_derived() {
        let turn = Completion::LandTurn {
            state_dir: PathBuf::from("pipe-order-absent-state-dir"),
            run: "me".to_owned(),
        };
        assert_eq!(turn.pid(), 0, "pid を見張らない variant");
        assert_eq!(wait(turn, Duration::ZERO), Ok(()), "自分が列に居ない＝Unmeasurable＝待たない");
    }

    /// 複数行の goal は **本文に逐語**（改行ごと）で載り、件名は先頭の文だけを持つ。
    /// 最終行は `run:` の trailer である。
    #[test]
    fn pipe_land_subject_keeps_multiline_goal_verbatim_in_body() {
        let goal = "## 何を作るか\n- 1 本目の行である。ここは件名に載らない\n- 2 本目の行";
        let message = squash_message("s2-07l.130", goal, "s2-07l.130-1757600000");
        let mut lines = message.lines();
        assert_eq!(lines.next(), Some("s2-07l.130: 何を作るか"), "件名は先頭の文（`#` と空白を除く）");
        assert_eq!(lines.next(), Some(""), "件名の次は空行");
        assert!(message.contains(goal), "goal 全文が逐語で在る: {message}");
        assert_eq!(
            message.lines().last(),
            Some("run: s2-07l.130-1757600000"),
            "最終行は run trailer: {message}"
        );
    }

    /// 要旨が空（goal が空・先頭の文が空白と `#` だけ）の周は **`<bead>` だけ**の件名にして
    /// 落とさない（land を止める理由ではない）。
    #[test]
    fn pipe_land_subject_falls_back_to_bead_when_gist_is_empty() {
        assert_eq!(subject_of("s2-07l.130", ""), "s2-07l.130");
        assert_eq!(subject_of("s2-07l.130", "## \n本文だけ"), "s2-07l.130");
    }

    /// 切るのは **char 単位**である（byte で切ると UTF-8 の途中で割れる）。
    #[test]
    fn pipe_land_subject_cuts_by_chars_not_bytes() {
        let goal = "あ".repeat(SUBJECT_CHARS + 1);
        let subject = subject_of("b", &goal);
        assert_eq!(
            subject.chars().count(),
            "b: ".chars().count() + SUBJECT_CHARS + 1,
            "件名 = `b: ` + 72 文字 + `…`: {subject}"
        );
        assert!(subject.ends_with('…'), "切った印が付く: {subject}");
    }
}
