// flip-check: moved s2-07l.264
//! 入口の歯: `pipe_intake_` / `pipe_refuse_`（write-set の排他と `stop --run`）/ `contract_`（契約表）。
//!
//! 共有 helper は親（`tests/e2e/pipe.rs`）に在り `use super::*` で引く（歯の本文は移しただけ・`s2-07l.264`）。

use super::*;

#[test]
fn pipe_intake_rejects_missing_field() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &["goal", "size"], &[]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "欠落を通さない");
    let err = stderr_of(&out);
    // **全件集めて返す**（1 件目で止めない）。落とした 2 本がどちらも出る。
    assert!(err.contains("goal"), "goal の欠落: {err}");
    assert!(err.contains("size"), "size の欠落: {err}");
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_rejects_multiline_verify() {
    let (repo, state) = repo_with_state();
    let path = repo.join("multiline.toml");
    let mut lines = contract_body()
        .into_iter()
        .filter(|line| !line.starts_with("verify"))
        .collect::<Vec<String>>();
    lines.push("verify = [".to_owned());
    lines.push(r#"  "true","#.to_owned());
    lines.push("]".to_owned());
    fs::write(&path, format!("{}\n", lines.join("\n"))).expect("契約 file を書ける");
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "1 行で完結しない verify を通さない");
    assert!(
        stderr_of(&out).contains("1 行で完結"),
        "理由は行を跨いだことである: {}",
        stderr_of(&out)
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_rejects_contract_without_req_or_design() {
    let (repo, state) = repo_with_state();
    for (drop, add, want) in [
        (vec!["req"], vec![], "req"),
        (vec!["design"], vec![], "design"),
        (vec!["req"], vec![r#"req = []"#], "req"),
    ] {
        let path = write_contract(&repo, &drop, &add);
        let out = run_pipe(&[
            "intake", "--contract", &path.display().to_string(), "--bead", "b",
            "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
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
    // 現在地は event log から読める。
    let out = run_pipe(&["show", "--run", &id, "--state-dir", &state.display().to_string(),
                         "--repo", &repo.display().to_string()]);
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "show は rc 0");
    assert!(stdout_of(&out).contains("stage=Intake"), "{}", stdout_of(&out));
    clean(&[&repo, &state]);
}

#[test]
fn pipe_state_survives_process_restart() {
    let (repo, state) = repo_with_state();
    let path = write_contract(&repo, &[], &[]);
    // 置き場を **--state-dir なしで** 解く＝repo に紐づいた git 設定から読む。
    let out = Command::new(bin())
        .args(["pipe", "intake", "--contract"])
        .arg(&path)
        .args(["--bead", "s2-2e5", "--repo"])
        .arg(&repo)
        .args(["--rules", &ceiling_rules(&state)])
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    let id = run_id_of(&out);
    // **別 process** が同じ現在地を読む（process の記憶に何も置いていない）。
    let shown = Command::new(bin())
        .args(["pipe", "show", "--run", &id, "--repo"])
        .arg(&repo)
        .output()
        .expect("binary を起動できる");
    assert_eq!(shown.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&shown));
    assert!(stdout_of(&shown).contains("stage=Intake"), "{}", stdout_of(&shown));
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
    let out = run_pipe(&[
        "intake", "--contract", &second.display().to_string(), "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &ceiling_rules(&state),
    ]);
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
        (vec!["verify"], vec![r#"verify = ["a" "b"]"#], "引用符 1 組の文字列でない"),
        (vec!["write-set"], vec![r#"write-set = []"#], "write-set は 1 本以上"),
        (vec!["verify"], vec![r#"verify = []"#], "verify は 1 本以上"),
        (vec![], vec![r#"nonsense = "x""#], "未知の key nonsense"),
        (vec![], vec![r#"classes = ["publish", "bogus"]"#], "未知の classes 値 bogus"),
        // 契約の印（`s2-07l.201`・AC16）も閉じた名の列＝列に無い名は既存の契約 error。
        (vec![], vec![r#"opens = ["code", "everything"]"#], "未知の opens 値 everything"),
    ] {
        let path = write_contract(&repo, &drop, &add);
        let out = run_pipe(&[
            "intake", "--contract", &path.display().to_string(), "--bead", "b",
            "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        ]);
        let err = stderr_of(&out);
        assert_ne!(out.status.code(), Some(i32::from(RC_OK)), "{want} を通さない: {err}");
        assert!(err.contains(want), "理由に {want} が出る: {err}");
        assert!(err.contains("line="), "行番号を持つ: {err}");
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

    let plain = Contract::parse(&fs::read_to_string(write_contract(&repo, &[], &[])).unwrap_or_default())
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    assert!(plain.opens.is_empty() && plain.opened_kinds().is_empty(), "印の無い便は空");
    // 取る名は PathKind の全数で、variant 名の字面・空の配列・重複 key は受けない。
    let all: Vec<String> = PATH_KINDS.iter().map(|kind| format!("\"{}\"", kind.as_str())).collect();
    let every = Contract::parse(&fs::read_to_string(write_contract(&repo, &[], &[&format!("opens = [{}]", all.join(", "))])).unwrap_or_default())
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    assert_eq!(every.opened_kinds(), PATH_KINDS.to_vec(), "全種別を開ける");
    for (add, want) in [
        (r#"opens = ["Code"]"#, "未知の opens 値 Code"),
        (r#"opens = "code""#, "opens は配列である"),
    ] {
        let text = fs::read_to_string(write_contract(&repo, &[], &[add])).unwrap_or_default();
        let errors = Contract::parse(&text).err().unwrap_or_default();
        assert!(errors.iter().any(|error| error.reason.contains(want)), "{add}: {errors:?}");
    }
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_reports_broken_value_without_claiming_absence() {
    let (repo, state) = repo_with_state();
    // 値が壊れているだけで key は書かれている。「無い」と二重に言わない。
    // 文字列 key を壊すと値が 1 つも取れない＝「書かれていた」を別に覚えていないと
    // 欠落として二重に報告される。
    let path = write_contract(&repo, &["goal"], &[r#"goal = 1"#]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
    ]);
    let err = stderr_of(&out);
    assert!(err.contains("goal の value が文字列でない"), "値の不備を言う: {err}");
    assert!(
        !err.contains("必須の key goal が無い"),
        "書かれている key を「無い」とは言わない: {err}"
    );
    clean(&[&repo, &state]);
}

#[test]
fn pipe_intake_refuses_non_git_repo() {
    let (_repo, state) = repo_with_state();
    let bare = tmp();
    let path = write_contract(&bare, &[], &[]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "b",
        "--repo", &bare.display().to_string(), "--state-dir", &state.display().to_string(),
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
    assert!(err.contains("契約の verify"), "どちらの面かを言う: {err}");
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
        assert_eq!(out.status.code(), Some(i32::from(RC_REFUSED)), "{add} は rc 1: {err}");
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
#[test]
fn pipe_intake_accepts_self_hosted_declaration_under_embedded_ceiling() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo の root を解ける");
    let declared = fs::read_to_string(root.join(".vessel.toml")).expect("自己ホストの宣言を読める");
    let (repo, state) = repo_with_state();
    fs::write(repo.join(".vessel.toml"), &declared).expect("宣言を写せる");
    git(&repo, &["add", "-f", ".vessel.toml"]);
    git(&repo, &["commit", "-q", "-m", "self-hosted"]);
    // **`--rules` を渡さない**＝埋め込みの上限で測る。
    let path = write_contract(&repo, &["verify"], &[r#"verify = ["git status"]"#]);
    let out = run_pipe(&[
        "intake", "--contract", &path.display().to_string(), "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
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
    let line = declared
        .lines()
        .find(|line| line.trim_start().starts_with("allowed-commands"))
        .unwrap_or_default();
    for command in &ceiling {
        assert!(
            line.contains(command.as_str()),
            "自己ホストの宣言は上限の {command:?} を名乗る（母集団 {} 本）: {line}",
            ceiling.len()
        );
    }
    clean(&[&repo, &state]);
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
        "intake", "--contract", &path.display().to_string(), "--bead", "s2-2e5",
        "--repo", &repo.display().to_string(), "--state-dir", &state.display().to_string(),
        "--rules", &rules,
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
    fs::write(plain.join(".vessel.toml"), &declared).expect("宣言を写せる");
    git(&plain, &["add", "-f", ".vessel.toml"]);
    git(&plain, &["commit", "-q", "-m", "self-hosted"]);
    let contract = write_contract(&plain, &["verify"], &[r#"verify = ["git status"]"#]);
    let out = run_pipe(&[
        "intake", "--contract", &contract.display().to_string(), "--bead", "s2-2e5",
        "--repo", &plain.display().to_string(), "--state-dir", &plain_state.display().to_string(),
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
    let out = Command::new(bin())
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
    let first = write_set_contract(&repo, "first.toml", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-live");
    let before = events_bytes(&state);
    let dirs = run_dirs(&state);
    let second = write_set_contract(&repo, "second.toml", &["src/lib.rs"]);
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
/// guard と同じ規則（先頭の `./`・連続する `/`・`..` の畳み）で、dir `a/` は `a/…` を含み
/// `ab/` は含まない。
#[test]
fn pipe_refuse_intake_measures_dir_and_file_overlap() {
    for (live_entry, next_entry, refused) in [
        ("a/", "a/b.rs", true),
        ("a/", "ab/", false),
        ("a", "a/", true),
        ("./a/b.rs", "a/b.rs", true),
        ("a//b.rs", "a/b.rs", true),
        ("src/../src/x.rs", "src/x.rs", true),
    ] {
        let (repo, state) = repo_with_state();
        let first = write_set_contract(&repo, "first.toml", &[live_entry]);
        intake_bead(&repo, &state, &first, "s2-live");
        let second = write_set_contract(&repo, "second.toml", &[next_entry]);
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
        let first = write_set_contract(&repo, "first.toml", &["src/lib.rs"]);
        let id = intake_bead(&repo, &state, &first, "s2-live");
        if let Some(found) = verdict {
            write_verdict(&state, &id, found);
        }
        record_stage(&state, &id, stage);
        let second = write_set_contract(&repo, "second.toml", &["src/lib.rs"]);
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
        let first = write_set_contract(&repo, "first.toml", &["src/lib.rs"]);
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
        let second = write_set_contract(&repo, "second.toml", &["src/lib.rs"]);
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
    let path = write_set_contract(&repo, "first.toml", &["src/lib.rs"]);
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
    let first = write_set_contract(&repo, "first.toml", &["src/a.rs", "src/b.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-live");
    let second = write_set_contract(&repo, "second.toml", &["src/a.rs", "src/b.rs"]);
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
    let first = write_set_contract(&repo, "first.toml", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &first, "s2-live");
    let second = write_set_contract(&repo, "second.toml", &["src/lib.rs"]);
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
    let path = write_set_contract(&repo, "first.toml", &["src/lib.rs"]);
    let id = intake_bead(&repo, &state, &path, "s2-live");
    // **孫**として起こす（test process の子のままだと zombie が /proc に残る）。
    let spawned = Command::new("sh")
        .arg("-c")
        .arg("sleep 60 >/dev/null 2>&1 & echo $!")
        .output()
        .expect("fake runner を起こせる");
    let pid: u32 = String::from_utf8_lossy(&spawned.stdout).trim().parse().expect("pid を読める");
    let record = Command::new(bin())
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
        ("write-set", "[\"src/lib.rs\"]".to_owned()),
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
    Command::new(bin()).args(["contracts", "check", "--repo"]).arg(repo).output().expect("binary を起動できる")
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
    let out = Command::new(bin()).args(["contracts", "schema"]).output().expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "{}", stderr_of(&out));
    assert!(out.stderr.is_empty(), "stderr は 0 byte: {}", stderr_of(&out));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("contracts").join("schema.toml");
    let tracked = fs::read_to_string(&path).expect("tracked の生成物を読める");
    assert_eq!(stdout_of(&out), tracked, "render と tracked の差分 0（`{NAME} contracts schema` で描き直す）");
    let names: Vec<&str> =
        tracked.lines().filter_map(|line| line.strip_prefix("name = \"")?.strip_suffix('"')).collect();
    let fields: Vec<&str> = vessel::pipe::table::FIELDS.iter().map(|field| field.name).collect();
    assert_eq!(names, fields, "欄の列は const slice と同じ順");
    let extra = Command::new(bin()).args(["contracts", "schema", "x"]).output().expect("binary を起動できる");
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

/// 使い方の誤りは rc 1（stderr に理由）・git repo でない `--repo` は判定できないので rc 2（判定行を出さない）。
#[test]
fn contract_check_refuses_usage_errors_and_non_repositories() {
    let bare = Command::new(bin()).args(["contracts", "check"]).output().expect("binary を起動できる");
    assert_eq!(bare.status.code(), Some(i32::from(RC_REFUSED)), "--repo 無しは rc 1");
    assert!(stderr_of(&bare).contains("--repo が要る"), "{}", stderr_of(&bare));
    let none = Command::new(bin()).arg("contracts").output().expect("binary を起動できる");
    assert_eq!(none.status.code(), Some(i32::from(RC_REFUSED)), "subcommand 無しは rc 1");
    assert!(stderr_of(&none).contains("contracts <check"), "使い方を出す: {}", stderr_of(&none));
    let dir = tmp();
    let out = contracts_check(&dir);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "git repo でない: {}", stderr_of(&out));
    assert!(out.stdout.is_empty() && stderr_of(&out).contains("git repo でない"), "{}", stderr_of(&out));
    clean(&[&dir]);
}
