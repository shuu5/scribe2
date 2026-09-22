//! `flipcheck` の歯（overlay 判定）: base へ test 区間を写して撃つ本筋——RED-on-base /
//! green-on-base / no-test-diff / not-flippable / tests-removed-only / outside-teeth を測る。
//! 共通の helper は親 module（`flipcheck_tests.rs`）に在る。

// 純粋な移動（歯を足さない・s2-07l.91）。札は file ごとに要る＝この file の差は削除にならない。
// flip-check: moved s2-07l.91

use super::*;

/// subdir を cwd にしても .rs の差分を取り落とさない（pathspec が cwd 配下へ縮まない）。
///
/// 縮むと差分 0 件に化けて docs-only の側へ倒れる（面の中だけの便なら rc 0＝fail-open）。
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

/// .rs の差分が 0 件でも、docs-only の面（rules 行 `flip.docs_only_faces`）の外の file を含む便は
/// rc 1 / `reason=no-test-diff` で落ちる（`.rs` の有無だけで通す skip は消えた・`s2-07l.170`）。
#[test]
fn flipcheck_no_rust_diff_outside_docs_faces_fails() {
    let (dir, base) = base_commit();
    write_at(&dir, "plugin/hooks.json", "fixture touched\n");
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=no-test-diff");
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
    // 別語である。本便はこの語を変えない＝逃がし 4 語のどれへも逃がさない。
    assert_verdict(&got.line, got.code, 1, "reason=no-test-diff");
    // `moved` の裸の字面は `removed-only` の部分列なので判定行の token 形で見る。
    for escaped in ["not-flippable", "removed-only", "retroactive", "moved="] {
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

// ---- 歯の外の file の同梱（s2-07l.450・設計 docs/design/pipeline.md §37）----
//
// 歯の外（helper・fixture）の行だけが動いた test file は、単独 overlay が base の歯を 1 本も
// 動かさず構造的に緑になる。宣言 file と同じ側＝本体を撃つ木へ同梱し、判定行に `fixture=N`。

/// 歯 `holds` の隣に **`#[test]` の付かない helper** を持つ lib（`holds` は helper を呼ばない
/// ＝helper だけ変えた便は単独では必ず緑）。`dead_code` の allow は base から在る（動かない行）。
const HELPER_FN: &str = "    #[allow(dead_code)]\n    fn helper() -> u32 {\n        10\n    }\n";

/// [`HELPER_FN`] の本文の 1 行（歯の外）だけを変えた lib。
fn helper_changed_lib(base_lib: &str) -> String {
    base_lib.replace("        10\n", "        20\n")
}

/// `extra_rel()` に置く、base の実装（`val()` は 1）で落ちる歯 1 本。
fn red_extra() -> String {
    format!("#[test]\nfn fresh() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 2);\n}}\n")
}

/// (a) 歯の外の 1 行だけを動かした lib と、新しい歯を足した test file の 2 本を持つ便は、
/// lib を本体（extra）の木へ同梱して `RED-on-base ok` で通り、判定行に `fixture=1` が載る。
/// `tests_changed` は単独で撃った本体の 1 本。
///
/// base ではこの便は 2 本とも flip して lib が単独で撃たれ、`holds` が base で通るので
/// `FAIL reason=green-on-base file=<lib>` で落ちる（→ RED）。
#[test]
fn flipcheck_fixture_outside_teeth_file_is_bundled_with_the_body() {
    let dir = make_tmp_dir();
    scaffold(&dir);
    let base_lib = base_lib_with(HELPER_FN);
    let base = seed_fixture(&dir, &base_lib);
    write_at(&dir, &lib_rel(), &helper_changed_lib(&base_lib));
    write_at(&dir, &extra_rel(), &red_extra());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains("tests_changed=1"),
        "単独で撃った本体の 1 本だけを数えるはず: {}",
        got.line
    );
    assert!(
        got.line.contains("fixture=1"),
        "同梱した歯の外の file の本数が判定行に載るはず: {}",
        got.line
    );
}

/// (b) 負例: 同じ形で、動かすのが**歯の中**の 1 行（base でも通る形＝assert に説明の引数を
/// 足すだけ）なら、lib は従来どおり単独で撃たれて `green-on-base file=<lib>` で落ち、
/// `fixture` の後置を持たない（2 つを両方見る＝別の理由で通った / 落ちたを塞ぐ）。
#[test]
fn flipcheck_fixture_inside_teeth_change_is_still_shot_alone() {
    let dir = make_tmp_dir();
    scaffold(&dir);
    let base_lib = base_lib_with(HELPER_FN);
    let base = seed_fixture(&dir, &base_lib);
    write_at(
        &dir,
        &lib_rel(),
        &base_lib.replace(
            "        assert_eq!(super::val(), 1);\n",
            "        assert_eq!(super::val(), 1, \"still one\");\n",
        ),
    );
    write_at(&dir, &extra_rel(), &red_extra());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(
        &got.line,
        got.code,
        1,
        &format!("reason=green-on-base file={}", lib_rel()),
    );
    assert!(
        !got.line.contains("fixture"),
        "歯の中の行が動いた file を歯の外へ逃がさない: {}",
        got.line
    );
}

/// (c) 歯の外の file **しか**動かない便（新しい歯がどこにも無い）は、従来どおり単独で撃たれて
/// rc 1 の `FAIL reason=green-on-base` のまま落ち、`fixture` の後置を持たない（同梱の
/// fail-closed な落とし方・判定行の形は base と同じ）。
#[test]
fn flipcheck_fixture_only_change_falls_back_to_a_lone_shot() {
    let dir = make_tmp_dir();
    scaffold(&dir);
    let base_lib = base_lib_with(HELPER_FN);
    let base = seed_fixture(&dir, &base_lib);
    write_at(&dir, &lib_rel(), &helper_changed_lib(&base_lib));
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        !got.line.contains("fixture"),
        "本体の無い便は同梱せず従来どおり落ちるはず: {}",
        got.line
    );
}
