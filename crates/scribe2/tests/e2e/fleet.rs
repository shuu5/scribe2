//! fleet event log の歯（設計 docs/design/fleet-event-log.md §6）。
//!
//! 置き場は毎回 tmp dir を `--state-dir` で指す（env も HOME も読まない形の裏返し）。

use crate::make_tmp_dir;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::mem::discriminant;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant, SystemTime};
use vessel::order::is_declaration_order;
use vessel::fleet::store::{self, LockPolicy, StoreError};
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
use vessel::polarity::{OnFailure, Polarity, Timing};
use vessel::rules::manifest::Manifest;
use vessel::fleet::json_tree::{self, parse, Tree, TreeError, MAX_DEPTH};
use vessel::fleet::select::{self, Input, Purpose, Selection};
use vessel::fleet::{KINDS, REASONS, STAGES, WINDOWS};
use vessel::fleet::{
    json_lite, replay, wait, Allowance, AllowanceKey, Completion, Event, EventKind, Measured,
    Registration, SeatState, Stage, Timeout, Unmeasured, UnmeasuredReason, WindowKind, SCHEMA,
};
use vessel::seat::role::Role;

/// binary の path。
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_scribe2")
}

/// tmp の state dir。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn state_dir() -> PathBuf {
    make_tmp_dir().expect("tmp dir を作れる")
}

/// 最小の event を組む。
fn event(kind: EventKind, run: &str, ts: &str) -> Event {
    Event {
        schema: SCHEMA,
        ts: ts.to_owned(),
        kind,
        run: run.to_owned(),
        bead: "s2-x".to_owned(),
        host: "h".to_owned(),
        actor: kind.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: None,
        allowance: None,
        registration: None,
        account: None,
    }
}

/// `fleet` を binary で 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_fleet(args: &[&str]) -> Output {
    Command::new(bin())
        .arg("fleet")
        .args(args)
        .output()
        .expect("binary を起動できる")
}

/// PATH を差し替えて `fleet` を binary で 1 回撃つ（`pipe.rs` の `run_pipe_with_path` と同じ型）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_fleet_with_env(args: &[&str], path: &str) -> Output {
    Command::new(bin())
        .arg("fleet")
        .args(args)
        .env("PATH", path)
        .output()
        .expect("binary を起動できる")
}

/// store に生の行を書く（malformed の fixture 用）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_raw(dir: &Path, lines: &[&str]) {
    let path = store::events_path(dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("dir を作れる");
    }
    fs::write(&path, format!("{}\n", lines.join("\n"))).expect("fixture を書ける");
}

#[test]
fn outcome_rc_covers_zero_one_two() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let args = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<String>>();
    let empty = vessel::fleet::cli::dispatch(&args(&["export", "--state-dir", &path]));
    assert_eq!(empty.rc, RC_OK, "空の store は rc 0");
    write_raw(&dir, &["こわれ"]);
    let now_broken = vessel::fleet::cli::dispatch(&args(&["export", "--state-dir", &path]));
    assert_eq!(now_broken.rc, RC_BROKEN, "読めない store は rc 2");
    let refused = vessel::fleet::cli::dispatch(&args(&["show", "--run", "r1"]));
    assert_eq!(refused.rc, RC_REFUSED, "--state-dir 欠けは rc 1");
    assert!(refused.out.is_empty(), "rc 1 でも stdout は 0 byte");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_json_rejects_short_unicode() {
    // 断りの字面まで見る。`16 進でない` だけだと base の `from_str_radix` の error
    // （"invalid digit found in string"）にも当たってしまい、guard を外しても通る。
    for bad in [
        r#"{"a":"\u+123"}"#,
        r#"{"a":"\u12"}"#,
        r#"{"a":"\uZZZZ"}"#,
    ] {
        let reason = json_lite::parse_object(bad).expect_err("桁が 16 進でない \\u は拒む");
        assert!(
            reason.contains("\\u の桁が 16 進でない"),
            "理由: {reason}（入力 {bad}）"
        );
    }
    // 入力が尽きる経路（4 桁に届かない）は別の断りになる。
    let cut = json_lite::parse_object(r#"{"a":"\u12"#).expect_err("桁が足りない \\u は拒む");
    assert!(cut.contains("\\u の 4 桁が足りない"), "理由: {cut}");
    let good = json_lite::parse_object(r#"{"a":"\u0041"}"#).expect("正しい 4 桁は通る");
    let (_, value) = good.first().expect("1 組");
    assert_eq!(value.as_str(), Some("A"), "U+0041 は A");
}

#[test]
fn fleet_json_rejects_unknown_key() {
    let mut pairs: Vec<(&str, json_lite::Value)> = vec![
        ("schema", json_lite::Value::Num(SCHEMA)),
        ("ts", json_lite::Value::Str("2026-09-09T00:00:00Z".to_owned())),
        ("kind", json_lite::Value::Str("RunCreated".to_owned())),
        ("run", json_lite::Value::Str("r1".to_owned())),
        ("bead", json_lite::Value::Str("b1".to_owned())),
        ("host", json_lite::Value::Str("h".to_owned())),
        ("actor", json_lite::Value::Str("machine".to_owned())),
    ];
    assert!(Event::from_line(&json_lite::write_object(&pairs)).is_ok(), "既知 key だけなら通る");
    pairs.push(("stgae", json_lite::Value::Str("Gated".to_owned())));
    let reason = Event::from_line(&json_lite::write_object(&pairs))
        .expect_err("綴り違いの key を黙って捨てない");
    assert!(reason.contains("未知の key stgae"), "理由: {reason}");
}

#[test]
fn fleet_replay_rebuilds_state_from_events() {
    let mut created = event(EventKind::RunCreated, "r1", "2026-09-09T00:00:00Z");
    created.stage = Some(Stage::Intake);
    let mut moved = event(EventKind::RunStage, "r1", "2026-09-09T00:00:10Z");
    moved.stage = Some(Stage::Implemented);
    let state = replay(&[created, moved]);
    let run = state.runs.get("r1").expect("r1 が在る");
    assert_eq!(run.stage, Stage::Implemented, "物理順で最後の stage が現在地");
    assert_eq!(run.updated, "2026-09-09T00:00:10Z", "最後の ts");
    assert!(!run.approved, "承認 event はまだ無い");
}

#[test]
fn fleet_replay_marks_run_approved_on_approval_received() {
    let asked = event(EventKind::ApprovalRequested, "r1", "2026-09-09T00:00:00Z");
    let mut got = event(EventKind::ApprovalReceived, "r1", "2026-09-09T00:00:05Z");
    // 逐語まで揃って初めて承認である（C7.2）。読み手は actor と detail も見る。
    got.detail = Some("消してよい".to_owned());
    assert_eq!(got.actor, "human", "承認の受理だけが人由来（FR22）");
    let state = replay(&[asked, got]);
    assert!(state.runs.get("r1").expect("r1 が在る").approved, "approved は導出値");
}

#[test]
fn fleet_replay_seat_state_is_stopped_after_seat_stopped() {
    let mut up = event(EventKind::SeatSpawned, "r1", "2026-09-09T00:00:00Z");
    up.seat = Some("s1".to_owned());
    up.pid = Some(4242);
    let mut down = event(EventKind::SeatStopped, "r1", "2026-09-09T00:00:09Z");
    down.seat = Some("s1".to_owned());
    let state = replay(&[up, down]);
    let seat = state.seats.get("s1").expect("s1 が在る");
    assert_eq!(seat.state, SeatState::Stopped, "畳んだ席は Stopped");
    assert_eq!(seat.pid, Some(4242), "pid は残る");
}

#[test]
fn fleet_read_rejects_malformed_line_with_line_number() {
    let dir = state_dir();
    let good = event(EventKind::RunCreated, "r1", "2026-09-09T00:00:00Z").to_line();
    let third = event(EventKind::RunDone, "r3", "2026-09-09T00:00:20Z").to_line();
    write_raw(&dir, &[&good, "{ここは JSON でない", &third]);
    let errors = store::read_all(&dir).expect_err("malformed は Err になる");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(ToString::to_string).unwrap_or_default();
    assert!(first.contains("line=2"), "行番号: {first}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_read_rejects_unknown_schema() {
    let dir = state_dir();
    let line = json_lite::write_object(&[
        ("schema", json_lite::Value::Num(2)),
        ("ts", json_lite::Value::Str("2026-09-09T00:00:00Z".to_owned())),
        ("kind", json_lite::Value::Str("RunCreated".to_owned())),
        ("run", json_lite::Value::Str("r1".to_owned())),
        ("bead", json_lite::Value::Str("s2-x".to_owned())),
        ("host", json_lite::Value::Str("h".to_owned())),
        ("actor", json_lite::Value::Str("machine".to_owned())),
    ]);
    write_raw(&dir, &[&line]);
    let errors = store::read_all(&dir).expect_err("未知 schema は Err になる");
    let joined: Vec<String> = errors.iter().map(ToString::to_string).collect();
    assert!(joined.join("\n").contains("schema"), "理由: {joined:?}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_append_serializes_concurrent_writers() {
    let dir = state_dir();
    let policy = LockPolicy::embedded().expect("rules 行を引ける");
    std::thread::scope(|scope| {
        for thread in 0..8_u32 {
            let dir = dir.clone();
            scope.spawn(move || {
                for seq in 0..50_u32 {
                    let run = format!("r{thread}-{seq}");
                    let ev = event(EventKind::RunCreated, &run, "2026-09-09T00:00:00Z");
                    store::append(&dir, &ev, policy).expect("追記できる");
                }
            });
        }
    });
    let events = store::read_all(&dir).expect("全行 parse できる＝interleave 0");
    assert_eq!(events.len(), 400, "8 thread × 50 = 400 行");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_state_survives_process_restart() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let recorded = run_fleet(&[
        "record", "--kind", "RunStage", "--run", "r1", "--bead", "s2-x", "--stage", "Gated",
        "--state-dir", &path,
    ]);
    assert!(recorded.status.success(), "record の rc: {recorded:?}");
    let shown = run_fleet(&["show", "--run", "r1", "--state-dir", &path]);
    assert!(shown.status.success(), "show の rc: {shown:?}");
    let line = String::from_utf8_lossy(&shown.stdout);
    assert!(line.contains("stage=Gated"), "別 process が同じ現在地を読む: {line}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_export_first_line_is_schema_header() {
    let dir = state_dir();
    let out = run_fleet(&["export", "--state-dir", &dir.display().to_string()]);
    assert!(out.status.success(), "export の rc: {out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next().unwrap_or_default();
    let pairs = json_lite::parse_object(first).expect("header は flat JSON");
    let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, vec!["schema", "kind", "host", "runs", "seats"], "header の key 並び");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_export_is_read_only() {
    let dir = state_dir();
    let path = dir.display().to_string();
    for run in ["r1", "r2"] {
        let out = run_fleet(&["record", "--kind", "RunCreated", "--run", run, "--bead", "s2-x", "--state-dir", &path]);
        assert!(out.status.success(), "record の rc: {out:?}");
    }
    let events = store::events_path(&dir);
    let before = fs::metadata(&events).expect("event log が在る");
    let out = run_fleet(&["export", "--state-dir", &path]);
    let after = fs::metadata(&events).expect("event log が在る");
    assert!(out.status.success(), "export の rc: {out:?}");
    let lines = String::from_utf8_lossy(&out.stdout).lines().count();
    assert_eq!(lines, 3, "header 1 + run 2 + seat 0");
    assert_eq!(before.len(), after.len(), "size を変えない");
    assert_eq!(
        before.modified().ok(),
        after.modified().ok(),
        "mtime を変えない"
    );
    assert!(!store::lock_path(&dir).exists(), "lock を残さない");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_state_dir_flag_is_required() {
    let out = run_fleet(&["show", "--run", "r1"]);
    assert_eq!(out.status.code(), Some(1), "rc 1");
    assert!(out.stdout.is_empty(), "stdout は 0 byte");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--state-dir"), "使い方を stderr へ: {err}");
}

#[test]
fn fleet_json_roundtrip_escapes() {
    let nasty = "quote=\" back=\\ tab=\t nl=\n ctrl=\u{1}";
    let line = json_lite::write_object(&[("detail", json_lite::Value::Str(nasty.to_owned()))]);
    let pairs = json_lite::parse_object(&line).expect("読み戻せる");
    let (_, value) = pairs.first().expect("1 組");
    assert_eq!(value.as_str(), Some(nasty), "escape を往復して同じ");
    assert!(!line.contains('\n'), "1 行に収まる: {line:?}");
}

#[test]
fn fleet_stale_lock_is_removed_after_threshold() {
    let dir = state_dir();
    let policy = LockPolicy::embedded().expect("rules 行を引ける");
    let lock = store::lock_path(&dir);
    if let Some(parent) = lock.parent() {
        fs::create_dir_all(parent).expect("dir を作れる");
    }
    let handle = fs::File::create(&lock).expect("lock を置ける");
    let old = SystemTime::now() - Duration::from_millis(policy.stale_ms + 60_000);
    handle.set_modified(old).expect("mtime を戻せる");
    drop(handle);
    let ev = event(EventKind::RunCreated, "r1", "2026-09-09T00:00:00Z");
    let warnings = store::append(&dir, &ev, policy).expect("古い lock を外して追記できる");
    assert!(!warnings.is_empty(), "黙って消さず警告に載せる");
    assert!(!lock.exists(), "lock は残らない");
    fs::remove_dir_all(&dir).ok();
}

/// 所有者の死んだ lock（書き手が lock を持ったまま SIGKILL で落ちた周が残す形＝pid の 10 進 1 行）は、
/// stale の線（ここでは 1 時間）に掛からなくても外して追記でき、**警告 1 行**で名乗る。
///
/// 死んだ pid は自分が起こして回収した子のもの（`/proc/<pid>` が無いことを前提 assert する）。
/// 置いたばかりの lock なので mtime の線には掛からず、base（中身を見ない）は retry を待ち切って
/// `StoreError::Lock` になる＝この歯は base で決定的に赤い。
#[test]
fn fleet_dead_owner_lock_is_removed_with_a_warning() {
    let mut child = Command::new("true").spawn().expect("子を起こせる");
    let dead = child.id();
    child.wait().expect("子を回収できる");
    assert!(!Path::new(&format!("/proc/{dead}")).exists(), "前提: 殺した pid {dead} は生きていない");
    let strict = Manifest::parse(&lock_rules(50, 3_600_000)).expect("fixture を読める");
    let strict = LockPolicy::from_rules(&strict).expect("2 行を引ける");

    let dir = state_dir();
    let lock = store::lock_path(&dir);
    if let Some(parent) = lock.parent() {
        fs::create_dir_all(parent).expect("dir を作れる");
    }
    fs::write(&lock, format!("{dead}\n")).expect("死んだ所有者の lock を置ける");
    let ev = event(EventKind::RunCreated, "r1", "2026-09-09T00:00:00Z");
    let warnings = store::append(&dir, &ev, strict).expect("所有者の死んだ lock を外して追記できる");
    assert_eq!(
        warnings.iter().map(|w| w.as_str()).collect::<Vec<&str>>(),
        ["fleet: 所有者の死んだ lock を外した"],
        "外したことを警告 1 行で名乗る（古い lock の警告とは別の理由）"
    );
    assert!(!lock.exists(), "lock は残らない");
    assert_eq!(store::read_all(&dir).map(|found| found.len()), Ok(1), "追記は届いている");

    // 生きている所有者の lock（自分の pid）は外さない＝retry を待ち切って error（極性不変）。
    fs::write(&lock, format!("{}\n", std::process::id())).expect("生きた所有者の lock を置ける");
    let blocked = store::append(&dir, &ev, strict);
    assert!(matches!(blocked, Err(StoreError::Lock(_))), "生きている所有者の lock は待つ側: {blocked:?}");
    fs::remove_file(&lock).ok();
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_wait_times_out_with_typed_error() {
    let alive = std::process::id();
    let outcome = wait(Completion::RunnerExited(alive), Duration::from_millis(60));
    assert_eq!(outcome, Err(Timeout), "生きている pid は期限で Timeout");
    assert_eq!(Completion::SeatGone(alive).pid(), alive, "見張る pid は 2 variant 共通");
}

#[test]
fn fleet_export_rc2_on_malformed_store() {
    let dir = state_dir();
    write_raw(&dir, &["これは JSON ではない"]);
    let out = run_fleet(&["export", "--state-dir", &dir.display().to_string()]);
    assert_eq!(out.status.code(), Some(2), "壊れた store は rc 2");
    assert!(out.stdout.is_empty(), "stdout へは書かない");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("line=1"), "行番号つきで断る: {err}");
    fs::remove_dir_all(&dir).ok();
}

/// 2 行だけの rules fixture（lock の 2 値を外から与える）。
///
/// `enabled` は**必須 key**なので全行に書く（`s2-07l.80`）。この便の test 区間の差は
/// この字面の追加だけで、assert の意味は 1 つも動かない——base の loader は `enabled` を
/// 書いた行も同じ値で読むので、base で新しく赤くなる歯は 1 本も無い。
// flip-check: retroactive s2-07l.80
fn lock_rules(retry_ms: u64, stale_ms: u64) -> String {
    format!(
        "schema = 1\n\n[[rule]]\nid = \"fleet.lock_retry_ms\"\nkind = \"LockRetryMs\"\nvalue = {retry_ms}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n\n[[rule]]\nid = \"fleet.lock_stale_ms\"\nkind = \"LockStaleMs\"\nvalue = {stale_ms}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n"
    )
}

#[test]
fn fleet_record_rejects_unknown_actor() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let out = run_fleet(&[
        "record", "--kind", "RunCreated", "--run", "r1", "--bead", "b1", "--actor", "bogus",
        "--state-dir", &path,
    ]);
    assert_eq!(out.status.code(), Some(1), "書き側で断る");
    assert!(out.stdout.is_empty(), "stdout へは書かない");
    assert!(
        !store::events_path(&dir).exists(),
        "読めない行を append-only の log に残さない"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("machine でも human でもない"), "理由: {err}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_record_rejects_flag_without_value() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let out = run_fleet(&[
        "record", "--kind", "RunCreated", "--run", "r1", "--bead", "b1", "--detail",
        "--state-dir", &path,
    ]);
    assert_eq!(out.status.code(), Some(1), "値の無い flag は黙って落とさない");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--detail に値が無い"), "理由: {err}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_json_rejects_duplicate_key() {
    let line = "{\"schema\":1,\"schema\":9}";
    let reason = json_lite::parse_object(line).expect_err("重複 key は拒む");
    assert!(reason.contains("重複"), "理由: {reason}");
}

#[test]
fn fleet_export_reports_runs_and_seats() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let calls: [&[&str]; 3] = [
        &["record", "--kind", "RunCreated", "--run", "r1", "--bead", "b1", "--stage", "Gated"],
        &["record", "--kind", "SeatSpawned", "--run", "r1", "--bead", "b1", "--seat", "s1", "--pid", "4242"],
        &["record", "--kind", "ApprovalReceived", "--run", "r1", "--bead", "b1", "--detail", "消してよい"],
    ];
    for call in calls {
        let mut args = call.to_vec();
        args.extend_from_slice(&["--state-dir", &path]);
        assert!(run_fleet(&args).status.success(), "record: {call:?}");
    }
    let out = run_fleet(&["export", "--state-dir", &path]);
    assert!(out.status.success(), "export の rc: {out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "header + run 1 + seat 1: {lines:?}");
    let header = json_lite::parse_object(lines.first().copied().unwrap_or_default()).expect("header");
    let field = |pairs: &[(String, json_lite::Value)], key: &str| {
        pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };
    assert_eq!(field(&header, "runs"), Some(json_lite::Value::Num(1)), "runs の件数");
    assert_eq!(field(&header, "seats"), Some(json_lite::Value::Num(1)), "seats の件数");
    let run = json_lite::parse_object(lines.get(1).copied().unwrap_or_default()).expect("run 行");
    assert_eq!(field(&run, "stage"), Some(json_lite::Value::Str("Gated".to_owned())), "段");
    assert_eq!(field(&run, "approved"), Some(json_lite::Value::Bool(true)), "承認は導出値");
    let seat = json_lite::parse_object(lines.get(2).copied().unwrap_or_default()).expect("seat 行");
    assert_eq!(field(&seat, "state"), Some(json_lite::Value::Str("Live".to_owned())), "席の状態");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_wait_returns_ok_when_process_exits() {
    let mut child = Command::new("true").spawn().expect("子を起こせる");
    let pid = child.id();
    child.wait().expect("子を回収できる");
    let outcome = wait(Completion::SeatGone(pid), Duration::from_millis(500));
    assert_eq!(outcome, Ok(()), "消えた pid は Ok を返す");
}

#[test]
fn fleet_lock_policy_comes_from_rules_rows() {
    let strict = Manifest::parse(&lock_rules(1, 3_600_000)).expect("fixture を読める");
    let strict = LockPolicy::from_rules(&strict).expect("2 行を引ける");
    assert_eq!(strict.retry_ms, 1, "再試行の上限は rules 行から");
    assert_eq!(strict.stale_ms, 3_600_000, "stale の線は rules 行から");

    let dir = state_dir();
    let lock = store::lock_path(&dir);
    if let Some(parent) = lock.parent() {
        fs::create_dir_all(parent).expect("dir を作れる");
    }
    fs::File::create(&lock).expect("lock を置ける");
    let ev = event(EventKind::RunCreated, "r1", "2026-09-09T00:00:00Z");
    let blocked = store::append(&dir, &ev, strict);
    assert!(blocked.is_err(), "stale の線が遠いと新しい lock は外さない");

    let loose = Manifest::parse(&lock_rules(1, 0)).expect("fixture を読める");
    let loose = LockPolicy::from_rules(&loose).expect("2 行を引ける");
    let warnings = store::append(&dir, &ev, loose).expect("線が 0 なら外して進む");
    assert!(!warnings.is_empty(), "外したことは警告に載る");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_read_collects_every_malformed_line() {
    let dir = state_dir();
    let good = event(EventKind::RunCreated, "r1", "2026-09-09T00:00:00Z").to_line();
    write_raw(&dir, &["こわれ 1", &good, "こわれ 3"]);
    let errors = store::read_all(&dir).expect_err("malformed は Err になる");
    assert_eq!(errors.len(), 2, "最初の 1 件で止めない: {errors:?}");
    let joined: Vec<String> = errors.iter().map(ToString::to_string).collect();
    let text = joined.join("\n");
    assert!(text.contains("line=1") && text.contains("line=3"), "両方の行番号: {text}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_host_ignores_env() {
    let dir = state_dir();
    let out = Command::new(bin())
        .args(["fleet", "export", "--state-dir", &dir.display().to_string()])
        .env("HOSTNAME", "bogus-from-env")
        .output()
        .expect("binary を起動できる");
    assert!(out.status.success(), "export の rc: {out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        !text.contains("bogus-from-env"),
        "HOSTNAME env を読まない（憲法 C2.2）: {text}"
    );
    assert!(
        text.contains(&vessel::fleet::cli::host()),
        "host は /etc/hostname か hostname コマンドから来る: {text}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_event_log_path_is_the_cross_version_seam() {
    let dir = PathBuf::from("/tmp/does-not-need-to-exist");
    assert!(
        store::events_path(&dir).ends_with("fleet/events.jsonl"),
        "跨版 面 2 の path は固定: {:?}",
        store::events_path(&dir)
    );
    assert!(
        store::lock_path(&dir).ends_with("fleet/events.jsonl.lock"),
        "lock の path: {:?}",
        store::lock_path(&dir)
    );
}

#[test]
fn fleet_show_reports_missing_run_and_full_line() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let missing = run_fleet(&["show", "--run", "nope", "--state-dir", &path]);
    assert_eq!(missing.status.code(), Some(1), "無い便は rc 1");
    assert!(missing.stdout.is_empty(), "stdout へは書かない");
    assert_eq!(
        String::from_utf8_lossy(&missing.stderr).trim(),
        "fleet: no such run",
        "断りの行"
    );
    let recorded = run_fleet(&[
        "record", "--kind", "RunStage", "--run", "r1", "--bead", "b1", "--stage", "Landed",
        "--state-dir", &path,
    ]);
    assert!(recorded.status.success(), "record の rc: {recorded:?}");
    let shown = run_fleet(&["show", "--run", "r1", "--state-dir", &path]);
    let line = String::from_utf8_lossy(&shown.stdout).trim().to_owned();
    let fields: Vec<&str> = line.split(' ').collect();
    assert_eq!(fields.len(), 5, "5 つ組の 1 行: {line}");
    assert_eq!(fields.first().copied(), Some("run=r1"), "{line}");
    assert_eq!(fields.get(1).copied(), Some("bead=b1"), "{line}");
    assert_eq!(fields.get(2).copied(), Some("stage=Landed"), "{line}");
    assert_eq!(fields.get(3).copied(), Some("approved=false"), "{line}");
    assert!(fields.get(4).is_some_and(|f| f.starts_with("updated=")), "{line}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn fleet_external_form() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let usage = run_fleet(&[]);
    let missing = run_fleet(&["show", "--run", "nope", "--state-dir", &path]);
    let empty = run_fleet(&["export", "--state-dir", &path]);
    let recorded = run_fleet(&[
        "record", "--kind", "RunCreated", "--run", "r1", "--bead", "s2-x", "--state-dir", &path,
    ]);
    let fx = usage_fixture(&["a1"]);
    put_credential(&fx, "a1", &expired_credential("tok-old"));
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
    let claude = fake_claude(&fx, "exit 0");
    let refreshed = run_usage_with_claude(&fx, &curl, &claude);
    // 口座の口の外形（account-lifecycle.md §3）: 使い方・add の 1 行・断りの 1 行・ls の 1 行・retire の 1 行。
    let acct = state_dir();
    let acct_path = acct.display().to_string();
    let account_usage = run_account(&[]);
    let prepared = run_account(&["add", "a1", "--state-dir", &acct_path]);
    let exists = run_account(&["add", "a1", "--state-dir", &acct_path]);
    let listed = run_account(&["ls", "--state-dir", &acct_path]);
    let retired = run_account(&["retire", "a1", "--state-dir", &acct_path]);
    let form = format!(
        "{}{}{}{}{}{}{}{}{}{}",
        String::from_utf8_lossy(&usage.stderr),
        String::from_utf8_lossy(&missing.stderr),
        String::from_utf8_lossy(&empty.stdout),
        String::from_utf8_lossy(&recorded.stdout),
        String::from_utf8_lossy(&refreshed.stdout),
        String::from_utf8_lossy(&account_usage.stderr),
        String::from_utf8_lossy(&prepared.stdout),
        String::from_utf8_lossy(&exists.stderr),
        String::from_utf8_lossy(&listed.stdout),
        String::from_utf8_lossy(&retired.stdout)
    )
    .replace(&acct_path, "[state]")
    .replace(&vessel::fleet::cli::host(), "[host]");
    insta::assert_snapshot!(form);
    fs::remove_dir_all(&dir).ok();
    fs::remove_dir_all(&acct).ok();
    drop_fixture(&fx);
}

/// `StoreError` の表示が 1 件 1 行で行番号を持つ（読み側の断りの形）。
#[test]
fn fleet_store_error_shows_line_number() {
    let error = StoreError::Malformed {
        line: 7,
        reason: "壊れている".to_owned(),
    };
    assert_eq!(error.to_string(), "fleet: 壊れている line=7");
}

/// `STAGES` の並びが**宣言順**と一致する（ADR-0013 §2.2）。憲法 C2 が名指す「stage 列挙」は
/// この面であり、並びが宣言順から外れれば段の意味が静かにずれる。
#[test]
fn fleet_stages_follow_declaration_order() {
    assert!(
        is_declaration_order(STAGES, |stage| stage as usize),
        "STAGES の並びが宣言順と乖離している（母集団 {} 段）",
        STAGES.len()
    );
}

/// `RateLimited` は `Questioned` の直後に並び（母集団 11 段・`Reviewed` は `s2-07l.241` が `Intake` の直後に
/// 足した）、字面が往復する（`s2-07l.190`・設計 account-autonomy.md §2）。
#[test]
fn fleet_stages_place_rate_limited_after_questioned() {
    let at = |want: Stage| STAGES.iter().position(|stage| *stage == want);
    assert_eq!(STAGES.len(), 11, "段は 11 個");
    assert_eq!(
        at(Stage::RateLimited),
        at(Stage::Questioned).map(|found| found.saturating_add(1)),
        "RateLimited は Questioned の直後"
    );
    assert_eq!(Stage::RateLimited.as_str(), "RateLimited");
    assert_eq!(Stage::parse("RateLimited"), Some(Stage::RateLimited), "as_str ↔ parse の往復");
}

/// `KINDS` の並びが**宣言順**と一致する（ADR-0013 §2.2）。
#[test]
fn fleet_kinds_follow_declaration_order() {
    assert!(
        is_declaration_order(KINDS, |kind| kind as usize),
        "KINDS の並びが宣言順と乖離している（母集団 {} 種）",
        KINDS.len()
    );
}

/// 質問の段と 2 つの event は schema 1 のまま書けて読める（ADR-0004 §2.5・既存行の読みは
/// 変わらない）。
#[test]
fn pipe_question_kinds_round_trip_on_schema_1() {
    for (kind, stage) in [
        (EventKind::QuestionRaised, None),
        (EventKind::RunStage, Some(Stage::Questioned)),
        (EventKind::QuestionAnswered, None),
    ] {
        let event = Event {
            schema: SCHEMA,
            ts: "2026-09-12T00:00:00Z".to_owned(),
            kind,
            run: "r".to_owned(),
            bead: "b".to_owned(),
            host: "h".to_owned(),
            actor: kind.default_actor().to_owned(),
            stage,
            seat: None,
            pid: None,
            detail: Some("verify 行が矛盾する".to_owned()),
            allowance: None,
            registration: None,
            account: None,
        };
        let line = event.to_line();
        assert!(line.contains("\"schema\":1"), "{line}");
        assert_eq!(Event::from_line(&line), Ok(event.clone()), "{line}");
        assert_eq!(event.actor, "machine", "質問と回答は machine 由来");
    }
    let old = r#"{"schema":1,"ts":"2026-09-01T00:00:00Z","kind":"RunCreated","run":"r","bead":"b","host":"h","actor":"machine","stage":"Intake"}"#;
    assert!(Event::from_line(old).is_ok(), "既存の行はそのまま読める");
}

/// 口座残量の応答と同じ形（設計 fleet-usage.md §3）: 窓の object・`limits` の配列・
/// `scope.model.display_name` の 3 段・`id` は null の実測。
const USAGE_BODY: &str = r#"{
  "five_hour": {"utilization": 0.97, "resets_at": "2026-09-12T05:00:00Z"},
  "seven_day": {"utilization": 0.125, "resets_at": "2026-09-18T00:00:00Z"},
  "limits": [
    {"kind": "weekly_scoped", "id": null,
     "scope": {"model": {"display_name": "Opus 5", "id": null}},
     "utilization": 1.25, "resets_at": "2026-09-18T00:00:00Z"},
    {"kind": "weekly", "id": null, "utilization": 0.5, "resets_at": "2026-09-18T00:00:00Z"}
  ],
  "ok": true
}"#;

/// `depth` 段の配列だけの入れ子（深さの境界を測る fixture）。
fn nest(depth: usize) -> String {
    format!("{}{}", "[".repeat(depth), "]".repeat(depth))
}

/// 応答の形を `get` の連鎖で辿れる（3 段の `display_name` と各窓の `utilization` に届く）。
#[test]
fn fleet_json_tree_reads_the_usage_shape() {
    let tree = parse(USAGE_BODY).expect("応答の形を読める");
    let window = |name: &str| tree.get(name).and_then(|found| found.get("utilization"));
    assert_eq!(window("five_hour").and_then(Tree::as_pct), Some(97), "5 時間窓");
    assert_eq!(window("seven_day").and_then(Tree::as_pct), Some(12), "7 日窓は切り捨て");
    assert_eq!(
        tree.get("seven_day").and_then(|w| w.get("resets_at")).and_then(Tree::as_str),
        Some("2026-09-18T00:00:00Z"),
        "reset の字面"
    );
    let limits = tree.get("limits").and_then(Tree::as_array).expect("limits は配列");
    assert_eq!(limits.len(), 2, "配列の要素数: {limits:?}");
    let scoped = limits.first().expect("1 件目");
    assert_eq!(
        scoped
            .get("scope")
            .and_then(|scope| scope.get("model"))
            .and_then(|model| model.get("display_name"))
            .and_then(Tree::as_str),
        Some("Opus 5"),
        "3 段の入れ子を辿る"
    );
    assert_eq!(scoped.get("id"), Some(&Tree::Null), "id は null のまま持つ");
    assert_eq!(scoped.get("utilization").and_then(Tree::as_pct), Some(125), "1 超も cap しない");
    assert_eq!(tree.get("ok").and_then(Tree::as_bool), Some(true), "真偽");
    assert_eq!(tree.get("nope"), None, "無い key は None");
    assert_eq!(scoped.as_str(), None, "object は文字列ではない");
    assert_eq!(tree.get("limits").and_then(Tree::as_bool), None, "配列は真偽ではない");
}

/// `as_pct` は 100 で cap せず切り捨て、負数と `u64` を超える値は `None`。
///
/// 巨大な指数（`1e9999999999` 等）は**桁を作る前に** `None` へ落ちる＝指数に比例する仕事を
/// しない。run 1（commit edd15b7）はここで指数由来の幅の桁埋めを回して abort した。
#[test]
fn fleet_json_tree_as_pct_floors_without_cap_and_never_pads_by_exponent() {
    let started = Instant::now();
    let cases: [(&str, Option<u64>); 11] = [
        ("0.97", Some(97)),
        ("1.0", Some(100)),
        ("1.25", Some(125)),
        ("-0.1", None),
        ("1e9999999999", None),
        ("1e40", None),
        ("123e18", None),
        ("0.1e-9999999999", Some(0)),
        ("1e18", None),
        ("9.99e17", None),
        ("1.8e17", Some(18_000_000_000_000_000_000)),
    ];
    for (text, want) in cases {
        let tree = parse(text).expect("数の字面は読める");
        assert_eq!(tree.as_pct(), want, "入力 {text}");
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "None の経路が指数に比例する仕事をしている（{elapsed:?}）"
    );
    // u64 の端（`as_pct` の最大 = u64::MAX）の内と外。
    let inside = parse("184467440737095516.15").expect("端の内側の字面は読める");
    assert_eq!(inside.as_pct(), Some(u64::MAX), "×100 が u64::MAX ちょうど");
    let outside = parse("184467440737095516.16").expect("端の外側の字面も読める");
    assert_eq!(outside.as_pct(), None, "端を 1 越えたら None");
}

/// 壊れた字面を断らせ、理由が `want` の形であることまで見て返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn broken(text: &str, want: fn(&TreeError) -> bool) -> TreeError {
    let reason = parse(text).expect_err("壊れた字面は断る");
    assert!(want(&reason), "入力 {text} の理由: {reason}");
    reason
}

/// 理由が互いに別 variant であること（1 つに潰れていない）。
fn each_reason_differs(reasons: &[TreeError]) {
    let kinds: Vec<_> = reasons.iter().map(discriminant).collect();
    for (index, first) in kinds.iter().enumerate() {
        for second in kinds.iter().skip(index.saturating_add(1)) {
            assert_ne!(
                first, second,
                "{} 通りの壊れ方が同じ variant に潰れている: {reasons:?}",
                reasons.len()
            );
        }
    }
}

/// 壊れ方ごとに**別の variant** で断る（読めない字面は部分 parse を返さない）。
#[test]
fn fleet_json_tree_rejects_each_broken_shape_with_its_own_variant() {
    let reasons = [
        broken(r#"{"a":1,"a":2}"#, |found| matches!(found, TreeError::DuplicateKey { .. })),
        broken(r#"{"a":"x}"#, |found| matches!(found, TreeError::Unterminated { .. })),
        broken(r#"{"a":"\q"}"#, |found| matches!(found, TreeError::BadEscape { .. })),
        broken(r#"{"a":1} x"#, |found| matches!(found, TreeError::Trailing { .. })),
        broken(&nest(MAX_DEPTH.saturating_add(1)), |found| {
            matches!(found, TreeError::TooDeep { .. })
        }),
        broken("1e99999999999999999999", |found| {
            matches!(found, TreeError::BadNumber { .. })
        }),
    ];
    each_reason_differs(&reasons);
    assert!(parse(&nest(MAX_DEPTH)).is_ok(), "上限ちょうどは通る");
    let first = reasons.first().map(ToString::to_string).unwrap_or_default();
    assert!(first.contains("位置"), "断りは位置を持つ: {first}");
}

/// escape（`\uXXXX` と surrogate 対）を解き、壊れた対は拒む。
#[test]
fn fleet_json_tree_reads_escapes_and_surrogate_pairs() {
    let text = parse(r#""A😀\n\t\"\\\/あ""#).expect("escape を解ける");
    assert_eq!(text.as_str(), Some("A\u{1f600}\n\t\"\\/あ"), "解いた中身");
    let escaped = "\"\\uD83D\\uDE00\\u3042\\u0041\"";
    let pair = parse(escaped).expect("surrogate 対を解ける");
    assert_eq!(pair.as_str(), Some("\u{1f600}あA"), "対は 1 文字へ・BMP の \\uXXXX はそのまま");
    for broken in [r#""\uD83D""#, r#""\uD83Dx""#, r#""\uDE00""#, r#""\u00Z1""#, r#""\u12""#] {
        let reason = parse(broken).expect_err("壊れた escape は拒む");
        assert!(
            matches!(reason, TreeError::BadEscape { .. }),
            "入力 {broken} の理由: {reason}"
        );
    }
}

/// flat 行の reader は**入れ子を拒み続ける**（広げたのは新しい型の側だけ）。
#[test]
fn fleet_json_tree_leaves_the_flat_reader_narrow() {
    for nested in [r#"{"a":{"b":1}}"#, r#"{"a":[1]}"#, r#"{"a":1.5}"#, r#"{"a":-1}"#] {
        let reason = json_lite::parse_object(nested).expect_err("flat の reader は受けない");
        assert!(!reason.is_empty(), "断りの理由が空: {nested}");
        assert!(parse(nested).is_ok(), "同じ字面を Tree は読める: {nested}");
    }
    let flat = r#"{"a":"x","b":1,"c":true,"d":null}"#;
    assert!(json_lite::parse_object(flat).is_ok(), "1 段の行はこれまで通り読める");
}

/// 口座残量の行に使う時刻。
const ALLOWANCE_TS: &str = "2026-09-12T02:00:00Z";

/// 窓が開き直る時刻。
const RESETS_AT: &str = "2026-09-12T05:00:00Z";

/// 聞き先の短い識別子。
const ENDPOINT: &str = "usage";

/// 実測 1 件。
fn measured(account: &str, window: WindowKind, model: Option<&str>, used_pct: u64) -> Allowance {
    Allowance::Measured(Measured {
        account: account.to_owned(),
        window,
        model: model.map(str::to_owned),
        endpoint: ENDPOINT.to_owned(),
        used_pct,
        resets_at: Some(RESETS_AT.to_owned()),
    })
}

/// 測れなかった 1 件。
fn unmeasured(account: &str, window: Option<WindowKind>, reason: UnmeasuredReason) -> Allowance {
    Allowance::Unmeasured(Unmeasured {
        account: account.to_owned(),
        window,
        model: None,
        endpoint: ENDPOINT.to_owned(),
        reason,
    })
}

/// 口座残量の event を組む。**kind は本体から導く**（食い違った組を fixture にしない）。
fn allowance_event(ts: &str, allowance: Allowance) -> Event {
    let kind = match &allowance {
        Allowance::Measured(_) => EventKind::AllowanceMeasured,
        Allowance::Unmeasured(_) => EventKind::AllowanceUnmeasured,
    };
    Event {
        schema: SCHEMA,
        ts: ts.to_owned(),
        kind,
        run: String::new(),
        bead: String::new(),
        host: "h".to_owned(),
        actor: kind.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: None,
        allowance: Some(allowance),
        registration: None,
        account: None,
    }
}

/// 最新を引く key。
fn allowance_key(account: &str, window: Option<WindowKind>, model: Option<&str>) -> AllowanceKey {
    AllowanceKey {
        account: account.to_owned(),
        window,
        model: model.map(str::to_owned),
    }
}

/// 口座残量の生の行を key/value から組む（malformed の fixture 用）。
///
/// `extra` の key は base の 7 key と衝突させない——重複 key は `json_lite` が**別の理由**で
/// 断るので、測りたい欠陥と返る理由が入れ替わる。
fn allowance_line(kind: &str, extra: &[(&str, json_lite::Value)]) -> String {
    let mut pairs: Vec<(&str, json_lite::Value)> = vec![
        ("schema", json_lite::Value::Num(SCHEMA)),
        ("ts", json_lite::Value::Str(ALLOWANCE_TS.to_owned())),
        ("kind", json_lite::Value::Str(kind.to_owned())),
        ("account", json_lite::Value::Str("a1".to_owned())),
        ("endpoint", json_lite::Value::Str(ENDPOINT.to_owned())),
        ("host", json_lite::Value::Str("h".to_owned())),
        ("actor", json_lite::Value::Str("machine".to_owned())),
    ];
    pairs.extend(extra.iter().cloned());
    json_lite::write_object(&pairs)
}

/// 2 行目に置いた fixture が `line=2` の malformed になり、理由が `want` を含むこと。
///
/// 1 行目に**読める実測**を置くのは、`read_all` が Err になったのが 2 行目だけのせいだと
/// 言えるようにするためである（件数 1 まで測る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn refuses_second_line(forged: &str, want: &str) {
    let dir = state_dir();
    let good =
        allowance_event(ALLOWANCE_TS, measured("a1", WindowKind::FiveHour, None, 97)).to_line();
    write_raw(&dir, &[&good, forged]);
    let errors = store::read_all(&dir).expect_err("読めない行を Ok で通さない");
    let joined: Vec<String> = errors.iter().map(ToString::to_string).collect();
    let text = joined.join("\n");
    assert_eq!(errors.len(), 1, "件数（1 行目は読める）: {text}");
    assert!(text.contains("line=2"), "行番号: {text}");
    assert!(text.contains(want), "理由に {want} が無い: {text}（入力 {forged}）");
    fs::remove_dir_all(&dir).ok();
}

/// 行を順に追記して読み戻す（`append` → `read_all` の往復を通す）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn append_allowance(dir: &Path, rows: Vec<(&str, Allowance)>) -> Vec<Event> {
    let policy = LockPolicy::embedded().expect("rules 行を引ける");
    for (ts, allowance) in rows {
        store::append(dir, &allowance_event(ts, allowance), policy).expect("追記できる");
    }
    store::read_all(dir).expect("全行 parse できる")
}

/// 口座 × 窓 × model ごとの最新が入り、便と席は 1 件も増えない（歯 (a)(1)）。
#[test]
fn fleet_allowance_replay_keeps_latest_per_account_and_window() {
    let dir = state_dir();
    let model_ts = "2026-09-12T02:00:02Z";
    let events = append_allowance(
        &dir,
        vec![
            (ALLOWANCE_TS, measured("a1", WindowKind::FiveHour, None, 97)),
            ("2026-09-12T02:00:01Z", measured("a1", WindowKind::SevenDay, None, 12)),
            (model_ts, measured("a1", WindowKind::SevenDayModel, Some("Opus 5"), 125)),
            ("2026-09-12T02:00:03Z", unmeasured("a2", None, UnmeasuredReason::NoCredentials)),
        ],
    );
    assert_eq!(events.len(), 4, "4 行");
    let state = replay(&events);
    assert_eq!(
        state.allowance.len(),
        4,
        "口座 × 窓 × model ごとに 1 件: {:?}",
        state.allowance
    );
    assert!(
        state.runs.is_empty(),
        "allowance 行は便を作らない: {:?}",
        state.runs
    );
    assert!(
        state.seats.is_empty(),
        "allowance 行は席を作らない: {:?}",
        state.seats
    );
    let key = allowance_key("a1", Some(WindowKind::SevenDayModel), Some("Opus 5"));
    let model = state.allowance.get(&key).expect("モデル別の窓が在る");
    assert_eq!(
        model.allowance,
        measured("a1", WindowKind::SevenDayModel, Some("Opus 5"), 125),
        "実測の中身（100 で cap しない）"
    );
    assert_eq!(model.ts, model_ts, "行の時刻");
    let other = state
        .allowance
        .get(&allowance_key("a2", None, None))
        .expect("測れなかった口座も 1 枠を持つ");
    assert_eq!(
        other.allowance,
        unmeasured("a2", None, UnmeasuredReason::NoCredentials),
        "理由つきで残る（0 に読み替えない）"
    );
    fs::remove_dir_all(&dir).ok();
}

/// 同じ枠は Measured → Unmeasured の順で書くと最新が Unmeasured（歯 (a)(2)）。
#[test]
fn fleet_allowance_unmeasured_replaces_the_older_measured() {
    let first = allowance_event(ALLOWANCE_TS, measured("a1", WindowKind::FiveHour, None, 42));
    let later = allowance_event(
        "2026-09-12T03:00:00Z",
        unmeasured("a1", Some(WindowKind::FiveHour), UnmeasuredReason::HttpStatus),
    );
    let state = replay(&[first, later]);
    assert_eq!(state.allowance.len(), 1, "同じ枠は 1 件のまま");
    let latest = state
        .allowance
        .get(&allowance_key("a1", Some(WindowKind::FiveHour), None))
        .expect("5 時間窓が在る");
    assert_eq!(latest.ts, "2026-09-12T03:00:00Z", "物理順で後の行が勝つ");
    assert_eq!(
        latest.allowance,
        unmeasured("a1", Some(WindowKind::FiveHour), UnmeasuredReason::HttpStatus),
        "古い実測値で「最新」を覆わない"
    );
}

/// 口座残量の行は `run` / `bead` を持たず、書いて読むと同じ event に戻る。
#[test]
fn fleet_allowance_line_carries_no_run_or_bead() {
    let event = allowance_event(
        ALLOWANCE_TS,
        measured("a1", WindowKind::SevenDayModel, Some("Opus 5"), 125),
    );
    let line = event.to_line();
    let pairs = json_lite::parse_object(&line).expect("flat JSON である");
    let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "schema",
            "ts",
            "kind",
            "account",
            "window",
            "model",
            "endpoint",
            "used_pct",
            "resets_at",
            "host",
            "actor"
        ],
        "行の key の並び: {line}"
    );
    assert_eq!(Event::from_line(&line), Ok(event), "書いて読むと同じ: {line}");
    // 必須 field が揃っていても、便 id を足した行は読まない（`run` の有無が kind から
    // 読めなくなる形を通さない）。
    let with_run = allowance_line(
        "AllowanceMeasured",
        &[
            ("window", json_lite::Value::Str("five_hour".to_owned())),
            ("used_pct", json_lite::Value::Num(97)),
            ("resets_at", json_lite::Value::Str(RESETS_AT.to_owned())),
            ("run", json_lite::Value::Str("r1".to_owned())),
        ],
    );
    let reason = Event::from_line(&with_run).expect_err("run を持つ口座の行は読まない");
    assert!(reason.contains("run を持たない"), "理由: {reason}");
}

/// `AllowanceUnmeasured` が持てない field と、型・字面の違う field
/// （歯 (a)(3)・改訂 (11)(13)）。
#[test]
fn fleet_allowance_unmeasured_rejects_used_pct_and_wrong_types() {
    let reason = ("reason", json_lite::Value::Str("http_status".to_owned()));
    let cases: Vec<(Vec<(&str, json_lite::Value)>, &str)> = vec![
        (
            vec![reason.clone(), ("used_pct", json_lite::Value::Num(0))],
            "used_pct を持たない",
        ),
        (
            vec![
                reason.clone(),
                ("resets_at", json_lite::Value::Str(RESETS_AT.to_owned())),
            ],
            "resets_at を持たない",
        ),
        (
            vec![("reason", json_lite::Value::Num(123))],
            "reason が無いか文字列でない",
        ),
        (
            vec![(
                "reason",
                json_lite::Value::Str("unknown-reason".to_owned()),
            )],
            "reason unknown-reason は未知である",
        ),
        (
            vec![
                reason.clone(),
                ("window", json_lite::Value::Str("monthly".to_owned())),
            ],
            "window monthly は未知である",
        ),
        (
            vec![reason, ("model", json_lite::Value::Num(5))],
            "model が文字列でない",
        ),
        (vec![], "reason が無いか文字列でない"),
    ];
    for (extra, want) in cases {
        refuses_second_line(&allowance_line("AllowanceUnmeasured", &extra), want);
    }
}

/// `AllowanceMeasured` の必須 field は key の有無と**値の型**の両方で見る
/// （歯 (a)(5)・改訂 (12)(13)）。
#[test]
fn fleet_allowance_measured_rejects_missing_model_and_wrong_types() {
    let window = |text: &str| ("window", json_lite::Value::Str(text.to_owned()));
    let pct = ("used_pct", json_lite::Value::Num(97));
    let resets = ("resets_at", json_lite::Value::Str(RESETS_AT.to_owned()));
    let cases: Vec<(Vec<(&str, json_lite::Value)>, &str)> = vec![
        (
            vec![window("seven_day_model"), pct.clone(), resets.clone()],
            "model が無い",
        ),
        (
            vec![window("monthly"), pct.clone(), resets.clone()],
            "window monthly は未知である",
        ),
        (
            vec![pct.clone(), resets.clone()],
            "window が無いか文字列でない",
        ),
        (
            vec![
                window("five_hour"),
                ("used_pct", json_lite::Value::Str("50".to_owned())),
                resets.clone(),
            ],
            "used_pct が無いか整数でない",
        ),
        (
            vec![
                window("five_hour"),
                pct.clone(),
                ("resets_at", json_lite::Value::Num(5)),
            ],
            "resets_at が文字列でない",
        ),
        (
            vec![window("five_hour"), resets.clone()],
            "used_pct が無いか整数でない",
        ),
        (
            vec![
                window("five_hour"),
                pct,
                resets,
                ("reason", json_lite::Value::Str("timeout".to_owned())),
            ],
            "reason を持たない",
        ),
    ];
    for (extra, want) in cases {
        refuses_second_line(&allowance_line("AllowanceMeasured", &extra), want);
    }
}

/// 型不一致の行が 1 本混ざった log は `read_all` が Err で、現在地を作れない
/// ＝**古い実測が「最新」を名乗らない**（改訂 (14)）。
#[test]
fn fleet_allowance_read_refuses_the_whole_log_on_a_type_mismatch() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let good =
        allowance_event(ALLOWANCE_TS, measured("a1", WindowKind::FiveHour, None, 97)).to_line();
    let broken = allowance_line(
        "AllowanceUnmeasured",
        &[
            ("window", json_lite::Value::Str("five_hour".to_owned())),
            ("reason", json_lite::Value::Num(123)),
        ],
    );
    write_raw(&dir, &[&good, &broken]);
    let errors = store::read_all(&dir).expect_err("型不一致の行は Ok で通らない");
    let text = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<String>>()
        .join("\n");
    assert!(text.contains("line=2"), "行番号つきで断る: {text}");
    let args = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<String>>();
    let outcome = vessel::fleet::cli::dispatch(&args(&["export", "--state-dir", &path]));
    assert_eq!(
        outcome.rc, RC_BROKEN,
        "読めない log からは現在地を出さない: {outcome:?}"
    );
    assert!(outcome.out.is_empty(), "古い実測を 1 行も名乗らない");
    fs::remove_dir_all(&dir).ok();
}

/// 既存 kind の行は `run` / `bead` が必須のままで、口座の field を持てない（歯 (a)(4)）。
#[test]
fn fleet_allowance_leaves_existing_kinds_requiring_run_and_bead() {
    let full: Vec<(&str, json_lite::Value)> = vec![
        ("schema", json_lite::Value::Num(SCHEMA)),
        ("ts", json_lite::Value::Str(ALLOWANCE_TS.to_owned())),
        ("kind", json_lite::Value::Str("RunCreated".to_owned())),
        ("run", json_lite::Value::Str("r1".to_owned())),
        ("bead", json_lite::Value::Str("b1".to_owned())),
        ("host", json_lite::Value::Str("h".to_owned())),
        ("actor", json_lite::Value::Str("machine".to_owned())),
    ];
    assert!(
        Event::from_line(&json_lite::write_object(&full)).is_ok(),
        "揃った行はこれまで通り読める"
    );
    for dropped in ["run", "bead"] {
        let kept: Vec<(&str, json_lite::Value)> = full
            .iter()
            .filter(|(key, _)| *key != dropped)
            .cloned()
            .collect();
        let reason =
            Event::from_line(&json_lite::write_object(&kept)).expect_err("欠落は malformed のまま");
        assert!(reason.contains(dropped), "理由: {reason}");
    }
    let mut with_account = full;
    with_account.push(("account", json_lite::Value::Str("a1".to_owned())));
    let reason = Event::from_line(&json_lite::write_object(&with_account))
        .expect_err("既存 kind は口座の field を持てない");
    assert!(reason.contains("account を持たない"), "理由: {reason}");
}

/// `UnmeasuredReason` の 10 variant は round-trip し、極性は fail-open
/// （**guard は足していない**・歯 (a)(6)）。
#[test]
fn fleet_allowance_reasons_round_trip_and_polarity_is_fail_open() {
    assert_eq!(REASONS.len(), 10, "母集団");
    for reason in REASONS {
        assert_eq!(
            UnmeasuredReason::parse(reason.as_str()),
            Some(*reason),
            "{}",
            reason.as_str()
        );
    }
    assert!(
        is_declaration_order(REASONS, |reason| reason as usize),
        "REASONS の並びが宣言順と乖離している（母集団 {} 種）",
        REASONS.len()
    );
    assert_eq!(
        UnmeasuredReason::parse("ShapeMismatch"),
        None,
        "variant 名の字面は受けない（行に書くのは snake_case）"
    );
    assert_eq!(UnmeasuredReason::parse(""), None, "空は理由ではない");
    let polarity: Polarity = UnmeasuredReason::POLARITY;
    assert_eq!(
        polarity.on_failure,
        OnFailure::FailOpen,
        "測れない周は行にして続ける"
    );
    assert_eq!(polarity.timing, Timing::InLoop, "読む時点で理由が決まる");
    let listed = vessel::polarity::ALL
        .iter()
        .filter(|guard| guard.boundary().contains("Unmeasured"))
        .count();
    assert_eq!(
        listed, 0,
        "計測は行為を止めない＝guard を足していない（母集団 {} 件）",
        vessel::polarity::ALL.len()
    );
}

/// 窓の字面は snake_case の 3 つで、並びは宣言順。
#[test]
fn fleet_allowance_windows_round_trip_on_snake_case() {
    assert_eq!(WINDOWS.len(), 3, "母集団");
    for window in WINDOWS {
        assert_eq!(
            WindowKind::parse(window.as_str()),
            Some(*window),
            "{}",
            window.as_str()
        );
    }
    assert!(
        is_declaration_order(WINDOWS, |window| window as usize),
        "WINDOWS の並びが宣言順と乖離している（母集団 {} 種）",
        WINDOWS.len()
    );
    assert_eq!(WindowKind::parse("FiveHour"), None, "variant 名は字面でない");
}

/// (6e) `KINDS` は 15 variant で並びは宣言順のまま（席の登録 1 の後ろに口座の退役・戻しの 2 を末尾に足した・
/// account-lifecycle.md §3）。base は 13 で落ちる（RED）。
#[test]
fn account_cmd_kinds_are_fifteen_with_retire_and_restore_last() {
    assert_eq!(KINDS.len(), 15, "母集団（既存 10 + 口座残量 2 + 席の登録 1 + 口座の退役・戻し 2）");
    assert_eq!(
        KINDS.get(12..),
        Some(&[EventKind::SeatRegistered, EventKind::AccountRetired, EventKind::AccountRestored][..]),
        "登録 → 退役 → 戻しが末尾の順"
    );
    for kind in [EventKind::SeatRegistered, EventKind::AccountRetired, EventKind::AccountRestored] {
        assert!(!kind.is_allowance(), "{} は口座残量の kind ではない", kind.as_str());
        assert_eq!(EventKind::parse(kind.as_str()), Some(kind), "{}", kind.as_str());
        assert_eq!(kind.default_actor(), "machine", "{} は機械由来", kind.as_str());
    }
    assert!(
        is_declaration_order(KINDS, |kind| kind as usize),
        "KINDS の並びが宣言順と乖離している（母集団 {} 種）",
        KINDS.len()
    );
    assert_eq!(
        KINDS.iter().filter(|kind| kind.is_allowance()).count(),
        2,
        "便に紐づかない kind は 2 つだけ"
    );
    for kind in [EventKind::AllowanceMeasured, EventKind::AllowanceUnmeasured] {
        assert_eq!(EventKind::parse(kind.as_str()), Some(kind), "{}", kind.as_str());
        assert_eq!(kind.default_actor(), "machine", "計測は機械由来（FR22 不変）");
    }
}

/// 席の登録の event（本体は `registration` の束・`model` は任意なので `Some` の row を組む）。
fn registration_event(target: &str) -> Event {
    registration_event_with_model(target, Some("Fable"))
}

/// 席の登録の event（`model` を選ぶ・`None` の row は key ごと無い旧 row と同じ形）。
fn registration_event_with_model(target: &str, model: Option<&str>) -> Event {
    Event {
        schema: SCHEMA,
        ts: ALLOWANCE_TS.to_owned(),
        kind: EventKind::SeatRegistered,
        run: String::new(),
        bead: String::new(),
        host: "h".to_owned(),
        actor: EventKind::SeatRegistered.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: None,
        allowance: None,
        registration: Some(Registration {
            role: Role::Admin,
            anchor: "/repo".to_owned(),
            target: target.to_owned(),
            sid: Some("sid-1".to_owned()),
            account: "a1".to_owned(),
            launch: "line 1\n\"line 2\"\n".to_owned(),
            model: model.map(str::to_owned),
        }),
        account: None,
    }
}

/// 登録 row の `model`（契約 (e)）: `Some` は `"model":"…"` の key で書き読みが戻り、`None` は key ごと書かず
/// 旧 row（`model` の無い行）は `None` で読める（schema 1 のまま値の追加）。key が在って文字列でなければ malformed。
#[test]
fn fleet_seat_registration_row_carries_optional_model_and_old_rows_read_as_none() {
    let with = registration_event_with_model("s:w", Some("Fable"));
    let line = with.to_line();
    assert!(line.contains("\"model\":\"Fable\""), "{line}");
    assert!(line.contains("\"schema\":1"), "schema 1 のまま: {line}");
    assert_eq!(Event::from_line(&line), Ok(with.clone()), "{line}");
    let without = registration_event_with_model("s:w", None);
    let old = without.to_line();
    assert!(!old.contains("\"model\""), "None は key ごと書かない: {old}");
    assert_eq!(Event::from_line(&old), Ok(without.clone()), "{old}");
    let stripped = line.replacen(",\"model\":\"Fable\"", "", 1);
    assert_ne!(stripped, line, "置換が効く");
    assert_eq!(Event::from_line(&stripped).ok().and_then(|found| found.registration).and_then(|row| row.model), None, "旧 row は None");
    let typed = line.replacen("\"model\":\"Fable\"", "\"model\":7", 1);
    assert!(Event::from_line(&typed).is_err(), "文字列でない model は malformed: {typed}");
    let state = replay(&[without, with]);
    let models: Vec<Option<String>> = state.registrations.values().map(|latest| latest.registration.model.clone()).collect();
    assert_eq!(models, vec![Some("Fable".to_owned())], "同じ鍵の再登録は model も最新に置き換わる");
}

/// `SeatRegistered` の行は `to_line` → `from_line` で戻り、`run` / `bead` と口座残量だけの key を持たない。
/// 登録の key を持つ `RunStage` の行・口座残量の行は malformed・未知の role も malformed（歯 (b)）。
#[test]
fn fleet_seat_role_registration_row_round_trips_and_its_keys_stay_exclusive() {
    let event = registration_event("s:w");
    let line = event.to_line();
    assert_eq!(Event::from_line(&line), Ok(event), "{line}");
    assert!(!line.contains("\"run\":") && !line.contains("\"bead\":"), "{line}");
    assert!(line.contains("\"schema\":1"), "schema 1 のまま: {line}");
    let run_stage: Vec<(&str, json_lite::Value)> = vec![
        ("schema", json_lite::Value::Num(SCHEMA)),
        ("ts", json_lite::Value::Str(ALLOWANCE_TS.to_owned())),
        ("kind", json_lite::Value::Str("RunStage".to_owned())),
        ("run", json_lite::Value::Str("r1".to_owned())),
        ("bead", json_lite::Value::Str("b1".to_owned())),
        ("host", json_lite::Value::Str("h".to_owned())),
        ("actor", json_lite::Value::Str("machine".to_owned())),
    ];
    assert!(Event::from_line(&json_lite::write_object(&run_stage)).is_ok(), "揃った RunStage は読める");
    for key in ["role", "anchor", "target", "sid", "launch"] {
        let mut forged = run_stage.clone();
        forged.push((key, json_lite::Value::Str("x".to_owned())));
        let reason = Event::from_line(&json_lite::write_object(&forged)).expect_err("登録の key を持つ RunStage は malformed");
        assert!(reason.contains(&format!("{key} を持たない")), "{key}: {reason}");
        let allowance = allowance_line("AllowanceUnmeasured", &[("reason", json_lite::Value::Str("timeout".to_owned())), (key, json_lite::Value::Str("x".to_owned()))]);
        let reason = Event::from_line(&allowance).expect_err("登録の key を持つ口座残量の行は malformed");
        assert!(reason.contains(&format!("{key} を持たない")), "{key}: {reason}");
    }
    let with_run = line.replacen("\"kind\":\"SeatRegistered\"", "\"kind\":\"SeatRegistered\",\"run\":\"r1\"", 1);
    assert!(Event::from_line(&with_run).is_err(), "登録の行は run を持たない: {with_run}");
    let with_window = line.replacen("\"kind\":\"SeatRegistered\"", "\"kind\":\"SeatRegistered\",\"window\":\"five_hour\"", 1);
    assert!(Event::from_line(&with_window).is_err(), "登録の行は口座残量だけの key を持たない: {with_window}");
    let unknown = line.replacen("\"role\":\"admin\"", "\"role\":\"Admin\"", 1);
    assert!(Event::from_line(&unknown).is_err(), "未知の role は malformed: {unknown}");
    let missing = line.replacen(",\"launch\":\"line 1\\n\\\"line 2\\\"\\n\"", "", 1);
    assert_ne!(missing, line, "置換が効く");
    assert!(Event::from_line(&missing).is_err(), "項目の欠けは malformed: {missing}");
}

/// 登録 row の `sid` は任意（account-lifecycle.md §4・`s2-07l.244`）: `Some` は `"sid":"…"` の key で書き読みが戻り、
/// `None`（`seat launch` が書く row）は key ごと書かず、key の省略も `null` も `None` で読める（schema 1 のまま）。
/// key が在って文字列でも `null` でもなければ malformed。`sid` 有りの row と無しの row は同じ log で両方 replay できる
/// （契約 (h)）。
#[test]
fn seat_launch_registration_sid_is_optional_and_both_forms_replay() {
    let with = registration_event("s:w");
    let line = with.to_line();
    assert!(line.contains("\"sid\":\"sid-1\""), "{line}");
    let mut launched = registration_event("s:launched");
    launched.registration = launched.registration.map(|row| Registration { sid: None, role: Role::Planner, ..row });
    let bare = launched.to_line();
    assert!(!bare.contains("\"sid\""), "None は key ごと書かない（null を出さない）: {bare}");
    assert!(bare.contains("\"schema\":1"), "schema 1 のまま: {bare}");
    assert_eq!(Event::from_line(&bare), Ok(launched.clone()), "{bare}");
    let nulled = bare.replacen("\"target\":\"s:launched\"", "\"target\":\"s:launched\",\"sid\":null", 1);
    assert_ne!(nulled, bare, "置換が効く");
    assert_eq!(Event::from_line(&nulled), Ok(launched.clone()), "null も None で読む: {nulled}");
    let typed = line.replacen("\"sid\":\"sid-1\"", "\"sid\":7", 1);
    assert!(Event::from_line(&typed).is_err(), "文字列でも null でもない sid は malformed: {typed}");
    let state = replay(&[with.clone(), launched.clone()]);
    let sids: Vec<Option<String>> = state.registrations.values().map(|latest| latest.registration.sid.clone()).collect();
    assert_eq!(sids, vec![None, Some("sid-1".to_owned())], "鍵の違う 2 row（planner / admin の鍵順）が両方 replay に載る");
}

/// 登録の行は便も席も作らず `export` を変えず、`fleet record` からは書けない（書き手は `seat register`）。
#[test]
fn fleet_seat_role_registration_rows_do_not_touch_runs_and_record_refuses_the_kind() {
    let state = replay(&[event(EventKind::RunCreated, "r1", ALLOWANCE_TS), registration_event("s:w")]);
    assert_eq!(state.runs.len(), 1, "幽霊の便を作らない");
    assert_eq!(state.seats.len(), 0, "席の現在地にも載らない");
    assert_eq!(state.registrations.len(), 1, "登録の現在地に載る");
    let dir = state_dir();
    let path = dir.display().to_string();
    let out = run_fleet(&["record", "--kind", "SeatRegistered", "--run", "r1", "--bead", "b1", "--state-dir", &path]);
    assert_eq!(out.status.code(), Some(1), "書き側で断る: {out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("record では書けない"));
    assert!(!store::events_path(&dir).exists(), "行を残さない");
    fs::remove_dir_all(&dir).ok();
}

/// allowance の行が在っても `export`（跨版 面 2）は 1 byte も変わらない（歯 (a)(8)）。
#[test]
fn fleet_allowance_rows_do_not_change_export() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let calls: [&[&str]; 2] = [
        &["record", "--kind", "RunCreated", "--run", "r1", "--bead", "b1", "--stage", "Gated"],
        &["record", "--kind", "SeatSpawned", "--run", "r1", "--bead", "b1", "--seat", "s1", "--pid", "7"],
    ];
    for call in calls {
        let mut args = call.to_vec();
        args.extend_from_slice(&["--state-dir", &path]);
        assert!(run_fleet(&args).status.success(), "record: {call:?}");
    }
    let before = run_fleet(&["export", "--state-dir", &path]);
    assert!(before.status.success(), "export の rc: {before:?}");
    let policy = LockPolicy::embedded().expect("rules 行を引ける");
    let rows = [
        measured("a1", WindowKind::FiveHour, None, 97),
        unmeasured("a2", None, UnmeasuredReason::NoCredentials),
    ];
    for allowance in rows {
        store::append(&dir, &allowance_event(ALLOWANCE_TS, allowance), policy)
            .expect("追記できる");
    }
    let after = run_fleet(&["export", "--state-dir", &path]);
    assert!(after.status.success(), "export の rc: {after:?}");
    assert_eq!(
        String::from_utf8_lossy(&before.stdout),
        String::from_utf8_lossy(&after.stdout),
        "header の件数も run 行も seat 行も不変"
    );
    let lines = String::from_utf8_lossy(&after.stdout).lines().count();
    assert_eq!(lines, 3, "header + run 1 + seat 1");
    fs::remove_dir_all(&dir).ok();
}

/// `fleet record` は口座残量の kind を書けない（書き手は `fleet usage` の 1 本・改訂 (15)）。
#[test]
fn fleet_allowance_record_refuses_the_kind() {
    let dir = state_dir();
    let path = dir.display().to_string();
    for kind in ["AllowanceMeasured", "AllowanceUnmeasured"] {
        let out = run_fleet(&[
            "record", "--kind", kind, "--run", "r1", "--bead", "b1", "--state-dir", &path,
        ]);
        assert_eq!(out.status.code(), Some(1), "書き側で断る: {out:?}");
        assert!(out.stdout.is_empty(), "stdout へは書かない");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("record では書けない"), "理由: {err}");
    }
    assert!(
        !store::events_path(&dir).exists(),
        "必須 field の揃わない行を append-only の log に残さない"
    );
    fs::remove_dir_all(&dir).ok();
}

/// 極性は fail-closed で、**極性一覧の guard には載らない**（計測は行為を止めない・設計 §6）。
#[test]
fn fleet_json_tree_polarity_is_fail_closed_outside_the_guard_list() {
    let polarity: Polarity = json_tree::POLARITY;
    assert_eq!(polarity.on_failure, OnFailure::FailClosed, "読めない字面は Err へ倒す");
    assert_eq!(polarity.timing, Timing::InLoop, "読む時点で断る");
    let listed = vessel::polarity::ALL
        .iter()
        .filter(|guard| guard.boundary().contains("json_tree"))
        .count();
    assert_eq!(
        listed, 0,
        "guard を足していない（母集団 {} 件）",
        vessel::polarity::ALL.len()
    );
}

/// `fleet usage` の歯の rules fixture が名乗る待ち時間（埋め込みの値と違う数にして出所を測る）。
const USAGE_TIMEOUT_S: u64 = 13;

/// 期限の遠い credential の `expiresAt`（2100-01-01 の epoch ms）。
const FAR_EXPIRES_MS: u64 = 4_102_444_800_000;

/// 口座 a1 / a2 の fixture token（不在を数える字面・実在の token ではない）。
const TOKEN_A1: &str = "tok-a1-7f3c9e0d";
const TOKEN_A2: &str = "tok-a2-b81d04aa";

/// 実測の応答と同じ形の本文（設計 §3）: 窓の `utilization` は**すでに % の値**・`limits[]` の要素は
/// `utilization` を持たず `percent`（整数）が値・reset は `+00:00` 形と `Z` 形が混ざる。値は架空。
const LIVE_BODY: &str = r#"{
  "five_hour": {"utilization": 13.0, "resets_at": "2026-09-12T05:00:00.412000+00:00"},
  "seven_day": {"utilization": 41.7, "resets_at": "2026-09-18T00:00:00+00:00"},
  "limits": [
    {"kind": "weekly_scoped", "group": "g", "percent": 38, "severity": "normal",
     "resets_at": "2026-09-18T00:00:00Z",
     "scope": {"model": {"display_name": "Fable", "id": null}}, "is_active": true},
    {"kind": "weekly", "group": "g", "percent": 50, "severity": "normal",
     "resets_at": "2026-09-18T00:00:00Z", "is_active": true}
  ]
}"#;

/// [`LIVE_BODY`] を読んだ口座の 1 行（`label` の口座）。
fn live_line(label: &str) -> String {
    format!(
        "usage: account={label} five_hour=13% resets=2026-09-12T05:00:00Z seven_day=41% resets=2026-09-18T00:00:00Z model=Fable:38% resets=2026-09-18T00:00:00Z"
    )
}

/// `fleet usage` の歯の置き場。
struct UsageFixture {
    /// `--state-dir`。
    state: PathBuf,
    /// 偽 curl の置き場と、偽 curl が残す写し（`args` / `stdin`）。
    spy: PathBuf,
    /// `--rules` の fixture。
    rules: PathBuf,
}

/// `[[account]]` を `labels` の順で持ち、`fleet.usage_timeout_s` を 1 行持つ rules fixture。
fn usage_rules(labels: &[&str]) -> String {
    let mut text = format!(
        "schema = 1\n\n[[rule]]\nid = \"fleet.usage_timeout_s\"\nkind = \"UsageTimeoutS\"\nvalue = {USAGE_TIMEOUT_S}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n"
    );
    for label in labels {
        text.push_str(&format!("\n[[account]]\nlabel = \"{label}\"\n"));
    }
    text
}

/// 期限の遠い、読める credential の本文。
fn live_credential(token: &str) -> String {
    format!(
        r#"{{"claudeAiOauth":{{"accessToken":"{token}","refreshToken":"r-not-read","expiresAt":{FAR_EXPIRES_MS},"scopes":["user:inference"]}},"other":1}}"#
    )
}

/// 置き場を作り、rules fixture を書く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn usage_fixture(labels: &[&str]) -> UsageFixture {
    let state = state_dir();
    let spy = state_dir();
    let rules = spy.join("rules.toml");
    fs::write(&rules, usage_rules(labels)).expect("rules fixture を書ける");
    UsageFixture { state, spy, rules }
}

/// `<state>/accounts/<label>/.credentials.json` に `text` を置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_credential(fx: &UsageFixture, label: &str, text: &str) {
    let dir = fx.state.join("accounts").join(label);
    fs::create_dir_all(&dir).expect("credential の dir を作れる");
    fs::write(dir.join(".credentials.json"), text).expect("credential を書ける");
}

/// 偽 curl（headless の歯の `fake_claude` と同じ型）。
///
/// argv を 1 行 1 引数で `args` へ、stdin を `stdin` へ**追記**で写し（口座ごとに 1 回呼ばれる）、
/// `body` と `\n<status>` を stdout へ出して `rc` で終わる。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_curl(fx: &UsageFixture, body: &str, status: &str, rc: u8) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let d = fx.spy.display().to_string();
    fs::write(fx.spy.join("body"), body).expect("body を書ける");
    let script = format!(
        "#!/bin/sh\n\
         printf '%s\\n' \"$@\" >> \"{d}/args\"\n\
         cat >> \"{d}/stdin\"\n\
         cat \"{d}/body\"\n\
         printf '\\n%s' '{status}'\n\
         exit {rc}\n"
    );
    let path = fx.spy.join("fake-curl");
    fs::write(&path, script).expect("fake を書ける");
    let mut perm = fs::metadata(&path).expect("fake の権限を読める").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("fake を実行可能にできる");
    path
}

/// `fleet usage` を fixture の置き場・rules・client で撃つ。
fn run_usage(fx: &UsageFixture, curl: &Path, extra: &[&str]) -> Output {
    let state = fx.state.display().to_string();
    let rules = fx.rules.display().to_string();
    let curl = curl.display().to_string();
    let mut args = vec!["usage", "--state-dir", &state, "--rules", &rules, "--curl", &curl];
    args.extend_from_slice(extra);
    run_fleet(&args)
}

/// `fleet usage` を fixture の置き場・rules・client・偽 claude で、PATH を `path` に差し替えて撃つ。
fn run_usage_with_claude_on_path(fx: &UsageFixture, curl: &Path, claude: &Path, path: &str) -> Output {
    let state = fx.state.display().to_string();
    let rules = fx.rules.display().to_string();
    let curl = curl.display().to_string();
    let claude = claude.display().to_string();
    let args = ["usage", "--state-dir", &state, "--rules", &rules, "--curl", &curl, "--claude", &claude];
    run_fleet_with_env(&args, path)
}

/// stdout の行。
fn out_lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect()
}

/// 置き場の口座残量の行（読めなければ空）。
fn allowances(fx: &UsageFixture) -> Vec<Allowance> {
    store::read_all(&fx.state)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|event| event.allowance)
        .collect()
}

/// 置き場を片付ける。
fn drop_fixture(fx: &UsageFixture) {
    fs::remove_dir_all(&fx.state).ok();
    fs::remove_dir_all(&fx.spy).ok();
}

/// (1) live 2 口座相当: 口座ごと 1 行・口座 × 窓（3 窓 × 2）の event・reset は UTC 形・使用率は % の値の切り捨て。
#[test]
fn fleet_usage_measures_two_accounts_into_lines_and_events() {
    let fx = usage_fixture(&["a1", "a2"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    put_credential(&fx, "a2", &live_credential(TOKEN_A2));
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
    let out = run_usage(&fx, &curl, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc 0: {out:?}");
    assert_eq!(out_lines(&out), vec![live_line("a1"), live_line("a2")], "宣言順に口座ごと 1 行");

    let events = store::read_all(&fx.state).expect("event log を読める");
    assert_eq!(events.len(), 6, "3 窓 × 2 口座: {events:?}");
    for event in &events {
        assert_eq!(event.kind, EventKind::AllowanceMeasured, "{event:?}");
        assert_eq!(event.actor, "machine");
        assert!(event.run.is_empty() && event.bead.is_empty(), "便に紐づかない");
        assert_eq!(event.host, vessel::fleet::cli::host(), "host は既存の解決");
    }
    type Seen = (String, WindowKind, Option<String>, u64, Option<String>);
    let mut seen: Vec<Seen> = allowances(&fx)
        .into_iter()
        .filter_map(|row| match row {
            Allowance::Measured(found) => {
                assert_eq!(found.endpoint, "oauth-usage", "出所の識別子");
                Some((found.account, found.window, found.model, found.used_pct, found.resets_at))
            }
            Allowance::Unmeasured(_) => None,
        })
        .collect();
    seen.sort();
    let want_for = |label: &str| {
        vec![
            (label.to_owned(), WindowKind::FiveHour, None, 13, Some("2026-09-12T05:00:00Z".to_owned())),
            (label.to_owned(), WindowKind::SevenDay, None, 41, Some("2026-09-18T00:00:00Z".to_owned())),
            (label.to_owned(), WindowKind::SevenDayModel, Some("Fable".to_owned()), 38, Some("2026-09-18T00:00:00Z".to_owned())),
        ]
    };
    let mut want = want_for("a1");
    want.extend(want_for("a2"));
    assert_eq!(seen, want, "used_pct は切り捨て・cap しない・reset は UTC の Z 形");
    drop_fixture(&fx);
}

/// (2) token は stdin の設定行にだけ在り、argv・stdout・stderr・events.jsonl に 0 回。
/// (3) `--max-time` には rules 行の値が渡る。
#[test]
fn fleet_usage_token_travels_only_on_stdin_and_timeout_comes_from_rules() {
    let fx = usage_fixture(&["a1", "a2"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    put_credential(&fx, "a2", &live_credential(TOKEN_A2));
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
    let out = run_usage(&fx, &curl, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc 0: {out:?}");

    let stdin = fs::read_to_string(fx.spy.join("stdin")).expect("偽 curl が stdin を写した");
    for token in [TOKEN_A1, TOKEN_A2] {
        assert!(stdin.contains(&format!("Authorization: Bearer {token}")), "stdin の設定行に token: {stdin}");
    }
    assert!(stdin.contains("anthropic-beta: oauth-2025-04-20"), "{stdin}");
    assert!(stdin.contains("Accept: application/json"), "{stdin}");
    assert!(!stdin.contains("r-not-read"), "refresh token は読まない・渡さない");

    let args = fs::read_to_string(fx.spy.join("args")).expect("偽 curl が argv を写した");
    let events = fs::read_to_string(store::events_path(&fx.state)).expect("event log が在る");
    let faces = [
        ("argv", args.clone()),
        ("stdout", String::from_utf8_lossy(&out.stdout).into_owned()),
        ("stderr", String::from_utf8_lossy(&out.stderr).into_owned()),
        ("events.jsonl", events.clone()),
    ];
    for (face, text) in &faces {
        assert!(!text.is_empty() || *face == "stderr", "{face} の母集団が空でない");
        for token in [TOKEN_A1, TOKEN_A2] {
            assert_eq!(text.matches(token).count(), 0, "{face} に token が 0 回（母集団 {} byte）", text.len());
        }
    }
    let argv: Vec<&str> = args.lines().collect();
    assert_eq!(argv.len() % 11, 0, "口座ごとに 11 引数: {argv:?}");
    let pairs = argv.windows(2).filter(|w| w == &["--max-time", "13"]).count();
    assert_eq!(pairs, 2, "--max-time に rules 行の値（口座 2 回分）: {argv:?}");
    assert!(argv.windows(2).any(|w| w == ["-K", "-"]), "設定は stdin から: {argv:?}");
    assert_eq!(argv.iter().filter(|arg| **arg == "https://api.anthropic.com/api/oauth/usage").count(), 2);
    drop_fixture(&fx);
}

/// (4) credential の無い label は `no_credentials` の 1 行で、他の口座の読みは続く。
#[test]
fn fleet_usage_missing_credential_is_one_line_and_others_continue() {
    let fx = usage_fixture(&["a1", "ghost", "a2"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    put_credential(&fx, "a2", &live_credential(TOKEN_A2));
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
    let out = run_usage(&fx, &curl, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "測れなかったは失敗ではない: {out:?}");
    assert_eq!(
        out_lines(&out),
        vec![live_line("a1"), "usage: account=ghost unmeasured reason=no_credentials".to_owned(), live_line("a2")]
    );
    let ghost: Vec<Allowance> = allowances(&fx)
        .into_iter()
        .filter(|row| row.key().account == "ghost")
        .collect();
    assert_eq!(
        ghost,
        vec![Allowance::Unmeasured(Unmeasured {
            account: "ghost".to_owned(),
            window: None,
            model: None,
            endpoint: "oauth-usage".to_owned(),
            reason: UnmeasuredReason::NoCredentials,
        })],
        "窓を持たない Unmeasured 1 行"
    );
    let calls = fs::read_to_string(fx.spy.join("args")).unwrap_or_default();
    assert_eq!(calls.lines().filter(|arg| *arg == "--max-time").count(), 2, "ghost では client を起こさない");
    drop_fixture(&fx);
}

/// (5) credential の各失敗が別の理由になる（client は 1 度も起きない）。
#[test]
fn fleet_usage_credential_failures_name_their_reason() {
    let fx = usage_fixture(&["tomb", "old", "notoken", "broken"]);
    put_credential(&fx, "tomb", r#"{"claudeAiOauth":{"accessToken":"tok-tomb","expiresAt":0}}"#);
    put_credential(&fx, "old", r#"{"claudeAiOauth":{"accessToken":"tok-old","expiresAt":1000}}"#);
    put_credential(&fx, "notoken", &format!(r#"{{"claudeAiOauth":{{"expiresAt":{FAR_EXPIRES_MS}}}}}"#));
    put_credential(&fx, "broken", "{ not json");
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
    let claude = fake_claude(&fx, "exit 0");
    let claude = claude.display().to_string();
    let out = run_usage(&fx, &curl, &["--claude", &claude]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(
        out_lines(&out),
        vec![
            "usage: account=tomb unmeasured reason=tombstone",
            "usage: account=old unmeasured reason=token_expired refresh=ok",
            "usage: account=notoken unmeasured reason=no_token",
            "usage: account=broken unmeasured reason=shape_mismatch",
        ]
    );
    assert_eq!(allowances(&fx).len(), 4, "口座ごとに 1 行");
    assert!(!fx.spy.join("args").exists(), "credential で止まった口座は client を起こさない");
    drop_fixture(&fx);
}

/// (6) client 側の各失敗が別の理由になる。`display_name` 欠落はその要素だけ。
#[test]
fn fleet_usage_client_failures_name_their_reason() {
    let no_name = LIVE_BODY.replace(r#""display_name": "Fable", "#, "");
    let cases: [(&str, &str, &str, u8, &str); 5] = [
        ("timeout", LIVE_BODY, "200", 28, "usage: account=a1 unmeasured reason=timeout"),
        ("refused", LIVE_BODY, "200", 7, "usage: account=a1 unmeasured reason=client_failed"),
        ("status", LIVE_BODY, "500", 0, "usage: account=a1 unmeasured reason=http_status"),
        ("garbage", "<html>oops</html>", "200", 0, "usage: account=a1 unmeasured reason=body_unreadable"),
        (
            "no display_name",
            &no_name,
            "200",
            0,
            "usage: account=a1 five_hour=13% resets=2026-09-12T05:00:00Z seven_day=41% resets=2026-09-18T00:00:00Z seven_day_model=unmeasured:shape_mismatch",
        ),
    ];
    for (name, body, status, rc, want) in cases {
        let fx = usage_fixture(&["a1"]);
        put_credential(&fx, "a1", &live_credential(TOKEN_A1));
        let curl = fake_curl(&fx, body, status, rc);
        let out = run_usage(&fx, &curl, &[]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{name}: {out:?}");
        assert_eq!(out_lines(&out), vec![want.to_owned()], "{name}");
        drop_fixture(&fx);
    }

    let fx = usage_fixture(&["a1"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    let absent = fx.spy.join("no-such-curl");
    let out = run_usage(&fx, &absent, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["usage: account=a1 unmeasured reason=client_missing".to_owned()]);
    drop_fixture(&fx);

    let fx = usage_fixture(&["a1"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    let curl = fake_curl(&fx, &no_name, "200", 0);
    let out = run_usage(&fx, &curl, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    let rows = allowances(&fx);
    let measured = rows.iter().filter(|row| matches!(row, Allowance::Measured(_))).count();
    let broken: Vec<&Allowance> = rows.iter().filter(|row| matches!(row, Allowance::Unmeasured(_))).collect();
    assert_eq!(measured, 2, "five_hour / seven_day は Measured: {rows:?}");
    assert_eq!(
        broken,
        vec![&Allowance::Unmeasured(Unmeasured {
            account: "a1".to_owned(),
            window: Some(WindowKind::SevenDayModel),
            model: None,
            endpoint: "oauth-usage".to_owned(),
            reason: UnmeasuredReason::ShapeMismatch,
        })],
        "その要素だけ ShapeMismatch"
    );
    drop_fixture(&fx);
}

/// (7) 宣言が 0 件（tracked の面にも host の面にも `[[account]]` が無い）の周は「宣言なし」を stderr に 1 行出して
/// **止めない**（rc 0・stdout 0 byte・何も書かない・client を起こさない・account-lifecycle.md §2）。
#[test]
fn fleet_usage_without_declaration_says_so_and_writes_nothing() {
    let fx = usage_fixture(&[]);
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
    let out = run_usage(&fx, &curl, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert!(out.stdout.is_empty(), "stdout は 0 byte");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "fleet usage: 宣言なし（[[account]] が 0 行・計測しない）\n",
        "理由を stderr へ 1 行"
    );
    assert!(!store::events_path(&fx.state).exists(), "event を書かない");
    assert!(!fx.spy.join("args").exists(), "client を起こさない");
    let shown = run_usage(&fx, &curl, &["--show"]);
    assert_eq!(shown.status.code(), Some(i32::from(RC_OK)), "--show も同じく止めない");
    assert!(shown.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&shown.stderr), String::from_utf8_lossy(&out.stderr), "--show も同じ 1 行");
    drop_fixture(&fx);
}

/// 壊れた host の面（未知 key line=5・型違い line=8）。
const HOST_BROKEN: &str = "schema = 1\n\n[[account]]\nlabel = \"a1\"\nhost = \"x\"\n\n[[plugin]]\ndir = 1\n";

/// `<state>/host.toml` に `text` を置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_host(fx: &UsageFixture, text: &str) {
    fs::create_dir_all(&fx.state).expect("置き場を作れる");
    fs::write(fx.state.join(vessel::rules::HOST_MANIFEST), text).expect("host の面を書ける");
}

/// (d) 壊れた host の面では `fleet usage` / `fleet select` が typed に止まる（`UsageError::Manifest`・rc 1・stderr 1 行に
/// `host.toml:` の欠陥を行番号付きで全件・stdout 0 byte・event を書かない・client を起こさない）。
#[test]
fn rules_host_broken_host_manifest_stops_fleet_usage_and_select_without_events() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    put_host(&fx, HOST_BROKEN);
    let want = "fleet usage: manifest を読めない（rules: host.toml: 未知の key host line=5 / rules: host.toml: dir は文字列でなければならない（実 One(Int(1))） line=8）\n";
    for (face, out) in [
        ("usage", run_usage(&fx, &curl, &[])),
        ("usage --show", run_usage(&fx, &curl, &["--show"])),
        ("select", run_select(&fx, &curl, &["--purpose", "run"])),
    ] {
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{face}: {out:?}");
        assert!(out.stdout.is_empty(), "{face}: stdout は 0 byte");
        assert_eq!(String::from_utf8_lossy(&out.stderr), want, "{face}: 1 行・全件・面の接頭辞");
    }
    assert!(!store::events_path(&fx.state).exists(), "event を書かない");
    assert_eq!(curl_calls(&fx), 0, "client を起こさない");
    drop_fixture(&fx);
}

/// (e) `--rules` 無し（tracked の面 = 埋め込み・口座 0）でも host の面の宣言だけで口座ごとに 1 行を宣言順で出し、
/// 同じ宣言から `fleet select` が選ぶ。host の面が無い周は「宣言なし」で止めない（候補なし）。
#[test]
fn rules_host_fleet_usage_measures_the_host_declared_accounts_without_rules() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    let state = fx.state.display().to_string();
    let client = curl.display().to_string();
    let absent = run_fleet(&["usage", "--state-dir", &state, "--curl", &client]);
    assert_eq!(absent.status.code(), Some(i32::from(RC_OK)), "{absent:?}");
    assert!(absent.stdout.is_empty(), "host の面が無い周は 0 行: {absent:?}");
    assert!(String::from_utf8_lossy(&absent.stderr).contains("宣言なし"), "{absent:?}");
    let none = run_fleet(&["select", "--state-dir", &state, "--curl", &client, "--purpose", "run"]);
    assert_eq!(out_lines(&none), vec!["select purpose=run none=unmeasured earliest_reset=-".to_owned()], "{none:?}");
    assert_eq!(curl_calls(&fx), 0, "宣言なしは client を起こさない");

    put_host(&fx, "schema = 1\n\n[[account]]\nlabel = \"a3\"\n\n[[account]]\nlabel = \"a1\"\n\n[[plugin]]\ndir = \"plugins/p\"\n");
    let out = run_fleet(&["usage", "--state-dir", &state, "--curl", &client]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    let accounts: Vec<String> = out_lines(&out)
        .iter()
        .filter_map(|line| line.strip_prefix("usage: account=").and_then(|rest| rest.split(' ').next()).map(str::to_owned))
        .collect();
    assert_eq!(accounts, ["a3", "a1"], "host の面の宣言順に口座ごと 1 行（a2 は宣言外）: {out:?}");
    assert_eq!(curl_calls(&fx), 2, "宣言した口座だけ計測する");
    let chosen = run_fleet(&["select", "--state-dir", &state, "--curl", &client, "--purpose", "run"]);
    assert_eq!(out_lines(&chosen), vec!["select purpose=run chosen=a1".to_owned()], "a3 は当たっている: {chosen:?}");
    drop_fixture(&fx);
}

/// (8) `--show` は append しない（size・mtime 同一・lock 不在）で replay の最新を同じ 1 行形で出す。
#[test]
fn fleet_usage_show_is_read_only_and_prints_the_latest() {
    let fx = usage_fixture(&["a1", "a2"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    put_credential(&fx, "a2", &live_credential(TOKEN_A2));
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
    let first = run_usage(&fx, &curl, &[]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "{first:?}");
    let newer = LIVE_BODY.replace("13.0", "33.0");
    let curl = fake_curl(&fx, &newer, "200", 0);
    let second = run_usage(&fx, &curl, &[]);
    assert_eq!(second.status.code(), Some(i32::from(RC_OK)), "{second:?}");
    assert!(out_lines(&second).iter().all(|line| line.contains("five_hour=33%")), "{second:?}");

    let events = store::events_path(&fx.state);
    let before = fs::metadata(&events).expect("event log が在る");
    let shown = run_usage(&fx, &curl, &["--show"]);
    let after = fs::metadata(&events).expect("event log が在る");
    assert_eq!(shown.status.code(), Some(i32::from(RC_OK)), "{shown:?}");
    assert_eq!(out_lines(&shown), out_lines(&second), "replay の最新を計測と同じ 1 行形で");
    assert_eq!(before.len(), after.len(), "size を変えない");
    assert_eq!(before.modified().ok(), after.modified().ok(), "mtime を変えない");
    assert!(!store::lock_path(&fx.state).exists(), "lock を取らない");
    assert_eq!(allowances(&fx).len(), 12, "--show は行を足さない（2 回 × 6 行）");
    drop_fixture(&fx);
}

/// (9) `--state-dir` 無しは rc 1。
#[test]
fn fleet_usage_requires_state_dir() {
    let out = run_fleet(&["usage"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{out:?}");
    assert!(out.stdout.is_empty(), "stdout は 0 byte");
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage"), "使い方を stderr へ");
}

/// 期限の過ぎた、読める credential の本文。
fn expired_credential(token: &str) -> String {
    format!(r#"{{"claudeAiOauth":{{"accessToken":"{token}","refreshToken":"r-not-read","expiresAt":1000}}}}"#)
}

/// 偽 claude（偽 curl と同じ型）。
///
/// argv を 1 行 1 引数で `claude-args` へ、口座の env を `claude-env` へ、cwd を `claude-cwd` へ**追記**で写し
/// （口座ごとに 1 回呼ばれる）、`tail` の shell 行を撃つ。`tail` の中の `{spy}` は写しの置き場に、`{fresh}` は
/// 期限の遠い credential の本文を置いた file に置き換わる。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_claude(fx: &UsageFixture, tail: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let d = fx.spy.display().to_string();
    let fresh = fx.spy.join("fresh-credential");
    fs::write(&fresh, live_credential(TOKEN_A1)).expect("fresh な credential を書ける");
    let tail = tail.replace("{spy}", &d).replace("{fresh}", &fresh.display().to_string());
    let script = format!(
        "#!/bin/sh\n\
         printf '%s\\n' \"$@\" >> \"{d}/claude-args\"\n\
         printf 'CLAUDE_CONFIG_DIR=%s\\n' \"$CLAUDE_CONFIG_DIR\" >> \"{d}/claude-env\"\n\
         pwd -P >> \"{d}/claude-cwd\"\n\
         {tail}\n"
    );
    let path = fx.spy.join("fake-claude");
    fs::write(&path, script).expect("fake を書ける");
    let mut perm = fs::metadata(&path).expect("fake の権限を読める").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("fake を実行可能にできる");
    path
}

/// 偽 claude の起動回数（argv の写しの `-p` の数・写しが無ければ 0）。
fn claude_calls(fx: &UsageFixture) -> usize {
    fs::read_to_string(fx.spy.join("claude-args"))
        .unwrap_or_default()
        .lines()
        .filter(|arg| *arg == "-p")
        .count()
}

/// `fleet usage` を偽 curl・偽 claude で撃つ。
fn run_usage_with_claude(fx: &UsageFixture, curl: &Path, claude: &Path) -> Output {
    let claude = claude.display().to_string();
    run_usage(fx, curl, &["--claude", &claude])
}

// ─────────────── 口座の口（`account`・account-lifecycle.md §3 / §8・接頭辞 `account_cmd_`） ───────────────

/// `account` を binary で 1 回撃つ。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_account(args: &[&str]) -> Output {
    Command::new(bin()).arg("account").args(args).output().expect("binary を起動できる")
}

/// 出力の byte を文字列で見る。
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `labels` を宣言順に持つ host の面の本文（`account add` が書く形と同じ）。
fn host_body(labels: &[&str]) -> String {
    labels.iter().fold("schema = 1\n".to_owned(), |body, label| format!("{body}\n[[account]]\nlabel = \"{label}\"\n"))
}

/// 置き場の host の面の本文（無ければ空）。
fn host_text(dir: &Path) -> String {
    fs::read_to_string(dir.join(vessel::rules::HOST_MANIFEST)).unwrap_or_default()
}

/// 置き場に host の面を書く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_host_labels(dir: &Path, labels: &[&str]) {
    fs::write(dir.join(vessel::rules::HOST_MANIFEST), host_body(labels)).expect("host の面を書ける");
}

/// `root` の下の全 entry（相対 path・種類・本文〔link は指す先〕）を path の順に（link は辿らない）。
fn tree(root: &Path) -> Vec<(PathBuf, String, Vec<u8>)> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        let rel = path.strip_prefix(root).map(Path::to_path_buf).unwrap_or_default();
        if meta.file_type().is_symlink() {
            let to = fs::read_link(&path).map(|found| found.display().to_string()).unwrap_or_default();
            found.push((rel, "link".to_owned(), to.into_bytes()));
        } else if meta.is_dir() {
            found.push((rel, "dir".to_owned(), Vec::new()));
            let children: Vec<PathBuf> =
                fs::read_dir(&path).map(|entries| entries.filter_map(|entry| entry.ok().map(|e| e.path())).collect()).unwrap_or_default();
            stack.extend(children);
        } else {
            found.push((rel, "file".to_owned(), fs::read(&path).unwrap_or_default()));
        }
    }
    found.sort();
    found
}

/// 置き場の口座の退役・戻しの event（kind・label）を物理順に。
fn account_events(dir: &Path) -> Vec<(EventKind, Option<String>)> {
    store::read_all(dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|event| matches!(event.kind, EventKind::AccountRetired | EventKind::AccountRestored))
        .map(|event| (event.kind, event.account))
        .collect()
}

/// 退役先（`<state>/accounts/.retired/`）の entry 名（名前の順）。
fn retired_entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir.join("accounts").join(".retired"))
        .map(|entries| entries.filter_map(|entry| entry.ok().map(|e| e.file_name().to_string_lossy().into_owned())).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// 登録 row を 1 件積む（口座 = `label`・`seat register` は打刻を要るので行を直に積む）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn register_account(dir: &Path, label: &str) {
    let mut event = registration_event("s:w");
    event.registration = event.registration.map(|row| Registration { account: label.to_owned(), ..row });
    store::append(dir, &event, LockPolicy::embedded().expect("lock の規則を読める")).expect("登録 row を積める");
}

/// (6a) `add` は dir と直下の `settings.json`（agent view を切る 1 項目）と host の面の `[[account]]` 行 1 つを揃え、stdout に
/// login の起動行を 1 行（cwd は `--anchor`・無ければ置き場）。2 つ目の口座は既存の宣言の後ろに 1 行。
/// base は `account` の subcommand が無く使い方で断る（RED・機能不在）。
#[test]
fn account_cmd_add_prepares_the_dir_settings_and_one_declaration_line() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let out = run_account(&["add", "a1", "--state-dir", &path]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    let a1 = dir.join("accounts").join("a1");
    assert_eq!(text(&out.stdout), format!("account: prepared a1 next=cd {path} && CLAUDE_CONFIG_DIR={} claude\n", a1.display()));
    assert!(a1.is_dir(), "dir を作る");
    assert_eq!(fs::read_to_string(a1.join("settings.json")).unwrap_or_default(), "{\"disableAgentView\": true}\n", "1 項目だけ");
    assert_eq!(fs::read_dir(&a1).map(Iterator::count).unwrap_or_default(), 1, "credential は書かない（settings.json だけ）");
    assert_eq!(host_text(&dir), host_body(&["a1"]), "schema = 1 から作り行を 1 つ");
    assert!(!dir.join("host.toml.staged").exists(), "一時 file を残さない");
    let anchor = dir.join("anchor");
    let anchored = run_account(&["add", "a3", "--state-dir", &path, "--anchor", &anchor.display().to_string()]);
    assert_eq!(anchored.status.code(), Some(i32::from(RC_OK)), "{anchored:?}");
    let a3 = dir.join("accounts").join("a3");
    assert_eq!(text(&anchored.stdout), format!("account: prepared a3 next=cd {} && CLAUDE_CONFIG_DIR={} claude\n", anchor.display(), a3.display()));
    assert_eq!(host_text(&dir), host_body(&["a1", "a3"]), "既存の宣言の後ろに 1 行");
    fs::remove_dir_all(&dir).ok();
}

/// (6a) 2 回目の `add`（宣言済み）は `exists`・user の既存 dir は `dir-exists` で、どちらも何も書かない（宣言も足さない）。
#[test]
fn account_cmd_add_refuses_exists_and_dir_exists_without_writing() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let first = run_account(&["add", "a1", "--state-dir", &path]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "{first:?}");
    let a2 = dir.join("accounts").join("a2");
    fs::create_dir_all(&a2).expect("user の dir を置ける");
    fs::write(a2.join("mine"), "user").expect("user の file を置ける");
    let before = tree(&dir);
    for (label, reason) in [("a1", "exists"), ("a2", "dir-exists")] {
        let out = run_account(&["add", label, "--state-dir", &path]);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{out:?}");
        assert!(out.stdout.is_empty(), "断りは stdout 0 byte");
        assert_eq!(text(&out.stderr), format!("account: refused reason={reason} label={label}\n"));
        assert_eq!(tree(&dir), before, "{label}: 何も書かない");
    }
    fs::remove_dir_all(&dir).ok();
}

/// 偽 claude（`CLAUDE_CONFIG_DIR` と cwd を 1 行で `<dir>/login.log` へ追記する）を置いた shim の dir。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn login_shim(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let shim = dir.join("shim");
    fs::create_dir_all(&shim).expect("shim の dir を作れる");
    let log = dir.join("login.log");
    let script = format!("#!/bin/sh\nprintf '%s %s\\n' \"$CLAUDE_CONFIG_DIR\" \"$(pwd -P)\" >> \"{}\"\n", log.display());
    let path = shim.join("claude");
    fs::write(&path, script).expect("fake を書ける");
    let mut perm = fs::metadata(&path).expect("fake の権限を読める").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("fake を実行可能にできる");
    shim
}

/// 独立 socket の session を畳む guard（panic 経路でも drop が走る・socket を消す前に畳む）。
struct LoginSession {
    /// 独立 socket の path。
    socket: String,
    /// session 名。
    name: String,
}

impl Drop for LoginSession {
    fn drop(&mut self) {
        let _ = Command::new("tmux").args(["-S", &self.socket, "-f", "/dev/null", "kill-session", "-t", &self.name]).output();
    }
}

/// pane 本文（行末の空白は tmux が落とす）。
fn login_pane(socket: &str, name: &str) -> String {
    text(&crate::seat::tmux(socket, &["capture-pane", "-p", "-t", name]).stdout)
}

/// 条件が立つまで 5 秒の窓で 50 ms ごとに見る。
fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    done()
}

/// shim を PATH の先頭に置いた shell（prompt `$ `）の session を独立 socket に立て、prompt が描かれたかを添えて返す。
fn login_session(dir: &Path, name: &str, shim: &Path) -> (LoginSession, bool) {
    let socket = dir.join("sock").display().to_string();
    let guard = LoginSession { socket: socket.clone(), name: name.to_owned() };
    let shell = format!("PATH='{}':/usr/bin:/bin; export PATH; exec sh -i", shim.display());
    let args = ["new-session", "-d", "-s", name, "-n", name, "-x", "200", "-y", "40", "-e", "PS1=$ ", "sh", "-c", &shell];
    let started = crate::seat::tmux(&socket, &args).status.success();
    let ready = started && wait_until(|| login_pane(&socket, name).trim_end().ends_with('$'));
    (guard, ready)
}

/// (6a) `--target` は契約 (b) と同じ注入の門を通して login の起動行を shell へ 1 回だけ送る（偽 claude が
/// `CLAUDE_CONFIG_DIR` = 口座の dir・cwd = `--anchor` で 1 回走る）。入力欄に打ちかけの在る shell へは 1 key も送らず
/// `refused=input-busy` と行を返す（dir と宣言は揃え終えている）。
#[test]
fn account_cmd_add_target_injects_the_login_line_once() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let shim = login_shim(&dir);
    let name = "acctlogin";
    let (guard, ready) = login_session(&dir, name, &shim);
    assert!(ready, "独立 socket に shell の session を立てられる: {}", login_pane(&guard.socket, name));
    let anchor = dir.join("anchor");
    fs::create_dir_all(&anchor).expect("anchor を作れる");
    let target = format!("{name}:{name}");
    let anchor_s = anchor.display().to_string();

    let out = run_account(&["add", "a1", "--state-dir", &path, "--anchor", &anchor_s, "--target", &target, "--tmux-socket", &guard.socket]);

    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(text(&out.stdout), format!("account: prepared a1 target={name}_{name}\n"));
    let log = dir.join("login.log");
    let real = fs::canonicalize(&anchor).expect("anchor を実 path にできる");
    let want = format!("{} {}\n", dir.join("accounts").join("a1").display(), real.display());
    assert!(
        wait_until(|| fs::read_to_string(&log).unwrap_or_default() == want),
        "偽 claude が口座の dir と anchor で 1 回走る: {:?} pane={}",
        fs::read_to_string(&log),
        login_pane(&guard.socket, name)
    );
    assert!(wait_until(|| login_pane(&guard.socket, name).trim_end().ends_with('$')), "prompt に戻る");
    assert!(crate::seat::tmux(&guard.socket, &["send-keys", "-t", &target, "-l", "git st"]).status.success());
    assert!(wait_until(|| login_pane(&guard.socket, name).trim_end().ends_with("$ git st")), "打ちかけが描かれる");
    let busy = run_account(&["add", "a2", "--state-dir", &path, "--target", &target, "--tmux-socket", &guard.socket]);
    assert_eq!(busy.status.code(), Some(i32::from(RC_REFUSED)), "{busy:?}");
    assert!(text(&busy.stderr).starts_with("account: prepared a2 refused=input-busy next=cd "), "{busy:?}");
    assert!(dir.join("accounts").join("a2").join("settings.json").is_file(), "dir と宣言は揃え終えている");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), want, "門で止まった周は 1 key も送らない＝偽 claude は 1 回だけ");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// (6b) `ls` の行は doctor の口座行（同じ 1 関数・`retired=` 込み）に最新の実測の要約（実測行あり＝使用率・なし＝
/// `unmeasured`）を足したもの（label の辞書順・rc 0・計測は撃たない＝event は増えない）。
#[test]
fn account_cmd_ls_is_the_doctor_line_plus_retired_and_allowance() {
    let dir = state_dir();
    let path = dir.display().to_string();
    put_host_labels(&dir, &["zeta", "a1"]);
    let a1 = dir.join("accounts").join("a1");
    fs::create_dir_all(&a1).expect("口座の dir を作れる");
    fs::write(a1.join(".credentials.json"), "{}").expect("credential の印を置ける");
    fs::write(a1.join("settings.json"), "{\"disableAgentView\": true}").expect("settings を置ける");
    let policy = LockPolicy::embedded().expect("rules 行を引ける");
    for row in [measured("a1", WindowKind::FiveHour, None, 13), measured("a1", WindowKind::SevenDay, None, 41)] {
        store::append(&dir, &allowance_event(ALLOWANCE_TS, row), policy).expect("追記できる");
    }
    let events = fs::read(store::events_path(&dir)).unwrap_or_default();

    let out = run_account(&["ls", "--state-dir", &path]);

    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    let lines: Vec<String> = text(&out.stdout).lines().map(str::to_owned).collect();
    assert_eq!(
        lines,
        [
            "account=a1 dir=present credential=present config=present agentview=off trust=n/a retired=no five_hour=13% seven_day=41%",
            "account=zeta dir=missing credential=missing config=missing agentview=unreadable trust=n/a retired=no five_hour=unmeasured seven_day=unmeasured",
        ]
    );
    assert_eq!(fs::read(store::events_path(&dir)).unwrap_or_default(), events, "計測を撃たない");
    let socket = dir.join("no-sock").display().to_string();
    let doctor = Command::new(bin()).args(["doctor", "--state-dir", &path, "--tmux-socket", &socket]).output().expect("binary を起動できる");
    let doctor_rows: Vec<String> = text(&doctor.stdout).lines().filter(|line| line.starts_with("account=")).map(str::to_owned).collect();
    let heads: Vec<String> = lines.iter().map(|line| line.split(" five_hour=").next().unwrap_or_default().to_owned()).collect();
    assert_eq!(doctor_rows, heads, "doctor の口座行と同じ行（要約だけが後ろに足される）");
    fs::remove_dir_all(&dir).ok();
}

/// 退役の歯の置き場: host の面に a1 / a2・両口座に読める credential・偽 curl（[`LIVE_BODY`]）。`(fixture, state, curl)`。
fn retire_place() -> (UsageFixture, String, String) {
    let fx = usage_fixture(&[]);
    let state = fx.state.display().to_string();
    put_host_labels(&fx.state, &["a1", "a2"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    put_credential(&fx, "a2", &live_credential(TOKEN_A2));
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0).display().to_string();
    (fx, state, curl)
}

/// (6c) `retire` は dir を `.retired/` へ 1 つだけ動かし（中身ごと）・`AccountRetired` を 1 件積み・宣言の行は残す。
#[test]
fn account_cmd_retire_moves_the_dir_once_and_records_one_event() {
    let (fx, state, _) = retire_place();
    let out = run_account(&["retire", "a1", "--state-dir", &state]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(text(&out.stdout), "account: retired a1\n");
    assert!(fs::symlink_metadata(fx.state.join("accounts").join("a1")).is_err(), "元の場所から消える");
    let moved = retired_entries(&fx.state);
    assert_eq!(moved.len(), 1, "退役先に 1 つだけ: {moved:?}");
    let name = moved.first().cloned().unwrap_or_default();
    assert!(name.starts_with("a1."), "{name}");
    assert!(fx.state.join("accounts").join(".retired").join(&name).join(".credentials.json").is_file(), "中身ごと動く");
    assert_eq!(account_events(&fx.state), vec![(EventKind::AccountRetired, Some("a1".to_owned()))], "event 1 件");
    assert_eq!(host_text(&fx.state), host_body(&["a1", "a2"]), "宣言の行は消さない");
    drop_fixture(&fx);
}

/// (6c) 退役中の口座は `fleet select` の候補から消え（同点で辞書順の先だった a1 → a2）、`fleet usage` が測らない（a1 の行が
/// 増えない）。
#[test]
fn account_cmd_retired_account_leaves_select_and_usage() {
    let (fx, state, curl) = retire_place();
    let select = || text(&run_fleet(&["select", "--purpose", "session", "--state-dir", &state, "--curl", &curl]).stdout);
    assert_eq!(select(), "select purpose=session chosen=a1\n", "同点は辞書順で a1");
    assert_eq!(run_account(&["retire", "a1", "--state-dir", &state]).status.code(), Some(i32::from(RC_OK)));
    let a1_rows = |fx: &UsageFixture| allowances(fx).iter().filter(|row| row.key().account == "a1").count();
    let measured_a1 = a1_rows(&fx);
    let usage = run_fleet(&["usage", "--state-dir", &state, "--curl", &curl]);
    assert_eq!(out_lines(&usage), vec![live_line("a2")], "退役中の a1 は測らない: {usage:?}");
    assert_eq!(a1_rows(&fx), measured_a1, "a1 の行は増えない");
    assert_eq!(select(), "select purpose=session chosen=a2\n", "退役中の a1 は候補に入らない");
    drop_fixture(&fx);
}

/// (6c) 登録 row のどれかが持つ口座は `in-use` で動かず、event も書かない（置き場の全 entry が不変）。
#[test]
fn account_cmd_retire_refuses_an_account_in_use() {
    let (fx, state, _) = retire_place();
    register_account(&fx.state, "a2");
    let before = tree(&fx.state);
    let used = run_account(&["retire", "a2", "--state-dir", &state]);
    assert_eq!(used.status.code(), Some(i32::from(RC_REFUSED)), "{used:?}");
    assert_eq!(text(&used.stderr), "account: refused reason=in-use label=a2\n");
    assert_eq!(tree(&fx.state), before, "dir も event も不変");
    drop_fixture(&fx);
}

/// 戻しの歯の置き場: host の面に a1 / a3・a1 は中身を持つ dir・a3 は実 dir への link。`(置き場, その path, link 先の実 dir)`。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn restore_place() -> (PathBuf, String, PathBuf) {
    let dir = state_dir();
    let path = dir.display().to_string();
    put_host_labels(&dir, &["a1", "a3"]);
    let a1 = dir.join("accounts").join("a1");
    fs::create_dir_all(&a1).expect("口座の dir を作れる");
    fs::write(a1.join("mine"), "kept").expect("中身を置ける");
    let real = dir.join("real-config");
    fs::create_dir_all(&real).expect("link 先を作れる");
    std::os::unix::fs::symlink(&real, dir.join("accounts").join("a3")).expect("link を置ける");
    (dir, path, real)
}

/// `at` が `real` を指す link か（link を辿らずに見る）。
fn is_link_to(at: &Path, real: &Path) -> bool {
    fs::symlink_metadata(at).is_ok_and(|meta| meta.file_type().is_symlink()) && fs::read_link(at).ok().as_deref() == Some(real)
}

/// `account` を撃ち、rc 0 と stdout の 1 行（`want`）を確かめる。
fn account_ok(args: &[&str], want: &str) {
    let out = run_account(args);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{args:?}: {out:?}");
    assert_eq!(text(&out.stdout), format!("{want}\n"), "{args:?}");
}

/// (6d) `restore` は最新の退役先を元へ戻し（中身ごと・link は link のまま）`AccountRestored` を 1 件積む（退役・戻しが 1 件ずつ）。
#[test]
fn account_cmd_restore_moves_back_once_and_records_one_event() {
    let (dir, path, real) = restore_place();
    for label in ["a1", "a3"] {
        account_ok(&["retire", label, "--state-dir", &path], &format!("account: retired {label}"));
    }
    let link = retired_entries(&dir).into_iter().find(|name| name.starts_with("a3.")).unwrap_or_default();
    assert!(is_link_to(&dir.join("accounts").join(".retired").join(&link), &real), "link は link のまま動く: {link}");
    for label in ["a1", "a3"] {
        account_ok(&["restore", label, "--state-dir", &path], &format!("account: restored {label}"));
    }
    assert_eq!(fs::read_to_string(dir.join("accounts").join("a1").join("mine")).unwrap_or_default(), "kept", "中身ごと戻る");
    assert!(is_link_to(&dir.join("accounts").join("a3"), &real), "link のまま戻る");
    assert!(retired_entries(&dir).is_empty(), "退役先に残らない");
    let (a1, a3) = (Some("a1".to_owned()), Some("a3".to_owned()));
    let want = vec![
        (EventKind::AccountRetired, a1.clone()),
        (EventKind::AccountRetired, a3.clone()),
        (EventKind::AccountRestored, a1),
        (EventKind::AccountRestored, a3),
    ];
    assert_eq!(account_events(&dir), want, "退役・戻しが 1 件ずつ");
    fs::remove_dir_all(&dir).ok();
}

/// (6d) `ls` の `retired=` は退役の間だけ `yes`（戻せば `no`）。
#[test]
fn account_cmd_ls_names_retired_only_while_retired() {
    let (dir, path, _) = restore_place();
    let count = |word: &str| {
        let listed = text(&run_account(&["ls", "--state-dir", &path]).stdout);
        listed.lines().filter(|line| line.contains(&format!(" retired={word} "))).count()
    };
    assert_eq!(count("no"), 2, "退役前");
    account_ok(&["retire", "a1", "--state-dir", &path], "account: retired a1");
    assert_eq!((count("yes"), count("no")), (1, 1), "退役中の a1 だけ yes");
    account_ok(&["restore", "a1", "--state-dir", &path], "account: restored a1");
    assert_eq!(count("no"), 2, "戻せば no");
    fs::remove_dir_all(&dir).ok();
}

/// (6d) 退役中でない label は `not-retired`・元の場所が埋まっている周は `dir-exists` で、どちらも何も書かない。
#[test]
fn account_cmd_restore_refuses_not_retired_and_an_occupied_place() {
    let (dir, path, _) = restore_place();
    let refused = |reason: &str| {
        let before = tree(&dir);
        let out = run_account(&["restore", "a1", "--state-dir", &path]);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{out:?}");
        assert_eq!(text(&out.stderr), format!("account: refused reason={reason} label=a1\n"));
        assert_eq!(tree(&dir), before, "{reason}: 何も書かない");
    };
    refused("not-retired");
    account_ok(&["retire", "a1", "--state-dir", &path], "account: retired a1");
    fs::create_dir_all(dir.join("accounts").join("a1")).expect("user が元の場所を作り直す");
    refused("dir-exists");
    fs::remove_dir_all(&dir).ok();
}

/// (6f) 前提違反（label の規則・`exists`・`dir-exists`・`unknown`・`in-use`・`not-retired`）と使い方の誤りは、file も
/// event も 1 byte も変えずに断る（置き場の全 entry の本文が不変）。
#[test]
fn account_cmd_refusals_write_no_file_and_no_event() {
    let dir = state_dir();
    let path = dir.display().to_string();
    put_host_labels(&dir, &["a1"]);
    fs::create_dir_all(dir.join("accounts").join("a1")).expect("口座の dir を作れる");
    fs::create_dir_all(dir.join("accounts").join("a9")).expect("宣言の無い dir を置ける");
    register_account(&dir, "a1");
    let before = tree(&dir);
    for (args, reason, label) in [
        (&["add", "a/b"][..], "label-invalid", "a/b"),
        (&["add", ".x"], "label-invalid", ".x"),
        (&["add", "a1"], "exists", "a1"),
        (&["add", "a9"], "dir-exists", "a9"),
        (&["retire", "ghost"], "unknown", "ghost"),
        (&["retire", "a1"], "in-use", "a1"),
        (&["retire", "../x"], "label-invalid", "../x"),
        (&["restore", "a1"], "not-retired", "a1"),
        (&["restore", "ghost"], "unknown", "ghost"),
    ] {
        let mut call = args.to_vec();
        call.extend_from_slice(&["--state-dir", &path]);
        let out = run_account(&call);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty(), "{args:?}: stdout 0 byte");
        assert_eq!(text(&out.stderr), format!("account: refused reason={reason} label={label}\n"), "{args:?}");
        assert_eq!(tree(&dir), before, "{args:?}: file も event も不変");
    }
    let usage = text(&run_account(&[]).stderr);
    assert!(usage.starts_with("usage: account <add <label>"), "{usage}");
    for bad in [
        &["add"][..],
        &["add", "a2"],
        &["ls"],
        &["ls", "--state-dir"],
        &["ls", "--state-dir", &path, "--state-dir", &path],
        &["retire", "a1", "--state-dir", &path, "--target", "x:y"],
        &["nope", "--state-dir", &path],
    ] {
        let out = run_account(bad);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{bad:?}");
        assert_eq!(text(&out.stderr), usage, "{bad:?} は使い方で断る");
    }
    assert_eq!(tree(&dir), before, "使い方の誤りも何も書かない");
    fs::remove_dir_all(&dir).ok();
}

/// `account` を撃ち、`write-failed`（rc 2・stderr の 1 行・stdout 0 byte）で断られることを確かめる。
fn account_write_failed(args: &[&str], label: &str) {
    let out = run_account(args);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{args:?}: {out:?}");
    assert!(out.stdout.is_empty(), "{args:?}: stdout 0 byte");
    assert_eq!(text(&out.stderr), format!("account: refused reason=write-failed label={label}\n"), "{args:?}");
}

// flip-check: retroactive s2-07l.283
/// (a) host の面が**在るのに読めない**（dir である）周は `write-failed` で断り、`schema = 1` から作り直さない（user の宣言を
/// 「無い」に読み替えない・NFR4）: host.toml は dir のまま・`host.toml.staged` も `accounts/` も現れない（置き場の全 entry が不変）。
#[test]
fn account_cmd_add_refuses_when_host_manifest_is_unreadable_without_rewriting() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let host = dir.join(vessel::rules::HOST_MANIFEST);
    fs::create_dir(&host).expect("host.toml を dir として置ける");
    let before = tree(&dir);
    account_write_failed(&["add", "a1", "--state-dir", &path], "a1");
    assert!(host.is_dir(), "host.toml は dir のまま");
    assert!(fs::symlink_metadata(dir.join("host.toml.staged")).is_err(), "一時 file を作らない");
    assert!(fs::symlink_metadata(dir.join("accounts")).is_err(), "口座の dir（親ごと）を作らない");
    assert_eq!(tree(&dir), before, "置き場の全 entry が不変");
    fs::remove_dir_all(&dir).ok();
}

// flip-check: retroactive s2-07l.283
/// (b) 改行で終わる既存の host.toml（`[[account]]` 1 本）への追記は区切りの空行 1 つ + 2 行＝行数が元 + 3 で、元の本文は
/// 接頭辞として不変（余分な空行が入らない）。対で、末尾改行の無い host.toml へも同じ形（前の行と `[[account]]` が別の行＝読める）。
#[test]
fn account_cmd_add_appends_without_a_blank_line_to_a_newline_terminated_manifest() {
    for (name, original) in [("改行終端", host_body(&["x"])), ("改行なし", host_body(&["x"]).trim_end().to_owned())] {
        let dir = state_dir();
        let path = dir.display().to_string();
        fs::write(dir.join(vessel::rules::HOST_MANIFEST), &original).expect("host の面を書ける");
        let before = original.lines().count();
        let a1 = dir.join("accounts").join("a1");
        account_ok(&["add", "a1", "--state-dir", &path], &format!("account: prepared a1 next=cd {path} && CLAUDE_CONFIG_DIR={} claude", a1.display()));
        let after = host_text(&dir);
        assert_eq!(after.lines().count(), before + 3, "{name}: 区切りの空行 1 つ + 2 行: {after:?}");
        assert!(after.starts_with(original.trim_end()), "{name}: 元の本文は接頭辞として不変: {after:?}");
        assert_eq!(after, host_body(&["x", "a1"]), "{name}: 余分な空行が入らない");
        let lines: Vec<&str> = after.lines().collect();
        assert_eq!(lines.get(before.saturating_sub(1) + 2), Some(&"[[account]]"), "{name}: 前の行と別の行に [[account]]: {lines:?}");
        let listed = text(&run_account(&["ls", "--state-dir", &path]).stdout);
        assert_eq!(listed.lines().filter(|line| line.starts_with("account=")).count(), 2, "{name}: 2 口座とも読める: {listed}");
        fs::remove_dir_all(&dir).ok();
    }
}

// flip-check: retroactive s2-07l.283
/// (c) 既存の host.toml が未知 key を持つ（host の面として読めない）周は `write-failed` で断り、host.toml の bytes は不変・
/// `host.toml.staged` が残らず・口座の dir も現れない（壊れた面を rename して上書きしない・NFR4）。
#[test]
fn account_cmd_add_refuses_when_the_staged_manifest_does_not_parse_and_keeps_host_toml() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let host = dir.join(vessel::rules::HOST_MANIFEST);
    let broken = "schema = 1\n\n[[account]]\nlabel = \"x\"\nbogus = 1\n";
    fs::write(&host, broken).expect("host の面を書ける");
    let before = tree(&dir);
    account_write_failed(&["add", "a1", "--state-dir", &path], "a1");
    assert_eq!(fs::read(&host).unwrap_or_default(), broken.as_bytes(), "host.toml の bytes は不変");
    assert!(fs::symlink_metadata(dir.join("host.toml.staged")).is_err(), "一時 file が残らない");
    assert!(fs::symlink_metadata(dir.join("accounts").join("a1")).is_err(), "口座の dir を作らない");
    assert_eq!(tree(&dir), before, "置き場の全 entry が不変");
    fs::remove_dir_all(&dir).ok();
}

// flip-check: retroactive s2-07l.283
/// (d) 宣言だけ在って `<state>/accounts/<label>` が無い口座の `retire` は、退役先の親（`accounts/.retired/`）を**作る前に**
/// `write-failed` で断り、event も 0 件（置き場の全 entry が不変）。正例は
/// [`account_cmd_retire_moves_the_dir_once_and_records_one_event`]。
#[test]
fn account_cmd_retire_refuses_a_missing_dir_before_creating_the_retired_parent() {
    let dir = state_dir();
    let path = dir.display().to_string();
    put_host_labels(&dir, &["a1"]);
    let before = tree(&dir);
    account_write_failed(&["retire", "a1", "--state-dir", &path], "a1");
    assert!(fs::symlink_metadata(dir.join("accounts").join(".retired")).is_err(), "退役先の親を作らない");
    assert!(fs::symlink_metadata(dir.join("accounts")).is_err(), "accounts/ も作らない");
    assert!(account_events(&dir).is_empty(), "event 0 件");
    assert_eq!(tree(&dir), before, "置き場の全 entry が不変");
    fs::remove_dir_all(&dir).ok();
}

// flip-check: retroactive s2-07l.283
/// (e) flag の 3 条件はそれぞれ単独で使い方の誤り: 空の値（`--anchor ''`）・`--` で始まる値（`--anchor --target`）・
/// 未知の flag（`--bogus x`）のどれも usage（rc 1・stderr・stdout 0 byte）で断り、file も event も書かない（3 形を別々に撃つ）。
#[test]
fn account_cmd_flags_refuse_empty_value_dashed_value_and_unknown_flag() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let before = tree(&dir);
    let usage = text(&run_account(&[]).stderr);
    assert!(usage.starts_with("usage: account <add <label>"), "{usage}");
    for (name, tail) in [("空の値", &["--anchor", ""][..]), ("-- で始まる値", &["--anchor", "--target"]), ("未知の flag", &["--bogus", "x"])] {
        let mut call = vec!["add", "a2", "--state-dir", &path];
        call.extend_from_slice(tail);
        let out = run_account(&call);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{name}: {out:?}");
        assert!(out.stdout.is_empty(), "{name}: stdout 0 byte");
        assert_eq!(text(&out.stderr), usage, "{name}: 使い方で断る");
        assert_eq!(tree(&dir), before, "{name}: file も event も不変");
    }
    fs::remove_dir_all(&dir).ok();
}

/// (3a) 期限切れの credential を偽 claude が書き換える周は、読み直して measured になり行の末尾に `refresh=ok`。
/// 起動は `-p` と `--max-turns 1`・口座の設定 dir を `CLAUDE_CONFIG_DIR` に・cwd は state dir。
#[test]
fn fleet_usage_refresh_rewritten_credential_is_measured_with_refresh_ok() {
    let fx = usage_fixture(&["a1"]);
    put_credential(&fx, "a1", &expired_credential("tok-old"));
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
    let claude = fake_claude(&fx, "cat \"{fresh}\" > \"$CLAUDE_CONFIG_DIR/.credentials.json\"\nexit 0");
    let out = run_usage_with_claude(&fx, &curl, &claude);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec![format!("{} refresh=ok", live_line("a1"))], "{out:?}");

    let args: Vec<String> = fs::read_to_string(fx.spy.join("claude-args"))
        .expect("偽 claude が argv を写した")
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(args.iter().filter(|arg| *arg == "-p").count(), 1, "-p で 1 回: {args:?}");
    assert_eq!(args.windows(2).filter(|pair| pair == &["--max-turns", "1"]).count(), 1, "--max-turns 1: {args:?}");
    let env = fs::read_to_string(fx.spy.join("claude-env")).expect("偽 claude が env を写した");
    let account = fx.state.join("accounts").join("a1");
    assert_eq!(env, format!("CLAUDE_CONFIG_DIR={}\n", account.display()), "口座の設定 dir");
    let cwd = fs::read_to_string(fx.spy.join("claude-cwd")).expect("偽 claude が cwd を写した");
    let state = fs::canonicalize(&fx.state).expect("state dir を解ける");
    assert_eq!(cwd, format!("{}\n", state.display()), "cwd は state dir");
    assert!(
        allowances(&fx).iter().all(|row| matches!(row, Allowance::Measured(_))),
        "event は measured の行だけ: {:?}",
        allowances(&fx)
    );
    let events = fs::read_to_string(store::events_path(&fx.state)).expect("event log が在る");
    assert!(!events.contains("refresh"), "event の行には載せない: {events}");
    drop_fixture(&fx);
}

/// (3b)(3c)(3e) 書き換えない偽 claude は rc 0 で `refresh=ok`・rc 7 は `refresh=rc:7`・実行 file 不在は
/// `refresh=unlaunchable`。いずれも `token_expired` のまま（client は起きない）。
#[test]
fn fleet_usage_refresh_failures_keep_token_expired_and_name_the_refresh() {
    for (name, tail, want) in [
        ("rc 0", Some("exit 0"), "refresh=ok"),
        ("rc 7", Some("exit 7"), "refresh=rc:7"),
        ("不在", None, "refresh=unlaunchable"),
    ] {
        let fx = usage_fixture(&["a1"]);
        put_credential(&fx, "a1", &expired_credential("tok-old"));
        let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
        let claude = match tail {
            Some(tail) => fake_claude(&fx, tail),
            None => fx.spy.join("no-such-claude"),
        };
        let out = run_usage_with_claude(&fx, &curl, &claude);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{name}: {out:?}");
        assert_eq!(
            out_lines(&out),
            vec![format!("usage: account=a1 unmeasured reason=token_expired {want}")],
            "{name}"
        );
        assert_eq!(claude_calls(&fx), usize::from(tail.is_some()), "{name}: 1 回だけ起こす");
        assert!(!fx.spy.join("args").exists(), "{name}: 期限切れのままの口座は client を起こさない");
        drop_fixture(&fx);
    }
}

/// refresh の timeout の歯が rules fixture に置く上限（秒）。1 s だと負荷下で子の起動より先に切れる
/// （.249 run 5 の再 gate 2026-09-14 12:08Z・load avg 17 で (d) が赤・main 単独では緑）ので数秒にする。
/// fixture の manifest の値であって rules 行の裁定ではない。
const REFRESH_TIMEOUT_S: u64 = 4;

/// 偽 claude が pid file を書くのを待つ上限（子が起動に達しない周を停止経路の失敗と混同しないための待ち）。
const PID_FILE_WAIT: Duration = Duration::from_secs(5);

/// 「上限で止める」の余裕（timeout + 猶予 2 回の和に足す・等号は壁時計で pin しない）。
const STOP_MARGIN_S: u64 = 12;

/// (d)(c)(e)(f) の fixture: 上限を [`REFRESH_TIMEOUT_S`] にした rules・期限切れの a1・偽 curl。偽 claude は呼び手が置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn refresh_timeout_fixture() -> (UsageFixture, PathBuf) {
    let fx = usage_fixture(&["a1"]);
    let short = usage_rules(&["a1"]).replace(
        &format!("value = {USAGE_TIMEOUT_S}"),
        &format!("value = {REFRESH_TIMEOUT_S}"),
    );
    fs::write(&fx.rules, short).expect("短い上限の rules fixture を書ける");
    put_credential(&fx, "a1", &expired_credential("tok-old"));
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
    (fx, curl)
}

/// 停止経路が要する壁時計の上限: 上限 + 猶予（埋め込みの `pipe.stop_grace_ms`・TERM 後と KILL 後の 2 回）+ 余裕。
fn refresh_stop_bound() -> Duration {
    let grace_ms = Manifest::embedded()
        .ok()
        .and_then(|manifest| manifest.get("pipe.stop_grace_ms").map(|row| row.value.clone()))
        .and_then(|value| match value {
            vessel::rules::RuleValue::Int(found) => Some(found),
            _ => None,
        })
        .unwrap_or(0);
    Duration::from_secs(REFRESH_TIMEOUT_S + STOP_MARGIN_S) + Duration::from_millis(grace_ms.saturating_mul(2))
}

/// 偽 claude が `{spy}/<who>` に書いた 1 行（trim 済み）。上限まで poll し、達しない周は `None`。
fn spy_line(fx: &UsageFixture, who: &str) -> Option<String> {
    let started = Instant::now();
    loop {
        if let Ok(text) = fs::read_to_string(fx.spy.join(who)) {
            let line = text.trim().to_owned();
            if !line.is_empty() {
                return Some(line);
            }
        }
        if started.elapsed() >= PID_FILE_WAIT {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// timeout の歯に共通の外形: rc 0・`refresh=timeout` の 1 行・停止経路の上限の内側で返る。
fn assert_refresh_timeout(out: &Output, elapsed: Duration) {
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(
        out_lines(out),
        vec!["usage: account=a1 unmeasured reason=token_expired refresh=timeout".to_owned()],
        "{out:?}"
    );
    let bound = refresh_stop_bound();
    assert!(elapsed < bound, "上限で止める（{bound:?} の内側）: {elapsed:?}");
}

/// `who`（`child` / `grandchild`）の pid が書かれていて、その `/proc` が無い。書かれていない周は
/// 「子が起動に達しない」で落ちる（停止経路の失敗と混同しない）。
#[expect(
    clippy::panic,
    reason = "統合 test の helper。clippy の allow-panic-in-tests は #[test] 関数の中だけに効く"
)]
fn assert_gone(fx: &UsageFixture, who: &str) {
    let Some(pid) = spy_line(fx, who) else {
        panic!("{who}: 子が起動に達しない（pid file が {PID_FILE_WAIT:?} で書かれない・停止経路の失敗ではない）");
    };
    assert!(!Path::new(&format!("/proc/{pid}")).exists(), "{who}（pid {pid}）が残らない");
}

/// `sh` / `sleep` / `kill` だけを引ける PATH（`systemd-run` の**無い** host＝`Unconfined(NoTool)`・scope の release が
/// 停止を肩代わりしない）。`pipe.rs` の `lean_path` と同じ作りだが、`kill` は sh の builtin なので `command -v` でなく
/// この process の PATH の dir を直接引く（builtin の名を link にすると自分を指す）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn stop_path(fx: &UsageFixture) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = fx.spy.join("stop-bin");
    fs::create_dir_all(&bin_dir).expect("stop dir を作れる");
    let dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|found| std::env::split_paths(&found).collect()).unwrap_or_default();
    for name in ["sh", "sleep", "kill"] {
        let real = dirs
            .iter()
            .map(|dir| dir.join(name))
            .find(|at| fs::metadata(at).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0));
        let real = real.expect("PATH の dir に実行 file が在る");
        std::os::unix::fs::symlink(&real, bin_dir.join(name)).expect("link を置ける");
    }
    bin_dir.display().to_string()
}

/// 孫つきで上限を超えて眠る偽 claude の本文（(d) と (c) で共用）。
const SLEEPING_GRANDCHILD: &str = "sleep 30 &\necho $! > \"{spy}/grandchild\"\necho $$ > \"{spy}/child\"\nwait";

/// (3d) 上限を超えて眠る偽 claude（孫を持つ）は group ごと止めて `refresh=timeout`。子と孫の両方が残らない
/// （包める host では scope の release が肩代わりしうる周の歯・停止経路そのものは (c)(e)(f) が測る）。
#[test]
fn fleet_usage_refresh_timeout_stops_the_child_and_its_grandchild() {
    let (fx, curl) = refresh_timeout_fixture();
    let claude = fake_claude(&fx, SLEEPING_GRANDCHILD);
    let started = Instant::now();
    let out = run_usage_with_claude(&fx, &curl, &claude);
    let elapsed = started.elapsed();
    assert_refresh_timeout(&out, elapsed);
    for who in ["child", "grandchild"] {
        assert_gone(&fx, who);
    }
    drop_fixture(&fx);
}

/// (c) **包めない PATH**（`systemd-run` 無し・`Unconfined(NoTool)`）で (d) を回す: scope の release が無いので、子と孫を
/// 消したのは器の停止経路（TERM → 猶予 → KILL）だけである。`.229` run 3 の生存 7 件（`stop_group` / `signal_group` /
/// `grace_of`）はこの周で落ちる（`s2-07l.255`）。現物の挙動を pin する歯なので base でも通る（retroactive）。
// flip-check: retroactive s2-07l.255
#[test]
fn fleet_usage_refresh_timeout_unconfined_stops_the_child_and_its_grandchild() {
    let (fx, curl) = refresh_timeout_fixture();
    let path = stop_path(&fx);
    let claude = fake_claude(&fx, SLEEPING_GRANDCHILD);
    let started = Instant::now();
    let out = run_usage_with_claude_on_path(&fx, &curl, &claude, &path);
    let elapsed = started.elapsed();
    assert_refresh_timeout(&out, elapsed);
    for who in ["child", "grandchild"] {
        assert_gone(&fx, who);
    }
    drop_fixture(&fx);
}

/// (e) TERM に応じる子（trap で `{spy}/term` を書いて rc 0 で終える）: 包めない PATH で `refresh=timeout`・`term` が在る
/// （TERM が届いた証拠・`signal_group` を空にすると無い）・子が残らない。実 signal の宛先は器が自分で起こした子の
/// group だけ（fixture が pid を選ばない・N1）。現物の挙動を pin する歯なので base でも通る（retroactive）。
// flip-check: retroactive s2-07l.255
#[test]
fn fleet_usage_refresh_timeout_unconfined_term_reaches_the_child() {
    let (fx, curl) = refresh_timeout_fixture();
    let path = stop_path(&fx);
    let claude = fake_claude(
        &fx,
        "trap 'echo term > \"{spy}/term\"; exit 0' TERM\necho $$ > \"{spy}/child\"\nsleep 30 &\nwait",
    );
    let started = Instant::now();
    let out = run_usage_with_claude_on_path(&fx, &curl, &claude, &path);
    let elapsed = started.elapsed();
    assert_refresh_timeout(&out, elapsed);
    assert_gone(&fx, "child");
    assert_eq!(spy_line(&fx, "term").as_deref(), Some("term"), "TERM が子に届いた");
    drop_fixture(&fx);
}

/// (f) TERM を無視する子（`trap '' TERM` を孫の `sleep` にも継がせる）: 包めない PATH で `refresh=timeout`・子と孫が
/// 残らない（KILL が届いた証拠・KILL の行を消すと 30 s 眠り続ける）。現物の挙動を pin する歯なので base でも通る（retroactive）。
// flip-check: retroactive s2-07l.255
#[test]
fn fleet_usage_refresh_timeout_unconfined_kills_the_child_that_ignores_term() {
    let (fx, curl) = refresh_timeout_fixture();
    let path = stop_path(&fx);
    let claude = fake_claude(&fx, &format!("trap '' TERM\n{SLEEPING_GRANDCHILD}"));
    let started = Instant::now();
    let out = run_usage_with_claude_on_path(&fx, &curl, &claude, &path);
    let elapsed = started.elapsed();
    assert_refresh_timeout(&out, elapsed);
    for who in ["child", "grandchild"] {
        assert_gone(&fx, who);
    }
    drop_fixture(&fx);
}

/// (3f)(3g) fresh な credential と墓標では偽 claude を起こさない（argv の写しが無い）。
#[test]
fn fleet_usage_refresh_is_not_attempted_for_fresh_or_tombstone() {
    let fx = usage_fixture(&["a1", "tomb"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    put_credential(&fx, "tomb", r#"{"claudeAiOauth":{"accessToken":"tok-tomb","expiresAt":0}}"#);
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
    let claude = fake_claude(&fx, "exit 0");
    let out = run_usage_with_claude(&fx, &curl, &claude);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(
        out_lines(&out),
        vec![live_line("a1"), "usage: account=tomb unmeasured reason=tombstone".to_owned()],
        "試みない周は refresh= を足さない"
    );
    assert!(!fx.spy.join("claude-args").exists(), "偽 claude は起こされない");
    drop_fixture(&fx);
}

/// (3h) 2 口座のうち 1 つだけ期限切れなら、起動は 1 回（その口座の設定 dir で）。
#[test]
fn fleet_usage_refresh_launches_once_for_the_single_expired_account() {
    let fx = usage_fixture(&["a1", "a2"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    put_credential(&fx, "a2", &expired_credential("tok-old"));
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
    let claude = fake_claude(&fx, "exit 0");
    let out = run_usage_with_claude(&fx, &curl, &claude);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(
        out_lines(&out),
        vec![live_line("a1"), "usage: account=a2 unmeasured reason=token_expired refresh=ok".to_owned()]
    );
    assert_eq!(claude_calls(&fx), 1, "起動は 1 回");
    let env = fs::read_to_string(fx.spy.join("claude-env")).expect("偽 claude が env を写した");
    assert_eq!(env, format!("CLAUDE_CONFIG_DIR={}\n", fx.state.join("accounts").join("a2").display()));
    drop_fixture(&fx);
}

/// `fleet select` の歯の 5 時間窓の reset（遠い未来＝どの「いま」でも古くない）。
const SELECT_FIVE_RESET: &str = "2099-01-01T05:00:00Z";

/// 3 口座: a1 = 30（5h）・a2 = 70（7d）・a3 = 100（5h・当たっている）。
const SELECT_THREE: &[(&str, u64, u64)] = &[("a1", 30, 10), ("a2", 20, 70), ("a3", 100, 5)];

/// `fleet select` の rules fixture。待ち時間の行（`timeout`）と R-C9-1 の行（値の字面 `selection`）を持ち分ける。
fn select_rules(labels: &[&str], timeout: bool, selection: Option<&str>) -> String {
    let mut text = "schema = 1\n".to_owned();
    if timeout {
        text.push_str(&format!(
            "\n[[rule]]\nid = \"fleet.usage_timeout_s\"\nkind = \"UsageTimeoutS\"\nvalue = {USAGE_TIMEOUT_S}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n"
        ));
    }
    if let Some(value) = selection {
        text.push_str(&format!(
            "\n[[rule]]\nid = \"R-C9-1\"\nkind = \"AccountSelection\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n"
        ));
    }
    for label in labels {
        text.push_str(&format!("\n[[account]]\nlabel = \"{label}\"\n"));
    }
    text
}

/// 5 時間窓と 7 日窓だけの本文（`limits` は空＝モデル別の行なし）。
fn select_body(five: u64, seven: u64) -> String {
    format!(
        r#"{{"five_hour":{{"utilization":{five},"resets_at":"{SELECT_FIVE_RESET}"}},"seven_day":{{"utilization":{seven},"resets_at":"2099-01-07T00:00:00+00:00"}},"limits":[]}}"#
    )
}

/// 口座ごとに違う本文を返す偽 curl。stdin の token で `body-<token>` を選び、argv は `args` へ追記で写す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn token_curl(fx: &UsageFixture) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let d = fx.spy.display().to_string();
    let script = format!(
        "#!/bin/sh\n\
         printf '%s\\n' \"$@\" >> \"{d}/args\"\n\
         cfg=$(cat)\n\
         for f in \"{d}\"/body-*; do\n\
         case \"$cfg\" in *\"Bearer ${{f##*/body-}}\\\"\"*) cat \"$f\" ;; esac\n\
         done\n\
         printf '\\n%s' '200'\n\
         exit 0\n"
    );
    let path = fx.spy.join("token-curl");
    fs::write(&path, script).expect("fake を書ける");
    let mut perm = fs::metadata(&path).expect("fake の権限を読める").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("fake を実行可能にできる");
    path
}

/// 口座ごとに (label, 5h %, 7d %) を返す置き場と偽 curl。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn select_fixture(accounts: &[(&str, u64, u64)], timeout: bool, selection: Option<&str>) -> (UsageFixture, PathBuf) {
    let labels: Vec<&str> = accounts.iter().map(|(label, _, _)| *label).collect();
    let fx = usage_fixture(&labels);
    fs::write(&fx.rules, select_rules(&labels, timeout, selection)).expect("rules fixture を書ける");
    for (label, five, seven) in accounts {
        let token = format!("tok-{label}");
        put_credential(&fx, label, &live_credential(&token));
        fs::write(fx.spy.join(format!("body-{token}")), select_body(*five, *seven)).expect("本文を書ける");
    }
    let curl = token_curl(&fx);
    (fx, curl)
}

/// `fleet select` を fixture の置き場・rules・client で撃つ。
fn run_select(fx: &UsageFixture, curl: &Path, extra: &[&str]) -> Output {
    let state = fx.state.display().to_string();
    let rules = fx.rules.display().to_string();
    let curl = curl.display().to_string();
    let mut args = vec!["select", "--state-dir", &state, "--rules", &rules, "--curl", &curl];
    args.extend_from_slice(extra);
    run_fleet(&args)
}

/// 偽 curl が呼ばれた回数（口座 1 つにつき 1 回）。
fn curl_calls(fx: &UsageFixture) -> usize {
    fs::read_to_string(fx.spy.join("args"))
        .unwrap_or_default()
        .lines()
        .filter(|arg| *arg == "--max-time")
        .count()
}

/// (1) 便用は当たっていない口座のうち reset が最も早いもの（`SELECT_THREE` は全口座が同じ reset → 便数 0 → label の
/// 先頭 a1・ADR-0027 §2.2）を出し、stdout は同じ log を渡した純関数の 1 行と一致する。
#[test]
fn fleet_select_run_prints_the_earliest_reset_unlimited_account() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()], "同じ reset → label・a3 は 100 で当たっている");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: account=a2 five_hour=20%"), "計測の行は stderr へ: {stderr}");

    let events = store::read_all(&fx.state).expect("event log を読める");
    let state = replay(&events);
    let labels: Vec<String> = ["a1", "a2", "a3"].iter().map(|label| (*label).to_owned()).collect();
    let exclude = BTreeSet::new();
    let found = select::select(&Input {
        labels: &labels,
        allowance: &state.allowance,
        purpose: Purpose::Run,
        model: None,
        exclude: &exclude,
        inflight: &BTreeMap::new(),
        threshold_pct: 85,
        now: "2026-09-13T00:00:00Z",
        prefer: None,
    });
    assert_eq!(found, Selection::Chosen("a1".to_owned()), "純関数の答え");
    assert_eq!(out_lines(&out), vec![select::line(Purpose::Run, &found)], "stdout は純関数の 1 行");
    drop_fixture(&fx);
}

/// 5 時間窓の reset を口座ごとに変えた本文（`select_body` の reset 違い）。
fn select_body_resetting(five: u64, seven: u64, five_reset: &str) -> String {
    format!(
        r#"{{"five_hour":{{"utilization":{five},"resets_at":"{five_reset}"}},"seven_day":{{"utilization":{seven},"resets_at":"2099-01-07T00:00:00+00:00"}},"limits":[]}}"#
    )
}

/// (h) CLI の便用は逼迫度でなく **reset が最も近い口座**を選ぶ（ADR-0027 §2.2・C9.2）: a1（5h 20%・reset 2099-01-01T01Z）と
/// a2（5h 80%・reset 2099-01-01T04Z）→ `chosen=a1`。base（逼迫度最大）は a2 → RED。実測行は `fleet select` が撃つ
/// 計測（偽 curl の口座ごとの本文）で置く。
#[test]
fn fleet_select_run_prefers_earliest_reset_over_pressure() {
    let (fx, curl) = select_fixture(&[("a1", 20, 10), ("a2", 80, 10)], true, Some("85"));
    fs::write(fx.spy.join("body-tok-a1"), select_body_resetting(20, 10, "2099-01-01T01:00:00+00:00")).expect("本文を書ける");
    fs::write(fx.spy.join("body-tok-a2"), select_body_resetting(80, 10, "2099-01-01T04:00:00+00:00")).expect("本文を書ける");
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()], "reset が近い a1（逼迫度なら a2）: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: account=a1 five_hour=20% resets=2099-01-01T01:00:00Z"), "a1 の reset: {stderr}");
    assert!(stderr.contains("usage: account=a2 five_hour=80% resets=2099-01-01T04:00:00Z"), "a2 の reset: {stderr}");
    let out = run_select(&fx, &curl, &["--purpose", "session"]);
    assert_eq!(out_lines(&out), vec!["select purpose=session chosen=a1".to_owned()], "session 用は逼迫度の最小（同じ答えだが鍵が違う）");
    // 逆に a2 の reset を近くすれば a2（逼迫度 80 でも当たってはいない）。
    fs::write(fx.spy.join("body-tok-a2"), select_body_resetting(80, 10, "2099-01-01T00:30:00+00:00")).expect("本文を書ける");
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a2".to_owned()], "reset が近い側へ動く: {out:?}");
    drop_fixture(&fx);
}

/// 便の event 1 件（`event` の run と bead に段・席・口座を足す）。
fn run_event(kind: EventKind, run: &str, stage: Option<Stage>, account: Option<&str>) -> Event {
    Event {
        stage,
        seat: Some(format!("seat-{run}")),
        account: account.map(str::to_owned),
        ..event(kind, run, "2026-09-15T00:00:00Z")
    }
}

/// (g) `State::inflight_by_account` は終端でない便のうち `account` を持つものを label ごとに数える（ADR-0027 §2.3）:
/// SeatSpawned(account=a1) ×2（1 本は Landed 済み）+ SeatSpawned(account 無し) → `{a1: 1}`。Stopped / Failed /
/// `detail=retired` も数えず、最新の `SeatSpawned` の値が勝つ（起こし直しで口座が変わる）。
#[test]
fn fleet_replay_counts_inflight_runs_per_account() {
    let mut events = vec![
        run_event(EventKind::SeatSpawned, "r1", Some(Stage::Spawned), Some("a1")),
        run_event(EventKind::SeatSpawned, "r2", Some(Stage::Spawned), Some("a1")),
        run_event(EventKind::RunDone, "r2", Some(Stage::Landed), None),
        run_event(EventKind::SeatSpawned, "r3", Some(Stage::Spawned), None),
    ];
    let state = replay(&events);
    assert_eq!(state.inflight_by_account(), BTreeMap::from([("a1".to_owned(), 1)]), "r2 は Landed・r3 は口座不明");
    assert_eq!(state.runs.get("r1").and_then(|run| run.account.clone()), Some("a1".to_owned()));
    assert_eq!(state.runs.get("r2").and_then(|run| run.account.clone()), Some("a1".to_owned()), "終端でも口座は残る");
    assert_eq!(state.runs.get("r3").and_then(|run| run.account.clone()), None);
    assert_eq!(state.runs.len(), 3, "口座つきの SeatSpawned は便に紐づく行（幽霊の便を作らない）");
    // 段が進んでも走行中（Implemented / Gated / RateLimited）・終端（Stopped / Failed）と retired は数えない。
    events.push(run_event(EventKind::RunStage, "r1", Some(Stage::Gated), None));
    events.push(run_event(EventKind::SeatSpawned, "r4", Some(Stage::Spawned), Some("a2")));
    events.push(run_event(EventKind::RunStopped, "r4", Some(Stage::Stopped), None));
    events.push(run_event(EventKind::SeatSpawned, "r5", Some(Stage::Spawned), Some("a2")));
    events.push(run_event(EventKind::RunStage, "r5", Some(Stage::Failed), None));
    events.push(run_event(EventKind::SeatSpawned, "r6", Some(Stage::RateLimited), Some("a2")));
    events.push(run_event(EventKind::SeatSpawned, "r7", Some(Stage::Spawned), Some("a3")));
    events.push(Event { detail: Some("retired".to_owned()), ..run_event(EventKind::RunStage, "r7", None, None) });
    let state = replay(&events);
    assert_eq!(
        state.inflight_by_account(),
        BTreeMap::from([("a1".to_owned(), 1), ("a2".to_owned(), 1)]),
        "Gated の r1・RateLimited の r6 は走行中・Stopped / Failed / retired は数えない"
    );
    // 起こし直しで口座が変わる: 最新の SeatSpawned の値。
    events.push(run_event(EventKind::SeatSpawned, "r1", Some(Stage::Spawned), Some("a3")));
    let state = replay(&events);
    assert_eq!(state.inflight_by_account(), BTreeMap::from([("a2".to_owned(), 1), ("a3".to_owned(), 1)]), "r1 は a3 へ");
    assert_eq!(replay(&[]).inflight_by_account(), BTreeMap::new(), "便 0 は空");
}

/// (f) `fleet record --kind SeatSpawned --account x` は行に `"account":"x"` を書き、読み返した便が口座を持つ。
/// `--kind RunStage --account x` は rc 1（他の kind の `account` は malformed のまま・書かない）。`--account` の
/// 値欠けも rc 1。field の無い SeatSpawned はこれまでどおり書けて `account` 無しで読める（schema 1 のまま）。
#[test]
fn fleet_record_seat_spawned_carries_account() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let spawned = run_fleet(&[
        "record", "--kind", "SeatSpawned", "--run", "r1", "--bead", "s2-x", "--seat", "s1", "--account", "x",
        "--state-dir", &path,
    ]);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "{spawned:?}");
    let log = fs::read_to_string(store::events_path(&dir)).expect("event log が在る");
    assert_eq!(log.lines().count(), 1);
    assert!(log.contains(r#""account":"x""#), "{log}");
    assert!(log.contains(r#""run":"r1""#) && log.contains(r#""bead":"s2-x""#), "便に紐づく行のまま: {log}");
    for bad in [
        &["record", "--kind", "RunStage", "--run", "r1", "--bead", "s2-x", "--stage", "Gated", "--account", "x", "--state-dir", &path][..],
        &["record", "--kind", "RunCreated", "--run", "r2", "--bead", "s2-x", "--account", "x", "--state-dir", &path][..],
        &["record", "--kind", "SeatStopped", "--run", "r1", "--bead", "s2-x", "--account", "x", "--state-dir", &path][..],
        &["record", "--kind", "SeatSpawned", "--run", "r1", "--bead", "s2-x", "--account", "--state-dir", &path][..],
    ] {
        let refused = run_fleet(bad);
        assert_eq!(refused.status.code(), Some(i32::from(RC_REFUSED)), "{bad:?}: {refused:?}");
        assert!(refused.stdout.is_empty(), "{bad:?}: 書かない");
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert!(stderr.contains("--account"), "{bad:?}: 理由は flag を名指す: {stderr}");
    }
    let plain = run_fleet(&["record", "--kind", "SeatSpawned", "--run", "r2", "--bead", "s2-x", "--state-dir", &path]);
    assert_eq!(plain.status.code(), Some(i32::from(RC_OK)), "{plain:?}");
    let events = store::read_all(&dir).expect("全行を読める");
    assert_eq!(events.len(), 2, "断った周は書いていない");
    assert_eq!(events.first().and_then(|found| found.account.clone()), Some("x".to_owned()));
    assert_eq!(events.get(1).and_then(|found| found.account.clone()), None, "field の無い行は None");
    assert_eq!(replay(&events).inflight_by_account(), BTreeMap::from([("x".to_owned(), 1)]));
    fs::remove_dir_all(&dir).ok();
}

/// 読み手: `account` の例外は `SeatSpawned` だけ——他の便の kind の生の行に在れば malformed のまま・`SeatSpawned` でも
/// 文字列でなければ malformed・key の無い旧い行は `None`（schema 1 のまま値の追加・ADR-0004 §2.5 D-5）。
#[test]
fn fleet_seat_spawned_account_is_the_only_exception_for_run_kinds() {
    let line = |kind: &str, account: &str| {
        format!(r#"{{"schema":1,"ts":"2026-09-15T00:00:00Z","kind":"{kind}","run":"r1","bead":"s2-x",{account}"host":"h","actor":"machine"}}"#)
    };
    let spawned = Event::from_line(&line("SeatSpawned", r#""account":"x","#)).expect("SeatSpawned の account は読める");
    assert_eq!(spawned.account, Some("x".to_owned()));
    assert_eq!(spawned.run, "r1");
    assert_eq!(spawned.to_line(), line("SeatSpawned", r#""account":"x","#), "書いて読んで同じ行（account は run / bead の後）");
    let old = Event::from_line(&line("SeatSpawned", "")).expect("旧い行は読める");
    assert_eq!(old.account, None);
    assert_eq!(old.to_line(), line("SeatSpawned", ""), "None は key ごと書かない");
    for kind in ["SeatStopped", "RunStage", "RunCreated", "RunDone", "RunStopped", "ApprovalRequested", "QuestionRaised"] {
        let reason = Event::from_line(&line(kind, r#""account":"x","#)).expect_err(kind);
        assert!(reason.contains("account を持たない"), "{kind}: {reason}");
    }
    let reason = Event::from_line(&line("SeatSpawned", r#""account":7,"#)).expect_err("文字列でない account");
    assert!(reason.contains("account"), "{reason}");
    let reason = Event::from_line(&line("SeatSpawned", r#""account":"x","window":"five_hour","#)).expect_err("口座残量の key");
    assert!(reason.contains("window を持たない"), "{reason}");
}

/// 5 時間窓に消費の無い口座 a1（`five_hour` の reset が null）と、使用中の口座 a2（5h 50%）の置き場。
/// a1 の 5 時間窓の本文は `idle_five` の字面で差し替える。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn idle_window_fixture(idle_five: &str) -> (UsageFixture, PathBuf) {
    let (fx, curl) = select_fixture(&[("a1", 0, 10), ("a2", 50, 10)], true, Some("85"));
    let body = format!(
        r#"{{"five_hour":{idle_five},"seven_day":{{"utilization":10.0,"resets_at":"2099-01-07T00:00:00+00:00"}},"limits":[]}}"#
    );
    fs::write(fx.spy.join("body-tok-a1"), body).expect("本文を書ける");
    (fx, curl)
}

/// 口座 a1 の 5 時間窓の最新の行。
fn five_hour_of_a1(fx: &UsageFixture) -> Option<Allowance> {
    allowances(fx)
        .into_iter()
        .rfind(|row| row.key() == allowance_key("a1", Some(WindowKind::FiveHour), None))
}

/// 5 時間窓が `{utilization: 0.0, resets_at: null}` の口座は `AllowanceMeasured used_pct=0`（reset 無し）で記録され、
/// 便用の選定の候補に入る（ADR-0024 §2.1 / §2.2）。reset 無しは 1 行表示で `resets=none`。
#[test]
fn fleet_usage_idle_window_null_reset_is_measured_zero_and_a_run_candidate() {
    let (fx, curl) = idle_window_fixture(r#"{"utilization":0.0,"resets_at":null}"#);
    let out = run_select(&fx, &curl, &["--purpose", "run", "--exclude", "a2"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()], "消費の無い口座は候補: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("usage: account=a1 five_hour=0% resets=none seven_day=10% resets=2099-01-07T00:00:00Z"),
        "reset 無しは明示の字面: {stderr}"
    );
    assert_eq!(
        five_hour_of_a1(&fx),
        Some(Allowance::Measured(Measured {
            account: "a1".to_owned(),
            window: WindowKind::FiveHour,
            model: None,
            endpoint: "oauth-usage".to_owned(),
            used_pct: 0,
            resets_at: None,
        })),
        "測れた 0%・reset 無し"
    );
    let events = fs::read_to_string(store::events_path(&fx.state)).expect("event log が在る");
    let idle_line = events
        .lines()
        .find(|line| line.contains(r#""account":"a1""#) && line.contains(r#""window":"five_hour""#))
        .expect("a1 の 5 時間窓の行が在る");
    assert!(idle_line.contains(r#""kind":"AllowanceMeasured""#), "{idle_line}");
    assert!(!idle_line.contains("resets_at"), "reset 無しの周は key を出さない: {idle_line}");
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a2".to_owned()], "便用は reset が早い側（a2 の 5h・a1 は 7d の reset だけ）");
    drop_fixture(&fx);
}

/// 5 時間窓の reset が null でも使用率が 0 でなければ（`utilization: 3.0`）ShapeMismatch のまま＝候補に入らない。
#[test]
fn fleet_usage_idle_window_nonzero_without_reset_stays_shape_mismatch() {
    let (fx, curl) = idle_window_fixture(r#"{"utilization":3.0,"resets_at":null}"#);
    let out = run_select(&fx, &curl, &["--purpose", "run", "--exclude", "a2"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(
        out_lines(&out),
        vec!["select purpose=run none=unmeasured earliest_reset=-".to_owned()],
        "0 以外を reset 無しで記録しない: {out:?}"
    );
    assert_eq!(
        five_hour_of_a1(&fx),
        Some(unmeasured_window("a1", WindowKind::FiveHour)),
        "shape_mismatch の Unmeasured"
    );
    drop_fixture(&fx);
}

/// 窓 1 つの ShapeMismatch（`fleet usage` の出所の識別子）。
fn unmeasured_window(account: &str, window: WindowKind) -> Allowance {
    Allowance::Unmeasured(Unmeasured {
        account: account.to_owned(),
        window: Some(window),
        model: None,
        endpoint: "oauth-usage".to_owned(),
        reason: UnmeasuredReason::ShapeMismatch,
    })
}

/// (2) `--exclude`（複数可）で席の口座を外す。外した残りが当たっていれば候補なし（rc 0）。
#[test]
fn fleet_select_exclude_drops_the_seat_accounts() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    let out = run_select(&fx, &curl, &["--purpose", "run", "--exclude", "a2"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()]);
    let out = run_select(&fx, &curl, &["--purpose", "run", "--exclude", "a2", "--exclude", "a1"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "候補なしは断りではない: {out:?}");
    assert_eq!(
        out_lines(&out),
        vec![format!("select purpose=run none=all-limited earliest_reset={SELECT_FIVE_RESET}")]
    );
    let calls = curl_calls(&fx);
    let out = run_select(&fx, &curl, &["--purpose", "run", "--exclude"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "値の無い --exclude は断る: {out:?}");
    assert!(out.stdout.is_empty(), "選ばない");
    assert_eq!(curl_calls(&fx), calls, "断った周は計測しない");
    drop_fixture(&fx);
}

/// `--exclude` の直後に別の flag が来る周（`--exclude --purpose run`）は **値欠け**で usage に断られる
/// （rc 1・chosen を出さない・次の flag を label に取らない・計測しない）。`--` で始まる字面は label にならない。
///
/// .191 の検出線で生き残った変異 `excludes` の match guard `!label.starts_with("--")` → `true` は、この周だけ
/// 挙動が変わる（`--purpose` が label に化けて選定が通り chosen を出す）。現物の挙動を pin する歯なので base
/// でも通る（retroactive）。
// flip-check: retroactive s2-07l.196
#[test]
fn mutant_e2e_fleet_select_exclude_followed_by_a_flag_is_a_missing_value() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    for extra in [
        &["--exclude", "--purpose", "run"][..],
        &["--purpose", "run", "--exclude", "--model", "Fable"],
        &["--purpose", "run", "--exclude", "a2", "--exclude", "--purpose", "run"],
        &["--purpose", "run", "--exclude", "--"],
    ] {
        let out = run_select(&fx, &curl, extra);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{extra:?}: 値欠けは断る: {out:?}");
        assert!(out.stdout.is_empty(), "{extra:?}: chosen を出さない: {out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("fleet: --exclude に値が無い"), "{extra:?}: 値欠けの理由: {stderr}");
        assert!(stderr.contains("usage: fleet"), "{extra:?}: 使い方を stderr へ: {stderr}");
        assert!(!stderr.contains("chosen="), "{extra:?}: 次の flag を label に取って選ばない: {stderr}");
    }
    assert_eq!(curl_calls(&fx), 0, "断った周は計測しない");
    assert!(!store::events_path(&fx.state).exists(), "event を書かない");
    let out = run_select(&fx, &curl, &["--exclude", "a2", "--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "値の在る `--exclude` は flag の前でも通る: {out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()]);
    drop_fixture(&fx);
}

/// (3) session 用は閾値未満で最小の口座を出す。全口座が閾値以上（当たってはいない）の周は
/// `all-limited` でなく閾値の理由。
#[test]
fn fleet_select_session_keeps_headroom_and_names_the_threshold_reason() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    let out = run_select(&fx, &curl, &["--purpose", "session"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=session chosen=a1".to_owned()]);
    drop_fixture(&fx);

    let (fx, curl) = select_fixture(&[("a1", 85, 0), ("a2", 90, 10), ("a3", 99, 99)], true, Some("85"));
    let out = run_select(&fx, &curl, &["--purpose", "session"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    let lines = out_lines(&out);
    assert_eq!(lines, vec!["select purpose=session none=over-threshold earliest_reset=-".to_owned()], "閾値ちょうども候補外");
    assert!(lines.iter().all(|line| !line.contains("all-limited")), "当たってはいない: {lines:?}");
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()], "便用は閾値を持たない（同じ reset → label）");
    drop_fixture(&fx);
}

/// (4) 選定の前に計測が 1 回（口座ごとに偽 curl 1 回・実測行が 1 周分）走る。
#[test]
fn fleet_select_measures_once_before_selecting() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    assert_eq!(curl_calls(&fx), 0, "撃つ前は 0 回");
    assert!(allowances(&fx).is_empty(), "撃つ前は実測行なし");
    let first = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "{first:?}");
    assert_eq!(curl_calls(&fx), 3, "3 口座 × 1 周");
    assert_eq!(allowances(&fx).len(), 6, "3 口座 × 2 窓");
    let second = run_select(&fx, &curl, &["--purpose", "session"]);
    assert_eq!(second.status.code(), Some(i32::from(RC_OK)), "{second:?}");
    assert_eq!(curl_calls(&fx), 6, "撃つたびに 1 周");
    assert_eq!(allowances(&fx).len(), 12, "撃つたびに 1 周分");
    drop_fixture(&fx);
}

/// (5) 計測が撃てない周（待ち時間の行が無い）は `UsageError` の rc で、選ばない・書かない。
#[test]
fn fleet_select_refuses_with_the_usage_error_when_measurement_cannot_run() {
    let (fx, curl) = select_fixture(SELECT_THREE, false, Some("85"));
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "UsageError::Manifest の rc: {out:?}");
    assert!(out.stdout.is_empty(), "選ばない");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("fleet usage:") && stderr.contains("fleet.usage_timeout_s"), "計測の断り: {stderr}");
    assert_eq!(curl_calls(&fx), 0, "client を起こさない");
    assert!(!store::events_path(&fx.state).exists(), "event を書かない");
    drop_fixture(&fx);
}

/// (6) `--purpose` の未知の値・値欠け・欠落は usage で断る（計測しない）。
#[test]
fn fleet_select_refuses_unknown_purpose_with_usage() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    for extra in [&["--purpose", "lane"][..], &["--purpose"][..], &[][..]] {
        let out = run_select(&fx, &curl, extra);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{extra:?}: {out:?}");
        assert!(out.stdout.is_empty(), "{extra:?}: 選ばない");
        assert!(String::from_utf8_lossy(&out.stderr).contains("usage: fleet"), "{extra:?}: 使い方を stderr へ");
    }
    assert_eq!(curl_calls(&fx), 0, "計測しない");
    assert!(!store::events_path(&fx.state).exists(), "event を書かない");
    drop_fixture(&fx);
}

/// `--model` は閉じた表（`Model::parse`・別名か表示名・`s2-07l.297`）で受ける: 表に無い値（`OPUS` / `claude-opus-5` /
/// `nope`）は usage で typed に断り（rc 1・値を名指す・計測しない）、別名 `opus` と表示名 `Opus` はどちらも通って
/// 同じ選定になる（usage の 1 行は不変＝fleet の外形 snapshot は動かない）。
#[test]
fn fleet_select_model_must_be_in_the_closed_table() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    for bad in ["OPUS", "claude-opus-5", "nope"] {
        let out = run_select(&fx, &curl, &["--purpose", "run", "--model", bad]);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{bad}: {out:?}");
        assert!(out.stdout.is_empty(), "{bad}: 選ばない");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(&format!("fleet: model {bad} は未知である")), "{bad}: 値を名指す: {stderr}");
        assert!(stderr.contains("Opus") && stderr.contains("Fable"), "{bad}: 取る名を名指す: {stderr}");
        assert!(stderr.contains("usage: fleet"), "{bad}: 使い方を stderr へ: {stderr}");
    }
    assert_eq!(curl_calls(&fx), 0, "断った周は計測しない");
    assert!(!store::events_path(&fx.state).exists(), "event を書かない");
    for good in ["opus", "Opus", "fable", "Sonnet", "haiku"] {
        let out = run_select(&fx, &curl, &["--purpose", "run", "--model", good]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{good}: {out:?}");
        assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()], "{good}: モデル別の行が無い表では model に依らない");
    }
    drop_fixture(&fx);
}

/// (7) R-C9-1 の欠落・散文の値は `RuleError` で断る（計測しない）。
#[test]
fn fleet_select_refuses_rules_without_the_selection_row() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, None);
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{out:?}");
    assert!(out.stdout.is_empty(), "選ばない");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("rules: R-C9-1 が無い"), "RuleError の行: {stderr}");
    assert_eq!(curl_calls(&fx), 0, "計測しない");
    drop_fixture(&fx);

    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("\"新規投入は 5h 線\""));
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("rules:") && stderr.contains("形と合わない"), "形の不一致: {stderr}");
    assert_eq!(curl_calls(&fx), 0, "計測しない");
    drop_fixture(&fx);
}
