//! 作業記憶の復元 DATA（`seat rebrief`・設計 docs/design/working-memory.md §5.2・ADR-0018 §2.1 / §2.2 /
//! §2.4・SRS FR23 / AC8・憲法 C15 / C3.3）。
//!
//! **read-only**: 退避物の dir・置き場・anchor・台帳のどれも変えない（台帳は `bd --readonly` の子
//! process で読む・lock を取らない）。出力は行頭 [`Marker`] の typed 行で、brief の文章は作らない
//! （開発 session の層）。本文の意味は読まない——節 3 の行は形（tag・出所 pointer の kind と実在）
//! だけで印を付け、起票候補を列挙しても**起票しない**。
//!
//! DATA を出せない周（dir・打刻・anchor・台帳のいずれかが読めない）は **1 行も出さず**
//! [`RebriefError`] で断る（FailClosed・「0 件」に化けさせない）。`[WM] missing` は正常の DATA である。

use super::wm::{Anchor, Item, Pointer, PointerKind, Resolution, WmDoc};
use super::{is_unconsumed_name, seat_dir, seat_of, state, StateDir, WM_PREFIX, WM_SUFFIX};
use crate::fleet::json_tree::{self, Tree};
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// 台帳の待ち上限を宣言する rules 行の id（**値は code に焼かない**・憲法 C5）。
pub const ID_TIMEOUT: &str = "seat.ledger_timeout_s";
/// `--bd` を渡さない周の台帳 client（PATH 解決は子 process の起動側）。
pub const DEFAULT_BD: &str = "bd";
/// 台帳を読む引数（`--readonly` を必ず付ける）。
const BD_ARGS: [&str; 5] = ["--readonly", "list", "--limit", "0", "--json"];
/// 起票候補の tag。
const HARD_CANDIDATE: &str = "[hard候補]";
/// kind / resolution が無い行の字面。
const NONE: &str = "none";
/// 台帳に無い id の status の字面。
const UNKNOWN: &str = "unknown";
/// 子 process の終了を見に行く刻み。
const POLL: Duration = Duration::from_millis(10);

/// この境界の極性（[`RebriefError`]）: DATA を出せない周は 1 行も出さない。行為を止める判定ではない
/// ので Guard ではない（極性一覧に載せない・設計 §7）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// DATA の行頭 marker。**宣言順 = 出力順**（C2・正本は [`ALL`] と外形 snapshot）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Marker {
    /// 現在の sid（打刻から）。
    Sid,
    /// 退避物の採否（found / candidate / missing / ambiguous / unreadable）。
    Wm,
    /// 節 2 の 1 行。
    WmPlan,
    /// 節 2 が不在か空。
    WmPlanEmpty,
    /// 節 1 の 1 行（逐語）。
    WmUserDirective,
    /// 節 1 が不在か空。
    WmUserDirectiveEmpty,
    /// 節 3 の 1 項目（kind・resolution・逐語の先頭行）。
    WmDirective,
    /// 節 3 が不在か空。
    WmDirectiveEmpty,
    /// 節 3 の件数（列挙との対）。
    WmDirectiveCount,
    /// 別席の未 consumed 退避物（消費しない）。
    OrphanWm,
    /// 確認した上で別席の未 consumed が 0。
    OrphanNone,
    /// 台帳の status 別件数。
    BdCount,
    /// 台帳の in_progress の 1 件。
    BdInprogress,
    /// 台帳の in_progress が 0。
    BdInprogressNone,
    /// 節 3 が言及する bead id の台帳 status。
    Diff,
    /// 節 3 が bead id を 1 つも言及しない。
    DiffNone,
    /// 起票候補（`[hard候補]` かつ pointer なし）の 1 行。
    TicketCandidate,
    /// 起票候補が 0。
    TicketCandidateNone,
}

/// [`Marker`] の全 variant（宣言順）。
pub const ALL: &[Marker] = &[
    Marker::Sid,
    Marker::Wm,
    Marker::WmPlan,
    Marker::WmPlanEmpty,
    Marker::WmUserDirective,
    Marker::WmUserDirectiveEmpty,
    Marker::WmDirective,
    Marker::WmDirectiveEmpty,
    Marker::WmDirectiveCount,
    Marker::OrphanWm,
    Marker::OrphanNone,
    Marker::BdCount,
    Marker::BdInprogress,
    Marker::BdInprogressNone,
    Marker::Diff,
    Marker::DiffNone,
    Marker::TicketCandidate,
    Marker::TicketCandidateNone,
];

impl Marker {
    /// 行頭の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sid => "[SID]",
            Self::Wm => "[WM]",
            Self::WmPlan => "[WM-PLAN]",
            Self::WmPlanEmpty => "[WM-PLAN-EMPTY]",
            Self::WmUserDirective => "[WM-USER-DIRECTIVE]",
            Self::WmUserDirectiveEmpty => "[WM-USER-DIRECTIVE-EMPTY]",
            Self::WmDirective => "[WM-DIRECTIVE]",
            Self::WmDirectiveEmpty => "[WM-DIRECTIVE-EMPTY]",
            Self::WmDirectiveCount => "[WM-DIRECTIVE-COUNT]",
            Self::OrphanWm => "[ORPHAN-WM]",
            Self::OrphanNone => "[ORPHAN-NONE]",
            Self::BdCount => "[BD-COUNT]",
            Self::BdInprogress => "[BD-INPROGRESS]",
            Self::BdInprogressNone => "[BD-INPROGRESS-NONE]",
            Self::Diff => "[DIFF]",
            Self::DiffNone => "[DIFF-NONE]",
            Self::TicketCandidate => "[TICKET-CANDIDATE]",
            Self::TicketCandidateNone => "[TICKET-CANDIDATE-NONE]",
        }
    }
}

/// DATA を出せない理由（**境界の enum**・[`POLARITY`]）。どの variant でも stdout は 0 行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebriefError {
    /// 置き場を解けない。
    StateDir,
    /// 待ち上限の rules 行が読めない・不発効。
    NoRule,
    /// 退避物の dir を読めない（0 件と読み替えない）。
    WmDirUnreadable,
    /// 打刻 file が無い。
    SidMissing,
    /// 打刻 file が読めない・最終行が壊れている・空。
    SidUnreadable,
    /// 最終の打刻の `sid` が空。
    SidEmpty,
    /// anchor が dir でない。
    AnchorMissing,
    /// 台帳を読めない（起動できない・rc 非 0・JSON 不能・待ち上限超過）。
    LedgerUnreadable,
}

impl RebriefError {
    /// 断りの行の `reason=`。
    pub fn reason(self) -> &'static str {
        match self {
            Self::StateDir => "state-dir",
            Self::NoRule => "no-rule",
            Self::WmDirUnreadable => "wm-dir-unreadable",
            Self::SidMissing => "sid-missing",
            Self::SidUnreadable => "sid-unreadable",
            Self::SidEmpty => "sid-empty",
            Self::AnchorMissing => "anchor-missing",
            Self::LedgerUnreadable => "ledger-unreadable",
        }
    }
}

/// 断りの 1 行（stderr）。
pub fn render_unavailable(err: RebriefError) -> String {
    format!("seat: rebrief unavailable reason={}", err.reason())
}

/// 待ち上限を**渡された manifest** から読む。不発効・別の形・不在は `None`（呼び側は `no-rule`）。
pub fn timeout_of(manifest: &Manifest) -> Option<Duration> {
    let row = manifest.get(ID_TIMEOUT)?;
    match (row.enabled, &row.value) {
        (true, RuleValue::Int(found)) => Some(Duration::from_secs(*found)),
        _ => None,
    }
}

/// 復元 1 回の入力。
pub struct Request<'a> {
    /// tmux target（frontmatter の `seat:` と突き合わせる）。
    pub target: &'a str,
    /// 退避物の dir。
    pub wm_dir: &'a Path,
    /// 解決済みの置き場（打刻を読む）。
    pub state_dir: &'a StateDir,
    /// 実在検査と台帳 prefix の repo root。
    pub anchor: &'a Path,
    /// `.beads` から prefix を解けない周の台帳 prefix。
    pub prefix: Option<&'a str>,
    /// 台帳 client。
    pub bd: &'a str,
    /// 台帳の待ち上限（rules 行 [`ID_TIMEOUT`]・呼び側が解く）。
    pub timeout: Duration,
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
}

/// 退避物の dir の走査結果（名前順）。
struct Scan {
    /// 自席の未 consumed の名前。
    own: Vec<String>,
    /// 別席の未 consumed の (名前, 席)。
    orphans: Vec<(String, String)>,
}

/// 節 3 の 1 項目の読み。
struct Row<'a> {
    /// 項目。
    item: &'a Item,
    /// 出所 pointer（暫定なら `None`）。
    pointed: Option<(PointerKind, Resolution)>,
}

/// DATA を組む。**全部を読み終えてから**行を返す（途中で断る周は 1 行も返さない）。
pub fn run(request: &Request) -> Result<Vec<String>, RebriefError> {
    let scan = scan(request.wm_dir, request.target)?;
    let sid = sid_of(&seat_dir(&request.state_dir.path, request.target))?;
    let anchor = anchor_of(request.anchor, request.prefix)?;
    let issues = read_ledger(request.bd, request.timeout)?;
    let mut lines: Vec<(Marker, String)> = vec![(Marker::Sid, sid.clone())];
    let doc = adopt(request.wm_dir, &scan.own, &sid, &mut lines);
    if let Some(doc) = &doc {
        let rows: Vec<Row> = doc
            .directives
            .iter()
            .map(|item| Row { item, pointed: pointed(item, &anchor) })
            .collect();
        section_lines(doc, &rows, &mut lines);
        diff_lines(&rows, &issues, anchor.prefixes(), &mut lines);
        ticket_lines(&rows, &mut lines);
    }
    orphan_lines(&scan.orphans, &mut lines);
    ledger_lines(&issues, &mut lines);
    // 出力順 = marker の宣言順（安定 sort＝同じ marker の中は組んだ順のまま）。
    lines.sort_by_key(|(marker, _)| *marker);
    Ok(lines.into_iter().map(|(marker, body)| render_line(marker, &body)).collect())
}

/// 1 行を組む（本文が空なら marker だけ）。
fn render_line(marker: Marker, body: &str) -> String {
    if body.is_empty() {
        marker.as_str().to_owned()
    } else {
        format!("{} {body}", marker.as_str())
    }
}

/// 未 consumed の退避物を自席と別席に分ける（名乗りの無い file は数えない＝`scan_wm` と同じ）。
fn scan(dir: &Path, target: &str) -> Result<Scan, RebriefError> {
    let entries = std::fs::read_dir(dir).map_err(|_| RebriefError::WmDirUnreadable)?;
    let mut found = Scan { own: Vec::new(), orphans: Vec::new() };
    for entry in entries {
        let entry = entry.map_err(|_| RebriefError::WmDirUnreadable)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_unconsumed_name(&name) {
            continue;
        }
        match seat_of(&entry.path()) {
            Some(seat) if seat == target => found.own.push(name),
            Some(seat) => found.orphans.push((name, seat)),
            None => {}
        }
    }
    found.own.sort();
    found.orphans.sort();
    Ok(found)
}

/// 打刻の最終行の `sid`（不在 / 読めない / 空 を分けて断る・externalize と同じ読み）。
fn sid_of(seat: &Path) -> Result<String, RebriefError> {
    let text = match std::fs::read_to_string(state::path(seat)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(RebriefError::SidMissing),
        Err(_) => return Err(RebriefError::SidUnreadable),
    };
    let line = text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or(RebriefError::SidUnreadable)?;
    let stamp = state::Stamp::from_line(line).map_err(|_| RebriefError::SidUnreadable)?;
    let sid = stamp.sid.trim();
    if sid.is_empty() {
        return Err(RebriefError::SidEmpty);
    }
    Ok(sid.to_owned())
}

/// anchor を開く。`.beads` から prefix を解けなければ `--prefix` を使う。
fn anchor_of(root: &Path, prefix: Option<&str>) -> Result<Anchor, RebriefError> {
    let anchor = Anchor::open(root).ok_or(RebriefError::AnchorMissing)?;
    match prefix {
        Some(found) if anchor.prefixes().is_empty() => Ok(Anchor::with_prefixes(root, vec![found.to_owned()])),
        _ => Ok(anchor),
    }
}

/// 自席の退避物を採否し `[WM]` の行を足す。採った file を読めた周だけ中身を返す。
fn adopt(dir: &Path, own: &[String], sid: &str, lines: &mut Vec<(Marker, String)>) -> Option<WmDoc> {
    let name = match own {
        [] => {
            lines.push((Marker::Wm, "missing".to_owned()));
            return None;
        }
        [one] => one,
        many => {
            lines.push((Marker::Wm, format!("ambiguous n={}", many.len())));
            return None;
        }
    };
    let doc = std::fs::read_to_string(dir.join(name))
        .ok()
        .and_then(|text| WmDoc::parse(&text).ok());
    let from = name.strip_prefix(WM_PREFIX).and_then(|rest| rest.strip_suffix(WM_SUFFIX)).unwrap_or_default();
    let state = match &doc {
        None => format!("unreadable file={name}"),
        Some(_) if from == sid => format!("found file={name}"),
        Some(_) => format!("candidate file={name} sid={from}"),
    };
    lines.push((Marker::Wm, state));
    doc
}

/// 項目の出所 pointer（暫定なら `None`）。
fn pointed(item: &Item, anchor: &Anchor) -> Option<(PointerKind, Resolution)> {
    match super::wm::pointer_of(item, anchor) {
        Pointer::Provisional => None,
        Pointer::Pointed { kind, resolution } => Some((kind, resolution)),
    }
}

/// resolution の DATA の名前（variant 名）。
fn resolution_name(resolution: Resolution) -> &'static str {
    match resolution {
        Resolution::Resolved => "Resolved",
        Resolution::Unresolved => "Unresolved",
        Resolution::Unchecked => "Unchecked",
    }
}

/// 項目の先頭行（逐語）。
fn head_line(item: &Item) -> &str {
    item.text.lines().next().unwrap_or_default()
}

/// 3 節の行（空の節は空印）と節 3 の件数。
fn section_lines(doc: &WmDoc, rows: &[Row], lines: &mut Vec<(Marker, String)>) {
    let plan: Vec<&str> = doc.plan.lines().filter(|line| !line.trim().is_empty()).collect();
    if plan.is_empty() {
        lines.push((Marker::WmPlanEmpty, String::new()));
    }
    lines.extend(plan.into_iter().map(|line| (Marker::WmPlan, line.to_owned())));
    let user: Vec<&str> = doc.user.iter().flat_map(|item| item.text.lines()).collect();
    if user.is_empty() {
        lines.push((Marker::WmUserDirectiveEmpty, String::new()));
    }
    lines.extend(user.into_iter().map(|line| (Marker::WmUserDirective, line.to_owned())));
    if rows.is_empty() {
        lines.push((Marker::WmDirectiveEmpty, String::new()));
    }
    let (mut provisional, mut unresolved) = (0_usize, 0_usize);
    for row in rows {
        let (kind, resolution) = match row.pointed {
            None => {
                provisional = provisional.saturating_add(1);
                (NONE, NONE)
            }
            Some((kind, resolution)) => {
                if resolution == Resolution::Unresolved {
                    unresolved = unresolved.saturating_add(1);
                }
                (kind.as_str(), resolution_name(resolution))
            }
        };
        let body = format!("kind={kind} resolution={resolution} line={}", head_line(row.item));
        lines.push((Marker::WmDirective, body));
    }
    let count = format!("total={} provisional={provisional} unresolved={unresolved}", rows.len());
    lines.push((Marker::WmDirectiveCount, count));
}

/// 別席の未 consumed（無ければ確認済みの 0）。
fn orphan_lines(orphans: &[(String, String)], lines: &mut Vec<(Marker, String)>) {
    if orphans.is_empty() {
        lines.push((Marker::OrphanNone, String::new()));
    }
    lines.extend(
        orphans
            .iter()
            .map(|(name, seat)| (Marker::OrphanWm, format!("file={name} seat={seat}"))),
    );
}

/// 台帳の件数と in_progress の列挙。
fn ledger_lines(issues: &[Issue], lines: &mut Vec<(Marker, String)>) {
    let count = |status: &str| issues.iter().filter(|issue| issue.status == status).count();
    let body = format!(
        "open={} in_progress={} blocked={}",
        count("open"),
        count("in_progress"),
        count("blocked")
    );
    lines.push((Marker::BdCount, body));
    let active: Vec<&Issue> = issues.iter().filter(|issue| issue.status == "in_progress").collect();
    if active.is_empty() {
        lines.push((Marker::BdInprogressNone, String::new()));
    }
    lines.extend(active.into_iter().map(|issue| {
        (Marker::BdInprogress, format!("{} updated={} {}", issue.id, issue.updated, issue.title))
    }));
}

/// 節 3 が言及する bead id ごとの台帳 status（台帳に無い id は `unknown`）。
fn diff_lines(rows: &[Row], issues: &[Issue], prefixes: &[String], lines: &mut Vec<(Marker, String)>) {
    let mut ids: Vec<String> = Vec::new();
    for row in rows {
        for id in ledger_ids(&row.item.text, prefixes) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    if ids.is_empty() {
        lines.push((Marker::DiffNone, String::new()));
    }
    for id in ids {
        let status = issues
            .iter()
            .find(|issue| issue.id == id)
            .map_or(UNKNOWN, |issue| issue.status.as_str());
        lines.push((Marker::Diff, format!("{id} bd={status}")));
    }
}

/// 起票候補（tag が `[hard候補]` かつ pointer なし＝typed な条件だけ）。
fn ticket_lines(rows: &[Row], lines: &mut Vec<(Marker, String)>) {
    let found: Vec<&str> = rows
        .iter()
        .filter(|row| row.pointed.is_none())
        .map(|row| head_line(row.item))
        .filter(|head| head.split_whitespace().any(|word| word == HARD_CANDIDATE))
        .collect();
    if found.is_empty() {
        lines.push((Marker::TicketCandidateNone, String::new()));
    }
    lines.extend(found.into_iter().map(|head| (Marker::TicketCandidate, head.to_owned())));
}

/// 本文が言及する `<prefix>-<id>` の列（出現順・重複あり・文末の `.` は落とす）。
pub fn ledger_ids(text: &str, prefixes: &[String]) -> Vec<String> {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '.'))
        .map(|token| token.trim_end_matches('.'))
        .filter(|token| prefixes.iter().any(|prefix| is_ledger_id(token, prefix)))
        .map(str::to_owned)
        .collect()
}

/// `<prefix>-<id>`（prefix は非空・id は英数で始まり英数と `.` だけ）。
fn is_ledger_id(token: &str, prefix: &str) -> bool {
    !prefix.is_empty()
        && token
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(|id| {
                id.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
                    && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
            })
}

/// 台帳の JSON（`bd list --json` の配列）を読む。配列でない・要素に `id` / `status` の文字列が無い → `None`。
pub fn issues_of(text: &str) -> Option<Vec<Issue>> {
    let tree = json_tree::parse(text).ok()?;
    tree.as_array()?
        .iter()
        .map(|node| {
            let text_of = |key: &str| node.get(key).and_then(Tree::as_str).map(str::to_owned);
            Some(Issue {
                id: text_of("id")?,
                status: text_of("status")?,
                title: text_of("title").unwrap_or_default(),
                updated: text_of("updated_at").unwrap_or_else(|| NONE.to_owned()),
            })
        })
        .collect()
}

/// 台帳を子 process で読む（待ち上限を超えたら殺して断る・stderr は捨てる）。
fn read_ledger(bd: &str, timeout: Duration) -> Result<Vec<Issue>, RebriefError> {
    let mut child = Command::new(bd)
        .args(BD_ARGS)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| RebriefError::LedgerUnreadable)?;
    let deadline = Instant::now().checked_add(timeout);
    let body = collect_stdout(&mut child, deadline);
    let status = body.and_then(|bytes| finish(&mut child, deadline).map(|status| (bytes, status)));
    let Some((bytes, status)) = status else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(RebriefError::LedgerUnreadable);
    };
    if !status.success() {
        return Err(RebriefError::LedgerUnreadable);
    }
    let text = String::from_utf8(bytes).map_err(|_| RebriefError::LedgerUnreadable)?;
    issues_of(&text).ok_or(RebriefError::LedgerUnreadable)
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

#[cfg(test)]
mod tests {
    use super::{issues_of, ledger_ids, render_unavailable, Marker, RebriefError, ALL};
    use crate::order::is_declaration_order;

    /// (10) `ALL` は宣言順（判別子 0, 1, 2, …・ADR-0013 §2.2）で、並びは出力順そのもの。
    #[test]
    fn seat_wm_rebrief_marker_all_is_in_declaration_order() {
        assert!(is_declaration_order(ALL, |marker| marker as usize), "ALL は宣言順: {ALL:?}");
        let names: Vec<&str> = ALL.iter().map(|marker| marker.as_str()).collect();
        assert_eq!(
            names,
            [
                "[SID]",
                "[WM]",
                "[WM-PLAN]",
                "[WM-PLAN-EMPTY]",
                "[WM-USER-DIRECTIVE]",
                "[WM-USER-DIRECTIVE-EMPTY]",
                "[WM-DIRECTIVE]",
                "[WM-DIRECTIVE-EMPTY]",
                "[WM-DIRECTIVE-COUNT]",
                "[ORPHAN-WM]",
                "[ORPHAN-NONE]",
                "[BD-COUNT]",
                "[BD-INPROGRESS]",
                "[BD-INPROGRESS-NONE]",
                "[DIFF]",
                "[DIFF-NONE]",
                "[TICKET-CANDIDATE]",
                "[TICKET-CANDIDATE-NONE]",
            ],
            "宣言順 = 出力順"
        );
        assert_eq!(Marker::Sid as usize, 0, "先頭の判別子");
        assert_eq!(Marker::TicketCandidateNone as usize, 17, "末尾の判別子");
    }

    /// 言及 id は prefix の形だけを拾い、文末の `.` と別 prefix を落とす。
    #[test]
    fn seat_wm_rebrief_ledger_ids_pick_prefixed_ids_only() {
        let prefixes = vec!["s2".to_owned()];
        let text = "- [auto] [P1] 続き s2-07l.61。s2-abc. と (s2-x1) → SSOT: ADR-0018 / f2-9 / s2- / xs2-1";
        assert_eq!(ledger_ids(text, &prefixes), ["s2-07l.61", "s2-abc", "s2-x1"]);
        assert!(ledger_ids("s2-1", &[]).is_empty(), "prefix が無ければ拾わない");
    }

    /// 台帳の JSON は配列で、要素ごとに id と status の文字列を要る（欠ければ全体を読めない側へ倒す）。
    #[test]
    fn seat_wm_rebrief_issues_require_id_and_status() {
        let good = r#"[{"id":"s2-1","status":"open","title":"t","updated_at":"2026-09-13T00:00:00Z","priority":1}]"#;
        let issues = issues_of(good).unwrap_or_default();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues.first().map(|issue| issue.updated.as_str()), Some("2026-09-13T00:00:00Z"));
        assert_eq!(issues_of("[]"), Some(Vec::new()), "空の台帳は正当な 0");
        assert_eq!(issues_of(r#"[{"id":"s2-1"}]"#), None, "status 欠落");
        assert_eq!(issues_of(r#"{"id":"s2-1"}"#), None, "配列でない");
        assert_eq!(issues_of("not json"), None, "JSON 不能");
        assert_eq!(render_unavailable(RebriefError::LedgerUnreadable), "seat: rebrief unavailable reason=ledger-unreadable");
    }
}
