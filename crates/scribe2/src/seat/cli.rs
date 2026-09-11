//! `seat` に続く引数の面（設計 §3）。
//!
//! **env も HOME も読まない**（憲法 C2.2）: 出所（pane / transcript）も置き場も
//! 引数で明示されたものだけを見る。値欠けの flag は黙って落とさず使い方で断る。

use super::cycle::{self, Cycle};
use super::{heartbeat, inject, meter, tick};
use crate::cli_outcome::{Outcome, RC_OK, RC_REFUSED};

/// `seat` の使い方。
pub fn usage() -> String {
    "usage: seat <meter --target T [--transcript PATH]|inject --target T (--text S|--file PATH)|heartbeat --target T|tick --target T --wm-dir DIR [--pointer TEXT] [--restore CMD]|cycle --target T --wm-dir DIR [--restore CMD]> [--tmux-socket PATH] [--capture-file PATH] [--state-dir PATH]".to_owned()
}

/// `seat` に続く引数を捌く。
pub fn dispatch(args: &[String]) -> Outcome {
    match args.first().map(String::as_str) {
        Some("meter") => meter_of(args),
        Some("inject") => inject_of(args),
        Some("heartbeat") => heartbeat_of(args),
        Some("tick") => tick_of(args),
        Some("cycle") => cycle_of(args),
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
        inject::Delivery::Delivered(bytes, settled) => Outcome {
            out: vec![inject::render_delivered(request.target, bytes, settled)],
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

/// 空文字を**使い方の誤り**として断る任意の flag。
///
/// 空の text は `send-keys -l ""` が rc 0 で終わるので、渡し忘れが「送った」に化ける
/// （実測 2026-09-10・空の口は成功に化ける）。断る側へ倒し、1 key も送らない。
fn nonempty<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, ()> {
    match optional(args, name)? {
        Some(found) if found.trim().is_empty() => Err(()),
        other => Ok(other),
    }
}

/// 場所を指す共有の flag（3 つの subcommand が同じ字で持つ）。
struct Common<'a> {
    /// tmux の socket。
    socket: Option<&'a str>,
    /// pane 本文の代わりに読む file。
    capture_file: Option<&'a str>,
    /// 記録と marker の置き場。
    state_dir: Option<&'a str>,
}

/// 必須の flag のうち**空文字を断る**もの。
fn required_nonempty<'a>(args: &'a [String], name: &str) -> Result<&'a str, ()> {
    nonempty(args, name)?.ok_or(())
}

/// 共有の flag を読む。**場所の flag も空文字を断る**——空の `--state-dir` は
/// `PathBuf::from("")` が cwd 相対になり、記録と marker が撃った場所へ散る
/// （実測 2026-09-10・lens-384 L-13: `./seat/<t>/heartbeat` が cwd に作られた）。
fn common_of(args: &[String]) -> Result<Common<'_>, ()> {
    Ok(Common {
        socket: nonempty(args, "--tmux-socket")?,
        capture_file: nonempty(args, "--capture-file")?,
        state_dir: nonempty(args, "--state-dir")?,
    })
}

/// `seat heartbeat`。
fn heartbeat_of(args: &[String]) -> Outcome {
    let (Ok(target), Ok(state_dir)) = (
        required_nonempty(args, "--target"),
        nonempty(args, "--state-dir"),
    ) else {
        return refused_usage();
    };
    let Some(state) = super::state_dir_of(state_dir) else {
        return Outcome::failed_line(
            RC_REFUSED,
            heartbeat::render_refused(heartbeat::REASON_STATE_DIR),
        );
    };
    match heartbeat::touch(&super::seat_dir(&state, target)) {
        Ok(()) => Outcome::ok_line(heartbeat::render(target)),
        Err(_) => Outcome::failed_line(
            RC_REFUSED,
            heartbeat::render_refused(heartbeat::REASON_UNWRITABLE),
        ),
    }
}

/// `seat tick`。
fn tick_of(args: &[String]) -> Outcome {
    let (Ok(target), Ok(wm_dir), Ok(pointer), Ok(restore), Ok(common)) = (
        required_nonempty(args, "--target"),
        required_nonempty(args, "--wm-dir"),
        nonempty(args, "--pointer"),
        nonempty(args, "--restore"),
        common_of(args),
    ) else {
        return refused_usage();
    };
    tick::run(&tick::Request {
        target,
        wm_dir,
        pointer,
        socket: common.socket,
        capture_file: common.capture_file,
        state_dir: common.state_dir,
        restore,
    })
}

/// `seat cycle`。
fn cycle_of(args: &[String]) -> Outcome {
    let (Ok(target), Ok(wm_dir), Ok(restore), Ok(common)) = (
        required_nonempty(args, "--target"),
        required_nonempty(args, "--wm-dir"),
        nonempty(args, "--restore"),
        common_of(args),
    ) else {
        return refused_usage();
    };
    let Some(state) = super::state_dir_of(common.state_dir) else {
        return Outcome::failed_line(
            RC_REFUSED,
            cycle::render(target, &Cycle::Refused(cycle::REASON_STATE_DIR)),
        );
    };
    let result = cycle::run(&cycle::Request {
        target,
        wm_dir,
        socket: common.socket,
        capture_file: common.capture_file,
        state_dir: &state,
        restore,
    });
    match result {
        Cycle::Done => Outcome::ok_line(cycle::render(target, &result)),
        Cycle::Refused(_) | Cycle::Failed(_) => {
            Outcome::failed_line(RC_REFUSED, cycle::render(target, &result))
        }
    }
}
