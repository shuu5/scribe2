//! fleet event log の歯（設計 docs/design/fleet-event-log.md §6）。
//!
//! 置き場は毎回 tmp dir を `--state-dir` で指す（env も HOME も読まない形の裏返し）。

use crate::{make_tmp_dir, TmpDir};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::mem::discriminant;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vessel::order::is_declaration_order;
use vessel::fleet::store::{self, LockPolicy, StoreError};
use vessel::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
use vessel::polarity::{OnFailure, Polarity, Timing};
use vessel::rules::manifest::Manifest;
use vessel::rules::GroupedError;
use vessel::hook::group::RecordError;
use vessel::fleet::cli::format_utc;
use vessel::fleet::json_tree::{self, parse, Tree, TreeError, MAX_DEPTH};
use vessel::fleet::select::{self, Input, Purpose, Selection};
use vessel::fleet::{Cost, CostSource, Usage, COST_SOURCES, KINDS, REASONS, STAGES, WINDOWS};
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
fn state_dir() -> TmpDir {
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
        mark: None,
        account: None,
        cost: None,
        rule: None,
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
    // in-process の dispatch は cwd（この repo）から置き場を解けるので、断りは verb 固有の引数の不足で測る
    // （置き場の断りは tmp の cwd で撃つ `fleet_usage_statedir_` の歯）。
    let refused = vessel::fleet::cli::dispatch(&args(&["show", "--state-dir", &path]));
    assert_eq!(refused.rc, RC_REFUSED, "--run 欠けは rc 1");
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
            let dir = dir.to_path_buf();
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
    // 値欠けは入口の閉包の検査が typed に断る（rc 2・設計 pipeline.md §14 約束 4）。
    assert_eq!(out.status.code(), Some(2), "値の無い flag は黙って落とさない");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--detail に値が無い"), "理由: {err}");
    fs::remove_dir_all(&dir).ok();
}

/// (8) `fleet` の口: 5 verb のどれに**未知の flag** を足しても rc 2・理由の 1 行が flag を名指し usage を添え・置き場の全 entry が
/// 不変（event を書かない・計測しない）。`--help` は usage を stdout へ出して rc 0（設計 pipeline.md §14 約束 3 / 4 / 8）。
#[test]
fn fleet_args_unknown_flag_is_refused_with_rc_2_on_every_verb() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let before = tree(&dir);
    let verbs: [&[&str]; 5] = [
        &["record", "--kind", "RunCreated", "--run", "r1", "--bead", "b1"],
        &["show", "--run", "r1"],
        &["export"],
        &["usage", "--show"],
        &["select", "--purpose", "run"],
    ];
    for verb in verbs {
        let mut args = verb.to_vec();
        args.extend_from_slice(&["--state-dir", &path, "--bogus", "x"]);
        let out = run_fleet(&args);
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{verb:?}: rc 2: {out:?}");
        assert!(out.stdout.is_empty(), "{verb:?}: stdout 0 byte");
        assert_eq!(text(&out.stderr), format!("fleet: 未知の引数 --bogus\n{}\n", vessel::fleet::cli::usage()), "{verb:?}");
        assert_eq!(tree(&dir), before, "{verb:?}: 置き場は不変");
        let mut help = verb.to_vec();
        help.extend_from_slice(&["--state-dir", &path, "--help"]);
        let out = run_fleet(&help);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{verb:?}: --help は rc 0: {out:?}");
        assert_eq!(text(&out.stdout), format!("{}\n", vessel::fleet::cli::usage()), "{verb:?}: usage を stdout へ");
        assert!(out.stderr.is_empty(), "{verb:?}: stderr 0 byte");
        assert_eq!(tree(&dir), before, "{verb:?}: --help も置き場は不変");
    }
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
    crate::install_spawner();
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
    crate::install_spawner();
    let dir = state_dir();
    let path = dir.display().to_string();
    let usage = run_fleet(&[]);
    let missing = run_fleet(&["show", "--run", "nope", "--state-dir", &path]);
    let empty = run_fleet(&["export", "--state-dir", &path]);
    let recorded = run_fleet(&[
        "record", "--kind", "RunCreated", "--run", "r1", "--bead", "s2-x", "--state-dir", &path,
    ]);
    // install の kind は手で書けない（consumer-sync.md §5・書き手は `vessel update` だけ）。
    let install = run_fleet(&[
        "record", "--kind", "InstallRecorded", "--run", "r1", "--bead", "s2-x", "--state-dir", &path,
    ]);
    let fx = usage_fixture(&["a1"]);
    put_credential(&fx, "a1", &expired_credential("tok-old"));
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
    let claude = fake_claude(&fx, "exit 0");
    let refreshed = run_usage_with_claude(&fx, &curl, &claude);
    // 口座の口の外形（account-lifecycle.md §3）: 使い方・add の 1 行・断りの 1 行・ls の 1 行・wire の 1 行（vessel-hook.md
    // §12）・retire の 1 行。
    let acct = state_dir();
    let acct_path = acct.display().to_string();
    let account_usage = run_account(&[]);
    let prepared = run_account(&["add", "a1", "--state-dir", &acct_path]);
    let exists = run_account(&["add", "a1", "--state-dir", &acct_path]);
    let listed = run_account(&["ls", "--state-dir", &acct_path]);
    let wired = run_account(&["wire", "--state-dir", &acct_path]);
    let retired = run_account(&["retire", "a1", "--state-dir", &acct_path]);
    let form = format!(
        "{}{}{}{}{}{}{}{}{}{}{}{}",
        String::from_utf8_lossy(&usage.stderr),
        String::from_utf8_lossy(&missing.stderr),
        String::from_utf8_lossy(&empty.stdout),
        String::from_utf8_lossy(&recorded.stdout),
        String::from_utf8_lossy(&install.stderr),
        String::from_utf8_lossy(&refreshed.stdout),
        String::from_utf8_lossy(&account_usage.stderr),
        String::from_utf8_lossy(&prepared.stdout),
        String::from_utf8_lossy(&exists.stderr),
        String::from_utf8_lossy(&listed.stdout),
        String::from_utf8_lossy(&wired.stdout),
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

/// `KINDS` の並びが**宣言順**と一致し、母集団は 24 種で末尾の 8 つが `InstallRecorded`（`vessel update` が足した・設計
/// consumer-sync.md §5 (4)）→ `RunCost`（消費の 1 件・gate-cost.md §26 形 (2)）→ `RulingReceived`（run 無しの裁定・
/// fleet-event-log.md §9）→ `GroupPressureNotified`（群の逼迫の通知・account-lifecycle.md §19 形 3）→ `GroupMoved` /
/// `GroupMoveRefused` / `GroupMovePending`（群の移動の承認・断り・保留・account-lifecycle.md §20 形 5 / 6）→ `SeatRetired`（席の
/// 登録 row の退役・account-lifecycle.md §24 形 4）。variant を足して列に足し忘れた周・件数だけ合って末尾が違う周はここで赤になる。
#[test]
fn fleet_kinds_follow_declaration_order() {
    assert!(
        is_declaration_order(KINDS, |kind| kind as usize),
        "KINDS の並びが宣言順と乖離している（母集団 {} 種）",
        KINDS.len()
    );
    assert_eq!(KINDS.len(), 24, "母集団（列の印までの 16 + install 1 + 消費 1 + 裁定 1 + 群の逼迫の通知 1 + 群の移動 3 + 登録 row の退役 1）");
    assert_eq!(
        KINDS.get(16..),
        Some(
            &[
                EventKind::InstallRecorded,
                EventKind::RunCost,
                EventKind::RulingReceived,
                EventKind::GroupPressureNotified,
                EventKind::GroupMoved,
                EventKind::GroupMoveRefused,
                EventKind::GroupMovePending,
                EventKind::SeatRetired,
            ][..]
        ),
        "install → 消費 → 裁定 → 群の逼迫の通知 → 群の移動の承認・断り・保留 → 登録 row の退役が宣言順の末尾"
    );
    assert_eq!(EventKind::InstallRecorded.as_str(), "InstallRecorded");
    assert_eq!(EventKind::parse("InstallRecorded"), Some(EventKind::InstallRecorded), "as_str ↔ parse の往復");
    assert_eq!(EventKind::InstallRecorded.default_actor(), "machine", "install は機械由来");
    assert!(!EventKind::InstallRecorded.is_allowance(), "口座残量の kind ではない");
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
            mark: None,
            account: None,
            cost: None,
            rule: None,
        };
        let line = event.to_line();
        assert!(line.contains("\"schema\":1"), "{line}");
        assert_eq!(Event::from_line(&line), Ok(event.clone()), "{line}");
        assert_eq!(event.actor, "machine", "質問と回答は machine 由来");
    }
    let old = r#"{"schema":1,"ts":"2026-09-01T00:00:00Z","kind":"RunCreated","run":"r","bead":"b","host":"h","actor":"machine","stage":"Intake"}"#;
    assert!(Event::from_line(old).is_ok(), "既存の行はそのまま読める");
}

/// run 無しの裁定の event（`bead` 無し・`rule` 無し・逐語 1 つ）。
fn ruling_event(ts: &str) -> Event {
    Event { run: String::new(), bead: String::new(), detail: Some("推奨で進めて".to_owned()), ..event(EventKind::RulingReceived, "", ts) }
}

/// run 無しの裁定（`RulingReceived`・設計 fleet-event-log.md §9 (1)）: 既定の actor は human・schema 1 のまま `run` を持たずに
/// 書けて読め（`bead` / `rule` は在る周だけ key が現れる）、replay は便も席も作らない。`run` / `seat` / `stage` を持つ行・逐語
/// （detail）の無い行・他の kind に `rule` が在る行は malformed。既存の承認と質問の kind は形も actor も不変。
#[test]
fn fleet_ruling_event_is_human_without_a_run_and_malformed_rows_are_refused() {
    assert_eq!(EventKind::RulingReceived.default_actor(), "human", "裁定は人由来");
    assert_eq!(EventKind::parse("RulingReceived"), Some(EventKind::RulingReceived), "as_str ↔ parse の往復");
    let bare = ruling_event("2026-09-22T01:02:03Z");
    let line = bare.to_line();
    assert_eq!(
        line,
        r#"{"schema":1,"ts":"2026-09-22T01:02:03Z","kind":"RulingReceived","host":"h","actor":"human","detail":"推奨で進めて"}"#,
        "run / bead / rule の key を書かない"
    );
    assert_eq!(Event::from_line(&line), Ok(bare.clone()));
    let full = Event { bead: "s2-x.1".to_owned(), rule: Some("R-C9-1".to_owned()), ..bare.clone() };
    let full_line = full.to_line();
    assert!(full_line.contains(r#""bead":"s2-x.1","rule":"R-C9-1""#) && !full_line.contains("\"run\":"), "{full_line}");
    assert_eq!(Event::from_line(&full_line), Ok(full.clone()));
    let state = replay(&[bare, full]);
    assert!(state.runs.is_empty() && state.seats.is_empty(), "便も席も作らない: {state:?}");

    let head = r#"{"schema":1,"ts":"2026-09-22T01:02:03Z","kind":"RulingReceived","host":"h","actor":"human""#;
    for extra in [r#","run":"r1","detail":"w""#, r#","seat":"s1","detail":"w""#, r#","stage":"Intake","detail":"w""#, "", r#","rule":1,"detail":"w""#] {
        let malformed = format!("{head}{extra}}}");
        assert!(Event::from_line(&malformed).is_err(), "malformed: {malformed}");
    }
    let foreign = r#"{"schema":1,"ts":"2026-09-22T01:02:03Z","kind":"RunStage","run":"r","bead":"b","host":"h","actor":"machine","rule":"R-C9-1"}"#;
    assert!(Event::from_line(foreign).is_err(), "他の kind は rule を持たない");
    // 既存の承認と質問の kind は不変（actor と形）。
    assert_eq!(EventKind::ApprovalReceived.default_actor(), "human");
    for kind in [EventKind::ApprovalRequested, EventKind::QuestionRaised, EventKind::QuestionAnswered] {
        assert_eq!(kind.default_actor(), "machine", "{}", kind.as_str());
    }
    let approval = r#"{"schema":1,"ts":"2026-09-22T00:00:00Z","kind":"ApprovalReceived","run":"r","bead":"b","host":"h","actor":"human","detail":"w"}"#;
    assert_eq!(Event::from_line(approval).map(|found| found.to_line()).as_deref(), Ok(approval), "承認の行は同じ字面で往復する");
}

/// `fleet record` は裁定の kind を断る（書き手は対話面の席を確かめる `seat ruling add` だけ・§9 (2)）: rc 1・stdout 0 byte・log を
/// 作らない。`--run` / `--bead` / `--actor human` / `--detail` を揃えても断る。
#[test]
fn fleet_ruling_record_refuses_the_ruling_kind() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let out = run_fleet(&[
        "record", "--state-dir", &path, "--kind", "RulingReceived", "--run", "r1", "--bead", "b", "--actor", "human", "--detail", "推奨で",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{out:?}");
    assert!(out.stdout.is_empty(), "stdout 0 byte");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("kind RulingReceived は record では書けない"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!store::events_path(&dir).exists(), "log を作らない");
    fs::remove_dir_all(&dir).ok();
}

/// 消費の event の fixture（出所 3 値のどれか・6 値は互いに違う数）。
fn cost_event(source: CostSource) -> Event {
    let usage = Usage { input: 11, output: 22, cache_read: 33, cache_create: 44, turns: 5, wall_ms: 6000 };
    Event { cost: Some(Cost { source, usage }), ..event(EventKind::RunCost, "r1", "2026-09-22T00:00:00Z") }
}

/// 消費の event（`RunCost`・設計 gate-cost.md §26 形 (2)）は schema 1 のまま書けて読め、`run` / `bead` と `source` /
/// `usage` / `turns` / `wall_ms` を持つ。**replay は便を作らない**（段を持たない行）。6 値のどれかが欠ける行・`source` が
/// 3 値の外の行・他の kind に消費の key が在る行は malformed（欠けを 0 に倒して読まない・C10）。
#[test]
fn run_cost_event_round_trips_and_malformed_rows_are_refused() {
    for source in COST_SOURCES {
        let event = cost_event(*source);
        let line = event.to_line();
        assert!(line.contains("\"schema\":1"), "{line}");
        assert!(line.contains(&format!("\"source\":\"{}\"", source.as_str())), "{line}");
        assert!(line.contains("\"usage\":\"in:11,out:22,cache_read:33,cache_create:44\",\"turns\":5,\"wall_ms\":6000"), "{line}");
        assert_eq!(Event::from_line(&line), Ok(event.clone()), "{line}");
    }
    assert_eq!(COST_SOURCES.len(), 3, "出所は閉じた 3 値");
    assert!(replay(&[cost_event(CostSource::Runner)]).runs.is_empty(), "消費の行だけでは便を作らない");
    let line = cost_event(CostSource::Lens).to_line();
    for (broken, why) in [
        (line.replacen(",\"turns\":5", "", 1), "turns が欠ける"),
        (line.replacen("\"wall_ms\":6000", "\"wall_ms\":\"6000\"", 1), "wall_ms が数でない"),
        (line.replacen("cache_create:44", "cache_create:x", 1), "usage の token が数でない"),
        (line.replacen("\"source\":\"lens\"", "\"source\":\"planner\"", 1), "source が 3 値の外"),
    ] {
        assert!(Event::from_line(&broken).is_err(), "{why}: {broken}");
    }
    let staged = event(EventKind::RunStage, "r1", "2026-09-22T00:00:00Z").to_line();
    let foreign = staged.replacen("\"host\"", "\"turns\":5,\"host\"", 1);
    assert!(Event::from_line(&foreign).is_err(), "他の kind の行は消費の key を持たない: {foreign}");
    let dir = state_dir();
    let path = dir.display().to_string();
    let out = run_fleet(&["record", "--kind", "RunCost", "--run", "r1", "--bead", "s2-x", "--state-dir", &path]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "消費の行は record では書けない");
    assert!(!dir.join("fleet").join("events.jsonl").exists(), "断った周は行を残さない");
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
        mark: None,
        account: None,
        cost: None,
        rule: None,
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

/// (6e) `KINDS` の 13〜16 番目は登録 → 退役 → 戻し → 列の印の順（口座の退役・戻しの後ろに列の印 1 を足した・
/// account-lifecycle.md §3・dispatcher.md §4）。母集団の件数と末尾は [`fleet_kinds_follow_declaration_order`] が pin する。
#[test]
fn account_cmd_kinds_are_fifteen_with_retire_and_restore_last() {
    assert_eq!(
        KINDS.len(),
        24,
        "母集団（既存 10 + 口座残量 2 + 席の登録 1 + 口座の退役・戻し 2 + 列の印 1 + install 1 + 消費 1 + 裁定 1 + 群の逼迫の通知 1 + 群の移動 3 + 登録 row の退役 1）"
    );
    assert_eq!(
        KINDS.get(12..16),
        Some(
            &[
                EventKind::SeatRegistered,
                EventKind::AccountRetired,
                EventKind::AccountRestored,
                EventKind::DispatchMark,
            ][..]
        ),
        "登録 → 退役 → 戻し → 列の印の順"
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
        mark: None,
        registration: Some(Registration {
            role: Role::Orchestrator,
            anchor: "/repo".to_owned(),
            target: target.to_owned(),
            sid: Some("sid-1".to_owned()),
            account: "a1".to_owned(),
            launch: "line 1\n\"line 2\"\n".to_owned(),
            model: model.map(str::to_owned),
        }),
        account: None,
        cost: None,
        rule: None,
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
    // 知らない役割（variant 名の字面も含む）は malformed でなく**本体を持たない行**として読む
    // ＝退役した役割の row を読み飛ばす（憲法 N4 の schema 互換・replay の面は
    // `fleet_seat_registration_row_with_a_retired_role_is_skipped_by_replay` が測る）。
    let unknown = line.replacen("\"role\":\"orchestrator\"", "\"role\":\"Orchestrator\"", 1);
    let read = Event::from_line(&unknown).expect("知らない役割の行も読める");
    assert_eq!(read.registration, None, "知らない役割の行は本体を持たない: {unknown}");
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
    // 鍵は (役割, anchor) で役割は 1 つ＝2 row を別の鍵にするのは anchor である（ADR-0045 §2 (1)）。
    launched.registration = launched.registration.map(|row| Registration { sid: None, anchor: "/repo/launched".to_owned(), ..row });
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
    assert_eq!(sids, vec![Some("sid-1".to_owned()), None], "鍵の違う 2 row（anchor の違う 2 鍵）が両方 replay に載る");
}

/// 退役した役割の登録 row は **読み飛ばす**（憲法 N4・schema 互換・`s2-07l.478`）。
///
/// 既に在る event log は役割を 1 つにする前の `SeatRegistered` 行（`role` が planner / admin）を持つので、
/// 知らない役割を `Err` に倒すと**その 1 行で replay 全体が unreadable**になる。role guard は FailClosed
/// ゆえ、そうなると全席の権能付きの操作が deny になり、doctor の突合も口座の欄も `unreadable` に落ちる。
/// 行は読めて `Ok` になり、登録には数えず（本体を持たない）、便も作らない（`run` が空の幽霊を生まない）。
#[test]
fn fleet_seat_registration_row_with_a_retired_role_is_skipped_by_replay() {
    let live = registration_event("s:live");
    let retired_line = live.to_line().replacen("\"role\":\"orchestrator\"", "\"role\":\"planner\"", 1);
    assert_ne!(retired_line, live.to_line(), "置換が効く");
    let retired = Event::from_line(&retired_line).expect("退役した役割の行も読める");
    assert_eq!(retired.kind, EventKind::SeatRegistered, "kind は登録のまま");
    assert_eq!(retired.registration, None, "本体は持たない＝登録に数えない");
    let state = replay(&[retired, live.clone()]);
    assert_eq!(state.registrations.len(), 1, "数えるのは読めた役割の row だけ");
    assert!(state.runs.is_empty(), "run が空の幽霊の便を作らない: {:?}", state.runs);
    // 役割の key そのものが欠けた行は従来どおり malformed（「知らない値」と「項目の欠け」を混ぜない）。
    let missing = live.to_line().replacen("\"role\":\"orchestrator\",", "", 1);
    assert_ne!(missing, live.to_line(), "置換が効く");
    assert!(Event::from_line(&missing).is_err(), "role の欠けは malformed: {missing}");
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

// ─────────────── 登録 row の退役（account-lifecycle.md §24・契約表の行 m・接頭辞 `fleet_replay_seat_retired_`） ───────────────

/// `row` の登録 event を退役の event にする（本体は row の写しのまま・`detail` = 理由・actor は human）。
fn seat_retired_of(row: &Event) -> Event {
    Event {
        kind: EventKind::SeatRetired,
        actor: EventKind::SeatRetired.default_actor().to_owned(),
        detail: Some("moved".to_owned()),
        ..row.clone()
    }
}

/// 形 2: `SeatRegistered` → `SeatRetired` の並びで row は `registrations` から外れ、`registered_accounts` が空。別の鍵の row は残る。
/// 退役の行は schema 1 のまま登録と同じ本体で往復し、便も席も作らない（base では kind が無い＝RED）。
#[test]
fn fleet_replay_seat_retired_drops_the_row_and_its_account() {
    let registered = registration_event("s:w");
    let retired = seat_retired_of(&registered);
    let line = retired.to_line();
    assert!(line.contains("\"kind\":\"SeatRetired\"") && line.contains("\"schema\":1"), "{line}");
    assert!(line.contains("\"actor\":\"human\"") && line.contains("\"detail\":\"moved\""), "{line}");
    assert!(!line.contains("\"run\":") && !line.contains("\"bead\":"), "便に紐づかない: {line}");
    assert_eq!(Event::from_line(&line), Ok(retired.clone()), "{line}");
    let state = replay(&[registered.clone(), retired.clone()]);
    assert!(state.registrations.is_empty(), "退役した鍵の row は無い: {:?}", state.registrations);
    assert_eq!(state.registered_accounts(None), BTreeSet::new(), "便用の除外にも載らない");
    assert_eq!(state.registered_accounts(Some(Path::new("/repo"))), BTreeSet::new(), "anchor の絞りでも空");
    assert!(state.runs.is_empty() && state.seats.is_empty(), "便も席も作らない: {state:?}");
    let mut other = registration_event("s:other");
    other.registration = other.registration.map(|row| Registration { anchor: "/repo/other".to_owned(), account: "a9".to_owned(), ..row });
    let kept = replay(&[registered, other.clone(), retired]);
    assert_eq!(kept.registered_accounts(None), BTreeSet::from(["a9".to_owned()]), "退役は同じ鍵（role, anchor）だけを外す");
    assert_eq!(kept.registrations.values().map(|latest| latest.seq).collect::<Vec<_>>(), vec![1], "別の鍵の row は seq ごと残る");
}

/// 形 2: `SeatRegistered` → `SeatRetired` → `SeatRegistered` は最後の row が勝つ（物理順・後の登録が復活させる）。退役の前の登録を
/// 後ろに並べ替えた周は row が在り、退役が最後なら無い。
#[test]
fn fleet_replay_seat_retired_then_registered_resolves_the_last_row() {
    let first = registration_event("s:w");
    let retired = seat_retired_of(&first);
    let mut again = registration_event("s:w2");
    again.registration = again.registration.map(|row| Registration { account: "a2".to_owned(), ..row });
    let state = replay(&[first.clone(), retired.clone(), again.clone()]);
    let rows: Vec<(usize, String, String)> = state
        .registrations
        .values()
        .map(|latest| (latest.seq, latest.registration.target.clone(), latest.registration.account.clone()))
        .collect();
    assert_eq!(rows, vec![(2, "s:w2".to_owned(), "a2".to_owned())], "最後の登録の row");
    assert_eq!(state.registered_accounts(None), BTreeSet::from(["a2".to_owned()]));
    let last = replay(&[first.clone(), again.clone(), retired.clone()]);
    assert!(last.registrations.is_empty(), "退役が最後なら row は無い: {:?}", last.registrations);
    let before = replay(&[retired, first]);
    assert_eq!(before.registrations.len(), 1, "登録より前の退役は後の登録を消さない");
}

/// 形 4: `SeatRetired` は `Shape::Registration`・既定の actor は human・`KINDS` の末尾（24 種目）で、`fleet record` からは書けない
/// （書き手は `seat retire` だけ・rc 1・log を作らない）。
#[test]
fn fleet_replay_seat_retired_kind_is_a_registration_shape_and_record_refuses_it() {
    use vessel::fleet::Shape;
    assert_eq!(KINDS.len(), 24, "母集団");
    assert_eq!(KINDS.last(), Some(&EventKind::SeatRetired), "宣言順の末尾");
    assert_eq!(EventKind::SeatRetired.shape(), Shape::Registration);
    assert_eq!(EventKind::SeatRetired.default_actor(), "human", "退役は人由来");
    assert_eq!(EventKind::parse("SeatRetired"), Some(EventKind::SeatRetired), "as_str ↔ parse の往復");
    assert!(!EventKind::SeatRetired.is_allowance());
    let dir = state_dir();
    let path = dir.display().to_string();
    let out = run_fleet(&["record", "--kind", "SeatRetired", "--run", "r1", "--bead", "b1", "--state-dir", &path]);
    assert_eq!(out.status.code(), Some(1), "書き側で断る: {out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("record では書けない"));
    assert!(!store::events_path(&dir).exists(), "行を残さない");
    fs::remove_dir_all(&dir).ok();
}

/// install の行（`detail` = `sha=<sha12> path=<path>`・`run` / `bead` を持たない）。
fn install_event(detail: &str) -> Event {
    Event { run: String::new(), bead: String::new(), detail: Some(detail.to_owned()), ..event(EventKind::InstallRecorded, "", ALLOWANCE_TS) }
}

/// (3・consumer-sync.md §5) `fleet record` は `InstallRecorded` を手で渡されると断り、行を残さない（書き手は install の成功の
/// 後の `vessel update` だけ＝「撃った」と「入った」を融合しない）。base は kind を知らず「未知である」で断る（語が違う＝RED）。
#[test]
fn vessel_update_fleet_record_refuses_the_install_kind() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let out = run_fleet(&[
        "record", "--kind", "InstallRecorded", "--run", "r1", "--bead", "b1", "--detail", "sha=0123456789ab path=/x",
        "--state-dir", &path,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "書き側で断る: {out:?}");
    assert!(out.stdout.is_empty(), "stdout へは書かない");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr).trim(),
        "fleet: kind InstallRecorded は record では書けない",
        "断りの行"
    );
    assert!(!store::events_path(&dir).exists(), "行を残さない");
}

/// install の行は書いて読むと同じ event に戻り、便も席も作らず、`detail` の形が外れた行・`run` を持つ行は malformed で読む。
#[test]
fn vessel_update_install_row_round_trips_and_refuses_foreign_shapes() {
    let row = install_event("sha=0123456789ab path=/opt/bin/scribe2");
    let line = row.to_line();
    assert!(!line.contains("\"run\":") && !line.contains("\"bead\":"), "run / bead を書かない: {line}");
    assert_eq!(Event::from_line(&line), Ok(row.clone()), "{line}");
    assert_eq!(
        row.install(),
        Some(vessel::fleet::Install { sha: "0123456789ab".to_owned(), path: "/opt/bin/scribe2".to_owned() }),
        "本体は sha12 と path"
    );
    let state = replay(std::slice::from_ref(&row));
    assert_eq!((state.runs.len(), state.seats.len()), (0, 0), "幽霊の便も席も作らない");
    for detail in ["sha=0123456789 path=/x", "sha=0123456789AB path=/x", "sha=0123456789ab path=", "path=/x"] {
        let bad = install_event(detail).to_line();
        assert!(Event::from_line(&bad).is_err(), "detail の形が外れた行は malformed: {bad}");
    }
    let with_run = line.replacen("\"kind\":\"InstallRecorded\",", "\"kind\":\"InstallRecorded\",\"run\":\"r1\",", 1);
    assert_ne!(with_run, line, "fixture の置換が効いている");
    assert!(Event::from_line(&with_run).is_err(), "run を持つ install の行は malformed: {with_run}");
    assert_eq!(event(EventKind::RunCreated, "r1", ALLOWANCE_TS).install(), None, "他の kind は本体を持たない");
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

/// [`LIVE_BODY`] とその期待が名乗る reset の 2 つ（`(five_hour, seven_day)`・字面は `Z` 形）。
///
/// 選定は `resets_at >= now` で測る（`fleet/select.rs`）ので、fixture に固定日付を書くと
/// その日を壁時計が越えた瞬間に歯が赤くなる——時限である。壁時計の**今日**（UNIX 秒 / 86 400）から
/// five_hour = 翌日 05:00:00Z・seven_day = 7 日後 00:00:00Z を組んで、常に今より未来にする。
/// `LazyLock` なので process で 1 回だけ組む＝走行中に日付を跨いでも fixture と期待は同じ値を見る。
/// 字面は器の `format_utc` と同じ経路で作る（`YYYY-MM-DDThh:mm:ssZ`・憲法 C2）。
// flip-check: retroactive s2-07l.468
static LIVE_RESETS: LazyLock<(String, String)> = LazyLock::new(|| {
    let today = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    (format_utc((today + 1) * 86_400 + 5 * 3_600), format_utc((today + 7) * 86_400))
});

/// `Z` 形の字面から末尾の `Z` を外す（`+00:00` 形を組む材料・`Z` が無ければそのまま）。
fn without_z(ts: &str) -> &str {
    ts.strip_suffix('Z').unwrap_or(ts)
}

/// 実測の応答と同じ形の本文（設計 §3）: 窓の `utilization` は**すでに % の値**・`limits[]` の要素は
/// `utilization` を持たず `percent`（整数）が値・reset は `+00:00` 形と `Z` 形が混ざる。値は架空。
///
/// reset だけ [`LIVE_RESETS`] から差す（混在の形・小数付きの `.412000`・使用率・`scope` は不変）。
static LIVE_BODY: LazyLock<String> = LazyLock::new(|| {
    let (five, seven) = &*LIVE_RESETS;
    format!(
        r#"{{
  "five_hour": {{"utilization": 13.0, "resets_at": "{five_naked}.412000+00:00"}},
  "seven_day": {{"utilization": 41.7, "resets_at": "{seven_naked}+00:00"}},
  "limits": [
    {{"kind": "weekly_scoped", "group": "g", "percent": 38, "severity": "normal",
     "resets_at": "{seven}",
     "scope": {{"model": {{"display_name": "Fable", "id": null}}}}, "is_active": true}},
    {{"kind": "weekly", "group": "g", "percent": 50, "severity": "normal",
     "resets_at": "{seven}", "is_active": true}}
  ]
}}"#,
        five_naked = without_z(five),
        seven_naked = without_z(seven),
    )
});

/// [`LIVE_BODY`] を読んだ口座の 1 行（`label` の口座）。
fn live_line(label: &str) -> String {
    let (five, seven) = &*LIVE_RESETS;
    format!(
        "usage: account={label} five_hour=13% resets={five} seven_day=41% resets={seven} model=Fable:38% resets={seven}"
    )
}

/// `fleet usage` の歯の置き場。
struct UsageFixture {
    /// `--state-dir`（既定は [`Self::root`] そのもの・群の歯は 1 段下＝host の根が歯ごとに閉じる・[`group_fixture`]）。
    state: PathBuf,
    /// 置き場を包む一時 dir（drop で消える）。
    root: TmpDir,
    /// 偽 curl の置き場と、偽 curl が残す写し（`args` / `stdin`）。
    spy: TmpDir,
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
    let root = state_dir();
    let spy = state_dir();
    let rules = spy.join("rules.toml");
    fs::write(&rules, usage_rules(labels)).expect("rules fixture を書ける");
    UsageFixture { state: root.to_path_buf(), root, spy, rules }
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
    fs::remove_dir_all(&fx.root).ok();
    fs::remove_dir_all(&fx.spy).ok();
}

/// (1) live 2 口座相当: 口座ごと 1 行・口座 × 窓（3 窓 × 2）の event・reset は UTC 形・使用率は % の値の切り捨て。
#[test]
fn fleet_usage_measures_two_accounts_into_lines_and_events() {
    crate::install_spawner();
    let fx = usage_fixture(&["a1", "a2"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    put_credential(&fx, "a2", &live_credential(TOKEN_A2));
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
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
    let (five, seven) = &*LIVE_RESETS;
    let want_for = |label: &str| {
        vec![
            (label.to_owned(), WindowKind::FiveHour, None, 13, Some(five.clone())),
            (label.to_owned(), WindowKind::SevenDay, None, 41, Some(seven.clone())),
            (label.to_owned(), WindowKind::SevenDayModel, Some("Fable".to_owned()), 38, Some(seven.clone())),
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
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
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
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
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
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
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
    let (five, seven) = &*LIVE_RESETS;
    let no_name_line = format!(
        "usage: account=a1 five_hour=13% resets={five} seven_day=41% resets={seven} seven_day_model=unmeasured:shape_mismatch"
    );
    let cases: [(&str, &str, &str, u8, &str); 5] = [
        ("timeout", &LIVE_BODY, "200", 28, "usage: account=a1 unmeasured reason=timeout"),
        ("refused", &LIVE_BODY, "200", 7, "usage: account=a1 unmeasured reason=client_failed"),
        ("status", &LIVE_BODY, "500", 0, "usage: account=a1 unmeasured reason=http_status"),
        ("garbage", "<html>oops</html>", "200", 0, "usage: account=a1 unmeasured reason=body_unreadable"),
        ("no display_name", &no_name, "200", 0, &no_name_line),
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
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
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
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
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

// ─────────────── 置き場の既定と人が読む表（fleet-usage.md §11・接頭辞 `fleet_usage_statedir_` / `fleet_usage_table_`） ───────────────

/// cwd を指定して `fleet` を binary で 1 回撃つ（置き場を git 設定から解く経路を測る・`seat.rs` の `run_seat_in` と同じ型）。
/// **cwd は tmp**（repo の cwd で撃つと本物の置き場を解く）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_fleet_in(cwd: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .arg("fleet")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("binary を起動できる")
}

/// git 設定 `<NAME>.stateDir` に `state` を持つ tmp の git repo（commit なし・`rev-parse --show-toplevel` は init だけで解ける）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn repo_with_state_dir(state: &Path) -> TmpDir {
    let repo = state_dir();
    let key = format!("{}.stateDir", vessel::name::NAME);
    for args in [vec!["init", "-q"], vec!["config", &key, &state.display().to_string()]] {
        let out = Command::new("git").arg("-C").arg(&repo).args(&args).output().expect("git を起動できる");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    repo
}

/// 5 verb の flag 無しの引数（`rules` / `curl` は fixture の path）。verb 固有の必須の引数は揃える（置き場の断りだけを測る）。
fn verbs_without_flag<'a>(rules: &'a str, curl: &'a str) -> [Vec<&'a str>; 5] {
    [
        vec!["record", "--kind", "RunCreated", "--run", "r1", "--bead", "s2-x"],
        vec!["show", "--run", "r1"],
        vec!["export"],
        vec!["usage", "--rules", rules, "--curl", curl],
        vec!["select", "--purpose", "session", "--rules", rules, "--curl", curl],
    ]
}

/// 約束 1: flag 無しの `fleet usage` は cwd の repo の git 設定から置き場を解いて計測し、store がその dir に出来る。
/// flag が在る周は flag の dir に出来る（git 設定の dir には増えない）。base は `--state-dir` 必須で使い方の rc 1（RED・機能不在）。
#[test]
fn fleet_usage_statedir_git_config_is_used_without_the_flag_and_the_flag_wins() {
    let fx = usage_fixture(&["a1"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
    let repo = repo_with_state_dir(&fx.state);
    let rules = fx.rules.display().to_string();
    let client = curl.display().to_string();
    assert!(!store::events_path(&fx.state).exists(), "撃つ前は store が無い");
    let out = run_fleet_in(&repo, &["usage", "--rules", &rules, "--curl", &client]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec![live_line("a1")], "1 行形の字面は不変（出所を足さない）");
    assert!(store::events_path(&fx.state).exists(), "store は git 設定の dir に出来る");
    assert_eq!(allowances(&fx).len(), 3, "3 窓の event");

    let other = state_dir();
    let flag = other.display().to_string();
    let out = run_fleet_in(&repo, &["usage", "--rules", &rules, "--curl", &client, "--state-dir", &flag]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["usage: account=a1 unmeasured reason=no_credentials".to_owned()], "flag の dir に credential は無い");
    assert!(store::events_path(&other).exists(), "flag が在れば flag の dir");
    assert_eq!(allowances(&fx).len(), 3, "git 設定の dir には増えない");
    fs::remove_dir_all(&repo).ok();
    fs::remove_dir_all(&other).ok();
    drop_fixture(&fx);
}

/// 約束 2: 5 verb（record / show / export / usage / select）が同じ入口を通る＝git 設定の repo で flag 無しに撃くと、どれも
/// 置き場の断りを出さず rc 0 で終わる。
#[test]
fn fleet_usage_statedir_five_verbs_share_the_entry_without_the_flag() {
    let fx = usage_fixture(&["a1"]);
    fs::write(&fx.rules, select_rules(&["a1"], true, Some("50"))).expect("rules fixture を書ける");
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
    let repo = repo_with_state_dir(&fx.state);
    let rules = fx.rules.display().to_string();
    let client = curl.display().to_string();
    for args in verbs_without_flag(&rules, &client) {
        let out = run_fleet_in(&repo, &args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{args:?}: stderr={err}");
        assert!(!err.contains("reason=state-dir"), "{args:?}: 置き場の断りを出さない: {err}");
        assert!(!err.contains("usage: fleet"), "{args:?}: 使い方を出さない: {err}");
    }
    fs::remove_dir_all(&repo).ok();
    drop_fixture(&fx);
}

/// 約束 3: git の無い tmp の cwd では 5 verb とも同じ 1 行（`fleet: refused reason=state-dir`）+ 使い方で rc 1・stdout 0 byte・
/// store を作らない（cwd に何も出来ない）。
#[test]
fn fleet_usage_statedir_unresolved_cwd_refuses_every_verb_with_one_line_and_no_store() {
    let fx = usage_fixture(&["a1"]);
    fs::write(&fx.rules, select_rules(&["a1"], true, Some("50"))).expect("rules fixture を書ける");
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
    let cwd = state_dir();
    let rules = fx.rules.display().to_string();
    let client = curl.display().to_string();
    for args in verbs_without_flag(&rules, &client) {
        let out = run_fleet_in(&cwd, &args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{args:?}: stderr={err}");
        assert!(out.stdout.is_empty(), "{args:?}: stdout は 0 byte");
        let lines: Vec<&str> = err.lines().collect();
        assert_eq!(lines.first().copied(), Some("fleet: refused reason=state-dir"), "{args:?}: 同じ 1 行");
        assert!(lines.get(1).is_some_and(|line| line.starts_with("usage: fleet ")), "{args:?}: 使い方: {err}");
        assert_eq!(lines.len(), 2, "{args:?}: 断りと使い方だけ: {err}");
    }
    let entries = fs::read_dir(&cwd).map(Iterator::count).unwrap_or(usize::MAX);
    assert_eq!(entries, 0, "cwd に store を作らない");
    assert!(!store::events_path(&fx.state).exists(), "fixture の置き場にも書かない");
    assert!(!fx.spy.join("args").exists(), "client を起こさない");
    fs::remove_dir_all(&cwd).ok();
    drop_fixture(&fx);
}

/// 表の列の値（空白区切り）。
fn cells_of(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

/// 約束 4: `--show --table` は見出し 2 行（出所 + path・列名）と口座ごとの行を出し、seat 列が登録 row の役割名（登録の無い
/// 口座は `-`）を映す。read-only（event が増えない）。`--table` だけの周は計測してから同じ表（event が増える）。
/// base は `--table` が無視され 1 行形が出る（RED・機能不在）。
#[test]
fn fleet_usage_table_show_prints_two_headers_and_seat_roles_from_registration_rows() {
    let fx = usage_fixture(&["a1", "a2"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    put_credential(&fx, "a2", &live_credential(TOKEN_A2));
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
    let measured = run_usage(&fx, &curl, &[]);
    assert_eq!(measured.status.code(), Some(i32::from(RC_OK)), "{measured:?}");
    register_account(&fx.state, "a1");
    let (five, _) = &*LIVE_RESETS;

    let shown = run_usage(&fx, &curl, &["--show", "--table"]);
    assert_eq!(shown.status.code(), Some(i32::from(RC_OK)), "{shown:?}");
    let lines = out_lines(&shown);
    let line = |at: usize| lines.get(at).map(String::as_str).unwrap_or_default();
    assert_eq!(lines.len(), 4, "見出し 2 行 + 口座 2 行: {lines:?}");
    assert_eq!(line(0), format!("source=flag state_dir={}", fx.state.display()), "1 行目は出所が先・path が行末");
    assert_eq!(cells_of(line(1)), ["account", "5h", "7d", "model", "seat", "resets"]);
    assert_eq!(cells_of(line(2)), ["a1", "13%", "41%", "Fable:38%", "orchestrator", five], "登録 row の役割名");
    assert_eq!(cells_of(line(3)), ["a2", "13%", "41%", "Fable:38%", "-", five], "登録の無い口座は -");
    let seat_at = line(1).find("seat").unwrap_or(usize::MAX);
    assert!(line(2).get(seat_at..).is_some_and(|tail| tail.starts_with("orchestrator")), "列が揃う: {lines:?}");
    assert!(line(3).get(seat_at..).is_some_and(|tail| tail.starts_with('-')), "列が揃う: {lines:?}");
    assert_eq!(allowances(&fx).len(), 6, "--show --table は行を足さない");
    assert!(!lines.iter().any(|line| line.starts_with("usage: account=")), "表の周に 1 行形は出ない");

    let counted = run_usage(&fx, &curl, &["--table"]);
    assert_eq!(counted.status.code(), Some(i32::from(RC_OK)), "{counted:?}");
    assert_eq!(out_lines(&counted), lines, "計測してから同じ表");
    assert_eq!(allowances(&fx).len(), 12, "--table だけの周は計測する");
    drop_fixture(&fx);
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
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0).display().to_string();
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
fn restore_place() -> (TmpDir, String, PathBuf) {
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
    for bad in [&["add"][..], &["nope", "--state-dir", &path]] {
        let out = run_account(bad);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{bad:?}");
        assert_eq!(text(&out.stderr), usage, "{bad:?} は使い方で断る");
    }
    // flag の閉包の断り（欠け・値欠け・重複・未知）は typed な理由の 1 行 + usage で rc 2（設計 pipeline.md §14 約束 4）。
    for (bad, reason) in [
        (&["add", "a2"][..], "--state-dir に値が無い"),
        (&["ls"], "--state-dir に値が無い"),
        (&["ls", "--state-dir"], "--state-dir に値が無い"),
        (&["ls", "--state-dir", &path, "--state-dir", &path], "--state-dir が 2 回以上在る"),
        (&["retire", "a1", "--state-dir", &path, "--target", "x:y"], "未知の引数 --target"),
    ] {
        let out = run_account(bad);
        assert_eq!(out.status.code(), Some(2), "{bad:?}");
        assert_eq!(text(&out.stderr), format!("account: {reason}\n{usage}"), "{bad:?} は理由と使い方で断る");
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
/// (e) flag の 3 条件はそれぞれ単独で使い方の誤り: 空の値（`--anchor ''`）は usage（rc 1）・`--` で始まる値（`--anchor --target`）と
/// 未知の flag（`--bogus x`）は閉包の断り（typed な理由の 1 行 + usage・rc 2・設計 pipeline.md §14 約束 4）で断り、どれも stdout 0 byte で
/// file も event も書かない（3 形を別々に撃つ）。
#[test]
fn account_cmd_flags_refuse_empty_value_dashed_value_and_unknown_flag() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let before = tree(&dir);
    let usage = text(&run_account(&[]).stderr);
    assert!(usage.starts_with("usage: account <add <label>"), "{usage}");
    for (name, tail, rc, err) in [
        ("空の値", &["--anchor", ""][..], i32::from(RC_REFUSED), usage.clone()),
        ("-- で始まる値", &["--anchor", "--target"], 2, format!("account: --anchor に値が無い\n{usage}")),
        ("未知の flag", &["--bogus", "x"], 2, format!("account: 未知の引数 --bogus\n{usage}")),
    ] {
        let mut call = vec!["add", "a2", "--state-dir", &path];
        call.extend_from_slice(tail);
        let out = run_account(&call);
        assert_eq!(out.status.code(), Some(rc), "{name}: {out:?}");
        assert!(out.stdout.is_empty(), "{name}: stdout 0 byte");
        assert_eq!(text(&out.stderr), err, "{name}: 使い方で断る");
        assert_eq!(tree(&dir), before, "{name}: file も event も不変");
    }
    fs::remove_dir_all(&dir).ok();
}

// ─────────────── host-guard の配線（`account wire`・vessel-hook.md §12 行 d・接頭辞 `host_guard_wire_`） ───────────────

/// user の既存の設定（PreToolUse の hook 1 本と他の key・数の字面 `1.50`）。
const USER_SETTINGS: &str = "{\"model\": \"opus\", \"hooks\": {\"PreToolUse\": [{\"matcher\": \"Bash\", \"hooks\": [{\"type\": \"command\", \"command\": \"mine.sh\"}]}], \"Stop\": []}, \"n\": 1.50}\n";

/// 置き場に host の面（`labels`）を書き、各口座の dir の `settings.json` を `shared` への symlink で置く（`shared` の本文は
/// [`USER_SETTINGS`]）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn linked_accounts(dir: &Path, labels: &[&str], shared: &Path) {
    put_host_labels(dir, labels);
    if let Some(parent) = shared.parent() {
        fs::create_dir_all(parent).expect("実体の dir を作れる");
    }
    fs::write(shared, USER_SETTINGS).expect("実体を書ける");
    for label in labels {
        let at = dir.join("accounts").join(label);
        fs::create_dir_all(&at).expect("口座の dir を作れる");
        std::os::unix::fs::symlink(shared, at.join("settings.json")).expect("link を置ける");
    }
}

/// file を読んで入れ子の値にする（読めなければ `null`＝比べる側の assert が落ちる）。
fn tree_of(path: &Path) -> Tree {
    parse(&fs::read_to_string(path).unwrap_or_default()).unwrap_or(Tree::Null)
}

/// 値の `hooks.PreToolUse` の配列（無ければ空）。
fn pre_tool_use(tree: &Tree) -> Vec<Tree> {
    tree.get("hooks").and_then(|hooks| hooks.get("PreToolUse")).and_then(Tree::as_array).unwrap_or_default().to_vec()
}

/// 値から `hooks.PreToolUse` の末尾の 1 要素を外した値（足した 1 要素を除けば元と同じかを比べる）。
fn without_last_pre_tool_use(tree: Tree) -> Tree {
    let Tree::Object(mut pairs) = tree else { return tree };
    for (key, value) in &mut pairs {
        if let (true, Tree::Object(events)) = (key == "hooks", value) {
            for (event, items) in events {
                if let (true, Tree::Array(found)) = (event == "PreToolUse", items) {
                    found.pop();
                }
            }
        }
    }
    Tree::Object(pairs)
}

/// (2) 同じ実体への symlink の口座 2 つは実体 1 つとして 1 回だけ書かれ（entities=1 added=1）、実体の PreToolUse に要素が 1 つ
/// 増え、他の key・順序・値（数の字面）と両口座の symlink は不変、event は 1 件も書かれない（出力は 1 行）。
#[test]
fn host_guard_wire_writes_one_entity_once_through_symlinked_accounts() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let shared = dir.join("shared").join("settings.json");
    linked_accounts(&dir, &["a1", "a2"], &shared);
    let out = run_account(&["wire", "--state-dir", &path]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(text(&out.stdout), "account: wired accounts=2 entities=1 added=1 kept=0 refused=0\n");
    assert!(out.stderr.is_empty(), "{out:?}");
    let after = tree_of(&shared);
    let before = parse(USER_SETTINGS).unwrap_or_else(|error| panic!("fixture は読める: {error}"));
    assert_eq!(pre_tool_use(&after).len(), pre_tool_use(&before).len() + 1, "要素が 1 つだけ増える: {after:?}");
    assert_eq!(pre_tool_use(&after).first(), pre_tool_use(&before).first(), "既存の要素は先頭のまま");
    assert_eq!(without_last_pre_tool_use(after), before, "足した 1 要素を除けば key・順序・値が不変");
    for label in ["a1", "a2"] {
        assert!(is_link_to(&dir.join("accounts").join(label).join("settings.json"), &shared), "{label}: symlink のまま");
    }
    assert!(fs::symlink_metadata(store::events_path(&dir)).is_err(), "event を書かない");
    assert!(fs::symlink_metadata(dir.join("shared").join("settings.json.staged")).is_err(), "一時 file を残さない");
    fs::remove_dir_all(&dir).ok();
}

/// (3) 2 回目は host-guard の項目が在るので実体を byte 単位で変えない（kept=1・置き場の全 entry が不変＝冪等）。
#[test]
fn host_guard_wire_second_run_keeps_every_byte() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let shared = dir.join("shared").join("settings.json");
    linked_accounts(&dir, &["a1", "a2"], &shared);
    let first = run_account(&["wire", "--state-dir", &path]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "{first:?}");
    let before = tree(&dir);
    let again = run_account(&["wire", "--state-dir", &path]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "{again:?}");
    assert_eq!(text(&again.stdout), "account: wired accounts=2 entities=1 added=0 kept=1 refused=0\n");
    assert_eq!(tree(&dir), before, "実体も link も byte 単位で不変");
    fs::remove_dir_all(&dir).ok();
}

/// (4) 読めない実体（JSON でない・root が配列）は refused に数えて 1 byte も書かず rc 2、読める実体は書かれ、口座の dir が無い
/// 口座は数に入れて実体に数えない。event は 1 件も書かれない。
#[test]
fn host_guard_wire_refuses_an_unreadable_entity_and_writes_the_others() {
    let dir = state_dir();
    let path = dir.display().to_string();
    put_host_labels(&dir, &["a1", "a2", "a3", "gone"]);
    let bodies = [("a1", "{\"disableAgentView\": tru"), ("a2", "{\"disableAgentView\": true}\n"), ("a3", "[1]\n")];
    for (label, body) in bodies {
        let at = dir.join("accounts").join(label);
        fs::create_dir_all(&at).expect("口座の dir を作れる");
        fs::write(at.join("settings.json"), body).expect("設定を書ける");
    }
    let out = run_account(&["wire", "--state-dir", &path]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{out:?}");
    assert_eq!(text(&out.stdout), "account: wired accounts=4 entities=3 added=1 kept=0 refused=2\n");
    for (label, body) in bodies.into_iter().filter(|(label, _)| *label != "a2") {
        let settings = dir.join("accounts").join(label).join("settings.json");
        assert_eq!(fs::read(&settings).unwrap_or_default(), body.as_bytes(), "{label}: 1 byte も書かない");
        assert!(fs::symlink_metadata(dir.join("accounts").join(label).join("settings.json.staged")).is_err(), "{label}");
    }
    let written = tree_of(&dir.join("accounts").join("a2").join("settings.json"));
    assert_eq!(pre_tool_use(&written).len(), 1, "読める実体は書かれる: {written:?}");
    assert_eq!(written.get("disableAgentView"), Some(&Tree::Bool(true)), "他の key は保つ");
    assert!(fs::symlink_metadata(dir.join("accounts").join("gone")).is_err(), "無い口座の dir を作らない");
    assert!(fs::symlink_metadata(store::events_path(&dir)).is_err(), "event を書かない");
    fs::remove_dir_all(&dir).ok();
}

/// (2) 足した要素の command は `<NAME> host-guard --state-dir <絶対 path>`（相対の `--state-dir` も絶対化する）・matcher は 5 道具・
/// timeout は跨版で固定の 10 で、埋め込みの `hook.timeout_s` の値と同じ（2 面の値が今は一致する事実を pin する）。
#[test]
fn host_guard_wire_command_names_the_absolute_state_dir_with_the_fixed_matcher_and_timeout() {
    let parent = state_dir();
    let state = parent.join("state");
    let settings = state.join("accounts").join("a1").join("settings.json");
    fs::create_dir_all(settings.parent().unwrap_or(&state)).expect("口座の dir を作れる");
    put_host_labels(&state, &["a1"]);
    fs::write(&settings, "{}\n").expect("設定を書ける");
    let out = Command::new(bin()).current_dir(&parent).args(["account", "wire", "--state-dir", "state"]).output().expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    let groups = pre_tool_use(&tree_of(&settings));
    assert_eq!(groups.len(), 1, "{groups:?}");
    let group = groups.first().cloned().unwrap_or(Tree::Null);
    assert_eq!(group.get("matcher").and_then(Tree::as_str), Some("Bash|Edit|Write|MultiEdit|NotebookEdit"));
    let hooks = group.get("hooks").and_then(Tree::as_array).unwrap_or_default().to_vec();
    assert_eq!(hooks.len(), 1, "{hooks:?}");
    let hook = hooks.first().cloned().unwrap_or(Tree::Null);
    let real = fs::canonicalize(&parent).expect("置き場の親を実 path にできる").join("state");
    assert_eq!(hook.get("command").and_then(Tree::as_str), Some(format!("{} host-guard --state-dir {}", vessel::name::NAME, real.display()).as_str()));
    assert_eq!(hook.get("type").and_then(Tree::as_str), Some("command"));
    let embedded = Manifest::embedded().expect("埋め込みの manifest を読める");
    let timeout = match embedded.get("hook.timeout_s").map(|row| row.value.clone()) {
        Some(vessel::rules::RuleValue::Int(found)) => found,
        other => panic!("hook.timeout_s は整数: {other:?}"),
    };
    assert_eq!(hook.get("timeout"), Some(&Tree::Num("10".to_owned())), "跨版で固定の 10");
    assert_eq!(hook.get("timeout"), Some(&Tree::Num(timeout.to_string())), "埋め込みの hook.timeout_s の値と同じ");
    fs::remove_dir_all(&parent).ok();
}

/// (3a) 期限切れの credential を偽 claude が書き換える周は、読み直して measured になり行の末尾に `refresh=ok`。
/// 起動は `-p` と `--max-turns 1`・口座の設定 dir を `CLAUDE_CONFIG_DIR` に・cwd は state dir。
#[test]
fn fleet_usage_refresh_rewritten_credential_is_measured_with_refresh_ok() {
    let fx = usage_fixture(&["a1"]);
    put_credential(&fx, "a1", &expired_credential("tok-old"));
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
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
        let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
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
/// 4 s でも走行 9〜11 の下では偽 claude が `{spy}/child` を書く前に切れた（gate の log 5 便・2026-09-16・
/// 設計 gate-cost.md §23 形 (2)(c)）ので 15 s にする。fixture の manifest の値であって rules 行の裁定ではない。
const REFRESH_TIMEOUT_S: u64 = 15;

/// 偽 claude が pid file を書くのを待つ上限（子が起動に達しない周を停止経路の失敗と混同しないための待ち）。
// flip-check: retroactive s2-07l.385
const PID_FILE_WAIT: Duration = Duration::from_secs(60);

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
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
    (fx, curl)
}

/// 停止の猶予 1 回分（埋め込み manifest の `pipe.stop_grace_ms`・読めない周は 0）。
fn stop_grace() -> Duration {
    let grace_ms = Manifest::embedded()
        .ok()
        .and_then(|manifest| manifest.get("pipe.stop_grace_ms").map(|row| row.value.clone()))
        .and_then(|value| match value {
            vessel::rules::RuleValue::Int(found) => Some(found),
            _ => None,
        })
        .unwrap_or(0);
    Duration::from_millis(grace_ms)
}

/// 停止経路が要する壁時計の上限: 上限 + 猶予（[`stop_grace`]・TERM 後と KILL 後の 2 回）+ 余裕。
fn refresh_stop_bound() -> Duration {
    Duration::from_secs(REFRESH_TIMEOUT_S + STOP_MARGIN_S) + stop_grace().saturating_mul(2)
}

/// 器を 1 回起こした周の材料: 出力と、時刻 2 つ（起動から返るまでの経過・返った時刻）。起動の段の判定は
/// 「返ってから」を自分の時計で測るので、歯は壁時計の assert を持たない（設計 gate-cost.md §23 形 (2)）。
struct RefreshRun {
    /// `fleet usage` の出力。
    out: Output,
    /// 起動から返るまでの経過。
    elapsed: Duration,
    /// 器が返った時刻（「返ってから判定まで」の基点）。
    returned: Instant,
}

/// 偽 claude つきで `fleet usage` を 1 回撃ち、時刻 2 つを測って返す（`path` が在れば PATH を差し替える）。
fn run_refresh(fx: &UsageFixture, curl: &Path, claude: &Path, path: Option<&str>) -> RefreshRun {
    let started = Instant::now();
    let out = match path {
        Some(found) => run_usage_with_claude_on_path(fx, curl, claude, found),
        None => run_usage_with_claude(fx, curl, claude),
    };
    let returned = Instant::now();
    RefreshRun { out, elapsed: returned.saturating_duration_since(started), returned }
}

/// `/proc/loadavg` の 1 分値（読めない周は `-`）。落ちた周の文に載せる provenance（C10）。
fn loadavg_1min() -> String {
    fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|text| text.split_whitespace().next().map(str::to_owned))
        .unwrap_or_else(|| "-".to_owned())
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
/// 超えた周の文は段 `return` と経過・上限・猶予を名乗る（bound の式は不変・§23 形 (2)(d)）。
fn assert_refresh_timeout(run: &RefreshRun) {
    assert_eq!(run.out.status.code(), Some(i32::from(RC_OK)), "{:?}", run.out);
    assert_eq!(
        out_lines(&run.out),
        vec!["usage: account=a1 unmeasured reason=token_expired refresh=timeout".to_owned()],
        "{:?}",
        run.out
    );
    let bound = refresh_stop_bound();
    assert!(
        run.elapsed < bound,
        "stage=return 上限で止めない elapsed_ms={} bound_ms={} timeout_s={REFRESH_TIMEOUT_S} grace_ms={}",
        run.elapsed.as_millis(),
        bound.as_millis(),
        stop_grace().as_millis()
    );
}

/// 起動の段（`launch`）の判定: **器が返った後**に `{spy}/<who>` の pid を読む。無い周は [`PID_FILE_WAIT`] を
/// poll せず、未達の文——段・経過 2 値（起動から返るまで `run_ms` / 返ってから判定まで `judged_after_return_ms`）・
/// fixture の上限・load——を返す。返る**前**の待ち（`term` の到着など）は [`spy_line`] のまま（§23 形 (2)(a)）。
fn launched_pid(fx: &UsageFixture, who: &str, run: &RefreshRun) -> Result<String, String> {
    if let Ok(text) = fs::read_to_string(fx.spy.join(who)) {
        let line = text.trim().to_owned();
        if !line.is_empty() {
            return Ok(line);
        }
    }
    Err(format!(
        "{who}: stage=launch 起動に達しない（器が返った後は待たない・停止経路の失敗ではない） \
         run_ms={} judged_after_return_ms={} timeout_s={REFRESH_TIMEOUT_S} load1={}",
        run.elapsed.as_millis(),
        run.returned.elapsed().as_millis(),
        loadavg_1min()
    ))
}

/// `/proc/<pid>/stat` の state（最後の `)` の後ろの第 1 欄）。`/proc` が無い・読めない周は `None`。
fn proc_state(pid: &str) -> Option<char> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = text.rsplit_once(')')?.1;
    after.split_whitespace().next()?.chars().next()
}

/// pid が「残っていない」か: `/proc/<pid>` が無いか、state が `Z`（回収待ち＝既に死んでいて親の wait を待つだけ・
/// KILL の直後はこの形で `/proc` に残る）。
fn is_gone(pid: &str) -> bool {
    !Path::new(&format!("/proc/{pid}")).exists() || proc_state(pid) == Some('Z')
}

/// `who`（`child` / `grandchild`）の pid が器の返る前に書かれていて、その process が残っていない。
/// 消えるのは停止経路の後なので猶予（[`stop_grace`]）まで poll する。落ちた周の文は段（`launch` / `stop`）を名乗る。
#[expect(
    clippy::panic,
    reason = "統合 test の helper。clippy の allow-panic-in-tests は #[test] 関数の中だけに効く"
)]
fn assert_gone(fx: &UsageFixture, who: &str, run: &RefreshRun) {
    let pid = match launched_pid(fx, who, run) {
        Ok(found) => found,
        Err(line) => panic!("{line}"),
    };
    let grace = stop_grace();
    let started = Instant::now();
    loop {
        if is_gone(&pid) {
            return;
        }
        if started.elapsed() >= grace {
            panic!(
                "{who}: stage=stop 残っている pid={pid} state={} elapsed_ms={} grace_ms={}",
                proc_state(&pid).unwrap_or('-'),
                started.elapsed().as_millis(),
                grace.as_millis()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
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
    let run = run_refresh(&fx, &curl, &claude, None);
    assert_refresh_timeout(&run);
    for who in ["child", "grandchild"] {
        assert_gone(&fx, who, &run);
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
    let run = run_refresh(&fx, &curl, &claude, Some(&path));
    assert_refresh_timeout(&run);
    for who in ["child", "grandchild"] {
        assert_gone(&fx, who, &run);
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
    let run = run_refresh(&fx, &curl, &claude, Some(&path));
    assert_refresh_timeout(&run);
    assert_gone(&fx, "child", &run);
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
    let run = run_refresh(&fx, &curl, &claude, Some(&path));
    assert_refresh_timeout(&run);
    for who in ["child", "grandchild"] {
        assert_gone(&fx, who, &run);
    }
    drop_fixture(&fx);
}

/// 文の `<key><数>` の値（ms）。token が無い・数でない周は [`u128::MAX`]（上限の pin に落ちる側）。
fn token_ms(line: &str, key: &str) -> u128 {
    line.split_whitespace()
        .find_map(|word| word.strip_prefix(key))
        .and_then(|value| value.parse().ok())
        .unwrap_or(u128::MAX)
}

/// (n-a) 起動の段の未達は器が返った後に待たない（設計 gate-cost.md §23 形 (2)(a)）: pid を書かずに上限を超えて
/// 眠る偽 claude で `refresh=timeout` の後、[`launched_pid`] が段 `launch` の文（経過 2 値・上限・load つき）を返し、
/// その「返ってから判定まで」は [`PID_FILE_WAIT`] に届かない——返った後も poll する形はこの pin で落ちる。
// flip-check: retroactive s2-07l.417
#[test]
fn gate_flaky_bound_launch_miss_is_reported_without_the_pid_wait() {
    let (fx, curl) = refresh_timeout_fixture();
    let claude = fake_claude(&fx, "sleep 30");
    let run = run_refresh(&fx, &curl, &claude, None);
    assert_refresh_timeout(&run);
    let line = launched_pid(&fx, "child", &run).expect_err("pid file を書かない周は起動に達しない");
    assert!(line.contains("stage=launch"), "段を名乗る: {line}");
    assert!(line.contains(&format!("timeout_s={REFRESH_TIMEOUT_S}")), "fixture の上限を載せる: {line}");
    assert!(line.contains("load1="), "load の provenance を載せる: {line}");
    assert!(token_ms(&line, "run_ms=") < u128::MAX, "起動から返るまでの経過を載せる: {line}");
    let after = token_ms(&line, "judged_after_return_ms=");
    assert!(after < PID_FILE_WAIT.as_millis(), "返ってから判定まで待たない（{PID_FILE_WAIT:?} 未満）: {line}");
    drop_fixture(&fx);
}

/// (n-b) 回収していない子（state `Z`）は「残っていない」に数える（設計 gate-cost.md §23 形 (2)(b)）: 歯が起こして
/// wait しない `sh` は終えた後も回収まで `/proc/<pid>` に残るので、`/proc` の存在だけで見る判定は停止経路の全長で
/// 「残った」と読む。器を起こさない pure な判定の歯。
// flip-check: retroactive s2-07l.417
#[test]
fn gate_flaky_bound_zombie_counts_as_gone() {
    let mut child = Command::new("sh").args(["-c", "exit 0"]).spawn().expect("子を起こせる");
    let pid = child.id().to_string();
    let started = Instant::now();
    let mut state = proc_state(&pid);
    while state != Some('Z') && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(10));
        state = proc_state(&pid);
    }
    assert_eq!(state, Some('Z'), "回収していない子は Z で残る（pid {pid}）");
    assert!(
        Path::new(&format!("/proc/{pid}")).exists(),
        "回収の前は /proc に残る（存在だけを見る判定は「残った」と読む・pid {pid}）"
    );
    assert!(is_gone(&pid), "state Z は「残っていない」に数える（pid {pid}）");
    child.wait().expect("子を回収できる");
}

/// (3f)(3g) fresh な credential と墓標では偽 claude を起こさない（argv の写しが無い）。
#[test]
fn fleet_usage_refresh_is_not_attempted_for_fresh_or_tombstone() {
    let fx = usage_fixture(&["a1", "tomb"]);
    put_credential(&fx, "a1", &live_credential(TOKEN_A1));
    put_credential(&fx, "tomb", r#"{"claudeAiOauth":{"accessToken":"tok-tomb","expiresAt":0}}"#);
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
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
    let curl = fake_curl(&fx, &LIVE_BODY, "200", 0);
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

/// `fleet select` の歯の 7 日窓の reset（便用の 1 つ目の鍵・ADR-0042・既定は全口座で同じ＝並びは label に落ちる）。
const SELECT_WEEK_RESET: &str = "2099-01-07T00:00:00+00:00";

/// 3 口座: a1 = 30（5h）・a2 = 70（7d）・a3 = 100（5h・当たっている）。
const SELECT_THREE: &[(&str, u64, u64)] = &[("a1", 30, 10), ("a2", 20, 70), ("a3", 100, 5)];

/// `fleet select` の歯の既定の鮮度（秒・`fleet.usage_fresh_s`）。**0** = 境が「いま」なので、いま以前の ts の実測は
/// どれも「新しい」と読まれず、撃つたびに全口座を測り直す（鮮度を持つ前の歯の前提を保つ・鮮度の歯は
/// [`fresh_select_fixture`] で値を持つ）。
const SELECT_FRESH_S: u64 = 0;

/// `fleet select` の rules fixture。待ち時間の行（`timeout`）と R-C9-1 の行（値の字面 `selection`）を持ち分け、
/// 鮮度の行は [`SELECT_FRESH_S`] で持つ。
fn select_rules(labels: &[&str], timeout: bool, selection: Option<&str>) -> String {
    select_rules_fresh(labels, timeout, selection, Some(SELECT_FRESH_S))
}

/// [`select_rules`] の鮮度の行（`fresh`・`None` = 行なし）まで持ち分ける形。
fn select_rules_fresh(labels: &[&str], timeout: bool, selection: Option<&str>, fresh: Option<u64>) -> String {
    let mut text = "schema = 1\n".to_owned();
    if timeout {
        text.push_str(&format!(
            "\n[[rule]]\nid = \"fleet.usage_timeout_s\"\nkind = \"UsageTimeoutS\"\nvalue = {USAGE_TIMEOUT_S}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n"
        ));
    }
    if let Some(secs) = fresh {
        text.push_str(&format!(
            "\n[[rule]]\nid = \"fleet.usage_fresh_s\"\nkind = \"UsageFreshS\"\nvalue = {secs}\nenabled = true\nruling = \"f\"\nruled_at = \"d\"\n"
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

/// 5 時間窓と 7 日窓だけの本文（`limits` は空＝モデル別の行なし・reset は口座で同じ）。
fn select_body(five: u64, seven: u64) -> String {
    select_body_resets(five, seven, SELECT_FIVE_RESET, SELECT_WEEK_RESET)
}

/// 両方の窓の reset を口座ごとに変えた本文（`select_body` の reset 違い）。
fn select_body_resets(five: u64, seven: u64, five_reset: &str, seven_reset: &str) -> String {
    format!(
        r#"{{"five_hour":{{"utilization":{five},"resets_at":"{five_reset}"}},"seven_day":{{"utilization":{seven},"resets_at":"{seven_reset}"}},"limits":[]}}"#
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

// ───── ADR-0042: 便用の 1 つ目の鍵は口座単位の 7 日窓の reset（接頭辞 `fleet_select_week_`） ─────

/// 7 日窓の鍵を測る置き場: a1 = 5 時間窓の reset が**早く**（2099-01-01T01Z）7 日窓の reset が**遅い**（2099-01-07）・
/// 20%。a2 = 5 時間窓の reset が遅く（2099-01-01T04Z）7 日窓の reset が早い（2099-01-05）・80%。
/// label 順でも逼迫度の最小でも a1 が先＝便用の答え a2 は 7 日窓の鍵でだけ出る（偶然では通らない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn week_reset_fixture() -> (UsageFixture, PathBuf) {
    let (fx, curl) = select_fixture(&[("a1", 20, 10), ("a2", 80, 10)], true, Some("85"));
    let a1 = select_body_resets(20, 10, "2099-01-01T01:00:00+00:00", "2099-01-07T00:00:00+00:00");
    let a2 = select_body_resets(80, 10, "2099-01-01T04:00:00+00:00", "2099-01-05T00:00:00+00:00");
    fs::write(fx.spy.join("body-tok-a1"), a1).expect("本文を書ける");
    fs::write(fx.spy.join("body-tok-a2"), a2).expect("本文を書ける");
    (fx, curl)
}

/// (a) CLI の便用は **7 日窓の reset が早い口座**を、5 時間窓の reset が早い口座より先に選ぶ（ADR-0042・C9.2
/// 「reset で消える枠から使い潰す」）→ `chosen=a2`。base（数える窓の reset の最小＝実質 5 時間窓）は a1 → RED。
/// 計測の行が両口座の 7 日窓の reset を名指す＝fixture が効いている証拠。
#[test]
fn fleet_select_week_prefers_the_earlier_seven_day_reset_over_the_earlier_five_hour_reset() {
    let (fx, curl) = week_reset_fixture();
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(
        out_lines(&out),
        vec!["select purpose=run chosen=a2".to_owned()],
        "7 日窓の reset が早い a2（5 時間窓の reset なら a1・label 順でも a1）: {out:?}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("usage: account=a1 five_hour=20% resets=2099-01-01T01:00:00Z seven_day=10% resets=2099-01-07T00:00:00Z"),
        "a1 の 7 日窓の reset は遅い: {stderr}"
    );
    assert!(
        stderr.contains("usage: account=a2 five_hour=80% resets=2099-01-01T04:00:00Z seven_day=10% resets=2099-01-05T00:00:00Z"),
        "a2 の 7 日窓の reset は早い: {stderr}"
    );
    drop_fixture(&fx);
}

/// (b) 同じ表の席用の答えは変わらない（ADR-0042 は便用の順序だけを差し替える）: `--purpose session` は逼迫度の
/// 最小 = a1（20%）で、(a) の便用の答え a2 とは別の口座＝同じ答えで偶然通らない。
#[test]
fn fleet_select_week_session_answer_is_unchanged_on_the_same_table() {
    let (fx, curl) = week_reset_fixture();
    let out = run_select(&fx, &curl, &["--purpose", "session"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(
        out_lines(&out),
        vec!["select purpose=session chosen=a1".to_owned()],
        "逼迫度の最小（20%）・便用の答え（a2）とは別の口座: {out:?}"
    );
    drop_fixture(&fx);
}

/// (h) 便用の答えは **7 日窓の reset を動かすと動く**（逼迫度でも 5 時間窓の reset でもない・ADR-0042）: a2 の 7 日窓が
/// 早い間は `chosen=a2`、a1 の 7 日窓を（5 時間窓は遅いまま）もっと早くすれば `chosen=a1`。どちらの段も base
/// （数える窓の最小＝5 時間窓）は逆の口座 → RED。実測行は `fleet select` が撃つ計測（偽 curl の本文）で置く。
#[test]
fn fleet_select_run_prefers_earliest_reset_over_pressure() {
    let (fx, curl) = week_reset_fixture();
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a2".to_owned()], "7 日窓の reset が近い a2: {out:?}");
    // a1 の 7 日窓を最も早くする（5 時間窓の reset は a2 より遅くしておく＝5 時間窓の鍵なら a2 のまま）。
    let a1 = select_body_resets(20, 10, "2099-01-01T10:00:00+00:00", "2099-01-03T00:00:00+00:00");
    fs::write(fx.spy.join("body-tok-a1"), a1).expect("本文を書ける");
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()], "7 日窓の reset が近い側へ動く: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("usage: account=a1 five_hour=20% resets=2099-01-01T10:00:00Z seven_day=10% resets=2099-01-03T00:00:00Z"),
        "a1 の 5 時間窓は a2 より遅い: {stderr}"
    );
    let out = run_select(&fx, &curl, &["--purpose", "session"]);
    assert_eq!(out_lines(&out), vec!["select purpose=session chosen=a1".to_owned()], "session 用は逼迫度の最小（20%）");
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
/// 値欠けは入口の閉包の断りで rc 2（設計 pipeline.md §14 約束 4）。field の無い SeatSpawned はこれまでどおり書けて `account`
/// 無しで読める（schema 1 のまま）。
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
    for (bad, rc) in [
        (&["record", "--kind", "RunStage", "--run", "r1", "--bead", "s2-x", "--stage", "Gated", "--account", "x", "--state-dir", &path][..], RC_REFUSED),
        (&["record", "--kind", "RunCreated", "--run", "r2", "--bead", "s2-x", "--account", "x", "--state-dir", &path][..], RC_REFUSED),
        (&["record", "--kind", "SeatStopped", "--run", "r1", "--bead", "s2-x", "--account", "x", "--state-dir", &path][..], RC_REFUSED),
        (&["record", "--kind", "SeatSpawned", "--run", "r1", "--bead", "s2-x", "--account", "--state-dir", &path][..], RC_BROKEN),
    ] {
        let refused = run_fleet(bad);
        assert_eq!(refused.status.code(), Some(i32::from(rc)), "{bad:?}: {refused:?}");
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
    // 便用の鍵は 7 日窓の reset（ADR-0042）: a2 の 7 日窓を早くすれば a2（a1 の 5 時間窓は reset 無しのまま）。
    let a2 = select_body_resets(50, 10, SELECT_FIVE_RESET, "2099-01-05T00:00:00+00:00");
    fs::write(fx.spy.join("body-tok-a2"), a2).expect("本文を書ける");
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a2".to_owned()], "便用は 7 日窓の reset が早い側（a2）");
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
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "値の無い --exclude は入口の閉包の断り: {out:?}");
    assert!(out.stdout.is_empty(), "選ばない");
    assert_eq!(curl_calls(&fx), calls, "断った周は計測しない");
    drop_fixture(&fx);
}

/// `--exclude` の直後に別の flag が来る周（`--exclude --purpose run`）は **値欠け**で usage に断られる
/// （入口の閉包の断りで rc 2・設計 pipeline.md §14 約束 4・chosen を出さない・次の flag を label に取らない・計測しない）。
/// `--` で始まる字面は label にならない。
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
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{extra:?}: 値欠けは断る: {out:?}");
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

/// (6) `--purpose` の未知の値・値欠け・欠落は usage で断る（計測しない・値欠けは入口の閉包の断りで rc 2）。
#[test]
fn fleet_select_refuses_unknown_purpose_with_usage() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    for (extra, rc) in [(&["--purpose", "lane"][..], RC_REFUSED), (&["--purpose"][..], RC_BROKEN), (&[][..], RC_REFUSED)] {
        let out = run_select(&fx, &curl, extra);
        assert_eq!(out.status.code(), Some(i32::from(rc)), "{extra:?}: {out:?}");
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

// ───── 選定の前計測の鮮度（設計 account-autonomy.md §13・FR36 / FR33・接頭辞 `fleet_select_fresh_`） ─────

/// 鮮度の歯の `fleet.usage_fresh_s`（秒）。歯の壁時計より十分に長い＝「いま」置いた実測は境より新しい。
const FRESH_S: u64 = 3600;

/// 鮮度の歯の「古い」実測の ts（`FRESH_S` より古い・reset は 2099 なので選定は古いと読まない）。
const STALE_TS: &str = "2026-09-12T02:00:00Z";

/// 偽 curl の本文の使用率（置いた実測の値と違えて、測り直したかを値で読む）。
const REMEASURED_PCT: u64 = 55;

/// 置いた実測の使用率。
const PLACED_PCT: u64 = 30;

/// いまの UTC の ts（実測行と同じ字面）。
fn now_ts() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or(0);
    format_utc(secs)
}

/// [`select_fixture`] の鮮度の行に `fresh` 秒を持つ形（偽 curl の本文は全口座 [`REMEASURED_PCT`] / 10）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fresh_select_fixture(labels: &[&str], fresh: u64) -> (UsageFixture, PathBuf) {
    let accounts: Vec<(&str, u64, u64)> = labels.iter().map(|label| (*label, REMEASURED_PCT, 10)).collect();
    let (fx, curl) = select_fixture(&accounts, true, Some("85"));
    fs::write(&fx.rules, select_rules_fresh(labels, true, Some("85"), Some(fresh))).expect("rules fixture を書ける");
    (fx, curl)
}

/// reset が遠い実測 1 件（選定が古いと読まない・`measured` の RESETS_AT は過去）。
fn far_measured(account: &str, window: WindowKind, used_pct: u64) -> Allowance {
    Allowance::Measured(Measured {
        account: account.to_owned(),
        window,
        model: None,
        endpoint: ENDPOINT.to_owned(),
        used_pct,
        resets_at: Some(SELECT_FIVE_RESET.to_owned()),
    })
}

/// 口座 1 つの実測の回（5 時間窓 [`PLACED_PCT`]・7 日窓 10）を `ts` で置く。
fn put_round(fx: &UsageFixture, ts: &str, label: &str) {
    append_allowance(
        &fx.state,
        vec![
            (ts, far_measured(label, WindowKind::FiveHour, PLACED_PCT)),
            (ts, far_measured(label, WindowKind::SevenDay, 10)),
        ],
    );
}

/// 置き場の replay から口座の最新の 5 時間窓（`Measured` なら使用率・`Unmeasured` なら `None`）。
fn latest_five_hour(fx: &UsageFixture, label: &str) -> Option<u64> {
    let events = store::read_all(&fx.state).unwrap_or_default();
    let state = replay(&events);
    let key = allowance_key(label, Some(WindowKind::FiveHour), None);
    match state.allowance.get(&key).map(|latest| &latest.allowance) {
        Some(Allowance::Measured(found)) => Some(found.used_pct),
        Some(Allowance::Unmeasured(_)) | None => None,
    }
}

/// (1) 形 (2): 最新の回が全部実測で ts が `now − fresh_s` より新しい口座（a1）は選定の前計測で測り直されず（偽 curl の
/// 呼出 0・event 不変・値は置いたまま）、古い実測の口座（a2）・行の無い口座（a3）・最新の回が Unmeasured の口座
/// （a4）だけが測られる（呼出 1 ずつ・値は偽 curl の本文）。直後にもう 1 回撃つと全口座が新しいので呼出 0。
/// base（条件なしで全口座を測る）は呼出 4 → RED。
#[test]
fn fleet_select_fresh_recent_measurement_is_not_remeasured_but_stale_unmeasured_and_absent_are() {
    let (fx, curl) = fresh_select_fixture(&["a1", "a2", "a3", "a4"], FRESH_S);
    let now = now_ts();
    put_round(&fx, &now, "a1");
    put_round(&fx, STALE_TS, "a2");
    // a4 の Unmeasured は「いま」と同じ秒に置かない（最新の回は ts の等値で束ねる＝同じ秒の測り直しと同じ回に
    // 束ねられて a4 が 2 回目にも測られる）。Unmeasured の口座は ts に関わらず測られるので古い ts で足りる。
    append_allowance(&fx.state, vec![(STALE_TS, unmeasured("a4", None, UnmeasuredReason::HttpStatus))]);
    let before = allowances(&fx).len();
    assert_eq!(before, 5, "置いた行: a1 ×2・a2 ×2・a4 ×1");

    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()], "同じ reset → label: {out:?}");
    assert_eq!(curl_calls(&fx), 3, "a2（古い）・a3（行なし）・a4（Unmeasured）だけを測る");
    assert_eq!(allowances(&fx).len(), before + 6, "測った 3 口座 × 2 窓だけが増える（a1 は増えない）");
    assert_eq!(latest_five_hour(&fx, "a1"), Some(PLACED_PCT), "a1 は置いた値のまま（測り直していない）");
    for label in ["a2", "a3", "a4"] {
        assert_eq!(latest_five_hour(&fx, label), Some(REMEASURED_PCT), "{label} は偽 curl の本文の値");
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&format!("usage: account=a1 five_hour={PLACED_PCT}% resets={SELECT_FIVE_RESET}")),
        "測らなかった口座も最新の実測の 1 行形で stderr へ: {stderr}"
    );
    assert!(!stderr.contains(" kept "), "届いた周に kept は出ない: {stderr}");

    let again = run_select(&fx, &curl, &["--purpose", "session"]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "{again:?}");
    assert_eq!(curl_calls(&fx), 3, "直後の周は全口座が新しい＝呼出 0");
    assert_eq!(allowances(&fx).len(), before + 6, "event も増えない");
    drop_fixture(&fx);
}

/// (2) 形 (3): 測り直した口座が 429（`http_status`）/ timeout を返し、最新の回が実測（古い・reset 前）の周は Unmeasured
/// を追記せず（event の本数不変）その実測を最新のまま使って候補に残す（`chosen=a1`・stdout は純関数の 1 行と 1 字も
/// 違わない）。stderr に `usage: account=a1 kept reason=<reason>` の 1 行（timeout の周は reason の語だけが違う）。
/// base（Unmeasured を追記して候補から外す）は `none=unmeasured` → RED。
#[test]
fn fleet_select_fresh_unreachable_keeps_the_stale_measurement_and_says_kept() {
    for (name, status, rc, reason) in [("429", "429", 0_u8, "http_status"), ("timeout", "200", 28, "timeout")] {
        let (fx, _) = fresh_select_fixture(&["a1"], FRESH_S);
        let curl = fake_curl(&fx, &select_body(REMEASURED_PCT, 10), status, rc);
        put_round(&fx, STALE_TS, "a1");
        let before = allowances(&fx).len();
        let out = run_select(&fx, &curl, &["--purpose", "run"]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{name}: {out:?}");
        let want = select::line(Purpose::Run, &Selection::Chosen("a1".to_owned()));
        assert_eq!(out_lines(&out), vec![want], "{name}: 候補に残る・stdout は 1 行形のまま: {out:?}");
        assert_eq!(curl_calls(&fx), 1, "{name}: 古い実測の口座は測り直す");
        assert_eq!(allowances(&fx).len(), before, "{name}: Unmeasured を追記しない");
        assert_eq!(latest_five_hour(&fx, "a1"), Some(PLACED_PCT), "{name}: 最新は置いた実測のまま");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.lines().any(|line| line == format!("usage: account=a1 kept reason={reason}")),
            "{name}: kept の 1 行が label と理由を運ぶ: {stderr}"
        );
        assert!(
            stderr.contains(&format!("usage: account=a1 five_hour={PLACED_PCT}% resets={SELECT_FIVE_RESET}")),
            "{name}: 保った実測の 1 行形: {stderr}"
        );
        assert!(!stderr.contains("unmeasured"), "{name}: 届かなかった側の行は出さない: {stderr}");
        drop_fixture(&fx);
    }
}

/// (3) 否定の枝: 429 でも最新の回が Unmeasured の口座・行の無い口座は従来どおり Unmeasured が追記され（event +1）
/// 候補から外れ（`none=unmeasured`）、kept の 1 行は出ない。
#[test]
fn fleet_select_fresh_unreachable_with_unmeasured_or_absent_latest_appends_without_kept() {
    for prior in [Some(UnmeasuredReason::Timeout), None] {
        let (fx, _) = fresh_select_fixture(&["a1"], FRESH_S);
        let curl = fake_curl(&fx, &select_body(REMEASURED_PCT, 10), "429", 0);
        if let Some(reason) = prior {
            append_allowance(&fx.state, vec![(STALE_TS, unmeasured("a1", None, reason))]);
        }
        let before = allowances(&fx).len();
        let out = run_select(&fx, &curl, &["--purpose", "run"]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{prior:?}: {out:?}");
        let lines = out_lines(&out);
        assert_eq!(lines.len(), 1, "{prior:?}: 1 行: {lines:?}");
        assert!(lines[0].starts_with("select purpose=run none=unmeasured"), "{prior:?}: 候補から外れる: {lines:?}");
        assert_eq!(curl_calls(&fx), 1, "{prior:?}: 測る");
        assert_eq!(allowances(&fx).len(), before + 1, "{prior:?}: 口座単位の Unmeasured を追記する");
        assert_eq!(latest_five_hour(&fx, "a1"), None, "{prior:?}: 最新は Unmeasured");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains(" kept "), "{prior:?}: kept は出ない: {stderr}");
        assert!(stderr.contains("usage: account=a1 unmeasured reason=http_status"), "{prior:?}: 従来の行: {stderr}");
        drop_fixture(&fx);
    }
}

/// (4) 形 (4): `fleet usage` の口は鮮度に関わらず全口座を測り（`Always`・新しい実測の a1 も測り直す・呼出 = 口座数）、
/// 429 の周も従来どおり Unmeasured を追記して kept を出さない（外形不変）。
#[test]
fn fleet_select_fresh_usage_mouth_measures_every_account_regardless_of_freshness() {
    let (fx, curl) = fresh_select_fixture(&["a1", "a2"], FRESH_S);
    put_round(&fx, &now_ts(), "a1");
    let out = run_usage(&fx, &curl, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(curl_calls(&fx), 2, "新しい実測の a1 も測る");
    assert_eq!(allowances(&fx).len(), 2 + 4, "2 口座 × 2 窓が増える");
    assert_eq!(latest_five_hour(&fx, "a1"), Some(REMEASURED_PCT), "a1 は測り直した値");
    assert_eq!(out_lines(&out).len(), 2, "口座ごと 1 行: {out:?}");

    let failing = fake_curl(&fx, &select_body(REMEASURED_PCT, 10), "429", 0);
    let out = run_usage(&fx, &failing, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(
        out_lines(&out),
        vec![
            "usage: account=a1 unmeasured reason=http_status".to_owned(),
            "usage: account=a2 unmeasured reason=http_status".to_owned()
        ],
        "429 は従来どおり Unmeasured の行"
    );
    assert_eq!(allowances(&fx).len(), 6 + 2, "口座単位の Unmeasured を追記する");
    assert!(!String::from_utf8_lossy(&out.stderr).contains(" kept "), "fleet usage は kept を出さない: {out:?}");
    drop_fixture(&fx);
}

/// (5) 鮮度の行の無い manifest は `fleet.usage_timeout_s` の読み手と**同じ極性**で断る（rc 1・stdout 0 byte・測らない・
/// 書かない）。断りの字面は行 id だけが違う（読み手を増やしていない）。
#[test]
fn fleet_select_fresh_rules_without_the_row_refuse_like_the_timeout_row() {
    let labels: Vec<&str> = SELECT_THREE.iter().map(|(label, _, _)| *label).collect();
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    fs::write(&fx.rules, select_rules_fresh(&labels, true, Some("85"), None)).expect("rules fixture を書ける");
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{out:?}");
    assert!(out.stdout.is_empty(), "選ばない");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(stderr.contains("fleet usage: manifest を読めない（fleet.usage_fresh_s が無い）"), "断りの 1 行: {stderr}");
    assert_eq!(curl_calls(&fx), 0, "client を起こさない");
    assert!(!store::events_path(&fx.state).exists(), "event を書かない");

    fs::write(&fx.rules, select_rules_fresh(&labels, false, Some("85"), Some(0))).expect("rules fixture を書ける");
    let other = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(other.status.code(), out.status.code(), "rc は待ち時間の行の無い周と同じ: {other:?}");
    assert_eq!(
        String::from_utf8_lossy(&other.stderr),
        stderr.replace("fleet.usage_fresh_s", "fleet.usage_timeout_s"),
        "字面は行 id だけが違う"
    );
    assert_eq!(curl_calls(&fx), 0, "どちらも測らない");
    drop_fixture(&fx);
}

// ───── 便用の除外は便の repo（anchor）の席だけ（設計 account-autonomy.md §14・FR36 / FR40・接頭辞 `fleet_select_anchor_`） ─────

/// 席の登録 row を anchor つきで 1 件積む（[`register_account`] と同型・鍵は (役割, anchor) なので anchor 違いの
/// 2 row は両方残る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn register_anchored_account(dir: &Path, anchor: &str, label: &str) {
    let mut event = registration_event("s:w");
    event.registration = event
        .registration
        .map(|row| Registration { anchor: anchor.to_owned(), account: label.to_owned(), ..row });
    store::append(dir, &event, LockPolicy::embedded().expect("lock の規則を読める")).expect("登録 row を積める");
}

/// (a) 2 anchor の登録 row（anchor X の席 = a1・anchor Y の席 = a2）を置いた置き場で `--purpose run --anchor X` は
/// X の席の口座だけを外す＝他 repo の席の口座 a2 が候補に入り `chosen=a2`（a3 は 100 で当たっている）。一致は
/// 登録が書いた値との `OsStr` の等値で正規化しない（FR40）: 末尾 `/` の違う `--anchor X/` はどの row とも一致せず
/// 除外 0 で `chosen=a1`。base は `--anchor` を読まず全 row も読まないので `chosen=a1` → RED。
#[test]
fn fleet_select_anchor_keeps_other_repo_seat_accounts() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    register_anchored_account(&fx.state, "/repo/x", "a1");
    register_anchored_account(&fx.state, "/repo/y", "a2");
    let out = run_select(&fx, &curl, &["--purpose", "run", "--anchor", "/repo/x"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a2".to_owned()], "X の席 a1 だけ外れ、Y の席 a2 は候補");
    let out = run_select(&fx, &curl, &["--purpose", "run", "--anchor", "/repo/y"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()], "Y の席 a2 だけ外れ、X の席 a1 は候補");
    let out = run_select(&fx, &curl, &["--purpose", "run", "--anchor", "/repo/x/"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()], "正規化しない: 末尾 / はどの row とも一致せず除外 0");
    drop_fixture(&fx);
}

/// (b) 同じ fixture で `--anchor` 無しは従来どおり置き場の全 row の口座を外す（保守側）: a1 も a2 も外れ a3 は窓
/// 100 → 候補なし（`all-limited`・rc 0）。base の cli は row を読まず `--exclude` だけで除外していたので
/// `chosen=a1` → RED。加えて `--purpose session --anchor X` と値欠けの `--anchor` は usage で断る（rc 1・選ばない・
/// 計測しない）。
#[test]
fn fleet_select_anchor_absent_excludes_every_seat_account() {
    let (fx, curl) = select_fixture(SELECT_THREE, true, Some("85"));
    register_anchored_account(&fx.state, "/repo/x", "a1");
    register_anchored_account(&fx.state, "/repo/y", "a2");
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "候補なしは断りではない: {out:?}");
    assert_eq!(
        out_lines(&out),
        vec![format!("select purpose=run none=all-limited earliest_reset={SELECT_FIVE_RESET}")],
        "--anchor 無しは全 row の口座を外す"
    );
    let calls = curl_calls(&fx);
    let out = run_select(&fx, &curl, &["--purpose", "session", "--anchor", "/repo/x"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "session 用に --anchor は無い: {out:?}");
    assert!(out.stdout.is_empty(), "選ばない");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("fleet: --anchor は --purpose run だけが取る"), "断りの理由: {stderr}");
    assert!(stderr.contains("usage: fleet") && stderr.contains("[--anchor DIR]"), "使い方を stderr へ: {stderr}");
    let out = run_select(&fx, &curl, &["--purpose", "run", "--anchor"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "値の無い --anchor は入口の閉包の断り: {out:?}");
    assert!(out.stdout.is_empty(), "選ばない");
    assert!(String::from_utf8_lossy(&out.stderr).contains("fleet: --anchor に値が無い"), "{out:?}");
    assert_eq!(curl_calls(&fx), calls, "断った周は計測しない");
    drop_fixture(&fx);
}

/// (c) `pipe spawn` は便の repo（`--repo X`）を選定に渡す: 口座 a1 / a2（どちらも余裕）の置き場に、anchor = X の
/// path そのものの席 = a1・別 path の席 = a2 の row を置くと、初回の起動の選定（`follow.rs` `spawn_selected`）は
/// X の席 a1 だけを外して a2 を選び、`Spawned` の detail が `account:a2` を持つ（`base:<sha>,account:<label>`）。
/// base は全 row を除外して候補が空（reset を持たない `none=excluded`）→ rc 3 `next=wait reset=-` で止まり
/// `Spawned` は無い → RED（reset 2099 の窓の口座を置かないので待ちに入らず hang しない）。
///
/// 便の側の fixture は `super::pipe` の helper（repo と置き場・契約・intake・`pipe` の起動）で、口座の側は
/// [`select_fixture`]（credential + 偽 curl + rules）。`pipe` の manifest は rules fixture に `runner.model`
/// （[`vessel::pipe::ratelimit::Pool`] は口座を宣言する置き場に要る）と lock の 2 行（`pipe` の dispatch が読む）を
/// 足した写し。
#[test]
fn fleet_select_anchor_pipe_run_passes_the_repo() {
    use std::os::unix::fs::PermissionsExt;
    let (fx, curl) = select_fixture(&[("a1", 30, 10), ("a2", 20, 10)], true, Some("85"));
    let (repo, state) = super::pipe::repo_with_state_in(&fx.state);
    let mut rules = fs::read_to_string(&fx.rules).expect("rules fixture を読める");
    for (id, kind, value) in [
        ("runner.model", "RunnerModel", "\"opus\""),
        ("fleet.lock_retry_ms", "LockRetryMs", "5000"),
        ("fleet.lock_stale_ms", "LockStaleMs", "30000"),
    ] {
        rules.push_str(&format!(
            "\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n"
        ));
    }
    let rules_path = fx.spy.join("rules-pipe.toml");
    fs::write(&rules_path, rules).expect("pipe の manifest を書ける");
    let runner = fx.spy.join("runner.sh");
    fs::write(&runner, "#!/bin/sh\ncat >/dev/null\nexit 0\n").expect("stub runner を書ける");
    let mut perm = fs::metadata(&runner).expect("stub の権限を読める").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&runner, perm).expect("stub を実行可能にできる");
    let contract = super::pipe::write_contract(&repo, &[], &[]);
    let id = super::pipe::intake_bead(&repo, &state, &contract, "s2-anchor");
    // 席の row: 便の repo の path そのものを anchor に持つ席 = a1・別 path の席 = a2。
    register_anchored_account(&state, &repo.display().to_string(), "a1");
    register_anchored_account(&state, "/repo/other", "a2");
    let out = super::pipe::run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &format!("sh {}", runner.display()),
        "--rules", &rules_path.display().to_string(), "--curl", &curl.display().to_string(),
    ]);
    let stdout = super::pipe::stdout_of(&out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stdout.contains("next=wait"), "候補が在るので待たない: {stdout} / {stderr}");
    assert_eq!(curl_calls(&fx), 2, "起動の前に FR33 の計測を 1 回（口座 2 つ）: {stderr}");
    let spawned: Vec<String> = store::read_all(&state)
        .expect("event log を読める")
        .into_iter()
        .filter(|event| event.run == id && event.stage == Some(Stage::Spawned))
        .filter_map(|event| event.detail)
        .collect();
    assert_eq!(spawned.len(), 1, "runner を 1 回起こした: {spawned:?} / {stdout} / {stderr}");
    assert!(
        spawned.first().is_some_and(|detail| detail.starts_with("base:") && detail.ends_with(",account:a2")),
        "便の repo の席 a1 だけを外し、他 repo の席 a2 で起きる: {spawned:?}"
    );
    super::pipe::clean(&[&repo]);
    drop_fixture(&fx);
}

// ─── 便用の選定は群ごとの今の口座だけを host 全体で外す（account-lifecycle.md §23・§17 の約束 4 の改め・接頭辞 `host_group_`） ───

/// 余裕の在る 3 口座（どれも当たっていない・7 日窓の reset は同じ＝便用の並びは label に落ちる）。
const GROUP_THREE: &[(&str, u64, u64)] = &[("a1", 30, 10), ("a2", 20, 10), ("a3", 25, 10)];

/// [`select_fixture`] の置き場を一時 dir の 1 段下（`<root>/place`）へ移した形。群の今の口座の記録は host の根（置き場の
/// 親の下）に在るので、tmp の直下の置き場では記録が歯どうしで共有される＝群の歯は置き場ごとに host の根を閉じる。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn group_fixture(accounts: &[(&str, u64, u64)]) -> (UsageFixture, PathBuf) {
    let (mut fx, curl) = select_fixture(accounts, true, Some("85"));
    let place = fx.root.join("place");
    fs::create_dir_all(&place).expect("置き場を作れる");
    fs::rename(fx.root.join("accounts"), place.join("accounts")).expect("credential を置き場へ移せる");
    fx.state = place;
    (fx, curl)
}

/// host の根の群用 dir（`<置き場の親>/<NAME>-host/groups`・器の字面を借りない）に群 `name` の今の口座の記録を置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_group_record(fx: &UsageFixture, name: &str, body: &str) -> PathBuf {
    let dir = fx.root.join(format!("{}-host", vessel::name::NAME)).join("groups");
    fs::create_dir_all(&dir).expect("群用 dir を作れる");
    let path = dir.join(format!("{name}.account"));
    fs::write(&path, body).expect("記録を書ける");
    path
}

/// 記録の本文（[`vessel::hook::group::Record::render`] の形・今の口座 = `account`）。
fn record_body(account: &str) -> String {
    format!("account={account}\nts=2026-09-25T00:00:00Z\nreason=move\nprevious=a1\n")
}

/// `fx` の置き場に群を宣言した host の面を置く（口座の表は持たない＝候補は tracked の面〔`--rules`〕の label を指す）。
/// `groups` は (名, 置き場の列, 候補の口座の列) の宣言順。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_groups(fx: &UsageFixture, groups: &[(&str, &[&str], &[&str])]) {
    let quoted = |items: &[&str]| items.iter().map(|item| format!("\"{item}\"")).collect::<Vec<String>>().join(", ");
    let body = groups.iter().fold("schema = 1\n".to_owned(), |body, (name, anchors, accounts)| {
        format!(
            "{body}\n[[account-group]]\nname = \"{name}\"\nanchors = [{}]\naccounts = [{}]\n",
            quoted(anchors),
            quoted(accounts)
        )
    });
    fs::write(fx.state.join(vessel::rules::HOST_MANIFEST), body).expect("host の面を書ける");
}

/// (a) 群 [a1, a2, a3] の記録 = a2 → 便用の候補から外れるのは **a2 だけ**（`fleet select --purpose run` の口）: 並びの先頭
/// a1 が選ばれ、`--exclude a1` を重ねると a3 が選ばれ（a1 と a3 は候補に残る）、両方を `--exclude` すると候補なし
/// （`excluded`＝a2 は候補に戻らない）。除外は置き場（anchor）で絞らない——群の置き場と関係の無い `--anchor` を付けても
/// 同じ答え（host 全体で外す）。base は群の候補の全部を外すので 1 周目が `none=excluded` → RED。
#[test]
fn host_group_run_selection_drops_only_the_current_account() {
    let (fx, curl) = group_fixture(GROUP_THREE);
    put_groups(&fx, &[("alpha", &["/repo/a"], &["a1", "a2", "a3"])]);
    put_group_record(&fx, "alpha", &record_body("a2"));
    for anchor in [&[][..], &["--anchor", "/repo/elsewhere"]] {
        let pick = |more: &[&str]| {
            let extra: Vec<&str> = ["--purpose", "run"].iter().chain(anchor).chain(more).copied().collect();
            let out = run_select(&fx, &curl, &extra);
            assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{extra:?}: {out:?}");
            out_lines(&out)
        };
        assert_eq!(pick(&[]), ["select purpose=run chosen=a1"], "{anchor:?}: 記録の a2 だけが外れ a1 は候補");
        assert_eq!(pick(&["--exclude", "a1"]), ["select purpose=run chosen=a3"], "{anchor:?}: a3 も候補に残る");
        assert_eq!(
            pick(&["--exclude", "a1", "--exclude", "a3"]),
            ["select purpose=run none=excluded earliest_reset=-"],
            "{anchor:?}: 今の口座 a2 は候補に戻らない"
        );
    }
    drop_fixture(&fx);
}

/// (b) 記録の無い群は種（宣言の候補の先頭 a1）だけが外れる: a2 が選ばれ、`--exclude a2` を重ねると a3（候補の残りは便に開く）。
/// base は群の候補の全部を外すので `none=excluded` → RED。
#[test]
fn host_group_run_selection_drops_the_seed_without_a_record() {
    let (fx, curl) = group_fixture(GROUP_THREE);
    put_groups(&fx, &[("alpha", &["/repo/a"], &["a1", "a2", "a3"])]);
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), ["select purpose=run chosen=a2"], "種 a1 だけが外れる");
    let out = run_select(&fx, &curl, &["--purpose", "run", "--exclude", "a2"]);
    assert_eq!(out_lines(&out), ["select purpose=run chosen=a3"], "a3 も候補: {out:?}");
    drop_fixture(&fx);
}

/// (b) 記録が在るのに読めない群（形の崩れた file・file の位置の dir）は typed に断る: rc 1・stdout 0 byte・stderr に群の名と
/// 読めなさの型・計測を撃たない（client の呼出 0）＝候補の全部にも種にも読み替えず 1 つも返さない。置き場から解く 1 本も
/// 同じ断り（[`vessel::rules::GroupedError::Record`]）。session 用は群を読まないので同じ置き場で選ぶ（1 字も変えない）。
#[test]
fn host_group_run_selection_refuses_an_unreadable_record() {
    let (fx, curl) = group_fixture(GROUP_THREE);
    put_groups(&fx, &[("alpha", &["/repo/a"], &["a1", "a2", "a3"])]);
    let path = put_group_record(&fx, "alpha", "account=a2\nts=2026-09-25T00:00:00Z\n");
    let cases = [(RecordError::Malformed, "record=malformed"), (RecordError::Unreadable, "record=unreadable")];
    for (error, word) in cases {
        if error == RecordError::Unreadable {
            fs::remove_file(&path).expect("file を外せる");
            fs::create_dir_all(&path).expect("記録の位置に dir を置ける");
        }
        let out = run_select(&fx, &curl, &["--purpose", "run"]);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{word}: {out:?}");
        assert!(out.stdout.is_empty(), "{word}: 選ばない: {out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(stderr.contains(&format!("group=alpha {word} ")), "{word}: 断りは群と型を名指す: {stderr}");
        assert_eq!(curl_calls(&fx), 0, "{word}: 測らない");
        assert_eq!(vessel::rules::grouped_accounts(&fx.state), Err(GroupedError::Record("alpha".to_owned(), error)));
    }
    let out = run_select(&fx, &curl, &["--purpose", "session"]);
    assert_eq!(out_lines(&out), ["select purpose=session chosen=a2"], "session 用は群を読まない: {out:?}");
    drop_fixture(&fx);
}

/// (d) 除外の集合は群ごとの今の口座を畳んだもの（置き場から解く 1 本を直に読む）: 記録の無い 2 群（候補 [a1, a2] と
/// [a1, a3]）の種は宣言順に重ならない（alpha は a1・beta は a1 でない最初の候補 a3・§28）ので除外は 2 つ、beta の記録が a1
/// なら 2 群が同じ今の口座で除外は 1 つ。`fleet select` の口でも同じ: 2 つの周は a2 で `--exclude a2` を重ねると候補なし、
/// 1 つの周は `--exclude a2` で a3。base は種がどちらも先頭 a1（除外 1 つ）→ RED。
#[test]
fn host_group_run_exclusion_folds_the_current_accounts_of_the_groups() {
    let (fx, curl) = group_fixture(GROUP_THREE);
    put_groups(&fx, &[("alpha", &["/repo/a"], &["a1", "a2"]), ("beta", &["/repo/b"], &["a1", "a3"])]);
    let set = |labels: &[&str]| -> BTreeSet<String> { labels.iter().map(|label| (*label).to_owned()).collect() };
    assert_eq!(vessel::rules::grouped_accounts(&fx.state), Ok(set(&["a1", "a3"])), "種は重ならない＝別の今の口座は 2 つ");
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out_lines(&out), ["select purpose=run chosen=a2"], "候補の残り a2 は便に開く: {out:?}");
    let out = run_select(&fx, &curl, &["--purpose", "run", "--exclude", "a2"]);
    assert_eq!(out_lines(&out), ["select purpose=run none=excluded earliest_reset=-"], "除外 2 つ: {out:?}");
    put_group_record(&fx, "beta", &record_body("a1"));
    assert_eq!(vessel::rules::grouped_accounts(&fx.state), Ok(set(&["a1"])), "同じ今の口座は 1 つ");
    let out = run_select(&fx, &curl, &["--purpose", "run", "--exclude", "a2"]);
    assert_eq!(out_lines(&out), ["select purpose=run chosen=a3"], "除外 1 つ: {out:?}");
    drop_fixture(&fx);
}

/// (e) 群に属さない口座は便用の候補に残る（a1 だけを持つ群 → a2 が選ばれる）。席の登録 row の除外はそのまま重なる:
/// a2 を口座に持つ席の row を便の anchor に置くと、a1（群の今の口座）も a2（席）も外れて候補なしになる。
#[test]
fn host_group_ungrouped_account_stays_in_the_run_candidates() {
    let (fx, curl) = group_fixture(SELECT_THREE);
    put_groups(&fx, &[("alpha", &["/repo/a"], &["a1"])]);
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a2".to_owned()], "群の a1 だけ外れ、a2 は候補");
    register_anchored_account(&fx.state, "/repo/x", "a2");
    let out = run_select(&fx, &curl, &["--purpose", "run", "--anchor", "/repo/x"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(
        out_lines(&out),
        vec![format!("select purpose=run none=all-limited earliest_reset={SELECT_FIVE_RESET}")],
        "席の登録 row の除外の**上に**群の除外が重なる"
    );
    drop_fixture(&fx);
}

/// (f) 群を 1 つも宣言しない host は便用の候補が今までどおり（host の面が無い周・`schema = 1` だけの周・群 0 の
/// `[[plugin]]` だけの周のどれも `chosen=a1`）。同じ置き場で面を書き換えながら撃つので、除外が**次の選定から**
/// 効く（選定のたびに宣言を読み直す＝前の周の宣言を覚えない）ことも同時に測る。群 0 の除外は席の登録 row の口座だけ
/// （a1 の row を便の anchor に置くと a1 だけが外れて a2）。
#[test]
fn host_group_zero_groups_keeps_the_run_candidates() {
    let (fx, curl) = group_fixture(SELECT_THREE);
    let host = fx.state.join(vessel::rules::HOST_MANIFEST);
    // 群を宣言した周は a1 が外れ、外した宣言を消せば**次の選定で**また候補に戻る。
    put_groups(&fx, &[("alpha", &["/repo/a"], &["a1"])]);
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a2".to_owned()], "宣言の在る周: {out:?}");
    for body in [None, Some("schema = 1\n"), Some("schema = 1\n\n[[plugin]]\ndir = \"/opt/p\"\n")] {
        match body {
            Some(text) => fs::write(&host, text).expect("host の面を書ける"),
            None => {
                fs::remove_file(&host).ok();
            }
        }
        let out = run_select(&fx, &curl, &["--purpose", "run"]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{body:?}: {out:?}");
        assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()], "{body:?}: 除外 0 件");
    }
    register_anchored_account(&fx.state, "/repo/x", "a1");
    let out = run_select(&fx, &curl, &["--purpose", "run", "--anchor", "/repo/x"]);
    assert_eq!(out_lines(&out), ["select purpose=run chosen=a2"], "群 0 は登録 row の口座だけが外れる: {out:?}");
    drop_fixture(&fx);
}

/// (g) session 用の候補には群の口座が残る（群が a1 を持っていても `--purpose session` は `chosen=a1`）＝席を起こす
/// 口座の選び方は 1 行も変えない。便用の**並べ順**も変わらない（群が a3 だけを持つ周は従来どおり `chosen=a1`）。
#[test]
fn host_group_session_selection_keeps_the_group_accounts() {
    let (fx, curl) = group_fixture(SELECT_THREE);
    put_groups(&fx, &[("alpha", &["/repo/a"], &["a1"])]);
    let out = run_select(&fx, &curl, &["--purpose", "session"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=session chosen=a1".to_owned()], "session 用は群を読まない");
    put_groups(&fx, &[("alpha", &["/repo/a"], &["a3"])]);
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=run chosen=a1".to_owned()], "外れるのが候補外の口座なら並びは不変");
    // 群の「今の口座」の記録は 1 件も書かれない（event log の kind は計測と便の行だけ）。
    let kinds: Vec<EventKind> =
        store::read_all(&fx.state).expect("event log を読める").into_iter().map(|event| event.kind).collect();
    assert!(
        kinds.iter().all(|kind| matches!(kind, EventKind::AllowanceMeasured | EventKind::AllowanceUnmeasured)),
        "群の記録は書かない: {kinds:?}"
    );
    drop_fixture(&fx);
}

/// (c) 便用の候補を作る**もう 1 つの口**（`select_for_run`・`pipe spawn` が通る経路）も同じ 1 本の除外を読む: 余裕の在る
/// 口座 a1 / a2 の置き場で群 [a1, a2] の記録が a2 だと、席の登録 row が 1 件も無くても起動は a1 を選ぶ（`Spawned` の
/// detail が `account:a1`・種 a1 は外れない）。base は群の候補の全部を外すので候補なしの待ち → RED。
#[test]
fn host_group_run_selection_applies_to_the_spawn_mouth() {
    use std::os::unix::fs::PermissionsExt;
    let (fx, curl) = group_fixture(&[("a1", 30, 10), ("a2", 20, 10)]);
    let (repo, state) = super::pipe::repo_with_state_in(&fx.state);
    put_groups(&fx, &[("alpha", &["/repo/a"], &["a1", "a2"])]);
    put_group_record(&fx, "alpha", &record_body("a2"));
    let mut rules = fs::read_to_string(&fx.rules).expect("rules fixture を読める");
    for (id, kind, value) in [
        ("runner.model", "RunnerModel", "\"opus\""),
        ("fleet.lock_retry_ms", "LockRetryMs", "5000"),
        ("fleet.lock_stale_ms", "LockStaleMs", "30000"),
    ] {
        rules.push_str(&format!(
            "\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n"
        ));
    }
    let rules_path = fx.spy.join("rules-group.toml");
    fs::write(&rules_path, rules).expect("pipe の manifest を書ける");
    let runner = fx.spy.join("runner-group.sh");
    fs::write(&runner, "#!/bin/sh\ncat >/dev/null\nexit 0\n").expect("stub runner を書ける");
    let mut perm = fs::metadata(&runner).expect("stub の権限を読める").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&runner, perm).expect("stub を実行可能にできる");
    let contract = super::pipe::write_contract(&repo, &[], &[]);
    let id = super::pipe::intake_bead(&repo, &state, &contract, "s2-group");
    let out = super::pipe::run_pipe(&[
        "spawn", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &format!("sh {}", runner.display()),
        "--rules", &rules_path.display().to_string(), "--curl", &curl.display().to_string(),
    ]);
    let stdout = super::pipe::stdout_of(&out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stdout.contains("next=wait"), "候補が在るので待たない: {stdout} / {stderr}");
    let spawned: Vec<String> = store::read_all(&state)
        .expect("event log を読める")
        .into_iter()
        .filter(|event| event.run == id && event.stage == Some(Stage::Spawned))
        .filter_map(|event| event.detail)
        .collect();
    assert_eq!(spawned.len(), 1, "runner を 1 回起こした: {spawned:?} / {stdout} / {stderr}");
    assert!(
        spawned.first().is_some_and(|detail| detail.ends_with(",account:a1")),
        "群の今の口座 a2 だけが起動の選定からも外れる: {spawned:?}"
    );
    super::pipe::clean(&[&repo]);
    drop_fixture(&fx);
}

// ─── `fleet usage` の 2 旗（account-lifecycle.md §19 形 5・席の hook が起こす子の口・接頭辞 `fleet_usage_narrowed_`） ───

/// 置き場の実測の行の口座 label（行の順・重複は残す）。
fn measured_accounts(fx: &UsageFixture) -> Vec<String> {
    allowances(fx)
        .into_iter()
        .map(|row| match row {
            Allowance::Measured(found) => found.account,
            Allowance::Unmeasured(found) => found.account,
        })
        .collect()
}

/// (a) `--account a2` は宣言の 1 口座だけを測る: 偽 client の呼出は 1 回（a1 は 0 回）・stdout は a2 の 1 行・追記される行は
/// a2 の 2 窓だけ。旗の無い口は同じ置き場で 2 口座とも測る（対）。
#[test]
fn fleet_usage_narrowed_account_measures_only_the_named_account() {
    let (fx, curl) = fresh_select_fixture(&["a1", "a2"], FRESH_S);
    let out = run_usage(&fx, &curl, &["--account", "a2"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    let lines = out_lines(&out);
    assert_eq!(lines.len(), 1, "a2 の 1 行だけ: {lines:?}");
    assert!(lines.first().is_some_and(|line| line.starts_with("usage: account=a2 ")), "{lines:?}");
    assert_eq!(curl_calls(&fx), 1, "名指した 1 口座だけ client を起こす");
    assert_eq!(measured_accounts(&fx), vec!["a2".to_owned(), "a2".to_owned()], "追記は a2 の 2 窓だけ");
    let out = run_usage(&fx, &curl, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(curl_calls(&fx), 3, "旗の無い口は 2 口座とも測る（1 + 2）");
    drop_fixture(&fx);
}

/// (b) 宣言に無い label は typed に断る: rc 1・stdout 0 byte・stderr に名指した label の 1 行・client の呼出 0・event 0。
/// 値の無い `--account` も同じく断る（黙って全口座に倒さない）。
#[test]
fn fleet_usage_narrowed_account_refuses_an_undeclared_label_without_calls() {
    let (fx, curl) = fresh_select_fixture(&["a1", "a2"], FRESH_S);
    let out = run_usage(&fx, &curl, &["--account", "nope"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{out:?}");
    assert!(out.stdout.is_empty(), "測らない: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(stderr.contains("--account nope"), "断りは label を名指す: {stderr}");
    assert_eq!(curl_calls(&fx), 0, "client を起こさない");
    assert!(!store::events_path(&fx.state).exists(), "event を書かない");
    let bare = run_usage(&fx, &curl, &["--fresh", "--account"]);
    assert_ne!(bare.status.code(), Some(i32::from(RC_OK)), "値の無い --account は断る: {bare:?}");
    assert_eq!(curl_calls(&fx), 0, "値の無い周も client を起こさない");
    drop_fixture(&fx);
}

/// (c) `--fresh` は選定の前計測と同じ鮮度つき: 新しい実測（いまの ts）を持つ a1 は測り直さず（値は置いたまま）、行の無い a2
/// だけを測る（呼出 1）。`--fresh --account a1` は新しい a1 を測らない（呼出 0 のまま）。旗の無い口の鮮度なしは
/// `fleet_select_fresh_usage_mouth_measures_every_account_regardless_of_freshness` が pin する。
#[test]
fn fleet_usage_narrowed_fresh_skips_the_recently_measured_account() {
    let (fx, curl) = fresh_select_fixture(&["a1", "a2"], FRESH_S);
    put_round(&fx, &now_ts(), "a1");
    let out = run_usage(&fx, &curl, &["--fresh"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(curl_calls(&fx), 1, "鮮度の外の a2 だけを測る");
    assert_eq!(latest_five_hour(&fx, "a1"), Some(PLACED_PCT), "a1 は置いた実測のまま");
    assert_eq!(latest_five_hour(&fx, "a2"), Some(REMEASURED_PCT), "a2 は測った値");
    let out = run_usage(&fx, &curl, &["--fresh", "--account", "a1"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(curl_calls(&fx), 1, "新しい a1 は名指しても測り直さない");
    assert_eq!(out_lines(&out).len(), 1, "a1 の 1 行（最新の実測の 1 行形）: {out:?}");
    drop_fixture(&fx);
}
