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

/// **純粋な移動**を名乗る札（`s2-07l.86`）。
///
/// 「歯を 1 本も足さず、挙動を 1 つも変えず、file の間で動かしただけ」の便に使う。
/// 機械の意図としては [`FilePair::removed_only`] が同じことを見ているが、**あの門は
/// test 区間が動いたときにしか効かない**——歯が `check()` 越しの統合形で書かれた repo
/// では、実装だけを module へ出す便で test 区間が 1 行も動かず、門が立たない（実測
/// 2026-09-11・`s2-07l.84` は `no-test-diff` で落ちた）。
///
/// **`retroactive` を転用しない**のが本札の存在理由である。あちらは「**後から足す歯**」の
/// 例外で、判定行の `retroactive=N` は「後から足した歯が N 本ある」と読まれる。移動の便に
/// 貼ると、その数が何を免除したのか読めなくなる——札の doc が繰り返し警戒している
/// 「静かな逃がし」と同型になる。
///
/// 効く条件は `retroactive` と**同じ 4 つ**（test 区間内 / 行頭 / bead id 必須 /
/// base から持ち越した札は効かない）で、判定は同じ [`marker_beads`] を通る。
const MOVED_MARK: &str = "// flip-check: moved ";

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
    /// この札が覆うのは「**既に land した挙動へ後から歯を足す**」便だけである。歯を 1 本も
    /// 足さない便（実装を module へ移すだけ等）は前提を満たさない——そちらは [`MOVED_MARK`]
    /// を使う。判定行の `retroactive=N` が「後から足した歯が N 本」と読めることが、この札の
    /// 値打ちである。
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
        self.marked(RETROACTIVE_MARK) && (self.test_diff() || self.base.is_none())
    }

    /// **純粋な移動**の明示例外を名乗るか（[`MOVED_MARK`]・条件は `retroactive` と同じ）。
    ///
    /// 同じ 4 条件（test 区間内 / 行頭 / bead id 必須 / base から持ち越した札は効かない）を
    /// 通すため、判定は [`FilePair::marked`] を共有する——2 つ目の実装を作ると、片方だけが
    /// 緩む形で穴が開く。
    fn moved(&self) -> bool {
        self.marked(MOVED_MARK) && (self.test_diff() || self.base.is_none())
    }

    /// **この便で足した**札を持つか。
    ///
    /// **両方の札で共有する**（`mark` で切り替える）。2 つ目の実装を作ると片方だけが
    /// 緩む形で穴が開く。
    ///
    /// **bead id が要る**。marker は「どの便がなぜ RED を免除したか」を残すための札で、
    /// id の無い `// flip-check: retroactive`（`moved` も同じ）は誰にも辿れない——
    /// review の対象にならない逃がしは、静かな逃がしと同じである。
    ///
    /// **base に既に在る札は数えない**。marker 行は file に残るので、在るだけで数えると、
    /// 一度貼った札がその file の test 区間を触る**以後のすべての便**を免除する——札の
    /// bead id と便が対応しなくなり、判定行の `retroactive=N` / `moved=N` を review しても
    /// 何を免除したのかを辿れない。
    fn marked(&self, mark: &str) -> bool {
        !self.fresh_markers(mark).is_empty()
    }

    /// HEAD の test 区間に在り base の test 区間に無い札の bead id（＝この便で足した札）。
    ///
    /// base に無い file（新規 module）は base 側の test 区間が空なので、HEAD の札が
    /// そのまま「この便で足した札」になる。
    fn fresh_markers(&self, mark: &str) -> Vec<String> {
        let carried = marker_beads(&self.base_test(), mark);
        marker_beads(&self.head_test(), mark)
            .into_iter()
            .filter(|bead| !carried.contains(bead))
            .collect()
    }

    /// 札を持つのに **1 枚も新しくない**（base から持ち越した札だけ）か。
    ///
    /// 効かない札を黙って無視すると、書いた人は免除したつもりで RED を要求され、
    /// 理由を判定行から読めない。stderr へ 1 行出して直し方を渡す。
    fn stale_marker(&self) -> bool {
        [RETROACTIVE_MARK, MOVED_MARK].iter().any(|mark| {
            !marker_beads(&self.head_test(), mark).is_empty()
                && self.fresh_markers(mark).is_empty()
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
        self.overlay().is_none() && !self.head_test().is_empty() && !self.escaped()
    }

    /// 明示の逃がし（`retroactive` か `moved`）を名乗るか。
    fn escaped(&self) -> bool {
        self.retroactive() || self.moved()
    }

    /// **宣言だけの file** か（test 区間の差分行が全部 `mod x;` 形）。
    ///
    /// 新規 module は「宣言（`mod x;` の 1 行）」と「本体（module の file）」の 2 file に
    /// 割れる。単独 overlay では**どちらの判定も意味を持たない**——宣言だけを置くと本体が
    /// 無く `E0583` の compile error（偽 RED）、本体だけを置くと base に宣言が無く compile
    /// 対象に入らず全 PASS（偽 GREEN）になる（実測 2026-09-10・s2-07l.38.2 が初発）。
    /// ゆえに宣言 file は単独で撃たず、本体を撃つ木へ同梱する（[`judge_each`]）。
    ///
    /// 判定は**差分行の字面だけ**で行い parser は足さない。`mod` 以外の行が 1 行でも
    /// 動いていれば宣言 file ではない——自前の歯を足した file を宣言と見なして同梱すると、
    /// **その歯が単独で測られなくなる**（同梱は判定を緩める側なので、弁別は狭く取る）。
    fn declaration_only(&self) -> bool {
        let changed = changed_lines(&self.base_test(), &self.head_test());
        !changed.is_empty() && changed.iter().all(|line| is_mod_line(line))
    }

    /// 「base で赤くなること」を要求する差か。
    fn flips(&self) -> bool {
        self.test_diff() && !self.removed_only() && !self.escaped()
    }
}

/// test 区間の marker 行が名乗る **bead id** を拾う（行頭で見る・素の `contains` では
/// 字面の言及まで拾う）。
///
/// **札の同一性は bead id で見る**。行の字面で比べると、字下げや id の前後の空白が 1 個
/// 違うだけで base から持ち越した札が「この便で足した札」に化け、**古い id のまま免除が
/// 効き続ける**——この門が塞ごうとしている当の穴の裏口になる。
fn marker_beads(region: &str, mark: &str) -> Vec<String> {
    region
        .lines()
        .filter_map(|line| {
            let bead = line.trim_start().strip_prefix(mark)?.trim();
            (!bead.is_empty()).then(|| bead.to_owned())
        })
        .collect()
}

/// 2 つの本文の**片側にしか無い行**（追加行と削除行）。空白だけの行は数えない。
///
/// 順序は見ない（行の多重集合の差）。宣言 file の弁別に要るのは「何の行が動いたか」だけで、
/// どこへ動いたかではない。
fn changed_lines(base: &str, head: &str) -> Vec<String> {
    let mut rest: Vec<&str> = meaningful(base);
    let mut changed = Vec::new();
    for line in meaningful(head) {
        match rest.iter().position(|found| *found == line) {
            Some(at) => {
                rest.remove(at);
            }
            None => changed.push(line.to_owned()),
        }
    }
    changed.extend(rest.into_iter().map(str::to_owned));
    changed
}

/// 空白だけの行を除いた行の列。
fn meaningful(text: &str) -> Vec<&str> {
    text.lines().filter(|line| !line.trim().is_empty()).collect()
}

/// `mod x;`（`pub` / `pub(crate)` 可）の 1 行か。
fn is_mod_line(line: &str) -> bool {
    mod_name(line).is_some()
}

/// `mod x;`（`pub` / `pub(crate)` 可）の 1 行なら、その module 名。
///
/// 弁別は [`is_mod_line`] と同じ字面で行う（同じ規則を 2 か所へ書くと、宣言と数えた行と
/// 本体の在処を探した行がずれる）。
fn mod_name(line: &str) -> Option<&str> {
    let name = strip_visibility(line.trim())?
        .strip_prefix("mod ")?
        .strip_suffix(';')?;
    (!name.is_empty() && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_'))
        .then_some(name)
}

/// 可視性の前置きを外す。`pub` の後ろに空白が無い字面（`pubmod x;`）は `None`。
fn strip_visibility(body: &str) -> Option<&str> {
    let Some(rest) = body.strip_prefix("pub") else {
        return Some(body);
    };
    let rest = rest.strip_prefix("(crate)").unwrap_or(rest);
    rest.starts_with(char::is_whitespace).then(|| rest.trim_start())
}

/// `crates/*/tests/` 配下の `.rs` か（flip-check 独自の追加規則で全体を test 区間と扱う）。
///
/// 段数の**完全一致では数えない**。統合 test は `tests/<dir>/main.rs` の module 形を取り
/// （設計 docs/design/rules-manifest.md §2・憲法 R-C13-2 は target 数で数える）、
/// `crates/<c>/tests/<dir>/<f>.rs` は 5 段になる。4 段に限ると module 形の test file が
/// `not-copied` へ落ち、新しい test が base へ写らないまま rc 0 が出る。
fn is_test_file(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    if !rel.ends_with(".rs") || parts.first() != Some(&"crates") || parts.len() < 4 {
        return false;
    }
    if parts.get(2) == Some(&"tests") {
        return true;
    }
    // `#[path]` で src 配下へ外出しした test module は `#[cfg(test)] mod` の形を持たず、
    // 区間判定には **src 区間だけの file** に見える＝そこへ足した歯が 1 本も測られない。
    // 名前で test file と見なして丸ごと写す（base に mod 宣言が在れば base で compile
    // され、新しい歯の RED を測れる）。
    parts.get(2) == Some(&"src")
        && parts
            .last()
            .is_some_and(|name| *name == "tests.rs" || name.ends_with("_tests.rs"))
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
    write_text(dest, &pair.rel, &body)?;
    Ok(true)
}

/// 本文 1 本を base tree の `rel` へ書く（親 dir は作る）。
fn write_text(dest: &Path, rel: &str, body: &str) -> Result<(), String> {
    let path = dest.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("{} を作れない: {err}", parent.display()))?;
    }
    fs::write(&path, body).map_err(|err| format!("{} を書けない: {err}", path.display()))?;
    emit_err(&format!("flip-check: changed {rel}"));
    Ok(())
}

/// overlay を base tree へ書く。**数えるのは [`Counts`] の仕事**（判定行の数を
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

/// 便 1 本の内訳（判定行に載る数）。
#[derive(Debug, Clone, Copy, Default)]
struct Counts {
    /// base で赤くなることを要求した file の本数。
    flipped: usize,
    /// 削除・移動だけゆえ flip に数えなかった本数。
    removed: usize,
    /// marker で RED を免除した本数。
    retro: usize,
    /// 純粋な移動の札で RED を免除した本数。
    moved: usize,
    /// 本体 file へ同梱した宣言 file の本数。
    decl: usize,
}

impl Counts {
    /// 便の pairs から数える。
    fn of(pairs: &[FilePair]) -> Self {
        Self {
            flipped: pairs.iter().filter(|pair| pair.flips()).count(),
            removed: pairs.iter().filter(|pair| pair.removed_only()).count(),
            retro: pairs.iter().filter(|pair| pair.retroactive()).count(),
            moved: pairs.iter().filter(|pair| pair.moved()).count(),
            // 同梱した本数は base を実体化する段（[`run_on_base`]）で決まる。
            decl: 0,
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
    if counts.moved > 0 {
        line.push_str(&format!(" moved={}", counts.moved));
    }
    if counts.decl > 0 {
        line.push_str(&format!(" decl={}", counts.decl));
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
fn judge_each(dest: &Path, target: &Path, plan: &Plan, counts: Counts) -> Verdict {
    // flip しない pair は overlay しても内容が base と同じ（base の src + 同一の test 区間）
    // なので、先にまとめて置く。ここで置いても base の緑は動かない。
    for pair in plan.pairs.iter().filter(|pair| !pair.flips()) {
        if let Err(reason) = write_one(dest, pair) {
            return infra(&reason);
        }
    }
    // 宣言 file（`mod x;` だけの差分）は**単独では撃たず**、本体を撃つ turn ごとに
    // 同梱する（[`bundle_decls`]）。宣言と本体が別 file に割れる新規 module は、単独
    // overlay ではどちらの判定も意味を持たないからである（[`FilePair::declaration_only`]）。
    for pair in &plan.bodies {
        emit_err(&format!("flip-check: test-diff {}", pair.rel));
        let path = dest.join(&pair.rel);
        let restore = match swap_in(dest, pair) {
            Err(reason) => return infra(&reason),
            Ok(found) => found,
        };
        // **本体を置いた後**に絞る（絞り込みは dest の実体で本体の在処を見る）。
        let judged = match bundle_decls(dest, &plan.decls) {
            Err(reason) => Err(infra(&reason)),
            Ok(()) => match nextest(dest, target) {
                Err(reason) => Err(infra(&reason)),
                Ok(output) => {
                    relay(&format!("overlay {}", pair.rel), &output);
                    judge_one(&output, Some(&pair.rel))
                }
            },
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

/// 宣言 file を **その turn の tree に本体が在る `mod <name>;` 行だけ**へ絞って置く。
///
/// 便の宣言行を全部置くと、その turn ではまだ置かれていない兄弟 module の `E0583` が
/// 「overlay 後の compile error は RED」の規則で RED に化け、撃っている本体が base で
/// **緑でも隠れる**（実測 2026-09-10・`s2-07l.41` が入れた fail-open。本体 2 本とも緑の便が
/// `RED-on-base ok decl=2` で通った）。turn ごとに絞れば、その turn に木へ載っている
/// 本体の宣言だけが compile 対象になる。
fn bundle_decls(dest: &Path, decls: &[&FilePair]) -> Result<(), String> {
    for pair in decls {
        emit_err(&format!("flip-check: decl-with-body {}", pair.rel));
        let Some(body) = pair.overlay() else {
            emit_err(&format!("flip-check: not-copied {}", pair.rel));
            continue;
        };
        write_text(
            dest,
            &pair.rel,
            &present_mods_only(dest, &pair.rel, &body, &pair.base_test()),
        )?;
    }
    Ok(())
}

/// 宣言 file の本文から、**この便が足した** `mod <name>;` 行のうち `dest` に本体が
/// 無いものだけを落とす。
///
/// `mod` 行**以外は 1 行も触らない**（宣言 file は `use` や helper を持ちうる。落とすと
/// 本体が compile できず、これも捏造 RED になる）。本体の在処は宣言 file と同じ dir の
/// `<name>.rs` か `<name>/mod.rs` で見る。
///
/// **base に既に在った宣言行は落とさない**——base が緑である以上（[`base_is_green`]）
/// その本体は必ず在り、落とす理由が無い。`#[path = "…"]` 付きの宣言まで落とすと属性行
/// （`mod` 行ではないので残る）が**孤児**になり、`expected item after attributes` の
/// compile error が RED に化ける＝**この関数が消しに来た当の fail-open を別の扉から
/// 作り直す**（実測 2026-09-10・lens-44 H1: base の xtask は正しく `green-on-base` で
/// 落ちるのに、絞り込みを入れた側が `RED-on-base ok` で通した）。
///
/// ゆえに `#[path]` 付き module について本関数がするのは「壊さない」ことだけで、
/// **救済はしない**——path 属性の指す先は字面から追えず、追うには parser が要る。
/// この便が `#[path]` 付きの新規 module を足した周は従来どおり測れない（M4・記録のみ）。
fn present_mods_only(dest: &Path, rel: &str, body: &str, base: &str) -> String {
    let dir = dest.join(Path::new(rel).parent().unwrap_or(Path::new("")));
    let carried: Vec<&str> = base.lines().filter_map(mod_name).collect();
    body.split_inclusive('\n')
        .filter(|line| match mod_name(line) {
            None => true,
            Some(name) => {
                carried.contains(&name)
                    || dir.join(format!("{name}.rs")).is_file()
                    || dir.join(name).join("mod.rs").is_file()
            }
        })
        .collect()
}

/// 1 便の overlay 対象（judge_each が要る 3 つの集合）。
struct Plan<'a> {
    /// 便の全 pair（flip しない pair は先にまとめて置く）。
    pairs: &'a [FilePair],
    /// 本体と同梱する宣言 file（単独では撃たない）。
    decls: Vec<&'a FilePair>,
    /// 1 本ずつ単独で撃つ本体 file。
    bodies: Vec<&'a FilePair>,
}

/// flip した pair を「宣言 file」と「本体 file」へ割る。
///
/// **宣言だけの便は割らない**（本体が 1 本も無ければ従来どおり単独で撃つ）。存在しない
/// module を指す `mod x;` だけの便も base では `E0583` で赤くなるが、それは**本当の**
/// RED であって、同梱で消してよいものではない。
fn plan_of<'a>(pairs: &'a [FilePair], flipping: &[&'a FilePair]) -> Plan<'a> {
    let (decls, bodies): (Vec<&FilePair>, Vec<&FilePair>) = flipping
        .iter()
        .copied()
        .partition(|pair| pair.declaration_only());
    if bodies.is_empty() {
        return Plan { pairs, decls: Vec::new(), bodies: decls };
    }
    Plan { pairs, decls, bodies }
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
    let plan = plan_of(pairs, &flipping);
    let counts = Counts {
        decl: plan.decls.len(),
        ..counts
    };
    if plan.bodies.len() >= 2 || !plan.decls.is_empty() {
        return judge_each(&dest, &target, &plan, counts);
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
    judge_into(base, workdir, &mut emit_err)
}

/// [`judge`] の本体。効かない札の行（`stale-marker`）を `sink` へ渡す。
///
/// stderr へ直に書くと、**出したこと自体を歯から読めない**——`stale-marker` の行は
/// 判定行にも rc にも載らないので、emit を丸ごと消しても全部の歯が緑のままになる
/// （実測 2026-09-10・s2-07l.34 の lens F1）。CLI 面の出力は [`judge`] が
/// `emit_err` を渡すので変わらない。
fn judge_into(base: &str, workdir: &Path, sink: &mut dyn FnMut(&str)) -> Verdict {
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
    // **test 区間が動いた便にだけ**言う。札は file に残るので、`stale_marker()` だけで
    // 数えると、その file の src を触るたびに「札を削除しろ」と言われる——免除を
    // 求めていない便には無関係な指示で、狼少年にすると本当に効かない札を見落とす。
    for pair in pairs
        .iter()
        .filter(|pair| pair.stale_marker() && pair.test_diff())
    {
        sink(&format!(
            "flip-check: stale-marker {}（base に既に在る marker は効かない\
             ・削除するか新しい bead id で置き直す）",
            pair.rel
        ));
    }
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
             後から足す歯なら test 区間へ `// flip-check: retroactive <bead-id>` を、\
             歯を足さない純粋な移動なら `// flip-check: moved <bead-id>` を 1 行置く",
        );
        return fail(&format!("not-flippable files={}", stuck.join(",")));
    }
    for pair in pairs.iter().filter(|pair| pair.removed_only()) {
        emit_err(&format!(
            "flip-check: not-flipped reason=tests-removed-only {}",
            pair.rel
        ));
    }
    if counts.removed > 0 || counts.retro > 0 || counts.moved > 0 {
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
#[path = "flipcheck_tests.rs"]
mod tests;
