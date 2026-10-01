//! 台帳の形の門（起票の門の続き・設計 docs/design/ledger-form.md §12・契約表の行 h・SRS FR20 / FR51 / NFR5 / NFR4）。
//!
//! 起票の門（[`super::ledger_guard`]）で止まらなかった Bash の command のうち、台帳のグラフの崩れを増やしうる閉じた 6 つの
//! 書き（[`Op`]）だけを掛ける: create の `--parent`・create の `--graph` の plan の node・update の `--parent`（空も）・
//! update の `--type`・`dep remove` / `dep rm`・数えに戻す書き（`reopen` と、update の `--status` / `-s` / `--claim` の
//! closed でも pinned でもない状態）。掛かる segment が 1 つも無い command と、hook の root が `.beads` の dir を持たない
//! repo の command は台帳を読まない（NFR5）。
//!
//! 掛かる segment が全部、付け先を持たず型の値が epic の update の周は台帳を読まずに通し、payload の cwd が hook の root で先読みの口の写しの
//! 鍵が今の鍵と等しい周は client を起こさず写しで測る（設計 §20）。
//!
//! 掛かる周は台帳を 1 回だけ読み（待ち上限は rules 行 [`BUDGET_ROW`]）、読んだ写し 1 つ（[`Ledger`]）に segment の順で
//! 当て、通った segment の効き（足す子・親の付け替えと外し・型・status）を写しに足してから次の segment を測る。断るのは
//! 崩れが**増える**向きだけ（[`Refusal`]・問いは §10 の [`Graph`] が持つ）で、すでに崩れた所を直す書きは通す。読めない
//! 台帳・待ち上限の超過・rules 行の欠けは断る（FailClosed・起票の門の極性のまま＝極性一覧は増えない）。結果は起票の門の
//! enum（[`LedgerDecision`]）で返し、記録は起票の門と同じ `ledger-deny <語>`。

use super::ledger_guard::{create_of, is_assignment, segments, write_of, LedgerDecision};
use crate::fleet::json_tree::{self, Tree};
use crate::ledger::graph::{self, Graph, Reach};
use crate::name::NAME;
use crate::rules::int_row;
use crate::rules::manifest::Manifest;
use crate::seat::ledger::{self, Dep, Issue, LedgerError};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

/// 台帳を持つ repo の印（hook の root の直下の dir・host の見張りの台帳の印と同じ向き）。
const BEADS: &str = ".beads";

/// 台帳の読みの待ち上限を宣言する rules 行の id（ms・NFR5 の hook の予算・値は code に焼かない）。
pub const BUDGET_ROW: &str = "hook.budget_ms";

/// 根になれる型。
const EPIC: &str = "epic";
/// 閉じた status。
const CLOSED: &str = "closed";
/// 常設・閉じない status（数えない）。
const PINNED: &str = "pinned";
/// `reopen` が置く status。
const OPEN: &str = "open";
/// `--claim` が置く status。
const CLAIMED: &str = "in_progress";
/// 親子の辺の種別。
const PARENT_CHILD: &str = "parent-child";

/// 掛かる subcommand（create は起票の門の読み [`create_of`] が読む）。
const UPDATE: &str = "update";
/// 数えに戻す subcommand。
const REOPEN: &str = "reopen";
/// 辺の subcommand（次の語が [`REMOVE`] のとき辺を外す）。
const DEP: &str = "dep";
/// 辺を外す語（綴り 2 つ）。
const REMOVE: [&str; 2] = ["remove", "rm"];
/// 親の flag。
const PARENT: &str = "--parent";
/// 型の flag（綴り 2 つ）。
const KIND: [&str; 2] = ["--type", "-t"];
/// status の flag（綴り 2 つ）。
const STATUS: [&str; 2] = ["--status", "-s"];
/// 取って仕掛かりにする flag（値を取らない）。
const CLAIM: &str = "--claim";

/// 値を取らない flag の閉じた列（update の 8 語と大域の旗・道具の語彙）。ここに無い flag は次の語を値に取る（`-` で始まる
/// 語は値にしない）＝値を id に数えない。
const BOOLS: &[&str] = &[
    "--claim", "--ephemeral", "--persistent", "--stdin", "--force", "--dry-run", "--history", "--no-history", "--json",
    "--readonly", "--sandbox", "--quiet", "-q", "--verbose", "-v", "--help", "-h", "--allow-stale", "--profile",
];

/// hook が渡す材料（command 行・anchor・作業 dir・台帳 client と rules の差し替え）。
#[derive(Debug, Clone, Copy)]
pub struct Scene<'a> {
    /// Bash の command 行。
    pub command: &'a str,
    /// hook の root（`--project` の anchor・無ければ payload の cwd）。`.beads` の dir を持たなければ掛けない。
    pub root: &'a Path,
    /// payload の cwd（台帳の子 process の作業 dir・plan の相対 path の起点）。
    pub cwd: &'a Path,
    /// 台帳 client の差し替え（`--bd`・無ければ既定の名）。
    pub bd: Option<&'a str>,
    /// rules manifest の差し替え（`--rules`・無ければ埋め込み）。
    pub rules: Option<&'a Path>,
    /// 記録の置き場（先読みの口が置く台帳の形の写しの dir・設計 §20 約束 5）。
    pub state_dir: &'a Path,
}

/// 掛かる書き（閉じた 6 つ・create と graph は起票の門の [`create_of`] の読み）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// create の `--parent P`（空の P も）と型（`--type` / `-t`・無ければ空）。
    Create {
        /// 付け先。
        parent: String,
        /// 型の字面。
        kind: String,
    },
    /// create の `--graph F`（`--parent` は plan の node に効かない）。
    Graph {
        /// plan の file（cwd からの相対も）。
        file: String,
    },
    /// update X… の `--parent`・`--type`・`--status` / `--claim`（どれか 1 つ以上が掛かる形）。
    Update {
        /// 名指した id（flag でない語）。
        ids: Vec<String>,
        /// 付け先（空は外す）。
        parent: Option<String>,
        /// 型。
        kind: Option<String>,
        /// status（`--claim` は仕掛かり）。
        status: Option<String>,
    },
    /// `dep remove A B` / `dep rm A B`。
    DepRemove {
        /// 辺の元（子）。
        child: String,
        /// 辺の先（親）。
        parent: String,
    },
    /// `reopen X…`。
    Reopen {
        /// 名指した id。
        ids: Vec<String>,
    },
}

/// 断る閉じた理由（1 周に 1 つ・segment の順で最初に当たったもの）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// 付け先が根の epic に着かない（`top` は鎖の終わり・台帳に無い付け先と輪は付け先そのもの）。
    ParentUnrooted {
        /// 付け先。
        parent: String,
        /// 鎖の終わり。
        top: String,
    },
    /// 付け先の直下の open の子が上限を越える。
    ParentFull {
        /// 付け先。
        parent: String,
        /// 今の直下の open の子の数。
        children: usize,
        /// 上限（rules 行 `ledger.open_children_max`）。
        max: u64,
    },
    /// update の付け先が X 自身か X の子孫。
    ParentLoop {
        /// 付け先。
        parent: String,
        /// 名指した id。
        id: String,
    },
    /// epic でない bead を根から外す（親を外す update・唯一の親の dep remove）か、根の epic の型を epic 以外にする。
    Unrooting {
        /// 外れる id。
        id: String,
    },
    /// plan の node が plan の中をたどって台帳の親にも親の無い epic の node にも着かない。
    PlanOrphan {
        /// node の key。
        key: String,
    },
    /// plan の file が無い・読めない・JSON でない。
    PlanUnreadable {
        /// plan の file。
        file: String,
    },
    /// 台帳を読めない。
    LedgerUnreadable,
    /// 台帳の読みが待ち上限を越えた。
    LedgerTimeout {
        /// hook の root（次の一手の先読みの口が名指す repo）。
        root: String,
    },
    /// rules を読めない。
    RulesUnreadable,
    /// rules 行（上限・待ち上限）が無い・不発効・整数でない。
    NoRule,
}

impl Refusal {
    /// 記録と deny 文に出す理由の 1 語。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ParentUnrooted { .. } => "parent-unrooted",
            Self::ParentFull { .. } => "parent-full",
            Self::ParentLoop { .. } => "parent-loop",
            Self::Unrooting { .. } => "unrooting",
            Self::PlanOrphan { .. } => "plan-orphan",
            Self::PlanUnreadable { .. } => "plan-unreadable",
            Self::LedgerUnreadable => "ledger-unreadable",
            Self::LedgerTimeout { .. } => "ledger-timeout",
            Self::RulesUnreadable => "rules-unreadable",
            Self::NoRule => "no-rule",
        }
    }

    /// 理由の説明と、` — ` の後ろの直す 1 行。
    fn guidance(&self) -> String {
        match self {
            Self::ParentUnrooted { parent, top } => format!(
                "付け先 {} は根の epic に着かない（鎖の終わり {top}） — bdw update {top} --type epic か bdw update {top} \
                 --parent <epic> で根に着けてから付ける",
                shown(parent)
            ),
            Self::ParentFull { parent, children, max } => format!(
                "{parent} の直下の open の子は {children} 本で上限 {max} を越える — bdw create <題> --type epic --parent \
                 {parent} で子 epic を作り、その下へ置く（既存の子は bdw update <子> --parent <子 epic>）"
            ),
            Self::ParentLoop { parent, id } => {
                format!("付け先 {parent} は {id} 自身か子孫で親の輪を作る — 輪の外の epic を bdw update {id} --parent <epic> で名指す")
            }
            Self::Unrooting { id } => format!("{id} が根の epic から外れる — bdw update {id} --parent <epic> で付け替える"),
            Self::PlanOrphan { key } => format!(
                "plan の node {key} は親をたどって台帳の親にも親の無い epic の node にも着かない — node に parent_id を書く"
            ),
            Self::PlanUnreadable { file } => format!("plan の file {file} を読めない（無い・JSON でない） — 読める plan の file を渡す"),
            Self::LedgerUnreadable => "台帳を読めない — 形を測れない周は書きを通さない（台帳を直してから撃ち直す）".to_owned(),
            Self::LedgerTimeout { root } => format!(
                "台帳の読みが rules 行 {BUDGET_ROW} を越えた — {NAME} ledger prefetch --repo {root} で写しを作り、root の dir から同じ書きを\
                 撃ち直す（写しの後に台帳が書かれていなければ、門は写しで測る）"
            ),
            Self::RulesUnreadable => "rules を読めない — 形を測れない周は書きを通さない（rules を直す）".to_owned(),
            Self::NoRule => format!(
                "rules 行 {} か {BUDGET_ROW} が無い・不発効・整数でない — 形を測れない周は書きを通さない（行を戻す）",
                graph::ROW
            ),
        }
    }
}

/// 空の付け先の字面（deny 文で見える形）。
fn shown(id: &str) -> &str {
    if id.is_empty() {
        "\"\""
    } else {
        id
    }
}

/// command 行を捌く（起票の門で止まらなかった周だけ hook が撃つ）。掛かる書きが無い・台帳の無い repo は台帳を読まずに通す。
pub fn decide(scene: &Scene) -> LedgerDecision {
    match judge_command(scene) {
        Ok(()) => LedgerDecision::Allow,
        Err(found) => LedgerDecision::Deny { what: found.as_str().to_owned(), line: denied_line(&found) },
    }
}

/// 判定の全部（印 → 掛かる segment → rules → 台帳の 1 回の読み → segment の順の判定）。
fn judge_command(scene: &Scene) -> Result<(), Refusal> {
    if !scene.root.join(BEADS).is_dir() {
        return Ok(());
    }
    let ops: Vec<Op> = segments(scene.command).iter().filter_map(|words| op_of(words)).collect();
    if ops.is_empty() {
        return Ok(());
    }
    let manifest = scene.rules.map_or_else(Manifest::embedded, Manifest::load).map_err(|_| Refusal::RulesUnreadable)?;
    let (max, budget) = limits(&manifest)?;
    if passes_unread(&ops) {
        return Ok(());
    }
    let issues = match copied(scene) {
        Some(issues) => issues,
        None => {
            let bd = scene.bd.filter(|found| !found.trim().is_empty()).unwrap_or(ledger::DEFAULT_BD);
            ledger::read_ledger(bd, scene.cwd, budget).map_err(|error| match error {
                LedgerError::Unreadable => Refusal::LedgerUnreadable,
                LedgerError::Timeout => Refusal::LedgerTimeout { root: scene.root.display().to_string() },
            })?
        }
    };
    let mut copy = Ledger::new(issues, max);
    let read = |file: &str| std::fs::read_to_string(scene.cwd.join(file)).ok();
    ops.iter().try_for_each(|op| copy.apply(op, &read))
}

/// 台帳を読まずに通す書きの列か（付け先を持たず型の値が epic の update だけ・設計 §20 約束 7）。門が断るのは根の epic を epic 以外にする周だけで、
/// 同じ update の status の書きは型を epic にした後に判定される（epic は数えない）ので、どの台帳でも断られない。
fn passes_unread(ops: &[Op]) -> bool {
    ops.iter().all(|op| matches!(op, Op::Update { parent: None, kind: Some(kind), .. } if kind == EPIC))
}

/// 台帳の形の写し（設計 §20 約束 5）: payload の cwd を正規化した path が hook の root と等しく、写しの鍵が今の store の鍵と等しい周だけ在る。
/// bd の子 process も git も撃たず、写しは書かない。
fn copied(scene: &Scene) -> Option<Vec<Issue>> {
    if std::fs::canonicalize(scene.cwd).ok()? != scene.root {
        return None;
    }
    let key = crate::ledger::store_key(scene.root)?;
    crate::ledger::read_copy(scene.state_dir, scene.root, &key)
}

/// rules 行 2 本（上限 N と待ち上限）を id で引いて整数だけを読む。無い・不発効・整数でない周は [`Refusal::NoRule`]。
pub fn limits(manifest: &Manifest) -> Result<(u64, Duration), Refusal> {
    let max = int_row(manifest, graph::ROW).map_err(|_| Refusal::NoRule)?;
    let budget = int_row(manifest, BUDGET_ROW).map_err(|_| Refusal::NoRule)?;
    Ok((max, Duration::from_millis(budget)))
}

/// segment の語が掛かる書きなら [`Op`] を返す（それ以外は `None`＝台帳を読む理由にならない）。
pub fn op_of(words: &[String]) -> Option<Op> {
    if let Some(create) = create_of(words) {
        return match (create.graph, create.parent) {
            (Some(file), _) => Some(Op::Graph { file }),
            (None, Some(parent)) => Some(Op::Create { parent, kind: create.kind.unwrap_or_default() }),
            (None, None) => None,
        };
    }
    write_of(words)?;
    let args = args_of(words);
    let mut bare = args.bare.iter().map(String::as_str);
    match bare.next()? {
        UPDATE => update_of(bare.map(str::to_owned).collect(), &args),
        REOPEN => Some(bare.map(str::to_owned).collect::<Vec<_>>()).filter(|ids| !ids.is_empty()).map(|ids| Op::Reopen { ids }),
        DEP if bare.next().is_some_and(|word| REMOVE.contains(&word)) => {
            Some(Op::DepRemove { child: bare.next()?.to_owned(), parent: bare.next()?.to_owned() })
        }
        _ => None,
    }
}

/// update の読み（`--parent`・`--type`・数えに戻す `--status` / `--claim` のどれかを持つ周だけ掛かる）。
fn update_of(ids: Vec<String>, args: &Args) -> Option<Op> {
    let parent = args.last(&[PARENT]).map(str::to_owned);
    let kind = args.last(&KIND).map(str::to_owned);
    let status = args.last(&STATUS).map(str::to_owned).or_else(|| args.has(CLAIM).then(|| CLAIMED.to_owned()));
    let counted = status.as_deref().is_some_and(is_counted);
    let hits = parent.is_some() || kind.is_some() || counted;
    (hits && !ids.is_empty()).then_some(Op::Update { ids, parent, kind, status })
}

/// 直下の open の子に数える status か（closed でも pinned でもない）。
fn is_counted(status: &str) -> bool {
    status != CLOSED && status != PINNED
}

/// client の後ろの語の読み（flag でない語と flag の値）。
#[derive(Debug, Default)]
struct Args {
    /// flag でない語（subcommand を含む・flag の値は含めない）。
    bare: Vec<String>,
    /// flag の名と値（`--flag=value` と `--flag value`）。
    values: Vec<(String, String)>,
    /// flag の名。
    flags: Vec<String>,
}

impl Args {
    /// `flags` のどれかの最後の値。
    fn last(&self, flags: &[&str]) -> Option<&str> {
        self.values.iter().rev().find(|(flag, _)| flags.contains(&flag.as_str())).map(|(_, value)| value.as_str())
    }

    /// flag を持つか。
    fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|found| found == flag)
    }
}

/// client の後ろの語を読む（[`BOOLS`] に無い flag は次の `-` で始まらない語を値に取る）。
fn args_of(words: &[String]) -> Args {
    let rest: Vec<&String> = words.iter().skip_while(|word| is_assignment(word)).skip(1).collect();
    let mut args = Args::default();
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        at = at.saturating_add(1);
        if !word.starts_with('-') {
            args.bare.push((*word).clone());
            continue;
        }
        let (flag, inline) = match word.split_once('=') {
            Some((flag, value)) => (flag, Some(value.to_owned())),
            None => (word.as_str(), None),
        };
        args.flags.push(flag.to_owned());
        let value = inline.or_else(|| {
            let next = rest.get(at).filter(|next| !BOOLS.contains(&flag) && !next.starts_with('-'));
            at = at.saturating_add(usize::from(next.is_some()));
            next.map(|found| (*found).clone())
        });
        if let Some(value) = value {
            args.values.push((flag.to_owned(), value));
        }
    }
    args
}

/// plan の node 1 つ（判定が読む key だけ）。
#[derive(Debug, Clone)]
struct Node {
    /// node の key。
    key: String,
    /// 型（無ければ空）。
    kind: String,
    /// plan の中の親の key（空は無し）。
    parent_key: Option<String>,
    /// 台帳の親の id（空は無し）。
    parent_id: Option<String>,
}

/// plan の JSON の node の列（`nodes` の配列・key の無い node は読めない側）。
fn nodes_of(text: &str) -> Option<Vec<Node>> {
    let tree = json_tree::parse(text).ok()?;
    tree.get("nodes")?
        .as_array()?
        .iter()
        .map(|node| {
            let text_of = |key: &str| node.get(key).and_then(Tree::as_str).filter(|found| !found.is_empty()).map(str::to_owned);
            Some(Node {
                key: text_of("key")?,
                kind: text_of("type").unwrap_or_default(),
                parent_key: text_of("parent_key"),
                parent_id: text_of("parent_id"),
            })
        })
        .collect()
}

/// node が plan の中を parent_key でたどって、parent_id を持つ node か親の無い epic の node に着くか（知らない key と輪は
/// 着かない）。
fn anchored(node: &Node, by_key: &BTreeMap<&str, &Node>) -> bool {
    let mut seen = BTreeSet::new();
    let mut at = node;
    loop {
        if at.parent_id.is_some() {
            return true;
        }
        if !seen.insert(at.key.as_str()) {
            return false;
        }
        match at.parent_key.as_deref() {
            Some(key) => match by_key.get(key) {
                Some(next) => at = next,
                None => return false,
            },
            None => return at.kind == EPIC,
        }
    }
}

/// 読んだ台帳の写し（通った segment の効きを足していく）と上限 N。
#[derive(Debug, Clone)]
pub struct Ledger {
    /// 台帳の列（効きを足した後の形）。
    issues: Vec<Issue>,
    /// 直下の open の子の上限（0 は溢れを測らない）。
    max: u64,
    /// 写しに足した bead の通し番号（採番前の仮の id）。
    fresh: usize,
}

impl Ledger {
    /// 読んだ台帳と上限から写しを作る。
    pub fn new(issues: Vec<Issue>, max: u64) -> Self {
        Self { issues, max, fresh: 0 }
    }

    /// 書き 1 つを判定し、通れば効きを写しに足す（`read` は plan の file の本文・開けない周は `None`）。
    pub fn apply(&mut self, op: &Op, read: &dyn Fn(&str) -> Option<String>) -> Result<(), Refusal> {
        match op {
            Op::Create { parent, kind } => self.create(parent, kind),
            Op::Graph { file } => self.plan(file, read),
            Op::Update { ids, parent, kind, status } => ids.iter().try_for_each(|id| {
                if let Some(parent) = parent {
                    self.attach(id, parent, kind.as_deref())?;
                }
                if let Some(kind) = kind {
                    self.retype(id, kind)?;
                }
                status.as_deref().map_or(Ok(()), |status| self.restatus(id, status))
            }),
            Op::DepRemove { child, parent } => self.unlink(child, parent),
            Op::Reopen { ids } => ids.iter().try_for_each(|id| self.restatus(id, OPEN)),
        }
    }

    /// id の bead（写しの中）。
    fn find(&self, id: &str) -> Option<&Issue> {
        self.issues.iter().find(|issue| issue.id == id)
    }

    /// 付け先 `parent` の直下の open の子に `adding` 本を足して上限を越えるか（上限 0 は測らない）。
    fn room(&self, graph: &Graph<'_>, parent: &str, adding: usize) -> Result<(), Refusal> {
        let children = graph.open_children(parent);
        let over = u64::try_from(children.saturating_add(adding)).map_or(true, |count| count > self.max);
        if self.max > 0 && over {
            return Err(Refusal::ParentFull { parent: parent.to_owned(), children, max: self.max });
        }
        Ok(())
    }

    /// create の `--parent P`: 付け先が根に着き、epic でなければ溢れない。通れば仮の子を 1 本足す。
    fn create(&mut self, parent: &str, kind: &str) -> Result<(), Refusal> {
        let graph = Graph::of(&self.issues);
        rooted(&graph, parent)?;
        if kind != EPIC {
            self.room(&graph, parent, 1)?;
        }
        self.add(kind, Some(parent.to_owned()));
        Ok(())
    }

    /// create の `--graph F`: 読めない file・着かない node・根に着かない parent_id・溢れる parent_id を断り、通れば node を
    /// 全部足す。
    fn plan(&mut self, file: &str, read: &dyn Fn(&str) -> Option<String>) -> Result<(), Refusal> {
        let nodes = read(file).and_then(|text| nodes_of(&text)).ok_or_else(|| Refusal::PlanUnreadable { file: file.to_owned() })?;
        let by_key: BTreeMap<&str, &Node> = nodes.iter().map(|node| (node.key.as_str(), node)).collect();
        if let Some(orphan) = nodes.iter().find(|node| !anchored(node, &by_key)) {
            return Err(Refusal::PlanOrphan { key: orphan.key.clone() });
        }
        let graph = Graph::of(&self.issues);
        let mut adding: BTreeMap<&str, usize> = BTreeMap::new();
        for node in &nodes {
            let Some(parent) = node.parent_id.as_deref() else {
                continue;
            };
            rooted(&graph, parent)?;
            let count = adding.entry(parent).or_default();
            *count = count.saturating_add(usize::from(node.kind != EPIC));
        }
        adding.iter().try_for_each(|(parent, count)| self.room(&graph, parent, *count))?;
        for node in &nodes {
            let parent = node.parent_id.clone().or_else(|| node.parent_key.as_ref().map(|key| plan_id(key)));
            self.push(plan_id(&node.key), &node.kind, parent);
        }
        Ok(())
    }

    /// update X の `--parent P`（`kind` はこの update が置く型＝epic への付け替えは溢れで断らない）。
    fn attach(&mut self, id: &str, parent: &str, kind: Option<&str>) -> Result<(), Refusal> {
        let issue = self.find(id);
        let epic = kind.map_or_else(|| issue.is_some_and(|found| found.kind == EPIC), |found| found == EPIC);
        if parent.is_empty() && !epic {
            return Err(Refusal::Unrooting { id: id.to_owned() });
        }
        if !parent.is_empty() {
            let graph = Graph::of(&self.issues);
            if parent == id || graph.is_descendant(parent, id) {
                return Err(Refusal::ParentLoop { parent: parent.to_owned(), id: id.to_owned() });
            }
            if graph.is_rooted(id) {
                rooted(&graph, parent)?;
            }
            let moving = issue.is_some_and(|found| is_counted(&found.status) && graph::parent_of(found) != Some(parent));
            if moving && !epic {
                self.room(&graph, parent, 1)?;
            }
        }
        if let Some(found) = self.issues.iter_mut().find(|found| found.id == id) {
            found.deps.retain(|dep| dep.kind != PARENT_CHILD);
            found.deps.extend((!parent.is_empty()).then(|| parent_dep(parent)));
        }
        Ok(())
    }

    /// update X の `--type T`: 根の epic を epic 以外にする書きを断る。
    fn retype(&mut self, id: &str, kind: &str) -> Result<(), Refusal> {
        let root = self.find(id).is_some_and(|found| found.kind == EPIC && graph::parent_of(found).is_none());
        if root && kind != EPIC {
            return Err(Refusal::Unrooting { id: id.to_owned() });
        }
        if let Some(found) = self.issues.iter_mut().find(|found| found.id == id) {
            found.kind = kind.to_owned();
        }
        Ok(())
    }

    /// status を置く: closed か pinned の epic でない X を数えに戻す書きは、今の親の溢れを測る。
    fn restatus(&mut self, id: &str, status: &str) -> Result<(), Refusal> {
        let back = self.find(id).filter(|found| is_counted(status) && !is_counted(&found.status) && found.kind != EPIC);
        if let Some(parent) = back.and_then(graph::parent_of) {
            self.room(&Graph::of(&self.issues), parent, 1)?;
        }
        if let Some(found) = self.issues.iter_mut().find(|found| found.id == id) {
            found.status = status.to_owned();
        }
        Ok(())
    }

    /// `dep remove A B`: B が A の唯一の親で A が epic でなければ断る。通れば A の B への辺を外す。
    fn unlink(&mut self, child: &str, parent: &str) -> Result<(), Refusal> {
        let only = self.find(child).is_some_and(|found| {
            let parents: BTreeSet<&str> =
                found.deps.iter().filter(|dep| dep.kind == PARENT_CHILD).map(|dep| dep.on.as_str()).collect();
            found.kind != EPIC && parents.len() == 1 && parents.contains(parent)
        });
        if only {
            return Err(Refusal::Unrooting { id: child.to_owned() });
        }
        if let Some(found) = self.issues.iter_mut().find(|found| found.id == child) {
            found.deps.retain(|dep| dep.on != parent);
        }
        Ok(())
    }

    /// 採番前の仮の子を 1 本足す（同じ行の後の segment がその id を名指しても写しには無い＝設計 §12 の限界）。
    fn add(&mut self, kind: &str, parent: Option<String>) {
        self.fresh = self.fresh.saturating_add(1);
        self.push(format!("<new-{}>", self.fresh), kind, parent);
    }

    /// bead を 1 本写しに足す（open・親は parent-child の 1 本）。
    fn push(&mut self, id: String, kind: &str, parent: Option<String>) {
        self.issues.push(Issue {
            id,
            status: OPEN.to_owned(),
            priority: None,
            labels: Vec::new(),
            acceptance: String::new(),
            deps: parent.as_deref().map(parent_dep).into_iter().collect(),
            kind: kind.to_owned(),
            description: String::new(),
            notes: String::new(),
            close_reason: String::new(),
            created_at: None,
            closed_at: None,
            effect: String::new(),
        });
    }
}

/// 付け先が根に着くか（着かなければ鎖の終わりを名指す parent-unrooted・空と台帳に無い付け先と輪は付け先そのもの）。
fn rooted(graph: &Graph<'_>, parent: &str) -> Result<(), Refusal> {
    match graph.reach(parent) {
        Reach::Root(_) if !parent.is_empty() => Ok(()),
        Reach::Top(top) => Err(Refusal::ParentUnrooted { parent: parent.to_owned(), top: top.to_owned() }),
        _ => Err(Refusal::ParentUnrooted { parent: parent.to_owned(), top: shown(parent).to_owned() }),
    }
}

/// parent-child の辺 1 本。
fn parent_dep(parent: &str) -> Dep {
    Dep { on: parent.to_owned(), kind: PARENT_CHILD.to_owned() }
}

/// plan の node の写しの中の仮の id。
fn plan_id(key: &str) -> String {
    format!("<plan:{key}>")
}

/// deny 文 1 行（起票の門の台帳 write の断りと同じ頭・出所は §12）。
fn denied_line(refusal: &Refusal) -> String {
    format!(
        "{NAME}: deny 台帳の write は起票の門が止める reason={}（{}・ledger-form.md §12）",
        refusal.as_str(),
        refusal.guidance()
    )
}

#[cfg(test)]
mod tests {
    use super::{decide, limits, op_of, Ledger, Op, Refusal, Scene};
    use crate::hook::ledger_guard::{create_of, segments, LedgerDecision};
    use crate::rules::manifest::Manifest;
    use crate::seat::ledger::issues_of;
    use std::path::Path;

    /// 1 行の最初の segment の語。
    fn words(line: &str) -> Vec<String> {
        segments(line).into_iter().next().unwrap_or_default()
    }

    /// 1 件の JSON（`parents` は parent-child の辺の先）。
    fn bead(id: &str, status: &str, kind: &str, parents: &[&str]) -> String {
        let deps: Vec<String> = parents
            .iter()
            .map(|parent| format!("{{\"issue_id\":\"{id}\",\"depends_on_id\":\"{parent}\",\"type\":\"parent-child\"}}"))
            .collect();
        format!("{{\"id\":\"{id}\",\"status\":\"{status}\",\"issue_type\":\"{kind}\",\"dependencies\":[{}]}}", deps.join(","))
    }

    /// 根の epic r（open の子 r.1 と r.2・closed の r.3・pinned の r.4・r.1 の子 r.1.1）・子 epic r.e・feature の top f と
    /// その子 f.1・親 2 つの t2。
    fn fixture(max: u64) -> Ledger {
        let json = format!(
            "[{}]",
            [
                bead("r", "open", "epic", &[]),
                bead("r.1", "open", "task", &["r"]),
                bead("r.2", "open", "task", &["r"]),
                bead("r.3", "closed", "task", &["r"]),
                bead("r.4", "pinned", "task", &["r"]),
                bead("r.1.1", "open", "task", &["r.1"]),
                bead("r.e", "open", "epic", &["r"]),
                bead("f", "open", "feature", &[]),
                bead("f.1", "open", "task", &["f"]),
                bead("t2", "open", "task", &["r.1", "r.e"]),
            ]
            .join(",")
        );
        Ledger::new(issues_of(&json).unwrap_or_default(), max)
    }

    /// 1 行の segment を順に写しへ当て、最初の断りの語（通れば空）。
    fn run(ledger: &mut Ledger, line: &str, plan: Option<&str>) -> String {
        let read = |_: &str| plan.map(str::to_owned);
        let ops: Vec<Op> = segments(line).iter().filter_map(|found| op_of(found)).collect();
        assert!(!ops.is_empty(), "{line}: 掛かる書き");
        ops.iter().try_for_each(|op| ledger.apply(op, &read)).err().map_or_else(String::new, |found| found.as_str().to_owned())
    }

    /// 6 つの書きの読み: update の値を取らない flag と値を取る flag・`--parent=` の空・dep rm・reopen の複数の id・
    /// `--status` と `-s` と `--claim`・create の `--parent` / `--type` / `--graph`。掛からない書きは `None`。
    #[test]
    fn hook_graph_guard_reads_the_six_writes() {
        let update = |ids: &[&str], parent: Option<&str>, kind: Option<&str>, status: Option<&str>| {
            let own = |found: Option<&str>| found.map(str::to_owned);
            Some(Op::Update { ids: ids.iter().map(|id| (*id).to_owned()).collect(), parent: own(parent), kind: own(kind), status: own(status) })
        };
        assert_eq!(op_of(&words("bdw update --claim a b --title x --parent e")), update(&["a", "b"], Some("e"), None, Some("in_progress")));
        assert_eq!(op_of(&words("scripts/bdw --json update a --parent=")), update(&["a"], Some(""), None, None));
        assert_eq!(op_of(&words("bdw update a --parent \"\" --notes-x y")), update(&["a"], Some(""), None, None));
        assert_eq!(op_of(&words("bdw update a -t epic -s open")), update(&["a"], None, Some("epic"), Some("open")));
        assert_eq!(op_of(&words("bdw update a --status=blocked")), update(&["a"], None, None, Some("blocked")));
        let dep = Some(Op::DepRemove { child: "a".to_owned(), parent: "b".to_owned() });
        assert_eq!(op_of(&words("bdw dep rm a b")), dep);
        assert_eq!(op_of(&words("bdw dep remove a b --type parent-child")), dep);
        assert_eq!(op_of(&words("X=1 bdw reopen a b --reason x")), Some(Op::Reopen { ids: vec!["a".to_owned(), "b".to_owned()] }));
        assert_eq!(op_of(&words("bdw create x --parent e -t epic")), Some(Op::Create { parent: "e".to_owned(), kind: "epic".to_owned() }));
        assert_eq!(op_of(&words("bdw create x --graph p.json --parent e")), Some(Op::Graph { file: "p.json".to_owned() }));
        for line in [
            "bdw update a -s closed", "bdw update a --status pinned", "bdw update a --title x --append-notes y",
            "bdw update --parent e", "bdw dep add a b", "bdw close a", "bdw show a", "bdw reopen", "bdw create x",
            "echo update a --parent b", "bdw list --parent e",
        ] {
            assert_eq!(op_of(&words(line)), None, "{line}");
        }
        let create = create_of(&words("bdw create x --parent=e --type task --graph=p.json")).unwrap_or_default();
        assert_eq!((create.parent.as_deref(), create.kind.as_deref(), create.graph.as_deref()), (Some("e"), Some("task"), Some("p.json")));
        assert_eq!(create.titles, ["x"], "値を title に数えない");
    }

    /// parent-unrooted と parent-full（create）: 根に着かない付け先・空・台帳に無い付け先を断り、closed と pinned の子は
    /// 数えず、epic の create は溢れで断らない。
    #[test]
    fn hook_graph_guard_create_judges_unrooted_and_full() {
        assert_eq!(run(&mut fixture(3), "bdw create x --parent r", None), "parent-full", "open の子 r.1 r.2 r.e で 3");
        assert_eq!(run(&mut fixture(4), "bdw create x --parent r", None), "", "closed と pinned は数えない");
        assert_eq!(run(&mut fixture(3), "bdw create x --parent r --type epic", None), "");
        assert_eq!(run(&mut fixture(0), "bdw create x --parent r", None), "", "上限 0 は測らない");
        for parent in ["f.1", "f", "\"\"", "gone"] {
            assert_eq!(run(&mut fixture(9), &format!("bdw create x --parent {parent}"), None), "parent-unrooted", "{parent}");
        }
        let mut ledger = fixture(9);
        let found = ledger.apply(&Op::Create { parent: "f.1".to_owned(), kind: String::new() }, &|_| None);
        assert_eq!(found, Err(Refusal::ParentUnrooted { parent: "f.1".to_owned(), top: "f".to_owned() }), "top を名指す");
        assert_eq!(run(&mut fixture(4), "bdw create x --parent r && bdw create y --parent r", None), "parent-full", "効きを足す");
    }

    /// update の付け先: 自身と子孫は parent-loop・根に着く X を根に着かない親へは parent-unrooted・溢れは parent-full・
    /// 空は epic でない X だけ unrooting。根に着かない X を根に着く親へ付け替える書きと epic の付け替えは通る。
    #[test]
    fn hook_graph_guard_update_parent_judges_loop_unrooted_full_and_unrooting() {
        assert_eq!(run(&mut fixture(9), "bdw update r.1 --parent r.1.1", None), "parent-loop");
        assert_eq!(run(&mut fixture(9), "bdw update r.1 --parent r.1", None), "parent-loop");
        assert_eq!(run(&mut fixture(9), "bdw update r.1.1 --parent f.1", None), "parent-unrooted");
        assert_eq!(run(&mut fixture(9), "bdw update f.1 --parent r", None), "", "減る向き");
        assert_eq!(run(&mut fixture(9), "bdw update f --parent r.1", None), "", "top を根へ");
        assert_eq!(run(&mut fixture(3), "bdw update r.1.1 --parent r", None), "parent-full");
        assert_eq!(run(&mut fixture(3), "bdw update r.2 --parent r", None), "", "今の親が P");
        assert_eq!(run(&mut fixture(3), "bdw update r.1.1 --parent r --type epic", None), "", "epic への付け替え");
        assert_eq!(run(&mut fixture(9), "bdw update r.1 --parent \"\"", None), "unrooting");
        assert_eq!(run(&mut fixture(9), "bdw update r.e --parent=", None), "", "epic の親外し");
        let line = "bdw update r.1 --parent r.2 && bdw update r.2 --parent r.1";
        assert_eq!(run(&mut fixture(9), line, None), "parent-loop", "2 つ目は 1 つ目の後の形で測る");
        assert_eq!(run(&mut fixture(9), "bdw update r.2 --parent r.1", None), "", "単独なら通る");
    }

    /// 型・dep remove・数えに戻す書き: 根の epic の型を変える書きと唯一の親の dep remove は unrooting・型を epic にする書き
    /// と親 2 つの片方を外す書きは通る・closed の子の reopen と --status open と --claim は溢れを測り、closed は測らない。
    #[test]
    fn hook_graph_guard_type_dep_and_reopen_follow_the_ratchet() {
        assert_eq!(run(&mut fixture(9), "bdw update r --type feature", None), "unrooting");
        assert_eq!(run(&mut fixture(9), "bdw update r.e -t task", None), "", "根でない epic");
        assert_eq!(run(&mut fixture(9), "bdw update f --type epic", None), "");
        assert_eq!(run(&mut fixture(9), "bdw dep remove r.1 r", None), "unrooting");
        assert_eq!(run(&mut fixture(9), "bdw dep rm r.e r", None), "", "epic");
        assert_eq!(run(&mut fixture(9), "bdw dep rm t2 r.e", None), "", "親 2 つの片方");
        for line in ["bdw reopen r.3", "bdw update r.3 --status open", "bdw update r.4 -s in_progress", "bdw update r.3 --claim"] {
            assert_eq!(run(&mut fixture(3), line, None), "parent-full", "{line}");
            assert_eq!(run(&mut fixture(4), line, None), "", "{line}: 余地あり");
        }
        assert_eq!(run(&mut fixture(3), "bdw update r.1 --claim", None), "", "既に数える子");
        let mut ledger = fixture(3);
        let closed = Op::Update { ids: vec!["r.1".to_owned()], parent: None, kind: None, status: Some("closed".to_owned()) };
        assert_eq!(ledger.apply(&closed, &|_| None), Ok(()), "closed へは測らない");
        assert_eq!(run(&mut ledger, "bdw reopen r.3", None), "", "閉じた効きの後は余地がある");
    }

    /// plan: 親の無い非 epic の node・知らない key・輪は plan-orphan・parent_id が溢れた親なら parent-full・根に着かない
    /// parent_id は parent-unrooted・読めない file は plan-unreadable・親の無い epic の下の node は通る。
    #[test]
    fn hook_graph_guard_plan_judges_orphans_and_parent_ids() {
        let node = |key: &str, kind: &str, parent: &str| format!("{{\"key\":\"{key}\",\"type\":\"{kind}\",{parent}}}");
        let plan = |nodes: &[String]| format!("{{\"nodes\":[{}],\"edges\":[]}}", nodes.join(","));
        let line = "bdw create --graph p.json --parent r";
        let orphan = plan(&[node("a", "task", "\"title\":\"x\"")]);
        assert_eq!(run(&mut fixture(9), line, Some(&orphan)), "plan-orphan");
        for parent in ["\"parent_key\":\"gone\"", "\"parent_key\":\"a\""] {
            assert_eq!(run(&mut fixture(9), line, Some(&plan(&[node("a", "task", parent)]))), "plan-orphan", "{parent}");
        }
        let looped = plan(&[node("a", "task", "\"parent_key\":\"b\""), node("b", "task", "\"parent_key\":\"a\"")]);
        assert_eq!(run(&mut fixture(9), line, Some(&looped)), "plan-orphan");
        let rooted = plan(&[node("e", "epic", "\"title\":\"e\""), node("a", "task", "\"parent_key\":\"e\"")]);
        assert_eq!(run(&mut fixture(9), line, Some(&rooted)), "");
        let two = plan(&[node("a", "task", "\"parent_id\":\"r\""), node("b", "task", "\"parent_id\":\"r\"")]);
        assert_eq!(run(&mut fixture(4), line, Some(&two)), "parent-full", "同じ P への非 epic の node の本数");
        assert_eq!(run(&mut fixture(5), line, Some(&two)), "");
        let top = plan(&[node("a", "task", "\"parent_id\":\"f.1\"")]);
        assert_eq!(run(&mut fixture(9), line, Some(&top)), "parent-unrooted");
        assert_eq!(run(&mut fixture(9), line, None), "plan-unreadable");
        assert_eq!(run(&mut fixture(9), line, Some("not json")), "plan-unreadable");
    }

    /// 台帳の無い repo と掛からない command は台帳を読まない（読めば unreadable で断られる client で通る）・rules 行の
    /// 欠けは no-rule・parent-full の断り文は子 epic の作り方を名指す。
    #[test]
    fn hook_graph_guard_skips_reads_and_names_the_fix() {
        let nowhere = Path::new("/nonexistent-graph-guard-root");
        let scene = |command: &'static str| Scene { command, root: nowhere, cwd: nowhere, bd: Some("/nonexistent-graph-guard-bd"), rules: None, state_dir: nowhere };
        assert_eq!(decide(&scene("bdw create x --parent r")), LedgerDecision::Allow, ".beads の無い root");
        let embedded = Manifest::embedded().unwrap_or_else(|errors| panic!("{errors:?}"));
        assert!(limits(&embedded).is_ok(), "埋め込みの行 2 本");
        let empty = Manifest::parse("schema = 1\n").unwrap_or_else(|errors| panic!("{errors:?}"));
        assert_eq!(limits(&empty), Err(Refusal::NoRule));
        let full = Refusal::ParentFull { parent: "r".to_owned(), children: 3, max: 3 };
        let line = super::denied_line(&full);
        assert!(line.contains("reason=parent-full（") && line.contains("--type epic --parent r"), "{line}");
        assert_eq!(line.lines().count(), 1);
    }

    /// 読まずに通す書きの列の判定（設計 §20 約束 7）: 付け先の無い型 epic の update（綴り 4 つと `--claim` を併せ持つ形）だけが通り、付け先つき・
    /// 型 task・create・reopen・dep remove・混ざった列は読む側。
    #[test]
    fn hook_graph_copy_passes_unread_only_the_plain_epic_update() {
        let ops = |line: &str| -> Vec<Op> { segments(line).iter().filter_map(|found| op_of(found)).collect() };
        for line in [
            "bdw update F --type epic", "bdw update F -t epic", "bdw update F --type=epic", "bdw update F -t=epic",
            "bdw update F -t=epic --claim", "bdw update F G --type epic --status open",
            "bdw update F --type epic && bdw update G -t epic",
        ] {
            assert!(super::passes_unread(&ops(line)), "{line}: 読まずに通す");
        }
        for line in [
            "bdw update F --type epic --parent E", "bdw update F --type epic --parent=", "bdw update E --type task", "bdw create x --parent F",
            "bdw reopen F", "bdw dep rm F E", "bdw update F --claim", "bdw update F --type epic && bdw create x --parent F.1",
            "bdw update F --type epic && bdw update E --type task",
        ] {
            assert!(!super::passes_unread(&ops(line)), "{line}: 読む");
        }
    }

    /// ledger-timeout の断り文は先読みの口と hook の root を名指し、ほかの断りの直す 1 行は先読みの口を持たない。
    #[test]
    fn hook_graph_copy_timeout_line_names_the_prefetch_and_the_root() {
        let line = super::denied_line(&Refusal::LedgerTimeout { root: "/work/repo".to_owned() });
        assert!(line.contains("reason=ledger-timeout（") && line.contains("scribe2 ledger prefetch --repo /work/repo で写しを作り"), "{line}");
        assert_eq!(line.lines().count(), 1);
        assert!(!super::denied_line(&Refusal::LedgerUnreadable).contains("prefetch"));
    }
}
