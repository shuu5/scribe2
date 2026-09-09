//! `fleet` subcommand の面（設計 §5）。
//!
//! **env も HOME も読まない**（憲法 C2.2・ADR-0004 §2.4）。置き場は `--state-dir` で
//! 必ず外から受け取り、既定を持たない。出力は行を組んで返すだけで、stdout / stderr
//! へは bin 側の `emit` / `emit_err` が書く。

use super::store::{self, LockPolicy, StoreError};
use super::{json_lite, replay, Event, EventKind, Stage, State, SCHEMA};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// subcommand 1 回の結果。rc は 0 / 1 / 2 の 3 値を取る（設計 §5）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// stdout へ書く行。
    pub out: Vec<String>,
    /// stderr へ書く行。
    pub err: Vec<String>,
    /// 終了コード。
    pub code: u8,
}

impl Outcome {
    /// stdout へ行を出し rc 0。
    fn ok(out: Vec<String>) -> Self {
        Self {
            out,
            err: Vec::new(),
            code: 0,
        }
    }

    /// stderr へ行を出し rc を立てる。**stdout へは 1 byte も書かない**。
    fn failed(code: u8, err: Vec<String>) -> Self {
        Self {
            out: Vec::new(),
            err,
            code,
        }
    }
}

/// `fleet` の使い方。
pub fn usage() -> String {
    "usage: fleet <record|show|export> --state-dir D [flags]".to_owned()
}

/// `fleet` に続く引数を捌く。
pub fn dispatch(args: &[String]) -> Outcome {
    let dir = match required(args, "--state-dir") {
        Ok(found) => PathBuf::from(found),
        Err(_) => return Outcome::failed(1, vec![usage()]),
    };
    match args.first().map(String::as_str) {
        Some("record") => record(args, &dir),
        Some("show") => show(args, &dir),
        Some("export") => export(&dir),
        _ => Outcome::failed(1, vec![usage()]),
    }
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
    match args.get(at + 1) {
        Some(found) if !found.starts_with("--") => Flag::Value(found),
        _ => Flag::Missing,
    }
}

/// 任意の flag。**値欠けは黙って落とさず error にする**（SRS NFR4）。
fn optional<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, String> {
    match flag(args, name) {
        Flag::Absent => Ok(None),
        Flag::Value(found) => Ok(Some(found)),
        Flag::Missing => Err(format!("{name} に値が無い")),
    }
}

/// 必須の flag。
fn required<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    optional(args, name)?.ok_or(format!("{name} が要る"))
}

/// event を 1 件追記する。
fn record(args: &[String], dir: &Path) -> Outcome {
    let event = match build_event(args) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(1, vec![format!("fleet: {reason}")]),
    };
    let policy = match LockPolicy::embedded() {
        Ok(found) => found,
        Err(err) => return Outcome::failed(2, vec![err.to_string()]),
    };
    match store::append(dir, &event, policy) {
        Ok(warnings) => {
            let mut outcome = Outcome::ok(vec![format!(
                "fleet: recorded {} run={}",
                event.kind.as_str(),
                event.run
            )]);
            outcome.err = warnings.iter().map(|w| w.as_str().to_owned()).collect();
            outcome
        }
        Err(err) => Outcome::failed(2, vec![err.to_string()]),
    }
}

/// 引数から event を組む。必須の欠けと未知の字面は理由つきで `Err`。
fn build_event(args: &[String]) -> Result<Event, String> {
    let kind_text = required(args, "--kind")?;
    let kind = EventKind::parse(kind_text).ok_or(format!("kind {kind_text} は未知である"))?;
    let stage = match optional(args, "--stage")? {
        None => None,
        Some(text) => Some(Stage::parse(text).ok_or(format!("stage {text} は未知である"))?),
    };
    let pid = match optional(args, "--pid")? {
        None => None,
        Some(text) => Some(text.parse::<u64>().map_err(|err| format!("pid が整数でない（{err}）"))?),
    };
    let actor = match optional(args, "--actor")? {
        None => kind.default_actor(),
        Some(text) => super::parse_actor(text)
            .ok_or(format!("actor {text} は machine でも human でもない"))?,
    };
    Ok(Event {
        schema: SCHEMA,
        ts: now_utc(),
        kind,
        run: required(args, "--run")?.to_owned(),
        bead: required(args, "--bead")?.to_owned(),
        host: host(),
        actor: actor.to_owned(),
        stage,
        seat: optional(args, "--seat")?.map(str::to_owned),
        pid,
        detail: optional(args, "--detail")?.map(str::to_owned),
    })
}

/// 便 1 件の現在地を 1 行で出す。
fn show(args: &[String], dir: &Path) -> Outcome {
    let id = match required(args, "--run") {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(1, vec![format!("fleet: {reason}")]),
    };
    let state = match load(dir) {
        Ok(found) => found,
        Err(lines) => return Outcome::failed(2, lines),
    };
    match state.runs.get(id) {
        None => Outcome::failed(1, vec!["fleet: no such run".to_owned()]),
        Some(run) => Outcome::ok(vec![format!(
            "run={} bead={} stage={} approved={} updated={}",
            run.id,
            run.bead,
            run.stage.as_str(),
            run.approved,
            run.updated
        )]),
    }
}

/// 跨版 面 2 の export。**read-only**（lock を取らず file を 1 byte も変えない）。
fn export(dir: &Path) -> Outcome {
    let state = match load(dir) {
        Ok(found) => found,
        Err(lines) => return Outcome::failed(2, lines),
    };
    let mut lines = vec![json_lite::write_object(&[
        ("schema", json_lite::Value::Num(SCHEMA)),
        ("kind", json_lite::Value::Str("export".to_owned())),
        ("host", json_lite::Value::Str(host())),
        ("runs", json_lite::Value::Num(state.runs.len() as u64)),
        ("seats", json_lite::Value::Num(state.seats.len() as u64)),
    ])];
    for run in state.runs.values() {
        lines.push(json_lite::write_object(&[
            ("kind", json_lite::Value::Str("run".to_owned())),
            ("id", json_lite::Value::Str(run.id.clone())),
            ("bead", json_lite::Value::Str(run.bead.clone())),
            ("stage", json_lite::Value::Str(run.stage.as_str().to_owned())),
            ("approved", json_lite::Value::Bool(run.approved)),
            ("updated", json_lite::Value::Str(run.updated.clone())),
        ]));
    }
    for seat in state.seats.values() {
        lines.push(json_lite::write_object(&[
            ("kind", json_lite::Value::Str("seat".to_owned())),
            ("id", json_lite::Value::Str(seat.id.clone())),
            ("run", json_lite::Value::Str(seat.run.clone())),
            ("state", json_lite::Value::Str(seat.state.as_str().to_owned())),
            ("updated", json_lite::Value::Str(seat.updated.clone())),
        ]));
    }
    Outcome::ok(lines)
}

/// store を読んで現在地を導く。読めなければ error 行を返す。
fn load(dir: &Path) -> Result<State, Vec<String>> {
    match store::read_all(dir) {
        Ok(events) => Ok(replay(&events)),
        Err(errors) => Err(errors.iter().map(StoreError::to_string).collect()),
    }
}

/// host 名。`/etc/hostname` → `hostname` コマンド → `"unknown"` の順。**env は読まない**。
pub fn host() -> String {
    if let Ok(text) = std::fs::read_to_string("/etc/hostname") {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    if let Ok(output) = std::process::Command::new("hostname").output() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !text.is_empty() {
            return text;
        }
    }
    "unknown".to_owned()
}

/// いまの UTC を `YYYY-MM-DDTHH:MM:SSZ` で返す。
fn now_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_utc(secs)
}

/// UNIX 秒を `YYYY-MM-DDTHH:MM:SSZ` にする。
fn format_utc(secs: u64) -> String {
    let days = secs / 86_400;
    let rest = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        (rest % 3_600) / 60,
        rest % 60
    )
}

/// 1970-01-01 からの日数を暦の (年, 月, 日) にする。
///
/// chrono を足さないための最小実装である（NFR3・直接依存 0 本）。
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let doe = shifted - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}
