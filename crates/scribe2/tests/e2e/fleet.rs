//! fleet event log の歯（設計 docs/design/fleet-event-log.md §6）。
//!
//! 置き場は毎回 tmp dir を `--state-dir` で指す（env も HOME も読まない形の裏返し）。

use crate::make_tmp_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};
use vessel::order::is_declaration_order;
use vessel::fleet::store::{self, LockPolicy, StoreError};
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
use vessel::rules::manifest::Manifest;
use vessel::fleet::{KINDS, STAGES};
use vessel::fleet::{
    json_lite, replay, wait, Completion, Event, EventKind, SeatState, Stage, Timeout, SCHEMA,
};

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
        };
        let line = event.to_line();
        assert!(line.contains("\"schema\":1"), "{line}");
        assert_eq!(Event::from_line(&line), Ok(event.clone()), "{line}");
        assert_eq!(event.actor, "machine", "質問と回答は machine 由来");
    }
    let old = r#"{"schema":1,"ts":"2026-09-01T00:00:00Z","kind":"RunCreated","run":"r","bead":"b","host":"h","actor":"machine","stage":"Intake"}"#;
    assert!(Event::from_line(old).is_ok(), "既存の行はそのまま読める");
}
