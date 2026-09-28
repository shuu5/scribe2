//! 便の worktree の build と依存の置き場を、便が live でなくなった周に器が消す（設計 dispatcher.md §30・ADR-0081）。
//!
//! 消すのは名が [`NAMES`] の閉じた列に在り、追跡されている file を 1 つも持たない dir だけである。無視の規則も
//! host の個人設定の除外も読まず、`git clean` は撃たない（形 1）。live と測れない便・repo を解けない便・木の無い便は
//! 触らない（残す側に倒す・形 2）。撃つのは運転手の終端の周の 1 回で、置き場ごとの lock の中で撃つ（形 3・形 4）。

use super::cli::live;
use super::retire::retired_path;
use super::{current, git_bytes, repo_of_run, worktree_path, DIR};
use crate::fleet::store::{self, LockPolicy};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

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

/// 置き場の live でない便の木を掃き、stderr の 1 行を返す（形 2・形 5）。
///
/// 行を返すのは dir を消した周・失敗した木が在る周・lock を取れない周だけで、stdout・event・rc は変えない。
/// pipe の dir の無い置き場は repo を解ける便を持たないので、dir を作らずに何もしない。置き場を読めない周も
/// 便の段を測れないので 1 本も撃たない（測れないを「live でない」に読み替えない・C10）。
pub(super) fn sweep(state_dir: &Path, policy: LockPolicy) -> Option<String> {
    let lock = state_dir.join(DIR).join(LOCK);
    if !state_dir.join(DIR).is_dir() {
        return None;
    }
    // 生きている掃除の lock は古さで剥がさない（木が大きいと掃除は長い）。第 2 の lock の実装は作らない（C17）。
    if store::acquire_with(&lock, policy, store::Reclaim::DeadOnly).is_err() {
        return Some("sweep: skipped=lock".to_owned());
    }
    let _held = Held(lock);
    let state = current(state_dir).ok()?;
    let (mut removed, mut runs, mut failed) = (0_usize, 0_usize, Vec::new());
    for run in state.runs.values().filter(|run| live(state_dir, &run.id, run.stage) == Some(false)) {
        let Some(tree) = tree_of(state_dir, &run.id) else {
            continue;
        };
        let (count, broken) = swept(&tree);
        removed = removed.saturating_add(count);
        runs = runs.saturating_add(usize::from(count > 0));
        if broken {
            failed.push(run.id.as_str());
        }
    }
    if removed == 0 && failed.is_empty() {
        return None;
    }
    let named = if failed.is_empty() { String::new() } else { format!(":{}", failed.join(",")) };
    Some(format!("sweep: removed={removed} runs={runs} failed={}{named}", failed.len()))
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
fn swept(tree: &Path) -> (usize, bool) {
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
            } else if std::fs::remove_dir_all(entry.path()).is_ok() {
                removed = removed.saturating_add(1);
            } else {
                broken = true;
            }
        }
    }
    (removed, broken)
}
