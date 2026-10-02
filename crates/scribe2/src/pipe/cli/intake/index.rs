//! 受付の索引の状態の扱いと閉包の判定（設計 docs/design/reverse-index.md §7 (b)・契約表の行 d・FR48 / FR107）。
//!
//! 受付の材料が持つ索引の状態（行 a2 の状態の読み・閉じた 6 値）と、行の `touches` と write-set から、**閉じた結果**
//! （[`Indexed`]）を返す。断りの組み立て（受付の断りの型・待ちの理由）は親の `intake.rs` が書く——この module は断りの型を
//! 組まず match もしない（ほかの行の touches の型を新しい file で名指すと、その行の閉包が広がる）。
//! 索引を要する行は `touches` に型の項目を持つ行だけで、持たない行はどの状態でも [`Indexed::Clear`] と尾なし（出力の字が変わらない）。

use crate::pipe::closure::{self, Source};
use crate::pipe::dispatch::index_build::Status;
use crate::pipe::refuse::covered;

/// 状態と閉包の判定の結果（断りの組み立ては親が書く）。
pub(super) enum Indexed {
    /// 通る（索引を要さない行・状態なし・undeclared・failed〔字面に縮退〕・閉包が write-set に収まる）。
    Clear,
    /// 作り中（状態の語は `absent` か `building`）。
    Building(&'static str),
    /// 索引の宣言の不備（key の名と行番号の行の列・状態が half）。
    Half(Vec<String>),
    /// 索引の閉包が名指し、字面の閉包が名指さず、write-set に無い file（file に ` (索引)` を添えた字）。
    Missing(Vec<String>),
}

/// 状態と閉包を判定する。`write_set` は行が手で列挙した write-set（導出する行は `None`＝閉包を測らない）、`sources` は base の `.rs`。
pub(super) fn judge(state: Option<&Status>, touches: &[String], write_set: Option<&[String]>, sources: &[Source]) -> Indexed {
    let types = closure::type_items(touches);
    let Some(state) = state.filter(|_| !types.is_empty()) else {
        return Indexed::Clear;
    };
    match state {
        Status::Absent => Indexed::Building("absent"),
        Status::Building => Indexed::Building("building"),
        Status::Half(errors) => Indexed::Half(errors.iter().map(ToString::to_string).collect()),
        Status::Undeclared | Status::Failed(_) => Indexed::Clear,
        Status::Ready(rows) => {
            let Some(write_set) = write_set else {
                return Indexed::Clear;
            };
            let literal = closure::closure(&types, sources).unwrap_or_default();
            let missing: Vec<String> = closure::index_closure(rows, &types)
                .into_iter()
                .filter(|file| !literal.contains(file) && !covered(write_set, file))
                .map(|file| format!("{file} (索引)"))
                .collect();
            if missing.is_empty() {
                Indexed::Clear
            } else {
                Indexed::Missing(missing)
            }
        }
    }
}

/// 作れない周（状態が failed）の結果の行の尾 `index=unavailable:<語>`（touches に型の項目を持つ行だけ・undeclared と状態なしは尾なし）。
pub(super) fn tail(state: Option<&Status>, touches: &[String]) -> Option<String> {
    match state {
        Some(Status::Failed(word)) if !closure::type_items(touches).is_empty() => Some(format!("index=unavailable:{word}")),
        _ => None,
    }
}
