// flip-check: moved s2-07l.349
//! 便を止める歯: `pipe_stop_`（`--all` / `--run` / process group 宛て）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く（歯の本文は `lifecycle.rs` から移しただけ・
//! `s2-07l.349`）。

use super::*;

#[test]
fn pipe_stop_all_rc0_when_nothing_to_stop() {
    let (repo, state) = repo_with_state();
    for _ in 0..2 {
        let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "対象なしは rc 0（冪等）");
        assert!(stdout_of(&out).contains("seats=0 stopped=0"), "{}", stdout_of(&out));
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_stop_all_terminates_live_runner() {
    let (repo, state) = repo_with_state();
    // **孫**として起こす。test process の子のままだと kill 後に zombie が残り、
    // `/proc/<pid>` が在るせいで「まだ生きている」と読めてしまう（実運用の席は
    // 別 process が起こすので init に引き取られる）。
    let spawned = Command::new("sh")
        .arg("-c")
        .arg("sleep 60 >/dev/null 2>&1 & echo $!")
        .output()
        .expect("fake runner を起こせる");
    let pid: u32 = String::from_utf8_lossy(&spawned.stdout)
        .trim()
        .parse()
        .expect("pid を読める");
    assert!(
        Path::new(&format!("/proc/{pid}")).exists(),
        "fake runner が動いている"
    );
    // 生きた席を event log に置く（spawn は runner の終了まで待つので、席が Live な
    // 周を作るには log 側から組む）。
    let record = bin_cmd()
        .args([
            "fleet", "record", "--kind", "SeatSpawned", "--run", "r1", "--bead", "s2-2e5",
            "--seat", "r1", "--pid", &pid.to_string(), "--state-dir",
        ])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));

    let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "全部止まれば rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("seats=1 stopped=1"), "{}", stdout_of(&out));
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "runner の process は消えている"
    );
    // 2 回目は対象が無く rc 0（冪等）。
    let again = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "冪等");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_stop_returns_rc2_on_malformed_store() {
    let (repo, state) = repo_with_state();
    let events = state.join("fleet").join("events.jsonl");
    fs::create_dir_all(state.join("fleet")).expect("dir を作れる");
    fs::write(&events, "こわれ\n").expect("壊れた行を書ける");
    let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "state が読めない周は rc 2（rc 語彙の 3 値目）"
    );
    assert!(out.stdout.is_empty(), "rc 2 でも stdout は 0 byte");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_stop_counts_seat_without_pid() {
    let (repo, state) = repo_with_state();
    // pid の無い Live 席（`fleet record` の --pid は任意）。
    let record = bin_cmd()
        .args(["fleet", "record", "--kind", "SeatSpawned", "--run", "r9", "--bead", "b",
               "--seat", "s9", "--state-dir"])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));
    let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    assert!(
        stdout_of(&out).contains("seats=1"),
        "pid の無い Live 席も母集団に数える: {}",
        stdout_of(&out)
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_REFUSED)),
        "止められない席が残るので rc 1（「対象なし rc 0」に化けない）"
    );
    clean(&[&repo, &state]);
}

/// PATH の先頭に置く偽 `kill`（引数を 1 起動 1 行で写し rc 1 を返す・返すのは PATH の値と記録 file）。
///
/// **pid 1 のような adversarial な pid は必ずこの下で撃つ**。変異検査は変異を当てた binary で
/// e2e を回すので、guard を壊す変異の周に実の `kill -TERM -- -1`（user の全 process）が走る
/// （2026-09-13 に開発 session が 2 度落ちた）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn kill_stub(state: &Path) -> (String, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = state.join("kill-bin");
    let record = state.join("kill-args");
    fs::create_dir_all(&bin_dir).expect("stub の dir を作れる");
    let shim = bin_dir.join("kill");
    let script = format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 1\n", record.display());
    fs::write(&shim, script).expect("stub を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("stub に実行権を付ける");
    let path = format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default());
    (path, record)
}

#[test]
fn pipe_stop_keeps_unstoppable_seat_live() {
    let (repo, state) = repo_with_state();
    // pid 1 は殺せない。止めていない席を終端にしない（偽の全クリアを作らない）。
    // **実の kill は撃たない**（偽 kill の下で、group 宛てに化けないことも測る）。
    let (path, calls) = kill_stub(&state);
    let record = bin_cmd()
        .args(["fleet", "record", "--kind", "SeatSpawned", "--run", "r8", "--bead", "b",
               "--seat", "s8", "--pid", "1", "--state-dir"])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));
    for round in 1..=2 {
        let out = run_pipe_with_path(&path, &["stop", "--all", "--state-dir", &state.display().to_string()]);
        assert_eq!(
            out.status.code(),
            Some(i32::from(RC_REFUSED)),
            "{round} 回目も rc 1（止めていないのに rc 0 を返さない）: {}",
            stdout_of(&out)
        );
        assert!(stdout_of(&out).contains("seats=1"), "{round} 回目: {}", stdout_of(&out));
    }
    let seen = fs::read_to_string(&calls).unwrap_or_default();
    let lines: Vec<&str> = seen.lines().collect();
    assert!(!lines.is_empty(), "偽 kill が撃たれている（母集団 > 0）: {seen:?}");
    assert!(!lines.iter().any(|line| line.contains("-- -")), "group 宛て（-- -1）を 1 件も撃たない: {seen:?}");
    assert!(lines.contains(&"-TERM -- 1"), "単一 pid 宛ての TERM: {seen:?}");
    assert!(lines.contains(&"-KILL -- 1"), "単一 pid 宛ての KILL: {seen:?}");
    clean(&[&repo, &state]);
}

// ───── 席を process group 宛てに止める（`s2-07l.180`・設計 §5.6・接頭辞 `pipe_stop_group_`） ─────
//
// **自分が起こした子 process 以外の pid に実 signal を送らない**。adversarial な pid（1 / 0 / 無い pid）は
// [`kill_stub`] の下でだけ撃つ。

/// 偽 runner の PATH（`systemd-run` の**無い** host＝包めない経路でも同じ group になる）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn group_path(state: &Path) -> String {
    let bin_dir = state.join("group-bin");
    fs::create_dir_all(&bin_dir).expect("dir を作れる");
    for name in ["sh", "git", "sleep", "setsid"] {
        let found = Command::new("sh")
            .args(["-c", &format!("command -v {name}")])
            .output()
            .expect("command -v を撃てる");
        let real = String::from_utf8_lossy(&found.stdout).trim().to_owned();
        assert!(!real.is_empty(), "{name} を引ける");
        std::os::unix::fs::symlink(&real, bin_dir.join(name)).ok();
    }
    bin_dir.display().to_string()
}

/// 孫まで持つ偽 runner で `pipe spawn` を**背景で**起こし、席が Live になり孫の pid が書かれるまで待つ。
///
/// `body` は runner の script（孫の pid を `pid_file` へ書いてから前景で待つ形）。`path` は spawn の PATH
/// （[`group_path`] か、包める周に固定する [`confined_path`]）。返すのは（便 id・spawn の process・孫の pid）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn spawn_live_seat(
    repo: &Path,
    state: &Path,
    body: &str,
    pid_file: &Path,
    path: &str,
) -> (String, std::process::Child, u32) {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let contract = write_contract(repo, &[], &[]);
    let id = intake(repo, state, &contract);
    let script = state.join("group-runner.sh");
    fs::write(&script, body).expect("runner の script を書ける");
    let runner = format!("sh {}", script.display());
    let mut child = bin_cmd()
        .args(["pipe", "spawn", "--run", &id, "--repo", &repo.display().to_string(),
               "--state-dir", &state.display().to_string(), "--runner", &runner])
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("binary を起動できる");
    let begun = Instant::now();
    loop {
        let pid = fs::read_to_string(pid_file).ok().and_then(|text| text.trim().parse::<u32>().ok());
        let seated = kind_count(state, &id, EventKind::SeatSpawned) == 1;
        if let (Some(found), true) = (pid, seated) {
            return (id, child, found);
        }
        assert!(child.try_wait().ok().flatten().is_none(), "spawn が席を立てる前に終わった");
        assert!(begun.elapsed() < Duration::from_secs(60), "席が Live にならない");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 前提: `pid` が席の group から抜けているか（`SeatSpawned` の pid＝leader の pgid と異なるか）。
///
/// 読めない周（席も pid も見えない）は抜けていない側へ倒す＝前提を作れなかったと読む。
fn left_the_seat_group(state: &Path, id: &str, pid: u32) -> Result<(), String> {
    let leader = events(state)
        .iter()
        .find(|found| found.run == id && found.kind == EventKind::SeatSpawned)
        .and_then(|found| found.pid)
        .and_then(|found| u32::try_from(found).ok())
        .and_then(proc_pgid);
    let own = proc_pgid(pid);
    match (leader, own) {
        (Some(group), Some(found)) if group != found => Ok(()),
        _ => Err(format!("leader の pgid {leader:?} / {pid} の pgid {own:?}")),
    }
}

/// group から抜けた子を持つ偽 runner の script。**子が抜けたのを確かめてから** pid を書く。
///
/// `$!` は fork 直後に出るので、そのまま書くと子が `setsid()` を呼ぶ前の pid を渡しうる（遅い箱で
/// stop の TERM が子にも届き、止め切れる周に化けた＝`s2-07l.186`）。`/proc/$!/stat` の pgid
/// （5 番目の field）が自分の pgid と異なるまで回数の上限つきで待ち、抜けないまま上限に達したら
/// pid を書かずに rc 3 で終える。PATH は [`group_path`] の 4 つだけなので sh の builtin で書く。
// flip-check: retroactive s2-07l.186
fn escaped_runner(pid_file: &Path) -> String {
    format!(
        "setsid sleep 300 &\n\
         child=$!\n\
         read -r line < /proc/$$/stat\n\
         set -- $line\n\
         own=$5\n\
         tries=0\n\
         while :; do\n\
         \x20 read -r line < /proc/$child/stat || exit 3\n\
         \x20 set -- $line\n\
         \x20 [ \"$5\" != \"$own\" ] && break\n\
         \x20 tries=$((tries + 1))\n\
         \x20 [ \"$tries\" -lt 200000 ] || exit 3\n\
         done\n\
         echo $child > '{}'\n\
         wait\n",
        pid_file.display()
    )
}

/// (1) `pipe stop --run` は席の **process group ごと**止める——wrapper だけが死んで孫が残る形を塞ぐ。
///
/// spawn の process は先に外す（test の子）。外さないと runner の終了を見届けた spawn が自分の
/// `SeatStopped` を書き、stop の記帳と数が混ざる。
#[test]
fn pipe_stop_group_run_kills_the_grandchild() {
    let (repo, state) = repo_with_state();
    let pid_file = state.join("grandchild.pid");
    let body = format!("sleep 300 </dev/null >/dev/null 2>&1 &\necho $! > '{}'\nwait\n", pid_file.display());
    let (id, mut spawner, grandchild) = spawn_live_seat(&repo, &state, &body, &pid_file, &group_path(&state));
    spawner.kill().ok();
    spawner.wait().ok();
    assert!(proc_alive(grandchild), "孫が動いている（前提）");

    let out = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    let survived = proc_alive(grandchild);
    reap_own(grandchild);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "席ごと止まる: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("seats=1 stopped=1"), "{}", stdout_of(&out));
    assert!(!survived, "孫 {grandchild} も消えている（group 宛て）");
    assert_eq!(kind_count(&state, &id, EventKind::SeatStopped), 1, "SeatStopped は 1 件");
    assert_eq!(kind_count(&state, &id, EventKind::RunStopped), 1, "RunStopped は 1 件");
    let shown = run_pipe(&["show", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert!(stdout_of(&shown).contains("stage=Stopped"), "終端: {}", stdout_of(&shown));
    clean(&[&repo, &state]);
}

/// (2) group から抜けた子（`setsid`）が runner の stdout を握ったままの周: spawn は stdout の EOF を
/// 待って runner を回収できず、group は zombie の leader で残る＝**止め切れない**。
/// rc 1・`RunStopped` も `SeatStopped` も書かない（run は live のまま）。
///
/// stop を撃つ**前に**子が group から抜けたことを assert する（崩れていれば止め切れる周に化ける
/// ＝偽の緑にしない・`s2-07l.186`）。
#[test]
fn pipe_stop_group_unstoppable_seat_keeps_the_run_live() {
    let (repo, state) = repo_with_state();
    let pid_file = state.join("escaped.pid");
    let body = escaped_runner(&pid_file);
    let (id, mut spawner, escaped) = spawn_live_seat(&repo, &state, &body, &pid_file, &group_path(&state));
    let premise = left_the_seat_group(&state, &id, escaped);
    if premise.is_err() {
        reap_own(escaped);
        spawner.wait().ok();
    }
    assert!(premise.is_ok(), "止め切れない状態を作れなかった: {premise:?}");

    let out = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    let run_stopped = kind_count(&state, &id, EventKind::RunStopped);
    let seat_stopped = kind_count(&state, &id, EventKind::SeatStopped);
    let shown = run_pipe(&["show", "--run", &id, "--state-dir", &state.display().to_string()]);
    // 片付け（抜けた孫を自分で止める → spawn が EOF を見て終わる）。
    reap_own(escaped);
    spawner.wait().ok();

    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "止め切れない周は rc 1: {}", stdout_of(&out));
    assert!(stdout_of(&out).contains("seats=1 stopped=0"), "{}", stdout_of(&out));
    assert!(stderr_of(&out).contains("止められない席"), "{}", stderr_of(&out));
    assert_eq!(run_stopped, 0, "RunStopped を書かない");
    assert_eq!(seat_stopped, 0, "SeatStopped を書かない");
    assert!(stdout_of(&shown).contains("stage=Spawned"), "run は非終端のまま: {}", stdout_of(&shown));
    clean(&[&repo, &state]);
}

/// (2′) 前提 assert の RED の向き: **`setsid` の前に** pid を書く旧 script（遅い箱の再現として子の
/// `setsid` を 30 秒遅らせる）では、書かれた pid はまだ席の group に居る＝前提 assert が落ちる側。
/// この形のまま stop を撃つと子ごと止まり rc 0 に化ける（CI で見た偽の形）ので、片付けは group
/// 宛ての stop そのものに任せる（新しい signal の口を足さない）。
#[test]
fn pipe_stop_group_premature_pid_fails_the_premise() {
    let (repo, state) = repo_with_state();
    let pid_file = state.join("premature.pid");
    let body = format!("(sleep 30; exec setsid sleep 300) &\necho $! > '{}'\nwait\n", pid_file.display());
    let (id, mut spawner, premature) = spawn_live_seat(&repo, &state, &body, &pid_file, &group_path(&state));
    let premise = left_the_seat_group(&state, &id, premature);

    let out = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    spawner.wait().ok();
    let survived = proc_alive(premature);

    assert!(premise.is_err(), "setsid 前の pid は group に居る＝前提 assert が落ちる: {premise:?}");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "子ごと止まる（偽の形）: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("seats=1 stopped=1"), "{}", stdout_of(&out));
    assert!(!survived, "{premature} も group ごと消えている");
    clean(&[&repo, &state]);
}

/// (3) 旧 record の互換: group leader でない pid の席は**単一 pid** の経路で止まる（rc 0）。
#[test]
fn pipe_stop_group_legacy_seat_falls_back_to_the_single_pid() {
    let (repo, state) = repo_with_state();
    // **孫**として起こす（test process の子のままだと zombie が /proc に残る）。
    let spawned = Command::new("sh")
        .arg("-c")
        .arg("sleep 60 >/dev/null 2>&1 & echo $!")
        .output()
        .expect("fake runner を起こせる");
    let pid: u32 = String::from_utf8_lossy(&spawned.stdout).trim().parse().expect("pid を読める");
    assert!(proc_alive(pid), "fake runner が動いている");
    assert_ne!(proc_pgid(pid), Some(pid), "前提: group leader でない（旧 record の席）");
    let record = bin_cmd()
        .args(["fleet", "record", "--kind", "SeatSpawned", "--run", "r7", "--bead", "b",
               "--seat", "s7", "--pid", &pid.to_string(), "--state-dir"])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));
    let out = run_pipe(&["stop", "--all", "--state-dir", &state.display().to_string()]);
    let survived = proc_alive(pid);
    reap_own(pid);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "互換の経路で止まる: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("seats=1 stopped=1"), "{}", stdout_of(&out));
    assert!(!survived, "runner の process は消えている");
    assert_eq!(kind_count(&state, "r7", EventKind::SeatStopped), 1, "SeatStopped は 1 件");
    clean(&[&repo, &state]);
}

// ───── stop 起因の終端を oom-kill に誤分類しない（`s2-07l.340`・設計 pipeline.md §23・接頭辞 `pipe_spawn_terminal_reason_`） ─────

/// 偽 `systemd-run` の argv を 1 起動 1 行で写す file 名。
const CONFINED_CALLS: &str = "systemd-run-calls";

/// **包める周に固定する** PATH: 偽 `systemd-run`（argv を写し `--` の後ろを exec する・`tests/e2e/pipe/gate.rs` の
/// 同型）を [`group_path`] の前に積む。
///
/// [`group_path`] だけだと包めない host になり、「oom-kill が 0 件」が**包めないことで空虚に充足する**。
/// 包みの終端行は実 scope の中でしか出ない（偽の包みの中では `/proc/self/cgroup` が一致しない）＝kernel の証拠が
/// 無い周を作る。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn confined_path(state: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = state.join("confined-bin");
    fs::create_dir_all(&bin_dir).expect("stub の dir を作れる");
    let shim = bin_dir.join("systemd-run");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nwhile [ $# -gt 0 ] && [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
        state.join(CONFINED_CALLS).display()
    );
    fs::write(&shim, script).expect("stub を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("stub に実行権を付ける");
    format!("{}:{}", bin_dir.display(), group_path(state))
}

/// runner の起動が偽 `systemd-run` を通った（包めた）か。
fn runner_was_confined(state: &Path) -> bool {
    fs::read_to_string(state.join(CONFINED_CALLS))
        .unwrap_or_default()
        .lines()
        .any(|line| line.contains("--scope") && line.contains("-runner-"))
}

/// `fleet record` を 1 回撃つ（rc 0 を assert・stop.rs の既存の 4 か所と同じ形）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn record_event(state: &Path, args: &[&str]) {
    let out = bin_cmd()
        .args(["fleet", "record"])
        .args(args)
        .arg("--state-dir")
        .arg(state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{args:?}: {}", stderr_of(&out));
}

/// 便の `RunStage` のうち detail が `detail` の行の段（物理順）。
fn stages_with(state: &Path, id: &str, detail: &str) -> Vec<Option<Stage>> {
    stages(state, id)
        .into_iter()
        .filter(|(_, found)| found.as_deref() == Some(detail))
        .map(|(stage, _)| stage)
        .collect()
}

/// (a) **包める周**で走る runner の便を `pipe stop --run` で止めると、spawn の終端検出は段を書かない:
/// `detail=oom-kill` の `RunStage` は 0 件・印 `(Spawned, stopping)` が `RunStopped` より前に 1 件・`RunStopped` 1 件・
/// show は `stage=Stopped`。base は包めた周の rc < 0 だけで `Failed detail=oom-kill` を書く（実測 2026-09-15 の再現）。
#[test]
fn pipe_spawn_terminal_reason_stop_is_not_oom() {
    let (repo, state) = repo_with_state();
    let pid_file = state.join("confined.pid");
    let body = format!("sleep 300 </dev/null >/dev/null 2>&1 &\necho $! > '{}'\nwait\n", pid_file.display());
    let (id, mut spawner, grandchild) = spawn_live_seat(&repo, &state, &body, &pid_file, &confined_path(&state));
    let out = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    let survived = proc_alive(grandchild);
    reap_own(grandchild);
    // spawn の終端検出（runner の消滅を見た経路）まで見届けてから測る。
    spawner.wait().ok();

    assert!(runner_was_confined(&state), "前提: runner は包めた周で起きた（空虚な充足にしない）");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "席ごと止まる: {}", stderr_of(&out));
    assert!(!survived, "孫 {grandchild} も消えている");
    let seen = trail(&state, &id);
    assert!(stages_with(&state, &id, "oom-kill").is_empty(), "stop の kill を oom-kill に化けさせない: {seen:?}");
    assert_eq!(stage_count(&state, &id, Stage::Failed), 0, "Failed を 1 件も書かない: {seen:?}");
    let mark = seen
        .iter()
        .position(|(kind, stage, detail)| {
            *kind == EventKind::RunStage && *stage == Some(Stage::Spawned) && detail.as_deref() == Some("stopping")
        });
    let stopped = seen.iter().position(|(kind, _, _)| *kind == EventKind::RunStopped);
    assert!(matches!((mark, stopped), (Some(at), Some(end)) if at < end), "印は RunStopped より前: {seen:?}");
    assert_eq!(stages_with(&state, &id, "stopping").len(), 1, "印は 1 件: {seen:?}");
    assert_eq!(kind_count(&state, &id, EventKind::RunStopped), 1, "RunStopped は 1 件");
    let shown = run_pipe(&["show", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert!(stdout_of(&shown).contains("stage=Stopped"), "終端: {}", stdout_of(&shown));
    clean(&[&repo, &state]);
}

/// (e) 止められない席（偽 kill の下の pid 1）を持つ便を `pipe stop --run` で撃つ: rc 1・`RunStopped` 0 件・**生の**
/// 最後の `RunStage` が印（段は `Spawned` のまま）。止め切れない 2 回目も印を増やさない。席が消えた後
/// （`SeatStopped` を record）の stop は signal を送らず、`RunStopped` を 1 件書く。base は印を書かない。
#[test]
fn pipe_spawn_terminal_reason_mark_survives_an_unstoppable_stop() {
    let (repo, state) = repo_with_state();
    let (path, _calls) = kill_stub(&state);
    record_event(&state, &["--kind", "RunStage", "--run", "r5", "--bead", "b", "--stage", "Spawned", "--detail", "base:abc"]);
    record_event(&state, &["--kind", "SeatSpawned", "--run", "r5", "--bead", "b", "--seat", "s5", "--pid", "1"]);
    let dir = state.display().to_string();
    for round in 1..=2 {
        let out = run_pipe_with_path(&path, &["stop", "--run", "r5", "--state-dir", &dir]);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{round} 回目は止め切れない: {}", stdout_of(&out));
        assert_eq!(kind_count(&state, "r5", EventKind::RunStopped), 0, "{round} 回目: RunStopped を書かない");
        assert_eq!(
            stages(&state, "r5").last(),
            Some(&(Some(Stage::Spawned), Some("stopping".to_owned()))),
            "{round} 回目: 生の最後の RunStage は印・段は変わらない"
        );
        assert_eq!(stages_with(&state, "r5", "stopping").len(), 1, "{round} 回目: 印は 1 件のまま");
        let shown = run_pipe(&["show", "--run", "r5", "--state-dir", &dir]);
        assert!(stdout_of(&shown).contains("stage=Spawned"), "{round} 回目: live のまま: {}", stdout_of(&shown));
    }
    record_event(&state, &["--kind", "SeatStopped", "--run", "r5", "--bead", "b", "--seat", "s5", "--pid", "1"]);
    let last = run_pipe_with_path(&path, &["stop", "--run", "r5", "--state-dir", &dir]);
    assert_eq!(last.status.code(), Some(i32::from(RC_OK)), "席が消えた後は止め切れる: {}", stderr_of(&last));
    assert_eq!(kind_count(&state, "r5", EventKind::RunStopped), 1, "RunStopped を 1 件書く");
    let shown = run_pipe(&["show", "--run", "r5", "--state-dir", &dir]);
    assert!(stdout_of(&shown).contains("stage=Stopped"), "終端: {}", stdout_of(&shown));
    clean(&[&repo, &state]);
}

/// (f) 衝突を記帳した `Implemented` の便（`rebase-conflict:<base>..<main>`）に live 席を置いて `pipe stop --run` を撃つ:
/// (i) log に印（`RunStage stage=Implemented detail=stopping`）が 1 件在り（base は書かない＝RED）、(ii) 席が消えた後も
/// resume はその便を**起こし直しの続き**と読む（`--runner` を要る＝印が衝突の記帳を隠さない）。印を読み飛ばさない
/// 口では resume が gate へ流れ、`--runner が要る` を名乗らない。
#[test]
fn pipe_spawn_terminal_reason_mark_keeps_the_conflict_readable() {
    let (repo, state) = repo_with_state();
    let (path, _calls) = kill_stub(&state);
    let conflict = "rebase-conflict:1111111..2222222";
    record_event(&state, &["--kind", "RunStage", "--run", "r6", "--bead", "b", "--stage", "Implemented", "--detail", conflict]);
    record_event(&state, &["--kind", "SeatSpawned", "--run", "r6", "--bead", "b", "--seat", "s6", "--pid", "1"]);
    let dir = state.display().to_string();
    let out = run_pipe_with_path(&path, &["stop", "--run", "r6", "--state-dir", &dir]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "止め切れない（偽 kill）: {}", stdout_of(&out));
    assert_eq!(stages_with(&state, "r6", "stopping"), vec![Some(Stage::Implemented)], "(i) 印の行が 1 件: {:?}", stages(&state, "r6"));
    record_event(&state, &["--kind", "SeatStopped", "--run", "r6", "--bead", "b", "--seat", "s6", "--pid", "1"]);
    let resumed = run_pipe(&["resume", "--run", "r6", "--state-dir", &dir]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_REFUSED)), "(ii) 起こし直しの続きは --runner を要る: {}", stderr_of(&resumed));
    assert!(stderr_of(&resumed).contains("--runner が要る"), "(ii) 衝突の記帳を読む: {}", stderr_of(&resumed));
    clean(&[&repo, &state]);
}

// ───── 審査の理由の閉じた型・器が作る INCONCLUSIVE の「scope の中で死んだ」形（`s2-07l.395`・設計 contract-source.md §22） ─────

/// 審査の lens の起動が偽 `systemd-run` を通った（包めた）か（[`runner_was_confined`] の審査の側）。
fn lens_was_confined(state: &Path) -> bool {
    fs::read_to_string(state.join(CONFINED_CALLS))
        .unwrap_or_default()
        .lines()
        .any(|line| line.contains("--scope") && line.contains("-review-"))
}

/// 歯 (2) の 5 形のうち「scope の中で死んだ」: **包める周**（[`confined_path`]）で審査の lens が自分を signal で殺すと、
/// 器は INCONCLUSIVE（`scope の中で死んだ`）を作り、理由の型は 7 語目 `unparsed`・`at` は無い・detail は
/// `verdict:INCONCLUSIVE kind:unparsed`・rc 3。包めたことを argv の写しで測る（包めない host では別の理由に落ちて
/// 空虚に充足する）。
#[test]
fn pipe_review_kind_lens_killed_in_scope_is_unparsed() {
    let (repo, state) = repo_with_state();
    let contract = write_contract(&repo, &[], &[]);
    let out = run_pipe_with_path(&confined_path(&state), &[
        "intake", "--design", &contract, "--bead", "s2-scope",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state), "--lens", "kill -9 $$",
    ]);
    assert!(lens_was_confined(&state), "前提: lens は包めた周で起きた（空虚な充足にしない）");
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "箱の中の死は INCONCLUSIVE: {}", stderr_of(&out));
    let id = run_id_of(&out);
    let pairs = intake::review_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "{pairs:?}");
    assert!(value_of(&pairs, "evidence").contains("scope の中で死んだ"), "理由は箱の中の死: {pairs:?}");
    assert_eq!(value_of(&pairs, "kind"), "unparsed", "lens の JSON が無い周は 7 語目: {pairs:?}");
    assert!(!pairs.iter().any(|(key, _)| key == "at"), "at は無い: {pairs:?}");
    assert_eq!(intake::reviewed_detail(&state, &id), "verdict:INCONCLUSIVE kind:unparsed");
    clean(&[&repo, &state]);
}
