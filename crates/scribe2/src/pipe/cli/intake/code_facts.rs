//! 欄 `code-facts` の照らしと測り（設計 docs/design/reverse-index.md §7 (c)・契約表の行 e・FR47 / FR55 / FR68 / FR109）。
//!
//! 欄を持つ行の要素（`<列>:<項目>=<値>`・読みは表の読みの 1 本 [`code_fact`]）を、base の commit の索引（状態 ready の表）で
//! 行 c の列の数え（審査の子 module の [`count`]・2 本目の数えを作らない）に照らし、**閉じた結果**（[`Facts`]）を返す。
//! 断りの組み立て・待ちの理由は親の `intake.rs` が書く——この module は断りの型・契約表の行・表の欠陥・受付の断りの型を組まず match
//! もしない（ほかの行の touches の型を新しい file で名指すと、その行の閉包が広がる）。状態が ready でない周は測れない（名乗った
//! 事実を測らずに起動しない・C10）で、状態の語を返す。

use crate::pipe::dispatch::index_build::Status;
use crate::pipe::refuse::{Difference, STATE_UNDECLARED};
use crate::pipe::review::index::{count, Ctx, Scope, Tables};
use crate::pipe::table::{code_fact, Claim};
use std::collections::BTreeMap;
use std::path::Path;

/// 実測の site を断りの 1 行に載せる数（先頭 3 つ・残りは件数）。
const SHOWN: usize = 3;

/// 欄を持つ行の判定の結果（断りの組み立ては親が書く）。
pub(super) enum Facts {
    /// 通る（要素が全部、名乗りと実測が等しい）。
    Clear,
    /// 測れない（先頭の要素の字面と状態の語＝`absent`・`building`・`failed:<語>`・`none`・`undeclared`）。
    Unmeasured { element: String, state: String },
    /// 索引の宣言の不備（key の名と行番号の行の列・状態が half）。
    Half(Vec<String>),
    /// 名乗りと実測が違う要素（欄の順）。
    Differ(Vec<Difference>),
}

/// 欄 `elements` を base の索引の状態 `state` で測る。`at` は索引を読んだ commit（repo と sha・状態を載せた呼び手だけが持つ）。
pub(super) fn judge(elements: &[String], state: Option<&Status>, at: Option<(&Path, &str)>) -> Facts {
    let first = elements.first().cloned().unwrap_or_default();
    let unmeasured = |word: &str| Facts::Unmeasured { element: first.clone(), state: word.to_owned() };
    match (state, at) {
        (Some(Status::Ready(rows)), Some((repo, sha))) => {
            let tables = Tables::default();
            let ctx = Ctx { rows, repo, sha, tables: &tables };
            measure(elements, &ctx).map_or_else(|| unmeasured("form"), |found| if found.is_empty() { Facts::Clear } else { Facts::Differ(found) })
        }
        (Some(Status::Undeclared), _) => unmeasured(STATE_UNDECLARED),
        (Some(Status::Half(errors)), _) => Facts::Half(errors.iter().map(ToString::to_string).collect()),
        (Some(Status::Absent), _) => unmeasured("absent"),
        (Some(Status::Building), _) => unmeasured("building"),
        (Some(Status::Failed(word)), _) => unmeasured(&format!("failed:{word}")),
        (Some(Status::Ready(_)) | None, _) => unmeasured("none"),
    }
}

/// 要素ごとに列の数えを撃ち、名乗りと違う要素を返す（要素が形を崩していれば `None`＝表の検査が先に断る形で、ここへは届かない）。
/// 数えは項目ごとに 1 回（同じ項目の別の列は同じ数えを読む）。
fn measure(elements: &[String], ctx: &Ctx<'_>) -> Option<Vec<Difference>> {
    let scope = Scope { own: "", marks: None };
    let mut counted = BTreeMap::new();
    let mut found = Vec::new();
    for element in elements {
        let fact = code_fact(element).ok()?;
        let one = counted.entry(fact.item.clone()).or_insert_with(|| count(ctx, &fact.item, &scope));
        let column = if fact.column == "files" { "refs" } else { fact.column };
        let sites = one.sites(column).unwrap_or_default();
        let (claimed, measured) = match (&fact.claim, one.ambiguous()) {
            (Claim::Count(want), None) => {
                let have = if fact.column == "files" { files_of(&sites) } else { sites.len() };
                (want.to_string(), have.to_string())
            }
            (Claim::Vis(want), None) => (want.clone(), one.definition_vis().unwrap_or_else(|| "unresolved".to_owned())),
            (Claim::Count(want), Some(many)) => (want.to_string(), format!("ambiguous:{many}")),
            (Claim::Vis(want), Some(many)) => (want.clone(), format!("ambiguous:{many}")),
        };
        if claimed != measured {
            let text = one.text().map_or_else(|| "text=-".to_owned(), |lines| format!("text={lines}"));
            let rest = sites.len().saturating_sub(SHOWN);
            found.push(Difference { element: element.clone(), claimed, measured, sites: sites.into_iter().take(SHOWN).collect(), rest, text });
        }
    }
    Some(found)
}

/// site（`<path>:<行>`）の file の数。
fn files_of(sites: &[String]) -> usize {
    let mut paths: Vec<&str> = sites.iter().filter_map(|site| site.rsplit_once(':').map(|(path, _)| path)).collect();
    paths.sort_unstable();
    paths.dedup();
    paths.len()
}
