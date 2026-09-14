//! `hook <event>` の入口と、注入計測の slot（設計 §3 / §4 / §6・FR19 / FR21 / FR24）。
//!
//! marker が自分の NAME を言い、state dir が紐づいているときだけ仕える。それ以外は
//! **stdout 0 byte・stderr 0 byte・rc 0** で黙る。未知 event も黙る（fail-open＝他の
//! 器と衝突しない）。**env も HOME も読まない**（憲法 C2.2）。anchor（repo root）は生成 hooks.json の
//! shell 行が渡す `--project`（session の起動 dir）から解き、席が `cd` しても変わらない。**例外は席の
//! 権能**（[`role_guard`]・設計 seat-roles.md §4）: `--pane` が在る（席である）周は anchor を解けなくても
//! 黙らず、権能付きの操作を deny する。
//!
//! 記録の置き場は `<state_dir>/inject.jsonl` ただ 1 つで、これが C6.3 の「消費を記録
//! する append-only store 1 つ」である。書き込みは fleet と**同じ lock 実装**
//! （[`store::append_line`]）を通す。

pub mod guard;
pub mod permission;
pub mod role_guard;
pub mod seat_guard;
pub mod stamp;
pub mod vessel;

use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::json_tree;
use crate::fleet::store::{self, LockPolicy, StoreError};
use crate::name::NAME;
use crate::rules::manifest::Manifest;
use crate::seat::state::Event;
use guard::Decision;
use permission::PermissionDecision;
use role_guard::{Operation, RoleDecision, Seat};
use seat_guard::SeatDecision;
use std::path::{Path, PathBuf};
use std::time::Instant;
use vessel::Served;

/// 注入記録の schema 版。**読めなくなる変更で上げる**: optional な field の追加（`seat` / `ts`・
/// `s2-07l.150`）は同じ番号のまま（ADR-0004 §2.5 D-5・既存 key の名・順序・型は不変）。
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
/// payload から拾う key（`Bash` の command 行・`tool_input` の中）。
const KEY_TOOL_INPUT: &str = "tool_input";
/// payload から拾う key（`Bash` の command 行）。
const KEY_COMMAND: &str = "command";

/// `session-start` の event 名。
const EVENT_SESSION_START: &str = "session-start";
/// `pre-tool-use` の event 名。
const EVENT_PRE_TOOL_USE: &str = "pre-tool-use";
/// `permission-request` の event 名。
const EVENT_PERMISSION_REQUEST: &str = "permission-request";
/// `user-prompt-submit` の event 名（席の状態の打刻 = Busy・設計 seat-state.md §2）。
const EVENT_USER_PROMPT_SUBMIT: &str = "user-prompt-submit";
/// `stop` の event 名（席の状態の打刻 = Idle）。
const EVENT_STOP: &str = "stop";
/// 記録の置き場を上書きする flag。
const FLAG_STATE_DIR: &str = "--state-dir";
/// 自席の pane id を渡す flag（打刻と記録の `seat` 列が同じ値から解く）。
const FLAG_PANE: &str = "--pane";
/// tmux の socket を渡す flag（歯は独立 socket で撃つ）。
const FLAG_SOCKET: &str = "--tmux-socket";
/// session の起動 dir（anchor）を渡す flag。生成 hooks.json の shell 行が `$CLAUDE_PROJECT_DIR` から渡す
/// （席が `cd` しても変わらない・設計 seat-roles.md §4）。無い周（旧 hooks.json）は payload の `cwd` で解く。
const FLAG_PROJECT: &str = "--project";
/// rules manifest を差し替える flag（役割の行の歯の seam・`rules get --rules` と同じ形）。
const FLAG_RULES: &str = "--rules";

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
    /// どの席の記録か（潰した target・解けない周は `None`）。`tokens` と同じく、解いていない席を
    /// 空文字や推測で埋めない（憲法 C10）。
    pub seat: Option<String>,
    /// 書いた時刻（1970 年からの秒・UTC）。席の打刻 `state.jsonl` の `ts` と同じ時計・同じ単位。
    pub ts: u64,
}

impl InjectionRecord {
    /// 1 行の flat JSON にする。`seat` と `ts` は既存 key の後ろ（既存の名・順序・型は不変）。
    /// 空の席は `null` に倒す（空文字の席を作らない）。
    pub fn to_line(&self) -> String {
        let seat = self.seat.as_deref().filter(|found| !found.is_empty());
        json_lite::write_object(&[
            ("schema", Value::Num(self.schema)),
            ("who", Value::Str(self.who.clone())),
            ("what", Value::Str(self.what.clone())),
            ("when", Value::Str(self.when.clone())),
            ("bytes", Value::Num(self.bytes)),
            ("tokens", self.tokens.map_or(Value::Null, Value::Num)),
            ("wall_ms", Value::Num(self.wall_ms)),
            ("seat", seat.map_or(Value::Null, |found| Value::Str(found.to_owned()))),
            ("ts", Value::Num(self.ts)),
        ])
    }
}

/// 記録の `seat` 列の値（潰した target・潰して空になる target は `None`）。記録の書き手 4 面
/// （hook / inject / tick / cycle）がこの 1 本を通る＝席の字面の作り方を 2 面に持たない。
pub fn seat_name(target: &str) -> Option<String> {
    Some(crate::seat::sanitize_target(target)).filter(|found| !found.is_empty())
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
/// 仕えない周・未知 event は **1 byte も書かず rc 0** で終える（FR24）。ただし `pre-tool-use` で
/// `--pane` が在る（席である）のに anchor を解けない周は黙らず、権能付きの操作を deny する
/// （[`unanchored`]・席が repo の外へ `cd` しても guard は外れない・設計 seat-roles.md §4）。
pub fn dispatch(args: &[String], payload: &str) -> Outcome {
    let started = Instant::now();
    let Some(cwd) = cwd_of(payload) else {
        return Outcome::ok(Vec::new());
    };
    let Some((root, version, dir)) = anchor_of(args, &cwd) else {
        return unanchored(args, &cwd, payload, started);
    };
    let hooked = Hooked {
        root: &root,
        cwd: &cwd,
        dir: &dir,
        pane: flag_of(args, FLAG_PANE),
        socket: flag_of(args, FLAG_SOCKET),
        rules: flag_of(args, FLAG_RULES),
    };
    match args.first().map(String::as_str) {
        Some(EVENT_SESSION_START) => {
            let mut outcome = session_start(&hooked, version, started);
            // 名乗りの後に打刻（Idle）。打刻の失敗は名乗りの行も rc も変えない（席を止めない）。
            outcome.err.extend(stamp::stamp(args, payload, Event::SessionStart, &dir));
            outcome
        }
        Some(EVENT_PRE_TOOL_USE) => pre_tool_use(&hooked, payload, started),
        Some(EVENT_PERMISSION_REQUEST) => permission_request(&hooked, payload, started),
        Some(EVENT_USER_PROMPT_SUBMIT) => stamped(args, payload, Event::UserPromptSubmit, &dir),
        Some(EVENT_STOP) => stamped(args, payload, Event::Stop, &dir),
        _ => Outcome::ok(Vec::new()),
    }
}

/// anchor を解く: repo root は `--project`（無い・空なら payload の `cwd`・互換）から、`served` と state dir は
/// その root から。どれかが解けない周は `None`（仕えない側）。
fn anchor_of(args: &[String], cwd: &Path) -> Option<(PathBuf, u64, PathBuf)> {
    let start = flag_of(args, FLAG_PROJECT)
        .filter(|found| !found.trim().is_empty())
        .map_or_else(|| cwd.to_path_buf(), PathBuf::from);
    let root = vessel::repo_root(&start)?;
    let Served::ByMe(version) = vessel::served(&root) else {
        return None;
    };
    let dir = state_dir_of(args, &root)?;
    Some((root, version, dir))
}

/// anchor を解けない周。`pre-tool-use` で `--pane` が在れば（席なのに仕える repo が無い）権能付きの操作を
/// deny し（FailClosed・stderr 1 行・rc 2）、記録は置き場（`--state-dir`）が解ける周にだけ 1 行残す。
/// それ以外（pane が無い・他の event）は **1 byte も書かず rc 0**（FR24 の沈黙は pane が無い周にだけ当たる）。
fn unanchored(args: &[String], cwd: &Path, payload: &str, started: Instant) -> Outcome {
    let pane = flag_of(args, FLAG_PANE).filter(|found| !found.trim().is_empty());
    if args.first().map(String::as_str) != Some(EVENT_PRE_TOOL_USE) || pane.is_none() {
        return Outcome::ok(Vec::new());
    }
    let tool = field(payload, KEY_TOOL).unwrap_or_default();
    let path = field(payload, KEY_FILE).or_else(|| field(payload, KEY_NOTEBOOK));
    let command = command_of(payload);
    let op = Operation { tool: &tool, command: command.as_deref(), path: path.as_deref(), root: None, cwd };
    let Some(subject) = role_guard::subject(&op, None) else {
        return Outcome::ok(Vec::new());
    };
    let line = role_guard::unanchored_line(&subject);
    if let Some(dir) = flag_of(args, FLAG_STATE_DIR).map(PathBuf::from) {
        let hooked = Hooked {
            root: cwd,
            cwd,
            dir: &dir,
            pane,
            socket: flag_of(args, FLAG_SOCKET),
            rules: None,
        };
        return denied(&hooked, &format!("role-deny {}", subject.render()), line, started);
    }
    Outcome::failed_line(RC_BROKEN, line)
}

/// 仕える周に解いた材料（repo・作業 dir・置き場・席の出所）。各 event へ 1 つで渡す。
///
/// 畳むのは憲法 C4 の引数上限（R-C4-4.args = 5）ゆえ: `pre_tool_use` は既に 5 引数で、席の出所
/// （`--pane` / `--tmux-socket`）を素の引数で足せない。席は**記録を書く周と役割の判定にだけ**解く
/// （[`seat_of`] / [`role_outcome`]）。
struct Hooked<'a> {
    /// repo の root（anchor・`--project` から。無い周は payload の `cwd` から）。
    root: &'a Path,
    /// 作業 dir（payload の `cwd`）。
    cwd: &'a Path,
    /// 記録の置き場。
    dir: &'a Path,
    /// 自席の pane id（生成 hooks.json の shell 行が `$TMUX_PANE` から渡す・無い周は `None`）。
    pane: Option<&'a str>,
    /// tmux の socket（歯が独立 socket を渡す口・既定の server なら `None`）。
    socket: Option<&'a str>,
    /// rules manifest の差し替え（`--rules`・無ければ埋め込み）。
    rules: Option<&'a str>,
}

/// 記録の `seat` 列（`--pane` → target → 潰した字面）。pane が無い・空・解けない周は `None`
/// （空文字の席を作らない）。tmux を撃つので**記録を書く周にだけ**呼ぶ（毎編集 tmux を撃たない・NFR5）。
fn seat_of(hooked: &Hooked) -> Option<String> {
    let pane = hooked.pane.filter(|found| !found.trim().is_empty())?;
    let socket = hooked.socket.filter(|found| !found.trim().is_empty());
    crate::seat::target_of_pane(socket, pane).and_then(|target| seat_name(&target))
}

/// 打刻だけを行う event の外形: **stdout 0 byte・rc 0**（guard ではない・設計 seat-state.md §5）。
/// 書けなかった周の 1 行だけ stderr に載せる。
fn stamped(args: &[String], payload: &str, event: Event, dir: &Path) -> Outcome {
    let mut outcome = Outcome::ok(Vec::new());
    outcome.err = stamp::stamp(args, payload, event, dir);
    outcome
}

/// 記録の置き場。`--state-dir` が上書きし、無ければ repo の git 設定から読む。
fn state_dir_of(args: &[String], root: &Path) -> Option<PathBuf> {
    match flag_of(args, FLAG_STATE_DIR) {
        Some(found) => Some(PathBuf::from(found)),
        None => vessel::state_dir(root),
    }
}

/// `--<name> <値>` を読む。flag が無い・値が無い（末尾か次が別の flag）はどちらも `None`。
pub(crate) fn flag_of<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|arg| arg == name)?;
    args.get(at.saturating_add(1))
        .filter(|found| !found.starts_with("--"))
        .map(String::as_str)
}

/// payload の `cwd`。無ければ process の cwd（`current_dir` は syscall＝env ではない）。
fn cwd_of(payload: &str) -> Option<PathBuf> {
    match field(payload, KEY_CWD) {
        Some(found) => Some(PathBuf::from(found)),
        None => std::env::current_dir().ok(),
    }
}

/// payload の `tool_input.command`（`Bash` の command 行）。
///
/// command 行は `"` や `\` を含みうる（[`field`] は escape を解かないので、escape された `"` の手前で
/// 切れて後ろの subcommand を見落とす＝fail-open）。入れ子の reader（[`json_tree`]）で escape を解いて
/// 読み、payload が木として読めない周だけ [`field`] へ倒す。
fn command_of(payload: &str) -> Option<String> {
    let parsed = json_tree::parse(payload).ok();
    let nested = parsed
        .as_ref()
        .and_then(|tree| tree.get(KEY_TOOL_INPUT))
        .and_then(|input| input.get(KEY_COMMAND))
        .and_then(json_tree::Tree::as_str)
        .map(str::to_owned);
    nested.or_else(|| field(payload, KEY_COMMAND))
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
pub(crate) fn field(src: &str, key: &str) -> Option<String> {
    raw_value(src, key)?
        .strip_prefix('"')
        .and_then(|body| body.split_once('"'))
        .map(|(found, _)| found.to_owned())
}

/// payload から真偽の field を 1 つ抜く（`true` / `false` 以外・不在は `None`）。
///
/// key の弁別は [`field`] と同じ（値として現れた同綴りを拾わない）。`Stop` hook の
/// `stop_hook_active` を読むためのもので、文字列の field と型を混ぜない。
pub(crate) fn bool_field(src: &str, key: &str) -> Option<bool> {
    let value = raw_value(src, key)?;
    if value.starts_with("true") {
        Some(true)
    } else if value.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// `"<key>"` が **key として**現れた最初の occurrence の、`:` と空白を剥がした直後の残り。
fn raw_value<'a>(src: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let mut rest = src;
    loop {
        let (_, after) = rest.split_once(&needle)?;
        match after.trim_start().strip_prefix(':') {
            None => rest = after,
            Some(value) => return Some(value.trim_start()),
        }
    }
}

/// `u128` の実測値を記録用の `u64` へ落とす（溢れたら上限で止める）。
fn as_u64(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// 記録 1 件の中身（誰が・何を・いつの hook で・どの行を出したか）。
///
/// [`record`] は憲法 C4 の引数上限（R-C4-4.args = 5）ゆえに畳む——既に 5 引数で、席の出所
/// （[`Hooked`]）を足せない。[`silent`] は引数上限に触れないが、同じ構造体を共有するために同じ形へ畳む。
struct Emit<'a> {
    /// 出した hook（`hook:` の後ろ）。
    who: &'a str,
    /// 何を出したか。
    what: &'a str,
    /// Claude Code の event 名。
    when: &'a str,
    /// 出した 1 行（1 byte も出さない周は空）。
    line: &'a str,
}

/// 出した 1 行についての記録を組む。
fn record(emit: &Emit, hooked: &Hooked, started: Instant) -> InjectionRecord {
    InjectionRecord {
        // 出力層（`emit` / `emit_err`）が付ける改行 1 byte を含めた実出力の byte 数。
        bytes: as_u64(emit.line.len() as u128 + 1),
        ..silent(emit, hooked, started)
    }
}

/// 1 byte も出さなかった周の記録（`bytes` は実出力どおり 0）。
fn silent(emit: &Emit, hooked: &Hooked, started: Instant) -> InjectionRecord {
    InjectionRecord {
        schema: SCHEMA,
        who: format!("hook:{}", emit.who),
        what: emit.what.to_owned(),
        when: emit.when.to_owned(),
        bytes: 0,
        tokens: None,
        wall_ms: as_u64(started.elapsed().as_millis()),
        seat: seat_of(hooked),
        ts: crate::seat::state::now_secs(),
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
fn session_start(hooked: &Hooked, version: u64, started: Instant) -> Outcome {
    let line = format!(
        "[{NAME}/SessionStart] served version={version} root={}",
        hooked.root.display()
    );
    let emit = Emit {
        who: EVENT_SESSION_START,
        what: "session-start-header",
        when: "SessionStart",
        line: &line,
    };
    let entry = record(&emit, hooked, started);
    let mut outcome = Outcome::ok_line(line);
    outcome.err = record_lines(hooked.dir, &entry);
    brief(hooked, &mut outcome, started);
    outcome
}

/// 記録の `what`（席の指示文）。
const WHAT_BRIEF: &str = "session-start-brief";

/// 席の指示文を名乗りの後ろへ出す（設計 seat-roles.md §5・ADR-0022 §2.4・FR42）: pane → target → 登録 row で役割を解き、
/// 役割の雛形と rules 行 `role.<役割>` の値から生成した文を stdout に足し、記録 1 行（`what` = [`WHAT_BRIEF`]）を残す。
///
/// **登録の無い席・pane の無い周・target が解けない周は 0 byte**（断りも出さない・記録も増やさない）。読めない周
/// （event log・rules 行）は guard と同じ理由の 1 語を stderr に 1 行（席は止めない＝rc は変えない・注入は guard で
/// はない・設計 §6）。
fn brief(hooked: &Hooked, outcome: &mut Outcome, started: Instant) {
    let Some(pane) = hooked.pane.filter(|found| !found.trim().is_empty()) else {
        return;
    };
    let socket = hooked.socket.filter(|found| !found.trim().is_empty());
    let Some(target) = crate::seat::target_of_pane(socket, pane) else {
        return;
    };
    let Ok(events) = store::read_all(hooked.dir) else {
        outcome.err.push(brief_refused("registry-unreadable"));
        return;
    };
    let state = crate::fleet::replay(&events);
    let Some(row) = crate::seat::role::registration_of_target(&state, &target) else {
        return;
    };
    let manifest = hooked.rules.map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path)));
    let Ok(manifest) = manifest else {
        outcome.err.push(brief_refused("rules-unreadable"));
        return;
    };
    let Some(capabilities) = crate::seat::brief::capabilities_of(&manifest, row.role) else {
        outcome.err.push(brief_refused(&format!("no-row {}", role_guard::row_id(row.role))));
        return;
    };
    let text = crate::seat::brief::render(row.role, row, &capabilities);
    let emit = Emit { who: EVENT_SESSION_START, what: WHAT_BRIEF, when: "SessionStart", line: text.trim_end_matches('\n') };
    outcome.err.extend(record_lines(hooked.dir, &record(&emit, hooked, started)));
    outcome.out.extend(text.lines().map(str::to_owned));
}

/// 指示文を出せない周の 1 行（理由の 1 語つき・guard の断りと同じ形）。
fn brief_refused(reason: &str) -> String {
    format!("{NAME}: 席の指示文を出せない reason={reason}（席の登録 row と rules 行から権能を解けない）")
}

/// 編集と権能付きの操作を行為の時点で止める。deny は rc 2 + stderr 1 行 + stdout 0 byte。
///
/// 門の順は write-set guard → seat guard → role guard（既存の順のまま末尾に足す・**deny 文は先の門が先**＝
/// 2 つの門が同時に落ちる周に、直す側がどちらを直せばよいか読めなくならないため）。前 2 つは cwd の
/// git dir が要る（cwd が repo の外なら測れない＝従来どおり通す側）が、role guard は anchor から解くので
/// cwd に依らず評価する。
fn pre_tool_use(hooked: &Hooked, payload: &str, started: Instant) -> Outcome {
    let (root, cwd) = (hooked.root, hooked.cwd);
    let tool = field(payload, KEY_TOOL).unwrap_or_default();
    let path = field(payload, KEY_FILE).or_else(|| field(payload, KEY_NOTEBOOK));
    if let Some(git_dir) = vessel::git_dir(cwd) {
        if let Decision::Deny(line) = guard::decide(root, cwd, &git_dir, &tool, path.as_deref()) {
            return denied(hooked, "deny", line, started);
        }
        let transcript = field(payload, KEY_TRANSCRIPT);
        let decided = seat_guard::decide(root, cwd, &tool, path.as_deref(), transcript.as_deref());
        if let Some(outcome) = seat_outcome(&decided, hooked, started) {
            return outcome;
        }
    }
    let command = command_of(payload);
    let op = Operation { tool: &tool, command: command.as_deref(), path: path.as_deref(), root: Some(root), cwd };
    role_outcome(hooked, &op, started)
}

/// deny の外形（rc 2 + stderr 1 行 + stdout 0 byte・FR20）と記録 1 行。
///
/// stderr は丸ごと model への判定文になる。記録（FR21・推奨）の警告や失敗をここへ足すと判定文が
/// 濁るので、deny の周だけは戻りを stderr へ載せない。
fn denied(hooked: &Hooked, what: &str, line: String, started: Instant) -> Outcome {
    let emit = Emit { who: EVENT_PRE_TOOL_USE, what, when: "PreToolUse", line: &line };
    let _ = append(hooked.dir, &record(&emit, hooked, started));
    Outcome::failed_line(RC_BROKEN, line)
}

/// 1 byte も出さない周の記録 1 行（allow の周に判定文を濁さない・stderr へは出さない）。
fn noted(hooked: &Hooked, what: &str, started: Instant) {
    let emit = Emit { who: EVENT_PRE_TOOL_USE, what, when: "PreToolUse", line: "" };
    let _ = append(hooked.dir, &silent(&emit, hooked, started));
}

/// seat guard の判定を外形へ写す。止める周だけ `Some`（通す周は次の門へ・**1 byte も書かない**）。
///
/// 判定（[`seat_guard::decide`]）と外形をここで分けているのは憲法 C4 の引数上限である
/// ——1 本に畳むと判定の 5 引数へ記録の 2 引数が乗って上限を超える。
fn seat_outcome(decided: &SeatDecision, hooked: &Hooked, started: Instant) -> Option<Outcome> {
    match decided {
        // 通す 2 つは記録も残さない（hook budget を毎編集ごとの追記で食い潰さない）。
        SeatDecision::Allow | SeatDecision::Externalize => None,
        SeatDecision::Deny(line) => Some(denied(hooked, "seat-guard-deny", line.clone(), started)),
        // **測れない周は通す**。ただし黙って通すと「測れていない」ことが誰にも見えないので
        // 記録だけ 1 行残す。
        SeatDecision::Unmeasured(reason) => {
            noted(hooked, &format!("seat-guard-unmeasured reason={reason}"), started);
            None
        }
    }
}

/// role guard の判定を外形へ写す（設計 seat-roles.md §4）。
///
/// 権能付きでない操作（[`role_guard::subject`] が `None`）は tmux も event log も撃たずに通す（NFR5）。
/// 権能付きの操作は allow / deny の両方で記録 1 行（`what` = `role-<allow|deny> <種別>`）。pane が無い周は
/// 席ではない＝通す・記録なし。
fn role_outcome(hooked: &Hooked, op: &Operation, started: Instant) -> Outcome {
    let Some(subject) = role_guard::subject(op, Some(hooked.dir)) else {
        return Outcome::ok(Vec::new());
    };
    let seat = Seat {
        pane: hooked.pane,
        socket: hooked.socket,
        state_dir: hooked.dir,
        rules: hooked.rules.map(Path::new),
    };
    match role_guard::decide(&subject, &seat) {
        RoleDecision::Inactive => Outcome::ok(Vec::new()),
        RoleDecision::Allow => {
            noted(hooked, &format!("role-allow {}", subject.render()), started);
            Outcome::ok(Vec::new())
        }
        RoleDecision::Deny(line) => denied(hooked, &format!("role-deny {}", subject.render()), line, started),
    }
}

/// 内蔵 guard の承認の問いへ答える。**deny か沈黙のどちらか**で、allow は返さない。
fn permission_request(hooked: &Hooked, payload: &str, started: Instant) -> Outcome {
    let tool = field(payload, KEY_TOOL).unwrap_or_default();
    match permission::decide(&tool) {
        // 管轄外は **0 byte・rc 0**（FR24）＝Claude Code の既定の問いへ戻す。
        PermissionDecision::Silent => Outcome::ok(Vec::new()),
        PermissionDecision::Deny(line) => {
            let emit = Emit {
                who: EVENT_PERMISSION_REQUEST,
                what: "deny",
                when: "PermissionRequest",
                line: &line,
            };
            let entry = record(&emit, hooked, started);
            let mut outcome = Outcome::ok_line(line);
            outcome.err = record_lines(hooked.dir, &entry);
            outcome
        }
    }
}
