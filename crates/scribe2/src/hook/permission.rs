//! 内蔵 guard の承認の問いへ機械で答える（設計 docs/design/vessel-hook.md §7・
//! SRS FR19 / FR21 / FR24・憲法 C11 / C2.2 / CON5）。
//!
//! Claude Code の内蔵 Bash guard は、`rm` の path に変数展開や `$(…)` が混ざる周を筆頭に、
//! bypassPermissions でも dialog を出す。対話 session はそこで**止まる**——無人の席では
//! 誰も答えず、席が沈黙したまま cycle が進まない。ゆえに器が **`Bash` の承認要求を一律
//! deny** し、「allow 規則に合う形へ書き直せ」という次の一手を model へ返す。`rm` の
//! literal path はその**従属句**であって、答える範囲を `rm` の周へ狭めるものではない
//! （matcher と同じく `Bash` 全体に答える）。
//!
//! **allow を返す経路は作らない**（憲法 C11 の極性は型で持つ）。承認を機械が与えると、
//! 人間の承認 gate がここから空洞化する——止める側へ倒すのは安全だが、通す側へ倒すのは
//! 取り返しがつかない。答えるのは `Bash` の周だけで、それ以外は **0 byte で黙る**
//! （FR24＝Claude Code の既定の問いへ戻す。器が答える筋合いの無い承認まで奪わない）。

use crate::polarity::{OnFailure, Polarity, Timing};
use crate::fleet::json_lite;
use crate::name::NAME;

/// 器が答える tool。内蔵 guard の問いのうち引き受けるのは `Bash` だけである。
const ANSWERED: &str = "Bash";

/// この境界の極性: 問いの時点で答え、allow を返す経路が無い（止める側へしか倒れない）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

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
        PermissionDecision::Deny(deny_line(&message()))
    } else {
        PermissionDecision::Silent
    }
}

/// model へ返す理由の 1 行（**次の一手まで書く**＝「駄目だ」だけでは席が止まる）。
///
/// 名乗るのは `Bash` の承認要求**一般**である。`rm` の書き直しは従属句として添えるだけで、
/// 「`rm` の周だけを deny する」とは読ませない——答える範囲は matcher と同じ `Bash` 全体で、
/// 文言がそこを狭く言うと、model は他の Bash が deny された理由を読めなくなる。
fn message() -> String {
    format!(
        "{NAME}: この席では Bash の承認要求を機械が一律 deny します\
         （無人の席が dialog で止まらないため・allow は返しません）。\
         allow 規則に合う形へ書き直してください。\
         rm なら literal path で書き、path に変数展開や $(...) を使わないでください"
    )
}

/// Claude Code が読む stdout の 1 行。
///
/// [`json_lite`] は **flat object 専用**（`parse_object` は入れ子を error にする）で、
/// この形は 3 段の入れ子ゆえ通らない。値の escape だけ [`json_lite::quote`] を通し、
/// 入れ子はここで組み立てる（**書き側が escape を自前で持たない**のが要点＝素の
/// `format!` で囲むと `"` や `\` や制御文字を含む message が JSON を壊す）。
///
/// message を**引数で受ける**のは、その escape を歯から測れるようにするためである。
/// [`message`] の固定文言は `"` も `\` も制御文字も含まないので、`quote` を素の `format!`
/// へ替える変異が**出力 byte 同一のまま生き残る**（lens-43 M-1・実測）。
pub(crate) fn deny_line(message: &str) -> String {
    format!(
        "{{\"hookSpecificOutput\":{{\"hookEventName\":\"PermissionRequest\",\
         \"decision\":{{\"behavior\":\"deny\",\"message\":{}}}}}}}",
        json_lite::quote(message)
    )
}

#[cfg(test)]
mod tests {
    // flip-check: retroactive s2-07l.47
    use super::deny_line;
    use crate::fleet::json_lite;

    /// `"` / `\` / 改行 / `$(…)` を含む message が **そのまま届く** 1 行になる。
    ///
    /// 字面の `contains` では escape の壊れは見えない（壊れた行も部分文字列は持つ）。
    /// 決定 object を切り出して `parse_object` へ通し、`message` が渡した文字列そのものへ
    /// 戻ることまで見る——`quote` を素の `format!` へ替える変異は、ここで parse か等値の
    /// どちらかで落ちる。
    #[test]
    fn deny_line_round_trips_quotes_and_backslashes() {
        // `{` `}` は入れない（決定 object の切り出しは最初の `{`〜`}` で行う）。
        let raw = "\"引用\" と \\ と 改行\nの後に $(x)";
        let line = deny_line(raw);
        let at = line.find("\"decision\":").expect("decision が在る");
        let rest = &line[at..];
        let open = rest.find('{').expect("決定 object が開く");
        let close = rest.find('}').expect("決定 object が閉じる");
        let pairs = json_lite::parse_object(&rest[open..=close])
            .unwrap_or_else(|err| panic!("決定 object を parse できる: {err} / {line}"));
        assert_eq!(
            pairs.iter().find(|(key, _)| key == "behavior").map(|(_, value)| value.clone()),
            Some(json_lite::Value::Str("deny".to_owned())),
            "一律 deny である: {line}"
        );
        assert_eq!(
            pairs.iter().find(|(key, _)| key == "message").map(|(_, value)| value.clone()),
            Some(json_lite::Value::Str(raw.to_owned())),
            "message は渡した文字列そのものへ戻る: {line}"
        );
    }
}
