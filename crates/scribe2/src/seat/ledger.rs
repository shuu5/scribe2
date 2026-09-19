//! 台帳の読み（`bd --readonly list --json` の子 process・席の指示文の `{ledger}` が読む）。
//!
//! `seat/rebrief.rs` から**挙動不変で移した**もの（`s2-07l.479.2`）: 作業記憶の復元は ADR-0045 §2 (2) で
//! 消えたが、SessionStart の指示文は台帳の現在値を 1 行で持つ（同 §2 (3)）ので、その読みだけを残す。
//! 待ち上限は rules 行 [`ID_TIMEOUT`] から読み、**読めない周は `None`**（数え損ねを 0 に化けさせない・C10）。
//! 子 process の出力（[`read_text`]）は復帰の DATA（`seat/recent.rs`・`s2-07l.489`）と**同じ 1 回**を共用する。
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

/// 台帳を読めない理由（**境界の enum**・[`POLARITY`]）。読めない周は数えを返さない。
///
/// **境界ごとに 1 つの enum が極性を持つ**のは憲法 C11.2 の求めで、`Option` に潰すと [`POLARITY`] の宣言 site
/// （極性一覧の guard でない側）ごと消える。待ち上限超過は `s2-07l.489` で別の variant になった（復帰の DATA
/// が `ledger-unreadable` / `ledger-timeout` を分けて出す・設計 seat-roles.md §21）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerError {
    /// 台帳を読めない（起動できない・rc 非 0・JSON 不能）。
    Unreadable,
    /// 待ち上限までに読み切れなかった（子は殺した）。
    Timeout,
}

/// 台帳の待ち上限を宣言する rules 行の id（**値は code に焼かない**・憲法 C5）。
pub const ID_TIMEOUT: &str = "seat.ledger_timeout_s";

/// `--bd` を渡さない周の台帳 client（PATH 解決は子 process の起動側）。
pub const DEFAULT_BD: &str = "bd";

/// 台帳を読む引数（`--readonly` を必ず付ける）。`--all` は closed を含む一覧。呼出しは 1 回である
/// （待ち上限 [`ID_TIMEOUT`]）。
const BD_ARGS: [&str; 6] = ["--readonly", "list", "--all", "--limit", "0", "--json"];

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

/// 台帳の 1 件（**読み手が読む key だけ**）。`s2-07l.479.2` で数え（status）だけに縮み、
/// `s2-07l.345` で列の読み手（`pipe::dispatch`・設計 dispatcher.md §2）が足した分だけ戻った
/// ——足すのは**読み手を足す便**である（到達しない構造を将来のために抱えない・C17）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// bead id。
    pub id: String,
    /// status の字面。
    pub status: String,
    /// priority field（無い・非負の整数でなければ `None`・列の順序が読む）。
    pub priority: Option<u64>,
    /// label の列（無ければ空・`intake:memo` の弁別が読む）。
    pub labels: Vec<String>,
    /// acceptance の本文（無ければ空・設計 pointer の行の出所）。
    pub acceptance: String,
    /// 依存の列（`dependencies[]`・2 key の揃う要素だけ・依存が閉じたかの判定が読む）。
    pub deps: Vec<Dep>,
}

/// 依存の 1 件（`dependencies[]` の `depends_on_id` と `type` だけを読む・**要素は status を持たない**ので
/// 閉じたかは読み手が同じ一覧（`--all`）の中で引く）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    /// 依存先の bead id（`depends_on_id`）。
    pub on: String,
    /// 依存の種別の字面（`type`・実測の母集団は `blocks` と `parent-child` の 2 値）。
    pub kind: String,
}

/// 台帳の JSON（`bd list --json` の配列）を読む。配列でない・要素に `id` / `status` の文字列が無い → `None`
/// （**2 key の必須は不変**＝欠けた要素を読み飛ばして「読めた」に化けさせない・C10）。
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
                priority,
                labels: array_of("labels").iter().filter_map(Tree::as_str).map(str::to_owned).collect(),
                acceptance: text_of("acceptance_criteria").unwrap_or_default(),
                deps: array_of("dependencies").iter().filter_map(dep_of).collect(),
            })
        })
        .collect()
}

/// 依存の 1 要素（`depends_on_id` / `type` の文字列が揃わなければ `None`）。key の字面は `bd --readonly list
/// --all --json` の現物から採った（2026-09-19 の実測: 要素は `issue_id` / `depends_on_id` / `type` /
/// `created_at` / `created_by` / `metadata` を持ち、**依存先の status は持たない**）。
fn dep_of(node: &Tree) -> Option<Dep> {
    let text_of = |key: &str| node.get(key).and_then(Tree::as_str).map(str::to_owned);
    Some(Dep { on: text_of("depends_on_id")?, kind: text_of("type")? })
}

/// 台帳の現在値の 1 行（status 3 つの数え・DATA の `[BD_COUNT]` と席の指示文の `{ledger}` が同じ 1 本を読む）。
fn counts_body(issues: &[Issue]) -> String {
    let count = |status: &str| issues.iter().filter(|issue| issue.status == status).count();
    format!("open={} in_progress={} blocked={}", count("open"), count("in_progress"), count("blocked"))
}

/// 台帳の現在値を 1 行で返す（席の指示文の `{ledger}`・SessionStart の hook が読む）。入力は [`read_text`] の
/// 出力（**同じ 1 回の出力を復帰の DATA と共用する**・設計 seat-roles.md §21）。
///
/// JSON として読めない周は `None` である——呼び側が `unknown` を書く。**数え損ねを 0 に化けさせない**（憲法 C10）。
pub fn counts_of(text: &str) -> Option<String> {
    issues_of(text).as_deref().map(counts_body)
}

/// 台帳を子 process で読み、[`Issue`] の列にする。**列の読み手も同じ 1 本**（設計 dispatcher.md §2・C2）。
pub fn read_ledger(bd: &str, timeout: Duration) -> Result<Vec<Issue>, LedgerError> {
    let text = read_text(bd, timeout)?;
    issues_of(&text).ok_or(LedgerError::Unreadable)
}

/// 台帳を子 process で読み、stdout の本文（JSON の text）を返す（待ち上限を超えたら殺して `Timeout`・stderr は
/// 捨てる）。件数の 1 行（[`counts_of`]）と復帰の DATA（`seat::recent`）が**この 1 回の出力**を分けて読む。
pub fn read_text(bd: &str, timeout: Duration) -> Result<String, LedgerError> {
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
    let (bytes, status) = match status {
        Ok(found) => found,
        Err(reason) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(reason);
        }
    };
    if !status.success() {
        return Err(LedgerError::Unreadable);
    }
    String::from_utf8(bytes).map_err(|_| LedgerError::Unreadable)
}

/// stdout を別 thread で読み切る（pipe の詰まりで待ちが上限を越えない）。上限までに読めなければ `Timeout`・
/// pipe を読めなければ `Unreadable`。
fn collect_stdout(child: &mut Child, deadline: Option<Instant>) -> Result<Vec<u8>, LedgerError> {
    let mut stdout = child.stdout.take().ok_or(LedgerError::Unreadable)?;
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
    received.ok_or(LedgerError::Timeout)?.map_err(|_| LedgerError::Unreadable)
}

/// 上限までに終わった子の status。終わらなければ `Timeout`・待てなければ `Unreadable`。
fn finish(child: &mut Child, deadline: Option<Instant>) -> Result<ExitStatus, LedgerError> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if deadline.is_none_or(|at| Instant::now() < at) => std::thread::sleep(POLL),
            Ok(None) => return Err(LedgerError::Timeout),
            Err(_) => return Err(LedgerError::Unreadable),
        }
    }
}
