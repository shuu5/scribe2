//! 追随（main が動いた便の rebase）の**衝突を runner が解く**段（設計
//! docs/design/pipeline-conflict.md §3 / §5・ADR-0019 §2.2 / §2.4 / §2.6・FR34 / FR14・
//! 憲法 C2 / C3 / C6・NFR4）。
//!
//! 衝突しても**便を終端にしない**。木を `git rebase --abort` で戻し、`RunStage
//! stage=Implemented detail=rebase-conflict:<base>..<main>` を 1 件記帳して、実装役を
//! もう 1 turn 起こす（「main を取り込んで直してから続けて」）。何回まで起こし直すかは
//! rules 行 `pipe.follow_retries` が持ち、**回数は replay から導く**（別の状態 file を
//! 持たない・C3）。
//!
//! **runner を起こす経路はこの module の [`spawn_turn`] ただ 1 本**である（起動そのものは
//! [`super::spawn::spawn`]＝C6 の 1 口）。起こし直しと通常の起動で turn の後始末（[`settle`]）が
//! 分かれると、追随の base 記帳が片方の経路から静かに抜ける——`resume` で起こし直した turn が
//! 新しい base を記帳できないと、次の gate が**古い base の 2 点 diff**を測り、先着便の file を
//! write-set の外と誤る。

use super::contract::Contract;
use super::gate::RC_INCONCLUSIVE;
use super::land::MAIN_REF;
use super::ratelimit::{choose_account, Pool};
use super::spawn::{spawn, Account, Launch};
use super::{base_of_run, emit, git_line, git_ok, question_of_run, worktree_path, Emit, Precheck};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{EventKind, Stage};
use crate::polarity::{OnFailure, Polarity, Timing};
use std::path::Path;

/// この境界の極性（[`FollowCheck`]）: 起こし直す**前**に回数を測って止め（in-loop）、
/// 回数を読めない周は起こし直さない（fail-closed・NFR4）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 上限に達した周の終端の理由（`Failed` の `detail`）。
///
/// 衝突を**記帳する**周の `detail` は同じ語に `:<base>..<main>` を付けた形で、判定は
/// [`is_conflict`] ただ 1 本が持つ（字面を 2 度書かない——片方だけを直すと、起こし直せる
/// 便の集合が静かにずれる）。
pub(crate) const EXHAUSTED: &str = "rebase-conflict";

/// 回数を読めなかった周の終端の理由（**上限到達とは分ける**・rc 2）。
const UNMEASURED: &str = "follow-unmeasured";

/// runner が rebase の途中で turn を終えた周の終端の理由。
const DIRTY: &str = "rebase-dirty";

/// 衝突した木を戻せなかった周の終端の理由。
const ABORT_FAILED: &str = "rebase-abort-failed";

/// 木が rebase の途中であることを示す git の作業 dir（どちらの形も「途中」である）。
const REBASE_DIRS: [&str; 2] = ["rebase-merge", "rebase-apply"];

/// 起こし直しの回数の判定（**閉じた 3 値**・設計 §5 の guard）。
///
/// 終端形は variant ごとに固定である: [`Exhausted`](Self::Exhausted) は
/// `Failed detail=rebase-conflict` + rc 1、[`Unreadable`](Self::Unreadable) は
/// `Failed detail=follow-unmeasured` + rc 2（読めない store は「上限に達した」ではない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowCheck {
    /// 上限の内。もう 1 turn 起こす。
    Retry,
    /// 上限に達した。終端する。
    Exhausted,
    /// 回数を読めない。**起こし直さない**（0 に読み替えない）。
    Unreadable,
}

impl FollowCheck {
    /// 起こし直した回数（replay の導出値・読めない周は `None`）と上限から決める。
    ///
    /// 値 N = **最大 N 回起こし直す**（N+1 回目の衝突で終端する）。
    pub fn judge(retried: Option<u64>, limit: u64) -> Self {
        match retried {
            None => Self::Unreadable,
            Some(found) if found < limit => Self::Retry,
            Some(_) => Self::Exhausted,
        }
    }
}

/// 衝突を記帳した `detail` か（接頭辞 `rebase-conflict:`）。
///
/// **読み手はこの 1 本だけ**である——回数の導出（[`retried`]）と `resume` の弁別
/// （`pipe::cli`）が同じ判定を見る。
pub(crate) fn is_conflict(detail: &str) -> bool {
    detail
        .strip_prefix(EXHAUSTED)
        .is_some_and(|rest| rest.starts_with(':'))
}

/// runner を 1 turn 起こす材料（**通常の起動も起こし直しも同じ形**）。
pub(crate) struct Turn<'a> {
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
    /// runner のコマンドと、その turn の口座を選ぶ入力。**無い周は起こせない**（起こし直しには `--runner` が要る）。
    pub runner: Option<Runner<'a>>,
    /// 承認 event が在るか（replay の導出値）。
    pub approved: bool,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// runner を起こす材料のうち `--runner` に紐づくもの（設計 account-autonomy.md §4・`s2-07l.285`）。
///
/// 口座の選定の入力（[`Pool`]）は runner と**対**で運ぶ——runner を持たない口（`pipe land` の素の形）は
/// 起こし直さないので選定も要らず、runner を持つ口（`pipe run` / `resume` / `--runner` 付きの `land`）は
/// 宣言を 1 回解いてここへ載せる。`pool` が `None` の周は口座の宣言が 0＝親の環境を継承する。
#[derive(Clone, Copy)]
pub struct Runner<'a> {
    /// runner のコマンド（placeholder を含む）。
    pub cmd: &'a str,
    /// 便用の選定の入力（宣言した口座が 1 つ以上在る周だけ・[`Pool::declared`]）。
    pub pool: Option<&'a Pool>,
}

/// 衝突 1 回分の材料（land の追随が渡す）。
pub(crate) struct Conflict<'a> {
    /// 起こし直しの材料。
    pub turn: Turn<'a>,
    /// 便の記録済み base。
    pub base: &'a str,
    /// 追随の相手（land が読んだ main）。
    pub main: &'a str,
    /// 起こし直しの上限（rules 行 `pipe.follow_retries`）。
    pub limit: u64,
}

/// 追随の rebase が衝突した周（設計 §3 の手順 1〜4）。**返り値は必ず land を止める**
/// ——起こし直した turn の後は `gate` から撃ち直す（`land の追随で再び rebase が走る`）。
pub(crate) fn on_conflict(entry: &Conflict<'_>) -> Outcome {
    let worktree = worktree_path(entry.turn.repo, entry.turn.run);
    if !git_ok(&worktree, &["rebase", "--abort"]) {
        return terminate(
            entry,
            ABORT_FAILED,
            RC_BROKEN,
            format!(
                "run {} は rebase の途中のまま（--abort も失敗・base={} main={}）",
                entry.turn.run, entry.base, entry.main
            ),
        );
    }
    let recorded = record(
        &entry.turn,
        Stage::Implemented,
        format!("{EXHAUSTED}:{}..{}", entry.base, entry.main),
    );
    if let Err(reason) = recorded {
        return broken(reason);
    }
    match FollowCheck::judge(retried(entry.turn.state_dir, entry.turn.run), entry.limit) {
        FollowCheck::Unreadable => terminate(
            entry,
            UNMEASURED,
            RC_BROKEN,
            format!("run {} の起こし直しの回数を読めない", entry.turn.run),
        ),
        FollowCheck::Exhausted => terminate(
            entry,
            EXHAUSTED,
            RC_REFUSED,
            format!(
                "run {} の rebase が衝突した（base={} main={}・起こし直しの上限 {} に達した）",
                entry.turn.run, entry.base, entry.main, entry.limit
            ),
        ),
        FollowCheck::Retry => retry(entry),
    }
}

/// 起こし直した回数 = 同じ run の `rebase-conflict:` の `RunStage` の行数 − 1
/// （**いま記帳した分を除く**）。
///
/// **replay の導出値**で、別の状態 file を持たない（C3・設計 §9 の却下案）。store を
/// 読めない周は `None`＝「0 回起こした」に読み替えない（fail-closed）。
fn retried(state_dir: &Path, run: &str) -> Option<u64> {
    let events = store::read_all(state_dir).ok()?;
    let seen = events
        .iter()
        .filter(|event| {
            event.run == run
                && event.kind == EventKind::RunStage
                && event.detail.as_deref().is_some_and(is_conflict)
        })
        .count();
    Some(u64::try_from(seen).unwrap_or(u64::MAX).saturating_sub(1))
}

/// 上限の内の周: runner をもう 1 turn 起こし、**次に撃つ段（gate）を名乗って止まる**。
///
/// 口座は初回の起動と同じ選定（[`spawn_selected`]・設計 account-autonomy.md §4「初回の起動も同じ選定を通す」の
/// 列挙 = 衝突の起こし直し）で選ぶ。待ちの間に便が居るはずの段は、いま記帳した `Implemented`。
fn retry(entry: &Conflict<'_>) -> Outcome {
    let mut outcome = spawn_selected(&entry.turn, Stage::Implemented);
    if outcome.rc != RC_OK {
        return outcome;
    }
    // turn は通った。段が `Implemented` でなければ（runner が commit 無しで落ちた等）
    // その事実は spawn が既に記帳しているので、ここは rc で名乗るだけである。
    let stage = super::current(entry.turn.state_dir)
        .ok()
        .and_then(|state| state.runs.get(entry.turn.run).map(|run| run.stage));
    if stage != Some(Stage::Implemented) {
        outcome.err.push(format!(
            "pipe: run {} の起こし直しは {} で終わった",
            entry.turn.run,
            stage.map_or("読めない", Stage::as_str)
        ));
        outcome.rc = RC_REFUSED;
        return outcome;
    }
    outcome.out.push(format!("run={} next=gate", entry.turn.run));
    outcome.err.push(format!(
        "pipe: run {} の衝突を runner が解いた（次は gate）",
        entry.turn.run
    ));
    outcome.rc = RC_INCONCLUSIVE;
    outcome
}

/// 便用の選定を通した口座で runner を 1 turn 起こす（設計 account-autonomy.md §4「初回の起動も同じ選定を通す」・
/// FR36）。**初回の起動（Intake の審査後・承認後の `Blocked`・回答後の `Questioned`・`Reviewed` の起こし直し）と
/// 衝突の起こし直しはすべてこの 1 本を通る**——操作役の口座で起きる周を残さない。
///
/// 宣言した口座が 1 つ以上在る周（[`Runner::pool`] が `Some`）は [`choose_account`]（計測 → 便用の規則 → 候補なしの
/// 待ち）で label を選んで [`Account::Chosen`]、0 の周は [`Account::Inherit`]（親の環境を継承・stderr に 1 行）。
/// 候補なしの止まり方（rc 3 `run=<id> next=wait reset=…`）は `RateLimited` の再開と同じ字面。初回の起動は
/// `next=spawn account=` の判定行を持たない（名乗るのは再開だけ）。`expected` は待ちの間に便が居るはずの段。
pub(crate) fn spawn_selected(entry: &Turn<'_>, expected: Stage) -> Outcome {
    let Some(runner) = entry.runner else {
        return missing_runner(entry);
    };
    let mut outcome = Outcome::ok(Vec::new());
    let chosen = match runner.pool {
        None => {
            outcome.err.push("pipe: 口座の宣言が無い＝親の環境を継承".to_owned());
            None
        }
        Some(pool) => match choose_account(pool, entry.run, entry.state_dir, expected, &mut outcome) {
            Ok(label) => Some(label),
            // 止まる周も、それまでの判定行（`next=wait …`）と計測の行は残す。
            Err(stopped) => {
                outcome.out.extend(stopped.out);
                outcome.err.extend(stopped.err);
                outcome.rc = stopped.rc;
                return outcome;
            }
        },
    };
    let account = chosen.as_deref().map_or(Account::Inherit, Account::Chosen);
    let turn = spawn_turn(entry, account);
    outcome.out.extend(turn.out);
    outcome.err.extend(turn.err);
    outcome.rc = turn.rc;
    outcome
}

/// `--runner` を持たない周の断り（1 行も書かない・[`spawn_selected`] と [`spawn_turn`] が同じ字面で断る）。
fn missing_runner(entry: &Turn<'_>) -> Outcome {
    refused(format!("run {} の起こし直しに --runner が要る", entry.run))
}

/// runner を 1 turn 起こし、終わったら追随の後始末まで見る。
///
/// **[`super::spawn::spawn`] への呼び手はこの 1 本だけ**である（起動口そのものは spawn で、
/// ここはその唯一の経路＝C6 の形を崩さない）。`--runner` を持たない周は 1 行も書かずに断る
/// ——起こし直しの口（`pipe land` / `pipe resume`）は runner を渡す責務を持つ。
///
/// `account` は runner を起こす口座（閉じた 3 値・設計 account-autonomy.md §4）: 初回の起動と衝突の起こし直しは
/// [`spawn_selected`] が選んだ [`Account::Chosen`]（宣言 0 なら [`Account::Inherit`]）、上限で止まった便の
/// 別口座での起こし直しは [`Account::Resumed`]。
pub(crate) fn spawn_turn(entry: &Turn<'_>, account: Account<'_>) -> Outcome {
    let Some(runner) = entry.runner else {
        return missing_runner(entry);
    };
    let budget = match Precheck::measure(entry.contract, entry.repo) {
        Ok(found) => found.into_budget(),
        Err(reason) => return refused(reason),
    };
    // 回答済みの質問が在る周だけ「回答」節が付く（`Questioned` 以外の段では質問が無く `None`）。
    let answered =
        question_of_run(entry.state_dir, entry.run).filter(|question| question.answer.is_some());
    // 「追随」節の有無は **stdin の組立にだけ**効く。turn の後始末（[`settle`]）は節の有無に
    // 依らず同じ 1 本である（設計 §3 手順 5）。
    let follow = section(entry.state_dir, entry.repo, entry.run);
    // 「途中再開」節も同じく stdin の組立にだけ効く（`RateLimited` の便だけが持つ）。
    let resumed = resumption(entry.state_dir, entry.repo, entry.run);
    let mut outcome = spawn(
        budget,
        &Launch {
            run: entry.run,
            bead: entry.bead,
            repo: entry.repo,
            state_dir: entry.state_dir,
            contract: entry.contract,
            runner: runner.cmd,
            approved: entry.approved,
            answered,
            follow,
            resumed,
            account,
            policy: entry.policy,
        },
    );
    if outcome.rc != RC_OK {
        return outcome;
    }
    let settled = settle(entry);
    outcome.out.extend(settled.out);
    outcome.err.extend(settled.err);
    if settled.rc != RC_OK {
        outcome.rc = settled.rc;
    }
    outcome
}

/// 便が追随すべき相手（main の sha）。追随の要らない周は `None`。
///
/// **「便の base が main の真の祖先である」の 1 条件**で決める。決めるのは runner の
/// stdin の「追随」節（[`super::spawn`]）**だけ**である——turn の後始末（[`settle`]）は
/// この値を見ない（節を渡さなかった turn で runner が自ら rebase した周も同じ 1 本で測る）。
pub(crate) fn section(state_dir: &Path, repo: &Path, run: &str) -> Option<String> {
    let base = base_of_run(state_dir, run)?;
    let main = git_line(repo, &["rev-parse", MAIN_REF])?;
    if main == base {
        return None;
    }
    git_ok(repo, &["merge-base", "--is-ancestor", &base, &main]).then_some(main)
}

/// 「途中再開」節の材料（設計 account-autonomy.md §4）: 止まった時刻と base からの commit の一覧。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resumption {
    /// 上限で止まった時刻（`RunStage(RateLimited)` の event の `ts`）。
    pub stopped_at: String,
    /// base から便が積んだ commit（`git log --oneline <base>..HEAD` の行・古い順）。
    pub commits: Vec<String>,
}

/// 便が途中再開すべきか（`RateLimited` の段に在る周だけ `Some`・[`section`] と同型の組み立て）。
///
/// 決めるのは runner の stdin の「途中再開」節（[`super::spawn`]）**だけ**である。止まった時刻は
/// 追記だけの log の原本から読む（最後の `RateLimited` の行・replay の `updated` は後の event で
/// 動く）。commit の一覧を読めない周は空（節は付く・一覧だけ無い）。
pub(crate) fn resumption(state_dir: &Path, repo: &Path, run: &str) -> Option<Resumption> {
    let events = store::read_all(state_dir).ok()?;
    let stage = crate::fleet::replay(&events).runs.get(run).map(|found| found.stage);
    if stage != Some(Stage::RateLimited) {
        return None;
    }
    let stopped_at = events
        .iter()
        .rev()
        .find(|event| event.run == run && event.kind == EventKind::RunStage && event.stage == Some(Stage::RateLimited))?
        .ts
        .clone();
    let commits = base_of_run(state_dir, run)
        .and_then(|base| {
            let range = format!("{base}..HEAD");
            super::git_bytes(&worktree_path(repo, run), &["log", "--oneline", "--reverse", &range])
        })
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Some(Resumption { stopped_at, commits })
}

/// **すべての turn** の後始末（設計 §3 の手順 5 / 6）。「追随」節を渡したかは見ない。
///
/// 1. 木が rebase の途中（`rebase-merge` / `rebase-apply` 在り）で終わった周は
///    `Failed detail=rebase-dirty` で終端する（clean 前提を守る・fail-closed）。
/// 2. **器が base を進める**: worktree の HEAD と main の merge-base を実測し、記録済みの
///    base より進んでいれば `RunStage stage=Implemented detail=rebase:<old>..<merge-base>` を
///    記帳する。**書く値は実測した merge-base で、現在の main ではない**——turn の間に main が
///    さらに進んでいても、2 点 diff に main の新しい commit の逆向きが載る穴を作らない。
///    節を渡していない turn で runner が頼まれずに `git rebase` を撃った周も**同じ 1 本**で測る
///    ——節の有無で経路を分けると、記帳の無い便の gate が古い base の 2 点 diff を測り、
///    main 側の commit を write-set の外と誤る（.203 の実測）。
///
/// 2 は **turn が `Implemented` で終わった周だけ**である。`Failed` / `RateLimited` で終わった
/// turn の後に `Implemented` を記帳すると、段が静かに `Implemented` へ戻る（終端した便が
/// gate へ進む・止まった便が再開されない）。段を読めない周も記帳しない（fail-closed）。
fn settle(entry: &Turn<'_>) -> Outcome {
    let worktree = worktree_path(entry.repo, entry.run);
    if mid_rebase(&worktree) {
        let recorded = record(entry, Stage::Failed, DIRTY.to_owned());
        return match recorded {
            Err(reason) => broken(reason),
            Ok(()) => refused(format!(
                "run {} の worktree が rebase の途中で turn が終わった",
                entry.run
            )),
        };
    }
    if !ended_implemented(entry) {
        return Outcome::ok(Vec::new());
    }
    let Some((old, advanced)) = advanced(entry) else {
        return Outcome::ok(Vec::new());
    };
    match record(entry, Stage::Implemented, format!("rebase:{old}..{advanced}")) {
        Err(reason) => broken(reason),
        Ok(()) => Outcome::ok_line(format!("run={} rebase={old}..{advanced}", entry.run)),
    }
}

/// turn が `Implemented` で終わったか（replay の段・読めない周は `false`＝base を進めない）。
fn ended_implemented(entry: &Turn<'_>) -> bool {
    super::current(entry.state_dir)
        .ok()
        .and_then(|state| state.runs.get(entry.run).map(|run| run.stage))
        == Some(Stage::Implemented)
}

/// 木が rebase の途中か。**状態を読めない周は「途中」側へ倒す**（fail-closed）。
fn mid_rebase(worktree: &Path) -> bool {
    let Some(dir) = git_line(worktree, &["rev-parse", "--absolute-git-dir"]) else {
        return true;
    };
    REBASE_DIRS
        .iter()
        .any(|name| Path::new(&dir).join(name).exists())
}

/// 記録済みの base と、turn の後に実測した merge-base（**進んでいる周だけ** `Some`）。
fn advanced(entry: &Turn<'_>) -> Option<(String, String)> {
    let old = base_of_run(entry.state_dir, entry.run)?;
    let head = git_line(&worktree_path(entry.repo, entry.run), &["rev-parse", "HEAD"])?;
    let main = git_line(entry.repo, &["rev-parse", MAIN_REF])?;
    let merged = git_line(entry.repo, &["merge-base", &head, &main])?;
    if merged == old || !git_ok(entry.repo, &["merge-base", "--is-ancestor", &old, &merged]) {
        return None;
    }
    Some((old, merged))
}

/// 終端の 1 件を記帳して断る（**終端形は [`FollowCheck`] の variant ごとに固定**・設計 §5）。
fn terminate(entry: &Conflict<'_>, detail: &str, rc: u8, reason: String) -> Outcome {
    match record(&entry.turn, Stage::Failed, detail.to_owned()) {
        Err(broke) => broken(broke),
        Ok(()) => Outcome::failed_line(rc, format!("pipe: {reason}・main は動かさない")),
    }
}

/// 段を 1 件記帳する。
fn record(entry: &Turn<'_>, stage: Stage, detail: String) -> Result<(), String> {
    emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(stage),
            seat: None,
            pid: None,
            detail: Some(detail),
        },
        entry.policy,
    )
    .map_err(|err| err.to_string())
}

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行・何もしない）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::{is_conflict, spawn_turn, FollowCheck, Runner, Turn, DIRTY, EXHAUSTED};
    use crate::cli_outcome::{RC_OK, RC_REFUSED};
    use crate::fleet::store::{self, LockPolicy};
    use crate::fleet::Stage;
    use crate::pipe::approve::RC_BLOCKED;
    use crate::pipe::fixture::{contract, scratch};
    use crate::pipe::spawn::Account;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// git を 1 回撃つ（失敗は読み手の assert が落とす）。
    fn git(dir: &Path, args: &[&str]) {
        let _ = Command::new("git").arg("-C").arg(dir).args(args).output();
    }

    /// commit を 1 つ持つ tmp の git repo（`<root>/repo`）と置き場（`<root>/state`）。
    fn repo_with_state(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = scratch(name);
        let (repo, state) = (root.join("repo"), root.join("state"));
        let _ = std::fs::create_dir_all(&repo);
        let _ = std::fs::create_dir_all(&state);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "mutant"]);
        git(&repo, &["config", "user.email", "mutant@example.invalid"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "seed"]);
        (root, repo, state)
    }

    /// 置き場の `RunStage` の段と detail（物理順）。
    fn stages(state: &Path, run: &str) -> Vec<(Option<Stage>, Option<String>)> {
        store::read_all(state)
            .unwrap_or_default()
            .into_iter()
            .filter(|event| event.run == run && event.stage.is_some())
            .map(|event| (event.stage, event.detail))
            .collect()
    }

    // flip-check: retroactive s2-07l.222
    /// `spawn_turn` の 2 つの `!=` を、spawn の段が turn の後始末の段と一致する周と違う周の 2 fixture で撃つ。
    ///
    /// 1. spawn が起こさずに断った周（承認の関門で `Blocked`・rc 3）は、その rc のまま返り後始末を撃たない
    ///    （1 つ目の `!=` を `==` にすると、無い worktree を「rebase の途中」と読んで `Failed rebase-dirty` を足す）。
    /// 2. spawn は通った（`Implemented`・rc 0）が runner が木を rebase の途中で残した周は、後始末の rc 1 と
    ///    `Failed rebase-dirty` が勝つ（2 つ目の `!=` を `==` にすると rc 0 のまま返る・1 つ目を `==` にすると
    ///    後始末を撃たずに返る）。
    #[test]
    fn mutant_in_pipe_spawn_turn_returns_spawn_or_settle_rc() {
        let (root, repo, state) = repo_with_state("spawn-turn");
        let policy = LockPolicy::embedded().expect("埋め込みの lock 規則を読める");

        let gated = contract(&["src/lib.rs"], &["C9"]);
        let blocked = spawn_turn(
            &Turn {
                run: "blocked",
                bead: "s2-mutant",
                repo: &repo,
                state_dir: &state,
                contract: &gated,
                runner: Some(Runner { cmd: "true", pool: None }),
                approved: false,
                policy,
            },
            Account::Inherit,
        );
        assert_eq!(blocked.rc, RC_BLOCKED, "承認の関門の rc のまま: {:?}", blocked.err);
        assert_eq!(stages(&state, "blocked"), vec![(Some(Stage::Blocked), Some("C9".to_owned()))], "後始末の段を足さない");

        let open = contract(&["src/lib.rs"], &[]);
        let runner = "git commit -q --allow-empty -m runner && mkdir \"$(git rev-parse --absolute-git-dir)/rebase-merge\"";
        let dirty = spawn_turn(
            &Turn {
                run: "dirty",
                bead: "s2-mutant",
                repo: &repo,
                state_dir: &state,
                contract: &open,
                runner: Some(Runner { cmd: runner, pool: None }),
                approved: false,
                policy,
            },
            Account::Inherit,
        );
        assert_ne!(dirty.rc, RC_OK, "後始末の rc が勝つ: {:?}", dirty.err);
        assert_eq!(dirty.rc, RC_REFUSED, "rebase の途中は rc 1: {:?}", dirty.err);
        let trail = stages(&state, "dirty");
        assert!(trail.contains(&(Some(Stage::Implemented), None)), "spawn は Implemented で終わった: {trail:?}");
        assert_eq!(trail.last(), Some(&(Some(Stage::Failed), Some(DIRTY.to_owned()))), "後始末の終端: {trail:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 上限 N は「**最大 N 回起こし直す**」である（N 回目までは `Retry`・N 回目からは
    /// `Exhausted`）。回数を読めない周は `Unreadable` で、**0 回に読み替えない**。
    #[test]
    fn pipe_follow_judge_pins_the_boundary_of_the_limit() {
        for (retried, limit, want) in [
            (0_u64, 2_u64, FollowCheck::Retry),
            (1, 2, FollowCheck::Retry),
            (2, 2, FollowCheck::Exhausted),
            (3, 2, FollowCheck::Exhausted),
            (0, 0, FollowCheck::Exhausted),
            (0, 1, FollowCheck::Retry),
            (1, 1, FollowCheck::Exhausted),
        ] {
            assert_eq!(
                FollowCheck::judge(Some(retried), limit),
                want,
                "起こし直し {retried} 回・上限 {limit}"
            );
        }
        assert_eq!(
            FollowCheck::judge(None, 2),
            FollowCheck::Unreadable,
            "読めない回数は Unreadable（上限の内へ倒さない）"
        );
        assert_eq!(
            FollowCheck::judge(None, 0),
            FollowCheck::Unreadable,
            "上限 0 でも「読めない」は別の値である"
        );
    }

    /// 衝突の記帳（`rebase-conflict:<base>..<main>`）と終端の理由（`rebase-conflict`）を
    /// **同じ判定で混ぜない**。retire と resume はこの弁別の上に乗っている。
    #[test]
    fn pipe_follow_conflict_detail_is_distinguished_from_the_terminal_reason() {
        assert!(is_conflict("rebase-conflict:abc..def"), "記帳した衝突");
        assert!(is_conflict(&format!("{EXHAUSTED}:a..b")), "接頭辞は 1 本から組む");
        assert!(!is_conflict(EXHAUSTED), "終端の理由は衝突の記帳ではない");
        assert!(!is_conflict("rebase-empty"), "同一変更の終端は別の理由");
        assert!(!is_conflict("rebase:abc..def"), "追随の記帳は別の理由");
        assert!(!is_conflict("rebase-conflicted:x"), "接頭辞は `:` まで見る");
    }
}
