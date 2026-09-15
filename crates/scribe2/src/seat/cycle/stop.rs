//! 席の**停止**（退避後の終了の手の第 2 手・account-autonomy.md §5・`s2-07l.252`）: pane の shell の pid → 直下の子 →
//! process group へ TERM → 唯一の wait → KILL（[`terminate_group`]）。[`super`] から純移動（`s2-07l.319`）。**pid が 2 未満は
//! 撃たない**（`kill -- -1` / `-0` の字面をこの module から作れない・N1）。

use super::PROC;
use crate::fleet::{wait, Completion};
use std::path::Path;
use std::time::Duration;

/// target の pane の shell の pid（`list-panes -F '#{pane_pid}'`・[`crate::seat::pane_is_shell`] と同じ typed な metadata・
/// 端末描画の字面ではない＝C3.3 の外）。撃てない・pane が 1 つでない・数でない周は `None`（「取れない」の側＝撃たない）。
pub fn pane_pid(socket: Option<&str>, target: &str) -> Option<u32> {
    let out = crate::seat::tmux_stdout(socket, &["list-panes", "-t", target, "-F", "#{pane_pid}"])?;
    let mut pids = out.lines().map(str::trim).filter(|line| !line.is_empty());
    let first = pids.next()?.parse().ok()?;
    pids.next().is_none().then_some(first)
}

/// pane の shell（[`pane_pid`]）の**直下の子** 1 つ（`/proc/<pid>/stat` の ppid を走査・複数なら最も若い pid＝最大・
/// [`crate::fleet::wait`] の group の読みと同じ `/proc` の読み方）。子が無い・`/proc` を読めない周は `None`。
/// **`pane_pid` が 2 未満の周も `None`**（pid 0 / 1 の子を「pane の子」と読まない＝撃たない側・N1）。
pub fn foreground_child(pane_pid: u32) -> Option<u32> {
    child_of(Path::new(PROC), pane_pid)
}

/// [`foreground_child`] の本体（`proc` は `/proc` か歯の fixture・読むだけ）。読む間に消えた process の stat は飛ばす。
fn child_of(proc: &Path, pane_pid: u32) -> Option<u32> {
    if pane_pid < 2 {
        return None;
    }
    let entries = std::fs::read_dir(proc).ok()?;
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let pid = name.to_str().filter(|name| name.bytes().all(|b| b.is_ascii_digit()))?.parse::<u32>().ok()?;
            let ppid = stat_field(&std::fs::read_to_string(entry.path().join("stat")).ok()?, 1)?;
            (ppid == pane_pid).then_some(pid)
        })
        .max()
}

/// `/proc/<pid>/stat` の 1 行の、**最後の `)`** の後ろから数えた `index` 番目の欄（0 = state・1 = ppid・2 = pgid・pure）。
/// `comm` は空白も `)` も含みうるので前から数えない（[`crate::fleet::wait`] の pgid の読みと同じ）。欄が足りない・数でない周は `None`。
fn stat_field(stat_text: &str, index: usize) -> Option<u32> {
    let (_, rest) = stat_text.rsplit_once(')')?;
    rest.split_whitespace().nth(index)?.parse().ok()
}

/// 停止の結果（**閉じた 3 値**・憲法 C11・結果の enum であって Guard ではない＝極性一覧には載らない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// TERM で消えた。
    Gone,
    /// TERM では残り、KILL で消えた。
    Killed,
    /// KILL の後も猶予の内に消えない。
    Unstoppable,
}

/// [`Stopped`] の全 variant（宣言順）。
pub const STOPPEDS: &[Stopped] = &[Stopped::Gone, Stopped::Killed, Stopped::Unstoppable];

impl Stopped {
    /// 記録と表示に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gone => "gone",
            Self::Killed => "killed",
            Self::Unstoppable => "unstoppable",
        }
    }
}

/// `pid` の process group へ TERM → **唯一の wait**（[`Completion::GroupGone`]・猶予 `grace`）→ 残れば KILL → 同じ wait
/// （`pipe stop` / `fleet usage` の停止と同じ列・C3.4・新しい sleep loop を持たない）。group が無い（pgid ≠ pid）周は
/// pid 単体へ同じ列（[`Completion::SeatGone`]）。**pid が 2 未満の周は撃たない**（`None`＝「取れない」の側・
/// `kill -- -1` は user の全 process・`kill -- -0` は自分の group・宛先は [`StopTarget`] からしか作らない・N1）。
pub fn terminate_group(pid: u32, grace: Duration) -> Option<Stopped> {
    let target = StopTarget::of(Path::new(PROC), pid)?;
    signal(&target.arg(), "-TERM");
    if wait(target.completion(), grace).is_ok() {
        return Some(Stopped::Gone);
    }
    signal(&target.arg(), "-KILL");
    Some(if wait(target.completion(), grace).is_ok() { Stopped::Killed } else { Stopped::Unstoppable })
}

/// 撃つ宛先（2 値）。**pid ≥ 2 だけを持てる**（[`StopTarget::plan`] が唯一の作り口・`pipe/stop.rs` の `GroupId` と同じ guard）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopTarget {
    /// process group 宛て（pgid == pid・値は group id）。
    Group(u32),
    /// 単一 pid 宛て（group の leader でない pid）。
    Single(u32),
}

impl StopTarget {
    /// `/proc/<pid>/stat` の pgid を読んで決める（読めない周は単体宛て・pid が 2 未満は `None`）。
    fn of(proc: &Path, pid: u32) -> Option<Self> {
        let pgid = std::fs::read_to_string(proc.join(pid.to_string()).join("stat"))
            .ok()
            .and_then(|text| stat_field(&text, 2));
        Self::plan(pid, pgid)
    }

    /// pid と読めた pgid から宛先を決める（**pure**・pid < 2 は `None`）。
    fn plan(pid: u32, pgid: Option<u32>) -> Option<Self> {
        if pid < 2 {
            return None;
        }
        Some(if pgid == Some(pid) { Self::Group(pid) } else { Self::Single(pid) })
    }

    /// `kill` へ渡す宛先の字面（group は負の pid）。
    fn arg(self) -> String {
        match self {
            Self::Group(group) => format!("-{group}"),
            Self::Single(pid) => pid.to_string(),
        }
    }

    /// 待つ対象（唯一の wait 実装へ渡す data）。
    fn completion(self) -> Completion {
        match self {
            Self::Group(group) => Completion::GroupGone(group),
            Self::Single(pid) => Completion::SeatGone(pid),
        }
    }
}

/// `kill <name> -- <target>` を撃つ（std に kill は無い・結果は待ちの側で測る・`fleet usage` の停止と同じ）。
fn signal(target: &str, name: &str) {
    let _ = std::process::Command::new("kill")
        .arg(name)
        .arg("--")
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    // flip-check: retroactive s2-07l.344
    use super::{child_of, stat_field, terminate_group, StopTarget, Stopped, STOPPEDS};
    use crate::fleet::Completion;
    use crate::order::is_declaration_order;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    /// 歯の `/proc` fixture: `<root>/<name>/stat` を `(name, comm, ppid, pgid)` で置く（`stat` の無い dir も置ける）。
    fn proc_fixture(label: &str, rows: &[(&str, &str, u32, u32)], bare: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("seat-cycle-proc-{label}-{}", std::process::id()));
        for (name, comm, ppid, pgid) in rows {
            std::fs::create_dir_all(root.join(name)).ok();
            std::fs::write(root.join(name).join("stat"), format!("{name} ({comm}) S {ppid} {pgid} {pgid} 0 -1 4194560 0\n")).ok();
        }
        for name in bare {
            std::fs::create_dir_all(root.join(name)).ok();
        }
        root
    }

    /// 前面の子は pane の shell の**直下**の子で、複数なら最も若い pid（最大）。孫・別の親の子・数でない名・stat の無い
    /// dir は数えず、子の無い pid と `/proc` を読めない周は `None`。**pane の pid が 2 未満なら子が在っても `None`**
    /// （pid 1 の子を pane の子と読まない・N1）。欄は最後の `)` の後ろから数える（`)` 入りの comm）。
    #[test]
    fn cycle_stop_foreground_child_is_the_youngest_direct_child_of_the_pane() {
        let root = proc_fixture(
            "tree",
            &[
                ("100", "sh", 7, 100),
                ("200", "sleep", 100, 200),
                ("300", "Web Content", 100, 300),
                ("250", "cat", 200, 200),
                ("400", "x)y", 7, 400),
                ("600", "init-child", 1, 600),
            ],
            &["abc", "500"],
        );
        std::fs::write(root.join("abc").join("stat"), "abc (sleep) S 100 100 100\n").ok();
        assert_eq!(child_of(&root, 100), Some(300), "直下の子 200 / 300 のうち最も若い 300（孫 250 は数えない）");
        assert_eq!(child_of(&root, 200), Some(250), "孫は自分の親からは子");
        assert_eq!(child_of(&root, 7), Some(400), "`)` 入りの comm でも ppid が読める");
        assert_eq!(child_of(&root, 300), None, "子が無い");
        assert_eq!(child_of(&root, 1), None, "pid 1 の子（600）は pane の子と読まない");
        assert_eq!(child_of(&root, 0), None, "pid 0 も同じ");
        assert_eq!(child_of(&root.join("missing"), 100), None, "/proc を読めない");
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(stat_field("4242 (a) S 1 2 (b)) R 9 31 32 0", 1), Some(9), "最後の `)` の後ろの ppid");
        assert_eq!(stat_field("4242 (sleep) S 4200 4100 4100 0", 2), Some(4100), "pgid");
        assert_eq!(stat_field("4242 (sleep) S 4200", 2), None, "欄が足りない");
        assert_eq!(stat_field("4242 sleep S 4200 4100", 1), None, "comm の閉じが無い");
    }

    /// pane の pid の guard の境界（`s2-07l.344`・.319 の検出線の生存 `pane_pid < 2` → `<=` を潰す）: pid 1 は子が在っても
    /// 断り（`None`）、pid 2 は通す（直下の子を返す）。境界の両側を同じ fixture で測る。
    #[test]
    fn cycle_stop_refuses_pid_below_two() {
        let root = proc_fixture("boundary", &[("60", "init-child", 1, 60), ("50", "sh", 2, 50)], &[]);
        assert_eq!(child_of(&root, 1), None, "pid 1 は子（60）が在っても断る");
        assert_eq!(child_of(&root, 2), Some(50), "pid 2 は通す（直下の子 50）");
        std::fs::remove_dir_all(&root).ok();
    }

    /// 宛先は pure に決まる: pgid == pid は group 宛て（`-<pid>` / `GroupGone`）、それ以外と pgid が読めない周は単体宛て
    /// （`<pid>` / `SeatGone`）、**pid < 2 は `None`**（`kill -- -1` / `-0` の字面をこの型から作れない）。[`Stopped`] の
    /// 3 値の字面は宣言順で pin する（C2）。
    #[test]
    fn cycle_stop_target_is_pure_and_refuses_pid_below_two() {
        assert_eq!(StopTarget::plan(0, Some(0)), None);
        assert_eq!(StopTarget::plan(1, Some(1)), None);
        assert_eq!(StopTarget::plan(1, None), None);
        assert_eq!(StopTarget::plan(2, Some(2)), Some(StopTarget::Group(2)));
        assert_eq!(StopTarget::plan(4242, Some(4100)), Some(StopTarget::Single(4242)), "leader でない pid は単体");
        assert_eq!(StopTarget::plan(4242, None), Some(StopTarget::Single(4242)), "pgid を読めない周も単体");
        assert_eq!(StopTarget::Group(2).arg(), "-2");
        assert_eq!(StopTarget::Single(4242).arg(), "4242");
        assert_eq!(StopTarget::Group(2).completion(), Completion::GroupGone(2));
        assert_eq!(StopTarget::Single(4242).completion(), Completion::SeatGone(4242));
    }

    /// 宛先の `/proc` の読み: pgid == pid は group、違えば単体、stat が無ければ単体（撃って wait が決める）、guard は
    /// `/proc` を読む前。[`Stopped`] の 3 値の字面は宣言順で pin する（C2）。
    #[test]
    fn cycle_stop_target_reads_pgid_from_proc_and_stopped_is_pinned() {
        let root = proc_fixture("plan", &[("4242", "sleep", 4200, 4242), ("4243", "sleep", 4200, 4200)], &[]);
        assert_eq!(StopTarget::of(&root, 4242), Some(StopTarget::Group(4242)), "/proc の pgid から group");
        assert_eq!(StopTarget::of(&root, 4243), Some(StopTarget::Single(4243)), "/proc の pgid が違えば単体");
        assert_eq!(StopTarget::of(&root, 4244), Some(StopTarget::Single(4244)), "stat が無ければ単体（撃って wait が決める）");
        assert_eq!(StopTarget::of(&root, 1), None, "guard は /proc を読む前");
        std::fs::remove_dir_all(&root).ok();
        let names: Vec<&str> = STOPPEDS.iter().map(|stopped| stopped.as_str()).collect();
        assert_eq!(names, ["gone", "killed", "unstoppable"]);
        assert!(is_declaration_order(STOPPEDS, |stopped| stopped as usize));
    }

    /// 実子を起こし、`/proc/<pid>/comm` が `sleep` になるまで待つ（上限 2 s・10 ms 刻み・**撃つ前**の fixture 側の待ち）。
    /// `sh -c 'trap "" TERM; exec sleep N'` は sh が `trap` を実行する前に TERM が届くと sh が死んで `Gone` に化ける
    /// （run 1 の cargo-mutants baseline で顕在化）ので、`exec` の後（= `trap` の後）を comm で確かめる。達しない周は `None`。
    fn sleeper(args: &[&str], own_group: bool) -> Option<Child> {
        let mut command = Command::new(args.first().copied().unwrap_or("sleep"));
        command.args(args.iter().skip(1)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        if own_group {
            command.process_group(0);
        }
        let child = command.spawn().ok()?;
        let deadline = Instant::now().checked_add(Duration::from_secs(2))?;
        while Instant::now() < deadline {
            let comm = std::fs::read_to_string(format!("/proc/{}/comm", child.id())).unwrap_or_default();
            if comm.trim() == "sleep" {
                return Some(child);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    /// 子の終わりを別 thread で回収する（回収まで行うので group の消滅の判定に zombie が残らない・`fleet usage` と同型）。
    fn reap(mut child: Child) -> u32 {
        let pid = child.id();
        std::thread::spawn(move || child.wait());
        pid
    }

    /// 実 signal の 3 値（自分の group の実子へ・pid は実子のもの＝2 未満にならない）: TERM で消える子は `Gone`、TERM を
    /// 無視する子（`trap '' TERM` を exec で継いだ `sleep`）は `Killed`、回収されない子（zombie が group に残る）は KILL の後も
    /// 消えず `Unstoppable`。group の leader でない子（自分の group で起こす）は単体宛てで `Gone`。撃った後の待ちは
    /// 唯一の wait のまま（歯は結果を poll しない）。
    #[test]
    fn cycle_stop_terminate_group_reports_gone_killed_and_unstoppable() {
        let grace = Duration::from_millis(300);
        let gone = sleeper(&["sleep", "20"], true).map(reap).expect("子が exec に達しない");
        assert_eq!(terminate_group(gone, grace), Some(Stopped::Gone), "TERM で消える");
        assert!(!Path::new(&format!("/proc/{gone}")).exists(), "wait は消えるまで待ってから返る");

        let killed = sleeper(&["sh", "-c", "trap '' TERM; exec sleep 20"], true).map(reap).expect("子が exec に達しない");
        assert_eq!(terminate_group(killed, grace), Some(Stopped::Killed), "TERM を無視する子は KILL で消える");
        assert!(!Path::new(&format!("/proc/{killed}")).exists());

        let mut zombie = sleeper(&["sleep", "20"], true).expect("子が exec に達しない");
        assert_eq!(terminate_group(zombie.id(), grace), Some(Stopped::Unstoppable), "回収されない zombie は group に残る");
        let _ = zombie.wait();

        let single = sleeper(&["sleep", "20"], false).map(reap).expect("子が exec に達しない");
        assert_eq!(terminate_group(single, grace), Some(Stopped::Gone), "leader でない pid は単体宛てで消える");
        assert!(!Path::new(&format!("/proc/{single}")).exists());
    }
}
