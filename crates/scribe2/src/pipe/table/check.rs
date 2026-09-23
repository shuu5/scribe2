//! 契約表の検査の本体と要件面の読みと CLI の駆動（設計 docs/design/contract-source.md §2 / §3 / §15・SRS FR47 /
//! FR55）。
//!
//! 表の全行を [`check_table`] が**全件・行番号付き**で検査し（id の一意・`req` の要件面での実在・`section` の節の
//! 実在・verify の形・`depends` の解決と輪・`touches` の閉包と `surfaces` の外形 pin ⊆ `write-set`・write-set の
//! 項目の実在・名指しの実在）、要件面の id は [`requirement_ids`] が拡張子ごとの読み手で取る。`contracts check` の
//! 駆動（tracked file の一覧・doc の読み・判定行）は [`check_repo`]、同じ検査の findings を doc・行 id・未解決の項目で
//! 呼び手へ返す口は [`repo_findings`]（追随の後に便の木へ撃つ・設計 pipeline.md §34）。findings の語彙（[`super::TableError`] /
//! [`super::Finding`] / [`super::Context`]）は親 module `table.rs`・置き場の抜き出しと parse は兄弟 `table/parse.rs`
//! に置いたまま。呼び手（`pipe/cli.rs`・`pipe/cli/intake.rs`・歯）の `use` は親の再 export を通る。

use super::super::closure::{closure, surface_closure, teeth_places, unresolved_names, Base, ClosureError, Fields, Source};
use super::super::declaration::{self, read_write_set, Basis, Ceiling, NewFilePolicy};
use super::super::refuse::{covered, Refuse, NEW_FILE};
use super::{read_table, unreadable, Context, ContractRow, Finding, PromiseRow, TableError, BEGIN, DESIGN_DIR, END};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK};
use crate::name::NAME;
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

/// 約束の行の検査（設計 §33・**全件・行番号の順**）: `of` が `ids`（同じ doc の全行の id）に無い約束の行と、親の行
/// ごとの `n` の重複（2 本目の約束の行）と欠番（1 から数えた欠けの始まりの番号・親の行の最初の約束の行）。欄の形と
/// 空の必須欄は parse の段（`read_table`）が名指し済み。
pub fn check_promises(promises: &[PromiseRow], ids: &[&str]) -> Vec<Finding> {
    let mut found: Vec<Finding> = promises
        .iter()
        .filter(|promise| !ids.contains(&promise.of.as_str()))
        .map(|promise| Finding::table(TableError::PromiseOrphan { line: promise.line, of: promise.of.clone() }))
        .collect();
    for (index, promise) in promises.iter().enumerate() {
        if promises.iter().take(index).any(|seen| seen.of == promise.of && seen.n == promise.n) {
            let (of, n) = (promise.of.clone(), promise.n);
            found.push(Finding::table(TableError::PromiseNumber { line: promise.line, of, n, duplicate: true }));
        }
    }
    let mut parents: Vec<&str> = Vec::new();
    for promise in promises {
        if !parents.contains(&promise.of.as_str()) {
            parents.push(&promise.of);
        }
    }
    for parent in parents {
        let own: Vec<&PromiseRow> = promises.iter().filter(|promise| promise.of == parent).collect();
        let first = own.first().map_or(0, |promise| promise.line);
        let numbers: BTreeSet<u64> = own.iter().map(|promise| promise.n).collect();
        let mut last = 0_u64;
        for n in numbers {
            let next = last.saturating_add(1);
            if n > next {
                let of = parent.to_owned();
                found.push(Finding::table(TableError::PromiseNumber { line: first, of, n: next, duplicate: false }));
            }
            last = n;
        }
    }
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
/// （導出の形・`+` 無しで書く）の両方から解き、他の行が宣言済みの新規 file（[`Context::declared`]・§39）も解に
/// 足す。宣言の母集団を読めない周も `unreadable` の 1 件（縮めた母集団で通さない）。
fn name_findings(doc: &str, row: &ContractRow, ctx: &Context<'_>) -> Vec<Finding> {
    let declared = match *ctx.declared {
        Err(ref reason) => return vec![Finding::table(unreadable(row.line, reason))],
        Ok(ref found) => found,
    };
    let mut texts = vec![("title".to_owned(), row.title.clone()), ("done".to_owned(), row.done.clone())];
    texts.extend(
        section_lines(doc, &row.section).into_iter().map(|(at, line)| (format!("section {} line {at}", row.section), line)),
    );
    let mut new_files = row.write_set.clone();
    new_files.extend(row.creates.iter().chain(declared).map(|item| format!("{NEW_FILE}{item}")));
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
/// 未追跡の設計 doc は判定行の前に 1 件 1 行で知らせ、判定行の `untracked=` に本数を出す（findings にも rc にも
/// 数えない検出線・設計 §43 (3) / 行 at）。Declared 行の歯の置き場も同じ検出線で、判定行の末尾の
/// `place-out=<行数>/<Declared 行数>` に出し、当たった行は `verbose` の周だけ判定行の前に 1 行ずつ出す（§45・行 av）。
pub(crate) fn check_repo(repo: &Path, ceiling: &Ceiling<'_>, verbose: bool) -> Outcome {
    let judged = match judge_repo(repo, ceiling) {
        Ok(found) => found,
        Err(stopped) => return stopped,
    };
    let rc = judged.found.iter().map(|(_, finding)| finding.rc()).fold(RC_OK, u8::max);
    let mut out: Vec<String> = judged.found.iter().map(|(doc, finding)| finding.render(doc)).collect();
    let (notices, untracked) = untracked_notices(judged.untracked.as_deref());
    out.extend(notices);
    let (hits, place_out) = place_notices(&judged.places);
    if verbose {
        out.extend(hits);
    }
    // 宣言が入口の flip を測らないと名乗った周だけ末尾に欄を足す（名乗りの無い周の判定行は 1 字も変えない・§54 形 5）。
    let entrance = judged.entrance.map(|named| format!(" entrance={}", named.as_str())).unwrap_or_default();
    out.push(format!(
        "contracts check: docs={} rows={} untracked={untracked} findings={}{entrance} place-out={place_out}",
        judged.docs,
        judged.rows,
        judged.found.len()
    ));
    Outcome { out, err: Vec::new(), rc }
}

/// 置き場の検出線から、当たった行の知らせ（1 件 1 行・doc・行 id・write-set の外の歯の file）と判定行の
/// `place-out=` の値（`<行数>/<Declared 行数>`）を組む。閉包の入力を読めない周は知らせ 0 行で行数は `?`（0 に化けさせ
/// ない・NFR4）。
fn place_notices(places: &Places) -> (Vec<String>, String) {
    match places.outside {
        None => (Vec::new(), format!("?/{}", places.declared)),
        Some(ref hits) => {
            let notices = hits
                .iter()
                .map(|hit| format!("contracts place-out: {} 行 {} の歯の file が write-set の外: {}", hit.doc, hit.id, hit.files.join(", ")))
                .collect();
            (notices, format!("{}/{}", hits.len(), places.declared))
        }
    }
}

/// Declared 行の歯の置き場の検出線（設計 §45・行 av）: 母集団と、解けた歯の file を write-set の外に持つ行。
struct Places {
    /// Declared 行（`creates` / `tests` / `also` を持たず `write-set` を持つ行）の数＝判定行の分母。
    declared: usize,
    /// 当たった行（doc 順・doc の中は行の順）。閉包の入力を読めない周は `None`（測れないを 0 行に読み替えない）。
    outside: Option<Vec<PlaceHit>>,
}

/// 置き場の検出線に当たった 1 行。
struct PlaceHit {
    /// 行を持つ設計 doc（repo 相対）。
    doc: String,
    /// 行 id。
    id: String,
    /// write-set の外の歯の file（辞書順）。
    files: Vec<String>,
}

impl Places {
    /// doc 1 本の区間の Declared 行を数え、行ごとに歯の置き場を測る（区間を読めない doc は数えない＝`rows=` と同じ母集団）。
    fn measure(&mut self, doc: &str, text: &str, ctx: &Context<'_>) {
        let Ok((rows, _)) = read_table(doc, text) else {
            return;
        };
        let declared: Vec<&ContractRow> = rows.iter().filter(|row| is_declared(row)).collect();
        self.declared = self.declared.saturating_add(declared.len());
        let texts: Option<Vec<(&str, &str)>> =
            ctx.sources.iter().map(|source| source.body.as_deref().ok().map(|body| (source.path.as_str(), body))).collect();
        let (Some(hits), Some(texts)) = (self.outside.as_mut(), texts) else {
            self.outside = None;
            return;
        };
        let base = Base { sources: ctx.sources, snapshots: ctx.snapshots, tracked: ctx.tracked, core_crate: NAME };
        for row in declared {
            let files = teeth_outside(row, &base, &texts);
            if !files.is_empty() {
                hits.push(PlaceHit { doc: doc.to_owned(), id: row.id.clone(), files });
            }
        }
    }
}

/// Declared 行か（受付の弁別と同じ形: 導出の欄 `creates` / `tests` / `also` を持たず `write-set` を持つ・設計 §3）。
fn is_declared(row: &ContractRow) -> bool {
    row.creates.is_empty() && row.tests.is_empty() && row.also.is_empty() && !row.write_set.is_empty()
}

/// 行の verify の nextest 行ごとに歯の置き場を [`teeth_places`] で解き、write-set（印を剥がして照合・dir 項目は配下）の
/// 外の file を集める（辞書順）。解けない行（base で 0 本の filter 語）は数えない（既存の findings に任せる・§45 形 1）。
fn teeth_outside(row: &ContractRow, base: &Base<'_>, texts: &[(&str, &str)]) -> Vec<String> {
    let fields = Fields {
        touches: &row.touches,
        surfaces: &row.surfaces,
        verify: &row.verify,
        creates: &row.creates,
        tests: &row.tests,
        also: &row.also,
        files: &[],
    };
    let mut outside = BTreeSet::new();
    for line in &row.verify {
        let one = Fields { verify: std::slice::from_ref(line), ..fields };
        if let Ok(found) = teeth_places(&one, base, texts) {
            outside.extend(found.into_iter().filter(|path| !covered(&row.write_set, path)));
        }
    }
    outside.into_iter().collect()
}

/// 未追跡の設計 doc の列から知らせの行（1 件 1 行・path を名乗る）と判定行の `untracked=` の値を組む。git が答え
/// なかった周（`None`）は知らせ 0 行で値は `?`（0 に化けさせない・NFR4）。
fn untracked_notices(untracked: Option<&[String]>) -> (Vec<String>, String) {
    match untracked {
        None => (Vec::new(), "?".to_owned()),
        Some(paths) => {
            let notices = paths
                .iter()
                .map(|path| format!("contracts untracked-doc: {path} は未追跡の設計 doc（検査の母集団に入らない）"))
                .collect();
            (notices, paths.len().to_string())
        }
    }
}

/// 契約表の検査の 1 件を doc と行 id 付きで持つ（[`repo_findings`] の戻り・設計 pipeline.md §34）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Located {
    /// 行を持つ設計 doc（repo 相対）。
    pub doc: String,
    /// 行 id（行番号に当たる契約の行が無い 1 件〔doc 全体・約束の行〕は `None`）。
    pub id: Option<String>,
    /// write-set の項目の未解決ならその項目の字面（[`Finding::unresolved_item`]・他の理由は `None`）。
    pub unresolved: Option<String>,
}

/// [`check_repo`] と**同じ入力・同じ検査**の findings を、呼び手が読める形（doc・行 id・未解決の項目）で返す
/// （設計 pipeline.md §34・追随の後に便の木へ撃つ口）。tracked file の一覧か宣言を読めない周は `None`
/// （読めないを「findings 0」に読み替えない・NFR4）。
pub(crate) fn repo_findings(repo: &Path, ceiling: &Ceiling<'_>) -> Option<Vec<Located>> {
    let judged = judge_repo(repo, ceiling).ok()?;
    let mut located = Vec::new();
    for (doc, finding) in judged.found {
        let rows = read(repo, &doc).ok().and_then(|text| read_table(&doc, &text).ok()).map(|(rows, _)| rows);
        let id = rows.and_then(|rows| rows.into_iter().find(|row| row.line == finding.line).map(|row| row.id));
        let unresolved = finding.unresolved_item().map(str::to_owned);
        located.push(Located { doc, id, unresolved });
    }
    Some(located)
}

/// repo の全 doc を検査した結果（doc の数・行の数・doc 順の findings）。
struct Judged {
    /// 検査した doc の数。
    docs: usize,
    /// 検査した行の数。
    rows: usize,
    /// (doc, 1 件)（doc 順・doc の中は行番号の順）。
    found: Vec<(String, Finding)>,
    /// 未追跡の設計 doc（検査の母集団の外・知らせだけ）。git が答えない周は `None`。
    untracked: Option<Vec<String>>,
    /// 宣言の入口の flip の名乗り（任意 key `entrance-flip`・無ければ `None`）。
    entrance: Option<declaration::EntranceFlip>,
    /// Declared 行の歯の置き場の検出線（findings にも rc にも数えない・§45）。
    places: Places,
}

/// tracked な `docs/design/*.md` の区間を全行検査する（[`check_repo`] と [`repo_findings`] の共通の 1 本）。判定できない
/// 周（tracked file の一覧か宣言を読めない）は理由の Outcome（rc 2）。
fn judge_repo(repo: &Path, ceiling: &Ceiling<'_>) -> Result<Judged, Outcome> {
    let Some(tracked) = tracked_files(repo) else {
        let reason = format!("contracts: {} の tracked file を読めない（git repo でない）", repo.display());
        return Err(Outcome::failed_line(RC_BROKEN, reason));
    };
    let (facts, entrance) = match declaration::table_facts_named(repo, ceiling) {
        Ok(found) => found,
        Err(errors) => return Err(Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect())),
    };
    let sources = read_all(repo, &tracked, ".rs");
    let snapshots = read_all(repo, &tracked, ".snap");
    let requirements = read(repo, &facts.requirements).and_then(|text| requirement_ids(&facts.requirements, &text));
    let declared = declared_files(repo, &tracked);
    let ctx = Context {
        allowed: &facts.allowed,
        denied: &facts.denied,
        requirements: &requirements,
        sources: &sources,
        tracked: &tracked,
        snapshots: &snapshots,
        declared: &declared,
    };
    let docs = design_docs(&tracked);
    let (mut rows, mut found) = (0_usize, Vec::new());
    let mut places = Places { declared: 0, outside: Some(Vec::new()) };
    for doc in &docs {
        let (count, judged) = judge_doc(repo, doc, &ctx, &mut places);
        rows = rows.saturating_add(count);
        found.extend(judged.into_iter().map(|finding| ((*doc).clone(), finding)));
    }
    let untracked = untracked_files(repo).map(|paths| design_docs(&paths).into_iter().cloned().collect());
    Ok(Judged { docs: docs.len(), rows, found, untracked, entrance, places })
}

/// tracked な設計 doc（`docs/design/` 直下の `.md`・tracked の順）。
fn design_docs(tracked: &[String]) -> Vec<&String> {
    tracked
        .iter()
        .filter(|path| path.strip_prefix(DESIGN_DIR).is_some_and(|rest| !rest.contains('/') && rest.ends_with(".md")))
        .collect()
}

/// 宣言済みの新規 file の母集団（設計 §39・行 an）: tracked な設計 doc の区間の全行から write-set の `+` 項目と
/// `creates` の欄を集める（印は剥がす・辞書順・重複は畳む）。path は repo で一意ゆえ doc の境は引かない。
/// `contracts check`（[`check_repo`]）と受付の材料（`pipe/cli/intake.rs` の `Materials`）が同じこの 1 本を呼ぶ。
/// 区間を読めない doc が 1 本でも在れば理由を返す（読めなさを「宣言 0 本」に読み替えない・NFR4）。
pub(crate) fn declared_files(repo: &Path, tracked: &[String]) -> Result<Vec<String>, String> {
    let mut found = BTreeSet::new();
    for doc in design_docs(tracked) {
        let text = read(repo, doc)?;
        let (rows, _) = read_table(doc, &text).map_err(|errors| {
            let first = errors.first().map(TableError::reason).unwrap_or_default();
            format!("{doc} の区間を読めない（宣言済みの新規 file の母集団）: {first}")
        })?;
        for row in rows {
            found.extend(row.write_set.iter().filter_map(|item| item.strip_prefix(NEW_FILE)).map(str::to_owned));
            found.extend(row.creates);
        }
    }
    Ok(found.into_iter().collect())
}

/// doc 1 本の行数と findings（読めない doc・区間は 1 件ずつ名指す）。同じ本文で置き場の検出線も測る。
fn judge_doc(repo: &Path, doc: &str, ctx: &Context<'_>, places: &mut Places) -> (usize, Vec<Finding>) {
    let text = match read(repo, doc) {
        Ok(found) => found,
        Err(reason) => return (0, vec![Finding::table(unreadable(0, &reason))]),
    };
    places.measure(doc, &text, ctx);
    judge_text(doc, &text, ctx)
}

/// 読めた doc 1 本の本文の行数と findings（契約の行の検査と約束の行の検査を行番号の順に合わせる）。
fn judge_text(doc: &str, text: &str, ctx: &Context<'_>) -> (usize, Vec<Finding>) {
    match read_table(doc, text) {
        Ok((rows, promises)) => {
            let ids = ids_of(&rows);
            let mut found = check_table(text, &rows, &ids, ctx);
            if !promises.is_empty() {
                found.extend(check_promises(&promises, &ids));
                found.sort_by_key(|finding| finding.line);
            }
            (rows.len(), found)
        }
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
    ls_files(repo, &[])
}

/// 設計 doc の dir 配下の未追跡 file の repo 相対 path（ignore された file は除く・git が答えなければ `None`）。
/// 読む口は [`tracked_files`] と同じ `git ls-files -z` の 1 本（2 本目の読み手を作らない・設計 §43 (3)）。
fn untracked_files(repo: &Path) -> Option<Vec<String>> {
    ls_files(repo, &["--others", "--exclude-standard", "--", DESIGN_DIR])
}

/// `git ls-files -z <extra>` の repo 相対 path の列（rc≠0 は `None`）。
fn ls_files(repo: &Path, extra: &[&str]) -> Option<Vec<String>> {
    let mut args = vec!["ls-files", "-z"];
    args.extend_from_slice(extra);
    let listed = super::super::git_bytes(repo, &args)?;
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

    use super::super::read_rows;
    use super::super::tests::full_promise;
    use super::{
        check_table, ids_of, judge_text, requirement_ids, section_lines, untracked_notices, Context, ContractRow, BEGIN, END,
    };
    use crate::cli_outcome::{RC_BROKEN, RC_REFUSED};
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
            targets: Vec::new(),
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
            declared: &Ok(Vec::new()),
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
            declared: &Ok(Vec::new()),
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
                declared: &Ok(Vec::new()),
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
            declared: &Ok(Vec::new()),
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
            declared: &Ok(Vec::new()),
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

    /// 約束の行の 3 形（親の行の無い `of`・`n` の欠番・`n` の重複）は `contracts check` の doc 1 本の判定で
    /// `contract-table:promise-orphan` / `contract-table:promise-number` の 1 行ずつに行番号付きで名指され rc 1。
    /// 欠陥の無い約束の行は 0 件で、行の数（`rows=`）に約束の行を数えない。
    #[test]
    fn contract_promise_parse_check_names_orphans_gaps_and_duplicates_one_line_each() {
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
            declared: &Ok(Vec::new()),
        };
        let contract = "schema = 1\n\n[[contract]]\nid = \"a\"\ntitle = \"t\"\nreq = [\"FR1\"]\nsection = \"1\"\nwrite-set = [\"src/kind.rs\"]\nverify = [\"git status\"]\nsize = \"S\"\ndone = \"d\"\n";
        let doc = |promises: &[String]| format!("# t\n\n## 1. 本文の在る節\n\n本文。\n\n{BEGIN}\n{contract}{}{END}\n", promises.concat());
        let clean = doc(&[full_promise("a", 1, &[]), full_promise("a", 2, &[])]);
        let (rows, found) = judge_text("docs/design/t.md", &clean, &ctx);
        assert_eq!((rows, found.len()), (1, 0), "欠陥の無い約束の行は 0 件・行の数は契約の行だけ: {found:?}");
        let broken = doc(&[
            full_promise("zz", 1, &[]),
            full_promise("a", 1, &[]),
            full_promise("a", 1, &[]),
            full_promise("a", 3, &[]),
        ]);
        let at: Vec<usize> =
            broken.lines().enumerate().filter(|(_, line)| *line == "[[promise]]").map(|(index, _)| index + 1).collect();
        let (_, found) = judge_text("docs/design/t.md", &broken, &ctx);
        let rendered: Vec<String> = found.iter().map(|finding| finding.render("docs/design/t.md")).collect();
        let line = |index: usize| at.get(index).copied().unwrap_or_default();
        let want = vec![
            format!("contracts: docs/design/t.md:{} contract-table:promise-orphan: [[promise]] の of zz が同じ doc の行 id に無い", line(0)),
            format!("contracts: docs/design/t.md:{} contract-table:promise-number: 行 a の約束の n 2 が欠ける（n は 1 から連番）", line(1)),
            format!("contracts: docs/design/t.md:{} contract-table:promise-number: 行 a の約束の n 1 が重複する", line(2)),
        ];
        assert_eq!(rendered, want, "3 形を 1 行ずつ行番号の順に名指す");
        assert!(found.iter().all(|finding| finding.rc() == RC_REFUSED), "前提違反の rc 1: {rendered:?}");
        let empty = doc(&[full_promise("a", 1, &[("text", "\"\"")])]);
        let (rows, found) = judge_text("docs/design/t.md", &empty, &ctx);
        let labels: Vec<String> = found.iter().map(|finding| finding.refuse.label()).collect();
        assert_eq!((rows, labels), (0, vec!["contract-table:unreadable".to_owned()]), "空の必須欄は既存の欄検査で名指す");
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
            declared: &Ok(Vec::new()),
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

    /// 未追跡の設計 doc の知らせ（§43 (3)・行 at）: 0 本・1 本・2 本の列で、知らせは 1 件 1 行で path を名乗り
    /// 列の順のまま、判定行の値は本数。git が答えない周（`None`）は知らせ 0 行で値は `?`（0 に化けない）。
    #[test]
    fn contracts_untracked_doc_notices_name_each_path_and_count_or_question_mark() {
        let paths = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect::<Vec<String>>();
        let notice = |path: &str| format!("contracts untracked-doc: {path} は未追跡の設計 doc（検査の母集団に入らない）");
        assert_eq!(untracked_notices(Some(&[])), (Vec::new(), "0".to_owned()), "0 本は知らせ無し・値 0");
        let one = paths(&["docs/design/draft.md"]);
        assert_eq!(untracked_notices(Some(&one)), (vec![notice("docs/design/draft.md")], "1".to_owned()));
        let two = paths(&["docs/design/b.md", "docs/design/a.md"]);
        let want = vec![notice("docs/design/b.md"), notice("docs/design/a.md")];
        assert_eq!(untracked_notices(Some(&two)), (want, "2".to_owned()), "1 件 1 行・列の順");
        assert_eq!(untracked_notices(None), (Vec::new(), "?".to_owned()), "git が答えない周は ?");
    }

    // ─────── Declared 行の歯の置き場の検出線（§45・行 av・接頭辞 `contract_check_place_`） ───────

    /// tmp の git repo（宣言・要件面・crate `toy` の型と歯 1 本・設計 doc）を作って commit する。
    fn place_repo(name: &str, doc: &str) -> std::path::PathBuf {
        let repo = std::env::temp_dir().join(format!("table-check-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        let files = [
            (".vessel.toml", "schema = 1\nallowed-commands = [\"git\", \"cargo\"]\ncommon-verify = [\"git status\"]\n"),
            ("design-intent/spec/srs.html", "<p id=\"FR1\">x</p>\n"),
            ("crates/toy/src/tint.rs", "pub enum Tint {\n    Warm,\n}\n"),
            ("crates/toy/tests/e2e.rs", "#[test]\nfn place_ok() {}\n"),
            ("docs/design/toy.md", doc),
        ];
        for (path, body) in files {
            let target = repo.join(path);
            let _ = target.parent().map(std::fs::create_dir_all);
            let _ = std::fs::write(&target, body);
        }
        let git = |args: &[&str]| crate::pipe::git_bytes(&repo, args).is_some();
        let seeded = git(&["init", "-q", "-b", "main"])
            && git(&["config", "user.name", "t"])
            && git(&["config", "user.email", "t@example.invalid"])
            && git(&["add", "-A"])
            && git(&["commit", "-q", "-m", "seed"]);
        assert!(seeded, "tmp の repo を commit できる: {}", repo.display());
        repo
    }

    /// Declared 行 3 本（歯が write-set の内 / 外 / base に 0 本で解けない）の repo で、判定行は `place-out=1/3`（解けない行は
    /// 数えない）・rc と findings は欄の無い形と同じ（0 件・rc 0）。当たった行は `verbose` の周だけ判定行の前に 1 行
    /// （doc・行 id・file）出て、無い周は 0 行。
    #[test]
    fn contract_check_place_counts_only_rows_whose_resolved_teeth_are_outside_the_write_set() {
        use super::check_repo;
        use crate::cli_outcome::RC_OK;
        use crate::pipe::declaration::Ceiling;
        let row = |id: &str, filter: &str, write_set: &str| {
            format!("[[contract]]\nid = \"{id}\"\ntitle = \"t\"\nreq = [\"FR1\"]\nsection = \"1\"\nwrite-set = [{write_set}]\nverify = [\"cargo nextest run -p toy --no-tests=fail {filter}\"]\nsize = \"S\"\ndone = \"d\"\n")
        };
        let rows = [
            row("in", "place_ok", "\"crates/toy/src/tint.rs\", \"crates/toy/tests/e2e.rs\""),
            row("out", "place_ok", "\"crates/toy/src/tint.rs\""),
            row("none", "place_fresh", "\"crates/toy/src/tint.rs\""),
        ];
        let doc = format!("# t\n\n## 1. 本文の在る節\n\n本文。\n\n{BEGIN}\nschema = 1\n\n{}{END}\n", rows.join("\n"));
        let repo = place_repo("place", &doc);
        let commands = ["git".to_owned(), "cargo".to_owned()];
        let ceiling = Ceiling { row: "runner.allowed_commands", commands: &commands, denied: &[] };
        let quiet = check_repo(&repo, &ceiling, false);
        let loud = check_repo(&repo, &ceiling, true);
        let _ = std::fs::remove_dir_all(&repo);
        let judgement = "contracts check: docs=1 rows=3 untracked=0 findings=0 place-out=1/3";
        assert_eq!((quiet.rc, quiet.out.clone()), (RC_OK, vec![judgement.to_owned()]), "旗の無い周は判定行だけ: {:?}", quiet.err);
        let hit = "contracts place-out: docs/design/toy.md 行 out の歯の file が write-set の外: crates/toy/tests/e2e.rs";
        assert_eq!((loud.rc, loud.out), (RC_OK, vec![hit.to_owned(), judgement.to_owned()]), "旗の周は当たった行 1 行 + 判定行");
    }

    // ─────── 節の切り出し（§31 (e)）: 4 本は腕 / guard / 否定を 1 つずつ落として別の歯が落ちる形 ───────

    /// 節 `number` の本文のうち doc 上の行番号が `keep` を満たす件数と、本文の全件（母集団）。
    fn counted(doc: &str, number: &str, keep: impl Fn(u64) -> bool) -> (usize, Vec<(u64, String)>) {
        let found = section_lines(doc, number);
        (found.iter().filter(|(at, _)| keep(*at)).count(), found)
    }

    /// (e) 開始の腕: 区間（3〜5 行目）の開始の行と中身は節の本文に混ざらない（腕を落とすと開始の行と行の中身が本文
    /// に入る）。区間は節の末尾に置く＝終了の腕を落としても後ろに失う行が無い。
    #[test]
    fn contract_closure_ext_survivor_e_begin_region_start_is_not_section_body() {
        // flip-check: retroactive s2-07l.277
        let doc = format!("## 1. t\nbody\n{BEGIN}\n| r |\n{END}\n");
        let (leaked, found) = counted(&doc, "1", |at| (3..=5).contains(&at));
        assert_eq!(leaked, 0, "区間の行は本文でない（区間の行 {leaked} 件 / 母集団 本文 {} 行）: {found:?}", found.len());
    }

    /// (e) 終了の腕: 区間（2〜4 行目）が閉じた後の行は節の本文に戻る（腕を落とすと区間が閉じず後ろを全部失う）。
    /// 後ろには fence の外と中の 1 行ずつを置く＝本文を拾う条件の否定を落としても 0 件にならない。
    #[test]
    fn contract_closure_ext_survivor_e_end_region_close_resumes_the_section_body() {
        // flip-check: retroactive s2-07l.277
        let doc = format!("## 2. t\n{BEGIN}\n| r |\n{END}\nafter\n```\nafter-fenced\n```\n");
        let (resumed, found) = counted(&doc, "2", |at| at > 4);
        assert!(resumed >= 1, "区間の後ろの行を拾う（後ろ {resumed} 件 / 母集団 本文 {} 行）: {found:?}", found.len());
    }

    /// (e) fence の guard: fence の中の `## 4.` は節を切り替えない＝fence（2〜4 行目）の後ろの行も §3 の本文（guard を
    /// 落とすと fence の中の見出しで §4 に切り替わり後ろを失う）。後ろには fence の外と中の 1 行ずつを置く。
    #[test]
    fn contract_closure_ext_survivor_e_fence_heading_inside_a_fence_keeps_the_section() {
        // flip-check: retroactive s2-07l.277
        let doc = "## 3. t\n```\n## 4. other\n```\nafter\n```\ntail-fenced\n```\n";
        let (kept, found) = counted(doc, "3", |at| at > 4);
        assert!(kept >= 1, "fence の後ろも §3 の本文（後ろ {kept} 件 / 母集団 本文 {} 行）: {found:?}", found.len());
    }

    /// (e) 本文を拾う条件の否定: fence の外の行（2 行目）は本文に入る（否定を落とすと fence の中だけを拾う）。
    #[test]
    fn contract_closure_ext_survivor_e_inside_body_outside_a_fence_is_collected() {
        // flip-check: retroactive s2-07l.277
        let doc = "## 5. t\nbody\n```\nin-fence\n```\n";
        let (outside, found) = counted(doc, "5", |at| at == 2);
        assert_eq!(outside, 1, "fence の外の行を拾う（外 {outside} 件 / 母集団 本文 {} 行）: {found:?}", found.len());
    }
}
