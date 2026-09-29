//! host の面の公開の除外の表 `[[publish-exclusion]]` の行の組み立てと欄の検査（設計 vessel-hook.md §19 行 m・ADR-0093・FR57 / FR80）。
//!
//! 表は**host の面にだけ**置ける（無害と裁定した字句を tracked な file に書くと、PUBLIC な repo への公開そのもの・CON2）。tracked の面に
//! 置いた表は [`super::manifest`] の読み手が 1 表 1 件で断る。読み手はここを呼ぶだけで、見出しと受ける key の列もここの定数を引く。
//! 読みが確かめるのは欄の有無と空だけで、裁定 id が台帳の裁定に解けるかは測らない。字句は digest にせず字のまま持つ（大小も畳まない・
//! 空の字句も断らない＝成分を持たない字句は何も落とさない）。照合と当たりの落としは後の行が持ち、ここは値を持つだけ。

use super::manifest::{check_keys, text_field, RawRow, RawValue};
use super::RuleError;

/// 表の見出しの字面。
pub(super) const HEADER: &str = "[[publish-exclusion]]";

/// 1 行が持てる key の全体（宣言順・閉じた 2 つ）。
pub(super) const KEYS: &[&str] = &["phrase", "ruling"];

/// 1 行に必ず要る key（2 つとも）。
pub(super) const REQUIRED: &[&str] = &["phrase", "ruling"];

/// `[[publish-exclusion]]` 1 行が名乗る除外の字句（host の面にだけ書く・無害と裁定した字句と、その裁定 id）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishExclusion {
    phrase: String,
    ruling: String,
    line: u64,
}

impl PublishExclusion {
    /// 除外の字句（書いたままの字面・大小も区切り字もそのまま）。
    pub fn phrase(&self) -> &str {
        &self.phrase
    }

    /// 裁定 id（書いたままの字面・空でない）。
    pub fn ruling(&self) -> &str {
        &self.ruling
    }

    /// manifest の中でこの行が始まる物理行番号。
    pub fn line(&self) -> u64 {
        self.line
    }
}

/// `[[publish-exclusion]]` 1 行を組む。欠けと未知 key と型違いは `check_keys` と `text_field` が 1 件ずつ積み、`ruling` が空
/// （前後の空白を落として 0 字）の行は `ruling` を書いた行の行番号で断る（裁定 id が要る・ADR-0093・FR57）。
pub(super) fn build(raw: &RawRow, errors: &mut Vec<RuleError>) -> Option<PublishExclusion> {
    let before = errors.len();
    check_keys(raw, errors);
    if raw.fields.iter().any(|(_, value, _)| matches!(value, RawValue::Broken)) {
        return None;
    }
    let phrase = text_field(raw, "phrase", errors);
    let ruling = text_field(raw, "ruling", errors);
    if let Some(ruling) = ruling.as_deref().filter(|found| found.trim().is_empty()) {
        let line = raw.fields.iter().find(|(key, _, _)| key == "ruling").map_or(raw.line, |(_, _, line)| *line);
        errors.push(RuleError::new(line, format!("ruling {ruling:?} が空である（裁定 id が要る・ADR-0093）")));
    }
    let (Some(phrase), Some(ruling)) = (phrase, ruling) else {
        return None;
    };
    (errors.len() == before).then_some(PublishExclusion { phrase, ruling, line: raw.line })
}
