//! `fleet` subcommand の面（設計 §5 / §11）。
//!
//! **env も HOME も読まない**（憲法 C2.2・ADR-0004 §2.4）。置き場は `--state-dir` が勝ち、無ければ
//! `seat` と**同じ 1 関数**（[`crate::seat::state_dir_of`]）で git 設定から解く（第 2 の解決を持たない・
//! 解けない周は typed に断り store を作らない）。出力は行を組んで返すだけで、stdout / stderr
//! へは bin 側の `emit` / `emit_err` が書く。

use super::select;
use super::store::{self, LockPolicy, StoreError};
use super::usage;
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::rules::manifest::Manifest;
use crate::rules::{RuleError, RuleValue};
use crate::seat::StateDir;
use super::{json_lite, replay, Event, EventKind, Stage, State, SCHEMA};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// 口座選定の rules 行（session 用の閾値・設計 account-autonomy.md §3）。
const ROW_SELECTION: &str = "R-C9-1";

/// `fleet` の使い方。
pub fn usage() -> String {
    "usage: fleet <record|show|export|usage|select [--anchor DIR]> [--state-dir D] [flags]".to_owned()
}

/// `fleet` の verb（閉じた列）。置き場を解くのは既知の verb の周だけ（verb の無い周・未知の verb の周は
/// 従来どおり使い方の 1 行・設計 §11 (1)）。
#[derive(Clone, Copy)]
enum Verb {
    /// event を 1 件追記する。
    Record,
    /// 便 1 件の現在地。
    Show,
    /// 跨版 面 2 の export。
    Export,
    /// 口座残量の計測 / 表示。
    Usage,
    /// 口座を 1 つ選ぶ。
    Select,
}

impl Verb {
    /// 字面から引く。未知なら `None`。
    fn parse(text: &str) -> Option<Self> {
        match text {
            "record" => Some(Self::Record),
            "show" => Some(Self::Show),
            "export" => Some(Self::Export),
            "usage" => Some(Self::Usage),
            "select" => Some(Self::Select),
            _ => None,
        }
    }
}

/// `fleet` に続く引数を捌く。全 verb が同じ入口（[`place`]）で置き場を解く。
pub fn dispatch(args: &[String]) -> Outcome {
    let Some(verb) = args.first().and_then(|text| Verb::parse(text)) else {
        return Outcome::failed(RC_REFUSED, vec![usage()]);
    };
    let state = match place(args) {
        Ok(found) => found,
        Err(lines) => return Outcome::failed(RC_REFUSED, lines),
    };
    let dir = state.path.as_path();
    match verb {
        Verb::Record => record(args, dir),
        Verb::Show => show(args, dir),
        Verb::Export => export(dir),
        Verb::Usage => super::usage::run_in(args, &state),
        Verb::Select => select_account(args, dir),
    }
}

/// 置き場を解く（`--state-dir` > git 設定・`seat` と同じ 1 関数・出所付き）。値欠けの flag は黙って落とさず断る
/// （NFR4）。解けない周は `fleet: refused reason=state-dir` の 1 行 + 使い方（store を作らない・rc 1 は呼び手）。
fn place(args: &[String]) -> Result<StateDir, Vec<String>> {
    let flag = optional(args, "--state-dir").map_err(|reason| vec![format!("fleet: {reason}"), usage()])?;
    crate::seat::state_dir_of(flag).ok_or_else(|| {
        vec![format!("fleet: refused reason={}", crate::seat::cycle::REASON_STATE_DIR), usage()]
    })
}

/// 口座を 1 つ選ぶ（設計 account-autonomy.md §3）。引数と rules 行を先に読み、選定の直前に FR33 の計測を
/// 1 回撃ち（その行は stderr 側へ）、log を replay して純関数へ渡す。**候補なしも rc 0**（断りではない・
/// FailOpen）。計測が撃てない周は `fleet usage` の rc のまま返し、選ばない。
///
/// 前計測は**鮮度つき**（設計 §13・[`usage::Freshness::Within`]）: 新しい実測を持つ口座は測り直さず、測り直した口座が
/// 読みに届かなかった周は最新の実測を保つ（stderr に `kept` の 1 行）。stdout の 1 行形は不変。
///
/// 除外集合（設計 §14 (3)・account-lifecycle.md §17 の約束 4 / 5）: `--purpose run` は `--exclude` の集合 ∪ 席の登録
/// row の口座（`--anchor DIR` が在ればその anchor の row だけ・無い周は置き場の全 row＝保守側）∪ host の面が宣言した
/// 群の候補の口座（host 全体・`--anchor` で絞らない）。`--purpose session` は `--exclude` だけ（row も群も読まない）。
fn select_account(args: &[String], dir: &Path) -> Outcome {
    let SelectFlags { purpose, model, mut exclude, anchor } = match select_flags(args) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(RC_REFUSED, vec![format!("fleet: {reason}"), usage()]),
    };
    let (manifest, threshold_pct) = match selection_rules(args, dir) {
        Ok(found) => found,
        Err(lines) => return Outcome::failed(RC_REFUSED, lines),
    };
    // 前計測は鮮度つき（設計 §13・秒は rules 行 `fleet.usage_fresh_s`・計測の口は `fleet usage` と同じ 1 本で方針だけ
    // が違う）。行の無い manifest は `fleet usage` の rules 行の読み手と同じ極性で断る（測らない・選ばない）。
    let fresh_s = match usage::fresh_of(&manifest) {
        Ok(found) => found,
        Err(error) => return Outcome::failed(error.rc(), vec![error.to_string()]),
    };
    let measured = usage::run_with(args, dir, usage::Freshness::Within(fresh_s));
    if measured.rc != RC_OK {
        return measured;
    }
    let state = match load(dir) {
        Ok(found) => found,
        Err(lines) => return Outcome::failed(RC_BROKEN, lines),
    };
    // 候補は有効な口座の集合だけ（退役中の口座を候補に入れない・account-lifecycle.md §3）。
    let labels = super::effective_accounts(&manifest, &state);
    // 便用は席の口座を外す（`select_for_run` と同じ読み手・`--anchor` の有無で絞りが変わる・§14）。加えて host の面が
    // 宣言した群の候補の口座を host 全体で外す（`select_for_run` と**同じ除外**・設計 account-lifecycle.md §17 の
    // 約束 4 / 5＝便用の候補を作る口が 2 つ在るので、片方だけでは `fleet select` の口から群の口座が漏れる）。
    // session 用は群を読まない（約束 6・席を起こす口座の選び方は不変）。
    if purpose == select::Purpose::Run {
        exclude.extend(state.registered_accounts(anchor.map(Path::new)));
        exclude.extend(manifest.grouped_accounts());
    }
    let now = now_utc();
    // 走行中の便数は便用の 2 つ目の鍵（`select_for_run` と同じ導出・ADR-0027 §2.3）。
    let found = select::select(&select::Input {
        labels: &labels,
        allowance: &state.allowance,
        purpose,
        model,
        exclude: &exclude,
        inflight: &state.inflight_by_account(),
        threshold_pct,
        now: &now,
        // 留まる口座は席の立て直しだけが渡す（`fleet select` の外形は不変・`s2-07l.312`）。
        prefer: None,
    });
    let mut outcome = Outcome::ok(vec![select::line(purpose, &found)]);
    outcome.err = measured.out.into_iter().chain(measured.err).collect();
    outcome
}

/// `fleet select` の引数（[`select_flags`] が読む）。
struct SelectFlags<'a> {
    /// `--purpose`。
    purpose: select::Purpose,
    /// `--model`（字面のまま）。
    model: Option<&'a str>,
    /// `--exclude` の集合。
    exclude: BTreeSet<String>,
    /// `--anchor DIR`（便用だけ・設計 account-autonomy.md §14 (3)）。
    anchor: Option<&'a str>,
}

/// `--purpose`（必須）・`--model`・`--exclude`（複数可）・`--anchor`（便用だけ）を読む。
///
/// `--model` は閉じた表（[`select::Model::parse`]・別名か表示名）で受け、表に無い値は typed に断る（字面は
/// そのまま選定へ渡し、型にするのは選定の中・`s2-07l.297`）。`--anchor` は値欠けを `--exclude` と同じく断り、
/// session 用に付いた周も断る（session 用は row を読まない＝絞る対象が無い flag を黙って落とさない・NFR4）。
fn select_flags(args: &[String]) -> Result<SelectFlags<'_>, String> {
    let text = required(args, "--purpose")?;
    let purpose = select::Purpose::parse(text).ok_or(format!("purpose {text} は run でも session でもない"))?;
    let model = optional(args, "--model")?;
    if let Some(found) = model.filter(|found| select::Model::parse(found).is_none()) {
        let taken: Vec<&str> = select::MODELS.iter().map(|model| model.display()).collect();
        return Err(format!("model {found} は未知である（取るのは {}）", taken.join(" / ")));
    }
    let anchor = optional(args, "--anchor")?;
    if anchor.is_some() && purpose != select::Purpose::Run {
        return Err("--anchor は --purpose run だけが取る".to_owned());
    }
    Ok(SelectFlags { purpose, model, exclude: excludes(args)?, anchor })
}

/// `--exclude L` を全部読む。値欠けは黙って落とさず断る（SRS NFR4）。
fn excludes(args: &[String]) -> Result<BTreeSet<String>, String> {
    let mut found = BTreeSet::new();
    for (at, arg) in args.iter().enumerate() {
        if arg != "--exclude" {
            continue;
        }
        match args.get(at + 1) {
            Some(label) if !label.starts_with("--") => {
                found.insert(label.clone());
            }
            _ => return Err("--exclude に値が無い".to_owned()),
        }
    }
    Ok(found)
}

/// 宣言（口座 label の出所）と R-C9-1 の値。manifest は `fleet usage` と同じ口で読む
/// （`--rules PATH` か埋め込み + `<dir>/host.toml`・env を読まない・[`super::usage::declared`]）。
fn selection_rules(args: &[String], dir: &Path) -> Result<(Manifest, u64), Vec<String>> {
    let rules = optional(args, "--rules").map_err(|reason| vec![format!("fleet: {reason}")])?;
    let manifest = super::usage::declared(rules, dir).map_err(|error| vec![error.to_string()])?;
    let threshold = threshold_of(&manifest).map_err(|error| vec![error.to_string()])?;
    Ok((manifest, threshold))
}

/// R-C9-1 の値（session 用の閾値・使用率の百分率）。無い・不発効・整数でない行は `RuleError`。
fn threshold_of(manifest: &Manifest) -> Result<u64, RuleError> {
    let row = manifest
        .get(ROW_SELECTION)
        .ok_or_else(|| RuleError::new(0, format!("{ROW_SELECTION} が無い")))?;
    if !row.enabled {
        return Err(RuleError::new(row.line, format!("{ROW_SELECTION} は不発効である")));
    }
    match row.value {
        RuleValue::Int(found) => Ok(found),
        _ => Err(RuleError::new(row.line, format!("{ROW_SELECTION} が整数でない"))),
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

/// 任意の flag。**値欠けは黙って落とさず error にする**（SRS NFR4）。`usage` も同じ読みを使う。
pub(super) fn optional<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, String> {
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
        Err(reason) => return Outcome::failed(RC_REFUSED, vec![format!("fleet: {reason}")]),
    };
    let policy = match LockPolicy::embedded() {
        Ok(found) => found,
        Err(err) => return Outcome::failed(RC_BROKEN, vec![err.to_string()]),
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
        Err(err) => Outcome::failed(RC_BROKEN, vec![err.to_string()]),
    }
}

/// 引数から event を組む。必須の欠けと未知の字面は理由つきで `Err`。
fn build_event(args: &[String]) -> Result<Event, String> {
    let kind_text = required(args, "--kind")?;
    let kind = EventKind::parse(kind_text).ok_or(format!("kind {kind_text} は未知である"))?;
    // 口座残量の行は**この口から書けない**。`record` は `--run` / `--bead` を要る形なので、
    // 口座の行をここで許すと便に紐づかない行に便 id が付き、必須 field も揃わない
    // （書き手は `fleet usage` の 1 本だけである・設計 fleet-usage.md §5）。
    // 席の登録の行も同じ（書き手は打刻の条件付きの `seat register` だけ）。口座の退役・戻しの行も同じ
    // （書き手は mv と対の `account retire` / `restore` だけ・dir を動かさずに状態だけを書く口を作らない）。
    if kind.is_allowance() || matches!(kind, EventKind::SeatRegistered | EventKind::AccountRetired | EventKind::AccountRestored) {
        return Err(format!("kind {kind_text} は record では書けない"));
    }
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
    // `--account` は席を立てた行の任意 field（便を起こした口座・ADR-0027 §2.3）。他の kind に渡されたら読み手が
    // malformed にする行なので、書く前に断る（読めない行を append-only の log に残さない）。
    let account = optional(args, "--account")?.map(str::to_owned);
    if account.is_some() && kind != EventKind::SeatSpawned {
        return Err(format!("kind {kind_text} は --account を持たない"));
    }
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
        allowance: None,
        registration: None,
        mark: None,
        account,
    })
}

/// 便 1 件の現在地を 1 行で出す。
fn show(args: &[String], dir: &Path) -> Outcome {
    let id = match required(args, "--run") {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(RC_REFUSED, vec![format!("fleet: {reason}")]),
    };
    let state = match load(dir) {
        Ok(found) => found,
        Err(lines) => return Outcome::failed(RC_BROKEN, lines),
    };
    match state.runs.get(id) {
        None => Outcome::failed(RC_REFUSED, vec!["fleet: no such run".to_owned()]),
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
        Err(lines) => return Outcome::failed(RC_BROKEN, lines),
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
///
/// `pipe` も event の `ts` と run id の stamp に同じ字面を使うので pub である
/// （時刻の字面を作る経路は器の中で 1 本・憲法 C2）。
pub fn now_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_utc(secs)
}

/// UNIX 秒を `YYYY-MM-DDTHH:MM:SSZ` にする。
pub fn format_utc(secs: u64) -> String {
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
