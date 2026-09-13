//! prose-gate（散文部の門・暫定）: tracked な `docs/design/*.md` の散文で、**規範の印を持つ文**が
//! 出所の pointer を持ち、数 + 単位の値を持たないことを測る（設計 docs/design/contract-source.md
//! §12・§9 (f)・憲法 N2 / C1.2・SRS FR52・`s2-07l.202`）。
//!
//! 散文にしか無い規則は規則ではない（N2）。印を持つ文は pointer で規則の正本を名指すか、値を
//! 持たない形でなければならず、閾値の数は manifest の 1 面へ置く（C1.2）。印の一覧・節の終わり・
//! 単位の中身は本 file の const が正本である（設計 doc へ写さない）。
//!
//! **文 = 「。」または行末で区切る**（planner 裁定 2026-09-13）: 行を跨ぐ文は行末で切れ、印を持つ片が
//! pointer を失えば違反として表に出る（fail-closed の極性・見出しや箇条書きの行が次の行の pointer に
//! 救われない）。母集団から外すのは code fence の内側・行頭 `|` の表の行・行頭 `<!--` の行だけ。
//! 読めない file・列挙できない index・対象 0 本は違反に倒す（paths-clean と同じ極性）。依存は足さない。

use crate::check::{failed, Layout, Measured};
use crate::paths_clean::{body_of, Tracked, TrackedFile};

/// 判定行の tag。
const TAG: &str = "prose-gate";

/// 対象 file の dir（repo root からの相対・直下の file だけ）。
const DOC_DIR: &str = "docs/design/";

/// 対象 file の拡張子。
const DOC_EXT: &str = ".md";

/// 述語形の印（宣言順）。NOT 付きの英語形は部分一致で含まれる。副詞（「必ず」等）は数えない。
pub(crate) const MARKS: &[&str] = &["しなければならない", "してはならない", "してはいけない", "SHALL", "MUST"];

/// 直後が [`CLAUSE_END`] のときだけ印になる語（「禁止列挙」の名詞句は印でない）。
const PROHIBITION: &str = "禁止";

/// [`PROHIBITION`] を印にする直後の文字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClauseEnd {
    /// 句点「。」。
    Period,
    /// 閉じ括弧「）」。
    CloseParen,
    /// 中黒「・」。
    MiddleDot,
    /// 読点「、」。
    Comma,
    /// 行末（直後に文字が無い）。
    LineEnd,
}

/// 節の終わりの全 variant（宣言順）。
pub(crate) const CLAUSE_END: &[ClauseEnd] = &[
    ClauseEnd::Period,
    ClauseEnd::CloseParen,
    ClauseEnd::MiddleDot,
    ClauseEnd::Comma,
    ClauseEnd::LineEnd,
];

impl ClauseEnd {
    /// 直後の文字としての字面（行末は文字を持たない）。
    fn char(self) -> Option<char> {
        match self {
            Self::Period => Some('。'),
            Self::CloseParen => Some('）'),
            Self::MiddleDot => Some('・'),
            Self::Comma => Some('、'),
            Self::LineEnd => None,
        }
    }
}

/// 数字列の直後に来ると「数 + 単位」になる単位（宣言順）。
pub(crate) const UNITS: &[&str] = &["秒", "分", "時間", "日", "件", "本", "行", "byte", "KB", "MB", "%", "s", "ms"];

/// 違反の理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    /// 印を持つ文が pointer を 1 つも持たない。
    NoPointer,
    /// 印を持つ文が数 + 単位を持つ。
    NumberWithUnit,
}

/// 理由の全 variant（宣言順＝1 文に 2 つ当たるときは先の理由で名指す）。
pub(crate) const REASONS: &[Reason] = &[Reason::NoPointer, Reason::NumberWithUnit];

impl Reason {
    /// 違反行に載せる理由の名。
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NoPointer => "no-pointer",
            Self::NumberWithUnit => "number-with-unit",
        }
    }

    /// 印を持つ文 `sentence`（pointer の範囲 `spans`）にこの理由が当たるか。
    fn holds(self, sentence: &str, spans: &[(usize, usize)]) -> bool {
        match self {
            Self::NoPointer => spans.is_empty(),
            Self::NumberWithUnit => has_number_with_unit(sentence, spans),
        }
    }
}

/// 1 件の違反（file 名を持たない＝判定は本文だけから決まる）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Violation {
    /// 1 始まりの行番号。
    pub(crate) line: usize,
    /// 理由。
    pub(crate) reason: Reason,
    /// 文の先頭 60 字（前後の空白を落とした形）。
    pub(crate) head: String,
}

impl Violation {
    /// 出力行 `<file>:<line>: <理由の名> <文の先頭 60 字>`。
    pub(crate) fn render(&self, rel: &str) -> String {
        format!("{rel}:{}: {} {}", self.line, self.reason.as_str(), self.head)
    }
}

/// 違反行に載せる文の先頭の字数。
const HEAD_CHARS: usize = 60;

/// 印を持つ文 1 つ。
struct Sentence<'a> {
    /// 1 始まりの行番号。
    line: usize,
    /// 文の本文（「。」を含む）。
    text: &'a str,
}

/// 本文の中で印を持つ文を順に返す（母集団）。
fn marked_sentences(text: &str) -> Vec<Sentence<'_>> {
    let mut found = Vec::new();
    let mut in_fence = false;
    for (index, raw) in text.lines().enumerate() {
        if raw.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || raw.starts_with('|') || raw.starts_with("<!--") {
            continue;
        }
        found.extend(
            raw.trim_end()
                .split_inclusive('。')
                .filter(|piece| is_marked(piece))
                .map(|piece| Sentence { line: index.saturating_add(1), text: piece }),
        );
    }
    found
}

/// 文が印を持つか。文の末尾は「。」か行末なので、[`PROHIBITION`] の直後が空なら行末である。
fn is_marked(sentence: &str) -> bool {
    MARKS.iter().any(|mark| sentence.contains(mark))
        || sentence.match_indices(PROHIBITION).any(|(at, word)| {
            let next = sentence.get(at.saturating_add(word.len())..).and_then(|after| after.chars().next());
            CLAUSE_END.iter().any(|end| end.char() == next)
        })
}

/// 母集団の数（印を持つ文の数）。
pub(crate) fn population(text: &str) -> usize {
    marked_sentences(text).len()
}

/// 本文の違反を行順に返す（pure・I/O なし）。
pub(crate) fn violations(text: &str) -> Vec<Violation> {
    marked_sentences(text)
        .into_iter()
        .filter_map(|sentence| {
            let spans = pointer_spans(sentence.text);
            let reason = REASONS.iter().copied().find(|reason| reason.holds(sentence.text, &spans))?;
            Some(Violation {
                line: sentence.line,
                reason,
                head: sentence.text.trim().chars().take(HEAD_CHARS).collect(),
            })
        })
        .collect()
}

/// pointer の byte 範囲を左から重ならずに返す。
///
/// 形 = 憲法条 `C<n>[.<m>]` / `A<n>` / `N<n>[.<m>]`・`FR<n>` `AC<n>` `NFR<n>`・`R-<…>`・
/// `ADR-<4 桁>`・`§<n>`・契約 id `<doc>#<row>`・bead id `s2-…`。英字で始まる形は直前が ASCII 英数字で
/// ない位置でだけ読む（識別子の途中を pointer にしない）。
pub(crate) fn pointer_spans(sentence: &str) -> Vec<(usize, usize)> {
    let chars: Vec<(usize, char)> = sentence.char_indices().collect();
    let offset = |index: usize| chars.get(index).map_or(sentence.len(), |(at, _)| *at);
    let mut spans = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        match pointer_len(&chars, index) {
            Some(len) => {
                spans.push((offset(index), offset(index.saturating_add(len))));
                index = index.saturating_add(len);
            }
            None => index = index.saturating_add(1),
        }
    }
    spans
}

/// `chars[index]` から始まる pointer の字数。
fn pointer_len(chars: &[(usize, char)], index: usize) -> Option<usize> {
    let at = |k: usize| chars.get(k).map(|(_, ch)| *ch);
    let digits = |from: usize| (from..).take_while(|k| at(*k).is_some_and(|ch| ch.is_ascii_digit())).count();
    let word_char = |ch: char| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.');
    let prefixed = |prefix: &str| {
        prefix
            .chars()
            .enumerate()
            .all(|(k, ch)| at(index.saturating_add(k)) == Some(ch))
            .then(|| index.saturating_add(prefix.chars().count()))
    };
    // `<n>[.<m>]` の字数（数字が無ければ 0）。
    let dotted = |from: usize, allow_dot: bool| {
        let major = digits(from);
        let dot = from.saturating_add(major);
        let minor = digits(dot.saturating_add(1));
        if major > 0 && allow_dot && at(dot) == Some('.') && minor > 0 {
            major.saturating_add(1).saturating_add(minor)
        } else {
            major
        }
    };
    let current = at(index)?;
    if current == '#' {
        let before = index.checked_sub(1).and_then(at).is_some_and(word_char);
        let row = (index.saturating_add(1)..)
            .take_while(|k| at(*k).is_some_and(|ch| ch.is_ascii_alphanumeric()))
            .count();
        return (before && row > 0).then(|| row.saturating_add(1));
    }
    if current == '§' {
        let len = dotted(index.saturating_add(1), true);
        return (len > 0).then(|| len.saturating_add(1));
    }
    if index.checked_sub(1).and_then(at).is_some_and(|ch| ch.is_ascii_alphanumeric()) {
        return None;
    }
    if let Some(from) = prefixed("ADR-") {
        return (digits(from) == 4).then_some(8);
    }
    for (prefix, allow_dot) in [("NFR", false), ("FR", false), ("AC", false), ("C", true), ("N", true), ("A", false)] {
        if let Some(from) = prefixed(prefix) {
            let len = dotted(from, allow_dot);
            if len > 0 {
                return Some(from.saturating_sub(index).saturating_add(len));
            }
        }
    }
    for prefix in ["R-", "s2-"] {
        if let Some(from) = prefixed(prefix) {
            let tail = (from..).take_while(|k| at(*k).is_some_and(word_char)).count();
            if at(from).is_some_and(|ch| ch.is_ascii_alphanumeric()) {
                return Some(from.saturating_sub(index).saturating_add(tail));
            }
        }
    }
    None
}

/// pointer の外に「数字列 + 空白 1 つまで + 単位」が在るか。
///
/// 英字の単位は直後が英字でないときだけ単位と読む（`4 skills` を `4 s` にしない・複数形の `s` 1 字は可）。
fn has_number_with_unit(sentence: &str, spans: &[(usize, usize)]) -> bool {
    let mut previous_digit = false;
    for (at, ch) in sentence.char_indices() {
        let starts_run = ch.is_ascii_digit() && !previous_digit;
        previous_digit = ch.is_ascii_digit();
        if !starts_run || spans.iter().any(|(start, end)| (*start..*end).contains(&at)) {
            continue;
        }
        let tail = sentence.get(at..).unwrap_or_default().trim_start_matches(|c: char| c.is_ascii_digit());
        let tail = tail.strip_prefix(' ').unwrap_or(tail);
        if UNITS.iter().any(|unit| unit_at(tail, unit)) {
            return true;
        }
    }
    false
}

/// `tail` が単位 `unit` で始まるか（英字の単位は語の境界まで見る）。
fn unit_at(tail: &str, unit: &str) -> bool {
    let Some(after) = tail.strip_prefix(unit) else {
        return false;
    };
    if !unit.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return true;
    }
    let after = after.strip_prefix('s').unwrap_or(after);
    !after.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic())
}

/// 対象 file か（`docs/design/` 直下の `.md`）。
fn is_target(rel: &str) -> bool {
    rel.strip_prefix(DOC_DIR)
        .is_some_and(|name| !name.contains('/') && name.ends_with(DOC_EXT))
}

/// tracked な `docs/design/*.md` を測り `prose-gate=<違反数>/<母集団>` を出す。
pub(crate) fn measure(layout: &Layout) -> Measured {
    match crate::paths_clean::tracked_files(&layout.root) {
        Tracked::Unmeasurable(reason) => failed(TAG, &reason),
        Tracked::NotRepoRoot => Measured {
            fact: format!("{TAG}=n/a(not-a-repo-root)"),
            violations: Vec::new(),
        },
        Tracked::Listed(listed) => {
            let targets: Vec<TrackedFile> = listed.into_iter().filter(|file| is_target(&file.rel)).collect();
            if targets.is_empty() {
                return failed(TAG, &format!("tracked な {DOC_DIR}*{DOC_EXT} が 0 本である"));
            }
            scan(layout, &targets)
        }
    }
}

/// 対象を 1 本ずつ判定する（読めない file は違反）。
fn scan(layout: &Layout, targets: &[TrackedFile]) -> Measured {
    let mut lines = Vec::new();
    let mut found = 0_usize;
    let mut marked = 0_usize;
    for file in targets {
        let rel = &file.rel;
        let text = body_of(&layout.root, file)
            .and_then(|bytes| String::from_utf8(bytes).map_err(|err| format!("UTF-8 でない: {err}")));
        match text {
            Err(reason) => {
                found = found.saturating_add(1);
                lines.push(format!("{TAG}: {rel} を読めない: {reason}（読めない file は違反である）"));
            }
            Ok(body) => {
                marked = marked.saturating_add(population(&body));
                for violation in violations(&body) {
                    found = found.saturating_add(1);
                    lines.push(format!("{TAG}: {}", violation.render(rel)));
                }
            }
        }
    }
    Measured {
        fact: format!("{TAG}={found}/{marked}"),
        violations: lines,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        pointer_spans, population, scan, violations, ClauseEnd, Reason, CLAUSE_END, MARKS, REASONS, UNITS,
    };
    use crate::check::Layout;
    use crate::paths_clean::TrackedFile;
    use std::fs;

    /// 違反行を render した形で返す（fixture の file 名は入力の本文に現れない字面）。
    fn rendered(text: &str) -> Vec<String> {
        violations(text).iter().map(|found| found.render("probe-doc-3k.md")).collect()
    }

    /// 印を持つ文 3 つ（pointer 無し・数 + 単位付き・適合）で違反 2 件を file:line 付きで名指す。
    #[test]
    fn prose_gate_names_violations_with_file_and_line() {
        let doc = "# 見出し\n\n席は lock を確保しなければならない。\n判定は 30 秒で打ち切りを実行しなければならない（C16）。\n器は失敗を記録しなければならない（N2）。\n";
        assert_eq!(
            rendered(doc),
            vec![
                "probe-doc-3k.md:3: no-pointer 席は lock を確保しなければならない。".to_owned(),
                "probe-doc-3k.md:4: number-with-unit 判定は 30 秒で打ち切りを実行しなければならない（C16）。".to_owned(),
            ]
        );
        assert_eq!(population(doc), 3, "印を持つ文は 3 つ");
    }

    /// 適合だけの fixture は 0 件（母集団は在る＝空虚な 0 ではない）。
    #[test]
    fn prose_gate_passes_conforming_doc() {
        let doc = "器は失敗を記録しなければならない（N2）。\n値は 1 面に置く。rules を読まずに発火してはならない（ADR-0020 §3）。\n";
        assert_eq!(rendered(doc), Vec::<String>::new());
        assert_eq!(population(doc), 2);
    }

    /// 副詞「必ず」だけの文は母集団外。
    #[test]
    fn prose_gate_does_not_count_adverbs() {
        let doc = "器は必ず失敗を記録する。\n";
        assert_eq!(population(doc), 0);
        assert!(violations(doc).is_empty());
    }

    /// 「禁止」は直後が節の終わりのときだけ印（名詞句「禁止列挙」は母集団外）。
    #[test]
    fn prose_gate_counts_prohibition_only_before_clause_end() {
        assert_eq!(population("prompt の禁止列挙に足した。\n"), 0, "名詞句は印でない");
        assert_eq!(population("env の直読は禁止に当たる。\n"), 0, "助詞が続く形は印でない");
        for (doc, reason) in [
            ("env の直読は禁止。\n", Some(Reason::NoPointer)),
            ("器（env の直読禁止）で読む。\n", Some(Reason::NoPointer)),
            ("器は env の直読禁止、読むのは 1 面。\n", Some(Reason::NoPointer)),
            ("env の直読は禁止\n", Some(Reason::NoPointer)),
            ("**最初の違反で止めない**（silent drop 禁止・NFR4）。\n", None),
        ] {
            assert_eq!(population(doc), 1, "節の終わりの前の禁止は印: {doc}");
            assert_eq!(violations(doc).first().map(|found| found.reason), reason, "{doc}");
        }
    }

    /// 印を持つ行の次の行にだけ pointer が在る文は、印の行を NoPointer で名指す（行末で切れる）。
    #[test]
    fn prose_gate_cuts_sentences_at_line_end() {
        let doc = "- 器は失敗を記録しなければならない\n  （C1）。\n";
        assert_eq!(rendered(doc), vec!["probe-doc-3k.md:1: no-pointer - 器は失敗を記録しなければならない".to_owned()]);
    }

    /// code fence の内側・行頭 `|` の表の行・行頭 `<!--` の行は母集団外（fence の後は戻る）。
    #[test]
    fn prose_gate_skips_fences_tables_and_comments() {
        let doc = "```\n席は確保しなければならない。\n```\n| 席は確保しなければならない。 |\n<!-- 席は確保しなければならない。 -->\n席は確保しなければならない。\n";
        assert_eq!(population(doc), 1, "fence を閉じた後の文だけが母集団");
        assert_eq!(rendered(doc), vec!["probe-doc-3k.md:6: no-pointer 席は確保しなければならない。".to_owned()]);
    }

    /// pointer の各形を読む（識別子の途中は読まない）。
    #[test]
    fn prose_gate_reads_every_pointer_form() {
        for pointer in [
            "C1", "C12.7", "A3", "N2.1", "FR52", "AC25", "NFR4", "R-C4-1", "ADR-0020", "§12", "contract-source.md#f", "s2-07l.202",
        ] {
            let sentence = format!("器は {pointer} を守らなければならない。");
            assert_eq!(pointer_spans(&sentence).len(), 1, "{sentence}");
        }
        for plain in ["ABC1 を読む", "ADR-12 を読む", "#[attr] を読む", "BC1 を読む", "s2 を読む"] {
            assert_eq!(pointer_spans(plain), Vec::new(), "{plain}");
        }
    }

    /// 数 + 単位は間の空白 1 つまで・英字の単位は語の境界まで・pointer の数字は数えない。
    #[test]
    fn prose_gate_reads_number_with_unit_forms() {
        let reason = |body: &str| {
            let doc = format!("器は {body} で停止しなければならない（C1）。\n");
            assert_eq!(population(&doc), 1, "印を持つ文のはず: {doc}");
            violations(&doc).first().map(|found| found.reason)
        };
        for hit in ["30秒", "30 秒", "5ms", "10 bytes", "80%", "2 時間", "3 本"] {
            assert_eq!(reason(hit), Some(Reason::NumberWithUnit), "{hit}");
        }
        for miss in ["4 skills", "30  秒", "C12 本", "ADR-0013 本文"] {
            assert_eq!(reason(miss), None, "{miss}");
        }
    }

    /// const slice 3 本の宣言順と件数の pin。
    #[test]
    fn prose_gate_pins_const_slices() {
        assert_eq!(MARKS, ["しなければならない", "してはならない", "してはいけない", "SHALL", "MUST"]);
        assert_eq!(
            CLAUSE_END,
            [ClauseEnd::Period, ClauseEnd::CloseParen, ClauseEnd::MiddleDot, ClauseEnd::Comma, ClauseEnd::LineEnd]
        );
        assert_eq!(CLAUSE_END.len(), 5);
        assert_eq!(
            CLAUSE_END.iter().map(|end| end.char()).collect::<Vec<_>>(),
            [Some('。'), Some('）'), Some('・'), Some('、'), None]
        );
        assert_eq!(UNITS, ["秒", "分", "時間", "日", "件", "本", "行", "byte", "KB", "MB", "%", "s", "ms"]);
    }

    /// 理由 enum の `as_str` と宣言順。
    #[test]
    fn prose_gate_pins_reason_names_in_order() {
        assert_eq!(REASONS.iter().map(|reason| reason.as_str()).collect::<Vec<_>>(), ["no-pointer", "number-with-unit"]);
    }

    /// 読めない file は違反・fact は `<違反数>/<母集団>`。
    #[test]
    fn prose_gate_scan_counts_unreadable_file_as_violation() {
        let dir = std::env::temp_dir().join(format!("xtask-prose-gate-{}", std::process::id()));
        let docs = dir.join("docs/design");
        let made = fs::create_dir_all(&docs).is_ok() && fs::write(docs.join("ok.md"), "席は確保しなければならない。\n").is_ok();
        let tracked = |rel: &str| TrackedFile { rel: rel.to_owned(), mode: "100644".to_owned(), oid: String::new() };
        let layout = Layout { root: dir.clone(), core_dir: dir.clone(), member_dirs: Vec::new(), name: "probe".to_owned() };
        let measured = scan(&layout, &[tracked("docs/design/ok.md"), tracked("docs/design/gone.md")]);
        fs::remove_dir_all(&dir).ok();
        assert!(!dir.exists(), "後始末できる");
        assert!(made, "fixture を書ける");
        assert_eq!(measured.fact, "prose-gate=2/1");
        assert_eq!(measured.violations.len(), 2, "{:?}", measured.violations);
        assert!(measured.violations.iter().any(|line| line.contains("docs/design/gone.md を読めない")));
        assert!(measured.violations.iter().any(|line| line.starts_with("prose-gate: docs/design/ok.md:1: no-pointer")));
    }

    /// 決定的な擬似乱数（依存を足さない property の材料）。
    struct Rng(u64);

    impl Rng {
        fn pick<'a>(&mut self, pool: &[&'a str]) -> &'a str {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            let len = u64::try_from(pool.len()).unwrap_or(1).max(1);
            usize::try_from(self.0 % len).ok().and_then(|at| pool.get(at)).copied().unwrap_or_default()
        }
    }

    /// 印・pointer・数を持たない散文の片。
    const PLAIN: &[&str] = &["器は席を起こす", "判定行を読む", "値は manifest に置く", "lens が差分を見る", "台帳へ記帳する"];
    /// 印の述語（「禁止。」の形を含む）。
    const PREDICATES: &[&str] = &["しなければならない", "してはならない", "してはいけない", "SHALL", "MUST NOT", "禁止"];
    /// pointer（括弧で包む）。
    const POINTERS: &[&str] = &["（C1）", "（C12.7）", "（N2）", "（FR52）", "（ADR-0020）", "（§12）", "（s2-07l.202）", "（R-C4-1）"];
    /// 数 + 単位（読点で区切る）。
    const QUANTITIES: &[&str] = &["、30 秒", "、5ms", "、3 件", "、80%", "、2 時間", "、10 byte"];

    /// 回す周の数。
    const CASES: u64 = 256;

    /// 適合する文に pointer を足しても適合のまま。
    #[test]
    fn prop_prose_gate_adding_pointer_keeps_conforming() {
        for seed in 1..=CASES {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let (plain, pointer, predicate) = (rng.pick(PLAIN), rng.pick(POINTERS), rng.pick(PREDICATES));
            let base = format!("{plain}{pointer}{predicate}。");
            let added = format!("{plain}{pointer}{}{predicate}。", rng.pick(POINTERS));
            for doc in [base, added] {
                assert_eq!(population(&doc), 1, "{doc}");
                assert!(violations(&doc).is_empty(), "{doc}");
            }
        }
    }

    /// 印を持つ文に `<数><単位>` を足すと違反。
    #[test]
    fn prop_prose_gate_adding_quantity_violates() {
        for seed in 1..=CASES {
            let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
            let (plain, pointer, predicate) = (rng.pick(PLAIN), rng.pick(POINTERS), rng.pick(PREDICATES));
            let conforming = format!("{plain}{pointer}{predicate}。");
            assert!(violations(&conforming).is_empty(), "{conforming}");
            let doc = format!("{plain}{}{pointer}{predicate}。", rng.pick(QUANTITIES));
            assert_eq!(violations(&doc).first().map(|found| found.reason), Some(Reason::NumberWithUnit), "{doc}");
        }
    }

    /// 印を持たない文は何を足しても母集団外。
    #[test]
    fn prop_prose_gate_unmarked_stays_outside() {
        let anything: Vec<&str> = [PLAIN, POINTERS, QUANTITIES].concat();
        for seed in 1..=CASES {
            let mut rng = Rng(seed.wrapping_mul(0x94D0_49BB_1331_11EB));
            let doc = format!("{}{}{}{}。\n", rng.pick(PLAIN), rng.pick(&anything), rng.pick(&anything), rng.pick(&anything));
            assert_eq!(population(&doc), 0, "{doc}");
            assert!(violations(&doc).is_empty(), "{doc}");
        }
    }

    /// 文の順序を入れ替えても件数不変。
    #[test]
    fn prop_prose_gate_order_does_not_change_counts() {
        for seed in 1..=CASES {
            let mut rng = Rng(seed.wrapping_mul(0xBF58_476D_1CE4_E5B9));
            let sentences: Vec<String> = (0..4)
                .map(|_| {
                    let pointer = if rng.pick(&["y", "n"]) == "y" { rng.pick(POINTERS) } else { "" };
                    let quantity = if rng.pick(&["y", "n"]) == "y" { rng.pick(QUANTITIES) } else { "" };
                    let predicate = if rng.pick(&["y", "n"]) == "y" { rng.pick(PREDICATES) } else { "する" };
                    format!("{}{quantity}{pointer}{predicate}。", rng.pick(PLAIN))
                })
                .collect();
            let forward = sentences.concat();
            let backward: String = sentences.iter().rev().map(String::as_str).collect();
            assert_eq!(population(&forward), population(&backward), "{forward}");
            assert_eq!(violations(&forward).len(), violations(&backward).len(), "{forward}");
        }
    }
}
