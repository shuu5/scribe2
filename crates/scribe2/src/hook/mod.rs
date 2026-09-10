//! `hook <event>` の入口と、注入計測の slot（設計 §3 / §4 / §6・FR19 / FR21 / FR24）。
//!
//! marker が自分の NAME を言い、state dir が紐づいているときだけ仕える。それ以外は
//! **stdout 0 byte・stderr 0 byte・rc 0** で黙る。未知 event も黙る（fail-open＝他の
//! 器と衝突しない）。**env も HOME も読まない**（憲法 C2.2）。
//!
//! 記録の置き場は `<state_dir>/inject.jsonl` ただ 1 つで、これが C6.3 の「消費を記録
//! する append-only store 1 つ」である。書き込みは fleet と**同じ lock 実装**
//! （[`store::append_line`]）を通す。

pub mod guard;
pub mod seat_guard;
pub mod vessel;

use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{self, LockPolicy, StoreError};
use crate::name::NAME;
use guard::Decision;
use seat_guard::SeatDecision;
use std::path::{Path, PathBuf};
use std::time::Instant;
use vessel::Served;

/// 注入記録の schema 版。非互換な変更で上げる。
pub const SCHEMA: u64 = 1;

/// 記録 file の名前。
const INJECT_FILE: &str = "inject.jsonl";

/// payload から拾う key（作業 dir）。
const KEY_CWD: &str = "cwd";
/// payload から拾う key（tool 名）。
const KEY_TOOL: &str = "tool_name";
/// payload から拾う key（編集先）。
const KEY_FILE: &str = "file_path";
/// payload から拾う key（notebook の編集先）。
const KEY_NOTEBOOK: &str = "notebook_path";
/// payload から拾う key（この session の transcript）。
const KEY_TRANSCRIPT: &str = "transcript_path";

/// `session-start` の event 名。
const EVENT_SESSION_START: &str = "session-start";
/// `pre-tool-use` の event 名。
const EVENT_PRE_TOOL_USE: &str = "pre-tool-use";

/// 注入 1 回の記録（FR21: who / what / when / bytes / tokens / wall）。
///
/// `tokens` が `Option` なのは、MVP が token を数える口を持たないためである
/// （数えていないことを `0` と書かず `null` で表す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectionRecord {
    /// schema 版。
    pub schema: u64,
    /// 誰が出したか。
    pub who: String,
    /// 何を出したか。
    pub what: String,
    /// いつの hook か（Claude Code の event 名）。
    pub when: String,
    /// 出力の byte 数。
    pub bytes: u64,
    /// token 数（数えていなければ `None`）。
    pub tokens: Option<u64>,
    /// hook 1 回の実測ミリ秒。
    pub wall_ms: u64,
}

impl InjectionRecord {
    /// 1 行の flat JSON にする。
    pub fn to_line(&self) -> String {
        json_lite::write_object(&[
            ("schema", Value::Num(self.schema)),
            ("who", Value::Str(self.who.clone())),
            ("what", Value::Str(self.what.clone())),
            ("when", Value::Str(self.when.clone())),
            ("bytes", Value::Num(self.bytes)),
            ("tokens", self.tokens.map_or(Value::Null, Value::Num)),
            ("wall_ms", Value::Num(self.wall_ms)),
        ])
    }
}

/// 記録 file の path。
pub fn inject_path(state_dir: &Path) -> PathBuf {
    state_dir.join(INJECT_FILE)
}

/// 記録を 1 件追記する。lock は fleet と同じ実装を通る（第 2 の writer を作らない）。
pub fn append(state_dir: &Path, record: &InjectionRecord) -> Result<Vec<store::Warning>, StoreError> {
    let policy = LockPolicy::embedded()?;
    store::append_line(&inject_path(state_dir), &record.to_line(), policy)
}

/// `hook` に続く引数と stdin の payload を捌く。
///
/// 仕えない周・未知 event は **1 byte も書かず rc 0** で終える（FR24）。
pub fn dispatch(args: &[String], payload: &str) -> Outcome {
    let started = Instant::now();
    let Some(cwd) = cwd_of(payload) else {
        return Outcome::ok(Vec::new());
    };
    let Some(root) = vessel::repo_root(&cwd) else {
        return Outcome::ok(Vec::new());
    };
    let Served::ByMe(version) = vessel::served(&root) else {
        return Outcome::ok(Vec::new());
    };
    let Some(dir) = state_dir_of(args, &root) else {
        return Outcome::ok(Vec::new());
    };
    match args.first().map(String::as_str) {
        Some(EVENT_SESSION_START) => session_start(&root, version, &dir, started),
        Some(EVENT_PRE_TOOL_USE) => pre_tool_use(&root, &cwd, payload, &dir, started),
        _ => Outcome::ok(Vec::new()),
    }
}

/// 記録の置き場。`--state-dir` が上書きし、無ければ repo の git 設定から読む。
fn state_dir_of(args: &[String], root: &Path) -> Option<PathBuf> {
    let at = args.iter().position(|arg| arg == "--state-dir");
    match at.and_then(|found| args.get(found + 1)) {
        Some(found) if !found.starts_with("--") => Some(PathBuf::from(found)),
        _ => vessel::state_dir(root),
    }
}

/// payload の `cwd`。無ければ process の cwd（`current_dir` は syscall＝env ではない）。
fn cwd_of(payload: &str) -> Option<PathBuf> {
    match field(payload, KEY_CWD) {
        Some(found) => Some(PathBuf::from(found)),
        None => std::env::current_dir().ok(),
    }
}

/// Claude Code の hook payload から文字列 field を 1 つ抜く。
///
/// `json_lite` は **flat な object 専用**で、payload は `tool_input` を入れ子に持つ
/// ため通らない。payload は外が形を決める入力なので、要る key だけを字面で拾う最小の
/// reader をここに置く（**書き側**の `inject.jsonl` は `json_lite` で書く）。
///
/// 拾うのは **key として現れた occurrence だけ**である（同綴りの直後の非空白が `:`）。
/// 字面の 1 発目を無条件に拾うと、`"file_path"` という**値**を本物の key より手前へ置く
/// だけで guard が別の path を判定し、write-set の外が通る（fail-open）。JSON では
/// 文字列の内側の `"` は必ず escape されるので、値として現れた同綴りの次は `,` か `}`
/// になり、この判定で弁別できる。escape は解かない: 使うのは path と tool 名だけで、
/// いずれも `\` を含まない。
fn field(src: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut rest = src;
    loop {
        let (_, after) = rest.split_once(&needle)?;
        match after.trim_start().strip_prefix(':') {
            None => rest = after,
            Some(value) => {
                return value
                    .trim_start()
                    .strip_prefix('"')
                    .and_then(|body| body.split_once('"'))
                    .map(|(found, _)| found.to_owned())
            }
        }
    }
}

/// `u128` の実測値を記録用の `u64` へ落とす（溢れたら上限で止める）。
fn as_u64(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// 出した 1 行についての記録を組む。
fn record(who: &str, what: &str, when: &str, line: &str, started: Instant) -> InjectionRecord {
    InjectionRecord {
        schema: SCHEMA,
        who: format!("hook:{who}"),
        what: what.to_owned(),
        when: when.to_owned(),
        // 出力層（`emit` / `emit_err`）が付ける改行 1 byte を含めた実出力の byte 数。
        bytes: as_u64(line.len() as u128 + 1),
        tokens: None,
        wall_ms: as_u64(started.elapsed().as_millis()),
    }
}

/// 記録を追記し、黙って済ませない出来事を stderr の行にして返す。
///
/// **記録の失敗で hook を落とさない**（FR21 は推奨で、判定そのものではない）。
/// ただし黙って消さず 1 行 surface する。
fn record_lines(dir: &Path, entry: &InjectionRecord) -> Vec<String> {
    match append(dir, entry) {
        Ok(warnings) => warnings.iter().map(|w| w.as_str().to_owned()).collect(),
        Err(err) => vec![err.to_string()],
    }
}

/// 名乗りの 1 行を出し、その 1 行についての記録を 1 件書く。
fn session_start(root: &Path, version: u64, dir: &Path, started: Instant) -> Outcome {
    let line = format!(
        "[{NAME}/SessionStart] served version={version} root={}",
        root.display()
    );
    let entry = record(
        EVENT_SESSION_START,
        "session-start-header",
        "SessionStart",
        &line,
        started,
    );
    let mut outcome = Outcome::ok_line(line);
    outcome.err = record_lines(dir, &entry);
    outcome
}

/// write-set の外への編集を編集の時点で止める。deny は rc 2 + stderr 1 行 + stdout 0 byte。
fn pre_tool_use(
    root: &Path,
    cwd: &Path,
    payload: &str,
    dir: &Path,
    started: Instant,
) -> Outcome {
    let Some(git_dir) = vessel::git_dir(cwd) else {
        return Outcome::ok(Vec::new());
    };
    let tool = field(payload, KEY_TOOL).unwrap_or_default();
    let path = field(payload, KEY_FILE).or_else(|| field(payload, KEY_NOTEBOOK));
    match guard::decide(root, cwd, &git_dir, &tool, path.as_deref()) {
        // write-set が通した周にだけ seat guard を評価する（**deny 文は write-set が先**＝
        // 2 つの門が同時に落ちる周に、直す側がどちらを直せばよいか読めなくならないため）。
        Decision::Inactive | Decision::Allow => {
            let transcript = field(payload, KEY_TRANSCRIPT);
            let decided =
                seat_guard::decide(root, cwd, &tool, path.as_deref(), transcript.as_deref());
            seat_outcome(&decided, dir, started)
        }
        Decision::Deny(line) => {
            let entry = record(EVENT_PRE_TOOL_USE, "deny", "PreToolUse", &line, started);
            // deny の外形は「rc 2 + stderr 1 行 + stdout 0 byte」（FR20・必須）で、
            // stderr は丸ごと model への判定文になる。記録（FR21・推奨）の警告や失敗を
            // ここへ足すと判定文が濁るので、deny の周だけは戻りを stderr へ載せない。
            let _ = append(dir, &entry);
            Outcome::failed_line(RC_BROKEN, line)
        }
    }
}

/// seat guard の判定を外形へ写す。通す周は **1 byte も書かない**。
///
/// 判定（[`seat_guard::decide`]）と外形をここで分けているのは憲法 C4 の引数上限である
/// ——1 本に畳むと判定の 5 引数へ記録の 2 引数が乗って上限を超える。
fn seat_outcome(decided: &SeatDecision, dir: &Path, started: Instant) -> Outcome {
    match decided {
        // 通す 2 つは記録も残さない（hook budget を毎編集ごとの追記で食い潰さない）。
        SeatDecision::Allow | SeatDecision::Externalize => Outcome::ok(Vec::new()),
        SeatDecision::Deny(line) => {
            let entry = record(EVENT_PRE_TOOL_USE, "seat-guard-deny", "PreToolUse", line, started);
            // deny の外形は write-set guard と同じ（rc 2 + stderr 1 行 + stdout 0 byte）。
            // 記録の警告を stderr へ足すと判定文が濁るので戻りは載せない。
            let _ = append(dir, &entry);
            Outcome::failed_line(RC_BROKEN, line.clone())
        }
        // **測れない周は通す**。ただし黙って通すと「測れていない」ことが誰にも見えないので
        // 記録だけ 1 行残す（stderr へは出さない＝allow の周に判定文を濁さない）。
        SeatDecision::Unmeasured(reason) => {
            let entry = silent(
                EVENT_PRE_TOOL_USE,
                &format!("seat-guard-unmeasured reason={reason}"),
                started,
            );
            let _ = append(dir, &entry);
            Outcome::ok(Vec::new())
        }
    }
}

/// 1 byte も出さなかった周の記録（`bytes` は実出力どおり 0）。
fn silent(who: &str, what: &str, started: Instant) -> InjectionRecord {
    InjectionRecord {
        schema: SCHEMA,
        who: format!("hook:{who}"),
        what: what.to_owned(),
        when: "PreToolUse".to_owned(),
        bytes: 0,
        tokens: None,
        wall_ms: as_u64(started.elapsed().as_millis()),
    }
}
