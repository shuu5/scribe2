// flip-check: moved s2-07l.684
//! terminal と order の族の歯（接頭辞 `pipe_terminal_` / `pipe_order_`・設計 docs/design/carry-prep.md §10 行 l・親 `tests/e2e/pipe/land.rs` の helper を `use super::*` で使う）。

use super::*;

/// 着地の順番（設計 gate-cost.md §6）: 後から Gated になった便は前の便が列に居る間は待ち、上限
/// （fixture 1 秒）で**待たずに進む**（`order=degraded`・断らない・止めない）。main は動いていないので
/// 追随せずに Landed。
#[test]
fn pipe_order_later_run_degrades_at_the_limit_and_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(LAND_WAIT_S));
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "上限で進んで land する: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "degraded", "stdout の land 行: {}", stdout_of(&out));
    assert_eq!(exported_order(&state, &id_b), "degraded", "面 5 の record");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "段は Landed");
    assert!(show_line(&repo, &state, &id_a).contains("stage=Gated"), "前の便は列に残ったまま");
    clean(&[&repo, &state]);
}

/// 先に Gated になった便は待たない（`order=first`）。
#[test]
fn pipe_order_oldest_run_lands_first_without_waiting() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, _id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let out = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "first", "stdout の land 行: {}", stdout_of(&out));
    assert_eq!(exported_order(&state, &id_a), "first", "面 5 の record");
    clean(&[&repo, &state]);
}

/// 待っている便は、前の便が land して列を空けた時点で**上限を待たずに**進み（`order=waited:<n>`・n < 上限）、
/// 追随 1 回・gate の撃ち直し 1 回で Landed（撃ち直しの間は main が動かない＝(vi) が起きない）。
#[test]
fn pipe_order_waiting_run_lands_after_the_front_with_one_follow() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let mut waiting = land_in_background(&repo, &state, &id_b, &rules, &lens);
    std::thread::sleep(Duration::from_secs(2));
    assert!(waiting.try_wait().expect("子の状態を読める").is_none(), "後の便は列の前が空くまで待っている");
    let first = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "前の便の land: {}", stderr_of(&first));
    assert_eq!(order_token(&first), "first", "前の便は待たない: {}", stdout_of(&first));
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);

    let out = waiting.wait_with_output().expect("待っていた land が終わる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "待っていた便も land する: {}", stderr_of(&out));
    let token = order_token(&out);
    let waited: u64 = token.strip_prefix("waited:").and_then(|secs| secs.parse().ok()).unwrap_or(u64::MAX);
    assert!(waited < 30, "上限を待たずに進んだ（order={token}）: {}", stdout_of(&out));
    assert_eq!(exported_order(&state, &id_b), token, "面 5 の record も同じ値");
    assert_eq!(follow_count(&state, &id_b), 1, "追随は 1 回: {:?}", stages(&state, &id_b));
    assert_eq!(gate_count(&state, &id_b), 2, "gate は初回 + 撃ち直し 1 回: {:?}", stages(&state, &id_b));
    let new = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(git(&repo, &["rev-parse", &format!("{new}^")]), moved, "前の便の上に載る");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 前の便の worktree が在らない（retire 済み＝move 済み）なら列から外れ、後の便は待たない。
#[test]
fn pipe_order_front_run_without_worktree_leaves_the_queue() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let front = worktree_of(&repo, &id_a);
    let retired = repo.join(".worktrees").join("scribe2").join("retired").join(&id_a);
    fs::create_dir_all(retired.parent().unwrap_or(&repo)).expect("retired の親を作れる");
    git(&repo, &["worktree", "move", &front.display().to_string(), &retired.display().to_string()]);
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "first", "列の前が空: {}", stdout_of(&out));
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 3 便（Gated の ts 順 a < b < c）: a を land した後に b と c の land を背景で撃つと、b が追随 →
/// 撃ち直しの間も b は列の先頭に残り c は待つ（`order=waited:<n>`）。追随は b・c とも **1 回ずつ**
/// （2 回の便が 0＝c が (vi) の `stale base` を踏まない）で、main には a → b → c の順に載る
/// （lens の指摘 2026-09-13T04:05Z の形・撃ち直し中の便が列から外れると c が b と並行に撃ち直す）。
#[test]
fn pipe_order_three_runs_follow_once_each_and_land_in_gated_order() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b, id_c) = three_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let first = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "a の land: {}", stderr_of(&first));
    assert_eq!(order_token(&first), "first", "a は待たない: {}", stdout_of(&first));
    let second = land_in_background(&repo, &state, &id_b, &rules, &lens);
    let third = land_in_background(&repo, &state, &id_c, &rules, &lens);
    let out_b = second.wait_with_output().expect("b の land が終わる");
    let out_c = third.wait_with_output().expect("c の land が終わる");
    assert_eq!(out_b.status.code(), Some(i32::from(RC_OK)), "b の land: {}", stderr_of(&out_b));
    assert_eq!(out_c.status.code(), Some(i32::from(RC_OK)), "c の land: {}", stderr_of(&out_c));
    assert_eq!(order_token(&out_b), "first", "a の着地後の b は列の先頭: {}", stdout_of(&out_b));
    let token = order_token(&out_c);
    let waited: u64 = token.strip_prefix("waited:").and_then(|secs| secs.parse().ok()).unwrap_or(u64::MAX);
    assert!(waited < 30, "c は b が列を空けるまで待ち、上限は待たない（order={token}）: {}", stdout_of(&out_c));
    assert_eq!(exported_order(&state, &id_c), token, "面 5 の record も同じ値");
    assert_eq!(follow_count(&state, &id_b), 1, "b の追随は 1 回: {:?}", stages(&state, &id_b));
    assert_eq!(follow_count(&state, &id_c), 1, "c の追随は 1 回: {:?}", stages(&state, &id_c));
    let (sha_a, sha_b, sha_c) = (landed_token(&first), landed_token(&out_b), landed_token(&out_c));
    assert_eq!(git(&repo, &["rev-parse", &format!("{sha_b}^")]), sha_a, "b は a の上に載る");
    assert_eq!(git(&repo, &["rev-parse", &format!("{sha_c}^")]), sha_b, "c は b の上に載る");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), sha_c, "main の先頭は c");
    clean(&[&repo, &state]);
}

/// 撃ち直しが FAIL になった便（`Gated` のまま verdict が FAIL）は列から外れ、後続は待たずに進む。
/// 「待たなかった」は**待ちの record** で pin する（設計 gate-cost.md §23 形 (1)）: stdout の `order=first` と
/// 面 5（`verdicts.jsonl`）の `order` = `first`（待った周は `waited:<s>`）。壁時計は測らない——負荷下では
/// land 自体（rebase + 再 gate + 主実測）が上限を超えて偽に落ちる。
#[test]
fn pipe_order_regate_fail_leaves_the_queue() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b, id_c) = three_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let first = land_extra(&repo, &state, &id_a, &["--rules", &rules]);
    assert_eq!(first.status.code(), Some(i32::from(RC_OK)), "a の land: {}", stderr_of(&first));
    let fail = fake_lens(&marker, &lens_verdict("FAIL"));
    let failed = land_extra(&repo, &state, &id_b, &["--rules", &rules, "--lens", &fail]);
    assert_ne!(failed.status.code(), Some(i32::from(RC_OK)), "撃ち直しが FAIL なら land しない: {}", stdout_of(&failed));
    assert_eq!(follow_count(&state, &id_b), 1, "b は追随して撃ち直した: {:?}", stages(&state, &id_b));
    assert!(show_line(&repo, &state, &id_b).contains("stage=Gated"), "b は Gated(FAIL) のまま");
    let pass = fake_lens(&marker, &lens_verdict("PASS"));
    let out = land_extra(&repo, &state, &id_c, &["--rules", &rules, "--lens", &pass]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "c の land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "first", "FAIL の b は列に居ない: {}", stdout_of(&out));
    assert_eq!(exported_order(&state, &id_c), "first", "面 5 の record も `first`（待った周は waited:<s>）");
    assert!(show_line(&repo, &state, &id_c).contains("stage=Landed"), "c は Landed");
    clean(&[&repo, &state]);
}

/// `--pr-cmd` の形は列を見ない（main を動かさない）: 前の便が列に居ても待たず、`order=` を出さない。
/// 列を見ない形は面 5 へも `order` を書かないので、「待たなかった」の pin は stdout に `order=` が無いこと
/// だけである（設計 gate-cost.md §23 形 (1)・記録を書かない面に空文字の pin を置いても RED を作れない）。
/// 壁時計は測らない（負荷下で偽に落ちる）。
#[test]
fn pipe_order_pr_cmd_does_not_look_at_the_queue() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (_id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules, "--pr-cmd", "true"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PR の口: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("landed=pr"), "{}", stdout_of(&out));
    assert!(!stdout_of(&out).contains("order="), "列を見ない形は order= を出さない: {}", stdout_of(&out));
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main, "main は動かない");
    clean(&[&repo, &state]);
}

/// 列を導けない周（worktree 在りの `Gated` の便の判定が読めない）は `order=unmeasured` で**進む**
/// （rc 2 にしない＝待ちは deny の関門でない・main 実測の「測れなかった」とは別の極性）。
#[test]
fn pipe_order_unreadable_front_verdict_is_unmeasured_and_lands() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    fs::write(state.join("pipe").join(&id_a).join("verdict.json"), "{broken\n").expect("判定を壊せる");
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "読めない周も進む: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "unmeasured", "stdout の land 行: {}", stdout_of(&out));
    assert_eq!(exported_order(&state, &id_b), "unmeasured", "面 5 の record");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "段は Landed");
    clean(&[&repo, &state]);
}

/// 先頭の便の札が死んだ pid（設計 pipeline.md §36）: 後続の land は待たずに `first` で進み、stdout の `order=` の直後に
/// `skipped-dead=1`、面 5 の `skipped_dead` に先頭の便 id を載せる。外すだけで先頭の段と worktree は動かない。
#[test]
fn pipe_order_dead_front_driver_is_skipped_and_named() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    put_driver_pid(&state, &id_a, dead_pid());
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(30));
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "first", "死んだ先頭は数えない: {}", stdout_of(&out));
    assert!(stdout_of(&out).contains(" order=first skipped-dead=1 "), "order= の直後に外した本数: {}", stdout_of(&out));
    assert_eq!(exported_order(&state, &id_b), "first", "面 5 の order");
    assert_eq!(exported_skipped_dead(&state, &id_b), id_a, "面 5 の skipped_dead は外した便 id");
    assert!(show_line(&repo, &state, &id_b).contains("stage=Landed"), "後続は Landed");
    assert!(show_line(&repo, &state, &id_a).contains("stage=Gated"), "死んだ便の段は動かない");
    assert!(worktree_of(&repo, &id_a).is_dir(), "死んだ便の worktree は残る");
    clean(&[&repo, &state]);
}

/// 先頭の便の札が生きている（所有者 = この test の process）周は従来どおり待つ（上限 1 秒で `degraded`・外した便は
/// 名指さない）。
#[test]
fn pipe_order_dead_live_front_driver_still_waits() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    put_driver_pid(&state, &id_a, std::process::id());
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(LAND_WAIT_S));
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "degraded", "生きている先頭を待つ: {}", stdout_of(&out));
    assert!(!stdout_of(&out).contains("skipped-dead="), "外した便は無い: {}", stdout_of(&out));
    assert_eq!(exported_skipped_dead(&state, &id_b), "", "面 5 に skipped_dead を書かない");
    assert!(show_line(&repo, &state, &id_a).contains("stage=Gated"), "先頭は列に残ったまま");
    clean(&[&repo, &state]);
}

/// 先頭の便に札が無い（`pipe spawn` で起こした便）周も従来どおり待つ（札の無いを「死んだ」に読み替えない）。
#[test]
fn pipe_order_dead_absent_front_driver_still_waits() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let (id_a, id_b) = two_gated_runs(&repo, &state, &marker);
    assert!(!state.join("pipe").join(&id_a).join("driver").exists(), "前提: 先頭は札を持たない");
    let rules = write_rules_land_wait(&state, "rules-order.toml", Some(LAND_WAIT_S));
    let out = land_extra(&repo, &state, &id_b, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land: {}", stderr_of(&out));
    assert_eq!(order_token(&out), "degraded", "札の無い先頭を待つ: {}", stdout_of(&out));
    assert!(!stdout_of(&out).contains("skipped-dead="), "外した便は無い: {}", stdout_of(&out));
    assert_eq!(exported_skipped_dead(&state, &id_b), "", "面 5 に skipped_dead を書かない");
    clean(&[&repo, &state]);
}

/// `pipe.land_wait_s` の行が無い manifest は land を 1 byte も動かさない（rc 2・event 0 増・main 不変）。
#[test]
fn pipe_order_missing_land_wait_row_moves_nothing() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let path = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &path, &marker);
    let rules = write_rules_land_wait(&state, "rules-order.toml", None);
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    let before = event_count(&state);
    let out = land_extra(&repo, &state, &id, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "行の欠落は rc 2: {}", stdout_of(&out));
    assert!(stderr_of(&out).contains("pipe.land_wait_s が無い"), "行を名指す: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main, "main は動かない");
    clean(&[&repo, &state]);
}

/// (§5 land の終端) 偽 remote + 偽 CI（success）+ 偽 adapter で、`Landed` の後ろに **push → CI → close の
/// 3 event**が並び、bead が閉じられる。押した先の main は着地した sha を指す。
#[test]
fn pipe_terminal_land_pushes_checks_ci_and_closes_the_bead() {
    let (repo, state) = repo_with_state();
    let tools = fake_terminal(&repo, &state, "success");
    let marker = state.join("lens-ran");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &marker);
    let bd = state.join("fake-bd.sh").display().to_string();
    // **上限は fixture の manifest から渡す**（埋め込みの 900 s を待たない）: CI を測れなくする変異は
    // ここで速やかに落ちる側に倒れる＝timeout でなく撃墜として数えられる。
    let rules = ceiling_rules(&state);
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "終端まで通った land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("terminal=closed"), "終端の token: {}", stdout_of(&out));
    let landed = git(&repo, &["rev-parse", "refs/heads/main"]);
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec!["terminal:push:fake".to_owned(), "terminal:ci:success".to_owned(), "terminal:close:ok".to_owned()],
        "Landed の後ろに段ごとの 3 件（母集団 {} 件）: {details:?}",
        details.len()
    );
    // **押した先が動いている**（数えただけでは撃ったと言えない）。
    assert_eq!(git(&tools.remote, &["rev-parse", "refs/heads/main"]), landed, "偽 remote の main は着地した sha");
    // **`{sha}` の穴が埋まっている**: CI の行は着地した **40 桁の** sha を名指して撃たれる（短縮 sha だと
    // forge の CLI は完了済みの run でも空を返し続ける）。
    let ci_argv = fs::read_to_string(&tools.ci_log).expect("偽 CI が撃たれた");
    let words: Vec<&str> = ci_argv.lines().collect();
    assert!(words.contains(&landed.as_str()), "argv に着地した sha が入る: {words:?}");
    assert_eq!(landed.len(), 40, "穴に入るのは 40 桁の sha: {landed}");
    assert!(!ci_argv.contains("{sha}"), "穴の字面が残らない: {ci_argv}");
    // **台帳は close の 1 種だけで撃たれる**（起票も acceptance も撃たない）。
    let argv = fs::read_to_string(&tools.bd_log).expect("偽 bd が撃たれた");
    let words: Vec<&str> = argv.lines().collect();
    assert_eq!(words.first().copied(), Some("close"), "subcommand は close: {words:?}");
    assert_eq!(words.get(2).copied(), Some("--reason"), "理由を渡す: {words:?}");
    assert!(words.get(3).is_some_and(|line| line.contains(&landed) && line.ends_with("ci=success")), "理由の中身: {words:?}");
    clean(&[&repo, &state]);
}

/// (設計 pipeline.md 行 ap) 運転手の cwd が**消えた dir**でも、台帳の close は repo を cwd にして撃たれ
/// `terminal:close:ok` まで進む。
///
/// 偽 bd は本物と同じく**台帳を cwd から探す**（`.vessel.toml` が cwd に無ければ rc 1 で断る）うえで cwd を
/// 記録に残す。運転手の cwd を継ぐ実装では、消えた dir から撃たれて `close:failed` で止まる。
#[test]
fn pipe_terminal_land_close_cwd_survives_a_vanished_driver_cwd() {
    let (repo, state) = repo_with_state();
    let _tools = fake_terminal(&repo, &state, "success");
    let cwd_log = state.join("bd-cwd.txt");
    let bd = exec_script(
        &state.join("fake-bd-cwd.sh"),
        &format!(
            "pwd -P > '{}' 2>&1\ntest -e .vessel.toml || {{ echo 'no ledger found from cwd' >&2; exit 1; }}\n",
            cwd_log.display()
        ),
    );
    let marker = state.join("lens-ran");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &marker);
    let rules = ceiling_rules(&state);
    let gone = state.join("vanishing-cwd");
    fs::create_dir_all(&gone).expect("消える dir を作れる");
    let (repo_arg, state_arg) = (repo.display().to_string(), state.display().to_string());
    let args = [
        "land", "--run", id.as_str(), "--repo", repo_arg.as_str(), "--state-dir", state_arg.as_str(),
        "--bd", bd.as_str(), "--rules", rules.as_str(),
    ];
    let mut cmd = pipe_cmd(&args);
    // **後置の cwd が勝つ**（[`bin_cmd`] の tmp dir を上書き）。起こした直後に dir を消す＝子の cwd は消えた dir。
    let child = cmd.current_dir(&gone).spawn().expect("binary を起動できる");
    fs::remove_dir(&gone).expect("子の cwd を消せる");
    assert!(!gone.exists(), "fixture: 子の cwd は消えている");
    let out = child.wait_with_output().expect("子の終わりを待てる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "終端まで通った land は rc 0: {}", stderr_of(&out));
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec!["terminal:push:fake".to_owned(), "terminal:ci:success".to_owned(), "terminal:close:ok".to_owned()],
        "消えた cwd からでも close まで進む（母集団 {} 件）: {details:?}",
        details.len()
    );
    let written = fs::read_to_string(&cwd_log).expect("偽 bd が撃たれた");
    assert_eq!(written.trim_end(), repo.display().to_string(), "偽 bd の cwd は repo");
    clean(&[&repo, &state]);
}

/// (§5 land の終端) CI が **failure** の周は**台帳を閉じない**（rc 1・記録は `ci:failure` で終わる）。
///
/// 着地そのものは取り消さない（main は進んだまま）——止めるのは close であって着地ではない。
#[test]
fn pipe_terminal_land_ci_failure_does_not_close_the_bead() {
    let (repo, state) = repo_with_state();
    let tools = fake_terminal(&repo, &state, "failure");
    let marker = state.join("lens-ran");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &marker);
    let before = git(&repo, &["rev-parse", "refs/heads/main"]);
    let bd = state.join("fake-bd.sh").display().to_string();
    let rules = ceiling_rules(&state);
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert_eq!(out.status.code(), Some(1), "close しなかった周は rc 1: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("terminal=ci:failure"), "終端の token: {}", stdout_of(&out));
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec!["terminal:push:fake".to_owned(), "terminal:ci:failure".to_owned()],
        "close の段は記さない（母集団 {} 件）: {details:?}",
        details.len()
    );
    assert!(!tools.bd_log.exists(), "台帳 client は 1 度も撃たれない");
    // 着地は取り消さない（main は進んだまま・押した先も動いている）。
    assert_ne!(git(&repo, &["rev-parse", "refs/heads/main"]), before, "main は進んだまま");
    clean(&[&repo, &state]);
}

/// 撃ち直しの歯の fixture（設計 contract-source.md §58・行 bm）: CI が failure の着地（close しない）の後に CI を直す
/// （直した偽 CI も argv を写し、呼ばれた回数を数える）。返すのは道具・便・`--bd` と `--rules` の値・着地した sha。
fn replay_fixture(repo: &Path, state: &Path) -> (FakeTerminal, String, [String; 2], String) {
    let tools = fake_terminal(repo, state, "failure");
    let design = write_contract(repo, &[], &[]);
    let id = gated_pass(repo, state, &design, &state.join("lens-ran"));
    let flags = [state.join("fake-bd.sh").display().to_string(), ceiling_rules(state)];
    let first = land_extra(repo, state, &id, &["--bd", &flags[0], "--rules", &flags[1]]);
    assert_eq!(first.status.code(), Some(1), "1 周目は close しない: {}", stderr_of(&first));
    assert!(!tools.bd_log.exists(), "前提: 台帳はまだ閉じていない");
    let landed = git(repo, &["rev-parse", "refs/heads/main"]);
    // CI を直す（宣言は同じ path を指したまま・行は 1 byte も変えない）。
    let (log, calls) = (tools.ci_log.display(), tools.ci_calls.display());
    let body = format!("printf '%s\\n' \"$@\" > '{log}'\necho call >> '{calls}'\nprintf '[{{\"status\":\"completed\",\"conclusion\":\"success\"}}]\\n'\n");
    exec_script(&state.join("fake-ci.sh"), &body);
    (tools, id, flags, landed)
}

/// 終端だけを撃ち直す（`pipe land --terminal-only`・`--bd` と `--rules` は fixture の値）。
fn replay(repo: &Path, state: &Path, id: &str, flags: &[String; 2]) -> Output {
    land_extra(repo, state, id, &["--bd", &flags[0], "--rules", &flags[1], "--terminal-only"])
}

/// anchor の main と偽 remote の main を `sha` へ付け替える（remote の ref は bare repo の中で直に動かす）。
fn point_main(repo: &Path, tools: &FakeTerminal, sha: &str) {
    git(&tools.remote, &["update-ref", "refs/heads/main", sha]);
    git(repo, &["update-ref", "refs/heads/main", sha]);
}

/// (§58 形 3) 着地した sha が main の今の先端の祖先の撃ち直しは、**先端の CI** で照合して close する（着地をやり直さない）。
///
/// CI が failure の便の後に別の commit が main を進め、CI を直して撃ち直す。main は器が動かさず、CI の argv は main の今の
/// sha（着地した sha ではない）、reason は `landed <着地した sha> ci=success tip=<今の sha>`。base は着地した sha を照合する＝RED。
#[test]
fn pipe_replay_tip_behind_checks_the_current_main_tip_and_closes() {
    let (repo, state) = repo_with_state();
    let (tools, id, flags, landed) = replay_fixture(&repo, &state);
    fs::write(repo.join("unrelated.md"), "別の便\n").expect("別の便の file を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "another-run"]);
    let moved = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_ne!(moved, landed, "前提: main は着地した sha から動いた");
    let again = replay(&repo, &state, &id, &flags);
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "継いだ終端は rc 0: {}", stderr_of(&again));
    assert_eq!(stdout_of(&again).trim(), format!("run={id} terminal=closed"), "終端だけの 1 行");
    // **着地はやり直さない**: main は別の便が進めた位置のままで、器は 1 mm も動かさない。
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), moved, "main は器が動かさない");
    assert_eq!(git(&tools.remote, &["rev-parse", "refs/heads/main"]), moved, "偽 remote の main は今の sha");
    // **照合したのは main の今の先端である**（着地した sha には forge の CI の run が付かない）。
    let ci_argv = fs::read_to_string(&tools.ci_log).expect("偽 CI が撃たれた");
    let words: Vec<&str> = ci_argv.lines().collect();
    assert!(words.contains(&moved.as_str()), "CI の argv は main の今の sha: {words:?}");
    assert!(!words.contains(&landed.as_str()), "着地した sha では照合しない: {words:?}");
    // 記録は 1 周目の 2 件に 2 周目の 3 件が続く（段ごとに 1 件・やり直した段も残る）。
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec![
            "terminal:push:fake".to_owned(),
            "terminal:ci:failure".to_owned(),
            "terminal:push:fake".to_owned(),
            "terminal:ci:success".to_owned(),
            "terminal:close:ok".to_owned(),
        ],
        "母集団 {} 件: {details:?}",
        details.len()
    );
    let argv = fs::read_to_string(&tools.bd_log).expect("2 周目で台帳が閉じられた");
    let reason = format!("landed {landed} ci=success tip={moved}");
    assert_eq!(argv.lines().nth(3), Some(reason.as_str()), "理由は着地した sha と先端の sha を名指す: {argv}");
    clean(&[&repo, &state]);
}

/// (§58 形 2 / 3) 1 本の fn の撃ち直し 2 周: main を着地した commit を祖先に持たない commit（同じ木・同じ親）へ動かした
/// 周は CI を撃たず close しない（rc 1・`ci:unmeasurable`）。着地した commit へ戻した周は自分の sha の CI で照合して
/// `tip=` の無い reason で close する。base は 1 周目で着地した sha を照合して close する＝RED。
#[test]
fn pipe_replay_tip_non_ancestor_stays_open_and_own_tip_closes_without_tip() {
    let (repo, state) = repo_with_state();
    let (tools, id, flags, landed) = replay_fixture(&repo, &state);
    let tree = format!("{landed}^{{tree}}");
    let sibling = git(&repo, &["commit-tree", &tree, "-p", &format!("{landed}^"), "-m", "sibling"]);
    assert_ne!(sibling, landed, "前提: 着地した commit と別の commit");
    git(&repo, &["push", "-q", "fake", &format!("{sibling}:refs/heads/sibling")]);
    point_main(&repo, &tools, &sibling);
    let calls = tools.ci_call_count();
    let off = replay(&repo, &state, &id, &flags);
    assert_eq!(off.status.code(), Some(1), "祖先でない周は close しない: {}", stderr_of(&off));
    assert_eq!(stdout_of(&off).trim(), format!("run={id} terminal=ci:unmeasurable"), "終端の 1 行");
    assert_eq!(tools.ci_call_count(), calls, "偽 CI は撃たれない");
    assert!(!tools.bd_log.exists(), "偽 bd は撃たれない");
    point_main(&repo, &tools, &landed);
    let own = replay(&repo, &state, &id, &flags);
    assert_eq!(own.status.code(), Some(i32::from(RC_OK)), "先端そのものの周は close する: {}", stderr_of(&own));
    let ci_argv = fs::read_to_string(&tools.ci_log).expect("偽 CI が撃たれた");
    assert!(ci_argv.lines().any(|word| word == landed), "CI の argv は着地した sha: {ci_argv}");
    let argv = fs::read_to_string(&tools.bd_log).expect("2 周目で台帳が閉じられた");
    let reason = format!("landed {landed} ci=success");
    assert_eq!(argv.lines().nth(3), Some(reason.as_str()), "reason は tip= を持たない: {argv}");
    let tail: Vec<String> = landed_details(&state, &id).into_iter().skip(1).collect();
    let push = "terminal:push:fake";
    let expected = [push, "terminal:ci:failure", push, "terminal:ci:unmeasurable", push, "terminal:ci:success", "terminal:close:ok"];
    assert_eq!(tail, expected, "Landed の後ろの 7 件");
    clean(&[&repo, &state]);
}

/// (§58 形 1) `refs/heads/main` を読めない撃ち直しは先端の側に倒さず、何も書かず何も撃たずに断る（rc 1・stderr 1 行）。
/// base は終端へ進んで `terminal:unreadable` を記す＝RED。
#[test]
fn pipe_replay_tip_unreadable_main_refuses_without_events() {
    let (repo, state) = repo_with_state();
    let (tools, id, flags, _) = replay_fixture(&repo, &state);
    git(&repo, &["update-ref", "-d", "refs/heads/main"]);
    let (events, calls) = (event_count(&state), tools.ci_call_count());
    let out = replay(&repo, &state, &id, &flags);
    assert_eq!(out.status.code(), Some(1), "断りは rc 1: {}", stderr_of(&out));
    assert_eq!(stdout_of(&out), "", "stdout は空");
    assert!(stderr_of(&out).contains("refs/heads/main を読めない"), "断りの 1 行: {}", stderr_of(&out));
    assert_eq!(event_count(&state), events, "event を 1 件も書かない");
    assert_eq!(tools.ci_call_count(), calls, "偽 CI は撃たれない");
    assert!(!tools.bd_log.exists(), "偽 bd は撃たれない");
    clean(&[&repo, &state]);
}

/// (§5 手順 4) record の `generation` は **binary の build 元 commit**（`--version` の括弧の中身と同じ 1 本）で、
/// 同じ行の `sha`（着地した commit）とは**別の値**である。
///
/// 同値の欄を 2 つ並べると、読み手はどちらを版の比較（§12）に使うのか判じられない（C10）。
#[test]
fn pipe_terminal_land_generation_is_the_binary_build_commit_not_the_landed_sha() {
    let (repo, state) = repo_with_state();
    let marker = state.join("lens-ran");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &marker);
    let out = land_extra(&repo, &state, &id, &[]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land は rc 0: {}", stderr_of(&out));
    let pairs = exported_pairs(&state, &id);
    let landed = git(&repo, &["rev-parse", "refs/heads/main"]);
    assert_eq!(value_of(&pairs, "sha"), landed, "sha は着地した commit: {pairs:?}");
    // `--version` の括弧の中身を現物から採る（器の字面を借りずに外形から測る）。
    let version = String::from_utf8_lossy(&bin_cmd().arg("--version").output().expect("--version").stdout)
        .trim()
        .to_owned();
    let generation = version
        .rsplit_once('(')
        .and_then(|(_, tail)| tail.strip_suffix(')'))
        .unwrap_or_default()
        .to_owned();
    assert!(!generation.is_empty(), "--version の括弧の中身を読める: {version}");
    assert_eq!(value_of(&pairs, "generation"), generation, "generation は build 元 commit: {pairs:?}");
    assert_ne!(value_of(&pairs, "generation"), value_of(&pairs, "sha"), "同値の欄を 2 つ並べない: {pairs:?}");
    clean(&[&repo, &state]);
}

/// (§5 手順 2) **落ちた run が 1 本在れば、別の run が走っていても `ci:failure`** である。
///
/// 実 CI では複数の workflow が並ぶので「1 本が落ちた後も別の 1 本が走っている」が常態である。
/// 未完了を先に見る実装は、**測って落ちた事実**を上限いっぱい待った末の `ci:unmeasurable` に化けさせる
/// （C10 の反転）。落ちたと分かった時点で待つ理由は無い。
#[test]
fn pipe_terminal_land_ci_failure_wins_over_a_still_running_workflow() {
    let (repo, state) = repo_with_state();
    let json = "[{\"status\":\"completed\",\"conclusion\":\"failure\"},{\"status\":\"in_progress\",\"conclusion\":null}]";
    let tools = fake_terminal_json(&repo, &state, json);
    let marker = state.join("lens-ran");
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &marker);
    let bd = state.join("fake-bd.sh").display().to_string();
    // **上限は fixture の manifest から渡す**（埋め込みの 900 s を待たない）。落ちた run を先に見ない
    // 実装はここで上限まで待ってから `ci:unmeasurable` を名乗る＝この歯はその差で落ちる。
    let rules = ceiling_rules(&state);
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert_eq!(out.status.code(), Some(1), "close しなかった周は rc 1: {}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains("terminal=ci:failure"),
        "走っている run が同居しても **failure** を名乗る（unmeasurable に化けない）: {}",
        stdout_of(&out)
    );
    assert!(!tools.bd_log.exists(), "台帳 client は 1 度も撃たれない");
    // 負例の対: 同じ形で落ちた run を外すと（走っている run だけ）測れない側へ倒れる。
    // **上限は fixture の manifest から渡す**（埋め込みの 900 s を待たない＝測れない周だけが待つ側である）。
    clean(&[&repo, &state]);
    let (repo, state) = repo_with_state();
    let running = "[{\"status\":\"in_progress\",\"conclusion\":null}]";
    let tools = fake_terminal_json(&repo, &state, running);
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let bd = state.join("fake-bd.sh").display().to_string();
    let rules = ceiling_rules(&state);
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert!(
        stdout_of(&out).contains("terminal=ci:unmeasurable"),
        "走っている run だけの周は測れない側: {}",
        stdout_of(&out)
    );
    assert!(!tools.bd_log.exists(), "測れない周も台帳は閉じない");
    clean(&[&repo, &state]);
}

/// (pipeline.md §46) 同じ sha で cron の run（`event=schedule`）が走っていても、**それを待たずに**
/// push の run の success で終端が close まで進む。
///
/// cron の run を数える実装は、走っている schedule の run の完了を上限いっぱい待って `ci:unmeasurable` に倒れる。
#[test]
fn pipe_terminal_land_ci_ignores_scheduled_runs() {
    let (repo, state) = repo_with_state();
    let json = "[{\"status\":\"completed\",\"conclusion\":\"success\",\"event\":\"push\"},{\"status\":\"in_progress\",\"conclusion\":null,\"event\":\"schedule\"}]";
    let tools = fake_terminal_json(&repo, &state, json);
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let bd = state.join("fake-bd.sh").display().to_string();
    // **上限は fixture の manifest から渡す**（埋め込みの 900 s を待たない）。
    let rules = ceiling_rules(&state);
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "close まで通った land は rc 0: {}", stderr_of(&out));
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec!["terminal:push:fake".to_owned(), "terminal:ci:success".to_owned(), "terminal:close:ok".to_owned()],
        "schedule の run を待たない（母集団 {} 件）: {details:?}",
        details.len()
    );
    assert!(tools.bd_log.exists(), "bead が閉じられた");
    clean(&[&repo, &state]);
}

/// (pipeline.md §46) run が cron（`event=schedule`）だけの周は、外した後に 0 本＝**測れない**。
///
/// 外した後の空を success に倒すと、着地した commit の CI を 1 本も見ずに bead を閉じる（C10 の反転）。
#[test]
fn pipe_terminal_land_ci_only_scheduled_runs_is_unmeasurable() {
    let (repo, state) = repo_with_state();
    let json = "[{\"status\":\"completed\",\"conclusion\":\"success\",\"event\":\"schedule\"}]";
    let tools = fake_terminal_json(&repo, &state, json);
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let bd = state.join("fake-bd.sh").display().to_string();
    let rules = ceiling_rules(&state);
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    assert_eq!(out.status.code(), Some(1), "close しなかった周は rc 1: {}", stderr_of(&out));
    let details = landed_details(&state, &id);
    assert_eq!(
        details.iter().skip(1).cloned().collect::<Vec<String>>(),
        vec!["terminal:push:fake".to_owned(), "terminal:ci:unmeasurable".to_owned()],
        "schedule の run だけでは測れない（母集団 {} 件）: {details:?}",
        details.len()
    );
    assert!(!tools.bd_log.exists(), "測れない周は台帳を閉じない");
    clean(&[&repo, &state]);
}

/// 終端の CI の照合の歯の fixture（偽 CI が `json` を返す便を Gated(PASS) まで進め、CI の 2 行を差し替えた manifest で
/// land を撃つ）。land の出力・撃ってから返るまでの時間・道具一式を返す。
fn land_with_ci_poll(json: &str, ci_wait_s: u64, ci_poll_s: u64) -> (Output, std::time::Duration, FakeTerminal, [PathBuf; 2]) {
    let (repo, state) = repo_with_state();
    let tools = fake_terminal_json(&repo, &state, json);
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let bd = state.join("fake-bd.sh").display().to_string();
    let rules = write_rules_ci(&state, ci_wait_s, Some(ci_poll_s));
    let started = std::time::Instant::now();
    let out = land_extra(&repo, &state, &id, &["--bd", &bd, "--rules", &rules]);
    (out, started.elapsed(), tools, [repo, state])
}

/// 走り続ける CI の JSON（照合は満たされない＝上限まで待つ）。
const CI_RUNNING: &str = "[{\"status\":\"in_progress\",\"conclusion\":null}]";

/// (§50 形 2) 照合は rules 行 `pipe.ci_poll_s` の間隔で撃つ: 上限 2 秒・間隔 1 秒の周は `terminal=ci:unmeasurable` で、
/// 偽 CI の呼び出しは 2 回以上 4 回以下（待ちの中の最初・1 秒・上限と、終端の読み直しの 1 回・20 ms の周期なら数十回）。
#[test]
fn pipe_terminal_ci_poll_interval_bounds_the_ci_calls() {
    let (out, _, tools, dirs) = land_with_ci_poll(CI_RUNNING, 2, 1);
    assert!(stdout_of(&out).contains("terminal=ci:unmeasurable"), "走り続ける CI は測れない: {}", stdout_of(&out));
    let calls = tools.ci_call_count();
    assert!((2..=4).contains(&calls), "照合は間隔 1 秒で撃つ（20 ms で撃たない）: {calls} 回");
    assert!(!tools.bd_log.exists(), "測れない周は台帳を閉じない");
    clean(&[&dirs[0], &dirs[1]]);
}

/// (§50 形 3) 最初の評価は眠る前: 最初から success の CI は間隔 30 秒（上限 60 秒）でも待たずに `terminal=closed`
/// まで 10 秒未満で進み、照合は 1 回。
#[test]
fn pipe_terminal_ci_poll_first_check_is_before_the_sleep() {
    let json = "[{\"status\":\"completed\",\"conclusion\":\"success\"}]";
    let (out, took, tools, dirs) = land_with_ci_poll(json, 60, 30);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "close まで通った land は rc 0: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("terminal=closed"), "終端の token: {}", stdout_of(&out));
    assert!(took < std::time::Duration::from_secs(10), "間隔ぶん眠ってから照合しない: {took:?}");
    assert_eq!(tools.ci_call_count(), 2, "待ちの最初の 1 回と終端の読み直しの 1 回");
    clean(&[&dirs[0], &dirs[1]]);
}

/// (§50 形 4) 上限を越えて眠らない: 走り続ける CI・上限 1 秒・間隔 30 秒の周は `terminal=ci:unmeasurable` まで 10 秒未満。
#[test]
fn pipe_terminal_ci_poll_sleep_never_overruns_the_limit() {
    let (out, took, tools, dirs) = land_with_ci_poll(CI_RUNNING, 1, 30);
    assert!(stdout_of(&out).contains("terminal=ci:unmeasurable"), "上限で測れない側: {}", stdout_of(&out));
    assert!(took < std::time::Duration::from_secs(10), "上限の後に間隔ぶん眠らない: {took:?}");
    assert!(!tools.bd_log.exists(), "測れない周は台帳を閉じない");
    clean(&[&dirs[0], &dirs[1]]);
}

/// (§50 形 1) `pipe.ci_poll_s` の行が無い manifest は `pipe.ci_wait_s` と同じ極性で断る（rc 2・行を名指す・event 0 増・
/// main 不変）。
#[test]
fn pipe_terminal_ci_poll_missing_row_moves_nothing() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let id = gated_pass(&repo, &state, &design, &state.join("lens-ran"));
    let rules = write_rules_ci(&state, CI_WAIT_S, None);
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    let before = event_count(&state);
    let out = land_extra(&repo, &state, &id, &["--rules", &rules]);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "行の欠落は rc 2: {}", stdout_of(&out));
    assert!(stderr_of(&out).contains("pipe.ci_poll_s が無い"), "行を名指す: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before, "event を 1 件も書かない");
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), main, "main は動かない");
    clean(&[&repo, &state]);
}
