// flip-check: moved s2-07l.686
//! 群の族の歯（接頭辞 `pipe_dispatch_group_`・設計 docs/design/carry-prep.md §10 行 n・親 `tests/e2e/pipe/dispatch.rs` の
//! helper を `use super::*` で使う）。

use super::*;

/// (群 0) 群を宣言しない host は、逼迫の口座と席の登録 row が在っても行 0・event 0・偽 client の呼出 0（群の段は 1 語も
/// 出さない＝既存の終端の周のまま）。
#[test]
fn pipe_dispatch_group_zero_groups_sends_and_records_nothing() {
    let place = group_place(&[("a1", 90, 10, 10)], &["a1"], false);
    group_seats(&place.state, ["a1", "a1"]);
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(group_sends(&place.state), Vec::<String>::new(), "行 0（{}）", told(&out));
    assert_eq!(group_notices(&place.state), Vec::new(), "event 0");
    assert_eq!(group_calls(&place.state), 0, "計測も撃たない");
    assert!(!stdout_of(&out).contains("group="), "stdout に群の語は無い（{}）", told(&out));
    clean(&[&place.repo, &place.state]);
}

/// (5 時間窓) 口座 a1 の 5 時間窓 90（行の値 85 以上）の群は、2 つの置き場の席へ各 1 行・event 1 件（口座 a1・窓 5h・
/// 送り先 2）。道具つきの終端の周で撃つ。今の口座は閾値未満の種 a0（[`quiet_seed`]・§20 以後の通知の形）。
#[test]
fn pipe_dispatch_group_five_hour_seed_pressure_reaches_both_anchor_seats() {
    let place = group_place(&[("a0", 10, 10, 10), ("a1", 90, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&place);
    group_seats(&place.state, ["a1", "a1"]);
    let out = group_terminal(&place, "r-group-1");
    let sends = group_sends(&place.state);
    assert_eq!(sends.len(), 2, "置き場ごとに 1 行（{}）: {sends:?}", told(&out));
    assert_both_seats(&sends, &group_payload("a1", "5h", 90, 85));
    let notices = group_notices(&place.state);
    assert_eq!(notices.len(), 1, "event 1 件: {notices:?}");
    let (account, body) = notices.first().cloned().expect("event が在る");
    assert_eq!((account.as_str(), body.group.as_str(), body.window.short()), ("a1", GROUP, "5h"), "{body:?}");
    assert_eq!((body.used, body.cap, body.sent), (90, 85, 2), "値・行の値・送り先の数");
    clean(&[&place.repo, &place.state]);
}

/// (鮮度) 鮮度の内側の実測（いまの ts・5 時間窓 90）を持つ口座は偽 client を 1 回も起こさずに置いた実測で通知し、鮮度の外
/// （古い ts）の口座は 1 回だけ測って測った値で通知する（今の口座は鮮度の内側の種 a0）。
#[test]
fn pipe_dispatch_group_fresh_account_is_not_remeasured_but_stale_is_once() {
    let fresh = group_place(&[("a0", 10, 10, 10), ("a1", 10, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&fresh);
    group_seats(&fresh.state, ["a1", "a1"]);
    put_group_round(&fresh.state, &group_now(), "a1", (90, 10, 10));
    group_terminal(&fresh, "r-group-1");
    assert_eq!(group_calls(&fresh.state), 0, "鮮度の内側は呼出 0");
    assert_both_seats(&group_sends(&fresh.state), &group_payload("a1", "5h", 90, 85));
    clean(&[&fresh.repo, &fresh.state]);
    let stale = group_place(&[("a0", 10, 10, 10), ("a1", 90, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&stale);
    group_seats(&stale.state, ["a1", "a1"]);
    put_group_round(&stale.state, GROUP_STALE_TS, "a1", (10, 10, 10));
    group_terminal(&stale, "r-group-1");
    assert_eq!(group_calls(&stale.state), 1, "鮮度の外は呼出 1");
    assert_both_seats(&group_sends(&stale.state), &group_payload("a1", "5h", 90, 85));
    clean(&[&stale.repo, &stale.state]);
}

/// (2 度目) 同じ実測のまま 2 周目を撃つと、鮮度の内側なので測らず、前回の通知より新しい実測が無いので送らず記さない
/// （行は 2 のまま・event は 1 のまま・呼出は 1 のまま）。
#[test]
fn pipe_dispatch_group_same_measurement_is_not_notified_twice() {
    let place = group_place(&[("a0", 10, 10, 10), ("a1", 90, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&place);
    group_seats(&place.state, ["a1", "a1"]);
    group_terminal(&place, "r-group-1");
    assert_eq!((group_sends(&place.state).len(), group_notices(&place.state).len()), (2, 1), "1 周目は送って記す");
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(group_sends(&place.state).len(), 2, "2 周目は送らない（{}）", told(&out));
    assert_eq!(group_notices(&place.state).len(), 1, "2 周目は記さない");
    assert_eq!(group_calls(&place.state), 1, "2 周目は測らない（鮮度の内側）");
    clean(&[&place.repo, &place.state]);
}

/// (再通知) 通知の後に**同じ値**の新しい実測が event log に入った周は、値が同じでも再び通知する（log の順序で判じる＝
/// 「通知済みなら永久に送らない」と「値が同じなら送らない」の 2 変異を捕まえる）。
#[test]
fn pipe_dispatch_group_new_measurement_with_the_same_value_notifies_again() {
    let place = group_place(&[("a0", 10, 10, 10), ("a1", 90, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&place);
    group_seats(&place.state, ["a1", "a1"]);
    group_terminal(&place, "r-group-1");
    assert_eq!(group_notices(&place.state).len(), 1, "1 周目は記す");
    put_group_round(&place.state, &group_now(), "a1", (90, 10, 10));
    group_terminal(&place, "r-group-2");
    let notices = group_notices(&place.state);
    assert_eq!(notices.len(), 2, "新しい実測の後は再び記す: {notices:?}");
    assert_eq!(notices.last().map(|(_, body)| body.used), Some(90), "値は同じ 90");
    assert_eq!(group_sends(&place.state).len(), 4, "再び 2 つの席へ");
    assert_eq!(group_calls(&place.state), 1, "2 周目は置いた実測を読む（測らない）");
    clean(&[&place.repo, &place.state]);
}

/// (席の口座) 種 a1 は閾値未満で、席の登録 row の口座 a2 だけが逼迫（5 時間窓 90）の周も (群, a2) の 1 行を送る
/// （種 a1 の行は無い＝群の今の口座だけを測る変異を捕まえる）。
#[test]
fn pipe_dispatch_group_seat_account_pressure_is_notified() {
    let place = group_place(&[("a1", 10, 10, 10), ("a2", 90, 10, 10)], &["a1", "a2"], true);
    group_seats(&place.state, ["a2", "a2"]);
    group_terminal(&place, "r-group-1");
    assert_both_seats(&group_sends(&place.state), &group_payload("a2", "5h", 90, 85));
    let accounts: Vec<String> = group_notices(&place.state).into_iter().map(|(account, _)| account).collect();
    assert_eq!(accounts, vec!["a2".to_owned()], "通知は a2 の 1 件だけ");
    clean(&[&place.repo, &place.state]);
}

/// (種) 席が種と違う閾値未満の口座 a2 に居て、種 a1 だけが逼迫する周も種を測って逼迫と判じる（他の歯は席を種と同じ口座に
/// 置くので、測る集合から種を外す変異はここで落ちる）。§20 以後、今の口座（種 a1）の逼迫は §19 の通知でなく移動の周になる:
/// 群の今の口座の記録は a2（候補の次・3 窓とも閾値未満）・§19 の通知は 0・席は既に a2 に居るので退避の合図も起動も 0。
#[test]
fn pipe_dispatch_group_seed_pressure_is_notified_while_the_seats_sit_elsewhere() {
    let place = group_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], true);
    group_seats(&place.state, ["a2", "a2"]);
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a2"), "種 a1 の逼迫で a2 へ移る（{}）", told(&out));
    assert_eq!(group_notices(&place.state), Vec::new(), "§19 の通知は送らない");
    assert_eq!(group_sends(&place.state), Vec::<String>::new(), "席は既に a2＝退避の合図も起動行も無い");
    assert_eq!(group_calls(&place.state), 2, "種と席の口座を 1 回ずつ測る");
    clean(&[&place.repo, &place.state]);
}

/// (窓ごとの行) 7 日窓 90（5 時間窓の行 85 なら越える値）は通知せず、7 日窓 96 は window=7d cap=95 で通知する。
#[test]
fn pipe_dispatch_group_caps_differ_per_window() {
    for (seven, notified) in [(90, false), (96, true)] {
        let place = group_place(&[("a0", 10, 10, 10), ("a1", 10, seven, 10)], &["a0", "a1"], true);
        quiet_seed(&place);
        group_seats(&place.state, ["a1", "a1"]);
        group_terminal(&place, "r-group-1");
        let sends = group_sends(&place.state);
        match notified {
            false => assert_eq!((sends.len(), group_notices(&place.state).len()), (0, 0), "7 日窓 {seven} は通知しない"),
            true => assert_both_seats(&sends, &group_payload("a1", "7d", seven, 95)),
        }
        clean(&[&place.repo, &place.state]);
    }
}

/// (モデル別窓) 5 時間窓と 7 日窓は閾値未満（10）で、モデル別窓（`fleet.group_pressure_model_pct`・実測行の窓は
/// `SevenDayModel`）だけが 96 の口座も通知し、window は model（3 窓それぞれを別々の歯が pin する）。
#[test]
fn pipe_dispatch_group_model_window_alone_is_notified_as_model() {
    let place = group_place(&[("a0", 10, 10, 10), ("a1", 10, 10, 96)], &["a0", "a1"], true);
    quiet_seed(&place);
    group_seats(&place.state, ["a1", "a1"]);
    group_terminal(&place, "r-group-1");
    assert_both_seats(&group_sends(&place.state), &group_payload("a1", "model", 96, 95));
    let windows: Vec<&str> = group_notices(&place.state).iter().map(|(_, body)| body.window.short()).collect();
    assert_eq!(windows, vec!["model"], "event の窓も model");
    clean(&[&place.repo, &place.state]);
}

/// (見る側) 逼迫の群が在る同じ fixture で `dispatch ls` の周は行 0・event 0・偽 client の呼出 0（群の段は起こす側の
/// 1 周だけが持つ）。対: 同じ置き場の起こす側の手動の 1 周は通知する。
#[test]
fn pipe_dispatch_group_ls_round_sends_nothing() {
    let place = group_place(&[("a0", 10, 10, 10), ("a1", 90, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&place);
    group_seats(&place.state, ["a1", "a1"]);
    let listed = group_turn(&place, &["ls"], false);
    assert_eq!(stdout_of(&listed).trim_end(), NONE_LINE, "見る側の行だけ（{}）", told(&listed));
    assert_eq!(group_sends(&place.state), Vec::<String>::new(), "行 0");
    assert_eq!(group_notices(&place.state), Vec::new(), "event 0");
    assert_eq!(group_calls(&place.state), 0, "呼出 0");
    let fired = group_turn(&place, &[], true);
    assert_eq!(fired.status.code(), Some(i32::from(RC_OK)), "{}", told(&fired));
    assert_eq!(group_notices(&place.state).len(), 1, "起こす側の 1 周は通知する（{}）", told(&fired));
    clean(&[&place.repo, &place.state]);
}

/// (道具なし) `--runner` の無い手動の 1 周（台帳を読まない周・列は `no-runner`）でも群の段は走り、逼迫の種に 1 行ずつ +
/// event 1（他の歯は道具つきの終端の周で撃つ＝道具の有無の両側を別々の歯が pin する）。
#[test]
fn pipe_dispatch_group_round_without_runner_still_notifies() {
    let place = group_place(&[("a0", 10, 10, 10), ("a1", 90, 10, 10)], &["a0", "a1"], true);
    quiet_seed(&place);
    group_seats(&place.state, ["a1", "a1"]);
    let out = group_turn(&place, &[], false);
    assert_eq!(stdout_of(&out).trim_end(), "dispatch=unmeasured reason=no-runner", "列は測らない（{}）", told(&out));
    assert_both_seats(&group_sends(&place.state), &group_payload("a1", "5h", 90, 85));
    assert_eq!(group_notices(&place.state).len(), 1, "event 1");
    clean(&[&place.repo, &place.state]);
}

/// (移動) 今の口座（種 a1）の 5 時間窓 90 の群は a2 へ移る: 記録 1（account=a2・previous=a1・reason=move）・承認 event 1
/// （account=a2・detail は宣言の行の逐語）・2 つの席へ退避の合図が 1 行ずつ・同じ target へ a2 の口座の起動行が 1 本ずつ・
/// 登録 row は a2・§19 の通知は 0。
#[test]
fn pipe_dispatch_group_move_pressed_group_records_approves_evacuates_and_relaunches() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let out = group_terminal(&place, "r-group-1");
    let record = fs::read_to_string(groups_dir(&place.state).join(format!("{GROUP}.account"))).unwrap_or_default();
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a2"), "記録は a2（{}）", told(&out));
    assert!(record.contains("\nreason=move\nprevious=a1\n"), "理由と前の口座: {record}");
    let moved = move_events(&place.state, vessel::fleet::EventKind::GroupMoved);
    assert_eq!(moved.len(), 1, "承認 event 1: {moved:?}");
    let (account, words) = moved.first().cloned().unwrap_or_default();
    assert_eq!(account, "a2", "account = 移り先");
    assert!(words.contains("host.toml:") && words.contains("[[account-group]]\nname = \"Tier1\""), "宣言の行の逐語: {words}");
    assert_both_seats(&group_sends(&place.state), &evacuate_payload(GROUP, "a2"));
    for (anchor, target) in GROUP_ANCHORS {
        let lines = launched_lines(&place.state, target);
        assert_eq!(lines.len(), 1, "{target} へ起動行 1 本: {lines:?}");
        assert!(lines.iter().all(|line| line.contains("accounts/a2") && !line.contains("accounts/a1")), "a2 の口座で起こす: {lines:?}");
        assert_eq!(seat_account_of(&place.state, anchor).as_deref(), Some("a2"), "{anchor} の登録 row は a2");
    }
    assert_eq!(group_notices(&place.state), Vec::new(), "移動した周は §19 の通知を送らない");
    clean(&[&place.repo, &place.state]);
}

/// (trust・host-init.md §7 / 行 e・接頭辞 `pipe_dispatch_group_trust_`) 群の起こし直しの周は、移り先の口座 a2 の `.claude.json`
/// （無い＝作る）に群の 2 つの anchor の印が置かれ、各 target へ起動行が送られた瞬間の写し（偽 tmux の `claude-at-<target>`）に
/// その target の anchor の印が既に在る（書き → 送りの順）。base は file が作られない（RED）。
#[test]
fn pipe_dispatch_group_trust_marks_the_new_account_before_the_launch_line() {
    use vessel::fleet::json_tree::parse;
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let out = group_terminal(&place, "r-group-trust");
    let marked = |text: &str, anchor: &str| {
        let tree = parse(text).ok();
        tree.as_ref().and_then(|found| found.get("projects")?.get(anchor)?.get("hasTrustDialogAccepted")?.as_bool()) == Some(true)
    };
    let file = fs::read_to_string(place.state.join("accounts").join("a2").join(".claude.json")).unwrap_or_default();
    for (anchor, target) in GROUP_ANCHORS {
        assert!(marked(&file, anchor), "{anchor} の印が a2 に在る（{}）: {file}", told(&out));
        assert_eq!(launched_lines(&place.state, target).len(), 1, "{target} へ起動行 1 本");
        let at = spy_of(&place.state, "claude-at", target).unwrap_or_default();
        assert!(marked(&at, anchor), "{target} へ送った瞬間に {anchor} の印は既に在る: {at:?}");
    }
    assert!(!place.state.join("accounts").join("a1").join(".claude.json").exists(), "移る前の口座には書かない");
    clean(&[&place.repo, &place.state]);
}

/// (i・account-lifecycle.md §30 形 3 / 4・接頭辞 `pipe_dispatch_group_scope_`) 群の段の起こし直しが各 target へ送る起動行は、
/// 移り先の口座の env の直後・`claude` の前に席の箱の頭（埋め込みの `seat.memory_max_mb` = 32768・`CPUWeight` 無し・unit 名は
/// `<NAME>-<潰した target>-seat-0-<pid>-<seq>` で席ごとに別）を持つ（道具箱は列の fixture が積む・base では頭が無い ＝ RED）。
#[test]
fn pipe_dispatch_group_scope_relaunch_lines_carry_the_seat_box_head() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let out = group_terminal(&place, "r-group-scope");
    let dir = place.state.join("accounts").join("a2").display().to_string();
    let mut units = Vec::new();
    for (_, target) in GROUP_ANCHORS {
        let lines = launched_lines(&place.state, target);
        assert_eq!(lines.len(), 1, "{target} へ起動行 1 本（{}）: {lines:?}", told(&out));
        let line = lines.first().cloned().unwrap_or_default();
        let unit = line.split(' ').find_map(|word| word.strip_prefix("--unit=")).unwrap_or_default().to_owned();
        let head = format!("{}-{}-seat-0-", vessel::name::NAME, target.replace(':', "_"));
        let tail = unit.strip_prefix(&head).and_then(|rest| rest.split_once('-'));
        assert!(tail.is_some_and(|(pid, seq)| pid.parse::<u32>().is_ok() && seq.parse::<u64>().is_ok()), "unit 名の形: {line}");
        let words = format!("systemd-run --user --scope --quiet --collect --unit={unit} -p MemoryMax=32768M -p OOMPolicy=continue --");
        assert!(line.contains(&format!("CLAUDE_CONFIG_DIR={dir} {words} claude ")), "頭は env の直後・claude の前: {line}");
        assert!(!line.contains("CPUWeight"), "席の箱は CPUWeight を持たない: {line}");
        units.push(unit);
    }
    units.dedup();
    assert_eq!(units.len(), GROUP_ANCHORS.len(), "席ごとに別の unit 名: {units:?}");
    clean(&[&place.repo, &place.state]);
}

/// (候補なし) 候補がどれも逼迫（a1 / a2 とも 5 時間窓 90）の群は移らない: 記録 0・断りの event 1・席の pane への行は群の
/// 置き場ごとに断りの 1 行だけ（全 send の行数 2・§19 の通知の行は 0）・起動行 0。
#[test]
fn pipe_dispatch_group_move_without_candidate_refuses_once_per_anchor() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 90, 10, 10)], &["a1", "a2"], "a1");
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP), None, "記録 0（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (0, 1, 0), "断りの event 1");
    let sends = group_sends(&place.state);
    assert_eq!(sends.len(), 2, "席への行は置き場ごとに 1 行だけ: {sends:?}");
    assert_both_seats(&sends, &refused_payload(GROUP));
    assert_eq!(group_notices(&place.state), Vec::new(), "断った周は §19 の通知を送らない");
    assert!(GROUP_ANCHORS.iter().all(|(_, target)| launched_lines(&place.state, target).is_empty()), "起動行 0");
    clean(&[&place.repo, &place.state]);
}

/// (他の群) 群 Tier2（置き場 2 つ目・候補 [a2, a4]）の今の口座（種 a2）は、群 Tier1（置き場 1 つ目・候補 [a1, a2, a3]）の移り先から
/// 外れて Tier1 は a3 へ移る（飛ばす側）。対: Tier2 の候補を [a4, a2]（種 a4）にすると Tier1 は a2 へ移る（移る側）。
#[test]
fn pipe_dispatch_group_move_skips_another_groups_current_account() {
    let accounts = [("a1", 90, 10, 10), ("a2", 10, 10, 10), ("a3", 10, 10, 10), ("a4", 10, 10, 10)];
    for (other, want) in [(["a2", "a4"], "a3"), (["a4", "a2"], "a2")] {
        let place = groups_place(&accounts, &[(GROUP, &[0], &["a1", "a2", "a3"]), ("Tier2", &[1], &other)]);
        group_seats(&place.state, ["a1", other[0]]);
        let out = group_terminal(&place, "r-group-1");
        assert_eq!(move_account(&place.state, GROUP).as_deref(), Some(want), "Tier2 の種 {} で Tier1 は {want} へ（{}）", other[0], told(&out));
        assert_eq!(move_account(&place.state, "Tier2"), None, "Tier2 は移らない");
        clean(&[&place.repo, &place.state]);
    }
}

/// (同じ周の 2 群) Tier1（候補 [a1, a3, a4]）と Tier2（候補 [a2, a3, a4]）が同じ周に逼迫し候補を共有する: 先の Tier1 が a3 へ移り、後の
/// Tier2 は Tier1 の移り先 a3 を飛ばして a4 へ移る（記録 2 の label が異なる＝周の頭の記録だけを読む変異を捕まえる）。
#[test]
fn pipe_dispatch_group_move_two_groups_in_one_round_take_different_targets() {
    let accounts = [("a1", 90, 10, 10), ("a2", 90, 10, 10), ("a3", 10, 10, 10), ("a4", 10, 10, 10)];
    let place = groups_place(&accounts, &[(GROUP, &[0], &["a1", "a3", "a4"]), ("Tier2", &[1], &["a2", "a3", "a4"])]);
    group_seats(&place.state, ["a1", "a2"]);
    let out = group_terminal(&place, "r-group-1");
    let records = (move_account(&place.state, GROUP), move_account(&place.state, "Tier2"));
    assert_eq!(records, (Some("a3".to_owned()), Some("a4".to_owned())), "先の Tier1 は a3・後の Tier2 は a4（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (2, 0, 0), "承認 event 2・断り 0");
    clean(&[&place.repo, &place.state]);
}

/// (保留の続き) 2 つ目の置き場の席が退避の合図の後も shell に戻らない周は、1 つ目だけを起こし 2 つ目に保留の event を 1 件
/// 記す。席が shell に戻った後の 2 周目は判定を繰り返さず（承認 event 1 のまま・退避の合図を重ねない・§19 の通知 0・保留を
/// 重ねない）、保留の席だけを同じ target へ a2 で起こす。
#[test]
fn pipe_dispatch_group_move_second_round_only_relaunches_the_pending_seat() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let ((_, one), (anchor, two)) = (GROUP_ANCHORS[0], GROUP_ANCHORS[1]);
    put_spy(&place.state, "stuck", two, "");
    let out = group_terminal(&place, "r-group-1");
    assert_eq!((launched_lines(&place.state, one).len(), launched_lines(&place.state, two).len()), (1, 0), "{}", told(&out));
    assert_eq!(move_counts(&place.state), (1, 0, 1), "承認 1・保留 1");
    let pending = move_events(&place.state, vessel::fleet::EventKind::GroupMovePending);
    assert!(pending.iter().all(|(account, detail)| account == "a2" && detail.contains(&format!("anchor={anchor}"))), "{pending:?}");
    assert_eq!(seat_account_of(&place.state, anchor).as_deref(), Some("a1"), "保留の席の row は古い口座のまま");
    put_spy(&place.state, "front", two, "bash");
    let again = group_terminal(&place, "r-group-2");
    assert_eq!(launched_lines(&place.state, two).len(), 1, "保留の席を起こす（{}）", told(&again));
    assert_eq!(launched_lines(&place.state, one).len(), 1, "起きた席は起こし直さない");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "判定を繰り返さない・保留を重ねない");
    let evacuations = group_sends(&place.state).iter().filter(|line| line.contains(" group: evacuate ")).count();
    assert_eq!(evacuations, 2, "退避の合図は 1 周目の 2 行だけ");
    assert_eq!(group_notices(&place.state), Vec::new(), "続きの周は §19 の通知を送らない");
    assert_eq!(seat_account_of(&place.state, anchor).as_deref(), Some("a2"), "保留の席の row は a2");
    clean(&[&place.repo, &place.state]);
}

/// (移動を頼む記録) 頼みの在る群は今の口座が鮮度の内側でも計測を 1 回撃ち、判定の後に頼みを群用 dir から履歴へ move する
/// （群用 dir の頼み 0・履歴 1）。同じ fixture の 2 周目は頼みが無いので鮮度の内側の計測 0（move せず残す変異を捕まえる）。
#[test]
fn pipe_dispatch_group_move_request_forces_one_measurement_and_moves_to_history() {
    let place = move_place(&[("a1", 10, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    put_group_round(&place.state, &group_now(), "a1", (10, 10, 10));
    fs::create_dir_all(groups_dir(&place.state)).unwrap_or_default();
    fs::write(groups_dir(&place.state).join(format!("{GROUP}.request")), "ts=t\naccount=a1\nwindow=5h\n").unwrap_or_default();
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(group_calls(&place.state), 1, "鮮度の内側でも頼みの在る周は 1 回測る（{}）", told(&out));
    assert_eq!(group_files(&place.state, "request"), Vec::<String>::new(), "群用 dir の頼みは 0");
    let history = history_names(&place.state);
    assert_eq!(history.iter().filter(|name| name.starts_with(&format!("{GROUP}.request."))).count(), 1, "履歴に 1: {history:?}");
    let again = group_terminal(&place, "r-group-2");
    assert_eq!(group_calls(&place.state), 1, "2 周目は頼みが無い＝鮮度の内側は測らない（{}）", told(&again));
    assert_eq!(move_account(&place.state, GROUP), None, "閾値未満は移らない");
    clean(&[&place.repo, &place.state]);
}

/// (4 手の順) 退避の合図を受けた瞬間に記録と承認 event が既に在り（偽 tmux が合図の Enter で測る）、起動行は合図より後に
/// 届く（時刻の並び）。対: 記録を書けない周（一時 file の path が dir）は承認 event 0・合図 0・起動 0（承認を記録より先に
/// 書く変異を捕まえる）。
#[test]
fn pipe_dispatch_group_move_four_steps_run_in_order() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let out = group_terminal(&place, "r-group-1");
    for (_, target) in GROUP_ANCHORS {
        assert_eq!(spy_of(&place.state, "record-at-evacuate", target).as_deref(), Some("1\n"), "合図の時に記録が在る（{}）", told(&out));
        assert_eq!(spy_of(&place.state, "moved-at-evacuate", target).as_deref(), Some("1\n"), "合図の時に承認 event が在る");
        let at = |name| spy_of(&place.state, name, target).and_then(|found| found.trim().parse::<u128>().ok());
        let (evacuated, launched) = (at("evacuate-at"), at("launch-at"));
        assert!(evacuated.is_some() && launched > evacuated, "{target}: 起動は合図より後: {evacuated:?} {launched:?}");
    }
    clean(&[&place.repo, &place.state]);
    let broken = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    fs::create_dir_all(groups_dir(&broken.state).join(format!("{GROUP}.account.tmp"))).unwrap_or_default();
    let out = group_terminal(&broken, "r-group-1");
    assert_eq!(move_account(&broken.state, GROUP), None, "記録を書けない（{}）", told(&out));
    assert_eq!(move_counts(&broken.state), (0, 0, 0), "承認 event 0");
    assert_eq!(group_sends(&broken.state), Vec::<String>::new(), "合図 0");
    assert!(GROUP_ANCHORS.iter().all(|(_, target)| launched_lines(&broken.state, target).is_empty()), "起動 0");
    clean(&[&broken.repo, &broken.state]);
}

/// (群 0) 群を宣言しない host で逼迫の口座と席が在っても、起こす側の周は群用 dir を作らず記録 0・event 0・送り 0・起動 0・
/// 計測 0 で、便の列の rc（終端の周は rc 0）と `dispatch ls` の外形は今のまま。
#[test]
fn pipe_dispatch_group_move_zero_groups_touch_nothing() {
    let place = group_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], false);
    group_seats(&place.state, ["a1", "a1"]);
    let out = group_terminal(&place, "r-group-1");
    assert!(!groups_dir(&place.state).exists(), "群用 dir を作らない（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (0, 0, 0), "event 0");
    assert_eq!(group_notices(&place.state), Vec::new(), "通知 0");
    assert_eq!(group_sends(&place.state), Vec::<String>::new(), "送り 0");
    assert!(GROUP_ANCHORS.iter().all(|(_, target)| launched_lines(&place.state, target).is_empty()), "起動 0");
    assert_eq!(group_calls(&place.state), 0, "計測 0");
    let listed = group_turn(&place, &["ls"], false);
    assert_eq!(listed.status.code(), Some(i32::from(RC_OK)), "{}", told(&listed));
    assert_eq!(stdout_of(&listed).trim_end(), NONE_LINE, "dispatch ls の外形は今のまま");
    assert!(!groups_dir(&place.state).exists(), "見る側の周も作らない");
    clean(&[&place.repo, &place.state]);
}

/// (書き換え) 1 周目に a1 → a2 へ移った群の a2 が 2 周目に逼迫すると、a1（逼迫の実測）を飛ばして a3 へ移り、前の記録
/// （account=a2）は履歴へ move して群用 dir の記録は 1 file のまま。
#[test]
fn pipe_dispatch_group_move_rewrite_moves_the_previous_record_to_history() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10), ("a3", 10, 10, 10)], &["a1", "a2", "a3"], "a1");
    group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a2"), "1 周目は a2");
    put_group_round(&place.state, &group_now(), "a2", (90, 10, 10));
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a3"), "2 周目は a3（{}）", told(&out));
    assert_eq!(group_files(&place.state, "account"), vec![format!("{GROUP}.account")], "記録は 1 file");
    let history: Vec<String> =
        history_names(&place.state).into_iter().filter(|name| name.starts_with(&format!("{GROUP}.account."))).collect();
    assert_eq!(history.len(), 1, "前の記録が履歴に 1: {history:?}");
    let previous = history.first().map(|name| groups_dir(&place.state).join("history").join(name));
    let text = previous.and_then(|path| fs::read_to_string(path).ok()).unwrap_or_default();
    assert!(text.starts_with("account=a2\n"), "履歴は前の記録（a2）: {text}");
    clean(&[&place.repo, &place.state]);
}

/// (live 便・§27) 候補の順で先の a2 を置き場の live 便が使っている周も a2 へ移る（live 便は移動を妨げない）。対: live 便が
/// 無くても a2 へ移る（live あり / なしで同じ移り先）。
#[test]
fn pipe_dispatch_group_move_also_moves_to_an_account_used_by_a_live_run() {
    for (live, want) in [(true, "a2"), (false, "a2")] {
        let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10), ("a3", 10, 10, 10)], &["a1", "a2", "a3"], "a1");
        if live {
            put_live_run(&place.state, "a2");
        }
        let out = group_terminal(&place, "r-group-1");
        assert_eq!(move_account(&place.state, GROUP).as_deref(), Some(want), "live={live} は {want} へ（{}）", told(&out));
        let inflight = vessel::fleet::replay(&vessel::fleet::store::read_all(&place.state).unwrap_or_default()).inflight_by_account();
        assert_eq!(inflight.get("a2").copied().unwrap_or(0), usize::from(live), "live 便は止めず口座も替えない");
        clean(&[&place.repo, &place.state]);
    }
}

/// (5 時間窓) 先の a2 の 5 時間窓だけが 90（行の値 85 以上）なら a2 を飛ばして a3 へ移る。
#[test]
fn pipe_dispatch_group_move_skips_a_candidate_over_the_five_hour_cap() {
    assert_eq!(move_with_second((90, 10, 10), (10, 10, 10)).as_deref(), Some("a3"), "5 時間窓の逼迫は飛ばす");
    assert_eq!(move_with_second((80, 10, 10), (10, 10, 10)).as_deref(), Some("a2"), "閾値未満なら a2 へ");
}

/// (7 日窓) 先の a2 の 7 日窓だけが 96（行の値 95 以上）なら a2 を飛ばして a3 へ移る。閾値未満の対は a3 も同じ 90 にして
/// 残量の鍵を同点にする（§29 の鍵は宣言の順に戻る＝門だけを測る）。
#[test]
fn pipe_dispatch_group_move_skips_a_candidate_over_the_seven_day_cap() {
    assert_eq!(move_with_second((10, 96, 10), (10, 90, 10)).as_deref(), Some("a3"), "7 日窓の逼迫は飛ばす");
    assert_eq!(move_with_second((10, 90, 10), (10, 90, 10)).as_deref(), Some("a2"), "7 日窓 90 は行の値 95 未満＝a2 へ");
}

/// (モデル別窓) 先の a2 のモデル別窓だけが 96 なら a2 を飛ばして a3 へ移る（閾値未満の対は 7 日窓の歯と同じく同点）。
#[test]
fn pipe_dispatch_group_move_skips_a_candidate_over_the_model_cap() {
    assert_eq!(move_with_second((10, 10, 96), (10, 10, 90)).as_deref(), Some("a3"), "モデル別窓の逼迫は飛ばす");
    assert_eq!(move_with_second((10, 10, 90), (10, 10, 90)).as_deref(), Some("a2"), "モデル別窓 90 は行の値 95 未満＝a2 へ");
}

/// (lock) 群用 dir に lock の file が残る周は群の段が typed に止まり、記録 0・event 0・送り 0・計測 0 で、便の列の rc は
/// 変わらない（終端の周は rc 0）。lock の file は消さない（他の周のもの）。
#[test]
fn pipe_dispatch_group_move_leftover_lock_stops_the_stage_without_touching_the_queue() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    fs::create_dir_all(groups_dir(&place.state)).unwrap_or_default();
    fs::write(groups_dir(&place.state).join("lock"), "pid=1\n").unwrap_or_default();
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP), None, "記録 0（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (0, 0, 0), "event 0");
    assert_eq!(group_notices(&place.state), Vec::new(), "通知 0");
    assert_eq!(group_sends(&place.state), Vec::<String>::new(), "送り 0");
    assert_eq!(group_calls(&place.state), 0, "計測 0");
    assert!(groups_dir(&place.state).join("lock").is_file(), "lock の file は残る");
    clean(&[&place.repo, &place.state]);
}

/// (a) 宣言順で先の a2 の残量が小さく（7 日窓 60）後の a3 の残量が大きい（7 日窓 20）周は a3 へ移る（base は宣言順の a2 ＝ RED）。
#[test]
fn pipe_dispatch_group_reserve_moves_to_the_larger_remainder_not_the_first_declared() {
    let found = move_with_second((10, 60, 10), (10, 20, 10));
    assert_eq!(found.as_deref(), Some("a3"), "残量 80 の a3 が残量 40 の a2 より先");
}

/// (b) 7 日窓が同じ（20）で役割の model〔Fable〕の窓が違う 2 候補は model の残量の大きい方へ（a2 は 70 ＝残量 30・a3 は 30 ＝
/// 残量 70・base は宣言順の a2 ＝ RED）。
#[test]
fn pipe_dispatch_group_reserve_model_window_remainder_decides_between_equal_seven_day() {
    let found = move_with_second((10, 20, 70), (10, 20, 30));
    assert_eq!(found.as_deref(), Some("a3"), "min(80, 30) < min(80, 70)");
}

/// (c) 残量の同点（7 日窓・model とも 50）は 7 日窓の reset の早い a3 へ（base は宣言順の a2 ＝ RED）。
#[test]
fn pipe_dispatch_group_reserve_tie_goes_to_the_earlier_seven_day_reset() {
    let second = [(None, 10, RESERVE_EARLY), (Some(""), 50, RESERVE_LATE), (Some("Fable"), 50, RESERVE_EARLY)];
    let third = [(None, 10, RESERVE_EARLY), (Some(""), 50, RESERVE_EARLY), (Some("Fable"), 50, RESERVE_EARLY)];
    assert_eq!(reserve_move(&second, &third).as_deref(), Some("a3"), "reset の早い方");
}

/// (c) 残量も 7 日窓の reset も同じ 2 候補は宣言順の a2 へ（鍵の最後の段）。
#[test]
fn pipe_dispatch_group_reserve_full_tie_keeps_the_declaration_order() {
    let rows = [(None, 10, RESERVE_EARLY), (Some(""), 50, RESERVE_EARLY), (Some("Fable"), 50, RESERVE_EARLY)];
    assert_eq!(reserve_move(&rows, &rows).as_deref(), Some("a2"), "宣言の順");
}

/// (d) 役割の model〔Fable〕のモデル別窓の行を持たない a2 は残量（7 日窓 0）に依らず最後で、行を持つ a3（残量 50）へ移る（base は
/// 宣言順の a2 ＝ RED）。別の model〔Opus〕の行は役割の model の行の代わりにならない。
#[test]
fn pipe_dispatch_group_reserve_candidate_missing_the_role_model_row_goes_last() {
    let second = [(None, 10, RESERVE_EARLY), (Some(""), 0, RESERVE_EARLY), (Some("Opus"), 0, RESERVE_EARLY)];
    let third = [(None, 10, RESERVE_EARLY), (Some(""), 50, RESERVE_EARLY), (Some("Fable"), 50, RESERVE_EARLY)];
    assert_eq!(reserve_move(&second, &third).as_deref(), Some("a3"), "model の行を欠く候補は最後");
}

/// (e) 5 時間窓が閾値（85）以上の a2 は残量が最大でも門で落ちて a3 へ移る。
#[test]
fn pipe_dispatch_group_reserve_five_hour_over_the_cap_is_gated_out() {
    assert_eq!(move_with_second((90, 0, 0), (10, 50, 50)).as_deref(), Some("a3"), "5 時間窓の逼迫は門で落ちる");
}

/// (e) 5 時間窓が閾値未満の周は 5 時間窓の値が並びを変えない: a2（5h 0・7 日窓 50）より a3（5h 80・7 日窓 10）の残量が大きく
/// a3 へ移る（base は宣言順の a2 ＝ RED・5 時間窓を鍵に入れる変異はここで落ちる）。
#[test]
fn pipe_dispatch_group_reserve_five_hour_under_the_cap_does_not_reorder() {
    assert_eq!(move_with_second((0, 50, 10), (80, 10, 10)).as_deref(), Some("a3"), "5 時間窓は鍵に入らない");
}

/// (f) Tier1（候補 [a1, a3, a4, a5]）と Tier2（候補 [a2, a3, a4, a5]）が同じ周に逼迫し、残量は a5 > a4 > a3（宣言の先頭 a3 は
/// 鍵の最後）: Tier1 は鍵の先頭 a5 へ（自分の予約）・Tier2 は Tier1 の予約 a5 を飛ばして次の a4 へ（base は宣言順の a3 / a4 ＝
/// RED）。
#[test]
fn pipe_dispatch_group_reserve_two_pressed_groups_tier1_takes_the_key_head() {
    let accounts = [("a1", 90, 10, 10), ("a2", 90, 10, 10), ("a3", 10, 60, 10), ("a4", 10, 40, 10), ("a5", 10, 20, 10)];
    let place = groups_place(&accounts, &[(GROUP, &[0], &["a1", "a3", "a4", "a5"]), ("Tier2", &[1], &["a2", "a3", "a4", "a5"])]);
    group_seats(&place.state, ["a1", "a2"]);
    let out = group_terminal(&place, "r-group-1");
    let records = (move_account(&place.state, GROUP), move_account(&place.state, "Tier2"));
    assert_eq!(records, (Some("a5".to_owned()), Some("a4".to_owned())), "Tier1 は鍵の先頭・Tier2 は次（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (2, 0, 0), "承認 event 2・断り 0");
    clean(&[&place.repo, &place.state]);
}

/// (g・§31 (a)) Tier2 だけが逼迫する周は、逼迫でない Tier1 が予約を持たず、Tier2 は Tier1 の鍵の先頭 a3 へ移る（base は Tier1 の
/// 予約 a3 を飛ばして a4 ＝ RED）。(h) 予約は記録しない: Tier1 の記録は動かず、群用 dir の file は Tier2 の記録 1 つだけ（予約の
/// file 0・断りの印 0）で、同じ周の後の便用の除外（群の今の口座・§23）は今の口座だけ。
#[test]
fn pipe_dispatch_group_reserve_tier2_alone_takes_the_tier1_key_head() {
    let place = tier2_alone_place();
    let out = group_terminal(&place, "r-group-1");
    let records = (move_account(&place.state, GROUP), move_account(&place.state, "Tier2"));
    assert_eq!(records, (None, Some("a3".to_owned())), "Tier1 は移らず Tier2 は a3（{}）", told(&out));
    let files: Vec<String> = fs::read_dir(groups_dir(&place.state))
        .map(|entries| entries.filter_map(Result::ok).filter(|entry| entry.path().is_file()))
        .map(|entries| entries.map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    assert_eq!(files, ["Tier2.account"], "予約の file 0・断りの印 0");
    let grouped = vessel::rules::grouped_accounts(&place.state).unwrap_or_default();
    assert_eq!(grouped.into_iter().collect::<Vec<String>>(), ["a1", "a3"], "便用の除外は今の口座だけ");
    clean(&[&place.repo, &place.state]);
}

/// (i・§31 (b)) Tier2 だけが逼迫する周は、Tier1 の今の口座 a1 を 1 回（3 窓）測り、Tier1 だけの候補 a5（実測なし＝鮮度の外）は
/// 測らない（base は Tier1 の予約の導きで a5 も測る ＝ RED）。Tier2 は Tier1 の予約に阻まれず a3 へ移る。
#[test]
fn pipe_dispatch_group_reserve_tier2_alone_measures_only_the_tier1_current() {
    let accounts = [("a1", 10, 10, 10), ("a2", 90, 10, 10), ("a3", 10, 20, 10), ("a5", 10, 30, 10)];
    let place = groups_place(&accounts, &[(GROUP, &[0], &["a1", "a5", "a3"]), ("Tier2", &[1], &["a2", "a3"])]);
    group_seats(&place.state, ["a1", "a2"]);
    let out = group_terminal(&place, "r-group-1");
    let measured = |label: &str| {
        vessel::fleet::store::read_all(&place.state).unwrap_or_default().iter().filter(|event| {
            matches!(&event.allowance, Some(vessel::fleet::Allowance::Measured(row)) if row.account == label)
        }).count()
    };
    assert_eq!((measured("a1"), measured("a5")), (3, 0), "今の口座 a1 を 1 回・候補 a5 は 0（{}）", told(&out));
    assert_eq!(move_account(&place.state, "Tier2").as_deref(), Some("a3"), "Tier2 は a3 へ");
    clean(&[&place.repo, &place.state]);
}

/// (d・§31) 移り先の無い断りの周に印が書かれ、次の Stay の周（今の口座 a1 が閾値未満の実測）に history へ退避される（base は
/// 印が無い ＝ RED）。
#[test]
fn pipe_dispatch_group_reserve_refusal_writes_the_mark_and_stay_moves_it_to_history() {
    let place = refused_place();
    put_group_round(&place.state, &group_now(), "a1", (10, 10, 10));
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(refused_mark(&place.state, GROUP), None, "Stay の周に印は消える（{}）", told(&out));
    assert_eq!(refused_history(&place.state, GROUP), 1, "history に 1 つ");
    clean(&[&place.repo, &place.state]);
}

/// (d2・§31) 同じ実測に 2 度目の断りの周（event を記さない）は印を書き直さない（番兵の字面のまま・base は 1 周目の印が無い ＝ RED）。
#[test]
fn pipe_dispatch_group_reserve_repeated_refusal_does_not_rewrite_the_mark() {
    let place = refused_place();
    fs::write(groups_dir(&place.state).join(format!("{GROUP}.refused")), REFUSED_SENTINEL).unwrap_or_default();
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(move_counts(&place.state), (0, 1, 0), "2 周目は断りの event を記さない（{}）", told(&out));
    assert_eq!(refused_mark(&place.state, GROUP).as_deref(), Some(REFUSED_SENTINEL), "印の字面は不変");
    clean(&[&place.repo, &place.state]);
}

/// (d3・§31) 印が在る状態で移る周（記録を a2 へ書く）は印が history へ退避される（base は印が残る ＝ RED）。
#[test]
fn pipe_dispatch_group_reserve_move_round_moves_the_mark_to_history() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    fs::create_dir_all(groups_dir(&place.state)).unwrap_or_default();
    fs::write(groups_dir(&place.state).join(format!("{GROUP}.refused")), REFUSED_SENTINEL).unwrap_or_default();
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a2"), "記録は移り先（{}）", told(&out));
    assert_eq!(refused_mark(&place.state, GROUP), None, "印は消える");
    assert_eq!(refused_history(&place.state, GROUP), 1, "history に 1 つ");
    clean(&[&place.repo, &place.state]);
}

/// (j) 群の席の役割〔orchestrator〕の `seat.model.orchestrator` 行を欠く `--rules` で逼迫の周を撃つと、集合を空に読み替えず typed
/// に止まる: 移らず記録 0・承認 event 0・断りの event 0・送り 0 で、便の列の rc は不変（終端の周は rc 0・base は宣言順の a2 へ移る
/// ＝ RED）。
#[test]
fn pipe_dispatch_group_reserve_missing_role_row_stops_typed_without_moving() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let rules = fs::read_to_string(&place.rules).unwrap_or_default();
    assert!(rules.contains(GROUP_ROLE_ROW), "写しに役割の行が在る");
    fs::write(&place.rules, rules.replace(GROUP_ROLE_ROW, "")).unwrap_or_default();
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP), None, "記録 0（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (0, 0, 0), "承認 event 0・断りの event 0");
    assert_eq!(group_sends(&place.state), Vec::<String>::new(), "送り 0");
    clean(&[&place.repo, &place.state]);
}

/// (a) 役割 opus の群の今の口座 a1 が Fable の窓 100・5 時間窓 10・7 日窓 50 の周は逼迫でなく移らない（記録 0・承認 0・断り 0・
/// base は Fable の窓で逼迫と読み a2 へ移る ＝ RED）。
#[test]
fn pipe_dispatch_group_model_gate_opus_role_current_with_only_the_fable_window_high_stays() {
    let place = opus_role_place(&[("a1", 10, 50, 100), ("a2", 10, 10, 10)], &["a1", "a2"]);
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP), None, "記録は不変（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (0, 0, 0), "承認 event 0・断りの event 0・保留 0");
    assert_eq!(group_notices(&place.state), Vec::new(), "逼迫の通知も 0");
    clean(&[&place.repo, &place.state]);
}

/// (b) 役割 opus の群の今の口座 a1 が 7 日窓 97 で逼迫の周、Fable の窓 100・7 日窓 40 の候補 a2 へ移る（base は Fable の窓で門に
/// 落ちて移り先なし＝断り ＝ RED）。
#[test]
fn pipe_dispatch_group_model_gate_opus_role_moves_to_a_candidate_with_only_the_fable_window_high() {
    let place = opus_role_place(&[("a1", 10, 97, 10), ("a2", 10, 40, 100)], &["a1", "a2"]);
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a2"), "a2 へ移る（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (1, 0, 0), "承認 event 1・断り 0");
    clean(&[&place.repo, &place.state]);
}

/// (c) 役割 fable（今の写し）の群は今の口座 a1 の Fable の窓 96 で逼迫のまま a2 へ移り、Fable の窓 100 の候補は門で落ちる
/// （役割の model の窓は今のまま数える）。
#[test]
fn pipe_dispatch_group_model_gate_fable_role_still_counts_the_fable_window() {
    let place = move_place(&[("a1", 10, 10, 96), ("a2", 10, 40, 100), ("a3", 10, 10, 10)], &["a1", "a2", "a3"], "a1");
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a3"), "逼迫で移り、Fable 100 の a2 は飛ばす（{}）", told(&out));
    assert_eq!(move_counts(&place.state), (1, 0, 0), "承認 event 1");
    clean(&[&place.repo, &place.state]);
}

/// (続きの周) 記録 a2 ≠ row a1 の群の続きの周に pane が claude の席へ `/exit` の 1 行を送り、その周は起こさず（起動行 0）、
/// 保留の event を重ねない（保留 1 のまま）。既に a2 に居る席へは送らない。
#[test]
fn pipe_dispatch_group_exit_continuation_round_sends_one_exit_to_the_seat_not_at_a_shell() {
    let place = exit_place();
    let ((_, one), (_, two)) = (GROUP_ANCHORS[0], GROUP_ANCHORS[1]);
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(exit_sends(&place.state, two), 1, "保留の席へ /exit 1 行（{}）: {:?}", told(&out), group_sends(&place.state));
    assert_eq!(exit_sends(&place.state, one), 0, "起きた席へは送らない");
    assert_eq!(launched_lines(&place.state, two).len(), 0, "その周は起こさない");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "判定を繰り返さず保留の event を重ねない");
    clean(&[&place.repo, &place.state]);
}

/// (周ごとに 1 回) 同じ席が次の周も shell に戻らなければ、もう 1 行送る（2 周で 2 行・1 周に 2 行は送らない）。
#[test]
fn pipe_dispatch_group_exit_is_sent_once_per_round_while_the_seat_is_not_at_a_shell() {
    let place = exit_place();
    let two = GROUP_ANCHORS[1].1;
    group_terminal(&place, "r-group-2");
    assert_eq!(exit_sends(&place.state, two), 1, "2 周目は 1 行");
    let out = group_terminal(&place, "r-group-3");
    assert_eq!(exit_sends(&place.state, two), 2, "3 周目にもう 1 行（{}）", told(&out));
    assert_eq!(launched_lines(&place.state, two).len(), 0, "起こさない");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "保留を重ねない");
    clean(&[&place.repo, &place.state]);
}

/// (shell に戻った周) `/exit` の後に席が shell に戻った周は送り 0 で、§20 形 6 のとおり同じ target へ a2 の口座の起動行 1 本。
#[test]
fn pipe_dispatch_group_exit_seat_back_at_a_shell_is_launched_without_another_exit() {
    let place = exit_place();
    let (anchor, two) = GROUP_ANCHORS[1];
    group_terminal(&place, "r-group-2");
    assert_eq!(exit_sends(&place.state, two), 1, "続きの周に 1 行");
    put_spy(&place.state, "front", two, "bash");
    let out = group_terminal(&place, "r-group-3");
    assert_eq!(exit_sends(&place.state, two), 1, "shell に戻った周は送らない（{}）", told(&out));
    let lines = launched_lines(&place.state, two);
    assert_eq!(lines.len(), 1, "起動行 1: {lines:?}");
    assert!(lines.iter().all(|line| line.contains("accounts/a2")), "a2 の口座で起こす: {lines:?}");
    assert_eq!(seat_account_of(&place.state, anchor).as_deref(), Some("a2"), "登録 row は a2");
    clean(&[&place.repo, &place.state]);
}

/// (移動の周) 記録を書いた同じ周は退避の合図の 1 行だけで `/exit` は 0（shell に戻らない席にも送らない＝席が作業記憶を残す番を
/// 1 周ぶん持つ）。
#[test]
fn pipe_dispatch_group_exit_move_round_sends_only_the_evacuation() {
    let place = exit_place();
    for (_, target) in GROUP_ANCHORS {
        assert_eq!(exit_sends(&place.state, target), 0, "{target}: 移動の周は /exit 0");
    }
    assert_both_seats(&group_sends(&place.state), &evacuate_payload(GROUP, "a2"));
    clean(&[&place.repo, &place.state]);
}

/// (k) 移動の周の合図は形 4 の字面（残りの秒 = 猶予の値 1800）で席ごとに 1 行・合図の記録が各席の置き場に 3 field
/// （`to=a2 ts=<群の記録の ts> at=<撃った周の今>`）で在る（tick が同じ移動で二重に送らない・base では `ts=` だけの 1 行 ＝ RED）。
#[test]
fn pipe_dispatch_group_grace_move_round_signals_with_the_grace_and_records_it() {
    let before = grace_now();
    let place = grace_place(&[1]);
    let after = grace_now();
    assert_both_seats(&group_sends(&place.state), &evacuate_line(GROUP, "a2", 1800));
    let ts = grace_ts(&place);
    assert!(!ts.is_empty(), "群の記録に ts が在る");
    for (_, target) in GROUP_ANCHORS {
        let found = fs::read_to_string(grace_signal(&place, target)).unwrap_or_default();
        let at = found
            .strip_prefix(&format!("to=a2 ts={ts} at="))
            .and_then(|rest| rest.strip_suffix('\n'))
            .and_then(|at| at.parse::<u64>().ok());
        assert!(at.is_some_and(|at| (before..=after).contains(&at)), "{target} の合図の記録は 3 field・at は撃った周の今: {found:?}");
    }
    clean(&[&place.repo, &place.state]);
}

/// (l) 続きの周は席ごとに合図の記録を読む: 同じ周の 2 席のうち `at` = 今 − 100 の席へは `/exit` 0・`at` = 今 − 1801 の席へは
/// `/exit` 1（base では群の記録の ts〔今〕で一括に保留＝どちらも 0 ＝ RED）。次の周に記録の無い席・別の移動（`to` / `ts` が違う）
/// の席・前の版の形の席へは送らない。写しを猶予 0 に書き換えた周は同じ移動の記録の在る席へ `/exit` 1。どの周も起こさず保留を重ねない。
#[test]
fn pipe_dispatch_group_grace_continuation_round_reads_each_seat_record() {
    let place = grace_place(&[0, 1]);
    let ((_, one), (_, two)) = (GROUP_ANCHORS[0], GROUP_ANCHORS[1]);
    let (ts, now) = (grace_ts(&place), grace_now());
    grace_signal_put(&place, one, ("a2", &ts), now - 100);
    grace_signal_put(&place, two, ("a2", &ts), now - 1801);
    let out = group_terminal(&place, "r-group-2");
    let sends = (exit_sends(&place.state, one), exit_sends(&place.state, two));
    assert_eq!(sends, (0, 1), "猶予の内側の席は 0・越えた席は 1（{}）: {:?}", told(&out), group_sends(&place.state));
    let past = now - 1801;
    let others = [
        (None, "記録の無い席".to_owned()),
        (Some(format!("to=a1 ts={ts} at={past}\n")), "移り先が別の記録".to_owned()),
        (Some(format!("to=a2 ts=2026-09-24T00:00:00Z at={past}\n")), "記録の ts が別の記録".to_owned()),
        (Some(format!("ts={ts}\n")), "前の版の形".to_owned()),
    ];
    for (at, (body, label)) in others.iter().enumerate() {
        match body {
            Some(body) => fs::write(grace_signal(&place, two), body).unwrap_or_default(),
            None => fs::remove_file(grace_signal(&place, two)).unwrap_or_default(),
        }
        let out = group_terminal(&place, &format!("r-group-{}", at + 3));
        let sends = (exit_sends(&place.state, one), exit_sends(&place.state, two));
        assert_eq!(sends, (0, 1), "{label}へは送らない（{}）", told(&out));
    }
    grace_put(&place, 1800, 0);
    let out = group_terminal(&place, "r-group-9");
    assert_eq!(exit_sends(&place.state, one), 1, "猶予 0 の周は記録の在る席へ /exit 1（{}）", told(&out));
    assert_eq!(exit_sends(&place.state, two), 1, "前の版の形の席へは猶予 0 でも送らない");
    assert_eq!(launched_lines(&place.state, one).len() + launched_lines(&place.state, two).len(), 0, "起こさない");
    assert_eq!(move_counts(&place.state), (1, 0, 2), "保留 2 のまま・判定を繰り返さない");
    clean(&[&place.repo, &place.state]);
}

/// 席 `target` の打刻 file の末尾に Stop の Idle を 1 行足す（ts は今・契約の字面から組む＝合図の turn が終わった席）。
fn saved_stop(place: &GroupPlace, target: &str) {
    let path = place.state.join("seat").join(target.replace(':', "_")).join("state.jsonl");
    let line = format!("{{\"schema\":1,\"state\":\"idle\",\"event\":\"Stop\",\"ts\":{},\"sid\":\"\"}}\n", grace_now());
    let body = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, body + &line).unwrap_or_default();
}

/// (h) 続きの周は合図に応え終えた席にだけ猶予を待たずに `/exit`（seat-heartbeat.md §18 形 2）: 同じ周の 2 席に `at` = 今 − 100 の
/// 記録を置き、合図の Busy の後に Stop の Idle を足した席へ `/exit` 1・最終行が Busy の席へ 0（base ではどちらも 0 ＝ RED）。どちらも
/// 起こさず保留を重ねない。
#[test]
fn pipe_dispatch_group_saved_continuation_round_exits_only_the_seat_whose_turn_ended() {
    let place = grace_place(&[0, 1]);
    let ((_, one), (_, two)) = (GROUP_ANCHORS[0], GROUP_ANCHORS[1]);
    let (ts, now) = (grace_ts(&place), grace_now());
    grace_signal_put(&place, one, ("a2", &ts), now - 100);
    grace_signal_put(&place, two, ("a2", &ts), now - 100);
    saved_stop(&place, one);
    let out = group_terminal(&place, "r-group-2");
    let sends = (exit_sends(&place.state, one), exit_sends(&place.state, two));
    assert_eq!(sends, (1, 0), "応え終えた席は 1・走っている席は 0（{}）: {:?}", told(&out), group_sends(&place.state));
    assert_eq!(launched_lines(&place.state, one).len() + launched_lines(&place.state, two).len(), 0, "起こさない");
    assert_eq!(move_counts(&place.state), (1, 0, 2), "保留 2 のまま・判定を繰り返さない");
    clean(&[&place.repo, &place.state]);
}

/// (i) 群の段の合図の記録の `at` は送る前の時刻（seat-heartbeat.md §18 形 7）: 合図の Enter で Busy を打った後に 2 秒眠る席（`slow`）
/// が shell に戻らず（`stuck`）保留になった後、歯が Stop の Idle を足して続きの周を撃つと、その席へ `/exit` 1（猶予 1800 の内側・
/// base では印の関数が無いので 0 ＝ RED・送達の後の時刻で書くと `at` が Busy より後になり 0）。
#[test]
fn pipe_dispatch_group_saved_signal_at_is_taken_before_the_send() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    grace_put(&place, GROUP_GRACE_S, 1800);
    let (_, two) = GROUP_ANCHORS[1];
    put_spy(&place.state, "stuck", two, "");
    put_spy(&place.state, "slow", two, "");
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "移動の周は承認 1・保留 1（{}）", told(&out));
    assert_eq!(exit_sends(&place.state, two), 0, "移動の周は /exit 0");
    saved_stop(&place, two);
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(exit_sends(&place.state, two), 1, "応え終えた席へ /exit 1（{}）: {:?}", told(&out), group_sends(&place.state));
    assert_eq!(launched_lines(&place.state, two).len(), 0, "起こさない");
    clean(&[&place.repo, &place.state]);
}

/// (a) 起こす席の打刻の最終行に会話 id が在る →起動行の末尾が `--resume <sid> '<NAME> seat: relaunch …'`（1 つずつ・a2 の口座の
/// まま）。base は末尾が `--resume <sid>` で初手が無い（RED）。
#[test]
fn pipe_dispatch_group_carry_relaunch_resumes_the_stamped_session() {
    let place = exit_place();
    let line = carry_relaunch(&place, &carry_stamp("idle", "Stop", CARRY_SID));
    assert!(line.ends_with(&format!(" --resume {CARRY_SID} {}", carry_first_word())), "末尾に --resume <sid> と初手: {line}");
    assert_eq!(line.matches("--resume").count(), 1, "--resume は 1 つ: {line}");
    assert_eq!(line.matches("seat: relaunch").count(), 1, "初手は 1 つ: {line}");
    assert!(line.contains("accounts/a2"), "a2 の口座で起こす: {line}");
    clean(&[&place.repo, &place.state]);
}

/// (b) 打刻が無い（空の file）・最終行の sid が会話 id の形でない（前の行の会話 id にも倒れない）→ 起こすが `--resume` は無く、
/// 末尾は初手の 1 語だけ。
#[test]
fn pipe_dispatch_group_carry_without_a_session_id_carries_nothing() {
    let wrong = format!("{}{}", carry_stamp("idle", "Stop", CARRY_SID), carry_stamp("busy", "UserPromptSubmit", "sid-group"));
    for (case, stamps) in [("no-stamp", String::new()), ("not-a-uuid", wrong)] {
        let place = exit_place();
        let line = carry_relaunch(&place, &stamps);
        assert!(line.contains("accounts/a2"), "{case}: 起こす: {line}");
        assert!(!line.contains("--resume") && !line.contains(CARRY_SID), "{case}: --resume 無し: {line}");
        assert!(line.ends_with(&format!(" {}", carry_first_word())), "{case}: 末尾は初手の 1 語: {line}");
        assert_eq!(line.matches("seat: relaunch").count(), 1, "{case}: 初手は 1 つ: {line}");
        clean(&[&place.repo, &place.state]);
    }
}

/// (c) 会話を運んだ周も、登録 row の `launch` は雛形のまま（`--resume` も sid も初手も載らない・起こした row は a2 で launch を持つ）。
#[test]
fn pipe_dispatch_group_carry_leaves_the_row_launch_without_resume() {
    let place = exit_place();
    let (anchor, two) = GROUP_ANCHORS[1];
    let line = carry_relaunch(&place, &carry_stamp("idle", "Stop", CARRY_SID));
    assert!(line.ends_with(&format!(" --resume {CARRY_SID} {}", carry_first_word())), "運んだ周: {line}");
    let rows = carry_rows(&place.state);
    let last = rows.iter().rev().find(|row| row.anchor == anchor && row.target == two);
    assert!(last.is_some_and(|row| row.account == "a2" && !row.launch.is_empty()), "起こした row は a2 で launch を持つ: {rows:?}");
    assert!(rows.iter().all(|row| !row.launch.contains("--resume") && !row.launch.contains(CARRY_SID)), "row の launch は雛形のまま: {rows:?}");
    assert!(rows.iter().all(|row| !row.launch.contains("seat: relaunch")), "row の launch に初手は載らない: {rows:?}");
    clean(&[&place.repo, &place.state]);
}

/// (移動の周) 2 つ目の置き場の orchestrator の row を退役させてから、種 a1 の逼迫した群を移す周: 退避の合図は残る row の
/// target にだけ 1 行・退役した row の target へは送り 0・起動行 0（base では `retire` が使い方の誤りで row が残り合図が届く＝RED）。
#[test]
fn pipe_dispatch_group_retired_row_target_gets_no_evacuation() {
    let place = move_place(&[("a1", 90, 10, 10), ("a2", 10, 10, 10)], &["a1", "a2"], "a1");
    let ((_, one), (anchor, two)) = (GROUP_ANCHORS[0], GROUP_ANCHORS[1]);
    retire_seat(&place.state, two);
    assert_eq!(seat_account_of(&place.state, anchor), None, "退役した row は replay に無い");
    let out = group_terminal(&place, "r-group-1");
    assert_eq!(move_account(&place.state, GROUP).as_deref(), Some("a2"), "群は移る（{}）", told(&out));
    let sends = group_sends(&place.state);
    let evacuate = evacuate_payload(GROUP, "a2");
    assert_eq!(sends.iter().filter(|line| line.contains(&format!("-t {one} ")) && line.ends_with(&evacuate)).count(), 1, "{sends:?}");
    assert_eq!(target_sends(&place.state, two), 0, "退役した row の target へは送らない: {sends:?}");
    assert_eq!(launched_lines(&place.state, two).len(), 0, "起こさない");
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(target_sends(&place.state, two), 0, "続きの周も送らない（{}）", told(&out));
    assert_eq!(exit_sends(&place.state, two), 0, "/exit 0");
    clean(&[&place.repo, &place.state]);
}

/// (続きの周) 移動の周に shell へ戻らず保留になった席の row を退役させると、続きの周は `/exit` も退避の合図も送らず（送り 0）
/// 起こさない（base では row が残り `/exit` が 1 行届く＝RED）。保留の event は重ねない。
#[test]
fn pipe_dispatch_group_retired_pending_seat_gets_no_exit() {
    let place = exit_place();
    let (anchor, two) = GROUP_ANCHORS[1];
    let before = target_sends(&place.state, two);
    assert_eq!(before, 1, "移動の周の退避の合図 1 行だけ: {:?}", group_sends(&place.state));
    retire_seat(&place.state, two);
    assert_eq!(seat_account_of(&place.state, anchor), None, "退役した row は replay に無い");
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(exit_sends(&place.state, two), 0, "/exit 0（{}）: {:?}", told(&out), group_sends(&place.state));
    assert_eq!(target_sends(&place.state, two), before, "送りは増えない");
    assert_eq!(launched_lines(&place.state, two).len(), 0, "起こさない");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "保留の event を重ねない");
    clean(&[&place.repo, &place.state]);
}

/// (形 1) 続きの周の `/exit` は dialog が出て送達を確認できない周（echo が無い）でも inject の記録に 1 行残る（what は `/exit`・
/// who は 1 つ）。送りは 1 行・その周は起こさない・保留の event を重ねない。
#[test]
fn pipe_dispatch_group_exit_dialog_unconfirmed_exit_leaves_one_record() {
    let place = dialog_place(None);
    let two = GROUP_ANCHORS[1].1;
    let out = group_terminal(&place, "r-group-2");
    assert_eq!(exit_sends(&place.state, two), 1, "/exit の送り 1 行（{}）", told(&out));
    let whos = record_whos(&place.state, two, "/exit");
    assert_eq!(whos.len(), 1, "/exit の記録 1 行: {whos:?}");
    assert_eq!(spy_of(&place.state, "screen", two).as_deref(), Some(DIALOG_SCREEN), "dialog が出ている");
    assert_eq!(launched_lines(&place.state, two).len(), 0, "起こさない");
    assert_eq!(move_counts(&place.state), (1, 0, 1), "保留を重ねない");
    clean(&[&place.repo, &place.state]);
}

/// (形 2 / 3) dialog の出た席の次の周は、門の tail が既定の行なので `/exit` を送らず Enter を 1 回だけ送り、記録に
/// `enter:exit-dialog` の 1 行（`/exit` の行と同じ who）。Enter で shell に戻った次の周は同じ target へ a2 の口座の起動行 1 本。
#[test]
fn pipe_dispatch_group_exit_dialog_default_row_gets_one_enter_and_one_record() {
    let place = dialog_place(None);
    let (anchor, two) = GROUP_ANCHORS[1];
    group_terminal(&place, "r-group-2");
    let (exits, enters) = (exit_sends(&place.state, two), enter_sends(&place.state, two));
    let out = group_terminal(&place, "r-group-3");
    assert_eq!(enter_sends(&place.state, two), enters + 1, "Enter 1 回（{}）", told(&out));
    assert_eq!(exit_sends(&place.state, two), exits, "/exit の送り 0");
    let (confirms, exit_whos) = (record_whos(&place.state, two, DIALOG_WHAT), record_whos(&place.state, two, "/exit"));
    assert_eq!(confirms.len(), 1, "{DIALOG_WHAT} の記録 1 行: {confirms:?}");
    assert_eq!(confirms, exit_whos, "/exit と同じ who");
    assert_eq!(launched_lines(&place.state, two).len(), 0, "Enter の周は起こさない");
    let out = group_terminal(&place, "r-group-4");
    let lines = launched_lines(&place.state, two);
    assert_eq!(lines.len(), 1, "shell に戻った周に起動行 1（{}）: {lines:?}", told(&out));
    assert!(lines.iter().all(|line| line.contains("accounts/a2")), "a2 の口座で起こす: {lines:?}");
    assert_eq!(seat_account_of(&place.state, anchor).as_deref(), Some("a2"), "登録 row は a2");
    assert_eq!(record_whos(&place.state, two, DIALOG_WHAT).len(), 1, "起こした周に Enter の記録は増えない");
    clean(&[&place.repo, &place.state]);
}

/// (形 3) dialog が残る周は周ごとに Enter を 1 回ずつ送る（上限を置かない・1 周に 2 回は送らない）。
#[test]
fn pipe_dispatch_group_exit_dialog_remaining_dialog_gets_one_enter_per_round() {
    let place = dialog_place(None);
    let two = GROUP_ANCHORS[1].1;
    group_terminal(&place, "r-group-2");
    let enters = enter_sends(&place.state, two);
    for (at, run) in ["r-group-3", "r-group-4"].into_iter().enumerate() {
        // 前の周の Enter で閉じた dialog を出し直し、前面を席に戻す（dialog が残る席）。
        put_spy(&place.state, "screen", two, DIALOG_SCREEN);
        put_spy(&place.state, "front", two, "claude");
        let out = group_terminal(&place, run);
        assert_eq!(enter_sends(&place.state, two), enters + at + 1, "{run}: 周ごとに Enter 1 回（{}）", told(&out));
    }
    assert_eq!(record_whos(&place.state, two, DIALOG_WHAT).len(), 2, "記録は周ごとに 1 行");
    assert_eq!(exit_sends(&place.state, two), 1, "/exit は続きの 1 周目だけ");
    clean(&[&place.repo, &place.state]);
}

/// (形 2 の fail-closed) 門の tail が既定の行と違う字面（1 字欠け・人の打ちかけ）の席へは 1 key も送らず記録も残さない。
#[test]
fn pipe_dispatch_group_exit_dialog_other_tail_gets_no_key_and_no_record() {
    for screen in ["\u{276f} 1. Exit and stop task\n", "\u{276f} foo\n"] {
        let place = dialog_place(Some(screen));
        let two = GROUP_ANCHORS[1].1;
        let (payloads, enters) = (payload_sends(&place.state, two), enter_sends(&place.state, two));
        let out = group_terminal(&place, "r-group-2");
        assert_eq!(payload_sends(&place.state, two), payloads, "{screen:?}: payload の送り 0（{}）", told(&out));
        assert_eq!(enter_sends(&place.state, two), enters, "{screen:?}: Enter 0");
        assert_eq!(record_whos(&place.state, two, DIALOG_WHAT).len(), 0, "{screen:?}: Enter の記録 0");
        assert_eq!(record_whos(&place.state, two, "/exit").len(), 0, "{screen:?}: /exit の記録 0");
        clean(&[&place.repo, &place.state]);
    }
}

/// (形 4) 移動の周（記録を書いた同じ周）は dialog の既定の行を返す席にも Enter を送らない（退避の合図は門で断られ、`/exit` も
/// Enter も 0・記録 0）。
#[test]
fn pipe_dispatch_group_exit_dialog_move_round_sends_no_enter() {
    let place = dialog_place(Some(DIALOG_SCREEN));
    let two = GROUP_ANCHORS[1].1;
    assert_eq!(enter_sends(&place.state, two), 0, "移動の周の Enter 0");
    assert_eq!(payload_sends(&place.state, two), 0, "移動の周の payload の送り 0");
    assert_eq!(record_whos(&place.state, two, DIALOG_WHAT).len(), 0, "Enter の記録 0");
    clean(&[&place.repo, &place.state]);
}
