//! 現在地の DATA（`[MAIN]` / `[RUN]` / `[SEAT]` / `[WIN]`・設計 working-memory.md §12.2・ADR-0031 §2.2・
//! SRS FR23・憲法 C10 / C10.2 / C11）。開発 session が退避物の節 2 に手で写していた「走行中の便・席の状態・
//! anchor の sha・前回の退避より後に着地した便」を、器の記録（event log・打刻）と anchor の git から出す。
//!
//! 読み手 3 本（[`read_git`] / [`read_log`] / [`read_seat`]）と**純関数 1 本**（[`derive`]）に分ける。読めない周は
//! `unknown` の語で出し、確認した上での 0（`-NONE`）と混ぜない（C10）。この段の読みが落ちても rebrief の rc 2 には
//! 倒さない（DATA の他の段を殺さない）。GitHub は読まない（設計 §11）。`fleet` / `pipe` は読むだけで触らない
//! （`base` の規則は `pipe::base_of_run` と同文をここに写す＝呼ばない）。

use super::super::{seat_dir, seat_of, state, WM_CONSUMED, WM_PREFIX};
use super::{Marker, Request, UNKNOWN};
use crate::fleet::{replay, store, Event, EventKind, Stage, State};
use crate::seat::role::Role;
use crate::seat::state::SeatState;
use crate::seat::wm::WmDoc;
use std::path::Path;
use std::process::{Command, Stdio};

/// 行に出す sha の桁（`[PLUGIN] binary=` の build 元 commit と同じ 12 桁）。
const SHORT: usize = 12;
/// `Spawned` の detail の接頭辞（`pipe::base_of_run` と同じ字面）。
const BASE_PREFIX: &str = "base:";
/// `Implemented` の detail の接頭辞（追随の後の base）。
const REBASE_PREFIX: &str = "rebase:";
/// `RunDone` ∧ `Landed` の detail の接頭辞（`pipe/land.rs` の `sha:<sha> main:<sha>`）。
const SHA_PREFIX: &str = "sha:";
/// 退避物の frontmatter の退避時刻の key。
const EXTERNALIZED_AT: &str = "externalized_at";
/// HEAD を読めない周の `[MAIN]` の理由。
const HEAD_UNREADABLE: &str = "head-unreadable";

/// anchor の HEAD と `origin/main` の関係（**closed の 5 値**・設計 §12.2）。`unknown` は読めない周の値で、
/// `diverged`（双方に相手に無い commit）とは別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// 左右とも 0。
    Same,
    /// 左 > 0 ∧ 右 0（HEAD にだけ在る commit）。
    Ahead,
    /// 左 0 ∧ 右 > 0（origin/main にだけ在る commit）。
    Behind,
    /// 両方 > 0。
    Diverged,
    /// origin/main が無い・rev-list が落ちた・数が読めない。
    Unknown,
}

impl Origin {
    /// 行の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Same => "same",
            Self::Ahead => "ahead",
            Self::Behind => "behind",
            Self::Diverged => "diverged",
            Self::Unknown => UNKNOWN,
        }
    }
}

/// `[MAIN]` の値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MainStatus {
    /// HEAD が読めた（origin と porcelain は個別に unknown になりうる）。
    Known {
        /// HEAD の短 sha。
        sha: String,
        /// origin/main との関係。
        origin: Origin,
        /// `status --porcelain` の行数（撃てない周は `None`）。
        porcelain: Option<usize>,
    },
    /// HEAD が読めない（anchor が repo でない・git が無い）。値は理由。
    Unknown(&'static str),
}

/// `[RUN]` の 1 便（走行中＝終端でなく畳まれてもいない・[`crate::fleet::Run::is_inflight`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStatus {
    /// 便 id。
    pub id: String,
    /// 物理順で最後に見た段。
    pub stage: Stage,
    /// 便を起こした口座（最後の `SeatSpawned` の `account`・無ければ `None`＝`unknown`）。
    pub account: Option<String>,
    /// base の全桁 sha（`Spawned` の `base:` / `Implemented` の `rebase:..` の new・無ければ `None`＝`unknown`）。
    pub base: Option<String>,
    /// 最後に触れた時刻。
    pub updated: String,
}

/// `[SEAT]` の 1 席（登録 row + 打刻の最終行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatStatus {
    /// tmux target。
    pub target: String,
    /// 役割。
    pub role: Role,
    /// 打刻の最終行の状態（file が無い・読めない周は `None`＝`unknown`・鮮度は判定しない）。
    pub state: Option<SeatState>,
    /// 口座 label。
    pub account: String,
    /// 席が使う model（row に無ければ `None`＝`unknown`）。
    pub model: Option<String>,
}

/// `[WIN]` の 1 便（自席の直近の退避より後に Landed した便）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Win {
    /// 契約の bead id。
    pub bead: String,
    /// Landed の時刻。
    pub landed: String,
    /// land した sha の全桁（`RunDone` ∧ `Landed` の detail `sha:<sha>`・`pr` の周や detail の無い周は `None`＝`unknown`）。
    pub sha: Option<String>,
}

/// `[WIN]` を組めない理由（**closed**・`-NONE` に潰さない・C10）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinUnknown {
    /// 自席の消費済み退避物が 1 つも無い（比較の起点が無い）。
    NoConsumed,
    /// 消費済み退避物は在るが `externalized_at` を 1 つも読めない。
    ConsumedUnreadable,
    /// event log を読めない。
    LogUnreadable,
}

impl WinUnknown {
    /// 行の `reason=`。
    pub fn reason(self) -> &'static str {
        match self {
            Self::NoConsumed => "no-consumed",
            Self::ConsumedUnreadable => "consumed-unreadable",
            Self::LogUnreadable => "log-unreadable",
        }
    }
}

/// 現在地（[`derive`] の出力・[`render`] が行にする）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// anchor の git。
    pub main: MainStatus,
    /// 走行中の便（id 順）。event log を読めない周は `None`（`[RUN-COUNT] n=unknown`・`-NONE` を出さない）。
    pub runs: Option<Vec<RunStatus>>,
    /// 登録 row の席（鍵の順）。event log を読めない周は `None`。
    pub seats: Option<Vec<SeatStatus>>,
    /// 直近の退避より後に Landed した便（時刻順）か、組めない理由。
    pub wins: Result<Vec<Win>, WinUnknown>,
}

/// anchor の git の**生の読み**（4 命令の stdout・失敗は `None`・導出は [`derive`]）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitProbe {
    /// `rev-parse HEAD`。
    pub head: Option<String>,
    /// `rev-parse origin/main`。
    pub origin_main: Option<String>,
    /// `status --porcelain`（空は正当な 0 行）。
    pub porcelain: Option<String>,
    /// `rev-list --left-right --count HEAD...origin/main`（`<左>\t<右>`）。
    pub counts: Option<String>,
}

/// 席の file の読み（打刻と消費済み退避物）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatFiles {
    /// 登録 row の target ごとの打刻の最終行の状態。
    pub stamps: Vec<(String, Option<SeatState>)>,
    /// 自席の直近の消費済み退避物の `externalized_at`。
    pub consumed: Result<String, WinUnknown>,
}

/// [`derive`] の入力（読んだ値だけ・I/O なし）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inputs<'a> {
    /// git の生の読み。
    pub git: &'a GitProbe,
    /// event log の全件（読めない周は `None`）。
    pub log: Option<&'a [Event]>,
    /// target ごとの打刻の状態。
    pub stamps: &'a [(String, Option<SeatState>)],
    /// 比較の起点（自席の直近の退避時刻）。
    pub consumed: &'a Result<String, WinUnknown>,
}

/// 読んで導いて行にする（rebrief の `run` が 1 回呼ぶ・落ちても `Err` にしない）。
pub(super) fn push_lines(request: &Request, lines: &mut Vec<(Marker, String)>) {
    let git = read_git(request.anchor);
    let log = read_log(&request.state_dir.path);
    let targets: Vec<String> = log
        .as_deref()
        .map(|events| replay(events).registrations.values().map(|latest| latest.registration.target.clone()).collect())
        .unwrap_or_default();
    let files = read_seat(request.wm_dir, &request.state_dir.path, request.target, &targets);
    let status = derive(&Inputs { git: &git, log: log.as_deref(), stamps: &files.stamps, consumed: &files.consumed });
    render(&status, lines);
}

/// anchor で git を 4 回撃つ（`hook/vessel.rs` と同じ `git -C` の子 process・stderr は捨てる）。
pub fn read_git(anchor: &Path) -> GitProbe {
    GitProbe {
        head: git_out(anchor, &["rev-parse", "HEAD"]),
        origin_main: git_out(anchor, &["rev-parse", "origin/main"]),
        porcelain: git_out(anchor, &["status", "--porcelain"]),
        counts: git_out(anchor, &["rev-list", "--left-right", "--count", "HEAD...origin/main"]),
    }
}

/// git を 1 回撃ち、rc 0 なら stdout（trim 済み・**空も `Some`**＝`status --porcelain` の 0 行を落とさない）。
fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// event log を 1 回読む（malformed を含む周は `None`＝読めない側・`store::read_all` と同じ極性）。
pub fn read_log(state_dir: &Path) -> Option<Vec<Event>> {
    store::read_all(state_dir).ok()
}

/// 席の file を読む: `targets` の打刻の最終行と、自席の直近の消費済み退避物の `externalized_at`。
pub fn read_seat(wm_dir: &Path, state_dir: &Path, target: &str, targets: &[String]) -> SeatFiles {
    let stamps = targets
        .iter()
        .map(|found| (found.clone(), last_state(&seat_dir(state_dir, found))))
        .collect();
    SeatFiles { stamps, consumed: consumed_since(wm_dir, target) }
}

/// 打刻の最終行の状態（不在・読めない・壊れた行は `None`・`sid_of` と同じ行の読み・鮮度は見ない）。
fn last_state(seat: &Path) -> Option<SeatState> {
    let text = std::fs::read_to_string(state::path(seat)).ok()?;
    let line = text.lines().rev().find(|line| !line.trim().is_empty())?;
    state::Stamp::from_line(line).ok().map(|stamp| stamp.state)
}

/// 自席（frontmatter の `seat:` が `target`）の消費済み退避物のうち、読めた `externalized_at` の最大。読めた
/// file が 0 で在る file が 1 以上なら `ConsumedUnreadable`・1 つも無ければ `NoConsumed`。
fn consumed_since(wm_dir: &Path, target: &str) -> Result<String, WinUnknown> {
    let entries = std::fs::read_dir(wm_dir).map_err(|_| WinUnknown::ConsumedUnreadable)?;
    let (mut latest, mut seen) = (None::<String>, false);
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with(WM_PREFIX) && name.ends_with(WM_CONSUMED)) || seat_of(&entry.path()).as_deref() != Some(target) {
            continue;
        }
        seen = true;
        if let Some(at) = externalized_at(&entry.path()) {
            latest = Some(latest.map_or(at.clone(), |found| found.max(at)));
        }
    }
    match latest {
        Some(at) => Ok(at),
        None if seen => Err(WinUnknown::ConsumedUnreadable),
        None => Err(WinUnknown::NoConsumed),
    }
}

/// 退避物の frontmatter の `externalized_at`（読めない・無い・空は `None`）。
fn externalized_at(path: &Path) -> Option<String> {
    let doc = WmDoc::parse(&std::fs::read_to_string(path).ok()?).ok()?;
    doc.value(EXTERNALIZED_AT).map(str::trim).filter(|at| !at.is_empty()).map(str::to_owned)
}

/// 読んだ値から現在地を導く（**純関数・I/O なし**）。
pub fn derive(inputs: &Inputs<'_>) -> Status {
    let main = main_of(inputs.git);
    let Some(events) = inputs.log else {
        return Status { main, runs: None, seats: None, wins: Err(WinUnknown::LogUnreadable) };
    };
    let state = replay(events);
    let runs = state
        .runs
        .values()
        .filter(|run| run.is_inflight())
        .map(|run| RunStatus {
            id: run.id.clone(),
            stage: run.stage,
            account: account_of(events, &run.id),
            base: base_of(events, &run.id),
            updated: run.updated.clone(),
        })
        .collect();
    let seats = state
        .registrations
        .values()
        .map(|latest| {
            let row = &latest.registration;
            SeatStatus {
                target: row.target.clone(),
                role: row.role,
                state: stamp_of(inputs.stamps, &row.target),
                account: row.account.clone(),
                model: row.model.clone(),
            }
        })
        .collect();
    let wins = match inputs.consumed {
        Ok(since) => Ok(wins_of(events, &state, since)),
        Err(reason) => Err(*reason),
    };
    Status { main, runs: Some(runs), seats: Some(seats), wins }
}

/// `[MAIN]` の値: HEAD が無ければ全部 unknown。origin は origin/main が在り rev-list が読めた周だけ導く。
fn main_of(git: &GitProbe) -> MainStatus {
    let Some(head) = &git.head else {
        return MainStatus::Unknown(HEAD_UNREADABLE);
    };
    let origin = match (&git.origin_main, &git.counts) {
        (Some(_), Some(counts)) => origin_of(counts),
        _ => Origin::Unknown,
    };
    MainStatus::Known {
        sha: short(head).to_owned(),
        origin,
        porcelain: git.porcelain.as_deref().map(|text| text.lines().count()),
    }
}

/// `rev-list --left-right --count HEAD...origin/main` の `<左>\t<右>` から origin の 5 値を導く（純関数）。
/// 2 数でない・数でない周は `Unknown`（0 に読み替えない）。
pub fn origin_of(counts: &str) -> Origin {
    let mut parts = counts.split_whitespace();
    let (Some(left), Some(right), None) = (parts.next(), parts.next(), parts.next()) else {
        return Origin::Unknown;
    };
    match (left.parse::<u64>(), right.parse::<u64>()) {
        (Ok(0), Ok(0)) => Origin::Same,
        (Ok(_), Ok(0)) => Origin::Ahead,
        (Ok(0), Ok(_)) => Origin::Behind,
        (Ok(_), Ok(_)) => Origin::Diverged,
        _ => Origin::Unknown,
    }
}

/// 便を起こした口座（物理順で最後の `SeatSpawned` の `account`・field の無い行は `None`）。
fn account_of(events: &[Event], id: &str) -> Option<String> {
    events
        .iter()
        .rev()
        .find(|event| event.run == id && event.kind == EventKind::SeatSpawned)
        .and_then(|event| event.account.clone())
}

/// 便の base（`pipe::base_of_run` と同じ規則の写し）: 物理順で後ろから、`Spawned` の `base:<sha>[,…]` の sha か
/// `Implemented` の `rebase:<old>..<new>` の new（段は等値で見る＝`Stage` の arm を持たず、`Stage` の閉包に入らない）。
fn base_of(events: &[Event], id: &str) -> Option<String> {
    events.iter().rev().filter(|event| event.run == id).find_map(|event| {
        let detail = event.detail.as_deref()?;
        if event.stage == Some(Stage::Spawned) {
            detail.strip_prefix(BASE_PREFIX).map(|rest| rest.split_once(',').map_or(rest, |(sha, _)| sha).to_owned())
        } else if event.stage == Some(Stage::Implemented) {
            detail.strip_prefix(REBASE_PREFIX).and_then(|range| range.split_once("..")).map(|(_, new)| new.to_owned())
        } else {
            None
        }
    })
}

/// target の打刻の状態（読みの列に無い target は `None`）。
fn stamp_of(stamps: &[(String, Option<SeatState>)], target: &str) -> Option<SeatState> {
    stamps.iter().find(|(found, _)| found == target).and_then(|(_, state)| *state)
}

/// `since` より**後**に Landed した便（`State.runs` の段が `Landed` で、その `RunDone` ∧ `Landed` の ts > since・
/// 時刻は同じ `YYYY-MM-DDTHH:MM:SSZ` の字面で比べる）。sha は同じ event の detail `sha:<sha>`（`pr` の周は `None`）。
fn wins_of(events: &[Event], state: &State, since: &str) -> Vec<Win> {
    let mut wins: Vec<Win> = state
        .runs
        .values()
        .filter(|run| run.stage == Stage::Landed)
        .filter_map(|run| {
            let done = events
                .iter()
                .rev()
                .find(|event| event.run == run.id && event.kind == EventKind::RunDone && event.stage == Some(Stage::Landed));
            let landed = done.map_or(run.updated.as_str(), |event| event.ts.as_str());
            (landed > since).then(|| Win {
                bead: run.bead.clone(),
                landed: landed.to_owned(),
                sha: done.and_then(|event| landed_sha(event.detail.as_deref()?)),
            })
        })
        .collect();
    wins.sort_by(|a, b| (&a.landed, &a.bead).cmp(&(&b.landed, &b.bead)));
    wins
}

/// `sha:<sha>[ main:<sha>]` の sha（接頭辞が違う周＝`pr` は `None`）。
fn landed_sha(detail: &str) -> Option<String> {
    let sha = detail.strip_prefix(SHA_PREFIX)?.split_whitespace().next()?;
    (!sha.is_empty()).then(|| sha.to_owned())
}

/// 全桁 sha を行の桁に落とす（短い字面はそのまま）。
fn short(sha: &str) -> &str {
    sha.get(..SHORT).unwrap_or(sha)
}

/// `Option<String>` を `<値|unknown>` にする。
fn or_unknown(value: Option<&str>) -> &str {
    value.unwrap_or(UNKNOWN)
}

/// 現在地を行にする（marker ごと・件数と列挙は対・読めない周は `unknown`）。
pub fn render(status: &Status, lines: &mut Vec<(Marker, String)>) {
    lines.push((Marker::Main, main_line(&status.main)));
    match &status.runs {
        None => lines.push((Marker::RunCount, format!("n={UNKNOWN}"))),
        Some(runs) => {
            lines.extend(runs.iter().map(|run| {
                let body = format!(
                    "id={} stage={} account={} base={} updated={}",
                    run.id,
                    run.stage.as_str(),
                    or_unknown(run.account.as_deref()),
                    or_unknown(run.base.as_deref().map(short)),
                    run.updated
                );
                (Marker::Run, body)
            }));
            lines.push((Marker::RunCount, format!("n={}", runs.len())));
            if runs.is_empty() {
                lines.push((Marker::RunNone, String::new()));
            }
        }
    }
    match &status.seats {
        None => lines.push((Marker::SeatCount, format!("n={UNKNOWN}"))),
        Some(seats) => {
            lines.extend(seats.iter().map(|seat| {
                let body = format!(
                    "target={} role={} state={} account={} model={}",
                    seat.target,
                    seat.role.as_str(),
                    or_unknown(seat.state.map(SeatState::as_str)),
                    seat.account,
                    or_unknown(seat.model.as_deref())
                );
                (Marker::Seat, body)
            }));
            lines.push((Marker::SeatCount, format!("n={}", seats.len())));
            if seats.is_empty() {
                lines.push((Marker::SeatNone, String::new()));
            }
        }
    }
    match &status.wins {
        Err(reason) => lines.push((Marker::WinUnknown, format!("reason={}", reason.reason()))),
        Ok(wins) => {
            lines.extend(wins.iter().map(|win| {
                let body = format!("id={} landed={} sha={}", win.bead, win.landed, or_unknown(win.sha.as_deref().map(short)));
                (Marker::Win, body)
            }));
            lines.push((Marker::WinCount, format!("n={}", wins.len())));
            if wins.is_empty() {
                lines.push((Marker::WinNone, String::new()));
            }
        }
    }
}

/// `[MAIN]` の本文。HEAD が読めない周は 3 値とも `unknown` で理由を添える。
fn main_line(main: &MainStatus) -> String {
    match main {
        MainStatus::Known { sha, origin, porcelain } => {
            let porcelain = porcelain.map_or_else(|| UNKNOWN.to_owned(), |n| n.to_string());
            format!("sha={sha} origin={} porcelain={porcelain}", origin.as_str())
        }
        MainStatus::Unknown(reason) => format!("sha={UNKNOWN} origin={UNKNOWN} porcelain={UNKNOWN} reason={reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{derive, render, GitProbe, Inputs, MainStatus, Status, Win, WinUnknown};
    use crate::fleet::{Event, EventKind, Registration, Stage, SCHEMA};
    use crate::seat::rebrief::Marker;
    use crate::seat::role::Role;
    use crate::seat::state::SeatState;

    /// 偽の rev-list の出力で `derive` を撃ち、`[MAIN]` の origin を読む。
    fn origin_line(counts: Option<&str>, origin_main: Option<&str>) -> String {
        let git = GitProbe {
            head: Some("0123456789abcdef0123456789abcdef01234567".to_owned()),
            origin_main: origin_main.map(str::to_owned),
            porcelain: Some(String::new()),
            counts: counts.map(str::to_owned),
        };
        let status = derive(&Inputs { git: &git, log: Some(&[]), stamps: &[], consumed: &Err(WinUnknown::NoConsumed) });
        let mut lines = Vec::new();
        render(&status, &mut lines);
        lines.into_iter().find(|(marker, _)| *marker == Marker::Main).map(|(_, body)| body).unwrap_or_default()
    }

    /// (a0) `rev-list --left-right --count` の左右 2 数から origin の 5 値が pure に出る: `2\t0` = ahead・`0\t3` = behind・
    /// `1\t1` = diverged・`0\t0` = same・読めない（数でない・1 数・rev-list が落ちた・origin/main が無い）= unknown。
    /// sha は 12 桁・porcelain の空は 0 行。
    #[test]
    fn seat_wm_status_reports_main_origin_from_left_right_counts() {
        let origin = Some("fedcba");
        assert_eq!(origin_line(Some("2\t0"), origin), "sha=0123456789ab origin=ahead porcelain=0");
        assert_eq!(origin_line(Some("0\t3"), origin), "sha=0123456789ab origin=behind porcelain=0");
        assert_eq!(origin_line(Some("1\t1"), origin), "sha=0123456789ab origin=diverged porcelain=0");
        assert_eq!(origin_line(Some("0\t0"), origin), "sha=0123456789ab origin=same porcelain=0");
        for (label, counts) in [("数でない", Some("x\t0")), ("1 数", Some("2")), ("3 数", Some("1 2 3")), ("rev-list が落ちた", None)] {
            assert_eq!(origin_line(counts, origin), "sha=0123456789ab origin=unknown porcelain=0", "{label}");
        }
        assert_eq!(origin_line(Some("0\t0"), None), "sha=0123456789ab origin=unknown porcelain=0", "origin/main が無い");
        let absent = derive(&Inputs { git: &GitProbe::default(), log: None, stamps: &[], consumed: &Err(WinUnknown::NoConsumed) });
        assert_eq!(absent.main, MainStatus::Unknown("head-unreadable"));
        let mut lines = Vec::new();
        render(&absent, &mut lines);
        assert_eq!((absent.runs, absent.seats, absent.wins), (None, None, Err(WinUnknown::LogUnreadable)), "log が読めない周");
        assert_eq!(
            lines,
            [
                (Marker::Main, "sha=unknown origin=unknown porcelain=unknown reason=head-unreadable".to_owned()),
                (Marker::RunCount, "n=unknown".to_owned()),
                (Marker::SeatCount, "n=unknown".to_owned()),
                (Marker::WinUnknown, "reason=log-unreadable".to_owned()),
            ],
            "読めない周は -NONE を出さない"
        );
    }

    /// 便の event 1 件（`run` / `bead` 付き・`account` は呼び手が struct update で足す）。
    fn event(ts: &str, kind: EventKind, run: &str, stage: Option<Stage>, detail: Option<&str>) -> Event {
        Event {
            schema: SCHEMA,
            ts: ts.to_owned(),
            kind,
            run: run.to_owned(),
            bead: format!("s2-{run}"),
            host: "h".to_owned(),
            actor: "machine".to_owned(),
            stage,
            seat: None,
            pid: None,
            detail: detail.map(str::to_owned),
            allowance: None,
            registration: None,
            account: None,
        }
    }

    /// 歯の全桁 sha。
    const SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";

    /// 偽の log: 走行中 2 便（r1 = `base:<sha>,account:x` + `SeatSpawned account=x`・r2 = 追随で base が new に）・終端 1 便
    /// （Failed）・畳んだ 1 便（`retired`）・Landed 3 便（w1 は起点と同時刻・w3 は sha 付き・w2 は `pr`）・登録 row 2 席。
    fn log() -> Vec<Event> {
        let (spawned, landed) = (format!("base:{SHA},account:x"), format!("sha:{SHA} main:{SHA}"));
        vec![
            event("2026-09-16T01:00:00Z", EventKind::RunStage, "r1", Some(Stage::Spawned), Some(&spawned)),
            Event { account: Some("x".to_owned()), ..event("2026-09-16T01:00:01Z", EventKind::SeatSpawned, "r1", None, None) },
            event("2026-09-16T01:00:02Z", EventKind::RunStage, "r2", Some(Stage::Spawned), Some("base:1111111111111111")),
            event("2026-09-16T01:00:03Z", EventKind::RunStage, "r2", Some(Stage::Implemented), Some("rebase:1111111111111111..2222222222222222")),
            event("2026-09-16T01:00:04Z", EventKind::RunStage, "r3", Some(Stage::Failed), None),
            event("2026-09-16T01:00:05Z", EventKind::RunStage, "r4", Some(Stage::Spawned), Some("retired")),
            event("2026-09-16T01:00:06Z", EventKind::RunDone, "w1", Some(Stage::Landed), Some(&landed)),
            event("2026-09-16T02:00:00Z", EventKind::RunDone, "w2", Some(Stage::Landed), Some("pr")),
            event("2026-09-16T01:30:00Z", EventKind::RunDone, "w3", Some(Stage::Landed), Some(&format!("sha:{SHA}"))),
            registered("2026-09-16T01:00:07Z", "wm:1", Some("Opus")),
            registered("2026-09-16T01:00:08Z", "wm:2", None),
        ]
    }

    /// 上の log を打刻（wm:1 = busy・wm:2 = 無し）と起点 `01:00:06Z` で導いた現在地。
    fn derived(events: &[Event]) -> Status {
        let stamps = vec![("wm:1".to_owned(), Some(SeatState::Busy)), ("wm:2".to_owned(), None)];
        derive(&Inputs { git: &GitProbe::default(), log: Some(events), stamps: &stamps, consumed: &Ok("2026-09-16T01:00:06Z".to_owned()) })
    }

    /// 登録 row の event 1 件（鍵 = role × anchor ゆえ anchor は target ごと）。
    fn registered(ts: &str, target: &str, model: Option<&str>) -> Event {
        let registration = Registration {
            role: Role::Orchestrator,
            anchor: format!("/a/{target}"),
            target: target.to_owned(),
            sid: None,
            account: "a1".to_owned(),
            launch: "cld".to_owned(),
            model: model.map(str::to_owned),
        };
        Event { registration: Some(registration), ..event(ts, EventKind::SeatRegistered, "", None, None) }
    }

    /// 走行中の便と席の導出（純関数）: base は最後の `Spawned` の `base:`（`,` の前まで）か `Implemented` の `rebase:..`
    /// の new、account は最後の `SeatSpawned`、終端（Failed）と `detail=retired` と Landed は `[RUN]` に出ない、`[SEAT]` の
    /// state は打刻の列から・無い席は `None`。空の log は確認した上での 0（`-NONE`）。
    #[test]
    fn seat_wm_status_derives_runs_and_seats_from_the_log() {
        let status = derived(&log());
        let runs: Vec<(String, Option<String>, Option<String>)> =
            status.runs.clone().unwrap_or_default().into_iter().map(|run| (run.id, run.account, run.base)).collect();
        assert_eq!(
            runs,
            [
                ("r1".to_owned(), Some("x".to_owned()), Some(SHA.to_owned())),
                ("r2".to_owned(), None, Some("2222222222222222".to_owned())),
            ],
            "終端と retired は出ない・base は `,` の前まで / rebase の new"
        );
        let seats: Vec<(String, Option<SeatState>, Option<String>)> =
            status.seats.clone().unwrap_or_default().into_iter().map(|seat| (seat.target, seat.state, seat.model)).collect();
        assert_eq!(seats, [("wm:1".to_owned(), Some(SeatState::Busy), Some("Opus".to_owned())), ("wm:2".to_owned(), None, None)]);
        let mut lines = Vec::new();
        render(&status, &mut lines);
        let bodies: Vec<String> = lines.iter().map(|(marker, body)| format!("{} {body}", marker.as_str())).take(7).collect();
        assert_eq!(
            bodies,
            [
                "[MAIN] sha=unknown origin=unknown porcelain=unknown reason=head-unreadable",
                "[RUN] id=r1 stage=Spawned account=x base=abcdef012345 updated=2026-09-16T01:00:01Z",
                "[RUN] id=r2 stage=Implemented account=unknown base=222222222222 updated=2026-09-16T01:00:03Z",
                "[RUN-COUNT] n=2",
                "[SEAT] target=wm:1 role=orchestrator state=busy account=a1 model=Opus",
                "[SEAT] target=wm:2 role=orchestrator state=unknown account=a1 model=unknown",
                "[SEAT-COUNT] n=2",
            ]
        );
        let none = derived(&[]);
        let mut lines = Vec::new();
        render(&none, &mut lines);
        assert_eq!(
            lines.iter().map(|(marker, _)| *marker).collect::<Vec<_>>(),
            [Marker::Main, Marker::RunCount, Marker::RunNone, Marker::SeatCount, Marker::SeatNone, Marker::WinCount, Marker::WinNone],
            "確認した上での 0 は -NONE"
        );
        assert_eq!(none.wins, Ok(Vec::new()));
    }

    /// 着地の導出（純関数）: `[WIN]` は起点より**後**（同時刻は出ない）の Landed だけ・sha は `RunDone` の detail `sha:` から
    /// （`pr` は `None`＝`unknown`）・時刻順。起点が無い / 読めない周はその理由で `WinUnknown`。
    #[test]
    fn seat_wm_status_derives_wins_landed_after_the_consumed_stamp() {
        let events = log();
        let status = derived(&events);
        assert_eq!(
            status.wins,
            Ok(vec![
                Win { bead: "s2-w3".to_owned(), landed: "2026-09-16T01:30:00Z".to_owned(), sha: Some(SHA.to_owned()) },
                Win { bead: "s2-w2".to_owned(), landed: "2026-09-16T02:00:00Z".to_owned(), sha: None },
            ]),
            "起点と同時刻の w1 は出ない・pr は sha None・時刻順"
        );
        let mut lines = Vec::new();
        render(&status, &mut lines);
        let wins: Vec<String> = lines.iter().filter(|(marker, _)| *marker >= Marker::Win).map(|(marker, body)| format!("{} {body}", marker.as_str())).collect();
        assert_eq!(
            wins,
            [
                "[WIN] id=s2-w3 landed=2026-09-16T01:30:00Z sha=abcdef012345",
                "[WIN] id=s2-w2 landed=2026-09-16T02:00:00Z sha=unknown",
                "[WIN-COUNT] n=2",
            ]
        );
        for reason in [WinUnknown::NoConsumed, WinUnknown::ConsumedUnreadable] {
            let unknown = derive(&Inputs { git: &GitProbe::default(), log: Some(&events), stamps: &[], consumed: &Err(reason) });
            assert_eq!(unknown.wins, Err(reason));
            let mut lines = Vec::new();
            render(&unknown, &mut lines);
            assert_eq!(lines.last(), Some(&(Marker::WinUnknown, format!("reason={}", reason.reason()))), "{reason:?}");
            assert!(!lines.iter().any(|(marker, _)| matches!(marker, Marker::Win | Marker::WinCount | Marker::WinNone)), "{reason:?}");
        }
    }
}
