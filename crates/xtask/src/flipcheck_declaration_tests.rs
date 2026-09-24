//! `flipcheck` の歯（宣言 file の同梱）: 新規 module の宣言 file を本体 file の木へ同梱する
//! 判定を測る。共通の helper は親 module（`flipcheck_tests.rs`）に在る。

// 純粋な移動（歯を足さない・s2-07l.91）。札は file ごとに要る＝この file の差は削除にならない。
// flip-check: moved s2-07l.91

use super::*;

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

/// base 側に**子 module を持つ宣言 file**（`tests/e2e/seat.rs` が `mod inner;` を宣言し、
/// 本体は `tests/e2e/seat/inner.rs`）を持つ fixture を作る。
///
/// `main.rs` / `mod.rs` でない宣言 file の子 module は Rust の規則で `<stem>/<name>.rs` に
/// 置かれる。この形の便は [`base_commit_with_e2e`] の同じ dir の 2 形では作れない。
fn base_commit_with_nested_e2e() -> (PathBuf, String) {
    let (dir, _) = base_commit_with_e2e();
    write_at(&dir, &e2e_rel("main.rs"), "mod seed;\nmod seat;\n");
    write_at(&dir, &e2e_rel("seat.rs"), "mod inner;\n");
    write_at(
        &dir,
        &e2e_rel("seat/inner.rs"),
        &format!("#[test]\nfn inner_holds() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n"),
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
/// 緩める側なので弁別は狭く取る）。本便の本体 file の turn は木に宣言が無く compile されないので、
/// 判定は `not-flippable`（§55 形 4）になり、`decl=` は載らない。
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
    assert_verdict(&got.line, got.code, 1, "reason=not-flippable");
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
    assert_verdict(&got.line, got.code, 1, "reason=not-flippable");
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

/// 本体が `<stem>/<name>.rs` に置かれた新規 module も同梱する（在処判定の第 3 形・§31）。
///
/// `tests/e2e/seat.rs` が足す `mod probe;` の本体は Rust の規則で `tests/e2e/seat/probe.rs`
/// に在る。同じ dir の 2 形（`<name>.rs` / `<name>/mod.rs`）しか見ない実装は、この宣言行を
/// 落として本体を compile 対象から外し、base で赤い歯なのに `green-on-base` で落とす
/// （実測 2026-09-16・s2-07l.320 run 110459Z）。本体は base で赤いので PASS `decl=1` になる。
#[test]
fn flipcheck_declaration_nested_child_module_is_kept() {
    let (dir, base) = base_commit_with_nested_e2e();
    write_at(&dir, &e2e_rel("seat.rs"), "mod inner;\nmod probe;\n");
    write_at(&dir, &e2e_rel("seat/probe.rs"), &red_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains("decl=1"),
        "<stem>/<name>.rs 形の本体も同梱するはず: {}",
        got.line
    );
}

/// 第 3 形の同梱も **RED を捏造しない**——本体が base で緑なら FAIL のまま
/// （[`flipcheck_declaration_nested_child_module_is_kept`] の対）。
#[test]
fn flipcheck_declaration_nested_green_child_still_fails() {
    let (dir, base) = base_commit_with_nested_e2e();
    write_at(&dir, &e2e_rel("seat.rs"), "mod inner;\nmod probe;\n");
    write_at(&dir, &e2e_rel("seat/probe.rs"), &green_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        got.line.contains(&e2e_rel("seat/probe.rs")),
        "緑だった子 module の本体を名指すはず: {}",
        got.line
    );
}

// ---- 宣言と pin の file（設計 docs/design/pipeline.md §55・契約表の行 ax・s2-07l.562）----

/// fixture の統合 test の根（`tests/<name>`）の repo 相対 path。
fn root_rel(name: &str) -> String {
    format!("crates/{FIXTURE_MEMBER}/tests/{name}")
}

/// pin の歯を持つ宣言 file（`tests/e2e/main.rs`）の本文。
///
/// `mods` の後に歯 `pinned` 1 本を置く。歯は `git ls-files` で `tests/e2e` 配下の `.rs` を数えて
/// `check` の行で比べる＝本物の pin と同じ母集団の読み（flip-check の木では base の index）を再現する。
/// 数える helper は歯の後ろ（歯の外）に在る。
fn pin_main(mods: &str, check: &str) -> String {
    format!(
        "{mods}#[test]\nfn pinned() {{\n    let n = e2e_files();\n{check}\n}}\n\n\
         fn e2e_files() -> usize {{\n    let out = std::process::Command::new(\"git\")\n        \
         .args([\"ls-files\", \"tests/e2e\"])\n        .current_dir(env!(\"CARGO_MANIFEST_DIR\"))\n        \
         .output()\n        .map(|out| out.stdout)\n        .unwrap_or_default();\n    \
         String::from_utf8_lossy(&out).lines().filter(|line| line.ends_with(\".rs\")).count()\n}}\n"
    )
}

/// base 側の宣言 file が pin の歯を持つ fixture（`mods` / `check` は base の母集団 2 本に合う形）。
fn base_commit_with_pin(mods: &str, check: &str) -> (PathBuf, String) {
    let (dir, _) = base_commit_with_e2e();
    write_at(&dir, &e2e_rel("main.rs"), &pin_main(mods, check));
    head_commit(&dir);
    let base = head_sha(&dir);
    (dir, base)
}

/// 形 1〜3: `mod` 行の追加と pin の数値の +1 を持つ宣言 file を本体の木へ同梱し、pin の歯の赤を
/// turn の RED に数えず本体の歯の赤で通す。判定行は `decl=1` の直後に `pin=1` を持つ。
///
/// 同梱しない実装では本体の turn に宣言が無く `not-flippable` で落ちる。
#[test]
fn flipcheck_declaration_pin_bundles_mod_line_and_pin_number() {
    let (dir, base) = base_commit_with_pin("mod seed;\n", "    assert_eq!(n, 2);");
    write_at(&dir, &e2e_rel("main.rs"), &pin_main("mod seed;\nmod newmod;\n", "    assert_eq!(n, 3);"));
    write_at(&dir, &e2e_rel("newmod.rs"), &red_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(
        got.line.contains(" decl=1 pin=1"),
        "宣言と pin の file を decl= に数え pin= を直後に後置するはず: {}",
        got.line
    );
}

/// 形 3: 同じ便で本体だけを base で緑にすると `green-on-base file=<本体>` で落ちる。
///
/// 同梱した pin の歯は base の index を数えて毎 turn 赤い。その赤を turn の RED に数えると本体の
/// 緑が隠れる（`s2-07l.41` の fail-open と同じ型）。
#[test]
fn flipcheck_declaration_pin_red_pin_tooth_does_not_hide_green_body() {
    let (dir, base) = base_commit_with_pin("mod seed;\n", "    assert_eq!(n, 2);");
    write_at(&dir, &e2e_rel("main.rs"), &pin_main("mod seed;\nmod newmod;\n", "    assert_eq!(n, 3);"));
    write_at(&dir, &e2e_rel("newmod.rs"), &green_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    assert!(
        got.line.contains(&format!("file={}", e2e_rel("newmod.rs"))),
        "緑だった本体 file を名指すはず: {}",
        got.line
    );
}

/// 形 1 の狭さ（数値の差が 2 か所）: 形に当たらず本体へ落ち、`pin=` は載らない（形 4 で落ちる）。
#[test]
fn flipcheck_declaration_pin_two_number_changes_are_not_bundled() {
    let (dir, base) = base_commit_with_pin("mod seed;\n", "    assert_eq!(n * 1, 2);");
    write_at(&dir, &e2e_rel("main.rs"), &pin_main("mod seed;\nmod newmod;\n", "    assert_eq!(n * 2, 6);"));
    write_at(&dir, &e2e_rel("newmod.rs"), &red_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=not-flippable");
    assert!(!got.line.contains("pin="), "差が 2 か所の file を pin と数えない: {}", got.line);
}

/// 形 1 の狭さ（pin の行が歯の外の helper の中）: 形に当たらず、`pin=` は載らない。
#[test]
fn flipcheck_declaration_pin_number_outside_teeth_is_not_bundled_as_pin() {
    let helper = |value: u32| format!("fn expected() -> usize {{\n    {value}\n}}\n");
    let (dir, base) = base_commit_with_pin(&format!("mod seed;\n{}", helper(2)), "    assert_eq!(n, expected());");
    write_at(
        &dir,
        &e2e_rel("main.rs"),
        &pin_main(&format!("mod seed;\nmod newmod;\n{}", helper(3)), "    assert_eq!(n, expected());"),
    );
    write_at(&dir, &e2e_rel("newmod.rs"), &red_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert!(!got.line.contains("pin="), "歯の外の数値の差を pin と数えない: {}", got.line);
}

/// 形 4: 宣言 file が自前の歯の本文も動かした便は宣言が同梱されず、新設 module の turn は木に宣言が
/// 無い＝nextest を撃たずに `not-flippable files=<本体>` で落ちる（`green-on-base` と呼ばない）。
#[test]
fn flipcheck_declaration_pin_undeclared_new_module_is_not_flippable() {
    let (dir, base) = base_commit_with_pin("mod seed;\n", "    assert_eq!(n, 2);");
    write_at(
        &dir,
        &e2e_rel("main.rs"),
        &pin_main("mod seed;\nmod newmod;\n", "    assert_eq!(n, 3);\n    assert!(n > 0);"),
    );
    write_at(&dir, &e2e_rel("newmod.rs"), &red_body());
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    assert_verdict(&got.line, got.code, 1, "reason=not-flippable");
    assert!(
        got.line.contains(&format!("files={}", e2e_rel("newmod.rs"))),
        "宣言の無い新設 module を名指すはず: {}",
        got.line
    );
    assert!(!got.line.contains("green-on-base"), "compile されない歯の緑を読まない: {}", got.line);
}

/// 形 3 の純関数: 落ちた歯が除く名だけ → 緑、除く名の外に 1 本 → RED、名指し 0 本 → 名指せない。
#[test]
fn flipcheck_declaration_pin_bundled_turn_reads_three_ways() {
    let own = ["pinned"];
    assert_eq!(super::super::bundled_turn(&["pinned"], &own), super::super::BundledTurn::Green);
    assert_eq!(
        super::super::bundled_turn(&["pinned", "newmod::probe"], &own),
        super::super::BundledTurn::Red
    );
    assert_eq!(super::super::bundled_turn(&[], &own), super::super::BundledTurn::Unnamed);
}

/// 形 4 の在処: target の根 `tests/<f>.rs`（`main.rs` でない名）の `mod <d>;` で宣言された新規
/// `tests/<d>/mod.rs` の turn は撃たれ、同じ便から根の `mod` 行を消すと `not-flippable` で落ちる。
///
/// 在処を本体の dir の 3 形に閉じる変異は前半を、在処を見ない変異は後半を落とす。
#[test]
fn flipcheck_declaration_pin_root_file_declares_module_dir() {
    let alt = |mods: &str| format!("{mods}#[test]\nfn alt_holds() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n");
    let fixture = |declare: bool| {
        let (dir, _) = base_commit_with_e2e();
        write_at(&dir, &root_rel("alt.rs"), &alt(""));
        head_commit(&dir);
        let base = head_sha(&dir);
        if declare {
            write_at(&dir, &root_rel("alt.rs"), &alt("mod pinmod;\n"));
        }
        write_at(&dir, &root_rel("pinmod/mod.rs"), &red_body());
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        got
    };
    let got = fixture(true);
    assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
    assert!(got.line.contains("decl=1"), "根の宣言 file を同梱するはず: {}", got.line);

    let got = fixture(false);
    assert_verdict(&got.line, got.code, 1, "reason=not-flippable");
    assert!(
        got.line.contains(&format!("files={}", root_rel("pinmod/mod.rs"))),
        "宣言の無い tests/<d>/mod.rs を名指すはず: {}",
        got.line
    );
}
