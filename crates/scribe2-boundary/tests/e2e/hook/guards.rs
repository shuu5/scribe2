// flip-check: moved s2-07l.679
//! guard の族の歯（接頭辞 `host_guard_` / `hook_guard_` / `hook_command_` / `hook_memo_` / `hook_ledger_` / `hook_choice_question_`・設計 docs/design/carry-prep.md §9 行 g）。

use super::*;

#[test]
fn hook_guard_denies_edit_outside_write_set() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/lib.rs\n");
    let out = run_hook(
        "pre-tool-use",
        &tool_payload(&repo, "Edit", "docs/other.md"),
    );

    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "deny は rc 2");
    assert!(out.stdout.is_empty(), "deny でも stdout は 0 byte");
    assert!(!out.stderr.is_empty(), "deny の理由が stderr に 1 行");

    let denies = inject_lines(&state)
        .into_iter()
        .filter(|line| value_of(line, "what") == Some(json_lite::Value::Str("deny".to_owned())))
        .count();
    assert_eq!(denies, 1, "deny も注入の記録に 1 件残る");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_denies_path_escaping_root() {
    let repo = git_repo();
    let state = linked(&repo);
    // **畳んだ後の名前を policy が許す**形で撃つ。こうしないと「write-set の外」でも
    // deny になり、root を出たことを一切測らない歯になる（理由まで弁別する）。
    write_policy(&repo, "outside.rs\n");
    let out = run_hook(
        "pre-tool-use",
        &tool_payload(&repo, "Write", "../outside.rs"),
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "root の外へ出る path は deny"
    );
    assert!(out.stdout.is_empty(), "deny でも stdout は 0 byte");
    assert!(
        stderr_text(&out).contains("repo の外"),
        "deny の理由は write-set 違反でなく root 逸脱である: {}",
        stderr_text(&out)
    );
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_allows_edit_inside_write_set() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/lib.rs\ndocs/\n");
    for target in ["src/lib.rs", "docs/design/x.md"] {
        let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", target));
        assert_silent(&out, &format!("write-set の内側（{target}）"));
    }
    let absolute = repo.join("src").join("lib.rs").display().to_string();
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", &absolute));
    assert_silent(&out, "絶対 path でも root 相対へ正規化して通す");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_fails_closed_when_policy_unreadable() {
    let repo = git_repo();
    let state = linked(&repo);
    let path = write_policy(&repo, "src/lib.rs\n");
    fs::remove_file(&path).expect("policy を消せる");
    fs::create_dir_all(&path).expect("policy の場所を dir にできる");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "src/lib.rs"));
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "policy が読めない周は通さず deny（fail-closed）"
    );
    assert!(out.stdout.is_empty(), "deny でも stdout は 0 byte");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_is_inactive_without_policy_file() {
    let repo = git_repo();
    let state = linked(&repo);
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "anywhere.rs"));
    assert_silent(&out, "policy file が無い周は不活性");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_ignores_bash_tool() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/lib.rs\n");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Bash", "anywhere.rs"));
    assert_silent(&out, "Bash は guard の対象でない（interpreter 経路は v3）");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_denies_decoy_payload() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/lib.rs\n");
    // 値が key 名そのもので、本物の key より手前に在る payload。字面の 1 発目を拾う
    // reader だと、guard は本物の編集先でなく decoy の後ろの値を判定してしまう。
    let decoy = format!(
        "{{\"cwd\":\"{}\",\"tool_name\":\"MultiEdit\",\"tool_input\":{{\"edits\":[{{\"old_string\":\"file_path\",\"new_string\":\"src/lib.rs\"}}],\"file_path\":\"docs/other.md\"}}}}",
        repo.display()
    );
    let out = run_hook("pre-tool-use", &decoy);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "decoy を挟んでも本物の file_path を判定する"
    );
    // notebook 側も同じ形で撃つ（file_path key が存在しない tool）。
    let notebook = format!(
        "{{\"cwd\":\"{}\",\"tool_name\":\"NotebookEdit\",\"tool_input\":{{\"cell_id\":\"file_path\",\"cell_type\":\"src/lib.rs\",\"notebook_path\":\"docs/evil.ipynb\"}}}}",
        repo.display()
    );
    let out = run_hook("pre-tool-use", &notebook);
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "notebook_path も decoy に迂回されない"
    );
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_resolves_relative_path_against_payload_cwd() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/lib.rs\n");
    // cwd が subdir のとき "src/lib.rs" の実体は <repo>/docs/src/lib.rs であり
    // write-set の外。root 基準で解くと通ってしまう。
    let subdir = repo.join("docs");
    fs::create_dir_all(&subdir).expect("subdir を作れる");
    let out = run_hook("pre-tool-use", &tool_payload(&subdir, "Edit", "src/lib.rs"));
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "相対 path は payload の cwd 基準で解く"
    );
    // 同じ path でも cwd が root なら通る（基準が効いていることの対）。
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "src/lib.rs"));
    assert_silent(&out, "cwd が root なら同じ相対 path は write-set の内側");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_denies_symlink_escape() {
    let repo = git_repo();
    let state = linked(&repo);
    let outside = tmp();
    let docs = repo.join("docs");
    fs::create_dir_all(&docs).expect("docs を作れる");
    std::os::unix::fs::symlink(&outside, docs.join("link")).expect("symlink を張れる");
    write_policy(&repo, "docs/\n");
    // 字句では docs/evil.rs に畳まれて allowlist に当たるが、実体は repo の外。
    let out = run_hook(
        "pre-tool-use",
        &tool_payload(&repo, "Write", "docs/link/../evil.rs"),
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "symlink 経由で repo の外へ出る path は deny"
    );
    clean(&[&repo, &state, &outside]);
}

#[test]
fn hook_guard_denies_symlink_inside_repo_outside_write_set() {
    let repo = git_repo();
    let state = linked(&repo);
    let docs = repo.join("docs");
    fs::create_dir_all(&docs).expect("docs を作れる");
    // repo の**内側**で閉じる symlink。root からは一歩も出ないので「repo の外」の段では
    // 落ちない。字句の docs/link/evil.rs は allowlist に当たるが、実体は src/evil.rs。
    std::os::unix::fs::symlink("../src", docs.join("link")).expect("symlink を張れる");
    write_policy(&repo, "docs/\n");
    let out = run_hook(
        "pre-tool-use",
        &tool_payload(&repo, "Write", "docs/link/evil.rs"),
    );
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "repo 内 symlink 経由でも write-set の外へは書かせない"
    );
    assert!(out.stdout.is_empty(), "deny でも stdout は 0 byte");
    // 理由まで測る。root 逸脱の枝で落ちても rc は同じなので、字面で弁別しないと
    // 「実体で allowlist を当てる」ことを一切測らない歯になる。名指すのも実体側である。
    assert_eq!(
        stderr_text(&out).trim_end(),
        NAME.to_owned() + ": deny src/evil.rs は契約 write-set の外（C16）",
        "deny の 1 行は実体側の path を名指す write-set 違反である"
    );
    // 逆向き（実体が write-set の内側）は通る。「symlink を含む path は一律 deny」という
    // 直し方だとここが赤くなる＝実体で解いていることを弁別する対の歯である。
    std::os::unix::fs::symlink("../docs", repo.join("src").join("into-docs"))
        .expect("symlink を張れる");
    let out = run_hook(
        "pre-tool-use",
        &tool_payload(&repo, "Write", "src/into-docs/new.md"),
    );
    assert_silent(&out, "実体が write-set の内側なら symlink 経由でも通す");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_deny_stderr_is_one_line() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/lib.rs\n");
    // 古い lock を置くと記録側が警告を返す。deny の判定文はそれに濁らされない。
    let lock = inject_path(&state).with_extension("jsonl.lock");
    fs::write(&lock, "").expect("lock を置ける");
    let touched = Command::new("touch")
        .args(["-d", "2020-01-01"])
        .arg(&lock)
        .status()
        .expect("touch を起動できる");
    assert!(touched.success(), "lock の mtime を古くできる");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "docs/x.md"));
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "deny は rc 2");
    assert!(out.stdout.is_empty(), "deny でも stdout は 0 byte");
    assert_eq!(
        stderr_lines(&out),
        1,
        "deny の stderr は 1 行（母集団: {}）",
        stderr_text(&out)
    );
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_denies_when_policy_is_empty() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "\n   \n");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "src/lib.rs"));
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "空の policy は不活性でなく deny（fail-closed）"
    );
    // 理由まで測る。空 allowlist は「どの項目にも当たらない」でも deny になるので、
    // 理由を見ないと「policy が読めない」枝を消しても通る歯になる。
    assert!(
        stderr_text(&out).contains("policy unreadable"),
        "理由は policy を読めないことである: {}",
        stderr_text(&out)
    );
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_normalizes_dot_dot_inside_root() {
    let repo = git_repo();
    let state = linked(&repo);
    // allowlist は畳んだ後の 1 本だけを許す。`..` を畳まないと `src/a/lib.rs` になり
    // deny 側へ倒れるので、この歯は正規化そのものを測る。
    write_policy(&repo, "src/lib.rs\n");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "src/a/../lib.rs"));
    assert_silent(&out, "root の内側で閉じる .. は畳んで通す");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_denies_sibling_of_allowed_dir() {
    let repo = git_repo();
    let state = linked(&repo);
    write_policy(&repo, "src/\n");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "src-other/x.rs"));
    assert_eq!(
        out.status.code(),
        Some(i32::from(RC_BROKEN)),
        "末尾 / の項目は配下だけを許す（前方一致で兄弟 dir を拾わない）"
    );
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Edit", "src/a.rs"));
    assert_silent(&out, "配下は通る");
    clean(&[&repo, &state]);
}

#[test]
fn hook_guard_allows_bash_when_policy_unreadable() {
    let repo = git_repo();
    let state = linked(&repo);
    let path = write_policy(&repo, "src/lib.rs\n");
    fs::remove_file(&path).expect("policy を消せる");
    fs::create_dir_all(&path).expect("policy の場所を dir にできる");
    let out = run_hook("pre-tool-use", &tool_payload(&repo, "Bash", "anywhere.rs"));
    assert_silent(&out, "policy が壊れていても Bash は guard の対象でない");
    clean(&[&repo, &state]);
}

/// (a) marker を持つ repo で `git push --force origin main` の Bash → rc 2・stderr 1 行（行 id・語列・次の一手）・
/// stdout 0 byte・`inject.jsonl` に `who=hook:pre-tool-use` / `what=command-deny <語列>` の 1 行（席は無し＝null）。
/// 埋め込み manifest（`--rules` 無し）と fixture の `--rules` の両方で同じ deny＝裁定の値が binary に在る。
#[test]
fn hook_command_guard_denies_a_denied_sequence_from_bash() {
    let repo = git_repo();
    let state = linked(&repo);
    let payload = bash_payload(&repo, "git push --force origin main");

    let before = command_records(&state).len();
    let out = run_hook("pre-tool-use", &payload);
    let text = assert_command_deny(&out, "git push --force", "host_guard.git", "埋め込み manifest の deny");
    assert!(text.contains("N1 / C16") && text.contains("書き直す"), "次の一手を含む: {text}");
    let lines = command_records(&state);
    assert_eq!(lines.len(), before + 1, "記録は 1 行増える: {lines:?}");
    let line = lines.last().cloned().unwrap_or_default();
    assert_eq!(what_of(&line), "command-deny git push --force", "記録の what: {line}");
    assert_eq!(value_of(&line, "who"), Some(json_lite::Value::Str("hook:pre-tool-use".to_owned())), "{line}");
    assert_eq!(value_of(&line, "when"), Some(json_lite::Value::Str("PreToolUse".to_owned())), "{line}");
    assert_eq!(value_of(&line, "seat"), Some(json_lite::Value::Null), "席ではない周の seat は null: {line}");
    assert_eq!(value_of(&line, "bytes"), Some(json_lite::Value::Num(text.len() as u64)), "出した 1 行の byte 数: {line}");

    // fixture の manifest（`--rules`）でも同じ deny（値は行から来る）。
    let rules = state.join("rules.toml");
    fs::write(&rules, role_rules_text(ORCHESTRATOR_CAPS)).expect("rules を書ける");
    let out = run_hook_args(&["pre-tool-use", "--rules", &rules.display().to_string()], &payload);
    assert_command_deny(&out, "git push --force", "host_guard.git", "fixture の manifest の deny");
    // 語列の先頭語が segment の先頭語でない command（`echo git push --force`）は当たらない。
    let out = run_hook("pre-tool-use", &bash_payload(&repo, "echo git push --force"));
    assert_silent(&out, "先頭語が違う segment は当たらない");
    clean(&[&repo, &state]);
}

/// (b) 当たらない Bash（`cargo nextest run -p x`）→ rc 0・0 byte・記録なし（hook budget・write-set guard と同じ沈黙）。
/// marker を持たない repo では当たる command でも 0 byte（FR24・他の repo を汚さない）。
#[test]
fn hook_command_guard_passes_allowed_command_silently() {
    let repo = git_repo();
    let state = linked(&repo);
    let before = inject_lines(&state).len();
    let out = run_hook("pre-tool-use", &bash_payload(&repo, "cargo nextest run -p x"));
    assert_silent(&out, "当たらない command は通す");
    assert_eq!(inject_lines(&state).len(), before, "通す周は記録も残さない");
    let out = run_hook("pre-tool-use", &bash_payload(&repo, "git push origin feat/x"));
    assert_silent(&out, "force の無い push は通す");

    let bare = git_repo();
    let out = run_hook("pre-tool-use", &bash_payload(&bare, "git push --force origin main"));
    assert_silent(&out, "marker の無い repo では仕えない（FR24）");
    clean(&[&repo, &state, &bare]);
}

/// (c) rules が読めない（`--rules` に dir・無い file）→ deny（FailClosed・`reason=rules-unreadable`）／行の無い manifest →
/// deny（`reason=no-row runner.denied_commands`）。当たらない command でも止まる＝禁じる語列を解けない周は通さない。
#[test]
fn hook_command_guard_denies_when_rules_unreadable() {
    let repo = git_repo();
    let state = linked(&repo);
    let payload = bash_payload(&repo, "cargo nextest run -p x");
    let dir = state.join("rules-dir");
    fs::create_dir_all(&dir).expect("dir を作れる");
    for (rules, why) in [(dir.display().to_string(), "dir"), (state.join("nope.toml").display().to_string(), "無い file")] {
        let before = command_records(&state).len();
        let out = run_hook_args(&["pre-tool-use", "--rules", &rules], &payload);
        assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{why}: 読めない rules は deny: {}", stderr_text(&out));
        assert!(out.stdout.is_empty(), "{why}: stdout 0 byte");
        let text = stderr_text(&out);
        assert_eq!(text.lines().count(), 1, "{why}: stderr 1 行: {text}");
        assert!(text.contains("runner.denied_commands") && text.contains("reason=rules-unreadable"), "{why}: {text}");
        let lines = command_records(&state);
        assert_eq!(lines.len(), before + 1, "{why}: 記録 1 行: {lines:?}");
        assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), "command-deny reason=rules-unreadable", "{why}");
    }
    // 行の無い manifest（読めるが `runner.denied_commands` が無い）も deny。
    let rowless = state.join("rowless.toml");
    fs::write(&rowless, format!("schema = 1\n{}", role_rows_text(ORCHESTRATOR_CAPS))).expect("rules を書ける");
    let out = run_hook_args(&["pre-tool-use", "--rules", &rowless.display().to_string()], &payload);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "行の無い manifest は deny: {}", stderr_text(&out));
    assert!(stderr_text(&out).contains("reason=no-row runner.denied_commands"), "{}", stderr_text(&out));
    // Edit は command guard の対象でない（rules が読めなくても write-set guard の判定のまま）。
    let out = run_hook_args(&["pre-tool-use", "--rules", &dir.display().to_string()], &tool_payload(&repo, "Edit", "src/lib.rs"));
    assert_silent(&out, "Edit は command guard を通らない（policy 不在＝write-set guard は不活性）");
    clean(&[&repo, &state]);
}

/// (d) 語列は**順序不問**で当たる: `git push origin main --force` も `git push origin main -f` も deny。連結（`;` / `&&`）
/// の後ろの segment も見る。`--force-with-lease` は語が違う＝当たらない（語の包含であって前方一致ではない）。
#[test]
fn hook_command_guard_matches_sequence_regardless_of_flag_order() {
    let repo = git_repo();
    let state = linked(&repo);
    for (line, sequence, row) in [
        ("git push origin main --force", "git push --force", "host_guard.git"),
        ("git push origin main -f", "git push -f", "host_guard.git"),
        ("cargo build && git push --force origin main", "git push --force", "host_guard.git"),
        ("echo x; git branch -D feat", "git branch -D", "host_guard.git"),
        ("cargo mutants --in-diff x.diff", "cargo mutants", "runner.denied_commands"),
    ] {
        let out = run_hook("pre-tool-use", &bash_payload(&repo, line));
        let text = assert_command_deny(&out, sequence, row, line);
        assert!(text.contains(&format!("deny {sequence} は")), "当たった語列は表の字面: {text}");
    }
    for line in ["git push --force-with-lease origin feat/x", "git branch -d feat", "git stash list"] {
        let out = run_hook("pre-tool-use", &bash_payload(&repo, line));
        assert_silent(&out, line);
    }
    clean(&[&repo, &state]);
}

/// marker の無い tmp repo で `git push --force origin main` を rc 2・stdout 0 byte・stderr 1 行（kind / hit / row / ruling /
/// 代わりの経路の 5 欄）で断り、`inject.jsonl` に what=`host-guard-deny git` の 1 行（who=host-guard・席は null）を残す。
#[test]
fn host_guard_kind_denies_force_push_in_a_repo_without_marker() {
    use vessel::hook::host_guard::Kind;
    let repo = git_repo();
    let state = tmp();
    assert!(!repo.join(MARKER).exists(), "前提: marker が無い");
    let out = run_host_guard_in(&state, &bash_payload(&repo, "git push --force origin main"));
    let text = assert_host_guard_deny(&out, "marker の無い repo");
    let want = format!(
        "{NAME}: host-guard deny kind=git hit=git push --force row=host_guard.git ruling={HOST_GUARD_RULING} — {}",
        Kind::Git.route()
    );
    assert_eq!(text.trim_end(), want, "5 欄の 1 行");
    let lines = host_guard_records(&state);
    assert_eq!(lines.len(), 1, "記録は 1 行: {lines:?}");
    let line = lines.last().cloned().unwrap_or_default();
    assert_eq!(what_of(&line), "host-guard-deny git", "{line}");
    assert_eq!(value_of(&line, "when"), Some(json_lite::Value::Str("PreToolUse".to_owned())), "{line}");
    assert_eq!(value_of(&line, "seat"), Some(json_lite::Value::Null), "席は null: {line}");
    assert_eq!(value_of(&line, "bytes"), Some(json_lite::Value::Num(text.len() as u64)), "出した 1 行の byte 数: {line}");
    clean(&[&repo, &state]);
}

/// 他の name の marker を持つ repo と git repo でない cwd でも同じ deny（FR24 の沈黙は hook の入口だけ）。
#[test]
fn host_guard_kind_denies_the_same_under_other_marker_and_outside_git() {
    let other = git_repo();
    let marker = Marker { name: "other-vessel".to_owned(), version: GENERATION };
    fs::write(other.join(MARKER), marker.render()).expect("marker を書ける");
    let bare = tmp();
    assert!(!bare.join(".git").exists(), "前提: git repo でない");
    for (cwd, why) in [(&other, "他の name の marker"), (&bare, "git repo でない cwd")] {
        let state = tmp();
        let out = run_host_guard_in(&state, &bash_payload(cwd, "git push --force origin main"));
        let text = assert_host_guard_deny(&out, why);
        assert!(text.contains(" kind=git hit=git push --force row=host_guard.git "), "{why}: {text}");
        assert_eq!(host_guard_records(&state).len(), 1, "{why}: 記録 1 行");
        clean(&[&state]);
    }
    clean(&[&other, &bare]);
}

/// `--force-with-lease` の push と、Bash / 編集系でない tool（Read）は 0 byte・rc 0・記録 0。
#[test]
fn host_guard_kind_passes_force_with_lease_and_other_tools_silently() {
    let (repo, origin) = pushable_repo();
    let state = tmp();
    let out = run_host_guard_in(&state, &bash_payload(&repo, "git push --force-with-lease origin feat/x"));
    assert_silent(&out, "--force-with-lease は通す");
    let out = run_host_guard_in(&state, &tool_payload(&repo, "Read", "src/lib.rs"));
    assert_silent(&out, "Read は判定に載らない");
    assert!(inject_lines(&state).is_empty(), "通す周は記録を残さない");
    clean(&[&repo, &origin, &state]);
}

/// bare な origin と main・feat/x の branch を持つ repo（publish が push の行き先を git に解かせられる形・§22 行 n2）と origin の置き場。
fn pushable_repo() -> (TmpDir, TmpDir) {
    let (repo, origin) = (git_repo(), tmp());
    git(&origin, &["init", "-q", "--bare"]);
    git(&repo, &["branch", "-M", "main"]);
    git(&repo, &["branch", "feat/x"]);
    git(&repo, &["remote", "add", "origin", &origin.display().to_string()]);
    (repo, origin)
}

/// payload が JSON でない周は rc 2・stderr 1 行（fail-closed）。
#[test]
fn host_guard_kind_fails_closed_on_a_payload_that_is_not_json() {
    let state = tmp();
    let out = run_host_guard_in(&state, "git push --force");
    assert_host_guard_fail_closed(&state, &out, "payload-unreadable");
    clean(&[&state]);
}

/// payload に `tool_name` が無い周は rc 2・stderr 1 行（fail-closed）。
#[test]
fn host_guard_kind_fails_closed_without_tool_name() {
    let state = tmp();
    let out = run_host_guard_in(&state, &payload(&state));
    assert_host_guard_fail_closed(&state, &out, "no-tool-name");
    clean(&[&state]);
}

/// `Bash` なのに command が無い周は rc 2・stderr 1 行（fail-closed）。
#[test]
fn host_guard_kind_fails_closed_on_bash_without_command() {
    let state = tmp();
    let body = format!("{{\"cwd\":\"{}\",\"tool_name\":\"Bash\",\"tool_input\":{{}}}}", state.display());
    let out = run_host_guard_in(&state, &body);
    assert_host_guard_fail_closed(&state, &out, "no-command");
    clean(&[&state]);
}

/// `--state-dir` が無い周は、当たらない command でも rc 2・stderr 1 行（fail-closed・置き場が無いので記録も無い）。
#[test]
fn host_guard_kind_fails_closed_without_state_dir() {
    let repo = git_repo();
    let out = run_host_guard(&[], &bash_payload(&repo, "ls"));
    let text = assert_host_guard_deny(&out, "--state-dir 無し");
    assert!(text.contains(" kind=- hit=no-state-dir row=- ruling=- — "), "{text}");
    clean(&[&repo]);
}

/// `--rules` が読めない file を指す周は、当たらない command でも rc 2・stderr 1 行（fail-closed）。
#[test]
fn host_guard_kind_fails_closed_on_unreadable_rules() {
    let repo = git_repo();
    let state = tmp();
    let rules = state.join("nope.toml").display().to_string();
    let out = run_host_guard(&["--state-dir", &state.display().to_string(), "--rules", &rules], &bash_payload(&repo, "ls"));
    assert_host_guard_fail_closed(&state, &out, "rules-unreadable");
    clean(&[&repo, &state]);
}

/// tmux の server を壊す語列は kind=tmux・行 id host_guard.tmux で断る。
#[test]
fn host_guard_kind_names_tmux_for_kill_server() {
    let repo = git_repo();
    let state = tmp();
    let out = run_host_guard_in(&state, &bash_payload(&repo, "tmux -L x kill-server"));
    let text = assert_host_guard_deny(&out, "tmux kill-server");
    assert!(text.contains(" kind=tmux hit=tmux kill-server row=host_guard.tmux "), "{text}");
    assert_eq!(what_of(&host_guard_records(&state).last().cloned().unwrap_or_default()), "host-guard-deny tmux");
    clean(&[&repo, &state]);
}

/// `--rules` で host_guard.tmux を `enabled = false` にした周、その行にだけ在る語列を host-guard は通し（git の種類は動く）、
/// 同じ fixture で `hook pre-tool-use` の command guard は断って行 id host_guard.tmux を名指す（enabled を見ない）。
#[test]
fn host_guard_kind_disabled_tmux_row_passes_here_and_the_command_guard_still_denies() {
    let repo = git_repo();
    let state = linked(&repo);
    let rules = state.join("rules.toml");
    fs::write(&rules, format!("schema = 1\n{}", denied_rows_text(false))).expect("rules を書ける");
    let rules = rules.display().to_string();
    let args = ["--state-dir", &state.display().to_string(), "--rules", &rules];
    let out = run_host_guard(&args, &bash_payload(&repo, "tmux kill-server"));
    assert_silent(&out, "切った行の語列は host-guard が通す");
    let out = run_host_guard(&args, &bash_payload(&repo, "git push --force origin main"));
    assert!(assert_host_guard_deny(&out, "他の種類は動く").contains(" kind=git "));
    let out = run_hook_args(&["pre-tool-use", "--rules", &rules], &bash_payload(&repo, "tmux kill-server"));
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "command guard は断る: {}", stderr_text(&out));
    let text = stderr_text(&out);
    assert!(text.starts_with(&format!("{NAME}: deny tmux kill-server は rules 行 host_guard.tmux が禁じる")), "{text}");
    assert_eq!(what_of(&command_records(&state).last().cloned().unwrap_or_default()), "command-deny tmux kill-server");
    clean(&[&repo, &state]);
}

/// host_guard.tmux にだけ在る語列（runner.denied_commands に無い）を `hook pre-tool-use` の command guard が埋め込みの rules で
/// 断り、deny 文は行 id host_guard.tmux を名指す（runner の id を名乗らない）。
#[test]
fn host_guard_kind_command_guard_names_the_tmux_row_from_the_embedded_rules() {
    let repo = git_repo();
    let state = linked(&repo);
    let out = run_hook("pre-tool-use", &bash_payload(&repo, "tmux kill-server"));
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "command guard は断る: {}", stderr_text(&out));
    assert_eq!(stderr_lines(&out), 1, "stderr 1 行");
    let text = stderr_text(&out);
    assert!(text.starts_with(&format!("{NAME}: deny tmux kill-server は rules 行 host_guard.tmux が禁じる")), "{text}");
    assert!(!text.contains("runner.denied_commands"), "{text}");
    assert_eq!(what_of(&command_records(&state).last().cloned().unwrap_or_default()), "command-deny tmux kill-server");
    clean(&[&repo, &state]);
}

/// `rm <tracked>` と `rm -rf <tracked の親 dir>` は kind=rm・hit=repo-tracked:<path> で rc 2。
#[test]
fn host_guard_rm_denies_a_tracked_file_and_its_parent_dir() {
    let repo = git_repo();
    let state = tmp();
    let out = run_host_guard_in(&state, &bash_payload(&repo, "rm src/lib.rs"));
    assert_rm_deny(&state, &out, &format!("repo-tracked:{}", repo.join("src/lib.rs").display()));
    let out = run_host_guard_in(&state, &bash_payload(&repo, "rm -rf src"));
    assert_rm_deny(&state, &out, &format!("repo-tracked:{}", repo.join("src").display()));
    assert_eq!(host_guard_records(&state).len(), 2, "断った周ごとに 1 行");
    clean(&[&repo, &state]);
}

/// `rm <untracked>` と `git rm <tracked>` は 0 byte・rc 0・記録 0 で通る。
#[test]
fn host_guard_rm_passes_an_untracked_file_and_git_rm() {
    let repo = git_repo();
    let state = tmp();
    fs::write(repo.join("notes.txt"), "x\n").expect("untracked を書ける");
    for command in ["rm notes.txt", "git rm src/lib.rs", "rm -f nope.txt"] {
        assert_silent(&run_host_guard_in(&state, &bash_payload(&repo, command)), command);
    }
    assert!(inject_lines(&state).is_empty(), "通す周は記録を残さない");
    clean(&[&repo, &state]);
}

/// state dir そのもの・その配下の host.toml・state dir の親 dir の rm は hit=state-dir で断る。
#[test]
fn host_guard_rm_denies_the_state_dir_its_files_and_its_parent() {
    let bare = tmp();
    let parent = tmp();
    let state = parent.join("state");
    fs::create_dir_all(&state).expect("state dir を作れる");
    fs::write(state.join("host.toml"), "schema = 1\n").expect("host.toml を書ける");
    for path in [state.clone(), state.join("host.toml"), parent.to_path_buf()] {
        let out = run_host_guard_in(&state, &bash_payload(&bare, &format!("rm -rf {}", path.display())));
        assert_rm_deny(&state, &out, &format!("state-dir:{}", path.display()));
    }
    clean(&[&bare, &parent]);
}

/// `rm "$X"`・チルダ始まり・brace・`cd sub && rm x` は path を解かずに hit=unresolved:<語> で断る（home の短縮の字面は
/// paths-clean が数えるので `concat!` で組む）。
#[test]
fn host_guard_rm_denies_unresolved_paths_without_resolving_them() {
    const HOME_X: &str = concat!("~", "/x");
    let repo = git_repo();
    let state = tmp();
    let tilde = format!("rm -rf {HOME_X}");
    for (command, word) in [("rm \"$X\"", "$X"), (&tilde, HOME_X), ("rm {a,b}.txt", "{a,b}.txt"), ("cd sub && rm x", "x")] {
        let out = run_host_guard_in(&state, &bash_payload(&repo, command));
        assert_rm_deny(&state, &out, &format!("unresolved:{word}"));
    }
    clean(&[&repo, &state]);
}

/// 一時 dir の下の `rm *.bak` は通り、repo の root の配下の `rm *.bak` は断る。
#[test]
fn host_guard_rm_glob_passes_under_a_tmp_dir_and_denies_under_the_repo_root() {
    let repo = git_repo();
    let other = tmp();
    let state = tmp();
    fs::write(other.join("a.bak"), "x\n").expect("bak を書ける");
    assert_silent(&run_host_guard_in(&state, &bash_payload(&other, "rm *.bak")), "一時 dir の下の glob");
    let out = run_host_guard_in(&state, &bash_payload(&repo, "rm *.bak"));
    assert_rm_deny(&state, &out, &format!("repo-tracked:{}/*.bak", repo.display()));
    clean(&[&repo, &other, &state]);
}

/// 台帳を持つ tmp repo で `bd update x --notes y` を kind=ledger・hit=notes-replace・行 id ledger.denied_writes で rc 2 に
/// 断り、`inject.jsonl` に what=`host-guard-deny ledger` の 1 行を残す。
#[test]
fn host_guard_ledger_denies_notes_replace_in_a_repo_with_a_ledger() {
    use vessel::hook::host_guard::Kind;
    let repo = git_repo();
    let state = tmp();
    fs::create_dir_all(repo.join(".beads")).expect(".beads を作れる");
    fs::create_dir_all(repo.join("scripts")).expect("scripts を作れる");
    fs::write(repo.join("scripts").join("bdw"), "#!/bin/sh\n").expect("bdw を書ける");
    let out = run_host_guard_in(&state, &bash_payload(&repo, "bd update x --notes y"));
    let text = assert_host_guard_deny(&out, "台帳を持つ repo");
    assert!(text.contains(" kind=ledger hit=notes-replace row=ledger.denied_writes "), "{text}");
    assert!(text.trim_end().ends_with(Kind::Ledger.route()), "代わりの経路: {text}");
    let lines = host_guard_records(&state);
    assert_eq!(lines.len(), 1, "記録は 1 行: {lines:?}");
    assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), "host-guard-deny ledger");
    let out = run_host_guard_in(&state, &bash_payload(&repo, "scripts/bdw update x --append-notes y"));
    assert_silent(&out, "bdw の --append-notes は通す");
    clean(&[&repo, &state]);
}

/// 台帳を持たない repo（`.beads` も `scripts/bdw` も無い）では同じ command を 0 byte・rc 0・記録 0 で通す。
#[test]
fn host_guard_ledger_passes_the_same_command_in_a_repo_without_a_ledger() {
    let repo = git_repo();
    let state = tmp();
    assert!(!repo.join(".beads").exists() && !repo.join("scripts").exists(), "前提: 台帳を持たない");
    let out = run_host_guard_in(&state, &bash_payload(&repo, "bd update x --notes y"));
    assert_silent(&out, "台帳を持たない repo は読まない");
    assert!(inject_lines(&state).is_empty(), "通す周は記録を残さない");
    clean(&[&repo, &state]);
}

/// symlink の口座の settings.json を `Write` で書く payload は kind=self・hit=self:<実体の path>・row=- ruling=- で rc 2（記録の
/// what は `host-guard-deny self`）、同じ dir の別 file は 0 byte・rc 0 で通る。
#[test]
fn host_guard_self_write_to_the_symlinked_account_settings_is_denied() {
    use vessel::hook::host_guard::Kind;
    let (state, shared, real) = self_state();
    let bare = tmp();
    let link = state.join("accounts").join("a").join("settings.json");
    let out = run_host_guard_in(&state, &tool_payload(&bare, "Write", &link.display().to_string()));
    let text = assert_host_guard_deny(&out, "口座の settings.json への Write");
    let want = format!("{NAME}: host-guard deny kind=self hit=self:{} row=- ruling=- — {}", real.display(), Kind::Settings.route());
    assert_eq!(text.trim_end(), want, "5 欄の 1 行");
    let lines = host_guard_records(&state);
    assert_eq!(lines.len(), 1, "記録は 1 行: {lines:?}");
    assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), "host-guard-deny self");
    let other = state.join("accounts").join("a").join("other.json");
    assert_silent(&run_host_guard_in(&state, &tool_payload(&bare, "Write", &other.display().to_string())), "同じ dir の別 file");
    assert_eq!(host_guard_records(&state).len(), 1, "通す周は記録を残さない");
    clean(&[&state, &shared, &bare]);
}

/// `echo x > <口座の settings.json>`・`rm -rf <repo の root>/.claude`・`mv <口座の dir> x` は kind=self で rc 2、口座の
/// settings.json を一時 dir へ写す `cp` と `cat` は 0 byte・rc 0 で通る。
#[test]
fn host_guard_self_bash_writes_removals_and_moves_are_denied_and_copies_out_pass() {
    let (state, shared, real) = self_state();
    let repo = git_repo();
    let spare = tmp();
    fs::create_dir_all(repo.join(".claude")).expect(".claude を作れる");
    fs::write(repo.join(".claude").join("settings.json"), "{}\n").expect("project の設定を書ける");
    let account = state.join("accounts").join("a");
    let link = account.join("settings.json");
    let denied = [
        (format!("echo x > {}", link.display()), real.clone()),
        (format!("rm -rf {}", repo.join(".claude").display()), repo.join(".claude").join("settings.json")),
        (format!("mv {} x", account.display()), real.clone()),
    ];
    for (command, hit) in &denied {
        let out = run_host_guard_in(&state, &bash_payload(&repo, command));
        let text = assert_host_guard_deny(&out, command);
        assert!(text.contains(&format!(" kind=self hit=self:{} row=- ruling=- — ", hit.display())), "{command}: {text}");
    }
    assert_eq!(host_guard_records(&state).len(), denied.len(), "断った周ごとに 1 行");
    for command in [format!("cp {} {}", link.display(), spare.display()), format!("cat {}", link.display())] {
        assert_silent(&run_host_guard_in(&state, &bash_payload(&repo, &command)), &command);
    }
    assert_eq!(host_guard_records(&state).len(), denied.len(), "通す周は記録を残さない");
    clean(&[&state, &shared, &repo, &spare]);
}

/// publish の行を持たない `--rules`（語列の行だけ）で、公開の segment `git push origin main` は rc 2・stdout 0 byte・stderr 1 行
/// （kind=publish・hit=no-row・row=host_guard.publish・ruling=-・publish の経路）で断られ `inject.jsonl` に what=`host-guard-deny
/// publish` の 1 行を残し、同じ `--rules` で公開の segment の無い `ls` と `git status && gh pr list` は rc 0・0 byte（§16 形 5）。
#[test]
fn host_guard_publish_without_the_row_denies_push_and_passes_the_rest() {
    use vessel::hook::host_guard::Kind;
    let repo = git_repo();
    let state = tmp();
    let rules = state.join("rules.toml");
    fs::write(&rules, format!("schema = 1\n{}", denied_rows_text(true))).expect("rules を書ける");
    let rules = rules.display().to_string();
    let args = ["--state-dir", &state.display().to_string(), "--rules", &rules];
    let text = assert_host_guard_deny(&run_host_guard(&args, &bash_payload(&repo, "git push origin main")), "行の無い push");
    let want = format!("{NAME}: host-guard deny kind=publish hit=no-row row=host_guard.publish ruling=- — {}", Kind::Publish.route());
    assert_eq!(text.trim_end(), want, "5 欄の 1 行");
    let lines = host_guard_records(&state);
    assert_eq!(lines.iter().map(|line| what_of(line)).collect::<Vec<_>>(), ["host-guard-deny publish"], "記録 1 行: {lines:?}");
    for command in ["ls", "git status && gh pr list"] {
        assert_silent(&run_host_guard(&args, &bash_payload(&repo, command)), command);
    }
    assert_eq!(host_guard_records(&state).len(), 1, "通す周は記録を残さない");
    clean(&[&repo, &state]);
}

/// 埋め込みの manifest（publish の行は enabled）で、包みの `(git push)` と env の付け替えの push は rc 2・stdout 0 byte・stderr 1 行
/// （hit=unresolved:<印の語>・unresolved の経路）と記録 1 行ずつ、解ける push と cd の後ろの ls は rc 0・記録なし（§17 行 k）。
#[test]
fn publish_marks_are_denied_through_the_binary() {
    let ((repo, origin), state) = (pushable_repo(), tmp());
    let route = "解ける形で書き直す（git / gh を包まずに頭の語に置く・ref と remote と dir と -R と可視性の欄は literal・本文は file〔--body-file か api の -F k=@file〕か区切りを引用した heredoc で渡す）";
    for (at, (command, mark)) in [("(git push)", "wrapped"), ("env GIT_DIR=../p/.git git push origin main", "redirect")].into_iter().enumerate() {
        let text = assert_host_guard_deny(&run_host_guard_in(&state, &bash_payload(&repo, command)), command);
        let want = format!("{NAME}: host-guard deny kind=publish hit=unresolved:{mark} row=host_guard.publish ruling=user 2026-09-27T23:55Z — {route}");
        assert_eq!(text.trim_end(), want, "{command}");
        let lines = host_guard_records(&state);
        assert_eq!(lines.len(), at + 1, "記録 1 行ずつ: {lines:?}");
        assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), "host-guard-deny publish", "{command}");
    }
    for command in ["git push origin main", "cd \"$D\" && ls"] {
        assert_silent(&run_host_guard_in(&state, &bash_payload(&repo, command)), command);
    }
    assert_eq!(host_guard_records(&state).len(), 2, "通す周は記録を残さない");
    clean(&[&repo, &origin, &state]);
}

/// 埋め込みの manifest で、remote の無い repo の push は rc 2・stdout 0 byte・stderr 1 行（hit=unresolved:target・unresolved の経路）と
/// 記録 1 行、bare な origin への push は rc 0・記録なし（§22 行 n2）。
#[test]
fn publish_push_target_is_unresolved_without_a_remote_and_silent_with_a_bare_origin() {
    use vessel::hook::host_guard::publish::Reason;
    let (repo, state) = (git_repo(), tmp());
    let text = assert_host_guard_deny(&run_host_guard_in(&state, &bash_payload(&repo, "git push origin main")), "remote の無い repo");
    let want = format!("{NAME}: host-guard deny kind=publish hit=unresolved:target row=host_guard.publish ruling=user 2026-09-27T23:55Z — {}", Reason::Unresolved.parts().1);
    assert_eq!(text.trim_end(), want, "remote の無い push");
    let lines = host_guard_records(&state);
    assert_eq!(lines.iter().map(|line| what_of(line)).collect::<Vec<_>>(), ["host-guard-deny publish"], "記録 1 行: {lines:?}");
    let (pushable, origin) = pushable_repo();
    assert_silent(&run_host_guard_in(&state, &bash_payload(&pushable, "git push origin main")), "bare な origin への push");
    assert_silent(&run_host_guard_in(&state, &bash_payload(&pushable, "git push -q origin feat/x 2>&1")), "-q と redirect を持つ push");
    assert_eq!(host_guard_records(&state).len(), 1, "通す周は記録を残さない");
    clean(&[&repo, &pushable, &origin, &state]);
}

/// 埋め込みの manifest で、UTF-8 でない author の名を持つ commit の push と、無い file を `--body-file` に名指す gh pr create は rc 2・
/// stdout 0 byte・stderr 1 行（hit=unresolved:text・unresolved の経路）と記録 1 行ずつ、普通の author の commit の push は rc 0・記録
/// なし（§22 行 n3）。
#[test]
fn publish_texts_unreadable_author_and_missing_body_file_are_unresolved() {
    use vessel::hook::host_guard::publish::Reason;
    let ((repo, origin), state) = (pushable_repo(), tmp());
    assert_silent(&run_host_guard_in(&state, &bash_payload(&repo, "git push origin feat/x")), "普通の author の commit の push");
    let (tree, parent) = (git(&repo, &["rev-parse", "HEAD^{tree}"]), git(&repo, &["rev-parse", "HEAD"]));
    let object = [b"tree ".as_slice(), tree.as_bytes(), b"\nparent ", parent.as_bytes(), b"\nauthor bad\xffname <a@e.invalid> 0 +0000\ncommitter c <c@e.invalid> 0 +0000\n\nbad author\n"].concat();
    let mut child = Command::new("git").arg("-C").arg(&*repo).args(["hash-object", "-t", "commit", "-w", "--literally", "--stdin"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("git を起動できる");
    child.stdin.take().expect("stdin を開ける").write_all(&object).expect("commit を書ける");
    let made = child.wait_with_output().expect("終了を待てる");
    git(&repo, &["update-ref", "refs/heads/main", String::from_utf8_lossy(&made.stdout).trim()]);
    let want = format!("{NAME}: host-guard deny kind=publish hit=unresolved:text row=host_guard.publish ruling=user 2026-09-27T23:55Z — {}", Reason::Unresolved.parts().1);
    for (at, (dir, command)) in [(&repo, "git push origin main"), (&repo, "gh pr create --title x --body-file missing.md")].into_iter().enumerate() {
        let text = assert_host_guard_deny(&run_host_guard_in(&state, &bash_payload(dir, command)), command);
        assert_eq!(text.trim_end(), want, "{command}");
        let lines = host_guard_records(&state);
        assert_eq!(lines.len(), at + 1, "記録 1 行ずつ: {lines:?}");
        assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), "host-guard-deny publish", "{command}");
    }
    clean(&[&repo, &origin, &state]);
}

/// PATH の先頭に置く偽の command（gh・ssh・git）の置き場（§22 行 n4）。呼出しの引数は `<command>.log` に 1 行ずつ残る。
struct Fakes {
    dir: TmpDir,
}

/// PATH の最初の `command` の実体。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn on_path(command: &str) -> PathBuf {
    let path = std::env::var("PATH").expect("PATH を読める");
    path.split(':').map(|dir| Path::new(dir).join(command)).find(|found| found.is_file()).expect("command が PATH に在る")
}

impl Fakes {
    /// gh は `gh_tail` を走らせ、ssh は github.com への接続を tmp の `bare` へ届け、git は本物へ渡す（3 つとも呼出しを数える）。
    #[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
    fn new(gh_tail: &str, bare: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let (dir, git) = (tmp(), on_path("git").display().to_string());
        let serve = format!("for last; do :; done\nservice=${{last%% *}}\nexec {git} \"${{service#git-}}\" {}", bare.display());
        for (name, tail) in [("gh", gh_tail.to_owned()), ("ssh", serve), ("git", format!("exec {git} \"$@\""))] {
            let path = dir.join(name);
            fs::write(&path, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}.log\n{tail}\n", path.display())).expect("偽の command を書ける");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("偽の command を実行可能にできる");
        }
        Self { dir }
    }

    /// `command` の呼出しの引数（1 回 1 行）。
    fn calls(&self, command: &str) -> Vec<String> {
        fs::read_to_string(self.dir.join(format!("{command}.log"))).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    /// `host-guard` を偽の command を PATH の先頭に置いて撃つ**起こし口**（`GIT_CONFIG_GLOBAL` と `GIT_CONFIG_NOSYSTEM` で利用者の git の設定を切る）。
    #[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
    fn run(&self, state: &Path, extra: &[&str], payload: &str) -> Output {
        let path = format!("{}:{}", self.dir.display(), std::env::var("PATH").unwrap_or_default());
        let mut child = Command::new(bin())
            .arg("host-guard")
            .arg("--state-dir")
            .arg(state)
            .args(extra)
            .env("PATH", path)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_SSH_COMMAND")
            .env_remove("GIT_SSH")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("binary を起動できる");
        child.stdin.as_mut().expect("stdin を開ける").write_all(payload.as_bytes()).expect("payload を書ける");
        child.wait_with_output().expect("終了を待てる")
    }
}

/// origin が github.com の scp 形の URL（偽の ssh が tmp の bare へ届ける）で main の branch を持つ repo と bare。
fn github_repo() -> (TmpDir, TmpDir) {
    let (repo, bare) = (git_repo(), tmp());
    git(&bare, &["init", "-q", "--bare"]);
    git(&repo, &["branch", "-M", "main"]);
    git(&repo, &["remote", "add", "origin", "git@github.com:acme/pub.git"]);
    (repo, bare)
}

/// origin の URL が `url` の git repo（群の anchor に使う）。
fn anchor_repo(url: &str) -> TmpDir {
    let dir = tmp();
    git(&dir, &["init", "-q"]);
    git(&dir, &["remote", "add", "origin", url]);
    dir
}

/// host の面に群 1 つ（anchor の列）を書く。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn put_anchors(state: &Path, anchors: &[&Path]) {
    let list = anchors.iter().map(|dir| format!("\"{}\"", dir.display())).collect::<Vec<_>>().join(", ");
    let body = format!("schema = 1\n\n[[account]]\nlabel = \"a1\"\n\n[[account-group]]\nname = \"Tier1\"\nanchors = [{list}]\naccounts = [\"a1\"]\n");
    fs::write(state.join("host.toml"), body).expect("host の面を書ける");
}

/// publish の行（enabled は引数）と上限の 2 行（締め切りは `deadline` ミリ秒）を持つ `--rules` の file の path。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn publish_rules(state: &Path, enabled: bool, deadline: u64) -> String {
    let int = |id: &str, kind: &str, value: u64| format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"2026-09-29\"\n");
    let row = format!("\n[[rule]]\nid = \"host_guard.publish\"\nkind = \"HostGuardPublish\"\nvalue = [\"form repo-name\"]\nenabled = {enabled}\nruling = \"r\"\nruled_at = \"2026-09-29\"\n");
    let limits = int("host_guard.publish_deadline_ms", "HostGuardPublishDeadlineMs", deadline) + &int("host_guard.publish_read_bytes", "HostGuardPublishReadBytes", 8_388_608);
    let path = state.join("rules.toml");
    fs::write(&path, format!("schema = 1\n{}{row}{limits}", denied_rows_text(true))).expect("rules を書ける");
    path.display().to_string()
}

/// gh が `repo view` に acme/pub を答え、`api graphql` に全ての alias の PUBLIC を答える tail。
const GH_PUBLIC: &str = "case \"$1 $2\" in\n\"repo view\") echo '{\"nameWithOwner\":\"acme/pub\",\"url\":\"https://github.com/acme/pub\"}';;\n*) echo '{\"data\":{\"r0\":{\"visibility\":\"PUBLIC\"},\"r1\":{\"visibility\":\"PUBLIC\"}}}';;\nesac";

/// 偽の gh と偽の ssh（PATH の先頭）・利用者の git の設定を切った起こし口で、github.com の origin への push は、群の無い host で gh 0 回・anchor が
/// 対象と public の repo（と origin を持たない anchor）の host で gh 1 回（引数の repo の識別は owner/name だけ）で rc 0・記録なし、全履歴の
/// command と `enabled = false` の周と `git status && ls` の周は gh 0 回（`git status && ls` は偽の git も 0 回・AC50 の公開でない周）（§22 行 n4）。
#[test]
fn publish_visibility_asks_github_once_only_beside_an_outside_anchor() {
    let ((repo, bare), (state, open)) = (github_repo(), (tmp(), anchor_repo("git@github.com:acme/open.git")));
    let fakes = Fakes::new(GH_PUBLIC, &bare);
    let push = bash_payload(&repo, "git push origin main");
    assert_silent(&fakes.run(&state, &[], &push), "群の無い host の push");
    assert!(fakes.calls("gh").is_empty(), "群の無い host は gh を撃たない: {:?}", fakes.calls("gh"));
    let lonely = state.join("lonely-anchor");
    fs::create_dir_all(&lonely).expect("anchor の dir を作れる");
    git(&lonely, &["init", "-q"]);
    put_anchors(&state, &[&repo, &open, &lonely]);
    assert_silent(&fakes.run(&state, &[], &push), "anchor が対象と public の repo の host の push");
    let query = "query{r0:repository(owner:\"acme\",name:\"pub\"){visibility} r1:repository(owner:\"acme\",name:\"open\"){visibility}}";
    assert_eq!(fakes.calls("gh"), [format!("api graphql --hostname github.com -f query={query}")], "gh は 1 回・repo の識別は owner/name だけ");
    assert!(host_guard_records(&state).is_empty(), "通す周は記録を残さない");
    let quiet = |command: &str, extra: &[&str]| {
        let fakes = Fakes::new(GH_PUBLIC, &bare);
        let out = fakes.run(&state, extra, &bash_payload(&repo, command));
        (out, fakes.calls("gh"), fakes.calls("git"))
    };
    let (out, gh, _) = quiet("gh repo edit acme/pub --visibility public", &[]);
    assert!(assert_host_guard_deny(&out, "全履歴").contains("hit=full-history:repo-edit-public "), "全履歴の断り");
    assert!(gh.is_empty(), "全履歴の command は gh を撃たない: {gh:?}");
    let off = publish_rules(&state, false, 6000);
    let (out, gh, _) = quiet("git push origin main", &["--rules", &off]);
    assert_silent(&out, "enabled = false の行");
    assert!(gh.is_empty(), "enabled = false は gh を撃たない: {gh:?}");
    let (out, gh, git_calls) = quiet("git status && ls", &[]);
    assert_silent(&out, "公開の segment の無い周");
    assert!((gh.is_empty(), git_calls.is_empty()) == (true, true), "偽の gh {gh:?} と偽の git {git_calls:?} は 0 回");
    clean(&[&repo, &bare, &state, &open]);
}

/// `--rules` の締め切り 1500 ms で、30 秒眠る偽の gh・1 回ごとは内（1 秒）で合計が越える偽の gh（repo view と graphql）・群の無い host の gh の本文の動詞
/// （`gh issue comment`）と締め切りを越えて返る偽の gh の 3 本が rc 2・stdout 0 byte・stderr 1 行（hit=deadline:host_guard.publish_deadline_ms・締め切りの
/// 行の裁定 id と経路）と 2.5 秒の内、記録 1 行ずつ（§22 行 n4）。
#[test]
fn publish_visibility_a_slow_gh_is_stopped_by_the_deadline_within_two_and_a_half_seconds() {
    use vessel::hook::host_guard::publish::Reason;
    let ((repo, bare), (state, open)) = (github_repo(), (tmp(), anchor_repo("git@github.com:acme/open.git")));
    let rules = publish_rules(&state, true, 1500);
    let want = format!(
        "{NAME}: host-guard deny kind=publish hit=deadline:host_guard.publish_deadline_ms row=host_guard.publish_deadline_ms ruling=r — {}",
        Reason::Deadline.parts().1
    );
    let view = "case \"$1 $2\" in\n\"repo view\") echo '{\"nameWithOwner\":\"acme/pub\",\"url\":\"https://github.com/acme/pub\"}';;\n*) echo '{}';;\nesac";
    let cases = [
        ("sleep 30".to_owned(), false, "gh pr comment 1 --body hi"), ("sleep 2\necho '{}'".to_owned(), false, "gh issue comment 1 --body hi"),
        (format!("sleep 1\n{view}"), true, "gh pr create --title x --body y"),
    ];
    for (at, (tail, anchored, command)) in cases.into_iter().enumerate() {
        if anchored {
            put_anchors(&state, &[&open]);
        }
        let fakes = Fakes::new(&tail, &bare);
        let started = std::time::Instant::now();
        let out = fakes.run(&state, &["--rules", &rules], &bash_payload(&repo, command));
        let took = started.elapsed();
        assert_eq!(assert_host_guard_deny(&out, command).trim_end(), want, "{command}");
        assert!(took < std::time::Duration::from_millis(2500), "締め切り 1500 ms の内: {command} {took:?}");
        assert_eq!(host_guard_records(&state).len(), at + 1, "記録 1 行ずつ: {command}");
    }
    clean(&[&repo, &bare, &state, &open]);
}

/// 偽の gh の tail: `repo view` は acme/pub、`api graphql` は問いの alias ごとに、名が pub と open-mate の repo は PUBLIC・他は PRIVATE と答える。
const GH_BY_NAME: &str = r#"case "$1 $2" in
"repo view") echo '{"nameWithOwner":"acme/pub","url":"https://github.com/acme/pub"}';;
*) echo "$*" | grep -o 'r[0-9]*:repository([^)]*)' | { printf '{"data":{'; sep=; while read -r line; do case "$line" in *'name:"pub"'*|*'name:"open-mate"'*) seen=PUBLIC;; *) seen=PRIVATE;; esac; printf '%s"%s":{"visibility":"%s"}' "$sep" "${line%%:*}" "$seen"; sep=,; done; echo '}}'; };;
esac"#;

/// `nest` の下の anchor（basename が `name`・origin が github.com の acme/<name>）。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn named_anchor(nest: &Path, name: &str) -> PathBuf {
    let dir = nest.join(name);
    fs::create_dir_all(&dir).expect("anchor の dir を作れる");
    git(&dir, &["init", "-q"]);
    git(&dir, &["remote", "add", "origin", &format!("git@github.com:acme/{name}.git")]);
    dir
}

/// main から枝を切り、出ていく字面の 1 箇所（`place`: message / author / committer / patch〔追加行〕/ path / tag / ref）に `text` を持つ 1 commit を足し、
/// その push の command を返す（`at` は枝の名の通し番号）。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn leak_push(repo: &Path, (at, place, text): (usize, &str, &str)) -> String {
    let branch = if place == "ref" { text.to_owned() } else { format!("leak-{at}") };
    git(repo, &["switch", "-q", "-C", &branch, "main"]);
    let (file, body) = match place {
        "path" => (format!("{text}.txt"), "x\n".to_owned()),
        "patch" => ("note.txt".to_owned(), format!("{text}\n")),
        _ => ("plain.txt".to_owned(), "x\n".to_owned()),
    };
    fs::write(repo.join(&file), body).expect("file を書ける");
    git(repo, &["add", "--", &file]);
    let message = if place == "message" { text } else { "x" };
    let author = if place == "author" { format!("{text} <a@e.invalid>") } else { "e2e <e2e@example.invalid>".to_owned() };
    let committer = format!("user.name={}", if place == "committer" { text } else { "e2e" });
    git(repo, &["-c", &committer, "commit", "-q", "-m", message, "--author", &author]);
    if place == "tag" {
        git(repo, &["tag", "-a", &format!("v{at}"), "-m", text]);
        return format!("git push origin v{at}");
    }
    format!("git push origin {branch}")
}

/// AC50 の隣 2 つ（nbr-alpha・nbr-beta は偽の gh が private と返す）・公開先の repo 自身・public の anchor（open-mate）・除外の行 1 つ（`nbr-alpha guide`）の
/// 面で、隣の名を出ていく字面の 7 箇所と名の 4 形と除外の字句の順を入れ替えた字句と gh の本文の 2 本と可視性を読めない周と GitHub の外の remote と url / pushurl の
/// 割れた remote に持つ 17 本は rc 2（stderr 1 行・hit=identifier:<件数>:<先頭 5 件>・row は publish の行・識別子の経路）、読む上限を越える 1 本は oversize で
/// rc 2、どれも記録 1 行ずつ。名を持たない push・群の無い host の push（gh 0 回）・private の対象・可視性を読めない周・public の anchor の名・除外の字句の中の名・
/// 名を語の中に持つ push の 10 本は rc 0・記録なし（§22 行 n5・AC50 (a)(b)(c)(f)(g)）。
#[test]
fn publish_scan_neighbor_names_are_denied_in_every_place_and_the_rest_pass() {
    use vessel::hook::host_guard::publish::Reason;
    let ((repo, bare), (state, nest)) = (github_repo(), (tmp(), tmp()));
    let [alpha, beta, mate] = ["nbr-alpha", "nbr-beta", "open-mate"].map(|name| named_anchor(&nest, name));
    put_anchors(&state, &[&repo, &alpha, &beta, &mate]);
    let face = state.join("host.toml");
    let body = fs::read_to_string(&face).expect("面を読める") + "\n[[publish-exclusion]]\nphrase = \"nbr-alpha guide\"\nruling = \"user 2026-09-29T05:46Z\"\n";
    fs::write(&face, body).expect("除外の行を足せる");
    let (named, unread) = (Fakes::new(GH_BY_NAME, &bare), Fakes::new("echo '{}'", &bare));
    let deny = |out: &Output, hit: &str, word: &str| {
        let want = format!("{NAME}: host-guard deny kind=publish hit={hit}:{word} row=host_guard.publish ruling=user 2026-09-27T23:55Z — {}", Reason::Identifier.parts().1);
        assert_eq!(assert_host_guard_deny(out, word).trim_end(), want, "{word}");
    };
    let pushes = [
        ("message", "see nbr-alpha", "nbr-alpha"), ("author", "nbr-alpha", "nbr-alpha"), ("committer", "nbr-alpha", "nbr-alpha"), ("patch", "see nbr-alpha", "nbr-alpha"),
        ("path", "nbr-alpha", "nbr-alpha"), ("tag", "nbr-alpha", "nbr-alpha"), ("ref", "nbr-alpha-fix", "nbr-alpha"), ("message", "nbr-alpha-next", "nbr-alpha"),
        ("message", "NBR-ALPHA", "NBR-ALPHA"), ("message", "acme/nbr-alpha", "nbr-alpha"), ("message", "nbr.alpha", "nbr.alpha"), ("message", "guide nbr-alpha", "nbr-alpha"),
    ];
    for (at, (place, text, word)) in pushes.into_iter().enumerate() {
        let command = leak_push(&repo, (at, place, text));
        deny(&named.run(&state, &[], &bash_payload(&repo, &command)), "identifier", &format!("1:repo-name={word}@nbr-alpha"));
    }
    let note = nest.join("note.md");
    fs::write(&note, "see nbr-alpha\n").expect("本文の file を書ける");
    for command in ["gh pr comment 1 --body \"see nbr-alpha\"".to_owned(), format!("gh pr comment 1 --body-file {}", note.display())] {
        deny(&named.run(&state, &[], &bash_payload(&repo, &command)), "identifier", "1:repo-name=nbr-alpha@nbr-alpha");
    }
    let leak = |dir: &Path, at: usize| bash_payload(dir, &leak_push(dir, (at, "message", "see nbr-alpha")));
    deny(&unread.run(&state, &[], &leak(&repo, 20)), "identifier", "1:repo-name=nbr-alpha@nbr-alpha");
    let (outside, origin) = pushable_repo();
    deny(&named.run(&state, &[], &leak(&outside, 21)), "identifier", "1:repo-name=nbr-alpha@nbr-alpha");
    git(&repo, &["remote", "set-url", "origin", "git@github.com:acme/priv.git"]);
    git(&repo, &["config", "remote.origin.pushurl", "git@github.com:acme/pub.git"]);
    deny(&named.run(&state, &[], &leak(&repo, 22)), "identifier", "1:repo-name=nbr-alpha@nbr-alpha");
    git(&repo, &["config", "--unset", "remote.origin.pushurl"]);
    git(&repo, &["remote", "set-url", "origin", "git@github.com:acme/pub.git"]);
    assert_eq!(host_guard_records(&state).len(), 17, "断りは記録 1 行ずつ");
    let rules = publish_rules(&state, true, 6000);
    fs::write(&rules, fs::read_to_string(&rules).expect("rules を読める").replace("8388608", "3000")).expect("読む上限を縮められる");
    let big = leak_push(&repo, (30, "patch", &"y".repeat(50_000)));
    let text = assert_host_guard_deny(&named.run(&state, &["--rules", &rules], &bash_payload(&repo, &big)), "上限越え");
    let want = format!("{NAME}: host-guard deny kind=publish hit=oversize:host_guard.publish_read_bytes row=host_guard.publish_read_bytes ruling=r — {}", Reason::Oversize.parts().1);
    assert_eq!(text.trim_end(), want, "読む上限を越える字面は照合せず断る");
    assert_eq!(host_guard_records(&state).len(), 18, "上限越えも記録 1 行");
    let private = Fakes::new(&GH_BY_NAME.replace("*'name:\"pub\"'*|", ""), &bare);
    let (bare_state, quiet) = (tmp(), Fakes::new(GH_BY_NAME, &bare));
    let passes = [
        (&named, &state, "nothing to see"), (&quiet, &bare_state, "see nbr-alpha"), (&private, &state, "see nbr-alpha"), (&unread, &state, "nothing to see"),
        (&unread, &state, "about pub"), (&named, &state, "with open-mate"), (&named, &state, "the nbr-alpha guide"), (&named, &state, "THE NBR-ALPHA GUIDE"),
        (&named, &state, "nbr-alphabet"), (&named, &state, "nbr-alpha_x"),
    ];
    for (at, (fakes, dir, text)) in passes.into_iter().enumerate() {
        let command = leak_push(&repo, (40 + at, "message", text));
        assert_silent(&fakes.run(dir, &[], &bash_payload(&repo, &command)), text);
    }
    assert!(quiet.calls("gh").is_empty(), "群の無い host は gh を撃たない: {:?}", quiet.calls("gh"));
    assert_eq!(host_guard_records(&state).len(), 18, "通す周は記録を残さない");
    clean(&[&repo, &bare, &state, &nest, &outside, &origin, &bare_state]);
}

/// park の区画（Tier9）だけが隣の private repo を anchor に持つ面で、その repo の名を持つ git push は rc 2（hit=identifier:1:repo-name=nbr-alpha@nbr-alpha）で断られ、
/// 名を持たない push は rc 0（§22 行 n5・AC63 (e)）。
#[test]
fn publish_scan_a_neighbor_only_in_the_park_lot_is_named_by_its_push() {
    use vessel::hook::host_guard::publish::Reason;
    let ((repo, bare), (state, nest)) = (github_repo(), (tmp(), tmp()));
    let lot = named_anchor(&nest, "nbr-alpha");
    let face = format!("schema = 1\n\n[[account]]\nlabel = \"a1\"\n\n[[account-group]]\nname = \"Tier9\"\nanchors = [\"{}\"]\naccounts = [\"a1\"]\n", lot.display());
    fs::write(state.join("host.toml"), face).expect("host の面を書ける");
    let fakes = Fakes::new(GH_BY_NAME, &bare);
    let payload = |at: usize, text: &str| bash_payload(&repo, &leak_push(&repo, (at, "message", text)));
    let text = assert_host_guard_deny(&fakes.run(&state, &[], &payload(0, "see nbr-alpha")), "park の区画の隣");
    let want = format!("{NAME}: host-guard deny kind=publish hit=identifier:1:repo-name=nbr-alpha@nbr-alpha row=host_guard.publish ruling=user 2026-09-27T23:55Z — {}", Reason::Identifier.parts().1);
    assert_eq!(text.trim_end(), want);
    assert_silent(&fakes.run(&state, &[], &payload(1, "nothing to see")), "名を持たない push");
    assert_eq!(host_guard_records(&state).len(), 1, "断りだけが記録 1 行");
    clean(&[&repo, &bare, &state, &nest]);
}

/// 埋め込みの manifest と群を宣言しない置き場（host.toml 無し）で、AC50 (e) の解けない形 4 つは rc 2・stdout 0 byte・stderr 1 行
/// （hit=unresolved:<形の語>・unresolved の経路）と記録 1 行ずつ、区切りを引用した heredoc の本文の gh pr create は rc 0・記録なし
/// （§17 行 k2・偽の gh / git の回数は数えない）。
#[test]
fn publish_unresolved_forms_are_denied_before_any_scan() {
    let (repo, state) = (git_repo(), tmp());
    assert!(!state.join("host.toml").exists(), "群を宣言しない置き場");
    let route = "解ける形で書き直す（git / gh を包まずに頭の語に置く・ref と remote と dir と -R と可視性の欄は literal・本文は file〔--body-file か api の -F k=@file〕か区切りを引用した heredoc で渡す）";
    let forms = [
        ("git push origin $B", "variable-ref"), ("git push --mirror", "mirror"), ("gh pr create --body-file -", "stdin-body"),
        ("gh api -X PATCH repos/o/n -F visibility=@v", "api-visibility-file"),
    ];
    for (at, (command, form)) in forms.into_iter().enumerate() {
        let text = assert_host_guard_deny(&run_host_guard_in(&state, &bash_payload(&repo, command)), command);
        let want = format!("{NAME}: host-guard deny kind=publish hit=unresolved:{form} row=host_guard.publish ruling=user 2026-09-27T23:55Z — {route}");
        assert_eq!(text.trim_end(), want, "{command}");
        let lines = host_guard_records(&state);
        assert_eq!(lines.len(), at + 1, "記録 1 行ずつ: {lines:?}");
        assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), "host-guard-deny publish", "{command}");
    }
    let body = "gh pr create --title x --body \"$(cat <<'EOF'\n## 要約\n$HOME も字\nEOF\n)\"";
    let fakes = Fakes::new("exit 1", &state);
    assert_silent(&fakes.run(&state, &[], &bash_payload(&repo, body)), "区切りを引用した heredoc の本文");
    assert_eq!(host_guard_records(&state).len(), forms.len(), "通す周は記録を残さない");
    clean(&[&repo, &state]);
}

/// 埋め込みの manifest で、広げた読みの形（graphql の変数の query・`--mirror` の接頭辞・gist edit の標準入力）は rc 2・stdout 0 byte・
/// stderr 1 行（hit=unresolved:<形の語>・unresolved の経路）と記録 1 行ずつ、読みの graphql は rc 0・記録なし（§17 行 k3）。
#[test]
fn publish_widened_forms_are_denied_through_the_binary() {
    let (repo, state) = (git_repo(), tmp());
    let route = "解ける形で書き直す（git / gh を包まずに頭の語に置く・ref と remote と dir と -R と可視性の欄は literal・本文は file〔--body-file か api の -F k=@file〕か区切りを引用した heredoc で渡す）";
    let forms = [("gh api graphql -f query=\"$Q\"", "unreadable-body"), ("git push --m origin", "mirror"), ("gh gist edit abc -", "stdin-body")];
    for (at, (command, form)) in forms.into_iter().enumerate() {
        let text = assert_host_guard_deny(&run_host_guard_in(&state, &bash_payload(&repo, command)), command);
        let want = format!("{NAME}: host-guard deny kind=publish hit=unresolved:{form} row=host_guard.publish ruling=user 2026-09-27T23:55Z — {route}");
        assert_eq!(text.trim_end(), want, "{command}");
        let lines = host_guard_records(&state);
        assert_eq!(lines.len(), at + 1, "記録 1 行ずつ: {lines:?}");
        assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), "host-guard-deny publish", "{command}");
    }
    let read = "gh api graphql -f query={viewer{login}}";
    assert_silent(&run_host_guard_in(&state, &bash_payload(&repo, read)), read);
    assert_eq!(host_guard_records(&state).len(), forms.len(), "通す周は記録を残さない");
    clean(&[&repo, &state]);
}

/// 埋め込みの manifest と群を宣言しない置き場（host.toml 無し）で、AC50 (d) の全履歴の 3 形は rc 2・stdout 0 byte・stderr 1 行
/// （hit=full-history:<種別>・埋め込みの行の ruling・全履歴の経路）と記録 1 行ずつ、`gh repo edit o/n --visibility private` は rc 0・
/// 記録なし（§18 行 l・偽の gh の回数は数えない）。
#[test]
fn publish_history_commands_are_denied_through_the_binary() {
    let (repo, state) = (git_repo(), tmp());
    assert!(!state.join("host.toml").exists(), "群を宣言しない置き場");
    let route = "持ち主が手で行う（可視性を public へ変える command と public の repo を作る command は repo の全履歴を走査せずに出すので席の session からは撃たない）";
    let forms = [
        ("gh repo edit o/n --visibility public", "repo-edit-public"), ("gh repo create x --public", "repo-create-public"),
        ("gh api -X PATCH repos/o/n -f visibility=public", "api-visibility-public"),
    ];
    for (at, (command, kind)) in forms.into_iter().enumerate() {
        let text = assert_host_guard_deny(&run_host_guard_in(&state, &bash_payload(&repo, command)), command);
        let want = format!("{NAME}: host-guard deny kind=publish hit=full-history:{kind} row=host_guard.publish ruling=user 2026-09-27T23:55Z — {route}");
        assert_eq!(text.trim_end(), want, "{command}");
        let lines = host_guard_records(&state);
        assert_eq!(lines.len(), at + 1, "記録 1 行ずつ: {lines:?}");
        assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), "host-guard-deny publish", "{command}");
    }
    let private = "gh repo edit o/n --visibility private";
    assert_silent(&run_host_guard_in(&state, &bash_payload(&repo, private)), private);
    assert_eq!(host_guard_records(&state).len(), forms.len(), "通す周は記録を残さない");
    clean(&[&repo, &state]);
}

/// 埋め込みの manifest で、ruling の空の行を持つ host の面と ruling の欄の無い行を持つ host の面では、識別子を持たない
/// `git push origin main` が rc 2・stdout 0 byte・stderr 1 行（`hit=host-unreadable:<行番号>` と「<行番号> 行目を直す」）と記録 1 行
/// （what は行番号を持たない `host-guard-deny reason=host-unreadable`）で断られ、裁定 id を書いた面では同じ push が rc 0（断りの理由が
/// 行であることの対）。`exclude` の要素を持つ publish の行の `--rules` では rules-unreadable で断られ、要素を消すと通る（§19 行 m）。
#[test]
fn publish_exclusion_host_face_rows_gate_even_an_identifier_free_push() {
    let ((repo, origin), state) = (pushable_repo(), tmp());
    let push = bash_payload(&repo, "git push origin main");
    let face = |row: &str| fs::write(state.join("host.toml"), format!("schema = 1\n\n[[publish-exclusion]]\n{row}")).expect("host の面を書ける");
    for (at, (row, line)) in [("phrase = \"a\"\nruling = \"\"\n", 5), ("phrase = \"a\"\n", 3)].into_iter().enumerate() {
        face(row);
        let text = assert_host_guard_deny(&run_host_guard_in(&state, &push), row);
        let want = format!("{NAME}: host-guard deny kind=- hit=host-unreadable:{line} row=- ruling=- — 読めない周は通さない（fail-closed）: host の面（host.toml）の {line} 行目を直す");
        assert_eq!(text.trim_end(), want, "{row}");
        let lines = host_guard_records(&state);
        assert_eq!(lines.len(), at + 1, "記録 1 行ずつ: {lines:?}");
        assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), "host-guard-deny reason=host-unreadable", "{row}");
    }
    face("phrase = \"a\"\nruling = \"user 2026-09-29T05:46Z\"\n");
    assert_silent(&run_host_guard_in(&state, &push), "裁定 id を書いた面");
    assert_eq!(host_guard_records(&state).len(), 2, "通す周は記録を残さない");
    let rules = state.join("rules.toml");
    let rules_with = |value: &str| {
        let row = format!("\n[[rule]]\nid = \"host_guard.publish\"\nkind = \"HostGuardPublish\"\nvalue = [{value}]\nenabled = true\nruling = \"r\"\nruled_at = \"2026-09-29\"\n");
        let int = |id: &str, kind: &str, value: u64| format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"2026-09-29\"\n");
        let limits = int("host_guard.publish_deadline_ms", "HostGuardPublishDeadlineMs", 6000) + &int("host_guard.publish_read_bytes", "HostGuardPublishReadBytes", 8_388_608);
        fs::write(&rules, format!("schema = 1\n{}{row}{limits}", denied_rows_text(true))).expect("rules を書ける");
    };
    let args = ["--state-dir", &state.display().to_string(), "--rules", &rules.display().to_string()];
    rules_with("\"form repo-name\", \"exclude 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"");
    let text = assert_host_guard_deny(&run_host_guard(&args, &push), "exclude の要素");
    assert!(text.contains("hit=rules-unreadable "), "{text}");
    rules_with("\"form repo-name\"");
    assert_silent(&run_host_guard(&args, &push), "要素を消した manifest");
    clean(&[&repo, &origin, &state]);
}

/// (a) `[memo]` の title か `intake:memo` の label を持つ create は、body-file の本文に 4 節が全部在れば通り、1 つでも
/// 欠ければ閉じた理由 1 つ（宣言順で最初の欠け）で止まる。相対 path は payload の `cwd` から解き、`scripts/bdw` も
/// 連結の後ろの segment も読む。
#[test]
fn hook_memo_guard_requires_every_memo_section_in_the_body_file() {
    let repo = git_repo();
    let state = linked(&repo);
    fs::write(repo.join("memo.md"), MEMO_BODY).expect("本文を書ける");
    let full = state.join("full.md");
    fs::write(&full, MEMO_BODY).expect("本文を書ける");
    let full = full.display().to_string();
    for command in [
        "bdw create \"[memo] 観測の件\" --type=task --parent s2-1 --body-file memo.md".to_owned(),
        format!("scripts/bdw create --title=x --parent=s2-1 --labels=doc:toy,intake:memo --body-file {full}"),
        format!("cd . && bdw create --title \"[memo] y\" --parent s2-1 -l intake:memo --body-file={full}"),
    ] {
        assert_ledger_pass(&state, &repo, &command);
    }
    for (drop, reason) in [("### 出所\n", "no-source"), ("### 観測\n", "no-observation"), ("### 候補\n", "no-candidate"), ("### 昇格条件\n", "no-promotion")] {
        let body = state.join(format!("{reason}.md"));
        fs::write(&body, MEMO_BODY.replace(drop, "")).expect("本文を書ける");
        let body = body.display().to_string();
        let text = assert_ledger_deny(&state, &repo, &format!("bd create \"[memo] x\" --body-file {body}"), reason);
        assert!(text.contains(drop.trim_end()), "欠けた見出しを名指す: {text}");
        assert_ledger_deny(&state, &repo, &format!("bdw create --title=x --labels intake:memo --body-file {body}"), reason);
    }
    // 2 つ欠けても理由は 1 つ（宣言順で最初の欠け）。
    let two = state.join("two.md");
    fs::write(&two, "### 出所\n### 昇格条件\n").expect("本文を書ける");
    let text = assert_ledger_deny(&state, &repo, &format!("bd create '[memo] x' --body-file {}", two.display()), "no-observation");
    assert_eq!(text.matches("reason=").count(), 1, "理由は 1 つ: {text}");
    clean(&[&repo, &state]);
}

/// (b) acceptance に設計 pointer 行を持つ create は label `intake:memo` を持てば止まり（本文が揃っていても）、label が
/// 無ければ通る（契約の create）。
#[test]
fn hook_memo_guard_denies_memo_label_on_a_contract_create() {
    let repo = git_repo();
    let state = linked(&repo);
    fs::write(repo.join("memo.md"), MEMO_BODY).expect("本文を書ける");
    let text = assert_ledger_deny(
        &state,
        &repo,
        "bd create --title=c --acceptance \"design = docs/design/toy.md#a\" --labels intake:memo --body-file memo.md",
        "memo-on-contract",
    );
    assert!(text.contains("intake:memo"), "label を名指す: {text}");
    assert_ledger_deny(&state, &repo, "bdw create c --labels=intake:memo --acceptance=\"x\ndesign = docs/design/toy.md#a\"", "memo-on-contract");
    assert_ledger_pass(&state, &repo, "bdw create --title=c --parent s2-1 --acceptance \"design = docs/design/toy.md#a\" --labels doc:toy");
    clean(&[&repo, &state]);
}

/// (c) memo の create で body-file が無い・開けない（無い file・dir・展開されない変数）周は止まる（fail-closed）。
#[test]
fn hook_memo_guard_fails_closed_without_a_readable_body_file() {
    let repo = git_repo();
    let state = linked(&repo);
    assert_ledger_deny(&state, &repo, "bd create \"[memo] x\" --description \"### 出所\"", "no-body-file");
    assert_ledger_deny(&state, &repo, "bdw create --title=x --labels intake:memo", "no-body-file");
    let dir = state.join("body-dir");
    fs::create_dir_all(&dir).expect("dir を作れる");
    for path in [state.join("nope.md").display().to_string(), dir.display().to_string(), "$BODY".to_owned()] {
        assert_ledger_deny(&state, &repo, &format!("bd create '[memo] x' --body-file {path}"), "body-unreadable");
    }
    clean(&[&repo, &state]);
}

/// (d) memo でも契約でもない create（epic・裁定）と create 以外の bd の command は 1 byte も書かず通る（記録も増えない）。
/// title の `[memo]` が create 以外の subcommand に在っても判定に載らない。
#[test]
fn hook_memo_guard_passes_non_memo_creates_and_other_bd_commands() {
    let repo = git_repo();
    let state = linked(&repo);
    for command in [
        "bdw create \"program\" --type feature --parent s2-1",
        "bdw create --title=\"裁定 x\" --type=decision --parent=s2-1 --body-file nope.md",
        "bdw update s2-1 --title \"[memo] x\" --add-label intake:memo",
        "bd list --label intake:memo --json",
        "scripts/bdw update s2-1 --append-notes \"### 出所\"",
        "echo bd create \"[memo] x\"",
    ] {
        assert_ledger_pass(&state, &repo, command);
    }
    clean(&[&repo, &state]);
}

/// (a) 4 形が断られる: bd と bdw のどちらでも `--notes` の両形・記憶の 3 語・`--parent` の無い create・`bd` の書き込み。
#[test]
fn hook_ledger_write_denies_each_form() {
    let repo = git_repo();
    let state = linked(&repo);
    for command in ["bd update s2-1 --notes x", "bdw update s2-1 --notes=x", "scripts/bdw update s2-1 --notes \"a b\"", "bd update s2-1 --notes=x"] {
        assert_write_denied(&state, &repo, command, "notes-replace");
    }
    for command in ["bdw remember x", "bdw recall x", "bdw memories", "bd remember x", "bd recall x", "bd memories"] {
        assert_write_denied(&state, &repo, command, "memory-subcommand");
    }
    for command in ["bdw create \"x\" --type task", "cd . && bdw create --title=x", "bd create \"program\" --type epic"] {
        assert_write_denied(&state, &repo, command, "create-without-parent");
    }
    for command in ["bd update s2-1 --status open", "bd close s2-1", "ls; /usr/bin/bd dep add a b", "bd create x --parent=s2-1"] {
        assert_write_denied(&state, &repo, command, "bd-outside-bdw");
    }
    clean(&[&repo, &state]);
}

/// (b) 当たらない例が形ごとに通る（rc 0・0 byte・記録なし）: `--append-notes`・`bdw` の書き込み・`--parent` を持つ create・
/// 読みの subcommand。
#[test]
fn hook_ledger_write_passes_the_near_misses() {
    let repo = git_repo();
    let state = linked(&repo);
    for command in [
        "bdw update s2-1 --append-notes x",
        "scripts/bdw update s2-1 --append-notes=\"### 出所\"",
        "bdw close s2-1 --reason x",
        "bdw update s2-1 --status open",
        "bdw create x --type task --parent s2-1",
        "bdw create --title=x --parent=s2-1",
        "bd list --json",
        "bd show s2-1",
        "bd ready",
        "echo bd update s2-1 --notes x",
    ] {
        assert_ledger_pass(&state, &repo, command);
    }
    clean(&[&repo, &state]);
}

/// 埋め込みの rules で `bdw q x` が create-bypass・`bdw dep add a b --type parent-child` が parent-edge として断られ
/// （rc 2・stdout 0 byte・stderr 1 行・記録 1 行）、断り文は次の一手（create の `--parent`・update の `--parent`）を持つ。
/// 当たらない隣（`bdw todo list`・`bdw dep add a b`・`bdw link a b`）は 0 byte・rc 0・記録 0 で通る。
#[test]
fn hook_ledger_edge_denies_bypass_and_parent_edge_from_bash() {
    let repo = git_repo();
    let state = linked(&repo);
    for (command, reason, next) in [
        ("bdw q x", "create-bypass", "bdw create <題> --parent <epic>"),
        ("bdw dep add a b --type parent-child", "parent-edge", "bdw update <子> --parent <親>"),
    ] {
        let before = ledger_records(&state).len();
        let out = run_hook("pre-tool-use", &bash_payload(&repo, command));
        assert_write_deny(&state, &out, command, reason);
        assert!(stderr_text(&out).contains(next), "{command}: 次の一手: {}", stderr_text(&out));
        assert_eq!(ledger_records(&state).len(), before + 1, "{command}: 記録は 1 行増える");
    }
    for command in ["bdw todo list", "bdw dep add a b", "bdw link a b", "bdw dep remove a b --type parent-child"] {
        assert_ledger_pass(&state, &repo, command);
    }
    clean(&[&repo, &state]);
}

/// (c) rules の行が無い fixture では bd / bdw を断り（`no-row`・FailClosed）、bd / bdw の無い command は通す。壊れた
/// rules は command guard が先に断る（判定の順は動かない）。
#[test]
fn hook_ledger_write_fails_closed_without_the_row() {
    let repo = git_repo();
    let state = linked(&repo);
    let rowless = state.join("rowless.toml");
    fs::write(&rowless, role_rules_text(ORCHESTRATOR_CAPS)).expect("rules を書ける");
    let rowless = rowless.display().to_string();
    for command in ["bdw show s2-1", "bdw create x --parent s2-1"] {
        let before = ledger_records(&state).len();
        let out = run_hook_args(&["pre-tool-use", "--rules", &rowless], &bash_payload(&repo, command));
        assert_write_deny(&state, &out, command, "no-row");
        assert!(stderr_text(&out).contains("ledger.denied_writes"), "{command}: 行 id を名指す");
        assert_eq!(ledger_records(&state).len(), before + 1, "{command}: 記録 1 行");
    }
    let out = run_hook_args(&["pre-tool-use", "--rules", &rowless], &bash_payload(&repo, "cargo nextest run -p x"));
    assert_silent(&out, "bd / bdw の無い command は行が無くても通す");
    let broken = state.join("broken.toml");
    fs::write(&broken, "schema = ").expect("rules を書ける");
    let out = run_hook_args(&["pre-tool-use", "--rules", &broken.display().to_string()], &bash_payload(&repo, "bdw show s2-1"));
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "壊れた rules は deny: {}", stderr_text(&out));
    assert!(stderr_text(&out).contains("reason=rules-unreadable"), "{}", stderr_text(&out));
    clean(&[&repo, &state]);
}

// ─────────────── 台帳の問いの create の形（`s2-07l.738.11`・設計 ledger-form.md §14 行 j・接頭辞 `hook_question_form_`） ───────────────
//
// `.beads` の無い toy repo で撃つ。rules は埋め込みの manifest の写しで `ledger.denied_writes` から bd-outside-bdw を外す
// （`bd create` の揃った問いが 6 形に当たらず通る）。

/// 埋め込みの manifest の字面（`--rules` に渡す写しの元）。
const QUESTION_EMBEDDED: &str = include_str!("../../../../../rules/manifest.toml");

/// 揃った問いの本文（4 行）。
const QUESTION_BODY: &str = "概要 = 何を決めるか\n- 技術: 器の門\n理由：台帳の形\n推奨 = 断る\n";

/// 揃った metadata。
const QUESTION_META: &str = "{\"effect\":\"document\",\"asked\":\"seat\"}";

/// 写しの rules を置き場に書き、path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn question_rules(state: &Path) -> String {
    let text = QUESTION_EMBEDDED.replace("\"bd-outside-bdw\", ", "");
    assert_ne!(text, QUESTION_EMBEDDED, "写しは bd-outside-bdw を外した（字面が manifest と揃っている）");
    let path = state.join("question-rules.toml");
    fs::write(&path, text).expect("rules の写しを書ける");
    path.display().to_string()
}

/// 問いの create の command（label・親の flag・本文の `-d`・metadata の字）。
fn question_create(client: &str, labels: &str, parent: &str, body: &str, meta: &str) -> String {
    format!("{client} create 問い --labels {labels} {parent} -d '{body}' --metadata '{meta}'")
}

/// 写しの rules で撃ち、問いの段の deny の外形（rc 2・stdout 0 byte・stderr 1 行・語と §14）と記録 1 行を確かめ、stderr を返す。
fn assert_question_deny(state: &Path, repo: &Path, rules: &str, command: &str, reason: &str) -> String {
    let before = ledger_records(state).len();
    let out = run_hook_args(&["pre-tool-use", "--rules", rules], &bash_payload(repo, command));
    let text = stderr_text(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{command}: deny は rc 2: {text}");
    assert!(out.stdout.is_empty(), "{command}: deny でも stdout は 0 byte");
    assert_eq!(stderr_lines(&out), 1, "{command}: stderr は 1 行: {text}");
    let head = format!("{NAME}: deny bd create は起票の門が止める reason={reason}（");
    assert!(text.starts_with(&head) && text.contains("・ledger-form.md §14）"), "{command}: {text}");
    let lines = ledger_records(state);
    assert_eq!(lines.len(), before + 1, "{command}: 記録は 1 行増える: {lines:?}");
    assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), format!("ledger-deny {reason}"), "{command}");
    text
}

/// 写しの rules で撃ち、rc 0・0 byte・記録なしで通ることを確かめる。
fn assert_question_pass(state: &Path, repo: &Path, rules: &str, command: &str) {
    let before = inject_lines(state).len();
    assert_silent(&run_hook_args(&["pre-tool-use", "--rules", rules], &bash_payload(repo, command)), command);
    assert_eq!(inject_lines(state).len(), before, "{command}: 通す周は記録を残さない");
}

/// (a) 10 形（4 行の欠け 4・effect の欠けと値の外・asked の欠けと値の外・intake:memo の併せ持ち・継がない指定の欠け）×
/// bd と bdw の 20 本がどれも断られ、断り文は欠けた行・外れた値・併せ持つ label・継ぐ親の id を名指す。(c) label
/// intake:question を持たない create と (d) 問いの create を中で撃つ script を起こす command は問いの語で断られない。
#[test]
fn hook_question_form_denies_each_missing_part_from_bd_and_bdw() {
    let repo = git_repo();
    let state = linked(&repo);
    let rules = question_rules(&state);
    let (question, keep) = ("intake:question", "--parent s2-1 --no-inherit-labels");
    let without = |word: &str| QUESTION_BODY.lines().filter(|line| !line.contains(word)).collect::<Vec<_>>().join("\n");
    let forms = [
        ("question-no-summary", question, keep, without("概要"), QUESTION_META, "本文に 概要 の行が無い"),
        ("question-no-technical", question, keep, without("技術"), QUESTION_META, "本文に 技術 の行が無い"),
        ("question-no-reason", question, keep, without("理由"), QUESTION_META, "本文に 理由 の行が無い"),
        ("question-no-recommendation", question, keep, without("推奨"), QUESTION_META, "本文に 推奨 の行が無い"),
        ("question-no-effect", question, keep, QUESTION_BODY.to_owned(), "{\"asked\":\"seat\"}", "metadata に effect が無い"),
        ("question-bad-effect", question, keep, QUESTION_BODY.to_owned(), "{\"effect\":\"docs\",\"asked\":\"seat\"}", "effect が docs"),
        ("question-no-asked", question, keep, QUESTION_BODY.to_owned(), "{\"effect\":\"operation\"}", "metadata に asked が無い"),
        ("question-bad-asked", question, keep, QUESTION_BODY.to_owned(), "{\"effect\":\"document\",\"asked\":\"planner\"}", "asked が planner"),
        ("question-memo-label", "intake:question,intake:memo", keep, QUESTION_BODY.to_owned(), QUESTION_META, "label intake:memo を併せ持てない"),
        ("question-inherits-labels", question, "--parent s2-1", QUESTION_BODY.to_owned(), QUESTION_META, "親 s2-1 の label を継ぐ"),
    ];
    for (reason, labels, parent, body, meta, named) in &forms {
        for client in ["bd", "bdw"] {
            let command = question_create(client, labels, parent, body, meta);
            let text = assert_question_deny(&state, &repo, &rules, &command, reason);
            assert!(text.contains(named), "{command}: {named} を名指す: {text}");
        }
    }
    assert_eq!(ledger_records(&state).len(), 20, "20 本 × 記録 1 行");
    for client in ["bd", "bdw"] {
        assert_question_pass(&state, &repo, &rules, &format!("{client} create x --parent s2-1 --labels doc:toy -d '無い 4 行'"));
    }
    fs::write(repo.join("ask.sh"), "bdw create q --labels intake:question -d 'x'\n").expect("script を書ける");
    assert_question_pass(&state, &repo, &rules, "sh ask.sh");
    clean(&[&repo, &state]);
}

/// (b) 揃った問いの create（本文が body-file・`-d`・metadata が `@meta.json` の 3 形）は rc 0 で記録を残さず、(e) `--stdin`
/// と `--body-file -` の問いは question-body-unreadable で断られる。
#[test]
fn hook_question_form_passes_complete_questions_and_denies_stdin_bodies() {
    let repo = git_repo();
    let state = linked(&repo);
    let rules = question_rules(&state);
    fs::write(repo.join("body.md"), QUESTION_BODY).expect("本文を書ける");
    fs::write(repo.join("meta.json"), QUESTION_META).expect("metadata を書ける");
    let keep = "--parent s2-1 --no-inherit-labels";
    for command in [
        format!("bdw create 問い --labels intake:question {keep} --body-file body.md --metadata @meta.json"),
        question_create("bd", "doc:toy,intake:question", keep, QUESTION_BODY, QUESTION_META),
        format!("scripts/bdw create --title=問い -l intake:question --parent=s2-1 --no-inherit-labels=true --body-file=body.md --metadata='{QUESTION_META}'"),
    ] {
        assert_question_pass(&state, &repo, &rules, &command);
    }
    for (client, body) in [("bd", "--stdin"), ("bdw", "--body-file -"), ("bdw", "--stdin --body-file body.md")] {
        let command = format!("{client} create 問い --labels intake:question {keep} {body} --metadata @meta.json");
        let text = assert_question_deny(&state, &repo, &rules, &command, "question-body-unreadable");
        assert!(text.contains("--stdin"), "{command}: 読めない形を名指す: {text}");
    }
    clean(&[&repo, &state]);
}

// ─────────────── memo の引き金の行（`s2-07l.738.12`・設計 ledger-form.md §15 行 k・接頭辞 `hook_memo_trigger_`） ───────────────
//
// rules は §14 と同じ写し（[`question_rules`]）。`.beads` の無い toy repo で撃ち、接頭辞の歯だけ `.beads/config.yaml` を置いた
// repo で撃つ（形の門が読む偽の client は根の epic E の 1 件を返し argv を 1 行ずつ記録する）。

/// 4 節が揃い、昇格条件が散文だけ（値に括弧が続く引き金の行を含む）の memo の本文。
const PROSE_MEMO: &str = "## memo\n### 出所\n- run: r\n### 観測\n- x\n### 候補\n### 昇格条件\n- 引き金: 再発 3（同じ落ち方）\n- 同じ落ち方が続いたら\n";

/// 断り文が名指す 5 形の字面。
const TRIGGER_SHAPES: &str = "引き金: 再発 <n> / 同梱 <path> / 依存 <id> / 期日 YYYY-MM-DDTHH:MMZ / 着地 <pointer>";

/// `pre-tool-use` に `args` を足して撃つ。
fn trigger_hook(repo: &Path, args: &[&str], command: &str) -> Output {
    let all: Vec<&str> = ["pre-tool-use"].into_iter().chain(args.iter().copied()).collect();
    run_hook_args(&all, &bash_payload(repo, command))
}

/// 引き金の段の deny の外形（rc 2・stdout 0 byte・stderr 1 行・`deny bd <verb>` の頭と語と §15）と記録 1 行を確かめ、stderr を返す。
fn assert_trigger_deny(state: &Path, repo: &Path, args: &[&str], command: &str, (verb, reason): (&str, &str)) -> String {
    let before = ledger_records(state).len();
    let out = trigger_hook(repo, args, command);
    let text = stderr_text(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{command}: deny は rc 2: {text}");
    assert!(out.stdout.is_empty(), "{command}: deny でも stdout は 0 byte");
    assert_eq!(stderr_lines(&out), 1, "{command}: stderr は 1 行: {text}");
    let head = format!("{NAME}: deny bd {verb} は起票の門が止める reason={reason}（");
    assert!(text.starts_with(&head) && text.trim_end().ends_with("・ledger-form.md §15）"), "{command}: {text}");
    let lines = ledger_records(state);
    assert_eq!(lines.len(), before + 1, "{command}: 記録は 1 行増える: {lines:?}");
    assert_eq!(what_of(&lines.last().cloned().unwrap_or_default()), format!("ledger-deny {reason}"), "{command}");
    text
}

/// rc 0・0 byte・記録なしで通ることを確かめる。
fn assert_trigger_pass(state: &Path, repo: &Path, args: &[&str], command: &str) {
    let before = inject_lines(state).len();
    assert_silent(&trigger_hook(repo, args, command), command);
    assert_eq!(inject_lines(state).len(), before, "{command}: 通す周は記録を残さない");
}

/// (a) 昇格条件が散文だけの memo の create と、昇格条件を引き金の無い本文へ書き換える update（`--body-file`）の 2 形 × bd と bdw
/// の 4 本がどれも断られ、断り文は 5 形の字面（create は最初の読めない行も）を名指す (b) 再発 1 の引き金を持つ memo の create と
/// 見出しの無い本文の update は通る (c) `bdw update s2-1 --stdin` は update-body-unreadable。
#[test]
fn hook_memo_trigger_denies_prose_memos_and_untriggered_updates_from_bd_and_bdw() {
    let repo = git_repo();
    let state = linked(&repo);
    let rules = question_rules(&state);
    let args = ["--rules", rules.as_str()];
    for (name, body) in [("prose.md", PROSE_MEMO), ("rewrite.md", "### 昇格条件\n- 散文だけの条件\n"), ("memo.md", MEMO_BODY), ("plain.md", "本文だけ\n")] {
        fs::write(repo.join(name), body).expect("本文を書ける");
    }
    for client in ["bd", "bdw"] {
        let create = format!("{client} create \"[memo] 観測の件\" --parent s2-1 --labels intake:memo --body-file prose.md");
        let text = assert_trigger_deny(&state, &repo, &args, &create, ("create", "no-trigger"));
        assert!(text.contains(TRIGGER_SHAPES), "{create}: 5 形の字面: {text}");
        assert!(text.contains("最初の読めない行: - 引き金: 再発 3（同じ落ち方）"), "{create}: 読めない行: {text}");
        let update = format!("{client} update s2-1 --body-file rewrite.md");
        let text = assert_trigger_deny(&state, &repo, &args, &update, ("update", "update-no-trigger"));
        assert!(text.contains(TRIGGER_SHAPES), "{update}: 5 形の字面: {text}");
    }
    assert_eq!(ledger_records(&state).len(), 4, "4 本 × 記録 1 行");
    for client in ["bd", "bdw"] {
        assert_trigger_pass(&state, &repo, &args, &format!("{client} create \"[memo] x\" --parent s2-1 --body-file memo.md"));
        assert_trigger_pass(&state, &repo, &args, &format!("{client} update s2-1 --body-file plain.md"));
    }
    let text = assert_trigger_deny(&state, &repo, &args, "bdw update s2-1 --stdin", ("update", "update-body-unreadable"));
    assert!(text.contains("--stdin"), "読めない形を名指す: {text}");
    clean(&[&repo, &state]);
}

/// (d) `.beads/config.yaml` に接頭辞 toy を持つ repo で、toy の依存の行を持つ memo は通り（形の門が台帳を 1 回読む）、別の接頭辞
/// の依存の行だけの memo は no-trigger で、引き金の段は台帳を 1 度も読まない。
#[test]
fn hook_memo_trigger_reads_dependencies_with_the_ledger_prefix() {
    use std::os::unix::fs::PermissionsExt;
    let repo = git_repo();
    let state = linked(&repo);
    fs::create_dir_all(repo.join(".beads")).expect(".beads を作れる");
    fs::write(repo.join(".beads").join("config.yaml"), "issue-prefix: toy\n").expect("台帳の設定を書ける");
    let (bd, log) = (state.join("bd"), state.join("bd.log"));
    let ledger = graph_bead("E", "open", "epic", "");
    fs::write(&bd, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nprintf '%s' '[{ledger}]'\n", log.display())).expect("偽の bd を書ける");
    fs::set_permissions(&bd, fs::Permissions::from_mode(0o755)).expect("偽の bd を実行可能にできる");
    let (rules, bd) = (question_rules(&state), bd.display().to_string());
    let args = ["--rules", rules.as_str(), "--bd", bd.as_str()];
    let reads = || fs::read_to_string(&log).map(|text| text.lines().count()).unwrap_or_default();
    let command = "bdw create \"[memo] x\" --parent E --labels intake:memo --body-file dep.md";
    fs::write(repo.join("dep.md"), MEMO_BODY.replace("- 引き金: 再発 1", "- 引き金: 依存 s2-7")).expect("本文を書ける");
    let text = assert_trigger_deny(&state, &repo, &args, command, ("create", "no-trigger"));
    assert!(text.contains("依存 s2-7"), "読めない依存の行を名指す: {text}");
    assert_eq!(reads(), 0, "引き金の段は台帳を読まない");
    fs::write(repo.join("dep.md"), MEMO_BODY.replace("- 引き金: 再発 1", "- 引き金: 依存 toy-7.1")).expect("本文を書ける");
    assert_trigger_pass(&state, &repo, &args, command);
    assert_eq!(reads(), 1, "通った create は形の門が 1 回だけ読む");
    clean(&[&repo, &state]);
}

// ─────────────── 台帳の形の門（`s2-07l.733`・設計 ledger-form.md §12 行 h・接頭辞 `hook_graph_guard_`） ───────────────
//
// toy repo の root に `.beads` の dir を置き、`--bd` に偽の client（fixture の JSON を返し argv を 1 行ずつ記録する・読めない
// 形は rc 1）を渡して `pre-tool-use` を撃つ。上限 N は埋め込みの rules 行の値。

/// 置き場（toy repo・置き場・偽の client・client の argv の記録）。
struct GraphPlace {
    /// toy repo。
    repo: TmpDir,
    /// 置き場。
    state: TmpDir,
    /// 偽の client の path。
    bd: String,
    /// client の argv の記録。
    log: PathBuf,
}

/// 台帳の 1 件（`parent` が空でなければ parent-child の辺 1 本）。
fn graph_bead(id: &str, status: &str, kind: &str, parent: &str) -> String {
    let deps = if parent.is_empty() {
        String::new()
    } else {
        format!("{{\"issue_id\":\"{id}\",\"depends_on_id\":\"{parent}\",\"type\":\"parent-child\"}}")
    };
    format!("{{\"id\":\"{id}\",\"status\":\"{status}\",\"issue_type\":\"{kind}\",\"dependencies\":[{deps}]}}")
}

/// 埋め込みの上限 N。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn graph_max() -> u64 {
    let manifest = vessel::rules::manifest::Manifest::embedded().expect("埋め込みの rules を読める");
    vessel::rules::int_row(&manifest, vessel::ledger::graph::ROW).expect("上限の行を読める")
}

/// 根の epic E（open の子は E.1〜E.<N-1> と子 epic E.e の N 本・closed の E.c）・E.1 の子 E.1.1・feature の top F と
/// その子 F.1・親の無い task O。
fn graph_ledger() -> String {
    let mut beads: Vec<String> = (1..graph_max()).map(|at| graph_bead(&format!("E.{at}"), "open", "task", "E")).collect();
    beads.extend([
        graph_bead("E", "open", "epic", ""),
        graph_bead("E.e", "open", "epic", "E"),
        graph_bead("E.c", "closed", "task", "E"),
        graph_bead("E.1.1", "open", "task", "E.1"),
        graph_bead("F", "open", "feature", ""),
        graph_bead("F.1", "open", "task", "F"),
        graph_bead("O", "open", "task", ""),
    ]);
    format!("[{}]", beads.join(","))
}

/// 置き場を作る（`readable` が偽なら client は rc 1・`beads` が偽なら root に `.beads` を置かない）。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn graph_place(readable: bool, beads: bool) -> GraphPlace {
    use std::os::unix::fs::PermissionsExt;
    let repo = git_repo();
    let state = linked(&repo);
    if beads {
        fs::create_dir_all(repo.join(".beads")).expect(".beads を作れる");
    }
    let json = state.join("ledger.json");
    fs::write(&json, graph_ledger()).expect("台帳の fixture を書ける");
    let log = state.join("bd.log");
    let tail = if readable { format!("cat '{}'", json.display()) } else { "exit 1".to_owned() };
    let bd = state.join("bd");
    fs::write(&bd, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n{tail}\n", log.display())).expect("偽の bd を書ける");
    fs::set_permissions(&bd, fs::Permissions::from_mode(0o755)).expect("偽の bd を実行可能にできる");
    GraphPlace { repo, state, bd: bd.display().to_string(), log }
}

/// 偽の client が記録した argv（起きた回数）。
fn graph_reads(place: &GraphPlace) -> Vec<String> {
    fs::read_to_string(&place.log).map(|text| text.lines().map(str::to_owned).collect()).unwrap_or_default()
}

/// `--bd` に偽の client を渡して撃つ（埋め込みの rules）。
fn graph_hook(place: &GraphPlace, command: &str) -> Output {
    run_hook_args(&["pre-tool-use", "--bd", &place.bd], &bash_payload(&place.repo, command))
}

/// 断る周（rc 2・stdout 0 byte・stderr 1 行・記録 1 行・台帳の読みはちょうど 1 回）の stderr。
fn assert_graph_deny(place: &GraphPlace, command: &str, reason: &str) -> String {
    let (records, reads) = (ledger_records(&place.state).len(), graph_reads(place).len());
    let out = graph_hook(place, command);
    assert_write_deny(&place.state, &out, command, reason);
    assert_eq!(ledger_records(&place.state).len(), records + 1, "{command}: 記録は 1 行増える");
    assert_eq!(graph_reads(place).len(), reads + 1, "{command}: 台帳は 1 回だけ読む");
    stderr_text(&out)
}

/// 通る周（0 byte・rc 0・記録なし）。
fn assert_graph_pass(place: &GraphPlace, command: &str) {
    let before = inject_lines(&place.state).len();
    assert_silent(&graph_hook(place, command), command);
    assert_eq!(inject_lines(&place.state).len(), before, "{command}: 記録を残さない");
}

/// (a) 溢れた根の epic E への create は parent-full で子 epic の作り方を名指し、同じ E への epic の create は通る (b) feature
/// の top の下への create は parent-unrooted で top を名指す。台帳の読みは `--readonly list --all` の 1 回。
#[test]
fn hook_graph_guard_create_denies_full_and_unrooted_parents() {
    let place = graph_place(true, true);
    let max = graph_max();
    let text = assert_graph_deny(&place, "bdw create x --parent E", "parent-full");
    assert!(text.contains("--type epic --parent E"), "子 epic の作り方: {text}");
    assert!(text.contains(&format!("{max} 本で上限 {max}")), "子の数と上限: {text}");
    assert_eq!(graph_reads(&place), ["--readonly list --all --limit 0 --json"], "client の argv");
    assert_graph_pass(&place, "bdw create x --parent E --type epic");
    let text = assert_graph_deny(&place, "bdw create x --parent F.1", "parent-unrooted");
    assert!(text.contains("鎖の終わり F）") && text.contains("bdw update F --type epic"), "top を名指す: {text}");
    clean(&[&place.repo, &place.state]);
}

/// (c) 子への付け替えは parent-loop (d) 親の空の update と唯一の親の dep remove は unrooting で、epic の親外しは通る (f) 根に
/// 着かない O を根に着く親へ付け替える update と top の型を epic にする update は通る (k) 1 行に並べた付け替えの 2 つ目は
/// 1 つ目の後の形で parent-loop（読みは 1 回）。
#[test]
fn hook_graph_guard_update_and_dep_remove_follow_the_ratchet() {
    let place = graph_place(true, true);
    assert_graph_deny(&place, "bdw update E.1 --parent E.1.1", "parent-loop");
    assert_graph_deny(&place, "bdw update E.1 --parent \"\"", "unrooting");
    let text = assert_graph_deny(&place, "bdw dep remove E.1 E", "unrooting");
    assert!(text.contains("bdw update E.1 --parent <epic>"), "付け替えを名指す: {text}");
    assert_graph_pass(&place, "bdw update E.e --parent \"\"");
    assert_graph_pass(&place, "bdw update O --parent E.e");
    assert_graph_pass(&place, "bdw update F --type epic");
    assert_graph_deny(&place, "bdw update E.1 --parent E.2 && bdw update E.2 --parent E.1", "parent-loop");
    assert_graph_pass(&place, "bdw update E.2 --parent E.1");
    clean(&[&place.repo, &place.state]);
}

/// (e) plan の node に親が無ければ plan-orphan・parent_id が溢れた E なら parent-full・file が無ければ plan-unreadable
/// (j) 溢れた E の closed の子の reopen と --status open は parent-full で、--status closed は通る。
#[test]
fn hook_graph_guard_plan_and_reopen_count_against_the_parent() {
    let place = graph_place(true, true);
    let plan = |name: &str, node: &str| {
        fs::write(place.repo.join(name), format!("{{\"nodes\":[{{\"key\":\"a\",\"title\":\"x\",\"type\":\"task\"{node}}}],\"edges\":[]}}"))
            .unwrap_or_else(|error| panic!("plan を書ける: {error}"));
    };
    plan("orphan.json", "");
    plan("full.json", ",\"parent_id\":\"E\"");
    assert_graph_deny(&place, "bdw create --graph orphan.json --parent E", "plan-orphan");
    assert_graph_deny(&place, "bdw create --graph full.json --parent E", "parent-full");
    assert_graph_deny(&place, "bdw create --graph gone.json --parent E", "plan-unreadable");
    assert_graph_deny(&place, "bdw reopen E.c", "parent-full");
    assert_graph_deny(&place, "bdw update E.c --status open", "parent-full");
    assert_graph_pass(&place, "bdw update E.1 --status closed");
    clean(&[&place.repo, &place.state]);
}

/// (g) 読めない client は ledger-unreadable で断る (h) 掛からない command（close・append-notes・show・--status closed）は
/// client を 1 回も起こさない (i) `.beads` の無い repo では同じ create を読まずに通す（どちらも読めば断られる client で撃つ）。
#[test]
fn hook_graph_guard_reads_only_for_the_six_writes_in_a_ledger_repo() {
    let place = graph_place(false, true);
    for command in ["bdw close E.1", "bdw update E.1 --append-notes x", "bdw show E.1", "bdw update E.1 --status closed"] {
        assert_graph_pass(&place, command);
    }
    assert!(graph_reads(&place).is_empty(), "掛からない command は読まない: {:?}", graph_reads(&place));
    assert_graph_deny(&place, "bdw create x --parent E", "ledger-unreadable");
    let bare = graph_place(false, false);
    assert_graph_pass(&bare, "bdw create x --parent E");
    assert_graph_pass(&bare, "bdw update E.1 --parent \"\"");
    assert!(graph_reads(&bare).is_empty(), ".beads の無い repo は読まない");
    clean(&[&place.repo, &place.state, &bare.repo, &bare.state]);
}

// ─────────────── 走っている便の行の門（`s2-07l.698`・設計 vessel-hook.md §15 行 i・接頭辞 `hook_live_row_`） ───────────────
//
// tmp の repo に行 a / b / c の表を持つ docs/design/x.md を commit し、`fleet record` の段の記帳と run dir の写し（契約・
// 判定の file・repo）で live な便を置く。`--project` は anchor（便の worktree から撃つ周も anchor が仕える）。

/// 表の doc の repo 相対 path。
const LIVE_DOC: &str = "docs/design/x.md";

/// 表の行 1 つ（done は `d-<id>`）。
fn live_row_text(id: &str) -> String {
    format!(
        "[[contract]]\nid = \"{id}\"\ntitle = \"t\"\nreq = [\"FR1\"]\nsection = \"1\"\nverify = [\"cargo test\"]\n\
         size = \"S\"\ndone = \"d-{id}\"\n"
    )
}

/// 行 a / b / c の表と散文を持つ doc の本文。
fn live_doc_text() -> String {
    let rows: Vec<String> = ["a", "b", "c"].iter().map(|id| live_row_text(id)).collect();
    format!("# x\n\nprose line\n\n<!-- contracts:begin -->\nschema = 1\n\n{}<!-- contracts:end -->\n", rows.join("\n"))
}

/// 表の doc を commit した repo と、紐づけた置き場。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn live_place() -> (TmpDir, TmpDir) {
    let repo = git_repo();
    fs::create_dir_all(repo.join("docs").join("design")).expect("docs/design を作れる");
    fs::write(repo.join(LIVE_DOC), live_doc_text()).expect("doc を書ける");
    git(&repo, &["add", LIVE_DOC]);
    git(&repo, &["commit", "-q", "-m", "doc"]);
    let state = linked(&repo);
    (repo, state)
}

/// live な便 1 本を置く（段の記帳・写しの契約・判定の file〔`verdict` が在れば Gated の verdict.json と Reviewed の
/// review.json〕・repo の書き留め）。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn live_run(state: &Path, repo: &Path, (run, stage, row): (&str, &str, &str), verdict: Option<&str>) {
    let out = Command::new(bin())
        .args(["fleet", "record", "--kind", "RunStage", "--run", run, "--bead", "s2-live", "--stage", stage])
        .args(["--detail", "e2e", "--state-dir"])
        .arg(state)
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "fleet record: {}", stderr_text(&out));
    fs::create_dir_all(vessel::pipe::run_dir(state, run)).expect("run dir を作れる");
    let contract = format!(
        "goal = \"g\"\ndone = \"d\"\nsize = \"S\"\nowner = \"generated\"\ndisposition = \"A-now\"\n\
         write-set = [\"{LIVE_DOC}\"]\nverify = [\"cargo test\"]\nreq = [\"FR1\"]\ndesign = \"{LIVE_DOC}#{row}\"\n"
    );
    fs::write(vessel::pipe::contract_path(state, run), contract).expect("写しを書ける");
    fs::write(vessel::pipe::repo_path(state, run), format!("{}\n", repo.display())).expect("repo を書ける");
    if let Some(found) = verdict {
        let body = format!("{{\"verdict\":\"{found}\"}}\n");
        fs::write(vessel::pipe::verdict_path(state, run), &body).expect("verdict.json を書ける");
        fs::write(vessel::pipe::review::review_path(state, run), &body).expect("review.json を書ける");
    }
}

/// `--project <anchor>` 付きで pre-tool-use を撃つ。
fn live_hook(repo: &Path, payload: &str) -> Output {
    run_hook_args(&["pre-tool-use", "--project", &repo.display().to_string()], payload)
}

/// Edit の payload（`file` の `old` を `new` へ・cwd は `cwd`）。
fn edit_payload(cwd: &Path, file: &Path, old: &str, new: &str) -> String {
    format!(
        "{{\"cwd\":\"{}\",\"tool_name\":\"Edit\",\"tool_input\":{{\"file_path\":{},\"old_string\":{},\"new_string\":{}}}}}",
        cwd.display(),
        json_lite::quote(&file.display().to_string()),
        json_lite::quote(old),
        json_lite::quote(new)
    )
}

/// Write の payload（`file` へ `content`）。
fn write_payload(cwd: &Path, file: &Path, content: &str) -> String {
    format!(
        "{{\"cwd\":\"{}\",\"tool_name\":\"Write\",\"tool_input\":{{\"file_path\":{},\"content\":{}}}}}",
        cwd.display(),
        json_lite::quote(&file.display().to_string()),
        json_lite::quote(content)
    )
}

/// 記録のうち走っている便の行の門の行。
fn live_row_records(state: &Path) -> Vec<String> {
    inject_lines(state).into_iter().filter(|line| what_of(line).starts_with("live-row-deny")).collect()
}

/// 断りの外形（rc 2・stdout 0 byte・stderr 1 行）と記録の `what` が 1 行増えることを確かめ、stderr を返す。
fn assert_live_row_deny(state: &Path, out: &Output, what: &str) -> String {
    let text = stderr_text(out);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{what}: deny は rc 2: {text}");
    assert!(out.stdout.is_empty(), "{what}: stdout 0 byte");
    assert_eq!(stderr_lines(out), 1, "{what}: stderr 1 行: {text}");
    assert_eq!(live_row_records(state).last().map(|line| what_of(line)), Some(format!("live-row-deny {what}")), "{what}");
    text
}

/// (1)(2) Questioned の便の行の done を変える Edit は断り（答える口・末尾が止める 1 行）、散文だけの Edit は通す。
#[test]
fn hook_live_row_questioned_row_edit_is_denied_and_prose_edit_passes() {
    let (repo, state) = live_place();
    live_run(&state, &repo, ("r-q", "Questioned", "a"), None);
    let doc = repo.join(LIVE_DOC);
    let out = live_hook(&repo, &edit_payload(&repo, &doc, "done = \"d-a\"", "done = \"e-a\""));
    let text = assert_live_row_deny(&state, &out, "changed");
    assert!(text.contains("run=r-q") && text.contains("stage=Questioned"), "{text}");
    assert!(text.contains(&format!("{NAME} pipe answer --run r-q --words ")), "答える口: {text}");
    let stop = format!("{NAME} pipe stop --run r-q --state-dir {} --repo {}", state.display(), repo.display());
    assert!(text.trim_end().ends_with(&stop), "末尾が止める 1 行: {text}");
    assert_eq!(live_row_records(&state).len(), 1, "記録 1 行");
    let out = live_hook(&repo, &edit_payload(&repo, &doc, "prose line", "prose line 2"));
    assert_silent(&out, "散文だけの Edit");
    clean(&[&repo, &state]);
}

/// (2)(3) 判定 FAIL の Gated の便の行を変える Write は通し、判定 PASS の Gated と審査 PASS の Reviewed の便の行は断る。
#[test]
fn hook_live_row_gated_and_reviewed_rows_follow_the_verdict() {
    let (repo, state) = live_place();
    live_run(&state, &repo, ("r-fail", "Gated", "a"), Some("FAIL"));
    live_run(&state, &repo, ("r-pass", "Gated", "b"), Some("PASS"));
    live_run(&state, &repo, ("r-rev", "Reviewed", "c"), Some("PASS"));
    let doc = repo.join(LIVE_DOC);
    let out = live_hook(&repo, &write_payload(&repo, &doc, &live_doc_text().replace("d-a", "e-a")));
    assert_silent(&out, "判定 FAIL の Gated は終端");
    for (id, run) in [("b", "r-pass"), ("c", "r-rev")] {
        let changed = live_doc_text().replace(&format!("d-{id}"), &format!("e-{id}"));
        let out = live_hook(&repo, &write_payload(&repo, &doc, &changed));
        let text = assert_live_row_deny(&state, &out, "changed");
        assert!(text.contains(&format!("run={run}")) && !text.contains("pipe answer"), "{text}");
    }
    clean(&[&repo, &state]);
}

/// (4) 作業の木の doc を fs で書き換えた後の git commit -am は断り、元へ戻した後は通す。
#[test]
fn hook_live_row_commit_of_a_changed_tree_is_denied_until_restored() {
    let (repo, state) = live_place();
    live_run(&state, &repo, ("r-q", "Questioned", "a"), None);
    fs::write(repo.join(LIVE_DOC), live_doc_text().replace("d-a", "e-a")).expect("書き換えられる");
    let out = live_hook(&repo, &bash_payload(&repo, "git commit -am x"));
    assert_live_row_deny(&state, &out, "changed");
    fs::write(repo.join(LIVE_DOC), live_doc_text()).expect("戻せる");
    assert_silent(&live_hook(&repo, &bash_payload(&repo, "git commit -am x")), "元へ戻した木");
    clean(&[&repo, &state]);
}

/// (5) 便の worktree の中の自分の行を変える Edit は通し（記録 0）、同じ worktree から他の live な行を変える Edit と、同じ
/// 変更を anchor の doc に当てる Edit は断る。
#[test]
fn hook_live_row_own_row_in_its_worktree_passes() {
    let (repo, state) = live_place();
    live_run(&state, &repo, ("r-own", "Spawned", "a"), None);
    live_run(&state, &repo, ("r-other", "Spawned", "b"), None);
    let worktree = vessel::pipe::worktree_path(&repo, "r-own");
    git(&repo, &["worktree", "add", "-q", &worktree.display().to_string()]);
    let own = worktree.join(LIVE_DOC);
    let out = live_hook(&repo, &edit_payload(&worktree, &own, "done = \"d-a\"", "done = \"e-a\""));
    assert_silent(&out, "自分の worktree の自分の行");
    assert!(live_row_records(&state).is_empty(), "記録 0");
    let out = live_hook(&repo, &edit_payload(&worktree, &own, "done = \"d-b\"", "done = \"e-b\""));
    assert!(assert_live_row_deny(&state, &out, "changed").contains("run=r-other"), "他の live な行は残る");
    let out = live_hook(&repo, &edit_payload(&repo, &repo.join(LIVE_DOC), "done = \"d-a\"", "done = \"e-a\""));
    assert!(assert_live_row_deny(&state, &out, "changed").contains("run=r-own"), "anchor の doc では効く");
    clean(&[&repo, &state]);
}

/// (6) event log を読めない置き場では、行を変える Edit は state-unreadable で断り、散文だけの Edit と docs/design/ の外の
/// 区間を持つ `.md` の Edit は通す。
#[test]
fn hook_live_row_unreadable_state_denies_only_row_changes() {
    let (repo, state) = live_place();
    let events = vessel::fleet::store::events_path(&state);
    fs::remove_file(&events).ok();
    fs::create_dir_all(&events).expect("event log の位置に dir を置ける");
    let doc = repo.join(LIVE_DOC);
    let out = live_hook(&repo, &edit_payload(&repo, &doc, "done = \"d-a\"", "done = \"e-a\""));
    assert!(assert_live_row_deny(&state, &out, "state-unreadable").contains("reason=state-unreadable"));
    assert_silent(&live_hook(&repo, &edit_payload(&repo, &doc, "prose line", "prose 2")), "散文だけ");
    let outside = repo.join("notes.md");
    fs::write(&outside, live_doc_text()).expect("docs/design/ の外の doc を書ける");
    let out = live_hook(&repo, &edit_payload(&repo, &outside, "done = \"d-a\"", "done = \"e-a\""));
    assert_silent(&out, "docs/design/ の外");
    clean(&[&repo, &state]);
}

/// (7) 解けない dir の commit は、live な便が在る置き場では dir-unresolved で断り（git -C の形を示す）、live な便が 0 本の
/// 置き場では通す。
#[test]
fn hook_live_row_unresolved_commit_dir_follows_the_live_count() {
    let command = "cd \"$X\" && git commit -am x";
    let (repo, state) = live_place();
    assert_silent(&live_hook(&repo, &bash_payload(&repo, command)), "live な便が 0 本");
    live_run(&state, &repo, ("r-q", "Questioned", "a"), None);
    let out = live_hook(&repo, &bash_payload(&repo, command));
    let text = assert_live_row_deny(&state, &out, "dir-unresolved");
    assert!(text.contains("reason=dir-unresolved") && text.contains("run=r-q"), "{text}");
    assert!(text.contains("git -C <絶対 path> commit"), "dir を literal で書く形: {text}");
    clean(&[&repo, &state]);
}

/// (8) live な行を変えた doc を add した後に作業の木を HEAD の本文へ戻した木（index にだけ在る変更）の commit は断る。
#[test]
fn hook_live_row_index_only_change_is_denied() {
    let (repo, state) = live_place();
    live_run(&state, &repo, ("r-q", "Questioned", "a"), None);
    fs::write(repo.join(LIVE_DOC), live_doc_text().replace("d-a", "e-a")).expect("書き換えられる");
    git(&repo, &["add", LIVE_DOC]);
    fs::write(repo.join(LIVE_DOC), live_doc_text()).expect("作業の木を戻せる");
    assert_live_row_deny(&state, &live_hook(&repo, &bash_payload(&repo, "git commit -m x")), "changed");
    clean(&[&repo, &state]);
}

/// (9) live な行を持つ design doc を git mv で docs/design/ の外へ移した木の commit は断る（旧 path の行の消失）。
#[test]
fn hook_live_row_git_mv_out_of_design_is_denied() {
    let (repo, state) = live_place();
    live_run(&state, &repo, ("r-q", "Questioned", "a"), None);
    git(&repo, &["mv", LIVE_DOC, "moved.md"]);
    assert_live_row_deny(&state, &live_hook(&repo, &bash_payload(&repo, "git commit -m x")), "removed");
    clean(&[&repo, &state]);
}

// ─────────────── anchor の門（`s2-07l.700`・設計 vessel-hook.md §14 行 h・接頭辞 `hook_anchor_guard_`） ───────────────
//
// HEAD が main の tmp repo を器へ紐づけ、揃えなかった anchor（main を別の木の commit へ進め、index と作業の木を着地の前の
// 中身のまま残す）に行 az の書き手で印を置く。`--project` は anchor。

/// HEAD が `refs/heads/main` の tmp repo と、紐づけた置き場。
fn anchor_place() -> (TmpDir, TmpDir) {
    let repo = git_repo();
    git(&repo, &["branch", "-M", "main"]);
    let state = linked(&repo);
    (repo, state)
}

/// 揃えなかった形の anchor: main を別の木の commit へ進め、index と作業の木を着地の前の中身へ戻し、印を置く。
/// 返すのは着地の前と後の main。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn stale_anchor(repo: &Path) -> (String, String) {
    let old = git(repo, &["rev-parse", "refs/heads/main"]);
    fs::write(repo.join("src").join("lib.rs"), "// landed\n").expect("着地の中身を書ける");
    git(repo, &["commit", "-q", "-am", "landed"]);
    let new = git(repo, &["rev-parse", "refs/heads/main"]);
    git(repo, &["read-tree", "-m", "-u", &new, &old]);
    let marked = vessel::pipe::land::write_mark(repo, &old, &new);
    assert_eq!(marked, Ok(vessel::pipe::land::Marked::Written), "印を行 az の書き手で置ける");
    (old, new)
}

/// 記録のうち anchor の門の行。
fn anchor_records(state: &Path) -> Vec<String> {
    inject_lines(state).into_iter().filter(|line| what_of(line).starts_with("anchor-deny")).collect()
}

/// 断りの外形（rc 2・stdout 0 byte・stderr 1 行）と記録「anchor-deny <動詞>」が 1 行増えることを確かめ、stderr を返す。
fn assert_anchor_deny(state: &Path, repo: &Path, command: &str, verb: &str) -> String {
    let before = anchor_records(state).len();
    let out = live_hook(repo, &bash_payload(repo, command));
    let text = stderr_text(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{command}: deny は rc 2: {text}");
    assert!(out.stdout.is_empty(), "{command}: stdout 0 byte");
    assert_eq!(stderr_lines(&out), 1, "{command}: stderr 1 行: {text}");
    assert!(text.starts_with(&format!("{NAME}: deny anchor-guard ")), "{command}: 器が名乗る: {text}");
    let records = anchor_records(state);
    assert_eq!(records.len(), before + 1, "{command}: 記録 1 行");
    assert_eq!(records.last().map(|line| what_of(line)), Some(format!("anchor-deny {verb}")), "{command}");
    text
}

/// 揃える 1 行の literal。
fn sync_literal(repo: &Path) -> String {
    format!("{NAME} pipe anchor-sync --repo {}", repo.display())
}

/// (a) 揃えなかった anchor で `git commit -m x` は断られ、stderr の 1 行が揃える literal を持ち、記録が 1 行残る。`git -C`・
/// `cd && git pull`・`FOO=1 git merge` も同じく断られ、`gh pr merge` は窓の anchor=stale から揃える literal を次の一手に持つ。
#[test]
fn hook_anchor_guard_stale_anchor_denies_the_index_verbs_with_the_sync_literal() {
    let (repo, state) = anchor_place();
    let (old, _) = stale_anchor(&repo);
    let text = assert_anchor_deny(&state, &repo, "git commit -m x", "commit");
    assert!(text.contains("reason=anchor-stale") && text.contains(&format!("from={old}")), "{text}");
    assert!(text.trim_end().ends_with(&sync_literal(&repo)), "揃える 1 行で終わる: {text}");
    let at = repo.display();
    for (command, verb) in [
        (format!("git -C {at} commit -m x"), "commit"),
        (format!("cd {at} && git pull"), "pull"),
        ("FOO=1 git merge x".to_owned(), "merge"),
    ] {
        let text = assert_anchor_deny(&state, &repo, &command, verb);
        assert!(text.contains(&sync_literal(&repo)), "{command}: {text}");
    }
    let text = assert_anchor_deny(&state, &repo, "gh pr merge 1", "gh-pr-merge");
    assert!(text.contains("land-window=busy") && text.contains(" anchor=stale"), "窓の busy の行を写す: {text}");
    assert!(text.trim_end().ends_with(&sync_literal(&repo)), "窓が anchor=stale なら揃える 1 行: {text}");
    clean(&[&repo, &state]);
}

/// (b) 同じ anchor で `git status`・`git add x`・`git log` と Bash 以外の tool は通り、linked worktree の中の `git commit` も通り、
/// 着地の path を手で揃えた後の `git commit` は印が在るまま通る（空虚さの柵）。
#[test]
fn hook_anchor_guard_passes_reads_worktrees_and_a_hand_synced_anchor() {
    let (repo, state) = anchor_place();
    let (old, new) = stale_anchor(&repo);
    for command in ["git status", "git add x", "git log"] {
        assert_silent(&live_hook(&repo, &bash_payload(&repo, command)), command);
    }
    let read = tool_payload(&repo, "Read", &repo.join("src").join("lib.rs").display().to_string());
    assert_silent(&live_hook(&repo, &read), "Bash 以外の tool");
    let place = tmp();
    let worktree = place.join("wt");
    git(&repo, &["worktree", "add", "-q", &worktree.display().to_string()]);
    assert_silent(&live_hook(&repo, &bash_payload(&worktree, "git commit -m x")), "linked worktree の中");
    git(&repo, &["read-tree", "-m", "-u", &old, &new]);
    assert!(vessel::pipe::land::mark_path(Path::new(&git(&repo, &["rev-parse", "--absolute-git-dir"]))).exists(), "印は残る");
    assert_silent(&live_hook(&repo, &bash_payload(&repo, "git commit -m x")), "手で揃えた anchor");
    assert!(anchor_records(&state).is_empty(), "記録 0");
    git(&repo, &["worktree", "remove", "--force", &worktree.display().to_string()]);
    clean(&[&repo, &state, &place]);
}

/// (c) 印の中身を壊した anchor では `git commit` を断り、理由が「読めない」を名指す。
#[test]
fn hook_anchor_guard_unreadable_mark_is_denied() {
    let (repo, state) = anchor_place();
    stale_anchor(&repo);
    let mark = vessel::pipe::land::mark_path(Path::new(&git(&repo, &["rev-parse", "--absolute-git-dir"])));
    fs::write(&mark, "garbage\n").expect("壊した印を置ける");
    let text = assert_anchor_deny(&state, &repo, "git commit -m x", "commit");
    assert!(text.contains("reason=anchor-unreadable:mark") && text.contains("読めない"), "{text}");
    assert!(text.contains(&sync_literal(&repo)), "{text}");
    clean(&[&repo, &state]);
}

/// (d) origin の main を local の main の 1 つ前に置いた（未 push の）anchor で、HEAD が main の `git commit` と `gh pr merge 1`
/// は窓の busy の行と land-window の literal で断られる。HEAD を別 branch に替えた `git commit` は通り、origin を local に
/// 揃えた後の `gh pr merge 1`（発端の trailer の本文を持つ・merge の門は行 mg）は通る。
#[test]
fn hook_anchor_guard_closed_window_denies_main_commit_and_pr_merge() {
    let (repo, state) = anchor_place();
    fs::write(repo.join("src").join("lib.rs"), "// second\n").expect("2 つ目の中身を書ける");
    git(&repo, &["commit", "-q", "-am", "second"]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", "refs/heads/main~1"]);
    let main = git(&repo, &["rev-parse", "refs/heads/main"]);
    let window = format!("{NAME} pipe land-window --repo {}", repo.display());
    for (command, verb) in [("git commit -m x", "commit"), ("gh pr merge 1", "gh-pr-merge")] {
        let text = assert_anchor_deny(&state, &repo, command, verb);
        assert!(text.contains(&format!("land-window=busy queue=- following=- unpushed={main}")), "{command}: {text}");
        assert!(text.trim_end().ends_with(&window), "{command}: 窓を見る 1 行: {text}");
    }
    git(&repo, &["checkout", "-q", "-b", "side"]);
    assert_silent(&live_hook(&repo, &bash_payload(&repo, "git commit -m x")), "HEAD が別 branch");
    git(&repo, &["update-ref", "refs/remotes/origin/main", "refs/heads/main"]);
    assert_silent(&live_hook(&repo, &bash_payload(&repo, &format!("gh pr merge 1 --body '{}s2-a.1'", merge_key()))), "窓が開いた");
    clean(&[&repo, &state]);
}

/// (e) marker の無い repo では、揃えなかった anchor の `git commit` でも 1 byte も出さない（FR24）。
#[test]
fn hook_anchor_guard_is_silent_in_a_repo_without_marker() {
    let repo = git_repo();
    git(&repo, &["branch", "-M", "main"]);
    stale_anchor(&repo);
    assert_silent(&live_hook(&repo, &bash_payload(&repo, "git commit -m x")), "marker の無い repo");
    clean(&[&repo]);
}

// ─────────────── merge の門（設計 vessel-hook.md §21 行 mg・FR92 / AC62・接頭辞 `hook_merge_gate_`） ───────────────
//
// 窓を開いた anchor（origin の main を local の main へ揃える）で `gh pr merge` を撃ち、本文の発端の trailer か器の便の trailer
// の有無で通すか断るかを binary の外形で測る。`--project` は anchor・payload の cwd も anchor。

/// 発端の trailer の key（NAME の先頭を大文字にして `-Source: ` を足す・core の導出と独立に組む）。
fn merge_key() -> String {
    let mut chars = NAME.chars();
    let head: String = chars.next().map(|first| first.to_uppercase().to_string()).unwrap_or_default();
    format!("{head}{}-Source: ", chars.as_str())
}

/// 窓を開いた anchor と、紐づけた置き場。
fn merge_place() -> (TmpDir, TmpDir) {
    let (repo, state) = anchor_place();
    git(&repo, &["update-ref", "refs/remotes/origin/main", "refs/heads/main"]);
    (repo, state)
}

/// 記録のうち merge の門の行。
fn merge_records(state: &Path) -> Vec<String> {
    inject_lines(state).into_iter().filter(|line| what_of(line).starts_with("merge-deny")).collect()
}

/// 断りの外形（rc 2・stdout 0 byte・stderr 1 行・器の名乗りと理由）と記録「merge-deny <理由>」が 1 行増えることを確かめ、
/// stderr を返す。
fn assert_merge_deny(state: &Path, repo: &Path, command: &str, reason: &str) -> String {
    let before = merge_records(state).len();
    let out = live_hook(repo, &bash_payload(repo, command));
    let text = stderr_text(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{command}: deny は rc 2: {text}");
    assert!(out.stdout.is_empty(), "{command}: stdout 0 byte");
    assert_eq!(stderr_lines(&out), 1, "{command}: stderr 1 行: {text}");
    assert!(text.starts_with(&format!("{NAME}: deny merge-gate reason={reason} ")), "{command}: {text}");
    let records = merge_records(state);
    assert_eq!(records.len(), before + 1, "{command}: 記録 1 行");
    assert_eq!(records.last().map(|line| what_of(line)), Some(format!("merge-deny {reason}")), "{command}");
    assert!(text.contains(merge_key().trim_end()) && text.contains("run: "), "{command}: key の字と run の形: {text}");
    assert!(text.contains("gh pr merge <PR> --squash --body-file <絶対 path>"), "{command}: 次の一手: {text}");
    text
}

/// (1) 通る 3 形（AC62 の通過 3/3）: 題の行と空行と発端の trailer（id 1 本）の `--body`、id 2 本の行の本文 file を絶対 path で渡す
/// `--body-file`、器の便の trailer の `-b` は rc 0・0 byte・merge-deny の記録 0 行。
#[test]
fn hook_merge_gate_passes_the_three_trailer_forms() {
    let (repo, state) = merge_place();
    let place = tmp();
    let key = merge_key();
    let file = place.join("body.md");
    fs::write(&file, format!("要旨の段落。\n\nCo-Authored-By: e2e <e2e@example.invalid>\n{key}s2-a.1 s2-b.2\n")).expect("本文を書ける");
    for command in [
        format!("gh pr merge 1 --squash --body \"題の行\n\n{key}s2-07l.739\""),
        format!("gh pr merge 1 --squash --body-file {}", file.display()),
        "gh pr merge 1 --squash -b 'run: s2-07l.351-20260922T061245Z'".to_owned(),
    ] {
        assert_silent(&live_hook(&repo, &bash_payload(&repo, &command)), &command);
    }
    assert!(merge_records(&state).is_empty(), "記録 0 行");
    clean(&[&repo, &state, &place]);
}

/// (2) 断る 2 形（AC62 の断り 2/2）: 本文の無い `gh pr merge 1 --squash` は no-body で 4 つの flag と key の字を名指し、形の外の
/// id（大文字）の trailer の本文は bad-source でその行を名指す。
#[test]
fn hook_merge_gate_denies_a_missing_body_and_a_bad_source_line() {
    let (repo, state) = merge_place();
    let key = merge_key();
    let text = assert_merge_deny(&state, &repo, "gh pr merge 1 --squash", "no-body");
    for flag in ["--body", "-b", "--body-file", "-F"] {
        assert!(text.contains(flag), "{flag}: {text}");
    }
    let bad = format!("{key}S2-A.1");
    let text = assert_merge_deny(&state, &repo, &format!("gh pr merge 1 --squash --body '要旨\n\n{bad}'"), "bad-source");
    assert!(text.contains(&bad), "形の外の行を名指す: {text}");
    assert_eq!(merge_records(&state).len(), 2, "記録 2 行");
    clean(&[&repo, &state]);
}

/// (3) 形の周り: 良い本文の `--rebase` は rebase、`-F -` と値が変数の `--body` と無い file の `--body-file` は body-unreadable、
/// payload の cwd からの相対 path の本文 file は通る。
#[test]
fn hook_merge_gate_refuses_rebase_and_unreadable_bodies_and_reads_relative_files() {
    let (repo, state) = merge_place();
    let key = merge_key();
    assert_merge_deny(&state, &repo, &format!("gh pr merge 1 --rebase --body '{key}s2-a.1'"), "rebase");
    let missing = repo.join("no-such-body.md");
    for command in [
        "gh pr merge 1 --squash -F -".to_owned(),
        "gh pr merge 1 --squash --body \"$BODY\"".to_owned(),
        format!("gh pr merge 1 --squash --body-file {}", missing.display()),
    ] {
        let text = assert_merge_deny(&state, &repo, &command, "body-unreadable");
        assert!(text.contains("flag=-F") || text.contains("flag=--body"), "{command}: flag を名指す: {text}");
    }
    fs::write(repo.join("merge-body.md"), format!("要旨\n\n{key}s2-a.1\n")).expect("本文を書ける");
    assert_silent(&live_hook(&repo, &bash_payload(&repo, "gh pr merge 1 --squash --body-file merge-body.md")), "相対 path");
    clean(&[&repo, &state]);
}

/// (4) marker の無い repo では本文の無い merge も 0 byte・rc 0（FR24）。
#[test]
fn hook_merge_gate_is_silent_in_a_repo_without_marker() {
    let repo = git_repo();
    git(&repo, &["branch", "-M", "main"]);
    assert_silent(&live_hook(&repo, &bash_payload(&repo, "gh pr merge 1 --squash")), "marker の無い repo");
    clean(&[&repo]);
}

// ─────────────── 選択式の問いの門（設計 vessel-hook.md §20 行 ca・ADR-0084・接頭辞 `hook_choice_question_`） ───────────────
//
// AskUserQuestion の呼び出しを、席か runner か・誰が開いた session か・skill の直後か・例外の印が在るかに依らず全部の門の前で
// 止める。器を名乗らない repo では 1 byte も出さない。

/// AskUserQuestion の payload（同じ session_id と prompt_id・`extra` は先頭に足す欄〔`,` 始まり〕）。
fn ask_payload(cwd: &Path, extra: &str) -> String {
    format!(
        "{{\"session_id\":\"sid-ask\",\"prompt_id\":\"p-ask\"{extra},\"cwd\":\"{}\",\"tool_name\":\"AskUserQuestion\",\
         \"tool_input\":{{\"questions\":[{{\"question\":\"どれにする?\",\"options\":[{{\"label\":\"a\"}},{{\"label\":\"b\"}}]}}]}}}}",
        cwd.display()
    )
}

/// 記録のうち選択式の問いの門の行。
fn choice_records(state: &Path) -> Vec<String> {
    inject_lines(state).into_iter().filter(|line| what_of(line) == "choice-question-deny").collect()
}

/// 断りの外形（rc 2・stdout 0 byte・stderr 1 行・器の名乗り）と記録が 1 行増えることを確かめ、stderr を返す。
fn assert_choice_deny(state: &Path, why: &str, run: impl FnOnce() -> Output) -> String {
    let before = choice_records(state).len();
    let out = run();
    let text = stderr_text(&out);
    assert_eq!(out.status.code(), Some(i32::from(RC_BROKEN)), "{why}: deny は rc 2: {text}");
    assert!(out.stdout.is_empty(), "{why}: stdout 0 byte");
    assert_eq!(stderr_lines(&out), 1, "{why}: stderr 1 行: {text}");
    assert!(text.starts_with(&format!("{NAME}: deny choice-question tool=AskUserQuestion ")), "{why}: {text}");
    assert_eq!(choice_records(state).len(), before + 1, "{why}: 記録 1 行");
    text
}

/// 宣言の必須 key だけの本文（3 行）。
const ASK_DECL: &str = "schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git diff --quiet\"]\n";

/// `ASK_DECL` に `extra` を足した宣言を commit した repo と、紐づけた置き場。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn ask_place(extra: &str) -> (TmpDir, TmpDir) {
    let repo = git_repo();
    fs::write(repo.join(DECL_FILE), format!("{ASK_DECL}{extra}")).expect("宣言を書ける");
    git(&repo, &["add", DECL_FILE]);
    git(&repo, &["commit", "-q", "-m", "decl"]);
    let state = linked(&repo);
    (repo, state)
}

/// (1) 5 形の AskUserQuestion がどれも断られ、1 形 1 行ずつ記録が増える: 登録 row を持つ orchestrator の席・pane の無い session・
/// runner の形（`.vessel` を commit した repo の linked worktree を `--project` にし pane が無い）・skill の直後・例外の印。
#[test]
fn hook_choice_question_denies_every_form_of_session() {
    let place = role_place();
    let (seat, pane) = role_seat(&place, "askseat", Some("orchestrator"));
    let state: &Path = &place.state;
    let ask = ask_payload(&place.repo, "");
    assert_choice_deny(state, "登録 row を持つ席", || run_role_hook(&place, &pane, &[], &ask));
    assert_choice_deny(state, "pane の無い session", || run_hook_args(&["pre-tool-use"], &ask));

    git(&place.repo, &["add", MARKER]);
    git(&place.repo, &["commit", "-q", "-m", "marker"]);
    let runner = tmp();
    let worktree = runner.join("wt");
    git(&place.repo, &["worktree", "add", "-q", &worktree.display().to_string()]);
    let project = worktree.display().to_string();
    assert_choice_deny(state, "runner の形", || run_hook_args(&["pre-tool-use", "--project", &project], &ask_payload(&worktree, "")));

    let skill = format!(
        "{{\"session_id\":\"sid-ask\",\"prompt_id\":\"p-ask\",\"cwd\":\"{}\",\"hook_event_name\":\"UserPromptSubmit\",\"prompt\":\"/grill-me 設計を詰める\"}}",
        place.repo.display()
    );
    let submitted = run_hook_args(&["user-prompt-submit"], &skill);
    assert_eq!(submitted.status.code(), Some(i32::from(RC_OK)), "skill の起動の行: {}", stderr_text(&submitted));
    assert_choice_deny(state, "skill の直後", || run_hook_args(&["pre-tool-use"], &ask));

    fs::write(state.join("grill-exception"), "").expect("v1 草稿の印の名の空 file を置ける");
    let role_only = place.sock_dir.join("role-only.toml");
    fs::write(&role_only, format!("schema = 1\n{}", role_rows_text(ORCHESTRATOR_CAPS))).expect("役割の行だけの manifest を書ける");
    let marked = ask_payload(&place.repo, ",\"permission_mode\":\"bypassPermissions\"");
    let args = ["pre-tool-use", "--pane", &pane, "--tmux-socket", &place.socket, "--rules", &role_only.display().to_string()];
    assert_choice_deny(state, "例外の印", || run_hook_args(&args, &marked));
    assert_eq!(choice_records(state).len(), 5, "1 形 1 行");
    git(&place.repo, &["worktree", "remove", "--force", &project]);
    drop(seat);
    clean(&[&place.repo, &place.state, &place.sock_dir, &runner]);
}

/// (2) HEAD の宣言が question-route を持つ repo は stderr がその値を持ち、key の無い宣言の repo は持たず、不備の宣言（未知の key）
/// の repo は「読めない」を持つ。どれも rc 2。
#[test]
fn hook_choice_question_names_the_declared_route() {
    let (routed, routed_state) = ask_place("question-route = \"台帳の問いは bd で立てる\"\n");
    let text = assert_choice_deny(&routed_state, "宣言した経路", || run_hook("pre-tool-use", &ask_payload(&routed, "")));
    assert!(text.trim_end().ends_with("この repo の問いの経路: 台帳の問いは bd で立てる"), "{text}");
    let (plain, plain_state) = ask_place("");
    let text = assert_choice_deny(&plain_state, "key の無い宣言", || run_hook("pre-tool-use", &ask_payload(&plain, "")));
    assert!(!text.contains("問いの経路") && !text.contains("読めない"), "{text}");
    let (broken, broken_state) = ask_place("unknown-key = \"x\"\n");
    let text = assert_choice_deny(&broken_state, "不備の宣言", || run_hook("pre-tool-use", &ask_payload(&broken, "")));
    assert!(text.contains("vessel 宣言を読めない"), "{text}");
    clean(&[&routed, &routed_state, &plain, &plain_state, &broken, &broken_state]);
}

/// (3) marker が別の NAME を言う repo と marker の無い repo では、`--pane` の有無の両方で同じ payload が 0 byte・rc 0・記録なし。
#[test]
fn hook_choice_question_is_silent_outside_the_vessel() {
    let other = git_repo();
    let marker = Marker { name: "other-vessel".to_owned(), version: GENERATION };
    fs::write(other.join(MARKER), marker.render()).expect("marker を書ける");
    let bare = git_repo();
    let state = tmp();
    let dir = state.display().to_string();
    for (repo, why) in [(&other, "他の name の marker"), (&bare, "marker の無い repo")] {
        let ask = ask_payload(repo, "");
        assert_silent(&run_hook_args(&["pre-tool-use", "--state-dir", &dir], &ask), &format!("{why}・pane 無し"));
        assert_silent(&run_hook_args(&["pre-tool-use", "--pane", "%999", "--state-dir", &dir], &ask), &format!("{why}・pane 在り"));
    }
    assert!(inject_lines(&state).is_empty(), "記録なし");
    clean(&[&other, &bare, &state]);
}

/// (4) question-route を持つ repo と持たない repo で、禁じた語列の Bash と write-set の外への Edit の rc・stdout・stderr が一致して
/// 既存の断りの字面のままで、選択式の問いの門の記録は 0 行。
#[test]
fn hook_choice_question_leaves_other_tools_unchanged() {
    let (routed, routed_state) = ask_place("question-route = \"route-x\"\n");
    let (plain, plain_state) = ask_place("");
    let outs: Vec<(Output, Output)> = [&routed, &plain]
        .iter()
        .map(|repo| {
            write_policy(repo, "src/lib.rs\n");
            let bash = run_hook("pre-tool-use", &bash_payload(repo, "git push --force origin main"));
            (bash, run_hook("pre-tool-use", &tool_payload(repo, "Edit", "docs/other.md")))
        })
        .collect();
    let [(routed_bash, routed_edit), (plain_bash, plain_edit)] = outs.as_slice() else {
        panic!("2 repo の出力");
    };
    for (left, right, why) in [(routed_bash, plain_bash, "Bash"), (routed_edit, plain_edit, "Edit")] {
        assert_eq!(left.status.code(), Some(i32::from(RC_BROKEN)), "{why}: 既存の門が断る");
        assert_eq!((left.status.code(), &left.stdout, &left.stderr), (right.status.code(), &right.stdout, &right.stderr), "{why}");
        assert!(!stderr_text(left).contains("choice-question") && !stderr_text(left).contains("route-x"), "{why}");
    }
    assert!(stderr_text(routed_bash).contains("host_guard.git"), "command guard の字面: {}", stderr_text(routed_bash));
    assert_eq!(stderr_text(routed_edit).trim_end(), format!("{NAME}: deny docs/other.md は契約 write-set の外（C16）"));
    assert!(choice_records(&routed_state).is_empty() && choice_records(&plain_state).is_empty(), "記録 0 行");
    clean(&[&routed, &routed_state, &plain, &plain_state]);
}
