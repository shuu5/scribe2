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

/// in-module test の始まりを示す行頭の印。
const TEST_MOD_MARK: &str = "#[cfg(test)]";

/// [`TEST_MOD_MARK`] の直後（空行は跨ぐ）に来てよい `mod` 宣言の前置き。
const TEST_MOD_HEADS: &[&str] = &["mod ", "pub mod ", "pub(crate) mod "];

/// **後から足す歯**の明示例外を名乗る marker（test 区間内の 1 行）。
///
/// 既に land した挙動へ後から歯を足す便は、歯をどこへ置いても base で緑になる
/// ——測る対象が base に在るからで、TDD の不履行ではない。marker はその弁別を
/// **書いた人が明示する**ための逃がしであり、verdict 行に `retroactive=M` として
/// 載る（planner review の対象・notes に変異 proof が要る）。
const RETROACTIVE_MARK: &str = "// flip-check: retroactive ";

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

    /// test 区間に byte 差を持つか（写せる新規 test file も差と数える）。
    fn test_diff(&self) -> bool {
        self.overlay().is_some() && self.head_test() != self.base_test()
    }

    /// **後から足す歯**の明示例外を名乗るか（test 区間内の marker **行**）。
    ///
    /// src 区間の marker は効かない。src へ書けば「実装の隣に 1 行足すだけで
    /// flip 検査を外せる」ことになり、逃がしが静かになる。
    ///
    /// **行頭で見る**（素の `contains` では足りない）。marker の字面を文字列の中で
    /// 言及しただけの file——この門を測る歯そのものがそれである——まで免除され、
    /// **その便が丸ごと flip 検査を素通りする**（実測 2026-09-10: 本便自身が
    /// `retroactive=1` で通ってしまった）。逃がしは書いた人が 1 行として置いたときだけ効く。
    ///
    /// **その便で test 区間が動いた file にだけ効く**。marker の在るだけで数えると、
    /// 一度貼った札が**以後のすべての便を恒久的に rc 0 で通す**（実測 2026-09-10:
    /// base に marker が在る repo で src だけ変えた便が `retroactive=1` で通った）。
    /// base に無い file（新規 module）は test 区間が丸ごと新しいので対象に含める。
    fn retroactive(&self) -> bool {
        self.marked() && (self.test_diff() || self.base.is_none())
    }

    /// marker **行**を持つか（行頭で見る・素の `contains` では字面の言及まで拾う）。
    ///
    /// **bead id が要る**。marker は「どの便がなぜ RED を免除したか」を残すための札で、
    /// id の無い `// flip-check: retroactive` は誰にも辿れない——review の対象に
    /// ならない逃がしは、静かな逃がしと同じである。
    fn marked(&self) -> bool {
        self.head_test().lines().any(|line| {
            line.trim_start()
                .strip_prefix(RETROACTIVE_MARK)
                .is_some_and(|bead| !bead.trim().is_empty())
        })
    }

    /// test 区間の差が**削除だけ**か（順序を保った行の削除だけで HEAD が得られる）。
    ///
    /// 純粋な module 分割は「歯が別 file へ移った」だけで、base の src に対して
    /// 新しく赤くなる歯は 1 本も無い。これを flip と数えると、移動しただけの便が
    /// 恒久 `green-on-base` で落ちる。
    ///
    /// **行の部分列で見る**（`#[test]` fn 名の集合では足りない）。名前で数えると
    /// **本文の改変が丸ごと免除される**——`⊆` は「名前が同じで本文だけ変えた歯」を、
    /// 真部分集合でも「1 本消して別の 1 本の本文を変えた file」を通す（planner review
    /// 2026-09-10 で 2 度指摘された）。部分列なら、1 行でも足された / 書き換えられた
    /// 時点で成立しない。**fn 名を数えないので parser も要らない**。
    fn removed_only(&self) -> bool {
        self.test_diff() && is_line_subsequence(&self.head_test(), &self.base_test())
    }

    /// base へ写せないのに HEAD が test 区間を持つ（**構造的に flip を測れない**）。
    ///
    /// 新規 module は base 側に `mod` 宣言ごと存在せず compile されないので、
    /// test 区間だけを写しても測れない。
    fn not_flippable(&self) -> bool {
        self.overlay().is_none() && !self.head_test().is_empty() && !self.retroactive()
    }

    /// 「base で赤くなること」を要求する差か。
    fn flips(&self) -> bool {
        self.test_diff() && !self.removed_only() && !self.retroactive()
    }
}

/// `crates/*/tests/` 配下の `.rs` か（flip-check 独自の追加規則で全体を test 区間と扱う）。
///
/// 段数の**完全一致では数えない**。統合 test は `tests/<dir>/main.rs` の module 形を取り
/// （設計 docs/design/rules-manifest.md §2・憲法 R-C13-2 は target 数で数える）、
/// `crates/<c>/tests/<dir>/<f>.rs` は 5 段になる。4 段に限ると module 形の test file が
/// `not-copied` へ落ち、新しい test が base へ写らないまま rc 0 が出る。
fn is_test_file(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    parts.len() >= 4
        && parts.first() == Some(&"crates")
        && parts.get(2) == Some(&"tests")
        && rel.ends_with(".rs")
}

/// test 区間の始まる byte offset。
///
/// `#[cfg(test)]` のうち、**次の非空行が `mod` 宣言**であるものの最初の位置を返す。
/// 列 0 の marker を先に探し、1 本も無いときだけ字下げされた marker へ落ちる。
/// 該当が無ければ `None`（＝test 区間なし）。単に最初の `#[cfg(test)]` で切ると、
/// file 先頭付近の `#[cfg(test)] use …;` を始点に取ってしまい base の src 区間が空になる。
/// overlay から実装が丸ごと落ちた compile error は RED と数える規則なので、base で GREEN
/// な test でも rc 0 が出る（fail-open）。該当なしを「test 区間なし」へ倒すのは、写さない
/// 側が偽 RED を作らないためである。
fn test_offset(text: &str) -> Option<usize> {
    marker_offset(text, false).or_else(|| marker_offset(text, true))
}

/// marker の位置を探す。`allow_indent` が false なら列 0 の marker だけを見る。
///
/// **列 0 を先に見るのは字下げ許容の回帰を塞ぐためである**。字下げを一律に許すと、
/// file 前半の入れ子 module の中の marker を先に拾い、その後ろに在る実装まで test
/// 区間に入ってしまう。overlay は `base の src 区間 + HEAD の test 区間`なので、
/// HEAD の実装が base 木へ紛れ込み、「新しい test が古い実装で赤い」という前提が
/// 崩れる（`green-on-base` の偽 FAIL になる）。
fn marker_offset(text: &str, allow_indent: bool) -> Option<usize> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut at = 0;
    for (index, line) in lines.iter().enumerate() {
        let head = if allow_indent { line.trim_start() } else { line };
        if head.starts_with(TEST_MOD_MARK) && next_line_is_test_mod(&lines, index) {
            return Some(at);
        }
        at += line.len();
    }
    None
}

/// `index` の次に来る非空行が [`TEST_MOD_HEADS`] のいずれかで始まるか。
fn next_line_is_test_mod(lines: &[&str], index: usize) -> bool {
    lines
        .iter()
        .skip(index + 1)
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .is_some_and(|line| TEST_MOD_HEADS.iter().any(|head| line.starts_with(head)))
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

/// `head` の行が `base` の行の**部分列**か（順序を保った行の削除だけで得られるか）。
///
/// 2 本の指を進めるだけ（差分アルゴリズムを持ち込まない）。等しい行が現れたら両方、
/// 違えば `base` 側だけ進める。`head` を使い切れたら部分列である。
fn is_line_subsequence(head: &str, base: &str) -> bool {
    let mut wanted = head.lines();
    let mut next = wanted.next();
    for line in base.lines() {
        if next == Some(line) {
            next = wanted.next();
        }
    }
    next.is_none()
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

/// `git diff --name-only <base>...HEAD -- :(top)*.rs` の出力行。rc≠0 は Err（infra-error）。
///
/// pathspec は shell を介さない独立した 1 引数なのでクォート文字を字面に含めない。
/// **repo root から撃ち、かつ `:(top)` 錨を付ける**——素の `*.rs` は git の prefix
/// （cwd）配下へ縮むので、subdir から起動すると差分 0 件に化けて
/// `skip reason=no-rust-diff` の rc 0 が出る（何も検証しない fail-open）。
/// 出力 path は `--relative` を付けない限り root 相対である。
fn changed_rs(base: &str, root: &Path) -> Result<Vec<String>, String> {
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
fn load_pairs(base: &str, root: &Path, changed: &[String]) -> Result<Vec<FilePair>, String> {
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

/// overlay 1 本を base tree へ書く。写せない pair は書かず `false` を返す。
fn write_one(dest: &Path, pair: &FilePair) -> Result<bool, String> {
    let Some(body) = pair.overlay() else {
        emit_err(&format!("flip-check: not-copied {}", pair.rel));
        return Ok(false);
    };
    let path = dest.join(&pair.rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("{} を作れない: {err}", parent.display()))?;
    }
    fs::write(&path, body).map_err(|err| format!("{} を書けない: {err}", path.display()))?;
    emit_err(&format!("flip-check: changed {}", pair.rel));
    Ok(true)
}

/// overlay を base tree へ書く。**数えるのは [`Counts`] の仕事**（判定行の 3 数を
/// 書き込み経路で数えると、file ごとに撃つ路とまとめ撃ちの路で意味がずれる）。
fn write_overlay(dest: &Path, pairs: &[FilePair]) -> Result<(), String> {
    for pair in pairs {
        if !write_one(dest, pair)? {
            continue;
        }
        if pair.flips() {
            emit_err(&format!("flip-check: test-diff {}", pair.rel));
        }
    }
    Ok(())
}

/// overlay 後の runner の rc を判定に写す。
///
/// rc 4（no tests to run）は infra-error で、compile error 時の nextest rc は 101
/// なので弁別できる。**rc を持たない終了（signal / OOM kill）は RED と数えない**
/// ——flip の証拠が無いまま合格させる fail-open を作らないためである。
fn judge_run(output: &Output, counts: Counts) -> Verdict {
    match judge_one(output, None) {
        Err(found) => found,
        Ok(()) => ok_line(counts),
    }
}

/// overlay 1 回分の rc を判定する。**RED（期待どおり）だけが `Ok(())`**。
///
/// rc の語彙はここ 1 か所に閉じる（まとめ撃ちと file ごとの撃ちで意味がずれない）。
/// `rel` を渡した周は緑だった file を名指す——2 本以上を 1 度に撃つと「どれが緑か」が
/// 判定行から落ち、直す側が全部を疑うことになる。
fn judge_one(output: &Output, rel: Option<&str>) -> Result<(), Verdict> {
    match output.status.code() {
        Some(0) => Err(fail(&match rel {
            Some(rel) => format!("green-on-base file={rel}"),
            None => "green-on-base".to_owned(),
        })),
        Some(4) => Err(infra("no-tests-on-base")),
        Some(_) => Ok(()),
        None => Err(infra("runner-killed-by-signal")),
    }
}

/// 便 1 本の内訳（判定行に載る 3 つの数）。
#[derive(Debug, Clone, Copy, Default)]
struct Counts {
    /// base で赤くなることを要求した file の本数。
    flipped: usize,
    /// 削除・移動だけゆえ flip に数えなかった本数。
    removed: usize,
    /// marker で RED を免除した本数。
    retro: usize,
}

impl Counts {
    /// 便の pairs から数える。
    fn of(pairs: &[FilePair]) -> Self {
        Self {
            flipped: pairs.iter().filter(|pair| pair.flips()).count(),
            removed: pairs.iter().filter(|pair| pair.removed_only()).count(),
            retro: pairs.iter().filter(|pair| pair.retroactive()).count(),
        }
    }
}

/// 通した判定 1 行。**0 の内訳は出さない**（毎便に出ると読み手が意味を薄める）。
fn ok_line(counts: Counts) -> Verdict {
    let mut line = format!("flip-check: RED-on-base ok tests_changed={}", counts.flipped);
    if counts.removed > 0 {
        line.push_str(&format!(" removed-only={}", counts.removed));
    }
    if counts.retro > 0 {
        line.push_str(&format!(" retroactive={}", counts.retro));
    }
    verdict(&line, 0)
}

/// 単独 overlay の後始末（撃つ前の状態へ戻す）。
enum Restore {
    /// base に在った file——この本文へ書き戻す。
    Body(Vec<u8>),
    /// base に無かった file——消す。
    Absent,
}

impl Restore {
    /// 撃つ前の状態へ戻す。
    fn apply(self, path: &Path) -> Result<(), String> {
        match self {
            Self::Body(bytes) => {
                fs::write(path, bytes).map_err(|err| format!("{} へ戻せない: {err}", path.display()))
            }
            Self::Absent => match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(format!("{} を消せない: {err}", path.display())),
            },
        }
    }
}

/// 1 本だけ overlay を置く（戻し方を返す）。
///
/// 呼び手は flip する pair だけを渡す。[`FilePair::flips`] は `overlay().is_some()` を
/// 含むので [`write_one`] の `false`（写せない pair）はここでは起きない。
fn swap_in(dest: &Path, pair: &FilePair) -> Result<Restore, String> {
    let path = dest.join(&pair.rel);
    let before = match fs::read(&path) {
        Ok(found) => Restore::Body(found),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Restore::Absent,
        Err(err) => return Err(format!("{} を読めない: {err}", path.display())),
    };
    write_one(dest, pair)?;
    Ok(before)
}

/// flip した file が 2 本以上のとき、**1 本ずつ単独で** overlay して撃つ。
///
/// まとめて 1 回だけ撃つと、**1 本でも RED なら全体が RED に見える**——隣の file の
/// 新しい test が base で緑でも、赤い file に隠れて `RED-on-base ok` が出る（偽の RED）。
/// flip-check が守ろうとしているのは「新しい歯は 1 本ずつ base で赤い」であって
/// 「どれか 1 本が赤い」ではない。
fn judge_each(
    dest: &Path,
    target: &Path,
    pairs: &[FilePair],
    flipping: &[&FilePair],
    counts: Counts,
) -> Verdict {
    // flip しない pair は overlay しても内容が base と同じ（base の src + 同一の test 区間）
    // なので、先にまとめて置く。ここで置いても base の緑は動かない。
    for pair in pairs.iter().filter(|pair| !pair.flips()) {
        if let Err(reason) = write_one(dest, pair) {
            return infra(&reason);
        }
    }
    for pair in flipping {
        emit_err(&format!("flip-check: test-diff {}", pair.rel));
        let path = dest.join(&pair.rel);
        let restore = match swap_in(dest, pair) {
            Err(reason) => return infra(&reason),
            Ok(found) => found,
        };
        let judged = match nextest(dest, target) {
            Err(reason) => Err(infra(&reason)),
            Ok(output) => {
                relay(&format!("overlay {}", pair.rel), &output);
                judge_one(&output, Some(&pair.rel))
            }
        };
        // **戻しは判定より先**（次の file を base の上で撃つ前提が崩れる）。ただし
        // **返す判定は judged を優先する**——戻せなかったことで `green-on-base file=<rel>`
        // を infra-error に化けさせると、どの file が緑だったかが判定行から消える
        // （review 2026-09-10）。戻し失敗は loud に出すが判定は上書きしない。
        let restored = restore.apply(&path);
        if let Err(found) = judged {
            if let Err(reason) = restored {
                emit_err(&format!("flip-check: restore {reason}"));
            }
            return found;
        }
        if let Err(reason) = restored {
            return infra(&reason);
        }
    }
    ok_line(counts)
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
fn run_on_base(base: &str, root: &Path, pairs: &[FilePair], counts: Counts) -> Verdict {
    let dest = match materialize_base(base, root) {
        Err(reason) => return infra(&reason),
        Ok(found) => found,
    };
    let target = work_dir(root).join("target");
    if let Err(blocked) = base_is_green(&dest, &target) {
        return blocked;
    }
    // flip した file が 2 本以上なら **1 本ずつ**撃つ（まとめ撃ちは偽の RED を作る）。
    // 1 本のときは従来どおり 1 回で足りる（分ける対象が無い）。
    let flipping: Vec<&FilePair> = pairs.iter().filter(|pair| pair.flips()).collect();
    if flipping.len() >= 2 {
        return judge_each(&dest, &target, pairs, &flipping, counts);
    }
    if let Err(reason) = write_overlay(&dest, pairs) {
        return infra(&reason);
    }
    match nextest(&dest, &target) {
        Err(reason) => infra(&reason),
        Ok(output) => {
            relay("overlay", &output);
            judge_run(&output, counts)
        }
    }
}

/// `<root>/target/flipcheck` を消す。
///
/// 「`<root>/target/` 配下か」の門は置かない——[`work_dir`] が `root/target/<WORK_DIR>` を
/// 組み立てる唯一の口なので、その門は**構造上必ず真**であり、`Refused` の枝には
/// 到達できなかった（到達しない枝は読み手に「そういう場合が在る」と誤読させる死枝である）。
fn cleanup(root: &Path) -> Cleanup {
    let work = work_dir(root);
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
    }
}

/// 段を順に踏んで判定を返す。判定行は 3 語のいずれか 1 行だけで、内訳は stderr へ出す。
///
/// `workdir` は repo 内のどこでもよい。最初に repo root を解いてから git を撃つので、
/// 判定は起動 dir に依らない。
pub fn judge(base: &str, workdir: &Path) -> Verdict {
    let root = match repo_root(workdir) {
        Err(reason) => return infra(&reason),
        Ok(found) => found,
    };
    let changed = match changed_rs(base, &root) {
        Err(reason) => return infra(&reason),
        Ok(list) => list,
    };
    if changed.is_empty() {
        return verdict("flip-check: skip reason=no-rust-diff", 0);
    }
    let pairs = match load_pairs(base, &root, &changed) {
        Err(reason) => return infra(&reason),
        Ok(found) => found,
    };
    let counts = Counts::of(&pairs);
    if counts.flipped == 0 {
        return no_flip_verdict(&pairs, counts);
    }
    let outcome = run_on_base(base, &root, &pairs, counts);
    finish(&root, outcome)
}

/// **base で赤くなることを要求する差が 1 本も無い**周の判定（runner を撃たない）。
///
/// 3 通りを弁別する。まとめて 1 語で落とすと、直す側は「何を直せばよいか」を
/// 判定行から読めない——`green-on-base` は TDD の不履行を指す語であって、
/// 構造的に測れない便や、移動だけの便に貼ってよい札ではない。
fn no_flip_verdict(pairs: &[FilePair], counts: Counts) -> Verdict {
    let stuck: Vec<&str> = pairs
        .iter()
        .filter(|pair| pair.not_flippable())
        .map(|pair| pair.rel.as_str())
        .collect();
    if !stuck.is_empty() {
        // **逃がし方を書く**。「測れない」とだけ言われた側は、次に何をすれば
        // 測れるようになるのかを自分で探すことになる。
        emit_err(
            "flip-check: 新規 module は base に mod 宣言ごと無く compile されない。\
             test を crates/<c>/tests/<dir>/<f>.rs の module file か既存 file の test 区間へ置くか、\
             後から足す歯なら test 区間へ `// flip-check: retroactive <bead-id>` を 1 行置く",
        );
        return fail(&format!("not-flippable files={}", stuck.join(",")));
    }
    for pair in pairs.iter().filter(|pair| pair.removed_only()) {
        emit_err(&format!(
            "flip-check: not-flipped reason=tests-removed-only {}",
            pair.rel
        ));
    }
    if counts.removed > 0 || counts.retro > 0 {
        return ok_line(counts);
    }
    fail("no-test-diff")
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
    use super::{is_test_file, judge, parse_base, split_regions, Verdict};
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
        let base = seed_fixture(&dir, BASE_LIB);
        (dir, base)
    }

    /// base と HEAD の lib 本文を与えて 1 便を判定する（marker まわりの負例で使い回す）。
    fn judge_lib(base_lib: &str, head_lib: &str) -> Verdict {
        let dir = make_tmp_dir();
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

    /// module 形の統合 test（`tests/<dir>/<f>.rs`）も test file と見なす。
    ///
    /// 4 段完全一致だと `crates/<c>/tests/<dir>/<f>.rs` が漏れ、新しい test が base へ
    /// 写らないまま `not-copied` になる。
    #[test]
    fn entrance_is_test_file_accepts_module_dirs() {
        for rel in [
            "crates/demo/tests/e2e/main.rs",
            "crates/demo/tests/e2e/rules.rs",
            "crates/demo/tests/single.rs",
            "crates/demo/tests/a/b/c.rs",
        ] {
            assert!(is_test_file(rel), "test file のはず: {rel}");
        }
        for rel in [
            "crates/demo/src/lib.rs",
            "crates/demo/tests/e2e/main.txt",
            "tests/e2e/main.rs",
            "crates/demo/benches/x.rs",
        ] {
            assert!(!is_test_file(rel), "test file でないはず: {rel}");
        }
    }

    /// 列 0 の marker が後方に在るとき、split 点は字下げ marker でなく列 0 側である。
    ///
    /// 字下げを一律に許すと、入れ子 module の marker を先に拾い、その後ろの実装まで
    /// test 区間へ移る。overlay で HEAD の実装が base 木へ紛れ込む回帰の負例である。
    #[test]
    fn entrance_test_mod_mark_prefers_column_zero() {
        let text = "mod inner {\n    #[cfg(test)]\n    mod probe {\n        fn x() {}\n    }\n}\npub fn real_impl() -> u32 {\n    1\n}\n#[cfg(test)]\nmod tests {\n    fn y() {}\n}\n";
        let (src, test) = split_regions("crates/demo/src/lib.rs", text);
        assert!(src.contains("real_impl"), "実装は src 区間に残る: {src:?}");
        assert!(!test.contains("real_impl"), "実装は test 区間へ移らない: {test:?}");
        assert!(
            test.starts_with("#[cfg(test)]\nmod tests {"),
            "test 区間は列 0 の marker から始まる: {test:?}"
        );
    }

    /// 列 0 の marker が 1 本も無いときは、字下げされた `#[cfg(test)]` を始点にする。
    #[test]
    fn entrance_test_mod_mark_allows_indent() {
        let text = "mod outer {\n    #[cfg(test)]\n    mod t {\n        fn a() {}\n    }\n}\n";
        let (src, test) = split_regions("crates/demo/src/lib.rs", text);
        assert_eq!(src, "mod outer {\n", "src 区間は marker の手前まで");
        assert!(
            test.starts_with("    #[cfg(test)]\n    mod t {"),
            "test 区間が字下げされた marker から始まる: {test:?}"
        );
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

    /// 先頭の `#[cfg(test)] use …;` を test 区間の始点にしない。
    ///
    /// 始点に取ると base の src 区間が空になり overlay から実装が丸ごと落ちる。
    /// その compile error は RED と数える規則なので、base で GREEN な test でも
    /// rc 0 が出る（fail-open）。始点は「直後の非空行が `mod` である `#[cfg(test)]`」。
    #[test]
    fn flipcheck_test_region_starts_at_test_mod() {
        let rel = lib_rel();
        let text = "#[cfg(test)]\nuse std::fmt;\n\npub fn v() -> u32 {\n    1\n}\n\n#[cfg(test)]\nmod t {\n    // body\n}\n";
        let (src, test) = split_regions(&rel, text);
        assert!(src.contains("pub fn v()"), "実装は src 区間に残るはず: {src}");
        assert!(
            src.contains("use std::fmt;"),
            "先頭の cfg(test) use は src 区間に残るはず: {src}"
        );
        assert!(
            test.starts_with("#[cfg(test)]\nmod t {"),
            "test 区間は mod 宣言から始まるはず: {test}"
        );
        assert!(!test.contains("pub fn v()"), "実装は test 区間に入らないはず: {test}");

        let lone = "#[cfg(test)]\nuse std::fmt;\npub fn v() -> u32 {\n    1\n}\n";
        let (only_src, empty) = split_regions(&rel, lone);
        assert_eq!(only_src, lone, "mod が無ければ全体が src 区間のはず");
        assert!(empty.is_empty(), "mod が無ければ test 区間は空のはず: {empty}");
    }

    /// subdir を cwd にしても .rs の差分を取り落とさない（pathspec が cwd 配下へ縮まない）。
    ///
    /// 縮むと差分 0 件に化けて `skip reason=no-rust-diff` の rc 0 が出る（fail-open）。
    #[test]
    fn flipcheck_sees_rust_diff_from_subdir() {
        let (dir, base) = base_commit();
        write_at(&dir, "notes/keep.md", "note\n");
        write_at(&dir, &lib_rel(), &format!("// touched\n{BASE_LIB}"));
        head_commit(&dir);
        let got = judge(&base, &dir.join("notes"));
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 1, "reason=no-test-diff");
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

    /// flip した file が 2 本以上のとき、**1 本でも base で緑なら FAIL** し、どの file が
    /// 緑かを名指す。
    ///
    /// まとめて 1 回だけ撃つ実装はここで落ちる——赤い方（lib.rs）の失敗に隠れて全体が
    /// RED に見え、緑の新規 test（tests/it.rs）を載せたまま `RED-on-base ok` が出る。
    #[test]
    fn flip_check_fails_when_one_of_two_flipped_files_is_green_on_base() {
        let (dir, base) = base_commit();
        // 1 本目: base の src（val() == 1）では落ちる新しい test ＝単独で RED。
        write_at(&dir, &lib_rel(), &BASE_LIB.replace('1', "2"));
        // 2 本目: base でも通る新規の統合 test ＝単独で GREEN（これを見逃してはならない）。
        write_at(
            &dir,
            &format!("crates/{FIXTURE_MEMBER}/tests/it.rs"),
            &format!("#[test]\nfn green() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 1);\n}}\n"),
        );
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
        assert!(
            got.line.contains(&format!("file=crates/{FIXTURE_MEMBER}/tests/it.rs")),
            "緑だった file を名指すはず: {}",
            got.line
        );
    }

    /// flip した file が 2 本とも**単独で** RED なら rc 0 で通り、`tests_changed` は
    /// **flip した本数**を数える。
    ///
    /// 合格路の正例である（もう 1 本は FAIL 側の負例）。`ok_line(flipping.len())` を
    /// 定数へ縮める変異は、負例だけでは生き残る（review 2026-09-10 Q3）。
    #[test]
    fn flip_check_passes_when_both_flipped_files_are_red_on_base() {
        let (dir, base) = base_commit();
        // どちらも base の src（val() == 1）では落ちる＝単独で RED。
        write_at(&dir, &lib_rel(), &BASE_LIB.replace('1', "2"));
        write_at(
            &dir,
            &format!("crates/{FIXTURE_MEMBER}/tests/it.rs"),
            &format!("#[test]\nfn red() {{\n    assert_eq!({FIXTURE_MEMBER}::val(), 2);\n}}\n"),
        );
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
        assert!(
            got.line.contains("tests_changed=2"),
            "flip した 2 本を数えるはず: {}",
            got.line
        );
    }

    /// 新規 module の in-file 歯は base に写せない＝`green-on-base` でなく
    /// **`not-flippable`** と名乗り、どの file かを名指す。
    ///
    /// base 側に `mod` 宣言ごと存在しない file の test 区間だけを写しても compile
    /// されないので、構造的に測れない。TDD の不履行（`green-on-base`）と同じ札を
    /// 貼ると、直す側は何を直せばよいか判定行から読めない。
    #[test]
    fn flip_check_reports_not_flippable_for_new_module_with_inline_tests() {
        let (dir, base) = base_commit();
        let rel = format!("crates/{FIXTURE_MEMBER}/src/extra.rs");
        write_at(&dir, &rel, "pub fn v() -> u32 {\n    1\n}\n#[cfg(test)]\nmod t {\n    #[test]\n    fn probe() {\n        assert_eq!(super::v(), 1);\n    }\n}\n");
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 1, "reason=not-flippable");
        assert!(got.line.contains(&rel), "測れない file を名指すはず: {}", got.line);

        // **歯を持たない新規 module は not-flippable ではない**（写せなくても測るものが無い）。
        let (dir, base) = base_commit();
        write_at(&dir, &format!("crates/{FIXTURE_MEMBER}/src/plain.rs"), "pub fn w() -> u32 {\n    2\n}\n");
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert!(
            !got.line.contains("not-flippable"),
            "test 区間の無い新規 file を not-flippable に数えない: {}",
            got.line
        );

        // **marker を置いた新規 module は not-flippable ではなく retroactive**
        // （契約 4「copied / not-copied 両方」）。
        let (dir, base) = base_commit();
        write_at(
            &dir,
            &format!("crates/{FIXTURE_MEMBER}/src/marked.rs"),
            "pub fn w() -> u32 {\n    2\n}\n#[cfg(test)]\nmod t {\n    // flip-check: retroactive s2-07l.14\n    #[test]\n    fn probe() {\n        assert_eq!(super::w(), 2);\n    }\n}\n",
        );
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
        assert!(
            got.line.contains("retroactive=1") && !got.line.contains("not-flippable"),
            "marker 付きの新規 module は retroactive へ倒れるはず: {}",
            got.line
        );
    }

    /// src だけ変えた便は従来どおり落ちる（`not-flippable` へ逃がさない）。
    #[test]
    fn flip_check_keeps_green_on_base_when_no_test_changed() {
        let (dir, base) = base_commit();
        write_at(&dir, &lib_rel(), &BASE_LIB.replace("    1\n}", "    1 + 0\n}"));
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        // **従来どおり落ちる**（TDD の不履行）。現行 base では reason 語は
        // `no-test-diff` で、`green-on-base`（overlay を撃った上で緑だった周）とは
        // 別語である。本便はこの語を変えない＝新しい 3 語のどれへも逃がさない。
        assert_verdict(&got.line, got.code, 1, "reason=no-test-diff");
        for escaped in ["not-flippable", "removed-only", "retroactive"] {
            assert!(
                !got.line.contains(escaped),
                "src だけの変更を {escaped} へ逃がさない: {}",
                got.line
            );
        }
    }

    /// test 区間の差が**削除・移動だけ**の file は flip に数えず、その便に他の flip が
    /// 無くても `green-on-base` へ落とさない。
    ///
    /// 純粋な module 分割（歯が別 file へ移る）で恒久 FAIL しないための門である。
    #[test]
    fn flip_check_ignores_file_whose_test_diff_only_removes_tests() {
        // base は 2 本目の commit で取り直す（1 本目は「歯 2 本の状態」を作るためだけ）。
        let (dir, _seed) = base_commit();
        let two = BASE_LIB.replace(
            "    fn holds() {\n        assert_eq!(super::val(), 1);\n    }\n",
            "    fn holds() {\n        assert_eq!(super::val(), 1);\n    }\n    #[test]\n    fn also() {\n        assert_eq!(super::val(), 1);\n    }\n",
        );
        write_at(&dir, &lib_rel(), &two);
        assert!(git(&dir, &["add", "-A"]), "fixture で add できる");
        assert!(git(&dir, &["commit", "-q", "-m", "two"]), "2 本の歯を commit できる");
        let base = head_sha(&dir);
        // HEAD では 1 本減らすだけ（追加も改名も無い）。
        write_at(&dir, &lib_rel(), BASE_LIB);
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
        assert!(
            got.line.contains("tests_changed=0") && got.line.contains("removed-only=1"),
            "移動・削除だけと名乗るはず: {}",
            got.line
        );

        // **負例: 1 本消して別の 1 本の本文を変えた file は免除しない**。
        // `#[test]` fn 名で数える実装（`⊆` も真部分集合も）はここで落ちる——名前の上では
        // 「1 本減っただけ」に見えるが、残った歯の中身は書き換わっている。
        let (dir, seed) = base_commit();
        write_at(&dir, &lib_rel(), &two);
        assert!(git(&dir, &["add", "-A"]), "fixture で add できる");
        assert!(git(&dir, &["commit", "-q", "-m", "two"]), "2 本の歯を commit できる");
        let base = head_sha(&dir);
        let _ = seed;
        // 1 本（also）を消し、残った holds の本文を base でも通る形へ書き換える。
        write_at(
            &dir,
            &lib_rel(),
            &BASE_LIB.replace(
                "        assert_eq!(super::val(), 1);\n",
                "        assert!(super::val() >= 1);\n",
            ),
        );
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
        assert!(
            !got.line.contains("removed-only"),
            "本文が変わった file を削除だけへ逃がさない: {}",
            got.line
        );
    }

    /// marker を置いた file の新しい歯は base で緑でも通り、判定行に `retroactive=1`。
    ///
    /// 既に land した挙動へ後から歯を足す便は、歯をどこへ置いても base で緑になる。
    /// marker はその弁別を**書いた人が明示する**逃がしで、判定行に残るので review できる。
    #[test]
    fn flip_check_reports_retroactive_marker_instead_of_failing() {
        let (dir, base) = base_commit();
        let with_marker = BASE_LIB.replace(
            "mod checks {\n",
            "mod checks {\n    // flip-check: retroactive s2-07l.14\n    #[test]\n    fn added_later() {\n        assert_eq!(super::val(), 1);\n    }\n",
        );
        write_at(&dir, &lib_rel(), &with_marker);
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 0, "RED-on-base ok");
        assert!(got.line.contains("retroactive=1"), "marker を数えるはず: {}", got.line);

        // **字面を言及しただけの file は免除しない**。素の `contains` で見る実装は
        // ここで落ちる——marker を文字列に持つ歯（この門を測る当の歯）まで免除され、
        // その便が丸ごと flip 検査を素通りする。
        let (dir, base) = base_commit();
        let mentions = BASE_LIB.replace(
            "mod checks {\n",
            "mod checks {\n    #[test]\n    fn mentions() {\n        let note = \"// flip-check: retroactive s2-xxxx\";\n        assert!(!note.is_empty());\n    }\n",
        );
        write_at(&dir, &lib_rel(), &mentions);
        head_commit(&dir);
        let got = judge(&base, &dir);
        drop_fixture(&dir);
        assert_verdict(&got.line, got.code, 1, "reason=green-on-base");
        assert!(
            !got.line.contains("retroactive"),
            "言及しただけの file を免除しない: {}",
            got.line
        );

        // 以下は**免除されてはならない**負例。marker は「この便で足した歯」の逃がしであり、
        // 貼っておけば恒久的に検査が外れる札でも、src へ書けば効く札でもない。
        let marked = BASE_LIB.replace(
            "mod checks {\n",
            "mod checks {\n    // flip-check: retroactive s2-07l.14\n",
        );
        // (a) base に残った古い marker（HEAD では src だけ変えた便・review 2026-09-10 F1）
        assert_not_retroactive(
            &judge_lib(&marked, &marked.replace("    1\n}", "    1 + 0\n}")),
            "base から引き継いだ marker",
        );
        // (b) src 区間の marker（実装の隣の 1 行で検査を外せる形にしない）
        let added = |body: &str| {
            body.replace(
                "mod checks {\n",
                "mod checks {\n    #[test]\n    fn later() {\n        assert_eq!(super::val(), 1);\n    }\n",
            )
        };
        let src_side = BASE_LIB.replace(
            "pub fn val() -> u32 {\n",
            "// flip-check: retroactive s2-07l.14\npub fn val() -> u32 {\n",
        );
        assert_not_retroactive(&judge_lib(BASE_LIB, &added(&src_side)), "src 区間の marker");
        // (c) bead id の無い marker（区切りの空白も要る＝review の対象にならない札）
        for bare in [
            "// flip-check: retroactive",
            "// flip-check: retroactives2-07l.14",
            // 区切りの空白は在るが id が無い形（この 1 本だけが id 要求を測る）。
            "// flip-check: retroactive ",
        ] {
            let head = added(BASE_LIB).replace("mod checks {\n", &format!("mod checks {{\n    {bare}\n"));
            assert_not_retroactive(&judge_lib(BASE_LIB, &head), bare);
        }
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
