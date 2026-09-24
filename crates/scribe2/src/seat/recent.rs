//! 復帰の DATA（設計 seat-roles.md §21・FR42 / FR19・`s2-07l.489`）: SessionStart が §5 の指示文の**後ろ**に、
//! 台帳の仕掛かり中と直近更新の bead・anchor の git の直近・dirty な worktree を **typed な行**で出す。
//!
//! 行は規範を持たない事実の行（穴も pointer も無い＝雛形の行の規律と xtask の検査の外・N2 に当たらない）で、
//! 行頭の marker（[`Kind`]）が種類を決める。**0 件と測れないを分ける**（C10）: 読めた上で 0 件は
//! `[RECENT-NONE]`、測れなかった種類だけ `[RECENT-UNMEASURED]`（理由は閉じた enum [`Unmeasured`]）で、
//! 他の種類と §5 の指示文は出す（fail-open・[`POLARITY`]）。
//!
//! 台帳は §5 と**同じ 1 回**の `bd --readonly list --json` の出力（`seat/ledger.rs` が読む）をこの module が
//! [`beads_of`] で読み直す——`Issue`（列の順序が読む型）は広げない（構築 site が歯に多数在る）。git は anchor で
//! `git` の読みの口だけを撃つ（network に出ない＝fetch しない・origin との差は手元の remote 追跡 ref）。
//! 時刻の窓は呼び手が渡す現在時刻で測る（壁時計をここで読まない＝歯が時刻を固定できる）。

use crate::fleet::cli::format_utc;
use crate::fleet::json_tree::{self, Tree};
use crate::invocation::Invocation;
use crate::polarity::{OnFailure, Polarity, Timing};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// この境界の極性（[`Unmeasured`]）: 測れない種類はその種類だけ `[RECENT-UNMEASURED]` を出し、他の種類と
/// §5 の指示文は出す（読みの失敗で注入全体を黙らせない・fail-open）。行為を止める判定ではないので Guard では
/// ない（計測の境界＝`polarity::NOT_A_GUARD` の 1 行・`fleet::UnmeasuredReason` と同型・設計 polarity.md §3）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailOpen,
};

/// 測れなかった理由（**境界の enum**・[`POLARITY`]・行の `reason=` の字面は [`Unmeasured::as_str`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmeasured {
    /// 台帳を読めない（起動できない・rc 非 0・JSON 不能・待ち上限の行が無い）。
    LedgerUnreadable,
    /// 台帳の読みが待ち上限を超えた。
    LedgerTimeout,
    /// `git` を起動できない。
    GitUnavailable,
    /// anchor が git の repo でない。
    NotARepo,
}

/// [`Unmeasured`] の全 variant（宣言順）。
pub const UNMEASURED: &[Unmeasured] = &[
    Unmeasured::LedgerUnreadable,
    Unmeasured::LedgerTimeout,
    Unmeasured::GitUnavailable,
    Unmeasured::NotARepo,
];

impl Unmeasured {
    /// 行の `reason=` に出す字面（kebab・閉じた列）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LedgerUnreadable => "ledger-unreadable",
            Self::LedgerTimeout => "ledger-timeout",
            Self::GitUnavailable => "git-unavailable",
            Self::NotARepo => "not-a-repo",
        }
    }
}

/// 行の種類（行頭の marker で弁別・**宣言順が出す順**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// status が `in_progress` の bead の全件。
    Wip,
    /// 直近 [`WINDOW_SECS`] に更新された bead（新しい順・上位 [`BEAD_LIMIT`]・Wip に出た id は除く）。
    Bead,
    /// head・branch・ahead / behind の 1 行。
    Git,
    /// 直近の commit（新しい順・上位 [`COMMIT_LIMIT`]）。
    Commit,
    /// 未 commit の変更を持つ worktree（anchor を含む）。
    Dirty,
}

/// [`Kind`] の全 variant（宣言順＝出す順）。
pub const KINDS: &[Kind] = &[Kind::Wip, Kind::Bead, Kind::Git, Kind::Commit, Kind::Dirty];

impl Kind {
    /// `kind=` に出す字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wip => "wip",
            Self::Bead => "bead",
            Self::Git => "git",
            Self::Commit => "commit",
            Self::Dirty => "dirty",
        }
    }

    /// 行頭の marker。
    pub fn marker(self) -> &'static str {
        match self {
            Self::Wip => "[RECENT-WIP]",
            Self::Bead => "[RECENT-BEAD]",
            Self::Git => "[RECENT-GIT]",
            Self::Commit => "[RECENT-COMMIT]",
            Self::Dirty => "[RECENT-DIRTY]",
        }
    }
}

/// 上限で切った周の末尾の行の marker。
pub const MARKER_CUT: &str = "[RECENT-CUT]";
/// 読めた上で 0 件の行の marker。
pub const MARKER_NONE: &str = "[RECENT-NONE]";
/// 測れなかった種類の行の marker。
pub const MARKER_UNMEASURED: &str = "[RECENT-UNMEASURED]";

/// 直近更新の窓（秒・表示の幅であって閾値ではない＝rules 行を足さない）。
pub const WINDOW_SECS: u64 = 24 * 60 * 60;
/// `[RECENT-BEAD]` の上限（本）。
pub const BEAD_LIMIT: usize = 10;
/// `[RECENT-COMMIT]` の上限（本）。
pub const COMMIT_LIMIT: usize = 10;
/// `[RECENT-DIRTY]` のために `status` を撃つ worktree の上限（本・anchor が先頭・残りは HEAD の commit が新しい順）。
pub const DIRTY_SCAN_LIMIT: usize = 20;
/// 題と subject を切り詰める幅（文字）。
pub const TITLE_WIDTH: usize = 80;
/// 時刻を読めない・上流が無い周の字面（数に化けさせない・C10）。
const ABSENT: &str = "-";

/// 台帳の 1 件（**この module が読む key だけ**・`ledger::Issue` とは別の型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bead {
    /// bead id。
    pub id: String,
    /// status の字面。
    pub status: String,
    /// 題（無ければ空）。
    pub title: String,
    /// 更新時刻（`updated_at` を UNIX 秒に読んだ値・読めない・無い周は `None`）。
    pub updated: Option<u64>,
}

/// 台帳の JSON（`bd list --json` の配列）から id / status / title / updated_at だけを読む。配列でない・要素に
/// `id` / `status` の文字列が無い → `None`（`ledger::issues_of` と同じ 2 key の必須・欠けた要素を読み飛ばさない）。
pub fn beads_of(text: &str) -> Option<Vec<Bead>> {
    let tree = json_tree::parse(text).ok()?;
    tree.as_array()?
        .iter()
        .map(|node| {
            let text_of = |key: &str| node.get(key).and_then(Tree::as_str).map(str::to_owned);
            Some(Bead {
                id: text_of("id")?,
                status: text_of("status")?,
                title: text_of("title").unwrap_or_default(),
                updated: node.get("updated_at").and_then(Tree::as_str).and_then(epoch_of_rfc3339),
            })
        })
        .collect()
}

/// RFC 3339 の時刻を UNIX 秒にする（`YYYY-MM-DDTHH:MM:SS` + 任意の小数秒 + `Z` か `±HH:MM`）。
/// `bd` は Go の `time.Time` の形（RFC3339Nano・offset 付き）で書くので、`fleet::epoch_of` の
/// `Z` ちょうどの形へ畳んでから読む。それ以外の形は `None`（0 秒に化けさせない）。
fn epoch_of_rfc3339(text: &str) -> Option<u64> {
    let (head, rest) = text.split_at_checked(19)?;
    let rest = match rest.strip_prefix('.') {
        Some(fraction) => fraction.trim_start_matches(|c: char| c.is_ascii_digit()),
        None => rest,
    };
    let local = crate::fleet::epoch_of(&format!("{head}Z"))?;
    if rest == "Z" {
        return Some(local);
    }
    let (sign, offset) = rest.split_at_checked(1)?;
    let (hours, minutes) = offset.split_once(':')?;
    if hours.len() != 2 || minutes.len() != 2 {
        return None;
    }
    let hours = hours.parse::<u64>().ok()?.checked_mul(3_600)?;
    let seconds = hours.checked_add(minutes.parse::<u64>().ok()?.checked_mul(60)?)?;
    match sign {
        "+" => local.checked_sub(seconds),
        "-" => local.checked_add(seconds),
        _ => None,
    }
}

/// 自由文を 1 行に畳み（改行と制御文字を空白へ）幅で切る（切ったら末尾に `…`）。行頭の marker を題が
/// 偽装しても行の種類は行頭の 1 語で決まる（題は 3 語目以降にしか現れない）。
pub fn fold(text: &str) -> String {
    let flat: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let trimmed = flat.trim();
    if trimmed.chars().count() <= TITLE_WIDTH {
        return trimmed.to_owned();
    }
    let mut cut: String = trimmed.chars().take(TITLE_WIDTH).collect();
    cut.push('…');
    cut
}

/// 時刻の字面（読めた周は UTC の `YYYY-MM-DDTHH:MM:SSZ`・読めない周は [`ABSENT`]）。
fn time_of(secs: Option<u64>) -> String {
    secs.map_or_else(|| ABSENT.to_owned(), format_utc)
}

/// 読めた上で 0 件の 1 行。
fn none_line(kind: Kind) -> String {
    format!("{MARKER_NONE} kind={}", kind.as_str())
}

/// 測れなかった種類の 1 行。
fn unmeasured_line(kind: Kind, reason: Unmeasured) -> String {
    format!("{MARKER_UNMEASURED} kind={} reason={}", kind.as_str(), reason.as_str())
}

/// 上限で切った周の末尾の 1 行。
fn cut_line(kind: Kind, shown: usize, total: usize) -> String {
    format!("{MARKER_CUT} kind={} shown={shown} total={total}", kind.as_str())
}

/// 1 種類の行を出す（0 件は `[RECENT-NONE]`・上限を超えた周は `[RECENT-CUT]`・`limit` 無しは全件）。
fn section(kind: Kind, items: Vec<String>, limit: Option<usize>) -> Vec<String> {
    if items.is_empty() {
        return vec![none_line(kind)];
    }
    let total = items.len();
    let shown = limit.map_or(total, |cap| cap.min(total));
    let mut lines: Vec<String> = items.into_iter().take(shown).collect();
    if shown < total {
        lines.push(cut_line(kind, shown, total));
    }
    lines
}

/// 台帳の 2 種類（Wip / Bead）。読めない周は 2 種類とも `[RECENT-UNMEASURED]`。
pub fn ledger_lines(beads: Result<&[Bead], Unmeasured>, now: u64) -> Vec<String> {
    let beads = match beads {
        Ok(found) => found,
        Err(reason) => return vec![unmeasured_line(Kind::Wip, reason), unmeasured_line(Kind::Bead, reason)],
    };
    let wip: Vec<&Bead> = beads.iter().filter(|bead| bead.status == "in_progress").collect();
    let since = now.saturating_sub(WINDOW_SECS);
    let mut recent: Vec<&Bead> = beads
        .iter()
        .filter(|bead| bead.updated.is_some_and(|at| at >= since && at <= now))
        .filter(|bead| !wip.iter().any(|found| found.id == bead.id))
        .collect();
    recent.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
    let wip_lines = wip
        .iter()
        .map(|bead| format!("{} {} {} {}", Kind::Wip.marker(), bead.id, time_of(bead.updated), fold(&bead.title)))
        .map(|line| line.trim_end().to_owned())
        .collect();
    let bead_lines = recent
        .iter()
        .map(|bead| {
            let (marker, time) = (Kind::Bead.marker(), time_of(bead.updated));
            format!("{marker} {} {} {time} {}", bead.id, bead.status, fold(&bead.title))
        })
        .map(|line| line.trim_end().to_owned())
        .collect();
    let mut lines = section(Kind::Wip, wip_lines, None);
    lines.extend(section(Kind::Bead, bead_lines, Some(BEAD_LIMIT)));
    lines
}

/// git を anchor で 1 回撃つ。起動できない → `Err(GitUnavailable)`・rc 非 0 → `Ok(None)`・rc 0 → stdout。
fn git(anchor: &Path, args: &[&str]) -> Result<Option<String>, Unmeasured> {
    let output = Invocation::new("git")
        .arg("-C")
        .arg(anchor)
        .args(args)
        .output()
        .map_err(|_| Unmeasured::GitUnavailable)?;
    Ok(output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
}

/// git の 3 種類（Git / Commit / Dirty）。`git` を起動できない・anchor が repo でない周は 3 種類とも
/// `[RECENT-UNMEASURED]`（理由は `git-unavailable` / `not-a-repo`）。
pub fn git_lines(anchor: &Path) -> Vec<String> {
    // git の 3 種類（`Kind` の部分集合は const にしない・`enum-slices` の全数の対を崩さない）。
    let git_kinds = [Kind::Git, Kind::Commit, Kind::Dirty];
    match git_sections(anchor) {
        Ok(lines) => lines,
        Err(reason) => git_kinds.iter().map(|kind| unmeasured_line(*kind, reason)).collect(),
    }
}

/// git の 3 種類の本体（repo でない周は `Err(NotARepo)`）。
fn git_sections(anchor: &Path) -> Result<Vec<String>, Unmeasured> {
    let inside = git(anchor, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.is_none_or(|found| found.trim() != "true") {
        return Err(Unmeasured::NotARepo);
    }
    let word = |found: Option<String>| found.map_or_else(|| ABSENT.to_owned(), |text| text.trim().to_owned());
    let head = word(git(anchor, &["rev-parse", "--short", "HEAD"])?);
    let branch = word(git(anchor, &["rev-parse", "--abbrev-ref", "HEAD"])?);
    let (ahead, behind) = ahead_behind(anchor)?;
    let mut lines = vec![format!("{} head={head} branch={branch} ahead={ahead} behind={behind}", Kind::Git.marker())];
    lines.extend(commit_section(anchor)?);
    lines.extend(dirty_section(anchor)?);
    Ok(lines)
}

/// 上流（remote 追跡 ref）との差 `(ahead, behind)`。上流が無い周は両方 [`ABSENT`]（0 と書かない）。
fn ahead_behind(anchor: &Path) -> Result<(String, String), Unmeasured> {
    let counted = git(anchor, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])?;
    let pair = counted.as_deref().and_then(|text| {
        let mut parts = text.split_whitespace();
        Some((parts.next()?.to_owned(), parts.next()?.to_owned()))
    });
    Ok(pair.unwrap_or_else(|| (ABSENT.to_owned(), ABSENT.to_owned())))
}

/// `[RECENT-COMMIT]` の区間（新しい順・上限 [`COMMIT_LIMIT`]・総数は `rev-list --count`）。
fn commit_section(anchor: &Path) -> Result<Vec<String>, Unmeasured> {
    let limit = COMMIT_LIMIT.to_string();
    let log = git(anchor, &["log", "-n", &limit, "--format=%h%x09%s"])?.unwrap_or_default();
    let mut items: Vec<String> = log
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(sha, subject)| format!("{} {sha} {}", Kind::Commit.marker(), fold(subject)).trim_end().to_owned())
        .collect();
    let total = git(anchor, &["rev-list", "--count", "HEAD"])?
        .and_then(|text| text.trim().parse::<usize>().ok())
        .unwrap_or(items.len());
    if items.is_empty() {
        return Ok(vec![none_line(Kind::Commit)]);
    }
    if total > items.len() {
        let shown = items.len();
        items.push(cut_line(Kind::Commit, shown, total));
    }
    Ok(items)
}

/// `[RECENT-DIRTY]` の区間: worktree（anchor が先頭・残りは HEAD の commit が新しい順）の上位 [`DIRTY_SCAN_LIMIT`]
/// 本を `status --porcelain`（untracked も数える）で測り、dirty なものを出す。worktree が上限より多い周は末尾に
/// `[RECENT-CUT] kind=dirty shown=<測った本数> total=<worktree の本数>`（0 件の `[RECENT-NONE]` の後ろにも付く）＝
/// 便ごとの worktree が数百本溜まった repo で SessionStart が hook の時間予算を食い潰さない。
/// path は anchor の repo 相対（anchor 自身は `.`・外は絶対 path）。
fn dirty_section(anchor: &Path) -> Result<Vec<String>, Unmeasured> {
    let worktrees = worktrees_by_recency(anchor)?;
    let total = worktrees.len();
    let mut lines = Vec::new();
    for path in worktrees.iter().take(DIRTY_SCAN_LIMIT) {
        let dirty = git(path, &["status", "--porcelain"])?.is_some_and(|status| !status.trim().is_empty());
        if dirty {
            lines.push(format!("{} {}", Kind::Dirty.marker(), relative_of(anchor, path).display()));
        }
    }
    if lines.is_empty() {
        lines.push(none_line(Kind::Dirty));
    }
    if total > DIRTY_SCAN_LIMIT {
        lines.push(cut_line(Kind::Dirty, DIRTY_SCAN_LIMIT, total));
    }
    Ok(lines)
}

/// worktree の path を測る順に並べる: `git worktree list --porcelain` の先頭（anchor＝main の worktree）は先頭のまま、
/// 残りは HEAD の commit 時刻（`rev-list --no-walk` の 1 回で全 HEAD を引く）が新しい順。時刻を引けない worktree は
/// 列挙の順のまま末尾側。
fn worktrees_by_recency(anchor: &Path) -> Result<Vec<PathBuf>, Unmeasured> {
    let listed = git(anchor, &["worktree", "list", "--porcelain"])?.unwrap_or_default();
    let mut entries: Vec<(PathBuf, Option<String>)> = Vec::new();
    for line in listed.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            entries.push((PathBuf::from(path), None));
        } else if let Some(sha) = line.strip_prefix("HEAD ") {
            // 未生の HEAD（全部 0）は時刻を引けない＝列挙の順のまま。
            let sha = sha.trim();
            if let Some(last) = entries.last_mut() {
                last.1 = (sha.len() == 40 && !sha.bytes().all(|b| b == b'0')).then(|| sha.to_owned());
            }
        }
    }
    let shas: Vec<&str> = entries.iter().filter_map(|(_, sha)| sha.as_deref()).collect();
    let times = commit_times(anchor, &shas)?;
    let mut rest: Vec<(usize, PathBuf, Option<u64>)> = entries
        .iter()
        .enumerate()
        .skip(1)
        .map(|(at, (path, sha))| (at, path.clone(), sha.as_deref().and_then(|found| times.get(found).copied())))
        .collect();
    rest.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    let mut ordered: Vec<PathBuf> = entries.first().map(|(path, _)| path.clone()).into_iter().collect();
    ordered.extend(rest.into_iter().map(|(_, path, _)| path));
    Ok(ordered)
}

/// commit の時刻（UNIX 秒）を sha ごとに引く（`rev-list --no-walk --format=%H%x09%ct` の 1 回・sha が 0 本なら撃たない）。
/// 引けない周（rc 非 0・形が読めない行）は空＝呼び手は列挙の順を保つ。
fn commit_times(anchor: &Path, shas: &[&str]) -> Result<BTreeMap<String, u64>, Unmeasured> {
    if shas.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut args = vec!["rev-list", "--no-walk", "--format=%H%x09%ct"];
    args.extend_from_slice(shas);
    let text = git(anchor, &args)?.unwrap_or_default();
    Ok(text
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter_map(|(sha, secs)| secs.trim().parse::<u64>().ok().map(|at| (sha.to_owned(), at)))
        .collect())
}

/// worktree の path を anchor の相対にする（anchor 自身は `.`・anchor の外は絶対のまま）。
fn relative_of(anchor: &Path, path: &Path) -> PathBuf {
    let base = anchor.canonicalize().unwrap_or_else(|_| anchor.to_path_buf());
    let real = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    match real.strip_prefix(&base) {
        Ok(rest) if rest.as_os_str().is_empty() => PathBuf::from("."),
        Ok(rest) => rest.to_path_buf(),
        Err(_) => real,
    }
}

/// 区間の全行（台帳の 2 種類 → git の 3 種類・[`KINDS`] の宣言順）。
pub fn render(beads: Result<&[Bead], Unmeasured>, anchor: &Path, now: u64) -> Vec<String> {
    let mut lines = ledger_lines(beads, now);
    lines.extend(git_lines(anchor));
    lines
}

#[cfg(test)]
mod tests {
    use super::{git, Unmeasured};
    use crate::pipe::fixture::{exited, Call, Stub};
    use std::path::Path;

    /// recent の git の読みは起動の記述を通る（設計 core-boundary.md §9 行 f）: program は git・引数は `-C <anchor>`
    /// の後に呼び手の列。起動の失敗は `Err(GitUnavailable)`・rc 非 0 は `Ok(None)`・rc 0 は stdout をそのまま。
    #[test]
    fn invocation_seat_recent_git_passes_the_repo_args() {
        let stub = Stub::install(|call| match call.args.last().map(String::as_str) {
            Some("ok") => exited(0, b" true \n"),
            Some("fail") => exited(128, b"out\n"),
            _ => Err(std::io::Error::other("gone")),
        });
        let anchor = Path::new("/nonexistent-invocation-seat-recent");
        assert_eq!(git(anchor, &["rev-parse", "ok"]), Ok(Some(" true \n".to_owned())), "rc 0 は stdout のまま");
        assert_eq!(git(anchor, &["fail"]), Ok(None), "rc 非 0");
        assert_eq!(git(anchor, &["gone"]), Err(Unmeasured::GitUnavailable), "起動の失敗");
        let call = |tail: &[&str]| Call {
            program: "git".to_owned(),
            args: ["-C", "/nonexistent-invocation-seat-recent"].iter().chain(tail).map(|arg| (*arg).to_owned()).collect(),
            cwd: None,
            envs: Vec::new(),
        };
        assert_eq!(stub.calls(), [call(&["rev-parse", "ok"]), call(&["fail"]), call(&["gone"])], "git の program と引数");
    }
}
