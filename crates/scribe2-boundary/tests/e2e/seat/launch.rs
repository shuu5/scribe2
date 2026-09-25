//! 席の起動と復元の歯（Enter 落ちの修復と記録の席の列・復元の第 2 手・`seat launch`・設計
//! docs/design/seat-roles.md §7 / account-lifecycle.md §4 / account-autonomy.md §5・接頭辞 `seat_attrib_` /
//! `seat_restore_` / `seat_launch_`）。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。
//! 歯の本文は `seat/account.rs` から**挙動不変で移した**もの（`s2-07l.361`・seat-roles.md §7 の固定した組）。
// flip-check: moved s2-07l.361

use super::*;

// ─────────────────── Enter 落ちの修復と記録の席の列（`s2-07l.150`・接頭辞 `seat_attrib_`） ───────────────────

// ─────────────────── 復元の第 2 手（account-autonomy.md §5「復元の第 2 手」・`s2-07l.318`・接頭辞 `seat_restore_`） ───────────────────

// ─────────────────── 席の起動（account-lifecycle.md §4・ADR-0026 §2.3・`s2-07l.244`・接頭辞 `seat_launch_`） ───────────────────

/// 期待する導出行（穴を埋める前・`launch` の row に載る形）。
fn launch_derived(place: &AcctPlace) -> String {
    format!(
        "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY=1 CLAUDE_CONFIG_DIR={{account_dir}} claude --plugin-dir {} --plugin-dir {} --plugin-dir {} {} {}",
        launch_plugin_root(place), LAUNCH_PLUGINS[0], LAUNCH_PLUGINS[1], LAUNCH_ARGS[0], LAUNCH_ARGS[1]
    )
}

/// 役割の既定の対（埋め込みの行 `seat.model.orchestrator` / `seat.effort.orchestrator`）が起動行で `claude` の直後に運ぶ 4 語
/// （設計 seat-roles.md §20 の約束 1・`--model` → `--effort` の順）。
const LAUNCH_DEFAULT_FLAGS: [&str; 4] = ["--model", "fable", "--effort", "high"];

/// 期待する偽 claude の記録に既定の 4 語を前置した形（argv の先頭が `claude` の直後の語）。
fn launch_defaults_argv(place: &AcctPlace, label: &str) -> String {
    format!("{}\n{}", LAUNCH_DEFAULT_FLAGS.join("\n"), launch_expected_argv(place, label))
}

/// 期待する送る行（穴を埋める前）: 導出行の `claude` の直後に既定の 4 語（雛形 [`launch_derived`] は旗無しのまま）。
fn launch_carried(place: &AcctPlace) -> String {
    launch_derived(place).replacen(" claude ", &format!(" claude {} ", LAUNCH_DEFAULT_FLAGS.join(" ")), 1)
}

/// 包みの tmux が写した argv のうち `verb` で始まる呼出しの数（`-S <socket>` の後ろを見る）。
fn launch_tmux_calls(place: &AcctPlace, verb: &str) -> usize {
    fs::read_to_string(place.dir.join(LAUNCH_TMUX_ARGS))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.split_whitespace().nth(2) == Some(verb))
        .count()
}

/// `<state>/inject.jsonl` の行のうち `kind=launch` を持つものの `(who, what)`。
fn launch_inject_rows(place: &AcctPlace) -> Vec<(String, String)> {
    fs::read_to_string(place.state.join("inject.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| Some((acct_text(line, "who")?, acct_text(line, "what")?)))
        .filter(|(_, what)| tick_token(what, "kind").as_deref() == Some("launch"))
        .collect()
}

/// 起動しなかったことの 3 面（`case`）: 偽 claude は走らず・`send-keys` は 0 回・登録 row は `rows` 件。
fn launch_assert_not_sent(place: &AcctPlace, rows: usize, case: &str) {
    assert!(!place.dir.join("launched").exists(), "{case}: 起動行は届かない");
    assert_eq!(launch_tmux_calls(place, "send-keys"), 0, "{case}: 1 key も送らない");
    assert_eq!(acct_rows(&place.state).len(), rows, "{case}: 登録 row の件数");
    assert!(launch_inject_rows(place).is_empty(), "{case}: inject.jsonl に launch の行は無い");
}

/// (a)(b) window が無い target へ `--account l2` で起こす: `new-window` が 1 回・導出した行（`CLAUDE_CONFIG_DIR=<state>/accounts/l2`・
/// agent view off・anchor の `--plugin-dir` → `[[plugin]]` の dir → `[[launch-arg]]` の value の順）が偽 claude に 1 回だけ届き、
/// `SeatRegistered` は送る**前**に 1 件（`sid` 無し・`launch` = 導出した行〔穴を埋める前〕・account=l2）、`inject.jsonl` に
/// `kind=launch` 1 行、席の打刻に `SessionStart`。base は `launch` の subcommand が無く使い方で断る（RED）。
#[test]
fn seat_launch_creates_the_window_and_injects_the_derived_line_once() {
    let place = launch_place();
    let name = "launchnew";
    let target = format!("{name}:seat");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let out = launch_run(&place, &path, &target, &["--account", "l2"]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    assert_eq!(line, format!("seat launch: launched target={name}_seat account=l2{} trust=unwritable\n", provenance(&place.state, "flag")));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "window を 1 回作る");
    assert!(
        fs::read_to_string(place.dir.join(LAUNCH_TMUX_ARGS)).unwrap_or_default().lines().any(|found| found.ends_with(&format!("new-window -t ={name}: -n seat"))),
        "`new-window -t <session> -n <window>` の形（session は exact の名 + 次の空き index）: {}",
        fs::read_to_string(place.dir.join(LAUNCH_TMUX_ARGS)).unwrap_or_default()
    );
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_defaults_argv(&place, "l2"), "導出した行が既定の旗つきで 1 回だけ届く");
    launch_assert_registered_before_send(&place, &target, "l2");
    let stamps = fs::read_to_string(state_file(&seat_dir_of(&place.state, &format!("{name}_seat")))).unwrap_or_default();
    assert!(stamps.contains("\"event\":\"SessionStart\""), "席が立った打刻: {stamps}");
    assert!(!place.dir.join(LAUNCH_LOG).exists(), "`--restore` 無しは復元を送らない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) 登録 row は**送る前**に 1 件（偽 claude が起動時に写した event log に既に在る・`sid` 無し・`launch` = 導出した行〔旗無し〕・
/// account = `label`・`model` = 行から導いた表示名・鍵 = planner × anchor）・`inject.jsonl` に `kind=launch`（`who=seat-launch`）が 1 行。
fn launch_assert_registered_before_send(place: &AcctPlace, target: &str, label: &str) {
    let rows = acct_rows(&place.state);
    let want = vessel::fleet::Registration {
        role: vessel::seat::role::Role::Orchestrator,
        anchor: launch_anchor(place),
        target: target.to_owned(),
        sid: None,
        account: label.to_owned(),
        launch: launch_derived(place),
        model: Some("Fable".to_owned()),
    };
    assert_eq!(rows, vec![want], "SeatRegistered 1 件・sid 無し・launch は導出した行（穴を埋める前）");
    let seen_text = fs::read_to_string(place.dir.join(LAUNCH_EVENTS_SEEN)).unwrap_or_default();
    assert_eq!(seen_text.lines().filter(|found| found.contains("\"kind\":\"SeatRegistered\"")).count(), 1, "row は起動行を送る前に在る: {seen_text}");
    assert!(!seen_text.contains("\"sid\""), "送る前の row にも sid は無い: {seen_text}");
    let injected = launch_inject_rows(place);
    assert_eq!(injected.len(), 1, "inject.jsonl に kind=launch 1 行: {injected:?}");
    assert_eq!(injected.first().map(|(who, _)| who.as_str()), Some("seat-launch"));
    assert!(injected.first().is_some_and(|(_, what)| tick_token(what, "account").as_deref() == Some(label)), "{injected:?}");
}

/// (c) `--account` 無しは session 用の選定: 実測行 2 口座（l1 = 30・l2 = 40・どちらも閾値未満・逼迫度の最小は l1）のうち、
/// 別席（同じ planner でも別 anchor＝別の鍵）の登録 row が持つ l1 を除外し、残る l2 が選ばれる（既存 window `name:name` へ・
/// `new-window` は 0 回）。
#[test]
fn seat_launch_without_account_selects_excluding_other_seats_accounts() {
    let place = launch_place();
    let name = "launchpick";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    acct_measured(&place.state, "l1", 30, &acct_now());
    acct_measured(&place.state, "l2", 40, &acct_now());
    let other = acct_register_as(&place, "otherseat", "l1", ACCT_LAUNCH);
    assert_eq!(rc_of(&other), i32::from(RC_OK), "別席の登録: stderr={}", stderr_of(&other));

    let out = launch_run(&place, &path, &target, &[]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    assert!(line.starts_with(&format!("seat launch: launched target={name}_{name} account=l2 ")), "l1 は別席の口座＝除外: {line}");
    assert_eq!(launch_tmux_calls(&place, "new-window"), 0, "既存 window には作らない");
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_defaults_argv(&place, "l2"));
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "別席の row + 自席の row: {rows:?}");
    assert_eq!(rows.last().map(|row| (row.account.as_str(), row.sid.clone())), Some(("l2", None)));
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) typed な断り（rc 1・stderr 1 行・`reason=` を値で名指す）で row も key も書かない: 候補なし（実測行なし＝`no-account`
/// `detail=unmeasured`）／session 無し（`session-missing`・作らない）／`--account` が宣言に無い（`account-unknown`）／
/// 入力欄に打ちかけ（`input-busy`）。**入力欄の門も登録 row の前**に移ったので（seat-roles.md §26 の約束 5）、4 つとも
/// row は 0 件である（base は `input-busy` だけ row を 1 件書いてから断る＝RED）。
#[test]
fn seat_launch_refuses_typed_without_sending_or_registering() {
    for (case, extra, rows) in [
        ("no-account", &[][..], 0),
        ("session-missing", &["--account", "l2"][..], 0),
        ("account-unknown", &["--account", "ghost"][..], 0),
        ("input-busy", &["--account", "l2"][..], 0),
    ] {
        let place = launch_place();
        let name = "launchrefuse";
        let target = format!("{name}:{name}");
        let path = launch_shims(&place, &target);
        let guard = launch_session(&place, name, &path);
        assert!(guard.ready(), "{case}: 独立 socket に shell の session を立てられる");
        if case == "input-busy" {
            assert!(tmux(&place.socket, &["send-keys", "-t", &target, "-l", "git st"]).status.success());
            assert!(acct_wait_pane(&place, &target, |pane| pane.trim_end().ends_with("$ git st")), "打ちかけが描かれる");
        }
        let aimed = if case == "session-missing" { "nosuch:seat" } else { target.as_str() };

        let out = launch_run(&place, &path, aimed, extra);

        let line = stderr_of(&out);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{case}: stdout={}", stdout_of(&out));
        assert!(stdout_of(&out).is_empty(), "{case}: stdout は空");
        assert!(line.starts_with("seat launch: refused reason="), "{case}: {line}");
        assert_eq!(tick_token(&line, "reason").as_deref(), Some(case), "{case}: {line}");
        if case == "no-account" {
            assert_eq!(tick_token(&line, "detail").as_deref(), Some("unmeasured"), "{line}");
        }
        launch_assert_not_sent(&place, rows, case);
        assert!(!tmux(&place.socket, &["has-session", "-t", "=nosuch"]).status.success(), "{case}: session を作らない");
        drop(guard);
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// (e) `--restore /rebrief` は立ち上がり（`SessionStart` の打刻）の**後**に 1 回届く: 偽 claude の受けた行は `/rebrief` だけ・
/// 打刻の順は SessionStart → UserPromptSubmit・席の記録は 起動行 → `/rebrief` の 2 行・成立の行に `consumed=true`。
#[test]
fn seat_launch_restore_is_sent_once_after_session_start() {
    let place = launch_place();
    let name = "launchrestore";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let out = launch_run(&place, &path, &target, &["--account", "l1", "--restore", "/rebrief"]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={} pane={}", stderr_of(&out), capture(&place.socket, &target));
    assert!(line.starts_with(&format!("seat launch: launched target={name}_{name} account=l1 consumed=true ")), "{line}");
    assert_eq!(fs::read_to_string(place.dir.join(LAUNCH_LOG)).unwrap_or_default(), "/rebrief\n", "復元は 1 回だけ届く");
    let stamps = fs::read_to_string(state_file(&seat_dir_of(&place.state, &format!("{name}_{name}")))).unwrap_or_default();
    let events: Vec<String> = stamps.lines().filter_map(|found| acct_text(found, "event")).collect();
    assert_eq!(events, ["SessionStart", "UserPromptSubmit", "Stop"], "復元は立ち上がりの後: {stamps}");
    let sent = acct_sent(&place.state, &format!("{name}_{name}"));
    assert_eq!(sent.len(), 2, "起動行と復元の 2 行: {sent:?}");
    assert!(sent.first().is_some_and(|what| what.starts_with(&acct_launch_prefix(&launch_anchor(&place), "CLAUDE_CONFIG_DIR="))), "{sent:?}");
    assert_eq!(sent.get(1).map(String::as_str), Some("/rebrief"), "{sent:?}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (g) 起動行は登録 row の anchor への `cd` を先頭に前置する（`s2-07l.324`・account-lifecycle.md §4・C3「真実は row の anchor」）:
/// pane の shell の cwd が anchor でない（host の再起動後の復元で home に戻った pane の形）まま `seat launch --anchor <dir>` で
/// 起こすと、席の記録の起動行はちょうど `cd '<anchor>' && CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY=1
/// CLAUDE_CONFIG_DIR=<l2> claude …`（cd → agent view と調査の env → claude の順）で、効果でも測る: 起こした偽 claude の cwd（pane の前面 process の cwd）は anchor。
/// 登録 row の `launch`（雛形）と偽 claude の argv に `cd` は載らない（雛形不変・`--plugin-dir` の列も不変）。base は pane の
/// cwd のまま起こす（RED）。
#[test]
fn seat_launch_injects_cd_to_the_row_anchor_before_the_line() {
    let place = launch_place();
    let name = "launchcd";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let anchor = launch_anchor(&place);
    let elsewhere = place.dir.display().to_string();
    assert!(tmux(&place.socket, &["send-keys", "-t", &target, "-l", &format!("cd '{elsewhere}'")]).status.success());
    assert!(tmux(&place.socket, &["send-keys", "-t", &target, "Enter"]).status.success());
    let cwd_of = || stdout_of(&tmux(&place.socket, &["display-message", "-p", "-t", &target, "#{pane_current_path}"])).trim_end().to_owned();
    assert!(
        acct_wait_pane(&place, &target, |pane| pane.trim_end().ends_with('$') && cwd_of() == elsewhere),
        "前提: 起こす前の pane の cwd は anchor ではない: {}",
        cwd_of()
    );

    let out = launch_run(&place, &path, &target, &["--account", "l2"]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={} pane={}", stderr_of(&out), capture(&place.socket, &target));
    let l2_dir = place.state.join("accounts").join("l2").display().to_string();
    let sent = acct_sent(&place.state, &format!("{name}_{name}"));
    assert_eq!(
        sent,
        vec![format!("cd '{anchor}' && {}", launch_carried(&place).replace("{account_dir}", &l2_dir))],
        "起動行は row の anchor への cd → agent view の env → claude の順の 1 行"
    );
    assert_eq!(cwd_of(), anchor, "起こした席の cwd は row の anchor（pane の cwd ではない）");
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_defaults_argv(&place, "l2"), "argv に cd は載らない");
    launch_assert_registered_before_send(&place, &target, "l2");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (f) 起動行は model を typed に運ぶ（`s2-07l.313`・C2.2 / C10・SRS FR59 / FR36）: 行と一致する `--model Fable`（表示名）で
/// 起こすと偽 claude の argv は `--model fable --effort high`（別名・既定の対）で**始まり**（`claude` の直後・anchor の
/// `--plugin-dir` より前）、送った行に `--model` の語は 1 つ・登録 row は `model=Fable` を持ち **`launch`（雛形）に旗の語は無い**
/// （旗無しの導出行のまま）。表に無い `--model nope` は `launch-model-unknown` で row も key も書かない（設計 seat-roles.md §20）。
#[test]
fn seat_launch_carries_the_model_alias_in_the_launch_line() {
    let place = launch_place();
    let name = "launchmodel";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let unknown = launch_run(&place, &path, &target, &["--account", "l2", "--model", "nope"]);
    assert_eq!(rc_of(&unknown), i32::from(RC_REFUSED), "stdout={}", stdout_of(&unknown));
    assert_eq!(tick_token(&stderr_of(&unknown), "reason").as_deref(), Some("launch-model-unknown"), "{}", stderr_of(&unknown));
    launch_assert_not_sent(&place, 0, "model-unknown");

    let out = launch_run(&place, &path, &target, &["--account", "l2", "--model", "Fable"]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        launch_defaults_argv(&place, "l2"),
        "別名の `--model fable --effort high` が `claude` の直後に 1 回だけ届く"
    );
    let sent = acct_sent(&place.state, &format!("{name}_{name}"));
    assert_eq!(sent.len(), 1, "起動行の 1 行: {sent:?}");
    assert!(sent.first().is_some_and(|what| what.contains(" claude --model fable --effort high --plugin-dir ") && launch_model_words(what) == 1), "{sent:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows.first().map(|row| row.model.as_deref()), Some(Some("Fable")), "row の model は表示名のまま: {rows:?}");
    assert_eq!(rows.first().map(|row| row.launch.as_str()), Some(launch_derived(&place).as_str()), "雛形は旗無しの導出行: {rows:?}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) 雛形に literal の `--model` が在る（host の面の `[[launch-arg]]` に `--model` / `opus`＝暫定の再登録の形）周に `--model Fable`
/// で起こすと、器の 1 つと二重になるので後勝ちにせず `launch-model-duplicated` で断る（row も key も書かない）。器は `--model` 無しの
/// 周も行の model を運ぶので（設計 seat-roles.md §20 の約束 1）、literal を持つ雛形は `--model` 無しでも同じ理由で断られる
/// （literal だけが載って行を黙って上書きする形は無い）。
#[test]
fn seat_launch_refuses_a_duplicated_model_in_the_template() {
    let place = launch_place();
    let name = "launchdup";
    let target = format!("{name}:{name}");
    let host = place.state.join(vessel::rules::HOST_MANIFEST);
    let literal = format!("{}\n[[launch-arg]]\nvalue = \"--model\"\n\n[[launch-arg]]\nvalue = \"opus\"\n", fs::read_to_string(&host).unwrap_or_default());
    fs::write(&host, literal).ok();
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let out = launch_run(&place, &path, &target, &["--account", "l2", "--model", "Fable"]);

    let line = stderr_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(tick_token(&line, "reason").as_deref(), Some("launch-model-duplicated"), "{line}");
    launch_assert_not_sent(&place, 0, "model-duplicated");

    let bare = launch_run(&place, &path, &target, &["--account", "l2"]);
    assert_eq!(rc_of(&bare), i32::from(RC_REFUSED), "stdout={}", stdout_of(&bare));
    assert_eq!(tick_token(&stderr_of(&bare), "reason").as_deref(), Some("launch-model-duplicated"), "{}", stderr_of(&bare));
    launch_assert_not_sent(&place, 0, "model-duplicated-bare");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 席の起動の短い形（account-lifecycle.md §14・`s2-07l.404`・接頭辞 `seat_launch_short_`） ───────────────────

/// 短い形 `seat <label> …` を shim の PATH で 1 回撃つ（置き場・anchor・独立 socket は長い形の [`launch_run`] と同じ flag・`extra` は
/// 役割の flag と明示の値）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn launch_run_short(place: &AcctPlace, path: &str, label: &str, extra: &[&str]) -> Output {
    let state = place.state.display().to_string();
    let anchor = launch_anchor(place);
    let mut args = vec!["seat", label];
    args.extend_from_slice(extra);
    args.extend_from_slice(&["--state-dir", &state, "--anchor", &anchor, "--tmux-socket", &place.socket]);
    Command::new(bin()).args(&args).env("PATH", path).output().expect("binary を起動できる")
}

/// 起こした偽 claude を EOF（`C-d`）で終え、前面が shell（prompt `$`）に戻るのを待つ＝同じ window へもう 1 度起こせる形
/// （`new-window` は要らない）。
fn launch_quit_seat(place: &AcctPlace, target: &str) -> bool {
    tmux(&place.socket, &["send-keys", "-t", target, "C-d"]).status.success()
        && acct_wait_pane(place, target, |pane| pane.trim_end().ends_with('$'))
}

/// 短い形の断りの 3 面: rc 1・stdout 0 byte・stderr はちょうど 1 行 `line`。
fn launch_assert_refused_line(out: &Output, line: &str, case: &str) {
    assert_eq!(rc_of(out), i32::from(RC_REFUSED), "{case}: stdout={} stderr={}", stdout_of(out), stderr_of(out));
    assert!(stdout_of(out).is_empty(), "{case}: stdout は空");
    assert_eq!(stderr_of(out), format!("{line}\n"), "{case}");
}

/// (a) 登録 row の在る anchor で短い形 `<label> --orchestrator` は長い形と同じ row と同じ起動行を作る: 長い形（`--account l2 --model Fable`）
/// で 1 回起こして row を作った place で、席を終えて前面を shell に戻し、短い形を `--target` / `--model` 無しで撃つ → rc 0・
/// `inject.jsonl` の `kind=launch` の `what` は 2 行とも同一・最新 row の target / model / account は長い形の row と一致・偽 claude の
/// argv も同じ（`--model fable` を運ぶ）・`new-window` は長い形の 1 回だけ（短い形は 0 回＝window は在る）。base は第 1 token を
/// verb と読めず使い方で断る（RED）。
#[test]
fn seat_launch_short_form_reuses_the_registered_row_target_and_model() {
    let place = launch_place();
    let name = "launchshort";
    let target = format!("{name}:seat");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let long = launch_run(&place, &path, &target, &["--account", "l2", "--model", "Fable"]);
    assert_eq!(rc_of(&long), i32::from(RC_OK), "長い形: stdout={} stderr={}", stdout_of(&long), stderr_of(&long));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "長い形が window を 1 回作る");
    assert!(launch_quit_seat(&place, &target), "席を終えて前面を shell に戻せる: {}", capture(&place.socket, &target));

    let out = launch_run_short(&place, &path, "l2", &["--orchestrator"]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "短い形: stdout={line} stderr={} pane={}", stderr_of(&out), capture(&place.socket, &target));
    assert_eq!(line, format!("seat launch: launched target={name}_seat account=l2{} trust=unwritable\n", provenance(&place.state, "flag")));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "短い形は window を作らない（在る window へ起こす）");
    let injected = launch_inject_rows(&place);
    assert_eq!(injected.len(), 2, "長い形と短い形の kind=launch が 1 行ずつ: {injected:?}");
    assert_eq!(injected.first().map(|(_, what)| what), injected.get(1).map(|(_, what)| what), "注入の記録の what は同一: {injected:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "同じ鍵の row が 2 件: {rows:?}");
    assert_eq!(rows.first(), rows.get(1), "短い形の row は長い形の row と同じ（target / model / account）: {rows:?}");
    assert_eq!(rows.get(1).map(|row| (row.target.as_str(), row.model.as_deref(), row.account.as_str())), Some((target.as_str(), Some("Fable"), "l2")));
    let argv = launch_defaults_argv(&place, "l2");
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("{argv}{argv}"),
        "偽 claude の argv は 2 回とも同じ（row の model を運ぶ）"
    );
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) row が無く flag も無い周は `defaults-unresolved` で typed に断る: rc 1・stderr の行はちょうど
/// `seat launch: refused reason=defaults-unresolved missing=--target`（`target=` を持たない）・0 key・row 0・`inject.jsonl` に
/// launch 行なし。`missing=` に `--model` は載らない（model は役割の既定の行から導く＝欠けるのは行を読めない周だけ・設計
/// seat-roles.md §20 の約束 4）。2 周目: 長い形（`--model` 無し）で作った row は行から導いた `model=Fable` を持ち、その place で
/// 短い形を `--model` 無しで撃つと断られず、row の target と行の model で起きる。
#[test]
fn seat_launch_short_form_refuses_typed_without_a_row() {
    let place = launch_place();
    let name = "launchshortnorow";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let out = launch_run_short(&place, &path, "l2", &["--orchestrator"]);

    launch_assert_refused_line(&out, "seat launch: refused reason=defaults-unresolved missing=--target", "no-row");
    assert!(!stderr_of(&out).contains("target="), "解けない target を行に置かない: {}", stderr_of(&out));
    assert!(!stderr_of(&out).contains("--model"), "model は行から導く: {}", stderr_of(&out));
    launch_assert_not_sent(&place, 0, "no-row");

    let long = launch_run(&place, &path, &target, &["--account", "l2"]);
    assert_eq!(rc_of(&long), i32::from(RC_OK), "長い形（model 無し）: stdout={} stderr={}", stdout_of(&long), stderr_of(&long));
    assert!(launch_quit_seat(&place, &target), "席を終えて前面を shell に戻せる: {}", capture(&place.socket, &target));
    let rows = acct_rows(&place.state);
    assert_eq!(rows.first().and_then(|row| row.model.as_deref()), Some("Fable"), "長い形の row は行から導いた model を持つ: {rows:?}");

    let again = launch_run_short(&place, &path, "l2", &["--orchestrator"]);

    assert_eq!(rc_of(&again), i32::from(RC_OK), "短い形: stdout={} stderr={}", stdout_of(&again), stderr_of(&again));
    assert_eq!(launch_inject_rows(&place).len(), 2, "長い形と短い形の launch の記録");
    let argv = launch_defaults_argv(&place, "l2");
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), format!("{argv}{argv}"), "2 回とも行の model を運ぶ");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) 役割の flag は**多くとも 1 つ**（0 個の極性は seat-roles.md §26 の約束 1 で変わった）: 2 個（`--orchestrator` の
/// 重複）は従来どおり使い方で断り（rc 1・stderr は usage・stdout 0 byte・0 key・row 0）、**0 個は既定の orchestrator と
/// して成立する**（rc 0・導出した行が偽 claude に届き row は 1 件・`role` は既定の役割）。使い方の 1 枚では役割の flag が
/// 任意の形（`[--orchestrator]`）で載る。base は 0 個も使い方で断る（RED）。
#[test]
fn seat_launch_short_form_requires_exactly_one_role_flag() {
    let place = launch_place();
    let name = "launchshortrole";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let two = launch_run_short(&place, &path, "l2", &["--orchestrator", "--orchestrator", "--target", &target, "--model", "Fable"]);

    assert_eq!(rc_of(&two), i32::from(RC_REFUSED), "two: stdout={} stderr={}", stdout_of(&two), stderr_of(&two));
    assert!(stdout_of(&two).is_empty(), "two: stdout は空");
    assert!(stderr_of(&two).starts_with("usage: seat "), "two: 使い方で断る: {}", stderr_of(&two));
    assert!(stderr_of(&two).contains("[--orchestrator]"), "two: 使い方の短い形で役割の flag は任意: {}", stderr_of(&two));
    launch_assert_not_sent(&place, 0, "two");

    let zero = launch_run_short(&place, &path, "l2", &["--target", &target, "--model", "Fable"]);

    assert_eq!(rc_of(&zero), i32::from(RC_OK), "zero: stdout={} stderr={}", stdout_of(&zero), stderr_of(&zero));
    assert!(stdout_of(&zero).starts_with(&format!("seat launch: launched target={name}_{name} account=l2 ")), "zero: {}", stdout_of(&zero));
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        launch_defaults_argv(&place, "l2"),
        "zero: 既定の役割で導出した行が届く"
    );
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 1, "zero: 登録 row は 1 件: {rows:?}");
    assert_eq!(rows.first().map(|row| row.role), Some(vessel::seat::role::Role::Orchestrator), "zero: 既定の役割: {rows:?}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// 会話の引き継ぎの語 `tail` を偽 claude の argv の末尾（env の 2 行の前）に足した形。
fn launch_carried_argv(place: &AcctPlace, label: &str, tail: &[&str]) -> String {
    launch_defaults_argv(place, label).replacen("\nenv:CLAUDE_CONFIG_DIR=", &format!("\n{}\nenv:CLAUDE_CONFIG_DIR=", tail.join("\n")), 1)
}

/// 置き場の下の `*.launch` file（手書きの起動 script の置き場）の中身を全部繋いだ字面（無ければ空）。
fn launch_files_text(dir: &Path) -> String {
    let mut text = String::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            text.push_str(&launch_files_text(&path));
        } else if path.extension().is_some_and(|ext| ext == "launch") {
            text.push_str(&fs::read_to_string(&path).unwrap_or_default());
        }
    }
    text
}

/// 会話の引き継ぎの 1 周の 4 面（§18 の約束 1）: 送った行の末尾が `tail`・偽 claude の argv の末尾も `tail`・登録 row の `launch` は
/// 旗無しの導出行・置き場の `.launch` に旗は無い。
fn launch_assert_carried(place: &AcctPlace, name: &str, tail: &[&str], case: &str) {
    let sent = acct_sent(&place.state, &format!("{name}_{name}"));
    assert_eq!(sent.len(), 1, "{case}: 起動行の 1 行: {sent:?}");
    assert!(sent.first().is_some_and(|line| line.ends_with(&format!(" claude {} --plugin-dir {} --plugin-dir {} --plugin-dir {} {} {} {}", LAUNCH_DEFAULT_FLAGS.join(" "), launch_plugin_root(place), LAUNCH_PLUGINS[0], LAUNCH_PLUGINS[1], LAUNCH_ARGS[0], LAUNCH_ARGS[1], tail.join(" ")))), "{case}: 注入行の末尾: {sent:?}");
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_carried_argv(place, "l2", tail), "{case}: argv の末尾");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.iter().map(|row| row.launch.as_str()).collect::<Vec<_>>(), [launch_derived(place).as_str()], "{case}: row の launch は旗無し: {rows:?}");
    assert!(rows.iter().all(|row| !row.launch.contains("--continue") && !row.launch.contains("--resume")), "{case}: {rows:?}");
    let files = launch_files_text(&place.state);
    assert!(!files.contains("--continue") && !files.contains("--resume"), "{case}: .launch は旗無し: {files}");
    assert_eq!(launch_inject_rows(place).len(), 1, "{case}: inject.jsonl に kind=launch 1 行");
}

/// §18 の約束 1 / 4: `seat <label> -c`（別名 `--continue`）は注入する起動行の**末尾**に `--continue` を足し、登録 row の `launch`
/// と置き場の `.launch` は旗無しのまま。役割の flag は 0 個（既定の orchestrator）のまま通る。base は `-c` を黙って捨てる（RED）。
#[test]
fn seat_launch_short_form_continues_the_last_conversation_at_the_tail() {
    for (case, flag) in [("short", "-c"), ("long", "--continue")] {
        let place = launch_place();
        let name = "launchcontinue";
        let target = format!("{name}:{name}");
        let path = launch_shims(&place, &target);
        let guard = launch_session(&place, name, &path);
        assert!(guard.ready(), "{case}: 独立 socket に shell の session を立てられる");

        let out = launch_run_short(&place, &path, "l2", &[flag, "--target", &target, "--model", "Fable"]);

        assert_eq!(rc_of(&out), i32::from(RC_OK), "{case}: stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
        assert_eq!(acct_rows(&place.state).first().map(|row| row.role), Some(vessel::seat::role::Role::Orchestrator), "{case}: 既定の役割");
        launch_assert_carried(&place, name, &["--continue"], case);
        drop(guard);
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// §18 の約束 1 / 2: `seat <label> -r ID`（別名 `--resume ID`）は注入行の末尾に `--resume ID` を足し row の `launch` には載せない。
/// 使い方の誤り（rc 1・stderr は usage・0 key・row 0）: `-c -r ID`・値の無い `-r`・`-c -c`・`-r -r`・会話 id の形でない値
/// （空白を含む・`$(` を含む・`-` 始まり・大文字・短い）。base は `-r` を黙って捨てて起こす（RED）。
#[test]
fn seat_launch_short_form_resumes_a_named_conversation_and_refuses_malformed_ids() {
    let id = "0f3c9a2e-1b4d-4e8f-9a6b-7c2d5e8f1a3b";
    let place = launch_place();
    let name = "launchresume";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    for (case, extra) in [
        ("both", &["-c", "-r", id][..]),
        ("both-long", &["--resume", id, "--continue"][..]),
        ("no-value", &["-r"][..]),
        ("flag-as-value", &["-r", "--target", &target][..]),
        ("twice-c", &["-c", "-c"][..]),
        ("twice-r", &["-r", id, "-r", id][..]),
        ("space", &["-r", "0f3c9a2e 1b4d-4e8f-9a6b-7c2d5e8f1a3b"][..]),
        ("subshell", &["-r", "$(touch pwned)"][..]),
        ("dash", &["-r", "-f3c9a2e-1b4d-4e8f-9a6b-7c2d5e8f1a3b"][..]),
        ("upper", &["-r", "0F3C9A2E-1B4D-4E8F-9A6B-7C2D5E8F1A3B"][..]),
        ("short", &["-r", "0f3c9a2e"][..]),
    ] {
        let mut args = extra.to_vec();
        args.extend_from_slice(&["--target", &target, "--model", "Fable"]);

        let out = launch_run_short(&place, &path, "l2", &args);

        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{case}: stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
        assert!(stdout_of(&out).is_empty(), "{case}: stdout は空");
        assert!(stderr_of(&out).starts_with("usage: seat "), "{case}: 使い方で断る: {}", stderr_of(&out));
        launch_assert_not_sent(&place, 0, case);
    }

    let out = launch_run_short(&place, &path, "l2", &["-r", id, "--target", &target, "--model", "Fable"]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    launch_assert_carried(&place, name, &["--resume", id], "resume");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 席の入口の 1 語（seat-roles.md §26・`s2-07l.488`・接頭辞 `seat_entry_`） ───────────────────

/// target の pane の前面 process が `want` になるのを待つ（上限 [`PROMPT_WAIT`]・`#{pane_current_command}` は typed な
/// metadata で端末描画の字面ではない）。読むのは**本物の** tmux（shim の PATH を通さない test 自身の口）。
fn launch_wait_front(place: &AcctPlace, target: &str, want: &str) -> bool {
    let deadline = Instant::now().checked_add(PROMPT_WAIT);
    while deadline.is_some_and(|at| Instant::now() < at) {
        let out = tmux(&place.socket, &["display-message", "-p", "-t", target, "#{pane_current_command}"]);
        if String::from_utf8_lossy(&out.stdout).trim() == want {
            return true;
        }
        sleep(Duration::from_millis(100));
    }
    false
}

/// 約束 1（§26）: **役割の flag が 0 個**の短い形が、長い形と同じ登録 row と同じ注入行を作る。長い形
/// （`--account l2 --model Fable`）で row を作って席を終えた place で `seat l2` の 1 語（役割の flag も `--target` も
/// `--model` も無い）を撃つ → rc 0・`inject.jsonl` の `kind=launch` の `what` は 2 行とも同一・row は 2 件とも等しい・
/// 偽 claude の argv も 2 回とも同じ・`new-window` は長い形の 1 回だけ（1 語は在る窓へ起こす）。
/// base は 0 個を使い方で断る（RED）。
#[test]
fn seat_entry_short_form_defaults_the_role_to_orchestrator() {
    let place = launch_place();
    let name = "entryrole";
    let target = format!("{name}:seat");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let long = launch_run(&place, &path, &target, &["--account", "l2", "--model", "Fable"]);
    assert_eq!(rc_of(&long), i32::from(RC_OK), "長い形: stdout={} stderr={}", stdout_of(&long), stderr_of(&long));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "長い形が window を 1 回作る");
    assert!(launch_quit_seat(&place, &target), "席を終えて前面を shell に戻せる: {}", capture(&place.socket, &target));

    let out = launch_run_short(&place, &path, "l2", &[]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "1 語: stdout={line} stderr={} pane={}", stderr_of(&out), capture(&place.socket, &target));
    assert_eq!(line, format!("seat launch: launched target={name}_seat account=l2{} trust=unwritable\n", provenance(&place.state, "flag")));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "1 語は window を作らない（row の target の窓へ起こす）");
    let injected = launch_inject_rows(&place);
    assert_eq!(injected.len(), 2, "長い形と 1 語の kind=launch が 1 行ずつ: {injected:?}");
    assert_eq!(injected.first().map(|(_, what)| what), injected.get(1).map(|(_, what)| what), "注入の記録の what は同一: {injected:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "同じ鍵の row が 2 件: {rows:?}");
    assert_eq!(rows.first(), rows.get(1), "1 語の row は長い形の row と同じ（役割 / target / model / 口座）: {rows:?}");
    let argv = launch_defaults_argv(&place, "l2");
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), format!("{argv}{argv}"), "偽 claude の argv は 2 回とも同じ");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// 約束 3 / 4（§26）: 登録 row も `--target` も無い周は、**`-t` を付けない** `display-message` の 1 問いで測った session の
/// 名と役割の字面を `:` で繋いだ target で席が立つ。3 周を同じ place で測る: (1) 問いが撃てない（偽 tmux が rc 1）→
/// `missing=--target` (2) session の名が空（空の答え）→ 同じ断り（空を target に化けさせない） (3) 名は返るが row も
/// `--model` も無い → model は役割の既定の行から導かれ（設計 seat-roles.md §20 の約束 4）、`<session>:orchestrator` の窓が
/// 1 つ作られて席が立ち、row の target もその字面。**環境変数は 1 つも読まない**（器の `env::` の許し列は 3 つのままで、
/// この経路は tmux への問いだけ）。
#[test]
fn seat_entry_target_defaults_to_the_caller_session_and_role() {
    let place = launch_place();
    let name = "entrytarget";
    let target = format!("{name}:orchestrator");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let away = launch_run_short(&place, &path, "l2", &["--model", "Fable"]);
    launch_assert_refused_line(&away, "seat launch: refused reason=defaults-unresolved missing=--target", "no-tmux");
    launch_assert_not_sent(&place, 0, "no-tmux");

    launch_caller_session(&place, "");
    let empty = launch_run_short(&place, &path, "l2", &["--model", "Fable"]);
    launch_assert_refused_line(&empty, "seat launch: refused reason=defaults-unresolved missing=--target", "empty-session");
    launch_assert_not_sent(&place, 0, "empty-session");

    launch_caller_session(&place, name);

    let out = launch_run_short(&place, &path, "l2", &[]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={} pane={}", stderr_of(&out), capture(&place.socket, &target));
    assert_eq!(line, format!("seat launch: launched target={name}_orchestrator account=l2{} trust=unwritable\n", provenance(&place.state, "flag")));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "役割の名の窓を 1 回作る");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 1, "登録 row は 1 件: {rows:?}");
    assert_eq!(rows.first().map(|row| row.target.as_str()), Some(target.as_str()), "row の target は <測った session>:<役割>: {rows:?}");
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        launch_defaults_argv(&place, "l2"),
        "導出した行が 1 回だけ届く"
    );
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// 断りの周ごとの前提を作る（[`seat_entry_refuses_before_writing_the_registration_row`] の各 `case`）: 窓に生きた席が在る
/// 形（前面 process が shell でない）・撃てない tmux（`capture-pane` / `new-window` を rc 1 にする）・打ちかけの入力・
/// prompt の末尾が閉じた列のどれでもない形。`session-missing` は target を別の名にするだけなので何もしない。
fn launch_arrange_refusal(place: &AcctPlace, target: &str, case: &str) {
    match case {
        "not-a-shell" => {
            assert!(tmux(&place.socket, &["send-keys", "-t", target, "-l", "cat"]).status.success());
            assert!(tmux(&place.socket, &["send-keys", "-t", target, "Enter"]).status.success());
            assert!(launch_wait_front(place, target, "cat"), "{case}: 前面が shell でなくなる: {}", capture(&place.socket, target));
        }
        "pane-missing" => launch_tmux_refuse(place, "capture-pane"),
        "input-busy" => {
            assert!(tmux(&place.socket, &["send-keys", "-t", target, "-l", "git st"]).status.success());
            assert!(acct_wait_pane(place, target, |pane| pane.trim_end().ends_with("$ git st")), "{case}: 打ちかけが描かれる");
        }
        "input-unknown" => {
            assert!(tmux(&place.socket, &["send-keys", "-t", target, "-l", "PS1=Password:"]).status.success());
            assert!(tmux(&place.socket, &["send-keys", "-t", target, "Enter"]).status.success());
            assert!(
                acct_wait_pane(place, target, |pane| pane.trim_end().ends_with("Password:")),
                "{case}: 入力欄を特定できない prompt になる: {}",
                capture(&place.socket, target)
            );
        }
        "window-unwritable" => launch_tmux_refuse(place, "new-window"),
        _ => {}
    }
}

/// 約束 5 / 6 / 9（§26）: 前提の断り 5 つは**登録 row を書く前**に出て row は 0 件のまま残り、row の後に残る
/// `window-unwritable` だけが row を 1 件残す（`s2-07l.488` の実測＝断った周に点検の口が `registered=1 live=1` と出る形を
/// 塞ぐ）。どの周も 1 key も送らず偽 claude も走らない（置き換えも起きない）。`not-a-shell` の 1 行は**次の 1 手**の
/// 字面を持つ（約束 9）。base は `not-a-shell` / `pane-missing` / `input-busy` / `input-unknown` の 4 つが row を書いた
/// 後に断り、断りの行も次の 1 手を持たない（RED）。
#[test]
fn seat_entry_refuses_before_writing_the_registration_row() {
    for (case, rows) in [
        ("session-missing", 0),
        ("not-a-shell", 0),
        ("pane-missing", 0),
        ("input-busy", 0),
        ("input-unknown", 0),
        ("window-unwritable", 1),
    ] {
        let place = launch_place();
        let name = "entryrefuse";
        let target = format!("{name}:{name}");
        let path = launch_shims(&place, &target);
        let guard = launch_session(&place, name, &path);
        assert!(guard.ready(), "{case}: 独立 socket に shell の session を立てられる");
        let aimed = match case {
            "session-missing" => "nosuch:seat".to_owned(),
            "window-unwritable" => format!("{name}:ghost"),
            _ => target.clone(),
        };
        launch_arrange_refusal(&place, &target, case);

        let out = launch_run_short(&place, &path, "l2", &["--target", &aimed, "--model", "Fable"]);

        let line = stderr_of(&out);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{case}: stdout={}", stdout_of(&out));
        assert!(stdout_of(&out).is_empty(), "{case}: stdout は空");
        assert!(line.starts_with("seat launch: refused reason="), "{case}: {line}");
        assert_eq!(tick_token(&line, "reason").as_deref(), Some(case), "{case}: {line}");
        assert!(!place.dir.join("launched").exists(), "{case}: 起動行は届かない（送信も置き換えも無い）");
        assert_eq!(launch_tmux_calls(&place, "send-keys"), 0, "{case}: 1 key も送らない");
        assert_eq!(acct_rows(&place.state).len(), rows, "{case}: 登録 row の件数");
        assert!(launch_inject_rows(&place).is_empty(), "{case}: inject.jsonl に launch の行は無い");
        if case == "not-a-shell" {
            assert_eq!(
                tick_token(&line, "next").as_deref(),
                Some("その窓の席を終わらせてから同じ窓で打つ／別の名の窓を--targetで名指す"),
                "{case}: 断りの行が次の 1 手を持つ: {line}"
            );
        }
        drop(guard);
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// 約束 7 / 8（§26）: 呼び手の target と解いた target の**字面が一致する**周は、前面の判定も入力欄の門も掛けず
/// （その pane で `cat` が走り〔shell でない〕打ちかけも在る＝掛ければ 1 回も通らない形でも断られない）、**key を 1 つも
/// 送らず**に登録 row と起動の記帳を済ませ、`sh -c <起動行>` の 1 枚で自分の process を置き換える——偽 claude が argv と
/// env を長い形の起動と同じ形で記録し、器の行は 1 つも出ない。その周の `--restore` は置き換えの後に合図を送る process が
/// 残らないので**登録 row を書く前に**閉じた理由 1 つで断る（row 0）。base は前面が起動の口自身なので `not-a-shell`
/// （しかも row を書いた後）で断る（RED）。
#[test]
fn seat_entry_same_window_replaces_the_process_with_the_launch_line() {
    let place = launch_place();
    let name = "entrysame";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    // 呼び手の pane は target の pane そのもの（偽 tmux の `-t` 無しの 1 問いが同じ字面を返す）。
    launch_caller_target(&place, &target);
    assert!(tmux(&place.socket, &["send-keys", "-t", &target, "-l", "cat"]).status.success());
    assert!(tmux(&place.socket, &["send-keys", "-t", &target, "Enter"]).status.success());
    assert!(launch_wait_front(&place, &target, "cat"), "前面が shell でなくなる: {}", capture(&place.socket, &target));
    assert!(tmux(&place.socket, &["send-keys", "-t", &target, "-l", "half typed"]).status.success());

    let restore = launch_run_short(&place, &path, "l2", &["--target", &target, "--model", "Fable", "--restore", "/rebrief"]);

    launch_assert_refused_line(
        &restore,
        &format!("seat launch: refused reason=restore-in-the-same-window target={name}_{name}{}", provenance(&place.state, "flag")),
        "same-restore",
    );
    launch_assert_not_sent(&place, 0, "same-restore");

    let out = launch_run_short(&place, &path, "l2", &["--target", &target, "--model", "Fable"]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    assert!(!stdout_of(&out).contains("seat launch:"), "置き換えの後に器の行は 1 つも出ない: {}", stdout_of(&out));
    assert_eq!(launch_tmux_calls(&place, "send-keys"), 0, "1 key も送らない");
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        launch_defaults_argv(&place, "l2"),
        "偽 claude の argv と env は長い形の起動と同じ"
    );
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 1, "登録 row は 1 件: {rows:?}");
    assert_eq!(rows.first().map(|row| (row.target.as_str(), row.account.as_str())), Some((target.as_str(), "l2")));
    assert_eq!(launch_inject_rows(&place).len(), 1, "起動の記帳も置き換えの前に済ませる");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// 約束 10（§26・今の挙動の確認・新しい名詞の口を足さない）: `seat <別の label>` は同じ鍵（役割 × anchor）の登録 row を
/// 新しい label で書き直し、起動行の口座の dir も新しい label を指す。長い形で `l2` の row を作って席を終えた place で
/// `seat l1` の 1 語を撃つ → 2 件目の row は鍵が同じまま `account=l1`（target と model は row から継ぐ）・偽 claude の
/// 2 回目の argv の `CLAUDE_CONFIG_DIR` は `l1` の credential dir。
#[test]
fn seat_entry_relabels_the_registered_row_for_another_account() {
    let place = launch_place();
    let name = "entryrelabel";
    let target = format!("{name}:seat");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let long = launch_run(&place, &path, &target, &["--account", "l2", "--model", "Fable"]);
    assert_eq!(rc_of(&long), i32::from(RC_OK), "長い形: stdout={} stderr={}", stdout_of(&long), stderr_of(&long));
    assert!(launch_quit_seat(&place, &target), "席を終えて前面を shell に戻せる: {}", capture(&place.socket, &target));

    let out = launch_run_short(&place, &path, "l1", &[]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={} pane={}", stderr_of(&out), capture(&place.socket, &target));
    assert_eq!(line, format!("seat launch: launched target={name}_seat account=l1{} trust=unwritable\n", provenance(&place.state, "flag")));
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(
        rows.get(1).map(|row| (row.role, row.anchor.clone())),
        rows.first().map(|row| (row.role, row.anchor.clone())),
        "鍵（役割 × anchor）は同じ: {rows:?}"
    );
    assert_eq!(
        rows.get(1).map(|row| (row.account.as_str(), row.target.as_str(), row.model.as_deref())),
        Some(("l1", target.as_str(), Some("Fable"))),
        "同じ鍵の row を新しい label で書き直す: {rows:?}"
    );
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("{}{}", launch_defaults_argv(&place, "l2"), launch_defaults_argv(&place, "l1")),
        "起動行の口座の dir が新しい label を指す"
    );
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) 既知の verb は従来どおり通る: `launch` の長い形は短い形の口が在っても同じ結果（window 1 回・導出した行が 1 回届く・row は
/// 送る前に 1 件・`inject.jsonl` に `kind=launch` 1 行＝[`seat_launch_creates_the_window_and_injects_the_derived_line_once`] と同じ）。
#[test]
fn seat_launch_short_form_keeps_known_verbs() {
    let place = launch_place();
    let name = "launchshortverb";
    let target = format!("{name}:seat");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let out = launch_run(&place, &path, &target, &["--account", "l2"]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    assert_eq!(line, format!("seat launch: launched target={name}_seat account=l2{} trust=unwritable\n", provenance(&place.state, "flag")));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "window を 1 回作る");
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_defaults_argv(&place, "l2"), "導出した行が 1 回だけ届く");
    launch_assert_registered_before_send(&place, &target, "l2");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 席の既定の model と effort（設計 seat-roles.md §20・`s2-07l.433`・接頭辞 `seat_defaults_`） ───────────────────

/// 約束 1: `--model` 無しの起動行は `claude` の語の直後に `--model fable` → `--effort high`（役割の既定の行の値・この順）を
/// **1 つずつ**運ぶ（送った行の語の並びと偽 claude の argv の先頭の 4 語で測る）。雛形（登録 row の `launch`）には旗の語が
/// 1 つも残らず、導出行と 1 語も違わない。
#[test]
fn seat_defaults_launch_line_carries_model_then_effort_after_claude() {
    let place = launch_place();
    let name = "defaultsline";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let out = launch_run(&place, &path, &target, &["--account", "l2"]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    let argv = fs::read_to_string(place.dir.join("launched")).unwrap_or_default();
    assert_eq!(argv.lines().take(4).collect::<Vec<_>>(), LAUNCH_DEFAULT_FLAGS, "claude の直後の 4 語: {argv}");
    assert_eq!(argv, launch_defaults_argv(&place, "l2"), "{argv}");
    let sent = acct_sent(&place.state, &format!("{name}_{name}"));
    let line = sent.first().cloned().unwrap_or_default();
    let words: Vec<&str> = line.split(' ').collect();
    let after: Vec<&str> = words.iter().skip_while(|word| **word != "claude").skip(1).take(4).copied().collect();
    assert_eq!(after, LAUNCH_DEFAULT_FLAGS, "送った行も同じ位置・同じ順: {line}");
    for flag in ["--model", "--effort"] {
        assert_eq!(words.iter().filter(|word| **word == flag).count(), 1, "{flag} は 1 つ: {line}");
    }
    let rows = acct_rows(&place.state);
    let template = rows.first().map(|row| row.launch.clone()).unwrap_or_default();
    assert_eq!(template, launch_derived(&place), "雛形は 1 語も書き換わらない: {rows:?}");
    assert!(!template.contains("--model") && !template.contains("--effort"), "雛形に旗は残らない: {template}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// 約束 2: 雛形に literal の `--effort` が在る（host の面の `[[launch-arg]]` に `--effort` / `low`）周は、器の 1 つと二重になるので
/// 後勝ちにせず `launch-effort-duplicated`（`--model` の二重の `launch-model-duplicated` とは違う閉じた理由）で断る・row も key も
/// 書かない。
#[test]
fn seat_defaults_launch_refuses_a_duplicated_effort_with_its_own_reason() {
    let place = launch_place();
    let name = "defaultsdup";
    let target = format!("{name}:{name}");
    let host = place.state.join(vessel::rules::HOST_MANIFEST);
    let literal = format!("{}\n[[launch-arg]]\nvalue = \"--effort\"\n\n[[launch-arg]]\nvalue = \"low\"\n", fs::read_to_string(&host).unwrap_or_default());
    fs::write(&host, literal).ok();
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let out = launch_run(&place, &path, &target, &["--account", "l2"]);

    let line = stderr_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(tick_token(&line, "reason").as_deref(), Some("launch-effort-duplicated"), "{line}");
    assert_ne!(tick_token(&line, "reason").as_deref(), Some("launch-model-duplicated"), "{line}");
    launch_assert_not_sent(&place, 0, "effort-duplicated");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// 約束 3: `--model` は照合であって宣言ではない。行（`fable`）と食い違う `--model Opus` は `launch-model-mismatch` で断り、1 key も
/// 送らず登録 row も書かない。一致する `--model fable`（別名）は通り、row の model は行の表示名。使い方の 1 枚に `--effort` の
/// flag は無い（席の effort の宣言は行の 1 か所）。
#[test]
fn seat_defaults_launch_model_is_a_check_against_the_row() {
    let place = launch_place();
    let name = "defaultscheck";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let other = launch_run(&place, &path, &target, &["--account", "l2", "--model", "Opus"]);

    assert_eq!(rc_of(&other), i32::from(RC_REFUSED), "stdout={}", stdout_of(&other));
    assert_eq!(tick_token(&stderr_of(&other), "reason").as_deref(), Some("launch-model-mismatch"), "{}", stderr_of(&other));
    launch_assert_not_sent(&place, 0, "model-mismatch");

    let same = launch_run(&place, &path, &target, &["--account", "l2", "--model", "fable"]);

    assert_eq!(rc_of(&same), i32::from(RC_OK), "stdout={} stderr={}", stdout_of(&same), stderr_of(&same));
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_defaults_argv(&place, "l2"));
    let rows = acct_rows(&place.state);
    assert_eq!(rows.iter().map(|row| row.model.as_deref()).collect::<Vec<_>>(), [Some("Fable")], "{rows:?}");
    assert!(!stderr_of(&run_seat(&[])).contains("--effort"), "使い方に --effort の flag は無い");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// 約束 4: 登録 row も `--model` も無い置き場（呼び手の session だけが測れる）で `seat <label>` の 1 語が起動まで通り、起動行は
/// 行の model と effort を運び、row の model は行から導いた表示名（`missing=` に `--model` が載るのは行を読めない周だけ）。
#[test]
fn seat_defaults_short_form_derives_the_model_without_a_row() {
    let place = launch_place();
    let name = "defaultsshort";
    let target = format!("{name}:orchestrator");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    launch_caller_session(&place, name);
    assert!(acct_rows(&place.state).is_empty(), "前提: 登録 row は無い");

    let out = launch_run_short(&place, &path, "l2", &[]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={} pane={}", stderr_of(&out), capture(&place.socket, &target));
    assert_eq!(line, format!("seat launch: launched target={name}_orchestrator account=l2{} trust=unwritable\n", provenance(&place.state, "flag")));
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_defaults_argv(&place, "l2"), "行の既定を運ぶ");
    let rows = acct_rows(&place.state);
    assert_eq!(
        rows.iter().map(|row| (row.target.as_str(), row.model.as_deref())).collect::<Vec<_>>(),
        [(target.as_str(), Some("Fable"))],
        "{rows:?}"
    );
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 群の置き場の席の口座（account-lifecycle.md §20 形 3・契約表の行 i・接頭辞 `seat_launch_group_`） ───────────────────
//
// 本物の tmux を使わない（独立 socket も立てない）: 偽 tmux の script が session と窓の在りか・前面の shell・空の入力欄を返し、
// `send-keys -l` の行を [`GROUP_LAUNCHED`] へ写して Enter で席の打刻に `SessionStart` を 1 行足す（起動が立ち上がりを確かめられる）。

/// 偽 tmux が受けた起動行の置き場。
const GROUP_LAUNCHED: &str = "group-launched";

/// 群の歯の target（窓 `seat` は偽 tmux の `list-windows` が返す名）。
const GROUP_TARGET: &str = "gl:seat";

/// 偽 tmux の前面の file（在ればその中身を `list-panes` が返す・無ければ `bash`＝shell）。
const GROUP_FRONT: &str = "group-front";

/// 偽 tmux の呼び手の target の file（在れば `display-message` がその中身を返す・無ければ rc 1＝呼び手は target でない）。
const GROUP_CALLER: &str = "group-caller";

/// 群の歯の置き場: [`launch_place`] の host の面に群 `g`（置き場 = この置き場の anchor か、`outside` なら別の `/elsewhere`・候補 =
/// l1 → l2）を足し、偽 tmux だけの PATH を返す。`record` が在れば群の今の口座の記録（host の根の群用 dir の `g.account`）を置く。
fn launch_group_place(outside: bool, record: Option<&str>) -> (AcctPlace, String) {
    let place = launch_place();
    let anchor = if outside { "/elsewhere".to_owned() } else { launch_anchor(&place) };
    let host = place.state.join(vessel::rules::HOST_MANIFEST);
    let body = fs::read_to_string(&host).unwrap_or_default();
    let group = format!("\n[[account-group]]\nname = \"g\"\nanchors = [\"{anchor}\"]\naccounts = [\"l1\", \"l2\"]\n");
    fs::write(&host, format!("{body}{group}")).ok();
    if let Some(label) = record {
        let dir = place.dir.join(format!("{NAME}-host")).join("groups");
        fs::create_dir_all(&dir).ok();
        fs::write(dir.join("g.account"), format!("account={label}\nts=2026-09-24T00:00:00Z\nreason=move\nprevious=l1\n")).ok();
    }
    let bin = place.dir.join("group-bin");
    fs::create_dir_all(&bin).ok();
    let (args, launched, seats) = (place.dir.join(LAUNCH_TMUX_ARGS), place.dir.join(GROUP_LAUNCHED), place.state.join("seat"));
    let (front, caller) = (place.dir.join(GROUP_FRONT), place.dir.join(GROUP_CALLER));
    let tmux = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{args}'\nt=''; p=''\nfor a in \"$@\"; do [ \"$p\" = '-t' ] && t=\"$a\"; p=\"$a\"; done\n\
         f=$(printf '%s' \"$t\" | tr ':' '_')\ncase \"$1\" in\n\
         has-session) exit 0;;\nlist-windows) echo seat;;\nlist-panes) cat '{front}' 2>/dev/null || echo bash;;\n\
         display-message) cat '{caller}' 2>/dev/null || exit 1;;\n\
         capture-pane) printf '$ \\n';;\n\
         send-keys) if [ \"$4\" = \"-l\" ]; then printf '%s\\n' \"$5\" >> '{launched}'\n\
         elif [ \"$4\" = \"Enter\" ]; then mkdir -p '{seats}/'\"$f\"\n\
         printf '{{\"schema\":1,\"state\":\"idle\",\"event\":\"SessionStart\",\"ts\":%s,\"sid\":\"\"}}\\n' \"$(date +%s)\" \
         >> '{seats}/'\"$f\"'/state.jsonl'; fi;;\n*) exit 1;;\nesac\nexit 0\n",
        args = args.display(),
        launched = launched.display(),
        seats = seats.display(),
        front = front.display(),
        caller = caller.display(),
    );
    fs::write(bin.join("tmux"), tmux).ok();
    fs::set_permissions(bin.join("tmux"), fs::Permissions::from_mode(0o755)).ok();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    (place, path)
}

/// 偽 tmux だけの PATH で `seat` を 1 回撃つ（`--tmux-socket` は渡さない＝PATH の偽 tmux が答える・anchor は置き場の anchor）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn launch_group_run(place: &AcctPlace, path: &str, head: &[&str]) -> Output {
    let (state, anchor) = (place.state.display().to_string(), launch_anchor(place));
    Command::new(bin())
        .args(head)
        .args(["--target", GROUP_TARGET, "--state-dir", &state, "--anchor", &anchor])
        .env("PATH", path)
        .output()
        .expect("binary を起動できる")
}

/// 長い形（`seat launch --role orchestrator`・`extra` は `--account` など）。
fn launch_group_long(place: &AcctPlace, path: &str, extra: &[&str]) -> Output {
    let mut head = vec!["seat", "launch", "--role", "orchestrator"];
    head.extend_from_slice(extra);
    launch_group_run(place, path, &head)
}

/// 偽 tmux が写した argv のうち `verb` で始まる呼出しの数（`-S` を渡さない形＝1 語目が verb）。
fn launch_group_tmux_calls(place: &AcctPlace, verb: &str) -> usize {
    fs::read_to_string(place.dir.join(LAUNCH_TMUX_ARGS))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.split_whitespace().next() == Some(verb))
        .count()
}

/// 起こした周の 3 面: rc 0・成立の行の口座は `label`（口座の dir が無い fixture なので末尾は `trust=unwritable`）・起動行はその
/// 口座の dir を運び、最新の登録 row の口座も `label`。
fn launch_group_assert_launched(place: &AcctPlace, out: &Output, label: &str, case: &str) {
    let line = stdout_of(out);
    assert_eq!(rc_of(out), i32::from(RC_OK), "{case}: stdout={line} stderr={}", stderr_of(out));
    assert_eq!(line, format!("seat launch: launched target=gl_seat account={label}{} trust=unwritable\n", provenance(&place.state, "flag")), "{case}");
    let sent = fs::read_to_string(place.dir.join(GROUP_LAUNCHED)).unwrap_or_default();
    let dir = place.state.join("accounts").join(label).display().to_string();
    assert!(sent.lines().last().is_some_and(|found| found.contains(&format!("CLAUDE_CONFIG_DIR={dir} "))), "{case}: {sent}");
    assert_eq!(acct_rows(&place.state).last().map(|row| row.account.clone()), Some(label.to_owned()), "{case}: 登録 row");
}

/// (解決値で起きる) 群の置き場の anchor は、口座の実測が 1 行も無い（session 用の選定なら `no-account` で断る）置き場でも選定を
/// 撃たず群の今の口座で起きる: 記録 l2 の周は l2（記録 > 種）・記録の無い周は種 l1。短い形も解決値と同じ label なら起きる。
#[test]
fn seat_launch_group_anchor_launches_with_the_resolved_account_without_selection() {
    for (record, want) in [(Some("l2"), "l2"), (None, "l1")] {
        let (place, path) = launch_group_place(false, record);
        let out = launch_group_long(&place, &path, &[]);
        launch_group_assert_launched(&place, &out, want, &format!("長い形・記録 {record:?}"));
        let short = launch_group_run(&place, &path, &["seat", want]);
        launch_group_assert_launched(&place, &short, want, &format!("短い形・記録 {record:?}"));
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// (違う label) 群の今の口座が l2（記録）の置き場で、長い形の `--account l1` と短い形の `seat l1` は `group-account` で断る
/// （rc 1・stdout 空・1 key も送らず・登録 row 0・`inject.jsonl` に launch の行なし）。
#[test]
fn seat_launch_group_other_label_is_refused_without_a_row() {
    let (place, path) = launch_group_place(false, Some("l2"));
    for (case, head) in [("長い形", vec!["seat", "launch", "--role", "orchestrator", "--account", "l1"]), ("短い形", vec!["seat", "l1"])] {
        let out = launch_group_run(&place, &path, &head);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{case}: stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
        assert!(stdout_of(&out).is_empty(), "{case}: stdout は空");
        assert_eq!(tick_token(&stderr_of(&out), "reason").as_deref(), Some("group-account"), "{case}: {}", stderr_of(&out));
        assert!(!place.dir.join(GROUP_LAUNCHED).exists(), "{case}: 起動行は送らない");
        assert_eq!(launch_group_tmux_calls(&place, "send-keys"), 0, "{case}: 1 key も送らない");
        assert!(acct_rows(&place.state).is_empty(), "{case}: 登録 row 0");
        assert!(launch_inject_rows(&place).is_empty(), "{case}: inject.jsonl に launch の行なし");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// (群の外) 群が別の置き場（`/elsewhere`）だけを持つ host では、この anchor は今のまま: 群の記録（l2）が在っても `--account l1` は
/// l1 で起き、`--account` 無しは session 用の選定へ進んで実測の無い置き場を `no-account` で断る。
#[test]
fn seat_launch_group_outside_anchor_is_unchanged() {
    let (place, path) = launch_group_place(true, Some("l2"));
    let out = launch_group_long(&place, &path, &["--account", "l1"]);
    launch_group_assert_launched(&place, &out, "l1", "群の外の --account l1");
    let picked = launch_group_long(&place, &path, &[]);
    assert_eq!(rc_of(&picked), i32::from(RC_REFUSED), "stdout={} stderr={}", stdout_of(&picked), stderr_of(&picked));
    assert_eq!(tick_token(&stderr_of(&picked), "reason").as_deref(), Some("no-account"), "選定へ進む: {}", stderr_of(&picked));
    fs::remove_dir_all(&place.dir).ok();
}

// ───── 席の起動が tick の unit を入れる（seat-heartbeat.md §5 形 2・契約表の行 d・接頭辞 `seat_launch_tick_`・§20 の fixture） ─────
//
// 群の外の置き場（[`launch_group_place`] の `outside`）に偽 tmux と偽 systemctl（引数を 1 行ずつ残す・[`LAUNCH_TICK_FAIL`] が在れば
// `enable` を rc 1 で落とす）を置き、host の面に `[[tick]]`（unit dir と binary は tmp）を足す。unit の字面は契約から組む。

/// 偽 systemctl の呼出の記録。
const LAUNCH_TICK_CALLS: &str = "tick-systemctl-calls";

/// 在れば偽 systemctl の `enable` が rc 1 で落ちる。
const LAUNCH_TICK_FAIL: &str = "tick-systemctl-fail";

/// 起動の歯の置き場（群の外の置き場 + unit dir + binary）。
struct LaunchTick {
    /// 置き場・host の面・偽 tmux。
    place: AcctPlace,
    /// 偽 tmux と偽 systemctl の PATH。
    path: String,
    /// host の面の `unit-dir`。
    units: PathBuf,
    /// host の面の `binary`（在る必要は無い＝unit の字面にだけ載る）。
    binary: String,
}

impl LaunchTick {
    /// service と timer の file 名（契約の字面・`<NAME>-seat-tick-<潰した target>`）。
    fn names(&self) -> [String; 2] {
        [format!("{NAME}-seat-tick-gl_seat.service"), format!("{NAME}-seat-tick-gl_seat.timer")]
    }

    /// 2 file の今の本文（無い file は空）。
    fn bodies(&self) -> [String; 2] {
        self.names().map(|name| fs::read_to_string(self.units.join(name)).unwrap_or_default())
    }

    /// 導出の 2 file の本文（契約の字面・`--rules` 無し・周期は埋め込みの `seat.tick_interval_s` = 60）。
    fn expected(&self) -> [String; 2] {
        let mark = format!("# {NAME} tick-install schema=1");
        [
            format!(
                "{mark}\n[Unit]\nDescription={NAME} seat tick {GROUP_TARGET}\n\n[Service]\nType=oneshot\nExecStart={} seat tick --state-dir {} --target {GROUP_TARGET}\n",
                self.binary,
                self.place.state.display()
            ),
            format!(
                "{mark}\n[Unit]\nDescription={NAME} seat tick timer {GROUP_TARGET}\n\n[Timer]\nOnBootSec=60s\nOnUnitActiveSec=60s\nPersistent=false\n\n[Install]\nWantedBy=timers.target\n"
            ),
        ]
    }

    /// 偽 systemctl の呼出（引数の行・呼んだ順）。
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.place.dir.join(LAUNCH_TICK_CALLS)).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    /// 成立の行（`tail` は置き場の 2 語の後ろ・空なら表の無い host の行そのもの・末尾は口座の dir が無い周の `trust=` の語）。
    fn launched(&self, tail: &str) -> String {
        format!("seat launch: launched target=gl_seat account=l1{}{tail} trust=unwritable\n", provenance(&self.place.state, "flag"))
    }
}

/// 群の外の置き場に偽 systemctl を足し、`declared` なら host の面に `[[tick]]` を足す。
fn launch_tick_place(declared: bool) -> LaunchTick {
    let (place, path) = launch_group_place(true, None);
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$2\" = enable ] && [ -f '{}' ]; then exit 1; fi\nexit 0\n",
        place.dir.join(LAUNCH_TICK_CALLS).display(),
        place.dir.join(LAUNCH_TICK_FAIL).display()
    );
    let systemctl = place.dir.join("group-bin").join("systemctl");
    fs::write(&systemctl, script).ok();
    fs::set_permissions(&systemctl, fs::Permissions::from_mode(0o755)).ok();
    let units = place.dir.join("units");
    let binary = place.dir.join("opt").join(NAME).display().to_string();
    if declared {
        let host = place.state.join(vessel::rules::HOST_MANIFEST);
        let body = fs::read_to_string(&host).unwrap_or_default();
        fs::write(&host, format!("{body}\n[[tick]]\nunit-dir = \"{}\"\nbinary = \"{binary}\"\n", units.display())).ok();
    }
    LaunchTick { place, path, units, binary }
}

/// (入れる) `[[tick]]` の在る面で長い形の起動が立つと、2 file が導出の bytes で unit dir に在り、偽 systemctl が `daemon-reload` →
/// `enable --now <timer>` の順・`tick.jsonl` に `who=seat-tick-install` の 1 行・成立の行の末尾（置き場の 2 語の後ろ）に
/// `tick-unit=installed`。同じ席を短い形で起こし直すと `unchanged`（file の mtime 不変・`enable --now` だけ 1 回増える）。
/// base は行に `tick-unit=` が無く file も 0（RED）。
#[test]
fn seat_launch_tick_installs_the_derived_pair_and_names_it_at_the_tail() {
    let tick = launch_tick_place(true);
    let [service_name, timer_name] = tick.names();
    let out = launch_group_long(&tick.place, &tick.path, &["--account", "l1"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    assert_eq!(stdout_of(&out), tick.launched(" tick-unit=installed"), "起動の行の末尾に 1 語");
    assert_eq!(tick.bodies(), tick.expected(), "導出の bytes");
    assert_eq!(tick.calls(), ["--user daemon-reload".to_owned(), format!("--user enable --now {timer_name}")], "reload → enable の順");
    let tick_jsonl = fs::read_to_string(seat_dir_of(&tick.place.state, "gl_seat").join("tick.jsonl")).unwrap_or_default();
    let installs: Vec<&str> = tick_jsonl.lines().filter(|line| acct_text(line, "who").as_deref() == Some("seat-tick-install")).collect();
    assert_eq!(installs.len(), 1, "記録 1 行: {tick_jsonl}");
    let mtimes = || tick.names().map(|name| fs::metadata(tick.units.join(name)).and_then(|meta| meta.modified()).ok());
    let before = mtimes();
    assert!(before.iter().all(Option::is_some), "2 file が在る: {service_name}");
    let again = launch_group_run(&tick.place, &tick.path, &["seat", "l1"]);
    assert_eq!(rc_of(&again), i32::from(RC_OK), "stdout={} stderr={}", stdout_of(&again), stderr_of(&again));
    assert_eq!(stdout_of(&again), tick.launched(" tick-unit=unchanged"), "短い形も同じ 1 本");
    assert_eq!(mtimes(), before, "file は書き直さない");
    assert_eq!(tick.calls().get(2..), Some(&[format!("--user enable --now {timer_name}")][..]), "enable だけ 1 回");
    fs::remove_dir_all(&tick.place.dir).ok();
}

/// (表の無い host) `[[tick]]` の無い面の起動の行は従来の字面のまま（`tick-unit=` が無い）・unit dir は作られず・systemctl は 0 回。
#[test]
fn seat_launch_tick_absent_table_leaves_the_line_and_the_units_untouched() {
    let tick = launch_tick_place(false);
    for head in [vec!["seat", "launch", "--role", "orchestrator", "--account", "l1"], vec!["seat", "l1"]] {
        let out = launch_group_run(&tick.place, &tick.path, &head);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{head:?}: stderr={}", stderr_of(&out));
        assert_eq!(stdout_of(&out), tick.launched(""), "{head:?}: 行は 1 字も変わらない");
    }
    assert!(!tick.units.exists(), "unit dir を作らない");
    assert!(tick.calls().is_empty(), "systemctl 0 回");
    fs::remove_dir_all(&tick.place.dir).ok();
}

/// (立たない周は撃たない) `[[tick]]` の在る面でも、起動が断られる周（前面が shell でない `not-a-shell`・短い形と長い形）と候補の
/// 無い周（`--account` 無しの選定が `no-account`）は unit を入れない: file 0・systemctl 0 回・断りの行に `tick-unit=` が無い。
#[test]
fn seat_launch_tick_refused_or_no_candidate_launch_installs_nothing() {
    let tick = launch_tick_place(true);
    fs::write(tick.place.dir.join(GROUP_FRONT), "claude\n").ok();
    for (case, head, reason) in [
        ("短い形", vec!["seat", "l1"], "not-a-shell"),
        ("長い形", vec!["seat", "launch", "--role", "orchestrator", "--account", "l1"], "not-a-shell"),
        ("候補なし", vec!["seat", "launch", "--role", "orchestrator"], "no-account"),
    ] {
        let out = launch_group_run(&tick.place, &tick.path, &head);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{case}: stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
        assert_eq!(tick_token(&stderr_of(&out), "reason").as_deref(), Some(reason), "{case}: {}", stderr_of(&out));
        assert!(!stderr_of(&out).contains("tick-unit="), "{case}: {}", stderr_of(&out));
    }
    assert!(!tick.units.exists(), "file 0");
    assert!(tick.calls().is_empty(), "systemctl 0 回");
    fs::remove_dir_all(&tick.place.dir).ok();
}

/// (断りは起動の rc を変えない) 偽 systemctl の `enable` が落ちる周は `tick-unit=refused:enable-failed` を行の末尾に足し、rc は起動の
/// rc（0）のまま・stdout の成立の行のまま（席は立っている）。
#[test]
fn seat_launch_tick_failed_enable_is_named_without_changing_the_launch_rc() {
    let tick = launch_tick_place(true);
    fs::write(tick.place.dir.join(LAUNCH_TICK_FAIL), "").ok();
    let out = launch_group_long(&tick.place, &tick.path, &["--account", "l1"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    assert_eq!(stdout_of(&out), tick.launched(" tick-unit=refused:enable-failed"));
    let timer_name = &tick.names()[1];
    assert_eq!(tick.calls(), ["--user daemon-reload".to_owned(), format!("--user enable --now {timer_name}")], "reload → enable（落ちる）");
    fs::remove_dir_all(&tick.place.dir).ok();
}

// ───── 断りの次の 1 手（account-lifecycle.md §21 形 3・契約表の行 j・接頭辞 `seat_launch_group_next_`・§20 の fixture） ─────

/// 断りの 1 行の全体（`next` が在れば target の後・置き場の 2 語の前）。器の字面を借りない。
fn launch_group_refused_line(place: &AcctPlace, reason: &str, next: Option<&str>) -> String {
    let next = next.map(|found| format!(" next={found}")).unwrap_or_default();
    format!("seat launch: refused reason={reason} target=gl_seat{next}{}\n", provenance(&place.state, "flag"))
}

/// (次の 1 手) 群の今の口座が l2（記録）の置き場で、短い形の `seat l1` と長い形の `--account l1` の断りの行は
/// `next=seat l2 -c` を置き場の 2 語の前に持つ（行の全体を 1 字ずつ比べる＝位置も pin）。rc 1・stdout 空・登録 row 0・
/// 1 key も送らない。記録の無い置き場（種 l1）で `seat l2` を撃つと `next=seat l1 -c`（解決値を写す＝記録の label を焼く変異を
/// 捕まえる）。
#[test]
fn seat_launch_group_next_names_the_current_account_before_the_place_words() {
    for (record, other, current) in [(Some("l2"), "l1", "l2"), (None, "l2", "l1")] {
        let (place, path) = launch_group_place(false, record);
        let next = format!("seat {current} -c");
        for (case, head) in
            [("短い形", vec!["seat", other]), ("長い形", vec!["seat", "launch", "--role", "orchestrator", "--account", other])]
        {
            let out = launch_group_run(&place, &path, &head);
            assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{case}: stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
            assert!(stdout_of(&out).is_empty(), "{case}: stdout は空");
            assert_eq!(stderr_of(&out), launch_group_refused_line(&place, "group-account", Some(&next)), "{case}・記録 {record:?}");
            assert_eq!(launch_group_tmux_calls(&place, "send-keys"), 0, "{case}: 1 key も送らない");
            assert!(acct_rows(&place.state).is_empty(), "{case}: 登録 row 0");
        }
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// (他の断りは不変) 群の置き場で前面が shell でない窓の `not-a-shell` の行は従来の `next=`（窓の席を終わらせる手）のままで
/// `seat ` の手を持たず、読めない記録の `group-record-unreadable` と群の外の置き場の `account-unknown` の行は `next=` を
/// 持たない（行の全体を 1 字ずつ比べる）。
#[test]
fn seat_launch_group_next_leaves_other_refusals_unchanged() {
    let (place, path) = launch_group_place(false, None);
    fs::write(place.dir.join(GROUP_FRONT), "claude\n").ok();
    let out = launch_group_run(&place, &path, &["seat", "l1"]);
    let shell = "その窓の席を終わらせてから同じ窓で打つ／別の名の窓を--targetで名指す";
    assert_eq!(stderr_of(&out), launch_group_refused_line(&place, "not-a-shell", Some(shell)), "not-a-shell は従来の行");
    fs::remove_dir_all(&place.dir).ok();
    let (broken, path) = launch_group_place(false, Some("l2"));
    let record = broken.dir.join(format!("{NAME}-host")).join("groups").join("g.account");
    fs::write(&record, "account=l2\n").ok();
    let out = launch_group_run(&broken, &path, &["seat", "l1"]);
    assert_eq!(stderr_of(&out), launch_group_refused_line(&broken, "group-record-unreadable", None), "読めない記録は next 無し");
    fs::remove_dir_all(&broken.dir).ok();
    let (outside, path) = launch_group_place(true, Some("l2"));
    let out = launch_group_long(&outside, &path, &["--account", "zz"]);
    assert_eq!(stderr_of(&out), launch_group_refused_line(&outside, "account-unknown", None), "群の外の断りは next 無し");
    fs::remove_dir_all(&outside.dir).ok();
}

// ───── 口座 × anchor の trust を起動の前に置く（host-init.md §7・契約表の行 e・ADR-0065・接頭辞 `seat_launch_trust_`） ─────

/// 口座 `l2` の設定 file（置き場の `accounts/l2/.claude.json`）。
fn trust_file(place: &AcctPlace) -> PathBuf {
    place.state.join("accounts").join("l2").join(".claude.json")
}

/// 実物の `.claude.json` の書式（2 空白の入れ子・末尾改行あり）の本文: 他の key（30 桁の数を含む）と別 anchor の項目を持ち、
/// `extra` を projects の末尾に足す（空なら足さない）。
fn trust_body(extra: &str) -> String {
    format!(
        "{{\n  \"numStartups\": 12,\n  \"projects\": {{\n    \"/elsewhere\": {{\n      \"allowedTools\": [],\n      \
         \"hasTrustDialogAccepted\": false\n    }}{extra}\n  }},\n  \"userID\": \"abc\",\n  \"big\": 123456789012345678901234567890\n}}\n"
    )
}

/// 当該 anchor の印の項目（[`trust_body`] の `extra` の形・器の字面を借りない）。
fn trust_entry(place: &AcctPlace) -> String {
    format!(",\n    \"{}\": {{\n      \"hasTrustDialogAccepted\": true\n    }}", launch_anchor(place))
}

/// 群の外の置き場（§20 の fixture・偽 tmux だけで本物の tmux を立てない）で口座 l2 の dir を作り、`body` の本文を `.claude.json`
/// に置いて（`None` は置かない）、`before` の後に長い形の `--account l2` で起こす。
fn trust_launch(body: impl Fn(&AcctPlace) -> Option<String>, before: impl Fn(&AcctPlace)) -> (AcctPlace, Output) {
    let (place, path) = launch_group_place(true, None);
    fs::create_dir_all(place.state.join("accounts").join("l2")).ok();
    if let Some(body) = body(&place) {
        fs::write(trust_file(&place), body).ok();
    }
    before(&place);
    let out = launch_group_long(&place, &path, &["--account", "l2"]);
    (place, out)
}

/// 起動行が l2 の口座の dir を運んで 1 本届き、stdout の 1 行の末尾と inject の記録の `what` の末尾が `trust=<word>`。
fn trust_assert_launched(place: &AcctPlace, out: &Output, word: &str) {
    let line = stdout_of(out);
    assert_eq!(rc_of(out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(out));
    assert!(line.starts_with("seat launch: launched ") && line.ends_with(&format!(" trust={word}\n")), "{line}");
    let sent = fs::read_to_string(place.dir.join(GROUP_LAUNCHED)).unwrap_or_default();
    let dir = place.state.join("accounts").join("l2").display().to_string();
    assert!(sent.lines().count() == 1 && sent.contains(&format!("CLAUDE_CONFIG_DIR={dir} ")), "起動行は送られる: {sent}");
    let injected = launch_inject_rows(place);
    assert_eq!(injected.len(), 1, "{injected:?}");
    assert!(injected.first().is_some_and(|(_, what)| what.ends_with(&format!(" trust={word}"))), "記録の what の末尾: {injected:?}");
}

/// 他の key と別 anchor の項目を持ち当該 anchor の無い file の周は、当該 anchor の印だけが projects の末尾に増え（他の key・並び・
/// 値・末尾改行は同じ＝全文が一致）、parse で当該の項目を除けば元と同じ木・`trust=written`。base は file が変わらない（RED）。
#[test]
fn seat_launch_trust_writes_only_the_mark_and_names_it_at_the_tail() {
    use vessel::fleet::json_tree::{parse, Tree};
    let (place, out) = trust_launch(|_| Some(trust_body("")), |_| {});
    trust_assert_launched(&place, &out, "written");
    let written = fs::read_to_string(trust_file(&place)).unwrap_or_default();
    assert_eq!(written, trust_body(&trust_entry(&place)), "当該の印だけが増える");
    let anchor = launch_anchor(&place);
    let strip = |tree: Tree| match tree {
        Tree::Object(pairs) => Tree::Object(
            pairs
                .into_iter()
                .map(|(key, value)| match value {
                    Tree::Object(items) if key == "projects" => (key, Tree::Object(items.into_iter().filter(|(at, _)| *at != anchor).collect())),
                    other => (key, other),
                })
                .collect(),
        ),
        other => other,
    };
    assert_eq!(parse(&written).map(strip), parse(&trust_body("")), "他の key・並び・値は parse で同じ木");
    assert_eq!(fs::read_dir(place.state.join("accounts").join("l2")).map(Iterator::count).unwrap_or_default(), 1, "一時 file は残らない");
    fs::remove_dir_all(&place.dir).ok();
}

/// 同じ窓へ置き換える周（約束 7・偽 tmux が呼び手の target に同じ字面を返す）も分岐の前に印を置く: key は 1 つも送らず器の行も
/// 出ず、置き換えた先（偽 claude）が起動時に写した file は既に印を持ち、exec の前に書く記録の `what` の末尾は `trust=written`。
/// base は記録に `trust=` が無く file も変わらない（RED）。
#[test]
fn seat_launch_trust_same_window_marks_before_the_exec() {
    let (place, path) = launch_group_place(true, None);
    fs::write(place.dir.join(GROUP_CALLER), GROUP_TARGET).ok();
    let (claude, seen) = (place.dir.join("group-bin").join("claude"), place.dir.join("claude-saw"));
    fs::write(&claude, format!("#!/bin/sh\ncat '{}' > '{}'\n", trust_file(&place).display(), seen.display())).ok();
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).ok();
    fs::create_dir_all(place.state.join("accounts").join("l2")).ok();
    fs::write(trust_file(&place), trust_body("")).ok();

    let out = launch_group_long(&place, &path, &["--account", "l2"]);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    assert!(!stdout_of(&out).contains("seat launch:"), "置き換えの後に器の行は出ない: {}", stdout_of(&out));
    assert_eq!(launch_group_tmux_calls(&place, "send-keys"), 0, "1 key も送らない");
    assert_eq!(fs::read_to_string(&seen).unwrap_or_default(), trust_body(&trust_entry(&place)), "exec の先が起動時に読む file に印");
    let injected = launch_inject_rows(&place);
    assert!(injected.len() == 1 && injected.iter().all(|(_, what)| what.ends_with(" trust=written")), "exec の前の記録: {injected:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// 既に true の周は 1 byte も書かない（全文も mtime も同じ）・`trust=accepted`。
#[test]
fn seat_launch_trust_already_true_writes_nothing() {
    let mtime = |place: &AcctPlace| fs::metadata(trust_file(place)).and_then(|found| found.modified()).ok();
    let seen = std::cell::Cell::new(None);
    let (place, out) = trust_launch(|place| Some(trust_body(&trust_entry(place))), |place| seen.set(mtime(place)));
    trust_assert_launched(&place, &out, "accepted");
    assert_eq!(fs::read_to_string(trust_file(&place)).unwrap_or_default(), trust_body(&trust_entry(&place)), "全文は同じ");
    assert!(seen.get().is_some() && mtime(&place) == seen.get(), "mtime も動かない");
    fs::remove_dir_all(&place.dir).ok();
}

/// file が無い周は `projects` だけの最小の file（2 空白・末尾改行あり）を作って印を置く・`trust=created`。
#[test]
fn seat_launch_trust_missing_file_is_created_minimal() {
    let (place, out) = trust_launch(|_| None, |_| {});
    trust_assert_launched(&place, &out, "created");
    let want = format!("{{\n  \"projects\": {{\n    \"{}\": {{\n      \"hasTrustDialogAccepted\": true\n    }}\n  }}\n}}\n", launch_anchor(&place));
    assert_eq!(fs::read_to_string(trust_file(&place)).unwrap_or_default(), want, "projects だけの最小の file");
    fs::remove_dir_all(&place.dir).ok();
}

/// JSON でない file の周は 1 byte も変えず `trust=unreadable`・起動行は送られる（起動は止めない）。
#[test]
fn seat_launch_trust_unreadable_file_is_left_and_the_launch_goes_on() {
    let (place, out) = trust_launch(|_| Some("not json\n".to_owned()), |_| {});
    trust_assert_launched(&place, &out, "unreadable");
    assert_eq!(fs::read_to_string(trust_file(&place)).unwrap_or_default(), "not json\n", "変えない");
    fs::remove_dir_all(&place.dir).ok();
}

/// 口座の dir が読み取り専用の周は一時 file を書けず、file を変えず `trust=unwritable`・起動行は送られる。
#[test]
fn seat_launch_trust_read_only_dir_is_unwritable_and_the_launch_goes_on() {
    let dir = |place: &AcctPlace| place.state.join("accounts").join("l2");
    let lock = |place: &AcctPlace| {
        fs::set_permissions(dir(place), fs::Permissions::from_mode(0o555)).ok();
    };
    let (place, out) = trust_launch(|_| Some(trust_body("")), lock);
    fs::set_permissions(dir(&place), fs::Permissions::from_mode(0o755)).ok();
    trust_assert_launched(&place, &out, "unwritable");
    assert_eq!(fs::read_to_string(trust_file(&place)).unwrap_or_default(), trust_body(""), "変えない");
    assert_eq!(fs::read_dir(dir(&place)).map(Iterator::count).unwrap_or_default(), 1, "一時 file は残らない");
    fs::remove_dir_all(&place.dir).ok();
}

// ───── 引数の無い seat launch（host-init.md §5・契約表の行 c・`s2-07l.614`・接頭辞 `seat_launch_default_`） ─────
//
// §20 の群の置き場（群の anchor = 置き場の anchor・候補 l1 → l2・記録なし＝種 l1）の anchor の dir を git の repo にし、local 設定
// `<NAME>.stateDir` を置き場へ向ける。偽 tmux は本物を使わず、窓 `orchestrator` を在る窓として返し、`new-window` を受ける。
// 字面は契約から組む（実装の helper を使わない）。

/// 既定の歯の repo の dir 名（既定の target の session）。
const DEFAULT_REPO: &str = "anchor";

/// 既定の歯の置き場と偽 tmux の PATH（`list-windows` は `orchestrator` を返し `new-window` は rc 0・他の口は §20 と同じ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn launch_default_place() -> (AcctPlace, String) {
    let (place, path) = launch_group_place(false, None);
    let repo = place.dir.join(DEFAULT_REPO);
    crate::git_repo_at(&repo).expect("anchor を git の repo にできる");
    let key = format!("{NAME}.stateDir");
    crate::git_out(&repo, &["config", "--local", &key, &place.state.display().to_string()]).expect("local 設定を書ける");
    let top = crate::git_out(&repo, &["rev-parse", "--show-toplevel"]);
    assert_eq!(top, Some(launch_anchor(&place)), "repo の root は群の anchor と同じ字面");
    let (args, launched, seats) = (place.dir.join(LAUNCH_TMUX_ARGS), place.dir.join(GROUP_LAUNCHED), place.state.join("seat"));
    let tmux = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{args}'\nt=''; p=''\nfor a in \"$@\"; do [ \"$p\" = '-t' ] && t=\"$a\"; p=\"$a\"; done\n\
         f=$(printf '%s' \"$t\" | tr ':' '_')\ncase \"$1\" in\n\
         has-session) exit 0;;\nlist-windows) echo orchestrator;;\nnew-window) exit 0;;\nlist-panes) echo bash;;\n\
         capture-pane) printf '$ \\n';;\n\
         send-keys) if [ \"$4\" = \"-l\" ]; then printf '%s\\n' \"$5\" >> '{launched}'\n\
         elif [ \"$4\" = \"Enter\" ]; then mkdir -p '{seats}/'\"$f\"\n\
         printf '{{\"schema\":1,\"state\":\"idle\",\"event\":\"SessionStart\",\"ts\":%s,\"sid\":\"\"}}\\n' \"$(date +%s)\" \
         >> '{seats}/'\"$f\"'/state.jsonl'; fi;;\n*) exit 1;;\nesac\nexit 0\n",
        args = args.display(),
        launched = launched.display(),
        seats = seats.display(),
    );
    fs::write(place.dir.join("group-bin").join("tmux"), tmux).expect("偽 tmux を書ける");
    (place, path)
}

/// 偽 tmux の PATH・cwd `cwd` で `seat launch` と `extra` だけを撃つ（置き場も役割も target も渡さない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn launch_default_run(cwd: &Path, path: &str, extra: &[&str]) -> Output {
    Command::new(bin()).args(["seat", "launch"]).args(extra).current_dir(cwd).env("PATH", path).output().expect("binary を起動できる")
}

/// 起こした周: rc 0・成立の 1 行の全体（target・種の口座 l1・置き場の出所 `source`）・登録 row は 1 件で役割 orchestrator・
/// anchor = repo・target = `target`・口座 l1・起動行は l1 の口座の dir を運ぶ。
fn launch_default_assert_launched(place: &AcctPlace, out: &Output, target: &str, source: &str, case: &str) {
    let line = stdout_of(out);
    assert_eq!(rc_of(out), i32::from(RC_OK), "{case}: stdout={line} stderr={}", stderr_of(out));
    let flat = target.replace(':', "_");
    assert_eq!(line, format!("seat launch: launched target={flat} account=l1{} trust=unwritable\n", provenance(&place.state, source)), "{case}");
    let rows: Vec<_> = acct_rows(&place.state).into_iter().map(|row| (row.role, row.anchor, row.target, row.account)).collect();
    let want = (vessel::seat::role::Role::Orchestrator, launch_anchor(place), target.to_owned(), "l1".to_owned());
    assert_eq!(rows.last(), Some(&want), "{case}: 登録 row");
    let sent = fs::read_to_string(place.dir.join(GROUP_LAUNCHED)).unwrap_or_default();
    let dir = place.state.join("accounts").join("l1").display().to_string();
    assert!(sent.lines().last().is_some_and(|found| found.contains(&format!("CLAUDE_CONFIG_DIR={dir} "))), "{case}: {sent}");
    let calls = fs::read_to_string(place.dir.join(LAUNCH_TMUX_ARGS)).unwrap_or_default();
    let session = target.split(':').next().unwrap_or_default();
    assert!(calls.lines().any(|found| found == format!("has-session -t ={session}")), "{case}: session {session} を確かめる: {calls}");
}

/// (1) 引数の無い `seat launch` は cwd の repo の local 設定から置き場（`source=git-config`）を、役割 orchestrator を、
/// `<repo の dir 名>:orchestrator` を target に、群の今の口座（種 l1）を口座に解いて登録 row を書き席を起こす。`--role orchestrator`
/// だけを明示した周も target は同じ。base は引数の無い `seat launch` を使い方で断る（RED）。
#[test]
fn seat_launch_default_resolves_the_place_the_role_and_the_target_from_the_cwd_repo() {
    let want = format!("{DEFAULT_REPO}:orchestrator");
    for (case, extra) in [("引数無し", &[][..]), ("--role だけ", &["--role", "orchestrator"][..])] {
        let (place, path) = launch_default_place();
        let out = launch_default_run(&place.dir.join(DEFAULT_REPO), &path, extra);
        launch_default_assert_launched(&place, &out, &want, "git-config", case);
        assert_eq!(acct_rows(&place.state).len(), 1, "{case}: row は 1 件");
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// (1) 明示の引数は既定に勝つ: `--target other:win` は役割名に依らずその target・`--state-dir` は local 設定に勝ち出所は `flag`。
#[test]
fn seat_launch_default_explicit_flags_win_over_the_defaults() {
    let (place, path) = launch_default_place();
    let repo = place.dir.join(DEFAULT_REPO);
    let out = launch_default_run(&repo, &path, &["--target", "other:win"]);
    launch_default_assert_launched(&place, &out, "other:win", "git-config", "--target");
    let state = place.state.display().to_string();
    let out = launch_default_run(&repo, &path, &["--state-dir", &state]);
    launch_default_assert_launched(&place, &out, &format!("{DEFAULT_REPO}:orchestrator"), "flag", "--state-dir");
    fs::remove_dir_all(&place.dir).ok();
}

/// (1) 既定を解けない周は `defaults-unresolved` で解けない名を宣言順に `missing=` に載せ、長い形の判定と断りの字面は変わらない:
/// 値欠け・空文字・未知の役割・`S:W` でない target は使い方・3 つとも明示で anchor を解けない周は `anchor-unresolvable` の従来の行。
/// どの周も row を書かず tmux を 1 度も撃たない。
#[test]
fn seat_launch_default_unresolved_names_the_missing_flags_and_long_form_refusals_are_unchanged() {
    let (place, path) = launch_default_place();
    let plain = place.dir.join("plain");
    fs::create_dir_all(&plain).ok();
    let bare = place.dir.join("bare");
    assert!(crate::git_repo_at(&bare).is_some(), "設定の無い repo を作れる");
    let state = place.state.display().to_string();
    let unresolved: [(&str, &Path, Vec<&str>, &str); 4] = [
        ("repo の外", &plain, vec![], "--state-dir,--target"),
        ("repo の外・--state-dir", &plain, vec!["--state-dir", &state], "--target"),
        ("repo の外・--target", &plain, vec!["--target", "x:y"], "--state-dir"),
        ("設定の無い repo", &bare, vec!["--role", "orchestrator"], "--state-dir"),
    ];
    for (case, cwd, extra, missing) in unresolved {
        let out = launch_default_run(cwd, &path, &extra);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{case}: stdout={}", stdout_of(&out));
        assert!(stdout_of(&out).is_empty(), "{case}: stdout は空");
        assert_eq!(stderr_of(&out), format!("seat launch: refused reason=defaults-unresolved missing={missing}\n"), "{case}");
    }
    let usage = format!("{}\n", vessel::seat::cli::usage());
    let usages: [(&str, Vec<&str>); 4] = [
        ("値欠け", vec!["--role"]),
        ("空文字", vec!["--target", ""]),
        ("未知の役割", vec!["--role", "planner"]),
        ("S:W でない", vec!["--target", "bad"]),
    ];
    for (case, extra) in usages {
        let out = launch_default_run(&place.dir.join(DEFAULT_REPO), &path, &extra);
        assert_eq!((rc_of(&out), stdout_of(&out), stderr_of(&out)), (i32::from(RC_REFUSED), String::new(), usage.clone()), "{case}: 使い方");
    }
    let full = launch_default_run(&plain, &path, &["--state-dir", &state, "--role", "orchestrator", "--target", "x:y"]);
    let line = format!("seat launch: refused reason=anchor-unresolvable target=x_y{}\n", provenance(&place.state, "flag"));
    assert_eq!((rc_of(&full), stderr_of(&full)), (i32::from(RC_REFUSED), line), "3 つとも明示は従来の断り");
    assert!(acct_rows(&place.state).is_empty(), "row を書かない");
    assert!(!place.dir.join(LAUNCH_TMUX_ARGS).exists(), "tmux を 1 度も撃たない");
    fs::remove_dir_all(&place.dir).ok();
}
