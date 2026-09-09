//! `pre-tool-use` の write-set guard（設計 §5・FR20・憲法 C16）。
//!
//! **極性は in-loop / fail-closed**（C11）: policy file が在るのに読めない・空なら
//! 通さず deny する。policy file が無いときだけ不活性（開発 session が main を直接
//! 編集する場面）。**env は使わない**（C2.2・ADR-0004 §2.4）——policy は worktree の
//! git dir の私有 dir に在り、tracked 面を汚さない。
//!
//! deny は時間切れに頼らず返す（NFR5）。timeout 到達は Claude Code 側で「判定の消失」
//! ＝fail-open なので、判定は必ず timeout の内側で終える。

use crate::name::NAME;
use std::path::{Component, Path, PathBuf};

/// guard が見る tool の名前。ここに無い tool は通す（`Bash` を含む）。
const GUARDED: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// policy file の名前。
const POLICY_FILE: &str = "write-set.txt";

/// 1 回の編集の判定。**bool で持たない**（憲法 C11）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// policy file が無い＝guard は働かない。
    Inactive,
    /// 通す。
    Allow,
    /// 止める。中身は stderr へ出す 1 行。
    Deny(String),
}

/// policy file の path（`<git-dir>/<NAME>/write-set.txt`）。
pub fn policy_path(git_dir: &Path) -> PathBuf {
    git_dir.join(NAME).join(POLICY_FILE)
}

/// policy が読めないときの 1 行。**在るのに読めない・空はどちらも deny**。
fn unreadable() -> String {
    format!("{NAME}: deny policy unreadable（C16）")
}

/// write-set の外を触ったときの 1 行。
fn outside(path: &str) -> String {
    format!("{NAME}: deny {path} は契約 write-set の外（C16）")
}

/// 1 回の編集を判定する。
///
/// `path` は payload の `file_path` か `notebook_path`。guard が見る tool なのに
/// path を読めないときは **deny** する（fail-closed の極性を path 側でも保つ）。
pub fn decide(root: &Path, git_dir: &Path, tool: &str, path: Option<&str>) -> Decision {
    let policy = policy_path(git_dir);
    if !policy.exists() {
        return Decision::Inactive;
    }
    let Ok(text) = std::fs::read_to_string(&policy) else {
        return Decision::Deny(unreadable());
    };
    let allowed = entries(&text);
    if allowed.is_empty() {
        return Decision::Deny(unreadable());
    }
    if !GUARDED.contains(&tool) {
        return Decision::Allow;
    }
    let Some(target) = path else {
        return Decision::Deny(format!("{NAME}: deny 編集先の path を読めない（C16）"));
    };
    match relative_to(root, target) {
        None => Decision::Deny(outside(target)),
        Some(rel) if is_allowed(&allowed, &rel) => Decision::Allow,
        Some(rel) => Decision::Deny(outside(&rel.display().to_string())),
    }
}

/// policy file の行を allowlist へ写す。空行は項目に数えない（glob は無い）。
fn entries(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 編集先を repo 相対へ正規化する。root の外・`..` で外れるものは `None`。
///
/// 実 file の存在に依らず**字句だけ**で解く（まだ無い file への Write も判定する）。
fn relative_to(root: &Path, target: &str) -> Option<PathBuf> {
    let raw = Path::new(target);
    let rel = if raw.is_absolute() {
        raw.strip_prefix(root).ok()?
    } else {
        raw
    };
    let mut out = PathBuf::new();
    for part in rel.components() {
        match part {
            Component::Normal(name) => out.push(name),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// repo 相対 path が allowlist の内側か。末尾 `/` の項目は配下全部を許す。
fn is_allowed(allowed: &[String], rel: &Path) -> bool {
    let text = rel.to_string_lossy();
    allowed.iter().any(|entry| match entry.strip_suffix('/') {
        Some(dir) => text.starts_with(&format!("{dir}/")),
        None => *entry == text,
    })
}
