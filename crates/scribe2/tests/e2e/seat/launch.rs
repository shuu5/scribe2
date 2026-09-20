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
        "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={{account_dir}} claude --plugin-dir {} --plugin-dir {} --plugin-dir {} {} {}",
        launch_anchor(place), LAUNCH_PLUGINS[0], LAUNCH_PLUGINS[1], LAUNCH_ARGS[0], LAUNCH_ARGS[1]
    )
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
    assert_eq!(line, format!("seat launch: launched target={name}_seat account=l2{}\n", provenance(&place.state, "flag")));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "window を 1 回作る");
    assert!(
        fs::read_to_string(place.dir.join(LAUNCH_TMUX_ARGS)).unwrap_or_default().lines().any(|found| found.ends_with(&format!("new-window -t ={name}: -n seat"))),
        "`new-window -t <session> -n <window>` の形（session は exact の名 + 次の空き index）: {}",
        fs::read_to_string(place.dir.join(LAUNCH_TMUX_ARGS)).unwrap_or_default()
    );
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_expected_argv(&place, "l2"), "導出した行が 1 回だけ届く");
    launch_assert_registered_before_send(&place, &target, "l2");
    let stamps = fs::read_to_string(state_file(&seat_dir_of(&place.state, &format!("{name}_seat")))).unwrap_or_default();
    assert!(stamps.contains("\"event\":\"SessionStart\""), "席が立った打刻: {stamps}");
    assert!(!place.dir.join(LAUNCH_LOG).exists(), "`--restore` 無しは復元を送らない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) 登録 row は**送る前**に 1 件（偽 claude が起動時に写した event log に既に在る・`sid` 無し・`launch` = 導出した行・
/// account = `label`・鍵 = planner × anchor）・`inject.jsonl` に `kind=launch`（`who=seat-launch`）が 1 行。
fn launch_assert_registered_before_send(place: &AcctPlace, target: &str, label: &str) {
    let rows = acct_rows(&place.state);
    let want = vessel::fleet::Registration {
        role: vessel::seat::role::Role::Orchestrator,
        anchor: launch_anchor(place),
        target: target.to_owned(),
        sid: None,
        account: label.to_owned(),
        launch: launch_derived(place),
        model: None,
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
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_expected_argv(&place, "l2"));
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
/// 起こすと、席の記録の起動行はちょうど `cd '<anchor>' && CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR=<l2> claude …`
/// （cd → agent view の env → claude の順）で、効果でも測る: 起こした偽 claude の cwd（pane の前面 process の cwd）は anchor。
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
        vec![format!("cd '{anchor}' && {}", launch_derived(&place).replace("{account_dir}", &l2_dir))],
        "起動行は row の anchor への cd → agent view の env → claude の順の 1 行"
    );
    assert_eq!(cwd_of(), anchor, "起こした席の cwd は row の anchor（pane の cwd ではない）");
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_expected_argv(&place, "l2"), "argv に cd は載らない");
    launch_assert_registered_before_send(&place, &target, "l2");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (f) 起動行は `--model` を typed に運ぶ（`s2-07l.313`・C2.2 / C10・SRS FR59 / FR36）: `--model Fable`（表示名）で起こすと偽
/// claude の argv は `--model fable`（別名）で**始まり**（`claude` の直後・anchor の `--plugin-dir` より前）、送った行に
/// `--model` の語は 1 つ・登録 row は `model=Fable` を持ち **`launch`（雛形）に `--model` の語は無い**（model 無しの導出行のまま）。
/// 表に無い `--model nope` は `launch-model-unknown` で row も key も書かない。base は `--model` を運ばず黙って settings の
/// model で立てる（RED）。
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
        format!("--model\nfable\n{}", launch_expected_argv(&place, "l2")),
        "別名の `--model fable` が `claude` の直後に 1 回だけ届く"
    );
    let sent = acct_sent(&place.state, &format!("{name}_{name}"));
    assert_eq!(sent.len(), 1, "起動行の 1 行: {sent:?}");
    assert!(sent.first().is_some_and(|what| what.contains(" claude --model fable --plugin-dir ") && launch_model_words(what) == 1), "{sent:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows.first().map(|row| row.model.as_deref()), Some(Some("Fable")), "row の model は表示名のまま: {rows:?}");
    assert_eq!(rows.first().map(|row| row.launch.as_str()), Some(launch_derived(&place).as_str()), "雛形は model 無しの導出行: {rows:?}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) 雛形に literal の `--model` が在る（host の面の `[[launch-arg]]` に `--model` / `opus`＝暫定の再登録の形）周に `--model Fable`
/// で起こすと、器の 1 つと二重になるので後勝ちにせず `launch-model-duplicated` で断る（row も key も書かない）。`--model` 無しなら
/// literal の 1 つだけが載って起こせる（二重**だけ**を断る）。
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

    let single = launch_run(&place, &path, &target, &["--account", "l2"]);
    assert_eq!(rc_of(&single), i32::from(RC_OK), "stdout={} stderr={}", stdout_of(&single), stderr_of(&single));
    let argv = fs::read_to_string(place.dir.join("launched")).unwrap_or_default();
    assert_eq!(argv, launch_expected_argv(&place, "l2").replacen("\nenv:", "\n--model\nopus\nenv:", 1), "literal の 1 つだけが載る: {argv}");
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
    assert_eq!(line, format!("seat launch: launched target={name}_seat account=l2{}\n", provenance(&place.state, "flag")));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "短い形は window を作らない（在る window へ起こす）");
    let injected = launch_inject_rows(&place);
    assert_eq!(injected.len(), 2, "長い形と短い形の kind=launch が 1 行ずつ: {injected:?}");
    assert_eq!(injected.first().map(|(_, what)| what), injected.get(1).map(|(_, what)| what), "注入の記録の what は同一: {injected:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "同じ鍵の row が 2 件: {rows:?}");
    assert_eq!(rows.first(), rows.get(1), "短い形の row は長い形の row と同じ（target / model / account）: {rows:?}");
    assert_eq!(rows.get(1).map(|row| (row.target.as_str(), row.model.as_deref(), row.account.as_str())), Some((target.as_str(), Some("Fable"), "l2")));
    let argv = launch_expected_argv(&place, "l2");
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("--model\nfable\n{argv}--model\nfable\n{argv}"),
        "偽 claude の argv は 2 回とも同じ（row の model を運ぶ）"
    );
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) row が無く flag も無い周は `defaults-unresolved` で typed に断る: rc 1・stderr の行はちょうど
/// `seat launch: refused reason=defaults-unresolved missing=--target,--model`（`target=` を持たない）・0 key・row 0・`inject.jsonl` に
/// launch 行なし。2 周目: `model` を持たない row を長い形（`--model` 無し）で作った place で短い形を `--model` 無しで撃つ →
/// 同じ形で `missing=--model`・0 key（席を終えた後なので送れる状態だが送らない）・row の本数は撃つ前と同じ。
#[test]
fn seat_launch_short_form_refuses_typed_without_a_row() {
    let place = launch_place();
    let name = "launchshortnorow";
    let target = format!("{name}:{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");

    let out = launch_run_short(&place, &path, "l2", &["--orchestrator"]);

    launch_assert_refused_line(&out, "seat launch: refused reason=defaults-unresolved missing=--target,--model", "no-row");
    assert!(!stderr_of(&out).contains("target="), "解けない target を行に置かない: {}", stderr_of(&out));
    launch_assert_not_sent(&place, 0, "no-row");

    let long = launch_run(&place, &path, &target, &["--account", "l2"]);
    assert_eq!(rc_of(&long), i32::from(RC_OK), "長い形（model 無し）: stdout={} stderr={}", stdout_of(&long), stderr_of(&long));
    assert!(launch_quit_seat(&place, &target), "席を終えて前面を shell に戻せる: {}", capture(&place.socket, &target));
    let sent_before = launch_tmux_calls(&place, "send-keys");
    let rows_before = acct_rows(&place.state).len();
    assert_eq!(rows_before, 1, "model を持たない row が 1 件");

    let again = launch_run_short(&place, &path, "l2", &["--orchestrator"]);

    launch_assert_refused_line(&again, "seat launch: refused reason=defaults-unresolved missing=--model", "no-model");
    assert_eq!(launch_tmux_calls(&place, "send-keys"), sent_before, "no-model: 1 key も送らない");
    assert_eq!(acct_rows(&place.state).len(), rows_before, "no-model: row を書かない");
    assert_eq!(launch_inject_rows(&place).len(), 1, "no-model: launch の記録は長い形の 1 行のまま");
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
        format!("--model\nfable\n{}", launch_expected_argv(&place, "l2")),
        "zero: 既定の役割で導出した行が届く"
    );
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 1, "zero: 登録 row は 1 件: {rows:?}");
    assert_eq!(rows.first().map(|row| row.role), Some(vessel::seat::role::Role::Orchestrator), "zero: 既定の役割: {rows:?}");
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
    assert_eq!(line, format!("seat launch: launched target={name}_seat account=l2{}\n", provenance(&place.state, "flag")));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "1 語は window を作らない（row の target の窓へ起こす）");
    let injected = launch_inject_rows(&place);
    assert_eq!(injected.len(), 2, "長い形と 1 語の kind=launch が 1 行ずつ: {injected:?}");
    assert_eq!(injected.first().map(|(_, what)| what), injected.get(1).map(|(_, what)| what), "注入の記録の what は同一: {injected:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "同じ鍵の row が 2 件: {rows:?}");
    assert_eq!(rows.first(), rows.get(1), "1 語の row は長い形の row と同じ（役割 / target / model / 口座）: {rows:?}");
    let argv = format!("--model\nfable\n{}", launch_expected_argv(&place, "l2"));
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), format!("{argv}{argv}"), "偽 claude の argv は 2 回とも同じ");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// 約束 3 / 4（§26）: 登録 row も `--target` も無い周は、**`-t` を付けない** `display-message` の 1 問いで測った session の
/// 名と役割の字面を `:` で繋いだ target で席が立つ。4 周を同じ place で測る: (1) 問いが撃てない（偽 tmux が rc 1）→
/// `missing=--target` (2) session の名が空（空の答え）→ 同じ断り（空を target に化けさせない） (3) 名は返るが row も
/// `--model` も無い → `missing=--model`（本便は model の要求を外さない） (4) `--model` を添えると
/// `<session>:orchestrator` の窓が 1 つ作られて席が立ち、row の target もその字面。**環境変数は 1 つも読まない**
/// （器の `env::` の許し列は 3 つのままで、この経路は tmux への問いだけ）。base は 4 周とも `missing=--target,--model`
/// で断る（RED）。
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
    let no_model = launch_run_short(&place, &path, "l2", &[]);
    launch_assert_refused_line(&no_model, "seat launch: refused reason=defaults-unresolved missing=--model", "no-model");
    launch_assert_not_sent(&place, 0, "no-model");

    let out = launch_run_short(&place, &path, "l2", &["--model", "Fable"]);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={} pane={}", stderr_of(&out), capture(&place.socket, &target));
    assert_eq!(line, format!("seat launch: launched target={name}_orchestrator account=l2{}\n", provenance(&place.state, "flag")));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "役割の名の窓を 1 回作る");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 1, "登録 row は 1 件: {rows:?}");
    assert_eq!(rows.first().map(|row| row.target.as_str()), Some(target.as_str()), "row の target は <測った session>:<役割>: {rows:?}");
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("--model\nfable\n{}", launch_expected_argv(&place, "l2")),
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
        format!("--model\nfable\n{}", launch_expected_argv(&place, "l2")),
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
    assert_eq!(line, format!("seat launch: launched target={name}_seat account=l1{}\n", provenance(&place.state, "flag")));
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
        format!("--model\nfable\n{}--model\nfable\n{}", launch_expected_argv(&place, "l2"), launch_expected_argv(&place, "l1")),
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
    assert_eq!(line, format!("seat launch: launched target={name}_seat account=l2{}\n", provenance(&place.state, "flag")));
    assert_eq!(launch_tmux_calls(&place, "new-window"), 1, "window を 1 回作る");
    assert_eq!(fs::read_to_string(place.dir.join("launched")).unwrap_or_default(), launch_expected_argv(&place, "l2"), "導出した行が 1 回だけ届く");
    launch_assert_registered_before_send(&place, &target, "l2");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}
