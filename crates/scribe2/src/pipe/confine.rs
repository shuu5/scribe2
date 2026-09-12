//! 器が起こす子 process を cgroup の transient scope で封じ込める（設計
//! docs/design/gate-cost.md §4・ADR-0021 §2.2）。
//!
//! **止めない、縮退する**（設計 §2）。`systemd-run` の無い host・scope を作れない host・
//! 箱を作れない host では包まずに素のまま撃ち、record に `confined=false reason=<閉じた
//! enum の名>` を残す。ゆえにこの境界は**行為を止めうる判定を返さない**——ADR-0014 §2.1 の
//! guard の定義に当たらないので、**極性一覧には載せない**（設計 §4.5・受付と同じ）。
//!
//! 現物と設計の差（unit 名の pid・probe・箱を作れない host・終端行の固定形）は設計 §4.4 の
//! errata に写してある。
//!
//! **値をこの file に焼かない**（憲法 C1 / C5）。箱の大きさと CPU の重みは [`Caps`] が
//! manifest の 3 行から読む。`MemoryHigh` は付けない（係数を持たない・設計 §4.2）。
//!
//! peak の読みは **scope の内側**で行う（設計 §4.3）。transient scope は最後の process の
//! 終了で cgroup dir ごと消えるので、外から終了後に読む形は成立しない。包みの `sh -c` が
//! 自分の `/proc/self/cgroup` から数を読み、stdout の終端に固定形 1 行で出す。器はその行を
//! [`read_usage`]（pure・in-file の歯が fixture 文字列で測る）で剥がす。

use crate::name::NAME;
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::process::{Command, Stdio};
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
/// **理由を自由文にしない**（憲法 C3.3・設計 §4.2）。包めなかった 4 つと、包んだ箱の中で
/// 起きた 2 つ（oom / signal）を 1 つの列挙で持つ——record の読み手はどちらも同じ `reason=`
/// で読むので、語彙が 2 面に割れると「外からの kill」と弁別できない（lens-132d L1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// `systemd-run` を起動できない（PATH に無い host）。
    NoTool,
    /// `systemd-run` は在るが scope を作れない（user の session manager が無い host）。
    NoScope,
    /// 封じ込めの rules 行を読めない。
    NoRules,
    /// 箱の大きさが残らない（`MemTotal − host.reserve_memory_mb` が 0 以下）。
    NoRoom,
    /// scope の中で kernel が process を殺した（`memory.events` の `oom_kill` ≥ 1）。
    OomKill,
    /// 包みごと signal で死んだ（oom の代理・設計 §4.3）。
    Signal,
}

/// [`Reason`] の全 variant（宣言順）。
pub const REASONS: &[Reason] = &[
    Reason::NoTool,
    Reason::NoScope,
    Reason::NoRules,
    Reason::NoRoom,
    Reason::OomKill,
    Reason::Signal,
];

impl Reason {
    /// record に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoTool => "no-systemd-run",
            Self::NoScope => "no-scope",
            Self::NoRules => "no-rules",
            Self::NoRoom => "no-room",
            Self::OomKill => "oom-kill",
            Self::Signal => "signal",
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
    /// それ以外（`{jobs}` を持たない verify 行・runner・lens）= `MemTotal − host.reserve_memory_mb`。
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
    /// 埋め込み manifest から読む。**1 行でも欠ければ `None`**＝包まない（止めない）。
    ///
    /// 読むのは埋め込みだけである——封じ込めの起動点は 3 つ（gate の verify 行・runner・
    /// claude）で、そのうち 2 つは `--rules` を受ける口を持たない。片方だけ override が
    /// 効く形にすると、同じ host の 3 つの箱が別々の値で走る。
    pub fn embedded() -> Option<Self> {
        let manifest = Manifest::embedded().ok()?;
        let int = |id: &str| match manifest.get(id).map(|row| &row.value) {
            Some(&RuleValue::Int(found)) => Some(found),
            _ => None,
        };
        Some(Self {
            job_memory_mb: int(JOB_MEMORY_ROW)?,
            reserve_memory_mb: int(RESERVE_ROW)?,
            cpu_weight: int(CPU_WEIGHT_ROW)?,
        })
    }
}

/// 1 起動を包む材料。
pub struct Wrap<'a> {
    /// transient scope の unit 名（[`unit_name`] が組む）。
    pub unit: &'a str,
    /// この起動に掛ける上限の種。
    pub limit: Limit,
    /// 封じ込めの 3 線（読めない周は `None`）。
    pub caps: Option<Caps>,
}

/// 便の 1 起動の unit 名（`<NAME>-<場所>-<段>-<n>-<pid>`）。
///
/// 設計 §4.2 は `<NAME>-<run>-<段>-<n>` を書くが、**同じ id を別 process が同時に測る周**
/// （歯の並列走行）で transient scope の名が衝突し、2 本目が起動できず偽の RED になる。
/// 場所（gate では便の worktree の dir 名 = run id）に pid を足して一意にする
/// （errata は設計 §4 に記した）。`<n>` は `verify.jsonl` の record 番号である。
pub fn unit_name(place: &str, stage: &str, n: usize) -> String {
    format!(
        "{NAME}-{}-{}-{n}-{}",
        tame(place),
        tame(stage),
        std::process::id()
    )
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
pub fn wrap_line(line: &str, entry: &Wrap<'_>) -> (Command, Confinement) {
    match fitting(entry) {
        Err(reason) => (shell(line), Confinement::Unconfined(reason)),
        Ok((caps, mb)) => (
            scope(shell(&script(line, entry.unit)), mb, entry, &caps),
            Confinement::Confined { unit: entry.unit.to_owned() },
        ),
    }
}

/// 既に組んだ起動を scope で包む。**終端行は足さない**（argv の起動に epilogue は書けない
/// ＝peak は測らない・設計 §4.3 の記録先は verify 行と runner の 2 面である）。
///
/// cwd・env・stdio は `Command` から読み戻せないので、**包んだ後に**外側へ付けること。
pub fn wrap_command(cmd: Command, entry: &Wrap<'_>) -> (Command, Confinement) {
    match fitting(entry) {
        Err(reason) => (cmd, Confinement::Unconfined(reason)),
        Ok((caps, mb)) => (
            scope(cmd, mb, entry, &caps),
            Confinement::Confined { unit: entry.unit.to_owned() },
        ),
    }
}

/// 箱の大きさ（MiB）を決める。包めない周は閉じた理由を返す。
fn fitting(entry: &Wrap<'_>) -> Result<(Caps, u64), Reason> {
    let caps = entry.caps.ok_or(Reason::NoRules)?;
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
        match cmd.status() {
            Err(_) => Err(Reason::NoTool),
            Ok(status) if status.success() => Ok(()),
            Ok(_) => Err(Reason::NoScope),
        }
    })
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
fn scope_args(unit: &str, mb: u64, caps: &Caps) -> Vec<String> {
    vec![
        "--user".to_owned(),
        "--scope".to_owned(),
        "--quiet".to_owned(),
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
    /// scope の中で kernel が殺した数（読めない周は 0）。
    pub oom_kill: u64,
}

/// stdout の**最後の終端行**を剥がす（pure・in-file の歯が fixture 文字列で測る）。
///
/// 終端行が無い周（`memory.peak` の無い kernel・包みの外で撃った周・行が途中で `exit` した
/// 周）は既定＝`peak_mb` 不明・`oom_kill` 0 である。
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
        oom_kill: field("oom_kill=").unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::{limit_mb, limit_of, mem_total_mb, read_usage, script, Caps, Limit, Reason, REASONS};
    use crate::order::is_declaration_order;

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
        assert_eq!(found.oom_kill, 0, "殺されていない");

        let killed = read_usage("confine-usage peak_bytes=2097152 oom_kill=2\n");
        assert_eq!(killed.oom_kill, 2, "殺された数をそのまま読む");

        // 終端行が無い周は **`peak_mb` 不明**（0 ではない）。
        let none = read_usage("ふつうの出力\n{\"verdict\":\"PASS\"}\n");
        assert_eq!(none.peak_mb, None, "終端行が無い周は不明");
        assert_eq!(none.oom_kill, 0, "殺された証拠も無い");

        // `memory.peak` の無い kernel は `-` を出す（0 と書かない）。
        let dash = read_usage("confine-usage peak_bytes=- oom_kill=0\n");
        assert_eq!(dash.peak_mb, None, "- は不明");

        // **最後の 1 行が勝つ**（同じ行が 2 度出た周は後の周の数である）。
        let twice = read_usage(
            "confine-usage peak_bytes=1048576 oom_kill=0\nconfine-usage peak_bytes=4194304 oom_kill=1\n",
        );
        assert_eq!(twice.peak_mb, Some(4), "後の行を読む");
        assert_eq!(twice.oom_kill, 1, "後の行を読む");
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
        assert_eq!(REASONS.len(), 6, "母集団（包めない 4 つ + 箱の中の 2 つ）");
    }
}
