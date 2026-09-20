//! 契約表の検査の本体と要件面の読みと CLI の駆動（設計 docs/design/contract-source.md §2 / §3 / §15・SRS FR47 /
//! FR55）。
//!
//! 表の全行を [`check_table`] が**全件・行番号付き**で検査し（id の一意・`req` の要件面での実在・`section` の節の
//! 実在・verify の形・`depends` の解決と輪・`touches` の閉包と `surfaces` の外形 pin ⊆ `write-set`・write-set の
//! 項目の実在・名指しの実在）、要件面の id は [`requirement_ids`] が拡張子ごとの読み手で取る。`contracts check` の
//! 駆動（tracked file の一覧・doc の読み・判定行）は [`check_repo`]。findings の語彙（[`super::TableError`] /
//! [`super::Finding`] / [`super::Context`]）は親 module `table.rs`・置き場の抜き出しと parse は兄弟 `table/parse.rs`
//! に置いたまま。呼び手（`pipe/cli.rs`・`pipe/cli/intake.rs`・歯）の `use` は親の再 export を通る。

use super::super::closure::{closure, surface_closure, unresolved_names, ClosureError, Source};
use super::super::declaration::{self, read_write_set, Basis, Ceiling, NewFilePolicy};
use super::super::refuse::{covered, Refuse};
use super::{read_rows, unreadable, Context, ContractRow, Finding, TableError, BEGIN, DESIGN_DIR, END};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK};
use std::collections::BTreeSet;
use std::path::Path;

/// 表の全行を検査する（**全件・行番号の順**・同じ行の中は検査の順）。intake（契約 (b)）は同じ関数を 1 行に撃つ。
///
/// `ids` は `depends` の解決の母集団＝**同じ doc の全行の id**（§30・行 ad）。`contracts check` は `rows` と同じ
/// 全行の id を渡し、intake は検査する行を 1 つ（`rows`）のまま母集団だけを doc の全行から渡す（1 行の slice の id
/// だけを母集団に読むと、相手が別の行に在る `depends` が常に解けない）。id の一意は `rows` の中で測る。
pub fn check_table(doc: &str, rows: &[ContractRow], ids: &[&str], ctx: &Context<'_>) -> Vec<Finding> {
    let numbered = sections(doc);
    let mut found = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if rows.iter().take(index).any(|seen| seen.id == row.id) {
            found.push(Finding::table(TableError::DuplicateId { line: row.line, id: row.id.clone() }));
        }
        if !numbered.iter().any(|(number, filled)| *filled && number.as_deref() == Some(row.section.as_str())) {
            found.push(Finding::table(TableError::SectionMissing { line: row.line, section: row.section.clone() }));
        }
        found.extend(requirement_findings(row, ctx.requirements));
        found.extend(verify_findings(row, &Basis { allowed: ctx.allowed, denied: ctx.denied }));
        let unresolved = row.depends.iter().filter(|id| !ids.contains(&id.as_str()));
        found.extend(unresolved.map(|id| Finding::table(TableError::DependsUnresolved { line: row.line, id: id.clone() })));
        found.extend(write_set_findings(row, ctx));
        // 閉包・外形 pin・名指しは `.rs` / `.snap` の本文を読む。1 本でも読めなければ行ごとに 1 件で名指し、
        // 測れない検査は撃たない（読めなさを「足りない file なし」に読み替えない・NFR4）。
        match unreadable_input(ctx) {
            Some(reason) => found.push(Finding::table(unreadable(row.line, &reason))),
            None => {
                found.extend(closure_findings(row, ctx));
                found.extend(name_findings(doc, row, ctx));
            }
        }
    }
    found.extend(cycle_findings(rows));
    found.sort_by_key(|finding| finding.line);
    found
}

/// 節 `number` の本文の (doc 上の行番号, 行)（見出しの次の行から次の `## ` 見出しの前まで・区間と fence の中は除く）。
fn section_lines(text: &str, number: &str) -> Vec<(u64, String)> {
    let mut found = Vec::new();
    let (mut fenced, mut inside, mut open) = (false, false, false);
    for (index, line) in text.lines().enumerate() {
        match line.trim() {
            BEGIN => inside = true,
            END => inside = false,
            _ if inside => {}
            trimmed => {
                if trimmed.starts_with("```") {
                    fenced = !fenced;
                    continue;
                }
                match line.strip_prefix("## ").filter(|_| !fenced) {
                    Some(title) => open = number_of(title).as_deref() == Some(number),
                    None if open && !fenced => found.push(((index as u64).saturating_add(1), line.to_owned())),
                    None => {}
                }
            }
        }
    }
    found
}

/// 名指しの実在（§3）: `title` / `done` と `section` の本文の backtick の中身のうち解けないものを全件（在り処付き）。
/// 閉包の入力を読めない周は `unreadable`（黙って通さない）。新規 file は write-set の `+` 項目と `creates` の欄
/// （導出の形・`+` 無しで書く）の両方から解く。
fn name_findings(doc: &str, row: &ContractRow, ctx: &Context<'_>) -> Vec<Finding> {
    let mut texts = vec![("title".to_owned(), row.title.clone()), ("done".to_owned(), row.done.clone())];
    texts.extend(
        section_lines(doc, &row.section).into_iter().map(|(at, line)| (format!("section {} line {at}", row.section), line)),
    );
    let mut new_files = row.write_set.clone();
    new_files.extend(row.creates.iter().map(|item| format!("{}{item}", super::super::refuse::NEW_FILE)));
    match unresolved_names(&texts, &row.touches, &new_files, ctx.tracked, ctx.sources) {
        Err(error) => vec![Finding::table(unreadable(row.line, &error.reason()))],
        Ok(names) => names
            .into_iter()
            .map(|(name, at)| Finding { line: row.line, refuse: Refuse::NameUnresolved { name, at } })
            .collect(),
    }
}

/// doc の `## ` 見出しの節番号（`## N.` の N・番号の無い見出しは `None`）と、本文が非空か。区間と code fence の
/// 中の行は見出しに数えず、区間の行は本文にも数えない。
fn sections(text: &str) -> Vec<(Option<String>, bool)> {
    let mut found: Vec<(Option<String>, bool)> = Vec::new();
    let (mut fenced, mut inside) = (false, false);
    for line in text.lines() {
        match line.trim() {
            BEGIN => inside = true,
            END => inside = false,
            _ if inside => {}
            trimmed => {
                if trimmed.starts_with("```") {
                    fenced = !fenced;
                }
                match line.strip_prefix("## ").filter(|_| !fenced) {
                    Some(title) => found.push((number_of(title), false)),
                    None => {
                        if let Some(last) = found.last_mut() {
                            last.1 |= !trimmed.is_empty();
                        }
                    }
                }
            }
        }
    }
    found
}

/// 見出しの字面の先頭の `N.` の N（数字だけ）。
fn number_of(title: &str) -> Option<String> {
    let (head, _) = title.split_once('.')?;
    (!head.is_empty() && head.chars().all(|found| found.is_ascii_digit())).then(|| head.to_owned())
}

/// `req` の各 id が要件面に在るか（要件面を読めない周は行ごとに 1 件・黙って通さない）。
fn requirement_findings(row: &ContractRow, requirements: &Result<BTreeSet<String>, String>) -> Vec<Finding> {
    match *requirements {
        Err(ref reason) => vec![Finding::table(unreadable(row.line, reason))],
        Ok(ref known) => row
            .req
            .iter()
            .filter(|req| !known.contains(*req))
            .map(|req| Finding::table(TableError::RequirementMissing { line: row.line, req: req.clone() }))
            .collect(),
    }
}

/// verify 行の形（判定は宣言の 1 本 [`declaration::verify_unfit`]・契約の行は穴を持てず、禁じる語列にも当たれない）。
fn verify_findings(row: &ContractRow, basis: &Basis<'_>) -> Vec<Finding> {
    row.verify
        .iter()
        .filter_map(|line| {
            let reason = declaration::verify_unfit(line, basis)?;
            Some(Finding::table(TableError::VerifyForm { line: row.line, verify: line.clone(), reason }))
        })
        .collect()
}

/// write-set の項目の 2 検査（base の tracked file だけで測る・§3「項目の実在と展開」）: 末尾 `/` 無しで tracked な
/// dir を指す項目と、base に解けない項目（末尾 `/` 無しの dir として既に名指した項目は重ねて名指さない）。契約表の
/// 行は履歴を持つので、`+` の項目が tracked に在れば land 済みの実在 file と読む（[`NewFilePolicy::MayBeLanded`]・
/// intake は在れば断る・`s2-07l.346`）。
fn write_set_findings(row: &ContractRow, ctx: &Context<'_>) -> Vec<Finding> {
    let without_slash: Vec<&String> = row
        .write_set
        .iter()
        .filter(|item| !item.ends_with('/') && ctx.tracked.iter().any(|path| declaration::is_under(path, item)))
        .collect();
    let mut found: Vec<Finding> = without_slash
        .iter()
        .map(|item| Finding { line: row.line, refuse: Refuse::WriteSetDirWithoutSlash { path: (*item).clone() } })
        .collect();
    if let Err(items) = read_write_set(&row.write_set, ctx.tracked, NewFilePolicy::MayBeLanded) {
        found.extend(
            items
                .into_iter()
                .filter(|item| !without_slash.contains(&item))
                .map(|item| Finding { line: row.line, refuse: Refuse::WriteSetItemUnresolved { item } }),
        );
    }
    found
}

/// 閉包の入力（`.rs` と `.snap`）のうち読めない 1 本の理由（全部読めれば `None`・行ごとに 1 件で名指す材料）。
fn unreadable_input(ctx: &Context<'_>) -> Option<String> {
    ctx.sources.iter().chain(ctx.snapshots).find_map(|source| match source.body {
        Ok(_) => None,
        Err(ref reason) => {
            Some(ClosureError::Unreadable { path: source.path.clone(), reason: reason.clone() }.reason())
        }
    })
}

/// `touches` の閉包と `surfaces` の外形 pin（§3 の 4 形 + 第 5 形）のうち write-set に無い file を 1 件に全部。
/// 未知の外形の名は [`TableError::SurfaceUnknown`]。読めない入力は呼び手が先に除く。**write-set の無い行**（§3
/// 「write-set の導出」の形・受付が導出値を write-set にする）は閉包 ⊆ write-set を持たず、型と外形の名の形だけを
/// 見る（CI は drift も撃たない＝表は履歴を持つ）。
fn closure_findings(row: &ContractRow, ctx: &Context<'_>) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut files = BTreeSet::new();
    match closure(&row.touches, ctx.sources) {
        Ok(paths) => files.extend(paths),
        Err(error) => found.push(Finding::table(unreadable(row.line, &error.reason()))),
    }
    match surface_closure(&row.surfaces, ctx.sources, ctx.snapshots) {
        Ok(paths) => files.extend(paths),
        Err(ClosureError::SurfaceUnknown { name }) => {
            found.push(Finding::table(TableError::SurfaceUnknown { line: row.line, name }));
        }
        Err(error) => found.push(Finding::table(unreadable(row.line, &error.reason()))),
    }
    let missing: Vec<String> = files.into_iter().filter(|path| !covered(&row.write_set, path)).collect();
    if !missing.is_empty() && !row.write_set.is_empty() {
        found.push(Finding { line: row.line, refuse: Refuse::WriteSetIncomplete { missing } });
    }
    found
}

/// `depends` の輪（輪 1 つにつき 1 件・輪の中で doc 順が最初の行に置く）。解けない id は辿らない（別の 1 件）。
fn cycle_findings(rows: &[ContractRow]) -> Vec<Finding> {
    let reach: Vec<BTreeSet<&str>> = rows.iter().map(|row| reachable(rows, row)).collect();
    let mut named: BTreeSet<&str> = BTreeSet::new();
    let mut found = Vec::new();
    for (row, from) in rows.iter().zip(&reach) {
        if named.contains(row.id.as_str()) || !from.contains(row.id.as_str()) {
            continue;
        }
        let members: Vec<&str> = rows
            .iter()
            .zip(&reach)
            .filter(|(other, back)| from.contains(other.id.as_str()) && back.contains(row.id.as_str()))
            .map(|(other, _)| other.id.as_str())
            .collect();
        named.extend(members.iter().copied());
        let cycle = members.iter().map(|id| (*id).to_owned()).collect();
        found.push(Finding::table(TableError::DependsCycle { line: row.line, cycle }));
    }
    found
}

/// `start` から `depends` を 1 本以上辿って届く id の集合。
fn reachable<'r>(rows: &'r [ContractRow], start: &'r ContractRow) -> BTreeSet<&'r str> {
    let mut found: BTreeSet<&str> = BTreeSet::new();
    let mut stack: Vec<&str> = start.depends.iter().map(String::as_str).collect();
    while let Some(id) = stack.pop() {
        if !found.insert(id) {
            continue;
        }
        if let Some(next) = rows.iter().find(|row| row.id == id) {
            stack.extend(next.depends.iter().map(String::as_str));
        }
    }
    found
}

/// 要件面の id の集合（`.html` = `id="…"` の anchor・`.yaml` / `.yml` = 要件 id の列・`.md` = 要件 id で始まる
/// 見出し）。他の拡張子は読まない。形の弁別は拡張子の 1 match（C2）で、審査の段の本文の読み手（`pipe/review.rs` の
/// `requirements_text`）も同じ match で形の関数を選ぶ（設計 contract-source.md §4・`s2-07l.354`）。
pub fn requirement_ids(path: &str, text: &str) -> Result<BTreeSet<String>, String> {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("html") => Ok(anchors(text)),
        Some("yaml" | "yml") => Ok(text.lines().filter_map(yaml_id).collect()),
        Some("md") => Ok(text.lines().filter_map(md_id).collect()),
        _ => Err(format!("要件面 {path} の形を読めない（.html の anchor / .yaml の列 / .md の見出しだけ）")),
    }
}

/// `id="…"` / `id='…'` の値のうち要件 id の形のもの。
fn anchors(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find("id=") {
        let tail = rest.get(at.saturating_add(3)..).unwrap_or_default();
        let value = ['"', '\''].iter().find_map(|quote| {
            let body = tail.strip_prefix(*quote)?;
            body.get(..body.find(*quote)?)
        });
        if let Some(id) = value.filter(|id| is_requirement(id)) {
            found.insert(id.to_owned());
        }
        rest = tail;
    }
    found
}

/// yaml の 1 行（`- FR1` / `- "FR1"` / `id: FR1` / `- id: FR1`）から要件 id を取る。
fn yaml_id(line: &str) -> Option<String> {
    let item = line.trim();
    let item = item.strip_prefix('-').map_or(item, str::trim_start);
    let item = item.strip_prefix("id:").map_or(item, str::trim_start);
    let item = item.trim().trim_matches(|found: char| found == '"' || found == '\'');
    is_requirement(item).then(|| item.to_owned())
}

/// md の 1 行が行頭 `#` の見出しで、その先頭 token が要件 id の形なら id（`## FR1 便の起動` → `FR1`・`## FR1` も
/// 同じ）。見出しでない行・先頭 token が id の形でない見出しは `None`。見出しの読み（行頭の `#` の列 + 空白）は審査の
/// 段の本文の読み手（`pipe/review.rs` の `requirement_md`）と同じ形。
fn md_id(line: &str) -> Option<String> {
    let rest = line.strip_prefix('#')?.trim_start_matches('#');
    let title = rest.strip_prefix([' ', '\t'])?;
    title.split_whitespace().next().filter(|head| is_requirement(head)).map(str::to_owned)
}

/// 要件 id の形（英大文字の列 + 数字の列・`FR47` / `NFR4` / `AC21`）。
fn is_requirement(text: &str) -> bool {
    let letters = text.trim_end_matches(|found: char| found.is_ascii_digit());
    !letters.is_empty() && letters.len() < text.len() && letters.chars().all(|found| found.is_ascii_uppercase())
}

/// `<NAME> contracts check --repo R` の本体（設計 §2「表の検査」・FR55）: tracked な `docs/design/*.md` の区間を
/// 全行検査し、findings を `contracts: <file>:<line> …` の 1 行ずつ、末尾に判定行を stdout へ出す。
///
/// rc = 違反 0 → 0 / 違反 ≥ 1 → 1 / 読めない周 → 2（読めない doc・区間・要件面・閉包の入力も 1 件として名指し、
/// 判定行も出す）。tracked file の一覧か宣言を読めない周は判定できないので、理由だけを stderr へ出して rc 2。
pub(crate) fn check_repo(repo: &Path, ceiling: &Ceiling<'_>) -> Outcome {
    let Some(tracked) = tracked_files(repo) else {
        let reason = format!("contracts: {} の tracked file を読めない（git repo でない）", repo.display());
        return Outcome::failed_line(RC_BROKEN, reason);
    };
    let facts = match declaration::table_facts(repo, ceiling) {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect()),
    };
    let sources = read_all(repo, &tracked, ".rs");
    let snapshots = read_all(repo, &tracked, ".snap");
    let requirements = read(repo, &facts.requirements).and_then(|text| requirement_ids(&facts.requirements, &text));
    let ctx = Context {
        allowed: &facts.allowed,
        denied: &facts.denied,
        requirements: &requirements,
        sources: &sources,
        tracked: &tracked,
        snapshots: &snapshots,
    };
    let docs: Vec<&String> = tracked
        .iter()
        .filter(|path| path.strip_prefix(DESIGN_DIR).is_some_and(|rest| !rest.contains('/') && rest.ends_with(".md")))
        .collect();
    let (mut out, mut rows, mut findings, mut rc) = (Vec::new(), 0_usize, 0_usize, RC_OK);
    for doc in &docs {
        let (count, found) = judge_doc(repo, doc, &ctx);
        rows = rows.saturating_add(count);
        findings = findings.saturating_add(found.len());
        rc = found.iter().map(Finding::rc).fold(rc, u8::max);
        out.extend(found.iter().map(|finding| finding.render(doc)));
    }
    out.push(format!("contracts check: docs={} rows={rows} findings={findings}", docs.len()));
    Outcome { out, err: Vec::new(), rc }
}

/// doc 1 本の行数と findings（読めない doc・区間は 1 件ずつ名指す）。
fn judge_doc(repo: &Path, doc: &str, ctx: &Context<'_>) -> (usize, Vec<Finding>) {
    let text = match read(repo, doc) {
        Ok(found) => found,
        Err(reason) => return (0, vec![Finding::table(unreadable(0, &reason))]),
    };
    match read_rows(doc, &text) {
        Ok(rows) => (rows.len(), check_table(&text, &rows, &ids_of(&rows), ctx)),
        Err(errors) => (0, errors.into_iter().map(Finding::table).collect()),
    }
}

/// 行の列の id（`depends` の解決の母集団を全行から組む・[`check_table`] の `ids`）。
fn ids_of(rows: &[ContractRow]) -> Vec<&str> {
    rows.iter().map(|row| row.id.as_str()).collect()
}

/// repo 相対の file を読む（読めない理由は path を名乗る 1 行）。intake が設計 pointer の doc を読む口でもある。
pub(crate) fn read(repo: &Path, path: &str) -> Result<String, String> {
    std::fs::read_to_string(repo.join(path)).map_err(|err| format!("{path} を読めない: {err}"))
}

/// tracked file の repo 相対 path（`git ls-files -z`・git repo でなければ `None`）。`contracts check` と intake の
/// 上限の余地・交差の展開が同じ一覧を読む。
pub(crate) fn tracked_files(repo: &Path) -> Option<Vec<String>> {
    let listed = super::super::git_bytes(repo, &["ls-files", "-z"])?;
    Some(String::from_utf8_lossy(&listed).split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect())
}

/// tracked のうち拡張子 `ext` の file を全部読む（読めない周は理由を持つ・黙って落とさない）。
pub(crate) fn read_all(repo: &Path, tracked: &[String], ext: &str) -> Vec<Source> {
    tracked
        .iter()
        .filter(|path| path.ends_with(ext))
        .map(|path| Source { path: path.clone(), body: read(repo, path) })
        .collect()
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.374

    use super::{check_table, ids_of, read_rows, requirement_ids, Context, ContractRow, BEGIN, END};
    use crate::cli_outcome::RC_BROKEN;
    use crate::pipe::closure::Source;
    use crate::pipe::refuse::Refuse;
    use std::collections::BTreeSet;

    /// 要件面は拡張子で読み手を分ける（`.html` の要件 id の形の anchor・`.yaml` の列・`.md` の要件 id で始まる
    /// 見出し）。他の形は読まない。
    #[test]
    fn table_requirement_faces_read_html_anchors_and_yaml_lists_by_extension() {
        let set = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<BTreeSet<String>>();
        let html = "<p id=\"FR47\">x</p><p id='NFR4'>y</p><div id=\"toc\">z</div>";
        assert_eq!(requirement_ids("spec/srs.html", html), Ok(set(&["FR47", "NFR4"])), "要件 id の形の anchor だけ");
        let yaml = "requirements:\n  - FR1\n  - \"AC2\"\n  - id: FR3\n";
        assert_eq!(requirement_ids("spec/reqs.yaml", yaml), Ok(set(&["AC2", "FR1", "FR3"])));
        let md = "# 要件\n\n## FR1 便の起動\n\n本文。\n\n### NFR2\n\n## 3. 番号の節\n\nFR9 は本文の字面。\n#FR8 は見出しでない\n";
        assert_eq!(requirement_ids("spec/reqs.md", md), Ok(set(&["FR1", "NFR2"])), "行頭 `#` の見出しの先頭 token だけ");
        assert!(requirement_ids("spec/reqs.json", "{}").is_err(), "他の拡張子は読まない");
    }

    /// 検査に要る欄だけを選ぶ 1 行（残りは適合する既定）。
    fn row(line: u64, id: &str) -> ContractRow {
        let one = |value: &str| vec![value.to_owned()];
        ContractRow {
            line,
            id: id.to_owned(),
            title: "t".to_owned(),
            req: one("FR1"),
            section: "1".to_owned(),
            touches: Vec::new(),
            surfaces: Vec::new(),
            write_set: one("src/kind.rs"),
            creates: Vec::new(),
            tests: Vec::new(),
            also: Vec::new(),
            verify: one("git status"),
            size: "S".to_owned(),
            done: "d".to_owned(),
            depends: Vec::new(),
            classes: Vec::new(),
            opens: Vec::new(),
        }
    }

    /// 節の fixture（§1 = 本文あり / §2 = 本文なし / fence の中の `## 4.` は見出しでない）。
    const DOC: &str = "# t\n\n## 1. 本文の在る節\n\n本文。\n\n## 2. 空の節\n\n## 3. fence\n\n```\n## 4. 見出しではない\n```\n";

    /// 閉包の fixture（`crate::kind::Kind` の宣言 file と、`use` で取り込んで arm を持つ file＝閉包は「その file から型が
    /// 見えているか」を先に判定する〔closure.rs の `sees`・`s2-07l.347`〕ので、素の `Kind::A` は現実の Rust と同じに
    /// scope に無い）。
    fn sources() -> Vec<Source> {
        let source = |path: &str, body: &str| Source { path: path.to_owned(), body: Ok(body.to_owned()) };
        vec![
            source("src/kind.rs", "pub enum Kind {\n    A,\n}\n\npub const KINDS: &[Kind] = &[Kind::A];\n"),
            source("src/use.rs", "use crate::kind::Kind;\n\nfn f(kind: Kind) -> u8 {\n    match kind {\n        Kind::A => 1,\n    }\n}\n"),
        ]
    }

    /// 欠陥を 1 つずつ持つ行を、行番号の順に**全件**名指す（1 件目で止めない・適合する行は名指さない）。
    #[test]
    fn table_check_names_every_defect_with_its_row_line() {
        let requirements = Ok(["FR1".to_owned()].into_iter().collect::<BTreeSet<String>>());
        let (allowed, sources) = (["git".to_owned()], sources());
        let tracked = ["src/kind.rs".to_owned(), "src/use.rs".to_owned()];
        let ctx = Context {
            allowed: &allowed,
            denied: &[],
            requirements: &requirements,
            sources: &sources,
            tracked: &tracked,
            snapshots: &[],
        };
        let mut rows: Vec<ContractRow> = ["a", "a", "c", "d", "e", "f", "g", "h", "i"]
            .iter()
            .enumerate()
            .map(|(index, id)| row((index as u64 + 1) * 10, id))
            .collect();
        rows[1].req.push("FR9".to_owned());
        rows[2].section = "2".to_owned();
        rows[3].section = "4".to_owned();
        rows[4].verify = vec!["git log (x)".to_owned()];
        rows[5].depends = vec!["zz".to_owned()];
        rows[6].depends = vec!["h".to_owned()];
        rows[7].depends = vec!["g".to_owned()];
        rows[8].touches = vec!["crate::kind::Kind".to_owned()];
        rows[8].write_set = vec!["src/kind.rs".to_owned(), "src".to_owned()];
        let found = check_table(DOC, &rows, &ids_of(&rows), &ctx);
        let shown: Vec<(u64, String)> = found.iter().map(|finding| (finding.line, finding.refuse.label())).collect();
        let want: Vec<(u64, &str)> = vec![
            (20, "contract-table:duplicate-id"),
            (20, "contract-table:requirement-missing"),
            (30, "contract-table:section-missing"),
            (40, "contract-table:section-missing"),
            (50, "contract-table:verify-form"),
            (60, "contract-table:depends-unresolved"),
            (70, "contract-table:depends-cycle"),
            (90, "write-set-dir-without-slash"),
            (90, "write-set-incomplete"),
        ];
        assert_eq!(shown, want.iter().map(|(line, label)| (*line, (*label).to_owned())).collect::<Vec<_>>());
        let missing = Refuse::WriteSetIncomplete { missing: vec!["src/use.rs".to_owned()] };
        assert_eq!(found.last().map(|finding| &finding.refuse), Some(&missing), "足りない file だけを名指す");
        let cycle = found.iter().find(|finding| finding.line == 70).map(|finding| finding.refuse.reason());
        assert_eq!(cycle.as_deref(), Some("depends が輪を成す（g → h → g）"), "輪は 1 件で 2 行を名乗る");
    }

    /// `depends` の解決の母集団は引数の `ids`（§30・行 ad）: 検査する行が 1 つの slice でも、母集団に相手の id が
    /// 在れば解け、母集団に無ければ `depends-unresolved` の 1 件（母集団を slice の id だけにすると、別の行への
    /// `depends` は常に解けない＝受付の従来の形）。
    #[test]
    fn table_check_resolves_depends_against_the_given_ids_not_the_checked_slice() {
        let requirements = Ok(["FR1".to_owned()].into_iter().collect::<BTreeSet<String>>());
        let (allowed, sources) = (["git".to_owned()], sources());
        let tracked = ["src/kind.rs".to_owned(), "src/use.rs".to_owned()];
        let ctx = Context {
            allowed: &allowed,
            denied: &[],
            requirements: &requirements,
            sources: &sources,
            tracked: &tracked,
            snapshots: &[],
        };
        let mut dependent = row(20, "b");
        dependent.depends = vec!["a".to_owned()];
        let labels = |ids: &[&str]| -> Vec<String> {
            check_table(DOC, std::slice::from_ref(&dependent), ids, &ctx).iter().map(|finding| finding.refuse.label()).collect()
        };
        assert!(labels(&["a", "b"]).is_empty(), "母集団に相手が在れば解ける");
        assert_eq!(labels(&["b"]), vec!["contract-table:depends-unresolved".to_owned()], "slice の id だけでは解けない");
        assert_eq!(labels(&["b", "zz"]), vec!["contract-table:depends-unresolved".to_owned()], "相手の無い depends は断る");
    }

    /// 閉包は「その file から型が見えているか」を先に判定する（closure.rs の `sees`・§3「閉包の同名衝突」・`s2-07l.347`）:
    /// 取り込みも修飾も無い素の `Kind::A` の arm を持つ file（別 module の同名の型を指す形）は閉包に入らず、write-set が
    /// その file を欠いても `write-set-incomplete` で名指されない。取り込む fixture（[`sources`]）は従来どおり名指す。
    #[test]
    fn contract_closure_ext_same_name_table_check_skips_a_file_that_cannot_see_the_type() {
        let requirements = Ok(["FR1".to_owned()].into_iter().collect::<BTreeSet<String>>());
        let allowed = ["git".to_owned()];
        let tracked = ["src/kind.rs".to_owned(), "src/use.rs".to_owned()];
        let mut blind = sources();
        blind[1].body = Ok("fn f(kind: Kind) -> u8 {\n    match kind {\n        Kind::A => 1,\n    }\n}\n".to_owned());
        let ctx = |sources: &[Source]| -> Vec<String> {
            let ctx = Context {
                allowed: &allowed,
                denied: &[],
                requirements: &requirements,
                sources,
                tracked: &tracked,
                snapshots: &[],
            };
            let mut touched = row(10, "a");
            touched.touches = vec!["crate::kind::Kind".to_owned()];
            check_table(DOC, &[touched], &["a"], &ctx).iter().map(|finding| finding.refuse.label()).collect()
        };
        assert!(ctx(&blind).is_empty(), "型が見えていない file は閉包に入らない");
        assert_eq!(ctx(&sources()), vec!["write-set-incomplete".to_owned()], "取り込む file は従来どおり名指す");
    }

    /// 導出の形の行（契約 (h)・§3）: `write-set` の無い行は読め（`creates` / `tests` / `also` は欄として持つ）、CI の検査は
    /// 閉包 ⊆ write-set を撃たない（受付が導出値を write-set にする）が、型の形と外形の名は従来どおり名指す。
    /// `creates` の新規 file は名指しの実在で解ける（write-set の `+` 項目と同じ）。write-set を持つ行は従来どおり。
    #[test]
    fn table_check_skips_the_closure_subset_for_rows_without_a_write_set() {
        let requirements = Ok(["FR1".to_owned()].into_iter().collect::<BTreeSet<String>>());
        let (allowed, sources) = (["git".to_owned()], sources());
        let tracked = ["src/kind.rs".to_owned(), "src/use.rs".to_owned()];
        let ctx = Context {
            allowed: &allowed,
            denied: &[],
            requirements: &requirements,
            sources: &sources,
            tracked: &tracked,
            snapshots: &[],
        };
        let text = format!("# t\n\n{BEGIN}\nschema = 1\n\n[[contract]]\nid = \"a\"\ntitle = \"t\"\nreq = [\"FR1\"]\nsection = \"1\"\ntouches = [\"crate::kind::Kind\"]\ncreates = [\"src/new.rs\"]\ntests = [\"tests/t.rs\"]\nalso = [\"docs/d.md\"]\nverify = [\"git status\"]\nsize = \"S\"\ndone = \"`src/new.rs` が通る\"\n{END}\n");
        let rows = read_rows("docs/design/t.md", &text).unwrap_or_else(|errors| panic!("write-set の無い行は読める: {errors:?}"));
        let row = rows.first().cloned().unwrap_or_else(|| panic!("1 行"));
        assert!(row.write_set.is_empty(), "write-set は無い");
        assert_eq!((row.creates.len(), row.tests.len(), row.also.len()), (1, 1, 1), "導出の 3 欄を欄として持つ");
        assert!(check_table(DOC, &rows, &["a"], &ctx).is_empty(), "閉包 ⊆ write-set と名指しは撃たない・creates の新規 file は解ける");
        let mut malformed = row.clone();
        malformed.touches = vec!["Kind".to_owned()];
        let labels: Vec<String> = check_table(DOC, &[malformed], &["a"], &ctx).iter().map(|finding| finding.refuse.label()).collect();
        assert_eq!(labels, vec!["contract-table:unreadable".to_owned()], "型の形は従来どおり名指す");
        let mut declared = row;
        declared.write_set = vec!["src/kind.rs".to_owned()];
        let labels: Vec<String> = check_table(DOC, &[declared], &["a"], &ctx).iter().map(|finding| finding.refuse.label()).collect();
        assert_eq!(labels, vec!["write-set-incomplete".to_owned()], "write-set を持つ行は閉包 ⊆ write-set を撃つ");
    }

    /// 契約表の verify 行にも intake と同じ禁じる語列の判定が掛かる（ADR-0025 §2.3・FR55「intake と同じ検査を表の全行に」）:
    /// 先頭語が allowlist に在っても `runner.denied_commands` の語列に当たる行は `verify-form` で行番号付きに名指し、
    /// 理由は行 id と語列を持つ。語列を持たない文脈（`denied = []`）では同じ行が通る＝判定の出所は行の値である。
    #[test]
    fn table_check_names_a_verify_line_that_hits_a_denied_sequence() {
        let requirements = Ok(["FR1".to_owned()].into_iter().collect::<BTreeSet<String>>());
        let (allowed, sources) = (["git".to_owned()], sources());
        let denied = ["git push --force".to_owned()];
        let tracked = ["src/kind.rs".to_owned(), "src/use.rs".to_owned()];
        let mut forced = row(10, "a");
        forced.verify = vec!["git push origin main --force".to_owned()];
        let closed = Context {
            allowed: &allowed,
            denied: &denied,
            requirements: &requirements,
            sources: &sources,
            tracked: &tracked,
            snapshots: &[],
        };
        let found = check_table(DOC, &[forced.clone()], &["a"], &closed);
        let labels: Vec<String> = found.iter().map(|finding| finding.refuse.label()).collect();
        assert_eq!(labels, vec!["contract-table:verify-form".to_owned()], "禁じる語列の行は verify-form の 1 件: {labels:?}");
        let rendered = found.first().map(|finding| finding.render("docs/design/t.md")).unwrap_or_default();
        assert!(rendered.starts_with("contracts: docs/design/t.md:10 "), "行番号付き: {rendered}");
        assert!(rendered.contains("runner.denied_commands") && rendered.contains("git push --force"), "行 id と語列: {rendered}");
        let open = Context { denied: &[], ..closed };
        assert!(check_table(DOC, &[forced], &["a"], &open).is_empty(), "語列の無い文脈では通る（判定の出所は行の値）");
    }

    /// 要件面を読めない周・閉包の入力を読めない周は、黙って通さず行ごとに `unreadable`（rc 2）で名指す。
    #[test]
    fn table_check_fails_closed_when_the_requirement_face_or_a_source_is_unreadable() {
        let requirements: Result<BTreeSet<String>, String> = Err("srs を読めない".to_owned());
        let allowed = ["git".to_owned()];
        let mut sources = sources();
        sources.push(Source { path: "src/broken.rs".to_owned(), body: Err("invalid utf-8".to_owned()) });
        let tracked = ["src/kind.rs".to_owned()];
        let ctx = Context {
            allowed: &allowed,
            denied: &[],
            requirements: &requirements,
            sources: &sources,
            tracked: &tracked,
            snapshots: &[],
        };
        let mut touched = row(10, "a");
        touched.touches = vec!["crate::kind::Kind".to_owned()];
        let found = check_table(DOC, &[touched], &["a"], &ctx);
        let rendered: Vec<String> = found.iter().map(|finding| finding.render("docs/design/t.md")).collect();
        assert_eq!(found.len(), 2, "要件面と閉包の入力の 2 件: {rendered:?}");
        assert!(found.iter().all(|finding| finding.rc() == RC_BROKEN), "読めない周は rc 2: {rendered:?}");
        assert!(rendered.iter().any(|line| line.starts_with("contracts: docs/design/t.md:10 contract-table:unreadable: srs")));
        assert!(rendered.iter().any(|line| line.contains("src/broken.rs を読めない")), "{rendered:?}");
    }
}
