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
use super::lens_record::LensSource;
use super::{
    emit, git_bytes, git_line, git_ok, size, verdict_path, worktree_path, worktrees_dir, Emit,
};
use super::queue::{await_turn, Order};
use super::retire::verdict_field;
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{self, append_line, LockPolicy};
use crate::fleet::{cli::now_utc, EventKind, Stage, SCHEMA};
use std::path::{Path, PathBuf};

pub(crate) use super::queue::turn_now;
pub use super::queue::{turn_in, Queued, Turn};
pub use super::retire::{retire, retired_path, Retire};

/// 進める ref。設計 §5.4 が名指す 1 本である（追随の相手を読む [`super::follow`] も同じ字面を使う）。
pub(crate) const MAIN_REF: &str = "refs/heads/main";

/// 面 5 の export 先の file 名（ADR-0004 §2.2・**版番号に依らず固定**）。
const VERDICTS_FILE: &str = "verdicts.jsonl";

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
    /// main が動いた便の追随で gate を撃ち直す周の lens の出所（`--lens` か run dir の写し・無い / 読めないは
    /// 撃ち直しが INCONCLUSIVE へ倒れ land しない・[`super::lens_record`]・設計 §26）。land 自身は読まず再 gate へ渡す。
    pub lens: &'a LensSource,
    /// 規則から読んだ線（撃ち直しの gate へ渡す・land 自身は数値を見ない）。
    pub limits: Limits,
    /// 追随が衝突した周に runner を起こし直すコマンド（`--runner`）と、その turn の口座を選ぶ入力
    /// （[`follow::Runner`]・設計 account-autonomy.md §4）。**無い周は起こし直さない**。
    pub runner: Option<follow::Runner<'a>>,
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

/// 終端で `refs/heads/main` を読めなかった周の実測値の字面（`main=unknown` / `main:unknown`）。
///
/// 読めないを「一致した」にも「動いた」にも化けさせない（C10）。land 自体は成立している
/// （ref は既に進み実測も緑）ので落とさず、理由は stderr 1 行に残す。
const MAIN_UNKNOWN: &str = "unknown";

/// 終端の直前に `refs/heads/main` を 1 回実測する（設計 §27・`s2-07l.379`）。
///
/// `landed=` / verdicts.jsonl の `sha` は **宣言値**（squash で main に載せた `new`）で、
/// CAS の後に main がさらに動いた周（追随の chain・別の便・手の操作）をそこからは見分けられない。
/// 実測値は宣言値と**別の列**に置く（一致する周も省かない）。読めない周は
/// [`MAIN_UNKNOWN`] と stderr の理由 1 行。
fn measure_main(repo: &Path) -> (String, Vec<String>) {
    match git_line(repo, &["rev-parse", MAIN_REF]) {
        Some(found) => (found, Vec::new()),
        None => (
            MAIN_UNKNOWN.to_owned(),
            vec![format!("pipe: 終端で {MAIN_REF} を読めない（main={MAIN_UNKNOWN}・land は成立している）")],
        ),
    }
}

/// export → `Landed` → 後始末。ここまで来た周は land が成立している（anchor は呼び手が揃え済み）。
///
/// export の前に main を実測し、stdout の `main=` と `Landed` の detail の `main:` に写す
/// （`sha:` は宣言値のまま・verdicts.jsonl の key 列は触らない）。
fn finish(entry: &Land<'_>, worktree: &Path, new: &str, anchor: &AnchorSync, order: Order) -> Outcome {
    let (measured, mut err) = measure_main(entry.repo);
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
            detail: Some(format!("sha:{new} main:{measured}")),
        },
        entry.policy,
    );
    if let Err(err) = emitted {
        return broken(err.to_string());
    }
    // 後始末の失敗は land を取り消さない（**rc 0 のまま stderr 1 行**）。anchor の warning も同じ列。
    err.extend(retire_worktree(entry.repo, entry.run, worktree));
    err.extend(anchor.warning());
    Outcome {
        out: vec![format!(
            "run={} landed={new} main={measured} {} order={}",
            entry.run,
            anchor.token(),
            order.as_value()
        )],
        // 後始末の失敗は land を取り消さない（**rc 0 のまま stderr**）。
        err,
        rc: RC_OK,
    }
}

/// 面 5 の 1 行を `verdicts.jsonl` へ append する（跨版 契約・key 列は固定）。
///
/// `order` は schema 1 のまま足した**任意 field**（ADR-0021 §2.6 (iv)・古い読み手は無視する）で、
/// 列の後ろに置く（既存の 7 key の並びは動かさない）。便の規模の 4 field（[`size::fields`]・git を読めない周は欠く）はその後ろ。
fn export_verdict(entry: &Land<'_>, new: &str, order: Order) -> Result<(), String> {
    let evidence = verdict_path(entry.state_dir, entry.run).display().to_string();
    let mut pairs = vec![
        ("schema", Value::Num(SCHEMA)),
        ("run", Value::Str(entry.run.to_owned())),
        ("bead", Value::Str(entry.bead.to_owned())),
        ("sha", Value::Str(new.to_owned())),
        ("verdict", Value::Str(Verdict::Pass.as_str().to_owned())),
        ("evidence", Value::Str(evidence)),
        ("ts", Value::Str(now_utc())),
        ("order", Value::Str(order.as_value())),
    ];
    let base = super::base_of_run(entry.state_dir, entry.run);
    pairs.extend(size::fields(&entry.contract.size, base.as_deref(), new, |args| git_bytes(entry.repo, args)));
    let line = json_lite::write_object(&pairs);
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
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Dirty => "dirty",
            Self::Unreadable => "unreadable",
        }
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
pub(super) fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
pub(super) fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}

/// message の 3 部（`s2-07l.130`）を **goal に改行が在る形**で測る歯。
///
/// 契約 file の parser は 1 行 1 値で escape を解かない（`pipe::contract`）ので、e2e の
/// 契約からは改行入りの goal を作れない——「改行を保つ」の側はここで測る。
#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.253
    use super::{next_number, skip_record, squash_message, subject_of, SUBJECT_CHARS};

    // flip-check: retroactive s2-07l.222
    /// `next_number` は record 数の次（1 始まり）で、検出線を省いた record を挟む 2 周分でも単調に増える。
    #[test]
    fn mutant_in_pipe_land_next_number_increases_across_two_rounds() {
        assert_eq!((0..4).map(next_number).collect::<Vec<u64>>(), vec![1, 2, 3, 4], "1 始まりの通し番号");
        for (len, n) in [(0, 1), (3, 4)] {
            assert!(skip_record(len, "tree").contains(&format!("\"n\":{n}")), "{}", skip_record(len, "tree"));
        }
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
