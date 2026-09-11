//! `flipcheck` の歯。**本体は `flipcheck.rs`** で、ここには test だけが在る。
//!
//! 分けたのは憲法 C4（1 file の上限）である——判定の実装と歯が同じ file に載ると
//! 上限に当たり、次に歯を足す便が「上限に入らないから書かない」へ倒れる。
//! `#[path]` で `flipcheck` の子 module として取り込むので、module path は
//! `flipcheck::tests` のまま＝歯の名前は 1 つも変わらない。
//!
//! この file は `#[cfg(test)] mod` の形を持たないが、`crates/*/src/**/*_tests.rs` は
//! **名前で test file と見なして丸ごと写す**（s2-07l.34 の (6)）ので、ここへ足した歯は
//! base へ写り flip を検査される。名前で見なければ区間判定には src 区間だけの file に
//! 見え、ここへ足した歯が 1 本も測られないままになる。

use super::{is_test_file, judge, judge_into, parse_base, split_regions, FilePair, Verdict, RETROACTIVE_MARK};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 合成 workspace の member 名（実 NAME の字面を .rs へ持ち込まない別名）。
const FIXTURE_MEMBER: &str = "flipdemo";
/// 合成 workspace の toolchain channel。
const FIXTURE_CHANNEL: &str = "1.98.1";
/// base 側の lib.rs（通る test を 1 本持つ＝base 健全性前段が rc 0 になる）。
const BASE_LIB: &str = "pub fn val() -> u32 {\n    1\n}\n#[cfg(test)]\nmod checks {\n    #[test]\n    fn holds() {\n        assert_eq!(super::val(), 1);\n    }\n}\n";

/// 同一 process 内での dir 名衝突を避ける連番。
static SEQ: AtomicU32 = AtomicU32::new(0);

/// repo の外に一意な tmp dir を作る。
fn make_tmp_dir() -> PathBuf {
    let base = std::env::temp_dir();
    for _ in 0..8 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("xtask-flip-{}-{nanos}-{seq}", std::process::id()));
        if std::fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
    panic!("合成 workspace 用の tmp dir を作れない");
}

/// fixture 内で git を撃つ（identity と署名を明示し外の設定に依存しない）。
fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// `rel` へ本文を書く（親 dir は作る）。
fn write_at(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("合成 workspace の dir を作れる");
    }
    std::fs::write(&path, body).expect("合成 workspace の file を書ける");
}

/// core crate の `src/lib.rs` の相対 path。
fn lib_rel() -> String {
    format!("crates/{FIXTURE_MEMBER}/src/lib.rs")
}

/// base commit を積んだ合成 workspace を作り、その dir と base の SHA を返す。
fn base_commit() -> (PathBuf, String) {
    let dir = make_tmp_dir();
    scaffold(&dir);
    let base = seed_fixture(&dir, BASE_LIB);
    (dir, base)
}

/// 合成 workspace の骨組み（workspace / toolchain / member の manifest）を書く。
///
/// **`judge_lib` と共有する**——片方だけが書くと、flip が 1 本でも立つ便で base tree が
/// 不完全になり、判定行が `reason=infra-error` に化ける。免除を数えなかったことを
/// `!line.contains("moved=")` で測る負例は、そのとき**判定路へ 1 度も入らないまま自動的に
/// 真**になる（実測 2026-09-11: `moved=0` を出す変異が 28/28 緑のまま生存した）。
fn scaffold(dir: &Path) {
    write_at(
        dir,
        "Cargo.toml",
        &format!("[workspace]\nresolver = \"2\"\nmembers = [\"crates/{FIXTURE_MEMBER}\"]\n"),
    );
    write_at(
        dir,
        "rust-toolchain.toml",
        &format!("[toolchain]\nchannel = \"{FIXTURE_CHANNEL}\"\n"),
    );
    write_at(
        dir,
        &format!("crates/{FIXTURE_MEMBER}/Cargo.toml"),
        &format!(
            "[package]\nname = \"{FIXTURE_MEMBER}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
        ),
    );
}

/// base と HEAD の lib 本文を与えて 1 便を判定する（marker まわりの負例で使い回す）。
fn judge_lib(base_lib: &str, head_lib: &str) -> Verdict {
    let dir = make_tmp_dir();
    scaffold(&dir);
    let base = seed_fixture(&dir, base_lib);
    write_at(&dir, &lib_rel(), head_lib);
    head_commit(&dir);
    let got = judge(&base, &dir);
    drop_fixture(&dir);
    got
}

/// marker を数えなかったことを確かめる（負例の共通 assert）。
fn assert_not_retroactive(got: &Verdict, what: &str) {
    assert_eq!(got.code, 1, "{what} で通してはならない: {}", got.line);
    assert!(!got.line.contains("retroactive"), "{what} を数えない: {}", got.line);
}

/// 既に組んだ fixture dir へ lib を書いて base commit を作る。
fn seed_fixture(dir: &Path, lib: &str) -> String {
    write_at(dir, &lib_rel(), lib);
    write_at(dir, "README.md", "fixture\n");
    if !dir.join(".git").exists() {
        assert!(git(dir, &["init", "-q"]), "fixture で git init できる");
    }
    assert!(git(dir, &["add", "-A"]), "fixture で git add できる");
    assert!(
        git(dir, &["commit", "-q", "-m", "base"]),
        "fixture で base を commit できる"
    );
    head_sha(dir)
}

/// fixture の HEAD の SHA。
fn head_sha(dir: &Path) -> String {
    let sha = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse を起動できる");
    let found = String::from_utf8_lossy(&sha.stdout).trim().to_owned();
    assert!(!found.is_empty(), "HEAD の SHA を読める");
    found
}

/// HEAD 側を書いて commit する。
fn head_commit(dir: &Path) {
    assert!(git(dir, &["add", "-A"]), "fixture で HEAD を add できる");
    assert!(
        git(dir, &["commit", "-q", "-m", "head"]),
        "fixture で HEAD を commit できる"
    );
}

/// fixture を使い切りにする。
fn drop_fixture(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// 判定 1 行が `reason=<語>` を持ち rc が期待どおりであることを表明する。
fn assert_verdict(line: &str, code: u8, want_code: u8, want: &str) {
    assert_eq!(code, want_code, "rc が期待と違う: {line}");
    assert!(line.contains(want), "判定行に {want} が無い: {line}");
}

// 歯は判定クラスごとの子 module（`flipcheck_<主題>_tests.rs`・`#[path]` で取り込む）に在り、
// この file は共通の helper と宣言だけを持つ（s2-07l.91・純粋な移動・C4 R-C4-2）。
// flip-check: moved s2-07l.91

#[path = "flipcheck_entrance_tests.rs"]
mod entrance;

#[path = "flipcheck_overlay_tests.rs"]
mod overlay;

#[path = "flipcheck_retroactive_tests.rs"]
mod retroactive;

#[path = "flipcheck_declaration_tests.rs"]
mod declaration;

#[path = "flipcheck_moved_tests.rs"]
mod moved;
