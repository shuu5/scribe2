//! rules の歯（doctor の host の面・設計 docs/design/seat-roles.md §7 / rules-manifest.md §4.2 / §5・
//! 接頭辞 `rules_host_` / `seat_rules_`）。壊れた `--rules` を defect の列で断る歯の族は、席の `--rules` の
//! seam ごと消えた（ADR-0045 §2 (2)・`s2-07l.479.2`: 最後の 2 口 externalize / rebrief が無くなり、
//! seat に `--rules` を受ける口が 1 つも残らない）。残るのは**その不在を測る歯 1 本**である。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。
//! 歯の本文は `seat/account.rs` から**挙動不変で移した**もの（`s2-07l.361`・seat-roles.md §7 の固定した組）。
// flip-check: moved s2-07l.361

use super::*;

// ─────────────────────────── doctor の host の面（s2-07l.233・account-autonomy.md §5） ───────────────────────────

/// (f) doctor の `host-manifest=` は 3 値: 無い = `absent`・読める = `present`（口座の行は host の面込みの宣言）・
/// 壊れている / 面をまたいで重複する = `unreadable`（報告は止めない＝rc 0・口座の行は `accounts: manifest=unreadable`）。
/// 位置は突合の行の直後・口座の行の直前。
#[test]
fn rules_host_doctor_names_the_host_manifest_in_three_values() {
    let place = role_doctor_place();
    let host = place.state.join(vessel::rules::HOST_MANIFEST);
    let tail_of = |lines: &[String]| -> Vec<String> {
        let seats = lines.iter().position(|line| line.starts_with("seats: ")).unwrap_or(lines.len());
        lines.iter().skip(seats + 1).cloned().collect()
    };
    assert_eq!(
        tail_of(&doctor_rows(&place, &account_rules(&["tracked"]))),
        [HOST_ABSENT, account_line_of("tracked").as_str(), CONSUMER_REPO],
        "口座の行の後ろに導入先の行"
    );
    fs::write(&host, account_rules(&["hosted"])).expect("host の面を書ける");
    assert_eq!(
        tail_of(&doctor_rows(&place, &account_rules(&["tracked"]))),
        ["host-manifest=present".to_owned(), account_line_of("hosted"), account_line_of("tracked"), CONSUMER_REPO.to_owned()],
        "host の面込みの宣言（label の辞書順）"
    );
    let unreadable = ["host-manifest=unreadable", "accounts: manifest=unreadable"];
    fs::write(&host, "schema = 1\n\n[[account]]\nlabel = \"hosted\"\nbogus = 1\n").expect("host の面を壊せる");
    assert_eq!(tail_of(&doctor_rows(&place, &account_rules(&["tracked"]))), unreadable, "壊れた host の面");
    fs::write(&host, account_rules(&["tracked"])).expect("host の面を書ける");
    assert_eq!(tail_of(&doctor_rows(&place, &account_rules(&["tracked"]))), unreadable, "面をまたぐ重複も読めない側");
    fs::remove_dir_all(&place.dir).ok();
}

/// 登録 row が anchor `/repo` の置き場で、dir の無い口座 1 つの doctor の行。
fn account_line_of(label: &str) -> String {
    format!("account={label} dir=missing credential=missing config=missing agentview=unreadable trust=unreadable retired=no")
}

// ─────────────────────────── 壊れた --rules の族の不在（s2-07l.479.2） ───────────────────────────

/// 欠陥 3 件の manifest（未知 kind / `ruling` 欠け / id 重複）。base では席の口がこれを defect の列で
/// 並べた——不在を測るには「並べられる入力」を渡す必要があるので fixture だけ残す。
const BROKEN_RULES: &str = concat!(
    "schema = 1\n",
    "\n",
    "[[rule]]\n",
    "id = \"seat.unknown_kind\"\n",
    "kind = \"NoSuchKind\"\n",
    "value = 1\n",
    "enabled = true\n",
    "ruling = \"user 2026-09-12T02:01Z\"\n",
    "ruled_at = \"2026-09-12\"\n",
    "\n",
    "[[rule]]\n",
    "id = \"seat.no_ruling\"\n",
    "kind = \"LedgerTimeoutS\"\n",
    "value = 30\n",
    "enabled = true\n",
    "ruled_at = \"2026-09-12\"\n",
);

/// 席の口はもう `--rules` を読まない（ADR-0045 §2 (2)・`s2-07l.479.2`）: 使い方の 1 行に `--rules` の
/// 字面が 1 つも無く、`--rules` を読んでいた 2 口（externalize / rebrief）へ壊れた manifest を
/// base と同じ引数で渡しても、`rules validate` の defect の行は 1 行も出ず**使い方 1 行だけ**で断る。
///
/// **消えたことを測る歯**である（base では defect の行が並ぶので RED）。
#[test]
fn seat_rules_no_seat_face_reads_a_rules_flag() {
    let dir = tmp();
    let rules = fixture(&dir, "rules.toml", BROKEN_RULES);
    let wm = dir.join("wm").display().to_string();
    let anchor = dir.display().to_string();
    let plan = dir.join("plan.md").display().to_string();
    let usage = stderr_of(&run_seat(&[]));
    assert!(!usage.contains("--rules"), "使い方に `--rules` は残らない: {usage}");
    let faces: [(&str, Vec<&str>); 2] = [
        ("externalize", vec!["--anchor", anchor.as_str(), "--plan", plan.as_str(), "--directives", plan.as_str()]),
        ("rebrief", vec!["--anchor", anchor.as_str()]),
    ];
    for (face, extra) in faces {
        let mut args = vec![face, "--target", "seatrules", "--wm-dir", wm.as_str()];
        args.extend(extra);
        args.extend(["--rules", rules.as_str()]);
        let out = run_seat(&args);
        let lines: Vec<String> = stderr_of(&out).lines().map(str::to_owned).collect();
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{face}: 断る: {lines:?}");
        assert_eq!(out.stdout.len(), 0, "{face}: 断りの周は stdout 0 byte");
        assert_eq!(lines.len(), 1, "{face}: 使い方 1 行だけ（defect は並ばない）: {lines:?}");
        assert!(lines[0].starts_with("usage: seat "), "{face}: 使い方の行: {lines:?}");
    }
    fs::remove_dir_all(&dir).ok();
}
