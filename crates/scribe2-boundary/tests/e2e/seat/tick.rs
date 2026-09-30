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
    // 写しは段の上げの行（設計 §17 形 1）を持たないので、送った合図の末尾に ` alarm=idle-unset` が付く。
    let want: Vec<String> = (0..=5).map(|step| format!("{} alarm=idle-unset", tick_text_key(step))).collect();
    assert_eq!(texts, want, "合図は段 0〜5 の 6 本（列の長さ・段 5 は次が無い）");
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

// ───────────── 群は今の口座が墓標なら移る（account-lifecycle.md §38 行 ac・接頭辞 `seat_tick_judge_tombstone_`） ─────────────
//
// §9 の判定の fixture（[`judge_place`]）と §29 の予約の fixture（[`reserve_place`]）の口座の credential を墓標（`expiresAt` 0）に
// 書き換える。判定行は全体を等値で測らず末尾の `judged=` の語と event の数と記録で測る（行 ad の後に理由の語が変わっても緑）。

/// 口座 `label` の credential を墓標に書き換える（token の字は fixture の本文の字のまま）。
fn tombstone_put(place: &MovePlace, label: &str) {
    let file = place.state.join("accounts").join(label).join(".credentials.json");
    let body = format!("{{\"claudeAiOauth\":{{\"accessToken\":\"tok-{label}\",\"refreshToken\":\"r\",\"expiresAt\":0}}}}");
    fs::write(file, body).ok();
}

/// 置き場の event log の口座 `label` の実測の行（測れた・測れなかった）の件数。
fn tombstone_rows(place: &MovePlace, label: &str) -> usize {
    use vessel::fleet::{Allowance, EventKind};
    let events = vessel::fleet::store::read_all(&place.state).unwrap_or_default();
    let of = |allowance: &Allowance| match allowance {
        Allowance::Measured(found) => found.account == label,
        Allowance::Unmeasured(found) => found.account == label,
    };
    events.iter().filter(|event| event.kind == EventKind::AllowanceMeasured && event.allowance.as_ref().is_some_and(of)).count()
}

/// 実測の窓が開き直る時刻（遠い未来の番兵）。
const TOMBSTONE_FAR: &str = "2099-01-01T00:00:00Z";

/// 口座 `label` の鮮度の内側の実測（5 時間窓・7 日窓・モデル別窓〔Fable〕とも 10%・10 秒前の event）を event log へ直に置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn tombstone_fresh(place: &MovePlace, label: &str) {
    use vessel::fleet::{Allowance, Event, EventKind, Measured, WindowKind};
    let policy = vessel::fleet::store::LockPolicy::embedded().expect("lock の規則を読める");
    let ts = vessel::fleet::cli::format_utc(unix_now() - 10);
    for window in [WindowKind::FiveHour, WindowKind::SevenDay, WindowKind::SevenDayModel] {
        let measured = Measured {
            account: label.to_owned(),
            window,
            model: (window == WindowKind::SevenDayModel).then(|| "Fable".to_owned()),
            endpoint: "oauth-usage".to_owned(),
            used_pct: 10,
            resets_at: Some(TOMBSTONE_FAR.to_owned()),
        };
        let event = Event {
            schema: vessel::fleet::SCHEMA,
            ts: ts.clone(),
            kind: EventKind::AllowanceMeasured,
            run: String::new(),
            bead: String::new(),
            host: "h".to_owned(),
            actor: EventKind::AllowanceMeasured.default_actor().to_owned(),
            stage: None,
            seat: None,
            pid: None,
            detail: None,
            allowance: Some(Allowance::Measured(measured)),
            registration: None,
            mark: None,
            account: None,
            cost: None,
            rule: None,
            case: None,
        };
        vessel::fleet::store::append(&place.state, &event, policy).expect("実測を置ける");
    }
}

/// (f) 5 時間窓 [10, 10]（A も B も閾値未満）で A（群の種）の credential が墓標 → `judged=moved:<B>`・群の記録が B・承認 event 1
/// （逐語は群の宣言の行の逐語）・偽 client の呼出は B の 1 回だけ（A は測らない・base は `judged=stay` ＝ RED）。
#[test]
fn seat_tick_judge_tombstone_current_account_moves_the_group_without_measuring_it() {
    use vessel::fleet::EventKind;
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [10, 10]);
    tombstone_put(&place, MOVE_A);
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert!(stdout_of(&out).ends_with(&format!(" judged=moved:{MOVE_B}\n")), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    let record = fs::read_to_string(move_groups_dir(&root).join(format!("{MOVE_GROUP}.account"))).unwrap_or_default();
    assert!(record.starts_with(&format!("account={MOVE_B}\n")) && record.contains(&format!("previous={MOVE_A}\n")), "{record}");
    let events = vessel::fleet::store::read_all(&place.state).unwrap_or_default();
    let moved: Vec<_> = events.iter().filter(|event| event.kind == EventKind::GroupMoved).collect();
    assert_eq!(moved.len(), 1, "承認 event 1");
    let host = place.state.join("host.toml");
    let text = fs::read_to_string(&host).unwrap_or_default();
    let head = text.lines().position(|line| line == "[[account-group]]").map_or(0, |at| at + 1);
    let block = text.lines().skip(head - 1).collect::<Vec<_>>().join("\n");
    let words = format!("{}:{head}\n{block}", host.display());
    assert_eq!(moved.first().and_then(|event| event.detail.as_deref()), Some(words.as_str()), "承認の逐語は群の宣言の行の逐語");
    assert_eq!(judge_calls(&place), 1, "偽 client の呼出は B の 1 回だけ");
    assert_eq!(tombstone_rows(&place, MOVE_A), 0, "墓標の A は測らない");
}

/// (g) A は逼迫（5 時間窓 90）・B の credential が墓標で B の鮮度の内側の実測（10%）が在る → `judged=none`・断りの event 1・記録不変
/// （墓標の候補を予約しない・base は B を予約して `judged=moved:<B>` ＝ RED）。
#[test]
fn seat_tick_judge_tombstone_candidate_is_not_reserved_even_with_a_fresh_measurement() {
    use vessel::fleet::EventKind;
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [90, 10]);
    tombstone_put(&place, MOVE_B);
    tombstone_fresh(&place, MOVE_B);
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert!(stdout_of(&out).ends_with(" judged=none\n"), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    assert_eq!(judge_events(&place, EventKind::GroupMoveRefused), 1, "断りの event 1");
    assert_eq!(judge_events(&place, EventKind::GroupMoved), 0, "承認 event 0");
    assert!(!move_groups_dir(&root).join(format!("{MOVE_GROUP}.account")).exists(), "記録不変");
}

/// (h) A も B も墓標 → `judged=none`・断りの event 1・記録不変・A の実測の行（測れなかったを含む）は 0 件（墓標の口座を測らない・
/// base は A を測り `AllowanceUnmeasured` の行が 1 件 ＝ RED）。
#[test]
fn seat_tick_judge_tombstone_every_account_dead_refuses_without_measuring() {
    use vessel::fleet::EventKind;
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [10, 10]);
    tombstone_put(&place, MOVE_A);
    tombstone_put(&place, MOVE_B);
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert!(stdout_of(&out).ends_with(" judged=none\n"), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    assert_eq!(judge_events(&place, EventKind::GroupMoveRefused), 1, "断りの event 1");
    assert!(!move_groups_dir(&root).join(format!("{MOVE_GROUP}.account")).exists(), "記録不変");
    assert_eq!(tombstone_rows(&place, MOVE_A), 0, "A の実測の行 0 件");
    assert_eq!(judge_calls(&place), 0, "偽 client は呼ばれない");
}

/// (h2) 群 2 つ（Tier1 = [D, B, C]・種 D、Tier2 = [A, B, C]・種 A が 90）で Tier1 の種 D の credential が墓標 → 墓標の今の口座の
/// Tier1 が予約 B を持ち、Tier2 は C へ移る（`judged=moved:acct-c`・Tier2 の記録が C・Tier1 の記録は書かない・base は D を逼迫でない
/// と読み `judged=moved:acct-b` ＝ RED）。
#[test]
fn seat_tick_judge_tombstone_earlier_group_with_a_dead_current_holds_a_reserve() {
    let root = tmp();
    let place = reserve_place(&root);
    tombstone_put(&place, RESERVE_D);
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert!(stdout_of(&out).ends_with(&format!(" judged=moved:{RESERVE_C}\n")), "stdout={} stderr={}", stdout_of(&out), stderr_of(&out));
    assert_eq!(reserve_record(&root).as_deref(), Some(RESERVE_C), "Tier2 の記録は C");
    assert!(!move_groups_dir(&root).join("Tier1.account").exists(), "Tier1 の記録は書かない");
    assert_eq!(tombstone_rows(&place, RESERVE_D), 0, "墓標の D は測らない");
}

// ───────────── 管理 tick の墓標の門（account-lifecycle.md §38 行 ad・接頭辞 `seat_tick_tombstone_`） ─────────────
//
// §9 の判定の fixture（[`judge_place`]）の row の口座 A を墓標に書き換える。群の外の席は anchor を群の置き場の外に置く。

/// 口座 `label` の credential の期限を `at_ms`（epoch ミリ秒）に書き換える（0 は墓標）。
fn tombstone_expires(place: &MovePlace, label: &str, at_ms: u64) {
    let file = place.state.join("accounts").join(label).join(".credentials.json");
    fs::write(file, format!("{{\"claudeAiOauth\":{{\"accessToken\":\"tok-{label}\",\"refreshToken\":\"r\",\"expiresAt\":{at_ms}}}}}")).ok();
}

/// 席の打刻の最終行を `ago` 秒前の `(state, event)` 1 行に書き直す。
fn tombstone_stamp(place: &MovePlace, (state, event): (&str, &str), ago: u64) {
    let seat = seat_dir_of(&place.state, TICK_SEAT);
    fs::write(state_file(&seat), format!("{}\n", stamp_line(state, event, unix_now() - ago, "sid-move"))).ok();
}

/// 群の外の席（row の口座 A・最終行 40 分前の Idle・入力欄が空）の置き場（A の credential は未来の期限＝墓標でない）。
fn tombstone_silent(root: &Path) -> MovePlace {
    let place = judge_place(root, "/elsewhere", [10, 10]);
    tombstone_stamp(&place, ("idle", "Stop"), TICK_STALE + 600);
    place
}

/// 窓が shell（前面 `bash`・子なしの prompt）の群の席の置き場（row の口座 A・群の記録なし・判定の打刻なし）。
fn tombstone_shell(root: &Path) -> MovePlace {
    let place = judge_place(root, MOVE_ANCHOR, [10, 10]);
    fs::write(place.at(MOVE_FRONT), "bash\n").ok();
    fs::write(place.at(TICK_PANE), PANE_SHELL_PROMPT).ok();
    place
}

/// 撃って起こした周（`move=launch`・`judged=<judged>`）を測り、起動行（1 行）を返す。
fn tombstone_launch(place: &MovePlace, judged: &str) -> String {
    let out = move_run(place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let want = format!(
        "decision=move target={TICK_SEAT} reason=- pointer=- step=- consumed=- move=launch launched=launch-unconfirmed judged={judged}\n"
    );
    assert_eq!(stdout_of(&out), want, "判定行 1 行: stderr={}", stderr_of(&out));
    let texts = wake_texts(place);
    assert_eq!(texts.len(), 1, "起動行 1 行: {texts:?}");
    texts.first().cloned().unwrap_or_default()
}

/// 起動行が `has` の口座の設定 dir を持ち `lacks` の口座の設定 dir を持たない。
fn tombstone_assert_account(place: &MovePlace, text: &str, (has, lacks): (&str, &str)) {
    assert!(text.contains(&wake_account_dir(place, has)), "{has} の設定 dir を持つ起動行: {text}");
    assert!(!text.contains(&wake_account_dir(place, lacks)), "{lacks} の設定 dir を持たない起動行: {text}");
}

/// 群の記録の口座（無ければ `None`）。
fn tombstone_record(root: &Path) -> Option<String> {
    let text = fs::read_to_string(move_groups_dir(root).join(format!("{MOVE_GROUP}.account"))).ok()?;
    text.lines().find_map(|line| line.strip_prefix("account=").map(str::to_owned))
}

/// (j) 群の席の row の口座 A（種）が墓標・最終行が 10 秒前の Busy・窓が claude で入力欄が空 → 打刻の Busy で止まらず群の判定を撃ち
/// `judged=moved:<B>`・同じ周に移動の門が撃たれる（猶予 0 の写しで `/exit`・base は `busy` で止まり判定を撃たない ＝ RED）。
#[test]
fn seat_tick_tombstone_judges_the_group_past_a_busy_stamp() {
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [10, 10]);
    tombstone_put(&place, MOVE_A);
    tombstone_stamp(&place, ("busy", "UserPromptSubmit"), 10);
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let want = format!("decision=move target={TICK_SEAT} reason=- pointer=- step=- consumed=false move=exit launched=- judged=moved:{MOVE_B}\n");
    assert_eq!(stdout_of(&out), want, "stderr={}", stderr_of(&out));
    assert_eq!(tombstone_record(&root).as_deref(), Some(MOVE_B), "群の記録は B");
    assert_eq!(
        move_keys(&place),
        [format!("send-keys -t {TICK_TARGET} -l /exit"), format!("send-keys -t {TICK_TARGET} Enter")],
        "自席への key は移動の門の /exit だけ"
    );
    assert!(!place.state.join("seat").join(TICK_SEAT).join("pointer-ladder").exists(), "梯子の記録は書かれない");
}

/// (k) 群の外の席の row の口座が墓標・最終行が 40 分前の Idle・入力欄が空 → `noop reason=account-dead pointer=- step=-`・0 key・梯子の
/// 記録 0（base は合図を送る ＝ RED）。pane が `no prompt here` の周も `account-dead`（入力欄の門より前）。
#[test]
fn seat_tick_tombstone_sends_no_heartbeat_outside_a_group() {
    let root = tmp();
    let place = tombstone_silent(&root);
    tombstone_put(&place, MOVE_A);
    move_assert_quiet(&place, &move_noop("account-dead"));
    fs::write(place.at(TICK_PANE), "no prompt here\n").ok();
    move_assert_quiet(&place, &move_noop("account-dead"));
}

/// (l) 期限切れ（墓標でない）の credential は門を閉じず今どおり合図が 1 行。墓標の周の後で file を未来の期限に書き換えた次の周も
/// 合図が 1 行（毎周読む）。
#[test]
fn seat_tick_tombstone_expiry_is_not_death_and_relogin_reopens() {
    let root = tmp();
    let place = tombstone_silent(&root);
    tombstone_expires(&place, MOVE_A, 1000);
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), tick_inject(0), "期限切れは門を閉じない: stderr={}", stderr_of(&out));
    let again = tmp();
    let place = tombstone_silent(&again);
    tombstone_put(&place, MOVE_A);
    move_assert_quiet(&place, &move_noop("account-dead"));
    tombstone_expires(&place, MOVE_A, 4_102_444_800_000);
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), tick_inject(0), "再 login の次の周は合図: stderr={}", stderr_of(&out));
}

/// (m) (k) の席に停止の記録（heartbeat off）→ `reason=heartbeat-off`（停止の記録の門が先）。
#[test]
fn seat_tick_tombstone_heartbeat_off_comes_first() {
    let root = tmp();
    let place = tombstone_silent(&root);
    tombstone_put(&place, MOVE_A);
    heartbeat_assert(&place.state, "off", &heartbeat_line("off", "off"));
    move_assert_quiet(&place, &move_noop("heartbeat-off"));
}

/// (n) 記録 B を先に置き row の口座 A が墓標・pane が claude・猶予 0 → `move=exit`・`judged=-`（移動の門が墓標の読みより前・
/// 判定は撃たない）。
#[test]
fn seat_tick_tombstone_move_gate_comes_first_for_a_claude_pane() {
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [10, 10]);
    move_record(&root, MOVE_B);
    tombstone_put(&place, MOVE_A);
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), move_line("exit", "false", "-"), "stderr={}", stderr_of(&out));
    assert_eq!(judge_calls(&place), 0, "判定は撃たない");
}

/// (o) (n) で pane が shell → `move=launch`・起動行が B の設定 dir を持ち A のを持たない・`judged=-`（群の今の口座 B は墓標でない）。
#[test]
fn seat_tick_tombstone_shell_pane_wakes_the_moved_record() {
    let root = tmp();
    let place = tombstone_shell(&root);
    move_record(&root, MOVE_B);
    tombstone_put(&place, MOVE_A);
    let text = tombstone_launch(&place, "-");
    tombstone_assert_account(&place, &text, (MOVE_B, MOVE_A));
    assert_eq!(judge_calls(&place), 0, "判定は撃たない");
}

/// (p) row の口座 A（種・記録なし）が墓標・pane が shell → 起こす前に群の判定を撃ち `judged=moved:<B>`・起動行が B の設定 dir を持ち
/// A のを持たない・群の記録 B・承認 event 1（base は判定を撃たず墓標の A で起こし `judged=-` ＝ RED）。
#[test]
fn seat_tick_tombstone_shell_pane_judges_before_waking() {
    let root = tmp();
    let place = tombstone_shell(&root);
    tombstone_put(&place, MOVE_A);
    let text = tombstone_launch(&place, &format!("moved:{MOVE_B}"));
    tombstone_assert_account(&place, &text, (MOVE_B, MOVE_A));
    assert_eq!(tombstone_record(&root).as_deref(), Some(MOVE_B), "群の記録は B");
    assert_eq!(judge_events(&place, vessel::fleet::EventKind::GroupMoved), 1, "承認 event 1");
}

/// (q) (p) で B も墓標 → `judged=none`・起動行が A の設定 dir を持つ・群の記録なし・断りの event 1（墓標の口座のまま起こす）。
#[test]
fn seat_tick_tombstone_shell_pane_wakes_the_dead_account_without_candidates() {
    let root = tmp();
    let place = tombstone_shell(&root);
    tombstone_put(&place, MOVE_A);
    tombstone_put(&place, MOVE_B);
    let text = tombstone_launch(&place, "none");
    tombstone_assert_account(&place, &text, (MOVE_A, MOVE_B));
    assert_eq!(tombstone_record(&root), None, "群の記録なし");
    assert_eq!(judge_events(&place, vessel::fleet::EventKind::GroupMoveRefused), 1, "断りの event 1");
}

/// (r) (p) の置き場に鮮度の窓の内側の判定の打刻を先に置く → `judged=-`・起動行が A の設定 dir を持ち B のを持たない・群の記録なし
/// （判定は窓に 1 回まで・墓標の口座のまま起こす）。
#[test]
fn seat_tick_tombstone_shell_pane_wakes_the_dead_account_inside_the_window() {
    let root = tmp();
    let place = tombstone_shell(&root);
    tombstone_put(&place, MOVE_A);
    fs::write(judge_stamp(&root), format!("{}\n", unix_now())).ok();
    let text = tombstone_launch(&place, "-");
    tombstone_assert_account(&place, &text, (MOVE_A, MOVE_B));
    assert_eq!(tombstone_record(&root), None, "群の記録なし");
    assert_eq!(judge_calls(&place), 0, "計測 0");
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

/// 合図の Busy（`at` + 2）。
const SAVED_BUSY: (&str, &str, i64) = ("busy", "UserPromptSubmit", 2);
/// 合図の turn の終わり（`at` + 50 の Stop の Idle）。
const SAVED_STOP: (&str, &str, i64) = ("idle", "Stop", 50);

/// (a) 同じ移動の記録で `at` = 今 − 100・打刻が Busy（at + 2）と Stop の Idle（at + 50）→ 猶予の内側でも `move=exit`・`/exit` 1 回・
/// 合図の記録は不変（base では `move=wait` ＝ RED）。
#[test]
fn seat_tick_saved_turn_end_after_the_signal_sends_the_exit_early() {
    let root = tmp();
    let place = saved_place(&root, 100, Some(&[SAVED_BUSY, SAVED_STOP]));
    let body = fs::read_to_string(grace_signal(&place)).ok();
    evacuate_assert_exit(&place);
    assert_eq!(fs::read_to_string(grace_signal(&place)).ok(), body, "合図の記録は書き換えない");
    assert!(!move_groups_dir(&root).join("lock").exists(), "lock は残らない");
}

/// (b) 最終行が Busy（at + 2 の Busy だけ＝合図の turn が走っている）→ `move=wait`・0 key。
#[test]
fn seat_tick_saved_busy_last_line_waits() {
    let root = tmp();
    let place = saved_place(&root, 100, Some(&[SAVED_BUSY]));
    move_assert_quiet(&place, &move_line("wait", "-", "-"));
}

/// (c) 打刻が `at` より前の Stop の Idle だけ（合図に応えていない）→ `move=wait`・0 key。
#[test]
fn seat_tick_saved_stop_before_the_signal_waits() {
    let root = tmp();
    let place = saved_place(&root, 100, Some(&[("idle", "Stop", -10)]));
    move_assert_quiet(&place, &move_line("wait", "-", "-"));
}

/// (d) `at` − 5 の Busy と `at` + 20 の Stop の Idle（合図の前から走っていた turn の終わり）→ `move=wait`・0 key。
#[test]
fn seat_tick_saved_turn_started_before_the_signal_waits() {
    let root = tmp();
    let place = saved_place(&root, 100, Some(&[("busy", "UserPromptSubmit", -5), ("idle", "Stop", 20)]));
    move_assert_quiet(&place, &move_line("wait", "-", "-"));
}

/// (e) `at` + 2 の Busy と `at` + 50 の SessionStart の Idle（turn の途中の圧縮）→ `move=wait`・0 key。
#[test]
fn seat_tick_saved_session_start_last_line_waits() {
    let root = tmp();
    let place = saved_place(&root, 100, Some(&[SAVED_BUSY, ("idle", "SessionStart", 50)]));
    move_assert_quiet(&place, &move_line("wait", "-", "-"));
}

/// (f) 打刻の file が無い席は猶予の上限で `/exit`: `at` = 今 − 301 は `move=exit`・`at` = 今 − 100 は `move=wait`（打刻は `/exit` を
/// 早めるためにだけ読む・消費は打刻が無いので `unknown:state-missing`）。
#[test]
fn seat_tick_saved_missing_stamps_fall_back_to_the_grace_cap() {
    let root = tmp();
    let place = saved_place(&root, GRACE_S + 1, None);
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), move_line("exit", "unknown:state-missing", "-"), "上限を越えた周は move=exit");
    assert_eq!(move_keys(&place), [format!("send-keys -t {TICK_TARGET} -l /exit"), format!("send-keys -t {TICK_TARGET} Enter")], "/exit 1 行");
    let root = tmp();
    let place = saved_place(&root, 100, None);
    move_assert_quiet(&place, &move_line("wait", "-", "-"));
}

/// (g) 記録の無い席へ送る合図の text は形 5 の字面（残り 300 秒）。
#[test]
fn seat_tick_saved_signal_text_names_the_turn_end_and_the_cap() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    grace_assert_signal(&place, MOVE_TS);
    let want = format!(
        "send-keys -t {TICK_TARGET} -l {NAME} group: evacuate group={MOVE_GROUP} to={MOVE_B} — 新しい subagent を起こさず、走っている \
         subagent は /exit で落ちる前提で依頼の要旨と出力 file の path を台帳の notes に書き、作業記憶を台帳と git に残して turn を\
         終える（turn が終わると器が /exit を送る・遅くとも 300 秒の後）"
    );
    assert!(move_keys(&place).contains(&want), "形 5 の字面・残り 300 秒: {:?}", move_keys(&place));
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
    let want = format!("seat heartbeat status: target={TICK_TARGET} heartbeat=on heartbeat_by=default last={ts} decision=noop reason=stamp-recent\n");
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

/// 行 t（設計 seat-heartbeat.md §16）の歯の 1 周: 置き場の event log に便 1 本の event（`RunCreated` と段 `stage` の
/// `RunStage`・ts はどちらも `ago` 秒前）を足し（`stage` が `None` なら便 0 本）、黙った席へ 1 回撃って偽 tmux へ送った合図の
/// text を返す。撃つ前後で event log は 1 byte も変わらない（tick は列の 1 周も起こしも撃たない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn tick_facts_sent(stage: Option<&str>, ago: u64) -> String {
    let place = tick_place(true);
    let log = vessel::fleet::store::events_path(&place.state);
    if let Some(stage) = stage {
        let ts = vessel::fleet::cli::format_utc(unix_now().saturating_sub(ago));
        let mut text = fs::read_to_string(&log).expect("登録 row の event log が在る");
        for (kind, at) in [("RunCreated", "Intake"), ("RunStage", stage)] {
            text.push_str(&format!(
                "{{\"schema\":1,\"ts\":\"{ts}\",\"kind\":\"{kind}\",\"run\":\"r-facts\",\"bead\":\"s2-facts.1\",\"host\":\"h\",\"actor\":\"machine\",\"stage\":\"{at}\"}}\n"
            ));
        }
        fs::write(&log, text).expect("便の event を足せる");
    }
    tick_silent_for(&place, TICK_STALE + 60);
    let before = fs::read(&log).expect("event log を読める");
    let out = tick_run(&place, &[]);
    assert_eq!(stdout_of(&out), tick_inject(0), "合図を 1 回送る周: stderr={}", stderr_of(&out));
    assert_eq!(fs::read(&log).expect("event log を読める"), before, "tick は event log を書かない");
    assert!(!place.at(TICK_CLIENT).exists(), "偽 client は呼ばれない");
    let prefix = format!("send-keys -t {TICK_TARGET} -l ");
    let texts: Vec<String> = tick_keys(&place).iter().filter_map(|key| key.strip_prefix(&prefix).map(str::to_owned)).collect();
    assert_eq!(texts.len(), 1, "text の送りは 1 回: {texts:?}");
    texts.into_iter().next().unwrap_or_default()
}

/// 段 0 の合図の文面（`signal` の返り値）の後ろに並列の実測の字面 `tail` が付いた送り（`held=` は含まない）。
fn tick_facts_want(tail: &str) -> String {
    let signal = tick_signal(0);
    format!("{}{tail}", signal.strip_suffix(" live=0 idle=-").unwrap_or(&signal))
}

/// (t-a) Landed の便 1 本・最後の event が 7230 秒前 → 合図の末尾が ` live=0 idle=120m`（分は切り捨て）。埋め込みの
/// `seat.idle_alarm_s` = 900 で 120 分 × 60 ≥ 900 の周は段が上がり、末尾に ` alarm=idle` が続く（設計 §17 形 3 (c)）。
#[test]
fn seat_tick_facts_landed_run_7230s_ago_ends_live_zero_idle_120m() {
    let sent = tick_facts_sent(Some("Landed"), 7230);
    assert_eq!(sent, tick_facts_want(" live=0 idle=120m alarm=idle"));
    assert!(!sent.contains("held="), "列の結果なし: {sent}");
}

/// (t-b) Spawned の便 1 本 → ` live=1 idle=-`（live が 1 本以上の周の 0 本の分数は値なし）。
#[test]
fn seat_tick_facts_spawned_run_ends_live_one_idle_absent() {
    let sent = tick_facts_sent(Some("Spawned"), 60);
    assert_eq!(sent, tick_facts_want(" live=1 idle=-"));
    assert!(!sent.contains("held="), "列の結果なし: {sent}");
}

/// (t-c) 便 0 本 → ` live=0 idle=-`（便の無い周の 0 本の分数は値なし）。
#[test]
fn seat_tick_facts_no_runs_ends_live_zero_idle_absent() {
    let sent = tick_facts_sent(None, 0);
    assert_eq!(sent, tick_facts_want(" live=0 idle=-"));
    assert!(!sent.contains("held="), "列の結果なし: {sent}");
}

/// (t-d) verdict の読めない Gated の便 → ` live=? idle=?`（測れないを 0 本に読み替えない）。
#[test]
fn seat_tick_facts_gated_run_without_a_verdict_ends_unmeasured() {
    let sent = tick_facts_sent(Some("Gated"), 7230);
    assert_eq!(sent, tick_facts_want(" live=? idle=?"));
    assert!(!sent.contains("held="), "列の結果なし: {sent}");
}

/// 行 u（設計 seat-heartbeat.md §17）の歯の置き場: 置き場の event log に便 1 本の event（`RunCreated` と段 `stage` の
/// `RunStage`・ts はどちらも `ago` 秒前）を足し、席の打刻を `silent` 秒前の Idle の Stop にする。rules の写し（`tick_rules_text`
/// の本文・`alarm` が在ればその後ろに `seat.idle_alarm_s` の行）の path を対で返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn tick_idle_place(stage: &str, ago: u64, silent: u64, alarm: Option<u64>) -> (TickPlace, String) {
    let place = tick_place(true);
    let log = vessel::fleet::store::events_path(&place.state);
    let ts = vessel::fleet::cli::format_utc(unix_now().saturating_sub(ago));
    let mut text = fs::read_to_string(&log).expect("登録 row の event log が在る");
    for (kind, at) in [("RunCreated", "Intake"), ("RunStage", stage)] {
        text.push_str(&format!(
            "{{\"schema\":1,\"ts\":\"{ts}\",\"kind\":\"{kind}\",\"run\":\"r-idle\",\"bead\":\"s2-idle.1\",\"host\":\"h\",\"actor\":\"machine\",\"stage\":\"{at}\"}}\n"
        ));
    }
    fs::write(&log, text).expect("便の event を足せる");
    tick_silent_for(&place, silent);
    let row = alarm.map_or_else(String::new, |secs| {
        tick_rules_body(&[("seat.idle_alarm_s", "SeatIdleAlarmS", secs.to_string())]).replacen("schema = 1\n", "", 1)
    });
    let rules = fixture(&place.dir, "idle-alarm.toml", &format!("{}{row}", tick_rules_text("", None)));
    (place, rules)
}

/// 写しで 1 回撃ち、判定行と偽 tmux へ送った合図の text の列（`-l` の呼出の行）を返す。
fn tick_idle_run(place: &TickPlace, rules: &str) -> (String, Vec<String>) {
    let out = tick_run(place, &["--rules", rules]);
    let prefix = format!("send-keys -t {TICK_TARGET} -l ");
    let texts = tick_keys(place).iter().filter_map(|key| key.strip_prefix(&prefix).map(str::to_owned)).collect();
    (stdout_of(&out), texts)
}

/// (u-a) 値 60・Landed の便 1 本の最後の event が 120 秒前（2 分 × 60 ≥ 60）・席の打刻が 90 秒前（`seat.tick_stale_s` より新しい）
/// の周は黙りの門が 60 秒に縮み、合図が 1 回出て末尾が ` alarm=idle`（base と門を縮めない実装は `stamp-recent` の noop）。
#[test]
fn seat_tick_idle_alarm_shortens_the_silence_gate_and_ends_with_alarm_idle() {
    let (place, rules) = tick_idle_place("Landed", 120, 90, Some(60));
    let (line, texts) = tick_idle_run(&place, &rules);
    assert_eq!(line, tick_inject(0), "打刻が 90 秒前でも値 60 の門を越えた周は送る");
    assert_eq!(texts, [tick_facts_want(" live=0 idle=2m alarm=idle")], "合図 1 回・末尾は alarm=idle");
}

/// (u-b) 同じ周で梯子の記録が段 2・基準が今の digest・`sent_at` が列の最初の待ちより前で段 3 の待ちより後 → 段を 0 に留めて
/// 合図は `step=0`・記録の段は 0（段を留めない実装は段 3 の床で `wait` の noop）。
#[test]
fn seat_tick_idle_alarm_holds_the_ladder_at_step_zero() {
    let (place, rules) = tick_idle_place("Landed", 120, 90, Some(60));
    let now = unix_now();
    tick_stamps(&place, &[("idle", "Stop", now - 90)]);
    tick_ladder_put(&place, now - TICK_STALE - 100, 2, Some(now - 90));
    let (line, texts) = tick_idle_run(&place, &rules);
    assert_eq!(line, tick_inject(0), "段の候補 3 を段 0 に留め、列の最初の待ちを越えた床で送る");
    assert_eq!(texts, [tick_facts_want(" live=0 idle=2m alarm=idle")], "合図は step=0");
    assert_eq!(tick_ladder(&place).map(|(_, step, digest)| (step, digest)), Some((0, None)), "記録の段は 0（段を上らせない）");
}

/// (u-c) 値 0 の周と live 1 本（Spawned）の周は段を上げず、打刻 90 秒前の周は `stamp-recent` の noop（値 0 は語も足さない）。
#[test]
fn seat_tick_idle_alarm_zero_value_or_a_live_run_stays_quiet() {
    for (stage, alarm) in [("Landed", 0), ("Spawned", 60)] {
        let (place, rules) = tick_idle_place(stage, 120, 90, Some(alarm));
        let (line, texts) = tick_idle_run(&place, &rules);
        assert_eq!(line, tick_noop("stamp-recent", "wait:0", "0"), "{stage} 値 {alarm} は上げない");
        assert!(texts.is_empty(), "{stage} 値 {alarm}: 0 key: {texts:?}");
        assert_eq!(tick_ladder(&place), None, "{stage} 値 {alarm}: 記録は書かない");
    }
}

/// (u-d) 席の打刻が 150 秒前・最後の event が 170 秒前（2 分・切り捨て）の周: 値 120 は 2 × 60 ≥ 120 で上げて合図が出、値 121 は
/// 上げず `stamp-recent` の noop（秒のまま 170 ≥ 121 と比べる実装は 121 でも上げる）。
#[test]
fn seat_tick_idle_alarm_compares_whole_minutes_with_the_seconds() {
    let (place, rules) = tick_idle_place("Landed", 170, 150, Some(121));
    let (line, texts) = tick_idle_run(&place, &rules);
    assert_eq!(line, tick_noop("stamp-recent", "wait:0", "0"), "2 分 × 60 = 120 < 121 は上げない");
    assert!(texts.is_empty(), "値 121: 0 key: {texts:?}");
    let (place, rules) = tick_idle_place("Landed", 170, 150, Some(120));
    let (line, texts) = tick_idle_run(&place, &rules);
    assert_eq!(line, tick_inject(0), "2 分 × 60 ≥ 120 は上げて送る");
    assert_eq!(texts, [tick_facts_want(" live=0 idle=2m alarm=idle")], "値 120: 末尾は alarm=idle");
}

/// (u-e) 段の上げの行が無い写し: (u-a) と同じ周は門を縮めず `stamp-recent` の noop で、打刻が `seat.tick_stale_s` を越えた周の
/// 合図の末尾は ` alarm=idle-unset`（決めた値が無いことを黙って「上げない」に畳まない）。
#[test]
fn seat_tick_idle_alarm_without_the_row_keeps_the_gate_and_says_unset() {
    let (place, rules) = tick_idle_place("Landed", 120, 90, None);
    let (line, texts) = tick_idle_run(&place, &rules);
    assert_eq!(line, tick_noop("stamp-recent", "wait:0", "0"), "行の無い写しは門を縮めない");
    assert!(texts.is_empty(), "0 key: {texts:?}");
    tick_silent_for(&place, TICK_STALE + 60);
    let (line, texts) = tick_idle_run(&place, &rules);
    assert_eq!(line, tick_inject(0), "打刻の古い周は送る");
    assert_eq!(texts, [tick_facts_want(" live=0 idle=2m alarm=idle-unset")], "末尾は alarm=idle-unset");
}

/// 行 y（設計 dispatcher.md §27 形 3 / 4）の歯の置き場: 席の置き場の事前審査の dir（`<state>/pipe/precheck`）を作り、`bundle` が
/// 在れば確定を持つ結果の file 1 つと、初めて見た時刻が `bundle` 秒前の束の file 1 つを置く（無ければ dir だけ）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn tick_precheck_put(place: &TickPlace, bundle: Option<u64>) {
    let dir = place.state.join("pipe").join("precheck");
    fs::create_dir_all(&dir).expect("事前審査の dir を作れる");
    let Some(ago) = bundle else {
        return;
    };
    let (root, id) = ("write-set-item-unresolved at=files:src/nowhere.rs", "0123456789abcdef");
    let result = format!("key=k\nresult=firm:1,provisional:0\nfinding=firm name={root} new=true reason=r\n");
    fs::write(dir.join("s2-pre.2"), result).expect("結果の file を書ける");
    fs::create_dir_all(dir.join("bundle")).expect("束の dir を作れる");
    let first = unix_now().saturating_sub(ago);
    let body = format!("id={id}\nroot={root}\nfirst_seen={first}\nrow=s2-pre.2 pointer=docs/design/toy.md#b\n");
    fs::write(dir.join("bundle").join(id), body).expect("束の file を書ける");
}

/// `tick_rules_text` の本文の後ろに段の上げの 2 行（`idle` と `precheck` の在る方）を足した写しの path。
fn tick_precheck_rules(place: &TickPlace, idle: Option<u64>, precheck: Option<u64>) -> String {
    let mut rows = Vec::new();
    rows.extend(idle.map(|secs| ("seat.idle_alarm_s", "SeatIdleAlarmS", secs.to_string())));
    rows.extend(precheck.map(|secs| ("seat.precheck_alarm_s", "SeatPrecheckAlarmS", secs.to_string())));
    let extra = tick_rules_body(&rows).replacen("schema = 1\n", "", 1);
    fixture(&place.dir, "precheck-alarm.toml", &format!("{}{extra}", tick_rules_text("", None)))
}

/// (y-a) 事前審査の dir の在る置き場（束 0）の合図の末尾に ` precheck=0/0:0` が付き、dir の無い置き場は付かない（埋め込みの
/// manifest・便 0 本・打刻は `seat.tick_stale_s` を越えた周）。
#[test]
fn seat_tick_precheck_tail_follows_the_dir() {
    for (made, tail) in [(true, " live=0 idle=- precheck=0/0:0"), (false, " live=0 idle=-")] {
        let place = tick_place(true);
        if made {
            tick_precheck_put(&place, None);
        }
        tick_silent_for(&place, TICK_STALE + 60);
        let out = tick_run(&place, &[]);
        assert_eq!(stdout_of(&out), tick_inject(0), "dir {made}: 合図を 1 回送る周: stderr={}", stderr_of(&out));
        let prefix = format!("send-keys -t {TICK_TARGET} -l ");
        let texts: Vec<String> = tick_keys(&place).iter().filter_map(|key| key.strip_prefix(&prefix).map(str::to_owned)).collect();
        assert_eq!(texts, [tick_facts_want(tail)], "dir {made}: 末尾");
    }
}

/// (y-b) 値 60 の写しで束の時刻が 120 秒前・席の打刻が 90 秒前（`seat.tick_stale_s` より新しい）の周は黙りの門が 60 秒に縮み、合図が
/// 1 回出て ` alarm=` の列に `precheck`（base と門を縮めない実装は `stamp-recent` の noop）。値 0 の同じ周は上げない。
#[test]
fn seat_tick_precheck_alarm_shortens_the_silence_gate() {
    let place = tick_place(true);
    tick_precheck_put(&place, Some(120));
    tick_silent_for(&place, 90);
    let (line, texts) = tick_idle_run(&place, &tick_precheck_rules(&place, Some(900), Some(60)));
    assert_eq!(line, tick_inject(0), "打刻が 90 秒前でも値 60 の門を越えた周は送る");
    assert_eq!(texts, [tick_facts_want(" live=0 idle=- precheck=1/1:1 alarm=precheck")], "合図 1 回・alarm= の列に precheck");
    let place = tick_place(true);
    tick_precheck_put(&place, Some(120));
    tick_silent_for(&place, 90);
    let (line, texts) = tick_idle_run(&place, &tick_precheck_rules(&place, Some(900), Some(0)));
    assert_eq!(line, tick_noop("stamp-recent", "wait:0", "0"), "値 0 は上げない");
    assert!(texts.is_empty(), "値 0: 0 key: {texts:?}");
}

/// (y-c) 段の上げの行の無い写しで事前審査の dir の在る置き場: (y-b) と同じ周は門を縮めず `stamp-recent` の noop で、打刻が
/// `seat.tick_stale_s` を越えた周の合図の ` alarm=` の列は u の語の後ろに `precheck-unset`。
#[test]
fn seat_tick_precheck_without_the_row_keeps_the_gate_and_says_unset() {
    let place = tick_place(true);
    tick_precheck_put(&place, Some(120));
    tick_silent_for(&place, 90);
    let rules = tick_precheck_rules(&place, None, None);
    let (line, texts) = tick_idle_run(&place, &rules);
    assert_eq!(line, tick_noop("stamp-recent", "wait:0", "0"), "行の無い写しは門を縮めない");
    assert!(texts.is_empty(), "0 key: {texts:?}");
    tick_silent_for(&place, TICK_STALE + 60);
    let (line, texts) = tick_idle_run(&place, &rules);
    assert_eq!(line, tick_inject(0), "打刻の古い周は送る");
    assert_eq!(
        texts,
        [tick_facts_want(" live=0 idle=- precheck=1/1:1 alarm=idle-unset,precheck-unset")],
        "alarm= の列は u の語の後ろに precheck-unset"
    );
}

// ───── seat tick status の器の判定の 3 欄（seat-heartbeat.md §20 形 1〜4・契約表の行 x・接頭辞 `seat_tick_reopens_` / `seat_tick_pending_`） ─────
//
// §12 の status の fixture（[`tick_place`]・[`status_run`]）と §13 / §14 / §18 の移動の fixture（[`move_place`]・[`move_record`]・
// [`grace_signal_put`]・[`saved_place`]）をそのまま使い、実測行を窓ごとの reset つきで積む helper 1 つ（[`reopens_rows`]）を足す。
// 字面は契約の key の表から組む（器の helper を借りない）。

/// 当たっている 5 時間窓の reset（R1）。
const REOPENS_R1: &str = "2099-01-01T05:00:00Z";
/// 7 日窓の reset（R2・R1 と R3 より遅い）。
const REOPENS_R2: &str = "2099-01-07T00:00:00Z";
/// 席の model のモデル別窓の reset（R3・R1 より遅く R2 より早い）。
const REOPENS_R3: &str = "2099-01-03T00:00:00Z";
/// 席の登録 row の model（表示名）。
const REOPENS_MODEL: &str = "Opus";

/// 登録 row（口座 [`TICK_ACCOUNT`]・`model` が在れば `--model`）の在る status の置き場。
fn reopens_place(model: Option<&str>) -> TickPlace {
    let place = tick_place(false);
    let (launch, state) = (fixture(&place.dir, "launch.txt", "claude\n"), place.state.display().to_string());
    let mut args = vec![
        "register", "--state-dir", state.as_str(), "--target", TICK_TARGET, "--role", "orchestrator", "--account", TICK_ACCOUNT,
        "--launch", launch.as_str(), "--anchor", "/repo",
    ];
    args.extend(model.map(|found| ["--model", found]).into_iter().flatten());
    let out = run_seat(&args);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "登録 row を積める: {}", stderr_of(&out));
    place
}

/// 口座 [`TICK_ACCOUNT`] の実測行を (窓, モデル別窓の model, 使用率, reset) の列で今の時刻に積む（`fleet usage` と同じ形）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn reopens_rows(place: &TickPlace, rows: &[(vessel::fleet::WindowKind, Option<&str>, u64, Option<&str>)]) {
    use vessel::fleet::{Allowance, Event, EventKind, Measured, SCHEMA};
    let policy = vessel::fleet::store::LockPolicy::embedded().expect("lock の規則を読める");
    let ts = acct_now();
    for (window, model, used_pct, reset) in rows.iter().copied() {
        let allowance = Allowance::Measured(Measured {
            account: TICK_ACCOUNT.to_owned(),
            window,
            model: model.map(str::to_owned),
            endpoint: "oauth-usage".to_owned(),
            used_pct,
            resets_at: reset.map(str::to_owned),
        });
        let event = Event {
            schema: SCHEMA,
            ts: ts.clone(),
            kind: EventKind::AllowanceMeasured,
            run: String::new(),
            bead: String::new(),
            host: "h".to_owned(),
            actor: "machine".to_owned(),
            stage: None,
            seat: None,
            pid: None,
            detail: None,
            allowance: Some(allowance),
            registration: None,
            mark: None,
            account: None,
            cost: None,
            rule: None,
            case: None,
        };
        vessel::fleet::store::append(&place.state, &event, policy).expect("実測行を積める");
    }
}

/// status を全部の行の写しで撃ち、rc 0 の stdout を返す。
fn reopens_status(place: &TickPlace) -> String {
    let rules = status_rules(place, "");
    let out = status_run(place, &["--rules", &rules]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    stdout_of(&out)
}

/// (a) 席の model（Opus）の row で: 5 時間窓 100（R1）+ 7 日窓 40 → R1・5 時間窓 100 + 7 日窓 100（R2）→ R2（遅い方）・(R1 の組) + 別の
/// model（Sonnet）のモデル別窓 100 → R1（数えない）・(R1 の組) + 席の model のモデル別窓 100（R3）→ R3（base では欄が無く RED）。
#[test]
fn seat_tick_reopens_takes_the_later_reset_of_the_limited_windows_of_the_seat_model() {
    use vessel::fleet::WindowKind::{FiveHour, SevenDay, SevenDayModel};
    let base = [(FiveHour, None, 100, Some(REOPENS_R1)), (SevenDay, None, 40, Some(REOPENS_R2))];
    let cases = [
        (vec![], REOPENS_R1, "5 時間窓だけが当たる"),
        (vec![(SevenDay, None, 100, Some(REOPENS_R2))], REOPENS_R2, "7 日窓も当たる＝遅い方"),
        (vec![(SevenDayModel, Some("Sonnet"), 100, Some(REOPENS_R3))], REOPENS_R1, "席の model でない窓は数えない"),
        (vec![(SevenDayModel, Some(REOPENS_MODEL), 100, Some(REOPENS_R3))], REOPENS_R3, "席の model の窓は数える"),
    ];
    for (extra, want, why) in cases {
        let place = reopens_place(Some(REOPENS_MODEL));
        let rows: Vec<_> = base.iter().copied().filter(|row| !extra.iter().any(|found| found.0 == row.0)).chain(extra.iter().copied()).collect();
        reopens_rows(&place, &rows);
        let line = reopens_status(&place);
        assert_eq!(tick_token(&line, "reopens").as_deref(), Some(want), "{why}: {line}");
        assert_eq!((tick_token(&line, "move").as_deref(), tick_token(&line, "grace_left").as_deref()), (Some("-"), Some("-")), "{line}");
    }
}

/// (b) 5 時間窓 99 → `reopens=-`（行の全体）・実測なし → `unmeasured`・reset の無い 100 の行 → `unknown`。
#[test]
fn seat_tick_reopens_says_dash_unmeasured_or_unknown() {
    use vessel::fleet::WindowKind::{FiveHour, SevenDay};
    let place = reopens_place(None);
    assert_eq!(tick_token(&reopens_status(&place), "reopens").as_deref(), Some("unmeasured"), "実測なし");
    reopens_rows(&place, &[(FiveHour, None, 99, Some(REOPENS_R1)), (SevenDay, None, 40, Some(REOPENS_R2))]);
    let want = status_line("-", "-", "no", "on", ("-", "-")).replace(STATUS_TAIL, " reopens=- move=- grace_left=-");
    assert_eq!(reopens_status(&place), want, "当たっていない");
    let place = reopens_place(None);
    reopens_rows(&place, &[(FiveHour, None, 100, None), (SevenDay, None, 40, Some(REOPENS_R2))]);
    assert_eq!(tick_token(&reopens_status(&place), "reopens").as_deref(), Some("unknown"), "当たっているが時刻が無い");
}

/// 移動の置き場で status を偽 tmux の PATH と写しで撃ち、rc 0 の stdout を返す。撃つ前後で event log・群の記録・合図の記録は 1 byte も
/// 変わらず、偽 tmux と偽 client の呼出は 0・lock は残らない（計測の子・tmux・lock・書き込みは 0・§20 形 6）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn pending_status(root: &Path, place: &MovePlace) -> String {
    let log = vessel::fleet::store::events_path(&place.state);
    let record = move_groups_dir(root).join(format!("{MOVE_GROUP}.account"));
    let files = || [&log, &record, &grace_signal(place)].map(|path| fs::read(path).ok());
    let before = files();
    let state = place.state.display().to_string();
    let out = Command::new(bin())
        .args(["seat", "tick", "status", "--state-dir", &state, "--rules", &place.rules])
        .env("PATH", &place.path)
        .output()
        .expect("binary を起動できる");
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(files(), before, "event log・群の記録・合図の記録は動かない");
    assert!(!place.at(TICK_CALLS).exists() && !place.at(TICK_CLIENT).exists(), "tmux と client の呼出 0");
    assert!(!move_groups_dir(root).join("lock").exists(), "lock を取らない");
    stdout_of(&out)
}

/// status の行の `move=` と `grace_left=`。
fn pending_pair(line: &str) -> (Option<String>, Option<String>) {
    (tick_token(line, "move"), tick_token(line, "grace_left"))
}

/// (c) 群の記録が口座 B・同じ移動の合図の at = 今 − 100 → `move=<B> grace_left=` が猶予 − 100 前後・合図の記録なし → `-`・別の移動の
/// 記録 → `-`（合図がまだ届いていない＝猶予が始まっていない・base では欄が無く RED）。
#[test]
fn seat_tick_pending_counts_the_grace_left_after_the_signal() {
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    let before = unix_now();
    grace_signal_put(&place, (MOVE_B, MOVE_TS), before - 100);
    let line = pending_status(&root, &place);
    let spent = unix_now() - before;
    let (moved, left) = pending_pair(&line);
    assert_eq!(moved.as_deref(), Some(MOVE_B), "{line}");
    let want = GRACE_S - 100;
    assert!(left.and_then(|secs| secs.parse::<u64>().ok()).is_some_and(|secs| (want - spent..=want).contains(&secs)), "猶予の残り: {line}");
    fs::remove_file(grace_signal(&place)).ok();
    assert_eq!(pending_pair(&pending_status(&root, &place)), (Some(MOVE_B.to_owned()), Some("-".to_owned())), "合図の記録なし");
    grace_signal_put(&place, (MOVE_A, MOVE_TS), unix_now() - 100);
    assert_eq!(pending_pair(&pending_status(&root, &place)), (Some(MOVE_B.to_owned()), Some("-".to_owned())), "別の移動の記録");
}

/// (c2) 猶予の外の合図 → `0`・猶予の内で合図の at 以後の Busy と最終行 Stop の Idle（応え終えた印）→ `0`・猶予 0 の写しで合図なし → `0`
/// （次の tick の周で `/exit`）。
#[test]
fn seat_tick_pending_is_zero_when_the_exit_is_due() {
    let zero = (Some(MOVE_B.to_owned()), Some("0".to_owned()));
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    move_signal_past(&place);
    assert_eq!(pending_pair(&pending_status(&root, &place)), zero, "猶予切れ");
    let root = tmp();
    let place = saved_place(&root, 100, Some(&[SAVED_BUSY, SAVED_STOP]));
    assert_eq!(pending_pair(&pending_status(&root, &place)), zero, "応え終えた印");
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    grace_put(&place, 0);
    assert_eq!(pending_pair(&pending_status(&root, &place)), zero, "猶予 0");
}

/// (d) 群の無い置き場 → `move=- grace_left=-`（行の全体）・群の記録が無く row が種と同じ → `-` / `-`・形でない記録 →
/// `unreadable` / `unreadable` で rc 0・他の欄は不変。
#[test]
fn seat_tick_pending_is_dash_without_a_move_and_unreadable_on_a_malformed_record() {
    let place = tick_place(true);
    assert_eq!(reopens_status(&place), status_line("-", "-", "no", "on", ("-", "-")), "群の無い置き場");
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    let seed = pending_status(&root, &place);
    assert_eq!(pending_pair(&seed), (Some("-".to_owned()), Some("-".to_owned())), "種 = row: {seed}");
    let body = format!("account={MOVE_B}\nts=yesterday\nreason=move\nprevious={MOVE_A}\n");
    fs::write(move_groups_dir(&root).join(format!("{MOVE_GROUP}.account")), body).ok();
    let line = pending_status(&root, &place);
    assert_eq!(line, seed.replace(" move=- grace_left=-", " move=unreadable grace_left=unreadable"), "形でない記録");
}

/// (e) `seat.move_grace_s` を欠く写し → rc 1・`seat tick status: refused reason=no-rule`・stdout 0 行（base では rc 0 で 6 項目 ＝ RED）。
#[test]
fn seat_tick_pending_missing_grace_row_is_no_rule() {
    let place = tick_place(true);
    let rules = status_rules(&place, "seat.move_grace_s");
    let out = status_run(&place, &["--rules", &rules]);
    assert_eq!((rc_of(&out), stdout_of(&out)), (i32::from(RC_REFUSED), String::new()), "rc 1・stdout 0 行");
    assert_eq!(stderr_of(&out), "seat tick status: refused reason=no-rule\n");
}

// ───── park の区画の席の移動（seat-heartbeat.md §21・契約表の行 z・FR95・接頭辞 `seat_tick_park_`） ─────
//
// §4 の移動の fixture（[`move_place`]・偽 tmux・`--rules` の写し）の host の面を「口座 p（= [`MOVE_A`]・席の row）・q（= [`MOVE_B`]）・
// r（= [`PARK_R`]）と、置き場 [`PARK_ANCHOR`] の区画 Tier9（候補 p, q, r）」に書き換え、写しに R-C9-1（[`PARK_THRESHOLD`]）を足す。
// 口座の実測は event log に置く（鮮度の内側は今・外は [`PARK_STALE_TS`]）。字面は契約から組む。

/// 区画の置き場の席の anchor。
const PARK_ANCHOR: &str = "/lot";
/// 区画の名。
const PARK_GROUP: &str = "Tier9";
/// 区画の 3 つ目の口座（r）。
const PARK_R: &str = "acct-r";
/// 写しの R-C9-1 の値（session 用の閾値）。
const PARK_THRESHOLD: u64 = 80;
/// 鮮度の外の実測の ts（写しの鮮度 300 秒より十分古い）。
const PARK_STALE_TS: &str = "2020-01-01T00:00:00Z";

/// 区画の行の key `heartbeat = "on"`（区画の既定は合図を送らない＝off なので、区画の判定の歯は行の key で on にして黙りの門以降の列を
/// 今のまま測る・設計 §22 形 3）。
const PARK_KEY_ON: &str = "heartbeat = \"on\"\n";

/// 区画の置き場を作る。`threshold` が `None` なら写しに R-C9-1 を足さない。席の row は口座 p・anchor は `anchor`。区画の行は
/// [`PARK_KEY_ON`] を持つ。
fn park_place(root: &Path, anchor: &str, threshold: Option<u64>) -> MovePlace {
    let place = move_place(root, "state", anchor);
    fs::remove_file(judge_stamp(root)).ok();
    let host = format!(
        "schema = 1\n\n[[account]]\nlabel = \"{MOVE_A}\"\n\n[[account]]\nlabel = \"{MOVE_B}\"\n\n[[account]]\nlabel = \"{PARK_R}\"\n\n\
         [[account-group]]\nname = \"{PARK_GROUP}\"\nanchors = [\"{PARK_ANCHOR}\"]\naccounts = [\"{MOVE_A}\", \"{MOVE_B}\", \"{PARK_R}\"]\n{PARK_KEY_ON}"
    );
    fs::write(place.state.join("host.toml"), host).ok();
    if let Some(value) = threshold {
        let rules = fs::read_to_string(&place.rules).unwrap_or_default();
        let row = format!(
            "\n[[rule]]\nid = \"R-C9-1\"\nkind = \"AccountSelection\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n"
        );
        fs::write(&place.rules, format!("{rules}{row}")).ok();
    }
    place
}

/// 口座ごとの鮮度の内側の実測（5 時間窓 = 使用率・7 日窓 1）を今の ts で置く。
fn park_measure(place: &MovePlace, rows: &[(&str, u64)]) {
    rows.iter().for_each(|(label, pct)| acct_measured(&place.state, label, *pct, &acct_now()));
}

/// 席の登録 row を積み直す（`seq` を 0 から離す・口座 p・区画の anchor）。
fn park_register(place: &MovePlace) {
    let (launch, state) = (fixture(&place.tools, "launch-again.txt", "claude\n"), place.state.display().to_string());
    let out = run_seat(&[
        "register", "--state-dir", &state, "--target", TICK_TARGET, "--role", "orchestrator", "--account", MOVE_A, "--launch",
        &launch, "--anchor", PARK_ANCHOR,
    ]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "登録 row を積み直せる: {}", stderr_of(&out));
}

/// 席の最新の登録 row の `seq`（event log の物理順・0 始まり）。
fn park_seq(place: &MovePlace) -> usize {
    let events = vessel::fleet::store::read_all(&place.state).unwrap_or_default();
    events.iter().rposition(|event| event.registration.as_ref().is_some_and(|row| row.target == TICK_TARGET)).unwrap_or(usize::MAX)
}

/// 群用 dir の直下の file 名（無ければ空）。
fn park_groups_files(root: &Path) -> Vec<String> {
    let entries = fs::read_dir(move_groups_dir(root)).map(|found| found.filter_map(Result::ok).collect::<Vec<_>>()).unwrap_or_default();
    entries.iter().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect()
}

/// 区画の周の判定行（`judged=` の語だけを移動の周の行と変える）。
fn park_line(step: &str, consumed: &str, launched: &str, judged: &str) -> String {
    move_line(step, consumed, launched).replace("judged=-", &format!("judged={judged}"))
}

/// 区画の退避の合図の 1 行（群の名 Tier9・移り先 `to`・残りは猶予の値・群の字面と同じ 1 関数の出力）。
fn park_signal(to: &str) -> String {
    grace_line(GRACE_S).replace(&format!("group={MOVE_GROUP} to={MOVE_B}"), &format!("group={PARK_GROUP} to={to}"))
}

/// 窓を shell にする（前面 `bash`・prompt・打刻の最終行は Busy〔いま〕で会話 id は [`WAKE_SID`]）。
fn park_shell(place: &MovePlace) {
    fs::write(place.at(MOVE_FRONT), "bash\n").ok();
    fs::write(place.at(TICK_PANE), PANE_SHELL_PROMPT).ok();
    let seat = seat_dir_of(&place.state, TICK_SEAT);
    fs::write(state_file(&seat), format!("{}\n", stamp_line("busy", "UserPromptSubmit", unix_now() - 10, WAKE_SID))).ok();
}

/// 口座の退役の event を積む。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn park_retire(place: &MovePlace, label: &str) {
    use vessel::fleet::{Event, EventKind, SCHEMA};
    let policy = vessel::fleet::store::LockPolicy::embedded().expect("lock の規則を読める");
    let event = Event {
        schema: SCHEMA,
        ts: acct_now(),
        kind: EventKind::AccountRetired,
        run: String::new(),
        bead: String::new(),
        host: "h".to_owned(),
        actor: EventKind::AccountRetired.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: None,
        allowance: None,
        registration: None,
        mark: None,
        account: Some(label.to_owned()),
        cost: None,
        rule: None,
        case: None,
    };
    vessel::fleet::store::append(&place.state, &event, policy).expect("退役の event を積める");
}

/// 群の判定・承認・断りの event と計測の子が 0 件（区画の手は群の面を触らない）。
fn park_assert_no_group_surface(root: &Path, place: &MovePlace) {
    use vessel::fleet::EventKind;
    let kinds = [EventKind::GroupMoved, EventKind::GroupMoveRefused, EventKind::GroupPressureNotified];
    assert!(kinds.iter().all(|kind| judge_events(place, *kind) == 0), "群の event は 0 件");
    assert_eq!(park_groups_files(root), Vec::<String>::new(), "群用 dir の file は 0 件");
    assert!(!place.at(TICK_CLIENT).exists() && !place.at(JUDGE_ARGS).exists(), "計測の子は起動しない");
}

/// (a) 窓が claude ∧ p の鮮度の内側の記録が閾値以上 ∧ q が閾値未満 → `move=signal`・`judged=park:q`・送った 1 行が `group=Tier9` と
/// `to=q` を持つ退避の合図・合図の記録の鍵が（p, `park.<登録 row の seq>`）・承認 event 0・群用 dir の file 0・lock 0・計測の子 0。
#[test]
fn seat_tick_park_signals_a_pressed_seat_toward_the_lot_account() {
    let root = tmp();
    let place = park_place(&root, PARK_ANCHOR, Some(PARK_THRESHOLD));
    park_measure(&place, &[(MOVE_A, 90), (MOVE_B, 10), (PARK_R, 20)]);
    park_register(&place);
    let seq = park_seq(&place);
    assert!(seq > 0, "seq は 0 でない（鍵が seq を写すことを測る）: {seq}");
    let (injections, before) = (move_injections(&place).len(), unix_now());
    let out = move_run(&place);
    let after = unix_now();
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), park_line("signal", "false", "-", &format!("park:{MOVE_B}")), "判定行 1 行");
    let line = park_signal(MOVE_B);
    let want = [format!("send-keys -t {TICK_TARGET} -l {line}"), format!("send-keys -t {TICK_TARGET} Enter")];
    assert_eq!(move_keys(&place), want, "合図の text 1 回 + Enter 1 回・/exit 0");
    let lines = move_injections(&place);
    assert_eq!(lines.len(), injections + 1, "記録 1 行: {lines:?}");
    let (who, what) = move_who_what(lines.last().map(String::as_str).unwrap_or_default());
    let head = format!("{NAME} group: evacuate group={PARK_GROUP} to={MOVE_B} ");
    assert!(who.as_deref() == Some("seat-tick-move") && what.is_some_and(|what| what.starts_with(&head)), "who と what: {lines:?}");
    let found = fs::read_to_string(grace_signal(&place)).unwrap_or_default();
    let at = found
        .strip_prefix(&format!("to={MOVE_A} ts=park.{seq} at="))
        .and_then(|rest| rest.strip_suffix('\n'))
        .and_then(|at| at.parse::<u64>().ok());
    assert!(at.is_some_and(|at| (before..=after).contains(&at)), "鍵は（p, park.<seq>）・at は撃った周の今: {found:?}");
    park_assert_no_group_surface(&root, &place);
}

/// (b) 2 周目: 同じ登録 row の合図の記録が在る間は、移り先が q のままの fixture でも r に変わる fixture でも退避の合図 0（`move=wait`・
/// 合図の記録は不変・他の手の lock が在っても `group-locked` にならない）。登録 row の `seq` か row の口座が違う記録は別の移動で、再び合図を送る。
#[test]
fn seat_tick_park_waits_within_one_registration_whatever_the_destination() {
    for (q, r, to) in [(10, 20, MOVE_B), (20, 5, PARK_R)] {
        let root = tmp();
        let place = park_place(&root, PARK_ANCHOR, Some(PARK_THRESHOLD));
        park_measure(&place, &[(MOVE_A, 90), (MOVE_B, q), (PARK_R, r)]);
        let seq = park_seq(&place);
        grace_signal_put(&place, (MOVE_A, &format!("park.{seq}")), unix_now() - 100);
        let body = fs::read_to_string(grace_signal(&place)).ok();
        let lock = move_groups_dir(&root).join("lock");
        fs::write(&lock, "pid=1\n").ok();
        move_assert_quiet(&place, &park_line("wait", "-", "-", &format!("park:{to}")));
        assert_eq!(fs::read_to_string(grace_signal(&place)).ok(), body, "{to}: 合図の記録は不変");
        assert_eq!(fs::read_to_string(&lock).ok().as_deref(), Some("pid=1\n"), "{to}: 他の手の lock は触らない");
    }
    for other in [(MOVE_A, "park.999999".to_owned()), (MOVE_B, String::new())] {
        let root = tmp();
        let place = park_place(&root, PARK_ANCHOR, Some(PARK_THRESHOLD));
        park_measure(&place, &[(MOVE_A, 90), (MOVE_B, 10), (PARK_R, 20)]);
        let tag = if other.1.is_empty() { format!("park.{}", park_seq(&place)) } else { other.1.clone() };
        grace_signal_put(&place, (other.0, &tag), unix_now() - 100);
        let out = move_run(&place);
        assert_eq!(stdout_of(&out), park_line("signal", "false", "-", &format!("park:{MOVE_B}")), "{other:?}: 別の移動は再び合図: {}", stderr_of(&out));
    }
}

/// (b2) 合図の記録が猶予の外 → `move=exit`（`/exit` の text 1 回 + Enter 1 回・lock を取らない＝他の手の lock が在っても通り、触らない）。
#[test]
fn seat_tick_park_sends_the_exit_past_the_grace_without_the_lock() {
    let root = tmp();
    let place = park_place(&root, PARK_ANCHOR, Some(PARK_THRESHOLD));
    park_measure(&place, &[(MOVE_A, 90), (MOVE_B, 10), (PARK_R, 20)]);
    let seq = park_seq(&place);
    grace_signal_put(&place, (MOVE_A, &format!("park.{seq}")), unix_now() - GRACE_S - 1);
    let lock = move_groups_dir(&root).join("lock");
    fs::write(&lock, "pid=1\n").ok();
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), park_line("exit", "false", "-", &format!("park:{MOVE_B}")), "stderr={}", stderr_of(&out));
    let want = [format!("send-keys -t {TICK_TARGET} -l /exit"), format!("send-keys -t {TICK_TARGET} Enter")];
    assert_eq!(move_keys(&place), want, "/exit の text 1 回 + Enter 1 回");
    assert_eq!(fs::read_to_string(&lock).ok().as_deref(), Some("pid=1\n"), "他の手の lock は触らない");
}

/// (c) 窓が shell の同じ fixture → q で起こし直す（起動行が q の設定 dir と `--resume <会話 id>` を持つ）・登録の記帳の口座が q・
/// 群用 dir の file 0。
#[test]
fn seat_tick_park_relaunches_a_dead_seat_on_the_destination_carrying_the_conversation() {
    let root = tmp();
    let place = park_place(&root, PARK_ANCHOR, Some(PARK_THRESHOLD));
    park_measure(&place, &[(MOVE_A, 90), (MOVE_B, 10), (PARK_R, 20)]);
    park_shell(&place);
    let out = move_run(&place);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), park_line("launch", "-", "launch-unconfirmed", &format!("park:{MOVE_B}")), "判定行 1 行");
    let texts = wake_texts(&place);
    assert_eq!(texts.len(), 1, "起動行 1 行: {texts:?}");
    let text = texts.first().cloned().unwrap_or_default();
    assert!(text.contains(&wake_account_dir(&place, MOVE_B)) && !text.contains("/exit"), "q の設定 dir を持つ起動行: {text}");
    assert!(text.ends_with(&format!(" --resume {WAKE_SID} {}", wake_first_word())), "会話 id と初手を運ぶ: {text}");
    let rows = acct_rows(&place.state);
    let last = rows.last().map(|row| (row.account.as_str(), row.anchor.as_str(), row.target.as_str()));
    assert_eq!(last, Some((MOVE_B, PARK_ANCHOR, TICK_TARGET)), "登録の記帳は q");
    park_assert_no_group_surface(&root, &place);
}

/// (d) p が閾値未満・記録なし・鮮度の外の 3 fixture → 合図 0・起こし直し 0・`judged=` が `stay` / `unmeasured` / `unmeasured`。
#[test]
fn seat_tick_park_leaves_a_seat_whose_account_is_below_or_unmeasured() {
    for (word, own) in [("stay", Some((10, acct_now()))), ("unmeasured", None), ("unmeasured", Some((96, PARK_STALE_TS.to_owned())))] {
        let root = tmp();
        let place = park_place(&root, PARK_ANCHOR, Some(PARK_THRESHOLD));
        if let Some((pct, ts)) = own {
            acct_measured(&place.state, MOVE_A, pct, &ts);
        }
        park_measure(&place, &[(MOVE_B, 10), (PARK_R, 20)]);
        move_assert_quiet(&place, &judge_recent(word));
        assert!(!grace_signal(&place).exists(), "{word}: 合図の記録は書かれない");
        park_assert_no_group_surface(&root, &place);
    }
}

/// (e) 窓が shell で判定が移り先を返さない fixture（閾値未満・記録なし・q と r が無い）→ p で起こし直す（`judged=` は判定の語）。
#[test]
fn seat_tick_park_relaunches_on_the_row_account_when_no_destination() {
    let cases = [("stay", vec![(MOVE_A, 10), (MOVE_B, 10)]), ("unmeasured", vec![(MOVE_B, 10)]), ("park-no-candidate", vec![(MOVE_A, 90)])];
    for (word, rows) in cases {
        let root = tmp();
        let place = park_place(&root, PARK_ANCHOR, Some(PARK_THRESHOLD));
        park_measure(&place, &rows);
        park_shell(&place);
        let out = move_run(&place);
        assert_eq!(stdout_of(&out), park_line("launch", "-", "launch-unconfirmed", word), "{word}: stderr={}", stderr_of(&out));
        let text = wake_texts(&place).first().cloned().unwrap_or_default();
        assert!(text.contains(&wake_account_dir(&place, MOVE_A)), "{word}: p の設定 dir で起こす: {text}");
        let last = acct_rows(&place.state).last().map(|row| row.account.clone());
        assert_eq!(last.as_deref(), Some(MOVE_A), "{word}: 登録の記帳は p");
    }
}

/// (f) q と r が閾値以上か鮮度の外 → 移らず `judged=park-no-candidate`・0 key。退役中の q と鮮度の外の q は候補に数えず、残りの
/// 候補 r に移る。
#[test]
fn seat_tick_park_counts_only_fresh_unretired_candidates() {
    for rows in [[(MOVE_B, 90, false), (PARK_R, 95, false)], [(MOVE_B, 90, false), (PARK_R, 10, true)]] {
        let root = tmp();
        let place = park_place(&root, PARK_ANCHOR, Some(PARK_THRESHOLD));
        park_measure(&place, &[(MOVE_A, 90)]);
        for (label, pct, old) in rows {
            let ts = if old { PARK_STALE_TS.to_owned() } else { acct_now() };
            acct_measured(&place.state, label, pct, &ts);
        }
        move_assert_quiet(&place, &judge_recent("park-no-candidate"));
    }
    let retired = tmp();
    let place = park_place(&retired, PARK_ANCHOR, Some(PARK_THRESHOLD));
    park_measure(&place, &[(MOVE_A, 90), (MOVE_B, 10), (PARK_R, 30)]);
    park_retire(&place, MOVE_B);
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), park_line("signal", "false", "-", &format!("park:{PARK_R}")), "退役中の q は数えない: {}", stderr_of(&out));
    let stale = tmp();
    let place = park_place(&stale, PARK_ANCHOR, Some(PARK_THRESHOLD));
    park_measure(&place, &[(MOVE_A, 90), (PARK_R, 30)]);
    acct_measured(&place.state, MOVE_B, 10, PARK_STALE_TS);
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), park_line("signal", "false", "-", &format!("park:{PARK_R}")), "鮮度の外の q は数えない: {}", stderr_of(&out));
}

/// (g) R-C9-1 の無い rules の写し → `judged=error:no-rule`・列は今のまま進む（黙りの門）・0 key。
#[test]
fn seat_tick_park_without_the_threshold_row_is_no_rule_and_the_list_goes_on() {
    let root = tmp();
    let place = park_place(&root, PARK_ANCHOR, None);
    park_measure(&place, &[(MOVE_A, 96), (MOVE_B, 10)]);
    move_assert_quiet(&place, &judge_recent("error:no-rule"));
}

/// (h) 群にも区画にも属さない席の判定行が今と同じ字（`judged=-`）・区画の口座が逼迫でも合図 0。
#[test]
fn seat_tick_park_outside_the_lot_and_the_groups_is_unjudged() {
    let root = tmp();
    let place = park_place(&root, "/elsewhere", Some(PARK_THRESHOLD));
    park_measure(&place, &[(MOVE_A, 96), (MOVE_B, 10)]);
    move_assert_quiet(&place, &judge_recent("-"));
}

// ───── heartbeat の実効の値（seat-heartbeat.md §22・契約表の行 aa・FR78・ADR-0092・接頭辞 `seat_heartbeat_mode_`） ─────
//
// §4 の移動の fixture（[`move_place`]）の host の面を置き場の種類 5 形（[`beat_shapes`]）に書き換え、黙った席（最終行 Idle が黙りの
// 閾値の外）へ `seat tick` と `seat tick status` を撃つ。明示の記録は `seat heartbeat on|off` の口で置く。字面は契約から組む。

/// 置き場の種類 5 形: 名・host の面の行・明示の記録が無い周の（実効の値, 決まり方）。席の anchor は [`MOVE_ANCHOR`]。
fn beat_shapes() -> Vec<(&'static str, String, (&'static str, &'static str))> {
    let row = |name: &str, anchor: &str, key: &str| {
        format!("\n[[account-group]]\nname = \"{name}\"\nanchors = [\"{anchor}\"]\naccounts = [\"{MOVE_A}\", \"{MOVE_B}\"]\n{key}")
    };
    vec![
        ("group", row(MOVE_GROUP, MOVE_ANCHOR, ""), ("on", "default")),
        ("group-off", row(MOVE_GROUP, MOVE_ANCHOR, "heartbeat = \"off\"\n"), ("off", "group")),
        ("lot", row(PARK_GROUP, MOVE_ANCHOR, ""), ("off", "default")),
        ("lot-on", row(PARK_GROUP, MOVE_ANCHOR, "heartbeat = \"on\"\n"), ("on", "group")),
        ("outside", row(MOVE_GROUP, MOVE_ANCHOR_TWO, ""), ("on", "default")),
    ]
}

/// 置き場を作る: 移動の fixture の host の面を `host_row`（口座 A / B の宣言に続ける）で書き換え、打刻を黙りの閾値の外の Idle にし、
/// `explicit` が在れば明示の記録を口で置く（口の 1 行も測る）。
fn beat_place(root: &Path, host_row: &str, explicit: Option<&str>) -> MovePlace {
    let place = move_place(root, "state", MOVE_ANCHOR);
    let host = format!("schema = 1\n\n[[account]]\nlabel = \"{MOVE_A}\"\n\n[[account]]\nlabel = \"{MOVE_B}\"\n{host_row}");
    fs::write(place.state.join("host.toml"), host).ok();
    let seat = seat_dir_of(&place.state, TICK_SEAT);
    fs::write(state_file(&seat), format!("{}\n", stamp_line("idle", "Stop", unix_now().saturating_sub(TICK_STALE + 60), "sid-move"))).ok();
    if let Some(switch) = explicit {
        heartbeat_assert(&place.state, switch, &heartbeat_line(switch, switch));
    }
    place
}

/// 置き場の host の面の末尾（群か区画の行の最後）に key の行を足す。
fn beat_key(place: &MovePlace, value: &str) {
    let path = place.state.join("host.toml");
    let text = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, format!("{text}heartbeat = \"{value}\"\n")).ok();
}

/// `seat tick status --state-dir S --rules F` の 1 行。
fn beat_status(place: &MovePlace) -> String {
    let state = place.state.display().to_string();
    stdout_of(&run_seat(&["tick", "status", "--state-dir", &state, "--rules", &place.rules]))
}

/// status の `heartbeat=` / `heartbeat_by=` が `want` で、tick の 1 周が on なら合図を注入・off なら `heartbeat-off` で 1 key も送らない。
fn beat_assert(place: &MovePlace, want: (&str, &str), label: &str) {
    let status = beat_status(place);
    let got = (tick_token(&status, "heartbeat"), tick_token(&status, "heartbeat_by"));
    assert_eq!((got.0.as_deref(), got.1.as_deref()), (Some(want.0), Some(want.1)), "{label}: status: {status}");
    let ladder = place.state.join("seat").join(TICK_SEAT).join("pointer-ladder");
    let (keys, record) = (move_keys(place).len(), fs::read(&ladder).ok());
    let out = move_run(place);
    let line = stdout_of(&out);
    if want.0 == "on" {
        assert!(line.starts_with("decision=inject "), "{label}: on は合図を送る: {line} stderr={}", stderr_of(&out));
    } else {
        assert!(line.starts_with("decision=noop ") && line.contains(" reason=heartbeat-off "), "{label}: off は送らない: {line}");
        assert_eq!(move_keys(place).len(), keys, "{label}: 0 key");
        assert_eq!(fs::read(&ladder).ok(), record, "{label}: 梯子の記録は動かない");
    }
}

/// (a-1) 明示 on は置き場の種類に依らず on・explicit（区画の既定 off も群の key の off も上書きする）。
#[test]
fn seat_heartbeat_mode_explicit_on_wins_over_every_shape() {
    for (name, row, _) in beat_shapes() {
        let root = tmp();
        beat_assert(&beat_place(&root, &row, Some("on")), ("on", "explicit"), name);
    }
}

/// (a-2) 明示 off は置き場の種類に依らず off・explicit。
#[test]
fn seat_heartbeat_mode_explicit_off_wins_over_every_shape() {
    for (name, row, _) in beat_shapes() {
        let root = tmp();
        beat_assert(&beat_place(&root, &row, Some("off")), ("off", "explicit"), name);
    }
}

/// (a-3) 明示の記録が無い周は群の表の行の key（group）→ 種類の既定（default）の順: key の無い群は on・default、`"off"` の群は
/// off・group、key の無い区画は off・default、`"on"` の区画は on・group、どの行にも無い置き場は on・default。
#[test]
fn seat_heartbeat_mode_without_a_record_follows_the_table_key_then_the_kind_default() {
    for (name, row, want) in beat_shapes() {
        let root = tmp();
        beat_assert(&beat_place(&root, &row, None), want, name);
    }
}

/// (b) 明示 off と明示 on の両方が在る周・明示 on が dir（在るのに読めない）の周は off・explicit（合図は正の証拠でだけ送る）。
/// 対照: 明示 on だけが読める file の周は on・explicit。
#[test]
fn seat_heartbeat_mode_both_records_and_an_unreadable_on_read_off_explicit() {
    let root = tmp();
    let place = beat_place(&root, "", None);
    let seat = seat_dir_of(&place.state, TICK_SEAT);
    fs::write(seat.join("heartbeat-on"), "ts=2\n").ok();
    beat_assert(&place, ("on", "explicit"), "明示 on だけ");
    fs::write(seat.join("heartbeat-off"), "ts=1\n").ok();
    beat_assert(&place, ("off", "explicit"), "両方在る");
    fs::remove_file(seat.join("heartbeat-off")).ok();
    fs::remove_file(seat.join("heartbeat-on")).ok();
    fs::create_dir_all(seat.join("heartbeat-on")).ok();
    beat_assert(&place, ("off", "explicit"), "明示 on が dir");
}

/// (c-1) 群の行の key が off の席でも、起こし直し（`--resume` と初手の合図）・退避（`/exit` の text 1 回 + Enter 1 回）・群の判定
/// （計測と `judged=stay`）は on の席と同じに撃つ（止めるのは黙りの門以降の合図だけ）。
#[test]
fn seat_heartbeat_mode_group_key_off_still_relaunches_evacuates_and_judges() {
    let root = tmp();
    let place = wake_place(&root, MOVE_ANCHOR, WAKE_SID);
    beat_key(&place, "off");
    let text = wake_assert_launched(&place, MOVE_B, MOVE_ANCHOR);
    assert!(text.ends_with(&format!(" --resume {WAKE_SID} {}", wake_first_word())), "起こし直しは会話 id と初手を運ぶ: {text}");
    let root = tmp();
    let place = move_place(&root, "state", MOVE_ANCHOR);
    move_record(&root, MOVE_B);
    move_signal_past(&place);
    beat_key(&place, "off");
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), move_line("exit", "false", "-"), "退避の判定行: stderr={}", stderr_of(&out));
    assert_eq!(
        move_keys(&place),
        [format!("send-keys -t {TICK_TARGET} -l /exit"), format!("send-keys -t {TICK_TARGET} Enter")],
        "/exit の text 1 回 + Enter 1 回"
    );
    let root = tmp();
    let place = judge_place(&root, MOVE_ANCHOR, [10, 10]);
    beat_key(&place, "off");
    let out = move_run(&place);
    let want = format!("decision=noop target={TICK_SEAT} reason=heartbeat-off pointer=- step=- consumed=- move=- launched=- judged=stay\n");
    assert_eq!(stdout_of(&out), want, "判定は撃ち合図は止まる: stderr={}", stderr_of(&out));
    assert!(judge_calls(&place) > 0 && move_keys(&place).is_empty(), "群の判定は計測し・0 key");
}

/// 区画の行から key を外す（区画の既定の off を測る歯の置き場）。
fn park_default_lot(place: &MovePlace) {
    let path = place.state.join("host.toml");
    let text = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, text.replace(PARK_KEY_ON, "")).ok();
}

/// (c-2) 区画の既定が off の席（key の無い区画の行）でも、区画の移り先の判定は撃ち（退避の合図と `judged=park:q`・窓が shell なら q で
/// 起こし直し）、黙りの門以降の合図だけを止める。
#[test]
fn seat_heartbeat_mode_lot_default_off_still_judges_signals_and_relaunches() {
    let root = tmp();
    let place = park_place(&root, PARK_ANCHOR, Some(PARK_THRESHOLD));
    park_default_lot(&place);
    park_measure(&place, &[(MOVE_A, 90), (MOVE_B, 10), (PARK_R, 20)]);
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), park_line("signal", "false", "-", &format!("park:{MOVE_B}")), "退避の合図と移り先の判定: {}", stderr_of(&out));
    let root = tmp();
    let place = park_place(&root, PARK_ANCHOR, Some(PARK_THRESHOLD));
    park_default_lot(&place);
    park_measure(&place, &[(MOVE_A, 90), (MOVE_B, 10), (PARK_R, 20)]);
    park_shell(&place);
    let out = move_run(&place);
    assert_eq!(stdout_of(&out), park_line("launch", "-", "launch-unconfirmed", &format!("park:{MOVE_B}")), "q で起こし直す: {}", stderr_of(&out));
    let root = tmp();
    let place = park_place(&root, PARK_ANCHOR, Some(PARK_THRESHOLD));
    park_default_lot(&place);
    park_measure(&place, &[(MOVE_A, 10)]);
    let out = move_run(&place);
    let want = "decision=noop target=tk_tk reason=heartbeat-off pointer=- step=- consumed=- move=- launched=- judged=stay\n";
    assert_eq!(stdout_of(&out), want, "移らない周は黙りの門以降が止まる: {}", stderr_of(&out));
}

/// (d) 同じ fixture の群の行の `heartbeat` を値ごとに書き換えて 2 周撃つ: `"on"` の面は管理 tick が `group-unreadable` で止まらず
/// （正例・先に撃つ＝base は `heartbeat` を未知の key で断るのでここで落ちる）、`"maybe"` の面は `noop reason=group-unreadable` で止まる（負例）。
#[test]
fn seat_heartbeat_mode_group_key_value_is_read_by_the_tick_and_a_bad_value_stops_it() {
    let group = |key: &str| {
        format!("\n[[account-group]]\nname = \"{MOVE_GROUP}\"\nanchors = [\"{MOVE_ANCHOR}\"]\naccounts = [\"{MOVE_A}\", \"{MOVE_B}\"]\nheartbeat = \"{key}\"\n")
    };
    let root = tmp();
    let place = beat_place(&root, &group("on"), None);
    let line = stdout_of(&move_run(&place));
    assert!(line.starts_with("decision=inject ") && !line.contains("group-unreadable"), "on の面は止まらず合図を送る: {line}");
    let host = format!("schema = 1\n\n[[account]]\nlabel = \"{MOVE_A}\"\n\n[[account]]\nlabel = \"{MOVE_B}\"\n{}", group("maybe"));
    fs::write(place.state.join("host.toml"), host).ok();
    let out = move_run(&place);
    assert!(stdout_of(&out).contains(" reason=group-unreadable "), "maybe の面は group-unreadable で止まる: {} stderr={}", stdout_of(&out), stderr_of(&out));
}
