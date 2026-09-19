//! event log の read / write（設計 §4）。
//!
//! write は lock を取って 1 行追記するだけ、read は **malformed を黙って飛ばさず
//! 全件 error にする**（SRS NFR4 の fail-closed）。lock の再試行と stale の線は
//! 数値を焼かず rules 行から読む（憲法 C1 / C5）。
//!
//! lock file には**所有者の pid を 10 進 1 行**で書く。書き手が lock を持ったまま SIGKILL で
//! 落ちた周（AC4 の「process を殺して」）は所有者の死んだ lock が残るので、既存の lock に
//! 当たった側は中身を読み、所有者が死んでいれば外して取り直す（設計 fleet-event-log.md §4）。
//! pid の生存判定（起動時刻の probe）は受付の札（[`crate::pipe::admission`]・ADR-0021 §2.3）
//! と**共有する 1 本**で、ここに置く（lock の実装が 1 本であるのと同じ理由・憲法 C6.3）。

use crate::polarity::{OnFailure, Polarity, Timing};
use super::Event;
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// lock の再試行の上限を持つ rules 行。
const ROW_RETRY: &str = "fleet.lock_retry_ms";
/// lock を stale と見なす経過時間を持つ rules 行。
const ROW_STALE: &str = "fleet.lock_stale_ms";
/// lock の取り直しの間隔。
const RETRY_TICK: Duration = Duration::from_millis(5);

/// 実 probe が読む proc の root。
const PROC_ROOT: &str = "/proc";

/// `/proc/<pid>/stat` の starttime の単位（clock tick / 秒）。
///
/// Linux が user 空間へ見せる `USER_HZ` は ABI として 100 に固定されている（kernel の `HZ` とは
/// 別）。libc の `sysconf` を足さないための定数である（NFR3）。
const USER_HZ: u64 = 100;

/// `/proc/<pid>/stat` の `)` の後ろで starttime が何番目か（0 始まり・field 22 − 3）。
const STARTTIME_AT: usize = 19;

/// この境界の極性（[`StoreError`]）: 書込の時点で lock を取り、取れない・読めない周は書かず error にする。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

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
    /// 回収の途中で死んだ process が残した token が在る（`s2-07l.486`・設計 fleet-event-log.md §4）。
    ///
    /// **黙って外す側へ倒さない**（外すとその lock は誰でも回収できる＝穴が戻る）。人が見て外す面で、
    /// 恒久の直しは §8 の OS の file lock である。
    ReclaimToken(String),
    /// rules 行が引けない。
    Rules(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(reason) => write!(f, "fleet: {reason}"),
            Self::Malformed { line, reason } => write!(f, "fleet: {reason} line={line}"),
            Self::Lock(reason) => write!(f, "fleet: lock を取れない（{reason}）"),
            Self::ReclaimToken(path) => write!(f, "fleet: 回収の token {path} が残っている（人が外す）"),
            Self::Rules(reason) => write!(f, "fleet: rules 行を引けない（{reason}）"),
        }
    }
}

/// 黙って済ませない出来事。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warning {
    /// 古い lock を外した。
    StaleLockRemoved,
    /// 所有者の死んだ lock を外した（書き手が lock を持ったまま落ちた周）。
    DeadOwnerLockRemoved,
}

impl Warning {
    /// 表示に使う 1 行。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StaleLockRemoved => "fleet: 古い lock を外した",
            Self::DeadOwnerLockRemoved => "fleet: 所有者の死んだ lock を外した",
        }
    }
}

/// pid の起動時刻の probe の結果（3 値・**「無い」と「読めない」を畳まない**・C11.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// `/proc/<pid>/stat` が無い（process が無い）。
    Absent,
    /// 起動時刻（epoch ms）。
    Started(u64),
    /// それ以外の理由で読めない（`/proc` 自体が読めない・parse 不能・権限）。
    Unreadable,
}

impl Probe {
    /// 起動時刻だけを取る（「無い」と「読めない」はどちらも `None`）。
    ///
    /// 受付の札はこの 2 つを同じ回収側に読む（ADR-0021 §5 (D)・札を失っても過剰に配る側へ
    /// 倒れる）ので、札の判じにはこの写像で渡す。lock の判定（[`lock_owner`]）は 3 値のまま読む。
    pub fn started(self) -> Option<u64> {
        match self {
            Self::Started(ms) => Some(ms),
            Self::Absent | Self::Unreadable => None,
        }
    }
}

/// lock の所有者の判じ（pure・[`lock_owner`] の返り）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// 所有者の process が無い（外して取り直す側）。
    Dead,
    /// 所有者が生きている（待つ側・pid の再利用もここへ倒れる）。
    Live,
    /// 本文か probe が読めない（従来どおり `fleet.lock_stale_ms` の線に従う側）。
    Unreadable,
}

/// lock の本文（所有者の pid・10 進 1 行）を判じる（pure）。
///
/// `probe` は呼び手が渡す起動時刻の写像（実 probe は [`started_ms`]）。**`Dead` になるのは
/// probe が「無い」を返した周だけ**である——本文が 10 進 1 行でない周と probe が「読めない」を
/// 返した周は `Unreadable` で、fail-closed の極性（stale の線まで待つ）を変えない。
pub fn lock_owner(body: &str, probe: impl Fn(u32) -> Probe) -> Owner {
    let Some(pid) = owner_pid(body) else {
        return Owner::Unreadable;
    };
    match probe(pid) {
        Probe::Absent => Owner::Dead,
        Probe::Started(_) => Owner::Live,
        Probe::Unreadable => Owner::Unreadable,
    }
}

/// 本文を 10 進 1 行（末尾の改行 1 つは許す）の pid として読む。それ以外は `None`。
fn owner_pid(body: &str) -> Option<u32> {
    let line = body.strip_suffix('\n').unwrap_or(body);
    if line.is_empty() || !line.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    line.parse().ok()
}

/// pid の起動時刻を実 `/proc` で測る。
pub fn started_ms(pid: u32) -> Probe {
    started_ms_in(Path::new(PROC_ROOT), pid)
}

/// pid の起動時刻を `root`（proc の root・歯は tmp dir を注入する）で測る。
///
/// **順序で弁別する**: 先に `<root>/stat` の `btime` を読み、読めない周は pid の有無を見ずに
/// 「読めない」（`/proc` 自体が無い環境を「無い」に畳まない）。`btime` が読めた後で
/// `<root>/<pid>/stat` が NotFound の周だけが「無い」。それ以外の失敗（権限・parse 不能）は
/// 「読めない」。
///
/// `btime` は秒へ切り捨てた値なので、ここが出す時刻は実際の起動より**早い側**へ寄る
/// ——札の持ち主を死んだと読む向きには外れない。
pub fn started_ms_in(root: &Path, pid: u32) -> Probe {
    let Some(boot) = fs::read_to_string(root.join("stat")).ok().as_deref().and_then(boot_s) else {
        return Probe::Unreadable;
    };
    let stat = match fs::read_to_string(root.join(pid.to_string()).join("stat")) {
        Ok(found) => found,
        Err(err) if err.kind() == ErrorKind::NotFound => return Probe::Absent,
        Err(_) => return Probe::Unreadable,
    };
    let Some(ticks) = starttime_ticks(&stat) else {
        return Probe::Unreadable;
    };
    boot.checked_mul(1000)
        .and_then(|ms| ms.checked_add(ticks_to_ms(ticks, USER_HZ)?))
        .map_or(Probe::Unreadable, Probe::Started)
}

/// clock tick を ms へ直す（pure・`starttime` の算術はこの 1 本だけ）。
///
/// `ticks * 1000 / hz` の**切り捨て**で、剰余は捨てる（[`started_ms_in`] が出す時刻が実際の
/// 起動より早い側へ寄る向きと同じ）。`ticks * 1000` が `u64` を溢れる周は `None` で、呼び手は
/// 「読めない」へ倒す（[`started_ms_in`] の極性を変えない）。
///
/// `hz` は呼び手が渡す clock tick / 秒（実経路は [`USER_HZ`] の 100 で、0 は来ない）。
fn ticks_to_ms(ticks: u64, hz: u64) -> Option<u64> {
    Some(ticks.checked_mul(1000)? / hz)
}

/// `/proc/<pid>/stat` の starttime（clock tick・pure）。comm の中の空白と `)` に釣られない。
fn starttime_ticks(stat: &str) -> Option<u64> {
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(STARTTIME_AT)?.parse().ok()
}

/// `/proc/stat` の `btime`（秒・pure）。
fn boot_s(proc_stat: &str) -> Option<u64> {
    let line = proc_stat.lines().find(|line| line.starts_with("btime "))?;
    line.split_whitespace().nth(1)?.parse().ok()
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
    lock_of(&events_path(dir))
}

/// 追記先に対応する lock file の path（`<path>.lock`）。
fn lock_of(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".lock");
    PathBuf::from(name)
}

/// 1 件を追記する。lock を取り、1 行書いて flush し、lock を外す。
pub fn append(dir: &Path, event: &Event, policy: LockPolicy) -> Result<Vec<Warning>, StoreError> {
    append_line(&events_path(dir), &event.to_line(), policy)
}

/// 任意の追記 file へ 1 行を lock 付きで書く。
///
/// **lock の実装はこの 1 本だけである**（憲法 C6.3）。event log も hook の注入計測も
/// ここを通り、第 2 の writer を作らない。lock file は `<path>.lock` で、event log に
/// ついては [`lock_path`] が指す従来の path と同一である（挙動不変）。
pub fn append_line(path: &Path, line: &str, policy: LockPolicy) -> Result<Vec<Warning>, StoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| StoreError::Io("追記先の親 dir が無い".to_owned()))?;
    fs::create_dir_all(parent).map_err(|err| StoreError::Io(format!("dir を作れない: {err}")))?;
    let lock = lock_of(path);
    let warnings = acquire(&lock, policy)?;
    let outcome = write_line(path, line);
    let released = fs::remove_file(&lock);
    outcome?;
    released.map_err(|err| StoreError::Io(format!("lock を外せない: {err}")))?;
    Ok(warnings)
}

/// 1 行を追記して flush する。
fn write_line(path: &Path, line: &str) -> Result<(), StoreError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| StoreError::Io(format!("追記 file を開けない: {err}")))?;
    writeln!(file, "{line}").map_err(|err| StoreError::Io(format!("書けない: {err}")))?;
    file.flush()
        .map_err(|err| StoreError::Io(format!("flush できない: {err}")))
}

/// 古い lock を外してよいか（**閉じた 2 値**・`s2-07l.482`）。
///
/// 追記の lock は「書いて閉じる」までが短いので、古い lock は持ち主が生きていても外してよい
/// （[`Self::Stale`]・従来の唯一の形）。**driver の札は数分〜数十分握られる**ので、同じ扱いにすると
/// 生きている driver の札を別の driver が奪う（[`Self::DeadOnly`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reclaim {
    /// 所有者が死んだ lock と、`stale_ms` を超えた lock を外す。
    Stale,
    /// **所有者が死んだ lock だけ**を外す（生きている所有者は `retry_ms` まで待つ）。
    DeadOnly,
}

/// [`Reclaim`] の全 variant（宣言順・`enum-slices` が集合完全性を測る）。
pub const RECLAIMS: &[Reclaim] = &[Reclaim::Stale, Reclaim::DeadOnly];

/// lock を取る。所有者の死んだ lock と（[`Reclaim::Stale`] の周は）古い lock を外して警告に載せる
/// （黙って消さない）。
///
/// 取れた lock には**自分の pid を 10 進 1 行**で書く（`create_new` で開いた handle にそのまま
/// 書く・第 2 の writer を作らない）。書けない周は lock を戻して error（fail-closed）。
///
/// **crate の中へ開く**のは受付（[`crate::pipe::admission`]）が slot dir の lock に、driver の札
/// （[`crate::pipe::Driver`]）が run dir の札に同じ実装を使うためである（lock file は別・実装は 1 本・
/// 憲法 C6.3）。外すのは呼び手が lock file を消すこと。
pub(crate) fn acquire(lock: &Path, policy: LockPolicy) -> Result<Vec<Warning>, StoreError> {
    acquire_with(lock, policy, Reclaim::Stale)
}

/// 回収の token の path（`<lock>.reclaim`）。
fn reclaim_token(lock: &Path) -> PathBuf {
    let mut name = lock.as_os_str().to_owned();
    name.push(".reclaim");
    PathBuf::from(name)
}

/// **回収の 1 手**（設計 fleet-event-log.md §4「回収は 1 手」・`s2-07l.486`）。外せたのが自分なら `true`。
///
/// 現物の回収は 3 手（本文を読んで死んだ／古いと判じる → `remove_file` → `create_new`）で、同じ死んだ
/// lock を観測した 2 本が両方とも回収に入ると、**後の 1 本の `remove_file` が先の 1 本が取ったばかりの
/// 生きた lock を外す**（2 本が同時に lock を持つ）。`rename` で置き換える案は塞がらない——後の 1 本が
/// path で先の新しい lock を動かすためである（`s2-07l.482` の実測）。
///
/// 直しは token 1 つで**回収する者を 1 本に絞る**ことである: `<lock>.reclaim` を `create_new` で取れた
/// 1 本だけが lock を読み直し、**観測した本文と同じ周に限って**外す。token を取れなかった本と、読み直しが
/// 観測と違った本（＝既に別の 1 本が取り直した lock）は `false` で、外さずに次の周の取り直しへ戻る。
fn reclaim(lock: &Path, observed: &str) -> bool {
    let token = reclaim_token(lock);
    if OpenOptions::new().create_new(true).write(true).open(&token).is_err() {
        return false;
    }
    // **読み直しが観測と同じ周だけ外す**（違えば別の 1 本が既に取り直している）。
    let same = fs::read_to_string(lock).is_ok_and(|body| body == observed);
    let outcome = same && fs::remove_file(lock).is_ok();
    // token は寿命 μs で必ず外す（残るのは回収の途中で死んだ周だけ＝呼び手が typed に落とす）。
    let _ = fs::remove_file(&token);
    outcome
}

/// [`acquire`] に古い lock の扱いを渡す形（**判定の本文は 1 本**）。
pub(crate) fn acquire_with(lock: &Path, policy: LockPolicy, policy_reclaim: Reclaim) -> Result<Vec<Warning>, StoreError> {
    let started = Instant::now();
    let mut warnings = Vec::new();
    loop {
        match OpenOptions::new().create_new(true).write(true).open(lock) {
            Ok(mut handle) => {
                if let Err(err) = writeln!(handle, "{}", std::process::id()).and_then(|()| handle.flush()) {
                    let _ = fs::remove_file(lock);
                    return Err(StoreError::Lock(format!("所有者を書けない: {err}")));
                }
                return Ok(warnings);
            }
            Err(err) if err.kind() != ErrorKind::AlreadyExists => {
                return Err(StoreError::Lock(err.to_string()));
            }
            Err(_) => {}
        }
        // **観測は 1 度だけ読む**: 判じた本文と外す本文を同じにする（判定の本文は不変・§4）。
        let observed = fs::read_to_string(lock).unwrap_or_default();
        if lock_owner(&observed, started_ms) == Owner::Dead && reclaim(lock, &observed) {
            warnings.push(Warning::DeadOwnerLockRemoved);
            continue;
        }
        if policy_reclaim == Reclaim::Stale && is_stale(lock, policy.stale_ms) && reclaim(lock, &observed) {
            warnings.push(Warning::StaleLockRemoved);
            continue;
        }
        if started.elapsed() >= Duration::from_millis(policy.retry_ms) {
            // 回収の途中で死んだ process が残した token は**黙って外さない**（外すと穴が戻る）。
            let token = reclaim_token(lock);
            if token.exists() {
                return Err(StoreError::ReclaimToken(token.display().to_string()));
            }
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

#[cfg(test)]
mod tests {
    use super::{
        acquire_with, boot_s, lock_owner, reclaim, reclaim_token, started_ms_in, starttime_ticks, ticks_to_ms,
        LockPolicy, Owner, Probe, Reclaim, StoreError, Warning, USER_HZ,
    };
    use proptest::prelude::*;
    use proptest::test_runner::Config;
    use std::path::PathBuf;

    /// 確実に居ない pid（`true` を起こして待った pid）。
    fn dead_pid() -> u32 {
        let mut child = std::process::Command::new("true").spawn().expect("true を起こせる");
        let pid = child.id();
        child.wait().expect("true を待てる");
        pid
    }

    /// **回収は 1 手である**（設計 fleet-event-log.md §4・行 c）: 同じ死んだ lock を観測した 2 本のうち、
    /// 外せるのは **1 本だけ**である（逐次 2 回の呼び出しで `true`, `false`）。
    ///
    /// **順序は現物どおりに並べる**: 1 本目が外した後、その 1 本は `create_new` で lock を取り直す。
    /// 2 本目はそこで初めて回収に入る——3 手（読む → `remove_file` → `create_new`）の形では、この
    /// 2 本目の `remove_file` が**1 本目が取ったばかりの生きた lock を落とす**（2 本が同時に lock を
    /// 持つ）。外した直後に 2 回目を撃つだけの並びでは、どちらの実装でも `true`, `false` になって
    /// 空虚である（`s2-07l.482` で同じ罠を踏んだ）。並行の e2e は負荷で揺れるので置かない。
    #[test]
    fn fleet_lock_reclaim_admits_one_of_two_observers() {
        let dir = scratch("one-of-two");
        let lock = dir.join("events.jsonl.lock");
        let observed = format!("{}\n", dead_pid());
        std::fs::write(&lock, &observed).expect("lock を書ける");
        // 1 本目: 観測した本文のまま外し、続けて自分の lock を取る（`create_new` の後の姿）。
        assert!(reclaim(&lock, &observed), "先に token を取った 1 本は外せる");
        std::fs::write(&lock, format!("{}\n", std::process::id())).expect("1 本目が lock を取る");
        // 2 本目: **同じ（古い）観測**で回収に入る。外せてはならない。
        assert!(!reclaim(&lock, &observed), "2 本目は外せない（母集団 2 本）");
        assert!(lock.exists(), "1 本目が取ったばかりの lock は残る");
        assert_eq!(
            std::fs::read_to_string(&lock).unwrap_or_default(),
            format!("{}\n", std::process::id()),
            "残るのは 1 本目の lock である"
        );
        assert!(!reclaim_token(&lock).exists(), "token は寿命 μs で必ず外れる");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **生きている所有者の（古くない）lock は `Stale` でも `DeadOnly` でも `retry_ms` まで待つ**
    /// （行 c の done の 3 項目）。回収の 1 手は「外してよい」と判じた周にしか呼ばれない。
    #[test]
    fn fleet_lock_reclaim_live_owner_waits_under_both_reclaims() {
        let dir = scratch("live-owner");
        let lock = dir.join("events.jsonl.lock");
        std::fs::write(&lock, format!("{}\n", std::process::id())).expect("lock を書ける");
        // 古くない線（`stale_ms` を十分大きく）＝外してよい理由が 1 つも無い周。
        let policy = LockPolicy { retry_ms: 30, stale_ms: 600_000 };
        for reclaim in [Reclaim::Stale, Reclaim::DeadOnly] {
            let outcome = acquire_with(&lock, policy, reclaim);
            assert!(outcome.is_err(), "{reclaim:?} は生きている所有者の lock を奪わない");
            assert!(lock.exists(), "{reclaim:?} の周でも lock は残る");
        }
        assert!(!reclaim_token(&lock).exists(), "回収に入らないので token も作らない");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **stale の回収も同じ 1 手を通る**（`Reclaim::Stale` の周）。`DeadOnly` の周は古くても外さない。
    #[test]
    fn fleet_lock_reclaim_stale_goes_through_the_same_one_step() {
        let dir = scratch("stale");
        let lock = dir.join("events.jsonl.lock");
        // **生きている所有者**の lock（自分の pid）を古い側に倒す（`stale_ms` = 1）。
        std::fs::write(&lock, format!("{}\n", std::process::id())).expect("lock を書ける");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let policy = LockPolicy { retry_ms: 30, stale_ms: 1 };
        let dead_only = acquire_with(&lock, policy, Reclaim::DeadOnly);
        assert!(dead_only.is_err(), "DeadOnly は古くても生きている所有者の lock を奪わない");
        std::fs::write(&lock, format!("{}\n", std::process::id())).expect("lock を書き直せる");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let stale = acquire_with(&lock, policy, Reclaim::Stale).expect("Stale は古い lock を回収して取れる");
        assert_eq!(stale, vec![Warning::StaleLockRemoved], "回収の warning は従来どおり 1 件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **回収の途中で死んだ process が残した token は黙って外さない**（FailClosed・C11.2）。
    ///
    /// `retry_ms` の後に token を名指す typed な error で落ちる（外す側へ倒すと、その lock は誰でも
    /// 回収できて穴が戻る）。
    #[test]
    fn fleet_lock_reclaim_leftover_token_fails_closed_with_its_path() {
        let dir = scratch("token");
        let lock = dir.join("events.jsonl.lock");
        std::fs::write(&lock, format!("{}\n", dead_pid())).expect("lock を書ける");
        std::fs::write(reclaim_token(&lock), "").expect("token を書ける");
        let policy = LockPolicy { retry_ms: 30, stale_ms: 30_000 };
        let outcome = acquire_with(&lock, policy, Reclaim::DeadOnly);
        match outcome {
            Err(StoreError::ReclaimToken(path)) => {
                assert!(path.contains(".reclaim"), "error は token を名指す: {path}");
            }
            other => panic!("token が残る周は typed に落ちる: {other:?}"),
        }
        assert!(reclaim_token(&lock).exists(), "残った token は黙って外さない");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 反例の永続化を切り、case 数を 256 に pin する（`tests/e2e/prop.rs` と同じ形）。
    fn config() -> Config {
        Config {
            cases: 256,
            failure_persistence: None,
            ..Config::default()
        }
    }

    /// 歯ごとの空の tmp dir（proc root の注入先・env を読まないのは器の本体の規律〔C2.2〕）。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("store-proc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// `/proc/<pid>/stat` の fixture（starttime = field 22 が `ticks`）。
    fn pid_stat(ticks: u64) -> String {
        let fields: Vec<String> = (3..=25).map(|at| if at == 22 { ticks.to_string() } else { at.to_string() }).collect();
        format!("123 (a b) c) {}", fields.join(" "))
    }

    /// `/proc/<pid>/stat` の starttime は comm の空白と `)` に釣られない。
    #[test]
    fn store_starttime_skips_the_comm_field() {
        let fields: Vec<String> = (3..=25).map(|at| at.to_string()).collect();
        let stat = format!("123 (a b) c) {}", fields.join(" "));
        assert_eq!(starttime_ticks(&stat), Some(22), "field 22");
        assert_eq!(starttime_ticks("123 (x) S 1"), None, "短い");
    }

    /// `/proc/stat` の `btime` を読む（無ければ `None`）。
    #[test]
    fn store_boot_s_reads_btime() {
        assert_eq!(boot_s("cpu 1 2\nbtime 1700000000\n"), Some(1_700_000_000), "btime");
        assert_eq!(boot_s("cpu 1 2\n"), None, "btime が無い");
    }

    /// (2″) 実 probe の順序: `stat` を持たない root では pid dir の有無に依らず「読めない」・`btime` を持つ
    /// `stat` が在って pid dir が無い root では「無い」・pid dir と `stat` が揃う root では起動時刻 ms。
    #[test]
    fn store_probe_reads_btime_before_telling_absent_from_unreadable() {
        let root = scratch("order");
        std::fs::create_dir_all(root.join("42")).expect("pid dir を作れる");
        std::fs::write(root.join("42").join("stat"), pid_stat(250)).expect("pid の stat を書ける");
        assert_eq!(started_ms_in(&root, 42), Probe::Unreadable, "stat が無い root は pid dir が在っても「読めない」");
        assert_eq!(started_ms_in(&root, 43), Probe::Unreadable, "stat が無い root は pid dir が無くても「読めない」");
        std::fs::write(root.join("stat"), "cpu 1 2\nbtime 1700000000\n").expect("btime を書ける");
        assert_eq!(started_ms_in(&root, 43), Probe::Absent, "btime が読めた後で pid dir が無い周だけ「無い」");
        // 1_700_000_000 s → ms ＋ 250 tick / USER_HZ 100 = 2500 ms。
        assert_eq!(started_ms_in(&root, 42), Probe::Started(1_700_000_002_500), "揃った周は起動時刻 ms");
        std::fs::write(root.join("42").join("stat"), "123 (x) S 1").expect("壊れた stat を書ける");
        assert_eq!(started_ms_in(&root, 42), Probe::Unreadable, "parse 不能は「読めない」（「無い」に畳まない）");
        let _ = std::fs::remove_dir_all(&root);
    }

    // flip-check: retroactive s2-07l.247
    //
    // 以下の 4 本は `ticks_to_ms`（`started_ms_in` の clock tick → ms）の算術を pin する。
    // 実装は純移動（式は `started_ms_in` に在ったものと同一）なので挙動は変わらない
    // ＝赤にする flip は変異で示す（`/`→`*` と `/`→`%`・admin の A/B・母集団 2）。

    /// 境界の tick（0 / 1 / hz − 1 / hz / hz + 1）での clock tick → ms。
    ///
    /// tick = hz の丁度 1 秒を挟んで、割り切れない手前（hz − 1）と余りの出る後ろ（hz + 1）を
    /// 両方 pin する。
    #[test]
    fn fleet_store_started_ms_ticks_to_ms_at_the_boundaries() {
        assert_eq!(ticks_to_ms(0, USER_HZ), Some(0), "tick = 0");
        assert_eq!(ticks_to_ms(1, USER_HZ), Some(10), "tick = 1（1000 / 100）");
        assert_eq!(ticks_to_ms(USER_HZ - 1, USER_HZ), Some(990), "tick = hz − 1");
        assert_eq!(ticks_to_ms(USER_HZ, USER_HZ), Some(1000), "tick = hz（丁度 1 秒）");
        assert_eq!(ticks_to_ms(USER_HZ + 1, USER_HZ), Some(1010), "tick = hz + 1");
    }

    /// 割り切れない周は**切り捨て**で、剰余は ms に足さない（`/` を `%` に替えると外れる 1 例）。
    #[test]
    fn fleet_store_started_ms_ticks_to_ms_truncates_the_remainder() {
        // 1000 / 1024 は 0、剰余は 1000——剰余を返す実装なら 0 では済まない。
        assert_eq!(ticks_to_ms(1, 1024), Some(0), "1 tick は 1024 Hz では 0 ms");
        assert_eq!(ticks_to_ms(1, 3), Some(333), "1000 / 3 は 333（剰余 1 は捨てる）");
        assert_eq!(ticks_to_ms(1024, 1024), Some(1000), "割り切れる周は剰余 0 と区別が付く");
    }

    /// `ticks * 1000` が `u64` を溢れる周だけ `None`（呼び手は「読めない」へ倒す）。
    #[test]
    fn fleet_store_started_ms_ticks_to_ms_overflow_is_none() {
        let last = u64::MAX / 1000;
        assert_eq!(ticks_to_ms(last, USER_HZ), Some(last * 1000 / USER_HZ), "溢れない最大の tick");
        assert_eq!(ticks_to_ms(last + 1, USER_HZ), None, "溢れる周は None");
        assert_eq!(ticks_to_ms(u64::MAX, USER_HZ), None, "u64::MAX も None");
    }

    /// 溢れた tick は `started_ms_in` で「読めない」になる（`None` の行き先を pin する）。
    #[test]
    fn fleet_store_started_ms_overflowing_ticks_are_unreadable() {
        let root = scratch("overflow");
        std::fs::create_dir_all(root.join("42")).expect("pid dir を作れる");
        std::fs::write(root.join("stat"), "cpu 1 2\nbtime 1700000000\n").expect("btime を書ける");
        std::fs::write(root.join("42").join("stat"), pid_stat(u64::MAX)).expect("pid の stat を書ける");
        assert_eq!(started_ms_in(&root, 42), Probe::Unreadable, "溢れる tick は「読めない」");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 10 進 1 行の本文（末尾の改行は有無を振る）。
    fn pid_line() -> impl Strategy<Value = (u32, String)> {
        (any::<u32>(), any::<bool>()).prop_map(|(pid, newline)| {
            let body = if newline { format!("{pid}\n") } else { pid.to_string() };
            (pid, body)
        })
    }

    /// 10 進 1 行として読めない本文（空・前後の空白・非数字・複数行・巨大な数）。
    fn broken_body() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(String::new()),
            Just("\n".to_owned()),
            any::<u32>().prop_map(|pid| format!(" {pid}")),
            any::<u32>().prop_map(|pid| format!("{pid} ")),
            any::<u32>().prop_map(|pid| format!("{pid}\n\n")),
            any::<u32>().prop_map(|pid| format!("+{pid}")),
            any::<u32>().prop_map(|pid| format!("-{pid}")),
            "[a-zA-Z_.:-]{1,8}",
            (any::<u32>(), any::<u32>()).prop_map(|(left, right)| format!("{left}\n{right}\n")),
            (any::<u32>(), any::<u32>()).prop_map(|(left, right)| format!("{left} {right}")),
            any::<u64>().prop_map(|big| format!("{}", u64::from(u32::MAX) + 1 + (big % 1000))),
            any::<u128>().prop_map(|huge| format!("{huge}{huge}")),
        ]
    }

    proptest! {
        #![proptest_config(config())]

        /// (0) 任意の tick / hz で `ms = tick * 1000 / hz`（`u128` の別経路で照合）。溢れる周だけ
        /// `None` で、それ以外は必ず値が出る（`hz` は 0 を外す＝実経路に 0 は来ない）。
        ///
        /// tick は**小さい側を明示的に混ぜる**: `any::<u64>()` だけだと殆どの draw が
        /// `tick * 1000` の溢れ（= `None`）へ落ち、算術の本体を通らない案が薄くなる。
        #[test]
        fn fleet_store_started_ms_prop_ticks_to_ms_matches_the_wide_oracle(
            ticks in prop_oneof![0u64..10_000, 0u64..u64::MAX / 1000, any::<u64>()],
            hz in 1u64..=4096,
        ) {
            let wide = u128::from(ticks) * 1000;
            let expected = u64::try_from(wide).ok().map(|product| product / hz);
            prop_assert_eq!(ticks_to_ms(ticks, hz), expected);
        }

        /// (0′) 剰余は捨てる＝`ms * hz` は必ず `tick * 1000` 以下で、差は `hz` 未満（切り捨ての形を
        /// 商の側から pin する・`%` を返す実装も `*` の実装もこの不等式に乗らない）。
        #[test]
        fn fleet_store_started_ms_prop_ticks_to_ms_truncates_toward_zero(ticks in 0u64..1_000_000, hz in 1u64..=4096) {
            let product = ticks * 1000;
            let held = ticks_to_ms(ticks, hz).filter(|ms| ms * hz <= product && product - ms * hz < hz);
            prop_assert!(held.is_some(), "商 * hz は tick * 1000 を超えず、捨てた剰余は hz 未満: {:?}", ticks_to_ms(ticks, hz));
        }

        /// (1) 任意の pid で probe が「無い」を返す周は必ず `Dead`（外して取り直す側）。
        #[test]
        fn prop_lock_owner_absent_process_is_dead((pid, body) in pid_line()) {
            let seen = std::cell::Cell::new(None);
            let owner = lock_owner(&body, |asked| {
                seen.set(Some(asked));
                Probe::Absent
            });
            prop_assert_eq!(owner, Owner::Dead);
            prop_assert_eq!(seen.get(), Some(pid));
        }

        /// (2) 起動時刻を返す周は必ず `Live`（待つ側＝pid の再利用は安全な向きへ倒れる）。
        #[test]
        fn prop_lock_owner_started_process_is_live((_, body) in pid_line(), ms in any::<u64>()) {
            prop_assert_eq!(lock_owner(&body, |_| Probe::Started(ms)), Owner::Live);
        }

        /// (2′) probe が「読めない」を返す周は必ず `Unreadable` で **`Dead` には決してならない**
        /// （`/proc` が読めない環境で全 lock が外れる側へ倒れない・C11.2）。
        #[test]
        fn prop_lock_owner_unreadable_probe_is_never_dead((_, body) in pid_line()) {
            let owner = lock_owner(&body, |_| Probe::Unreadable);
            prop_assert_eq!(owner, Owner::Unreadable);
            prop_assert_ne!(owner, Owner::Dead);
        }

        /// (3) 10 進として読めない本文は probe の返りに依らず必ず `Unreadable` で **`Dead` には決して
        /// ならない**（fail-closed の極性＝従来の `stale_ms` の線に従う）。probe は呼ばれない。
        #[test]
        fn prop_lock_owner_broken_body_is_never_dead(body in broken_body(), probe in 0u8..3) {
            let asked = std::cell::Cell::new(0u32);
            let owner = lock_owner(&body, |_| {
                asked.set(asked.get() + 1);
                match probe {
                    0 => Probe::Absent,
                    1 => Probe::Started(1),
                    _ => Probe::Unreadable,
                }
            });
            prop_assert_eq!(owner, Owner::Unreadable);
            prop_assert_ne!(owner, Owner::Dead);
            prop_assert_eq!(asked.get(), 0);
        }
    }
}
