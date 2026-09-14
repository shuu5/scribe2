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

use super::{
    failed_tests, is_test_file, judge, judge_into, nextest_args, parse_base, split_regions,
    FailedTest, FilePair, Verdict, RETROACTIVE_MARK,
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
