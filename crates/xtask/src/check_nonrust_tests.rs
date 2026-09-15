//! `check` の歯（非 Rust 実行物・CI の shell 行）: `non_rust_exec_*` と `ci_shell_lines_*` を測る。
//! 共通の helper（`check_fixture` / `write_at` / `rules_manifest` 等）は親 module（`check_tests.rs`）に在る。
//! 純粋な移動（`check_tests.rs` から歯を足さずに写した・s2-07l.370）。札は file ごとに要る。

// flip-check: moved s2-07l.370

use super::*;

/// 非 Rust 実行物は **file 種別**で捕まえる（shebang / 実行 bit / 拡張子）。
///
/// 憲法 C12 の「歯は Rust 1 framework」は、歯以外の実行物から静かに崩れる。3 つの
/// 経路を**別々の file** で置くのは、1 つだけ見る実装（例: 拡張子しか見ない）が
/// 緑にならないようにするためである。
#[test]
fn non_rust_exec_denies_shebang_exec_bit_and_extension() {
    let violations = check_fixture(|dir| {
        write_at(dir, "tools/from-shebang", "#!/bin/sh\necho hi\n");
        write_at(dir, "tools/from-mode", "echo hi\n");
        write_at(dir, "tools/from-ext.py", "print('hi')\n");
        let mode = fs::metadata(dir.join("tools/from-mode"))
            .expect("fixture の file を読める")
            .permissions();
        let mut mode = mode;
        std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
        fs::set_permissions(dir.join("tools/from-mode"), mode).expect("実行 bit を立てられる");
    });
    for rel in ["tools/from-shebang", "tools/from-mode", "tools/from-ext.py"] {
        assert!(
            violations.iter().any(|line| line.starts_with("non-rust-exec:") && line.contains(rel)),
            "{rel} を非 Rust 実行物として名指す: {violations:?}"
        );
    }
}

/// 例外は **manifest の 1 面**（`repo.non_rust_exec_allow`）だけが持つ（憲法 C1）。
#[test]
fn non_rust_exec_allows_paths_listed_in_the_manifest_row() {
    let violations = check_fixture(|dir| {
        write_at(dir, "tools/from-ext.py", "print('hi')\n");
        write_at(dir, RULES_REL, &rules_manifest(&["tools/from-ext.py"]));
    });
    assert!(
        !violations.iter().any(|line| line.starts_with("non-rust-exec:")),
        "例外行に載る path は通る: {violations:?}"
    );
    // **完全一致**である（部分一致で通ると、例外 1 本が dir ごと素通しになる）。
    let partial = check_fixture(|dir| {
        write_at(dir, "tools/from-ext.py", "print('hi')\n");
        write_at(dir, RULES_REL, &rules_manifest(&["tools/from-ext"]));
    });
    assert!(
        partial.iter().any(|line| line.starts_with("non-rust-exec:")),
        "前方一致では通さない: {partial:?}"
    );
}

/// **母集団 0 は「測れなかった」**（`paths-clean` と同じ極性・0 件を緑にしない）。
#[test]
fn non_rust_exec_is_unmeasured_when_the_population_is_empty() {
    let dir = make_tmp_dir();
    write_healthy(&dir);
    git_track_all(&dir);
    // index を空にする（file は作業木に残るので「見に行けたが 0 件」の形になる）。
    assert!(git_fixture(&dir, &["rm", "-r", "-q", "--cached", "."]), "index を空にできる");
    let violations = check(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        violations
            .iter()
            .any(|line| line.starts_with("non-rust-exec:") && line.contains("0 件")),
        "母集団 0 は違反として名乗る: {violations:?}"
    );
}

/// CI の shell 行は **`run:` の 1 行形と block scalar の継続行**を数える（検出線）。
#[test]
fn ci_shell_lines_counts_run_lines_including_continuations() {
    let yml = "jobs:\n  a:\n    steps:\n      - run: cargo test\n      - run: |\n          echo one\n          echo two\n\n      - uses: actions/checkout@v4\n      - run: echo tail\n";
    assert_eq!(
        crate::non_rust_exec::count_run_lines(yml),
        4,
        "1 行形 2 本 + 継続行 2 本"
    );
    // 継続行の終わりは**字下げが浅くなった行**で決まる（次の step を数え込まない）。
    assert_eq!(
        crate::non_rust_exec::count_run_lines("      - run: |\n          echo one\n      - uses: x\n"),
        1,
        "block を抜けた行は数えない"
    );
}

/// 例外行の **`[` 〜 `]` の中だけ**を読む（末尾コメントの引用符を拾わない）。
///
/// `value = [...] # 旧 "x" は外した` の**注記が例外を 1 件増やす**形は、例外を減らす
/// 意図の文が静かに例外を足す＝憲法 C1 の「値の面は 1 つ」が崩れる（lens 2026-09-11 H2）。
#[test]
fn non_rust_exec_ignores_quotes_in_trailing_comments() {
    let violations = check_fixture(|dir| {
        write_at(dir, "tools/ghosted.py", "print('hi')\n");
        let row = rules_manifest(&[]);
        let commented = row.replace(
            "value = []",
            "value = [] # 旧 \"tools/ghosted.py\" は外した",
        );
        write_at(dir, RULES_REL, &commented);
    });
    assert!(
        violations.iter().any(|line| line.starts_with("non-rust-exec:") && line.contains("tools/ghosted.py")),
        "コメント内の引用符は例外にならない: {violations:?}"
    );
}

/// **腐った例外行を落とす**（憲法 C10.3: 未配線の設定は CI が落とす）。
///
/// 母集団に居ない path を例外に置けるままだと、file を land する前に例外だけ先に通せる。
#[test]
fn non_rust_exec_denies_allow_entries_that_match_nothing() {
    let violations = check_fixture(|dir| {
        write_at(dir, RULES_REL, &rules_manifest(&["tools/never-existed.sh"]));
    });
    assert!(
        violations
            .iter()
            .any(|line| line.starts_with("non-rust-exec:") && line.contains("never-existed")),
        "母集団に居ない例外は違反: {violations:?}"
    );
}

/// rules manifest を読めない tree は **fail-closed**（`paths-clean` と同じ極性）。
///
/// `check` 全体は閾値を読めない時点で止まる（`layout:` の 1 件・`s2-07l.163`）ので、measure 単体の
/// 極性は `Layout` を組んで直に撃って測る（check 経由では届かない）。
#[test]
fn non_rust_exec_is_unmeasured_when_the_manifest_is_missing() {
    let dir = make_tmp_dir();
    write_healthy(&dir);
    let _ = fs::remove_file(dir.join(RULES_REL));
    git_track_all(&dir);
    let whole = check(&dir);
    let layout = Layout::discover(&dir);
    let single = layout.as_ref().ok().map(crate::non_rust_exec::measure);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        whole.iter().any(|line| line.starts_with("layout: ") && line.contains("manifest.toml")),
        "閾値を読めない周は check 全体が止まる: {whole:?}"
    );
    let single = single.unwrap_or_else(|| panic!("fixture の Layout を組める: {}", layout.err().unwrap_or_default()));
    assert!(
        single
            .violations
            .iter()
            .any(|line| line.starts_with("non-rust-exec:") && line.contains("manifest")),
        "manifest を読めない周は違反として名乗る: {:?}",
        single.violations
    );
}

/// 閾値の行を 1 本欠いた manifest は `check` 全体を止め、欠いた行 id を名指す（SRS FR18）。
#[test]
fn check_is_blocked_when_a_threshold_row_is_missing() {
    let violations = check_fixture(|dir| {
        let dropped = rules_manifest(&[]).replace("id = \"R-C4-3\"", "id = \"R-C4-9\"");
        write_at(dir, RULES_REL, &dropped);
    });
    assert_single(&violations, "layout");
    let head = violations.first().map(String::as_str).unwrap_or_default();
    assert!(head.contains("R-C4-3"), "欠いた行 id を名指す: {head}");
}

/// 判定行に clippy-thresholds と dep-budget の fact が並び、`clippy.toml` を緩めると違反が
/// key と両値を名指す（写しの実効値を manifest が縛る・憲法 C14.2）。
#[test]
fn check_fails_when_clippy_toml_loosens_a_manifest_threshold() {
    let line = summary_fixture(|_| {});
    assert!(line.contains(" clippy-thresholds=ok "), "{line}");
    assert!(
        line.contains(&format!(" dep-budget={}/{} ", ALLOWED_DEPS.len(), real_limits().dep_budget)),
        "{line}"
    );
    let violations = check_fixture(|dir| {
        let loosened = clippy_toml().replace(
            &format!("too-many-lines-threshold = {}", real_limits().fn_lines),
            "too-many-lines-threshold = 600",
        );
        write_at(dir, "clippy.toml", &loosened);
    });
    assert_single(&violations, "clippy-thresholds");
    let head = violations.first().map(String::as_str).unwrap_or_default();
    assert!(head.contains("too-many-lines-threshold = 600") && head.contains("R-C4-4.fn-lines"), "{head}");
}

/// 分類器の拡張子は**閉じた列**で、`js` を外さない（契約が名指しで禁じた形）。
///
/// asset は**例外行に載せて**通す。定義を緩めると次の asset が黙って通る。
#[test]
fn non_rust_exec_pins_the_extension_list() {
    let sample = |rel: &str| crate::paths_clean::TrackedFile {
        rel: rel.to_owned(),
        mode: "100644".to_owned(),
        oid: String::new(),
    };
    for ext in ["sh", "bash", "zsh", "py", "bats", "pl", "rb", "js", "ts", "mjs"] {
        assert!(
            crate::non_rust_exec::is_non_rust_exec(&sample(&format!("a/b.{ext}")), None),
            "{ext} は分類器の内側"
        );
    }
    assert!(
        !crate::non_rust_exec::is_non_rust_exec(&sample("a/b.rs"), None),
        "Rust は非 Rust 実行物ではない"
    );
}

/// 判定行の**値**まで測る（検出線が恒久 0 になっても緑、を防ぐ）。
#[test]
fn non_rust_exec_and_ci_shell_lines_report_counts_in_the_fact_line() {
    let dir = make_tmp_dir();
    write_healthy(&dir);
    write_at(&dir, "tools/one.py", "print('hi')\n");
    write_at(&dir, RULES_REL, &rules_manifest(&["tools/one.py"]));
    write_at(&dir, ".github/workflows/ci.yml", "jobs:\n  a:\n    steps:\n      - run: cargo test\n");
    git_track_all(&dir);
    let line = summary(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        line.contains("non-rust-exec=1/") && line.contains("allow=1"),
        "該当 / 母集団 / 例外数を判定行へ出す: {line}"
    );
    assert!(line.contains("ci-shell-lines=1"), "検出線の値を判定行へ出す: {line}");
}

/// 拡張子の照合は **大文字小文字を区別しない**（`.PY` は「拡張子 py の file」である）。
///
/// 線引きの問題ではなく**同じ signal の取り落とし**（lens 2026-09-11・planner 裁定で本便の射程）。
#[test]
fn non_rust_exec_matches_extensions_case_insensitively() {
    let violations = check_fixture(|dir| {
        write_at(dir, "tools/shouty.PY", "print('hi')\n");
    });
    assert!(
        violations.iter().any(|line| line.starts_with("non-rust-exec:") && line.contains("shouty.PY")),
        "大文字の拡張子も分類器の内側: {violations:?}"
    );
}

/// **BOM 付きの shebang も shebang である**（先頭 3 byte の BOM で見落とさない）。
#[test]
fn non_rust_exec_sees_shebang_behind_a_bom() {
    let violations = check_fixture(|dir| {
        write_at(dir, "tools/bom-script", "\u{feff}#!/bin/sh\necho hi\n");
    });
    assert!(
        violations.iter().any(|line| line.starts_with("non-rust-exec:") && line.contains("bom-script")),
        "BOM の後ろの shebang も見る: {violations:?}"
    );
}
