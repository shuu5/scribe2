//! 器が起こす子 process を cgroup の transient scope で封じ込める（設計
//! docs/design/gate-cost.md §4・ADR-0021 §2.2）。
//!
//! **止めない、縮退する**（設計 §2）。`systemd-run` の無い host・scope を作れない host・
//! 箱を作れない host では包まずに素のまま撃ち、record に `confined=false reason=<閉じた
//! enum の名>` を残す。ゆえにこの境界は**行為を止めうる判定を返さない**——ADR-0014 §2.1 の
//! guard の定義に当たらないので、**極性一覧には載せない**（設計 §4.5・受付と同じ）。
//!
//! 現物と設計の差（unit 名の pid と通し番号・probe・箱を作れない host・終端行の固定形・行の終端の
//! [`release`]）は設計 §4.4 の
//! errata に写してある。
//!
//! **値をこの file に焼かない**（憲法 C1 / C5）。箱の大きさと CPU の重みは [`Caps`] が
//! manifest の 3 行から読む（読めない周は理由付き＝manifest そのものが読めない周は
//! `reason=manifest-unreadable`・行が欠ける周は `no-rules`）。`MemoryHigh` は付けない
//! （係数を持たない・設計 §4.2）。
//!
//! peak の読みは **scope の内側**で行う（設計 §4.3）。transient scope は最後の process の
//! 終了で cgroup dir ごと消えるので、外から終了後に読む形は成立しない。包みの `sh -c` が
//! 自分の `/proc/self/cgroup` から数を読み、stdout の終端に固定形 1 行で出す。器はその行を
//! [`read_usage`]（pure・in-file の歯が fixture 文字列で測る）で剥がす。
//!
//! argv で包む起動（runner / lens が起こす claude・[`wrap_command`]）は epilogue を持てないので、
//! 器が**走行中に** `memory.peak` を sample する（設計 §13・`s2-07l.273`）: scope の cgroup dir は
//! `systemctl show` で 1 回解き（[`control_group_of`]）、[`Sampler`] が周期ごとに読んで最後に読めた
//! 値を保つ（high-water mark ゆえそれが peak）。終端で 1 回読む形は、最後の process の終了で dir が
//! 消えた正常系を測れない。

use crate::name::NAME;
use crate::rules::manifest::Manifest;
use crate::seat::{embedded_manifest, int_rule_of, RuleRead};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

/// scope を作る道具。**PATH で解決する**（絶対 path を焼かない・env も読まない）。
const SYSTEMD_RUN: &str = "systemd-run";

/// 行を撃つ shell。
const SHELL: &str = "sh";

/// host の memory を測る面。
const MEMINFO: &str = "/proc/meminfo";

/// 包みが stdout の終端に出す固定形の見出し。
///
/// **`{` で始めない**——[`super::gate::last_json_object`] は lens の verdict と runner の
/// 質問 record を stdout の末尾から探すので、`{` で始まる終端行を足すとその経路と衝突する
/// （lens-132d L5）。
const USAGE_HEAD: &str = "confine-usage";

/// job 1 つが要る memory の rules 行。
const JOB_MEMORY_ROW: &str = "gate.job_memory_mb";

/// 席と host のために残す memory の rules 行。
const RESERVE_ROW: &str = "host.reserve_memory_mb";

/// scope に付ける CPU の重みの rules 行。
const CPU_WEIGHT_ROW: &str = "gate.cpu_weight";

/// 1 MiB の byte 数（peak の換算）。
const MIB: u64 = 1024 * 1024;

/// 封じ込めの record（`reason=`）に載る閉じた語彙。
///
/// **理由を自由文にしない**（憲法 C3.3・設計 §4.2）。包めなかった 5 つと、包んだ箱の中で
/// 起きた 3 つ（oom / signal / unknown）を 1 つの列挙で持つ——record の読み手はどちらも同じ `reason=`
/// で読むので、語彙が 2 面に割れると「外からの kill」と弁別できない（lens-132d L1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// `systemd-run` を起動できない（PATH に無い host）。
    NoTool,
    /// `systemd-run` は在るが scope を作れない（user の session manager が無い host）。
    NoScope,
    /// 封じ込めの rules 行を読めない（manifest は読めたが行が無い・不発効・整数でない）。
    NoRules,
    /// 埋め込み manifest そのものを parse できない（`s2-07l.205`・[`RuleRead::ManifestUnreadable`]）。
    /// 「行が無い」と潰さない: tracked の埋め込みが壊れているのは器の欠陥で、record から読める形で残す。
    ManifestUnreadable,
    /// 箱の大きさが残らない（`MemTotal − host.reserve_memory_mb` が 0 以下）。
    NoRoom,
    /// scope の中で kernel が process を殺した（`memory.events` の `oom_kill` ≥ 1）。
    OomKill,
    /// 包みごと signal で死んだ（oom の代理・設計 §4.3）。
    Signal,
    /// 包みごと signal で死んだが、kernel の証拠（終端行の `oom_kill`）が無い / 読めない（runner の終端・
    /// 設計 pipeline.md §23）。外からの kill を oom-kill と読まない（C10）。
    Unknown,
}

/// [`Reason`] の全 variant（宣言順）。
pub const REASONS: &[Reason] = &[
    Reason::NoTool,
    Reason::NoScope,
    Reason::NoRules,
    Reason::ManifestUnreadable,
    Reason::NoRoom,
    Reason::OomKill,
    Reason::Signal,
    Reason::Unknown,
];

impl Reason {
    /// record に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoTool => "no-systemd-run",
            Self::NoScope => "no-scope",
            Self::NoRules => "no-rules",
            Self::ManifestUnreadable => "manifest-unreadable",
            Self::NoRoom => "no-room",
            Self::OomKill => "oom-kill",
            Self::Signal => "signal",
            Self::Unknown => "unknown",
        }
    }

    /// 3 線を読めなかった理由を record の語彙へ写す（manifest が読めない周だけ別の 1 語・それ以外は
    /// 従来どおり `no-rules`）。
    fn of_rule_read(read: RuleRead) -> Self {
        match read {
            RuleRead::ManifestUnreadable => Self::ManifestUnreadable,
            // `.433` の 2 理由（値が文字列でない・字面が閉じた表に無い）も既存の「行を読めない」側の
            // 1 語に倒す＝封じ込めが読む 3 線は整数の行だけなので、包みの挙動は変わらない。
            RuleRead::Missing
            | RuleRead::Disabled
            | RuleRead::NotInt
            | RuleRead::NotStr
            | RuleRead::NotInTable => Self::NoRules,
        }
    }
}

/// 1 起動を包めたか。**bool で持たない**（包めなかった周は理由まで record へ出す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confinement {
    /// scope で包めた（作った unit 名）。
    Confined {
        /// transient scope の unit 名。
        unit: String,
    },
    /// 包めなかった（閉じた理由）。
    Unconfined(Reason),
}

impl Confinement {
    /// 包めたか（record の `confined=`）。
    pub fn confined(&self) -> bool {
        matches!(*self, Self::Confined { .. })
    }

    /// 包めなかった理由（包めた周は `None`）。
    pub fn reason(&self) -> Option<Reason> {
        match *self {
            Self::Confined { .. } => None,
            Self::Unconfined(found) => Some(found),
        }
    }
}

/// 箱の上限の種。**2 種しかない**（設計 §4.2・行ごとの値を持たない＝rules 行を増やさない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// `{jobs}` を持つ行 = 実効 jobs × `gate.job_memory_mb`。
    PerJob(u64),
    /// それ以外（`{jobs}` を持たない verify 行）= `MemTotal − host.reserve_memory_mb`。
    HostReserve,
}

/// 行の穴から上限の種を選ぶ。**`{jobs}` を持つ行だけが job の箱に入る**（設計 §4.2）。
///
/// 見るのは**置換前の行**である——置換後の字面からは、その行が並列度を受け取ったのか
/// たまたま同じ数を書いていたのかを弁別できない。
pub fn limit_of(line: &str, jobs: u64) -> Limit {
    if line.contains(super::declaration::JOBS_HOLE) {
        Limit::PerJob(jobs)
    } else {
        Limit::HostReserve
    }
}

/// 封じ込めが読む 3 つの線（値は manifest が持つ・憲法 C1 / C5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// job 1 つが要る memory（MiB・rules 行 `gate.job_memory_mb`）。
    pub job_memory_mb: u64,
    /// 席と host のために常に残す memory（MiB・rules 行 `host.reserve_memory_mb`）。
    pub reserve_memory_mb: u64,
    /// 便の scope に付ける CPU の重み（rules 行 `gate.cpu_weight`）。
    pub cpu_weight: u64,
}

impl Caps {
    /// 埋め込み manifest から読む。**manifest が読めない・1 行でも欠ければ理由付きの Err**＝包まない
    /// （止めない・理由は [`Reason::of_rule_read`] が record の語彙へ写す）。
    ///
    /// 読むのは埋め込みだけである——封じ込めの起動点は 3 つ（gate の verify 行・runner・
    /// claude）で、そのうち 2 つは `--rules` を受ける口を持たない。片方だけ override が
    /// 効く形にすると、同じ host の 3 つの箱が別々の値で走る。
    pub fn embedded() -> Result<Self, RuleRead> {
        Self::of(embedded_manifest())
    }

    /// 読んだ manifest（または読めなかった理由）から 3 線を組む（pure・in-file の歯が parse に失敗する
    /// text から `Manifest::parse` の Err を作って撃つ＝埋め込みは差し替えられない）。
    ///
    /// 行の読みは席と同じ [`int_rule_of`] 1 本（不在 / 不発効 / 整数でない、を別の variant で返す）。
    pub fn of(read: Result<Manifest, RuleRead>) -> Result<Self, RuleRead> {
        let manifest = read?;
        Ok(Self {
            job_memory_mb: int_rule_of(&manifest, JOB_MEMORY_ROW)?,
            reserve_memory_mb: int_rule_of(&manifest, RESERVE_ROW)?,
            cpu_weight: int_rule_of(&manifest, CPU_WEIGHT_ROW)?,
        })
    }
}

/// 1 起動を包む材料。
pub struct Wrap<'a> {
    /// transient scope の unit 名（[`unit_name`] が組む）。
    pub unit: &'a str,
    /// この起動に掛ける上限の種。
    pub limit: Limit,
    /// 封じ込めの 3 線（読めない周は理由付きの Err・[`Caps::embedded`] の結果をそのまま渡す）。
    pub caps: Result<Caps, RuleRead>,
}

/// 便の 1 起動の unit 名（`<NAME>-<場所>-<段>-<n>-<pid>-<seq>`）。
///
/// 設計 §4.2 は `<NAME>-<run>-<段>-<n>` を書くが、**同じ id を別 process が同時に測る周**
/// （歯の並列走行）で transient scope の名が衝突し、2 本目が起動できず偽の RED になる。
/// 場所（gate では便の worktree の dir 名 = run id）に pid を足して一意にする
/// （errata は設計 §4 に記した）。`<n>` は `verify.jsonl` の record 番号である。
///
/// **同じ process が同じ `<n>` を 2 度撃つ周**（land の追随 → 再 gate → main 実測）でも名を
/// 分けるため、末尾に process 内の通し番号 `<seq>` を足す（`s2-07l.234`・設計 §4.4 errata）。
/// 1 周目の scope が孤児の process で active のまま残っても、2 周目は同名で衝突しない。
pub fn unit_name(place: &str, stage: &str, n: usize) -> String {
    format!(
        "{NAME}-{}-{}-{n}-{}-{}",
        tame(place),
        tame(stage),
        std::process::id(),
        next_seq()
    )
}

/// unit 名の通し番号（process の起動ごとに 0 から・呼ぶたびに 1 進む）。
static SEQ: AtomicU64 = AtomicU64::new(0);

/// 通し番号を 1 つ取る。
fn next_seq() -> u64 {
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// scope を片付ける道具。**PATH で解決する**（[`SYSTEMD_RUN`] と同じ）。綴りはこの 1 定数だけで、tick の unit の有効化・撤去
/// （`crate::seat::tick::install`・設計 seat-heartbeat.md §3）も同じ定数を撃つ。
pub(crate) const SYSTEMCTL: &str = "systemctl";

/// unit が既に無い周に `systemctl` が stderr へ出す字面。
const NOT_LOADED: &str = "not loaded";

/// 行の終端で scope を片付けた結果（閉じた enum・record の `scope=`）。
///
/// **判定の極性を持たない**（設計 §4.5）: 片付けに失敗しても行は赤にしない（縮退・憲法 C11.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Released {
    /// unit が既に無い（最後の process の終了で消えた・正常＝[`release_scope`] の呼び手の record には書かず、
    /// runner / lens の stderr の `scope=` 行だけが `gone` を書く・設計 §13）。
    Gone,
    /// scope の中に残った process を殺した。
    Killed,
    /// `systemctl` が別の理由で断った（rc・signal で死んだ周は 255）。
    Failed(u8),
    /// `systemctl` を起動できない（PATH に無い host）。
    NoTool,
}

impl Released {
    /// record に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gone => "gone",
            Self::Killed => "killed",
            Self::Failed(_) => "failed",
            Self::NoTool => "no-tool",
        }
    }
}

/// 行の終端で scope に残った process を **SIGKILL で全部**殺す（設計 §4.4 errata・`s2-07l.234`）。
///
/// §4.3 の「最後の process の終了で scope は消える」は、行が fixture の server や shell を
/// 孤児で残す周に成立しない——scope は active のまま CPU を焼き、次の周の同名の相手になる。
/// SIGTERM の猶予は待たない（子は既に終わっている・残りは孤児だけ）。
///
/// kill の後に `reset-failed` を 1 回撃つ（設計 gate-cost.md §25・`s2-07l.421`）: kill で殺した scope は
/// `--collect` があっても failed のまま残る周がある。reset の結果は record に写さない（unit が無い周の
/// 「not loaded」も含め、[`Released`] は kill の結果だけで決まる＝閉じた 4 値は不変）。
pub fn release(unit: &str) -> Released {
    let scope = format!("{unit}.scope");
    let out = Command::new(SYSTEMCTL)
        .args(["--user", "kill", "--signal=SIGKILL"])
        .arg(&scope)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    let _reset = Command::new(SYSTEMCTL)
        .args(["--user", "reset-failed"])
        .arg(&scope)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    released_of(out)
}

/// `systemctl kill` の結果を閉じた enum へ読む（pure・in-file の歯が fixture で測る）。
fn released_of(out: std::io::Result<Output>) -> Released {
    let Ok(out) = out else {
        return Released::NoTool;
    };
    if out.status.success() {
        return Released::Killed;
    }
    if String::from_utf8_lossy(&out.stderr).contains(NOT_LOADED) {
        return Released::Gone;
    }
    let rc = out.status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(u8::MAX);
    Released::Failed(rc)
}

/// scope に残った process の pid が並ぶ file（cgroup v2・1 行 1 pid）。
const CGROUP_PROCS: &str = "cgroup.procs";

/// scope を止める**直前**に数えた、scope に残っていた process の数（設計 pipeline.md §20）。
///
/// **0 と「測れなかった」を融合しない**（C10・[`Peak`] と同じ型）: cgroup の path を解けない周・
/// dir が消えた後・`cgroup.procs` の無い host は [`Orphans::Unreadable`] で、行には `-` と書く。
/// 「読めた上で 1 本も残っていなかった」は測れた事実なので [`Orphans::Count`] の 0 である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orphans {
    /// 数えられた本数（0 を含む）。
    Count(usize),
    /// 数えられなかった。
    Unreadable,
}

impl Orphans {
    /// 行に書く字面（`Count` は 10 進・`Unreadable` は `-`・[`Peak::word`] と同じ極性）。
    pub fn word(self) -> String {
        match self {
            Self::Count(found) => found.to_string(),
            Self::Unreadable => "-".to_owned(),
        }
    }
}

/// `cgroup.procs` の中身を数える（pure・in-file の歯が fixture 文字列で測る）: 空でない行の数。
///
/// `Err`（file が無い・読めない）は [`Orphans::Unreadable`]。**空の file は `Count(0)`** である
/// ——読めた上で 1 本も残っていなかった周であり、読めなかった周と同じ字面にしない。
pub fn orphans_from(read: std::io::Result<String>) -> Orphans {
    match read {
        Err(_) => Orphans::Unreadable,
        Ok(text) => Orphans::Count(text.lines().filter(|line| !line.trim().is_empty()).count()),
    }
}

/// `<root>/<control_group>/cgroup.procs` を 1 回読んで数える（file の read 1 回・待ちは無い）。
///
/// つなぎ方は [`peak_of`] と同じ＝`ControlGroup` の先頭の `/` を剥がして root を捨てない。
pub fn orphans_of(root: &Path, control_group: &str) -> Orphans {
    let relative = control_group.trim_start_matches('/');
    orphans_from(std::fs::read_to_string(root.join(relative).join(CGROUP_PROCS)))
}

/// 止める**直前**の孤児の数を数える（呼び手は [`release`] を撃つ**前に**これを撃つ・設計 pipeline.md §20）。
///
/// 止めた後に数える形は成立しない: `systemctl kill` の後の scope は空で、最後の process が消えた時点で
/// cgroup dir ごと消える——どちらも 0 と `-` に化けて「何を殺したか」が残らない。
///
/// root は [`CGROUP_ROOT`] 固定である。runner / lens の `--cgroup-root` は走行中の peak の読みの
/// 差し替え口で、終端の 1 行を組む口（headless/runner.rs）は unit 名しか持たない——root を差し替えて
/// 測りたい歯は pure な [`orphans_of`] を直に撃つ。
pub fn orphans_before_release(unit: &str) -> Orphans {
    control_group_of(unit).map_or(Orphans::Unreadable, |group| orphans_of(Path::new(CGROUP_ROOT), &group))
}

/// 包めた起動の scope を片付け、**record に書く周だけ**結果を返す。
///
/// 包めなかった周は撃たない（scope が無い）。`Gone` は正常なので `None`＝record は変わらない。
///
/// **孤児の数はここでは数えない**（[`orphans_before_release`] は runner / lens の終端の 1 行の口だけが
/// 撃つ）——この口は gate の verify 行ごとの片付けも通り、行の終端の `systemctl` の呼出は 1 行
/// `kill` + `reset-failed` の 2 本で pin されている（`show` を足すと、その本数の歯が落ちる）。
pub fn release_scope(confinement: &Confinement) -> Option<Released> {
    match confinement {
        Confinement::Confined { unit } => Some(release(unit)).filter(|found| *found != Released::Gone),
        Confinement::Unconfined(_) => None,
    }
}

/// unit 名から作り手の pid を読む（pure・設計 gate-cost.md §38 形 1）: `-` で割った列の**末尾から 2 番目**。
///
/// [`unit_name`] の場所と段は [`tame`] が記号を `-` に畳むので割れ数は名ごとに違うが、末尾の 2 つ（pid と
/// 通し番号）は畳まれない。割れ数が足りない名・数でない名は `None`（推測で埋めない）。
pub fn creator_pid(unit: &str) -> Option<u32> {
    unit.rsplit('-').nth(1)?.parse().ok()
}

/// 残骸の一覧を取る `systemctl` の引数（設計 §38 形 2）: active な scope だけを legend と pager を止めた素の形で、
/// pattern は器の名から導いた 1 語。
pub fn list_args() -> Vec<String> {
    vec![
        "--user".to_owned(),
        "list-units".to_owned(),
        "--no-legend".to_owned(),
        "--no-pager".to_owned(),
        "--plain".to_owned(),
        "--type=scope".to_owned(),
        "--state=active".to_owned(),
        format!("{NAME}-*.scope"),
    ]
}

/// active な scope の unit 名（`.scope` 付き）を 1 回引く（PATH 解決・[`SYSTEMCTL`]）。
///
/// 道具が無い周と rc 非 0 の周は `None`＝**空の一覧（0 件）と融合しない**（C10）。
pub fn list_scopes() -> Option<Vec<String>> {
    let out = Command::new(SYSTEMCTL)
        .args(list_args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    listed_from(out)
}

/// `systemctl list-units` の結果を読む（pure・in-file の歯が fixture で測る）: 行頭の unit 名だけを取る。
pub fn listed_from(out: std::io::Result<Output>) -> Option<Vec<String>> {
    let out = out.ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(str::to_owned)
            .collect(),
    )
}

/// 畳む相手を決める（pure・設計 §38 形 3）: 名が器の scope の形で作り手の pid を読め、その pid が `alive` に
/// **無い**行だけを残す（`.scope` を剥がした unit 名で返す＝[`release`] へそのまま渡せる）。
pub fn reap_targets(rows: &[String], alive: &std::collections::BTreeSet<u32>) -> Vec<String> {
    let head = format!("{NAME}-");
    rows.iter()
        .filter_map(|row| row.strip_suffix(".scope"))
        .filter(|unit| unit.starts_with(&head))
        .filter(|unit| creator_pid(unit).is_some_and(|pid| !alive.contains(&pid)))
        .map(str::to_owned)
        .collect()
}

/// 一覧の行の作り手のうち生きている pid（[`started_ms`](crate::fleet::store::started_ms) が `Started`・
/// **`Unreadable` は生きている側**＝測れない作り手の scope は畳まない）。
fn alive_creators(rows: &[String]) -> std::collections::BTreeSet<u32> {
    use crate::fleet::store::{started_ms, Probe};
    rows.iter()
        .filter_map(|row| creator_pid(row.strip_suffix(".scope").unwrap_or(row)))
        .filter(|pid| !matches!(started_ms(*pid), Probe::Absent))
        .collect()
}

/// 作り手が死んだ scope を畳んだ結果（止める口の行の `scopes=`・設計 §38 形 5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaped {
    /// 畳んだ数と一覧の件数。
    Counted {
        /// 既存の片付けで畳めた数（`Killed` か `Gone`）。
        reaped: usize,
        /// 一覧の件数。
        listed: usize,
    },
    /// 一覧を測れなかった（0 件と融合しない）。
    Unmeasured,
}

impl Reaped {
    /// 行に書く字面（`<畳んだ数>/<一覧の件数>`・測れなかった周は `-`）。
    pub fn word(self) -> String {
        match self {
            Self::Counted { reaped, listed } => format!("{reaped}/{listed}"),
            Self::Unmeasured => "-".to_owned(),
        }
    }
}

/// 作り手が死んだ器の scope を一覧から見つけ、既存の [`release`] で 1 本ずつ畳む（設計 §38 形 4）。
pub fn reap_orphan_scopes() -> Reaped {
    let Some(rows) = list_scopes() else {
        return Reaped::Unmeasured;
    };
    let reaped = reap_targets(&rows, &alive_creators(&rows))
        .iter()
        .filter(|unit| matches!(release(unit), Released::Killed | Released::Gone))
        .count();
    Reaped::Counted { reaped, listed: rows.len() }
}

/// cgroup v2 の root（`memory.peak` の置き場の頭・設計 §13）。**typed な既定値**で、runner / lens の
/// `--cgroup-root DIR` が差し替える（env は読まない・C2.2）。包みの epilogue（[`script`]）は自分の字面を
/// 持つのでここを参照しない（触らない・設計 §13）。
pub const CGROUP_ROOT: &str = "/sys/fs/cgroup";

/// cgroup dir の中の high-water mark の file 名。
const MEMORY_PEAK: &str = "memory.peak";

/// `systemctl show` で引く property（scope の cgroup の path・root からの相対）。
const CONTROL_GROUP: &str = "ControlGroup";

/// 走行中に読んだ scope の peak（閉じた型・設計 §13）。
///
/// **読めない周を 0 に融合しない**（C10）: `memory.peak` の無い kernel・dir が消えた後・parse 不能は
/// [`Peak::Unreadable`] で、行には `-` と書く。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Peak {
    /// 読めた値（byte）。
    Bytes(u64),
    /// 1 度も読めていない。
    Unreadable,
}

impl Peak {
    /// 行に書く字面（`Bytes` は 10 進・`Unreadable` は `-`）。
    pub fn word(self) -> String {
        match self {
            Self::Bytes(bytes) => bytes.to_string(),
            Self::Unreadable => "-".to_owned(),
        }
    }

    /// 次の sample を畳む: 読めた値なら置き換え、読めない周は自分を保つ（high-water mark ゆえ最後に
    /// 読めた値が peak・dir が消えた後の周で値を失わない）。
    pub fn absorb(self, next: Self) -> Self {
        match next {
            Self::Bytes(_) => next,
            Self::Unreadable => self,
        }
    }
}

/// `memory.peak` の中身を読む（pure・in-file の歯が fixture で測る）: trim して 10 進の u64・
/// `Err` / 空 / parse 不能は [`Peak::Unreadable`]。
pub fn peak_from(read: std::io::Result<String>) -> Peak {
    read.ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .map_or(Peak::Unreadable, Peak::Bytes)
}

/// `<root>/<control_group>/memory.peak` を 1 回読む（file の read 1 回・待ちは無い）。
///
/// `ControlGroup` は `/user.slice/…` の絶対形で来るので**先頭の `/` を剥がして**つなぐ——`Path::join` は
/// 絶対 path を渡されると root を捨てる（`--cgroup-root` の差し替えが黙って効かなくなる）。
pub fn peak_of(root: &Path, control_group: &str) -> Peak {
    let relative = control_group.trim_start_matches('/');
    peak_from(std::fs::read_to_string(root.join(relative).join(MEMORY_PEAK)))
}

/// scope の cgroup の path（root からの相対）を `systemctl --user show <unit>.scope -p ControlGroup --value`
/// で 1 回引く（PATH 解決・子 process・[`SYSTEMCTL`]）。解けない周は `None`＝peak は読まない。
pub fn control_group_of(unit: &str) -> Option<String> {
    let out = Command::new(SYSTEMCTL)
        .args(["--user", "show"])
        .arg(format!("{unit}.scope"))
        .args(["-p", CONTROL_GROUP, "--value"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    control_group_from(out)
}

/// `systemctl show` の結果を読む（pure・in-file の歯が fixture で測る）: rc≠0 / 起動不能 / 空は `None`・
/// 末尾の改行は trim する。
pub fn control_group_from(out: std::io::Result<Output>) -> Option<String> {
    let out = out.ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// 走行中の scope の peak の sample（設計 §13）。runner は stream の行ごと・lens は poll の周ごとに
/// [`Sampler::sample`] を撃ち、終端で [`Sampler::peak`] を行に写す。
///
/// cgroup の path は**最初の sample で 1 回だけ**解く（起動の直後ではない）——`systemd-run --scope` は
/// spawn が返った後に scope を作るので、直後の `show` は空を返しうる。1 回解けなかった周は撃ち直さず
/// `Unreadable` のまま（`show` を周期で撃たない）。包めなかった周（[`Confinement::Unconfined`]）は
/// `show` も read も撃たない。
pub struct Sampler<'a> {
    /// 包めた周の unit 名（包めなかった周は `None`）。
    unit: Option<&'a str>,
    /// cgroup の root（[`CGROUP_ROOT`] か `--cgroup-root`）。
    root: &'a Path,
    /// 解いた cgroup の path（外側の `None` は未だ解いていない）。
    control_group: Option<Option<String>>,
    /// 最後に読めた値。
    peak: Peak,
}

impl<'a> Sampler<'a> {
    /// 包みの結果と root から。
    pub fn of(confinement: &'a Confinement, root: &'a Path) -> Self {
        let unit = match confinement {
            Confinement::Confined { unit } => Some(unit.as_str()),
            Confinement::Unconfined(_) => None,
        };
        Self { unit, root, control_group: None, peak: Peak::Unreadable }
    }

    /// 1 回 sample する（`memory.peak` の read 1 回・待ちは足さない）。
    pub fn sample(&mut self) {
        let Some(unit) = self.unit else {
            return;
        };
        let resolved = self.control_group.get_or_insert_with(|| control_group_of(unit));
        if let Some(control_group) = resolved.as_deref() {
            self.peak = self.peak.absorb(peak_of(self.root, control_group));
        }
    }

    /// 最後に読めた値（1 度も読めていなければ [`Peak::Unreadable`]）。
    pub fn peak(&self) -> Peak {
        self.peak
    }
}

/// systemd の unit 名に置ける字だけへ畳む。
fn tame(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

/// `sh -c <line>` を scope で包む。包めない周は**素の** `sh -c <line>` を返す（止めない）。
///
/// 包めた周だけ終端行の epilogue を足す（設計 §4.3）——scope の外で `/proc/self/cgroup` を
/// 読んでも、それは自分の箱ではない別の cgroup の数である。
///
/// 子の env から [`PANE_ENV`] を外すのは [`wrap_command`] と同じ 1 点（[`wrap`]）である。
pub fn wrap_line(line: &str, entry: &Wrap<'_>) -> (Command, Confinement) {
    wrap(entry, |confined| if confined { shell(&script(line, entry.unit)) } else { shell(line) })
}

/// 起動した席の pane を指す tmux の変数。
///
/// runner / lens は pane を持たない（席ではない・設計 seat-roles.md §4）。起動側の管理席の
/// 値を継承させると、子の hook が `--pane` で**管理席の打刻**へ書き込む（他 process の打刻の
/// 混入・C3.3）。
const PANE_ENV: &str = "TMUX_PANE";

/// 既に組んだ起動を scope で包む。**終端行は足さない**（argv の起動に epilogue は書けない
/// ＝peak は測らない・設計 §4.3 の記録先は verify 行と runner の 2 面である）。
///
/// cwd・env・stdio は `Command` から読み戻せないので、**包んだ後に**外側へ付けること。
/// 子の env から [`PANE_ENV`] を外すのは [`wrap_line`] と同じ 1 点（[`wrap`]）である——`pipe` の外で
/// 席の pane から `runner` / `lens` を単体起動する周（headless の `build`）もここを通る。
pub fn wrap_command(cmd: Command, entry: &Wrap<'_>) -> (Command, Confinement) {
    wrap(entry, |_| cmd)
}

/// 起動の包みの 2 口の本体（設計 seat-roles.md §4）。`inner` は包める周（`true`）と包めない周で別の
/// 起動を組める（`wrap_line` は包める周だけ終端行を足す）。**子の env から [`PANE_ENV`] だけを外す
/// 唯一の点**（ADR-0022 §2.3）——runner / lens / verify 行のどの経路もここを通る。env は足さない（C2.2）。
fn wrap(entry: &Wrap<'_>, inner: impl FnOnce(bool) -> Command) -> (Command, Confinement) {
    let (mut cmd, confinement) = match fitting(entry) {
        Err(reason) => (inner(false), Confinement::Unconfined(reason)),
        Ok((caps, mb)) => (
            scope(inner(true), mb, entry, &caps),
            Confinement::Confined { unit: entry.unit.to_owned() },
        ),
    };
    cmd.env_remove(PANE_ENV);
    (cmd, confinement)
}

/// 箱の大きさ（MiB）を決める。包めない周は閉じた理由を返す。
fn fitting(entry: &Wrap<'_>) -> Result<(Caps, u64), Reason> {
    let caps = entry.caps.map_err(Reason::of_rule_read)?;
    let meminfo = std::fs::read_to_string(MEMINFO).unwrap_or_default();
    let mb = limit_mb(entry.limit, &caps, &meminfo).ok_or(Reason::NoRoom)?;
    let host = limit_mb(Limit::HostReserve, &caps, &meminfo).ok_or(Reason::NoRoom)?;
    probe(&caps, host)?;
    Ok((caps, mb))
}

/// scope を 1 つだけ試しに作り、結果をこの process の間だけ憶える。
///
/// **撃つ前に確かめる**のがここの要点である。`systemd-run` が在っても user の session
/// manager が無い host では scope を作れず、包んだ行は**撃たれないまま rc≠0** になる
/// ——偽の RED であって、縮退（設計 §2）ではない。1 便で何本撃っても probe は 1 回。
fn probe(caps: &Caps, host_mb: u64) -> Result<(), Reason> {
    static PROBED: OnceLock<Result<(), Reason>> = OnceLock::new();
    *PROBED.get_or_init(|| {
        let unit = format!("{NAME}-{}-probe", std::process::id());
        let mut cmd = Command::new(SYSTEMD_RUN);
        cmd.args(scope_args(&unit, host_mb, caps));
        cmd.arg("--").arg(SHELL).arg("-c").arg("exit 0");
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        probe_outcome(cmd.status())
    })
}

/// probe の起動結果を読む（pure・in-file の歯が実 process を撃たずに 3 通りを作る）。
///
/// 起動できない周は道具が無い・rc 0 は scope を作れた・それ以外は道具は在るが scope を作れない。
fn probe_outcome(started: std::io::Result<std::process::ExitStatus>) -> Result<(), Reason> {
    match started {
        Err(_) => Err(Reason::NoTool),
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(Reason::NoScope),
    }
}

/// `sh -c <line>` の素の起動。
fn shell(line: &str) -> Command {
    let mut cmd = Command::new(SHELL);
    cmd.arg("-c").arg(line);
    cmd
}

/// 中身の起動を `systemd-run --user --scope` で包む。
fn scope(inner: Command, mb: u64, entry: &Wrap<'_>, caps: &Caps) -> Command {
    let mut outer = Command::new(SYSTEMD_RUN);
    outer.args(scope_args(entry.unit, mb, caps));
    outer.arg("--");
    outer.arg(inner.get_program());
    outer.args(inner.get_args());
    outer
}

/// scope の引数（`--` の手前まで）。**`MemoryHigh` は付けない**（設計 §4.2）。
///
/// `OOMPolicy=continue` は包みを systemd の OOM 停止から外すためである——既定の `stop` では
/// kernel が箱の中の 1 process を殺した直後に unit ごと止められ、包みが終端行を出す前に
/// SIGTERM で死ぬ（設計 §4.2・lens-132c M1）。
///
/// `--collect` は終了後の unit を failed の周も含めて unload させる（設計 §25・`s2-07l.421`）——無いと
/// 正常終了した scope が `failed` で host に残り、同じ host の他の観察を汚す。probe の scope もこれで消える。
fn scope_args(unit: &str, mb: u64, caps: &Caps) -> Vec<String> {
    vec![
        "--user".to_owned(),
        "--scope".to_owned(),
        "--quiet".to_owned(),
        "--collect".to_owned(),
        format!("--unit={unit}"),
        "-p".to_owned(),
        format!("MemoryMax={mb}M"),
        "-p".to_owned(),
        format!("CPUWeight={}", caps.cpu_weight),
        "-p".to_owned(),
        "OOMPolicy=continue".to_owned(),
    ]
}

/// 箱の大きさ（MiB）。**2 種しかない**（設計 §4.2）。読めない / 残らない周は `None`。
pub fn limit_mb(limit: Limit, caps: &Caps, meminfo: &str) -> Option<u64> {
    match limit {
        Limit::PerJob(jobs) => jobs.checked_mul(caps.job_memory_mb).filter(|mb| *mb > 0),
        Limit::HostReserve => mem_total_mb(meminfo)?
            .checked_sub(caps.reserve_memory_mb)
            .filter(|mb| *mb > 0),
    }
}

/// `/proc/meminfo` の `MemTotal` を MiB で読む（pure）。読めなければ `None`＝0 と書かない。
pub fn mem_total_mb(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// 包みの `sh -c` に足す終端行の epilogue（設計 §4.3）。
///
/// **自分の scope の中でしか読まない**: `/proc/self/cgroup` の path の末尾が `<unit>.scope`
/// でない周（= 包みの外）は 1 行も出さない——別の箱の数を自分の peak として記録しない。
/// 行の rc は退避して最後に返す（終端行を出すために rc を失わない）。
fn script(line: &str, unit: &str) -> String {
    format!(
        "{line}\n\
         __rc=$?\n\
         __cg=\n\
         while IFS= read -r __l; do case \"$__l\" in 0::*) __cg=${{__l#0::}};; esac; done < /proc/self/cgroup\n\
         case \"$__cg\" in */{unit}.scope) __d=/sys/fs/cgroup$__cg;; *) __d=;; esac\n\
         if [ -n \"$__d\" ]; then\n\
         __peak=-\n\
         [ -r \"$__d/memory.peak\" ] && __peak=$(cat \"$__d/memory.peak\")\n\
         __oom=-\n\
         [ -r \"$__d/memory.events\" ] && __oom=$(while read -r __k __v; do case \"$__k\" in oom_kill) echo \"$__v\";; esac; done < \"$__d/memory.events\")\n\
         printf '{USAGE_HEAD} peak_bytes=%s oom_kill=%s\\n' \"${{__peak:--}}\" \"${{__oom:--}}\"\n\
         fi\n\
         exit $__rc\n"
    )
}

/// 包みが出した終端行の読み。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// scope の peak（MiB）。**終端行が無い / 読めない周は `None`**（0 と書かない）。
    pub peak_mb: Option<u64>,
    /// scope の中で kernel が殺した数。**終端行が無い / 読めない周は `None`**（0 と「測れない」を融合しない）。
    pub oom_kill: Option<u64>,
}

/// stdout の**最後の終端行**を剥がす（pure・in-file の歯が fixture 文字列で測る）。
///
/// 終端行が無い周（`memory.peak` の無い kernel・包みの外で撃った周・行が途中で `exit` した
/// 周）は既定＝`peak_mb` も `oom_kill` も不明である。
pub fn read_usage(stdout: &str) -> Usage {
    let found = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with(USAGE_HEAD));
    let Some(line) = found else {
        return Usage::default();
    };
    let field = |key: &str| {
        line.split_whitespace()
            .find_map(|token| token.strip_prefix(key)?.parse::<u64>().ok())
    };
    Usage {
        peak_mb: field("peak_bytes=").map(|bytes| bytes / MIB),
        oom_kill: field("oom_kill="),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        control_group_from, creator_pid, limit_mb, limit_of, list_args, listed_from, mem_total_mb, next_seq, orphans_from,
        orphans_of, peak_from, peak_of, probe_outcome, read_usage, reap_targets, release_scope, released_of, scope_args,
        script, tame, unit_name, wrap_command, wrap_line, Caps, Confinement, Limit, Orphans, Peak, Reaped, Reason, Released,
        Sampler, Wrap, CGROUP_ROOT, PANE_ENV, REASONS,
    };
    use crate::order::is_declaration_order;
    use crate::rules::manifest::Manifest;
    use crate::seat::{manifest_read, RuleRead};
    use std::ffi::OsStr;
    use std::os::unix::process::ExitStatusExt;
    use std::path::Path;
    use std::process::{Command, ExitStatus, Output};

    /// 同じ引数の `unit_name` を 2 回呼ぶと**別の名**になる（追随の再 gate で同名が衝突しない・`s2-07l.234`）。
    /// `<n>` と pid の位置は変えない（別 process の一意性は pid のまま）。
    #[test]
    fn confine_unit_name_differs_for_the_same_arguments() {
        let first = unit_name("run-1", "detection", 7);
        let second = unit_name("run-1", "detection", 7);
        assert_ne!(first, second, "同じ引数でも別名");
        let head = format!("{}-run-1-detection-7-{}-", crate::name::NAME, std::process::id());
        assert!(first.starts_with(&head), "`<n>` と pid は従来の位置: {first}");
        assert!(second.starts_with(&head), "`<n>` と pid は従来の位置: {second}");
        let seq = |name: &str| name.strip_prefix(&head).and_then(|tail| tail.parse::<u64>().ok());
        assert!(seq(&first).is_some(), "末尾は通し番号: {first}");
        assert!(seq(&second) > seq(&first), "後の名の番号が大きい: {first} / {second}");
    }

    // flip-check: retroactive s2-07l.222
    /// `tame` の 3 分岐を text の境界で片側ずつ撃つ: 英数字・`.`・`_` は各々そのまま残り（`||` を `&&` にすると
    /// 残らない）、`/`・空白・`-`・非 ASCII は `-` へ畳む（`==` を `!=` にすると畳まれずに残る）。
    #[test]
    fn mutant_in_pipe_tame_keeps_only_unit_name_characters() {
        assert_eq!(tame("aZ9"), "aZ9", "英数字だけ");
        assert_eq!(tame("."), ".", "`.` だけ");
        assert_eq!(tame("_"), "_", "`_` だけ");
        assert_eq!(tame("/"), "-", "`/` は畳む");
        assert_eq!(tame("s2-07l.222/run 1_x"), "s2-07l.222-run-1_x", "混ぜた字");
        assert_eq!(tame("便"), "-", "非 ASCII は 1 字 1 本の `-`");
        assert_eq!(tame(""), "", "空は空");
    }

    // flip-check: retroactive s2-07l.222
    /// probe の起動結果の 3 分岐（`probe_outcome`）を実 process を撃たずに撃つ: 起動できない周は `NoTool`・
    /// rc 0 は包める・rc 1 は `NoScope`（guard を `true` / `false` に固定すると rc 0 と rc 1 の片側が落ちる）。
    #[test]
    fn mutant_in_pipe_probe_outcome_reads_the_three_results() {
        assert_eq!(probe_outcome(Ok(ExitStatus::from_raw(0))), Ok(()), "rc 0 は scope を作れた");
        assert_eq!(probe_outcome(Ok(ExitStatus::from_raw(256))), Err(Reason::NoScope), "rc 1 は scope を作れない");
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "no systemd-run");
        assert_eq!(probe_outcome(Err(missing)), Err(Reason::NoTool), "起動できない");
    }

    /// (a) 名から作り手の pid を読む: 素直な名・記号を `tame` が畳んだ名の 2 形で末尾から 2 番目が読め、割れ数が
    /// 足りない名・数でない名は `None`（設計 gate-cost.md §38 形 1）。
    #[test]
    fn pipe_scope_reap_creator_pid_reads_the_second_from_last() {
        let plain = format!("{}-run1-detection-7-4242-3", crate::name::NAME);
        let folded = format!("{}-{}-{}-2-5151-0", crate::name::NAME, tame("s2-07l.452/run 1"), tame("review-lens"));
        let cases = [
            (plain.as_str(), Some(4242)),
            (folded.as_str(), Some(5151)),
            (crate::name::NAME, None),
            ("vessel-run1-detection-7-x42-3", None),
        ];
        for (name, want) in cases {
            assert_eq!(creator_pid(name), want, "{name}（母集団 {} 形）", cases.len());
        }
        let own = unit_name("s2-07l.452/run 1", "review", 3);
        assert_eq!(creator_pid(&own), Some(std::process::id()), "unit_name の組む名は自分の pid を返す: {own}");
    }

    /// (b) 畳む相手: 死んだ作り手の unit だけが `.scope` を剥がして残り、生きた作り手の行と形に合わない行は
    /// 残らない（設計 §38 形 3）。
    #[test]
    fn pipe_scope_reap_targets_keep_only_dead_creators() {
        let name = crate::name::NAME;
        let rows: Vec<String> = vec![
            format!("{name}-run-a-contract-3-100-0.scope"),
            format!("{name}-run-b-runner-1-200-4.scope"),
            format!("{name}-300-probe.scope"),
            format!("{name}-run-c-lens-1-x-0.scope"),
            "other-run-d-contract-1-400-0.scope".to_owned(),
            format!("{name}-run-e-contract-1-500-0.service"),
        ];
        let alive: std::collections::BTreeSet<u32> = [200].into_iter().collect();
        assert_eq!(
            reap_targets(&rows, &alive),
            vec![format!("{name}-run-a-contract-3-100-0"), format!("{name}-300-probe")],
            "死んだ作り手の 2 本だけ（母集団 {} 行）",
            rows.len()
        );
        let all: std::collections::BTreeSet<u32> = [100, 200, 300].into_iter().collect();
        assert!(reap_targets(&rows, &all).is_empty(), "全員生きていれば 0 本");
        assert_eq!(Reaped::Counted { reaped: 1, listed: 2 }.word(), "1/2");
        assert_eq!(Reaped::Counted { reaped: 0, listed: 0 }.word(), "0/0", "空の一覧は測れた 0");
        assert_eq!(Reaped::Unmeasured.word(), "-", "測れなかった周は -");
    }

    /// (c) 一覧を取る呼出は `--user list-units` で始まり末尾の pattern は器の名から導き、rc 非 0 は「測れなかった」
    /// で空の一覧と弁別される（設計 §38 形 2）。
    #[test]
    fn pipe_scope_reap_list_call_and_unmeasured_are_distinct() {
        let args = list_args();
        assert_eq!(args.get(..2), Some(&["--user".to_owned(), "list-units".to_owned()][..]), "{args:?}");
        assert_eq!(args.last(), Some(&format!("{}-*.scope", crate::name::NAME)), "{args:?}");
        let out = |raw: i32, stdout: &str| {
            Ok(Output { status: ExitStatus::from_raw(raw), stdout: stdout.as_bytes().to_vec(), stderr: Vec::new() })
        };
        assert_eq!(listed_from(out(0, "")), Some(Vec::new()), "rc 0 の空は 0 件");
        assert_eq!(listed_from(out(1 << 8, "")), None, "rc 非 0 は測れなかった");
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "no systemctl");
        assert_eq!(listed_from(Err(missing)), None, "道具が無い");
        assert_eq!(
            listed_from(out(0, "vessel-a-1-9-0.scope loaded active running x\nvessel-b-1-8-0.scope loaded active running y\n")),
            Some(vec!["vessel-a-1-9-0.scope".to_owned(), "vessel-b-1-8-0.scope".to_owned()]),
            "行頭の unit 名だけ"
        );
    }

    /// 通し番号は呼ぶたびに単調に増える。
    #[test]
    fn confine_seq_increases_monotonically() {
        let taken: Vec<u64> = (0..4).map(|_| next_seq()).collect();
        assert!(taken.windows(2).all(|pair| pair.first() < pair.get(1)), "単調増加: {taken:?}");
    }

    /// [`Released::as_str`] の網羅（字面は record の語彙・重複しない）。
    #[test]
    fn confine_released_names_are_closed() {
        let all = [Released::Gone, Released::Killed, Released::Failed(1), Released::NoTool];
        let names: Vec<&str> = all.iter().map(|found| found.as_str()).collect();
        assert_eq!(names, vec!["gone", "killed", "failed", "no-tool"]);
        assert_eq!(Released::Failed(9).as_str(), "failed", "rc は字面を変えない");
    }

    /// `systemctl kill` の結果の読み: 無い unit は `Gone`・rc 0 は `Killed`・起動できない周は `NoTool`・
    /// その他は rc 付きの `Failed`（signal 死は 255）。
    #[test]
    fn confine_released_of_reads_the_systemctl_result() {
        let out = |raw: i32, stderr: &str| {
            Ok(Output { status: ExitStatus::from_raw(raw), stdout: Vec::new(), stderr: stderr.as_bytes().to_vec() })
        };
        assert_eq!(released_of(out(0, "")), Released::Killed);
        let gone = "Failed to kill unit x.scope: Unit x.scope not loaded.\n";
        assert_eq!(released_of(out(1 << 8, gone)), Released::Gone);
        assert_eq!(released_of(out(1 << 8, "Failed to connect to bus\n")), Released::Failed(1));
        assert_eq!(released_of(out(9, "")), Released::Failed(u8::MAX), "signal 死");
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "no systemctl");
        assert_eq!(released_of(Err(missing)), Released::NoTool);
    }

    /// 孤児は scope を**止める前**に数える（設計 pipeline.md §20 の約束 4）。
    ///
    /// fixture の cgroup dir で「止める前」（`cgroup.procs` に pid が 2 本）と「止めた後」（最後の
    /// process の終了で dir ごと消えた）を続けて読み、**後から数える形では残った数が `-` に化ける**
    /// ことまで測る（systemd の scope は起こさない・read だけ）。読みそのものの 3 通り（数えた本数・
    /// 読めた 0・読めない）は pure な [`orphans_from`] で、字面は [`Orphans::word`] で pin する。
    #[test]
    fn confine_orphans_counts_before_release_reads_the_scope_procs() {
        let root = std::env::temp_dir().join(format!("confine-orphans-root-{}", std::process::id()));
        let dir = root.join("user.slice").join("x.scope");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cgroup.procs"), "4242\n4243\n").unwrap();
        // 止める前（`ControlGroup` の絶対形でも root を捨てない・[`peak_of`] と同じつなぎ方）。
        assert_eq!(orphans_of(&root, "/user.slice/x.scope"), Orphans::Count(2), "残った 2 本を数える");
        assert_eq!(orphans_of(&root, "user.slice/x.scope"), Orphans::Count(2), "相対形も同じ");
        // 止めた後は dir ごと消える＝**後から数える形では 2 本が残らない**。
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            orphans_of(&root, "/user.slice/x.scope"),
            Orphans::Unreadable,
            "片付けの後に数えると殺した数は読めない"
        );
        // 読みの 3 通り（pure）。**空の file は読めた 0** で、読めない周と融合しない（C10）。
        assert_eq!(orphans_from(Ok("101\n102\n103\n".to_owned())), Orphans::Count(3), "空でない行を数える");
        assert_eq!(orphans_from(Ok("\n\n".to_owned())), Orphans::Count(0), "空行は pid ではない");
        assert_eq!(orphans_from(Ok(String::new())), Orphans::Count(0), "空の file は読めた 0");
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "no cgroup.procs");
        assert_eq!(orphans_from(Err(missing)), Orphans::Unreadable, "読めない周");
        assert_eq!(Orphans::Count(0).word(), "0", "読めた 0 は 0 と書く");
        assert_eq!(Orphans::Unreadable.word(), "-", "読めない周は -");
        assert_eq!(Orphans::Count(2).word(), "2");
    }

    /// scope の引数に `--collect` が**ちょうど 1 回**在り、既存の引数の順序と箱の大きさは不変
    /// （設計 gate-cost.md §25・`s2-07l.421`）。
    #[test]
    fn confine_collect_scope_args_carry_collect_once_in_order() {
        let args = scope_args("scribe2-probe-unit", 21, &CAPS);
        assert_eq!(
            args.iter().filter(|arg| *arg == "--collect").count(),
            1,
            "--collect は 1 回: {args:?}"
        );
        assert_eq!(
            args,
            vec![
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                "--unit=scribe2-probe-unit",
                "-p",
                "MemoryMax=21M",
                "-p",
                "CPUWeight=50",
                "-p",
                "OOMPolicy=continue",
            ],
            "既存の引数の順序と箱の大きさは不変"
        );
    }

    /// 包めなかった周は片付けを撃たない（record も変わらない）。
    #[test]
    fn confine_release_scope_skips_the_unconfined() {
        assert_eq!(release_scope(&Confinement::Unconfined(Reason::NoTool)), None);
    }

    /// `memory.peak` の読み（設計 §13）: 10 進の値は `Bytes`・空 / parse 不能 / `Err` は `Unreadable`（0 に
    /// 融合しない・C10）。`word` は `Bytes` を 10 進の字面・`Unreadable` を `-` に写す。
    #[test]
    fn confine_peak_from_reads_bytes_and_marks_the_rest_unreadable() {
        assert_eq!(peak_from(Ok("5006000000\n".to_owned())), Peak::Bytes(5_006_000_000));
        assert_eq!(peak_from(Ok(String::new())), Peak::Unreadable, "空");
        assert_eq!(peak_from(Ok("abc".to_owned())), Peak::Unreadable, "parse 不能");
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "no memory.peak");
        assert_eq!(peak_from(Err(missing)), Peak::Unreadable, "読めない");
        assert_eq!(Peak::Bytes(5_006_000_000).word(), "5006000000");
        assert_eq!(Peak::Unreadable.word(), "-");
    }

    /// `ControlGroup` は `/` で始まる絶対形で来る。root の下につなぐときに root を**捨てない**
    /// （`Path::join` に絶対 path を渡すと root が消える）＝tmp の root + `/user.slice/x.scope` で file を読める。
    #[test]
    fn confine_peak_of_keeps_the_root_when_control_group_starts_with_slash() {
        let root = std::env::temp_dir().join(format!("confine-peak-root-{}", std::process::id()));
        let dir = root.join("user.slice").join("x.scope");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("memory.peak"), "4096\n").unwrap();
        assert_eq!(peak_of(&root, "/user.slice/x.scope"), Peak::Bytes(4096), "root の下から読む");
        assert_eq!(peak_of(&root, "user.slice/x.scope"), Peak::Bytes(4096), "相対形も同じ");
        assert_eq!(peak_of(&root, "/user.slice/y.scope"), Peak::Unreadable, "無い dir は読めない");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 畳み込み: 読めた値は置き換え・読めない周は直前を保つ（最後に読めた値が peak）。全部読めなければ
    /// `Unreadable` のまま。
    #[test]
    fn confine_peak_absorb_keeps_the_last_readable_sample() {
        let samples = [Peak::Bytes(1), Peak::Unreadable, Peak::Bytes(5), Peak::Unreadable];
        let folded = samples.iter().fold(Peak::Unreadable, |kept, next| kept.absorb(*next));
        assert_eq!(folded, Peak::Bytes(5), "最後に読めた値");
        let none = [Peak::Unreadable, Peak::Unreadable].iter().fold(Peak::Unreadable, |kept, next| kept.absorb(*next));
        assert_eq!(none, Peak::Unreadable, "1 度も読めない");
        assert_eq!(Peak::Bytes(3).absorb(Peak::Bytes(2)), Peak::Bytes(2), "読めた値は後の周が勝つ");
    }

    /// `systemctl show … -p ControlGroup --value` の読み: rc 0 + path は `Some`（末尾の改行を落とす）・rc 1 /
    /// 空 / 起動不能は `None`。
    #[test]
    fn confine_peak_control_group_from_reads_the_show_output() {
        let out = |raw: i32, stdout: &str| {
            Ok(Output { status: ExitStatus::from_raw(raw), stdout: stdout.as_bytes().to_vec(), stderr: Vec::new() })
        };
        assert_eq!(control_group_from(out(0, "/user.slice/x.scope\n")), Some("/user.slice/x.scope".to_owned()));
        assert_eq!(control_group_from(out(1 << 8, "/user.slice/x.scope\n")), None, "rc 1");
        assert_eq!(control_group_from(out(0, "\n")), None, "空");
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "no systemctl");
        assert_eq!(control_group_from(Err(missing)), None, "起動できない");
    }

    /// 包めなかった周の [`Sampler`] は `show` も read も撃たず `Unreadable` のまま（行に語が出ない側の材料）。
    #[test]
    fn confine_peak_sampler_stays_unreadable_when_unconfined() {
        let confinement = Confinement::Unconfined(Reason::NoTool);
        let mut sampler = Sampler::of(&confinement, Path::new(CGROUP_ROOT));
        sampler.sample();
        assert_eq!(sampler.peak(), Peak::Unreadable);
    }

    /// `wrap_line` が組む起動は `TMUX_PANE` を**外す**指定を持ち、ほかの env を足さない。
    ///
    /// runner（spawn）と lens / verify 行（gate）は同じこの関数を通る——e2e の歯が runner と
    /// lens の 2 経路を撃ち、ここは関数そのものの指定を pin する。
    #[test]
    fn pipe_spawn_drops_tmux_pane_in_wrap_line() {
        let entry = Wrap { unit: "scribe2-probe-unit", limit: Limit::HostReserve, caps: Err(RuleRead::Missing) };
        let (cmd, confinement) = wrap_line("true", &entry);
        assert_eq!(confinement.reason(), Some(Reason::NoRules), "rules の無い周は素のまま撃つ");
        let envs: Vec<(&OsStr, Option<&OsStr>)> = cmd.get_envs().collect();
        assert_eq!(
            envs,
            vec![(OsStr::new(PANE_ENV), None)],
            "外すのは TMUX_PANE だけで、足す env は無い"
        );
        assert_eq!(PANE_ENV, "TMUX_PANE", "外す名は tmux の pane の変数");
    }

    /// `wrap_command`（argv の包み・headless の `build` が通る口）も `TMUX_PANE` を**外す**指定を持ち、
    /// ほかの env を足さない（`wrap_line` と同じ 1 点・設計 seat-roles.md §4 行 c）。base は空で RED。
    #[test]
    fn pipe_spawn_drops_tmux_pane_in_wrap_command() {
        let entry = Wrap { unit: "scribe2-probe-unit", limit: Limit::HostReserve, caps: Err(RuleRead::Missing) };
        let (cmd, _) = wrap_command(Command::new("true"), &entry);
        let envs: Vec<(&OsStr, Option<&OsStr>)> = cmd.get_envs().collect();
        assert_eq!(envs, vec![(OsStr::new(PANE_ENV), None)], "外すのは TMUX_PANE だけで、足す env は無い");
    }

    /// 歯の fixture の 3 線（tracked manifest の値を写さない＝値が動いても歯は動かない）。
    const CAPS: Caps = Caps {
        job_memory_mb: 7,
        reserve_memory_mb: 11,
        cpu_weight: 50,
    };

    /// `/proc/meminfo` の頭（MemTotal は 64 MiB ちょうど）。
    ///
    /// **2 つの式が同じ数を出す fixture を使わない**（job の箱 21 / host の箱 53）——同じ数だと
    /// 箱の種を取り違えた実装が両方の assert を通る（fixture 衝突）。
    const MEMINFO: &str = "MemTotal:          65536 kB\nMemFree:           16384 kB\n";

    /// [`REASONS`] の並びが宣言順である（ADR-0013 §2.2）。
    #[test]
    fn confine_reasons_follow_declaration_order() {
        assert!(
            is_declaration_order(REASONS, |reason| reason as usize),
            "REASONS の並びが宣言順と乖離している（母集団 {} 種）",
            REASONS.len()
        );
        // 字面は record の語彙である（重複すると 2 つの理由が 1 つに読める）。
        for (at, reason) in REASONS.iter().enumerate() {
            let same = REASONS
                .iter()
                .filter(|other| other.as_str() == reason.as_str())
                .count();
            assert_eq!(same, 1, "{at} 番目の字面 {} が重複する", reason.as_str());
        }
    }

    /// 上限の種は**行の穴だけ**で決まる（設計 §4.2）。
    ///
    /// **両方向を 1 本で撃つ**——片側だけを見る歯は `limit_of` を常に `PerJob` にする変異
    /// （`|| true`）でも常に `HostReserve` にする変異（`&& false`）でも生き残る。
    #[test]
    fn confine_limit_of_takes_the_box_from_the_hole_in_the_line() {
        assert_eq!(
            limit_of("cargo xtask mutants-diff --base {base} --jobs {jobs}", 4),
            Limit::PerJob(4),
            "{{jobs}} を持つ行は job の箱"
        );
        assert_eq!(
            limit_of("cargo nextest run --workspace --no-tests=fail", 4),
            Limit::HostReserve,
            "{{jobs}} を持たない行は host の箱"
        );
        // 穴でない字面（`jobs` という語）は箱を変えない。
        assert_eq!(
            limit_of("cargo xtask jobs --base {base}", 4),
            Limit::HostReserve,
            "穴でない jobs の語は host の箱のまま"
        );
    }

    /// 箱の大きさは 2 種で、**互いに違う式**である（片方の式に潰す変異はここで落ちる）。
    #[test]
    fn confine_limit_mb_uses_two_distinct_formulas() {
        assert_eq!(
            limit_mb(Limit::PerJob(3), &CAPS, MEMINFO),
            Some(21),
            "job の箱 = jobs × job_memory_mb（meminfo を見ない）"
        );
        assert_eq!(
            limit_mb(Limit::HostReserve, &CAPS, MEMINFO),
            Some(53),
            "host の箱 = MemTotal − reserve（同じ meminfo で job の箱と別の数）"
        );
        assert_eq!(limit_mb(Limit::PerJob(1), &CAPS, MEMINFO), Some(7), "job の箱は jobs で動く");
        assert_eq!(
            limit_mb(Limit::HostReserve, &CAPS, "MemTotal:  32768 kB\n"),
            Some(21),
            "host の箱は MemTotal で動く（jobs では動かない）"
        );
        // 残らない / 読めない周は `None`＝0 の箱を作らない（作れば全部 OOM で死ぬ）。
        assert_eq!(
            limit_mb(Limit::HostReserve, &CAPS, "MemTotal:  11264 kB\n"),
            None,
            "予約を引いて残らない host は包まない"
        );
        assert_eq!(limit_mb(Limit::HostReserve, &CAPS, "こわれ\n"), None, "読めない meminfo");
        assert_eq!(limit_mb(Limit::PerJob(0), &CAPS, MEMINFO), None, "jobs 0 の箱は作らない");
    }

    /// `MemTotal` は kB → MiB へ畳む（他の行に釣られない）。
    #[test]
    fn confine_mem_total_reads_only_its_own_line() {
        assert_eq!(mem_total_mb(MEMINFO), Some(64), "65536 kB は 64 MiB");
        assert_eq!(mem_total_mb("MemFree: 999999 kB\n"), None, "MemTotal が無い");
        assert_eq!(mem_total_mb("MemTotal:  x kB\n"), None, "数でない値は読めない");
        assert_eq!(mem_total_mb(""), None, "空の meminfo");
    }

    /// 終端行の parser（設計 §4.3）。**無い周を 0 に化けさせない**。
    #[test]
    fn confine_read_usage_strips_the_trailing_line() {
        let found = read_usage("noise\nconfine-usage peak_bytes=3145728 oom_kill=0\n");
        assert_eq!(found.peak_mb, Some(3), "3 MiB");
        assert_eq!(found.oom_kill, Some(0), "殺されていない");

        let killed = read_usage("confine-usage peak_bytes=2097152 oom_kill=2\n");
        assert_eq!(killed.oom_kill, Some(2), "殺された数をそのまま読む");

        // 終端行が無い周は **`peak_mb` も `oom_kill` も不明**（0 ではない）。
        let none = read_usage("ふつうの出力\n{\"verdict\":\"PASS\"}\n");
        assert_eq!(none.peak_mb, None, "終端行が無い周は不明");
        assert_eq!(none.oom_kill, None, "殺された証拠も殺されていない証拠も無い");

        // `memory.peak` の無い kernel は `-` を出す（0 と書かない）。
        let dash = read_usage("confine-usage peak_bytes=- oom_kill=0\n");
        assert_eq!(dash.peak_mb, None, "- は不明");
        assert_eq!(dash.oom_kill, Some(0), "oom_kill は読める");
        let unread = read_usage("confine-usage peak_bytes=1048576 oom_kill=-\n");
        assert_eq!(unread.oom_kill, None, "`memory.events` の読めない周は不明（0 ではない）");

        // **最後の 1 行が勝つ**（同じ行が 2 度出た周は後の周の数である）。
        let twice = read_usage(
            "confine-usage peak_bytes=1048576 oom_kill=0\nconfine-usage peak_bytes=4194304 oom_kill=1\n",
        );
        assert_eq!(twice.peak_mb, Some(4), "後の行を読む");
        assert_eq!(twice.oom_kill, Some(1), "後の行を読む");
    }

    /// epilogue は **rc を返し**、**自分の scope の中でだけ**終端行を出す（設計 §4.3）。
    #[test]
    fn confine_script_keeps_the_exit_code_and_pins_its_own_scope() {
        let body = script("cargo xtask check", "scribe2-probe-unit");
        assert!(body.starts_with("cargo xtask check\n__rc=$?\n"), "行の rc を先に退避する: {body}");
        assert!(body.trim_end().ends_with("exit $__rc"), "退避した rc を返す: {body}");
        assert!(
            body.contains("*/scribe2-probe-unit.scope)"),
            "自分の unit の scope だけを自分の箱と認める: {body}"
        );
        assert!(body.contains("confine-usage peak_bytes=%s oom_kill=%s"), "固定形: {body}");
        assert!(!body.contains("\n{"), "終端行は `{{` で始めない（last_json_object と衝突する）: {body}");
    }

    /// 包めなかった理由と、箱の中で起きたことは**同じ 1 つの語彙**で読む。
    #[test]
    fn confine_reason_names_are_kebab_and_closed() {
        assert_eq!(Reason::NoTool.as_str(), "no-systemd-run");
        assert_eq!(Reason::OomKill.as_str(), "oom-kill");
        assert_eq!(Reason::Signal.as_str(), "signal");
        assert_eq!(Reason::ManifestUnreadable.as_str(), "manifest-unreadable");
        assert_eq!(Reason::Unknown.as_str(), "unknown");
        assert_eq!(REASONS.len(), 8, "母集団（包めない 5 つ + 箱の中の 3 つ）");
    }

    /// parse に失敗する text からの読みは `Caps::of` が [`RuleRead::ManifestUnreadable`] のまま運び、包みは
    /// 「包まない」のまま record の理由を `manifest-unreadable` にする（`s2-07l.205`）。行が欠ける周は従来
    /// どおり `no-rules`（2 つを 1 語に潰す変異はここで落ちる）。
    #[test]
    fn rule_read_confine_unreadable_manifest_is_named_in_the_reason() {
        let broken = Caps::of(manifest_read(Manifest::parse("schema = 1\n\n[[rule]]\nid = \"x\"\n")));
        assert_eq!(broken, Err(RuleRead::ManifestUnreadable), "parse の Err は 1 語に畳む");
        let entry = Wrap { unit: "scribe2-probe-unit", limit: Limit::HostReserve, caps: broken };
        let (cmd, confinement) = wrap_line("true", &entry);
        assert!(!confinement.confined(), "読めない周は包まない（止めない）");
        assert_eq!(confinement.reason(), Some(Reason::ManifestUnreadable), "理由は manifest-unreadable");
        assert_eq!(cmd.get_program(), OsStr::new("sh"), "素の sh -c のまま");
        let (_, wrapped) = wrap_command(Command::new("true"), &entry);
        assert_eq!(wrapped.reason(), Some(Reason::ManifestUnreadable), "argv の包みも同じ理由");

        // 行が欠ける（読めた manifest に 3 線が無い）周は `no-rules` のまま。
        let absent = Caps::of(manifest_read(Manifest::parse("schema = 1\n")));
        assert_eq!(absent, Err(RuleRead::Missing), "行が無い");
        let entry = Wrap { unit: "scribe2-probe-unit", limit: Limit::HostReserve, caps: absent };
        assert_eq!(wrap_line("true", &entry).1.reason(), Some(Reason::NoRules), "行が無い周は no-rules");
        for read in [RuleRead::Disabled, RuleRead::NotInt, RuleRead::NotStr, RuleRead::NotInTable] {
            let entry = Wrap { unit: "scribe2-probe-unit", limit: Limit::HostReserve, caps: Err(read) };
            assert_eq!(wrap_line("true", &entry).1.reason(), Some(Reason::NoRules), "{read:?} は no-rules");
        }
    }

    /// 3 線の揃った manifest は `Caps` になり、1 行でも欠ければその行の variant で Err（3 線の全部を読む）。
    #[test]
    fn rule_read_confine_caps_of_reads_all_three_rows() {
        let row = |id: &str, kind: &str, value: u64| {
            format!(
                "[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n\n"
            )
        };
        let job = row("gate.job_memory_mb", "GateJobMemoryMb", 7);
        let reserve = row("host.reserve_memory_mb", "HostReserveMemoryMb", 11);
        let weight = row("gate.cpu_weight", "GateCpuWeight", 50);
        let full = format!("schema = 1\n\n{job}{reserve}{weight}");
        assert_eq!(Caps::of(manifest_read(Manifest::parse(&full))), Ok(CAPS), "3 線が揃う");
        let short = format!("schema = 1\n\n{job}{reserve}");
        assert_eq!(Caps::of(manifest_read(Manifest::parse(&short))), Err(RuleRead::Missing), "cpu_weight が無い");
    }
}
