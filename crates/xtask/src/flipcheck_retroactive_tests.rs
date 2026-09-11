//! `flipcheck` の歯（retroactive 札）: 後から歯を足す便の逃がし札と、持ち越した札の
//! stale 判定・名前で写す `*_tests.rs` を測る。共通の helper は親 module
//! （`flipcheck_tests.rs`）に在る。

// 純粋な移動（歯を足さない・s2-07l.91）。札は file ごとに要る＝この file の差は削除にならない。
// flip-check: moved s2-07l.91

use super::*;

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
    assert!(!stale.marked(super::RETROACTIVE_MARK), "持ち越した札を数えないはず");

    // 負例。**同じ札でも base に無ければ**この便で足したものとして効く
    // （HEAD 側だけを見る実装も、両側とも無視する実装も、ここで落ちる）。
    let fresh = FilePair {
        rel: lib_rel(),
        base: Some(BASE_LIB.to_owned()),
        head: Some(head),
    };
    assert!(fresh.marked(super::RETROACTIVE_MARK), "HEAD にだけ在る札は効くはず");
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
