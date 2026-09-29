//! publish の照合の核（設計 docs/design/vessel-hook.md §19 行 m2・ADR-0078 / ADR-0093・SRS FR80 / AC50 / NFR4）。
//!
//! 出ていく字面（出所の種別と本文）を ASCII の英数字と `_` の成分に切り、隣の repo の名を成分の連続した並びで大小を畳んで当て、
//! 除外の字句と成分の並びが等しい区間に収まる当たりだけを落とし、当たりを件数と先頭 5 件の 1 行に畳む。pure（引数と戻りだけ・
//! I/O も子 process も無く、git も gh も撃たない）。他の 3 形・gh の本文の読み・配線・除外の表の読みは後続の行が足す。

use super::{Elements, Form};
use std::collections::HashSet;
use std::ops::Range;

/// 断りの hit が名指す当たりの数の上限（先頭からこの数だけを並べる）。
const SHOWN: usize = 5;

/// 出ていく字面の出所の種別（閉じた 9 値・宣言順・設計 §19 形 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// commit の message。
    CommitMessage,
    /// author の名。
    AuthorName,
    /// committer の名。
    CommitterName,
    /// patch の追加行。
    PatchAdded,
    /// patch の path。
    PatchPath,
    /// 押す annotated tag の本文。
    TagBody,
    /// 押す ref の名。
    RefName,
    /// gh の本文の flag の値（api の欄を含む）。
    GhFlagValue,
    /// gh の本文の file の中身。
    GhFileBody,
}

/// [`Source`] の全 variant（宣言順）。
pub const SOURCES: &[Source] = &[
    Source::CommitMessage,
    Source::AuthorName,
    Source::CommitterName,
    Source::PatchAdded,
    Source::PatchPath,
    Source::TagBody,
    Source::RefName,
    Source::GhFlagValue,
    Source::GhFileBody,
];

impl Source {
    /// 出所の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CommitMessage => "commit-message",
            Self::AuthorName => "author-name",
            Self::CommitterName => "committer-name",
            Self::PatchAdded => "patch-added",
            Self::PatchPath => "patch-path",
            Self::TagBody => "tag-body",
            Self::RefName => "ref-name",
            Self::GhFlagValue => "gh-flag-value",
            Self::GhFileBody => "gh-file-body",
        }
    }
}

/// 出ていく字面 1 つ（出所の種別と本文・照合は本文 1 つの中だけで行い、本文を跨がない）。
#[derive(Debug, Clone, Copy)]
pub struct Text<'a> {
    /// 出所の種別。
    pub source: Source,
    /// 本文。
    pub body: &'a str,
}

/// 隣の材料（隣ごと）: 当たりの `@` の後ろに出す名札と、照合する repo の名の列。
#[derive(Debug, Clone, Default)]
pub struct Neighbor {
    /// 名札。
    pub tag: String,
    /// repo の名の列。
    pub names: Vec<String>,
}

/// 公開先の材料: 既に公開の名の列（その字面は既に公開されているので照合しない）。
#[derive(Debug, Clone, Default)]
pub struct Public {
    /// 既に公開の名の列。
    pub names: Vec<String>,
}

/// 除外の材料: 除外の字句の列（裁定 id は渡さない）。
#[derive(Debug, Clone, Default)]
pub struct Phrases {
    /// 除外の字句の列。
    pub phrases: Vec<String>,
}

/// 成分 1 つ（本文の中の byte の範囲と、大小を畳んだ字）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// 本文の中の byte の範囲。
    pub range: Range<usize>,
    /// 大小を畳んだ字。
    pub folded: String,
}

/// 成分を作る字か（ASCII の英数字と `_`）。
fn is_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// 成分に切る（**1 本**・repo の名・除外の字句・本文の 3 つがこの関数で切る）: ASCII の英数字と `_` の空でない連なりが成分で、
/// それ以外の字（`-` `.` `/`・空白・改行・ASCII の外の字）で切る。
pub fn components(text: &str) -> Vec<Component> {
    let make = |from: usize, to: usize| Component { range: from..to, folded: text.get(from..to).unwrap_or_default().to_ascii_lowercase() };
    let mut found = Vec::new();
    let mut start: Option<usize> = None;
    for (at, c) in text.char_indices() {
        match (is_part(c), start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                found.push(make(from, at));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        found.push(make(from, text.len()));
    }
    found
}

/// 成分の大小を畳んだ字の列。
fn folds(text: &str) -> Vec<String> {
    components(text).into_iter().map(|part| part.folded).collect()
}

/// 本文の成分の列の中で、`needle` と大小を畳んで等しい連続した出現の成分の index の範囲（重なる出現も）。
fn runs(body: &[Component], needle: &[String]) -> Vec<Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }
    let same = |window: &[Component]| window.iter().zip(needle).all(|(part, want)| part.folded == *want);
    body.windows(needle.len()).enumerate().filter(|(_, window)| same(window)).map(|(at, _)| at..at + needle.len()).collect()
}

/// 本文 1 つを切ったもの。
struct Body<'a> {
    /// 本文。
    text: &'a str,
    /// 本文の成分。
    comps: Vec<Component>,
    /// 除外の区間（成分の index の範囲）。
    spans: Vec<Range<usize>>,
}

impl<'a> Body<'a> {
    /// 本文を切り、除外の字句と成分の並びが大小を畳んで等しい区間を除外の区間にする（成分を持たない字句は区間を作らない）。
    fn new(text: &'a str, phrases: &[Vec<String>]) -> Self {
        let comps = components(text);
        let spans = phrases.iter().flat_map(|phrase| runs(&comps, phrase)).collect();
        Self { text, comps, spans }
    }

    /// 成分の範囲が 1 つの除外の区間に収まるか。
    fn excluded(&self, run: &Range<usize>) -> bool {
        self.spans.iter().any(|span| span.start <= run.start && run.end <= span.end)
    }
}

/// 当たり 1 つ（本文の中の位置と、`<形>=<語>@<名札>` の字面）。
struct Hit {
    /// 本文の中の byte の位置。
    at: usize,
    /// 断りの 1 行に出す字面。
    item: String,
}

/// 語と名札の空白・制御字・`,` を `_` に置く（改行を跨ぐ当たりでも断りは 1 行）。
fn safe(text: &str) -> String {
    text.chars().map(|c| if c.is_whitespace() || c.is_control() || c == ',' { '_' } else { c }).collect()
}

/// 当たりを作る。
fn hit(at: usize, word: &str, tag: &str) -> Hit {
    Hit { at, item: format!("{}={}@{}", Form::RepoName.as_str(), safe(word), safe(tag)) }
}

/// 名 1 つの本文の中の当たり。名の成分の列が既に公開の名のどれかと等しければ照合しない。成分を持たない名は字面の部分一致で当て
/// （除外は効かない・空の名は何も当てない）、成分を持つ名は成分の連続した出現で当てて、除外の区間に収まる当たりを落とす。
fn name_hits(body: &Body, name: &str, tag: &str, public: &[Vec<String>]) -> Vec<Hit> {
    let folded = folds(name);
    if folded.is_empty() {
        return if name.is_empty() { Vec::new() } else { body.text.match_indices(name).map(|(at, word)| hit(at, word, tag)).collect() };
    }
    if public.contains(&folded) {
        return Vec::new();
    }
    let word_of = |run: Range<usize>| {
        let parts = body.comps.get(run)?;
        let (from, to) = (parts.first()?.range.start, parts.last()?.range.end);
        Some(hit(from, body.text.get(from..to)?, tag))
    };
    runs(&body.comps, &folded).into_iter().filter(|run| !body.excluded(run)).filter_map(word_of).collect()
}

/// 当たりを（形・語・名札）で畳み、`<件数>:<先頭 5 件>` の 1 つの字面にする（0 件は `None`）。
fn fold(hits: Vec<Hit>) -> Option<String> {
    let mut seen = HashSet::new();
    let items: Vec<String> = hits.into_iter().map(|found| found.item).filter(|item| seen.insert(item.clone())).collect();
    if items.is_empty() {
        return None;
    }
    let shown: Vec<&str> = items.iter().take(SHOWN).map(String::as_str).collect();
    Some(format!("{}:{}", items.len(), shown.join(",")))
}

/// 出ていく字面を隣の repo の名で照合し、除外の区間に収まる当たりを落とし、当たりを本文の順・本文の中の位置の順に並べて畳む。
/// 返りは `<件数>:<先頭 5 件の <形>=<語>@<名札> を , で並べた字面>`（当たりが 0 件は `None`）。行の `form` に repo-name が無い
/// 周は照合しない。
pub fn scan(row: &Elements, texts: &[Text], neighbors: &[Neighbor], public: &Public, excluded: &Phrases) -> Option<String> {
    if !row.forms.contains(&Form::RepoName) {
        return None;
    }
    let public: Vec<Vec<String>> = public.names.iter().map(|name| folds(name)).filter(|parts| !parts.is_empty()).collect();
    let phrases: Vec<Vec<String>> = excluded.phrases.iter().map(|phrase| folds(phrase)).collect();
    let mut hits = Vec::new();
    for text in texts {
        let body = Body::new(text.body, &phrases);
        let mut found: Vec<Hit> = Vec::new();
        for neighbor in neighbors {
            found.extend(neighbor.names.iter().flat_map(|name| name_hits(&body, name, &neighbor.tag, &public)));
        }
        found.sort_by_key(|entry| entry.at);
        hits.extend(found);
    }
    fold(hits)
}

#[cfg(test)]
mod tests {
    use super::super::{Elements, Form};
    use super::{components, scan, Neighbor, Phrases, Public, Source, Text, SOURCES};

    /// 名札 `t` の隣 1 つ。
    fn near(names: &[&str]) -> Vec<Neighbor> {
        vec![Neighbor { tag: "t".to_owned(), names: names.iter().map(|name| (*name).to_owned()).collect() }]
    }

    /// repo-name の要素。
    fn row() -> Elements {
        Elements { forms: vec![Form::RepoName] }
    }

    /// 本文 1 つを照合する（公開の名・除外の字句つき）。
    fn found(names: &[&str], public: &[&str], phrases: &[&str], bodies: &[&str]) -> Option<String> {
        let texts: Vec<Text> = bodies.iter().map(|body| Text { source: Source::CommitMessage, body }).collect();
        let public = Public { names: public.iter().map(|name| (*name).to_owned()).collect() };
        let excluded = Phrases { phrases: phrases.iter().map(|phrase| (*phrase).to_owned()).collect() };
        scan(&row(), &texts, &near(names), &public, &excluded)
    }

    /// 除外も公開の名も無い照合。
    fn plain(names: &[&str], bodies: &[&str]) -> Option<String> {
        found(names, &[], &[], bodies)
    }

    /// (a) 出所は閉じた 9 値で、宣言順が const slice の順。
    #[test]
    fn publish_names_sources_are_the_closed_nine() {
        let words: Vec<&str> = SOURCES.iter().map(|source| source.as_str()).collect();
        assert_eq!(
            words,
            ["commit-message", "author-name", "committer-name", "patch-added", "patch-path", "tag-body", "ref-name", "gh-flag-value", "gh-file-body"]
        );
    }

    /// 成分は ASCII の英数字と `_` の連なりで、大小を畳み、他の字で切る。
    #[test]
    fn publish_names_components_cut_at_every_other_char() {
        let cut = |text: &str| components(text).into_iter().map(|part| part.folded).collect::<Vec<_>>();
        assert_eq!(cut("Owner/proj-A.b_c 隣のprojを\nx"), ["owner", "proj", "a", "b_c", "proj", "x"]);
        assert!(cut("-/. 隣").is_empty());
    }

    /// (b) 当たる 6 形と当たらない 2 形・本文を跨がない・連続でない並び・成分を持たない名。
    #[test]
    fn publish_names_match_a_run_of_components_case_folded() {
        for (name, body, word) in [
            ("proj", "proj-x", "proj"),
            ("proj", "PROJ", "PROJ"),
            ("proj", "owner/proj", "proj"),
            ("proj", "隣のprojを", "proj"),
            ("proj-a", "Proj.A", "Proj.A"),
            ("proj-a", "proj a", "proj_a"),
        ] {
            assert_eq!(plain(&[name], &[body]), Some(format!("1:repo-name={word}@t")), "{name} / {body}");
        }
        for (name, body) in [("proj", "projx"), ("proj", "proj_x")] {
            assert_eq!(plain(&[name], &[body]), None, "{name} / {body}");
        }
        assert_eq!(plain(&["proj-a"], &["proj", "a"]), None, "本文を跨がない");
        assert_eq!(plain(&["proj-a"], &["proj-b a"]), None, "連続でない");
        assert_eq!(plain(&["a-b"], &["x a b y"]).as_deref(), Some("1:repo-name=a_b@t"), "空白を挟んでも続く");
        assert_eq!(plain(&["--"], &["a--b"]).as_deref(), Some("1:repo-name=--@t"), "成分を持たない名は字面の部分一致");
        assert_eq!(plain(&["--"], &["a-b"]), None);
        assert_eq!(plain(&[""], &["a-b"]), None, "空の名は何も当てない");
    }

    /// (c) 除外の字句の区間に収まる名の当たりだけを落とす。
    #[test]
    fn publish_names_exclude_drops_only_hits_inside_the_phrase_run() {
        let phrase = ["--proj-accent"];
        for body in ["a { color: var(--proj-accent); }", "var(--PROJ-Accent)", "proj.accent", "x PROJ accent y"] {
            assert_eq!(found(&["proj"], &[], &phrase, &[body]), None, "落ちる: {body}");
        }
        for body in ["accent-proj", "var(--proj-accent) then proj-a."] {
            assert_eq!(found(&["proj"], &[], &phrase, &[body]).as_deref(), Some("1:repo-name=proj@t"), "残る: {body}");
        }
        assert_eq!(found(&["accent-x"], &[], &phrase, &["proj-accent-x"]).as_deref(), Some("1:repo-name=accent-x@t"), "はみ出す");
        assert_eq!(found(&["proj-a"], &[], &phrase, &["--proj-accent proj-a."]).as_deref(), Some("1:repo-name=proj-a@t"), "区間の外");
        assert_eq!(found(&["-"], &[], &phrase, &["proj-accent"]).as_deref(), Some("1:repo-name=-@t"), "成分を持たない名は残る");
        assert_eq!(found(&["proj"], &[], &["--", ""], &["proj-accent"]).as_deref(), Some("1:repo-name=proj@t"), "成分を持たない字句は区間を作らない");
    }

    /// (d) 既に公開の名と成分の列が同じ名と、`form` に repo-name の無い行は当たらない。
    #[test]
    fn publish_names_skip_public_names_and_rows_without_the_form() {
        assert_eq!(found(&["proj-a", "other"], &["Proj.A"], &[], &["proj-a other"]).as_deref(), Some("1:repo-name=other@t"));
        assert_eq!(found(&["proj-a"], &["proj"], &[], &["proj-a"]).as_deref(), Some("1:repo-name=proj-a@t"), "列が違えば照合する");
        let texts = [Text { source: Source::RefName, body: "proj" }];
        let no_form = Elements { forms: vec![Form::ObjectId] };
        assert_eq!(scan(&no_form, &texts, &near(&["proj"]), &Public::default(), &Phrases::default()), None);
        assert_eq!(scan(&Elements::default(), &texts, &near(&["proj"]), &Public::default(), &Phrases::default()), None);
    }

    /// (e) 件数と先頭 5 件へ畳む: 同じ語 3 回は 1 件・6 件は件数 6 と位置の順の先頭 5 件・本文の順が先・改行を跨ぐ当たりは 1 行・0 件は無し。
    #[test]
    fn publish_names_fold_to_the_count_and_the_first_five() {
        assert_eq!(plain(&["proj"], &["proj proj proj"]).as_deref(), Some("1:repo-name=proj@t"));
        assert_eq!(plain(&["p1", "p2", "p3", "p4", "p5", "p6"], &["p6 p5 p4 p3 p2 p1"]).as_deref(), Some(
            "6:repo-name=p6@t,repo-name=p5@t,repo-name=p4@t,repo-name=p3@t,repo-name=p2@t"
        ));
        assert_eq!(plain(&["aa", "bb"], &["bb", "aa bb"]).as_deref(), Some("2:repo-name=bb@t,repo-name=aa@t"), "本文の順が先");
        assert_eq!(plain(&["a-b"], &["a\nb"]).as_deref(), Some("1:repo-name=a_b@t"));
        assert_eq!(plain(&["a,b"], &["x a,b"]).as_deref(), Some("1:repo-name=a_b@t"), "語の , も _");
        let spaced = vec![Neighbor { tag: "t a,b".to_owned(), names: vec!["proj".to_owned()] }];
        let texts = [Text { source: Source::PatchAdded, body: "proj" }];
        let got = scan(&row(), &texts, &spaced, &Public::default(), &Phrases::default());
        assert_eq!(got.as_deref(), Some("1:repo-name=proj@t_a_b"), "名札の空白と , も _");
        assert_eq!(plain(&["proj"], &["nothing", ""]), None);
    }
}
