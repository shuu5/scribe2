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
use super::super::row_review::Basis;
use super::super::table::{parse_pointer, read_rows, ContractRow, Pointer};
use super::super::{base_of_run, contract_path, current, git_bytes, head_of, show_head, worktree_path, DIR};
use super::candidates::{is_blocking, pointer_of};
use super::{Input, Turn, WaitReason, CLOSED, DASH};
use crate::fleet::{Stage, State};
use crate::ledger::form::{is_memo, is_question};
use crate::rules::manifest::Manifest;
use crate::seat::ledger::Issue;
use std::cell::RefCell;
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

/// 母集団と到達の 1 関数（形 1）: 同じ周の台帳の全件と置き場の run の列から、(a) 閉じていない契約の行（closed でない ∧ memo でも
/// 台帳の問いでもない〔§31〕 ∧ acceptance の設計 pointer が列と同じ [`pointer_of`] で解ける bead・live な便の在る bead はその run dir の契約の
/// 写しの write-set つき）と、(b) blocks の推移の到達（[`is_blocking`] の依存だけ＝`parent-child` は数えず closed で止まる・
/// 1 度訪ねた bead で止まり循環で回らない）を返す。live でない行の write-set は呼び手が自分の材料で [`generated`] を撃って決める。
pub(super) fn population(issues: &[Issue], state_dir: &Path, state: &State) -> Population {
    let closed: BTreeSet<&str> = issues.iter().filter(|issue| issue.status == CLOSED).map(|issue| issue.id.as_str()).collect();
    let rows: BTreeMap<String, Row> = issues
        .iter()
        .filter(|issue| issue.status != CLOSED && !is_memo(issue) && !is_question(issue))
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

/// 祖先 1 つの重ね方（形 1・先撃ちの実体化も同じ層を当てる・行 aa）。
#[derive(Clone)]
pub(in crate::pipe) enum Layer {
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
        /// A の木の HEAD の sha（先撃ちが `add` の file を写す元）。
        head: String,
    },
}

/// 1 周に固定な材料（候補ごとに読み直さない）。
struct Ctx<'a, 'b> {
    /// 列の材料。
    input: &'a Input<'b>,
    /// 同じ周に読んだ台帳の全件。
    issues: &'a [Issue],
    /// 母集団と到達。
    population: &'a Population,
    /// 同じ周に `turn` が読んだ base の材料。
    base: &'a Materials,
}

/// Gated PASS の便（live ∧ `Gated` ∧ verdict PASS・verdict の読みは着地の段と同じ [`verdict_of`]）なら run id。
fn tree_run<'a>(state_dir: &Path, row: &'a Row) -> Option<&'a str> {
    let (run, stage, _) = row.live.as_ref()?;
    (*stage == Stage::Gated && verdict_of(state_dir, run) == Some(Verdict::Pass)).then_some(run.as_str())
}

/// 祖先ごとの状態の語（鍵の材料・形 4）: `declared`／`run:<便>`／`tree:<便>@<worktree の HEAD の sha>`。
fn state_word(repo: &Path, state_dir: &Path, row: &Row) -> String {
    match (tree_run(state_dir, row), row.live.as_ref()) {
        (Some(run), _) => format!("tree:{run}@{}", head_of(&worktree_path(repo, run)).unwrap_or_else(|| DASH.to_owned())),
        (None, Some((run, ..))) => format!("run:{run}"),
        (None, None) => "declared".to_owned(),
    }
}

/// 祖先 1 つの状態（行の審査の記録の `ancestors` の語・設計 row-review.md §3 形 3）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::pipe) enum Standing {
    /// 祖先を指す bead が全部 closed（base に着地済み・層は空）。
    Landed,
    /// 祖先の便が Gated で判定 PASS（実物の層）。
    Tree,
    /// それ以外（宣言の予想の層）。
    Declared,
}

impl Standing {
    /// 行の記録の `ancestors` に書く字面。
    pub(in crate::pipe) fn word(self) -> &'static str {
        match self {
            Self::Landed => "landed",
            Self::Tree => "tree",
            Self::Declared => "declared",
        }
    }
}

/// 祖先 1 つの（行・状態・当てる層）と、行の祖先の列（依存の順）と basis の対（口 (G) の返り）。
pub(in crate::pipe) type Ancestor = (String, Standing, Layer);
pub(in crate::pipe) type Ancestry = (Vec<Ancestor>, Basis);

/// 設計 pointer の行の字（`<path>#<id>`・行の祖先の層の口の鍵）。
fn row_key(pointer: &Pointer) -> String {
    format!("{}#{}", pointer.path, pointer.id)
}

/// 行を鍵にした祖先の読み（1 周に 1 回の台帳と表の読みを借りる・設計 row-review.md §3 形 3）。
struct Walk<'a> {
    /// repo（Gated PASS の便の worktree を引く anchor）。
    repo: &'a Path,
    /// 表の木（表の depends と便の無い祖先の契約の生成を HEAD から読む・事前審査は repo と同じ path を渡す）。
    table: &'a Path,
    /// 置き場。
    state_dir: &'a Path,
    /// 母集団と到達。
    population: Population,
    /// 設計 pointer の行ごとの bead（memo でも台帳の問いでもない全件）。
    beads: BTreeMap<String, Vec<&'a Issue>>,
    /// doc ごとの表の行（読めない doc は理由・1 度読んだら読み直さない）。
    docs: RefCell<BTreeMap<String, Result<Vec<ContractRow>, String>>>,
}

impl<'a> Walk<'a> {
    /// 台帳と置き場の状態から読む。
    fn new(repo: &'a Path, table: &'a Path, state_dir: &'a Path, issues: &'a [Issue], state: &State) -> Self {
        let mut beads: BTreeMap<String, Vec<&Issue>> = BTreeMap::new();
        for issue in issues.iter().filter(|issue| !is_memo(issue) && !is_question(issue)) {
            if let Some(pointer) = pointer_of(&issue.acceptance) {
                beads.entry(row_key(&pointer)).or_default().push(issue);
            }
        }
        Self { repo, table, state_dir, population: population(issues, state_dir, state), beads, docs: RefCell::default() }
    }

    /// 行を指す bead が全部 closed か（bead が 1 本以上在る行だけ）。
    fn landed(&self, row: &str) -> bool {
        self.beads.get(row).is_some_and(|found| found.iter().all(|issue| issue.status == CLOSED))
    }

    /// 行を代表する open な bead（Gated PASS の便を持つ bead → live な便を持つ bead → bead id の先頭の順）。
    fn rep(&self, row: &str) -> Option<(&str, &Row)> {
        let mut open: Vec<(&str, &Row)> = self.beads.get(row)?.iter().filter_map(|issue| Some((issue.id.as_str(), self.population.rows.get(&issue.id)?))).collect();
        open.sort_by_key(|(id, _)| *id);
        let tree = open.iter().find(|(_, found)| tree_run(self.state_dir, found).is_some());
        tree.or_else(|| open.iter().find(|(_, found)| found.live.is_some())).or_else(|| open.first()).copied()
    }

    /// 行の表の depends（同じ doc の行・読めない周は理由）。
    fn depends_of(&self, row: &str) -> Result<Vec<String>, String> {
        let pointer = parse_pointer(row).map_err(|err| format!("{row} は設計 pointer の形でない（{}）", err.reason()))?;
        let mut docs = self.docs.borrow_mut();
        let table = docs.entry(pointer.path.clone()).or_insert_with(|| {
            let text = show_head(self.table, &pointer.path).ok_or_else(|| format!("{} を base（HEAD）から読めない", pointer.path))?;
            read_rows(&pointer.path, &text).map_err(|errors| errors.iter().map(|error| error.reason()).collect::<Vec<String>>().join(" / "))
        });
        let rows = table.as_ref().map_err(Clone::clone)?;
        let found = rows.iter().find(|found| found.id == pointer.id).ok_or_else(|| format!("{} に行 {} が無い", pointer.path, pointer.id))?;
        Ok(found.depends.iter().map(|id| format!("{}#{id}", pointer.path)).collect())
    }

    /// 行の bead が台帳の blocks で待つ open な契約の行（依存の順）。
    fn blocked_by(&self, row: &str) -> Vec<String> {
        let found = self.beads.get(row).map(Vec::as_slice).unwrap_or_default();
        let reach = found.iter().filter_map(|issue| self.population.reach.get(&issue.id)).flatten();
        reach.filter_map(|id| self.population.rows.get(id)).map(|found| row_key(&found.pointer)).collect()
    }

    /// 行の祖先（依存の順・自分は含まない）: 表の depends を推移でたどった同じ doc の行と、行の bead の台帳の blocks の到達
    /// （着地済みの行は先へたどらない・表を読めない周は行自身なら理由・祖先なら先を持たない行）。
    fn closure(&self, row: &str) -> Result<Vec<String>, String> {
        let (mut seen, mut order) = (BTreeSet::from([row.to_owned()]), Vec::new());
        self.visit(row, &mut seen, &mut order, true)?;
        Ok(order)
    }

    /// [`Self::closure`] の深さ優先（帰りがけに積む＝依存が先に並ぶ）。
    fn visit(&self, row: &str, seen: &mut BTreeSet<String>, order: &mut Vec<String>, root: bool) -> Result<(), String> {
        if self.landed(row) {
            return Ok(());
        }
        let direct = match self.depends_of(row) {
            Ok(found) => found,
            Err(reason) if root => return Err(reason),
            Err(_) => Vec::new(),
        };
        for dep in direct.into_iter().chain(self.blocked_by(row)) {
            if seen.insert(dep.clone()) {
                self.visit(&dep, seen, order, false)?;
                order.push(dep);
            }
        }
        Ok(())
    }

    /// 祖先ごとの鍵の語（`<bead>=<状態の語>`・bead の無い祖先は行の字・着地済みは書かない＝base の HEAD が鍵に入る）。
    fn words(&self, row: &str) -> Vec<String> {
        let ancestors = self.closure(row).unwrap_or_default();
        let word = |id: &String| match self.rep(id) {
            Some((bead, found)) => format!("{bead}={}", state_word(self.repo, self.state_dir, found)),
            None => format!("{id}=declared"),
        };
        ancestors.iter().filter(|id| !self.landed(id)).map(word).collect()
    }

    /// 祖先 1 つの状態と層を決めて memo に置く（決まらない周は `None`）。着地済みは空の層。live な便は写しの write-set か、Gated
    /// PASS なら実物。便の無い行は自分の祖先を重ねた予想の base で [`generated`] を撃った契約の write-set。先に `None` を置くので
    /// 循環は決まらない側に倒れて回らない。
    fn resolve(&self, base: &Materials, row: &str, memo: &mut BTreeMap<String, Option<(Standing, Layer)>>) {
        if memo.contains_key(row) {
            return;
        }
        memo.insert(row.to_owned(), None);
        let found = if self.landed(row) { Some((Standing::Landed, Layer::Declared(Vec::new()))) } else { self.pending(base, row, memo) };
        memo.insert(row.to_owned(), found);
    }

    /// 着地していない祖先の状態と層。
    fn pending(&self, base: &Materials, row: &str, memo: &mut BTreeMap<String, Option<(Standing, Layer)>>) -> Option<(Standing, Layer)> {
        let live = self.rep(row).map(|(_, found)| found);
        if let Some(run) = live.and_then(|found| tree_run(self.state_dir, found)) {
            return tree_of(self.repo, self.state_dir, run).map(|layer| (Standing::Tree, layer));
        }
        if let Some((_, _, copied)) = live.and_then(|found| found.live.as_ref()) {
            return copied.clone().map(|write_set| (Standing::Declared, Layer::Declared(write_set)));
        }
        let (pointer, ancestors) = (parse_pointer(row).ok()?, self.closure(row).ok()?);
        for id in &ancestors {
            self.resolve(base, id, memo);
        }
        let found: Option<Vec<&Layer>> = ancestors.iter().map(|id| memo.get(id)?.as_ref().map(|(_, layer)| layer)).collect();
        let (materials, _) = overlay(base, &found?);
        generated(self.table, &pointer, &materials).ok().map(|(contract, _)| (Standing::Declared, Layer::Declared(contract.write_set)))
    }

    /// 行の祖先ごとの（行・状態・当てる層）と basis（依存の順・祖先の層か契約の生成が決まらない周は理由）。
    fn ancestry(&self, base: &Materials, row: &str) -> Result<Ancestry, String> {
        let order = self.closure(row)?;
        let mut memo = BTreeMap::new();
        for id in &order {
            self.resolve(base, id, &mut memo);
        }
        let mut found = Vec::new();
        for id in order {
            let decided = memo.remove(&id).flatten();
            let (standing, layer) = decided.ok_or_else(|| format!("祖先 {id} の層を決められない（写し・差分・契約の生成を読めない）"))?;
            found.push((id, standing, layer));
        }
        let declared = found.iter().any(|(_, standing, _)| *standing == Standing::Declared);
        let basis = match (declared, self.beads.contains_key(row)) {
            (true, _) => Basis::Forecast,
            (false, true) => Basis::Actual,
            (false, false) => Basis::Partial,
        };
        Ok((found, basis))
    }
}

/// 祖先の層の口（設計 row-review.md §3 の口 (G)・事前審査の予想はこの口の上に載る）: 行 `row`（`<doc>#<行 id>`）の祖先ごとの
/// （行・状態・当てる層）の列と basis。祖先は表の depends を推移でたどった同じ doc の行と、行を指す bead が在ればその台帳の blocks の
/// 祖先。`repo` は Gated PASS の便の worktree を引く anchor、`table` は表の depends と便の無い祖先の契約の生成が HEAD から読む木
/// （事前審査は 2 つに同じ path を渡す）。`base` は 1 周に 1 回読んだ材料で、口の中で読み直さない。表か bead の pointer を読めない周・
/// 祖先の層を決められない周は理由。
pub(in crate::pipe) fn ancestry(trees: (&Path, &Path), state_dir: &Path, issues: &[Issue], base: &Materials, row: &str) -> Result<Ancestry, String> {
    let state = current(state_dir).map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<String>>().join(" / "))?;
    Walk::new(trees.0, trees.1, state_dir, issues, &state).ancestry(base, row)
}

/// 予想に重ねる層（着地済みの祖先は base に在るので重ねない）。
fn applied(found: &[Ancestor]) -> Vec<&Layer> {
    found.iter().filter(|(_, standing, _)| *standing != Standing::Landed).map(|(_, _, layer)| layer).collect()
}

/// Gated PASS の便の実物（worktree の base..HEAD の name-status・rename は対）。
fn tree_of(repo: &Path, state_dir: &Path, run: &str) -> Option<Layer> {
    let worktree = worktree_path(repo, run);
    let (base, head) = (base_of_run(state_dir, run).known()?, head_of(&worktree)?);
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
    Some(Layer::Tree { add, remove, bodies, head })
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
            Layer::Tree { add: ref added, remove: ref removed, bodies: ref read, .. } => {
                add.extend(added.iter().cloned());
                remove.extend(removed.iter().cloned());
                bodies.extend(read.iter().cloned());
            }
        }
    }
    (base.forecast(&add, &remove, &bodies), moving)
}

/// finding 1 つ（確からしさ・断りの名・在り処の字面・理由の 1 行・先撃ちの確定も同じ形・行 aa）。
pub(in crate::pipe) struct Finding {
    /// 確定 / 暫定 / 測れない。
    pub(in crate::pipe) certainty: Certainty,
    /// 断りの名（型の断りは `Refuse::label`・型を持たない断りは材料の名・先撃ちは lens の理由の型）。
    pub(in crate::pipe) name: String,
    /// 在り処の字面（型を持たない断りは `-`・先撃ちは [`super::prelens::AT`]）。
    pub(in crate::pipe) at: String,
    /// 理由の 1 行。
    pub(in crate::pipe) reason: String,
}

/// 待ち行の祖先の層（行の鍵の口 [`ancestry`]・依存の順）。祖先の層を決められない周は `None`。
fn layers_of(ctx: &Ctx<'_, '_>, row: &Row) -> Option<Vec<Ancestor>> {
    let key = row_key(&row.pointer);
    let repo = ctx.input.repo;
    ancestry((repo, repo), ctx.input.state_dir, ctx.issues, ctx.base, &key).ok().map(|(found, _)| found)
}

/// 先撃ちの予想の口（行 aa・設計 §27 形 aa 1・層の読み手を 2 本にしない・C2）: 待ち行の祖先の層（依存の順）と、予想の base で
/// [`generated`] を撃った契約。祖先の層か生成が決まらない周は `None`。
fn forecast(ctx: &Ctx<'_, '_>, bead: &str) -> Option<super::prelens::Forecast> {
    let row = ctx.population.rows.get(bead)?;
    let found = layers_of(ctx, row)?;
    let found = applied(&found);
    let (contract, body) = generated(ctx.input.repo, &row.pointer, &overlay(ctx.base, &found).0).ok()?;
    Some(super::prelens::Forecast { layers: found.into_iter().cloned().collect(), contract, body })
}

/// 待ち行 1 つを予想の base で撃つ（形 2）: 祖先の重ね方が 1 つでも決まらなければ `None`（`unmeasured:forecast`）。
/// 撃つのは [`generated`] と [`judge`]（置き場なし・lock の前の読みなし＝列の候補の `blocker` と同じ形）だけで、置き場の要る
/// 判定と base の木の実走は撃たない。
fn judged(ctx: &Ctx<'_, '_>, row: &Row) -> Option<Vec<Finding>> {
    let found = layers_of(ctx, row)?;
    Some(forecast_findings(ctx.input.repo, ctx.input.manifest, ctx.base, &row.pointer, &found).1)
}

/// 予想の上の機械の検査の口（設計 row-review.md §3 の口 (I)・事前審査の待ち行の判定はこの口の上に載る）: 着地でない祖先の層を
/// `base` に重ねた予想の材料で [`generated`] を撃った契約とその file の字（生成が断った周は `None`）と、生成の断りか [`judge`]
/// （置き場なし）の断りを、宣言で重ねた祖先の write-set の file を動く file にして [`discern`] で確定・暫定・測れないに分けた
/// finding の列。`table` は契約の生成が HEAD から読む木。祖先を組めない周は呼び手の (G) が理由を返すのでここには無い。
pub(in crate::pipe) fn forecast_findings(
    table: &Path,
    manifest: &Manifest,
    base: &Materials,
    pointer: &Pointer,
    ancestors: &[Ancestor],
) -> (Option<(Contract, String)>, Vec<Finding>) {
    let (materials, moving) = overlay(base, &applied(ancestors));
    let (made, denials) = match generated(table, pointer, &materials) {
        Err(denial) => (None, vec![denial]),
        Ok(made) => {
            let material = Material { repo: table, manifest, contract: &made.0, state_dir: None, bead: "", materials: &materials, early: None };
            let denials = judge(&material).denials;
            (Some(made), denials)
        }
    };
    (made, denials.iter().flat_map(|denial| findings_of(denial, &moving)).collect())
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
    pub(super) key: String,
    /// 結果の語。
    result: String,
    /// 確定の finding（(名, 在り処) → 理由の 1 行・new の印の突き合わせと直しの束の根・行 y）。
    pub(super) firm: BTreeMap<(String, String), String>,
    /// 事前審査の判定が clean か（finding が先撃ちの在り処の行だけ・測れない結果でない・先撃ちを撃つ行・行 aa）。
    pub(super) clean: bool,
}

/// 結果の file を読む（1 行目 `key=`・2 行目 `result=` の無い file は読めない＝無いと同じ・跨版の約束を持たない cache）。
pub(super) fn read(path: &Path) -> Option<Kept> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let key = lines.next()?.strip_prefix("key=")?.to_owned();
    let result = lines.next()?.strip_prefix("result=")?.to_owned();
    let rest: Vec<&str> = lines.collect();
    let prelens = format!(" at={} ", super::prelens::AT);
    let clean = !result.starts_with("unmeasured") && rest.iter().all(|line| line.contains(&prelens));
    let firm = rest
        .into_iter()
        .filter_map(|line| {
            let (name, rest) = line.strip_prefix("finding=firm name=")?.split_once(" at=")?;
            let (at, rest) = rest.split_once(" new=")?;
            let reason = rest.split_once(" reason=").map_or("", |(_, found)| found);
            Some(((name.to_owned(), at.to_owned()), reason.to_owned()))
        })
        .collect();
    Some(Kept { key, result, firm, clean })
}

/// 結果を書く（一時 file → rename・前の結果に無かった確定に `new=true`・書けない周は黙る＝次の周に撃ち直す）。
pub(super) fn write(dir: &Path, bead: &str, key: &str, findings: Option<&[Finding]>, previous: Option<&Kept>) {
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
    let walk = Walk::new(input.repo, input.repo, input.state_dir, issues, &state);
    let population = &walk.population;
    let waiting: Vec<&str> = waiting_of(turn).into_iter().filter(|bead| population.rows.contains_key(*bead)).collect();
    let dir = dir_of(input.state_dir);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if !waiting.iter().any(|bead| entry.file_name() == **bead) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let (rules, ctx) = (rules_word(input), Ctx { input, issues, population, base });
    let mut before = BTreeMap::new();
    for &bead in &waiting {
        let Some(row) = population.rows.get(bead) else {
            continue;
        };
        let words = walk.words(&row_key(&row.pointer));
        let key = format!("head:{head} rules:{rules} ancestors:{}", words.join(","));
        let previous = read(&dir.join(bead));
        if previous.as_ref().is_some_and(|kept| kept.key == key) {
            continue;
        }
        let findings = judged(&ctx, row);
        write(&dir, bead, &key, findings.as_deref(), previous.as_ref());
        before.insert(bead, previous);
    }
    // 先撃ち（行 aa・設計 §27 形 aa）は書き直した後・束ねる前（書き直す前の結果を前の結果として判定を同じ周に写し直す・形 aa 4）。
    let sweep = super::prelens::Sweep { input, dir: &dir, population, waiting: &waiting, before };
    super::prelens::round(&sweep, &mut |bead| forecast(&ctx, bead));
    // 周の終わりに確定の finding を根で束ねる（行 y・設計 §27 形 1）。
    super::bundle::round(input, &dir, population, &waiting);
}

/// `dispatch ls` の事前審査の行（依存待ちの候補ごとに 1 行・結果の file を読むだけ・形 7）:
/// `[DISPATCH-PRECHECK] bead=<id> result=<結果の語か -> base=<current|moved>`（鍵の HEAD が今の base と同じ周だけ `current`）と、
/// 先撃ちの状態の末尾の語（[`super::prelens::word`]・行 aa）。
pub(super) fn lines(input: &Input<'_>, turn: &Turn) -> Vec<String> {
    let (dir, head) = (dir_of(input.state_dir), head_of(input.repo));
    waiting_of(turn)
        .into_iter()
        .map(|bead| {
            let kept = read(&dir.join(bead));
            let result = kept.as_ref().map_or(DASH, |found| found.result.as_str());
            let at = kept.as_ref().and_then(|found| found.key.strip_prefix("head:")?.split_whitespace().next());
            let base = if head.is_some() && at == head.as_deref() { "current" } else { "moved" };
            format!("{LINE} bead={bead} result={result} base={base}{}", super::prelens::word(input, bead))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ancestry, forecast_findings, population, Layer, Path, Standing, State};
    use crate::ledger::form::{MEMO_LABEL, QUESTION_LABEL};
    use crate::pipe::cli::Materials;
    use crate::pipe::fixture::scratch;
    use crate::pipe::refuse::Certainty;
    use crate::pipe::row_review::Basis;
    use crate::pipe::table::parse_pointer;
    use crate::pipe::{git_line, git_ok};
    use crate::rules::manifest::Manifest;
    use crate::seat::ledger::{Dep, Issue};

    /// open の bead（label と acceptance と blocks の依存先だけを与える）。
    fn open(id: &str, labels: &[&str], acceptance: &str, blocked_by: &[&str]) -> Issue {
        let labels = labels.iter().map(|label| (*label).to_owned()).collect();
        let deps = blocked_by.iter().map(|on| Dep { on: (*on).to_owned(), kind: "blocks".to_owned() }).collect();
        let (id, status, acceptance, kind) = (id.to_owned(), "open".to_owned(), acceptance.to_owned(), "task".to_owned());
        Issue { id, status, priority: None, labels, acceptance, deps, kind, description: String::new(), notes: String::new(), close_reason: String::new(), created_at: None, closed_at: None, updated_at: None, effect: String::new() }
    }

    /// 契約 c（q に blocks される）・label `label` と設計 pointer を持つ q・label の無い同じ pointer の p の (契約の行, c の到達)。
    fn rows_and_reach(label: &str) -> (Vec<String>, Vec<String>) {
        let issues = [
            open("c", &[], "design = docs/design/x.md#c", &["q"]),
            open("q", &[label], "design = docs/design/x.md#q", &[]),
            open("p", &[], "design = docs/design/x.md#q", &[]),
        ];
        let found = population(&issues, Path::new("/nonexistent"), &State::default());
        (found.rows.keys().cloned().collect(), found.reach.get("c").cloned().unwrap_or_default())
    }

    /// (c) 台帳の問い（label intake:question・設計 pointer）は契約の行に数えず、blocks の到達には残る（§31 形 4）。
    #[test]
    fn precheck_intake_label_question_is_not_a_row_but_stays_reachable() {
        assert_eq!(rows_and_reach(QUESTION_LABEL), (vec!["c".to_owned(), "p".to_owned()], vec!["q".to_owned()]));
    }

    /// (d) 回帰: memo（label intake:memo・設計 pointer）も契約の行に数えない。
    #[test]
    fn precheck_intake_label_memo_is_not_a_row() {
        assert_eq!(rows_and_reach(MEMO_LABEL).0, ["c", "p"]);
    }

    /// 設計 doc の 1 行（`y` は `+src/fresh.rs` を宣言し、`x` はその file を素で持って表の depends で `y` に繋がる）。
    fn toy_row(id: &str, write_set: &str, depends: &str) -> String {
        let tail = format!("write-set = [\"{write_set}\"]\n{depends}verify = [\"git status\"]\nsize = \"S\"\ndone = \"d\"\n");
        format!("[[contract]]\nid = \"{id}\"\ntitle = \"t\"\nreq = [\"FR4\"]\nsection = \"1\"\n{tail}")
    }

    /// toy repo（2 つの commit: 行の無い表の C1・行 x と y を足した表の C2）。返りは (anchor = C1 を detach した木, 表の木 = C2 の repo)。
    fn toy_repo(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = scratch(name);
        let (repo, anchor) = (root.join("repo"), root.join("anchor"));
        assert!(std::fs::create_dir_all(repo.join("src")).is_ok() && std::fs::create_dir_all(repo.join("docs/design")).is_ok());
        let put = |path: &str, body: &str| assert!(std::fs::write(repo.join(path), body).is_ok(), "{path}");
        for args in [&["init", "-q", "-b", "main"][..], &["config", "user.name", "t"], &["config", "user.email", "t@example.invalid"]] {
            assert!(git_ok(&repo, args), "{args:?}");
        }
        put("src/lib.rs", "// seed\n");
        put("reqs.md", "# 要件\n\n## FR4\n");
        put(".vessel.toml", "schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git status\"]\nrequirements = \"reqs.md\"\n");
        put("docs/design/toy.md", "# 設計: toy\n\n## 1. 節\n\n節の本文。\n");
        for args in [&["add", "-A"][..], &["commit", "-q", "-m", "c1"]] {
            assert!(git_ok(&repo, args), "{args:?}");
        }
        let c1 = git_line(&repo, &["rev-parse", "HEAD"]).unwrap_or_default();
        let rows = [toy_row("y", "+src/fresh.rs", ""), toy_row("x", "src/fresh.rs", "depends = [\"y\"]\n")];
        let doc = format!("# 設計: toy\n\n## 1. 節\n\n節の本文。\n\n<!-- contracts:begin -->\nschema = 1\n\n{}\n<!-- contracts:end -->\n", rows.join("\n"));
        put("docs/design/toy.md", &doc);
        for args in [&["add", "-A"][..], &["commit", "-q", "-m", "c2"]] {
            assert!(git_ok(&repo, args), "{args:?}");
        }
        assert!(git_ok(&repo, &["worktree", "add", "--detach", &anchor.display().to_string(), &c1]), "anchor の木");
        (anchor, repo)
    }

    /// 組んだ材料（表の木から 1 回読む）。
    fn toy_base(table: &Path, manifest: &Manifest) -> Materials {
        let Ok(found) = Materials::of(table, manifest, "bd") else {
            panic!("表の木から材料を読める");
        };
        found
    }

    /// (q) 口 (G): anchor（C1）と表の木（C2）を渡すと、C2 だけに在る行 x の祖先 y を declared・basis forecast で返す。表の木にも C1 を
    /// 渡すと行 x が表に無いので組めない理由を返す（表の木の引数を無視して anchor から読むと後者が通ってしまう）。
    #[test]
    fn precheck_row_mouths_ancestry_reads_the_table_from_the_table_tree() {
        let Ok(manifest) = Manifest::embedded() else {
            panic!("埋め込み manifest を読める");
        };
        let (anchor, table) = toy_repo("mouths-g");
        let (base, state) = (toy_base(&table, &manifest), scratch("mouths-g-state"));
        let found = ancestry((&anchor, &table), &state, &[], &base, "docs/design/toy.md#x");
        let Ok((layers, basis)) = found else {
            panic!("表の木に行 x と y が在る: {:?}", found.err());
        };
        assert!(basis == Basis::Forecast, "宣言の祖先を持つ行は forecast");
        let [(id, standing, Layer::Declared(write_set))] = layers.as_slice() else {
            panic!("祖先は y の 1 つで宣言の層");
        };
        assert_eq!((id.as_str(), write_set.as_slice()), ("docs/design/toy.md#y", ["+src/fresh.rs".to_owned()].as_slice()));
        assert!(*standing == Standing::Declared, "未着地の祖先は declared");
        let blind = ancestry((&anchor, &anchor), &state, &[], &base, "docs/design/toy.md#x");
        assert!(blind.as_ref().is_err_and(|reason| reason.contains("行 x が無い")), "表の木に C1 を渡すと組めない: {:?}", blind.err());
    }

    /// (q) 口 (I): (G) の祖先の列を渡すと行 x の finding に確定が無く、空の祖先の列を渡すと確定の write-set-item-unresolved を返す。
    #[test]
    fn precheck_row_mouths_findings_are_firm_only_without_the_ancestors_layers() {
        let Ok(manifest) = Manifest::embedded() else {
            panic!("埋め込み manifest を読める");
        };
        let (anchor, table) = toy_repo("mouths-i");
        let (base, state) = (toy_base(&table, &manifest), scratch("mouths-i-state"));
        let Ok(pointer) = parse_pointer("docs/design/toy.md#x") else {
            panic!("設計 pointer");
        };
        let Ok((layers, _)) = ancestry((&anchor, &table), &state, &[], &base, "docs/design/toy.md#x") else {
            panic!("祖先を組める");
        };
        let (made, with) = forecast_findings(&table, &manifest, &base, &pointer, &layers);
        assert!(made.is_some(), "祖先の層を重ねた予想の材料で契約を生成できる");
        assert!(with.iter().all(|found| found.certainty != Certainty::Firm), "祖先を渡すと確定が無い: {:?}", with.iter().map(|found| &found.name).collect::<Vec<_>>());
        let (_, without) = forecast_findings(&table, &manifest, &base, &pointer, &[]);
        assert!(without.iter().any(|found| found.certainty == Certainty::Firm && found.name == "write-set-item-unresolved"), "空の祖先は確定");
    }
}
