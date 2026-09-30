//! 着地の留めの判定（設計 docs/design/pipeline.md §62・契約表の行 be・FR83 / FR10 / AC53・数えは dispatcher.md §36 の
//! [`crate::ledger::citation`] の 1 本）。
//!
//! 便の base の宣言が `ruling-check = true` のとき、便の差分（`git -c core.quotePath=false diff --unified=0 <base> HEAD`）が足した行を読み、
//! 留める名指しの列を返す: 解けない引用（`<id>`・`time:<時刻>@<path>`）・問い id の形を持たない判断の欄（`field:<rules|adr|design>@<path>`）・
//! 問い id を足さない `ruling-check` の外し（`ruling-check-off`）・測れなかった面（`unmeasured:<語>`）。空の列は通す。
//!
//! 返すのは素の値だけで、記帳（RunStage）は親の `land.rs` が書く——この子は `Land`・`Stage`・`EventKind`・`Issue` を名指さない
//! （4 つの型はほかの行の touches に在り、子が名指すとその行の閉包に子の file が入る・設計 §62 約束 10）。

use crate::ledger::citation::{before_line, is_fixture, line_commit, prefix_of, scan, verdict, Cite, Form, Index, Notes, Verdict};
use crate::ledger::form::is_question;
use crate::pipe::declaration::ruling_keys_at;
use crate::pipe::git_bytes;
use crate::pipe::table::{BEGIN, END};
use crate::seat::ledger::read_ledger;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

/// 便の木の先端（差分の終わりと、宣言・設計 doc を読む rev）。
const TIP: &str = "HEAD";

/// 閉じた bead の status の字面。
const CLOSED: &str = "closed";

/// 台帳を読めない周の語。
const LEDGER: &str = "ledger";

/// 裁定の欄を名乗る語（設計 doc の見出しと欄の頭）。
const WORD: &str = "裁定";

/// ADR の裁定の欄の字面。
const ROLE: &str = "class=\"role\">裁定";

/// ADR を置く dir。
const ADR_DIR: &str = "design-intent/decisions/";

/// 設計 doc を置く dir。
const DESIGN_DIR: &str = "docs/design/";

/// 判定の材料（素の値だけ）。
pub(super) struct Input<'a> {
    /// 対象 repo（anchor）。宣言は base の rev で、台帳と線はここから読む。
    pub(super) repo: &'a Path,
    /// 便の worktree（差分と先端の宣言と設計 doc を読む）。
    pub(super) worktree: &'a Path,
    /// 便の記録した base。
    pub(super) base: &'a str,
    /// 台帳の client。
    pub(super) bd: &'a str,
    /// 台帳の待ち上限（台帳を読む周だけ呼ぶ・`None` は台帳を読めない周）。
    pub(super) timeout: &'a dyn Fn() -> Option<Duration>,
}

/// 留める名指しの列（並べ替えて重複を除く・空は通す）。
pub(super) fn judge(input: &Input<'_>) -> Vec<String> {
    let Ok(base) = ruling_keys_at(input.repo, input.base) else {
        return Vec::new();
    };
    if !base.check {
        return Vec::new();
    }
    let mut names = BTreeSet::new();
    let (tip_check, mut fixtures) = match ruling_keys_at(input.worktree, TIP) {
        Ok(tip) => (tip.check, tip.fixtures),
        Err(_) => {
            names.insert(unmeasured("declaration"));
            (true, Vec::new())
        }
    };
    fixtures.extend(base.fixtures);
    let args = [
        "-c", "core.quotePath=false", "diff", "--unified=0", "--no-color", "--no-ext-diff", "--no-textconv", "-M", "--src-prefix=a/",
        "--dst-prefix=b/", input.base, TIP, "--",
    ];
    let Some(bytes) = git_bytes(input.worktree, &args) else {
        names.insert(unmeasured("diff"));
        return names.into_iter().collect();
    };
    let diff = Diff::read(&String::from_utf8_lossy(&bytes));
    let prefix = prefix_of(input.repo);
    let rules = Rules { prefix: prefix.as_deref(), fixtures: &fixtures, tip_check };
    let tables = Tables { worktree: input.worktree, seen: RefCell::new(BTreeMap::new()) };
    let in_table = |path: &str, line: u64| tables.contains(path, line);
    names.extend(names_of(&diff, &rules, &in_table, &mut |times| lookup(input, prefix.as_deref(), times)));
    names.into_iter().collect()
}

/// 測れなかった面の名指し。
fn unmeasured(word: &str) -> String {
    format!("unmeasured:{word}")
}

/// 引用の規則（宣言から決めた値）。
struct Rules<'a> {
    /// 台帳の接頭辞（解けない周は `None`＝問い id の形は拾わない）。
    prefix: Option<&'a str>,
    /// 完全一致で外す字面（base と先端の宣言の `ruling-fixtures` の和）。
    fixtures: &'a [String],
    /// 先端の宣言の `ruling-check`。
    tip_check: bool,
}

/// 台帳の索引と、線より前の時刻の形（引用を持つ周だけ読む）。
struct Lookup {
    /// 台帳の裁定の行から作った、解けるの索引。
    index: Index,
    /// 線の木に同じ file の同じ字面が在る時刻の形（path・字面）。
    before: BTreeSet<(String, String)>,
}

impl Lookup {
    /// 引用が解けるか（時刻の形は線の前に在るものだけ・ほかは台帳の裁定の行）。
    fn resolves(&self, path: &str, cite: &Cite) -> bool {
        match cite.form {
            Form::Time => self.before.contains(&(path.to_owned(), cite.text.clone())),
            Form::Question | Form::Batch | Form::Policy => verdict(Some(&self.index), cite) == Verdict::Resolved,
        }
    }
}

/// 時刻の形の（path・字面）の組。
type Times = BTreeSet<(String, String)>;

/// 台帳と線の引き（時刻の形の組を受けて材料か読めなかった語を返す・呼び手が引用を持つ周にだけ呼ぶ）。
type Ask<'a> = &'a mut dyn FnMut(&Times) -> Result<Lookup, &'static str>;

/// 台帳を 1 回読んで索引を作り、opt-in の線を引いて時刻の形を線の木と照らす（読めない周は語・台帳 → 線 → git の順に最初の 1 つ）。
fn lookup(input: &Input<'_>, prefix: Option<&str>, times: &Times) -> Result<Lookup, &'static str> {
    let timeout = (input.timeout)().ok_or(LEDGER)?;
    let issues = read_ledger(input.bd, input.repo, timeout).map_err(|_| LEDGER)?;
    let closed_questions = issues.iter().filter(|found| found.status == CLOSED && is_question(found)).map(|found| found.notes.clone()).collect();
    let notes = Notes { closed_questions, every: issues.iter().map(|found| found.notes.clone()).collect() };
    let line = line_commit(input.repo).map_err(|reason| reason.as_str())?.ok_or("git")?;
    let before = before_line(input.repo, &line, times).map_err(|reason| reason.as_str())?;
    Ok(Lookup { index: Index::new(&notes, prefix), before })
}

/// 差分の足した行から名指しの集合を導く（台帳の引きと設計 doc の表の区間だけを呼び手が渡す）。
fn names_of(diff: &Diff, rules: &Rules<'_>, in_table: &dyn Fn(&str, u64) -> bool, lookup: Ask<'_>) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if diff.unreadable {
        names.insert(unmeasured("diff-path"));
    }
    names.extend(field_names(diff, rules, in_table));
    names.extend(cite_names(diff, rules, lookup));
    if !rules.tip_check && !diff.added.iter().any(|line| asks(&line.text, rules.prefix)) {
        names.insert("ruling-check-off".to_owned());
    }
    names
}

/// 行が台帳の接頭辞の問い id の形を 1 つでも持つか（解けるかは見ない）。
fn asks(text: &str, prefix: Option<&str>) -> bool {
    scan(text, prefix).iter().any(|cite| cite.form == Form::Question)
}

/// 問い id の形を持たない判断の欄（3 つの閉じた字面）。
fn field_names(diff: &Diff, rules: &Rules<'_>, in_table: &dyn Fn(&str, u64) -> bool) -> BTreeSet<String> {
    let judging = diff.added.iter().filter_map(|line| Some((field_of(line, in_table)?, line)));
    judging.filter(|(_, line)| !asks(&line.text, rules.prefix)).map(|(field, line)| format!("field:{field}@{}", line.path)).collect()
}

/// 足した行が判断の欄か（欄の語を返す）。
fn field_of(line: &Added, in_table: &dyn Fn(&str, u64) -> bool) -> Option<&'static str> {
    let (path, text) = (line.path.as_str(), line.text.as_str());
    if path.ends_with(".toml") && is_ruling_assignment(text) {
        return Some("rules");
    }
    if path.starts_with(ADR_DIR) && path.ends_with(".html") && text.contains(ROLE) {
        return Some("adr");
    }
    let design = path.starts_with(DESIGN_DIR) && path.ends_with(".md") && is_design_judgement(text) && !in_table(path, line.number);
    design.then_some("design")
}

/// toml の行で、剥がした字が `ruling` と `=` で始まるか（`ruling-check = true` は `-` が挟まるので当たらない）。
fn is_ruling_assignment(text: &str) -> bool {
    text.trim().strip_prefix("ruling").is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// 設計 doc の契約表の外の行が判断の欄か: 見出しの行は `裁定` + 空白か括弧を持つ、ほかの行はリストの印と太字を剥がした字が
/// `裁定` + `:`・`：`・`=`・括弧・` id` で始まる。
fn is_design_judgement(text: &str) -> bool {
    let trimmed = text.trim_start();
    if trimmed.starts_with('#') {
        return trimmed.match_indices(WORD).any(|(at, _)| {
            let next = trimmed.get(at.saturating_add(WORD.len())..).and_then(|rest| rest.chars().next());
            next.is_some_and(|found| found.is_whitespace() || matches!(found, '(' | '（'))
        });
    }
    let bare = ["- ", "* ", "+ "].iter().find_map(|mark| trimmed.strip_prefix(mark)).unwrap_or(trimmed).replace("**", "");
    let after = bare.trim_start().strip_prefix(WORD);
    after.is_some_and(|rest| [":", "：", "=", "(", "（", " id"].iter().any(|lead| rest.starts_with(lead)))
}

/// 解けない引用の名指し（fixtures を外した引用のうち、台帳にも線の前にも解けないもの）。台帳か線を読めない周は測れなかった名指し 1 つ。
fn cite_names(diff: &Diff, rules: &Rules<'_>, lookup: Ask<'_>) -> BTreeSet<String> {
    let scanned = diff.added.iter().flat_map(|line| scan(&line.text, rules.prefix).into_iter().map(move |cite| (line.path.as_str(), cite)));
    let cites: Vec<(&str, Cite)> = scanned.filter(|(_, cite)| !is_fixture(cite, rules.fixtures, false)).collect();
    if cites.is_empty() {
        return BTreeSet::new();
    }
    let times = cites.iter().filter(|(_, cite)| cite.form == Form::Time).map(|(path, cite)| ((*path).to_owned(), cite.text.clone())).collect();
    let found = match lookup(&times) {
        Ok(found) => found,
        Err(word) => return BTreeSet::from([unmeasured(word)]),
    };
    let unresolved = cites.iter().filter(|(path, cite)| !found.resolves(path, cite));
    unresolved.map(|(path, cite)| name_of(path, cite)).collect()
}

/// 解けない引用の名指し（時刻の形は `time:<時刻>@<path>`・ほかは字面）。
fn name_of(path: &str, cite: &Cite) -> String {
    match cite.form {
        Form::Time => format!("time:{}@{path}", cite.text.strip_prefix("user ").unwrap_or(&cite.text)),
        Form::Question | Form::Batch | Form::Policy => cite.text.clone(),
    }
}

/// 差分の足した 1 行。
struct Added {
    /// 足した file の path（escape を戻した字・`b/` を外した字）。
    path: String,
    /// 先端の file の行番号（1 始まり）。
    number: u64,
    /// 行の字（先頭の `+` を外した字）。
    text: String,
}

/// 差分の足した行の全部と、path を読めない file に足した行が在ったか。
struct Diff {
    /// path を読めた file の足した行。
    added: Vec<Added>,
    /// header の path を読めない file に足した行が在る（読み飛ばして通さない）。
    unreadable: bool,
}

/// file ごとの header の path の読み。
enum Head {
    /// まだ `+++ ` の header を読んでいない。
    Unseen,
    /// header の path を読めない（閉じない引用符・戻せない escape・`b/` の無い字）。
    Unreadable,
    /// 消えた file（`/dev/null`）。
    Gone,
    /// 読めた path。
    Known(String),
}

impl Head {
    /// `+++ ` の後ろの字から path を読む。引用符で囲まれた形は escape を戻してから `b/` を外す。
    fn of(rest: &str) -> Self {
        if rest == "/dev/null" {
            return Self::Gone;
        }
        let raw = match rest.starts_with('"') {
            true => unquote(rest),
            false => Some(rest.strip_suffix('\t').unwrap_or(rest).to_owned()),
        };
        match raw.as_deref().and_then(|path| path.strip_prefix("b/")) {
            Some(path) if !path.is_empty() => Self::Known(path.to_owned()),
            _ => Self::Unreadable,
        }
    }
}

impl Diff {
    /// `--unified=0` の出力を読む。header の区間と hunk の区間を分けるので、足した行が `++` で始まっても header と読まない。
    fn read(text: &str) -> Self {
        let mut diff = Self { added: Vec::new(), unreadable: false };
        let (mut head, mut in_hunk, mut number) = (Head::Unseen, false, 0_u64);
        for line in text.lines() {
            if line.starts_with("diff --git ") {
                (head, in_hunk) = (Head::Unseen, false);
            } else if line.starts_with("@@ ") {
                (in_hunk, number) = (true, new_start(line));
            } else if !in_hunk {
                if let Some(rest) = line.strip_prefix("+++ ") {
                    head = Head::of(rest);
                }
            } else if let Some(added) = line.strip_prefix('+') {
                diff.push(&head, number, added);
                number = number.saturating_add(1);
            }
        }
        diff
    }

    /// 足した 1 行を file の読みに従って積む。
    fn push(&mut self, head: &Head, number: u64, text: &str) {
        match head {
            Head::Known(path) => self.added.push(Added { path: path.clone(), number, text: text.to_owned() }),
            Head::Gone => {}
            Head::Unseen | Head::Unreadable => self.unreadable = true,
        }
    }
}

/// hunk header（`@@ -a,b +c,d @@`）の新しい側の始まりの行（読めなければ 0）。
fn new_start(line: &str) -> u64 {
    let range = line.split_whitespace().find_map(|word| word.strip_prefix('+'));
    range.and_then(|found| found.split(',').next()).and_then(|first| first.parse().ok()).unwrap_or(0)
}

/// 引用符で囲まれた header の path の escape を戻す（閉じの引用符が字の最後に 1 つだけ在る形・戻せない escape と UTF-8 でない path は `None`）。
fn unquote(quoted: &str) -> Option<String> {
    let bytes = quoted.strip_prefix('"')?.as_bytes();
    let (mut out, mut at) = (Vec::new(), 0_usize);
    while let Some(&byte) = bytes.get(at) {
        at = at.saturating_add(1);
        match byte {
            b'"' => return (at == bytes.len()).then(|| String::from_utf8(out).ok()).flatten(),
            b'\\' => {
                let (value, used) = escape(bytes.get(at..)?)?;
                out.push(value);
                at = at.saturating_add(used);
            }
            _ => out.push(byte),
        }
    }
    None
}

/// `\` の後ろの escape 1 つ（戻した byte と使った長さ）。git が出す形: `\"`・`\\`・`\t`・`\n` ほかの制御文字の字・8 進の 3 桁。
fn escape(rest: &[u8]) -> Option<(u8, usize)> {
    let simple = match *rest.first()? {
        b'"' => b'"',
        b'\\' => b'\\',
        b'a' => 7,
        b'b' => 8,
        b't' => 9,
        b'n' => 10,
        b'v' => 11,
        b'f' => 12,
        b'r' => 13,
        b'0'..=b'3' => return octal(rest.get(..3)?).map(|value| (value, 3)),
        _ => return None,
    };
    Some((simple, 1))
}

/// 8 進の 3 桁（先頭は 0〜3）を byte に戻す。
fn octal(digits: &[u8]) -> Option<u8> {
    let value = digits.iter().try_fold(0_u16, |sum, digit| {
        (b'0'..=b'7').contains(digit).then(|| sum.saturating_mul(8).saturating_add(u16::from(digit.saturating_sub(b'0'))))
    })?;
    u8::try_from(value).ok()
}

/// 設計 doc の契約表の区間（先端の file ごとに 1 回だけ読む）。
struct Tables<'a> {
    /// 便の worktree。
    worktree: &'a Path,
    /// path ごとの区間（始まりの行・終わりの行）。
    seen: RefCell<BTreeMap<String, Vec<(u64, u64)>>>,
}

impl Tables<'_> {
    /// 先端の file の `line` 行目が契約表の区間の中か（読めない file は区間なし＝外の行として数える側）。
    fn contains(&self, path: &str, line: u64) -> bool {
        let mut seen = self.seen.borrow_mut();
        let spans = seen.entry(path.to_owned()).or_insert_with(|| {
            git_bytes(self.worktree, &["show", &format!("{TIP}:{path}")]).map(|bytes| regions(&String::from_utf8_lossy(&bytes))).unwrap_or_default()
        });
        spans.iter().any(|(start, end)| *start <= line && line <= *end)
    }
}

/// 契約表の区間（`<!-- contracts:begin -->` … `<!-- contracts:end -->` の行番号の組・閉じない区間は数えない）。
fn regions(text: &str) -> Vec<(u64, u64)> {
    let (mut found, mut open) = (Vec::new(), None);
    for (index, line) in text.lines().enumerate() {
        let at = u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1);
        match line.trim() {
            BEGIN => open = Some(at),
            END => found.extend(open.take().map(|start| (start, at))),
            _ => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{names_of, regions, Diff, Lookup, Rules};
    use crate::ledger::citation::{Index, Notes};

    /// 台帳の接頭辞。
    const PREFIX: &str = "s2";

    /// 解ける問い id（閉じた問いの裁定の行）。
    const RESOLVED: &str = "s2-q.1:20260930T0000Z-1";

    /// 解けない問い id。
    const UNRESOLVED: &str = "s2-q.9:20260930T0000Z-1";

    /// 接頭辞違いの問い id の形。
    const OTHER_PREFIX: &str = "tz-1:20260930T0000Z-1";

    /// 台帳の裁定の行を 1 つだけ持つ材料（線の前の時刻の形は `before` の組）。
    fn lookup_with(before: &[(&str, &str)]) -> Lookup {
        let row = format!("{RESOLVED} | s2-q.1 | 2026-09-30T00:00Z | chat | 逐語");
        let notes = Notes { closed_questions: vec![row.clone()], every: vec![row] };
        Lookup { index: Index::new(&notes, Some(PREFIX)), before: before.iter().map(|(path, text)| ((*path).to_owned(), (*text).to_owned())).collect() }
    }

    /// 1 file の差分（`+++ ` の header の字と足した行。行番号は 1 から）。
    fn diff_of(header: &str, added: &[&str]) -> String {
        let body: String = added.iter().map(|line| format!("+{line}\n")).collect();
        format!("diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ {header}\n@@ -0,0 +1,{} @@\n{body}", added.len())
    }

    /// 名指しの列を導く（設計 doc の表の区間は `table` の行だけ・台帳は [`lookup_with`]・ready の列は `before`）。
    fn names(text: &str, tip_check: bool, table: &[u64], before: &[(&str, &str)]) -> Vec<String> {
        let rules = Rules { prefix: Some(PREFIX), fixtures: &["batch:fix".to_owned()], tip_check };
        let in_table = |_: &str, line: u64| table.contains(&line);
        names_of(&Diff::read(text), &rules, &in_table, &mut |_| Ok(lookup_with(before))).into_iter().collect()
    }

    /// 足した行 1 本（file の頭の header は `b/<path>`）だけの名指し。
    fn names_of_line(path: &str, line: &str) -> Vec<String> {
        names(&diff_of(&format!("b/{path}"), &[line]), true, &[], &[])
    }

    /// 判断の欄 3 字面 × 3 引用: bead の id だけ・接頭辞違いだけの行は欄の名指し、台帳の接頭辞の問い id の形の行（解けなくても）は名指さない。
    #[test]
    fn hold_diff_judgement_fields_need_a_question_id_shape() {
        let fields: [(&str, String, &str); 3] = [
            ("rules/t.toml", "ruling = \"{}\"".to_owned(), "field:rules@rules/t.toml"),
            ("design-intent/decisions/ADR-1.html", "<td class=\"role\">裁定</td><td>{}</td>".to_owned(), "field:adr@design-intent/decisions/ADR-1.html"),
            ("docs/design/a.md", "- 裁定: {}".to_owned(), "field:design@docs/design/a.md"),
        ];
        let mut checked = 0;
        for (path, template, named) in fields {
            for (cite, held) in [("s2-07l.738", true), (OTHER_PREFIX, true), (UNRESOLVED, false), (RESOLVED, false)] {
                let found = names_of_line(path, &template.replace("{}", cite));
                let fielded = found.iter().any(|name| name == named);
                assert_eq!(fielded, held, "{path} / {cite}: {found:?}");
                checked += 1;
            }
        }
        assert_eq!(checked, 12, "3 字面 × 4 引用を全部確かめた");
    }

    /// 欄の字面は閉じている: `ruling-check = true` の行・別の file の `ruling =`・契約表の区間の中の `裁定:` の行は欄でない。
    #[test]
    fn hold_diff_field_shapes_are_closed() {
        assert!(names_of_line("rules/t.toml", "ruling-check = true").is_empty());
        assert!(names_of_line("rules/t.toml", "rulings = [\"x\"]").is_empty());
        assert!(names_of_line("docs/design/a.md", "ruling = \"x\"").is_empty(), "toml でない file の ruling = は欄でない");
        assert!(names_of_line("crates/a.rs", "- 裁定: x").is_empty(), "設計 doc でない file は欄でない");
        let table = names(&diff_of("b/docs/design/a.md", &["- 裁定: x"]), true, &[1], &[]);
        assert!(table.is_empty(), "契約表の区間の中の行は欄でない: {table:?}");
        let outside = names(&diff_of("b/docs/design/a.md", &["- 裁定: x"]), true, &[2], &[]);
        assert_eq!(outside, ["field:design@docs/design/a.md"], "区間の外なら欄");
    }

    /// 設計 doc の欄の 6 字面（見出しの空白・括弧・リストの印と太字・括弧・` id`・行頭）と欄でない字面。
    #[test]
    fn hold_diff_design_field_shapes() {
        for yes in ["## 裁定 x", "### 4. 裁定（要件）", "- 裁定: x", "* **裁定**：x", "  + 裁定=x", "- 裁定(user x)", "- 裁定 id x", "裁定: x"] {
            assert_eq!(names_of_line("docs/design/a.md", yes), ["field:design@docs/design/a.md"], "欄: {yes}");
        }
        for no in ["## 裁定", "## 裁定について", "裁定は x", "- 裁定者: x", "本文の裁定: x", "- 先に裁定: x"] {
            assert!(names_of_line("docs/design/a.md", no).is_empty(), "欄でない: {no}");
        }
    }

    /// 差分が足す 3 形: 解けない問い id・解けない batch: と policy:・線の後の時刻の形は名指す。fixtures・解ける問い id・線の前の時刻の形は名指さない。
    #[test]
    fn hold_diff_unresolved_cites_are_named_and_resolvable_ones_are_not() {
        let text = diff_of(
            "b/src/lib.rs",
            &[
                &format!("// {UNRESOLVED} と batch:zz と policy:pp と user 2026-09-30T05:00Z"),
                &format!("// {RESOLVED} と batch:fix と user 2026-09-29T12:00Z"),
            ],
        );
        let found = names(&text, true, &[], &[("src/lib.rs", "user 2026-09-29T12:00Z")]);
        let want = ["batch:zz", "policy:pp", UNRESOLVED, "time:2026-09-30T05:00Z@src/lib.rs"];
        assert_eq!(found, want, "並べ替えて重複を除いた名指し");
        let again = names(&format!("{text}{}", diff_of("b/src/other.rs", &["// batch:zz"])), true, &[], &[]);
        assert_eq!(again.iter().filter(|name| *name == "batch:zz").count(), 1, "別の file の同じ字面も 1 件: {again:?}");
        assert!(names(&diff_of("b/src/lib.rs", &["// user 2026-09-29T12:00Z"]), true, &[], &[]).iter().any(|name| name.starts_with("time:")), "線の前の組に無ければ線の後");
    }

    /// `ruling-check` の外し: 先端が false か無いのに、足した行に問い id の形が 1 つも無ければ名指す（解けなくても形が在れば外しでは名指さない）。
    #[test]
    fn hold_diff_turning_the_check_off_needs_a_question_id_shape() {
        let off = diff_of("b/.vessel.toml", &["ruling-check = false"]);
        assert_eq!(names(&off, false, &[], &[]), ["ruling-check-off"]);
        assert!(names(&off, true, &[], &[]).is_empty(), "先端が true なら外しでない");
        let cited = diff_of("b/.vessel.toml", &["ruling-check = false", &format!("# {RESOLVED}")]);
        assert!(names(&cited, false, &[], &[]).is_empty(), "問い id の形を足せば外しは通る");
        let other = diff_of("b/.vessel.toml", &["ruling-check = false", &format!("# {OTHER_PREFIX}")]);
        assert_eq!(names(&other, false, &[], &[]), ["ruling-check-off"], "接頭辞違いの形は数えない");
        assert_eq!(names("", false, &[], &[]), ["ruling-check-off"], "足した行が無い外しも名指す");
    }

    /// 引用符で囲まれた header（約束 8）: escape を戻した path に足した解けない引用が名指しに入り、名指しの path は戻した字。
    #[test]
    fn hold_diff_quoted_headers_are_read_with_their_escapes_undone() {
        for (header, path) in [("\"b/docs/design/a\\\"b.md\"", "docs/design/a\"b.md"), ("\"b/docs/design/\\346\\227\\245.md\"", "docs/design/日.md"), ("\"b/docs/design/t\\tn\\\\.md\"", "docs/design/t\tn\\.md")] {
            let text = diff_of(header, &[&format!("- 裁定: {OTHER_PREFIX} x"), "- 裁定: y", &format!("{UNRESOLVED} user 2026-09-30T05:00Z")]);
            let found = names(&text, true, &[], &[]);
            let want = [format!("field:design@{path}"), UNRESOLVED.to_owned(), format!("time:2026-09-30T05:00Z@{path}")];
            assert_eq!(found, want, "{header}");
        }
    }

    /// path を読めない header の file に足した行が在る周は `unmeasured:diff-path`（読み飛ばして通さない）。足した行の無い file は留めない。
    #[test]
    fn hold_diff_unreadable_header_path_is_unmeasured() {
        for header in ["\"b/docs/a.md", "\"b/docs/a\\.md\"", "\"b/docs/a\\9.md\"", "\"b/docs/a\\34.md\"", "\"b/docs/a\"b.md\"", "a/docs/a.md", "\"x/docs/a.md\"", "\"b/\\377.md\""] {
            let text = diff_of(header, &[&format!("// {UNRESOLVED}")]);
            assert_eq!(names(&text, true, &[], &[]), ["unmeasured:diff-path"], "{header}: 読めない path の足した行は読み飛ばさない");
        }
        assert_eq!(names(&diff_of("\"b/docs/a.md", &[]), true, &[], &[]), Vec::<String>::new(), "足した行が無い file は留めない");
        assert_eq!(names(&diff_of("/dev/null", &[]), true, &[], &[]), Vec::<String>::new(), "消えた file は留めない");
    }

    /// header と足した行の区別: `++` で始まる足した行（`+++ ` に見える）は header と読まず、file の切れ目は `diff --git ` で来る。
    #[test]
    fn hold_diff_added_lines_that_look_like_headers_stay_added_lines() {
        let text = format!("{}{}", diff_of("b/src/lib.rs", &["++ // batch:aa", "x"]), diff_of("b/src/next.rs", &["// batch:bb"]));
        let diff = Diff::read(&text);
        let seen: Vec<(String, u64, String)> = diff.added.iter().map(|line| (line.path.clone(), line.number, line.text.clone())).collect();
        let want: [(&str, u64, &str); 3] = [("src/lib.rs", 1, "++ // batch:aa"), ("src/lib.rs", 2, "x"), ("src/next.rs", 1, "// batch:bb")];
        assert_eq!(seen, want.map(|(path, number, line)| (path.to_owned(), number, line.to_owned())), "三つの行は元の file の元の行番号");
        assert!(!diff.unreadable);
    }

    /// 台帳を読めない周は、引用を持つ時だけ測れなかった名指しで留める（引用の無い差分は台帳を読まない）。
    #[test]
    fn hold_diff_ledger_unreadable_is_named_only_when_a_cite_needs_it() {
        let rules = Rules { prefix: Some(PREFIX), fixtures: &[], tip_check: true };
        let none = |_: &str, _: u64| false;
        let mut reads = 0;
        let quiet = names_of(&Diff::read(&diff_of("b/src/lib.rs", &["// 何もない"])), &rules, &none, &mut |_| {
            reads += 1;
            Err("ledger")
        });
        assert!(quiet.is_empty() && reads == 0, "引用の無い差分は台帳を読まない: {quiet:?} / {reads}");
        let cited = names_of(&Diff::read(&diff_of("b/src/lib.rs", &["// batch:zz"])), &rules, &none, &mut |_| Err("ledger"));
        assert_eq!(cited.into_iter().collect::<Vec<_>>(), ["unmeasured:ledger"]);
    }

    /// 契約表の区間は begin と end の行（閉じない区間は数えない）。
    #[test]
    fn hold_diff_table_regions_need_both_markers() {
        let text = "a\n<!-- contracts:begin -->\nb\n<!-- contracts:end -->\nc\n<!-- contracts:begin -->\nd\n";
        assert_eq!(regions(text), [(2, 4)]);
    }
}
