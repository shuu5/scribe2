//! 行の審査の口 `pipe review --ref`（設計 docs/design/row-review.md §3・§9・契約表の行 a）: 設計の PR の head の commit と
//! `origin/main` の先端の merge-base の間で digest の変わった契約表の行ごとに、受付と同じ機械の検査（予想の上の口
//! [`forecast_findings`]・祖先の層は [`ancestry`]）と、審査の木を cwd にした読みの道具の lens を撃ち、判定の鍵ごとの行の記録と
//! head ごとの ref の記録を state dir に書いて stdout に返す。記録の読みと鍵は [`super::row_review`]、木の腕は
//! [`super::review::tree`]、lens の判定の読みと版は [`super::review`] の口を呼ぶ（2 本目の読み手を書かない）。歯は e2e の
//! `pipe_review_ref_`。

use super::admission;
use super::cli::{broken, flag, generated, refused, repo_of, state_dir_of, Denial, Materials};
use super::confine;
use super::contract::Contract;
use super::dispatch::floor::Worktree;
use super::dispatch::precheck::{ancestry, forecast_findings, Ancestor, Ancestry, Finding, Layer, Standing};
use super::gate::{Limits, Verdict};
use super::ratelimit::{select_lens_account, LensAccount, Pool};
use super::refuse::Certainty;
use super::review::tree::{digest, materialize, requirements_of};
use super::review::{design_material, lens_version, read_lens, stage, FindingKind, REVIEW_DIR};
use super::row_review::{judgement, mark_path, ref_path, root_of, row_digest, tree_key, Basis, Parts, REF_DIR, SCHEMA_LINE};
use super::table::{form_of, parse_pointer, read_rows, tracked_files, Pointer, BEGIN, END};
use super::{git_line, git_ok};
use crate::cli_outcome::{Outcome, RC_OK, RC_REFUSED};
use crate::fleet::lifecycle_mark::MAIN_REF;
use crate::fleet::store::{lock_owner, started_ms, LockPolicy, Owner};
use crate::fleet::Usage;
use crate::ledger::form::{is_memo, is_question};
use crate::ledger::lint::pointer_of;
use crate::rules::manifest::Manifest;
use crate::seat::ledger::{read_ledger, timeout_of, Issue, DEFAULT_BD};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// 結果の行と行ごとの行の書き出し。
const LINE: &str = "[ROW-REVIEW]";

/// 台帳の閉じた status の字面（着地済みの行の判定が読む）。
const CLOSED: &str = "closed";

/// 宣言の祖先の材料の頭の 1 行（設計の材料の末尾に足す・設計 §3 形 3）。
const ANCESTOR_NOTE: &str = "次の行は未着地の祖先で、その write-set の file はこの祖先が作る・変える";

/// 祖先の節の本文が材料に既に在る本文と byte で同じとき、本文の代わりに足す 1 行（設計 §14）。
const SAME_BODY: &str = "（節の本文は上と同じ）";

/// 行の記録の file の名（置き場の行の記録の dir の中・読み手 [`super::row_review`] と同じ字）。
const RECORD: &str = "record";

/// 撃ち中の印を持つ間だけ生きる guard（Drop で印を外す＝落ちた周も外れる）。
struct Mark(PathBuf);

impl Drop for Mark {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 一時の木を置く dir を畳む guard（Drop で worktree の登録ごと外す・次の周の頭の片付けも同じ [`clear`]）。
struct Work<'a> {
    /// anchor の repo。
    repo: &'a Path,
    /// ref の dir の下の一時の dir。
    dir: PathBuf,
}

impl Drop for Work<'_> {
    fn drop(&mut self) {
        clear(self.repo, &self.dir);
    }
}

/// `dir` の下の worktree（`<dir>/<名>/<sha>.tree`）を登録ごと外し、dir を消す。
fn clear(repo: &Path, dir: &Path) {
    let slots: Vec<PathBuf> = std::fs::read_dir(dir).map(|found| found.flatten().map(|entry| entry.path()).collect()).unwrap_or_default();
    for slot in slots {
        let trees = std::fs::read_dir(&slot).map(|found| found.flatten().map(|entry| entry.path()).collect::<Vec<_>>()).unwrap_or_default();
        for tree in trees.iter().filter(|tree| tree.join(".git").is_file()) {
            let _ = git_ok(repo, &["worktree", "remove", "--force", &tree.display().to_string()]);
        }
    }
    let _ = std::fs::remove_dir_all(dir);
    let _ = git_ok(repo, &["worktree", "prune"]);
}

/// 口の引数（`--ref` は審査する commit・`--lens` は行ごとに撃つ lens の cmd）。
struct Inputs {
    /// anchor の repo。
    repo: PathBuf,
    /// 置き場。
    state: PathBuf,
    /// `--ref` の値。
    rev: String,
    /// lens の cmd。
    lens: String,
}

/// 引数を読む（欠けは使い方の誤り）。
fn inputs(args: &[String]) -> Result<Inputs, String> {
    let (repo, state) = (repo_of(args)?, state_dir_of(args)?);
    let need = |name: &str| flag(args, name)?.map(str::to_owned).ok_or(format!("{name} が要る"));
    Ok(Inputs { repo, state, rev: need("--ref")?, lens: need("--lens")? })
}

/// 口の入口（設計 §3）: `--ref` の変わった行ごとに審査して記録を書く。
pub fn review(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let found = match inputs(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let Some(sha) = git_line(&found.repo, &["rev-parse", "--verify", &format!("{}^{{commit}}", found.rev)]) else {
        return broken(format!("--ref {} を解けない", found.rev));
    };
    let mark = match take_mark(&found.state, &sha) {
        Ok(Some(mark)) => mark,
        Ok(None) => return Outcome { out: vec![format!("{LINE} result=pending ref={sha}")], err: Vec::new(), rc: RC_REFUSED },
        Err(reason) => return broken(reason),
    };
    sweep(&found.repo, &found.state, &sha);
    let outcome = shoot(&found, (args, manifest, policy), &sha).unwrap_or_else(broken);
    drop(mark);
    outcome
}

/// 撃ち中の印を置く（持ち主が生きている印が在れば `None`＝待たずに pending・死んだ印は 1 回だけ外して取り直す）。
fn take_mark(state: &Path, sha: &str) -> Result<Option<Mark>, String> {
    let path = mark_path(state, sha);
    let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
    std::fs::create_dir_all(&parent).map_err(|err| format!("{} を作れない: {err}", parent.display()))?;
    for _ in 0..2 {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let pid = std::process::id();
                let body = started_ms(pid).started().map_or_else(|| format!("{pid}\n"), |at| format!("{pid} {at}\n"));
                let mark = Mark(path.clone());
                file.write_all(body.as_bytes()).map_err(|err| format!("撃ち中の印を書けない: {err}"))?;
                return Ok(Some(mark));
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let dead = std::fs::read_to_string(&path).is_ok_and(|body| lock_owner(&body, started_ms) == Owner::Dead);
                if !dead || std::fs::remove_file(&path).is_err() {
                    return Ok(None);
                }
            }
            Err(err) => return Err(format!("撃ち中の印を置けない: {err}")),
        }
    }
    Ok(None)
}

/// 落ちた周の一時の木の残りを外す（印の持ち主が生きている別の sha の dir は触らない）。
fn sweep(repo: &Path, state: &Path, own: &str) {
    let dir = root_of(state).join(REF_DIR);
    let entries: Vec<PathBuf> = std::fs::read_dir(&dir).map(|found| found.flatten().map(|entry| entry.path()).collect()).unwrap_or_default();
    for path in entries {
        let Some(sha) = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_suffix(".work")) else {
            continue;
        };
        let alive = std::fs::read_to_string(mark_path(state, sha)).is_ok_and(|body| lock_owner(&body, started_ms) != Owner::Dead);
        if sha == own || !alive {
            clear(repo, &path);
        }
    }
}

/// 契約表の 1 行（`<doc>#<行 id>` と、`--ref` の木で受付と同じ生成が作った契約）と digest。
struct Row {
    /// 設計 pointer。
    pointer: Pointer,
    /// 行の digest（[`row_digest`]）。
    digest: String,
    /// 生成した契約と file の字（受付の生成が断った行は `None`）。
    made: Option<(Contract, String)>,
}

impl Row {
    /// `<doc>#<行 id>`。
    fn key(&self) -> String {
        key_of(&self.pointer)
    }
}

/// 設計 pointer の字（`<doc>#<行 id>`）。
fn key_of(pointer: &Pointer) -> String {
    format!("{}#{}", pointer.path, pointer.id)
}

/// 末尾の改行を 1 つ除く（便の置き場の写しから digest を求める形と同じ）。
fn trimmed(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

/// 受付の生成が断った行の「断りの 1 行」（型の断りは先頭の理由・型を持たない断りは断りの名）。
fn refusal_line(denial: &Denial) -> String {
    denial.refusals.first().map_or_else(|| denial.name.to_owned(), |found| format!("{}: {}", found.label(), found.reason().replace('\n', " ")))
}

/// 木の 1 行の digest と生成した契約（受付と同じ生成 → 断った行は契約 file の字の代わりに断りの 1 行・節の本文は審査の材料と同じ読み手）。
fn row_state(tree: &Path, materials: &Materials, pointer: &Pointer) -> (String, Option<(Contract, String)>) {
    let section = design_material(tree, &key_of(pointer));
    match generated(tree, pointer, materials) {
        Ok(made) => (row_digest(trimmed(&made.1), trimmed(&section)), Some(made)),
        Err(denial) => (row_digest(&refusal_line(&denial), trimmed(&section)), None),
    }
}

/// 契約表を持つ file か（受付と同じ表の読み手が行を返す file・区間の marker を持つ file）。
fn bears_table(path: &str, text: &str) -> bool {
    let marker = if path.ends_with(".md") { BEGIN } else { "[[contract]]" };
    text.contains(marker) && read_rows(path, text).map_or(true, |rows| !rows.is_empty())
}

/// 木の契約表の file（path の順・本文つき）。
fn table_files(tree: &Path) -> Result<Vec<(String, String)>, String> {
    let tracked = tracked_files(tree).ok_or_else(|| format!("{} の tracked file を読めない", tree.display()))?;
    let texts = tracked.into_iter().filter(|path| form_of(path).is_ok()).filter_map(|path| Some((std::fs::read_to_string(tree.join(&path)).ok()?, path)));
    Ok(texts.filter(|(text, path)| bears_table(path, text)).map(|(text, path)| (path, text)).collect())
}

/// 契約表の file の行の pointer の列（表を読めない file は理由）。
fn pointers_of(files: &[(String, String)]) -> Result<Vec<Pointer>, String> {
    let mut found = Vec::new();
    for (path, text) in files {
        let rows = read_rows(path, text).map_err(|errors| format!("{path} を読めない: {}", errors.iter().map(|error| error.reason()).collect::<Vec<_>>().join(" / ")))?;
        found.extend(rows.into_iter().map(|row| Pointer { path: path.clone(), id: row.id }));
    }
    Ok(found)
}

/// 1 回の口が持つ材料（木・台帳・規則・lens）。
struct Ctx<'a> {
    /// 引数。
    inputs: &'a Inputs,
    /// 審査する commit の 40 桁。
    sha: &'a str,
    /// 規則の値。
    manifest: &'a Manifest,
    /// 表の木（`--ref` の commit の detach の木）。
    table: &'a Path,
    /// 表の木から 1 回読んだ base の材料。
    base: &'a Materials,
    /// 台帳（変わった行がある周だけ読む）。
    issues: &'a [Issue],
    /// 一時の木を置く dir。
    work: &'a Path,
    /// code の木の鍵。
    code: &'a str,
    /// 受付の材料（lens の同時の本数を host の枠で絞る）。
    rules: admission::Rules,
    /// lens の口座の選定の入力（宣言が無い周は `None`）。
    pool: Option<Pool>,
}

/// 変わった行（`--ref` の digest が merge-base の digest と違う行と、merge-base に無い行）。
fn changed(table: (&Path, &Materials), base: (&Path, Option<&Materials>), files: &[(String, String)]) -> Result<Vec<Row>, String> {
    let before = pointers_of(&table_files(base.0).unwrap_or_default()).unwrap_or_default();
    let mut rows = Vec::new();
    for pointer in pointers_of(files)? {
        let (digest, made) = row_state(table.0, table.1, &pointer);
        let known = before.iter().any(|found| key_of(found) == key_of(&pointer));
        let old = base.1.filter(|_| known).map(|found| row_state(base.0, found, &pointer).0);
        if old.as_deref() != Some(digest.as_str()) {
            rows.push(Row { pointer, digest, made });
        }
    }
    Ok(rows)
}

/// 行を指す bead が 1 本以上在って全部 closed か（着地済み・bead の pointer は台帳の lint と同じ読み手）。
fn landed(issues: &[Issue], key: &str) -> bool {
    let beads: Vec<&Issue> = issues.iter().filter(|issue| !is_memo(issue) && !is_question(issue) && pointer_of(issue) == Some(key)).collect();
    !beads.is_empty() && beads.iter().all(|issue| issue.status == CLOSED)
}

/// 1 行の審査の結果（ref の記録の row の行と stdout の行の材料）。
struct Decided {
    /// `<doc>#<行 id>`。
    key: String,
    /// 行の digest。
    digest: String,
    /// 行の記録の dir の名。
    name: String,
    /// 判定。
    verdict: Verdict,
    /// 審査の basis。
    basis: Basis,
    /// 理由の型が unparsed か（pass の数え方が読む）。
    unparsed: bool,
}

/// 口の本体: 木を作り、変わった行を決め、行ごとに審査して ref の記録を書く。
fn shoot(found: &Inputs, run: (&[String], &Manifest, LockPolicy), sha: &str) -> Result<Outcome, String> {
    let (args, manifest, policy) = run;
    let work = Work { repo: &found.repo, dir: root_of(&found.state).join(REF_DIR).join(format!("{sha}.work")) };
    std::fs::create_dir_all(&work.dir).map_err(|err| format!("{} を作れない: {err}", work.dir.display()))?;
    let base_sha = git_line(&found.repo, &["merge-base", sha, MAIN_REF]).ok_or_else(|| format!("{MAIN_REF} との merge-base を読めない"))?;
    let tree = |slot: &str, rev: &str| Worktree::make(&found.repo, &work.dir.join(slot), rev).ok_or_else(|| format!("{rev} の木を作れない"));
    let (table, base_tree) = (tree("table", sha)?, tree("base", &base_sha)?);
    let refused_as = |denial: Denial| refusal_line(&denial);
    let base = Materials::of(&table.path, manifest, DEFAULT_BD).map_err(refused_as)?;
    let files = table_files(&table.path)?;
    let before = Materials::of(&base_tree.path, manifest, DEFAULT_BD).ok();
    let mut rows = changed((&table.path, &base), (&base_tree.path, before.as_ref()), &files)?;
    let issues = if rows.is_empty() { Vec::new() } else { read_issues(&found.repo, manifest)? };
    rows.retain(|row| !landed(&issues, &row.key()));
    let (limits, code) = (Limits::of(manifest)?, tree_key(&found.repo, sha)?);
    let ctx = Ctx {
        inputs: found,
        sha,
        manifest,
        table: &table.path,
        base: &base,
        issues: &issues,
        work: &work.dir,
        code: &code,
        rules: limits.admission(policy),
        pool: Pool::declared(args, manifest, &found.state)?,
    };
    let (decided, notes) = judge_rows(&ctx, &rows)?;
    let result = result_of(&decided);
    let tables: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
    write_ref(&found.state, sha, (&base_sha, &tables), (&decided, result))?;
    let mut out: Vec<String> = decided.iter().map(|row| format!("{LINE} row={} verdict={} basis={} record={}", row.key, row.verdict.as_str(), row.basis.as_str(), row.name)).collect();
    out.push(format!("{LINE} result={result} ref={sha}"));
    Ok(Outcome { out, err: notes, rc: if result == "pass" { RC_OK } else { RC_REFUSED } })
}

/// 台帳を読みだけで読む（読めない周は理由＝着地済みかを測れない行を撃つ側にも外す側にも倒さない）。
fn read_issues(repo: &Path, manifest: &Manifest) -> Result<Vec<Issue>, String> {
    let timeout = timeout_of(manifest).ok_or("台帳の待ち上限の行 seat.ledger_timeout_s を読めない".to_owned())?;
    read_ledger(DEFAULT_BD, repo, timeout).map_err(|err| format!("台帳を読めない（{err:?}）"))
}

/// 変わった行の全部の祖先の層を、lens より先に組んでから行ごとに審査する（祖先を組めない行が 1 本でも在れば何も撃たない）。
fn judge_rows(ctx: &Ctx<'_>, rows: &[Row]) -> Result<(Vec<Decided>, Vec<String>), String> {
    let trees = (ctx.inputs.repo.as_path(), ctx.table);
    let found: Vec<Result<Ancestry, String>> = rows.iter().map(|row| ancestry(trees, &ctx.inputs.state, ctx.issues, ctx.base, &row.key())).collect();
    let failed: Vec<String> = rows.iter().zip(&found).filter_map(|(row, found)| Some(format!("{}: {}", row.key(), found.as_ref().err()?))).collect();
    if !failed.is_empty() {
        return Err(format!("祖先の層を組めない（{}）", failed.join(" / ")));
    }
    let (mut decided, mut notes) = (Vec::new(), Vec::new());
    for (n, (row, found)) in rows.iter().zip(found).enumerate() {
        let (one, note) = judge_row(ctx, n, row, found?)?;
        decided.push(one);
        notes.extend(note);
    }
    Ok((decided, notes))
}

/// 判定の鍵の材料のうち撃つ前に決まる 4 つ（材料の鍵と版は撃つ側が足す）。
struct Known {
    /// 行の digest。
    digest: String,
    /// code の木の鍵。
    tree: String,
    /// 審査の basis。
    basis: Basis,
    /// 祖先ごとの `<行>:<状態の語>`。
    ancestors: Vec<String>,
}

impl Known {
    /// 判定の鍵と行の記録の dir の名（[`judgement`] の 1 本）。
    fn name(&self, key: &str, keyed: (&str, &str)) -> (String, String) {
        let parts = Parts { digest: &self.digest, materials: keyed.0, tree: &self.tree, basis: self.basis, ancestors: &self.ancestors, version: keyed.1 };
        judgement(key, &parts)
    }
}

/// 行 1 つを審査して記録を書く（確定の機械の検査が在れば lens を撃たず FAIL・撃てない行は INCONCLUSIVE の unparsed）。
fn judge_row(ctx: &Ctx<'_>, n: usize, row: &Row, found: Ancestry) -> Result<(Decided, Option<String>), String> {
    let (layers, basis) = found;
    let ancestors = layers.iter().map(|(id, standing, _)| format!("{id}:{}", standing.word())).collect();
    let known = Known { digest: row.digest.clone(), tree: ctx.code.to_owned(), basis, ancestors };
    let (made, findings) = forecast_findings(ctx.table, ctx.manifest, ctx.base, &row.pointer, &layers);
    let firm = findings.iter().find(|found| found.certainty == Certainty::Firm);
    let judged = match firm {
        Some(found) => {
            let reason = format!("確定の機械の検査 {} {} {}", found.name, found.at, found.reason);
            Judged { mech: format!("firm:{}", found.name), ..Judged::stopped(Verdict::Fail, None, Some(reason)) }
        }
        None => lens_row(ctx, (n, row), &known, (&layers, made, &findings)),
    };
    let key = row.key();
    let name = write_record(ctx, &key, &known, &judged)?;
    let unparsed = judged.kind == Some(FindingKind::Unparsed);
    let note = judged.reason.map(|reason| format!("pipe: {key}: {reason}"));
    Ok((Decided { key, digest: row.digest.clone(), name, verdict: judged.verdict, basis, unparsed }, note))
}

/// 撃って決まった判定（記録の材料）。
struct Judged {
    /// `clean` か `firm:<断りの名>`。
    mech: String,
    /// 判定。
    verdict: Verdict,
    /// 理由の型（PASS と確定の機械の検査の FAIL は `None`）。
    kind: Option<FindingKind>,
    /// 材料の鍵（材料を組まなかった周は `-`）。
    materials: String,
    /// lens の版の 1 行（求めなかった周は `-`）。
    version: String,
    /// lens の消費。
    usage: Option<Usage>,
    /// lens の rc と stdout（撃った周だけ）。
    ran: Option<(Option<i32>, String)>,
    /// 記録の dir に写す材料の dir（撃った周だけ）。
    staged: Option<PathBuf>,
    /// 同じ鍵の記録を写した周（記録を書き直さない）。
    reused: bool,
    /// 撃てなかった理由（stderr の 1 行）。
    reason: Option<String>,
}

impl Judged {
    /// lens を撃たずに決めた判定。
    fn stopped(verdict: Verdict, kind: Option<FindingKind>, reason: Option<String>) -> Self {
        Self { mech: "clean".to_owned(), verdict, kind, materials: "-".to_owned(), version: "-".to_owned(), usage: None, ran: None, staged: None, reused: false, reason }
    }

    /// 撃てなかった周（INCONCLUSIVE・理由の型は unparsed）。
    fn unmeasured(reason: String) -> Self {
        Self::stopped(Verdict::Inconclusive, Some(FindingKind::Unparsed), Some(reason))
    }

    /// 材料を組んだ後に撃てなかった周（材料の鍵と版は記録に残す）。
    fn unfired(self, reason: String, staged: &Staged) -> Self {
        Self { staged: Some(staged.dir.clone()), reason: Some(reason), verdict: Verdict::Inconclusive, kind: Some(FindingKind::Unparsed), ..self }
    }
}

/// 機械の検査を通った行の lens の審査（材料を組み・版を求め・同じ鍵の記録が在れば写し・無ければ撃つ）。
fn lens_row(ctx: &Ctx<'_>, at: (usize, &Row), known: &Known, staging: (&[Ancestor], Option<(Contract, String)>, &[Finding])) -> Judged {
    let (n, row) = at;
    let staged = match stage_row(ctx, (n, row), staging) {
        Ok(found) => found,
        Err(reason) => return Judged::unmeasured(reason),
    };
    let line = crate::headless::fill(&ctx.inputs.lens, &[("{contract}", &staged.contract.display().to_string()), ("{worktree}", &staged.tree.path.display().to_string())]);
    let mut base = Judged { materials: staged.materials.clone(), ..Judged::stopped(Verdict::Pass, None, None) };
    match lens_version(&line) {
        Ok(found) => base.version = found,
        Err(reason) => return base.unfired(reason, &staged),
    }
    let kept = kept_verdict(&ctx.inputs.state, &known.name(&row.key(), (&base.materials, &base.version)).1);
    match kept {
        Some(verdict) => Judged { verdict, reused: true, ..base },
        None => fire(ctx, (&row.pointer, &staged, line), base),
    }
}

/// 審査の木と材料（lens が読む）。
struct Staged {
    /// 審査の木（Drop で登録ごと外れる）。
    tree: Worktree,
    /// 材料の dir。
    dir: PathBuf,
    /// lens に渡す契約の写しの path。
    contract: PathBuf,
    /// 材料の鍵。
    materials: String,
    /// 契約の done の字。
    done: String,
    /// 約束の行を持つか。
    promised: bool,
}

/// 審査の木を置き、実物の祖先の差分を重ね、Reviewed と同じ組み手で材料を組む。
fn stage_row(ctx: &Ctx<'_>, at: (usize, &Row), staging: (&[Ancestor], Option<(Contract, String)>, &[Finding])) -> Result<Staged, String> {
    let (n, row) = at;
    let (layers, made, findings) = staging;
    let (contract, body) = made.or_else(|| row.made.clone()).ok_or("契約を生成できない（受付の生成が断った行）")?;
    let slot = ctx.work.join(format!("row{n}"));
    std::fs::create_dir_all(&slot).map_err(|err| format!("{} を作れない: {err}", slot.display()))?;
    let tree = Worktree::make(&ctx.inputs.repo, &slot, ctx.sha).ok_or_else(|| format!("審査の木 {} を作れない", Worktree::place(&slot, ctx.sha).display()))?;
    let actual: Vec<Layer> = layers.iter().filter(|(_, standing, _)| *standing == Standing::Tree).map(|(_, _, layer)| layer.clone()).collect();
    materialize(&tree.path, &actual)?;
    let requirements = requirements_of(&tree.path, ctx.manifest)?;
    let source = slot.join("contract.toml");
    std::fs::write(&source, &body).map_err(|err| format!("{} を書けない: {err}", source.display()))?;
    let dir = slot.join(REVIEW_DIR);
    let note = note_of(ctx, &design_material(&tree.path, &contract.design), layers, findings);
    let (copy, promised) = stage(&tree.path, (&contract, &source), &requirements, &dir, &note)?;
    Ok(Staged { tree, materials: digest(&dir)?, dir, contract: copy, done: contract.done, promised })
}

/// 設計の材料の末尾に足す字: 宣言の祖先ごとに「未着地の祖先」の 1 行・祖先の行の TOML の写し・祖先の節の本文、続けて確定でない
/// finding（暫定と測れない）の断りの名・在り処・理由を 1 行ずつ。祖先の節の本文が材料に既に在る本文（`own` は行自身の設計の材料・先に足した祖先の
/// 本文）と byte で同じなら、本文の代わりに見出しの 1 行と [`SAME_BODY`] の 1 行だけを足す（設計 §14）。
fn note_of(ctx: &Ctx<'_>, own: &str, layers: &[Ancestor], findings: &[Finding]) -> String {
    let mut lines = Vec::new();
    let mut seen: Vec<String> = own.trim_end().split_once('\n').map(|(_, body)| body.to_owned()).into_iter().collect();
    for (id, _, _) in layers.iter().filter(|(_, standing, _)| *standing == Standing::Declared) {
        lines.push(ANCESTOR_NOTE.to_owned());
        lines.extend(row_toml(ctx.table, id));
        let section = design_material(ctx.table, id).trim_end().to_owned();
        match section.split_once('\n') {
            Some((head, body)) if seen.iter().any(|known| known == body) => {
                lines.push(head.to_owned());
                lines.push(SAME_BODY.to_owned());
            }
            Some((_, body)) => {
                seen.push(body.to_owned());
                lines.push(section);
            }
            None => lines.push(section),
        }
    }
    for found in findings.iter().filter(|found| found.certainty != Certainty::Firm) {
        lines.push(format!("予想の base の断り（{}）: {} {} {}", found.certainty.as_str(), found.name, found.at, found.reason));
    }
    lines.join("\n")
}

/// 表の file の字のうち、行の見出しの行から次の行の見出しか表の終わりの前まで（末尾の空白は落とす）。
fn row_toml(table: &Path, key: &str) -> Option<String> {
    let pointer = parse_pointer(key).ok()?;
    let text = std::fs::read_to_string(table.join(&pointer.path)).ok()?;
    let start = read_rows(&pointer.path, &text).ok()?.into_iter().find(|row| row.id == pointer.id)?.line;
    let from = usize::try_from(start.saturating_sub(1)).ok()?;
    let mut lines = text.lines().skip(from);
    let head = lines.next()?;
    let rest = lines.take_while(|line| !matches!(line.trim(), "[[contract]]" | END));
    Some(std::iter::once(head).chain(rest).collect::<Vec<&str>>().join("\n").trim_end().to_owned())
}

/// 同じ名の行の記録が在り、理由の型が unparsed でない周の判定。
fn kept_verdict(state: &Path, name: &str) -> Option<Verdict> {
    let text = std::fs::read_to_string(root_of(state).join(name).join(RECORD)).ok()?;
    let field = |key: &str| text.lines().skip(1).find_map(|line| line.strip_prefix(key)?.strip_prefix('='));
    (text.lines().next() == Some(SCHEMA_LINE) && field("kind") != Some("unparsed")).then(|| Verdict::parse(field("verdict")?)).flatten()
}

/// lens を 1 回撃つ（箱で包み・受付札を取り・口座を選び・審査の木を cwd にする・撃てない周は INCONCLUSIVE の unparsed）。
fn fire(ctx: &Ctx<'_>, at: (&Pointer, &Staged, String), base: Judged) -> Judged {
    let (pointer, staged, line) = at;
    let label = format!("row-review-{}-{}", ctx.sha.chars().take(12).collect::<String>(), pointer.id);
    let line = match with_account(ctx, line) {
        Ok(found) => found,
        Err(reason) => return base.unfired(reason, staged),
    };
    let _grant = admission::admit(&ctx.inputs.state, &label, 1, &ctx.rules);
    let unit = confine::unit_name(&label, "review", 1);
    let wrap = confine::Wrap { unit: &unit, limit: confine::Limit::PerJob(1), caps: confine::Caps::embedded(), width: None };
    let (mut command, confinement) = confine::wrap_line(&line, &wrap);
    let spawned = command.current_dir(&staged.tree.path).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return base.unfired(format!("lens を起動できない: {err}"), staged),
    };
    drop(child.stdin.take());
    let waited = child.wait_with_output();
    let ran = waited.as_ref().ok().map(|out| (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned()));
    let lensed = read_lens(waited, &confinement, (&staged.done, staged.promised), &staged.contract);
    let reason = (lensed.verdict != Verdict::Pass).then_some(lensed.evidence);
    Judged { verdict: lensed.verdict, kind: lensed.kind, usage: lensed.usage, ran, staged: Some(staged.dir.clone()), reason, ..base }
}

/// 選んだ口座を lens の起動行の末尾に足す（宣言が無い周は行を変えない・選べない周は理由）。
fn with_account(ctx: &Ctx<'_>, line: String) -> Result<String, String> {
    let Some(pool) = &ctx.pool else {
        return Ok(line);
    };
    let state = &ctx.inputs.state;
    match select_lens_account(pool, state, &ctx.inputs.repo, &mut Vec::new()) {
        Ok(LensAccount::Chosen(label)) => super::spawn::with_account(line, Some(&label), state).map_err(|refusal| format!("lens の{refusal}")),
        Ok(LensAccount::None(reason)) => Err(format!("lens の口座の候補が無い（account:none={reason}）")),
        Err(reason) => Err(format!("lens の口座を選べない（{reason}）")),
    }
}

/// 行の記録を書く（同じ鍵の記録を写した周は書き直さない・`record` は最後に一時 file → rename）。dir の名を返す。
fn write_record(ctx: &Ctx<'_>, key: &str, known: &Known, judged: &Judged) -> Result<String, String> {
    let (id, name) = known.name(key, (&judged.materials, &judged.version));
    if judged.reused {
        return Ok(name);
    }
    let dir = root_of(&ctx.inputs.state).join(&name);
    let put = |path: PathBuf, body: String| std::fs::write(&path, body).map_err(|err| format!("{} を書けない: {err}", path.display()));
    std::fs::create_dir_all(&dir).map_err(|err| format!("{} を作れない: {err}", dir.display()))?;
    if let Some((rc, out)) = &judged.ran {
        put(dir.join("rc"), format!("{}\n", rc.unwrap_or(-1)))?;
        put(dir.join("out"), out.clone())?;
    }
    if let Some(from) = &judged.staged {
        copy_files(from, &dir.join(REVIEW_DIR))?;
    }
    let usage = judged.usage.map_or_else(|| "-".to_owned(), |found| format!("{},turns:{},wall_ms:{}", found.tokens(), found.turns, found.wall_ms));
    let kind = judged.kind.map_or("-", FindingKind::as_str);
    let ancestors = super::row_review::ancestors_word(&known.ancestors);
    let at = crate::seat::state::now_secs();
    let fields = [
        ("row", key), ("digest", &known.digest), ("key", &id), ("basis", known.basis.as_str()), ("ancestors", &ancestors), ("mech", &judged.mech),
        ("verdict", judged.verdict.as_str()), ("kind", kind), ("materials", &judged.materials), ("tree", &known.tree), ("version", &judged.version),
        ("ref", ctx.sha), ("at", &at.to_string()), ("usage", &usage),
    ];
    let body: String = fields.iter().map(|(name, value)| format!("{name}={}\n", value.replace('\n', " "))).collect();
    let partial = dir.join(format!("{RECORD}.{}.tmp", std::process::id()));
    put(partial.clone(), format!("{SCHEMA_LINE}\n{body}"))?;
    std::fs::rename(&partial, dir.join(RECORD)).map_err(|err| format!("{} を置けない: {err}", dir.join(RECORD).display()))?;
    Ok(name)
}

/// 材料の dir の file を写す（dir は 1 段・既に在れば置き換える）。
fn copy_files(from: &Path, to: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(to);
    std::fs::create_dir_all(to).map_err(|err| format!("{} を作れない: {err}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|err| format!("{} を読めない: {err}", from.display()))?.flatten() {
        std::fs::copy(entry.path(), to.join(entry.file_name())).map_err(|err| format!("{} を写せない: {err}", entry.path().display()))?;
    }
    Ok(())
}

/// ref の結果（設計 §3 形 9）: 全行が PASS か、INCONCLUSIVE の行が全部 basis が forecast か partial で理由の型が unparsed でなければ pass。
fn result_of(rows: &[Decided]) -> &'static str {
    let passes = |row: &Decided| row.verdict == Verdict::Pass || (row.verdict == Verdict::Inconclusive && row.basis != Basis::Actual && !row.unparsed);
    if rows.iter().all(passes) { "pass" } else { "fail" }
}

/// ref の記録を書く（一時 file → rename・1 行目 schema・base・tables・変わった行ごとの row の行・最後に result）。
fn write_ref(state: &Path, sha: &str, base: (&str, &[&str]), rows: (&[Decided], &str)) -> Result<(), String> {
    let mut body = format!("{SCHEMA_LINE}\nbase={}\ntables={}\n", base.0, base.1.join(","));
    for row in rows.0 {
        body.push_str(&format!("row={} digest={} id={} verdict={} basis={}\n", row.key, row.digest, row.name, row.verdict.as_str(), row.basis.as_str()));
    }
    body.push_str(&format!("result={}\n", rows.1));
    let path = ref_path(state, sha);
    let partial = path.with_extension(format!("{}.tmp", std::process::id()));
    let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
    std::fs::create_dir_all(&parent).map_err(|err| format!("{} を作れない: {err}", parent.display()))?;
    std::fs::write(&partial, body).map_err(|err| format!("{} を書けない: {err}", partial.display()))?;
    std::fs::rename(&partial, &path).map_err(|err| format!("{} を置けない: {err}", path.display()))
}
