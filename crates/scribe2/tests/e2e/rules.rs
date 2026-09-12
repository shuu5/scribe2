//! rules manifest の歯（設計 docs/design/rules-manifest.md §7）。
//!
//! fixture は文字列 literal で持つ（file を置くと歯が repo の状態に依存する）。

use crate::make_tmp_dir;
use std::process::Command;
use vessel::cli_outcome::{Outcome, RC_OK, RC_REFUSED};
use vessel::order::is_declaration_order;
use vessel::rules::manifest::Manifest;
use vessel::rules::{Rule, RuleKind, RuleValue, ValueShape, ALL};

/// 受理される最小の manifest（2 行）。`enabled` は**全行に書く**（必須 key）。
const GOOD: &str = r#"schema = 1

[[rule]]
id = "R-C4-1"
kind = "CoreLines"
value = 20000
enabled = true
ruling = "r"
ruled_at = "2026-09-07"

[[rule]]
id = "R-C7-1"
kind = "DialogueSurface"
value = "user-direct"
enabled = true
ruling = "r"
ruled_at = "2026-09-09"
"#;

/// 欠陥 3 箇所（11 行目 = id 重複 / 19 行目 = 未知 kind / 27 行目 = ruling 欠け）。
const DEFECTIVE: &str = r#"schema = 1

[[rule]]
id = "a"
kind = "CoreLines"
value = 1
enabled = true
ruling = "r"
ruled_at = "d"

[[rule]]
id = "a"
kind = "ModuleLines"
value = 2
enabled = true
ruling = "r"
ruled_at = "d"

[[rule]]
id = "b"
kind = "Nope"
value = 3
enabled = true
ruling = "r"
ruled_at = "d"

[[rule]]
id = "c"
kind = "FnLines"
value = 4
enabled = true
ruled_at = "d"
"#;

/// 1 行だけの fixture を組む。値は kind の形に合わせる。
fn one_row(kind: RuleKind, value: &str) -> String {
    format!(
        "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"{}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"2026-09-09\"\n",
        kind.as_str()
    )
}

/// 種類の形に合う値の字面。
fn sample_value(kind: RuleKind) -> String {
    match kind.shape() {
        ValueShape::Int => "1".to_owned(),
        ValueShape::Str | ValueShape::Policy => "\"sample\"".to_owned(),
        ValueShape::List => "[\"sample\"]".to_owned(),
    }
}

/// List の kind を 1 つ（歯の fixture 用）。`ALL` の順に依らず名前で選ぶ。
const LIST_KIND: RuleKind = RuleKind::RunnerAllowedCommands;

#[test]
fn rules_list_parses_string_array() {
    let text = one_row(LIST_KIND, r#"["cargo", "git"]"#);
    let manifest = parsed(&text).expect("受理されるはずの fixture が拒まれた");
    let row = manifest.get("probe").expect("probe が在る");
    assert_eq!(
        row.value,
        RuleValue::List(vec!["cargo".to_owned(), "git".to_owned()]),
        "要素は書いた順のまま"
    );
    assert_eq!(row.kind.shape(), ValueShape::List, "kind の形");
}

#[test]
fn rules_list_rejects_empty_array() {
    // 空の配列は「規則が無い」ではなく書き間違いである（空の allowlist を黙って効かせない）。
    let errors = rejected(&one_row(LIST_KIND, "[]")).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("配列が空である"), "理由: {first}");
    assert!(first.contains("line="), "行番号: {first}");
}

#[test]
fn rules_list_rejects_non_string_element() {
    let errors = rejected(&one_row(LIST_KIND, r#"["cargo", 1]"#))
        .expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("引用符 1 組の文字列でない"), "理由: {first}");
}

#[test]
fn rules_list_rejects_unseparated_elements() {
    // `["a" "b"]` を 1 本の壊れた文字列として黙って通さない（書いた本数と通る本数の食い違い）。
    let errors = rejected(&one_row(LIST_KIND, r#"["cargo" "git"]"#))
        .expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("引用符 1 組の文字列でない"), "理由: {first}");
}

#[test]
fn rules_list_rejects_empty_element() {
    // 空文字の command 名・verify 行は「何もしない口」ゆえ受けない。
    let errors = rejected(&one_row(LIST_KIND, r#"["cargo", ""]"#))
        .expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("空文字"), "理由: {first}");
}

#[test]
fn rules_list_rejects_scalar_for_list_kind() {
    // List を要求する kind に文字列を渡したら loud（AC6）。
    let errors = rejected(&one_row(LIST_KIND, "\"cargo\""))
        .expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("形と合わない"), "理由: {first}");
    assert!(first.contains("List"), "要求する形を名指す: {first}");
}

#[test]
fn rules_list_rejects_array_for_scalar_kind() {
    // 逆向きの負例（配列を受ける口が、配列でない kind まで通していないか）。
    let errors = rejected(&one_row(RuleKind::CoreLines, r#"["1"]"#))
        .expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("形と合わない"), "理由: {first}");
}

/// 埋め込み manifest は**上限の行を持ち、共通 verify の行を持たない**。
///
/// 共通 verify の値は対象 repo の vessel 宣言 `common-verify` が持つ（ADR-0010 §2.2・
/// 裁定 id = ADR-0010）。**行と variant は 1 PR で揃える**——片側だけ消すと、残った行の
/// `kind` が閉じた enum の外になり `Manifest::embedded()` 自体が拒まれる（この歯が落ちる）。
#[test]
fn rules_embedded_manifest_carries_allowlist_and_has_no_common_verify_row() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let allowed = manifest.get("runner.allowed_commands").expect("上限の行が在る");
    assert_eq!(
        allowed.value,
        RuleValue::List(vec!["cargo".to_owned(), "git".to_owned()]),
        "user 裁定 2026-09-10 の上限"
    );
    assert_eq!(allowed.ruling, "user 裁定 2026-09-10（ADR-0009）", "裁定: {}", allowed.id);
    assert_eq!(allowed.ruled_at, "2026-09-10", "裁定日: {}", allowed.id);
    assert!(allowed.enabled, "既定で効く: {}", allowed.id);

    assert!(
        manifest.get("gate.common_verify").is_none(),
        "廃止した行は manifest に無い（母集団 {} 行）",
        manifest.rows().len()
    );
    // **行 id の集合 ⊆ RuleKind の as_str 集合**。除去し忘れた行が残れば、その kind が
    // 閉じた enum の外になって上の `embedded()` が落ちる＝2 面が同時に動く。
    let kinds: Vec<&str> = ALL.iter().map(|kind| kind.as_str()).collect();
    assert!(
        !kinds.contains(&"GateCommonVerify"),
        "廃止した variant は ALL に無い（母集団 {} 種）",
        kinds.len()
    );
    for row in manifest.rows() {
        assert!(
            kinds.contains(&row.kind.as_str()),
            "行 {} の kind {} は閉じた enum の内（母集団 {} 種）",
            row.id,
            row.kind.as_str(),
            kinds.len()
        );
    }
}

#[test]
fn rules_list_keeps_comma_inside_quotes() {
    // 要素の中の `,` は区切りではない。共通 verify の行は `,` を含みうるので、
    // ここを割ると **書いた本数と通る本数が食い違う**（1 行が 2 本に化ける）。
    let text = one_row(LIST_KIND, r#"["cargo test --features a,b", "git"]"#);
    let manifest = parsed(&text).expect("受理されるはずの fixture が拒まれた");
    let row = manifest.get("probe").expect("probe が在る");
    assert_eq!(
        row.value,
        RuleValue::List(vec!["cargo test --features a,b".to_owned(), "git".to_owned()]),
        "quote の内側の , で割らない（母集団 2 要素）"
    );
}

#[test]
fn rules_list_rejects_unclosed_bracket() {
    // 配列は 1 行で閉じる（要素に改行を置けない）。
    let errors = rejected(&one_row(LIST_KIND, r#"["cargo""#))
        .expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("同じ行で閉じていない"), "理由: {first}");
}

#[test]
fn rules_list_rejects_unclosed_quote() {
    let errors = rejected(&one_row(LIST_KIND, r#"["cargo", "git]"#))
        .expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("引用符が閉じていない"), "理由: {first}");
}

#[test]
fn rules_manifest_reports_broken_value_once() {
    // 読めなかった値は scan が 1 件報告する。後段が「必須 key が無い」と**嘘の 2 行目**を
    // 足さないこと（key は在って値が壊れている）。value 以外の key でも同じ。
    let text = "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"CoreLines\"\nvalue = 1\nenabled = true\nruling = 1.5\nruled_at = \"d\"\n";
    let errors = rejected(text).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数（同じ欠陥を 2 行にしない）: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("TOML subset の形でない"), "理由: {first}");
    assert!(!first.contains("必須 key"), "「無い」と言わない: {first}");
}

#[test]
fn rules_list_cli_get_renders_every_element() {
    // 1 行表示で**要素の区切りが読める**こと（空白で継ぐと、空白を含む要素が
    // 何本あるのか読めない）。
    let args = ["get".to_owned(), "runner.allowed_commands".to_owned()];
    let outcome = vessel::rules::cli::dispatch(&args);
    assert_eq!(outcome.rc, RC_OK, "rc: {outcome:?}");
    assert_eq!(
        outcome.out,
        vec!["[\"cargo\", \"git\"]".to_owned()],
        "値の行"
    );
}

/// 受理されるはずの fixture を読む。拒まれたら理由を 1 本の文字列にして `Err`。
///
/// helper の中で `panic!` を撃たないのは、clippy の `allow-panic-in-tests` が
/// `#[test]` 関数の中だけに効き、統合 test の helper 関数には効かないためである。
fn parsed(text: &str) -> Result<Manifest, String> {
    Manifest::parse(text).map_err(|errors| {
        let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
        lines.join("\n")
    })
}

/// 拒まれるはずの fixture の理由行。受理されてしまったら行数を `Err` で返す。
fn rejected(text: &str) -> Result<Vec<String>, usize> {
    match Manifest::parse(text) {
        Ok(found) => Err(found.rows().len()),
        Err(errors) => Ok(errors.iter().map(ToString::to_string).collect()),
    }
}

#[test]
fn outcome_rules_and_fleet_return_the_same_type() {
    let from_rules = vessel::rules::cli::dispatch(&["validate".to_owned()]);
    let from_fleet = vessel::fleet::cli::dispatch(&[]);
    // 2 型が並んでいると、この Vec が型不一致で compile error になる（憲法 C2）。
    let both: Vec<Outcome> = vec![from_rules, from_fleet];
    assert_eq!(both.len(), 2, "1 つの Vec に入る＝同じ型");
    assert_eq!(both.first().map(|o| o.rc), Some(RC_OK), "rules validate は rc 0");
    assert_eq!(
        both.get(1).map(|o| o.rc),
        Some(RC_REFUSED),
        "引数の無い fleet は rc 1"
    );
}

#[test]
fn rules_manifest_accepts_good_fixture() {
    let manifest = parsed(GOOD).expect("受理されるはずの fixture が拒まれた");
    assert_eq!(manifest.rows().len(), 2, "行数");
    let row = manifest.get("R-C4-1").expect("R-C4-1 が在る");
    assert_eq!(row.value, RuleValue::Int(20_000), "閾値");
    assert!(row.enabled, "書いた enabled がそのまま載る（省略は拒まれる）");
    let surface = manifest.get("R-C7-1").expect("R-C7-1 が在る");
    assert_eq!(
        surface.value,
        RuleValue::Str("user-direct".to_owned()),
        "識別子"
    );
}

#[test]
fn rules_manifest_rejects_unknown_kind() {
    let errors = rejected(&one_row_raw("Nope", "1")).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("未知である"), "理由: {first}");
    assert!(first.contains("line="), "行番号: {first}");
}

/// kind の字面を直に差し込む fixture（未知 kind を作るため）。
fn one_row_raw(kind: &str, value: &str) -> String {
    format!(
        "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"2026-09-09\"\n"
    )
}

#[test]
fn rules_manifest_rejects_duplicate_id() {
    let text = "schema = 1\n\n[[rule]]\nid = \"same\"\nkind = \"CoreLines\"\nvalue = 1\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n\n[[rule]]\nid = \"same\"\nkind = \"FnLines\"\nvalue = 2\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n";
    let errors = rejected(text).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("重複"), "理由: {first}");
}

#[test]
fn rules_manifest_rejects_row_without_ruling() {
    let text = "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"CoreLines\"\nvalue = 1\nenabled = true\nruled_at = \"d\"\n";
    let errors = rejected(text).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("必須 key ruling"), "理由: {first}");
}

/// `enabled` の欠落は**既定 true で埋めない**（裁定 id `user 2026-09-11T23:59Z`）。
///
/// 「断ってから解いて通す」形で測る——足せば通ることまで見ないと、別の理由で拒まれている
/// 周と区別がつかない。埋めていた間は、書き忘れた行が「効く」側へ黙って倒れていた。
#[test]
fn rules_manifest_rejects_row_without_enabled() {
    let text = "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"CoreLines\"\nvalue = 1\nruling = \"r\"\nruled_at = \"d\"\n";
    let errors = rejected(text).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("必須 key enabled"), "理由: {first}");
    assert!(first.contains("line=3"), "行番号: {first}");
    let healed = "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"CoreLines\"\nvalue = 1\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n";
    let manifest = parsed(healed).expect("enabled を足せば通る");
    let row = manifest.get("probe").expect("probe が在る");
    assert!(row.enabled, "書いた値がそのまま載る");
}

/// `ruled_at` の欠落も空文字で埋めない（同じ裁定・`ruling` と同じ形）。
#[test]
fn rules_manifest_rejects_row_without_ruled_at() {
    let text = "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"CoreLines\"\nvalue = 1\nenabled = true\nruling = \"r\"\n";
    let errors = rejected(text).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("必須 key ruled_at"), "理由: {first}");
    assert!(first.contains("line=3"), "行番号: {first}");
    let healed = "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"CoreLines\"\nvalue = 1\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n";
    let manifest = parsed(healed).expect("ruled_at を足せば通る");
    let row = manifest.get("probe").expect("probe が在る");
    assert_eq!(row.ruled_at, "d", "書いた値がそのまま載る");
}

#[test]
fn rules_manifest_rejects_value_type_mismatch() {
    let errors = rejected(&one_row(RuleKind::CoreLines, "\"twenty\"")).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("形と合わない"), "理由: {first}");
}

#[test]
fn rules_manifest_rejects_missing_schema() {
    let text = "[[rule]]\nid = \"probe\"\nkind = \"CoreLines\"\nvalue = 1\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n";
    let errors = rejected(text).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("schema = 1 が無い"), "理由: {first}");
}

#[test]
fn rules_manifest_rejects_duplicate_key_in_row() {
    let text = "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"GateTokenCap\"\nvalue = 1\nenabled = true\nenabled = false\nruling = \"r\"\nruled_at = \"d\"\n";
    let errors = rejected(text).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("key enabled が重複する"), "理由: {first}");
}

#[test]
fn rules_manifest_rejects_duplicate_schema() {
    let text = "schema = 7\nschema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"GateTokenCap\"\nvalue = 1\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n";
    let errors = rejected(text).expect("拒まれるはずの fixture が受理された");
    let joined = errors.join("\n");
    assert!(joined.contains("schema が重複する"), "理由: {joined}");
}

#[test]
fn rules_manifest_rejects_empty_id() {
    let text = "schema = 1\n\n[[rule]]\nid = \"\"\nkind = \"GateTokenCap\"\nvalue = 1\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n";
    let errors = rejected(text).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("id が空である"), "理由: {first}");
}

#[test]
fn rules_cli_refuses_rules_flag_without_path() {
    let args = ["validate".to_owned(), "--rules".to_owned()];
    let outcome = vessel::rules::cli::dispatch(&args);
    assert_eq!(outcome.rc, RC_REFUSED, "PATH の無い --rules は rc 1");
    assert!(outcome.out.is_empty(), "埋め込みへ倒れない");
    let joined = outcome.err.join("\n");
    assert!(joined.contains("--rules に PATH が無い"), "断りの行: {joined}");
}

#[test]
fn rules_manifest_reports_all_errors_with_line_numbers() {
    let errors = rejected(DEFECTIVE).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 3, "欠陥 3 箇所は 3 行になる: {errors:?}");
    let joined = errors.join("\n");
    for want in ["line=11", "line=19", "line=27"] {
        assert!(joined.contains(want), "{want} が無い:\n{joined}");
    }
}

#[test]
fn rules_kind_parity_every_kind_has_sample() {
    for kind in ALL {
        let text = one_row(*kind, &sample_value(*kind));
        let manifest = parsed(&text).expect("受理されるはずの fixture が拒まれた");
        let row = manifest.get("probe").expect("probe が在る");
        assert_eq!(row.kind(), *kind, "kind: {}", kind.as_str());
        assert!(
            row.validate().is_ok(),
            "validate: {}",
            kind.as_str()
        );
    }
}

/// `[[account]]` を `labels` の順で持つ fixture（`[[rule]]` 1 行の後ろに並べる）。
fn accounts_fixture(labels: &[&str]) -> String {
    let mut text = String::from(GOOD);
    for label in labels {
        text.push_str(&format!("\n[[account]]\nlabel = \"{label}\"\n"));
    }
    text
}

/// 口座の宣言は**宣言順**で返り、`[[rule]]` 行の読みは 1 つも動かない（歯 (b)(1)(3)）。
#[test]
fn rules_accounts_are_returned_in_declaration_order() {
    let manifest = parsed(&accounts_fixture(&["a3", "a1", "a2"]))
        .expect("受理されるはずの fixture が拒まれた");
    let labels: Vec<&str> = manifest
        .accounts()
        .iter()
        .map(vessel::rules::manifest::AccountLabel::label)
        .collect();
    assert_eq!(labels, vec!["a3", "a1", "a2"], "書いた順のまま（並べ替えない）");
    assert_eq!(manifest.rows().len(), 2, "同じ file の [[rule]] 行は 2 行のまま");
    let row = manifest.get("R-C4-1").expect("R-C4-1 が在る");
    assert_eq!(row.value, RuleValue::Int(20_000), "規則の値は変わらない");
    assert_eq!(row.ruling, "r", "裁定の読みも変わらない");
    let bare = parsed(GOOD).expect("受理されるはずの fixture が拒まれた");
    assert!(bare.accounts().is_empty(), "[[account]] が無い manifest は 0 件");
}

/// 口座の宣言の 4 つの壊し方は**それぞれ**行番号つきの error になる（歯 (b)(2)）。
#[test]
fn rules_accounts_reject_each_broken_row_with_line_numbers() {
    let cases: [(&str, &str, u64); 4] = [
        ("[[account]]\nlabel = \"a1\"\nhost = \"nope\"\n", "未知の key host", 21),
        ("[[account]]\nlabel = \"\"\n", "label が空である", 19),
        ("[[account]]\nlabel = \"a1\"\n\n[[account]]\nlabel = \"a1\"\n", "label a1 が重複する", 22),
        ("[[account]]\n", "必須 key label が無い", 19),
    ];
    for (tail, want, line) in cases {
        let text = format!("{GOOD}\n{tail}");
        let errors = rejected(&text).expect("拒まれるはずの fixture が受理された");
        assert_eq!(errors.len(), 1, "件数（{want}）: {errors:?}");
        let first = errors.first().map(String::as_str).unwrap_or_default();
        assert!(first.contains(want), "理由: {first}");
        assert!(first.contains(&format!("line={line}")), "行番号: {first}");
    }
}

/// `[[rule]]` の検査は `[[account]]` を混ぜても不変（規則側の必須 key は要り続ける）。
#[test]
fn rules_accounts_do_not_loosen_rule_rows() {
    let text = format!(
        "{}\n[[rule]]\nid = \"probe\"\nkind = \"CoreLines\"\nvalue = 1\nenabled = true\nruled_at = \"d\"\n",
        accounts_fixture(&["a1"])
    );
    let errors = rejected(&text).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("必須 key ruling"), "理由: {first}");
    // 逆向き: 口座に規則の key を書いても通らない（key 集合は section ごとである）。
    let mixed = format!("{GOOD}\n[[account]]\nlabel = \"a1\"\nenabled = true\n");
    let errors = rejected(&mixed).expect("拒まれるはずの fixture が受理された");
    let joined = errors.join("\n");
    assert!(joined.contains("未知の key enabled"), "理由: {joined}");
}

/// 未知の section は受理しない（受ける section は 2 つちょうど）。
#[test]
fn rules_manifest_rejects_unknown_section() {
    let text = format!("{GOOD}\n[[seat]]\nlabel = \"a1\"\n");
    let errors = rejected(&text).expect("拒まれるはずの fixture が受理された");
    let joined = errors.join("\n");
    assert!(joined.contains("未知の section [[seat]]"), "理由: {joined}");
    assert!(joined.contains("[[rule]]") && joined.contains("[[account]]"), "受理する形を名指す: {joined}");
}

/// tracked manifest は口座 5 件と待ち時間の行を持つ（歯 (b)(4)）。
#[test]
fn rules_embedded_manifest_declares_five_accounts_and_usage_timeout() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let labels: Vec<&str> = manifest
        .accounts()
        .iter()
        .map(vessel::rules::manifest::AccountLabel::label)
        .collect();
    assert_eq!(labels.len(), 5, "宣言した口座の母集団: {labels:?}");
    for label in &labels {
        assert!(!label.is_empty(), "label は空でない: {labels:?}");
    }
    let timeout = manifest.get("fleet.usage_timeout_s").expect("待ち時間の行が在る");
    assert_eq!(timeout.value, RuleValue::Int(30), "user 裁定 2026-09-12T02:01Z の値");
    assert_eq!(timeout.kind, RuleKind::UsageTimeoutS, "kind");
    assert_eq!(timeout.kind.shape(), ValueShape::Int, "値の形は Int（秒）");
    assert!(timeout.enabled, "既定で効く");
    assert_eq!(timeout.ruling, "user 2026-09-12T02:01Z", "裁定 id");
    assert_eq!(timeout.ruled_at, "2026-09-12", "裁定日");
}

/// 起こし直しの回数の行（`pipe.follow_retries`・裁定 id `user 2026-09-12T03:25Z`・
/// 設計 pipeline-conflict.md §5）。**値は manifest が持ち、ADR も設計 doc も写さない**（C1 / C5）。
/// 行が欠けた manifest は `RuleError` で拒まれる（kind の字面は `ALL` を通してしか解けない）。
#[test]
fn rules_embedded_manifest_declares_follow_retries() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let row = manifest.get("pipe.follow_retries").expect("起こし直しの回数の行が在る");
    assert_eq!(row.value, RuleValue::Int(2), "user 裁定 2026-09-12T03:25Z の値");
    assert_eq!(row.kind, RuleKind::FollowRetries, "kind");
    assert_eq!(row.kind.shape(), ValueShape::Int, "値の形は Int（回）");
    assert!(row.enabled, "既定で効く");
    assert_eq!(row.ruling, "user 2026-09-12T03:25Z", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-12", "裁定日");
    // 未知の kind は `parse` できない＝行を落とした manifest は読めない（親 test と同じ形）。
    let errors = rejected(&one_row_raw("FollowRetry", "2")).expect("未知の kind の fixture が受理された");
    let joined = errors.join("\n");
    assert!(joined.contains("未知である"), "行の kind を綴り違えた manifest は読めない: {joined}");
}

/// gate の費用の 5 行（設計 gate-cost.md §3.1・ADR-0021）。**値は manifest が持ち、ADR も
/// 設計 doc も写さない**（C1 / C5）。
///
/// kind の包含を 5 行まとめて測るのは、行と variant を**対で**足させるためである——片方だけ
/// 足した manifest は `parse` できず（未知の kind）、片方だけ足した enum は行の無い variant を
/// 残す（親 test の `covers_all_kinds` が落ちる）。**行数は pin しない**（他便と同時に並ぶと
/// 順序次第で動く数であり、母集団の健全性は親 test が持つ）。
#[test]
fn rules_embedded_manifest_declares_the_gate_cost_rows() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    // (id, kind, 値, 裁定 id)。値は **user 2026-09-12 の裁定**（台帳 s2-07l.153 notes 逐語）。
    let rows: [(&str, RuleKind, u64, &str); 5] = [
        ("gate.mutants_jobs", RuleKind::GateMutantsJobs, 4, "user 2026-09-12T11:42Z"),
        ("gate.job_memory_mb", RuleKind::GateJobMemoryMb, 3072, "user 2026-09-12T12:08Z"),
        ("host.reserve_memory_mb", RuleKind::HostReserveMemoryMb, 8192, "user 2026-09-12T12:08Z"),
        ("gate.slot_wait_s", RuleKind::GateSlotWaitS, 900, "user 2026-09-12T12:08Z"),
        ("gate.cpu_weight", RuleKind::GateCpuWeight, 50, "user 2026-09-12T12:08Z"),
    ];
    for (id, kind, value, ruling) in rows {
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!(row.value, RuleValue::Int(value), "{id} の値");
        assert_eq!(row.kind, kind, "{id} の kind");
        assert_eq!(row.kind.shape(), ValueShape::Int, "{id} の値の形");
        assert!(row.enabled, "{id} は既定で効く");
        assert_eq!(row.ruling, ruling, "{id} の裁定 id");
        assert_eq!(row.ruled_at, "2026-09-12", "{id} の裁定日");
        // kind の字面は閉じた enum を通してしか解けない（綴り違いの manifest は読めない）。
        assert_eq!(RuleKind::parse(kind.as_str()), Some(kind), "{id} の kind を字面から引ける");
    }
}

#[test]
fn rules_embedded_manifest_is_valid_and_covers_all_kinds() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    // **tracked な `rules/manifest.toml` の全行が受理される**（`parse` は 1 件でも違反が
    // 在れば `Err` を返すので、ここに届いた時点で全行が必須 key を持つ）。母集団を額面に
    // 出すのは、行が黙って落ちた周を「全部読めた」と読み違えないためである。
    assert_eq!(manifest.rows().len(), 37, "埋め込み manifest の行数（母集団）");
    for kind in ALL {
        let covered = manifest.rows().iter().any(|row| row.kind == *kind);
        assert!(covered, "{} の行が manifest に無い", kind.as_str());
    }
}

#[test]
fn rules_cli_get_returns_value() {
    let args = ["get".to_owned(), "R-C4-1".to_owned()];
    let outcome = vessel::rules::cli::dispatch(&args);
    assert_eq!(outcome.rc, RC_OK, "rc: {outcome:?}");
    assert_eq!(outcome.out, vec!["20000".to_owned()], "値の行");
}

#[test]
fn rules_cli_get_refuses_disabled_row() {
    let args = ["get".to_owned(), "R-C8-1".to_owned()];
    let outcome = vessel::rules::cli::dispatch(&args);
    assert_eq!(outcome.rc, RC_REFUSED, "不発効の行は rc 1");
    assert_eq!(
        outcome.err,
        vec!["rules: disabled R-C8-1".to_owned()],
        "断りの行"
    );
    assert!(outcome.out.is_empty(), "stdout へは書かない");
}

#[test]
fn rules_cli_rules_flag_overrides_embedded() {
    let dir = make_tmp_dir().expect("tmp dir を作れる");
    let path = dir.join("manifest.toml");
    let text = one_row(RuleKind::GateTokenCap, "1");
    std::fs::write(&path, text).expect("tmp manifest を書ける");
    let args = [
        "get".to_owned(),
        "probe".to_owned(),
        "--rules".to_owned(),
        path.display().to_string(),
    ];
    let outcome = vessel::rules::cli::dispatch(&args);
    assert_eq!(outcome.rc, RC_OK, "rc: {outcome:?}");
    assert_eq!(outcome.out, vec!["1".to_owned()], "override した値");
    let embedded = vessel::rules::cli::dispatch(&["get".to_owned(), "gate.token_cap".to_owned()]);
    assert_eq!(
        embedded.out,
        vec!["150000".to_owned()],
        "override は埋め込みを書き換えない"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rules_external_form() {
    let bin = env!("CARGO_BIN_EXE_scribe2");
    let usage = Command::new(bin).output().expect("binary を起動できる");
    let validate = Command::new(bin)
        .args(["rules", "validate"])
        .output()
        .expect("binary を起動できる");
    let missing = Command::new(bin)
        .args(["rules", "get", "nope"])
        .output()
        .expect("binary を起動できる");
    let form = format!(
        "{}{}{}",
        String::from_utf8_lossy(&usage.stdout),
        String::from_utf8_lossy(&validate.stdout),
        String::from_utf8_lossy(&missing.stderr)
    );
    insta::assert_snapshot!(form);
}

/// `ALL` の並びが**宣言順**（判別子 0, 1, 2, …）と一致する（ADR-0013 D2）。
///
/// 並べ替え・重複・**中間**の欠番はここで落ちる。**末尾の足し忘れは落ちない**——判別子が
/// `0..len` に収まるからである。それを捕まえるのは manifest parity 側（未知の kind は
/// `parse` できない）で、限界を歯の隣に置くのは「これで全部守られている」と読み違えさせない
/// ためである。
#[test]
fn rules_all_follows_declaration_order() {
    assert!(
        is_declaration_order(ALL, |kind| kind as usize),
        "ALL の並びが宣言順と乖離している（母集団 {} 種）",
        ALL.len()
    );
}

/// 述語が**真を返すだけ**でないこと（非空虚性）。3 つの壊し方をすべて false で返す。
#[test]
fn declaration_order_rejects_broken_slices() {
    let swapped = [RuleKind::ModuleLines, RuleKind::CoreLines];
    assert!(
        !is_declaration_order(&swapped, |kind| kind as usize),
        "入れ替えた並びは宣言順ではない"
    );
    let gap = [RuleKind::CoreLines, RuleKind::TestSrcRatioPct];
    assert!(
        !is_declaration_order(&gap, |kind| kind as usize),
        "中間を抜いた並びは宣言順ではない"
    );
    let duplicated = [RuleKind::CoreLines, RuleKind::CoreLines];
    assert!(
        !is_declaration_order(&duplicated, |kind| kind as usize),
        "重複した並びは宣言順ではない"
    );
}
