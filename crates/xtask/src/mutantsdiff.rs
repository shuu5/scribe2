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
    pub fn line(&self, scope: &Scope) -> String {
        format!(
            "mutants-diff: total={} caught={} missed={} unviable={} timeout={} scope={}",
            self.total, self.caught, self.missed, self.unviable, self.timeout, scope.0
        )
    }
}

/// 測った範囲＝`cargo mutants -p` へ**実際に渡した**名前。作れるのは [`measure_args`] だけ
/// （field は private・`Default` も持たない）ので、行の `scope=` と実際に測った package が
/// 別々の読みで食い違う形は型で組めない（lens-82 MEDIUM-1・再確認の残余）。
#[derive(Debug, PartialEq, Eq)]
pub struct Scope(String);

/// `cargo` へ渡す引数（`cargo` の直後から）と、その `-p` に載せた [`Scope`]。
///
/// `--in-diff <diff>` / `-p <scope>` / `-o <out>` はそれぞれ隣り合う対で、歯が対のまま見る
/// （`-o` を落とすと測った結果を読まずに `total=0` へ化ける・lens-82 再確認 MEDIUM-4）。
pub fn measure_args(diff: &Path, out: &Path, scope: &str) -> (Vec<String>, Scope) {
    let mut args: Vec<String> = ["mutants", "--in-diff"].iter().map(|s| (*s).to_owned()).collect();
    args.push(diff.display().to_string());
    args.push("-p".to_owned());
    args.push(scope.to_owned());
    args.extend(["--no-shuffle", "--copy-vcs", "true", "-o"].iter().map(|s| (*s).to_owned()));
    args.push(out.display().to_string());
    (args, Scope(scope.to_owned()))
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
/// manifest に 1 つ）。行が無い周は「門にしない」側へ倒す——無い規則を勝手に発効させない。
///
/// section は [`toml_lite::sections`] で読む。array-of-tables の header は `[` を 1 つだけ
/// 剥がした `[rule` になる（`limits.rs` の歯と同じ字面）。
pub fn deny_line_enabled(manifest: &str) -> bool {
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
        return pairs
            .iter()
            .find(|(key, _)| *key == "enabled")
            .is_some_and(|(_, value)| value.trim() == "true");
    }
    false
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
const USAGE: &str = "usage: cargo xtask mutants-diff --base <ref>";

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
    if let Err(reason) = write_diff(&root, base, &diff_path) {
        return unmeasured(&format!("mutants-diff: {reason}"));
    }
    // **前便の測定結果を今便の 1 行として出さない**（lens 2026-09-11 H2・実測で再現した）。
    // cargo-mutants は変異 0 の周に出力 dir へ触らないので、掃除しないと前の周の
    // `total=18 missed=6` がそのまま今の周の測定を名乗る。
    let out = work.join("out");
    if let Err(err) = std::fs::remove_dir_all(&out) {
        if err.kind() != std::io::ErrorKind::NotFound {
            return unmeasured(&format!("mutants-diff: 前回の出力を掃除できない: {err}"));
        }
    }
    // **package 名は NAME から解決する**（字面を持たない＝憲法 C2.2・`xtask check` の name-literal）。
    let layout = match crate::check::Layout::discover(&root) {
        Ok(found) => found,
        Err(reason) => return unmeasured(&format!("mutants-diff: {reason}")),
    };
    // **測る範囲は 1 つの束縛**: `-p` へ渡した名前を [`Scope`] として受け取り、行はそれでしか組めない。
    let (args, scope) = measure_args(&diff_path, &out, &layout.name);
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
    let counts = match counts {
        Ok(found) => found,
        Err(reason) => return unmeasured(&format!("mutants-diff: {reason}")),
    };
    // **`-p` に渡した名前そのもの**を行に持ち回る（[`Scope`] は literal から作れない）。
    crate::emit(&counts.line(&scope));
    let manifest = std::fs::read_to_string(root.join("rules").join("manifest.toml")).unwrap_or_default();
    verdict(&counts, deny_line_enabled(&manifest))
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
