//! `flipcheck` の歯（入口・区間判定）: test file の見なし方・`#[cfg(test)]` の位置・
//! test 区間の切り出し・base ref の受け方を測る。本体は `flipcheck.rs`、共通の
//! helper は親 module（`flipcheck_tests.rs`）に在る。

// 純粋な移動（歯を足さない・s2-07l.91）。札は file ごとに要る＝この file の差は削除にならない。
// flip-check: moved s2-07l.91

use super::*;

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
