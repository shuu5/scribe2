//! 事前審査（設計 docs/design/dispatcher.md §27・契約表の行 x）: 依存を待つ行に受付の判定（[`generated`] → [`judge`]）を
//! **予想の base**（未着地の祖先の宣言か、Gated PASS の祖先の実物を重ねた木）で先に撃ち、断りを確定 / 暫定 / 測れないに
//! 分けて置き場の file に残す。
//!
//! 撃つのは起こす側の周（[`super::fire`]）の起こし終えた後だけで、台帳と材料は同じ周に [`super::turn`] の読みが持った
//! 1 回を借りる（形 4）。結果は `WaitReason`・起こす判定・受付のどれも読まない（**予想は通行証にしない**・形 6）。見る側
//! （`dispatch ls`）は file を読むだけで、母集団の 1 関数も呼ばない（[`lines`]）。event kind は足さない（形 5）。

use super::super::cli::{generated, judge, live, Denial, Material, Materials};
use super::super::closure::Source;
use super::super::contract::Contract;
use super::super::gate::Verdict;
use super::super::land::verdict_of;
use super::super::refuse::{discern, normalize, Certainty, DELETE_FILE, NEW_FILE};
use super::super::table::Pointer;
use super::super::{base_of_run, contract_path, current, git_bytes, head_of, worktree_path, DIR};
use super::candidates::{is_blocking, pointer_of};
use super::{Input, Turn, WaitReason, CLOSED, DASH, MEMO_LABEL};
use crate::fleet::{Stage, State};
use crate::seat::ledger::Issue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 事前審査の dir の名（置き場の `pipe/` の下・bead ごとの 1 file・形 5）。
const PRECHECK_DIR: &str = "precheck";

/// `dispatch ls` の事前審査の行の書き出し（形 7）。
const LINE: &str = "[DISPATCH-PRECHECK]";

/// 閉じていない契約の行 1 つ（形 1 (a)）。
pub(super) struct Row {
    /// 設計 pointer（列と同じ読み [`pointer_of`]）。
    pub(super) pointer: Pointer,
    /// live な便（id・段・run dir の契約の写しの write-set〔読めない周は `None`〕）。
    pub(super) live: Option<(String, Stage, Option<Vec<String>>)>,
}

/// 母集団と到達（形 1）。
pub(super) struct Population {
    /// 閉じていない契約の行（bead id の順）。
    pub(super) rows: BTreeMap<String, Row>,
    /// 行ごとの blocks の到達（**依存の順**＝祖先が先・自分は含まない・契約の行でない bead も含む）。
    pub(super) reach: BTreeMap<String, Vec<String>>,
}

/// 母集団と到達の 1 関数（形 1）: 同じ周の台帳の全件と置き場の run の列から、(a) 閉じていない契約の行（closed でない ∧ memo の
/// label が無い ∧ acceptance の設計 pointer が列と同じ [`pointer_of`] で解ける bead・live な便の在る bead はその run dir の契約の
/// 写しの write-set つき）と、(b) blocks の推移の到達（[`is_blocking`] の依存だけ＝`parent-child` は数えず closed で止まる・
/// 1 度訪ねた bead で止まり循環で回らない）を返す。live でない行の write-set は呼び手が自分の材料で [`generated`] を撃って決める。
pub(super) fn population(issues: &[Issue], state_dir: &Path, state: &State) -> Population {
    let closed: BTreeSet<&str> = issues.iter().filter(|issue| issue.status == CLOSED).map(|issue| issue.id.as_str()).collect();
    let rows: BTreeMap<String, Row> = issues
        .iter()
        .filter(|issue| issue.status != CLOSED && !issue.labels.iter().any(|label| label == MEMO_LABEL))
        .filter_map(|issue| {
            let row = Row { pointer: pointer_of(&issue.acceptance)?, live: live_of(state_dir, state, &issue.id) };
            Some((issue.id.clone(), row))
        })
        .collect();
    let by_id: BTreeMap<&str, &Issue> = issues.iter().map(|issue| (issue.id.as_str(), issue)).collect();
    let reach = rows
        .keys()
        .map(|id| {
            let (mut seen, mut order) = (BTreeSet::from([id.clone()]), Vec::new());
            visit(&by_id, &closed, id, &mut seen, &mut order);
            (id.clone(), order)
        })
        .collect();
    Population { rows, reach }
}

/// blocks の依存を深さ優先でたどり、帰りがけに積む（依存が先に並ぶ＝依存の順）。
fn visit(by_id: &BTreeMap<&str, &Issue>, closed: &BTreeSet<&str>, bead: &str, seen: &mut BTreeSet<String>, order: &mut Vec<String>) {
    let Some(issue) = by_id.get(bead) else {
        return;
    };
    for dep in issue.deps.iter().filter(|dep| is_blocking(dep, closed)) {
        if seen.insert(dep.on.clone()) {
            visit(by_id, closed, &dep.on, seen, order);
            order.push(dep.on.clone());
        }
    }
}

/// bead の live な便（新しい id の側・生死は受付と同じ [`live`] の 1 本）と、その run dir の契約の写しの write-set。
fn live_of(state_dir: &Path, state: &State, bead: &str) -> Option<(String, Stage, Option<Vec<String>>)> {
    let (id, run) = state.runs.iter().rev().find(|(id, run)| run.bead == bead && live(state_dir, id, run.stage) == Some(true))?;
    let write_set = Contract::load(&contract_path(state_dir, id)).ok().map(|found| found.write_set);
    Some((id.clone(), run.stage, write_set))
}

/// 祖先 1 つの重ね方（形 1）。
enum Layer {
    /// 宣言の予想: write-set の `+` を tracked に足し `~` を除く（本文は持たない）。項目は動く file に入る。
    Declared(Vec<String>),
    /// Gated PASS の便の実物: worktree の base..HEAD の差分で tracked を足し引きし、本文を置き換える（動く file に入れない）。
    Tree {
        /// 足した / 変えた file。
        add: Vec<String>,
        /// 消した file（rename の元を含む）。
        remove: Vec<String>,
        /// 本文の読み手が読む拡張子の file の本文（A の木の HEAD から）。
        bodies: Vec<Source>,
    },
}

/// 1 周に固定な材料（候補ごとに読み直さない）。
struct Ctx<'a, 'b> {
    /// 列の材料。
    input: &'a Input<'b>,
    /// 母集団と到達。
    population: &'a Population,
    /// 同じ周に `turn` が読んだ base の材料。
    base: &'a Materials,
}

/// Gated PASS の便（live ∧ `Gated` ∧ verdict PASS・verdict の読みは着地の段と同じ [`verdict_of`]）なら run id。
fn tree_run<'a>(input: &Input<'_>, row: &'a Row) -> Option<&'a str> {
    let (run, stage, _) = row.live.as_ref()?;
    (*stage == Stage::Gated && verdict_of(input.state_dir, run) == Some(Verdict::Pass)).then_some(run.as_str())
}

/// 祖先ごとの状態の語（鍵の材料・形 4）: `declared`／`run:<便>`／`tree:<便>@<worktree の HEAD の sha>`。
fn state_word(input: &Input<'_>, row: &Row) -> String {
    match (tree_run(input, row), row.live.as_ref()) {
        (Some(run), _) => format!("tree:{run}@{}", head_of(&worktree_path(input.repo, run)).unwrap_or_else(|| DASH.to_owned())),
        (None, Some((run, ..))) => format!("run:{run}"),
        (None, None) => "declared".to_owned(),
    }
}

/// bead の open な祖先（到達のうち閉じていない契約の行だけ・依存の順）。
fn ancestors_of(population: &Population, bead: &str) -> Vec<String> {
    let reach = population.reach.get(bead).map(Vec::as_slice).unwrap_or_default();
    reach.iter().filter(|id| population.rows.contains_key(*id)).cloned().collect()
}

/// 祖先 1 つの重ね方を決めて memo に置く（決まらない周は `None`＝生成が断る・写しを読めない・差分を読めない）。
///
/// live な便は写しの write-set（受付が凍結した値）か、Gated PASS なら実物。live でない行は自分の祖先を重ねた予想の base で
/// [`generated`] を撃った契約の write-set。先に `None` を置くので、循環は決まらない側に倒れて回らない。
fn resolve(ctx: &Ctx<'_, '_>, bead: &str, memo: &mut BTreeMap<String, Option<Layer>>) {
    if memo.contains_key(bead) {
        return;
    }
    memo.insert(bead.to_owned(), None);
    let Some(row) = ctx.population.rows.get(bead) else {
        return;
    };
    let layer = match (tree_run(ctx.input, row), row.live.as_ref()) {
        (Some(run), _) => tree_of(ctx.input, run),
        (None, Some((_, _, copied))) => copied.clone().map(Layer::Declared),
        (None, None) => {
            let ancestors = ancestors_of(ctx.population, bead);
            for id in &ancestors {
                resolve(ctx, id, memo);
            }
            layers(&ancestors, memo).and_then(|found| {
                let (materials, _) = overlay(ctx.base, &found);
                generated(ctx.input.repo, &row.pointer, &materials).ok().map(|(contract, _)| Layer::Declared(contract.write_set))
            })
        }
    };
    memo.insert(bead.to_owned(), layer);
}

/// 祖先の重ね方の列（1 つでも決まらなければ `None`）。
fn layers<'a>(ancestors: &[String], memo: &'a BTreeMap<String, Option<Layer>>) -> Option<Vec<&'a Layer>> {
    ancestors.iter().map(|id| memo.get(id).and_then(Option::as_ref)).collect()
}

/// Gated PASS の便の実物（worktree の base..HEAD の name-status・rename は対）。
fn tree_of(input: &Input<'_>, run: &str) -> Option<Layer> {
    let worktree = worktree_path(input.repo, run);
    let (base, head) = (base_of_run(input.state_dir, run).known()?, head_of(&worktree)?);
    let range = format!("{base}..{head}");
    let text = String::from_utf8(git_bytes(&worktree, &["diff", "--name-status", "-z", "-M", &range])?).ok()?;
    let (mut add, mut remove, mut bodies) = (Vec::new(), Vec::new(), Vec::new());
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    while let Some(status) = fields.next() {
        let first = fields.next()?.to_owned();
        let kept = match status.chars().next()? {
            'D' => {
                remove.push(first);
                continue;
            }
            'R' => {
                remove.push(first);
                fields.next()?.to_owned()
            }
            'C' => fields.next()?.to_owned(),
            _ => first,
        };
        if [".rs", ".snap"].iter().any(|ext| kept.ends_with(ext)) {
            let body = String::from_utf8(git_bytes(&worktree, &["show", &format!("{head}:{kept}")])?).ok()?;
            bodies.push(Source { path: kept.clone(), body: Ok(body) });
        }
        add.push(kept);
    }
    Some(Layer::Tree { add, remove, bodies })
}

/// 祖先を base に重ねた予想の材料と動く file（宣言で重ねた祖先の write-set の全項目・接頭辞を剥がし dir は base の tracked に展開）。
fn overlay(base: &Materials, layers: &[&Layer]) -> (Materials, Vec<String>) {
    let (mut add, mut remove, mut bodies, mut moving) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for layer in layers {
        match **layer {
            Layer::Declared(ref write_set) => {
                for item in write_set {
                    let path = normalize(item);
                    if item.starts_with(NEW_FILE) {
                        add.push(path.clone());
                    }
                    if item.starts_with(DELETE_FILE) {
                        remove.push(path.clone());
                    }
                    if path.ends_with('/') {
                        moving.extend(base.tracked().iter().filter(|file| file.starts_with(&path)).cloned());
                    } else {
                        moving.push(path);
                    }
                }
            }
            Layer::Tree { add: ref added, remove: ref removed, bodies: ref read } => {
                add.extend(added.iter().cloned());
                remove.extend(removed.iter().cloned());
                bodies.extend(read.iter().cloned());
            }
        }
    }
    (base.forecast(&add, &remove, &bodies), moving)
}

/// finding 1 つ（確からしさ・断りの名・在り処の字面・理由の 1 行）。
struct Finding {
    /// 確定 / 暫定 / 測れない。
    certainty: Certainty,
    /// 断りの名（型の断りは `Refuse::label`・型を持たない断りは材料の名）。
    name: String,
    /// 在り処の字面（型を持たない断りは `-`）。
    at: String,
    /// 理由の 1 行。
    reason: String,
}

/// 待ち行 1 つを予想の base で撃つ（形 2）: 祖先の重ね方が 1 つでも決まらなければ `None`（`unmeasured:forecast`）。
/// 撃つのは [`generated`] と [`judge`]（置き場なし・lock の前の読みなし＝列の候補の `blocker` と同じ形）だけで、置き場の要る
/// 判定と base の木の実走は撃たない。
fn judged(ctx: &Ctx<'_, '_>, row: &Row, ancestors: &[String], memo: &mut BTreeMap<String, Option<Layer>>) -> Option<Vec<Finding>> {
    for id in ancestors {
        resolve(ctx, id, memo);
    }
    let (materials, moving) = overlay(ctx.base, &layers(ancestors, memo)?);
    let input = ctx.input;
    let denials = match generated(input.repo, &row.pointer, &materials) {
        Err(denial) => vec![denial],
        Ok((contract, _)) => {
            let material = Material {
                repo: input.repo,
                manifest: input.manifest,
                contract: &contract,
                state_dir: None,
                bead: "",
                materials: &materials,
                early: None,
            };
            judge(&material).denials
        }
    };
    Some(denials.iter().flat_map(|denial| findings_of(denial, &moving)).collect())
}

/// 断り 1 つの finding（型の断りは在り処を弁別の 1 関数 [`discern`] に通す・型を持たない断りは測れない＝名を残す）。
fn findings_of(denial: &Denial, moving: &[String]) -> Vec<Finding> {
    if denial.refusals.is_empty() {
        let reason = "型を持たない断り（予想の base では測れない）".to_owned();
        return vec![Finding { certainty: Certainty::Unmeasured, name: denial.name.to_owned(), at: DASH.to_owned(), reason }];
    }
    let finding = |refuse: &super::super::refuse::Refuse| {
        let evidence = refuse.evidence();
        let reason = refuse.reason().replace('\n', " ");
        Finding { certainty: discern(&evidence, moving), name: refuse.label(), at: evidence.render(), reason }
    };
    denial.refusals.iter().map(finding).collect()
}

/// 結果の語（`clean`／`firm:<k>,provisional:<j>`／`unmeasured:<名>`）。
fn result_word(findings: Option<&[Finding]>) -> String {
    let Some(findings) = findings else {
        return "unmeasured:forecast".to_owned();
    };
    let count = |want: Certainty| findings.iter().filter(|found| found.certainty == want).count();
    let (firm, provisional) = (count(Certainty::Firm), count(Certainty::Provisional));
    if firm + provisional > 0 {
        return format!("firm:{firm},provisional:{provisional}");
    }
    findings.first().map_or_else(|| "clean".to_owned(), |found| format!("unmeasured:{}", found.name))
}

/// 置き場の結果の file の読み（鍵・結果の語・確定の finding の (名, 在り処) と理由）。
pub(super) struct Kept {
    /// 鍵の字。
    key: String,
    /// 結果の語。
    result: String,
    /// 確定の finding（(名, 在り処) → 理由の 1 行・new の印の突き合わせと直しの束の根・行 y）。
    pub(super) firm: BTreeMap<(String, String), String>,
}

/// 結果の file を読む（1 行目 `key=`・2 行目 `result=` の無い file は読めない＝無いと同じ・跨版の約束を持たない cache）。
pub(super) fn read(path: &Path) -> Option<Kept> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let key = lines.next()?.strip_prefix("key=")?.to_owned();
    let result = lines.next()?.strip_prefix("result=")?.to_owned();
    let firm = lines
        .filter_map(|line| {
            let (name, rest) = line.strip_prefix("finding=firm name=")?.split_once(" at=")?;
            let (at, rest) = rest.split_once(" new=")?;
            let reason = rest.split_once(" reason=").map_or("", |(_, found)| found);
            Some(((name.to_owned(), at.to_owned()), reason.to_owned()))
        })
        .collect();
    Some(Kept { key, result, firm })
}

/// 結果を書く（一時 file → rename・前の結果に無かった確定に `new=true`・書けない周は黙る＝次の周に撃ち直す）。
fn write(dir: &Path, bead: &str, key: &str, findings: Option<&[Finding]>, previous: Option<&Kept>) {
    let mut body = format!("key={key}\nresult={}\n", result_word(findings));
    for found in findings.unwrap_or_default() {
        let seen = previous.is_some_and(|kept| kept.firm.contains_key(&(found.name.clone(), found.at.clone())));
        let new = found.certainty == Certainty::Firm && !seen;
        let (certainty, name, at, reason) = (found.certainty.as_str(), &found.name, &found.at, &found.reason);
        body.push_str(&format!("finding={certainty} name={name} at={at} new={new} reason={reason}\n"));
    }
    let temporary = dir.join(format!("{bead}.{}.tmp", std::process::id()));
    if std::fs::create_dir_all(dir).is_err() || std::fs::write(&temporary, body).is_err() {
        return;
    }
    if std::fs::rename(&temporary, dir.join(bead)).is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
}

/// 事前審査の dir（`<state_dir>/pipe/precheck`）。
pub(super) fn dir_of(state_dir: &Path) -> PathBuf {
    state_dir.join(DIR).join(PRECHECK_DIR)
}

/// 列の依存待ちの候補（`WaitReason::Dependency`）の bead。
fn waiting_of(turn: &Turn) -> Vec<&str> {
    let waits = |reason: &Option<WaitReason>| matches!(reason, Some(WaitReason::Dependency { .. }));
    turn.candidates.iter().filter(|found| waits(&found.reason)).map(|found| found.bead.as_str()).collect()
}

/// rules の写しの blob の sha（写しが無いか sha を測れない周は器の版）。
fn rules_word(input: &Input<'_>) -> String {
    let path = input.rules.and_then(|found| std::fs::canonicalize(found).ok());
    let sha = path.and_then(|found| git_bytes(input.repo, &["hash-object", "--", &found.display().to_string()]));
    sha.and_then(|found| String::from_utf8(found).ok())
        .map_or_else(|| crate::name::BUILD_COMMIT.trim().to_owned(), |found| found.trim().to_owned())
}

/// 起こす側の周の事前審査（形 4 / 5）: 依存待ちで設計 pointer を持つ行ごとに鍵（base の HEAD・rules の写しの sha か器の版・
/// 祖先ごとの id と状態の語）を組み、置き場の結果の鍵と字が同じ行は撃たない。依存待ちに居ない bead の file は同じ周に外す。
/// 置き場か base の材料を読めない周は撃たない（file は次の周まで残る）。
pub(super) fn round(input: &Input<'_>, turn: &Turn, issues: &[Issue], base: Option<&Materials>) {
    let (Some(base), Ok(state), Some(head)) = (base, current(input.state_dir), head_of(input.repo)) else {
        return;
    };
    let population = population(issues, input.state_dir, &state);
    let waiting: Vec<&str> = waiting_of(turn).into_iter().filter(|bead| population.rows.contains_key(*bead)).collect();
    let dir = dir_of(input.state_dir);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if !waiting.iter().any(|bead| entry.file_name() == **bead) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let (rules, ctx) = (rules_word(input), Ctx { input, population: &population, base });
    let mut memo = BTreeMap::new();
    for &bead in &waiting {
        let Some(row) = population.rows.get(bead) else {
            continue;
        };
        let ancestors = ancestors_of(&population, bead);
        let words: Vec<String> = ancestors
            .iter()
            .filter_map(|id| population.rows.get(id).map(|found| format!("{id}={}", state_word(input, found))))
            .collect();
        let key = format!("head:{head} rules:{rules} ancestors:{}", words.join(","));
        let previous = read(&dir.join(bead));
        if previous.as_ref().is_some_and(|kept| kept.key == key) {
            continue;
        }
        let findings = judged(&ctx, row, &ancestors, &mut memo);
        write(&dir, bead, &key, findings.as_deref(), previous.as_ref());
    }
    // 周の終わりに確定の finding を根で束ねる（行 y・設計 §27 形 1）。
    super::bundle::round(input, &dir, &population, &waiting);
}

/// `dispatch ls` の事前審査の行（依存待ちの候補ごとに 1 行・結果の file を読むだけ・形 7）:
/// `[DISPATCH-PRECHECK] bead=<id> result=<結果の語か -> base=<current|moved>`（鍵の HEAD が今の base と同じ周だけ `current`）。
pub(super) fn lines(input: &Input<'_>, turn: &Turn) -> Vec<String> {
    let (dir, head) = (dir_of(input.state_dir), head_of(input.repo));
    waiting_of(turn)
        .into_iter()
        .map(|bead| {
            let kept = read(&dir.join(bead));
            let result = kept.as_ref().map_or(DASH, |found| found.result.as_str());
            let at = kept.as_ref().and_then(|found| found.key.strip_prefix("head:")?.split_whitespace().next());
            let base = if head.is_some() && at == head.as_deref() { "current" } else { "moved" };
            format!("{LINE} bead={bead} result={result} base={base}")
        })
        .collect()
}
