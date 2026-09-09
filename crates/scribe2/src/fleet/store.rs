//! event log の read / write（設計 §4）。
//!
//! write は lock を取って 1 行追記するだけ、read は **malformed を黙って飛ばさず
//! 全件 error にする**（SRS NFR4 の fail-closed）。lock の再試行と stale の線は
//! 数値を焼かず rules 行から読む（憲法 C1 / C5）。

use super::Event;
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// lock の再試行の上限を持つ rules 行。
const ROW_RETRY: &str = "fleet.lock_retry_ms";
/// lock を stale と見なす経過時間を持つ rules 行。
const ROW_STALE: &str = "fleet.lock_stale_ms";
/// lock の取り直しの間隔。
const RETRY_TICK: Duration = Duration::from_millis(5);

/// store の失敗。極性は fail-closed（C11.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// file 操作が失敗した。
    Io(String),
    /// 行が読めない（`line` は 1 始まり）。
    Malformed {
        /// 何行目か。
        line: u64,
        /// なぜ読めないか。
        reason: String,
    },
    /// lock を取れなかった。
    Lock(String),
    /// rules 行が引けない。
    Rules(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(reason) => write!(f, "fleet: {reason}"),
            Self::Malformed { line, reason } => write!(f, "fleet: {reason} line={line}"),
            Self::Lock(reason) => write!(f, "fleet: lock を取れない（{reason}）"),
            Self::Rules(reason) => write!(f, "fleet: rules 行を引けない（{reason}）"),
        }
    }
}

/// 黙って済ませない出来事。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warning {
    /// 古い lock を外した。
    StaleLockRemoved,
}

impl Warning {
    /// 表示に使う 1 行。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StaleLockRemoved => "fleet: 古い lock を外した",
        }
    }
}

/// lock の待ち方。値は rules 行から来る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockPolicy {
    /// 取り直しを続ける上限（ミリ秒）。
    pub retry_ms: u64,
    /// これより古い lock は stale として外す（ミリ秒）。
    pub stale_ms: u64,
}

impl LockPolicy {
    /// 埋め込みの manifest から読む。
    pub fn embedded() -> Result<Self, StoreError> {
        match Manifest::embedded() {
            Ok(manifest) => Self::from_rules(&manifest),
            Err(errors) => Err(StoreError::Rules(join_errors(&errors))),
        }
    }

    /// manifest の 2 行から組む。
    pub fn from_rules(manifest: &Manifest) -> Result<Self, StoreError> {
        Ok(Self {
            retry_ms: int_row(manifest, ROW_RETRY)?,
            stale_ms: int_row(manifest, ROW_STALE)?,
        })
    }
}

/// rules 行の整数値を引く。無い・不発効・型違いは error。
fn int_row(manifest: &Manifest, id: &str) -> Result<u64, StoreError> {
    let row = manifest
        .get(id)
        .ok_or_else(|| StoreError::Rules(format!("{id} が無い")))?;
    if !row.enabled {
        return Err(StoreError::Rules(format!("{id} は不発効である")));
    }
    match row.value {
        RuleValue::Int(found) => Ok(found),
        _ => Err(StoreError::Rules(format!("{id} が整数でない"))),
    }
}

/// 読み取り error を 1 本の文字列へ畳む。
fn join_errors<E: std::fmt::Display>(errors: &[E]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<String>>()
        .join(" / ")
}

/// event log の path。
pub fn events_path(dir: &Path) -> PathBuf {
    dir.join("fleet").join("events.jsonl")
}

/// lock file の path。
pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join("fleet").join("events.jsonl.lock")
}

/// 1 件を追記する。lock を取り、1 行書いて flush し、lock を外す。
pub fn append(dir: &Path, event: &Event, policy: LockPolicy) -> Result<Vec<Warning>, StoreError> {
    let events = events_path(dir);
    let parent = events
        .parent()
        .ok_or_else(|| StoreError::Io("event log の親 dir が無い".to_owned()))?;
    fs::create_dir_all(parent).map_err(|err| StoreError::Io(format!("dir を作れない: {err}")))?;
    let warnings = acquire(&lock_path(dir), policy)?;
    let outcome = write_line(&events, &event.to_line());
    let released = fs::remove_file(lock_path(dir));
    outcome?;
    released.map_err(|err| StoreError::Io(format!("lock を外せない: {err}")))?;
    Ok(warnings)
}

/// 1 行を追記して flush する。
fn write_line(events: &Path, line: &str) -> Result<(), StoreError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(events)
        .map_err(|err| StoreError::Io(format!("event log を開けない: {err}")))?;
    writeln!(file, "{line}").map_err(|err| StoreError::Io(format!("書けない: {err}")))?;
    file.flush()
        .map_err(|err| StoreError::Io(format!("flush できない: {err}")))
}

/// lock を取る。古い lock は外して警告に載せる（黙って消さない）。
fn acquire(lock: &Path, policy: LockPolicy) -> Result<Vec<Warning>, StoreError> {
    let started = Instant::now();
    let mut warnings = Vec::new();
    loop {
        match OpenOptions::new().create_new(true).write(true).open(lock) {
            Ok(_) => return Ok(warnings),
            Err(err) if err.kind() != std::io::ErrorKind::AlreadyExists => {
                return Err(StoreError::Lock(err.to_string()));
            }
            Err(_) => {}
        }
        if is_stale(lock, policy.stale_ms) && fs::remove_file(lock).is_ok() {
            warnings.push(Warning::StaleLockRemoved);
            continue;
        }
        if started.elapsed() >= Duration::from_millis(policy.retry_ms) {
            return Err(StoreError::Lock(format!("{} ms 待った", policy.retry_ms)));
        }
        std::thread::sleep(RETRY_TICK);
    }
}

/// lock が stale か（mtime が線より古いか）。
fn is_stale(lock: &Path, stale_ms: u64) -> bool {
    let Ok(meta) = fs::metadata(lock) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age >= Duration::from_millis(stale_ms))
}

/// 全 event を読む。**malformed は 1 件も飛ばさず全件返す**。file 不在は空。
pub fn read_all(dir: &Path) -> Result<Vec<Event>, Vec<StoreError>> {
    let events = events_path(dir);
    if !events.exists() {
        return Ok(Vec::new());
    }
    let text = match fs::read_to_string(&events) {
        Ok(found) => found,
        Err(err) => return Err(vec![StoreError::Io(format!("event log を読めない: {err}"))]),
    };
    let mut parsed = Vec::new();
    let mut errors = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let at = index as u64 + 1;
        match Event::from_line(line) {
            Ok(event) => parsed.push(event),
            Err(reason) => errors.push(StoreError::Malformed { line: at, reason }),
        }
    }
    if errors.is_empty() {
        Ok(parsed)
    } else {
        Err(errors)
    }
}
