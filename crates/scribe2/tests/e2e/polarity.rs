//! `polarity` の歯（設計 docs/design/polarity.md §4 / §7・ADR-0014・契約 s2-07l.25）。
//!
//! 外形の snapshot が**極性一覧の生成物**である（C11.2 / C12.5）。集計行は snapshot と
//! **独立に**行を数えて突き合わせる（snapshot が集計の嘘ごと固定される形を作らない）。

use std::process::Command;
use vessel::cli_outcome::RC_OK;
use vessel::order::is_declaration_order;
use vessel::polarity::{Guard, OnFailure, Polarity, Timing, ALL};

/// `<NAME> polarity` を撃って stdout を返す（rc 0・stderr 0 byte を表明する）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn output() -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_scribe2"))
        .arg("polarity")
        .output()
        .expect("binary を起動できる");
    assert_eq!(out.status.code(), Some(i32::from(RC_OK)), "rc 0 のはず: {out:?}");
    assert!(out.stderr.is_empty(), "stderr は 0 byte のはず: {out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 集計行の `key=<n>` を読む。
fn count_of(line: &str, key: &str) -> Option<usize> {
    line.split(' ')
        .find_map(|token| token.strip_prefix(&format!("{key}=")))
        .and_then(|value| value.parse().ok())
}

/// 全出力を snapshot に pin する（これが一覧の生成物・ADR-0014 §2.2）。
#[test]
fn polarity_external_form() {
    let form = output();
    insta::assert_snapshot!(form);
}

/// 集計行の N / K / M / F は行を数えた値と一致し、N = K + M・N = `ALL` の長さである。
#[test]
fn polarity_summary_counts_match_lines() {
    let text = output();
    let lines: Vec<&str> = text.lines().collect();
    let guards: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| line.starts_with("guard="))
        .collect();
    let summary = lines.last().copied().unwrap_or_default();
    assert!(summary.starts_with("polarity: guards="), "末尾は集計行のはず: {summary}");
    assert_eq!(lines.len(), guards.len() + 1, "guard 行 + 集計 1 行だけ: {text}");
    let in_loop = guards.iter().filter(|line| line.contains(" timing=in-loop ")).count();
    let post_hoc = guards.iter().filter(|line| line.contains(" timing=post-hoc ")).count();
    let fail_open = guards.iter().filter(|line| line.contains(" on-failure=fail-open ")).count();
    assert_eq!(count_of(summary, "guards"), Some(guards.len()), "N = guard 行数: {summary}");
    assert_eq!(count_of(summary, "in-loop"), Some(in_loop), "K = in-loop 行数: {summary}");
    assert_eq!(count_of(summary, "post-hoc"), Some(post_hoc), "M = post-hoc 行数: {summary}");
    assert_eq!(count_of(summary, "fail-open"), Some(fail_open), "F = fail-open 行数: {summary}");
    assert_eq!(in_loop + post_hoc, guards.len(), "N = K + M");
    assert_eq!(guards.len(), ALL.len(), "1 guard 1 行");
    // 各行の形（token の名前と順序・設計 §4）と、名前の重複が無いこと。
    let mut names = Vec::new();
    for line in &guards {
        let tokens: Vec<&str> = line.split(' ').collect();
        assert_eq!(tokens.len(), 4, "token は 4 つ: {line}");
        for (token, key) in tokens.iter().zip(["guard=", "timing=", "on-failure=", "boundary="]) {
            assert!(token.starts_with(key), "{key} の位置が違う: {line}");
        }
        names.push(tokens.first().copied().unwrap_or_default());
    }
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), names.len(), "guard 名は一意: {names:?}");
}

/// `ALL` は宣言順（ADR-0013 §2.2 の判別子順 pin・既存の述語を使う）。
#[test]
fn polarity_all_is_in_declaration_order() {
    assert!(is_declaration_order(ALL, |guard| guard as usize), "ALL は宣言順: {ALL:?}");
    let swapped = [Guard::Permission, Guard::WriteSet];
    assert!(!is_declaration_order(&swapped, |guard| guard as usize), "述語は並べ替えを落とす");
}

/// FailOpen の境界（cap guard・FR26）は **そのまま** fail-open で出る（隠さない）。in-loop の
/// guard が 1 つ以上在る（C16.2 の門が守る事実の現物）。
#[test]
fn polarity_lists_cap_guard_as_fail_open_without_hiding_it() {
    assert_eq!(Guard::Cap.polarity().on_failure, OnFailure::FailOpen, "cap guard は FailOpen");
    assert_eq!(Guard::Cap.polarity().timing, Timing::InLoop, "cap guard は編集の時点で止める");
    let text = output();
    assert!(
        text.lines()
            .any(|line| line.starts_with("guard=cap-guard ") && line.contains(" on-failure=fail-open ")),
        "一覧に cap guard が fail-open で載る: {text}"
    );
    assert!(
        ALL.iter().any(|guard| guard.polarity().timing == Timing::InLoop),
        "in-loop の guard が 1 つ以上在る"
    );
    // 境界の pointer は crate 相対の `module::Type` の形（`::` を 1 つ以上持つ）。
    for guard in ALL {
        assert!(guard.boundary().contains("::"), "boundary は module::Type の形: {}", guard.boundary());
    }
}

/// `.25` の母集団に無かった 3 境界（`s2-07l.106`・lens MEDIUM-1）が**値で**載る。承認関門と注入の
/// 断りは in-loop / fail-closed、runner の中断は集合に無い status では止めない（ADR-0012 §2.1）
/// ゆえ **fail-open のまま**出る（cap guard と同じく隠さない）。snapshot の字面は pin しない。
#[test]
fn polarity_lists_the_three_added_guards() {
    assert_eq!(Guard::Approval.polarity(), Polarity { timing: Timing::InLoop, on_failure: OnFailure::FailClosed });
    assert_eq!(Guard::Inject.polarity(), Polarity { timing: Timing::InLoop, on_failure: OnFailure::FailClosed });
    assert_eq!(Guard::RunnerStop.polarity(), Polarity { timing: Timing::InLoop, on_failure: OnFailure::FailOpen });
    let text = output();
    for expected in [
        "guard=approval-gate timing=in-loop on-failure=fail-closed boundary=pipe::approve::needs_approval",
        "guard=runner-stop timing=in-loop on-failure=fail-open boundary=headless::runner::Decision",
        "guard=inject-refusal timing=in-loop on-failure=fail-closed boundary=seat::inject::Delivery",
    ] {
        assert!(text.lines().any(|line| line == expected), "一覧に載る: {expected}\n{text}");
    }
    assert_eq!(ALL.len(), 13, "母集団は 13（10 + 3）");
}
