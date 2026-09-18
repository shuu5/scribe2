//! rules の歯（doctor の host の面・壊れた host の面での `seat tick`・壊れた `--rules`・設計
//! docs/design/seat-roles.md §7 / rules-manifest.md §4.2 / §5・接頭辞 `rules_host_` / `seat_rules_`・
//! nested module `rules_prop`）。
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

// ─────────────────────────── 壊れた --rules（s2-07l.154） ───────────────────────────

/// 欠陥 3 件の manifest（未知 kind / `ruling` 欠け / id 重複）。`line=` は key の行ではなく
/// `[[rule]]` の見出し行（3・11・26）。id 重複の 2 行は他に欠陥の無い完全な行にする
/// （欠陥のある行は `rows` に載らず、重複検査の母集団から消える）。
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
    "\n",
    "[[rule]]\n",
    "id = \"seat.ledger_timeout_s\"\n",
    "kind = \"LedgerTimeoutS\"\n",
    "value = 30\n",
    "enabled = true\n",
    "ruling = \"user 2026-09-12T02:01Z\"\n",
    "ruled_at = \"2026-09-12\"\n",
    "\n",
    "[[rule]]\n",
    "id = \"seat.ledger_timeout_s\"\n",
    "kind = \"LedgerTimeoutS\"\n",
    "value = 30\n",
    "enabled = true\n",
    "ruling = \"user 2026-09-12T02:01Z\"\n",
    "ruled_at = \"2026-09-12\"\n",
);

/// [`BROKEN_RULES`] の 3 欠陥が名指す行（見出し行）。
const BROKEN_LINES: [&str; 3] = [" line=3", " line=11", " line=26"];

/// 主に撃つ口（`--rules` を受ける口のうち代表 1 つ）。
const RULES_FACE: &str = "externalize";

/// `--rules` を受ける seat の口の全体（ADR-0045 §2 (2) で tick / cycle の 2 口が消え 2 口になった）。
const RULES_FACES: [&str; 2] = ["externalize", "rebrief"];

/// 壊れた `--rules` の歯の席名。
const RULES_TARGET: &str = "seatrules";

/// 壊れた `--rules` の歯の場所（tmp・空の置き場・rules の fixture）。
struct RulesPlace {
    /// tmp の root。
    dir: PathBuf,
    /// 置き場（空で作る＝断りの周に 1 file も増えないことを測る）。
    state: PathBuf,
    /// `--rules` に渡す path。
    rules: String,
}

/// 場所を作り、`body` を rules の fixture として書く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rules_place(body: &str) -> RulesPlace {
    let dir = tmp();
    let state = dir.join("state");
    for sub in [dir.join("wm"), state.clone()] {
        fs::create_dir_all(sub).expect("dir を作れる");
    }
    let rules = fixture(&dir, "rules.toml", body);
    RulesPlace { dir, state, rules }
}

/// stderr を行へ切る。
fn stderr_lines(out: &Output) -> Vec<String> {
    stderr_of(out).lines().map(str::to_owned).collect()
}

/// 同じ fixture への `rules validate --rules` の stderr の行（同じ描画の再利用を測る基準）。
fn validate_lines(rules: &str) -> Vec<String> {
    Command::new(bin())
        .args(["rules", "validate", "--rules", rules])
        .output()
        .map(|out| stderr_lines(&out))
        .unwrap_or_default()
}

/// 口 `face` を `--rules` 付きで 1 回撃つ（判定の前に断る周なので、plan / directives は名前だけ渡す）。
fn run_face(place: &RulesPlace, face: &str) -> Output {
    let wm = place.dir.join("wm").display().to_string();
    let state = place.state.display().to_string();
    let anchor = place.dir.display().to_string();
    let plan = place.dir.join("plan.md").display().to_string();
    let mut args = vec![face, "--target", RULES_TARGET, "--wm-dir", wm.as_str(), "--state-dir", state.as_str()];
    match face {
        "externalize" => args.extend(["--anchor", anchor.as_str(), "--plan", plan.as_str(), "--directives", plan.as_str()]),
        "rebrief" => args.extend(["--anchor", anchor.as_str()]),
        _ => {}
    }
    args.extend(["--rules", place.rules.as_str()]);
    run_seat(&args)
}

/// 口ごとの既存の断り行（行が無い周に出すものと同じ描画）。
fn judged_of(face: &str, _state: &Path) -> Vec<String> {
    use vessel::seat::{externalize, rebrief};
    match face {
        "externalize" => externalize::render_refused(&externalize::ExternalizeError::NoRule),
        _ => vec![rebrief::render_unavailable(rebrief::RebriefError::NoRule)],
    }
}

/// (a) 欠陥 3 件の manifest を `seat externalize --rules` へ渡すと、defect を `rules validate` と同じ行で
/// 全件並べ、末尾に既存の判定行 1 行を残して rc 1（設計 rules-manifest.md §4.2 / §5・seat-autonomy.md §3）。
#[test]
fn seat_rules_broken_manifest_lists_every_defect() {
    let place = rules_place(BROKEN_RULES);
    let want = validate_lines(&place.rules);
    assert_eq!(want.len(), 3, "基準の rules validate は 3 件: {want:?}");
    let out = run_face(&place, RULES_FACE);
    let lines = stderr_lines(&out);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{lines:?}");
    assert_eq!(out.stdout.len(), 0, "断りの周は stdout 0 byte");
    assert_eq!(lines.len(), 3 + 1, "defect 3 行 + 判定行 1 行: {lines:?}");
    assert_eq!(lines.get(..3), Some(want.as_slice()), "defect 行は rules validate と 1 byte 同じ");
    assert_eq!(lines.last().map(String::as_str), judged_of(RULES_FACE, &place.state).first().map(String::as_str), "判定行は消さない");
    for at in BROKEN_LINES {
        let hits = lines.iter().take(3).filter(|line| line.ends_with(at)).count();
        assert_eq!(hits, 1, "{at} は 1 回ずつ: {lines:?}");
    }
    assert!(lines.iter().take(3).all(|line| line.contains("line=")), "{lines:?}");
    assert!(!place.state.join("seat").exists(), "判定へ入らない（置き場の seat/ を作らない）");
    let left = tree_stat(&place.state);
    assert!(left.is_empty(), "置き場は空のまま: {left:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) 読めるが行を持たない manifest は既存どおり判定行 1 行だけ（「壊れている」と「行が無い」の弁別の
/// もう片側・全部を defect 列挙へ倒す実装を落とす）。
#[test]
fn seat_rules_absent_rows_still_refuse_with_one_line() {
    let place = rules_place("schema = 1\n");
    let out = run_face(&place, RULES_FACE);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stderr={}", stderr_of(&out));
    assert_eq!(stderr_lines(&out), judged_of(RULES_FACE, &place.state), "行が無い周は既存の断りのまま");
    assert_eq!(stderr_lines(&out).iter().filter(|line| line.contains("line=")).count(), 0);
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) `--rules` を受ける 4 口のどれも、壊れた manifest の defect を全件並べ、末尾に口の既存の断り行を
/// 残して rc 1 で断る（置き場へ 1 file も書かない）。`.ok()?` が 1 口でも残れば落ちる。
#[test]
fn seat_rules_broken_manifest_refuses_on_every_seat_face() {
    let mut refused = Vec::new();
    for face in RULES_FACES {
        let place = rules_place(BROKEN_RULES);
        let want = validate_lines(&place.rules);
        let out = run_face(&place, face);
        let lines = stderr_lines(&out);
        let judged = judged_of(face, &place.state);
        assert_eq!(want.len(), 3, "{face}（2 口のうち）: 基準 {want:?}");
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{face}（2 口のうち）: {lines:?}");
        assert_eq!(lines.len(), 3 + 1, "{face}（2 口のうち）: {lines:?}");
        assert_eq!(lines.get(..3), Some(want.as_slice()), "{face}（2 口のうち）");
        assert!(lines.iter().take(3).all(|line| line.contains("line=")), "{face}（2 口のうち）: {lines:?}");
        assert_eq!(lines.get(3..), Some(judged.as_slice()), "{face}（2 口のうち）: 末尾は既存の断り行");
        let left = tree_stat(&place.state);
        assert!(left.is_empty(), "{face}（2 口のうち）: 置き場は空のまま {left:?}");
        refused.push(face);
        fs::remove_dir_all(&place.dir).ok();
    }
    assert_eq!(refused, RULES_FACES, "2 / 2 の口が defect を全件並べて断る");
}

/// (d) 欠陥 k 個（k ∈ 1..=5）の manifest で、`seat externalize --rules` の先頭 k 行は `rules validate` の行と
/// 多重集合で一致し、末尾に判定行 1 行が残る。性質の歯は通常 `prop.rs`（純関数の面）に置くが、
/// 本件は外形（binary の stderr）の性質なのでこの file に置き、verify 行を 1 本に保つ。
mod rules_prop {
    use super::{judged_of, rules_place, run_face, stderr_lines, validate_lines, RULES_FACE};
    use proptest::prelude::*;
    use proptest::test_runner::Config;
    use std::fs;

    /// 1 行に当てる欠陥（1 行につきちょうど 1 つ・loader が 1 欠陥 1 行で報告する形だけ）。
    ///
    /// 型不一致は `enabled` / `kind` にだけ当てる: `id` の型違いは「id が空である」を追撃して
    /// 2 行になり、`value` は未知 kind の行では報告されない。
    #[derive(Debug, Clone, Copy)]
    enum Defect {
        /// 未知 kind。
        UnknownKind,
        /// `ruling` 欠け。
        NoRuling,
        /// `enabled` が bool でない。
        EnabledType,
        /// `kind` が文字列でない。
        KindType,
    }

    /// 欠陥の生成。
    fn any_defect() -> impl Strategy<Value = Defect> {
        prop_oneof![Just(Defect::UnknownKind), Just(Defect::NoRuling), Just(Defect::EnabledType), Just(Defect::KindType)]
    }

    /// 欠陥 1 つを持つ `[[rule]]` 1 行（id は行ごとに別＝重複を混ぜない）。
    fn row(at: usize, defect: Defect) -> String {
        let (kind, enabled, ruling) = match defect {
            Defect::UnknownKind => ("\"NoSuchKind\"", "true", "ruling = \"user 2026-09-12T02:01Z\"\n"),
            Defect::NoRuling => ("\"LedgerTimeoutS\"", "true", ""),
            Defect::EnabledType => ("\"LedgerTimeoutS\"", "\"yes\"", "ruling = \"user 2026-09-12T02:01Z\"\n"),
            Defect::KindType => ("1", "true", "ruling = \"user 2026-09-12T02:01Z\"\n"),
        };
        format!("\n[[rule]]\nid = \"seat.row_{at}\"\nkind = {kind}\nvalue = 30\nenabled = {enabled}\n{ruling}ruled_at = \"2026-09-12\"\n")
    }

    /// case ごとに binary を 2 回起動するので 16 に絞り、反例の永続化を切る。
    fn config() -> Config {
        Config {
            cases: 16,
            failure_persistence: None,
            ..Config::default()
        }
    }

    proptest! {
        #![proptest_config(config())]

        #[test]
        fn seat_rules_broken_manifest_lists_k_defects(defects in prop::collection::vec(any_defect(), 1..=5)) {
            let rows: String = defects.iter().enumerate().map(|(at, defect)| row(at, *defect)).collect();
            let place = rules_place(&format!("schema = 1\n{rows}"));
            let mut want = validate_lines(&place.rules);
            let lines = stderr_lines(&run_face(&place, RULES_FACE));
            fs::remove_dir_all(&place.dir).ok();
            let k = defects.len();
            prop_assert_eq!(want.len(), k);
            prop_assert_eq!(lines.len(), k + 1);
            let mut head: Vec<String> = lines.iter().take(k).cloned().collect();
            prop_assert!(head.iter().all(|line| line.contains("line=")));
            head.sort();
            want.sort();
            prop_assert_eq!(head, want);
            prop_assert_eq!(lines.last().cloned(), judged_of(RULES_FACE, &place.state).last().cloned());
        }
    }
}
