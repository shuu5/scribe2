//! `cargo xtask flip-check --base <ref>` の本体。TDD の flip を後から機械で確かめる。
//!
//! HEAD の **test 区間**を base の **src 区間**の上へ重ねた木で runner を撃ち、
//! 「新しい test が古い実装で赤い」ことを見る。依存は std だけで、外部 binary は
//! git と tar の 2 本である（呼出は [`std::process::Command`]）。
//!
//! **honest fence**: git / tar / cargo の spawn 失敗と、diff / rev-parse / archive /
//! tar / base 健全性前段 / runner 不在の rc≠0 は例外なく `reason=infra-error` として
//! rc 1 で返し、RED とも skip とも数えない。runner が signal で殺され rc を持たない
//! ときも RED と数えない（rc≠0 でないので (c) の RED-on-base に当たらない）。
//! **overlay 後の compile error は RED と数える**（新しい test が古い木で通らないこと
//! の一形態だからである）。
//!
//! **新規 src file の in-module test は base へ写さないので flip されない**——本 check が
//! 保証するのは既存 src file の test 区間の変更についてのみである。
//!
//! 掃除する範囲は `<root>/target/flipcheck` 配下だけである。憲法 N1 の射程は
//! scribe2 が管理する object の不可逆削除であり、`.gitignore` 済み `target/` の
//! 再生成物は含まない。

use crate::{emit, emit_err};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output, Stdio};

/// in-module test の始まりを示す行頭の印（`check.rs` の `split_test_src` と同じ規則）。
const TEST_MOD_MARK: &str = "#[cfg(test)]";

/// base tree と runner target を置く `target/` 配下の作業 dir 名。
const WORK_DIR: &str = "flipcheck";

/// 判定 1 行と rc の対。stdout へ出る判定行はこの `line` ただ 1 本である。
pub struct Verdict {
    /// stdout へ出す判定 1 行。
    pub line: String,
    /// rc（0 = 合格）。
    pub code: u8,
}

/// 掃除段の結果。guard 違反だけが判定を上書きする。
enum Cleanup {
    /// 消せた（または元から無い）。
    Done,
    /// 消せなかった（stderr へ loud に出し判定 rc を優先する）。
    Warned(String),
    /// 対象が `<root>/target/` 配下でないので 1 byte も消さなかった。
    Refused(String),
}

/// 変更された 1 本の `.rs` の base 側 / HEAD 側の姿。
struct FilePair {
    /// repo 相対 path。
    rel: String,
    /// base 側の本文（その rev に無ければ `None`）。
    base: Option<String>,
    /// HEAD 側の本文（その rev に無ければ `None`）。
    head: Option<String>,
}

impl FilePair {
    /// HEAD 側の test 区間（HEAD に無ければ空）。
    fn head_test(&self) -> String {
        self.head
            .as_deref()
            .map(|text| split_regions(&self.rel, text).1)
            .unwrap_or_default()
    }

    /// base 側の test 区間（base に無ければ空）。
    fn base_test(&self) -> String {
        self.base
            .as_deref()
            .map(|text| split_regions(&self.rel, text).1)
            .unwrap_or_default()
    }

    /// base へ写す本文。base に在る file は `base の src 区間 + HEAD の test 区間`、
    /// base に無い file は HEAD が test file のときだけ全体を写す（他は写さない）。
    fn overlay(&self) -> Option<String> {
        let head = self.head.as_deref()?;
        match self.base.as_deref() {
            Some(base) => Some(format!("{}{}", split_regions(&self.rel, base).0, self.head_test())),
            None if is_test_file(&self.rel) || head.starts_with(TEST_MOD_MARK) => Some(head.to_owned()),
            None => None,
        }
    }

    /// test 区間が base と byte 差を持つか（写せる新規 test file も差と数える）。
    fn flips(&self) -> bool {
        self.overlay().is_some() && self.head_test() != self.base_test()
    }
}

/// `crates/*/tests/*.rs` か（flip-check 独自の追加規則で全体を test 区間と扱う）。
fn is_test_file(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    parts.len() == 4
        && parts.first() == Some(&"crates")
        && parts.get(2) == Some(&"tests")
        && rel.ends_with(".rs")
}

/// test 区間の始まる byte offset（最初の行頭 `#[cfg(test)]`）。
fn test_offset(text: &str) -> Option<usize> {
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with(TEST_MOD_MARK) {
            return Some(at);
        }
        at += line.len();
    }
    None
}

/// 本文を `(src 区間, test 区間)` に分ける。`crates/*/tests/*.rs` は全体が test 区間。
fn split_regions(rel: &str, text: &str) -> (String, String) {
    if is_test_file(rel) {
        return (String::new(), text.to_owned());
    }
    match test_offset(text).and_then(|at| text.split_at_checked(at)) {
        Some((src, test)) => (src.to_owned(), test.to_owned()),
        None => (text.to_owned(), String::new()),
    }
}

/// 判定 1 行を組み立てる。
fn verdict(line: &str, code: u8) -> Verdict {
    Verdict {
        line: line.to_owned(),
        code,
    }
}

/// honest fence の判定（RED とも skip とも数えない）。
fn infra(reason: &str) -> Verdict {
    verdict(&format!("flip-check: FAIL reason=infra-error {reason}"), 1)
}

/// 歯が咬んだ判定（flip していない）。
fn fail(reason: &str) -> Verdict {
    verdict(&format!("flip-check: FAIL reason={reason}"), 1)
}

/// 子プロセスの stderr を 1 行へ畳む。
fn trimmed(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<&str>>()
        .join(" / ")
}

/// 子の出力を親の stderr へ流す（親 stdout は判定 1 行のみ）。
fn relay(label: &str, output: &Output) {
    for stream in [&output.stdout, &output.stderr] {
        for line in String::from_utf8_lossy(stream).lines() {
            emit_err(&format!("flip-check: {label}| {line}"));
        }
    }
}

/// `--base <ref>` を取り出す。flag 不在 / 値不在 / 空文字はすべて Err（rc 2 の理由）。
fn parse_base(args: &[String]) -> Result<String, String> {
    let at = args
        .iter()
        .position(|arg| arg == "--base")
        .ok_or_else(|| "flip-check: --base <ref> が無い".to_owned())?;
    let value = args
        .get(at + 1)
        .ok_or_else(|| "flip-check: --base の直後に値が無い".to_owned())?;
    if value.is_empty() {
        return Err("flip-check: --base が空文字である".to_owned());
    }
    Ok(value.clone())
}

/// `git -C <dir> <args...>` を撃ち rc 0 のときだけ stdout を返す。
fn git_stdout(dir: &Path, label: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|err| format!("{label} を起動できない: {err}"))?;
    if !output.status.success() {
        return Err(format!("{label} が rc≠0: {}", trimmed(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `git diff --name-only <base>...HEAD -- *.rs` の出力行。rc≠0 は Err（infra-error）。
///
/// pathspec は shell を介さない独立した 1 引数なのでクォート文字を字面に含めない。
fn changed_rs(base: &str, workdir: &Path) -> Result<Vec<String>, String> {
    let range = format!("{base}...HEAD");
    let output = Command::new("git")
        .arg("-C")
        .arg(workdir)
        .args(["diff", "--name-only"])
        .arg(&range)
        .arg("--")
        .arg("*.rs")
        .output()
        .map_err(|err| format!("git diff を起動できない: {err}"))?;
    if !output.status.success() {
        return Err(format!("git diff が rc≠0: {}", trimmed(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect())
}

/// `<rev>:<rel>` の本文。その rev に無ければ `Ok(None)`。
///
/// git の **spawn 失敗だけ**を `Err`（infra-error の理由）へ上げる。rc≠0 は「その rev に
/// その path が無い」という正当な意味なので `Ok(None)` に保つ。両者を畳むと spawn 失敗が
/// `not-copied` に化けて flip 未検証のまま rc 0 が出る（honest fence は「例外なく」である）。
fn show(workdir: &Path, rev: &str, rel: &str) -> Result<Option<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workdir)
        .arg("show")
        .arg(format!("{rev}:{rel}"))
        .output()
        .map_err(|err| format!("git show {rev}:{rel} を起動できない: {err}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
}

/// 変更 .rs ごとに base / HEAD の本文を読む。
fn load_pairs(base: &str, workdir: &Path, changed: &[String]) -> Result<Vec<FilePair>, String> {
    let mut pairs = Vec::new();
    for rel in changed {
        pairs.push(FilePair {
            rel: rel.clone(),
            base: show(workdir, base, rel)?,
            head: show(workdir, "HEAD", rel)?,
        });
    }
    Ok(pairs)
}

/// workspace root を解決する。失敗は infra-error の理由になる。
fn repo_root(workdir: &Path) -> Result<PathBuf, String> {
    let shown = git_stdout(workdir, "git rev-parse", &["rev-parse", "--show-toplevel"])?;
    let text = shown.trim();
    if text.is_empty() {
        return Err("git rev-parse が空を返した".to_owned());
    }
    Ok(PathBuf::from(text))
}

/// `git archive <base> | tar -x -C <dest>` を撃ち **両方の rc** を見る。
///
/// 共有 `.git` へは 1 byte も書かない（`git worktree add` は使わない）。
fn extract_archive(base: &str, root: &Path, dest: &Path) -> Result<(), String> {
    let mut archive = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["archive", base])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("git archive を起動できない: {err}"))?;
    let piped = archive
        .stdout
        .take()
        .ok_or_else(|| "git archive の stdout を取れない".to_owned())?;
    let tar = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(dest)
        .stdin(Stdio::from(piped))
        .output()
        .map_err(|err| format!("tar を起動できない: {err}"))?;
    let archived = archive
        .wait_with_output()
        .map_err(|err| format!("git archive を待てない: {err}"))?;
    if !archived.status.success() {
        return Err(format!("git archive が rc≠0: {}", trimmed(&archived.stderr)));
    }
    if !tar.status.success() {
        return Err(format!("tar が rc≠0: {}", trimmed(&tar.stderr)));
    }
    Ok(())
}

/// `<root>/target/flipcheck/base` へ base tree を実体化する。
///
/// `tar` は truncate を rc 0 で通すので、実体化直後に `Cargo.toml` の存在を確かめる。
fn materialize_base(base: &str, root: &Path) -> Result<PathBuf, String> {
    let dest = work_dir(root).join("base");
    if let Err(err) = fs::remove_dir_all(&dest) {
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("{} を掃除できない: {err}", dest.display()));
        }
    }
    fs::create_dir_all(&dest).map_err(|err| format!("{} を作れない: {err}", dest.display()))?;
    extract_archive(base, root, &dest)?;
    let manifest = dest.join("Cargo.toml");
    if !manifest.is_file() {
        return Err(format!(
            "{} が無い（base tree が不完全である）",
            manifest.display()
        ));
    }
    Ok(dest)
}

/// 作業 dir（`<root>/target/flipcheck`）。
fn work_dir(root: &Path) -> PathBuf {
    root.join("target").join(WORK_DIR)
}

/// base tree で runner を撃つ。env は親を継承し `CARGO_TARGET_DIR` だけ上書きする。
fn nextest(dir: &Path, target_dir: &Path) -> Result<Output, String> {
    Command::new("cargo")
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target_dir)
        .args(["nextest", "run", "--workspace", "--no-tests=fail"])
        .output()
        .map_err(|err| format!("cargo nextest を起動できない: {err}"))
}

/// overlay を base tree へ書き `tests_changed` を返す。
fn write_overlay(dest: &Path, pairs: &[FilePair]) -> Result<usize, String> {
    let mut flipped = 0;
    for pair in pairs {
        let Some(body) = pair.overlay() else {
            emit_err(&format!("flip-check: not-copied {}", pair.rel));
            continue;
        };
        let path = dest.join(&pair.rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("{} を作れない: {err}", parent.display()))?;
        }
        fs::write(&path, body).map_err(|err| format!("{} を書けない: {err}", path.display()))?;
        emit_err(&format!("flip-check: changed {}", pair.rel));
        if pair.flips() {
            flipped += 1;
            emit_err(&format!("flip-check: test-diff {}", pair.rel));
        }
    }
    Ok(flipped)
}

/// overlay 後の runner の rc を判定に写す。
///
/// rc 4（no tests to run）は infra-error で、compile error 時の nextest rc は 101
/// なので弁別できる。**rc を持たない終了（signal / OOM kill）は RED と数えない**
/// ——flip の証拠が無いまま合格させる fail-open を作らないためである。
fn judge_run(output: &Output, flipped: usize) -> Verdict {
    match output.status.code() {
        Some(0) => fail("green-on-base"),
        Some(4) => infra("no-tests-on-base"),
        Some(_) => verdict(
            &format!("flip-check: RED-on-base ok tests_changed={flipped}"),
            0,
        ),
        None => infra("runner-killed-by-signal"),
    }
}

/// base tree の健全性前段。overlay を書く前に base のまま runner を撃つ。
fn base_is_green(dest: &Path, target: &Path) -> Result<(), Verdict> {
    match nextest(dest, target) {
        Err(reason) => Err(infra(&reason)),
        Ok(output) => {
            relay("base", &output);
            if output.status.success() {
                Ok(())
            } else {
                Err(infra("base-not-green"))
            }
        }
    }
}

/// base を実体化し健全性を確かめ overlay を書いて runner を撃つ。
fn run_on_base(base: &str, root: &Path, pairs: &[FilePair]) -> Verdict {
    let dest = match materialize_base(base, root) {
        Err(reason) => return infra(&reason),
        Ok(found) => found,
    };
    let target = work_dir(root).join("target");
    if let Err(blocked) = base_is_green(&dest, &target) {
        return blocked;
    }
    let flipped = match write_overlay(&dest, pairs) {
        Err(reason) => return infra(&reason),
        Ok(count) => count,
    };
    match nextest(&dest, &target) {
        Err(reason) => infra(&reason),
        Ok(output) => {
            relay("overlay", &output);
            judge_run(&output, flipped)
        }
    }
}

/// `<root>/target/flipcheck` を消す。`<root>/target/` 配下でなければ 1 byte も消さない。
fn cleanup(root: &Path) -> Cleanup {
    let work = work_dir(root);
    if !work.starts_with(root.join("target")) {
        return Cleanup::Refused(format!("{} が target 配下でない", work.display()));
    }
    match fs::remove_dir_all(&work) {
        Ok(()) => Cleanup::Done,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Cleanup::Done,
        Err(err) => Cleanup::Warned(format!("{} を消せない: {err}", work.display())),
    }
}

/// 後片付けをして判定を返す（rm の失敗は loud に出すだけで判定 rc を優先する）。
fn finish(root: &Path, outcome: Verdict) -> Verdict {
    match cleanup(root) {
        Cleanup::Done => outcome,
        Cleanup::Warned(reason) => {
            emit_err(&format!("flip-check: cleanup {reason}"));
            outcome
        }
        Cleanup::Refused(reason) => infra(&reason),
    }
}

/// 段を順に踏んで判定を返す。判定行は 3 語のいずれか 1 行だけで、内訳は stderr へ出す。
pub fn judge(base: &str, workdir: &Path) -> Verdict {
    let changed = match changed_rs(base, workdir) {
        Err(reason) => return infra(&reason),
        Ok(list) => list,
    };
    if changed.is_empty() {
        return verdict("flip-check: skip reason=no-rust-diff", 0);
    }
    let pairs = match load_pairs(base, workdir, &changed) {
        Err(reason) => return infra(&reason),
        Ok(found) => found,
    };
    if !pairs.iter().any(FilePair::flips) {
        return fail("no-test-diff");
    }
    let root = match repo_root(workdir) {
        Err(reason) => return infra(&reason),
        Ok(found) => found,
    };
    let outcome = run_on_base(base, &root, &pairs);
    finish(&root, outcome)
}

/// CLI 面。判定行を stdout へ 1 行だけ出し rc を返す（引数不正だけが rc 2）。
pub fn run(args: &[String]) -> ExitCode {
    let base = match parse_base(args) {
        Ok(found) => found,
        Err(reason) => {
            emit_err(&reason);
            return ExitCode::from(2);
        }
    };
    let workdir = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(err) => {
            emit(&infra(&format!("cwd を解決できない: {err}")).line);
            return ExitCode::FAILURE;
        }
    };
    let outcome = judge(&base, &workdir);
    emit(&outcome.line);
    if outcome.code == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::{judge, parse_base, split_regions};
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
        write_at(
            &dir,
            "Cargo.toml",
            &format!("[workspace]\nresolver = \"2\"\nmembers = [\"crates/{FIXTURE_MEMBER}\"]\n"),
        );
        write_at(
            &dir,
            "rust-toolchain.toml",
            &format!("[toolchain]\nchannel = \"{FIXTURE_CHANNEL}\"\n"),
        );
        write_at(
            &dir,
            &format!("crates/{FIXTURE_MEMBER}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{FIXTURE_MEMBER}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
            ),
        );
        write_at(&dir, &lib_rel(), BASE_LIB);
        write_at(&dir, "README.md", "fixture\n");
        assert!(git(&dir, &["init", "-q"]), "fixture で git init できる");
        assert!(git(&dir, &["add", "-A"]), "fixture で git add できる");
        assert!(
            git(&dir, &["commit", "-q", "-m", "base"]),
            "fixture で base を commit できる"
        );
        let sha = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git rev-parse を起動できる");
        let base = String::from_utf8_lossy(&sha.stdout).trim().to_owned();
        assert!(!base.is_empty(), "base の SHA を読める");
        (dir, base)
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

    /// overlay は base の src 区間を保ち HEAD の test 区間だけを乗せる。
    ///
    /// 区間規則そのものを撃つ純関数の test である（pipeline 側の対照は
    /// `flipcheck_red_on_base_passes`＝HEAD の src が混ざれば green-on-base に倒れる）。
    #[test]
    fn flipcheck_test_region_overlay_keeps_base_src() {
        let src_rel = lib_rel();
        let base = "pub fn v() -> u32 {\n    1\n}\n#[cfg(test)]\nmod t {}\n";
        let head = "pub fn v() -> u32 {\n    2\n}\n#[cfg(test)]\nmod t {\n    // new\n}\n";
        let overlay = format!(
            "{}{}",
            split_regions(&src_rel, base).0,
            split_regions(&src_rel, head).1
        );
        assert!(overlay.contains("    1\n"), "base の src が残るはず: {overlay}");
        assert!(!overlay.contains("    2\n"), "HEAD の src は混ざらないはず: {overlay}");
        assert!(overlay.contains("// new"), "HEAD の test 区間が乗るはず: {overlay}");

        let (src, test) = split_regions(&format!("crates/{FIXTURE_MEMBER}/tests/it.rs"), base);
        assert!(src.is_empty(), "tests/*.rs は全体が test 区間のはず: {src}");
        assert_eq!(test, base, "tests/*.rs は全体が test 区間のはず");
    }

    /// src 区間だけの変更は rc 1 / `reason=no-test-diff` で落ちる。
    #[test]
    fn flipcheck_no_test_diff_fails() {
        let (dir, base) = base_commit();
        write_at(&dir, &lib_rel(), &format!("// touched\n{BASE_LIB}"));
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 1, "reason=no-test-diff");
    }

    /// .rs の差分が 0 件なら rc 0 / `reason=no-rust-diff` で skip する。
    #[test]
    fn flipcheck_no_rust_diff_skips() {
        let (dir, base) = base_commit();
        write_at(&dir, "README.md", "fixture touched\n");
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 0, "reason=no-rust-diff");
    }

    /// 新しい test が base の src で赤いなら rc 0 / `RED-on-base ok` で通る。
    #[test]
    fn flipcheck_red_on_base_passes() {
        let (dir, base) = base_commit();
        write_at(&dir, &lib_rel(), &BASE_LIB.replace('1', "2"));
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
        assert!(
            got.line.contains("tests_changed=1"),
            "写した 1 本を数えるはず: {}",
            got.line
        );
    }

    /// test 区間に差分は在るが base でも通るなら rc 1 / `reason=green-on-base`。
    #[test]
    fn flipcheck_green_on_base_fails() {
        let (dir, base) = base_commit();
        write_at(
            &dir,
            &lib_rel(),
            &BASE_LIB.replace(
                "        assert_eq!(super::val(), 1);\n",
                "        assert_eq!(super::val(), 1);\n        assert!(super::val() > 0);\n",
            ),
        );
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
    }

    /// 存在しない base ref は rc 1 / `reason=infra-error` で loud に落ちる
    /// （ref 存在チェックの特別扱いではなく git の rc≠0 経路で自然に到達する）。
    #[test]
    fn flipcheck_base_setup_failure_is_loud() {
        let (dir, _) = base_commit();
        let got = judge("no-such-base-ref", &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 1, "reason=infra-error");
    }

    /// `--base` の 3 つの不正形はすべて Err（CLI 面が rc 2 を返す経路）。
    #[test]
    fn flipcheck_base_arg_forms_are_rejected() {
        let empty: Vec<String> = Vec::new();
        assert!(parse_base(&empty).is_err(), "flag 不在は Err のはず");
        assert!(
            parse_base(&["--base".to_owned()]).is_err(),
            "値不在は Err のはず"
        );
        assert!(
            parse_base(&["--base".to_owned(), String::new()]).is_err(),
            "空文字は Err のはず"
        );
        assert_eq!(
            parse_base(&["--base".to_owned(), "main".to_owned()]).ok(),
            Some("main".to_owned()),
            "値が在れば Ok のはず"
        );
    }
}
