//! rules の歯（doctor の host の面・設計 docs/design/seat-roles.md §7 / rules-manifest.md §4.2 / §5・
//! 接頭辞 `rules_host_`）。壊れた `--rules` の歯は席の `--rules` の seam ごと消えた（ADR-0045 §2 (2)・
//! `s2-07l.479.2`: 最後の 2 口 externalize / rebrief が無くなり、seat に `--rules` を受ける口が
//! 1 つも残らない）。
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
