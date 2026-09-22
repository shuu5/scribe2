//! `cargo xtask flip-check --base <ref>` の本体。TDD の flip を後から機械で確かめる。
//!
//! HEAD の **test 区間**を base の **src 区間**の上へ重ねた木で runner を撃ち、
//! 「新しい test が古い実装で赤い」ことを見る。依存は std だけで、外部 binary は
//! git と tar の 2 本である（呼出は [`std::process::Command`]）。
//!
//! **honest fence**: git / tar / cargo の spawn 失敗と、diff / rev-parse / archive /
//! tar / init / update-ref / add / base 健全性前段 / runner 不在の rc≠0 は例外なく `reason=infra-error` として
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

mod git;
mod nextest;

pub use git::{changed_files, changed_rs, materialize_base, parse_base, repo_root, work_dir};
// `load_pairs` は private 型 `FilePair` を返すので `pub(super)` のまま名指しで読む（`private_interfaces`）。
use git::load_pairs;
use nextest::{failed_tests, nextest, nextest_with, relay, trimmed};

use crate::check::{read_text, RULES_REL};
use crate::limits::FlipLimits;
use crate::{emit, emit_err};
use std::fs;
use std::path::Path;
use std::process::{ExitCode, Output};

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

    /// **歯の外の file** か（その便で動いた行が 1 本も歯の中に無い・§37）。
    ///
    /// helper・共有の fixture・宣言以外の作りだけが動いた test file は、単独 overlay が base の
    /// 歯を 1 本も動かさず**構造的に RED になりようがない**（実測 2026-09-17: `s2-07l.447` ×2・
    /// `s2-07l.412` ×1 が `green-on-base` で gate 1 周を失った）。宣言 file と同じ側＝単独では
    /// 撃たず本体を撃つ木へ同梱する（[`plan_of`]）。
    ///
    /// 「歯の中」は [`teeth_lines`] の読みで、**変更行の字面が base 側か HEAD 側のどちらかの歯の中に
    /// 1 度でも現れれば歯の中**へ倒す（`}` のように重複する字面は歯の中＝同梱は判定を緩める側
    /// なので弁別は狭く取る）。字面は trim して比べる（字下げの差で歯の外へ倒れる方が緩い側）。
    /// 宣言 file の弁別が先で、この判定は `mod` 行の差にも当たるが [`plan_of`] が先に宣言 file を
    /// 取り分ける。**歯を 1 本も持たない file（両側とも）は歯の外の file と読まない**——「歯の外」
    /// は歯が在ってこその弁別で、宣言 file の形を外れた行（`pub(crate)mod x;` 等）だけの file を
    /// 絞り込み無しで同梱する扉にしない（出所の母集団も歯を持つ file だけ・§37）。brace を数える
    /// parser は足さない。
    fn outside_teeth(&self) -> bool {
        let (base, head) = (self.base_test(), self.head_test());
        let changed = changed_lines(&base, &head);
        let teeth: Vec<&str> = teeth_lines(&base)
            .into_iter()
            .chain(teeth_lines(&head))
            .map(str::trim)
            .collect();
        !changed.is_empty()
            && !teeth.is_empty()
            && !changed.iter().any(|line| teeth.contains(&line.trim()))
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

/// 札の bead id の**閉じた形**（設計 pipeline.md §7 約束 2・`s2-07l.170`）: `<接頭辞>-<段>(.<段>)*`。
///
/// 接頭辞は英小文字で始まる英小文字と数字の列、段は空でない英小文字と数字の列（`s2-07l.91` / `s2-07l.479.2` /
/// `s2-07l.37x`）。空白・大文字・記号・空の段を持つ字面（`TODO`・`s2-07l..3`・`s2-07l.91 (理由)`）は形に合わない
/// ——誰にも辿れない札を免除の鍵にしない。land の trailer の run id の頭（`main-provenance`）も同じ形で見る。
pub(crate) fn is_bead_id(id: &str) -> bool {
    let segment = |part: &str| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit());
    let Some((prefix, rest)) = id.split_once('-') else {
        return false;
    };
    prefix.starts_with(|ch: char| ch.is_ascii_lowercase()) && segment(prefix) && rest.split('.').all(segment)
}

/// 便が**この便で足した**札（両方の札・test 区間の行）を `(rel, bead id)` で数えた列。
fn fresh_marks(pairs: &[FilePair]) -> Vec<(&str, String)> {
    pairs
        .iter()
        .flat_map(|pair| {
            [RETROACTIVE_MARK, MOVED_MARK]
                .into_iter()
                .flat_map(move |mark| pair.fresh_markers(mark).into_iter().map(move |bead| (pair.rel.as_str(), bead)))
        })
        .collect()
}

/// 札の形と本数の門（`bad-marker` / `too-many-marks`）。通れば `None`。
///
/// 数えるのは **この便で足した**札だけ（持ち越した札は [`FilePair::fresh_markers`] が落とす）——file に残った
/// 古い札で後続の便を落とさない。形の門が先（形に合わない札は本数に数える前に名指す）。
fn marks_verdict(pairs: &[FilePair], limit: u64, sink: &mut dyn FnMut(&str)) -> Option<Verdict> {
    let marks = fresh_marks(pairs);
    let bad: Vec<&(&str, String)> = marks.iter().filter(|(_, bead)| !is_bead_id(bead)).collect();
    for (rel, bead) in &bad {
        sink(&format!("flip-check: bad-marker {rel} id={bead:?}（札の bead id は <接頭辞>-<段>(.<段>)* の形）"));
    }
    if let Some((rel, _)) = bad.first() {
        return Some(fail(&format!("bad-marker file={rel}")));
    }
    let count = u64::try_from(marks.len()).unwrap_or(u64::MAX);
    (count > limit).then(|| fail(&format!("too-many-marks marks={count} limit={limit}")))
}

/// path が docs-only の面に入るか（`/` で終わる面は接頭辞・他は path の完全一致）。
fn in_docs_faces(rel: &str, faces: &[String]) -> bool {
    faces
        .iter()
        .any(|face| if face.ends_with('/') { rel.starts_with(face.as_str()) } else { rel == face })
}

/// `.rs` の差が 1 本も無い便の判定（設計 pipeline.md §7 約束 1・`s2-07l.170`）。
///
/// 動いた path が**全部**面（rules 行 `flip.docs_only_faces`）の中なら `docs-only=N` の後置で通し、面の外の file を
/// 1 本でも含む便（や差が 1 本も無い便）は `no-test-diff` で落とす。以前の `.rs` の有無だけで通す経路は、
/// `plugin/` の生成物や `rules/` の規則だけを変えた便を歯無しで通していた（監査 2026-09-12 塊 14）。
fn docs_only_verdict(base: &str, root: &Path, faces: &[String], sink: &mut dyn FnMut(&str)) -> Verdict {
    let changed = match changed_files(base, root) {
        Err(reason) => return infra(&reason),
        Ok(list) => list,
    };
    let outside: Vec<&String> = changed.iter().filter(|rel| !in_docs_faces(rel, faces)).collect();
    for rel in &outside {
        sink(&format!("flip-check: outside-docs-faces {rel}"));
    }
    if changed.is_empty() || !outside.is_empty() {
        return fail("no-test-diff");
    }
    ok_line(Counts {
        docs_only: changed.len(),
        ..Counts::default()
    })
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

/// `#[test]` の行頭の印。
const TEST_ATTR: &str = "#[test]";

/// test 区間の**歯の中**の行（§37）。
///
/// 歯は **`#[test]` の直下の `fn` の宣言行から次の `fn` の宣言行の手前まで**（属性・doc・空行は
/// 跨ぐ＝`#[test]` の後に他の行が先に来ればその `#[test]` は歯を開かない）。次の `fn` が
/// `#[test]` を持たない helper なら、そこで歯は閉じる。歯の中に在る `#[test]` の属性行や
/// 閉じ brace は歯の中に数える——brace を数える parser は足さない（重複する字面が歯の中へ
/// 倒れるのは狭く取る側）。
fn teeth_lines(region: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut pending = false;
    let mut inside = false;
    for line in region.lines() {
        let trimmed = line.trim();
        if trimmed == TEST_ATTR {
            pending = true;
        } else if is_fn_line(trimmed) {
            inside = pending;
            pending = false;
        } else if pending && !(trimmed.is_empty() || trimmed.starts_with("#[") || trimmed.starts_with("//")) {
            pending = false;
        }
        if inside {
            found.push(line);
        }
    }
    found
}

/// `fn` の宣言行か（`pub` / `pub(…)` / `async` / `const` / `unsafe` の前置きは跨ぐ）。
///
/// 見るのは行頭だけで、行の途中の `fn `（コメントや closure の言及）は宣言と読まない。
fn is_fn_line(trimmed: &str) -> bool {
    let mut rest = trimmed;
    loop {
        if let Some(name) = rest.strip_prefix("fn ") {
            return name
                .trim_start()
                .starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_');
        }
        let Some((word, tail)) = rest.split_once(char::is_whitespace) else {
            return false;
        };
        let keyword = matches!(word, "pub" | "async" | "const" | "unsafe")
            || (word.starts_with("pub(") && word.ends_with(')'));
        if !keyword {
            return false;
        }
        rest = tail.trim_start();
    }
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

/// runner の出力から名指せた、落ちた歯 1 本。
#[derive(Debug, Clone, PartialEq, Eq)]
struct FailedTest {
    /// nextest の binary id（`<crate>` / `<crate>::<target>` / `<crate>::bin/<name>`）。
    binary: String,
    /// 歯の名（module path 込み）。
    name: String,
}

impl FailedTest {
    /// この歯 **1 本だけ**に当たる filterset（binary id と名の**完全一致**）。
    ///
    /// 部分一致（`test(foo)`）で名指すと同名を含む隣の歯まで撃ち直し、その rc で base の
    /// 緑を読むことになる——撃ち直しは緩める側なので、当たる範囲は狭く取る。
    fn filterset(&self) -> String {
        format!("(binary_id(={}) & test(={}))", self.binary, self.name)
    }
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
    /// 本体 file へ同梱した歯の外の file の本数（§37・`s2-07l.450`）。
    fixture: usize,
    /// base 段で撃ち直して緑と読んだ歯の本数（負荷下の flaky の検出線・`s2-07l.270`）。
    base_retried: usize,
    /// `.rs` の差が無く、docs-only の面の中だけが動いた path の本数（`s2-07l.170`）。
    docs_only: usize,
}

impl Counts {
    /// 便の pairs から数える。
    fn of(pairs: &[FilePair]) -> Self {
        Self {
            flipped: pairs.iter().filter(|pair| pair.flips()).count(),
            removed: pairs.iter().filter(|pair| pair.removed_only()).count(),
            retro: pairs.iter().filter(|pair| pair.retroactive()).count(),
            moved: pairs.iter().filter(|pair| pair.moved()).count(),
            // 同梱した本数と撃ち直した本数は base を実体化する段（[`run_on_base`]）で決まる。
            decl: 0,
            fixture: 0,
            base_retried: 0,
            docs_only: 0,
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
    if counts.base_retried > 0 {
        line.push_str(&format!(" base-retried={}", counts.base_retried));
    }
    if counts.fixture > 0 {
        line.push_str(&format!(" fixture={}", counts.fixture));
    }
    if counts.docs_only > 0 {
        line.push_str(&format!(" docs-only={}", counts.docs_only));
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
    // 歯の外の file（helper・fixture だけの差分）も単独では撃たない（§37）。行は同梱する
    // file ごとに 1 本（turn ごとに繰り返さない）。
    for pair in &plan.fixtures {
        emit_err(&format!("flip-check: not-flipped reason=outside-teeth {}", pair.rel));
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
        // 歯の外の file は絞り込み無しでそのまま置く。**宣言の絞り込みより先**に置く——
        // 新規の歯の外の file が宣言の指す本体なら、先に在ってこそ `mod` 行が残る。
        // **本体を置いた後**に絞る（絞り込みは dest の実体で本体の在処を見る）。
        let judged = match bundle_fixtures(dest, &plan.fixtures).and_then(|()| bundle_decls(dest, &plan.decls)) {
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

/// 歯の外の file を**絞り込み無しでそのまま**置く（§37）。
///
/// 宣言 file と違い落とす行が無い——動いた行は歯の外（helper・fixture）で、本体の歯が
/// それを呼ぶからこそ同梱する。写せない pair は [`FilePair::flips`] の `overlay().is_some()`
/// で来ないが、[`write_one`] の形をそのまま通す（別の書き口を作らない）。
fn bundle_fixtures(dest: &Path, fixtures: &[&FilePair]) -> Result<(), String> {
    for pair in fixtures {
        write_one(dest, pair)?;
    }
    Ok(())
}

/// 宣言 file の本文から、**この便が足した** `mod <name>;` 行のうち `dest` に本体が
/// 無いものだけを落とす。
///
/// `mod` 行**以外は 1 行も触らない**（宣言 file は `use` や helper を持ちうる。落とすと
/// 本体が compile できず、これも捏造 RED になる）。本体の在処は宣言 file と同じ dir の
/// `<name>.rs` か `<name>/mod.rs`、または宣言 file が `main.rs` / `mod.rs` でない周の
/// `<stem>/<name>.rs`（`tests/e2e/seat.rs` → `tests/e2e/seat/statusline.rs`・Rust の規則）
/// の 3 形で見る（§31・s2-07l.410）。
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
    let rel = Path::new(rel);
    let dir = dest.join(rel.parent().unwrap_or(Path::new("")));
    // 宣言 file が `<stem>.rs` なら子 module は `<stem>/<name>.rs` に置かれる。
    let nested = dir.join(rel.file_stem().unwrap_or_default());
    let carried: Vec<&str> = base.lines().filter_map(mod_name).collect();
    body.split_inclusive('\n')
        .filter(|line| match mod_name(line) {
            None => true,
            Some(name) => {
                carried.contains(&name)
                    || dir.join(format!("{name}.rs")).is_file()
                    || dir.join(name).join("mod.rs").is_file()
                    || nested.join(format!("{name}.rs")).is_file()
            }
        })
        .collect()
}

/// 1 便の overlay 対象（judge_each が要る 4 つの集合）。
struct Plan<'a> {
    /// 便の全 pair（flip しない pair は先にまとめて置く）。
    pairs: &'a [FilePair],
    /// 本体と同梱する宣言 file（単独では撃たない・`mod` 行の絞り込み付き）。
    decls: Vec<&'a FilePair>,
    /// 本体と同梱する歯の外の file（単独では撃たない・絞り込み無し・§37）。
    fixtures: Vec<&'a FilePair>,
    /// 1 本ずつ単独で撃つ本体 file。
    bodies: Vec<&'a FilePair>,
}

/// flip した pair を「同梱する側（宣言 file ∨ 歯の外の file）」と「単独で撃つ本体 file」へ割る。
///
/// **同梱する側だけの便は割らない**（本体が 1 本も無ければ従来どおり全部を単独で撃つ）。
/// 存在しない module を指す `mod x;` だけの便も base では `E0583` で赤くなるが、それは
/// **本当の** RED であって、同梱で消してよいものではない。歯の外の file しか flip しない便も
/// 同じ落とし方＝`green-on-base` のまま落ちる（fail-closed・§37）。宣言 file の弁別が先
/// （`mod` 行の差は歯の外にも当たるので、先に取り分けないと絞り込みを持たない側へ倒れる）。
fn plan_of<'a>(pairs: &'a [FilePair], flipping: &[&'a FilePair]) -> Plan<'a> {
    let mut decls = Vec::new();
    let mut fixtures = Vec::new();
    let mut bodies = Vec::new();
    for pair in flipping.iter().copied() {
        if pair.declaration_only() {
            decls.push(pair);
        } else if pair.outside_teeth() {
            fixtures.push(pair);
        } else {
            bodies.push(pair);
        }
    }
    if bodies.is_empty() {
        return Plan {
            pairs,
            decls: Vec::new(),
            fixtures: Vec::new(),
            bodies: flipping.to_vec(),
        };
    }
    Plan { pairs, decls, fixtures, bodies }
}

/// base 段が「名指せない失敗」へ倒れた経路（設計 docs/design/pipeline.md §32・`s2-07l.380`）。
///
/// 宣言順は (i) signal（rc 無し）(ii) 名指し 0（compile error の rc 101・出力の形が読めない）
/// (iii) 名指した歯の撃ち直しが rc≠0。**判定クラスは増えない**——3 つとも `infra-error` の
/// `base-not-green` で、後置の弁別子だけが違う（極性一覧の行は不変）。操作役はこの後置で
/// 負荷 / 環境 / 本物の赤を判定行から分け、retire か run N+1 かを推測で決めない（C10）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaseNotGreen {
    /// base の runner が rc を持たない（signal で死んだ）。
    Signal,
    /// rc≠0 だが落ちた歯を 1 本も名指せない。
    Unnamed { rc: i32 },
    /// 名指した歯の撃ち直しが rc≠0（rc 4 = filterset に該当 0 本を含む）。
    RetryFailed { rc: i32 },
}

impl BaseNotGreen {
    /// [`infra`] の理由に置く字面（先頭 `flip-check: FAIL reason=infra-error ` は `infra` のまま）。
    fn label(self) -> String {
        match self {
            Self::Signal => "base-not-green:signal".to_owned(),
            Self::Unnamed { rc } => format!("base-not-green:unnamed rc={rc}"),
            Self::RetryFailed { rc } => format!("base-not-green:retry-failed rc={rc}"),
        }
    }
}

/// 「名指せない失敗」を経路へ写す純関数（base 段の 3 か所はここを通る）。
///
/// 入力は base の rc（`None` = signal）・名指した歯の本数・撃ち直しの rc（撃っていない周と
/// 撃ち直しが signal で死んだ周は `None`）。撃っていない周は名指し 0 の側で決まるので、
/// 名指しが在って撃ち直しの rc が無い形だけが signal へ倒れる。
fn base_not_green(base_rc: Option<i32>, named: usize, retry_rc: Option<i32>) -> BaseNotGreen {
    match (base_rc, named) {
        (None, _) => BaseNotGreen::Signal,
        (Some(rc), 0) => BaseNotGreen::Unnamed { rc },
        (Some(_), _) => match retry_rc {
            None => BaseNotGreen::Signal,
            Some(rc) => BaseNotGreen::RetryFailed { rc },
        },
    }
}

/// base tree の健全性前段。overlay を書く前に base のまま runner を撃つ。
///
/// 通れば `Ok(撃ち直した歯の本数)`。**base 自身の緑は前提であって判定対象ではない**
/// （C12.2）——main は常に緑（C12.6）なので、ここで落ちる歯は負荷か環境の赤である。
/// 落ちた歯を出力から名指せる周だけ [`retry_named`] で **1 回**撃ち直し、通れば base 緑と
/// 読む。名指せない周（compile error の rc 101・signal で rc 無し・出力の形が読めない）は
/// 従来どおり `base-not-green`＝名指せない失敗を撃ち直しで緑に化けさせない（C11.2）。
/// どの経路で倒れたかは [`base_not_green`] が写し、字面の後置で名指す。
/// `sink` は `base-retry` の診断行の出口（[`judge_into`] と同じ理由で stderr へ直に書かない）。
fn base_is_green(dest: &Path, target: &Path, sink: &mut dyn FnMut(&str)) -> Result<usize, Verdict> {
    let output = match nextest(dest, target) {
        Err(reason) => return Err(infra(&reason)),
        Ok(output) => output,
    };
    relay("base", &output);
    match output.status.code() {
        Some(0) => Ok(0),
        Some(rc) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            retry_named(dest, target, rc, &failed_tests(&text), sink)
        }
        None => Err(infra(&base_not_green(None, 0, None).label())),
    }
}

/// 名指せた歯だけを同じ base copy・同じ target dir で **1 回だけ**撃ち直す。
///
/// 撃ち直しの rc が 0 のときだけ `Ok(本数)`。0 本（名指せない）・rc≠0（rc 4 = filterset
/// に該当 0 本を含む）・起動失敗はすべて `base-not-green`——**2 回目は撃たない**。
/// 撃ち直しは緩める側なので、当たる範囲（完全一致）も回数（1 回）も狭く取る。
/// `base_rc` は素の撃ちの rc で、経路の弁別子（`unnamed rc=<rc>`）にだけ使う。
fn retry_named(
    dest: &Path,
    target: &Path,
    base_rc: i32,
    failed: &[FailedTest],
    sink: &mut dyn FnMut(&str),
) -> Result<usize, Verdict> {
    if failed.is_empty() {
        return Err(infra(&base_not_green(Some(base_rc), 0, None).label()));
    }
    for test in failed {
        sink(&format!("flip-check: base-retry {}::{}", test.binary, test.name));
    }
    let expr = failed
        .iter()
        .map(FailedTest::filterset)
        .collect::<Vec<String>>()
        .join(" | ");
    let output = match nextest_with(dest, target, &["-E", &expr]) {
        Err(reason) => return Err(infra(&reason)),
        Ok(output) => output,
    };
    relay("base-retry", &output);
    if output.status.success() {
        Ok(failed.len())
    } else {
        let reason = base_not_green(Some(base_rc), failed.len(), output.status.code());
        Err(infra(&reason.label()))
    }
}

/// base を実体化し健全性を確かめ overlay を書いて runner を撃つ。
fn run_on_base(
    base: &str,
    root: &Path,
    pairs: &[FilePair],
    counts: Counts,
    sink: &mut dyn FnMut(&str),
) -> Verdict {
    let dest = match materialize_base(base, root) {
        Err(reason) => return infra(&reason),
        Ok(found) => found,
    };
    let target = work_dir(root).join("target");
    let base_retried = match base_is_green(&dest, &target, sink) {
        Err(blocked) => return blocked,
        Ok(retried) => retried,
    };
    // flip した file が 2 本以上なら **1 本ずつ**撃つ（まとめ撃ちは偽の RED を作る）。
    // 1 本のときは従来どおり 1 回で足りる（分ける対象が無い）。
    let flipping: Vec<&FilePair> = pairs.iter().filter(|pair| pair.flips()).collect();
    let plan = plan_of(pairs, &flipping);
    // 同梱した歯の外の file は「base で赤くなることを要求した」側から外す（`tests_changed` は
    // 単独で撃った本数）。宣言 file の数え方は従来のまま動かさない。
    let counts = Counts {
        flipped: counts.flipped - plan.fixtures.len(),
        decl: plan.decls.len(),
        fixture: plan.fixtures.len(),
        base_retried,
        ..counts
    };
    if plan.bodies.len() >= 2 || !plan.decls.is_empty() || !plan.fixtures.is_empty() {
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

/// [`judge`] の本体。効かない札の行（`stale-marker`）・base 段の撃ち直しの行（`base-retry`）・
/// 形に合わない札の行（`bad-marker`）・docs-only の面の外の path の行（`outside-docs-faces`）を `sink` へ渡す。
///
/// stderr へ直に書くと、**出したこと自体を歯から読めない**——`stale-marker` の行は
/// 判定行にも rc にも載らないので、emit を丸ごと消しても全部の歯が緑のままになる
/// （実測 2026-09-10・s2-07l.34 の lens F1）。`base-retry` も同じで、「撃ち直していない」
/// ことは行の不在でしか測れない。CLI 面の出力は [`judge`] が `emit_err` を渡すので変わらない。
fn judge_into(base: &str, workdir: &Path, sink: &mut dyn FnMut(&str)) -> Verdict {
    let root = match repo_root(workdir) {
        Err(reason) => return infra(&reason),
        Ok(found) => found,
    };
    // 免除経路の上限は rules 行（C1）。読めない周は測れなかった（面も札の上限も既定に倒さない）。
    let flip = match read_text(&root.join(RULES_REL)).and_then(|text| FlipLimits::read(&text)) {
        Err(reason) => return infra(&reason),
        Ok(found) => found,
    };
    let changed = match changed_rs(base, &root) {
        Err(reason) => return infra(&reason),
        Ok(list) => list,
    };
    if changed.is_empty() {
        return docs_only_verdict(base, &root, &flip.docs_only_faces, sink);
    }
    let pairs = match load_pairs(base, &root, &changed) {
        Err(reason) => return infra(&reason),
        Ok(found) => found,
    };
    if let Some(refused) = marks_verdict(&pairs, flip.marks_per_pr, sink) {
        return refused;
    }
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
    let outcome = run_on_base(base, &root, &pairs, counts, sink);
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
use nextest::nextest_args;
#[cfg(test)]
#[path = "flipcheck_tests.rs"]
mod tests;
