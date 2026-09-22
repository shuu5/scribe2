//! `account` subcommand の面（設計 docs/design/account-lifecycle.md §3）: flag を読み、[`super`] の処理を 1 つ呼び、結果を
//! [`Outcome`] の行へ写す。**env も HOME も読まない**（C2.2）: 置き場は `--state-dir` で必ず外から受け取り、既定を持たない。
//! 出力は行を組んで返すだけで、stdout / stderr へは bin 側の `emit` / `emit_err` が書く。

use super::{add, ls_lines, restore, retire, AccountError, Add, Delivery, Prepared};
use crate::cli_args::{self, Allowed, ArgsError};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::seat::sanitize_target;
use std::path::Path;

/// `add` が受ける flag（`--state-dir` は全 verb が要る）。
const ADD_FLAGS: &[cli_args::Allowed] =
    &[Allowed::value("--state-dir"), Allowed::value("--anchor"), Allowed::value("--target"), Allowed::value("--tmux-socket")];
/// `ls` / `retire` / `restore` が受ける flag。
const BARE_FLAGS: &[cli_args::Allowed] = &[Allowed::value("--state-dir")];

/// `account` の使い方。
pub fn usage() -> String {
    "usage: account <add <label> [--anchor DIR] [--target T] [--tmux-socket PATH]|ls|retire <label>|restore <label>> --state-dir S"
        .to_owned()
}

/// 読んだ flag（どれも 1 回まで）。
#[derive(Default)]
struct Flags<'a> {
    /// `--state-dir`（必須）。
    state_dir: Option<&'a str>,
    /// `--anchor`。
    anchor: Option<&'a str>,
    /// `--target`。
    target: Option<&'a str>,
    /// `--tmux-socket`。
    socket: Option<&'a str>,
}

/// 引数の断り（閉包の 4 値は [`ArgsError`]・残る 2 つは従来どおり使い方の誤りの rc 1）。
enum FlagsError {
    /// 未知の flag・値欠け・重複・`--state-dir` の欠け・`--help`（[`crate::cli_args::refusal`] が rc を決める）。
    Args(ArgsError),
    /// 空の値・flag の組に続く余りの positional（使い方の誤り・rc 1）。
    Usage,
}

/// `--<name> <value>` の組を [`crate::cli_args::parse`] で読む。`allowed` に無い flag・値欠け・重複・`--state-dir` の欠けは
/// typed な [`ArgsError`]、空の値と余りの positional は [`FlagsError::Usage`]。
fn flags<'a>(rest: &'a [String], allowed: &[Allowed]) -> Result<Flags<'a>, FlagsError> {
    let parsed = crate::cli_args::parse(rest, allowed).map_err(FlagsError::Args)?;
    if !parsed.positionals().is_empty() {
        return Err(FlagsError::Usage);
    }
    let found = Flags {
        state_dir: Some(parsed.need("--state-dir").map_err(FlagsError::Args)?),
        anchor: parsed.value("--anchor"),
        target: parsed.value("--target"),
        socket: parsed.value("--tmux-socket"),
    };
    let values = [found.state_dir, found.anchor, found.target, found.socket];
    if values.iter().flatten().any(|value| value.trim().is_empty()) {
        return Err(FlagsError::Usage);
    }
    Ok(found)
}

/// `account` に続く引数を捌く。
pub fn dispatch(args: &[String]) -> Outcome {
    let verb = args.first().map(String::as_str);
    let (label, rest) = match verb {
        Some("ls") => (None, args.get(1..).unwrap_or_default()),
        Some("add" | "retire" | "restore") => match args.get(1).filter(|found| !found.starts_with("--")) {
            Some(found) => (Some(found.as_str()), args.get(2..).unwrap_or_default()),
            None => return refused(),
        },
        _ => return refused(),
    };
    let allowed = if verb == Some("add") { ADD_FLAGS } else { BARE_FLAGS };
    let found = match flags(rest, allowed) {
        Ok(found) => found,
        Err(FlagsError::Args(error)) => return crate::cli_args::refusal("account", &error, usage()),
        Err(FlagsError::Usage) => return refused(),
    };
    let Some(dir) = found.state_dir.map(Path::new) else {
        return refused();
    };
    match (verb, label) {
        (Some("ls"), None) => Outcome::ok(ls_lines(dir)),
        (Some("add"), Some(label)) => {
            let request = Add { state_dir: dir, label, anchor: found.anchor.map(Path::new), target: found.target, socket: found.socket };
            added(label, found.target, add(&request))
        }
        (Some("retire"), Some(label)) => done("retired", label, retire(dir, label).map(|_| ())),
        (Some("restore"), Some(label)) => done("restored", label, restore(dir, label).map(|_| ())),
        _ => refused(),
    }
}

/// 使い方の誤り（stderr に使い方・rc 1）。
fn refused() -> Outcome {
    Outcome::failed(RC_REFUSED, vec![usage()])
}

/// `account add` の結果の行: `--target` 無しは stdout に `account: prepared <label> next=<行>`・注入した周は `target=` を名乗る・
/// 門で止まった周は rc 1 で理由と行（user が打てるように）を stderr へ。
fn added(label: &str, target: Option<&str>, result: Result<Prepared, AccountError>) -> Outcome {
    match result {
        Err(error) => refusal(label, error),
        Ok(Prepared { next, delivery: Delivery::Printed }) => Outcome::ok_line(format!("account: prepared {label} next={next}")),
        Ok(Prepared { delivery: Delivery::Sent, .. }) => {
            Outcome::ok_line(format!("account: prepared {label} target={}", sanitize_target(target.unwrap_or_default())))
        }
        Ok(Prepared { next, delivery: Delivery::Refused(reason) }) => {
            Outcome::failed_line(RC_REFUSED, format!("account: prepared {label} refused={reason} next={next}"))
        }
    }
}

/// `retire` / `restore` の結果の行（成立は stdout `account: <word> <label>`）。
fn done(word: &str, label: &str, result: Result<(), AccountError>) -> Outcome {
    match result {
        Ok(()) => Outcome::ok_line(format!("account: {word} {label}")),
        Err(error) => refusal(label, error),
    }
}

/// typed な断りの 1 行（stderr・`reason=` は [`AccountError::as_str`]）。置き場を読めない・書けない周は rc 2、前提違反は rc 1。
fn refusal(label: &str, error: AccountError) -> Outcome {
    let rc = match error {
        AccountError::WriteFailed => RC_BROKEN,
        AccountError::Exists
        | AccountError::DirExists
        | AccountError::Unknown
        | AccountError::AlreadyRetired
        | AccountError::NotRetired
        | AccountError::InUse
        | AccountError::LabelInvalid => RC_REFUSED,
    };
    Outcome::failed_line(rc, format!("account: refused reason={} label={label}", error.as_str()))
}
