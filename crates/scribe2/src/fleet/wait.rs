//! 完了待ち（[`wait`]）——**唯一の待機実装**（設計 pipeline-conflict.md §3・憲法 C3.4）。
//!
//! 待つ対象は [`Completion`] のデータだけで、満たされたと判じる読み手はこの file の内側が持つ
//! （`s2-07l.260` で挙動不変に分割・外の呼び手の path は `fleet` の再 export で保つ）。

use super::{cli, replay, select, select_for_run, store, Stage};
use std::time::{Duration, Instant};

/// 待つ対象。**述語を受ける口は作らない**（C3.4: 待機は 1 実装）。
///
/// variant が運ぶのは**データ**だけである。何を読んで満たされたと判じるかは [`wait`] の
/// 内側が持つ（受付の枠なら meminfo と札の読み手・設計 gate-cost.md §3.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    /// runner の process が終わること。
    RunnerExited(u32),
    /// 席の process が消えること（TERM の後）。
    SeatGone(u32),
    /// host の受付に枠が 1 つ以上空くこと（[`crate::pipe::admission`]）。
    SlotFree {
        /// 受付札の置き場（`<state_dir の親>/<NAME>-host/slots/`）。
        slots_dir: std::path::PathBuf,
        /// この受付が要る枠（jobs の数）。
        want: u64,
        /// job 1 つが要る memory（MiB・rules 行 `gate.job_memory_mb`）。
        job_mb: u64,
        /// 席と host のために残す memory（MiB・rules 行 `host.reserve_memory_mb`）。
        reserve_mb: u64,
        /// 並列度の上限（rules 行 `gate.mutants_jobs`）。
        cap: u64,
    },
    /// process group の全員が消えること（group 宛ての TERM / KILL の後・値は group id）。
    GroupGone(u32),
    /// land の番が来ること（[`crate::pipe::land`]・設計 gate-cost.md §6）: 同じ置き場の着地待ちの列で
    /// 自分より前の便が居なくなる。列を導けない周も満たされた側である（待たずに進む・記録は land が残す）。
    LandTurn {
        /// event log の置き場（列は replay から導く・別の状態 file を持たない）。
        state_dir: std::path::PathBuf,
        /// 待つ便の id。
        run: String,
    },
    /// 便用の口座が 1 つ空くこと（設計 account-autonomy.md §4・ADR-0020 §2.3）: 置き場の最新の実測行で
    /// §3 の便用の規則を再評価して `Chosen` になる。deadline は呼び手が `reset_at` から計算する
    /// （rules 行ではない・縮退を持たない）。
    AccountFree {
        /// 当たっている口座が開き直る最も早い時刻（`YYYY-MM-DDTHH:MM:SSZ`・deadline の出所）。
        reset_at: String,
        /// 実測行の置き場（`SlotFree` が `slots_dir` を運ぶのと同型）。
        state_dir: std::path::PathBuf,
        /// 待つ便の id。**便が `RateLimited` でなくなった周は満たされた側**（`pipe stop --run` が待ちの
        /// 途中の便を終端した周に待ちから抜ける・呼び手が段を読み直す）。
        run: String,
        /// manifest の `[[account]]` の label 列（宣言値・置き場は持たないので運ぶ）。
        labels: Vec<String>,
    },
}

impl Completion {
    /// 見張る pid。**pid を見張らない variant（[`Self::SlotFree`] / [`Self::LandTurn`] /
    /// [`Self::AccountFree`]）は 0**——pid 0 は `/proc/0` を持たない（user の process に振られない）ので、
    /// 生きている pid と取り違えない。[`Self::GroupGone`] は group id（= group leader の pid）を返す。
    pub fn pid(&self) -> u32 {
        match *self {
            Self::RunnerExited(pid) | Self::SeatGone(pid) | Self::GroupGone(pid) => pid,
            Self::SlotFree { .. } | Self::LandTurn { .. } | Self::AccountFree { .. } => 0,
        }
    }

    /// 満たされたか（1 周分の観測）。
    fn is_met(&self) -> bool {
        match self {
            Self::RunnerExited(pid) | Self::SeatGone(pid) => !pid_is_live(*pid),
            Self::GroupGone(group) => !group_is_live(*group),
            Self::SlotFree { slots_dir, want, job_mb, reserve_mb, cap } => {
                crate::pipe::admission::has_room(
                    slots_dir,
                    (*want).min(*cap),
                    crate::pipe::admission::Sizes { job_mb: *job_mb, reserve_mb: *reserve_mb },
                )
            }
            Self::LandTurn { state_dir, run } => !matches!(
                crate::pipe::land::turn_now(state_dir, run),
                crate::pipe::land::Turn::After(_)
            ),
            Self::AccountFree { state_dir, run, labels, .. } => account_free(state_dir, run, labels),
        }
    }
}

/// [`Completion::AccountFree`] の 1 周分の観測。
///
/// 置き場を replay し、便がまだ `RateLimited` なら最新の実測行で便用の規則（[`select_for_run`]）を
/// 再評価して `Chosen` の周だけ満たされる。便が `RateLimited` でなくなった周（stop で終端した・別の
/// process が起こし直した）は**満たされた側**＝待ち続ける理由が無い。置き場を読めない周は満たされない
/// （読めなさで起こし直さない・期限で Timeout に倒れて計測から撃ち直す）。
fn account_free(state_dir: &std::path::Path, run: &str, labels: &[String]) -> bool {
    let Ok(events) = store::read_all(state_dir) else {
        return false;
    };
    let state = replay(&events);
    if state.runs.get(run).map(|found| found.stage) != Some(Stage::RateLimited) {
        return true;
    }
    matches!(
        select_for_run(&state, labels, &cli::now_utc()),
        select::Selection::Chosen(_)
    )
}

/// `YYYY-MM-DDTHH:MM:SSZ` を UNIX 秒にする（[`cli::format_utc`] の逆・**それ以外の形は `None`**）。
///
/// 待ちの deadline を reset 時刻から計算する読み手である。数の読み替えを持たない（形が違う字面を
/// 0 秒にしない＝呼び手は `None` を「待つ時刻が無い」と読む）。
pub fn epoch_of(ts: &str) -> Option<u64> {
    let shape = b"0000-00-00T00:00:00Z";
    let bytes = ts.as_bytes();
    let fits = bytes.len() == shape.len()
        && bytes.iter().zip(shape.iter()).all(|(found, want)| match want {
            b'0' => found.is_ascii_digit(),
            _ => found == want,
        });
    if !fits {
        return None;
    }
    let num = |from: usize, len: usize| ts.get(from..from + len)?.parse::<u64>().ok();
    let (year, month, day) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (hour, minute, second) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    // 暦の (年, 月, 日) を 1970-01-01 からの日数にする（`cli::civil_from_days` の逆・chrono を足さない）。
    let shifted_year = if month <= 2 { year.checked_sub(1)? } else { year };
    let era = shifted_year / 400;
    let yoe = shifted_year - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = (era * 146_097 + doe).checked_sub(719_468)?;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// 期限までに終わらなかった。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeout;

/// 待つ間隔。
const POLL: Duration = Duration::from_millis(20);

/// [`Completion`] が満たされるまで待つ。**これが唯一の待機実装である**。
///
/// process の生存は `/proc/<pid>` の有無で見る（libc を足さないため・NFR3）。受付の枠は
/// 周ごとに meminfo と札を読み直す（周期はこの [`POLL`] のまま・上限は呼び手の期限）。
pub fn wait(completion: Completion, deadline: Duration) -> Result<(), Timeout> {
    let started = Instant::now();
    loop {
        if completion.is_met() {
            return Ok(());
        }
        if started.elapsed() >= deadline {
            return Err(Timeout);
        }
        std::thread::sleep(POLL);
    }
}

/// pid が生きているか。
fn pid_is_live(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// pgid が `group` の process が `/proc` に 1 つでも在るか（zombie も数える＝回収されるまで在る）。
///
/// **`/proc` を読めない周は「在る」**（消えたと測れていないものを消えたにしない・fail-closed）。
/// 読む間に消えた process の stat は読めないので飛ばす。
fn group_is_live(group: u32) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return true;
    };
    entries.flatten().any(|entry| {
        let numeric = entry.file_name().to_str().is_some_and(|name| name.bytes().all(|b| b.is_ascii_digit()));
        numeric
            && std::fs::read_to_string(entry.path().join("stat"))
                .ok()
                .and_then(|text| pgid_of(&text))
                == Some(group)
    })
}

/// `/proc/<pid>/stat` の 1 行から pgid（第 5 欄）を読む（pure）。
///
/// `comm` は空白も `)` も含みうるので、**最後の `)`** の後ろから数える（state・ppid・pgrp の順）。
/// 欄が足りない行・数でない欄は `None`。
fn pgid_of(stat_text: &str) -> Option<u32> {
    let (_, rest) = stat_text.rsplit_once(')')?;
    rest.split_whitespace().nth(2)?.parse().ok()
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.260
    use super::{cli::format_utc, epoch_of, pgid_of, Completion};

    /// `epoch_of` は `format_utc` の逆で、形の違う字面は `None`（0 秒に読み替えない）。
    #[test]
    fn pipe_ratelimit_resume_epoch_of_inverts_format_utc() {
        for secs in [0_u64, 951_782_400, 1_789_000_000, 4_102_444_800, 1_709_251_199] {
            let text = format_utc(secs);
            assert_eq!(epoch_of(&text), Some(secs), "{text}");
        }
        assert_eq!(epoch_of("2026-09-13T06:00:00Z"), Some(1_789_279_200));
        for bad in [
            "2026-09-13T06:00:00",
            "2026-09-13T06:00:00+00:00",
            "2026-13-13T06:00:00Z",
            "2026-09-13T24:00:00Z",
            "2026-09-13 06:00:00Z",
            "",
            "-",
        ] {
            assert_eq!(epoch_of(bad), None, "{bad:?}");
        }
    }

    /// `AccountFree` は pid を見張らない（0）。
    #[test]
    fn pipe_ratelimit_resume_account_free_watches_no_pid() {
        let found = Completion::AccountFree {
            reset_at: "2026-09-13T06:00:00Z".to_owned(),
            state_dir: std::path::PathBuf::from("state"),
            run: "r".to_owned(),
            labels: Vec::new(),
        };
        assert_eq!(found.pid(), 0);
    }

    #[test]
    fn pipe_stop_group_pgid_of_reads_the_fifth_field() {
        assert_eq!(pgid_of("4242 (sleep) S 4200 4100 4100 0 -1 4194560 91 0"), Some(4100), "通常の comm");
        assert_eq!(pgid_of("4242 (Web Content) S 4200 777 777 0 -1"), Some(777), "空白入り comm");
        assert_eq!(pgid_of("4242 (a) S 1 2 (b)) R 9 31 32 0"), Some(31), "`)` 入り comm は最後の `)` から数える");
        assert_eq!(pgid_of("4242 (sleep) S 4200"), None, "欄が足りない");
        assert_eq!(pgid_of("4242 (sleep) S 4200 x 1"), None, "数でない欄");
        assert_eq!(pgid_of("4242 sleep S 4200 4100"), None, "comm の閉じが無い");
    }

    #[test]
    fn pipe_stop_group_completion_pid_is_the_group_id() {
        assert_eq!(Completion::GroupGone(31337).pid(), 31337);
    }

    /// wait の網羅 match が新 variant を含む（variant を足したら compile で気付く形の歯）。
    #[test]
    fn pipe_stop_group_completion_match_is_exhaustive() {
        let all = [
            Completion::RunnerExited(7),
            Completion::SeatGone(8),
            Completion::SlotFree {
                slots_dir: std::path::PathBuf::from("slots"),
                want: 1,
                job_mb: 1,
                reserve_mb: 1,
                cap: 1,
            },
            Completion::GroupGone(9),
            Completion::LandTurn { state_dir: std::path::PathBuf::from("state"), run: "r".to_owned() },
            Completion::AccountFree {
                reset_at: "2026-09-13T06:00:00Z".to_owned(),
                state_dir: std::path::PathBuf::from("state"),
                run: "r".to_owned(),
                labels: Vec::new(),
            },
        ];
        let names: Vec<&str> = all
            .iter()
            .map(|found| match found {
                Completion::RunnerExited(_) => "RunnerExited",
                Completion::SeatGone(_) => "SeatGone",
                Completion::SlotFree { .. } => "SlotFree",
                Completion::GroupGone(_) => "GroupGone",
                Completion::LandTurn { .. } => "LandTurn",
                Completion::AccountFree { .. } => "AccountFree",
            })
            .collect();
        assert_eq!(
            names,
            ["RunnerExited", "SeatGone", "SlotFree", "GroupGone", "LandTurn", "AccountFree"],
            "宣言順の末尾に AccountFree"
        );
    }
}
