//! build 元 commit を binary に焼く build script（設計 consumer-sync.md §2・ADR-0028 §2.1）。
//!
//! `git rev-parse --short=12 HEAD` と `git status --porcelain --untracked-files=no` を撃ち、
//! `SCRIBE2_BUILD_COMMIT=<sha12>[+dirty]` を compile time の env として出す。実行時は
//! `env!` で読むだけ（実行時に env を読まない・憲法 C2.2）。git が無い・repo でない・
//! rc ≠ 0・出力が sha の形でない周は `unknown`（測れない周を成功に倒さない・C10）。
//! build は落とさない（panic しない・依存は std だけ）。
//!
//! env 名の接頭辞 `SCRIBE2_` は core の `ENV_PREFIX` と同じ綴りだが、build script は
//! core の const を import できないので literal で書く（xtask の `name-literal` の母集団は
//! `src/` だけで、この file は数えられない＝限界）。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 焼く env の名前。
const ENV_NAME: &str = "SCRIBE2_BUILD_COMMIT";

/// 測れない周の値。
const UNKNOWN: &str = "unknown";

/// 短縮 sha の桁数。
const SHA_LEN: usize = 12;

/// build script の出力層。stdout へ書くのはこの関数だけである（cargo への指示行は stdout で渡す
/// 規約・clippy の `print_stdout` は build script を母集団に数えない）。
fn emit(line: &str) {
    println!("{line}");
}

/// `git -C <dir> <args>` を撃ち、rc 0 なら stdout を返す。git が無い・rc ≠ 0 は `None`。
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// 短縮 sha の形（`[0-9a-f]{12}`）か。
fn is_sha12(text: &str) -> bool {
    text.len() == SHA_LEN && text.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
}

/// HEAD の短縮 sha と作業木の汚れから焼く値を組む。測れない周は `unknown`。
fn build_commit(dir: &Path) -> String {
    let Some(head) = git(dir, &["rev-parse", "--short=12", "HEAD"]) else {
        return UNKNOWN.to_owned();
    };
    let sha = head.trim();
    if !is_sha12(sha) {
        return UNKNOWN.to_owned();
    }
    match git(dir, &["status", "--porcelain", "--untracked-files=no"]) {
        Some(status) if status.trim().is_empty() => sha.to_owned(),
        Some(_) => format!("{sha}+dirty"),
        None => UNKNOWN.to_owned(),
    }
}

/// 再 build の引き金にする path の列（`<root>/.git/HEAD` と `<root>/.git/index`・worktree では
/// `.git` file と実体の git dir〔`rev-parse --absolute-git-dir`〕の同名 2 本）。
///
/// **在る file だけ**を出す。存在しない path を `rerun-if-changed` に出すと cargo は落ちないが
/// 「file が無い＝常に stale」と読んで **毎回** build script と crate を作り直す（worktree の
/// `.git` は file で `<root>/.git/HEAD` が無い周に実測）。
fn rerun_paths(dir: &Path) -> Vec<PathBuf> {
    let dot_git = dir.join("..").join("..").join(".git");
    let mut candidates = vec![dot_git.join("HEAD"), dot_git.join("index"), dot_git.clone()];
    if let Some(git_dir) = git(dir, &["rev-parse", "--absolute-git-dir"]) {
        let git_dir = PathBuf::from(git_dir.trim());
        candidates.push(git_dir.join("HEAD"));
        candidates.push(git_dir.join("index"));
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    for path in candidates {
        if path.is_file() && !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for path in rerun_paths(&dir) {
        emit(&format!("cargo:rerun-if-changed={}", path.display()));
    }
    emit(&format!("cargo:rustc-env={ENV_NAME}={}", build_commit(&dir)));
}
