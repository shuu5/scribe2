//! `flipcheck` の歯。**本体は `flipcheck.rs`** で、ここには test だけが在る。
//!
//! 分けたのは憲法 C4（1 file の上限）である——判定の実装と歯が同じ file に載ると
//! 上限に当たり、次に歯を足す便が「上限に入らないから書かない」へ倒れる。
//! `#[path]` で `flipcheck` の子 module として取り込むので、module path は
//! `flipcheck::tests` のまま＝歯の名前は 1 つも変わらない。
//!
//! ⚠ この file は `#[cfg(test)] mod` の形を持たないので **flip-check の区間判定には
//! 見えない**（区間は空と数えられ、移動は `tests-removed-only` の枝で通る）。ここへ
//! 足す歯が flip を検査されるようになるのは、`crates/*/src/**/*_tests.rs` を test file
//! として丸ごと写す規則（s2-07l.34 の (6)）が land してからである。

use super::{is_test_file, judge, parse_base, split_regions, Verdict};
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
    write_at(
        &dir,
        "Cargo.toml",
        &format!("[workspace]\nresolver = \"2\"\nmembers = [\"crates/{FIXTURE_MEMBER}\"]\n"),
    );
    write_at(
        &dir,
        "rust-toolchain.toml",
        &format!("[toolchain]\nchannel = \"{FIXTURE_CHANNEL}\"\n"),
    );
    write_at(
        &dir,
        &format!("crates/{FIXTURE_MEMBER}/Cargo.toml"),
        &format!(
            "[package]\nname = \"{FIXTURE_MEMBER}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
        ),
    );
    let base = seed_fixture(&dir, BASE_LIB);
    (dir, base)
}

/// base と HEAD の lib 本文を与えて 1 便を判定する（marker まわりの負例で使い回す）。
fn judge_lib(base_lib: &str, head_lib: &str) -> Verdict {
    let dir = make_tmp_dir();
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

/// module 形の統合 test（`tests/<dir>/<f>.rs`）も test file と見なす。
///
/// 4 段完全一致だと `crates/<c>/tests/<dir>/<f>.rs` が漏れ、新しい test が base へ
/// 写らないまま `not-copied` になる。
#[test]
fn entrance_is_test_file_accepts_module_dirs() {
    for rel in [
        "crates/demo/tests/e2e/main.rs",
        "crates/demo/tests/e2e/rules.rs",
        "crates/demo/tests/single.rs",
        "crates/demo/tests/a/b/c.rs",
    ] {
        assert!(is_test_file(rel), "test file のはず: {rel}");
    }
    for rel in [
        "crates/demo/src/lib.rs",
        "crates/demo/tests/e2e/main.txt",
        "tests/e2e/main.rs",
        "crates/demo/benches/x.rs",
    ] {
        assert!(!is_test_file(rel), "test file でないはず: {rel}");
    }
}

/// 列 0 の marker が後方に在るとき、split 点は字下げ marker でなく列 0 側である。
///
/// 字下げを一律に許すと、入れ子 module の marker を先に拾い、その後ろの実装まで
/// test 区間へ移る。overlay で HEAD の実装が base 木へ紛れ込む回帰の負例である。
#[test]
fn entrance_test_mod_mark_prefers_column_zero() {
    let text = "mod inner {\n    #[cfg(test)]\n    mod probe {\n        fn x() {}\n    }\n}\npub fn real_impl() -> u32 {\n    1\n}\n#[cfg(test)]\nmod tests {\n    fn y() {}\n}\n";
    let (src, test) = split_regions("crates/demo/src/lib.rs", text);
    assert!(src.contains("real_impl"), "実装は src 区間に残る: {src:?}");
    assert!(!test.contains("real_impl"), "実装は test 区間へ移らない: {test:?}");
    assert!(
        test.starts_with("#[cfg(test)]\nmod tests {"),
        "test 区間は列 0 の marker から始まる: {test:?}"
    );
}

/// 列 0 の marker が 1 本も無いときは、字下げされた `#[cfg(test)]` を始点にする。
#[test]
fn entrance_test_mod_mark_allows_indent() {
    let text = "mod outer {\n    #[cfg(test)]\n    mod t {\n        fn a() {}\n    }\n}\n";
    let (src, test) = split_regions("crates/demo/src/lib.rs", text);
    assert_eq!(src, "mod outer {\n", "src 区間は marker の手前まで");
    assert!(
        test.starts_with("    #[cfg(test)]\n    mod t {"),
        "test 区間が字下げされた marker から始まる: {test:?}"
    );
}

/// overlay は base の src 区間を保ち HEAD の test 区間だけを乗せる。
///
/// 区間規則そのものを撃つ純関数の test である（pipeline 側の対照は
/// `flipcheck_red_on_base_passes`＝HEAD の src が混ざれば green-on-base に倒れる）。
#[test]
fn flipcheck_test_region_overlay_keeps_base_src() {
    let src_rel = lib_rel();
    let base = "pub fn v() -> u32 {\n    1\n}\n#[cfg(test)]\nmod t {}\n";
    let head = "pub fn v() -> u32 {\n    2\n}\n#[cfg(test)]\nmod t {\n    // new\n}\n";
    let overlay = format!(
        "{}{}",
        split_regions(&src_rel, base).0,
        split_regions(&src_rel, head).1
    );
    assert!(overlay.contains("    1\n"), "base の src が残るはず: {overlay}");
    assert!(!overlay.contains("    2\n"), "HEAD の src は混ざらないはず: {overlay}");
    assert!(overlay.contains("// new"), "HEAD の test 区間が乗るはず: {overlay}");

    let (src, test) = split_regions(&format!("crates/{FIXTURE_MEMBER}/tests/it.rs"), base);
    assert!(src.is_empty(), "tests/*.rs は全体が test 区間のはず: {src}");
    assert_eq!(test, base, "tests/*.rs は全体が test 区間のはず");
}

/// 先頭の `#[cfg(test)] use …;` を test 区間の始点にしない。
///
/// 始点に取ると base の src 区間が空になり overlay から実装が丸ごと落ちる。
/// その compile error は RED と数える規則なので、base で GREEN な test でも
/// rc 0 が出る（fail-open）。始点は「直後の非空行が `mod` である `#[cfg(test)]`」。
#[test]
fn flipcheck_test_region_starts_at_test_mod() {
    let rel = lib_rel();
    let text = "#[cfg(test)]\nuse std::fmt;\n\npub fn v() -> u32 {\n    1\n}\n\n#[cfg(test)]\nmod t {\n    // body\n}\n";
    let (src, test) = split_regions(&rel, text);
    assert!(src.contains("pub fn v()"), "実装は src 区間に残るはず: {src}");
    assert!(
        src.contains("use std::fmt;"),
        "先頭の cfg(test) use は src 区間に残るはず: {src}"
    );
    assert!(
        test.starts_with("#[cfg(test)]\nmod t {"),
        "test 区間は mod 宣言から始まるはず: {test}"
    );
    assert!(!test.contains("pub fn v()"), "実装は test 区間に入らないはず: {test}");

    let lone = "#[cfg(test)]\nuse std::fmt;\npub fn v() -> u32 {\n    1\n}\n";
    let (only_src, empty) = split_regions(&rel, lone);
    assert_eq!(only_src, lone, "mod が無ければ全体が src 区間のはず");
    assert!(empty.is_empty(), "mod が無ければ test 区間は空のはず: {empty}");
}

/// subdir を cwd にしても .rs の差分を取り落とさない（pathspec が cwd 配下へ縮まない）。
///
/// 縮むと差分 0 件に化けて `skip reason=no-rust-diff` の rc 0 が出る（fail-open）。
#[test]
fn flipcheck_sees_rust_diff_from_subdir() {
    let (dir, base) = base_commit();
    write_at(&dir, "notes/keep.md", "note\n");
    write_at(&dir, &lib_rel(), &format!("// touched\n{BASE_LIB}"));
    head_commit(&dir);
    let got = judge(&base, &dir.join("notes"));
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=no-test-diff");
}

/// src 区間だけの変更は rc 1 / `reason=no-test-diff` で落ちる。
#[test]
fn flipcheck_no_test_diff_fails() {
    let (dir, base) = base_commit();
    write_at(&dir, &lib_rel(), &format!("// touched\n{BASE_LIB}"));
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=no-test-diff");
}

/// .rs の差分が 0 件なら rc 0 / `reason=no-rust-diff` で skip する。
#[test]
fn flipcheck_no_rust_diff_skips() {
    let (dir, base) = base_commit();
    write_at(&dir, "README.md", "fixture touched\n");
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "reason=no-rust-diff");
}

/// 新しい test が base の src で赤いなら rc 0 / `RED-on-base ok` で通る。
#[test]
fn flipcheck_red_on_base_passes() {
    let (dir, base) = base_commit();
    write_at(&dir, &lib_rel(), &BASE_LIB.replace('1', "2"));
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains("tests_changed=1"),
        "写した 1 本を数えるはず: {}",
        got.line
    );
}

/// test 区間に差分は在るが base でも通るなら rc 1 / `reason=green-on-base`。
#[test]
fn flipcheck_green_on_base_fails() {
    let (dir, base) = base_commit();
    write_at(
        &dir,
        &lib_rel(),
        &BASE_LIB.replace(
            "        assert_eq!(super::val(), 1);\n",
            "        assert_eq!(super::val(), 1);\n        assert!(super::val() > 0);\n",
        ),
    );
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
}

/// flip した file が 2 本以上のとき、**1 本でも base で緑なら FAIL** し、どの file が
/// 緑かを名指す。
///
/// まとめて 1 回だけ撃つ実装はここで落ちる——赤い方（lib.rs）の失敗に隠れて全体が
/// RED に見え、緑の新規 test（tests/it.rs）を載せたまま `RED-on-base ok` が出る。
#[test]
fn flip_check_fails_when_one_of_two_flipped_files_is_green_on_base() {
    let (dir, base) = base_commit();
    // 1 本目: base の src（val() == 1）では落ちる新しい test ＝単独で RED。
    write_at(&dir, &lib_rel(), &BASE_LIB.replace('1', "2"));
    // 2 本目: base でも通る新規の統合 test ＝単独で GREEN（これを見逃してはならない）。
    write_at(
        &dir,
        &format!("crates/{FIXTURE_MEMBER}/tests/it.rs"),
        &format!("#[test]\nfn green() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n"),
    );
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        got.line.contains(&format!("file=crates/{FIXTURE_MEMBER}/tests/it.rs")),
        "緑だった file を名指すはず: {}",
        got.line
    );
}

/// flip した file が 2 本とも**単独で** RED なら rc 0 で通り、`tests_changed` は
/// **flip した本数**を数える。
///
/// 合格路の正例である（もう 1 本は FAIL 側の負例）。`ok_line(flipping.len())` を
/// 定数へ縮める変異は、負例だけでは生き残る（review 2026-09-10 Q3）。
#[test]
fn flip_check_passes_when_both_flipped_files_are_red_on_base() {
    let (dir, base) = base_commit();
    // どちらも base の src（val() == 1）では落ちる＝単独で RED。
    write_at(&dir, &lib_rel(), &BASE_LIB.replace('1', "2"));
    write_at(
        &dir,
        &format!("crates/{FIXTURE_MEMBER}/tests/it.rs"),
        &format!("#[test]\nfn red() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 2);\n}}\n"),
    );
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains("tests_changed=2"),
        "flip した 2 本を数えるはず: {}",
        got.line
    );
}

/// 新規 module の in-file 歯は base に写せない＝`green-on-base` でなく
/// **`not-flippable`** と名乗り、どの file かを名指す。
///
/// base 側に `mod` 宣言ごと存在しない file の test 区間だけを写しても compile
/// されないので、構造的に測れない。TDD の不履行（`green-on-base`）と同じ札を
/// 貼ると、直す側は何を直せばよいか判定行から読めない。
#[test]
fn flip_check_reports_not_flippable_for_new_module_with_inline_tests() {
    let (dir, base) = base_commit();
    let rel = format!("crates/{FIXTURE_MEMBER}/src/extra.rs");
    write_at(&dir, &rel, "pub fn v() -> u32 {\n    1\n}\n#[cfg(test)]\nmod t {\n    #[test]\n    fn probe() {\n        assert_eq!(super::v(), 1);\n    }\n}\n");
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=not-flippable");
    assert!(got.line.contains(&rel), "測れない file を名指すはず: {}", got.line);

    // **歯を持たない新規 module は not-flippable ではない**（写せなくても測るものが無い）。
    let (dir, base) = base_commit();
    write_at(&dir, &format!("crates/{FIXTURE_MEMBER}/src/plain.rs"), "pub fn w() -> u32 {\n    2\n}\n");
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert!(
        !got.line.contains("not-flippable"),
        "test 区間の無い新規 file を not-flippable に数えない: {}",
        got.line
    );

    // **marker を置いた新規 module は not-flippable ではなく retroactive**
    // （契約 4「copied / not-copied 両方」）。
    let (dir, base) = base_commit();
    write_at(
        &dir,
        &format!("crates/{FIXTURE_MEMBER}/src/marked.rs"),
        "pub fn w() -> u32 {\n    2\n}\n#[cfg(test)]\nmod t {\n    // flip-check: retroactive s2-07l.14\n    #[test]\n    fn probe() {\n        assert_eq!(super::w(), 2);\n    }\n}\n",
    );
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains("retroactive=1") && !got.line.contains("not-flippable"),
        "marker 付きの新規 module は retroactive へ倒れるはず: {}",
        got.line
    );
}

/// src だけ変えた便は従来どおり落ちる（`not-flippable` へ逃がさない）。
#[test]
fn flip_check_keeps_green_on_base_when_no_test_changed() {
    let (dir, base) = base_commit();
    write_at(&dir, &lib_rel(), &BASE_LIB.replace("    1\n}", "    1 + 0\n}"));
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    // **従来どおり落ちる**（TDD の不履行）。現行 base では reason 語は
    // `no-test-diff` で、`green-on-base`（overlay を撃った上で緑だった周）とは
    // 別語である。本便はこの語を変えない＝新しい 3 語のどれへも逃がさない。
    assert_verdict(&got.line, got.code, 1, "reason=no-test-diff");
    for escaped in ["not-flippable", "removed-only", "retroactive"] {
        assert!(
            !got.line.contains(escaped),
            "src だけの変更を {escaped} へ逃がさない: {}",
            got.line
        );
    }
}

/// test 区間の差が**削除・移動だけ**の file は flip に数えず、その便に他の flip が
/// 無くても `green-on-base` へ落とさない。
///
/// 純粋な module 分割（歯が別 file へ移る）で恒久 FAIL しないための門である。
#[test]
fn flip_check_ignores_file_whose_test_diff_only_removes_tests() {
    // base は 2 本目の commit で取り直す（1 本目は「歯 2 本の状態」を作るためだけ）。
    let (dir, _seed) = base_commit();
    let two = BASE_LIB.replace(
        "    fn holds() {\n        assert_eq!(super::val(), 1);\n    }\n",
        "    fn holds() {\n        assert_eq!(super::val(), 1);\n    }\n    #[test]\n    fn also() {\n        assert_eq!(super::val(), 1);\n    }\n",
    );
    write_at(&dir, &lib_rel(), &two);
    assert!(git(&dir, &["add", "-A"]), "fixture で add できる");
    assert!(git(&dir, &["commit", "-q", "-m", "two"]), "2 本の歯を commit できる");
    let base = head_sha(&dir);
    // HEAD では 1 本減らすだけ（追加も改名も無い）。
    write_at(&dir, &lib_rel(), BASE_LIB);
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains("tests_changed=0") && got.line.contains("removed-only=1"),
        "移動・削除だけと名乗るはず: {}",
        got.line
    );

    // **負例: 1 本消して別の 1 本の本文を変えた file は免除しない**。
    // `#[test]` fn 名で数える実装（`⊆` も真部分集合も）はここで落ちる——名前の上では
    // 「1 本減っただけ」に見えるが、残った歯の中身は書き換わっている。
    let (dir, seed) = base_commit();
    write_at(&dir, &lib_rel(), &two);
    assert!(git(&dir, &["add", "-A"]), "fixture で add できる");
    assert!(git(&dir, &["commit", "-q", "-m", "two"]), "2 本の歯を commit できる");
    let base = head_sha(&dir);
    let _ = seed;
    // 1 本（also）を消し、残った holds の本文を base でも通る形へ書き換える。
    write_at(
        &dir,
        &lib_rel(),
        &BASE_LIB.replace(
            "        assert_eq!(super::val(), 1);\n",
            "        assert!(super::val() >= 1);\n",
        ),
    );
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        !got.line.contains("removed-only"),
        "本文が変わった file を削除だけへ逃がさない: {}",
        got.line
    );
}

/// marker を置いた file の新しい歯は base で緑でも通り、判定行に `retroactive=1`。
///
/// 既に land した挙動へ後から歯を足す便は、歯をどこへ置いても base で緑になる。
/// marker はその弁別を**書いた人が明示する**逃がしで、判定行に残るので review できる。
#[test]
fn flip_check_reports_retroactive_marker_instead_of_failing() {
    let (dir, base) = base_commit();
    let with_marker = BASE_LIB.replace(
        "mod checks {\n",
        "mod checks {\n    // flip-check: retroactive s2-07l.14\n    #[test]\n    fn added_later() {\n        assert_eq!(super::val(), 1);\n    }\n",
    );
    write_at(&dir, &lib_rel(), &with_marker);
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(got.line.contains("retroactive=1"), "marker を数えるはず: {}", got.line);

    // **字面を言及しただけの file は免除しない**。素の `contains` で見る実装は
    // ここで落ちる——marker を文字列に持つ歯（この門を測る当の歯）まで免除され、
    // その便が丸ごと flip 検査を素通りする。
    let (dir, base) = base_commit();
    let mentions = BASE_LIB.replace(
        "mod checks {\n",
        "mod checks {\n    #[test]\n    fn mentions() {\n        let note = \"// flip-check: retroactive s2-xxxx\";\n        assert!(!note.is_empty());\n    }\n",
    );
    write_at(&dir, &lib_rel(), &mentions);
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        !got.line.contains("retroactive"),
        "言及しただけの file を免除しない: {}",
        got.line
    );

    // 以下は**免除されてはならない**負例。marker は「この便で足した歯」の逃がしであり、
    // 貼っておけば恒久的に検査が外れる札でも、src へ書けば効く札でもない。
    let marked = BASE_LIB.replace(
        "mod checks {\n",
        "mod checks {\n    // flip-check: retroactive s2-07l.14\n",
    );
    // (a) base に残った古い marker（HEAD では src だけ変えた便・review 2026-09-10 F1）
    assert_not_retroactive(
        &judge_lib(&marked, &marked.replace("    1\n}", "    1 + 0\n}")),
        "base から引き継いだ marker",
    );
    // (b) src 区間の marker（実装の隣の 1 行で検査を外せる形にしない）
    let added = |body: &str| {
        body.replace(
            "mod checks {\n",
            "mod checks {\n    #[test]\n    fn later() {\n        assert_eq!(super::val(), 1);\n    }\n",
        )
    };
    let src_side = BASE_LIB.replace(
        "pub fn val() -> u32 {\n",
        "// flip-check: retroactive s2-07l.14\npub fn val() -> u32 {\n",
    );
    assert_not_retroactive(&judge_lib(BASE_LIB, &added(&src_side)), "src 区間の marker");
    // (c) bead id の無い marker（区切りの空白も要る＝review の対象にならない札）
    for bare in [
        "// flip-check: retroactive",
        "// flip-check: retroactives2-07l.14",
        // 区切りの空白は在るが id が無い形（この 1 本だけが id 要求を測る）。
        "// flip-check: retroactive ",
    ] {
        let head = added(BASE_LIB).replace("mod checks {\n", &format!("mod checks {{\n    {bare}\n"));
        assert_not_retroactive(&judge_lib(BASE_LIB, &head), bare);
    }
}

/// 存在しない base ref は rc 1 / `reason=infra-error` で loud に落ちる
/// （ref 存在チェックの特別扱いではなく git の rc≠0 経路で自然に到達する）。
#[test]
fn flipcheck_base_setup_failure_is_loud() {
    let (dir, _) = base_commit();
    let got = judge("no-such-base-ref", &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=infra-error");
}

/// `--base` の 3 つの不正形はすべて Err（CLI 面が rc 2 を返す経路）。
#[test]
fn flipcheck_base_arg_forms_are_rejected() {
    let empty: Vec<String> = Vec::new();
    assert!(parse_base(&empty).is_err(), "flag 不在は Err のはず");
    assert!(
        parse_base(&["--base".to_owned()]).is_err(),
        "値不在は Err のはず"
    );
    assert!(
        parse_base(&["--base".to_owned(), String::new()]).is_err(),
        "空文字は Err のはず"
    );
    assert_eq!(
        parse_base(&["--base".to_owned(), "main".to_owned()]).ok(),
        Some("main".to_owned()),
        "値が在れば Ok のはず"
    );
}
