//! `seat` に続く引数の面（設計 §3）。
//!
//! **env も HOME も読まない**（憲法 C2.2）: 出所（pane / transcript）も置き場も
//! 引数で明示されたものだけを見る。値欠けの flag は黙って落とさず使い方で断る。

use super::{inject, meter};
use crate::cli_outcome::{Outcome, RC_OK, RC_REFUSED};

/// `seat` の使い方。
pub fn usage() -> String {
    "usage: seat <meter --target T [--capture-file PATH] [--transcript PATH]|inject --target T (--text S|--file PATH)> [--tmux-socket PATH] [--state-dir PATH]".to_owned()
}

/// `seat` に続く引数を捌く。
pub fn dispatch(args: &[String]) -> Outcome {
    match args.first().map(String::as_str) {
        Some("meter") => meter_of(args),
        Some("inject") => inject_of(args),
        _ => refused_usage(),
    }
}

/// 使い方を stderr へ出して rc 1。
fn refused_usage() -> Outcome {
    Outcome::failed(RC_REFUSED, vec![usage()])
}

/// `--<name> <value>` の読み取り結果。
enum Flag<'a> {
    /// flag そのものが無い。
    Absent,
    /// 値が在る。
    Value(&'a str),
    /// flag は在るが値が無い（末尾か、次が別の flag）。
    Missing,
}

/// `--<name>` を読む。
fn flag<'a>(args: &'a [String], name: &str) -> Flag<'a> {
    let Some(at) = args.iter().position(|arg| arg == name) else {
        return Flag::Absent;
    };
    match args.get(at.saturating_add(1)) {
        Some(found) if !found.starts_with("--") => Flag::Value(found),
        _ => Flag::Missing,
    }
}

/// 任意の flag。**値欠けは黙って落とさず断る**（SRS NFR4）。
fn optional<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, ()> {
    match flag(args, name) {
        Flag::Absent => Ok(None),
        Flag::Value(found) => Ok(Some(found)),
        Flag::Missing => Err(()),
    }
}

/// 必須の flag。
fn required<'a>(args: &'a [String], name: &str) -> Result<&'a str, ()> {
    optional(args, name)?.ok_or(())
}

/// `seat meter`。
fn meter_of(args: &[String]) -> Outcome {
    let (Ok(target), Ok(socket), Ok(capture_file), Ok(transcript)) = (
        required(args, "--target"),
        optional(args, "--tmux-socket"),
        optional(args, "--capture-file"),
        optional(args, "--transcript"),
    ) else {
        return refused_usage();
    };
    let request = meter::Request {
        target,
        socket,
        capture_file,
        transcript,
    };
    match meter::measure(&request) {
        meter::Measure::Measured(reading) => Outcome::ok_line(meter::render(&reading)),
        meter::Measure::Unmeasured(reason) => {
            Outcome::failed_line(RC_REFUSED, meter::render_unmeasured(reason))
        }
    }
}

/// `seat inject`。
fn inject_of(args: &[String]) -> Outcome {
    let (Ok(target), Ok(socket), Ok(state_dir)) = (
        required(args, "--target"),
        optional(args, "--tmux-socket"),
        optional(args, "--state-dir"),
    ) else {
        return refused_usage();
    };
    let Some(payload) = payload_of(args) else {
        return refused_usage();
    };
    let request = inject::Request {
        target,
        socket,
        payload: &payload,
        state_dir,
    };
    deliver(&request)
}

/// 注入を 1 回行い、結果を行にする。
fn deliver(request: &inject::Request) -> Outcome {
    match inject::deliver(request) {
        inject::Delivery::Delivered(bytes) => Outcome {
            out: vec![inject::render_delivered(request.target, bytes)],
            err: Vec::new(),
            rc: RC_OK,
        },
        inject::Delivery::Refused(reason) => {
            Outcome::failed_line(RC_REFUSED, inject::render_refused(reason))
        }
        inject::Delivery::Unconfirmed(reason) => {
            Outcome::failed_line(RC_REFUSED, inject::render_unconfirmed(reason))
        }
    }
}

/// 送る 1 行を `--text` か `--file` から取る。
///
/// **空の口を成功に化けさせない**: 空白だけの payload は使い方の誤りとして断り、
/// 1 key も送らない（`send-keys -l ""` は rc 0 で終わるので、何もしていない周が
/// 「送った」として記録されうる）。
fn payload_of(args: &[String]) -> Option<String> {
    let (Ok(text), Ok(file)) = (optional(args, "--text"), optional(args, "--file")) else {
        return None;
    };
    let payload = match (text, file) {
        // 2 つ同時に渡された周は、どちらを送ったか曖昧なので断る。
        (Some(_), Some(_)) | (None, None) => return None,
        (Some(found), None) => found.to_owned(),
        (None, Some(path)) => std::fs::read_to_string(path).ok()?,
    };
    (!payload.trim().is_empty()).then_some(payload)
}
