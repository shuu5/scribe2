// 再 build の引き金にする path の列挙（設計 consumer-sync.md §18・契約表の行 i・`s2-07l.317`）。
//
// `crates/<NAME>/build.rs` と e2e の歯（`crates/<NAME>-boundary/tests/e2e/seat.rs` の `build_rerun_*`）が
// 同じこの file を `include!` で読む＝列挙の実装は 1 か所である。`include!` は item を読み手の module へ
// そのまま貼るので、inner doc（`//!`）と `use` を持たず、型と関数は full path で書く（読み手の `use` と
// 衝突させない）。依存は std だけ（build script は crate の依存を持たない）。

/// 再 build の引き金にする path の列（在る file だけ・sort 済みで重複なし＝決定的な並び）。
///
/// 母集団は 3 つ:
/// - git の meta: `<dir>/../../.git/HEAD` と `<dir>/../../.git/index`（worktree では `.git` file そのもの）と、
///   実体の git dir（`rev-parse --absolute-git-dir`）の `HEAD` / `index`。
/// - **tracked 全 file**: repo root（`rev-parse --show-toplevel`）で `git ls-files -z` を撃ち、root に繋いだ
///   絶対 path。unstaged の変更は index を動かさないので、git の meta だけでは cargo が build script を
///   再走させず `+dirty` が古い値で止まる（§18 の出所）。untracked は列挙しない（`+dirty` は
///   `--untracked-files=no` で測る＝母集団を揃える）。
/// - 消えた tracked・無い meta は出さない。存在しない path を `rerun-if-changed` に出すと cargo は落ちないが
///   「file が無い＝常に stale」と読んで **毎回** build script と crate を作り直す（worktree の `.git` は
///   file で `<root>/.git/HEAD` が無い周に実測）。
///
/// git が無い・repo でない・rc ≠ 0 の周は git の meta の候補のうち在る物だけ（0 本もある）を返す＝build は
/// 落とさない（panic しない）。
fn rerun_paths(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let git = |at: &std::path::Path, args: &[&str]| -> Option<Vec<u8>> {
        let out = std::process::Command::new("git").arg("-C").arg(at).args(args).output().ok()?;
        out.status.success().then_some(out.stdout)
    };
    let line = |bytes: Vec<u8>| String::from_utf8(bytes).ok().map(|text| std::path::PathBuf::from(text.trim()));
    let dot_git = dir.join("..").join("..").join(".git");
    let mut candidates = vec![dot_git.join("HEAD"), dot_git.join("index"), dot_git];
    if let Some(git_dir) = git(dir, &["rev-parse", "--absolute-git-dir"]).and_then(line) {
        candidates.push(git_dir.join("HEAD"));
        candidates.push(git_dir.join("index"));
    }
    if let Some(top) = git(dir, &["rev-parse", "--show-toplevel"]).and_then(line) {
        let listed = git(&top, &["ls-files", "-z"]).unwrap_or_default();
        for name in listed.split(|byte| *byte == 0).filter(|name| !name.is_empty()) {
            let name: &std::ffi::OsStr = std::os::unix::ffi::OsStrExt::from_bytes(name);
            candidates.push(top.join(name));
        }
    }
    let mut paths: Vec<std::path::PathBuf> = candidates.into_iter().filter(|path| path.is_file()).collect();
    paths.sort();
    paths.dedup();
    paths
}
