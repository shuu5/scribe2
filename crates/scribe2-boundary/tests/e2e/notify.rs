//! 運転手の終端と「起こす便 0 ∧ 候補あり」を登録 row の席の pane へ 1 行で知らせる歯（設計 docs/design/dispatcher.md §19・
//! 契約表の行 p・接頭辞 `pipe_notify_`）。
//!
//! 偽の `tmux`（呼ばれた引数を 1 行ずつ記録 file へ足し、`send-keys -l` の payload を偽の pane へ写す script）を道具箱の
//! dir（PATH の先頭）に置き、登録 row は core の `register` で積む（実 tmux を立てない）。歯の file は `pipe/` の外に
//! 置く（§19 形 6・`pipe/` 配下の file 数の pin は動かさない）。

use crate::pipe::{
    bin_cmd, ceiling_rules, clean, design_doc_rows, design_pointer, embedded_int, fake_lens, gate_once, gated_pass, git,
    intake_bead, lens_verdict, repo_with_state, review_lens_pass, row_fields, run_pipe, spawn_with, stderr_of, stdout_of,
    write_design, DESIGN_FILE, TOY_COMMIT,
};
use crate::TOOLBOX_BIN;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use vessel::cli_outcome::RC_OK;
use vessel::fleet::lifecycle_mark::{add_mark, Added, Kind as MarkKind, Ledger, Mark, Value};
use vessel::fleet::store::LockPolicy;
use vessel::fleet::Registration;
use vessel::pipe::review::REVIEW_FILE;
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

/// `pipe stop --run` を `--repo` 付きで撃つ（道具を渡さない周＝列は `no-runner` で測らない）。局面の出力の file には先に読めない字を
/// 書く（道具の無い周は書き直さないので、終端の語は `unreadable` になる・設計 dispatcher.md §43 行 ar）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn stop(repo: &Path, state: &Path) -> Output {
    let fleet = state.join("fleet");
    fs::create_dir_all(&fleet).expect("fleet dir を作れる");
    fs::write(fleet.join("lifecycle.json"), "not json\n").expect("読めない出力を書ける");
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

/// (約束 2) 列の結果が「起こした便 0 ∧ 候補 1」（ready の bead を hold にした台帳）の終端の周は、idle の 1 行
/// （`ready=1 launched=0 reason=hold`）を同じ宛先へ送る（出力の file の無い置き場なので終端の行は送らない・設計 §43 行 ar）。
/// 候補 0 の台帳の周は 1 行も送らない。
#[test]
fn pipe_notify_idle_round_reports_ready_count_and_top_reason() {
    let (out, sent) = idle_round(&[QUEUED], true);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", told(&out));
    let idle: Vec<&String> = sent.iter().filter(|line| line.contains(" idle ")).collect();
    assert_eq!(idle.len(), 1, "idle の行は 1 回: {sent:?}");
    let line = idle.first().map(|found| found.as_str()).unwrap_or_default();
    assert!(line.contains("ready=1 launched=0 reason=hold"), "idle の行の字面: {line}");
    assert!(line.contains(&format!("-t {TARGET} -l ")), "宛先は登録 row の target: {line}");
    assert_eq!(sent.len(), 1, "送る行は idle の 1 行だけ: {sent:?}");
    assert_eq!(
        stdout_of(&out).lines().filter(|found| found.starts_with("notify=delivered consumed=")).count(),
        1,
        "送った 1 行の結果: {}",
        told(&out)
    );

    let (quiet, sent) = idle_round(&[], false);
    assert_eq!(quiet.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", told(&quiet));
    assert_eq!(sent.len(), 0, "候補 0 の周は 1 行も送らない: {sent:?}");
}

/// 送った payload の行のうち idle の 1 行（無ければ空）。
fn idle_of(sent: &[String]) -> String {
    sent.iter().find(|line| line.contains(" idle ")).cloned().unwrap_or_default()
}

/// [`idle_round`] の置き場に、候補（行 a・`src/lib.rs`）と 1 file 交差する live な便を足して終端を撃つ。返すのは stdout と
/// payload の送りの列と、live な便の run id。live な便は契約を持たない終端の便より**先に**起こす（契約の無い便が live の間は
/// 受付が交差を測れず断る）。
fn crossing_round() -> (Output, Vec<String>, String) {
    let (repo, state) = repo_with_state();
    fake_tmux(&state);
    register(&state, &repo);
    let crossed = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), "s2-facts.1");
    live_run(&state);
    let bd = fake_bd(&state, "bd-facts", &[QUEUED]);
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
    (out, sent, crossed)
}

/// (§26 歯 (a)) 候補の write-set が live な便 1 本と 1 file 交差する周の終端: idle の行は既存の `reason=overlap:<相手>/1` を
/// 持ったまま、末尾が ` live=1 idle=- held=1:lib.rs`（live が 1 本以上の周の分数は値なし・file 名は最後の 1 要素）。
#[test]
fn pipe_notify_facts_crossing_live_run_reports_one_live_and_the_crossed_file() {
    let (out, sent, crossed) = crossing_round();
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", told(&out));
    let line = idle_of(&sent);
    assert!(
        line.contains(&format!("ready=1 launched=0 reason=overlap:{crossed}/1 ")),
        "既存の key と順と reason= の字面は不変: {line} / {sent:?}"
    );
    assert!(line.ends_with(" live=1 idle=- held=1:lib.rs"), "末尾に並列の実測: {line} / {sent:?}");
}

/// (§26 歯 (b)) live な便が無く候補が hold の周の終端: idle の行は `ready=1 launched=0 reason=hold` の後に
/// ` live=0 idle=0m held=0` で終わる（終端の直後なので 0 分・重なりで待つ候補 0 はコロンなし）。
#[test]
fn pipe_notify_facts_hold_round_without_live_runs_reports_zero_live_and_zero_minutes() {
    let (out, sent) = idle_round(&[QUEUED], true);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", told(&out));
    let line = idle_of(&sent);
    assert!(line.contains("ready=1 launched=0 reason=hold:"), "既存の字面は同じ行に在る: {line} / {sent:?}");
    assert!(line.ends_with(" live=0 idle=0m held=0"), "末尾に並列の実測: {line} / {sent:?}");
}

// ───── 未処置の終端（設計 dispatcher.md §29・行 ad・接頭辞 `pipe_notify_pending_`） ─────

/// 審査の判定 FAIL で終端に着く便の bead（行 a・台帳で ready のまま）。
const PENDING: &str = "s2-pending.1";

/// 行 a の便を審査 FAIL の `Reviewed` の終端に着け（台帳では ready のまま＝`Settled` の候補）、契約を持たない live な便の
/// `pipe stop --run` の終端を道具つきで撃つ。返すのは stdout / stderr と送った payload の列。置き場は局面の出力を書ける置き場で、
/// 測る周の前に `fleet lifecycle write` を 1 回撃って切り替えの線を記帳し、測る周の後に出力が古くなく便の部品が
/// `run-review-failed` であることを先に測る（設計 dispatcher.md §43 行 ar）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn pending_round() -> (Output, Vec<String>) {
    let (repo, state) = repo_with_state();
    fake_tmux(&state);
    register(&state, &repo);
    writable(&repo);
    let failed = intake_bead(&repo, &state, &format!("{DESIGN_FILE}#a"), PENDING);
    fs::write(state.join("pipe").join(&failed).join(REVIEW_FILE), "{\"verdict\":\"FAIL\"}\n").expect("審査の判定を書ける");
    record_stage(&state, &failed, PENDING, "Reviewed", "verdict:FAIL");
    live_run(&state);
    let bd = fake_bd(&state, "bd-pending", &[PENDING]);
    switch_line(&state, &repo, &bd);
    let out = run_pipe(&[
        "stop", "--run", RUN,
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &queue_rules(&state),
        "--bd", &bd,
        "--runner", "true",
    ]);
    assert_fresh(&state, "pending_round");
    assert!(part_of(&state, &failed).contains(" phase=run-review-failed "), "便の部品は run-review-failed: {}", shown(&state));
    let sent = sends(&state);
    clean(&[&repo, &state]);
    (out, sent)
}

/// (§29 歯 (a)) 審査 FAIL の終端の bead が `Settled` の候補に並ぶ周の終端: idle の行は既存の `reason=settled:` を持ったまま
/// ` pending=1:<bead>/Reviewed=FAIL` で終わる。base は key が無い（RED）。
// flip-check: retroactive s2-07l.738.39.5
#[test]
fn pipe_notify_pending_reviewed_fail_rides_the_idle_line() {
    let (out, sent) = pending_round();
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", told(&out));
    let line = idle_of(&sent);
    assert!(line.contains("ready=1 launched=0 reason=settled:"), "既存の字面は同じ行に在る: {line} / {sent:?}");
    assert!(line.ends_with(&format!(" pending=1:{PENDING}/Reviewed=FAIL")), "末尾に未処置の終端: {line} / {sent:?}");
}

/// (§29 歯 (b)) 同じ周の終端の stderr が stdout の `notify=` の行と同じ行を同じ順で持つ（base は stderr に無い＝RED）。
#[test]
fn pipe_notify_pending_terminal_stderr_carries_the_notify_lines() {
    let (out, _) = pending_round();
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", told(&out));
    let stdout = notify_lines(&out);
    assert!(!stdout.is_empty(), "stdout に notify= の行が在る: {}", told(&out));
    let stderr: Vec<String> = stderr_of(&out).lines().filter(|line| line.starts_with("notify=")).map(str::to_owned).collect();
    assert_eq!(stderr, stdout, "stderr に同じ字面: {}", told(&out));
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

// ───── 直しの束（設計 dispatcher.md §27 形 1〜3・行 y・接頭辞 `pipe_notify_precheck_`） ─────
//
// 依存 A（行 a・held）を待つ行 b / c / d が、base にも A の宣言にも無い同じ file を素で持つ＝3 行が同じ根（断りの名
// write-set-item-unresolved と在り処の file）の確定を持つ。base には束が無い＝束の file も知らせの行も末尾も無い（RED）。

/// 束の歯の依存待ちの行の数。
const BUNDLED: usize = 3;

/// 束の歯の依存 A の bead（行 a・held）。
const ANCESTOR: &str = "s2-pre.1";

/// 束の歯の依存待ちの bead（行 b / c / d の順）。
const WAITING: [(&str, &str); BUNDLED] = [("s2-pre.2", "b"), ("s2-pre.3", "c"), ("s2-pre.4", "d")];

/// 行 a が `+src/fresh.rs` を宣言し、行 b / c / d が `file` を素の path で持つ設計 doc を書いて commit する。
fn precheck_rows(repo: &Path, file: &str) {
    let row = |id: &str, item: &str| {
        let write_set = format!("write-set = [\"{item}\"]");
        row_fields(id, &["write-set"], &[write_set.as_str()])
    };
    let mut rows = vec![row("a", "+src/fresh.rs")];
    rows.extend(WAITING.iter().map(|(_, id)| row(id, file)));
    write_design(repo, &design_doc_rows(&rows));
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "precheck-rows"]);
}

/// 台帳の 1 件（行 `row` を指す open の bead・依存は blocks の列）。
fn precheck_issue(id: &str, row: &str, blocks: &[&str]) -> String {
    let deps: Vec<String> = blocks
        .iter()
        .map(|on| format!("{{\"issue_id\":\"{id}\",\"depends_on_id\":\"{on}\",\"type\":\"blocks\"}}"))
        .collect();
    format!(
        "{{\"id\":\"{id}\",\"status\":\"open\",\"priority\":2,\"labels\":[],\
         \"acceptance_criteria\":\"design = {DESIGN_FILE}#{row}\",\"dependencies\":[{}]}}",
        deps.join(",")
    )
}

/// 束の歯の偽の `bd`（A と A を blocks で待つ 3 行）を置き、その path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn precheck_bd(state: &Path) -> String {
    let mut issues = vec![precheck_issue(ANCESTOR, "a", &[])];
    issues.extend(WAITING.iter().map(|(bead, row)| precheck_issue(bead, row, &[ANCESTOR])));
    let json = state.join("bd-precheck.json");
    fs::write(&json, format!("[{}]\n", issues.join(","))).expect("偽の台帳を書ける");
    let path = state.join("bd-precheck");
    fs::write(&path, format!("#!/bin/sh\ncat '{}'\n", json.display())).expect("偽の bd を書ける");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("偽の bd に実行権を付ける");
    path.display().to_string()
}

/// 束の歯の置き場（登録 row・偽の tmux・行 b / c / d が `src/nowhere.rs` を持つ設計 doc・A の hold）。
fn precheck_place() -> (PathBuf, PathBuf, String) {
    let (repo, state) = repo_with_state();
    fake_tmux(&state);
    register(&state, &repo);
    precheck_rows(&repo, "src/nowhere.rs");
    let bd = precheck_bd(&state);
    let held = run_pipe(&["dispatch", "hold", ANCESTOR, "--state-dir", &state.display().to_string()]);
    assert_eq!(held.status.code(), Some(i32::from(RC_OK)), "hold: {}", told(&held));
    (repo, state, bd)
}

/// 契約を持たない live な便 `run` を置いて `pipe stop --run` の終端を道具つきで撃ち（列が測り事前審査が撃たれる）、この周に送った
/// payload の送りの列を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn precheck_stop(repo: &Path, state: &Path, bd: &str, run: &str) -> Vec<String> {
    let recorded = bin_cmd()
        .args(["fleet", "record", "--kind", "RunStage", "--run", run, "--bead", &format!("{run}.bead"), "--stage", "Implemented"])
        .args(["--detail", "implemented"])
        .arg("--state-dir")
        .arg(state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(recorded.status.code(), Some(i32::from(RC_OK)), "fleet record: {}", stderr_of(&recorded));
    let before = sends(state).len();
    let out = run_pipe(&[
        "stop", "--run", run,
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &queue_rules(state),
        "--bd", bd,
        "--runner", "true",
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "stop は rc 0: {}", told(&out));
    sends(state).into_iter().skip(before).collect()
}

/// 送りの列のうち直しの束の行。
fn bundle_sends(sent: &[String]) -> Vec<String> {
    sent.iter().filter(|line| line.contains(" pipe: precheck bundles=")).cloned().collect()
}

/// 置き場の束の dir。
fn bundle_dir(state: &Path) -> PathBuf {
    state.join("pipe").join("precheck").join("bundle")
}

/// 置き場の束の file（名に `.` を持たない file・名の順）。
fn bundle_files(state: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(bundle_dir(state))
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    found.retain(|path| path.file_name().is_some_and(|name| !name.to_string_lossy().contains('.')));
    found.sort();
    found
}

/// `pipe dispatch ls` の `[DISPATCH-BUNDLE]` の行。
fn bundle_lines(repo: &Path, state: &Path, bd: &str) -> Vec<String> {
    let out = run_pipe(&[
        "dispatch", "ls",
        "--state-dir", &state.display().to_string(),
        "--repo", &repo.display().to_string(),
        "--rules", &queue_rules(state),
        "--bd", bd,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "ls は rc 0: {}", told(&out));
    stdout_of(&out).lines().filter(|line| line.starts_with("[DISPATCH-BUNDLE]")).map(str::to_owned).collect()
}

/// (1)(2)(3)(d) 同じ根の確定を持つ待ち行 3 本が束 1 つになり、束の file が 3 行の pointer と TOML と節と測り直しの argv を持ち、
/// 席への 1 行が周ごとに 1 回（束の集合が同じ次の周は送らない）で `precheck bundles=1 rows=3 <束の id>=<束の file の path>` で終わり、
/// idle の行の末尾が ` precheck=3/3:1`。`dispatch ls` は束ごとに `[DISPATCH-BUNDLE]` の 1 行を出す。
#[test]
fn pipe_notify_precheck_same_root_rows_make_one_bundle_sent_once() {
    let (repo, state, bd) = precheck_place();
    let sent = precheck_stop(&repo, &state, &bd, "r-pre-1");
    let files = bundle_files(&state);
    assert_eq!(files.len(), 1, "同じ根の 3 行は束 1 つ: {files:?} / {sent:?}");
    let file = files.first().cloned().unwrap_or_default();
    let id = file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let body = fs::read_to_string(&file).unwrap_or_default();
    for (bead, row) in WAITING {
        for want in [
            format!("row={bead} pointer={DESIGN_FILE}#{row}\n"),
            format!("remeasure={bead} argv=scribe2 pipe preflight --design {DESIGN_FILE}#{row} --bead {bead} --repo "),
            format!("== toml {bead}\n[[contract]]\nid = \"{row}\"\n"),
            format!("== section {bead}\n{DESIGN_FILE}#{row} §1\n"),
        ] {
            assert!(body.contains(&want), "束の file に {want:?}: {body}");
        }
    }
    assert!(body.starts_with(&format!("id={id}\nroot=write-set-item-unresolved at=files:src/nowhere.rs\nfirst_seen=")), "{body}");
    let bundled = bundle_sends(&sent);
    assert_eq!(bundled.len(), 1, "束の行は周に 1 回: {sent:?}");
    let want = format!(" pipe: precheck bundles=1 rows={BUNDLED} {id}={}", file.display());
    assert!(bundled.first().is_some_and(|line| line.ends_with(&want)), "束の行の字面 {want:?}: {bundled:?}");
    let idle = idle_of(&sent);
    assert!(idle.ends_with(" precheck=3/3:1"), "idle の末尾に事前審査の本数: {idle} / {sent:?}");
    assert_eq!(
        bundle_lines(&repo, &state, &bd),
        [format!("[DISPATCH-BUNDLE] id={id} rows={BUNDLED} root=write-set-item-unresolved file={}", file.display())],
        "ls は束ごとに 1 行"
    );
    let again = precheck_stop(&repo, &state, &bd, "r-pre-2");
    assert_eq!(bundle_sends(&again), Vec::<String>::new(), "束の集合が同じ次の周は送らない: {again:?}");
    assert!(idle_of(&again).ends_with(" precheck=3/3:1"), "次の周も末尾は同じ: {again:?}");
    assert_eq!(bundle_files(&state), [file], "束の file は同じ id のまま");
    clean(&[&repo, &state]);
}

/// (1)(2)(e) fixture の設計を直して 3 行の確定が消えた周に、束の file が置き場から外れ、`[DISPATCH-BUNDLE]` の行が 0 になり、
/// `precheck bundles=0 rows=0` の 1 行が 1 回だけ送られる（次の周は送らない）。
#[test]
fn pipe_notify_precheck_cleared_rows_drop_the_bundle_and_say_zero_once() {
    let (repo, state, bd) = precheck_place();
    let first = precheck_stop(&repo, &state, &bd, "r-pre-1");
    assert_eq!(bundle_files(&state).len(), 1, "直す前は束 1 つ: {first:?}");
    precheck_rows(&repo, "src/fresh.rs");
    let cleared = precheck_stop(&repo, &state, &bd, "r-pre-2");
    assert_eq!(bundle_files(&state), Vec::<PathBuf>::new(), "確定の消えた束の file は外れる");
    assert_eq!(bundle_lines(&repo, &state, &bd), Vec::<String>::new(), "ls の束の行は 0");
    let bundled = bundle_sends(&cleared);
    assert_eq!(bundled.len(), 1, "束が 0 本になった周は 1 回送る: {cleared:?}");
    assert!(bundled.first().is_some_and(|line| line.ends_with(" pipe: precheck bundles=0 rows=0")), "0 本の行: {bundled:?}");
    assert!(idle_of(&cleared).ends_with(" precheck=0/3:0"), "確定 0・結果 3・束 0: {cleared:?}");
    let quiet = precheck_stop(&repo, &state, &bd, "r-pre-3");
    assert_eq!(bundle_sends(&quiet), Vec::<String>::new(), "次の周は送らない: {quiet:?}");
    clean(&[&repo, &state]);
}

// ───── 便の終端の知らせの 1 語を局面の出力の便の部品から読む（設計 dispatcher.md §43 行 ar・接頭辞 `pipe_notify_lifecycle_`） ─────

/// 局面の出力を書ける置き場にする（台帳の印の file と git の exclude の `.beads/` と origin/main の ref）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn writable(repo: &Path) {
    fs::create_dir_all(repo.join(".beads")).expect(".beads を作れる");
    fs::write(repo.join(".beads/issues.jsonl"), "[]\n").expect("台帳の file を書ける");
    let exclude = repo.join(".git/info/exclude");
    let body = fs::read_to_string(&exclude).unwrap_or_default();
    fs::write(&exclude, format!("{body}.beads/\n")).expect("exclude を書ける");
    let main = git(repo, &["rev-parse", "refs/heads/main"]);
    git(repo, &["update-ref", "refs/remotes/origin/main", &main]);
}

/// 便の段を 1 つ `fleet record` で足す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn record_stage(state: &Path, run: &str, bead: &str, stage: &str, detail: &str) {
    let out = bin_cmd()
        .args(["fleet", "record", "--kind", "RunStage", "--run", run, "--bead", bead, "--stage", stage, "--detail", detail])
        .arg("--state-dir")
        .arg(state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fleet record: {}", stderr_of(&out));
}

/// bead を hold にする（測る周に列が便を起こさない・起こす便の子が足す event で語が揺れない）。
fn hold(state: &Path, bead: &str) {
    let out = run_pipe(&["dispatch", "hold", bead, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "hold: {}", told(&out));
}

/// `fleet lifecycle write`（書き手の 1 回・rc は呼び手が見る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn lifecycle_write(state: &Path, repo: &Path, bd: &str) -> Output {
    bin_cmd()
        .args(["fleet", "lifecycle", "write", "--state-dir"])
        .arg(state)
        .arg("--repo")
        .arg(repo)
        .args(["--bd", bd])
        .output()
        .expect("binary を起動できる")
}

/// 測る周の前に書き直しを 1 回撃って切り替えの線を記帳する（置き場で最初の書き直しの周は出力が古い・設計 §43 何が起きているか）。
fn switch_line(state: &Path, repo: &Path, bd: &str) {
    let out = lifecycle_write(state, repo, bd);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fleet lifecycle write: {}", stderr_of(&out));
}

/// `fleet lifecycle show` の stdout。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn shown(state: &Path) -> String {
    let out = bin_cmd().args(["fleet", "lifecycle", "show", "--state-dir"]).arg(state).output().expect("binary を起動できる");
    stdout_of(&out)
}

/// 便の部品の行（無ければ空）。
fn part_of(state: &Path, run: &str) -> String {
    let prefix = format!("part=run id={run} ");
    shown(state).lines().find(|line| line.starts_with(&prefix)).map(str::to_owned).unwrap_or_default()
}

/// 部品の行の `reason=` の値。
fn reason_of(line: &str) -> String {
    line.split_once(" reason=").map(|(_, value)| value.trim().to_owned()).unwrap_or_default()
}

/// 置き場の event log の byte 長。
fn events_len(state: &Path) -> u64 {
    fs::metadata(state.join("fleet").join("events.jsonl")).map(|found| found.len()).unwrap_or(0)
}

/// 出力の頭の行の `events=` が event log の byte 長に等しく `stale=-` である（古くない前提を歯が測る）。
fn assert_fresh(state: &Path, who: &str) {
    let text = shown(state);
    let head = text.lines().next().unwrap_or_default();
    assert!(head.contains(&format!(" events={} ", events_len(state))), "{who}: events= が event log の長さに等しい: {text}");
    assert!(head.ends_with(" stale=-"), "{who}: stale=-: {text}");
}

/// 出力の頭の行の `events=` が event log の長さと違う（古い周の前提）。
fn assert_behind(state: &Path, who: &str) {
    let text = shown(state);
    let head = text.lines().next().unwrap_or_default();
    assert!(!head.contains(&format!(" events={} ", events_len(state))), "{who}: 出力は event log より後れている: {text}");
}

/// 台帳の 1 件（行 a を指す契約・依存なし）。
fn issue_json(id: &str, status: &str) -> String {
    format!(
        "{{\"id\":\"{id}\",\"status\":\"{status}\",\"priority\":2,\"labels\":[],\
         \"acceptance_criteria\":\"design = {DESIGN_FILE}#a\",\"dependencies\":[]}}"
    )
}

/// 台帳の JSON を返し、close は `close_rc` で返る偽の `bd`（path を返す）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn ledger_bd(state: &Path, name: &str, issues: &[String], close_rc: u8) -> String {
    let json = state.join(format!("{name}.json"));
    fs::write(&json, format!("[{}]\n", issues.join(","))).expect("偽の台帳を書ける");
    let path = state.join(name);
    fs::write(&path, format!("#!/bin/sh\nif [ \"$1\" = close ]; then exit {close_rc}; fi\ncat '{}'\n", json.display())).expect("偽の bd を書ける");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("偽の bd に実行権を付ける");
    path.display().to_string()
}

/// 局面の出力を書ける置き場（登録 row と偽の tmux つき）。
fn site() -> (PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    fake_tmux(&state);
    register(&state, &repo);
    writable(&repo);
    (repo, state)
}

/// 終端の周の引数（置き場・repo・規則・台帳・実装役の口）を足して撃つ。
fn terminal_round(repo: &Path, state: &Path, bd: &str, rules: &str, verb: &[&str]) -> Output {
    let mut args: Vec<&str> = verb.to_vec();
    let (state_arg, repo_arg) = (state.display().to_string(), repo.display().to_string());
    args.extend(["--state-dir", &state_arg, "--repo", &repo_arg, "--rules", rules, "--bd", bd, "--runner", "true"]);
    run_pipe(&args)
}

/// 送った payload のうち終端の行の `<段>=<語>`（idle と束の行は除く）。
fn terminal_words(sent: &[String]) -> Vec<String> {
    sent.iter()
        .filter(|line| !line.contains(" idle ") && !line.contains(" precheck "))
        .filter_map(|line| {
            line.split_whitespace()
                .find(|token| token.split_once('=').is_some_and(|(key, _)| key.starts_with(|ch: char| ch.is_ascii_uppercase())))
                .map(str::to_owned)
        })
        .collect()
}

/// `lock_retry_ms` を 100 に縮めた列の写し（Busy の周を 5 秒待たない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn fast_rules(state: &Path) -> String {
    let body = fs::read_to_string(queue_rules(state)).expect("列の写しを読める");
    let from = "kind = \"LockRetryMs\"\nvalue = 5000";
    assert!(body.contains(from), "行の字面が在る");
    let path = state.join("rules-notify-fast.toml");
    fs::write(&path, body.replace(from, "kind = \"LockRetryMs\"\nvalue = 100")).expect("写しを書ける");
    path.display().to_string()
}

/// 審査 FAIL で終端する便（契約の bead は台帳で開き `Settled` の候補になる）を置く。返すのは便の id。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn reviewed_fail(repo: &Path, state: &Path, bead: &str) -> String {
    let id = intake_bead(repo, state, &design_pointer(), bead);
    fs::write(state.join("pipe").join(&id).join(REVIEW_FILE), "{\"verdict\":\"FAIL\"}\n").expect("審査の判定を書ける");
    record_stage(state, &id, bead, "Reviewed", "verdict:FAIL");
    id
}

/// (a) 古くない周: Reviewed の FAIL の便の終端の行の語は判定の語・Stopped の便の語は `Stopped`・`Settled` の候補の Gated の FAIL の
/// 便の pending の語は `FAIL`。
#[test]
fn pipe_notify_lifecycle_fresh_round_reads_the_word_from_the_part_reason() {
    let (repo, state) = site();
    let failed = reviewed_fail(&repo, &state, "s2-rf.1");
    let bd = ledger_bd(&state, "bd-reviewed", &[issue_json("s2-rf.1", "open")], 0);
    switch_line(&state, &repo, &bd);
    let rules = queue_rules(&state);
    let out = terminal_round(&repo, &state, &bd, &rules, &["retire", "--run", &failed]);
    assert_fresh(&state, "Reviewed の FAIL");
    assert!(part_of(&state, &failed).contains(" phase=run-review-failed turn=seat "), "前提: 手番が seat の部品: {}", shown(&state));
    assert_eq!(terminal_words(&sends(&state)), ["Reviewed=FAIL"], "語は判定の語: {}", told(&out));
    clean(&[&repo, &state]);

    let (repo, state) = site();
    live_run(&state);
    hold(&state, BEAD);
    let bd = ledger_bd(&state, "bd-stopped", &[issue_json(BEAD, "open")], 0);
    switch_line(&state, &repo, &bd);
    let out = terminal_round(&repo, &state, &bd, &queue_rules(&state), &["stop", "--run", RUN]);
    assert_fresh(&state, "Stopped");
    assert_eq!(terminal_words(&sends(&state)), ["Stopped=Stopped"], "語は段の名: {}", told(&out));
    clean(&[&repo, &state]);

    let (repo, state) = site();
    let gated = intake_bead(&repo, &state, &design_pointer(), "s2-gf.1");
    let spawned = spawn_with(&repo, &state, &gated, TOY_COMMIT);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", told(&spawned));
    let lens = fake_lens(&state.join("gate-lens-fail"), &lens_verdict("FAIL"));
    assert_ne!(gate_once(&repo, &state, &gated, Some(&lens)).status.code(), Some(i32::from(RC_OK)), "FAIL の gate は rc 0 でない");
    live_run(&state);
    let bd = ledger_bd(&state, "bd-gated", &[issue_json("s2-gf.1", "open")], 0);
    switch_line(&state, &repo, &bd);
    let out = terminal_round(&repo, &state, &bd, &queue_rules(&state), &["stop", "--run", RUN]);
    assert_fresh(&state, "Gated の FAIL");
    assert!(part_of(&state, &gated).contains(" phase=run-gate-failed "), "前提: Gated の FAIL の部品: {}", shown(&state));
    let sent = sends(&state);
    assert!(idle_of(&sent).ends_with(" pending=1:s2-gf.1/Gated=FAIL"), "pending の語は FAIL: {sent:?} / {}", told(&out));
    clean(&[&repo, &state]);
}

/// (b) 古くない周: 台帳で契約が閉じた Stopped の便（出力に部品が無い）は送らず、開いた契約の Failed の便は送り、契約が開いたまま札の
/// 無い Landed の便（run-landed-open）は送って語がその便の部品の行の `reason=` の値に等しい。
#[test]
fn pipe_notify_lifecycle_fresh_round_sends_only_the_runs_with_a_seat_part() {
    let (repo, state) = site();
    live_run(&state);
    let bd = ledger_bd(&state, "bd-closed", &[issue_json(BEAD, "closed")], 0);
    switch_line(&state, &repo, &bd);
    let out = terminal_round(&repo, &state, &bd, &queue_rules(&state), &["stop", "--run", RUN]);
    assert_fresh(&state, "契約が閉じた Stopped");
    assert_eq!(part_of(&state, RUN), "", "前提: 部品が無い: {}", shown(&state));
    assert_eq!(terminal_words(&sends(&state)), Vec::<String>::new(), "部品の無い便は送らない: {}", told(&out));
    clean(&[&repo, &state]);

    let (repo, state) = site();
    let bd = ledger_bd(&state, "bd-failed", &[issue_json("s2-fl.1", "open")], 0);
    switch_line(&state, &repo, &bd);
    let out = run_pipe(&[
        "run", "--design", &design_pointer(), "--bead", "s2-fl.1",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &queue_rules(&state), "--bd", &bd, "--runner", "exit 2", "--lens", &review_lens_pass(&state),
    ]);
    assert_fresh(&state, "Failed");
    assert_eq!(terminal_words(&sends(&state)), ["Failed=runner-rc"], "語は最後の detail の頭: {}", told(&out));
    clean(&[&repo, &state]);

    let (repo, state) = site();
    let id = gated_pass(&repo, &state, &design_pointer(), &state.join("lens-ran"));
    let bd = ledger_bd(&state, "bd-landed", &[issue_json("s2-2e5", "open")], 3);
    let rules = queue_rules(&state);
    let landed = terminal_round(&repo, &state, &bd, &rules, &["land", "--run", &id]);
    assert_ne!(landed.status.code(), Some(i32::from(RC_OK)), "close の落ちる着地は rc 0 でない: {}", told(&landed));
    switch_line(&state, &repo, &bd);
    let sent_before = sends(&state).len();
    let out = terminal_round(&repo, &state, &bd, &rules, &["land", "--run", &id, "--terminal-only"]);
    assert_fresh(&state, "Landed");
    let part = part_of(&state, &id);
    assert!(part.contains(" phase=run-landed-open turn=seat "), "前提: 契約が開いたまま札の無い Landed: {}", shown(&state));
    let sent: Vec<String> = sends(&state).into_iter().skip(sent_before).collect();
    assert_eq!(terminal_words(&sent), [format!("Landed={}", reason_of(&part))], "語は部品の理由: {}", told(&out));
    clean(&[&repo, &state]);
}

/// (c) 古い周: lifecycle.lock を生きた pid で持たせて書き直しを Busy にした周は、出力の部品が run-asking（手番 seat）の便の語に
/// `:stale` が付き、run-implementing（手番 runner）の便の語は `stale`。3 つ目の置き場は古さの印だけで古い周になり、`Settled` の
/// 候補の Reviewed の FAIL の便の pending の語が `FAIL:stale`。
#[test]
fn pipe_notify_lifecycle_stale_round_marks_the_word_and_never_stays_quiet() {
    for (stage, bead, run, expected) in [
        ("Questioned", "s2-ask.1", "r-ask", "Stopped=Questioned:stale"),
        ("Spawned", "s2-impl.1", "r-impl", "Stopped=stale"),
    ] {
        let (repo, state) = site();
        record_stage(&state, run, bead, stage, "x");
        hold(&state, bead);
        let bd = ledger_bd(&state, "bd-stale", &[issue_json(bead, "open")], 0);
        switch_line(&state, &repo, &bd);
        fs::write(state.join("fleet").join("lifecycle.lock"), format!("{}\n", std::process::id())).expect("lock を置ける");
        let out = terminal_round(&repo, &state, &bd, &fast_rules(&state), &["stop", "--run", run]);
        assert!(stderr_of(&out).lines().any(|line| line == "lifecycle=busy"), "書き直しは Busy: {}", told(&out));
        assert_behind(&state, stage);
        assert_eq!(terminal_words(&sends(&state)), [expected], "{stage} の便の語: {}", told(&out));
        clean(&[&repo, &state]);
    }

    let (repo, state) = site();
    reviewed_fail(&repo, &state, "s2-rf.1");
    live_run(&state);
    let bd = ledger_bd(&state, "bd-pending-stale", &[issue_json("s2-rf.1", "open")], 0);
    switch_line(&state, &repo, &bd);
    let mark = Mark { kind: MarkKind::LedgerGate, at: "2999-01-01T00:00:00Z".to_owned(), value: Value::Ledger(Ledger::Files { len: 1, mtime_ns: 1 }) };
    assert_eq!(add_mark(&state, &mark, LockPolicy { retry_ms: 500, stale_ms: 600_000 }), Added::Added, "古さの印を足せる");
    let out = terminal_round(&repo, &state, &bd, &queue_rules(&state), &["stop", "--run", RUN]);
    let text = shown(&state);
    assert!(text.lines().next().is_some_and(|head| head.ends_with(" stale=ledger-gate")), "印が残る: {text}");
    let sent = sends(&state);
    assert!(idle_of(&sent).ends_with(" pending=1:s2-rf.1/Reviewed=FAIL:stale"), "pending の古い形: {sent:?} / {}", told(&out));
    clean(&[&repo, &state]);
}

/// (d) 書き手が出力を書けない置き場（台帳の印の file も origin/main の ref も無い）で、出力の file の無い周は終端の行を送らず、
/// `Settled` の候補が 1 本の周の pending は `unreadable`。出力の file が読めない字の周の終端は語 `unreadable` で送る。
#[test]
fn pipe_notify_lifecycle_absent_output_sends_no_terminal_and_an_unreadable_one_says_so() {
    let (repo, state) = repo_with_state();
    fake_tmux(&state);
    register(&state, &repo);
    reviewed_fail(&repo, &state, "s2-rf.1");
    live_run(&state);
    let bd = ledger_bd(&state, "bd-absent", &[issue_json("s2-rf.1", "open")], 0);
    let out = terminal_round(&repo, &state, &bd, &queue_rules(&state), &["stop", "--run", RUN]);
    assert!(!state.join("fleet").join("lifecycle.json").exists(), "前提: 出力の file は無い");
    let sent = sends(&state);
    assert_eq!(terminal_words(&sent), Vec::<String>::new(), "出力の file の無い周は終端の行を送らない: {}", told(&out));
    assert!(idle_of(&sent).ends_with(" pending=unreadable"), "pending は unreadable: {sent:?}");
    clean(&[&repo, &state]);

    let (repo, state) = repo_with_state();
    fake_tmux(&state);
    register(&state, &repo);
    live_run(&state);
    let out = stop(&repo, &state);
    assert_eq!(terminal_words(&sends(&state)), ["Stopped=unreadable"], "読めない出力の終端は語 unreadable: {}", told(&out));
    clean(&[&repo, &state]);
}
