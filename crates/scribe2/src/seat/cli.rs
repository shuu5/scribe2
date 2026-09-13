//! `seat` に続く引数の面（設計 §3）。
//!
//! **env も HOME も読まない**（憲法 C2.2）: 出所（pane / transcript）も置き場も
//! 引数で明示されたものだけを見る。値欠けの flag は黙って落とさず使い方で断る。

use super::consume::{self, ConsumeError};
use super::cycle::{self, Cycle};
use super::externalize::{self, ExternalizeError, Trigger};
use super::rebrief::{self, RebriefError};
use super::{heartbeat, inject, meter, tick};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use std::path::Path;
use std::time::Duration;

/// `seat` の使い方。
pub fn usage() -> String {
    "usage: seat <meter --target T [--transcript PATH]|inject --target T (--text S|--file PATH)|heartbeat --target T|tick --target T --wm-dir DIR [--pointer TEXT] [--restore CMD] [--rules PATH]|cycle --target T --wm-dir DIR [--restore CMD] [--rules PATH]|externalize --target T --wm-dir DIR --anchor DIR --plan FILE --directives FILE [--user FILE] [--trigger manual|tick] [--role R] [--rules PATH]|rebrief --target T --wm-dir DIR --anchor DIR [--bd PATH] [--prefix P] [--rules PATH]|consume --target T --wm-dir DIR> [--tmux-socket PATH] [--capture-file PATH] [--state-dir PATH]".to_owned()
}

/// `seat` に続く引数を捌く。
pub fn dispatch(args: &[String]) -> Outcome {
    match args.first().map(String::as_str) {
        Some("meter") => meter_of(args),
        Some("inject") => inject_of(args),
        Some("heartbeat") => heartbeat_of(args),
        Some("tick") => tick_of(args),
        Some("cycle") => cycle_of(args),
        Some("externalize") => externalize_of(args),
        Some("rebrief") => rebrief_of(args),
        Some("consume") => consume_of(args),
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
    // 置き場は席の 1 実装で **1 回だけ** 解き、表示と記録の両方に同じ 1 つを渡す。
    let state = super::state_dir_of(state_dir);
    let request = inject::Request {
        target,
        socket,
        payload: &payload,
        state_dir: state.as_ref(),
    };
    deliver(&request)
}

/// 注入を 1 回行い、結果を行にする。
fn deliver(request: &inject::Request) -> Outcome {
    // 表示行の所在は request が持つ解決済みの 1 つ（記録側と同じ・解けない周は 2 語を出さない）。
    let state = request.state_dir;
    match inject::deliver(request) {
        inject::Delivery::Delivered(bytes, settled) => Outcome {
            out: vec![inject::render_delivered(request.target, bytes, settled, state)],
            err: Vec::new(),
            rc: RC_OK,
        },
        inject::Delivery::Refused(reason) => {
            Outcome::failed_line(RC_REFUSED, inject::render_refused(reason, state))
        }
        inject::Delivery::Unconfirmed(reason) => Outcome::failed_line(
            RC_REFUSED,
            inject::render_unconfirmed(reason, state),
        ),
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
    match heartbeat::touch(&super::seat_dir(&state.path, target)) {
        Ok(()) => Outcome::ok_line(heartbeat::render(target, &state)),
        Err(_) => Outcome::failed_line(
            RC_REFUSED,
            heartbeat::render_refused(heartbeat::REASON_UNWRITABLE),
        ),
    }
}

/// cycle の確認の刻み（上限・周期）を解く。`--rules PATH` が在ればその file・無ければ埋め込み
/// （`rules` subcommand と**同じ 1 本の口**＝[`crate::rules::cli::open`]・`s2-07l.151`）。
///
/// 読めない file・行の無い file・不発効の行は `None`＝呼び側が `no-rule` で断る（fail-closed・
/// 値を焼かない・憲法 C5）。**1 回の呼出しで 1 回だけ解く**: tick と cycle が別々に解くと、
/// 同じ判定の中で別の値で走りうる。
fn pace_of(args: &[String]) -> Option<(Duration, Duration)> {
    let manifest = crate::rules::cli::open(args).ok()?;
    cycle::pace_of(&manifest)
}

/// `seat tick`。
fn tick_of(args: &[String]) -> Outcome {
    // 値欠け・空文字の `--rules` は使い方の誤り（他の flag と同じ極性・SRS NFR4）。
    let (Ok(target), Ok(wm_dir), Ok(pointer), Ok(restore), Ok(_), Ok(common)) = (
        required_nonempty(args, "--target"),
        required_nonempty(args, "--wm-dir"),
        nonempty(args, "--pointer"),
        nonempty(args, "--restore"),
        nonempty(args, "--rules"),
        common_of(args),
    ) else {
        return refused_usage();
    };
    let Some((settle, step)) = pace_of(args) else {
        return Outcome::failed_line(RC_REFUSED, tick::render_no_rule());
    };
    tick::run(&tick::Request {
        target,
        wm_dir,
        pointer,
        socket: common.socket,
        capture_file: common.capture_file,
        state_dir: common.state_dir,
        restore,
        settle,
        step,
    })
}

/// `seat cycle`。
fn cycle_of(args: &[String]) -> Outcome {
    // 値欠け・空文字の `--rules` は使い方の誤り（他の flag と同じ極性・SRS NFR4）。
    let (Ok(target), Ok(wm_dir), Ok(restore), Ok(_), Ok(common)) = (
        required_nonempty(args, "--target"),
        required_nonempty(args, "--wm-dir"),
        nonempty(args, "--restore"),
        nonempty(args, "--rules"),
        common_of(args),
    ) else {
        return refused_usage();
    };
    let Some(state) = super::state_dir_of(common.state_dir) else {
        return Outcome::failed_line(
            RC_REFUSED,
            cycle::render(target, &Cycle::Refused(cycle::REASON_STATE_DIR), None),
        );
    };
    // 規則が読めない周は **1 key も送らずに** 断る（lock も取らない・fail-closed・`s2-07l.151`）。
    let Some((settle, step)) = pace_of(args) else {
        return Outcome::failed_line(
            RC_REFUSED,
            cycle::render(target, &Cycle::Refused(cycle::REASON_NO_RULE), Some(&state)),
        );
    };
    let result = cycle::run(&cycle::Request {
        target,
        wm_dir,
        socket: common.socket,
        capture_file: common.capture_file,
        state_dir: &state,
        restore,
        settle,
        step,
    });
    match result {
        Cycle::Done => Outcome::ok_line(cycle::render(target, &result, Some(&state))),
        Cycle::Refused(_) | Cycle::Failed(_) => {
            Outcome::failed_line(RC_REFUSED, cycle::render(target, &result, Some(&state)))
        }
    }
}

/// `seat externalize`（設計 working-memory.md §5.1）。
fn externalize_of(args: &[String]) -> Outcome {
    // 値欠け・空文字は使い方の誤り（他の flag と同じ極性・SRS NFR4）。
    let (Ok(target), Ok(wm_dir), Ok(anchor), Ok(plan), Ok(directives), Ok(user), Ok(trigger), Ok(role), Ok(_), Ok(state_dir)) = (
        required_nonempty(args, "--target"),
        required_nonempty(args, "--wm-dir"),
        required_nonempty(args, "--anchor"),
        required_nonempty(args, "--plan"),
        required_nonempty(args, "--directives"),
        nonempty(args, "--user"),
        nonempty(args, "--trigger"),
        nonempty(args, "--role"),
        nonempty(args, "--rules"),
        nonempty(args, "--state-dir"),
    ) else {
        return refused_usage();
    };
    // frontmatter の 1 行に入る字面だけを受ける（改行を含む名乗りは別の key を作りうる）。
    if [Some(target), role].into_iter().flatten().any(|text| text.contains(['\n', '\r'])) {
        return refused_usage();
    }
    let Some(trigger) = trigger.map_or(Some(Trigger::Manual), Trigger::parse) else {
        return refused_usage();
    };
    let refused = |err: ExternalizeError| Outcome::failed(RC_REFUSED, externalize::render_refused(&err));
    let Some(state) = super::state_dir_of(state_dir) else {
        return refused(ExternalizeError::StateDir);
    };
    // 上限は `rules` と同じ 1 本の口で解く（`--rules` が在ればその file・読めなければ断る・C5）。
    let Some(cap) = crate::rules::cli::open(args).ok().as_ref().and_then(externalize::cap_of) else {
        return refused(ExternalizeError::NoRule);
    };
    let request = externalize::Request {
        target,
        wm_dir: Path::new(wm_dir),
        state_dir: &state,
        anchor: Path::new(anchor),
        plan: Path::new(plan),
        directives: Path::new(directives),
        user: user.map(Path::new),
        trigger,
        role,
        cap,
    };
    match externalize::run(&request) {
        Ok(done) => Outcome::ok_line(externalize::render(&done)),
        Err(err) => refused(err),
    }
}

/// `seat rebrief`（設計 working-memory.md §5.2・read-only）。DATA を出せない周は stdout 0 行で rc 2。
fn rebrief_of(args: &[String]) -> Outcome {
    // 値欠け・空文字は使い方の誤り（他の flag と同じ極性・SRS NFR4）。
    let (Ok(target), Ok(wm_dir), Ok(anchor), Ok(bd), Ok(prefix), Ok(_), Ok(state_dir)) = (
        required_nonempty(args, "--target"),
        required_nonempty(args, "--wm-dir"),
        required_nonempty(args, "--anchor"),
        nonempty(args, "--bd"),
        nonempty(args, "--prefix"),
        nonempty(args, "--rules"),
        nonempty(args, "--state-dir"),
    ) else {
        return refused_usage();
    };
    let unavailable = |err: RebriefError| Outcome::failed_line(RC_BROKEN, rebrief::render_unavailable(err));
    let Some(state) = super::state_dir_of(state_dir) else {
        return unavailable(RebriefError::StateDir);
    };
    // 待ち上限は `rules` と同じ 1 本の口で解く（`--rules` が在ればその file・読めなければ断る・C5）。
    let Some(timeout) = crate::rules::cli::open(args).ok().as_ref().and_then(rebrief::timeout_of) else {
        return unavailable(RebriefError::NoRule);
    };
    let request = rebrief::Request {
        target,
        wm_dir: Path::new(wm_dir),
        state_dir: &state,
        anchor: Path::new(anchor),
        prefix,
        bd: bd.unwrap_or(rebrief::DEFAULT_BD),
        timeout,
    };
    match rebrief::run(&request) {
        Ok(lines) => Outcome::ok(lines),
        Err(err) => unavailable(err),
    }
}

/// `seat consume`（設計 working-memory.md §5.3）。
fn consume_of(args: &[String]) -> Outcome {
    // 値欠け・空文字は使い方の誤り（他の flag と同じ極性・SRS NFR4）。
    let (Ok(target), Ok(wm_dir), Ok(state_dir)) = (
        required_nonempty(args, "--target"),
        required_nonempty(args, "--wm-dir"),
        nonempty(args, "--state-dir"),
    ) else {
        return refused_usage();
    };
    let refused = |err: &ConsumeError| Outcome::failed_line(RC_REFUSED, consume::render_refused(err));
    let Some(state) = super::state_dir_of(state_dir) else {
        return refused(&ConsumeError::StateDir);
    };
    let request = consume::Request {
        target,
        wm_dir: Path::new(wm_dir),
        state_dir: &state,
    };
    match consume::run(&request) {
        Ok(done) => Outcome::ok_line(consume::render(&done)),
        Err(err) => refused(&err),
    }
}
