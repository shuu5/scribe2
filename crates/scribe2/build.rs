//! build 元 commit を binary に焼く build script（設計 consumer-sync.md §2・ADR-0028 §2.1）。
//!
//! `git rev-parse --short=12 HEAD` と `git status --porcelain --untracked-files=no` を撃ち、
//! 値 `<sha12>[+dirty]` を cargo の出力 dir（`OUT_DIR`）の 1 file [`OUT_FILE`] に書く。compile 時の env は
//! 出さない（名の無い file 1 つに置くので env の名の字面が要らない・設計 carry-prep.md §2 の 3）。core の
//! `name::BUILD_COMMIT` が `include_str!` で読むだけ（実行時に env を読まない・憲法 C2.2）。git が無い・repo でない・
//! rc ≠ 0・出力が sha の形でない周は `unknown`（測れない周を成功に倒さない・C10）。
//! build は落とさない（panic しない・依存は std だけ）。
//!
//! 再走の母集団は git の meta（HEAD / index）と **tracked 全 file**（`git ls-files`・在る物だけ）である
//! （設計 consumer-sync.md §18・`s2-07l.317`）。unstaged の変更は index を動かさないので、meta だけを
//! `cargo:rerun-if-changed` に出すと build script が再走せず `+dirty` が古い値で止まる。列挙は
//! `build/rerun.rs` の 1 関数で、e2e の歯も同じ file を `include!` で読む。git の無い周は meta の在る物だけ。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 値を書く file の名（`OUT_DIR` 相対・core の `name::BUILD_COMMIT` の `include_str!` と同じ綴り）。
const OUT_FILE: &str = "build_commit";

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

// 再 build の引き金にする path の列挙 `rerun_paths`（git の meta と **tracked 全 file**・在る物だけ・決定的な
// 並び）。e2e の歯と同じ file を読む＝列挙の実装は 1 か所（設計 consumer-sync.md §18・`s2-07l.317`）。
include!("build/rerun.rs");

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for path in rerun_paths(&dir) {
        emit(&format!("cargo:rerun-if-changed={}", path.display()));
    }
    // `OUT_DIR` は cargo が build script の実行時に必ず渡す。書けない周は build を落とさず、読み手の
    // `include_str!` が file の不在で compile error になる（黙って古い値を焼かない）。
    if let Some(out) = std::env::var_os("OUT_DIR") {
        let _ = std::fs::write(Path::new(&out).join(OUT_FILE), build_commit(&dir));
    }
}
