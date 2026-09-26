//! rules manifest の歯（設計 docs/design/rules-manifest.md §7）。
//!
//! fixture は文字列 literal で持つ（file を置くと歯が repo の状態に依存する）。

use crate::{make_tmp_dir, TmpDir};
use std::process::Command;
use vessel::cli_outcome::{Outcome, RC_OK, RC_REFUSED};
use vessel::fleet::select::{Model, MODELS};
use vessel::headless::{Effort, EFFORTS};
use vessel::hook::host_guard::{Protected, PROTECTED};
use vessel::hook::command::{denied_in, denied_of};
use vessel::order::is_declaration_order;
use vessel::pipe::contract::{class_element, Class, ClassElement, CLASS_ROW};
use vessel::rules::manifest::{contract_rows, Manifest, TableValue};
use vessel::rules::{int_row, str_row, Rule, RuleKind, RuleValue, ValueShape, ALL};
use vessel::seat::brief;
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
value = "orchestrator"
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
/// 権能の行は `Capability` の名・`s2-07l.201`／役割の既定は `Model` と `Effort` の字面・`s2-07l.433`／rm の守る集合は
/// `Protected` の記号・`s2-07l.575`／クラスの語列表は `Class` の名 + 語列・`s2-07l.601`）。
fn sample_value(kind: RuleKind) -> String {
    match kind {
        RuleKind::DialogueSurface => format!("\"{}\"", Role::Orchestrator.as_str()),
        RuleKind::RoleCapabilities => format!("[\"{}\"]", Capability::Answer.as_str()),
        RuleKind::RoleModel => format!("\"{}\"", Model::Fable.alias()),
        RuleKind::RoleEffort => format!("\"{}\"", Effort::High.alias()),
        RuleKind::HostGuardRmProtected => format!("[\"{}\"]", Protected::StateDir.as_str()),
        RuleKind::RunnerClassCommands => format!("[\"{} sample\"]", Class::Publish.as_str()),
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
        RuleValue::Str("orchestrator".to_owned()),
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
fn host_state_dir(host: Option<&str>) -> Option<TmpDir> {
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

/// (h・`s2-07l.303`) `[[vessel]] repo` 1 行は host の面から読める（`validate --state-dir` が rc 0 で数え、`Manifest::vessel`
/// が dir と見出し行を運ぶ・tracked の面は持たない）。2 行目は**行番号付きで重複として拒む**（`host.toml:` の接頭辞・
/// stdout 0 行・1 行目の値は読まない）。`repo` 欠け・空・未知 key は他の表と同じ拒否形。面をまたぐ 2 行（`--rules` の
/// tracked 側と host 側）も host の面の行番号で拒む。base は `[[vessel]]` を未知の section として拒む（RED）。
#[test]
fn rules_host_vessel_row_is_read_and_duplicates_are_refused() {
    let one = "schema = 1\n\n[[account]]\nlabel = \"h1\"\n\n[[vessel]]\nrepo = \"/srv/vessel\"\n";
    let dir = host_state_dir(Some(one)).expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let outcome = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(outcome.out, vec![format!("{} accounts=1 plugins=0 launch-args=0 host=present", embedded_validate_line())]);
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    let found = manifest.vessel().expect("[[vessel]] が読める");
    assert_eq!((found.repo(), found.line()), ("/srv/vessel", 6), "dir と見出し行");
    assert_eq!(Manifest::embedded().expect("埋め込み").vessel(), None, "tracked の面は持たない");
    let host = dir.join(vessel::rules::HOST_MANIFEST);
    std::fs::write(&host, format!("{one}\n[[vessel]]\nrepo = \"/srv/other\"\n")).expect("host の面を書ける");
    let dup = rules_dispatch(&["validate", "--state-dir", &state]);
    assert_eq!(dup.rc, RC_REFUSED, "{dup:?}");
    assert!(dup.out.is_empty(), "stdout へは書かない: {dup:?}");
    assert_eq!(dup.err, vec!["rules: host.toml: [[vessel]] が重複する（最大 1 行） line=9".to_owned()], "2 行目を行番号で名指す");
    for (body, want) in [
        ("schema = 1\n\n[[vessel]]\n", "rules: host.toml: 必須 key repo が無い line=3"),
        ("schema = 1\n\n[[vessel]]\nrepo = \"\"\n", "rules: host.toml: repo が空である line=3"),
        ("schema = 1\n\n[[vessel]]\nrepo = \"/x\"\nbranch = \"main\"\n", "rules: host.toml: 未知の key branch line=5"),
        ("schema = 1\n\n[[vessel]]\nrepo = 3\n", "rules: host.toml: repo は文字列でなければならない（実 One(Int(3))） line=4"),
    ] {
        std::fs::write(&host, body).expect("host の面を書ける");
        let refused = rules_dispatch(&["validate", "--state-dir", &state]);
        assert_eq!(refused.rc, RC_REFUSED, "{body:?}: {refused:?}");
        assert_eq!(refused.err, vec![want.to_owned()], "{body:?}: 他の表と同じ拒否形・1 件");
    }
    let tracked = dir.join("tracked.toml");
    std::fs::write(&tracked, format!("{GOOD}\n[[vessel]]\nrepo = \"/srv/tracked\"\n")).expect("tracked の fixture を書ける");
    std::fs::write(&host, one).expect("host の面を書ける");
    let rules = tracked.display().to_string();
    let crossed = rules_dispatch(&["validate", "--rules", &rules, "--state-dir", &state]);
    assert_eq!(crossed.rc, RC_REFUSED, "{crossed:?}");
    assert_eq!(crossed.err, vec!["rules: host.toml: [[vessel]] が面をまたいで重複する（最大 1 行） line=6".to_owned()], "面をまたぐ 2 行");
    std::fs::remove_dir_all(&dir).ok();
}

// ─────────────────── host の面の `[[tick]]`（seat-heartbeat.md §5 形 1・契約表の行 d・接頭辞 `rules_host_tick_`） ───────────────────

/// 1 行の `[[tick]]` を持つ host の面（見出しは 6 行目・`unit-dir` は 7 行目・`binary` は 8 行目）。
const HOST_TICK: &str = "schema = 1\n\n[[account]]\nlabel = \"h1\"\n\n[[tick]]\nunit-dir = \"/srv/units\"\nbinary = \"/opt/bin/scribe2\"\n";

/// (1) 1 行の `[[tick]]` は host の面から読める: `validate --state-dir` は rc 0 で宣言の数の 1 行は表の無い面と同じ字面（表を
/// 数えない＝既存の外形は動かない）・`Manifest::tick` が 2 欄（絶対 path）と見出し行を運ぶ・tracked の面（埋め込み）は持たない。
/// base は `[[tick]]` を未知の section として拒む（RED）。
#[test]
fn rules_host_tick_one_row_is_read_with_two_absolute_fields() {
    let dir = host_state_dir(Some(HOST_TICK)).expect("tmp の state dir を作れる");
    let outcome = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(outcome.out, vec![format!("{} accounts=1 plugins=0 launch-args=0 host=present", embedded_validate_line())]);
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    let tick = manifest.tick().expect("[[tick]] が読める");
    assert_eq!((tick.unit_dir(), tick.binary(), tick.line()), ("/srv/units", "/opt/bin/scribe2", 6), "2 欄と見出し行");
    assert_eq!(Manifest::embedded().expect("埋め込み").tick(), None, "tracked の面は持たない");
    std::fs::remove_dir_all(&dir).ok();
}

/// (2) 2 行目・相対 path・欠けた欄は行番号つきで断る（`host.toml:` の接頭辞・rc 1・stdout 0 行・1 件ずつ）。tracked の面に置いた
/// 表は 1 表 1 件で断る。
#[test]
fn rules_host_tick_refuses_a_second_row_relative_paths_and_missing_fields() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let host = dir.join(vessel::rules::HOST_MANIFEST);
    for (body, want) in [
        (format!("{HOST_TICK}\n[[tick]]\nunit-dir = \"/srv/other\"\nbinary = \"/opt/bin/other\"\n"), vec!["rules: host.toml: [[tick]] が重複する（最大 1 行） line=10"]),
        (HOST_TICK.replace("\"/srv/units\"", "\"units\""), vec!["rules: host.toml: unit-dir が絶対 path でない: \"units\" line=7"]),
        (HOST_TICK.replace("\"/opt/bin/scribe2\"", "\"bin/scribe2\""), vec!["rules: host.toml: binary が絶対 path でない: \"bin/scribe2\" line=8"]),
        (HOST_TICK.replace("binary = \"/opt/bin/scribe2\"\n", ""), vec!["rules: host.toml: 必須 key binary が無い line=6"]),
        (HOST_TICK.replace("unit-dir = \"/srv/units\"\n", ""), vec!["rules: host.toml: 必須 key unit-dir が無い line=6"]),
    ] {
        std::fs::write(&host, &body).expect("host の面を書ける");
        let refused = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
        assert_eq!(refused.rc, RC_REFUSED, "{body:?}: {refused:?}");
        assert!(refused.out.is_empty(), "{body:?}: stdout へは書かない");
        assert_eq!(refused.err, want, "{body:?}");
    }
    let tracked = dir.join("tracked.toml");
    // `GOOD` は 17 行なので、空行を挟んで足した表の見出しは 19 行目。
    std::fs::write(&tracked, format!("{GOOD}\n[[tick]]\nunit-dir = \"/srv/units\"\nbinary = \"/opt/bin/scribe2\"\n")).expect("tracked の fixture を書ける");
    std::fs::remove_file(&host).ok();
    let outcome = rules_dispatch(&["validate", "--rules", &tracked.display().to_string(), "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert_eq!(outcome.err, vec!["rules: [[tick]] は tracked の manifest に置けない（unit の置き場は host の面だけ） line=19".to_owned()]);
    std::fs::remove_dir_all(&dir).ok();
}

/// (3) 表の無い host は今のまま: `Manifest::tick` は `None`・`validate --state-dir` の 1 行は表を足す前と同じ字面（`HOST_GOOD` の
/// 3 種の表の数だけ・外形 snapshot `rules_external_form` は同じ本文を撃つ）。
#[test]
fn rules_host_tick_absent_table_leaves_the_host_face_unchanged() {
    let dir = host_state_dir(Some(HOST_GOOD)).expect("tmp の state dir を作れる");
    let outcome = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.out, vec![format!("{} accounts=2 plugins=2 launch-args=2 host=present", embedded_validate_line())]);
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    assert_eq!(manifest.tick(), None, "表の無い面は None");
    std::fs::remove_dir_all(&dir).ok();
    let absent = host_state_dir(None).expect("tmp の state dir を作れる");
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(absent.as_path())))
        .expect("面が無くても続く");
    assert_eq!(manifest.tick(), None, "面の無い host も None");
    std::fs::remove_dir_all(&absent).ok();
}

// ─────────────────── host の面が dir（在るが読めない・`s2-07l.250`・`.243` run 3 の生存変異を塞ぐ） ───────────────────
// flip-check: retroactive s2-07l.250

/// `S/host.toml` の位置に dir を置いた state dir（権限に依らず読めない＝「無い」に潰れたら縮退の側へ倒れる）。
fn host_dir_state() -> Option<TmpDir> {
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

/// (g) 同じ fixture で `fleet usage` も typed に止まる（rc 1・stdout 0 byte・event を書かない）。
#[test]
fn rules_host_directory_host_manifest_stops_fleet_usage_without_events() {
    let dir = host_dir_state().expect("tmp の state dir を作れる");
    let state = dir.display().to_string();
    let host = dir.join(vessel::rules::HOST_MANIFEST).display().to_string();
    let bin = env!("CARGO_BIN_EXE_scribe2");
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

/// 埋め込みの manifest は選定の前計測の鮮度の行 `fleet.usage_fresh_s`（設計 account-autonomy.md §13 (1)・`s2-07l.407`）を
/// 値 300・裁定 id `user 2026-09-16T11:14Z`・裁定日 2026-09-16 つきで持ち、kind `UsageFreshS` は `ALL` に在って
/// `fleet.usage_timeout_s` と同じ Int の形（行と variant は対で足す・`fleet.usage_timeout_s` の歯と同型）。
#[test]
fn rules_embedded_manifest_declares_usage_fresh_s_with_its_ruling() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let fresh = manifest.get("fleet.usage_fresh_s").expect("鮮度の行が在る");
    assert_eq!(fresh.value, RuleValue::Int(300), "user 裁定 2026-09-16T11:14Z の値（秒）");
    assert_eq!(fresh.kind, RuleKind::UsageFreshS, "kind");
    assert!(ALL.contains(&RuleKind::UsageFreshS), "variant は ALL に在る");
    let at = ALL.iter().position(|kind| *kind == RuleKind::UsageTimeoutS).unwrap_or_default();
    assert_eq!(ALL.get(at.saturating_add(1)), Some(&RuleKind::UsageFreshS), "宣言順は UsageTimeoutS の直後");
    assert_eq!(fresh.kind.shape(), ValueShape::Int, "値の形は Int（秒）");
    assert!(fresh.enabled, "既定で効く");
    assert_eq!(fresh.ruling, "user 2026-09-16T11:14Z", "裁定 id");
    assert_eq!(fresh.ruled_at, "2026-09-16", "裁定日");
    let timeout = manifest.get("fleet.usage_timeout_s").expect("待ち時間の行が在る");
    assert_eq!(timeout.kind.shape(), fresh.kind.shape(), "待ち時間の行と同じ形");
    assert_ne!(timeout.ruling, fresh.ruling, "裁定は別（相乗りではない・rules-diff §4.3 (ii)）");
}

// ─── 群の逼迫の閾値（account-lifecycle.md §19 形 1・`s2-07l.491`・接頭辞 `rules_embedded_manifest_declares_group_pressure_`） ───

/// 群の逼迫の閾値の 3 行（id・kind・値）。値は user の要求そのもの（5 時間窓 85 / 7 日窓 95 / モデル別窓 95）。
const GROUP_PRESSURE_ROWS: [(&str, RuleKind, u64); 3] = [
    ("fleet.group_pressure_5h_pct", RuleKind::GroupPressure5hPct, 85),
    ("fleet.group_pressure_7d_pct", RuleKind::GroupPressure7dPct, 95),
    ("fleet.group_pressure_model_pct", RuleKind::GroupPressureModelPct, 95),
];

/// 群の逼迫の閾値の裁定 id と裁定日（user の要求 2026-09-19・逐語は台帳）。
const GROUP_PRESSURE_RULING: (&str, &str) = ("user 2026-09-19T12:12Z", "2026-09-19");

/// (1) 埋め込みの manifest は窓ごとに 1 行を持ち、値・発効・裁定 id・裁定日が行ごとに合い、整数の読み手で値が取れる
/// （3 行は**窓ごとに別の値**を持てる＝5 時間窓だけ 85）。
#[test]
fn rules_embedded_manifest_declares_group_pressure_rows_with_values_and_ruling() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    for (id, kind, value) in GROUP_PRESSURE_ROWS {
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!(row.kind, kind, "{id} の kind");
        assert_eq!(row.value, RuleValue::Int(value), "{id} の値（百分率）");
        assert!(row.enabled, "{id} は既定で効く");
        assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), GROUP_PRESSURE_RULING, "{id} の裁定 id と裁定日");
        assert_eq!(int_row(&manifest, id), Ok(value), "{id} は整数の読み手で取れる");
    }
    let shared = manifest.rows().iter().filter(|row| row.ruling == GROUP_PRESSURE_RULING.0).count();
    assert_eq!(shared, 3, "同じ裁定で決めた 3 行だけが裁定 id を持つ（他の行と相乗りしない）");
}

/// (2) kind 3 つは行と対で足され（`ALL` に在り字面から引ける）、宣言順は `UsageFreshS` の直後に 3 つ続き、値の形は Int。
#[test]
fn rules_embedded_manifest_declares_group_pressure_kinds_after_usage_fresh_s() {
    let at = ALL.iter().position(|kind| *kind == RuleKind::UsageFreshS).expect("UsageFreshS は ALL に在る");
    let next: Vec<RuleKind> = ALL.iter().skip(at.saturating_add(1)).take(3).copied().collect();
    let want: Vec<RuleKind> = GROUP_PRESSURE_ROWS.iter().map(|(_, kind, _)| *kind).collect();
    assert_eq!(next, want, "宣言順は UsageFreshS の直後に 5 時間窓 → 7 日窓 → モデル別窓");
    for (_, kind, _) in GROUP_PRESSURE_ROWS {
        assert_eq!(RuleKind::parse(kind.as_str()), Some(kind), "{} を字面から引ける", kind.as_str());
        assert_eq!(kind.shape(), ValueShape::Int, "{} の値の形は Int（百分率）", kind.as_str());
    }
}

/// (3) 形の外れた行は読み込みで断る（文字列の値・綴り違いの kind）。どれか 1 行でも欠けた manifest は 3 行を全部読めない
/// 周と区別できる（行 id が名指される）。
#[test]
fn rules_embedded_manifest_declares_group_pressure_rows_refused_when_malformed() {
    for (_, kind, _) in GROUP_PRESSURE_ROWS {
        let errors = rejected(&one_row(kind, "\"85\"")).expect("文字列の値の fixture が受理された");
        assert!(errors.join("\n").contains("形と合わない"), "{}: 形は Int だけ: {errors:?}", kind.as_str());
    }
    let errors = rejected(&one_row_raw("GroupPressure5hPcts", "85")).expect("未知の kind の fixture が受理された");
    assert!(errors.join("\n").contains("未知である"), "綴り違いの kind は読めない: {errors:?}");
    let empty = Manifest::parse("schema = 1\n").unwrap_or_else(|errors| panic!("空の manifest を読める: {errors:?}"));
    for (id, _, _) in GROUP_PRESSURE_ROWS {
        let refused = int_row(&empty, id).expect_err("行の無い manifest は値を返さない");
        assert!(refused.contains(id), "断りは行 id を名指す: {refused}");
    }
}

// ─── 同型の審査 FAIL の停止の回数（`review.same_kind_stop`・contract-source.md §23・`s2-07l.396`・接頭辞 `rules_review_same_kind_`） ───

/// 埋め込みの manifest は同型の停止の回数の行 `review.same_kind_stop` を **値 2**・kind `ReviewSameKindStop`・Int の形・
/// 発効・裁定 id `user 2026-09-16T05:53Z`・裁定日 2026-09-16 つきで持つ（**値は manifest が持ち、設計 doc は写さない**・
/// C1 / C5）。裁定は他の行と相乗りしない（rules-diff §4.3 (ii)）。
#[test]
fn rules_review_same_kind_stop_row_carries_value_two_and_its_ruling() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let row = manifest.get("review.same_kind_stop").expect("同型の停止の回数の行が在る");
    assert_eq!(row.value, RuleValue::Int(2), "user 裁定 2026-09-16T05:53Z の値（本）");
    assert_eq!(row.kind, RuleKind::ReviewSameKindStop, "kind");
    assert_eq!(row.kind.shape(), ValueShape::Int, "値の形は Int（本）");
    assert!(row.enabled, "既定で効く");
    assert_eq!(row.ruling, "user 2026-09-16T05:53Z", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-16", "裁定日");
    assert_eq!(int_row(&manifest, "review.same_kind_stop"), Ok(2), "整数の読み手で 2 が取れる");
    let shared = manifest.rows().iter().filter(|other| other.ruling == row.ruling).count();
    assert_eq!(shared, 1, "裁定 id は他の行と相乗りしない（母集団 {} 行）", manifest.rows().len());
}

/// kind `ReviewSameKindStop` は `ALL` の**宣言順**で `RoleEffort` の直後に在り（`.428` の `LandTrainMax` と `.398` の
/// `PipeMaxLive`、`.170` の flip の 2 kind が後ろに足された）、字面から引け、行と variant は対で足す＝variant を綴り違えた行の
/// manifest は未知の kind として読めない（片方だけの enum は親 test の `covers_all_kinds` が落ちる）。
#[test]
fn rules_review_same_kind_stop_kind_is_last_in_declaration_order_and_paired_with_the_row() {
    let at = ALL.iter().position(|kind| *kind == RuleKind::ReviewSameKindStop).unwrap_or_default();
    let run: Vec<RuleKind> = ALL.iter().skip(at).take(3).copied().collect();
    assert_eq!(
        run,
        vec![RuleKind::ReviewSameKindStop, RuleKind::LandTrainMax, RuleKind::PipeMaxLive],
        "宣言順で `.428` の LandTrainMax・`.398` の PipeMaxLive が直後に続く（母集団 {} 種）",
        ALL.len()
    );
    assert_eq!(RuleKind::parse("ReviewSameKindStop"), Some(RuleKind::ReviewSameKindStop), "kind を字面から引ける");
    assert_eq!(RuleKind::ReviewSameKindStop.as_str(), "ReviewSameKindStop", "字面は variant 名");
    let at = ALL.iter().position(|kind| *kind == RuleKind::RoleEffort).unwrap_or_default();
    assert_eq!(ALL.get(at.saturating_add(1)), Some(&RuleKind::ReviewSameKindStop), "`.433` の RoleEffort の直後");
    let errors = rejected(&one_row_raw("ReviewSameKindStops", "2")).expect("未知の kind の fixture が受理された");
    assert!(errors.join("\n").contains("未知である"), "行の kind を綴り違えた manifest は読めない: {errors:?}");
    let probe = parsed(&one_row(RuleKind::ReviewSameKindStop, "3")).expect("Int の値は受理される");
    assert_eq!(probe.get("probe").map(|found| found.value.clone()), Some(RuleValue::Int(3)));
    let errors = rejected(&one_row(RuleKind::ReviewSameKindStop, "\"two\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

// ─── 席の口座を持つ単位は project の群（host の面の `[[account-group]]`・account-lifecycle.md §17・接頭辞 `host_group_`） ───

/// 群 2 つを持つ host の面（口座 g1 / g2 / g3 は見出し行 3 / 6 / 9・群の見出し行は 12 と 17）。
const HOST_GROUPS: &str = r#"schema = 1

[[account]]
label = "g1"

[[account]]
label = "g2"

[[account]]
label = "g3"

[[account-group]]
name = "Tier1"
anchors = ["/repo/a", "/repo/b"]
accounts = ["g2", "g1"]

[[account-group]]
name = "Tier2"
anchors = ["/repo/c"]
accounts = ["g3"]
"#;

/// `HOST_GROUPS` の本文の口座の表だけ（群の行を差し替える土台・見出し行は同じ 3 / 6 / 9 で、次の表は 12 行目から）。
const GROUP_HEAD: &str = "schema = 1\n\n[[account]]\nlabel = \"g1\"\n\n[[account]]\nlabel = \"g2\"\n\n[[account]]\nlabel = \"g3\"\n";

/// `body` を host の面に置いて `rules validate --state-dir` を撃つ（rc と行を返す）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn group_validate(dir: &std::path::Path, body: &str) -> Outcome {
    std::fs::write(dir.join(vessel::rules::HOST_MANIFEST), body).expect("host の面を書ける");
    rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()])
}

/// (1) host の面の `[[account-group]]` を**既存の読み手 1 本**が読む: 3 key が宣言順で取れ、群も宣言順・行番号は
/// host の面の行。`rules validate --state-dir` は rc 0（口座の数は従来どおり数える＝群は口座の表を増やさない）。
/// 便用の除外の集合（`grouped_accounts`）は各群の今の口座（記録なし＝種・§23 形 1）。tracked の面（埋め込み）は群を 1 つも持たない。
/// base は `[[account-group]]` を未知の section として拒む（RED）。
#[test]
fn host_group_table_is_read_from_the_host_face_with_three_keys() {
    let dir = host_state_dir(Some(HOST_GROUPS)).expect("tmp の state dir を作れる");
    let outcome = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(
        outcome.out,
        vec![format!("{} accounts=3 plugins=0 launch-args=0 host=present", embedded_validate_line())],
        "群は口座 / plugin / 起動引数の数を動かさない"
    );
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    let groups: Vec<(&str, Vec<&str>, Vec<&str>, u64)> = manifest
        .groups()
        .iter()
        .map(|group| {
            let anchors: Vec<&str> = group.anchors().iter().map(String::as_str).collect();
            let accounts: Vec<&str> = group.accounts().iter().map(String::as_str).collect();
            (group.name(), anchors, accounts, group.line())
        })
        .collect();
    assert_eq!(
        groups,
        [
            ("Tier1", vec!["/repo/a", "/repo/b"], vec!["g2", "g1"], 12),
            ("Tier2", vec!["/repo/c"], vec!["g3"], 17),
        ],
        "群も置き場も候補も宣言順（候補の順は label の昇順ではない）"
    );
    // 便用の除外は各群の今の口座だけ（記録なし＝種＝候補の先頭・§23 形 1）。置き場は tmp の 1 段下（host の根の記録を
    // 歯どうしで共有しない）。
    let place = dir.join("place");
    std::fs::create_dir_all(&place).expect("置き場を作れる");
    std::fs::write(place.join(vessel::rules::HOST_MANIFEST), HOST_GROUPS).expect("host の面を書ける");
    let grouped: Vec<String> = vessel::rules::grouped_accounts(&place).expect("除外を解ける").into_iter().collect();
    assert_eq!(grouped, ["g2", "g3"], "便用の除外は各群の今の口座（候補の和ではない）");
    assert!(Manifest::embedded().expect("埋め込み").groups().is_empty(), "tracked の面は群を持たない");
    std::fs::remove_dir_all(&dir).ok();
}

/// (2) host の面が無い周は 0 群で続く（縮退・rc 0・`host=absent`）。除外も 0 件＝便用の候補は今までどおり。
#[test]
fn host_group_absent_host_face_declares_zero_groups() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let outcome = rules_dispatch(&["validate", "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert_eq!(
        outcome.out,
        vec![format!("{} accounts=0 plugins=0 launch-args=0 host=absent", embedded_validate_line())]
    );
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("面が無くても続く");
    assert!(manifest.groups().is_empty(), "0 群: {:?}", manifest.groups());
    assert_eq!(
        vessel::rules::grouped_accounts(dir.as_path()),
        Ok(std::collections::BTreeSet::new()),
        "置き場から直に読む口も 0 件（便の口はこちらを読む）"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// (3) tracked の面に群の表が在る周は**未知の表**として行番号付きで断る（`[[rule]]` を host の面で断るのと対称）。
/// 行の中身は検査しない（1 表 1 件）＝未知 key も必須 key の欠けも重ねない。
#[test]
fn host_group_table_on_the_tracked_face_is_refused_as_unknown() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let tracked = dir.join("tracked.toml");
    // `GOOD` は 17 行なので、空行を挟んで足した表の見出しは 19 行目。key は 3 つとも書かない（1 表 1 件の確認）。
    std::fs::write(&tracked, format!("{GOOD}\n[[account-group]]\n")).expect("tracked の fixture を書ける");
    let outcome = rules_dispatch(&["validate", "--rules", &tracked.display().to_string(), "--state-dir", &dir.display().to_string()]);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert!(outcome.out.is_empty(), "stdout へは書かない: {outcome:?}");
    assert_eq!(
        outcome.err,
        vec!["rules: [[account-group]] は tracked の manifest に置けない（群の宣言は host の面だけ） line=19".to_owned()],
        "1 表 1 件・行番号付き・host の面の接頭辞は付かない"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// (4) 宣言の欠陥 6 種を行番号付きで**全件**断る（`host.toml:` の接頭辞・stdout 0 行）: 同じ名が 2 行・同じ置き場が
/// 2 つの群・宣言に無い候補・置き場の列が空・候補の列が空・未知の key。最後に、面の中の欠陥で止まった周は**合わせの
/// 検査へ進まない**（同じ本文が未知の候補も持つのに、出るのは面の中の 1 件だけ）。
#[test]
fn host_group_defects_are_refused_with_line_numbers_and_the_face_prefix() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let group = |name: &str, anchors: &str, accounts: &str| {
        format!("\n[[account-group]]\nname = \"{name}\"\nanchors = {anchors}\naccounts = {accounts}\n")
    };
    let tier1 = group("Tier1", "[\"/repo/a\"]", "[\"g1\"]");
    for (body, want) in [
        // 2 つ目の群の見出し（17 行目）で名の重複。
        (format!("{GROUP_HEAD}{tier1}{}", group("Tier1", "[\"/repo/b\"]", "[\"g2\"]")), vec![(17, "群の名 Tier1 が重複する")]),
        // 同じ置き場が 2 つの群に在る（2 つ目の群の見出し）。
        (format!("{GROUP_HEAD}{tier1}{}", group("Tier2", "[\"/repo/a\"]", "[\"g2\"]")), vec![(17, "置き場 /repo/a が 2 つの群に在る")]),
        // 宣言に無い候補（合わせの検査・群の見出し行）。
        (format!("{GROUP_HEAD}{}", group("Tier1", "[\"/repo/a\"]", "[\"nope\"]")), vec![(12, "群 Tier1 の候補 nope が宣言された口座に無い")]),
        // 置き場の列が空（14 行目 = anchors の行）。
        (format!("{GROUP_HEAD}{}", group("Tier1", "[]", "[\"g1\"]")), vec![(14, "anchors の 配列が空である")]),
        // 候補の列が空（15 行目 = accounts の行）。
        (format!("{GROUP_HEAD}{}", group("Tier1", "[\"/repo/a\"]", "[]")), vec![(15, "accounts の 配列が空である")]),
        // 未知の key（16 行目）。
        (format!("{GROUP_HEAD}{tier1}model = \"opus\"\n"), vec![(16, "未知の key model")]),
    ] {
        let outcome = group_validate(dir.as_path(), &body);
        assert_eq!(outcome.rc, RC_REFUSED, "{body}: {outcome:?}");
        assert!(outcome.out.is_empty(), "{body}: stdout へは書かない");
        assert_eq!(outcome.err.len(), want.len(), "{body}: 全件・同じ欠陥を 2 行にしない: {:?}", outcome.err);
        for ((line, reason), got) in want.iter().zip(&outcome.err) {
            assert!(got.starts_with("rules: host.toml: "), "{body}: 面を名指す接頭辞: {got}");
            assert!(got.contains(reason) && got.ends_with(&format!(" line={line}")), "{body}: {reason} line={line}: {got}");
        }
    }
    // 面の中の欠陥（名の重複）と合わせの欠陥（未知の候補 nope）を同時に持つ本文は、面の中の 1 件だけを出す。
    let both = format!("{GROUP_HEAD}{tier1}{}", group("Tier1", "[\"/repo/b\"]", "[\"nope\"]"));
    let outcome = group_validate(dir.as_path(), &both);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert_eq!(
        outcome.err,
        vec!["rules: host.toml: 群の名 Tier1 が重複する line=17".to_owned()],
        "面の中で止まった周は合わせの検査へ進まない"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ─── 群の種は宣言順に重ならない（account-lifecycle.md §28・契約表の行 q・接頭辞 `host_group_seed_`） ───

/// 群 1 つの宣言（`GROUP_HEAD` に続ける・見出しは 1 つ目が 12 行目・2 つ目が 17 行目）。
fn seed_group(name: &str, anchor: &str, accounts: &str) -> String {
    format!("\n[[account-group]]\nname = \"{name}\"\nanchors = [\"{anchor}\"]\naccounts = {accounts}\n")
}

/// `body` を tmp の 1 段下の置き場の host の面に置き、宣言順に各群の解決の 1 関数の値（label と出所）を返す
/// （置き場を 1 段下にするのは host の根の記録を歯どうしで共有しないため）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn seed_currents(place: &std::path::Path, body: &str) -> Vec<(String, vessel::hook::group::Source)> {
    std::fs::create_dir_all(place).expect("置き場を作れる");
    std::fs::write(place.join(vessel::rules::HOST_MANIFEST), body).expect("host の面を書ける");
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(place)))
        .expect("host の面を合わせられる");
    manifest
        .groups()
        .iter()
        .map(|group| vessel::hook::group::current_of(place, group).expect("記録を読める"))
        .map(|found| (found.label, found.source))
        .collect()
}

/// (d) 同じ候補の列 [g1, g2, g3] を宣言した 2 群の記録なしの周は、今の口座が宣言順に先頭 g1 と 2 番目 g2（種は前の群の種で
/// ない最初の候補）で、便用の除外も 2 つ。(f) 後の群に記録（g3）が在れば記録が勝ち、前の群は種 g1 のまま。
/// base は両群とも先頭 g1（除外は 1 つ）→ RED。
#[test]
fn host_group_seed_two_groups_with_the_same_candidates_take_the_first_and_the_second() {
    use vessel::hook::group::Source;
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let place = dir.join("place");
    let same = "[\"g1\", \"g2\", \"g3\"]";
    let body = format!("{GROUP_HEAD}{}{}", seed_group("Tier1", "/repo/a", same), seed_group("Tier2", "/repo/b", same));
    assert_eq!(
        seed_currents(&place, &body),
        [("g1".to_owned(), Source::Seed), ("g2".to_owned(), Source::Seed)],
        "種は宣言順に先頭と 2 番目"
    );
    let grouped: Vec<String> = vessel::rules::grouped_accounts(&place).expect("除外を解ける").into_iter().collect();
    assert_eq!(grouped, ["g1", "g2"], "便用の除外は 2 つ（同じ口座に畳まれない）");
    let record = vessel::hook::group::current_path(&vessel::seat::host_groups_dir(&place), "Tier2");
    std::fs::create_dir_all(record.parent().expect("記録の dir")).expect("群用 dir を作れる");
    std::fs::write(&record, "account=g3\nts=2026-09-25T00:00:00Z\nreason=move\nprevious=g2\n").expect("記録を書ける");
    assert_eq!(
        seed_currents(&place, &body),
        [("g1".to_owned(), Source::Seed), ("g3".to_owned(), Source::Record)],
        "記録 > 種（記録は種に依らない）"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// (e) 候補 1 つ（g1）を共有する 2 群は、後の群の種を決める候補が無い＝面の欠陥（2 番目の群の見出し 17 行目・`host.toml:` の
/// 接頭辞・全件 1 件）で、便用の除外も面の欠陥で typed に止まる（fail-closed・前の群の種に読み替えない）。
/// base は両群とも種 g1 で rc 0 → RED。
#[test]
fn host_group_seed_one_shared_candidate_is_a_defect_on_the_second_group_line() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let body = format!("{GROUP_HEAD}{}{}", seed_group("Tier1", "/repo/a", "[\"g1\"]"), seed_group("Tier2", "/repo/b", "[\"g1\"]"));
    let outcome = group_validate(dir.as_path(), &body);
    assert_eq!(outcome.rc, RC_REFUSED, "{outcome:?}");
    assert!(outcome.out.is_empty(), "stdout へは書かない: {outcome:?}");
    assert_eq!(outcome.err, vec!["rules: host.toml: 群 Tier2 の種を決める候補が無い line=17".to_owned()], "2 番目の群の行で 1 件");
    assert!(
        matches!(vessel::rules::grouped_accounts(dir.as_path()), Err(vessel::rules::GroupedError::Manifest(_))),
        "除外は面の欠陥で止まる"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// (g) 群 1 つの host は種が候補の先頭のまま（候補の順は label の昇順ではない＝[g2, g1] の種は g2）。
#[test]
fn host_group_seed_single_group_keeps_the_first_candidate() {
    use vessel::hook::group::Source;
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let body = format!("{GROUP_HEAD}{}", seed_group("Tier1", "/repo/a", "[\"g2\", \"g1\"]"));
    assert_eq!(seed_currents(&dir.join("place"), &body), [("g2".to_owned(), Source::Seed)], "先頭のまま");
    std::fs::remove_dir_all(&dir).ok();
}

// ─── 群の名は Tier と数字・宣言順は数字の昇順（account-lifecycle.md §29 の行 s・ADR-0069・接頭辞 `host_group_tier_`） ───

/// 群の宣言の列（名・置き場 `/repo/<n>`・候補 1 つ）を `GROUP_HEAD` に続け、host の面の欠陥の行（rc 1 と stdout 0 行を測った後）を返す。
fn tier_refusals(dir: &std::path::Path, groups: &[(&str, &str)]) -> Vec<String> {
    let body = groups.iter().enumerate().fold(GROUP_HEAD.to_owned(), |body, (at, (name, label))| {
        format!("{body}{}", seed_group(name, &format!("/repo/{at}"), &format!("[\"{label}\"]")))
    });
    let outcome = group_validate(dir, &body);
    assert_eq!(outcome.rc, RC_REFUSED, "{body}: {outcome:?}");
    assert!(outcome.out.is_empty(), "{body}: stdout へは書かない");
    outcome.err
}

/// (j) 名が Tier と数字の形でない群（alpha）は群の見出し行（12 行目）の欠陥で 1 件（`host.toml:` の接頭辞）。base は通る（RED）。
#[test]
fn host_group_tier_non_tier_name_is_a_defect_on_the_group_line() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let err = tier_refusals(&dir, &[("alpha", "g1")]);
    assert_eq!(err.len(), 1, "1 件: {err:?}");
    let got = err.first().cloned().unwrap_or_default();
    assert!(got.starts_with("rules: host.toml: 群の名 alpha "), "面と名を名指す: {got}");
    assert!(got.contains("Tier と数字の形でない") && got.ends_with(" line=12"), "群の行の形の欠陥: {got}");
    std::fs::remove_dir_all(&dir).ok();
}

/// (k) Tier2, Tier1 の宣言順は 2 番目の群の見出し行（17 行目）の欠陥で 1 件（数字の昇順でない）。
#[test]
fn host_group_tier_descending_order_is_a_defect_on_the_second_group_line() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let err = tier_refusals(&dir, &[("Tier2", "g1"), ("Tier1", "g2")]);
    assert_eq!(err.len(), 1, "1 件: {err:?}");
    let got = err.first().cloned().unwrap_or_default();
    assert!(got.starts_with("rules: host.toml: 群 Tier1 "), "後の群を名指す: {got}");
    assert!(got.contains("前の群より大きくない") && got.ends_with(" line=17"), "2 番目の群の行: {got}");
    std::fs::remove_dir_all(&dir).ok();
}

/// (l) Tier1, Tier1 は名の重複の 1 件だけ（昇順の欠陥を重ねない＝同じ欠陥を 2 行にしない）。
#[test]
fn host_group_tier_same_name_twice_is_only_the_duplicate_defect() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let err = tier_refusals(&dir, &[("Tier1", "g1"), ("Tier1", "g2")]);
    assert_eq!(err, vec!["rules: host.toml: 群の名 Tier1 が重複する line=17".to_owned()], "名の重複の 1 件だけ");
    std::fs::remove_dir_all(&dir).ok();
}

/// (m) 先頭の 0（Tier01・Tier0）・数字なし（Tier）・接頭辞の綴り違い（tier1）・数字の後ろの字（Tier1a）は形の欠陥で 1 件。
#[test]
fn host_group_tier_malformed_digits_are_a_form_defect() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    for name in ["Tier01", "Tier0", "Tier", "tier1", "Tier1a"] {
        let err = tier_refusals(&dir, &[(name, "g1")]);
        assert_eq!(err.len(), 1, "{name}: 1 件: {err:?}");
        let got = err.first().cloned().unwrap_or_default();
        assert!(got.contains(&format!("群の名 {name} ")) && got.contains("Tier と数字の形でない"), "{name}: {got}");
        assert!(got.ends_with(" line=12"), "{name}: 群の行: {got}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// (n) Tier1, Tier2, Tier10 は通る（数字は数値で比べる＝辞書順で Tier10 < Tier2 と読まない）。群は宣言順のまま。
#[test]
fn host_group_tier_ascending_numbers_pass_compared_numerically() {
    let dir = host_state_dir(None).expect("tmp の state dir を作れる");
    let body = format!(
        "{GROUP_HEAD}{}{}{}",
        seed_group("Tier1", "/repo/a", "[\"g1\"]"),
        seed_group("Tier2", "/repo/b", "[\"g2\"]"),
        seed_group("Tier10", "/repo/c", "[\"g3\"]")
    );
    let outcome = group_validate(dir.as_path(), &body);
    assert_eq!(outcome.rc, RC_OK, "{outcome:?}");
    assert!(outcome.err.is_empty(), "欠陥 0: {outcome:?}");
    let manifest = Manifest::embedded()
        .and_then(|tracked| vessel::rules::with_state_dir(tracked, Some(dir.as_path())))
        .expect("host の面を合わせられる");
    let names: Vec<&str> = manifest.groups().iter().map(|group| group.name()).collect();
    assert_eq!(names, ["Tier1", "Tier2", "Tier10"], "宣言順のまま（数字で並べ替えない）");
    std::fs::remove_dir_all(&dir).ok();
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

/// 着地の列を候補の木 1 つに積む本数の上限の行（`land.train_max`・裁定 id `user 2026-09-17T03:28Z`・設計
/// pipeline.md §40・ADR-0039）。**値は manifest が持ち、設計 doc は写さない**（C1 / C5）。
/// 行の kind を綴り違えた manifest は `RuleError` で拒まれる（kind の字面は `ALL` を通してしか解けない）。
#[test]
fn rules_land_train_max_row_is_declared() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let row = manifest.get("land.train_max").expect("着地の列の上限の行が在る");
    assert_eq!(row.value, RuleValue::Int(4), "user 裁定 2026-09-17T03:28Z の値");
    assert_eq!(row.kind, RuleKind::LandTrainMax, "kind");
    assert_eq!(row.kind.shape(), ValueShape::Int, "値の形は Int（本）");
    assert!(row.enabled, "既定で効く");
    assert_eq!(row.ruling, "user 2026-09-17T03:28Z", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-17", "裁定日");
    assert!(ALL.contains(&RuleKind::LandTrainMax), "ALL に在る");
    assert_eq!(RuleKind::parse("LandTrainMax"), Some(RuleKind::LandTrainMax), "kind を字面から引ける");
    let errors = rejected(&one_row_raw("LandTrain", "4")).expect("未知の kind の fixture が受理された");
    let joined = errors.join("\n");
    assert!(joined.contains("未知である"), "行の kind を綴り違えた manifest は読めない: {joined}");
}

/// host で同時に走る便の本数の最大値の行（`pipe.max_live`・裁定 id `user 2026-09-16T11:14Z`・設計 gate-cost.md §24・
/// ADR-0035・`s2-07l.398`）。**値は manifest が持ち、設計 doc は写さない**（C1 / C5）。行と variant は対で足す
/// （片方だけの manifest は未知の kind として読めず、片方だけの enum は `covers_all_kinds` が落とす・行 j と同型）。
#[test]
fn rules_embedded_manifest_declares_max_live_row_with_its_ruling() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
    let row = manifest.get("pipe.max_live").expect("同時本数の最大値の行が在る");
    assert_eq!(row.value, RuleValue::Int(16), "user 裁定 2026-09-16T11:14Z の値（本）");
    assert_eq!(row.kind, RuleKind::PipeMaxLive, "kind");
    assert_eq!(row.kind.shape(), ValueShape::Int, "値の形は Int（本）");
    assert!(row.enabled, "既定で効く");
    assert_eq!(row.ruling, "user 2026-09-16T11:14Z", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-16", "裁定日");
    assert_eq!(int_row(&manifest, "pipe.max_live"), Ok(16), "整数の読み手で 16 が取れる");
    let at = ALL.iter().position(|kind| *kind == RuleKind::PipeMaxLive).unwrap_or_default();
    assert_eq!(ALL.get(at.saturating_add(1)), Some(&RuleKind::FlipDocsOnlyFaces), "直後は `.170` の flip の kind（母集団 {} 種）", ALL.len());
    assert_eq!(RuleKind::parse("PipeMaxLive"), Some(RuleKind::PipeMaxLive), "kind を字面から引ける");
    let errors = rejected(&one_row_raw("PipeMaxLives", "16")).expect("未知の kind の fixture が受理された");
    let joined = errors.join("\n");
    assert!(joined.contains("未知である"), "行の kind を綴り違えた manifest は読めない: {joined}");
    let errors = rejected(&one_row(RuleKind::PipeMaxLive, "\"sixteen\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

// ─── 入口の flip check の免除経路の行（設計 pipeline.md §7・`s2-07l.170`・接頭辞 `rules_flip_`） ───

/// flip の 2 行が持つ裁定 id と裁定日（C5・逐語は台帳）。
const FLIP_RULING: (&str, &str) = ("user 2026-09-22T06:48Z", "2026-09-22");

/// docs-only の面の行（`flip.docs_only_faces`）が kind・enabled・裁定 id・ALL の列・parse の 5 面で引ける。
/// **値は manifest が持ち、設計 doc は写さない**（C1 / C5）。形は List だけで、kind の綴り違いは未知として読めない。
#[test]
fn rules_flip_docs_only_faces_row_is_declared_on_five_faces() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get("flip.docs_only_faces").expect("docs-only の面の行が在る");
    let faces = ["docs/", "design-intent/", ".beads/", "README.md", "CLAUDE.md"];
    assert_eq!(row.value, RuleValue::List(faces.iter().map(|face| (*face).to_owned()).collect()), "裁定の値");
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::FlipDocsOnlyFaces, ValueShape::List), "kind と形");
    assert!(row.enabled, "既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), FLIP_RULING, "裁定 id と裁定日");
    assert!(ALL.contains(&RuleKind::FlipDocsOnlyFaces), "ALL に在る");
    assert_eq!(RuleKind::parse("FlipDocsOnlyFaces"), Some(RuleKind::FlipDocsOnlyFaces), "kind を字面から引ける");
    assert_eq!(RuleKind::FlipDocsOnlyFaces.as_str(), "FlipDocsOnlyFaces", "字面は variant 名");
    let errors = rejected(&one_row_raw("FlipDocsOnlyFace", "[\"docs/\"]")).expect("未知の kind の fixture が受理された");
    assert!(errors.join("\n").contains("未知である"), "綴り違いの kind は読めない: {errors:?}");
    let errors = rejected(&one_row(RuleKind::FlipDocsOnlyFaces, "16")).expect("整数の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は List だけ: {errors:?}");
}

/// 札の上限の行（`flip.marks_per_pr`）が同じ 5 面で引け、整数の読み手で値が取れる。2 行は `ALL` の宣言順の末尾に
/// 対で在る（`PipeMaxLive` の後ろ）。
#[test]
fn rules_flip_marks_per_pr_row_is_declared_on_five_faces() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get("flip.marks_per_pr").expect("札の上限の行が在る");
    assert_eq!(row.value, RuleValue::Int(16), "裁定の値（本）");
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::FlipMarksPerPr, ValueShape::Int), "kind と形");
    assert!(row.enabled, "既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), FLIP_RULING, "裁定 id と裁定日");
    assert_eq!(int_row(&manifest, "flip.marks_per_pr"), Ok(16), "整数の読み手で 16 が取れる");
    let at = ALL.iter().position(|kind| *kind == RuleKind::PipeMaxLive).unwrap_or_default();
    let run: Vec<RuleKind> = ALL.iter().skip(at).take(4).copied().collect();
    assert_eq!(
        run,
        vec![RuleKind::PipeMaxLive, RuleKind::FlipDocsOnlyFaces, RuleKind::FlipMarksPerPr, RuleKind::LedgerDeniedWrites],
        "flip の 2 kind は PipeMaxLive の後ろに対で在り、その後ろが `.169` の kind（その後ろに `.574` の 2 kind・母集団 {} 種）",
        ALL.len()
    );
    assert_eq!(RuleKind::parse("FlipMarksPerPr"), Some(RuleKind::FlipMarksPerPr), "kind を字面から引ける");
    let errors = rejected(&one_row_raw("FlipMarkPerPr", "16")).expect("未知の kind の fixture が受理された");
    assert!(errors.join("\n").contains("未知である"), "綴り違いの kind は読めない: {errors:?}");
    let errors = rejected(&one_row(RuleKind::FlipMarksPerPr, "\"sixteen\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

/// 台帳 write の断る形の行（`ledger.denied_writes`・設計 vessel-hook.md §10・`s2-07l.169`）が kind・値の 4 語・enabled・
/// 裁定 id の 4 面で引ける。**値は manifest が持つ**（C1 / C5）。形は List だけで、kind の綴り違いは未知として読めない。
#[test]
fn rules_ledger_denied_writes_row_is_declared_on_four_faces() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get("ledger.denied_writes").expect("台帳 write の断る形の行が在る");
    let forms = ["notes-replace", "memory-subcommand", "create-without-parent", "bd-outside-bdw"];
    assert_eq!(row.value, RuleValue::List(forms.iter().map(|form| (*form).to_owned()).collect()), "裁定の値（閉じた 4 語）");
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::LedgerDeniedWrites, ValueShape::List), "kind と形");
    assert!(row.enabled, "既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-22T08:44Z", "2026-09-22"), "裁定 id と裁定日");
    assert!(ALL.contains(&RuleKind::LedgerDeniedWrites), "ALL に在る");
    assert_eq!(RuleKind::parse("LedgerDeniedWrites"), Some(RuleKind::LedgerDeniedWrites), "kind を字面から引ける");
    let errors = rejected(&one_row_raw("LedgerDeniedWrite", "[\"notes-replace\"]")).expect("未知の kind の fixture が受理された");
    assert!(errors.join("\n").contains("未知である"), "綴り違いの kind は読めない: {errors:?}");
    let errors = rejected(&one_row(RuleKind::LedgerDeniedWrites, "16")).expect("整数の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は List だけ: {errors:?}");
}

// ─── host の破壊防止の見張りの種類ごとの行（設計 vessel-hook.md §11 行 b・ADR-0056・`s2-07l.574`・接頭辞
// `rules_embedded_manifest_declares_host_guard_`） ───

/// host の見張りの 4 行が持つ裁定 id と裁定日（user 裁定 2026-09-19T15:28Z・UTC・逐語は台帳）。
const HOST_GUARD_RULING: (&str, &str) = ("user 2026-09-19T15:28Z", "2026-09-19");

/// 4 行（host_guard.git / tmux / ledger は HostGuardDeniedCommands・host_guard.rm は HostGuardRmProtected）が id / kind / 形 List /
/// enabled / 裁定 id / 裁定日で引け、同じ裁定 id を持つ行はちょうど 4 本。host_guard.git は git の 7 語列（runner.denied_commands
/// から移した＝行 f・[`rules_moved_host_guard_git_sequences_leave_runner_denied_commands`]）で、rm の値は守る集合の 3 記号。
/// **値は manifest が持つ**（C1 / C5）。
#[test]
fn rules_embedded_manifest_declares_host_guard_rows_with_one_ruling() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    for (id, kind) in [
        ("host_guard.git", RuleKind::HostGuardDeniedCommands),
        ("host_guard.tmux", RuleKind::HostGuardDeniedCommands),
        ("host_guard.ledger", RuleKind::HostGuardDeniedCommands),
        ("host_guard.rm", RuleKind::HostGuardRmProtected),
    ] {
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!((row.kind, row.kind.shape()), (kind, ValueShape::List), "{id} の kind と形");
        assert!(row.enabled, "{id} は既定で効く");
        assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), HOST_GUARD_RULING, "{id} の裁定 id と裁定日");
        let RuleValue::List(ref items) = row.value else {
            panic!("{id} の値は列");
        };
        assert!(!items.is_empty(), "{id} の値は非空");
    }
    let shared = manifest.rows().iter().filter(|row| row.ruling == HOST_GUARD_RULING.0).count();
    assert_eq!(shared, 4, "同じ裁定 id の行はちょうど 4 本（母集団 {} 行）", manifest.rows().len());
    let git = ["git push --force", "git push -f", "git reset --hard", "git branch -D", "git clean -f", "git stash drop", "git stash clear"];
    let want: Vec<String> = git.iter().map(|item| (*item).to_owned()).collect();
    assert_eq!(manifest.get("host_guard.git").map(|row| row.value.clone()), Some(RuleValue::List(want)), "git の 7 語列");
    for (id, sequences) in [
        ("host_guard.tmux", &["tmux kill-server", "tmux -C", "tmux -f"][..]),
        ("host_guard.ledger", &["bd delete", "dolt reset --hard", "dolt branch -D", "dolt branch -d"][..]),
    ] {
        let want: Vec<String> = sequences.iter().map(|item| (*item).to_owned()).collect();
        assert_eq!(manifest.get(id).map(|row| row.value.clone()), Some(RuleValue::List(want)), "{id} の初期値（§11 形 3）");
    }
    let symbols = ["state-dir", "repo-tracked", "repo-git"].map(str::to_owned).to_vec();
    assert_eq!(manifest.get("host_guard.rm").map(|row| row.value.clone()), Some(RuleValue::List(symbols)), "守る集合の 3 記号");
}

/// kind 2 つは `LedgerDeniedWrites` の直後に宣言順で並び、その直後に管理 tick の 3 kind（`.582`・設計 seat-heartbeat.md §2
/// 形 5 / §10 形 5＝梯子の列の kind は `SeatTickStaleS` の直後）と退避の猶予の kind（`.651`・§13 形 1＝梯子の列の kind の直後）が
/// 宣言順で続き、クラスの語列表の kind（`.601`・設計 contract-source.md §48）で `ALL` が終わり、字面から引ける。末尾の絶対位置で
/// なく `LedgerDeniedWrites` の位置から測る（kind を末尾に足した後も同じ並びを測る）。
#[test]
fn rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all() {
    let at = ALL.iter().position(|kind| *kind == RuleKind::LedgerDeniedWrites).expect("LedgerDeniedWrites は ALL に在る");
    let run: Vec<RuleKind> = ALL.iter().skip(at).copied().collect();
    assert_eq!(
        run,
        vec![
            RuleKind::LedgerDeniedWrites,
            RuleKind::HostGuardDeniedCommands,
            RuleKind::HostGuardRmProtected,
            RuleKind::SeatTickIntervalS,
            RuleKind::SeatTickStaleS,
            RuleKind::SeatPointerLadderS,
            RuleKind::SeatMoveGraceS,
            RuleKind::RunnerClassCommands,
        ],
        "LedgerDeniedWrites の後ろに宣言順で 2 kind・管理 tick の 3 kind・退避の猶予・クラスの語列表の kind で ALL が終わる（母集団 {} 種）",
        ALL.len()
    );
    let tail: Vec<RuleKind> = ALL.iter().rev().take(5).rev().copied().collect();
    assert_eq!(
        tail,
        vec![
            RuleKind::SeatTickIntervalS,
            RuleKind::SeatTickStaleS,
            RuleKind::SeatPointerLadderS,
            RuleKind::SeatMoveGraceS,
            RuleKind::RunnerClassCommands
        ],
        "逆順に 5 つ取った並びが管理 tick の 3 kind・退避の猶予・クラスの語列表の kind"
    );
    for kind in [RuleKind::HostGuardDeniedCommands, RuleKind::HostGuardRmProtected] {
        assert_eq!(RuleKind::parse(kind.as_str()), Some(kind), "kind を字面から引ける: {}", kind.as_str());
    }
    assert_eq!(RuleKind::HostGuardDeniedCommands.as_str(), "HostGuardDeniedCommands", "字面は variant 名");
    assert_eq!(RuleKind::HostGuardRmProtected.as_str(), "HostGuardRmProtected", "字面は variant 名");
}

// ─── 管理 tick の行 3 本（設計 seat-heartbeat.md §2 形 5 / §10 形 5〜7・ADR-0058 §2・ADR-0068・`s2-07l.582` / `s2-07l.637`・
// 接頭辞 `rules_embedded_manifest_declares_tick_`） ───

/// 管理 tick の 3 行が持つ裁定 id と裁定日（梯子の列と周期の user 裁定 2026-09-25T14:39Z・UTC・逐語は台帳）。
const TICK_RULING: (&str, &str) = ("user 2026-09-25T14:39Z", "2026-09-25");

/// 管理 tick の整数の 2 行（id・kind・値）。値は裁定の値（timer 15 秒・黙りの閾値 30 分）。
const TICK_ROWS: [(&str, RuleKind, u64); 2] =
    [("seat.tick_interval_s", RuleKind::SeatTickIntervalS, 15), ("seat.tick_stale_s", RuleKind::SeatTickStaleS, 1800)];

/// 梯子の列の行の id。
const TICK_LADDER_ROW: &str = "seat.pointer_ladder_s";

/// 梯子の列の裁定の値（30 分 → 1 時間 → 3 時間 → 6 時間 → 12 時間 → 24 時間 → 停止）。
const TICK_LADDER: [&str; 6] = ["1800", "3600", "10800", "21600", "43200", "86400"];

/// 退役した梯子の行 2 本の id（係数と上限・§10 形 5）。
const TICK_RETIRED_ROWS: [&str; 2] = ["seat.pointer_backoff_factor", "seat.pointer_backoff_max_s"];

/// 整数の 2 行が埋め込み manifest に id / kind / 形 Int / 値 / enabled / 裁定 id / 裁定日で在り、整数の読み手で値が取れ、kind
/// ごとに 1 行（timer 15 秒・黙りの閾値 1800 秒）。同じ裁定 id の行は梯子の列を足してちょうど 3 本。**値は manifest が持つ**
/// （C1 / C5）。
#[test]
fn rules_embedded_manifest_declares_tick_rows_with_one_ruling() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    for (id, kind, value) in TICK_ROWS {
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!((row.kind, row.kind.shape()), (kind, ValueShape::Int), "{id} の kind と形");
        assert_eq!(row.value, RuleValue::Int(value), "{id} の値");
        assert!(row.enabled, "{id} は既定で効く");
        assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), TICK_RULING, "{id} の裁定 id と裁定日");
        assert_eq!(int_row(&manifest, id), Ok(value), "{id} を整数の読み手で引ける");
        let same_kind = manifest.rows().iter().filter(|found| found.kind == kind).count();
        assert_eq!(same_kind, 1, "{id} の kind の行は 1 本");
    }
    let shared = manifest.rows().iter().filter(|row| row.ruling == TICK_RULING.0).count();
    assert_eq!(shared, 3, "同じ裁定 id の行はちょうど 3 本（母集団 {} 行）", manifest.rows().len());
}

/// 梯子の列の行が埋め込み manifest に形 List・裁定の値・enabled・裁定 id / 裁定日で 1 本在り、退役した係数と上限の行は無い
/// （設計 §10 形 5）。
#[test]
fn rules_embedded_manifest_declares_tick_ladder_row_and_retires_factor_and_max() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get(TICK_LADDER_ROW).unwrap_or_else(|| panic!("{TICK_LADDER_ROW} の行が在る"));
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::SeatPointerLadderS, ValueShape::List), "梯子の列の kind と形");
    assert_eq!(row.value, RuleValue::List(TICK_LADDER.map(str::to_owned).to_vec()), "梯子の列の値");
    assert!(row.enabled, "梯子の列は既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), TICK_RULING, "梯子の列の裁定 id と裁定日");
    let same_kind = manifest.rows().iter().filter(|found| found.kind == RuleKind::SeatPointerLadderS).count();
    assert_eq!(same_kind, 1, "梯子の列の kind の行は 1 本");
    for id in TICK_RETIRED_ROWS {
        assert!(manifest.get(id).is_none(), "{id} は退役（行が無い）");
    }
}

/// 3 kind は `ALL` に在って字面（variant 名）から引け、整数の 2 kind の形は Int だけ・梯子の列の kind の形は List だけ。退役した
/// 係数と上限の kind の字面と綴り違いの kind は読めない。
#[test]
fn rules_embedded_manifest_declares_tick_kinds_in_all_with_int_and_list_shapes() {
    let names = ["SeatTickIntervalS", "SeatTickStaleS"];
    for ((_, kind, _), name) in TICK_ROWS.iter().zip(names) {
        assert!(ALL.contains(kind), "{name} は ALL に在る");
        assert_eq!(kind.as_str(), name, "字面は variant 名");
        assert_eq!(RuleKind::parse(name), Some(*kind), "{name} を字面から引ける");
        let errors = rejected(&one_row(*kind, "\"forty\"")).expect("文字列の値の fixture が受理された");
        assert!(errors.join("\n").contains("形と合わない"), "{name} の形は Int だけ: {errors:?}");
    }
    let ladder = RuleKind::SeatPointerLadderS;
    assert!(ALL.contains(&ladder), "SeatPointerLadderS は ALL に在る");
    assert_eq!((ladder.as_str(), RuleKind::parse("SeatPointerLadderS")), ("SeatPointerLadderS", Some(ladder)), "字面から引ける");
    let errors = rejected(&one_row(ladder, "1800")).expect("整数の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "梯子の列の形は List だけ: {errors:?}");
    for retired in ["SeatPointerBackoffFactor", "SeatPointerBackoffMaxS", "SeatTickStale"] {
        assert_eq!(RuleKind::parse(retired), None, "{retired} は kind でない");
        let errors = rejected(&one_row_raw(retired, "2")).expect("未知の kind の fixture が受理された");
        assert!(errors.join("\n").contains("未知である"), "{retired} は読めない: {errors:?}");
    }
}

/// 退避の猶予の行（設計 seat-heartbeat.md §13 形 1・ADR-0071・`s2-07l.651`）が埋め込み manifest に id / kind / 形 Int / 値 1800 /
/// enabled / 裁定 id / 裁定日で 1 本在り、kind は `ALL` の `SeatPointerLadderS` の直後で字面から引け、形は Int だけ（base では
/// 行も kind も無い ＝ RED）。
#[test]
fn rules_embedded_manifest_declares_tick_move_grace_row_after_the_ladder() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let id = "seat.move_grace_s";
    let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::SeatMoveGraceS, ValueShape::Int), "{id} の kind と形");
    assert_eq!(row.value, RuleValue::Int(1800), "{id} の値（30 分）");
    assert!(row.enabled, "{id} は既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-26T06:29Z", "2026-09-26"), "{id} の裁定 id と裁定日");
    assert_eq!(int_row(&manifest, id), Ok(1800), "{id} を整数の読み手で引ける");
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == RuleKind::SeatMoveGraceS).count(), 1, "kind の行は 1 本");
    let at = ALL.iter().position(|kind| *kind == RuleKind::SeatPointerLadderS).expect("SeatPointerLadderS は ALL に在る");
    assert_eq!(ALL.get(at + 1), Some(&RuleKind::SeatMoveGraceS), "kind は SeatPointerLadderS の直後");
    assert_eq!(RuleKind::parse("SeatMoveGraceS"), Some(RuleKind::SeatMoveGraceS), "字面から引ける");
    let errors = rejected(&one_row(RuleKind::SeatMoveGraceS, "\"half\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

/// 重なる語列の移動（設計 vessel-hook.md §11 の形 f 4・user 裁定 2026-09-19T15:28Z）: runner.denied_commands は cargo の
/// 2 語列だけを持ち、host_guard.git は git の 7 語列を持ち、両方に同じ語列は無い（command guard と intake は ∪ で読むので
/// 語列が禁じられることは変わらない）。
#[test]
fn rules_moved_host_guard_git_sequences_leave_runner_denied_commands() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let list = |id: &str| match manifest.get(id).map(|row| row.value.clone()) {
        Some(RuleValue::List(items)) => items,
        other => panic!("{id} の値は列: {other:?}"),
    };
    let (runner, git) = (list("runner.denied_commands"), list("host_guard.git"));
    assert_eq!(runner, ["cargo mutants", "cargo publish"], "runner.denied_commands は cargo の 2 語列");
    let moved = ["git push --force", "git push -f", "git reset --hard", "git branch -D", "git clean -f", "git stash drop", "git stash clear"];
    assert_eq!(git, moved, "host_guard.git は git の 7 語列");
    let shared: Vec<&String> = runner.iter().filter(|item| git.contains(item)).collect();
    assert!(shared.is_empty(), "両方に在る語列は無い: {shared:?}");
    assert!(runner.iter().all(|item| item.starts_with("cargo ")), "runner に git の語列は残らない: {runner:?}");
}

/// 語を持たない語列は validate が断り（runner.denied_commands と同じ検査）、2 kind とも形は List だけ。
#[test]
fn rules_embedded_manifest_declares_host_guard_rows_refuse_blank_sequences() {
    let blank = one_row(RuleKind::HostGuardDeniedCommands, r#"["tmux kill-server", " "]"#);
    let errors = parsed(&blank).expect_err("語を持たない語列は不備");
    assert!(errors.contains("語を持たない語列"), "{errors}");
    assert!(parsed(&one_row(RuleKind::HostGuardDeniedCommands, r#"["tmux kill-server"]"#)).is_ok(), "語を持つ語列は通る");
    for kind in [RuleKind::HostGuardDeniedCommands, RuleKind::HostGuardRmProtected] {
        let errors = rejected(&one_row(kind, "16")).expect("整数の値の fixture が受理された");
        assert!(errors.join("\n").contains("形と合わない"), "{}: 形は List だけ: {errors:?}", kind.as_str());
    }
}

// ─── rm の守る集合の記号（設計 vessel-hook.md §11 行 c・`s2-07l.575`・接頭辞 `rules_host_guard_rm_symbol_`） ───

/// host_guard.rm の値の綴り違いの記号は読み込みで拒み（取る記号を名指す）、3 記号は全部受理される。
#[test]
fn rules_host_guard_rm_symbol_misspelled_is_refused_and_the_three_are_accepted() {
    let all: Vec<String> = PROTECTED.iter().map(|symbol| format!("\"{}\"", symbol.as_str())).collect();
    assert_eq!(all.len(), 3, "記号は 3 つ: {all:?}");
    let manifest = parsed(&one_row(RuleKind::HostGuardRmProtected, &format!("[{}]", all.join(", "))))
        .unwrap_or_else(|errors| panic!("3 記号は受理される: {errors}"));
    let row = manifest.get("probe").unwrap_or_else(|| panic!("probe が在る"));
    let want: Vec<String> = ["state-dir", "repo-tracked", "repo-git"].map(str::to_owned).to_vec();
    assert_eq!(row.value, RuleValue::List(want), "宣言順の 3 記号");
    for bad in ["state_dir", "repo-tracked ", "Repo-Git", "home"] {
        let errors = rejected(&one_row(RuleKind::HostGuardRmProtected, &format!("[\"repo-git\", \"{bad}\"]")))
            .unwrap_or_else(|rows| panic!("{bad:?} が受理された（{rows} 行）"));
        let text = errors.join("\n");
        assert!(text.contains(&format!("未知の守る集合の記号 {bad}")), "{bad:?}: {text}");
        assert!(text.contains("state-dir / repo-tracked / repo-git"), "取る記号を名指す: {text}");
    }
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
    // `.297` で `RunnerModel`・`.322` で `RunnerEffort` が末尾に足され、`.479.1` で席の自律の 7 つが消えたので、
    // size の 3 つは末尾から 3 つ目までの手前（末尾から 5 つ目まで）に並ぶ。位置は現物から数える。
    let at = ALL.iter().position(|kind| *kind == RuleKind::PipeSizeSLines).unwrap_or_default();
    let tail: Vec<RuleKind> = ALL.iter().skip(at).take(3).copied().collect();
    assert_eq!(tail, [RuleKind::PipeSizeSLines, RuleKind::PipeSizeMLines, RuleKind::PipeSizeLLines], "宣言順の末尾から 3 つ目までの 3 つ");
    let errors = rejected(&one_row_raw("PipeSizeXlLines", "1600")).expect("未知の kind の fixture が受理された");
    assert!(errors.join("\n").contains("未知である"), "4 段目の size は kind として読めない");
}

/// 口座選定の行（`R-C9-1`・裁定 id `user 2026-09-17T07:30Z`・設計 account-autonomy.md §3・rules-manifest.md §13）。値は
/// session 用の閾値（使用率の百分率）で形は `Int`。窓別の閾値が着地するまでの特例で 95（`s2-07l.447`・5 時間窓を 85 に
/// 戻すのは窓別の便）。**散文（Policy）の値を置いた行は形の不一致で拒まれる**（形は `RuleKind::shape` の 1 箇所が持つ）。
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
    assert_eq!(row.value, RuleValue::Int(95), "user 裁定 2026-09-17T07:30Z の値");
    assert_eq!(row.kind, RuleKind::AccountSelection, "kind は既存のまま");
    assert_eq!(row.kind.shape(), ValueShape::Int, "値の形は Int（百分率）");
    assert!(row.enabled, "発効している");
    assert_eq!(row.ruling, "user 2026-09-17T07:30Z", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-17", "裁定日");
    let errors = rejected(&one_row(RuleKind::AccountSelection, "\"新規投入は 5h 線\""))
        .expect("散文の値の fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("形と合わない"), "理由: {first}");
    assert!(first.contains("要 Int"), "要求する形を名指す: {first}");
    let healed = parsed(&one_row(RuleKind::AccountSelection, "85")).expect("Int の値は受理される");
    assert_eq!(healed.get("probe").map(|found| found.value.clone()), Some(RuleValue::Int(85)));
}

/// gate の費用の 6 行（設計 gate-cost.md §3.1・ADR-0021・tmux の歯の同時本数は `s2-07l.360`）。**値は manifest が
/// 持ち、ADR も設計 doc も写さない**（C1 / C5）。
///
/// kind の包含を 6 行まとめて測るのは、行と variant を**対で**足させるためである——片方だけ
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
    // (id, kind, 値, 裁定 id, 裁定日)。値は **user 2026-09-12 の裁定**（台帳 s2-07l.153 notes 逐語）と
    // `gate.tmux_test_threads` の **user 2026-09-15T21:09Z の裁定**（planner の推奨 1 への承認・`s2-07l.360`）と
    // `gate.slot_wait_s` の **user 2026-09-21T09:41Z の裁定**（900 → 5400・受付の待ちの上限を着地の列の上限と同じ値に）。
    let rows: [(&str, RuleKind, u64, &str, &str); 6] = [
        ("gate.mutants_jobs", RuleKind::GateMutantsJobs, 4, "user 2026-09-12T11:42Z", "2026-09-12"),
        ("gate.job_memory_mb", RuleKind::GateJobMemoryMb, 3072, "user 2026-09-12T12:08Z", "2026-09-12"),
        ("host.reserve_memory_mb", RuleKind::HostReserveMemoryMb, 8192, "user 2026-09-12T12:08Z", "2026-09-12"),
        ("gate.slot_wait_s", RuleKind::GateSlotWaitS, 5400, "user 2026-09-21T09:41Z", "2026-09-21"),
        ("gate.tmux_test_threads", RuleKind::GateTmuxTestThreads, 1, "user 2026-09-15T21:09Z", "2026-09-15"),
        ("gate.cpu_weight", RuleKind::GateCpuWeight, 50, "user 2026-09-12T12:08Z", "2026-09-12"),
    ];
    for (id, kind, value, ruling, ruled_at) in rows {
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!(row.value, RuleValue::Int(value), "{id} の値");
        assert_eq!(row.kind, kind, "{id} の kind");
        assert_eq!(row.kind.shape(), ValueShape::Int, "{id} の値の形");
        assert!(row.enabled, "{id} は既定で効く");
        assert_eq!(row.ruling, ruling, "{id} の裁定 id");
        assert_eq!(row.ruled_at, ruled_at, "{id} の裁定日");
        // kind の字面は閉じた enum を通してしか解けない（綴り違いの manifest は読めない）。
        assert_eq!(RuleKind::parse(kind.as_str()), Some(kind), "{id} の kind を字面から引ける");
    }
}

/// 器の健康の遮断器の倍率 2 行（設計 gate-cost.md §32 約束 2・契約表の行 x）: 埋め込みの manifest が
/// `host.runnable_per_core` = 4・`host.blocked_per_core` = 1 を**裁定 id `user 2026-09-20T15:23Z`・裁定日 2026-09-20** つきで
/// 持ち、kind は Int の `HostRunnablePerCore` / `HostBlockedPerCore`。kind の包含で**行と variant を対で足させる**——片方だけの
/// manifest は parse できず（未知の kind）、片方だけの enum は親 test の `covers_all_kinds` が落とす（行 o と同型）。
#[test]
fn rules_embedded_manifest_declares_host_health_per_core_rows_with_the_ruling() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let rows: [(&str, RuleKind, u64); 2] = [
        ("host.runnable_per_core", RuleKind::HostRunnablePerCore, 4),
        ("host.blocked_per_core", RuleKind::HostBlockedPerCore, 1),
    ];
    for (id, kind, value) in rows {
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!(row.value, RuleValue::Int(value), "{id} の値（user 裁定の倍率）");
        assert_eq!(row.kind, kind, "{id} の kind");
        assert_eq!(row.kind.shape(), ValueShape::Int, "{id} の値の形は Int（倍率）");
        assert!(row.enabled, "{id} は既定で効く");
        assert_eq!(row.ruling, "user 2026-09-20T15:23Z", "{id} の裁定 id");
        assert_eq!(row.ruled_at, "2026-09-20", "{id} の裁定日");
        assert_eq!(RuleKind::parse(kind.as_str()), Some(kind), "{id} の kind を字面から引ける");
        assert!(ALL.contains(&kind), "{id} の kind は ALL に在る");
    }
    let shared = manifest.rows().iter().filter(|row| row.ruling == "user 2026-09-20T15:23Z").count();
    assert_eq!(shared, 2, "同じ裁定で決めた 2 行だけが裁定 id を持つ（他の行と相乗りしない）");
    let errors = rejected(&one_row_raw("HostRunnablePerCores", "4")).expect("未知の kind の fixture が受理された");
    assert!(errors.join("\n").contains("未知である"), "綴り違いの kind は読めない: {errors:?}");
    let errors = rejected(&one_row(RuleKind::HostBlockedPerCore, "\"one\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

/// 便ごとの token 消費の検出線の行（設計 gate-cost.md §43 歯 (c)・契約表の行 aj・`s2-07l.172`）: 埋め込みの manifest が
/// `R-C6-1` = 25000000 を**裁定 id `user 2026-09-23T07:16Z`・裁定日 2026-09-23** つきで発効して持ち、整数の読み手で値が
/// 取れ、kind は Int の新しい variant 1 つで `ALL` の `GateTokenCap` の直後（同じ token の上限の族）に在って字面から引ける。
#[test]
fn rules_embedded_manifest_declares_run_token_ceiling_row_with_its_ruling() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get("R-C6-1").unwrap_or_else(|| panic!("R-C6-1 の行が在る"));
    assert_eq!(row.value, RuleValue::Int(25_000_000), "値（user 裁定の総 token）");
    assert_eq!(row.kind, RuleKind::RunTokenCeiling, "kind");
    assert_eq!(row.kind.shape(), ValueShape::Int, "値の形は Int（token）");
    assert!(row.enabled, "発効（検出線の読み手を効かせる）");
    assert_eq!(row.ruling, "user 2026-09-23T07:16Z", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-23", "裁定日");
    assert_eq!(int_row(&manifest, "R-C6-1"), Ok(25_000_000), "整数の読み手で 25000000 が取れる");
    assert_eq!(RuleKind::parse("RunTokenCeiling"), Some(RuleKind::RunTokenCeiling), "kind を字面から引ける");
    let at = ALL.iter().position(|kind| *kind == RuleKind::GateTokenCap).expect("GateTokenCap は ALL に在る");
    assert_eq!(ALL.get(at.saturating_add(1)), Some(&RuleKind::RunTokenCeiling), "宣言順は GateTokenCap の直後");
    let shared = manifest.rows().iter().filter(|found| found.kind == RuleKind::RunTokenCeiling).count();
    assert_eq!(shared, 1, "kind の行は 1 本");
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
    assert_eq!(manifest.rows().len(), 70, "埋め込み manifest の行数（母集団・`.651` で +1〔退避の猶予〕・`.637` で -2 +1〔梯子の係数と上限の行の退役と梯子の列の行〕・`.600` で +1〔境界 crate の上限 R-C4-5〕・`.601` で +1〔クラスの語列表〕・`.582` で +4〔管理 tick の行 4 本〕・`.172` で +1〔便ごとの token 消費の検出線 R-C6-1〕・`.574` で +4〔host の見張りの種類ごとの行〕・`.169` で +1〔台帳 write の断る形〕・`.170` で +2〔flip の免除経路の面と札の上限〕・`.217` で +2・`.249` で +3・`.254` で +1・`.168` で +1・`.297` で +1・`.315` で +1・`.322` で +1・`.360` で +1・`.423` で +2・`.478` で -1〔役割の行 2 本が 1 本〕・`.479.1` で -7〔席の自律の行〕・`.479.2` で -3〔作業記憶の行 1 本と棚卸しの行 2 本〕・`.382` で +1〔終端の CI の上限〕・`.433` で +2〔役割の既定の model と effort〕・`.407` で +1〔選定の前計測の鮮度〕・`.396` で +1〔同型の審査 FAIL の停止の回数〕・`.504` の行 x で +2〔器の健康の遮断器の倍率〕・`.428` で +1〔着地の列の上限〕・`.398` で +1〔同時本数の最大値〕・`.491.3` で +3〔群の逼迫の閾値の窓ごとの行〕）");
    for kind in ALL {
        let covered = manifest.rows().iter().any(|row| row.kind == *kind);
        assert!(covered, "{} の行が manifest に無い", kind.as_str());
    }
}

/// 作業記憶の行（`seat.wm_directive_cap`）と台帳の棚卸しの行 2 本（`ledger.memo_stale_days` /
/// `ledger.memo_stale_priority`）は **もう無い**（ADR-0045 §2 (2)・`s2-07l.479.2`）: 読み手（退避と
/// rebrief の triage）が消えたので行も kind も消える（C10.3: 配線の無い設定を残さない）。
///
/// **消えたことを測る歯**である（base では 3 行とも在り kind も引けるので RED）。
#[test]
fn rules_embedded_manifest_drops_the_working_memory_rows() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    for id in ["seat.wm_directive_cap", "ledger.memo_stale_days", "ledger.memo_stale_priority"] {
        assert!(manifest.get(id).is_none(), "{id} の行は残らない");
    }
    for name in ["WmDirectiveCap", "MemoStaleDays", "MemoStalePriority"] {
        assert_eq!(RuleKind::parse(name), None, "{name} は kind の字面から引けない");
        assert!(!ALL.iter().any(|kind| kind.as_str() == name), "{name} は ALL に無い");
    }
    // 台帳の待ち上限（席の指示文の `{ledger}` の読み手）は残る＝「全部消した」ではないことを同時に測る。
    let kept = manifest.get("seat.ledger_timeout_s").expect("台帳の待ち上限の行は残る");
    assert_eq!(kept.kind, RuleKind::LedgerTimeoutS, "kind");
    assert!(kept.enabled, "既定で効く");
}

#[test]
fn rules_cli_get_returns_value() {
    let args = ["get".to_owned(), "R-C4-1".to_owned()];
    let outcome = vessel::rules::cli::dispatch(&args);
    assert_eq!(outcome.rc, RC_OK, "rc: {outcome:?}");
    assert_eq!(outcome.out, vec!["60000".to_owned()], "値の行（裁定 id user 2026-09-15T11:2xZ）");
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

/// 役割ごとの権能の行（`role.<役割名>`・`RuleKind::RoleCapabilities`・設計 seat-roles.md §3・ADR-0022 §2.2・
/// ADR-0045 §2 (1)・`s2-07l.201`）: **行は `Role::ALL` と同数**（役割ごとに 1 行＝席は orchestrator の 1 つなので
/// 1 行）、値は `Capability` の名の列、宣言順の末尾の kind。**値は manifest が持ち、設計 doc は写さない**（C1 / C5）。
/// 行が持つ裁定 id と裁定日: 便を止める権能 `stop` を席に与えた裁定 `user 2026-09-20`（ADR-0048・役割を 1 つにした
/// 裁定 `user 2026-09-18T08:3xZ` の値に `stop` を 1 語足した）。
fn role_row_ruling(role: Role) -> (&'static str, &'static str) {
    match role {
        Role::Orchestrator => (STOP_RULING, STOP_RULED_AT),
    }
}

/// 便を止める権能を席に与えた裁定 id（ADR-0048 §2・`s2-07l.495`）。
const STOP_RULING: &str = "user 2026-09-20";

/// [`STOP_RULING`] の裁定日。
const STOP_RULED_AT: &str = "2026-09-20";

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
        let (ruling, ruled_at) = role_row_ruling(*role);
        assert_eq!(row.ruling, ruling, "{id} の裁定 id（役割を orchestrator 1 つにした裁定）");
        assert_eq!(row.ruled_at, ruled_at, "{id} の裁定日");
        let names = role_row_names(&manifest, *role);
        assert!(!names.is_empty(), "{id} の値は非空の列");
        for name in &names {
            assert!(Capability::parse(name).is_some(), "{id} の値 {name} は Capability の名");
        }
    }
    assert!(ALL.contains(&RuleKind::RoleCapabilities), "ALL に在る（末尾は `.601` のクラスの語列表）");
    assert_eq!(RuleKind::parse("RoleCapabilities"), Some(RuleKind::RoleCapabilities), "kind を字面から引ける");
    let kinds = ALL.len();
    assert_eq!(kinds, 68, "kind の母集団（`.651` で +1〔退避の猶予〕・`.637` で -2 +1〔梯子の係数と上限の 2 種の退役と梯子の列の 1 種〕・`.600` で +1〔境界 crate の上限〕・`.601` で +1〔クラスの語列表〕・`.582` で +4〔管理 tick の 4 種〕・`.172` で +1〔便ごとの token 消費の検出線〕・`.574` で +2〔host の見張りの語列と rm の守る集合〕・`.169` で +1〔台帳 write の断る形〕・`.170` で +2〔flip の免除経路の面と札の上限〕・`.201` で +1・`.217` で +2・`.249` で +3・`.254` で +1・`.168` で +1・`.297` で +1・`.315` で +1・`.322` で +1・`.360` で +1・`.423` で +2・`.479.2` で -3・`.382` で +1〔終端の CI の上限〕・`.433` で +2〔役割の既定の 2 種〕・`.407` で +1〔選定の前計測の鮮度〕・`.396` で +1〔同型の審査 FAIL の停止の回数〕・`.504` の行 x で +2〔器の健康の遮断器の倍率〕・`.428` で +1〔着地の列の上限〕・`.398` で +1〔同時本数の最大値〕・`.491.3` で +3〔群の逼迫の閾値の 3 種〕）");
}

/// 禁じる語列の行（`runner.denied_commands`・`RuleKind::RunnerDeniedCommands`・裁定 id `user 2026-09-14`・ADR-0025 §2.1・
/// 設計 rules-manifest.md / vessel-hook.md §5・`s2-07l.168`）: **値は manifest が持つ**（C1 / C5）＝初期値 9 語列を名指す。
/// kind は宣言順で `RunnerAllowedCommands` の直後（対で読む行）・値の形は List・各要素は語を 1 つ以上持つ（空白だけの
/// 語列は validate が断る）。
#[test]
fn rules_embedded_manifest_declares_the_denied_commands_row() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get("runner.denied_commands").expect("禁じる語列の行が在る");
    let want: Vec<String> = ["cargo mutants", "cargo publish"].iter().map(|item| (*item).to_owned()).collect();
    assert_eq!(row.value, RuleValue::List(want), "cargo の 2 語列（git の 7 語列は host_guard.git へ移した・user 裁定 2026-09-19T15:28Z）");
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

/// クラスの語列表の行（`runner.class_commands`・`RuleKind::RunnerClassCommands`・設計 contract-source.md §48 の 2・ADR-0061・
/// `s2-07l.601`）: **値は manifest が持つ**（C1 / C5）＝裁定の 3 要素をこの順で持ち（consume は要素なし）、行の裁定 id と裁定日は
/// 裁定の値。kind は `ALL` の末尾・形は List。各要素は崩れ (a)〜(d) に当たらない（要素の読み手 1 本で名 + 語列に分かれ、語列の
/// 先頭語は上限の行の値に在り、語列は受付が読む禁じる語列の和集合のどれも含まない）。
#[test]
fn class_derive_embedded_row_carries_the_ruled_three_elements_and_ruling_id() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get(CLASS_ROW).unwrap_or_else(|| panic!("{CLASS_ROW} の行が在る"));
    let want: Vec<String> =
        ["publish git push", "delete git push --delete", "delete git push -d"].iter().map(|item| (*item).to_owned()).collect();
    assert_eq!(row.value, RuleValue::List(want.clone()), "裁定の 3 要素・この順（consume は要素なし）");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-24T00:13Z", "2026-09-24"), "裁定 id と裁定日");
    assert!(row.enabled, "既定で効く");
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::RunnerClassCommands, ValueShape::List), "kind と形");
    assert_eq!(ALL.last(), Some(&RuleKind::RunnerClassCommands), "kind は ALL の末尾");
    assert_eq!(RuleKind::parse("RunnerClassCommands"), Some(RuleKind::RunnerClassCommands), "kind を字面から引ける");
    let allowed = match manifest.get("runner.allowed_commands").map(|found| &found.value) {
        Some(RuleValue::List(commands)) => commands.clone(),
        other => panic!("上限の行は列: {other:?}"),
    };
    let denied: Vec<String> = denied_of(&manifest)
        .unwrap_or_else(|| panic!("禁じる語列の 4 行が揃う"))
        .into_iter()
        .flat_map(|(_, sequences)| sequences)
        .collect();
    let classes: Vec<Class> = want
        .iter()
        .map(|element| {
            let ClassElement::Pair(class, sequence) = class_element(element) else {
                panic!("{element:?} は名 + 語列（(a)(b) に当たらない）");
            };
            let head = sequence.split_whitespace().next().unwrap_or_default();
            assert!(allowed.iter().any(|command| command == head), "{element:?} の先頭語は上限の内（(c)）: {allowed:?}");
            assert_eq!(denied_in(&sequence, &denied), None, "{element:?} は禁じる語列を含まない（(d)）");
            class
        })
        .collect();
    assert_eq!(classes, [Class::Publish, Class::Delete, Class::Delete], "要素のクラス（consume は無い）");
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

/// 境界 crate の src の本体の上限の行（`R-C4-5`・`RuleKind::BoundaryLines`・裁定 id `user 2026-09-15T10:07Z`・設計
/// core-boundary.md §9 行 i・ADR-0062）。**値は manifest が持つ**（C1 / C5）＝行 i の便の base で測った本体 263 行
/// （幅 120）の 1.2 倍の切り上げ。kind は宣言順で `LineWidth` の直後（R-C4 の行の並び）・値の形は Int。裁定 id は他の
/// 行と相乗りしない（rules-diff）。
#[test]
fn rules_embedded_manifest_declares_boundary_lines_row_with_its_ruling() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get("R-C4-5").expect("境界 crate の上限の行が在る");
    assert_eq!(row.value, RuleValue::Int(316), "base の本体 263 行 × 1.2 の切り上げ");
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::BoundaryLines, ValueShape::Int), "kind と形");
    assert!(row.enabled, "既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-15T10:07Z", "2026-09-15"), "裁定 id と裁定日");
    let shared = manifest.rows().iter().filter(|found| found.ruling == row.ruling).count();
    assert_eq!(shared, 1, "裁定 id は他の行と相乗りしない");
    assert_eq!(RuleKind::parse("BoundaryLines"), Some(RuleKind::BoundaryLines), "kind を字面から引ける");
    let at = ALL.iter().position(|kind| *kind == RuleKind::BoundaryLines);
    let width = ALL.iter().position(|kind| *kind == RuleKind::LineWidth);
    assert_eq!(at, width.map(|found| found + 1), "宣言順は LineWidth の直後");
}

/// 埋め込み manifest の `role.<役割名>` の行の値（名の列・行が無い・列でない周は空＝呼び側の assert が落とす）。
fn role_row_names(manifest: &Manifest, role: Role) -> Vec<String> {
    match manifest.get(&format!("role.{}", role.as_str())).map(|row| row.value.clone()) {
        Some(RuleValue::List(names)) => names,
        _ => Vec::new(),
    }
}

/// 裁定 `user 2026-09-18T08:3xZ`（ADR-0045 §2 (1)）の値に裁定 `user 2026-09-20`（ADR-0048 §2）が `stop` を足した:
/// 席は orchestrator 1 つで、記帳（回答・承認・go）と便 1 本を名指す停止と契約・`design-intent/`・設計 doc・repo の
/// 外の編集と歯（`crates/<crate>/tests/`）の編集を持つ。
/// **便の起動と着地（launch / merge）は器の dispatcher の口ゆえ持たず、src の編集（edit-code）も持たない**
/// （印で開いた便の write-set だけ・AC16）。
#[test]
fn rules_embedded_manifest_role_rows_carry_the_ruled_capabilities() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let held = |role: Role, cap: Capability| role_row_names(&manifest, role).iter().any(|name| name == cap.as_str());
    for cap in [
        Capability::Answer,
        Capability::Approve,
        Capability::Go,
        Capability::Stop,
        Capability::EditContract,
        Capability::EditDesignIntent,
        Capability::EditDesignDoc,
        Capability::EditTests,
        Capability::EditOutside,
    ] {
        assert!(held(Role::Orchestrator, cap), "orchestrator は {} を持つ", cap.as_str());
    }
    for cap in [Capability::Launch, Capability::Merge, Capability::EditCode] {
        assert!(
            !held(Role::Orchestrator, cap),
            "orchestrator は {} を持たない（起動と着地は dispatcher・src は便の write-set）",
            cap.as_str()
        );
    }
}

/// 役割の権能の行は **1 本**（`role.orchestrator`）で、値は裁定 user 2026-09-18T08:3xZ / 09:0xZ
/// （ADR-0045 §2 (1)）の権能の列に裁定 user 2026-09-20（ADR-0048 §2）が `stop` を足したものである。
/// **id と名は字面で pin する**（`Role` / `Capability` の読み手を通さない独立の pin）。
///
/// 人と話す席として記帳（回答・承認・go）と便 1 本を名指す停止（stop）と契約・`design-intent/`・設計 doc・
/// repo の外を持ち、歯（`crates/*/tests/`）を書ける。**便の起動・着地・merge（launch / merge）は器の
/// dispatcher の口ゆえ行に無く、src の編集（edit-code）も中継（relay）も無い**。
#[test]
fn rules_embedded_manifest_role_row_is_one_orchestrator_row() {
    const ORCHESTRATOR_ROW: &str = "role.orchestrator";
    const EXPECTED: &[&str] = &[
        "answer",
        "approve",
        "go",
        "stop",
        "edit-contract",
        "edit-design-intent",
        "edit-design-doc",
        "edit-tests",
        "edit-outside",
    ];
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let rows: Vec<&str> = manifest
        .rows()
        .iter()
        .filter(|row| row.kind == RuleKind::RoleCapabilities)
        .map(|row| row.id.as_str())
        .collect();
    assert_eq!(rows, vec![ORCHESTRATOR_ROW], "役割の権能の行は 1 本だけ");
    let row = manifest.get(ORCHESTRATOR_ROW).unwrap_or_else(|| panic!("{ORCHESTRATOR_ROW} の行が在る"));
    let names = match &row.value {
        RuleValue::List(found) => found.clone(),
        other => panic!("{ORCHESTRATOR_ROW} の値は名の列: {other:?}"),
    };
    assert_eq!(names, EXPECTED, "{ORCHESTRATOR_ROW} の値（宣言順）");
    assert_eq!(row.ruling, STOP_RULING, "{ORCHESTRATOR_ROW} の裁定 id");
}

/// 約束 1（設計 seat-roles.md §25・ADR-0048 §2・`s2-07l.495`）: 権能の閉じた列と全 variant の列に `stop` が 1 つ在り
/// （字面は `stop`・宣言順は `merge` の直後）、rules 行の loader は `stop` を知っている名として受け、知らない名
/// （`stopp`・variant 名の字面 `Stop`）は今までどおり `RuleError` で拒む（取る名の列に `stop` を名指す）。
#[test]
fn rules_role_stop_is_a_capability_name_the_loader_accepts() {
    assert_eq!(Capability::Stop.as_str(), "stop", "行と記録の字面");
    assert_eq!(Capability::parse("stop"), Some(Capability::Stop), "字面から引ける");
    assert_eq!(CAPABILITIES.iter().filter(|cap| **cap == Capability::Stop).count(), 1, "全 variant の列に 1 つ");
    let at = CAPABILITIES.iter().position(|cap| *cap == Capability::Stop);
    let merge = CAPABILITIES.iter().position(|cap| *cap == Capability::Merge);
    assert_eq!(at, merge.map(|found| found + 1), "宣言順は merge の直後");
    assert!(Capability::Merge < Capability::Stop && Capability::Stop < Capability::EditContract, "判別子順");
    let accepted = parsed(&one_row(RuleKind::RoleCapabilities, r#"["answer", "stop"]"#)).expect("stop を持つ列は受理される");
    assert_eq!(
        accepted.get("probe").map(|row| row.value.clone()),
        Some(RuleValue::List(vec!["answer".to_owned(), "stop".to_owned()])),
        "書いた順のまま"
    );
    for (bad, word) in [(r#"["stopp"]"#, "stopp"), (r#"["Stop"]"#, "Stop"), (r#"["stop", "halt"]"#, "halt")] {
        let errors = rejected(&one_row(RuleKind::RoleCapabilities, bad)).expect("知らない名は受理されない");
        assert_eq!(errors.len(), 1, "{bad}: 件数: {errors:?}");
        let first = errors.first().map(String::as_str).unwrap_or_default();
        assert!(first.contains(&format!("未知の権能 {word}")), "{bad}: 理由: {first}");
        assert!(first.contains("stop"), "{bad}: 取る名の列に stop を名指す: {first}");
    }
}

/// 約束 2（§25・ADR-0048 §2）: 埋め込みの rules 行 `role.orchestrator` の値が `stop` を 1 つ持ち、裁定 id と裁定日が
/// 今回の裁定（`user 2026-09-20`）で、`launch` / `merge` は今までどおり無い（`--all` と名指しの無い停止と起動・着地は
/// 席から撃てないまま）。席の指示文（§5 の権能の行）に `stop` が出ることは `hook_brief_` の歯と外形 snapshot が測る。
#[test]
fn rules_role_stop_is_in_the_orchestrator_row_with_the_ruling() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get("role.orchestrator").expect("役割の行が在る");
    let names = role_row_names(&manifest, Role::Orchestrator);
    assert_eq!(names.iter().filter(|name| *name == "stop").count(), 1, "値に stop が 1 つ: {names:?}");
    assert!(!names.iter().any(|name| name == "launch" || name == "merge"), "起動と着地は無いまま: {names:?}");
    assert!(row.enabled, "発効している");
    assert_eq!(row.ruling, STOP_RULING, "裁定 id は今回の裁定");
    assert_eq!(row.ruled_at, STOP_RULED_AT, "裁定日");
    assert_ne!(row.ruling, "user 2026-09-18T08:3xZ", "前の裁定 id のままではない");
    let held = brief::capabilities_of(&manifest, Role::Orchestrator).expect("指示文の読み手も同じ行を読む");
    assert!(held.contains(&Capability::Stop), "指示文の権能の列に stop: {held:?}");
    assert!(!held.contains(&Capability::Launch), "{held:?}");
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

/// 役割ごとの既定の対の裁定 id（設計 seat-roles.md §19・逐語は台帳 `s2-07l.433`）。
const DEFAULTS_RULING: &str = "user 2026-09-17T04:23Z";

/// [`DEFAULTS_RULING`] の裁定日。
const DEFAULTS_RULED_AT: &str = "2026-09-17";

/// 歯 (a・設計 seat-roles.md §19 の形 1 / 2): 役割の閉じた列の**どの役割にも** model と effort の 2 行が在り、
/// kind と値の形（`Str`）と発効と裁定 id が一致する。**行の本数は役割の閉じた列の 2 倍**（母集団を同時に出す）。
/// id の前置きは `seat.model.` / `seat.effort.`（`role.` を避ける根は
/// [`rules_role_defaults_ids_avoid_the_capability_prefix`]）。
#[test]
fn rules_role_defaults_two_rows_per_role() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let count = |kind: RuleKind| manifest.rows().iter().filter(|row| row.kind == kind).count();
    assert_eq!(count(RuleKind::RoleModel), ROLES.len(), "RoleModel の行は役割ごとに 1 本（母集団 {}）", ROLES.len());
    assert_eq!(count(RuleKind::RoleEffort), ROLES.len(), "RoleEffort の行は役割ごとに 1 本（母集団 {}）", ROLES.len());
    let pair = count(RuleKind::RoleModel) + count(RuleKind::RoleEffort);
    assert_eq!(pair, ROLES.len() * 2, "既定の行は役割の閉じた列の 2 倍（母集団 {}）", ROLES.len());
    for role in ROLES {
        for (id, kind) in [
            (format!("seat.model.{}", role.as_str()), RuleKind::RoleModel),
            (format!("seat.effort.{}", role.as_str()), RuleKind::RoleEffort),
        ] {
            let row = manifest.get(&id).unwrap_or_else(|| panic!("{id} の行が在る"));
            assert_eq!(row.kind, kind, "{id} の kind");
            assert_eq!(row.kind.shape(), ValueShape::Str, "{id} の値の形は Str（閉じた表の字面）");
            assert!(row.enabled, "{id} は発効している");
            assert_eq!(row.ruling, DEFAULTS_RULING, "{id} の裁定 id");
            assert_eq!(row.ruled_at, DEFAULTS_RULED_AT, "{id} の裁定日");
            assert!(row.validate().is_ok(), "{id} は validate を通る");
        }
        // 権能の行は別の行のまま（id の完全一致・値の形も違う）。
        let caps = manifest.get(&format!("role.{}", role.as_str())).expect("権能の行が在る");
        assert_eq!(caps.kind, RuleKind::RoleCapabilities, "権能の行の kind は動かない");
    }
}

/// 歯 (a・続き): 既定の対の id は **`role.` で始まらない**。その前置きは権能の行（`role.<役割名>`・役割ごとに
/// 雛形を 1 枚ずつ要る行・設計 §5）の印で、xtask の seat-brief は `role.` の行を全部「雛形が要る役割の行」と
/// 数える＝既定の対に付けると雛形の無い役割として check が赤くなる。前置きを `role.` へ戻す変異はここで落ちる。
#[test]
fn rules_role_defaults_ids_avoid_the_capability_prefix() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let is_default = |kind: RuleKind| matches!(kind, RuleKind::RoleModel | RuleKind::RoleEffort);
    let defaults: Vec<&str> = manifest.rows().iter().filter(|row| is_default(row.kind)).map(|row| row.id.as_str()).collect();
    assert_eq!(defaults.len(), ROLES.len() * 2, "既定の行の母集団: {defaults:?}");
    for id in &defaults {
        assert!(!id.starts_with("role."), "{id} は role. で始まらない（雛形を要る行の前置き）");
        assert!(id.starts_with("seat.model.") || id.starts_with("seat.effort."), "{id} の前置き");
    }
    // `role.` で始まる行は権能の行**だけ**のまま（母集団は役割の閉じた列と同じ本数）。
    let prefixed: Vec<&str> = manifest.rows().iter().map(|row| row.id.as_str()).filter(|id| id.starts_with("role.")).collect();
    assert_eq!(prefixed.len(), ROLES.len(), "role. で始まる行は権能の行だけ（母集団 {}）: {prefixed:?}", ROLES.len());
    for id in prefixed {
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!(row.kind, RuleKind::RoleCapabilities, "{id} の kind は RoleCapabilities");
    }
}

/// 歯 (b・形 3・**否定の枝**): 値が閉じた表に無い manifest は**読み込みで拒まれる**（model 側・effort 側の
/// 2 例）。綴り違いを黙って「既定なし」に倒さない（NFR4）＝理由は行番号付きで取る名を名指す。表に在る
/// 字面（別名も表示名も）は今までどおり受理される。
#[test]
fn rules_role_defaults_reject_values_outside_the_closed_tables() {
    for (kind, bad, what, taken) in [
        (RuleKind::RoleModel, "\"opuss\"", "model", Model::Fable.alias()),
        (RuleKind::RoleEffort, "\"higher\"", "effort", Effort::Xhigh.alias()),
    ] {
        let errors = rejected(&one_row(kind, bad)).expect("表に無い値が受理された");
        assert_eq!(errors.len(), 1, "件数（{bad}）: {errors:?}");
        let first = errors.first().map(String::as_str).unwrap_or_default();
        assert!(first.contains(&format!("未知の{what}")), "{bad}: 理由: {first}");
        assert!(first.contains(taken), "{bad}: 取る名を名指す: {first}");
        assert!(first.contains("line=3"), "{bad}: 行番号: {first}");
    }
    // variant 名の字面と空文字も名ではない（`Model::parse` / `Effort::parse` は完全一致）。
    for (kind, bad) in [(RuleKind::RoleModel, "\"\""), (RuleKind::RoleEffort, "\"High\"")] {
        assert!(rejected(&one_row(kind, bad)).is_ok(), "{bad} は受理されない");
    }
    // 表に在る字面は全部通る（model は別名と表示名の両方・effort は字面）。
    for model in MODELS {
        for text in [model.alias(), model.display()] {
            let healed = parsed(&one_row(RuleKind::RoleModel, &format!("\"{text}\""))).expect("表の字面は受理される");
            assert_eq!(healed.get("probe").map(|row| row.value.clone()), Some(RuleValue::Str(text.to_owned())));
        }
    }
    for effort in EFFORTS {
        let text = effort.alias();
        let healed = parsed(&one_row(RuleKind::RoleEffort, &format!("\"{text}\""))).expect("表の字面は受理される");
        assert_eq!(healed.get("probe").map(|row| row.value.clone()), Some(RuleValue::Str(text.to_owned())));
    }
}

/// 歯 (d・形 6): 埋め込み manifest が役割の既定の 2 行を**値ごと**運ぶ（裁定 `user 2026-09-17T04:23Z` の
/// `fable` / `high`・値の正本は manifest で設計 doc は写さない・C1 / C5）。kind は宣言順で隣り合う 2 つ
/// （`RunnerEffort` の直後が `RoleModel`・その直後が `RoleEffort`・末尾は `.396` の `ReviewSameKindStop`）で、字面から引ける。
#[test]
fn rules_manifest_carries_role_defaults() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let model = manifest.get("seat.model.orchestrator").expect("既定の model の行が在る");
    assert_eq!(model.value, RuleValue::Str("fable".to_owned()), "裁定 user 2026-09-17T04:23Z の model");
    assert_eq!(Model::parse("fable"), Some(Model::Fable), "値は閉じた表で引ける");
    let effort = manifest.get("seat.effort.orchestrator").expect("既定の effort の行が在る");
    assert_eq!(effort.value, RuleValue::Str("high".to_owned()), "裁定 user 2026-09-17T04:23Z の effort");
    assert_eq!(Effort::parse("high"), Some(Effort::High), "値は閉じた表で引ける");
    let at = ALL.iter().position(|kind| *kind == RuleKind::RunnerEffort).unwrap_or_default();
    assert_eq!(ALL.get(at.saturating_add(1)), Some(&RuleKind::RoleModel), "宣言順は RunnerEffort の直後");
    assert_eq!(ALL.get(at.saturating_add(2)), Some(&RuleKind::RoleEffort), "対は宣言順で隣り合う");
    assert_eq!(ALL.get(at.saturating_add(3)), Some(&RuleKind::ReviewSameKindStop), "`.433` の 2 種の直後が `.396` の末尾");
    assert_eq!(RuleKind::parse("RoleModel"), Some(RuleKind::RoleModel), "kind を字面から引ける");
    assert_eq!(RuleKind::parse("RoleEffort"), Some(RuleKind::RoleEffort), "kind を字面から引ける");
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
    assert_eq!(row.value, RuleValue::Str(Role::Orchestrator.as_str().to_owned()), "値は orchestrator の席");
    assert!(row.enabled, "発効している");
    assert_eq!(row.ruling, "user 2026-09-18T08:3xZ", "裁定 id");
    assert_eq!(row.ruled_at, "2026-09-18", "裁定日");
    for bad in ["\"user-direct\"", "\"Orchestrator\"", "\"\""] {
        let errors = rejected(&one_row(RuleKind::DialogueSurface, bad)).expect("Role の名でない値は受理されない");
        assert_eq!(errors.len(), 1, "件数（{bad}）: {errors:?}");
        let first = errors.first().map(String::as_str).unwrap_or_default();
        assert!(first.contains("未知の役割"), "{bad}: 理由: {first}");
        assert!(first.contains("orchestrator"), "{bad}: 取る名を名指す: {first}");
    }
    for role in ROLES {
        let healed = parsed(&one_row(RuleKind::DialogueSurface, &format!("\"{}\"", role.as_str())))
            .expect("Role の名は受理される");
        assert_eq!(healed.get("probe").map(|row| row.value.clone()), Some(RuleValue::Str(role.as_str().to_owned())));
    }
}

/// (f) `runner.model`（runner / lens が claude に毎回渡す model・裁定 id `user 2026-09-14T21:59Z`・設計 pipeline.md §6・
/// `s2-07l.297`）: 埋め込み manifest の行は発効 ∧ `Str("opus")`（claude CLI の別名・閉じた表 `Model` で引ける）・kind は
/// 宣言順で `RunnerEffort` の直前 `RunnerModel`（形は `Str`・末尾は `.433` の `RoleEffort`）・`str_row` が同じ値を返し、
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
    let at = ALL.iter().position(|kind| *kind == RuleKind::RunnerModel).unwrap_or_default();
    assert_eq!(ALL.get(at.saturating_add(1)), Some(&RuleKind::RunnerEffort), "宣言順で `.322` の RunnerEffort が直後");
    assert_eq!(RuleKind::parse("RunnerModel"), Some(RuleKind::RunnerModel));
    let healed = parsed(&one_row(RuleKind::RunnerModel, "\"sonnet\"")).expect("文字列の値は受理される");
    assert_eq!(healed.get("probe").map(|row| row.value.clone()), Some(RuleValue::Str("sonnet".to_owned())));
    let errors = rejected(&one_row(RuleKind::RunnerModel, "5")).expect("整数の値は形が合わない");
    assert!(errors.join("\n").contains("形と合わない"), "{errors:?}");
}

/// 行の読み手 `str_row` / `int_row`（`rules::` の 1 本・headless と `pipe::ratelimit` が読む・`s2-07l.297`）: 発効した
/// 文字列 / 整数の行の値を返し、無い / 不発効 / 形違いは 3 理由の `Err`（lens の cap の字面と同じ）。
#[test]
fn rules_row_readers_return_the_value_or_one_of_three_reasons() {
    let manifest = Manifest::embedded().expect("埋め込み manifest を読める");
    assert_eq!(str_row(&manifest, "runner.model"), Ok("opus"), "文字列の行の読み手");
    assert_eq!(int_row(&manifest, "gate.token_cap"), Ok(150_000), "整数の行の読み手");
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

/// 戻しの行（rules-manifest §14・`s2-07l.376`）: 行 h（`s2-07l.375`）の一時の上げ 400000 を `s2-07l.209` の着地後に
/// 150000（SRS NFR1 の目標値・上げ前の値）へ戻す。裁定は行 h と同じ承認の時刻で始まり、戻しの便を名指す。
#[test]
fn rules_token_cap_revert_row_carries_target_value_and_ruling() {
    let manifest = Manifest::embedded().expect("埋め込み manifest を読める");
    let row = manifest.get("gate.token_cap").expect("gate.token_cap の行が在る");
    assert_eq!(row.kind, RuleKind::GateTokenCap, "kind");
    assert_eq!(row.value, RuleValue::Int(150_000), "値は SRS NFR1 の目標値（上げ前の値）");
    assert!(row.enabled, "発効している");
    assert!(row.ruling.starts_with("user 2026-09-15T23:31Z"), "裁定は行 h と同じ承認の時刻で始まる: {}", row.ruling);
    assert!(row.ruling.contains("s2-07l.376"), "裁定は戻しの便を名指す: {}", row.ruling);
    assert_eq!(int_row(&manifest, "gate.token_cap"), Ok(150_000), "上限の読み手が戻した値を返す");
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
