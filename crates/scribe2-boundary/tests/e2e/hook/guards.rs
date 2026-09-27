// flip-check: moved s2-07l.679
//! guard の族の歯（接頭辞 `host_guard_` / `hook_guard_` / `hook_command_` / `hook_memo_` / `hook_ledger_`・設計 docs/design/carry-prep.md §9 行 g）。

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
    let repo = git_repo();
    let state = tmp();
    let out = run_host_guard_in(&state, &bash_payload(&repo, "git push --force-with-lease origin feat/x"));
    assert_silent(&out, "--force-with-lease は通す");
    let out = run_host_guard_in(&state, &tool_payload(&repo, "Read", "src/lib.rs"));
    assert_silent(&out, "Read は判定に載らない");
    assert!(inject_lines(&state).is_empty(), "通す周は記録を残さない");
    clean(&[&repo, &state]);
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
/// 揃えた後の `gh pr merge 1` は通る。
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
    assert_silent(&live_hook(&repo, &bash_payload(&repo, "gh pr merge 1")), "窓が開いた");
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
