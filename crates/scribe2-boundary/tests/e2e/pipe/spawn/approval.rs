// flip-check: moved s2-07l.687
//! approval と run_cost と report の族の歯（接頭辞 `pipe_approval_` / `run_cost_` / `pipe_report_`・設計 docs/design/carry-prep.md §10 行 o・親 `tests/e2e/pipe/spawn.rs` の helper を `use super::*` で使う）。

use super::*;

#[test]
fn pipe_approval_blocks_before_spawn_when_contract_declares_class() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    // **2 クラス**で撃つ。1 クラスだと detail の連結が恒等になり、区切りを測れない。
    let (id, said) = blocked(&repo, &state, &marker, r#"classes = ["delete", "publish"]"#);
    assert!(said.contains(&format!("run={id} stage=Blocked")), "止まった先を stdout で名乗る: {said}");
    // **数字そのものが約束である**（設計 §5.5）。定数を辿るだけの assert は、定数が
    // 動いたときに歯も黙って追随する——外形の 3 はここで literal に留める。
    assert_eq!(RC_BLOCKED, 3, "人の手番で止まっている周の rc は 3");
    // **効果で測る**: 止めたと名乗るだけでなく、runner が 1 度も起きていない。A1 の
    // 「実行前」は、消す / 出す / 使うが**起きた後**に聞くのでは意味が無い。
    assert!(!marker.exists(), "runner を起こさない");
    assert!(
        !repo.join(".worktrees").join("scribe2").join(&id).exists(),
        "worktree も切らない"
    );
    let mine: Vec<Event> = events(&state).into_iter().filter(|found| found.run == id).collect();
    let requested = mine
        .iter()
        .find(|found| found.kind == EventKind::ApprovalRequested)
        .expect("ApprovalRequested を記帳する");
    assert_eq!(
        requested.detail.as_deref(),
        Some("delete+publish"),
        "何のクラスで止めたかを名指す（複数なら全部・区切りは +）"
    );
    assert_eq!(requested.actor, "machine", "止めたのは機械であって人の event ではない");
    assert!(
        mine.iter().any(|found| found.stage == Some(Stage::Blocked)),
        "段は Blocked に落ちる"
    );
    let line = show_line(&repo, &state, &id);
    assert!(line.contains("stage=Blocked"), "{line}");
    assert!(line.contains("approved=false"), "{line}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_records_verbatim_as_human_event() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let (id, _) = blocked(&repo, &state, &marker, r#"classes = ["publish"]"#);
    // 引用符も全角も入った 1 行を **要約せずそのまま** 通す（C7.2）。前後の空白と
    // 大文字を混ぜてあるのは、正規化（trim / 小文字化）を「そのまま」と言い張れない
    // ようにするためである——fixture が綺麗だと歯は正規化を見逃す。
    let words = r#"  OK：出してよい（user 逐語 2026-09-09）："推奨で進めて"  "#;
    let out = run_pipe(&[
        "approve", "--run", &id, "--words", words,
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let human: Vec<Event> =
        events(&state).into_iter().filter(|found| found.actor == "human").collect();
    assert_eq!(human.len(), 1, "人由来の event は承認の 1 件だけである（FR22 の計測面）");
    let received = human.first().expect("承認 event が 1 件在る");
    assert_eq!(received.kind, EventKind::ApprovalReceived, "種類は ApprovalReceived");
    assert_eq!(received.detail.as_deref(), Some(words), "逐語をそのまま持つ");
    assert_eq!(received.bead, "s2-2e5", "どの契約への承認かを持つ");
    let line = show_line(&repo, &state, &id);
    assert!(line.contains("approved=true"), "{line}");
    // 承認は「許し」であって「前進」ではない——段を動かすのは resume である。
    assert!(line.contains("stage=Blocked"), "{line}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_refuses_empty_words() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let (id, _) = blocked(&repo, &state, &marker, r#"classes = ["consume"]"#);
    let before = event_count(&state);
    // 空も空白だけも承認ではない。「聞いた形」だけが残る記録を作らない。
    for words in ["", "   "] {
        let out = run_pipe(&[
            "approve", "--run", &id, "--words", words,
            "--state-dir", &state.display().to_string(),
        ]);
        assert_eq!(
            out.status.code(),
            Some(i32::from(RC_REFUSED)),
            "空の逐語を承認にしない: {words:?}"
        );
    }
    assert_eq!(event_count(&state), before, "1 byte も書かない");
    assert!(show_line(&repo, &state, &id).contains("approved=false"), "承認は立たない");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_resume_spawns_after_received() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let (id, _) = blocked(&repo, &state, &marker, r#"classes = ["delete"]"#);
    let approved = run_pipe(&[
        "approve", "--run", &id, "--words", "消してよい",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(approved.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&approved));
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", &runner_cmd(&marker),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    // 効果で測る: 承認の後は runner が実際に起き、便が先の段へ進む。
    assert!(marker.exists(), "承認の後は runner が起きる");
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"), "段が進む");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_resume_stays_blocked_without_received() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let (id, _) = blocked(&repo, &state, &marker, r#"classes = ["publish"]"#);
    let before = event_count(&state);
    let out = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--runner", &runner_cmd(&marker),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BLOCKED)), "未承認の resume は rc 3");
    assert!(!marker.exists(), "runner を起こさない");
    // 待っている事実は Blocked が既に持っている。resume のたびに積むと
    // 「何回聞いたか」が事実と食い違う。
    assert_eq!(event_count(&state), before, "何も書かない");

    // **資格の無い `ApprovalReceived` では関門は開かない**（憲法 C7.2・planner 裁定 Q1）。
    // `fleet record` は公開の口なので、書き手（`pipe approve`）の逐語検査だけでは
    // 「承認は event に残った逐語だけ」を守れない——読み手が資格を見る。
    for disqualified in [
        vec!["--actor", "machine"],
        vec!["--actor", "human"],
        vec!["--actor", "human", "--detail", "   "],
        vec!["--actor", "machine", "--detail", "出してよい"],
    ] {
        let wrote = record_approval(&state, &id, &disqualified);
        assert_eq!(
            wrote.status.code(),
            Some(i32::from(RC_OK)),
            "event を積むこと自体はできる（塞ぐのは読み手である）: {}",
            stderr_of(&wrote)
        );
        assert!(
            show_line(&repo, &state, &id).contains("approved=false"),
            "資格の無い承認で関門は開かない: {disqualified:?}"
        );
        let again = run_pipe(&[
            "resume", "--run", &id, "--repo", &repo.display().to_string(),
            "--state-dir", &state.display().to_string(),
            "--runner", &runner_cmd(&marker),
        ]);
        assert_eq!(
            again.status.code(),
            Some(i32::from(RC_BLOCKED)),
            "資格の無い承認の後も rc 3 のまま: {disqualified:?}"
        );
        assert!(!marker.exists(), "runner を起こさない: {disqualified:?}");
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_unlisted_class_value_is_rejected_at_intake() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[r#"classes = ["deploy"]"#]);
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
    ]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "名簿に無いクラスを通さない");
    let err = stderr_of(&out);
    assert!(err.contains("deploy"), "断る値を名指す: {err}");
    for listed in ["delete", "publish", "consume"] {
        assert!(err.contains(listed), "取れる値を全部見せる: {err}");
    }
    assert_eq!(event_count(&state), 0, "断った便は 1 行も記帳しない");
    // **弁別**: 断っているのは「classes が在ること」ではなく **値**である。
    let listed = write_contract(&repo, &[], &[r#"classes = ["publish"]"#]);
    let ok = intake_raw(&repo, &state, &listed, "b");
    assert_eq!(ok.status.code(), Some(i32::from(RC_OK)), "名簿に在る値は通す: {}", stderr_of(&ok));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_approval_blocks_in_one_shot_run() {
    let (repo, state) = repo_with_state();
    let marker = state.join("runner-ran");
    let path = write_contract(&repo, &[], &[r#"classes = ["publish"]"#]);
    // **一発経路**（intake → spawn → gate → land を 1 process で通す）でも関門は効く。
    // 段ごとの口だけを測ると、この経路だけ素通りする実装に気づけない——関門は唯一の
    // 起動口 `spawn()` に在るという主張を、経路の側から裏書きする歯である。
    let out = run_pipe(&[
        "run", "--design", &path, "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
        "--runner", &runner_cmd(&marker), "--lens", &review_lens_pass(&state),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BLOCKED)),
        "一発経路でも spawn の手前で止まる: {}",
        stderr_of(&out)
    );
    assert!(!marker.exists(), "runner を起こさない");
    let id = run_id_of(&out);
    assert!(!id.is_empty(), "止まった周も run id を出す: {}", stdout_of(&out));
    assert!(show_line(&repo, &state, &id).contains("stage=Blocked"), "段は Blocked に落ちる");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_report_counts_human_events() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    // 1 便を land まで通す（機械だけで進む便）。
    let landed_id = gated_pass(&repo, &state, &path, &marker);
    let landed = land_once(&repo, &state, &landed_id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    // もう 1 便は intake で止める（`landed` に数えない側）。
    let open_id = intake_bead(&repo, &state, &path, "s2-open");
    let approved = run_pipe(&[
        "approve", "--run", &open_id, "--words", "推奨で進めて",
        "--state-dir", &state.display().to_string(),
    ]);
    assert_eq!(approved.status.code(), Some(i32::from(RC_OK)), "approve: {}", stderr_of(&approved));

    let out = report_once(&state);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "report は rc 0: {}", stderr_of(&out));
    assert_eq!(
        report_head(&out),
        format!("runs=2 landed=1 human_events=1 human_events_other_than_approval=0 {NO_REVIEW_FAIL}"),
        "到達点の 1 行（設計 §5.8・既存 token は不変で §22 の 2 token が末尾に足される）"
    );
    assert!(report_cost(&out).starts_with("cost: with_usage=0 out=0 cache_read=0 gate_secs="), "{}", stdout_of(&out));
    assert_eq!(stdout_of(&out).lines().count(), 2, "到達点の行と消費の行の 2 行: {}", stdout_of(&out));

    // **approval 以外の人由来 event は別に数える**——ここが 0 であることが到達点の主張
    // なので、0 のままにしか動かない数え方だと主張を測れない。
    let recorded = record_human_stage(&state, &open_id);
    assert_eq!(recorded.status.code(), Some(i32::from(RC_OK)), "record: {}", stderr_of(&recorded));
    let after = report_once(&state);
    assert_eq!(
        report_head(&after),
        format!("runs=2 landed=1 human_events=2 human_events_other_than_approval=1 {NO_REVIEW_FAIL}"),
        "approval 以外の人由来 event を数える"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_report_returns_rc2_on_malformed_store() {
    let (repo, state) = repo_with_state();
    let events = state.join("fleet").join("events.jsonl");
    fs::create_dir_all(state.join("fleet")).expect("dir を作れる");
    fs::write(&events, "こわれ\n").expect("壊れた行を書ける");
    // **数えられなかったを 0 に化けさせない**（C11.2）。到達点の 1 行は「人手 0」を
    // 主張する面なので、読めない台帳から 0 を出すと**偽の全クリア**そのものになる。
    let out = report_once(&state);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない台帳は rc 2");
    assert!(out.stdout.is_empty(), "rc 2 でも数を出さない");
    assert!(
        !stdout_of(&out).contains("human_events_other_than_approval=0"),
        "0 を名乗らない: {}",
        stdout_of(&out)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_report_counts_landed_runs_not_landed_events() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran");
    let id = gated_pass(&repo, &state, &path, &marker);
    let landed = land_once(&repo, &state, &id);
    assert_eq!(landed.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&landed));
    // 同じ便へ `Landed` の event をもう 1 件積む（手で積んだ / 台帳が壊れた周）。
    // **replay は便を数える**ので landed は 1 のまま——生の行を数える実装だと 2 になる。
    let doubled = bin_cmd()
        .args(["fleet", "record", "--state-dir"])
        .arg(&state)
        .args(["--kind", "RunDone", "--stage", "Landed", "--run", &id, "--bead", "s2-2e5"])
        .output()
        .expect("binary を起動できる");
    assert_eq!(doubled.status.code(), Some(i32::from(RC_OK)), "record: {}", stderr_of(&doubled));
    let out = report_once(&state);
    assert_eq!(
        report_head(&out),
        format!("runs=1 landed=1 human_events=0 human_events_other_than_approval=0 {NO_REVIEW_FAIL}"),
        "landed は便の数であって event の数ではない"
    );
    clean(&[&repo, &state]);
}

/// (e) runner の消費（設計 gate-cost.md §26 歯 (e)・spawn の側）: 便 1 本で `RunCost source=runner` が 1 件、`Implemented`
/// の前に書かれ、6 値は要約行の数と一致する。`pipe show --run` は母集団（`cost: events=1`）と消費の行を写し、`pipe report`
/// の 2 行目は便の数と token の和を出す。(f) 要約行が usage を運ばない便は event を書かず段は同じ `Implemented`。
#[test]
fn run_cost_spawn_records_one_runner_event_before_implemented() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let out = spawn_with(&repo, &state, &id, COST_RUNNER);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "spawn は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("stage=Implemented"), "{}", stdout_of(&out));
    let all = events(&state);
    let costs: Vec<(usize, &Event)> = all.iter().enumerate().filter(|(_, event)| event.kind == EventKind::RunCost).collect();
    assert_eq!(costs.len(), 1, "消費の event は 1 件（母集団 = event 数）: {costs:?}");
    let Some(&(at, event)) = costs.first() else {
        panic!("消費の event が無い");
    };
    let want = Usage { input: 11, output: 22, cache_read: 33, cache_create: 44, turns: 5, wall_ms: 6000 };
    assert_eq!(event.cost, Some(Cost { source: CostSource::Runner, usage: want }), "6 値は要約行の数");
    assert_eq!((event.run.as_str(), event.stage), (id.as_str(), None), "便に紐づき段を持たない");
    let implemented = all.iter().position(|event| event.stage == Some(Stage::Implemented));
    assert!(implemented.is_some_and(|found| at < found), "Implemented の前に書く: {at} / {implemented:?}");
    let shown = show_line(&repo, &state, &id);
    let cost_lines: Vec<&str> = shown.lines().filter(|line| line.starts_with("cost:")).collect();
    assert_eq!(
        cost_lines,
        [
            "cost: events=1",
            "cost: source=runner usage=in:11,out:22,cache_read:33,cache_create:44 turns=5 wall_ms=6000",
        ],
        "pipe show は母集団と消費の行を写す: {shown}"
    );
    assert!(shown.lines().next().unwrap_or_default().contains("stage=Implemented"), "1 行目は段のまま: {shown}");
    let report = report_once(&state);
    assert_eq!(report_cost(&report), "cost: with_usage=1 out=22 cache_read=33 gate_secs=0", "{}", stdout_of(&report));
    // (f) usage を運ばない便（要約行が無い）は event を書かない・段は同じ `Implemented`（別の置き場で同じ契約）。
    let (bare_repo, bare_state) = repo_with_state();
    let bare_path = write_contract(&bare_repo, &[], &[]);
    let bare_id = intake(&bare_repo, &bare_state, &bare_path);
    let bare = spawn_with(&bare_repo, &bare_state, &bare_id, "echo x >> src/lib.rs && git add -A && git commit -q -m runner");
    assert!(stdout_of(&bare).contains("stage=Implemented"), "{}", stdout_of(&bare));
    let bare_costs = events(&bare_state).iter().filter(|event| event.kind == EventKind::RunCost).count();
    assert_eq!(bare_costs, 0, "usage の無い便は消費の event を書かない");
    assert!(!show_line(&bare_repo, &bare_state, &bare_id).contains("cost:"), "消費の無い便の描画は従来のまま");
    let bare_report = report_once(&bare_state);
    assert_eq!(report_cost(&bare_report), "cost: with_usage=0 out=0 cache_read=0 gate_secs=0", "{}", stdout_of(&bare_report));
    clean(&[&repo, &state, &bare_repo, &bare_state]);
}

/// (e) 審査の lens の消費: intake の審査で判定 object が 6 値を運ぶ周は `RunCost source=review` が 1 件、`Reviewed` の前に
/// 書かれ、判定は PASS のまま（受付は rc 0）。6 値を運ばない審査は event を書かない（同じ歯の中で対に並べる）。
#[test]
fn run_cost_review_records_one_review_event_before_reviewed() {
    let usage = r#""usage":"in:7,out:8,cache_read:9,cache_create:10","turns":2,"wall_ms":300"#;
    for (extra, want) in [(Some(usage), 1_usize), (None, 0)] {
        let (repo, state) = repo_with_state();
        let path = write_contract(&repo, &[], &[]);
        let verdict = lens_verdict("PASS");
        let body = extra.map_or_else(|| verdict.clone(), |pairs| format!("{},{pairs}}}", verdict.trim_end_matches('}')));
        let rules = ceiling_rules(&state);
        let marker = state.join("lens-ran");
        let out = run_pipe(&[
            "intake", "--design", &path, "--bead", "s2-cost", "--repo", &repo.display().to_string(),
            "--state-dir", &state.display().to_string(), "--rules", &rules, "--lens", &fake_lens(&marker, &body),
        ]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "審査は PASS のまま: {}", stderr_of(&out));
        let all = events(&state);
        let costs: Vec<usize> = all.iter().enumerate().filter(|(_, event)| event.kind == EventKind::RunCost).map(|(at, _)| at).collect();
        assert_eq!(costs.len(), want, "消費の event 数: {all:?}");
        if let Some(at) = costs.first() {
            let found = all.get(*at).and_then(|event| event.cost);
            let want_usage = Usage { input: 7, output: 8, cache_read: 9, cache_create: 10, turns: 2, wall_ms: 300 };
            assert_eq!(found, Some(Cost { source: CostSource::Review, usage: want_usage }), "値は偽 lens の数");
            let reviewed = all.iter().position(|event| event.stage == Some(Stage::Reviewed));
            assert!(reviewed.is_some_and(|stage| *at < stage), "Reviewed の前に書く: {at} / {reviewed:?}");
        }
        clean(&[&repo, &state]);
    }
}

/// (a) 値の両側と断らない（設計 gate-cost.md §43 歯 (a)・契約表の行 aj）: 消費の event 2 件の 4 値の和が 25000000 ちょうどの
/// 便は over の行がちょうど 1 本（消費の行の後＝最後の行）、24999999 の便は 0 本で、どちらも land が rc 0 で `Landed` に着く。
/// 消費の行と母集団の行は両方の便で従来の形のまま 3 行。4 値のどれかを落とす・最後の event だけを数える・`>` で比べる
/// 実装は、ちょうどの便で行が消えて落ちる。
#[test]
fn run_cost_ceiling_over_line_at_the_limit_and_none_below_while_both_land() {
    for (short, want) in [(0_u64, vec![CEILING_OVER]), (1, Vec::new())] {
        let (repo, state, id) = ceiling_landed(short);
        let shown = show_line(&repo, &state, &id);
        assert!(shown.lines().next().unwrap_or_default().contains("stage=Landed"), "1 行目は段のまま（Landed）: {shown}");
        assert_eq!(ceiling_lines(&shown), want, "short={short}: {shown}");
        let costs = shown.lines().filter(|line| line.starts_with("cost:")).count();
        assert_eq!(costs, 3, "母集団の行と消費の行 2 本は従来の形のまま: {shown}");
        if short == 0 {
            assert_eq!(shown.lines().last(), Some(CEILING_OVER), "判定行は消費の行の後: {shown}");
        }
        clean(&[&repo, &state]);
    }
}

/// (b) 読めない閾値（設計 gate-cost.md §43 歯 (b)）: 消費の event 2 件の便に、行 `R-C6-1` を持たない fixture
/// （`ceiling_rules`）と不発効の fixture を渡すと、どちらも `cost-ceiling: unmeasured events=2` が 1 行（rc 0・1 行目は段の行）。
/// `--rules` 無しの対照は over の行を出し、`pipe show` は event を書かない。消費の event が 0 件の便は読めない fixture でも
/// `cost-ceiling:` の行が 0 本。
#[test]
fn run_cost_ceiling_unreadable_limit_is_unmeasured_and_zero_events_show_nothing() {
    let (repo, state, id) = ceiling_landed(0);
    let before = events(&state).len();
    for rules in [ceiling_rules(&state), disabled_ceiling_rules(&state)] {
        let out = show_with_rules(&repo, &state, &id, &rules);
        let shown = stdout_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc は 0 のまま: {}", stderr_of(&out));
        assert!(shown.lines().next().unwrap_or_default().contains("stage=Landed"), "1 行目は段の行: {shown}");
        assert_eq!(ceiling_lines(&shown), ["cost-ceiling: unmeasured events=2"], "{rules}: {shown}");
    }
    assert_eq!(ceiling_lines(&show_line(&repo, &state, &id)), [CEILING_OVER], "--rules 無しの対照は over の行");
    assert_eq!(events(&state).len(), before, "pipe show は event を書かない");
    let (bare_repo, bare_state) = repo_with_state();
    let bare_path = write_contract(&bare_repo, &[], &[]);
    let bare_id = intake(&bare_repo, &bare_state, &bare_path);
    let bare = spawn_with(&bare_repo, &bare_state, &bare_id, TOY_COMMIT);
    assert!(stdout_of(&bare).contains("stage=Implemented"), "{}", stdout_of(&bare));
    for rules in [ceiling_rules(&bare_state), disabled_ceiling_rules(&bare_state)] {
        let out = show_with_rules(&bare_repo, &bare_state, &bare_id, &rules);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc 0: {}", stderr_of(&out));
        assert!(ceiling_lines(&stdout_of(&out)).is_empty(), "消費の event 0 件の便は行を出さない: {}", stdout_of(&out));
    }
    clean(&[&repo, &state, &bare_repo, &bare_state]);
}
