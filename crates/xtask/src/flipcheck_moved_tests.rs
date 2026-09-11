//! `flipcheck` の歯（moved 札）: 歯を 1 本も足さない純粋な移動の便の逃がし札を測る。
//! 共通の helper は親 module（`flipcheck_tests.rs`）に在る。

// 純粋な移動（歯を足さない・s2-07l.91）。札は file ごとに要る＝この file の差は削除にならない。
// flip-check: moved s2-07l.91

use super::*;

/// **純粋な移動の札**（`// flip-check: moved <bead-id>`）は RED を免除し、判定行に `moved=1`。
///
/// `retroactive` を転用しない理由は判定行の読みである——あちらの `retroactive=N` は
/// 「後から足した歯が N 本」と読まれるので、歯を 1 本も足さない移動の便に貼ると、
/// その数が何を免除したのか読めなくなる（`s2-07l.84` で実際に貼ってしまった）。
#[test]
fn flip_check_reports_moved_marker_for_a_pure_move() {
    let got = judge_lib(
        "pub fn val() -> u32 {\n    1\n}\n#[cfg(test)]\nmod checks {\n    #[test]\n    fn holds() {\n        assert_eq!(super::val(), 1);\n    }\n}\n",
        "pub fn val() -> u32 {\n    1\n}\n#[cfg(test)]\nmod checks {\n    // flip-check: moved s2-07l.86\n    #[test]\n    fn holds() {\n        assert_eq!(super::val(), 1);\n    }\n}\n",
    );
    assert_eq!(got.code, 0, "純粋な移動の札は通す: {}", got.line);
    assert!(got.line.contains("moved=1"), "判定行に moved=1 が載る: {}", got.line);
    assert!(!got.line.contains("retroactive"), "retroactive は数えない: {}", got.line);
}

/// **新規 module に置いた `moved` 札**は `not-flippable` ではなく `moved` へ倒れる。
///
/// `no_flip_verdict` の stderr はこの逃がしを名指して案内するので、案内どおりに置いた便が
/// 落ちると案内と挙動が食い違う（`retroactive` 側の対は「marker を置いた新規 module」の歯）。
#[test]
fn flip_check_treats_moved_marker_on_a_new_module_as_moved() {
    let (dir, base) = base_commit();
    write_at(
        &dir,
        &format!("crates/{FIXTURE_MEMBER}/src/shifted.rs"),
        "pub fn w() -> u32 {\n    2\n}\n#[cfg(test)]\nmod t {\n    // flip-check: moved s2-07l.86\n    #[test]\n    fn probe() {\n        assert_eq!(super::w(), 2);\n    }\n}\n",
    );
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains("moved=1") && !got.line.contains("not-flippable"),
        "moved 札付きの新規 module は moved へ倒れるはず: {}",
        got.line
    );
}

/// `moved` は `retroactive` と**同じ 4 条件**でしか効かない（緩めない）。
///
/// 4 条件 = test 区間内 / 行頭 / bead id 必須 / base から持ち越した札は効かない。
/// 1 つずつ壊して、どれを壊しても免除が消えることを測る。
#[test]
fn flip_check_moved_marker_needs_the_same_four_conditions() {
    let base = "pub fn val() -> u32 {\n    1\n}\n#[cfg(test)]\nmod checks {\n    #[test]\n    fn holds() {\n        assert_eq!(super::val(), 1);\n    }\n}\n";
    // (a) src 区間の札は効かない（実装の隣に 1 行足すだけで検査を外せる形にしない）。
    let in_src = judge_lib(
        base,
        "// flip-check: moved s2-07l.86\npub fn val() -> u32 {\n    2\n}\n#[cfg(test)]\nmod checks {\n    #[test]\n    fn holds() {\n        assert_eq!(super::val(), 2);\n    }\n}\n",
    );
    assert!(!in_src.line.contains("moved="), "src 区間の札は数えない: {}", in_src.line);
    // (b) bead id が無い札は効かない（誰にも辿れない逃がしは静かな逃がしと同じ）。
    for bare in [
        "// flip-check: moved",
        "// flip-check: moveds2-07l.86",
        // 区切りの空白は在るが id が無い形（この 1 本だけが id 要求を測る。
        // 空白の無い 2 形は prefix 不一致で外れるので、id の空判定には届かない）。
        "// flip-check: moved ",
    ] {
        let head = base.replace("mod checks {\n", &format!("mod checks {{\n    {bare}\n"));
        let no_bead = judge_lib(base, &head);
        assert_not_retroactive(&no_bead, bare);
        assert!(!no_bead.line.contains("moved="), "id の無い札は数えない: {}", no_bead.line);
    }
    // (c) 行頭で見る（字面の言及は拾わない）。
    let mentioned = judge_lib(
        base,
        "pub fn val() -> u32 {\n    1\n}\n#[cfg(test)]\nmod checks {\n    #[test]\n    fn holds() {\n        let _ = \"// flip-check: moved s2-07l.86\";\n        assert_eq!(super::val(), 1);\n    }\n}\n",
    );
    assert!(!mentioned.line.contains("moved="), "字面の言及は数えない: {}", mentioned.line);
}

/// **base から持ち越した `moved` 札は効かない**（4 条件の 4 つ目）。
///
/// 札は file に残るので、在るだけで数えると一度貼った札がその file の test 区間を触る
/// 以後のすべての便を免除する——`retroactive` が塞いだ穴と同じものが `moved` 側に開く。
#[test]
fn flip_check_ignores_a_carried_over_moved_marker() {
    let carried = "pub fn val() -> u32 {\n    1\n}\n#[cfg(test)]\nmod checks {\n    // flip-check: moved s2-07l.86\n    #[test]\n    fn holds() {\n        assert_eq!(super::val(), 1);\n    }\n}\n";
    let head = "pub fn val() -> u32 {\n    1\n}\n#[cfg(test)]\nmod checks {\n    // flip-check: moved s2-07l.86\n    #[test]\n    fn holds() {\n        assert_eq!(super::val(), 1);\n    }\n\n    #[test]\n    fn added() {\n        assert_eq!(super::val(), 1);\n    }\n}\n";
    // **理由を渡すところまで測る**（`judge_into` の sink 越し）。免除しないだけで黙ると、
    // 書いた人は免除したつもりで RED を要求され、判定行から理由を読めない。
    let (dir, _) = base_commit();
    let base = seed_fixture(&dir, carried);
    write_at(&dir, &lib_rel(), head);
    head_commit(&dir);
    let mut lines: Vec<String> = Vec::new();
    let got = judge_into(&base, &dir, &mut |line| lines.push(line.to_owned()));
    drop_fixture(&dir);
    assert_not_retroactive(&got, "持ち越した moved 札だけの便");
    assert!(!got.line.contains("moved="), "持ち越した札は数えない: {}", got.line);
    let stale: Vec<&String> = lines
        .iter()
        .filter(|line| line.starts_with("flip-check: stale-marker "))
        .collect();
    assert_eq!(stale.len(), 1, "効かない moved 札を 1 行で名指すはず: {lines:?}");
}
