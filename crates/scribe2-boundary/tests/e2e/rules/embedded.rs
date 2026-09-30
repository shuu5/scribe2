// flip-check: moved s2-07l.682
//! embedded と manifest の族の歯（接頭辞 `rules_embedded_` / `rules_manifest_`・設計 docs/design/carry-prep.md §9 行 j）。
//!
//! 共有の helper と const と外形 snapshot の歯（`rules_external_form`・snapshot 名が module path を含むので
//! 動かさない）は親 module（`tests/e2e/rules.rs`）に在り、`use super::*` で使う。
//! 歯の本文は親から**挙動不変で移した**もの（`s2-07l.682`）。

use super::*;

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
fn rules_manifest_reports_all_errors_with_line_numbers() {
    let errors = rejected(DEFECTIVE).expect("拒まれるはずの fixture が受理された");
    assert_eq!(errors.len(), 3, "欠陥 3 箇所は 3 行になる: {errors:?}");
    let joined = errors.join("\n");
    for want in ["line=11", "line=19", "line=27"] {
        assert!(joined.contains(want), "{want} が無い:\n{joined}");
    }
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

/// 公開の見張りの行（host_guard.publish・設計 vessel-hook.md §16 形 6・ADR-0078・`s2-07l.696`）が kind `HostGuardPublish`・形 List・
/// enabled・裁定 id と裁定日で引け、値は 4 記号の `form` の 4 要素（`exclude` は 0 件）で、kind を字面から引ける（base では行も
/// kind も無い ＝ RED）。
#[test]
fn rules_embedded_manifest_declares_publish_guard_row_with_the_four_forms() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let kind = RuleKind::parse("HostGuardPublish").expect("kind を字面から引ける");
    let row = manifest.get("host_guard.publish").unwrap_or_else(|| panic!("host_guard.publish の行が在る"));
    assert_eq!((row.kind, row.kind.shape()), (kind, ValueShape::List), "kind と形");
    assert!(row.enabled, "既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-27T23:55Z", "2026-09-27"), "裁定 id と裁定日");
    let want = ["form repo-name", "form object-id", "form tracked-path", "form ledger-id"].map(str::to_owned).to_vec();
    assert_eq!(row.value, RuleValue::List(want), "4 記号の form・exclude は 0 件");
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == kind).count(), 1, "kind の行は 1 本");
}

/// 公開の配線の上限の 2 行（host_guard.publish_deadline_ms・host_guard.publish_read_bytes・設計 vessel-hook.md §22 行 n・NFR5・
/// `s2-07l.738.18`）が kind・形 Int・値（6 秒と 8 MB）・enabled・裁定 id と裁定日で引け、kind の行は 1 本ずつで、kind 2 値は
/// `ALL` の `HookBudgetMs` の直後に宣言順で並ぶ（base では行も kind も無い ＝ RED）。
#[test]
fn rules_embedded_manifest_declares_publish_bounds_after_the_hook_budget() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    for (id, name, want) in [
        ("host_guard.publish_deadline_ms", "HostGuardPublishDeadlineMs", 6000),
        ("host_guard.publish_read_bytes", "HostGuardPublishReadBytes", 8_388_608),
    ] {
        let kind = RuleKind::parse(name).unwrap_or_else(|| panic!("{name} を字面から引ける"));
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!((row.kind, row.kind.shape()), (kind, ValueShape::Int), "{id} の kind と形");
        assert_eq!(row.value, RuleValue::Int(want), "{id} の値");
        assert!(row.enabled, "{id} は既定で効く");
        assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-29T04:37Z", "2026-09-29"), "{id} の裁定 id と裁定日");
        assert_eq!(manifest.rows().iter().filter(|found| found.kind == kind).count(), 1, "{id} の kind の行は 1 本");
    }
    let at = ALL.iter().position(|kind| *kind == RuleKind::HookBudgetMs).expect("HookBudgetMs は ALL に在る");
    let next: Vec<RuleKind> = ALL.iter().skip(at.saturating_add(1)).take(2).copied().collect();
    assert_eq!(next, [RuleKind::HostGuardPublishDeadlineMs, RuleKind::HostGuardPublishReadBytes], "HookBudgetMs の直後に宣言順で 2 値");
}

/// `LedgerDeniedWrites` の直後に台帳のグラフの上限の kind（`.719`・設計 ledger-form.md §10 形 4）が在り、kind 2 つはその
/// 直後に宣言順で並び、その直後に管理 tick の 3 kind（`.582`・設計 seat-heartbeat.md §2
/// 形 5 / §10 形 5＝梯子の列の kind は `SeatTickStaleS` の直後）と退避の猶予の kind（`.651`・§13 形 1＝梯子の列の kind の直後）が
/// 宣言順で続き、クラスの語列表の kind（`.601`・設計 contract-source.md §48）と公開の見張りの kind（`.696`・設計 vessel-hook.md
/// §16）で `ALL` が終わり、字面から引ける。末尾の絶対位置で
/// なく `LedgerDeniedWrites` の位置から測る（kind を末尾に足した後も同じ並びを測る）。
#[test]
fn rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all() {
    let at = ALL.iter().position(|kind| *kind == RuleKind::LedgerDeniedWrites).expect("LedgerDeniedWrites は ALL に在る");
    let run: Vec<RuleKind> = ALL.iter().skip(at).copied().collect();
    assert_eq!(
        run,
        vec![
            RuleKind::LedgerDeniedWrites,
            RuleKind::LedgerOpenChildrenMax,
            RuleKind::HostGuardDeniedCommands,
            RuleKind::HostGuardRmProtected,
            RuleKind::SeatTickIntervalS,
            RuleKind::SeatTickStaleS,
            RuleKind::SeatPointerLadderS,
            RuleKind::SeatMoveGraceS,
            RuleKind::SeatMemoryMaxMb,
            RuleKind::SeatIdleAlarmS,
            RuleKind::SeatPrecheckAlarmS,
            RuleKind::RunnerClassCommands,
            RuleKind::HostGuardPublish,
        ],
        "LedgerDeniedWrites の後ろに台帳のグラフの上限の kind（`.719`）・宣言順で 2 kind・管理 tick の 3 kind・退避の猶予・席の箱・段の上げ・事前審査の束の段の上げ（`.717`）・クラスの語列表の kind・公開の見張りの kind（`.696`）で ALL が終わる（母集団 {} 種）",
        ALL.len()
    );
    let tail: Vec<RuleKind> = ALL.iter().rev().take(9).rev().copied().collect();
    assert_eq!(
        tail,
        vec![
            RuleKind::SeatTickIntervalS,
            RuleKind::SeatTickStaleS,
            RuleKind::SeatPointerLadderS,
            RuleKind::SeatMoveGraceS,
            RuleKind::SeatMemoryMaxMb,
            RuleKind::SeatIdleAlarmS,
            RuleKind::SeatPrecheckAlarmS,
            RuleKind::RunnerClassCommands,
            RuleKind::HostGuardPublish
        ],
        "逆順に 9 つ取った並びが管理 tick の 3 kind・退避の猶予・席の箱・段の上げ・事前審査の束の段の上げ・クラスの語列表の kind・公開の見張りの kind"
    );
    for kind in [RuleKind::HostGuardDeniedCommands, RuleKind::HostGuardRmProtected, RuleKind::HostGuardPublish] {
        assert_eq!(RuleKind::parse(kind.as_str()), Some(kind), "kind を字面から引ける: {}", kind.as_str());
    }
    assert_eq!(RuleKind::HostGuardDeniedCommands.as_str(), "HostGuardDeniedCommands", "字面は variant 名");
    assert_eq!(RuleKind::HostGuardRmProtected.as_str(), "HostGuardRmProtected", "字面は variant 名");
}

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

/// 退避の猶予の行（設計 seat-heartbeat.md §13 形 1 / §18 形 4・ADR-0071 / ADR-0079・`s2-07l.651` / `s2-07l.729`）が埋め込み
/// manifest に id / kind / 形 Int / 値 300 / enabled / 裁定 id / 裁定日で 1 本在り、kind は `ALL` の `SeatPointerLadderS` の直後で字面から引け、形は Int だけ（base では
/// 行も kind も無い ＝ RED）。
#[test]
fn rules_embedded_manifest_declares_tick_move_grace_row_after_the_ladder() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let id = "seat.move_grace_s";
    let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
    assert_eq!((row.kind, row.kind.shape()), (RuleKind::SeatMoveGraceS, ValueShape::Int), "{id} の kind と形");
    assert_eq!(row.value, RuleValue::Int(300), "{id} の値（上限 5 分・seat-heartbeat.md §18 形 4）");
    assert!(row.enabled, "{id} は既定で効く");
    assert_eq!((row.ruling.as_str(), row.ruled_at.as_str()), ("user 2026-09-28T01:44Z", "2026-09-28"), "{id} の裁定 id と裁定日");
    assert_eq!(int_row(&manifest, id), Ok(300), "{id} を整数の読み手で引ける");
    assert_eq!(manifest.rows().iter().filter(|found| found.kind == RuleKind::SeatMoveGraceS).count(), 1, "kind の行は 1 本");
    let at = ALL.iter().position(|kind| *kind == RuleKind::SeatPointerLadderS).expect("SeatPointerLadderS は ALL に在る");
    assert_eq!(ALL.get(at + 1), Some(&RuleKind::SeatMoveGraceS), "kind は SeatPointerLadderS の直後");
    assert_eq!(RuleKind::parse("SeatMoveGraceS"), Some(RuleKind::SeatMoveGraceS), "字面から引ける");
    let errors = rejected(&one_row(RuleKind::SeatMoveGraceS, "\"half\"")).expect("文字列の値の fixture が受理された");
    assert!(errors.join("\n").contains("形と合わない"), "形は Int だけ: {errors:?}");
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
    assert_eq!(manifest.rows().len(), 82, "埋め込み manifest の行数（母集団・`.738.18` で +2〔公開の配線の締め切りと読む上限〕・`.736.25` で +1〔席の起草の置き場の書きの線〕・`.736.19` で +2〔lens の model と先撃ちの lens の model〕・`.696` で +1〔公開の見張りの行〕・`.718` で +1〔事前審査の先撃ちの 1 周の本数〕・`.717` で +1〔事前審査の確定の束の段の上げ〕・`.719` で +1〔台帳のグラフの直下の open の子の上限〕・`.704` で +1〔heartbeat の段の上げ〕・`.694` で +1〔終端の CI の照合の間隔〕・`.627` で +1〔席の箱〕・`.651` で +1〔退避の猶予〕・`.637` で -2 +1〔梯子の係数と上限の行の退役と梯子の列の行〕・`.600` で +1〔境界 crate の上限 R-C4-5〕・`.601` で +1〔クラスの語列表〕・`.582` で +4〔管理 tick の行 4 本〕・`.172` で +1〔便ごとの token 消費の検出線 R-C6-1〕・`.574` で +4〔host の見張りの種類ごとの行〕・`.169` で +1〔台帳 write の断る形〕・`.170` で +2〔flip の免除経路の面と札の上限〕・`.217` で +2・`.249` で +3・`.254` で +1・`.168` で +1・`.297` で +1・`.315` で +1・`.322` で +1・`.360` で +1・`.423` で +2・`.478` で -1〔役割の行 2 本が 1 本〕・`.479.1` で -7〔席の自律の行〕・`.479.2` で -3〔作業記憶の行 1 本と棚卸しの行 2 本〕・`.382` で +1〔終端の CI の上限〕・`.433` で +2〔役割の既定の model と effort〕・`.407` で +1〔選定の前計測の鮮度〕・`.396` で +1〔同型の審査 FAIL の停止の回数〕・`.504` の行 x で +2〔器の健康の遮断器の倍率〕・`.428` で +1〔着地の列の上限〕・`.398` で +1〔同時本数の最大値〕・`.491.3` で +3〔群の逼迫の閾値の窓ごとの行〕）");
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
    assert!(ALL.contains(&RuleKind::RoleCapabilities), "ALL に在る（末尾は `.696` の公開の見張り）");
    assert_eq!(RuleKind::parse("RoleCapabilities"), Some(RuleKind::RoleCapabilities), "kind を字面から引ける");
    let kinds = ALL.len();
    assert_eq!(kinds, 80, "kind の母集団（`.738.18` で +2〔公開の配線の締め切りと読む上限〕・`.736.25` で +1〔席の起草の置き場の書きの線〕・`.736.19` で +2〔lens の model と先撃ちの lens の model〕・`.696` で +1〔公開の見張りの行〕・`.718` で +1〔事前審査の先撃ちの 1 周の本数〕・`.717` で +1〔事前審査の確定の束の段の上げ〕・`.719` で +1〔台帳のグラフの直下の open の子の上限〕・`.704` で +1〔heartbeat の段の上げ〕・`.694` で +1〔終端の CI の照合の間隔〕・`.627` で +1〔席の箱〕・`.651` で +1〔退避の猶予〕・`.637` で -2 +1〔梯子の係数と上限の 2 種の退役と梯子の列の 1 種〕・`.600` で +1〔境界 crate の上限〕・`.601` で +1〔クラスの語列表〕・`.582` で +4〔管理 tick の 4 種〕・`.172` で +1〔便ごとの token 消費の検出線〕・`.574` で +2〔host の見張りの語列と rm の守る集合〕・`.169` で +1〔台帳 write の断る形〕・`.170` で +2〔flip の免除経路の面と札の上限〕・`.201` で +1・`.217` で +2・`.249` で +3・`.254` で +1・`.168` で +1・`.297` で +1・`.315` で +1・`.322` で +1・`.360` で +1・`.423` で +2・`.479.2` で -3・`.382` で +1〔終端の CI の上限〕・`.433` で +2〔役割の既定の 2 種〕・`.407` で +1〔選定の前計測の鮮度〕・`.396` で +1〔同型の審査 FAIL の停止の回数〕・`.504` の行 x で +2〔器の健康の遮断器の倍率〕・`.428` で +1〔着地の列の上限〕・`.398` で +1〔同時本数の最大値〕・`.491.3` で +3〔群の逼迫の閾値の 3 種〕）");
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

/// 裁定 `user 2026-09-18T08:3xZ`（ADR-0045 §2 (1)）の値に裁定 `user 2026-09-20`（ADR-0048 §2）が `stop` を足した:
/// 席は orchestrator 1 つで、記帳（回答・承認・go）と便 1 本を名指す停止と契約・`design-intent/`・設計 doc・repo の
/// 外の編集と歯（`crates/<crate>/tests/`）の編集を持つ。裁定 `user 2026-09-29T21:53Z`（ADR-0097）が止まった終端を閉じる
/// `settle` を足した。
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
        Capability::Settle,
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
        "settle",
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
    assert_eq!(row.ruling, ROW_RULING, "{ORCHESTRATOR_ROW} の裁定 id");
}

/// 歯 (d・形 6): 埋め込み manifest が役割の既定の 2 行を**値ごと**運ぶ（裁定 `user 2026-09-26T15:41Z` の
/// `opus` / `xhigh`・§28・値の正本は manifest で設計 doc は写さない・C1 / C5）。kind は宣言順で隣り合う 2 つ
/// （`RunnerEffort` の直後の lens の model の 2 つ〔`.736.19`〕の直後が `RoleModel`・その直後が `RoleEffort`・末尾は `.396` の
/// `ReviewSameKindStop`）で、字面から引ける。
#[test]
fn rules_manifest_carries_role_defaults() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let model = manifest.get("seat.model.orchestrator").expect("既定の model の行が在る");
    assert_eq!(model.value, RuleValue::Str("opus".to_owned()), "裁定 user 2026-09-26T15:41Z の model");
    assert_eq!(Model::parse("opus"), Some(Model::Opus), "値は閉じた表で引ける");
    assert_eq!(model.ruling, DEFAULTS_RULING, "model の行の裁定 id");
    assert_eq!(model.ruled_at, DEFAULTS_RULED_AT, "model の行の裁定日");
    let effort = manifest.get("seat.effort.orchestrator").expect("既定の effort の行が在る");
    assert_eq!(effort.value, RuleValue::Str("xhigh".to_owned()), "裁定 user 2026-09-26T15:41Z の effort");
    assert_eq!(Effort::parse("xhigh"), Some(Effort::Xhigh), "値は閉じた表で引ける");
    assert_eq!(effort.ruling, DEFAULTS_RULING, "effort の行の裁定 id");
    assert_eq!(effort.ruled_at, DEFAULTS_RULED_AT, "effort の行の裁定日");
    let at = ALL.iter().position(|kind| *kind == RuleKind::RunnerEffort).unwrap_or_default();
    assert_eq!(ALL.get(at.saturating_add(3)), Some(&RuleKind::RoleModel), "宣言順は RunnerEffort と lens の model の 2 つの直後");
    assert_eq!(ALL.get(at.saturating_add(4)), Some(&RuleKind::RoleEffort), "対は宣言順で隣り合う");
    assert_eq!(ALL.get(at.saturating_add(5)), Some(&RuleKind::ReviewSameKindStop), "`.433` の 2 種の直後が `.396` の末尾");
    assert_eq!(RuleKind::parse("RoleModel"), Some(RuleKind::RoleModel), "kind を字面から引ける");
    assert_eq!(RuleKind::parse("RoleEffort"), Some(RuleKind::RoleEffort), "kind を字面から引ける");
}

/// (f) `runner.model`（runner が claude に毎回渡す model・裁定 id `user 2026-09-29T07:44Z`・設計 pipeline.md §6 / §61・
/// `s2-07l.297` / `.736.19`）: 埋め込み manifest の行は発効 ∧ `Str("sonnet")`（claude CLI の別名・閉じた表 `Model` で引ける）・
/// kind は宣言順で `RunnerEffort` の直前 `RunnerModel`（形は `Str`）・`str_row` が同じ値を返し、不発効 / 整数の行は 3 理由で
/// `Err`。
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
    assert_eq!(row.value, RuleValue::Str("sonnet".to_owned()), "値は claude CLI の別名（裁定 = Sonnet）");
    assert!(row.enabled, "発効している");
    assert!(row.ruling.starts_with(MODEL_SPLIT_RULING), "裁定 id: {}", row.ruling);
    assert_eq!(row.ruled_at, MODEL_SPLIT_RULED_AT, "裁定日");
    assert_eq!(Model::parse("sonnet"), Some(Model::Sonnet), "値は閉じた表で引ける");
    assert_eq!(RuleKind::RunnerModel.shape(), ValueShape::Str, "形は識別子");
    let at = ALL.iter().position(|kind| *kind == RuleKind::RunnerModel).unwrap_or_default();
    assert_eq!(ALL.get(at.saturating_add(1)), Some(&RuleKind::RunnerEffort), "宣言順で `.322` の RunnerEffort が直後");
    assert_eq!(RuleKind::parse("RunnerModel"), Some(RuleKind::RunnerModel));
    let healed = parsed(&one_row(RuleKind::RunnerModel, "\"sonnet\"")).expect("文字列の値は受理される");
    assert_eq!(healed.get("probe").map(|row| row.value.clone()), Some(RuleValue::Str("sonnet".to_owned())));
    let errors = rejected(&one_row(RuleKind::RunnerModel, "5")).expect("整数の値は形が合わない");
    assert!(errors.join("\n").contains("形と合わない"), "{errors:?}");
}

/// model の 3 行の裁定 id の頭（設計 pipeline.md §61・契約表の行 bd）。
const MODEL_SPLIT_RULING: &str = "user 2026-09-29T07:44Z";

/// 同じく裁定日。
const MODEL_SPLIT_RULED_AT: &str = "2026-09-29";

/// (g) 埋め込み manifest の model の 3 行（設計 pipeline.md §61 形 1・`s2-07l.736.19`）: `runner.model` = sonnet・`lens.model` =
/// opus・`pipe.precheck_lens_model` = sonnet が同じ裁定で発効し、kind の宣言順が `RunnerModel` → `RunnerEffort` → `LensModel` →
/// `PipePrecheckLensModel` → `RoleModel`（形は `Str`・字面から引ける）。`runner.effort` は値も裁定も不変。base は新しい kind が
/// 無く compile されないので RED。
#[test]
fn model_split_embedded_rows_carry_the_three_models_in_declaration_order() {
    let manifest = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest が拒まれた: {errors:?}"));
    let rows = [
        ("runner.model", RuleKind::RunnerModel, "sonnet", Model::Sonnet),
        ("lens.model", RuleKind::LensModel, "opus", Model::Opus),
        ("pipe.precheck_lens_model", RuleKind::PipePrecheckLensModel, "sonnet", Model::Sonnet),
    ];
    for (id, kind, value, model) in rows {
        let row = manifest.get(id).unwrap_or_else(|| panic!("{id} の行が在る"));
        assert_eq!(row.kind, kind, "{id} の kind");
        assert_eq!(row.value, RuleValue::Str(value.to_owned()), "{id} の値");
        assert!(row.enabled, "{id} は発効している");
        assert!(row.ruling.starts_with(MODEL_SPLIT_RULING), "{id} の裁定 id: {}", row.ruling);
        assert_eq!(row.ruled_at, MODEL_SPLIT_RULED_AT, "{id} の裁定日");
        assert_eq!(Model::parse(value), Some(model), "{id} の値は閉じた表で引ける");
        assert_eq!(kind.shape(), ValueShape::Str, "{id} の形は識別子");
        assert_eq!(RuleKind::parse(kind.as_str()), Some(kind), "{id} の kind を字面から引ける");
    }
    let effort = manifest.get("runner.effort").expect("runner.effort の行が在る");
    assert_eq!(effort.value, RuleValue::Str("high".to_owned()), "effort の値は不変");
    assert!(effort.ruling.starts_with("user 2026-09-15T03:52Z"), "effort の裁定は不変: {}", effort.ruling);
    assert_eq!(effort.ruled_at, "2026-09-15", "effort の裁定日は不変");
    let at = ALL.iter().position(|kind| *kind == RuleKind::RunnerModel).unwrap_or_default();
    let order: Vec<RuleKind> = ALL.iter().skip(at).take(5).copied().collect();
    let want = [RuleKind::RunnerModel, RuleKind::RunnerEffort, RuleKind::LensModel, RuleKind::PipePrecheckLensModel, RuleKind::RoleModel];
    assert_eq!(order, want, "kind の宣言順");
}
