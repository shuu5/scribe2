//! `pre-tool-use` の write-set guard（設計 §5・FR20・憲法 C16）。
//!
//! **極性は in-loop / fail-closed**（C11）: policy file が在るのに読めない・空なら
//! 通さず deny する。policy file が無いときだけ不活性（開発 session が main を直接
//! 編集する場面）。**env は使わない**（C2.2・ADR-0004 §2.4）——policy は worktree の
//! git dir の私有 dir に在り、tracked 面を汚さない。
//!
//! deny は時間切れに頼らず返す（NFR5）。timeout 到達は Claude Code 側で「判定の消失」
//! ＝fail-open なので、判定は必ず timeout の内側で終える。
//!
//! path は **2 段**で見る: `..` を字句で畳む段（まだ無い file も判定できる）と、実体を
//! 解く段（symlink を経由した `..` は字句では見抜けない）。字句が root の外を指したら
//! deny し、**allowlist は実体で解いた名前で当てる**——repo の内側で閉じる symlink は
//! root を一歩も出ないので、字句の名前で当てると write-set の外へ書けてしまう。

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

/// repo の外を指したときの 1 行。**write-set の外**とは理由が違うので字面を分ける。
fn escaped(path: &str) -> String {
    format!("{NAME}: deny {path} は repo の外を指す（C16）")
}

/// write-set の外を触ったときの 1 行。
fn outside(path: &str) -> String {
    format!("{NAME}: deny {path} は契約 write-set の外（C16）")
}

/// 1 回の編集を判定する。
///
/// `cwd` は payload が言う作業 dir で、**相対 `file_path` の基準**である（tool 側が
/// その基準で解くので、guard も同じ基準で解かないと別の file を判定する）。
/// `path` は payload の `file_path` か `notebook_path`。guard が見る tool なのに path を
/// 読めないときは deny する（fail-closed の極性を path 側でも保つ）。
pub fn decide(root: &Path, cwd: &Path, git_dir: &Path, tool: &str, path: Option<&str>) -> Decision {
    // 「`Bash` と他 tool は rc 0・0 byte」は policy の状態に依らない（設計 §5）。
    // policy を先に読むと、policy が壊れた周に `Bash` まで deny してしまう。
    if !GUARDED.contains(&tool) {
        return Decision::Allow;
    }
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
    let Some(target) = path else {
        return Decision::Deny(format!("{NAME}: deny 編集先の path を読めない（C16）"));
    };
    let absolute = absolute_of(cwd, target);
    // 字句の段。root の外へ出る形はここで落とす（実体が root の内側を指す symlink でも、
    // 字句で外に居る path は通さない＝fail-closed の極性を保つ）。
    if relative_to(root, &absolute).is_none() {
        return Decision::Deny(escaped(target));
    }
    // 実体の段。allowlist は**実体で解いた名前**で当てる。
    match resolved_relative(root, &absolute) {
        None => Decision::Deny(escaped(target)),
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

/// 編集先を絶対 path にする。相対 path の基準は payload の `cwd` である。
fn absolute_of(cwd: &Path, target: &str) -> PathBuf {
    let raw = Path::new(target);
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    }
}

/// 絶対 path を repo 相対へ**字句で**畳む。root の外・`..` で外れるものは `None`。
///
/// 実 file の存在に依らないので、まだ無い file への Write も判定できる。
fn relative_to(root: &Path, absolute: &Path) -> Option<PathBuf> {
    let rel = absolute.strip_prefix(root).ok()?;
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

/// 実体で解いた repo 相対 path。実体が root の外に出るなら `None`。
///
/// 字句だけでは `docs/<symlink>/../evil.rs` が `docs/evil.rs` に畳まれ、実際の書き先が
/// repo の外でも通ってしまう。repo の**内側**で閉じる symlink も同じで、
/// `docs/<link>/evil.rs` の実体が `src/evil.rs` なら root は一歩も出ない——**allowlist を
/// 実体の名前で当てないと** write-set の外へ書ける。**前から 1 段ずつ**積み、存在する段
/// では symlink を解く。存在しない段はそのまま字句で積む（まだ無い file への Write も
/// 判定するため。その段に symlink は在り得ないので、字句の結果を信じてよい）。
fn resolved_relative(root: &Path, absolute: &Path) -> Option<PathBuf> {
    let real_root = root.canonicalize().ok()?;
    let mut real = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::RootDir | Component::Prefix(_) => real.push(part.as_os_str()),
            Component::CurDir => {}
            Component::Normal(name) => {
                real.push(name);
                if let Ok(found) = real.canonicalize() {
                    real = found;
                }
            }
            Component::ParentDir => {
                if !real.pop() {
                    return None;
                }
            }
        }
    }
    real.strip_prefix(&real_root).ok().map(Path::to_path_buf)
}

/// repo 相対 path が allowlist の内側か。末尾 `/` の項目は配下全部を許す。
fn is_allowed(allowed: &[String], rel: &Path) -> bool {
    let text = rel.to_string_lossy();
    allowed.iter().any(|entry| match entry.strip_suffix('/') {
        Some(dir) => text.starts_with(&format!("{dir}/")),
        None => *entry == text,
    })
}
