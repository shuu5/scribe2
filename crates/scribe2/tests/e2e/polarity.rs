//! `polarity` の歯（設計 docs/design/polarity.md §4 / §7・ADR-0014・契約 s2-07l.25）。
//!
//! 外形の snapshot が**極性一覧の生成物**である（C11.2 / C12.5）。集計行は snapshot と
//! **独立に**行を数えて突き合わせる（snapshot が集計の嘘ごと固定される形を作らない）。

use std::process::Command;
use vessel::cli_outcome::RC_OK;
use vessel::order::is_declaration_order;
use vessel::pipe::approve::{Approval, POLARITY as APPROVAL_POLARITY};
use vessel::pipe::contract::Contract;
use vessel::pipe::land::{WorktreeCheck, ANCHOR_POLARITY, WORKTREE_POLARITY};
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
        "guard=approval-gate timing=in-loop on-failure=fail-closed boundary=pipe::approve::Approval",
        "guard=runner-stop timing=in-loop on-failure=fail-open boundary=headless::runner::Decision",
        "guard=inject-refusal timing=in-loop on-failure=fail-closed boundary=seat::inject::Delivery",
    ] {
        assert!(text.lines().any(|line| line == expected), "一覧に載る: {expected}\n{text}");
    }
    assert_eq!(ALL.len(), 16, "母集団は 16（10 + 3 + 質問の口 1・`s2-07l.115` + land の 2・`s2-07l.124`）");
}

/// runner の包みの質問 record（`s2-07l.115`・FR31・ADR-0016 §2.2）は **in-loop / fail-open** で載る。
/// record が無い・読めない周は claude の rc へ落とす（ADR-0012 §2.1 と同じ向き）＝FailOpen を隠さない。
#[test]
fn runner_question_guard_is_in_loop_fail_open() {
    assert_eq!(
        Guard::RunnerQuestion.polarity(),
        Polarity { timing: Timing::InLoop, on_failure: OnFailure::FailOpen }
    );
    let text = output();
    let expected = "guard=runner-question timing=in-loop on-failure=fail-open boundary=headless::runner::Ending";
    assert!(text.lines().any(|line| line == expected), "一覧に載る: {expected}\n{text}");
    // 上限の中断（runner-stop）の**直後**に並ぶ（行為の流れ = runner の 2 判定）。
    let names: Vec<&str> = text.lines().filter_map(|line| line.split(' ').next()).collect();
    let stop = names.iter().position(|name| *name == "guard=runner-stop");
    let question = names.iter().position(|name| *name == "guard=runner-question");
    assert!(matches!((stop, question), (Some(s), Some(q)) if q == s + 1), "runner-stop の直後: {names:?}");
    assert!(text.lines().last().is_some_and(|line| line.contains(" in-loop=13 ") && line.contains(" fail-open=3")), "集計 +1（.124 の 2 を含む）: {text}");
}

/// 3 クラスを名乗らない契約。
fn contract_with(classes: &[&str]) -> Contract {
    Contract {
        goal: String::new(),
        done: String::new(),
        size: String::new(),
        owner: String::new(),
        disposition: String::new(),
        write_set: Vec::new(),
        verify: Vec::new(),
        req: Vec::new(),
        design: String::new(),
        classes: classes.iter().map(|class| (*class).to_owned()).collect(),
    }
}

/// 承認関門の判定は閉じた enum で、`POLARITY` はその enum の隣の定数である（`s2-07l.108`・
/// C11.2「境界ごとの enum が極性型を運ぶ」）。一覧の boundary は fn ではなく enum を名指し、
/// 判定の値（3 クラスを名乗る契約が未承認なら止める・それ以外は通す）は `.106` から変わらない。
#[test]
fn polarity_approval_gate_boundary_is_an_enum() {
    // 一覧の pointer が enum の型名を指す（型名は erasure 後の path から取る＝字面を 2 面化しない）。
    let type_name = std::any::type_name::<Approval<'_>>();
    let type_path = type_name.split('<').next().unwrap_or_default();
    assert!(
        type_path.ends_with(Guard::Approval.boundary()),
        "boundary は enum を名指す: type={type_name} boundary={}",
        Guard::Approval.boundary()
    );
    assert!(!Guard::Approval.boundary().ends_with("needs_approval"), "fn を指さない");
    // POLARITY は enum の隣の定数として参照でき、一覧が返す値と同じ 1 つの値である。
    let polarity: Polarity = APPROVAL_POLARITY;
    assert_eq!(Guard::Approval.polarity(), polarity, "一覧は境界の定数を返すだけ");
    assert_eq!(polarity, Polarity { timing: Timing::InLoop, on_failure: OnFailure::FailClosed });
    // 判定は網羅 match で受けられる 2 値で、値は不変（.106 の needs_approval と同じ真理表）。
    let declared = contract_with(&["send-out", "consume"]);
    let silent = contract_with(&[]);
    for (contract, approved, stop) in [(&declared, false, true), (&declared, true, false), (&silent, false, false), (&silent, true, false)] {
        let stopped = match Approval::judge(contract, approved) {
            Approval::Granted => false,
            Approval::Required(classes) => {
                assert_eq!(classes, contract.classes.as_slice(), "止める理由は名乗った 3 クラスそのもの");
                true
            }
        };
        assert_eq!(stopped, stop, "classes={:?} approved={approved}", contract.classes);
    }
}

/// land の anchor 同期判定と retire の clean 判定（`s2-07l.124`・.120 lens M4・C11.2 / C16.2）が
/// **in-loop / fail-closed** で載る。値は境界の定数（`ANCHOR_POLARITY` / `WORKTREE_POLARITY`）で一覧は
/// それを返すだけ。一覧に 2 行が値で載り、集計の in-loop が 2 増える（snapshot の字面は pin しない）。
#[test]
fn polarity_lists_land_anchor_sync_and_retire_clean_as_in_loop_fail_closed() {
    let closed = Polarity { timing: Timing::InLoop, on_failure: OnFailure::FailClosed };
    assert_eq!(Guard::LandAnchor.polarity(), closed, "anchor の同期は同期の前に止め、読めない周は揃えない");
    assert_eq!(Guard::LandWorktree.polarity(), closed, "worktree の clean は rebase / move の前に止め、読めない周は止める");
    // 値が境界の定数と一致する（「一覧側に literal を持たない」は値の比較では測れない・lens-124 L1）。
    let anchor: Polarity = ANCHOR_POLARITY;
    let worktree: Polarity = WORKTREE_POLARITY;
    assert_eq!(Guard::LandAnchor.polarity(), anchor, "境界の定数と同じ値");
    assert_eq!(Guard::LandWorktree.polarity(), worktree, "境界の定数と同じ値");
    let text = output();
    for expected in [
        "guard=land-anchor-sync timing=in-loop on-failure=fail-closed boundary=pipe::land::AnchorPlan",
        "guard=land-worktree-clean timing=in-loop on-failure=fail-closed boundary=pipe::land::WorktreeCheck",
    ] {
        assert!(text.lines().any(|line| line == expected), "一覧に載る: {expected}\n{text}");
    }
    // 集計は行数から独立に数えた値と一致し、.115 の 11 から 2 増えている。
    let in_loop = text.lines().filter(|line| line.contains(" timing=in-loop ")).count();
    assert_eq!(in_loop, 13, "in-loop の行数: {text}");
    let summary = text.lines().last().unwrap_or_default();
    assert_eq!(count_of(summary, "in-loop"), Some(13), "集計 +2: {summary}");
    assert_eq!(count_of(summary, "guards"), Some(16), "母集団 +2: {summary}");
}

/// 2 境界の boundary は `pipe::land::` 配下の **型**を名指す（最終 segment が大文字で始まる＝fn 名の形でない）。
/// 公開されている `WorktreeCheck` は erasure 後の型名とも突き合わせる（`AnchorPlan` は crate の外へ出さない）。
/// 一覧では land-main-check の**直後**に anchor → worktree の順で並ぶ（行為の流れ = land の 3 つの止め口）。
#[test]
fn polarity_lists_land_anchor_boundaries_as_enums_right_after_main_check() {
    for guard in [Guard::LandAnchor, Guard::LandWorktree] {
        let boundary = guard.boundary();
        assert!(boundary.starts_with("pipe::land::"), "land の境界: {boundary}");
        let last = boundary.rsplit("::").next().unwrap_or_default();
        assert!(last.starts_with(|c: char| c.is_ascii_uppercase()), "型名の形（fn を指さない）: {boundary}");
    }
    let type_name = std::any::type_name::<WorktreeCheck>();
    assert!(type_name.ends_with(Guard::LandWorktree.boundary()), "boundary は enum を名指す: {type_name}");
    let text = output();
    let names: Vec<&str> = text.lines().filter_map(|line| line.split(' ').next()).collect();
    let main = names.iter().position(|name| *name == "guard=land-main-check");
    let anchor = names.iter().position(|name| *name == "guard=land-anchor-sync");
    let worktree = names.iter().position(|name| *name == "guard=land-worktree-clean");
    assert!(
        matches!((main, anchor, worktree), (Some(m), Some(a), Some(w)) if a == m + 1 && w == a + 1),
        "land-main-check の直後に anchor → worktree: {names:?}"
    );
}

/// worktree の clean 判定は閉じた enum（`WorktreeCheck`・C11.2「境界ごとの enum が極性型を運ぶ」）で、bool は
/// enum から導く。**読めない周は `Unreadable`＝止める**（fail-closed・`.120` までの `is_clean` と同じ真理表）。
/// 歯の名は契約の接頭辞 `polarity_lists_land_anchor_` を持つ（anchor の assert は無い）。
#[test]
fn polarity_lists_land_anchor_retire_check_is_a_closed_enum_that_fails_closed() {
    let Some(dir) = crate::make_tmp_dir() else {
        panic!("tmp dir を作れる");
    };
    // 前提: git repo でない dir（temp_dir が repo の中を指す環境では 1 段目が囲いの repo を読んでしまう）。
    assert!(!dir.join(".git").exists(), "tmp dir は repo でない");
    // git repo でない dir は status を読めない＝Unreadable（clean に読み替えない）。
    let unreadable = WorktreeCheck::judge(&dir);
    assert_eq!(unreadable, WorktreeCheck::Unreadable, "読めない周は Unreadable");
    assert!(!unreadable.is_clean(), "読めない周は畳まない");
    let init = Command::new("git").args(["-C", &dir.display().to_string(), "init", "-q"]).status();
    assert!(init.is_ok_and(|status| status.success()), "git init できる");
    let clean = WorktreeCheck::judge(&dir);
    assert_eq!(clean, WorktreeCheck::Clean, "空の repo は Clean");
    assert!(clean.is_clean(), "Clean だけが進める");
    std::fs::write(dir.join("stray.txt"), "x\n").expect("汚せる");
    let dirty = WorktreeCheck::judge(&dir);
    assert_eq!(dirty, WorktreeCheck::Dirty, "untracked も数える（fail-closed）");
    assert!(!dirty.is_clean(), "Dirty は畳まない");
    let _ = std::fs::remove_dir_all(&dir);
}
