//! `check` の歯（enum と const slice の対・`enum-slices`）: `enum_slices_` を測る。
//! 共通の helper（`check_fixture` / `write_at` / `summary_fixture` 等）は親 module（`check_tests.rs`）に在り、
//! 専用の fixture（`write_enum_slice`）はこの file が持つ。
//! 純粋な移動（`check_tests.rs` から歯を足さずに写した・s2-07l.501）。札は file ごとに要る。

// flip-check: moved s2-07l.501

use super::*;

/// 擬似 workspace の core crate に enum と const slice の対を 1 つ置く。
fn write_enum_slice(dir: &Path, enum_body: &str, slice_body: &str) {
    write_at(
        dir,
        &format!("crates/{FIXTURE_CORE}/src/kinds.rs"),
        &format!(
            "/// 閉じた enum。\npub enum Kind {{\n{enum_body}}}\n\n\
             /// 全 variant。\npub const KINDS: &[Kind] = &[\n{slice_body}];\n"
        ),
    );
}

/// enum の末尾に足した variant が const slice に無い木は `enum-slices` で落ちる
/// （ADR-0013 §2.3 が「どの面も受けていない」と記録した穴・`s2-07l.88`）。集合で見るので
/// 重複と余りも落ちる（数の一致では入れ忘れと余りが相殺して「一致」に化ける・lens-88 MEDIUM-4）。
#[test]
fn enum_slices_reports_variant_missing_from_slice() {
    let violations = check_fixture(|dir| {
        write_enum_slice(
            dir,
            "    /// 1。\n    Alpha,\n\n    /// 2。\n    Beta,\n    /// 末尾に足した。\n    Gamma,\n",
            "    Kind::Alpha,\n    Kind::Beta,\n",
        );
    });
    assert_single(&violations, "enum-slices");
    let line = violations.first().map(String::as_str).unwrap_or_default();
    assert!(line.contains("Kind::Gamma"), "欠けた variant を名指す: {line}");
    // 揃っている木は通る（空行と doc は形の一部ではない・対は 1 つ数える）。
    let ok = summary_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n\n    Beta,\n    Gamma,\n",
            "    Kind::Alpha,\n    Kind::Beta,\n    Kind::Gamma,\n",
        );
    });
    assert!(ok.contains(" enum-slices=1"), "揃った対を 1 つ数える: {ok}");
    // 重複（同じ variant を 2 回）と余り（enum に無い名前）も落ちる。
    let duplicated = check_fixture(|dir| {
        write_enum_slice(dir, "    Alpha,\n    Beta,\n", "    Kind::Alpha,\n    Kind::Beta,\n    Kind::Beta,\n");
    });
    assert_single(&duplicated, "enum-slices");
    let extra = check_fixture(|dir| {
        write_enum_slice(dir, "    Alpha,\n", "    Kind::Alpha,\n    Kind::Ghost,\n");
    });
    assert_single(&extra, "enum-slices");
    assert!(
        extra.first().is_some_and(|line| line.contains("Kind::Ghost")),
        "余りの要素を名指す: {extra:?}"
    );
}

/// 型の側も黙って母集団から落とさない（lens-88 MEDIUM-1）: `&'static [Enum]` と字下げされた
/// const は対に数え、`&[&Enum]` は読めない型として違反、struct の slice は対象外（0 対）。
#[test]
fn enum_slices_covers_type_forms_instead_of_dropping_them() {
    let core_src = format!("crates/{FIXTURE_CORE}/src/kinds.rs");
    let static_form = check_fixture(|dir| {
        write_at(
            dir,
            &core_src,
            "pub enum Kind {\n    Alpha,\n    Beta,\n}\n\npub const KINDS: &'static [Kind] = &[Kind::Alpha];\n",
        );
    });
    assert_single(&static_form, "enum-slices");
    let indented = check_fixture(|dir| {
        write_at(
            dir,
            &core_src,
            "pub enum Kind {\n    Alpha,\n    Beta,\n}\n\nimpl Kind {\n    pub const ALL: &[Kind] = &[Kind::Alpha];\n}\n",
        );
    });
    assert_single(&indented, "enum-slices");
    let by_ref = check_fixture(|dir| {
        write_at(
            dir,
            &core_src,
            "pub enum Kind {\n    Alpha,\n}\n\npub const KINDS: &[&Kind] = &[&Kind::Alpha];\n",
        );
    });
    assert_single(&by_ref, "enum-slices");
    // ライフタイム付きの参照と、`:` の後に空白が無い形も逃がさない（lens-88 再確認）。
    let by_static_ref = check_fixture(|dir| {
        write_at(
            dir,
            &core_src,
            "pub enum Kind {\n    Alpha,\n}\n\npub const KINDS: &'static [&'static Kind] = &[&Kind::Alpha];\n",
        );
    });
    assert_single(&by_static_ref, "enum-slices");
    let no_space = check_fixture(|dir| {
        write_at(
            dir,
            &core_src,
            "pub enum Kind {\n    Alpha,\n    Beta,\n}\n\npub const KINDS:&[Kind] = &[Kind::Alpha];\n",
        );
    });
    assert_single(&no_space, "enum-slices");
    let of_struct = summary_fixture(|dir| {
        write_at(
            dir,
            &core_src,
            "pub struct Kind;\n\npub const KINDS: &[Kind] = &[Kind, Kind];\n",
        );
    });
    assert!(of_struct.contains(" enum-slices=0"), "struct の slice は対象外（0 対）: {of_struct}");
    // 自 workspace には ADR-0013 §2.3 が記録した 5 面が在る。黙って母集団から消えれば減る。
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let own = summary(&root);
    let pairs: usize = own
        .split(' ')
        .find_map(|token| token.strip_prefix("enum-slices="))
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("自 workspace の判定行に enum-slices の数が出るはず: {own}"));
    assert!(pairs >= 5, "自 workspace の対は 5 面以上のはず（黙って消えた）: {own}");
}

/// 読めない形（payload 付き variant・属性行・`Enum::` でない要素）は**違反に倒す**
/// （fail-closed・「読めなかった」を「一致していた」に化けさせない）。
#[test]
fn enum_slices_refuses_unrecognized_forms_instead_of_counting() {
    // (a) payload 付き variant: 数えれば 3 == 3 で「一致」に化ける形。
    let payload = check_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n    Beta(u8),\n    Gamma,\n",
            "    Kind::Alpha,\n    Kind::Beta,\n    Kind::Gamma,\n",
        );
    });
    assert_single(&payload, "enum-slices");
    // (b) 属性行が混じる形。
    let attribute = check_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n    #[default]\n    Beta,\n",
            "    Kind::Alpha,\n    Kind::Beta,\n",
        );
    });
    assert_single(&attribute, "enum-slices");
    // (c) slice の要素が `Kind::<Variant>` の形でない。
    let element = check_fixture(|dir| {
        write_enum_slice(dir, "    Alpha,\n", "    Kind::Alpha, OTHER,\n");
    });
    assert_single(&element, "enum-slices");
}

/// 集合が揃っていても**宣言順**とずれた slice は落ち、ずれた最初の添字を名指す
/// （憲法 C2「宣言順」・`s2-07l.177`）。集合だけの一致は「全部並んでいるが順序は無関係」を通す。
#[test]
fn enum_slices_order_names_the_first_mismatched_index() {
    // 逆順（添字 0 から食い違う）。
    let reversed = check_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n    Beta,\n    Gamma,\n",
            "    Kind::Gamma,\n    Kind::Beta,\n    Kind::Alpha,\n",
        );
    });
    assert_single(&reversed, "enum-slices");
    let head = reversed.first().map(String::as_str).unwrap_or_default();
    assert!(head.contains("order:KINDS"), "順序の違反として名指す: {head}");
    assert!(head.contains("expected=Kind::Alpha"), "宣言順の名を出す: {head}");
    assert!(head.contains("at=0"), "ずれた添字を出す: {head}");
    // 途中の入れ替え（添字 0 は一致・**最初の**ずれだけを 1 件出す）。
    let swapped = check_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n    Beta,\n    Gamma,\n",
            "    Kind::Alpha,\n    Kind::Gamma,\n    Kind::Beta,\n",
        );
    });
    assert_single(&swapped, "enum-slices");
    let swapped_head = swapped.first().map(String::as_str).unwrap_or_default();
    assert!(swapped_head.contains("expected=Kind::Beta"), "添字 1 の期待を出す: {swapped_head}");
    assert!(swapped_head.contains("at=1"), "最初のずれの添字: {swapped_head}");
}

/// 宣言順に並ぶ slice は違反 0 で、対は 1 つ数える（順序の面を足しても fact の形は変わらない）。
/// 欠けが在る周は添字を出さない——1 つの入れ忘れで以降の添字が丸ごとずれ、同じずれを 2 面で
/// 数えることになるからである（集合が揃うまで順序は見ない）。
#[test]
fn enum_slices_order_passes_when_slice_follows_declaration() {
    let ordered = summary_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n    Beta,\n    Gamma,\n",
            "    Kind::Alpha,\n    Kind::Beta,\n    Kind::Gamma,\n",
        );
    });
    assert!(ordered.contains(" enum-slices=1"), "宣言順の対を 1 つ数える: {ordered}");
    let missing = check_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n    Beta,\n    Gamma,\n",
            "    Kind::Gamma,\n    Kind::Alpha,\n",
        );
    });
    assert_single(&missing, "enum-slices");
    let head = missing.first().map(String::as_str).unwrap_or_default();
    assert!(head.contains("Kind::Beta"), "欠けを名指す: {head}");
    assert!(!head.contains("order:"), "集合が揃うまで添字は出さない: {head}");
}
