//! 群の名の形と宣言順の検査（設計 account-lifecycle.md §29 の行 s・ADR-0069）。
//!
//! 群の名は `Tier` の後ろに 10 進の数字 1 桁以上（先頭の 0 は不可）に限り、宣言順は数字の狭義の昇順である。
//! 数字は宣言順の検査にだけ使い、並べ替えには使わない（優先は宣言順のまま・食い違う面は黙って通さず断る）。

use super::manifest::AccountGroup;
use super::RuleError;
use std::cmp::Ordering;

/// 群の名の接頭辞。
const TIER: &str = "Tier";

/// 名の形と宣言順を検査し、外れる群ごとに群の見出し行で 1 件ずつ `errors` へ積む。
///
/// 名が前の群と同じ群は昇順の欠陥を重ねない（名の重複は [`super::manifest`] の重複の検査が 1 件にする・同じ欠陥を 2 行にしない）。
/// 前の群の名が形に外れる周も重ねない（前の群の行が形の欠陥で 1 件になっている）。
pub(super) fn check_tiers(groups: &[AccountGroup], errors: &mut Vec<RuleError>) {
    for (index, group) in groups.iter().enumerate() {
        let Some(digits) = tier_digits(group.name()) else {
            errors.push(RuleError::new(
                group.line(),
                format!("群の名 {} が Tier と数字の形でない（{TIER} の後ろに 10 進の数字・先頭の 0 は不可）", group.name()),
            ));
            continue;
        };
        if groups.iter().take(index).any(|found| found.name() == group.name()) {
            continue;
        }
        let before = index.checked_sub(1).and_then(|at| groups.get(at));
        let prior = before.and_then(|found| tier_digits(found.name()).map(|prior| (found.name(), prior)));
        if let Some((name, _)) = prior.filter(|(_, prior)| compare_digits(digits, prior) != Ordering::Greater) {
            errors.push(RuleError::new(
                group.line(),
                format!("群 {} の数字が前の群より大きくない（前の群は {name}・宣言順は数字の昇順）", group.name()),
            ));
        }
    }
}

/// `Tier<数字>` の数字の字面（形に外れる名は `None`）。
fn tier_digits(name: &str) -> Option<&str> {
    let digits = name.strip_prefix(TIER)?;
    let well_formed = !digits.is_empty() && !digits.starts_with('0') && digits.bytes().all(|byte| byte.is_ascii_digit());
    well_formed.then_some(digits)
}

/// 先頭の 0 を持たない 10 進の字面を**数値で**比べる（桁数 → 同じ桁数なら字面・桁あふれしない）。
fn compare_digits(left: &str, right: &str) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}
