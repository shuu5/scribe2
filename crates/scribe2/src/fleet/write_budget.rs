//! 書き込みの検出線の測り — host の面の表 `[[write-budget]]` が名指す装置の stat file の書いた区の数を、管理 tick の周で表の行ごとに
//! host の根の記録（`open` と `days.log`・schema=1）へ lock の内で 1 回だけ進める（設計 write-budget.md §2〜§4・ADR-0112・FR113）。
//!
//! 記録は装置の累計から取った出所つきの実測で、測れない日は `unmeasured`・一部が欠けた日は `partial` と書き 0 と書かない（C10・NFR4）。
//! 進めの規則は [`advance`]（pure な 1 本）が持ち、早抜けは [`settled`]（pure な 1 本）を lock の外と内の 2 か所が呼ぶ。記録は
//! [`read_open`] の 1 本で読む（読み手は行 b の doctor も同じ口を呼ぶ）。run が受ける manifest は tracked の面で、host の面の表は
//! ここで合わせて読む。標本の 1 本は stdout にも stderr にも書かない。

use super::cli::format_utc;
use super::store::{acquire, LockPolicy};
use super::Event;
use crate::rules::manifest::Manifest;
use crate::rules::{RuleValue, write_budget::WriteBudget};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;

/// 書いた区の 1 つの大きさ（byte・装置の論理 block に依らない kernel の約束）。
const SECTOR_BYTES: u64 = 512;

/// 1 日の秒数（UTC）。
const DAY_S: u64 = 86_400;

/// stat file の 1 行目で書いた区の数が在る欄の添字（7 欄目）。
const SECTORS_AT: usize = 6;

/// 記録の file 名と lock file 名。
const OPEN: &str = "open";
const OPEN_TMP: &str = "open.tmp";
const DAYS: &str = "days.log";
const LOCK: &str = "open.lock";

/// `open` の 10 語の key（この順）。
const OPEN_KEYS: [&str; 10] = ["schema", "stat", "date", "sectors", "at", "written", "state", "reboots", "probed", "probe"];

/// 読みの結果（閉じた 4 語）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Probe {
    /// 読めて数を進めた。
    Ok,
    /// stat file を読めない。
    Unreadable,
    /// 7 欄に満たないか 7 欄目が 10 進でない。
    Malformed,
    /// 記録の stat が表の stat と違う。
    StatMismatch,
}

/// [`Probe`] の全 variant（宣言順）。
const PROBES: &[Probe] = &[Probe::Ok, Probe::Unreadable, Probe::Malformed, Probe::StatMismatch];

impl Probe {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Unreadable => "unreadable",
            Self::Malformed => "malformed",
            Self::StatMismatch => "stat-mismatch",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        PROBES.iter().copied().find(|found| found.as_str() == text)
    }
}

/// 日の測りの状態（閉じた 3 語・`open` は先頭の 2 つだけ持ち、`unmeasured` は `days.log` の欠けた日だけ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Day {
    /// 0 時からの全部を測った日。
    Measured,
    /// 一部が欠けた日（下限）。
    Partial,
    /// 測れなかった日。
    Unmeasured,
}

/// [`Day`] の全 variant（宣言順）。
const DAYS_OF: &[Day] = &[Day::Measured, Day::Partial, Day::Unmeasured];

impl Day {
    fn as_str(self) -> &'static str {
        match self {
            Self::Measured => "measured",
            Self::Partial => "partial",
            Self::Unmeasured => "unmeasured",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        DAYS_OF.iter().copied().find(|found| found.as_str() == text)
    }
}

/// `open`（今日の 1 行）の 10 欄。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Open {
    pub(super) stat: String,
    pub(super) date: String,
    pub(super) sectors: u64,
    pub(super) at: u64,
    pub(super) written: u64,
    pub(super) state: Day,
    pub(super) reboots: u64,
    pub(super) probed: u64,
    pub(super) probe: Probe,
}

/// `open` を読んだ結果（閉じた 3 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Sighting {
    /// 無い。
    Absent,
    /// 在るのに読めない（dir・読めない file・語の数と順と値の字の違い）。
    Unreadable,
    /// 読めた。
    Read(Open),
}

/// 10 進の字だけの 1 語を数にする（符号と空は不可）。
fn decimal(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// UNIX 秒の UTC の日付（`YYYY-MM-DD`・[`format_utc`] の頭 10 字）。
fn date_of(secs: u64) -> String {
    format_utc(secs).chars().take(10).collect()
}

impl Open {
    fn render(&self) -> String {
        format!(
            "schema=1 stat={} date={} sectors={} at={} written={} state={} reboots={} probed={} probe={}\n",
            self.stat,
            self.date,
            self.sectors,
            self.at,
            self.written,
            self.state.as_str(),
            self.reboots,
            self.probed,
            self.probe.as_str()
        )
    }

    /// 10 語が key の順に並び、schema が 1・stat が空でなく・date が at の UTC の日付と等しく・数が 10 進で・state と probe が閉じた語の
    /// ときだけ読める。
    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let line = lines.next()?;
        if lines.next().is_some() {
            return None;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.len() != OPEN_KEYS.len() {
            return None;
        }
        let values: Vec<&str> = words.iter().zip(OPEN_KEYS).map(|(word, key)| word.strip_prefix(key)?.strip_prefix('=')).collect::<Option<_>>()?;
        let [schema, stat, date, sectors, at, written, state, reboots, probed, probe] = values.as_slice() else {
            return None;
        };
        let at = decimal(at)?;
        if *schema != "1" || stat.is_empty() || *date != date_of(at) {
            return None;
        }
        Some(Self {
            stat: (*stat).to_owned(),
            date: (*date).to_owned(),
            sectors: decimal(sectors)?,
            at,
            written: decimal(written)?,
            state: Day::parse(state).filter(|found| *found != Day::Unmeasured)?,
            reboots: decimal(reboots)?,
            probed: decimal(probed)?,
            probe: Probe::parse(probe)?,
        })
    }
}

/// 記録の dir の `open` を読む（読み手の 1 本・行 a の早抜けと進めも行 b の doctor もここを呼ぶ）。
pub(super) fn read_open(dir: &Path) -> Sighting {
    match fs::read_to_string(dir.join(OPEN)) {
        Ok(text) => Open::parse(&text).map_or(Sighting::Unreadable, Sighting::Read),
        Err(err) if err.kind() == ErrorKind::NotFound => Sighting::Absent,
        Err(_) => Sighting::Unreadable,
    }
}

/// 早抜け（pure・lock の外と内が同じ関数を呼ぶ）: 記録が今と同じ UTC 日で、probed + 周期 > 今（加算は飽和）。
pub(super) fn settled(open: &Open, now: u64, period: u64) -> bool {
    open.date == date_of(now) && open.probed.saturating_add(period) > now
}

/// `days.log` の 1 行（written が `-` なのは unmeasured のときだけ・tail は partial の閉じだけ値を持つ）。
fn day_line(date: &str, written: Option<u64>, state: Day, reboots: u64, tail: Option<u64>) -> String {
    let word = |value: Option<u64>| value.map_or_else(|| "-".to_owned(), |found| found.to_string());
    format!("schema=1 date={date} written={} state={} reboots={reboots} tail={}\n", word(written), state.as_str(), word(tail))
}

/// 進め（pure）: 読めた記録（無ければ `None`）・stat の読み・今・周期から、新しい `open` と `days.log` へ足す行の列を返す。
/// 記録の日が今より後（時計が戻った周）は `None`（何も書かない）。
fn advance(prev: Option<&Open>, stat: &str, sectors: u64, now: u64, period: u64) -> Option<(Open, Vec<String>)> {
    let fresh = |state, written, reboots| Open { stat: stat.to_owned(), date: date_of(now), sectors, at: now, written, state, reboots, probed: now, probe: Probe::Ok };
    let Some(prev) = prev else {
        return Some((fresh(Day::Partial, 0, 0), Vec::new()));
    };
    let (rise, fell) = match sectors.checked_sub(prev.sectors) {
        Some(diff) => (diff.saturating_mul(SECTOR_BYTES), false),
        None => (sectors.saturating_mul(SECTOR_BYTES), true),
    };
    if prev.date == date_of(now) {
        let (state, reboots) = if fell { (Day::Partial, prev.reboots.saturating_add(1)) } else { (prev.state, prev.reboots) };
        return Some((fresh(state, prev.written.saturating_add(rise), reboots), Vec::new()));
    }
    let (then, today) = (prev.at / DAY_S, now / DAY_S);
    if then >= today {
        return None;
    }
    let on_time = !fell && today == then + 1 && now.saturating_sub(prev.at) <= period.saturating_mul(2);
    let (closed, opened) = if on_time {
        (day_line(&prev.date, Some(prev.written.saturating_add(rise)), prev.state, prev.reboots, None), fresh(Day::Measured, 0, 0))
    } else {
        (day_line(&prev.date, Some(prev.written), Day::Partial, prev.reboots, Some(rise)), fresh(Day::Partial, 0, u64::from(fell)))
    };
    let mut lines = vec![closed];
    lines.extend((then + 1..today).map(|day| day_line(&date_of(day * DAY_S), None, Day::Unmeasured, 0, None)));
    Some((opened, lines))
}

/// stat file の読み。
enum Reading {
    Sectors(u64),
    Unreadable,
    Malformed,
}

/// stat file の 1 行目を空白で割った 7 欄目（10 進）を読む。
fn read_stat(path: &str) -> Reading {
    let Ok(text) = fs::read_to_string(path) else {
        return Reading::Unreadable;
    };
    let fields: Vec<&str> = text.lines().next().unwrap_or_default().split_whitespace().collect();
    fields.get(SECTORS_AT).copied().and_then(decimal).map_or(Reading::Malformed, Reading::Sectors)
}

/// 1 行ぶんの次の記録: 装置の違い → stat の読み → 進め（読めない・形でない・装置の違いは数を変えず probed と probe だけを書き換え、
/// 記録の無い周に読めないときは何も書かない）。
fn next(prev: Option<Open>, stat: &str, now: u64, period: u64) -> Option<(Open, Vec<String>)> {
    let probed = |prev: Option<Open>, probe| prev.map(|found| (Open { probed: now, probe, ..found }, Vec::new()));
    if prev.as_ref().is_some_and(|found| found.stat != stat) {
        return probed(prev, Probe::StatMismatch);
    }
    match read_stat(stat) {
        Reading::Sectors(sectors) => advance(prev.as_ref(), stat, sectors, now, period),
        Reading::Unreadable => probed(prev, Probe::Unreadable),
        Reading::Malformed => probed(prev, Probe::Malformed),
    }
}

/// lock file を外す（取れた lock だけに掛ける）。
struct Unlock<'a>(&'a Path);

impl Drop for Unlock<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0);
    }
}

/// 閉じた日の行を `days.log` へ追記し、そのあと `open` を `open.tmp` から rename で置く（途中で落ちた周は同じ date の行が重なる＝欠けにしない）。
fn commit(dir: &Path, open: &Open, closed: &[String]) -> Option<()> {
    if !closed.is_empty() {
        let mut log = OpenOptions::new().create(true).append(true).open(dir.join(DAYS)).ok()?;
        log.write_all(closed.concat().as_bytes()).ok()?;
    }
    let temporary = dir.join(OPEN_TMP);
    fs::write(&temporary, open.render()).ok()?;
    fs::rename(&temporary, dir.join(OPEN)).ok()
}

/// 表の 1 行を進める。判定の順: 早抜け → lock → 記録の読み → 早抜け（lock の内）→ 進め。
/// lock を取れない周と、`open` が在るのに読めない周は記録を 1 byte も変えない。
fn measure(row: &WriteBudget, (state_dir, policy): (&Path, LockPolicy), (now, period): (u64, u64)) -> Option<()> {
    let dir = crate::seat::host_write_budget_dir(state_dir, row.name());
    if matches!(read_open(&dir), Sighting::Read(ref open) if settled(open, now, period)) {
        return None;
    }
    fs::create_dir_all(&dir).ok()?;
    let lock = dir.join(LOCK);
    acquire(&lock, policy).ok()?;
    let _unlock = Unlock(&lock);
    let prev = match read_open(&dir) {
        Sighting::Unreadable => return None,
        Sighting::Absent => None,
        Sighting::Read(open) if settled(&open, now, period) => return None,
        Sighting::Read(open) => Some(open),
    };
    let (open, closed) = next(prev, row.stat(), now, period)?;
    commit(&dir, &open, &closed)
}

/// 周期の rules 行（`seat.tick_interval_s`）の値（無い・不発効・整数でない周は `None`＝既定の値で埋めない・C5）。
fn period_of(manifest: &Manifest) -> Option<u64> {
    let row = manifest.get(crate::seat::tick::ROW_INTERVAL)?;
    match (row.enabled, &row.value) {
        (true, RuleValue::Int(found)) => Some(*found),
        _ => None,
    }
}

/// 管理 tick の標本の 1 本（run が full_rewrite の後の同じ枝で呼び、返りは捨てる）。stdout にも stderr にも書かない。
///
/// target に登録 row が無い周・host の面を合わせた manifest を読めない周・表が 0 行の周は何もせず（host の根に dir も作らない）、周期の行と
/// lock の 2 行のどれかを読めない周は記録を書かない。表の行ごとに独立に進め、和は取らない。
pub fn sample(state_dir: &Path, target: &str, manifest: &Manifest, events: &[Event]) -> Option<()> {
    crate::seat::role::registration_of_target(&super::replay(events), target)?;
    let merged = crate::rules::with_state_dir(manifest.clone(), Some(state_dir)).ok()?;
    if merged.write_budgets().is_empty() {
        return None;
    }
    let period = period_of(&merged)?;
    let policy = LockPolicy::from_rules(&merged).ok()?;
    let now = crate::seat::state::now_secs();
    for row in merged.write_budgets() {
        let _ = measure(row, (state_dir, policy), (now, period));
    }
    Some(())
}
