//! 宣言の任意 key `entrance-flip`（入口の flip の名乗り）の値の型と読み手（設計 pipeline.md §54 / §56・ADR-0054 / ADR-0059）。
//!
//! 親（`declaration.rs`）の余地のために子 module へ置く（ADR-0047 の path の種別と同じ置き方）。外から呼ぶ path は親の
//! 再輸出で `crate::pipe::declaration::EntranceFlip` のまま。語は閉じた 3 語で、どれも入口の flip の行を要らない
//! （`NoEntranceRed` で断らない）。`detect` と `deny` は受付が契約の nextest の検証行を base の木でも撃つ名乗りで、
//! `deny` は base で緑か測れない行が 1 本でも在れば受付で断る（判定は受付の側・この file は語を読むだけ）。

use super::{Ceiling, DeclError, Effective, Raw, Sourced};
use std::path::Path;

/// 名乗りの key。書かない宣言は現行のとおり入口の flip の行を要る（`KindGap::NoEntranceRed`）。
pub(super) const KEY: &str = "entrance-flip";

/// 宣言の任意 key `entrance-flip` の値（閉じた 3 語）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntranceFlip {
    /// 入口の flip を測らない（撃つ手段を持たない Rust の消費側の名乗り・§54）。
    Unmeasured,
    /// 契約の nextest の検証行を受付で base の木でも撃ち、base で緑の本数を記録する（受付の結果は変えない・§56）。
    Detect,
    /// `Detect` に加え、base で緑か測れない行が 1 本でも在れば受付で断る（§56 形 6）。
    Deny,
}

impl EntranceFlip {
    /// 宣言 file と判定行に書く語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unmeasured => "unmeasured",
            Self::Detect => "detect",
            Self::Deny => "deny",
        }
    }

    /// 受付が契約の nextest の検証行を base の木で撃つ名乗りか（`detect` と `deny`）。
    pub fn fires_on_base(self) -> bool {
        self != Self::Unmeasured
    }

    /// base で緑か測れない行を受付で断る名乗りか（`deny`）。
    pub fn denies(self) -> bool {
        self == Self::Deny
    }

    /// 語から引く（閉じた 3 語の外は `None`）。
    fn parse(word: &str) -> Option<Self> {
        [Self::Unmeasured, Self::Detect, Self::Deny].into_iter().find(|found| found.as_str() == word)
    }
}

/// 名乗り（任意）を読む。値は閉じた 3 語の文字列だけを受ける（他の語・空・配列は断る）。
pub(super) fn entrance_of(found: &[(String, Raw, u64)], errors: &mut Vec<DeclError>) -> Option<EntranceFlip> {
    let (_, value, line) = found.iter().find(|(seen, _, _)| seen == KEY)?;
    let named = match value {
        Raw::Text(word) => EntranceFlip::parse(word),
        Raw::Int(_) | Raw::List(_) => None,
    };
    if named.is_none() {
        errors.push(DeclError::new(*line, format!("{KEY} は 1 語 \"unmeasured\" / \"detect\" / \"deny\" の文字列だけである")));
    }
    named
}

/// [`super::measure`] に宣言の名乗りを添えた形（受付が lock の前に 1 度だけ撃ち、lock の中の判定は結果を借りる・§56 形 2）。
pub fn measure_named(
    repo: &Path,
    ceiling: &Ceiling<'_>,
    contract_verify: &[String],
) -> Result<(Effective, Option<EntranceFlip>), Vec<DeclError>> {
    let sourced = Sourced::read(repo, ceiling)?;
    let entrance = sourced.declared.entrance_flip;
    sourced.measure(ceiling, contract_verify).map(|effective| (effective, entrance))
}
