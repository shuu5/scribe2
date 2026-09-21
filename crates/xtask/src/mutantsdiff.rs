//! `cargo xtask mutants-diff --base <ref>`: diff の中の変異を **1 行で記録する**
//! （[ADR-0009 §2.3] の検出線・rules 行 `R-C12-1`）。
//!
//! **記録であって門ではない**。`R-C12-1` の `enabled` が偽である限り、生存（missed）が
//! 在っても rc 0 を返す——deny 化は user 裁定（憲法 C5）が要る変更で、この道具の側で
//! 決めてよいことではない。`enabled` が真になった周は同じ 1 行のまま rc 1 へ倒れる。
//!
//! **「測れなかった」を 0 に化けさせない**のがもう 1 つの要点である。cargo-mutants が
//! 入っていない環境で `missed=0` を出すと、母集団 0 の緑が「歯は非空虚」と読まれる。
//! 道具が無い周は rc 2 と stderr 1 行で落ちる（`unviable` も 1 行に出るので、
//! 「測れた / 測れていない」が報告の額面で読める）。
//!
//! [ADR-0009 §2.3]: ../../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html

use crate::toml_lite;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

/// 1 行に写す件数。**何を測ったか**（scope）は field で持たず [`Counts::line`] の引数で受ける
/// ＝範囲を名乗らない行は組めない（lens-82 MEDIUM-2: 空の scope を表現可能にしない）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// 生成された変異の総数。
    pub total: u64,
    /// 歯が落とした数。
    pub caught: u64,
    /// **生き残った**数（検出線の主役）。
    pub missed: u64,
    /// compile できず成立しなかった数。**撃墜に数えない**。
    pub unviable: u64,
    /// 時間切れの数。
    pub timeout: u64,
}

impl Counts {
    /// stdout へ出す 1 行。既存 5 token の名前・順序・書式は据え置き、末尾に `scope=` を足す。
    ///
    /// `scope` = 測った範囲＝`cargo mutants -p` へ**実際に渡した**名前（bd `s2-07l.82`）。行は
    /// 出所から切り離されて流通する（bead notes / PR 本文 / CI log から切り出される）ので、
    /// 「core package だけを測った」という限界は報告でなく**行そのもの**に載せる。値は行を組む側の
    /// literal ではなく [`Scope`]（[`measure_args`] だけが作る）で受ける＝`-p` へ渡した名前と
    /// 別の値を行に書く形は compile できない。将来 diff が触った package を並べて測る形（案 (b)）
    /// になっても同じ引数で表せる。
    ///
    /// 末尾の `teeth=<-|n>`（設計 gate-cost.md §34 約束 3・行 z）は mutant の test に掛けた filter の
    /// 語の数（`--teeth` 無し = `-`・空 = `0`）。これも [`Scope`] から読む＝`-E` に渡した語と別の数を
    /// 行に書く形は組めない。
    pub fn line(&self, scope: &Scope) -> String {
        format!(
            "mutants-diff: total={} caught={} missed={} unviable={} timeout={} scope={} teeth={}",
            self.total,
            self.caught,
            self.missed,
            self.unviable,
            self.timeout,
            scope.name(),
            scope.teeth()
        )
    }
}

pub use scope::{measure_args, Pace, Scope};

/// [`Scope`] を作れる場所を **この module の内側だけ**にする。親（[`run`] を含む）からは field が
/// 見えないので、`-p` へ渡した名前と別の値で行を組む形は compile できない（lens-82 再確認の残余:
/// 同じ module に置くと private field でも呼び側が literal から組めた・実測）。
mod scope {
    use std::path::Path;

    /// 測った範囲＝`cargo mutants -p` へ**実際に渡した**名前。作れるのは [`measure_args`] だけ
    /// （field は private・`Default` も持たない）ので、行の `scope=` と実際に測った package が
    /// 別々の読みで食い違う形は型で組めない（lens-82 MEDIUM-1）。
    ///
    /// 2 つ目の field は mutant の test に掛けた filter の語の数（`None` = `--teeth` 無し・§34）。
    #[derive(Debug, PartialEq, Eq)]
    pub struct Scope(String, Option<usize>);

    impl Scope {
        /// 行に写す名前。
        pub fn name(&self) -> &str {
            &self.0
        }

        /// 行に写す `teeth=` の値（無し = `-`・空 = `0`・語の数）。
        pub fn teeth(&self) -> String {
            self.1.map_or_else(|| "-".to_owned(), |count| count.to_string())
        }
    }

    /// cargo-mutants へ渡す数 3 つ（並列度・thread 数・mutant の test の timeout 秒）。引数を
    /// 束ねるのは clippy の `too-many-arguments-threshold`（5）の内に収めるためで、意味は 3 つの
    /// 独立な値のまま（どれも歯が別々に pin する）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Pace {
        /// `--jobs`（器の受付が決めた値・§3.3）。
        pub jobs: u64,
        /// `--test-threads`（器の受付が決めた値・§31 約束 7）。
        pub threads: u64,
        /// `--timeout`（自前の baseline の壁時計から導いた秒・§33 約束 4）。
        pub timeout_s: u64,
    }

    /// `cargo` へ渡す引数（`cargo` の直後から）と、その `-p` に載せた [`Scope`]。
    ///
    /// `--in-diff <diff>` / `-p <scope>` / `-o <out>` / `--jobs <n>` はそれぞれ隣り合う対で、
    /// 歯が対のまま見る（`-o` を落とすと測った結果を読まずに `total=0` へ化ける・lens-82
    /// 再確認 MEDIUM-4）。
    ///
    /// **並列度も thread 数もこの道具は値を持たない**（設計 gate-cost.md §3.3 / §31 約束 7）。器が
    /// 受付で導いた実効値を宣言 file の `{jobs}` / `{threads}` 経由で受け取り、cargo-mutants の
    /// `--jobs` と test binary の `--test-threads` へそのまま渡すだけである。
    ///
    /// **mutant の test は fail-fast**（設計 gate-cost.md §33・行 y）: `--baseline skip` で
    /// cargo-mutants の baseline を撃たず（全数の baseline は [`super::baseline_args`] が道具の外で
    /// 1 回撃つ）、`--timeout <T>` にその壁時計から導いた秒を渡す（skip した周に固定 300 秒へ
    /// 倒れさせない）。1 つ目の `--` の後ろに `--no-fail-fast` は置かない＝撃墜は最初に落ちた
    /// binary で決まる。
    ///
    /// 2 つ目の `--` の後ろ `--test-threads <t>` は cargo test が test binary へ渡す引数
    /// （設計 gate-cost.md §22・行 m）。cargo-mutants は 1 つ目の `--` より後ろを 2 つ目の `--`
    /// ごと逐語で cargo test へ渡す。libtest の既定は core 数の thread なので、`--jobs` の各 job
    /// が全 core に広がり gate 1 本で jobs × cores 並列になる（2026-09-16 の load 57 / 16 core）。
    /// `t` の導出（cores と `gate.mutants_jobs` から）は器の受付が持つ（§31・行 w）——道具が
    /// `cores / jobs` で導くと、受け付けた枠が上限より小さい周に job あたりの値段が上がり、gate 2 本で
    /// core の 2 倍の thread を作る。
    ///
    /// **`teeth` が `Some` の周は mutant の test を nextest で走らせる**（設計 gate-cost.md §34 約束 1・
    /// 行 z）: `--timeout <T>` の後ろに `--test-tool nextest`、1 つ目の `--` の後ろは
    /// `-E <式> --test-threads <t>` だけ（nextest は引数を逐語で受けるので 2 つ目の `--` は無い）。
    /// 式は [`super::nextest_expr`]。`None` の周は上の §33 の形を 1 語も変えない。
    pub fn measure_args(
        diff: &Path,
        out: &Path,
        scope: &str,
        pace: Pace,
        teeth: Option<&[String]>,
    ) -> (Vec<String>, Scope) {
        let mut args: Vec<String> = ["mutants", "--in-diff"].iter().map(|s| (*s).to_owned()).collect();
        args.push(diff.display().to_string());
        args.push("-p".to_owned());
        args.push(scope.to_owned());
        args.extend(["--no-shuffle", "--copy-vcs", "true", "-o"].iter().map(|s| (*s).to_owned()));
        args.push(out.display().to_string());
        args.push("--jobs".to_owned());
        args.push(pace.jobs.to_string());
        args.extend(["--baseline", "skip", "--timeout"].iter().map(|s| (*s).to_owned()));
        args.push(pace.timeout_s.to_string());
        match teeth {
            Some(words) => {
                args.extend(["--test-tool", "nextest", "--", "-E"].iter().map(|s| (*s).to_owned()));
                args.push(super::nextest_expr(words));
                args.push("--test-threads".to_owned());
            }
            None => args.extend(["--", "--", "--test-threads"].iter().map(|s| (*s).to_owned())),
        }
        args.push(pace.threads.to_string());
        (args, Scope(scope.to_owned(), teeth.map(<[String]>::len)))
    }
}

/// 数えた結果に対する rc。**極性は manifest の `R-C12-1` 行が決める**。
pub fn verdict(counts: &Counts, deny: bool) -> ExitCode {
    if deny && counts.missed > 0 {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `outcomes.json` の **top-level の数値 field**だけを読む（json subset を自前で・外部 crate を
/// 足さない＝A3 と C13 予算の外に居る）。
///
/// 入れ子（`outcomes[]` の各要素や `phase_results`）に同名の key が現れても拾わないよう、
/// **深さ 1 の key だけ**を見る。file 全体を grep する形にすると、値の文字列に同じ字面が
/// 現れただけで数が動く。
pub fn parse_outcomes(json: &str) -> Result<Counts, String> {
    let mut counts = Counts::default();
    // **5 つとも在ることを要求する**。1 つでも欠けたら Err＝欠けた面を 0 と読むと、
    // 「measured 0」と「書かれていない」が同じ緑になる（lens 2026-09-11 MEDIUM）。
    let mut seen = 0_u8;
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for (at, ch) in json.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            if depth == 1 {
                if let Some(rest) = json.get(at..) {
                    if let Some((key, value)) = top_level_number(rest) {
                        match key {
                            "total_mutants" => {
                                counts.total = value;
                                seen = seen.saturating_add(1);
                            }
                            "caught" => {
                                counts.caught = value;
                                seen = seen.saturating_add(1);
                            }
                            "missed" => {
                                counts.missed = value;
                                seen = seen.saturating_add(1);
                            }
                            "unviable" => {
                                counts.unviable = value;
                                seen = seen.saturating_add(1);
                            }
                            "timeout" => {
                                counts.timeout = value;
                                seen = seen.saturating_add(1);
                            }
                            _ => {}
                        }
                    }
                }
            }
            in_string = true;
        } else if ch == '{' || ch == '[' {
            depth = depth.saturating_add(1);
        } else if ch == '}' || ch == ']' {
            depth = depth.saturating_sub(1);
        }
    }
    if seen == 5 {
        return Ok(counts);
    }
    Err(format!(
        "outcomes.json の 5 つの数のうち {seen} つしか読めない（測れていない）"
    ))
}

/// **道具の rc を捨てない**（lens 2026-09-11 H1）。
///
/// cargo-mutants は baseline（変異を当てない木）の test が落ちた周も `outcomes.json` を書く
/// （`total_mutants=0`）。rc を見ずに件数だけ読むと、その 1 行は「測る対象が無い」周と
/// **1 bit も違わない緑**になる——suite が壊れているときほど門が緑になる、最悪の極性である。
///
/// 判定に rc の表は要らない: **非 0 の理由が件数から説明できる周だけ**を測定として受ける
/// （生存や時間切れが在れば cargo-mutants は非 0 で終える）。説明できない非 0 は
/// 「測れなかった」＝rc 2 に残す。
pub fn measured(counts: Counts, tool_succeeded: bool) -> Result<Counts, String> {
    if tool_succeeded || counts.missed > 0 || counts.timeout > 0 {
        return Ok(counts);
    }
    Err("cargo mutants が非 0 で終えたが生存も時間切れも無い（baseline が落ちた疑い・測れていない）".to_owned())
}

/// `outcomes.json` が無かった周の扱い（user 裁定を受けた planner 裁定 2026-09-11）。
///
/// **「測る対象が無い」と「測れなかった」は別の事実である**。cargo-mutants は diff に変異が
/// 1 つも無い周を **rc 0 + `No mutants to filter`** で終え、出力 dir を作らない（実測 2026-09-11）。
/// これを「測れなかった」に潰すと、core を触らない便（docs-only / xtask 便）は common-verify に
/// 載せた瞬間から**恒久 FAIL**になる——**偽の赤は偽の緑と同じく毒**で、赤が常態化すると本当の
/// 赤が読めなくなる。逆に道具の異常終了を rc 0 にすれば、道具の不在が緑に化ける。
///
/// ゆえに **rc 0 の周だけ** `total=0` の 1 行（母集団を額面に出す）へ倒し、非 0 は `Err`＝rc 2 に残す。
pub fn without_outcomes(tool_succeeded: bool) -> Result<Counts, String> {
    if tool_succeeded {
        return Ok(Counts::default());
    }
    Err("outcomes.json を読めない（測れていない）".to_owned())
}

/// rc 2 の周に stderr へ写す `baseline.log` の末尾の行数（設計 pipeline.md §5.3 の
/// 「stderr の末尾 20 行」と同じ値・rules 行ではない）。
pub const BASELINE_TAIL_LINES: usize = 20;

/// `baseline.log` の末尾 `lines` 行（順序はそのまま）。空なら「空」だと 1 行で名乗る
/// ——空 file を空文字で写すと「末尾が無い」と「写していない」が同じ字面になる。
pub fn baseline_tail(log: &str, lines: usize) -> String {
    if log.trim().is_empty() {
        return "baseline.log は空".to_owned();
    }
    let mut tail: Vec<&str> = log.lines().rev().take(lines).collect();
    tail.reverse();
    tail.join("\n")
}

/// stderr へ写す見出し（この行の次から `baseline.log` の末尾）。
const BASELINE_TAIL_HEADING: &str = "mutants-diff: baseline.log の末尾:";

/// 「測れなかった」周（`Err`）**だけ**に `baseline.log` の末尾を添える（憲法 C10: 測れなかった
/// 理由を測定値として残す・設計 pipeline.md §5.3: 診断は `verify.stderr.log` に載る）。
///
/// `s2-07l.329` run 1 の rc 2 は理由の 1 行だけが残り、原因（baseline の 1 本の歯が落ちた）は
/// 退避 worktree の `baseline.log` を手で開くまで読めなかった。**測れた周（`Ok`）には何も
/// 足さない**——緑の行に診断を残すと、緑と赤の stderr が同じ形になる。`tail` は `Err` の周に
/// だけ呼ぶ（file を読むのは理由が立った後）。
pub fn diagnosed(outcome: Result<Counts, String>, tail: impl FnOnce() -> String) -> Result<Counts, String> {
    outcome.map_err(|reason| format!("{reason}\n{BASELINE_TAIL_HEADING}\n{}", tail()))
}

/// baseline の build（`cargo` の直後から）。秒は測らない（設計 gate-cost.md §33 約束 1 (i)）。
pub fn baseline_build_args(scope: &str) -> Vec<String> {
    ["test", "-p", scope, "--no-run"].iter().map(|s| (*s).to_owned()).collect()
}

/// baseline の test（`cargo` の直後から・設計 gate-cost.md §33 約束 1 (ii)）。**全数**を走らせる
/// ＝`--no-fail-fast` は 1 つ目の `--` の前（cargo test 自身の flag）に置く。§19 の理由（flaky
/// 1 本で落ちた歯の全数を名指せない）は baseline にだけ当たるので、ここにだけ残す。
pub fn baseline_args(scope: &str, threads: u64) -> Vec<String> {
    let mut args: Vec<String> = ["test", "-p", scope, "--no-fail-fast", "--", "--test-threads"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    args.push(threads.to_string());
    args
}

/// mutant の test の timeout の床（秒・cargo-mutants v27 の既定と同じ値・rules 行ではない）。
const TIMEOUT_FLOOR_S: u64 = 20;

/// baseline の test の壁時計に掛ける倍率（cargo-mutants v27 の既定と同じ値）。
const TIMEOUT_MULTIPLIER: u64 = 5;

/// baseline の test の壁時計（ミリ秒の整数）から mutant の test の timeout 秒を導く:
/// `T = max(20, ceil(5 × ms / 1000))`（設計 gate-cost.md §33 約束 4）。**整数演算**で導く
/// ——float の丸めで 309 / 310 が揺れる形を作らない。
pub fn mutant_timeout_s(baseline_ms: u64) -> u64 {
    baseline_ms
        .saturating_mul(TIMEOUT_MULTIPLIER)
        .div_ceil(1000)
        .max(TIMEOUT_FLOOR_S)
}

/// 自前の baseline の判定（設計 gate-cost.md §33 約束 2）。rc ≠ 0 の周は cargo-mutants を起こさず
/// 「測れなかった」（rc 2）へ倒し、理由行の後ろに**自前の** `baseline.log` の末尾を添える
/// （[`diagnosed`] と同じ形・読む file の出所だけが変わる）。rc 0 の周は `Ok`＝cargo-mutants へ進む。
pub fn baseline_judged(succeeded: bool, log: &str) -> Result<(), String> {
    if succeeded {
        return Ok(());
    }
    diagnosed(
        Err("baseline の cargo test が非 0 で終えた（測れていない）".to_owned()),
        || baseline_tail(log, BASELINE_TAIL_LINES),
    )
    .map(|_| ())
}

/// `"key": <整数>` の形を 1 つ読む。数でなければ `None`（文字列や object は数えない）。
fn top_level_number(rest: &str) -> Option<(&str, u64)> {
    let after_quote = rest.strip_prefix('"')?;
    let (key, tail) = after_quote.split_once('"')?;
    let value = tail.trim_start().strip_prefix(':')?.trim_start();
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok().map(|number| (key, number))
}

/// rules manifest の `R-C12-1` 行の `enabled`。**この道具は値を持たない**（憲法 C1: 規則値は
/// manifest に 1 つ）。行が無い周は `Some(false)`＝「門にしない」側へ倒す——無い規則を勝手に
/// 発効させない。
///
/// **行は在るのに `enabled` が読めない周は `None`**（`s2-07l.80`・裁定 id
/// `user 2026-09-11T23:59Z`）。`false` に化けさせると、極性を決める行が壊れている周ほど
/// 門が緩む側へ黙って倒れる——「不発効だと書かれている」と「書かれていない」は別の事実で、
/// 後者は**測れていない**（呼び手は rc 2 で止める）。`enabled` は manifest の必須 key なので、
/// 正しい manifest でこの枝は起きない。
///
/// section は [`toml_lite::sections`] で読む。array-of-tables の header は `[` を 1 つだけ
/// 剥がした `[rule` になる（`limits.rs` の歯と同じ字面）。
pub fn deny_line_enabled(manifest: &str) -> Option<bool> {
    for (header, pairs) in toml_lite::sections(manifest) {
        if header != RULE_HEADER {
            continue;
        }
        let id = pairs
            .iter()
            .find(|(key, _)| *key == "id")
            .and_then(|(_, value)| toml_lite::quoted(value));
        if id.as_deref() != Some(DENY_LINE_ID) {
            continue;
        }
        return match pairs.iter().find(|(key, _)| *key == "enabled") {
            Some((_, value)) if value.trim() == "true" => Some(true),
            Some((_, value)) if value.trim() == "false" => Some(false),
            // 行は在るのに bool として読めない（欠落・型違い）＝極性が決まらない。
            _ => None,
        };
    }
    Some(false)
}

/// array-of-tables の section header の字面（`sections` は `[` を 1 つだけ剥がす）。
const RULE_HEADER: &str = "[rule";

/// 極性を決める rules 行の id。
const DENY_LINE_ID: &str = "R-C12-1";

/// flag の値を取る（値が無ければ `None`）。
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|arg| arg == name)?;
    args.get(at.saturating_add(1))
        .map(String::as_str)
        .filter(|value| !value.starts_with("--"))
}

/// 使い方（rc 2 の 1 行）。
const USAGE: &str = "usage: cargo xtask mutants-diff --base <ref> [--jobs <n>] [--threads <t>] [--teeth <語,…|->]";

/// `--teeth` の字面が読めない周の理由（閉じた 1 つ・設計 gate-cost.md §34 約束 1）。
pub const TEETH_MALFORMED: &str =
    "mutants-diff: --teeth の語は , 区切りの [A-Za-z0-9_]+ か - だけ（測れていない・rc 2）";

/// `--teeth <語列>` を読む（設計 gate-cost.md §34 約束 1・行 z）。
///
/// flag が無い周は `Ok(None)`（§33 の形のまま撃つ）、`-` は `Ok(Some(空))`、それ以外は `,` で割った
/// 語が全部 `[A-Za-z0-9_]+` の周だけ `Ok(Some(語))`。値の無い flag・空の語・それ以外の字を含む語は
/// [`TEETH_MALFORMED`] の `Err`＝**測らずに rc 2**（語を黙って落とすと filter が広がる／狭まる側へ
/// 静かに倒れる・fail-closed）。
pub fn teeth_of(args: &[String]) -> Result<Option<Vec<String>>, String> {
    if !args.iter().any(|arg| arg == "--teeth") {
        return Ok(None);
    }
    let value = flag(args, "--teeth").ok_or_else(|| TEETH_MALFORMED.to_owned())?;
    if value == "-" {
        return Ok(Some(Vec::new()));
    }
    let words: Vec<String> = value.split(',').map(str::to_owned).collect();
    let well_formed = |word: &String| !word.is_empty() && word.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
    if words.iter().all(well_formed) {
        return Ok(Some(words));
    }
    Err(TEETH_MALFORMED.to_owned())
}

/// mutant の test に掛ける nextest の filter の式: `kind(lib) | kind(bin)`、語が在れば
/// `| test(/^(語1|語2)/)` を足す（e2e の歯は契約が名指した語で始まる分だけ・§34 約束 1）。
pub fn nextest_expr(words: &[String]) -> String {
    let base = "kind(lib) | kind(bin)";
    if words.is_empty() {
        return base.to_owned();
    }
    format!("{base} | test(/^({})/)", words.join("|"))
}

/// `--jobs` / `--threads` を渡されなかった周の並列度と thread 数（設計 gate-cost.md §2「止めない、縮退する」）。
///
/// **1 は常に許される**（従来と同じ費用）。器を通さずに人が撃つ周と、受付が枠を取れなかった
/// 周が同じ値になる形で、道具の側に「速い既定」を持たない。
const JOBS_FLOOR: u64 = 1;

/// `--jobs` の値。読めない字面は [`JOBS_FLOOR`] へ落とす（**速い側へ倒さない**）。
pub fn jobs_of(args: &[String]) -> u64 {
    floored(args, "--jobs")
}

/// `--threads` の値（job 1 つの `cargo test` に許す test thread・器の受付が決めた値・設計 gate-cost.md §31
/// 約束 7）。渡されない周と数でない周は **1**（`--jobs` と同じ向き・cores から導かない）。
pub fn threads_of(args: &[String]) -> u64 {
    floored(args, "--threads")
}

/// 数の flag の値。渡されない・数でない・0 は [`JOBS_FLOOR`]（1）へ落とす。
fn floored(args: &[String], name: &str) -> u64 {
    flag(args, name)
        .and_then(|value| value.parse().ok())
        .filter(|found| *found >= JOBS_FLOOR)
        .unwrap_or(JOBS_FLOOR)
}

/// 「測れなかった」を表す rc。**0 に化けさせない**ための第 3 の値である。
fn unmeasured(reason: &str) -> ExitCode {
    crate::emit_err(reason);
    ExitCode::from(2)
}

/// `mutants-diff` の入口。
pub fn run(args: &[String]) -> ExitCode {
    let Some(base) = flag(args, "--base") else {
        return unmeasured(USAGE);
    };
    // **語の形が悪い周は何も撃たない**（rc 2・設計 gate-cost.md §34 約束 1）。
    let teeth = match teeth_of(args) {
        Ok(found) => found,
        Err(reason) => return unmeasured(&reason),
    };
    let Ok(root) = std::env::current_dir() else {
        return unmeasured("mutants-diff: cwd を解決できない");
    };
    if !tool_present() {
        return unmeasured("mutants-diff: cargo-mutants が無い（測れていない・rc 2）");
    }
    let work = root.join("target").join("mutants-diff");
    if let Err(err) = std::fs::create_dir_all(&work) {
        return unmeasured(&format!("mutants-diff: 作業 dir を作れない: {err}"));
    }
    let diff_path = work.join("in.diff");
    // **前便の測定結果を今便の 1 行として出さない**（lens 2026-09-11 H2・実測で再現した）。
    // cargo-mutants は変異 0 の周に出力 dir へ触らないので、掃除しないと前の周の
    // `total=18 missed=6` がそのまま今の周の測定を名乗る。
    let out = work.join("out");
    if let Err(reason) = write_diff(&root, base, &diff_path).and_then(|()| clear_previous_out(&out)) {
        return unmeasured(&format!("mutants-diff: {reason}"));
    }
    // **package 名は NAME から解決する**（字面を持たない＝憲法 C2.2・`xtask check` の name-literal）。
    let layout = match crate::check::Layout::discover(&root) {
        Ok(found) => found,
        Err(reason) => return unmeasured(&format!("mutants-diff: {reason}")),
    };
    // **baseline は道具の外で 1 回、全数**（設計 gate-cost.md §33）。赤なら cargo-mutants を起こさない。
    let log_path = work.join("baseline.log");
    let threads = threads_of(args);
    let timeout_s = match own_baseline(&root, &layout.name, threads, &log_path) {
        Ok(found) => found,
        Err(reason) => return unmeasured(&format!("mutants-diff: {reason}")),
    };
    // **測る範囲は 1 つの束縛**: `-p` へ渡した名前を [`Scope`] として受け取り、行はそれでしか組めない。
    // 並列度も thread 数も**受けた値をそのまま**渡す（cores はここで読まない・設計 gate-cost.md §31 約束 7）。
    let pace = Pace { jobs: jobs_of(args), threads, timeout_s };
    let (args, scope) = measure_args(&diff_path, &out, &layout.name, pace, teeth.as_deref());
    let status = Command::new("cargo")
        .args(args)
        .current_dir(&root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    // **rc は判定に使わない**。cargo-mutants は生存が在る周に非 0 を返すが、門にするかは
    // manifest の `R-C12-1` が決める（ここで倒すと enabled を無視して deny 化してしまう）。
    let Ok(status) = status else {
        return unmeasured("mutants-diff: cargo mutants を起動できない（測れていない・rc 2）");
    };
    let outcomes = out.join("mutants.out").join("outcomes.json");
    let counts = match std::fs::read_to_string(&outcomes) {
        // 読めた周も **道具の rc を見る**（[`measured`]・baseline 失敗を緑にしない）。
        Ok(json) => parse_outcomes(&json).and_then(|counts| measured(counts, status.success())),
        // **不在**の周だけ 2 つの事実を弁別する（[`without_outcomes`]）。権限や I/O の失敗を
        // 「無かった」と同じに扱うと、読めないことが `total=0` の緑に化ける。
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => without_outcomes(status.success()),
        Err(err) => Err(format!("outcomes.json を読めない: {err}（測れていない）")),
    };
    // **rc 2 の周だけ**自前の `baseline.log` の末尾を理由行の後ろに写す（cargo-mutants は
    // `--baseline skip` で baseline を書かない・設計 gate-cost.md §33 約束 2）。
    let counts = match diagnosed(counts, || baseline_log_tail(&log_path)) {
        Ok(found) => found,
        Err(reason) => return unmeasured(&format!("mutants-diff: {reason}")),
    };
    // **`-p` に渡した名前そのもの**を行に持ち回る（[`Scope`] は literal から作れない）。
    crate::emit(&counts.line(&scope));
    let manifest = std::fs::read_to_string(root.join("rules").join("manifest.toml")).unwrap_or_default();
    // **極性が読めない周は判定しない**（rc 2）。`R-C12-1` が在るのに `enabled` を読めない
    // まま `verdict` を呼ぶと、壊れた行が「門にしない」の緑と 1 bit も違わなくなる。
    let Some(deny) = deny_line_enabled(&manifest) else {
        return unmeasured("mutants-diff: R-C12-1 の enabled を読めない（極性が決まらない・測れていない・rc 2）");
    };
    verdict(&counts, deny)
}

/// 前便の出力 dir を消す（無い周は何もしない・それ以外の失敗は理由を返す）。
fn clear_previous_out(out: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(out) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(format!("前回の出力を掃除できない: {err}")),
        _ => Ok(()),
    }
}

/// 自前の `baseline.log`（作業 dir・[`own_baseline`] が書く）の末尾。無い・読めない周はその
/// 理由を 1 行で（診断が無いことも診断として残す）。
fn baseline_log_tail(log: &Path) -> String {
    match std::fs::read_to_string(log) {
        Ok(text) => baseline_tail(&text, BASELINE_TAIL_LINES),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            "baseline.log が無い（baseline に届いていない）".to_owned()
        }
        Err(err) => format!("baseline.log を読めない: {err}"),
    }
}

/// 自前の baseline（設計 gate-cost.md §33 約束 1 / 2 / 4）: build → test を 1 回ずつ撃ち、test の
/// stdout / stderr を `log` へ写す。赤なら [`baseline_judged`] の `Err`（末尾つき）、緑なら test の
/// 壁時計（ms）から導いた timeout 秒を返し、`log` の末尾に `timeout=<T>` の 1 行を足す。
fn own_baseline(root: &Path, scope: &str, threads: u64, log: &Path) -> Result<u64, String> {
    let fire = |args: Vec<String>| {
        Command::new("cargo")
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| format!("baseline の cargo test を起動できない: {err}（測れていない）"))
    };
    let build = fire(baseline_build_args(scope))?;
    if !build.status.success() {
        let text = log_text(&build);
        write_log(log, &text)?;
        return baseline_judged(false, &text).map(|()| 0);
    }
    let started = std::time::Instant::now();
    let test = fire(baseline_args(scope, threads))?;
    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut text = log_text(&test);
    let judged = baseline_judged(test.status.success(), &text);
    let timeout_s = mutant_timeout_s(ms);
    if judged.is_ok() {
        text.push_str(&format!("timeout={timeout_s}\n"));
    }
    write_log(log, &text)?;
    judged.map(|()| timeout_s)
}

/// 1 手の stdout と stderr を 1 本の log の字面へ（stdout が先）。
fn log_text(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn write_log(log: &Path, text: &str) -> Result<(), String> {
    std::fs::write(log, text).map_err(|err| format!("baseline.log を書けない: {err}（測れていない）"))
}

/// cargo-mutants が居るか（`--version` が rc 0 を返すか）。
fn tool_present() -> bool {
    Command::new("cargo")
        .args(["mutants", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// `git diff <base>...HEAD` を file へ落とす。
fn write_diff(root: &Path, base: &str, path: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("diff")
        .arg(format!("{base}...HEAD"))
        .current_dir(root)
        .output()
        .map_err(|err| format!("git diff を起動できない: {err}"))?;
    if !output.status.success() {
        return Err(format!("git diff が失敗した（base={base}）"));
    }
    std::fs::write(path, &output.stdout).map_err(|err| format!("diff を書けない: {err}"))
}

/// [`measure_args`] の歯。本体と同じ file に置く（設計 gate-cost.md §19・行 j の write-set）。
/// 他の `mutants_*` の歯は `main.rs` の test 区間に在り、そちらは触らない。
#[cfg(test)]
mod tests {
    use super::{
        baseline_args, baseline_build_args, baseline_judged, measure_args, mutant_timeout_s, teeth_of, Counts,
        Pace, BASELINE_TAIL_HEADING, TEETH_MALFORMED,
    };
    use std::path::Path;

    /// **baseline の** `cargo test` は全数（憲法 C10・§19 の理由は baseline にだけ当たる・§33）:
    /// `test -p <scope> --no-fail-fast -- --test-threads <t>` を**この順**で組み、`--no-fail-fast` は
    /// 1 つ目の `--` の前（cargo test 自身の flag）。`--` は 1 つで、その直後が `--test-threads`。
    /// build の手は `test -p <scope> --no-run`（--no-fail-fast も thread も持たない）。
    #[test]
    fn no_fail_fast_is_passed_to_cargo_test_by_mutants() {
        for (scope, threads) in [("probe-pkg-3f", 3_u64), ("other-pkg-7a", 1)] {
            let args = baseline_args(scope, threads);
            let threads = threads.to_string();
            assert_eq!(
                args,
                ["test", "-p", scope, "--no-fail-fast", "--", "--test-threads", threads.as_str()],
                "baseline の引数はこの順: {args:?}"
            );
            assert_eq!(args.iter().filter(|a| *a == "--").count(), 1, "-- は 1 つ: {args:?}");
            let build = baseline_build_args(scope);
            assert_eq!(build, ["test", "-p", scope, "--no-run"], "build の手: {build:?}");
        }
    }

    /// (a) mutant の test は fail-fast: `measure_args` の `--` の後ろに `--no-fail-fast` が無く、
    /// `--baseline skip` が `--jobs <j>` の後ろ・1 つ目の `--` の前に在る。
    #[test]
    fn mutants_diff_fail_fast_mutants_skip_the_baseline_and_do_not_pass_no_fail_fast() {
        for (jobs, threads, timeout_s) in [(3_u64, 4_u64, 21_u64), (1, 1, 310)] {
            let pace = Pace { jobs, threads, timeout_s };
            let (args, _) = measure_args(Path::new("probe.diff"), Path::new("probe-out"), "probe-pkg-3f", pace, None);
            assert!(!args.iter().any(|a| a == "--no-fail-fast"), "mutant に --no-fail-fast は無い: {args:?}");
            let jobs_at = args.iter().position(|a| a == "--jobs").expect("--jobs が在る");
            let skip_at = args.iter().position(|a| a == "--baseline").expect("--baseline が在る");
            let dashes = args.iter().position(|a| a == "--").expect("-- が在る");
            assert_eq!(args.get(jobs_at + 1).map(String::as_str), Some(jobs.to_string().as_str()), "{args:?}");
            assert_eq!(skip_at, jobs_at + 2, "--baseline は --jobs <j> の直後: {args:?}");
            assert_eq!(args.get(skip_at + 1).map(String::as_str), Some("skip"), "{args:?}");
            assert!(skip_at < dashes, "--baseline skip は -- の前: {args:?}");
            assert_eq!(
                &args[dashes..],
                ["--", "--", "--test-threads", threads.to_string().as_str()],
                "-- の後ろは -- --test-threads <t> だけ: {args:?}"
            );
        }
    }

    /// (d) の後半: `--timeout <T>` が `--baseline skip` の直後・1 つ目の `--` の前に `T` の字面で載る。
    #[test]
    fn mutants_diff_fail_fast_timeout_sits_after_baseline_skip_before_dashes() {
        for timeout_s in [20_u64, 21, 310] {
            let pace = Pace { jobs: 2, threads: 3, timeout_s };
            let (args, _) = measure_args(Path::new("d"), Path::new("o"), "p", pace, None);
            let skip_at = args.iter().position(|a| a == "--baseline").expect("--baseline が在る");
            let dashes = args.iter().position(|a| a == "--").expect("-- が在る");
            assert_eq!(
                &args[skip_at..dashes],
                ["--baseline", "skip", "--timeout", timeout_s.to_string().as_str()],
                "{args:?}"
            );
            assert_eq!(args.iter().filter(|a| *a == "--timeout").count(), 1, "{args:?}");
        }
    }

    /// (d) timeout の式 `max(20, ceil(5 × ms / 1000))`: 床・切り上げ・倍率が別々に落ちる 3 点。
    #[test]
    fn mutants_diff_fail_fast_timeout_is_max_20_ceil_5x_seconds() {
        assert_eq!(mutant_timeout_s(3000), 20, "床: 5 × 3 = 15 は 20 に上がる");
        assert_eq!(mutant_timeout_s(4020), 21, "切り上げ: 5 × 4.02 = 20.1 は 21（floor / round は 20）");
        assert_eq!(mutant_timeout_s(62000), 310, "倍率 5: 5 × 62 = 310");
    }

    /// (c) baseline の rc の判定の対: rc ≠ 0 は `Err`（字面に自前の log の末尾が載る）、rc 0 は `Ok`。
    #[test]
    fn mutants_diff_fail_fast_baseline_rc_decides_whether_mutants_run() {
        let log = "running 3 tests\ntest probe_tooth_4c1 ... FAILED\ntest result: FAILED. 2 passed; 1 failed\n";
        let red = baseline_judged(false, log).expect_err("rc ≠ 0 は測れていない");
        assert!(red.contains(BASELINE_TAIL_HEADING), "見出しが載る: {red}");
        assert!(red.contains("test probe_tooth_4c1 ... FAILED"), "自前の log の末尾が載る: {red}");
        assert!(red.ends_with("test result: FAILED. 2 passed; 1 failed"), "末尾で終わる: {red}");
        assert_eq!(baseline_judged(true, log), Ok(()), "rc 0 は cargo-mutants へ進む");
    }

    /// 空白区切りの 1 行を引数の列へ（`--teeth` の値に空白を入れる周は手で組む）。
    fn argv(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_owned).collect()
    }

    /// 行 z の歯が使う 1 組の値（jobs / threads / timeout は別々の値＝取り違えが字面に出る）。
    const TEETH_PACE: Pace = Pace { jobs: 3, threads: 5, timeout_s: 41 };

    /// §34 (a) `--teeth a,b`: `--test-tool nextest` が `--timeout <T>` の直後に在り、1 つ目の `--` の後ろは
    /// `-E` `kind(lib) | kind(bin) | test(/^(a|b)/)` `--test-threads <t>` **だけ**（2 つ目の `--` は無い）。
    #[test]
    fn mutants_diff_teeth_named_words_run_under_nextest_filter() {
        let words = teeth_of(&argv("--base main --teeth probe_a_,probe_b_"))
            .expect("語の形は正しい")
            .expect("--teeth が在る");
        assert_eq!(words, ["probe_a_", "probe_b_"], "語は宣言順");
        let (args, scope) = measure_args(Path::new("d"), Path::new("o"), "p", TEETH_PACE, Some(&words));
        let timeout_at = args.iter().position(|a| a == "--timeout").expect("--timeout が在る");
        let dashes = args.iter().position(|a| a == "--").expect("-- が在る");
        assert_eq!(
            &args[timeout_at..dashes],
            ["--timeout", "41", "--test-tool", "nextest"],
            "--test-tool nextest は --timeout <T> の直後・-- の前: {args:?}"
        );
        assert_eq!(
            &args[dashes..],
            ["--", "-E", "kind(lib) | kind(bin) | test(/^(probe_a_|probe_b_)/)", "--test-threads", "5"],
            "-- の後ろはこれだけ: {args:?}"
        );
        assert_eq!(args.iter().filter(|a| *a == "--").count(), 1, "2 つ目の -- は無い: {args:?}");
        assert_eq!(scope.teeth(), "2", "語の数");
        // `--teeth` より前の cargo-mutants の引数は §33 の形と同じ列。
        let (plain, _) = measure_args(Path::new("d"), Path::new("o"), "p", TEETH_PACE, None);
        assert_eq!(&args[..timeout_at + 2], &plain[..timeout_at + 2], "--timeout <T> までは不変");
    }

    /// §34 (b) `--teeth -` は空＝式は `kind(lib) | kind(bin)` だけ（`test(…)` を足さない）。
    #[test]
    fn mutants_diff_teeth_dash_is_empty_and_runs_lib_and_bin_only() {
        let words = teeth_of(&argv("--base main --teeth -")).expect("- は正しい").expect("--teeth が在る");
        assert!(words.is_empty(), "- は空: {words:?}");
        let (args, scope) = measure_args(Path::new("d"), Path::new("o"), "p", TEETH_PACE, Some(&words));
        let dashes = args.iter().position(|a| a == "--").expect("-- が在る");
        assert_eq!(&args[dashes..], ["--", "-E", "kind(lib) | kind(bin)", "--test-threads", "5"], "{args:?}");
        assert!(args.iter().any(|a| a == "nextest"), "空でも nextest で走る: {args:?}");
        assert_eq!(scope.teeth(), "0", "空 = 0");
    }

    /// §34 (c) `--teeth` 無しは §33 の形と 1 語も違わない（`mutants_diff_fail_fast_` の pin と同じ列）。
    #[test]
    fn mutants_diff_teeth_absent_keeps_the_section_33_shape() {
        assert_eq!(teeth_of(&argv("--base main --jobs 3 --threads 5")), Ok(None), "無しは None");
        let (args, scope) = measure_args(Path::new("d"), Path::new("o"), "p", TEETH_PACE, None);
        assert_eq!(
            args,
            [
                "mutants", "--in-diff", "d", "-p", "p", "--no-shuffle", "--copy-vcs", "true", "-o", "o", "--jobs", "3",
                "--baseline", "skip", "--timeout", "41", "--", "--", "--test-threads", "5"
            ],
            "{args:?}"
        );
        assert!(!args.iter().any(|a| a == "--test-tool" || a == "-E"), "nextest を名指さない: {args:?}");
        assert_eq!(scope.teeth(), "-", "無し = -");
    }

    /// §34 (d) 語に `[A-Za-z0-9_]` 以外（空白・`.`・`/`）・空の語・値の無い flag は閉じた理由 1 つの `Err`。
    #[test]
    fn mutants_diff_teeth_malformed_words_are_unmeasured() {
        let spaced = vec!["--base".to_owned(), "main".to_owned(), "--teeth".to_owned(), "a_,b c".to_owned()];
        for args in [
            spaced,
            argv("--base main --teeth a_,b.c"),
            argv("--base main --teeth a_/b"),
            argv("--base main --teeth a_,,b_"),
            argv("--base main --teeth"),
            argv("--base main --teeth --jobs 2"),
        ] {
            assert_eq!(teeth_of(&args), Err(TEETH_MALFORMED.to_owned()), "{args:?}");
        }
        assert_eq!(teeth_of(&argv("--base main --teeth Ab_9")), Ok(Some(vec!["Ab_9".to_owned()])), "英数字と _ は通る");
    }

    /// §34 (e) 記録の行の末尾が `teeth=-` / `teeth=0` / `teeth=2` の 3 形（5 数と `scope=` は不変）。
    #[test]
    fn mutants_diff_teeth_record_line_carries_three_forms() {
        let counts = Counts { total: 9, caught: 5, missed: 2, unviable: 1, timeout: 1 };
        let two = ["x_".to_owned(), "y_".to_owned()];
        let empty: [String; 0] = [];
        for (teeth, tail) in [(None, "-"), (Some(&empty[..]), "0"), (Some(&two[..]), "2")] {
            let (_, scope) = measure_args(Path::new("d"), Path::new("o"), "probe-pkg-2e", TEETH_PACE, teeth);
            assert_eq!(
                counts.line(&scope),
                format!("mutants-diff: total=9 caught=5 missed=2 unviable=1 timeout=1 scope=probe-pkg-2e teeth={tail}"),
                "teeth={tail}"
            );
        }
    }
}
