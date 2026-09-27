// flip-check: moved s2-07l.681
//! tick の族の歯（接頭辞 `seat_tick_`・設計 docs/design/carry-prep.md §9 行 i）。
//!
//! 共有の helper と const と fixture（isolated seat）・外形 snapshot の歯・tmux の群の歯・動詞の数を固定する歯は親
//! module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。歯の本文は親から**挙動不変で移した**もの
//! （`s2-07l.681`）。

use super::*;

/// (a) 登録 row ∧ 最終行 Idle が 40 分以上前 ∧ 入力欄が空 → 合図 1 行を注入し、梯子の記録 1 行（段 0・基準 null）と注入の記録
/// 1 行（`who`=`seat-inject`）が増える。送った key は text 1 回と Enter 1 回だけ。
#[test]
fn seat_tick_injects_one_signal_into_a_silent_registered_seat() {
    let place = tick_place(true);
    tick_silent_for(&place, TICK_STALE + 60);
    let before = unix_now();
    let out = tick_run(&place, &[]);
    let after = unix_now();
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), tick_inject(0), "判定行 1 行");
    assert_eq!(tick_keys(&place), [tick_text_key(0), format!("send-keys -t {TICK_TARGET} Enter")], "text 1 回 + Enter 1 回");
    let pane = fs::read_to_string(place.at(TICK_PANE)).unwrap_or_default();
    assert_eq!(pane.matches(&tick_signal(0)).count(), 1, "pane に合図 1 行: {pane}");
    let (sent_at, step, digest) = tick_ladder(&place).expect("梯子の記録が 1 行在る");
    assert!((before..=after).contains(&sent_at), "sent_at は送った時刻: {sent_at}");
    assert_eq!((step, digest), (0, None), "段 0・基準 null");
    let injections = tick_injections(&place);
    assert_eq!(injections.len(), 1, "注入の記録 1 行: {injections:?}");
    let line = injections.first().map(String::as_str).unwrap_or_default();
    assert_eq!(acct_text(line, "who").as_deref(), Some("seat-inject"), "既存の注入の経路の who: {line}");
    let what = acct_text(line, "what").unwrap_or_default();
    assert!(what.starts_with(&format!("{NAME} tick: heartbeat step=0")) && tick_signal(0).starts_with(&what), "what は合図の頭: {what}");
    assert!(!place.at(TICK_CLIENT).exists(), "偽 client は呼ばれない");
}

/// (b) 梯子の手前で止まる周（登録 row 無し・打刻無し・読めない・Busy・Busy が 40 分より古い）は理由を判定行に出し、
/// `pointer=- step=-`（評価していない印）で 1 key も送らず記録も増えない。
#[test]
fn seat_tick_refuses_before_the_ladder_without_a_key_or_a_record() {
    let bare = tick_place(false);
    tick_silent_for(&bare, TICK_STALE + 60);
    tick_assert_quiet(&bare, &tick_noop("no-row", "-", "-"));
    let place = tick_place(true);
    let file = state_file(&place.seat());
    fs::remove_file(&file).ok();
    tick_assert_quiet(&place, &tick_noop("state-missing", "-", "-"));
    fs::write(&file, "not json\n").ok();
    tick_assert_quiet(&place, &tick_noop("state-unreadable", "-", "-"));
    fs::remove_file(&file).ok();
    fs::create_dir_all(&file).ok();
    tick_assert_quiet(&place, &tick_noop("state-unreadable", "-", "-"));
    fs::remove_dir(&file).ok();
    let now = unix_now();
    tick_stamps(&place, &[("idle", "Stop", now - 5000), ("busy", "UserPromptSubmit", now - 10)]);
    tick_assert_quiet(&place, &tick_noop("busy", "-", "-"));
    tick_stamps(&place, &[("busy", "UserPromptSubmit", now - TICK_STALE - 60)]);
    tick_assert_quiet(&place, &tick_noop("state-stale", "-", "-"));
    assert!(tick_keys(&place).is_empty() && tick_keys(&bare).is_empty(), "どの周も 0 key");
}

/// (c) 最終行 Idle が 40 分未満前 → `stamp-recent`（梯子は評価済み＝段 0・床は開いて残り 0 秒）で 0 key。
#[test]
fn seat_tick_stamp_recent_sends_nothing() {
    let place = tick_place(true);
    for ago in [100, TICK_STALE - 30] {
        tick_silent_for(&place, ago);
        tick_assert_quiet(&place, &tick_noop("stamp-recent", "wait:0", "0"));
    }
    assert!(tick_keys(&place).is_empty(), "0 key");
}

/// (d) 梯子（設計 §10 形 5・`--rules` は梯子の列の写し）: 送った直後は `settling`・`sent_at` より後の Stop を足すと基準が入り
/// 無変化の床は `wait:<s> step=1`・`sent_at` を列の待ちの分だけ過去に書くと段 1〜5 の合図が出て、7 段目（段 6）は `stopped` で
/// 送らない（合図は列の長さの 6 本で打ち切り）。段 n の待ちは列の n 番目で、その 5 秒手前の周は床の内（`wait`）。
#[test]
fn seat_tick_ladder_climbs_six_signals_then_stops() {
    let place = tick_place(true);
    let rules = fixture(&place.dir, "ladder.toml", &tick_rules_text("", None));
    let run = || tick_run(&place, &["--rules", &rules]);
    tick_silent_for(&place, TICK_STALE + 60);
    assert_eq!(stdout_of(&run()), tick_inject(0), "段 0");
    assert_eq!(stdout_of(&run()), tick_noop("settling", "settling", "0"), "送った直後");
    let sent = unix_now() - 3000;
    tick_ladder_put(&place, sent, 0, None);
    tick_stamps(&place, &[("busy", "UserPromptSubmit", sent + 1), ("idle", "Stop", sent + 2)]);
    let line = stdout_of(&run());
    assert!(line.starts_with(&format!("decision=noop target={TICK_SEAT} reason=wait pointer=wait:")), "{line}");
    assert!(line.ends_with(&format!(" step=1 consumed=-{TICK_NO_MOVE}\n")), "{line}");
    let left = tick_token(&line, "pointer").and_then(|found| found.strip_prefix("wait:").and_then(|secs| secs.parse::<u64>().ok()));
    assert!(left.is_some_and(|secs| (595..=600).contains(&secs)), "残り秒 = 列の 2 番目 3600 − 3000: {line}");
    assert_eq!(tick_ladder(&place), Some((sent, 0, Some(sent + 2))), "基準は sent_at より後の Stop の ts");
    for step in 1..=5 {
        tick_climb(&place, &rules, step, tick_wait(step).expect("段 1〜5 は列の内"));
    }
    let sent = unix_now() - 200_000;
    tick_ladder_put(&place, sent, 5, None);
    tick_stamps(&place, &[("busy", "UserPromptSubmit", sent + 1), ("idle", "Stop", sent + 2)]);
    let keys = tick_keys(&place).len();
    assert_eq!(stdout_of(&run()), tick_noop("stopped", "stopped", "6"), "7 段目（段 6）は打ち切り");
    assert_eq!(tick_keys(&place).len(), keys, "段 6 は 0 key");
    assert_eq!(tick_ladder(&place), Some((sent, 5, Some(sent + 2))), "記録は段 5 のまま（基準だけが入る）");
    let texts: Vec<String> = tick_keys(&place).into_iter().filter(|key| key.contains(" -l ")).collect();
    assert_eq!(texts, (0..=5).map(tick_text_key).collect::<Vec<_>>(), "合図は段 0〜5 の 6 本（列の長さ・段 5 は次が無い）");
    assert_eq!(tick_injections(&place).len(), 6, "注入の記録 6 行");
}

/// (e) 基準の後に最終行の ts が動くと段 0 に戻り、列の先頭の 1800 秒（30 分）黙った周に段 0 で送る（打ち切りの後も同じ）。
/// 1800 秒の手前の周は `stamp-recent` で送らない（埋め込み manifest の `seat.tick_stale_s` = 1800・列の先頭 = 1800）。
#[test]
fn seat_tick_change_returns_to_step_zero_after_thirty_silent_minutes() {
    let place = tick_place(true);
    let now = unix_now();
    let base = now - 20_000;
    tick_ladder_put(&place, base - 10, 3, Some(base));
    tick_stamps(&place, &[("idle", "Stop", base), ("busy", "UserPromptSubmit", now - 120), ("idle", "Stop", now - 100)]);
    tick_assert_quiet(&place, &tick_noop("stamp-recent", "wait:0", "0"));
    tick_stamps(&place, &[("idle", "Stop", base), ("idle", "Stop", now - 1800 + 30)]);
    tick_assert_quiet(&place, &tick_noop("stamp-recent", "wait:0", "0"));
    tick_stamps(&place, &[("idle", "Stop", base), ("idle", "Stop", now - 1800 - 60)]);
    assert_eq!(stdout_of(&tick_run(&place, &[])), tick_inject(0), "変化の後 1800 秒黙った周は段 0");
    let base = now - 200_000;
    tick_ladder_put(&place, base - 10, 5, Some(base));
    tick_stamps(&place, &[("idle", "Stop", base)]);
    tick_assert_quiet(&place, &tick_noop("stopped", "stopped", "6"));
    tick_stamps(&place, &[("idle", "Stop", base), ("idle", "Stop", now - 100)]);
    tick_assert_quiet(&place, &tick_noop("stamp-recent", "wait:0", "0"));
    tick_stamps(&place, &[("idle", "Stop", base), ("idle", "Stop", now - TICK_STALE - 60)]);
    assert_eq!(stdout_of(&tick_run(&place, &[])), tick_inject(0), "打ち切りの後も変化で段 0");
    let texts: Vec<String> = tick_keys(&place).into_iter().filter(|key| key.contains(" -l ")).collect();
    assert_eq!(texts, [tick_text_key(0), tick_text_key(0)], "送ったのは段 0 の 2 本だけ");
}

/// (f) 応えない席: `sent_at` から `seat.tick_stale_s`（30 分）の手前は `settling`・過ぎても Stop が無い周はその周の digest で基準が入り、段の候補は段 + 1。
#[test]
fn seat_tick_unanswered_seat_settles_on_the_stale_digest_and_climbs() {
    let place = tick_place(true);
    let now = unix_now();
    let last = now - 6000;
    tick_stamps(&place, &[("idle", "Stop", last)]);
    tick_ladder_put(&place, now - 1000, 0, None);
    tick_assert_quiet(&place, &tick_noop("settling", "settling", "0"));
    tick_ladder_put(&place, now - 2500, 0, None);
    let line = stdout_of(&tick_run(&place, &[]));
    assert!(line.starts_with(&format!("decision=noop target={TICK_SEAT} reason=wait pointer=wait:")), "{line}");
    assert!(line.ends_with(&format!(" step=1 consumed=-{TICK_NO_MOVE}\n")), "段 + 1: {line}");
    let left = tick_token(&line, "pointer").and_then(|found| found.strip_prefix("wait:").and_then(|secs| secs.parse::<u64>().ok()));
    assert!(left.is_some_and(|secs| (1095..=1100).contains(&secs)), "残り秒 = 列の 2 番目 3600 − 2500 秒: {line}");
    assert_eq!(tick_ladder(&place), Some((now - 2500, 0, Some(last))), "基準はその周の digest");
    assert!(tick_keys(&place).is_empty(), "0 key");
}

/// (g) 口座の門は鮮度の内側の記録だけを読む: 記録無し・鮮度の外は通り（次の門の `input-unknown` で止まる）、鮮度の内側の閾値
/// 以上は `account-pressed`。どの周も偽 client の呼出は 0 件（計測を起こさない）。
#[test]
fn seat_tick_account_gate_reads_only_fresh_records_without_measuring() {
    let place = tick_place(true);
    tick_silent_for(&place, TICK_STALE + 60);
    fs::write(place.at(TICK_PANE), "no prompt here\n").ok();
    tick_assert_quiet(&place, &tick_noop("input-unknown", "wait:0", "0"));
    acct_measured(&place.state, TICK_ACCOUNT, 99, "2020-01-01T00:00:00Z");
    tick_assert_quiet(&place, &tick_noop("input-unknown", "wait:0", "0"));
    acct_measured(&place.state, TICK_ACCOUNT, 90, &acct_now());
    tick_assert_quiet(&place, &tick_noop("account-pressed", "wait:0", "0"));
    assert!(!place.at(TICK_CLIENT).exists(), "偽 client の呼出 0 件");
}

/// (g1) 役割の行が `opus` の写しは、Fable の窓だけが高い自席の口座で口座の門を通り、次の門の `input-unknown` で止まる（base は
/// Fable の窓で `account-pressed` ＝ RED）。
#[test]
fn seat_tick_account_gate_model_opus_role_passes_with_only_the_fable_window_high() {
    assert_eq!(tick_account_gate_with_role_model("opus"), tick_noop("input-unknown", "wait:0", "0"));
}

/// (g2) 役割の行が `fable` の写しは同じ記録で `account-pressed`（自席の役割の集合を空で渡す変異を捕まえる）。
#[test]
fn seat_tick_account_gate_model_fable_role_is_pressed_by_the_fable_window() {
    assert_eq!(tick_account_gate_with_role_model("fable"), tick_noop("account-pressed", "wait:0", "0"));
}

/// (h) 入力欄に人の文字 → `input-busy`・prompt 行の無い pane → `input-unknown`（どちらも 0 key）・自席の前の合図が残る → Enter
/// 1 回の後に `input-own-queued`（送ったのは Enter の 1 key だけ・合図の text は 0 key）。どの周も記録は増えない。
#[test]
fn seat_tick_input_gate_refuses_typed_text_unknown_pane_and_own_queued_signal() {
    let place = tick_place(true);
    tick_silent_for(&place, TICK_STALE + 60);
    fs::write(place.at(TICK_PANE), format!("{TICK_CLEAR_PANE}half typed")).ok();
    tick_assert_quiet(&place, &tick_noop("input-busy", "wait:0", "0"));
    fs::write(place.at(TICK_PANE), "no prompt here\n").ok();
    tick_assert_quiet(&place, &tick_noop("input-unknown", "wait:0", "0"));
    assert!(tick_keys(&place).is_empty(), "0 key");
    fs::write(place.at(TICK_PANE), TICK_CLEAR_PANE).ok();
    assert_eq!(stdout_of(&tick_run(&place, &[])), tick_inject(0), "自席の注入の記録を 1 行作る");
    fs::remove_file(tick_ladder_path(&place)).ok();
    fs::write(place.at(TICK_PANE), format!("{TICK_CLEAR_PANE}{}", tick_signal(0))).ok();
    fs::write(place.at(TICK_STUCK), "").ok();
    let before = tick_keys(&place);
    let out = tick_run(&place, &[]);
    assert_eq!(stdout_of(&out), tick_noop("input-own-queued", "wait:0", "0"), "stderr={}", stderr_of(&out));
    let after = tick_keys(&place);
    assert_eq!(after.get(before.len()..), Some(&[format!("send-keys -t {TICK_TARGET} Enter")][..]), "Enter の 1 key だけ");
    assert_eq!(tick_ladder(&place), None, "梯子の記録は増えない");
    assert_eq!(tick_injections(&place).len(), 1, "注入の記録は増えない");
}

/// (i) 記録が読めない（dir）→ `record-unreadable`（梯子を評価できない＝`pointer=- step=-`）・席 dir が読み取り専用 →
/// `record-unwritable`（0 key・pane 不変）・送れない tmux の周も梯子の記録は残る（送ったと数える・`consumed=unknown:<理由>`）。
#[test]
fn seat_tick_record_faults_send_nothing_and_a_failed_send_keeps_the_record() {
    let place = tick_place(true);
    tick_silent_for(&place, TICK_STALE + 60);
    fs::create_dir_all(tick_ladder_path(&place)).ok();
    tick_assert_quiet(&place, &tick_noop("record-unreadable", "-", "-"));
    fs::remove_dir(tick_ladder_path(&place)).ok();
    fs::set_permissions(place.seat(), fs::Permissions::from_mode(0o555)).ok();
    tick_assert_quiet(&place, &tick_noop("record-unwritable", "wait:0", "0"));
    fs::set_permissions(place.seat(), fs::Permissions::from_mode(0o755)).ok();
    assert_eq!(fs::read_to_string(place.at(TICK_PANE)).unwrap_or_default(), TICK_CLEAR_PANE, "pane 不変");
    assert!(tick_keys(&place).is_empty(), "0 key");
    fs::write(place.at(TICK_REFUSE), "").ok();
    let out = tick_run(&place, &[]);
    assert_eq!(
        stdout_of(&out),
        format!("decision=inject target={TICK_SEAT} reason=- pointer=sent step=0 consumed=unknown:tmux-failed{TICK_NO_MOVE}\n"),
        "送れない周も inject と数える"
    );
    assert_eq!(tick_ladder(&place).map(|(_, step, digest)| (step, digest)), Some((0, None)), "梯子の記録は残る");
    assert!(tick_injections(&place).is_empty(), "送達していない注入は tick.jsonl に書かない");
}

/// (j) `--rules` の写しが行 3 本のどれかを欠く周・梯子の列の要素が数でない周・列が狭義に昇順でない周は tick の読みが
/// `decision=error reason=no-rule` rc 1・0 key（stderr は空・全部を持つ写しは判定へ進む）。空の列 `[]` の写しは面の読みが
/// 断り、同じ `no-rule` rc 1・0 key で stderr が `rules: ` で始まり「配列が空である」を含む（経路の違いを stderr で弁別する）。
/// manifest が壊れている周も `no-rule`（defect は stderr）・event log が読めない周は `store`。
#[test]
fn seat_tick_missing_rule_rows_and_unreadable_store_are_errors() {
    let place = tick_place(true);
    tick_silent_for(&place, 100);
    let full = fixture(&place.dir, "full.toml", &tick_rules_text("", None));
    let out = tick_run(&place, &["--rules", &full]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_OK), tick_noop("stamp-recent", "wait:0", "0")), "全部を持つ写し");
    let error =
        |reason: &str| format!("decision=error target={TICK_SEAT} reason={reason} pointer=- step=- consumed=-{TICK_NO_MOVE}\n");
    let refused_by_tick = |label: &str, body: &str| {
        let rules = fixture(&place.dir, "refused.toml", body);
        let out = tick_run(&place, &["--rules", &rules]);
        assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), error("no-rule")), "{label}");
        assert_eq!(stderr_of(&out), "", "{label}: tick の読みの断り（面は通る）");
    };
    for (id, _, _) in tick_rule_rows() {
        refused_by_tick(&format!("{id} を欠く写し"), &tick_rules_text(id, None));
    }
    for ladder in [r#"["1800", "x"]"#, r#"["1800", "-3600"]"#, r#"["3600", "1800"]"#, r#"["1800", "1800"]"#] {
        refused_by_tick(&format!("列 {ladder}"), &tick_rules_text("", Some(ladder)));
    }
    let empty = fixture(&place.dir, "empty.toml", &tick_rules_text("", Some("[]")));
    let out = tick_run(&place, &["--rules", &empty]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), error("no-rule")), "空の列");
    let stderr = stderr_of(&out);
    assert!(stderr.starts_with("rules: ") && stderr.contains("配列が空である"), "空の列は面の読みが断る: {stderr}");
    let broken = fixture(&place.dir, "broken.toml", "こわれ\n");
    let out = tick_run(&place, &["--rules", &broken]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), error("no-rule")), "壊れた manifest");
    assert!(stderr_of(&out).starts_with("rules: "), "defect を stderr へ: {}", stderr_of(&out));
    let events = vessel::fleet::store::events_path(&place.state);
    fs::remove_file(&events).ok();
    fs::create_dir_all(&events).ok();
    let out = tick_run(&place, &[]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), error("store")), "event log が読めない");
    assert!(tick_keys(&place).is_empty() && tick_ladder(&place).is_none(), "どの周も 0 key・記録 0");
}

/// (a) 登録 row（口座 A・anchor は群 Tier1）∧ 記録は口座 B ∧ 最終行 Idle（いま）∧ pane が claude ∧ 入力欄が空 → `decision=move
/// move=exit`・`/exit` の text 1 回 + Enter 1 回・`tick.jsonl` に `who=seat-tick-move what=/exit` の 1 行・梯子の記録は書かれず
/// 合図の text は 0 key（base では黙りの門の `stamp-recent` ＝ RED）。
#[test]
fn seat_tick_move_evacuates_a_seat_whose_row_differs_from_the_group_record() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    move_signal_past(&place);
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), move_line("exit", "false", "-"), "判定行 1 行");
    assert_eq!(
        move_keys(&place),
        [format!("send-keys -t {TICK_TARGET} -l /exit"), format!("send-keys -t {TICK_TARGET} Enter")],
        "/exit の text 1 回 + Enter 1 回・合図の text は 0"
    );
    let injections = move_injections(&place);
    assert_eq!(injections.len(), 1, "席の記録 1 行: {injections:?}");
    let line = injections.first().map(String::as_str).unwrap_or_default();
    assert_eq!(move_who_what(line), (Some("seat-tick-move".to_owned()), Some("/exit".to_owned())), "{line}");
    assert!(!place.state.join("seat").join(TICK_SEAT).join("pointer-ladder").exists(), "梯子の記録は書かれない");
    assert!(!place.at(TICK_CLIENT).exists(), "偽 client は呼ばれない");
    assert!(!move_groups_dir(&root).join("lock").exists(), "lock は周の後に外れる");
}

/// (b) pane の最後の `❯` 行が dialog の既定の行 → Enter 1 key だけ・`/exit` 0・記録の `what` は `enter:exit-dialog`／tail が別の
/// 字面 → `input-busy`・0 key／prompt 行なし → `input-unknown`・0 key（どちらも記録 0）。
#[test]
fn seat_tick_move_confirms_the_exit_dialog_and_refuses_other_input() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    move_signal_past(&place);
    fs::write(place.at(TICK_PANE), format!("{TICK_CLEAR_PANE}half typed")).ok();
    move_assert_quiet(&place, &move_noop("input-busy"));
    fs::write(place.at(TICK_PANE), "no prompt here\n").ok();
    move_assert_quiet(&place, &move_noop("input-unknown"));
    fs::write(place.at(TICK_PANE), format!("Exit?\n\u{276f} {MOVE_DIALOG_ROW}\n  2. Cancel\n")).ok();
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), move_line("enter", "unknown:exit-dialog", "-"), "stderr={}", stderr_of(&out));
    assert_eq!(move_keys(&place), [format!("send-keys -t {TICK_TARGET} Enter")], "Enter の 1 key だけ・/exit 0");
    let injections = move_injections(&place);
    assert_eq!(injections.len(), 1, "記録 1 行: {injections:?}");
    let line = injections.first().map(String::as_str).unwrap_or_default();
    assert_eq!(move_who_what(line), (Some("seat-tick-move".to_owned()), Some("enter:exit-dialog".to_owned())), "{line}");
}

/// (c) pane が shell → `move=launch`・`send-keys` に起動行 1 行（口座 B の設定 dir を持つ）・fleet に口座 B の登録 row が 1 件
/// 増える・`/exit` 0（偽 tmux は打刻を打たない＝settle 1 秒で `launched=launch-unconfirmed`）。
#[test]
fn seat_tick_move_launches_the_group_account_into_a_shell_pane() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    move_signal_past(&place);
    fs::write(place.at(MOVE_FRONT), "bash\n").ok();
    fs::write(place.at(TICK_PANE), "old output\n$ ").ok();
    let before = acct_rows(&place.state).len();
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), move_line("launch", "-", "launch-unconfirmed"), "判定行 1 行");
    let dir = place.state.join("accounts").join(MOVE_B).display().to_string();
    let texts: Vec<String> = move_keys(&place).into_iter().filter(|key| key.contains(" -l ")).collect();
    assert_eq!(texts.len(), 1, "起動行 1 行: {texts:?}");
    assert!(texts.iter().all(|key| key.contains(&dir) && !key.contains("/exit")), "口座 B の設定 dir を持つ起動行: {texts:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), before + 1, "登録 row が 1 件増える");
    let last = rows.last().map(|row| (row.account.as_str(), row.anchor.as_str(), row.target.as_str()));
    assert_eq!(last, Some((MOVE_B, MOVE_ANCHOR, TICK_TARGET)), "口座 B・同じ anchor と target");
    assert!(!place.state.join("seat").join(TICK_SEAT).join("pointer-ladder").exists(), "梯子の記録は書かれない");
}

/// (d) 記録が dir（読めない）→ `group-unreadable`・0 key／lock の file が在る → `group-locked`・0 key・記録 0・起動行 0（pane が
/// shell でも起こさない）。最終行 Busy の移動の周は §10 形 8 で退避へ進む（`seat_tick_evacuate_` の歯）。
#[test]
fn seat_tick_move_stops_on_unreadable_record_and_held_lock() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    move_signal_past(&place);
    let lock = move_groups_dir(&root).join("lock");
    fs::write(&lock, "pid=1\n").ok();
    move_assert_quiet(&place, &move_noop("group-locked"));
    fs::write(place.at(MOVE_FRONT), "bash\n").ok();
    fs::write(place.at(TICK_PANE), "old output\n$ ").ok();
    let rows = acct_rows(&place.state).len();
    move_assert_quiet(&place, &move_noop("group-locked"));
    assert_eq!(acct_rows(&place.state).len(), rows, "起こさない（登録 row は増えない）");
    assert!(lock.exists(), "他の手の lock は外さない");
    fs::remove_file(&lock).ok();
    let record = move_groups_dir(&root).join(format!("{MOVE_GROUP}.account"));
    fs::remove_file(&record).ok();
    fs::create_dir_all(&record).ok();
    move_assert_quiet(&place, &move_noop("group-unreadable"));
}

/// (e) 記録の口座 = row の口座（移動済み）／群に属さない anchor／記録なしで種 = row → §2 の列のまま（`stamp-recent`・黙った席は
/// `inject`・どちらも `move=- launched=-`）。
#[test]
fn seat_tick_move_leaves_the_list_unchanged_when_the_row_matches_or_the_anchor_is_outside() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_signal_past(&place);
    move_assert_quiet(&place, &format!("decision=noop target={TICK_SEAT} reason=stamp-recent pointer=wait:0 step=0 consumed=-{TICK_NO_MOVE}\n"));
    move_record(&root, MOVE_A);
    move_assert_quiet(&place, &format!("decision=noop target={TICK_SEAT} reason=stamp-recent pointer=wait:0 step=0 consumed=-{TICK_NO_MOVE}\n"));
    let seat = seat_dir_of(&place.state, TICK_SEAT);
    fs::write(state_file(&seat), format!("{}\n", stamp_line("idle", "Stop", unix_now() - TICK_STALE - 60, "sid-move"))).ok();
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), tick_inject(0), "記録と一致する黙った席は合図: stderr={}", stderr_of(&out));
    let outside = tmp();
    let other = move_place(&outside, "state", "/elsewhere");
    move_record(&outside, MOVE_B);
    move_signal_past(&other);
    move_assert_quiet(&other, &format!("decision=noop target={TICK_SEAT} reason=stamp-recent pointer=wait:0 step=0 consumed=-{TICK_NO_MOVE}\n"));
    assert!(!move_groups_dir(&outside).join("lock").exists(), "群に属さない席は lock も取らない");
}

/// (f) 親を共有する 2 つの置き場に 1 席ずつ（群の置き場 2 つ）・記録は親の下の 1 file → 両方の tick が `move=exit`（置き場を
/// 跨いで同じ記録を読む＝別 project の席も移る）。
#[test]
fn seat_tick_move_reaches_seats_in_two_state_dirs_under_one_parent() {
    let root = tmp();
    let one = move_place(&root, "one", MOVE_ANCHOR);
    let two = move_place(&root, "two", MOVE_ANCHOR_TWO);
    move_record(&root, MOVE_B);
    for place in [&one, &two] {
        move_signal_past(place);
        let out = move_run(place);
        assert_eq!(stdout_of(&out), move_line("exit", "false", "-"), "stderr={}", stderr_of(&out));
        assert_eq!(move_keys(place).first(), Some(&format!("send-keys -t {TICK_TARGET} -l /exit")), "/exit を送る");
        assert_eq!(move_injections(place).len(), 1, "置き場ごとに記録 1 行");
    }
}

/// (a) 群の今の口座（種 A）が逼迫 ∧ 候補 B は閾値未満 ∧ 判定の打刻なし ∧ 前面 `claude` ∧ 入力欄が空 → 記録が B へ動き・承認
/// event 1・`judged=moved:acct-b`・自席への key は移動の門の `/exit` の 1 行だけ・2 つ目の席へ 0 key・通知 0・打刻は判定の周の
/// ts（base では記録不変 ∧ 打刻の file 無し ＝ RED）。
#[test]
fn seat_tick_judge_moves_the_record_and_only_the_move_gate_sends_the_exit() {
    use vessel::fleet::EventKind;
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 10]);
    let before = unix_now();
    let out = move_run(&place);
    let after = unix_now();
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let want = format!("decision=move target={TICK_SEAT} reason=- pointer=- step=- consumed=false move=exit launched=- judged=moved:{MOVE_B}\n");
    assert_eq!(stdout_of(&out), want, "判定行 1 行");
    let record = fs::read_to_string(move_groups_dir(&root).join(format!("{MOVE_GROUP}.account"))).unwrap_or_default();
    assert!(record.starts_with(&format!("account={MOVE_B}\n")) && record.contains(&format!("previous={MOVE_A}\n")), "{record}");
    assert_eq!(judge_events(&place, EventKind::GroupMoved), 1, "承認 event 1");
    assert_eq!(judge_events(&place, EventKind::GroupPressureNotified), 0, "通知 0");
    assert_eq!(
        move_keys(&place),
        [format!("send-keys -t {TICK_TARGET} -l /exit"), format!("send-keys -t {TICK_TARGET} Enter")],
        "自席への /exit は移動の門の 1 行だけ・他の席へ 0 key・通知の行 0"
    );
    assert_eq!(judge_calls(&place), 2, "今の口座と候補を 1 回ずつ測る");
    assert!(judge_ts(&root).is_some_and(|ts| (before..=after).contains(&ts)), "打刻は判定の周の ts: {:?}", judge_ts(&root));
    assert!(!move_groups_dir(&root).join("lock").exists(), "lock は周の後に外れる");
}

/// (b) 判定の打刻が鮮度の内側 → 計測 0・`judged=-`・打刻は不変。fixture が置いた打刻の周と、(a) の周が書いた打刻の直後に
/// もう 1 周撃つ周（自前の打刻を鮮度の内側と読む）の 2 本。
#[test]
fn seat_tick_judge_skips_a_fresh_stamp_without_measuring() {
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 10]);
    let put = unix_now() - 10;
    fs::write(judge_stamp(&root), format!("{put}\n")).ok();
    move_assert_quiet(&place, &judge_recent("-"));
    assert_eq!((judge_calls(&place), judge_ts(&root)), (0, Some(put)), "計測 0・打刻は不変");
    assert!(!move_groups_dir(&root).join(format!("{MOVE_GROUP}.account")).exists(), "記録は書かれない");
    let again = tmp();
    let place = judge_place(&again, MOVE_ANCHOR, [90, 10]);
    let first = move_run(&place);
    assert!(stdout_of(&first).ends_with(&format!(" judged=moved:{MOVE_B}\n")), "1 周目は判定する: {}", stdout_of(&first));
    let (calls, stamped) = (judge_calls(&place), judge_ts(&again));
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), move_line("exit", "false", "-"), "2 周目は自前の打刻で撃たない: stderr={}", stderr_of(&out));
    assert_eq!((judge_calls(&place), judge_ts(&again)), (calls, stamped), "2 周目の計測 0・打刻は不変");
}

/// (c) 候補なし（A / B とも逼迫）∧ 入力欄が空 → `judged=none`・断りの event 1・記録不変・自席へ断りの 1 行（群の段の断りの
/// 字面）・他の席へ 0 key（base では 0 行 ＝ RED）。
#[test]
fn seat_tick_judge_without_a_candidate_refuses_to_its_own_seat_only() {
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 90]);
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), judge_recent("none"), "stderr={}", stderr_of(&out));
    assert_eq!(judge_events(&place, vessel::fleet::EventKind::GroupMoveRefused), 1, "断りの event 1");
    assert!(!move_groups_dir(&root).join(format!("{MOVE_GROUP}.account")).exists(), "記録不変");
    assert_eq!(
        move_keys(&place),
        [format!("send-keys -t {TICK_TARGET} -l {}", judge_refused()), format!("send-keys -t {TICK_TARGET} Enter")],
        "自席へ断りの 1 行だけ・他の席へ 0 key"
    );
}

/// (c2) 候補なし ∧ 同じ実測に断りの event が既に在る（(c) の直後に打刻を消してもう 1 周）→ `judged=none`・event 0・自席へ 0 行。
#[test]
fn seat_tick_judge_does_not_refuse_the_same_measurement_twice() {
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 90]);
    let first = move_run(&place);
    assert_eq!(stdout_of(&first), judge_recent("none"), "1 周目は断る: stderr={}", stderr_of(&first));
    fs::remove_file(judge_stamp(&root)).ok();
    let keys = move_keys(&place).len();
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), judge_recent("none"), "stderr={}", stderr_of(&out));
    assert_eq!(judge_events(&place, vessel::fleet::EventKind::GroupMoveRefused), 1, "event 0（1 のまま）");
    assert_eq!(move_keys(&place).len(), keys, "自席へ 0 行");
}

/// (d) 群用 dir に lock が在る → 判定 0（計測 0・記録 0・打刻なし）・列は今のまま（`judged=-`）。
#[test]
fn seat_tick_judge_held_lock_judges_nothing() {
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 10]);
    let lock = move_groups_dir(&root).join("lock");
    fs::write(&lock, "pid=1\n").ok();
    move_assert_quiet(&place, &judge_recent("-"));
    assert_eq!((judge_calls(&place), judge_ts(&root)), (0, None), "計測 0・打刻なし");
    assert_eq!(judge_events(&place, vessel::fleet::EventKind::GroupMoved), 0, "承認 event 0");
    assert!(lock.exists(), "他の手の lock は外さない");
}

/// (e) 群に属さない anchor → `judged=-`・0 key・計測 0（今の口座が逼迫でも判定しない）。
#[test]
fn seat_tick_judge_outside_the_group_is_unjudged() {
    let root = tmp();
    let place = judge_place(&root, "/elsewhere", [90, 10]);
    move_assert_quiet(&place, &judge_recent("-"));
    assert_eq!((judge_calls(&place), judge_ts(&root)), (0, None), "計測 0・打刻なし");
}

/// (e・§31) Tier2 の今の口座 A だけが逼迫の周（Tier1 の今の口座 D は閾値未満）、tick の判定は逼迫でない Tier1 に予約を持たせず、
/// Tier1 の鍵の先頭 B（残量 90）へ移る（`judged=moved:acct-b`・承認 event 1・base は Tier1 の予約 B を飛ばして C ＝ RED）。
#[test]
fn seat_tick_judge_reserve_takes_the_tier1_key_head() {
    let root = tmp();
    let place = reserve_place(&root);
    let out = move_run(&place);
    assert!(stdout_of(&out).ends_with(&format!(" judged=moved:{MOVE_B}\n")), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    assert_eq!(reserve_record(&root).as_deref(), Some(MOVE_B), "記録は B");
    assert_eq!(judge_events(&place, vessel::fleet::EventKind::GroupMoved), 1, "承認 event 1");
    assert!(!move_groups_dir(&root).join("Tier1.account").exists(), "Tier1 の記録は書かない（予約は記録しない）");
}

/// (f・§31) tick の断りの周に群用 dir の印が書かれる（base は印が無い ＝ RED）。
#[test]
fn seat_tick_judge_reserve_refusal_writes_the_mark() {
    let root = tmp();
    let place = reserve_refused(&root);
    assert_eq!(judge_events(&place, vessel::fleet::EventKind::GroupMoveRefused), 1, "断りの event 1");
}

/// (f2・§31) 同じ実測の 2 周目（event を記さない）は印を書き直さない（番兵の字面のまま・base は 1 周目の印が無い ＝ RED）。
#[test]
fn seat_tick_judge_reserve_repeated_refusal_keeps_the_mark() {
    let root = tmp();
    let place = reserve_refused(&root);
    fs::write(move_groups_dir(&root).join(format!("{MOVE_GROUP}.refused")), RESERVE_SENTINEL).ok();
    fs::remove_file(judge_stamp(&root)).ok();
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), judge_recent("none"), "stderr={}", stderr_of(&out));
    assert_eq!(judge_events(&place, vessel::fleet::EventKind::GroupMoveRefused), 1, "event 0（1 のまま）");
    assert_eq!(reserve_mark(&root).as_deref(), Some(RESERVE_SENTINEL), "印の字面は不変");
}

/// (f3・§31) 印が在る状態で tick が移る周（記録を B へ書く）は印が history へ退避される（base は印が残る ＝ RED）。
#[test]
fn seat_tick_judge_reserve_move_round_moves_the_mark_to_history() {
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 10]);
    fs::write(move_groups_dir(&root).join(format!("{MOVE_GROUP}.refused")), RESERVE_SENTINEL).ok();
    let out = move_run(&place);
    assert!(stdout_of(&out).ends_with(&format!(" judged=moved:{MOVE_B}\n")), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    assert_eq!(reserve_mark(&root), None, "印は消える");
    let history: Vec<String> = fs::read_dir(move_groups_dir(&root).join("history"))
        .map(|entries| entries.filter_map(Result::ok).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    let marks = history.iter().filter(|name| name.starts_with(&format!("{MOVE_GROUP}.refused."))).count();
    assert_eq!(marks, 1, "history に 1 つ: {history:?}");
}

/// 群の席の役割の `seat.model.orchestrator` 行を欠く `--rules` の tick は、今の口座が逼迫でも移らず `judged=error:unreadable`
/// で 0 key・記録 0・承認 event 0・断りの event 0（集合を空に読み替えない・base は B へ移る ＝ RED）。
#[test]
fn seat_tick_judge_reserve_missing_role_row_is_an_error_with_zero_keys() {
    use vessel::fleet::EventKind;
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 10]);
    let row = format!("\n[[rule]]\nid = \"{MOVE_ROLE_ROW}\"\nkind = \"RoleModel\"\nvalue = \"fable\"\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n");
    let rules = fs::read_to_string(&place.rules).unwrap_or_default();
    assert!(rules.contains(&row), "写しに役割の行が在る");
    fs::write(&place.rules, rules.replace(&row, "")).ok();
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), judge_recent("error:unreadable"), "stderr={}", stderr_of(&out));
    assert_eq!(move_keys(&place), Vec::<String>::new(), "0 key");
    assert!(!move_groups_dir(&root).join(format!("{MOVE_GROUP}.account")).exists(), "記録 0");
    assert_eq!((judge_events(&place, EventKind::GroupMoved), judge_events(&place, EventKind::GroupMoveRefused)), (0, 0), "event 0");
}

/// (d) 役割の行を `opus` にした写しで、群の今の口座 A の本文が Fable の窓 100・5 時間窓 10・7 日窓 50 の周は逼迫でなく移らない
/// （`judged=stay`・記録 0・承認 event 0・断りの event 0・0 key・base は Fable の窓で逼迫と読み B へ移る ＝ RED）。
#[test]
fn seat_tick_judge_model_gate_opus_role_with_only_the_fable_window_high_stays() {
    use vessel::fleet::EventKind;
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [10, 10]);
    let far = "2099-01-01T00:00:00Z";
    let body = format!(
        "{{\"five_hour\":{{\"utilization\":10,\"resets_at\":\"{far}\"}},\"seven_day\":{{\"utilization\":50,\"resets_at\":\"{far}\"}},\
         \"limits\":[{{\"kind\":\"weekly_scoped\",\"percent\":100,\"resets_at\":\"{far}\",\"scope\":{{\"model\":{{\"display_name\":\"Fable\"}}}}}}]}}"
    );
    fs::write(place.at(&format!("body-tok-{MOVE_A}")), body).ok();
    let fable = format!("id = \"{MOVE_ROLE_ROW}\"\nkind = \"RoleModel\"\nvalue = \"fable\"\n");
    let rules = fs::read_to_string(&place.rules).unwrap_or_default();
    assert!(rules.contains(&fable), "写しに役割の行が在る");
    fs::write(&place.rules, rules.replace(&fable, &fable.replace("\"fable\"", "\"opus\""))).ok();
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), judge_recent("stay"), "stderr={}", stderr_of(&out));
    assert!(!move_groups_dir(&root).join(format!("{MOVE_GROUP}.account")).exists(), "記録 0");
    assert_eq!((judge_events(&place, EventKind::GroupMoved), judge_events(&place, EventKind::GroupMoveRefused)), (0, 0), "event 0");
    assert_eq!(move_keys(&place), Vec::<String>::new(), "0 key");
}

/// (a) 最終行 busy ∧ 前面 `bash` ∧ 群の外の row → `move=launch`・起動行 1 行が row の口座（A）を持ち、末尾に `--resume <打刻の
/// sid> '<NAME> seat: relaunch …'`（base では `--resume <sid>` で終わる ＝ RED）。群の外の席は lock を取らない。
#[test]
fn seat_tick_wake_launches_a_dead_seat_outside_a_group_with_the_stamped_sid() {
    let root = tmp();
    let place = wake_place(&root, "/elsewhere", WAKE_SID);
    let text = wake_assert_launched(&place, MOVE_A, "/elsewhere");
    assert!(text.ends_with(&format!(" --resume {WAKE_SID} {}", wake_first_word())), "末尾に --resume <sid> と初手: {text}");
    assert_eq!(text.matches("--resume").count(), 1, "--resume は 1 つ: {text}");
    assert_eq!(text.matches("seat: relaunch").count(), 1, "初手は 1 つ: {text}");
    assert!(!move_groups_dir(&root).join("lock").exists(), "lock は残らない");
}

/// (b) 同じ席で最終行の sid が無い・会話 id の形でない → 起こすが `--resume` は無く、末尾は初手の 1 語だけ（前の行の sid にも倒れない）。
#[test]
fn seat_tick_wake_without_a_session_id_carries_nothing() {
    for sid in ["", "sid-move"] {
        let root = tmp();
        let place = wake_place(&root, "/elsewhere", sid);
        let seat = seat_dir_of(&place.state, TICK_SEAT);
        let before = stamp_line("idle", "Stop", unix_now() - 100, WAKE_SID);
        let last = stamp_line("busy", "UserPromptSubmit", unix_now() - 10, sid);
        fs::write(state_file(&seat), format!("{before}\n{last}\n")).ok();
        let text = wake_assert_launched(&place, MOVE_A, "/elsewhere");
        assert!(!text.contains("--resume"), "{sid:?}: --resume 無し: {text}");
        assert!(!text.contains(WAKE_SID), "{sid:?}: 前の行の sid に倒れない: {text}");
        assert!(text.ends_with(&format!(" {}", wake_first_word())), "{sid:?}: 末尾は初手の 1 語: {text}");
        assert_eq!(text.matches("seat: relaunch").count(), 1, "{sid:?}: 初手は 1 つ: {text}");
    }
}

/// (c) 最終行 busy ∧ 前面 `bash` ∧ 群の row（口座 A）∧ 記録 = 口座 B → 記録の口座 B で起動・末尾に `--resume <sid>` と初手の 1 語。
#[test]
fn seat_tick_wake_launches_a_group_seat_with_the_record_account() {
    let root = tmp();
    let place = wake_place(&root, MOVE_ANCHOR, WAKE_SID);
    let text = wake_assert_launched(&place, MOVE_B, MOVE_ANCHOR);
    assert!(text.ends_with(&format!(" --resume {WAKE_SID} {}", wake_first_word())), "末尾に --resume <sid> と初手: {text}");
    assert!(!text.contains(&wake_account_dir(&place, MOVE_A)), "row の口座 A では起こさない: {text}");
    assert!(!move_groups_dir(&root).join("lock").exists(), "lock は周の後に外れる");
}

/// (d) 前面 `claude` ∧ 最終行 busy → `noop busy`（今のまま・0 key・起動行 0）。群の外の row も、記録 = row の群の row も同じ
/// （記録 ≠ row の移動の周は §10 形 8 で退避へ進む＝`seat_tick_evacuate_` の歯）。
#[test]
fn seat_tick_wake_leaves_a_busy_claude_front_alone() {
    for anchor in ["/elsewhere", MOVE_ANCHOR] {
        let root = tmp();
        let place = wake_place(&root, anchor, WAKE_SID);
        move_record(&root, MOVE_A);
        fs::remove_file(place.at(MOVE_FRONT)).ok();
        fs::write(place.at(TICK_PANE), TICK_CLEAR_PANE).ok();
        let rows = acct_rows(&place.state).len();
        move_assert_quiet(&place, &move_noop("busy"));
        assert_eq!(acct_rows(&place.state).len(), rows, "{anchor}: 起こさない");
    }
}

/// (e) 前面 `bash` ∧ row 無し（別の target）→ `no-row`・0 key・前面を引かない（row の無い窓は 1 字も変わらない）。
#[test]
fn seat_tick_wake_does_not_touch_a_window_without_a_row() {
    let root = tmp();
    let place = wake_place(&root, "/elsewhere", WAKE_SID);
    let state = place.state.display().to_string();
    let out = Command::new(bin())
        .args(["seat", "tick", "--state-dir", &state, "--target", "tk:other", "--rules", &place.rules])
        .env("PATH", &place.path)
        .output()
        .expect("binary を起動できる");
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        format!("decision=noop target=tk_other reason=no-row pointer=- step=- consumed=-{TICK_NO_MOVE}\n"),
        "判定行"
    );
    assert!(move_keys(&place).is_empty(), "0 key");
    assert!(pane_shell_calls(&place, "list-panes").is_empty(), "前面を引かない");
    assert!(!place.at(TICK_CLIENT).exists(), "偽 client は呼ばれない");
}

/// (c) 道具箱を積んだうえで写しの行が 0 の周と行を欠く写しの周は、起こし直しの起動行が素の行（env の直後が `claude`・
/// `systemd-run` を持たない）で、起こし直しは止まらない（写しを捨てて埋め込み〔32768〕を読む変異は頭が付いて落ちる）。
#[test]
fn seat_tick_wake_keeps_the_bare_line_when_the_copy_row_is_zero_or_missing() {
    for value in [Some(0), None] {
        let root = tmp();
        let mut place = wake_place(&root, "/elsewhere", WAKE_SID);
        wake_box(&mut place, value);
        let text = wake_assert_launched(&place, MOVE_A, "/elsewhere");
        assert!(!text.contains("systemd-run"), "{value:?}: 頭は付かない: {text}");
        assert!(text.contains(&format!("CLAUDE_CONFIG_DIR={} claude ", wake_account_dir(&place, MOVE_A))), "{value:?}: env の直後が claude: {text}");
        assert!(text.ends_with(&format!(" --resume {WAKE_SID} {}", wake_first_word())), "{value:?}: 末尾は不変: {text}");
    }
}

/// (c2) 道具箱を積み写しの行を 4096（埋め込みの 32768 と違う値）にした周は、起こし直しの起動行が env 3 語の直後に箱の頭
/// （`MemoryMax=4096M`・unit 名は `<NAME>-tk_tk-seat-0-<pid>-<seq>`）を持ち、末尾の `--resume <sid>` と初手は不変（base では頭が
/// 無い ＝ RED・写しを捨てて埋め込みを読む変異は `MemoryMax=32768M` で落ちる）。登録 row の launch は頭を持たない。
#[test]
fn seat_tick_wake_scope_boxes_the_relaunch_with_the_copy_value() {
    let root = tmp();
    let mut place = wake_place(&root, "/elsewhere", WAKE_SID);
    wake_box(&mut place, Some(4096));
    let text = wake_assert_launched(&place, MOVE_A, "/elsewhere");
    let unit = text.split(' ').find_map(|word| word.strip_prefix("--unit=")).unwrap_or_default().to_owned();
    assert!(launch_unit_well_formed(&unit, TICK_SEAT), "unit 名の形: {text}");
    let want = format!("CLAUDE_CONFIG_DIR={} {} claude ", wake_account_dir(&place, MOVE_A), launch_box_head(&unit, 4096));
    assert!(text.contains(&want), "env の直後・claude の前に頭: {text}");
    assert_eq!(text.matches("systemd-run").count(), 1, "頭は 1 つ: {text}");
    assert!(!text.contains("CPUWeight"), "{text}");
    assert!(text.ends_with(&format!(" --resume {WAKE_SID} {}", wake_first_word())), "末尾は不変: {text}");
    assert!(acct_rows(&place.state).iter().all(|row| !row.launch.contains("systemd-run")), "row の launch は頭を持たない");
}

/// (f) 最終行の Busy が stale の 2 倍より古い ∧ 前面 `claude` ∧ 入力欄が空 → Busy を無視して列の先へ進み、合図 1 行を注入する
/// （base では `state-stale`）。打刻 file は 1 byte も書き換えない。
#[test]
fn seat_tick_stale_busy_past_twice_the_stale_with_a_clear_input_goes_on_to_the_signal() {
    let place = tick_place(true);
    tick_stamps(&place, &[("busy", "UserPromptSubmit", unix_now() - 2 * TICK_STALE - 60)]);
    let stamps = fs::read_to_string(state_file(&place.seat())).unwrap_or_default();
    let out = tick_run(&place, &[]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), tick_inject(0), "判定行は列の先の語");
    assert_eq!(tick_keys(&place), [tick_text_key(0), format!("send-keys -t {TICK_TARGET} Enter")], "text 1 回 + Enter 1 回");
    assert_eq!(fs::read_to_string(state_file(&place.seat())).unwrap_or_default(), stamps, "打刻は書き換えない");
}

/// (f) 最終行の Busy が stale の 2 倍より古い ∧ 前面 `claude` ∧ 入力欄に字が在る → `state-stale` のまま（人が見る）・0 key。
#[test]
fn seat_tick_stale_busy_past_twice_the_stale_with_typed_input_stays_state_stale() {
    let place = tick_place(true);
    tick_stamps(&place, &[("busy", "UserPromptSubmit", unix_now() - 2 * TICK_STALE - 60)]);
    fs::write(place.at(TICK_PANE), format!("{TICK_CLEAR_PANE}half typed")).ok();
    tick_assert_quiet(&place, &tick_noop("state-stale", "-", "-"));
    assert!(tick_keys(&place).is_empty(), "0 key");
}

/// (g) 最終行の Busy が stale より古く 2 倍以内 ∧ 前面 `claude` ∧ 入力欄が空 → `state-stale` のまま・0 key（(f) と対で係数 2 を
/// pin する＝係数を 1 にする変異はここで落ちる）。
#[test]
fn seat_tick_stale_busy_within_twice_the_stale_with_a_clear_input_stays_state_stale() {
    let place = tick_place(true);
    for ago in [TICK_STALE + 60, 2 * TICK_STALE - 60] {
        tick_stamps(&place, &[("busy", "UserPromptSubmit", unix_now() - ago)]);
        tick_assert_quiet(&place, &tick_noop("state-stale", "-", "-"));
    }
    assert!(tick_keys(&place).is_empty(), "0 key");
}

/// (k) 記録 ≠ row ∧ 前面 `claude` ∧ 最終行 Busy（新しい）∧ 入力欄が空 → `/exit` 1 行・`decision=move move=exit`（base では
/// `noop busy` ＝ RED）。
#[test]
fn seat_tick_evacuate_sends_the_exit_to_a_fresh_busy_seat() {
    let root = tmp();
    evacuate_assert_exit(&evacuate_place(&root, MOVE_B, 10));
}

/// (l) 同じ席で最終行の Busy が stale より古い → `/exit` 1 行（base では `state-stale` ＝ RED）。
#[test]
fn seat_tick_evacuate_sends_the_exit_to_a_stale_busy_seat() {
    let root = tmp();
    evacuate_assert_exit(&evacuate_place(&root, MOVE_B, TICK_STALE + 60));
}

/// (m) 記録 = row ∧ 最終行 Busy → `noop busy`・0 key（移動の周でない席は今のまま）。
#[test]
fn seat_tick_evacuate_leaves_a_busy_seat_whose_row_matches_the_record() {
    let root = tmp();
    move_assert_quiet(&evacuate_place(&root, MOVE_A, 10), &move_noop("busy"));
}

/// (n) 記録 ≠ row ∧ 最終行 Busy ∧ 入力欄に字 → `input-busy`・0 key（移動の門の入力欄の門のまま）。
#[test]
fn seat_tick_evacuate_refuses_typed_input() {
    let root = tmp();
    let place = evacuate_place(&root, MOVE_B, 10);
    fs::write(place.at(TICK_PANE), format!("{TICK_CLEAR_PANE}half typed")).ok();
    move_assert_quiet(&place, &move_noop("input-busy"));
}

/// (o) 2 周続けて撃つ → `/exit` が 2 行（積む・止めない）・記録も 2 行。
#[test]
fn seat_tick_evacuate_sends_the_exit_every_round() {
    let root = tmp();
    let place = evacuate_place(&root, MOVE_B, 10);
    evacuate_assert_exit(&place);
    evacuate_assert_exit(&place);
    let exits = move_keys(&place).iter().filter(|key| key.ends_with(" -l /exit")).count();
    assert_eq!((exits, move_injections(&place).len()), (2, 2), "/exit 2 行・記録 2 行");
}

/// (a) 群の記録の ts が猶予の外（[`MOVE_TS`]）∧ 合図の記録なし ∧ 入力欄が空 → `move=signal`・合図の text 1 回 + Enter 1 回・
/// `/exit` 0・記録が `to=<口座 B> ts=<記録の ts> at=<今>`（base では `/exit` ＝ RED）。
#[test]
fn seat_tick_grace_old_record_still_signals_first() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    grace_assert_signal(&place, MOVE_TS);
    assert!(!move_groups_dir(&root).join("lock").exists(), "lock は残らない");
}

/// (b) 記録なし（種）∧ row ≠ 種 ∧ 合図の記録なし → `move=signal`・記録の `ts` 欄が `seed`（base では `/exit` ＝ RED）。
#[test]
fn seat_tick_grace_seed_differing_from_the_row_signals_first() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    let host = format!(
        "schema = 1\n\n[[account]]\nlabel = \"{MOVE_A}\"\n\n[[account]]\nlabel = \"{MOVE_B}\"\n\n[[account-group]]\nname = \"{MOVE_GROUP}\"\n\
         anchors = [\"{MOVE_ANCHOR}\", \"{MOVE_ANCHOR_TWO}\"]\naccounts = [\"{MOVE_B}\", \"{MOVE_A}\"]\n"
    );
    fs::write(place.state.join("host.toml"), host).ok();
    grace_assert_signal(&place, "seed");
    assert!(!move_groups_dir(&root).join(format!("{MOVE_GROUP}.account")).exists(), "群の記録は書かれない（種のまま）");
}

/// (c) 同じ移動の記録で `at` = 今 − 100 → `move=wait`・0 key・記録 0・合図の記録は不変・lock を取らない（他の手の lock が在っても
/// `group-locked` にならない・base では群の記録の ts が古く `/exit` ＝ RED）。
#[test]
fn seat_tick_grace_waits_after_the_signal_without_the_lock() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    grace_signal_put(&place, (MOVE_B, MOVE_TS), unix_now() - 100);
    let body = fs::read_to_string(grace_signal(&place)).ok();
    let lock = move_groups_dir(&root).join("lock");
    fs::write(&lock, "pid=1\n").ok();
    move_assert_quiet(&place, &move_line("wait", "-", "-"));
    assert_eq!(fs::read_to_string(grace_signal(&place)).ok(), body, "合図の記録は不変");
    assert_eq!(fs::read_to_string(&lock).ok().as_deref(), Some("pid=1\n"), "他の手の lock は触らない");
}

/// (d) 同じ移動の記録で `at` = 今 − 1801（猶予を越えた）→ 今の形（`move=exit`・合図の記録は不変・base では旧形の等値が外れて
/// signal ＝ RED）。
#[test]
fn seat_tick_grace_past_the_grace_sends_the_exit() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    move_signal_past(&place);
    let body = fs::read_to_string(grace_signal(&place)).ok();
    evacuate_assert_exit(&place);
    assert_eq!(fs::read_to_string(grace_signal(&place)).ok(), body, "合図の記録は書き換えない");
}

/// (e) 前の版の形（`ts=<記録の ts>` だけの 1 行）→ 記録なしと読んで送り直す（`move=signal`・記録が 3 field に書き換わる・base では
/// wait ＝ RED）。
#[test]
fn seat_tick_grace_old_form_record_resends() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    fs::write(grace_signal(&place), format!("ts={MOVE_TS}\n")).ok();
    grace_assert_signal(&place, MOVE_TS);
}

/// (f) `to` が別の口座か `ts` が別の移動の記録（`at` は猶予の内側）→ 送り直す（記録は今の移動の鍵に書き換わる）。
#[test]
fn seat_tick_grace_resends_when_the_signal_names_another_move() {
    for other in [(MOVE_A, MOVE_TS), (MOVE_B, "2026-09-24T00:00:00Z")] {
        let root = tmp();
        let place = move_place(&root, "state", MOVE_ANCHOR);
        move_record(&root, MOVE_B);
        grace_signal_put(&place, other, unix_now() - 100);
        grace_assert_signal(&place, MOVE_TS);
    }
}

/// (g) 猶予 0 の `--rules` → 合図の記録に依らず `/exit`（記録なし・猶予の内側の同じ移動の記録のどちらも・記録は書かない）。
#[test]
fn seat_tick_grace_zero_sends_the_exit_at_once() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    grace_put(&place, 0);
    move_record(&root, MOVE_B);
    evacuate_assert_exit(&place);
    assert!(!grace_signal(&place).exists(), "合図の記録は書かない");
    grace_signal_put(&place, (MOVE_B, MOVE_TS), unix_now());
    evacuate_assert_exit(&place);
}

/// (h) 合図の記録なし ∧ 入力欄に人の字 → `input-busy`・0 key・合図の記録なし（次の周にまた試す）。
#[test]
fn seat_tick_grace_typed_input_is_input_busy_without_a_record() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    fs::write(place.at(TICK_PANE), format!("{TICK_CLEAR_PANE}half typed")).ok();
    move_assert_quiet(&place, &move_noop("input-busy"));
    assert!(!grace_signal(&place).exists(), "合図の記録は書かない");
}

/// (i) ts が形でない群の記録 → `group-unreadable`・0 key（形でない記録を種に読み替えない）。
#[test]
fn seat_tick_grace_malformed_ts_is_group_unreadable() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    let body = format!("account={MOVE_B}\nts=yesterday\nreason=move\nprevious={MOVE_A}\n");
    fs::write(move_groups_dir(&root).join(format!("{MOVE_GROUP}.account")), body).ok();
    move_assert_quiet(&place, &move_noop("group-unreadable"));
    assert!(!grace_signal(&place).exists(), "合図の記録は書かない");
}

/// (j) `seat.move_grace_s` を欠く `--rules` → rc 1 `no-rule`・0 key（既定の猶予に倒さない・base では判定へ進む ＝ RED）。
#[test]
fn seat_tick_grace_missing_row_is_no_rule() {
    let place = tick_place(true);
    let rules = fixture(&place.dir, "no-grace.toml", &tick_rules_text("seat.move_grace_s", None));
    let out = tick_run(&place, &["--rules", &rules]);
    let want = format!("decision=error target={TICK_SEAT} reason=no-rule pointer=- step=- consumed=-{TICK_NO_MOVE}\n");
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), want), "stderr={}", stderr_of(&out));
    assert!(tick_keys(&place).is_empty(), "0 key");
}

/// (h) tick の 1 周の後に `tick-last` が 1 行在り、ts は撃った時刻・decision / reason は判定行と同じ（noop と inject の 2 周）
/// （base では file 無し ＝ RED）。
#[test]
fn seat_tick_status_tick_last_mirrors_the_judgement_line() {
    let place = tick_place(true);
    for ago in [100, TICK_STALE + 60] {
        tick_silent_for(&place, ago);
        let before = unix_now();
        let out = tick_run(&place, &[]);
        let after = unix_now();
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
        let line = stdout_of(&out);
        let (decision, reason) = (tick_token(&line, "decision").unwrap_or_default(), tick_token(&line, "reason").unwrap_or_default());
        let text = fs::read_to_string(status_last_path(&place)).unwrap_or_default();
        let ts = text.strip_prefix("ts=").and_then(|rest| rest.split(' ').next()).and_then(|secs| secs.parse::<u64>().ok());
        assert!(ts.is_some_and(|secs| (before..=after).contains(&secs)), "ts は撃った時刻: {text:?}");
        let ts = ts.unwrap_or_default();
        assert_eq!(text, format!("ts={ts} decision={decision} reason={reason}\n"), "判定行と同じ 2 語: {line}");
    }
    let text = fs::read_to_string(status_last_path(&place)).unwrap_or_default();
    assert!(text.ends_with(" decision=inject reason=-\n"), "注入の周は reason=-: {text:?}");
    assert!(!place.seat().join("tick-last.tmp").exists(), "一時 file は残らない");
}

/// (i) 周期の行を欠く写しの no-rule（rc 1）の周も打刻を書く。
#[test]
fn seat_tick_status_no_rule_round_still_stamps() {
    let place = tick_place(true);
    tick_silent_for(&place, 100);
    let rules = status_rules(&place, "seat.tick_interval_s");
    let out = tick_run(&place, &["--rules", &rules]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "no-rule は rc 1");
    let text = fs::read_to_string(status_last_path(&place)).unwrap_or_default();
    assert!(text.starts_with("ts=") && text.ends_with(" decision=error reason=no-rule\n"), "rc 1 の周も書く: {text:?}");
}

/// (j) 登録 row の無い target の tick は打刻を書かず、席の置き場の無い周は dir も作らない。status の `--target` に row が無ければ
/// rc 1・語 `no-row`・stdout 0 行。
#[test]
fn seat_tick_status_without_a_row_writes_no_stamp() {
    let bare = tick_place(false);
    tick_silent_for(&bare, 100);
    assert_eq!(rc_of(&tick_run(&bare, &[])), i32::from(RC_OK));
    assert!(!status_last_path(&bare).exists(), "row の無い席に打刻を書かない");
    fs::remove_dir_all(bare.seat()).ok();
    assert_eq!(rc_of(&tick_run(&bare, &[])), i32::from(RC_OK));
    assert!(!bare.seat().exists(), "席の置き場を作らない");
    let place = tick_place(true);
    let rules = status_rules(&place, "");
    let out = status_run(&place, &["--target", "zz:zz", "--rules", &rules]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "row の無い target は rc 1");
    assert!(stdout_of(&out).is_empty(), "stdout 0 行");
    assert_eq!(stderr_of(&out), "seat tick status: refused reason=no-row target=zz:zz\n");
}

/// (k) healthy は経過 ≤ 2 × 周期（周期 15）: 30 秒ちょうどは yes・31 秒は no・16 秒は yes、打刻が無ければ no と `last=- age=-`。
/// doctor の `tick=` も同じ 3 点で healthy / stale / healthy・無ければ absent（係数 1 と `<` の変異を別々の assert が落とす）。
#[test]
fn seat_tick_status_healthy_is_within_twice_the_interval() {
    let place = tick_place(true);
    let rules = status_rules(&place, "");
    let out = status_run(&place, &["--rules", &rules]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_OK), status_line("-", "-", "no", "on", ("-", "-"))), "打刻なし");
    assert_eq!(tick_token(&status_doctor_row(&place, &rules), "tick").as_deref(), Some("absent"), "doctor も打刻なし");
    for (ago, healthy, word) in [(30, "yes", "healthy"), (31, "no", "stale"), (16, "yes", "healthy")] {
        let (ts, text) = status_at(&place, &rules, ago);
        assert_eq!(text, status_line(&ts.to_string(), &ago.to_string(), healthy, "on", ("-", "-")), "経過 {ago} 秒");
        assert_eq!(status_doctor_at(&place, &rules, ago).as_deref(), Some(word), "doctor の経過 {ago} 秒");
    }
    fs::write(status_last_path(&place), "ts=x\n").ok();
    let out = status_run(&place, &["--rules", &rules, "--target", TICK_TARGET]);
    assert_eq!(stdout_of(&out), status_line("-", "-", "no", "on", ("-", "-")), "読めない打刻は no");
    assert_eq!(tick_token(&status_doctor_row(&place, &rules), "tick").as_deref(), Some("unreadable"), "doctor は unreadable");
}

/// (l) `heartbeat=` は停止の記録を映す（status と doctor の席の行）。
#[test]
fn seat_tick_status_heartbeat_reflects_the_off_record() {
    let place = tick_place(true);
    let rules = status_rules(&place, "");
    let heartbeat = |want: &str| {
        let out = status_run(&place, &["--rules", &rules]);
        assert_eq!(tick_token(&stdout_of(&out), "heartbeat").as_deref(), Some(want), "status: {}", stdout_of(&out));
        assert_eq!(tick_token(&status_doctor_row(&place, &rules), "heartbeat").as_deref(), Some(want), "doctor");
    };
    heartbeat("on");
    heartbeat_assert(&place.state, "off", &heartbeat_line("off", "off"));
    heartbeat("off");
    heartbeat_assert(&place.state, "on", &heartbeat_line("on", "on"));
    heartbeat("on");
}

/// (m) step / next は梯子の記録から: step は記録の段・next は次の段（段 + 1）の待ち − 経過（段 0 の記録なら段 1 の待ちで段 0 の待ち
/// ではない）・待ちを過ぎた記録は 0・次の段が列を越える記録（段 5）は `stopped`。
#[test]
fn seat_tick_status_step_and_next_follow_the_ladder_record() {
    let place = tick_place(true);
    let rules = status_rules(&place, "");
    let ladder = |step: u32, ago: u64| {
        let before = unix_now();
        tick_ladder_put(&place, before - ago, step, Some(1));
        let out = status_run(&place, &["--rules", &rules]);
        let spent = unix_now() - before;
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
        let text = stdout_of(&out);
        (tick_token(&text, "step"), tick_token(&text, "next").and_then(|next| next.parse::<u64>().ok()), spent, text)
    };
    let (step, next, spent, text) = ladder(0, 100);
    assert_eq!(step.as_deref(), Some("0"), "{text}");
    let want = TICK_LADDER[1] - 100;
    assert!(next.is_some_and(|secs| (want - spent..=want).contains(&secs)), "段 1 の待ち − 経過: {text}");
    let (step, next, _, text) = ladder(2, TICK_LADDER[3] + 10);
    assert_eq!((step.as_deref(), next), (Some("2"), Some(0)), "待ちを過ぎた記録: {text}");
    let (step, _, _, text) = ladder(5, 100);
    assert_eq!((step.as_deref(), tick_token(&text, "next").as_deref()), (Some("5"), Some("stopped")), "列を越える次の段: {text}");
}

/// (o) tick の 1 周の後の `seat heartbeat status` は `last=` / `decision=` / `reason=` を tick-last と同じ値で出す
/// （base では 3 欄とも `-` ＝ RED）。
#[test]
fn seat_tick_status_heartbeat_status_carries_the_tick_last() {
    let place = tick_place(true);
    tick_silent_for(&place, 100);
    assert_eq!(rc_of(&tick_run(&place, &[])), i32::from(RC_OK));
    let text = fs::read_to_string(status_last_path(&place)).unwrap_or_default();
    let ts = tick_token(&text, "ts").unwrap_or_default();
    assert!(!ts.is_empty(), "打刻が在る: {text:?}");
    let want = format!("seat heartbeat status: target={TICK_TARGET} heartbeat=on last={ts} decision=noop reason=stamp-recent\n");
    heartbeat_assert(&place.state, "status", &want);
}

/// (p) 周期の行を欠く写しで status は rc 1・語 `no-rule`・stdout 0 行、doctor の `tick=` は `tick-unit=` と同じ no-rule の語（rc 0）。
#[test]
fn seat_tick_status_missing_interval_row_is_no_rule() {
    let place = tick_place(true);
    status_last_put(&place, unix_now());
    let rules = status_rules(&place, "seat.tick_interval_s");
    let out = status_run(&place, &["--rules", &rules]);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "rc 1");
    assert!(stdout_of(&out).is_empty(), "stdout 0 行: {}", stdout_of(&out));
    assert_eq!(stderr_of(&out), "seat tick status: refused reason=no-rule\n");
    assert_eq!(tick_token(&status_doctor_row(&place, &rules), "tick").as_deref(), Some("no-rule:missing"), "doctor の語");
}

/// (q-1) 梯子の記録が dir の席は `step=unreadable next=unreadable` で他の欄は不変・rc 0。
#[test]
fn seat_tick_status_ladder_record_dir_is_unreadable() {
    let place = tick_place(true);
    let rules = status_rules(&place, "");
    fs::create_dir_all(tick_ladder_path(&place)).ok();
    let out = status_run(&place, &["--rules", &rules]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "報告であって判定でない: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), status_line("-", "-", "no", "on", ("unreadable", "unreadable")));
}

/// (q-2) 梯子の行を欠く写しで status は rc 1・語 `no-rule`・stdout 0 行、doctor の `tick=` は梯子の行を読まず healthy のまま。
#[test]
fn seat_tick_status_missing_ladder_row_is_no_rule_and_doctor_keeps_its_word() {
    let place = tick_place(true);
    let rules = status_rules(&place, "seat.pointer_ladder_s");
    let out = status_run(&place, &["--rules", &rules]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), String::new()), "rc 1・stdout 0 行");
    assert_eq!(stderr_of(&out), "seat tick status: refused reason=no-rule\n");
    assert_eq!(status_doctor_at(&place, &rules, 0).as_deref(), Some("healthy"), "doctor は周期の行だけ読む");
}
