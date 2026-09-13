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

use super::cli::{host, now_utc, optional};
use super::json_tree::{self, Tree};
use super::store::{self, LockPolicy};
use super::{
    replay, Allowance, AllowanceKey, AllowanceLatest, Event, EventKind, Measured, Unmeasured,
    UnmeasuredReason, WindowKind, ACTOR_MACHINE, SCHEMA,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// 残量を聞く先（host 固有の値ではない・manifest に置かない・設計 §3）。
pub const URL: &str = "https://api.anthropic.com/api/oauth/usage";

/// endpoint が要する beta header の値。
pub const BETA: &str = "oauth-2025-04-20";

/// `--curl` を渡さない周の client（PATH 解決は子 process の起動側）。
const DEFAULT_CLIENT: &str = "curl";

/// 待ち時間の上限を持つ rules 行。
const ROW_TIMEOUT: &str = "fleet.usage_timeout_s";

/// curl が `--max-time` を超えたときの rc。
const RC_CLIENT_TIMEOUT: i32 = 28;

/// モデル別 7 日窓を表す `limits[]` 要素の `kind`。
const SCOPED_KIND: &str = "weekly_scoped";

/// credential の置き場（`<state_dir>/accounts/<label>/` の下）の file 名。
const CREDENTIAL_FILE: &str = ".credentials.json";

/// `fleet usage` を止める誤り（設計 §6）。極性は fail-closed（[`Self::POLARITY`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    /// 引数の誤り（値欠けの flag）。
    Args(String),
    /// manifest が読めない・rules 行が引けない。
    Manifest(String),
    /// `[[account]]` が 1 行も無い。
    NoAccounts,
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
            Self::Args(_) | Self::Manifest(_) | Self::NoAccounts => RC_REFUSED,
            Self::Store(_) => RC_BROKEN,
        }
    }
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Args(reason) => write!(f, "fleet usage: {reason}"),
            Self::Manifest(reason) => write!(f, "fleet usage: manifest を読めない（{reason}）"),
            Self::NoAccounts => write!(f, "fleet usage: [[account]] が 1 行も無い"),
            Self::Store(reason) => write!(f, "{reason}"),
        }
    }
}

/// `fleet usage` の入口。`--show` が在れば read-only の表示だけを行う。
pub fn run(args: &[String], dir: &Path) -> Outcome {
    let result = if args.iter().any(|arg| arg == "--show") {
        show(args, dir)
    } else {
        measure(args, dir)
    };
    result.unwrap_or_else(|error| Outcome::failed(error.rc(), vec![error.to_string()]))
}

/// event の `endpoint` に書く短い識別子。[`URL`] の末尾 2 段を `-` で繋いで導く（`oauth-usage`）。
pub fn endpoint() -> String {
    let mut segments: Vec<&str> = URL.rsplit('/').take(2).collect();
    segments.reverse();
    segments.join("-")
}

/// 口座を 1 つずつ読み、窓ごとに追記し、口座ごとに 1 行を返す。
fn measure(args: &[String], dir: &Path) -> Result<Outcome, UsageError> {
    let client = optional(args, "--curl")
        .map_err(UsageError::Args)?
        .unwrap_or(DEFAULT_CLIENT)
        .to_owned();
    let (manifest, labels) = accounts(args)?;
    let timeout_s = timeout_of(&manifest)?;
    let policy = LockPolicy::embedded().map_err(|err| UsageError::Store(err.to_string()))?;
    let host = host();
    let mut outcome = Outcome::ok(Vec::new());
    for label in &labels {
        let rows = read_account(dir, label, &client, timeout_s);
        let ts = now_utc();
        for row in &rows {
            let warnings = store::append(dir, &event_of(&ts, &host, row), policy)
                .map_err(|err| UsageError::Store(err.to_string()))?;
            outcome.err.extend(warnings.iter().map(|w| w.as_str().to_owned()));
        }
        outcome.out.push(render(label, &rows));
    }
    Ok(outcome)
}

/// replay の `allowance` から、口座ごとに最新の 1 回分を同じ 1 行形で出す（lock を取らない）。
fn show(args: &[String], dir: &Path) -> Result<Outcome, UsageError> {
    let (_, labels) = accounts(args)?;
    let events = store::read_all(dir).map_err(|errors| UsageError::Store(joined(&errors)))?;
    let state = replay(&events);
    let lines = labels
        .iter()
        .filter_map(|label| latest_line(label, &state.allowance))
        .collect();
    Ok(Outcome::ok(lines))
}

/// 口座 1 つの最新の回（`ts` が最大の行の集まり）を 1 行にする。行が無ければ `None`。
fn latest_line(label: &str, allowance: &BTreeMap<AllowanceKey, AllowanceLatest>) -> Option<String> {
    let mine: Vec<&AllowanceLatest> = allowance
        .iter()
        .filter(|(key, _)| key.account == label)
        .map(|(_, latest)| latest)
        .collect();
    let newest = mine.iter().map(|latest| latest.ts.as_str()).max()?;
    let rows: Vec<Allowance> = mine
        .iter()
        .filter(|latest| latest.ts == newest)
        .map(|latest| latest.allowance.clone())
        .collect();
    Some(render(label, &rows))
}

/// manifest を読み、宣言した口座の label を宣言順で返す。0 行なら断る。
fn accounts(args: &[String]) -> Result<(Manifest, Vec<String>), UsageError> {
    let loaded = match optional(args, "--rules").map_err(UsageError::Args)? {
        Some(path) => Manifest::load(Path::new(path)),
        None => Manifest::embedded(),
    };
    let manifest = loaded.map_err(|errors| UsageError::Manifest(joined(&errors)))?;
    let labels: Vec<String> = manifest
        .accounts()
        .iter()
        .map(|account| account.label().to_owned())
        .collect();
    if labels.is_empty() {
        return Err(UsageError::NoAccounts);
    }
    Ok((manifest, labels))
}

/// rules 行 `fleet.usage_timeout_s` の秒。無い・不発効・型違いは断る。
fn timeout_of(manifest: &Manifest) -> Result<u64, UsageError> {
    let row = manifest
        .get(ROW_TIMEOUT)
        .ok_or_else(|| UsageError::Manifest(format!("{ROW_TIMEOUT} が無い")))?;
    if !row.enabled {
        return Err(UsageError::Manifest(format!("{ROW_TIMEOUT} は不発効である")));
    }
    match row.value {
        RuleValue::Int(found) => Ok(found),
        _ => Err(UsageError::Manifest(format!("{ROW_TIMEOUT} が整数でない"))),
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
fn read_account(dir: &Path, label: &str, client: &str, timeout_s: u64) -> Vec<Allowance> {
    let body = read_credential(&credential_path(dir, label))
        .and_then(|text| token_of(&text, now_ms()))
        .and_then(|token| fetch(client, &token, timeout_s))
        .and_then(|body| json_tree::parse(&body).map_err(|_| UnmeasuredReason::BodyUnreadable));
    match body {
        Ok(tree) => windows_of(label, &tree),
        Err(reason) => vec![unmeasured(label, None, None, reason)],
    }
}

/// credential の path。**`<state_dir>/accounts/` を走査しない**（label から 1 本に決まる）。
fn credential_path(dir: &Path, label: &str) -> PathBuf {
    dir.join("accounts").join(label).join(CREDENTIAL_FILE)
}

/// credential の本文を読む。不在は `NoCredentials`・読めない形は `ShapeMismatch`。
fn read_credential(path: &Path) -> Result<String, UnmeasuredReason> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| UnmeasuredReason::ShapeMismatch),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err(UnmeasuredReason::NoCredentials)
        }
        Err(_) => Err(UnmeasuredReason::ShapeMismatch),
    }
}

/// いまの UNIX ミリ秒。
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// credential の本文から token を取る（`claudeAiOauth.accessToken` と `expiresAt` だけを読む）。
///
/// 墓標（`expiresAt == 0`）は token の有無より先に見る——使わない印の置き場に token が
/// 無いのは当然で、`NoToken` と読むと「置き忘れ」に化ける。
fn token_of(text: &str, now_ms: u64) -> Result<String, UnmeasuredReason> {
    let tree = json_tree::parse(text).map_err(|_| UnmeasuredReason::ShapeMismatch)?;
    let oauth = tree
        .get("claudeAiOauth")
        .filter(|found| matches!(found, Tree::Object(_)))
        .ok_or(UnmeasuredReason::ShapeMismatch)?;
    let expires = oauth.get("expiresAt").map(epoch_ms);
    if expires == Some(Some(0)) {
        return Err(UnmeasuredReason::Tombstone);
    }
    let token = match oauth.get("accessToken") {
        None | Some(Tree::Null) => return Err(UnmeasuredReason::NoToken),
        Some(found) => found.as_str().ok_or(UnmeasuredReason::ShapeMismatch)?,
    };
    if token.is_empty() {
        return Err(UnmeasuredReason::NoToken);
    }
    // 制御文字（改行）入りの token は設定行を割って別の行を注入できる形なので読まない。
    if token.chars().any(char::is_control) {
        return Err(UnmeasuredReason::ShapeMismatch);
    }
    if expires.flatten().ok_or(UnmeasuredReason::ShapeMismatch)? < now_ms {
        return Err(UnmeasuredReason::TokenExpired);
    }
    Ok(token.to_owned())
}

/// epoch ms の整数。数でない・整数でない値は `None`。
fn epoch_ms(tree: &Tree) -> Option<u64> {
    match tree {
        Tree::Num(text) => text.parse().ok(),
        _ => None,
    }
}

/// client の argv。**token を載せない**（設定は stdin から `-K -` で読ませる）。
fn client_args(timeout_s: u64) -> Vec<String> {
    [
        "-sS",
        "-K",
        "-",
        "--max-time",
        &timeout_s.to_string(),
        "-o",
        "-",
        "-w",
        "\n%{http_code}",
        URL,
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect()
}

/// stdin へ書く curl の設定行。値は二重引用で包み、`\` と `"` を escape する。
fn config_of(token: &str) -> String {
    let quoted = token.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "header = \"Authorization: Bearer {quoted}\"\n\
         header = \"anthropic-beta: {BETA}\"\n\
         header = \"Accept: application/json\"\n"
    )
}

/// client を起こして本文を受け取る。stderr は捨てる（中継しない）。
fn fetch(client: &str, token: &str, timeout_s: u64) -> Result<String, UnmeasuredReason> {
    let mut child = Command::new(client)
        .args(client_args(timeout_s))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| UnmeasuredReason::ClientMissing)?;
    if let Some(mut stdin) = child.stdin.take() {
        // 書けない周（client が先に終わった）は応答の側で理由が付く。
        let _ = stdin.write_all(config_of(token).as_bytes());
    }
    let output = child
        .wait_with_output()
        .map_err(|_| UnmeasuredReason::ClientFailed)?;
    match output.status.code() {
        Some(0) => {}
        Some(RC_CLIENT_TIMEOUT) => return Err(UnmeasuredReason::Timeout),
        _ => return Err(UnmeasuredReason::ClientFailed),
    }
    let text = String::from_utf8(output.stdout).map_err(|_| UnmeasuredReason::BodyUnreadable)?;
    body_of(&text).map(str::to_owned)
}

/// stdout の末尾行を HTTP status として外し、本文を返す。200 以外は `HttpStatus`。
fn body_of(stdout: &str) -> Result<&str, UnmeasuredReason> {
    match stdout.rsplit_once('\n') {
        Some((body, status)) if status.trim() == "200" => Ok(body),
        _ => Err(UnmeasuredReason::HttpStatus),
    }
}

/// 応答の木を窓の行へ写す。根が object でなければ口座単位の `ShapeMismatch`。
fn windows_of(label: &str, root: &Tree) -> Vec<Allowance> {
    if !matches!(root, Tree::Object(_)) {
        return vec![unmeasured(label, None, None, UnmeasuredReason::ShapeMismatch)];
    }
    let mut rows = vec![
        window_row(label, WindowKind::FiveHour, None, root.get("five_hour")),
        window_row(label, WindowKind::SevenDay, None, root.get("seven_day")),
    ];
    rows.extend(model_rows(label, root.get("limits")));
    rows
}

/// `limits[]` の `weekly_scoped` 要素をモデル別の行へ。要素 0 件なら行なし。
///
/// `limits` そのものが無い・配列でない周は窓 1 つの `ShapeMismatch`（黙って「モデル行なし」に
/// 読み替えない）。`display_name` の無い要素は**その要素だけ** Unmeasured になる。
fn model_rows(label: &str, limits: Option<&Tree>) -> Vec<Allowance> {
    let model_window = Some(WindowKind::SevenDayModel);
    let Some(items) = limits.and_then(Tree::as_array) else {
        return vec![unmeasured(label, model_window, None, UnmeasuredReason::ShapeMismatch)];
    };
    items
        .iter()
        .filter(|item| item.get("kind").and_then(Tree::as_str) == Some(SCOPED_KIND))
        .map(|item| {
            let name = item
                .get("scope")
                .and_then(|scope| scope.get("model"))
                .and_then(|model| model.get("display_name"))
                .and_then(Tree::as_str);
            match name {
                None => unmeasured(label, model_window, None, UnmeasuredReason::ShapeMismatch),
                Some(name) => {
                    window_row(label, WindowKind::SevenDayModel, Some(name.to_owned()), Some(item))
                }
            }
        })
        .collect()
}

/// 窓の値を持つ field。`five_hour` / `seven_day` は `utilization`、`limits[]` の要素は `percent`
/// （要素は `utilization` を持たない実測・持っていても読まない）。
fn value_key(window: WindowKind) -> &'static str {
    match window {
        WindowKind::FiveHour | WindowKind::SevenDay => "utilization",
        WindowKind::SevenDayModel => "percent",
    }
}

/// 窓 1 つ（値の field と `resets_at` を持つ object）を行にする。
fn window_row(label: &str, window: WindowKind, model: Option<String>, node: Option<&Tree>) -> Allowance {
    match node.and_then(|node| reading(node, value_key(window))) {
        Some((used_pct, resets_at)) => Allowance::Measured(Measured {
            account: label.to_owned(),
            window,
            model,
            endpoint: endpoint(),
            used_pct,
            resets_at,
        }),
        None => unmeasured(label, Some(window), model, UnmeasuredReason::ShapeMismatch),
    }
}

/// 窓の object から（整数 %・正規化した reset）を読む。どちらかが読めなければ `None`。
fn reading(node: &Tree, key: &str) -> Option<(u64, String)> {
    let used_pct = whole_pct(node.get(key)?)?;
    let resets_at = normalize_resets(node.get("resets_at")?.as_str()?)?;
    Some((used_pct, resets_at))
}

/// **すでに % の値**（`2.0` = 2%）を整数 % へ切り捨てる（cap しない）。負数と数でない値は `None`。
///
/// [`Tree::as_pct`] は割合を ×100 して切り捨てる。非負の x で `floor(100x) / 100 == floor(x)`
/// なので、その値を 100 で割れば桁の読みを json_tree と共有したまま % の値を読める
/// （×100 が `u64` を超える巨大な値は `None`＝ShapeMismatch）。
fn whole_pct(value: &Tree) -> Option<u64> {
    value.as_pct().map(|hundredths| hundredths / 100)
}

/// Unmeasured の行を組む。
fn unmeasured(
    label: &str,
    window: Option<WindowKind>,
    model: Option<String>,
    reason: UnmeasuredReason,
) -> Allowance {
    Allowance::Unmeasured(Unmeasured {
        account: label.to_owned(),
        window,
        model,
        endpoint: endpoint(),
        reason,
    })
}

/// `resets_at` を UTC の `YYYY-MM-DDTHH:MM:SSZ` にする。`Z` と `+00:00` の両形・小数秒を受理する。
///
/// UTC 以外の offset は受けない（読み替えの規則を持たない・parse 不能と同じ扱い）。
fn normalize_resets(text: &str) -> Option<String> {
    let stamp = text
        .strip_suffix('Z')
        .or_else(|| text.strip_suffix("+00:00"))?;
    let stamp = match stamp.split_once('.') {
        None => stamp,
        Some((head, frac)) if !frac.is_empty() && frac.bytes().all(|b| b.is_ascii_digit()) => head,
        Some(_) => return None,
    };
    let shape = b"0000-00-00T00:00:00";
    let bytes = stamp.as_bytes();
    let fits = bytes.len() == shape.len()
        && bytes.iter().zip(shape.iter()).all(|(found, want)| match want {
            b'0' => found.is_ascii_digit(),
            _ => found == want,
        });
    if !fits {
        return None;
    }
    let in_range = |from: usize, low: u32, high: u32| {
        stamp
            .get(from..from + 2)
            .and_then(|digits| digits.parse::<u32>().ok())
            .is_some_and(|value| (low..=high).contains(&value))
    };
    let valid = in_range(5, 1, 12) && in_range(8, 1, 31) && in_range(11, 0, 23) && in_range(14, 0, 59) && in_range(17, 0, 59);
    valid.then(|| format!("{stamp}Z"))
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

/// 窓 1 つの表示。
fn part(row: &Allowance) -> String {
    match row {
        Allowance::Measured(found) => match found.window {
            WindowKind::FiveHour | WindowKind::SevenDay => format!(
                "{}={}% resets={}",
                found.window.as_str(),
                found.used_pct,
                found.resets_at
            ),
            WindowKind::SevenDayModel => format!(
                "model={}:{}% resets={}",
                found.model.as_deref().unwrap_or_default(),
                found.used_pct,
                found.resets_at
            ),
        },
        Allowance::Unmeasured(found) => format!(
            "{}=unmeasured:{}",
            found.window.map_or("account", WindowKind::as_str),
            found.reason.as_str()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        body_of, client_args, config_of, endpoint, normalize_resets, render, token_of, windows_of,
        UsageError,
    };
    use crate::cli_outcome::{RC_BROKEN, RC_REFUSED};
    use crate::fleet::json_tree::parse;
    use crate::fleet::{Allowance, UnmeasuredReason, WindowKind};
    use crate::polarity::OnFailure;

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
        assert_eq!(UsageError::NoAccounts.rc(), RC_REFUSED);
        assert_eq!(UsageError::Store(String::new()).rc(), RC_BROKEN);
    }
}
