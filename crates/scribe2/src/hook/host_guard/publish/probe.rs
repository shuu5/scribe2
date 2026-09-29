//! publish の配線の上限（設計 docs/design/vessel-hook.md §22 行 n・ADR-0078・SRS FR80 / NFR5）。
//!
//! rules 行 host_guard.publish_deadline_ms と host_guard.publish_read_bytes を読む 1 関数（[`budget`]）と、子を自分の process
//! group に起こし締め切りと読む上限で group ごと止める 1 関数（[`run`]）を持つ。判定（[`super::judge`]）はこの file を呼ばない
//! （呼ぶ段は行 n2 以後）。

use crate::account::wire::TIMEOUT_S;
use crate::invocation::Invocation;
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// 締め切りの行の id。
pub const DEADLINE_ROW: &str = "host_guard.publish_deadline_ms";
/// 読む上限の行の id。
pub const READ_ROW: &str = "host_guard.publish_read_bytes";
/// try_wait の周回の間隔。
const POLL: Duration = Duration::from_millis(10);
/// 全ての子に渡す env（尋ねる入力・更新の通知・言語の揺れを断つ）。
const ENV: [(&str, &str); 4] = [("GIT_TERMINAL_PROMPT", "0"), ("GH_PROMPT_DISABLED", "1"), ("GH_NO_UPDATE_NOTIFIER", "1"), ("LC_ALL", "C")];

/// 子を縛る 2 つ（[`budget`] の値から 1 度だけ決める）。
#[derive(Debug, Clone, Copy)]
pub struct Bound {
    /// 締め切りの時刻。
    pub deadline: Instant,
    /// 標準出力を読む上限（byte）。
    pub limit: u64,
}

/// 子の止まり（閉じた 2 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// 子を起こせない（起こせても待てない周を含む）。
    Spawn,
    /// 締め切りを越えた（group ごと止めた）。
    Deadline,
}

/// 子の終わり。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// 終了の code（signal で終わった周は -1）。
    pub code: i32,
    /// 読んだ標準出力（上限まで）。
    pub bytes: Vec<u8>,
    /// 上限を越えて出したか（越えた周は group ごと止めた）。
    pub over: bool,
}

/// rules 行の整数値（無い・不発効・整数でない・0 は `None`）。
fn int_row(manifest: &Manifest, id: &str) -> Option<u64> {
    match manifest.get(id).filter(|row| row.enabled)?.value {
        RuleValue::Int(found) if found > 0 => Some(found),
        _ => None,
    }
}

/// 予算の読み（締め切りのミリ秒・読む上限の byte）: 2 行を読む。行が無い・不発効・整数でない・0 以下・締め切りが配線の timeout
/// （[`TIMEOUT_S`] の千倍のミリ秒）以上（harness が hook を先に殺すと断りが出ない）のどれかの周は、その行の id を `Err` で返す。
pub fn budget(manifest: &Manifest) -> Result<(u64, u64), &'static str> {
    let deadline = int_row(manifest, DEADLINE_ROW).filter(|ms| *ms < TIMEOUT_S.saturating_mul(1000)).ok_or(DEADLINE_ROW)?;
    let bytes = int_row(manifest, READ_ROW).ok_or(READ_ROW)?;
    Ok((deadline, bytes))
}

/// 子の group ごと kill して待つ（`pipe stop` と同じ `kill -KILL -- -<pgid>`・group の宛先が失敗した周は子だけでも kill）。
fn stop(child: &mut Child) {
    let group = format!("-{}", child.id());
    let _ = Invocation::new("kill").args(["-KILL", "--"]).arg(group).stdout(Stdio::null()).stderr(Stdio::null()).status();
    let _ = child.kill();
    let _ = child.wait();
}

/// 子を撃つ（**1 関数**）: program を自分の process group に起こし、stdin に `input` を別 thread で書き、標準出力を別 thread で
/// `limit` + 1 byte まで読み、標準エラーは捨てる。[`POLL`] で周回し、`deadline` の時刻を越えたら group ごと止めて [`Stop::Deadline`]、
/// `limit` を越えて出したら group ごと止めて越えの印つきで返す。全ての子に [`ENV`] を渡す。
pub fn run<S: AsRef<OsStr>>(program: impl AsRef<OsStr>, args: &[S], cwd: &Path, input: &[u8], bound: Bound) -> Result<Ran, Stop> {
    let Bound { deadline, limit } = bound;
    let mut call = Invocation::new(program);
    call.args(args).current_dir(cwd).process_group(0).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    for (name, value) in ENV {
        call.env(name, value);
    }
    let mut child = call.spawn().map_err(|_| Stop::Spawn)?;
    if let Some(mut pipe) = child.stdin.take() {
        let data = input.to_vec();
        drop(thread::spawn(move || {
            let _ = pipe.write_all(&data);
        }));
    }
    let Some(pipe) = child.stdout.take() else {
        stop(&mut child);
        return Err(Stop::Spawn);
    };
    let over = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&over);
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.take(limit.saturating_add(1)).read_to_end(&mut bytes);
        flag.store(u64::try_from(bytes.len()).is_ok_and(|len| len > limit), Ordering::SeqCst);
        bytes
    });
    let mut status = None;
    while !(reader.is_finished() && status.is_some()) {
        status = status.or_else(|| child.try_wait().ok().flatten());
        if reader.is_finished() && over.load(Ordering::SeqCst) {
            break;
        }
        if Instant::now() >= deadline {
            stop(&mut child);
            return Err(Stop::Deadline);
        }
        thread::sleep(POLL);
    }
    let cut = over.load(Ordering::SeqCst);
    if cut || status.is_none() {
        stop(&mut child);
    }
    let code = status.or_else(|| child.wait().ok()).and_then(|found| found.code()).unwrap_or(-1);
    let mut bytes = reader.join().map_err(|_| Stop::Spawn)?;
    bytes.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    Ok(Ran { code, bytes, over: cut })
}

#[cfg(test)]
mod tests {
    use super::{run, Bound, Ran, Stop};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    /// sh の 1 本の script を撃つ（cwd は tmp・`limit` byte まで読む・締め切りは `ms` ミリ秒後）。
    fn sh(script: &str, input: &[u8], ms: u64, limit: u64) -> Result<Ran, Stop> {
        run("sh", &["-c", script], &std::env::temp_dir(), input, Bound { deadline: Instant::now() + Duration::from_millis(ms), limit })
    }

    /// 歯ごとに違う tmp の file（pid と用途の語で分ける）。
    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("scribe2-publish-limits-{}-{name}", std::process::id()))
    }

    /// pid の process が居ないか zombie になっているか。
    fn gone(pid: &str) -> bool {
        let stat = std::fs::read_to_string(Path::new("/proc").join(pid).join("stat")).unwrap_or_default();
        stat.is_empty() || stat.contains(") Z")
    }

    /// 行 n (a) 入力をそのまま返して rc 3 で終わる子の出力と rc。
    #[test]
    fn publish_limits_child_echoes_input_and_returns_its_rc() {
        let ran = sh("cat; exit 3", b"hello", 5000, 4096).unwrap_or_else(|stop| panic!("撃てる: {stop:?}"));
        assert_eq!(ran, Ran { code: 3, bytes: b"hello".to_vec(), over: false });
    }

    /// 行 n (b) 30 秒眠る孫を持つ子が締め切り 300 ms で締め切りの止まりになり、1.3 秒の内に返り、孫も止まる。
    #[test]
    fn publish_limits_deadline_stops_the_whole_group() {
        let file = scratch("grandchild");
        let script = format!("sleep 30 & echo $! > {}; wait", file.display());
        let start = Instant::now();
        let found = sh(&script, b"", 300, 4096);
        assert_eq!(found, Err(Stop::Deadline), "締め切りの止まり");
        assert!(start.elapsed() < Duration::from_millis(1300), "締め切りに 1 秒を足した内: {:?}", start.elapsed());
        let pid = std::fs::read_to_string(&file).unwrap_or_default().trim().to_owned();
        let _ = std::fs::remove_file(&file);
        assert!(!pid.is_empty(), "孫の pid を書いた");
        for _ in 0..100 {
            if gone(&pid) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("孫 {pid} も止まる");
    }

    /// 行 n (c) 10000 byte を出す子が上限 4096 で 4096 byte と越えの印。
    #[test]
    fn publish_limits_over_the_read_limit_marks_and_cuts() {
        let ran = sh("head -c 10000 /dev/zero", b"", 5000, 4096).unwrap_or_else(|stop| panic!("撃てる: {stop:?}"));
        assert_eq!((ran.bytes.len(), ran.over), (4096, true));
    }

    /// 行 n (d) 無い program は起こせない止まり。
    #[test]
    fn publish_limits_missing_program_cannot_be_spawned() {
        let bound = Bound { deadline: Instant::now() + Duration::from_secs(5), limit: 4096 };
        let found = run("/nonexistent/scribe2-no-such-program", &["x"], Path::new("/"), b"", bound);
        assert_eq!(found, Err(Stop::Spawn));
    }
}
