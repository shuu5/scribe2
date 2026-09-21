//! 運転手の終端と「起こす便 0 ∧ 候補あり」を登録 row の席の pane へ 1 行で知らせる歯（設計 docs/design/dispatcher.md §19・
//! 契約表の行 p・接頭辞 `pipe_notify_`）。
//!
//! 偽の `tmux`（呼ばれた引数を 1 行ずつ記録 file へ足し、`send-keys -l` の payload を偽の pane へ写す script）を道具箱の
//! dir（PATH の先頭）に置き、登録 row は core の `register` で積む（実 tmux を立てない）。歯の file は `pipe/` の外に
//! 置く（§19 形 6・`pipe/` 配下の file 数の pin は動かさない）。

use crate::pipe::{bin_cmd, ceiling_rules, clean, embedded_int, repo_with_state, run_pipe, stderr_of, stdout_of, DESIGN_FILE};
use crate::TOOLBOX_BIN;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use vessel::cli_outcome::RC_OK;
use vessel::fleet::Registration;
use vessel::seat::role::Role;

/// 偽の pane の名（登録 row の target）。
const TARGET: &str = "notify-seat:0";

/// 終端にする便の id と bead（`fleet record` で置く・契約を持たない＝列の候補と交差しない）。
const RUN: &str = "r-notify";

/// 同上の bead id。
const BEAD: &str = "s2-notify.1";

/// 列の候補にする bead（台帳の 1 件・行 a を指す）。
const QUEUED: &str = "s2-toy.1";

/// 偽の `tmux` の記録 file（置き場の直下・呼ばれた引数を 1 起動 1 行）。
fn calls_of(state: &Path) -> PathBuf {
    state.join("tmux-calls")
}

/// 偽の `tmux` を道具箱の dir に置く。`capture-pane` は偽の pane の本文の後に空の prompt 行を返し、`send-keys -l` は
/// payload を偽の pane へ足す（送達の目印が現れる）。どの起動も引数を記録 file へ 1 行で足す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_tmux(state: &Path) {
    let bin = state.join(TOOLBOX_BIN);
    fs::create_dir_all(&bin).expect("道具箱の dir を作れる");
    let pane = state.join("tmux-pane");
    fs::write(&pane, "").expect("偽の pane を作れる");
    let script = format!(
        "#!/bin/sh\n\
         printf '%s\\n' \"$*\" >> '{calls}'\n\
         case \"$1\" in\n\
         capture-pane) cat '{pane}'; printf '\\342\\235\\257 \\n';;\n\
         send-keys) if [ \"$4\" = \"-l\" ]; then printf '%s\\n' \"$5\" >> '{pane}'; fi;;\n\
         esac\n\
         exit 0\n",
        calls = calls_of(state).display(),
        pane = pane.display()
    );
    let path = bin.join("tmux");
    fs::write(&path, script).expect("偽の tmux を書ける");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("偽の tmux に実行権を付ける");
}

/// 偽の `tmux` が受けた payload の送り（`send-keys -t <target> -l <payload>` の行）の列。
fn sends(state: &Path) -> Vec<String> {
    fs::read_to_string(calls_of(state))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("send-keys ") && line.contains(" -l "))
        .map(str::to_owned)
        .collect()
}

/// orchestrator の登録 row を 1 件積む（anchor = toy repo・target = 偽の pane）。
fn register(state: &Path, repo: &Path) {
    let row = Registration {
        role: Role::Orchestrator,
        anchor: repo.display().to_string(),
        target: TARGET.to_owned(),
        sid: None,
        account: "acct-1".to_owned(),
        launch: String::new(),
        model: None,
    };
    assert!(vessel::seat::role::register(state, row).is_ok(), "登録 row を積める");
}

/// 走行中の便（`Implemented`・席も札も無い）を `fleet record` で置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn live_run(state: &Path) {
    let out = bin_cmd()
        .args(["fleet", "record", "--kind", "RunStage", "--run", RUN, "--bead", BEAD, "--stage", "Implemented"])
        .args(["--detail", "implemented"])
        .arg("--state-dir")
        .arg(state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fleet record: {}", stderr_of(&out));
}

/// 落ちた周に写す 1 行（rc と stdout と stderr）。
fn told(out: &Output) -> String {
    format!(
        "rc={:?} out={} err={}",
        out.status.code(),
        stdout_of(out).replace('\n', " / ").trim_end(),
        stderr_of(out).replace('\n', " / ").trim_end()
    )
}

/// `pipe stop --run` を `--repo` 付きで撃つ（道具を渡さない周＝列は `no-runner` で測らない）。
fn stop(repo: &Path, state: &Path) -> Output {
    run_pipe(&["stop", "--run", RUN, "--state-dir", &state.display().to_string(), "--repo", &repo.display().to_string()])
}

/// (約束 1) 登録 row の在る置き場で live な便に `pipe stop --run` を撃つと、偽の pane へ bead と run と `Stopped` を含む
/// 1 行を 1 回だけ送り、stdout に `notify=delivered` を残す（rc は stop のまま 0）。
#[test]
fn pipe_notify_terminal_failure_reaches_the_registered_seat_pane() {
    let (repo, state) = repo_with_state();
    fake_tmux(&state);
    register(&state, &repo);
    live_run(&state);
    let out = stop(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", told(&out));
    let sent = sends(&state);
    assert_eq!(sent.len(), 1, "payload の送りは 1 回だけ: {sent:?}");
    let payload = sent.first().cloned().unwrap_or_default();
    assert!(payload.contains(&format!("-t {TARGET} -l ")), "宛先は登録 row の target: {payload}");
    for word in [BEAD, RUN, "Stopped"] {
        assert!(payload.contains(word), "payload に {word}: {payload}");
    }
    assert!(!payload.contains('\n'), "payload は 1 行");
    assert!(stdout_of(&out).lines().any(|line| line == "notify=delivered"), "stdout に notify=delivered: {}", told(&out));
    clean(&[&repo, &state]);
}

/// (約束 1 の負の枝) 登録 row の無い置き場では 1 key も送らず、stdout に `notify=no-seat` を残す。
#[test]
fn pipe_notify_without_a_registered_seat_reports_no_seat() {
    let (repo, state) = repo_with_state();
    fake_tmux(&state);
    live_run(&state);
    let out = stop(&repo, &state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", told(&out));
    assert_eq!(sends(&state), Vec::<String>::new(), "send-keys は 0 回");
    assert!(stdout_of(&out).lines().any(|line| line == "notify=no-seat"), "stdout に notify=no-seat: {}", told(&out));
    assert!(!stdout_of(&out).contains("notify=delivered"), "送っていない周は delivered を名乗らない: {}", told(&out));
    clean(&[&repo, &state]);
}

/// 台帳の JSON（`issues` の各件は行 a を指す open の bead）を返す偽の `bd` を置き、その path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fake_bd(state: &Path, name: &str, issues: &[&str]) -> String {
    let listed: Vec<String> = issues
        .iter()
        .map(|id| {
            format!(
                "{{\"id\":\"{id}\",\"status\":\"open\",\"priority\":2,\"labels\":[],\
                 \"acceptance_criteria\":\"design = {DESIGN_FILE}#a\",\"dependencies\":[]}}"
            )
        })
        .collect();
    let json = state.join(format!("{name}.json"));
    fs::write(&json, format!("[{}]\n", listed.join(","))).expect("偽の台帳を書ける");
    let path = state.join(name);
    fs::write(&path, format!("#!/bin/sh\ncat '{}'\n", json.display())).expect("偽の bd を書ける");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("偽の bd に実行権を付ける");
    path.display().to_string()
}

/// 列が読む manifest（受付の上限の写しに台帳の待ち上限と停止の猶予の行を足す）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn queue_rules(state: &Path) -> String {
    let base = fs::read_to_string(ceiling_rules(state)).expect("受付の写しを読める");
    let row = |id: &str, kind: &str, value: u64| {
        format!("[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n")
    };
    let rows = format!(
        "{}\n{}",
        row("seat.ledger_timeout_s", "LedgerTimeoutS", 60),
        row("pipe.stop_grace_ms", "StopGraceMs", embedded_int("pipe.stop_grace_ms")),
    );
    let path = state.join("rules-notify.toml");
    fs::write(&path, format!("{base}\n{rows}")).expect("列の写しを書ける");
    path.display().to_string()
}

/// 登録 row と偽の tmux と live な便を置いた置き場で、`bd` を台帳にして `pipe stop --run` の終端を撃つ（道具つき＝列が測る）。
fn idle_round(issues: &[&str], hold: bool) -> (Output, Vec<String>) {
    // toy repo の設計 doc は行 a を 1 本持つ（[`repo_with_state`] の seed）＝台帳の bead はその行を指す。
    let (repo, state) = repo_with_state();
    fake_tmux(&state);
    register(&state, &repo);
    live_run(&state);
    let bd = fake_bd(&state, "bd-notify", issues);
    if hold {
        let held = run_pipe(&["dispatch", "hold", QUEUED, "--state-dir", &state.display().to_string()]);
        assert_eq!(held.status.code(), Some(i32::from(RC_OK)), "hold: {}", told(&held));
    }
    let out = run_pipe(&[
        "stop", "--run", RUN,
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &queue_rules(&state),
        "--bd", &bd,
        "--runner", "true",
    ]);
    let sent = sends(&state);
    clean(&[&repo, &state]);
    (out, sent)
}

/// (約束 2) 列の結果が「起こした便 0 ∧ 候補 1」（ready の bead を hold にした台帳）の終端の周は、終端の 1 行に続けて
/// idle の 1 行（`ready=1 launched=0 reason=hold`）を同じ宛先へ送る。候補 0 の台帳の周は idle の行を送らない
/// （send-keys は終端の 1 行だけ）。
#[test]
fn pipe_notify_idle_round_reports_ready_count_and_top_reason() {
    let (out, sent) = idle_round(&[QUEUED], true);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", told(&out));
    let idle: Vec<&String> = sent.iter().filter(|line| line.contains(" idle ")).collect();
    assert_eq!(idle.len(), 1, "idle の行は 1 回: {sent:?}");
    let line = idle.first().map(|found| found.as_str()).unwrap_or_default();
    assert!(line.contains("ready=1 launched=0 reason=hold"), "idle の行の字面: {line}");
    assert!(line.contains(&format!("-t {TARGET} -l ")), "宛先は登録 row の target: {line}");
    assert_eq!(sent.len(), 2, "終端の 1 行と idle の 1 行: {sent:?}");
    assert_eq!(
        stdout_of(&out).lines().filter(|found| *found == "notify=delivered").count(),
        2,
        "送った 2 行の結果: {}",
        told(&out)
    );

    let (quiet, sent) = idle_round(&[], false);
    assert_eq!(quiet.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", told(&quiet));
    assert_eq!(sent.len(), 1, "候補 0 の周は終端の 1 行だけ: {sent:?}");
    assert!(sent.iter().all(|line| !line.contains(" idle ")), "idle の行を送らない: {sent:?}");
    assert!(sent.iter().all(|line| line.contains("Stopped")), "送ったのは終端の行: {sent:?}");
}
