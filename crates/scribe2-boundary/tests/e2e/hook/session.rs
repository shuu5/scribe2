// flip-check: moved s2-07l.679
//! session の族の歯（接頭辞 `hook_session_` / `hook_recovery_` / `hook_precompact_`・設計 docs/design/carry-prep.md §9 行 g・
//! 発話の記帳 `hook_utterance_record_`・turn の終わりの止め `hook_unsorted_stop_`）。

use super::*;

#[test]
fn hook_session_start_is_noop_without_marker() {
    let repo = git_repo();
    let out = run_hook("session-start", &payload(&repo));
    assert_silent(&out, "marker が無い repo");
    clean(&[&repo]);
}

#[test]
fn hook_session_start_is_noop_for_other_name() {
    let repo = git_repo();
    let other = Marker {
        name: "other-vessel".to_owned(),
        version: GENERATION,
    };
    fs::write(repo.join(MARKER), other.render()).expect("marker を書ける");
    let out = run_hook("session-start", &payload(&repo));
    assert_silent(&out, "別の器が名乗る repo");
    clean(&[&repo]);
}

#[test]
fn hook_session_start_is_noop_without_state_dir() {
    let repo = git_repo();
    let mine = Marker {
        name: NAME.to_owned(),
        version: GENERATION,
    };
    fs::write(repo.join(MARKER), mine.render()).expect("marker を書ける");
    let out = run_hook("session-start", &payload(&repo));
    assert_silent(&out, "marker は在るが state dir が紐づいていない repo");
    clean(&[&repo]);
}

#[test]
fn hook_session_start_serves_own_marker() {
    let repo = git_repo();
    let state = linked(&repo);
    let out = run_hook("session-start", &payload(&repo));

    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "仕える周も rc 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&format!("[{NAME}/SessionStart]")),
        "名乗りの 1 行が出る: {stdout}"
    );

    let lines = inject_lines(&state);
    assert_eq!(lines.len(), 1, "注入の記録は 1 件（母集団 {}）", lines.len());
    let line = lines.first().map_or_else(String::new, Clone::clone);
    assert_eq!(
        value_of(&line, "schema"),
        Some(json_lite::Value::Num(SCHEMA)),
        "schema=1: {line}"
    );
    let bytes = value_of(&line, "bytes").and_then(|value| value.as_num());
    assert!(bytes.is_some_and(|found| found > 0), "bytes>0: {line}");
    assert_eq!(
        value_of(&line, "tokens"),
        Some(json_lite::Value::Null),
        "数えていない token は null で表す: {line}"
    );
    clean(&[&repo, &state]);
}

#[test]
fn hook_session_start_honors_state_dir_flag() {
    let repo = git_repo();
    let linked_state = linked(&repo);
    let override_state = tmp();
    let out = run_hook_args(
        &[
            "session-start",
            "--state-dir",
            &override_state.display().to_string(),
        ],
        &payload(&repo),
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "仕える周は rc 0");
    assert_eq!(
        inject_lines(&override_state).len(),
        1,
        "--state-dir が置き場を上書きする"
    );
    assert!(
        inject_lines(&linked_state).is_empty(),
        "git 設定の置き場へは書かない"
    );
    clean(&[&repo, &linked_state, &override_state]);
}

/// (a)(b)(f): in_progress の bead は `[RECENT-WIP]` に全件、直近 24 時間の更新は `[RECENT-BEAD]` に新しい順で上限まで
/// （上限で切った周は `[RECENT-CUT]` が shown と total を持つ）、窓の外と WIP に出た id は BEAD に出ない。改行入りの題は
/// 1 行に畳まれて行数が増えない。§5 の 12 行は不変で、記録は `session-start-recent` の 1 行が足される。
#[test]
fn hook_session_recent_lists_wip_and_windowed_beads_after_the_brief() {
    let place = role_place();
    let path = stub_seat(&place, "recentwip", Some("orchestrator"));
    let now = vessel::seat::state::now_secs();
    let mut items = vec![
        bead_json("w-1", "in_progress", now - 100, "仕掛かり A"),
        bead_json("w-2", "in_progress", now - 50, "行1\n行2\r\n\t行3"),
        bead_json("old-1", "open", now - WINDOW_SECS - 3_600, "窓の外"),
    ];
    let inside = BEAD_LIMIT + 2;
    for index in 0..inside {
        items.push(bead_json(&format!("r-{index}"), "open", now - 60 * (index as u64 + 1), &format!("直近 {index}")));
    }
    let bd = fake_bd_in(&place, "ledger-a", &format!("[{}]", items.join(",")));
    let before = inject_lines(&place.state).len();
    let (_, recent) = brief_and_recent(&place, &path, &bd);
    let wip = lines_of(&recent, Kind::Wip);
    assert_eq!(wip.len(), 2, "in_progress の全件: {recent:?}");
    assert!(wip[0].starts_with("[RECENT-WIP] w-1 ") && wip[0].ends_with(" 仕掛かり A"), "{}", wip[0]);
    assert!(wip[1].starts_with("[RECENT-WIP] w-2 ") && wip[1].ends_with(" 行1 行2   行3"), "改行は空白へ: {}", wip[1]);
    assert!(wip[0].contains(&vessel::fleet::cli::format_utc(now - 100)), "更新時刻: {}", wip[0]);
    assert_bead_section(&recent, inside);
    assert!(recent.iter().all(|line| !line.contains("old-1")), "窓の外は出ない: {recent:?}");
    assert!(marked(&recent, recent::MARKER_NONE, Kind::Wip).is_empty() && marked(&recent, recent::MARKER_NONE, Kind::Bead).is_empty());
    assert!(marked(&recent, recent::MARKER_UNMEASURED, Kind::Wip).is_empty(), "読めた周に UNMEASURED は無い: {recent:?}");
    assert_recent_record(&place.state, before, &recent, "recentwip_recentwip");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (c): 台帳が読めない周（`--bd` が無い file・JSON でない出力）は wip / bead の 2 種類だけ `[RECENT-UNMEASURED]`
/// （理由 `ledger-unreadable`）で、§5 の 12 行と git の行は出る。席は止めない（rc 0・stderr 0 byte）。
#[test]
fn hook_session_recent_marks_ledger_unmeasured_but_keeps_brief_and_git() {
    let place = role_place();
    let path = stub_seat(&place, "recentnobd", Some("orchestrator"));
    let missing = place.sock_dir.join("no-such-bd").display().to_string();
    let garbage = fake_bd_in(&place, "ledger-garbage", "not json");
    for bd in [missing.as_str(), garbage.as_str()] {
        let (brief, recent) = brief_and_recent(&place, &path, bd);
        assert!(brief.iter().any(|line| line.contains("unknown（台帳を読めない）")), "{brief:?}");
        for kind in [Kind::Wip, Kind::Bead] {
            let expected = format!("[RECENT-UNMEASURED] kind={} reason=ledger-unreadable", kind.as_str());
            assert_eq!(marked(&recent, recent::MARKER_UNMEASURED, kind), vec![&expected], "{bd}: {recent:?}");
            assert!(marked(&recent, recent::MARKER_NONE, kind).is_empty(), "測れない周に NONE は出ない: {recent:?}");
        }
        assert_eq!(lines_of(&recent, Kind::Git).len(), 1, "git の行は出る: {recent:?}");
        assert!(!lines_of(&recent, Kind::Commit).is_empty(), "commit の行は出る: {recent:?}");
        assert!(recent.iter().all(|line| !line.contains("git-unavailable") && !line.contains("not-a-repo")), "{recent:?}");
    }
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (g): 読めた上で 0 件の種類（in_progress が 0・窓の内の更新が 0・dirty が 0）は `[RECENT-NONE] kind=<種類>` で、
/// 同じ種類の `[RECENT-UNMEASURED]` は出ない（(c) と対＝0 件と測れないの両側）。
#[test]
fn hook_session_recent_none_is_distinct_from_unmeasured() {
    let place = role_place();
    let path = stub_seat(&place, "recentnone", Some("orchestrator"));
    // anchor を clean にする（`vessel init` の marker は untracked なので commit に含める）。
    git(&place.repo, &["add", "-A"]);
    git(&place.repo, &["commit", "-q", "-m", "marker"]);
    let now = vessel::seat::state::now_secs();
    let stale = bead_json("s-1", "open", now - WINDOW_SECS - 1, "窓の直外");
    let bd = fake_bd_in(&place, "ledger-empty", &format!("[{stale}]"));
    let (_, recent) = brief_and_recent(&place, &path, &bd);
    for kind in [Kind::Wip, Kind::Bead, Kind::Dirty] {
        let expected = format!("[RECENT-NONE] kind={}", kind.as_str());
        assert_eq!(marked(&recent, recent::MARKER_NONE, kind), vec![&expected], "{recent:?}");
        assert!(marked(&recent, recent::MARKER_UNMEASURED, kind).is_empty(), "読めた周に UNMEASURED は出ない: {recent:?}");
        assert!(lines_of(&recent, kind).is_empty(), "0 件の種類に本体の行は無い: {recent:?}");
    }
    assert!(recent.iter().all(|line| !line.contains("s-1")), "窓の直外は出ない: {recent:?}");
    assert_eq!(lines_of(&recent, Kind::Git).len(), 1, "git の行は 1 行: {recent:?}");
    assert_eq!(lines_of(&recent, Kind::Commit).len(), 2, "seed + marker の 2 commit: {recent:?}");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (d): `[RECENT-GIT]` が head（短い sha）・branch・上流との ahead / behind を持ち、`[RECENT-COMMIT]` が直近の commit を
/// 新しい順に短い sha と subject で持つ（上限を超える周は `[RECENT-CUT] kind=commit`）。dirty な worktree（anchor は `.`・
/// 追加の worktree は anchor 相対）が `[RECENT-DIRTY]` に出て、clean な worktree は出ない。
#[test]
fn hook_session_recent_git_head_commits_and_dirty_worktree() {
    let place = role_place();
    let path = stub_seat(&place, "recentgit", Some("orchestrator"));
    let repo = &place.repo;
    // 上流: bare の写しを origin にし、追跡 ref を持たせてから手元にだけ commit を積む（network に出ない）。
    let origin = place.sock_dir.join("origin.git");
    git(repo, &["clone", "-q", "--bare", &repo.display().to_string(), &origin.display().to_string()]);
    git(repo, &["remote", "add", "origin", &origin.display().to_string()]);
    git(repo, &["fetch", "-q", "origin"]);
    let branch = git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
    git(repo, &["branch", "-q", "-u", &format!("origin/{branch}")]);
    let extra = COMMIT_LIMIT + 1;
    for index in 0..extra {
        fs::write(repo.join("src").join(format!("c{index}.rs")), "// c\n").unwrap_or_else(|err| panic!("write: {err}"));
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-q", "-m", &format!("commit {index}\n\nbody")]);
    }
    let head = git(repo, &["rev-parse", "--short", "HEAD"]);
    // worktree: clean なもの 1 つと dirty なもの 1 つ（anchor の中）。anchor 自身は untracked で dirty。
    git(repo, &["worktree", "add", "-q", "-b", "wt-clean", &repo.join(".wt").join("clean").display().to_string()]);
    git(repo, &["worktree", "add", "-q", "-b", "wt-dirty", &repo.join(".wt").join("dirty").display().to_string()]);
    fs::write(repo.join(".wt").join("dirty").join("stray.txt"), "x\n").unwrap_or_else(|err| panic!("write: {err}"));
    fs::write(repo.join("untracked.txt"), "x\n").unwrap_or_else(|err| panic!("write: {err}"));
    let (_, recent) = brief_and_recent(&place, &path, &place.bd);
    let git_line = lines_of(&recent, Kind::Git);
    assert_eq!(git_line, vec![&format!("[RECENT-GIT] head={head} branch={branch} ahead={extra} behind=0")], "{recent:?}");
    let commits = lines_of(&recent, Kind::Commit);
    assert_eq!(commits.len(), COMMIT_LIMIT, "上限まで: {recent:?}");
    assert_eq!(commits[0].as_str(), format!("[RECENT-COMMIT] {head} commit {}", extra - 1), "先頭は head の commit");
    for (index, line) in commits.iter().enumerate() {
        assert!(line.ends_with(&format!(" commit {}", extra - 1 - index)), "新しい順・subject だけ（body は載らない）: {line}");
    }
    let total = extra + 1;
    assert_eq!(
        marked(&recent, recent::MARKER_CUT, Kind::Commit),
        vec![&format!("[RECENT-CUT] kind=commit shown={COMMIT_LIMIT} total={total}")],
        "{recent:?}"
    );
    let dirty: Vec<&str> = lines_of(&recent, Kind::Dirty).iter().map(|line| line.as_str()).collect();
    assert_eq!(dirty, vec!["[RECENT-DIRTY] .", "[RECENT-DIRTY] .wt/dirty"], "anchor と dirty な worktree だけ: {recent:?}");
    assert!(marked(&recent, recent::MARKER_NONE, Kind::Dirty).is_empty(), "{recent:?}");
    // 順序: 種類の宣言順（wip → bead → git → commit → dirty）で並ぶ。
    let first_of = |kind: Kind| {
        recent.iter().position(|line| line.starts_with(kind.marker()) || line.contains(&format!(" kind={}", kind.as_str())))
    };
    let order: Vec<usize> = recent::KINDS.iter().filter_map(|kind| first_of(*kind)).collect();
    assert_eq!(order.len(), recent::KINDS.len(), "6 種類が全部出る: {recent:?}");
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "種類の順: {order:?} {recent:?}");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (e): 登録の無い席（pane は解けるが row が無い）は今と同じく 0 byte（名乗りの 1 行だけ・DATA も出ない・記録も増えない）。
#[test]
fn hook_session_recent_is_silent_for_an_unregistered_target() {
    let place = role_place();
    let path = stub_seat(&place, "recentghost", None);
    let before = inject_lines(&place.state).len();
    let lines = stub_session_lines(&place, &path, &place.bd);
    assert_eq!(lines, Vec::<String>::new(), "登録の無い席は名乗りの後ろが 0 byte: {lines:?}");
    let records = inject_lines(&place.state);
    assert_eq!(records.len(), before + 1, "記録は名乗りの 1 行だけ: {records:?}");
    assert!(records.iter().skip(before).all(|line| what_of(line) == "session-start-header"), "{records:?}");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// 純関数の面（tmux を立てない）: `updated_at` は offset 付き・小数秒付きの RFC 3339 を UTC 秒に読み、読めない形は
/// `None`（0 秒に化けない）。題は制御文字を空白へ畳み幅で切る。
#[test]
fn hook_session_recent_reads_offsets_and_folds_titles_purely() {
    let json = concat!(
        "[{\"id\":\"a\",\"status\":\"open\",\"title\":\"t\",\"updated_at\":\"2026-09-20T09:00:00+09:00\"},",
        "{\"id\":\"b\",\"status\":\"open\",\"title\":\"t\",\"updated_at\":\"2026-09-20T00:00:00.123456789Z\"},",
        "{\"id\":\"c\",\"status\":\"open\",\"updated_at\":\"2026-09-20\"},",
        "{\"id\":\"d\",\"status\":\"open\",\"title\":\"t\",\"updated_at\":\"2026-09-19T23:30:00-00:30\"}]"
    );
    let beads = recent::beads_of(json).unwrap_or_else(|| panic!("読める"));
    let midnight = vessel::fleet::epoch_of("2026-09-20T00:00:00Z").unwrap_or_default();
    assert_eq!(beads[0].updated, Some(midnight), "+09:00 は UTC へ戻す");
    assert_eq!(beads[1].updated, Some(midnight), "小数秒は捨てる");
    assert_eq!(beads[2].updated, None, "日付だけの形は読めない（0 に化けない）");
    assert_eq!(beads[2].title, "", "題が無ければ空");
    assert_eq!(beads[3].updated, Some(midnight), "負の offset");
    assert!(recent::beads_of("[{\"status\":\"open\"}]").is_none(), "id の無い要素は読み飛ばさない（読めた に化けない）");
    assert!(recent::beads_of("{}").is_none(), "配列でない");
    // 畳みと幅。
    let long: String = "あ".repeat(TITLE_WIDTH + 1);
    assert_eq!(recent::fold(&long).chars().count(), TITLE_WIDTH + 1, "幅で切って … を足す");
    assert!(recent::fold(&long).ends_with('…'));
    assert_eq!(recent::fold("  a\x00b\n c \u{7f}"), "a b  c", "制御文字は空白・両端は落とす");
    assert_eq!(recent::fold("[RECENT-WIP] x"), "[RECENT-WIP] x", "題の偽装は 3 語目以降に留まる（行頭にならない）");
}

/// 純関数の面（tmux を立てない）: 窓は `now` を呼び手が渡す（境界は両端を含む・未来は出ない）。BEAD は新しい順・
/// 同時刻は id 順。読めた 0 件は NONE・読めない周は理由付きの UNMEASURED。
#[test]
fn hook_session_recent_windows_and_orders_purely() {
    let midnight = vessel::fleet::epoch_of("2026-09-20T00:00:00Z").unwrap_or_default();
    let now = midnight + WINDOW_SECS;
    let beads = vec![
        recent::Bead { id: "edge".into(), status: "open".into(), title: "端".into(), updated: Some(midnight) },
        recent::Bead { id: "out".into(), status: "open".into(), title: "外".into(), updated: Some(midnight - 1) },
        recent::Bead { id: "future".into(), status: "open".into(), title: "未来".into(), updated: Some(now + 1) },
        recent::Bead { id: "b-now".into(), status: "closed".into(), title: "同時刻 b".into(), updated: Some(now) },
        recent::Bead { id: "a-now".into(), status: "open".into(), title: "同時刻 a".into(), updated: Some(now) },
        recent::Bead { id: "wip".into(), status: "in_progress".into(), title: "仕掛かり".into(), updated: None },
    ];
    let lines = recent::ledger_lines(Ok(&beads), now);
    let expected = [
        "[RECENT-WIP] wip - 仕掛かり",
        "[RECENT-BEAD] a-now open 2026-09-21T00:00:00Z 同時刻 a",
        "[RECENT-BEAD] b-now closed 2026-09-21T00:00:00Z 同時刻 b",
        "[RECENT-BEAD] edge open 2026-09-20T00:00:00Z 端",
    ];
    assert_eq!(lines, expected, "窓の両端を含み・未来と外は出ず・新しい順・同時刻は id 順");
    assert_eq!(recent::ledger_lines(Ok(&[]), now), ["[RECENT-NONE] kind=wip", "[RECENT-NONE] kind=bead"]);
    assert_eq!(
        recent::ledger_lines(Err(Unmeasured::LedgerTimeout), now),
        ["[RECENT-UNMEASURED] kind=wip reason=ledger-timeout", "[RECENT-UNMEASURED] kind=bead reason=ledger-timeout"]
    );
}

/// dirty の走査は worktree の上位 `DIRTY_SCAN_LIMIT` 本（anchor が先頭・残りは HEAD の commit が新しい順）に限り、
/// 上限を超える周は `[RECENT-CUT] kind=dirty shown=<測った本数> total=<worktree の本数>` を末尾に付ける（0 件の
/// NONE の後ろにも付く）＝便ごとの worktree が数百本溜まった repo で hook の時間予算を食い潰さない。古い HEAD の
/// dirty な worktree は走査の外に落ち、新しい HEAD の dirty な worktree は出る。
#[test]
fn hook_session_recent_dirty_scan_is_cut_to_the_newest_worktrees() {
    crate::install_spawner();
    let repo = git_repo();
    // worktree の置き場 `.wt/` は anchor の untracked に数えない（anchor の dirty は `untracked.txt` で作る）。
    fs::write(repo.join(".git").join("info").join("exclude"), ".wt/\n").unwrap_or_else(|err| panic!("write: {err}"));
    let old = add_worktree(&repo, "old");
    fs::write(old.join("stray.txt"), "x\n").unwrap_or_else(|err| panic!("write: {err}"));
    // 新しい commit（committer の時刻を 1 日進める＝1 秒解像度の同時刻にしない）。
    fs::write(repo.join("src").join("b.rs"), "// b\n").unwrap_or_else(|err| panic!("write: {err}"));
    git(&repo, &["add", "-A"]);
    let out = Command::new("git")
        .args(["-C", &repo.display().to_string(), "commit", "-q", "-m", "b"])
        .env("GIT_COMMITTER_DATE", "2030-01-02T00:00:00Z")
        .env("GIT_AUTHOR_DATE", "2030-01-02T00:00:00Z")
        .output()
        .unwrap_or_else(|err| panic!("git: {err}"));
    assert!(out.status.success(), "commit b: {}", String::from_utf8_lossy(&out.stderr));
    let newest: Vec<PathBuf> = (0..DIRTY_SCAN_LIMIT).map(|index| add_worktree(&repo, &format!("b{index}"))).collect();
    fs::write(newest[0].join("stray.txt"), "x\n").unwrap_or_else(|err| panic!("write: {err}"));
    fs::write(repo.join("untracked.txt"), "x\n").unwrap_or_else(|err| panic!("write: {err}"));
    let total = DIRTY_SCAN_LIMIT + 2;
    let lines = recent::git_lines(&repo);
    let dirty: Vec<&String> = lines.iter().filter(|line| line.starts_with("[RECENT-DIRTY] ")).collect();
    assert_eq!(dirty, vec!["[RECENT-DIRTY] .", "[RECENT-DIRTY] .wt/b0"], "anchor と新しい dirty だけ・古い dirty は走査の外: {lines:?}");
    let cut = format!("[RECENT-CUT] kind=dirty shown={DIRTY_SCAN_LIMIT} total={total}");
    assert_eq!(lines.last(), Some(&cut), "上限で切った事実が末尾に載る: {lines:?}");
    assert!(lines.iter().all(|line| !line.contains("kind=dirty reason=")), "測れたので UNMEASURED ではない: {lines:?}");
    // 走査の上限の内側に dirty が無ければ NONE + CUT の 2 行（0 件と切った事実の両方）。
    fs::remove_file(newest[0].join("stray.txt")).unwrap_or_else(|err| panic!("rm: {err}"));
    fs::remove_file(repo.join("untracked.txt")).unwrap_or_else(|err| panic!("rm: {err}"));
    let lines = recent::git_lines(&repo);
    let tail: Vec<&String> = lines.iter().rev().take(2).collect();
    assert_eq!(tail, vec![&cut, &"[RECENT-NONE] kind=dirty".to_owned()], "NONE の後ろに CUT: {lines:?}");
    clean(&[&repo]);
}

/// git の 3 種類の UNMEASURED は 1 回の判定で揃って出る（repo でない dir）。
#[test]
fn hook_session_recent_git_kinds_are_unmeasured_together_outside_a_repo() {
    crate::install_spawner();
    let dir = tmp();
    assert!(!dir.join(".git").exists(), "tmp dir は repo でない");
    assert_eq!(
        recent::git_lines(&dir),
        [
            "[RECENT-UNMEASURED] kind=git reason=not-a-repo",
            "[RECENT-UNMEASURED] kind=commit reason=not-a-repo",
            "[RECENT-UNMEASURED] kind=dirty reason=not-a-repo"
        ]
    );
    clean(&[&dir]);
}

/// (a)(b): PreCompact が transcript の末尾から**直近の assistant の text block**（tool_use だけの行と user の行と JSON で
/// ない行は飛ばす・同じ行の最後の text block）を枠に書き（rc 0・stdout 0 byte・stderr 0 byte・記録 `precompact-slot`）、
/// 続く `source = compact` の SessionStart が指示文 12 行の直後・DATA の前に `[PRECOMPACT] trigger=auto ts=… lines=2` と
/// 逐語の 2 行を出して枠を消し（記録 `session-start-precompact`）、同じ SessionStart をもう 1 回撃つと `[PRECOMPACT]` は
/// 出ない（1 回だけ）。
#[test]
fn hook_precompact_writes_the_slot_and_compact_session_start_emits_it_once() {
    let place = role_place();
    let path = stub_seat(&place, "precomp", Some("orchestrator"));
    let latest = "いま .489.2 の枠を書いている途中。\n次は SessionStart の読み側。";
    let transcript = write_transcript(
        &place,
        "t-a",
        &[
            user_line("始めて"),
            assistant_line(&["古い発言"], true),
            user_line("続けて"),
            assistant_line(&["途中の text", latest], true),
            "not json at all".to_owned(),
            assistant_line(&[], true),
            user_line("[tool_result] 出力"),
        ],
    );
    let before = precompact_records(&place.state).len();
    let parsed = assert_slot_written(&place, &path, "precomp", &transcript, latest);
    let slot = slot_file(&place, "precomp");
    // 読む側: compact の SessionStart が 1 回だけ出して枠を消す。
    let lines = session_lines_with(&place, &path, "compact");
    let section = precompact_section(&lines);
    assert_eq!(section.len(), 3, "header + 逐語の 2 行: {lines:?}");
    assert_eq!(section[0], format!("[PRECOMPACT] trigger=auto ts={} lines=2", vessel::fleet::cli::format_utc(parsed.ts)), "{lines:?}");
    assert_eq!(section[1..], ["いま .489.2 の枠を書いている途中。", "次は SessionStart の読み側。"], "逐語: {lines:?}");
    assert!(!slot.exists(), "出した後に枠は消える");
    let records = precompact_records(&place.state);
    assert_eq!(records.len(), before + 2, "読む側の記録 1 行: {records:?}");
    assert_eq!(what_of(&records[before + 1]), "session-start-precompact", "{records:?}");
    let bytes = section.join("\n").len() as u64 + 1;
    assert_eq!(value_of(&records[before + 1], "bytes"), Some(json_lite::Value::Num(bytes)), "bytes は出した行の byte 数");
    // (b) もう 1 回: 枠は無いので出ない（指示文と DATA は出る）。
    let again = session_lines_with(&place, &path, "compact");
    assert!(precompact_section(&again).is_empty(), "2 回目は出ない: {again:?}");
    assert_eq!(split_recent(again.clone()).0.len(), 12, "指示文は 12 行のまま: {again:?}");
    assert!(again.iter().any(|line| line.starts_with("[RECENT-")), "DATA は出る: {again:?}");
    assert_eq!(precompact_records(&place.state).len(), before + 2, "出さない周は記録も増えない");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (c): `source = startup`（と `resume` / `clear`）の SessionStart は枠を読まず消さない（`[PRECOMPACT]` を出さず・
/// 枠は残る）。その後の `compact` で出る。
#[test]
fn hook_precompact_startup_neither_emits_nor_removes_the_slot() {
    let place = role_place();
    let path = stub_seat(&place, "precompstart", Some("orchestrator"));
    let transcript = write_transcript(&place, "t-c", &[assistant_line(&["手番の途中"], false)]);
    run_precompact(&place, &path, &precompact_payload(&place.repo, "manual", Some(&transcript)));
    let slot = slot_file(&place, "precompstart");
    assert!(slot.is_file(), "枠が在る");
    let written = fs::read_to_string(&slot).unwrap_or_default();
    for source in ["startup", "resume", "clear"] {
        let lines = session_lines_with(&place, &path, source);
        assert!(precompact_section(&lines).is_empty(), "{source} では出ない: {lines:?}");
        assert_eq!(fs::read_to_string(&slot).unwrap_or_default(), written, "{source} では枠を触らない");
    }
    // source の無い payload（旧い Claude Code）も同じ。
    let out = run_stub_hook(&path, &["session-start", "--pane", STUB_PANE, "--rules", &place.rules, "--bd", &place.bd], &stamp_payload(&place.repo, "sid-pc"));
    assert!(precompact_section(&after_header(&out)).is_empty(), "source 無しでは出ない");
    assert!(slot.is_file(), "source 無しでは枠を触らない");
    let lines = session_lines_with(&place, &path, "compact");
    let section = precompact_section(&lines);
    assert_eq!(section.len(), 2, "compact で出る: {lines:?}");
    assert!(section[0].starts_with("[PRECOMPACT] trigger=manual ts="), "{}", section[0]);
    assert_eq!(section[1], "手番の途中");
    assert!(!slot.exists(), "compact で消える");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (d): transcript が無い（path が無い file・payload に key が無い）・assistant の text が 1 つも無い（user の行と
/// tool_use だけの行・JSON でない行）周は枠を書かず rc 0・stdout 0 byte。読めない周だけ stderr 1 行（失敗を黙って
/// 消さない）と記録 `precompact-skip transcript-unreadable`・無い周は黙って記録 `precompact-skip no-text` / `no-transcript`。
/// 続く compact の SessionStart は `[PRECOMPACT]` を出さないだけで指示文と DATA は出す。
#[test]
fn hook_precompact_skips_unreadable_and_text_less_transcripts_without_a_slot() {
    let place = role_place();
    let path = stub_seat(&place, "precompnone", Some("orchestrator"));
    let slot = slot_file(&place, "precompnone");
    let missing = place.sock_dir.join("no-such.jsonl");
    let textless = write_transcript(
        &place,
        "t-d",
        &[user_line("x"), assistant_line(&[], true), "{\"type\":\"assistant\"".to_owned(), assistant_line(&["   \n"], false)],
    );
    let cases: [(&str, Option<&Path>, &str, bool); 3] = [
        ("無い file", Some(&missing), "transcript-unreadable", true),
        ("key 無し", None, "no-transcript", false),
        ("text 無し", Some(&textless), "no-text", false),
    ];
    for (why, transcript, reason, surfaces) in cases {
        let before = precompact_records(&place.state).len();
        let stderr = run_precompact(&place, &path, &precompact_payload(&place.repo, "auto", transcript));
        assert!(!slot.exists(), "{why}: 枠を書かない");
        if surfaces {
            assert_eq!(stderr.lines().count(), 1, "{why}: stderr 1 行: {stderr}");
            assert!(stderr.contains(&format!("reason={reason}")), "{why}: {stderr}");
        } else {
            assert_eq!(stderr, "", "{why}: 失敗ではない＝黙る");
        }
        let records = precompact_records(&place.state);
        assert_eq!(records.len(), before + 1, "{why}: 記録 1 行: {records:?}");
        assert_eq!(what_of(&records[before]), format!("precompact-skip {reason}"), "{why}: {records:?}");
    }
    let lines = session_lines_with(&place, &path, "compact");
    assert!(precompact_section(&lines).is_empty(), "枠が無い compact は出さない: {lines:?}");
    assert_eq!(split_recent(lines.clone()).0.len(), 12, "指示文は出る: {lines:?}");
    assert!(lines.iter().any(|line| line.starts_with("[RECENT-")), "DATA は出る: {lines:?}");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (e): 幅（`TEXT_WIDTH` 文字）を超える文は切られ、切った事実が `[PRECOMPACT]` の行に `cut=<shown>/<total>` で出る
/// （文字で数える＝多 byte を byte で切らない）。幅ちょうどは切らず `cut=` を持たない。
#[test]
fn hook_precompact_cuts_long_text_and_names_the_cut_on_the_line() {
    let place = role_place();
    let path = stub_seat(&place, "precompcut", Some("orchestrator"));
    let long: String = "あ".repeat(TEXT_WIDTH + 50);
    let transcript = write_transcript(&place, "t-e", &[assistant_line(&[&long], false)]);
    run_precompact(&place, &path, &precompact_payload(&place.repo, "auto", Some(&transcript)));
    let lines = session_lines_with(&place, &path, "compact");
    let section = precompact_section(&lines);
    assert_eq!(section.len(), 2, "{lines:?}");
    assert!(section[0].ends_with(&format!(" lines=1 cut={TEXT_WIDTH}/{}", TEXT_WIDTH + 50)), "切った事実: {}", section[0]);
    assert_eq!(section[1].chars().count(), TEXT_WIDTH, "幅で切る（文字）");
    assert_eq!(section[1], "あ".repeat(TEXT_WIDTH), "先頭から逐語");
    // 幅ちょうどは切らない。
    let exact: String = "い".repeat(TEXT_WIDTH);
    let transcript = write_transcript(&place, "t-e2", &[assistant_line(&[&exact], false)]);
    run_precompact(&place, &path, &precompact_payload(&place.repo, "auto", Some(&transcript)));
    let section = precompact_section(&session_lines_with(&place, &path, "compact"));
    assert!(section[0].ends_with(" lines=1"), "切らない周に cut= は無い: {}", section[0]);
    assert_eq!(section[1], exact);
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (f): 登録の無い席（pane は解けるが row が無い）と `--pane` の無い周は枠を書かず（席の dir も作らない）、記録も
/// 増やさない（rc 0・stdout 0 byte・stderr 0 byte）。
#[test]
fn hook_precompact_is_silent_for_an_unregistered_seat() {
    let place = role_place();
    let path = stub_seat(&place, "precompghost", None);
    let transcript = write_transcript(&place, "t-f", &[assistant_line(&["登録の無い席の発言"], false)]);
    let payload = precompact_payload(&place.repo, "auto", Some(&transcript));
    let before = inject_lines(&place.state).len();
    assert_eq!(run_precompact(&place, &path, &payload), "", "stderr 0 byte");
    assert!(!slot_file(&place, "precompghost").exists(), "登録の無い席は枠を書かない");
    let out = run_stub_hook(&path, &["pre-compact", "--rules", &place.rules], &payload);
    assert_silent(&out, "--pane 無し");
    assert_eq!(inject_lines(&place.state).len(), before, "記録は増えない");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// 純関数の面（1 行の読み）: assistant 以外・text の無い block・JSON でない行・配列でない content は `None`・同じ行の
/// 最後の text block を取る・空白だけの text は無いと見る。書かない理由は閉じた enum（3 variant・宣言順）。
#[test]
fn hook_precompact_reads_the_last_text_block_of_a_line_purely() {
    assert_eq!(precompact::text_of_line(&assistant_line(&["a", "b"], true)).as_deref(), Some("b"), "最後の text block");
    assert_eq!(precompact::text_of_line(&assistant_line(&[], true)), None, "tool_use だけ");
    assert_eq!(precompact::text_of_line(&assistant_line(&[" \n"], false)), None, "空白だけ");
    assert_eq!(precompact::text_of_line(&user_line("a")), None, "user の行");
    assert_eq!(precompact::text_of_line("{\"type\":\"assistant\""), None, "JSON でない");
    assert_eq!(precompact::text_of_line("{\"type\":\"assistant\",\"message\":{\"content\":\"str\"}}"), None, "配列でない content");
    let skips: Vec<&str> = precompact::SKIPS.iter().map(|skip| skip.as_str()).collect();
    assert_eq!(skips, ["no-transcript", "transcript-unreadable", "no-text"], "理由の閉じた列（宣言順）");
    assert!(vessel::order::is_declaration_order(precompact::SKIPS, |skip| skip as usize), "宣言順");
    assert!(precompact::Skip::Unreadable.surfaces() && !precompact::Skip::NoText.surfaces(), "読めないだけが失敗");
}

/// 純関数の面（枠の往復）: `to_text` ↔ `parse`（改行入りの逐語・別 schema と壊れた 1 行目は `None`）・trigger の畳み
/// （空白は `_`・空は `-`）・幅を超えた周の `cut=`。
#[test]
fn hook_precompact_round_trips_the_slot_purely() {
    let slot = Slot::capture("man ual", "1 行目\n2 行目\n", 7);
    assert_eq!(slot.trigger, "man_ual");
    assert_eq!((slot.total, slot.shown(), slot.cut()), (10, 10, false));
    assert_eq!(slot.to_text(), "schema=1 trigger=man_ual ts=7 total=10\n1 行目\n2 行目\n");
    assert_eq!(Slot::parse(&slot.to_text()), Some(slot.clone()), "往復");
    assert_eq!(slot.lines(), ["[PRECOMPACT] trigger=man_ual ts=1970-01-01T00:00:07Z lines=2", "1 行目", "2 行目"]);
    assert_eq!(Slot::capture("", "x", 0).trigger, "-", "空の trigger は -");
    let cut = Slot::capture("auto", &"x".repeat(TEXT_WIDTH + 1), 0);
    assert!(cut.cut() && cut.header().ends_with(&format!(" cut={TEXT_WIDTH}/{}", TEXT_WIDTH + 1)), "{}", cut.header());
    for broken in ["schema=2 trigger=a ts=1 total=1\nx", "schema=1 trigger=a ts=x total=1\nx", "schema=1 trigger=a ts=1 total=1", ""] {
        assert_eq!(Slot::parse(broken), None, "{broken:?}");
    }
}

/// 純関数の面（末尾だけの読み）: `TAIL_BYTES` を超える transcript は全読せず、途中から読んだ周は欠けた先頭の行を
/// 捨てて末尾の assistant の text を取る。無い file は `Unreadable`・path 無しは `NoTranscript`。
#[test]
fn hook_precompact_reads_only_the_tail_of_the_transcript() {
    let dir = tmp();
    let file = dir.join("big.jsonl");
    let filler = user_line(&"f".repeat(4_096));
    let mut lines: Vec<String> = vec![assistant_line(&["古い"], false)];
    let count = usize::try_from(precompact::TAIL_BYTES).unwrap_or_default() / filler.len() + 2;
    lines.extend(std::iter::repeat_n(filler, count));
    lines.push(assistant_line(&["末尾の発言"], false));
    fs::write(&file, format!("{}\n", lines.join("\n"))).unwrap_or_else(|err| panic!("write: {err}"));
    assert_eq!(precompact::last_text(Some(&file)).as_deref(), Ok("末尾の発言"));
    let tail = precompact::tail_of(&file).unwrap_or_default();
    assert!(u64::try_from(tail.len()).unwrap_or(u64::MAX) < precompact::TAIL_BYTES, "全読しない: {}", tail.len());
    assert!(tail.starts_with('{') && !tail.contains("古い"), "欠けた先頭の行を捨てる");
    assert_eq!(precompact::last_text(Some(&dir.join("none.jsonl"))), Err(precompact::Skip::Unreadable));
    assert_eq!(precompact::last_text(None), Err(precompact::Skip::NoTranscript));
    clean(&[&dir]);
}

/// §23 (1) 台帳の子 process の終わり方: (a) JSON を出した後 rc 非 0 で終わる偽の `bd` は `ledger-unreadable`（出力が読めても
/// rc を見る・件数の 1 行も `unknown`）／(b) stdout を閉じた後も待ち上限を越えて生き続ける偽の `bd` は `ledger-timeout`
/// （`ledger-unreadable` と分ける）／(c) stdout を閉じた後、上限の内側で少し遅れて rc 0 で終わる偽の `bd` は測れた側
/// （`[RECENT-WIP]` と件数の 1 行）に出る。待ち上限は fixture の rules 行で 2 秒に置く。
#[test]
fn hook_recovery_edge_ledger_child_exit_code_and_lingering_are_told_apart() {
    // flip-check: retroactive s2-07l.489.3
    let place = role_place();
    let path = stub_seat(&place, "edgeledger", Some("orchestrator"));
    let rules = rules_with_ledger_timeout(&place, 2);
    let json = "[{\"id\":\"w-1\",\"status\":\"in_progress\",\"title\":\"wip\"}]";
    let nonzero = fake_bd_script(&place, "bd-rc", &format!("echo '{json}'\nexit 3\n"));
    let lingering = fake_bd_script(&place, "bd-linger", &format!("echo '{json}'\nexec 1>&-\nsleep 8\nexit 0\n"));
    let late = fake_bd_script(&place, "bd-late", &format!("echo '{json}'\nexec 1>&-\nsleep 0.2\nexit 0\n"));
    let cases = [("JSON の後に rc 3", nonzero.as_str(), "ledger-unreadable"), ("stdout を閉じて上限を越えて生きる", lingering.as_str(), "ledger-timeout")];
    for (why, bd, reason) in cases {
        let (brief, recent) = brief_and_recent_with_rules(&place, &path, &rules, bd);
        assert!(brief.iter().any(|line| line.contains("unknown（台帳を読めない）")), "{why}: 件数も unknown: {brief:?}");
        for kind in [Kind::Wip, Kind::Bead] {
            let expected = format!("[RECENT-UNMEASURED] kind={} reason={reason}", kind.as_str());
            assert_eq!(marked(&recent, recent::MARKER_UNMEASURED, kind), vec![&expected], "{why}: {recent:?}");
            assert!(marked(&recent, recent::MARKER_NONE, kind).is_empty(), "{why}: 測れない周に NONE は出ない: {recent:?}");
        }
        assert!(recent.iter().all(|line| !line.contains("w-1")), "{why}: 出力が読めても採らない: {recent:?}");
        assert_eq!(lines_of(&recent, Kind::Git).len(), 1, "{why}: git の行は出る: {recent:?}");
    }
    let (brief, recent) = brief_and_recent_with_rules(&place, &path, &rules, &late);
    assert!(brief.iter().any(|line| line.contains("open=0 in_progress=1 blocked=0")), "上限の内側で終わった周は数える: {brief:?}");
    let wip: Vec<&str> = lines_of(&recent, Kind::Wip).iter().map(|line| line.as_str()).collect();
    assert_eq!(wip, vec!["[RECENT-WIP] w-1 - wip"], "上限の内側で遅れて rc 0 は測れた側: {recent:?}");
    assert!(marked(&recent, recent::MARKER_UNMEASURED, Kind::Wip).is_empty(), "{recent:?}");
    assert!(marked(&recent, recent::MARKER_UNMEASURED, Kind::Bead).is_empty(), "{recent:?}");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// §23 (2) worktree を測る順: 列挙の順（`git worktree list`）と HEAD の commit の新しい順が**食い違う** dirty な worktree 2 本
/// を作り、`[RECENT-DIRTY]` の行が anchor の後ろに commit の新しい順で並ぶ。commit を 1 つも持たない（未生の HEAD の）
/// dirty な worktree を混ぜても 2 本の順は変わらず、その worktree は末尾に来る（時刻を引けない側＝列挙の順に落ちない）。
#[test]
fn hook_recovery_edge_dirty_worktrees_follow_commit_recency_not_enumeration() {
    // flip-check: retroactive s2-07l.489.3
    let place = role_place();
    let path = stub_seat(&place, "edgeorder", Some("orchestrator"));
    let repo = &place.repo;
    let first = add_worktree(repo, "wt-a");
    let second = add_worktree(repo, "wt-b");
    // 列挙の順を実測し、先に列挙される方に古い commit・後に列挙される方に新しい commit を置く＝順が食い違う。
    let listed = listed_worktrees(repo);
    assert_eq!(listed.len(), 3, "anchor + 2 本: {listed:?}");
    let first_listed = listed[1].ends_with("/wt-a");
    let (older, newer) = if first_listed { (&first, &second) } else { (&second, &first) };
    commit_dated(older, "older", "2020-01-02T00:00:00Z");
    commit_dated(newer, "newer", "2030-01-02T00:00:00Z");
    fs::write(older.join("stray.txt"), "x\n").unwrap_or_else(|err| panic!("write: {err}"));
    fs::write(newer.join("stray.txt"), "x\n").unwrap_or_else(|err| panic!("write: {err}"));
    let unborn = repo.join(".wt").join("unborn");
    git(repo, &["worktree", "add", "-q", "--orphan", "-b", "unborn", &unborn.display().to_string()]);
    fs::write(unborn.join("stray.txt"), "x\n").unwrap_or_else(|err| panic!("write: {err}"));
    let listed = listed_worktrees(repo);
    assert_eq!(listed.len(), 4, "anchor + 3 本: {listed:?}");
    let porcelain = git(repo, &["worktree", "list", "--porcelain"]);
    assert!(porcelain.contains("HEAD 0000000000000000000000000000000000000000"), "未生の HEAD は全部 0: {porcelain}");
    let rel = |path: &Path| format!("[RECENT-DIRTY] {}", path.strip_prefix(repo).unwrap_or(path).display());
    let expected = vec!["[RECENT-DIRTY] .".to_owned(), rel(newer), rel(older), rel(&unborn)];
    let enumerated: Vec<String> = listed.iter().skip(1).map(|path| rel(Path::new(path))).collect();
    assert_ne!(enumerated, expected[1..].to_vec(), "前提: 列挙の順は期待の順と食い違う: {listed:?}");
    let (_, recent) = brief_and_recent(&place, &path, &place.bd);
    let dirty: Vec<String> = lines_of(&recent, Kind::Dirty).iter().map(|line| (*line).clone()).collect();
    assert_eq!(dirty, expected, "anchor → 新しい HEAD → 古い HEAD → 未生の HEAD: {recent:?}");
    assert!(marked(&recent, recent::MARKER_CUT, Kind::Dirty).is_empty(), "上限の内側: {recent:?}");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// §23 (3) 上限とちょうど同じ件数: commit の本数が `COMMIT_LIMIT` とちょうど同じ repo は `[RECENT-CUT] kind=commit` を出さず
/// 全 commit が出る。worktree の本数が `DIRTY_SCAN_LIMIT` とちょうど同じ repo は `[RECENT-CUT] kind=dirty` を出さない。
/// 対として 1 本ずつ足すとどちらの CUT も `shown=<上限> total=<上限 + 1>` で出る（境界は「超えた」側だけ）。
#[test]
fn hook_recovery_edge_counts_exactly_at_the_limit_are_not_cut() {
    // flip-check: retroactive s2-07l.489.3
    let place = role_place();
    let path = stub_seat(&place, "edgelimit", Some("orchestrator"));
    let repo = &place.repo;
    for index in 1..COMMIT_LIMIT {
        fs::write(repo.join("src").join(format!("c{index}.rs")), "// c\n").unwrap_or_else(|err| panic!("write: {err}"));
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-q", "-m", &format!("commit {index}")]);
    }
    assert_eq!(git(repo, &["rev-list", "--count", "HEAD"]), COMMIT_LIMIT.to_string(), "seed + 追加 = 上限ちょうど");
    for index in 1..DIRTY_SCAN_LIMIT {
        add_worktree(repo, &format!("w{index}"));
    }
    assert_eq!(listed_worktrees(repo).len(), DIRTY_SCAN_LIMIT, "anchor + 追加 = 上限ちょうど");
    let (_, recent) = brief_and_recent(&place, &path, &place.bd);
    assert_eq!(lines_of(&recent, Kind::Commit).len(), COMMIT_LIMIT, "全 commit が出る: {recent:?}");
    assert!(marked(&recent, recent::MARKER_CUT, Kind::Commit).is_empty(), "上限ちょうどは切らない: {recent:?}");
    let dirty: Vec<&str> = lines_of(&recent, Kind::Dirty).iter().map(|line| line.as_str()).collect();
    assert_eq!(dirty, vec!["[RECENT-DIRTY] ."], "anchor だけが dirty（worktree は clean）: {recent:?}");
    assert!(marked(&recent, recent::MARKER_CUT, Kind::Dirty).is_empty(), "上限ちょうどは切らない: {recent:?}");
    // 対: 1 本ずつ超えるとどちらも切る。
    fs::write(repo.join("src").join("over.rs"), "// over\n").unwrap_or_else(|err| panic!("write: {err}"));
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "over"]);
    add_worktree(repo, "over");
    let (_, recent) = brief_and_recent(&place, &path, &place.bd);
    let commit_cut = format!("[RECENT-CUT] kind=commit shown={COMMIT_LIMIT} total={}", COMMIT_LIMIT + 1);
    assert_eq!(marked(&recent, recent::MARKER_CUT, Kind::Commit), vec![&commit_cut], "{recent:?}");
    let dirty_cut = format!("[RECENT-CUT] kind=dirty shown={DIRTY_SCAN_LIMIT} total={}", DIRTY_SCAN_LIMIT + 1);
    assert_eq!(marked(&recent, recent::MARKER_CUT, Kind::Dirty), vec![&dirty_cut], "{recent:?}");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// §23 (4) 時刻の字: 時差の字が 2 桁でない `updated_at`（`+9:00`・`+09:0`）を持つ in_progress の bead は `[RECENT-WIP]` に
/// 更新時刻の値 `-` で出て（時刻を読めない側・9 時間ずれた時刻に化けない）、同じ字の open の bead は 24 時間の窓に
/// 入らない。対として同じ時刻を `+09:00` で持つ open の bead は窓に入る（字の桁だけが違う）。
#[test]
fn hook_recovery_edge_offset_without_two_digits_is_an_unreadable_time() {
    // flip-check: retroactive s2-07l.489.3
    let place = role_place();
    let path = stub_seat(&place, "edgeoffset", Some("orchestrator"));
    let now = vessel::seat::state::now_secs();
    let stamp = vessel::fleet::cli::format_utc(now - 60);
    assert!(stamp.ends_with('Z'), "{stamp}");
    let odd_hour = stamp.replace('Z', "+9:00");
    let odd_minute = stamp.replace('Z', "+09:0");
    let two_digits = stamp.replace('Z', "+09:00");
    let bead = |id: &str, status: &str, title: &str, at: &str| {
        format!("{{\"id\":\"{id}\",\"status\":\"{status}\",\"title\":\"{title}\",\"updated_at\":\"{at}\"}}")
    };
    let items = [
        bead("w-odd", "in_progress", "時差が 1 桁", &odd_hour),
        bead("o-odd", "open", "同じ字の open", &odd_hour),
        bead("o-min", "open", "分が 1 桁", &odd_minute),
        bead("o-ok", "open", "2 桁の対", &two_digits),
    ];
    let bd = fake_bd_in(&place, "ledger-offset", &format!("[{}]", items.join(",")));
    let (brief, recent) = brief_and_recent(&place, &path, &bd);
    assert!(brief.iter().any(|line| line.contains("open=3 in_progress=1 blocked=0")), "件数は字に依らない: {brief:?}");
    let wip: Vec<&str> = lines_of(&recent, Kind::Wip).iter().map(|line| line.as_str()).collect();
    assert_eq!(wip, vec!["[RECENT-WIP] w-odd - 時差が 1 桁"], "読めない時刻は -: {recent:?}");
    let shifted = vessel::fleet::cli::format_utc(now - 60 - 9 * 3_600);
    let beads: Vec<&str> = lines_of(&recent, Kind::Bead).iter().map(|line| line.as_str()).collect();
    assert_eq!(beads, vec![format!("[RECENT-BEAD] o-ok open {shifted} 2 桁の対")], "2 桁の字だけが窓に入る: {recent:?}");
    assert!(recent.iter().all(|line| !line.contains("o-odd") && !line.contains("o-min")), "2 桁でない字は窓に入らない: {recent:?}");
    assert!(marked(&recent, recent::MARKER_UNMEASURED, Kind::Bead).is_empty(), "読めた周: {recent:?}");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// §23 (5) 枠が「無い」以外の理由で読めない・消せない周: 席の置き場の枠の名前が dir になっている周の `source = compact` の
/// SessionStart は `[PRECOMPACT]` を出さず、stderr に読めない理由（`slot-unreadable`）の 1 行と消せない理由の 1 行を出し、
/// §5 の指示文 12 行と §21 の DATA は出す（rc 0・記録は増えない・dir は残る）。枠が無い普通の周は stderr にどちらの行も
/// 出さない（読めないと無いを分ける・C10.2）。
#[test]
fn hook_recovery_edge_compact_with_a_directory_slot_tells_both_failures_and_keeps_the_rest() {
    // flip-check: retroactive s2-07l.489.3
    let place = role_place();
    let path = stub_seat(&place, "edgeslotdir", Some("orchestrator"));
    let slot = slot_file(&place, "edgeslotdir");
    fs::create_dir_all(&slot).unwrap_or_else(|err| panic!("mkdir: {err}"));
    let before = precompact_records(&place.state).len();
    let args = ["session-start", "--pane", STUB_PANE, "--rules", &place.rules, "--bd", &place.bd];
    let out = run_stub_hook(&path, &args, &session_payload(&place.repo, "sid-pc", "compact"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "席は止めない: {}", stderr_text(&out));
    let stderr = stderr_text(&out);
    let errs: Vec<&str> = stderr.lines().collect();
    assert_eq!(errs.len(), 2, "読めない理由と消せない理由の 2 行: {stderr}");
    assert!(errs[0].contains("reason=slot-unreadable"), "読めない理由: {}", errs[0]);
    assert!(errs[1].contains("を消せない") && errs[1].contains(&slot.display().to_string()), "消せない理由: {}", errs[1]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut lines = stdout.lines();
    assert!(lines.next().unwrap_or_default().starts_with(&format!("[{NAME}/SessionStart] served version=")), "{stdout}");
    let rest: Vec<String> = lines.map(str::to_owned).collect();
    assert!(precompact_section(&rest).is_empty(), "枠は出ない: {rest:?}");
    let (brief, recent) = split_recent(rest);
    assert_eq!(brief.len(), 12, "指示文は出る: {brief:?}");
    assert!(!recent.is_empty() && recent.iter().all(|line| line.starts_with("[RECENT-")), "DATA は出る: {recent:?}");
    assert!(slot.is_dir(), "消せない枠は残る");
    assert_eq!(precompact_records(&place.state).len(), before, "出さない周は記録も増えない");
    // 枠が無い普通の周: dir を退けて同じ compact を撃つと stderr は 0 byte（どちらの行も出ない）。
    fs::remove_dir_all(&slot).unwrap_or_else(|err| panic!("rmdir: {err}"));
    let out = run_stub_hook(&path, &args, &session_payload(&place.repo, "sid-pc", "compact"));
    let rest = after_header(&out);
    assert!(precompact_section(&rest).is_empty(), "枠が無い compact は出さない: {rest:?}");
    assert_eq!(split_recent(rest).0.len(), 12, "指示文は出る");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// §23 (6) socket を渡した周の席の解決: PreCompact の口に `--tmux-socket` を渡した周も登録済みの席として枠を書き（記録
/// `precompact-slot` が席を名乗る）、続く socket 付きの `source = compact` の SessionStart がその枠を出す。対として
/// socket を渡さない周は（socket を要る偽 tmux では）席が解けず枠を書かない＝socket が解決に渡った証拠。
#[test]
fn hook_recovery_edge_precompact_with_a_socket_resolves_the_registered_seat() {
    // flip-check: retroactive s2-07l.489.3
    let place = role_place();
    let name = "edgesock";
    let path = socket_stub_tmux_path(&place, name, &place.socket);
    let stamp = run_stub_hook(&path, &["session-start", "--pane", STUB_PANE, "--tmux-socket", &place.socket], &stamp_payload(&place.repo, "sid-recent"));
    assert_eq!(stamp.status.code(), Some(i32::from(RC_OK)), "打刻の session-start は rc 0: {}", stderr_text(&stamp));
    let target = format!("{name}:{name}");
    let registered = Command::new(bin())
        .args(["seat", "register", "--state-dir", &place.state.display().to_string(), "--target", &target])
        .args(["--role", "orchestrator", "--account", "a1", "--launch", &place.launch, "--anchor", &place.repo.display().to_string()])
        .output()
        .unwrap_or_else(|err| panic!("binary: {err}"));
    assert_eq!(registered.status.code(), Some(i32::from(RC_OK)), "seat register は rc 0: {}", stderr_text(&registered));
    let text = "socket 付きの席の発言";
    let transcript = write_transcript(&place, "t-sock", &[assistant_line(&[text], false)]);
    let payload = precompact_payload(&place.repo, "auto", Some(&transcript));
    let slot = slot_file(&place, name);
    let with_socket = ["pre-compact", "--pane", STUB_PANE, "--tmux-socket", &place.socket, "--rules", &place.rules];
    let before = precompact_records(&place.state).len();
    let out = run_stub_hook(&path, &with_socket, &payload);
    assert_silent(&out, "socket 付きの pre-compact");
    let parsed = Slot::parse(&fs::read_to_string(&slot).unwrap_or_default()).unwrap_or_else(|| panic!("枠の形"));
    assert_eq!(parsed.text, text, "socket を渡した周も登録済みの席として枠を書く");
    let records = precompact_records(&place.state);
    assert_eq!(records.len(), before + 1, "記録 1 行: {records:?}");
    assert_eq!(what_of(&records[before]), "precompact-slot", "{records:?}");
    assert_attributed(&records[before], Some(&format!("{name}_{name}")), "枠の記録");
    // 対: socket を渡さない周はこの偽 tmux では席が解けない＝枠を書かず記録も増えない。
    fs::remove_file(&slot).unwrap_or_else(|err| panic!("rm: {err}"));
    let out = run_stub_hook(&path, &["pre-compact", "--pane", STUB_PANE, "--rules", &place.rules], &payload);
    assert_silent(&out, "socket 無しの pre-compact");
    assert!(!slot.exists(), "socket 無しでは席が解けず枠を書かない");
    assert_eq!(precompact_records(&place.state).len(), before + 1, "記録も増えない");
    // 読む側も socket 付きで同じ席を解く。
    assert_silent(&run_stub_hook(&path, &with_socket, &payload), "socket 付きの pre-compact（2 回目）");
    let args = ["session-start", "--pane", STUB_PANE, "--tmux-socket", &place.socket, "--rules", &place.rules, "--bd", &place.bd];
    let lines = after_header(&run_stub_hook(&path, &args, &session_payload(&place.repo, "sid-pc", "compact")));
    let section = precompact_section(&lines);
    assert_eq!(section.len(), 2, "header + 逐語の 1 行: {lines:?}");
    assert_eq!(section[1], text);
    assert!(!slot.exists(), "出した後に枠は消える");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

// ─────────────────── 発話の記帳（接頭辞 `hook_utterance_record_`・設計 fleet-event-log.md §13・ADR-0083 / ADR-0087） ───────────────────
//
// `user-prompt-submit` は user の prompt を model が読む前に `UtteranceReceived` として 1 件記帳し、stdout に ts の 1 行を返す。
// 差し込みの行・別の session の包み・runner は記帳せず、書けない周も prompt を止めない（rc 0）。どの歯も先に普通の prompt を
// 1 件記帳させる対照を置く（除外や不発だけの歯は base でも緑になるため）。

/// 逐語（改行・`"`・非 ASCII を含み、ts と `NAME` に無い字で作る＝stdout に逐語が載ったら見分けられる）。
const UTTER_WORDS: &str = "進めて\n\"承認\" です 🙆";

/// `session_id` と `prompt` を（在れば）持つ `UserPromptSubmit` の payload（逐語は JSON の escape を通す）。
fn utter_payload(cwd: &Path, sid: Option<&str>, prompt: Option<&str>) -> String {
    let mut pairs = vec![format!("\"cwd\":{}", json_lite::quote(&cwd.display().to_string()))];
    pairs.extend(sid.map(|found| format!("\"session_id\":{}", json_lite::quote(found))));
    pairs.extend(prompt.map(|words| format!("\"prompt\":{}", json_lite::quote(words))));
    format!("{{{}}}", pairs.join(","))
}

/// `user-prompt-submit` を（pane 無しで）撃つ。
fn utter(repo: &Path, args: &[&str], sid: Option<&str>, prompt: Option<&str>) -> Output {
    let mut all = vec!["user-prompt-submit"];
    all.extend(args);
    run_hook_args(&all, &utter_payload(repo, sid, prompt))
}

/// 普通の prompt を 1 件撃つ（対照・session は `sid-utt`）。
fn utter_plain(repo: &Path, args: &[&str], prompt: &str) -> Output {
    utter(repo, args, Some("sid-utt"), Some(prompt))
}

/// event log の発話 event。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn utterances(state: &Path) -> Vec<vessel::fleet::Event> {
    let events = vessel::fleet::store::read_all(state).expect("log を読める");
    events.into_iter().filter(|event| event.kind == vessel::fleet::EventKind::UtteranceReceived).collect()
}

/// stdout の行。
fn stdout_lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect()
}

/// 撃った回数を数える偽の台帳 client（引数に依らず `body` を stdout へ出し、呼ばれるたびに `log` へ 1 行足す）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn counting_bd(dir: &Path, body: &str) -> (String, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let (json, log, path) = (dir.join("bd.json"), dir.join("bd.calls"), dir.join("bd"));
    fs::write(&json, body).expect("台帳の fixture を書ける");
    fs::write(&path, format!("#!/bin/sh\necho called >> \"{}\"\ncat \"{}\"\n", log.display(), json.display())).expect("偽の bd を書ける");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("実行権を付けられる");
    (path.display().to_string(), log)
}

/// 記帳できた周の外形（rc 0・stderr 0 byte・stdout は `<NAME> utterance: ts=<ts>` の 1 行）と、その ts。
fn assert_recorded(out: &Output, why: &str) -> String {
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{why}: rc 0: {}", stderr_text(out));
    assert_eq!(stderr_text(out), "", "{why}: stderr 0 byte");
    let lines = stdout_lines(out);
    let prefix = format!("{NAME} utterance: ts=");
    let ts = lines.first().and_then(|line| line.strip_prefix(&prefix)).map(str::to_owned);
    assert!(lines.len() == 1 && ts.is_some(), "{why}: stdout は ts の 1 行: {lines:?}");
    ts.unwrap_or_default()
}

/// (a) 開いた問いが在る時と無い時の 2 本で、発話 event が 1 件ずつ: 逐語は 1 byte も変わらず、session と経路 chat・actor human・
/// run の key 無し、bd は 0 回、stdout は ts の 1 行（ミリ秒の字面で event と同じ・逐語の字を載せない）。
#[test]
fn hook_utterance_record_writes_one_verbatim_event_and_never_calls_the_ledger() {
    for (form, ledger) in [("開いた問い有り", r#"[{"id":"q-1","status":"open"}]"#), ("問い無し", "[]")] {
        let (repo, aux) = (git_repo(), tmp());
        let state = linked(&repo);
        let (bd, calls) = counting_bd(&aux, ledger);
        let out = utter_plain(&repo, &["--bd", &bd], UTTER_WORDS);
        let ts = assert_recorded(&out, form);
        let events = utterances(&state);
        assert_eq!(events.len(), 1, "{form}: 発話 event は 1 件");
        let event = events.first().unwrap_or_else(|| panic!("{form}: 1 件目"));
        assert_eq!(event.detail.as_deref(), Some(UTTER_WORDS), "{form}: 逐語は 1 byte も変わらない");
        let case = vessel::fleet::Case::Utterance { channel: vessel::fleet::Channel::Chat, session: Some("sid-utt".to_owned()) };
        assert_eq!(event.case, Some(case), "{form}: 経路 chat と session");
        assert_eq!((event.actor.as_str(), event.run.as_str()), ("human", ""), "{form}: actor と run");
        let raw = fs::read_to_string(vessel::fleet::store::events_path(&state)).unwrap_or_default();
        assert!(!raw.contains("\"run\""), "{form}: run の key を書かない: {raw}");
        assert_eq!(event.ts, ts, "{form}: stdout の ts と event の ts が同じ");
        assert_eq!(ts.len(), "YYYY-MM-DDTHH:MM:SS.mmmZ".len(), "{form}: ミリ秒 3 桁の字面: {ts}");
        assert!(vessel::fleet::epoch_ms_of(&ts).is_some(), "{form}: ミリ秒の字面として読める: {ts}");
        let shown = String::from_utf8_lossy(&out.stdout);
        assert!(!shown.contains("承認") && !shown.contains("進めて") && !shown.contains('"'), "{form}: stdout に逐語を載せない: {shown}");
        assert!(!calls.exists(), "{form}: bd は 0 回");
        clean(&[&repo, &aux, &state]);
    }
}

/// (b) 差し込みの 9 種の見本・包みの 5 つの頭の見本（どれも後ろに本文の行を持つ）・runner の plugin-root は、どれも記帳 0・
/// stdout も stderr も 0 byte。対照の普通の prompt は 1 件記帳される。
#[test]
fn hook_utterance_record_skips_injected_lines_wrappers_and_runners() {
    let repo = git_repo();
    let state = linked(&repo);
    assert_recorded(&utter_plain(&repo, &[], "普通の依頼"), "対照");
    let injected = [
        format!("{NAME} pipe: b-1 run-1 Landed=ok — 次の 1 手は pipe dispatch ls"),
        format!("{NAME} pipe: idle ready=1 launched=0 reason=-"),
        format!("{NAME} pipe: precheck bundles=0 rows=0"),
        format!("{NAME} seat: 裁定 r-1 が届いた（在りかは裁定面の記帳）"),
        format!("{NAME} tick: heartbeat step=0 — 台帳の現在地から続きを進める"),
        format!("{NAME} seat: relaunch — 台帳の現在地から続きを進める"),
        format!("{NAME} group: evacuate group=Tier1 to=a2 — 新しい subagent を起こさない"),
        format!("{NAME} group: move-refused group=Tier1 reason=no-candidate"),
        format!("{NAME} group: pressure group=Tier1 account=a1 window=5h used=90 cap=85"),
    ];
    let wrappers = [
        "Another Claude session sent a message:",
        "<cross-session-message from=x>",
        "<teammate-message teammate_id=x>",
        "<task-notification>",
        "This session is being continued from a previous conversation that ran out of context.",
    ];
    assert_eq!((injected.len(), wrappers.len()), (9, 5), "母集団");
    for head in injected.iter().map(String::as_str).chain(wrappers) {
        let out = utter_plain(&repo, &[], &format!("{head}\n本文の行\n二行目"));
        assert_silent(&out, head);
    }
    let plugin = state.join("pipe").join("run-1").join("plugin");
    assert_silent(&utter_plain(&repo, &["--plugin-root", &plugin.display().to_string()], "runner の prompt"), "runner の plugin-root");
    assert_eq!(utterances(&state).len(), 1, "記帳は対照の 1 件だけ");
    clean(&[&repo, &state]);
}

/// (b2) 除きすぎない: 語が閉じた 4 語の外・`NAME` でない名・包みの頭の字が 2 行目・`pipe` の dir の外の plugin-root の 4 形は
/// 1 件ずつ記帳され、先頭に空白を置いた包みは除かれる。
#[test]
fn hook_utterance_record_does_not_skip_lookalikes() {
    let repo = git_repo();
    let state = linked(&repo);
    let beside = state.join("pipe2").join("plugin").display().to_string();
    let lookalikes: [(&str, &[&str], String); 4] = [
        ("閉じた語の外", &[], format!("{NAME} note: 覚え書き")),
        ("別の名", &[], "other-name pipe: idle".to_owned()),
        ("包みの頭が 2 行目", &[], "依頼\n<task-notification>".to_owned()),
        ("pipe の dir の外の plugin-root", &["--plugin-root", &beside], "隣の dir".to_owned()),
    ];
    for (count, (why, args, prompt)) in lookalikes.iter().enumerate() {
        assert_recorded(&utter_plain(&repo, args, prompt), why);
        assert_eq!(utterances(&state).len(), count + 1, "{why}: 1 件記帳される");
    }
    assert_silent(&utter_plain(&repo, &[], "  \n<task-notification>\n本文"), "先頭に空白を置いた包み");
    assert_eq!(utterances(&state).len(), 4, "空白つきの包みは記帳されない");
    clean(&[&repo, &state]);
}

/// (b3) 同じ session の係の知らせ（頭が `<agent-message`）は、先頭に空白を置いても記帳 0・出力 0 byte。本文の 2 行目に
/// `<agent-message` を置いた prompt は 1 件記帳される。
#[test]
fn hook_utterance_record_skips_agent_message_() {
    let repo = git_repo();
    let state = linked(&repo);
    assert_recorded(&utter_plain(&repo, &[], "対照"), "対照");
    for (why, prompt) in [
        ("頭が agent-message", "<agent-message from=\"x\">\n本文の行"),
        ("先頭に空白を置いた agent-message", "  \n<agent-message from=\"x\">\n本文の行"),
    ] {
        assert_silent(&utter_plain(&repo, &[], prompt), why);
    }
    assert_eq!(utterances(&state).len(), 1, "記帳は対照の 1 件だけ");
    assert_recorded(&utter_plain(&repo, &[], "依頼\n<agent-message from=\"x\">"), "2 行目の agent-message");
    assert_eq!(utterances(&state).len(), 2, "2 行目の agent-message は 1 件記帳される");
    clean(&[&repo, &state]);
}

/// (c) marker の無い repo と別の `NAME` の repo は記帳 0・出力 0 byte。対照の仕える repo は 1 件記帳される。
#[test]
fn hook_utterance_record_is_silent_outside_a_served_repo() {
    let (bare, other, served) = (git_repo(), git_repo(), git_repo());
    let state = linked(&served);
    let foreign = Marker { name: "other-vessel".to_owned(), version: GENERATION };
    fs::write(other.join(MARKER), foreign.render()).unwrap_or_else(|err| panic!("marker: {err}"));
    let flag = state.display().to_string();
    assert_recorded(&utter_plain(&served, &["--state-dir", &flag], "対照"), "仕える repo");
    assert_eq!(utterances(&state).len(), 1, "対照は 1 件");
    for (why, repo) in [("marker の無い repo", &bare), ("別の器が名乗る repo", &other)] {
        assert_silent(&utter_plain(repo, &["--state-dir", &flag], "依頼"), why);
    }
    assert_eq!(utterances(&state).len(), 1, "仕えない repo は記帳しない");
    clean(&[&bare, &other, &served, &state]);}

/// (d) 続けて撃つと ts が 2 つとも違い、log の順に増える。
#[test]
fn hook_utterance_record_stamps_distinct_increasing_ts() {
    let repo = git_repo();
    let state = linked(&repo);
    let first = assert_recorded(&utter_plain(&repo, &[], "一つ目"), "1 回目");
    let second = assert_recorded(&utter_plain(&repo, &[], "二つ目"), "2 回目");
    assert!(vessel::fleet::epoch_ms_of(&first) < vessel::fleet::epoch_ms_of(&second), "ts は増える: {first} < {second}");
    let logged: Vec<String> = utterances(&state).into_iter().map(|event| event.ts).collect();
    assert_eq!(logged, [first, second], "log の順");
    clean(&[&repo, &state]);
}

/// (e) `pipe report` の human_events は発話の前後で同じ（発話は人由来に数えない・FR22）。
#[test]
fn hook_utterance_record_leaves_the_pipe_report_human_events_unchanged() {
    let repo = git_repo();
    let state = linked(&repo);
    let report = |state: &Path| {
        let out = Command::new(bin()).args(["pipe", "report", "--state-dir", &state.display().to_string()]).output().unwrap_or_else(|err| panic!("binary: {err}"));
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "report は rc 0: {}", stderr_text(&out));
        stdout_lines(&out).into_iter().next().unwrap_or_default()
    };
    let before = report(&state);
    assert!(before.contains("human_events=0 human_events_other_than_approval=0"), "前は 0: {before}");
    assert_recorded(&utter_plain(&repo, &[], "依頼"), "発話");
    assert_eq!(utterances(&state).len(), 1, "発話は 1 件");
    assert_eq!(report(&state), before, "発話の後も同じ");
    clean(&[&repo, &state]);
}

/// (k) 何も書かない 2 形: prompt の key の無い payload と、空白・改行・tab だけの prompt は rc 0・stdout と stderr が 0 byte・記帳 0。
#[test]
fn hook_utterance_record_writes_nothing_without_words() {
    let repo = git_repo();
    let state = linked(&repo);
    assert_recorded(&utter_plain(&repo, &[], "対照"), "対照");
    assert_silent(&utter(&repo, &[], Some("sid-utt"), None), "prompt の key が無い");
    assert_silent(&utter_plain(&repo, &[], " \n\t "), "空白だけ");
    assert_eq!(utterances(&state).len(), 1, "記帳は対照の 1 件だけ");
    clean(&[&repo, &state]);
}

/// (l) 止めない 3 形: session_id の無い周・生きた pid が握る lock（待ちを 50 ms にした rules）・event log の path が dir の置き場は、
/// rc 0・stdout 0 byte・stderr は `<NAME>: utterance unrecorded reason=<語>` の 1 行だけで記帳 0。
#[test]
fn hook_utterance_record_never_stops_the_prompt_when_it_cannot_write() {
    let (repo, aux) = (git_repo(), tmp());
    let state = linked(&repo);
    assert_recorded(&utter_plain(&repo, &[], "対照"), "対照");
    let unrecorded = |out: &Output, reason: &str| {
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{reason}: rc 0");
        assert!(out.stdout.is_empty(), "{reason}: stdout 0 byte");
        assert_eq!(stderr_text(out), format!("{NAME}: utterance unrecorded reason={reason}\n"), "{reason}: stderr の 1 行");
    };
    unrecorded(&utter(&repo, &[], None, Some("session の無い依頼")), "no-session");
    let rules = aux.join("rules.toml");
    let row = |id: &str, kind: &str, value: u64| format!("[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n\n");
    fs::write(&rules, format!("schema = 1\n\n{}{}", row("fleet.lock_retry_ms", "LockRetryMs", 50), row("fleet.lock_stale_ms", "LockStaleMs", 600_000)))
        .unwrap_or_else(|err| panic!("rules: {err}"));
    let lock = vessel::fleet::store::lock_path(&state);
    fs::write(&lock, format!("{}\n", std::process::id())).unwrap_or_else(|err| panic!("lock: {err}"));
    unrecorded(&utter_plain(&repo, &["--rules", &rules.display().to_string()], "lock 待ちの依頼"), "lock");
    fs::remove_file(&lock).unwrap_or_else(|err| panic!("lock を外せる: {err}"));
    assert_eq!(utterances(&state).len(), 1, "no-session と lock の周は記帳しない");
    let events = vessel::fleet::store::events_path(&state);
    fs::remove_file(&events).unwrap_or_else(|err| panic!("log を外せる: {err}"));
    fs::create_dir_all(&events).unwrap_or_else(|err| panic!("log の位置に dir: {err}"));
    unrecorded(&utter_plain(&repo, &[], "書けない依頼"), "write");
    clean(&[&repo, &aux, &state]);
}

// ─────────────────── turn の終わりの止め（接頭辞 `hook_unsorted_stop_`・設計 dialogue-surface.md §12・契約表の行 k） ───────────────────
//
// `session-start` が session の置き場へ event log の長さを開始の位置として 1 度だけ書き、`stop` がそこから末尾までを読んで、その session の
// 未仕分けの発話のうち未告のものが在る周を 1 度だけ rc 2 で止める。発話は `user-prompt-submit`（行 g）、仕分けは `utterance sort`（行 i）で作る。

/// 逐語にだけ在る字（止めの 1 行に載らないことを測る・止めの行の字と `NAME` に無い字で作る）。
const STOP_WORDS: &str = "🦊 秘 Ω の一言";
/// 逐語の字の見本（[`STOP_WORDS`] にだけ在る字）。
const STOP_MARKS: [char; 3] = ['🦊', '秘', 'Ω'];

/// `session_id`（在れば）と `stop_hook_active`（真のときだけ）を持つ payload。
fn stop_payload(cwd: &Path, sid: Option<&str>, active: bool) -> String {
    let mut pairs = vec![format!("\"cwd\":{}", json_lite::quote(&cwd.display().to_string()))];
    pairs.extend(sid.map(|found| format!("\"session_id\":{}", json_lite::quote(found))));
    pairs.extend(active.then(|| "\"stop_hook_active\":true".to_owned()));
    format!("{{{}}}", pairs.join(","))
}

/// `session-start` を session `sid` で撃つ（rc 0 を要求する）。
fn start_session(repo: &Path, args: &[&str], sid: &str) -> Output {
    let mut all = vec!["session-start"];
    all.extend(args);
    let out = run_hook_args(&all, &stop_payload(repo, Some(sid), false));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "session-start は rc 0: {}", stderr_text(&out));
    out
}

/// `stop` を撃つ。
fn stop_hook(repo: &Path, args: &[&str], sid: Option<&str>, active: bool) -> Output {
    let mut all = vec!["stop"];
    all.extend(args);
    run_hook_args(&all, &stop_payload(repo, sid, active))
}

/// session `sid` の発話を 1 件記帳して ts を返す。
fn say_in(repo: &Path, sid: &str, words: &str) -> String {
    assert_recorded(&utter(repo, &[], Some(sid), Some(words)), words)
}

/// 止めた周の外形: rc 2・stdout 0 byte・stderr はちょうど 1 行で `tss` の全部と口の名を持ち、逐語の字を載せない。
fn assert_blocked(out: &Output, tss: &[&str], why: &str) {
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{why}: rc 2: {}", stderr_text(out));
    assert!(out.stdout.is_empty(), "{why}: stdout 0 byte");
    let text = stderr_text(out);
    assert_eq!(text.lines().count(), 1, "{why}: stderr は 1 行: {text}");
    for ts in tss {
        assert!(text.contains(ts), "{why}: ts {ts} を並べる: {text}");
    }
    let listed = text.split_once("ts=").and_then(|(_, rest)| rest.split_once(" — ")).map(|(list, _)| list.split(',').collect::<Vec<_>>());
    assert_eq!(listed.as_deref(), Some(tss), "{why}: 並べた ts は未仕分けの全部（母集団 {}）: {text}", tss.len());
    for mouth in ["utterance sort", "--as request --memo", "--as chat", "seat ruling bind", "utterance show"] {
        assert!(text.contains(mouth), "{why}: 口の名 {mouth}: {text}");
    }
    assert!(text.contains(NAME) && !text.contains(STOP_MARKS), "{why}: 口の名は在り、逐語は運ばない: {text}");
}

/// 止めない周の外形: rc 0・stdout も stderr も 0 byte。
fn assert_passes(out: &Output, why: &str) {
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{why}: 止めない: {}", stderr_text(out));
    assert_silent(out, why);
}

/// event の fixture 1 件（共通の欄を固定する）。
fn fixture_event(kind: vessel::fleet::EventKind, bead: &str, case: vessel::fleet::Case) -> vessel::fleet::Event {
    vessel::fleet::Event {
        schema: vessel::fleet::SCHEMA,
        ts: vessel::fleet::cli::now_utc(),
        kind,
        run: String::new(),
        bead: bead.to_owned(),
        host: "h".to_owned(),
        actor: "human".to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: None,
        allowance: None,
        registration: None,
        mark: None,
        account: None,
        cost: None,
        rule: None,
        case: Some(case),
    }
}

/// event log へ fixture を 1 件足す。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn put_event(state: &Path, event: &vessel::fleet::Event) {
    let policy = vessel::fleet::store::LockPolicy::embedded().expect("lock の方針を読める");
    vessel::fleet::store::append(state, event, policy).expect("log へ足せる");
}

/// 発話 `ts` を要望として memo へ仕分ける（仕分けの event の fixture・台帳は読まない）。
fn sort_as_request(state: &Path, ts: &str, memo: &str) {
    let case = vessel::fleet::Case::Sorted { utterance: ts.to_owned(), sorting: vessel::fleet::Sorting::Request };
    put_event(state, &fixture_event(vessel::fleet::EventKind::UtteranceSorted, memo, case));
}

/// 発話 `ts` を問いへの答えとして結ぶ（裁定 event の fixture・発話の ts の key を持つ）。
fn sort_as_answer(state: &Path, ts: &str) {
    let case = vessel::fleet::Case::Ruling {
        ruling: "s2-q1:1".to_owned(),
        utterance: ts.to_owned(),
        channel: vessel::fleet::Channel::Chat,
        question_ts: "2026-09-30T06:00:00Z".to_owned(),
        asked: None,
    };
    let event = fixture_event(vessel::fleet::EventKind::RulingReceived, "s2-q1", case);
    put_event(state, &vessel::fleet::Event { detail: Some("推奨で".to_owned()), ..event });
}

/// 発話 `ts` を会話として仕分ける（実物の `utterance sort --as chat`）。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn sort_as_chat(state: &Path, ts: &str) {
    let out = Command::new(bin())
        .args(["utterance", "sort", "--state-dir", &state.display().to_string(), "--ts", ts, "--as", "chat"])
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "会話の仕分けは rc 0: {}", stderr_text(&out));
}

/// log の `TurnEndUnjudged` の (session, reason)（物理順）。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn unjudged_in(state: &Path) -> Vec<(Option<String>, String)> {
    let events = vessel::fleet::store::read_all(state).expect("log を読める");
    let cases = events.into_iter().filter(|event| event.kind == vessel::fleet::EventKind::TurnEndUnjudged);
    cases.filter_map(|event| match event.case {
        Some(vessel::fleet::Case::TurnEnd { session, reason }) => Some((session, reason)),
        _ => None,
    }).collect()
}

/// session の置き場（`<state>/session`）の下の file の相対 path（整列）。dir が無ければ空。
fn session_files(state: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                out.push(path.strip_prefix(root).map(|rest| rest.display().to_string()).unwrap_or_default());
            }
        }
    }
    let root = state.join("session");
    let mut found = Vec::new();
    walk(&root, &root, &mut found);
    found.sort();
    found
}

/// (a) 未仕分け 2 つの session の Stop が止まり（rc 2・stdout 0 byte・stderr 1 行に 2 つの ts と口の名・逐語の字は無い・差し込みの記録 0）、
/// 2 度目の Stop（再入でない）は rc 0・0 byte。3 つ目の発話を足すと止まって 3 つの ts を並べる。同じ log の別 session の未仕分けでは止まらない。
#[test]
fn hook_unsorted_stop_blocks_once_lists_every_ts_and_leaves_other_sessions_alone() {
    let repo = git_repo();
    let state = linked(&repo);
    let flag = state.display().to_string();
    let args = ["--state-dir", flag.as_str()];
    for sid in ["sid-a", "sid-b", "sid-c"] {
        start_session(&repo, &args, sid);
    }
    let first = say_in(&repo, "sid-a", &format!("{STOP_WORDS} 一つ目"));
    let second = say_in(&repo, "sid-a", &format!("{STOP_WORDS} 二つ目"));
    say_in(&repo, "sid-b", "別の session の一つ目");
    say_in(&repo, "sid-b", "別の session の二つ目");
    assert_passes(&stop_hook(&repo, &args, Some("sid-c"), false), "発話を持たない session（同じ log に別 session の未仕分けが 2 つ）");
    let (before_inject, before_log) = (inject_lines(&state).len(), fs::read_to_string(vessel::fleet::store::events_path(&state)).unwrap_or_default());
    let out = stop_hook(&repo, &args, Some("sid-a"), false);
    assert_blocked(&out, &[&first, &second], "未仕分け 2 つ");
    assert_eq!(inject_lines(&state).len(), before_inject, "差し込みの記録を書かない");
    assert_eq!(fs::read_to_string(vessel::fleet::store::events_path(&state)).unwrap_or_default(), before_log, "止める周は event を書かない");
    assert_passes(&stop_hook(&repo, &args, Some("sid-a"), false), "2 度目の Stop（同じ ts は 2 度止めない）");
    let third = say_in(&repo, "sid-a", &format!("{STOP_WORDS} 三つ目"));
    assert_blocked(&stop_hook(&repo, &args, Some("sid-a"), false), &[&first, &second, &third], "3 つ目を足す");
    assert_passes(&stop_hook(&repo, &args, Some("sid-a"), false), "3 つ目の後の 2 度目");
    clean(&[&repo, &state]);
}

/// (b) request・chat・答え（裁定 event の発話の ts の key）で仕分けた発話だけの session は止まらず、控えの file も書かない。
/// 対照の未仕分けの発話を 1 つ足すと、その 1 つの ts だけを並べて止まる。
#[test]
fn hook_unsorted_stop_does_not_block_a_session_whose_utterances_are_all_sorted() {
    let repo = git_repo();
    let state = linked(&repo);
    let flag = state.display().to_string();
    let args = ["--state-dir", flag.as_str()];
    start_session(&repo, &args, "sid-s");
    let (by_request, by_chat, by_answer) = (say_in(&repo, "sid-s", "要望"), say_in(&repo, "sid-s", "会話"), say_in(&repo, "sid-s", "答え"));
    sort_as_request(&state, &by_request, "s2-m1");
    sort_as_chat(&state, &by_chat);
    sort_as_answer(&state, &by_answer);
    assert_passes(&stop_hook(&repo, &args, Some("sid-s"), false), "仕分け済みの 3 形（request・chat・答え）");
    assert_eq!(session_files(&state), ["sid-s/start"], "仕分け済みの周は控えを書かない");
    let open = say_in(&repo, "sid-s", &format!("{STOP_WORDS} 未仕分け"));
    assert_blocked(&stop_hook(&repo, &args, Some("sid-s"), false), &[&open], "対照の未仕分け 1 つ");
    clean(&[&repo, &state]);
}

/// PATH の先頭に `fake`（偽の `tmux` を置いた dir・どの問いにも席の target を返す）を足して hook を撃つ。実 tmux を立てる歯は nextest の
/// tmux group（`.config/nextest.toml`・名前の列挙）に載せる要るので、席の解決だけを偽の tmux に替える。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn run_hook_with_fake_tmux(fake: &Path, args: &[&str], payload: &str) -> Output {
    let path = format!("{}:{}", fake.display(), std::env::var("PATH").unwrap_or_default());
    let mut child = Command::new(bin())
        .arg("hook")
        .args(args)
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary を起動できる");
    child.stdin.as_mut().expect("stdin を開ける").write_all(payload.as_bytes()).expect("payload を書ける");
    child.wait_with_output().expect("終了を待てる")
}

/// (c) 席（偽の tmux が target を返す）: 止めた周は state.jsonl の最終行が busy のまま・差し込みの記録 0 行増・打刻も増えず、続く再入の Stop で
/// idle に戻る。止めていない再入（と、戻した後の再入）は黙り打刻しない。
#[test]
fn hook_unsorted_stop_keeps_the_seat_busy_until_the_reentry_releases_it() {
    use std::os::unix::fs::PermissionsExt;
    let repo = git_repo();
    let (state, fake, name) = (linked(&repo), tmp(), "hookblock");
    let script = fake.join("tmux");
    fs::write(&script, format!("#!/bin/sh\nprintf '{name}:{name}\\n'\n")).unwrap_or_else(|err| panic!("偽の tmux: {err}"));
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap_or_else(|err| panic!("実行権: {err}"));
    let flag = state.display().to_string();
    let args = ["--state-dir", flag.as_str(), "--pane", "%1"];
    let hook = |event: &str, sid: &str, active: bool| {
        let mut all = vec![event];
        all.extend(args);
        run_hook_with_fake_tmux(&fake, &all, &stop_payload(&repo, Some(sid), active))
    };
    assert_eq!(hook("session-start", "sid-c", false).status.code(), Some(i32::from(RC_OK)), "session-start は rc 0");
    assert_silent(&hook("user-prompt-submit", "sid-c", false), "打刻だけの user-prompt-submit（busy）");
    let stamps = || fs::read_to_string(state_file(&state, name)).unwrap_or_default();
    let last = || stamps().lines().last().map(str::to_owned).unwrap_or_default();
    assert!(last().contains("\"busy\""), "busy から始める: {}", last());
    assert_passes(&hook("stop", "sid-c", true), "止めていない再入は黙る");
    assert!(last().contains("\"busy\""), "止めていない再入は打刻しない: {}", last());
    let ts = say_in(&repo, "sid-c", "未仕分け");
    let (lines, injects) = (stamps().lines().count(), inject_lines(&state).len());
    assert_blocked(&hook("stop", "sid-c", false), &[&ts], "未仕分けの Stop");
    assert_eq!((stamps().lines().count(), inject_lines(&state).len()), (lines, injects), "止めた周は打刻も差し込みの記録も増やさない");
    assert!(last().contains("\"busy\""), "止めた周の最終行は busy: {}", last());
    assert_passes(&hook("stop", "sid-c", true), "続く再入は止めず席を戻す");
    assert_eq!(stamps().lines().count(), lines + 1, "再入で 1 行増える");
    assert!(last().contains("\"idle\"") && last().contains("\"Stop\""), "再入で idle（event は Stop）: {}", last());
    assert_passes(&hook("stop", "sid-c", true), "戻した後の再入は黙る");
    assert_eq!(stamps().lines().count(), lines + 1, "戻した後の再入は打刻しない");
    assert_eq!(inject_lines(&state).len(), injects, "差し込みの記録は増えない");
    clean(&[&repo, &state, &fake]);
}

/// 止めずに通す周の外形（rc 0・stdout 0 byte）と、`TurnEndUnjudged` の (session, reason) の列。
fn unjudged_after(state: &Path, out: &Output, why: &str) -> Vec<(Option<String>, String)> {
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{why}: rc 0: {}", stderr_text(out));
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{why}: stdout と stderr は 0 byte: {}", stderr_text(out));
    unjudged_in(state)
}

/// (d) 読めない 5 語の各 fixture で Stop は rc 0・`TurnEndUnjudged` が 1 件（reason が一致）。同じ fixture で Stop をもう 1 度撃つと、
/// 控えを持てる 2 語（no-start・log-unreadable）は増えず、控えを持てない・読めない・書けない 3 語（no-session・told-unreadable・told-unwritable）は 1 件増える。
#[test]
fn hook_unsorted_stop_records_the_five_unjudged_words_once_or_every_time() {
    let words = ["no-session", "no-start", "log-unreadable", "told-unreadable", "told-unwritable"];
    let keeps = [false, true, true, false, false];
    assert_eq!((words.len(), keeps.len()), (5, 5), "母集団");
    for (word, keeps) in words.iter().zip(keeps) {
        let repo = git_repo();
        let (state, aux) = (linked(&repo), tmp());
        let flag = state.display().to_string();
        let args = ["--state-dir", flag.as_str()];
        let session = Some("sid-d");
        let sid = match *word {
            "no-session" => None,
            "no-start" => session,
            "log-unreadable" => {
                say_in(&repo, "other", &"長い発話 ".repeat(400));
                start_session(&repo, &args, "sid-d");
                fs::write(vessel::fleet::store::events_path(&state), "").unwrap_or_else(|err| panic!("log を縮められる: {err}"));
                session
            }
            "told-unreadable" => {
                start_session(&repo, &args, "sid-d");
                fs::create_dir_all(state.join("session").join("sid-d").join("told")).unwrap_or_else(|err| panic!("控えの位置に dir: {err}"));
                session
            }
            _ => {
                start_session(&repo, &args, "sid-d");
                say_in(&repo, "sid-d", "未仕分け");
                let dangling = std::os::unix::fs::symlink(aux.join("no-such-dir").join("told"), state.join("session").join("sid-d").join("told"));
                dangling.unwrap_or_else(|err| panic!("書けない控え: {err}"));
                session
            }
        };
        let expected = |count: usize| vec![(sid.map(str::to_owned), (*word).to_owned()); count];
        let first = unjudged_after(&state, &stop_hook(&repo, &args, sid, false), word);
        assert_eq!(first, expected(1), "{word}: 1 度目は 1 件");
        let second = unjudged_after(&state, &stop_hook(&repo, &args, sid, false), word);
        assert_eq!(second, expected(if keeps { 1 } else { 2 }), "{word}: 2 度目（控えを持てる語は増えない）");
        clean(&[&repo, &state, &aux]);
    }
}

/// (d2) 路に使えない session_id（`/` を含む・空・長すぎる）も no-session で、置き場に何も作らない。
#[test]
fn hook_unsorted_stop_treats_an_unsafe_session_id_as_no_session() {
    let repo = git_repo();
    let state = linked(&repo);
    let flag = state.display().to_string();
    let args = ["--state-dir", flag.as_str()];
    let long ="x".repeat(200);
    let unsafe_ids = ["../escape", "a/b", "", long.as_str()];
    for sid in unsafe_ids {
        start_session(&repo, &args, sid);
        assert_passes(&stop_hook(&repo, &args, Some(sid), false), sid);
    }
    assert_eq!(unjudged_in(&state).len(), unsafe_ids.len(), "毎回 1 件（母集団 {}）", unsafe_ids.len());
    assert!(unjudged_in(&state).iter().all(|(session, reason)| session.is_none() && reason == "no-session"), "session 無しの no-session");
    assert_eq!(session_files(&state), Vec::<String>::new(), "置き場に file を作らない");
    assert!(!state.join("escape").exists(), "置き場の外へ出ない");
    clean(&[&repo, &state]);
}

/// hook を `sh -c` の子として撃ち、待った後の親（sh）の `/proc/<sh>/io` の rchar を返す（子の読みは wait の後に親へ積まれる・
/// 親の io は `cat` の子が読むので、測る側の読みが rchar に混ざらない）。戻りの `Output` は hook の rc と stderr を持ち、stdout は hook の分だけ。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn rchar_of_hook_via_sh(args: &[&str], payload: &str) -> (u64, Output) {
    let script = "\"$0\" \"$@\"; code=$?; cat /proc/$$/io >&2; exit $code";
    let mut child = Command::new("sh")
        .args(["-c", script, bin(), "hook"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sh を起動できる");
    child.stdin.as_mut().expect("stdin を開ける").write_all(payload.as_bytes()).expect("payload を書ける");
    let out = child.wait_with_output().expect("終了を待てる");
    let text = String::from_utf8_lossy(&out.stderr).into_owned();
    let (hook_err, io) = text.split_once("rchar:").expect("io の行が stderr に在る");
    let rchar = io.lines().next().expect("rchar の行").trim().parse().expect("rchar は数");
    (rchar, Output { stderr: hook_err.as_bytes().to_vec(), ..out })
}

/// 同じ repo と置き場で、開始の位置の前に `bytes` 以上の埋め草の event を置いた log（と新しい session の置き場）を作り直して Stop を撃った周の
/// 子の rchar。止まることも確かめる。path の長さが payload と git の出力に写るので、2 つの大きさは同じ repo と置き場で測る。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn rchar_with_filler(place: (&Path, &Path), bd: &str, bytes: usize) -> u64 {
    let (repo, state) = place;
    let filler ="{\"schema\":1,\"ts\":\"2026-09-29T01:03:00Z\",\"kind\":\"UtteranceSorted\",\"utterance\":\"t\",\"sorting\":\"chat\",\"host\":\"h\",\"actor\":\"machine\"}\n";
    let events = vessel::fleet::store::events_path(state);
    fs::create_dir_all(events.parent().expect("親 dir")).expect("dir を作れる");
    fs::write(&events, filler.repeat(bytes / filler.len() + 1)).expect("埋め草を書ける");
    fs::remove_dir_all(state.join("session")).ok();
    let flag = state.display().to_string();
    let args = ["--state-dir", flag.as_str(), "--bd", bd];
    start_session(repo, &args, "sid-e");
    let ts = say_in(repo, "sid-e", "未仕分け");
    let mut argv = vec!["stop"];
    argv.extend(args);
    let (rchar, out) = rchar_of_hook_via_sh(&argv, &stop_payload(repo, Some("sid-e"), false));
    assert_blocked(&out, &[&ts], &format!("{bytes} byte の埋め草の後の Stop"));
    rchar
}

/// (e) session の前に 10 MB と 20 MB の埋め草の event を置いた 2 つの log で、どちらも止まり、`sh -c` で起こした子の読みの byte（rchar）の差が
/// 4096 byte 未満（子の起動の /proc/self/maps の読みの揺れは許し、log を丸ごと読めば約 10 MB の差で落ちる）、
/// 偽の台帳 client の呼び出しは 0 行（台帳を読まない）。
#[test]
fn hook_unsorted_stop_read_bytes_differ_under_4_kib_for_a_10_mb_and_a_20_mb_log_and_never_the_ledger() {
    // flip-check: retroactive s2-07l.749
    let repo = git_repo();
    let (state, aux) = (linked(&repo), tmp());
    let (bd, calls) = counting_bd(&aux, "[]");
    let small = rchar_with_filler((&repo, &state), &bd, 10 * 1024 * 1024);
    let large = rchar_with_filler((&repo, &state), &bd, 20 * 1024 * 1024);
    assert!(!calls.exists(), "台帳の読みは 0 回（偽の client の呼び出しの記録が無い）");
    assert!(small > 0, "子の読みを測れている（rchar の増えは 0 でない）: {small}");
    let diff = small.abs_diff(large);
    assert!(diff < 4096, "log の大きさに依らず読む byte の差が 4096 byte 未満（10 MB: {small} / 20 MB: {large} / 差: {diff}）");
    clean(&[&repo, &state, &aux]);
}

/// (f) marker の無い repo は止めず event も置き場も書かない。発話の無い session（runner の形の payload）も止めず、event も控えも書かない
/// （置き場に在るのは開始の位置だけ）。同じ歯で対象の session は止まる。
#[test]
fn hook_unsorted_stop_writes_nothing_outside_a_served_repo_and_for_a_session_without_utterances() {
    let (bare, state) = (git_repo(), tmp());
    let flag = state.display().to_string();
    let runner = state.join("pipe").join("run-1").join("plugin").display().to_string();
    let args = ["--state-dir", flag.as_str(), "--plugin-root", runner.as_str()];
    assert_passes(&stop_hook(&bare, &args, Some("sid-f"), false), "marker の無い repo");
    let served = git_repo();
    let state_of_served = linked(&served);
    let served_flag = state_of_served.display().to_string();
    let served_args = ["--state-dir", served_flag.as_str(), "--plugin-root", runner.as_str()];
    start_session(&served, &served_args, "sid-runner");
    assert_passes(&stop_hook(&served, &served_args, Some("sid-runner"), false), "発話の無い session");
    assert_eq!(unjudged_in(&state_of_served).len(), 0, "event を書かない");
    assert_eq!(session_files(&state_of_served), ["sid-runner/start"], "控えの file は無い（開始の位置だけ）");
    assert_eq!(session_files(&state), Vec::<String>::new(), "marker の無い repo の置き場は空");
    assert!(!vessel::fleet::store::events_path(&state).exists(), "marker の無い repo は log も作らない");
    start_session(&served, &served_args, "sid-target");
    let ts = say_in(&served, "sid-target", "未仕分け");
    assert_blocked(&stop_hook(&served, &served_args, Some("sid-target"), false), &[&ts], "対象の session は止まる");
    clean(&[&bare, &state, &served, &state_of_served]);
}

/// (g) 1 度目の SessionStart の後に未仕分けの発話を 1 つ書き、2 度目の SessionStart（resume の形）を撃つ。開始の位置は上書きされず、
/// 次の Stop がその発話で止まる。
#[test]
fn hook_unsorted_stop_keeps_the_first_start_position_across_a_second_session_start() {
    let repo = git_repo();
    let state = linked(&repo);
    let flag = state.display().to_string();
    let args = ["--state-dir", flag.as_str()];
    start_session(&repo, &args, "sid-g");
    let ts = say_in(&repo, "sid-g", "圧縮の前の未仕分け");
    start_session(&repo, &args, "sid-g");
    assert_blocked(&stop_hook(&repo, &args, Some("sid-g"), false), &[&ts], "2 度目の SessionStart の後の Stop");
    assert_eq!(session_files(&state), ["sid-g/start", "sid-g/told"], "置き場は開始の位置と控えの 2 file");
    clean(&[&repo, &state]);
}

/// (h) session_id の無い SessionStart の後は置き場の下に file が 0 本。session_id を持つ SessionStart は開始の位置を 1 つだけ書く。
#[test]
fn hook_unsorted_stop_writes_a_start_position_only_for_a_session_with_an_id() {
    let repo = git_repo();
    let state = linked(&repo);
    let flag = state.display().to_string();
    let out = run_hook_args(&["session-start", "--state-dir", &flag], &payload(&repo));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "session_id の無い session-start も rc 0: {}", stderr_text(&out));
    assert_eq!(session_files(&state), Vec::<String>::new(), "session_id が無ければ file 0 本");
    start_session(&repo, &["--state-dir", &flag], "sid-h");
    assert_eq!(session_files(&state), ["sid-h/start"], "開始の位置が 1 つ");
    clean(&[&repo, &state]);
}

// ───── 直近の流れの事実行の種類 owned（設計 seat-heartbeat.md §24 約束 4・5・契約表の行 ac・接頭辞 `hook_session_recent_owned_`） ─────
//
// 席の置き場の `fleet/lifecycle.json`（局面の出力）を直に置き、SessionStart が読み手 1 本（台帳と main の組）で出力の owned の値を
// `[RECENT-OWNED]` / `[RECENT-NONE]` / `[RECENT-UNMEASURED]` の行に写すかを測る。

/// 席の手番の閾値越えの数え（件数 `count`・最古は `count` が 1 以上の周だけ X）。
fn owned_value(count: u64) -> vessel::fleet::lifecycle::Owned {
    let oldest = (count > 0).then(|| vessel::fleet::lifecycle::Oldest {
        part: vessel::case::Kind::Contract,
        id: "s2-x.1".to_owned(),
        phase: vessel::case::Phase::ContractRefused,
        since: "2026-09-01T00:00:00Z".to_owned(),
    });
    vessel::fleet::lifecycle::Owned { count, unset: 0, unknown: 0, oldest }
}

/// toy repo に台帳の file（files の形）と main の ref を足す（出力の入力の印を器の印の読み手で今の値に読むため）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn owned_toy(place: &RolePlace) {
    fs::create_dir_all(place.repo.join(".beads")).expect("台帳の dir を作れる");
    fs::write(place.repo.join(".beads").join("issues.jsonl"), "[]\n").expect("台帳の file を書ける");
    let refs = place.repo.join(".git").join("refs").join("remotes").join("origin");
    fs::create_dir_all(&refs).expect("ref の dir を作れる");
    fs::write(refs.join("main"), "0123456789abcdef0123456789abcdef01234567\n").expect("main の ref を書ける");
}

/// 局面の出力を置く（入力の印は器の印の読み手で今の値を読んで書く）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn owned_put(place: &RolePlace, parts: Vec<vessel::case::Part>, owned: vessel::fleet::lifecycle::Owned) {
    use vessel::fleet::lifecycle::{render_output, Output, Scope};
    let marks = lmark::Marks {
        ledger: lmark::read_ledger(&place.repo).expect("台帳の印を読める"),
        events: lmark::read_events(&place.state).expect("event log の印を読める"),
        main: lmark::read_main(&place.repo).expect("main の印を読める"),
    };
    let stamp = vessel::fleet::cli::format_utc(vessel::seat::state::now_secs().saturating_sub(600));
    let out = Output {
        generated_at: stamp.clone(),
        scope: Scope::Full,
        full_at: stamp,
        interval_s: Some(600),
        closed_window_h: Some(72),
        marks,
        unmeasured: Vec::new(),
        owned,
        parts,
    };
    let dir = place.state.join("fleet");
    fs::create_dir_all(&dir).expect("fleet の dir を作れる");
    fs::write(dir.join("lifecycle.json"), render_output(&out)).expect("出力を置ける");
}

/// 登録した席で session-start を撃ち、復帰の DATA のうち owned の行（`[RECENT-OWNED]` と `kind=owned` の行）と DATA の最後の行を返す。
fn owned_lines(place: &RolePlace, path: &str) -> (Vec<String>, Option<String>) {
    let (_, recent) = brief_and_recent(place, path, &place.bd);
    let owned = recent.iter().filter(|line| line.starts_with("[RECENT-OWNED]") || line.contains(" kind=owned")).cloned().collect();
    (owned, recent.last().cloned())
}

/// 閾値越えの最古（X）を持つ件数 `count` の owned の行。
fn owned_want(count: u64) -> String {
    format!("[RECENT-OWNED] count={count} oldest=contract:s2-x.1 phase=contract-refused since=2026-09-01T00:00:00Z")
}

/// (h) 閾値越え 1 件の出力で `[RECENT-OWNED]` の行が count=1 と最古の部品・id・局面・時刻を持つ（古くない周は stale= が無い）。
#[test]
fn hook_session_recent_owned_names_the_count_and_the_oldest() {
    let place = role_place();
    let path = stub_seat(&place, "ownedh", Some("orchestrator"));
    owned_toy(&place);
    owned_put(&place, Vec::new(), owned_value(1));
    let (owned, _) = owned_lines(&place, &path);
    assert_eq!(owned, [owned_want(1)], "件数 1 と最古");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (i) 件数 0 の出力で `[RECENT-NONE] kind=owned`（OWNED の行は出ない）。
#[test]
fn hook_session_recent_owned_is_none_for_a_count_of_zero() {
    let place = role_place();
    let path = stub_seat(&place, "ownedi", Some("orchestrator"));
    owned_toy(&place);
    owned_put(&place, Vec::new(), owned_value(0));
    let (owned, _) = owned_lines(&place, &path);
    assert_eq!(owned, ["[RECENT-NONE] kind=owned"], "件数 0");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (j) 出力の無い置き場と json の読めない置き場の 2 周で `[RECENT-UNMEASURED] kind=owned reason=lifecycle`。owned の行は事実行の区間の最後の行。
#[test]
fn hook_session_recent_owned_is_unmeasured_for_an_absent_or_unreadable_output() {
    let place = role_place();
    let path = stub_seat(&place, "ownedj", Some("orchestrator"));
    owned_toy(&place);
    let want = "[RECENT-UNMEASURED] kind=owned reason=lifecycle";
    let (owned, last) = owned_lines(&place, &path);
    assert_eq!((owned, last.as_deref()), (vec![want.to_owned()], Some(want)), "出力が無い周");
    let dir = place.state.join("fleet");
    fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("mkdir: {err}"));
    fs::write(dir.join("lifecycle.json"), "{\"version\":2}\n").unwrap_or_else(|err| panic!("write: {err}"));
    let (owned, last) = owned_lines(&place, &path);
    assert_eq!((owned, last.as_deref()), (vec![want.to_owned()], Some(want)), "json が読めない周");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (k) 古さ: 変化の無い周は stale= が無く、台帳の file を変えた周は stale=ledger・main の ref を動かした周は stale=main・古さの印の file に
/// ledger-gate の印を置いた周は stale=ledger-gate（出力は周ごとに今の印で書き直す）。
#[test]
fn hook_session_recent_owned_names_why_the_output_is_stale() {
    let place = role_place();
    let path = stub_seat(&place, "ownedk", Some("orchestrator"));
    owned_toy(&place);
    let rounds = || {
        let (owned, _) = owned_lines(&place, &path);
        owned
    };
    owned_put(&place, Vec::new(), owned_value(1));
    assert_eq!(rounds(), [owned_want(1)], "変化の無い周");
    fs::write(place.repo.join(".beads").join("issues.jsonl"), "[]\n\n").unwrap_or_else(|err| panic!("write: {err}"));
    assert_eq!(rounds(), [format!("{} stale=ledger", owned_want(1))], "台帳を変えた周");
    owned_put(&place, Vec::new(), owned_value(1));
    let main = place.repo.join(".git").join("refs").join("remotes").join("origin").join("main");
    fs::write(main, "fedcba9876543210fedcba9876543210fedcba98\n").unwrap_or_else(|err| panic!("write: {err}"));
    assert_eq!(rounds(), [format!("{} stale=main", owned_want(1))], "main を動かした周");
    owned_put(&place, Vec::new(), owned_value(1));
    let ledger = lmark::read_ledger(&place.repo).unwrap_or_else(|| panic!("台帳の印を読める"));
    let found = lmark::Mark { kind: lmark::Kind::LedgerGate, at: "2026-10-01T00:00:00Z".to_owned(), value: lmark::Value::Ledger(ledger) };
    let policy = vessel::fleet::store::LockPolicy { retry_ms: 50, stale_ms: 600_000 };
    assert_eq!(lmark::add_mark(&place.state, &found, policy), lmark::Added::Added, "印を足せた");
    assert_eq!(rounds(), [format!("{} stale=ledger-gate", owned_want(1))], "古さの印の file に ledger-gate を置いた周");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}

/// (p) 件数 2・最古 X で閾値越えの印を持つ部品が 0 の出力は `[RECENT-OWNED] count=2` の行が最古 X を持ち、件数 0 で閾値越えの印を持つ部品が在る
/// 出力は `[RECENT-NONE] kind=owned`（部品の印を数え直さず出力の値を写す）。
#[test]
fn hook_session_recent_owned_copies_the_output_and_never_recounts_the_marks() {
    let place = role_place();
    let path = stub_seat(&place, "ownedp", Some("orchestrator"));
    owned_toy(&place);
    owned_put(&place, Vec::new(), owned_value(2));
    let (owned, _) = owned_lines(&place, &path);
    assert_eq!(owned, [owned_want(2)], "件数 2・印を持つ部品 0");
    let phase = vessel::case::Phase::MemoWaiting;
    let marked = vessel::case::Part {
        part: vessel::case::Kind::Memo,
        id: "s2-m.1".to_owned(),
        phase,
        turn: vessel::case::turn_of(phase.as_str(), None).unwrap_or_else(|| panic!("表に在る語")),
        since: None,
        reason: None,
        closed: false,
        overdue: Some(true),
        links: vessel::case::Links::default(),
        extra: vessel::case::Extra::Memo { due: None, triggers: None, keep: None },
    };
    owned_put(&place, vec![marked], owned_value(0));
    let (owned, _) = owned_lines(&place, &path);
    assert_eq!(owned, ["[RECENT-NONE] kind=owned"], "件数 0・印を持つ部品 1");
    clean(&[&place.repo, &place.state, &place.sock_dir]);
}
