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

// 本便（s2-07l.37）が足す歯は、既に land した `flipcheck.rs` の挙動を後から測る
// ——実装は 1 byte も変えないので、歯をどこへ置いても base で緑になる。逃がしは
// 下の 1 行で明示する（この file は名前で丸ごと test 区間ゆえ、ここに置いた札が効く。
// fixture の文字列の中に居る札は行頭が `"` なので別物である）。
// flip-check: retroactive s2-07l.37

use super::{is_test_file, judge, judge_into, parse_base, split_regions, FilePair, Verdict};
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

/// 持ち越した札を持つ base と、その札を残したまま base で緑の歯を 1 本足した HEAD。
fn carried_pair() -> (String, String) {
    let carried = BASE_LIB.replace(
        "mod checks {\n",
        "mod checks {\n    // flip-check: retroactive s2-07l.33\n",
    );
    let head = carried.replace(
        "s2-07l.33\n",
        "s2-07l.33\n    #[test]\n    fn added_later() {\n        assert_eq!(super::val(), 1);\n    }\n",
    );
    (carried, head)
}

/// 合成 workspace の src 配下へ外出しした test file の repo 相対 path。
fn extra_rel() -> String {
    format!("crates/{FIXTURE_MEMBER}/src/extra_tests.rs")
}

/// **base に既に在る札は効かない**（この便で足した札だけが免除する）。
///
/// marker 行は file に残るので、在るだけで数えると、一度貼った札がその file の
/// test 区間を触る**以後のすべての便**を免除する——札の bead id と便が対応しなく
/// なり、判定行の `retroactive=N` を review しても何を免除したのかを辿れない。
#[test]
fn flip_check_ignores_retroactive_marker_already_in_base() {
    let (carried, head) = carried_pair();
    let (dir, _) = base_commit();
    let base = seed_fixture(&dir, &carried);
    write_at(&dir, &lib_rel(), &head);
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert_not_retroactive(&got, "base から持ち越した札");

    // 述語も直接見る。効かない札は stale と名乗る。
    let stale = FilePair {
        rel: lib_rel(),
        base: Some(carried),
        head: Some(head.clone()),
    };
    assert!(stale.stale_marker(), "持ち越した札は stale と名乗るはず");
    assert!(!stale.marked(), "持ち越した札を数えないはず");

    // 負例。**同じ札でも base に無ければ**この便で足したものとして効く
    // （HEAD 側だけを見る実装も、両側とも無視する実装も、ここで落ちる）。
    let fresh = FilePair {
        rel: lib_rel(),
        base: Some(BASE_LIB.to_owned()),
        head: Some(head),
    };
    assert!(fresh.marked(), "HEAD にだけ在る札は効くはず");
    assert!(!fresh.stale_marker(), "この便で足した札を stale と呼ばない");
}

/// 札の同一性は **bead id** で見る（空白 1 個で持ち越した札が新しい札に化けない）。
///
/// 行の字面で比べると、字下げや id の前後の空白が 1 個違うだけで持ち越した札が
/// 「この便で足した札」に化け、**古い bead id のまま免除が効き続ける**——上の門が
/// 塞ごうとしている当の穴の裏口である。
#[test]
fn flip_check_treats_respaced_carried_marker_as_stale() {
    let (carried, _) = carried_pair();
    // **前後どちらの空白差も**同じ札として扱う（片側だけ trim する実装はここで落ちる）。
    let head = carried
        .replace("retroactive s2-07l.33", "retroactive  s2-07l.33 ")
        .replace(
            "s2-07l.33\n",
            "s2-07l.33\n    #[test]\n    fn added_later() {\n        assert_eq!(super::val(), 1);\n    }\n",
        );
    let (dir, _) = base_commit();
    let base = seed_fixture(&dir, &carried);
    write_at(&dir, &lib_rel(), &head);
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert_not_retroactive(&got, "空白を足しただけの持ち越し札");

    let pair = FilePair {
        rel: lib_rel(),
        base: Some(carried),
        head: Some(head),
    };
    assert!(pair.stale_marker(), "空白違いは同じ札として扱うはず");
}

/// 効かない札は **stderr へ 1 行**出す（判定行にも rc にも載らないので出所を渡す）。
///
/// 出したこと自体を測らないと、emit を丸ごと消しても全部の歯が緑のままになる。
#[test]
fn flip_check_emits_stale_marker_line_to_stderr() {
    let (carried, head) = carried_pair();
    let (dir, _) = base_commit();
    let base = seed_fixture(&dir, &carried);
    write_at(&dir, &lib_rel(), &head);
    head_commit(&dir);
    let mut lines: Vec<String> = Vec::new();
    let got = judge_into(&base, &dir, &mut |line| lines.push(line.to_owned()));
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    let stale: Vec<&String> = lines
        .iter()
        .filter(|line| line.starts_with("flip-check: stale-marker "))
        .collect();
    assert_eq!(stale.len(), 1, "効かない札を 1 行で名指すはず: {lines:?}");
    assert!(
        stale[0].contains(&lib_rel()),
        "どの file の札かを名指すはず: {}",
        stale[0]
    );

    // 負例。**この便で足した札**の周には出さない（在るだけで出す実装はここで落ちる）。
    let (dir, base) = base_commit();
    write_at(
        &dir,
        &lib_rel(),
        &BASE_LIB.replace(
            "mod checks {\n",
            "mod checks {\n    // flip-check: retroactive s2-07l.34\n    #[test]\n    fn added_later() {\n        assert_eq!(super::val(), 1);\n    }\n",
        ),
    );
    head_commit(&dir);
    let mut fresh_lines: Vec<String> = Vec::new();
    let got = judge_into(&base, &dir, &mut |line| fresh_lines.push(line.to_owned()));
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        !fresh_lines
            .iter()
            .any(|line| line.starts_with("flip-check: stale-marker ")),
        "この便で足した札に stale を出さない: {fresh_lines:?}"
    );

    // 負例 2。**免除を求めていない便**——test 区間が 1 byte も動かず src だけ触った便
    // ——にも出さない。札は file に残るので、これを出すとその file の src を触るたびに
    // 「札を削除しろ」と言われる（削除は別便の仕事）。狼少年にすると、本当に効かない
    // 札を見落とす。
    let (carried, _) = carried_pair();
    let src_only = carried.replace("    1\n}", "    1 + 0\n}");
    let (dir, _) = base_commit();
    let base = seed_fixture(&dir, &carried);
    write_at(&dir, &lib_rel(), &src_only);
    head_commit(&dir);
    let mut src_lines: Vec<String> = Vec::new();
    let _ = judge_into(&base, &dir, &mut |line| src_lines.push(line.to_owned()));
    drop_fixture(&dir);
    assert!(
        !src_lines
            .iter()
            .any(|line| line.starts_with("flip-check: stale-marker ")),
        "test 区間が動いていない便に stale を出さない: {src_lines:?}"
    );
}

/// 札を **1 枚も持たない** file には `stale-marker` を出さない。
///
/// 「新しい札が 1 枚も無い」だけで stale と名乗る実装——`stale_marker()` の前半
/// （HEAD の test 区間が札を持つか）を落として `fresh_markers().is_empty()` だけで
/// 見る形——はここで落ちる。札と無縁の便、つまり flip-check が通す便の**ほとんど
/// 全部**に「札を削除しろ」の 1 行が出る。狼少年にすると、本当に効かない札を見落とす。
/// 既存の負例（[`flip_check_emits_stale_marker_line_to_stderr`] の 2 本）はどちらも
/// **札を持つ** file なので、この変異を撃ち落とせない。
#[test]
fn flip_check_emits_no_stale_line_for_file_without_marker() {
    let (dir, base) = base_commit();
    // 札は 1 枚も置かず、base の src（`val()` は 1）では落ちる歯を 1 本足すだけ。
    write_at(
        &dir,
        &lib_rel(),
        &BASE_LIB.replace(
            "mod checks {\n",
            "mod checks {\n    #[test]\n    fn added_later() {\n        assert_eq!(super::val(), 2);\n    }\n",
        ),
    );
    head_commit(&dir);
    let mut lines: Vec<String> = Vec::new();
    let got = judge_into(&base, &dir, &mut |line| lines.push(line.to_owned()));
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    let stale = lines
        .iter()
        .filter(|line| line.starts_with("flip-check: stale-marker "))
        .count();
    assert_eq!(
        stale,
        0,
        "札の無い file に stale を出さない（sink は {} 行）: {lines:?}",
        lines.len()
    );
}

/// この便で **札の行を消した** file を `stale-marker` と呼ばない。
///
/// 効かない札の在処を **HEAD 側でなく base 側**で見る実装——`stale_marker()` の
/// `head_test()` を `base_test()` へ替える変異——はここで落ちる。base に札が在れば、
/// HEAD で消した後も「効かない札が在る」と言い続けるからである。札を消すのは
/// stale の 1 行が出した指示に従った側の便で、そこへ同じ指示を返すと直し方が閉じない
/// （消しても消しても言われる）。既存の歯は base と HEAD の**両方**に札を持つ便しか
/// 撃たないので、この変異は生き残る。
#[test]
fn flip_check_does_not_call_removed_marker_stale() {
    let (carried, _) = carried_pair();
    // HEAD: 札の行を消し、歯を base の src（`val()` は 1）では落ちる形へ書き換える。
    let head = carried
        .replace("    // flip-check: retroactive s2-07l.33\n", "")
        .replace(
            "        assert_eq!(super::val(), 1);\n",
            "        assert_eq!(super::val(), 2);\n",
        );
    let (dir, _) = base_commit();
    let base = seed_fixture(&dir, &carried);
    write_at(&dir, &lib_rel(), &head);
    head_commit(&dir);
    let mut lines: Vec<String> = Vec::new();
    let got = judge_into(&base, &dir, &mut |line| lines.push(line.to_owned()));
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    let stale = lines
        .iter()
        .filter(|line| line.starts_with("flip-check: stale-marker "))
        .count();
    assert_eq!(
        stale,
        0,
        "消した札を stale と呼ばない（sink は {} 行）: {lines:?}",
        lines.len()
    );
}

/// base の **src 区間にしか無い**札は持ち越しに数えない（比べるのは test 区間だけ）。
///
/// 持ち越しを base の **全文**から拾う実装——`fresh_markers()` の
/// `marker_beads(&self.base_test())` を base 本文へ広げる変異——はここで落ちる。
/// src 区間へ同じ id を 1 行置いておけば、test 区間へ足した札が「持ち越し」に化けて
/// **黙って無効化**され、その便は `green-on-base` の偽 FAIL になる。効く札は test 区間の
/// ものだけ（[`flip_check_reports_retroactive_marker_instead_of_failing`] の (b)）だが、
/// **効かない側を持ち越しに数えてもいけない**——src の 1 行はどちらの側でも数えない。
/// 既存の歯は src 側の札を HEAD にしか置かないので、この変異は生き残る。
#[test]
fn flip_check_treats_marker_carried_only_outside_tests_as_fresh() {
    // base: src 区間（`pub fn val` の直前の行）にだけ札が在る。
    let base_lib = BASE_LIB.replace(
        "pub fn val() -> u32 {\n",
        "// flip-check: retroactive s2-07l.37x\npub fn val() -> u32 {\n",
    );
    // HEAD: test 区間へ**同じ id** の札と、base で緑の歯を 1 本足す。
    let head = base_lib.replace(
        "mod checks {\n",
        "mod checks {\n    // flip-check: retroactive s2-07l.37x\n    #[test]\n    fn added_later() {\n        assert_eq!(super::val(), 1);\n    }\n",
    );
    let (dir, _) = base_commit();
    let base = seed_fixture(&dir, &base_lib);
    write_at(&dir, &lib_rel(), &head);
    head_commit(&dir);
    let mut lines: Vec<String> = Vec::new();
    let got = judge_into(&base, &dir, &mut |line| lines.push(line.to_owned()));
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains("retroactive=1"),
        "test 区間へ足した札は効くはず: {}",
        got.line
    );
    let stale = lines
        .iter()
        .filter(|line| line.starts_with("flip-check: stale-marker "))
        .count();
    assert_eq!(
        stale,
        0,
        "効いた札に stale を出さない（sink は {} 行）: {lines:?}",
        lines.len()
    );
}

/// `src` 配下へ外出しした test file（`*_tests.rs` / `tests.rs`）は **丸ごと写す**。
///
/// `#[path]` で外出しした test module は `#[cfg(test)] mod` の形を持たないので、区間判定
/// には src 区間だけの file に見える＝そこへ足した歯が 1 本も測られない（s2-07l.36 が
/// 作った穴）。名前で test file と見なせば、base に `mod` 宣言が在る限り base で compile
/// され RED を測れる。
#[test]
fn flip_check_copies_src_tests_file_whole() {
    let lib_with_mod =
        format!("{BASE_LIB}\n#[cfg(test)]\n#[path = \"extra_tests.rs\"]\nmod extra;\n");
    let base_extra = "#[test]\nfn extra_holds() {\n    assert_eq!(super::val(), 1);\n}\n";
    let head_extra = format!(
        "{base_extra}\n#[test]\nfn extra_added_later() {{\n    assert_eq!(super::val(), 1);\n}}\n"
    );
    let (dir, _) = base_commit();
    write_at(&dir, &extra_rel(), base_extra);
    let base = seed_fixture(&dir, &lib_with_mod);
    write_at(&dir, &extra_rel(), &head_extra);
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    // 丸ごと写されるので、base で緑の新しい歯は green-on-base で落ちる。
    // 写さない実装ではこの file の test 区間が空と数えられ reason=no-test-diff になる。
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");

    // 規則そのものも見る（実装 crate の file 名で当てる）。
    assert!(is_test_file(&extra_rel()), "src の *_tests.rs は test file");
    assert!(
        is_test_file(&format!("crates/{FIXTURE_MEMBER}/src/tests.rs")),
        "src の tests.rs も test file"
    );
    assert!(!is_test_file(&lib_rel()), "ふつうの src file は test file でない");
    assert!(
        !is_test_file(&format!("crates/{FIXTURE_MEMBER}/src/tests_helper.rs")),
        "接尾辞が違う file を巻き込まない"
    );
}

/// fixture の統合 test target（`tests/e2e/<name>`）の repo 相対 path。
fn e2e_rel(name: &str) -> String {
    format!("crates/{FIXTURE_MEMBER}/tests/e2e/{name}")
}

/// 2 本目の統合 test target（`tests/it2/<name>`）の repo 相対 path。
fn it2_rel(name: &str) -> String {
    format!("crates/{FIXTURE_MEMBER}/tests/it2/{name}")
}

/// base で **落ちる** 歯（`val()` は base で 1）。
fn red_body() -> String {
    format!("#[test]\nfn probe() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 2);\n}}\n")
}

/// base で **通る** 歯（同梱が RED を捏造しないことの負例に使う）。
fn green_body() -> String {
    format!("#[test]\nfn probe() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n")
}

/// base 側に統合 test target（`tests/e2e/{main.rs,seed.rs}`）を持つ fixture を作る。
///
/// 新規 module の便は「宣言 file（`main.rs`）」と「本体 file」の 2 file に割れるので、
/// その土台になる base が要る。
fn base_commit_with_e2e() -> (PathBuf, String) {
    let (dir, _) = base_commit();
    write_at(&dir, &e2e_rel("main.rs"), "mod seed;\n");
    write_at(
        &dir,
        &e2e_rel("seed.rs"),
        &format!("#[test]\nfn seed_holds() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n"),
    );
    head_commit(&dir);
    let base = head_sha(&dir);
    (dir, base)
}

/// 新規 module の**宣言 file は単独で撃たず**、本体 file を撃つ木へ同梱する。
///
/// 割れた 2 file を単独で撃つと、どちらの判定も意味を持たない——宣言だけなら本体不在の
/// `E0583`（偽 RED）、本体だけなら base に宣言が無く compile 対象外で全 PASS（偽 GREEN）。
/// 同梱すれば本体の歯が base の実装で実際に落ちることを測れる。
#[test]
fn flip_check_bundles_module_declaration_with_new_body_file() {
    let (dir, base) = base_commit_with_e2e();
    write_at(&dir, &e2e_rel("main.rs"), "mod seed;\nmod newmod;\n");
    write_at(&dir, &e2e_rel("newmod.rs"), &red_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains("decl=1"),
        "同梱した宣言 file の本数が判定行に載るはず: {}",
        got.line
    );
}

/// `mod x;` **以外**の行も動いた file は宣言 file と見なさず、従来どおり単独で撃つ。
///
/// 自前の歯を足した file まで宣言と見なすと、その歯が単独で測られなくなる（同梱は判定を
/// 緩める側なので弁別は狭く取る）。本便では本体 file が単独で緑になるので、判定は
/// `green-on-base`＝**従来どおりの**結果になり、`decl=` は載らない。
#[test]
fn flip_check_still_judges_declaration_file_that_also_changes_tests() {
    let (dir, base) = base_commit_with_e2e();
    write_at(
        &dir,
        &e2e_rel("main.rs"),
        &format!(
            "mod seed;\nmod newmod;\n#[test]\nfn own() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 2);\n}}\n"
        ),
    );
    write_at(&dir, &e2e_rel("newmod.rs"), &red_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        got.line.contains(&e2e_rel("newmod.rs")),
        "単独で緑だった本体 file を名指すはず: {}",
        got.line
    );
    assert!(
        !got.line.contains("decl="),
        "歯も動いた file を宣言として同梱しない: {}",
        got.line
    );
}

/// 同梱は **RED を捏造しない**——宣言を同梱しても本体の歯が base で緑なら FAIL のまま。
#[test]
fn flip_check_fails_when_new_body_file_is_green_on_base_even_with_declaration() {
    let (dir, base) = base_commit_with_e2e();
    write_at(&dir, &e2e_rel("main.rs"), "mod seed;\nmod newmod;\n");
    write_at(&dir, &e2e_rel("newmod.rs"), &green_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        got.line.contains(&e2e_rel("newmod.rs")),
        "緑だった本体 file を名指すはず: {}",
        got.line
    );
}

/// `pub(crate) mod x;` も宣言に数える（可視性の前置きは字面の境界だけの違い）。
#[test]
fn flip_check_treats_pub_crate_mod_line_as_declaration() {
    let (dir, base) = base_commit_with_e2e();
    write_at(&dir, &e2e_rel("main.rs"), "mod seed;\npub(crate) mod newmod;\n");
    write_at(&dir, &e2e_rel("newmod.rs"), &red_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains("decl=1"),
        "pub(crate) 付きの宣言も同梱するはず: {}",
        got.line
    );

    // 境界の負例: 可視性の直後に**空白が無い**字面は宣言に数えない（契約の
    // `(pub(\(crate\))?\s+)?mod \w+;` は空白 1 個以上を要求する）。数えてしまうと
    // 「pub で始まる別の行」まで宣言に化け、同梱が広がって歯が単独で測られなくなる。
    let (dir, base) = base_commit_with_e2e();
    write_at(&dir, &e2e_rel("main.rs"), "mod seed;\npub(crate)mod newmod;\n");
    write_at(&dir, &e2e_rel("newmod.rs"), &red_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        !got.line.contains("decl="),
        "空白の無い可視性は宣言に数えない: {}",
        got.line
    );
}

/// 同梱した宣言 file の**本数**を判定行が数える（`decl=N` の N は定数ではない）。
///
/// 統合 test target を 2 本持つ便では宣言 file も 2 本になる。`decl=1` に潰す変異は
/// 1 本だけの便では生き残るので、**2 本の便**で数えているかを測る。
#[test]
fn flip_check_counts_each_bundled_declaration_file() {
    let (dir, _) = base_commit_with_e2e();
    // base に 2 本目の target を置く（この commit を base にする）。
    write_at(&dir, &it2_rel("main.rs"), "mod seed2;\n");
    write_at(
        &dir,
        &it2_rel("seed2.rs"),
        &format!("#[test]\nfn seed2_holds() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n"),
    );
    head_commit(&dir);
    let base = head_sha(&dir);

    // HEAD: 2 本の target それぞれへ宣言 1 行 + base で赤い本体。
    write_at(&dir, &e2e_rel("main.rs"), "mod seed;\nmod newmod;\n");
    write_at(&dir, &e2e_rel("newmod.rs"), &red_body());
    write_at(&dir, &it2_rel("main.rs"), "mod seed2;\nmod newmod2;\n");
    write_at(&dir, &it2_rel("newmod2.rs"), &red_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains("decl=2"),
        "宣言 file 2 本を数えるはず: {}",
        got.line
    );
}

/// 同梱は **RED を捏造しない**——本体が **2 本**でも（`s2-07l.41` が入れた fail-open）。
///
/// 本体 1 本を撃つ turn に便の宣言行を全部置くと、その turn ではまだ置かれていない兄弟
/// module の `E0583` が「overlay 後の compile error は RED」の規則で RED に化ける。
/// 本体がどちらも base で緑でも `RED-on-base ok decl=2` が出た（実測 2026-09-10）。
/// fixture は [`flip_check_counts_each_bundled_declaration_file`] と同じで、本体だけを
/// 緑にした対である（並べて読めば「赤 2 本は PASS・緑 2 本は FAIL」が対比になる）。
#[test]
fn flip_check_fails_when_sibling_declaration_hides_green_body() {
    let (dir, _) = base_commit_with_e2e();
    // base に 2 本目の target を置く（この commit を base にする）。
    write_at(&dir, &it2_rel("main.rs"), "mod seed2;\n");
    write_at(
        &dir,
        &it2_rel("seed2.rs"),
        &format!("#[test]\nfn seed2_holds() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n"),
    );
    head_commit(&dir);
    let base = head_sha(&dir);

    // HEAD: 2 本の target それぞれへ宣言 1 行 + base で**緑**の本体。
    write_at(&dir, &e2e_rel("main.rs"), "mod seed;\nmod newmod;\n");
    write_at(&dir, &e2e_rel("newmod.rs"), &green_body());
    write_at(&dir, &it2_rel("main.rs"), "mod seed2;\nmod newmod2;\n");
    write_at(&dir, &it2_rel("newmod2.rs"), &green_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        got.line.contains(&e2e_rel("newmod.rs")),
        "単独で緑だった本体 file を名指すはず（兄弟の E0583 に隠されない）: {}",
        got.line
    );
}

/// 同梱するのは **その turn の tree に本体が在る**宣言行だけ（本体は A = 赤 / B = 緑）。
///
/// **本歯が単独で閉じるのは「B の turn で A の宣言行が落ちていない」側だけ**である
/// ——A は base で赤いので、A の turn が兄弟の `E0583` に汚されても判定は RED のまま
/// 変わらない。もう一方（A の turn で B の宣言行が落ちていない）は
/// [`flip_check_fails_when_sibling_declaration_hides_green_body`]（本体とも緑）が閉じる
/// ので、**両方向はこの 2 本の対で**閉じている（lens-44 M1 の訂正・2026-09-10）。
#[test]
fn flip_check_bundles_only_declarations_whose_body_is_present() {
    let (dir, _) = base_commit_with_e2e();
    write_at(&dir, &it2_rel("main.rs"), "mod seed2;\n");
    write_at(
        &dir,
        &it2_rel("seed2.rs"),
        &format!("#[test]\nfn seed2_holds() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n"),
    );
    head_commit(&dir);
    let base = head_sha(&dir);

    // HEAD: 本体 A（先に撃たれる）は base で赤く、本体 B は base で緑。
    write_at(&dir, &e2e_rel("main.rs"), "mod seed;\nmod newmod;\n");
    write_at(&dir, &e2e_rel("newmod.rs"), &red_body());
    write_at(&dir, &it2_rel("main.rs"), "mod seed2;\nmod newmod2;\n");
    write_at(&dir, &it2_rel("newmod2.rs"), &green_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        got.line.contains(&it2_rel("newmod2.rs")),
        "緑だった 2 本目の本体を名指すはず（A の RED に隠されない）: {}",
        got.line
    );
    assert!(
        !got.line.contains(&e2e_rel("newmod.rs")),
        "base で赤かった 1 本目は名指さない（B の E0583 が A を汚していない）: {}",
        got.line
    );
}

/// 宣言 file **しか** flip していない便は、従来どおり**単独で**撃つ。
///
/// 存在しない module を指す `mod x;` の `E0583` は**本当の** RED であって、同梱で
/// 消してよいものではない。本体が 1 本も無い便まで同梱の路へ流すと overlay の runner を
/// **1 度も撃たずに** PASS が出る（`plan_of` の空 bodies 分岐を消す変異は、本体を持つ便の
/// 歯では全部生き残る）。stderr の `decl-with-body` 不在は assert しない——in-process の
/// 歯が読める sink は stale-marker 行だけで、同じ seam は「やらない」の M2 が要るもの
/// だからである（planner 裁定 2026-09-10）。
#[test]
fn flip_check_judges_declaration_only_change_alone() {
    let (dir, base) = base_commit_with_e2e();
    write_at(&dir, &e2e_rel("main.rs"), "mod seed;\nmod newmod;\n");
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        !got.line.contains("decl="),
        "同梱していない便に decl= は載らない: {}",
        got.line
    );
}

/// **base に既に在った宣言行は落とさない**（絞り込みが捏造 RED を作る裏口を塞ぐ）。
///
/// `#[path = "…"]` 付きの module は本体が同じ dir の `<name>.rs` にも `<name>/mod.rs` にも
/// 無いので、在処の実体だけで絞ると落ちる。属性行は `mod` 行ではないので残り、**属性だけが
/// 孤児**になって `expected item after attributes` の compile error＝RED に化ける——絞り込みが
/// 消しに来た当の fail-open を別の扉から作り直す（実測 2026-09-10・lens-44 H1: 絞る前の
/// xtask は正しく `green-on-base` で落ちたのに、絞った側が `RED-on-base ok` で通した）。
/// base が緑である以上 base に在った宣言の本体は必ず在るので、落とす理由が無い。
#[test]
fn flip_check_keeps_declaration_that_base_already_had() {
    let (dir, _) = base_commit();
    // base: 宣言 file に `#[path]` 付きの module が**既に在る**。
    write_at(
        &dir,
        &e2e_rel("main.rs"),
        "mod seed;\n#[path = \"helpers/util.rs\"]\nmod util;\n",
    );
    write_at(
        &dir,
        &e2e_rel("seed.rs"),
        &format!("#[test]\nfn seed_holds() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n"),
    );
    write_at(
        &dir,
        &e2e_rel("helpers/util.rs"),
        &format!("#[test]\nfn util_holds() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n"),
    );
    head_commit(&dir);
    let base = head_sha(&dir);

    // HEAD: 新規 module を 1 本足す。本体は base で**緑**＝落ちるべき便である。
    write_at(
        &dir,
        &e2e_rel("main.rs"),
        "mod seed;\nmod newmod;\n#[path = \"helpers/util.rs\"]\nmod util;\n",
    );
    write_at(&dir, &e2e_rel("newmod.rs"), &green_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        got.line.contains(&e2e_rel("newmod.rs")),
        "孤児になった属性の compile error を RED と数えない: {}",
        got.line
    );
}

/// 宣言 file の **`mod` 行以外は 1 行も触らない**（`use` や helper を落とさない）。
///
/// 宣言 file は module 宣言だけの file とは限らない——この repo の
/// `crates/*/tests/e2e/main.rs` も `use` と helper 関数を持つ。本体はそれらに依存しうるので、
/// 落とすと本体が compile できず、やはり compile error＝捏造 RED になる（lens-44 H2）。
/// base に在る helper を本体から呼ぶ形で測る＝落ちれば本体が緑だと分からなくなる。
#[test]
fn flip_check_keeps_non_declaration_lines_of_bundled_file() {
    let (dir, _) = base_commit();
    write_at(
        &dir,
        &e2e_rel("main.rs"),
        "mod seed;\npub fn helper() -> u32 {\n    1\n}\n",
    );
    write_at(
        &dir,
        &e2e_rel("seed.rs"),
        &format!("#[test]\nfn seed_holds() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n"),
    );
    head_commit(&dir);
    let base = head_sha(&dir);

    // HEAD: 宣言 1 行だけを足す（helper は動かさない＝この file は宣言 file のまま）。
    // 本体は helper を呼び、base で**緑**になる＝落ちるべき便である。
    write_at(
        &dir,
        &e2e_rel("main.rs"),
        "mod seed;\nmod newmod;\npub fn helper() -> u32 {\n    1\n}\n",
    );
    write_at(
        &dir,
        &e2e_rel("newmod.rs"),
        &format!("#[test]\nfn probe() {{\n    assert_eq!(crate::helper(), {FIXTURE_MEMBER}::val());\n}}\n"),
    );
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        got.line.contains(&e2e_rel("newmod.rs")),
        "helper を落として compile を壊した RED を数えない: {}",
        got.line
    );
}

/// 絞り込みは **行**の粒度で効く（宣言 file 1 本 + 新規 module 2 本の形・lens-44 M2）。
///
/// file 粒度で「宣言 file ごと置く / 置かない」に潰すと、同じ file に居る 2 本の宣言を
/// 別々の turn で扱えず、兄弟の `E0583` がどちらかの turn を汚したまま PASS になる。
/// 本体 A = 赤 / B = 緑ゆえ、行の粒度で絞れているときだけ B が名指される。
#[test]
fn flip_check_bundles_two_new_modules_declared_in_one_file() {
    let (dir, base) = base_commit_with_e2e();
    write_at(&dir, &e2e_rel("main.rs"), "mod seed;\nmod alpha;\nmod beta;\n");
    write_at(&dir, &e2e_rel("alpha.rs"), &red_body());
    write_at(
        &dir,
        &e2e_rel("beta.rs"),
        &format!("#[test]\nfn probe_beta() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n"),
    );
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        got.line.contains(&e2e_rel("beta.rs")),
        "同じ宣言 file に居る 2 本目の本体を名指すはず: {}",
        got.line
    );
}

/// 本体が `<name>/mod.rs` に置かれた新規 module も同梱する（在処判定のもう半分）。
///
/// `<name>.rs` 側しか見ない実装は、この形の宣言行を落として本体を compile 対象から外す
/// ——判定は緑側へ倒れる（fail-closed）ので害は小さいが、**測っていなければ次の便が
/// 黙って落とす**。本体は base で赤いので PASS `decl=1` になる（lens-44 M3）。
#[test]
fn flip_check_sees_body_placed_as_module_dir() {
    let (dir, base) = base_commit_with_e2e();
    write_at(&dir, &e2e_rel("main.rs"), "mod seed;\nmod newmod;\n");
    write_at(&dir, &e2e_rel("newmod/mod.rs"), &red_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains("decl=1"),
        "<name>/mod.rs 形の本体も同梱するはず: {}",
        got.line
    );
}
