// flip-check: moved s2-07l.680
//! usage と allowance の族の歯（接頭辞 `fleet_usage_` / `fleet_allowance_`・設計 docs/design/carry-prep.md §9 行 h・親 `tests/e2e/fleet.rs` の helper を `use super::*` で使う）。

use super::*;

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
