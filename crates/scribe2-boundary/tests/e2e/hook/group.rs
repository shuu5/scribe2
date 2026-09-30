// flip-check: moved s2-07l.679
//! group の族の歯（接頭辞 `hook_group_` / `hook_permission_`・設計 docs/design/carry-prep.md §9 行 g）。

use super::*;

/// `Bash` の周は **stdout ちょうど 1 行**で deny を返し、記録を 1 行残す。
#[test]
fn hook_permission_request_denies_bash_with_one_json_line() {
    let repo = git_repo();
    let state = linked(&repo);
    let before = inject_lines(&state).len();
    let out = run_hook_args(
        &["permission-request", "--state-dir", &state.display().to_string()],
        &tool_payload(&repo, "Bash", "unused"),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "承認の答えは rc 0");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(text.lines().count(), 1, "stdout はちょうど 1 行: {text}");
    assert!(
        text.contains("\"hookEventName\":\"PermissionRequest\""),
        "どの event への答えかを名乗る: {text}"
    );
    // 入れ子を剥がして **parse できること**まで測る（escape が壊れていれば落ちる）。
    let pairs = json_lite::parse_object(&decision_object(text.trim()))
        .unwrap_or_else(|err| panic!("決定 object を parse できる: {err} / {text}"));
    let behavior = pairs.iter().find(|(key, _)| key == "behavior").map(|(_, v)| v.clone());
    assert_eq!(
        behavior,
        Some(json_lite::Value::Str("deny".to_owned())),
        "一律 deny である: {text}"
    );
    let message = match pairs.iter().find(|(key, _)| key == "message").map(|(_, v)| v.clone()) {
        Some(json_lite::Value::Str(found)) => found,
        other => panic!("message が文字列である: {other:?}"),
    };
    assert!(message.starts_with(&format!("{NAME}: ")), "器が名乗る: {message}");
    assert!(
        message.contains("literal path") && message.contains("$(...)"),
        "次の一手（literal path で書き直す）まで返す: {message}"
    );
    let lines = inject_lines(&state);
    assert_eq!(lines.len(), before + 1, "記録は 1 行だけ増える: {lines:?}");
    // `who` だけを見ると、`what` / `when` を別の値へ書き換える変異が生き残る
    // （実測 2026-09-10・lens-43 M-2）。契約が名指した field は全部測る。
    let last = &lines[lines.len() - 1];
    for (key, want) in [
        ("who", "hook:permission-request"),
        ("what", "deny"),
        ("when", "PermissionRequest"),
    ] {
        assert_eq!(
            value_of(last, key),
            Some(json_lite::Value::Str(want.to_owned())),
            "記録の {key}: {lines:?}"
        );
    }
    assert_eq!(
        value_of(last, "tokens"),
        Some(json_lite::Value::Null),
        "数えていない token は 0 でなく null: {lines:?}"
    );
    clean(&[&repo, &state]);
}

/// `Bash` 以外の問いには答えない（**0 byte・rc 0**＝既定の問いへ戻す・FR24）。
///
/// 答えてしまうと、器が引き受ける筋合いの無い承認まで機械が deny する。
#[test]
fn hook_permission_request_is_silent_for_other_tools() {
    let repo = git_repo();
    let state = linked(&repo);
    let before = inject_lines(&state).len();
    for tool in ["Edit", "Write", "WebFetch", ""] {
        let out = run_hook_args(
            &["permission-request", "--state-dir", &state.display().to_string()],
            &tool_payload(&repo, tool, "unused"),
        );
        assert_silent(&out, &format!("{tool} の問いには答えない"));
    }
    // payload が読めない面も同じく黙る（FR24 の「payload 不能」・lens-43 L-3）。
    // `cwd` を持つが tool 名が無い / 壊れている形を撃つ——`cwd` を落とすと process の
    // cwd へ落ちて別 repo を判定するので、ここで測りたいのは tool 名側の欠落である。
    for (why, payload) in [
        ("tool_name が無い", format!("{{\"cwd\":\"{}\"}}", repo.display())),
        (
            "tool_input だけ在る",
            format!("{{\"cwd\":\"{}\",\"tool_input\":{{}}}}", repo.display()),
        ),
        (
            "JSON として壊れている",
            format!("{{\"cwd\":\"{}\",\"tool_name\"", repo.display()),
        ),
    ] {
        let out = run_hook_args(
            &["permission-request", "--state-dir", &state.display().to_string()],
            &payload,
        );
        assert_silent(&out, &format!("payload 不能（{why}）でも答えない"));
    }
    assert_eq!(
        inject_lines(&state).len(),
        before,
        "答えなかった周は記録も残さない"
    );
    clean(&[&repo, &state]);
}

// flip-check: retroactive s2-07l.47
/// 記録に失敗しても **deny は取り消さず**、失敗を stderr へ出す（FR21 は推奨で判定ではない）。
///
/// `inject.jsonl` を **dir** にすると append が open の時点で落ちる。この周でも stdout は
/// ちょうど 1 行の deny・rc 0 のままで、落ちたことは stderr の行になる——`record_lines` を
/// `let _ = append(…)` へ替える変異は、ここで stderr が 0 行になって落ちる。
#[test]
fn hook_permission_request_surfaces_record_failure_on_stderr() {
    let repo = git_repo();
    let state = linked(&repo);
    // `inject_path` は器と同じ path を組む（file 名の字面を歯へ 2 本目として置かない）。
    fs::create_dir(inject_path(&state)).expect("inject.jsonl を dir として作れる");
    let out = run_hook_args(
        &["permission-request", "--state-dir", &state.display().to_string()],
        &tool_payload(&repo, "Bash", "unused"),
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_OK)),
        "記録が落ちても hook は落ちない"
    );
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(text.lines().count(), 1, "stdout はちょうど 1 行: {text}");
    assert!(
        text.contains("\"behavior\":\"deny\""),
        "記録に失敗しても deny は取り消さない: {text}"
    );
    assert!(
        stderr_lines(&out) >= 1,
        "記録の失敗を黙って消さない: {:?}",
        stderr_text(&out)
    );
    clean(&[&repo, &state]);
}

/// marker の無い repo では **`Bash` でも黙る**（他の器と衝突しない・FR24）。
#[test]
fn hook_permission_request_is_silent_outside_vessel() {
    let repo = git_repo();
    let out = run_hook_args(&["permission-request"], &tool_payload(&repo, "Bash", "unused"));
    assert_silent(&out, "marker の無い repo では答えない");
    clean(&[&repo]);
}

/// (1) SessionStart: 種 a1 の 5 時間窓 90（行の値 85 以上）の席は brief の後ろに 1 行を出し、記録 1 行
/// （`what` = `group-pressure`）を残す。
#[test]
fn hook_group_session_start_brief_carries_one_pressure_line() {
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "grpstart", "a1");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(90, 10, 10));
    let before = inject_lines(&place.state).len();
    assert_eq!(group_session_lines(&place, &path), vec![group_line("a1", "5h", 90, 85)], "brief の後ろに 1 行");
    let records = inject_lines(&place.state);
    let grouped = records.iter().skip(before).filter(|line| what_of(line) == "group-pressure").count();
    assert_eq!(grouped, 1, "群の行の記録 1 行: {records:?}");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (2) UserPromptSubmit: 同じ席の追加文脈（stdout）はちょうど 1 行（打刻だけの周の 0 byte に 1 行が足される）。
#[test]
fn hook_group_user_prompt_submit_adds_one_context_line() {
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "grpprompt", "a1");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(90, 10, 10));
    assert_eq!(group_prompt_lines(&place, &path), vec![group_line("a1", "5h", 90, 85)], "追加文脈は 1 行");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (発話 m) 撃つ位置（設計 fleet-event-log.md §13 約束 1 / 7）: prompt を持つ UserPromptSubmit は、stdout の 1 行目が発話の
/// `<NAME> utterance: ts=<ts>`・2 行目が群の行。発話 event は 1 件（ts は 1 行目と同じ）で、席の打刻は Busy。
#[test]
fn hook_utterance_record_line_comes_before_the_group_line() {
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "utterprompt", "a1");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(90, 10, 10));
    let cwd = json_lite::quote(&place.repo.display().to_string());
    let payload = format!("{{\"cwd\":{cwd},\"session_id\":\"sid-group\",\"prompt\":\"群の席への依頼\"}}");
    let out = run_stub_hook(&path, &["user-prompt-submit", "--pane", STUB_PANE], &payload);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc 0: {}", stderr_text(&out));
    assert_eq!(stderr_text(&out), "", "stderr 0 byte");
    let lines: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 2, "発話の行と群の行の 2 行: {lines:?}");
    let ts = lines[0].strip_prefix(&format!("{NAME} utterance: ts=")).unwrap_or_else(|| panic!("1 行目は発話の行: {lines:?}"));
    assert_eq!(lines[1], group_line("a1", "5h", 90, 85), "2 行目は群の行");
    let events = vessel::fleet::store::read_all(&place.state).unwrap_or_else(|errors| panic!("log を読める: {errors:?}"));
    let said: Vec<_> = events.iter().filter(|event| event.kind == vessel::fleet::EventKind::UtteranceReceived).collect();
    assert_eq!(said.iter().map(|event| event.ts.as_str()).collect::<Vec<_>>(), [ts], "発話 event は 1 件で ts は 1 行目と同じ");
    let stamps = fs::read_to_string(state_file(&place.state, "utterprompt")).unwrap_or_default();
    let last = stamps.lines().last().unwrap_or_default();
    assert_eq!(value_of(last, "state"), Some(json_lite::Value::Str("busy".to_owned())), "打刻は Busy: {stamps}");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (3) 群に属さない anchor（群の置き場が別の path）は、逼迫の実測が在っても 2 event とも 0 行（UserPromptSubmit は
/// stdout 0 byte のまま・SessionStart は群の行を足さない）。
#[test]
fn hook_group_anchor_outside_every_group_prints_nothing() {
    let place = group_role_place();
    put_group(&place, "/repo/not-this-one");
    let path = group_seat(&place, "grpoutside", "a1");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(99, 99, 99));
    assert_eq!(group_prompt_lines(&place, &path), Vec::<String>::new(), "UserPromptSubmit は 0 byte");
    assert_eq!(group_session_lines(&place, &path), Vec::<String>::new(), "SessionStart も群の行なし");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (4) 閾値未満（5 時間窓 84・7 日窓 94・モデル別窓 94）は 0 行（鮮度の内側なので子も起こさない＝measuring も無い）。
#[test]
fn hook_group_under_threshold_prints_nothing() {
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "grpunder", "a1");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(84, 94, 94));
    assert_eq!(group_prompt_lines(&place, &path), Vec::<String>::new(), "閾値未満は 0 行");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (5) 鮮度の外（実測なし・古い実測）は hook の中で測らず、器を子として 1 本起こして `usage: measuring` の 1 行だけを出す
/// （古い実測が閾値以上でも逼迫の行は出さない）。子は自席の口座 a1 だけを測る（credential の無い a1 の Unmeasured が
/// 置き場に届き、a2 の行は 1 件も来ない）。
#[test]
fn hook_group_stale_measurement_spawns_one_child_and_says_measuring() {
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "grpstale", "a1");
    let measuring = vec!["usage: measuring account=a1".to_owned()];
    assert_eq!(group_prompt_lines(&place, &path), measuring, "実測なし: measuring の 1 行");
    let rows = |label: &str| -> usize {
        vessel::fleet::store::read_all(&place.state)
            .unwrap_or_default()
            .iter()
            .filter_map(|event| event.allowance.as_ref())
            .filter(|row| row.key().account == label)
            .count()
    };
    let arrived = |label: &str, over: usize| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while rows(label) <= over && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        rows(label) > over
    };
    assert!(arrived("a1", 0), "子が a1 を測った行が置き場に届く");
    assert_eq!(rows("a2"), 0, "子は自席の口座だけを測る");
    put_group_round(&place.state, GROUP_STALE_TS, "a2", &group_windows(99, 99, 99));
    let other = group_seat(&place, "grpstale2", "a2");
    let measuring = vec!["usage: measuring account=a2".to_owned()];
    assert_eq!(group_prompt_lines(&place, &other), measuring, "古い実測（閾値以上）も measuring の 1 行だけ");
    assert!(arrived("a2", 3), "古い実測の口座も子が測り直す（置いた 3 行の後ろに届く）");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (6) 席の登録 row の口座が種と違う周は**自席の口座**を読む: 種 a1 は閾値未満・席の口座 a2 の 5 時間窓 90 → a2 の 1 行
/// （種を読む変異は 0 行になる）。
#[test]
fn hook_group_seat_account_differs_from_the_seed_and_is_read() {
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "grpseat", "a2");
    let now = group_now();
    put_group_round(&place.state, &now, "a1", &group_windows(10, 10, 10));
    put_group_round(&place.state, &now, "a2", &group_windows(90, 10, 10));
    assert_eq!(group_prompt_lines(&place, &path), vec![group_line("a2", "5h", 90, 85)], "自席の口座 a2 の 1 行");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (7) モデル別窓だけが閾値以上（5 時間窓 10・7 日窓 10・モデル別窓 96）の口座も 1 行で、window は model・cap は
/// モデル別窓の行の値（5 時間窓と 7 日窓の歯は model の窓を判定から落とす変異を捕まえない）。
#[test]
fn hook_group_model_window_alone_prints_the_model_window() {
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "grpmodel", "a1");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(10, 10, 96));
    assert_eq!(group_prompt_lines(&place, &path), vec![group_line("a1", "model", 96, 95)], "model の 1 行");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (f・account-lifecycle.md §33・契約表の行 w・接頭辞 `hook_group_model_gate_`) 埋め込みの役割の既定の表示名と**違う** model の
/// モデル別窓だけが 96（5 時間窓 10・7 日窓 10）の席は、逼迫の 1 行を出さず移動を頼む記録も置かない（base は出す ＝ RED）。
#[test]
fn hook_group_model_gate_other_model_window_alone_prints_nothing() {
    let other = ["Opus", "Fable", "Sonnet", "Haiku"].into_iter().find(|name| *name != group_role_model()).unwrap_or("Haiku");
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "grpother", "a1");
    put_group_round_as(&place.state, &group_now(), "a1", &group_windows(10, 10, 96), other);
    assert_eq!(group_prompt_lines(&place, &path), Vec::<String>::new(), "役割の model でない {other} の窓は数えない");
    assert_eq!(group_requests(&place), Vec::<String>::new(), "頼みも置かない");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (8) 窓ごとに別の行の値を当てる: 7 日窓 90（5 時間窓の行 85 なら越える値）は 0 行・7 日窓 96 は 1 行で cap は 95
/// （5 時間窓の行を他の窓に当てる変異を捕まえる）。
#[test]
fn hook_group_caps_differ_per_window() {
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "grpcaps", "a1");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(10, 90, 10));
    assert_eq!(group_prompt_lines(&place, &path), Vec::<String>::new(), "7 日窓 90 は 0 行");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(10, 96, 10));
    assert_eq!(group_prompt_lines(&place, &path), vec![group_line("a1", "7d", 96, 95)], "7 日窓 96 は 1 行");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (頼みを置く) 逼迫を読んだ UserPromptSubmit は群用 dir に移動を頼む記録を 1 file（`ts=` / `account=` / `window=` の 3 行）置く。
/// 別の窓（7 日窓 96）が逼迫する 2 度目の hook（SessionStart）は上書きしない（1 file のまま・本文は 1 度目と同じ＝ts も窓も不変）。
#[test]
fn hook_group_move_pressure_puts_one_request_and_never_overwrites_it() {
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "grpask", "a1");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(90, 10, 10));
    assert_eq!(group_prompt_lines(&place, &path), vec![group_line("a1", "5h", 90, 85)], "逼迫の 1 行");
    assert_eq!(group_requests(&place), vec![format!("{GROUP_NAME}.request")], "頼みは 1 file");
    let first = group_request_body(&place);
    let keys: Vec<&str> = first.lines().filter_map(|line| line.split_once('=').map(|(key, _)| key)).collect();
    assert_eq!(keys, ["ts", "account", "window"], "3 行: {first}");
    assert!(first.contains("\naccount=a1\nwindow=5h\n"), "口座と窓: {first}");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(10, 96, 10));
    assert_eq!(group_session_lines(&place, &path), vec![group_line("a1", "7d", 96, 95)], "2 度目も逼迫の 1 行");
    assert_eq!(group_requests(&place), vec![format!("{GROUP_NAME}.request")], "2 度目も 1 file");
    assert_eq!(group_request_body(&place), first, "上書きしない（ts も窓も 1 度目のまま）");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (頼みを置かない) 閾値未満の周と、群に属さない anchor の逼迫の周は、どちらも移動を頼む記録を 1 file も置かない。
#[test]
fn hook_group_move_under_threshold_or_outside_anchor_puts_no_request() {
    let under = group_role_place();
    put_group(&under, &under.repo.display().to_string());
    let path = group_seat(&under, "grpaskunder", "a1");
    put_group_round(&under.state, &group_now(), "a1", &group_windows(84, 94, 94));
    assert_eq!(group_prompt_lines(&under, &path), Vec::<String>::new(), "閾値未満は 0 行");
    assert_eq!(group_requests(&under), Vec::<String>::new(), "閾値未満は 0 file");
    clean(&[&under.repo, &under.state, &under.sock_dir]);
    let outside = group_role_place();
    put_group(&outside, "/repo/not-this-one");
    let path = group_seat(&outside, "grpaskoutside", "a1");
    put_group_round(&outside.state, &group_now(), "a1", &group_windows(99, 99, 99));
    assert_eq!(group_prompt_lines(&outside, &path), Vec::<String>::new(), "群の外は 0 行");
    assert_eq!(group_session_lines(&outside, &path), Vec::<String>::new(), "SessionStart も 0 行");
    assert_eq!(group_requests(&outside), Vec::<String>::new(), "群の外は 0 file");
    clean(&[&outside.repo, &outside.state, &outside.sock_dir]);
}

/// (食い違い) 記録の口座 a2 ≠ 登録 row の口座 a1 の席は、a1 が逼迫（5 時間窓 90）でも逼迫を測らず、UserPromptSubmit と
/// SessionStart の両方に `row=a1 current=a2` の 1 行だけを出し（`window=` を持たない）、移動を頼む記録を置かない。
#[test]
fn hook_group_current_record_differs_from_the_row_says_moving() {
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "grpmoving", "a1");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(90, 10, 10));
    put_group_record(&place, &group_record("a2", "a1"));
    let prompt = group_prompt_lines(&place, &path);
    assert_eq!(prompt, vec![moving_line("a1", "a2")], "UserPromptSubmit は移動中の 1 行");
    assert!(prompt.iter().all(|line| !line.contains("window=")), "逼迫を測らない: {prompt:?}");
    assert_eq!(group_session_lines(&place, &path), vec![moving_line("a1", "a2")], "SessionStart も移動中の 1 行");
    assert_eq!(group_requests(&place), Vec::<String>::new(), "移動を頼む記録は置かない");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (一致して逼迫) 記録の口座 a2 = 登録 row の口座 a2 で a2 が逼迫の席は「次の 1 周が移り先を決める」の 1 行で、「第 3 段」の
/// 語を持たない（移動を頼む記録は §20 形 4 のとおり置く）。
#[test]
fn hook_group_current_record_matches_the_row_says_the_next_round_decides() {
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "grpmatch", "a2");
    put_group_round(&place.state, &group_now(), "a2", &group_windows(90, 10, 10));
    put_group_record(&place, &group_record("a2", "a1"));
    let lines = group_prompt_lines(&place, &path);
    assert_eq!(lines, vec![group_line("a2", "5h", 90, 85)], "逼迫の 1 行");
    assert!(lines.iter().all(|line| line.contains("次の 1 周が移り先を決める") && !line.contains("第 3 段")), "{lines:?}");
    assert_eq!(group_requests(&place), vec![format!("{GROUP_NAME}.request")], "頼みは置く");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (読めない記録) 記録の形が壊れた席は、登録 row の口座が逼迫でも 2 event とも 0 行（席は止めない・rc 0・頼みも置かない）。
#[test]
fn hook_group_current_unreadable_record_prints_nothing() {
    let place = group_role_place();
    put_group(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "grpbroken", "a1");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(90, 10, 10));
    put_group_record(&place, "account=a2\n");
    assert_eq!(group_prompt_lines(&place, &path), Vec::<String>::new(), "UserPromptSubmit は 0 行");
    assert_eq!(group_session_lines(&place, &path), Vec::<String>::new(), "SessionStart も 0 行");
    assert_eq!(group_requests(&place), Vec::<String>::new(), "頼みも置かない");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

// ─────── park の区画の席の hook（seat-heartbeat.md §21 形 7・契約表の行 z・FR95・接頭辞 `hook_park_`・§19 の fixture） ───────
//
// 区画（Tier9）の席は群と同じ門で逼迫の 1 行（`group=Tier9 …`）を出すが、移動を頼む記録を置かず、鮮度の外でも計測の子を起こさない
// （区画の手は計測を起こさない）。字面は契約から組む。

/// 区画の名。
const PARK_NAME: &str = "Tier9";

/// host の面に口座 a1 / a2 と区画 1 つ（置き場 = `anchor`・候補 = a1 → a2）を書く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put_park(place: &RolePlace, anchor: &str) {
    let body = format!(
        "schema = 1\n\n[[account]]\nlabel = \"a1\"\n\n[[account]]\nlabel = \"a2\"\n\n[[account-group]]\nname = \"{PARK_NAME}\"\n\
         anchors = [\"{anchor}\"]\naccounts = [\"a1\", \"a2\"]\n"
    );
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), body).expect("host の面を書ける");
}

/// 区画の逼迫の 1 行（器の字面を借りない）。
fn park_line(account: &str, window: &str, used: u64, cap: u64) -> String {
    format!("group={PARK_NAME} account={account} window={window} used={used} cap={cap} — 次の 1 周が移り先を決める")
}

/// host の根の群用 dir の直下の file 名（無ければ空）。
fn park_groups_files(place: &RolePlace) -> Vec<String> {
    let dir = place.state.parent().unwrap_or(&place.state).join(format!("{}-host", vessel::name::NAME)).join("groups");
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| entries.filter_map(Result::ok).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// (i) 区画の席の hook が a1 の逼迫（5 時間窓 90・行の値 85）の fixture で `group=Tier9 account=a1` で始まる 1 行を 2 event とも出し、
/// 群用 dir に移動を頼む記録の file を 1 つも置かない（base は 0 行 ＝ RED）。
#[test]
fn hook_park_pressed_seat_says_one_tier9_line_and_asks_for_nothing() {
    let place = group_role_place();
    put_park(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "parkhot", "a1");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(90, 10, 10));
    assert_eq!(group_prompt_lines(&place, &path), vec![park_line("a1", "5h", 90, 85)], "UserPromptSubmit は 1 行");
    assert_eq!(group_session_lines(&place, &path), vec![park_line("a1", "5h", 90, 85)], "SessionStart も 1 行");
    assert_eq!(park_groups_files(&place), Vec::<String>::new(), "移動を頼む記録は置かない（群用 dir の file 0）");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (j) 閾値未満（5 時間窓 84・7 日窓 94・モデル別窓 94）は 0 行・file 0（回帰の歯・base でも緑）。
#[test]
fn hook_park_under_threshold_prints_nothing() {
    let place = group_role_place();
    put_park(&place, &place.repo.display().to_string());
    let path = group_seat(&place, "parkunder", "a1");
    put_group_round(&place.state, &group_now(), "a1", &group_windows(84, 94, 94));
    assert_eq!(group_prompt_lines(&place, &path), Vec::<String>::new(), "閾値未満は 0 行");
    assert_eq!(group_session_lines(&place, &path), Vec::<String>::new(), "SessionStart も 0 行");
    assert_eq!(park_groups_files(&place), Vec::<String>::new(), "file 0");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (k) 記録なし（a1 の行が置き場に 0 件）と古い記録（鮮度の外の a1 の 3 行）の 2 fixture は 0 行（`usage: measuring` も無い）で、hook の
/// 後に 3 秒待っても置き場の a1 の計測の行が増えない（記録なしは 0 件のまま・古い記録は 3 件のまま＝計測の子の起動 0）。
#[test]
fn hook_park_missing_or_stale_measurement_prints_nothing_and_measures_nothing() {
    for (why, stale, held) in [("記録なし", false, 0), ("古い記録", true, 3)] {
        let place = group_role_place();
        put_park(&place, &place.repo.display().to_string());
        let path = group_seat(&place, "parkstale", "a1");
        if stale {
            put_group_round(&place.state, GROUP_STALE_TS, "a1", &group_windows(99, 99, 99));
        }
        assert_eq!(group_prompt_lines(&place, &path), Vec::<String>::new(), "{why}: UserPromptSubmit は 0 行");
        assert_eq!(group_session_lines(&place, &path), Vec::<String>::new(), "{why}: SessionStart も 0 行");
        std::thread::sleep(std::time::Duration::from_secs(3));
        let rows = vessel::fleet::store::read_all(&place.state)
            .unwrap_or_default()
            .iter()
            .filter_map(|event| event.allowance.as_ref())
            .filter(|row| row.key().account == "a1")
            .count();
        assert_eq!(rows, held, "{why}: 置き場の a1 の行は増えない（計測の子の起動 0）");
        assert_eq!(park_groups_files(&place), Vec::<String>::new(), "{why}: file 0");
        clean(&[&place.repo, &place.state, &place.sock_dir]);
    }
}
