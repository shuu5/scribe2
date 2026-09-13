//! fleet event log の歯（設計 docs/design/fleet-event-log.md §6）。
//!
//! 置き場は毎回 tmp dir を `--state-dir` で指す（env も HOME も読まない形の裏返し）。

use crate::make_tmp_dir;
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
    let form = format!(
        "{}{}{}{}",
        String::from_utf8_lossy(&usage.stderr),
        String::from_utf8_lossy(&missing.stderr),
        String::from_utf8_lossy(&empty.stdout),
        String::from_utf8_lossy(&recorded.stdout)
    )
    .replace(&vessel::fleet::cli::host(), "[host]");
    insta::assert_snapshot!(form);
    fs::remove_dir_all(&dir).ok();
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

/// `RateLimited` は `Questioned` の直後に並び（母集団 10 段）、字面が往復する（`s2-07l.190`・
/// 設計 account-autonomy.md §2）。
#[test]
fn fleet_stages_place_rate_limited_after_questioned() {
    let at = |want: Stage| STAGES.iter().position(|stage| *stage == want);
    assert_eq!(STAGES.len(), 10, "段は 10 個");
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
        resets_at: RESETS_AT.to_owned(),
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
            "resets_at が無いか文字列でない",
        ),
        (
            vec![window("five_hour"), pct.clone()],
            "resets_at が無いか文字列でない",
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

/// `KINDS` は 13 variant で並びは宣言順のまま（歯 (a)(7)・席の登録 1 を末尾に足した）。
#[test]
fn fleet_allowance_kinds_are_thirteen_in_declaration_order() {
    assert_eq!(KINDS.len(), 13, "母集団（既存 10 + 口座残量 2 + 席の登録 1）");
    assert_eq!(KINDS.last(), Some(&EventKind::SeatRegistered), "登録は末尾");
    assert!(!EventKind::SeatRegistered.is_allowance(), "登録は口座残量の kind ではない");
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

/// 席の登録の event（本体は `registration` の束）。
fn registration_event(target: &str) -> Event {
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
            sid: "sid-1".to_owned(),
            account: "a1".to_owned(),
            launch: "line 1\n\"line 2\"\n".to_owned(),
        }),
    }
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
    let missing = line.replacen(",\"sid\":\"sid-1\"", "", 1);
    assert!(Event::from_line(&missing).is_err(), "項目の欠けは malformed: {missing}");
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
    let mut seen: Vec<(String, WindowKind, Option<String>, u64, String)> = allowances(&fx)
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
            (label.to_owned(), WindowKind::FiveHour, None, 13, "2026-09-12T05:00:00Z".to_owned()),
            (label.to_owned(), WindowKind::SevenDay, None, 41, "2026-09-18T00:00:00Z".to_owned()),
            (label.to_owned(), WindowKind::SevenDayModel, Some("Fable".to_owned()), 38, "2026-09-18T00:00:00Z".to_owned()),
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
    let out = run_usage(&fx, &curl, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(
        out_lines(&out),
        vec![
            "usage: account=tomb unmeasured reason=tombstone",
            "usage: account=old unmeasured reason=token_expired",
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

/// (7) `[[account]]` 0 行の manifest は rc 1・stdout 0 byte・何も書かない。
#[test]
fn fleet_usage_refuses_manifest_without_accounts() {
    let fx = usage_fixture(&[]);
    let curl = fake_curl(&fx, LIVE_BODY, "200", 0);
    let out = run_usage(&fx, &curl, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{out:?}");
    assert!(out.stdout.is_empty(), "stdout は 0 byte");
    assert!(String::from_utf8_lossy(&out.stderr).contains("[[account]]"), "理由を stderr へ");
    assert!(!store::events_path(&fx.state).exists(), "event を書かない");
    let shown = run_usage(&fx, &curl, &["--show"]);
    assert_eq!(shown.status.code(), Some(i32::from(RC_REFUSED)), "--show も同じく断る");
    assert!(shown.stdout.is_empty());
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
