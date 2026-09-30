//! `utterance sort|show` の引数と出力（設計 docs/design/dialogue-surface.md §10 約束 3〜8）。判定と書きは [`super::sort`] と
//! [`super::show`] が持つ。断りは rc 1・stdout 0 byte・stderr の 1 行 `utterance: refused reason=<語> ts=<ts>`、読めない log と書けない
//! 周は rc 2、使い方の誤り（未知・値欠け・重複・空文字・`--as` の語と組み合わせの誤り）は rc 2 で使い方を添え、第 1 token が
//! `sort` でも `show` でもない周は使い方の 1 行と rc 1。

use super::{Done, Failure, Sort};
use crate::cli_args::{self, ArgsError, Parsed, RC_USAGE};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::Sorting;
use std::path::Path;

/// 使い方の 1 行（`help.rs` の表の `form` はこの行から頭の `usage: ` を除いた写し）。
pub fn usage() -> String {
    "usage: utterance <sort --repo R --state-dir S --ts TS --as request --memo ID [--bd B]|sort --state-dir S --ts TS --as chat|show --state-dir S --ts TS>"
        .to_owned()
}

/// `sort` が受ける flag（`--bd` は歯の seam）。
const ALLOWED_SORT: &[cli_args::Allowed] = &[
    value("--repo"),
    value("--state-dir"),
    value("--ts"),
    value("--as"),
    value("--memo"),
    value("--bd"),
];
/// `show` が受ける flag。
const ALLOWED_SHOW: &[cli_args::Allowed] = &[value("--state-dir"), value("--ts")];

/// 値を 1 つ取る flag（[`cli_args::Allowed::value`]）。
const fn value(name: &'static str) -> cli_args::Allowed {
    cli_args::Allowed::value(name)
}

/// `utterance` に続く引数を捌く（第 1 token は `sort` か `show`・それ以外は使い方の 1 行と rc 1）。
pub fn dispatch(args: &[String]) -> Outcome {
    let (verb, rest) = (args.first().map(String::as_str), args.get(1..).unwrap_or_default());
    let allowed = match verb {
        Some("sort") => ALLOWED_SORT,
        Some("show") => ALLOWED_SHOW,
        _ => return Outcome::failed_line(RC_REFUSED, usage()),
    };
    let parsed = match cli_args::parse(rest, allowed) {
        Ok(found) => found,
        Err(error) => return cli_args::refusal("utterance", &error, usage()),
    };
    let outcome = match (verb, parsed.positionals().first()) {
        (_, Some(extra)) => Err(ArgsError::Unknown((*extra).to_owned())),
        (Some("sort"), None) => sort_of(&parsed),
        _ => show_of(&parsed),
    };
    outcome.unwrap_or_else(|error| cli_args::refusal("utterance", &error, usage()))
}

/// 必須の flag の値（空白だけの値は値欠け）。
fn need<'a>(parsed: &Parsed<'a>, name: &str) -> Result<&'a str, ArgsError> {
    parsed.need(name).ok().filter(|value| !value.trim().is_empty()).ok_or_else(|| ArgsError::Missing(name.to_owned()))
}

/// 名指しされたら空白だけでない flag の値（在るなら）。
fn given<'a>(parsed: &Parsed<'a>, name: &str) -> Result<Option<&'a str>, ArgsError> {
    parsed.value(name).map(|_| need(parsed, name)).transpose()
}

/// `--as` が使い方の語を外れた周の断り（未知の値は使い方の誤り）。
fn misuse(reason: &str) -> Outcome {
    Outcome::failed(RC_USAGE, vec![format!("utterance: {reason}"), usage()])
}

/// `sort`: `--as request` は `--repo` と `--memo` を要り（`--bd` は任意）、`--as chat` は `--memo` を受けない（`--repo` / `--bd` は
/// 受けても台帳を読まない＝歯が「撃たれない」ことを測る seam）。
fn sort_of(parsed: &Parsed<'_>) -> Result<Outcome, ArgsError> {
    let (state_dir, ts, kind) = (need(parsed, "--state-dir")?, need(parsed, "--ts")?, need(parsed, "--as")?);
    let (repo, memo, bd) = (given(parsed, "--repo")?, given(parsed, "--memo")?, given(parsed, "--bd")?);
    let how = match (Sorting::parse(kind), repo, memo, bd) {
        (Some(Sorting::Request), Some(repo), Some(memo), bd) => Sort::Request { repo: Path::new(repo), memo, bd: bd.unwrap_or(crate::ledger::DEFAULT_BD) },
        (Some(Sorting::Request), ..) => return Ok(misuse("--as request は --repo と --memo を要る")),
        (Some(Sorting::Chat), _, None, _) => Sort::Chat,
        (Some(Sorting::Chat), ..) => return Ok(misuse("--as chat は --memo を受けない")),
        (None, ..) => return Ok(misuse("--as は request か chat")),
    };
    Ok(match super::sort(Path::new(state_dir), ts, &how) {
        Ok(Done::Already) => Outcome::ok_line("already".to_owned()),
        Ok(Done::Written) => {
            let named = if let Sort::Request { memo, .. } = how { format!(" memo={memo}") } else { String::new() };
            Outcome::ok_line(format!("utterance: sorted ts={ts} as={kind}{named}"))
        }
        Err(failure) => failed(&failure, ts, "sort"),
    })
}

/// `show`: 逐語だけを stdout に返す（末尾の改行は出力層の 1 つ）。
fn show_of(parsed: &Parsed<'_>) -> Result<Outcome, ArgsError> {
    let (state_dir, ts) = (need(parsed, "--state-dir")?, need(parsed, "--ts")?);
    Ok(match super::show(Path::new(state_dir), ts) {
        Ok(words) => Outcome::ok_line(words),
        Err(failure) => failed(&failure, ts, "show"),
    })
}

/// 通らなかった形を outcome へ写す（断りは rc 1 の 1 行・読めない log は rc 2 で理由の行・書けない周は rc 2 の 1 行）。
fn failed(failure: &Failure, ts: &str, verb: &str) -> Outcome {
    match failure {
        Failure::Refused(reason) => Outcome::failed_line(RC_REFUSED, format!("utterance: refused reason={} ts={ts}", reason.as_str())),
        Failure::LogUnreadable(lines) => Outcome::failed(RC_BROKEN, lines.clone()),
        Failure::WriteFailed => Outcome::failed_line(RC_BROKEN, format!("utterance: failed verb={verb} reason=write ts={ts}")),
    }
}
