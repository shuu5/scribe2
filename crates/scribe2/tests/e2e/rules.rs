//! rules manifest の歯（設計 docs/design/rules-manifest.md §7）。
//!
//! fixture は文字列 literal で持つ（file を置くと歯が repo の状態に依存する）。

use crate::make_tmp_dir;
use std::process::Command;
use vessel::cli_outcome::{Outcome, RC_OK, RC_REFUSED};
use vessel::fleet::select::Model;
use vessel::order::is_declaration_order;
use vessel::rules::manifest::{contract_rows, Manifest, TableValue};
use vessel::rules::{int_row, str_row, Rule, RuleKind, RuleValue, ValueShape, ALL};
use vessel::seat::role::{Capability, Role, ALL as ROLES, CAPABILITIES};

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
value = "planner"
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

/// 種類の形に合う値の字面。**閉じた名の集合を指す kind** は名を core の enum から取る（対話面は `Role` の名・
/// 権能の行は `Capability` の名・`s2-07l.201`）。
fn sample_value(kind: RuleKind) -> String {
    match kind {
        RuleKind::DialogueSurface => format!("\"{}\"", Role::Planner.as_str()),
        RuleKind::RoleCapabilities => format!("[\"{}\"]", Capability::Answer.as_str()),
        _ => match kind.shape() {
            ValueShape::Int => "1".to_owned(),
            ValueShape::Str | ValueShape::Policy => "\"sample\"".to_owned(),
            ValueShape::List => "[\"sample\"]".to_owned(),
        },
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
    // `bats` は uns（ubuntu-note-system）の vessel 宣言のため上限に足した（user 裁定 2026-09-14・
    // `s2-07l.271`）。`bash` / `sh` は足さない。自 repo の宣言 `.vessel.toml` は `["cargo", "git"]` の
    // まま（上限は宣言より広くてよい・ADR-0010 §2.2）。
    assert_eq!(
        allowed.value,
        RuleValue::List(vec!["cargo".to_owned(), "git".to_owned(), "bats".to_owned()]),
        "user 裁定 2026-09-14 の上限（bash / sh は足さない）"
    );
    assert_eq!(
        allowed.ruling,
        "user 2026-09-14T13:23Z bats in runner ceiling (uns vessel; bash/sh excluded)",
        "裁定: {}",
        allowed.id
    );
    assert_eq!(allowed.ruled_at, "2026-09-14", "裁定日: {}", allowed.id);
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
        vec!["[\"cargo\", \"git\", \"bats\"]".to_owned()],
        "値の行（3 要素・user 裁定 2026-09-14）"
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
        RuleValue::Str("planner".to_owned()),
        "識別子（Role の名）"
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

/// 未知の section は受理しない（受ける section は 4 つちょうど）。
#[test]
fn rules_manifest_rejects_unknown_section() {
    let text = format!("{GOOD}\n[[seat]]\nlabel = \"a1\"\n");
    let errors = rejected(&text).expect("拒まれるはずの fixture が受理された");
    let joined = errors.join("\n");
    assert!(joined.contains("未知の section [[seat]]"), "理由: {joined}");
    for header in ["[[rule]]", "[[account]]", "[[plugin]]", "[[launch-arg]]"] {
        assert!(joined.contains(header), "受理する形を名指す（{header}）: {joined}");
    }
}

// ─────────────────── host の面（`<state_dir>/host.toml`・account-lifecycle.md §2・接頭辞 `rules_host_`） ───────────────────

/// 3 種の表を 2 行ずつ持つ host の面（見出し行: account 3・6 / plugin 9・12 / launch-arg 15・18）。
const HOST_GOOD: &str = r#"schema = 1

[[account]]
label = "h1"

[[account]]
label = "h2"

[[plugin]]
dir = "plugins/one"

[[plugin]]
dir = "plugins/two"

[[launch-arg]]
value = "--permission-mode"

[[launch-arg]]
value = "acceptEdits"
"#;

/// 欠陥 4 件の host の面（schema 欠落 line=0 / 未知 key line=6 / 型違い line=9 / `[[rule]]` の混入 line=11）。
const HOST_DEFECTIVE: &str = r#"[[account]]
label = "shared"

[[plugin]]
dir = "plugins/one"
color = "red"

[[launch-arg]]
value = 3

[[rule]]
id = "R-C4-1"
kind = "CoreLines"
value = 1
enabled = true
ruling = "r"
ruled_at = "d"
"#;

/// tmp の state dir を作り、`host` が在れば `host.toml` として置く。
fn host_state_dir(host: Option<&str>) -> Option<std::path::PathBuf> {
    let dir = make_tmp_dir()?;
    if let Some(text) = host {
        std::fs::write(dir.join(vessel::rules::HOST_MANIFEST), text).ok()?;
    }
    Some(dir)
}

/// 引数の列を `rules` に渡す。
fn rules_dispatch(args: &[&str]) -> Outcome {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    vessel::rules::cli::dispatch(&owned)
}

/// 埋め込みの `rules validate` の 1 行（`--state-dir` の無い周の形・行数と種類数）。
fn embedded_validate_line() -> String {
    rules_dispatch(&["validate"]).out.join("\n")
}

/// (a) host の面の 3 種の表を `rules validate --state-dir` が数え `host=present`・宣言順の値と行番号が
/// `Manifest` の口から読める。base は `--state-dir` を受けず従来の 1 行（RED）。
#[test]
fn rules_host_validate_counts_the_three_tables_and_names_the_host_present() {
    let dir = host_state_dir(Some(HOST_GOOD)).expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let outcome = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(
        outcome.out,
        vec![format!("{} accounts=2 plugins=2 launch-args=2 host=present", embedded_validate_line())],
        "rows / kinds は tracked の面のまま・宣言の数を足す"
    );
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    let accounts: Vec<(&str, u64)> = manifest.accounts().iter().map(|found| (found.label(), found.line())).collect();
    let plugins: Vec<(&str, u64)> = manifest.plugins().iter().map(|found| (found.dir(), found.line())).collect();
    let args: Vec<(&str, u64)> = manifest.launch_args().iter().map(|found| (found.value(), found.line())).collect();
    assert_eq!(accounts, [("h1", 3), ("h2", 6)], "口座は宣言順・行番号は host の面の行");
    assert_eq!(plugins, [("plugins/one", 9), ("plugins/two", 12)], "plugin dir は宣言順");
    assert_eq!(args, [("--permission-mode", 15), ("acceptEdits", 18)], "起動引数は宣言順");
    assert_eq!(manifest.rows(), Manifest::embedded().expect("埋め込み").rows(), "rules 行は tracked の面だけ");
    std::fs::remove_dir_all(&dir).ok();
}

/// (b) host の面が無い周は `host=absent`・0 宣言（縮退・rc 0）。`--state-dir` の無い周は従来の 1 行のまま、
/// PATH の無い `--state-dir` は断る（tracked の面だけで黙って通さない）。
#[test]
fn rules_host_validate_without_the_host_file_is_absent_with_zero_declarations() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let outcome = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(
        outcome.out,
        vec![format!("{} accounts=0 plugins=0 launch-args=0 host=absent", embedded_validate_line())]
    );
    let bare = embedded_validate_line();
    assert!(bare.starts_with("rules: ok rows=") && !bare.contains(" accounts="), "--state-dir 無しは従来の 1 行: {bare}");
    for bad in [&["validate", "--state-dir"][..], &["validate", "--state-dir", ""], &["validate", "--state-dir", "--rules"]] {
        let refused = rules_dispatch(bad);
        assert_eq!(refused.rc, RC_REFUSED, "{bad:?}: {refused:?}");
        assert!(refused.out.is_empty(), "{bad:?}: stdout へは書かない");
        assert_eq!(refused.err, vec!["rules: --state-dir に PATH が無い line=0".to_owned()], "{bad:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// (c) 壊れた host の面は欠陥を**全件**・行番号付き・`host.toml:` の接頭辞で拒む（rc 1・stdout 0 行）: schema 欠落・
/// 未知 key・型違い・`[[rule]]` の混入。面をまたぐ label の重複（`--rules` の tracked 側と host 側に同じ label）も
/// host の面の行番号で拒む。
#[test]
fn rules_host_rejects_every_defect_with_line_numbers_and_the_face_prefix() {
    let dir = host_state_dir(Some(HOST_DEFECTIVE)).expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let outcome = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert!(outcome.out.is_empty(), "stdout へは書かない: {outcome:?}");
    let want = [
        ("schema = 1 が無い", 0),
        ("未知の key color", 6),
        ("value は文字列でなければならない", 9),
        ("[[rule]] は host の面に置けない", 11),
    ];
    assert_eq!(outcome.err.len(), want.len(), "全件・同じ欠陥を 2 行にしない: {:?}", outcome.err);
    for ((reason, line), got) in want.iter().zip(&outcome.err) {
        assert!(got.starts_with("rules: host.toml: "), "面を名指す接頭辞: {got}");
        assert!(got.contains(reason) && got.ends_with(&format!(" line={line}")), "{reason} line={line}: {got}");
    }

    let tracked = dir.join("tracked.toml");
    std::fs::write(&tracked, format!("{GOOD}\n[[account]]\nlabel = \"shared\"\n")).expect("tracked の fixture を書ける");
    std::fs::write(dir.join(vessel::rules::HOST_MANIFEST), "schema = 1\n\n[[account]]\nlabel = \"solo\"\n\n[[account]]\nlabel = \"shared\"\n")
        .expect("host の面を書ける");
    let rules = tracked.display().to_string();
    let crossed = rules_dispatch(&["validate", "--rules", &rules, "--state-dir", &state]);
    assert_eq!(crossed.rc, RC_REFUSED, "{crossed:?}");
    assert_eq!(
        crossed.err,
        vec!["rules: host.toml: label shared が面をまたいで重複する（tracked の manifest にも在る） line=6".to_owned()],
        "面をまたぐ重複は host の面の行で 1 件"
    );
    let alone = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(alone.rc, RC_OK, "埋め込みの面に口座は無い＝同じ host の面が単独では通る: {alone:?}");
    std::fs::remove_dir_all(&dir).ok();
}

// ─────────────────── host の面が dir（在るが読めない・`s2-07l.250`・`.243` run 3 の生存変異を塞ぐ） ───────────────────
// flip-check: retroactive s2-07l.250

/// `S/host.toml` の位置に dir を置いた state dir（権限に依らず読めない＝「無い」に潰れたら縮退の側へ倒れる）。
fn host_dir_state() -> Option<std::path::PathBuf> {
    let dir = make_tmp_dir()?;
    std::fs::create_dir_all(dir.join(vessel::rules::HOST_MANIFEST)).ok()?;
    Some(dir)
}

/// (f) dir の host の面で `rules validate --state-dir S` は拒否・stderr は `host.toml:` の 1 行（`absent` に潰れない）。
#[test]
fn rules_host_validate_refuses_a_directory_host_manifest() {
    let dir = host_dir_state().expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let outcome = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert!(outcome.out.is_empty(), "host=absent の行を出さない: {outcome:?}");
    assert_eq!(outcome.err.len(), 1, "1 行: {:?}", outcome.err);
    let first = outcome.err.first().map(String::as_str).unwrap_or_default();
    assert!(first.starts_with("rules: host.toml: "), "面の接頭辞: {first}");
    assert!(first.contains(&dir.join(vessel::rules::HOST_MANIFEST).display().to_string()), "path を名指す: {first}");
    assert!(first.ends_with(" line=0"), "行番号: {first}");
    std::fs::remove_dir_all(&dir).ok();
}

/// (g) 同じ fixture で `fleet usage` と `seat tick` も typed に止まる（rc 1・stdout 0 byte・event も tick の記録も書かない）。
#[test]
fn rules_host_directory_host_manifest_stops_fleet_usage_and_seat_tick_without_events() {
    let dir = host_dir_state().expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let host = dir.join(vessel::rules::HOST_MANIFEST).display().to_string();
    let bin = env!("CARGO_BIN_EXE_scribe2");
    let wm = dir.join("wm");
    std::fs::create_dir_all(&wm).expect("wm dir を作れる");
    let pane = dir.join("pane.txt");
    std::fs::write(&pane, "❯ \n  10% 100k/1M Opus 5\n").expect("pane を書ける");
    let socket = dir.join("sock").display().to_string();
    let tick = Command::new(bin)
        .args(["seat", "tick", "--target", "hostdir", "--wm-dir", &wm.display().to_string(), "--state-dir", &state])
        .args(["--tmux-socket", &socket, "--capture-file", &pane.display().to_string()])
        .output()
        .expect("binary を起動できる");
    assert_eq!(tick.status.code(), Some(i32::from(RC_REFUSED)), "{tick:?}");
    assert!(tick.stdout.is_empty(), "stdout は 0 byte: {tick:?}");
    assert_eq!(String::from_utf8_lossy(&tick.stderr), "seat: tick decision=error reason=no-rule:manifest-unreadable\n", "1 行");
    assert!(!dir.join("seat").join("hostdir").join("tick.jsonl").exists(), "tick の記録を書かない");

    let curl = dir.join("no-curl").display().to_string();
    let usage = Command::new(bin)
        .args(["fleet", "usage", "--state-dir", &state, "--curl", &curl])
        .output()
        .expect("binary を起動できる");
    assert_eq!(usage.status.code(), Some(i32::from(RC_REFUSED)), "{usage:?}");
    assert!(usage.stdout.is_empty(), "stdout は 0 byte: {usage:?}");
    let said = String::from_utf8_lossy(&usage.stderr);
    assert!(said.starts_with("fleet usage: manifest を読めない（rules: host.toml: "), "typed の 1 行: {said}");
    assert!(said.contains(&host) && said.ends_with(" line=0）\n") && said.lines().count() == 1, "path と行番号: {said}");
    assert!(!vessel::fleet::store::events_path(&dir).exists(), "event を書かない");
    std::fs::remove_dir_all(&dir).ok();
}

/// (h) `rules` を引数なし・`rules get` を id なしで撃つと usage 1 行で拒否（行は `--rules PATH` と `--state-dir S` を名指す）。
#[test]
fn rules_host_usage_line_names_both_rules_and_state_dir_flags() {
    let want = "usage: rules <validate|get <id>> [--rules PATH] [--state-dir S]";
    assert!(want.contains("--rules PATH") && want.contains("--state-dir S"), "期待値の自己検査");
    for args in [&[][..], &["get"]] {
        let outcome = rules_dispatch(args);
        assert_eq!(outcome.rc, RC_REFUSED, "{args:?}: {outcome:?}");
        assert!(outcome.out.is_empty(), "{args:?}: stdout へは書かない");
        assert_eq!(outcome.err, vec![want.to_owned()], "{args:?}: usage 1 行");
    }
}

/// (g) tracked の manifest は口座の表を持たない（宣言は host の面にだけ・公開面の情報が減る側）・待ち時間の行は在る。
#[test]
fn rules_host_embedded_manifest_declares_no_account_and_keeps_usage_timeout() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    assert_eq!(manifest.accounts().len(), 0, "tracked の面の口座: {:?}", manifest.accounts());
    assert!(manifest.plugins().is_empty() && manifest.launch_args().is_empty(), "host 固有の値も持たない");
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

/// 退避物の節 3 の上限の行（`seat.wm_directive_cap`・裁定 id `user 2026-09-12T02:01Z`・設計
/// working-memory.md §5.1）。**値は manifest が持ち、設計 doc は写さない**（C1 / C5）。
#[test]
fn rules_embedded_manifest_declares_wm_directive_cap() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let row = manifest.get("seat.wm_directive_cap").expect("上限の行が在る");
    assert_eq!(row.value, RuleValue::Int(24), "user 裁定 2026-09-12T02:01Z の値");
    assert_eq!(row.kind, RuleKind::WmDirectiveCap, "kind");
    assert_eq!(row.kind.shape(), ValueShape::Int, "値の形は Int（行）");
    assert!(row.enabled, "既定で効く");
    assert_eq!(row.ruling, "user 2026-09-12T02:01Z", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-12", "裁定日");
    assert!(ALL.contains(&RuleKind::WmDirectiveCap), "ALL に在る");
    assert_eq!(RuleKind::parse("WmDirectiveCap"), Some(RuleKind::WmDirectiveCap), "kind を字面から引ける");
}

/// 着地の順番を待つ上限の行（`pipe.land_wait_s`・裁定 id `user 2026-09-13T02:50Z`・設計
/// gate-cost.md §6）。**値は manifest が持ち、設計 doc は写さない**（C1 / C5）。
/// 行の kind を綴り違えた manifest は `RuleError` で拒まれる（kind の字面は `ALL` を通してしか解けない）。
#[test]
fn rules_embedded_manifest_declares_land_wait() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let row = manifest.get("pipe.land_wait_s").expect("着地の順番を待つ上限の行が在る");
    assert_eq!(row.value, RuleValue::Int(5400), "user 裁定 2026-09-13T02:50Z の値");
    assert_eq!(row.kind, RuleKind::PipeLandWaitS, "kind");
    assert_eq!(row.kind.shape(), ValueShape::Int, "値の形は Int（秒）");
    assert!(row.enabled, "既定で効く");
    assert_eq!(row.ruling, "user 2026-09-13T02:50Z", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-13", "裁定日");
    assert!(ALL.contains(&RuleKind::PipeLandWaitS), "ALL に在る");
    assert_eq!(RuleKind::parse("PipeLandWaitS"), Some(RuleKind::PipeLandWaitS), "kind を字面から引ける");
    let errors = rejected(&one_row_raw("PipeLandWait", "5400")).expect("未知の kind の fixture が受理された");
    let joined = errors.join("\n");
    assert!(joined.contains("未知である"), "行の kind を綴り違えた manifest は読めない: {joined}");
}

/// rebrief が台帳を待つ上限の行（`seat.ledger_timeout_s`・裁定 id `user 2026-09-12T02:01Z`・設計
/// working-memory.md §5.2）。**値は manifest が持ち、設計 doc は写さない**（C1 / C5）。
#[test]
fn rules_embedded_manifest_declares_ledger_timeout() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let row = manifest.get("seat.ledger_timeout_s").expect("台帳の待ち上限の行が在る");
    assert_eq!(row.value, RuleValue::Int(60), "user 裁定 2026-09-12T02:01Z の値");
    assert_eq!(row.kind, RuleKind::LedgerTimeoutS, "kind");
    assert_eq!(row.kind.shape(), ValueShape::Int, "値の形は Int（秒）");
    assert!(row.enabled, "既定で効く");
    assert_eq!(row.ruling, "user 2026-09-12T02:01Z", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-12", "裁定日");
    assert!(ALL.contains(&RuleKind::LedgerTimeoutS), "ALL に在る（末尾は `.201` の RoleCapabilities）");
    assert_eq!(RuleKind::parse("LedgerTimeoutS"), Some(RuleKind::LedgerTimeoutS), "kind を字面から引ける");
    let errors = rejected(&one_row_raw("LedgerTimeout", "60")).expect("未知の kind の fixture が受理された");
    assert!(errors.join("\n").contains("未知である"), "行の kind を綴り違えた manifest は読めない");
}

/// 契約の size ↔ 1 file あたりの増分の見積の 3 行（`pipe.size_<s|m|l>_lines`・裁定 id `user 2026-09-14T06:4xZ`・設計
/// contract-source.md §3「上限の余地」・rules-manifest.md §4・`s2-07l.249`）。**値は manifest が持ち、設計 doc は
/// 写さない**（C1 / C5）。kind は宣言順の末尾 3 つで、行と variant を対で足させる（片方だけの manifest は `parse`
/// できず、片方だけの enum は親 test の `covers_all_kinds` が落ちる）。
#[test]
fn rules_embedded_manifest_declares_the_size_lines_rows() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let rows: [(&str, RuleKind, u64); 3] = [
        ("pipe.size_s_lines", RuleKind::PipeSizeSLines, 100),
        ("pipe.size_m_lines", RuleKind::PipeSizeMLines, 300),
        ("pipe.size_l_lines", RuleKind::PipeSizeLLines, 800),
    ];
    for (id, kind, value) in rows {
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!(row.value, RuleValue::Int(value), "{id} の値");
        assert_eq!(row.kind, kind, "{id} の kind");
        assert_eq!(row.kind.shape(), ValueShape::Int, "{id} の値の形は Int（行）");
        assert!(row.enabled, "{id} は既定で効く");
        assert_eq!(row.ruling, "user 2026-09-14T06:4xZ", "{id} の裁定 id");
        assert_eq!(row.ruled_at, "2026-09-14", "{id} の裁定日");
        assert_eq!(RuleKind::parse(kind.as_str()), Some(kind), "{id} の kind を字面から引ける");
    }
    // `.297` で `RunnerModel`・`.315` で `SeatSignalBackoffS` が末尾に足されたので、size の 3 つはその直前に並ぶ。
    let tail: Vec<RuleKind> = ALL.iter().rev().skip(2).take(3).rev().copied().collect();
    assert_eq!(tail, [RuleKind::PipeSizeSLines, RuleKind::PipeSizeMLines, RuleKind::PipeSizeLLines], "宣言順の末尾から 3 つ目までの 3 つ");
    let errors = rejected(&one_row_raw("PipeSizeXlLines", "1600")).expect("未知の kind の fixture が受理された");
    assert!(errors.join("\n").contains("未知である"), "4 段目の size は kind として読めない");
}

/// 口座選定の行（`R-C9-1`・裁定 id `user 2026-09-13T03:14Z`・設計 account-autonomy.md §3）。値は
/// session 用の閾値（使用率の百分率）で形は `Int`。**散文（Policy）の値を置いた行は形の不一致で拒まれる**
/// （形は `RuleKind::shape` の 1 箇所が持つ）。
#[test]
fn rules_embedded_manifest_declares_account_selection_threshold() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let row = manifest.get("R-C9-1").expect("口座選定の行が在る");
    assert_eq!(row.value, RuleValue::Int(85), "user 裁定 2026-09-13T03:14Z の値");
    assert_eq!(row.kind, RuleKind::AccountSelection, "kind は既存のまま");
    assert_eq!(row.kind.shape(), ValueShape::Int, "値の形は Int（百分率）");
    assert!(row.enabled, "発効している");
    assert_eq!(row.ruling, "user 2026-09-13T03:14Z", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-13", "裁定日");
    let errors = rejected(&one_row(RuleKind::AccountSelection, "\"新規投入は 5h 線\""))
        .expect("散文の値の fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("形と合わない"), "理由: {first}");
    assert!(first.contains("要 Int"), "要求する形を名指す: {first}");
    let healed = parsed(&one_row(RuleKind::AccountSelection, "85")).expect("Int の値は受理される");
    assert_eq!(healed.get("probe").map(|found| found.value.clone()), Some(RuleValue::Int(85)));
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
    assert_eq!(manifest.rows().len(), 51, "埋め込み manifest の行数（母集団・`.217` で +2・`.249` で +3・`.254` で +1・`.168` で +1・`.297` で +1・`.315` で +1）");
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
    assert_eq!(outcome.out, vec!["40000".to_owned()], "値の行（裁定 id user 2026-09-14）");
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
        vec!["1000000".to_owned()],
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
    // 口座選定の行は発効した Int（不発効の周は stderr の断りになり stdout は空）。
    let selection = Command::new(bin)
        .args(["rules", "get", "R-C9-1"])
        .output()
        .expect("binary を起動できる");
    // host の面を持つ state dir での validate（3 種の表 2 行ずつ・host=present の形）。
    let dir = host_state_dir(Some(HOST_GOOD)).expect("tmp の state dir を作れる");
    let hosted = Command::new(bin)
        .args(["rules", "validate", "--state-dir", &dir.display().to_string()])
        .output()
        .expect("binary を起動できる");
    std::fs::remove_dir_all(&dir).ok();
    // `rules` を引数なしで撃った usage の行（stderr・`s2-07l.250`）。
    let bare = Command::new(bin).arg("rules").output().expect("binary を起動できる");
    let form = format!(
        "{}{}{}{}{}{}{}",
        String::from_utf8_lossy(&usage.stdout),
        String::from_utf8_lossy(&validate.stdout),
        String::from_utf8_lossy(&missing.stderr),
        String::from_utf8_lossy(&selection.stdout),
        String::from_utf8_lossy(&selection.stderr),
        String::from_utf8_lossy(&hosted.stdout),
        String::from_utf8_lossy(&bare.stderr)
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

/// 役割ごとの権能の行（`role.<役割名>`・`RuleKind::RoleCapabilities`・裁定 id `user 2026-09-13T12:04Z`・設計
/// seat-roles.md §3・ADR-0022 §2.2・`s2-07l.201`）: **行は `Role::ALL` と同数**（役割ごとに 1 行・1 kind で 2 行）、
/// 値は `Capability` の名の列、宣言順の末尾の kind。**値は manifest が持ち、設計 doc は写さない**（C1 / C5）。
#[test]
fn rules_embedded_manifest_declares_one_capability_row_per_role() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let rows: Vec<_> = manifest.rows().iter().filter(|row| row.kind == RuleKind::RoleCapabilities).collect();
    assert_eq!(rows.len(), ROLES.len(), "RoleCapabilities の行は Role::ALL と同数（母集団 {}）", ROLES.len());
    for role in ROLES {
        let id = format!("role.{}", role.as_str());
        let row = manifest.get(&id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!(row.kind, RuleKind::RoleCapabilities, "{id} の kind");
        assert_eq!(row.kind.shape(), ValueShape::List, "{id} の値の形は List（名の列）");
        assert!(row.enabled, "{id} は発効している");
        assert_eq!(row.ruling, "user 2026-09-13T12:04Z", "{id} の裁定 id（edit-outside の追加）");
        assert_eq!(row.ruled_at, "2026-09-13", "{id} の裁定日");
        let names = role_row_names(&manifest, *role);
        assert!(!names.is_empty(), "{id} の値は非空の列");
        for name in &names {
            assert!(Capability::parse(name).is_some(), "{id} の値 {name} は Capability の名");
        }
    }
    assert!(ALL.contains(&RuleKind::RoleCapabilities), "ALL に在る（末尾は `.315` の SeatSignalBackoffS）");
    assert_eq!(RuleKind::parse("RoleCapabilities"), Some(RuleKind::RoleCapabilities), "kind を字面から引ける");
    let kinds = ALL.len();
    assert_eq!(kinds, 50, "kind の母集団（`.201` で +1・`.217` で +2・`.249` で +3・`.254` で +1・`.168` で +1・`.297` で +1・`.315` で +1）");
}

/// 禁じる語列の行（`runner.denied_commands`・`RuleKind::RunnerDeniedCommands`・裁定 id `user 2026-09-14`・ADR-0025 §2.1・
/// 設計 rules-manifest.md / vessel-hook.md §5・`s2-07l.168`）: **値は manifest が持つ**（C1 / C5）＝初期値 9 語列を名指す。
/// kind は宣言順で `RunnerAllowedCommands` の直後（対で読む行）・値の形は List・各要素は語を 1 つ以上持つ（空白だけの
/// 語列は validate が断る）。
#[test]
fn rules_embedded_manifest_declares_the_denied_commands_row() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get("runner.denied_commands").expect("禁じる語列の行が在る");
    let want: Vec<String> = [
        "cargo mutants",
        "cargo publish",
        "git push --force",
        "git push -f",
        "git reset --hard",
        "git branch -D",
        "git clean -f",
        "git stash drop",
        "git stash clear",
    ]
    .iter()
    .map(|item| (*item).to_owned())
    .collect();
    assert_eq!(row.value, RuleValue::List(want), "ADR-0025 §2.1 の初期値 9 語列（user 裁定 2026-09-14）");
    assert_eq!(row.kind, RuleKind::RunnerDeniedCommands, "kind");
    assert_eq!(row.kind.shape(), ValueShape::List, "値の形は List（語列の配列）");
    assert!(row.enabled, "既定で効く");
    assert_eq!(row.ruling, "user 2026-09-14", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-14", "裁定日");
    assert_eq!(RuleKind::parse("RunnerDeniedCommands"), Some(RuleKind::RunnerDeniedCommands), "kind を字面から引ける");
    let at = ALL.iter().position(|kind| *kind == RuleKind::RunnerDeniedCommands);
    let allowed = ALL.iter().position(|kind| *kind == RuleKind::RunnerAllowedCommands);
    assert_eq!(at, allowed.map(|found| found + 1), "宣言順は RunnerAllowedCommands の直後（対で読む）");
    // 空白だけの語列は validate が断る（何にも当たらず黙って効かない要素を持たせない）。
    let blank = one_row(RuleKind::RunnerDeniedCommands, r#"["cargo mutants", " "]"#);
    let errors = parsed(&blank).expect_err("語を持たない語列は不備");
    assert!(errors.contains("語を持たない語列"), "{errors}");
    assert!(parsed(&one_row(RuleKind::RunnerDeniedCommands, r#"["cargo mutants"]"#)).is_ok(), "語を持つ語列は通る");
}

/// 行の数え方の幅の行（`R-C4.line-width`・裁定 id `user 2026-09-14T06:5xZ`・設計 rules-manifest.md §4・`s2-07l.254`）。
/// **値は manifest が持つ**（C1 / C5）。kind は宣言順で `FnArgs` の直後（R-C4 の行の並び）。
#[test]
fn rules_embedded_manifest_declares_the_line_width_row() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get("R-C4.line-width").expect("行の数え方の幅の行が在る");
    assert_eq!(row.value, RuleValue::Int(120), "user 裁定 2026-09-14T06:5xZ の値");
    assert_eq!(row.kind, RuleKind::LineWidth, "kind");
    assert_eq!(row.kind.shape(), ValueShape::Int, "値の形は Int（文字）");
    assert!(row.enabled, "既定で効く");
    assert_eq!(row.ruling, "user 2026-09-14T06:5xZ", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-14", "裁定日");
    assert_eq!(RuleKind::parse("LineWidth"), Some(RuleKind::LineWidth), "kind を字面から引ける");
    let at = ALL.iter().position(|kind| *kind == RuleKind::LineWidth);
    let args = ALL.iter().position(|kind| *kind == RuleKind::FnArgs);
    assert_eq!(at, args.map(|found| found + 1), "宣言順は FnArgs の直後");
}

/// 台帳の棚卸しの閾値 2 行（`ledger.memo_stale_days` / `ledger.memo_stale_priority`・裁定 id
/// `user 2026-09-13T14:06Z`・設計 ledger-triage.md §4）。**値は manifest が持ち、設計 doc は写さない**（C1 / C5）。
#[test]
fn rules_embedded_manifest_declares_the_memo_stale_rows() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let rows: [(&str, RuleKind, u64); 2] = [
        ("ledger.memo_stale_days", RuleKind::MemoStaleDays, 3),
        ("ledger.memo_stale_priority", RuleKind::MemoStalePriority, 2),
    ];
    for (id, kind, value) in rows {
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!(row.value, RuleValue::Int(value), "{id} の値（user 裁定 2026-09-13T14:06Z）");
        assert_eq!(row.kind, kind, "{id} の kind");
        assert_eq!(row.kind.shape(), ValueShape::Int, "{id} の値の形は Int");
        assert!(row.enabled, "{id} は既定で効く");
        assert_eq!(row.ruling, "user 2026-09-13T14:06Z", "{id} の裁定 id");
        assert_eq!(row.ruled_at, "2026-09-13", "{id} の裁定日");
        assert_eq!(RuleKind::parse(kind.as_str()), Some(kind), "{id} の kind を字面から引ける");
    }
    // `.249` の size の 3 行・`.297` の RunnerModel・`.315` の SeatSignalBackoffS が末尾に続く＝この 2 行は末尾から
    // 7 つ目と 6 つ目。
    assert_eq!(
        ALL.get(ALL.len().saturating_sub(7)..ALL.len().saturating_sub(5)),
        Some([RuleKind::MemoStaleDays, RuleKind::MemoStalePriority].as_slice()),
        "宣言順で size の 3 行の直前に並ぶ 2 つ"
    );
    let errors = rejected(&one_row(RuleKind::MemoStaleDays, "\"3 日\"")).expect("散文の値の fixture が受理された");
    assert!(errors.join("\n").contains("要 Int"), "Int でない値は拒む: {errors:?}");
}

/// 埋め込み manifest の `role.<役割名>` の行の値（名の列・行が無い・列でない周は空＝呼び側の assert が落とす）。
fn role_row_names(manifest: &Manifest, role: Role) -> Vec<String> {
    match manifest.get(&format!("role.{}", role.as_str())).map(|row| row.value.clone()) {
        Some(RuleValue::List(names)) => names,
        _ => Vec::new(),
    }
}

/// 裁定 `user 2026-09-13T03:14Z` の値: planner だけが記帳（回答・承認・go）と design-intent / 設計 doc の編集を
/// 持ち、管理席だけが起動と merge を持つ。**code は両役割とも持たない**（印で開いた便の write-set だけ・AC16）。
/// 裁定 `user 2026-09-13T12:04Z`: 対象 repo の外（state dir・auto-memory・scratchpad）の編集は両役割とも持つ。
#[test]
fn rules_embedded_manifest_role_rows_carry_the_ruled_capabilities() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let held = |role: Role, cap: Capability| role_row_names(&manifest, role).iter().any(|name| name == cap.as_str());
    for cap in [Capability::Answer, Capability::Approve, Capability::Go, Capability::EditDesignIntent, Capability::EditDesignDoc] {
        assert!(held(Role::Planner, cap), "planner は {} を持つ", cap.as_str());
        assert!(!held(Role::Admin, cap), "管理席は {} を持たない", cap.as_str());
    }
    for cap in [Capability::Launch, Capability::Merge] {
        assert!(held(Role::Admin, cap), "管理席は {} を持つ", cap.as_str());
        assert!(!held(Role::Planner, cap), "planner は {} を持たない", cap.as_str());
    }
    for role in ROLES {
        assert!(held(*role, Capability::Relay), "{} は中継を持つ", role.as_str());
        assert!(!held(*role, Capability::EditCode), "{}: code は持たない（印で開いた便だけ）", role.as_str());
        assert!(held(*role, Capability::EditOutside), "{}: repo の外は持つ（12:04Z）", role.as_str());
    }
}

/// 権能の行の**列に無い名は `RuleError`**（`Capability::parse` の失敗・既存の型）: 綴り違いを黙って
/// 「権能なし」に倒さない。取る名を全部並べた列は受理される。
#[test]
fn rules_role_capabilities_reject_unknown_capability_names() {
    let errors = rejected(&one_row(RuleKind::RoleCapabilities, r#"["answer", "fly"]"#))
        .expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("未知の権能 fly"), "理由: {first}");
    assert!(first.contains("answer") && first.contains("edit-code"), "取る名を名指す: {first}");
    assert!(first.contains("line=3"), "行番号: {first}");
    let all: Vec<String> = CAPABILITIES.iter().map(|cap| format!("\"{}\"", cap.as_str())).collect();
    let healed = parsed(&one_row(RuleKind::RoleCapabilities, &format!("[{}]", all.join(", "))))
        .expect("全権能の列は受理される");
    assert_eq!(
        healed.get("probe").map(|row| row.value.clone()),
        Some(RuleValue::List(CAPABILITIES.iter().map(|cap| cap.as_str().to_owned()).collect())),
        "書いた順のまま"
    );
    // variant 名の字面（`Answer`）は名ではない。
    let errors = rejected(&one_row(RuleKind::RoleCapabilities, r#"["Answer"]"#)).expect("variant 名は受理されない");
    assert!(errors.join("\n").contains("未知の権能 Answer"), "{errors:?}");
}

/// R-C7-1（対話面）の値は **`Role` の名**（`planner`・裁定 id `user 2026-09-13T03:14Z`・ADR-0022 §2.2）:
/// 旧値 `user-direct` や `Role` の名でない fixture は `RuleError` で拒まれる。kind は既存の `DialogueSurface` のまま。
#[test]
fn rules_dialogue_surface_value_must_be_a_role_name() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let row = manifest.get("R-C7-1").expect("対話面の行が在る");
    assert_eq!(row.kind, RuleKind::DialogueSurface, "kind は既存のまま");
    assert_eq!(row.value, RuleValue::Str(Role::Planner.as_str().to_owned()), "値は planner の席");
    assert!(row.enabled, "発効している");
    assert_eq!(row.ruling, "user 2026-09-13T03:14Z", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-13", "裁定日");
    for bad in ["\"user-direct\"", "\"Planner\"", "\"\""] {
        let errors = rejected(&one_row(RuleKind::DialogueSurface, bad)).expect("Role の名でない値は受理されない");
        assert_eq!(errors.len(), 1, "件数（{bad}）: {errors:?}");
        let first = errors.first().map(String::as_str).unwrap_or_default();
        assert!(first.contains("未知の役割"), "{bad}: 理由: {first}");
        assert!(first.contains("planner") && first.contains("admin"), "{bad}: 取る名を名指す: {first}");
    }
    for role in ROLES {
        let healed = parsed(&one_row(RuleKind::DialogueSurface, &format!("\"{}\"", role.as_str())))
            .expect("Role の名は受理される");
        assert_eq!(healed.get("probe").map(|row| row.value.clone()), Some(RuleValue::Str(role.as_str().to_owned())));
    }
}

/// (f) `runner.model`（runner / lens が claude に毎回渡す model・裁定 id `user 2026-09-14T21:59Z`・設計 pipeline.md §6・
/// `s2-07l.297`）: 埋め込み manifest の行は発効 ∧ `Str("opus")`（claude CLI の別名・閉じた表 `Model` で引ける）・kind は
/// 宣言順の末尾から 2 つ目 `RunnerModel`（形は `Str`・末尾は `.315` の `SeatSignalBackoffS`）・`str_row` が同じ値を返し、
/// 不発効 / 整数の行は 3 理由で `Err`。base は行も kind も無いので RED。
#[test]
fn rules_manifest_carries_runner_model() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let row = manifest.get("runner.model").expect("runner.model の行が在る");
    assert_eq!(row.kind, RuleKind::RunnerModel, "kind");
    assert_eq!(row.value, RuleValue::Str("opus".to_owned()), "値は claude CLI の別名（実測の既定 = Opus）");
    assert!(row.enabled, "発効している");
    assert!(row.ruling.starts_with("user 2026-09-14T21:59Z"), "裁定 id: {}", row.ruling);
    assert_eq!(row.ruled_at, "2026-09-14", "裁定日");
    assert_eq!(Model::parse("opus"), Some(Model::Opus), "値は閉じた表で引ける");
    assert_eq!(RuleKind::RunnerModel.shape(), ValueShape::Str, "形は識別子");
    assert_eq!(ALL.get(ALL.len().saturating_sub(2)), Some(&RuleKind::RunnerModel), "宣言順の末尾から 2 つ目");
    assert_eq!(RuleKind::parse("RunnerModel"), Some(RuleKind::RunnerModel));
    let healed = parsed(&one_row(RuleKind::RunnerModel, "\"sonnet\"")).expect("文字列の値は受理される");
    assert_eq!(healed.get("probe").map(|row| row.value.clone()), Some(RuleValue::Str("sonnet".to_owned())));
    let errors = rejected(&one_row(RuleKind::RunnerModel, "5")).expect("整数の値は形が合わない");
    assert!(errors.join("\n").contains("形と合わない"), "{errors:?}");
}

/// (e) `seat.signal_backoff_s`（退避の合図の再送の back-off・裁定 id `user 2026-09-15T02:30Z`・設計 seat-autonomy.md
/// §3 / §8・`s2-07l.315`）: 埋め込み manifest の行は発効 ∧ `Int(300)`・kind は宣言順の末尾 `SeatSignalBackoffS`（形は
/// `Int`・`SeatTickStaleS` と同じ形）・kind の母集団は 50（`.297` の 49 に +1）・`int_row` が同じ値を返し、散文の値の行は
/// 形の不一致で拒まれる。**値は manifest が持ち、設計 doc は写さない**（C1 / C5）。base は行も kind も無いので RED。
#[test]
fn rules_manifest_carries_seat_signal_backoff() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let row = manifest.get("seat.signal_backoff_s").expect("seat.signal_backoff_s の行が在る");
    assert_eq!(row.kind, RuleKind::SeatSignalBackoffS, "kind");
    assert_eq!(row.value, RuleValue::Int(300), "user 裁定 2026-09-15T02:30Z の値（秒）");
    assert!(row.enabled, "発効している");
    assert!(row.ruling.starts_with("user 2026-09-15T02:30Z"), "裁定 id: {}", row.ruling);
    assert_eq!(row.ruled_at, "2026-09-15", "裁定日");
    assert_eq!(RuleKind::SeatSignalBackoffS.shape(), ValueShape::Int, "形は秒の整数");
    assert_eq!(RuleKind::SeatTickStaleS.shape(), RuleKind::SeatSignalBackoffS.shape(), "打刻の合図の brake の行と同じ形");
    assert_eq!(ALL.last(), Some(&RuleKind::SeatSignalBackoffS), "宣言順の末尾");
    assert_eq!(ALL.len(), 50, "kind の母集団（`.297` の 49 に +1）");
    assert_eq!(RuleKind::parse("SeatSignalBackoffS"), Some(RuleKind::SeatSignalBackoffS), "kind を字面から引ける");
    assert_eq!(int_row(&manifest, "seat.signal_backoff_s"), Ok(300), "整数の行の読み手が同じ値を返す");
    let healed = parsed(&one_row(RuleKind::SeatSignalBackoffS, "60")).expect("整数の値は受理される");
    assert_eq!(healed.get("probe").map(|row| row.value.clone()), Some(RuleValue::Int(60)));
    let errors = rejected(&one_row(RuleKind::SeatSignalBackoffS, "\"5 分\"")).expect("散文の値は形が合わない");
    assert!(errors.join("\n").contains("要 Int"), "{errors:?}");
}

/// 行の読み手 `str_row` / `int_row`（`rules::` の 1 本・headless と `pipe::ratelimit` が読む・`s2-07l.297`）: 発効した
/// 文字列 / 整数の行の値を返し、無い / 不発効 / 形違いは 3 理由の `Err`（lens の cap の字面と同じ）。
#[test]
fn rules_row_readers_return_the_value_or_one_of_three_reasons() {
    let manifest = Manifest::embedded().expect("埋め込み manifest を読める");
    assert_eq!(str_row(&manifest, "runner.model"), Ok("opus"), "文字列の行の読み手");
    assert_eq!(int_row(&manifest, "gate.token_cap"), Ok(1_000_000), "整数の行の読み手");
    assert_eq!(str_row(&manifest, "gate.token_cap"), Err("gate.token_cap が文字列でない".to_owned()), "整数の行");
    assert_eq!(int_row(&manifest, "runner.model"), Err("runner.model が整数でない".to_owned()), "文字列の行");
    assert_eq!(str_row(&manifest, "nope"), Err("nope が無い".to_owned()), "無い行");
    assert_eq!(int_row(&manifest, "nope"), Err("nope が無い".to_owned()), "無い行");
    let disabled = parsed(
        "schema = 1\n\n[[rule]]\nid = \"runner.model\"\nkind = \"RunnerModel\"\nvalue = \"opus\"\nenabled = false\nruling = \"r\"\nruled_at = \"d\"\n\n[[rule]]\nid = \"gate.token_cap\"\nkind = \"GateTokenCap\"\nvalue = 7\nenabled = false\nruling = \"r\"\nruled_at = \"d\"\n",
    )
    .expect("不発効の行は読める");
    assert_eq!(str_row(&disabled, "runner.model"), Err("runner.model は不発効である".to_owned()), "不発効");
    assert_eq!(int_row(&disabled, "gate.token_cap"), Err("gate.token_cap は不発効である".to_owned()), "不発効");
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

// ─────────────────── 契約表の `[[contract]]`（設計 contract-source.md §2・ADR-0023 §2.1・`s2-07l.208`） ───────────────────

/// 契約表の最小形（見出し行: 3 / 13・`depends` は 23 行目）。
const CONTRACT_TABLE: &str = r#"schema = 1

[[contract]]
id = "a"
title = "t"
req = ["FR1"]
section = "2"
write-set = ["src/lib.rs"]
verify = ["cargo test"]
size = "S"
done = "d"

[[contract]]
id = "b"
title = "t"
req = ["FR1", "FR2"]
section = "3"
touches = ["crate::x::Y"]
write-set = ["src/"]
verify = ["cargo test"]
size = "M"
done = "d"
depends = ["a"]
"#;

/// `[[contract]]` は rules manifest と**同じ reader** で読める（行番号・値の形はそのまま・任意の列は key の省略）。
#[test]
fn rules_contract_table_rows_read_through_the_same_reader() {
    let rows = contract_rows(CONTRACT_TABLE).expect("受理される");
    assert_eq!(rows.iter().map(|row| row.line()).collect::<Vec<u64>>(), vec![3, 13], "見出しの行");
    assert_eq!(rows[1].value("depends"), Some((&TableValue::List(vec!["a".to_owned()]), 23)), "配列の値と行");
    assert_eq!(rows[0].value("depends"), None, "書かない任意の列は無い（空の配列ではない）");
    assert_eq!(rows[0].value("touches"), None, "touches も同じ");
}

/// 空の配列の拒否は**緩めない**（空の列は key の省略で表す）。未知 key・必須 key の欠落・schema の欠落は全件・
/// 行番号付き（rules manifest と同じ拒否形）。
#[test]
fn rules_contract_table_keeps_the_empty_array_refusal_and_names_every_defect() {
    let empty = CONTRACT_TABLE.replace("depends = [\"a\"]", "depends = []");
    let errors = contract_rows(&empty).expect_err("空の配列は拒む");
    assert!(errors.iter().any(|error| error.line == 23 && error.to_string().contains("配列が空である")), "{errors:?}");
    let defects = format!("{}color = \"red\"\n", CONTRACT_TABLE.replacen("title = \"t\"\n", "", 1));
    let errors = contract_rows(&defects).expect_err("欠陥は拒む");
    let shown: Vec<(u64, String)> = errors.iter().map(|error| (error.line, error.message.clone())).collect();
    assert!(shown.contains(&(3, "必須 key title が無い".to_owned())), "{shown:?}");
    assert!(shown.contains(&(23, "未知の key color".to_owned())), "{shown:?}");
    let unschema = contract_rows(&CONTRACT_TABLE.replacen("schema = 1\n", "", 1)).expect_err("schema は要る");
    assert!(unschema.iter().any(|error| error.message.contains("schema = 1 が無い")), "{unschema:?}");
}

/// 規則の面と契約表を混ぜない: rules manifest に `[[contract]]` は置けず、契約表に `[[rule]]` は置けない。
#[test]
fn rules_contract_table_and_the_rules_manifest_refuse_each_others_tables() {
    let mixed = format!("{GOOD}\n[[contract]]\nid = \"a\"\n");
    let joined = rejected(&mixed).expect("拒まれるはずの fixture が受理された").join("\n");
    assert!(joined.contains("[[contract]] は rules manifest に置けない"), "{joined}");
    let table = format!("{CONTRACT_TABLE}\n[[rule]]\nid = \"R\"\n");
    let errors = contract_rows(&table).expect_err("契約表に規則の行は置けない");
    assert!(errors.iter().any(|error| error.message.contains("[[rule]] は契約表に置けない")), "{errors:?}");
}
