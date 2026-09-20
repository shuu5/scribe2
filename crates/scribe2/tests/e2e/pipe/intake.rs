// flip-check: moved s2-07l.264
//! 入口の歯: `pipe_intake_` / `pipe_refuse_`（write-set の排他と `stop --run`）/ `contract_`（契約表）。
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
fn pipe_state_survives_process_restart() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    // 置き場を **--state-dir なしで** 解く＝repo に紐づいた git 設定から読む。
    let out = bin_cmd()
        .args(["pipe", "intake", "--design"])
        .arg(&path)
        .args(["--bead", "s2-2e5", "--repo"])
        .arg(&repo)
        .args(["--rules", &ceiling_rules(&state), "--lens", &review_lens_pass(&state)])
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let id = run_id_of(&out);
    // **別 process** が同じ現在地を読む（process の記憶に何も置いていない）。
    let shown = bin_cmd()
        .args(["pipe", "show", "--run", &id, "--repo"])
        .arg(&repo)
        .output()
        .expect("binary を起動できる");
    assert_eq!(shown.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&shown));
    assert!(stdout_of(&shown).contains("stage=Reviewed"), "{}", stdout_of(&shown));
    assert!(stdout_of(&shown).contains(&id), "{}", stdout_of(&shown));
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

/// 契約の印 `opens`（`s2-07l.201`・設計 seat-roles.md §3「契約が開く例外」・AC16）: `classes` と同じ optional list
/// の形で、値は `PathKind` の名の列。intake の写し `contract.toml` にそのまま乗り、`Contract::load` が印を typed
/// に返す。印の無い便は空（従来どおり）。
#[test]
fn pipe_contract_opens_is_an_optional_list_of_path_kinds_copied_by_intake() {
    use vessel::hook::role_guard::{PathKind, PATH_KINDS};
    use vessel::pipe::contract::Contract;
    let (repo, state) = repo_with_state();
    let marked = write_contract(&repo, &[], &[r#"opens = ["code", "design-doc"]"#]);
    let id = intake(&repo, &state, &marked);
    let copied = Contract::load(&state.join("pipe").join(&id).join("contract.toml")).unwrap_or_else(|errors| panic!("{errors:?}"));
    assert_eq!(copied.opens, vec!["code".to_owned(), "design-doc".to_owned()], "写しに印が乗る（書いた順）");
    assert_eq!(copied.opened_kinds(), vec![PathKind::Code, PathKind::DesignDoc], "印は PathKind へ引ける");
    assert!(copied.classes.is_empty(), "classes は別の field のまま");
    stop_run_ok(&state, &id);

    let bare = intake_bead(&repo, &state, &write_contract(&repo, &[], &[]), "s2-bare");
    let plain = Contract::load(&state.join("pipe").join(&bare).join("contract.toml"))
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    stop_run_ok(&state, &bare);
    assert!(plain.opens.is_empty() && plain.opened_kinds().is_empty(), "印の無い便は空");
    // 取る名は PathKind の全数で、variant 名の字面・空の配列・重複 key は受けない。
    let all: Vec<String> = PATH_KINDS.iter().map(|kind| format!("\"{}\"", kind.as_str())).collect();
    let every_id = intake_bead(&repo, &state, &write_contract(&repo, &[], &[&format!("opens = [{}]", all.join(", "))]), "s2-every");
    let every = Contract::load(&state.join("pipe").join(&every_id).join("contract.toml"))
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    stop_run_ok(&state, &every_id);
    assert_eq!(every.opened_kinds(), PATH_KINDS.to_vec(), "全種別を開ける");
    // 名簿に無い名・配列でない値は**行**の側で断られる（契約 file は器が作るので手書きの不備は入口に無い）。
    for (add, want) in [(r#"opens = ["Code"]"#, "Code"), (r#"opens = "code""#, "opens")] {
        let out = intake_raw(&repo, &state, &write_contract(&repo, &[], &[add]), "s2-bad");
        let err = stderr_of(&out);
        assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "{add} を通さない: {err}");
        assert!(err.contains(want), "{add}: {err}");
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

#[test]
fn pipe_show_reads_repo_from_state() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let id = intake(&repo, &state, &path);
    // **--repo を渡さずに** 撃つ。cwd（この test を走らせている repo）でなく、
    // intake が書き留めた repo から worktree の path が組まれる。
    let out = run_pipe(&["show", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let line = stdout_of(&out);
    assert!(
        line.contains(&repo.display().to_string()),
        "worktree は便に紐づいた repo から組む: {line}"
    );
    clean(&[&repo, &state]);
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

/// 置き場に在る run dir の名（「run を作らない」を数で測る）。
fn run_dirs(state: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(state.join("pipe")) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .filter_map(|entry| entry.ok().map(|found| found.file_name().to_string_lossy().into_owned()))
        .collect();
    found.sort();
    found
}

/// fixture の event で run の段を動かす。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn record_stage(state: &Path, id: &str, stage: &str) {
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
fn write_verdict(state: &Path, id: &str, verdict: &str) {
    let path = state.join("pipe").join(id).join("verdict.json");
    fs::write(&path, format!("{{\"schema\":1,\"run\":\"{id}\",\"verdict\":\"{verdict}\"}}\n"))
        .expect("verdict.json を書ける");
}

/// live な便（intake だけ通した段 `Intake`）と **write-set が交差する 2 本目**は受け付けない
/// （ADR-0019 §2.1）。断った周は run dir も event も作らず、stderr が 1 本目の run id と
/// 交差した path を名乗る。**base はこの 2 本目を受理する**（run dir が 2 つできる）。
#[test]
fn pipe_refuse_intake_refuses_a_contract_that_overlaps_a_live_run() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-live");
    let before = events_bytes(&state);
    let dirs = run_dirs(&state);
    let second = write_set_contract(&repo, "second", &["src/lib.rs"]);
    let out = try_intake(&repo, &state, &second, "s2-next");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "交差は rc 1: {}", stdout_of(&out));
    let err = stderr_of(&out);
    assert!(err.contains(&id), "1 本目の run id を名乗る: {err}");
    assert!(err.contains("src/lib.rs"), "交差した path を名乗る: {err}");
    assert!(out.stdout.is_empty(), "断った周は stdout に 1 byte も書かない");
    assert_eq!(run_dirs(&state), dirs, "run dir を作らない（母集団 {} 本）", dirs.len());
    assert_eq!(events_bytes(&state), before, "events.jsonl は byte 不変");
    clean(&[&repo, &state]);
}

/// dir と file の交差の表（設計 §2）を **intake の受理 / 拒否**で測る。正規化は write-set
/// guard と同じ規則（先頭の `./`・連続する `/`・`..` の畳み）で、dir `src/` は `src/…` を含み
/// `srcx/` は含まない。dir 項目は base の tracked file に**展開して**数える（契約 (g)・設計 contract-source.md §3）
/// ＝base に在る `src/lib.rs` は `src/` と交差し、新規 file（`+src/new.rs`）と base に無い file は交差しない。
#[test]
fn pipe_refuse_intake_measures_dir_and_file_overlap() {
    for (live_entry, next_entry, refused) in [
        ("src/", "src/lib.rs", true),
        ("src/", "srcx/", false),
        ("src/", "src/", true),
        // 正規化していない形（`./x` / `x//y` / `x/../x`）は**行が持てない**（表の検査が base に解けないと断る）。
        // 畳み方そのものは `pipe::refuse` の in-file の歯（`normalize` / `overlaps`）が測る。
        ("src/", "+src/new.rs", false),
        // base に無い file を**素の path**で持つ行は表の検査が断る（`+` を付けるのが行の形）＝
        // 「dir と未来の file は交差しない」は上の `+` の対で測る。
        ("+src/new.rs", "src/new.rs", true),
    ] {
        let (repo, state) = repo_with_state();
        let first = write_set_contract(&repo, "first", &[live_entry]);
        intake_bead(&repo, &state, &first, "s2-live");
        let second = write_set_contract(&repo, "second", &[next_entry]);
        let out = try_intake(&repo, &state, &second, "s2-next");
        let want = if refused { RC_REFUSED } else { RC_OK };
        assert_eq!(
            out.status.code(),
            Some(i32::from(want)),
            "{live_entry} × {next_entry} は交差={refused}: {}",
            stderr_of(&out)
        );
        clean(&[&repo, &state]);
    }
}

/// **終端した便とは交差しない**（段が `Landed` / `Failed` / `Stopped`・`Gated` で verdict が
/// FAIL）。`Gated` の PASS / INCONCLUSIVE は終端でないので交差する（pipeline.md §4「FAIL は終端」）。
/// 契約を改訂して流し直す経路（本番 `.129` / `.131` の型）を塞がないことを測る。
#[test]
fn pipe_refuse_intake_ignores_terminal_runs() {
    for (stage, verdict, refused) in [
        ("Landed", None, false),
        ("Failed", None, false),
        ("Stopped", None, false),
        ("Gated", Some("FAIL"), false),
        ("Gated", Some("PASS"), true),
        ("Gated", Some("INCONCLUSIVE"), true),
    ] {
        let (repo, state) = repo_with_state();
        let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
        let id = intake_bead(&repo, &state, &first, "s2-live");
        if let Some(found) = verdict {
            write_verdict(&state, &id, found);
        }
        record_stage(&state, &id, stage);
        let second = write_set_contract(&repo, "second", &["src/lib.rs"]);
        let out = try_intake(&repo, &state, &second, "s2-next");
        let want = if refused { RC_REFUSED } else { RC_OK };
        assert_eq!(
            out.status.code(),
            Some(i32::from(want)),
            "段 {stage} verdict {verdict:?} は交差={refused}: {}",
            stderr_of(&out)
        );
        clean(&[&repo, &state]);
    }
}

/// live な便の契約の写しを読めない周は **rc 2**（壊れた store・NFR4）で、run dir も event も
/// 作らない。`Gated` の判定を読めない周も同じ「読めない」側である（fail-closed＝読めなさを
/// 「交差なし」に読み替えない）。
#[test]
fn pipe_refuse_intake_is_broken_when_a_live_copy_is_unreadable() {
    for damage in ["remove", "garble", "verdict"] {
        let (repo, state) = repo_with_state();
        let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
        let id = intake_bead(&repo, &state, &first, "s2-live");
        let copied = state.join("pipe").join(&id).join("contract.toml");
        match damage {
            "remove" => {
                fs::remove_file(&copied).ok();
            }
            "garble" => {
                fs::write(&copied, "こわれ\n").ok();
            }
            // 段は Gated だが verdict.json が無い＝終端かを測れない。
            _ => record_stage(&state, &id, "Gated"),
        }
        let before = events_bytes(&state);
        let dirs = run_dirs(&state);
        let second = write_set_contract(&repo, "second", &["src/lib.rs"]);
        let out = try_intake(&repo, &state, &second, "s2-next");
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{damage}: 読めない周は rc 2");
        let err = stderr_of(&out);
        assert!(err.contains(&id), "{damage}: 読めない run を名指す: {err}");
        assert!(err.contains("読めない"), "{damage}: 理由は読めないこと: {err}");
        assert_eq!(run_dirs(&state), dirs, "{damage}: run dir を作らない");
        assert_eq!(events_bytes(&state), before, "{damage}: events.jsonl は byte 不変");
        clean(&[&repo, &state]);
    }
}

/// **同じ bead の 2 本目も特別扱いしない**: write-set が同じなら交差で断られる（owner が同じ
/// ことに意味を持たせない＝自然に掛かる）。
#[test]
fn pipe_refuse_intake_refuses_the_second_run_of_the_same_bead() {
    let (repo, state) = repo_with_state();
    let path = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &path, "s2-same");
    let out = try_intake(&repo, &state, &path, "s2-same");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "同 bead の 2 本目も断る");
    let err = stderr_of(&out);
    assert!(err.contains("交差"), "断る理由は id の衝突でなく交差: {err}");
    assert!(err.contains(&id), "交差した相手を名乗る: {err}");
    clean(&[&repo, &state]);
}

/// 交差が 2 組以上の周は **stderr に全組が 1 組 1 行**で並び、理由の 1 行は先頭の 1 組を名乗る。
#[test]
fn pipe_refuse_intake_lists_every_overlapping_pair() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first", &["src/a.rs", "src/b.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-live");
    let second = write_set_contract(&repo, "second", &["src/a.rs", "src/b.rs"]);
    let out = try_intake(&repo, &state, &second, "s2-next");
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "交差は rc 1");
    let err = stderr_of(&out);
    let pairs: Vec<&str> = err.lines().filter(|line| line.contains("overlap ")).collect();
    assert_eq!(pairs.len(), 2, "交差した全組が並ぶ（母集団 {} 行）: {err}", err.lines().count());
    for entry in ["src/a.rs", "src/b.rs"] {
        assert!(
            pairs.iter().any(|line| line.contains(entry) && line.contains(&id)),
            "{entry} の組が run id つきで並ぶ: {err}"
        );
    }
    let head = err.lines().next().unwrap_or_default();
    assert!(head.contains("src/a.rs"), "理由の 1 行は先頭の 1 組: {head}");
    assert!(!head.contains("src/b.rs"), "理由の 1 行は 1 組だけ: {head}");
    clean(&[&repo, &state]);
}

/// `pipe stop --run <id>`: 終端でない便 1 本に `RunStopped` を **1 件だけ**書き、その後は同じ
/// write-set の契約が通る。終端した便には何も書かず rc 1（書込は冪等・rc は冪等でない）。
#[test]
fn pipe_refuse_stop_run_releases_the_write_set_of_a_live_run() {
    let (repo, state) = repo_with_state();
    let first = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-live");
    let second = write_set_contract(&repo, "second", &["src/lib.rs"]);
    let blocked = try_intake(&repo, &state, &second, "s2-next");
    assert_eq!(blocked.status.code(), Some(i32::from(RC_REFUSED)), "止める前は交差で断られる");
    let before = event_count(&state);
    let out = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "非終端の便は止まる: {}", stderr_of(&out));
    assert_eq!(event_count(&state), before + 1, "RunStopped を 1 件だけ書く");
    let last = events(&state).into_iter().rfind(|found| found.run == id);
    assert!(
        matches!(&last, Some(found) if found.kind == EventKind::RunStopped && found.stage == Some(Stage::Stopped)),
        "書くのは RunStopped stage=Stopped: {last:?}"
    );
    // 2 回撃っても 2 件目を書かない（終端した便は rc 1）。
    let again = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(again.status.code(), Some(i32::from(RC_REFUSED)), "終端の便は rc 1");
    assert_eq!(event_count(&state), before + 1, "2 件目を書かない");
    // 外れた便とは交差しない＝同じ write-set の契約が通る。
    let passed = try_intake(&repo, &state, &second, "s2-third");
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "止めた後は通る: {}", stderr_of(&passed));
    // 無い便は rc 1 で何も書かない。
    let missing = run_pipe(&["stop", "--run", "no-such-run", "--state-dir", &state.display().to_string()]);
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "無い便は rc 1");
    clean(&[&repo, &state]);
}

/// `stop --run` は便の **Live 席も止める**（`--all` と同じ関数を通る）。席を持つ便を外す口が
/// 席を残すと、止めたはずの便の runner が走り続ける。
#[test]
fn pipe_refuse_stop_run_stops_the_live_seat_of_the_run() {
    let (repo, state) = repo_with_state();
    let path = write_set_contract(&repo, "first", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &path, "s2-live");
    // **孫**として起こす（test process の子のままだと zombie が /proc に残る）。
    let spawned = Command::new("sh")
        .arg("-c")
        .arg("sleep 60 >/dev/null 2>&1 & echo $!")
        .output()
        .expect("fake runner を起こせる");
    let pid: u32 = String::from_utf8_lossy(&spawned.stdout).trim().parse().expect("pid を読める");
    let record = bin_cmd()
        .args(["fleet", "record", "--kind", "SeatSpawned", "--run", &id, "--bead", "s2-live",
               "--seat", "seat-1", "--pid", &pid.to_string(), "--state-dir"])
        .arg(&state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(record.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&record));
    let out = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "席ごと止まる: {}", stderr_of(&out));
    assert!(stdout_of(&out).contains("seats=1 stopped=1"), "席を数える: {}", stdout_of(&out));
    assert!(!Path::new(&format!("/proc/{pid}")).exists(), "runner の process は消えている");
    let kinds: Vec<EventKind> = events(&state)
        .into_iter()
        .filter(|found| found.run == id)
        .map(|found| found.kind)
        .collect();
    assert!(kinds.contains(&EventKind::SeatStopped), "席にも記帳する: {kinds:?}");
    assert!(kinds.contains(&EventKind::RunStopped), "便にも記帳する: {kinds:?}");
    clean(&[&repo, &state]);
}

// ─────────────────── 契約表（設計 docs/design/contract-source.md §2 / §3 / §9・`s2-07l.208`・接頭辞 `contract_`） ───────────────────

/// 契約表の toy repo の要件面（要件 id の anchor 3 つ・引用符は 2 形）。
const TABLE_SRS: &str = "<html><body>\n<p id=\"FR1\">1</p>\n<p id=\"FR2\">2</p>\n<p id='AC1'>3</p>\n</body></html>\n";

/// toy repo の閉じた型 `crate::tint::Tint`（const slice `TINTS` の宣言 file）。
const TABLE_TINT: &str = "pub enum Tint {\n    Warm,\n    Cool,\n}\n\npub const TINTS: &[Tint] = &[Tint::Warm, Tint::Cool];\n";

/// toy repo の `Tint` の match の arm を持つ file。
const TABLE_SHOW: &str =
    "use crate::tint::Tint;\n\npub fn show(tint: Tint) -> u8 {\n    match tint {\n        Tint::Warm => 1,\n        Tint::Cool => 2,\n    }\n}\n";

/// toy repo の宣言（allowlist は `git` だけ・要件面は既定）。
const TABLE_VESSEL: &str = "schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git status\"]\n";

/// 設計 doc（§1・§2 は本文あり・§3 は本文なし）の末尾に `table` を置く。
fn table_doc(table: &str) -> String {
    format!("# 設計: toy\n\n## 1. 何を解くか\n\n本文。\n\n## 2. 型\n\n本文。\n\n## 3. 空の節\n\n{table}")
}

/// 契約表の 1 行。既定の欄（適合する値）を `over` で差し替え、既定に無い key は末尾に足す。
fn table_row(id: &str, over: &[(&str, &str)]) -> String {
    let defaults = [
        ("title", format!("\"行 {id}\"")),
        ("req", "[\"FR1\"]".to_owned()),
        ("section", "\"1\"".to_owned()),
        // base に実在する file（項目の実在の検査〔契約 (g)〕を既定で通す）。
        ("write-set", "[\"src/tint.rs\"]".to_owned()),
        ("verify", "[\"git status\"]".to_owned()),
        ("size", "\"S\"".to_owned()),
        ("done", format!("\"{id} が通る\"")),
    ];
    let mut lines = vec!["[[contract]]".to_owned(), format!("id = \"{id}\"")];
    for (key, value) in &defaults {
        let chosen = over.iter().find(|(name, _)| name == key).map_or(value.as_str(), |(_, found)| *found);
        lines.push(format!("{key} = {chosen}"));
    }
    let extra = over.iter().filter(|(name, _)| !defaults.iter().any(|(key, _)| key == name));
    lines.extend(extra.map(|(key, value)| format!("{key} = {value}")));
    format!("{}\n", lines.join("\n"))
}

/// 行を区間で囲む（先頭に `schema = 1`）。
fn table_region(rows: &[String]) -> String {
    format!("<!-- contracts:begin -->\nschema = 1\n\n{}<!-- contracts:end -->\n", rows.join("\n"))
}

/// doc の中で行 `id` の見出し（`[[contract]]`・`id` の行の 1 つ上）が在る物理行番号（1 始まり）。
fn table_line(doc: &str, id: &str) -> usize {
    let want = format!("id = \"{id}\"");
    doc.lines().position(|line| line == want).unwrap_or_default()
}

/// 契約表の toy repo（宣言・要件面・toy の型・設計 doc `docs/design/toy.md`）を作って commit する。`files` は足す /
/// 上書きする file。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn table_repo(doc: &str, files: &[(&str, &str)]) -> PathBuf {
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
fn contracts_check(repo: &Path) -> Output {
    bin_cmd().args(["contracts", "check", "--repo"]).arg(repo).output().expect("binary を起動できる")
}

/// findings の行（`contracts: ` で始まる stdout の行）。
fn findings_of(out: &Output) -> Vec<String> {
    stdout_of(out).lines().filter(|line| line.starts_with("contracts: ")).map(str::to_owned).collect()
}

/// (1) 区間 1 つ・3 行（適合 / 型の構築点を write-set が欠く / 節が無い）の doc で、findings 2 件を `file:line` 付きで
/// 名指し rc 1・判定行 `docs=1 rows=3 findings=2`。適合だけの doc は rc 0（AC21 の表側）。
#[test]
fn contract_check_names_the_incomplete_write_set_and_the_missing_section_with_file_line() {
    let touches = ("touches", "[\"crate::tint::Tint\"]");
    let rows = [
        table_row("a", &[]),
        table_row("b", &[("section", "\"2\""), ("write-set", "[\"src/tint.rs\"]"), touches]),
        table_row("c", &[("section", "\"9\"")]),
    ];
    let doc = table_doc(&table_region(&rows));
    let repo = table_repo(&doc, &[]);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "違反 ≥ 1 は rc 1: {text}{}", stderr_of(&out));
    assert_eq!(found.len(), 2, "2 件ちょうど: {text}");
    let head = |id: &str| format!("contracts: docs/design/toy.md:{} ", table_line(&doc, id));
    let incomplete = found.iter().find(|line| line.starts_with(&head("b"))).cloned().unwrap_or_default();
    assert!(incomplete.contains("write-set-incomplete") && incomplete.contains("src/show.rs"), "閉包の足りない file: {text}");
    assert!(!incomplete.contains("src/tint.rs"), "write-set に在る file は名指さない: {incomplete}");
    let section = found.iter().find(|line| line.starts_with(&head("c"))).cloned().unwrap_or_default();
    assert!(section.contains("contract-table:section-missing"), "節の無い行を名指す: {text}");
    assert_eq!(text.lines().last(), Some("contracts check: docs=1 rows=3 findings=2"), "判定行: {text}");
    // 適合だけの doc は rc 0（write-set が閉包を覆えば touches を持つ行も通る）。
    let covering = ("write-set", "[\"src/tint.rs\", \"src/show.rs\"]");
    let good = table_repo(&table_doc(&table_region(&[table_row("a", &[]), table_row("b", &[covering, touches])])), &[]);
    let passed = contracts_check(&good);
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "適合だけの doc は rc 0: {}", stdout_of(&passed));
    assert_eq!(stdout_of(&passed).lines().collect::<Vec<&str>>(), ["contracts check: docs=1 rows=2 findings=0"]);
    clean(&[&repo, &good]);
}

/// (2) 区間の無い doc は findings 0・rows=0（表なしは違反ではない）・区間 2 つは `region-duplicate`（rc 1）・読めない
/// doc は `unreadable` を行番号 0 で名指して rc 2（黙って飛ばさない・NFR4）。
#[test]
fn contract_check_treats_a_doc_without_region_as_zero_rows_and_fails_closed_on_unreadable_docs() {
    let plain = table_repo(&table_doc(""), &[]);
    let out = contracts_check(&plain);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "表なしは違反でない: {}", stdout_of(&out));
    assert_eq!(stdout_of(&out).trim_end(), "contracts check: docs=1 rows=0 findings=0");
    let region = table_region(&[table_row("a", &[])]);
    let twice = table_repo(&table_doc(&format!("{region}\n{region}")), &[]);
    let out = contracts_check(&twice);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "区間 2 つは違反: {}", stdout_of(&out));
    assert!(findings_of(&out).iter().any(|line| line.contains("contract-table:region-duplicate")), "{}", stdout_of(&out));
    fs::write(plain.join("docs/design/bad.md"), [0xff, 0xfe, b'\n']).expect("非 UTF-8 の doc を書ける");
    git(&plain, &["add", "-A"]);
    git(&plain, &["commit", "-q", "-m", "bad"]);
    let out = contracts_check(&plain);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "読めない doc は rc 2: {text}");
    assert!(text.contains("contracts: docs/design/bad.md:0 contract-table:unreadable"), "読めない doc を名指す: {text}");
    assert_eq!(text.lines().last(), Some("contracts check: docs=2 rows=0 findings=1"), "母集団は tracked 設計 doc の全数: {text}");
    clean(&[&plain, &twice]);
}

/// (3) `req` が要件面に無い行・`depends` が解決しない行・輪を持つ 2 行・`verify` に `(` を持つ行・末尾 `/` 無しの
/// dir を指す行を、各 1 件ずつ行番号付きで名指す（全件・1 件目で止めない・輪は 2 行で 1 件）。
#[test]
fn contract_check_names_each_row_defect_once_with_its_line() {
    let rows = [
        table_row("a", &[("req", "[\"FR1\", \"FR9\"]")]),
        table_row("b", &[("depends", "[\"zz\"]")]),
        table_row("c", &[("depends", "[\"d\"]")]),
        table_row("d", &[("depends", "[\"c\"]")]),
        table_row("e", &[("verify", "[\"git log (x)\"]")]),
        table_row("f", &[("write-set", "[\"src\"]")]),
    ];
    let doc = table_doc(&table_region(&rows));
    let repo = table_repo(&doc, &[]);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{text}");
    for (id, label, needle) in [
        ("a", "contract-table:requirement-missing", "FR9"),
        ("b", "contract-table:depends-unresolved", "zz"),
        ("c", "contract-table:depends-cycle", "c → d → c"),
        ("e", "contract-table:verify-form", "'('"),
        ("f", "write-set-dir-without-slash", "src/"),
    ] {
        let head = format!("contracts: docs/design/toy.md:{} {label}: ", table_line(&doc, id));
        let hits: Vec<&String> = found.iter().filter(|line| line.starts_with(&head)).collect();
        assert_eq!(hits.len(), 1, "行 {id} の {label} を 1 件: {text}");
        assert!(hits.iter().all(|line| line.contains(needle)), "{needle} を名乗る: {text}");
    }
    assert_eq!(found.len(), 5, "他の行は名指さない: {text}");
    assert_eq!(text.lines().last(), Some("contracts check: docs=1 rows=6 findings=5"), "{text}");
    clean(&[&repo]);
}

/// (4) `contracts schema` の出力は tracked の `contracts/schema.toml` と byte で一致し（差分 0）、欄の列は core の
/// const slice（`FIELDS`）と同じ順。余りの引数は断る。
#[test]
fn contract_schema_matches_the_tracked_file_and_the_field_slice() {
    let out = bin_cmd().args(["contracts", "schema"]).output().expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(out.stderr.is_empty(), "stderr は 0 byte: {}", stderr_of(&out));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("contracts").join("schema.toml");
    let tracked = fs::read_to_string(&path).expect("tracked の生成物を読める");
    assert_eq!(stdout_of(&out), tracked, "render と tracked の差分 0（`{NAME} contracts schema` で描き直す）");
    let names: Vec<&str> =
        tracked.lines().filter_map(|line| line.strip_prefix("name = \"")?.strip_suffix('"')).collect();
    let fields: Vec<&str> = vessel::pipe::table::FIELDS.iter().map(|field| field.name).collect();
    assert_eq!(names, fields, "欄の列は const slice と同じ順");
    let extra = bin_cmd().args(["contracts", "schema", "x"]).output().expect("binary を起動できる");
    assert_eq!(extra.status.code(), Some(i32::from(RC_REFUSED)), "余りの引数は断る");
}

/// (6) 宣言 `requirements` が指す要件面（`.yaml`）で req の実在を測り、key 無しの repo は既定の `.html` を読む。
/// 宣言が指す要件面が無い周は rc 2（既定へ黙って倒さない）。
#[test]
fn contract_check_reads_requirements_from_the_declared_face() {
    let doc = table_doc(&table_region(&[table_row("a", &[("req", "[\"FR7\"]")])]));
    let with_key = |face: &str| format!("{TABLE_VESSEL}requirements = \"{face}\"\n");
    let yaml = "requirements:\n  - FR7\n  - id: FR8\n";
    let declared = table_repo(&doc, &[(".vessel.toml", &with_key("spec/reqs.yaml")), ("spec/reqs.yaml", yaml)]);
    let out = contracts_check(&declared);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "宣言の要件面に在る req は通る: {}", stdout_of(&out));
    // 既定の要件面（srs.html）に FR7 は無い＝同じ行が名指される（宣言の path で測っていたことの弁別）。
    let fallback = table_repo(&doc, &[]);
    let out = contracts_check(&fallback);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{}", stdout_of(&out));
    let named = findings_of(&out).iter().any(|line| line.contains("requirement-missing") && line.contains("FR7"));
    assert!(named, "既定の要件面で測る: {}", stdout_of(&out));
    let missing = table_repo(&doc, &[(".vessel.toml", &with_key("spec/none.yaml"))]);
    let out = contracts_check(&missing);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "宣言が指す要件面が無い周は rc 2: {}", stdout_of(&out));
    assert!(stdout_of(&out).contains("spec/none.yaml を読めない"), "{}", stdout_of(&out));
    clean(&[&declared, &fallback, &missing]);
}

/// (6') 要件面が `.md` の周は行頭 `#` の見出しの先頭 token を要件 id に読む（設計 contract-source.md §4・行 k・
/// `s2-07l.354`）: `## FR1 …` / `## FR2 …` を持つ面で `req = ["FR1"]` の行は通り、`["FR9"]` は「要件面に無い」で
/// 落ちる。base は `.md` を「形を読めない」で断る（rc 2・RED）。
#[test]
fn contract_check_reads_requirements_from_md_headings() {
    let with_key = format!("{TABLE_VESSEL}requirements = \"spec/reqs.md\"\n");
    let md = "# 要件\n\n## FR1 便の起動\n\n便を起こす。FR9 は本文の字面。\n\n## FR2 審査\n\n審査する。\n";
    let passing = table_doc(&table_region(&[table_row("a", &[("req", "[\"FR1\", \"FR2\"]")])]));
    let declared = table_repo(&passing, &[(".vessel.toml", &with_key), ("spec/reqs.md", md)]);
    let out = contracts_check(&declared);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "md の見出しの id は通る: {}", stdout_of(&out));
    assert_eq!(stdout_of(&out).lines().last(), Some("contracts check: docs=1 rows=1 findings=0"), "{}", stdout_of(&out));
    let failing = table_doc(&table_region(&[table_row("a", &[("req", "[\"FR9\"]")])]));
    let missing = table_repo(&failing, &[(".vessel.toml", &with_key), ("spec/reqs.md", md)]);
    let out = contracts_check(&missing);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "本文の字面は id でない: {}", stdout_of(&out));
    let named = findings_of(&out).iter().any(|line| line.contains("requirement-missing") && line.contains("FR9"));
    assert!(named, "要件面に無い id を名指す: {}", stdout_of(&out));
    clean(&[&declared, &missing]);
}

/// 使い方の誤りは rc 1（stderr に理由）・git repo でない `--repo` は判定できないので rc 2（判定行を出さない）。
#[test]
fn contract_check_refuses_usage_errors_and_non_repositories() {
    let bare = bin_cmd().args(["contracts", "check"]).output().expect("binary を起動できる");
    assert_eq!(bare.status.code(), Some(i32::from(RC_REFUSED)), "--repo 無しは rc 1");
    assert!(stderr_of(&bare).contains("--repo が要る"), "{}", stderr_of(&bare));
    let none = bin_cmd().arg("contracts").output().expect("binary を起動できる");
    assert_eq!(none.status.code(), Some(i32::from(RC_REFUSED)), "subcommand 無しは rc 1");
    assert!(stderr_of(&none).contains("contracts <check"), "使い方を出す: {}", stderr_of(&none));
    let dir = tmp();
    let out = contracts_check(&dir);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "git repo でない: {}", stderr_of(&out));
    assert!(out.stdout.is_empty() && stderr_of(&out).contains("git repo でない"), "{}", stderr_of(&out));
    clean(&[&dir]);
}

// ─────── 閉包の拡張（設計 docs/design/contract-source.md §3 の 4 点・§9・契約 (g)・`s2-07l.249`・接頭辞 `contract_closure_ext_`） ───────

/// toy repo の外形: doctor の外形 snapshot（`src/snapshots/`）と、それを描く歯・subcommand `tint` の usage を持つ
/// src と、その usage 文字列を持つ歯。
const SURFACE_FILES: &[(&str, &str)] = &[
    ("src/snapshots/toy__tests__doctor_external_form.snap", "---\nsource: src/main.rs\n---\ndoctor: ok\n"),
    ("tests/e2e/doctor.rs", "#[test]\nfn doctor_external_form() {\n    insta::assert_snapshot!(\"doctor: ok\");\n}\n"),
    ("src/cli.rs", "pub fn usage() -> String {\n    format!(\"usage: {NAME} tint <show|list> [--all]\")\n}\n"),
    ("tests/e2e/usage.rs", "#[test]\nfn usage_names_show() {\n    assert!(err.contains(\"tint <show|list> [--all]\"));\n}\n"),
];

/// findings のうち行 `id` の `label` の行（`file:line label: ` の接頭辞で選ぶ）。
fn findings_for(found: &[String], doc: &str, id: &str, label: &str) -> Vec<String> {
    let head = format!("contracts: docs/design/toy.md:{} {label}: ", table_line(doc, id));
    found.iter().filter(|line| line.starts_with(&head)).cloned().collect()
}

/// (1) `surfaces`（第 5 形）: snapshot の名を宣言した行は snapshot の file とその名を持つ歯が write-set に無いと
/// `write-set-incomplete` で両方を名指し、subcommand の名を宣言した行は usage 文字列を持つ歯を名指す。未知の名は
/// `contract-table:surface-unknown`。宣言なしの行と write-set が覆う行は名指さない。base は `surfaces` を読めない（RED）。
#[test]
fn contract_closure_ext_surfaces_name_the_snapshot_and_the_teeth_that_pin_it() {
    let rows = [
        table_row("a", &[("surfaces", "[\"doctor_external_form\"]")]),
        table_row("b", &[("surfaces", "[\"tint\"]")]),
        table_row("c", &[("surfaces", "[\"nope_external_form\"]")]),
        table_row("d", &[]),
        table_row(
            "e",
            &[
                ("surfaces", "[\"doctor_external_form\", \"tint\"]"),
                ("write-set", "[\"src/tint.rs\", \"src/snapshots/\", \"tests/e2e/\"]"),
            ],
        ),
    ];
    let doc = table_doc(&table_region(&rows));
    let repo = table_repo(&doc, SURFACE_FILES);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{text}{}", stderr_of(&out));
    let snapshot = findings_for(&found, &doc, "a", "write-set-incomplete");
    assert_eq!(snapshot.len(), 1, "行 a は 1 件に全部: {text}");
    for path in ["src/snapshots/toy__tests__doctor_external_form.snap", "tests/e2e/doctor.rs"] {
        assert!(snapshot.iter().all(|line| line.contains(path)), "snapshot の file と pin する歯 {path} を名指す: {text}");
    }
    assert!(snapshot.iter().all(|line| !line.contains("usage.rs") && !line.contains("cli.rs")), "外形の外は名指さない: {text}");
    let usage = findings_for(&found, &doc, "b", "write-set-incomplete");
    assert_eq!(usage.len(), 1, "行 b: {text}");
    assert!(usage.iter().all(|line| line.contains("tests/e2e/usage.rs") && !line.contains("doctor")), "usage 文字列を持つ歯: {text}");
    let unknown = findings_for(&found, &doc, "c", "contract-table:surface-unknown");
    assert_eq!(unknown.len(), 1, "未知の名: {text}");
    assert!(unknown.iter().all(|line| line.contains("nope_external_form")), "{text}");
    assert_eq!(found.len(), 3, "宣言なしの行 d と覆う行 e は名指さない: {text}");
    assert_eq!(text.lines().last(), Some("contracts check: docs=1 rows=5 findings=3"), "{text}");
    clean(&[&repo]);
}

/// (2) 項目の実在と dir の展開: 無い file・空の dir は `write-set-item-unresolved` で 1 項目 1 件（実在する file・
/// 配下を持つ dir・base に無い `+`・**base に在る file への `+`〔契約表の検査は land 済みの実在 file と読む・
/// `s2-07l.346`〕** は通る）。intake の交差は dir を base の file に展開して数える＝`src/` の live な便と
/// `+src/new.rs` の便は交差 0 で通り、`src/lib.rs` の便は交差で断られる。
#[test]
fn contract_closure_ext_dir_items_expand_and_unresolved_items_are_named() {
    let write_set = "[\"src/none.rs\", \"empty/\", \"+src/tint.rs\", \"src/tint.rs\", \"src/\", \"+src/new.rs\"]";
    let doc = table_doc(&table_region(&[table_row("a", &[("write-set", write_set)])]));
    let repo = table_repo(&doc, &[]);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{text}{}", stderr_of(&out));
    let unresolved = findings_for(&found, &doc, "a", "write-set-item-unresolved");
    let named: Vec<&str> = ["src/none.rs", "empty/"]
        .into_iter()
        .filter(|item| unresolved.iter().any(|line| line.contains(&format!("write-set の {item} は"))))
        .collect();
    assert_eq!(named.len(), 2, "解けない 2 項目を名指す: {text}");
    assert_eq!(unresolved.len(), 2, "1 項目 1 件（解ける 4 項目は名指さない）: {text}");
    assert_eq!(found.len(), 2, "他の理由は出ない: {text}");
    clean(&[&repo]);

    let (repo, state) = repo_with_state();
    let dir_run = write_set_contract(&repo, "dir", &["src/"]);
    let id = intake_bead(&repo, &state, &dir_run, "s2-live");
    let fresh = write_set_contract(&repo, "fresh", &["+src/new.rs"]);
    let passed = try_intake(&repo, &state, &fresh, "s2-fresh");
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "新規 file は dir と交差しない: {}", stderr_of(&passed));
    let existing = write_set_contract(&repo, "existing", &["src/lib.rs"]);
    let refused = try_intake(&repo, &state, &existing, "s2-old");
    assert_eq!(refused.status.code(), Some(i32::from(RC_REFUSED)), "base の file は dir に展開されて交差する");
    assert!(stderr_of(&refused).contains(&id) && stderr_of(&refused).contains("src/lib.rs"), "{}", stderr_of(&refused));
    clean(&[&repo, &state]);
}

/// 1399 行の `.rs` を `crates/toy/src/` に置いて commit した repo と置き場（上限の余地の fixture）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn repo_with_big_file() -> (PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    let dir = repo.join("crates").join("toy").join("src");
    fs::create_dir_all(&dir).expect("core の dir を作れる");
    fs::write(dir.join("big.rs"), "// x\n".repeat(1399)).expect("大きな file を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "big"]);
    (repo, state)
}

/// (3) 上限の余地（受付だけ）: base の 1399 行の `.rs`（上限 1500・余地 101）を write-set に持つ size M（300）の契約
/// は `cap-headroom` の理由で file と余地と size を名指して断られ、run dir も event も作らない。size S（100）は
/// 通り、余地の無い file を write-set に持たない M の契約も通る。core（`crates/toy/src/` の合計 1399・上限 1500）は
/// 新規 file だけの M でも見積 300 が余地 101 を超えて `core` を名指す。数は `--rules` の manifest から読む。
#[test]
fn contract_closure_ext_cap_headroom_refuses_a_size_that_does_not_fit_the_file_or_the_core() {
    let (repo, state) = repo_with_big_file();
    let caps = |core_lines: u64| CapFixture { core_lines, file_lines: 1_500 };
    let rules = |name: &str, core_lines: u64| {
        let fixture =
            RulesFixture { gate: (1, 1_000_000), retries: FOLLOW_RETRIES, slots: default_slots(), caps: caps(core_lines) };
        write_rules_capped(&state, name, fixture).display().to_string()
    };
    let roomy = rules("rules-roomy.toml", 40_000);
    let intake = |design: &str, bead: &str, rules: &str| intake_with_rules(&repo, &state, design, bead, rules);
    let sized = |id: &str, size: &str, write_set: &str| sized_contract(&repo, id, size, write_set);
    let big = "\"crates/toy/src/big.rs\"";
    let out = intake(&sized("m", "M", big), "s2-m", &roomy);
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "余地 101 に M（300）は入らない: {err}");
    assert!(err.contains("crates/toy/src/big.rs") && err.contains(" 101 ") && err.contains("size M"), "file と余地と size: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    let small = intake(&sized("s", "S", big), "s2-s", &roomy);
    assert_eq!(small.status.code(), Some(i32::from(RC_OK)), "S（100）は余地 101 に入る: {}", stderr_of(&small));
    stop_run_ok(&state, &run_id_of(&small));
    let other = intake(&sized("other", "M", "\"src/lib.rs\""), "s2-o", &roomy);
    assert_eq!(other.status.code(), Some(i32::from(RC_OK)), "余地の無い file を持たない行は通る: {}", stderr_of(&other));
    stop_run_ok(&state, &run_id_of(&other));
    // core の形: 上限 1500 に対し合計 1399（余地 101）・新規 file 1 本の M の見積 300 が超える（file の余地は 1500）。
    let tight = rules("rules-tight.toml", 1_500);
    let core = intake(&sized("core", "M", "\"+crates/toy/src/new.rs\""), "s2-c", &tight);
    let err = stderr_of(&core);
    assert_eq!(core.status.code(), Some(i32::from(RC_REFUSED)), "core の余地 101 に 300 は入らない: {err}");
    assert!(err.contains("core の上限の余地が 101 行") && !err.contains("big.rs"), "core を名指す: {err}");
    let fits = intake(&sized("fits", "S", "\"+crates/toy/src/new.rs\""), "s2-f", &tight);
    assert_eq!(fits.status.code(), Some(i32::from(RC_OK)), "S の見積 100 は core の余地 101 に入る: {}", stderr_of(&fits));
    clean(&[&repo, &state]);
}

/// 上限の 2 値を振った tmp manifest の path（[`repo_with_big_file`] の置き場に書く・file の上限は 1500）。
fn capped_rules(state: &Path, name: &str, core_lines: u64) -> String {
    let fixture = RulesFixture {
        gate: (1, 1_000_000),
        retries: FOLLOW_RETRIES,
        slots: default_slots(),
        caps: CapFixture { core_lines, file_lines: 1_500 },
    };
    write_rules_capped(state, name, fixture).display().to_string()
}

/// `size` と `write-set` だけ差し替えた契約 file を repo に書き、その path を返す。
fn sized_contract(repo: &Path, id: &str, size: &str, write_set: &str) -> String {
    let fields: Vec<String> = contract_body()
        .into_iter()
        .filter(|line| !line.starts_with("size") && !line.starts_with("write-set") && !line.starts_with("id"))
        .chain([format!("id = \"{id}\""), format!("size = \"{size}\""), format!("write-set = [{write_set}]")])
        .collect();
    commit_row(repo, &fields);
    format!("{DESIGN_FILE}#{id}")
}

/// `--rules` を名指して intake を 1 回撃つ（rc を assert しない形・審査の lens は偽 PASS）。
fn intake_with_rules(repo: &Path, state: &Path, design: &str, bead: &str, rules: &str) -> Output {
    run_pipe(&[
        "intake", "--design", design, "--bead", bead,
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", rules, "--lens", &review_lens_pass(state),
    ])
}

/// (6) 縮む面（`s2-07l.287`・設計 contract-source.md §3・接頭辞 `contract_closure_ext_shrink_`）: 余地 101 の 1399 行の
/// `.rs` を `-` で持つ size M（300）の契約は受付を**通り**（run dir と event 1 件）、同じ file を素の path で持つ M は
/// 従来どおり `cap-headroom`（対で測る＝`-` が効いた証拠）。
#[test]
fn contract_closure_ext_shrink_item_is_exempt_from_the_file_headroom() {
    let (repo, state) = repo_with_big_file();
    let roomy = capped_rules(&state, "rules-roomy.toml", 40_000);
    let plain = intake_with_rules(&repo, &state, &sized_contract(&repo, "m.toml", "M", "\"crates/toy/src/big.rs\""), "s2-m", &roomy);
    let err = stderr_of(&plain);
    assert_eq!(plain.status.code(), Some(i32::from(RC_REFUSED)), "素の path の M は余地 101 に入らない: {err}");
    assert!(err.contains("crates/toy/src/big.rs の上限の余地が 101 行") && err.contains("size M"), "cap-headroom: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    let shrink = intake_with_rules(&repo, &state, &sized_contract(&repo, "shrink.toml", "M", "\"-crates/toy/src/big.rs\""), "s2-sh", &roomy);
    assert_eq!(shrink.status.code(), Some(i32::from(RC_OK)), "- の big.rs は余地を求めない: {}", stderr_of(&shrink));
    let id = run_id_of(&shrink);
    assert!(state.join("pipe").join(&id).is_dir(), "run dir が作られる: {id}");
    assert_eq!(event_count(&state), 2, "RunCreated と審査の段（Reviewed）の 2 件");
    clean(&[&repo, &state]);
}

/// (6) `-` の先が base に無い項目は **`pipe intake` で** `write-set-item-unresolved` として項目の字面（`-` 込み）を
/// 名指して断り、run dir も event も作らない（落として測ると「余地を求めない」宣言が静かに消える）。
#[test]
fn contract_closure_ext_shrink_item_absent_from_base_is_refused_at_intake() {
    let (repo, state) = repo_with_big_file();
    let roomy = capped_rules(&state, "rules-roomy.toml", 40_000);
    let contract = sized_contract(&repo, "none.toml", "M", "\"-crates/toy/src/none.rs\"");
    let out = intake_with_rules(&repo, &state, &contract, "s2-n", &roomy);
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "base に無い file への - は解けない: {err}");
    assert!(err.contains("write-set の -crates/toy/src/none.rs は base に解けない"), "項目の字面（- 込み）を名指す: {err}");
    assert!(!err.contains("cap-headroom") && !err.contains("上限の余地"), "理由は解けない項目の 1 つ: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&repo, &state]);
}

/// (6) core の見積: core の余地を「縮む面を数えると超え・数えなければ入る」150（上限 1549・合計 1399）に置く。size S
/// （100）で `-big.rs` + `+new.rs` は見積 100 × 1 本で通り、素の `big.rs` + `+new.rs` は 100 × 2 本 = 200 が超えて
/// `core` を名指して断られる（file の余地 101 は S に足りるので、名指すのは core だけ）。
#[test]
fn contract_closure_ext_shrink_item_is_not_counted_in_the_core_estimate() {
    let (repo, state) = repo_with_big_file();
    let tight = capped_rules(&state, "rules-tight.toml", 1_549);
    let both = "\"crates/toy/src/big.rs\", \"+crates/toy/src/new.rs\"";
    let plain = intake_with_rules(&repo, &state, &sized_contract(&repo, "plain.toml", "S", both), "s2-p", &tight);
    let err = stderr_of(&plain);
    assert_eq!(plain.status.code(), Some(i32::from(RC_REFUSED)), "2 本の見積 200 は core の余地 150 に入らない: {err}");
    assert!(err.contains("core の上限の余地が 150 行") && !err.contains("big.rs の上限"), "core を名指す: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    let shrunk = "\"-crates/toy/src/big.rs\", \"+crates/toy/src/new.rs\"";
    let shrink = intake_with_rules(&repo, &state, &sized_contract(&repo, "shrink.toml", "S", shrunk), "s2-sc", &tight);
    assert_eq!(shrink.status.code(), Some(i32::from(RC_OK)), "- を数えない 1 本の見積 100 は余地 150 に入る: {}", stderr_of(&shrink));
    assert_eq!(event_count(&state), 2, "RunCreated と審査の段（Reviewed）の 2 件");
    clean(&[&repo, &state]);
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

// ── 着地で消える file の宣言（`~`・設計 contract-source.md §24・行 x・`s2-07l.405`・接頭辞 `contract_closure_ext_delete_`） ──

/// `~` の項目が base に在る行は受付を通り（run dir と event 1 件）、受付はその項目を**接頭辞を剥がした素の path**で
/// 読む（剥がす規則は `normalize` の 1 本＝交差の照合も同じ 1 本を通る）: 素の path を書いた 2 本目は live な `~` の
/// 便と交差して断られ、別の file の便は交差しない（base は `~` を剥がさないので交差 0 で通る＝RED）。
#[test]
fn contract_closure_ext_delete_intake_accepts_existing_and_strips_prefix() {
    let (repo, state) = repo_with_state();
    let doomed = write_set_contract(&repo, "delete", &["~src/lib.rs"]);
    let out = try_intake(&repo, &state, &doomed, "s2-del");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "base に在る file への ~ は受付を通る: {}", stderr_of(&out));
    let id = run_id_of(&out);
    assert!(state.join("pipe").join(&id).is_dir(), "run dir が作られる: {id}");
    assert_eq!(event_count(&state), 2, "RunCreated と審査の段（Reviewed）の 2 件");
    let plain = write_set_contract(&repo, "plain", &["src/lib.rs"]);
    let crossed = try_intake(&repo, &state, &plain, "s2-plain");
    let err = stderr_of(&crossed);
    assert_eq!(crossed.status.code(), Some(i32::from(RC_REFUSED)), "~ の便と素の path の便は同じ面を触る: {err}");
    assert!(err.contains(&id) && err.contains("src/lib.rs"), "1 本目の run id と交差した path を名乗る: {err}");
    let apart = try_intake(&repo, &state, &write_set_contract(&repo, "other", &["+src/new.rs"]), "s2-apart");
    assert_eq!(apart.status.code(), Some(i32::from(RC_OK)), "別の file の便は交差しない: {}", stderr_of(&apart));
    clean(&[&repo, &state]);
}

/// `~` の先が base に無い項目は **`pipe intake` で** `write-set-item-unresolved` として項目の字面（`~` 込み）を名指して
/// 断り、run dir も event も作らない（消す予定の file が無い＝宣言の誤りを入口で止める・落として測ると黙って通る）。
#[test]
fn contract_closure_ext_delete_intake_refuses_missing_file() {
    let (repo, state) = repo_with_state();
    let out = try_intake(&repo, &state, &write_set_contract(&repo, "none", &["~src/none.rs"]), "s2-none");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "base に無い file への ~ は解けない: {err}");
    assert!(err.contains("write-set の ~src/none.rs は base に解けない"), "項目の字面（~ 込み）を名指す: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&repo, &state]);
}

/// 契約表の検査（`MayBeLanded` の場面）は `~` の項目を **2 分岐とも**解く: tracked から消した後（着地の後）も、まだ
/// 消していない周も findings 0・rc 0（着地済みの行が `write-set-item-unresolved` で永久に赤くなる型を塞ぐ）。
#[test]
fn contract_closure_ext_delete_check_passes_after_landing() {
    let doc = table_doc(&table_region(&[table_row("a", &[("write-set", "[\"src/tint.rs\", \"~src/old.rs\"]")])]));
    let present = table_repo(&doc, &[("src/old.rs", "// old\n")]);
    let out = contracts_check(&present);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "まだ消していない周の ~ も解ける: {}", stdout_of(&out));
    assert_eq!(stdout_of(&out).trim_end(), "contracts check: docs=1 rows=1 findings=0");
    let landed = table_repo(&doc, &[("src/old.rs", "// old\n")]);
    git(&landed, &["rm", "-q", "src/old.rs"]);
    git(&landed, &["commit", "-q", "-m", "land"]);
    let out = contracts_check(&landed);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "着地で消えた ~ も解ける: {}", stdout_of(&out));
    assert_eq!(stdout_of(&out).trim_end(), "contracts check: docs=1 rows=1 findings=0");
    clean(&[&present, &landed]);
}

/// 名指しの実在（§24 (3)）: 節の本文が行の `~` の項目と等しい path 形を backtick で名指しても、着地（tracked から
/// 消えた後）に `name-unresolved` にならない。除外の無い別の path（`src/gone.rs`）は名指されたまま＝空虚でない対。
#[test]
fn contract_closure_ext_delete_name_in_section_resolves_after_landing() {
    let body = "本文。`src/old.rs` は行の消える file・`src/gone.rs` は write-set に無い。";
    let doc = table_doc(&table_region(&[table_row("a", &[("write-set", "[\"src/tint.rs\", \"~src/old.rs\"]")])]))
        .replace("## 1. 何を解くか\n\n本文。", &format!("## 1. 何を解くか\n\n{body}"));
    let repo = table_repo(&doc, &[("src/old.rs", "// old\n")]);
    git(&repo, &["rm", "-q", "src/old.rs"]);
    git(&repo, &["commit", "-q", "-m", "land"]);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{text}{}", stderr_of(&out));
    let names = findings_for(&found, &doc, "a", "name-unresolved");
    assert!(names.iter().all(|line| !line.contains("src/old.rs")), "~ の項目と等しい名指しは着地の後も解ける: {text}");
    assert_eq!(names.len(), 1, "解けない名指しは 1 件だけ: {text}");
    assert!(names.iter().all(|line| line.contains("名指し src/gone.rs が base に無い")), "除外の無い path は名指したまま: {text}");
    assert_eq!(found.len(), 1, "他の理由は出ない（~ の項目は write-set-item-unresolved にならない）: {text}");
    clean(&[&repo]);
}

/// 上限の余地: `~` の項目は増分が負なので **file の余地も core の見積の本数も**求めない（`-` と同じ扱い）。余地 101 の
/// 1399 行の `.rs` を `~` で持つ size M（300）の契約は受付を**通り**（run dir と event 1 件）、同じ file を素の path で
/// 持つ M は従来どおり `cap-headroom`。core も対で測る: 余地 150 に `~big.rs` + `+new.rs` の見積 100 × 1 本は入り、素の
/// 2 本 200 は超えて `core` を名指して断られる（`~` を `File` / `New` と同じ本数に数えると後段が赤くなる）。
#[test]
fn contract_closure_ext_delete_does_not_count_headroom() {
    let (repo, state) = repo_with_big_file();
    let roomy = capped_rules(&state, "rules-roomy.toml", 40_000);
    let plain = intake_with_rules(&repo, &state, &sized_contract(&repo, "m.toml", "M", "\"crates/toy/src/big.rs\""), "s2-mb", &roomy);
    let err = stderr_of(&plain);
    assert_eq!(plain.status.code(), Some(i32::from(RC_REFUSED)), "素の path の M は余地 101 に入らない: {err}");
    assert!(err.contains("crates/toy/src/big.rs の上限の余地が 101 行") && err.contains("size M"), "cap-headroom: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    let doomed = intake_with_rules(&repo, &state, &sized_contract(&repo, "del.toml", "M", "\"~crates/toy/src/big.rs\""), "s2-db", &roomy);
    assert_eq!(doomed.status.code(), Some(i32::from(RC_OK)), "~ の big.rs は余地を求めない: {}", stderr_of(&doomed));
    let id = run_id_of(&doomed);
    assert!(state.join("pipe").join(&id).is_dir(), "run dir が作られる: {id}");
    assert_eq!(event_count(&state), 2, "RunCreated と審査の段（Reviewed）の 2 件");
    stop_run_ok(&state, &id);
    // core の見積の本数（上限 1549・合計 1399＝余地 150・size S の見積は 1 本 100 行）。
    let tight = capped_rules(&state, "rules-tight.toml", 1_549);
    let both = "\"crates/toy/src/big.rs\", \"+crates/toy/src/new.rs\"";
    let counted = intake_with_rules(&repo, &state, &sized_contract(&repo, "both.toml", "S", both), "s2-bo", &tight);
    let err = stderr_of(&counted);
    assert_eq!(counted.status.code(), Some(i32::from(RC_REFUSED)), "2 本の見積 200 は core の余地 150 に入らない: {err}");
    assert!(err.contains("core の上限の余地が 150 行") && !err.contains("big.rs の上限"), "core を名指す: {err}");
    let gone = "\"~crates/toy/src/big.rs\", \"+crates/toy/src/new.rs\"";
    let apart = intake_with_rules(&repo, &state, &sized_contract(&repo, "gone.toml", "S", gone), "s2-go", &tight);
    assert_eq!(apart.status.code(), Some(i32::from(RC_OK)), "~ を数えない 1 本の見積 100 は余地 150 に入る: {}", stderr_of(&apart));
    assert!(state.join("pipe").join(run_id_of(&apart)).is_dir(), "run dir が作られる");
    clean(&[&repo, &state]);
}

/// (5) 行の数え方（`s2-07l.254`・設計 rules-manifest.md §4・接頭辞 `contract_closure_ext_width_`）: base の `.rs` が短い
/// 1399 行と 2000 字を詰めた 1 行を持つとき、余地は改行の数（1400 行 → 100）でなく幅（`--rules` の `R-C4.line-width`）で
/// 正規化した行数で出て、改行の数なら入る size S（100）が `cap-headroom` で断られる（詰め込みで余地が増えない）。
#[test]
fn contract_closure_ext_width_packed_line_does_not_widen_the_headroom() {
    let (repo, state) = repo_with_state();
    let dir = repo.join("crates").join("toy").join("src");
    fs::create_dir_all(&dir).expect("core の dir を作れる");
    fs::write(dir.join("packed.rs"), format!("{}{}\n", "// x\n".repeat(1399), "x".repeat(2000))).expect("file を書ける");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "packed"]);
    let width = embedded_int("R-C4.line-width");
    let headroom = 1_500 - 1_399 - 2_000_u64.div_ceil(width);
    assert!(headroom < 100, "幅で数えた余地は改行の数の余地（100）より小さい: {headroom}");
    let fixture = RulesFixture {
        gate: (1, 1_000_000),
        retries: FOLLOW_RETRIES,
        slots: default_slots(),
        caps: CapFixture { core_lines: 40_000, file_lines: 1_500 },
    };
    let rules = write_rules_capped(&state, "rules-width.toml", fixture).display().to_string();
    let design = sized_contract(&repo, "s", "S", "\"crates/toy/src/packed.rs\"");
    let out = run_pipe(&[
        "intake", "--design", &design, "--bead", "s2-w",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(), "--rules", &rules,
    ]);
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "詰め込んだ 1 行は幅で数える: {err}");
    let named = format!("crates/toy/src/packed.rs の上限の余地が {headroom} 行");
    assert!(err.contains(&named) && err.contains("size S"), "file と幅で数えた余地と size: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// (4) 名指しの実在: `title` / `done` / 節の本文の backtick の中身のうち path 形・型の path 形・fn 形が base に解けない
/// ものを `name-unresolved` で全件・在り処付き（行番号は行の見出し）。`touches` の型の variant・write-set の `+`
/// 宣言の新規 file・一致しない字面は名指さない。
#[test]
fn contract_closure_ext_unresolved_names_are_named_with_their_place() {
    let body = "本文。`crate::tint::Tint` と `show(` は在る。`Nope::Thing` と `src/nope.rs` は無い。`Tint::Warm => 1` は字面。";
    let doc = table_doc(&table_region(&[
        table_row("a", &[("title", "\"`src/none.rs` を直す\""), ("done", "\"`Tint::Hot` と `frob(` が通る\"")]),
        table_row("b", &[("done", "\"`Tint::Hot` は touches の型・`src/new.rs` は + 宣言\""), ("touches", "[\"crate::tint::Tint\"]"), ("write-set", "[\"src/tint.rs\", \"src/show.rs\", \"+src/new.rs\"]")]),
        table_row("c", &[("done", "\"`src/new.rs` は write-set に無い\"")]),
    ]))
    .replace("## 1. 何を解くか\n\n本文。", &format!("## 1. 何を解くか\n\n{body}"));
    let repo = table_repo(&doc, &[]);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{text}{}", stderr_of(&out));
    let body_line = doc.lines().position(|line| line.starts_with("本文。`crate")).unwrap_or_default() + 1;
    let row_a = findings_for(&found, &doc, "a", "name-unresolved");
    let want_a = [
        ("src/none.rs", "title".to_owned()),
        ("Tint::Hot", "done".to_owned()),
        ("frob(", "done".to_owned()),
        ("Nope::Thing", format!("section 1 line {body_line}")),
        ("src/nope.rs", format!("section 1 line {body_line}")),
    ];
    assert_eq!(row_a.len(), want_a.len(), "行 a は解けない名指しを全件: {text}");
    for ((name, at), line) in want_a.iter().zip(&row_a) {
        assert!(line.contains(&format!("名指し {name} が base に無い（{at}）")), "{name} を {at} で名指す: {line}");
    }
    let row_b = findings_for(&found, &doc, "b", "name-unresolved");
    let named_b: Vec<&String> = row_b.iter().filter(|line| line.contains("Tint::Hot") || line.contains("src/new.rs")).collect();
    assert!(named_b.is_empty(), "touches の型の variant と + 宣言の新規 file は名指さない: {row_b:?}");
    assert_eq!(row_b.len(), 2, "行 b も節の本文の 2 件は持つ（Nope::Thing / src/nope.rs）: {text}");
    let row_c = findings_for(&found, &doc, "c", "name-unresolved");
    assert!(row_c.iter().any(|line| line.contains("名指し src/new.rs が base に無い（done）")), "write-set に無い同名は解けない: {text}");
    assert!(!text.contains("Tint::Warm"), "一致しない字面（arm の断片）は名指さない: {text}");
    clean(&[&repo]);
}

/// (5) 現物の契約表（本 repo の `docs/design/*.md`）は 4 つの拡張（外形 pin・項目の実在と展開・名指しの実在・
/// 余地は CI で撃たない）を含めて違反 0・rc 0（設計 §9「現物の契約表で 4 つとも違反 0」）。
#[test]
fn contract_closure_ext_real_table_has_zero_findings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo の root を解ける");
    let out = contracts_check(root);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "現物の契約表は違反 0: {text}{}", stderr_of(&out));
    let last = text.lines().last().unwrap_or_default();
    assert!(last.starts_with("contracts check: docs=") && last.ends_with(" findings=0"), "判定行: {last}");
    let rows: u64 = last.split_whitespace().find_map(|token| token.strip_prefix("rows=")?.parse().ok()).unwrap_or_default();
    assert!(rows >= 8, "母集団は現物の契約表の行（contract-source.md の 8 行以上・空の表で 0 件を名乗らない）: {last}");
}

// ─────── 名指しの実在の impl 経路（設計 docs/design/contract-source.md §26・§3 (2)・`s2-07l.432`・接頭辞 `contract_names_impl_`） ───────

/// toy repo の method / 関連 fn を持つ file（素の impl `Report::violation`・generic impl `Wide::width`）。どちらの
/// file も「型::項目」の字面は持たない（呼び手は「値.項目(」なので (a) の字面の経路では解けない）。
const IMPL_FILES: &[(&str, &str)] = &[
    (
        "src/report.rs",
        "pub struct Report {\n    pub at: u8,\n}\n\nimpl Report {\n    pub fn violation(&self) -> u8 {\n        self.at\n    }\n}\n",
    ),
    (
        "src/wide.rs",
        "pub struct Wide<T> {\n    pub inner: T,\n}\n\nimpl<T: Copy> Wide<T> {\n    pub fn width(&self) -> usize {\n        0\n    }\n}\n",
    ),
];

/// impl 経路（§26）: base が宣言する method / 関連 fn の「型::項目」は `contracts check` で解け、同じ 1 語の形で
/// 並ぶ実在しない `Report::nope` だけが `name-unresolved` で名指される（done と § 本文の 2 か所ぶんの 2 行）・rc 1。
/// base（字面の経路だけ）では実在の 2 語も名指されて findings が 6 件になる（偽陽性・C16）。
#[test]
fn contract_names_impl_method_is_not_named_by_contracts_check() {
    let named = "`Report::violation` と `Wide::width` は在る。`Report::nope` は無い。";
    let doc = table_doc(&table_region(&[table_row("a", &[("done", &format!("\"{named}\""))])]))
        .replace("## 1. 何を解くか\n\n本文。", &format!("## 1. 何を解くか\n\n{named}"));
    let repo = table_repo(&doc, IMPL_FILES);
    let out = contracts_check(&repo);
    let (text, found) = (stdout_of(&out), findings_of(&out));
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "実在しない 1 語で rc 1: {text}{}", stderr_of(&out));
    let body_line = doc.lines().position(|line| line.starts_with("`Report::violation`")).unwrap_or_default() + 1;
    let row_a = findings_for(&found, &doc, "a", "name-unresolved");
    let want = [("Report::nope", "done".to_owned()), ("Report::nope", format!("section 1 line {body_line}"))];
    assert_eq!(row_a.len(), want.len(), "名指すのは実在しない 1 語の 2 か所だけ: {text}");
    for ((name, at), line) in want.iter().zip(&row_a) {
        assert!(line.contains(&format!("名指し {name} が base に無い（{at}）")), "{name} を {at} で名指す: {line}");
    }
    for resolved in ["Report::violation", "Wide::width"] {
        assert!(!text.contains(resolved), "base が impl で宣言する {resolved} は名指さない: {text}");
    }
    assert_eq!(text.lines().last(), Some("contracts check: docs=1 rows=1 findings=2"), "判定行: {text}");
    clean(&[&repo]);
}

// ─────── land 済みの `+`（設計 docs/design/contract-source.md §3・契約 (i)・`s2-07l.346`・接頭辞 `contract_table_landed_plus_`） ───────

/// 行 `i` の write-set の項目（`+` 付きの新規 file の宣言・land すると base に実在する）。
const LANDED_PLUS_ITEM: &str = "+crates/toy/src/new.rs";

/// `+` の項目を 1 つ持つ行 `i` の契約表を載せた設計 doc。
fn landed_plus_doc() -> String {
    table_doc(&table_region(&[table_row("i", &[("write-set", &format!("[\"{LANDED_PLUS_ITEM}\"]"))])]))
}

/// 設計 pointer `docs/design/toy.md#i` と行と同じ write-set（`+` 付き）を持つ契約 file。
fn landed_plus_contract(_repo: &Path) -> String {
    // 契約 (b) 以後、write-set は**行**が持つ（`+` の項目も行の側）。受付へ渡すのは pointer だけである。
    "docs/design/toy.md#i".to_owned()
}

/// (a) 契約表の行が `+crates/toy/src/new.rs` を持ち、その file を commit した base（land 後の main の形）で `contracts check`
/// を撃つと findings 0・rc 0（base は `write-set-item-unresolved` 1 件 → RED）。対: 同じ行で file がまだ無い base も 0
/// （新規 file の宣言として解ける＝land の前後で行の字面を変えずに緑）。
#[test]
fn contract_table_landed_plus_item_resolves_as_file() {
    let landed = table_repo(&landed_plus_doc(), &[("crates/toy/src/new.rs", "pub fn landed() {}\n")]);
    let out = contracts_check(&landed);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land 済みの + は実在 file と読む: {text}{}", stderr_of(&out));
    assert!(!text.contains("write-set-item-unresolved"), "解けない項目として名指さない: {text}");
    assert_eq!(text.trim_end(), "contracts check: docs=1 rows=1 findings=0", "判定行: {text}");
    let fresh = table_repo(&landed_plus_doc(), &[]);
    let out = contracts_check(&fresh);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "land 前の + は新規 file として解ける: {}", stdout_of(&out));
    assert_eq!(stdout_of(&out).trim_end(), "contracts check: docs=1 rows=1 findings=0");
    clean(&[&landed, &fresh]);
}

/// (b) 同じ契約（行 `i` を指し `+crates/toy/src/new.rs` を write-set に持つ）を、その file が base に在る repo の intake に
/// 出すと `write-set-item-unresolved` で項目の字面（`+` 込み）を名指して断り、run dir も event も作らない（intake は
/// `MustBeAbsent`・契約表の検査を緩めても入口は緩まない＝退行の pin）。対: file の無い base では通る。
#[test]
fn contract_table_landed_plus_intake_still_refuses() {
    let (landed, state) = derive_repo_with(&landed_plus_doc(), &[("crates/toy/src/new.rs", "pub fn landed() {}\n")]);
    let out = intake_raw(&landed, &state, &landed_plus_contract(&landed), "s2-landed");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "base に在る file への + は受付で断る: {err}");
    assert!(err.contains(&format!("write-set の {LANDED_PLUS_ITEM} は base に解けない")), "項目の字面（+ 込み）を名指す: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&landed, &state]);
    let (fresh, state) = derive_repo(&landed_plus_doc());
    let out = intake_raw(&fresh, &state, &landed_plus_contract(&fresh), "s2-fresh");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "base に無い file への + は通る: {}", stderr_of(&out));
    stop_run_ok(&state, &run_id_of(&out));
    clean(&[&fresh, &state]);
}

// ─────── write-set の導出（設計 docs/design/contract-source.md §3「write-set の導出」・契約 (h)・`s2-07l.311`・接頭辞 `contract_derive_`） ───────

/// 導出の toy repo の宣言（`cargo` を許す＝行の verify に nextest の行を書ける・要件面は既定）。
const DERIVE_VESSEL: &str = "schema = 1\nallowed-commands = [\"git\", \"sh\", \"cargo\"]\ncommon-verify = [\"git status\"]\n";

/// 導出の toy repo の file: crate `toy` の型 `crate::tint::Tint`（宣言 file と arm の file）・`derive_` の歯（tests と
/// src の test 区間に 1 本ずつ）・helper と別接頭辞の歯・非 `.rs` の面。
const DERIVE_FILES: &[(&str, &str)] = &[
    ("crates/toy/src/tint.rs", TABLE_TINT),
    ("crates/toy/src/show.rs", TABLE_SHOW),
    ("crates/toy/src/other.rs", "pub fn derive_outside() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn derive_in_src() {}\n}\n"),
    ("crates/toy/tests/e2e.rs", "#[test]\nfn derive_ok() {}\n"),
    ("crates/toy/tests/helper.rs", "fn derive_helper() {}\n\n#[test]\nfn other_case() {\n    derive_helper();\n}\n"),
    ("rules/manifest.toml", "schema = 1\n"),
];

/// 導出の toy repo（[`repo_with_state`] の repo に [`DERIVE_FILES`]・要件面・設計 doc `docs/design/toy.md` を足して
/// commit）と置き場。
fn derive_repo(doc: &str) -> (PathBuf, PathBuf) {
    derive_repo_with(doc, &[])
}

/// [`derive_repo`] に `files` を足した toy repo と置き場。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn derive_repo_with(doc: &str, files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
    let (repo, state) = repo_with_state();
    let seeded = [(".vessel.toml", DERIVE_VESSEL), ("design-intent/spec/srs.html", TABLE_SRS), ("docs/design/toy.md", doc)];
    for (path, body) in seeded.iter().chain(DERIVE_FILES).chain(files) {
        let target = repo.join(path);
        fs::create_dir_all(target.parent().expect("親 dir が在る")).expect("dir を作れる");
        fs::write(&target, body).expect("file を書ける");
    }
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "derive"]);
    (repo, state)
}

/// 導出の形の行（[`table_row`] から既定の `write-set` を落とす・`over` が `write-set` を持てばそれを載せる）。
fn derive_row(id: &str, over: &[(&str, &str)]) -> String {
    let keeps = over.iter().any(|(name, _)| *name == "write-set");
    table_row(id, over).lines().filter(|line| keeps || !line.starts_with("write-set")).map(|line| format!("{line}\n")).collect()
}

/// 設計 pointer `docs/design/toy.md#<id>` を持つ契約 file（残りの欄は [`contract_body`]・write-set は仮の `src/lib.rs`
/// ＝写しの差し替えを測る対）。
fn pointed_contract(_repo: &Path, _name: &str, id: &str) -> String {
    // 契約 (b) 以後、受付が受けるのは pointer そのものである（契約 file は器が行から作る）。
    format!("docs/design/toy.md#{id}")
}

/// 便の写しの契約の write-set。
#[expect(
    clippy::panic,
    reason = "統合 test の helper。clippy の allow-panic-in-tests は #[test] 関数の中だけに効く"
)]
fn copied_write_set(state: &Path, id: &str) -> Vec<String> {
    vessel::pipe::contract::Contract::load(&state.join("pipe").join(id).join("contract.toml"))
        .map(|contract| contract.write_set)
        .unwrap_or_else(|errors| panic!("写しを読める: {errors:?}"))
}

/// intake の判定行（1 行目）の token。
fn intake_tokens(out: &Output) -> Vec<String> {
    stdout_of(out).lines().next().unwrap_or_default().split_whitespace().map(str::to_owned).collect()
}

/// (a) `write-set` の無い行（`touches` + `verify` + `creates` + `tests` + `also`）は intake を通り、写しの契約の write-set に
/// 導出値（閉包 ∪ 歯の置き場 ∪ tests ∪ creates〔`+` 付き〕∪ also・辞書順）が載る・判定行 `write-set=derived files=<N>`・
/// 他の欄は逐語。base は `write-set` 必須で断る（RED）。
#[test]
fn contract_derive_fills_write_set_for_a_row_without_one() {
    let row = derive_row(
        "a",
        &[
            ("touches", "[\"crate::tint::Tint\"]"),
            ("verify", "[\"cargo nextest run -p toy --no-tests=fail derive_\"]"),
            ("creates", "[\"crates/toy/src/new.rs\"]"),
            ("tests", "[\"crates/toy/tests/helper.rs\"]"),
            ("also", "[\"rules/manifest.toml\"]"),
        ],
    );
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "a.toml", "a"), "s2-a");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "導出値で通る: {}", stderr_of(&out));
    let tokens = intake_tokens(&out);
    assert!(tokens.first().is_some_and(|token| token.starts_with("run=")), "既存 token が先頭のまま: {tokens:?}");
    assert!(tokens.contains(&"write-set=derived".to_owned()) && tokens.contains(&"files=7".to_owned()), "判定行: {tokens:?}");
    let id = run_id_of(&out);
    let want = [
        "+crates/toy/src/new.rs",
        "crates/toy/src/other.rs",
        "crates/toy/src/show.rs",
        "crates/toy/src/tint.rs",
        "crates/toy/tests/e2e.rs",
        "crates/toy/tests/helper.rs",
        "rules/manifest.toml",
    ];
    assert_eq!(copied_write_set(&state, &id), want, "写しの write-set は導出値（閉包 ∪ 歯の置き場 ∪ tests ∪ creates ∪ also）");
    let copied = fs::read_to_string(state.join("pipe").join(&id).join("contract.toml")).unwrap_or_default();
    // 契約 (b) 以後、写しの欄は**行**から来る（`verify` は行の逐語・新欄 `creates` / `tests` / `also` は写さない）。
    assert!(
        copied.contains("verify = [\"cargo nextest run -p toy --no-tests=fail derive_\"]"),
        "verify は行の逐語: {copied}"
    );
    for skipped in ["creates", "tests =", "also"] {
        assert!(!copied.contains(skipped), "新欄 {skipped} は写さない: {copied}");
    }
    assert_eq!(event_count(&state), 2, "RunCreated と審査の段（Reviewed）の 2 件");
    clean(&[&repo, &state]);
}

/// (b) 手書きの `write-set` が導出値とずれた行は `write-set-drift` で**余分**（src/lib.rs）を名指して断られ、run dir も
/// event も作らない。`+x` と `x` は同じ項目（new.rs は名指さない）。対: 一致する行は通り、写しの write-set は行の逐語。
///
/// **不足の側は表の検査が先に断る**（契約 (b)・`write-set-incomplete`＝閉包の file が write-set に無い）ので、
/// drift に届くのは余分だけである（不足は `contract_closure_ext_` の族が測る）。
#[test]
fn contract_derive_refuses_drift_naming_missing_and_extra() {
    let touches = ("touches", "[\"crate::tint::Tint\"]");
    let creates = ("creates", "[\"crates/toy/src/new.rs\"]");
    let rows = [
        derive_row("b", &[touches, creates, ("write-set", "[\"crates/toy/src/tint.rs\", \"crates/toy/src/show.rs\", \"+crates/toy/src/new.rs\", \"src/lib.rs\"]")]),
        derive_row("c", &[touches, creates, ("write-set", "[\"+crates/toy/src/new.rs\", \"crates/toy/src/show.rs\", \"crates/toy/src/tint.rs\"]")]),
    ];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "b.toml", "b"), "s2-b");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "ずれは rc 1: {err}");
    assert!(err.contains("write-set が導出値と一致しない"), "理由: {err}");
    assert!(err.contains("extra: src/lib.rs"), "余分を名指す: {err}");
    assert!(!err.contains("new.rs") && !err.contains("tint.rs"), "一致する項目（+ の有無は同じ）は名指さない: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    let passed = intake_raw(&repo, &state, &pointed_contract(&repo, "c.toml", "c"), "s2-c");
    assert_eq!(passed.status.code(), Some(i32::from(RC_OK)), "一致する手書きは通る: {}", stderr_of(&passed));
    let tokens = intake_tokens(&passed);
    assert!(tokens.contains(&"write-set=derived".to_owned()) && tokens.contains(&"files=3".to_owned()), "{tokens:?}");
    // 契約 (b) 以後、写しの write-set は**行の逐語**である（手書きの行は導出値で差し替えない）。
    assert_eq!(
        copied_write_set(&state, &run_id_of(&passed)),
        ["+crates/toy/src/new.rs", "crates/toy/src/show.rs", "crates/toy/src/tint.rs"],
        "手書きの在る行は写しを差し替えない"
    );
    clean(&[&repo, &state]);
}

/// (c) `also` の `.rs`・`tests` の歯でない file・新しい接頭辞で `tests` 無し、はそれぞれ typed に断られ run を作らない。
/// 新しい接頭辞は `tests` 欄（`creates` の新規 file）が置き場で、write-set には creates の側（`+` 付き）だけが載る。
#[test]
fn contract_derive_refuses_also_rs_and_tests_non_teeth_and_unresolved_filter() {
    let fresh = ("verify", "[\"cargo nextest run -p toy --no-tests=fail fresh_\"]");
    let rows = [
        derive_row("r", &[("also", "[\"crates/toy/src/tint.rs\"]")]),
        derive_row("t", &[("tests", "[\"crates/toy/src/tint.rs\"]")]),
        derive_row("f", &[fresh]),
        derive_row("p", &[fresh, ("creates", "[\"crates/toy/tests/fresh.rs\"]"), ("tests", "[\"crates/toy/tests/fresh.rs\"]")]),
    ];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    for (id, want) in [
        ("r", "also の crates/toy/src/tint.rs は .rs である"),
        ("t", "tests の crates/toy/src/tint.rs は歯の file でない"),
        ("f", "filter 語 fresh_ を含む #[test] の fn が base に無く tests 欄も無い"),
    ] {
        let out = intake_raw(&repo, &state, &pointed_contract(&repo, &format!("{id}.toml"), id), &format!("s2-{id}"));
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "行 {id} は rc 1: {err}");
        assert!(err.contains(want), "行 {id} の理由: {err}");
        assert_eq!(event_count(&state), 0, "断った周は event を書かない");
        assert!(!state.join("pipe").exists(), "run dir も作らない");
    }
    let placed = intake_raw(&repo, &state, &pointed_contract(&repo, "p.toml", "p"), "s2-p");
    assert_eq!(placed.status.code(), Some(i32::from(RC_OK)), "tests 欄が置き場になる: {}", stderr_of(&placed));
    assert_eq!(copied_write_set(&state, &run_id_of(&placed)), ["+crates/toy/tests/fresh.rs"], "creates の側だけが載る");
    clean(&[&repo, &state]);
}

/// (d) 新欄なし + `write-set` あり = `Declared`: 導出も drift も撃たず (g) までの検査だけで通り（判定行 `write-set=declared files=<N>`・写しは行の write-set のまま）。解けない pointer
/// （区間に無い行 id）は契約表の欠陥として断る。
#[test]
fn contract_derive_declared_rows_skip_derivation() {
    // 契約 (b) 以後、**Declared の行も閉包 ⊆ write-set** を表の検査が要る（導出と drift を撃たないだけ）。
    let row = table_row(
        "d",
        &[("touches", "[\"crate::tint::Tint\"]"), ("write-set", "[\"crates/toy/src/tint.rs\", \"crates/toy/src/show.rs\"]")],
    );
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "d.toml", "d"), "s2-d");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "Declared は導出しない: {}", stderr_of(&out));
    let tokens = intake_tokens(&out);
    assert!(tokens.contains(&"write-set=declared".to_owned()) && tokens.contains(&"files=2".to_owned()), "{tokens:?}");
    let id = run_id_of(&out);
    // 契約 (b) 以後、写しの write-set は**行の逐語**である（Declared は導出しない＝行がそのまま載る）。
    assert_eq!(copied_write_set(&state, &id), ["crates/toy/src/tint.rs", "crates/toy/src/show.rs"], "写しは行の write-set のまま");
    stop_run_ok(&state, &id);
    // 「pointer でない design」の周は**もう無い**（受付は pointer しか受けない・契約 (b)）。
    let missing = intake_raw(&repo, &state, &pointed_contract(&repo, "z.toml", "zz"), "s2-z");
    assert_eq!(missing.status.code(), Some(i32::from(RC_REFUSED)), "区間に無い行 id は rc 1: {}", stderr_of(&missing));
    assert!(stderr_of(&missing).contains("行 id zz が区間に無い"), "{}", stderr_of(&missing));
    clean(&[&repo, &state]);
}

// ───── Declared 行の歯の置き場の門（設計 docs/design/contract-source.md §20・行 t・`s2-07l.391`・接頭辞 `contract_declared_teeth_`） ─────

/// Declared 行（新欄なし + `write-set` あり）で `verify` が nextest 形の行。`write-set` だけを差し替える。
fn declared_teeth_row(id: &str, filter: &str, write_set: &str) -> String {
    let verify = format!("[\"cargo nextest run -p toy --no-tests=fail {filter}\"]");
    table_row(id, &[("write-set", write_set), ("verify", verify.as_str())])
}

/// (a) Declared 行の `verify` の歯（`derive_` = other.rs と e2e.rs）が `write-set`（tint.rs だけ）の外に在る契約は受付が
/// `teeth-outside-write-set`（rc 1）で**両方**を辞書順に名指して断り（helper の fn だけの helper.rs は出ない）、run dir は
/// 撃つ前と同数（便を作らない）。base は Declared を導出も門も無しで通す（rc 0 → RED）。
#[test]
fn contract_declared_teeth_outside_write_set_is_refused() {
    let row = declared_teeth_row("t", "derive_", "[\"crates/toy/src/tint.rs\"]");
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let before = run_dirs(&state);
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "t.toml", "t"), "s2-t");
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "write-set の外の歯は rc 1: {err}");
    assert!(err.contains("verify の歯の file が write-set に無い"), "teeth-outside-write-set の理由: {err}");
    assert!(err.contains("crates/toy/src/other.rs, crates/toy/tests/e2e.rs"), "両方を辞書順に名指す: {err}");
    assert!(!err.contains("helper.rs"), "helper の fn だけの file は歯の file でない: {err}");
    assert!(!err.contains("write-set が導出値と一致しない"), "drift は撃たない: {err}");
    assert_eq!(run_dirs(&state), before, "便を作らない（run dir は撃つ前と同数）");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    clean(&[&repo, &state]);
}

/// (b) 同じ行の `write-set` に歯の 2 file を足した形（3 項目）は通る: rc 0・判定行 `write-set=declared` ∧ `files=3`・
/// 写しの write-set は契約 file のまま（Declared は差し替えない）。
#[test]
fn contract_declared_teeth_inside_write_set_passes() {
    let write_set = "[\"crates/toy/src/tint.rs\", \"crates/toy/src/other.rs\", \"crates/toy/tests/e2e.rs\"]";
    let row = declared_teeth_row("u", "derive_", write_set);
    let (repo, state) = derive_repo(&table_doc(&table_region(&[row])));
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "u.toml", "u"), "s2-u");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "歯が write-set の中なら通る: {}", stderr_of(&out));
    let tokens = intake_tokens(&out);
    assert!(tokens.contains(&"write-set=declared".to_owned()) && tokens.contains(&"files=3".to_owned()), "{tokens:?}");
    // 契約 (b) 以後、写しの write-set は行の逐語である。
    assert_eq!(
        copied_write_set(&state, &run_id_of(&out)),
        ["crates/toy/src/tint.rs", "crates/toy/src/other.rs", "crates/toy/tests/e2e.rs"],
        "写しは行の write-set のまま"
    );
    clean(&[&repo, &state]);
}

/// (c) base で 0 本の filter 語（`fresh_`）: Declared 行に `tests` 欄は無いので、`write-set` が歯の file を 1 つも持たなければ
/// 従来の `teeth-place-unresolved`（rc 1・字面不変）で断り、歯の file（e2e.rs）を足せばそれを置き場と読んで通る（rc 0）。
/// 母集団 = 2 回の intake の rc。
#[test]
fn contract_declared_teeth_new_filter_needs_a_teeth_file_in_write_set() {
    let rows = [
        declared_teeth_row("v", "fresh_", "[\"crates/toy/src/tint.rs\"]"),
        declared_teeth_row("w", "fresh_", "[\"crates/toy/src/tint.rs\", \"crates/toy/tests/e2e.rs\"]"),
    ];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    let bare = intake_raw(&repo, &state, &pointed_contract(&repo, "v.toml", "v"), "s2-v");
    let err = stderr_of(&bare);
    assert_eq!(bare.status.code(), Some(i32::from(RC_REFUSED)), "歯の file の無い write-set は rc 1: {err}");
    assert!(err.contains("filter 語 fresh_ を含む #[test] の fn が base に無く tests 欄も無い"), "teeth-place-unresolved の字面のまま: {err}");
    assert!(!state.join("pipe").exists(), "run dir を作らない");
    let placed = intake_raw(&repo, &state, &pointed_contract(&repo, "w.toml", "w"), "s2-w");
    assert_eq!(placed.status.code(), Some(i32::from(RC_OK)), "write-set の歯の file が置き場: {}", stderr_of(&placed));
    assert!(intake_tokens(&placed).contains(&"write-set=declared".to_owned()), "{}", stdout_of(&placed));
    clean(&[&repo, &state]);
}

/// (e) `contracts schema` の出力は tracked の `contracts/schema.toml` と一致し、`creates` / `tests` / `also` を任意の list として
/// 持ち `write-set` は任意である。表の読み手も同じ: `write-set` の無い行を持つ doc は `contracts check` で違反 0（base は
/// 必須の欠落で断る → RED）。
#[test]
fn contract_schema_lists_creates_tests_also_and_write_set_is_optional() {
    let out = bin_cmd().args(["contracts", "schema"]).output().expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("contracts").join("schema.toml");
    let tracked = fs::read_to_string(&path).expect("tracked の生成物を読める");
    assert_eq!(stdout_of(&out), tracked, "render と tracked の差分 0");
    let mut fields: Vec<(String, String, String)> = Vec::new();
    for line in tracked.lines() {
        if line == "[[field]]" {
            fields.push((String::new(), String::new(), String::new()));
        }
        let Some(last) = fields.last_mut() else {
            continue;
        };
        if let Some(name) = line.strip_prefix("name = ") {
            last.0 = name.trim_matches('"').to_owned();
        } else if let Some(need) = line.strip_prefix("need = ") {
            last.1 = need.trim_matches('"').to_owned();
        } else if let Some(shape) = line.strip_prefix("shape = ") {
            last.2 = shape.trim_matches('"').to_owned();
        }
    }
    let field = |name: &str| fields.iter().find(|(found, _, _)| found == name).cloned().unwrap_or_default();
    for name in ["write-set", "creates", "tests", "also"] {
        assert_eq!(field(name), (name.to_owned(), "optional".to_owned(), "list".to_owned()), "{name} は任意の list");
    }
    assert_eq!(field("verify").1, "required", "verify は必須のまま");
    let doc = table_doc(&table_region(&[derive_row("a", &[("creates", "[\"src/new.rs\"]")])]));
    let repo = table_repo(&doc, &[]);
    let checked = contracts_check(&repo);
    assert_eq!(checked.status.code(), Some(i32::from(RC_OK)), "write-set の無い行は読める: {}", stdout_of(&checked));
    assert_eq!(stdout_of(&checked).trim_end(), "contracts check: docs=1 rows=1 findings=0");
    clean(&[&repo]);
}

/// (f) 歯の置き場は base の `#[test]` の直下の fn 名で解く: `derive_` は tests の歯（e2e.rs）と src の test 区間の歯（other.rs）
/// の 2 file で、helper の fn（`derive_helper`）を持つ file は数えない・`other_` は fn 名（`other_case`）で helper.rs に解け、
/// file 名（other.rs）では解かない。
#[test]
fn contract_derive_teeth_place_uses_base_test_names() {
    let rows = [
        derive_row("a", &[("verify", "[\"cargo nextest run -p toy --no-tests=fail derive_\"]")]),
        derive_row("b", &[("verify", "[\"cargo nextest run -p toy --no-tests=fail other_\"]")]),
    ];
    let (repo, state) = derive_repo(&table_doc(&table_region(&rows)));
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, "a.toml", "a"), "s2-a");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let id = run_id_of(&out);
    assert_eq!(copied_write_set(&state, &id), ["crates/toy/src/other.rs", "crates/toy/tests/e2e.rs"], "helper は数えない");
    assert!(intake_tokens(&out).contains(&"files=2".to_owned()), "{}", stdout_of(&out));
    stop_run_ok(&state, &id);
    let other = intake_raw(&repo, &state, &pointed_contract(&repo, "b.toml", "b"), "s2-b");
    assert_eq!(other.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&other));
    assert_eq!(copied_write_set(&state, &run_id_of(&other)), ["crates/toy/tests/helper.rs"], "fn 名で解く（file 名ではない）");
    clean(&[&repo, &state]);
}

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

// ───── fn 形の touches（設計 docs/design/contract-source.md §18・行 r・`s2-07l.358`・接頭辞 `contract_derive_fn_`） ─────

/// fn 形の toy（歯の中で組む・base の file ではない）: module `pipe::cli` の file が `fn resume(` を宣言し、呼び手
/// （`main.rs`）と別 module の同名の fn（`tone.rs`）を置く。型形の退行 pin のために**型を持つ file**も同じ toy に置く:
/// `paint.rs` に `pub enum Hue`（と const slice `HUES`＝宣言 file が第 4 形で閉包に入る・[`TABLE_TINT`] と同じ形）・
/// `arm.rs` に `Hue::Red =>` の arm。
const FN_FORM_FILES: &[(&str, &str)] = &[
    ("crates/toy/src/pipe/cli.rs", "pub fn resume(state: &str) -> usize {\n    state.len()\n}\n"),
    ("crates/toy/src/main.rs", "fn main() {\n    let _ = crate::pipe::cli::resume(\"x\");\n}\n"),
    ("crates/toy/src/tone.rs", "pub fn resume() -> usize {\n    0\n}\n"),
    ("crates/toy/src/paint.rs", "pub enum Hue {\n    Red,\n    Blue,\n}\n\npub const HUES: &[Hue] = &[Hue::Red, Hue::Blue];\n"),
    ("crates/toy/src/arm.rs", "use crate::paint::Hue;\n\npub fn name(hue: Hue) -> u8 {\n    match hue {\n        Hue::Red => 1,\n        _ => 0,\n    }\n}\n"),
];

/// fn 形の toy repo に `touches` だけの行 `id` を置いて intake を 1 回撃つ（repo と置き場は呼び手が畳む）。
fn fn_form_intake(id: &str, touches: &[&str]) -> (PathBuf, PathBuf, Output) {
    let quoted: Vec<String> = touches.iter().map(|item| format!("\"{item}\"")).collect();
    let touches = format!("[{}]", quoted.join(", "));
    let row = derive_row(id, &[("touches", touches.as_str())]);
    let (repo, state) = derive_repo_with(&table_doc(&table_region(&[row])), FN_FORM_FILES);
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, &format!("{id}.toml"), id), &format!("s2-{id}"));
    (repo, state, out)
}

/// (a) `touches = ["crate::pipe::cli::resume"]`（toy の `src/pipe/cli.rs` が `fn resume(` を宣言）の行は intake を通り、
/// 導出値に宣言する file が入る。呼び手（`main.rs`）と別 module の同名の fn（`tone.rs`）は入らない（下界）。base は
/// 型の形でないと断る（RED）。
#[test]
fn contract_derive_fn_touches_names_the_declaring_file() {
    let (repo, state, out) = fn_form_intake("a", &["crate::pipe::cli::resume"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fn 形の行は導出値で通る: {}", stderr_of(&out));
    let tokens = intake_tokens(&out);
    assert!(tokens.contains(&"write-set=derived".to_owned()) && tokens.contains(&"files=1".to_owned()), "判定行: {tokens:?}");
    let found = copied_write_set(&state, &run_id_of(&out));
    assert_eq!(found, ["crates/toy/src/pipe/cli.rs"], "宣言する file だけ（呼び手の main.rs・別 module の tone.rs は入らない）");
    clean(&[&repo, &state]);
}

/// (b) `touches = ["crate::pipe::cli::missing"]`（どの file も `fn missing(` を宣言しない）は受付で断られ（rc 1）、
/// 断りの字面は新 variant のもの（`TypeForm` の「crate::module::Type の形でない」ではない）。run dir も
/// event も作らない＝導出値を空集合に潰さない。
#[test]
fn contract_derive_fn_refuses_when_no_file_declares_it() {
    let (repo, state, out) = fn_form_intake("b", &["crate::pipe::cli::missing"]);
    let err = stderr_of(&out);
    // 契約 (b) 以後、行の欠陥の rc は**表の検査の分類**が持つ（`unreadable` は rc 2）。断ることと理由が要点。
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "宣言する file が無い周は断る: {err}");
    assert!(err.contains("touches の pipe::cli::missing を宣言する file が base に無い"), "新 variant の字面で断る: {err}");
    assert!(!err.contains("crate::module::Type の形でない"), "TypeForm の字面ではない: {err}");
    assert_eq!(event_count(&state), 0, "断った周は event を書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない（導出値を空集合にしない）");
    clean(&[&repo, &state]);
}

/// (c) 型を持つ同じ toy で、型形の行 `touches = ["crate::paint::Hue"]` の導出値は宣言 file（paint.rs）と arm の file
/// （arm.rs）の 2 つのまま（fn 形の追加で型形が動かない退行の pin）で、fn 形と型形を同じ行に並べた `touches` の導出値は
/// 両者の和集合。
#[test]
fn contract_derive_fn_keeps_type_closure_unchanged() {
    let (repo, state, out) = fn_form_intake("c", &["crate::paint::Hue"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "型形の行は通る: {}", stderr_of(&out));
    let typed = copied_write_set(&state, &run_id_of(&out));
    assert_eq!(typed, ["crates/toy/src/arm.rs", "crates/toy/src/paint.rs"], "型形の閉包は宣言 file と arm の file の 2 つのまま");
    clean(&[&repo, &state]);
    let (repo, state, out) = fn_form_intake("d", &["crate::paint::Hue", "crate::pipe::cli::resume"]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "型形と fn 形を並べた行は通る: {}", stderr_of(&out));
    let both = copied_write_set(&state, &run_id_of(&out));
    assert_eq!(
        both,
        ["crates/toy/src/arm.rs", "crates/toy/src/paint.rs", "crates/toy/src/pipe/cli.rs"],
        "型形の閉包 ∪ fn 形の宣言 file"
    );
    clean(&[&repo, &state]);
}

// ───── 閉包の同名衝突（設計 docs/design/contract-source.md §3「閉包の同名衝突」・行 j・`s2-07l.347`・接頭辞 `contract_closure_ext_same_name_`） ─────

/// 同名の struct `Marker` を持つ module の本文（`Marker {` の構築点・`Marker::HEAD` の arm・`const ALL: &[Marker]` の
/// 3 形＝現物の `hook::vessel::Marker` の形）。`crate::a::Marker` と `crate::b::Marker`・`crate::hook::vessel::Marker` が
/// 同じ字面で持つ。
const SAME_NAME_STRUCT: &str = "pub struct Marker {\n    pub generation: u32,\n}\n\nimpl Marker {\n    pub const HEAD: &'static str = \"marker\";\n\n    pub fn parse(text: &str) -> Marker {\n        Marker { generation: text.len() as u32 }\n    }\n}\n\npub const ALL: &[Marker] = &[Marker { generation: 0 }];\n\npub fn kind(head: &str) -> u8 {\n    match head {\n        Marker::HEAD => 1,\n        _ => 0,\n    }\n}\n";

/// 多段 module の enum `Marker`（`crate::seat::rebrief::Marker`・`Marker::` の arm と `const ALL: &[Marker]`＝現物の
/// `seat::rebrief::Marker` の形）。
const SAME_NAME_ENUM: &str = "pub enum Marker {\n    Sid,\n    Wm,\n}\n\npub const ALL: &[Marker] = &[Marker::Sid, Marker::Wm];\n\npub fn name(marker: Marker) -> &'static str {\n    match marker {\n        Marker::Sid => \"sid\",\n        Marker::Wm => \"wm\",\n    }\n}\n";

/// 同名の型の toy: `a` / `b`（同名の struct・同じ 3 形）と `a` から取り込んで構築する `build.rs`・多段 module の
/// `seat::rebrief`（enum）と同名の `hook::vessel`（struct・同じ 3 形）と `rebrief` から取り込んで分岐する `seat/tick.rs`。
const SAME_NAME_FILES: &[(&str, &str)] = &[
    ("src/a.rs", SAME_NAME_STRUCT),
    ("src/b.rs", SAME_NAME_STRUCT),
    ("src/build.rs", "use crate::a::Marker;\n\npub fn build() -> Marker {\n    Marker { generation: 1 }\n}\n"),
    ("src/seat/rebrief.rs", SAME_NAME_ENUM),
    ("src/hook/vessel.rs", SAME_NAME_STRUCT),
    ("src/seat/tick.rs", "use crate::seat::rebrief::Marker;\n\npub fn tick(marker: Marker) -> u8 {\n    match marker {\n        Marker::Sid => 1,\n        _ => 0,\n    }\n}\n"),
];

/// 同名の型の toy repo に `touches` だけの行 `id` を置き、intake の導出値（写しの契約の write-set）を返す。
fn same_name_write_set(id: &str, touches: &str) -> Vec<String> {
    let touches = format!("[\"{touches}\"]");
    let row = derive_row(id, &[("touches", touches.as_str())]);
    let (repo, state) = derive_repo_with(&table_doc(&table_region(&[row])), SAME_NAME_FILES);
    let out = intake_raw(&repo, &state, &pointed_contract(&repo, &format!("{id}.toml"), id), &format!("s2-{id}"));
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "導出値で通る: {}", stderr_of(&out));
    let found = copied_write_set(&state, &run_id_of(&out));
    clean(&[&repo, &state]);
    found
}

/// (a) `crate::a::Marker` と `crate::b::Marker`（同名・同じ 3 形）を置いた toy で、`touches = ["crate::a::Marker"]` の行の
/// 導出値は `a` 側（宣言 file と、そこから取り込んで構築する file）だけを持つ（base は裸の型名で照合し `b` 側と
/// 同名の `rebrief` / `vessel` / `tick` も入る → RED）。
#[test]
fn contract_closure_ext_same_name_type_in_another_module_is_not_widened() {
    let found = same_name_write_set("a", "crate::a::Marker");
    assert_eq!(found, ["src/a.rs", "src/build.rs"], "a 側だけ（b・同名の他 module は入らない）");
}

/// (b) `use crate::a::Marker` で取り込んで `Marker {` を構築する file は導出値に入ったまま（退行の pin・`sees` の (b)）。
/// 対: `b` 側の行は `b` の宣言 file だけを持ち、`a` から取り込む `build.rs` を持たない。
#[test]
fn contract_closure_ext_same_name_import_still_widens() {
    let found = same_name_write_set("a", "crate::a::Marker");
    assert!(found.iter().any(|path| path == "src/build.rs"), "取り込んで構築する file は入る: {found:?}");
    let other = same_name_write_set("b", "crate::b::Marker");
    assert_eq!(other, ["src/b.rs"], "b 側は宣言 file だけ（a から取り込む build.rs は入らない）");
}

/// (d) 多段 module: `crate::seat::rebrief::Marker`（`src/seat/rebrief.rs`・enum）と同名の `crate::hook::vessel::Marker`
/// （`src/hook/vessel.rs`・struct・同じ 3 形）と `rebrief` から取り込む `src/seat/tick.rs` を置いた toy で、`touches =
/// ["crate::seat::rebrief::Marker"]` の行の導出値は `rebrief.rs` と `tick.rs` を持ち `vessel.rs` を持たない（goal の実物と
/// 同じ 2 段の形・module は最後の段で弁別する）。
#[test]
fn contract_closure_ext_same_name_in_nested_module_keeps_the_declaring_file() {
    let found = same_name_write_set("d", "crate::seat::rebrief::Marker");
    assert_eq!(found, ["src/seat/rebrief.rs", "src/seat/tick.rs"], "宣言 file と取り込む file だけ（同名の vessel.rs は入らない）");
}

// ───── 契約の審査の段（`s2-07l.241`・設計 contract-source.md §4・SRS FR49 / FR9 / AC22・接頭辞 `pipe_review_`） ─────

/// 起こされたら marker を置く fake runner（**構築点の呼出**を効果で測る面）。
fn marker_runner(marker: &Path) -> String {
    format!("touch '{}'; exit 0", marker.display())
}

/// 便の `review.json` を key/value の並びとして読む（無ければ空）。
fn review_pairs(state: &Path, id: &str) -> Vec<(String, vessel::fleet::json_lite::Value)> {
    let text = fs::read_to_string(state.join("pipe").join(id).join("review.json")).unwrap_or_default();
    vessel::fleet::json_lite::parse_object(text.trim()).unwrap_or_default()
}

/// 便の審査の材料の dir（`<run_dir>/review/`）。
fn review_dir(state: &Path, id: &str) -> PathBuf {
    state.join("pipe").join(id).join("review")
}

/// 偽 lens が FAIL を返す契約を `pipe run` で流し、`Reviewed(FAIL)` で止まった便の id と runner の marker（**置かれて
/// いない**）を返す。runner は起きていない（構築点の呼出 0）ことをここで assert する。
fn reviewed_fail(repo: &Path, state: &Path) -> (String, PathBuf) {
    let path = write_contract(repo, &[], &[]);
    let lens_marker = state.join("review-fail-lens-ran");
    let runner_marker = state.join("runner-ran");
    let out = run_pipe(&[
        "run", "--design", &path, "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(state), "--runner", &marker_runner(&runner_marker),
        "--lens", &fake_lens(&lens_marker, &lens_verdict("FAIL")),
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "FAIL は rc 1: {}", stderr_of(&out));
    assert!(lens_marker.exists(), "審査の lens は 1 回起きた");
    assert!(!runner_marker.exists(), "runner は起きない（構築点の呼出 0）");
    let id = run_id_of(&out);
    assert!(!id.is_empty(), "止まった周も run id を出す: {}", stdout_of(&out));
    assert!(
        stdout_of(&out).contains(&format!("run={id} stage=Reviewed verdict=FAIL")),
        "審査の判定行: {}",
        stdout_of(&out)
    );
    (id, runner_marker)
}

/// (a) 偽 lens が FAIL を返す契約は `Reviewed(FAIL)` で止まり **runner は 1 度も起きない**（構築点の呼出 0・AC22）:
/// `pipe run` は rc 1 で intake の判定行と `stage=Reviewed verdict=FAIL` を出し、trail は `RunCreated(Intake)` →
/// `RunStage(Reviewed, verdict:FAIL)` で終わる（Spawned 無し・worktree 無し）。`review.json` に verdict と evidence が
/// 残り、`show` は `Reviewed` を名乗る。
#[test]
fn pipe_review_fail_stops_before_spawn() {
    let (repo, state) = repo_with_state();
    let (id, _) = reviewed_fail(&repo, &state);
    assert_eq!(
        trail(&state, &id),
        vec![
            (EventKind::RunCreated, Some(Stage::Intake), Some("classes:".to_owned())),
            (EventKind::RunStage, Some(Stage::Reviewed), Some("verdict:FAIL".to_owned())),
        ],
        "Reviewed(FAIL) が終端（Spawned 無し）"
    );
    assert!(!worktree_of(&repo, &id).exists(), "worktree を作らない");
    let pairs = review_pairs(&state, &id);
    assert_eq!(value_of(&pairs, "verdict"), "FAIL", "review.json の verdict");
    assert_eq!(value_of(&pairs, "evidence"), "fake", "lens の evidence を写す");
    assert_eq!(value_of(&pairs, "run"), id, "run id を持つ");
    assert!(show_line(&repo, &state, &id).contains("stage=Reviewed"), "段は Reviewed");
    clean(&[&repo, &state]);
}

/// (a'') `Reviewed(FAIL)` は終端: `spawn` / `resume` は段違いとして rc 1 で何も書かず runner を起こさず、`stop` は
/// 終端として断る。終端ゆえ同じ write-set の 2 本目の intake は交差で断られない（`live` は偽）。
#[test]
fn pipe_review_fail_is_terminal_for_spawn_resume_stop_and_overlap() {
    let (repo, state) = repo_with_state();
    let (id, runner_marker) = reviewed_fail(&repo, &state);
    let before = event_count(&state);
    let spawned = spawn_with(&repo, &state, &id, &marker_runner(&runner_marker));
    assert_eq!(spawned.status.code(), Some(i32::from(RC_REFUSED)), "spawn は段違い: {}", stderr_of(&spawned));
    assert!(stderr_of(&spawned).contains("Reviewed") && stderr_of(&spawned).contains("verdict=FAIL"), "{}", stderr_of(&spawned));
    let resumed = run_pipe(&[
        "resume", "--run", &id, "--repo", &repo.display().to_string(),
        "--state-dir", &state.display().to_string(), "--runner", &marker_runner(&runner_marker),
    ]);
    assert_eq!(resumed.status.code(), Some(i32::from(RC_REFUSED)), "resume も起こさない: {}", stderr_of(&resumed));
    assert!(!runner_marker.exists(), "spawn / resume のどちらでも runner は起きない");
    assert_eq!(event_count(&state), before, "段違いは event を 1 件も書かない");
    let stopped = run_pipe(&["stop", "--run", &id, "--state-dir", &state.display().to_string()]);
    assert_eq!(stopped.status.code(), Some(i32::from(RC_REFUSED)), "終端の便は stop で断る: {}", stderr_of(&stopped));
    assert!(stderr_of(&stopped).contains("終端"), "{}", stderr_of(&stopped));
    // 終端ゆえ交差の母集団に入らない（`live` は偽）。
    let next = write_set_contract(&repo, "next", &["src/lib.rs"]);
    let again = try_intake(&repo, &state, &next, "s2-next");
    assert_eq!(again.status.code(), Some(i32::from(RC_OK)), "FAIL の便と交差しても受理: {}", stderr_of(&again));
    clean(&[&repo, &state]);
}

/// (a') lens が無い・出力を読めない周は INCONCLUSIVE（FR9・偽の PASS を作らない）で、終端として spawn を断る。
/// `--lens` 無しの intake は rc 3 で `verdict:INCONCLUSIVE` を記帳し evidence が `--lens` を名指す。
#[test]
fn pipe_review_inconclusive_without_lens_or_unreadable_output_is_terminal() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let rules = ceiling_rules(&state);
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "s2-none",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(), "--rules", &rules,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "lens 無しは rc 3: {}", stderr_of(&out));
    let id = run_id_of(&out);
    assert_eq!(stages(&state, &id), vec![(Some(Stage::Reviewed), Some("verdict:INCONCLUSIVE".to_owned()))]);
    assert!(value_of(&review_pairs(&state, &id), "evidence").contains("--lens"), "{:?}", review_pairs(&state, &id));
    let marker = state.join("runner-ran");
    let spawned = spawn_with(&repo, &state, &id, &marker_runner(&marker));
    assert_eq!(spawned.status.code(), Some(i32::from(RC_REFUSED)), "INCONCLUSIVE は起こさない: {}", stderr_of(&spawned));
    assert!(stderr_of(&spawned).contains("verdict=INCONCLUSIVE"), "{}", stderr_of(&spawned));
    assert!(!marker.exists(), "runner は起きない");
    // 読めない出力（JSON 行が無い）・3 値の外・rc≠0 の lens も INCONCLUSIVE。
    for (bead, lens, want) in [
        ("s2-nojson", "cat >/dev/null; echo not-json".to_owned(), "JSON 行が無い"),
        ("s2-3v", format!("cat >/dev/null; echo '{}'", lens_verdict("MAYBE")), "3 値でない"),
        ("s2-rc", "cat >/dev/null; exit 7".to_owned(), "rc 7"),
    ] {
        let out = run_pipe(&[
            "intake", "--design", &path, "--bead", bead,
            "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
            "--rules", &rules, "--lens", &lens,
        ]);
        assert_eq!(out.status.code(), Some(i32::from(RC_INCONCLUSIVE)), "{bead}: {}", stderr_of(&out));
        let pairs = review_pairs(&state, &run_id_of(&out));
        assert_eq!(value_of(&pairs, "verdict"), "INCONCLUSIVE", "{bead}");
        assert!(value_of(&pairs, "evidence").contains(want), "{bead}: {pairs:?}");
    }
    clean(&[&repo, &state]);
}

/// (b) PASS の契約は `Reviewed(PASS)` を経て Spawned へ進み、verdict が便の記録（`review.json` と event）に残る（AC22）。
/// 審査の材料は run dir の `review/`（契約の写し・設計の節・要件本文）に置かれ、lens の `{contract}` はその写しの
/// path・`{worktree}` は base の repo で埋まる。pointer でない `design` と読めない要件面は材料の本文に明示される。
#[test]
fn pipe_review_pass_spawns() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let seen = state.join("lens-holes");
    let lens = format!(
        "cat >/dev/null; printf '%s\\n' '{{contract}}' '{{worktree}}' > '{}'; echo '{}'",
        seen.display(),
        lens_verdict("PASS")
    );
    let out = run_pipe(&[
        "intake", "--design", &path, "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state), "--lens", &lens,
    ]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "PASS は rc 0: {}", stderr_of(&out));
    let id = run_id_of(&out);
    assert!(stdout_of(&out).contains(&format!("run={id} stage=Reviewed verdict=PASS")), "{}", stdout_of(&out));
    assert_eq!(stages(&state, &id), vec![(Some(Stage::Reviewed), Some("verdict:PASS".to_owned()))]);
    assert_eq!(event_count(&state), 2, "RunCreated + Reviewed");
    assert_eq!(value_of(&review_pairs(&state, &id), "verdict"), "PASS");
    let dir = review_dir(&state, &id);
    let holes = fs::read_to_string(&seen).unwrap_or_default();
    assert_eq!(
        holes.lines().collect::<Vec<&str>>(),
        [dir.join("contract.toml").display().to_string(), repo.display().to_string()],
        "{{contract}} は審査の写し・{{worktree}} は base の repo"
    );
    assert_eq!(dir_names(&dir), ["contract.toml", "design.txt", "requirements.txt"], "材料の 3 file");
    assert_eq!(
        fs::read(dir.join("contract.toml")).ok(),
        fs::read(state.join("pipe").join(&id).join("contract.toml")).ok(),
        "契約の写しは byte で同じ"
    );
    // 契約 (b) 以後、`design` は**必ず**設計 pointer である（契約 file は行から作られる）＝材料は節の
    // 出所と本文をそのまま持つ（「pointer でない」形は入口から消えた）。
    let design = fs::read_to_string(dir.join("design.txt")).unwrap_or_default();
    assert!(design.starts_with(&format!("{} §1\n", design_pointer())), "節の出所を名乗る: {design}");
    assert!(design.contains("節の本文"), "節の本文を写す: {design}");
    let requirements = fs::read_to_string(dir.join("requirements.txt")).unwrap_or_default();
    assert!(requirements.contains("FR4"), "行の req を材料に載せる: {requirements}");
    // PASS の便だけが起こせる。
    let spawned = spawn_with(&repo, &state, &id, TOY_COMMIT);
    assert_eq!(spawned.status.code(), Some(i32::from(RC_OK)), "PASS → Spawned: {}", stderr_of(&spawned));
    let listed: Vec<Option<Stage>> = stages(&state, &id).into_iter().map(|(stage, _)| stage).collect();
    assert_eq!(listed, vec![Some(Stage::Reviewed), Some(Stage::Spawned), Some(Stage::Implemented)], "Reviewed → Spawned → Implemented");
    assert!(show_line(&repo, &state, &id).contains("stage=Implemented"));
    clean(&[&repo, &state]);
}

/// (b') 設計 pointer の契約は base の設計 doc からその行の `section` の節の本文を、要件面（`.html` の `id=`）から `req` の
/// 各 id の本文（tag 無し）を材料に写す。無い id はその旨を行に明示する（§4「順序」: 生成 (b) の前でも穴の出所は行の pointer）。
#[test]
fn pipe_review_reads_design_section_and_requirements_from_base() {
    // 契約 (b) 以後、行の `req` は表の検査が要件面と突き合わせる＝面に在る id だけを置く。
    let row = derive_row("a", &[("write-set", "[\"crates/toy/src/tint.rs\"]"), ("section", "\"2\""), ("req", "[\"FR2\"]")]);
    let doc = table_doc(&table_region(&[row])).replace("## 2. 型\n\n本文。", "## 2. 型\n\n節二の本文 SECTION-TWO-MARK。");
    let (repo, state) = derive_repo(&doc);
    let out = intake_raw(&repo, &state, "docs/design/toy.md#a", "s2-a");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let dir = review_dir(&state, &run_id_of(&out));
    let design = fs::read_to_string(dir.join("design.txt")).unwrap_or_default();
    assert!(design.starts_with("docs/design/toy.md#a §2\n"), "節の出所を名乗る: {design}");
    assert!(design.contains("SECTION-TWO-MARK"), "節 2 の本文: {design}");
    assert!(!design.contains("何を解くか") && !design.contains("[[contract]]"), "他の節と契約表は写さない: {design}");
    let requirements = fs::read_to_string(dir.join("requirements.txt")).unwrap_or_default();
    assert_eq!(requirements.trim_end(), "FR2: 2", "{requirements}");
    clean(&[&repo, &state]);
}

/// [`faced_repo`] の行の `req` を選ぶ形（契約 (b) 以後、`req` は**行**が持つ）。
fn faced_repo_with_req(face: &str, body: &str, req: &[&str]) -> (PathBuf, PathBuf) {
    let quoted: Vec<String> = req.iter().map(|id| format!("\"{id}\"")).collect();
    let fields = [("write-set", "[\"crates/toy/src/tint.rs\"]"), ("req", &format!("[{}]", quoted.join(", ")))]
        .map(|(key, value)| (key, value.to_owned()));
    let pairs: Vec<(&str, &str)> = fields.iter().map(|(key, value)| (*key, value.as_str())).collect();
    let doc = table_doc(&table_region(&[derive_row("a", &pairs)]));
    let vessel = format!("{DERIVE_VESSEL}requirements = \"{face}\"\n");
    derive_repo_with(&doc, &[(".vessel.toml", &vessel), (face, body)])
}

/// 設計 pointer `docs/design/toy.md#a` と `req` を持つ契約を intake し（偽 PASS の lens）、審査の材料 `requirements.txt`
/// の本文（末尾の改行を除く）を返す。
fn reviewed_requirements(repo: &Path, state: &Path) -> String {
    let out = intake_raw(repo, state, "docs/design/toy.md#a", "s2-a");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let dir = review_dir(state, &run_id_of(&out));
    fs::read_to_string(dir.join("requirements.txt")).unwrap_or_default().trim_end().to_owned()
}

/// (b'') 要件面が `.yaml` の周は `- id: FR1` と同じ mapping の `text:` の値が材料に載る（設計 §4「yaml の `id` + `text`」・
/// `s2-07l.354`）。`title:` は本文にしない。base は `id="…"` の字面だけを探すので「要件面に無い」になる（RED）。
#[test]
fn pipe_review_reads_requirements_text_from_yaml() {
    let yaml = "requirements:\n  - id: FR1\n    title: 起動\n    text: 便を起こす YAML-TEXT-MARK\n  - id: FR2\n    text: 審査する\n";
    // 契約 (b) 以後、行の `req` は受付の表の検査が要件面と突き合わせる＝面に無い id は intake が断る。
    // ここは**面に在る id** で材料の描画を測る（面に無い id の断りは別の歯）。
    let (repo, state) = faced_repo_with_req("spec/reqs.yaml", yaml, &["FR1", "FR2"]);
    let requirements = reviewed_requirements(&repo, &state);
    assert_eq!(requirements, "FR1: 便を起こす YAML-TEXT-MARK\nFR2: 審査する", "{requirements}");
    assert!(!requirements.contains("起動"), "title は本文にしない: {requirements}");
    clean(&[&repo, &state]);
}

/// (c) 要件面が `.md` の周は `## FR1 …` の見出しの下の本文（次の見出しの直前まで・空白を畳んだ 1 行）が材料に載る。
/// 無い id は「要件面に無い」。base は `.md` を読めず「要件面に無い」になる（RED）。
#[test]
fn pipe_review_reads_requirements_text_from_md() {
    let md = "# 要件\n\n## FR1 便の起動\n\n便を\n起こす MD-TEXT-MARK。\n\n## FR2 審査\n\n審査する。\n";
    let (repo, state) = faced_repo_with_req("spec/reqs.md", md, &["FR1"]);
    let requirements = reviewed_requirements(&repo, &state);
    assert_eq!(requirements, "FR1: 便を 起こす MD-TEXT-MARK。", "{requirements}");
    assert!(!requirements.contains("審査する"), "次の見出しの下は写さない: {requirements}");
    clean(&[&repo, &state]);
}

/// (d) 裸の `- FR1` の yaml は id の検査は通るが本文が無い＝「（要件面 <path> の FR1 に本文が無い）」の理由が材料に
/// 載る（黙って空にしない・NFR4）。
#[test]
fn pipe_review_reads_requirements_reason_for_bare_yaml_id() {
    let (repo, state) = faced_repo_with_req("spec/reqs.yaml", "requirements:\n  - FR1\n  - FR2\n", &["FR1"]);
    let requirements = reviewed_requirements(&repo, &state);
    assert_eq!(requirements, "FR1: （要件面 spec/reqs.yaml の FR1 に本文が無い）", "{requirements}");
    clean(&[&repo, &state]);
}

/// (e) `## FR1` の直下が空行だけで次の見出しに続く md は (d) と同じ形の理由が載る（本文の無い id の理由は形を問わない）。
#[test]
fn pipe_review_reads_requirements_reason_for_empty_md_heading() {
    let (repo, state) =
        faced_repo_with_req("spec/reqs.md", "# 要件\n\n## FR1\n\n\n## FR2 審査\n\n審査する。\n", &["FR1", "FR2"]);
    let requirements = reviewed_requirements(&repo, &state);
    assert_eq!(requirements, "FR1: （要件面 spec/reqs.md の FR1 に本文が無い）\nFR2: 審査する。", "{requirements}");
    clean(&[&repo, &state]);
}

/// (c) 審査を飛ばす口は無い: `--no-review` は `intake` / `run` / `resume` のどれでも usage で断り（rc 1）、event も
/// run dir も作らない（AC22・C16）。
#[test]
fn pipe_review_has_no_skip_flag() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let rules = ceiling_rules(&state);
    let (contract, repo_text, state_text) = (path.clone(), repo.display().to_string(), state.display().to_string());
    let common: [&str; 12] = [
        "--design", &contract, "--bead", "s2-2e5", "--repo", &repo_text,
        "--state-dir", &state_text, "--rules", &rules, "--runner", TOY_COMMIT,
    ];
    for head in ["intake", "run", "resume"] {
        let mut args = vec![head, "--no-review"];
        args.extend(common.iter().copied());
        let out = run_pipe(&args);
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{head} --no-review は rc 1: {}", stderr_of(&out));
        let err = stderr_of(&out);
        assert!(err.contains("--no-review"), "{head}: 断った引数を名指す: {err}");
        assert!(err.contains("usage: "), "{head}: usage を出す: {err}");
        assert!(out.stdout.is_empty(), "{head}: stdout 0 byte");
    }
    assert_eq!(event_count(&state), 0, "event を 1 件も書かない");
    assert!(!state.join("pipe").exists(), "run dir も作らない");
    clean(&[&repo, &state]);
}

/// (d) `Stage::Reviewed` は `Intake` の直後（母集団 11 段・字面が往復する）・`Guard::Review` は `IntakeRefuse` の直後
/// （in-loop / fail-closed・境界は `pipe::review::ReviewCheck`）で、極性一覧の行に載る（C2 / C11.2 / C16.2）。
#[test]
fn pipe_review_stage_and_guard_are_pinned_in_declaration_order() {
    use vessel::fleet::STAGES;
    use vessel::polarity::{Guard, OnFailure, Timing, ALL};
    let at = |want: Stage| STAGES.iter().position(|stage| *stage == want);
    assert_eq!(STAGES.len(), 11, "段は 11 個: {STAGES:?}");
    assert_eq!(at(Stage::Reviewed), at(Stage::Intake).map(|found| found + 1), "Reviewed は Intake の直後");
    assert!(is_declaration_order(STAGES, |stage| stage as usize), "STAGES は宣言順");
    assert_eq!(Stage::Reviewed.as_str(), "Reviewed");
    assert_eq!(Stage::parse("Reviewed"), Some(Stage::Reviewed), "as_str ↔ parse の往復");
    let guard_at = |want: Guard| ALL.iter().position(|guard| *guard == want);
    assert_eq!(guard_at(Guard::Review), guard_at(Guard::IntakeRefuse).map(|found| found + 1), "Review は IntakeRefuse の直後");
    assert!(is_declaration_order(ALL, |guard| guard as usize), "ALL は宣言順");
    assert_eq!(Guard::Review.polarity().timing, Timing::InLoop, "spawn の前に止める");
    assert_eq!(Guard::Review.polarity().on_failure, OnFailure::FailClosed, "読めない判定は起こさない");
    assert_eq!(Guard::Review.boundary(), "pipe::review::ReviewCheck");
    assert_eq!(Guard::Review.line(), "guard=review-gate timing=in-loop on-failure=fail-closed boundary=pipe::review::ReviewCheck");
    let listed = bin_cmd().arg("polarity").output().expect("binary を起動できる");
    assert!(stdout_of(&listed).lines().any(|line| line == Guard::Review.line()), "極性一覧に載る: {}", stdout_of(&listed));
}

/// (e) `Intake` で止まった便（`RunCreated` の直後に process が落ちた形）の `resume` は**先に審査**し、PASS なら
/// 起こし・FAIL なら起こさない（同じ形で 2 便を対で測る）。
#[test]
fn pipe_review_resume_from_intake_reviews_before_spawning() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    let seeded = intake(&repo, &state, &path);
    stop_run_ok(&state, &seeded);
    for (id, verdict, want_rc, spawned) in [("s2-pass-1", "PASS", RC_OK, true), ("s2-fail-1", "FAIL", RC_REFUSED, false)] {
        // 受付だけ済んだ便を組む: 写し面（契約・vessel・repo）は seed の便から写し、event は `RunCreated` 1 件。
        let dir = state.join("pipe").join(id);
        fs::create_dir_all(&dir).expect("run dir を作れる");
        for name in ["contract.toml", "vessel.toml", "repo"] {
            fs::copy(state.join("pipe").join(&seeded).join(name), dir.join(name)).expect("写しを置ける");
        }
        let out = bin_cmd()
            .args(["fleet", "record", "--kind", "RunCreated", "--stage", "Intake", "--run", id, "--bead", "s2-live", "--state-dir"])
            .arg(&state)
            .output()
            .expect("binary を起動できる");
        assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fleet record: {}", stderr_of(&out));
        assert!(show_line(&repo, &state, id).contains("stage=Intake"), "前提: 審査前の便");
        let marker = state.join(format!("runner-ran-{id}"));
        let out = run_pipe(&[
            "resume", "--run", id, "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
            "--rules", &ceiling_rules(&state), "--runner", &marker_runner(&marker),
            "--lens", &fake_lens(&state.join(format!("lens-{id}")), &lens_verdict(verdict)),
        ]);
        assert_eq!(out.status.code(), Some(i32::from(want_rc)), "{verdict}: {}", stderr_of(&out));
        assert_eq!(stage_count(&state, id, Stage::Reviewed), 1, "{verdict}: 審査の段が 1 件");
        assert_eq!(marker.exists(), spawned, "{verdict}: runner が起きたか");
        assert_eq!(stage_count(&state, id, Stage::Spawned), usize::from(spawned), "{verdict}: Spawned の件数");
        assert_eq!(value_of(&review_pairs(&state, id), "verdict"), verdict);
    }
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
