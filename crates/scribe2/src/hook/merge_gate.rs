//! merge の門（設計 docs/design/vessel-hook.md §21・契約表の行 mg・ADR-0088 (7)・FR92 / AC62・FR24 / NFR5）: anchor の門の直後・
//! 権能 guard の前の 1 段で、`gh pr merge` が本文に発端の trailer（`<Name>-Source: <bead id> …`）か器の便の trailer
//! （`run: <run id>`）を渡さなければ実行の前に欠けを名指して断る。見分けは anchor の門の [`is_pr_merge`]、語の読みは publish の
//! [`flag_of`] / [`is_stdin`] / [`is_loose_path`]、key は land と同じ導出の [`source_key`]（2 本目を書かない・C2）。当たる segment の
//! 無い Bash は何も読まずに通し、読むのは本文の file だけ（台帳・git・event log・置き場を読まない・読めない周は断る）。

use super::anchor_guard::is_pr_merge;
use super::host_guard::publish::{flag_of, is_loose_path, is_stdin};
use super::host_guard::verb_of;
use super::ledger_guard::{is_assignment, segments};
use crate::name::NAME;
use crate::pipe::land::{source_key, RUN_TRAILER};
use crate::polarity::{OnFailure, Polarity, Timing};
use std::path::Path;

/// この境界の極性: 実行の時点で止め、本文を読めない周は断る。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 値が本文の flag（gh の `-b, --body`）。
const BODY: [&str; 2] = ["--body", "-b"];
/// 値が本文の file の flag（gh の `-F, --body-file`・`-` は標準入力）。
const BODY_FILE: [&str; 2] = ["--body-file", "-F"];
/// rebase の方式の flag（長い名・値が false の形なら rebase でない）。
const REBASE: &str = "--rebase";
/// rebase の方式の flag（1 字）。
const REBASE_SHORT: &str = "-r";
/// 続きの字に `r` を持てば rebase を束ねた語と読む 1 字の flag（squash・merge・branch の削除）。
const BUNDLED: [&str; 3] = ["-s", "-m", "-d"];
/// 真偽の値が false の形（pflag の読み）。
const FALSE_FORMS: [&str; 6] = ["0", "f", "F", "false", "FALSE", "False"];
/// ここから後ろの語を読まない区切り。
const END: &str = "--";

/// 本文の file の読み手（歯が差し替える・読めない理由は 1 語）。
type Read<'a> = &'a dyn Fn(&Path) -> Result<String, &'static str>;

/// 門の判定（閉じた 2 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeDecision {
    /// 通す。
    Pass,
    /// 断る（理由＝記録の `merge-deny` の後ろの 1 語・stderr の 1 行）。
    Deny(Reason, String),
}

/// 断りの理由（閉じた 5 語・宣言順＝判定の順・§21 形 6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// rebase の方式（本文を渡せない）。
    Rebase,
    /// 本文の源が 0。
    NoBody,
    /// 本文を読めない（形 4）。
    BodyUnreadable,
    /// 形の外の Source の行を持つ。
    BadSource,
    /// run の行も形の内の Source の行も無い。
    NoTrailer,
}

/// [`Reason`] の全 variant（宣言順）。
pub const REASONS: &[Reason] = &[Reason::Rebase, Reason::NoBody, Reason::BodyUnreadable, Reason::BadSource, Reason::NoTrailer];

impl Reason {
    /// 記録と 1 行の `reason=` の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rebase => "rebase",
            Self::NoBody => "no-body",
            Self::BodyUnreadable => "body-unreadable",
            Self::BadSource => "bad-source",
            Self::NoTrailer => "no-trailer",
        }
    }
}

/// 本文 1 つを §21 形 2 (e) で分けた結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Trailer {
    /// run の行か形の内の Source の行が在る（通る）。
    Found,
    /// 形の外の Source の行（出てきた順・(a) の後の字）。
    Bad(Vec<String>),
    /// どちらも無い。
    Missing,
}

/// trailer の文法（**1 関数**・§21 形 2 (a)〜(e)）: 各行の末尾の CR・半角空白・tab を落とし、行頭が key（末尾の空白を落とした字）の
/// 行を Source の行、`run: ` と run id の形だけの行を run の行と読む。形の外の Source の行が 1 行でも在れば [`Trailer::Bad`]。
pub(crate) fn classify(body: &str, key: &str) -> Trailer {
    let head = key.trim_end();
    let (mut found, mut bad) = (false, Vec::new());
    for raw in body.lines() {
        let line = raw.trim_end_matches(['\r', ' ', '\t']);
        if let Some(rest) = line.strip_prefix(head) {
            match rest.strip_prefix(' ').filter(|ids| ids.split(' ').all(is_bead_id)) {
                Some(_) => found = true,
                None => bad.push(line.to_owned()),
            }
        } else if line.strip_prefix(RUN_TRAILER).is_some_and(is_run_id) {
            found = true;
        }
    }
    match (bad.is_empty(), found) {
        (false, _) => Trailer::Bad(bad),
        (true, true) => Trailer::Found,
        (true, false) => Trailer::Missing,
    }
}

/// bead id の閉じた形 `<接頭辞>-<段>(.<段>)*`（§21 形 2 (c)・xtask の `is_bead_id` と同じ形を見本 1 本で守る）。
fn is_bead_id(id: &str) -> bool {
    let segment = |part: &str| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit());
    let Some((prefix, rest)) = id.split_once('-') else {
        return false;
    };
    prefix.starts_with(|ch: char| ch.is_ascii_lowercase()) && segment(prefix) && rest.split('.').all(segment)
}

/// run id の閉じた形 `<bead id>-<YYYYMMDD>T<HHMMSS>Z`（§21 形 2 (d)）。
fn is_run_id(run: &str) -> bool {
    let Some((bead, stamp)) = run.rsplit_once('-') else {
        return false;
    };
    let digits = |text: &str, len: usize| text.len() == len && text.chars().all(|ch| ch.is_ascii_digit());
    let shaped = stamp.strip_suffix('Z').and_then(|rest| rest.split_once('T'));
    shaped.is_some_and(|(date, time)| digits(date, 8) && digits(time, 6)) && is_bead_id(bead)
}

/// 本文の源 1 つ（書かれた flag の名・値〔本文か file の path〕・値が file の path か）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Source {
    flag: String,
    value: String,
    file: bool,
}

/// segment の gh の後ろの語を先頭から読み、rebase の方式を持つかと本文の源の列（書かれた順）を返す（`--` から後ろは読まない・
/// §21 形 3 / 形 5）。値を取る他の flag の表は持たない。
pub(crate) fn reading_of(words: &[String]) -> (bool, Vec<Source>) {
    let lead = words.iter().take_while(|word| is_assignment(word)).count();
    let rest = verb_of(words.get(lead..).unwrap_or_default()).map(|(_, rest)| rest).unwrap_or_default();
    let (mut rebase, mut sources, mut at) = (false, Vec::new(), 0_usize);
    while let Some(word) = rest.get(at) {
        at = at.saturating_add(1);
        if word == END {
            break;
        }
        let Some((name, inline)) = flag_of(word) else {
            continue;
        };
        rebase |= (name == REBASE && inline.is_none_or(|value| !FALSE_FORMS.contains(&value)))
            || name == REBASE_SHORT
            || (BUNDLED.contains(&name) && inline.is_some_and(|value| value.contains('r')));
        let file = BODY_FILE.contains(&name);
        if !file && !BODY.contains(&name) {
            continue;
        }
        let value = inline.map_or_else(|| rest.get(at).cloned().unwrap_or_default(), str::to_owned);
        at = at.saturating_add(usize::from(inline.is_none()));
        sources.push(Source { flag: name.to_owned(), value, file });
    }
    (rebase, sources)
}

/// 本文の file を読む（link の先で測った通常の file だけを UTF-8 で全部）。
fn read_file(path: &Path) -> Result<String, &'static str> {
    let meta = std::fs::metadata(path).map_err(|err| if err.kind() == std::io::ErrorKind::NotFound { "missing" } else { "unreadable" })?;
    if !meta.is_file() {
        return Err("not-a-file");
    }
    String::from_utf8(std::fs::read(path).map_err(|_| "unreadable")?).map_err(|_| "not-utf8")
}

/// 源 1 つの本文（§21 形 4）: `--body` の値は展開の字を持てば読めない。`--body-file` の値は標準入力の形・展開の字・`~`
/// 始まりを断り、他は cwd から解いて（絶対 path はそのまま）`read` で読む。
fn text_of(source: &Source, cwd: &Path, read: Read) -> Result<String, &'static str> {
    let value = source.value.as_str();
    match source.file {
        _ if is_loose_path(value) => Err("expansion"),
        false => Ok(value.to_owned()),
        true if is_stdin(value) => Err("stdin"),
        true if value.starts_with('~') => Err("tilde"),
        true => read(&cwd.join(value)),
    }
}

/// segment 1 つの断り（判定の順に rebase → no-body → body-unreadable → bad-source → no-trailer・源は全部が通ることを求める）。
fn refusal((rebase, sources): &(bool, Vec<Source>), cwd: &Path, read: Read, head: &str) -> Option<(Reason, String)> {
    if *rebase {
        return Some((Reason::Rebase, "--rebase / -r は各 commit を元の本文のまま main へ入れ、本文を渡せない（--squash で撃つ）".to_owned()));
    }
    if sources.is_empty() {
        let why = format!("本文の源（--body / -b / --body-file / -F）が無い — 既定の本文は PR の commit の message の列で、{head} の行を持たない");
        return Some((Reason::NoBody, why));
    }
    let texts = sources.iter().map(|source| text_of(source, cwd, read).map(|text| (source, classify(&text, head))).map_err(|why| (source, why)));
    let judged: Vec<(&Source, Trailer)> = match texts.collect() {
        Ok(judged) => judged,
        Err((source, why)) => {
            let why = format!("flag={} why={why} — 展開の後の本文・標準入力・~ の path・無い file・通常の file でないもの・UTF-8 でない file は読めない", source.flag);
            return Some((Reason::BodyUnreadable, why));
        }
    };
    let bad: String = judged.iter().filter_map(|(_, trailer)| match trailer {
        Trailer::Bad(lines) => Some(lines.iter().map(|line| format!("「{line}」")).collect::<String>()),
        _ => None,
    }).collect();
    if !bad.is_empty() {
        let why = format!("形の外の Source の行 {bad} — key の直後に半角空白 1 つと bead id を置き、複数は半角空白 1 つで区切る形だけを読む");
        return Some((Reason::BadSource, why));
    }
    let (source, _) = judged.iter().find(|(_, trailer)| *trailer == Trailer::Missing)?;
    let why = format!("flag={} の本文に {head} の行も run: <run id> の行も無い — key は行頭に書く（字下げした行と大小の違う行は数えない）", source.flag);
    Some((Reason::NoTrailer, why))
}

/// 断りの 1 行（理由・欠けの名指し・本文の約束・次の一手の literal・改行を持たない）。
fn line_of(reason: Reason, why: &str, head: &str) -> String {
    let flat = why.replace(['\n', '\r'], " ");
    format!(
        "{NAME}: deny merge-gate reason={} {flat}。main へ入る commit の本文の約束: 行頭の {head} の後ろに半角空白 1 つと bead id \
         （複数は半角空白 1 つで区切る）か、run: <run id> の行（FR92・vessel-hook.md §21）。次の一手: 本文を file に書き、最後の段落に \
         {head} <bead id> を置いて gh pr merge <PR> --squash --body-file <絶対 path> で撃つ",
        reason.as_str()
    )
}

/// 判定の本体（`read` は本文の file の読み手）: 当たる segment を command の順に判じ、最初の断りを返す。
fn judge(command: &str, cwd: &Path, read: Read) -> MergeDecision {
    let key = source_key();
    let head = key.trim_end();
    for words in segments(command).iter().filter(|words| is_pr_merge(words)) {
        if let Some((reason, why)) = refusal(&reading_of(words), cwd, read, head) {
            return MergeDecision::Deny(reason, line_of(reason, &why, head));
        }
    }
    MergeDecision::Pass
}

/// 門の入口（Bash の command 行・payload の `cwd`）。当たる segment が無ければ何も読まずに通す。
pub fn decide(command: &str, cwd: &Path) -> MergeDecision {
    judge(command, cwd, &read_file)
}

#[cfg(test)]
mod tests {
    use super::{classify, judge, read_file, reading_of, MergeDecision, Read, Reason, Trailer, REASONS};
    use crate::hook::ledger_guard::segments;
    use crate::name::NAME;
    use crate::order::is_declaration_order;
    use crate::pipe::land::source_key;
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};

    /// 判定の理由（通る周は `None`・cwd は `/w`）。
    fn reason(command: &str, read: Read) -> Option<Reason> {
        match judge(command, Path::new("/w"), read) {
            MergeDecision::Pass => None,
            MergeDecision::Deny(reason, _) => Some(reason),
        }
    }

    /// 1 つ目の segment の読み（源の数・rebase か）。
    fn read(line: &str) -> (usize, bool) {
        let (rebase, sources) = reading_of(segments(line).first().expect("segment が在る"));
        (sources.len(), rebase)
    }

    /// (a) 見本 file の全塊が期待どおり（source と run は通過・none は Source の行を持てば bad-source・持たなければ
    /// no-trailer）で、CR LF の行の本文も通る。塊の数と 4 つの期待それぞれの数を同時に出す。
    #[test]
    fn hook_merge_trailer_sample_cases_match_expected_words() {
        let key = source_key();
        let head = key.trim_end();
        let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/xtask/src/source_trailer_cases.txt"))
            .expect("見本を workspace の path で読める");
        let blocks: Vec<Vec<&str>> = text.split("\n===\n").map(|block| block.lines().filter(|line| !line.starts_with('#')).collect()).collect();
        let mut counts = [0_usize; 4];
        for (word, body) in blocks.iter().filter_map(|block| block.split_first()).map(|(word, body)| (*word, body.join("\n"))) {
            let (expected, slot) = match word {
                "source" | "run" => (Trailer::Found, usize::from(word == "run")),
                _ if body.lines().any(|line| line.starts_with(head)) => (Trailer::Bad(Vec::new()), 2),
                _ => (Trailer::Missing, 3),
            };
            let judged = classify(&body, head);
            assert_eq!(std::mem::discriminant(&judged), std::mem::discriminant(&expected), "{word}: {body:?} → {judged:?}");
            counts[slot] += 1;
        }
        assert!(counts.iter().sum::<usize>() >= 24 && counts.iter().all(|count| *count > 0), "source / run / bad / missing = {counts:?}");
        assert_eq!(classify(&format!("要旨\r\n\r\n{key}s2-a.1\r\n"), head), Trailer::Found, "CR LF の行");
    }

    /// (b) flag の読み: 本文の 4 つの flag の全ての書き方・源 2 つ・`--` の後ろ・rebase の形。片方だけ trailer を持つ源 2 つは断る。
    #[test]
    fn hook_merge_trailer_flags_read_every_body_form_and_rebase() {
        for tail in ["--body X", "-b X", "-bX", "-b=X", "--body=X", "--body-file F", "-F F", "-FF", "--body-file=F"] {
            assert_eq!(read(&format!("gh pr merge 1 {tail}")), (1, false), "{tail}");
        }
        assert_eq!(read("gh pr merge 1 -b X -F F"), (2, false));
        assert_eq!(read("gh pr merge 1 -- --body X"), (0, false), "-- の後ろは読まない");
        assert_eq!(read("gh pr merge 1 -sb X"), (0, false), "束ねた -b は本文と読まない");
        for rebase in ["--rebase", "-r", "-rs", "-dr", "--rebase=true"] {
            assert!(read(&format!("gh pr merge 1 {rebase}")).1, "{rebase}");
        }
        for other in ["--rebase=false", "-sd", "--squash", "--merge", "-m"] {
            assert!(!read(&format!("gh pr merge 1 {other}")).1, "{other}");
        }
        let key = source_key();
        let good = |_: &Path| -> Result<String, &'static str> { Ok(format!("{key}s2-a.1")) };
        let none = |_: &Path| -> Result<String, &'static str> { Ok("要旨だけ".to_owned()) };
        let both = format!("gh pr merge 1 -b '{key}s2-a.1' -F f");
        assert_eq!(reason(&both, &good), None, "両方が通る");
        assert_eq!(reason(&both, &none), Some(Reason::NoTrailer), "本文の側だけ trailer を持つ");
        assert_eq!(reason("gh pr merge 1 -b 要旨 -F f", &good), Some(Reason::NoTrailer), "file の側だけ trailer を持つ");
        assert_eq!(reason("gh pr merge 1 --rebase -F f", &good), Some(Reason::Rebase), "rebase が先");
    }

    /// (c) 値の読み: 展開の字・標準入力・`~`・無い file・dir は body-unreadable で、相対 path は cwd から解いて読む。
    #[test]
    fn hook_merge_trailer_values_refuse_unreadable_and_read_relative_files() {
        let body = format!("{}s2-a.1", source_key());
        let asked = RefCell::new(Vec::new());
        let read = |path: &Path| -> Result<String, &'static str> {
            asked.borrow_mut().push(path.to_path_buf());
            Ok(body.clone())
        };
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        for tail in ["--body \"$B\"", "--body '`x`'", "-F -", "-F /dev/stdin", "-F \"$F\"", "--body-file ~b.md"] {
            assert_eq!(reason(&format!("gh pr merge 1 {tail}"), &read), Some(Reason::BodyUnreadable), "{tail}");
        }
        assert!(asked.borrow().is_empty(), "読めない形は file を開かない");
        assert_eq!(reason("gh pr merge 1 -F b.md", &read), None);
        assert_eq!(asked.borrow().as_slice(), [PathBuf::from("/w/b.md")], "cwd から解く");
        assert_eq!((read_file(here), read_file(&here.join("no-such-body.md"))), (Err("not-a-file"), Err("missing")), "dir・無い file");
        for tail in [here.to_path_buf(), here.join("no-such-body.md")] {
            assert_eq!(reason(&format!("gh pr merge 1 -F {}", tail.display()), &read_file), Some(Reason::BodyUnreadable), "{tail:?}");
        }
    }

    /// (d) 見分け: 連結の後ろの merge の segment を読み、merge でない command は本文の読み手を呼ばずに通す。
    #[test]
    fn hook_merge_trailer_reads_only_merge_segments() {
        let calls = Cell::new(0_usize);
        let read = |_: &Path| -> Result<String, &'static str> {
            calls.set(calls.get() + 1);
            Ok(String::new())
        };
        for line in ["echo gh pr merge 1 -F f", "gh pr view 1 -F f", "ls"] {
            assert_eq!(reason(line, &read), None, "{line}");
        }
        assert_eq!(calls.get(), 0, "本文の読み手を呼ばない");
        assert_eq!(reason("git status && gh pr merge 1 -F f", &read), Some(Reason::NoTrailer), "後ろの segment");
        assert_eq!(calls.get(), 1);
        assert_eq!(reason("git status && gh pr merge 1", &read), Some(Reason::NoBody));
    }

    /// (e) 1 行は改行を持たず、器の名乗りと理由で始まり、key の字・`run: `・次の一手の literal を持ち、bad-source は形の外の
    /// Source の行を全部持つ。理由の const slice は 5 語の宣言順。
    #[test]
    fn hook_merge_trailer_deny_line_names_the_gap_and_the_next_step() {
        let key = source_key();
        let head = key.trim_end();
        let empty = |_: &Path| -> Result<String, &'static str> { Ok(String::new()) };
        let bad = format!("gh pr merge 1 --body \"{key}S2-A.1\n{key}s2-a.1\n{head}\"");
        let cases = [("gh pr merge 1 -r", Reason::Rebase), ("gh pr merge 1 --squash", Reason::NoBody), ("gh pr merge 1 -F -", Reason::BodyUnreadable)];
        for (command, want) in cases.into_iter().chain([(bad.as_str(), Reason::BadSource), ("gh pr merge 1 --body 要旨", Reason::NoTrailer)]) {
            let MergeDecision::Deny(reason, line) = judge(command, Path::new("/w"), &empty) else {
                panic!("{command}: 断る");
            };
            assert_eq!(reason, want, "{command}");
            assert!(!line.contains(['\n', '\r']) && line.starts_with(&format!("{NAME}: deny merge-gate reason={} ", want.as_str())), "{line}");
            for part in [head, "run: ", "gh pr merge <PR> --squash --body-file <絶対 path>", "vessel-hook.md §21"] {
                assert!(line.contains(part), "{command}: {part}: {line}");
            }
            let named = [format!("「{key}S2-A.1」"), format!("「{head}」")];
            assert_eq!(named.iter().all(|found| line.contains(found.as_str())), want == Reason::BadSource, "{line}");
        }
        let words: Vec<&str> = REASONS.iter().map(|reason| reason.as_str()).collect();
        assert_eq!(words, ["rebase", "no-body", "body-unreadable", "bad-source", "no-trailer"]);
        assert!(is_declaration_order(REASONS, |reason| reason as usize), "宣言順");
    }
}
