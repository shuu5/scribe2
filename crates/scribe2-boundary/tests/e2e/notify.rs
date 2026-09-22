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

/// 偽の `tmux` を道具箱の dir に置く（Enter を 1 回も落とさない席）。
fn fake_tmux(state: &Path) {
    seat_tmux(state, 0, "");
}

/// 登録 row の席の置き場（`<state>/seat/<潰した target>/`）。
fn seat_of(state: &Path) -> PathBuf {
    state.join("seat").join(TARGET.replace(':', "_"))
}

/// 状態付きの偽の `tmux` を道具箱の dir に置く（設計 dispatcher.md §21 の歯の形）。`send-keys -l` は payload を入力欄の
/// file へ書き、`send-keys Enter` は「落とす回数」が 0 でなければ 1 減らして何もせず、0 なら入力欄を pane の本文へ移して
/// 席の打刻 file に `UserPromptSubmit` の 1 行を足す。`capture-pane` は本文の後に prompt 行 + 入力欄を返す。どの起動も
/// 引数を記録 file へ 1 行で足し、`send-keys` は時刻（ns）を別の file へ足す。打刻 file は空で先に置く（hook の載った席）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn seat_tmux(state: &Path, drops: u32, typed: &str) {
    let bin = state.join(TOOLBOX_BIN);
    fs::create_dir_all(&bin).expect("道具箱の dir を作れる");
    let pane = state.join("tmux-pane");
    fs::write(&pane, "").expect("偽の pane を作れる");
    let input = state.join("tmux-input");
    fs::write(&input, typed).expect("偽の入力欄を作れる");
    let drop_file = state.join("tmux-drops");
    fs::write(&drop_file, drops.to_string()).expect("落とす回数を書ける");
    let seat = seat_of(state);
    fs::create_dir_all(&seat).expect("席の置き場を作れる");
    let stamps = seat.join("state.jsonl");
    fs::write(&stamps, "").expect("打刻 file を作れる");
    let script = format!(
        "#!/bin/sh\n\
         printf '%s\\n' \"$*\" >> '{calls}'\n\
         case \"$1\" in\n\
         capture-pane) cat '{pane}'; printf '\\342\\235\\257 '; cat '{input}'; printf '\\n';;\n\
         send-keys) date +%s%N >> '{times}'\n\
         if [ \"$4\" = \"-l\" ]; then printf '%s' \"$5\" >> '{input}'\n\
         elif [ \"$4\" = \"Enter\" ]; then\n\
         n=$(cat '{drops}')\n\
         if [ \"$n\" -gt 0 ]; then echo $((n - 1)) > '{drops}'\n\
         else cat '{input}' >> '{pane}'; printf '\\n' >> '{pane}'; : > '{input}'\n\
         printf '{{\"schema\":1,\"state\":\"busy\",\"event\":\"UserPromptSubmit\",\"ts\":%s,\"sid\":\"\"}}\\n' \"$(date +%s)\" >> '{stamps}'\n\
         fi\n\
         fi;;\n\
         esac\n\
         exit 0\n",
        calls = calls_of(state).display(),
        times = times_of(state).display(),
        pane = pane.display(),
        input = input.display(),
        drops = drop_file.display(),
        stamps = stamps.display()
    );
    let path = bin.join("tmux");
    fs::write(&path, script).expect("偽の tmux を書ける");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("偽の tmux に実行権を付ける");
}

/// 偽の `tmux` の `send-keys` の時刻の file（1 起動 1 行・ns）。
fn times_of(state: &Path) -> PathBuf {
    state.join("tmux-times")
}

/// 偽の `tmux` が受けた Enter だけの送り（`send-keys -t <target> Enter` の行）の本数。
fn enters(state: &Path) -> usize {
    let enter = format!("send-keys -t {TARGET} Enter");
    fs::read_to_string(calls_of(state)).unwrap_or_default().lines().filter(|line| *line == enter).count()
}

/// 偽の `tmux` が受けた `send-keys` の全数（text も Enter も）。
fn keys(state: &Path) -> usize {
    fs::read_to_string(calls_of(state)).unwrap_or_default().lines().filter(|line| line.starts_with("send-keys ")).count()
}

/// stdout の `notify=` の行の列。
fn notify_lines(out: &Output) -> Vec<String> {
    stdout_of(out).lines().filter(|line| line.starts_with("notify=")).map(str::to_owned).collect()
}

/// 状態付きの偽の席（落とす回数 `drops`・入力欄の先の字面 `typed`）に登録 row と live な便を置いて `pipe stop --run` の
/// 終端を撃つ（道具を渡さない周＝終端の 1 行だけを送る）。返すのは stdout と、Enter の本数と send-keys の全数と、
/// 送達の記録（`tick.jsonl`）の行と、text と Enter の間の ns（text の後に Enter が無ければ `None`）。
fn seat_round(drops: u32, typed: &str) -> Round {
    let (repo, state) = repo_with_state();
    seat_tmux(&state, drops, typed);
    register(&state, &repo);
    live_run(&state);
    let out = stop(&repo, &state);
    let times: Vec<u128> = fs::read_to_string(times_of(&state))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect();
    let gap = match times.as_slice() {
        [text, enter, ..] => enter.checked_sub(*text),
        _ => None,
    };
    let round = Round {
        notify: notify_lines(&out),
        told: told(&out),
        rc: out.status.code(),
        enters: enters(&state),
        keys: keys(&state),
        texts: sends(&state).len(),
        ticks: fs::read_to_string(seat_of(&state).join("tick.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect(),
        gap,
    };
    clean(&[&repo, &state]);
    round
}

/// [`seat_round`] の測り。
struct Round {
    /// stdout の `notify=` の行。
    notify: Vec<String>,
    /// 落ちた周に写す 1 行。
    told: String,
    /// rc。
    rc: Option<i32>,
    /// Enter だけの送りの本数。
    enters: usize,
    /// send-keys の全数。
    keys: usize,
    /// text の送り（`-l`）の本数。
    texts: usize,
    /// 送達の記録の行。
    ticks: Vec<String>,
    /// text と Enter の間（ns）。
    gap: Option<u128>,
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
    assert!(
        stdout_of(&out).lines().any(|line| line.starts_with("notify=delivered consumed=")),
        "stdout に notify=delivered: {}",
        told(&out)
    );
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
        stdout_of(&out).lines().filter(|found| found.starts_with("notify=delivered consumed=")).count(),
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

/// settle の 1 歩（`SETTLE_STEP` = 200 ms）を ns で。本文と Enter の間はこれ以上空く（§21 形 3）。
const STEP_NS: u128 = 200_000_000;

/// (§21 (b)) Enter を落とさない席: Enter は 1 回・text は 1 回・stdout は `notify=delivered consumed=true`。送達の記録が
/// 運転手の置き場の `tick.jsonl` に自席の 1 行で増え（置き場を渡した＝形 1）、本文と Enter の間に settle の 1 歩が在る（形 3）。
#[test]
fn pipe_notify_delivery_consumed_on_the_first_enter_records_and_steps() {
    let round = seat_round(0, "");
    assert_eq!(round.rc, Some(i32::from(RC_OK)), "stop は rc 0: {}", round.told);
    assert_eq!(round.notify, vec!["notify=delivered consumed=true".to_owned()], "{}", round.told);
    assert_eq!(round.enters, 1, "Enter は 1 回: {}", round.told);
    assert_eq!(round.texts, 1, "text は 1 回: {}", round.told);
    assert_eq!(round.ticks.len(), 1, "送達の記録が 1 行増える: {:?}", round.ticks);
    let tick = round.ticks.first().cloned().unwrap_or_default();
    for word in ["\"who\":\"seat-inject\"", BEAD, RUN] {
        assert!(tick.contains(word), "記録に {word}: {tick}");
    }
    let gap = round.gap.unwrap_or_default();
    assert!(gap >= STEP_NS, "本文と Enter の間に 1 歩（{STEP_NS} ns 以上）: {gap} ns");
}

/// (§21 (a)) Enter を 1 回落とす席: 窓が Queued で閉じた周に入力欄の残りがこの周の本文なので Enter を 1 回だけ再送し、
/// 2 度目の settle で消費が測れて `consumed=true`。text の再送は 0 回（text 1・Enter 2）。
#[test]
fn pipe_notify_queued_once_resends_enter_and_is_consumed() {
    let round = seat_round(1, "");
    assert_eq!(round.rc, Some(i32::from(RC_OK)), "stop は rc 0: {}", round.told);
    assert_eq!(round.notify, vec!["notify=delivered consumed=true".to_owned()], "{}", round.told);
    assert_eq!(round.enters, 2, "Enter は 2 回: {}", round.told);
    assert_eq!(round.texts, 1, "text は再送しない: {}", round.told);
    assert_eq!(round.ticks.len(), 1, "送達の記録は 1 行: {:?}", round.ticks);
}

/// (§21 (c)) Enter を 2 回落とす席: 再送は 1 回だけ（3 回目は無い）で、2 度目も Queued のまま `consumed=false` を返す。
#[test]
fn pipe_notify_queued_twice_stops_after_one_resend() {
    let round = seat_round(2, "");
    assert_eq!(round.rc, Some(i32::from(RC_OK)), "stop は rc 0: {}", round.told);
    assert_eq!(round.notify, vec!["notify=delivered consumed=false".to_owned()], "{}", round.told);
    assert_eq!(round.enters, 2, "Enter は最大 2 回: {}", round.told);
    assert_eq!(round.texts, 1, "text は再送しない: {}", round.told);
}

/// (§21 (d)) 入力欄に他人の文が先に在る席: 1 key も送らず `notify=refused:busy`（不変）・送達の記録も書かない。
#[test]
fn pipe_notify_foreign_input_sends_no_key_and_is_refused_busy() {
    let round = seat_round(0, "人の打ちかけ");
    assert_eq!(round.rc, Some(i32::from(RC_OK)), "stop は rc 0: {}", round.told);
    assert_eq!(round.notify, vec!["notify=refused:busy".to_owned()], "{}", round.told);
    assert_eq!(round.keys, 0, "send-keys は 0 回: {}", round.told);
    assert_eq!(round.ticks.len(), 0, "送っていない周は記録しない: {:?}", round.ticks);
}
