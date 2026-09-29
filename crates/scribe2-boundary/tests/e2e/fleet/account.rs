// flip-check: moved s2-07l.680
//! account_cmd の族の歯（接頭辞 `account_cmd_`・設計 docs/design/carry-prep.md §9 行 h・親 `tests/e2e/fleet.rs` の helper を `use super::*` で使う）。

use super::*;

/// (6e) `KINDS` の 13〜16 番目は登録 → 退役 → 戻し → 列の印の順（口座の退役・戻しの後ろに列の印 1 を足した・
/// account-lifecycle.md §3・dispatcher.md §4）。母集団の件数と末尾は [`fleet_kinds_follow_declaration_order`] が pin する。
#[test]
fn account_cmd_kinds_are_fifteen_with_retire_and_restore_last() {
    assert_eq!(
        KINDS.len(),
        29,
        "母集団（既存 10 + 口座残量 2 + 席の登録 1 + 口座の退役・戻し 2 + 列の印 1 + install 1 + 消費 1 + 裁定 1 + 群の逼迫の通知 1 + 群の移動 3 + 登録 row の退役 1 + 案件の一生 5）"
    );
    assert_eq!(
        KINDS.get(12..16),
        Some(
            &[
                EventKind::SeatRegistered,
                EventKind::AccountRetired,
                EventKind::AccountRestored,
                EventKind::DispatchMark,
            ][..]
        ),
        "登録 → 退役 → 戻し → 列の印の順"
    );
    for kind in [EventKind::SeatRegistered, EventKind::AccountRetired, EventKind::AccountRestored] {
        assert!(!kind.is_allowance(), "{} は口座残量の kind ではない", kind.as_str());
        assert_eq!(EventKind::parse(kind.as_str()), Some(kind), "{}", kind.as_str());
        assert_eq!(kind.default_actor(), "machine", "{} は機械由来", kind.as_str());
    }
    assert!(
        is_declaration_order(KINDS, |kind| kind as usize),
        "KINDS の並びが宣言順と乖離している（母集団 {} 種）",
        KINDS.len()
    );
    assert_eq!(
        KINDS.iter().filter(|kind| kind.is_allowance()).count(),
        2,
        "便に紐づかない kind は 2 つだけ"
    );
    for kind in [EventKind::AllowanceMeasured, EventKind::AllowanceUnmeasured] {
        assert_eq!(EventKind::parse(kind.as_str()), Some(kind), "{}", kind.as_str());
        assert_eq!(kind.default_actor(), "machine", "計測は機械由来（FR22 不変）");
    }
}

/// (6a) `add` は dir と直下の `settings.json`（agent view を切る 1 項目）と host の面の `[[account]]` 行 1 つを揃え、stdout に
/// login の起動行を 1 行（cwd は `--anchor`・無ければ置き場）。2 つ目の口座は既存の宣言の後ろに 1 行。
/// base は `account` の subcommand が無く使い方で断る（RED・機能不在）。
#[test]
fn account_cmd_add_prepares_the_dir_settings_and_one_declaration_line() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let out = run_account(&["add", "a1", "--state-dir", &path]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    let a1 = dir.join("accounts").join("a1");
    assert_eq!(text(&out.stdout), format!("account: prepared a1 next=cd {path} && CLAUDE_CONFIG_DIR={} claude\n", a1.display()));
    assert!(a1.is_dir(), "dir を作る");
    assert_eq!(fs::read_to_string(a1.join("settings.json")).unwrap_or_default(), "{\"disableAgentView\": true}\n", "1 項目だけ");
    assert_eq!(fs::read_dir(&a1).map(Iterator::count).unwrap_or_default(), 1, "credential は書かない（settings.json だけ）");
    assert_eq!(host_text(&dir), host_body(&["a1"]), "schema = 1 から作り行を 1 つ");
    assert!(!dir.join("host.toml.staged").exists(), "一時 file を残さない");
    let anchor = dir.join("anchor");
    let anchored = run_account(&["add", "a3", "--state-dir", &path, "--anchor", &anchor.display().to_string()]);
    assert_eq!(anchored.status.code(), Some(i32::from(RC_OK)), "{anchored:?}");
    let a3 = dir.join("accounts").join("a3");
    assert_eq!(text(&anchored.stdout), format!("account: prepared a3 next=cd {} && CLAUDE_CONFIG_DIR={} claude\n", anchor.display(), a3.display()));
    assert_eq!(host_text(&dir), host_body(&["a1", "a3"]), "既存の宣言の後ろに 1 行");
    fs::remove_dir_all(&dir).ok();
}

/// (6a) 2 回目の `add`（宣言済み）は `exists`・user の既存 dir は `dir-exists` で、どちらも何も書かない（宣言も足さない）。
#[test]
fn account_cmd_add_refuses_exists_and_dir_exists_without_writing() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let first = run_account(&["add", "a1", "--state-dir", &path]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "{first:?}");
    let a2 = dir.join("accounts").join("a2");
    fs::create_dir_all(&a2).expect("user の dir を置ける");
    fs::write(a2.join("mine"), "user").expect("user の file を置ける");
    let before = tree(&dir);
    for (label, reason) in [("a1", "exists"), ("a2", "dir-exists")] {
        let out = run_account(&["add", label, "--state-dir", &path]);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{out:?}");
        assert!(out.stdout.is_empty(), "断りは stdout 0 byte");
        assert_eq!(text(&out.stderr), format!("account: refused reason={reason} label={label}\n"));
        assert_eq!(tree(&dir), before, "{label}: 何も書かない");
    }
    fs::remove_dir_all(&dir).ok();
}

/// (6a) `--target` は契約 (b) と同じ注入の門を通して login の起動行を shell へ 1 回だけ送る（偽 claude が
/// `CLAUDE_CONFIG_DIR` = 口座の dir・cwd = `--anchor` で 1 回走る）。入力欄に打ちかけの在る shell へは 1 key も送らず
/// `refused=input-busy` と行を返す（dir と宣言は揃え終えている）。
#[test]
fn account_cmd_add_target_injects_the_login_line_once() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let shim = login_shim(&dir);
    let name = "acctlogin";
    let (guard, ready) = login_session(&dir, name, &shim);
    assert!(ready, "独立 socket に shell の session を立てられる: {}", login_pane(&guard.socket, name));
    let anchor = dir.join("anchor");
    fs::create_dir_all(&anchor).expect("anchor を作れる");
    let target = format!("{name}:{name}");
    let anchor_s = anchor.display().to_string();

    let out = run_account(&["add", "a1", "--state-dir", &path, "--anchor", &anchor_s, "--target", &target, "--tmux-socket", &guard.socket]);

    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(text(&out.stdout), format!("account: prepared a1 target={name}_{name}\n"));
    let log = dir.join("login.log");
    let real = fs::canonicalize(&anchor).expect("anchor を実 path にできる");
    let want = format!("{} {}\n", dir.join("accounts").join("a1").display(), real.display());
    assert!(
        wait_until(|| fs::read_to_string(&log).unwrap_or_default() == want),
        "偽 claude が口座の dir と anchor で 1 回走る: {:?} pane={}",
        fs::read_to_string(&log),
        login_pane(&guard.socket, name)
    );
    assert!(wait_until(|| login_pane(&guard.socket, name).trim_end().ends_with('$')), "prompt に戻る");
    assert!(crate::seat::tmux(&guard.socket, &["send-keys", "-t", &target, "-l", "git st"]).status.success());
    assert!(wait_until(|| login_pane(&guard.socket, name).trim_end().ends_with("$ git st")), "打ちかけが描かれる");
    let busy = run_account(&["add", "a2", "--state-dir", &path, "--target", &target, "--tmux-socket", &guard.socket]);
    assert_eq!(busy.status.code(), Some(i32::from(RC_REFUSED)), "{busy:?}");
    assert!(text(&busy.stderr).starts_with("account: prepared a2 refused=input-busy next=cd "), "{busy:?}");
    assert!(dir.join("accounts").join("a2").join("settings.json").is_file(), "dir と宣言は揃え終えている");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), want, "門で止まった周は 1 key も送らない＝偽 claude は 1 回だけ");
    drop(guard);
    fs::remove_dir_all(&dir).ok();
}

/// (6b) `ls` の行は doctor の口座行（同じ 1 関数・`retired=` 込み）に最新の実測の要約（実測行あり＝使用率・なし＝
/// `unmeasured`）を足したもの（label の辞書順・rc 0・計測は撃たない＝event は増えない）。
#[test]
fn account_cmd_ls_is_the_doctor_line_plus_retired_and_allowance() {
    let dir = state_dir();
    let path = dir.display().to_string();
    put_host_labels(&dir, &["zeta", "a1"]);
    let a1 = dir.join("accounts").join("a1");
    fs::create_dir_all(&a1).expect("口座の dir を作れる");
    fs::write(a1.join(".credentials.json"), "{}").expect("credential の印を置ける");
    fs::write(a1.join("settings.json"), "{\"disableAgentView\": true}").expect("settings を置ける");
    let policy = LockPolicy::embedded().expect("rules 行を引ける");
    for row in [measured("a1", WindowKind::FiveHour, None, 13), measured("a1", WindowKind::SevenDay, None, 41)] {
        store::append(&dir, &allowance_event(ALLOWANCE_TS, row), policy).expect("追記できる");
    }
    let events = fs::read(store::events_path(&dir)).unwrap_or_default();

    let out = run_account(&["ls", "--state-dir", &path]);

    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    let lines: Vec<String> = text(&out.stdout).lines().map(str::to_owned).collect();
    assert_eq!(
        lines,
        [
            "account=a1 dir=present credential=present config=present agentview=off trust=n/a retired=no five_hour=13% seven_day=41%",
            "account=zeta dir=missing credential=missing config=missing agentview=unreadable trust=n/a retired=no five_hour=unmeasured seven_day=unmeasured",
        ]
    );
    assert_eq!(fs::read(store::events_path(&dir)).unwrap_or_default(), events, "計測を撃たない");
    let socket = dir.join("no-sock").display().to_string();
    let doctor = Command::new(bin()).args(["doctor", "--state-dir", &path, "--tmux-socket", &socket]).output().expect("binary を起動できる");
    let doctor_rows: Vec<String> = text(&doctor.stdout).lines().filter(|line| line.starts_with("account=")).map(str::to_owned).collect();
    let heads: Vec<String> = lines.iter().map(|line| line.split(" five_hour=").next().unwrap_or_default().to_owned()).collect();
    assert_eq!(doctor_rows, heads, "doctor の口座行と同じ行（要約だけが後ろに足される）");
    fs::remove_dir_all(&dir).ok();
}

/// (6c) `retire` は dir を `.retired/` へ 1 つだけ動かし（中身ごと）・`AccountRetired` を 1 件積み・宣言の行は残す。
#[test]
fn account_cmd_retire_moves_the_dir_once_and_records_one_event() {
    let (fx, state, _) = retire_place();
    let out = run_account(&["retire", "a1", "--state-dir", &state]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{out:?}");
    assert_eq!(text(&out.stdout), "account: retired a1\n");
    assert!(fs::symlink_metadata(fx.state.join("accounts").join("a1")).is_err(), "元の場所から消える");
    let moved = retired_entries(&fx.state);
    assert_eq!(moved.len(), 1, "退役先に 1 つだけ: {moved:?}");
    let name = moved.first().cloned().unwrap_or_default();
    assert!(name.starts_with("a1."), "{name}");
    assert!(fx.state.join("accounts").join(".retired").join(&name).join(".credentials.json").is_file(), "中身ごと動く");
    assert_eq!(account_events(&fx.state), vec![(EventKind::AccountRetired, Some("a1".to_owned()))], "event 1 件");
    assert_eq!(host_text(&fx.state), host_body(&["a1", "a2"]), "宣言の行は消さない");
    drop_fixture(&fx);
}

/// (6c) 退役中の口座は `fleet select` の候補から消え（同点で辞書順の先だった a1 → a2）、`fleet usage` が測らない（a1 の行が
/// 増えない）。
#[test]
fn account_cmd_retired_account_leaves_select_and_usage() {
    let (fx, state, curl) = retire_place();
    let select = || text(&run_fleet(&["select", "--purpose", "session", "--state-dir", &state, "--curl", &curl]).stdout);
    assert_eq!(select(), "select purpose=session chosen=a1\n", "同点は辞書順で a1");
    assert_eq!(run_account(&["retire", "a1", "--state-dir", &state]).status.code(), Some(i32::from(RC_OK)));
    let a1_rows = |fx: &UsageFixture| allowances(fx).iter().filter(|row| row.key().account == "a1").count();
    let measured_a1 = a1_rows(&fx);
    let usage = run_fleet(&["usage", "--state-dir", &state, "--curl", &curl]);
    assert_eq!(out_lines(&usage), vec![live_line("a2")], "退役中の a1 は測らない: {usage:?}");
    assert_eq!(a1_rows(&fx), measured_a1, "a1 の行は増えない");
    assert_eq!(select(), "select purpose=session chosen=a2\n", "退役中の a1 は候補に入らない");
    drop_fixture(&fx);
}

/// (6c) 登録 row のどれかが持つ口座は `in-use` で動かず、event も書かない（置き場の全 entry が不変）。
#[test]
fn account_cmd_retire_refuses_an_account_in_use() {
    let (fx, state, _) = retire_place();
    register_account(&fx.state, "a2");
    let before = tree(&fx.state);
    let used = run_account(&["retire", "a2", "--state-dir", &state]);
    assert_eq!(used.status.code(), Some(i32::from(RC_REFUSED)), "{used:?}");
    assert_eq!(text(&used.stderr), "account: refused reason=in-use label=a2\n");
    assert_eq!(tree(&fx.state), before, "dir も event も不変");
    drop_fixture(&fx);
}

/// (6d) `restore` は最新の退役先を元へ戻し（中身ごと・link は link のまま）`AccountRestored` を 1 件積む（退役・戻しが 1 件ずつ）。
#[test]
fn account_cmd_restore_moves_back_once_and_records_one_event() {
    let (dir, path, real) = restore_place();
    for label in ["a1", "a3"] {
        account_ok(&["retire", label, "--state-dir", &path], &format!("account: retired {label}"));
    }
    let link = retired_entries(&dir).into_iter().find(|name| name.starts_with("a3.")).unwrap_or_default();
    assert!(is_link_to(&dir.join("accounts").join(".retired").join(&link), &real), "link は link のまま動く: {link}");
    for label in ["a1", "a3"] {
        account_ok(&["restore", label, "--state-dir", &path], &format!("account: restored {label}"));
    }
    assert_eq!(fs::read_to_string(dir.join("accounts").join("a1").join("mine")).unwrap_or_default(), "kept", "中身ごと戻る");
    assert!(is_link_to(&dir.join("accounts").join("a3"), &real), "link のまま戻る");
    assert!(retired_entries(&dir).is_empty(), "退役先に残らない");
    let (a1, a3) = (Some("a1".to_owned()), Some("a3".to_owned()));
    let want = vec![
        (EventKind::AccountRetired, a1.clone()),
        (EventKind::AccountRetired, a3.clone()),
        (EventKind::AccountRestored, a1),
        (EventKind::AccountRestored, a3),
    ];
    assert_eq!(account_events(&dir), want, "退役・戻しが 1 件ずつ");
    fs::remove_dir_all(&dir).ok();
}

/// (6d) `ls` の `retired=` は退役の間だけ `yes`（戻せば `no`）。
#[test]
fn account_cmd_ls_names_retired_only_while_retired() {
    let (dir, path, _) = restore_place();
    let count = |word: &str| {
        let listed = text(&run_account(&["ls", "--state-dir", &path]).stdout);
        listed.lines().filter(|line| line.contains(&format!(" retired={word} "))).count()
    };
    assert_eq!(count("no"), 2, "退役前");
    account_ok(&["retire", "a1", "--state-dir", &path], "account: retired a1");
    assert_eq!((count("yes"), count("no")), (1, 1), "退役中の a1 だけ yes");
    account_ok(&["restore", "a1", "--state-dir", &path], "account: restored a1");
    assert_eq!(count("no"), 2, "戻せば no");
    fs::remove_dir_all(&dir).ok();
}

/// (6d) 退役中でない label は `not-retired`・元の場所が埋まっている周は `dir-exists` で、どちらも何も書かない。
#[test]
fn account_cmd_restore_refuses_not_retired_and_an_occupied_place() {
    let (dir, path, _) = restore_place();
    let refused = |reason: &str| {
        let before = tree(&dir);
        let out = run_account(&["restore", "a1", "--state-dir", &path]);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{out:?}");
        assert_eq!(text(&out.stderr), format!("account: refused reason={reason} label=a1\n"));
        assert_eq!(tree(&dir), before, "{reason}: 何も書かない");
    };
    refused("not-retired");
    account_ok(&["retire", "a1", "--state-dir", &path], "account: retired a1");
    fs::create_dir_all(dir.join("accounts").join("a1")).expect("user が元の場所を作り直す");
    refused("dir-exists");
    fs::remove_dir_all(&dir).ok();
}

/// (6f) 前提違反（label の規則・`exists`・`dir-exists`・`unknown`・`in-use`・`not-retired`）と使い方の誤りは、file も
/// event も 1 byte も変えずに断る（置き場の全 entry の本文が不変）。
#[test]
fn account_cmd_refusals_write_no_file_and_no_event() {
    let dir = state_dir();
    let path = dir.display().to_string();
    put_host_labels(&dir, &["a1"]);
    fs::create_dir_all(dir.join("accounts").join("a1")).expect("口座の dir を作れる");
    fs::create_dir_all(dir.join("accounts").join("a9")).expect("宣言の無い dir を置ける");
    register_account(&dir, "a1");
    let before = tree(&dir);
    for (args, reason, label) in [
        (&["add", "a/b"][..], "label-invalid", "a/b"),
        (&["add", ".x"], "label-invalid", ".x"),
        (&["add", "a1"], "exists", "a1"),
        (&["add", "a9"], "dir-exists", "a9"),
        (&["retire", "ghost"], "unknown", "ghost"),
        (&["retire", "a1"], "in-use", "a1"),
        (&["retire", "../x"], "label-invalid", "../x"),
        (&["restore", "a1"], "not-retired", "a1"),
        (&["restore", "ghost"], "unknown", "ghost"),
    ] {
        let mut call = args.to_vec();
        call.extend_from_slice(&["--state-dir", &path]);
        let out = run_account(&call);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty(), "{args:?}: stdout 0 byte");
        assert_eq!(text(&out.stderr), format!("account: refused reason={reason} label={label}\n"), "{args:?}");
        assert_eq!(tree(&dir), before, "{args:?}: file も event も不変");
    }
    let usage = text(&run_account(&[]).stderr);
    assert!(usage.starts_with("usage: account <add <label>"), "{usage}");
    for bad in [&["add"][..], &["nope", "--state-dir", &path]] {
        let out = run_account(bad);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{bad:?}");
        assert_eq!(text(&out.stderr), usage, "{bad:?} は使い方で断る");
    }
    // flag の閉包の断り（欠け・値欠け・重複・未知）は typed な理由の 1 行 + usage で rc 2（設計 pipeline.md §14 約束 4）。
    for (bad, reason) in [
        (&["add", "a2"][..], "--state-dir に値が無い"),
        (&["ls"], "--state-dir に値が無い"),
        (&["ls", "--state-dir"], "--state-dir に値が無い"),
        (&["ls", "--state-dir", &path, "--state-dir", &path], "--state-dir が 2 回以上在る"),
        (&["retire", "a1", "--state-dir", &path, "--target", "x:y"], "未知の引数 --target"),
    ] {
        let out = run_account(bad);
        assert_eq!(out.status.code(), Some(2), "{bad:?}");
        assert_eq!(text(&out.stderr), format!("account: {reason}\n{usage}"), "{bad:?} は理由と使い方で断る");
    }
    assert_eq!(tree(&dir), before, "使い方の誤りも何も書かない");
    fs::remove_dir_all(&dir).ok();
}

// flip-check: retroactive s2-07l.283
/// (a) host の面が**在るのに読めない**（dir である）周は `write-failed` で断り、`schema = 1` から作り直さない（user の宣言を
/// 「無い」に読み替えない・NFR4）: host.toml は dir のまま・`host.toml.staged` も `accounts/` も現れない（置き場の全 entry が不変）。
#[test]
fn account_cmd_add_refuses_when_host_manifest_is_unreadable_without_rewriting() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let host = dir.join(vessel::rules::HOST_MANIFEST);
    fs::create_dir(&host).expect("host.toml を dir として置ける");
    let before = tree(&dir);
    account_write_failed(&["add", "a1", "--state-dir", &path], "a1");
    assert!(host.is_dir(), "host.toml は dir のまま");
    assert!(fs::symlink_metadata(dir.join("host.toml.staged")).is_err(), "一時 file を作らない");
    assert!(fs::symlink_metadata(dir.join("accounts")).is_err(), "口座の dir（親ごと）を作らない");
    assert_eq!(tree(&dir), before, "置き場の全 entry が不変");
    fs::remove_dir_all(&dir).ok();
}

// flip-check: retroactive s2-07l.283
/// (b) 改行で終わる既存の host.toml（`[[account]]` 1 本）への追記は区切りの空行 1 つ + 2 行＝行数が元 + 3 で、元の本文は
/// 接頭辞として不変（余分な空行が入らない）。対で、末尾改行の無い host.toml へも同じ形（前の行と `[[account]]` が別の行＝読める）。
#[test]
fn account_cmd_add_appends_without_a_blank_line_to_a_newline_terminated_manifest() {
    for (name, original) in [("改行終端", host_body(&["x"])), ("改行なし", host_body(&["x"]).trim_end().to_owned())] {
        let dir = state_dir();
        let path = dir.display().to_string();
        fs::write(dir.join(vessel::rules::HOST_MANIFEST), &original).expect("host の面を書ける");
        let before = original.lines().count();
        let a1 = dir.join("accounts").join("a1");
        account_ok(&["add", "a1", "--state-dir", &path], &format!("account: prepared a1 next=cd {path} && CLAUDE_CONFIG_DIR={} claude", a1.display()));
        let after = host_text(&dir);
        assert_eq!(after.lines().count(), before + 3, "{name}: 区切りの空行 1 つ + 2 行: {after:?}");
        assert!(after.starts_with(original.trim_end()), "{name}: 元の本文は接頭辞として不変: {after:?}");
        assert_eq!(after, host_body(&["x", "a1"]), "{name}: 余分な空行が入らない");
        let lines: Vec<&str> = after.lines().collect();
        assert_eq!(lines.get(before.saturating_sub(1) + 2), Some(&"[[account]]"), "{name}: 前の行と別の行に [[account]]: {lines:?}");
        let listed = text(&run_account(&["ls", "--state-dir", &path]).stdout);
        assert_eq!(listed.lines().filter(|line| line.starts_with("account=")).count(), 2, "{name}: 2 口座とも読める: {listed}");
        fs::remove_dir_all(&dir).ok();
    }
}

// flip-check: retroactive s2-07l.283
/// (c) 既存の host.toml が未知 key を持つ（host の面として読めない）周は `write-failed` で断り、host.toml の bytes は不変・
/// `host.toml.staged` が残らず・口座の dir も現れない（壊れた面を rename して上書きしない・NFR4）。
#[test]
fn account_cmd_add_refuses_when_the_staged_manifest_does_not_parse_and_keeps_host_toml() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let host = dir.join(vessel::rules::HOST_MANIFEST);
    let broken = "schema = 1\n\n[[account]]\nlabel = \"x\"\nbogus = 1\n";
    fs::write(&host, broken).expect("host の面を書ける");
    let before = tree(&dir);
    account_write_failed(&["add", "a1", "--state-dir", &path], "a1");
    assert_eq!(fs::read(&host).unwrap_or_default(), broken.as_bytes(), "host.toml の bytes は不変");
    assert!(fs::symlink_metadata(dir.join("host.toml.staged")).is_err(), "一時 file が残らない");
    assert!(fs::symlink_metadata(dir.join("accounts").join("a1")).is_err(), "口座の dir を作らない");
    assert_eq!(tree(&dir), before, "置き場の全 entry が不変");
    fs::remove_dir_all(&dir).ok();
}

// flip-check: retroactive s2-07l.283
/// (d) 宣言だけ在って `<state>/accounts/<label>` が無い口座の `retire` は、退役先の親（`accounts/.retired/`）を**作る前に**
/// `write-failed` で断り、event も 0 件（置き場の全 entry が不変）。正例は
/// [`account_cmd_retire_moves_the_dir_once_and_records_one_event`]。
#[test]
fn account_cmd_retire_refuses_a_missing_dir_before_creating_the_retired_parent() {
    let dir = state_dir();
    let path = dir.display().to_string();
    put_host_labels(&dir, &["a1"]);
    let before = tree(&dir);
    account_write_failed(&["retire", "a1", "--state-dir", &path], "a1");
    assert!(fs::symlink_metadata(dir.join("accounts").join(".retired")).is_err(), "退役先の親を作らない");
    assert!(fs::symlink_metadata(dir.join("accounts")).is_err(), "accounts/ も作らない");
    assert!(account_events(&dir).is_empty(), "event 0 件");
    assert_eq!(tree(&dir), before, "置き場の全 entry が不変");
    fs::remove_dir_all(&dir).ok();
}

// flip-check: retroactive s2-07l.283
/// (e) flag の 3 条件はそれぞれ単独で使い方の誤り: 空の値（`--anchor ''`）は usage（rc 1）・`--` で始まる値（`--anchor --target`）と
/// 未知の flag（`--bogus x`）は閉包の断り（typed な理由の 1 行 + usage・rc 2・設計 pipeline.md §14 約束 4）で断り、どれも stdout 0 byte で
/// file も event も書かない（3 形を別々に撃つ）。
#[test]
fn account_cmd_flags_refuse_empty_value_dashed_value_and_unknown_flag() {
    let dir = state_dir();
    let path = dir.display().to_string();
    let before = tree(&dir);
    let usage = text(&run_account(&[]).stderr);
    assert!(usage.starts_with("usage: account <add <label>"), "{usage}");
    for (name, tail, rc, err) in [
        ("空の値", &["--anchor", ""][..], i32::from(RC_REFUSED), usage.clone()),
        ("-- で始まる値", &["--anchor", "--target"], 2, format!("account: --anchor に値が無い\n{usage}")),
        ("未知の flag", &["--bogus", "x"], 2, format!("account: 未知の引数 --bogus\n{usage}")),
    ] {
        let mut call = vec!["add", "a2", "--state-dir", &path];
        call.extend_from_slice(tail);
        let out = run_account(&call);
        assert_eq!(out.status.code(), Some(rc), "{name}: {out:?}");
        assert!(out.stdout.is_empty(), "{name}: stdout 0 byte");
        assert_eq!(text(&out.stderr), err, "{name}: 使い方で断る");
        assert_eq!(tree(&dir), before, "{name}: file も event も不変");
    }
    fs::remove_dir_all(&dir).ok();
}
