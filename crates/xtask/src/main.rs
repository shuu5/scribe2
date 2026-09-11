//! workspace 保守の内製 task runner（`cargo xtask <subcommand>`）。
//!
//! 依存は std だけ。憲法 C4 の歯（行数 / 比 / manifest parity / lints）を
//! [`check`] が測り、[`genmanifest`] が plugin manifest を NAME から生成し、
//! [`flipcheck`] が TDD の flip（新しい test が古い実装で赤い）を後から確かめる。
//! rc は `main` が返す [`ExitCode`] で表し、helper から `process::exit` は呼ばない。

mod check;
mod check_facts;
mod check_sizes;
mod claude_md;
mod enum_slices;
mod flipcheck;
mod genmanifest;
mod limits;
mod mutantsdiff;
mod non_rust_exec;
mod paths_clean;
mod private_clean;
mod spawn_points;
mod toml_lite;

use std::path::PathBuf;
use std::process::ExitCode;

/// 使い方の 1 行。
const USAGE: &str = "usage: cargo xtask <check|gen-manifest|gen-claude-md> [ROOT] | cargo xtask <flip-check|mutants-diff> --base <ref>";

/// stdout 出力層。stdout へ書くのはこの関数だけである。
#[expect(
    clippy::print_stdout,
    reason = "CLI の stdout 出力をこの 1 関数に閉じるための例外"
)]
fn emit(line: &str) {
    println!("{line}");
}

/// stderr 出力層。stderr へ書くのはこの関数だけである。
#[expect(
    clippy::print_stderr,
    reason = "CLI の stderr 出力をこの 1 関数に閉じるための例外"
)]
fn emit_err(line: &str) {
    eprintln!("{line}");
}

/// `ROOT` 引数を解決する。省略時**のみ** cwd を root とする（■D1）。
fn resolve_root(arg: Option<&str>) -> Result<PathBuf, String> {
    match arg {
        Some(path) => Ok(PathBuf::from(path)),
        None => std::env::current_dir().map_err(|err| format!("cwd を解決できない: {err}")),
    }
}

/// `check` subcommand。違反 0 ならサマリ 1 行、1 件以上なら違反行を並べて rc 1。
fn run_check(root_arg: Option<&str>) -> Result<ExitCode, String> {
    let root = resolve_root(root_arg)?;
    let violations = check::check(&root);
    if violations.is_empty() {
        emit(&check::summary(&root));
        return Ok(ExitCode::SUCCESS);
    }
    for violation in &violations {
        emit(violation);
    }
    Ok(ExitCode::FAILURE)
}

/// `gen-manifest` subcommand。
fn run_gen_manifest(root_arg: Option<&str>) -> Result<ExitCode, String> {
    let root = resolve_root(root_arg)?;
    emit(&genmanifest::generate(&root)?);
    Ok(ExitCode::SUCCESS)
}

/// `gen-claude-md` subcommand。`CLAUDE.md` の生成区間へ憲法の規範文を書く。
fn run_gen_claude_md(root_arg: Option<&str>) -> Result<ExitCode, String> {
    let root = resolve_root(root_arg)?;
    emit(&claude_md::generate(&root)?);
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root_arg = args.get(1).map(String::as_str);
    let tail = args.get(1..).unwrap_or_default();
    let outcome = match args.first().map(String::as_str) {
        Some("check") => run_check(root_arg),
        Some("gen-manifest") => run_gen_manifest(root_arg),
        Some("gen-claude-md") => run_gen_claude_md(root_arg),
        // flip-check だけは rc 2（引数不正）を持つので `Err` → rc 1 経路へ流さない。
        Some("flip-check") => return flipcheck::run(tail),
        // mutants-diff も rc 2（測れなかった）を持つので `Err` → rc 1 経路へ流さない。
        Some("mutants-diff") => return mutantsdiff::run(tail),
        _ => Err(USAGE.to_owned()),
    };
    match outcome {
        Ok(code) => code,
        Err(reason) => {
            emit_err(&reason);
            ExitCode::FAILURE
        }
    }
}

/// `mutants-diff` の歯。**本体は `mutantsdiff.rs`** だが、歯はここ（base に在る file）へ置く。
///
/// 新規 module の中に置くと、その file ごと base に無いので **flip-check が構造的に測れない**
/// （`not-flippable`・新規 module は base に `mod` 宣言ごと存在せず compile されない）。
/// 契約の write-set が `{main,mutantsdiff}.rs（+ tests）` と両方を挙げているのはこのためである。
///
/// **cargo-mutants 本体はここから起動しない**（CI に 10 分の実行を持ち込まない）。測るのは
/// 「1 行の形」と「rc の極性」で、材料は `outcomes.json` の fixture 3 種である。
#[cfg(test)]
mod tests {
    use crate::mutantsdiff::{deny_line_enabled, measure_args, measured, parse_outcomes, verdict, without_outcomes, Counts};
    use std::path::Path;
    use std::process::ExitCode;

    /// cargo-mutants が実際に書く形（生存 0 の周・入れ子に同名 key を持つ）。
    const MISSED_NONE: &str = r#"{
      "outcomes": [
        {"scenario": "Baseline", "summary": "Success", "missed": 99, "total_mutants": 99, "caught": 99, "timeout": 99, "unviable": 99},
        {"scenario": {"Mutant": {"function": {"function_name": "missed"}}}, "summary": "CaughtMutant"}
      ],
      "total_mutants": 23, "missed": 0, "caught": 23, "timeout": 0, "unviable": 0, "success": 1
    }"#;

    /// 生存が 2 件在る周（検出線の主役）。
    const MISSED_TWO: &str = r#"{
      "outcomes": [{"scenario": "Baseline", "summary": "Success"}],
      "total_mutants": 18, "missed": 2, "caught": 12, "timeout": 1, "unviable": 3, "success": 1
    }"#;

    /// 途中で切れた JSON（測れていない）。
    const BROKEN: &str = r#"{"outcomes": [{"scenario": "Baseline""#;

    #[test]
    fn mutants_diff_line_reports_every_count_from_outcomes() {
        let counts = parse_outcomes(MISSED_TWO).expect("fixture は読める");
        assert_eq!(
            counts,
            Counts { total: 18, caught: 12, missed: 2, unviable: 3, timeout: 1 },
            "5 つの数を outcomes.json から読む（範囲は行を組むときに呼び手が渡す）"
        );
        // **1 行の形**まで測る（読み取れても書式が崩れれば報告の額面が読めない）。
        let line = counts.line(&scope_of(PROBE_SCOPE));
        assert_eq!(
            line,
            "mutants-diff: total=18 caught=12 missed=2 unviable=3 timeout=1 scope=probe-pkg-7f3",
            "1 行の形は固定"
        );
        // ★**不成立は撃墜と別**（rc 101 を撃墜に数えないのと同じ極性）。1 行に別々に出る。
        assert!(line.contains("unviable=3"), "測れなかった分が額面に出る");
    }

    /// 行を組む側に置く**実在しない** package 名（`s2-07l.82`）。実在の名を置くと、値が固定
    /// 文字列に化けても歯が通る（fixture の字面と入力の衝突）。
    const PROBE_SCOPE: &str = "probe-pkg-7f3";

    /// 名前から [`crate::mutantsdiff::Scope`] を得る唯一の道＝`-p` へ渡す引数を組むこと。
    fn scope_of(name: &str) -> crate::mutantsdiff::Scope {
        measure_args(Path::new("in.diff"), Path::new("out"), name).1
    }

    /// 変異の行は**何を測ったか**を末尾の `scope=` で名乗り、その値は呼び手が渡した名前
    /// そのものである（`s2-07l.82`・行は出所から切り離されて流通するので限界は行に載せる）。
    ///
    /// 2 つの違う名前で撃つ——行を組む側が値を literal に差し替えると片方が必ず落ちる。
    /// **`-p` へ渡す名前も同じ値**であることを [`measure_args`] で見る（行の `scope=` と実際に
    /// 測った package が別々の読みで食い違わない・lens-82 MEDIUM-1）。
    #[test]
    fn mutants_diff_line_names_the_scope_it_was_given() {
        let other = "probe-pkg-9c1";
        let first = parse_outcomes(MISSED_TWO).expect("fixture は読める").line(&scope_of(PROBE_SCOPE));
        let second = parse_outcomes(MISSED_TWO).expect("fixture は読める").line(&scope_of(other));
        assert!(
            first.ends_with(&format!(" scope={PROBE_SCOPE}")),
            "渡した名前を末尾の scope= に出す: {first}"
        );
        assert!(second.ends_with(&format!(" scope={other}")), "別の名前も同じ形で出す: {second}");
        assert_ne!(first, second, "scope だけが違う 2 行は違う行になる");
        // 既存 5 token の名前・順序は据え置き（scope はその後ろ）。
        let tags: Vec<&str> = first.split(' ').skip(1).filter_map(|t| t.split_once('=').map(|(k, _)| k)).collect();
        assert_eq!(tags, ["total", "caught", "missed", "unviable", "timeout", "scope"], "{first}");
        // 「測る対象が無い」周の行も範囲を名乗る（unmeasured の経路は行を出さないので対象外）。
        let none = without_outcomes(true).expect("rc 0 なら測る対象が無いだけ").line(&scope_of(other));
        assert!(none.ends_with(&format!(" scope={other}")), "{none}");
        // `-p` の直後に来るのは渡した名前そのもの（literal でも core の NAME でもない）。
        // `--in-diff` と `-o` も対のまま在る（落とすと測った結果を読まずに total=0 へ化ける）。
        for scope in [PROBE_SCOPE, other] {
            let (args, _) = measure_args(Path::new("probe.diff"), Path::new("probe-out"), scope);
            let value_after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str);
            assert_eq!(value_after("-p"), Some(scope), "{args:?}");
            assert_eq!(args.iter().filter(|a| *a == "-p").count(), 1, "package は 1 つだけ: {args:?}");
            assert_eq!(value_after("--in-diff"), Some("probe.diff"), "{args:?}");
            assert_eq!(value_after("-o"), Some("probe-out"), "{args:?}");
        }
    }

    #[test]
    fn mutants_diff_reads_only_top_level_counts() {
        // 入れ子に `"missed"` の字面が在っても数えない（file 全体を grep する形との弁別）。
        // ★fixture の入れ子は **key として** 5 つとも `99` を持つ（値の中の字面ではない）。
        // 深さ条件を消した実装はこの 99 を拾うので **1 行が丸ごと変わる**（lens 2026-09-11:
        // 値に字面を置いただけの fixture では深さ条件を消しても緑＝空虚な歯だった）。
        let counts = parse_outcomes(MISSED_NONE).expect("fixture は読める");
        assert_eq!(
            counts.line(&scope_of(PROBE_SCOPE)),
            "mutants-diff: total=23 caught=23 missed=0 unviable=0 timeout=0 scope=probe-pkg-7f3",
            "入れ子の同名 key を 1 つも拾わない"
        );
    }

    #[test]
    fn mutants_diff_requires_every_count_to_be_written() {
        // `missed` が書かれていない outcomes.json を 0 と読むと、「測って 0」と「書かれて
        // いない」が同じ緑になる（lens 2026-09-11 MEDIUM）。
        let partial = r#"{"total_mutants": 3, "caught": 3, "timeout": 0, "unviable": 0}"#;
        assert!(parse_outcomes(partial).is_err(), "5 つ揃わない周は Err");
    }

    #[test]
    fn mutants_diff_does_not_trust_counts_when_the_tool_failed() {
        // ★baseline（変異を当てない木）の test が落ちた周も cargo-mutants は outcomes.json を
        // 書く（`total_mutants=0`）。rc を捨てると、その 1 行は「測る対象が無い」周と **1 bit も
        // 違わない緑**になる＝suite が壊れているときほど門が緑（lens 2026-09-11 H1）。
        assert!(
            measured(Counts::default(), false).is_err(),
            "非 0 で終えて生存も時間切れも無い周は測定として受けない"
        );
        // 非 0 の理由が件数から**説明できる**周（生存が在る）は測定として受ける。
        let survivors = Counts { total: 18, caught: 12, missed: 6, unviable: 0, timeout: 0 };
        assert!(measured(survivors, false).is_ok(), "生存が在る非 0 は正常な測定");
        assert!(measured(Counts::default(), true).is_ok(), "rc 0 は測定として受ける");
    }

    #[test]
    fn mutants_diff_entry_point_refuses_without_base() {
        // 配線（subcommand の入口）にも歯を 1 本置く。`--base` 無しは rc 2（使い方の誤り）。
        assert_eq!(crate::mutantsdiff::run(&[]), ExitCode::from(2), "--base 無しは rc 2");
        assert!(crate::USAGE.contains("mutants-diff"), "usage が subcommand を名指す");
    }

    #[test]
    fn mutants_diff_rejects_broken_outcomes_json() {
        // **壊れた JSON を 0 に化けさせない**。0 を返すと「母集団 0 の緑」が「歯は非空虚」と読まれる。
        let failed = parse_outcomes(BROKEN);
        assert!(failed.is_err(), "壊れた JSON は Err（0 ではない）: {failed:?}");
    }

    #[test]
    fn mutants_diff_records_missed_while_the_line_is_disabled() {
        let counts = parse_outcomes(MISSED_TWO).expect("fixture は読める");
        // enabled=false（現行の manifest）: 生存が在っても **rc 0**＝検出線であって門ではない。
        assert_eq!(verdict(&counts, false), ExitCode::SUCCESS, "検出線は門にしない");
        // enabled=true（C5 の user 裁定が出た後）: 同じ 1 行のまま rc 1 へ倒れる。
        assert_eq!(verdict(&counts, true), ExitCode::FAILURE, "裁定後は門になる");
        // 生存 0 の周は enabled の値に関わらず rc 0。
        let clean = parse_outcomes(MISSED_NONE).expect("fixture は読める");
        assert_eq!(verdict(&clean, true), ExitCode::SUCCESS, "生存 0 は門でも通る");
    }

    #[test]
    fn mutants_diff_reports_zero_when_there_is_nothing_to_measure() {
        // diff に変異が 1 つも無い周（core を触らない便）: cargo-mutants は **rc 0** で終え
        // 出力 dir を作らない。これは「**測る対象が無い**」であって「測れなかった」ではない。
        let counts = without_outcomes(true).expect("道具が rc 0 なら測る対象が無いだけ");
        assert_eq!(
            counts.line(&scope_of(PROBE_SCOPE)),
            "mutants-diff: total=0 caught=0 missed=0 unviable=0 timeout=0 scope=probe-pkg-7f3",
            "母集団を額面に出す（0 件の緑と読み違えないため）"
        );
        // **門でも通る**——測る対象が無い周を赤にすると、docs-only 便が恒久 FAIL になる。
        assert_eq!(verdict(&counts, true), ExitCode::SUCCESS, "測る対象が無い周は赤にしない");
    }

    #[test]
    fn mutants_diff_is_unmeasured_when_the_tool_fails() {
        // 道具が非 0 で終わった周は「**測れなかった**」＝rc 2 の側に残す（0 に化けさせない）。
        let failed = without_outcomes(false);
        assert!(failed.is_err(), "道具の異常終了は Err（total=0 の緑にしない）: {failed:?}");
    }

    #[test]
    fn mutants_diff_takes_the_deny_polarity_from_the_manifest_row() {
        // **値は manifest に 1 つ**（憲法 C1）。道具の側に既定を持たない。
        let disabled = "[[rule]]\nid = \"R-C12-1\"\nenabled = false\n\n[[rule]]\nid = \"R-C13-1\"\nenabled = true\n";
        assert!(!deny_line_enabled(disabled), "R-C12-1 の値を読む（隣の行に釣られない）");
        let enabled = "[[rule]]\nid = \"R-C12-1\"\nenabled = true\n";
        assert!(deny_line_enabled(enabled), "enabled=true は門");
        // 行が無い周は**門にしない**側へ倒す（無い規則を勝手に発効させない）。
        assert!(!deny_line_enabled("[[rule]]\nid = \"R-C4-1\"\nenabled = true\n"), "行が無ければ門にしない");
    }
}
