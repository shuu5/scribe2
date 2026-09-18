//! 台帳の読み（`bd --readonly list --json` の子 process・席の指示文の `{ledger}` が読む）。
//!
//! `seat/rebrief.rs` から**挙動不変で移した**もの（`s2-07l.479.2`）: 作業記憶の復元は ADR-0045 §2 (2) で
//! 消えたが、SessionStart の指示文は台帳の現在値を 1 行で持つ（同 §2 (3)）ので、その読みだけを残す。
//! 待ち上限は rules 行 [`ID_TIMEOUT`] から読み、**読めない周は `None`**（数え損ねを 0 に化けさせない・C10）。
// flip-check: moved s2-07l.479.2

use crate::fleet::json_tree::{self, Tree};
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// この境界の極性（[`LedgerError`]）: 読めない周は数えを 1 つも返さない（呼び側が `unknown` を書く）。
/// 行為を止める判定ではないので Guard ではない（極性一覧に載せない・設計 polarity.md §3）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 台帳を読めない理由（**境界の enum**・[`POLARITY`]）。読めない周は数えを返さない
/// （`LedgerError::Unreadable` の 1 つだけが残った・`s2-07l.479.2`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerError {
    /// 台帳を読めない（起動できない・rc 非 0・JSON 不能・待ち上限超過）。
    Unreadable,
}

/// 台帳の待ち上限を宣言する rules 行の id（**値は code に焼かない**・憲法 C5）。
pub const ID_TIMEOUT: &str = "seat.ledger_timeout_s";

/// `--bd` を渡さない周の台帳 client（PATH 解決は子 process の起動側）。
pub const DEFAULT_BD: &str = "bd";

/// 台帳を読む引数（`--readonly` を必ず付ける）。`--all` は closed を含む一覧。呼出しは 1 回である
/// （待ち上限 [`ID_TIMEOUT`]）。
const BD_ARGS: [&str; 6] = ["--readonly", "list", "--all", "--limit", "0", "--json"];

/// `updated_at` が無い行の字面。
const NONE: &str = "none";

/// 子 process の終了を見に行く刻み。
const POLL: Duration = Duration::from_millis(10);

/// 待ち上限を**渡された manifest** から読む。不発効・別の形・不在は `None`（呼び側は `no-rule`）。
pub fn timeout_of(manifest: &Manifest) -> Option<Duration> {
    let row = manifest.get(ID_TIMEOUT)?;
    match (row.enabled, &row.value) {
        (true, RuleValue::Int(found)) => Some(Duration::from_secs(*found)),
        _ => None,
    }
}

/// 台帳の 1 件（読む key だけ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// bead id。
    pub id: String,
    /// status の字面。
    pub status: String,
    /// title（無ければ空）。
    pub title: String,
    /// `updated_at`（無ければ `none`）。
    pub updated: String,
    /// priority field（無い・非負の整数でなければ `None`）。
    pub priority: Option<u64>,
    /// label の列（無ければ空）。
    pub labels: Vec<String>,
    /// 依存の列（`dependencies[]`・3 key の揃う要素だけ）。
    pub deps: Vec<Dep>,
}

/// 依存の 1 件（`dependencies[]` の `id` / `status` / `dependency_type` だけを読む・本文は読まない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    /// 依存先の bead id。
    pub id: String,
    /// 依存先の status の字面。
    pub status: String,
    /// 依存の種別の字面（`blocks` / `parent-child` …）。
    pub kind: String,
}

/// 台帳の JSON（`bd list --json` の配列）を読む。配列でない・要素に `id` / `status` の文字列が無い → `None`。
/// priority / labels / dependencies は任意（無い・形が違えば `None` / 空・依存は 3 key の揃う要素だけ）。
pub fn issues_of(text: &str) -> Option<Vec<Issue>> {
    let tree = json_tree::parse(text).ok()?;
    tree.as_array()?
        .iter()
        .map(|node| {
            let text_of = |key: &str| node.get(key).and_then(Tree::as_str).map(str::to_owned);
            let array_of = |key: &str| node.get(key).and_then(Tree::as_array).unwrap_or_default();
            let priority = match node.get("priority") {
                Some(Tree::Num(digits)) => digits.parse::<u64>().ok(),
                _ => None,
            };
            Some(Issue {
                id: text_of("id")?,
                status: text_of("status")?,
                title: text_of("title").unwrap_or_default(),
                updated: text_of("updated_at").unwrap_or_else(|| NONE.to_owned()),
                priority,
                labels: array_of("labels").iter().filter_map(Tree::as_str).map(str::to_owned).collect(),
                deps: array_of("dependencies").iter().filter_map(dep_of).collect(),
            })
        })
        .collect()
}

/// 依存の 1 要素（`id` / `status` / `dependency_type` の文字列が揃わなければ `None`）。
fn dep_of(node: &Tree) -> Option<Dep> {
    let text_of = |key: &str| node.get(key).and_then(Tree::as_str).map(str::to_owned);
    Some(Dep { id: text_of("id")?, status: text_of("status")?, kind: text_of("dependency_type")? })
}

/// 台帳の現在値の 1 行（status 3 つの数え・DATA の `[BD_COUNT]` と席の指示文の `{ledger}` が同じ 1 本を読む）。
fn counts_body(issues: &[Issue]) -> String {
    let count = |status: &str| issues.iter().filter(|issue| issue.status == status).count();
    format!("open={} in_progress={} blocked={}", count("open"), count("in_progress"), count("blocked"))
}

/// 台帳の現在値を 1 行で返す（席の指示文の `{ledger}`・SessionStart の hook が読む）。
///
/// 読めない周は `None` である——呼び側が `unknown` を書く。**数え損ねを 0 に化けさせない**（憲法 C10）。
pub fn counts_of(bd: &str, timeout: Duration) -> Option<String> {
    read_ledger(bd, timeout).ok().as_deref().map(counts_body)
}

/// 台帳を子 process で読む（待ち上限を超えたら殺して断る・stderr は捨てる）。
fn read_ledger(bd: &str, timeout: Duration) -> Result<Vec<Issue>, LedgerError> {
    let mut child = Command::new(bd)
        .args(BD_ARGS)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| LedgerError::Unreadable)?;
    let deadline = Instant::now().checked_add(timeout);
    let body = collect_stdout(&mut child, deadline);
    let status = body.and_then(|bytes| finish(&mut child, deadline).map(|status| (bytes, status)));
    let Some((bytes, status)) = status else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(LedgerError::Unreadable);
    };
    if !status.success() {
        return Err(LedgerError::Unreadable);
    }
    let text = String::from_utf8(bytes).map_err(|_| LedgerError::Unreadable)?;
    issues_of(&text).ok_or(LedgerError::Unreadable)
}

/// stdout を別 thread で読み切る（pipe の詰まりで待ちが上限を越えない）。上限までに読めなければ `None`。
fn collect_stdout(child: &mut Child, deadline: Option<Instant>) -> Option<Vec<u8>> {
    let mut stdout = child.stdout.take()?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = stdout.read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(read);
    });
    let received = match deadline {
        Some(at) => receiver.recv_timeout(at.saturating_duration_since(Instant::now())).ok(),
        None => receiver.recv().ok(),
    };
    received?.ok()
}

/// 上限までに終わった子の status。終わらなければ `None`。
fn finish(child: &mut Child, deadline: Option<Instant>) -> Option<ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if deadline.is_none_or(|at| Instant::now() < at) => std::thread::sleep(POLL),
            _ => return None,
        }
    }
}
