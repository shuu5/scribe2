//! `flipcheck` の歯。**本体は `flipcheck.rs`** で、ここには test だけが在る。
//!
//! 分けたのは憲法 C4（1 file の上限）である——判定の実装と歯が同じ file に載ると
//! 上限に当たり、次に歯を足す便が「上限に入らないから書かない」へ倒れる。
//! `#[path]` で `flipcheck` の子 module として取り込むので、module path は
//! `flipcheck::tests` のまま＝歯の名前は 1 つも変わらない。
//!
//! この file は `#[cfg(test)] mod` の形を持たないが、`crates/*/src/**/*_tests.rs` は
//! **名前で test file と見なして丸ごと写す**（s2-07l.34 の (6)）ので、ここへ足した歯は
//! base へ写り flip を検査される。名前で見なければ区間判定には src 区間だけの file に
//! 見え、ここへ足した歯が 1 本も測られないままになる。

// 純粋な移動（`flipcheck.rs` の git / tar の群を `flipcheck/git.rs` へ・歯は足していない・s2-07l.372）。
// flip-check: moved s2-07l.372
// 純粋な移動（`flipcheck.rs` の nextest を撃って出力を読む群を `flipcheck/nextest.rs` へ・歯は足していない・s2-07l.500）。
// flip-check: moved s2-07l.500

use super::{
    base_not_green, changed_rs, failed_tests, fresh_marks, is_bead_id, is_test_file, judge, judge_into,
    load_pairs, nextest_args, parse_base, split_regions, write_one, BaseNotGreen, FailedTest, FilePair, Verdict,
    RETROACTIVE_MARK,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 合成 workspace の member 名（実 NAME の字面を .rs へ持ち込まない別名）。
const FIXTURE_MEMBER: &str = "flipdemo";
/// 合成 workspace の toolchain channel。
const FIXTURE_CHANNEL: &str = "1.98.1";
/// base 側の lib.rs（通る test を 1 本持つ＝base 健全性前段が rc 0 になる）。
const BASE_LIB: &str = "pub fn val() -> u32 {\n    1\n}\n#[cfg(test)]\nmod checks {\n    #[test]\n    fn holds() {\n        assert_eq!(super::val(), 1);\n    }\n}\n";

/// 同一 process 内での dir 名衝突を避ける連番。
static SEQ: AtomicU32 = AtomicU32::new(0);

/// repo の外に一意な tmp dir を作る。
fn make_tmp_dir() -> PathBuf {
    let base = std::env::temp_dir();
    for _ in 0..8 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("xtask-flip-{}-{nanos}-{seq}", std::process::id()));
        if std::fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
    panic!("合成 workspace 用の tmp dir を作れない");
}

/// fixture 内で git を撃つ（identity と署名を明示し外の設定に依存しない）。
fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// `rel` へ本文を書く（親 dir は作る）。
fn write_at(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("合成 workspace の dir を作れる");
    }
    std::fs::write(&path, body).expect("合成 workspace の file を書ける");
}

/// core crate の `src/lib.rs` の相対 path。
fn lib_rel() -> String {
    format!("crates/{FIXTURE_MEMBER}/src/lib.rs")
}

/// base commit を積んだ合成 workspace を作り、その dir と base の SHA を返す。
fn base_commit() -> (PathBuf, String) {
    let dir = make_tmp_dir();
    scaffold(&dir);
    let base = seed_fixture(&dir, BASE_LIB);
    (dir, base)
}

/// 合成 workspace の骨組み（workspace / toolchain / member の manifest）を書く。
///
/// **`judge_lib` と共有する**——片方だけが書くと、flip が 1 本でも立つ便で base tree が
/// 不完全になり、判定行が `reason=infra-error` に化ける。免除を数えなかったことを
/// `!line.contains("moved=")` で測る負例は、そのとき**判定路へ 1 度も入らないまま自動的に
/// 真**になる（実測 2026-09-11: `moved=0` を出す変異が 28/28 緑のまま生存した）。
fn scaffold(dir: &Path) {
    write_at(
        dir,
        "Cargo.toml",
        &format!("[workspace]\nresolver = \"2\"\nmembers = [\"crates/{FIXTURE_MEMBER}\"]\n"),
    );
    write_at(
        dir,
        "rust-toolchain.toml",
        &format!("[toolchain]\nchannel = \"{FIXTURE_CHANNEL}\"\n"),
    );
    write_at(
        dir,
        &format!("crates/{FIXTURE_MEMBER}/Cargo.toml"),
        &format!(
            "[package]\nname = \"{FIXTURE_MEMBER}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
        ),
    );
    // 免除経路の上限は rules 行から読む（読めない周は infra-error）ので、現物の manifest をそのまま置く。
    write_at(dir, "rules/manifest.toml", &real_manifest());
}

/// 現物の `rules/manifest.toml`（workspace root は この crate の 2 つ上）。
fn real_manifest() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../rules/manifest.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{} を読めない: {err}", path.display()))
}

/// base と HEAD の lib 本文を与えて 1 便を判定する（marker まわりの負例で使い回す）。
fn judge_lib(base_lib: &str, head_lib: &str) -> Verdict {
    let dir = make_tmp_dir();
    scaffold(&dir);
    let base = seed_fixture(&dir, base_lib);
    write_at(&dir, &lib_rel(), head_lib);
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    got
}

/// marker を数えなかったことを確かめる（負例の共通 assert）。
fn assert_not_retroactive(got: &Verdict, what: &str) {
    assert_eq!(got.code, 1, "{what} で通してはならない: {}", got.line);
    assert!(!got.line.contains("retroactive"), "{what} を数えない: {}", got.line);
}

/// 既に組んだ fixture dir へ lib を書いて base commit を作る。
fn seed_fixture(dir: &Path, lib: &str) -> String {
    write_at(dir, &lib_rel(), lib);
    write_at(dir, "README.md", "fixture\n");
    if !dir.join(".git").exists() {
        assert!(git(dir, &["init", "-q"]), "fixture で git init できる");
    }
    assert!(git(dir, &["add", "-A"]), "fixture で git add できる");
    assert!(
        git(dir, &["commit", "-q", "-m", "base"]),
        "fixture で base を commit できる"
    );
    head_sha(dir)
}

/// fixture の HEAD の SHA。
fn head_sha(dir: &Path) -> String {
    let sha = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse を起動できる");
    let found = String::from_utf8_lossy(&sha.stdout).trim().to_owned();
    assert!(!found.is_empty(), "HEAD の SHA を読める");
    found
}

/// HEAD 側を書いて commit する。
fn head_commit(dir: &Path) {
    assert!(git(dir, &["add", "-A"]), "fixture で HEAD を add できる");
    assert!(
        git(dir, &["commit", "-q", "-m", "head"]),
        "fixture で HEAD を commit できる"
    );
}

/// fixture を使い切りにする。
fn drop_fixture(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// 判定 1 行が `reason=<語>` を持ち rc が期待どおりであることを表明する。
fn assert_verdict(line: &str, code: u8, want_code: u8, want: &str) {
    assert_eq!(code, want_code, "rc が期待と違う: {line}");
    assert!(line.contains(want), "判定行に {want} が無い: {line}");
}

// 歯は判定クラスごとの子 module（`flipcheck_<主題>_tests.rs`・`#[path]` で取り込む）に在り、
// この file は共通の helper と宣言だけを持つ（s2-07l.91・純粋な移動・C4 R-C4-2）。
// flip-check: moved s2-07l.91

#[path = "flipcheck_entrance_tests.rs"]
mod entrance;

#[path = "flipcheck_overlay_tests.rs"]
mod overlay;

#[path = "flipcheck_retroactive_tests.rs"]
mod retroactive;

#[path = "flipcheck_declaration_tests.rs"]
mod declaration;

#[path = "flipcheck_moved_tests.rs"]
mod moved;

// ---- 免除経路の閉じ方（s2-07l.170・設計 docs/design/pipeline.md §7 約束 1 / 2）----
//
// docs-only の面（rules 行 `flip.docs_only_faces`）と札の形と本数（rules 行 `flip.marks_per_pr`）。子 module を
// 足すと `#[path]` の新規 module は flip されない（not-flippable）ので、この便の歯は親 file のここへ置く。

/// base（[`BASE_LIB`]・`manifest` を置いた rules）を積み、HEAD に `head` の (path, 本文) を書いて判定する。
/// 判定と sink の行を返す。`manifest` が `None` の周は現物の manifest のまま。
fn judge_with(manifest: Option<&str>, head: &[(&str, &str)]) -> (Verdict, Vec<String>) {
    let dir = make_tmp_dir();
    scaffold(&dir);
    if let Some(text) = manifest {
        write_at(&dir, "rules/manifest.toml", text);
    }
    let base = seed_fixture(&dir, BASE_LIB);
    for (rel, body) in head {
        write_at(&dir, rel, body);
    }
    head_commit(&dir);
    let mut lines: Vec<String> = Vec::new();
    let got = judge_into(&base, &dir, &mut |line| lines.push(line.to_owned()));
    drop_fixture(&dir);
    (got, lines)
}

/// 現物の manifest の flip の 2 行だけを差し替えた本文（面の列と札の上限）。
fn flip_manifest(faces: &str, marks: u64) -> String {
    // 行 id の後ろで最初の `value = ` の行を差し替える（行の位置は差し替えのたびに引き直す）。
    let replace_value = |text: &str, id: &str, value: &str| -> String {
        let from = text.find(&format!("id = \"{id}\"")).expect("行が在る");
        let start = from + text[from..].find("\nvalue = ").expect("value の行が在る") + 1;
        let end = start + text[start..].find('\n').expect("value の行が閉じる");
        format!("{}value = {value}{}", &text[..start], &text[end..])
    };
    let text = replace_value(&real_manifest(), "flip.docs_only_faces", faces);
    replace_value(&text, "flip.marks_per_pr", &marks.to_string())
}

/// test 区間へ札 `mark`（`retroactive` / `moved`）を `beads` の数だけ置き、base で緑の歯を 1 本足した lib。
fn lib_with_marks(mark: &str, beads: &[&str]) -> String {
    let lines: String = beads.iter().map(|bead| format!("    // flip-check: {mark} {bead}\n")).collect();
    BASE_LIB.replace(
        "mod checks {\n",
        &format!("mod checks {{\n{lines}    #[test]\n    fn added_later() {{\n        assert_eq!(super::val(), 1);\n    }}\n"),
    )
}

/// (1) `.rs` の差が無くても、面の外の file を 1 本でも含む便は `no-test-diff` で落ち（rc 1）、面の外の path を
/// sink に名指す。判定行は `skip` を名乗らない（`.rs` の有無だけで通す経路は消えた）。
#[test]
fn flip_check_docs_only_outside_face_fails_without_rust_diff() {
    // `docs.md` は接頭辞の面 `docs/` に字面だけ近い path（`/` までを面と読む）。
    for rel in ["plugin/hooks.json", "rules/extra.toml", "notes/keep.md", "docs.md"] {
        let (got, lines) = judge_with(None, &[("docs/design/x.md", "x\n"), (rel, "outside\n")]);
        assert_verdict(&got.line, got.code, 1, "FAIL reason=no-test-diff");
        assert!(!got.line.contains("skip") && !got.line.contains("no-rust-diff"), "{}", got.line);
        assert_eq!(lines, vec![format!("flip-check: outside-docs-faces {rel}")], "面の外の path だけを名指す");
    }
}

/// (1) 面の中だけの便（接頭辞の面と完全一致の面）は従来どおり rc 0 で通り、判定行は `RED-on-base ok` の形に
/// `docs-only=N`（動いた path の本数）を後置する（`skip reason=no-rust-diff` の語は判定の語彙から消えた）。
#[test]
fn flip_check_docs_only_inside_faces_passes_with_the_count() {
    let (got, lines) = judge_with(
        None,
        &[("README.md", "touched\n"), ("docs/design/x.md", "x\n"), ("design-intent/spec/y.html", "y\n")],
    );
    assert_verdict(&got.line, got.code, 0, "flip-check: RED-on-base ok tests_changed=0 docs-only=3");
    assert!(!got.line.contains("skip") && !got.line.contains("no-rust-diff"), "{}", got.line);
    assert!(lines.is_empty(), "面の外の path は無い: {lines:?}");
}

/// (1) 面は rules 行 `flip.docs_only_faces` から読む（字面を実装に焼かない）: 面を `notes/` だけにした manifest では
/// `notes/` の便が通り `README.md` の便が落ちる。行を読めない manifest は測れなかった（infra-error）で、既定に倒さない。
#[test]
fn flip_check_docs_only_faces_come_from_the_rules_row() {
    let manifest = flip_manifest("[\"notes/\"]", 16);
    let (inside, _) = judge_with(Some(&manifest), &[("notes/keep.md", "note\n")]);
    assert_verdict(&inside.line, inside.code, 0, "docs-only=1");
    let (outside, _) = judge_with(Some(&manifest), &[("README.md", "touched\n")]);
    assert_verdict(&outside.line, outside.code, 1, "reason=no-test-diff");
    let broken = real_manifest().replace("id = \"flip.docs_only_faces\"", "id = \"flip.renamed\"");
    let (unread, _) = judge_with(Some(&broken), &[("README.md", "touched\n")]);
    assert_verdict(&unread.line, unread.code, 1, "reason=infra-error");
    assert!(unread.line.contains("flip.docs_only_faces"), "読めない行を名指す: {}", unread.line);
}

/// (2) 札の bead id の形は閉じている: `<接頭辞>-<段>(.<段>)*`（英小文字と数字）。
#[test]
fn flip_check_marks_bead_id_shape_is_closed() {
    for good in ["s2-07l.91", "s2-07l.479.2", "s2-07l.37x", "s2-07l", "ab-c"] {
        assert!(is_bead_id(good), "形に合う: {good}");
    }
    for bad in ["TODO", "s2", "S2-07l.91", "s2-07l..3", "s2-07l.", "-07l", "2s-07l", "s2-07l.91 (理由)", "s2_07l.9", "s2-07L"] {
        assert!(!is_bead_id(bad), "形に合わない: {bad}");
    }
}

/// (2) 形に合わない札は免除を与えず `bad-marker file=<rel>` で落ち（rc 1）、sink に札の id を名指す。
#[test]
fn flip_check_marks_bad_shape_fails_as_bad_marker() {
    for mark in ["retroactive", "moved"] {
        let (got, lines) = judge_with(None, &[(&lib_rel(), &lib_with_marks(mark, &["TODO"]))]);
        assert_verdict(&got.line, got.code, 1, &format!("FAIL reason=bad-marker file={}", lib_rel()));
        assert!(!got.line.contains(&format!("{mark}=")), "免除を数えない: {}", got.line);
        assert!(lines.iter().any(|line| line.starts_with("flip-check: bad-marker ") && line.contains("\"TODO\"")), "{lines:?}");
    }
}

/// (2) 便が足した札の本数が rules 行 `flip.marks_per_pr` を超えると `too-many-marks` で落ち（rc 1）、本数と上限を
/// 名指す。上限ちょうどは従来どおり免除が効く（境界の両側）。
#[test]
fn flip_check_marks_over_the_rules_row_fail_as_too_many_marks() {
    let manifest = flip_manifest("[\"docs/\"]", 2);
    let (over, _) = judge_with(Some(&manifest), &[(&lib_rel(), &lib_with_marks("retroactive", &["s2-07l.1", "s2-07l.2", "s2-07l.3"]))]);
    assert_verdict(&over.line, over.code, 1, "FAIL reason=too-many-marks marks=3 limit=2");
    let (at, _) = judge_with(Some(&manifest), &[(&lib_rel(), &lib_with_marks("retroactive", &["s2-07l.1", "s2-07l.2"]))]);
    assert_verdict(&at.line, at.code, 0, "RED-on-base ok tests_changed=0 retroactive=1");
}

/// (2) 形も本数も満たす札は従来どおり免除が効く（現物の manifest の上限の下・両方の札）。
#[test]
fn flip_check_marks_within_shape_and_count_keep_the_exemption() {
    let (retro, lines) = judge_with(None, &[(&lib_rel(), &lib_with_marks("retroactive", &["s2-07l.170"]))]);
    assert_verdict(&retro.line, retro.code, 0, "RED-on-base ok tests_changed=0 retroactive=1");
    assert!(!lines.iter().any(|line| line.contains("bad-marker")), "{lines:?}");
    let (moved, _) = judge_with(None, &[(&lib_rel(), &lib_with_marks("moved", &["s2-07l.479.2"]))]);
    assert_verdict(&moved.line, moved.code, 0, "RED-on-base ok tests_changed=0 moved=1");
}

// ---- base 段の撃ち直し（s2-07l.270・負荷下の flaky の検出線）----
//
// 子 module を足すと `#[path]` の新規 module は flip されない（not-flippable）ので、
// この便の歯は親 file のここへ置く。

/// `judge_into` を撃ち、判定と sink の `base-retry` 行だけを返す。
fn judge_with_retry_lines(base: &str, dir: &Path) -> (Verdict, Vec<String>) {
    let mut lines: Vec<String> = Vec::new();
    let got = judge_into(base, dir, &mut |line| lines.push(line.to_owned()));
    let retries = lines
        .into_iter()
        .filter(|line| line.starts_with("flip-check: base-retry "))
        .collect();
    (got, retries)
}

/// [`BASE_LIB`] の `mod checks` へ歯を 1 本足した本文。
fn base_lib_with(test_fn: &str) -> String {
    BASE_LIB.replace("mod checks {\n", &format!("mod checks {{\n{test_fn}"))
}

/// **1 回目だけ落ちる**歯（`CARGO_MANIFEST_DIR` 直下の marker file を作って落ち、2 回目は
/// marker が在るので通る）。base copy は便ごとに実体化し直すので、1 回目は必ず marker 無し。
const FLAKY_ONCE: &str = "    #[test]\n    fn settles() {\n        let marker = std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"settles.marker\");\n        if marker.exists() {\n            return;\n        }\n        std::fs::write(&marker, \"x\").expect(\"marker を書ける\");\n        panic!(\"first run\");\n    }\n";

/// 常に落ちる歯。
const ALWAYS_RED: &str = "    #[test]\n    fn broken() {\n        panic!(\"always\");\n    }\n";

/// nextest の出力から `FAIL [ <time>] <binary id> <歯の名>` 形の行だけを拾い、末尾の一覧で
/// 繰り返される同じ歯は 1 本に畳む。`PASS` / `Summary` / `TRY n FAIL` / 語数の崩れた行は拾わない。
///
/// 進捗の `(n/m)` は桁を揃える空白を挟む（実測 2026-09-14: `( 288/1146)`——語で割ると
/// `(` と `288/1146)` の 2 語に化け、1 語として除く実装は本物の FAIL 行を 1 本も拾えず
/// `base-not-green` へ落ちた）。fixture は実出力の形で pin する。
#[test]
fn flip_check_parses_failed_tests_from_nextest_output() {
    let text = "\
────────────
 Nextest run ID 0 with nextest profile: default
    Starting 3 tests across 2 binaries
        PASS [   0.012s] (   1/1146) flipdemo checks::holds
        FAIL [   0.010s] ( 288/1146) flipdemo checks::settles
  TRY 1 FAIL [   0.010s] ( 289/1146) flipdemo checks::retried
        FAIL [   0.011s] (3/3) flipdemo::it green
        FAIL [   0.011s] broken
     Summary [   0.013s] 3 tests run: 1 passed, 2 failed, 0 skipped
        FAIL [   0.010s] ( 288/1146) flipdemo checks::settles
        FAIL [   0.011s] flipdemo::it green
error: test run failed
";
    let got = failed_tests(text);
    let want = vec![
        FailedTest {
            binary: "flipdemo".to_owned(),
            name: "checks::settles".to_owned(),
        },
        FailedTest {
            binary: "flipdemo::it".to_owned(),
            name: "green".to_owned(),
        },
    ];
    assert_eq!(got, want, "FAIL 行 2 本を出力順に・重複は畳んで拾うはず");
    assert!(
        failed_tests("        PASS [   0.012s] (1/1) flipdemo checks::holds\n     Summary [   0.013s] 1 test run: 1 passed\n").is_empty(),
        "落ちた歯が無い出力からは 1 本も拾わない"
    );
    assert!(
        failed_tests("error[E0308]: mismatched types\nerror: could not compile `flipdemo`\n").is_empty(),
        "compile error の出力からは 1 本も拾わない"
    );
}

/// base の歯が **1 回目だけ**落ちる周は、その歯だけを 1 回撃ち直して base 緑と読み、判定行に
/// `base-retried=N` を後置する（負荷下の flaky が `base-not-green` → retire → 再走を踏まない）。
#[test]
fn flip_check_retries_flaky_base_test_once_and_reports_count() {
    let dir = make_tmp_dir();
    scaffold(&dir);
    let base_lib = base_lib_with(FLAKY_ONCE);
    let base = seed_fixture(&dir, &base_lib);
    // HEAD: 既存の歯（holds）の期待値だけを変え、base の src（val() は 1）で赤い flip を 1 本作る。
    write_at(
        &dir,
        &lib_rel(),
        &base_lib.replace(
            "        assert_eq!(super::val(), 1);\n",
            "        assert_eq!(super::val(), 2);\n",
        ),
    );
    head_commit(&dir);
    let (got, retries) = judge_with_retry_lines(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok tests_changed=1 base-retried=1");
    assert_eq!(
        retries,
        vec![format!("flip-check: base-retry {FIXTURE_MEMBER}::checks::settles")],
        "撃ち直した歯を binary::name で 1 行ずつ名指すはず"
    );
}

/// **2 回目も落ちる**歯は撃ち直しで緑に化けない——従来どおり `base-not-green`（rc 1）。
/// 撃ち直しは 1 回だけで、その 1 回は sink に残る（極性の pin・補助の歯）。
#[test]
fn flip_check_base_retry_does_not_rescue_a_test_that_fails_twice() {
    let dir = make_tmp_dir();
    scaffold(&dir);
    let base_lib = base_lib_with(ALWAYS_RED);
    let base = seed_fixture(&dir, &base_lib);
    write_at(
        &dir,
        &lib_rel(),
        &base_lib.replace(
            "        assert_eq!(super::val(), 1);\n",
            "        assert_eq!(super::val(), 2);\n",
        ),
    );
    head_commit(&dir);
    let (got, retries) = judge_with_retry_lines(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "FAIL reason=infra-error base-not-green");
    assert!(
        !got.line.contains("base-retried"),
        "落ちたままの撃ち直しを判定行へ載せない: {}",
        got.line
    );
    assert_eq!(
        retries,
        vec![format!("flip-check: base-retry {FIXTURE_MEMBER}::checks::broken")],
        "撃ち直しは 1 回だけ（2 回目を撃たない）"
    );
}

/// 落ちた歯を **名指せない**周（base が compile しない＝rc 101・`FAIL` 行が無い）は撃ち直さず
/// `base-not-green` で止まる——sink に `base-retry` 行が 0。
#[test]
fn flip_check_base_retry_needs_named_failures() {
    let dir = make_tmp_dir();
    scaffold(&dir);
    // src 区間が型を誤り compile できない base（test 区間は BASE_LIB のまま）。
    let base_lib = BASE_LIB.replace("    1\n}", "    \"one\"\n}");
    let base = seed_fixture(&dir, &base_lib);
    write_at(
        &dir,
        &lib_rel(),
        &base_lib.replace(
            "        assert_eq!(super::val(), 1);\n",
            "        assert_eq!(super::val(), 2);\n",
        ),
    );
    head_commit(&dir);
    let (got, retries) = judge_with_retry_lines(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "FAIL reason=infra-error base-not-green");
    assert!(
        retries.is_empty(),
        "名指せない失敗は撃ち直さない（sink に base-retry が {} 行）: {retries:?}",
        retries.len()
    );
}

// ---- base-not-green の経路の弁別子（s2-07l.380・設計 docs/design/pipeline.md §32）----
//
// 「名指せない失敗」の 3 経路が同じ字面へ倒れると、操作役が負荷 / 環境 / 本物の赤を判定行から
// 分けられない。経路の写像は純関数で測り（実 signal と実 retry-failed の fixture は壁時計と
// 環境に依るので置かない）、判定行まで通る形は compile error の実 fixture 1 本で測る。

/// 理由の純関数が 3 経路を**宣言順**の variant へ写し、後置の字面を 1 つずつ持つ。
///
/// 入力は (base の rc・名指した歯の本数・撃ち直しの rc)。(i) rc 無し＝signal ／(ii) rc≠0 で
/// 名指し 0 ／(iii) 名指した歯の撃ち直しが rc≠0。
#[test]
fn flipcheck_base_reason_maps_three_paths_in_declaration_order() {
    let cases: [(Option<i32>, usize, Option<i32>); 3] =
        [(None, 0, None), (Some(101), 0, None), (Some(1), 2, Some(1))];
    let got: Vec<(BaseNotGreen, String)> = cases
        .into_iter()
        .map(|(base_rc, named, retry_rc)| {
            let path = base_not_green(base_rc, named, retry_rc);
            (path, path.label())
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (BaseNotGreen::Signal, "base-not-green:signal".to_owned()),
            (BaseNotGreen::Unnamed { rc: 101 }, "base-not-green:unnamed rc=101".to_owned()),
            (BaseNotGreen::RetryFailed { rc: 1 }, "base-not-green:retry-failed rc=1".to_owned()),
        ],
        "(i) signal (ii) 名指し 0 (iii) 撃ち直しの赤 の順に写し、字面は後置だけのはず"
    );
}

/// base が compile しない周（rc 101・`FAIL` 行が無い＝名指せない）の判定行が
/// `base-not-green:unnamed rc=101` を名指す。判定行の先頭は不変（後置だけ）。
#[test]
fn flipcheck_base_reason_compile_error_names_unnamed_rc() {
    let dir = make_tmp_dir();
    scaffold(&dir);
    // src 区間が型を誤り compile できない base（test 区間は BASE_LIB のまま）。
    let base_lib = BASE_LIB.replace("    1\n}", "    \"one\"\n}");
    let base = seed_fixture(&dir, &base_lib);
    write_at(
        &dir,
        &lib_rel(),
        &base_lib.replace(
            "        assert_eq!(super::val(), 1);\n",
            "        assert_eq!(super::val(), 2);\n",
        ),
    );
    head_commit(&dir);
    let (got, retries) = judge_with_retry_lines(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=infra-error base-not-green:unnamed rc=101");
    assert!(
        got.line.starts_with("flip-check: FAIL reason=infra-error "),
        "判定行の先頭は不変（弁別子は後置だけ）: {}",
        got.line
    );
    assert!(
        retries.is_empty(),
        "名指せない失敗は撃ち直さない（sink に base-retry が {} 行）: {retries:?}",
        retries.len()
    );
}

// ---- 子の出力の色（s2-07l.276・CI の `CARGO_TERM_COLOR=always` で main が赤）----

/// CI の実出力と同じ色付きの `FAIL` 行（`ESC[31;1m … ESC[0m`・binary と歯の名にも色が付く）
/// から、色なしの同じ行と同じ 1 本が取れる。base の parser は `FAIL [` 接頭辞が色で隠れて
/// 0 本＝「名指せない失敗」として `base-not-green` へ倒れる（実測 2026-09-14・run 34856756368）。
#[test]
fn flip_check_parses_failed_tests_under_color_escapes() {
    let colored = "\x1b[31;1m        FAIL\x1b[0m [   0.316s] (1053/1153) \x1b[35;1mflipdemo\x1b[0m \x1b[36mchecks\x1b[0m\x1b[36m::\x1b[0m\x1b[34;1mbroken\x1b[0m\n";
    let plain = "        FAIL [   0.316s] (1053/1153) flipdemo checks::broken\n";
    let want = vec![FailedTest {
        binary: "flipdemo".to_owned(),
        name: "checks::broken".to_owned(),
    }];
    assert_eq!(failed_tests(colored), want, "色付きの FAIL 行から 1 本名指せるはず");
    assert_eq!(
        failed_tests(colored),
        failed_tests(plain),
        "色付きと色なしで同じ結果になるはず"
    );
    assert!(
        failed_tests("\x1b[32;1m        PASS\x1b[0m [   0.012s] (1/1) \x1b[35;1mflipdemo\x1b[0m checks::holds\n").is_empty(),
        "色付きでも PASS 行は拾わない"
    );
}

/// 子の `cargo nextest run` の引数の列に `--color never` が隣接して在る（親の env に依らず
/// 出力を機械形にする）。`extra` はその後ろへ足される。
#[test]
fn flip_check_child_nextest_disables_color() {
    let args = nextest_args(&["-E", "test(=x)"]);
    let at = args
        .iter()
        .position(|arg| arg == "--color")
        .expect("子の引数に --color が在る");
    assert_eq!(args.get(at + 1).map(String::as_str), Some("never"), "--color の直後は never: {args:?}");
    assert_eq!(args.first().map(String::as_str), Some("nextest"), "先頭は nextest: {args:?}");
    assert!(args.contains(&"--no-tests=fail".to_owned()), "--no-tests=fail を落とさない: {args:?}");
    assert_eq!(
        &args[args.len() - 2..],
        ["-E", "test(=x)"],
        "extra は末尾へ足される: {args:?}"
    );
}

/// 子の `cargo nextest run` の引数の列に `--no-fail-fast` が **1 つ**在る（設計 gate-cost.md §19・
/// 憲法 C10: 歯 1 本の flaky で残りを未実行のまま終えず、落ちた歯の全数を名指す）。
/// `--color never` の隣接・`--no-tests=fail`・先頭の `nextest` は不変。
#[test]
fn no_fail_fast_is_in_flipcheck_nextest_args() {
    let args = nextest_args(&[]);
    assert_eq!(
        args.iter().filter(|arg| *arg == "--no-fail-fast").count(),
        1,
        "--no-fail-fast は 1 つだけ: {args:?}"
    );
    let at = args
        .iter()
        .position(|arg| arg == "--color")
        .expect("子の引数に --color が在る");
    assert_eq!(args.get(at + 1).map(String::as_str), Some("never"), "--color の直後は never のまま: {args:?}");
    assert_eq!(args.first().map(String::as_str), Some("nextest"), "先頭は nextest のまま: {args:?}");
    assert!(args.contains(&"--no-tests=fail".to_owned()), "--no-tests=fail を落とさない: {args:?}");
    // extra は `--no-fail-fast` より後ろ（filterset の撃ち直しでも fail-fast に戻らない）。
    let with_extra = nextest_args(&["-E", "test(=x)"]);
    let flag = with_extra.iter().position(|arg| arg == "--no-fail-fast").expect("撃ち直しの列にも在る");
    let extra = with_extra.iter().position(|arg| arg == "-E").expect("extra が在る");
    assert!(flag < extra, "--no-fail-fast は extra の前: {with_extra:?}");
}

/// 実 fixture の撃ち直しの歯 2 本は、子に `CARGO_TERM_COLOR=always` を載せた周でも緑になる。
///
/// 親 process の env は触らない——この test binary 自身を `Command` で撃ち、その env にだけ
/// 色を置く。子の `judge_into` が起動する `cargo nextest` はその env を継承するので、CI と
/// 同じ「色付きの FAIL 行」を読む周を手元で再現できる（実測 2026-09-14: base ではこの形で
/// 同じ 2 本が落ちた＝再現 1/1）。
#[test]
fn flip_check_base_retry_is_color_independent() {
    let exe = std::env::current_exe().expect("test binary の path を取れる");
    // libtest の歯の名は crate 名を含まない（`flipcheck::tests::<name>`）。
    let prefix = module_path!()
        .split_once("::")
        .map(|(_, rest)| rest)
        .expect("module path に crate 名の後ろが在る");
    let names = [
        "flip_check_retries_flaky_base_test_once_and_reports_count",
        "flip_check_base_retry_does_not_rescue_a_test_that_fails_twice",
    ];
    let output = Command::new(exe)
        .env("CARGO_TERM_COLOR", "always")
        .arg("--exact")
        .args(names.iter().map(|name| format!("{prefix}::{name}")))
        .output()
        .expect("test binary を撃てる");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("2 passed"),
        "色ありの子でも撃ち直しの歯 2 本は緑のはず: {}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        output.status
    );
}

// ---- base copy の tracked 集合（s2-07l.280・`git archive` の展開は `.git` を持たない）----
//
// base copy の中で `git ls-files` を撃つ歯（`contracts check` の実 repo 母集団）は、copy が
// git repo でないと外側の repo を見つけ `target/` 配下の 0 本を読む＝main が緑でも毎便
// `base-not-green`。fixture の base の歯そのものに `git ls-files` を撃たせ、base 段で測れる
// ことを実 fixture で確かめる。

/// base copy の root（fixture の `CARGO_MANIFEST_DIR` の 2 つ上）で `git ls-files` を撃つ
/// fixture の歯の共通部（`root` と `listed`＝一覧を持つ）。
const LS_FILES_SNIPPET: &str = "        let root = std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"..\").join(\"..\");\n        \
     let out = std::process::Command::new(\"git\").arg(\"-C\").arg(&root).arg(\"ls-files\").output().expect(\"git ls-files\");\n        \
     let listed = String::from_utf8_lossy(&out.stdout).into_owned();\n";

/// base copy の index に自分の `src/lib.rs` が載ることを assert する fixture の歯。
///
/// copy が git repo でない周は外側（fixture）の repo の `target/flipcheck/base` 配下＝0 本を
/// 読むので、この歯が base 段で落ち `base-not-green` になる（= base の xtask で RED）。
fn tracked_lib_test() -> String {
    format!(
        "    #[test]\n    fn tracked() {{\n{LS_FILES_SNIPPET}        assert!(listed.lines().any(|line| line == \"{}\"), \"index に lib.rs が無い: {{listed:?}}\");\n    }}\n",
        lib_rel()
    )
}

/// overlay が足す新規 test file の相対 path（base に無い `+` の file）。
fn extra_rel() -> String {
    format!("crates/{FIXTURE_MEMBER}/tests/extra.rs")
}

/// `git ls-files` の一覧・`git rev-parse HEAD`・「working tree に overlay の新規 file が
/// 在るか」を `probe` へ書く fixture の歯（assert はしない＝overlay 段の RED と混ざらないよう、
/// 外の歯が読む）。
///
/// base 段と overlay 段の 2 回走り、最後に書いた overlay 段の姿が残る。`has-extra=` の行は
/// 「新規 file が在る周に測った」ことの証拠で、不在の周に通る空虚な負例を塞ぐ。
fn probe_lib_test(probe: &Path) -> String {
    format!(
        "    #[test]\n    fn probe() {{\n{LS_FILES_SNIPPET}        let extra = root.join(\"{}\").is_file();\n        \
         let head = std::process::Command::new(\"git\").arg(\"-C\").arg(&root).args([\"rev-parse\", \"HEAD\"]).output().expect(\"git rev-parse\");\n        \
         let head = String::from_utf8_lossy(&head.stdout).trim().to_owned();\n        \
         std::fs::write(\"{}\", format!(\"{{listed}}has-extra={{extra}}\\nhead={{head}}\\n\")).expect(\"probe を書ける\");\n    }}\n",
        extra_rel(),
        probe.display()
    )
}

/// (i) base copy は base の tracked 集合を `git ls-files` で読める git repo である——
/// base の歯が copy の index に自分の `src/lib.rs` を見つけ、base 段が緑・HEAD の flip 1 本で
/// `RED-on-base ok tests_changed=1`。
#[test]
fn flip_check_base_copy_is_a_git_repo_with_the_tracked_set() {
    let dir = make_tmp_dir();
    scaffold(&dir);
    let base_lib = base_lib_with(&tracked_lib_test());
    let base = seed_fixture(&dir, &base_lib);
    // HEAD: 既存の歯（holds）の期待値だけを変え、base の src（val() は 1）で赤い flip を 1 本作る。
    write_at(
        &dir,
        &lib_rel(),
        &base_lib.replace(
            "        assert_eq!(super::val(), 1);\n",
            "        assert_eq!(super::val(), 2);\n",
        ),
    );
    head_commit(&dir);
    let (got, retries) = judge_with_retry_lines(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok tests_changed=1");
    assert!(
        retries.is_empty(),
        "tracked 集合の歯は撃ち直し無しで base 緑のはず: {retries:?} / {}",
        got.line
    );
}

/// (ii) overlay で足した新規 file（`+`）は base copy の **index に載らない**（working tree
/// にだけ在る）——base の歯が overlay 段で読んだ `git ls-files` に `tests/extra.rs` が無く、
/// `src/lib.rs` は在る。同じ周の `git rev-parse HEAD` は **base の sha**（copy は commit を
/// 作らず base の commit を HEAD に置く＝`HEAD:<file>` を読む `contracts check` が base で測れる）。
#[test]
fn flip_check_base_copy_index_excludes_overlay() {
    let dir = make_tmp_dir();
    scaffold(&dir);
    let probe = dir.join("probe.txt");
    let base_lib = base_lib_with(&probe_lib_test(&probe));
    let base = seed_fixture(&dir, &base_lib);
    // HEAD: 新規の統合 test file 1 本（base の src で赤い）。lib は触らない。
    write_at(
        &dir,
        &extra_rel(),
        &format!("#[test]\nfn fresh() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 2);\n}}\n"),
    );
    head_commit(&dir);
    let got = judge(&base, &dir);
    let seen = std::fs::read_to_string(&probe).unwrap_or_default();
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok tests_changed=1");
    let lines: Vec<&str> = seen.lines().collect();
    assert!(
        lines.contains(&"has-extra=true"),
        "overlay 段（新規 file が working tree に在る周）の姿が残るはず: {seen:?}"
    );
    assert!(
        lines.contains(&lib_rel().as_str()),
        "base の tracked file は index に載るはず: {seen:?}"
    );
    assert!(
        !lines.contains(&extra_rel().as_str()),
        "overlay の新規 file は index に載らないはず: {seen:?}"
    );
    assert!(
        lines.contains(&format!("head={base}").as_str()),
        "base copy の HEAD は base の sha のはず（commit を捏造しない）: {seen:?}"
    );
}

// ---- rename を対にして読む（s2-07l.555・設計 docs/design/pipeline.md §53 / 契約表の行 av）----
//
// 子 module を足すと `#[path]` の新規 module は flip されない（not-flippable）ので、この便の歯は親 file のここへ置く。

/// rename 元の統合 test file（base に在る）。
fn rename_old() -> String {
    format!("crates/{FIXTURE_MEMBER}/tests/old_home.rs")
}

/// rename 先の統合 test file（HEAD に在る）。
fn rename_new() -> String {
    format!("crates/{FIXTURE_MEMBER}/tests/new_home.rs")
}

/// base で緑の歯 1 本（統合 test の形）。
fn rename_tooth(name: &str, want: u32) -> String {
    format!("#[test]\nfn {name}() {{\n    assert_eq!({FIXTURE_MEMBER}::val() * {want}, {want});\n}}\n\n")
}

/// 札 `marks`（`moved s2-07l.1` の形）を頭に置き、base で緑の歯 3 本を持つ統合 test file の本文。
fn rename_body(marks: &[&str]) -> String {
    let head: String = marks.iter().map(|mark| format!("// flip-check: {mark}\n")).collect();
    format!("{head}{}{}{}", rename_tooth("one", 1), rename_tooth("two", 2), rename_tooth("three", 3))
}

/// base に `files` を足して commit し、`moves` を `git mv` して `head` を書いた HEAD を commit する。
/// `manifest` が `None` の周は現物の manifest のまま。dir と base の sha を返す（片付けは呼び手）。
fn rename_fixture(
    manifest: Option<&str>,
    files: &[(&str, &str)],
    moves: &[(&str, &str)],
    head: &[(&str, &str)],
) -> (PathBuf, String) {
    let dir = make_tmp_dir();
    scaffold(&dir);
    if let Some(text) = manifest {
        write_at(&dir, "rules/manifest.toml", text);
    }
    for (rel, body) in files {
        write_at(&dir, rel, body);
    }
    let base = seed_fixture(&dir, BASE_LIB);
    for (from, to) in moves {
        if let Some(parent) = dir.join(to).parent() {
            std::fs::create_dir_all(parent).expect("rename 先の dir を作れる");
        }
        assert!(git(&dir, &["mv", *from, *to]), "fixture で {from} を {to} へ git mv できる");
    }
    for (rel, body) in head {
        write_at(&dir, rel, body);
    }
    head_commit(&dir);
    (dir, base)
}

/// `judge_into` を撃ち、判定と sink の全行を返す。
fn judge_lines(base: &str, dir: &Path) -> (Verdict, Vec<String>) {
    let mut lines: Vec<String> = Vec::new();
    let got = judge_into(base, dir, &mut |line| lines.push(line.to_owned()));
    (got, lines)
}

/// (a) 同一本文の rename だけの便は、旧 path の test 区間に上限（2）を超える札 3 本を持っていても
/// `too-many-marks` / `green-on-base` / `not-flippable` / `no-test-diff` のどれにも落ちず、flip 0 の rc 0 で
/// `renamed=1` を後置する。対は（旧 path・新 path）で、base の本文は旧 path から読まれ、この便の札は 0 本。
#[test]
fn flip_check_rename_same_body_passes_with_no_flip_and_no_fresh_marks() {
    let (old, new) = (rename_old(), rename_new());
    let body = rename_body(&["moved s2-07l.1", "moved s2-07l.2", "retroactive s2-07l.3"]);
    let manifest = flip_manifest("[\"docs/\"]", 2);
    let (dir, base) = rename_fixture(Some(&manifest), &[(&old, &body)], &[(&old, &new)], &[]);
    let changed = changed_rs(&base, &dir).expect("列挙できる");
    let pairs = load_pairs(&base, &dir, &changed).expect("対を読める");
    let (got, lines) = judge_lines(&base, &dir);
    drop_fixture(&dir);
    assert_eq!(changed, vec![(old.clone(), new.clone())], "R の行は（旧 path・新 path）の対");
    assert_eq!(pairs.len(), 1, "対は 1 本");
    assert_eq!(pairs[0].rel, new, "対の rel は HEAD の path");
    assert_eq!(pairs[0].base.as_deref(), Some(body.as_str()), "base の本文は旧 path から読む");
    assert_eq!(pairs[0].head.as_deref(), Some(body.as_str()), "HEAD の本文は新 path から読む");
    assert!(fresh_marks(&pairs).is_empty(), "旧 path の札は持ち越し: {:?}", fresh_marks(&pairs));
    assert_eq!(got.code, 0, "rename だけの便は通る: {}", got.line);
    assert_eq!(got.line, "flip-check: RED-on-base ok tests_changed=0 renamed=1");
    assert!(lines.is_empty(), "sink に行は無い: {lines:?}");
}

/// (b) rename + test 区間に base で赤い歯 1 本を足した対は flip に数えて撃ち（`tests_changed=1`）、overlay の
/// 書き先は新 path で旧 path には書かない。
#[test]
fn flip_check_rename_with_test_diff_flips_and_overlays_at_the_new_path() {
    let (old, new) = (rename_old(), rename_new());
    let body = rename_body(&[]);
    let fresh = format!("{body}#[test]\nfn fresh() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 2);\n}}\n");
    let (dir, base) = rename_fixture(None, &[(&old, &body)], &[(&old, &new)], &[(&new, &fresh)]);
    let changed = changed_rs(&base, &dir).expect("列挙できる");
    let pairs = load_pairs(&base, &dir, &changed).expect("対を読める");
    let dest = make_tmp_dir();
    let wrote = write_one(&dest, &pairs[0]).expect("overlay を書ける");
    let (at_new, at_old) = (dest.join(&new).is_file(), dest.join(&old).exists());
    let got = judge(&base, &dir);
    drop_fixture(&dest);
    drop_fixture(&dir);
    assert_eq!(changed, vec![(old, new)], "R の行は（旧 path・新 path）の対");
    assert!(pairs[0].flips(), "test 区間に差が在る対は flip に数える");
    assert!(wrote && at_new && !at_old, "overlay は新 path へ書く（new={at_new} old={at_old}）");
    assert_verdict(&got.line, got.code, 0, "flip-check: RED-on-base ok tests_changed=1");
    assert!(!got.line.contains("renamed="), "flip した対を rename に数えない: {}", got.line);
}

/// (c) 旧 path の test 区間に札 2 本を持つ file を rename し新しい札 1 本を置くと、2 本は持ち越しで札の本数は 1
/// （上限 2 の manifest で `too-many-marks` に落ちない）。`moved` の免除は従来どおり当たる。
#[test]
fn flip_check_rename_carries_old_marks_and_counts_only_the_new_one() {
    let (old, new) = (rename_old(), rename_new());
    let body = rename_body(&["moved s2-07l.1", "retroactive s2-07l.2"]);
    let marked = format!("// flip-check: moved s2-07l.555\n{body}");
    let manifest = flip_manifest("[\"docs/\"]", 2);
    let (dir, base) = rename_fixture(Some(&manifest), &[(&old, &body)], &[(&old, &new)], &[(&new, &marked)]);
    let pairs = load_pairs(&base, &dir, &changed_rs(&base, &dir).expect("列挙できる")).expect("対を読める");
    let (got, _) = judge_lines(&base, &dir);
    drop_fixture(&dir);
    assert_eq!(fresh_marks(&pairs), vec![(new.as_str(), "s2-07l.555".to_owned())], "この便の札は新しい 1 本だけ");
    assert_eq!(got.code, 0, "札の免除が効く: {}", got.line);
    assert_eq!(got.line, "flip-check: RED-on-base ok tests_changed=0 moved=1");
}

/// (c) rename + test 区間の歯を 1 本消しただけの対は `tests-removed-only` が従来どおり当たり rc 0。
#[test]
fn flip_check_rename_with_removed_tooth_is_removed_only() {
    let (old, new) = (rename_old(), rename_new());
    let body = rename_body(&[]);
    let removed = body.replace(&rename_tooth("three", 3), "");
    assert_ne!(removed, body, "歯を 1 本消せている");
    let (dir, base) = rename_fixture(None, &[(&old, &body)], &[(&old, &new)], &[(&new, &removed)]);
    let (got, _) = judge_lines(&base, &dir);
    drop_fixture(&dir);
    assert_eq!(got.code, 0, "{}", got.line);
    assert_eq!(got.line, "flip-check: RED-on-base ok tests_changed=0 removed-only=1");
}

/// (e) 同じ path で本文が同一の M の行（mode だけの変更）を 1 本だけ持つ便は rename に数えず、`no-test-diff` の
/// FAIL のまま（rename の対を本文の同一で決めていないことを撃つ）。
#[test]
fn flip_check_rename_mode_only_change_is_not_a_rename() {
    use std::os::unix::fs::PermissionsExt;
    let old = rename_old();
    let body = rename_body(&[]);
    let dir = make_tmp_dir();
    scaffold(&dir);
    write_at(&dir, &old, &body);
    let base = seed_fixture(&dir, BASE_LIB);
    std::fs::set_permissions(dir.join(&old), std::fs::Permissions::from_mode(0o755)).expect("mode を変えられる");
    assert!(git(&dir, &["update-index", "--chmod=+x", &old]), "index の mode を変えられる");
    head_commit(&dir);
    let changed = changed_rs(&base, &dir).expect("列挙できる");
    let (got, _) = judge_lines(&base, &dir);
    drop_fixture(&dir);
    assert_eq!(changed, vec![(old.clone(), old)], "mode だけの M は同じ path の対");
    assert_verdict(&got.line, got.code, 1, "flip-check: FAIL reason=no-test-diff");
    assert!(!got.line.contains("renamed"), "{}", got.line);
}

/// (f) docs-only の面の中の file を面の外へ rename しただけの便も、面の外から面の中へ rename しただけの便も
/// docs-only にならず `no-test-diff` で落ち、sink に面の外の path（前者は新 path・後者は旧 path）を名指す。
#[test]
fn flip_check_rename_across_docs_faces_is_not_docs_only() {
    let cases = [("docs/design/x.md", "notes/x.md", "notes/x.md"), ("notes/y.md", "docs/design/y.md", "notes/y.md")];
    for (from, to, outside) in cases {
        let body = "a docs page that keeps its body across the rename\n";
        let (dir, base) = rename_fixture(None, &[(from, body)], &[(from, to)], &[]);
        let (got, lines) = judge_lines(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 1, "flip-check: FAIL reason=no-test-diff");
        assert!(!got.line.contains("docs-only"), "{from} → {to}: {}", got.line);
        assert_eq!(lines, vec![format!("flip-check: outside-docs-faces {outside}")], "{from} → {to}");
    }
}
