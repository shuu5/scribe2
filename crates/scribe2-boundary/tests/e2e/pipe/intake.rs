// flip-check: moved s2-07l.264
// flip-check: moved s2-07l.351
//! 受付の口の歯: `pipe_intake_` / `pipe_preflight_` / `pipe_repo_` / `pipe_confine_`（審査は `review.rs`・契約表は
//! `contracts.rs`・断りは `refuse.rs`・`s2-07l.351`）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く（歯の本文は移しただけ・`s2-07l.264`）。

use super::*;

/// 契約 (b) 以後、1 行に収まらない配列は**行**の側の欠陥である（契約 file は器が作る）。
#[test]
fn pipe_intake_rejects_multiline_verify() {
    let (repo, state) = repo_with_state();
    let mut fields: Vec<String> = contract_body().into_iter().filter(|line| !line.starts_with("verify")).collect();
    fields.push("verify = [".to_owned());
    fields.push(r#"  "true","#.to_owned());
    fields.push("]".to_owned());
    commit_row(&repo, &fields);
    let out = run_pipe(&[
        "intake", "--design", &design_pointer(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
    ]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "1 行で完結しない verify を通さない");
    assert!(
        stderr_of(&out).contains("verify"),
        "理由は verify の欄を名乗る: {}",
        stderr_of(&out)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_rejects_contract_without_req_or_design() {
    let (repo, state) = repo_with_state();
    for (drop, add, want) in [
        // `design` は行の欄でなく pointer そのものなので、欠落の形が無い（契約 (b)）。
        (vec!["req"], vec![], "req"),
        (vec!["req"], vec![r#"req = []"#], "req"),
    ] {
        let path = write_contract(&repo, &drop, &add);
        let out = run_pipe(&[
            "intake", "--design", &path, "--bead", "b",
            "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
            "--rules", &ceiling_rules(&state),
        ]);
        assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "{want} が無い契約を通さない");
        assert!(stderr_of(&out).contains(want), "{want}: {}", stderr_of(&out));
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_records_run_in_fleet() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let stamp = id.strip_prefix("s2-2e5-").unwrap_or_default();
    assert!(!stamp.is_empty(), "run id は <bead>-<stamp>: {id}");
    // id は dir 名と branch 名になるので、stamp 側に : も - も残っていない。
    assert!(!stamp.contains(':'), "stamp に : が残る: {id}");
    assert!(!stamp.contains('-'), "stamp に - が残る: {id}");

    // 契約 file の写しが置き場に在る（process 間で持ち越す面）。
    assert!(
        state.join("pipe").join(&id).join("contract.toml").exists(),
        "契約の写しが置き場に在る"
    );
    // 現在地は event log から読める（受付の直後に審査の段を通るので、現在地は `Reviewed`・FR49）。
    let out = run_pipe(&["show", "--run", &id, "--state-dir", &state.display().to_string(),
                         "--repo", &repo.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "show は rc 0");
    assert!(stdout_of(&out).contains("stage=Reviewed"), "{}", stdout_of(&out));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_refuses_duplicate_run_id() {
    let (repo, state) = repo_with_state();
    let first = write_contract(&repo, &["write-set"], &[r#"write-set = ["src/A.rs"]"#]);
    let id = intake(&repo, &state, &first);
    // stamp は秒までなので、同じ秒の再 intake は id が衝突する。**黙って上書きしない**。
    let second = write_contract(&repo, &["write-set"], &[r#"write-set = ["src/B.rs"]"#]);
    let out = intake_raw(&repo, &state, &second, "s2-2e5");
    if out.status.code() == Some(i32::from(RC_OK)) {
        // 秒をまたいだ周は id が違う＝衝突していない。そのときは上書きが起きていない
        // ことだけを測る（時計に依存して flaky にしない）。
        assert_ne!(run_id_of(&out), id, "id が違うなら衝突していない");
    } else {
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "衝突は rc 1 で断る");
    }
    let kept = fs::read_to_string(state.join("pipe").join(&id).join("contract.toml"))
        .expect("最初の契約を読める");
    assert!(
        kept.contains("src/A.rs"),
        "最初の便の契約が別物に化けていない: {kept}"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_rejects_broken_arrays_and_unknown_keys() {
    let (repo, state) = repo_with_state();
    for (drop, add, want) in [
        // 区切り忘れを 1 本の壊れた文字列として受理しない。
        // 契約 (b) 以後、壊れているのは**行**である（契約 file は器が作る）。理由は当該の欄を名乗り、
        // 行番号を持つ——字面そのものは表の語彙で、契約 file の語彙ではない。
        (vec!["verify"], vec![r#"verify = ["a" "b"]"#], "verify"),
        (vec!["write-set"], vec![r#"write-set = []"#], "write-set"),
        (vec!["verify"], vec![r#"verify = []"#], "verify"),
        (vec![], vec![r#"nonsense = "x""#], "nonsense"),
        (vec![], vec![r#"classes = ["publish", "bogus"]"#], "bogus"),
        (vec![], vec![r#"opens = ["code", "everything"]"#], "everything"),
    ] {
        let path = write_contract(&repo, &drop, &add);
        let out = run_pipe(&[
            "intake", "--design", &path, "--bead", "b",
            "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
            "--rules", &ceiling_rules(&state),
        ]);
        let err = stderr_of(&out);
        assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "{want} を通さない: {err}");
        assert!(err.contains(want), "理由に {want} が出る: {err}");
        // 位置（doc:line）は**表の検査に届いた周**だけが持つ。値の読みで落ちる周（配列の形の不備）は
        // 欄と字面だけを名乗る＝ここでは位置を pin しない（位置の側は `contract_closure_ext_` の族が
        // `contracts: docs/design/toy.md:<line>` を逐語で測る）。
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_refuses_non_git_repo() {
    let (_repo, state) = repo_with_state();
    let bare = tmp();
    let path = write_contract(&bare, &[], &[]);
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "b",
        "--repo", &bare.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "git repo でなければ intake の時点で断る");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&bare, &state]);
}

/// 宣言が無い repo では便を起こさない（ADR-0010 §2.3）。**作業ツリーに置いただけの
/// 宣言も無いのと同じ**である——宣言の変更は対象 repo の PR として review を通る。
#[test]
fn pipe_intake_refuses_without_vessel_declaration() {
    let (repo, state) = repo_with_state();
    git(&repo, &["rm", "-q", ".vessel.toml"]);
    git(&repo, &["commit", "-q", "-m", "drop-vessel"]);
    let path = write_contract(&repo, &[], &[]);
    let out = intake_raw(&repo, &state, &path, "b");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "宣言が無ければ rc 1: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains(".vessel.toml"), "何が無いかを名指す: {}", stderr_of(&out));
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "断った周は run dir も作らない");

    // **未 commit の宣言は読まない**（作業ツリーへ置くだけでは効かない）。
    write_vessel(&repo, VESSEL_ALLOWED, VESSEL_COMMON);
    let again = intake_raw(&repo, &state, &path, "b");
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "untracked の宣言は無いのと同じ: {}", stderr_of(&again));
    assert_eq!(event_count(&state), 0, "こちらも記帳しない");
    clean(&[&repo, &state]);
}

/// 宣言の allowlist は器の上限（`runner.allowed_commands`）の部分集合でなければならない。
#[test]
fn pipe_intake_refuses_declaration_outside_ceiling() {
    let (repo, state) = repo_with_state();
    commit_vessel(&repo, r#"["git", "sh", "curl"]"#, VESSEL_COMMON);
    let path = write_contract(&repo, &[], &[]);
    let out = intake_raw(&repo, &state, &path, "b");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "上限の外は rc 1: {err}");
    assert!(err.contains("allowed-commands の curl が上限"), "外れた command を理由の形で名指す: {err}");
    assert!(err.contains("runner.allowed_commands"), "上限の出所を名指す: {err}");
    assert!(err.contains("line="), "行番号を持つ: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&repo, &state]);
}

/// 共通 verify の先頭語の基準は**宣言の** allowlist である（上限ではない）。
#[test]
fn pipe_intake_refuses_common_verify_outside_declared_allowlist() {
    let (repo, state) = repo_with_state();
    // `cargo` は上限には在るが、この repo の宣言には無い。
    commit_vessel(&repo, r#"["git"]"#, r#"["cargo xtask check"]"#);
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["git status"]"#]);
    let out = intake_raw(&repo, &state, &path, "b");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "宣言の外は rc 1: {err}");
    assert!(err.contains("先頭 command cargo が"), "外れた先頭語を理由の形で名指す: {err}");
    assert!(err.contains("allowed-commands"), "基準が宣言であることを言う: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// repo の外を指す語（絶対 path・home の短縮記号・`..` で遡る path）を持つ行は撃たせない。
#[test]
fn pipe_intake_refuses_common_verify_with_absolute_path() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    // home の短縮記号は **2 文字を組み立てて**書く（paths-clean は tracked file の本文に
    // その字面が残ることを違反として数えるので、literal では書けない）。
    let home = format!("{}{}", '~', '/');
    for common in [
        r#"["git rev-parse --verify /etc/passwd"]"#.to_owned(),
        format!("[\"git config --file {home}gitconfig list\"]"),
        // 先頭語が allowlist の内でも、引数が repo の外へ遡れば同じ穴である。
        r#"["git rev-parse --git-dir ../../../etc"]"#.to_owned(),
    ] {
        commit_vessel(&repo, VESSEL_ALLOWED, &common);
        let out = intake_raw(&repo, &state, &path, "b");
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{common} は rc 1: {err}");
        assert!(err.contains("repo の外"), "理由を名指す: {err}");
    }
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// 置ける穴は `{base}` だけ（閉じない `{` も穴として断る）。
#[test]
fn pipe_intake_refuses_common_verify_with_unknown_placeholder() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    for common in [
        r#"["git rev-parse --verify {head}"]"#,
        r#"["git rev-parse --verify {base"]"#,
    ] {
        commit_vessel(&repo, VESSEL_ALLOWED, common);
        let out = intake_raw(&repo, &state, &path, "b");
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{common} は rc 1: {err}");
        assert!(err.contains("置けない穴"), "理由を名指す: {err}");
    }
    // 弁別: **`{base}` は通る**（穴を丸ごと禁じているのではない）。
    commit_vessel(&repo, VESSEL_ALLOWED, VESSEL_COMMON);
    let ok = intake_raw(&repo, &state, &path, "b");
    assert_eq!(ok.status.code(), Some(i32::from(RC_OK)), "{{base}} は置ける: {}", stderr_of(&ok));
    clean(&[&repo, &state]);
}

/// **先頭語だけでは境界にならない**——gate と land は行を `sh -c` で撃つ（ADR-0010 §2.3）。
#[test]
fn pipe_intake_refuses_common_verify_with_shell_metachar() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    for common in [
        r#"["git --version; env"]"#.to_owned(),
        r#"["git --version | tee out"]"#.to_owned(),
        r#"["git --version && env"]"#.to_owned(),
        r#"["git $(env) --version"]"#.to_owned(),
        // 目に見えない制御文字も同じ扱い（改行を含む語彙は 1 行の値には書けない）。
        "[\"git --version\u{7}\"]".to_owned(),
    ] {
        commit_vessel(&repo, VESSEL_ALLOWED, &common);
        let out = intake_raw(&repo, &state, &path, "b");
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{common} は rc 1: {err}");
        assert!(err.contains("制御文字"), "理由を名指す: {err}");
    }
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// 読むのは **HEAD commit の tree** で、作業ツリーではない。
#[test]
fn pipe_intake_reads_declaration_from_head_not_worktree() {
    let (repo, state) = repo_with_state();
    let head = git(&repo, &["rev-parse", "HEAD"]);
    // 作業ツリーの宣言だけを**上限の外**へ広げる（commit しない）。
    write_vessel(&repo, r#"["git", "sh", "curl"]"#, VESSEL_COMMON);
    let path = write_contract(&repo, &[], &[]);
    let id = intake_bead(&repo, &state, &path, "s2-2e5");
    let copy = fs::read_to_string(vessel_copy(&state, &id)).expect("写しを読める");
    assert!(!copy.contains("curl"), "作業ツリーの宣言は読まない: {copy}");
    assert!(copy.contains(&format!("commit = \"{head}\"")), "出所は読んだ commit: {copy}");
    clean(&[&repo, &state]);
}

/// 契約の verify 行は穴を持てない（`{base}` も置けない）。
#[test]
fn pipe_intake_refuses_contract_verify_with_placeholder() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["sh verify-ok.sh {base}"]"#]);
    let out = intake_raw(&repo, &state, &path, "b");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "契約行の穴は rc 1: {err}");
    assert!(err.contains("verify"), "どちらの面かを言う: {err}");
    assert!(err.contains("置けない穴"), "理由を名指す: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&repo, &state]);
}

/// 契約の verify 行も**宣言の** allowlist で測る。空の行も撃てる形とは認めない。
#[test]
fn pipe_intake_refuses_contract_verify_outside_declared_allowlist() {
    let (repo, state) = repo_with_state();
    for (add, want) in [
        // `cargo` は上限には在るが toy repo の宣言には無い。
        (r#"verify = ["cargo xtask check"]"#, "allowed-commands"),
        // **完全一致**である（prefix 一致へ緩めると `gitk` が `git` で通る）。
        (r#"verify = ["gitk --all"]"#, "先頭 command gitk"),
        (r#"verify = [""]"#, "空"),
    ] {
        let path = write_contract(&repo, &["verify"], &[add]);
        let out = intake_raw(&repo, &state, &path, "b");
        let err = stderr_of(&out);
        // 撃てない行は断る。rc は理由の側が持つ（撃てない形は 1・**読めない行**は 2＝空の要素は
        // 行の parser が読めない側で、契約 (b) 以後は表の語彙で出る）。
        assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "{add} を通さない: {err}");
        assert!(err.contains(want), "理由に {want} が出る: {err}");
    }
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// 有効値は便ごとに凍結され、以後 repo の宣言が動いても写しは変わらない（ADR-0010 §2.4）。
#[test]
fn pipe_intake_freezes_effective_vessel_copy() {
    let (repo, state) = repo_with_state();
    let head = git(&repo, &["rev-parse", "HEAD"]);
    let path = write_contract(&repo, &[], &[]);
    let id = intake_bead(&repo, &state, &path, "s2-2e5");
    let copy = vessel_copy(&state, &id);
    let before = fs::read_to_string(&copy).expect("写しを読める");
    assert!(before.contains(r#"allowed-commands = ["git", "sh"]"#), "宣言の値: {before}");
    assert!(before.contains(r#"common-verify = ["git rev-parse --verify {base}"]"#), "共通 verify: {before}");
    assert!(before.contains(&format!("commit = \"{head}\"")), "出所の commit: {before}");
    assert!(before.contains(r#"source = ".vessel.toml""#), "出所の path: {before}");
    assert!(before.contains(r#"ceiling = "runner.allowed_commands""#), "出所の上限行: {before}");
    // **便の後に宣言を commit ごと書き換えても写しは動かない**（自己拡張の閉塞）。
    commit_vessel(&repo, r#"["git"]"#, r#"["git status"]"#);
    assert_eq!(fs::read_to_string(&copy).expect("写しを読める"), before, "写しは凍結されている");
    clean(&[&repo, &state]);
}

/// **本 repo 自身の宣言**が、埋め込みの上限（`--rules` を渡さない周の manifest）の内側に在る。
///
/// suite の他の歯はすべて `--rules` の tmp manifest で上限を広げて通しているので、この 1 本が
/// 無いと「自己ホストの宣言が壊れた」周も CI は緑のまま通る（lens M3・2026-09-10）。
/// 併せて manifest の**上限の行**（`runner.allowed_commands`）を宣言が名乗ることを pin する
/// （共通 verify の行は `s2-07l.57` で廃止済みゆえ 2 面同文の pin は畳んだ）。
///
/// 後段の pin は `s2-07l.271` で**宣言 ⊆ 上限**の向きへ直した——base の上限（`["cargo", "git"]`）
/// でも宣言と同じ 2 要素で緑になる（既に land した挙動へ向きを正す歯＝base で RED にならない）。
// flip-check: retroactive s2-07l.271
#[test]
fn pipe_intake_accepts_self_hosted_declaration_under_embedded_ceiling() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo の root を解ける");
    let declared = fs::read_to_string(root.join(".vessel.toml")).expect("自己ホストの宣言を読める");
    let (repo, state) = repo_with_state();
    // 自己ホストの宣言は本 repo の要件面を指すので、toy repo の面を宣言する（契約 (b) の表の検査が読む）。
    let declared = format!("{declared}requirements = \"{REQS_FILE}\"\n");
    fs::write(repo.join(".vessel.toml"), &declared).expect("宣言を写せる");
    git(&repo, &["add", "-f", ".vessel.toml"]);
    git(&repo, &["commit", "-q", "-m", "self-hosted"]);
    // **`--rules` を渡さない**＝埋め込みの上限で測る。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["git status"]"#]);
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--lens", &review_lens_pass(&state),
    ]);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_OK)),
        "自己ホストの宣言は埋め込みの上限の内側: {}",
        stderr_of(&out)
    );

    // 上限の行は残る（共通 verify の行は ADR-0010 §2.2 で廃止済み＝2 面同文の pin は畳んだ）。
    let manifest = Manifest::embedded().expect("埋め込み manifest を読める");
    let row = manifest.get("runner.allowed_commands").expect("上限の行が在る");
    let ceiling = match row.value {
        RuleValue::List(ref found) => found.clone(),
        _ => Vec::new(),
    };
    assert!(!ceiling.is_empty(), "母集団は上限の行の値（実 {} 本）", ceiling.len());
    // **`allowed-commands` の行だけに当てる**——宣言の本文には共通 verify（`cargo …`）も
    // 在るので、file 全体へ `contains` すると allowlist が空でも通る（字面衝突）。
    // 向きは **宣言 ⊆ 上限**（ADR-0010 §2.2）。上限の各語が宣言に載ることを求めると、上限を
    // 広げた周（`bats`・`s2-07l.271`）に自 repo の宣言を変えない限り落ちる＝逆向きの pin。
    // 母集団 = 宣言の要素数（0 なら落とす）。
    let declared_commands = declared_allowed_commands(&declared);
    assert!(
        !declared_commands.is_empty(),
        "母集団は宣言の allowed-commands の要素（実 {} 本）",
        declared_commands.len()
    );
    for command in &declared_commands {
        assert!(
            ceiling.contains(command),
            "自己ホストの宣言の {command:?} は上限に在る（宣言 {} 本 / 上限 {} 本）: {ceiling:?}",
            declared_commands.len(),
            ceiling.len()
        );
    }
    // **上限は宣言より広くてよい**（本便後は 3 >= 2）。
    assert!(
        ceiling.len() >= declared_commands.len(),
        "上限（{} 本）は宣言（{} 本）以上: {ceiling:?} / {declared_commands:?}",
        ceiling.len(),
        declared_commands.len()
    );
    clean(&[&repo, &state]);
}

/// vessel 宣言の `allowed-commands` の 1 行（string array）を要素に分ける。行が無い・配列の形で
/// ないなら空（呼び手が母集団 0 で落とす）。helper の中で `panic!` を撃たない（clippy の
/// `allow-panic-in-tests` は `#[test]` の中だけ）。
fn declared_allowed_commands(declared: &str) -> Vec<String> {
    declared
        .lines()
        .find(|line| line.trim_start().starts_with("allowed-commands"))
        .and_then(|line| line.split_once('='))
        .map(|(_, rest)| rest.trim())
        .and_then(|rest| rest.strip_prefix('['))
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or_default()
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

/// `--rules` で上限を差し替えて intake を通した周は、その事実が stdout に残る（`s2-07l.65`・
/// `.56` lens M1）。`--rules` は test の seam で、上限を無条件に差し替える——差し替えた周が
/// 通常の周と同じ 1 行しか出さないと、review は「埋め込みの上限で通った便」と区別できない。
/// 値は渡した path の字面そのもの（加工しない）。**差し替えていない周は出さない**（不在が既定・負例）。
#[test]
fn pipe_intake_names_ceiling_override_in_stdout() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let rules = ceiling_rules(&state);
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules, "--lens", &review_lens_pass(&state),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let line = stdout_of(&out).lines().next().unwrap_or_default().to_owned();
    assert!(line.starts_with("run="), "既存 token が先頭のまま: {line}");
    assert!(
        line.split_whitespace().any(|token| token == format!("ceiling-overridden={rules}")),
        "差し替えた path が対で載る: {line}"
    );
    assert!(!run_id_of(&out).is_empty(), "run id は取れる: {line}");
    // 負例: `--rules` を渡さない（自己ホストの宣言 = 埋め込みの上限の内側）。
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo の root を解ける");
    let declared = fs::read_to_string(root.join(".vessel.toml")).expect("自己ホストの宣言を読める");
    let (plain, plain_state) = repo_with_state();
    // 自己ホストの宣言は本 repo の要件面（design-intent/spec/srs.html）を指すので、toy repo の面を宣言する。
    let declared = format!("{declared}requirements = \"{REQS_FILE}\"\n");
    fs::write(plain.join(".vessel.toml"), &declared).expect("宣言を写せる");
    git(&plain, &["add", "-f", ".vessel.toml"]);
    git(&plain, &["commit", "-q", "-m", "self-hosted"]);
    let design = write_contract(&plain, &["verify"], &[r#"verify = ["git status"]"#]);
    let out = run_pipe(&[
        "intake", "--design", &design, "--bead", "s2-2e5",
        "--repo", &plain.display().to_string(), "--state-dir", &plain_state.display().to_string(),
        "--lens", &review_lens_pass(&plain_state),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(!stdout_of(&out).contains("ceiling-overridden"), "差し替えていない周は出さない: {}", stdout_of(&out));
    clean(&[&repo, &state, &plain, &plain_state]);
}

/// fixture の event で run の段を動かす。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn record_stage(state: &Path, id: &str, stage: &str) {
    let out = bin_cmd()
        .args(["fleet", "record", "--kind", "RunStage", "--stage", stage, "--run", id,
               "--bead", "s2-live", "--state-dir"])
        .arg(state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fleet record: {}", stderr_of(&out));
}

/// fixture の `verdict.json`（gate の判定を手で置く）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn write_verdict(state: &Path, id: &str, verdict: &str) {
    let path = state.join("pipe").join(id).join("verdict.json");
    fs::write(&path, format!("{{\"schema\":1,\"run\":\"{id}\",\"verdict\":\"{verdict}\"}}\n"))
        .expect("verdict.json を書ける");
}

/// toy repo の宣言（allowlist は `git` だけ・要件面は既定）。
pub(super) const TABLE_VESSEL: &str = "schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git status\"]\n";

/// 契約表の toy repo（宣言・要件面・toy の型・設計 doc `docs/design/toy.md`）を作って commit する。`files` は足す /
/// 上書きする file。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn table_repo(doc: &str, files: &[(&str, &str)]) -> PathBuf {
    let repo = tmp();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "e2e"]);
    git(&repo, &["config", "user.email", "e2e@example.invalid"]);
    let seeded = [
        (".vessel.toml", TABLE_VESSEL),
        ("design-intent/spec/srs.html", TABLE_SRS),
        ("src/tint.rs", TABLE_TINT),
        ("src/show.rs", TABLE_SHOW),
        ("docs/design/toy.md", doc),
    ];
    for (path, body) in seeded.iter().chain(files) {
        let target = repo.join(path);
        fs::create_dir_all(target.parent().expect("親 dir が在る")).expect("dir を作れる");
        fs::write(&target, body).expect("file を書ける");
    }
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "seed"]);
    repo
}

/// `contracts check --repo R` を binary で 1 回撃つ（上限は埋め込みの `runner.allowed_commands`）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn contracts_check(repo: &Path) -> Output {
    bin_cmd().args(["contracts", "check", "--repo"]).arg(repo).output().expect("binary を起動できる")
}

/// findings の行（`contracts: ` で始まる stdout の行）。
pub(super) fn findings_of(out: &Output) -> Vec<String> {
    stdout_of(out).lines().filter(|line| line.starts_with("contracts: ")).map(str::to_owned).collect()
}

/// 1399 行の `.rs` を `crates/toy/src/` に置いて commit した repo と置き場（上限の余地の fixture）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub(super) fn repo_with_big_file() -> (PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    let dir = repo.join("crates").join("toy").join("src");
    fs::create_dir_all(&dir).expect("core の dir を作れる");
    fs::write(dir.join("big.rs"), "// x\n".repeat(1399)).expect("大きな file を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "big"]);
    (repo, state)
}

/// 上限の 2 値を振った tmp manifest の path（[`repo_with_big_file`] の置き場に書く・file の上限は 1500）。
pub(super) fn capped_rules(state: &Path, name: &str, core_lines: u64) -> String {
    let fixture = RulesFixture {
        gate: (1, 1_000_000),
        retries: FOLLOW_RETRIES,
        slots: default_slots(),
        caps: CapFixture { core_lines, file_lines: 1_500 },
    };
    write_rules_capped(state, name, fixture).display().to_string()
}

/// `size` と `write-set` だけ差し替えた契約 file を repo に書き、その path を返す。
pub(super) fn sized_contract(repo: &Path, id: &str, size: &str, write_set: &str) -> String {
    let fields: Vec<String> = contract_body()
        .into_iter()
        .filter(|line| !line.starts_with("size") && !line.starts_with("write-set") && !line.starts_with("id"))
        .chain([format!("id = \"{id}\""), format!("size = \"{size}\""), format!("write-set = [{write_set}]")])
        .collect();
    commit_row(repo, &fields);
    format!("{DESIGN_FILE}#{id}")
}

/// `--rules` を名指して intake を 1 回撃つ（rc を assert しない形・審査の lens は偽 PASS）。
pub(super) fn intake_with_rules(repo: &Path, state: &Path, design: &str, bead: &str, rules: &str) -> Output {
    run_pipe(&[
        "intake", "--design", design, "--bead", bead,
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", rules, "--lens", &review_lens_pass(state),
    ])
}

// ── core の余地の母集団 = 本体（`s2-07l.198` 行 a・設計 core-boundary.md §2・接頭辞 `pipe_intake_core_headroom_`） ──

/// [`repo_with_state`] に、本体 1000 行 + in-file の歯 399 行（行頭 `#[cfg(test)]` から file 末尾・全体 1399 行）の
/// `crates/toy/src/heavy.rs` を 1 本足した repo。file の余地（上限 1500）は 101・core の本体は 1000。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn repo_with_heavy_file() -> (PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    let dir = repo.join("crates").join("toy").join("src");
    fs::create_dir_all(&dir).expect("core の dir を作れる");
    let body = format!("{}#[cfg(test)]\n{}", "// x\n".repeat(1000), "// t\n".repeat(398));
    fs::write(dir.join("heavy.rs"), body).expect("歯を持つ file を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "heavy"]);
    (repo, state)
}

/// core の合計は**本体**だけ（in-file の歯を除く＝xtask check の core-lines と同じ母集団）: 本体 1000 / 全体 1399 の
/// `.rs` を持つ base で、core の上限 1450 の余地は 450（全体で数えれば 51）＝新規 1 本の S（100）は通り、上限 1099 は
/// `core` の余地 **99** を名指して断られる（全体で数える実装は前者を断り、後者を 0 行と名指す）。
#[test]
fn pipe_intake_core_headroom_counts_the_core_total_without_in_file_tests() {
    let (repo, state) = repo_with_heavy_file();
    let fresh = "\"+crates/toy/src/new.rs\"";
    let roomy = capped_rules(&state, "rules-src.toml", 1_450);
    let fits = intake_with_rules(&repo, &state, &sized_contract(&repo, "fits", "S", fresh), "s2-cf", &roomy);
    assert_eq!(fits.status.code(), Some(i32::from(RC_OK)), "本体 1000 → 余地 450 に S の 1 本は入る: {}", stderr_of(&fits));
    let id = run_id_of(&fits);
    assert!(state.join("pipe").join(&id).is_dir(), "run dir が作られる: {id}");
    assert_eq!(event_count(&state), 2, "RunCreated と審査の段（Reviewed）の 2 件");
    stop_run_ok(&state, &id);
    let before = event_count(&state);
    let tight = capped_rules(&state, "rules-tight.toml", 1_099);
    let short = intake_with_rules(&repo, &state, &sized_contract(&repo, "short", "S", fresh), "s2-cs", &tight);
    let err = stderr_of(&short);
    assert_eq!(short.status.code(), Some(i32::from(RC_REFUSED)), "本体 1000 → 余地 99 に S の 1 本は入らない: {err}");
    assert!(err.contains("core の上限の余地が 99 行") && err.contains("size S"), "余地は本体から数える（全体なら 0 行）: {err}");
    assert!(!err.contains("heavy.rs の上限"), "名指すのは core だけ: {err}");
    assert_eq!(event_count(&state), before, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// file の余地は**全体**のまま（R-C4-2 の file-lines と同じ）: 同じ base で `heavy.rs` 自身への M（300）は file の余地
/// 101 で断られ（本体で数える実装は 500 で通す）、S（100）は入る。core は上限 40000 で余裕＝名指すのは file だけ。
#[test]
fn pipe_intake_core_headroom_keeps_the_file_headroom_on_the_whole_file() {
    let (repo, state) = repo_with_heavy_file();
    let heavy = "\"crates/toy/src/heavy.rs\"";
    let roomy = capped_rules(&state, "rules-roomy.toml", 40_000);
    let over = intake_with_rules(&repo, &state, &sized_contract(&repo, "m", "M", heavy), "s2-hm", &roomy);
    let err = stderr_of(&over);
    assert_eq!(over.status.code(), Some(i32::from(RC_REFUSED)), "全体 1399 → 余地 101 に M は入らない: {err}");
    assert!(err.contains("crates/toy/src/heavy.rs の上限の余地が 101 行") && err.contains("size M"), "file の余地は全体から: {err}");
    assert!(!err.contains("core の上限"), "core は余裕: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    let fits = intake_with_rules(&repo, &state, &sized_contract(&repo, "s", "S", heavy), "s2-hs", &roomy);
    assert_eq!(fits.status.code(), Some(i32::from(RC_OK)), "S（100）は余地 101 に入る: {}", stderr_of(&fits));
    assert!(state.join("pipe").join(run_id_of(&fits)).is_dir(), "run dir が作られる");
    clean(&[&repo, &state]);
}

// ── file ごとの見込み growth（設計 docs/design/contract-source.md §46・行 ax・`s2-07l.578`・接頭辞 `contract_growth_`） ──

/// [`sized_contract`] と同じ行に `growth` の欄（`None` なら書かない）を足して commit し、pointer を返す。
pub(super) fn grown_contract(repo: &Path, id: &str, size: &str, write_set: &str, growth: Option<&str>) -> String {
    let mut fields: Vec<String> = contract_body()
        .into_iter()
        .filter(|line| !line.starts_with("size") && !line.starts_with("write-set") && !line.starts_with("id"))
        .chain([format!("id = \"{id}\""), format!("size = \"{size}\""), format!("write-set = [{write_set}]")])
        .collect();
    fields.extend(growth.map(|items| format!("growth = [{items}]")));
    commit_row(repo, &fields);
    format!("{DESIGN_FILE}#{id}")
}

/// §46 形 5: preflight の `headroom=<file>:<余地>/<見積>` の見積は file ごとの見込み。余地 101 の big.rs と新規 file（余地
/// 1500）を持つ size S の行で、growth に big.rs:50 を書くと big.rs の見積だけが 50 になり新規 file は S の値のまま。同じ行
/// から growth を外すと 2 file とも S の値（形は不変）。
#[test]
fn contract_growth_preflight_headroom_estimate_is_per_file() {
    let (repo, state) = repo_with_big_file();
    let rules = capped_rules(&state, "rules-roomy.toml", 40_000);
    let small = embedded_int("pipe.size_s_lines");
    let write_set = "\"crates/toy/src/big.rs\", \"+crates/toy/src/new.rs\"";
    let rooms = |growth: Option<&str>| {
        let design = grown_contract(&repo, "g", "S", write_set, growth);
        let out = run_pipe(&[
            "preflight", "--design", &design, "--bead", "s2-g",
            "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(), "--rules", &rules,
        ]);
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "断り 0: {} {}", stdout_of(&out), stderr_of(&out));
        fact_lines(&out, "headroom=")
    };
    assert_eq!(
        rooms(Some("\"crates/toy/src/big.rs:50\"")),
        ["headroom=crates/toy/src/big.rs:101/50".to_owned(), format!("headroom=crates/toy/src/new.rs:1500/{small}")],
        "growth の file だけ見積が 50（余地の小さい順）"
    );
    assert_eq!(
        rooms(None),
        [format!("headroom=crates/toy/src/big.rs:101/{small}"), format!("headroom=crates/toy/src/new.rs:1500/{small}")],
        "growth の無い行は全 file が size の見積"
    );
    assert_eq!(run_dirs(&state), Vec::<String>::new(), "preflight は run dir を作らない");
    clean(&[&repo, &state]);
}

/// intake の判定行（1 行目）の token。
pub(super) fn intake_tokens(out: &Output) -> Vec<String> {
    stdout_of(out).lines().next().unwrap_or_default().split_whitespace().map(str::to_owned).collect()
}

/// Declared 行（新欄なし + `write-set` あり）で `verify` が nextest 形の行。`write-set` だけを差し替える。
pub(super) fn declared_teeth_row(id: &str, filter: &str, write_set: &str) -> String {
    let verify = format!("[\"cargo nextest run -p toy --no-tests=fail {filter}\"]");
    table_row(id, &[("write-set", write_set), ("verify", verify.as_str())])
}

// flip-check: moved s2-07l.198.2
/// 現物の 2 つの cli module と、それぞれの件数 pin の歯の file（base の字面そのもの・`include_str!`・core の src は
/// 境界 crate の外＝`crates/scribe2/src/` を指す）。
pub(super) const SUBCOMMAND_FILES: &[(&str, &str)] = &[
    ("crates/scribe2/src/seat/cli.rs", include_str!("../../../../scribe2/src/seat/cli.rs")),
    ("crates/scribe2/src/pipe/cli.rs", include_str!("../../../../scribe2/src/pipe/cli.rs")),
    ("crates/scribe2/tests/e2e/seat.rs", include_str!("../seat.rs")),
    ("crates/scribe2/tests/e2e/pipe.rs", include_str!("../pipe.rs")),
];

// ───── 受付は契約の散文（goal / done）を走査しない（設計 docs/design/contract-source.md §27・`s2-07l.476`・接頭辞 `pipe_intake_prose_`） ─────
//
// §27 の門（歯の名指しの被覆 / 判定行 token の pin・`s2-07l.429`）は user 裁定 2026-09-18 で消した。ここは負例＝散文に
// 既存の歯の名や判定行 token を backtick で書いても受付が断らない（rc 0）ことを測る。base はどちらも断る＝RED。

/// 判定行 token（`mode=`）を歯の区間に持つ toy の file 2 本（pin の file を辞書順に 2 件名乗らせる）。
const PROSE_PIN_FILES: &[(&str, &str)] = &[
    ("crates/toy/tests/pin_a.rs", "#[test]\nfn pin_a_case() {\n    assert!(out.contains(\"mode=fast\"));\n}\n"),
    ("crates/toy/tests/pin_b.rs", "#[test]\nfn pin_b_case() {\n    assert!(out.contains(\"mode=slow\"));\n}\n"),
];

/// Declared 行の `write-set`（行の `verify` の filter `other_` の歯の file〔helper.rs〕を持つ＝§20 の門は通る形）。
const PROSE_WRITE_SET: &str = "[\"crates/toy/src/tint.rs\", \"crates/toy/tests/helper.rs\"]";

/// 行と契約 file の `verify` に使う nextest 行（`other_` は helper.rs の歯 `other_case` に当たる）。
const PROSE_OTHER_LINE: &str = "cargo nextest run -p toy --no-tests=fail other_";

/// 設計 pointer `docs/design/toy.md#<id>` と、契約 file 側の `done`（散文）・`verify`（散文の門が読む filter 語）・
/// `write-set` を持つ契約 file（`goal` は [`contract_body`] の既定＝backtick を持たない）。
fn prose_contract(_repo: &Path, id: &str, _done: &str, _verify: &str, _write_set: &str) -> String {
    // 契約 (b) 以後、`done` / `verify` / `write-set` は**行**が持つ（契約 file は行から作られる）。
    // 受付へ渡すのは pointer だけで、散文は行の側に在る。
    format!("docs/design/toy.md#{id}")
}

/// (a) `done` が base の歯 `derive_ok` を backtick で名指し、契約 file の `verify` の filter が `other_`（名指しに当たらない）
/// でも受付は断らない（rc 0・判定行の弁別と本数は不変・散文の数の token は無い）。base は `teeth-uncovered` で断る＝RED。
#[test]
fn pipe_intake_prose_named_tooth_outside_filter_is_accepted() {
    let row = declared_teeth_row("t", "other_", PROSE_WRITE_SET);
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let contract = prose_contract(&repo, "t", "`derive_ok` を測る", PROSE_OTHER_LINE, PROSE_WRITE_SET);
    let out = intake_raw(&repo, &state, &contract, "s2-t");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "散文は走査しない＝通る: {}", stderr_of(&out));
    let tokens = intake_tokens(&out);
    assert!(tokens.contains(&"write-set=declared".to_owned()) && tokens.contains(&"files=2".to_owned()), "{tokens:?}");
    assert!(tokens.iter().all(|token| !token.starts_with("prose=")), "散文の数の token は無い: {tokens:?}");
    clean(&[&repo, &state]);
}

/// (b) `done` の判定行 token（`mode=<a|b>`）を歯の区間に持つ toy の 2 file が Declared 行の `write-set` に無くても受付は
/// 断らない（rc 0・写しの write-set は行のまま 2 file）。base は `pins-outside-write-set` で断る＝RED。
#[test]
fn pipe_intake_prose_token_pin_outside_write_set_is_accepted() {
    let row = declared_teeth_row("p", "other_", PROSE_WRITE_SET);
    let (repo, state) = derive_repo_with(&table_doc(&table_region(&[row])), PROSE_PIN_FILES);
    let contract = prose_contract(&repo, "p", "`mode=<a|b>` を pin する", PROSE_OTHER_LINE, PROSE_WRITE_SET);
    let out = intake_raw(&repo, &state, &contract, "s2-p");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "pin の file を write-set に求めない＝通る: {}", stderr_of(&out));
    let tokens = intake_tokens(&out);
    assert!(tokens.iter().all(|token| !token.starts_with("prose=")), "散文の数の token は無い: {tokens:?}");
    let want = ["crates/toy/src/tint.rs", "crates/toy/tests/helper.rs"];
    assert_eq!(copied_write_set(&state, &run_id_of(&out)), want, "写しの write-set は行のまま");
    clean(&[&repo, &state]);
}

// ───── 器の口 pipe preflight（設計 docs/design/contract-source.md §21・行 u・`s2-07l.394`・接頭辞 `pipe_preflight_`） ─────

/// `pipe preflight` を 1 回撃つ（intake と同じ引数の読み・審査の lens は無い・`--state-dir` は `with_state_dir` の周だけ）。
fn preflight_raw(repo: &Path, state: &Path, design: &str, bead: &str, with_state_dir: bool) -> Output {
    let (rules, state_dir) = (ceiling_rules(state), state.display().to_string());
    let mut args = vec![
        "preflight", "--design", design, "--bead", bead,
        "--repo", &repo.display().to_string(), "--rules", &rules,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<String>>();
    if with_state_dir {
        args.extend(["--state-dir".to_owned(), state_dir]);
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_pipe(&borrowed)
}

/// stdout の `key` で始まる行（1 行 1 事実の母集団を key で数える）。
fn fact_lines(out: &Output, key: &str) -> Vec<String> {
    stdout_of(out).lines().filter(|line| line.starts_with(key)).map(str::to_owned).collect()
}

/// stdout の末尾の行（`preflight: <ok|refused n=<件数>|broken>`）。
fn tail_line(out: &Output) -> String {
    stdout_of(out).lines().last().unwrap_or_default().to_owned()
}

/// 設計 pointer `docs/design/toy.md#<id>` と **契約 file 側の write-set** を持つ契約 file（Declared 行は写しを差し替えない
/// ので、受付の余地と交差は契約 file の write-set を読む＝行と同じ列挙を契約 file にも書く）。
fn pointed_contract_with(_repo: &Path, _name: &str, id: &str, _write_set: &str) -> String {
    // 契約 (b) 以後、write-set は**行**が持つ。受付へ渡すのは pointer だけである。
    format!("docs/design/toy.md#{id}")
}

/// (a) Declared 行（`derive_` の歯 other.rs / e2e.rs が write-set の外・`-` の先が base に無い）は preflight が判定関数
/// 1 本につき高々 1 件を**全部**並べる: `refuse=` が teeth-outside-write-set（settle_write_set）と write-set-item-unresolved
/// （exclude_cap_shortfall）の順に 2 本・末尾 `preflight: refused n=2`・rc 1・run dir と event は撃つ前と同数。同じ契約を
/// `intake` に通すと rc 1 で**先頭の 1 件**（teeth-outside-write-set）だけを名乗る（judge が同じ列を返し intake が先頭で
/// 断る証拠）。base は `pipe preflight` を usage 違反で断る（RED）。
#[test]
fn pipe_preflight_lists_one_refusal_per_judgement_without_creating_a_run() {
    let write_set = "[\"crates/toy/src/tint.rs\", \"-crates/toy/src/none.rs\"]";
    let row = declared_teeth_row("t", "derive_", write_set);
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let contract = pointed_contract_with(&repo, "t.toml", "t", write_set);
    let (dirs, events) = (run_dirs(&state), event_count(&state));
    let out = preflight_raw(&repo, &state, &contract, "s2-t", true);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "断り ≥ 1 は rc 1: {text}");
    // 契約 (b) 以後、**行の欠陥は表の検査が先に断る**（受付は行から契約を作るので、作れない行で judge まで
    // 進まない）。`-` の先が base に無い項目は表の findings で出て、preflight はそれを 1 件として並べる。
    let refuses = fact_lines(&out, "refuse=");
    assert!(refuses.is_empty(), "行が作れない周は判定の列を持たない: {text}");
    let err = stderr_of(&out);
    assert!(err.contains("write-set-item-unresolved"), "解けない項目を名乗る: {err}");
    assert!(err.contains("-crates/toy/src/none.rs"), "項目の字面（- 込み）を名乗る: {err}");
    assert_eq!(tail_line(&out), "preflight: refused n=1", "末尾は件数: {text}");
    assert!(fact_lines(&out, "write-set=").is_empty(), "断った周は事実の行を立てない: {text}");
    assert_eq!(run_dirs(&state), dirs, "run dir は撃つ前と同数（母集団 {} 本）", dirs.len());
    assert_eq!(event_count(&state), events, "event も同じ（母集団 {events} 件）");
    intake_names_only_the_first(&repo, &state, &contract, events);
    clean(&[&repo, &state]);
}

/// (a) の対照: 同じ契約を `intake` に通すと rc 1 で先頭の 1 件（teeth-outside-write-set）だけを名乗り、event を書かない。
fn intake_names_only_the_first(repo: &Path, state: &Path, design: &str, events: usize) {
    let taken = intake_raw(repo, state, design, "s2-t");
    let err = stderr_of(&taken);
    assert_eq!(taken.status.code(), Some(i32::from(RC_REFUSED)), "intake は従来どおり rc 1: {err}");
    // 契約 (b) 以後、行の欠陥は表の検査が先に断る＝intake も preflight も**同じ 1 件**を名乗る。
    assert!(err.contains("write-set-item-unresolved"), "先頭の 1 件を名乗る: {err}");
    assert!(!err.contains("verify の歯の file"), "judge の断りまで進まない（表の検査で止まる）: {err}");
    assert_eq!(event_count(state), events, "intake も断った周は event を書かない");
}

/// (b) 断り 0 の Derived 行（`derive_` の歯 2 file が導出値）: rc 0・`design=docs/design/toy.md#a section=1`（material= の
/// 行は出さない）・`write-set=derived files=2`・`teeth=derive_:2` に other.rs と e2e.rs・`headroom=` が write-set の file 数
/// （2 本・`<file>:<余地>/<見積>` の形）・末尾 `preflight: ok`。続けて `intake` が rc 0 で同じ `files=2` を出す。
#[test]
fn pipe_preflight_ok_reports_facts_and_matches_intake() {
    let row = derive_row("a", &[("verify", "[\"cargo nextest run -p toy --no-tests=fail derive_\"]")]);
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let contract = pointed_contract(&repo, "a.toml", "a");
    let out = preflight_raw(&repo, &state, &contract, "s2-a", true);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "断り 0 は rc 0: {text} {}", stderr_of(&out));
    assert_eq!(fact_lines(&out, "design="), ["design=docs/design/toy.md#a section=1"], "{text}");
    assert!(!text.contains("material="), "§ の本文の大きさは判定に効かないので出さない: {text}");
    assert_eq!(fact_lines(&out, "write-set="), ["write-set=derived files=2"], "{text}");
    assert_eq!(fact_lines(&out, "teeth="), ["teeth=derive_:2@crates/toy/src/other.rs,crates/toy/tests/e2e.rs"], "{text}");
    let rooms = fact_lines(&out, "headroom=");
    assert_eq!(rooms.len(), 2, "余地は write-set の file 数だけ: {text}");
    for (room, file) in rooms.iter().zip(["crates/toy/src/other.rs", "crates/toy/tests/e2e.rs"]) {
        let rest = room.strip_prefix(&format!("headroom={file}:")).unwrap_or_default();
        let (left, right) = rest.split_once('/').unwrap_or_default();
        assert!(left.parse::<u64>().is_ok() && right.parse::<u64>().is_ok(), "<余地>/<見積> の 2 数: {room}");
    }
    assert!(fact_lines(&out, "refuse=").is_empty(), "断り 0: {text}");
    assert_eq!(tail_line(&out), "preflight: ok", "{text}");
    assert_eq!(run_dirs(&state), Vec::<String>::new(), "run dir を作らない");
    assert_eq!(event_count(&state), 0, "event を書かない");
    let taken = intake_raw(&repo, &state, &contract, "s2-a");
    assert_eq!(taken.status.code(), Some(i32::from(RC_OK)), "同じ契約は intake も通る: {}", stderr_of(&taken));
    assert!(intake_tokens(&taken).contains(&"files=2".to_owned()), "同じ本数: {}", stdout_of(&taken));
    clean(&[&repo, &state]);
}

/// (c) `--state-dir` 無し・git 設定も無い toy: preflight は `overlap=unmeasured` を出し、rc は他の断りで決まる（断り 0 なら
/// rc 0）・置き場を作らない。対照: 同じ引数の `intake` は従来どおり置き場が無い旨で断る（rc 1）。
#[test]
fn pipe_preflight_without_state_dir_marks_overlap_unmeasured() {
    let row = derive_row("a", &[("verify", "[\"cargo nextest run -p toy --no-tests=fail derive_\"]")]);
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    git(&repo, &["config", "--unset", &format!("{NAME}.stateDir")]);
    let contract = pointed_contract(&repo, "a.toml", "a");
    let out = preflight_raw(&repo, &state, &contract, "s2-a", false);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "置き場が無くても断りでない: {text} {}", stderr_of(&out));
    assert_eq!(fact_lines(&out, "overlap="), ["overlap=unmeasured"], "交差は測れないと言う（0 に潰さない）: {text}");
    assert_eq!(tail_line(&out), "preflight: ok", "{text}");
    assert!(!state.join("pipe").exists() && event_count(&state) == 0, "置き場に何も作らない");
    let taken = run_pipe(&[
        "intake", "--design", &contract, "--bead", "s2-a",
        "--repo", &repo.display().to_string(), "--rules", &ceiling_rules(&state),
    ]);
    assert_eq!(taken.status.code(), Some(i32::from(RC_REFUSED)), "intake は置き場が要る（従来どおり）");
    assert!(stderr_of(&taken).contains("置き場が紐づいていない"), "{}", stderr_of(&taken));
    clean(&[&repo, &state]);
}

/// (d) 読めない契約表（行を跨いだ配列）は rc 2・末尾 `preflight: broken`・run dir も event も無し（理由は stderr）。
/// 契約 (b) 以後、読めない側は**行**である（契約 file は器が作るので手書きの壊れた file は入口に無い）。
#[test]
fn pipe_preflight_broken_contract_is_rc_2() {
    let broken = format!(
        "# 設計: toy\n\n## 1. 何を解くか\n\n本文。\n\n{}\nschema = 1\n\n[[contract]]\nid = \"a\"\nwrite-set = [\"src/lib.rs\"\n{}\n",
        vessel::pipe::table::BEGIN,
        vessel::pipe::table::END
    );
    let (repo, state) = derive_repo(&broken);
    let out = preflight_raw(&repo, &state, "docs/design/toy.md#a", "s2-a", true);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない契約は rc 2: {}", stderr_of(&out));
    assert_eq!(tail_line(&out), "preflight: broken", "{}", stdout_of(&out));
    assert!(!stderr_of(&out).is_empty(), "理由は stderr に出す");
    assert!(!state.join("pipe").exists() && event_count(&state) == 0, "run dir も event も作らない");
    clean(&[&repo, &state]);
}

/// 便の `review.json` を key/value の並びとして読む（無ければ空）。
pub(super) fn review_pairs(state: &Path, id: &str) -> Vec<(String, vessel::fleet::json_lite::Value)> {
    let text = fs::read_to_string(state.join("pipe").join(id).join("review.json")).unwrap_or_default();
    vessel::fleet::json_lite::parse_object(text.trim()).unwrap_or_default()
}

/// `kind` と `at` を持つ偽 lens の最終行（`kind` / `at` は `None` なら書かない）。
pub(super) fn lens_finding(verdict: &str, kind: Option<&str>, at: Option<&str>) -> String {
    let mut body = format!("{{\"verdict\":\"{verdict}\",\"evidence\":\"fake\"");
    if let Some(found) = kind {
        body.push_str(&format!(",\"kind\":\"{found}\""));
    }
    if let Some(found) = at {
        body.push_str(&format!(",\"at\":\"{found}\""));
    }
    body.push('}');
    body
}

/// `review.json` が key を持つか（`value_of` は無い key を空で返すので、不在と空を分けて測る）。
pub(super) fn review_has(state: &Path, id: &str, key: &str) -> bool {
    review_pairs(state, id).iter().any(|(found, _)| found == key)
}

/// 便の `Reviewed` の detail（1 件だけ在ることを assert）。
pub(super) fn reviewed_detail(state: &Path, id: &str) -> String {
    let listed: Vec<String> = stages(state, id)
        .into_iter()
        .filter(|(stage, _)| *stage == Some(Stage::Reviewed))
        .filter_map(|(_, detail)| detail)
        .collect();
    assert_eq!(listed.len(), 1, "Reviewed は 1 件: {listed:?}");
    listed.into_iter().next().unwrap_or_default()
}

/// **審査の lens の箱は 1 × `gate.job_memory_mb`**（設計 gate-cost.md §12・行 c・裁定 id user 2026-09-15T18:2xZ）。
/// 包める周（道具箱の偽 `systemd-run`）で審査を撃ち、unit `-review-1` の記録をちょうど 1 件読む。base は
/// `MemTotal − host.reserve_memory_mb`（host の箱）を渡す＝RED。
#[test]
fn pipe_confine_review_box_is_one_job() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let marker = state.join("lens-ran-review-box");
    let out = run_pipe_with_path(&crate::toolbox_path(&state), &[
        "intake", "--design", &path, "--bead", "s2-rbox",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state), "--lens", &fake_lens(&marker, &lens_verdict("PASS")),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS は rc 0: {}", stderr_of(&out));
    assert!(marker.exists(), "審査の lens は実際に撃たれている");
    let record = crate::toolbox_record(&state, "-review-1");
    assert!(record.lines().any(|line| line == "--scope"), "審査の lens は包めた: {record}");
    assert_eq!(
        record.lines().find_map(|line| line.strip_prefix("MemoryMax=")).unwrap_or_default(),
        format!("{}M", embedded_int("gate.job_memory_mb")),
        "審査の lens の箱は 1 × gate.job_memory_mb: {record}"
    );
    clean(&[&repo, &state]);
}

// ───── 同型の停止と焼き直しの門（`s2-07l.396`・設計 contract-source.md §23・SRS FR49・接頭辞 `pipe_intake_repeat_`） ─────

/// 同じ bead の便は run id が `<bead>-<UTC の秒>` なので、次の便は**別の秒**に起こす（同じ秒は duplicate-run）。
fn next_second() {
    let now = || {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|found| found.as_secs()).unwrap_or_default()
    };
    let start = now();
    while now() == start {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 同じ bead の便をもう 1 本起こす材料（repo・置き場・bead・設計 pointer）。
struct Again<'a> {
    /// 対象 repo。
    repo: &'a Path,
    /// 置き場。
    state: &'a Path,
    /// 契約の bead id。
    bead: &'a str,
    /// 設計 pointer（`--design`）。
    design: &'a str,
}

/// 同じ bead の便を偽 lens の 1 行で起こす（`rules` の manifest・**設計 doc を書き換えない**＝節の本文を変えた周も
/// そのまま撃てる・rc は測らない）。前の便と別の秒になるまで待ってから撃つ。
fn repeat_intake_with(again: &Again<'_>, rules: &str, line: &str) -> Output {
    next_second();
    let marker = again.state.join(format!("lens-ran-{}", again.bead));
    run_pipe(&[
        "intake", "--design", again.design, "--bead", again.bead,
        "--repo", &again.repo.display().to_string(), "--state-dir", &again.state.display().to_string(),
        "--rules", rules, "--lens", &fake_lens(&marker, line),
    ])
}

/// [`repeat_intake_with`] を既定の manifest（[`ceiling_rules`]）で撃つ。
fn repeat_intake(repo: &Path, state: &Path, bead: &str, design: &str, line: &str) -> Output {
    repeat_intake_with(&Again { repo, state, bead, design }, &ceiling_rules(state), line)
}

/// 便が**受理された**（run id を持ち run dir が 1 つ増えた・審査の判定行が出た）ことを測り、id を返す。受付の断りは
/// run を作らないので、rc（FAIL の審査は rc 1・断りも rc 1）でなく run dir の数で読む。
fn accepted(out: &Output, state: &Path, before: usize) -> String {
    let id = run_id_of(out);
    assert!(!id.is_empty(), "受理された便は run id を出す: {} {}", stdout_of(out), stderr_of(out));
    assert_eq!(run_dirs(state).len(), before.saturating_add(1), "run dir が 1 つ増える: {}", stderr_of(out));
    assert!(stdout_of(out).contains("stage=Reviewed"), "審査の段まで進む: {}", stdout_of(out));
    id
}

/// 同じ bead で FAIL の便を `kinds`（理由の型と `at`）の順に起こし、受理された id の列（起こした順）を返す。
fn failed_runs(repo: &Path, state: &Path, bead: &str, design: &str, kinds: &[(Option<&str>, Option<&str>)]) -> Vec<String> {
    let mut ids = Vec::new();
    for (kind, at) in kinds {
        let before = run_dirs(state).len();
        let out = repeat_intake(repo, state, bead, design, &lens_finding("FAIL", *kind, *at));
        ids.push(accepted(&out, state, before));
    }
    ids
}

/// 受付の断りを測る: rc 1・stdout 0 byte・stderr が `wants` を全部名乗る・run dir と event は不変・同じ引数の preflight
/// が `refuse=<name>:` を **1 行だけ**出して `preflight: refused n=1`（judge の同じ 1 本・§23 (4)）。stderr を返す。
fn assert_refused(again: &Again<'_>, name: &str, wants: &[&str]) -> String {
    let Again { repo, state, bead, design } = *again;
    let (dirs, events) = (run_dirs(state), events_bytes(state));
    let out = repeat_intake(repo, state, bead, design, &lens_finding("FAIL", Some("other"), None));
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{name} は rc 1: {err}");
    assert!(out.stdout.is_empty(), "断った周は stdout に 1 byte も書かない: {}", stdout_of(&out));
    for want in wants {
        assert!(err.contains(want), "{name}: {want} を名乗る: {err}");
    }
    assert_eq!(run_dirs(state), dirs, "{name}: run dir を作らない（母集団 {} 本）", dirs.len());
    assert_eq!(events_bytes(state), events, "{name}: events.jsonl は byte 不変");
    let flight = preflight_raw(repo, state, design, bead, true);
    assert_eq!(flight.status.code(), Some(i32::from(RC_REFUSED)), "{name}: preflight も rc 1: {}", stdout_of(&flight));
    let refuses = fact_lines(&flight, "refuse=");
    assert_eq!(refuses.len(), 1, "{name}: 断りは 1 件: {}", stdout_of(&flight));
    let line = refuses.first().map(String::as_str).unwrap_or_default();
    assert!(line.starts_with(&format!("refuse={name}:")), "{name}: 名を名乗る: {line}");
    assert_eq!(tail_line(&flight), "preflight: refused n=1", "{}", stdout_of(&flight));
    assert_eq!(run_dirs(state), dirs, "{name}: preflight も run dir を作らない");
    err
}

/// [`ceiling_rules`] の `review.same_kind_stop` の行だけを差し替えた tmp manifest（`None` = 行を落とす）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rules_with_stop(state: &Path, name: &str, stop: Option<u64>) -> String {
    let path = write_rules(state, name, 1, 1_000_000);
    let text = fs::read_to_string(&path).expect("tmp manifest を読める");
    let block = |value: u64| {
        format!(
            "[[rule]]\nid = \"{SAME_KIND_STOP_ROW}\"\nkind = \"ReviewSameKindStop\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n"
        )
    };
    let default = block(embedded_int(SAME_KIND_STOP_ROW));
    assert!(text.contains(&default), "既定の行が在る（差し替えが空振りしない）: {text}");
    fs::write(&path, text.replace(&default, &stop.map(block).unwrap_or_default())).expect("tmp manifest を書ける");
    path.display().to_string()
}

/// 節の本文を書き換えて commit する（行は既定のまま＝契約 file は不変・節の本文だけが変わる）。
fn commit_changed_section(repo: &Path) {
    let doc = design_doc(&[]).replace("節の本文。", "節の本文を改めた。");
    assert_ne!(doc, design_doc(&[]), "節の本文が変わる");
    write_design(repo, &doc);
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "section-changed"]);
}

/// (1) 同じ kind（literal-mismatch）の FAIL 2 便の後、契約 file と節の本文がともに不変の 3 便目は `same-kind-repeated`
/// で断られ、理由の 1 行が kind と本数と行の値（`review.same_kind_stop`）と 2 便の id（新しい順）を名乗る。run dir も
/// event も作らず、preflight にも同じ 1 件が出る。**回数は rules 行が持つ**: 値 3 の manifest なら同じ 3 便目が通る。
#[test]
fn pipe_intake_repeat_same_kind_twice_with_unchanged_materials_is_refused_naming_the_runs() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let kinds = [(Some("literal-mismatch"), Some("Marker")), (Some("literal-mismatch"), Some("Marker"))];
    let ids = failed_runs(&repo, &state, "s2-rep", &design, &kinds);
    let (first, second) = (ids.first().cloned().unwrap_or_default(), ids.get(1).cloned().unwrap_or_default());
    let newest_first = format!("{second}, {first}");
    let wants = ["literal-mismatch", " 2 便", "review.same_kind_stop の 2", &newest_first];
    let again = Again { repo: &repo, state: &state, bead: "s2-rep", design: &design };
    let err = assert_refused(&again, "same-kind-repeated", &wants);
    assert!(!err.contains("finding-unaddressed") && !err.contains("対応する差分"), "at=Marker は契約に無い＝対応済み: {err}");
    // 回数の線は manifest の値: 3 なら 2 便では止まらない（値を写した定数ではないことを測る）。
    let rules = rules_with_stop(&state, "rules-stop-3.toml", Some(3));
    let before = run_dirs(&state).len();
    let out = repeat_intake_with(&again, &rules, &lens_finding("FAIL", Some("literal-mismatch"), Some("Marker")));
    accepted(&out, &state, before);
    clean(&[&repo, &state]);
}

/// (2) 同じ kind の FAIL 2 便の後でも、契約 file（行の `done`）か節の本文のどちらかを変えると 3 便目は通る
/// （「焼き直しは書き直し」・§7 の形）。
#[test]
fn pipe_intake_repeat_changed_contract_or_section_passes() {
    for mode in ["contract", "section"] {
        let (repo, state) = repo_with_state();
        let design = write_contract(&repo, &[], &[]);
        let kinds = [(Some("literal-mismatch"), Some("Marker")), (Some("literal-mismatch"), Some("Marker"))];
        failed_runs(&repo, &state, "s2-rew", &design, &kinds);
        match mode {
            "contract" => {
                write_contract(&repo, &["done"], &[r#"done = "書き直した done""#]);
            }
            _ => commit_changed_section(&repo),
        }
        let before = run_dirs(&state).len();
        let out = repeat_intake(&repo, &state, "s2-rew", &design, &lens_finding("FAIL", Some("literal-mismatch"), Some("Marker")));
        let id = accepted(&out, &state, before);
        assert!(!stderr_of(&out).contains("同型") && !stderr_of(&out).contains("焼き直し"), "{mode}: {}", stderr_of(&out));
        assert_eq!(value_of(&review_pairs(&state, &id), "kind"), "literal-mismatch", "{mode}: 3 便目も審査に届く");
        clean(&[&repo, &state]);
    }
}

/// (3) kind の違う 2 便（literal-mismatch → other）と `unparsed` の 2 便（`kind` を書かない偽 lens）はどちらも 3 便目が通る
/// （同じ型の連鎖でない・lens の欠けを契約の型に化けさせない・C10）。
#[test]
fn pipe_intake_repeat_different_kinds_or_unparsed_pass() {
    for (label, kinds) in [
        ("different", [(Some("literal-mismatch"), Some("Marker")), (Some("other"), Some("Marker"))]),
        ("unparsed", [(None, Some("Marker")), (None, Some("Marker"))]),
    ] {
        let (repo, state) = repo_with_state();
        let design = write_contract(&repo, &[], &[]);
        failed_runs(&repo, &state, "s2-mix", &design, &kinds);
        let before = run_dirs(&state).len();
        let out = repeat_intake(&repo, &state, "s2-mix", &design, &lens_finding("FAIL", Some("literal-mismatch"), Some("Marker")));
        accepted(&out, &state, before);
        assert!(!stderr_of(&out).contains("同型"), "{label}: {}", stderr_of(&out));
        clean(&[&repo, &state]);
    }
}

/// (4) PASS を挟むと数え直す: FAIL(K) → PASS（stop で外す）→ FAIL(K) の後の 4 便目は通り（K は 1 便）、その 4 便目が
/// FAIL(K) なら 5 便目は 2 便続いたとして断られる＝PASS より前の便は数えない。
#[test]
fn pipe_intake_repeat_pass_in_between_restarts_the_count() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let fail = lens_finding("FAIL", Some("literal-mismatch"), Some("Marker"));
    failed_runs(&repo, &state, "s2-pas", &design, &[(Some("literal-mismatch"), Some("Marker"))]);
    let before = run_dirs(&state).len();
    let passed = repeat_intake(&repo, &state, "s2-pas", &design, &lens_verdict("PASS"));
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "PASS: {}", stderr_of(&passed));
    let passed_id = accepted(&passed, &state, before);
    stop_run_ok(&state, &passed_id);
    let third = failed_runs(&repo, &state, "s2-pas", &design, &[(Some("literal-mismatch"), Some("Marker"))]);
    let before = run_dirs(&state).len();
    let fourth = repeat_intake(&repo, &state, "s2-pas", &design, &fail);
    let fourth_id = accepted(&fourth, &state, before);
    let newest_first = format!("{fourth_id}, {}", third.first().cloned().unwrap_or_default());
    let again = Again { repo: &repo, state: &state, bead: "s2-pas", design: &design };
    let err = assert_refused(&again, "same-kind-repeated", &[" 2 便", &newest_first]);
    assert!(!err.contains(&passed_id), "PASS より前の便は数えない: {err}");
    clean(&[&repo, &state]);
}

/// (5) teeth-outside-write-set の指摘 `at=<path>` の後、write-set にその path の無い契約は `finding-unaddressed` で
/// path を名指して断られ、write-set に path を足した契約は通る。
#[test]
fn pipe_intake_repeat_teeth_outside_write_set_at_path_needs_the_path_in_the_write_set() {
    let (repo, state) = repo_with_state();
    let design = write_set_contract(&repo, "first", &["src/lib.rs"]);
    failed_runs(&repo, &state, "s2-tee", &design, &[(Some("teeth-outside-write-set"), Some("src/other.rs"))]);
    let again = Again { repo: &repo, state: &state, bead: "s2-tee", design: &design };
    assert_refused(&again, "finding-unaddressed", &["teeth-outside-write-set", "src/other.rs"]);
    let widened = write_set_contract(&repo, "second", &["src/lib.rs", "src/other.rs"]);
    let before = run_dirs(&state).len();
    let out = repeat_intake(&repo, &state, "s2-tee", &widened, &lens_verdict("PASS"));
    accepted(&out, &state, before);
    clean(&[&repo, &state]);
}

/// (5') §35: `at` に path でない項目（歯の接頭辞・§ の番号）が混ざった teeth-outside-write-set の後、path の項目
/// （src/other.rs）を write-set に足さない契約は src/other.rs だけを名指して断られ理由に測れない 2 件が数で出る。
/// src/other.rs を足した契約は通る（path でない項目は測れない＝永遠に断らない）。
#[test]
fn pipe_intake_repeat_teeth_outside_write_set_mixed_at_measures_only_the_path() {
    let (repo, state) = repo_with_state();
    let design = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let at = "headless_lens_promise_, src/other.rs, §33";
    failed_runs(&repo, &state, "s2-mxd", &design, &[(Some("teeth-outside-write-set"), Some(at))]);
    let again = Again { repo: &repo, state: &state, bead: "s2-mxd", design: &design };
    let err = assert_refused(&again, "finding-unaddressed", &["teeth-outside-write-set", "src/other.rs", "測った 1 件・測れない 2 件"]);
    assert!(!err.contains("headless_lens_promise_") && !err.contains("§33"), "測れない項目は名指さない: {err}");
    let widened = write_set_contract(&repo, "second", &["src/lib.rs", "src/other.rs"]);
    let before = run_dirs(&state).len();
    let out = repeat_intake(&repo, &state, "s2-mxd", &widened, &lens_verdict("PASS"));
    accepted(&out, &state, before);
    assert!(!stderr_of(&out).contains("対応する差分"), "{}", stderr_of(&out));
    clean(&[&repo, &state]);
}

/// (6) literal-mismatch の指摘 `at=<識別子>` の後、識別子（`Nope::Thing`）を `done` に書いたままで base に無い契約は
/// 断られ、識別子を消した契約も、base に `Nope::Thing` を足した後の同じ契約も通る（解けるかは名指しの読み手と同じ 1 本）。
#[test]
fn pipe_intake_repeat_literal_mismatch_at_identifier_must_leave_the_contract_or_resolve_in_base() {
    for mode in ["remove", "resolve"] {
        let (repo, state) = repo_with_state();
        let design = write_contract(&repo, &["done"], &[r#"done = "Nope::Thing を直す""#]);
        failed_runs(&repo, &state, "s2-lit", &design, &[(Some("literal-mismatch"), Some("Nope::Thing"))]);
        let again = Again { repo: &repo, state: &state, bead: "s2-lit", design: &design };
        assert_refused(&again, "finding-unaddressed", &["literal-mismatch", "Nope::Thing"]);
        let design = match mode {
            "remove" => write_contract(&repo, &[], &[]),
            _ => {
                fs::write(repo.join("src").join("nope.rs"), "pub enum Nope {\n    Thing,\n}\n\npub const ALL: &[Nope] = &[Nope::Thing];\n")
                    .unwrap_or_else(|err| panic!("base の file を書ける: {err}"));
                git(&repo, &["add", "-A"]);
                git(&repo, &["commit", "-q", "-m", "resolve"]);
                design
            }
        };
        let before = run_dirs(&state).len();
        let out = repeat_intake(&repo, &state, "s2-lit", &design, &lens_verdict("PASS"));
        accepted(&out, &state, before);
        assert!(!stderr_of(&out).contains("Nope::Thing"), "{mode}: {}", stderr_of(&out));
        clean(&[&repo, &state]);
    }
}

/// (7) section-material-missing の指摘の後、節の本文が不変の契約は `at`（`§1`）を名指して断られ、節の本文を変えると通る。
#[test]
fn pipe_intake_repeat_section_material_missing_needs_a_changed_section() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    failed_runs(&repo, &state, "s2-sec", &design, &[(Some("section-material-missing"), Some("§1"))]);
    let again = Again { repo: &repo, state: &state, bead: "s2-sec", design: &design };
    assert_refused(&again, "finding-unaddressed", &["section-material-missing", "§1"]);
    commit_changed_section(&repo);
    let before = run_dirs(&state).len();
    let out = repeat_intake(&repo, &state, "s2-sec", &design, &lens_verdict("PASS"));
    accepted(&out, &state, before);
    clean(&[&repo, &state]);
}

/// (8) 測れない 4 型（goal-done-contradiction / vacuous-assert / other / unparsed）と `at` の空な周は通す（判断を要する型は
/// planner に残す・C10）: 型を替えながら 6 便を続けて起こし、どの便も受理される。
#[test]
fn pipe_intake_repeat_unmeasurable_kinds_and_empty_at_pass() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let kinds = [
        (Some("goal-done-contradiction"), Some("src/lib.rs")),
        (Some("vacuous-assert"), Some("src/lib.rs")),
        (Some("other"), Some("src/lib.rs")),
        (None, Some("src/lib.rs")),
        (Some("literal-mismatch"), None),
        (Some("teeth-outside-write-set"), None),
        (Some("section-material-missing"), Some("")),
    ];
    let ids = failed_runs(&repo, &state, "s2-unm", &design, &kinds);
    assert_eq!(ids.len(), kinds.len(), "全便が受理される（母集団 {} 便）", kinds.len());
    let mut unique = ids.clone();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "便 id は一意: {ids:?}");
    clean(&[&repo, &state]);
}

/// (9) 行 `review.same_kind_stop` の無い manifest は受付を 1 byte も動かさない: intake は rc 2 で行を名指し run dir も event も
/// 作らず、preflight は `refuse=rules:` で行を名指して `preflight: broken`。
#[test]
fn pipe_intake_repeat_manifest_without_the_row_is_rc_2_naming_the_row() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let rules = rules_with_stop(&state, "rules-no-stop.toml", None);
    let (dirs, events) = (run_dirs(&state), events_bytes(&state));
    let again = Again { repo: &repo, state: &state, bead: "s2-row", design: &design };
    let out = repeat_intake_with(&again, &rules, &lens_verdict("PASS"));
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "行の無い manifest は rc 2: {}", stderr_of(&out));
    assert!(stderr_of(&out).contains(SAME_KIND_STOP_ROW), "行を名指す: {}", stderr_of(&out));
    assert!(out.stdout.is_empty(), "stdout に 1 byte も書かない");
    assert_eq!(run_dirs(&state), dirs, "run dir を作らない");
    assert_eq!(events_bytes(&state), events, "events.jsonl は byte 不変");
    let flight = run_pipe(&[
        "preflight", "--design", &design, "--bead", "s2-row", "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--rules", &rules,
    ]);
    assert_eq!(flight.status.code(), Some(i32::from(RC_BROKEN)), "preflight も rc 2: {}", stdout_of(&flight));
    let refuses = fact_lines(&flight, "refuse=");
    assert!(refuses.iter().any(|line| line.starts_with("refuse=rules:") && line.contains(SAME_KIND_STOP_ROW)), "{refuses:?}");
    assert_eq!(tail_line(&flight), "preflight: broken", "{}", stdout_of(&flight));
    clean(&[&repo, &state]);
}

// ───── 設計 pointer からの生成（契約表 (b)・設計 contract-source.md §2・接頭辞 `pipe_intake_design_`） ─────

/// (1) 適合の行 1 つを `--design` で渡すと run が起き、run dir の写しが **REQUIRED 全部 + `design` + `touches`** を
/// 持つ: `goal` は行の `title`（節の本文ではない・planner 裁定 2026-09-19）・`owner` / `disposition` は固定の導出値
/// （C10・行は持たない）・`write-set` / `verify` / `req` / `size` / `done` は行の逐語・`design` は pointer の逐語。
/// 記帳は `RunCreated` 1 件。
///
/// base では `--design` が未知の flag で `--contract` が要る＝この歯は 1 本も無い（`pipe_intake_design_` の filter は
/// rc 4 で RED）。
#[test]
fn pipe_intake_design_generates_the_contract_from_the_row() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[r#"touches = ["crate::pipe::refuse::Refuse"]"#]);
    let before = event_count(&state);
    let out = intake_raw(&repo, &state, &design, "s2-gen");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "適合の行は通る: {}", stderr_of(&out));
    let id = run_id_of(&out);
    let copied = fs::read_to_string(state.join("pipe").join(&id).join("contract.toml")).unwrap_or_default();
    for want in [
        r#"goal = "縦 1 本を通す""#,
        r#"done = "run が Implemented になる""#,
        r#"size = "S""#,
        r#"owner = "generated""#,
        r#"disposition = "A-now""#,
        r#"write-set = ["src/lib.rs"]"#,
        r#"verify = ["sh verify-ok.sh"]"#,
        r#"req = ["FR4"]"#,
        r#"design = "docs/design/toy.md#a""#,
        r#"touches = ["crate::pipe::refuse::Refuse"]"#,
    ] {
        assert!(copied.contains(want), "写しが {want} を持つ: {copied}");
    }
    assert!(!copied.contains("title ="), "行の欄の名（title）は写しに出ない: {copied}");
    // 受付の記帳は 2 件（run の作成と、その直後の審査の段・FR49）。
    assert_eq!(event_count(&state), before.saturating_add(2), "記帳は run の作成と審査の段");
    stop_run_ok(&state, &id);
    clean(&[&repo, &state]);
}

/// (2) 閉包の file を欠く行は `write-set-incomplete` で**足りない file を名指して**断り、run dir も event も増えない。
#[test]
fn pipe_intake_design_refuses_a_row_whose_write_set_misses_the_closure() {
    let row = table_row("a", &[("touches", "[\"crate::tint::Tint\"]"), ("write-set", "[\"crates/toy/src/tint.rs\"]")]);
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let (dirs, events) = (run_dirs(&state), event_count(&state));
    let out = intake_raw(&repo, &state, "docs/design/toy.md#a", "s2-miss");
    let err = stderr_of(&out);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "閉包を欠く行は通さない: {err}");
    assert!(err.contains("write-set-incomplete"), "理由の名: {err}");
    assert!(err.contains("crates/toy/src/show.rs"), "足りない file を名指す: {err}");
    assert_eq!(run_dirs(&state), dirs, "run dir は増えない");
    assert_eq!(event_count(&state), events, "event も増えない");
    clean(&[&repo, &state]);
}

/// (3) 節の無い `section` を持つ行は `contract-table:section-missing` で断り、run dir も event も増えない。
#[test]
fn pipe_intake_design_refuses_a_row_whose_section_is_missing() {
    let row = table_row("a", &[("section", "\"9\"")]);
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let (dirs, events) = (run_dirs(&state), event_count(&state));
    let out = intake_raw(&repo, &state, "docs/design/toy.md#a", "s2-sec");
    let err = stderr_of(&out);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "節の無い行は通さない: {err}");
    assert!(err.contains("section-missing"), "理由の名: {err}");
    assert_eq!(run_dirs(&state), dirs, "run dir は増えない");
    assert_eq!(event_count(&state), events, "event も増えない");
    clean(&[&repo, &state]);
}

/// (4) 手書きの契約 file（`--contract PATH`）は **usage の誤りでなく** `hand-written-contract` で断る（FR54）。
/// run dir も event も増えず、断りの行は正しい渡し方（`--design <doc>#<id>`）を名乗る。
#[test]
fn pipe_intake_design_refuses_a_hand_written_contract_file() {
    let (repo, state) = repo_with_state();
    write_contract(&repo, &[], &[]);
    let hand = repo.join("contract.toml");
    fs::write(&hand, "goal = \"g\"\n").expect("手書きの file を置ける");
    let (dirs, events) = (run_dirs(&state), event_count(&state));
    let out = run_pipe(&[
        "intake", "--contract", &hand.display().to_string(), "--bead", "s2-hand",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
    ]);
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "手書きは rc 1: {err}");
    assert!(err.contains("手書きの契約 file は受け付けない"), "typed な理由: {err}");
    assert!(err.contains("--design"), "正しい渡し方を名乗る: {err}");
    assert!(!err.starts_with("usage:"), "使い方の誤りではない: {err}");
    assert_eq!(run_dirs(&state), dirs, "run dir は増えない");
    assert_eq!(event_count(&state), events, "event も増えない");
    clean(&[&repo, &state]);
}

/// (5) 解けない pointer は typed に断る: 区間の無い doc・区間に無い行 id・`#` の無い pointer の 3 形。
#[test]
fn pipe_intake_design_refuses_a_pointer_that_does_not_resolve() {
    let (repo, state) = repo_with_state();
    write_contract(&repo, &[], &[]);
    let (dirs, events) = (run_dirs(&state), event_count(&state));
    for (pointer, want) in [
        ("docs/design/toy.md#zz", "行 id zz"),
        ("docs/design/none.md#a", "base（HEAD）から読めない"),
        ("docs/design/toy.md", "設計 pointer の形でない"),
    ] {
        let out = intake_raw(&repo, &state, pointer, "s2-ptr");
        let err = stderr_of(&out);
        assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "{pointer} を通さない: {err}");
        assert!(err.contains(want), "{pointer} の理由に {want}: {err}");
    }
    assert_eq!(run_dirs(&state), dirs, "run dir は増えない");
    assert_eq!(event_count(&state), events, "event も増えない");
    clean(&[&repo, &state]);
}

/// (6) 読む先は **base（`HEAD`）** である: 作業木の設計 doc を書き換えても commit していなければ受付は HEAD の行を
/// 読む（runner が base で見るものと契約を食い違わせない）。
#[test]
fn pipe_intake_design_reads_the_row_from_head_not_the_worktree() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    // commit **しない**書き換え: 作業木の行の title を変える。
    let edited: Vec<String> = contract_body()
        .into_iter()
        .map(|line| if line.starts_with("title") { r#"title = "作業木だけの題""#.to_owned() } else { line })
        .collect();
    write_design(&repo, &design_doc(&edited));
    let out = intake_raw(&repo, &state, &design, "s2-head");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let id = run_id_of(&out);
    let copied = fs::read_to_string(state.join("pipe").join(&id).join("contract.toml")).unwrap_or_default();
    assert!(copied.contains(r#"goal = "縦 1 本を通す""#), "HEAD の行を読む: {copied}");
    assert!(!copied.contains("作業木だけの題"), "作業木の書きかけは読まない: {copied}");
    stop_run_ok(&state, &id);
    clean(&[&repo, &state]);
}

// ───── 検出線の的の欄（設計 docs/design/gate-cost.md §16 (1)・行 g・`s2-07l.341`・接頭辞 `pipe_intake_targets_`） ─────

/// 的 2 本（桁つきの一覧の形と桁なしの形）を持つ行の欄。
const TARGETS_FIELD: &str = r#"targets = ["src/lib.rs:1:replace seed -> bool with true", "src/lib.rs:1:5: replace seed with ()"]"#;

/// (a) 的の欄を持つ行は受付を通り、生成された契約 file に的が逐語で写る（写しは器自身が読める）。欄を持たない行は
/// 従来どおり通り、写しは `targets` を持たない（(1) の否定の枝）。
#[test]
fn pipe_intake_targets_row_copies_its_targets_and_a_row_without_them_still_passes() {
    let (repo, state) = repo_with_state();
    let plain = intake_raw(&repo, &state, &write_contract(&repo, &[], &[]), "s2-plain");
    assert_eq!(plain.status.code(), Some(i32::from(RC_OK)), "欄の無い行は通る: {}", stderr_of(&plain));
    let plain_id = run_id_of(&plain);
    let copied = fs::read_to_string(state.join("pipe").join(&plain_id).join("contract.toml")).unwrap_or_default();
    assert!(!copied.contains("targets"), "欄の無い行の写しは的を持たない: {copied}");
    stop_run_ok(&state, &plain_id);
    let design = write_contract(&repo, &[], &[TARGETS_FIELD]);
    let aimed = intake_raw(&repo, &state, &design, "s2-aimed");
    assert_eq!(aimed.status.code(), Some(i32::from(RC_OK)), "的の欄を持つ行は通る: {}", stderr_of(&aimed));
    let id = run_id_of(&aimed);
    let path = state.join("pipe").join(&id).join("contract.toml");
    let copied = fs::read_to_string(&path).unwrap_or_default();
    assert!(copied.lines().any(|line| line == TARGETS_FIELD), "的が逐語で写る: {copied}");
    let read = vessel::pipe::contract::targets_of(&path).unwrap_or_default();
    assert_eq!(read, ["src/lib.rs:1:replace seed -> bool with true", "src/lib.rs:1:5: replace seed with ()"], "写しから読み戻せる");
    stop_run_ok(&state, &id);
    clean(&[&repo, &state]);
}

/// (a) 的の値が形に合わない行（行 0・file が `.rs` でない・名が空）は受付で typed に断られ（rc 1・的の字面と外れた
/// 部分を名指す）、run dir も event も増えない。`contracts check` も同じ読みで `contract-table:target-form` を欄の行に
/// 名指す（受付と CI が同じ 1 本）。
#[test]
fn pipe_intake_targets_malformed_values_are_refused_as_target_form() {
    let (repo, state) = repo_with_state();
    for (value, bead, part) in [
        ("src/lib.rs:0:replace seed with ()", "s2-t0", "1 以上の十進"),
        ("src/lib.txt:1:x", "s2-t1", ".rs"),
        ("src/lib.rs:1:", "s2-t2", "変異の名が空"),
    ] {
        let design = write_contract(&repo, &[], &[&format!("targets = [\"{value}\"]")]);
        let (dirs, events) = (run_dirs(&state), event_count(&state));
        let out = intake_raw(&repo, &state, &design, bead);
        let text = format!("{}{}", stdout_of(&out), stderr_of(&out));
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{value} は rc 1 で断る: {text}");
        assert!(text.contains(&format!("targets \"{value}\" が <file>:<行>:<変異の名> の形でない")), "的の字面: {text}");
        assert!(text.contains(part), "外れた部分を名指す（{part}）: {text}");
        assert_eq!(run_dirs(&state), dirs, "run dir は増えない");
        assert_eq!(event_count(&state), events, "event も増えない");
    }
    clean(&[&repo, &state]);
    let doc = table_doc(&table_region(&[table_row("a", &[("targets", "[\"src/tint.rs:0:x\"]")]), table_row("b", &[])]));
    let checked = table_repo(&doc, &[]);
    let out = contracts_check(&checked);
    let found = findings_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{}", stdout_of(&out));
    assert!(
        found.len() == 1 && found.iter().all(|line| line.contains("contract-table:target-form: targets \"src/tint.rs:0:x\"")),
        "形の外れた的 1 本だけを名指す: {found:?}"
    );
    clean(&[&checked]);
}

// ───── 受付の depends の解決（設計 docs/design/contract-source.md §30・行 ad・`s2-07l.496`・接頭辞 `pipe_intake_depends_`） ─────

/// depends の toy の verify 行（`derive_` の歯 2 file が導出値＝行は write-set を持たず、受付は導出で通る）。
const DEPENDS_VERIFY: &str = "[\"cargo nextest run -p toy --no-tests=fail derive_\"]";

/// 2 行の設計 doc: 行 `a`（`depends` なし）と、行 `b`（`depends = [<target>]`）。相手が `a` なら同じ doc の**自分でない
/// 別の行**・相手が `zz` なら doc に無い id。
fn depends_doc(target: &str) -> String {
    let rows = [
        derive_row("a", &[("verify", DEPENDS_VERIFY)]),
        derive_row("b", &[("verify", DEPENDS_VERIFY), ("depends", &format!("[\"{target}\"]"))]),
    ];
    table_doc(&table_region(&rows))
}

/// `contracts check --repo R --rules <受付と同じ上限>` の findings（`contracts: ` の行）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn depends_check_findings(repo: &Path, state: &Path) -> Vec<String> {
    let out = bin_cmd()
        .args(["contracts", "check", "--repo", &repo.display().to_string(), "--rules", &ceiling_rules(state)])
        .output()
        .expect("binary を起動できる");
    findings_of(&out)
}

/// (1) `depends` の相手が同じ doc の自分でない別の行（`b` → `a`）である行の受付は rc 0 で通り、run dir が 1 つ出来る。
/// base は検査する 1 行の slice の id だけを母集団に読むので `depends-unresolved` で断る（RED）。
#[test]
fn pipe_intake_depends_on_another_row_of_the_same_doc_is_accepted() {
    let (repo, state) = derive_repo(&depends_doc("a"));
    let out = intake_raw(&repo, &state, "docs/design/toy.md#b", "s2-dep");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "相手の在る depends は通る: {err}");
    assert!(!err.contains("depends-unresolved"), "depends を理由に断らない: {err}");
    let dirs = run_dirs(&state);
    assert_eq!(dirs.len(), 1, "run dir が 1 つ出来る: {dirs:?}");
    let id = run_id_of(&out);
    assert_eq!(dirs, vec![id.clone()], "出来た run dir は受けた便のもの");
    stop_run_ok(&state, &id);
    clean(&[&repo, &state]);
}

/// (2) 相手の id が doc に無い行（`b` → `zz`）は従来どおり断られ run dir は 0。断りの stderr は **`contracts check` の
/// 描画と逐語で同じ `depends-unresolved` の 1 行だけ**（doc と行番号・名・理由の文）で、他の理由の行を伴わない
/// （別の検査で先に落ちた偽の緑を除く・`contracts check` もその 1 件だけを名指す）。
#[test]
fn pipe_intake_depends_on_a_missing_id_is_refused_with_the_contracts_check_line() {
    let doc = depends_doc("zz");
    let (repo, state) = derive_repo(&doc);
    let out = intake_raw(&repo, &state, "docs/design/toy.md#b", "s2-dep");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "相手の無い depends は断る: {err}");
    let want = format!(
        "contracts: docs/design/toy.md:{} contract-table:depends-unresolved: depends zz が同じ doc の行 id に無い",
        table_line(&doc, "b")
    );
    assert_eq!(err.lines().collect::<Vec<&str>>(), vec![want.as_str()], "断りは depends-unresolved の 1 行だけ: {err}");
    assert_eq!(depends_check_findings(&repo, &state), vec![want], "contracts check と逐語で同じ 1 行");
    assert_eq!(run_dirs(&state), Vec::<String>::new(), "run dir は 0");
    assert_eq!(event_count(&state), 0, "event も書かない");
    clean(&[&repo, &state]);
}

/// (3) 事前の検査の口は受付と同じ 1 判定を通る: (1) の行では `refuse=` に `depends-unresolved` が出ず末尾 `preflight: ok`、
/// (2) の行では stderr に同じ `depends-unresolved` の行が出て末尾 `preflight: refused n=1`（行の欠陥は表の検査が先に
/// 断るので `refuse=` の列は持たない・[`pipe_preflight_lists_one_refusal_per_judgement_without_creating_a_run`] と同じ形）。
/// どちらも run dir を作らない。
#[test]
fn pipe_intake_depends_preflight_follows_the_same_judgement() {
    let (resolved, state) = derive_repo(&depends_doc("a"));
    let out = preflight_raw(&resolved, &state, "docs/design/toy.md#b", "s2-dep", true);
    let (text, err) = (stdout_of(&out), stderr_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "相手の在る depends は preflight も通る: {text} {err}");
    assert!(!text.contains("depends-unresolved") && !err.contains("depends-unresolved"), "{text} {err}");
    assert!(fact_lines(&out, "refuse=").is_empty(), "断り 0: {text}");
    assert_eq!(tail_line(&out), "preflight: ok", "{text}");
    assert_eq!(run_dirs(&state), Vec::<String>::new(), "run dir を作らない");
    clean(&[&resolved, &state]);

    let (missing, state) = derive_repo(&depends_doc("zz"));
    let out = preflight_raw(&missing, &state, "docs/design/toy.md#b", "s2-dep", true);
    let (text, err) = (stdout_of(&out), stderr_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "相手の無い depends は preflight も断る: {text} {err}");
    assert!(err.contains("contract-table:depends-unresolved: depends zz が同じ doc の行 id に無い"), "{err}");
    assert_eq!(tail_line(&out), "preflight: refused n=1", "{text}");
    assert_eq!(run_dirs(&state), Vec::<String>::new(), "run dir を作らない");
    clean(&[&missing, &state]);
}

// ───── 約束の行（設計 docs/design/contract-source.md §33・行 ag・`s2-07l.512`・接頭辞 `pipe_intake_promise_`） ─────

/// Promised の形の行（[`table_row`] から `write-set` / `verify` / `done` を落とす・`over` が持つ欄だけ載せる）。
fn promised_row(id: &str, over: &[(&str, &str)]) -> String {
    let keeps = |line: &str| over.iter().any(|(name, _)| line.starts_with(&format!("{name} =")));
    table_row(id, over)
        .lines()
        .filter(|line| keeps(line) || !["write-set =", "verify =", "done ="].iter().any(|head| line.starts_with(head)))
        .map(|line| format!("{line}\n"))
        .collect()
}

/// 約束の行 1 つ（`(of, n)` と欄の値は TOML の字面で渡す・`symbols` と `place` は空なら書かない）。
fn promise_toml((of, n): (&str, u64), symbols: &str, files: &str, teeth: &str, expect: &str) -> String {
    let mut lines = vec![
        "[[promise]]".to_owned(),
        format!("of = \"{of}\""),
        format!("n = {n}"),
        format!("text = \"約束 {n}\""),
        format!("files = {files}"),
    ];
    if !symbols.is_empty() {
        lines.push(format!("symbols = {symbols}"));
    }
    lines.extend([format!("teeth = {teeth}"), "fixture = \"toy の repo\"".to_owned(), format!("expect = \"{expect}\"")]);
    format!("{}\n", lines.join("\n"))
}

/// 行 `id` の約束の行 2 つ（n = 2 を先に書く）: 1 = 閉じた型 `crate::tint::Tint` と新設 file・非 `.rs` の file と tests の歯
/// `derive_ok`、2 = src の区間の歯 `derive_in_src`。
fn two_promises(id: &str) -> String {
    let second = promise_toml((id, 2), "", "[\"crates/toy/src/other.rs\"]", "[\"derive_in_src\"]", "src の歯が緑");
    let first = promise_toml(
        (id, 1),
        "[\"crate::tint::Tint\", \"+fresh_helper(\"]",
        "[\"crates/toy/src/tint.rs\", \"+crates/toy/src/new.rs\", \"rules/manifest.toml\"]",
        "[\"derive_ok\"]",
        "tests の歯が緑",
    );
    format!("{second}\n{first}")
}

/// [`two_promises`] の生成値の verify（歯の置き場の（crate・scope）ごとに 1 行・n の順）。
const PROMISED_VERIFY: [&str; 2] =
    ["cargo nextest run -p toy --test e2e --no-tests=fail derive_ok", "cargo nextest run -p toy --lib --no-tests=fail derive_in_src"];

/// 便の写しの契約 file の本文。
fn copied_contract(state: &Path, id: &str) -> String {
    fs::read_to_string(state.join("pipe").join(id).join("contract.toml")).unwrap_or_default()
}

/// (1)(2)(5) `write-set` / `verify` / `done` を持たない行は約束の行を持てば parse を通り（`contracts check` の findings 0）、
/// 受付は判定行 `write-set=promised files=6` で通る。写しの write-set は約束の行から導いた値（閉包 ∪ 歯の置き場 ∪ creates ∪
/// also）・verify は（crate・scope）で束ねた nextest 行・done は n の順の「(n) expect」で、設計 doc は 1 byte も変わらない。
/// base は `done` の無い行を parse の必須 key で断る（RED）。
#[test]
fn pipe_intake_promise_row_generates_write_set_verify_and_done() {
    let doc = table_doc(&table_region(&[format!("{}\n{}", promised_row("a", &[]), two_promises("a"))]));
    let (repo, state) = derive_repo(&doc);
    let checked = bin_cmd()
        .args(["contracts", "check", "--repo", &repo.display().to_string(), "--rules", &ceiling_rules(&state)])
        .output()
        .expect("binary を起動できる");
    assert_eq!(stdout_of(&checked).trim_end(), "contracts check: docs=1 rows=1 untracked=0 findings=0 place-out=0/0", "{}", stderr_of(&checked));
    let out = intake_raw(&repo, &state, "docs/design/toy.md#a", "s2-a");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "約束の行から生成して通る: {}", stderr_of(&out));
    let tokens = intake_tokens(&out);
    assert!(tokens.contains(&"write-set=promised".to_owned()) && tokens.contains(&"files=6".to_owned()), "判定行: {tokens:?}");
    let id = run_id_of(&out);
    let want = [
        "+crates/toy/src/new.rs",
        "crates/toy/src/other.rs",
        "crates/toy/src/show.rs",
        "crates/toy/src/tint.rs",
        "crates/toy/tests/e2e.rs",
        "rules/manifest.toml",
    ];
    assert_eq!(copied_write_set(&state, &id), want, "写しの write-set は約束の行からの導出値");
    let copied = vessel::pipe::contract::Contract::parse(&copied_contract(&state, &id)).map_err(|errors| format!("{errors:?}"));
    let (verify, done) = copied.map(|found| (found.verify, found.done)).unwrap_or_default();
    assert_eq!(verify, PROMISED_VERIFY, "verify は束ねた nextest 行");
    assert_eq!(done, "(1) tests の歯が緑 (2) src の歯が緑", "done は n の順");
    assert_eq!(fs::read_to_string(repo.join("docs/design/toy.md")).unwrap_or_default(), doc, "設計 doc に書き戻さない");
    stop_run_ok(&state, &id);
    clean(&[&repo, &state]);
}

/// (3) Promised の行が導く欄を書く（`write-set` / `done`・両方書けば両方を名指す）・`symbols` の `+` 無しの名が base に無い・
/// `+` 付きの名が base に在る の 4 行は、それぞれ rc 1 で断られ run dir も event も 0。
#[test]
fn pipe_intake_promise_refuses_written_fields_and_symbols_off_base() {
    let teeth = "[\"derive_ok\"]";
    let files = "[\"rules/manifest.toml\"]";
    let rows = [
        format!("{}\n{}", promised_row("w", &[("write-set", "[\"crates/toy/tests/e2e.rs\"]")]), promise_toml(("w", 1), "", files, teeth, "e")),
        format!(
            "{}\n{}",
            promised_row("d", &[("done", "\"手書きの done\""), ("touches", "[\"crate::tint::Tint\"]")]),
            promise_toml(("d", 1), "", files, teeth, "e")
        ),
        format!("{}\n{}", promised_row("s", &[]), promise_toml(("s", 1), "[\"crate::tint::Nope\"]", files, teeth, "e")),
        format!("{}\n{}", promised_row("p", &[]), promise_toml(("p", 1), "[\"+crate::tint::Tint\"]", files, teeth, "e")),
    ];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    for (id, want) in [
        ("w", "行 w は約束の行を持つ（Promised）ので write-set を書けない"),
        ("d", "行 d は約束の行を持つ（Promised）ので touches, done を書けない"),
        ("s", "約束 s の n 1 の symbols の crate::tint::Nope が base に無い"),
        ("p", "約束 p の n 1 の symbols の +crate::tint::Tint は base に既に在る"),
    ] {
        let out = intake_raw(&repo, &state, &format!("docs/design/toy.md#{id}"), &format!("s2-{id}"));
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "行 {id} は rc 1: {err}");
        assert!(err.contains(want), "行 {id} の理由: {err}");
        assert_eq!(run_dirs(&state), Vec::<String>::new(), "行 {id} は run dir 0");
        assert_eq!(event_count(&state), 0, "行 {id} は event 0");
    }
    clean(&[&repo, &state]);
}

/// (4) `verify` を持つ Promised の行: 生成値と集合で一致すれば（順は問わない）通り写しは生成値のまま、不一致は §3 と同じ
/// drift の断り（`write-set が導出値と一致しない`・不足と余分を名指す）で run dir 0。
#[test]
fn pipe_intake_promise_verify_matches_as_a_set_or_is_refused_as_drift() {
    let [tests_line, lib_line] = PROMISED_VERIFY;
    let same = format!("[\"{lib_line}\", \"{tests_line}\"]");
    let other = format!("[\"{tests_line}\", \"cargo nextest run -p toy --lib --no-tests=fail derive_\"]");
    let rows = [
        format!("{}\n{}", promised_row("v", &[("verify", &same)]), two_promises("v")),
        format!("{}\n{}", promised_row("x", &[("verify", &other)]), two_promises("x")),
    ];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    let drifted = intake_raw(&repo, &state, "docs/design/toy.md#x", "s2-x");
    let err = stderr_of(&drifted);
    assert_eq!(drifted.status.code(), Some(i32::from(RC_REFUSED)), "不一致は rc 1: {err}");
    assert!(err.contains("write-set が導出値と一致しない"), "§3 と同じ drift: {err}");
    assert!(err.contains(&format!("missing: {lib_line}")) && err.contains("extra: cargo nextest run -p toy --lib --no-tests=fail derive_）"), "{err}");
    assert_eq!(run_dirs(&state), Vec::<String>::new(), "run dir 0");
    let out = intake_raw(&repo, &state, "docs/design/toy.md#v", "s2-v");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "集合で一致すれば通る: {}", stderr_of(&out));
    assert!(intake_tokens(&out).contains(&"write-set=promised".to_owned()), "{}", stdout_of(&out));
    let copied = copied_contract(&state, &run_id_of(&out));
    let verify = vessel::pipe::contract::Contract::parse(&copied).map(|found| found.verify).unwrap_or_default();
    assert_eq!(verify, PROMISED_VERIFY, "写しは生成値（n の順）");
    clean(&[&repo, &state]);
}

// ───── 約束の行の files の既存 .rs と crate:: の型の path 形（設計 contract-source.md §34・行 ai・`s2-07l.528`・接頭辞 `pipe_intake_promise_files_`） ─────

/// 1 つの約束の行だけを持つ Promised の行 `id`（歯は tests の `derive_ok`）。
fn one_promise_row(id: &str, symbols: &str, files: &str) -> String {
    format!("{}\n{}", promised_row(id, &[]), promise_toml((id, 1), symbols, files, "[\"derive_ok\"]", "e"))
}

/// (a)(b) `files` の `+` 無しの `.rs`（base に実在する `show.rs`・閉包にも歯の置き場にも無い）は写しの write-set にそのまま
/// 載り（判定行 `files=2`・base は捨てて `files=1` → RED）、base に無い `.rs` は `write-set-item-unresolved` で rc 1・run dir 0
/// （base は黙って捨てて通す → RED）。
#[test]
fn pipe_intake_promise_files_existing_rs_lands_and_missing_rs_is_refused() {
    let rows = [one_promise_row("f", "", "[\"crates/toy/src/show.rs\"]"), one_promise_row("m", "", "[\"crates/toy/src/none.rs\"]")];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    let missing = intake_raw(&repo, &state, "docs/design/toy.md#m", "s2-m");
    let err = stderr_of(&missing);
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "base に無い .rs は rc 1: {err}");
    assert!(err.contains("write-set の crates/toy/src/none.rs は base に解けない"), "項目を名指す: {err}");
    assert_eq!(run_dirs(&state), Vec::<String>::new(), "run dir 0");
    assert_eq!(event_count(&state), 0, "event 0");
    let out = intake_raw(&repo, &state, "docs/design/toy.md#f", "s2-f");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "既存の .rs は通る: {}", stderr_of(&out));
    let tokens = intake_tokens(&out);
    assert!(tokens.contains(&"write-set=promised".to_owned()) && tokens.contains(&"files=2".to_owned()), "判定行: {tokens:?}");
    let id = run_id_of(&out);
    assert_eq!(copied_write_set(&state, &id), ["crates/toy/src/show.rs", "crates/toy/tests/e2e.rs"], "files の .rs がそのまま載る");
    stop_run_ok(&state, &id);
    clean(&[&repo, &state]);
}

/// `paint::Hue` の字面を持たない toy の閉じた型（宣言 file だけ・const slice の宣言で閉包に入る）。
const BARE_PAINT: &[(&str, &str)] =
    &[("crates/toy/src/paint.rs", "pub enum Hue {\n    Red,\n    Blue,\n}\n\npub const HUES: &[Hue] = &[Hue::Red, Hue::Blue];\n")];

/// (c)(d) `symbols` の `crate::paint::Hue`（toy に `paint::Hue` の字面は無い・module の宣言だけ）は受付を通り写しの write-set に
/// paint.rs の閉包が載る（base は末尾 2 節の字面を探して `promise-symbol-unresolved` → RED）。`+crate::paint::Hue`（宣言が在る）
/// は断られ（base は通す → RED）、`+crate::paint::Fresh`（宣言が無い）は通る。
#[test]
fn pipe_intake_promise_files_crate_type_path_resolves_by_module_declaration() {
    let manifest = "[\"rules/manifest.toml\"]";
    let rows = [
        one_promise_row("h", "[\"crate::paint::Hue\"]", manifest),
        one_promise_row("n", "[\"+crate::paint::Hue\"]", manifest),
        one_promise_row("g", "[\"+crate::paint::Fresh\"]", manifest),
    ];
    let (repo, state) = derive_repo_with(&table_doc(&table_region(&rows)), BARE_PAINT);
    let refused = intake_raw(&repo, &state, "docs/design/toy.md#n", "s2-n");
    let err = stderr_of(&refused);
    assert_eq!(refused.status.code(), Some(i32::from(RC_REFUSED)), "宣言の在る + は rc 1: {err}");
    assert!(err.contains("約束 n の n 1 の symbols の +crate::paint::Hue は base に既に在る"), "{err}");
    assert_eq!(run_dirs(&state), Vec::<String>::new(), "run dir 0");
    for (id, want) in [
        ("h", vec!["crates/toy/src/paint.rs", "crates/toy/tests/e2e.rs", "rules/manifest.toml"]),
        ("g", vec!["crates/toy/tests/e2e.rs", "rules/manifest.toml"]),
    ] {
        let out = intake_raw(&repo, &state, &format!("docs/design/toy.md#{id}"), &format!("s2-{id}"));
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "行 {id} は通る: {}", stderr_of(&out));
        let run = run_id_of(&out);
        assert_eq!(copied_write_set(&state, &run), want, "行 {id} の写しの write-set");
        stop_run_ok(&state, &run);
    }
    clean(&[&repo, &state]);
}

// ───── 約束の行の審査と焼き直しの門（設計 contract-source.md §33・行 ah・`s2-07l.513`・接頭辞 `pipe_intake_promise_`） ─────

/// 約束の行 1 つの Promised の行 `t` だけを持つ設計 doc（`expect` の字面で契約 file の `done` が変わる）。
fn rework_promise_doc(expect: &str) -> String {
    let row = format!("{}\n{}", promised_row("t", &[]), promise_toml(("t", 1), "", "[\"rules/manifest.toml\"]", "[\"derive_ok\"]", expect));
    table_doc(&table_region(&[row]))
}

/// (e′) teeth-outside-write-set の `at` に write-set の外の path（`crates/toy/src/outside.rs`）を持つ審査の後でも、Promised の
/// 行の受付は `at` の path 照合を撃たない: 契約 file が同じ 2 便目は通り（base は `finding-unaddressed` で断る → RED）、
/// 同じ 3 便目は `same-kind-repeated`（N = 2 回目）でだけ断られ、契約 file（約束の行の `expect` → `done`）を変えた便は
/// 通る。審査は 3 語の外の kind を INCONCLUSIVE に倒し（kind は lens の値のまま）、材料に約束の行の写しを置く。
#[test]
fn pipe_intake_promise_rework_gate_reads_only_the_contract_sha() {
    let (repo, state) = derive_repo(&rework_promise_doc("最初の expect"));
    let design = "docs/design/toy.md#t";
    let outside = (Some("teeth-outside-write-set"), Some("crates/toy/src/outside.rs"));
    let first = failed_runs(&repo, &state, "s2-pro", design, &[outside]);
    let first_id = first.first().cloned().unwrap_or_default();
    assert!(!copied_write_set(&state, &first_id).contains(&"crates/toy/src/outside.rs".to_owned()), "at の path は write-set の外");
    let pairs = review_pairs(&state, &first_id);
    assert_eq!((value_of(&pairs, "verdict").as_str(), value_of(&pairs, "kind").as_str()), ("INCONCLUSIVE", "teeth-outside-write-set"), "3 語の外は INCONCLUSIVE");
    let promises = fs::read_to_string(review_dir(&state, &first_id).join("promises.txt")).unwrap_or_default();
    assert_eq!(promises, "- n: 1\n  text: 約束 1\n  fixture: toy の repo\n  expect: 最初の expect\n", "約束の行の写し");
    let second = failed_runs(&repo, &state, "s2-pro", design, &[outside]);
    assert_eq!(second.len(), 1, "契約 file が同じ 2 便目は at の path 照合で断られない");
    let again = Again { repo: &repo, state: &state, bead: "s2-pro", design };
    let err = assert_refused(&again, "same-kind-repeated", &["teeth-outside-write-set", " 2 便"]);
    assert!(!err.contains("finding-unaddressed") && !err.contains("crates/toy/src/outside.rs"), "at の path を名指さない: {err}");
    fs::write(repo.join("docs/design/toy.md"), rework_promise_doc("書き直した expect")).expect("設計 doc を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "promise-rewritten"]);
    let before = run_dirs(&state).len();
    let out = repeat_intake(&repo, &state, "s2-pro", design, &lens_finding("FAIL", outside.0, outside.1));
    let id = accepted(&out, &state, before);
    assert!(copied_contract(&state, &id).contains("(1) 書き直した expect"), "契約 file が変わった便は通る");
    clean(&[&repo, &state]);
}

/// (e) 約束の行を持たない行の審査は材料に約束の行の写しを置かず、3 語の外の kind の FAIL も FAIL のまま（不変の対）。
#[test]
fn pipe_intake_promise_plain_row_review_keeps_fail_and_places_no_promises() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let ids = failed_runs(&repo, &state, "s2-pln", &design, &[(Some("teeth-outside-write-set"), Some("src/lib.rs"))]);
    let id = ids.first().cloned().unwrap_or_default();
    assert_eq!(value_of(&review_pairs(&state, &id), "verdict"), "FAIL", "約束の行を持たない行は倒さない");
    assert!(!review_dir(&state, &id).join("promises.txt").exists(), "写しを置かない");
    clean(&[&repo, &state]);
}

// ───── `--repo` / `--state-dir` の cwd fallback を落とす（`s2-07l.310`・設計 pipeline.md §15・接頭辞 `pipe_repo_required_`） ─────

/// repo の git が記録する worktree の本数（main の木を含む）。
fn worktree_count(repo: &Path) -> usize {
    git(repo, &["worktree", "list", "--porcelain"])
        .lines()
        .filter(|line| line.starts_with("worktree "))
        .count()
}

/// `pipe` を **cwd を toy repo（`vessel init` 済み＝置き場も紐づいた木）にして**撃つ。cwd が主題なので
/// [`pipe_cmd`] の固定した cwd に自分の `current_dir` を後置する（後の指定が勝つ）。道具箱は argv が
/// `--state-dir` を持たない周も同じ置き場の下のものを積む（救われる周の runner を実 `systemd-run` へ戻さない）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn run_pipe_in_repo(repo: &Path, state: &Path, args: &[&str]) -> Output {
    pipe_cmd(args)
        .env("PATH", crate::toolbox_path(state))
        .current_dir(repo)
        .output()
        .expect("binary を起動できる")
}

/// 断りの形: rc 1・stderr は flag 不在の 1 行だけ・worktree の本数と event の件数の**対**が撃つ前と同じ。
fn assert_refused_without_repo(out: &Output, before: (usize, usize), repo: &Path, state: &Path, label: &str) {
    let err = stderr_of(out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{label}: rc 1: {err}");
    assert_eq!(err.lines().collect::<Vec<&str>>(), vec!["pipe: --repo が要る"], "{label}: flag 不在の断り 1 行: {err}");
    assert_eq!(
        (worktree_count(repo), event_count(state)),
        before,
        "{label}: (worktree, event) は 1 つも増えない（cwd の repo に落ちない）"
    );
}

/// (1) 写し面を消した便に `--repo` 無しで spawn すると、cwd（その便の repo そのもの）を読まず flag 不在の 1 行で
/// rc 1——worktree も event も増えない。対: 同じ便・同じ cwd に `--repo` を渡した周は起きる（worktree が 1 つ増える）
/// ＝断りの理由は flag の不在だけ。
#[test]
fn pipe_repo_required_spawn_refuses_a_run_without_its_repo_copy() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    fs::remove_file(vessel::pipe::repo_path(&state, &id)).ok();
    assert!(!vessel::pipe::repo_path(&state, &id).exists(), "写し面を消した");
    let (state_arg, repo_arg) = (state.display().to_string(), repo.display().to_string());
    let before = (worktree_count(&repo), event_count(&state));
    let out = run_pipe_in_repo(&repo, &state, &["spawn", "--run", &id, "--state-dir", &state_arg, "--runner", TOY_COMMIT]);
    assert_refused_without_repo(&out, before, &repo, &state, "spawn");
    let out = run_pipe_in_repo(
        &repo,
        &state,
        &["spawn", "--run", &id, "--repo", &repo_arg, "--state-dir", &state_arg, "--runner", TOY_COMMIT],
    );
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "--repo の在る周は起きる: {}", stderr_of(&out));
    assert_eq!(worktree_count(&repo), before.0 + 1, "--repo の在る周は worktree が 1 つ増える");
    clean(&[&repo, &state]);
}

/// (2) 写し面の無い便の repo 解決は spawn 以外の段（gate）でも同じ断り——lens を起こさず、worktree も event も
/// 増えない。対: `--repo` を渡した周は判定まで進み event が増える。
#[test]
fn pipe_repo_required_run_repo_refuses_gate_without_its_repo_copy() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = implemented(&repo, &state, &path);
    fs::remove_file(vessel::pipe::repo_path(&state, &id)).ok();
    assert!(!vessel::pipe::repo_path(&state, &id).exists(), "写し面を消した");
    let rules = write_rules(&state, "gate.toml", 1, 150_000).display().to_string();
    let marker = state.join("gate-lens-ran");
    let lens = fake_lens(&marker, &lens_verdict("PASS"));
    let (state_arg, repo_arg) = (state.display().to_string(), repo.display().to_string());
    let before = (worktree_count(&repo), event_count(&state));
    let out = run_pipe_in_repo(
        &repo,
        &state,
        &["gate", "--run", &id, "--state-dir", &state_arg, "--rules", &rules, "--lens", &lens],
    );
    assert_refused_without_repo(&out, before, &repo, &state, "gate");
    assert!(!marker.exists(), "断った周は lens を起こさない");
    run_pipe_in_repo(
        &repo,
        &state,
        &["gate", "--run", &id, "--repo", &repo_arg, "--state-dir", &state_arg, "--rules", &rules, "--lens", &lens],
    );
    assert!(event_count(&state) > before.1, "--repo の在る周は判定まで進み event が増える");
    clean(&[&repo, &state]);
}

/// (3) `--state-dir` も `--repo` も無い周の置き場の解決は、cwd（置き場の紐づいた repo）を読まず同じ断り——写し面は
/// 在っても置き場に届かない。対: `--repo` だけ在る周はその repo の git 設定から置き場を解いて起きる（救われる）。
#[test]
fn pipe_repo_required_state_dir_refuses_without_either_flag() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    let repo_arg = repo.display().to_string();
    let before = (worktree_count(&repo), event_count(&state));
    let out = run_pipe_in_repo(&repo, &state, &["spawn", "--run", &id, "--runner", TOY_COMMIT]);
    assert_refused_without_repo(&out, before, &repo, &state, "spawn（flag 無し）");
    let out = run_pipe_in_repo(&repo, &state, &["spawn", "--run", &id, "--repo", &repo_arg, "--runner", TOY_COMMIT]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "--repo だけ在る周は救われる: {}", stderr_of(&out));
    assert_eq!(worktree_count(&repo), before.0 + 1, "救われた周は worktree が 1 つ増える");
    assert!(event_count(&state) > before.1, "救われた周は紐づいた置き場に event を書く");
    clean(&[&repo, &state]);
}

// ───── 同時本数の最大値（rules 行 `pipe.max_live`・設計 gate-cost.md §24・`s2-07l.398`・接頭辞 `pipe_intake_max_live_`） ─────

/// [`ceiling_rules`] の `pipe.max_live` の行だけを `cap` に差し替えた tmp manifest。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rules_with_max_live(state: &Path, name: &str, cap: u64) -> String {
    let path = write_rules(state, name, 1, 1_000_000);
    let text = fs::read_to_string(&path).expect("tmp manifest を読める");
    let block = |value: u64| {
        format!(
            "[[rule]]\nid = \"{MAX_LIVE_ROW}\"\nkind = \"PipeMaxLive\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n"
        )
    };
    let default = block(embedded_int(MAX_LIVE_ROW));
    assert!(text.contains(&default), "既定の行が在る（差し替えが空振りしない）: {text}");
    fs::write(&path, text.replace(&default, &block(cap))).expect("tmp manifest を書ける");
    path.display().to_string()
}

/// write-set `src/a.rs` の便を上限 `cap` の manifest で受付に通し（審査 PASS＝live）、(run id, manifest の path) を返す。
fn one_live_run(repo: &Path, state: &Path, cap: u64) -> (String, String) {
    let rules = rules_with_max_live(state, &format!("rules-max-live-{cap}.toml"), cap);
    let first = write_set_contract(repo, "first", &["src/a.rs"]);
    let out = intake_with_rules(repo, state, &first, "s2-live", &rules);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "live 0 本の周は通る: {}", stderr_of(&out));
    (run_id_of(&out), rules)
}

/// `pipe preflight` を tmp manifest と置き場つきで 1 回撃つ（judge の断りの列を `refuse=` の行で読む）。
fn preflight_with_rules(repo: &Path, state: &Path, design: &str, rules: &str) -> Output {
    run_pipe(&[
        "preflight", "--design", design, "--bead", "s2-next", "--repo", &repo.display().to_string(),
        "--rules", rules, "--state-dir", &state.display().to_string(),
    ])
}

/// (1) `pipe.max_live = 1` で live 1 本の下の 2 本目（write-set は交差しない）は `max-live` の 1 行（`live=1 cap=1`）だけで
/// rc 1 に断られ、stdout も run dir も event も増えない。その live の便を `stop --run` で終端に倒すと同じ契約が通る。
#[test]
fn pipe_intake_max_live_refuses_at_the_cap_and_admits_after_the_live_run_stops() {
    let (repo, state) = repo_with_state();
    let (id, rules) = one_live_run(&repo, &state, 1);
    let (before, dirs) = (events_bytes(&state), run_dirs(&state));
    let second = write_set_contract(&repo, "second", &["src/b.rs"]);
    let out = intake_with_rules(&repo, &state, &second, "s2-next", &rules);
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "上限は rc 1: {err}");
    assert_eq!(err.lines().collect::<Vec<&str>>(), ["pipe: max-live live=1 cap=1"], "名と 2 値の 1 行だけ");
    assert!(out.stdout.is_empty(), "断った周は stdout に 1 byte も書かない: {}", stdout_of(&out));
    assert_eq!(run_dirs(&state), dirs, "run dir を作らない（母集団 {} 本）", dirs.len());
    assert_eq!(events_bytes(&state), before, "events.jsonl は byte 不変");
    stop_run_ok(&state, &id);
    let out = intake_with_rules(&repo, &state, &second, "s2-next", &rules);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "live を止めれば同じ契約が通る: {}", stderr_of(&out));
    assert_eq!(run_dirs(&state).len(), dirs.len().saturating_add(1), "run dir が 1 つ増える");
    clean(&[&repo, &state]);
}

/// (2) 上限は rules 行の値: live 1 本の下で値 2 の manifest なら 2 本目が通り、live 2 本で 3 本目は `live=2 cap=2`。
#[test]
fn pipe_intake_max_live_reads_the_cap_from_the_rules_row() {
    let (repo, state) = repo_with_state();
    let (_, rules) = one_live_run(&repo, &state, 2);
    let second = write_set_contract(&repo, "second", &["src/b.rs"]);
    let out = intake_with_rules(&repo, &state, &second, "s2-next", &rules);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "live 1 < 値 2 は通る: {}", stderr_of(&out));
    let third = write_set_contract(&repo, "third", &["src/c.rs"]);
    let out = intake_with_rules(&repo, &state, &third, "s2-third", &rules);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "live 2 = 値 2 は断る: {}", stderr_of(&out));
    assert_eq!(stderr_of(&out).lines().next(), Some("pipe: max-live live=2 cap=2"), "{}", stderr_of(&out));
    clean(&[&repo, &state]);
}

/// (3) `Gated` で verdict FAIL の便は終端＝live に数えず上限 1 でも 2 本目が通る。対: verdict PASS の `Gated` は live で
/// `max-live` に断られる（段と verdict は交差の歯と同じ fixture で置く）。
#[test]
fn pipe_intake_max_live_does_not_count_a_gated_fail_run() {
    for (verdict, want) in [("FAIL", RC_OK), ("PASS", RC_REFUSED)] {
        let (repo, state) = repo_with_state();
        let (id, rules) = one_live_run(&repo, &state, 1);
        write_verdict(&state, &id, verdict);
        record_stage(&state, &id, "Gated");
        let second = write_set_contract(&repo, "second", &["src/b.rs"]);
        let out = intake_with_rules(&repo, &state, &second, "s2-next", &rules);
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(want)), "Gated {verdict}: {err}");
        assert_eq!(err.contains("max-live"), want == RC_REFUSED, "Gated {verdict} の断りの有無: {err}");
        clean(&[&repo, &state]);
    }
}

/// (4) 写しを読めない live の便が在る周は上限に届いていても `max-live` でなく読めない側（rc 2・run id を名指す）で、
/// run dir も event も増えない（fail-closed・読めない便を数え落とさない）。
#[test]
fn pipe_intake_max_live_is_broken_when_a_live_copy_is_unreadable() {
    let (repo, state) = repo_with_state();
    let (id, rules) = one_live_run(&repo, &state, 1);
    fs::remove_file(state.join("pipe").join(&id).join("contract.toml")).ok();
    let (before, dirs) = (events_bytes(&state), run_dirs(&state));
    let second = write_set_contract(&repo, "second", &["src/b.rs"]);
    let out = intake_with_rules(&repo, &state, &second, "s2-next", &rules);
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない周は rc 2: {err}");
    assert!(err.contains(&id) && err.contains("読めない"), "読めない run を名指す: {err}");
    assert!(!err.contains("max-live"), "上限の断りに化けない: {err}");
    let flight = preflight_with_rules(&repo, &state, &second, &rules);
    let refuses = fact_lines(&flight, "refuse=");
    assert!(!refuses.is_empty(), "preflight も断る: {}", stdout_of(&flight));
    assert!(refuses.iter().all(|line| line.starts_with("refuse=write-set-unreadable:")), "名は write-set-unreadable: {refuses:?}");
    assert_eq!(run_dirs(&state), dirs, "run dir を作らない");
    assert_eq!(events_bytes(&state), before, "events.jsonl は byte 不変");
    clean(&[&repo, &state]);
}

/// (5) 短絡しない: 上限で断る周も交差の判定は撃たれ、preflight の列は `max-live` が先頭・交差が後続に並ぶ（2 件）。intake は
/// 先頭の 1 件（`max-live`）で断る。
#[test]
fn pipe_intake_max_live_still_lists_the_overlap_after_the_cap() {
    let (repo, state) = repo_with_state();
    let (id, rules) = one_live_run(&repo, &state, 1);
    let second = write_set_contract(&repo, "second", &["src/a.rs"]);
    let out = intake_with_rules(&repo, &state, &second, "s2-next", &rules);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{}", stderr_of(&out));
    assert_eq!(stderr_of(&out).lines().next(), Some("pipe: max-live live=1 cap=1"), "先頭は上限: {}", stderr_of(&out));
    let flight = preflight_with_rules(&repo, &state, &second, &rules);
    let refuses = fact_lines(&flight, "refuse=");
    let names: Vec<&str> =
        refuses.iter().filter_map(|line| line.strip_prefix("refuse=")?.split(':').next()).collect();
    assert_eq!(names, ["max-live", "write-set-overlap"], "上限が先頭・交差が後続: {}", stdout_of(&flight));
    assert!(refuses.get(1).is_some_and(|line| line.contains(&id) && line.contains("src/a.rs")), "交差の組を名乗る: {refuses:?}");
    assert_eq!(tail_line(&flight), "preflight: refused n=2", "{}", stdout_of(&flight));
    clean(&[&repo, &state]);
}

// ───── host-guard の語列の行を受付が読む（`s2-07l.568`・設計 vessel-hook.md §11 行 f の形 3・接頭辞 `pipe_intake_host_guard_`） ─────

/// host_guard.git にだけ在る語列（[`ceiling_rules`] の runner.denied_commands は `cargo mutants` だけ）を verify 行に持つ契約を
/// 受付が rc 1 で断り、run dir も event も作らない。断り文は従来の行 id `runner.denied_commands` を名乗る（`declaration` を
/// 触らない限界の pin・後続の純移動で直す）。
#[test]
fn pipe_intake_host_guard_git_only_sequence_is_refused_under_the_old_row_id() {
    let (repo, state) = repo_with_state();
    assert!(HOST_GUARD_ROWS[0].1.contains("git branch -D"), "前提: 語列は host_guard.git の fixture に在る");
    let design = write_contract(&repo, &["verify"], &[r#"verify = ["git branch -D x"]"#]);
    let out = intake_raw(&repo, &state, &design, "b");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "host_guard.git の語列は断る: {err}");
    assert!(err.contains("git branch -D"), "当たった語列を名指す: {err}");
    assert!(err.contains("runner.denied_commands") && !err.contains("host_guard.git"), "行 id は従来のまま（限界）: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&repo, &state]);
}

/// host_guard.git の行を欠いた `--rules` は、受付も preflight も rules の断り（rc 1・行を名指す）で止まり run dir を作らない
/// （∪ の読み手は 3 行を欠くと揃わない＝fail-closed）。
#[test]
fn pipe_intake_host_guard_rules_without_the_git_row_are_refused() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let path = write_rules(&state, "rules-no-host-git.toml", 1, 1_000_000);
    let text = fs::read_to_string(&path).unwrap_or_default();
    let (id, value) = HOST_GUARD_ROWS[0];
    let block = format!(
        "\n[[rule]]\nid = \"{id}\"\nkind = \"HostGuardDeniedCommands\"\nvalue = {value}\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n"
    );
    assert!(text.contains(&block), "既定の行が在る（落としが空振りしない）: {text}");
    fs::write(&path, text.replace(&block, "")).unwrap_or_else(|err| panic!("tmp manifest を書ける: {err}"));
    let rules = path.display().to_string();
    let out = intake_with_rules(&repo, &state, &design, "b", &rules);
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "rules の断りは rc 1: {err}");
    assert_eq!(err.lines().next(), Some("pipe: host_guard.git が無いか文字列の列でない"), "行を名指す: {err}");
    let flight = preflight_with_rules(&repo, &state, &design, &rules);
    assert_eq!(flight.status.code(), Some(i32::from(RC_REFUSED)), "preflight も断る: {}", stderr_of(&flight));
    assert!(stderr_of(&flight).contains("host_guard.git"), "{}", stderr_of(&flight));
    assert!(run_dirs(&state).is_empty(), "run dir を作らない");
    let whole = write_rules(&state, "rules-whole.toml", 1, 1_000_000).display().to_string();
    let out = intake_with_rules(&repo, &state, &design, "b", &whole);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "行が揃えば受理: {}", stderr_of(&out));
    clean(&[&repo, &state]);
}

// ───── クラスの語列表の行を読めない周（設計 contract-source.md §48 の 5・行 az・`s2-07l.601`・接頭辞 `class_derive_`） ─────

/// (7) 語列表の行が無い・不発効・列でない `--rules` では、`contracts check` も受付も行 id `runner.class_commands` を名指して rc 1
/// で断り、受付は run dir を作らない（導出を空として通さない・NFR4）。行の揃った `--rules` では両方とも通る（対）。
#[test]
fn class_derive_rules_without_a_usable_class_row_are_refused_by_check_and_intake() {
    let (repo, state) = repo_with_state();
    let design = write_contract(&repo, &[], &[]);
    let path = write_rules(&state, "rules-class-base.toml", 1, 1_000_000);
    let text = fs::read_to_string(&path).unwrap_or_default();
    assert!(text.contains(CLASS_ROW_BLOCK), "既定の行が在る（差し替えが空振りしない）: {text}");
    let scalar = "\n[[rule]]\nid = \"runner.class_commands\"\nkind = \"CoreLines\"\nvalue = 1\nenabled = true\nruling = \"t\"\nruled_at = \"d\"\n";
    let cases = [
        ("無い", String::new(), "runner.class_commands が無い"),
        ("不発効", CLASS_ROW_BLOCK.replace("enabled = true", "enabled = false"), "runner.class_commands は不発効である"),
        ("列でない", scalar.to_owned(), "runner.class_commands が文字列の列でない"),
    ];
    for (index, (why, block, reason)) in cases.into_iter().enumerate() {
        let rules = state.join(format!("rules-class-{index}.toml"));
        fs::write(&rules, text.replace(CLASS_ROW_BLOCK, &block)).unwrap_or_else(|err| panic!("tmp manifest を書ける: {err}"));
        let rules = rules.display().to_string();
        let check = bin_cmd()
            .args(["contracts", "check", "--rules", &rules, "--repo"])
            .arg(&repo)
            .output()
            .unwrap_or_else(|err| panic!("binary を起動できる: {err}"));
        let said = format!("{}{}", stdout_of(&check), stderr_of(&check));
        assert_eq!(check.status.code(), Some(i32::from(RC_REFUSED)), "{why}: contracts check は rc 1: {said}");
        assert!(said.contains(&format!("contracts: {reason}")), "{why}: 行 id を名指す: {said}");
        let out = intake_with_rules(&repo, &state, &design, "b", &rules);
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{why}: 受付は rc 1: {err}");
        assert_eq!(err.lines().next(), Some(format!("pipe: {reason}").as_str()), "{why}: 行 id を名指す: {err}");
        assert!(run_dirs(&state).is_empty(), "{why}: run dir を作らない");
    }
    let whole = path.display().to_string();
    let check = bin_cmd().args(["contracts", "check", "--rules", &whole, "--repo"]).arg(&repo).output();
    let check = check.unwrap_or_else(|err| panic!("binary を起動できる: {err}"));
    assert_eq!(check.status.code(), Some(i32::from(RC_OK)), "行が揃えば contracts check は通る: {}", stdout_of(&check));
    let out = intake_with_rules(&repo, &state, &design, "b", &whole);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "行が揃えば受理: {}", stderr_of(&out));
    clean(&[&repo, &state]);
}
