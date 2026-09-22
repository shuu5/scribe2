//! 判定の読み手と受付の 2 門の群（設計 docs/design/contract-source.md §38・契約表の行 am・`s2-07l.545`）。
//!
//! 便の `review.json` を読む判定の読み手（[`judgement_of`]）と、受付の 2 門（§23）が直前の便の指摘の対応を kind
//! ごとに測る物差し（[`unaddressed`]）の群である。`pipe/review.rs` からの**純移動**で、歯は 1 本も足していない（親に
//! 残る in-file の歯が従来どおり測る）。外の呼び手は親の `pub use` を通って `review::` の path のまま入る。親の私有
//! item は `super::` でそのまま見えるので、**親側の可視性は 1 語も上げていない**。歯だけが読む `split_at` だけが
//! `pub(super)` で、残りの私有 item はこの module に閉じる。

// flip-check: moved s2-07l.545

use super::{json_lite, FindingKind, Verdict, REVIEW_DIR, REVIEW_FILE};
use crate::pipe::closure::{unresolved_names, ClosureError, Source};
use crate::pipe::refuse::covered;
use crate::pipe::run_dir;
use std::path::{Path, PathBuf};

/// 便の `review.json`。
pub fn review_path(state_dir: &Path, id: &str) -> PathBuf {
    run_dir(state_dir, id).join(REVIEW_FILE)
}

/// 審査の材料の dir（`<run_dir>/review/`）。
pub fn review_dir(state_dir: &Path, id: &str) -> PathBuf {
    run_dir(state_dir, id).join(REVIEW_DIR)
}

/// `review.json` から 3 値を読む。読めない周は `None`（＝PASS ではない）。読み手は [`judgement_of`] の 1 本。
pub fn verdict_of(state_dir: &Path, id: &str) -> Option<Verdict> {
    judgement_of(state_dir, id).map(|found| found.verdict)
}

/// 同型の停止の回数を持つ rules 行の id（設計 contract-source.md §23 (1)・値は manifest だけが持つ・C1）。読み手は
/// 受付（`cli/intake.rs`）で、断りの理由の 1 行（`pipe::refuse`）が同じ字面で行を名指す。
pub const ROW_SAME_KIND_STOP: &str = "review.same_kind_stop";

/// 便の `review.json` の判定（受付の 2 門〔§23〕が読む形・verdict と理由の型と指した場所の列）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgement {
    /// 3 値。
    pub verdict: Verdict,
    /// 理由の型。PASS は `None`・PASS でない周は必ず `Some`（`kind` を持たない §22 の前の便と語でない周は
    /// [`FindingKind::Unparsed`]＝lens の欠けを契約の型に化けさせない・C10）。
    pub kind: Option<FindingKind>,
    /// `at` の項目（`,` 区切りの語を割り、空白を剥がし、空を除いた列・書かれていない周は空）。
    pub at: Vec<String>,
}

/// `review.json` から判定を読む（**判定の読み手はこの 1 本**・[`verdict_of`] と [`ReviewCheck::judge`] も通る）。
/// file が無い・壊れている・3 値の外は `None`（呼び手が断る側へ倒す・fail-closed）。
pub fn judgement_of(state_dir: &Path, id: &str) -> Option<Judgement> {
    let text = std::fs::read_to_string(review_path(state_dir, id)).ok()?;
    let pairs = json_lite::parse_object(text.trim()).ok()?;
    let get = |key: &str| pairs.iter().find(|(found, _)| found == key).and_then(|(_, value)| value.as_str());
    let verdict = get("verdict").and_then(Verdict::parse)?;
    let kind = match verdict {
        Verdict::Pass => None,
        Verdict::Fail | Verdict::Inconclusive => {
            Some(get("kind").and_then(FindingKind::parse).unwrap_or(FindingKind::Unparsed))
        }
    };
    Some(Judgement { verdict, kind, at: split_at(get("at").unwrap_or_default()) })
}

/// lens の `at`（`,` 区切りの語の列）を項目に割る（空白を剥がし、空の項目は落とす）。
pub(super) fn split_at(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|item| !item.is_empty()).map(str::to_owned).collect()
}

/// 焼き直しの門（§23 (3)）の今回の材料。受付が写す形の契約 file と base から読む節の本文（[`design_material`] の
/// 形）に、直前の便の `design.txt` と、名指しの読み手が要る base の面を添える。
pub struct Rework<'a> {
    /// 今回の write-set（弁別済み・dir 項目はその配下を含む・[`covered`]）。
    pub write_set: &'a [String],
    /// 今回の契約 file の字面。
    pub contract: &'a str,
    /// 今回の節の本文（[`design_material`] の形）。
    pub design: &'a str,
    /// 直前の便の材料の `design.txt`。
    pub previous_design: &'a str,
    /// 行の `touches`（名指しの読み手が variant を散文に読む型・§3）。
    pub touches: &'a [String],
    /// base の tracked file。
    pub tracked: &'a [String],
    /// base の `.rs`。
    pub sources: &'a [Source],
}

/// 直前の便の指摘（`kind` と `at`）のうち、今回の材料に「対応する差分」の**無い**項目（辞書順・重複なし）。
///
/// **kind ごとに 1 関数**（閉じた型の網羅 match・§23 (3)）: teeth-outside-write-set → `at` の path の形の各項目が今回の
/// write-set に在る（path でない項目は測れない・§35 [`teeth_unaddressed`]）／literal-mismatch → `at` の各識別子が今回の契約 file と節の本文に無い、または base に解ける（`NameUnresolved`
/// の名指しの読み手と同じ 1 本 [`unresolved_names`]）／section-material-missing → 節の本文が直前の便の `design.txt` と
/// 異なる。goal-done-contradiction / vacuous-assert / other / unparsed は**測れない＝空**（判断を要する型は planner に
/// 残す・C10）。読めない `.rs` が在る周は `Err`（黙って通さない）。
pub fn unaddressed(kind: FindingKind, at: &[String], rework: &Rework<'_>) -> Result<Vec<String>, ClosureError> {
    let found = match kind {
        FindingKind::TeethOutsideWriteSet => return Ok(teeth_unaddressed(at, rework)),
        FindingKind::LiteralMismatch => literal_unaddressed(at, rework)?,
        FindingKind::SectionMaterialMissing => section_unaddressed(at, rework),
        FindingKind::GoalDoneContradiction | FindingKind::VacuousAssert | FindingKind::Other | FindingKind::Unparsed => {
            Vec::new()
        }
    };
    Ok(sorted(found))
}

/// 辞書順・重複なしに畳む。
fn sorted(mut found: Vec<String>) -> Vec<String> {
    found.sort();
    found.dedup();
    found
}

/// teeth-outside-write-set: `at` のうち path の形の項目（[`path_shaped`]）で今回の write-set に無いもの（dir 項目は配下を
/// 含む・辞書順）。path でない項目（歯の接頭辞・§ の番号）は**測れない**＝照合しない（§35・どんな契約でも covered に
/// ならない項目で永遠に断らない）。未対応が在り、かつ測れない項目も在る周は、列の末尾に母集団の 1 項目
/// 「測った n 件・測れない m 件」（どちらも重複なしの件数）を足す（理由の文に測らなかった分を出す・C10）。path の項目が
/// 0 か全部対応済みの周は空＝通す。
fn teeth_unaddressed(at: &[String], rework: &Rework<'_>) -> Vec<String> {
    let (paths, others): (Vec<String>, Vec<String>) = at.iter().cloned().partition(|item| path_shaped(item, rework.tracked));
    let (paths, others) = (sorted(paths), sorted(others));
    let mut found: Vec<String> = paths.iter().filter(|path| !covered(rework.write_set, path)).cloned().collect();
    if !found.is_empty() && !others.is_empty() {
        found.push(format!("測った {} 件・測れない {} 件", paths.len(), others.len()));
    }
    found
}

/// `at` の項目が path の形に解けるか: base の tracked file か、空白を持たず（`+` を剥がした後に）`/` を含む字面
/// （`src/a.rs`・末尾 `/` の dir・`+` 付きの新規 file・§3 の write-set の項目の形）。歯の接頭辞・§ の番号・型の path
/// （`::`）は解けない。
fn path_shaped(item: &str, tracked: &[String]) -> bool {
    tracked.iter().any(|file| file == item)
        || (!item.contains(char::is_whitespace) && item.strip_prefix('+').unwrap_or(item).contains('/'))
}

/// literal-mismatch: `at` の識別子のうち、今回の契約 file か節の本文に**まだ在り**、かつ base に解けないもの。
/// 解けるかは名指しの読み手（backtick の 3 形・path / 型の path / fn）の 1 本で、3 形でない字面は名指しでない＝
/// 測れないので対応済みに数えない側ではなく**通す側**（読み手が `Prose` と読む）。
fn literal_unaddressed(at: &[String], rework: &Rework<'_>) -> Result<Vec<String>, ClosureError> {
    let mut found = Vec::new();
    for name in at {
        if !rework.contract.contains(name.as_str()) && !rework.design.contains(name.as_str()) {
            continue;
        }
        let texts = [("at".to_owned(), format!("`{name}`"))];
        let unresolved = unresolved_names(&texts, rework.touches, rework.write_set, rework.tracked, rework.sources)?;
        if !unresolved.is_empty() {
            found.push(name.clone());
        }
    }
    Ok(found)
}

/// section-material-missing: 節の本文が直前の便の `design.txt` と同じ字面なら `at` の全項目が未対応、違えば全部対応済み。
fn section_unaddressed(at: &[String], rework: &Rework<'_>) -> Vec<String> {
    if rework.design == rework.previous_design {
        at.to_vec()
    } else {
        Vec::new()
    }
}
