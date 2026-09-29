//! 便の worktree の build と依存の置き場を、便が live でなくなった周に器が消す（設計 dispatcher.md §30・ADR-0081）。
//!
//! 消すのは名が [`NAMES`] の閉じた列に在り、追跡されている file を 1 つも持たない dir だけである。無視の規則も
//! host の個人設定の除外も読まず、`git clean` は撃たない（形 1）。live と測れない便・repo を解けない便・木の無い便は
//! 触らない（残す側に倒す・形 2）。撃つのは運転手の終端の周の 1 回で、置き場ごとの lock の中で撃つ（形 3・形 4）。

use super::cli::{int_row, live};
use super::retire::retired_path;
use super::{current, git_bytes, repo_of_run, worktree_path, DIR};
use crate::fleet::store::{self, LockPolicy};
use crate::rules::manifest::Manifest;
use crate::seat::{drafts_dir, seats_root};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 消す dir の名（器が対応する言語の build と依存の置き場・宣言順・形 1）: Rust・TypeScript・Python・React Native + Expo。
pub(super) const NAMES: &[&str] =
    &["target", "node_modules", ".venv", "__pycache__", ".mypy_cache", ".pytest_cache", ".ruff_cache", ".expo"];

/// 置き場の掃除の lock の名（`<state_dir>/pipe/` の直下・置き場ごとに 1 本・形 4）。
const LOCK: &str = "sweep.lock";

/// 握った掃除の lock（`Drop` で外す・外せない lock は次の周が所有者の生死で回収する）。
struct Held(PathBuf);

impl Drop for Held {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 席の起草の木の書きの線の rules 行の id（設計 dispatcher.md §33 形 5）。
const ROW_DRAFTS_STALE: &str = "seat.drafts_stale_h";

/// 1 周の集計（消した dir の数・dir を消した便の木の数・失敗した木の名）。
#[derive(Default)]
struct Totals {
    removed: usize,
    runs: usize,
    failed: Vec<String>,
}

impl Totals {
    /// 木 1 本の結果を足す。`name` は失敗した木の名（便 id か `<潰した target>/<木の dir 名>`）。
    fn add(&mut self, (count, broken): (usize, bool), name: &str) -> bool {
        self.removed = self.removed.saturating_add(count);
        if broken {
            self.failed.push(name.to_owned());
        }
        count > 0
    }
}

/// 起草の置き場の周の集計（dir を消した起草の木の数・`.git` を持たない写しの数・行を読めなかったか）。
struct Drafts {
    swept: usize,
    nogit: usize,
    no_rule: bool,
}

/// 置き場の live でない便の木と席の起草の木を掃き、stderr の 1 行を返す（形 2・形 5・§33）。
///
/// 行を返すのは dir を消した周・失敗した木が在る周・lock を取れない周・起草の行を読めなかった周だけで、
/// stdout・event・rc は変えない。pipe の dir の無い置き場は repo を解ける便を持たないので、dir を作らずに何も
/// しない。置き場を読めない周は便の段を測れないので便の木を 1 本も撃たない（測れないを「live でない」に
/// 読み替えない・C10）が、起草の木は便の段を読まないので掃く。
pub(super) fn sweep(state_dir: &Path, policy: LockPolicy, manifest: &Manifest) -> Option<String> {
    let lock = state_dir.join(DIR).join(LOCK);
    if !state_dir.join(DIR).is_dir() {
        return None;
    }
    // 生きている掃除の lock は古さで剥がさない（木が大きいと掃除は長い）。第 2 の lock の実装は作らない（C17）。
    if store::acquire_with(&lock, policy, store::Reclaim::DeadOnly).is_err() {
        return Some("sweep: skipped=lock".to_owned());
    }
    let _held = Held(lock);
    let mut totals = Totals::default();
    sweep_runs(state_dir, &mut totals);
    let drafts = sweep_drafts(state_dir, manifest, &mut totals);
    if totals.removed == 0 && totals.failed.is_empty() && !drafts.as_ref().is_some_and(|found| found.no_rule) {
        return None;
    }
    let named = if totals.failed.is_empty() { String::new() } else { format!(":{}", totals.failed.join(",")) };
    let tail = drafts.map_or_else(String::new, |found| {
        let swept = if found.no_rule { "no-rule".to_owned() } else { found.swept.to_string() };
        format!(" drafts={swept} nogit={}", found.nogit)
    });
    Some(format!("sweep: removed={} runs={} failed={}{named}{tail}", totals.removed, totals.runs, totals.failed.len()))
}

/// live でない便の木を掃く（置き場の replay を読めない周は 1 本も撃たない）。
fn sweep_runs(state_dir: &Path, totals: &mut Totals) {
    let Ok(state) = current(state_dir) else {
        return;
    };
    for run in state.runs.values().filter(|run| live(state_dir, &run.id, run.stage) == Some(false)) {
        let Some(tree) = tree_of(state_dir, &run.id) else {
            continue;
        };
        totals.runs = totals.runs.saturating_add(usize::from(totals.add(swept(&tree, None), &run.id)));
    }
}

/// 席の起草の木を掃く（起草の置き場が 1 つも無い周は `None`・行は木が 1 本以上在る周だけ読む・§33 形 4・形 5）。
fn sweep_drafts(state_dir: &Path, manifest: &Manifest, totals: &mut Totals) -> Option<Drafts> {
    let (trees, nogit) = drafts_of(state_dir)?;
    let mut found = Drafts { swept: 0, nogit, no_rule: false };
    if trees.is_empty() {
        return Some(found);
    }
    let Some(line) = stale_line(manifest) else {
        found.no_rule = true;
        return Some(found);
    };
    for (name, tree) in &trees {
        found.swept = found.swept.saturating_add(usize::from(totals.add(swept(tree, Some(line)), name)));
    }
    Some(found)
}

/// 書きの線（今 − rules 行の時間）。行を読めない周と引けない値は `None`（既定値へ倒さない・全部消す側へも倒さない）。
fn stale_line(manifest: &Manifest) -> Option<SystemTime> {
    let secs = int_row(manifest, ROW_DRAFTS_STALE).ok()?.checked_mul(3600)?;
    SystemTime::now().checked_sub(Duration::from_secs(secs))
}

/// 置き場の根の直下の席ごとの起草の置き場から、起草の木（`.git` を持つ dir）と `.git` を持たない写しの数を集める。
///
/// symlink は席の dir も起草の置き場も子も辿らない。起草の置き場が 1 つも無ければ `None`。木は名の順に並べる
/// （名は `<潰した target>/<木の dir 名>`）。
fn drafts_of(state_dir: &Path) -> Option<(Vec<(String, PathBuf)>, usize)> {
    let (mut trees, mut nogit, mut any) = (Vec::new(), 0_usize, false);
    for seat in std::fs::read_dir(seats_root(state_dir)).ok()?.flatten() {
        let name = seat.file_name();
        let drafts = drafts_dir(state_dir, &name.to_string_lossy());
        let real_dir = |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir());
        if !seat.file_type().is_ok_and(|kind| kind.is_dir()) || !real_dir(&drafts) {
            continue;
        }
        any = true;
        for child in std::fs::read_dir(&drafts).into_iter().flatten().flatten() {
            if !child.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let git = std::fs::symlink_metadata(child.path().join(".git"));
            if git.is_ok_and(|meta| meta.is_dir() || meta.is_file()) {
                let label = format!("{}/{}", name.to_string_lossy(), child.file_name().to_string_lossy());
                trees.push((label, child.path()));
            } else {
                nogit = nogit.saturating_add(1);
            }
        }
    }
    trees.sort();
    any.then_some((trees, nogit))
}

/// 便の木（元の場所と退役先のうち在る方・repo を解けない便と木の無い便は `None`）。
fn tree_of(state_dir: &Path, id: &str) -> Option<PathBuf> {
    let repo = repo_of_run(state_dir, id)?;
    [worktree_path(&repo, id), retired_path(&repo, id)].into_iter().find(|tree| tree.is_dir())
}

/// 木を `.git` に降りずに歩き、名が列に在り追跡されている file を持たない dir を消す（(消した数, 失敗したか)）。
///
/// 追跡の判定はその木の `git ls-files` の 1 回で、撃てない木は 1 つも消さずに失敗に数える。消した dir の下へは
/// 降りない（入れ子の `.git` を持っていても消す）。symlink は dir として辿らない（木の外を消さない）。
///
/// `line` を持つ木（席の起草の木・§33 形 3）は、その dir 自身と下の全 entry の mtime の最新が線より前の dir だけを消す。
/// 線以後の entry が 1 つでも在る dir と、mtime か dir を読めない dir は残す（後者は失敗に数える）。
fn swept(tree: &Path, line: Option<SystemTime>) -> (usize, bool) {
    let Some(listed) = git_bytes(tree, &["ls-files", "-z"]) else {
        return (0, true);
    };
    // 追跡されている path とその祖先の dir（木から相対）。gitlink の名そのものも残す側に数える。
    let tracked: BTreeSet<PathBuf> = listed
        .split(|byte| *byte == 0)
        .filter(|found| !found.is_empty())
        .flat_map(|found| Path::new(OsStr::from_bytes(found)).ancestors().map(Path::to_path_buf).collect::<Vec<_>>())
        .collect();
    let (mut removed, mut broken, mut pending) = (0_usize, false, vec![PathBuf::new()]);
    while let Some(rel) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(tree.join(&rel)) else {
            broken = true;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                broken = true;
                continue;
            };
            let name = entry.file_name();
            if name == ".git" || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let path = rel.join(&name);
            if !NAMES.iter().any(|found| OsStr::new(found) == name) || tracked.contains(&path) {
                pending.push(path);
            } else {
                match line.map_or(Some(true), |line| quiet_since(&entry.path(), line)) {
                    Some(false) => {}
                    Some(true) if std::fs::remove_dir_all(entry.path()).is_ok() => removed = removed.saturating_add(1),
                    _ => broken = true,
                }
            }
        }
    }
    (removed, broken)
}

/// dir 自身と下の全 entry の mtime がどれも線より前か（線以後を 1 つ見つけたら打ち切って `Some(false)`・
/// 読めない entry が在れば `None`）。symlink は辿らずに symlink そのものの mtime を読む。
fn quiet_since(dir: &Path, line: SystemTime) -> Option<bool> {
    let older = |path: &Path| std::fs::symlink_metadata(path).and_then(|meta| meta.modified()).ok().map(|when| when < line);
    if !older(dir)? {
        return Some(false);
    }
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(&current).ok()? {
            let entry = entry.ok()?;
            let path = entry.path();
            if !older(&path)? {
                return Some(false);
            }
            if entry.file_type().ok()?.is_dir() {
                pending.push(path);
            }
        }
    }
    Some(true)
}
