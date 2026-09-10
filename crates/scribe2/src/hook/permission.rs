//! 内蔵 guard の承認の問いへ機械で答える（設計 docs/design/vessel-hook.md §7・
//! SRS FR19 / FR21 / FR24・憲法 C11 / C2.2 / CON5）。
//!
//! Claude Code の内蔵 Bash guard は、`rm` の path に変数展開や `$(…)` が混ざると
//! bypassPermissions でも dialog を出す。対話 session はそこで**止まる**——無人の席では
//! 誰も答えず、席が沈黙したまま cycle が進まない。ゆえに器が**一律 deny**で答え、
//! 「literal path で書き直せ」という次の一手を model へ返す。
//!
//! **allow を返す経路は作らない**（憲法 C11 の極性は型で持つ）。承認を機械が与えると、
//! 人間の承認 gate がここから空洞化する——止める側へ倒すのは安全だが、通す側へ倒すのは
//! 取り返しがつかない。答えるのは `Bash` の周だけで、それ以外は **0 byte で黙る**
//! （FR24＝Claude Code の既定の問いへ戻す。器が答える筋合いの無い承認まで奪わない）。

use crate::fleet::json_lite;
use crate::name::NAME;

/// 器が答える tool。内蔵 guard の問いのうち引き受けるのは `Bash` だけである。
const ANSWERED: &str = "Bash";

/// 承認の問いへの答え。**`Allow` という variant を持たない**のが本 enum の要点である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    /// 一律 deny。中身は stdout へ出す 1 行。
    Deny(String),
    /// 管轄外＝0 byte で黙る（FR24）。
    Silent,
}

/// 1 回の問いについて判定する。
pub fn decide(tool: &str) -> PermissionDecision {
    if tool == ANSWERED {
        PermissionDecision::Deny(deny_line())
    } else {
        PermissionDecision::Silent
    }
}

/// model へ返す理由の 1 行（**次の一手まで書く**＝「駄目だ」だけでは席が止まる）。
fn message() -> String {
    format!(
        "{NAME}: この承認は機械が一律 deny します。rm は literal path で書き、\
         path に変数展開や $(...) を使わずに続行してください"
    )
}

/// Claude Code が読む stdout の 1 行。
///
/// [`json_lite`] は **flat object 専用**（`parse_object` は入れ子を error にする）で、
/// この形は 3 段の入れ子ゆえ通らない。値の escape だけ [`json_lite::quote`] を通し、
/// 入れ子はここで組み立てる（**書き側が escape を自前で持たない**のが要点＝message は
/// `$(...)` と読点を含むので、素の `format!` で囲むと壊れうる）。
fn deny_line() -> String {
    format!(
        "{{\"hookSpecificOutput\":{{\"hookEventName\":\"PermissionRequest\",\
         \"decision\":{{\"behavior\":\"deny\",\"message\":{}}}}}}}",
        json_lite::quote(&message())
    )
}
