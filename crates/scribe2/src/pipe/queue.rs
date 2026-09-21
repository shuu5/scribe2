//! 着地の順番待ち（`s2-07l.147`・設計 gate-cost.md §6・pipeline-conflict.md §3・`pipe::land` から呼ぶ）。
//!
//! 列は event log の replay から導く（別の状態 file を持たない）。公開の入口は `pipe::land` の
//! `pub use` が元の path のまま外へ見せる。

use super::gate::Verdict;
use super::land::{verdict_of, Land, MAIN_REF};
use super::{git_line, git_ok, worktree_path};
use crate::fleet::store;
use crate::fleet::{replay, Completion, Event, EventKind, Stage, Timeout};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

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
    queue_from(state_dir, &store::read_all(state_dir).ok()?)
}

/// [`queue_of`] の本体（読んだ log を受ける・窓の判定〔[`window_now`]〕が同じ 1 回の読みから追随中の便も導く）。
fn queue_from(state_dir: &Path, events: &[Event]) -> Option<Vec<Queued>> {
    let gated_at = first_gated_at(events);
    let state = replay(events);
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

/// 追随の記帳の detail の頭（`RunStage` `Implemented` の `rebase:<old>..<new>`・§18 の追随が書く）。
const FOLLOWING: &str = "rebase:";

/// origin の main（**読むだけで fetch しない**・撃つ側が fetch する・設計 pipeline.md §19 約束 4）。
const ORIGIN_MAIN_REF: &str = "refs/remotes/origin/main";

/// 窓の (c) の読み（local main と origin main・設計 pipeline.md §19 約束 3・**閉じた 4 値**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MainRead {
    /// local の `refs/heads/main` を読めない（origin の有無に依らず閉じる・fail-closed）。
    Unreadable,
    /// origin の main が無い（(c) を数えない・行に `remote=none` を載せる）。
    NoRemote,
    /// local main が origin main の祖先でない＝未 push の squash が在る（値は local main の sha）。
    Unpushed(String),
    /// local main が origin main の祖先である。
    Pushed,
}

/// 着地列の窓（pipeline 外の merge の待ち口・設計 pipeline.md §19・[`Completion::LandWindow`] の観測もこの 1 本）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Window {
    /// (a) 列の PASS の便と (b) 追随中の便（**store を読めない周は `None`**＝閉じる側）。
    runs: Option<(Vec<String>, Vec<String>)>,
    /// (c) の読み。
    main: MainRead,
}

impl Window {
    /// 窓が開いているか: (a)(b) が 0 本 ∧ (c) が数えない周か祖先の周。
    pub(crate) fn is_open(&self) -> bool {
        let quiet = self.runs.as_ref().is_some_and(|(queued, following)| queued.is_empty() && following.is_empty());
        quiet && matches!(self.main, MainRead::NoRemote | MainRead::Pushed)
    }

    /// stdout の 1 行（`land-window=clear` か、列の便と `unpushed=<sha|unreadable|->` を名指す `land-window=busy`）。
    pub(crate) fn line(&self) -> String {
        let remote = match self.main {
            MainRead::NoRemote => " remote=none",
            _ => "",
        };
        if self.is_open() {
            return format!("land-window=clear{remote}");
        }
        let names = |found: Option<&Vec<String>>| match found {
            None => "unreadable".to_owned(),
            Some(runs) if runs.is_empty() => "-".to_owned(),
            Some(runs) => runs.join(","),
        };
        let unpushed = match &self.main {
            MainRead::Unpushed(sha) => sha.as_str(),
            MainRead::Unreadable => "unreadable",
            MainRead::NoRemote | MainRead::Pushed => "-",
        };
        format!(
            "land-window=busy queue={} following={} unpushed={unpushed}{remote}",
            names(self.runs.as_ref().map(|(queued, _)| queued)),
            names(self.runs.as_ref().map(|(_, following)| following)),
        )
    }
}

/// いまの窓（設計 pipeline.md §19 約束 2・3）。列と追随中の便は log の 1 回の読みから導き、git は
/// **local main を先に**読む（読めない周は origin を読まずに閉じる）。列の便は [`turn_in`] と同じ面（終端でない ∧
/// `Gated` を通った ∧ worktree が実在）で、判定を読めない便も数える（PASS でないと測れていない便を外さない・C10）。
pub(crate) fn window_now(state_dir: &Path, repo: &Path) -> Window {
    let runs = store::read_all(state_dir).ok().and_then(|events| {
        let queued = queue_from(state_dir, &events)?
            .into_iter()
            .filter(|found| {
                may_queue(found.stage, found.gated_at.is_some())
                    && found.worktree
                    && !matches!(found.verdict, Some(Verdict::Fail | Verdict::Inconclusive))
            })
            .map(|found| found.run)
            .collect();
        Some((queued, following_of(&events)))
    });
    Window { runs, main: main_read(repo) }
}

/// 追随中の便（**pure**）: 最新の `RunStage` が `Implemented` で detail が [`FOLLOWING`] で始まる便（run id の辞書順）。
fn following_of(events: &[Event]) -> Vec<String> {
    let mut last: BTreeMap<&str, &Event> = BTreeMap::new();
    for event in events.iter().filter(|event| event.kind == EventKind::RunStage) {
        last.insert(event.run.as_str(), event);
    }
    last.into_iter()
        .filter(|(_, event)| {
            event.stage == Some(Stage::Implemented) && event.detail.as_deref().is_some_and(|detail| detail.starts_with(FOLLOWING))
        })
        .map(|(run, _)| run.to_owned())
        .collect()
}

/// (c) の読み（git は既存の 2 口 [`git_line`] / [`git_ok`] だけ・fetch しない）。
fn main_read(repo: &Path) -> MainRead {
    let Some(local) = git_line(repo, &["rev-parse", "--verify", "--quiet", MAIN_REF]) else {
        return MainRead::Unreadable;
    };
    let Some(origin) = git_line(repo, &["rev-parse", "--verify", "--quiet", ORIGIN_MAIN_REF]) else {
        return MainRead::NoRemote;
    };
    match git_ok(repo, &["merge-base", "--is-ancestor", &local, &origin]) {
        true => MainRead::Pushed,
        false => MainRead::Unpushed(local),
    }
}

/// land の record と stdout の `order=`（設計 gate-cost.md §6・**閉じた 4 値**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Order {
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
    pub(super) fn as_value(self) -> String {
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
pub(super) fn await_turn(entry: &Land<'_>) -> Order {
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

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.253
    use super::{after_wake, await_turn, first_gated_at, turn_in, Next, Order, Queued, Turn};
    use crate::fleet::store::LockPolicy;
    use crate::fleet::{wait, Completion, Event, EventKind, Stage};
    use crate::pipe::contract::Contract;
    use crate::pipe::fixture::{contract, gated_run, scratch};
    use crate::pipe::gate::{Limits, Verdict};
    use crate::pipe::land::Land;
    use crate::pipe::lens_record::LensSource;
    use crate::pipe::verdict_path;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    // flip-check: retroactive s2-07l.222
    /// `turn_in` の同時刻 3 便は id の辞書順: 最小は `First`・他は前の最小の id を名指す（`<` → `<=` は id が一意で equivalent）。
    #[test]
    fn mutant_in_pipe_land_turn_in_breaks_the_same_ts_by_run_id() {
        let pass = |run: &str| queued(run, Stage::Gated, Some(Verdict::Pass), EARLY, true);
        let queue = [pass("b"), pass("a"), pass("c")];
        assert_eq!(turn_in(Some(&queue), "a"), Turn::First, "最小の id は待たない");
        assert_eq!(turn_in(Some(&queue), "b"), Turn::After("a".to_owned()), "中の id は最小の id を待つ");
        assert_eq!(turn_in(Some(&queue), "c"), Turn::After("a".to_owned()), "前の 2 本のうち最小の id");
    }

    /// 便 `b-me` の待ちの材料（`land_wait_s` だけを呼び手が選ぶ・待ちは契約と線を読まない）。
    fn land<'a>(state: &'a Path, repo: &'a Path, contract: &'a Contract, policy: LockPolicy, wait_s: u64) -> Land<'a> {
        let limits = Limits {
            lens_count: 0,
            token_cap: 0,
            mutants_jobs: 0,
            job_memory_mb: 0,
            reserve_memory_mb: 0,
            slot_wait_s: 0,
            runnable_per_core: 0,
            blocked_per_core: 0,
        };
        Land { run: "b-me", bead: "s2-mutant", repo, state_dir: state, contract, pr_cmd: None, lens: &LensSource::Absent, limits, runner: None, retries: 0, land_wait_s: wait_s, ci_wait_s: 0, bd: crate::ledger::DEFAULT_BD, approved: false, policy }
    }

    // flip-check: retroactive s2-07l.222
    /// `await_turn` の残り deadline 0 は待ち直さず `Degraded`（前の便が列に居る）・前が空なら `First`・列を導けない
    /// なら `Unmeasured`（負の deadline は u64 で持てず `saturating_sub` で同じ 0）。deadline 0 では `== Err(Timeout)` の
    /// 反転・guard の固定・`>=` → `<` も即 Timeout で同じ `Degraded` に着く＝`==` の反転は下の起こされる歯が撃つ。
    #[test]
    fn mutant_in_pipe_land_await_turn_with_zero_deadline_degrades() {
        let root = scratch("await-zero");
        let (state, repo, absent) = (root.join("state"), root.join("repo"), root.join("absent"));
        let (policy, contract) = (LockPolicy::embedded().expect("埋め込みの lock 規則を読める"), contract(&[], &[]));
        ["a-front", "b-me"].iter().for_each(|run| gated_run(&state, &repo, run, "PASS"));
        assert_eq!(await_turn(&land(&state, &repo, &contract, policy, 0)), Order::Degraded, "前の便が居て上限 0");
        gated_run(&state, &repo, "a-front", "FAIL");
        assert_eq!(await_turn(&land(&state, &repo, &contract, policy, 0)), Order::First, "前が空なら待たない");
        assert_eq!(await_turn(&land(&absent, &repo, &contract, policy, 0)), Order::Unmeasured, "列を導けない");
        let _ = std::fs::remove_dir_all(&root);
    }

    // flip-check: retroactive s2-07l.222
    // flip-check: retroactive s2-07l.357
    /// 待ちの途中で前の便の判定が FAIL に書き直された周は `wait` が `Ok` で解けて `Waited` で進む（`== Err(Timeout)`
    /// を `!=` にすると `Degraded` に化ける）。書き直しは 500 ms 後・上限 30 秒＝壁時計の境界に等号を置かない。
    /// 書き直しは本番の `write_verdict` と同じ `.partial` → `rename` の 2 手（素の write は truncate の瞬間を poll が
    /// 読み `Unmeasured` に化ける＝遅い runner でだけ開く窓・gate-cost.md §6.1）。
    #[test]
    fn mutant_in_pipe_land_await_turn_proceeds_when_the_front_leaves() {
        let root = scratch("await-wake");
        let (state, repo) = (root.join("state"), root.join("repo"));
        let (policy, contract) = (LockPolicy::embedded().expect("埋め込みの lock 規則を読める"), contract(&[], &[]));
        ["a-front", "b-me"].iter().for_each(|run| gated_run(&state, &repo, run, "PASS"));
        let front = verdict_path(&state, "a-front");
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let partial = front.with_extension("json.partial");
            std::fs::write(&partial, "{\"verdict\":\"FAIL\"}\n").and_then(|()| std::fs::rename(&partial, &front)).is_ok()
        });
        let order = await_turn(&land(&state, &repo, &contract, policy, 30));
        assert!(writer.join().unwrap_or(false), "前の便の判定を書き直せた");
        assert!(matches!(order, Order::Waited(_)), "解けた周は待った秒で進む: {order:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

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
            registration: None,
            mark: None,
            account: None,
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
}
