//! 裁定 id の引用を数える閉じた規則 1 本と、その解け方・線・doctor の 1 行（設計 docs/design/dispatcher.md §36・契約表の行 ak・
//! FR83 / AC53・ADR-0083 (5)）。
//!
//! (1) 字面から 4 形を拾う [`scan`]（純関数・左から右へ 1 回・拾いは重ならない）と、vessel 宣言の ruling-fixtures との完全一致で
//! 外す [`is_fixture`]。(2) 台帳の notes の裁定の行に解ける [`Index`] と [`verdict`]（台帳を読めない周は「測れない」）。(3) opt-in の線
//! [`line_commit`] と、線の木に同じ file の同じ字面が在るか [`before_line`]（`git grep` 1 回）。(4) doctor の 1 行 [`doctor_line`]。
//!
//! 台帳の型は名指さない——台帳の読みは呼び手が行い、閉じた問いの notes と全 bead の notes の字の列（[`Notes`]）だけを渡す
//! （呼び手は受付・着地の留め・未反映の判定・doctor）。走査の対象は HEAD の追跡された file の字面で、binary は外す。

use super::close_reason::{is_ruling_id, ruling_row};
use super::form::field;
use crate::invocation::Invocation;
use crate::pipe::declaration::{ruling_keys_at, DECL_FILE};
use crate::seat::brief::pointer::Anchor;
use std::collections::BTreeSet;
use std::path::Path;

/// doctor の行の先頭の字面。
pub const PREFIX: &str = "ruling-cite:";

/// 引用の頭 `batch:`。
const BATCH: &str = "batch:";

/// 引用の頭 `policy:`。
const POLICY: &str = "policy:";

/// 時刻の形の頭 `user `（末尾の空白を含む）。
const USER: &str = "user ";

/// 問い id の形の時刻の部分 `YYYYMMDDTHHMMZ` の桁数。
const MINUTE_LEN: usize = 14;

/// 時刻の形 `YYYY-MM-DDTHH:M?` の型（`d` は数字・`?` は数字か `x`・ほかは字のまま）。この後ろに `Z` か `:SSZ` が続く。
const TIME_SHAPE: &[u8] = b"dddd-dd-ddTdd:d?";

/// opt-in の線を測る ref（main の first-parent の履歴）。
const MAIN: &str = "refs/heads/main";

/// 走査する tree。
const HEAD: &str = "HEAD";

/// 拾う形（閉じた 4 つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Form {
    /// 台帳の接頭辞の問い id の形（`<bead id>:<YYYYMMDDTHHMMZ>-<n>`）。
    Question,
    /// `batch:` の形。
    Batch,
    /// `policy:` の形。
    Policy,
    /// 時刻の形（`user YYYY-MM-DDTHH:M?Z`・`:SS` があってもよい）。
    Time,
}

/// 拾った引用 1 つ（字面は書式を外さない・末尾の句読点だけを剥がす）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cite {
    /// 形。
    pub form: Form,
    /// 字面。
    pub text: String,
}

/// 字の中から引用を拾う（**純関数**・閉じた規則 1 本）。`prefix` は台帳の接頭辞（解けない周は `None`＝問い id の形は拾わない）。
///
/// 走査は左から右へ 1 回で、拾った字面の後ろから続ける（`policy:batch:x` は 1 件）。書式（backtick・引用符）はどの字面も外さない。
pub fn scan(text: &str, prefix: Option<&str>) -> Vec<Cite> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(rest) = text.get(at..).filter(|rest| !rest.is_empty()) {
        let before = text.get(..at).and_then(|head| head.chars().next_back());
        let hit = question_at(rest, before, prefix).or_else(|| named_at(rest, before)).or_else(|| time_at(rest));
        let step = match hit {
            Some(cite) => {
                let taken = cite.text.len();
                found.push(cite);
                taken
            }
            None => rest.chars().next().map_or(1, char::len_utf8),
        };
        at = at.saturating_add(step);
    }
    found
}

/// (a) 問い id の形: 前の字が英数字・`_`・`.`・`-` でない位置から始まり、[`is_ruling_id`] が台帳の接頭辞の問い id と読む最長の字面。
fn question_at(rest: &str, before: Option<char>, prefix: Option<&str>) -> Option<Cite> {
    let prefix = prefix.filter(|found| !found.is_empty())?;
    if before.is_some_and(|found| found.is_ascii_alphanumeric() || matches!(found, '_' | '.' | '-')) {
        return None;
    }
    let after = rest.strip_prefix(prefix)?.strip_prefix('-')?;
    let id = after.bytes().take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'.').count();
    let tail = after.get(id..)?.strip_prefix(':')?;
    let count = tail.get(MINUTE_LEN..)?.strip_prefix('-')?;
    let digits = count.bytes().take_while(u8::is_ascii_digit).count();
    // 頭の `<接頭辞>-`・`:`・`-` の 3 つの区切りと、id・時刻・n の長さの和。
    let len = [prefix.len(), 1, id, 1, MINUTE_LEN, 1, digits].iter().fold(0, |sum: usize, part| sum.saturating_add(*part));
    let text = rest.get(..len)?;
    is_ruling_id(text, Some(prefix)).then(|| Cite { form: Form::Question, text: text.to_owned() })
}

/// (b) `batch:` / `policy:` の形: 前の字が英数字でも `_` でもない位置から始まり、コロンの直後は ASCII の英数字 1 字で、その後に英数字と
/// `.`・`_`・`:`・`/`・`-` が続く。末尾の `.`・`_`・`:`・`/`・`-` は剥がす。
fn named_at(rest: &str, before: Option<char>) -> Option<Cite> {
    let (form, head) = if rest.starts_with(BATCH) {
        (Form::Batch, BATCH)
    } else if rest.starts_with(POLICY) {
        (Form::Policy, POLICY)
    } else {
        return None;
    };
    if before.is_some_and(|found| found.is_ascii_alphanumeric() || found == '_') {
        return None;
    }
    let tail = rest.get(head.len()..)?;
    if !tail.bytes().next().is_some_and(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    let run = tail.bytes().take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-')).count();
    let raw = rest.get(..head.len().saturating_add(run))?;
    Some(Cite { form, text: raw.trim_end_matches(['.', '_', ':', '/', '-']).to_owned() })
}

/// (c) 時刻の形: `user ` の後に `YYYY-MM-DDTHH:M` と `[0-9x]` 1 字と `Z` が続く字面（`:SS` があってもよい）。
fn time_at(rest: &str) -> Option<Cite> {
    let tail = rest.strip_prefix(USER)?;
    let fits = tail.len() >= TIME_SHAPE.len()
        && TIME_SHAPE.iter().zip(tail.as_bytes()).all(|(want, got)| match want {
            b'd' => got.is_ascii_digit(),
            b'?' => got.is_ascii_digit() || *got == b'x',
            literal => got == literal,
        });
    if !fits {
        return None;
    }
    let after = tail.get(TIME_SHAPE.len()..)?;
    let seconds = after.strip_prefix(':').filter(|found| found.get(..2).is_some_and(|two| two.bytes().all(|byte| byte.is_ascii_digit())) && found.get(2..).is_some_and(|end| end.starts_with('Z')));
    let closing = match (after.starts_with('Z'), seconds) {
        (true, _) => 1,
        (false, Some(_)) => 4,
        (false, None) => return None,
    };
    let len = USER.len().saturating_add(TIME_SHAPE.len()).saturating_add(closing);
    Some(Cite { form: Form::Time, text: rest.get(..len)?.to_owned() })
}

/// ruling-fixtures が引用を数えの外にするか（一覧との完全一致だけ・wildcard の型は持たない）。外すのは `batch:` / `policy:` の形と、
/// **線より後の**時刻の形だけで、問い id の形は一覧に載っていても外さない。`before_line` は線より前の時刻の形（外さず線の前に数える）。
pub fn is_fixture(cite: &Cite, fixtures: &[String], before_line: bool) -> bool {
    let listed = || fixtures.contains(&cite.text);
    match cite.form {
        Form::Question => false,
        Form::Batch | Form::Policy => listed(),
        Form::Time => !before_line && listed(),
    }
}

/// 台帳の notes（呼び手が台帳を読んで渡す字の列）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notes {
    /// 閉じた問い（label `intake:question`・status closed）の notes。
    pub closed_questions: Vec<String>,
    /// 全 bead（閉じた問いも開いた問いも含む）の notes。
    pub every: Vec<String>,
}

/// 台帳の裁定の行から作る、解けるの索引。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    /// 閉じた問いの裁定の行の裁定 id の欄。
    question_ids: BTreeSet<String>,
    /// 全 bead の裁定の行の裁定 id の欄と束の欄。
    every_ids: BTreeSet<String>,
}

/// 解けるかの 3 値。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 解ける。
    Resolved,
    /// 解けない。
    Unresolved,
    /// 測れない（台帳を読めない周）。呼び手が断る側へ倒す。
    Unmeasured,
}

impl Index {
    /// 台帳の notes から索引を作る。`prefix` は台帳の接頭辞（裁定の行の先頭の欄を読む）。
    pub fn new(notes: &Notes, prefix: Option<&str>) -> Self {
        let question_ids = rows_of(&notes.closed_questions, prefix).map(|(id, _)| id).collect();
        let every_ids = rows_of(&notes.every, prefix).flat_map(|(id, bundles)| std::iter::once(id).chain(bundles)).collect();
        Self { question_ids, every_ids }
    }
}

/// notes の裁定の行の（裁定 id の欄・束の欄の列）。行の読みは [`ruling_row`] の 1 本で、束の欄は同じ行を `|` で割った欄から読む。
fn rows_of<'a>(notes: &'a [String], prefix: Option<&'a str>) -> impl Iterator<Item = (String, Vec<String>)> + 'a {
    notes.iter().flat_map(|text| text.lines()).filter_map(move |line| ruling_row(line, prefix).map(|row| (row.id, bundles_of(line))))
}

/// 束の欄: 先頭の欄とも最後の欄（逐語）とも違う欄のうち、欄の字全体が `batch:` / `policy:` の形のもの（0 個以上）。
fn bundles_of(line: &str) -> Vec<String> {
    let fields: Vec<&str> = line.split('|').map(str::trim).collect();
    let middle = fields.get(1..fields.len().saturating_sub(1)).unwrap_or_default();
    middle.iter().filter(|text| is_whole_named(text)).map(|text| (*text).to_owned()).collect()
}

/// 欄の字全体が (b) の形か（欄の中を走査して語を拾い直さない）。
fn is_whole_named(text: &str) -> bool {
    matches!(scan(text, None).as_slice(), [Cite { form: Form::Batch | Form::Policy, text: found }] if found == text)
}

/// 引用が台帳に解けるか。`index` が `None`（台帳を読めない周）の問い id の形と `batch:` / `policy:` の形は [`Verdict::Unmeasured`]。
/// 問い id の形は閉じた問いの裁定の行の裁定 id の欄だけに解け、`batch:` / `policy:` の形は全 bead の裁定の行の裁定 id の欄か束の欄に解ける
/// （どちらも前後の空白を剥いだ欄の字全体との完全一致）。時刻の形は台帳に解けない（線の前かで呼び手が分ける）。
pub fn verdict(index: Option<&Index>, cite: &Cite) -> Verdict {
    let found = |listed: &dyn Fn(&Index) -> bool| match index {
        None => Verdict::Unmeasured,
        Some(index) if listed(index) => Verdict::Resolved,
        Some(_) => Verdict::Unresolved,
    };
    match cite.form {
        Form::Time => Verdict::Unresolved,
        Form::Question => found(&|index| index.question_ids.contains(&cite.text)),
        Form::Batch | Form::Policy => found(&|index| index.every_ids.contains(&cite.text)),
    }
}

/// 台帳の接頭辞（repo の `.beads` から解く・解けない周は `None`）。
pub fn prefix_of(repo: &Path) -> Option<String> {
    Anchor::open(repo)?.prefixes().first().cloned()
}

/// 読めなかった面（doctor の行の閉じた 3 語・宣言 → 台帳 → git の順に最初に読めなかった 1 つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// 宣言（`.vessel.toml`）が在って読めない。
    Declaration,
    /// 台帳を読めない。
    Ledger,
    /// git を読めない（main の ref が無い・log か grep が落ちる・線が引けない）。
    Git,
}

impl Reason {
    /// 行に出す語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Declaration => "declaration",
            Self::Ledger => "ledger",
            Self::Git => "git",
        }
    }
}

/// git を 1 回撃って stdout を返す（cwd は `repo`・rc 0 だけ。`nothing_ok` の周は grep の「1 件も無い」＝rc 1 で stdout が空も返す）。
fn git_run(repo: &Path, args: &[String], nothing_ok: bool) -> Option<String> {
    let out = Invocation::new("git").arg("-C").arg(repo).args(args).output().ok()?;
    let ok = out.status.code().is_some_and(|code| code == 0 || (nothing_ok && code == 1 && out.stdout.is_empty()));
    ok.then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 字の列から引数の列を作る。
fn strings<const N: usize>(words: [&str; N]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// opt-in の線: main の first-parent の履歴で `.vessel.toml` を変えた commit の全部（古い順）のうち、宣言の `ruling-check` が初めて
/// true と読める commit の sha。`ruling-check` の行を変えない commit（壊れた宣言の別の行を直して初めて読めるようになった commit）も
/// 候補で、読めない宣言と false の commit は線にならない。どの commit も true でない周は `Ok(None)`、main の ref か log を読めない周は `Err`。
pub fn line_commit(repo: &Path) -> Result<Option<String>, Reason> {
    git_run(repo, &strings(["rev-parse", "--verify", "-q", MAIN]), false).ok_or(Reason::Git)?;
    let log = strings(["log", "--first-parent", "--reverse", "--format=%H", MAIN, "--", DECL_FILE]);
    let candidates = git_run(repo, &log, false).ok_or(Reason::Git)?;
    Ok(candidates.lines().find(|sha| ruling_keys_at(repo, sha).is_ok_and(|keys| keys.check)).map(str::to_owned))
}

/// `git grep` の出力（`<rev>:<path>` NUL 字 改行 の 1 行 1 件）を（path・字）にする。
fn grep_hits<'a>(out: &'a str, rev: &'a str) -> impl Iterator<Item = (String, &'a str)> + 'a {
    let lead = format!("{rev}:");
    out.lines().filter_map(move |line| {
        let (name, text) = line.split_once('\0')?;
        Some((name.strip_prefix(lead.as_str())?.to_owned(), text))
    })
}

/// 引用の（path・字面）のうち、線の commit の木に同じ file の同じ字面が在るもの（**`git grep` 1 回**・線の前の時刻の形）。
pub fn before_line(repo: &Path, line: &str, cited: &BTreeSet<(String, String)>) -> Result<BTreeSet<(String, String)>, Reason> {
    let texts: BTreeSet<&String> = cited.iter().map(|(_, text)| text).collect();
    if texts.is_empty() {
        return Ok(BTreeSet::new());
    }
    let mut args = strings(["grep", "-I", "-F", "-o", "-z"]);
    args.extend(texts.into_iter().flat_map(|text| [String::from("-e"), text.clone()]));
    args.push(line.to_owned());
    let out = git_run(repo, &args, true).ok_or(Reason::Git)?;
    let seen: BTreeSet<(String, String)> = grep_hits(&out, line).map(|(path, text)| (path, text.to_owned())).collect();
    Ok(cited.intersection(&seen).cloned().collect())
}

/// HEAD の追跡された file の字面から拾った引用（走査した file の数と、file ごとの引用）。
struct Population {
    /// 走査した file の数（binary と空の file は外す）。
    files: usize,
    /// （path・引用）。
    cites: BTreeSet<(String, Cite)>,
}

/// `rev` の tree を `git grep -I` で読み、引用を拾う（粗い候補の行を `git grep` が選び、[`scan`] が拾う）。
fn population(repo: &Path, rev: &str, prefix: Option<&str>) -> Result<Population, Reason> {
    let listed = git_run(repo, &strings(["grep", "-I", "-l", "-z", "-e", "", rev]), true).ok_or(Reason::Git)?;
    let files = listed.split('\0').filter(|name| !name.is_empty()).count();
    let mut args = strings(["grep", "-I", "-F", "-z"]);
    let lead = prefix.map(|found| format!("{found}-"));
    for word in [BATCH, POLICY, USER].into_iter().chain(lead.as_deref()) {
        args.extend([String::from("-e"), word.to_owned()]);
    }
    args.push(rev.to_owned());
    let out = git_run(repo, &args, true).ok_or(Reason::Git)?;
    let cites = grep_hits(&out, rev).flat_map(|(path, text)| scan(text, prefix).into_iter().map(move |cite| (path.clone(), cite))).collect();
    Ok(Population { files, cites })
}

/// `rev`（main の先端の sha など）の追跡された file が引く（path・引用）の集合（[`population`] の 1 回・ruling-check の有無に依らず、
/// fixtures も外さない）。読めない周（git が落ちる）は `Err`（未反映の判定が読めない側へ倒す）。
pub(crate) fn cited_at(repo: &Path, rev: &str, prefix: Option<&str>) -> Result<BTreeSet<(String, Cite)>, Reason> {
    population(repo, rev, prefix).map(|seen| seen.cites)
}

/// doctor の行が出す数（母集団と、解けない引用と、線より前の時刻の形）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// 走査した file の数。
    pub files: usize,
    /// 数えた引用の数（file ごとの字面 1 つを 1 件・ruling-fixtures が外したものは含めない）。
    pub cited: usize,
    /// 解けない引用の字面（昇順・重複を除く。線より後の時刻の形は台帳に解けないのでここに載る）。
    pub unresolved: Vec<String>,
    /// 線より前の時刻の形の字面（昇順・重複を除く）。
    pub before: Vec<String>,
}

/// 拾った引用を数える（線より前の時刻の形は外さず before に、ほかは fixtures を外して解けるかを見る）。
fn report_of(seen: &Population, fixtures: &[String], index: &Index, before: &BTreeSet<(String, String)>) -> Report {
    let mut report = Report { files: seen.files, ..Report::default() };
    let (mut unresolved, mut earlier) = (BTreeSet::new(), BTreeSet::new());
    for (path, cite) in &seen.cites {
        let early = cite.form == Form::Time && before.contains(&(path.clone(), cite.text.clone()));
        if is_fixture(cite, fixtures, early) {
            continue;
        }
        report.cited = report.cited.saturating_add(1);
        if early {
            earlier.insert(cite.text.clone());
        } else if verdict(Some(index), cite) != Verdict::Resolved {
            unresolved.insert(cite.text.clone());
        }
    }
    report.unresolved = unresolved.into_iter().collect();
    report.before = earlier.into_iter().collect();
    report
}

/// 測れた周の 1 行（`check=on`・母集団と件数と字面）。
pub fn render(report: &Report) -> String {
    format!("{PREFIX} check=on files={} cited={} {} {}", report.files, report.cited, field("unresolved", &report.unresolved), field("before-line", &report.before))
}

/// 鍵を持たない repo の 1 行。
pub fn render_off() -> String {
    format!("{PREFIX} check=off")
}

/// 測れていない周の 1 行（閉じた 3 語・件数を 1 つも出さない＝0 に化けさせない・C10）。
pub fn render_unreadable(reason: Reason) -> String {
    format!("{PREFIX} unreadable reason={}", reason.as_str())
}

/// 読みの全部（宣言 → 台帳 → git の順）。`ruling-check` が true でない repo は `Ok(None)`（数えも断りもしない）。
fn measure(repo: &Path, ledger: impl FnOnce() -> Option<Notes>) -> Result<Option<Report>, Reason> {
    let keys = ruling_keys_at(repo, HEAD).map_err(|_| Reason::Declaration)?;
    if !keys.check {
        return Ok(None);
    }
    let notes = ledger().ok_or(Reason::Ledger)?;
    let prefix = prefix_of(repo);
    let line = line_commit(repo)?.ok_or(Reason::Git)?;
    let seen = population(repo, HEAD, prefix.as_deref())?;
    let times = seen.cites.iter().filter(|(_, cite)| cite.form == Form::Time).map(|(path, cite)| (path.clone(), cite.text.clone())).collect();
    let before = before_line(repo, &line, &times)?;
    Ok(Some(report_of(&seen, &keys.fixtures, &Index::new(&notes, prefix.as_deref()), &before)))
}

/// doctor の 1 行（`ruling-cite: …`・`--repo R` の HEAD の宣言と台帳と git を読む）。`ledger` は呼び手が台帳を 1 回だけ読んで作る
/// 材料（`ruling-check` が true の repo だけが呼ぶ・読めない周は `None`）。
pub fn doctor_line(repo: &Path, ledger: impl FnOnce() -> Option<Notes>) -> String {
    match measure(repo, ledger) {
        Ok(Some(report)) => render(&report),
        Ok(None) => render_off(),
        Err(reason) => render_unreadable(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::{is_fixture, scan, verdict, Cite, Form, Index, Notes, Verdict};

    /// 字面の列（形は無視して字面だけ）。
    fn texts(text: &str, prefix: Option<&str>) -> Vec<String> {
        scan(text, prefix).into_iter().map(|cite| cite.text).collect()
    }

    /// 形と字面の列。
    fn forms(text: &str, prefix: Option<&str>) -> Vec<(Form, String)> {
        scan(text, prefix).into_iter().map(|cite| (cite.form, cite.text)).collect()
    }

    /// 引用 1 つ。
    fn cite(form: Form, text: &str) -> Cite {
        Cite { form, text: text.to_owned() }
    }

    /// 4 形（問い id・batch:・policy:・時刻）を 1 度の走査で全部拾い、形と字面が正しい。
    #[test]
    fn cite_scan_picks_the_four_forms() {
        let text = "裁定 s2-07l.739.1:20260928T1347Z-1 と batch:2026-09-28 と policy:p1 と user 2026-09-30T00:00Z を引く";
        assert_eq!(
            forms(text, Some("s2")),
            [
                (Form::Question, "s2-07l.739.1:20260928T1347Z-1".to_owned()),
                (Form::Batch, "batch:2026-09-28".to_owned()),
                (Form::Policy, "policy:p1".to_owned()),
                (Form::Time, "user 2026-09-30T00:00Z".to_owned()),
            ]
        );
        assert_eq!(scan("", Some("s2")), []);
    }

    /// 境界: 前の字が英数字か `_` の `apolicy:x`・`_batch:x`、コロンの直後が空白の `policy: Type`・空の `batch:` は拾わず、
    /// 詰めた `policy:Type` は拾う。`-` や `(` が前に付く字面は拾う。
    #[test]
    fn cite_scan_boundaries_of_the_named_forms() {
        for text in ["apolicy:x", "_batch:x", "sbatch:x", "fn f(policy: Type)", "batch: x", "batch:", "policy:", "batch:.x", "Batch:x"] {
            assert_eq!(texts(text, None), Vec::<String>::new(), "{text}");
        }
        assert_eq!(texts("policy:Type", None), ["policy:Type"], "詰めた書き方は拾う");
        assert_eq!(texts("(batch:x) -policy:y", None), ["batch:x", "policy:y"], "括弧や - の後ろから始まる");
        assert_eq!(texts("一括batch:x", None), ["batch:x"], "前が ASCII の英数字でなければ拾う");
    }

    /// 末尾の `.`・`_`・`:`・`/`・`-` は剥がし、中の区切りは残す。backtick の囲みは外さず（囲みの中の字面も拾う）、字面に backtick を含めない。
    #[test]
    fn cite_scan_strips_trailing_punctuation_and_keeps_the_backtick_form() {
        assert_eq!(texts("batch:x.", None), ["batch:x"]);
        assert_eq!(texts("batch:x._:/-", None), ["batch:x"]);
        assert_eq!(texts("policy:a/b-c.d_e:f、", None), ["policy:a/b-c.d_e:f"]);
        assert_eq!(texts("`batch:x`", None), ["batch:x"], "backtick の中も数える");
        assert_eq!(texts("\"policy:p\"", None), ["policy:p"], "引用符の中も数える");
        assert_eq!(texts("```\nbatch:x\n```", None), ["batch:x"], "code の囲みの中も数える");
    }

    /// 問い id の形は台帳の接頭辞だけ: 接頭辞違い・接頭辞なし・n が 0・時刻の欠け・前が英数字か `.` `-` `_` のものは拾わず、最長の n を拾う。
    #[test]
    fn cite_scan_question_ids_are_the_ledger_prefix_only() {
        let id = "s2-1:20260928T1347Z-1";
        assert_eq!(texts(id, Some("s2")), [id]);
        assert_eq!(texts(id, Some("tz")), Vec::<String>::new(), "接頭辞違い");
        assert_eq!(texts(id, None), Vec::<String>::new(), "接頭辞が解けない周は拾わない");
        assert_eq!(texts("tz-1:20260928T1347Z-1", Some("s2")), Vec::<String>::new());
        for bad in ["s2-1:20260928T1347Z-0", "s2-1:20260928T1347Z-", "s2-1:20261328T1347Z-1", "s2-1:20260928T134Z-1", "s2-1", "s2-1.:20260928T1347Z-1"] {
            assert_eq!(texts(bad, Some("s2")), Vec::<String>::new(), "{bad}");
        }
        for glued in ["as2-1:20260928T1347Z-1", "9s2-1:20260928T1347Z-1", "_s2-1:20260928T1347Z-1", ".s2-1:20260928T1347Z-1", "-s2-1:20260928T1347Z-1"] {
            assert_eq!(texts(glued, Some("s2")), Vec::<String>::new(), "{glued}");
        }
        assert_eq!(texts("(s2-07l.738.1:20260928T1347Z-12).", Some("s2")), ["s2-07l.738.1:20260928T1347Z-12"], "最長の n・後ろの句読点は入らない");
        assert_eq!(texts("s2-1:20260928T1347Z-1 s2-2:20260929T0000Z-7", Some("s2")).len(), 2);
    }

    /// 時刻の形: 分の 2 桁目は数字か x・秒つきも 1 件・`user ` の直後だけ・型が 1 字でも違えば拾わない。
    #[test]
    fn cite_scan_time_form_minutes_and_shape() {
        for good in ["user 2026-09-30T00:00Z", "user 2026-09-30T00:0xZ", "user 2026-09-30T23:59:59Z", "user 2026-09-30T00:0x:07Z"] {
            assert_eq!(texts(good, None), [good], "{good}");
        }
        for bad in [
            "user 2026-09-30T00:xxZ",
            "user 2026-09-30T0x:00Z",
            "user 2026-09-30T00:0yZ",
            "user 2026-09-30T00:00",
            "user 2026-09-30T00:00:0Z",
            "user 2026-09-30T00:00:xxZ",
            "user 2026-9-30T00:00Z",
            "user  2026-09-30T00:00Z",
            "user2026-09-30T00:00Z",
            "2026-09-30T00:00Z",
        ] {
            assert_eq!(texts(bad, None), Vec::<String>::new(), "{bad}");
        }
        assert_eq!(texts("(user 2026-09-30T00:00Z)。", None), ["user 2026-09-30T00:00Z"]);
    }

    /// 拾いは重ならない: `policy:batch:x` は 1 件で字面は全体、隣り合う 2 件は 2 件、問い id の形を含む batch: は 1 件。
    #[test]
    fn cite_scan_picks_do_not_overlap() {
        assert_eq!(forms("policy:batch:x", None), [(Form::Policy, "policy:batch:x".to_owned())]);
        assert_eq!(texts("batch:a batch:b,policy:c", None), ["batch:a", "batch:b", "policy:c"]);
        assert_eq!(texts("batch:s2-1:20260928T1347Z-1", Some("s2")), ["batch:s2-1:20260928T1347Z-1"], "中の問い id を別の件に数えない");
        assert_eq!(texts("user 2026-09-30T00:00Z batch:x", Some("s2")), ["user 2026-09-30T00:00Z", "batch:x"]);
    }

    /// fixtures は完全一致だけ: `batch:a` を載せても `batch:ab` は外さず、`batch:` / `policy:` は載せた字面だけを外し、問い id の形は
    /// 一覧に載せても外さない。時刻の形は線より後だけ外し、線より前は載せても外さない。backtick つきの字面は別の字面。
    #[test]
    fn cite_scan_fixtures_drop_only_exact_matches() {
        let fixtures = ["batch:a".to_owned(), "policy:p".to_owned(), "s2-1:20260928T1347Z-1".to_owned(), "user 2026-09-30T00:0xZ".to_owned(), "`batch:c`".to_owned()];
        let drops = |form, text: &str, before| is_fixture(&cite(form, text), &fixtures, before);
        assert!(drops(Form::Batch, "batch:a", false));
        assert!(drops(Form::Policy, "policy:p", false));
        assert!(!drops(Form::Batch, "batch:ab", false), "完全一致だけ");
        assert!(!drops(Form::Batch, "batch:c", false), "backtick つきの一覧の字面は別の字面");
        assert!(!drops(Form::Policy, "policy:a", false), "形の違う一覧の字面は外さない");
        assert!(!drops(Form::Question, "s2-1:20260928T1347Z-1", false), "問い id の形は一覧に載っていても外さない");
        assert!(drops(Form::Time, "user 2026-09-30T00:0xZ", false), "線より後の時刻の形は外す");
        assert!(!drops(Form::Time, "user 2026-09-30T00:0xZ", true), "線より前は外さない");
        assert!(!drops(Form::Time, "user 2026-09-30T00:00Z", false));
        assert!(!is_fixture(&cite(Form::Batch, "batch:a"), &[], false), "空の一覧は何も外さない");
    }

    /// 解けるの表: 問い id は閉じた問いの裁定 id の欄だけ・batch: / policy: は全 bead の裁定 id の欄と束の欄・
    /// 逐語の欄（の中の部分一致）と最後の欄は解けない・欄は字全体との完全一致・台帳を読めない周は測れない・時刻の形は解けない。
    #[test]
    fn cite_scan_verdicts_follow_the_ruling_row_fields() {
        let question = "s2-q.1:20260930T0000Z-1";
        let other = "s2-q.2:20260930T0100Z-1";
        let closed = format!(
            "  {question} | s2-q.1 | 2026-09-30T00:00Z | chat | 逐語に {other} を引く\n  batch:m1 | s2-q.1 | batch:b7 | 逐語 batch:v9\nbatch:m2 | s2-q.1 | 2026-09-30T00:00Z | batch:v8\n\
             policy:batch:x | s2-q.1 | 2026-09-30T00:00Z | chat | 逐語\nただの行 {other}\n{other} | 3 欄 | だけ"
        );
        let open_note = format!("{other} | s2-q.2 | 2026-09-30T01:00Z | chat | 開いた問い\nbatch:o1 | s2-q.2 | 2026-09-30T01:00Z | chat | 逐語");
        let notes = Notes { closed_questions: vec![closed.clone()], every: vec![closed, open_note] };
        let index = Index::new(&notes, Some("s2"));
        let of = |form, text: &str| verdict(Some(&index), &cite(form, text));
        assert_eq!(of(Form::Question, question), Verdict::Resolved);
        assert_eq!(of(Form::Question, other), Verdict::Unresolved, "開いた問いの行・逐語の欄・3 欄の行だけの id は解けない");
        for text in ["batch:m1", "batch:m2", "policy:batch:x", "batch:o1"] {
            assert_eq!(of(Form::Batch, text), Verdict::Resolved, "{text}: 裁定 id の欄（どの bead でも）");
        }
        assert_eq!(of(Form::Batch, "batch:b7"), Verdict::Resolved, "束の欄");
        assert_eq!(of(Form::Batch, "batch:v9"), Verdict::Unresolved, "逐語の欄の中だけ");
        assert_eq!(of(Form::Batch, "batch:v8"), Verdict::Unresolved, "最後の欄");
        assert_eq!(of(Form::Batch, "batch:x"), Verdict::Unresolved, "裁定 id の欄 policy:batch:x の中の語は解けない");
        assert_eq!(of(Form::Policy, "policy:batch:x"), Verdict::Resolved, "欄の字全体との完全一致");
        assert_eq!(of(Form::Question, "batch:b7"), Verdict::Unresolved, "問い id の形は束の欄に解けない");
        assert_eq!(verdict(None, &cite(Form::Batch, "batch:m1")), Verdict::Unmeasured);
        assert_eq!(verdict(None, &cite(Form::Question, question)), Verdict::Unmeasured);
        assert_eq!(of(Form::Time, "user 2026-09-30T00:00Z"), Verdict::Unresolved, "時刻の形は台帳に解けない");
        assert_eq!(verdict(None, &cite(Form::Time, "user 2026-09-30T00:00Z")), Verdict::Unresolved);
    }

    /// 5 欄の行の束の欄は 0 個（問い・発話の ts・経路の欄は batch: / policy: の形でない）。欄の字全体が形と一致しなければ束の欄にならず、
    /// 位置でなく字の形で見る（先頭と最後の間の欄なら、経路の位置でも字全体が形のものは束の欄）。
    #[test]
    fn cite_scan_bundle_fields_need_the_whole_field() {
        let line = "batch:m1 | s2-q.1 | 2026-09-30T00:00Z | chat | batch:v1\nbatch:m3 | batch:b1. | batch:b2 x | batch:b3 | 逐語";
        let index = Index::new(&Notes { closed_questions: Vec::new(), every: vec![line.to_owned()] }, Some("s2"));
        let of = |text: &str| verdict(Some(&index), &cite(Form::Batch, text));
        assert_eq!(of("batch:m1"), Verdict::Resolved);
        assert_eq!(of("batch:v1"), Verdict::Unresolved, "5 欄の行の最後の欄");
        assert_eq!(of("batch:b3"), Verdict::Resolved, "位置でなく字の形で見る");
        assert_eq!(of("batch:b1"), Verdict::Unresolved, "欄の字全体でない（句読点つき）");
        assert_eq!(of("batch:b2"), Verdict::Unresolved, "欄の字全体でない（後ろに語）");
    }
}
