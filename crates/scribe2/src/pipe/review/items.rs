//! 審査の材料の 6 本目: done の番号つき項目と lens が返す項目ごとの歯の対応の表（設計 docs/design/contract-source.md §64・行 bs）。
//!
//! lens は done の項目のうち最初に見つけた 1〜3 個だけを名指して FAIL を返しがちで、直して出し直すと別の項目で落ちる。器は done の
//! 項目を番号つきで材料の [`super::ITEMS_FILE`] に置き（[`items_text`]）、lens の最終行の key `done` に「<番号>:<歯>」の表を返させ、
//! 表の揃い（[`holes`]）と歯の無い項目（`-`）の番号だけを測る。項目の読み手は [`done_items`] の 1 本で、材料の書き手と判定の読みが
//! 同じ関数を呼ぶ（数え方を 2 通りにしない・C2）。歯の中身の正しさは器が照らさない（lens の判断）。

use crate::fleet::json_lite::Value;
use crate::pipe::table::parse_element;

/// 見出しの行（K 個の数を持つ）。
const HEADING: &str = "## done の項目";

/// 歯の無い項目の印。
const NO_TOOTH: &str = "-";

/// done の字を頭から見て、(1) から 1 ずつ増える番号の印「(n)」が順に現れた所で区切った項目の本文（前後の空白を剥がす）。
///
/// 順の外の印（(2) の後の 2 つ目の (2)・(1) の前の (3)）は本文の一部で、(1) を持たない done は 0 個。印は半角の括弧と ASCII の
/// 数字だけ（全角は印でない）。(1) より前の字は項目でない。
pub fn done_items(done: &str) -> Vec<String> {
    let mut starts: Vec<(usize, usize)> = Vec::new();
    let mut from = 0;
    while let Some(rest) = done.get(from..) {
        let mark = format!("({})", starts.len().saturating_add(1));
        let Some(found) = rest.find(&mark) else {
            break;
        };
        let at = from.saturating_add(found);
        from = at.saturating_add(mark.len());
        starts.push((at, from));
    }
    let ends = starts.iter().skip(1).map(|(at, _)| *at).chain(std::iter::once(done.len()));
    starts.iter().zip(ends).filter_map(|((_, body), end)| done.get(*body..end)).map(|text| text.trim().to_owned()).collect()
}

/// 宣言の歯の指示（契約 file が key `done-teeth` を持つ周の `<歯>` の読み・key の無い契約は [`ANY_TOOTH`]・設計 §66 形 6）。
const DECLARED_TOOTH: &str = "<歯> はその項目の宣言の歯のうち、約束を外した実装で落ちる 1 本・無ければ -。! の仕組みの歯は構造の制約の項目にだけ当たり、挙動の約束に仕組みの歯しか無ければ -";

/// key の無い契約の `<歯>` の読み（§64 のまま）。
const ANY_TOOTH: &str = "`<歯>` はその項目を外した実装で落ちる歯の名・落ちる歯が無い項目は `-`";

/// 宣言の歯（契約 file の key `done-teeth` の要素）のうち番号 `number` の歯の字（要素の最初の `:` の後ろ・要素の順・要素は
/// [`parse_element`] の 1 本で読む）。
fn declared_of(teeth: &[String], number: usize) -> Vec<&str> {
    let wanted = u64::try_from(number).ok();
    let found = teeth.iter().filter(|element| parse_element(element).ok().map(|(at, _)| at) == wanted);
    found.map(|element| element.split_once(':').map_or(element.as_str(), |(_, tooth)| tooth)).collect()
}

/// 歯の字の `=`（既存の歯の印）を剥がした字（表の歯と宣言の歯は `=` の有無を問わず突き合わせる）。
fn plain(tooth: &str) -> &str {
    tooth.strip_prefix('=').unwrap_or(tooth)
}

/// 材料 [`super::ITEMS_FILE`] の本文（項目 0 個は空＝置かない）: 見出し・表の形の指示・項目を 1 行ずつ「(n) <本文>」。`teeth` は契約 file の
/// key `done-teeth` の要素で、空でない周（key を持つ契約）だけ項目の行の末尾に「 ／ 歯: <その番号の歯を , で並べた字>」を添え、`<歯>` の指示を
/// 宣言の歯の読みへ替える（key の無い契約の本文は 1 字も変えない・設計 §66 形 6）。
pub fn items_text(done: &str, teeth: &[String]) -> String {
    let items = done_items(done);
    if items.is_empty() {
        return String::new();
    }
    let count = items.len();
    let shape: Vec<String> = (1..=count).map(|number| format!("{number}:<歯>")).collect();
    let listed: Vec<String> = items
        .iter()
        .enumerate()
        .map(|(at, body)| {
            let line = format!("({}) {}", at.saturating_add(1), body.split_whitespace().collect::<Vec<&str>>().join(" "));
            let declared = declared_of(teeth, at.saturating_add(1)).join(",");
            if teeth.is_empty() { line } else { format!("{line} ／ 歯: {declared}") }
        })
        .collect();
    let reading = if teeth.is_empty() { ANY_TOOTH } else { DECLARED_TOOTH };
    format!(
        "{HEADING}（{count} 個・番号つき）\n最終行の JSON に key `done` を足し、値の 1 つの文字列に項目ごとの対応を `{}` の形で全部並べる（{reading}）。`{NO_TOOTH}` の項目は見つけた 1〜3 個で止めず**全部**書く。番号が 1〜{count} をちょうど 1 回ずつ覆わない表と key `done` の無い周は器が INCONCLUSIVE に倒し、`{NO_TOOTH}` を持つ PASS は器が FAIL（`vacuous-assert`）に倒す。\n{}",
        shape.join(","),
        listed.join("\n")
    )
}

/// lens の最終行の key `done` の値（`value`・無ければ `None`）を、項目 `count` 個の表として読み、歯の無い項目の番号（昇順）を返す。
///
/// 値を `,` で割り、空白を剥がして空を落とした各項目を「<番号>:<歯>」と読む（番号は ASCII の数字・歯は空白を剥がして空でない字）。
/// 表が揃うのは番号が 1〜`count` をちょうど 1 回ずつ覆い、形の合わない項目が無い周。揃わない周は理由（「key done が無い」「key done が
/// 文字列でない」か、「無い番号」「余る番号」「重なる番号」「形の合わない項目 k 件」のうち在るものを「・」で結んだもの）を `Err` に返す。
///
/// `teeth` は契約 file の key `done-teeth` の要素で、空でない周（key を持つ契約）だけ、`-` でない表の歯がその番号の宣言の歯（`=` の有無は
/// 問わない）の外の項目を形の合わない項目に数える（設計 §66 形 6）。
pub fn holes(value: Option<&Value>, count: usize, teeth: &[String]) -> Result<Vec<usize>, String> {
    let text = match value {
        None => return Err("key done が無い".to_owned()),
        Some(found) => found.as_str().ok_or_else(|| "key done が文字列でない".to_owned())?,
    };
    let (mut numbers, mut bad): (Vec<(usize, bool)>, usize) = (Vec::new(), 0);
    for item in text.split(',').map(str::trim).filter(|item| !item.is_empty()) {
        let parsed = item.split_once(':').and_then(|(number, tooth)| {
            let digits = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
            let tooth = tooth.trim();
            let found = number.parse::<usize>().ok().filter(|_| digits && !tooth.is_empty())?;
            let outside = !teeth.is_empty()
                && tooth != NO_TOOTH
                && (1..=count).contains(&found)
                && !declared_of(teeth, found).iter().any(|declared| plain(declared) == plain(tooth));
            (!outside).then_some((found, tooth == NO_TOOTH))
        });
        match parsed {
            Some(found) => numbers.push(found),
            None => bad += 1,
        }
    }
    let times = |number: usize| numbers.iter().filter(|(found, _)| *found == number).count();
    let mut extra: Vec<usize> = numbers.iter().map(|(found, _)| *found).filter(|found| *found == 0 || *found > count).collect();
    extra.sort_unstable();
    extra.dedup();
    let missing: Vec<usize> = (1..=count).filter(|number| times(*number) == 0).collect();
    let doubled: Vec<usize> = (1..=count).filter(|number| times(*number) > 1).collect();
    let mut reasons: Vec<String> = Vec::new();
    for (head, found) in [("無い番号 ", missing), ("余る番号 ", extra), ("重なる番号 ", doubled)] {
        if !found.is_empty() {
            reasons.push(format!("{head}{}", listed(&found)));
        }
    }
    if bad > 0 {
        reasons.push(format!("形の合わない項目 {bad} 件"));
    }
    if !reasons.is_empty() {
        return Err(reasons.join("・"));
    }
    let mut none: Vec<usize> = numbers.iter().filter(|(_, empty)| *empty).map(|(found, _)| *found).collect();
    none.sort_unstable();
    Ok(none)
}

/// 歯の無い項目の `done(<n>)` のうち `at`（lens の値・`,` で割って空白を剥がした語）に無い物を番号の順に末尾へ足した at
/// （足す物が無ければ `at` のまま・`at` が無ければその列だけ・無ければ `None`）。
pub fn named_at(at: Option<&str>, none: &[usize]) -> Option<String> {
    let named: Vec<&str> = at.unwrap_or_default().split(',').map(str::trim).collect();
    let added: Vec<String> =
        none.iter().map(|number| format!("done({number})")).filter(|word| !named.contains(&word.as_str())).collect();
    match at.filter(|text| !text.trim().is_empty()) {
        Some(text) if added.is_empty() => Some(text.to_owned()),
        Some(text) => Some(format!("{text},{}", added.join(","))),
        None if added.is_empty() => at.map(str::to_owned),
        None => Some(added.join(",")),
    }
}

/// 歯の無い項目の番号を「(2)(3)」に並べる（evidence の字面）。
pub fn listed(numbers: &[usize]) -> String {
    numbers.iter().map(|number| format!("({number})")).collect()
}
