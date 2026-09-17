//! 口座の歯（doctor の口座の行 / 口座の退避と立て直し / hook 集合の食い違いの後の終了の手と立て直し・設計
//! docs/design/seat-roles.md §7 / account-autonomy.md §5 / account-lifecycle.md §8・接頭辞 `doctor_accounts_` /
//! `seat_account_` / `seat_tick_`）。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。
//! 歯の本文は `seat.rs` から**挙動不変で移した**もの（`s2-07l.261`）。状態 / 役割 / 登録の歯は `register`・起動 /
//! 復元 / Enter 落ちの歯は `launch`・rules の歯は `rules` へ**挙動不変で移した**（`s2-07l.361`・seat-roles.md §7）。
// flip-check: moved s2-07l.261
// flip-check: moved s2-07l.361

use super::*;

// ─────────────────────────── doctor の口座の行（s2-07l.233・account-autonomy.md §5） ───────────────────────────

/// `<state>/accounts/<label>` を dir で作り、直下に `files`（名前・本文）を置く。
fn account_fixture(place: &RolePlace, label: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = place.state.join("accounts").join(label);
    fs::create_dir_all(&dir).ok();
    for (name, body) in files {
        fs::write(dir.join(name), body).ok();
    }
    dir
}

/// 行の列のうち `account=<label> ` で始まる 1 行（無ければ空）。
fn account_line(lines: &[String], label: &str) -> String {
    let head = format!("account={label} ");
    lines.iter().find(|line| line.starts_with(&head)).cloned().unwrap_or_default()
}

/// `path` から下の全 entry の (path, 本文, mtime)（path の順）。dir の本文は空。
fn tree_facts(path: &Path) -> Vec<(PathBuf, Vec<u8>, Option<SystemTime>)> {
    let mtime = fs::metadata(path).and_then(|found| found.modified()).ok();
    let mut facts = vec![(path.to_path_buf(), fs::read(path).unwrap_or_default(), mtime)];
    let mut children: Vec<PathBuf> =
        fs::read_dir(path).map(|entries| entries.filter_map(|entry| entry.ok().map(|found| found.path())).collect()).unwrap_or_default();
    children.sort();
    for child in children {
        facts.extend(tree_facts(&child));
    }
    facts
}

/// credential だけの dir と設定 dir 全体（`settings.json` を持つ・link で置く周も）で `config=` が分かれ、
/// `agentview=` は `disableAgentView` を読む（歯 (a)・flip の RED＝base は口座の行を出さない）。
#[test]
fn doctor_accounts_config_splits_credential_only_dir_from_full_config_dir() {
    let place = role_place();
    account_fixture(&place, "cred-only", &[(".credentials.json", "{}")]);
    account_fixture(&place, "full-off", &[(".credentials.json", "{}"), ("settings.json", "{\"disableAgentView\": true}")]);
    account_fixture(&place, "full-on", &[(".credentials.json", "{}"), ("settings.json", "{\"disableAgentView\": false}")]);
    account_fixture(&place, "full-bare", &[("settings.json", "{}")]);
    account_fixture(&place, "full-broken", &[("settings.json", "{\"disableAgentView\": tru")]);
    account_fixture(&place, "full-shape", &[("settings.json", "{\"disableAgentView\": \"yes\"}")]);
    let real = place.dir.join("config-home");
    fs::create_dir_all(&real).ok();
    fs::write(real.join(".credentials.json"), "{}").ok();
    fs::write(real.join("settings.json"), "{\"disableAgentView\": true}").ok();
    std::os::unix::fs::symlink(&real, place.state.join("accounts").join("linked")).expect("link を置ける");
    let labels = ["cred-only", "full-off", "full-on", "full-bare", "full-broken", "full-shape", "linked", "gone"];
    let lines = doctor_rows(&place, &account_rules(&labels));
    for (label, rest) in [
        ("cred-only", "dir=present credential=present config=missing agentview=unreadable"),
        ("full-off", "dir=present credential=present config=present agentview=off"),
        ("full-on", "dir=present credential=present config=present agentview=on"),
        ("full-bare", "dir=present credential=missing config=present agentview=on"),
        ("full-broken", "dir=present credential=missing config=present agentview=unreadable"),
        ("full-shape", "dir=present credential=missing config=present agentview=unreadable"),
        ("linked", "dir=present credential=present config=present agentview=off"),
        ("gone", "dir=missing credential=missing config=missing agentview=unreadable"),
    ] {
        assert_eq!(account_line(&lines, label), format!("account={label} {rest} trust=n/a retired=no"), "{lines:?}");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// `.claude.json` の `projects[<anchor>].hasTrustDialogAccepted` で `trust=` が分かれ、file が無い・壊れた・形が違う
/// 周は `unreadable`（`missing` に潰さない）。別の anchor の key は数えない（歯 (b)）。
#[test]
fn doctor_accounts_trust_reads_the_anchor_key_and_never_folds_unreadable_into_missing() {
    let place = role_doctor_place();
    let key = |value: &str| format!("{{\"projects\":{{\"/repo\":{{\"hasTrustDialogAccepted\":{value}}}}}}}");
    let cases = [
        ("t-true", Some(key("true")), "accepted"),
        ("t-absent", Some("{\"projects\":{\"/repo\":{}}}".to_owned()), "missing"),
        ("t-false", Some(key("false")), "missing"),
        ("t-nofile", None, "unreadable"),
        ("t-broken", Some("{\"projects\":".to_owned()), "unreadable"),
        ("t-other", Some("{\"projects\":{\"/elsewhere\":{\"hasTrustDialogAccepted\":true}}}".to_owned()), "missing"),
        ("t-noproj", Some("{}".to_owned()), "missing"),
        ("t-shape", Some("{\"projects\":[]}".to_owned()), "unreadable"),
        ("t-string", Some(key("\"true\"")), "unreadable"),
    ];
    for (label, body, _) in &cases {
        let files: Vec<(&str, &str)> = body.iter().map(|found| (".claude.json", found.as_str())).collect();
        account_fixture(&place, label, &files);
    }
    let labels: Vec<&str> = cases.iter().map(|(label, _, _)| *label).collect();
    let lines = doctor_rows(&place, &account_rules(&labels));
    for (label, _, value) in &cases {
        let want = format!("account={label} dir=present credential=missing config=missing agentview=unreadable trust={value} retired=no");
        assert_eq!(account_line(&lines, label), want, "{lines:?}");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// 登録 row 0 件は `trust=n/a`・anchor が複数なら anchor ごとに `trust=<潰した anchor>:<値>`（辞書順）・event log を
/// 読めない周は `trust=unreadable`（n/a に潰さない）（歯 (c)）。
#[test]
fn doctor_accounts_trust_is_na_without_rows_and_per_anchor_with_many() {
    let place = role_place();
    let body = "{\"projects\":{\"/repo/a\":{\"hasTrustDialogAccepted\":true},\"/repo/b\":{}}}";
    account_fixture(&place, "multi", &[(".claude.json", body)]);
    let rules = account_rules(&["multi"]);
    let head = "account=multi dir=present credential=missing config=missing agentview=unreadable";
    assert_eq!(account_line(&doctor_rows(&place, &rules), "multi"), format!("{head} trust=n/a retired=no"), "登録 row 0 件");
    for (target, role, anchor) in [("mb:x", "planner", "/repo/b"), ("ma:x", "admin", "/repo/a"), ("mc:x", "admin", "/repo/b")] {
        role_stamp(&place, target, Some("sid-t"));
        let out = role_register(&place, target, role, &["--anchor", anchor]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    }
    let lines = doctor_rows(&place, &rules);
    assert_eq!(account_line(&lines, "multi"), format!("{head} trust=_repo_a:accepted trust=_repo_b:missing retired=no"), "{lines:?}");
    fs::write(vessel::fleet::store::events_path(&place.state), "not an event\n").expect("log を壊せる");
    let lines = doctor_rows(&place, &rules);
    // log を読めない周は退役も読めない（`no` に潰さない・C11）。
    assert_eq!(account_line(&lines, "multi"), format!("{head} trust=unreadable retired=unreadable"), "{lines:?}");
    assert!(lines.iter().any(|line| line.starts_with("seats: registered=unreadable")), "{lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// 口座の行は突合の行の直後に label の辞書順で並ぶ（宣言順ではない）（歯 (d)）。
#[test]
fn doctor_accounts_lines_follow_the_seat_lines_in_label_order() {
    let place = role_doctor_place();
    let lines = doctor_rows(&place, &account_rules(&["zeta", "alpha", "mid"]));
    let labels: Vec<&str> =
        lines.iter().filter_map(|line| line.strip_prefix("account=")).filter_map(|rest| rest.split(' ').next()).collect();
    assert_eq!(labels, ["alpha", "mid", "zeta"], "{lines:?}");
    let seats = lines.iter().position(|line| line.starts_with("seats: "));
    let host = lines.iter().position(|line| line == HOST_ABSENT);
    let first = lines.iter().position(|line| line.starts_with("account="));
    assert_eq!(host, seats.map(|at| at + 1), "host の面の行は突合の行の直後: {lines:?}");
    assert_eq!(first, seats.map(|at| at + 2), "口座の行は host の面の行の直後: {lines:?}");
    assert_eq!(lines.len(), 10, "2 行 + 登録 row 2 行 + 突合 1 行 + host の面 1 行 + 口座 3 行 + 導入先 1 行: {lines:?}");
    assert_eq!(lines.last().map(String::as_str), Some(CONSUMER_REPO), "導入先の行は口座の行の後ろ: {lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// `[[account]]` の無い manifest は口座の行 0 本で他の行は不変・`--rules` 無しは host の面（置き場の `host.toml`）の宣言・
/// 宣言なしなら行 0・読めない manifest は 1 行で名乗り rc は変えない・`--rules` の誤りは使い方で断る（歯 (e)・
/// 埋め込みの宣言に依らない形＝`s2-07l.243` の裁定 (B)）。
#[test]
fn doctor_accounts_no_declared_account_adds_no_line_and_keeps_the_rest() {
    let place = role_doctor_place();
    let without = doctor_rows(&place, NO_ACCOUNT_RULES);
    let with = doctor_rows(&place, &account_rules(&["solo"]));
    assert!(!without.iter().any(|line| line.starts_with("account=")), "{without:?}");
    assert_eq!(without.len(), 7, "2 行 + 登録 row 2 行 + 突合 1 行 + host の面 1 行 + 導入先 1 行: {without:?}");
    let rest: Vec<String> = with.iter().filter(|line| !line.starts_with("account=")).cloned().collect();
    assert_eq!(rest, without, "他の行は不変");
    assert_eq!(with.len(), without.len() + 1, "{with:?}");
    let state = place.state.display().to_string();
    let doctor = |args: &[&str]| Command::new(bin()).arg("doctor").args(args).output().expect("binary を起動できる");
    let labels_of = |out: &Output| -> Vec<String> {
        stdout_of(out)
            .lines()
            .filter_map(|line| line.strip_prefix("account=").and_then(|rest| rest.split(' ').next()).map(str::to_owned))
            .collect()
    };
    let bare = doctor(&["--state-dir", &state, "--tmux-socket", &place.socket]);
    assert_eq!(rc_of(&bare), i32::from(RC_OK), "stderr={}", stderr_of(&bare));
    assert_eq!(labels_of(&bare), Vec::<String>::new(), "--rules 無し・host の面も無い周は口座の行 0");
    let bare_out = stdout_of(&bare);
    let bare_tail: Vec<&str> = bare_out.lines().rev().take(2).collect();
    assert_eq!(bare_tail, [CONSUMER_REPO, HOST_ABSENT], "口座の行 0 でも導入先の行は出る: {bare_out}");
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), account_rules(&["zhost", "ahost"])).expect("host の面を書ける");
    let hosted = doctor(&["--state-dir", &state, "--tmux-socket", &place.socket]);
    assert_eq!(rc_of(&hosted), i32::from(RC_OK), "stderr={}", stderr_of(&hosted));
    assert_eq!(labels_of(&hosted), ["ahost", "zhost"], "--rules 無しは host の面の label の辞書順");
    fs::remove_file(place.state.join(vessel::rules::HOST_MANIFEST)).expect("host の面を外せる");
    let absent = place.dir.join("no-such-rules.toml").display().to_string();
    let unreadable = doctor(&["--state-dir", &state, "--tmux-socket", &place.socket, "--rules", &absent]);
    assert_eq!(rc_of(&unreadable), i32::from(RC_OK), "rc は変えない");
    assert_eq!(stdout_of(&unreadable).lines().last(), Some("accounts: manifest=unreadable"), "0 行に潰さない");
    let rules = fixture(&place.dir, "solo.toml", &account_rules(&["solo"]));
    for bad in [
        &["--rules", &rules][..],
        &["--state-dir", &state, "--rules", ""],
        &["--state-dir", &state, "--rules"],
        &["--state-dir", &state, "--rules", &rules, "--rules", &rules],
    ] {
        let out = doctor(bad);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{bad:?} は使い方で断る");
        let stdout = stdout_of(&out);
        assert!(stdout.starts_with("usage: ") && !stdout.contains("account="), "{bad:?}: {stdout}");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// doctor は口座の dir に何も書かない（全 entry の本文・mtime が不変・無い口座の dir を作らない）（歯 (f)）。
#[test]
fn doctor_accounts_writes_nothing_into_the_account_dirs() {
    let place = role_doctor_place();
    let full = [(".credentials.json", "{\"k\":1}"), ("settings.json", "{\"disableAgentView\": true}"), (".claude.json", "{\"projects\":{}}")];
    account_fixture(&place, "w-full", &full);
    account_fixture(&place, "w-empty", &[]);
    let accounts = place.state.join("accounts");
    let before = tree_facts(&accounts);
    assert_eq!(before.len(), 6, "accounts + dir 2 つ + file 3 つ: {before:?}");
    sleep(Duration::from_millis(20));
    let lines = doctor_rows(&place, &account_rules(&["w-full", "w-empty", "w-gone"]));
    assert_eq!(lines.iter().filter(|line| line.starts_with("account=")).count(), 3, "{lines:?}");
    assert_eq!(tree_facts(&accounts), before, "本文・mtime が不変");
    assert!(!accounts.join("w-gone").exists(), "無い口座の dir を作らない");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── 口座の退避と立て直し（account-autonomy.md §5・`s2-07l.211`・接頭辞 `seat_account_`） ───────────────────

/// 立て直さなかったことの 3 面: 起動 script が走っていない・登録 row が増えない・cycle-stamp を打っていない。
fn acct_assert_not_relaunched(place: &AcctPlace, target: &str, case: &str) {
    assert!(!place.dir.join("launched").exists(), "{case}: 起動の雛形は注入されない");
    assert_eq!(acct_rows(&place.state).len(), 1, "{case}: SeatRegistered は増えない");
    assert!(!seat_dir_of(&place.state, target).join("cycle-stamp").exists(), "{case}: cycle-stamp を打たない");
}

/// 前面が shell でない席（prompt を 1 つ描いて `cat` に exec する＝前面 process は `cat`）を立てる。
fn acct_cat_seat(socket: &str, name: &str) -> IsolatedSeat {
    let mut seat = IsolatedSeat {
        socket: socket.to_owned(),
        name: name.to_owned(),
        ready: false,
    };
    let out = tmux(
        socket,
        &["new-session", "-d", "-s", name, "-n", name, "-x", "120", "-y", "40", "sh", "-c", "printf '\u{276f} '; exec cat >/dev/null"],
    );
    seat.ready = out.status.success() && wait_prompt(socket, name);
    seat
}

/// manifest の `[[account]]` に無い口座の row は計測しても行が積まれないので撃たない（毎周の計測に化けさせない）:
/// 他の口座に credential と偽 curl が在っても呼出 0・測れないまま `account=ghost:unmeasured`。
#[test]
fn seat_account_tick_never_measures_an_undeclared_account() {
    let place = acct_place();
    let name = "acctghost";
    let registered = acct_register_as(&place, name, "ghost", ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_credential(&place, ACCT_SEAT);
    acct_fake_curl(&place, 50);
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

    let (out, touched) = acct_tick_probed(&place, name, &pane);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), acct_line("decision=noop reason=busy", &format!("{ST_BUSY} account=ghost:unmeasured plugin=unrecorded"), &place.state));
    assert!(!touched, "注入しない");
    assert_eq!(acct_curl_calls(&place), 0, "宣言外の口座のために計測を撃たない");
    fs::remove_dir_all(&place.dir).ok();
}

/// (1) 登録 row の口座が閾値以上（96 ≥ R-C9-1 の 95）の席は、打刻が Busy でも idle を待たずに退避の合図 1 行を注入する
/// （`kind=externalize origin=account`・判定行に `account=a1:96`・合図の字面は「閾値 95%」・打刻の合図は出ない）。注入の記録は
/// `<state_dir>/inject.jsonl` にも同じ席の行として残る（立て直しの入口 (1) の読み先・出所は終了の手の入口が読む・
/// `s2-07l.307`）。base は口座を見ず `noop reason=busy`（RED）。
#[test]
fn seat_account_tick_signals_externalize_over_threshold_while_busy() {
    let place = acct_place();
    let name = "acctover";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    let registered = acct_register(&place, name, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 96, &acct_now());
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

    let out = acct_tick(&place, name, Some(&pane));

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        acct_line(
            &format!("decision=inject target={name} consumed=false kind=externalize origin=account"),
            &format!("{ST_BUSY} account=a1:96"),
            &place.state
        ),
        "busy でも退避の合図・判定行に出所（口座）と口座と逼迫度"
    );
    let seen = capture(&place.socket, name);
    assert!(
        seen.contains("口座 a1 96%") && seen.contains("閾値 95%") && seen.contains("/ready-compaction"),
        "口座・実測値・閾値・退避 skill が届く: {seen}"
    );
    assert!(!seen.contains("seat heartbeat"), "打刻の合図は出ない: {seen}");
    let log = fs::read_to_string(place.state.join("inject.jsonl")).unwrap_or_default();
    let rows: Vec<&str> = log.lines().collect();
    assert_eq!(rows.len(), 1, "注入 1 件の記録: {log}");
    let row = rows.first().copied().unwrap_or_default();
    assert_eq!(acct_text(row, "who").as_deref(), Some("seat-tick"), "{log}");
    assert_eq!(acct_text(row, "seat").as_deref(), Some(name), "{log}");
    assert!(
        acct_text(row, "what").is_some_and(|what| what.contains(" kind=externalize origin=account ") && what.contains(" account=a1:96")),
        "判定行と同じ字面（出所は kind の直後）: {log}"
    );
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (2) 閾値未満（90 < 95）は注入せず判定行に `account=a1:90` を載せ、以後は既存の順序どおり: Busy なら `busy`（tmux に
/// 触れない）、Idle なら打刻の合図（FR27 の「使用率が閾値未満」が立つ）。実測値 90 は 85 の manifest では閾値以上＝
/// base は退避の合図の側へ倒れて RED（`s2-07l.447`）。
#[test]
fn seat_account_tick_below_threshold_keeps_the_existing_order() {
    let place = acct_place();
    let name = "acctunder";
    let registered = acct_register(&place, name, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 90, &acct_now());
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

    let (out, touched) = acct_tick_probed(&place, name, &pane);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), acct_line("decision=noop reason=busy", &format!("{ST_BUSY} account=a1:90 plugin=unrecorded"), &place.state));
    assert!(!touched, "注入しない周は tmux に触れない");

    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に prompt 付きの session を立てられる");
    write_state(&seat_dir_of(&place.state, name), StateFix::Idle);
    let out = acct_tick(&place, name, Some(&pane));
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        acct_line(
            &format!("decision=inject target={name} consumed=false kind=pointer"),
            &format!("{ST_IDLE} account=a1:90 plugin=unrecorded"),
            &place.state
        ),
        "閾値未満の Idle な席には打刻の合図"
    );
    assert!(capture(&place.socket, name).contains("seat heartbeat"), "打刻の合図が届く");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (3) 実測行が無い口座は計測を 1 回撃ち（credential の無い口座は子 process を起こさず `no_credentials` の
/// Unmeasured 行になる）、測れないまま＝注入も停止もせず、打刻の合図だけを送らない（`account-unmeasured`・
/// tmux に触れない・注入の記録なし）。
#[test]
fn seat_account_tick_unmeasured_account_is_a_typed_noop() {
    use vessel::fleet::{Allowance, UnmeasuredReason};
    let place = acct_place();
    let name = "acctunmeasured";
    let registered = acct_register(&place, name, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    write_state(&seat_dir_of(&place.state, name), StateFix::Idle);
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

    let (out, touched) = acct_tick_probed(&place, name, &pane);

    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(
        stdout_of(&out),
        acct_line("decision=noop reason=account-unmeasured", &format!("{ST_IDLE} account=a1:unmeasured plugin=unrecorded"), &place.state)
    );
    assert!(!touched, "注入しない（tmux に触れない）");
    assert!(!place.state.join("inject.jsonl").exists(), "注入の記録なし");
    let reasons: Vec<UnmeasuredReason> = vessel::fleet::store::read_all(&place.state)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|event| event.allowance)
        .filter_map(|row| match row {
            Allowance::Unmeasured(found) if found.account == ACCT_SEAT => Some(found.reason),
            Allowance::Unmeasured(_) | Allowance::Measured(_) => None,
        })
        .collect();
    assert_eq!(reasons, vec![UnmeasuredReason::NoCredentials], "実測行の無い周は計測を 1 回撃つ: {reasons:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (4) 最新の実測行が `seat.tick_stale_s` より古い口座は、判定の前に計測を 1 回撃ち（偽 curl の呼出 1 回）、新しい
/// 行（50）で判定する（古い行の 95 なら退避の合図になる）。行が新しくなった次の周は撃たない（定期計測はこの 1 形）。
#[test]
fn seat_account_tick_measures_once_when_the_latest_row_is_stale() {
    let place = acct_place();
    let name = "acctstale";
    let registered = acct_register(&place, name, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    let stale = vessel::fleet::cli::format_utc(unix_now().saturating_sub(STALE_S + 600));
    acct_measured(&place.state, ACCT_SEAT, 95, &stale);
    acct_credential(&place, ACCT_SEAT);
    acct_fake_curl(&place, 50);
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);
    let want = acct_line("decision=noop reason=busy", &format!("{ST_BUSY} account=a1:50 plugin=unrecorded"), &place.state);

    for round in [1_u32, 2] {
        let (out, touched) = acct_tick_probed(&place, name, &pane);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{round} 周目: stderr={}", stderr_of(&out));
        assert_eq!(stdout_of(&out), want, "{round} 周目: 計測した新しい行で判定する");
        assert!(!touched, "{round} 周目: 注入しない");
        assert_eq!(acct_curl_calls(&place), 1, "{round} 周目: 計測の子 process は通算 1 回");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// (5) 閾値以上（96 ≥ 95）でも FR29 と同じ除外の周は注入しない: 自席の未 consumed 退避物が在る・cycle lock が live（Busy の
/// 席は以後の順序どおり `busy`・tmux に触れない・注入の記録なし）。除外が無ければ (1) のとおり注入する。
#[test]
fn seat_account_tick_keeps_the_fr29_exclusions_over_threshold() {
    for case in ["wm-unconsumed", "cycle-live"] {
        let place = acct_place();
        let name = "acctexcluded";
        let registered = acct_register(&place, name, ACCT_LAUNCH);
        assert_eq!(rc_of(&registered), i32::from(RC_OK), "{case}: stderr={}", stderr_of(&registered));
        acct_measured(&place.state, ACCT_SEAT, 96, &acct_now());
        let seat = seat_dir_of(&place.state, name);
        write_state(&seat, StateFix::Busy { age_s: 0 });
        if case == "wm-unconsumed" {
            wm_file(&place.wm, "working-memory.parked.md", name);
        } else {
            fs::write(seat.join("cycle.lock"), "{}\n").ok();
        }
        let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

        let (out, touched) = acct_tick_probed(&place, name, &pane);

        assert_eq!(rc_of(&out), i32::from(RC_OK), "{case}: stderr={}", stderr_of(&out));
        assert_eq!(
            stdout_of(&out),
            acct_line("decision=noop reason=busy", &format!("{ST_BUSY} account=a1:96 plugin=unrecorded"), &place.state),
            "{case}: 閾値以上でも注入しない"
        );
        assert!(!touched, "{case}: tmux に触れない");
        assert!(!place.state.join("inject.jsonl").exists(), "{case}: 注入の記録なし");
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// (6) 退避して止まった席（直近の注入が退避の合図 ∧ その後の `Stop` ∧ pane の前面が shell）は、session 用の規則で
/// 選んだ別口座（a1 = 100 は当たっている・a2 = 30）で立て直す: 雛形の穴を a2 の credential dir で埋めた 1 行 →
/// 復元の command 1 行がこの順で注入され、`SeatRegistered`（account=a2・他の項目は既存 row のまま）が 1 件増え、
/// cycle-stamp が打たれる。次の周は更新した口座（a2）を読み、立て直しを繰り返さない。
#[test]
fn seat_account_relaunch_fills_the_template_on_another_account_and_restores() {
    let place = acct_place();
    let name = "acctrelaunch";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    acct_parked(&place, name, &acct_launcher(&place, name), 30);
    assert!(acct_shell_prompt(&place, name, ""), "席の終了後の pane は shell の prompt で終わる");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("account", "a1:100"), ("relaunch", ACCT_SPARE)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    acct_assert_launched_then_restored(&place, name);
    acct_assert_relabelled(&place, name);

    let again = stdout_of(&acct_tick(&place, name, None));
    assert_eq!(tick_token(&again, "account").as_deref(), Some("a2:30"), "次の周は更新した口座を読む: {again}");
    assert_eq!(tick_token(&again, "relaunch"), None, "立て直しを繰り返さない: {again}");
    assert_eq!(acct_rows(&place.state).len(), 2, "登録 row は増えない");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (6) の入口の 3 条件は同時に要る: (a) 退避の合図の記録が無い停止（user の終了・crash）(b) `Stop` が合図より前
/// (c) pane の前面が shell でない（session が終わっていない）——いずれも立て直さず、閾値以上の口座は (1) の退避の
/// 合図の側へ落ちる（起動の雛形は注入されない・登録 row は増えない・cycle-stamp を打たない）。
#[test]
fn seat_account_relaunch_needs_the_signal_the_stop_and_a_shell() {
    for case in ["no-signal", "stop-before-signal", "not-a-shell"] {
        let place = acct_place();
        let name = "acctentry";
        let guard = if case == "not-a-shell" { acct_cat_seat(&place.socket, name) } else { start_seat(&place.socket, name) };
        assert!(guard.ready(), "{case}: 席を立てられる");
        let launch = acct_launcher(&place, name);
        let registered = acct_register(&place, name, &launch);
        assert_eq!(rc_of(&registered), i32::from(RC_OK), "{case}: stderr={}", stderr_of(&registered));
        acct_measured(&place.state, ACCT_SEAT, 100, &acct_now());
        acct_measured(&place.state, ACCT_SPARE, 30, &acct_now());
        match case {
            "no-signal" => acct_stop(&place, name, unix_now()),
            "stop-before-signal" => {
                acct_signal(&place, name);
                acct_stop(&place, name, unix_now().saturating_sub(100));
            }
            _ => {
                acct_signal(&place, name);
                acct_stop(&place, name, unix_now().saturating_add(1));
            }
        }

        let out = acct_tick(&place, name, None);

        let line = format!("{}{}", stdout_of(&out), stderr_of(&out));
        assert_eq!(tick_token(&line, "relaunch"), None, "{case}: 立て直しを評価しない: {line}");
        assert!(line.contains(" account=a1:100"), "{case}: 口座の軸は評価した: {line}");
        acct_assert_not_relaunched(&place, name, case);
        drop(guard);
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// (7) 選べる口座が無い（a1 = 100 は当たっている・a2 = 95 は閾値ちょうど＝以上・他は実測行なし）周は立て直さない:
/// `noop reason=account-no-candidate` と選定の理由（`relaunch=none:over-threshold`）を判定行に残し、注入 0・
/// `SeatRegistered` 0・cycle-stamp なし（次の tick で選び直す）。
#[test]
fn seat_account_relaunch_without_a_candidate_is_a_typed_noop() {
    let place = acct_place();
    let name = "acctnone";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let launch = acct_launcher(&place, name);
    let registered = acct_register(&place, name, &launch);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 100, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, 95, &acct_now());
    let first = acct_signal(&place, name);
    assert_eq!(tick_token(&first, "kind").as_deref(), Some("externalize"), "1 周目は退避の合図: {first}");
    acct_stop(&place, name, unix_now().saturating_add(1));

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "候補なしは断りではない: stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "noop"), ("reason", "account-no-candidate"), ("account", "a1:100"), ("relaunch", "none:over-threshold")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    acct_assert_not_relaunched(&place, name, "no-candidate");
    assert_eq!(acct_injected(&place.state, name).len(), 1, "1 周目の退避の合図だけ（立て直しの注入 0）");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (8) 起動の雛形の穴が無い・2 つ在る row は typed に断る（`relaunch-launch-no-hole` / `relaunch-launch-many-holes`・
/// rc 1）: 注入 0・`SeatRegistered` 0・cycle-stamp なし（lock も打刻も取らない）。
#[test]
fn seat_account_relaunch_refuses_templates_without_exactly_one_hole() {
    for (tail, reason) in [("", "relaunch-launch-no-hole"), (" {account_dir} {account_dir}", "relaunch-launch-many-holes")] {
        let place = acct_place();
        let name = "acctholes";
        let guard = start_seat(&place.socket, name);
        assert!(guard.ready(), "{reason}: 独立 socket に shell の session を立てられる");
        let template = format!("{}{tail}", acct_launcher(&place, name).trim_end_matches(" {account_dir}"));
        let registered = acct_register(&place, name, &template);
        assert_eq!(rc_of(&registered), i32::from(RC_OK), "{reason}: stderr={}", stderr_of(&registered));
        acct_measured(&place.state, ACCT_SEAT, 100, &acct_now());
        acct_measured(&place.state, ACCT_SPARE, 30, &acct_now());
        acct_signal(&place, name);
        acct_stop(&place, name, unix_now().saturating_add(1));

        let out = acct_tick(&place, name, None);

        let line = stderr_of(&out);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{reason}: stdout={}", stdout_of(&out));
        assert_eq!(tick_token(&line, "decision").as_deref(), Some("error"), "{line}");
        assert_eq!(tick_token(&line, "reason").as_deref(), Some(reason), "{line}");
        acct_assert_not_relaunched(&place, name, reason);
        assert_eq!(acct_injected(&place.state, name).len(), 1, "{reason}: 1 周目の退避の合図だけ");
        drop(guard);
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// context 由来の退避の合図を実物の tick で 1 回注入させる（`--capture-file` の pane が cap 以上・打刻 Busy・登録 row の
/// 口座は閾値未満でも送る＝context の軸は口座の軸より前）。判定行が `kind=externalize origin=context` であることを
/// 確かめる。口座の軸の合図（[`acct_signal`]・`origin=account`）との対（`s2-07l.307`）。
pub(super) fn acct_context_signal(place: &AcctPlace, name: &str) {
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", &busy_pane_at(96));
    let line = stdout_of(&acct_tick(place, name, Some(&pane)));
    for (key, want) in [("kind", "externalize"), ("origin", "context")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "context 由来の合図: {key}: {line}");
    }
}

/// (9) 立て直しの入口 (3)「前面が shell」は合図の**出所を問わない**（`s2-07l.307`・極性不変の対・base でも PASS）:
/// context 由来の退避（記録の合図の行が `kind=externalize origin=context`・[`acct_context_signal`]）→ `Stop` の後に
/// user が手で session を終えた席（前面が shell）は、口座由来と同じく別口座で立て直す（`kind=relaunch`・起動 → 復元・
/// 登録 row の口座は a2 に）。終了の手（`/exit`）だけが出所で分かれる。
#[test]
fn seat_account_relaunch_runs_after_a_context_signal_when_the_front_is_a_shell() {
    let place = acct_place();
    let name = "acctctxshell";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let registered = acct_register(&place, name, &acct_launcher(&place, name));
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 100, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, 30, &acct_now());
    acct_context_signal(&place, name);
    acct_stop(&place, name, unix_now().saturating_add(1));
    assert!(acct_shell_prompt(&place, name, ""), "席の終了後の pane は shell の prompt で終わる");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("account", "a1:100"), ("relaunch", ACCT_SPARE)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(tick_token(&line, "origin"), None, "立て直しの判定行に出所は載らない: {line}");
    acct_assert_launched_then_restored(&place, name);
    acct_assert_relabelled(&place, name);
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (10) 立て直しは**自席の登録 row の口座に留まる**（ADR-0028 §2.4・consumer-sync.md §6・`s2-07l.312`）: context 由来の
/// 退避 → `Stop` → 前面が shell の席で、自席の口座 a1 = 90%（閾値 95 未満）・他候補 a2 = 5% → 立て直しは a1（判定行
/// `account=a1:90 relaunch=a1`・雛形の穴は a1 の credential dir・`SeatRegistered` は口座 a1 のまま 1 件増える）。
/// base（逼迫度最小の a2）→ RED。planner / admin が立て直しのたびに別口座へ動いた形（2026-09-15 01:15Z 実測）の対。
/// 実測値 90 は 85 の manifest では閾値以上＝別口座へ移る（`s2-07l.447` の base でも RED）。
#[test]
fn seat_account_relaunch_keeps_the_current_account_below_threshold() {
    let place = acct_place();
    let name = "acctstay";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let registered = acct_register(&place, name, &acct_launcher(&place, name));
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 90, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, 5, &acct_now());
    acct_context_signal(&place, name);
    acct_stop(&place, name, unix_now().saturating_add(1));
    assert!(acct_shell_prompt(&place, name, ""), "席の終了後の pane は shell の prompt で終わる");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("account", "a1:90"), ("relaunch", ACCT_SEAT)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    let own_dir = place.state.join("accounts").join(ACCT_SEAT);
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("{}\n", own_dir.display()),
        "穴は自席の口座（a1）の credential dir で埋まる（a2 ではない）"
    );
    assert_eq!(fs::read_to_string(place.dir.join("seat.log")).unwrap_or_default(), "/rebrief\n", "立ち上がった席が復元を受けた");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "SeatRegistered は 1 件増える: {rows:?}");
    assert_eq!(rows.last().map(|row| row.account.as_str()), Some(ACCT_SEAT), "口座は a1 のまま: {rows:?}");
    assert_eq!(rows.last(), rows.first(), "row は既存 row の写し（口座も含めて同じ）");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (11) (10) の極性の対: 自席の口座が閾値以上（95 ≥ R-C9-1 の 95＝ちょうど）なら留まらず別口座（a2 = 5%）へ（口座由来の退避 →
/// `Stop` → 前面が shell）。判定行は `account=a1:95 relaunch=a2`・穴は a2 の credential dir・row の口座は a2 に
/// （85 の manifest でも 95 は閾値以上＝この歯は base でも PASS・極性不変）。
#[test]
fn seat_account_relaunch_leaves_the_current_account_at_threshold() {
    let place = acct_place();
    let name = "acctleave";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let registered = acct_register(&place, name, &acct_launcher(&place, name));
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 95, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, 5, &acct_now());
    let first = acct_signal(&place, name);
    assert_eq!(tick_token(&first, "kind").as_deref(), Some("externalize"), "閾値ちょうどは退避の合図: {first}");
    acct_stop(&place, name, unix_now().saturating_add(1));
    assert!(acct_shell_prompt(&place, name, ""), "席の終了後の pane は shell の prompt で終わる");

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("account", "a1:95"), ("relaunch", ACCT_SPARE)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    acct_assert_launched_then_restored(&place, name);
    acct_assert_relabelled(&place, name);
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// 85 と 95 を弁別する歯（`s2-07l.447`・rules-manifest.md §13・接頭辞 `seat_threshold_95_`）: 登録 row の口座が 90 の席
/// （85 以上 95 未満）は退避の合図を受けない——Busy の周は `noop reason=busy`・判定行の口座の軸は `account=a1:90` で
/// `kind=` を持たず・tmux に触れず・注入の記録も無い。85 の manifest では 90 は閾値以上＝`kind=externalize origin=account`
/// を注入して RED。
#[test]
fn seat_threshold_95_account_at_90_gets_no_externalize_signal() {
    let place = acct_place();
    let name = "acctninety";
    let registered = acct_register(&place, name, ACCT_LAUNCH);
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 90, &acct_now());
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);

    let (out, touched) = acct_tick_probed(&place, name, &pane);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(line, acct_line("decision=noop reason=busy", &format!("{ST_BUSY} account=a1:90 plugin=unrecorded"), &place.state));
    assert_eq!(tick_token(&line, "kind"), None, "90 は閾値 95 未満＝口座由来の退避の合図は出ない: {line}");
    assert!(!touched, "tmux に触れない");
    assert!(!place.state.join("inject.jsonl").exists(), "注入の記録なし");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────────────── hook 集合の食い違いの後の終了の手と立て直し（consumer-sync.md §6・AC32・`s2-07l.304`・接頭辞 `seat_tick_hook_drift_`） ───────────────────

/// 終了の手を受けた席が写す 1 行の置き場（名に `/exit` を含めない＝送達の目印の出現数を pane の echo で汚さない）。
const DRIFT_LOG: &str = "drift-received.log";

/// pane の前面 process の名（`#{pane_current_command}`・器の入口 (3) と同じ typed な読み）。
fn drift_front(place: &AcctPlace, name: &str) -> String {
    let out = tmux(&place.socket, &["list-panes", "-t", name, "-F", "#{pane_current_command}"]);
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// 前面が `want` になるまで待つ（上限 [`PROMPT_WAIT`]）。
fn drift_wait_front(place: &AcctPlace, name: &str, want: &str) -> bool {
    let deadline = Instant::now().checked_add(PROMPT_WAIT);
    while deadline.is_some_and(|at| Instant::now() < at) {
        if drift_front(place, name) == want {
            return true;
        }
        sleep(Duration::from_millis(100));
    }
    false
}

/// 前面が shell でない偽の席にする（終了の手の歯と同じ形）: `sh -i` の session に prompt `❯ ` を描いて **1 行だけ読む `head`**
/// を走らせる。前面は `head`（入口 (3) は立たない）で、受けた 1 行を [`DRIFT_LOG`] に写して終わり、shell へ戻る
/// （`/exit` を受けた席が終わる形）。前面が `head` になったかを返す。
fn drift_seat_head(place: &AcctPlace, name: &str) -> bool {
    let line = format!("printf '\\342\\235\\257 '; head -n 1 >> '{}'", place.dir.join(DRIFT_LOG).display());
    tmux(&place.socket, &["send-keys", "-t", name, "-l", &line]).status.success()
        && tmux(&place.socket, &["send-keys", "-t", name, "Enter"]).status.success()
        && drift_wait_front(place, name, "head")
}

/// (b) 経路の統合: hook 由来の退避の合図（記録 A ≠ 今 B・口座 a1 = 13 は閾値未満・候補 a2 = 5 はさらに低い）→ 退避 → `Stop` →
/// 前面が shell でない席には次の周に `/exit`（`.307` の入口・`origin=hook` は口座由来と同じ側）→ 席が終わって前面が shell →
/// 次の周に立て直しが**同じ target・同じ口座 a1**（閾値未満なら自席の口座を優先・`.312`）で走り、起動の後に `/rebrief` が
/// 注入される（席の記録は 退避の合図 → `/exit` → 起動 → 復元 の 4 行・登録 row は a1 のまま 1 件増える）。base は軸が無く
/// 合図が出ない（RED）。
#[test]
fn seat_tick_hook_drift_then_exit_and_relaunch_on_the_same_account() {
    let place = acct_place();
    let name = "hookrelaunch";
    let guard = drift_parked(&place, name);

    let second = acct_tick(&place, name, None);
    let line = stdout_of(&second);
    assert_eq!(rc_of(&second), i32::from(RC_OK), "2 周目: stdout={line} stderr={}", stderr_of(&second));
    for (key, want) in [("decision", "inject"), ("kind", "exit"), ("account", "a1:13")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "2 周目は終了の手: {key}: {line}");
    }
    assert_eq!((tick_token(&line, "cycle"), tick_token(&line, "plugin")), (None, None), "/clear の cycle は回さず hook の軸は評価しない: {line}");
    assert_eq!(fs::read_to_string(place.dir.join(DRIFT_LOG)).unwrap_or_default(), "/exit\n", "席が受けた 1 行は /exit");
    assert!(drift_wait_front(&place, name, "sh"), "受けた席は終わって前面が shell へ戻る");
    assert!(acct_shell_prompt(&place, name, ""), "shell の prompt を描ける");

    let third = acct_tick(&place, name, None);
    let line = stdout_of(&third);
    assert_eq!(rc_of(&third), i32::from(RC_OK), "3 周目: stdout={line} stderr={}", stderr_of(&third));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("account", "a1:13"), ("relaunch", ACCT_SEAT)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "3 周目は同じ口座で立て直し: {key}: {line}");
    }
    drift_assert_relaunched_on_own_account(&place, name);
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// hook 由来の退避で止まり、前面が shell でない席の fixture: 登録 row（起動の雛形は偽の session の script）・口座 a1 = 13
/// （閾値未満）・候補 a2 = 5（さらに低い＝逼迫度最小は a2）・記録 A ≠ 今 B → 実物の tick の合図（判定行
/// `kind=externalize origin=hook … plugin=drift`）→ `Stop` → 自席の未 consumed 退避物 → 前面を `head` に。
fn drift_parked(place: &AcctPlace, name: &str) -> IsolatedSeat {
    use vessel::hook::vessel::digest;
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に session を立てられる");
    let registered = acct_register(place, name, &acct_launcher(place, name));
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    acct_measured(&place.state, ACCT_SEAT, 13, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, 5, &acct_now());
    let root = super::tick::drift_record(place, name, Some(super::tick::DRIFT_HOOKS_A), env!("SCRIBE2_BUILD_COMMIT"));
    fs::write(digest::hooks_path(&root), super::tick::DRIFT_HOOKS_B).ok();
    write_state(&seat_dir_of(&place.state, name), StateFix::Busy { age_s: 0 });
    let pane = fixture(&place.dir, "pane.txt", IDLE_PANE);
    let first = stdout_of(&acct_tick(place, name, Some(&pane)));
    for (key, want) in [("decision", "inject"), ("kind", "externalize"), ("origin", "hook"), ("account", "a1:13"), ("plugin", "drift")] {
        assert_eq!(tick_token(&first, key).as_deref(), Some(want), "1 周目は hook 由来の退避の合図: {key}: {first}");
    }
    acct_stop(place, name, unix_now().saturating_add(1));
    wm_file(&place.wm, "working-memory.parked.md", name);
    assert!(drift_seat_head(place, name), "前面が head の席を作れる");
    guard
}

/// 立て直した周の 4 面: 雛形の穴は自席の口座（a1）の credential dir・立ち上がった席が `/rebrief` を受けた・席の記録は
/// 退避の合図（`hook-drift`）→ `/exit` → 起動 → 復元 の 4 行・登録 row は a1 のまま 1 件増える（既存 row の写し）。
fn drift_assert_relaunched_on_own_account(place: &AcctPlace, name: &str) {
    let own_dir = place.state.join("accounts").join(ACCT_SEAT);
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("{}\n", own_dir.display()),
        "穴は自席の口座（a1）の credential dir で埋まる（a2 ではない）"
    );
    assert_eq!(fs::read_to_string(place.dir.join("seat.log")).unwrap_or_default(), "/rebrief\n", "立ち上がった席が復元を受けた");
    let sent = acct_sent(&place.state, name);
    assert_eq!(sent.len(), 4, "退避の合図・/exit・起動・復元の 4 行: {sent:?}");
    assert!(sent.first().is_some_and(|what| what.contains("hook-drift")), "1 行目は hook 由来の退避の合図: {sent:?}");
    assert_eq!(sent.get(1).map(String::as_str), Some("/exit"), "2 行目は終了の手: {sent:?}");
    assert_eq!(sent.get(3).map(String::as_str), Some("/rebrief"), "4 行目は復元: {sent:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "SeatRegistered は 1 件増える: {rows:?}");
    assert_eq!(rows.last(), rows.first(), "row は既存 row の写し（口座 a1 も含めて同じ）: {rows:?}");
}

// ─────────────────── 立て直しと row の model（`s2-07l.313`・接頭辞 `seat_account_`） ───────────────────

/// 席を planner として口座 [`ACCT_SEAT`] と `--model <model>` で `seat register` の口で登録する（[`acct_register_as`] と同じ
/// 打刻の前提・`model` は表示名か別名）。
fn acct_register_with_model(place: &AcctPlace, target: &str, launch: &str, model: &str) -> Output {
    let seat = seat_dir_of(&place.state, target);
    fs::create_dir_all(&seat).ok();
    fs::write(state_file(&seat), format!("{}\n", stamp_line("idle", "SessionStart", unix_now(), ACCT_SID))).ok();
    let launch_file = fixture(&place.dir, "launch.txt", launch);
    let state = place.state.display().to_string();
    run_seat(&[
        "register", "--state-dir", &state, "--target", target, "--role", "planner", "--account", ACCT_SEAT,
        "--launch", &launch_file, "--anchor", &acct_anchor(place), "--model", model,
    ])
}

/// 退避して止まった席の fixture（[`acct_parked`] と同じ・登録は済んでいる周・a1 = 100・a2 = 30・退避の合図 → `Stop` → shell）。
fn acct_park_registered(place: &AcctPlace, target: &str) {
    acct_measured(&place.state, ACCT_SEAT, 100, &acct_now());
    acct_measured(&place.state, ACCT_SPARE, 30, &acct_now());
    let first = acct_signal(place, target);
    assert_eq!(tick_token(&first, "kind").as_deref(), Some("externalize"), "1 周目は退避の合図: {first}");
    acct_stop(place, target, unix_now().saturating_add(1));
    assert!(acct_shell_prompt(place, target, ""), "席の終了後の pane は shell の prompt で終わる");
}

/// (12) 起動行は row の model を typed に運ぶ（`s2-07l.313`・C10 / SRS FR36 / FR38・account-lifecycle.md §4）: `--model Opus`
/// （表示名）で登録した席を立て直すと、注入する起動行は雛形の末尾（`claude` の語を持たない雛形）に**別名** `--model opus` を
/// ちょうど 1 つ持つ。雛形（row の `launch`）は書き換えず（写した row にも `--model` の語は無い・`model` は `Opus` のまま写す）、
/// 穴は a2 の credential dir で埋まり復元も届く。base の起動行は `--model` を持たない（RED）。
#[test]
fn seat_account_relaunch_carries_the_row_model() {
    let place = acct_place();
    let name = "acctmodel";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let launch = acct_launcher(&place, name);
    let registered = acct_register_with_model(&place, name, &launch, "Opus");
    assert_eq!(rc_of(&registered), i32::from(RC_OK), "stderr={}", stderr_of(&registered));
    assert!(stdout_of(&registered).trim_end().ends_with(" model=Opus"), "表示名で登録できる: {}", stdout_of(&registered));
    acct_park_registered(&place, name);

    let out = acct_tick(&place, name, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("relaunch", ACCT_SPARE)] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    acct_assert_launched_then_restored(&place, name);
    acct_assert_relabelled(&place, name);
    let spare_dir = place.state.join("accounts").join(ACCT_SPARE).display().to_string();
    let sent = acct_sent(&place.state, name);
    assert_eq!(
        sent.get(1).map(String::as_str),
        Some(acct_launch_prefix(&acct_anchor(&place), &format!("{} --model opus", launch.replace("{account_dir}", &spare_dir))).as_str()),
        "起動行は row の model を別名で 1 つ運ぶ（`claude` の語が無い雛形は末尾）: {sent:?}"
    );
    let rows = acct_rows(&place.state);
    assert!(rows.iter().all(|row| row.model.as_deref() == Some("Opus")), "row の model は表示名のまま: {rows:?}");
    assert!(rows.iter().all(|row| !row.launch.split(' ').any(|word| word == "--model")), "雛形に `--model` は書かない: {rows:?}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (13) row の `model` が表に無い（`nope`）席は立て直さない: `relaunch-model-unknown`（rc 1・decision=error）・注入 0・
/// `SeatRegistered` 0・cycle-stamp なし（壊れた宣言値で黙って settings の model で立てない・C10）。`seat register --model nope`
/// 自体は使い方で断り row を書かない（未知の値を row に書かない）ので、壊れた row は `role::register` で直に積む。base は黙って
/// 起こす（RED）。
#[test]
fn seat_account_relaunch_refuses_an_unknown_row_model() {
    let place = acct_place();
    let name = "acctnomodel";
    let guard = start_seat(&place.socket, name);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let launch = acct_launcher(&place, name);
    let via_cli = acct_register_with_model(&place, name, &launch, "nope");
    assert_eq!(rc_of(&via_cli), i32::from(RC_REFUSED), "表に無い --model は使い方で断る: stdout={}", stdout_of(&via_cli));
    assert_eq!(stderr_of(&via_cli), stderr_of(&run_seat(&[])), "使い方の 1 本と同じ字面");
    assert!(stdout_of(&via_cli).is_empty(), "stdout は空");
    assert!(acct_rows(&place.state).is_empty(), "未知の値は row に書かない");
    let broken = vessel::fleet::Registration {
        role: vessel::seat::role::Role::Planner,
        anchor: acct_anchor(&place),
        target: name.to_owned(),
        sid: Some(ACCT_SID.to_owned()),
        account: ACCT_SEAT.to_owned(),
        launch,
        model: Some("nope".to_owned()),
    };
    assert!(vessel::seat::role::register(&place.state, broken).is_ok(), "壊れた row を直に積める");
    acct_park_registered(&place, name);

    let out = acct_tick(&place, name, None);

    let line = stderr_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stdout={}", stdout_of(&out));
    assert_eq!(tick_token(&line, "decision").as_deref(), Some("error"), "{line}");
    assert_eq!(tick_token(&line, "reason").as_deref(), Some("relaunch-model-unknown"), "{line}");
    acct_assert_not_relaunched(&place, name, "model-unknown");
    assert_eq!(acct_injected(&place.state, name).len(), 1, "1 周目の退避の合図だけ");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}

/// (f) `--model Fable` で起こした席の立て直しは同じ model を運ぶ（`s2-07l.313`・row の `launch` は model 無しの雛形・model は
/// row の `model` の 1 か所）: 偽 claude を `seat launch --model Fable --account l1` で起こし → 退避の合図（l1 = 100）→ `Stop` →
/// 偽 claude を終えて shell へ戻す → tick が l2 で立て直す。立て直しの起動行は `claude --model fable` を**ちょうど 1 つ**持ち
/// （`launch-model-duplicated` にならない）、偽 claude の 2 回目の argv も `--model fable` で始まり、写した row の `launch` に
/// `--model` の語は無い。run 1 の実装（雛形に `--model` を書く）は二重で断る（RED）。
#[test]
fn seat_account_relaunch_keeps_the_model_of_a_seat_launched_with_model() {
    let place = launch_place();
    let name = "launchkeep";
    let target = format!("{name}:{name}");
    let seat = format!("{name}_{name}");
    let path = launch_shims(&place, &target);
    let guard = launch_session(&place, name, &path);
    assert!(guard.ready(), "独立 socket に shell の session を立てられる");
    let launched = launch_run(&place, &path, &target, &["--account", "l1", "--model", "Fable"]);
    assert_eq!(rc_of(&launched), i32::from(RC_OK), "stdout={} stderr={}", stdout_of(&launched), stderr_of(&launched));
    acct_measured(&place.state, "l1", 100, &acct_now());
    acct_measured(&place.state, "l2", 30, &acct_now());
    write_state(&seat_dir_of(&place.state, &seat), StateFix::Busy { age_s: 0 });
    let first = stdout_of(&acct_tick(&place, &target, None));
    assert_eq!(tick_token(&first, "kind").as_deref(), Some("externalize"), "1 周目は退避の合図: {first}");
    acct_stop(&place, &seat, unix_now().saturating_add(1));
    assert!(tmux(&place.socket, &["send-keys", "-t", &target, "C-d"]).status.success(), "偽 claude を終える");
    assert!(acct_shell_prompt(&place, &target, ""), "席の終了後の pane は shell の prompt で終わる");

    let out = acct_tick(&place, &target, None);

    let line = stdout_of(&out);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stdout={line} stderr={}", stderr_of(&out));
    for (key, want) in [("decision", "inject"), ("kind", "relaunch"), ("consumed", "true"), ("account", "l1:100"), ("relaunch", "l2")] {
        assert_eq!(tick_token(&line, key).as_deref(), Some(want), "{key}: {line}");
    }
    assert_eq!(
        fs::read_to_string(place.dir.join("launched")).unwrap_or_default(),
        format!("--model\nfable\n{}--model\nfable\n{}", launch_expected_argv(&place, "l1"), launch_expected_argv(&place, "l2")),
        "起動と立て直しの argv はどちらも `--model fable` で始まる"
    );
    let sent = acct_sent(&place.state, &seat);
    assert_eq!(sent.len(), 4, "起動・退避の合図・立て直し・復元の 4 行: {sent:?}");
    let l2_dir = place.state.join("accounts").join("l2").display().to_string();
    assert!(
        sent.get(2).is_some_and(|what| what.starts_with(&acct_launch_prefix(&launch_anchor(&place), &format!("CLAUDE_CONFIG_DIR={l2_dir} claude --model fable --plugin-dir "))) && launch_model_words(what) == 1),
        "立て直しの起動行は model を 1 つ運ぶ: {sent:?}"
    );
    assert_eq!(sent.get(3).map(String::as_str), Some("/rebrief"), "{sent:?}");
    let rows = acct_rows(&place.state);
    assert_eq!(rows.len(), 2, "SeatRegistered は 1 件増える: {rows:?}");
    assert!(rows.iter().all(|row| row.model.as_deref() == Some("Fable") && launch_model_words(&row.launch) == 0), "雛形に `--model` は無い: {rows:?}");
    drop(guard);
    fs::remove_dir_all(&place.dir).ok();
}
