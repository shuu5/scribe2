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
use crate::hook::vessel::digest::{self, PluginRecord};
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
/// 判定点を持たない memo を stale と数える日数（以上）の rules 行の id（設計 ledger-triage.md §4）。
pub const ID_STALE_DAYS: &str = "ledger.memo_stale_days";
/// stale と数える memo の priority の上限（以下）の rules 行の id。
pub const ID_STALE_PRIORITY: &str = "ledger.memo_stale_priority";
/// memo の弁別に使う label（題の字面は読まない）。
const MEMO_LABEL: &str = "intake:memo";
/// 判定点に数える依存の種別（parent-child は数えない）。
const BLOCKS: &str = "blocks";
/// 1 日の秒数。
const DAY_S: u64 = 86_400;
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
/// 読み込み元の記録が無い周の `[PLUGIN] drift=` の字面（doctor の導入先の行と同じ語・`none` に潰さない）。
const UNRECORDED: &str = "unrecorded";
/// `[PLUGIN] drift=` の食い違い 0 語の字面。
const DRIFT_NONE: &str = "none";
/// `[PLUGIN] drift=` の語（記録の digest ≠ 今の hooks.json の digest）。
const DRIFT_HOOKS: &str = "hooks";
/// `[PLUGIN] drift=` の語（記録の build 元 commit ≠ この binary の build 元 commit）。
const DRIFT_BINARY: &str = "binary";
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
    /// 読み込み元の記録（`.303` の `seat/<target>/plugin`）と今の hooks.json / この binary の食い違い（席の同一性の隣・
    /// consumer-sync.md §6・`s2-07l.304`）。判断材料であって規則ではない。
    Plugin,
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
    /// 判定点（blocks 依存）を全部過ぎた memo の 1 件（設計 ledger-triage.md §3）。
    MemoDue,
    /// 判定点を過ぎた memo の件数と memo の母集団。
    MemoDueCount,
    /// 確認した上で判定点を過ぎた memo が 0。
    MemoDueNone,
    /// 判定点を持たず齢を過ぎた memo の 1 件。
    MemoStale,
    /// 齢を過ぎた memo の件数・母集団・判定不能（`updated_at` が読めない）の件数。
    MemoStaleCount,
    /// 確認した上で齢を過ぎた memo が 0。
    MemoStaleNone,
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
    Marker::Plugin,
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
    Marker::MemoDue,
    Marker::MemoDueCount,
    Marker::MemoDueNone,
    Marker::MemoStale,
    Marker::MemoStaleCount,
    Marker::MemoStaleNone,
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
            Self::Plugin => "[PLUGIN]",
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
            Self::MemoDue => "[MEMO-DUE]",
            Self::MemoDueCount => "[MEMO-DUE-COUNT]",
            Self::MemoDueNone => "[MEMO-DUE-NONE]",
            Self::MemoStale => "[MEMO-STALE]",
            Self::MemoStaleCount => "[MEMO-STALE-COUNT]",
            Self::MemoStaleNone => "[MEMO-STALE-NONE]",
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

/// memo の stale を数える閾値（rules 行 [`ID_STALE_DAYS`] / [`ID_STALE_PRIORITY`]・呼び側が解く）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    /// `updated_at` からの日数（以上）。
    pub stale_days: u64,
    /// priority field の上限（以下）。
    pub stale_priority: u64,
}

/// 閾値 2 つを**渡された manifest** から読む。どちらかが不発効・別の形・不在なら `None`（呼び側は `no-rule`）。
pub fn thresholds_of(manifest: &Manifest) -> Option<Thresholds> {
    let int = |id: &str| match manifest.get(id) {
        Some(row) if row.enabled => match row.value {
            RuleValue::Int(found) => Some(found),
            _ => None,
        },
        _ => None,
    };
    Some(Thresholds { stale_days: int(ID_STALE_DAYS)?, stale_priority: int(ID_STALE_PRIORITY)? })
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
    /// memo の stale を数える閾値（rules 行・呼び側が解く）。
    pub thresholds: Thresholds,
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

/// 判定点を過ぎた memo の 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    /// bead id。
    pub id: String,
    /// priority field。
    pub priority: Option<u64>,
    /// 閉じた blocks 依存の id（数字順）。
    pub blocks: Vec<String>,
    /// `updated_at` の字面。
    pub updated: String,
}

/// 齢を過ぎた memo の 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stale {
    /// bead id。
    pub id: String,
    /// priority field。
    pub priority: Option<u64>,
    /// `updated_at` から now までの日数（切り捨て・実測）。
    pub age_days: u64,
    /// `updated_at` の字面。
    pub updated: String,
}

/// memo の棚卸しの判定（[`triage`] の出力・列は id の数字順）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Triage {
    /// 判定点を過ぎた memo。
    pub due: Vec<Due>,
    /// 齢を過ぎた memo。
    pub stale: Vec<Stale>,
    /// memo の母集団（label `intake:memo` を持つ open）。
    pub memo_total: usize,
    /// stale の判定に届いて `updated_at` を読めなかった memo の件数（stale に数えない・C10）。
    pub unreadable: usize,
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
    let seat = seat_dir(&request.state_dir.path, request.target);
    let sid = sid_of(&seat)?;
    let anchor = anchor_of(request.anchor, request.prefix)?;
    let issues = read_ledger(request.bd, request.timeout)?;
    let plugin = plugin_line(&PluginRecord::read(&seat), env!("SCRIBE2_BUILD_COMMIT"));
    let mut lines: Vec<(Marker, String)> = vec![(Marker::Sid, sid.clone()), (Marker::Plugin, plugin)];
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
    memo_lines(&triage(&issues, request.thresholds, &crate::fleet::cli::now_utc()), &mut lines);
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

/// `[PLUGIN]` の本文（consumer-sync.md §6・判断材料であって規則ではない）: 記録が在れば `root=<root> hooks=<digest|unreadable>
/// binary=<sha> drift=<語>`（`built` はこの binary の build 元 commit・[`plugin_drift`]）。記録が**無い**周は
/// `drift=unrecorded`・**読めない**周は `drift=unreadable` の 1 行（「無い」を黙らせず、「読めない」を「無い」に潰さない・C10）。
fn plugin_line(record: &PluginRecord, built: &str) -> String {
    let PluginRecord::Recorded { root, hooks, binary, .. } = record else {
        let word = if *record == PluginRecord::Absent { UNRECORDED } else { digest::UNREADABLE };
        return format!("drift={word}");
    };
    let current = digest::hooks_digest(Path::new(root));
    let drift = plugin_drift(hooks.as_deref(), current.as_deref(), binary, built);
    format!("root={root} hooks={} binary={binary} drift={drift}", hooks.as_deref().unwrap_or(digest::UNREADABLE))
}

/// 食い違いの語（closed・`none` / `hooks` / `binary` / `hooks+binary` / `unreadable`・doctor の `Drift` の列を席の側に写す）:
/// `hooks` は記録の digest（`recorded`）と今の hooks.json の digest（`current`）の不一致、`binary` は記録の build 元
/// commit と この binary の不一致。どちらかの digest が読めない周は `unreadable`（比べられないことを `none` に潰さない・
/// binary の語も重ねない＝語は closed の 5 つ）。
fn plugin_drift(recorded: Option<&str>, current: Option<&str>, binary: &str, built: &str) -> String {
    let (Some(recorded), Some(current)) = (recorded, current) else {
        return digest::UNREADABLE.to_owned();
    };
    let words: Vec<&str> = [(recorded != current, DRIFT_HOOKS), (binary != built, DRIFT_BINARY)]
        .into_iter()
        .filter_map(|(holds, word)| holds.then_some(word))
        .collect();
    if words.is_empty() { DRIFT_NONE.to_owned() } else { words.join("+") }
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

/// memo の棚卸しの行（件数行は必ず出す・0 件は `-NONE` も足す）。
fn memo_lines(found: &Triage, lines: &mut Vec<(Marker, String)>) {
    let priority = |value: Option<u64>| value.map_or_else(|| NONE.to_owned(), |p| p.to_string());
    lines.extend(found.due.iter().map(|due| {
        let body = format!("{} p={} blocks={} updated={}", due.id, priority(due.priority), due.blocks.join(","), due.updated);
        (Marker::MemoDue, body)
    }));
    lines.push((Marker::MemoDueCount, format!("n={} of={}", found.due.len(), found.memo_total)));
    if found.due.is_empty() {
        lines.push((Marker::MemoDueNone, String::new()));
    }
    lines.extend(found.stale.iter().map(|stale| {
        let body = format!("{} p={} age_days={} updated={}", stale.id, priority(stale.priority), stale.age_days, stale.updated);
        (Marker::MemoStale, body)
    }));
    let count = format!("n={} of={} unreadable={}", found.stale.len(), found.memo_total, found.unreadable);
    lines.push((Marker::MemoStaleCount, count));
    if found.stale.is_empty() {
        lines.push((Marker::MemoStaleNone, String::new()));
    }
}

/// memo の棚卸しを判定する（**純関数・I/O なし**・設計 ledger-triage.md §2）。memo = label `intake:memo` を
/// 持つ open の issue。due = blocks 依存を 1 つ以上持ち全部が `closed`。stale = blocks 依存を持たず ∧
/// priority ≤ 閾値 ∧ `updated_at` から閾値日以上。`updated_at`（か `now`）が読めない memo は stale に数えず
/// `unreadable` に数える。parent-child 依存は数えない。
pub fn triage(issues: &[Issue], thresholds: Thresholds, now: &str) -> Triage {
    let now = epoch_of_ts(now);
    let mut found = Triage { due: Vec::new(), stale: Vec::new(), memo_total: 0, unreadable: 0 };
    let memos = issues
        .iter()
        .filter(|issue| issue.status == "open" && issue.labels.iter().any(|label| label == MEMO_LABEL));
    for memo in memos {
        found.memo_total = found.memo_total.saturating_add(1);
        let blocks: Vec<&Dep> = memo.deps.iter().filter(|dep| dep.kind == BLOCKS).collect();
        if !blocks.is_empty() {
            if blocks.iter().all(|dep| dep.status == "closed") {
                let mut ids: Vec<String> = blocks.iter().map(|dep| dep.id.clone()).collect();
                ids.sort_by_cached_key(|id| id_key(id));
                found.due.push(Due { id: memo.id.clone(), priority: memo.priority, blocks: ids, updated: memo.updated.clone() });
            }
            continue;
        }
        if !memo.priority.is_some_and(|p| p <= thresholds.stale_priority) {
            continue;
        }
        let age_s = now.zip(epoch_of_ts(&memo.updated)).map(|(at, updated)| at.saturating_sub(updated));
        match age_s {
            None => found.unreadable = found.unreadable.saturating_add(1),
            Some(age_s) if age_s / DAY_S >= thresholds.stale_days => found.stale.push(Stale {
                id: memo.id.clone(),
                priority: memo.priority,
                age_days: age_s / DAY_S,
                updated: memo.updated.clone(),
            }),
            Some(_) => {}
        }
    }
    found.due.sort_by_cached_key(|due| id_key(&due.id));
    found.stale.sort_by_cached_key(|stale| id_key(&stale.id));
    found
}

/// id の数字順の鍵（数字の連なりは値で・それ以外は字面で比べる＝`s2-07l.61` < `s2-07l.140`）。
fn id_key(id: &str) -> Vec<(u8, usize, String)> {
    let mut key = Vec::new();
    let mut rest = id;
    while let Some(first) = rest.chars().next() {
        let digit = first.is_ascii_digit();
        let end = rest.find(|ch: char| ch.is_ascii_digit() != digit).unwrap_or(rest.len());
        let (run, tail) = rest.split_at(end);
        if digit {
            let value = run.trim_start_matches('0');
            key.push((1, value.len(), value.to_owned()));
        } else {
            key.push((0, 0, run.to_owned()));
        }
        rest = tail;
    }
    key
}

/// `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)` を UNIX 秒にする（それ以外の形は `None`＝判定不能）。
fn epoch_of_ts(ts: &str) -> Option<u64> {
    let base = crate::fleet::epoch_of(&format!("{}Z", ts.get(..19)?))?;
    let mut tail = ts.get(19..)?;
    if let Some(frac) = tail.strip_prefix('.') {
        tail = frac.trim_start_matches(|ch: char| ch.is_ascii_digit());
        if tail.len() == frac.len() {
            return None;
        }
    }
    if tail == "Z" {
        return Some(base);
    }
    let (sign, offset) = (tail.get(..1)?, tail.get(1..)?);
    let (hours, minutes) = offset.split_once(':')?;
    let two_digits = |part: &str| part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit());
    if !two_digits(hours) || !two_digits(minutes) {
        return None;
    }
    let (hours, minutes) = (hours.parse::<u64>().ok()?, minutes.parse::<u64>().ok()?);
    if hours > 23 || minutes > 59 {
        return None;
    }
    let shift = hours.checked_mul(3_600)?.checked_add(minutes.checked_mul(60)?)?;
    match sign {
        "+" => base.checked_sub(shift),
        "-" => base.checked_add(shift),
        _ => None,
    }
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
    use super::{
        adopt, anchor_of, epoch_of_ts, finish, issues_of, ledger_ids, orphan_lines, plugin_drift, plugin_line, render_unavailable,
        sid_of, triage, Dep, Issue, Marker, PluginRecord, RebriefError, Thresholds, ALL,
    };
    use crate::order::is_declaration_order;
    use crate::seat::state;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    use proptest::prelude::*;
    use proptest::test_runner::Config;

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
                "[PLUGIN]",
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
                "[MEMO-DUE]",
                "[MEMO-DUE-COUNT]",
                "[MEMO-DUE-NONE]",
                "[MEMO-STALE]",
                "[MEMO-STALE-COUNT]",
                "[MEMO-STALE-NONE]",
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
        assert_eq!(Marker::Plugin as usize, 2, "席の同一性の隣（`[WM]` の直後・`s2-07l.304`）");
        assert_eq!(Marker::TicketCandidateNone as usize, 24, "末尾の判別子");
    }

    /// `[PLUGIN]` の食い違いの語は closed の 5 つ（consumer-sync.md §6）: hooks の digest の不一致は `hooks`・build 元 commit の
    /// 不一致は `binary`・両方は `hooks+binary`・どちらも同じは `none`・digest のどちらかが読めない周は `unreadable`
    /// （binary が違っても重ねない）。記録の無い / 読めない周の本文は `drift=unrecorded` / `drift=unreadable` の 1 語だけ。
    #[test]
    fn seat_wm_rebrief_plugin_drift_words_are_closed() {
        let (a, b, built) = ("aaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbb", "0123456789ab");
        assert_eq!(plugin_drift(Some(a), Some(a), built, built), "none");
        assert_eq!(plugin_drift(Some(a), Some(b), built, built), "hooks");
        assert_eq!(plugin_drift(Some(a), Some(a), "ffffffffffff", built), "binary");
        assert_eq!(plugin_drift(Some(a), Some(b), "ffffffffffff", built), "hooks+binary");
        assert_eq!(plugin_drift(None, Some(a), "ffffffffffff", built), "unreadable", "記録の digest が unreadable");
        assert_eq!(plugin_drift(Some(a), None, built, built), "unreadable", "今の hooks.json が無い");
        assert_eq!(plugin_line(&PluginRecord::Absent, built), "drift=unrecorded");
        assert_eq!(plugin_line(&PluginRecord::Unreadable, built), "drift=unreadable");
        let recorded = PluginRecord::Recorded {
            root: "/no/such/root".to_owned(),
            hooks: Some(a.to_owned()),
            binary: built.to_owned(),
            sid: "s".to_owned(),
            ts: 1,
        };
        assert_eq!(
            plugin_line(&recorded, built),
            format!("root=/no/such/root hooks={a} binary={built} drift=unreadable"),
            "root に hooks.json が無い周は記録の値を写して drift=unreadable"
        );
    }

    /// 判定の閾値（`s2-07l.217` の歯で使う値＝裁定 id `user 2026-09-13T14:06Z` の 3 日 / P2）。
    const THRESHOLDS: Thresholds = Thresholds { stale_days: 3, stale_priority: 2 };
    /// 判定の now。
    const NOW: &str = "2026-09-14T12:00:00Z";

    /// 1 件を組む（`labels` と `deps` は (id, status, kind)）。
    fn issue(id: &str, labels: &[&str], priority: Option<u64>, updated: &str, deps: &[(&str, &str, &str)]) -> Issue {
        Issue {
            id: id.to_owned(),
            status: "open".to_owned(),
            title: String::new(),
            updated: updated.to_owned(),
            priority,
            labels: labels.iter().map(|label| (*label).to_owned()).collect(),
            deps: deps
                .iter()
                .map(|(id, status, kind)| Dep { id: (*id).to_owned(), status: (*status).to_owned(), kind: (*kind).to_owned() })
                .collect(),
        }
    }

    /// 判定の純関数: due は blocks が全部 closed・stale は依存なし ∧ P ≤ 閾値 ∧ 齢 ≥ 閾値・順は id の数字順。
    #[test]
    fn prop_rebrief_memo_triage_examples() {
        let memo = ["intake:memo"];
        let issues = [
            issue("s2-07l.140", &memo, Some(1), NOW, &[("s2-9", "closed", "blocks"), ("s2-10", "closed", "blocks")]),
            issue("s2-07l.61", &memo, Some(3), NOW, &[("s2-1", "closed", "blocks")]),
            issue("s2-5", &memo, Some(2), NOW, &[("s2-1", "closed", "blocks"), ("s2-2", "open", "blocks")]),
            issue("s2-6", &memo, Some(2), "2026-09-10T12:00:00Z", &[("s2-e", "closed", "parent-child")]),
            issue("s2-7", &memo, Some(3), "2026-09-01T12:00:00Z", &[]),
            issue("s2-8", &memo, Some(0), "2026-09-12T12:00:00Z", &[]),
            issue("s2-9", &memo, Some(1), "none", &[]),
            issue("s2-11", &["x"], Some(0), "2026-09-01T12:00:00Z", &[]),
        ];
        let found = triage(&issues, THRESHOLDS, NOW);
        let due: Vec<(&str, Vec<String>)> = found.due.iter().map(|due| (due.id.as_str(), due.blocks.clone())).collect();
        assert_eq!(
            due,
            [("s2-07l.61", vec!["s2-1".to_owned()]), ("s2-07l.140", vec!["s2-9".to_owned(), "s2-10".to_owned()])]
        );
        let stale: Vec<(&str, u64)> = found.stale.iter().map(|stale| (stale.id.as_str(), stale.age_days)).collect();
        assert_eq!(stale, [("s2-6", 4)], "parent-child だけは依存なし・P3 と 2 日前は出ない");
        assert_eq!((found.memo_total, found.unreadable), (7, 1), "label の無い bead は母集団の外・none は判定不能");
    }

    /// `updated_at` の字面: 小数秒と UTC offset を読み、形の違う字面は判定不能。
    #[test]
    fn prop_rebrief_memo_epoch_reads_fraction_and_offset() {
        let base = epoch_of_ts("2026-09-13T05:06:07Z");
        assert!(base.is_some());
        assert_eq!(epoch_of_ts("2026-09-13T05:06:07.123456Z"), base);
        assert_eq!(epoch_of_ts("2026-09-13T14:06:07+09:00"), base);
        assert_eq!(epoch_of_ts("2026-09-12T23:06:07.5-06:00"), base);
        for bad in ["none", "", "2026-09-13T05:06:07", "2026-09-13T05:06:07.Z", "2026-09-13T05:06:07+9:00", "2026-09-13T05:06:07++9:00"] {
            assert_eq!(epoch_of_ts(bad), None, "{bad}");
        }
    }

    /// 反例の永続化を切り、case 数を 256 に pin する（`tests/e2e/prop.rs` と同じ形）。
    fn config() -> Config {
        Config {
            cases: 256,
            failure_persistence: None,
            ..Config::default()
        }
    }

    /// 任意の 1 件の材料（label・status・priority・updated・依存）。
    fn any_issue() -> impl Strategy<Value = (bool, bool, Option<u64>, usize, Vec<(bool, bool)>)> {
        (
            any::<bool>(),
            any::<bool>(),
            prop::option::of(0_u64..5),
            0_usize..UPDATED.len(),
            prop::collection::vec((any::<bool>(), any::<bool>()), 0..4),
        )
    }

    /// updated の候補（読める齢・未来・読めない字面）。
    const UPDATED: [&str; 6] =
        ["2026-09-14T11:00:00Z", "2026-09-12T12:00:00Z", "2026-09-10T12:00:00Z", "2026-09-01T00:00:00+09:00", "2026-09-20T00:00:00Z", "none"];

    proptest! {
        #![proptest_config(config())]

        /// 任意の Issue 列で due ∩ stale = ∅・due + stale ≤ memo_total・memo でない issue は両方に出ない。
        #[test]
        fn prop_rebrief_memo_due_and_stale_are_disjoint_memos(raw in prop::collection::vec(any_issue(), 0..12)) {
            let issues: Vec<Issue> = raw
                .iter()
                .enumerate()
                .map(|(at, (memo, open, priority, updated, deps))| {
                    let deps: Vec<Dep> = deps
                        .iter()
                        .enumerate()
                        .map(|(to, (closed, blocks))| Dep {
                            id: format!("s2-d{to}"),
                            status: if *closed { "closed" } else { "open" }.to_owned(),
                            kind: if *blocks { "blocks" } else { "parent-child" }.to_owned(),
                        })
                        .collect();
                    Issue {
                        id: format!("s2-{at}"),
                        status: if *open { "open" } else { "in_progress" }.to_owned(),
                        title: String::new(),
                        updated: UPDATED.get(*updated).copied().unwrap_or("none").to_owned(),
                        priority: *priority,
                        labels: if *memo { vec!["intake:memo".to_owned()] } else { Vec::new() },
                        deps,
                    }
                })
                .collect();
            let found = triage(&issues, THRESHOLDS, NOW);
            prop_assert!(found.due.iter().all(|due| found.stale.iter().all(|stale| stale.id != due.id)));
            prop_assert!(found.due.len().saturating_add(found.stale.len()) <= found.memo_total);
            let memos: Vec<&str> = issues
                .iter()
                .filter(|issue| issue.status == "open" && issue.labels.iter().any(|label| label == "intake:memo"))
                .map(|issue| issue.id.as_str())
                .collect();
            prop_assert_eq!(found.memo_total, memos.len());
            prop_assert!(found.due.iter().all(|due| memos.contains(&due.id.as_str())));
            prop_assert!(found.stale.iter().all(|stale| memos.contains(&stale.id.as_str())));
        }
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

    /// 歯ごとの空の tmp dir（in-file の歯の置き場・env を読まないのは器の本体の規律〔C2.2〕）。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("seat-rebrief-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// 打刻 file に sid の列を書いた席 dir（最終行が現在・末尾の空行は読み飛ばされる）。
    fn seat_with_sids(root: &Path, sids: &[&str]) -> PathBuf {
        let seat = root.join("seat");
        let _ = std::fs::create_dir_all(&seat);
        let lines: Vec<String> = sids
            .iter()
            .map(|sid| state::Stamp::now(state::Event::SessionStart, sid).to_line())
            .collect();
        let _ = std::fs::write(state::path(&seat), format!("{}\n\n", lines.join("\n")));
        seat
    }

    // flip-check: retroactive s2-07l.223
    /// `sid_of` は**最終行**の sid（桁が 1 つ足りない後の打刻が現在＝前の行を読まない）で、不在 / 読めない / 空を
    /// 別の variant で断る（読めない周を `sid-missing` に潰さない）。
    #[test]
    fn mutant_in_seat_rebrief_sid_of_reads_the_last_stamp_and_separates_missing_from_unreadable() {
        let root = scratch("sid-of");
        assert_eq!(sid_of(&seat_with_sids(&root.join("two"), &["abcdef", "abcde"])), Ok("abcde".to_owned()));
        assert_eq!(sid_of(&seat_with_sids(&root.join("empty"), &[" "])), Err(RebriefError::SidEmpty));
        assert_eq!(sid_of(&root.join("absent")), Err(RebriefError::SidMissing), "打刻 file が無い");
        let unreadable = root.join("unreadable");
        let _ = std::fs::create_dir_all(state::path(&unreadable));
        assert_eq!(sid_of(&unreadable), Err(RebriefError::SidUnreadable), "打刻 file が dir（不在ではない）");
        let _ = std::fs::remove_dir_all(&root);
    }

    // flip-check: retroactive s2-07l.223
    /// `anchor_of` は `.beads` から prefix を解けた周は `--prefix` を使わず、解けない周だけ `--prefix` へ落ちる
    /// （区切りの無い `--prefix` の字面もそのまま）。dir でない anchor は `anchor-missing`。
    #[test]
    fn mutant_in_seat_rebrief_anchor_of_prefers_ledger_prefix_over_the_flag() {
        let root = scratch("anchor-of");
        let with_ledger = root.join("with");
        let _ = std::fs::create_dir_all(with_ledger.join(".beads"));
        let _ = std::fs::write(with_ledger.join(".beads").join("config.yaml"), "issue-prefix: s2\n");
        let bare = root.join("bare");
        let _ = std::fs::create_dir_all(&bare);
        let prefixes = |anchor: Result<super::Anchor, RebriefError>| anchor.map(|anchor| anchor.prefixes().to_vec());
        assert_eq!(prefixes(anchor_of(&with_ledger, Some("zz"))), Ok(vec!["s2".to_owned()]), "台帳の prefix が勝つ");
        assert_eq!(prefixes(anchor_of(&with_ledger, None)), Ok(vec!["s2".to_owned()]));
        assert_eq!(prefixes(anchor_of(&bare, Some("zz"))), Ok(vec!["zz".to_owned()]), "解けない周だけ flag");
        assert_eq!(prefixes(anchor_of(&bare, None)), Ok(Vec::new()), "どちらも無ければ空");
        assert_eq!(prefixes(anchor_of(&root.join("absent"), Some("zz"))), Err(RebriefError::AnchorMissing));
        let _ = std::fs::remove_dir_all(&root);
    }

    // flip-check: retroactive s2-07l.223
    /// `adopt` の件数境界: 候補 0 は `missing`・1 は sid が一致すれば `found` / 違えば `candidate`・2 は
    /// `ambiguous n=2`（選ばない）・読めない 1 件は `unreadable`。中身を返すのは読めた 1 件の周だけ。
    #[test]
    fn mutant_in_seat_rebrief_adopt_distinguishes_found_candidate_missing_ambiguous() {
        let root = scratch("adopt");
        let _ = std::fs::write(root.join("working-memory.s1.md"), "---\nseat: wm:1\n---\n");
        let _ = std::fs::write(root.join("working-memory.s0.md"), "---\nseat: wm:1\n---\n");
        let _ = std::fs::write(root.join("working-memory.bad.md"), "no frontmatter\n");
        let wm = |own: &[&str]| {
            let own: Vec<String> = own.iter().map(|name| (*name).to_owned()).collect();
            let mut lines = Vec::new();
            let doc = adopt(&root, &own, "s1", &mut lines);
            (lines, doc.is_some())
        };
        assert_eq!(wm(&[]), (vec![(Marker::Wm, "missing".to_owned())], false));
        assert_eq!(wm(&["working-memory.s1.md"]), (vec![(Marker::Wm, "found file=working-memory.s1.md".to_owned())], true));
        assert_eq!(
            wm(&["working-memory.s0.md"]),
            (vec![(Marker::Wm, "candidate file=working-memory.s0.md sid=s0".to_owned())], true)
        );
        assert_eq!(
            wm(&["working-memory.s0.md", "working-memory.s1.md"]),
            (vec![(Marker::Wm, "ambiguous n=2".to_owned())], false)
        );
        assert_eq!(wm(&["working-memory.bad.md"]), (vec![(Marker::Wm, "unreadable file=working-memory.bad.md".to_owned())], false));
        let _ = std::fs::remove_dir_all(&root);
    }

    // flip-check: retroactive s2-07l.223
    /// 別席の未 consumed は 0 なら `[ORPHAN-NONE]` だけ・1 以上なら 1 件 1 行で `-NONE` を出さない。
    #[test]
    fn mutant_in_seat_rebrief_orphan_lines_emit_none_or_one_line_each() {
        let mut lines = Vec::new();
        orphan_lines(&[], &mut lines);
        assert_eq!(lines, [(Marker::OrphanNone, String::new())]);
        let mut lines = Vec::new();
        orphan_lines(&[("working-memory.x.md".to_owned(), "wm:2".to_owned())], &mut lines);
        assert_eq!(lines, [(Marker::OrphanWm, "file=working-memory.x.md seat=wm:2".to_owned())]);
    }

    /// 子 process を起こす（`sleep <秒>`・coreutils）。
    fn sleeping(seconds: &str) -> Option<std::process::Child> {
        Command::new("sleep").arg(seconds).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()
    }

    // flip-check: retroactive s2-07l.223
    /// `finish` は上限の**前**に終わった子の status を返し（上限なし・十分先の上限のどちらも待つ）、上限を過ぎても
    /// 終わらない子は `None`（待ち続けない・上限を `Instant` の等号や逆向きで読まない）。
    #[test]
    fn mutant_in_seat_rebrief_finish_waits_only_until_the_deadline() {
        let Some(mut quick) = sleeping("0.2") else {
            panic!("sleep を起こせない");
        };
        assert!(finish(&mut quick, None).is_some_and(|status| status.success()), "上限なしは終わるまで待つ");
        let Some(mut quick) = sleeping("0.2") else {
            panic!("sleep を起こせない");
        };
        let far = Instant::now().checked_add(Duration::from_secs(30));
        assert!(finish(&mut quick, far).is_some_and(|status| status.success()), "先の上限は終わるまで待つ");
        let Some(mut slow) = sleeping("3") else {
            panic!("sleep を起こせない");
        };
        let near = Instant::now().checked_add(Duration::from_millis(100));
        let started = Instant::now();
        assert_eq!(finish(&mut slow, near), None, "上限を過ぎたら子の終了を待たない");
        assert!(started.elapsed() < Duration::from_secs(2), "上限の後に待ち続けない");
        let _ = slow.kill();
        let _ = slow.wait();
    }
}
