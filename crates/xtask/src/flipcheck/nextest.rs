//! `flip-check` のうち nextest を撃って出力を読む群（設計 docs/design/pipeline.md §45・契約表の行 am）。
//!
//! 子の stderr の畳み（[`trimmed`]）・子の出力の中継（[`relay`]）・runner の撃ち（[`nextest`] /
//! [`nextest_with`]・引数の列は [`nextest_args`]）・出力からの落ちた歯の名指し（[`failed_tests`]・色は
//! [`strip_csi`] で剥がす）。落ちた歯の型 `FailedTest` と判定（rc の読み・base 段の撃ち直し）は親
//! `flipcheck.rs` に置いたまま。

// flip-check: moved s2-07l.500

use super::FailedTest;
use crate::emit_err;
use std::path::Path;
use std::process::{Command, Output};

/// 子プロセスの stderr を 1 行へ畳む。
pub(super) fn trimmed(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<&str>>()
        .join(" / ")
}

/// 子の出力を親の stderr へ流す（親 stdout は判定 1 行のみ）。
pub(super) fn relay(label: &str, output: &Output) {
    for stream in [&output.stdout, &output.stderr] {
        for line in String::from_utf8_lossy(stream).lines() {
            emit_err(&format!("flip-check: {label}| {line}"));
        }
    }
}

/// base tree で runner を撃つ。env は親を継承し `CARGO_TARGET_DIR` だけ上書きする。
pub(super) fn nextest(dir: &Path, target_dir: &Path) -> Result<Output, String> {
    nextest_with(dir, target_dir, &[])
}

/// [`nextest`] の本体。引数の列は [`nextest_args`] が組む（素の撃ちと撃ち直しで起動の
/// 形を 2 つ持つと、片方だけ `--no-tests=fail` が落ちて rc 4 が緑に化ける）。
pub(super) fn nextest_with(dir: &Path, target_dir: &Path, extra: &[&str]) -> Result<Output, String> {
    Command::new("cargo")
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target_dir)
        .args(nextest_args(extra))
        .output()
        .map_err(|err| format!("cargo nextest を起動できない: {err}"))
}

/// 子の `cargo nextest run` へ渡す引数の列。`extra` は末尾へ足す（base 段の撃ち直しが
/// filterset を渡す口）。
///
/// **`--color never` を常に渡す**。子の出力は [`failed_tests`] が行の字面で読むので、
/// 親の env（CI の toolchain action が置く `CARGO_TERM_COLOR=always`）を継承して `FAIL`
/// 行に ANSI 色が付くと、落ちた歯を 1 本も名指せず `base-not-green` へ倒れる（実測
/// 2026-09-14・s2-07l.276: main が CI だけで赤）。env は読まず設定もしない——子の出力の
/// 形を親の env に依存させない（C2.2）ための口は、この引数 1 つに閉じる。
///
/// **`--no-fail-fast` も常に渡す**（設計 gate-cost.md §19・憲法 C10）。既定の fail-fast では
/// 歯 1 本の flaky で残りが未実行のまま終わり、[`failed_tests`] が名指せる歯が 1 本に縮む
/// ＝`retry_named` の撃ち直しが「落ちた歯の全数」でなく先頭の 1 本しか救えない。
pub(super) fn nextest_args(extra: &[&str]) -> Vec<String> {
    [
        "nextest",
        "run",
        "--workspace",
        "--no-tests=fail",
        "--no-fail-fast",
        "--color",
        "never",
    ]
    .iter()
    .chain(extra)
    .map(|arg| (*arg).to_owned())
    .collect()
}

/// ANSI CSI（`ESC [ <parameter bytes> <intermediate bytes> <final byte>`）だけを剥がす。
///
/// `--color never` が効かない周（古い nextest・別の runner）の第 2 の守り。剥がすのは
/// CSI だけで、他の制御文字（裸の `ESC`・改行・tab）は触らない——落ちた歯の名指しに
/// 要るのは色を外すことだけで、それ以上に出力を書き換えると別の行の形を壊す。
fn strip_csi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' || chars.peek() != Some(&'[') {
            out.push(ch);
            continue;
        }
        chars.next();
        // parameter bytes 0x30–0x3F と intermediate bytes 0x20–0x2F を読み飛ばし、
        // final byte 0x40–0x7E で閉じる。閉じないまま終わればそこまでを落とす。
        for found in chars.by_ref() {
            if ('\x40'..='\x7e').contains(&found) {
                break;
            }
        }
    }
    out
}

/// runner（nextest）の出力から `FAIL [ <time>] <binary id> <歯の名>` 形の行だけを拾う。
///
/// 拾うのは行頭（空白は跨ぐ）が `FAIL [` の行だけで、`PASS` / `Summary` / `TRY n FAIL`
/// の行は拾わない。`]` の後ろは進捗の `(n/m)`（桁を揃える空白を挟む＝`(  12/1146)`・
/// 1 語には割れない）を挟んで binary id と名の **2 語ちょうど**——語数が違う行（形の
/// 崩れた行）は拾わない。nextest は落ちた歯を末尾の一覧でもう 1 度出すので、同じ歯は
/// 1 本に畳む（順序は出力順）。
///
/// 照合の前に [`strip_csi`] で色を剥がす。子には `--color never` を渡しているが
/// （[`nextest_args`]）、色が残った周に「名指せない失敗」として `base-not-green` へ
/// 倒れるのを、parser の側でも塞ぐ。
pub(super) fn failed_tests(text: &str) -> Vec<FailedTest> {
    let text = strip_csi(text);
    let mut found: Vec<FailedTest> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("FAIL [") else {
            continue;
        };
        let Some((_, after)) = rest.split_once(']') else {
            continue;
        };
        let after = after.trim_start();
        let after = match after.strip_prefix('(') {
            None => after,
            Some(progress) => match progress.split_once(')') {
                Some((_, tail)) => tail,
                None => continue,
            },
        };
        let words: Vec<&str> = after.split_whitespace().collect();
        let &[binary, name] = words.as_slice() else {
            continue;
        };
        let test = FailedTest {
            binary: binary.to_owned(),
            name: name.to_owned(),
        };
        if !found.contains(&test) {
            found.push(test);
        }
    }
    found
}
