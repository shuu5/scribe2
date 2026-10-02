//! fleet event log の歯（設計 docs/design/fleet-event-log.md §6）。
//!
//! 置き場は毎回 tmp dir を `--state-dir` で指す（env も HOME も読まない形の裏返し）。
//!
//! 歯は族ごとの子 module に置く（設計 docs/design/carry-prep.md §9 行 h・`s2-07l.680`）: `usage`（接頭辞
//! `fleet_usage_` / `fleet_allowance_`）・`account`（接頭辞 `account_cmd_`）・`json`（接頭辞 `fleet_json_` /
//! `fleet_read_` / `fleet_record_` / `fleet_replay_` / `fleet_export_`）。この file には共有の helper と const・
//! 外形 snapshot の歯（snapshot 名が module path を含むので動かさない）・本文に `super::` を持つ族（`fleet_select_` /
//! `host_group_`・入れ子の子では指す先が変わる）・他の族の歯だけを残す。

mod account;
mod json;
mod usage;
// flip-check: moved s2-07l.680

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
        case: None,
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

/// `KINDS` の並びが**宣言順**と一致し、母集団は 30 種で末尾の 14 個が `InstallRecorded`（`vessel update` が足した・設計
/// consumer-sync.md §5 (4)）→ `RunCost`（消費の 1 件・gate-cost.md §26 形 (2)）→ `RulingReceived`（run 無しの裁定・
/// fleet-event-log.md §9）→ `GroupPressureNotified`（群の逼迫の通知・account-lifecycle.md §19 形 3）→ `GroupMoved` /
/// `GroupMoveRefused` / `GroupMovePending`（群の移動の承認・断り・保留・account-lifecycle.md §20 形 5 / 6）→ `SeatRetired`（席の
/// 登録 row の退役・account-lifecycle.md §24 形 4）→ 案件の一生の 5 kind（fleet-event-log.md §12）→ `MemoJudged`（memo の判定・
/// dispatcher.md §41）。variant を足して列に足し忘れた周・件数だけ合って末尾が違う周はここで赤になる。
#[test]
fn fleet_kinds_follow_declaration_order() {
    assert!(
        is_declaration_order(KINDS, |kind| kind as usize),
        "KINDS の並びが宣言順と乖離している（母集団 {} 種）",
        KINDS.len()
    );
    assert_eq!(KINDS.len(), 30, "母集団（列の印までの 16 + install 1 + 消費 1 + 裁定 1 + 群の逼迫の通知 1 + 群の移動 3 + 登録 row の退役 1 + 案件の一生 5 + memo の判定 1）");
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
                EventKind::UtteranceReceived,
                EventKind::UtteranceSorted,
                EventKind::TurnEndUnjudged,
                EventKind::IntakeRefused,
                EventKind::LifecycleCutover,
                EventKind::MemoJudged,
            ][..]
        ),
        "install → 消費 → 裁定 → 群の逼迫の通知 → 群の移動の承認・断り・保留 → 登録 row の退役 → 案件の一生の 5 kind → memo の判定が宣言順の末尾"
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
            case: None,
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
        case: None,
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
        case: None,
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

/// 表の列の値（空白区切り）。
fn cells_of(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
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

/// `account` を撃ち、`write-failed`（rc 2・stderr の 1 行・stdout 0 byte）で断られることを確かめる。
fn account_write_failed(args: &[&str], label: &str) {
    let out = run_account(args);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{args:?}: {out:?}");
    assert!(out.stdout.is_empty(), "{args:?}: stdout 0 byte");
    assert_eq!(text(&out.stderr), format!("account: refused reason=write-failed label={label}\n"), "{args:?}");
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

// 群の名を Tier と数字に改めただけの歯（account-lifecycle.md §29 の行 s・base でも緑）。
// flip-check: retroactive s2-07l.647
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

/// 区画の歯の置き場: 群 Tier1（置き場 `/repo/group`・候補 a1 → a2 → a3＝種 a1）と区画 Tier9（置き場 `/repo/lot`）を宣言し、区画の置き場の
/// 席の row = a2（p）・群の置き場の席の row = a3（g）を置く。
fn park_lot_fixture() -> (UsageFixture, PathBuf) {
    let (fx, curl) = group_fixture(GROUP_THREE);
    put_groups(&fx, &[("Tier1", &["/repo/group"], &["a1", "a2", "a3"]), ("Tier9", &["/repo/lot"], &["a1", "a2"])]);
    register_anchored_account(&fx.state, "/repo/lot", "a2");
    register_anchored_account(&fx.state, "/repo/group", "a3");
    (fx, curl)
}

/// `fleet select --purpose run` の 1 行（rc 0 を測る）。
fn park_lot_pick(fx: &UsageFixture, curl: &Path, extra: &[&str]) -> Vec<String> {
    let args: Vec<&str> = ["--purpose", "run"].iter().chain(extra).copied().collect();
    let out = run_select(fx, curl, &args);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{args:?}: {out:?}");
    out_lines(&out)
}

/// (a) `--anchor <区画の置き場>` は区画の席の row の口座 p（a2）を候補に残す: 群の種 a1 だけが外れ a2 が選ばれ、`--exclude a2` を
/// 重ねると a3（群の席の row は anchor が違うので数えない）。base は区画の席の row の a2 を外して a3。
#[test]
fn park_lot_select_run_with_the_lot_anchor_keeps_the_lot_seat_account() {
    let (fx, curl) = park_lot_fixture();
    assert_eq!(park_lot_pick(&fx, &curl, &["--anchor", "/repo/lot"]), ["select purpose=run chosen=a2"], "p は候補に残る");
    assert_eq!(park_lot_pick(&fx, &curl, &["--anchor", "/repo/lot", "--exclude", "a2"]), ["select purpose=run chosen=a3"]);
    drop_fixture(&fx);
}

/// (b) `--anchor` 無しは置き場の全 row を数える保守側のまま、区画の席の row だけを数えない: p（a2）は残って選ばれ、`--exclude a2` を
/// 重ねると群の席の row の g（a3）は外れたままで候補なし。base は a2 も外れ、最初から候補なし。
#[test]
fn park_lot_select_run_without_an_anchor_keeps_p_and_drops_g() {
    let (fx, curl) = park_lot_fixture();
    assert_eq!(park_lot_pick(&fx, &curl, &[]), ["select purpose=run chosen=a2"], "p は候補に残る");
    assert_eq!(park_lot_pick(&fx, &curl, &["--exclude", "a2"]), ["select purpose=run none=excluded earliest_reset=-"], "g は外れたまま");
    drop_fixture(&fx);
}

/// (c) `--anchor <群の置き場>` は群の席の row の口座 g（a3）を今までどおり外す（回帰の歯・base でも緑）: `--exclude a2` を重ねて候補なし。
#[test]
fn park_lot_select_run_with_the_group_anchor_still_drops_the_group_seat_account() {
    let (fx, curl) = park_lot_fixture();
    assert_eq!(park_lot_pick(&fx, &curl, &["--anchor", "/repo/group"]), ["select purpose=run chosen=a2"]);
    assert_eq!(
        park_lot_pick(&fx, &curl, &["--anchor", "/repo/group", "--exclude", "a2"]),
        ["select purpose=run none=excluded earliest_reset=-"],
        "g は外れる"
    );
    drop_fixture(&fx);
}

/// (a) 群 [a1, a2, a3] の記録 = a2 → 便用の候補から外れるのは **a2 だけ**（`fleet select --purpose run` の口）: 並びの先頭
/// a1 が選ばれ、`--exclude a1` を重ねると a3 が選ばれ（a1 と a3 は候補に残る）、両方を `--exclude` すると候補なし
/// （`excluded`＝a2 は候補に戻らない）。除外は置き場（anchor）で絞らない——群の置き場と関係の無い `--anchor` を付けても
/// 同じ答え（host 全体で外す）。base は群の候補の全部を外すので 1 周目が `none=excluded` → RED。
#[test]
fn host_group_run_selection_drops_only_the_current_account() {
    let (fx, curl) = group_fixture(GROUP_THREE);
    put_groups(&fx, &[("Tier1", &["/repo/a"], &["a1", "a2", "a3"])]);
    put_group_record(&fx, "Tier1", &record_body("a2"));
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
    put_groups(&fx, &[("Tier1", &["/repo/a"], &["a1", "a2", "a3"])]);
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
    put_groups(&fx, &[("Tier1", &["/repo/a"], &["a1", "a2", "a3"])]);
    let path = put_group_record(&fx, "Tier1", "account=a2\nts=2026-09-25T00:00:00Z\n");
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
        assert!(stderr.contains(&format!("group=Tier1 {word} ")), "{word}: 断りは群と型を名指す: {stderr}");
        assert_eq!(curl_calls(&fx), 0, "{word}: 測らない");
        assert_eq!(vessel::rules::grouped_accounts(&fx.state), Err(GroupedError::Record("Tier1".to_owned(), error)));
    }
    let out = run_select(&fx, &curl, &["--purpose", "session"]);
    assert_eq!(out_lines(&out), ["select purpose=session chosen=a2"], "session 用は群を読まない: {out:?}");
    drop_fixture(&fx);
}

/// (d) 除外の集合は群ごとの今の口座を畳んだもの（置き場から解く 1 本を直に読む）: 記録の無い 2 群（候補 [a1, a2] と
/// [a1, a3]）の種は宣言順に重ならない（Tier1 は a1・Tier2 は a1 でない最初の候補 a3・§28）ので除外は 2 つ、Tier2 の記録が a1
/// なら 2 群が同じ今の口座で除外は 1 つ。`fleet select` の口でも同じ: 2 つの周は a2 で `--exclude a2` を重ねると候補なし、
/// 1 つの周は `--exclude a2` で a3。base は種がどちらも先頭 a1（除外 1 つ）→ RED。
#[test]
fn host_group_run_exclusion_folds_the_current_accounts_of_the_groups() {
    let (fx, curl) = group_fixture(GROUP_THREE);
    put_groups(&fx, &[("Tier1", &["/repo/a"], &["a1", "a2"]), ("Tier2", &["/repo/b"], &["a1", "a3"])]);
    let set = |labels: &[&str]| -> BTreeSet<String> { labels.iter().map(|label| (*label).to_owned()).collect() };
    assert_eq!(vessel::rules::grouped_accounts(&fx.state), Ok(set(&["a1", "a3"])), "種は重ならない＝別の今の口座は 2 つ");
    let out = run_select(&fx, &curl, &["--purpose", "run"]);
    assert_eq!(out_lines(&out), ["select purpose=run chosen=a2"], "候補の残り a2 は便に開く: {out:?}");
    let out = run_select(&fx, &curl, &["--purpose", "run", "--exclude", "a2"]);
    assert_eq!(out_lines(&out), ["select purpose=run none=excluded earliest_reset=-"], "除外 2 つ: {out:?}");
    put_group_record(&fx, "Tier2", &record_body("a1"));
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
    put_groups(&fx, &[("Tier1", &["/repo/a"], &["a1"])]);
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
    put_groups(&fx, &[("Tier1", &["/repo/a"], &["a1"])]);
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
    put_groups(&fx, &[("Tier1", &["/repo/a"], &["a1"])]);
    let out = run_select(&fx, &curl, &["--purpose", "session"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(out_lines(&out), vec!["select purpose=session chosen=a1".to_owned()], "session 用は群を読まない");
    put_groups(&fx, &[("Tier1", &["/repo/a"], &["a3"])]);
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
    put_groups(&fx, &[("Tier1", &["/repo/a"], &["a1", "a2"])]);
    put_group_record(&fx, "Tier1", &record_body("a2"));
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

// ───── 局面の出力（設計 docs/design/case-lifecycle.md §12・接頭辞 `fleet_lifecycle_`） ─────

/// 偽の台帳 client（list は `ledger` の JSON を `list-rc` の rc で返し、close は argv を `close-log` へ足して `close-rc` の rc で返る）。
fn life_bd_body(dir: &Path) -> String {
    format!(
        "d='{}'\nif [ \"$1\" = close ]; then\n  echo \"$*\" >> \"$d/close-log\"\n  exit \"$(cat \"$d/close-rc\")\"\nfi\ncat \"$d/ledger\"\nexit \"$(cat \"$d/list-rc\")\"\n",
        dir.display()
    )
}

/// 開いた task（設計 pointer を持たない）。
const LIFE_TASK: &str = "{\"id\":\"toy-c1\",\"status\":\"open\",\"priority\":2,\"labels\":[],\"acceptance_criteria\":\"\",\"dependencies\":[]}";

/// 局面の出力の歯が 1 本ごとに持つ置き場（`pipe` の toy repo に台帳の files の形と origin/main の ref を足す）。
struct Life {
    repo: PathBuf,
    state: PathBuf,
    dir: PathBuf,
    bd: String,
    design: String,
}

impl Life {
    /// 台帳は `toy-c1`（開いた task）を `toy-c2`（契約の行を指す開いた契約）が blocks で待つ形（依存待ちの契約が出力に出る）。
    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn new() -> Self {
        let (repo, state) = super::pipe::repo_with_state();
        let design = super::pipe::write_contract(&repo, &[], &[]);
        let pointer = super::pipe::design_pointer();
        let waiting = format!(
            "{{\"id\":\"toy-c2\",\"status\":\"open\",\"priority\":2,\"labels\":[],\"acceptance_criteria\":\"design = {pointer}\",\"dependencies\":[{{\"issue_id\":\"toy-c2\",\"depends_on_id\":\"toy-c1\",\"type\":\"blocks\"}}]}}"
        );
        let dir = state.join("life");
        fs::create_dir_all(&dir).expect("置き場を作れる");
        let bd = state.join("life-bd.sh");
        fs::write(&bd, format!("#!/bin/sh\n{}", life_bd_body(&dir))).expect("偽 bd を書ける");
        fs::set_permissions(&bd, std::os::unix::fs::PermissionsExt::from_mode(0o755)).expect("実行権を付ける");
        let life = Self { repo, state, dir, bd: bd.display().to_string(), design };
        life.put("ledger", &format!("[{LIFE_TASK},{waiting}]\n"));
        life.put("close-rc", "0\n");
        life.put("list-rc", "0\n");
        fs::create_dir_all(life.repo.join(".beads")).expect(".beads を作れる");
        fs::write(life.repo.join(".beads/config.yaml"), "issue-prefix: toy\n").expect("config を書ける");
        fs::write(life.repo.join(".beads/issues.jsonl"), "[]\n").expect("台帳の file を書ける");
        let exclude = life.repo.join(".git/info/exclude");
        let body = fs::read_to_string(&exclude).unwrap_or_default();
        fs::write(&exclude, format!("{body}.beads/\n")).expect("exclude を書ける");
        let main = super::pipe::git(&life.repo, &["rev-parse", "refs/heads/main"]);
        super::pipe::git(&life.repo, &["update-ref", "refs/remotes/origin/main", &main]);
        life
    }

    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn put(&self, name: &str, text: &str) {
        fs::write(self.dir.join(name), text).expect("file を書ける");
    }

    fn json_path(&self) -> PathBuf {
        self.state.join("fleet").join("lifecycle.json")
    }

    /// 出力の `generated_at`（file が無いか読めない周は `None`）。
    fn generated(&self) -> Option<String> {
        let text = fs::read_to_string(self.json_path()).ok()?;
        parse(&text).ok()?.get("generated_at")?.as_str().map(str::to_owned)
    }

    /// `generated_at` を遠い過去へ戻す（次の書き直しが rename したかを `generated_at` の進みで測る）。
    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn retime(&self) {
        let now = self.generated().expect("出力が在る");
        let text = fs::read_to_string(self.json_path()).expect("出力を読める");
        fs::write(self.json_path(), text.replace(&now, LIFE_OLD)).expect("出力を書き戻せる");
    }

    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn fleet(&self, args: &[&str]) -> Output {
        Command::new(bin())
            .arg("fleet")
            .arg("lifecycle")
            .args(args)
            .args(["--state-dir", &self.state.display().to_string()])
            .output()
            .expect("binary を起動できる")
    }

    /// `fleet lifecycle write`（`extra` は `--wait-ms` などを足す）。
    fn write(&self, extra: &[&str]) -> Output {
        let mut args = vec!["write", "--repo", self.repo.to_str().unwrap_or_default(), "--bd", self.bd.as_str()];
        args.extend(extra);
        self.fleet(&args)
    }

    fn pipe_args<'a>(&'a self, head: &[&'a str], tail: &[&'a str]) -> Vec<&'a str> {
        let mut args = head.to_vec();
        args.extend(["--state-dir", self.state.to_str().unwrap_or_default(), "--repo", self.repo.to_str().unwrap_or_default(), "--bd", self.bd.as_str()]);
        args.extend(tail);
        args
    }

    /// `pipe dispatch`（1 周・契機 (a)・実装役の口は要る＝無い周は列を測らない・台帳の待ちだけの fixture なので何も起こさない）。
    fn dispatch(&self, tail: &[&str]) -> Output {
        let mut extra = vec!["--runner", "true"];
        extra.extend(tail);
        super::pipe::run_pipe(&self.pipe_args(&["dispatch"], &extra))
    }

    /// `pipe dispatch ls`（観測だけ・起こさず書かない）。
    fn ls(&self) -> Output {
        super::pipe::run_pipe(&self.pipe_args(&["dispatch", "ls"], &[]))
    }

    /// `fleet lifecycle show` の stdout。
    fn shown(&self) -> String {
        super::pipe::stdout_of(&self.fleet(&["show"]))
    }

    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn lock_by_a_live_process(&self) {
        fs::create_dir_all(self.state.join("fleet")).expect("dir を作れる");
        fs::write(self.state.join("fleet").join("lifecycle.lock"), format!("{}\n", std::process::id())).expect("lock を置ける");
    }

    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn unlock(&self) {
        fs::remove_file(self.state.join("fleet").join("lifecycle.lock")).expect("lock を外せる");
    }
}

/// `generated_at` を戻す先。
const LIFE_OLD: &str = "2020-01-01T00:00:00Z";

/// 出力の text に、依存を待つ開いた契約が contract-queued・理由 dependency で在る。
fn assert_queued(text: &str, who: &str) {
    let line = text.lines().find(|line| line.starts_with("part=contract id=toy-c2 "));
    assert!(
        line.is_some_and(|found| found.contains(" phase=contract-queued ") && found.ends_with(" reason=dependency")),
        "{who}: 依存を待つ契約は contract-queued・理由 dependency: {text}"
    );
}

/// write と show は同じ字で、頭の行と部品の行が §12 の形（key の順・値の無い欄は `-`）。
#[test]
fn fleet_lifecycle_write_and_show_print_the_same_text_in_the_documented_form() {
    let life = Life::new();
    let written = life.write(&[]);
    let (out, err) = (super::pipe::stdout_of(&written), super::pipe::stderr_of(&written));
    assert_eq!(written.status.code(), Some(i32::from(RC_OK)), "write は rc 0: {err}");
    assert_eq!(life.shown(), out, "write と show は 1 字も違わない");
    let mut lines = out.lines();
    let head: Vec<&str> = lines.next().unwrap_or_default().split_whitespace().collect();
    let keys: Vec<&str> = head.iter().map(|word| word.split('=').next().unwrap_or_default()).collect();
    assert_eq!(keys, ["lifecycle", "version", "generated", "scope", "ledger", "events", "main", "stale"], "頭の行の key の順: {head:?}");
    assert!(head.contains(&"version=1") && head.contains(&"scope=full") && head.contains(&"stale=-"), "{head:?}");
    let part = out.lines().find(|line| line.starts_with("part=contract id=toy-c2 ")).unwrap_or_default();
    let part_keys: Vec<&str> = part.split_whitespace().map(|word| word.split('=').next().unwrap_or_default()).collect();
    assert_eq!(part_keys, ["part", "id", "phase", "turn", "since", "reason"], "部品の行の key の順: {part}");
    assert!(part.contains(" since=-"), "値の無い欄は `-`: {part}");
    assert_queued(&out, "e");
    super::pipe::clean(&[&life.repo, &life.state]);
}

/// 出力の無い周と読めない周は rc 1 で stdout が空・stderr の 1 行。
#[test]
fn fleet_lifecycle_show_refuses_an_absent_and_an_unreadable_output() {
    let life = Life::new();
    let absent = life.fleet(&["show"]);
    assert_eq!(absent.status.code(), Some(i32::from(RC_REFUSED)), "無い周は rc 1");
    assert!(absent.stdout.is_empty(), "stdout は 0 byte");
    assert_eq!(super::pipe::stderr_of(&absent).trim_end(), "lifecycle=absent");
    fs::create_dir_all(life.state.join("fleet")).expect("dir を作れる");
    fs::write(life.json_path(), "{ not json").expect("壊れた出力を置ける");
    let broken = life.fleet(&["show"]);
    assert_eq!(broken.status.code(), Some(i32::from(RC_REFUSED)), "読めない周は rc 1");
    assert!(broken.stdout.is_empty(), "stdout は 0 byte");
    assert_eq!(super::pipe::stderr_of(&broken).trim_end(), "lifecycle=unreadable");
    super::pipe::clean(&[&life.repo, &life.state]);
}

/// 生きた pid が持つ lock は `--wait-ms` の後に busy（rc 1・stderr の 1 行・stdout は空）・撃った時の印より古くない file が在れば書き直さない
/// （台帳が変わっても出力は動かない）。
#[test]
fn fleet_lifecycle_write_is_busy_for_a_live_lock_and_coalesced_for_a_fresh_output() {
    let life = Life::new();
    life.lock_by_a_live_process();
    let busy = life.write(&["--wait-ms", "100"]);
    assert_eq!(busy.status.code(), Some(i32::from(RC_REFUSED)), "busy は rc 1");
    assert!(busy.stdout.is_empty(), "stdout は空");
    assert_eq!(super::pipe::stderr_of(&busy).trim_end(), "lifecycle=busy");
    assert_eq!(life.generated(), None, "書かない");
    life.unlock();
    assert_eq!(life.write(&[]).status.code(), Some(i32::from(RC_OK)));
    assert_eq!(life.write(&[]).status.code(), Some(i32::from(RC_OK)), "1 周目の線で進んだ印を写す 2 周目");
    life.retime();
    let before = life.shown();
    life.put("ledger", &format!("[{LIFE_TASK}]\n"));
    let again = life.write(&[]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "coalesced は rc 0");
    assert_eq!(super::pipe::stdout_of(&again), before, "印が古くない file は書き直さず今の組を出す");
    assert_eq!(life.generated().as_deref(), Some(LIFE_OLD), "rename しない");
    super::pipe::clean(&[&life.repo, &life.state]);
}

/// 使い方の行と `scribe2 help fleet` の頁（FORM と SUBCOMMANDS）が `lifecycle` を持つ。
#[test]
fn fleet_lifecycle_usage_and_help_page_name_the_verb() {
    let usage = run_fleet(&[]);
    assert!(String::from_utf8_lossy(&usage.stderr).contains("lifecycle <write|show>"), "使い方の行");
    let help = Command::new(bin()).args(["help", "fleet"]).output().expect("binary を起動できる");
    let page = String::from_utf8_lossy(&help.stdout).into_owned();
    assert!(page.lines().any(|line| line.starts_with("usage: fleet ") && line.contains("lifecycle <write|show>")), "FORM: {page}");
    assert!(page.lines().any(|line| line.starts_with("  lifecycle ")), "SUBCOMMANDS: {page}");
}

/// 契機 (a) の `pipe dispatch` と (e) の `fleet lifecycle write` で出力が進み、`dispatch ls` では進まない。どの契機の出力にも
/// 依存を待つ開いた契約が contract-queued・理由 dependency で出る。
#[test]
fn fleet_lifecycle_triggers_a_and_e_advance_generated_and_ls_does_not() {
    let life = Life::new();
    let round = life.dispatch(&[]);
    assert_eq!(round.status.code(), Some(i32::from(RC_OK)), "dispatch は rc 0: {}", super::pipe::stderr_of(&round));
    assert!(life.generated().is_some(), "(a) の周が出力を書く: {}", super::pipe::stderr_of(&round));
    assert_queued(&life.shown(), "a");
    fs::remove_file(life.json_path()).expect("出力を消せる");
    assert_eq!(life.write(&[]).status.code(), Some(i32::from(RC_OK)));
    assert_queued(&life.shown(), "e");
    life.retime();
    let ls = life.ls();
    assert_eq!(ls.status.code(), Some(i32::from(RC_OK)), "ls は rc 0: {}", super::pipe::stderr_of(&ls));
    assert_eq!(life.generated().as_deref(), Some(LIFE_OLD), "ls では進まない");
    let again = life.dispatch(&[]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "dispatch は rc 0: {}", super::pipe::stderr_of(&again));
    assert_ne!(life.generated().as_deref(), Some(LIFE_OLD), "(a) で進む");
    super::pipe::clean(&[&life.repo, &life.state]);
}

impl Life {
    /// Gated PASS の便を 1 本つくる（宣言に remote は無い＝終端は push も CI も撃たず台帳の close だけ）。
    fn gated(&self) -> String {
        super::pipe::gated_pass(&self.repo, &self.state, &self.design, &self.state.join("lens-ran"))
    }

    /// `pipe land --run ID`（`extra` は `--terminal-only` や `--rules`）。
    fn land(&self, id: &str, extra: &[&str]) -> Output {
        let mut tail = vec!["--run", id];
        tail.extend(extra);
        super::pipe::run_pipe(&self.pipe_args(&["land"], &tail))
    }

    /// PR の形で着地した便を、偽 remote・偽 gh・偽 CI つきで `pipe retire` が通る形にする（返すのは PATH の値）。
    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn prepared_pr(&self, id: &str) -> String {
        let (repo, state) = (&self.repo, &self.state);
        let landed = super::pipe::run_pipe(&[
            "land", "--run", id, "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(), "--pr-cmd", "true",
        ]);
        assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "PR 形の land は rc 0: {}", super::pipe::stderr_of(&landed));
        let remote = state.join("remote.git");
        super::pipe::git(state, &["init", "--bare", "-q", &remote.display().to_string()]);
        super::pipe::git(repo, &["remote", "add", "fake", &remote.display().to_string()]);
        let ci = state.join("life-ci.sh");
        fs::write(&ci, "#!/bin/sh\nprintf '[{\"status\":\"completed\",\"conclusion\":\"success\"}]\\n'\n").expect("偽 CI を書ける");
        let declaration = fs::read_to_string(repo.join(".vessel.toml")).expect("宣言を読める");
        fs::write(repo.join(".vessel.toml"), format!("{declaration}remote = \"fake\"\nci-cmd = \"{} {{sha}}\"\n", ci.display())).expect("宣言を書ける");
        super::pipe::git(repo, &["add", "-f", ".vessel.toml"]);
        super::pipe::git(repo, &["commit", "-q", "-m", "terminal-decl"]);
        let (main, branch) = (super::pipe::git(repo, &["rev-parse", "refs/heads/main"]), format!("scribe2/{id}"));
        let tree = super::pipe::git(repo, &["rev-parse", &format!("{branch}^{{tree}}")]);
        let merge = super::pipe::git(repo, &["commit-tree", &tree, "-p", &main, "-p", &branch, "-m", "merge"]);
        super::pipe::git(repo, &["push", "-q", "-f", &remote.display().to_string(), &format!("{merge}:refs/heads/main")]);
        let bin = state.join("life-bin");
        fs::create_dir_all(&bin).expect("道具の dir を作れる");
        let gh = bin.join("gh");
        fs::write(&gh, format!("#!/bin/sh\nprintf '{{\"state\":\"MERGED\",\"mergeCommit\":{{\"oid\":\"{merge}\"}}}}\\n'\n")).expect("偽 gh を書ける");
        fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).expect("実行権を付ける");
        fs::set_permissions(&ci, std::os::unix::fs::PermissionsExt::from_mode(0o755)).expect("実行権を付ける");
        format!("{}:{}", bin.display(), crate::toolbox_path(state))
    }

    /// `pipe retire --run ID`（偽 gh を PATH の先頭に）。
    fn retire(&self, id: &str, path: &str, extra: &[&str]) -> Output {
        let mut tail = vec!["--run", id];
        tail.extend(extra);
        super::pipe::run_pipe_with_path(path, &self.pipe_args(&["retire"], &tail))
    }

    /// 台帳 client の close を呼んだ回数。
    fn closes(&self) -> usize {
        fs::read_to_string(self.dir.join("close-log")).map(|text| text.lines().count()).unwrap_or(0)
    }
}

/// (d) 着地の本体の close と `--terminal-only` の終端の close と `pipe retire` の close で出力が書かれ、close が落ちた周は書かれない
/// （同じ歯の中の肯定と組）。呼び手の rc と stdout は変わらず stderr に `lifecycle=` も出ない。
#[test]
fn fleet_lifecycle_close_paths_advance_generated_and_a_failed_close_does_not() {
    let failing = Life::new();
    let id = failing.gated();
    failing.put("close-rc", "3\n");
    let failed = failing.land(&id, &[]);
    assert_eq!(failed.status.code(), Some(i32::from(RC_REFUSED)), "close が落ちた終端は rc 1: {}", super::pipe::stderr_of(&failed));
    assert_eq!(failing.generated(), None, "落ちた close の周は出力を書かない");
    failing.put("close-rc", "0\n");
    let replay = failing.land(&id, &["--terminal-only"]);
    assert_eq!(replay.status.code(), Some(i32::from(RC_OK)), "撃ち直しは rc 0: {}", super::pipe::stderr_of(&replay));
    assert_eq!(super::pipe::stdout_of(&replay).trim_end(), format!("run={id} terminal=closed:no-ci"), "終端の 1 行");
    assert!(failing.generated().is_some(), "terminal-only の close の後に書く");
    assert!(!super::pipe::stderr_of(&replay).contains("lifecycle="), "Written の周は stderr に出さない");
    super::pipe::clean(&[&failing.repo, &failing.state]);

    let body = Life::new();
    let id = body.gated();
    let landed = body.land(&id, &[]);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "着地の本体の終端は rc 0: {}", super::pipe::stderr_of(&landed));
    assert!(body.generated().is_some(), "着地の本体の close の後に書く: {}", super::pipe::stderr_of(&landed));
    assert_eq!(body.closes(), 1, "close は 1 回");
    assert_queued(&body.shown(), "d");
    super::pipe::clean(&[&body.repo, &body.state]);

    let retired = Life::new();
    let id = retired.gated();
    let path = retired.prepared_pr(&id);
    retired.put("close-rc", "3\n");
    let declined = retired.retire(&id, &path, &[]);
    assert_eq!(declined.status.code(), Some(i32::from(RC_REFUSED)), "close が落ちた retire は rc 1: {}", super::pipe::stderr_of(&declined));
    assert_eq!(retired.generated(), None, "落ちた close の周は出力を書かない");
    retired.put("close-rc", "0\n");
    let out = retired.retire(&id, &path, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "retire は rc 0: {} / {}", super::pipe::stdout_of(&out), super::pipe::stderr_of(&out));
    assert!(retired.generated().is_some(), "retire の close の後に書く");
    assert!(!super::pipe::stderr_of(&out).contains("lifecycle="), "Written の周は stderr に出さない");
    super::pipe::clean(&[&retired.repo, &retired.state]);
}

/// lock の待ちを短くした rules の写し（埋め込みの manifest の `fleet.lock_retry_ms` だけを 100 ms にする）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn life_fast_rules(life: &Life) -> String {
    let body = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../rules/manifest.toml")).expect("manifest を読める");
    let from = "id = \"fleet.lock_retry_ms\"\nkind = \"LockRetryMs\"\nvalue = 5000";
    assert!(body.contains(from), "行の字面が在る");
    let path = life.state.join("life-rules.toml");
    fs::write(&path, body.replace(from, "id = \"fleet.lock_retry_ms\"\nkind = \"LockRetryMs\"\nvalue = 100")).expect("写しを書ける");
    path.display().to_string()
}

/// 呼び手の出力の字（repo の path と便の id を伏せる）。
fn life_told(out: &Output, life: &Life, id: &str) -> (Option<i32>, String) {
    let text = super::pipe::stdout_of(out).replace(&life.repo.display().to_string(), "<repo>").replace(id, "<id>");
    (out.status.code(), text)
}

/// 契機 (a) の `pipe dispatch` と (d) の `--terminal-only` と `pipe retire` の周で、Written の周と lock を生きた pid で持たせた周の
/// 呼び手の rc と stdout の字が等しく `lifecycle=` を持たず、stderr の `lifecycle=` の行は busy の周の `lifecycle=busy` の 1 行だけ。
#[test]
fn fleet_lifecycle_caller_rc_and_stdout_are_the_same_for_a_written_and_a_busy_round() {
    let lines_of = |out: &Output| -> Vec<String> {
        super::pipe::stderr_of(out).lines().filter(|line| line.contains("lifecycle=")).map(str::to_owned).collect()
    };
    let written = Life::new();
    let rules = life_fast_rules(&written);
    let first = written.dispatch(&["--rules", &rules]);
    assert!(lines_of(&first).is_empty(), "Written の周は stderr に出さない: {}", super::pipe::stderr_of(&first));
    assert!(!super::pipe::stdout_of(&first).contains("lifecycle="), "stdout に出さない");
    assert!(written.generated().is_some());
    let busy = Life::new();
    busy.lock_by_a_live_process();
    let second = busy.dispatch(&["--rules", &life_fast_rules(&busy)]);
    assert_eq!(life_told(&second, &busy, ""), life_told(&first, &written, ""), "(a) の rc と stdout は等しい");
    assert_eq!(lines_of(&second), ["lifecycle=busy"], "busy の周の stderr の 1 行");
    assert_eq!(busy.generated(), None, "busy の周は書かない");
    super::pipe::clean(&[&written.repo, &written.state, &busy.repo, &busy.state]);

    assert_eq!(life_close_round(false, false), life_close_round(true, false), "(d) terminal-only の rc と stdout は等しい");
    assert_eq!(life_close_round(false, true), life_close_round(true, true), "(d) retire の rc と stdout は等しい");
}

/// (d) の 1 周（`retire` が偽なら 1 周目の close を落としてから `--terminal-only`・真なら PR の便の retire）を撃ち、呼び手の字を返す。
/// `locked` の周は lock を生きた pid で持たせる（stderr の `lifecycle=` の行は busy の周の 1 行だけ）。
fn life_close_round(locked: bool, retire: bool) -> (Option<i32>, String) {
    let life = Life::new();
    let id = life.gated();
    let path = if retire {
        life.prepared_pr(&id)
    } else {
        life.put("close-rc", "3\n");
        assert_eq!(life.land(&id, &[]).status.code(), Some(i32::from(RC_REFUSED)), "前提: 1 周目の close は落ちる");
        life.put("close-rc", "0\n");
        String::new()
    };
    if locked {
        life.lock_by_a_live_process();
    }
    let rules = life_fast_rules(&life);
    let out = if retire { life.retire(&id, &path, &["--rules", &rules]) } else { life.land(&id, &["--terminal-only", "--rules", &rules]) };
    let err = super::pipe::stderr_of(&out);
    let lines: Vec<&str> = err.lines().filter(|line| line.contains("lifecycle=")).collect();
    assert_eq!(lines, if locked { vec!["lifecycle=busy"] } else { Vec::new() }, "stderr の lifecycle= の行: {err}");
    let told = life_told(&out, &life, &id);
    super::pipe::clean(&[&life.repo, &life.state]);
    told
}

/// 台帳を読めない周は書かず、理由 `ledger` の印が残る（出力は前のまま・stderr の 1 行は `lifecycle=unreadable`）。
#[test]
fn fleet_lifecycle_unreadable_ledger_writes_nothing_and_marks_the_reason() {
    let life = Life::new();
    assert_eq!(life.write(&[]).status.code(), Some(i32::from(RC_OK)));
    assert_eq!(life.write(&[]).status.code(), Some(i32::from(RC_OK)), "1 周目の線で進んだ印を写す");
    life.retime();
    life.put("list-rc", "1\n");
    fs::write(life.repo.join(".beads/issues.jsonl"), "[]\n\n").expect("台帳の file を進められる");
    let out = life.write(&[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "読めない周は rc 1");
    assert_eq!(super::pipe::stderr_of(&out).trim_end(), "lifecycle=unreadable");
    assert_eq!(life.generated().as_deref(), Some(LIFE_OLD), "出力は動かない");
    let stale = fs::read_to_string(life.state.join("fleet").join("lifecycle.stale")).unwrap_or_default();
    assert!(stale.contains("unreadable") && stale.contains("\"ledger\""), "理由 ledger の印: {stale}");
    assert!(life.shown().contains("stale=unreadable"), "頭の行の stale に出る");
    super::pipe::clean(&[&life.repo, &life.state]);
}

// ───── 局面の導出へ渡す 2 つの列（設計 docs/design/case-lifecycle.md §17・接頭辞 `fleet_lifecycle_feeds_`） ─────

/// 部品 1 つの (局面, 手番, 理由)。
type Shape = (String, String, Option<String>);

/// 閉じた問い（metadata の effect と裁定の行の id・閉じた時刻を持つ）。
fn feeds_question(id: &str, effect: &str, ruling: &str, closed: &str) -> String {
    format!(
        "{{\"id\":\"{id}\",\"status\":\"closed\",\"priority\":2,\"labels\":[\"intake:question\"],\"acceptance_criteria\":\"\",\"dependencies\":[],\"notes\":\"{ruling} | {id} | 2026-01-01T00:00Z | chat | 逐語\",\"created_at\":\"2025-12-31T00:00:00Z\",\"closed_at\":\"{closed}\",\"metadata\":{{\"effect\":\"{effect}\"}}}}"
    )
}

/// 満ちない期日の引き金の行を持つ開いた memo（`updated` は台帳の updated_at）。
fn feeds_memo(id: &str, updated: &str) -> String {
    format!(
        "{{\"id\":\"{id}\",\"status\":\"open\",\"priority\":2,\"labels\":[\"intake:memo\"],\"acceptance_criteria\":\"\",\"dependencies\":[],\"description\":\"### 出所\\n### 観測\\n### 候補\\n### 昇格条件\\n- 引き金: 期日 2099-01-01T00:00Z\\n\",\"created_at\":\"2026-09-01T00:00:00Z\",\"updated_at\":\"{updated}\"}}"
    )
}

/// event log の MemoJudged の 1 行（bead は memo の id・detail は判定の語）。
fn feeds_judged(memo: &str, word: &str) -> String {
    format!("{{\"schema\":1,\"ts\":\"2026-10-01T00:00:00Z\",\"kind\":\"MemoJudged\",\"bead\":\"{memo}\",\"detail\":\"{word}\",\"host\":\"h\",\"actor\":\"machine\"}}")
}

impl Life {
    /// 出力の木（無いか読めない周は `None`）。
    fn tree(&self) -> Option<Tree> {
        parse(&fs::read_to_string(self.json_path()).ok()?).ok()
    }

    /// 出力の部品 1 つの (局面, 手番, 理由)（部品が無ければ `None`）。
    fn shape(&self, id: &str) -> Option<Shape> {
        let tree = self.tree()?;
        let part = tree.get("parts")?.as_array()?.iter().find(|part| part.get("id").and_then(Tree::as_str) == Some(id))?;
        let word = |key: &str| part.get(key).and_then(Tree::as_str).map(str::to_owned);
        Some((word("phase")?, word("turn")?, word("reason")))
    }

    /// 出力の `unmeasured` の (部品の種類, 理由)。
    fn named(&self) -> Vec<(String, String)> {
        let tree = self.tree();
        let items = tree.as_ref().and_then(|found| found.get("unmeasured")).and_then(Tree::as_array).unwrap_or_default();
        let word = |item: &Tree, key: &str| item.get(key).and_then(Tree::as_str).unwrap_or_default().to_owned();
        items.iter().map(|item| (word(item, "part"), word(item, "reason"))).collect()
    }
}

/// 2 つの列を渡す: 置き場が無い周（契機 (e)）は問いの部品が無く promote の memo が verdict、`pipe dispatch`（契機 (a)）の後は窓より古い
/// document の問いが ruling-unreflected・operation の問いは question-closed・keep と処置の後の memo は memo-waiting、出力を消した後の (e) は
/// 前の周の置き場から同じ局面を出す。
#[test]
fn fleet_lifecycle_feeds_the_unreflected_question_and_the_unhandled_verdict() {
    let life = Life::new();
    let (old, recent) = ("2026-01-01T00:00:00Z", format_utc(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs()).saturating_sub(3_600)));
    let waiting = fs::read_to_string(life.dir.join("ledger")).expect("台帳を読める");
    let base = waiting.trim_end().trim_end_matches(']');
    let added = [
        feeds_question("toy-qd", "document", "batch:doc1", old),
        feeds_question("toy-qo", "operation", "batch:op1", &recent),
        feeds_memo("toy-m1", "2026-09-30T00:00:00Z"),
        feeds_memo("toy-m2", "2026-09-30T00:00:00Z"),
        feeds_memo("toy-m3", "2099-01-01T00:00:00Z"),
    ];
    life.put("ledger", &format!("{base},{}]\n", added.join(",")));
    write_raw(&life.state, &[&feeds_judged("toy-m1", "promote"), &feeds_judged("toy-m2", "keep"), &feeds_judged("toy-m3", "promote")]);
    let seat = |phase: &str, reason: Option<&str>| Some((phase.to_owned(), "seat".to_owned(), reason.map(str::to_owned)));
    let nobody = |phase: &str, reason: Option<&str>| Some((phase.to_owned(), "none".to_owned(), reason.map(str::to_owned)));

    let first = life.write(&[]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "契機 (e) は rc 0: {}", super::pipe::stderr_of(&first));
    assert_eq!(life.shape("toy-qd"), None, "置き場が無いので窓より古い問いの部品は無い");
    assert_eq!(life.shape("toy-m1"), seat("memo-actionable", Some("verdict")), "最後の判定が promote の memo");
    assert_eq!(life.named(), Vec::<(String, String)>::new(), "置き場の無い周は名指さない");

    let round = life.dispatch(&[]);
    assert_eq!(round.status.code(), Some(i32::from(RC_OK)), "dispatch は rc 0: {}", super::pipe::stderr_of(&round));
    assert!(life.state.join("pipe").join("unreflected").exists(), "起こす側の周が置き場を書く");
    let after = [
        ("toy-qd", seat("ruling-unreflected", None)),
        ("toy-qo", nobody("question-closed", None)),
        ("toy-m1", seat("memo-actionable", Some("verdict"))),
        ("toy-m2", nobody("memo-waiting", None)),
        ("toy-m3", nobody("memo-waiting", None)),
    ];
    for (id, want) in &after {
        assert_eq!(&life.shape(id), want, "(a) の周の {id}");
    }
    fs::remove_file(life.json_path()).expect("出力を消せる");
    assert_eq!(life.write(&[]).status.code(), Some(i32::from(RC_OK)));
    for (id, want) in &after {
        assert_eq!(&life.shape(id), want, "出力を消した後の (e) の {id}");
    }
    super::pipe::clean(&[&life.repo, &life.state]);
}

/// 置き場が読めない周は rc 0 で書き、`unmeasured` に question と unreflected-unreadable を持ち問いの部品は無い。次の `pipe dispatch` の後は
/// 名指しが消えて ruling-unreflected に戻る（同じ歯の中の対）。
#[test]
fn fleet_lifecycle_feeds_name_an_unreadable_store_and_recover_after_the_next_dispatch() {
    let life = Life::new();
    let waiting = fs::read_to_string(life.dir.join("ledger")).expect("台帳を読める");
    let base = waiting.trim_end().trim_end_matches(']');
    life.put("ledger", &format!("{base},{}]\n", feeds_question("toy-qd", "document", "batch:doc1", "2026-01-01T00:00:00Z")));
    let round = life.dispatch(&[]);
    assert_eq!(round.status.code(), Some(i32::from(RC_OK)), "dispatch は rc 0: {}", super::pipe::stderr_of(&round));
    let unreflected = ("ruling-unreflected".to_owned(), "seat".to_owned(), None);
    assert_eq!(life.shape("toy-qd"), Some(unreflected.clone()), "読める置き場の周");
    assert_eq!(life.named(), Vec::<(String, String)>::new(), "読める置き場は名指さない");

    fs::write(life.state.join("pipe").join("unreflected"), "{ not json").expect("置き場を壊せる");
    fs::remove_file(life.json_path()).expect("出力を消せる");
    let broken = life.write(&[]);
    assert_eq!(broken.status.code(), Some(i32::from(RC_OK)), "読めない置き場でも rc 0: {}", super::pipe::stderr_of(&broken));
    assert_eq!(life.named(), [("question".to_owned(), "unreflected-unreadable".to_owned())], "読めない置き場を名指す");
    assert_eq!(life.shape("toy-qd"), None, "問いの部品は無い");

    let again = life.dispatch(&[]);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "dispatch は rc 0: {}", super::pipe::stderr_of(&again));
    assert_eq!(life.named(), Vec::<(String, String)>::new(), "次の dispatch の後は名指しが消える");
    assert_eq!(life.shape("toy-qd"), Some(unreflected), "ruling-unreflected に戻る");
    super::pipe::clean(&[&life.repo, &life.state]);
}

/// 置き場の anchor が 2 つの周（設定の無い repo の登録 row を足した置き場）は rc 0 で部品を書き、`unmeasured` に multi-anchor を 8 つの種類の順で
/// 1 件ずつ名指す（発話は名指さない）。その repo の設定に別の在る dir を書き台帳を伸ばした後の書き直しと、`Life` の repo だけの登録の置き場は
/// 名指さない（同じ歯の中の対）。
#[test]
fn anchor_census_names_the_many_anchor_place_in_the_unmeasured_list() {
    let life = Life::new();
    let (other, away) = (super::pipe::tmp(), super::pipe::tmp());
    super::pipe::git(&other, &["init", "-q", "-b", "main"]);
    register_anchored_account(&life.state, &other.display().to_string(), "a1");
    let first = life.write(&[]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "{}", super::pipe::stderr_of(&first));
    assert_queued(&life.shown(), "many");
    let parts = ["question", "memo", "contract", "run", "row", "requirement", "epic", "commit"];
    let named: Vec<(String, String)> = parts.iter().map(|part| ((*part).to_owned(), "multi-anchor".to_owned())).collect();
    assert_eq!(life.named(), named, "8 つの種類の順に 1 件ずつ（utterance は無い）");

    super::pipe::git(&other, &["config", &format!("{}.stateDir", vessel::name::NAME), &away.display().to_string()]);
    life.retime();
    fs::write(life.repo.join(".beads/issues.jsonl"), "[]\n\n").expect("台帳の file を伸ばせる");
    let second = life.write(&[]);
    assert_eq!(second.status.code(), Some(i32::from(RC_OK)), "{}", super::pipe::stderr_of(&second));
    assert_ne!(life.generated().as_deref(), Some(LIFE_OLD), "書き直しが走った（generated_at が進んだ）");
    assert_eq!(life.named(), Vec::<(String, String)>::new(), "別の置き場を名乗る anchor は数えない");

    let single = Life::new();
    register_anchored_account(&single.state, &single.repo.display().to_string(), "a1");
    assert_eq!(single.write(&[]).status.code(), Some(i32::from(RC_OK)));
    assert_eq!(single.named(), Vec::<(String, String)>::new(), "Life の repo だけの登録");
    super::pipe::clean(&[&life.repo, &life.state, &single.repo, &single.state, &other, &away]);
}
