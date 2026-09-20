//! host 単位の受付（設計 docs/design/gate-cost.md §3.2・ADR-0021 §2.3）。
//!
//! `{jobs}` を持つ verify 行を撃つ前に、host の空き memory と core 数と**生きている受付札**の
//! 合計から配れる枠を測り、枠 1 組 = 札 file 1 つを置いてから撃つ。project をまたいで 1 つの置き場
//! （[`crate::seat::host_slots_dir`]）を見るので、別 project の gate と同時に満額を取らない。
//!
//! 枠は **memory の 2 項と core の 1 項の min**（設計 §31・ADR-0050）。job 1 つの thread の
//! 値段（[`Cpu::price`]）は器がここで決め、受け付けた枠は jobs と thread を**対で**運ぶ
//! （[`Grant`]）。縮退の周と測れない周はどちらも jobs 1 かつ thread 1 である（`cores / 1` を
//! 渡して 1 本の行に core 数ぶんの thread を許す形＝速い側へ倒さない）。
//!
//! **止めない、縮退する**（設計 §2）。枠が空かない周は [`Completion::SlotFree`] を唯一の待機
//! 実装（[`fleet::wait`]）で待ち、上限（rules 行 `gate.slot_wait_s`）を超えたら並列度 1 で進む
//! （`slot=degraded`）。測れない周も並列度 1 で進む（`slot=unmeasured`）。**0 で返さない**。
//! ゆえにこの境界は行為を止めうる判定を持たず、ADR-0014 §2.1 の guard ではない——**極性一覧に
//! 載せない**（設計 §3.2・封じ込めと同じ）。
//!
//! 語の衝突を避けるため、code の識別子は **admission / Ticket** 系で持つ（hook の注入計測の
//! slot〔FR21〕と、intake の「受付」とは別の実体）。file 名の `.slot` と record の `slot=` は
//! ADR-0021 §2.3 の字面のままである。
//!
//! 札は器が管理する「物」ではなく受付の一時的な印で、死んだ札・読めない札は削除して回収する
//! （憲法 N1 の対象外・ADR-0021 §5 (D)）。回収は黙って落とさず record の `slot=` に数を残す
//! （NFR4）。札の持ち主の生存判定（pid + 起動時刻）は lock の所有者の判定と**同じ 1 本**
//! （[`crate::fleet::store::started_ms`]）で、ここには置かない（C6.3）。

use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{acquire, started_ms, LockPolicy};
use crate::fleet::{self, Completion, SCHEMA};
use crate::seat::{host_slots_dir, sanitize_target};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// host の memory を測る面（受付の読み手はこの 1 本）。
const MEMINFO: &str = "/proc/meminfo";

/// 受付札の拡張子（ADR-0021 §2.3 の字面）。
const TICKET_EXT: &str = "slot";

/// 書きかけの札の拡張子（rename の前）。**読み手は `.slot` しか数えない**＝半端な札を読まない。
const PARTIAL_EXT: &str = "partial";

/// slot dir 直下の lock file（lock の実装は fleet の 1 本）。
const LOCK_FILE: &str = "admission.lock";

/// 受付の式が読む 2 つの線（MiB・値は manifest が持つ・憲法 C1 / C5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    /// job 1 つが要る memory（rules 行 `gate.job_memory_mb`）。
    pub job_mb: u64,
    /// 席と host のために残す memory（rules 行 `host.reserve_memory_mb`）。
    pub reserve_mb: u64,
}

/// 受付の材料（rules 行の値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rules {
    /// 式の 2 線。
    pub sizes: Sizes,
    /// 並列度の上限（rules 行 `gate.mutants_jobs`）。
    pub cap: u64,
    /// 枠が空くのを待つ上限（秒・rules 行 `gate.slot_wait_s`）。
    pub wait_s: u64,
    /// slot dir の lock の待ち方（rules 行 `fleet.lock_retry_ms` / `fleet.lock_stale_ms`）。
    pub policy: LockPolicy,
}

/// 配れる枠。**読めない meminfo を 0 に潰さない**（憲法 C11: 「0」と「測れない」を混ぜない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Free {
    /// 測れた（枠の数・0 を含む）。
    Slots(u64),
    /// 測れない（meminfo が読めない・線が 0）。
    Unmeasured,
}

/// 配れる枠を測る（pure・設計 §3.2 の式そのまま）。
///
/// - `by_avail = floor((MemAvailable − reserve) / job)`（いま実際に空いている分）
/// - `by_token = floor((MemTotal − reserve) / job) − Σ 生きている札の jobs`（受け付けたが
///   まだ常駐していない分を数える）
/// - `free = min(by_avail, by_token)`（引き算は 0 の床）
pub fn capacity(meminfo: &str, sizes: Sizes, live_jobs: u64) -> Free {
    let (Some(total), Some(avail)) = (field_mb(meminfo, "MemTotal:"), field_mb(meminfo, "MemAvailable:"))
    else {
        return Free::Unmeasured;
    };
    if sizes.job_mb == 0 {
        return Free::Unmeasured;
    }
    let by_avail = avail.saturating_sub(sizes.reserve_mb) / sizes.job_mb;
    let by_token = (total.saturating_sub(sizes.reserve_mb) / sizes.job_mb).saturating_sub(live_jobs);
    Free::Slots(by_avail.min(by_token))
}

/// meminfo の 1 行（kB）を MiB で読む。読めなければ `None`。
fn field_mb(meminfo: &str, key: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with(key))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// CPU の材料（測定値の cores と、そこから導いた job 1 つの値段・憲法 C10・設計 §31）。
///
/// 組めるのは cores を**測れた**周だけである（[`cpu_of`]）——測れない周を `cores = 0` や
/// `price = 1` で表さない（0 と「測れない」を混ぜない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cpu {
    /// host の core 数（`available_parallelism` の実測）。
    pub cores: u64,
    /// job 1 つの thread の値段 = `max(1, floor(cores / gate.mutants_jobs))`。
    pub price: u64,
}

impl Cpu {
    /// 実測の cores と並列度の上限から値段を決める（pure・**導出の正本はこの 1 本**・設計 §31 約束 1 / 7）。
    ///
    /// `cap` が 0 の周は 1 で割る（受付が `cap` を 1 に clamp するのと同じ向き・0 除算で落とさない）。
    /// 値段の床は 1（cores < cap でも job は 1 thread を持つ）。
    pub fn priced(cores: u64, cap: u64) -> Self {
        Self { cores, price: (cores / cap.max(1)).max(1) }
    }
}

/// cores の読みから CPU の材料を組む（pure）。**cores 不明は `None`**＝cap 0 の周（値段 = cores）と弁別する。
pub fn cpu_of(cores: Option<u64>, cap: u64) -> Option<Cpu> {
    cores.map(|cores| Cpu::priced(cores, cap))
}

/// host の core 数（測定値）。読めない周は `None`。env は読まない（C2.2・host の面を読む口）。
fn host_cores() -> Option<u64> {
    std::thread::available_parallelism()
        .ok()
        .and_then(|found| u64::try_from(found.get()).ok())
}

/// CPU で配れる枠 = `floor(cores / price) − Σ 生きている札の jobs`（引き算は 0 の床・pure・設計 §31 約束 2）。
///
/// 単位は job で、1 job の値段が `price` thread である＝札の jobs の和がそのまま CPU の勘定になる
/// （札の形は変えない・約束 3）。
pub fn by_cpu(cpu: Cpu, live_jobs: u64) -> u64 {
    (cpu.cores / cpu.price.max(1)).saturating_sub(live_jobs)
}

/// 配れる枠 = **3 項の min**（`by_avail` / `by_token` / `by_cpu`・pure・設計 §31 約束 2）。
///
/// memory の 2 項は [`capacity`] のまま（引数も式も変えない）で、その結果に CPU の 1 項を重ねる。
/// meminfo が読めない周は従来どおり `Unmeasured`（CPU の項で 0 に潰さない）。
pub fn room(meminfo: &str, sizes: Sizes, cpu: Cpu, live_jobs: u64) -> Free {
    match capacity(meminfo, sizes, live_jobs) {
        Free::Slots(memory) => Free::Slots(memory.min(by_cpu(cpu, live_jobs))),
        Free::Unmeasured => Free::Unmeasured,
    }
}

/// 受付札 1 枚の中身（1 行 JSON・schema / pid / run / jobs / ts）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    /// 札を持つ process。
    pub pid: u32,
    /// 便 id。
    pub run: String,
    /// 受け付けた枠。
    pub jobs: u64,
    /// 札を書いた時刻（UNIX epoch の ms）。
    pub ts_ms: u64,
}

impl Ticket {
    /// 札の 1 行。
    fn to_line(&self) -> String {
        json_lite::write_object(&[
            ("schema", Value::Num(SCHEMA)),
            ("pid", Value::Num(u64::from(self.pid))),
            ("run", Value::Str(self.run.clone())),
            ("jobs", Value::Num(self.jobs)),
            ("ts", Value::Num(self.ts_ms)),
        ])
    }

    /// 札の 1 行を読む。schema 違い・数値の壊れ・key の欠けは `None`（＝回収の対象）。
    pub fn parse(text: &str) -> Option<Self> {
        let pairs = json_lite::parse_object(text.trim()).ok()?;
        let get = |key: &str| pairs.iter().find(|(found, _)| found == key).map(|(_, value)| value);
        if get("schema")?.as_num()? != SCHEMA {
            return None;
        }
        Some(Self {
            pid: u32::try_from(get("pid")?.as_num()?).ok()?,
            run: get("run")?.as_str()?.to_owned(),
            jobs: get("jobs")?.as_num()?,
            ts_ms: get("ts")?.as_num()?,
        })
    }
}

/// 札 1 枚の判じ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judged {
    /// 生きている（受け付けた枠）。
    Live(u64),
    /// 死んだ札か読めない札（回収する）。
    Reclaim,
}

/// 札の本文を判じる（pure）。`started_ms` は pid の起動時刻（epoch ms・無い process は `None`）。
///
/// 生きている = process が在り、**起動時刻が札の ts 以前**（pid の再利用を弁別する）。
pub fn judge(text: &str, started_ms: impl Fn(u32) -> Option<u64>) -> Judged {
    let Some(ticket) = Ticket::parse(text) else {
        return Judged::Reclaim;
    };
    match started_ms(ticket.pid) {
        Some(started) if started <= ticket.ts_ms => Judged::Live(ticket.jobs),
        Some(_) | None => Judged::Reclaim,
    }
}

/// 札の持ち主の起動時刻（epoch ms）。process が無い・読めない周は `None`。
///
/// 生存判定の実装は lock の所有者と共有する 1 本（[`started_ms`]・設計 fleet-event-log.md §4）。
/// 札は「無い」と「読めない」を同じ回収側に読む（ADR-0021 §5 (D)・
/// [`crate::fleet::store::Probe::started`]）。
fn ticket_started_ms(pid: u32) -> Option<u64> {
    started_ms(pid).started()
}

/// いまの時刻（epoch ms）。
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
}

/// 札の走査の結果。
struct Scan {
    /// 生きている札の jobs の合計。
    live_jobs: u64,
    /// 回収する札。
    doomed: Vec<PathBuf>,
}

/// slot dir の札を判じる（削除はしない）。dir を読めない周は `None`。
fn scan(dir: &Path) -> Option<Scan> {
    let entries = fs::read_dir(dir).ok()?;
    let mut found = Scan { live_jobs: 0, doomed: Vec::new() };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != TICKET_EXT) {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        match judge(&text, ticket_started_ms) {
            Judged::Live(jobs) => found.live_jobs = found.live_jobs.saturating_add(jobs),
            Judged::Reclaim => found.doomed.push(path),
        }
    }
    Some(found)
}

/// 生きている札の合計と、回収した札の数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tokens {
    /// 生きている札の jobs の合計。
    pub live_jobs: u64,
    /// 削除した札の数（record の `slot=reclaimed:<n>`）。
    pub reclaimed: u64,
}

/// 生きている札だけを数え、死んだ札と読めない札を削除して回収する（**lock の内側で呼ぶ**）。
///
/// 数えるのは**削除できた**札だけである（別の受付が先に消した札を 2 度数えない）。
pub fn live_tokens(dir: &Path) -> Option<Tokens> {
    let found = scan(dir)?;
    let reclaimed = found
        .doomed
        .iter()
        .filter(|path| fs::remove_file(path).is_ok())
        .count();
    Some(Tokens {
        live_jobs: found.live_jobs,
        reclaimed: u64::try_from(reclaimed).unwrap_or(u64::MAX),
    })
}

/// 枠が `want` のうち 1 つ以上空いているか（[`Completion::SlotFree`] の 1 周分の観測）。
///
/// **lock を取らず、札も消さない**（回収と記録は lock の内側の受付が持つ）。測れない周は
/// 待たせない——待っても測れるようにはならず、受付の側が `slot=unmeasured` で縮退する。
/// 測るのは受付と**同じ 3 項**（[`room`]・設計 §31 約束 8）——memory だけで解ける観測は、解けた
/// 直後の受付が CPU の項で 0 を出して待ち直す空回りになる。
pub fn has_room_on(dir: &Path, want: u64, sizes: Sizes, cpu: Cpu) -> bool {
    observe(dir, want, sizes, Some(cpu))
}

/// memory の 2 項だけで枠が `want` のうち 1 つ以上空いているか（CPU の材料を持たない呼び手の口＝列が便を
/// 起こす前の余地の検査・`pipe/dispatch.rs`）。待ちの観測は [`has_room_on`] を通る（3 項）。
pub fn has_room(dir: &Path, want: u64, sizes: Sizes) -> bool {
    observe(dir, want, sizes, None)
}

/// 観測の実装 1 本（[`has_room_on`] / [`has_room`]）。CPU の材料が在る周は 3 項、無い周は memory の 2 項。
fn observe(dir: &Path, want: u64, sizes: Sizes, cpu: Option<Cpu>) -> bool {
    let Some(found) = scan(dir) else {
        return true;
    };
    let meminfo = fs::read_to_string(MEMINFO).unwrap_or_default();
    let free = match cpu {
        Some(cpu) => room(&meminfo, sizes, cpu, found.live_jobs),
        None => capacity(&meminfo, sizes, found.live_jobs),
    };
    match free {
        Free::Unmeasured => true,
        Free::Slots(free) => free.min(want) >= 1,
    }
}

/// 測れなかった理由（閉じた enum・record の `slot_why=`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreadable {
    /// slot dir を作れない・読めない・札を書けない。
    SlotsDir,
    /// slot dir の lock を取れない。
    Lock,
    /// `/proc/meminfo` を読めない（または線が 0）。
    Meminfo,
    /// host の core 数を読めない（設計 §31 約束 5・jobs 1 / thread 1 で進む）。
    Cores,
}

impl Unreadable {
    /// record に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SlotsDir => "slots-dir",
            Self::Lock => "lock",
            Self::Meminfo => "meminfo",
            Self::Cores => "cores",
        }
    }
}

/// 受付 1 周の結果（**受付側の閉じた型**）。
///
/// lock の失敗は fleet の `StoreError`（fail-closed の極性）のまま持ち出さず、ここで
/// `Unmeasured` に包み直す——受付は縮退する側の境界で、store の極性を fail-open に読み替えた
/// 形を 1 つの型に同居させない（ADR-0014 §2.1・境界型は 1 極性）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Admission {
    /// 測れた。
    Measured {
        /// 配れた枠（0 なら札を書いていない）。
        jobs: u64,
        /// 回収した札の数。
        reclaimed: u64,
        /// 書いた札（`jobs == 0` の周は `None`）。
        ticket: Option<PathBuf>,
    },
    /// 測れなかった。
    Unmeasured(Unreadable),
}

/// 何枠を取りにいくか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// 空きの範囲で最大 n 枠（空きが 0 なら取らない）。
    UpTo(u64),
    /// 空きに依らず 1 枠（待ちの上限を超えた縮退）。
    Floor,
}

/// 受付の結果の名（record の `slot=`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// 測って枠を配れた。
    Granted,
    /// 待ちの上限を超え、並列度 1 で進む。
    Degraded,
    /// 測れず、並列度 1 で進む。
    Unmeasured,
}

impl Slot {
    /// record に書く字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Granted => "granted",
            Self::Degraded => "degraded",
            Self::Unmeasured => "unmeasured",
        }
    }
}

/// record の `slot=` の値。回収が在った周は数を足す（黙って落とさない）。
///
/// 枠を配れた周は `reclaimed:<n>` だけを書き、縮退の周は `<名>,reclaimed:<n>` と並べる。
fn slot_detail(slot: Slot, reclaimed: u64) -> String {
    match (slot, reclaimed) {
        (_, 0) => slot.as_str().to_owned(),
        (Slot::Granted, n) => format!("reclaimed:{n}"),
        (Slot::Degraded | Slot::Unmeasured, n) => format!("{},reclaimed:{n}", slot.as_str()),
    }
}

/// 縮退の周と測れない周の並列度と thread（**1 は常に許される**＝従来と同じ費用）。
const FLOOR: u64 = 1;

/// 受け付けた枠。**Drop で札を消す**（器が死んだ周は次の受付が pid で回収する）。
#[derive(Debug)]
pub struct Grant {
    /// 配れた枠（**1 以上**）。
    pub jobs: u64,
    /// job 1 つに許す thread（**1 以上**・測れた周は [`Cpu::price`]・縮退と測れない周は 1・設計 §31 約束 4）。
    ///
    /// jobs と**対で**運ぶ——jobs だけを 1 に落として thread を道具に導かせると、枠を配れないほど
    /// 混んだ host でいちばん太い行（`cores / 1`）を撃つ形になる（2026-09-20 の事故の出所）。
    pub threads: u64,
    /// record の `slot=`（`granted` / `degraded` / `unmeasured` / `reclaimed:<n>`）。
    pub detail: String,
    /// 測れなかった理由（測れた周は `None`・record の `slot_why=`）。
    pub why: Option<Unreadable>,
    /// 置いた札（置いていない周は `None`）。
    ticket: Option<PathBuf>,
}

impl Drop for Grant {
    fn drop(&mut self) {
        if let Some(path) = self.ticket.take() {
            // 消せない札は次の受付が pid で回収する（ここで止めない）。
            let _ = fs::remove_file(path);
        }
    }
}

/// 札を消して枠を返す。
pub fn release(grant: Grant) {
    drop(grant);
}

/// 枠を受け付ける（設計 §3.2 の取得）。**`jobs` は 1 以上で返る**。
///
/// lock の内側で測り、札を書く。空きが 0 の周は lock を離して [`Completion::SlotFree`] を
/// 唯一の待機実装で待ち、上限を超えたら 1 枠の札で進む。
pub fn admit(state_dir: &Path, run: &str, want: u64, rules: &Rules) -> Grant {
    let dir = host_slots_dir(state_dir);
    let want = want.clamp(1, rules.cap.max(1));
    // core 数は **1 受付で 1 回だけ測る**（待ちの観測も同じ値を運ぶ・meminfo と違い受付の間に動かない）。
    // 読めない周は札を置かず縮退する（設計 §31 約束 5・回収の数を残す前なので回収もしない）。
    let Some(cpu) = cpu_of(host_cores(), rules.cap) else {
        return unmeasured(Unreadable::Cores, 0);
    };
    let started = Instant::now();
    let deadline = Duration::from_secs(rules.wait_s);
    let mut reclaimed: u64 = 0;
    loop {
        match attempt(&dir, run, Ask::UpTo(want), rules, cpu) {
            Admission::Unmeasured(why) => return unmeasured(why, reclaimed),
            Admission::Measured { jobs, reclaimed: more, ticket } => {
                reclaimed = reclaimed.saturating_add(more);
                if jobs >= 1 {
                    return Grant {
                        jobs,
                        threads: cpu.price,
                        detail: slot_detail(Slot::Granted, reclaimed),
                        why: None,
                        ticket,
                    };
                }
            }
        }
        let left = deadline.saturating_sub(started.elapsed());
        let completion = Completion::SlotFree {
            slots_dir: dir.clone(),
            want,
            job_mb: rules.sizes.job_mb,
            reserve_mb: rules.sizes.reserve_mb,
            cap: rules.cap,
            cores: cpu.cores,
        };
        if left.is_zero() || fleet::wait(completion, left).is_err() {
            return degraded(&dir, run, rules, reclaimed, cpu);
        }
    }
}

/// 待ちの上限を超えた周: 1 枠の札を書いて進む。**thread も 1**（設計 §31 約束 4）。
fn degraded(dir: &Path, run: &str, rules: &Rules, reclaimed: u64, cpu: Cpu) -> Grant {
    match attempt(dir, run, Ask::Floor, rules, cpu) {
        Admission::Unmeasured(why) => unmeasured(why, reclaimed),
        Admission::Measured { reclaimed: more, ticket, .. } => Grant {
            jobs: FLOOR,
            threads: FLOOR,
            detail: slot_detail(Slot::Degraded, reclaimed.saturating_add(more)),
            why: None,
            ticket,
        },
    }
}

/// 測れなかった周: 札を置かず 1 枠・1 thread で進む。
fn unmeasured(why: Unreadable, reclaimed: u64) -> Grant {
    Grant {
        jobs: FLOOR,
        threads: FLOOR,
        detail: slot_detail(Slot::Unmeasured, reclaimed),
        why: Some(why),
        ticket: None,
    }
}

/// lock を取り、測り、札を書き、lock を離す（1 周）。
fn attempt(dir: &Path, run: &str, ask: Ask, rules: &Rules, cpu: Cpu) -> Admission {
    if fs::create_dir_all(dir).is_err() {
        return Admission::Unmeasured(Unreadable::SlotsDir);
    }
    let lock = dir.join(LOCK_FILE);
    if acquire(&lock, rules.policy).is_err() {
        return Admission::Unmeasured(Unreadable::Lock);
    }
    let taken = take(dir, run, ask, rules.sizes, cpu);
    // 外せない lock は stale の線（rules 行 `fleet.lock_stale_ms`）が次の受付で外す。
    let _ = fs::remove_file(&lock);
    taken
}

/// lock の内側: meminfo を読み、札を回収して数え、枠が在れば札を書く（枠は 3 項の min・[`room`]）。
///
/// **meminfo を先に読む**——読めない周に札を回収すると、回収の数を record に残す口が無い。
fn take(dir: &Path, run: &str, ask: Ask, sizes: Sizes, cpu: Cpu) -> Admission {
    let meminfo = fs::read_to_string(MEMINFO).unwrap_or_default();
    if capacity(&meminfo, sizes, 0) == Free::Unmeasured {
        return Admission::Unmeasured(Unreadable::Meminfo);
    }
    let Some(tokens) = live_tokens(dir) else {
        return Admission::Unmeasured(Unreadable::SlotsDir);
    };
    let Free::Slots(free) = room(&meminfo, sizes, cpu, tokens.live_jobs) else {
        return Admission::Unmeasured(Unreadable::Meminfo);
    };
    let jobs = match ask {
        Ask::UpTo(want) => want.min(free),
        Ask::Floor => FLOOR,
    };
    if jobs == 0 {
        return Admission::Measured { jobs, reclaimed: tokens.reclaimed, ticket: None };
    }
    match write_ticket(dir, run, jobs) {
        Some(ticket) => Admission::Measured { jobs, reclaimed: tokens.reclaimed, ticket: Some(ticket) },
        None => Admission::Unmeasured(Unreadable::SlotsDir),
    }
}

/// 札を `<pid>-<run>.slot` に書く（書きかけを別名で書いて rename する＝読み手は半端を見ない）。
fn write_ticket(dir: &Path, run: &str, jobs: u64) -> Option<PathBuf> {
    let pid = std::process::id();
    let ticket = Ticket { pid, run: run.to_owned(), jobs, ts_ms: now_ms() };
    let stem = format!("{pid}-{}", sanitize_target(run));
    let partial = dir.join(format!("{stem}.{PARTIAL_EXT}"));
    let path = dir.join(format!("{stem}.{TICKET_EXT}"));
    fs::write(&partial, format!("{}\n", ticket.to_line())).ok()?;
    if fs::rename(&partial, &path).is_err() {
        let _ = fs::remove_file(&partial);
        return None;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::{
        by_cpu, capacity, cpu_of, judge, now_ms, room, slot_detail, ticket_started_ms, Cpu, Free, Judged, Sizes,
        Slot, Ticket, Unreadable,
    };

    // flip-check: retroactive s2-07l.222
    /// `Unreadable::as_str` は record の `slot_why=` の字面で、variant ごとに固定である（空や同じ字面に潰すと
    /// 測れなかった理由を record から弁別できない）。
    #[test]
    fn mutant_in_pipe_admission_unreadable_names_are_fixed() {
        assert_eq!(Unreadable::SlotsDir.as_str(), "slots-dir");
        assert_eq!(Unreadable::Lock.as_str(), "lock");
        assert_eq!(Unreadable::Meminfo.as_str(), "meminfo");
        assert_eq!(Unreadable::Cores.as_str(), "cores");
    }

    /// job 1 つの値段は `max(1, floor(cores / cap))`（設計 §31 約束 1・歯 (a)）。**cap 0 と cores 不明は別の形**:
    /// cap 0 は 1 で割って値段 = cores、cores 不明は材料を組めない（`None`・0 や 1 に潰さない）。
    #[test]
    fn admission_cpu_price_is_cores_over_cap_floored_at_one() {
        assert_eq!(Cpu::priced(16, 4).price, 4, "16 core / cap 4");
        assert_eq!(Cpu::priced(16, 3).price, 5, "端数は切り捨て");
        assert_eq!(Cpu::priced(2, 4).price, 1, "cores < cap でも床は 1");
        assert_eq!(Cpu::priced(16, 1).price, 16, "cap 1 は core 数ぶん");
        assert_eq!(Cpu::priced(16, 0), Cpu { cores: 16, price: 16 }, "cap 0 は 1 で割る（0 除算で落とさない）");
        assert_eq!(cpu_of(Some(16), 0), Some(Cpu { cores: 16, price: 16 }), "cap 0 でも cores が在れば組める");
        assert_eq!(cpu_of(None, 4), None, "cores 不明は組めない（cap 0 と弁別）");
        assert_eq!(cpu_of(Some(8), 4), Some(Cpu { cores: 8, price: 2 }), "cores は測定値のまま運ぶ");
    }

    /// `by_cpu = floor(cores / price) − Σ 生きている札の jobs`（歯 (b)）: floor・差引・0 の床。
    #[test]
    fn admission_cpu_by_cpu_subtracts_live_jobs_and_floors_at_zero() {
        let cpu = Cpu::priced(16, 4);
        assert_eq!(by_cpu(cpu, 0), 4, "16 / 4 = 4 job");
        assert_eq!(by_cpu(cpu, 3), 1, "生きている札の jobs を引く");
        assert_eq!(by_cpu(cpu, 4), 0, "満杯");
        assert_eq!(by_cpu(cpu, 99), 0, "0 の床（負へ回り込まない）");
        assert_eq!(by_cpu(Cpu::priced(16, 3), 0), 3, "16 / 5 = 3（floor）");
        assert_eq!(by_cpu(Cpu { cores: 7, price: 0 }, 0), 7, "price 0 は 1 で割る");
    }

    /// 3 項の min（歯 (c)）: **CPU 側で決まる** fixture（memory は潤沢・core が細い）と **memory 側で決まる**
    /// fixture の両方で正しい——どちらか一方だけの歯は、min の項を落としても緑になる。
    #[test]
    fn admission_cpu_room_is_the_min_of_memory_and_cpu_terms() {
        // memory: by_avail = (70000 − 2000) / 1000 = 68・by_token = (100000 − 2000) / 1000 = 98 → 68。
        let rich = meminfo(100_000, 70_000);
        assert_eq!(capacity(&rich, SIZES, 0), Free::Slots(68), "memory の 2 項は不変");
        // CPU: 4 core / cap 4 → price 1 → by_cpu = 4 − 0 = 4 が min。
        assert_eq!(room(&rich, SIZES, Cpu::priced(4, 4), 0), Free::Slots(4), "CPU 側で決まる");
        assert_eq!(room(&rich, SIZES, Cpu::priced(4, 4), 3), Free::Slots(1), "CPU 側から札の jobs を引く");
        // memory: by_avail = (7000 − 2000) / 1000 = 5・CPU: 64 core / cap 4 → price 16 → by_cpu = 4。
        let lean = meminfo(10_000, 7_000);
        assert_eq!(room(&lean, SIZES, Cpu::priced(64, 4), 0), Free::Slots(4), "CPU 4 < memory 5");
        // memory: 5・CPU: 64 core / cap 8 → price 8 → by_cpu = 8 > 5。
        assert_eq!(room(&lean, SIZES, Cpu::priced(64, 8), 0), Free::Slots(5), "memory 側で決まる");
        // 札の jobs は両側から引く: memory by_token = 8 − 4 = 4・by_avail 5・CPU 8 − 4 = 4 → 4。
        assert_eq!(room(&lean, SIZES, Cpu::priced(64, 8), 4), Free::Slots(4), "by_token が min 側");
        // 読めない meminfo は CPU が潤沢でも `Unmeasured`（0 にも CPU の値にも潰さない）。
        assert_eq!(room("", SIZES, Cpu::priced(64, 8), 0), Free::Unmeasured, "meminfo が読めない");
    }

    /// cores を読めない周の閉じた理由（歯 (d)）: 材料を組めず（`cpu_of` が `None`）、record の `slot_why=` の
    /// 字面は固定の `cores`（他の理由と同じ字面に潰さない）。
    #[test]
    fn admission_cpu_unreadable_cores_has_a_fixed_reason() {
        assert_eq!(cpu_of(None, 4), None, "読めない cores は測れなかった側");
        assert_eq!(Unreadable::Cores.as_str(), "cores");
        for other in [Unreadable::SlotsDir, Unreadable::Lock, Unreadable::Meminfo] {
            assert_ne!(other.as_str(), Unreadable::Cores.as_str(), "{other:?} と弁別できる");
        }
        assert_eq!(slot_detail(Slot::Unmeasured, 0), "unmeasured", "縮退の名は従来のまま");
    }

    // flip-check: retroactive s2-07l.222
    /// `now_ms` は epoch の ms（0 / 1 に潰さない）で、続けて呼ぶと減らない。下限は repo の最初の commit より
    /// 前の固定時刻（2020-01-01T00:00:00Z = 1577836800000 ms）で、壁時計の境界に等号を置かない。
    #[test]
    fn mutant_in_pipe_admission_now_ms_is_epoch_millis() {
        let first = now_ms();
        let second = now_ms();
        assert!(first > 1_577_836_800_000, "epoch の ms: {first}");
        assert!(second >= first, "減らない: {first} → {second}");
    }

    /// 歯の fixture の 2 線（tracked manifest の値を写さない）。
    const SIZES: Sizes = Sizes { job_mb: 1000, reserve_mb: 2000 };

    /// meminfo の fixture（MiB は kB / 1024）。**2 つの式が別の数を出す**形で置く
    /// （by_avail = (7000 − 2000) / 1000 = 5・by_token = (10000 − 2000) / 1000 = 8）。
    fn meminfo(total_mb: u64, avail_mb: u64) -> String {
        format!(
            "MemTotal:       {} kB\nMemFree:        1 kB\nMemAvailable:   {} kB\n",
            total_mb * 1024,
            avail_mb * 1024
        )
    }

    /// `capacity` は by_avail と by_token の **min** で、reserve を両方から引く。
    #[test]
    fn admission_capacity_takes_the_min_of_the_two_formulas() {
        let text = meminfo(10_000, 7_000);
        assert_eq!(capacity(&text, SIZES, 0), Free::Slots(5), "by_avail 5 < by_token 8");
        // 札が 4 枠を持つと by_token = 8 − 4 = 4 が小さい側になる。
        assert_eq!(capacity(&text, SIZES, 4), Free::Slots(4), "by_token が min 側");
        // reserve を引かない実装は by_avail = 7 になる（引く実装は 5）。
        let no_reserve = Sizes { job_mb: 1000, reserve_mb: 0 };
        assert_eq!(capacity(&text, no_reserve, 0), Free::Slots(7), "reserve 0 は差引なし");
        // 端数は切り捨て（5999 − 2000 = 3999 → 3）。
        assert_eq!(capacity(&meminfo(10_000, 5_999), SIZES, 0), Free::Slots(3), "floor");
    }

    /// 引き算は **0 の床**（負へ回り込まない）。
    #[test]
    fn admission_capacity_floors_at_zero() {
        assert_eq!(capacity(&meminfo(10_000, 1_500), SIZES, 0), Free::Slots(0), "avail < reserve");
        assert_eq!(capacity(&meminfo(10_000, 7_000), SIZES, 99), Free::Slots(0), "札が総量を超える");
        assert_eq!(capacity(&meminfo(1_000, 1_000), SIZES, 0), Free::Slots(0), "total < reserve");
    }

    /// 読めない meminfo は `Unmeasured`（0 に潰さない）。
    #[test]
    fn admission_capacity_is_unmeasured_on_unreadable_meminfo() {
        assert_eq!(capacity("", SIZES, 0), Free::Unmeasured, "空");
        assert_eq!(capacity("MemTotal: 1024 kB\n", SIZES, 0), Free::Unmeasured, "MemAvailable が無い");
        assert_eq!(
            capacity("MemTotal: x kB\nMemAvailable: 1024 kB\n", SIZES, 0),
            Free::Unmeasured,
            "数でない"
        );
        let zero = Sizes { job_mb: 0, reserve_mb: 0 };
        assert_eq!(capacity(&meminfo(10_000, 7_000), zero, 0), Free::Unmeasured, "job 0 は割れない");
    }

    /// 壊れた札は**回収に数える**（黙って飛ばさない・数えずに残さない）。
    #[test]
    fn admission_broken_ticket_counts_as_reclaimed() {
        let alive = |_: u32| Some(0);
        assert_eq!(judge("{\"schema\":1,", alive), Judged::Reclaim, "JSON の壊れ");
        assert_eq!(
            judge("{\"schema\":9,\"pid\":1,\"run\":\"r\",\"jobs\":2,\"ts\":5}", alive),
            Judged::Reclaim,
            "schema 違い"
        );
        assert_eq!(
            judge("{\"schema\":1,\"pid\":1,\"run\":\"r\",\"jobs\":\"2\",\"ts\":5}", alive),
            Judged::Reclaim,
            "数値の壊れ"
        );
        assert_eq!(judge("{\"schema\":1,\"pid\":1,\"run\":\"r\",\"ts\":5}", alive), Judged::Reclaim, "jobs 欠け");
    }

    /// 生きている札 = process が在り、**起動時刻が ts 以前**。
    #[test]
    fn admission_judge_tells_live_dead_and_reused_pids() {
        let line = Ticket { pid: 42, run: "r".to_owned(), jobs: 3, ts_ms: 5_000 }.to_line();
        assert_eq!(Ticket::parse(&line).map(|found| found.jobs), Some(3), "書いた札を読める");
        assert_eq!(judge(&line, |_| Some(5_000)), Judged::Live(3), "ts ちょうどに起動は生");
        assert_eq!(judge(&line, |_| Some(4_000)), Judged::Live(3), "ts より前に起動は生");
        assert_eq!(judge(&line, |_| Some(5_001)), Judged::Reclaim, "ts より後に起動＝pid の再利用");
        assert_eq!(judge(&line, |_| None), Judged::Reclaim, "process が無い");
        assert_eq!(judge(&line, |pid| (pid == 42).then_some(0)), Judged::Live(3), "札の pid を引く");
    }

    /// 札の持ち主の生存は**共有の probe**（`fleet::store::started_ms`）で読む: 生きている pid（自分）は
    /// 起動時刻（now 以前）を返し、回収済みの pid（起こして wait した子）は `None`＝回収側。
    #[test]
    fn admission_ticket_probe_reads_live_and_reaped_pids() {
        let own = ticket_started_ms(std::process::id());
        assert!(own.is_some_and(|ms| ms <= now_ms()), "自分の pid は起動時刻を返す（now 以前）: {own:?}");
        let mut child = std::process::Command::new("true").spawn().expect("子を起こせる");
        let reaped = child.id();
        child.wait().expect("子を回収できる");
        assert_eq!(ticket_started_ms(reaped), None, "回収済みの pid は None（回収側）");
    }

    /// record の `slot=` は回収の数を落とさない（Granted は `reclaimed:n` だけ・縮退は
    /// `<名>,reclaimed:n`・回収 0 は名だけ）。
    #[test]
    fn admission_slot_detail_keeps_the_reclaimed_count() {
        assert_eq!(slot_detail(Slot::Granted, 0), "granted");
        assert_eq!(slot_detail(Slot::Granted, 2), "reclaimed:2");
        assert_eq!(slot_detail(Slot::Degraded, 0), "degraded");
        assert_eq!(slot_detail(Slot::Degraded, 1), "degraded,reclaimed:1");
        assert_eq!(slot_detail(Slot::Unmeasured, 0), "unmeasured");
        assert_eq!(slot_detail(Slot::Unmeasured, 3), "unmeasured,reclaimed:3");
    }
}
