//! 群の名の形と宣言順の検査（設計 account-lifecycle.md §29 の行 s・ADR-0069）。
//!
//! 群の名は `Tier` の後ろに 10 進の数字 1 桁以上（先頭の 0 は不可）に限り、宣言順は数字の狭義の昇順である。
//! 群の表の名は Tier1〜Tier9 で、数字 10 以上の名を断る（§34 の行 x）。`Tier9` は群でなく park の区画で、検査の後に群の列から分ける（§35 の行 y）。
//! 数字は宣言順の検査にだけ使い、並べ替えには使わない（優先は宣言順のまま・食い違う面は黙って通さず断る）。

use super::manifest::{text_field, AccountGroup, RawRow};
use super::RuleError;
use std::cmp::Ordering;

/// 群の名の接頭辞。
const TIER: &str = "Tier";

/// park の区画の名（§35 の行 y・ADR-0091）。
const PARK: &str = "Tier9";

/// 名の形と宣言順を検査し、外れる群ごとに群の見出し行で 1 件ずつ `errors` へ積む。
///
/// 形の合う名のうち数字が 10 以上の名を群の見出し行で断る（§34 の行 x）。`Tier9`（park の区画の名）は群と同じ検査を受ける（§35 の行 y）。
/// 名が前の群と同じ群は昇順の欠陥を重ねない（名の重複は [`super::manifest`] の重複の検査が 1 件にする・同じ欠陥を 2 行にしない）。
/// 前の群の名が形に外れる周と、前の群を 10 以上で断った周も重ねない（前の群の行がその欠陥で 1 件になっている）。
pub(super) fn check_tiers(groups: &[AccountGroup], errors: &mut Vec<RuleError>) {
    for (index, group) in groups.iter().enumerate() {
        let Some(digits) = tier_digits(group.name()) else {
            errors.push(RuleError::new(
                group.line(),
                format!("群の名 {} が Tier と数字の形でない（{TIER} の後ろに 10 進の数字・先頭の 0 は不可）", group.name()),
            ));
            continue;
        };
        if let Some(refusal) = outside_table(group.name(), digits) {
            errors.push(RuleError::new(group.line(), refusal));
            continue;
        }
        if groups.iter().take(index).any(|found| found.name() == group.name()) {
            continue;
        }
        let before = index.checked_sub(1).and_then(|at| groups.get(at));
        let prior = before.and_then(|found| group_digits(found.name()).map(|prior| (found.name(), prior)));
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

/// 群として読む名の数字の字面（形に外れる名と、[`outside_table`] が断る名は `None`）。
fn group_digits(name: &str) -> Option<&str> {
    tier_digits(name).filter(|digits| outside_table(name, digits).is_none())
}

/// 形の合う名のうち群の表の外の名の断り（数字が 10 以上＝群の表の名は Tier1〜Tier9）。
fn outside_table(name: &str, digits: &str) -> Option<String> {
    (digits.len() > 1).then(|| format!("群の名 {name} の数字が 9 を越える（群の表の名は Tier1〜Tier9）"))
}

/// 検査の済んだ行の列から park の区画の行（名 `Tier9`）を分けて返す（名の重複の検査が済んでいるので高々 1 つ・§35 形 1）。
pub(super) fn split_park(groups: &mut Vec<AccountGroup>) -> Option<AccountGroup> {
    let at = groups.iter().position(|group| group.name() == PARK)?;
    Some(groups.remove(at))
}

/// 群の表の行の任意 key `heartbeat` の値（**閉じた 2 値**・字面は口の語 `on` / `off` と同じ・設計 seat-heartbeat.md §22 形 1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heartbeat {
    /// 合図を送る。
    On,
    /// 合図を送らない。
    Off,
}

/// 行の `heartbeat` の値を判じる 1 関数（key が無い周は `None`・型違いと `on` / `off` 以外の文字列は key の行番号つきの欠陥で
/// `errors` へ積んで `None`・群の行と区画の行の両方が同じ 1 本を通る）。
pub(super) fn heartbeat_of(raw: &RawRow, errors: &mut Vec<RuleError>) -> Option<Heartbeat> {
    let text = text_field(raw, "heartbeat", errors)?;
    let found = match text.as_str() {
        "on" => Some(Heartbeat::On),
        "off" => Some(Heartbeat::Off),
        _ => None,
    };
    if found.is_none() {
        let line = raw.fields.iter().find(|(key, _, _)| key == "heartbeat").map_or(raw.line, |(_, _, line)| *line);
        errors.push(RuleError::new(line, format!("heartbeat の値 {text:?} が on でも off でもない")));
    }
    found
}

/// 先頭の 0 を持たない 10 進の字面を**数値で**比べる（桁数 → 同じ桁数なら字面・桁あふれしない）。
fn compare_digits(left: &str, right: &str) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}
