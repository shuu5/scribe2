//! 契約表の行の欄 `done-teeth` の照らし（設計 docs/design/contract-source.md §66 形 2・行 bw・SRS FR105 / FR47 / FR55）。
//!
//! 欄の要素 1 つは「<番号>:<歯>」で、歯は名・`=` と名・`@` と番号・`!` と仕組みの名の 4 形のどれか 1 つ（[`Tooth`]）。照らしは
//! 表の検査（`contracts check`）・受付・preflight が**同じ 1 本**を撃ち、外れを全件・理由つきで返す（1 件目で止めない）:
//! 形の照らし [`done_teeth_misses`]（(a) 形・(b) 覆い・(c) 検証行の番号・(d) 選ばれ・(f) 仕組み）は base を読まず、在りかの
//! 照らし [`done_teeth_located`]（(e)）は base の `.rs` を読む口で、受付と preflight だけが撃つ。外れは本 module の型 [`Miss`]
//! で返し、表の検査の 1 種への包みは表の検査と受付の側が持つ（ほかの行の touches の型を本 module は名指さない）。

use super::super::closure::{selects, tooth_sites, Base};
use super::super::review::done_items;
use std::collections::BTreeSet;

/// write-set の `=` の項目（置き場だけ）の接頭辞。
const PLACE_ONLY: char = '=';

/// write-set の新規 file の項目の接頭辞（`+` は作る・`~` は着地で消える）。
const NEW_FILES: [char; 2] = ['+', '~'];

/// 検証行が契約表の検査を撃つ形の語（`contracts check`）。
const CHECK_WORDS: [&str; 2] = ["contracts", "check"];

/// 照らしの外れ 1 件（要素の字と理由の字）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Miss {
    /// 外れた要素の字（覆いの欠けは `<番号>:`）。
    pub element: String,
    /// 外れの理由。
    pub reason: String,
}

impl Miss {
    /// 要素 `element` の外れ `reason`。
    fn of(element: &str, reason: impl Into<String>) -> Self {
        Self { element: element.to_owned(), reason: reason.into() }
    }
}

/// 構造の制約を測る器の仕組み（設計 §66 形 0 の表の 4 語・閉じた 4 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mechanism {
    /// write-set の外の file を変えない。
    WriteSet,
    /// write-set の `=` の file の中身を変えない。
    PlaceOnly,
    /// write-set の `+` の file を作り `~` の file を消す。
    NewFile,
    /// 足した file がほかの行の閉包を広げない（契約表の検査を撃つ検証行が測る）。
    Closure,
}

impl Mechanism {
    /// 形 0 の表の語。
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::WriteSet => "write-set",
            Self::PlaceOnly => "place-only",
            Self::NewFile => "new-file",
            Self::Closure => "closure",
        }
    }

    /// 語から仕組みを引く（4 語の外は `None`）。
    fn of(word: &str) -> Option<Self> {
        [Self::WriteSet, Self::PlaceOnly, Self::NewFile, Self::Closure].into_iter().find(|found| found.as_str() == word)
    }
}

/// 歯の 4 形（設計 §66 形 1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tooth {
    /// 名の歯 `<名>`。
    Named(String),
    /// 既存の歯 `=<名>`。
    Kept(String),
    /// 検証行の番号の歯 `@<k>`（1 から数える）。
    Line(usize),
    /// 仕組みの歯 `!<仕組みの名>`。
    Mechanism(Mechanism),
}

/// 照らしが読む行の欄（契約表の行から取り出した字面・型は表の側が持つ）。
#[derive(Debug, Clone, Copy)]
pub(crate) struct Given<'a> {
    /// done の字（番号つきの項目を数える）。
    pub done: &'a str,
    /// 検証行。
    pub verify: &'a [String],
    /// write-set の項目（接頭辞つきのまま）。
    pub write_set: &'a [String],
    /// 新設する file。
    pub creates: &'a [String],
    /// 欄 `done-teeth` の要素。
    pub elements: &'a [String],
}

/// 識別子 1 つか（module の path は書かない）。
fn ident(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(|head| head.is_ascii_alphabetic() || head == '_')
        && chars.all(|found| found.is_ascii_alphanumeric() || found == '_')
}

/// 全部 ASCII の数字の非空の字か。
fn digits(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_digit())
}

/// 要素の番号（最初の `:` の前が ASCII の数字なら）。歯の形が外れた要素の番号も覆いの数えに入る。
fn number_of(element: &str) -> Option<u64> {
    element.split_once(':').filter(|(number, _)| digits(number)).and_then(|(number, _)| number.parse().ok())
}

/// 要素 1 つの読み（要素の字 → 番号と歯の対か、読めない理由の字・設計 §66 形 2 の口）。
pub(crate) fn parse_element(element: &str) -> Result<(u64, Tooth), String> {
    if element.chars().any(|found| found.is_whitespace() || found == ',') {
        return Err("空白か , を含む".to_owned());
    }
    let Some((number, tooth)) = element.split_once(':') else {
        return Err("<番号>:<歯> の形でない（: が無い）".to_owned());
    };
    let number = number_of(element).ok_or_else(|| format!("最初の : の前 {number:?} が ASCII の数字の番号でない"))?;
    Ok((number, parse_tooth(tooth)?))
}

/// 歯の字 → 4 形のどれか（外れは理由の字）。
fn parse_tooth(tooth: &str) -> Result<Tooth, String> {
    let outside = || format!("歯 {tooth} が名・=名・@番号・!仕組みの名の 4 形の外");
    match tooth.chars().next() {
        Some('=') => tooth.get(1..).filter(|name| ident(name)).map(|name| Tooth::Kept(name.to_owned())).ok_or_else(outside),
        Some('@') => tooth.get(1..).filter(|line| digits(line)).and_then(|line| line.parse().ok()).map(Tooth::Line).ok_or_else(outside),
        Some('!') => {
            let word = tooth.get(1..).unwrap_or_default();
            Mechanism::of(word).map(Tooth::Mechanism).ok_or_else(|| format!("仕組みの名 {word} が write-set・place-only・new-file・closure の外"))
        }
        _ => ident(tooth).then(|| Tooth::Named(tooth.to_owned())).ok_or_else(outside),
    }
}

/// 仕組み `found` の測る物を行が持たないときの理由（持つ・測る物の要らない仕組みは `None`）。
fn unmeasured(found: Mechanism, given: &Given<'_>) -> Option<&'static str> {
    let place = || given.write_set.iter().any(|item| item.starts_with(PLACE_ONLY));
    let fresh = || !given.creates.is_empty() || given.write_set.iter().any(|item| item.starts_with(NEW_FILES));
    let check = || {
        given.verify.iter().any(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            words.windows(CHECK_WORDS.len()).any(|pair| pair == CHECK_WORDS)
        })
    };
    match found {
        Mechanism::WriteSet => None,
        Mechanism::PlaceOnly => (!place()).then_some("write-set に = の項目が無い（place-only の測る物が無い）"),
        Mechanism::NewFile => (!fresh()).then_some("write-set に + か ~ の項目も creates も無い（new-file の測る物が無い）"),
        Mechanism::Closure => (!check()).then_some("契約表の検査（contracts check）を撃つ検証行が無い（closure の測る物が無い）"),
    }
}

/// 歯 1 つの外れの理由（(c) 検証行の番号・(d) 選ばれ・(f) 仕組み・外れなければ `None`）。
fn tooth_reason(tooth: &Tooth, given: &Given<'_>, core_crate: &str) -> Option<String> {
    match *tooth {
        Tooth::Named(ref name) | Tooth::Kept(ref name) => {
            let chosen = given.verify.iter().any(|line| selects(line, name, core_crate));
            (!chosen).then(|| format!("撃たれない歯: 歯 {name} をどの検証行の filter 語も（行の一致の型で）選ばない"))
        }
        Tooth::Line(line) => (!(1..=given.verify.len()).contains(&line))
            .then(|| format!("@{line} が検証行 1〜{count} の外（検証行の本数 {count}）", count = given.verify.len())),
        Tooth::Mechanism(found) => unmeasured(found, given).map(str::to_owned),
    }
}

/// 形の照らし（設計 §66 形 2 の (a)(b)(c)(d)(f)・欄を持つ行だけ・pure）: 外れを全件、要素の順に返し、覆いの外れを後ろへ足す。
///
/// 要素が空の列（欄を持たない行）は外れ 0。`core_crate` は `-p` の無い検証行が指す crate の名（選ばれの判定が使う）。
pub(crate) fn done_teeth_misses(given: &Given<'_>, core_crate: &str) -> Vec<Miss> {
    let mut found = Vec::new();
    for (at, element) in given.elements.iter().enumerate() {
        let read = if given.elements.iter().take(at).any(|earlier| earlier == element) {
            Err("同じ要素が重なる".to_owned())
        } else {
            parse_element(element).map(|(_, tooth)| tooth)
        };
        match read {
            Err(reason) => found.push(Miss::of(element, reason)),
            Ok(tooth) => found.extend(tooth_reason(&tooth, given, core_crate).map(|reason| Miss::of(element, reason))),
        }
    }
    if !given.elements.is_empty() {
        found.extend(cover_misses(given.done, given.elements));
    }
    found
}

/// 覆い（(b)）: 番号の集合が done の番号つき項目の 1〜K とちょうど等しいか。K が 0 の行は欄を持てない。
fn cover_misses(done: &str, elements: &[String]) -> Vec<Miss> {
    let count = done_items(done).len() as u64;
    if count == 0 {
        return vec![Miss::of("done-teeth", "done が番号つきの項目 (1) を持たない行は欄を持てない")];
    }
    let numbers: BTreeSet<u64> = elements.iter().filter_map(|element| number_of(element)).collect();
    let mut found: Vec<Miss> = (1..=count)
        .filter(|number| !numbers.contains(number))
        .map(|number| Miss::of(&format!("{number}:"), format!("done の項目 {number} を覆う要素が無い（項目は 1〜{count}）")))
        .collect();
    let extra = elements.iter().filter_map(|element| Some((element, number_of(element)?))).filter(|(_, number)| !(1..=count).contains(number));
    found.extend(extra.map(|(element, number)| Miss::of(element, format!("番号 {number} が done の項目 1〜{count} に無い"))));
    found
}

/// 歯 `name` が base に在る所（path の列・同じ file に 2 つ在れば 2 回）を、それを選ぶ検証行 `lines` の全部で集める
/// （行どうしが同じ file を指す周は 1 度に畳む）。
fn sites_of(name: &str, lines: &[&String], base: &Base<'_>) -> Vec<String> {
    let mut sites: Vec<String> = Vec::new();
    for line in lines {
        let here: Vec<String> = tooth_sites(line, name, base).into_iter().map(|(path, _)| path).collect();
        for path in here.iter().collect::<BTreeSet<&String>>() {
            let count = |list: &[String]| list.iter().filter(|other| *other == path).count();
            let new = count(&here).saturating_sub(count(&sites));
            sites.extend(std::iter::repeat_n(path.clone(), new));
        }
    }
    sites
}

/// 在りかの照らし（設計 §66 形 2 の (e)・base の `.rs` を読む）: 既存の歯は、それを選ぶ検証行の crate と scope の base の歯の区間に
/// ちょうど 1 つ在ること（0 は無い歯・2 か所以上は 2 か所の名）、名の歯は base に在れば 1 か所に定まること（2 か所以上を名指す・
/// base に無い名は新しい歯で外れでない）。どの検証行にも選ばれない名は (d) が名指すのでここでは数えない。
pub(crate) fn done_teeth_located(elements: &[String], verify: &[String], base: &Base<'_>) -> Vec<Miss> {
    let mut found = Vec::new();
    for element in elements {
        let (name, kept) = match parse_element(element) {
            Ok((_, Tooth::Named(name))) => (name, false),
            Ok((_, Tooth::Kept(name))) => (name, true),
            _ => continue,
        };
        let chosen: Vec<&String> = verify.iter().filter(|line| selects(line, &name, base.core_crate)).collect();
        let sites = sites_of(&name, &chosen, base);
        if kept && !chosen.is_empty() && sites.is_empty() {
            found.push(Miss::of(element, format!("無い歯: 既存の歯 {name} が base の歯の区間に 0 か所")));
        } else if sites.len() > 1 {
            found.push(Miss::of(element, format!("2 か所の名: 歯 {name} が base の {} に在る", sites.join(", "))));
        }
    }
    found
}
