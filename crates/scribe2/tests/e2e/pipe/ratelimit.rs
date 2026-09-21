// flip-check: moved s2-07l.349
//! 上限停止と別口座での再開の歯: `pipe_ratelimit_`（`pipe_ratelimit_resume_` / `pipe_ratelimit_host_` を含む）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く（歯の本文は `lifecycle.rs` から移しただけ・
//! `s2-07l.349`）。口座の fixture（`pub(super)`）は `spawn.rs` / `land.rs` が親の shim `lifecycle` 越しに引く。

use super::*;

// ───── 上限停止の段（`s2-07l.190`・設計 account-autonomy.md §2・SRS FR35・接頭辞 `pipe_ratelimit_`） ─────

/// 上限で止まった runner が stdout に出す停止行（headless の `stop_line` と同じ字面）。
const RATE_LIMIT_STOP_LINE: &str = "runner: rate limit の record を見たので止めた rate-limit-status=allowed_warning";

/// 偽 runner（実行 file）の runner cmd: 呼出回数を置き場へ写し、`line` が在れば stdout へ 1 行出して
/// rc `rc` で終わる（**commit を作らない**）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn limit_runner(state: &Path, line: Option<&str>, rc: u8) -> String {
    let dir = stub_dir(state);
    fs::create_dir_all(&dir).expect("stub の置き場を作れる");
    let path = state.join("limit-runner.sh");
    let print = line.map(|found| format!("printf '%s\\n' '{found}'\n")).unwrap_or_default();
    let body = format!("#!/bin/sh\nprintf 'call\\n' >> '{}/calls'\n{print}exit {rc}\n", dir.display());
    fs::write(&path, body).expect("stub を書ける");
    format!("sh {}", path.display())
}

/// 便の `RunStage` の最新 `(段, detail)`。
fn last_stage(state: &Path, id: &str) -> Option<(Option<Stage>, Option<String>)> {
    stages(state, id).pop()
}

/// write-set `src/lib.rs` の便を intake → 停止行 + rc 75 の runner で spawn し、`RateLimited` に
/// 倒れたことを確かめて run id と runner cmd を返す。
fn rate_limited_run(repo: &Path, state: &Path) -> (String, String) {
    let first = write_set_contract(repo, "first", &["src/lib.rs"]);
    let id = intake_bead(repo, state, &first, "s2-limit");
    let runner = limit_runner(state, Some(RATE_LIMIT_STOP_LINE), RC_RATE_LIMIT);
    let out = spawn_with(repo, state, &id, &runner);
    assert!(stdout_of(&out).contains("stage=RateLimited"), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(stub_calls(state), 1, "runner は 1 回だけ起きる");
    (id, runner)
}

/// runner が停止行を出して rc 75 で終わった便は **`RateLimited` で残る**: (1) 段の最新が
/// `RateLimited` で rc と status を運び `Failed` は 0 本 (2) worktree と base を保つ (3) `pipe show`
/// に段名が出る。
#[test]
fn pipe_ratelimit_spawn_keeps_the_run_at_rate_limited() {
    let (repo, state) = repo_with_state();
    // 行の commit が main を進めるので、base は**便を起こした後**に読む（契約 (b)）。
    let (id, _) = rate_limited_run(&repo, &state);
    let base = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(
        last_stage(&state, &id),
        Some((Some(Stage::RateLimited), Some("rc:75,status:allowed_warning,account:inherited".to_owned()))),
        "段は RateLimited で status と口座（渡していない周は inherited）を運ぶ"
    );
    assert_eq!(stage_count(&state, &id, Stage::Failed), 0, "Failed に倒さない");
    let worktree = worktree_of(&repo, &id);
    assert!(worktree.exists(), "worktree は残る（retire されない）: {}", worktree.display());
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), base, "worktree は base の commit のまま");
    assert!(show_line(&repo, &state, &id).contains("stage=RateLimited"), "永続面に RateLimited が残る");
    clean(&[&repo, &state]);
}

/// `RateLimited` は**終端でない**: (4) intake の排他の母集団に数えられ、credential の無い置き場では
/// `resume` が口座待ち（rc 3・待つ reset が無いので起こさない）で止まり、(5) `stop --run` で `Stopped` に
/// 倒せる（倒した後は同じ write-set が通る）。
#[test]
fn pipe_ratelimit_run_stays_live_until_stopped() {
    let (repo, state) = repo_with_state();
    let (id, runner) = rate_limited_run(&repo, &state);
    let second = write_set_contract(&repo, "second", &["src/lib.rs"]);
    let blocked = try_intake(&repo, &state, &second, "s2-next");
    assert_eq!(blocked.status.code(), Some(i32::from(RC_REFUSED)), "RateLimited は live: {}", stdout_of(&blocked));
    assert!(stderr_of(&blocked).contains(&id), "交差した相手を名乗る: {}", stderr_of(&blocked));
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_BLOCKED)), "測れる口座が無い周は口座待ち: {}", stderr_of(&resumed));
    assert!(stdout_of(&resumed).contains(&format!("run={id} next=wait reset=-")), "{}", stdout_of(&resumed));
    assert!(stderr_of(&resumed).contains("候補なし: unmeasured"), "{}", stderr_of(&resumed));
    assert_eq!(stub_calls(&state), 1, "口座が無い周は runner を起こさない");
    assert!(show_line(&repo, &state, &id).contains("stage=RateLimited"), "便は RateLimited のまま live");
    stop_run_ok(&state, &id);
    assert!(show_line(&repo, &state, &id).contains("stage=Stopped"), "stop で Stopped");
    let passed = try_intake(&repo, &state, &second, "s2-third");
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "止めた後は通る: {}", stderr_of(&passed));
    clean(&[&repo, &state]);
}

/// (6) 停止行の無い rc 75（stdout 空）は `status:unknown` で、段は `RateLimited`（Failed に倒れない）。
#[test]
fn pipe_ratelimit_without_stop_line_records_unknown_status() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let out = spawn_with(&repo, &state, &id, &limit_runner(&state, None, RC_RATE_LIMIT));
    assert!(stdout_of(&out).contains("stage=RateLimited"), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(
        last_stage(&state, &id),
        Some((Some(Stage::RateLimited), Some("rc:75,status:unknown,account:inherited".to_owned()))),
        "読めない status は unknown"
    );
    assert_eq!(stage_count(&state, &id, Stage::Failed), 0, "読めないを Failed に倒さない");
    clean(&[&repo, &state]);
}

/// (7) rc 75 以外は従来どおり `Failed detail=runner-rc:<rc>,commits:<n>`（停止行を出していても
/// rc で分岐してから読む＝他の rc では読まない）。
#[test]
fn pipe_ratelimit_other_rc_still_fails() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let out = spawn_with(&repo, &state, &id, &limit_runner(&state, Some(RATE_LIMIT_STOP_LINE), 1));
    assert!(stdout_of(&out).contains("stage=Failed"), "{} / {}", stdout_of(&out), stderr_of(&out));
    assert_eq!(
        last_stage(&state, &id),
        Some((Some(Stage::Failed), Some("runner-rc:1,commits:0".to_owned()))),
        "他の rc の経路は不変"
    );
    assert_eq!(stage_count(&state, &id, Stage::RateLimited), 0, "rc 1 は RateLimited にしない");
    clean(&[&repo, &state]);
}

// ───── 別口座での途中再開（`s2-07l.210`・設計 account-autonomy.md §4・SRS FR37 / FR36 / FR14 / AC12・接頭辞 `pipe_ratelimit_resume_`） ─────

/// turn 1 つ分の本文: 停止行を出して上限の rc で止まる（commit を作らない）。
fn limit_turn() -> String {
    format!("printf '%s\\n' '{RATE_LIMIT_STOP_LINE}'\nexit {RC_RATE_LIMIT}")
}

/// turn 1 つ分の本文: commit を 1 本作ってから上限で止まる（途中再開の一覧に載る commit）。
fn commit_then_limit_turn() -> String {
    format!("printf 'y\\n' >> src/lib.rs\ngit add -A\ngit commit -q -m partial-work\n{}", limit_turn())
}

/// turn 1 つ分の本文: 質問 record で止まる（commit を作らない）。
fn ask_turn() -> String {
    format!("printf '%s\\n' '{QUESTION_RECORD}'\nexit 76")
}

/// turn ごとに本文を振る偽 runner（実行 file）の runner cmd。どの turn も呼出回数・argv（1 行 1 引数の
/// `argv-<n>`）・stdin（`stdin-<n>`）を置き場へ写す＝「どの口座で・何を渡されて」起きたかを効果で測る。
/// 本文の無い turn は rc 1 で落ちる（数え落としを静かに通さない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn turn_runner(state: &Path, turns: &[String]) -> String {
    let dir = stub_dir(state);
    fs::create_dir_all(&dir).expect("stub の置き場を作れる");
    let mut cases = String::new();
    for (at, body) in turns.iter().enumerate() {
        cases.push_str(&format!("{})\n{body}\n;;\n", at + 1));
    }
    let body = format!(
        "#!/bin/sh\nD='{}'\nprintf 'call\\n' >> \"$D/calls\"\nN=$(wc -l < \"$D/calls\" | tr -d ' ')\n\
         printf '%s\\n' \"$@\" > \"$D/argv-$N\"\ncat > \"$D/stdin-$N\"\ncase \"$N\" in\n{cases}*) exit 1 ;;\nesac\n",
        dir.display()
    );
    let path = state.join("turn-runner.sh");
    fs::write(&path, body).expect("stub を書ける");
    format!("sh {}", path.display())
}

/// n turn 目（1 始まり）に渡された argv（1 行 1 引数・無ければ空）。
pub(super) fn stub_argv(state: &Path, turn: usize) -> Vec<String> {
    fs::read_to_string(stub_dir(state).join(format!("argv-{turn}")))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// 遠い未来の reset（どの「いま」でも古くない）。
const FAR_RESET: &str = "2099-01-01T05:00:00Z";

/// 遠い未来の 7 日窓の reset。
const FAR_WEEK_RESET: &str = "2099-01-07T00:00:00Z";

/// 口座 1 つの窓の fixture（5 時間窓の使用率・7 日窓の使用率・reset・モデル別 7 日窓）。reset は `None` なら遠い未来
/// （[`FAR_RESET`] / [`FAR_WEEK_RESET`]）、`Some(secs)` なら**偽 curl が呼ばれた瞬間**から `secs` 秒後を
/// **全窓に**置く（[`fake_usage_curl`] が応答の直前に `date -u` で作って埋める＝process の起動遅れに依らず
/// 計測時点で未来。窓を揃えるのは、5 時間窓だけが古くなる周に 7 日窓の実測で計測なしに選ばれないため）。
/// `models` は `limits[]` の `weekly_scoped` 要素（usage API の**表示名**・`Opus` / `Fable` …・使用率）。
#[derive(Debug, Clone)]
pub(super) struct Windows {
    five: u64,
    seven: u64,
    reset_in: Option<u64>,
    models: Vec<(&'static str, u64)>,
}

/// 5 時間窓が `five`%・7 日窓が `seven`%（reset はどちらも遠い未来・モデル別の行なし）の fixture。
pub(super) fn windows(five: u64, seven: u64) -> Windows {
    Windows { five, seven, reset_in: None, models: Vec::new() }
}

/// 5 時間窓が当たっている（100%・7 日窓は 10%）口座で、reset は偽 curl の呼出しから `secs` 秒後。
pub(super) fn limited_for(secs: u64) -> Windows {
    Windows { five: 100, seven: 10, reset_in: Some(secs), models: Vec::new() }
}

/// `found` にモデル別 7 日窓（表示名 `model`・`pct`%）を 1 行足す。
fn with_model(found: Windows, model: &'static str, pct: u64) -> Windows {
    let mut models = found.models;
    models.push((model, pct));
    Windows { models, ..found }
}

/// `found` の reset を偽 curl の呼出しから `secs` 秒後にする（全窓）。
fn resetting_in(found: Windows, secs: u64) -> Windows {
    Windows { reset_in: Some(secs), ..found }
}

/// 偽 curl が呼出時刻から相対で埋める reset の穴（`@RESET+<secs>@`・穴はこの 1 種だけ）。
fn reset_hole(secs: u64) -> String {
    format!("@RESET+{secs}@")
}

/// 口座残量の応答の本文（`fleet usage` が読む形・モデル別の行は `models` の順に `limits[]` へ）。
fn usage_body(found: &Windows) -> String {
    let (five_reset, week_reset) = match found.reset_in {
        Some(secs) => (reset_hole(secs), reset_hole(secs)),
        None => (FAR_RESET.to_owned(), FAR_WEEK_RESET.to_owned()),
    };
    let limits: Vec<String> = found
        .models
        .iter()
        .map(|(model, pct)| {
            format!(
                r#"{{"kind":"weekly_scoped","percent":{pct},"resets_at":"{week_reset}","scope":{{"model":{{"display_name":"{model}"}}}}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"five_hour":{{"utilization":{},"resets_at":"{five_reset}"}},"seven_day":{{"utilization":{},"resets_at":"{week_reset}"}},"limits":[{}]}}"#,
        found.five,
        found.seven,
        limits.join(",")
    )
}

/// 便が使う model の rules 行 `runner.model` の値（本番と同じ claude CLI の**別名**・埋め込みの行と同じ・`s2-07l.297`）。
/// 実測行の表示名（`Opus`）とは**字面が違う**＝歯は 2 つの語彙の組で本番の照合を測る（揃えて隠さない）。
const RUNNER_MODEL: &str = "opus";

/// `pipe resume` / `pipe run` に渡す manifest: [`write_rules`] の写しに計測の待ち時間の行・便が使う model の行
/// （値は [`RUNNER_MODEL`]）と `[[account]]` を `labels` の順で足したもの。
pub(super) fn resume_rules(state: &Path, labels: &[&str]) -> String {
    resume_rules_with_model(state, labels, RUNNER_MODEL)
}

/// [`resume_rules`] の `runner.model` の値を `model` にした形（未知の値の歯だけが振る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn resume_rules_with_model(state: &Path, labels: &[&str], model: &str) -> String {
    let path = write_rules(state, "rules-resume.toml", 1, 1_000_000);
    let mut body = fs::read_to_string(&path).expect("写しを読める");
    body.push_str(
        "\n[[rule]]\nid = \"fleet.usage_timeout_s\"\nkind = \"UsageTimeoutS\"\nvalue = 13\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n",
    );
    body.push_str(&format!(
        "\n[[rule]]\nid = \"runner.model\"\nkind = \"RunnerModel\"\nvalue = \"{model}\"\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n"
    ));
    for label in labels {
        body.push_str(&format!("\n[[account]]\nlabel = \"{label}\"\n"));
    }
    fs::write(&path, body).expect("manifest を書ける");
    path.display().to_string()
}

/// 口座 label の credential（`<state>/accounts/<label>/.credentials.json`・token は `tok-<label>`）と、偽 curl が
/// その token に返す本文（`rounds` の n 番目は n 回目の呼出しの本文・尽きたら最後の本文のまま）を置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn put_account(state: &Path, label: &str, rounds: &[Windows]) {
    let dir = state.join("accounts").join(label);
    fs::create_dir_all(&dir).expect("credential の dir を作れる");
    let credential = format!(
        r#"{{"claudeAiOauth":{{"accessToken":"tok-{label}","refreshToken":"r","expiresAt":4102444800000}}}}"#
    );
    fs::write(dir.join(".credentials.json"), credential).expect("credential を書ける");
    let spy = curl_spy(state);
    fs::create_dir_all(&spy).expect("偽 curl の置き場を作れる");
    fs::write(spy.join(format!("token-tok-{label}")), "0").expect("呼出回数を置ける");
    for (at, found) in rounds.iter().enumerate() {
        fs::write(spy.join(format!("body-tok-{label}-{}", at + 1)), usage_body(found)).expect("本文を書ける");
    }
}

/// 偽 curl の置き場（呼出回数・token ごとの本文・argv の写し）。
fn curl_spy(state: &Path) -> PathBuf {
    state.join("curl-spy")
}

/// 偽 curl: stdin の設定行の token で口座を選び、その token の n 回目の呼出しに `body-<token>-<n>`（無ければ
/// 最後の本文）を返す。本文の `@RESET+<secs>@`（[`reset_hole`]）は**呼ばれた瞬間**の `date -u` から
/// `secs` 秒後の ts で埋め、その ts を `reset-<token>-<n>` に写す（歯は [`spy_reset`] で読む）。argv は
/// `curl-args` へ追記で写す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn fake_usage_curl(state: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let spy = curl_spy(state);
    fs::create_dir_all(&spy).expect("偽 curl の置き場を作れる");
    let script = format!(
        "#!/bin/sh\n\
         D='{}'\n\
         printf '%s\\n' \"$@\" >> \"$D/curl-args\"\n\
         cfg=$(cat)\n\
         for f in \"$D\"/token-*; do\n\
         tok=${{f##*/token-}}\n\
         case \"$cfg\" in *\"Bearer $tok\\\"\"*)\n\
         n=$(cat \"$f\"); n=$((n+1)); printf '%s' \"$n\" > \"$f\"\n\
         while [ ! -f \"$D/body-$tok-$n\" ] && [ \"$n\" -gt 1 ]; do n=$((n-1)); done\n\
         body=$(cat \"$D/body-$tok-$n\")\n\
         case \"$body\" in *@RESET+*)\n\
         secs=${{body#*@RESET+}}; secs=${{secs%%@*}}\n\
         at=$(date -u -d \"@$(( $(date -u +%s) + secs ))\" +%Y-%m-%dT%H:%M:%SZ)\n\
         printf '%s' \"$at\" > \"$D/reset-$tok-$n\"\n\
         while :; do case \"$body\" in *\"@RESET+$secs@\"*) body=\"${{body%%@RESET+$secs@*}}$at${{body#*@RESET+$secs@}}\" ;; *) break ;; esac; done\n\
         ;; esac\n\
         printf '%s' \"$body\"\n\
         ;; esac\n\
         done\n\
         printf '\\n%s' '200'\n\
         exit 0\n",
        spy.display()
    );
    let path = spy.join("fake-curl");
    fs::write(&path, script).expect("偽 curl を書ける");
    let mut perm = fs::metadata(&path).expect("偽 curl の権限を読める").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("偽 curl を実行可能にできる");
    path.display().to_string()
}

/// 偽 curl が口座 `label` の `round` 回目（1 始まり）の応答に埋めた相対 reset の ts（埋めていなければ空）。
pub(super) fn spy_reset(state: &Path, label: &str, round: usize) -> String {
    fs::read_to_string(curl_spy(state).join(format!("reset-tok-{label}-{round}"))).unwrap_or_default()
}

/// 偽 curl が呼ばれた回数（口座 1 つの計測につき 1 回）。
pub(super) fn curl_calls(state: &Path) -> usize {
    fs::read_to_string(curl_spy(state).join("curl-args"))
        .unwrap_or_default()
        .lines()
        .filter(|arg| *arg == "--max-time")
        .count()
}

/// `resume` を manifest・偽 curl・runner つきで 1 回撃つ。
fn resume_with_accounts(repo: &Path, state: &Path, id: &str, runner: &str, rules: &str) -> Output {
    run_pipe(&[
        "resume", "--run", id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", runner,
        "--rules", rules, "--curl", &fake_usage_curl(state),
    ])
}

/// 便の `Spawned` の detail の列（物理順）。
pub(super) fn spawned_details(state: &Path, id: &str) -> Vec<String> {
    stages(state, id)
        .into_iter()
        .filter(|(stage, _)| *stage == Some(Stage::Spawned))
        .filter_map(|(_, detail)| detail)
        .collect()
}

/// 便の `RunStage(RateLimited)` の detail の列（物理順）。
fn rate_limited_details(state: &Path, id: &str) -> Vec<String> {
    stages(state, id)
        .into_iter()
        .filter(|(stage, _)| *stage == Some(Stage::RateLimited))
        .filter_map(|(_, detail)| detail)
        .collect()
}

/// `argv` に `--account-dir <dir>` の 2 引数が並ぶか。
pub(super) fn argv_account_dir(argv: &[String]) -> Option<String> {
    argv.windows(2)
        .find(|pair| pair.first().is_some_and(|flag| flag == "--account-dir"))
        .and_then(|pair| pair.get(1).cloned())
}

/// 上限で止まった便（turn 1 = 上限・turn 2 以降 = `rest`）と、その置き場の口座 fixture（`labels` の manifest）を
/// 組む。返すのは `(run id, runner cmd, manifest の path)`。
fn rate_limited_with_accounts(repo: &Path, state: &Path, rest: &[String], labels: &[&str]) -> (String, String, String) {
    let first = write_set_contract(repo, "first", &["src/lib.rs"]);
    let id = intake_bead(repo, state, &first, "s2-limit");
    let mut turns = vec![limit_turn()];
    turns.extend(rest.iter().cloned());
    let runner = turn_runner(state, &turns);
    let out = spawn_with(repo, state, &id, &runner);
    assert!(stdout_of(&out).contains("stage=RateLimited"), "{} / {}", stdout_of(&out), stderr_of(&out));
    let rules = resume_rules(state, labels);
    (id, runner, rules)
}

/// 便を gate（PASS の偽 lens）→ land で Landed まで通し、人由来の event が 0 であることを測る。
pub(super) fn assert_lands_without_human(repo: &Path, state: &Path, id: &str) {
    let marker = state.join("lens-ran");
    let gated = gate_once(repo, state, id, Some(&fake_lens(&marker, &lens_verdict("PASS"))));
    assert_eq!(gated.status.code(), Some(i32::from(RC_OK)), "gate: {}", stderr_of(&gated));
    let landed = land_once(repo, state, id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    assert!(show_line(repo, state, id).contains("stage=Landed"), "Landed まで通る");
    let report = report_once(state);
    assert_eq!(
        stdout_of(&report).trim(),
        format!("runs=1 landed=1 human_events=0 human_events_other_than_approval=0 {}", spawn::NO_REVIEW_FAIL),
        "人由来の event は 0（FR22）"
    );
}

/// n turn 目の stdin の中で、契約 → 回答（在れば）→ 途中再開 の順に節が並ぶか。
fn sections_in_order(prompt: &str, with_answer: bool) -> bool {
    let contract_at = prompt.find("goal = ");
    let resume_at = prompt.find("## 途中再開");
    let answer_at = if with_answer { prompt.find("## 回答") } else { contract_at };
    matches!((contract_at, answer_at, resume_at), (Some(c), Some(a), Some(r)) if c <= a && a < r)
}

/// 席の登録 row を 1 件置く（`seat register` は打刻を要るので、便の歯は行を直に積む・読み手は replay）。
///
/// `anchor` は row の anchor（便用の除外はその repo の席だけ・設計 account-autonomy.md §14）: 「席の口座は便から
/// 外れる」を測る歯は**便の repo の path** を渡す（別の anchor を置くと席の口座は候補に入る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn register_seat_account(state: &Path, anchor: &Path, account: &str) {
    let event = Event {
        schema: vessel::fleet::SCHEMA,
        ts: vessel::fleet::cli::now_utc(),
        kind: EventKind::SeatRegistered,
        run: String::new(),
        bead: String::new(),
        host: vessel::fleet::cli::host(),
        actor: vessel::fleet::ACTOR_MACHINE.to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: None,
        allowance: None,
        mark: None,
        registration: Some(vessel::fleet::Registration {
            role: vessel::seat::role::Role::Orchestrator,
            anchor: anchor.display().to_string(),
            target: "rs:planner".to_owned(),
            sid: Some("sid-a".to_owned()),
            account: account.to_owned(),
            launch: "launch {credential-dir}\n".to_owned(),
            model: None,
        }),
        account: None,
    };
    let policy = vessel::fleet::store::LockPolicy::embedded().expect("埋め込みの lock 行を読める");
    vessel::fleet::store::append(state, &event, policy).expect("登録 row を積める");
}

/// (1)(5)(6)(7) 上限で止まった便は器が選んだ別口座で起こし直され Landed まで通る（AC12 の T）: 余裕の
/// label の `--account-dir` が runner の argv に渡り・`Spawned detail=account:<label>,resume:rate-limit` が
/// 記帳され・段の detail に `account:` が載り・2 度目の prompt に「途中再開」節が契約の後に在り・人由来の
/// event は 0。
#[test]
fn pipe_ratelimit_resume_respawns_on_the_free_account_and_lands() {
    let (repo, state) = repo_with_state();
    let (id, runner, rules) = rate_limited_with_accounts(&repo, &state, &[IMPLEMENT.to_owned()], &["a1", "a2"]);
    assert_eq!(
        rate_limited_details(&state, &id),
        vec!["rc:75,status:allowed_warning,account:inherited".to_owned()],
        "口座を渡していない turn の detail"
    );
    put_account(&state, "a1", &[windows(100, 10)]);
    put_account(&state, "a2", &[windows(40, 10)]);
    let resumed = resume_with_accounts(&repo, &state, &id, &runner, &rules);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    assert_eq!(
        stdout_of(&resumed).trim(),
        format!("run={id} next=spawn account=a2\nrun={id} stage=Implemented"),
        "判定行は next=spawn account=<label>・続けて turn の結果"
    );
    assert_eq!(curl_calls(&state), 2, "選定の直前に FR33 の計測を 1 回（口座 2 つ）");
    assert_eq!(stub_calls(&state), 2, "runner を 1 回起こし直した");
    assert_eq!(argv_account_dir(&stub_argv(&state, 1)), None, "初回の turn は口座を渡さない");
    assert_eq!(
        argv_account_dir(&stub_argv(&state, 2)),
        Some(state.join("accounts").join("a2").display().to_string()),
        "起こし直しは余裕の label の credential dir を渡す: {:?}",
        stub_argv(&state, 2)
    );
    let details = spawned_details(&state, &id);
    assert!(
        details.len() == 2 && details.first().is_some_and(|found| found.starts_with("base:")),
        "初回は base を名乗る: {details:?}"
    );
    assert_eq!(details.get(1).map(String::as_str), Some("account:a2,resume:rate-limit"), "起こし直しの記帳: {details:?}");
    let second = stub_stdin(&state, 2);
    assert!(sections_in_order(&second, false), "2 度目の prompt に「途中再開」節が契約の後に在る: {second}");
    assert!(second.contains("base からの commit（worktree に在る・やり直さない）: なし"), "commit の無い便は なし: {second}");
    assert!(!stub_stdin(&state, 1).contains("## 途中再開"), "初回の turn には節が無い");
    // 続きの段（gate → land）はそのまま通る。
    assert_lands_without_human(&repo, &state, &id);
    clean(&[&repo, &state]);
}

/// (2) 全口座が当たっている周は最も早い reset まで唯一の wait で待ち（`next=wait reset=<ts>`）、reset を
/// 過ぎて `Timeout` を受けた周は計測から撃ち直して（偽 curl の 2 回目は余裕）起こし直す。
///
/// reset は test の開始時刻でなく**偽 curl が呼ばれた瞬間**から相対で作る（[`limited_for`]）: 固定の
/// 壁時計だと負荷で `pipe resume` の起動と 1 回目の計測が 2 秒を超えた周に reset が既に過ぎていて、器は
/// 正しく待たずに選ぶ＝計測 2 回で落ちる（s2-07l.134 run 161621Z の main-red）。
// flip-check: retroactive s2-07l.219
#[test]
fn pipe_ratelimit_resume_waits_for_the_earliest_reset_then_remeasures() {
    let (repo, state) = repo_with_state();
    let (id, runner, rules) = rate_limited_with_accounts(&repo, &state, &[IMPLEMENT.to_owned()], &["a1", "a2"]);
    // a1 は計測の 2 秒後に開き直る（最も早い reset）・a2 は 4 秒後。2 回目の計測では a1 に余裕が戻る。
    put_account(&state, "a1", &[limited_for(2), windows(50, 10)]);
    put_account(&state, "a2", &[limited_for(4), limited_for(4)]);
    let started = Instant::now();
    let resumed = resume_with_accounts(&repo, &state, &id, &runner, &rules);
    let waited = started.elapsed();
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    let stdout = stdout_of(&resumed);
    let soon = spy_reset(&state, "a1", 1);
    assert!(vessel::fleet::epoch_of(&soon).is_some(), "偽 curl が a1 の 1 回目に相対 reset を埋めた: {soon:?}");
    assert!(stdout.contains(&format!("run={id} next=wait reset={soon}")), "最も早い reset を名乗って待つ: {stdout}");
    assert!(stdout.contains(&format!("run={id} next=spawn account=a1")), "reset の後の計測で a1 を選ぶ: {stdout}");
    assert!(stdout.contains(&format!("run={id} stage=Implemented")), "{stdout}");
    assert!(waited >= Duration::from_secs(1), "reset まで待った（{waited:?}）");
    assert_eq!(curl_calls(&state), 4, "Timeout の後に計測を撃ち直す（口座 2 つ × 2 回）");
    assert_eq!(stub_calls(&state), 2, "起こし直しは 1 回");
    assert_eq!(
        argv_account_dir(&stub_argv(&state, 2)),
        Some(state.join("accounts").join("a1").display().to_string()),
        "{:?}",
        stub_argv(&state, 2)
    );
    clean(&[&repo, &state]);
}

/// (3) 登録 row の口座は候補から外れる: その口座だけが余裕でも `NoCandidate`（理由 = 除外で空）で、待つ
/// reset が無いので起こさず rc 3 で止まる（便は `RateLimited` のまま live）。
#[test]
fn pipe_ratelimit_resume_excludes_the_registered_seat_account() {
    let (repo, state) = repo_with_state();
    let (id, runner, rules) = rate_limited_with_accounts(&repo, &state, &[IMPLEMENT.to_owned()], &["a2"]);
    put_account(&state, "a2", &[windows(40, 10)]);
    register_seat_account(&state, &repo, "a2");
    let resumed = resume_with_accounts(&repo, &state, &id, &runner, &rules);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_BLOCKED)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    assert!(stdout_of(&resumed).contains(&format!("run={id} next=wait reset=-")), "{}", stdout_of(&resumed));
    assert!(stderr_of(&resumed).contains("候補なし: excluded"), "理由は除外で空: {}", stderr_of(&resumed));
    assert_eq!(curl_calls(&state), 1, "計測は撃つ（除外は選定の入力）");
    assert_eq!(stub_calls(&state), 1, "席の口座では起こさない");
    assert!(show_line(&repo, &state, &id).contains("stage=RateLimited"), "便は RateLimited のまま");
    assert_eq!(spawned_details(&state, &id).len(), 1, "起こし直しの記帳は無い");
    clean(&[&repo, &state]);
}

/// (4) 待ちの途中の便を `pipe stop --run` が止める: `RunStopped` が書かれ、待ちから抜けて起こさない。
///
/// 待ちの判定行（`next=wait reset=…`）の有無は見ない: 判定行は process の終了時にだけ stdout へ出て、待ちに
/// 入ったことを走行中に観測する線が無いので、stop が待ちの前に着いた周（負荷下で選定が伸びる）は判定行を
/// 書かずに `Stopped` で断る＝順序に依る。壁時計で「待ちに入った」を仮定する sleep は置かない
/// （s2-07l.329 run 1 の baseline で 1/2 落ちた）。判定行は
/// [`pipe_ratelimit_resume_waits_for_the_earliest_reset_then_remeasures`] が担保する。
// flip-check: retroactive s2-07l.334
#[test]
fn pipe_ratelimit_resume_stop_breaks_the_wait() {
    let (repo, state) = repo_with_state();
    let (id, runner, rules) = rate_limited_with_accounts(&repo, &state, &[IMPLEMENT.to_owned()], &["a1", "a2"]);
    put_account(&state, "a1", &[windows(100, 10)]);
    put_account(&state, "a2", &[windows(100, 10)]);
    let curl = fake_usage_curl(&state);
    // 起動は `pipe_cmd`（口 (i)・設計 gate-cost.md §30）で組む——道具箱の PATH は argv の置き場から来る。
    // flip-check: retroactive s2-07l.504
    let mut child = pipe_cmd(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
        "--rules", &rules, "--curl", &curl,
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("resume を背景で起こせる");
    // 計測が終わる（口座 2 つの行が置き場に載る）まで待ってから止める。stop は待ちの前に着いても後に着いても
    // 便を Stopped で断る（以下の assert はどちらの順序でも成り立つ）。
    let deadline = Instant::now() + Duration::from_secs(20);
    while curl_calls(&state) < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(curl_calls(&state), 2, "計測が撃たれた");
    assert!(child.try_wait().expect("状態を読める").is_none(), "止める前は process が生きている（reset は 2099 年）");
    // resume は札を握る運転手である＝stop がその process を止める（設計 pipeline.md §39 (2)）。resume は test の子なので、
    // stop は別 thread で撃ち、この thread が止められた子を回収する（回収しないと zombie が残り stop は止め切れない）。
    let dir = state.display().to_string();
    let target = id.clone();
    let stopper = std::thread::spawn(move || run_pipe(&["stop", "--run", &target, "--state-dir", &dir]));
    let finished = child.wait_with_output().expect("resume の終了を待てる");
    let stopped = stopper.join().expect("stop の thread を待てる");
    let stdout = String::from_utf8_lossy(&finished.stdout);
    let stderr = String::from_utf8_lossy(&finished.stderr);
    assert_eq!(stopped.status.code(), Some(i32::from(RC_OK)), "stop --run: {}", stderr_of(&stopped));
    assert_eq!(finished.status.code(), None, "止められた終了（自力の終了 code を持たない）: {stdout} / {stderr}");
    let driver_rows = events(&state)
        .into_iter()
        .filter(|found| found.run == id && found.kind == EventKind::SeatStopped && found.seat.as_deref() == Some("driver"))
        .count();
    assert_eq!(driver_rows, 1, "seat=driver の記録が 1 行");
    assert_eq!(stub_calls(&state), 1, "止めた後に起こさない");
    assert!(show_line(&repo, &state, &id).contains("stage=Stopped"), "便は Stopped で終端");
    assert_eq!(kind_count(&state, &id, EventKind::RunStopped), 1, "RunStopped が 1 件");
    clean(&[&repo, &state]);
}

/// (5) 回答済みの質問を持つ便の途中再開: prompt は 契約 → 回答 → 途中再開 の順で、節は base からの
/// commit の一覧と止まった時刻（`RateLimited` の event の ts）を運ぶ。
#[test]
fn pipe_ratelimit_resume_prompt_lists_commits_after_the_answer() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-limit");
    let runner = turn_runner(&state, &[ask_turn(), commit_then_limit_turn(), IMPLEMENT.to_owned()]);
    let asked = spawn_with(&repo, &state, &id, &runner);
    assert_eq!(asked.status.code(), Some(i32::from(RC_BLOCKED)), "質問で止まる: {}", stderr_of(&asked));
    let answered = run_pipe(&[
        "answer", "--run", &id, "--words", "verify は 1 行目だけを撃つ",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(answered.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&answered));
    let second = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &runner,
    ]);
    assert_eq!(second.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&second));
    assert!(stdout_of(&second).contains("stage=RateLimited"), "commit を作ってから上限で止まる: {}", stdout_of(&second));
    let stopped_at = events(&state)
        .into_iter()
        .filter(|event| event.run == id && event.stage == Some(Stage::RateLimited))
        .map(|event| event.ts)
        .next_back()
        .unwrap_or_default();
    let rules = resume_rules(&state, &["a1"]);
    put_account(&state, "a1", &[windows(30, 30)]);
    let third = resume_with_accounts(&repo, &state, &id, &runner, &rules);
    assert_eq!(third.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&third), stderr_of(&third));
    let prompt = stub_stdin(&state, 3);
    assert!(sections_in_order(&prompt, true), "順序は 契約 → 回答 → 途中再開: {prompt}");
    let stopped_line = format!("- 前の turn は {stopped_at} に口座の上限で止まった");
    assert!(!stopped_at.is_empty() && prompt.contains(&stopped_line), "止まった時刻（log の ts）: {prompt}");
    assert!(prompt.contains("partial-work") && !prompt.contains("## 追随"), "base からの commit の一覧・追随は無い: {prompt}");
    assert!(!stub_stdin(&state, 2).contains("## 途中再開"), "上限で止まる前の turn には節が無い");
    assert_eq!(git(&worktree_of(&repo, &id), &["rev-list", "--count", "refs/heads/main..HEAD"]), "2", "途中の commit を保ったまま続きを積む");
    clean(&[&repo, &state]);
}

/// 起こし直した turn がまた上限で止まった周も回数の上限なく続く（`pipe run` の 1 process で
/// intake → 器が選んだ口座で初回 → 上限（detail に `account:<label>`）→ 別口座 → 上限 → 別の口座 → gate → land）。
///
/// 初回の起動も同じ選定を通る（`s2-07l.285`・設計 account-autonomy.md §4）ので、1 回目の計測は初回の起動の前で、
/// 上限で止まった段の detail は初回から `account:<label>`（`inherited` は宣言 0 の周だけ）。初回は判定行
/// （`next=spawn account=`）を持たない（名乗るのは再開だけ）。
#[test]
fn pipe_ratelimit_resume_run_rides_out_repeated_limits_without_a_cap() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let runner = turn_runner(&state, &[limit_turn(), limit_turn(), IMPLEMENT.to_owned()]);
    let rules = resume_rules(&state, &["a1", "a2"]);
    // 1 回目の計測（初回の起動の前）: a1 は当たっている・a2 に余裕。2 回目以降: a2 が当たり・a1 に余裕が戻る。
    put_account(&state, "a1", &[windows(100, 10), windows(60, 10)]);
    put_account(&state, "a2", &[windows(40, 10), windows(100, 10)]);
    let marker = state.join("lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let out = run_pipe(&[
        "run", "--design", &first, "--bead", "s2-limit",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules, "--curl", &fake_usage_curl(&state), "--runner", &runner, "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&out), stderr_of(&out));
    let id = run_id_of(&out);
    let stdout = stdout_of(&out);
    assert!(!stdout.contains("next=spawn account=a2"), "初回（a2）は判定行を持たない: {stdout}");
    assert_eq!(stdout.matches("next=spawn account=a1").count(), 2, "再開 2 回はどちらも a1 を名乗る: {stdout}");
    assert_eq!(
        rate_limited_details(&state, &id),
        vec![
            "rc:75,status:allowed_warning,account:a2".to_owned(),
            "rc:75,status:allowed_warning,account:a1".to_owned(),
        ],
        "上限の段の detail は起こした口座を運ぶ（初回から器が選ぶ）"
    );
    let details = spawned_details(&state, &id);
    assert_eq!(details.len(), 3, "{details:?}");
    assert!(
        details[0].starts_with("base:") && details[0].ends_with(",account:a2"),
        "初回は base と選んだ口座を名乗る: {details:?}"
    );
    assert_eq!(details[1], "account:a1,resume:rate-limit");
    assert_eq!(details[2], "account:a1,resume:rate-limit");
    assert_eq!(stub_calls(&state), 3);
    assert_eq!(
        argv_account_dir(&stub_argv(&state, 1)),
        Some(state.join("accounts").join("a2").display().to_string()),
        "初回の turn から選んだ口座の credential dir を渡す: {:?}",
        stub_argv(&state, 1)
    );
    // 初回 + 起こし直し 2 回 + gate の lens の前 1 回（`s2-07l.412`・設計 account-autonomy.md §15）。
    assert_eq!(curl_calls(&state), 8, "初回の起動と起こし直しのたび、および lens の前に計測（2 口座 × 4 回）");
    assert!(show_line(&repo, &state, &id).contains("stage=Landed"), "1 process で Landed まで: {stdout}");
    assert!(!events(&state).iter().any(|event| event.actor == "human"), "人由来の event は 0");
    clean(&[&repo, &state]);
}

// ───── 便用の選定は便が使う model の窓だけを数える（`s2-07l.297`・設計 account-autonomy.md §3・SRS FR36 / FR33 / FR5） ─────

/// (d) 候補の口座が **Fable の窓 100 / 7 日 50 / 5 時間 0** でも、便が使う model（rules 行 `runner.model` = `opus`）の
/// 窓ではないので候補に残り、再開は待たずに `Chosen(a1)` で runner を起こす（`next=spawn account=a1`・計測は 1 回・
/// `next=wait` は出ない）。
///
/// base は model を渡さず全 model 窓の最大（Fable の 100）で `all-limited` に倒れ、reset（5 秒後）まで待ってから
/// 2 回目の計測（Fable の行なし）で選ぶ＝`next=wait` が出て計測が 2 回になる → RED。2 回目の本文を置くのは
/// base で歯が永久に待たないため（RED を timeout でなく assert で測る）。
#[test]
fn pipe_ratelimit_resume_counts_only_the_runner_model_window() {
    let (repo, state) = repo_with_state();
    let (id, runner, rules) = rate_limited_with_accounts(&repo, &state, &[IMPLEMENT.to_owned()], &["a1"]);
    put_account(&state, "a1", &[resetting_in(with_model(windows(0, 50), "Fable", 100), 5), windows(0, 50)]);
    let resumed = resume_with_accounts(&repo, &state, &id, &runner, &rules);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    let stdout = stdout_of(&resumed);
    assert_eq!(
        stdout.trim(),
        format!("run={id} next=spawn account=a1\nrun={id} stage=Implemented"),
        "Fable の窓 100 は便に無関係＝待たずに a1 を選ぶ: {stdout}"
    );
    assert!(!stdout.contains("next=wait"), "待ちに入らない: {stdout}");
    assert_eq!(curl_calls(&state), 1, "計測は 1 回（待ちからの撃ち直しが無い）");
    assert_eq!(stub_calls(&state), 2, "runner を 1 回起こし直した");
    assert_eq!(
        argv_account_dir(&stub_argv(&state, 2)),
        Some(state.join("accounts").join("a1").display().to_string()),
        "{:?}",
        stub_argv(&state, 2)
    );
    let stderr = stderr_of(&resumed);
    assert!(stderr.contains("model=Fable:100%"), "計測の行は Fable の窓を写している（数えないだけ）: {stderr}");
    clean(&[&repo, &state]);
}

/// (e) 負例（極性不変・base でも PASS）: 候補の口座の **Opus の窓 100**（実測行の表示名 `Opus`・rules 行は本番と同じ
/// 別名 `opus`＝字面を揃えて隠さない）は便が使う model の窓なので候補から外れ、余裕の別口座 a2 が選ばれる。
#[test]
fn pipe_ratelimit_resume_still_skips_the_runner_model_window_at_100() {
    let (repo, state) = repo_with_state();
    let (id, runner, rules) = rate_limited_with_accounts(&repo, &state, &[IMPLEMENT.to_owned()], &["a1", "a2"]);
    // a1 は 5h / 7d に余裕（逼迫度なら a1 = 50 > a2 = 40 で a1 が勝つ）が Opus の窓が当たっている。
    put_account(&state, "a1", &[with_model(windows(0, 50), "Opus", 100)]);
    put_account(&state, "a2", &[windows(40, 10)]);
    let resumed = resume_with_accounts(&repo, &state, &id, &runner, &rules);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    assert_eq!(
        stdout_of(&resumed).trim(),
        format!("run={id} next=spawn account=a2\nrun={id} stage=Implemented"),
        "Opus の窓 100 の a1 は外れ・a2 を選ぶ"
    );
    assert_eq!(stub_calls(&state), 2, "runner を 1 回起こし直した");
    assert_eq!(
        argv_account_dir(&stub_argv(&state, 2)),
        Some(state.join("accounts").join("a2").display().to_string()),
        "{:?}",
        stub_argv(&state, 2)
    );
    let stderr = stderr_of(&resumed);
    assert!(stderr.contains("model=Opus:100%"), "計測の行は Opus の窓を写している: {stderr}");
    clean(&[&repo, &state]);
}

/// (e') rules 行 `runner.model` の値が閉じた表に無い（`nope`）manifest での再開は **typed に断り**（rc 1・理由に行 id と
/// 値を名指す）、計測も runner の起こし直しもしない（便は `RateLimited` のまま live）。base は字面比較で黙って全 model
/// 窓の最大に倒れる（断らない）→ RED。
#[test]
fn pipe_ratelimit_refuses_unknown_runner_model_value() {
    let (repo, state) = repo_with_state();
    let (id, runner, _) = rate_limited_with_accounts(&repo, &state, &[IMPLEMENT.to_owned()], &["a1"]);
    let rules = resume_rules_with_model(&state, &["a1"], "nope");
    put_account(&state, "a1", &[windows(40, 10)]);
    let resumed = resume_with_accounts(&repo, &state, &id, &runner, &rules);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_REFUSED)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    let stderr = stderr_of(&resumed);
    assert!(stderr.contains("runner.model の値 nope は未知の model"), "行 id と値を名指す: {stderr}");
    assert!(stdout_of(&resumed).is_empty(), "判定行を出さない: {}", stdout_of(&resumed));
    assert_eq!(curl_calls(&state), 0, "計測しない");
    assert_eq!(stub_calls(&state), 1, "runner を起こし直さない");
    assert!(show_line(&repo, &state, &id).contains("stage=RateLimited"), "便は RateLimited のまま live");
    clean(&[&repo, &state]);
}

// ───── 別口座での途中再開は host の面の口座も候補にする（`s2-07l.246`・ADR-0026 §2.1・SRS FR57 / FR37 / FR36・接頭辞 `pipe_ratelimit_host_`） ─────

/// 置き場の host の面（`<state>/host.toml`）に `labels` の順で `[[account]]` を宣言する。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_host_accounts(state: &Path, labels: &[&str]) {
    let accounts: String = labels.iter().map(|label| format!("\n[[account]]\nlabel = \"{label}\"\n")).collect();
    fs::write(state.join(vessel::rules::HOST_MANIFEST), format!("schema = 1\n{accounts}")).expect("host の面を書ける");
}

/// tracked の面（`--rules`）が 0 口座・host の面が 2 口座の置き場で、上限で止まった便は host の面の余裕の口座で
/// 起こし直される（runner の argv に `--account-dir <state>/accounts/<label>`・記帳は `account:<label>,resume:rate-limit`）。
#[test]
fn pipe_ratelimit_host_resume_respawns_on_a_host_manifest_account() {
    let (repo, state) = repo_with_state();
    let (id, runner, rules) = rate_limited_with_accounts(&repo, &state, &[IMPLEMENT.to_owned()], &[]);
    put_host_accounts(&state, &["h1", "h2"]);
    put_account(&state, "h1", &[windows(100, 10)]);
    put_account(&state, "h2", &[windows(40, 10)]);
    let resumed = resume_with_accounts(&repo, &state, &id, &runner, &rules);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_OK)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    assert_eq!(
        stdout_of(&resumed).trim(),
        format!("run={id} next=spawn account=h2\nrun={id} stage=Implemented"),
        "host の面の余裕の口座を選ぶ"
    );
    assert_eq!(curl_calls(&state), 2, "計測も host の面の 2 口座を撃つ");
    assert_eq!(stub_calls(&state), 2, "runner を 1 回起こし直した");
    assert_eq!(
        argv_account_dir(&stub_argv(&state, 2)),
        Some(state.join("accounts").join("h2").display().to_string()),
        "起こし直しは host の面の label の credential dir を渡す: {:?}",
        stub_argv(&state, 2)
    );
    assert_eq!(
        spawned_details(&state, &id).get(1).map(String::as_str),
        Some("account:h2,resume:rate-limit"),
        "起こし直しの記帳"
    );
    clean(&[&repo, &state]);
}

/// tracked の面が 0 口座で host の面も無い置き場では候補が無い: 起こさず・待つ reset も無いので rc 3 で止まり
/// （`next=wait reset=-`）、便は `RateLimited` のまま live。
#[test]
fn pipe_ratelimit_host_resume_without_host_manifest_has_no_candidate() {
    let (repo, state) = repo_with_state();
    let (id, runner, rules) = rate_limited_with_accounts(&repo, &state, &[IMPLEMENT.to_owned()], &[]);
    put_account(&state, "h1", &[windows(40, 10)]);
    assert!(!state.join(vessel::rules::HOST_MANIFEST).exists(), "host の面は置かない");
    let resumed = resume_with_accounts(&repo, &state, &id, &runner, &rules);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_BLOCKED)), "{} / {}", stdout_of(&resumed), stderr_of(&resumed));
    assert_eq!(stdout_of(&resumed).trim(), format!("run={id} next=wait reset=-"), "{}", stdout_of(&resumed));
    assert!(stderr_of(&resumed).contains("候補なし"), "{}", stderr_of(&resumed));
    assert_eq!(curl_calls(&state), 0, "宣言の無い口座は測らない");
    assert_eq!(stub_calls(&state), 1, "起こし直さない");
    assert!(show_line(&repo, &state, &id).contains("stage=RateLimited"), "便は RateLimited のまま");
    clean(&[&repo, &state]);
}

// ───── 審査の理由の閉じた型・器が作る INCONCLUSIVE の「起動できない」形（`s2-07l.395`・設計 contract-source.md §22） ─────

/// `git` **だけ**を引ける PATH（`sh` も `systemd-run` も無い host＝lens の `sh -c` を起動できない周を作る）。
/// 受付と審査の材料は git だけで読めるので、判定に届かない理由が起動の失敗 1 つに絞れる。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn git_only_path(state: &Path) -> String {
    let bin_dir = state.join("git-only-bin");
    fs::create_dir_all(&bin_dir).expect("dir を作れる");
    let found = Command::new("sh").args(["-c", "command -v git"]).output().expect("command -v を撃てる");
    let real = String::from_utf8_lossy(&found.stdout).trim().to_owned();
    assert!(!real.is_empty(), "git を引ける");
    std::os::unix::fs::symlink(&real, bin_dir.join("git")).ok();
    bin_dir.display().to_string()
}

/// 歯 (2) の 5 形のうち「起動できない」: `sh` の無い PATH で審査の lens を撃つと、器は INCONCLUSIVE（`起動できない`）を
/// 作り、理由の型は 7 語目 `unparsed`・`at` は無い・detail は `verdict:INCONCLUSIVE kind:unparsed`・rc 3。
#[test]
fn pipe_review_kind_lens_that_cannot_start_is_unparsed() {
    let (repo, state) = repo_with_state();
    let contract = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let out = run_pipe_with_path(&git_only_path(&state), &[
        "intake", "--design", &contract, "--bead", "s2-nosh",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state), "--lens", &fake_lens(&marker, &lens_verdict("PASS")),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "起動できない lens は INCONCLUSIVE: {}", stderr_of(&out));
    assert!(!marker.exists(), "lens は 1 度も起きていない（起動の失敗であって出力の欠けではない）");
    let id = run_id_of(&out);
    let pairs = intake::review_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "{pairs:?}");
    assert!(value_of(&pairs, "evidence").contains("起動できない"), "理由は起動の失敗: {pairs:?}");
    assert_eq!(value_of(&pairs, "kind"), "unparsed", "lens の JSON が無い周は 7 語目: {pairs:?}");
    assert!(!pairs.iter().any(|(key, _)| key == "at"), "at は無い: {pairs:?}");
    assert_eq!(intake::reviewed_detail(&state, &id), "verdict:INCONCLUSIVE kind:unparsed");
    clean(&[&repo, &state]);
}
