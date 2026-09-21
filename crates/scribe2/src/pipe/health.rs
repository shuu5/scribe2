//! 器の健康の遮断器（設計 docs/design/gate-cost.md §32・契約表の行 x）。
//!
//! verify の行を撃つ前に host の**走行可能**（`/proc/loadavg` の 4 番目の欄の分子）と**待ち**
//! （`/proc/stat` の `procs_blocked`）を読み、core あたりの倍率（rules 行 `host.runnable_per_core` /
//! `host.blocked_per_core`）× 実測の core 数を超えた周は空くまで待つ。待ちは完了 enum の
//! [`Completion::HostCalm`] で唯一の待機実装（[`fleet::wait`]）を通り、上限は受付と同じ
//! `gate.slot_wait_s` である（3 本目の値の線を足さない・§32 約束 4）。
//!
//! 上限を超えた周は**行を撃たない**（[`Passage::Closed`]）——判定は呼び手（gate の `decide`）が
//! INCONCLUSIVE へ倒す（FAIL で終端させない）。**測れない周は待たずに撃つ**（[`Mark::Unmeasured`] を
//! record に残す・0 に潰さない・C10）。ゆえにこの境界は in-loop / fail-open の guard である（[`POLARITY`]）。
//!
//! 判定（[`judge`]）と行動（[`act`]）は pure で、fixture 文字列で測る（host の面を外から差し替える口は
//! 作らない・C2.2）。host の面を読む口は [`now`] の 1 本である。

use crate::fleet::{self, Completion};
use crate::polarity::{OnFailure, Polarity, Timing};
use std::time::Duration;

/// 走行可能の数を運ぶ面（4 番目の欄 `<走行可能>/<総数>` の分子を読む）。
const LOADAVG: &str = "/proc/loadavg";

/// 待ちの数を運ぶ面（`procs_blocked <n>` の行を読む）。
const STAT: &str = "/proc/stat";

/// `/proc/stat` の待ちの行の見出し（**語の完全一致**で引く＝似た見出しの行に釣られない）。
const BLOCKED_KEY: &str = "procs_blocked";

/// 遮断器の極性: 行を撃つ時点で止め、測れない周は撃つ側へ倒して record に残す（§32 約束 7 / 8）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailOpen,
};

/// core あたりの倍率 2 本（rules 行の値・**閾値 = 倍率 × 実測の core 数**・§32 約束 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerCore {
    /// 走行可能の倍率（rules 行 `host.runnable_per_core`）。
    pub runnable: u64,
    /// 待ちの倍率（rules 行 `host.blocked_per_core`）。
    pub blocked: u64,
}

/// host の健康（**閉じた 3 値**・0 と「測れない」を混ぜない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// どちらの面も閾値以下（ちょうどの値を含む）。
    Calm,
    /// どちらかの面が閾値を超えた。
    Busy,
    /// どちらかの面か core 数を読めない・数でない。
    Unmeasured,
}

/// 字面 2 つと core 数から健康を判じる（pure・§32 約束 1）。
///
/// 走行可能は `loadavg` の 4 番目の欄の**分子**（分母の総 process 数は見ない）、待ちは `stat` の
/// `procs_blocked` の行の値。**閾値ちょうどは混んでいない**（`>` で比べる）。片方だけ超えた周も混んでいる。
pub fn judge(loadavg: &str, stat: &str, cores: Option<u64>, per_core: PerCore) -> Health {
    let (Some(runnable), Some(blocked), Some(cores)) = (runnable_of(loadavg), blocked_of(stat), cores) else {
        return Health::Unmeasured;
    };
    let over_runnable = runnable > per_core.runnable.saturating_mul(cores);
    let over_blocked = blocked > per_core.blocked.saturating_mul(cores);
    if over_runnable || over_blocked {
        Health::Busy
    } else {
        Health::Calm
    }
}

/// `loadavg` の 4 番目の欄 `<走行可能>/<総数>` の分子（無い・`/` が無い・数でない周は `None`）。
fn runnable_of(loadavg: &str) -> Option<u64> {
    let (running, _) = loadavg.split_whitespace().nth(3)?.split_once('/')?;
    running.parse().ok()
}

/// `stat` の `procs_blocked <n>` の値（見出しは語の完全一致・行が無い・数でない周は `None`）。
fn blocked_of(stat: &str) -> Option<u64> {
    stat.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        (words.next() == Some(BLOCKED_KEY)).then(|| words.next()?.parse().ok())?
    })
}

/// 健康から出す行動（撃つか待つか）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 行を撃つ。
    Fire,
    /// 空くまで待つ。
    Wait,
}

/// record の `host=` に残す印（**任意 field**・空いていた周は載らない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// 測れないまま撃った（待たずに進む・§32 約束 7）。
    Unmeasured,
    /// 待ちの上限を超えて**撃たなかった**（閉じた印・§32 約束 5）。
    Closed,
}

impl Mark {
    /// record の `host=` の字面（空でも 0 でもない固定の語）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unmeasured => "unmeasured",
            Self::Closed => "busy",
        }
    }
}

/// 3 値から行動と record の印を出す（pure・§32 約束 7）: **混んでいるだけが待つ**。空いている周と
/// 測れない周は撃ち、測れない周だけが印を持つ。
pub fn act(health: Health) -> (Action, Option<Mark>) {
    match health {
        Health::Calm => (Action::Fire, None),
        Health::Busy => (Action::Wait, None),
        Health::Unmeasured => (Action::Fire, Some(Mark::Unmeasured)),
    }
}

/// host の core 数（測定値・env は読まない）。読めない周は `None`。
fn host_cores() -> Option<u64> {
    std::thread::available_parallelism()
        .ok()
        .and_then(|found| u64::try_from(found.get()).ok())
}

/// host の面を今読んで判じる（**host を読む口はこの 1 本**・読めない面は空の字面＝測れない）。
pub fn now(per_core: PerCore) -> Health {
    let loadavg = std::fs::read_to_string(LOADAVG).unwrap_or_default();
    let stat = std::fs::read_to_string(STAT).unwrap_or_default();
    judge(&loadavg, &stat, host_cores(), per_core)
}

/// 遮断器の材料（倍率 2 本と待ちの上限・gate と land の主実測が同じ欄を埋める・§32 約束 9）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breaker {
    /// core あたりの倍率。
    pub per_core: PerCore,
    /// 空くのを待つ上限（秒・rules 行 `gate.slot_wait_s`）。
    pub wait_s: u64,
}

/// 行を撃ってよいか（[`pass`] の戻り）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passage {
    /// 撃つ（測れない周は印を持つ）。
    Fire(Option<Mark>),
    /// 待ちの上限を超えた＝撃たない。
    Closed,
}

/// 行を撃つ前に host の健康を通す（混んでいる周は [`Completion::HostCalm`] を上限まで待つ）。
pub fn pass(breaker: Breaker) -> Passage {
    let (action, mark) = act(now(breaker.per_core));
    if action == Action::Fire {
        return Passage::Fire(mark);
    }
    let completion = Completion::HostCalm {
        runnable_per_core: breaker.per_core.runnable,
        blocked_per_core: breaker.per_core.blocked,
    };
    match fleet::wait(completion, Duration::from_secs(breaker.wait_s)) {
        // 空いた直後の読みで印を決める（空いていた周は印なし・測れなくなった周は印を持つ）。
        Ok(()) => Passage::Fire(act(now(breaker.per_core)).1),
        Err(_) => Passage::Closed,
    }
}

/// 待ちの観測（[`Completion::HostCalm`] の 1 周・`fleet::wait` の内側が呼ぶ）: 撃つ側なら満たされた。
pub fn calm_now(per_core: PerCore) -> bool {
    act(now(per_core)).0 == Action::Fire
}

#[cfg(test)]
mod tests {
    use super::{act, judge, Action, Health, Mark, PerCore};

    /// 倍率（走行可能 4・待ち 1）と core 数 8 の fixture＝閾値は走行可能 32・待ち 8。
    const PER_CORE: PerCore = PerCore { runnable: 4, blocked: 1 };

    /// core 数の fixture。
    const CORES: Option<u64> = Some(8);

    /// 走行可能 `running`・総数 `total` の `loadavg` の字面。
    fn loadavg(running: u64, total: u64) -> String {
        format!("0.50 0.40 0.30 {running}/{total} 12345\n")
    }

    /// 待ち `blocked` の `stat` の字面（前後に他の行を置く）。
    fn stat(blocked: u64) -> String {
        format!("cpu  1 2 3 4\nprocs_running 3\nprocs_blocked {blocked}\nsoftirq 0\n")
    }

    /// (a) 走行可能は 4 番目の欄の**分子**から読む: 分母だけが閾値を超える形は空いている・分子が超える形は混んでいる。
    #[test]
    fn health_judge_reads_the_numerator_not_the_denominator() {
        assert_eq!(judge(&loadavg(3, 5000), &stat(0), CORES, PER_CORE), Health::Calm, "分母 5000 に釣られない");
        assert_eq!(judge(&loadavg(33, 40), &stat(0), CORES, PER_CORE), Health::Busy, "分子 33 > 32 は混んでいる");
    }

    /// (b) 待ちは `procs_blocked` の行から読む: 似た見出し（`procs_blocked_total`）を閾値超えで**先に**置き、本物を
    /// 閾値以下で後に置く＝接頭辞一致に釣られた実装は「混んでいる」へ倒れて落ちる。
    #[test]
    fn health_judge_reads_procs_blocked_by_the_exact_heading() {
        let text = "procs_blocked_total 999\nprocs_blocked 2\n";
        assert_eq!(judge(&loadavg(1, 100), text, CORES, PER_CORE), Health::Calm, "本物の行は 2（閾値 8 以下）");
        assert_eq!(judge(&loadavg(1, 100), &stat(9), CORES, PER_CORE), Health::Busy, "本物の行が 9 > 8 なら混んでいる");
    }

    /// (c) 閾値 = 倍率 × core 数で、**ちょうど**の値は混んでいない（等号を境界に置かない）・1 超えると混んでいる。
    #[test]
    fn health_judge_threshold_is_per_core_times_cores_and_exact_is_calm() {
        assert_eq!(judge(&loadavg(32, 100), &stat(8), CORES, PER_CORE), Health::Calm, "両方ちょうど");
        assert_eq!(judge(&loadavg(32, 100), &stat(8), Some(4), PER_CORE), Health::Busy, "core 4 なら閾値 16 / 4");
        assert_eq!(judge(&loadavg(16, 100), &stat(4), Some(4), PER_CORE), Health::Calm, "core 4 のちょうど");
        let zero = PerCore { runnable: 0, blocked: 0 };
        assert_eq!(judge(&loadavg(1, 100), &stat(0), CORES, zero), Health::Busy, "倍率 0 は走行可能 1 でも混んでいる");
    }

    /// (d) 片方だけが超えた 2 形はどちらも混んでいる。
    #[test]
    fn health_judge_either_side_over_is_busy() {
        assert_eq!(judge(&loadavg(33, 100), &stat(0), CORES, PER_CORE), Health::Busy, "走行可能だけ超えた");
        assert_eq!(judge(&loadavg(0, 100), &stat(9), CORES, PER_CORE), Health::Busy, "待ちだけ超えた");
    }

    /// (e) 空・数でない・行が無いの 3 形はどれも測れない（空いているに潰れない）。core 数を読めない周も同じ。
    #[test]
    fn health_judge_empty_non_number_and_missing_are_unmeasured() {
        let good = (loadavg(1, 100), stat(0));
        assert_eq!(judge(&good.0, &good.1, CORES, PER_CORE), Health::Calm, "対: 読める周は空いている");
        for (load, text, why) in [
            (String::new(), good.1.clone(), "loadavg が空"),
            (good.0.clone(), String::new(), "stat が空"),
            ("0.5 0.4 0.3 x/100 1\n".to_owned(), good.1.clone(), "分子が数でない"),
            ("0.5 0.4 0.3 12 1\n".to_owned(), good.1.clone(), "4 番目の欄に / が無い"),
            ("0.5 0.4 0.3\n".to_owned(), good.1.clone(), "4 番目の欄が無い"),
            (good.0.clone(), "procs_blocked x\n".to_owned(), "待ちが数でない"),
            (good.0.clone(), "procs_blocked\n".to_owned(), "待ちの値が無い"),
            (good.0.clone(), "procs_running 3\n".to_owned(), "待ちの行が無い"),
        ] {
            assert_eq!(judge(&load, &text, CORES, PER_CORE), Health::Unmeasured, "{why}");
        }
        assert_eq!(judge(&good.0, &good.1, None, PER_CORE), Health::Unmeasured, "core 数を読めない");
    }

    /// (f) 行動と record の字面: 混んでいるだけが待ち、空いている周と測れない周は撃つ。字面は測れない周だけに載り、
    /// 空でも 0 でもない（空いている周と字面だけが違う）。
    #[test]
    fn health_judge_action_waits_only_when_busy_and_marks_only_unmeasured() {
        assert_eq!(act(Health::Busy).0, Action::Wait, "混んでいる周は待つ");
        assert_eq!(act(Health::Calm), (Action::Fire, None), "空いている周は撃ち、印なし");
        let (action, mark) = act(Health::Unmeasured);
        assert_eq!(action, Action::Fire, "測れない周は待たずに撃つ");
        let shown = mark.map(Mark::as_str).unwrap_or_default();
        assert!(!shown.is_empty() && shown != "0", "測れない周の字面は空でも 0 でもない: {shown:?}");
        assert_eq!(shown, "unmeasured");
        assert_ne!(Mark::Closed.as_str(), shown, "閉じた印とは別の字面");
    }
}
