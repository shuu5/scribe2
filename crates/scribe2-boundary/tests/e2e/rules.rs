//! rules manifest の歯（設計 docs/design/rules-manifest.md §7）。
//!
//! fixture は文字列 literal で持つ（file を置くと歯が repo の状態に依存する）。
//!
//! 歯は族ごとの子 module に置く（設計 docs/design/carry-prep.md §9 行 j・`s2-07l.682`）: `embedded`（接頭辞
//! `rules_embedded_` / `rules_manifest_`）・`host`（接頭辞 `rules_host_` / `host_group_`）。この file には共有の
//! helper と const・外形 snapshot の歯（snapshot 名が module path を含むので動かさない）・他の族の歯だけを残す。

mod embedded;
mod host;
// flip-check: moved s2-07l.682

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
/// `Protected` の記号・`s2-07l.575`／クラスの語列表は `Class` の名 + 語列・`s2-07l.601`／公開の見張りは札つきの
/// `form` の記号・`s2-07l.696`）。
fn sample_value(kind: RuleKind) -> String {
    match kind {
        RuleKind::DialogueSurface => format!("\"{}\"", Role::Orchestrator.as_str()),
        RuleKind::RoleCapabilities => format!("[\"{}\"]", Capability::Answer.as_str()),
        RuleKind::RoleModel => format!("\"{}\"", Model::Fable.alias()),
        RuleKind::RoleEffort => format!("\"{}\"", Effort::High.alias()),
        RuleKind::HostGuardRmProtected => format!("[\"{}\"]", Protected::StateDir.as_str()),
        RuleKind::RunnerClassCommands => format!("[\"{} sample\"]", Class::Publish.as_str()),
        RuleKind::HostGuardPublish => "[\"form repo-name\"]".to_owned(),
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

/// kind の字面を直に差し込む fixture（未知 kind を作るため）。
fn one_row_raw(kind: &str, value: &str) -> String {
    format!(
        "schema = 1\n\n[[rule]]\nid = \"probe\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"2026-09-09\"\n"
    )
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
fn rules_kind_parity_every_kind_has_sample() {
    for kind in ALL {
        // 年齢の閾値の kind だけ id の後ろが語の内でなければ断られる（`lifecycle.age_h.<語>`・設計 case-lifecycle.md §12 約束 10）。
        let id = if *kind == RuleKind::LifecycleAgeH { "lifecycle.age_h.misfit" } else { "probe" };
        let text = one_row(*kind, &sample_value(*kind)).replace("\"probe\"", &format!("\"{id}\""));
        let manifest = parsed(&text).expect("受理されるはずの fixture が拒まれた");
        let row = manifest.get(id).expect("probe が在る");
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

// ─────────────────── host の面の `[[tick]]`（seat-heartbeat.md §5 形 1・契約表の行 d・接頭辞 `rules_host_tick_`） ───────────────────

/// 1 行の `[[tick]]` を持つ host の面（見出しは 6 行目・`unit-dir` は 7 行目・`binary` は 8 行目）。
const HOST_TICK: &str = "schema = 1\n\n[[account]]\nlabel = \"h1\"\n\n[[tick]]\nunit-dir = \"/srv/units\"\nbinary = \"/opt/bin/scribe2\"\n";

// ─────────────────── host の面の `[[device]]`（host-init.md §15・契約表の行 g・接頭辞 `rules_host_device_`） ───────────────────

/// 2 行の `[[device]]` を持つ host の面（1 行目は必須の欄だけ・見出しは 3 行目／2 行目は全部の欄・見出しは 9 行目）。
const HOST_DEVICE: &str = "schema = 1\n\n[[device]]\nname = \"win-1\"\nssh = \"me@win\"\nchrome = \"C:/Chrome/chrome.exe\"\nos = \"windows\"\n\n[[device]]\nname = \"mac-2\"\nssh = \"me@mac\"\nchrome = \"/Applications/Google Chrome.app\"\nos = \"macos\"\ndisplay = \":1\"\nime-env = [\"GTK_IM_MODULE=fcitx\", \"XMODIFIERS=@im=fcitx\"]\nprofile-dir = \"/tmp/prof\"\n";

/// 1 行の `[[device]]`（必須の欄だけ・見出しは 3 行目・name 4・ssh 5・chrome 6・os 7）。
const HOST_DEVICE_ONE: &str = "schema = 1\n\n[[device]]\nname = \"a\"\nssh = \"me@a\"\nchrome = \"/c\"\nos = \"linux\"\n";

/// 端末 1 台の欄を `|` で繋いだ 1 行（名・ssh・chrome・os・display・ime-env・profile-dir・見出し行）。
fn device_facts(device: &vessel::rules::device::Device) -> String {
    let env: Vec<String> = device.ime_env().iter().map(|(key, value)| format!("{key}={value}")).collect();
    format!(
        "{}|{}|{}|{}|{:?}|{env:?}|{:?}|{}",
        device.name(),
        device.ssh(),
        device.chrome(),
        device.os().as_str(),
        device.display(),
        device.profile_dir(),
        device.line()
    )
}

// ─────────────────── host の面が dir（在るが読めない・`s2-07l.250`・`.243` run 3 の生存変異を塞ぐ） ───────────────────
// flip-check: retroactive s2-07l.250

/// `S/host.toml` の位置に dir を置いた state dir（権限に依らず読めない＝「無い」に潰れたら縮退の側へ倒れる）。
fn host_dir_state() -> Option<TmpDir> {
    let dir = make_tmp_dir()?;
    std::fs::create_dir_all(dir.join(vessel::rules::HOST_MANIFEST)).ok()?;
    Some(dir)
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

/// 台帳 write の断る形の行（`ledger.denied_writes`・設計 vessel-hook.md §10・`s2-07l.169`・ledger-form.md §11）が kind・
/// 値の 6 語・enabled・裁定 id の 4 面で引ける。**値は manifest が持つ**（C1 / C5）。形は List だけで、kind の綴り違いは
/// 未知として読めない。
#[test]
fn rules_ledger_denied_writes_row_is_declared_on_four_faces() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get("ledger.denied_writes").expect("台帳 write の断る形の行が在る");
    let forms =
        ["notes-replace", "memory-subcommand", "create-without-parent", "bd-outside-bdw", "create-bypass", "parent-edge"];
    assert_eq!(row.value, RuleValue::List(forms.iter().map(|form| (*form).to_owned()).collect()), "裁定の値（閉じた 6 語）");
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::LedgerDeniedWrites, ValueShape::List), "kind と形");
    assert!(row.enabled, "既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-27T14:02Z 項 5", "2026-09-27"), "裁定 id と裁定日");
    assert!(ALL.contains(&RuleKind::LedgerDeniedWrites), "ALL に在る");
    assert_eq!(RuleKind::parse("LedgerDeniedWrites"), Some(RuleKind::LedgerDeniedWrites), "kind を字面から引ける");
    let errors = rejected(&one_row_raw("LedgerDeniedWrite", "[\"notes-replace\"]")).expect("未知の kind の fixture が受理された");
    assert!(errors.join("\n").contains("未知である"), "綴り違いの kind は読めない: {errors:?}");
    let errors = rejected(&one_row(RuleKind::LedgerDeniedWrites, "16")).expect("整数の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は List だけ: {errors:?}");
}

/// 台帳のグラフの直下の open の子の上限の行（設計 ledger-form.md §10 形 4・`s2-07l.719`）が埋め込み manifest に id / kind /
/// 形 Int / 値 15 / enabled / 裁定 id / 裁定日で 1 本在り、行は `ledger.denied_writes` の直後・kind は `ALL` の
/// `LedgerDeniedWrites` の直後で字面から引け、形は Int だけ（base では行も kind も無い ＝ RED）。
#[test]
fn rules_open_children_max_row_follows_the_denied_writes() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let id = "ledger.open_children_max";
    let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::LedgerOpenChildrenMax, ValueShape::Int), "{id} の kind と形");
    assert_eq!(row.value, RuleValue::Int(15), "{id} の値（本）");
    assert!(row.enabled, "{id} は既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-27T17:33Z 項 2-3", "2026-09-27"), "{id} の裁定 id と裁定日");
    assert_eq!(int_row(&manifest, id), Ok(15), "{id} を整数の読み手で引ける");
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == RuleKind::LedgerOpenChildrenMax).count(), 1, "kind の行は 1 本");
    let at = ALL.iter().position(|kind| *kind == RuleKind::LedgerDeniedWrites).expect("LedgerDeniedWrites は ALL に在る");
    assert_eq!(ALL.get(at + 1), Some(&RuleKind::LedgerOpenChildrenMax), "kind は LedgerDeniedWrites の直後");
    let rows: Vec<&str> = manifest.rows().iter().map(|found| found.id.as_str()).collect();
    let denied = rows.iter().position(|found| *found == "ledger.denied_writes").expect("ledger.denied_writes の行が在る");
    assert_eq!(rows.get(denied + 1).copied(), Some(id), "行も ledger.denied_writes の直後（母集団 {} 行）", rows.len());
    assert_eq!(RuleKind::parse("LedgerOpenChildrenMax"), Some(RuleKind::LedgerOpenChildrenMax), "字面から引ける");
    let errors = rejected(&one_row(RuleKind::LedgerOpenChildrenMax, "\"fifteen\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

// ─── host の破壊防止の見張りの種類ごとの行（設計 vessel-hook.md §11 行 b・ADR-0056・`s2-07l.574`・接頭辞
// `rules_embedded_manifest_declares_host_guard_`） ───

/// host の見張りの 4 行が持つ裁定 id と裁定日（user 裁定 2026-09-19T15:28Z・UTC・逐語は台帳）。
const HOST_GUARD_RULING: (&str, &str) = ("user 2026-09-19T15:28Z", "2026-09-19");

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

/// 席の箱の行（設計 account-lifecycle.md §30 形 1・ADR-0072・`s2-07l.627`・歯 (g)）が埋め込み manifest に id / kind / 形 Int /
/// 値 32768 / enabled / 裁定 id / 裁定日で 1 本在り、kind は `ALL` の `SeatMoveGraceS` の直後で字面から引け、形は Int だけ（base では
/// 行も kind も無い ＝ RED）。rows=71 kinds=69 は `rules_external_form` の snapshot が測る。
#[test]
fn rules_seat_box_row_follows_the_move_grace() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let id = "seat.memory_max_mb";
    let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::SeatMemoryMaxMb, ValueShape::Int), "{id} の kind と形");
    assert_eq!(row.value, RuleValue::Int(32768), "{id} の値（32 GiB）");
    assert!(row.enabled, "{id} は既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-26T08:28Z", "2026-09-26"), "{id} の裁定 id と裁定日");
    assert_eq!(int_row(&manifest, id), Ok(32768), "{id} を整数の読み手で引ける");
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == RuleKind::SeatMemoryMaxMb).count(), 1, "kind の行は 1 本");
    let at = ALL.iter().position(|kind| *kind == RuleKind::SeatMoveGraceS).expect("SeatMoveGraceS は ALL に在る");
    assert_eq!(ALL.get(at + 1), Some(&RuleKind::SeatMemoryMaxMb), "kind は SeatMoveGraceS の直後");
    let rows: Vec<&str> = manifest.rows().iter().map(|found| found.id.as_str()).collect();
    let grace = rows.iter().position(|found| *found == "seat.move_grace_s").expect("seat.move_grace_s の行が在る");
    assert_eq!(rows.get(grace + 1).copied(), Some(id), "行も seat.move_grace_s の直後（母集団 {} 行）", rows.len());
    assert_eq!(RuleKind::parse("SeatMemoryMaxMb"), Some(RuleKind::SeatMemoryMaxMb), "字面から引ける");
    let errors = rejected(&one_row(RuleKind::SeatMemoryMaxMb, "\"big\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

/// heartbeat の段の上げの行（設計 seat-heartbeat.md §17 形 1・`s2-07l.704`）が埋め込み manifest に id / kind / 形 Int / 値 900 /
/// enabled / 裁定 id / 裁定日で 1 本在り、行は `seat.memory_max_mb` の直後・kind は `ALL` の `SeatMemoryMaxMb` の直後で
/// `RunnerClassCommands` の前、字面から引け、形は Int だけ（base では行も kind も無い ＝ RED）。
#[test]
fn rules_idle_alarm_row_follows_the_seat_box() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let id = "seat.idle_alarm_s";
    let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::SeatIdleAlarmS, ValueShape::Int), "{id} の kind と形");
    assert_eq!(row.value, RuleValue::Int(900), "{id} の値（15 分）");
    assert!(row.enabled, "{id} は既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-27T14:02Z 項 4", "2026-09-27"), "{id} の裁定 id と裁定日");
    assert_eq!(int_row(&manifest, id), Ok(900), "{id} を整数の読み手で引ける");
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == RuleKind::SeatIdleAlarmS).count(), 1, "kind の行は 1 本");
    let at = ALL.iter().position(|kind| *kind == RuleKind::SeatMemoryMaxMb).expect("SeatMemoryMaxMb は ALL に在る");
    let after: Vec<RuleKind> = ALL.iter().skip(at + 1).take(2).copied().collect();
    // `.717` が直後に事前審査の束の段の上げの kind を足した（その後ろは RunnerClassCommands のまま）。
    assert_eq!(after, [RuleKind::SeatIdleAlarmS, RuleKind::SeatPrecheckAlarmS], "kind は SeatMemoryMaxMb の直後で SeatPrecheckAlarmS の前");
    let rows: Vec<&str> = manifest.rows().iter().map(|found| found.id.as_str()).collect();
    let boxed = rows.iter().position(|found| *found == "seat.memory_max_mb").expect("seat.memory_max_mb の行が在る");
    assert_eq!(rows.get(boxed + 1).copied(), Some(id), "行も seat.memory_max_mb の直後（母集団 {} 行）", rows.len());
    assert_eq!(RuleKind::parse("SeatIdleAlarmS"), Some(RuleKind::SeatIdleAlarmS), "字面から引ける");
    let errors = rejected(&one_row(RuleKind::SeatIdleAlarmS, "\"quarter\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

/// 事前審査の確定の束の段の上げの行（設計 dispatcher.md §27 形 4・行 y・`s2-07l.717`）が埋め込み manifest に id / kind / 形 Int /
/// 値 900 / enabled / 裁定 id / 裁定日で 1 本在り、行は `seat.idle_alarm_s` の直後・kind は `ALL` の `SeatIdleAlarmS` の直後で
/// `RunnerClassCommands` の前、字面から引け、形は Int だけ（base では行も kind も無い ＝ RED）。
#[test]
fn rules_precheck_alarm_row_follows_the_idle_alarm() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let id = "seat.precheck_alarm_s";
    let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
    let kind = RuleKind::parse("SeatPrecheckAlarmS").expect("字面から引ける");
    assert_eq!((kind.as_str(), kind.shape()), ("SeatPrecheckAlarmS", ValueShape::Int), "kind の字面と形");
    assert_eq!(row.kind, kind, "{id} の kind");
    assert_eq!(row.value, RuleValue::Int(900), "{id} の値（15 分）");
    assert!(row.enabled, "{id} は既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-27T17:33Z 項 2-1", "2026-09-27"), "{id} の裁定 id と裁定日");
    assert_eq!(int_row(&manifest, id), Ok(900), "{id} を整数の読み手で引ける");
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == kind).count(), 1, "kind の行は 1 本");
    let at = ALL.iter().position(|found| *found == RuleKind::SeatIdleAlarmS).expect("SeatIdleAlarmS は ALL に在る");
    let after: Vec<RuleKind> = ALL.iter().skip(at + 1).take(2).copied().collect();
    assert_eq!(after, [kind, RuleKind::RunnerClassCommands], "kind は SeatIdleAlarmS の直後で RunnerClassCommands の前");
    let rows: Vec<&str> = manifest.rows().iter().map(|found| found.id.as_str()).collect();
    let idle = rows.iter().position(|found| *found == "seat.idle_alarm_s").expect("seat.idle_alarm_s の行が在る");
    assert_eq!(rows.get(idle + 1).copied(), Some(id), "行も seat.idle_alarm_s の直後（母集団 {} 行）", rows.len());
    let errors = rejected(&one_row(kind, "\"quarter\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

/// 床の検査の待ちの上限の行（設計 dispatcher.md §34 約束 8・行 ai・`s2-07l.738.37.1`）が埋め込み manifest に id / kind / 形 Int / 値 600 /
/// enabled / 裁定 id / 裁定日で 1 本在り、kind は `ALL` の末尾から 3 つ目・行は manifest の末尾から 3 つ目（後ろは起草の置き場の量の
/// 上限と窓の 2 つ・`.736.30`）で、字面から引け、形は Int だけ。
#[test]
fn rules_floor_timeout_row_precedes_the_drafts_cap_rows() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let id = "floor.timeout_s";
    let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
    let kind = RuleKind::parse("FloorTimeoutS").expect("字面から引ける");
    assert_eq!((kind.as_str(), kind.shape()), ("FloorTimeoutS", ValueShape::Int), "kind の字面と形");
    assert_eq!(row.kind, kind, "{id} の kind");
    assert_eq!(row.value, RuleValue::Int(600), "{id} の値（10 分）");
    assert!(row.enabled, "{id} は既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-30T04:25Z", "2026-09-30"), "{id} の裁定 id と裁定日");
    assert_eq!(int_row(&manifest, id), Ok(600), "{id} を整数の読み手で引ける");
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == kind).count(), 1, "kind の行は 1 本");
    // 後ろの 2 kind・14 行は局面の出力（`lifecycle.closed_window_h` の 1 行と `lifecycle.age_h.<語>` の 13 行・`.738.38.7`）。
    // 行の予約の期限の 1 kind・1 行（`pipe.reserve_h`・`.736.33.17`）が直後に挟まる。
    let kinds: Vec<&RuleKind> = ALL.iter().rev().skip(5).take(4).collect();
    assert_eq!(kinds, [&RuleKind::SeatDraftsBusyS, &RuleKind::SeatDraftsCapMb, &RuleKind::PipeReserveH, &kind], "kind は ALL の末尾の 2 つの前から 4 つ目（母集団 {} 種）", ALL.len());
    let rows: Vec<&str> = manifest.rows().iter().rev().skip(17).take(4).map(|found| found.id.as_str()).collect();
    assert_eq!(rows, ["seat.drafts_busy_s", "seat.drafts_cap_mb", "pipe.reserve_h", id], "行は manifest の末尾の 14 行の前から 4 つ目（母集団 {} 行）", manifest.rows().len());
    let errors = rejected(&one_row(kind, "\"ten\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

/// 行の予約の期限の行（設計 row-review.md §7・行 f・`s2-07l.736.33.17`）が埋め込み manifest に id / kind / 形 Int / 値 24 / enabled / 裁定 id / 裁定日で
/// 1 本在り、kind は `ALL` の `FloorTimeoutS` の直後（`SeatDraftsCapMb` の前）・行も `floor.timeout_s` の直後で、字面から引け、形は Int だけ（base では行も kind も無い ＝ RED）。
#[test]
fn rules_reserve_hours_row_follows_the_floor_timeout_row() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let id = "pipe.reserve_h";
    let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
    let kind = RuleKind::parse("PipeReserveH").expect("字面から引ける");
    assert_eq!((kind.as_str(), kind.shape()), ("PipeReserveH", ValueShape::Int), "kind の字面と形");
    assert_eq!(row.kind, kind, "{id} の kind");
    assert_eq!(row.value, RuleValue::Int(24), "{id} の値（24 時間）");
    assert!(row.enabled, "{id} は既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-30T22:13Z 項 reserve", "2026-09-30"), "{id} の裁定 id と裁定日");
    assert_eq!(int_row(&manifest, id), Ok(24), "{id} を整数の読み手で引ける");
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == kind).count(), 1, "kind の行は 1 本");
    let at = ALL.iter().position(|found| *found == RuleKind::FloorTimeoutS).expect("FloorTimeoutS は ALL に在る");
    assert_eq!(ALL.get(at + 1..at + 3), Some(&[kind, RuleKind::SeatDraftsCapMb][..]), "kind は FloorTimeoutS の直後で SeatDraftsCapMb の前（母集団 {} 種）", ALL.len());
    let rows: Vec<&str> = manifest.rows().iter().map(|found| found.id.as_str()).collect();
    let floor = rows.iter().position(|found| *found == "floor.timeout_s").expect("floor.timeout_s の行が在る");
    assert_eq!(rows.get(floor + 1).copied(), Some(id), "行も floor.timeout_s の直後（母集団 {} 行）", rows.len());
    let errors = rejected(&one_row(kind, "\"day\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

/// 席の起草の置き場の量の上限と組み立て中の窓の 2 行（設計 dispatcher.md §39 形 8・行 an・ADR-0101・`s2-07l.736.30`）が埋め込み manifest に
/// id / kind / 形 Int / 値 102400 と 1800 / enabled / 裁定 id / 裁定日で 1 本ずつ在り、kind は `ALL` の末尾 2 つ・行は manifest の末尾 2 行で
/// この順、字面から引け、形は Int だけ（base では行も kind も無い ＝ RED）。
#[test]
fn rules_drafts_cap_rows_are_the_last_two_kinds_and_rows() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    for (id, name, kind, value) in [
        ("seat.drafts_cap_mb", "SeatDraftsCapMb", RuleKind::SeatDraftsCapMb, 102_400),
        ("seat.drafts_busy_s", "SeatDraftsBusyS", RuleKind::SeatDraftsBusyS, 1800),
    ] {
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!(RuleKind::parse(name), Some(kind), "{name} を字面から引ける");
        assert_eq!((kind.as_str(), kind.shape()), (name, ValueShape::Int), "kind の字面と形");
        assert_eq!(row.kind, kind, "{id} の kind");
        assert_eq!(row.value, RuleValue::Int(value), "{id} の値");
        assert!(row.enabled, "{id} は既定で効く");
        assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-30T07:18Z", "2026-09-30"), "{id} の裁定 id と裁定日");
        assert_eq!(int_row(&manifest, id), Ok(value), "{id} を整数の読み手で引ける");
        assert_eq!(manifest.rows().iter().filter(|found| found.kind == kind).count(), 1, "{id} の kind の行は 1 本");
        let errors = rejected(&one_row(kind, "\"big\"")).expect("文字列の値の fixture が受理された");
        assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
    }
    // 後ろの 2 kind・14 行は局面の出力（`.738.38.7`）。起草の置き場の 2 つはその直前に並ぶ。
    let tail: Vec<&RuleKind> = ALL.iter().rev().skip(5).take(2).rev().collect();
    assert_eq!(tail, [&RuleKind::SeatDraftsCapMb, &RuleKind::SeatDraftsBusyS], "kind は ALL の末尾の 2 つの前の 2 つ・この順（母集団 {} 種）", ALL.len());
    let rows: Vec<&str> = manifest.rows().iter().rev().skip(17).take(2).rev().map(|found| found.id.as_str()).collect();
    assert_eq!(rows, ["seat.drafts_cap_mb", "seat.drafts_busy_s"], "行は manifest の末尾の 14 行の前の 2 行・この順（母集団 {} 行）", manifest.rows().len());
}

/// 局面の出力の行（設計 case-lifecycle.md §12 約束 10・行 c・`s2-07l.738.38.7`）の裁定の字（base の行 `floor.timeout_s` の裁定 id に項を足した字）。
const LIFECYCLE_RULING: &str = "user 2026-09-30T04:25Z 項 lifecycle";

/// 局面の出力の 2 kind と 14 行が埋め込み manifest の末尾に在り（kind は `ALL` の末尾 2 つ・行は末尾の 14 行で窓の行が先）、値が窓 72 と
/// 年齢 2 / 4 / 24 / 72（語ごと）で、全部が Int・enabled・同じ裁定 id（base の行 `floor.timeout_s` の id と別の字）と裁定日を持つ（base では
/// 行も kind も無い ＝ RED）。
#[test]
fn rules_lifecycle_rows_carry_the_ruled_values_and_the_lifecycle_ruling() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let ages: [(&str, u64); 13] = [
        ("utterance-open", 2),
        ("run-landed-open", 2),
        ("contract-refused", 4),
        ("run-review-failed", 4),
        ("run-gate-failed", 4),
        ("run-stopped", 4),
        ("run-failed", 4),
        ("ruling-unreflected", 24),
        ("memo-actionable", 24),
        ("contract-queued", 24),
        ("row-unbeaded", 24),
        ("misfit", 24),
        ("epic-closable", 72),
    ];
    let mut expected: Vec<(String, RuleKind, u64)> = vec![("lifecycle.closed_window_h".to_owned(), RuleKind::LifecycleClosedWindowH, 72)];
    expected.extend(ages.iter().map(|(word, hours)| (format!("lifecycle.age_h.{word}"), RuleKind::LifecycleAgeH, *hours)));
    for (id, kind, hours) in &expected {
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!((row.kind, row.kind.shape()), (*kind, ValueShape::Int), "{id} の kind と形");
        assert_eq!(row.value, RuleValue::Int(*hours), "{id} の値");
        assert!(row.enabled, "{id} は既定で効く");
        assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), (LIFECYCLE_RULING, "2026-09-30"), "{id} の裁定 id と裁定日");
        assert_eq!(int_row(&manifest, id), Ok(*hours), "{id} を整数の読み手で引ける");
    }
    let floor = manifest.get("floor.timeout_s").expect("floor.timeout_s の行が在る");
    assert_ne!(floor.ruling, LIFECYCLE_RULING, "base の行の裁定 id を使い回さない（new-row-reuses-ruling）");
    assert_eq!(RuleKind::parse("LifecycleClosedWindowH"), Some(RuleKind::LifecycleClosedWindowH), "字面から引ける");
    assert_eq!(RuleKind::parse("LifecycleAgeH"), Some(RuleKind::LifecycleAgeH), "字面から引ける");
    let kinds: Vec<&RuleKind> = ALL.iter().rev().skip(3).take(2).rev().collect();
    assert_eq!(kinds, [&RuleKind::LifecycleClosedWindowH, &RuleKind::LifecycleAgeH], "kind は ALL の末尾 2 つ・この順（母集団 {} 種）", ALL.len());
    let rows: Vec<&str> = manifest.rows().iter().rev().skip(3).take(14).rev().map(|found| found.id.as_str()).collect();
    let want: Vec<&str> = expected.iter().map(|(id, _, _)| id.as_str()).collect();
    assert_eq!(rows, want, "行は manifest の末尾 14 行・この順（母集団 {} 行）", manifest.rows().len());
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == RuleKind::LifecycleAgeH).count(), 13, "年齢の行は 13 本");
}

/// `lifecycle.age_h.<語>` は id の後ろが手番 seat の局面の語のときだけ受理され（行の無い seat の語 `run-asking` は受理・手番が seat でない
/// 語と語の外は行番号つきで断る）、ほかの kind の行 id は縛らない。
#[test]
fn rules_lifecycle_rows_refuse_an_age_word_outside_the_seat_words() {
    let row = |id: &str| one_row(RuleKind::LifecycleAgeH, "4").replace("\"probe\"", &format!("\"{id}\""));
    parsed(&row("lifecycle.age_h.run-asking")).expect("手番が seat の語は受理される");
    for id in ["lifecycle.age_h.run-intake", "lifecycle.age_h.nope", "lifecycle.age_h.", "lifecycle.age_h.memo-waiting", "probe"] {
        let errors = rejected(&row(id)).unwrap_or_else(|rows| panic!("{id} が受理された（{rows} 行）"));
        let joined = errors.join("\n");
        assert!(joined.contains("手番 seat の局面の語でない"), "{id} の断りの字: {joined}");
        assert!(joined.contains("line="), "{id} の断りは行番号つき: {joined}");
    }
    parsed(&one_row(RuleKind::LifecycleClosedWindowH, "72")).expect("窓の行は id を縛らない");
}

/// 事前審査の先撃ちの 1 周の本数の行（設計 dispatcher.md §27 形 1・行 aa・`s2-07l.718`）が埋め込み manifest に id / kind / 形 Int /
/// 値 1 / enabled / 裁定 id / 裁定日で 1 本在り、行は `gate.lens_count` の直後・kind は `ALL` の `GateLensCount` の直後で
/// `GateTokenCap` の前、字面から引け、形は Int だけ（base では行も kind も無い ＝ RED）。
#[test]
fn rules_prelens_row_follows_the_lens_count() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let id = "pipe.precheck_lens_per_round";
    let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
    let kind = RuleKind::parse("PipePrecheckLensPerRound").expect("字面から引ける");
    assert_eq!((kind.as_str(), kind.shape()), ("PipePrecheckLensPerRound", ValueShape::Int), "kind の字面と形");
    assert_eq!(row.kind, kind, "{id} の kind");
    assert_eq!(row.value, RuleValue::Int(1), "{id} の値（1 周に 1 本）");
    assert!(row.enabled, "{id} は既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-27T17:33Z 項 2-2", "2026-09-27"), "{id} の裁定 id と裁定日");
    assert_eq!(int_row(&manifest, id), Ok(1), "{id} を整数の読み手で引ける");
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == kind).count(), 1, "kind の行は 1 本");
    let at = ALL.iter().position(|found| *found == RuleKind::GateLensCount).expect("GateLensCount は ALL に在る");
    let after: Vec<RuleKind> = ALL.iter().skip(at + 1).take(2).copied().collect();
    assert_eq!(after, [kind, RuleKind::GateTokenCap], "kind は GateLensCount の直後で GateTokenCap の前");
    let rows: Vec<&str> = manifest.rows().iter().map(|found| found.id.as_str()).collect();
    let lens = rows.iter().position(|found| *found == "gate.lens_count").expect("gate.lens_count の行が在る");
    assert_eq!(rows.get(lens + 1).copied(), Some(id), "行も gate.lens_count の直後（母集団 {} 行）", rows.len());
    let errors = rejected(&one_row(kind, "\"one\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

/// 終端の CI の照合の間隔の行（設計 contract-source.md §50 形 1・`s2-07l.694`）が埋め込み manifest に id / kind / 形 Int /
/// 値 30 / enabled / 裁定 id / 裁定日で 1 本在り、行は `pipe.ci_wait_s` の直後・kind は `ALL` の `PipeCiWaitS` の直後で字面から
/// 引け、形は Int だけ（base では行も kind も無い ＝ RED）。
#[test]
fn rules_ci_poll_row_follows_the_ci_wait() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let id = "pipe.ci_poll_s";
    let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::PipeCiPollS, ValueShape::Int), "{id} の kind と形");
    assert_eq!(row.value, RuleValue::Int(30), "{id} の値（30 秒）");
    assert!(row.enabled, "{id} は既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-27T11:14Z", "2026-09-27"), "{id} の裁定 id と裁定日");
    assert_eq!(int_row(&manifest, id), Ok(30), "{id} を整数の読み手で引ける");
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == RuleKind::PipeCiPollS).count(), 1, "kind の行は 1 本");
    let at = ALL.iter().position(|kind| *kind == RuleKind::PipeCiWaitS).expect("PipeCiWaitS は ALL に在る");
    assert_eq!(ALL.get(at + 1), Some(&RuleKind::PipeCiPollS), "kind は PipeCiWaitS の直後");
    let rows: Vec<&str> = manifest.rows().iter().map(|found| found.id.as_str()).collect();
    let wait = rows.iter().position(|found| *found == "pipe.ci_wait_s").expect("pipe.ci_wait_s の行が在る");
    assert_eq!(rows.get(wait + 1).copied(), Some(id), "行も pipe.ci_wait_s の直後（母集団 {} 行）", rows.len());
    assert_eq!(RuleKind::parse("PipeCiPollS"), Some(RuleKind::PipeCiPollS), "字面から引ける");
    let errors = rejected(&one_row(RuleKind::PipeCiPollS, "\"30\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
}

/// 席の起草の置き場の書きの線の行（設計 dispatcher.md §33 形 5・`s2-07l.736.25`）が埋め込み manifest に id / kind / 形 Int /
/// 値 6 / enabled / 裁定 id / 裁定日で 1 本在り、行は `pipe.ci_poll_s` の直後・kind は `ALL` の `PipeCiPollS` の直後で字面から
/// 引け、形は Int だけ（base では行も kind も無い ＝ RED）。
#[test]
fn rules_drafts_stale_row_follows_the_ci_poll() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let id = "seat.drafts_stale_h";
    let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::SeatDraftsStaleH, ValueShape::Int), "{id} の kind と形");
    assert_eq!(row.value, RuleValue::Int(6), "{id} の値（6 時間）");
    assert!(row.enabled, "{id} は既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-29T11:43Z", "2026-09-29"), "{id} の裁定 id と裁定日");
    assert_eq!(int_row(&manifest, id), Ok(6), "{id} を整数の読み手で引ける");
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == RuleKind::SeatDraftsStaleH).count(), 1, "kind の行は 1 本");
    let at = ALL.iter().position(|kind| *kind == RuleKind::PipeCiPollS).expect("PipeCiPollS は ALL に在る");
    assert_eq!(ALL.get(at + 1), Some(&RuleKind::SeatDraftsStaleH), "kind は PipeCiPollS の直後");
    let rows: Vec<&str> = manifest.rows().iter().map(|found| found.id.as_str()).collect();
    let poll = rows.iter().position(|found| *found == "pipe.ci_poll_s").expect("pipe.ci_poll_s の行が在る");
    assert_eq!(rows.get(poll + 1).copied(), Some(id), "行も pipe.ci_poll_s の直後（母集団 {} 行）", rows.len());
    assert_eq!(RuleKind::parse("SeatDraftsStaleH"), Some(RuleKind::SeatDraftsStaleH), "字面から引ける");
    let errors = rejected(&one_row(RuleKind::SeatDraftsStaleH, "\"6\"")).expect("文字列の値の fixture が受理された");
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

// ─── rm の守る集合の記号（設計 vessel-hook.md §11 行 c・`s2-07l.575`・接頭辞 `rules_host_guard_rm_symbol_`） ───

#[test]
fn rules_cli_get_returns_value() {
    let args = ["get".to_owned(), "R-C4-1".to_owned()];
    let outcome = vessel::rules::cli::dispatch(&args);
    assert_eq!(outcome.rc, RC_OK, "rc: {outcome:?}");
    assert_eq!(outcome.out, vec!["82000".to_owned()], "値の行（裁定 id user 2026-10-01T04:49Z）");
}

/// core の本体の上限の行 `R-C4-1`（設計 rules-manifest.md §21 行 r・user 裁定 2026-10-01T04:49Z・A2）が埋め込み manifest に
/// 値 82000 / kind `CoreLines` / enabled / 裁定 id / 裁定日で在り、整数の読み手が 82000 を返す（base は値 74000 と前の裁定 ＝ RED）。
#[test]
fn rules_core_lines_82000_raised_by_ruling() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let id = "R-C4-1";
    let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
    assert_eq!(row.kind, RuleKind::CoreLines, "{id} の kind");
    assert_eq!(row.value, RuleValue::Int(82_000), "{id} の値（行）");
    assert!(row.enabled, "{id} は発効している");
    assert!(row.ruling.starts_with("user 2026-10-01T04:49Z"), "{id} の裁定 id: {}", row.ruling);
    assert_eq!(row.ruled_at, "2026-10-01", "{id} の裁定日");
    assert_eq!(int_row(&manifest, id), Ok(82_000), "{id} を整数の読み手で引ける");
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
/// 行が持つ裁定 id と裁定日: 止まった終端を閉じる権能 `settle` を席に与えた裁定 `user 2026-09-29T21:53Z`（ADR-0097・
/// 便を止める権能 `stop` を足した裁定 `user 2026-09-20` の値に `settle` を 1 語足した）。
fn role_row_ruling(role: Role) -> (&'static str, &'static str) {
    match role {
        Role::Orchestrator => (ROW_RULING, ROW_RULED_AT),
    }
}

/// 権能の行の今の裁定 id（`settle` を席に与えた裁定・ADR-0097 §32・`s2-07l.737.18`）。
const ROW_RULING: &str = "user 2026-09-29T21:53Z";

/// [`ROW_RULING`] の裁定日。
const ROW_RULED_AT: &str = "2026-09-29";

/// クラスの語列表の行（`runner.class_commands`・`RuleKind::RunnerClassCommands`・設計 contract-source.md §48 の 2・ADR-0061・
/// `s2-07l.601`）: **値は manifest が持つ**（C1 / C5）＝裁定の 3 要素をこの順で持ち（consume は要素なし）、行の裁定 id と裁定日は
/// 裁定の値。kind は `ALL` の末尾の 1 つ前（末尾は `.696` の `HostGuardPublish`）・形は List。各要素は崩れ (a)〜(d) に当たらない（要素の読み手 1 本で名 + 語列に分かれ、語列の
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
    // `.696` が末尾に公開の見張りの kind を足し、`.738.37.1` が床の検査の待ちの上限の kind を足した（その 2 つ前が RunnerClassCommands）。
    // 後ろに局面の出力の 2 kind（`.738.38.7`）が続く。
    let tail: Vec<&RuleKind> = ALL.iter().rev().skip(5).take(6).collect();
    assert_eq!(
        tail,
        [&RuleKind::SeatDraftsBusyS, &RuleKind::SeatDraftsCapMb, &RuleKind::PipeReserveH, &RuleKind::FloorTimeoutS, &RuleKind::HostGuardPublish, &RuleKind::RunnerClassCommands],
        "kind は ALL の末尾の 2 つの前の 6 つ目"
    );
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

/// 埋め込み manifest の `role.<役割名>` の行の値（名の列・行が無い・列でない周は空＝呼び側の assert が落とす）。
fn role_row_names(manifest: &Manifest, role: Role) -> Vec<String> {
    match manifest.get(&format!("role.{}", role.as_str())).map(|row| row.value.clone()) {
        Some(RuleValue::List(names)) => names,
        _ => Vec::new(),
    }
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
/// 行の今の裁定（[`ROW_RULING`]・`settle` を足した裁定）で、`launch` / `merge` は今までどおり無い（`--all` と名指しの
/// 無い停止と起動・着地は席から撃てないまま）。席の指示文（§5 の権能の行）に `stop` が出ることは `hook_brief_` の歯と
/// 外形 snapshot が測る。
#[test]
fn rules_role_stop_is_in_the_orchestrator_row_with_the_ruling() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get("role.orchestrator").expect("役割の行が在る");
    let names = role_row_names(&manifest, Role::Orchestrator);
    assert_eq!(names.iter().filter(|name| *name == "stop").count(), 1, "値に stop が 1 つ: {names:?}");
    assert!(!names.iter().any(|name| name == "launch" || name == "merge"), "起動と着地は無いまま: {names:?}");
    assert!(row.enabled, "発効している");
    assert_eq!(row.ruling, ROW_RULING, "裁定 id は行の今の裁定");
    assert_eq!(row.ruled_at, ROW_RULED_AT, "裁定日");
    assert_ne!(row.ruling, "user 2026-09-18T08:3xZ", "前の裁定 id のままではない");
    let held = brief::capabilities_of(&manifest, Role::Orchestrator).expect("指示文の読み手も同じ行を読む");
    assert!(held.contains(&Capability::Stop), "指示文の権能の列に stop: {held:?}");
    assert!(!held.contains(&Capability::Launch), "{held:?}");
}

/// 約束 1（設計 seat-roles.md §32・ADR-0097・`s2-07l.737.18`）: 権能の閉じた列と全 variant の列に `settle` が 1 つ在り
/// （字面は `settle`・宣言順は `stop` の直後）、rules 行の loader は `settle` を知っている名として受け、知らない名
/// （`settl`・variant 名の字面 `Settle`）は今までどおり `RuleError` で拒む（取る名の列に `settle` を名指す）。
#[test]
fn rules_role_settle_is_a_capability_name_the_loader_accepts() {
    assert_eq!(Capability::Settle.as_str(), "settle", "行と記録の字面");
    assert_eq!(Capability::parse("settle"), Some(Capability::Settle), "字面から引ける");
    assert_eq!(CAPABILITIES.iter().filter(|cap| **cap == Capability::Settle).count(), 1, "全 variant の列に 1 つ");
    let at = CAPABILITIES.iter().position(|cap| *cap == Capability::Settle);
    let stop = CAPABILITIES.iter().position(|cap| *cap == Capability::Stop);
    assert_eq!(at, stop.map(|found| found + 1), "宣言順は stop の直後");
    assert!(Capability::Stop < Capability::Settle && Capability::Settle < Capability::EditContract, "判別子順");
    let accepted = parsed(&one_row(RuleKind::RoleCapabilities, r#"["answer", "settle"]"#)).expect("settle を持つ列は受理される");
    assert_eq!(
        accepted.get("probe").map(|row| row.value.clone()),
        Some(RuleValue::List(vec!["answer".to_owned(), "settle".to_owned()])),
        "書いた順のまま"
    );
    for (bad, word) in [(r#"["settl"]"#, "settl"), (r#"["Settle"]"#, "Settle"), (r#"["settle", "halt"]"#, "halt")] {
        let errors = rejected(&one_row(RuleKind::RoleCapabilities, bad)).expect("知らない名は受理されない");
        assert_eq!(errors.len(), 1, "{bad}: 件数: {errors:?}");
        let first = errors.first().map(String::as_str).unwrap_or_default();
        assert!(first.contains(&format!("未知の権能 {word}")), "{bad}: 理由: {first}");
        assert!(first.contains("settle"), "{bad}: 取る名の列に settle を名指す: {first}");
    }
}

/// 約束 2（§32・ADR-0097）: 埋め込みの rules 行 `role.orchestrator` の値が `settle` を stop の直後に 1 つ持ち、裁定 id と
/// 裁定日が `user 2026-09-29T21:53Z` / `2026-09-29` で、`launch` / `merge` は今までどおり無い。席の指示文の読み手も
/// `settle` を持つ（外形は `hook_brief_` の snapshot が測る）。
#[test]
fn rules_role_settle_is_in_the_orchestrator_row_with_the_ruling() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let row = manifest.get("role.orchestrator").expect("役割の行が在る");
    let names = role_row_names(&manifest, Role::Orchestrator);
    assert_eq!(names.iter().filter(|name| *name == "settle").count(), 1, "値に settle が 1 つ: {names:?}");
    let stop = names.iter().position(|name| name == "stop");
    assert_eq!(names.iter().position(|name| name == "settle"), stop.map(|found| found + 1), "stop の直後: {names:?}");
    assert!(!names.iter().any(|name| name == "launch" || name == "merge"), "起動と着地は無いまま: {names:?}");
    assert!(row.enabled, "発効している");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-29T21:53Z", "2026-09-29"), "裁定 id と裁定日");
    let held = brief::capabilities_of(&manifest, Role::Orchestrator).expect("指示文の読み手も同じ行を読む");
    assert!(held.contains(&Capability::Settle), "指示文の権能の列に settle: {held:?}");
    assert!(!held.contains(&Capability::Launch) && !held.contains(&Capability::Merge), "{held:?}");
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

/// 役割ごとの既定の対の裁定 id（設計 seat-roles.md §19 の形・値は §28 の改め・逐語は台帳）。
const DEFAULTS_RULING: &str = "user 2026-09-26T15:41Z";

/// [`DEFAULTS_RULING`] の裁定日。
const DEFAULTS_RULED_AT: &str = "2026-09-26";

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

/// 行の読み手 `str_row` / `int_row`（`rules::` の 1 本・headless と `pipe::ratelimit` が読む・`s2-07l.297`）: 発効した
/// 文字列 / 整数の行の値を返し、無い / 不発効 / 形違いは 3 理由の `Err`（lens の cap の字面と同じ）。
#[test]
fn rules_row_readers_return_the_value_or_one_of_three_reasons() {
    let manifest = Manifest::embedded().expect("埋め込み manifest を読める");
    assert_eq!(str_row(&manifest, "runner.model"), Ok("sonnet"), "文字列の行の読み手（裁定 user 2026-09-29T07:44Z）");
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
