// flip-check: moved s2-07l.680
//! json と記録の族の歯（接頭辞 `fleet_json_` / `fleet_read_` / `fleet_record_` / `fleet_replay_` / `fleet_export_`・設計 docs/design/carry-prep.md §9 行 h・親 `tests/e2e/fleet.rs` の helper を `use super::*` で使う）。

use super::*;

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
