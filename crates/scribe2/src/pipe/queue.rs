//! 着地の順番待ち（`s2-07l.147`・設計 gate-cost.md §6・pipeline-conflict.md §3・`pipe::land` から呼ぶ）。
//!
//! 列は event log の replay から導く（別の状態 file を持たない）。公開の入口は `pipe::land` の
//! `pub use` が元の path のまま外へ見せる。

use super::gate::Verdict;
use super::land::{verdict_of, Land, MAIN_REF};
use super::{driver_ticket, emit, git_line, git_ok, worktree_path, Emit, Ticket};
use crate::fleet::store;
use crate::fleet::{replay, Completion, Event, EventKind, Run, Stage, State, Timeout};
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
    /// **最新の** [`TURN_TAKEN`] の ts（番を取った周・設計 pipeline.md §22）。番を取った後に PASS でない判定の記帳が
    /// 在る便（列を一度離れた便）と 1 度も番を取っていない便は `None`＝戻ってきた便は番を持たない側から数え直す。
    pub taken_at: Option<String>,
    /// driver の札の状態（設計 pipeline.md §36・行 d の [`driver_ticket`] の同じ 1 本）。列に入りうる便でなければ
    /// 読まない＝`None`（`verdict` と同じ規則）。
    pub driver: Option<Ticket>,
}

/// 着地の番を取った周の記帳の detail（`RunStage stage=Gated`・設計 pipeline.md §22・新しい `EventKind` を足さない）。
pub const TURN_TAKEN: &str = "turn:taken";

/// gate の判定の記帳の detail の頭（`RunStage stage=Gated detail=verdict:<…>`・gate が書く形）。
const VERDICT: &str = "verdict:";

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
///
/// **番を取った便が列に居れば鍵の順より先に立つ**（設計 pipeline.md §22）: 列の便（自分を含む）のうち
/// `taken_at` を持つ便が在れば、最新の `taken_at`（同時刻は run id の辞書順）の 1 本だけが先頭＝撃ち直しの間に
/// 鍵の早い便が戻ってきても、番を取った便を追い抜かない。番を取った便が 1 本も無ければ鍵の順。
///
/// **driver の札が死んでいる便は数えない**（設計 pipeline.md §36・[`turn_skipping`]）。
pub fn turn_in(queue: Option<&[Queued]>, me: &str) -> Turn {
    turn_skipping(queue, me).0
}

/// [`turn_in`] の本体（**pure**・選別はこの 1 本）: 番と、札が死んでいて外した便 id の列（鍵の順）を返す。
///
/// 外すのは札が [`Ticket::Dead`] の便だけで、`Live` / `Absent` / `Unreadable` / 読んでいない（`None`）は従来どおり
/// 数える（測れないを「死んだ」に読み替えない）。自分の札は見ない。外した便として名指すのは、外さなければ自分の
/// 前に立ちえた便（鍵が自分より前か、番を取っていた便）だけ。判定を読めない便は札に依らず `Unmeasurable`（列は空）。
pub fn turn_skipping(queue: Option<&[Queued]>, me: &str) -> (Turn, Vec<String>) {
    let Some(entries) = queue else {
        return (Turn::Unmeasurable, Vec::new());
    };
    let Some(mine) = entries
        .iter()
        .find(|found| found.run == me)
        .and_then(|found| found.gated_at.as_deref())
    else {
        return (Turn::Unmeasurable, Vec::new());
    };
    let mut ahead: Option<(&str, &str)> = None;
    let mut taken: Option<(&str, &str)> = None;
    let mut skipped: Vec<(&str, &str)> = Vec::new();
    for entry in entries.iter().filter(|found| may_queue(found.stage, found.gated_at.is_some()) && found.worktree) {
        let (Some(verdict), Some(ts)) = (entry.verdict, entry.gated_at.as_deref()) else {
            if entry.run == me {
                continue;
            }
            return (Turn::Unmeasurable, Vec::new());
        };
        if verdict != Verdict::Pass {
            continue;
        }
        let key = (ts, entry.run.as_str());
        if entry.run != me && entry.driver == Some(Ticket::Dead) {
            if key < (mine, me) || entry.taken_at.is_some() {
                skipped.push(key);
            }
            continue;
        }
        if let Some(at) = entry.taken_at.as_deref() {
            let latest = |(ts, run): (&str, &str)| at > ts || (at == ts && entry.run.as_str() < run);
            if taken.is_none_or(latest) {
                taken = Some((at, entry.run.as_str()));
            }
        }
        if entry.run != me && key < (mine, me) && ahead.is_none_or(|found| key < found) {
            ahead = Some(key);
        }
    }
    let turn = match taken {
        Some((_, run)) if run == me => Turn::First,
        Some((_, run)) => Turn::After(run.to_owned()),
        None => ahead.map_or(Turn::First, |(_, run)| Turn::After(run.to_owned())),
    };
    skipped.sort_unstable();
    (turn, skipped.into_iter().map(|(_, run)| run.to_owned()).collect())
}

/// いまの番（[`Completion::LandTurn`] の観測もこの 1 本を通る）。
pub(crate) fn turn_now(state_dir: &Path, run: &str) -> Turn {
    skipping_now(state_dir, run).0
}

/// いまの番と外した便（[`turn_skipping`] を置き場の replay に当てる）。
fn skipping_now(state_dir: &Path, run: &str) -> (Turn, Vec<String>) {
    turn_skipping(queue_of(state_dir).as_deref(), run)
}

/// 列の**自分の後ろ**に並ぶ便を鍵の順に `max − 1` 本まで選ぶ（**pure**・設計 pipeline.md §40・行 ah）。
///
/// 数えるのは [`turn_in`] と同じ面（`Gated` を 1 度でも通った ∧ worktree が実在 ∧ 最新の verdict が PASS）で、段は
/// **`Gated` だけ**——終端の便は列に居らず、追随して `Implemented` へ戻り撃ち直している便は自分の land が worktree を
/// 動かしている最中なので候補の木に積まない。鍵は最初の `Gated` の ts（同時刻は run id の辞書順）で、自分の鍵より
/// 大きい便だけを取る。自分が列に居ない・鍵が無い周と `max` が 1 以下の周は空（先頭だけ＝従来の経路）。
pub fn train_in(queue: &[Queued], me: &str, max: u64) -> Vec<String> {
    let Some(mine) = queue.iter().find(|found| found.run == me).and_then(|found| found.gated_at.as_deref()) else {
        return Vec::new();
    };
    let mut behind: Vec<(&str, &str)> = queue
        .iter()
        .filter(|found| found.stage == Stage::Gated && found.worktree && found.verdict == Some(Verdict::Pass))
        .filter_map(|found| found.gated_at.as_deref().map(|ts| (ts, found.run.as_str())))
        .filter(|key| *key > (mine, me))
        .collect();
    behind.sort_unstable();
    let room = usize::try_from(max.saturating_sub(1)).unwrap_or(usize::MAX);
    behind.into_iter().take(room).map(|(_, run)| run.to_owned()).collect()
}

/// いまの列の自分の後ろ（[`train_in`] を置き場の replay に当てる）。**列を導けない周は空**（積まない＝従来の経路）。
pub(super) fn train_now(state_dir: &Path, run: &str, max: u64) -> Vec<String> {
    queue_of(state_dir).map(|queue| train_in(&queue, run, max)).unwrap_or_default()
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

/// 便ごとの**最新の** [`TURN_TAKEN`] の ts（**pure**・設計 pipeline.md §22 約束 4・5）。番を取った後に `verdict:` が PASS で
/// ない `Gated` の記帳（列を離れた周）が在れば消す＝戻ってきた便は番を持たない。撃ち直しの `verdict:PASS` と
/// `stale:` は列に残る周なので消さない。
fn taken_at(events: &[Event]) -> BTreeMap<&str, &str> {
    let mut taken: BTreeMap<&str, &str> = BTreeMap::new();
    for event in events.iter().filter(|event| event.kind == EventKind::RunStage && event.stage == Some(Stage::Gated)) {
        match event.detail.as_deref() {
            Some(TURN_TAKEN) => {
                taken.insert(event.run.as_str(), event.ts.as_str());
            }
            Some(detail) if detail.strip_prefix(VERDICT).is_some_and(|verdict| verdict != Verdict::Pass.as_str()) => {
                taken.remove(event.run.as_str());
            }
            _ => {}
        }
    }
    taken
}

/// 同じ置き場の便を replay して列の材料を組む。**store を読めない周は `None`**（空の列に読み替えない）。
///
/// 段は replay（[`replay`]）、判定は [`verdict_of`] の 1 本、`Gated` の ts は追記だけの log の原本から
/// 読む（[`first_gated_at`]・replay の `Run::updated` は `Gated` の後の自由文でも動く）。判定と worktree と driver の札は
/// 列に入りうる便（[`may_queue`]）だけ読み、worktree の実在は便の写し面の repo から導く——repo を
/// 読めない便が在る周も `None` へ倒す。
fn queue_of(state_dir: &Path) -> Option<Vec<Queued>> {
    queue_from(state_dir, &store::read_all(state_dir).ok()?)
}

/// [`queue_of`] の本体（読んだ log を受けて replay する）。
fn queue_from(state_dir: &Path, events: &[Event]) -> Option<Vec<Queued>> {
    queue_with(state_dir, events, &replay(events))
}

/// [`queue_from`] の本体（replay 済みの現在地を受ける・窓の判定〔[`window_now`]〕が同じ 1 回の replay から
/// 追随中の便も導く＝log を 2 度読まない・設計 pipeline.md §47）。
fn queue_with(state_dir: &Path, events: &[Event], state: &State) -> Option<Vec<Queued>> {
    let gated_at = first_gated_at(events);
    let taken = taken_at(events);
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
            taken_at: taken.get(id.as_str()).map(|ts| (*ts).to_owned()),
            driver: open.then(|| driver_ticket(state_dir, id)),
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
        let state = replay(&events);
        let queued = queue_with(state_dir, &events, &state)?
            .into_iter()
            .filter(|found| {
                may_queue(found.stage, found.gated_at.is_some())
                    && found.worktree
                    && !matches!(found.verdict, Some(Verdict::Fail | Verdict::Inconclusive))
            })
            .map(|found| found.run)
            .collect();
        Some((queued, following_of(&events, &state.runs)))
    });
    Window { runs, main: main_read(repo) }
}

/// 追随中の便（**pure**・設計 pipeline.md §47 が §19 約束 2 (b) を supersede）: 最新の `RunStage` が `Implemented`
/// ∧ detail が [`FOLLOWING`] で始まる ∧ **replay した段が終端でない**（[`may_queue`] と同じ 3 語 `Landed` / `Failed` /
/// `Stopped`）便（run id の辞書順）。`RunStopped` / `RunDone` は kind が `RunStage` でないので最後の `RunStage` は
/// `Implemented rebase:` のまま残る＝終端は同じ 1 回の replay の段（`runs`）で読む（log を 2 度読まない）。終端の
/// 記帳が無い便（runner が死んだ便）は従来どおり数える。
fn following_of(events: &[Event], runs: &BTreeMap<String, Run>) -> Vec<String> {
    let mut last: BTreeMap<&str, &Event> = BTreeMap::new();
    for event in events.iter().filter(|event| event.kind == EventKind::RunStage) {
        last.insert(event.run.as_str(), event);
    }
    last.into_iter()
        .filter(|(run, event)| {
            let terminal = runs.get(*run).is_some_and(|found| matches!(found.stage, Stage::Landed | Stage::Failed | Stage::Stopped));
            event.stage == Some(Stage::Implemented) && event.detail.as_deref().is_some_and(|detail| detail.starts_with(FOLLOWING)) && !terminal
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

/// land の record と stdout の `order=`（設計 gate-cost.md §6・**閉じた 5 値**・`Train` は pipeline.md §40）。
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
    /// 列の先頭が候補の木に積んで一緒に着地させた後続の便（設計 pipeline.md §40・自分では待っていない）。
    Train,
}

impl Order {
    /// `order=` の値の字面。
    pub(super) fn as_value(self) -> String {
        match self {
            Self::First => "first".to_owned(),
            Self::Waited(secs) => format!("waited:{secs}"),
            Self::Degraded => "degraded".to_owned(),
            Self::Unmeasured => "unmeasured".to_owned(),
            Self::Train => "train".to_owned(),
        }
    }

    /// 列の先頭として番を得た周か（`First` / `Waited`・候補の木を積んでよい周・設計 pipeline.md §40）。
    pub(super) fn is_front(self) -> bool {
        matches!(self, Self::First | Self::Waited(_))
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
///
/// 番を取った周（[`Order::is_front`]）は `RunStage stage=Gated detail=turn:taken` を 1 行記す（設計 pipeline.md §22）。
/// 縮退と列を導けなかった周は番を取っていないので書かない。番待ちの間に列の先頭が自分を着地 / 終端させた便（設計 §40・
/// 段が終端）も列に居ないので書かない（`Gated` の記帳が終端の段を上書きしないため）。記帳できない周も順番は変えない
/// （待ちは deny の関門でない）。
///
/// 返り値は `order=` の値と、番を読んだ最後の周に札が死んでいて列から外した便（設計 pipeline.md §36）。
pub(super) fn await_turn(entry: &Land<'_>) -> Turned {
    let (order, skipped_dead) = wait_turn(entry);
    let queued = super::current(entry.state_dir)
        .ok()
        .and_then(|state| state.runs.get(entry.run).map(|run| may_queue(run.stage, true)));
    if order.is_front() && queued == Some(true) {
        let _ = emit(
            entry.state_dir,
            &Emit {
                kind: EventKind::RunStage,
                run: entry.run,
                bead: entry.bead,
                stage: Some(Stage::Gated),
                seat: None,
                pid: None,
                detail: Some(TURN_TAKEN.to_owned()),
            },
            entry.policy,
        );
    }
    Turned { order, skipped_dead }
}

/// 番待ちの結果（[`await_turn`] の出力・設計 pipeline.md §36）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Turned {
    /// `order=` の値。
    pub(super) order: Order,
    /// 札が死んでいて列から外した便 id（鍵の順・外さなかった周は空）。
    pub(super) skipped_dead: Vec<String>,
}

/// [`await_turn`] の待ちの本体（記帳しない）。外した便は番を読んだ最後の周の選別（[`turn_skipping`]）のもの。
fn wait_turn(entry: &Land<'_>) -> (Order, Vec<String>) {
    let (turn, mut skipped) = skipping_now(entry.state_dir, entry.run);
    match turn {
        Turn::First => return (Order::First, skipped),
        Turn::Unmeasurable => return (Order::Unmeasured, skipped),
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
            return (Order::Degraded, skipped);
        }
        let (turn, now) = skipping_now(entry.state_dir, entry.run);
        skipped = now;
        match after_wake(&turn, started.elapsed().as_secs()) {
            Next::Proceed(order) => return (order, skipped),
            Next::KeepWaiting if started.elapsed() >= limit => return (Order::Degraded, skipped),
            Next::KeepWaiting => {}
        }
    }
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.253
    use super::{after_wake, await_turn, first_gated_at, following_of, train_in, turn_in, turn_skipping, Next, Order, Queued, Turn};
    use crate::fleet::store::LockPolicy;
    use crate::fleet::{replay, wait, Completion, Event, EventKind, Stage};
    use crate::pipe::contract::Contract;
    use crate::pipe::fixture::{contract, gated_run, scratch};
    use crate::pipe::gate::{Limits, Verdict};
    use crate::pipe::land::Land;
    use crate::pipe::lens_record::LensSource;
    use crate::pipe::{verdict_path, Ticket};
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
        Land { run: "b-me", bead: "s2-mutant", repo, state_dir: state, contract, pr_cmd: None, lens: &LensSource::Absent, limits, runner: None, retries: 0, land_wait_s: wait_s, ci_wait_s: 0, bd: crate::ledger::DEFAULT_BD, approved: false, policy, train_max: 1 }
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
        assert_eq!(await_turn(&land(&state, &repo, &contract, policy, 0)).order, Order::Degraded, "前の便が居て上限 0");
        gated_run(&state, &repo, "a-front", "FAIL");
        assert_eq!(await_turn(&land(&state, &repo, &contract, policy, 0)).order, Order::First, "前が空なら待たない");
        assert_eq!(await_turn(&land(&absent, &repo, &contract, policy, 0)).order, Order::Unmeasured, "列を導けない");
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
        let order = await_turn(&land(&state, &repo, &contract, policy, 30)).order;
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
            taken_at: None,
            driver: None,
        }
    }

    /// 札の状態を `ticket` にした列の fixture 1 本（段は `Gated`・PASS・worktree 在り）。
    fn driven(run: &str, ts: &str, ticket: Ticket) -> Queued {
        Queued { driver: Some(ticket), ..queued(run, Stage::Gated, Some(Verdict::Pass), ts, true) }
    }

    /// (pure) 札が `Dead` の便だけを外す: 先頭が死んでいれば次の便が先頭で、外した便を鍵の順に名指す。`Live` /
    /// `Absent` / `Unreadable` / 読んでいない（`None`）の先頭は従来どおり `After`（外した便は空）。
    #[test]
    fn pipe_order_dead_front_is_skipped_only_when_the_ticket_is_dead() {
        let dead = [driven("early", EARLY, Ticket::Dead), driven("me", LATE, Ticket::Live)];
        assert_eq!(turn_skipping(Some(&dead), "me"), (Turn::First, vec!["early".to_owned()]), "死んだ先頭は外す");
        for ticket in [Some(Ticket::Live), Some(Ticket::Absent), Some(Ticket::Unreadable), None] {
            let queue = [Queued { driver: ticket, ..driven("early", EARLY, Ticket::Live) }, driven("me", LATE, Ticket::Live)];
            assert_eq!(turn_skipping(Some(&queue), "me"), (Turn::After("early".to_owned()), Vec::new()), "{ticket:?} は従来どおり待つ");
        }
    }

    /// (pure) 3 値 × 鍵の順: 死んだ便を外しても生きている便の順は変わらない（外した後の最古を名指す）。外した便の列は
    /// 鍵の順（与えた並びに依らない）・自分より後ろで番を持たない死んだ便は名指さない・自分の札は見ない。
    #[test]
    fn pipe_order_dead_keeps_key_order_among_the_living() {
        let queue = [
            driven("mid-dead", MID, Ticket::Dead),
            driven("early-dead", EARLY, Ticket::Dead),
            driven("mid-live", MID, Ticket::Absent),
            driven("me", LATE, Ticket::Dead),
            driven("later-dead", "2026-09-13T03:00:00Z", Ticket::Dead),
        ];
        let expected = (Turn::After("mid-live".to_owned()), vec!["early-dead".to_owned(), "mid-dead".to_owned()]);
        assert_eq!(turn_skipping(Some(&queue), "me"), expected, "生きている最古を待ち、前の死んだ 2 本を鍵の順に名指す");
        let front = (Turn::First, vec!["early-dead".to_owned(), "mid-dead".to_owned()]);
        assert_eq!(turn_skipping(Some(&queue), "mid-live"), front, "同時刻は id 順で前の死んだ便まで・後ろの死んだ便は名指さない");
        assert_eq!(turn_in(Some(&queue), "me"), Turn::After("mid-live".to_owned()), "turn_in も同じ選別");
    }

    /// (pure) 番を取ったまま死んだ便（自分より鍵が後ろでも）は先頭に立たず、外した便として名指す。判定を読めない便は
    /// 札が死んでいても `Unmeasurable`（外した便は空・fail-closed）。
    #[test]
    fn pipe_order_dead_taker_and_unreadable_verdict() {
        let taker = [Queued { taken_at: Some(LATE.to_owned()), ..driven("taker", LATE, Ticket::Dead) }, driven("me", MID, Ticket::Live)];
        assert_eq!(turn_skipping(Some(&taker), "me"), (Turn::First, vec!["taker".to_owned()]), "死んだ番の便は先頭に立たない");
        let unreadable = [Queued { verdict: None, ..driven("front", EARLY, Ticket::Dead) }, driven("me", LATE, Ticket::Live)];
        assert_eq!(turn_skipping(Some(&unreadable), "me"), (Turn::Unmeasurable, Vec::new()), "判定を読めない便は札に依らない");
    }

    /// 番を `taken` の ts で取った列の fixture 1 本（段は `Gated`・worktree 在り）。
    fn taking(run: &str, verdict: Option<Verdict>, ts: &str, taken: &str) -> Queued {
        Queued { taken_at: Some(taken.to_owned()), ..queued(run, Stage::Gated, verdict, ts, true) }
    }

    /// (3) 番を取った便が列に居れば最新の 1 本だけが先頭（鍵の順に依らない）: 鍵の早い便も番を取った便を待ち、番を
    /// 取った便の中では最新の ts が勝つ。
    #[test]
    fn pipe_order_taken_latest_run_is_the_front() {
        let queue = [
            queued("early", Stage::Gated, Some(Verdict::Pass), EARLY, true),
            taking("mid", Some(Verdict::Pass), MID, MID),
            taking("late", Some(Verdict::Pass), LATE, LATE),
        ];
        assert_eq!(turn_in(Some(&queue), "late"), Turn::First, "最新の番を取った便は待たない");
        assert_eq!(turn_in(Some(&queue), "early"), Turn::After("late".to_owned()), "鍵の早い便も番を取った便を待つ");
        assert_eq!(turn_in(Some(&queue), "mid"), Turn::After("late".to_owned()), "古い番は最新の番に負ける");
    }

    /// (3) 同時刻の番は run id の辞書順（最小の id が先頭）＝2 本が互いを先頭と読まない。
    #[test]
    fn pipe_order_taken_latest_same_ts_breaks_by_run_id() {
        let queue = [taking("b", Some(Verdict::Pass), EARLY, LATE), taking("a", Some(Verdict::Pass), MID, LATE)];
        assert_eq!(turn_in(Some(&queue), "a"), Turn::First);
        assert_eq!(turn_in(Some(&queue), "b"), Turn::After("a".to_owned()));
    }

    /// (3) 番を取った便が列に 1 本も無ければ鍵の順に戻る（上の歯と同じ列から番だけを外す）。
    #[test]
    fn pipe_order_taken_falls_back_to_key_order() {
        let queue = [
            queued("early", Stage::Gated, Some(Verdict::Pass), EARLY, true),
            queued("mid", Stage::Gated, Some(Verdict::Pass), MID, true),
            queued("late", Stage::Gated, Some(Verdict::Pass), LATE, true),
        ];
        assert_eq!(turn_in(Some(&queue), "early"), Turn::First);
        assert_eq!(turn_in(Some(&queue), "late"), Turn::After("early".to_owned()));
    }

    /// (4) 列を離れた便（終端・worktree 無し・PASS でない）の番は数えない＝鍵の順に戻る。負例の対: 同じ列に PASS で
    /// 列に居る番の便を 1 本足すと、その便が先頭になる（上の鍵の順が空虚でない）。
    #[test]
    fn pipe_order_taken_leaver_is_not_counted() {
        let leavers = [
            Queued { stage: Stage::Landed, ..taking("landed", Some(Verdict::Pass), EARLY, LATE) },
            Queued { stage: Stage::Stopped, ..taking("stopped", Some(Verdict::Pass), EARLY, LATE) },
            Queued { worktree: false, ..taking("retired", Some(Verdict::Pass), EARLY, LATE) },
            taking("inconclusive", Some(Verdict::Inconclusive), EARLY, LATE),
            taking("fail", Some(Verdict::Fail), EARLY, LATE),
            queued("early", Stage::Gated, Some(Verdict::Pass), EARLY, true),
            queued("me", Stage::Gated, Some(Verdict::Pass), MID, true),
        ];
        assert_eq!(turn_in(Some(&leavers), "me"), Turn::After("early".to_owned()), "離れた便の番は数えず鍵の順");
        assert_eq!(turn_in(Some(&leavers), "early"), Turn::First);
        let mut with_taker = leavers.to_vec();
        with_taker.push(taking("taker", Some(Verdict::Pass), LATE, MID));
        assert_eq!(turn_in(Some(&with_taker), "early"), Turn::After("taker".to_owned()), "列に居る番の便は数える");
    }

    /// (4) INCONCLUSIVE で離れて戻った便（鍵が早い）は、離れている間に番を取って撃ち直している便を追い抜かない:
    /// 離れた周の `verdict:INCONCLUSIVE` が自分の古い番を消し、戻った `verdict:PASS` は番を返さない。
    #[test]
    fn pipe_order_taken_leaver_does_not_overtake_the_regating_run() {
        let taken = |run: &str, ts: &str| event_with(run, EventKind::RunStage, Stage::Gated, ts, Some("turn:taken"));
        let verdict = |run: &str, ts: &str, value: &str| event_with(run, EventKind::RunStage, Stage::Gated, ts, Some(value));
        let events = [
            verdict("front", EARLY, "verdict:PASS"),
            verdict("regating", EARLY, "verdict:PASS"),
            taken("front", EARLY),
            verdict("front", MID, "verdict:INCONCLUSIVE"),
            taken("regating", MID),
            following("regating", MID),
            verdict("front", LATE, "verdict:PASS"),
        ];
        let found = super::taken_at(&events);
        assert_eq!(found.get("front"), None, "離れた周が番を消す");
        assert_eq!(found.get("regating").copied(), Some(MID), "撃ち直し中の便の番は残る");
        let queue = [
            queued("front", Stage::Gated, Some(Verdict::Pass), EARLY, true),
            Queued { taken_at: Some(MID.to_owned()), ..queued("regating", Stage::Implemented, Some(Verdict::Pass), MID, true) },
        ];
        assert_eq!(turn_in(Some(&queue), "front"), Turn::After("regating".to_owned()), "戻った便は追い抜かない");
        assert_eq!(turn_in(Some(&queue), "regating"), Turn::First, "撃ち直し中の便が先頭のまま");
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
        event_with(run, EventKind::RunStage, stage, ts, None)
    }

    /// log の 1 行の fixture（kind と detail も取る・`RunStopped` / `RunDone` の終端の記帳を並べるため）。
    fn event_with(run: &str, kind: EventKind, stage: Stage, ts: &str, detail: Option<&str>) -> Event {
        Event {
            schema: 1,
            ts: ts.to_owned(),
            kind,
            run: run.to_owned(),
            bead: "b".to_owned(),
            host: "h".to_owned(),
            actor: "machine".to_owned(),
            stage: Some(stage),
            seat: None,
            pid: None,
            detail: detail.map(str::to_owned),
            allowance: None,
            registration: None,
            mark: None,
            account: None,
        }
    }

    /// `Implemented rebase:a..b` の記帳（追随の撃ち直し・§18）。
    fn following(run: &str, ts: &str) -> Event {
        event_with(run, EventKind::RunStage, Stage::Implemented, ts, Some("rebase:a..b"))
    }

    /// 追随中の便を event の列から導く（本番と同じく replay の段を重ねる）。
    fn following_in(events: &[Event]) -> Vec<String> {
        following_of(events, &replay(events).runs)
    }

    /// (a) `Implemented rebase:` の後に `RunStopped`（段 `Stopped`）で止めた便は追随中に数えない（設計 pipeline.md §47・
    /// 止めた便が窓を永久に閉じていた 2026-09-22 の実測）。
    #[test]
    fn pipe_window_following_leaves_out_a_stopped_run() {
        let events = [following("stopped", EARLY), event_with("stopped", EventKind::RunStopped, Stage::Stopped, MID, Some("stop"))];
        assert!(following_in(&events).is_empty(), "止めた便は追随中でない");
    }

    /// (b) `Implemented rebase:` の後に `RunDone` `Landed` で終えた便は追随中に数えない。
    #[test]
    fn pipe_window_following_leaves_out_a_landed_run() {
        let events = [following("landed", EARLY), event_with("landed", EventKind::RunDone, Stage::Landed, MID, Some("landed"))];
        assert!(following_in(&events).is_empty(), "終えた便は追随中でない");
    }

    /// (c) `Implemented rebase:` のまま終端の記帳が無い便は従来どおり数える（(a)(b) が「追随中を全部外す」変異でない
    /// ことの対）。止めた便・終えた便と同じ列に並べても、その 1 本だけが残る。
    #[test]
    fn pipe_window_following_keeps_a_run_without_a_terminal_record() {
        let events = [
            following("stopped", EARLY),
            event_with("stopped", EventKind::RunStopped, Stage::Stopped, MID, Some("stop")),
            following("landed", EARLY),
            event_with("landed", EventKind::RunDone, Stage::Landed, MID, Some("landed")),
            following("alive", LATE),
        ];
        assert_eq!(following_in(&events), vec!["alive"], "終端の無い追随中の便だけ");
    }

    /// (d) 最新の `RunStage` が `Implemented` でも detail が `rebase:` で始まらない便・detail の無い便・`Gated` へ
    /// 進んだ便は数えない（不変）。
    #[test]
    fn pipe_window_following_leaves_out_runs_whose_detail_is_not_a_rebase() {
        let events = [
            event_with("plain", EventKind::RunStage, Stage::Implemented, EARLY, Some("done")),
            event("bare", Stage::Implemented, EARLY),
            following("gated", EARLY),
            event("gated", Stage::Gated, MID),
        ];
        assert!(following_in(&events).is_empty(), "rebase: で始まらない便は追随中でない");
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

    /// 列の後ろは**鍵の順**（最初の `Gated` の ts・同時刻は run id）で、与えた並びに依らない。自分より前の便は数えない。
    #[test]
    fn pipe_train_picks_the_runs_behind_in_key_order() {
        let queue = [
            queued("late", Stage::Gated, Some(Verdict::Pass), LATE, true),
            queued("mid-b", Stage::Gated, Some(Verdict::Pass), MID, true),
            queued("ahead", Stage::Gated, Some(Verdict::Pass), EARLY, true),
            queued("mid-a", Stage::Gated, Some(Verdict::Pass), MID, true),
            queued("me", Stage::Gated, Some(Verdict::Pass), EARLY, true),
        ];
        assert_eq!(train_in(&queue, "me", 10), vec!["mid-a", "mid-b", "late"], "鍵の順・同時刻は id 順・前の `ahead` は外す");
        assert_eq!(train_in(&queue, "mid-a", 10), vec!["mid-b", "late"], "同時刻の自分より id の大きい便から");
    }

    /// PASS でない便・worktree の無い便・終端の便・撃ち直し中（`Implemented`）の便・`Gated` を通っていない便は積まない。
    /// 負例の対: 同じ列に `Gated(PASS)` の便を 1 本足すとその 1 本だけが積まれる（上の空が空虚でない）。
    #[test]
    fn pipe_train_leaves_out_non_pass_absent_worktree_and_terminal_runs() {
        let mut queue = vec![
            queued("me", Stage::Gated, Some(Verdict::Pass), EARLY, true),
            queued("fail", Stage::Gated, Some(Verdict::Fail), LATE, true),
            queued("inconclusive", Stage::Gated, Some(Verdict::Inconclusive), LATE, true),
            queued("unreadable", Stage::Gated, None, LATE, true),
            queued("retired", Stage::Gated, Some(Verdict::Pass), LATE, false),
            queued("landed", Stage::Landed, Some(Verdict::Pass), LATE, true),
            queued("failed", Stage::Failed, Some(Verdict::Pass), LATE, true),
            queued("stopped", Stage::Stopped, Some(Verdict::Pass), LATE, true),
            queued("regating", Stage::Implemented, Some(Verdict::Pass), LATE, true),
            queued("ungated", Stage::Implemented, None, "", true),
        ];
        assert!(train_in(&queue, "me", 10).is_empty(), "積める便が無い");
        queue.push(queued("pass", Stage::Gated, Some(Verdict::Pass), LATE, true));
        assert_eq!(train_in(&queue, "me", 10), vec!["pass"], "Gated(PASS) ∧ worktree 在りの 1 本だけ");
    }

    /// 上限は**先頭を含む本数**で切る（`max − 1` 本の後続）。1 以下と 0 は後続 0 本、自分が列に居ない・鍵が無い周も空。
    #[test]
    fn pipe_train_cuts_at_the_limit_including_the_front() {
        let queue = [
            queued("me", Stage::Gated, Some(Verdict::Pass), EARLY, true),
            queued("b", Stage::Gated, Some(Verdict::Pass), MID, true),
            queued("c", Stage::Gated, Some(Verdict::Pass), MID, true),
            queued("d", Stage::Gated, Some(Verdict::Pass), LATE, true),
        ];
        assert_eq!(train_in(&queue, "me", 3), vec!["b", "c"], "上限 3 = 自分 + 2 本");
        assert_eq!(train_in(&queue, "me", 4), vec!["b", "c", "d"], "上限 4 = 自分 + 3 本");
        assert!(train_in(&queue, "me", 1).is_empty(), "上限 1 は先頭だけ");
        assert!(train_in(&queue, "me", 0).is_empty(), "上限 0 も先頭だけ");
        assert!(train_in(&queue, "absent", 4).is_empty(), "列に居ない");
        let no_key = [queued("me", Stage::Gated, Some(Verdict::Pass), "", true), queued("b", Stage::Gated, Some(Verdict::Pass), MID, true)];
        assert!(train_in(&no_key, "me", 4).is_empty(), "自分の鍵が無い");
    }

    /// `order=` の `train` は列の先頭に積まれた後続の値で、先頭として番を得た周は `First` / `Waited` だけ。
    #[test]
    fn pipe_train_order_value_and_front() {
        assert_eq!(Order::Train.as_value(), "train");
        let fronts: Vec<bool> =
            [Order::First, Order::Waited(3), Order::Degraded, Order::Unmeasured, Order::Train].iter().map(|order| order.is_front()).collect();
        assert_eq!(fronts, vec![true, true, false, false, false], "縮退・導けない・積まれた周は先頭でない");
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
