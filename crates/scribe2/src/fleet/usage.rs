//! `fleet usage` の面（設計 docs/design/fleet-usage.md §2 / §3 / §5・ADR-0017 §2.2〜§2.4）。
//!
//! manifest の `[[account]]` を宣言順に読み、口座ごとに host の HTTP client（既定 `curl`）を
//! **子 process** で起こす。token は **stdin の設定行**（`-K -`）で渡し、argv・stdout・stderr・
//! event のどこにも出さない。読めた窓は `AllowanceMeasured`、読めなかった窓・口座は理由つきの
//! `AllowanceUnmeasured` として event log に追記する（0 に読み替えない）。
//!
//! 止めない・選ばない（ADR-0017 §2.4）: 口座の読みの失敗は行として記録して続行する
//! （[`UnmeasuredReason::POLARITY`] = FailOpen）。command を止めるのは引数・manifest・store の
//! 誤りだけである（[`UsageError::POLARITY`] = FailClosed）。**env も HOME も読まない**（C2.2）。
//!
//! 出力は 2 形（設計 §11）: 1 行形（`usage: account=…`・機械の読み手の面・字面は不変）と `--table` の表
//! （人の読む面・[`table`] の pure 関数 1 本が組む・置き場の出所は見出し行にだけ載る）。

mod read;
mod table;
use read::{credential_path, fetch, now_ms, read_credential, token_of, unmeasured, windows_of};
use table::{table, table_row, TableRow};
use super::cli::{format_utc, host, now_utc, optional};
use super::json_tree;
use super::store::{self, LockPolicy};
use super::{
    replay, wait, Allowance, AllowanceKey, AllowanceLatest, Completion, Event, EventKind, State, UnmeasuredReason,
    WindowKind, ACTOR_MACHINE, SCHEMA,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::headless::{self, Call, DEFAULT_CLAUDE};
use crate::pipe::confine;
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use crate::seat::StateDir;
use std::collections::BTreeMap;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// 残量を聞く先（host 固有の値ではない・manifest に置かない・設計 §3）。
pub const URL: &str = "https://api.anthropic.com/api/oauth/usage";

/// endpoint が要する beta header の値。
pub const BETA: &str = "oauth-2025-04-20";

/// `--curl` を渡さない周の client（PATH 解決は子 process の起動側）。
const DEFAULT_CLIENT: &str = "curl";

/// 待ち時間の上限を持つ rules 行。
const ROW_TIMEOUT: &str = "fleet.usage_timeout_s";

/// 選定の前計測の鮮度（秒）を持つ rules 行（設計 account-autonomy.md §13 (1)・読み手は [`fresh_of`] の 1 つ）。
const ROW_FRESH: &str = "fleet.usage_fresh_s";

/// curl が `--max-time` を超えたときの rc。
const RC_CLIENT_TIMEOUT: i32 = 28;

/// モデル別 7 日窓を表す `limits[]` 要素の `kind`。
const SCOPED_KIND: &str = "weekly_scoped";

/// reset 無しの窓の表示の字面（`resets=none`）。
const RESETS_NONE: &str = "none";

/// 宣言が 0 件の周に stderr へ出す 1 行（止めない・[`undeclared`]）。
const NO_DECLARATION: &str = "fleet usage: 宣言なし（[[account]] が 0 行・計測しない）";

/// credential の置き場（`<state_dir>/accounts/<label>/` の下）の file 名。
const CREDENTIAL_FILE: &str = ".credentials.json";

/// refresh の起動で止め方の猶予を持つ rules 行（`pipe stop` と共用・新しい行を足さない）。
const ROW_GRACE: &str = "pipe.stop_grace_ms";

/// refresh の起動に渡す prompt（code の定数 1 語・stdin から・設計 §3「token の refresh」）。
const REFRESH_PROMPT: &str = "ok";

/// refresh の起動の permission mode（lens と同じ定数・毎回明示する）。
const REFRESH_PERMISSION_MODE: &str = "plan";

/// refresh の起動の turn 上限。
const REFRESH_MAX_TURNS: u32 = 1;

/// refresh の子が signal で終わった周の rc の底（shell の慣習 128 + signal 番号）。
const RC_SIGNAL_BASE: i32 = 128;

/// token の refresh を試みた結果（設計 §3・stdout の行の末尾にだけ載る・event には載せない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    /// 子が rc 0 で終わった（credential が更新されたかは読み直しで決まる）。
    Ok,
    /// 子が rc 非 0 で終わった。
    Rc(u8),
    /// 上限（`fleet.usage_timeout_s`）までに終わらず、group ごと止めた。
    Timeout,
    /// 子を起こせなかった。
    Unlaunchable,
}

impl Refresh {
    /// 行に載せる字面（`ok` / `rc:<n>` / `timeout` / `unlaunchable`）。
    pub fn as_str(self) -> String {
        match self {
            Self::Ok => "ok".to_owned(),
            Self::Rc(rc) => format!("rc:{rc}"),
            Self::Timeout => "timeout".to_owned(),
            Self::Unlaunchable => "unlaunchable".to_owned(),
        }
    }
}

/// 計測の方針（設計 account-autonomy.md §13 (4)・閉じた enum・計測の実装は [`measure`] の 1 本のまま）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// 鮮度に関わらず全口座を測る（`fleet usage` の口・挙動不変）。
    Always,
    /// 鮮度つき（選定の前計測）: 最新の回が全部実測でその ts が `now − 秒` より新しい口座は測り直さず
    /// （子 process を起こさず event も書かない）、測り直した口座が読みに届かなかった（`HttpStatus` / `Timeout`）
    /// 周に最新の回が実測ならその Unmeasured を追記せず実測を最新のまま使う（§13 (2) / (3)）。
    Within(u64),
}

/// 口座を読むのに要る、口座に依らない材料。
struct Reader<'a> {
    /// 残量を聞く client（`--curl`）。
    client: &'a str,
    /// refresh に起こす claude（`--claude`）。
    claude: &'a str,
    /// 待ちの上限（秒・`fleet.usage_timeout_s`）。
    timeout_s: u64,
    /// refresh の子を止めるときの猶予（ms・`pipe.stop_grace_ms`）。
    grace_ms: u64,
    /// 鮮度の境（`now − fleet.usage_fresh_s` の ts・[`Freshness::Within`] の周だけ `Some`）。ts がこれより新しい
    /// 実測の口座は測り直さない。`None`（[`Freshness::Always`]）の周は鮮度も kept も見ない。
    cutoff: Option<String>,
    /// `--model`（数える窓の弁別・[`Freshness::Within`] の周だけ読む）。
    model: Option<&'a str>,
}

/// 口座の最新の回が全部実測であるときの、その ts と行（[`measured_round`]）。
struct MeasuredRound {
    /// 最新の回の ts。
    ts: String,
    /// 最新の回の行。
    rows: Vec<Allowance>,
}

/// 口座 1 つを方針に沿って読んだ結果（[`read_by_policy`]）。
enum Read {
    /// 測った（行を追記する）。refresh を試みた周はその結果。
    Measured(Vec<Allowance>, Option<Refresh>),
    /// 追記しない: 新しい実測を持つので測らなかった（理由なし）か、測ったが読みに届かず最新の実測を保った
    /// （kept の理由）。行はどちらも最新の実測。
    Kept(Vec<Allowance>, Option<UnmeasuredReason>),
}

/// `fleet usage` を止める誤り（設計 §6）。極性は fail-closed（[`Self::POLARITY`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    /// 引数の誤り（値欠けの flag）。
    Args(String),
    /// manifest が読めない（tracked の面か host の面）・rules 行が引けない。
    Manifest(String),
    /// store が書けない・読めない。
    Store(String),
}

impl UsageError {
    /// この境界の極性: 引数・manifest・store の誤りは command を止める。**Guard ではない**
    /// （行為を止めうる判定ではないので極性一覧には載らない・設計 §6）。
    pub const POLARITY: Polarity = Polarity {
        timing: Timing::InLoop,
        on_failure: OnFailure::FailClosed,
    };

    /// 終了コード。
    pub fn rc(&self) -> u8 {
        match self {
            Self::Args(_) | Self::Manifest(_) => RC_REFUSED,
            Self::Store(_) => RC_BROKEN,
        }
    }
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Args(reason) => write!(f, "fleet usage: {reason}"),
            Self::Manifest(reason) => write!(f, "fleet usage: manifest を読めない（{reason}）"),
            Self::Store(reason) => write!(f, "{reason}"),
        }
    }
}

/// `fleet usage` の入口。`--show` が在れば read-only の表示だけを行う。**1 行形**（機械の読み手の面・字面は不変）。
/// 鮮度に関わらず全口座を測る（[`Freshness::Always`]）。
pub fn run(args: &[String], dir: &Path) -> Outcome {
    run_with(args, dir, Freshness::Always)
}

/// 選定の前計測の口（**1 本**・設計 account-autonomy.md §13 (2)(3) / §18 (1)）: 鮮度つき（[`Freshness::Within`]）で、
/// 秒は rules 行 `fleet.usage_fresh_s`（[`fresh_of`]・manifest は計測と同じ宣言の口 [`declared`] で読む）。呼び手は
/// `fleet select`（`fleet::cli`）と便の起動の選定（`pipe::ratelimit::choose_account` の初回）の 2 つ＝鮮度の規則を
/// 2 か所に持たない（C2）。行の無い・不発効・型違いの manifest は測らずに断る（`fleet.usage_timeout_s` と同じ極性）。
pub fn run_fresh(args: &[String], dir: &Path) -> Outcome {
    let fresh_s = optional(args, "--rules")
        .map_err(UsageError::Args)
        .and_then(|rules| declared(rules, dir))
        .and_then(|manifest| fresh_of(&manifest));
    match fresh_s {
        Ok(secs) => run_with(args, dir, Freshness::Within(secs)),
        Err(error) => Outcome::failed(error.rc(), vec![error.to_string()]),
    }
}

/// 計測の方針を選べる入口（`fleet usage` の口は [`run`]・選定の前計測は [`run_fresh`]・設計 account-autonomy.md §13 (4)）。
/// `--show` は方針に依らず read-only の表示。
fn run_with(args: &[String], dir: &Path, freshness: Freshness) -> Outcome {
    let result = if args.iter().any(|arg| arg == "--show") {
        show(args, dir)
    } else {
        measure(args, dir, freshness)
    };
    result.unwrap_or_else(|error| Outcome::failed(error.rc(), vec![error.to_string()]))
}

/// `fleet` の dispatch からの入口（設計 §11 (2)）: `--table` は**出力の形**の指定で、計測か表示か（`--show`）とは
/// 直交する（`--show --table` = read-only の表・`--table` だけ = 計測してから表）。`--table` の無い周は [`run`] の
/// 1 行形そのまま。置き場の出所（[`StateDir`]）は表の見出し行にだけ載る。
pub fn run_in(args: &[String], place: &StateDir) -> Outcome {
    if !args.iter().any(|arg| arg == "--table") {
        return run(args, &place.path);
    }
    let read_only = args.iter().any(|arg| arg == "--show");
    tabled(args, place, read_only).unwrap_or_else(|error| Outcome::failed(error.rc(), vec![error.to_string()]))
}

/// 表を出す。`read_only` でなければ先に計測（[`measure`]・event の追記と stderr の行はそのまま）し、その後の replay の
/// `allowance` から組む（1 行形と同じ出所・stdout は表に置き換わる）。
fn tabled(args: &[String], place: &StateDir, read_only: bool) -> Result<Outcome, UsageError> {
    let dir = place.path.as_path();
    let mut outcome = if read_only { Outcome::ok(Vec::new()) } else { measure(args, dir, Freshness::Always)? };
    let (_, labels, _) = accounts(args, dir)?;
    let events = store::read_all(dir).map_err(|errors| UsageError::Store(joined(&errors)))?;
    let state = replay(&events);
    let rows: Vec<TableRow> = labels.iter().map(|label| table_row(label, &state)).collect();
    outcome.out = table(place, &rows);
    Ok(outcome)
}

/// event の `endpoint` に書く短い識別子。[`URL`] の末尾 2 段を `-` で繋いで導く（`oauth-usage`）。
pub fn endpoint() -> String {
    let mut segments: Vec<&str> = URL.rsplit('/').take(2).collect();
    segments.reverse();
    segments.join("-")
}

/// 口座を 1 つずつ読み、窓ごとに追記し、口座ごとに 1 行を返す。方針が [`Freshness::Within`] の周は口座ごとに
/// [`read_by_policy`] が測るか保つかを決める（kept の周は stderr に `usage: account=<label> kept reason=<reason>`
/// の 1 行・stdout の行は最新の実測の 1 行形のまま）。
fn measure(args: &[String], dir: &Path, freshness: Freshness) -> Result<Outcome, UsageError> {
    let client = optional(args, "--curl")
        .map_err(UsageError::Args)?
        .unwrap_or(DEFAULT_CLIENT)
        .to_owned();
    let claude = optional(args, "--claude")
        .map_err(UsageError::Args)?
        .unwrap_or(DEFAULT_CLAUDE)
        .to_owned();
    let (manifest, labels, state) = accounts(args, dir)?;
    if labels.is_empty() {
        return Ok(undeclared());
    }
    // `--model` は選定の flag（`fleet usage` の口は読まない＝外形不変）。
    let model = match freshness {
        Freshness::Always => None,
        Freshness::Within(_) => optional(args, "--model").map_err(UsageError::Args)?,
    };
    let reader = Reader {
        client: &client,
        claude: &claude,
        timeout_s: timeout_of(&manifest)?,
        grace_ms: grace_of(&manifest),
        cutoff: cutoff_of(freshness),
        model,
    };
    let policy = LockPolicy::embedded().map_err(|err| UsageError::Store(err.to_string()))?;
    let host = host();
    let mut outcome = Outcome::ok(Vec::new());
    for label in &labels {
        let prior = measured_round(label, &state.allowance, reader.model);
        let line = match read_by_policy(dir, label, &reader, prior) {
            Read::Kept(rows, reason) => {
                if let Some(reason) = reason {
                    outcome.err.push(format!("usage: account={label} kept reason={}", reason.as_str()));
                }
                render(label, &rows)
            }
            Read::Measured(rows, refreshed) => {
                let ts = now_utc();
                for row in &rows {
                    let warnings = store::append(dir, &event_of(&ts, &host, row), policy)
                        .map_err(|err| UsageError::Store(err.to_string()))?;
                    outcome.err.extend(warnings.iter().map(|w| w.as_str().to_owned()));
                }
                let mut line = render(label, &rows);
                if let Some(refresh) = refreshed {
                    line.push_str(&format!(" refresh={}", refresh.as_str()));
                }
                line
            }
        };
        outcome.out.push(line);
    }
    Ok(outcome)
}

/// 鮮度の境: [`Freshness::Within`] の周は `now − 秒` の ts（[`format_utc`] の字面・実測行の `ts` と同じ形で
/// 字面比較できる）・[`Freshness::Always`] は `None`。
fn cutoff_of(freshness: Freshness) -> Option<String> {
    match freshness {
        Freshness::Always => None,
        Freshness::Within(secs) => Some(format_utc((now_ms() / 1000).saturating_sub(secs))),
    }
}

/// 口座 1 つを方針に沿って読む（設計 account-autonomy.md §13 (2) / (3)）:
/// (2) 最新の回が全部実測でその ts が境より**新しい**口座は測らない（子 process を起こさない）。
/// (3) 測った結果が読みに届かなかった側（[`unreachable_reason`]）で、最新の回が実測（古さは問わない）なら
/// その実測を保つ（追記しない）。最新の回が Unmeasured・行なしの周は従来どおり測った行を返す。
/// [`Reader::cutoff`] が `None`（[`Freshness::Always`]）の周は (2) も (3) も掛けない。
fn read_by_policy(dir: &Path, label: &str, reader: &Reader<'_>, prior: Option<MeasuredRound>) -> Read {
    let Some(cutoff) = reader.cutoff.as_deref() else {
        let (rows, refreshed) = read_account(dir, label, reader);
        return Read::Measured(rows, refreshed);
    };
    if let Some(found) = prior.as_ref().filter(|found| found.ts.as_str() > cutoff) {
        return Read::Kept(found.rows.clone(), None);
    }
    let (rows, refreshed) = read_account(dir, label, reader);
    match (prior, unreachable_reason(&rows)) {
        (Some(found), Some(reason)) => Read::Kept(found.rows, Some(reason)),
        _ => Read::Measured(rows, refreshed),
    }
}

/// 測った結果が「読みに届かなかった」側（口座単位の `HttpStatus` / `Timeout`・設計 §13 (3)）ならその理由。
/// 本文の形の失敗（`BodyUnreadable` / `ShapeMismatch`）・credential の失敗・client の不在は含まない（値の側の
/// 失敗は従来どおり追記する）。
fn unreachable_reason(rows: &[Allowance]) -> Option<UnmeasuredReason> {
    match rows {
        [Allowance::Unmeasured(found)]
            if found.window.is_none()
                && matches!(found.reason, UnmeasuredReason::HttpStatus | UnmeasuredReason::Timeout) =>
        {
            Some(found.reason)
        }
        _ => None,
    }
}

/// 口座の最新の回が**全部実測**（数える窓に Unmeasured が無い・弁別は選定の [`super::select::counts`] と同じ
/// 1 関数）ならその ts と行。行が無い・数える窓に Unmeasured が在る周は `None`。古さは見ない（reset を過ぎた
/// 実測を古いと読むのは選定の側・鮮度の規則を 2 か所に持たない）。
fn measured_round(
    label: &str,
    allowance: &BTreeMap<AllowanceKey, AllowanceLatest>,
    model: Option<&str>,
) -> Option<MeasuredRound> {
    let (ts, rows) = latest_round(label, allowance)?;
    let unmeasured = rows.iter().any(|row| match row {
        Allowance::Unmeasured(found) => super::select::counts(model, found.window, found.model.as_deref()),
        Allowance::Measured(_) => false,
    });
    (!unmeasured).then_some(MeasuredRound { ts, rows })
}

/// rules 行 `pipe.stop_grace_ms` の ms。渡された manifest に発効した整数の行が無ければ埋め込みの行を読む
/// （`--rules` の fixture は計測の行だけを持ちうる）。どちらも読めない周は 0（猶予なしで KILL へ進む）。
fn grace_of(manifest: &Manifest) -> u64 {
    crate::seat::int_rule_of(manifest, ROW_GRACE)
        .or_else(|_| crate::seat::int_rule(ROW_GRACE))
        .unwrap_or(0)
}

/// replay の `allowance` から、口座ごとに最新の 1 回分を同じ 1 行形で出す（lock を取らない）。
fn show(args: &[String], dir: &Path) -> Result<Outcome, UsageError> {
    let (_, labels, state) = accounts(args, dir)?;
    if labels.is_empty() {
        return Ok(undeclared());
    }
    let lines = labels
        .iter()
        .filter_map(|label| latest_line(label, &state.allowance))
        .collect();
    Ok(Outcome::ok(lines))
}

/// 口座 1 つの最新の回（`ts` が最大の行の集まり）を 1 行にする。行が無ければ `None`。
fn latest_line(label: &str, allowance: &BTreeMap<AllowanceKey, AllowanceLatest>) -> Option<String> {
    latest_rows(label, allowance).map(|rows| render(label, &rows))
}

/// 口座 1 つの最新の回（`ts` が最大の行の集まり）。行が無ければ `None`（1 行形と表の同じ出所）。
fn latest_rows(label: &str, allowance: &BTreeMap<AllowanceKey, AllowanceLatest>) -> Option<Vec<Allowance>> {
    latest_round(label, allowance).map(|(_, rows)| rows)
}

/// 口座 1 つの最新の回の ts と行（[`latest_rows`] の出所・鮮度の判定は ts を読む）。行が無ければ `None`。
fn latest_round(label: &str, allowance: &BTreeMap<AllowanceKey, AllowanceLatest>) -> Option<(String, Vec<Allowance>)> {
    let mine: Vec<&AllowanceLatest> = allowance
        .iter()
        .filter(|(key, _)| key.account == label)
        .map(|(_, latest)| latest)
        .collect();
    let newest = mine.iter().map(|latest| latest.ts.as_str()).max()?;
    let rows = mine
        .iter()
        .filter(|latest| latest.ts == newest)
        .map(|latest| latest.allowance.clone())
        .collect();
    Some((newest.to_owned(), rows))
}

/// 宣言を読む: tracked の面（`rules` = `--rules PATH` か埋め込み）に host の面（`<dir>/host.toml`）を合わせる
/// （[`crate::rules::read`]・設計 account-lifecycle.md §2）。読めない周は [`UsageError::Manifest`]（1 行・FailClosed）。
/// `fleet select` も同じ口で読む（計測と選定が別の宣言を読まない）。
pub(super) fn declared(rules: Option<&str>, dir: &Path) -> Result<Manifest, UsageError> {
    crate::rules::read(rules.map(Path::new), Some(dir)).map_err(|errors| UsageError::Manifest(joined(&errors)))
}

/// manifest を読み、**有効な口座の集合**（宣言 − 退役中・[`super::effective_accounts`]）の label を宣言順で返す
/// （退役中の口座は測らない・account-lifecycle.md §3）。読んだ置き場の replay も返す（鮮度の判定と `--show` が
/// 同じ 1 回の読みを使う）。event log を読めない周は [`UsageError::Store`]。
fn accounts(args: &[String], dir: &Path) -> Result<(Manifest, Vec<String>, super::State), UsageError> {
    let manifest = declared(optional(args, "--rules").map_err(UsageError::Args)?, dir)?;
    let events = store::read_all(dir).map_err(|errors| UsageError::Store(joined(&errors)))?;
    let state = replay(&events);
    let labels = super::effective_accounts(&manifest, &state);
    Ok((manifest, labels, state))
}

/// 宣言が 0 件の周の結果（設計 account-lifecycle.md §2「宣言なしを出す・止めない」）: stdout 0 行・stderr に 1 行・rc 0・
/// 何も書かない。host の面の無い host（tracked の面は口座を持たない）はここへ来る＝選定は「候補なし」に倒れる。
fn undeclared() -> Outcome {
    let mut outcome = Outcome::ok(Vec::new());
    outcome.err.push(NO_DECLARATION.to_owned());
    outcome
}

/// rules 行 `fleet.usage_timeout_s` の秒。無い・不発効・型違いは断る。
fn timeout_of(manifest: &Manifest) -> Result<u64, UsageError> {
    int_row_of(manifest, ROW_TIMEOUT)
}

/// rules 行 `fleet.usage_fresh_s` の秒（設計 account-autonomy.md §13 (1)）。選定の前計測の口（[`run_fresh`]）が
/// [`Freshness::Within`] に渡す。無い・不発効・型違いは `fleet.usage_timeout_s` と**同じ読み手**で断る
/// （極性・字面・rc は同じ・読み手を増やさない）。
fn fresh_of(manifest: &Manifest) -> Result<u64, UsageError> {
    int_row_of(manifest, ROW_FRESH)
}

/// 整数の rules 行の値。無い・不発効・型違いは [`UsageError::Manifest`]（FailClosed）。
fn int_row_of(manifest: &Manifest, row_id: &str) -> Result<u64, UsageError> {
    let row = manifest
        .get(row_id)
        .ok_or_else(|| UsageError::Manifest(format!("{row_id} が無い")))?;
    if !row.enabled {
        return Err(UsageError::Manifest(format!("{row_id} は不発効である")));
    }
    match row.value {
        RuleValue::Int(found) => Ok(found),
        _ => Err(UsageError::Manifest(format!("{row_id} が整数でない"))),
    }
}

/// 誤りの列を 1 本の文字列へ畳む。
fn joined<E: std::fmt::Display>(errors: &[E]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<String>>()
        .join(" / ")
}

/// 口座 1 つを読む。口座単位の失敗は窓を持たない Unmeasured 1 行になる。
///
/// token が期限切れの周に限り refresh の子を **1 回**起こし、credential を読み直して測る（設計 §3
/// 「token の refresh」）。読み直してなお期限切れなら `TokenExpired` のまま（loop しない）。試みた周だけ
/// 結果を返す。
fn read_account(dir: &Path, label: &str, reader: &Reader<'_>) -> (Vec<Allowance>, Option<Refresh>) {
    let path = credential_path(dir, label);
    let mut token = read_credential(&path).and_then(|text| token_of(&text, now_ms()));
    let mut refreshed = None;
    if token == Err(UnmeasuredReason::TokenExpired) {
        refreshed = Some(refresh(dir, label, reader));
        token = read_credential(&path).and_then(|text| token_of(&text, now_ms()));
    }
    let body = token
        .and_then(|token| fetch(reader.client, &token, reader.timeout_s))
        .and_then(|body| json_tree::parse(&body).map_err(|_| UnmeasuredReason::BodyUnreadable));
    let rows = match body {
        Ok(tree) => windows_of(label, &tree),
        Err(reason) => vec![unmeasured(label, None, None, reason)],
    };
    (rows, refreshed)
}

/// その口座の設定 dir で claude を 1 回起こし、終わるか上限で止めるまで待つ（credential は書かない・
/// 書き手は Claude Code・ADR-0017 §2.5）。
///
/// 構築点は [`headless::build`] 1 つ。子は process group の leader として起こし（scope の包みの下では
/// claude は孫になる）、上限を超えたら group ごと止める。子の stdout / stderr は読まない・出さない。
fn refresh(dir: &Path, label: &str, reader: &Reader<'_>) -> Refresh {
    // path の形で名指した実行 file が無い周は起こさない（scope の包みの下では spawn が成功してしまい、
    // 不在が rc に化ける）。裸の名前の解決は子の起動側に任せる（env を読まない）。
    if reader.claude.contains('/') && !Path::new(reader.claude).is_file() {
        return Refresh::Unlaunchable;
    }
    let account = dir.join("accounts").join(label).display().to_string();
    let (mut command, confinement) = headless::build(&Call {
        claude: reader.claude,
        prompt: REFRESH_PROMPT,
        permission_mode: REFRESH_PERMISSION_MODE,
        // 計測の起動は model も effort も渡さない（argv は不変・rules 行 `runner.model` / `runner.effort` は
        // runner / lens の行）。
        model: None,
        effort: None,
        plugin_dir: None,
        account_dir: Some(&account),
        cwd: Some(dir),
        // 計測の起動は出力を読まない（text のまま＝argv は不変）。
        output: headless::Format::Text,
        max_turns: Some(REFRESH_MAX_TURNS),
    });
    command.stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
    let Ok(mut child) = command.spawn() else {
        return Refresh::Unlaunchable;
    };
    headless::feed(&mut child, REFRESH_PROMPT);
    let group = child.id();
    // 子の終わりは別 thread の `wait` で受ける（回収まで行うので、group の消滅の判定に zombie が残らない）。
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(child.wait());
    });
    let result = match receiver.recv_timeout(Duration::from_secs(reader.timeout_s)) {
        Ok(Ok(status)) => refresh_of(status),
        Ok(Err(_)) => Refresh::Unlaunchable,
        Err(_) => {
            stop_group(group, Duration::from_millis(reader.grace_ms));
            Refresh::Timeout
        }
    };
    let _ = confine::release_scope(&confinement);
    result
}

/// 終わった子の status を結果にする。signal で終わった周は 128 + signal 番号の rc と読む。
fn refresh_of(status: ExitStatus) -> Refresh {
    let code = status
        .code()
        .or_else(|| status.signal().map(|signal| RC_SIGNAL_BASE.saturating_add(signal)));
    match code {
        Some(0) => Refresh::Ok,
        Some(found) => Refresh::Rc(u8::try_from(found).unwrap_or(u8::MAX)),
        None => Refresh::Rc(u8::MAX),
    }
}

/// group 宛ての signal の宛先（`kill -- <宛先>` の引数・pure・in-file の歯の入口）。**group id が 2 未満の周は
/// `None`**（`kill -- -1` は user の全 process・`kill -- -0` は自分の group・N1: 実 signal の宛先は自分の子だけ）。
fn group_target(group: u32) -> Option<String> {
    if group < 2 {
        return None;
    }
    Some(format!("-{group}"))
}

/// group 宛てに TERM → 猶予だけ待つ → 残れば KILL → 同じ待ち（`pipe stop` と同じ列・待ちは
/// [`wait`] の 1 実装を通る・C3.4）。宛先は [`group_target`] で決め、`None` の周は撃たない。
/// 残った事実は返さない（結果は呼び手の `Timeout` のまま）。
fn stop_group(group: u32, grace: Duration) {
    let Some(target) = group_target(group) else {
        return;
    };
    signal_group(&target, "-TERM");
    if wait(Completion::GroupGone(group), grace).is_ok() {
        return;
    }
    signal_group(&target, "-KILL");
    let _ = wait(Completion::GroupGone(group), grace);
}

/// `kill <name> -- <target>` を撃つ（std に kill は無い・結果は待ちの側で測る）。
fn signal_group(target: &str, name: &str) {
    let _ = Command::new("kill")
        .arg(name)
        .arg("--")
        .arg(target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// 1 行を event にする（`run` / `bead` は持たない・actor は machine）。
fn event_of(ts: &str, host: &str, row: &Allowance) -> Event {
    let kind = match row {
        Allowance::Measured(_) => EventKind::AllowanceMeasured,
        Allowance::Unmeasured(_) => EventKind::AllowanceUnmeasured,
    };
    Event {
        schema: SCHEMA,
        ts: ts.to_owned(),
        kind,
        run: String::new(),
        bead: String::new(),
        host: host.to_owned(),
        actor: ACTOR_MACHINE.to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: None,
        allowance: Some(row.clone()),
        registration: None,
        mark: None,
        account: None,
        cost: None,
        rule: None,
    }
}

/// 口座 1 つ分の行を stdout の 1 行にする。並びは [`AllowanceKey`] の順（計測と `--show` で同じ）。
fn render(label: &str, rows: &[Allowance]) -> String {
    let account_level = rows.iter().find_map(|row| match row {
        Allowance::Unmeasured(found) if found.window.is_none() => Some(found.reason),
        Allowance::Measured(_) | Allowance::Unmeasured(_) => None,
    });
    if let Some(reason) = account_level {
        return format!("usage: account={label} unmeasured reason={}", reason.as_str());
    }
    let mut sorted = rows.to_vec();
    sorted.sort_by_key(Allowance::key);
    let mut line = format!("usage: account={label}");
    for row in &sorted {
        line.push(' ');
        line.push_str(&part(row));
    }
    line
}

/// 窓 1 つの表示。reset 無しは `resets=none` の字面（時刻の欄を空にしない・ADR-0024 §2.1）。
fn part(row: &Allowance) -> String {
    match row {
        Allowance::Measured(found) => {
            let resets = found.resets_at.as_deref().unwrap_or(RESETS_NONE);
            match found.window {
                WindowKind::FiveHour | WindowKind::SevenDay => {
                    format!("{}={}% resets={resets}", found.window.as_str(), found.used_pct)
                }
                WindowKind::SevenDayModel => format!(
                    "model={}:{}% resets={resets}",
                    found.model.as_deref().unwrap_or_default(),
                    found.used_pct
                ),
            }
        }
        Allowance::Unmeasured(found) => format!(
            "{}=unmeasured:{}",
            found.window.map_or("account", WindowKind::as_str),
            found.reason.as_str()
        ),
    }
}

#[cfg(test)]
use read::{body_of, client_args, config_of, normalize_resets};
#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.542
    // flip-check: moved s2-07l.544
    use super::{
        body_of, client_args, config_of, endpoint, grace_of, group_target, normalize_resets, render, table,
        table_row, token_of, windows_of, TableRow, UsageError, ROW_GRACE,
    };
    use crate::cli_outcome::{RC_BROKEN, RC_REFUSED};
    use crate::fleet::json_tree::parse;
    use crate::fleet::{
        Allowance, AllowanceLatest, Measured, Registration, RegistrationLatest, State, Unmeasured, UnmeasuredReason,
        WindowKind,
    };
    use crate::polarity::OnFailure;
    use crate::rules::manifest::Manifest;
    use crate::seat::role::Role;
    use crate::seat::{Provenance, StateDir};
    use std::path::PathBuf;

    /// 未来の epoch ms（2100-01-01）。
    const FUTURE_MS: u64 = 4_102_444_800_000;
    /// 「いま」として渡す epoch ms（2026-09-13）。
    const NOW_MS: u64 = 1_789_000_000_000;

    #[test]
    fn fleet_usage_resets_accepts_z_and_utc_offset_and_rejects_the_rest() {
        let want = Some("2026-09-12T05:00:00Z".to_owned());
        assert_eq!(normalize_resets("2026-09-12T05:00:00Z"), want, "Z");
        assert_eq!(normalize_resets("2026-09-12T05:00:00+00:00"), want, "+00:00");
        assert_eq!(normalize_resets("2026-09-12T05:00:00.918273+00:00"), want, "小数秒は切る");
        for bad in [
            "2026-09-12T05:00:00+09:00",
            "2026-09-12T05:00:00",
            "2026-09-12 05:00:00Z",
            "2026-13-12T05:00:00Z",
            "2026-09-12T24:00:00Z",
            "2026-09-12T05:00:00.Z",
            "tomorrow",
            "",
        ] {
            assert_eq!(normalize_resets(bad), None, "{bad:?} は受けない");
        }
    }

    #[test]
    fn fleet_usage_token_of_names_each_credential_failure() {
        let ok = format!(r#"{{"claudeAiOauth":{{"accessToken":"t-1","refreshToken":"r","expiresAt":{FUTURE_MS}}}}}"#);
        assert_eq!(token_of(&ok, NOW_MS), Ok("t-1".to_owned()));
        let cases = [
            (r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":0}}"#.to_owned(), UnmeasuredReason::Tombstone),
            (r#"{"claudeAiOauth":{"expiresAt":0}}"#.to_owned(), UnmeasuredReason::Tombstone),
            (r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":1000}}"#.to_owned(), UnmeasuredReason::TokenExpired),
            (format!(r#"{{"claudeAiOauth":{{"expiresAt":{FUTURE_MS}}}}}"#), UnmeasuredReason::NoToken),
            (format!(r#"{{"claudeAiOauth":{{"accessToken":"","expiresAt":{FUTURE_MS}}}}}"#), UnmeasuredReason::NoToken),
            (format!(r#"{{"claudeAiOauth":{{"accessToken":7,"expiresAt":{FUTURE_MS}}}}}"#), UnmeasuredReason::ShapeMismatch),
            (format!(r#"{{"claudeAiOauth":{{"accessToken":"a\nheader = x","expiresAt":{FUTURE_MS}}}}}"#), UnmeasuredReason::ShapeMismatch),
            (r#"{"claudeAiOauth":{"accessToken":"t"}}"#.to_owned(), UnmeasuredReason::ShapeMismatch),
            (r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":"soon"}}"#.to_owned(), UnmeasuredReason::ShapeMismatch),
            (r#"{"other":{}}"#.to_owned(), UnmeasuredReason::ShapeMismatch),
            ("{壊れ".to_owned(), UnmeasuredReason::ShapeMismatch),
        ];
        for (text, want) in cases {
            assert_eq!(token_of(&text, NOW_MS), Err(want), "{text}");
        }
    }

    #[test]
    fn fleet_usage_client_args_carry_timeout_and_never_the_token() {
        let args = client_args(17);
        let at = args.iter().position(|arg| arg == "--max-time").map(|at| at + 1);
        assert_eq!(at.and_then(|at| args.get(at)).map(String::as_str), Some("17"));
        assert!(args.windows(2).any(|pair| pair == ["-K", "-"]), "設定は stdin から: {args:?}");
        let config = config_of("se\"cr\\et");
        assert!(config.contains("header = \"Authorization: Bearer se\\\"cr\\\\et\"\n"), "{config}");
        assert!(config.contains("anthropic-beta: oauth-2025-04-20"), "{config}");
        assert!(config.contains("Accept: application/json"), "{config}");
        assert_eq!(endpoint(), "oauth-usage");
    }

    #[test]
    fn fleet_usage_body_of_splits_the_trailing_status_line() {
        assert_eq!(body_of("{}\n200"), Ok("{}"));
        assert_eq!(body_of("{\n}\n200"), Ok("{\n}"), "本文の改行は残す");
        assert_eq!(body_of("{}\n500"), Err(UnmeasuredReason::HttpStatus));
        assert_eq!(body_of("200"), Err(UnmeasuredReason::HttpStatus), "status 行が無い");
    }

    /// 窓 1 つだけを持つ本文（`five_hour` の `utilization` の字面を差し替える）。
    fn five_hour_line(utilization: &str) -> String {
        let body = format!(r#"{{"five_hour":{{"utilization":{utilization},"resets_at":"2026-09-12T05:00:00Z"}},"seven_day":{{"utilization":1.0,"resets_at":"2026-09-18T00:00:00Z"}},"limits":[]}}"#);
        let tree = parse(&body).expect("fixture は JSON");
        render("a1", &windows_of("a1", &tree))
    }

    #[test]
    fn fleet_usage_utilization_is_already_a_percent_floored_without_cap() {
        let tail = "seven_day=1% resets=2026-09-18T00:00:00Z";
        for (literal, want) in [("2.0", "2%"), ("0.0", "0%"), ("125.5", "125%"), ("13", "13%"), ("99.99", "99%")] {
            assert_eq!(
                five_hour_line(literal),
                format!("usage: account=a1 five_hour={want} resets=2026-09-12T05:00:00Z {tail}"),
                "{literal} は % の値"
            );
        }
        for bad in ["-1.0", "\"13\"", "null"] {
            assert_eq!(
                five_hour_line(bad),
                format!("usage: account=a1 five_hour=unmeasured:shape_mismatch {tail}"),
                "{bad} はその窓だけ shape_mismatch"
            );
        }
    }

    #[test]
    fn fleet_usage_idle_window_null_or_absent_reset_reads_as_measured_zero_without_reset() {
        let seven = r#""seven_day":{"utilization":1.0,"resets_at":"2026-09-18T00:00:00Z"},"limits":[]"#;
        let tail = "seven_day=1% resets=2026-09-18T00:00:00Z";
        for (five, name) in [
            (r#"{"utilization":0.0,"resets_at":null}"#, "null"),
            (r#"{"utilization":0.0}"#, "不在"),
        ] {
            let tree = parse(&format!(r#"{{"five_hour":{five},{seven}}}"#)).expect("fixture は JSON");
            let rows = windows_of("a1", &tree);
            assert!(
                rows.iter().any(|row| matches!(row, Allowance::Measured(found)
                    if found.window == WindowKind::FiveHour && found.used_pct == 0 && found.resets_at.is_none())),
                "{name}: 測れた 0%・reset 無し: {rows:?}"
            );
            assert_eq!(render("a1", &rows), format!("usage: account=a1 five_hour=0% resets=none {tail}"), "{name}");
        }
        let busy = parse(&format!(r#"{{"five_hour":{{"utilization":3.0,"resets_at":null}},{seven}}}"#)).expect("fixture は JSON");
        assert_eq!(
            render("a1", &windows_of("a1", &busy)),
            format!("usage: account=a1 five_hour=unmeasured:shape_mismatch {tail}"),
            "0 以外を reset 無しで記録しない"
        );
        let bad = parse(&format!(r#"{{"five_hour":{{"utilization":0.0,"resets_at":5}},{seven}}}"#)).expect("fixture は JSON");
        assert_eq!(
            render("a1", &windows_of("a1", &bad)),
            format!("usage: account=a1 five_hour=unmeasured:shape_mismatch {tail}"),
            "null でも文字列でもない reset は形が違う"
        );
        let element = parse(
            r#"{"five_hour":{"utilization":1.0,"resets_at":"2026-09-12T05:00:00Z"},"seven_day":{"utilization":1.0,"resets_at":"2026-09-18T00:00:00Z"},"limits":[{"kind":"weekly_scoped","percent":0,"resets_at":null,"scope":{"model":{"display_name":"Fable"}}}]}"#,
        )
        .expect("fixture は JSON");
        let rows = windows_of("a1", &element);
        assert!(
            rows.iter().any(|row| matches!(row, Allowance::Unmeasured(found) if found.window == Some(WindowKind::SevenDayModel))),
            "limits[] の要素には掛けない: {rows:?}"
        );
    }

    #[test]
    fn fleet_usage_windows_of_maps_windows_and_isolates_the_broken_element() {
        let body = r#"{
          "five_hour": {"utilization": 13.0, "resets_at": "2026-09-12T05:00:00.918273+00:00"},
          "seven_day": {"utilization": 41.7, "resets_at": "2026-09-18T00:00:00Z"},
          "limits": [
            {"kind": "weekly_scoped", "group": "g", "percent": 38, "severity": "normal", "resets_at": "2026-09-18T00:00:00+00:00", "scope": {"model": {"id": null, "display_name": "Fable"}}, "is_active": true},
            {"kind": "weekly_scoped", "percent": 10, "resets_at": "2026-09-18T00:00:00Z", "scope": {"model": {}}},
            {"kind": "weekly_scoped", "utilization": 55.0, "resets_at": "2026-09-18T00:00:00Z", "scope": {"model": {"display_name": "Nope"}}},
            {"kind": "weekly", "percent": 50, "resets_at": "2026-09-18T00:00:00Z"}
          ]
        }"#;
        let tree = parse(body).expect("fixture は JSON");
        let rows = windows_of("a1", &tree);
        assert_eq!(rows.len(), 5, "five + seven + model 3 要素（weekly は拾わない）: {rows:?}");
        assert_eq!(
            render("a1", &rows),
            "usage: account=a1 five_hour=13% resets=2026-09-12T05:00:00Z seven_day=41% resets=2026-09-18T00:00:00Z seven_day_model=unmeasured:shape_mismatch model=Fable:38% resets=2026-09-18T00:00:00Z seven_day_model=unmeasured:shape_mismatch"
        );
        assert!(
            rows.iter().any(|row| matches!(row, Allowance::Unmeasured(found) if found.model.as_deref() == Some("Nope"))),
            "percent の無い要素は utilization が在っても読まない: {rows:?}"
        );
        let empty = parse(r#"{"five_hour":{"utilization":0,"resets_at":"2026-09-12T05:00:00Z"},"seven_day":{"utilization":"x","resets_at":"2026-09-18T00:00:00Z"},"limits":[]}"#)
            .expect("fixture は JSON");
        assert_eq!(
            render("a2", &windows_of("a2", &empty)),
            "usage: account=a2 five_hour=0% resets=2026-09-12T05:00:00Z seven_day=unmeasured:shape_mismatch",
            "要素 0 件なら model 行なし・型違いの窓だけ unmeasured"
        );
        let array = parse("[]").expect("fixture は JSON");
        assert_eq!(render("a3", &windows_of("a3", &array)), "usage: account=a3 unmeasured reason=shape_mismatch");
        let no_limits = parse(r#"{"five_hour":{"utilization":0,"resets_at":"2026-09-12T05:00:00Z"}}"#)
            .expect("fixture は JSON");
        let rows = windows_of("a4", &no_limits);
        assert!(
            rows.iter().any(|row| matches!(row, Allowance::Unmeasured(found) if found.window == Some(WindowKind::SevenDayModel))),
            "limits 欠落を黙って落とさない: {rows:?}"
        );
    }

    #[test]
    fn fleet_usage_error_polarity_is_fail_closed_and_rc_splits_refused_from_broken() {
        assert_eq!(UsageError::POLARITY.on_failure, OnFailure::FailClosed);
        assert_eq!(UsageError::Args(String::new()).rc(), RC_REFUSED);
        assert_eq!(UsageError::Manifest(String::new()).rc(), RC_REFUSED);
        assert_eq!(UsageError::Store(String::new()).rc(), RC_BROKEN);
    }

    /// (a) 停止の宛先: group id 0 / 1 は撃たない（`None`）・2 以上は `-<group>`（`s2-07l.255`・N1）。
    /// 境界を `<= 2` に動かす変異は `(2)` で、guard を外す変異は `(0)` / `(1)` で落ちる。
    #[test]
    fn usage_stop_group_target_refuses_below_two_and_names_the_group() {
        assert_eq!(group_target(0), None, "0 は自分の group");
        assert_eq!(group_target(1), None, "1 は user の全 process");
        assert_eq!(group_target(2), Some("-2".to_owned()), "2 は撃てる最小の id");
        assert_eq!(group_target(4242), Some("-4242".to_owned()));
    }

    /// `pipe.stop_grace_ms` を 1 行だけ持つ manifest の fixture。
    fn grace_manifest(value: u64) -> Manifest {
        let text = format!(
            "schema = 1\n\n[[rule]]\nid = \"{ROW_GRACE}\"\nkind = \"StopGraceMs\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n"
        );
        match Manifest::parse(&text) {
            Ok(found) => found,
            Err(errors) => panic!("fixture の manifest を読める: {errors:?}"),
        }
    }

    /// (b) 猶予は渡された manifest の `pipe.stop_grace_ms` の値（250 → 250・750 → 750＝定数に潰す変異が落ちる）。
    /// 行の無い manifest は埋め込みの行の値へ倒れる（0 に潰れない）。
    #[test]
    fn usage_stop_grace_of_reads_the_manifest_row() {
        assert_eq!(grace_of(&grace_manifest(250)), 250);
        assert_eq!(grace_of(&grace_manifest(750)), 750);
        let absent = match Manifest::parse("schema = 1\n") {
            Ok(found) => found,
            Err(errors) => panic!("空の manifest を読める: {errors:?}"),
        };
        let embedded = crate::seat::int_rule(ROW_GRACE).unwrap_or(0);
        assert!(embedded > 0, "埋め込みの行は正の猶予を持つ");
        assert_eq!(grace_of(&absent), embedded, "行の無い manifest は埋め込みの値");
    }

    /// 表の歯の置き場（出所 = git 設定・path は `/s`）。
    fn table_place() -> StateDir {
        StateDir { path: PathBuf::from("/s"), source: Provenance::GitConfig }
    }

    /// 行を `ts` で積む（鍵は口座 × 窓 × model・後から積んだ行が勝つ）。
    fn put(state: &mut State, ts: &str, row: Allowance) {
        state.allowance.insert(row.key(), AllowanceLatest { ts: ts.to_owned(), allowance: row });
    }

    /// 実測の行。
    fn measured(account: &str, window: WindowKind, model: Option<&str>, used_pct: u64, resets_at: Option<&str>) -> Allowance {
        Allowance::Measured(Measured {
            account: account.to_owned(),
            window,
            model: model.map(str::to_owned),
            endpoint: endpoint(),
            used_pct,
            resets_at: resets_at.map(str::to_owned),
        })
    }

    /// 測れなかった行（[`super::unmeasured`] と同じ形・model 無し）。
    fn failed(account: &str, window: Option<WindowKind>, reason: UnmeasuredReason) -> Allowance {
        Allowance::Unmeasured(Unmeasured {
            account: account.to_owned(),
            window,
            model: None,
            endpoint: endpoint(),
            reason,
        })
    }

    /// 登録 row を 1 件積む（口座 = `account`・anchor が鍵）。
    fn put_registration(state: &mut State, anchor: &str, account: &str) {
        let registration = Registration {
            role: Role::Orchestrator,
            anchor: anchor.to_owned(),
            target: "s:w".to_owned(),
            sid: None,
            account: account.to_owned(),
            launch: String::new(),
            model: None,
        };
        let seq = state.registrations.len();
        state.registrations.insert((Role::Orchestrator, anchor.to_owned()), RegistrationLatest { seq, registration });
    }

    /// 列の始まり（見出し行の各列名の byte offset）。
    fn column_starts(head: &str) -> Vec<usize> {
        ["account", "5h", "7d", "model", "seat", "resets"].iter().filter_map(|name| head.find(name)).collect()
    }

    /// 約束 4（pure・Unmeasured 混在）: 窓の Unmeasured は `unmeasured:<reason>`・口座単位の Unmeasured は 5h / 7d の両欄・
    /// model 窓は `名:%` を `,` で並べ、無ければ `-`・reset 無しの 5h は `none`・測っていない口座は全欄 `-`・seat 列は
    /// 登録 row の口座だけ役割名（2 row でも 1 語）。値は 1 行形と同じ「最新の回」（古い ts の行は表に出ない）。
    #[test]
    fn fleet_usage_table_rows_name_unmeasured_windows_and_seat_roles() {
        let mut state = State::default();
        let week = Some("2026-09-27T00:00:00Z");
        put(&mut state, "t2", measured("a1", WindowKind::FiveHour, None, 13, Some("2026-09-21T05:00:00Z")));
        put(&mut state, "t2", measured("a1", WindowKind::SevenDay, None, 41, week));
        put(&mut state, "t2", measured("a1", WindowKind::SevenDayModel, Some("Fable"), 38, week));
        put(&mut state, "t2", measured("a1", WindowKind::SevenDayModel, Some("Opus"), 7, week));
        put(&mut state, "t2", failed("a2", Some(WindowKind::FiveHour), UnmeasuredReason::Timeout));
        put(&mut state, "t2", measured("a2", WindowKind::SevenDay, None, 0, None));
        put(&mut state, "t1", measured("a2", WindowKind::SevenDayModel, Some("Fable"), 99, week));
        put(&mut state, "t2", failed("a3", None, UnmeasuredReason::NoCredentials));
        put_registration(&mut state, "/repo-1", "a1");
        put_registration(&mut state, "/repo-2", "a1");
        put_registration(&mut state, "/repo-3", "a3");
        let rows: Vec<TableRow> = ["a1", "a2", "a3", "a4"].iter().map(|label| table_row(label, &state)).collect();
        let want = |account: &str, five: &str, seven: &str, model: &str, seat: &str, resets: &str| TableRow {
            account: account.to_owned(),
            five_hour: five.to_owned(),
            seven_day: seven.to_owned(),
            model: model.to_owned(),
            seat: seat.to_owned(),
            resets: resets.to_owned(),
        };
        assert_eq!(
            rows,
            vec![
                want("a1", "13%", "41%", "Fable:38%,Opus:7%", "orchestrator", "2026-09-21T05:00:00Z"),
                want("a2", "unmeasured:timeout", "0%", "-", "-", "-"),
                want("a3", "unmeasured:no_credentials", "unmeasured:no_credentials", "-", "orchestrator", "-"),
                want("a4", "-", "-", "-", "-", "-"),
            ]
        );
        put(&mut state, "t3", measured("a2", WindowKind::FiveHour, None, 0, None));
        let idle = table_row("a2", &state);
        assert_eq!((idle.five_hour.as_str(), idle.resets.as_str()), ("0%", "none"), "reset 無しの 5h は none・古い回の 7d は出ない");
        assert_eq!(idle.seven_day, "-", "最新の回に無い窓は -");
    }

    /// 約束 4（pure・列幅・口座 0 件）: 1 行目は `StateDir::suffix` から先頭の空白を落とした字面（出所が先・path が行末）、
    /// 2 行目は見出し。列幅は見出しと値の最大幅で揃い（長い label / 長い欄が列を押し広げる）、末尾の列は詰めない。
    /// 口座 0 件は見出し 2 行だけで、列幅は見出しの幅。
    #[test]
    fn fleet_usage_table_aligns_columns_by_the_widest_value_and_prints_headers_alone_for_no_accounts() {
        let place = table_place();
        let empty = table(&place, &[]);
        assert_eq!(empty, vec!["source=git-config state_dir=/s".to_owned(), "account  5h  7d  model  seat  resets".to_owned()]);
        assert_eq!(format!(" {}", empty.first().map(String::as_str).unwrap_or_default()), place.suffix(), "1 行目は suffix そのもの");

        let rows = [
            TableRow {
                account: "a-much-longer-label".to_owned(),
                five_hour: "unmeasured:shape_mismatch".to_owned(),
                seven_day: "0%".to_owned(),
                model: "-".to_owned(),
                seat: "-".to_owned(),
                resets: "none".to_owned(),
            },
            TableRow {
                account: "b".to_owned(),
                five_hour: "5%".to_owned(),
                seven_day: "100%".to_owned(),
                model: "Fable:38%".to_owned(),
                seat: "orchestrator".to_owned(),
                resets: "2026-09-21T05:00:00Z".to_owned(),
            },
        ];
        let lines = table(&place, &rows);
        assert_eq!(lines.len(), 4, "{lines:?}");
        let head = lines.get(1).map(String::as_str).unwrap_or_default();
        let starts = column_starts(head);
        assert_eq!(starts.len(), 6, "見出しの 6 列: {head:?}");
        for line in lines.iter().skip(1) {
            assert!(!line.ends_with(' '), "行末に空白を残さない: {line:?}");
            let cells: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(cells.len(), 6, "{line:?}");
            for (cell, start) in cells.iter().zip(&starts) {
                assert!(line.get(*start..).is_some_and(|tail| tail.starts_with(cell)), "列 {cell:?} が {start} から始まる: {line:?}");
            }
        }
        let first = starts.get(1).copied().unwrap_or(0);
        assert_eq!(first, "a-much-longer-label".len() + 2, "account 列の幅は最長の label");
        let seven = starts.get(2).copied().unwrap_or(0);
        assert_eq!(seven - first, "unmeasured:shape_mismatch".len() + 2, "5h 列の幅は最長の欄");
    }
}
