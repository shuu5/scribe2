//! `flip-check` のうち git / tar で base を取り出す群（設計 docs/design/pipeline.md §13・契約表の行 j）。
//!
//! `--base <ref>` の読み（[`parse_base`]）・変更 `.rs` の列挙（[`changed_rs`]）・base / HEAD の本文の読み
//! （[`load_pairs`]）・repo root の解決（[`repo_root`]）・base tree の実体化（[`materialize_base`] =
//! `git archive | tar` + [`index_base`]）・作業 dir（[`work_dir`]）。判定（`FilePair` の区間の弁別・
//! overlay・runner の撃ちと rc の読み）は親 `flipcheck.rs` に置いたまま。
//!
//! honest fence（親の module doc）はここでも同じ: git / tar の spawn 失敗と rc≠0 は例外なく
//! `Err(理由)` で上げ、呼び手が `reason=infra-error` に写す。

// flip-check: moved s2-07l.372

use super::{trimmed, FilePair, WORK_DIR};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `--base <ref>` を取り出す。flag 不在 / 値不在 / 空文字はすべて Err（rc 2 の理由）。
pub fn parse_base(args: &[String]) -> Result<String, String> {
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

/// `git diff --name-only <base>...HEAD -- :(top)*.rs` の出力行。rc≠0 は Err（infra-error）。
///
/// pathspec は shell を介さない独立した 1 引数なのでクォート文字を字面に含めない。
/// **repo root から撃ち、かつ `:(top)` 錨を付ける**——素の `*.rs` は git の prefix
/// （cwd）配下へ縮むので、subdir から起動すると差分 0 件に化けて
/// `skip reason=no-rust-diff` の rc 0 が出る（何も検証しない fail-open）。
/// 出力 path は `--relative` を付けない限り root 相対である。
pub fn changed_rs(base: &str, root: &Path) -> Result<Vec<String>, String> {
    let range = format!("{base}...HEAD");
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["diff", "--name-only"])
        .arg(&range)
        .arg("--")
        .arg(":(top)*.rs")
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
fn show(root: &Path, rev: &str, rel: &str) -> Result<Option<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
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
///
/// `pub(super)` に留める（`FilePair` は親の private 型なので、`pub` に上げると `private_interfaces`
/// で落ちる）。親は名指しの `use` で読む。
pub(super) fn load_pairs(base: &str, root: &Path, changed: &[String]) -> Result<Vec<FilePair>, String> {
    let mut pairs = Vec::new();
    for rel in changed {
        pairs.push(FilePair {
            rel: rel.clone(),
            base: show(root, base, rel)?,
            head: show(root, "HEAD", rel)?,
        });
    }
    Ok(pairs)
}

/// workspace root を解決する。失敗は infra-error の理由になる。
pub fn repo_root(workdir: &Path) -> Result<PathBuf, String> {
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
///
/// 展開の後に **base copy を git repo にする**（[`index_base`]）。`git archive` の展開は
/// `.git` を持たないので、base copy の中で撃った `git ls-files` / `git show HEAD:…` は
/// **外側の repo**（`<root>`）を見つけ、`.gitignore` 済み `target/` 配下の tracked file 0 本と
/// **HEAD（= base ではない）**を返す——tracked 集合を母集団に取り宣言を HEAD から読む歯
/// （`contracts check` の実 repo 母集団 `≥ 8` 行）が base 段で落ち、main が緑でも毎便
/// `base-not-green` になる（実測 2026-09-14・s2-07l.271 run 1 以後の全便）。
/// C12.2 の「base の緑」は実 checkout の緑であって、実体化は tracked 集合と HEAD を保たねばならない。
pub fn materialize_base(base: &str, root: &Path) -> Result<PathBuf, String> {
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
    index_base(base, root, &dest)?;
    Ok(dest)
}

/// base copy を **index と HEAD** を持つ git repo にする——commit は作らない。
///
/// 3 手: `git init -q` → 共有 object store を alternates で**読む**（`objects/info/alternates`
/// に `<root>` の objects dir を 1 行）→ `git update-ref --no-deref HEAD <base の sha>`
/// → `git add -A --force`。
///
/// - **HEAD は base の commit そのもの**である。`contracts check` は宣言 file を作業ツリー
///   でなく `HEAD:<file>` から読む（`pipe/declaration.rs`・「commit されていない宣言は無いのと
///   同じ」）ので、index だけでは `HEAD を読めない` の 2 件で落ちる（実測 2026-09-14・本便の
///   1 手目）。commit を作って HEAD を用意すると identity と署名の設定に依存し、しかも
///   base に無い commit を捏造することになる——alternates で object を読めば、base の
///   sha を HEAD に置くだけで足りる。
/// - index に載るのは archive の中身＝base の tracked 集合そのもので、`git ls-files` が
///   それをそのまま返す。`--force` は外の excludes（`core.excludesFile`）や base 自身の
///   `.gitignore` に当たる tracked file を index から落とさないため——archive に在る file は
///   base で tracked だった file だけなので、落とす理由が無い。
///
/// overlay（[`super::write_text`]）は従来どおり working tree にだけ書き、index にも HEAD にも
/// 触れない——HEAD の test 区間や `+` の新規 file が base の tracked 集合へ紛れると、
/// tracked 集合を読む歯が base 段と overlay 段で別の母集団を見ることになる。
///
/// 共有 `.git`（`<root>/.git`）へは 1 byte も書かない（alternates は読む側の設定で、
/// 書くのは copy の `.git` の中だけ）。作るのは `.gitignore` 済み `target/` 配下の入れ子
/// repo で、掃除は [`super::cleanup`] が dir ごと消す。失敗は infra-error の理由（RED にも skip
/// にも化けさせない）。
fn index_base(base: &str, root: &Path, dest: &Path) -> Result<(), String> {
    let sha = git_stdout(
        root,
        "git rev-parse",
        &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
    )?;
    let objects = git_stdout(
        root,
        "git rev-parse",
        &["rev-parse", "--path-format=absolute", "--git-path", "objects"],
    )?;
    git_stdout(dest, "git init", &["init", "-q"])?;
    let alternates = git_stdout(
        dest,
        "git rev-parse",
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "objects/info/alternates",
        ],
    )?;
    let alternates = Path::new(alternates.trim());
    if let Some(parent) = alternates.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("{} を作れない: {err}", parent.display()))?;
    }
    fs::write(alternates, format!("{}\n", objects.trim()))
        .map_err(|err| format!("{} を書けない: {err}", alternates.display()))?;
    git_stdout(
        dest,
        "git update-ref",
        &["update-ref", "--no-deref", "HEAD", sha.trim()],
    )?;
    git_stdout(dest, "git add", &["add", "-A", "--force"])?;
    Ok(())
}

/// 作業 dir（`<root>/target/flipcheck`）。
pub fn work_dir(root: &Path) -> PathBuf {
    root.join("target").join(WORK_DIR)
}
