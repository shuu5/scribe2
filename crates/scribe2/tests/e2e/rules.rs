//! rules manifest の歯（設計 docs/design/rules-manifest.md §7）。
//!
//! fixture は文字列 literal で持つ（file を置くと歯が repo の状態に依存する）。

use crate::make_tmp_dir;
use std::process::Command;
use vessel::cli_outcome::{Outcome, RC_OK, RC_REFUSED};
use vessel::rules::manifest::Manifest;
use vessel::rules::{Rule, RuleKind, RuleValue, ValueShape, ALL};

/// 受理される最小の manifest（2 行）。
const GOOD: &str = r#"schema = 1

[[rule]]
id = "R-C4-1"
kind = "CoreLines"
value = 20000
ruling = "r"
ruled_at = "2026-09-07"

[[rule]]
id = "R-C7-1"
kind = "DialogueSurface"
value = "user-direct"
ruling = "r"
ruled_at = "2026-09-09"
"#;

/// 欠陥 3 箇所（10 行目 = id 重複 / 17 行目 = 未知 kind / 24 行目 = ruling 欠け）。
const DEFECTIVE: &str = r#"schema = 1

[[rule]]
id = "a"
kind = "CoreLines"
value = 1
ruling = "r"
ruled_at = "d"

[[rule]]
id = "a"
kind = "ModuleLines"
value = 2
ruling = "r"
ruled_at = "d"

[[rule]]
id = "b"
kind = "Nope"
value = 3
ruling = "r"
ruled_at = "d"

[[rule]]
id = "c"
kind = "FnLines"
value = 4
ruled_at = "d"
"#;

/// 1 行だけの fixture を組む。値は kind の形に合わせる。
fn one_row(kind: RuleKind, value: &str) -> String {
    format!(
        "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"{}\"\nvalue = {value}\nruling = \"r\"\nruled_at = \"2026-09-09\"\n",
        kind.as_str()
    )
}

/// 種類の形に合う値の字面。
fn sample_value(kind: RuleKind) -> String {
    match kind.shape() {
        ValueShape::Int => "1".to_owned(),
        ValueShape::Str | ValueShape::Policy => "\"sample\"".to_owned(),
    }
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
    assert!(row.enabled, "enabled 省略時は true");
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
        "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"{kind}\"\nvalue = {value}\nruling = \"r\"\nruled_at = \"2026-09-09\"\n"
    )
}

#[test]
fn rules_manifest_rejects_duplicate_id() {
    let text = "schema = 1\n\n[[rule]]\nid = \"same\"\nkind = \"CoreLines\"\nvalue = 1\nruling = \"r\"\nruled_at = \"d\"\n\n[[rule]]\nid = \"same\"\nkind = \"FnLines\"\nvalue = 2\nruling = \"r\"\nruled_at = \"d\"\n";
    let errors = rejected(text).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("重複"), "理由: {first}");
}

#[test]
fn rules_manifest_rejects_row_without_ruling() {
    let text = "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"CoreLines\"\nvalue = 1\nruled_at = \"d\"\n";
    let errors = rejected(text).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 1, "件数: {errors:?}");
    let first = errors.first().map(String::as_str).unwrap_or_default();
    assert!(first.contains("必須 key ruling"), "理由: {first}");
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
    let text = "[[rule]]\nid = \"probe\"\nkind = \"CoreLines\"\nvalue = 1\nruling = \"r\"\nruled_at = \"d\"\n";
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
    let text = "schema = 7\nschema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"GateTokenCap\"\nvalue = 1\nruling = \"r\"\nruled_at = \"d\"\n";
    let errors = rejected(text).expect("拒まれるはずの fixture が受理された");
    let joined = errors.join("\n");
    assert!(joined.contains("schema が重複する"), "理由: {joined}");
}

#[test]
fn rules_manifest_rejects_empty_id() {
    let text = "schema = 1\n\n[[rule]]\nid = \"\"\nkind = \"GateTokenCap\"\nvalue = 1\nruling = \"r\"\nruled_at = \"d\"\n";
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
    for want in ["line=10", "line=17", "line=24"] {
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

#[test]
fn rules_embedded_manifest_is_valid_and_covers_all_kinds() {
    let manifest = match Manifest::embedded() {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("埋め込み manifest が拒まれた:\n{}", lines.join("\n"))
        }
    };
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
