//! const slice の並びが**宣言順**と一致するかを測る述語（ADR-0013 D2）。
//!
//! 憲法 C2 は「適用順は stage 列挙の**宣言順だけ**から取る」と定める。器はそれを「閉じた
//! enum + 全 variant の const slice + 網羅 match」の形で満たすが、**const slice の並びは
//! 手で書く**ので宣言順から静かにずれうる——ずれても compile は通り、順序の注記を散文で
//! 持たない方針（C2）ゆえ、機械が測らなければ誰も気づかない。
//!
//! **限界**: この述語が捕まえるのは「並べ替え・重複・**中間**の欠番」である。enum の
//! **末尾**に足した variant を slice へ入れ忘れた形は捕まえない——判別子が `0..len` に
//! 収まるからである。それを捕まえるのは別の面で、[`crate::rules::ALL`] では manifest
//! parity（未知の kind は `parse` できない）、[`crate::pipe::gate`] の 2 面では網羅 match と
//! 外形である。

/// `all` の並びが宣言順（判別子 0, 1, 2, …）と一致するか。
///
/// `index` は variant を判別子へ写す関数で、C-like enum なら `|v| v as usize` である。
/// 判別子の slice を受け取らず関数を受け取るのは、`as` を書く場所を**呼び手の 1 行**へ
/// 閉じ、述語側に enum ごとの分岐を持たせないためである。
pub fn is_declaration_order<T: Copy>(all: &[T], index: impl Fn(T) -> usize) -> bool {
    all.iter().copied().map(index).eq(0..all.len())
}
