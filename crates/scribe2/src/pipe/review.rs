//! 契約の審査の段（`Stage::Reviewed`・設計 docs/design/contract-source.md §4・SRS FR49 / FR9 / AC22）。
//!
//! `pipe intake` の直後に器が lens を **1 回**撃ち、契約とそれが実装する設計の節と要件を読ませて verdict を
//! 採る。verdict は run dir の [`REVIEW_FILE`]（gate の `verdict.json` と同型・tmp → rename の atomic 書き）と
//! `RunStage stage=Reviewed detail=verdict:<PASS|FAIL|INCONCLUSIVE>` に残る。**PASS だけが spawn へ進む**
//! （FAIL / INCONCLUSIVE は終端・`live` は偽）。審査を飛ばす口は無い（`--no-review` を作らない・C16）。
//!
//! lens の口は gate と同じ `--lens <cmd>`（穴は `{contract}` / `{worktree}` の 2 つ・語彙は共有し関数は共有しない）。
//! 審査の材料は run dir の [`REVIEW_DIR`] に**契約の写し**と `{design}` / `{requirements}` の本文（[`DESIGN_FILE`] /
//! [`REQUIREMENTS_FILE`]）として置き、lens には**その写しの path**を `{contract}` で渡す。lens は契約の隣に材料の
//! 2 file が在る周だけ契約審査の雛形（`headless/lens-contract.txt`）を使う（裁定の写し `rulings.txt` と同じ「隣の
//! file」の形・`lens.cmd` の穴は不変・`s2-07l.309` と同型）。gate の周は `<run_dir>/contract.toml` を渡すので
//! 材料は隣に無く、diff の雛形のまま＝1 つの `--lens` が 2 つの段に効く。
//!
//! **偽の PASS を作らない**（AC3 / FR9）。lens が無い・起動できない・出力を読めない・3 値の外はすべて
//! INCONCLUSIVE（終端）。材料の欠け（設計 pointer でない `design`・読めない要件面・要件面に無い id）は
//! 材料の本文に**明示の 1 行**として載せ（C10・空を黙らせない）、判定は lens が持つ。
//!
//! PASS でない判定は**理由の型**（[`FindingKind`]・設計 contract-source.md §22）を持つ。lens が最終行の JSON の
//! `kind` に書く閉じた 6 語を読み、`review.json` に `kind` と `at`（指した場所の列）を任意 field で足し、event の
//! detail を `verdict:<V> kind:<k>` の 2 語にする（`at` は event に載せない・PASS は `verdict:PASS` のまま）。lens の
//! JSON が無い周（器が作る INCONCLUSIVE）と `kind` が無い・語でない周は 7 語目 [`FindingKind::Unparsed`] に倒し、
//! **verdict は lens の値のまま**（理由の欠けを INCONCLUSIVE や `other` に化けさせない・C10）。`pipe report` は
//! event の detail からこの型を数える（[`read_detail`]）。
//!
//! 受付の 2 門（設計 contract-source.md §23・`s2-07l.396`）はこの型と `at` を入力にする: 同型の停止は `review.json` の
//! 判定を [`judgement_of`] で読み（`kind` の無い古い便は `unparsed`）、焼き直しの門は直前の便の指摘に「対応する差分」が
//! 在るかを **kind ごとに 1 関数**（[`unaddressed`]・[`FindingKind`] の網羅 match）で測る。門そのものは受付
//! （`cli/intake.rs`）に在り、ここは判定の読み手と kind ごとの物差しだけを持つ。
//!
//! **Promised の行の審査**（設計 contract-source.md §33 行 ah）: 行が約束の行を持つかは設計 doc の契約表を受付と同じ
//! 読み手（`read_table` → `promises_of`）で読んで知る（契約 file の 9 欄は不変・印を書かない）。約束の行の写し
//! （`n` / `text` / `fixture` / `expect` の 4 欄・`n` の順）を材料の [`PROMISES_FILE`] に置き（持たない行は置かない＝
//! lens の雛形は 1 字も変わらない）、lens の `kind` が 3 語（[`FindingKind::promised`]）の外の FAIL は INCONCLUSIVE に倒す
//! （残りの 3 語は器が受付で測り終えている・fail-closed・`FindingKind` の 7 語は不変）。
//!
//! **write-set の base の要約**（設計 contract-source.md §40 行 ao）: 契約の write-set の各項目の base の要約（行数の
//! 2 面・本体の宣言の名・歯の名）を子 module `base` が組み、材料の [`BASE_FILE`] として既存の 3 本と同じ [`keep`] の
//! loop で置く。lens は `{base}` の穴を埋め、要約を足すと cap を越える周は段ごと落とす（[`base_block`]）。観点・理由の
//! 型・既存の 3 材料は不変。

mod base;
mod judgement;
mod requirements;
pub use base::base_block;
pub use judgement::{judgement_of, review_dir, review_path, unaddressed, verdict_of};
pub use judgement::{Judgement, Rework, ROW_SAME_KIND_STOP};
use requirements::requirements_text;
use super::contract::Contract;
use super::gate::{last_json_object, lens_usage, Verdict};
use super::lens_record::LensSource;
use super::{confine, contract_path, emit, record_cost, table, Emit};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::LockPolicy;
use crate::fleet::{cli::now_utc, Cost, CostSource, EventKind, Stage, Usage, SCHEMA};
use crate::polarity::{OnFailure, Polarity, Timing};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// 審査の判定を書く file（gate の `verdict.json` と同型）。
pub const REVIEW_FILE: &str = "review.json";

/// 審査の材料を置く run dir 配下の dir 名（契約の写し + 設計の節 + 要件本文）。
pub const REVIEW_DIR: &str = "review";

/// `{design}` の穴の本文（契約の写しの隣・lens が読む）。
pub const DESIGN_FILE: &str = "design.txt";

/// `{requirements}` の穴の本文（契約の写しの隣・lens が読む）。
pub const REQUIREMENTS_FILE: &str = "requirements.txt";

/// `{promises}` の穴の本文（Promised の行だけ契約の写しの隣に置く・lens が読む）。
pub const PROMISES_FILE: &str = "promises.txt";

/// `{base}` の穴の本文（write-set の各項目の base の要約・契約の写しの隣・lens が読む・§40）。
pub const BASE_FILE: &str = "base.txt";

/// lens の scope の unit 名に載せる段の名。
const REVIEW_STAGE: &str = "review";

/// 書きかけの `review.json` の拡張子（同じ dir に置いて rename する）。
const PARTIAL_EXT: &str = "json.partial";

/// 審査の段の極性（[`ReviewCheck`]・設計 contract-source.md §8）: spawn の**前**に測り、verdict が PASS でない
/// 便と判定を読めない便はどちらも起こさない。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// spawn の入口が読む審査の判定。**bool で持たない**（「通らない」に「PASS でない」と「読めない」の 2 つの意味が
/// あるため・C11.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewCheck {
    /// verdict が PASS＝起こしてよい。
    Passed,
    /// verdict が FAIL / INCONCLUSIVE＝終端（起こさない）。
    Stopped(Verdict),
    /// `review.json` が無い・壊れている・3 値の外＝測れなかった（起こさない）。
    Unreadable,
}

impl ReviewCheck {
    /// run dir の [`REVIEW_FILE`] から判定する（**判定の読み手はこの 1 本**）。
    pub fn judge(state_dir: &Path, id: &str) -> Self {
        match verdict_of(state_dir, id) {
            Some(Verdict::Pass) => Self::Passed,
            Some(found) => Self::Stopped(found),
            None => Self::Unreadable,
        }
    }

    /// 起こしてよいか。**bool はここ 1 本で enum から導く**（読めない周は偽）。
    pub fn is_clear(self) -> bool {
        matches!(self, Self::Passed)
    }

    /// 便が live（終端でない）か。PASS は spawn へ進める側・FAIL / INCONCLUSIVE は終端・読めない周は `None`
    /// （呼び手が断る側へ倒す・`pipe::cli::live` と同じ極性）。
    pub fn live(self) -> Option<bool> {
        match self {
            Self::Passed => Some(true),
            Self::Stopped(_) => Some(false),
            Self::Unreadable => None,
        }
    }

    /// 断りの理由に載せる字面（verdict の 3 値か「読めない」）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passed => Verdict::Pass.as_str(),
            Self::Stopped(found) => found.as_str(),
            Self::Unreadable => "読めない",
        }
    }
}

/// 審査が PASS にならなかった理由の閉じた型（設計 contract-source.md §22・`s2-07l.395`）。
///
/// 先頭の 6 語は lens が最終行の JSON の `kind` に書く語彙（`headless/lens-contract.txt`）。7 語目
/// [`Self::Unparsed`] は**器が倒す側**——lens の JSON が無い周（`--lens` 無し・写しを読めない・起動できない・
/// 出力を読めない・scope の中で死んだ）と、JSON は在るが `kind` が無い・語でない周。理由の欠けを `other` に
/// 化けさせない（C10）ので lens の語彙には含めない。型は理由の語彙であって閾値でも極性でもない＝rules 行に
/// 置かない（閉じた enum の領分）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingKind {
    /// verify の歯が write-set の外の file に在る。
    TeethOutsideWriteSet,
    /// goal と done が矛盾する。
    GoalDoneContradiction,
    /// 歯が空虚で done を測れない。
    VacuousAssert,
    /// 契約の字面が設計の節・要件の現物と合わない。
    LiteralMismatch,
    /// 設計の節や要件の材料が欠けている。
    SectionMaterialMissing,
    /// 上のどれでもない（lens が判断した「その他」）。
    Other,
    /// lens の JSON が無い・`kind` が無い・語でない（器が倒す 7 語目）。
    Unparsed,
}

/// [`FindingKind`] の全 variant（宣言順・`pipe report` の `by_kind=` はこの順で 7 語とも出す）。
pub const FINDING_KINDS: &[FindingKind] = &[
    FindingKind::TeethOutsideWriteSet,
    FindingKind::GoalDoneContradiction,
    FindingKind::VacuousAssert,
    FindingKind::LiteralMismatch,
    FindingKind::SectionMaterialMissing,
    FindingKind::Other,
    FindingKind::Unparsed,
];

impl FindingKind {
    /// Promised の行（§33 行 ah）で lens が書いてよい理由の型か（`vacuous-assert` / `goal-done-contradiction` / `other` の
    /// 3 語・網羅 match）。`teeth-outside-write-set` / `literal-mismatch` / `section-material-missing` は器が受付で測り終えて
    /// いるので、この 3 語の外の FAIL は INCONCLUSIVE に倒す（[`narrow`]）。器が倒す `unparsed` も外。
    pub fn promised(self) -> bool {
        match self {
            Self::VacuousAssert | Self::GoalDoneContradiction | Self::Other => true,
            Self::TeethOutsideWriteSet | Self::LiteralMismatch | Self::SectionMaterialMissing | Self::Unparsed => false,
        }
    }

    /// `review.json` の `kind`・event の `kind:<k>`・report の `by_kind=` に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TeethOutsideWriteSet => "teeth-outside-write-set",
            Self::GoalDoneContradiction => "goal-done-contradiction",
            Self::VacuousAssert => "vacuous-assert",
            Self::LiteralMismatch => "literal-mismatch",
            Self::SectionMaterialMissing => "section-material-missing",
            Self::Other => "other",
            Self::Unparsed => "unparsed",
        }
    }

    /// 字面から引く。7 語の外は `None`（＝呼び手が [`Self::Unparsed`] へ倒す）。
    pub fn parse(text: &str) -> Option<Self> {
        FINDING_KINDS.iter().copied().find(|found| found.as_str() == text)
    }
}

/// event の detail の `kind:` の頭（書き手 [`detail_of`] と読み手 [`read_detail`] の同じ 1 つ）。
const KIND_HEAD: &str = "kind:";

/// event の detail の `verdict:` の頭。
const VERDICT_HEAD: &str = "verdict:";

/// `RunStage stage=Reviewed` の detail の字面: `verdict:<V>`（PASS）か `verdict:<V> kind:<k>`（PASS でない）。
/// **`at` は載せない**——`,` 区切りの語の列を空白区切りの detail に置くと [`read_detail`] の token の読みと衝突する。
fn detail_of(verdict: Verdict, kind: Option<FindingKind>) -> String {
    match kind {
        Some(found) => format!("{VERDICT_HEAD}{} {KIND_HEAD}{}", verdict.as_str(), found.as_str()),
        None => format!("{VERDICT_HEAD}{}", verdict.as_str()),
    }
}

/// `Reviewed` の detail から (verdict, 理由の型) を読む（`pipe report` の母集団の読み手・[`detail_of`] の対）。
/// verdict が 3 値の外・無い周は `None`（呼び手は「PASS でない」側に数える＝壊れた detail を PASS に化けさせない）。
/// `kind:` を持たない古い event と語でない `kind:` は [`FindingKind::Unparsed`]。
pub fn read_detail(detail: &str) -> (Option<Verdict>, FindingKind) {
    let token = |head: &str| detail.split_whitespace().find_map(|word| word.strip_prefix(head));
    let verdict = token(VERDICT_HEAD).and_then(Verdict::parse);
    let kind = token(KIND_HEAD).and_then(FindingKind::parse).unwrap_or(FindingKind::Unparsed);
    (verdict, kind)
}

/// 審査 1 回の材料。
pub struct Review<'a> {
    /// 便 id。
    pub run: &'a str,
    /// 契約の bead id。
    pub bead: &'a str,
    /// 対象 repo（設計 doc と要件面を読む base・lens の cwd）。
    pub repo: &'a Path,
    /// 置き場。
    pub state_dir: &'a Path,
    /// 読み込み済みの契約。
    pub contract: &'a Contract,
    /// 要件面の repo 相対 path（宣言 `requirements`・無ければ既定）。
    pub requirements: &'a str,
    /// lens のコマンドの出所（`--lens`・無い / 読めないは別の値＝どちらも INCONCLUSIVE・[`super::lens_record`]）。
    pub lens: &'a LensSource,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// `{design}` / `{requirements}` の穴の本文。
struct Material {
    /// 契約が実装する設計の節の本文（pointer でない周は明示の 1 行）。
    design: String,
    /// `req` の各 id の要件本文（読めない周は明示の 1 行）。
    requirements: String,
    /// 約束の行の写し（[`promises_text`]・Promised でない行は空）。
    promises: String,
    /// write-set の各項目の base の要約（読めない項目は明示の 1 行・§40）。
    base: String,
}

/// 審査の判定 1 件（verdict と根拠と、PASS でない周の理由の型と場所）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Finding {
    /// 3 値。
    verdict: Verdict,
    /// 根拠の 1 行（lens の `evidence`・器が作る INCONCLUSIVE はその理由）。
    evidence: String,
    /// 理由の型。**PASS の周は `None`**（PASS に理由の型は無い・空の値を作らない）・PASS でない周は必ず `Some`。
    kind: Option<FindingKind>,
    /// lens が `at` に書いた指した場所の列（`,` 区切りの語・書いた周だけ・PASS の周は `None`）。
    at: Option<String>,
}

impl Finding {
    /// 器が作る INCONCLUSIVE（lens の判定に届かなかった周・理由の型は [`FindingKind::Unparsed`]）。
    fn inconclusive(evidence: String) -> Self {
        Self { verdict: Verdict::Inconclusive, evidence, kind: Some(FindingKind::Unparsed), at: None }
    }
}

/// 審査を 1 回通す。
pub fn review(entry: &Review<'_>) -> Outcome {
    let material = materials(entry);
    let contract = match keep(entry, &material) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let (finding, scope, usage) = decide(entry, &contract);
    let finding = narrow(finding, !material.promises.is_empty());
    let verdict = finding.verdict;
    // **審査の lens の消費は判定を書く周に 1 件**（`Reviewed` の前・設計 gate-cost.md §26 形 (2)）。揃わない周は書かず、
    // 書けない周も判定と rc は変えない。
    let cost = usage.map(|found| Cost { source: CostSource::Review, usage: found });
    let noted = record_cost(entry.state_dir, (entry.run, entry.bead), cost, entry.policy);
    match settle(entry, &finding, scope) {
        Err(reason) => broken(reason),
        Ok(()) => Outcome {
            out: vec![format!("run={} stage={} verdict={}", entry.run, Stage::Reviewed.as_str(), verdict.as_str())],
            err: noted.into_iter().collect(),
            rc: verdict.rc(),
        },
    }
}

/// 材料を base から読む（読めなさは本文の明示の 1 行にする・C10）。
fn materials(entry: &Review<'_>) -> Material {
    Material {
        design: design_text(entry.repo, &entry.contract.design),
        requirements: requirements_text(entry.repo, entry.requirements, &entry.contract.req),
        promises: promises_text(entry.repo, &entry.contract.design),
        base: base::base_text(entry.repo, &entry.contract.write_set),
    }
}

/// 契約の `design` が設計 pointer なら、設計 doc の契約表からその行の約束の行を読み写しに組む（受付と同じ読み手
/// `read_table` → `promises_of`）。pointer でない・読めない・約束の行を持たない周は空（読めなさは [`design_text`] が
/// `{design}` の本文に明示する）。
fn promises_text(repo: &Path, design: &str) -> String {
    let Ok(pointer) = table::parse_pointer(design) else {
        return String::new();
    };
    let Ok(text) = table::read(repo, &pointer.path) else {
        return String::new();
    };
    let (_, promises) = table::read_table(&pointer.path, &text).unwrap_or_default();
    render_promises(&table::promises_of(&promises, &pointer.id))
}

/// 約束の行の写し: `n` の順に 1 行ずつ `n` / `text` / `fixture` / `expect` の 4 欄（約束の行が無ければ空）。
fn render_promises(promises: &[&table::PromiseRow]) -> String {
    let mut sorted = promises.to_vec();
    sorted.sort_by_key(|promise| promise.n);
    sorted
        .iter()
        .map(|promise| {
            format!(
                "- n: {}\n  text: {}\n  fixture: {}\n  expect: {}",
                promise.n, promise.text, promise.fixture, promise.expect
            )
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Promised の行の kind の絞り（§33 行 ah (iii)）: 約束の行を持つ行の FAIL で `kind` が 3 語（[`FindingKind::promised`]）の外なら
/// INCONCLUSIVE に倒す（`kind` は lens の値のまま・理由を evidence の頭に足す）。PASS・INCONCLUSIVE・約束の行を持たない
/// 行は不変。
fn narrow(finding: Finding, promised: bool) -> Finding {
    match finding.kind {
        Some(kind) if promised && finding.verdict == Verdict::Fail && !kind.promised() => Finding {
            verdict: Verdict::Inconclusive,
            evidence: format!("Promised の行の kind {} は 3 語の外（{}）", kind.as_str(), finding.evidence),
            ..finding
        },
        _ => finding,
    }
}

/// 材料 1 本の on-disk の形（本文 + 末尾の改行）。[`keep`] が書く側・[`design_material`] が突き合わせる側の
/// **同じ 1 本**（末尾の整え方を 2 か所に持たない・C2）。
fn material_file(body: &str) -> String {
    format!("{body}\n")
}

/// 行の `section` が指す § の本文を、審査の材料の dir に置かれる [`DESIGN_FILE`] と**同じ形**で読む
/// （設計 dispatcher.md §16「列外の鍵に審査役へ渡る材料を含める」）。列は直前の便の写しとこの値を突き合わせ、
/// 違う周は列外にしない。読み手は審査が材料を作る [`design_text`] そのもの（site を 2 つにしない・C2）。
pub(in crate::pipe) fn design_material(repo: &Path, design: &str) -> String {
    material_file(&design_text(repo, design))
}

/// 契約の `design` が設計 pointer（`<doc>#<id>`）なら、base の設計 doc からその行の `section` の節の本文を読む
/// （§4「順序」: 生成 (b) の前後で穴の出所は変わらない）。pointer でない周・解けない周は理由の 1 行。
fn design_text(repo: &Path, design: &str) -> String {
    let pointer = match table::parse_pointer(design) {
        Ok(found) => found,
        Err(error) => return format!("（設計の節なし: design={design} は設計 pointer でない・{}）", error.reason()),
    };
    let text = match table::read(repo, &pointer.path) {
        Ok(found) => found,
        Err(reason) => return format!("（設計の節を読めない: {reason}）"),
    };
    let row = match table::find_row(&pointer.path, &text, &pointer.id) {
        Ok(found) => found,
        Err(errors) => {
            let reasons: Vec<String> = errors.iter().map(|error| error.reason()).collect();
            return format!("（契約表の行 {} を読めない: {}）", pointer.id, reasons.join(" / "));
        }
    };
    let body = section_text(&text, &row.section);
    if body.trim().is_empty() {
        return format!("（設計 doc {} の節 {} が無いか空）", pointer.path, row.section);
    }
    format!("{}#{} §{}\n{body}", pointer.path, pointer.id, row.section)
}

/// 節 `number` の本文（`## N.` の見出しの次の行から次の `## ` 見出しの前まで・契約表の区間と fence の中の
/// `## ` は見出しに数えない・`pipe::table` の節の読みと同じ形）。無ければ空。
fn section_text(doc: &str, number: &str) -> String {
    let mut found: Vec<&str> = Vec::new();
    let (mut fenced, mut inside, mut open) = (false, false, false);
    for line in doc.lines() {
        let trimmed = line.trim();
        if trimmed == table::BEGIN {
            inside = true;
            continue;
        }
        if trimmed == table::END {
            inside = false;
            continue;
        }
        if inside {
            continue;
        }
        if trimmed.starts_with("```") {
            fenced = !fenced;
        }
        match line.strip_prefix("## ").filter(|_| !fenced) {
            Some(title) => open = section_number(title).as_deref() == Some(number),
            None if open => found.push(line),
            None => {}
        }
    }
    found.join("\n")
}

/// `## N. …` の N（数字の列だけ・それ以外は `None`）。
fn section_number(title: &str) -> Option<String> {
    let (head, _) = title.split_once('.')?;
    (!head.is_empty() && head.chars().all(|found| found.is_ascii_digit())).then(|| head.to_owned())
}

/// 材料を run dir の [`REVIEW_DIR`] へ置き、lens に渡す契約の写しの path を返す。書けない周は `Err`（判定に届かない）。
fn keep(entry: &Review<'_>, material: &Material) -> Result<PathBuf, String> {
    let dir = review_dir(entry.state_dir, entry.run);
    std::fs::create_dir_all(&dir).map_err(|err| format!("{} を作れない: {err}", dir.display()))?;
    let contract = dir.join(super::CONTRACT_FILE);
    std::fs::copy(contract_path(entry.state_dir, entry.run), &contract)
        .map_err(|err| format!("{} を写せない: {err}", contract.display()))?;
    for (name, body) in
        [(DESIGN_FILE, &material.design), (REQUIREMENTS_FILE, &material.requirements), (BASE_FILE, &material.base)]
    {
        let path = dir.join(name);
        std::fs::write(&path, material_file(body)).map_err(|err| format!("{} を書けない: {err}", path.display()))?;
    }
    // 約束の行の写しは Promised の行だけ置く（無い file ＝ lens の雛形は 1 字も変わらない）。
    if !material.promises.is_empty() {
        let path = dir.join(PROMISES_FILE);
        std::fs::write(&path, material_file(&material.promises))
            .map_err(|err| format!("{} を書けない: {err}", path.display()))?;
    }
    Ok(contract)
}

/// lens の cmd（在る周）か、lens に届かない理由の INCONCLUSIVE。**無いと読めないは別の理由**（設計 pipeline.md
/// §26・C10・gate の判定順と同じ 3 値の match）。どちらも lens の JSON が無い周なので理由の型は 7 語目。
fn lens_cmd(source: &LensSource) -> Result<&str, Finding> {
    match source {
        LensSource::Cmd(cmd) => Ok(cmd.as_str()),
        LensSource::Absent => Err(Finding::inconclusive("lens が要るのに --lens が無い".to_owned())),
        LensSource::Unreadable { path, reason } => {
            Err(Finding::inconclusive(format!("lens の写し {} を読めない（{reason}）", path.display())))
        }
    }
}

/// lens を 1 回撃って判定を得る（**wildcard 無し・判定に届かない周は INCONCLUSIVE**）。
///
/// 2 つ目は lens の scope を片付けた結果（record に書く周だけ `Some`）・3 つ目は lens の claude の消費の 6 値（lens が
/// rc 0 で終わり判定 object が運んだ周だけ `Some`＝gate の lens と同じ読み [`lens_usage`]）。
fn decide(entry: &Review<'_>, contract: &Path) -> (Finding, Option<confine::Released>, Option<Usage>) {
    let cmd = match lens_cmd(entry.lens) {
        Ok(found) => found,
        Err(finding) => return (finding, None, None),
    };
    // **渡すのは path であって本文ではない**（cmd は `sh -c` の 1 行）。穴は gate と同じ 2 つで、`{worktree}` は
    // 便の worktree がまだ無いので base の repo（lens が憲法を読む cwd）を置く。**1 走査で埋める**。
    let line = crate::headless::fill(
        cmd,
        &[("{contract}", &contract.display().to_string()), ("{worktree}", &entry.repo.display().to_string())],
    );
    let unit = confine::unit_name(entry.run, REVIEW_STAGE, 1);
    // 審査の lens の箱は 1 × `gate.job_memory_mb`（設計 gate-cost.md §12）。
    let wrap = confine::Wrap { unit: &unit, limit: confine::Limit::PerJob(1), caps: confine::Caps::embedded() };
    let (mut command, confinement) = confine::wrap_line(&line, &wrap);
    // diff は無い（stdin は piped のまま閉じる＝lens は EOF を見る）。stderr は捨てる（gate と同じ）。
    let spawned = command
        .current_dir(entry.repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return (Finding::inconclusive(format!("lens を起動できない: {err}")), None, None),
    };
    drop(child.stdin.take());
    let waited = child.wait_with_output();
    let scope = confine::release_scope(&confinement);
    let usage = waited
        .as_ref()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| lens_usage(&String::from_utf8_lossy(&out.stdout)));
    (lens_outcome(waited, &confinement), scope, usage)
}

/// 終わった lens の出力から判定を読む（箱の中の死 → rc → 最後の JSON 行の順・gate の lens と同じ極性）。
fn lens_outcome(waited: std::io::Result<std::process::Output>, confinement: &confine::Confinement) -> Finding {
    let out = match waited {
        Ok(found) => found,
        Err(err) => return Finding::inconclusive(format!("lens の出力を読めない: {err}")),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    if confinement.confined() {
        let killed = confine::read_usage(&text).oom_kill.is_some_and(|count| count >= 1).then_some(confine::Reason::OomKill);
        let killed = killed.or_else(|| out.status.code().is_none().then_some(confine::Reason::Signal));
        if let Some(reason) = killed {
            return Finding::inconclusive(format!("lens が scope の中で死んだ（reason={}）", reason.as_str()));
        }
    }
    if !out.status.success() {
        let rc = out.status.code().unwrap_or(-1);
        return Finding::inconclusive(format!("lens が rc {rc} で終わった"));
    }
    parse_lens(&text)
}

/// lens の stdout の最後の JSON 行から 3 値と理由の型を読む。読めない周・3 値の外は INCONCLUSIVE（FR9）。
///
/// PASS でない周の `kind` は閉じた語彙で読み、無い・語でない周は [`FindingKind::Unparsed`] に倒して **verdict は
/// lens の値のまま**（理由の欠けで判定を動かさない・C10）。`at` は文字列で在る周だけ写す。PASS の周は `kind` も
/// `at` も読まない（lens が書いても持たない＝PASS に理由の型は無い）。
fn parse_lens(text: &str) -> Finding {
    let pairs = match last_json_object(text) {
        Ok(parsed) => parsed,
        Err(reason) => return Finding::inconclusive(format!("lens の{reason}")),
    };
    let get = |key: &str| pairs.iter().find(|(found, _)| found == key).and_then(|(_, value)| value.as_str());
    let evidence = get("evidence").unwrap_or_default().to_owned();
    let Some(verdict) = get("verdict").and_then(Verdict::parse) else {
        return Finding::inconclusive("lens の verdict が 3 値でない".to_owned());
    };
    match verdict {
        Verdict::Pass => Finding { verdict, evidence, kind: None, at: None },
        Verdict::Fail | Verdict::Inconclusive => Finding {
            verdict,
            evidence,
            kind: Some(get("kind").and_then(FindingKind::parse).unwrap_or(FindingKind::Unparsed)),
            at: get("at").map(str::to_owned),
        },
    }
}

/// 判定を `review.json` へ atomic に書き、`Reviewed` を 1 件追記する。`kind` と `at` は任意 field（schema 1 のまま・
/// 古い読み手は無視・PASS の周は無い）。
fn settle(entry: &Review<'_>, finding: &Finding, scope: Option<confine::Released>) -> Result<(), String> {
    let mut fields = vec![
        ("schema", Value::Num(SCHEMA)),
        ("run", Value::Str(entry.run.to_owned())),
        ("verdict", Value::Str(finding.verdict.as_str().to_owned())),
        ("evidence", Value::Str(finding.evidence.clone())),
    ];
    if let Some(kind) = finding.kind {
        fields.push(("kind", Value::Str(kind.as_str().to_owned())));
    }
    if let Some(at) = &finding.at {
        fields.push(("at", Value::Str(at.clone())));
    }
    if let Some(released) = scope {
        fields.push(("scope", Value::Str(released.as_str().to_owned())));
    }
    fields.push(("ts", Value::Str(now_utc())));
    let body = json_lite::write_object(&fields);
    write_review(&review_path(entry.state_dir, entry.run), &format!("{body}\n"))?;
    emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Reviewed),
            seat: None,
            pid: None,
            detail: Some(detail_of(finding.verdict, finding.kind)),
        },
        entry.policy,
    )
    .map_err(|err| err.to_string())
}

/// `review.json` を **atomic に**書く（同じ dir の書きかけへ書いて rename・gate の `verdict.json` と同じ形）。
/// 書けなかった周は書きかけを残さない。
fn write_review(path: &Path, text: &str) -> Result<(), String> {
    let partial = path.with_extension(PARTIAL_EXT);
    std::fs::write(&partial, text)
        .and_then(|()| std::fs::rename(&partial, path))
        .map_err(|err| {
            let _ = std::fs::remove_file(&partial);
            format!("{} を書けない: {err}", path.display())
        })
}

/// 対象そのものが壊れている（rc 2・判定を書かない）。
fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}

#[cfg(test)]
use judgement::split_at;
#[cfg(test)]
use requirements::{requirement_md, requirement_row, requirement_yaml, strip_tags, Found};
#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.541
    // flip-check: moved s2-07l.545
    use super::{
        detail_of, judgement_of, lens_cmd, narrow, parse_lens, read_detail, render_promises, requirement_md,
        requirement_row, requirement_yaml, requirements_text, section_text, split_at, strip_tags, unaddressed, verdict_of,
        write_review, Finding, FindingKind, Found, Judgement, ReviewCheck, Rework, FINDING_KINDS,
    };
    use crate::pipe::closure::Source;
    use crate::pipe::gate::Verdict;
    use crate::pipe::lens_record::LensSource;
    use crate::pipe::run_dir;
    use crate::pipe::table::PromiseRow;
    use std::path::{Path, PathBuf};

    /// 歯ごとの空の tmp dir。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pipe-review-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// 節の本文は `## N.` の見出しの次の行から次の `## ` の前まで。fence の中の `## ` と契約表の区間は見出しに
    /// 数えず、無い節は空。
    #[test]
    fn pipe_review_section_text_reads_the_numbered_section_only() {
        let doc = "# t\n\n## 1. one\na\n```\n## 9. fenced\n```\nb\n\n## 2. two\nc\n<!-- contracts:begin -->\n## 3. region\n<!-- contracts:end -->\n";
        assert_eq!(section_text(doc, "1"), "a\n```\n## 9. fenced\n```\nb\n", "節 1 は fence 込みで次の見出しの前まで");
        assert_eq!(section_text(doc, "2"), "c", "節 2 は区間の前まで");
        assert_eq!(section_text(doc, "3"), "", "区間の中の見出しは節ではない");
        assert_eq!(section_text(doc, "9"), "", "fence の中の見出しは節ではない");
    }

    /// 要件本文は `id="<id>"` の行の tag を剥がした字面で、無い id はその旨を行に明示し、読めない要件面は
    /// 理由の 1 行になる。
    #[test]
    fn pipe_review_requirements_text_names_missing_ids_and_unreadable_faces() {
        let html = "<tr id=\"FR1\"><td>one</td><td class=\"x\">two  words</td></tr>\n<tr id='FR2'><td>deux</td></tr>\n";
        assert_eq!(strip_tags("<a href=\"x\">p</a>q<br/>  r"), "p q r");
        assert_eq!(requirement_row(html, "FR1").as_deref(), Some("one two words"));
        assert_eq!(requirement_row(html, "FR2").as_deref(), Some("deux"), "単引用符の id も読む");
        assert_eq!(requirement_row(html, "FR10"), None, "FR1 の行は FR10 に当たらない");
        let repo = scratch("req");
        let _ = std::fs::write(repo.join("srs.html"), html);
        let listed = requirements_text(&repo, "srs.html", &["FR1".to_owned(), "FR3".to_owned()]);
        assert_eq!(listed, "FR1: one two words\nFR3: （要件面 srs.html に無い）");
        assert!(requirements_text(&repo, "absent.html", &["FR1".to_owned()]).starts_with("（要件面を読めない: "));
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// 要件本文の読み手は形ごとに 1 関数で、呼び分けは拡張子の 1 match（設計 §4・`s2-07l.354`）: yaml は id と同じ
    /// mapping の `text:` の値だけ（`title:` へ倒さない・`text:` の前に在っても・block の続きも畳む）、md は見出しの
    /// 下の本文。裸の `- FR1`・`text:` の無い mapping・見出しの下が空の id は「本文が無い」の理由の行、他の拡張子は
    /// 形を読めない理由の 1 行。
    #[test]
    fn pipe_review_requirements_text_reads_yaml_text_and_md_headings_by_extension() {
        let yaml = "requirements:\n  - id: FR1\n    title: 起動\n    text: \"便を  起こす\"\n  - text: |\n      二行の\n      本文\n    id: FR2\n  - FR3\n  - id: FR4\n    title: 題だけ\nother:\n  text: 外の text\n";
        assert_eq!(requirement_yaml(yaml, "FR1"), Found::Body("便を 起こす".to_owned()), "同じ mapping の text だけ");
        assert_eq!(requirement_yaml(yaml, "FR2"), Found::Body("二行の 本文".to_owned()), "id の前の block も読む");
        assert_eq!(requirement_yaml(yaml, "FR3"), Found::Empty, "裸の列は本文なし");
        assert_eq!(requirement_yaml(yaml, "FR4"), Found::Empty, "title へ倒さない・隣の mapping の text を借りない");
        assert_eq!(requirement_yaml(yaml, "FR9"), Found::Absent);
        let md = "# 要件\n\n## FR1 便の起動\n\n便を\n起こす。\n\n## FR2\n\n## FR3 末尾\n";
        assert_eq!(requirement_md(md, "FR1"), Found::Body("便を 起こす。".to_owned()), "次の見出しの前までを 1 行に");
        assert_eq!(requirement_md(md, "FR2"), Found::Empty, "見出しの下が空行だけ");
        assert_eq!(requirement_md(md, "FR3"), Found::Empty, "file 末尾まで空");
        assert_eq!(requirement_md(md, "FR9"), Found::Absent);
        let repo = scratch("faces");
        let _ = std::fs::write(repo.join("reqs.yaml"), yaml);
        let _ = std::fs::write(repo.join("reqs.md"), md);
        let _ = std::fs::write(repo.join("reqs.json"), "{}");
        let ids = ["FR1".to_owned(), "FR3".to_owned(), "FR9".to_owned()];
        assert_eq!(
            requirements_text(&repo, "reqs.yaml", &ids),
            "FR1: 便を 起こす\nFR3: （要件面 reqs.yaml の FR3 に本文が無い）\nFR9: （要件面 reqs.yaml に無い）"
        );
        assert_eq!(
            requirements_text(&repo, "reqs.md", &["FR1".to_owned(), "FR2".to_owned()]),
            "FR1: 便を 起こす。\nFR2: （要件面 reqs.md の FR2 に本文が無い）"
        );
        assert!(requirements_text(&repo, "reqs.json", &ids).starts_with("（要件面 reqs.json の形を読めない: "));
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// (a) `text:` を持たず `shall:` を持つ mapping の本文は `shall:` の値（設計 §32 の約束 2）。
    #[test]
    fn pipe_review_yaml_shall_reads_shall_when_text_is_absent() {
        let yaml = "requirements:\n  - id: FR1\n    title: 起動\n    shall: \"便を  起こす\"\n";
        assert_eq!(requirement_yaml(yaml, "FR1"), Found::Body("便を 起こす".to_owned()), "text が無ければ shall の値");
    }

    /// (f) `shall: |` の block の続きも `text:` と同じ 1 本の形で畳む（設計 §32 の約束 2 の block の面）。
    #[test]
    fn pipe_review_yaml_shall_folds_the_block_continuation() {
        let yaml = "requirements:\n  - shall: |\n      二行の\n      約束\n    id: FR2\n";
        assert_eq!(requirement_yaml(yaml, "FR2"), Found::Body("二行の 約束".to_owned()), "block の続きを空白で畳む");
    }

    /// (b) 同じ mapping に `when:` が在れば本文は「`when:` の値 + 空白 + `—` + 空白 + `shall:` の値」の 1 本
    /// （設計 §32 の約束 3・区切りの字面を逐語で pin し、`when:` の値が落ちれば落ちる）。欄の並びは問わない。
    #[test]
    fn pipe_review_yaml_shall_joins_when_with_an_em_dash() {
        let yaml = "requirements:\n  - id: FR3\n    when: 席が  空いた とき\n    shall: 便を 起こす\n";
        let body = "席が 空いた とき — 便を 起こす".to_owned();
        assert_eq!(requirement_yaml(yaml, "FR3"), Found::Body(body.clone()), "条件と約束を区切りの 1 文字で繋ぐ");
        assert!(body.contains(" — "), "区切りは前後に空白 1 つを伴う em dash 1 文字");
        let swapped = "requirements:\n  - id: FR3\n    shall: 便を 起こす\n    when: 席が  空いた とき\n";
        assert_eq!(requirement_yaml(swapped, "FR3"), Found::Body(body), "yaml の欄の並びに依らない");
    }

    /// (c) `text:` と `shall:`（と `when:`）を両方持つ mapping は `text:` の値だけ（設計 §32 の約束 1 の優先・否定の枝）。
    #[test]
    fn pipe_review_yaml_shall_never_mixes_into_an_existing_text() {
        let yaml = "requirements:\n  - id: FR4\n    when: 条件の字面\n    text: 正本の  本文\n    shall: 約束の字面\n";
        let Found::Body(body) = requirement_yaml(yaml, "FR4") else {
            panic!("text を持つ mapping は本文を持つ");
        };
        assert_eq!(body, "正本の 本文", "text の値だけ");
        assert!(!body.contains("約束の字面"), "shall の値が混ざらない");
        assert!(!body.contains("条件の字面"), "when の値が混ざらない");
        assert!(!body.contains('—'), "区切りも混ざらない");
    }

    /// (d) `when:` だけ・`plain:` だけ・`title:` だけの mapping と裸の列はどれも本文なし（設計 §32 の約束 4 と 5 の
    /// 否定の枝＝`when:` だけで本文を作らず `plain:` を読まない）。
    #[test]
    fn pipe_review_yaml_shall_leaves_when_only_and_plain_without_a_body() {
        let yaml = "requirements:\n  - id: FR5\n    when: 条件だけ\n  - id: FR6\n    plain: 平易な 言い換え\n  - id: FR7\n    title: 題だけ\n  - FR8\n";
        assert_eq!(requirement_yaml(yaml, "FR5"), Found::Empty, "when だけでは本文を作らない");
        assert_eq!(requirement_yaml(yaml, "FR6"), Found::Empty, "plain は読まない");
        assert_eq!(requirement_yaml(yaml, "FR7"), Found::Empty, "title へ倒さない");
        assert_eq!(requirement_yaml(yaml, "FR8"), Found::Empty, "裸の列は本文なし");
    }

    /// (e) id が要件面に無い周は不在の 1 値（設計 §32 の約束 5・「本文が無い」と別の理由）。
    #[test]
    fn pipe_review_yaml_shall_keeps_absent_apart_from_empty() {
        let yaml = "requirements:\n  - id: FR5\n    when: 条件だけ\n  - id: FR1\n    shall: 便を 起こす\n";
        assert_eq!(requirement_yaml(yaml, "FR9"), Found::Absent, "要件面に無い id");
        let repo = scratch("shall");
        let _ = std::fs::write(repo.join("reqs.yaml"), yaml);
        let ids = ["FR1".to_owned(), "FR5".to_owned(), "FR9".to_owned()];
        assert_eq!(
            requirements_text(&repo, "reqs.yaml", &ids),
            "FR1: 便を 起こす\nFR5: （要件面 reqs.yaml の FR5 に本文が無い）\nFR9: （要件面 reqs.yaml に無い）",
            "呼び手の 2 つの理由の行の字面は不変"
        );
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// `review.json` の 3 値は 1 本の読み手で読み、無い / 壊れ / 3 値の外は `Unreadable`（起こさない側）。
    /// `ReviewCheck` の bool は PASS だけが真で、`live` は PASS = 真・他の 2 値 = 偽・読めない = `None`。
    #[test]
    fn pipe_review_check_reads_review_json_fail_closed() {
        let state = scratch("check");
        let dir = run_dir(&state, "r");
        let _ = std::fs::create_dir_all(&dir);
        assert_eq!(ReviewCheck::judge(&state, "r"), ReviewCheck::Unreadable, "file が無い");
        for (body, want) in [
            ("{\"verdict\":\"PASS\"}\n", ReviewCheck::Passed),
            ("{\"verdict\":\"FAIL\"}\n", ReviewCheck::Stopped(Verdict::Fail)),
            ("{\"verdict\":\"INCONCLUSIVE\"}\n", ReviewCheck::Stopped(Verdict::Inconclusive)),
            ("{\"verdict\":\"pass\"}\n", ReviewCheck::Unreadable),
            ("not json\n", ReviewCheck::Unreadable),
        ] {
            assert_eq!(write_review(&dir.join("review.json"), body), Ok(()));
            assert_eq!(ReviewCheck::judge(&state, "r"), want, "{body}");
        }
        assert_eq!(verdict_of(&state, "r"), None, "3 値の外は読めない");
        assert!(ReviewCheck::Passed.is_clear() && !ReviewCheck::Stopped(Verdict::Fail).is_clear());
        assert!(!ReviewCheck::Unreadable.is_clear());
        assert_eq!(ReviewCheck::Passed.live(), Some(true));
        assert_eq!(ReviewCheck::Stopped(Verdict::Inconclusive).live(), Some(false));
        assert_eq!(ReviewCheck::Unreadable.live(), None);
        assert_eq!(ReviewCheck::Unreadable.as_str(), "読めない");
        assert_eq!(ReviewCheck::Stopped(Verdict::Fail).as_str(), "FAIL");
        let _ = std::fs::remove_dir_all(&state);
    }

    /// 理由の型は 7 語が宣言順に並び、字面と往復し、7 語の外は `None`（設計 §22）。lens の語彙は先頭 6 語で、
    /// 7 語目 `unparsed` は末尾。
    #[test]
    fn pipe_review_kind_vocabulary_round_trips_in_declaration_order() {
        let words: Vec<&str> = FINDING_KINDS.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(
            words,
            [
                "teeth-outside-write-set",
                "goal-done-contradiction",
                "vacuous-assert",
                "literal-mismatch",
                "section-material-missing",
                "other",
                "unparsed",
            ],
            "7 語・宣言順"
        );
        for kind in FINDING_KINDS {
            assert_eq!(FindingKind::parse(kind.as_str()), Some(*kind), "{} は往復する", kind.as_str());
        }
        assert_eq!(FindingKind::parse("Other"), None, "大小は区別する");
        assert_eq!(FindingKind::parse(""), None);
        assert_eq!(FINDING_KINDS.last(), Some(&FindingKind::Unparsed), "器が倒す 7 語目は末尾");
    }

    /// lens の JSON の `kind` は PASS でない周だけ読む: 6 語は逐語で残り `at` も写る／無い・語でない周は `unparsed` で
    /// **verdict は lens の値のまま**（FAIL は FAIL・INCONCLUSIVE は INCONCLUSIVE）／PASS は lens が `kind` を書いても
    /// 持たない／JSON が読めない・3 値でない周は器の INCONCLUSIVE（`unparsed`）。
    #[test]
    fn pipe_review_kind_parse_lens_falls_to_unparsed_without_moving_the_verdict() {
        let found = parse_lens(r#"{"verdict":"FAIL","evidence":"e","kind":"literal-mismatch","at":"a.rs,§2"}"#);
        assert_eq!(
            found,
            Finding {
                verdict: Verdict::Fail,
                evidence: "e".to_owned(),
                kind: Some(FindingKind::LiteralMismatch),
                at: Some("a.rs,§2".to_owned()),
            }
        );
        let missing = parse_lens(r#"{"verdict":"FAIL","evidence":"e"}"#);
        assert_eq!((missing.verdict, missing.kind, missing.at), (Verdict::Fail, Some(FindingKind::Unparsed), None));
        let bogus = parse_lens(r#"{"verdict":"INCONCLUSIVE","evidence":"e","kind":"bogus","at":"x"}"#);
        assert_eq!(bogus.verdict, Verdict::Inconclusive, "語でない kind で verdict は動かない");
        assert_eq!(bogus.kind, Some(FindingKind::Unparsed));
        assert_eq!(bogus.at.as_deref(), Some("x"), "at は書かれていれば写す");
        let not_str = parse_lens(r#"{"verdict":"FAIL","evidence":"e","kind":1,"at":2}"#);
        assert_eq!((not_str.kind, not_str.at), (Some(FindingKind::Unparsed), None), "文字列でない kind / at は無いと同じ");
        let passed = parse_lens(r#"{"verdict":"PASS","evidence":"e","kind":"other","at":"x"}"#);
        assert_eq!((passed.verdict, passed.kind, passed.at), (Verdict::Pass, None, None), "PASS に理由の型は無い");
        let unreadable = parse_lens("not json\n");
        assert_eq!((unreadable.verdict, unreadable.kind), (Verdict::Inconclusive, Some(FindingKind::Unparsed)));
        let outside = parse_lens(r#"{"verdict":"MAYBE","kind":"other"}"#);
        assert_eq!((outside.verdict, outside.kind), (Verdict::Inconclusive, Some(FindingKind::Unparsed)));
    }

    /// lens に届かない 2 形（`--lens` 無し・写しを読めない）は理由を分けたまま、理由の型はどちらも `unparsed`。
    #[test]
    fn pipe_review_kind_lens_absent_or_unreadable_is_unparsed() {
        let absent = lens_cmd(&LensSource::Absent).expect_err("無い");
        assert_eq!((absent.verdict, absent.kind, absent.at), (Verdict::Inconclusive, Some(FindingKind::Unparsed), None));
        assert!(absent.evidence.contains("--lens"), "{}", absent.evidence);
        let source = LensSource::Unreadable { path: PathBuf::from("/x/lens.toml"), reason: "壊れ".to_owned() };
        let unreadable = lens_cmd(&source).expect_err("読めない");
        assert_eq!((unreadable.verdict, unreadable.kind), (Verdict::Inconclusive, Some(FindingKind::Unparsed)));
        assert!(unreadable.evidence.contains("/x/lens.toml") && unreadable.evidence.contains("壊れ"), "{}", unreadable.evidence);
        assert_eq!(lens_cmd(&LensSource::Cmd("true".to_owned())), Ok("true"));
    }

    /// event の detail は書き手と読み手で往復する: PASS は 1 語・PASS でない周は `kind:` を足した 2 語。読み手は
    /// `kind:` の無い古い detail と語でない `kind:` を `unparsed` に、3 値の外の verdict を `None` にする。
    #[test]
    fn pipe_review_kind_detail_round_trips_and_reads_legacy_as_unparsed() {
        assert_eq!(detail_of(Verdict::Pass, None), "verdict:PASS");
        assert_eq!(detail_of(Verdict::Fail, Some(FindingKind::Other)), "verdict:FAIL kind:other");
        for kind in FINDING_KINDS {
            let detail = detail_of(Verdict::Inconclusive, Some(*kind));
            assert_eq!(detail.split_whitespace().count(), 2, "{detail}");
            assert_eq!(read_detail(&detail), (Some(Verdict::Inconclusive), *kind), "{detail}");
        }
        assert_eq!(read_detail("verdict:PASS"), (Some(Verdict::Pass), FindingKind::Unparsed));
        assert_eq!(read_detail("verdict:FAIL"), (Some(Verdict::Fail), FindingKind::Unparsed), "古い event");
        assert_eq!(read_detail("verdict:FAIL kind:bogus"), (Some(Verdict::Fail), FindingKind::Unparsed));
        assert_eq!(read_detail("verdict:maybe kind:other"), (None, FindingKind::Other), "3 値の外は None");
        assert_eq!(read_detail(""), (None, FindingKind::Unparsed));
    }

    /// 受付の門が読む判定（§23）: PASS は型を持たず、FAIL / INCONCLUSIVE は `kind` の語を持ち、`kind` の無い古い便と
    /// 語でない周は `unparsed`。`at` は `,` で割って空白を剥がし空を落とす。file が無い・3 値の外は `None`。
    #[test]
    fn pipe_review_judgement_reads_kind_and_splits_at() {
        let state = scratch("judgement");
        let dir = run_dir(&state, "r");
        let _ = std::fs::create_dir_all(&dir);
        assert_eq!(judgement_of(&state, "r"), None, "file が無い");
        for (body, want) in [
            ("{\"verdict\":\"PASS\",\"kind\":\"other\",\"at\":\"x\"}\n", Some(Judgement { verdict: Verdict::Pass, kind: None, at: vec!["x".to_owned()] })),
            (
                "{\"verdict\":\"FAIL\",\"kind\":\"literal-mismatch\",\"at\":\"a.rs, §2,,Marker \"}\n",
                Some(Judgement {
                    verdict: Verdict::Fail,
                    kind: Some(FindingKind::LiteralMismatch),
                    at: vec!["a.rs".to_owned(), "§2".to_owned(), "Marker".to_owned()],
                }),
            ),
            ("{\"verdict\":\"FAIL\"}\n", Some(Judgement { verdict: Verdict::Fail, kind: Some(FindingKind::Unparsed), at: Vec::new() })),
            (
                "{\"verdict\":\"INCONCLUSIVE\",\"kind\":\"bogus\"}\n",
                Some(Judgement { verdict: Verdict::Inconclusive, kind: Some(FindingKind::Unparsed), at: Vec::new() }),
            ),
            ("{\"verdict\":\"maybe\"}\n", None),
        ] {
            assert_eq!(write_review(&dir.join("review.json"), body), Ok(()));
            assert_eq!(judgement_of(&state, "r"), want, "{body}");
        }
        assert_eq!(split_at(""), Vec::<String>::new());
        assert_eq!(split_at(" , "), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&state);
    }

    /// 焼き直しの門の物差し（§23 (3)・kind ごとに 1 関数）: teeth-outside-write-set は write-set に無い path だけ（dir 項目は
    /// 配下を含む）、literal-mismatch は契約か節に残っていて base に解けない識別子だけ（消えた識別子・base に解ける識別子・
    /// 3 形でない字面は対応済み）、section-material-missing は節が不変なら `at` の全部・変われば空、測れない 4 型は常に空。
    /// 返す列は辞書順・重複なし。読めない `.rs` は `Err`。
    #[test]
    fn pipe_review_unaddressed_measures_each_kind_with_one_ruler() {
        let sources = vec![Source {
            path: "src/tint.rs".to_owned(),
            body: Ok("pub enum Tint {\n    Warm,\n}\n\npub const TINTS: &[Tint] = &[Tint::Warm];\n".to_owned()),
        }];
        let write_set = ["src/lib.rs".to_owned(), "docs/".to_owned()];
        let tracked = ["src/lib.rs".to_owned(), "src/tint.rs".to_owned(), "docs/a.md".to_owned()];
        let rework = Rework {
            write_set: &write_set,
            contract: "done = \"Nope::Thing と Tint::Warm と gone を直す\"\n",
            design: "doc#a §1\n節の本文 Marker。\n",
            previous_design: "doc#a §1\n節の本文 Marker。\n",
            touches: &[],
            tracked: &tracked,
            sources: &sources,
        };
        let at = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect::<Vec<String>>();
        assert_eq!(
            unaddressed(FindingKind::TeethOutsideWriteSet, &at(&["tests/z.rs", "src/lib.rs", "docs/deep/b.md", "tests/a.rs", "tests/z.rs"]), &rework),
            Ok(at(&["tests/a.rs", "tests/z.rs"])),
            "write-set に無い path だけ・dir 項目は配下を含む・辞書順・重複なし"
        );
        assert_eq!(
            unaddressed(FindingKind::LiteralMismatch, &at(&["Tint::Warm", "Nope::Thing", "Marker", "vanished(", "src/none.rs"]), &rework),
            Ok(at(&["Nope::Thing"])),
            "残っていて解けない識別子だけ（解ける Tint::Warm・散文 Marker・消えた vanished( / src/none.rs は対応済み）"
        );
        assert_eq!(unaddressed(FindingKind::SectionMaterialMissing, &at(&["§1"]), &rework), Ok(at(&["§1"])), "節が不変");
        let changed = Rework { design: "doc#a §1\n節の本文を改めた。\n", ..rework };
        assert_eq!(unaddressed(FindingKind::SectionMaterialMissing, &at(&["§1"]), &changed), Ok(Vec::new()), "節が変わった");
        for kind in [FindingKind::GoalDoneContradiction, FindingKind::VacuousAssert, FindingKind::Other, FindingKind::Unparsed] {
            assert_eq!(unaddressed(kind, &at(&["Nope::Thing"]), &rework), Ok(Vec::new()), "{} は測れない＝通す", kind.as_str());
        }
        assert_eq!(unaddressed(FindingKind::LiteralMismatch, &[], &rework), Ok(Vec::new()), "at が空なら通す");
        let broken = vec![Source { path: "src/x.rs".to_owned(), body: Err("bad".to_owned()) }];
        let unreadable = Rework { sources: &broken, ..rework };
        assert!(unaddressed(FindingKind::LiteralMismatch, &at(&["Nope::Thing"]), &unreadable).is_err(), "読めない .rs は Err");
        assert_eq!(unaddressed(FindingKind::TeethOutsideWriteSet, &at(&["tests/a.rs"]), &unreadable), Ok(at(&["tests/a.rs"])), "path の物差しは .rs を読まない");
    }

    /// teeth-outside-write-set の物差しは path の形の項目だけを測る（§35）: 母集団 3（path 1・歯の接頭辞・§ の番号）で、
    /// path が write-set に無ければ path だけを名指し末尾に「測った 1 件・測れない 2 件」、在れば空＝通す。path の項目が
    /// 0 の周も空。tracked の file は `/` を持たなくても path、`+` 付きの新規 file と末尾 `/` の dir も path。
    #[test]
    fn pipe_review_unaddressed_teeth_measures_only_path_shaped_items() {
        let at = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect::<Vec<String>>();
        let mixed = at(&["headless_lens_promise_", "src/other.rs", "§33"]);
        let tracked = at(&["src/lib.rs", "src/other.rs", "Cargo.toml"]);
        let narrow = at(&["src/lib.rs"]);
        let rework = Rework {
            write_set: &narrow,
            contract: "",
            design: "",
            previous_design: "",
            touches: &[],
            tracked: &tracked,
            sources: &[],
        };
        assert_eq!(
            unaddressed(FindingKind::TeethOutsideWriteSet, &mixed, &rework),
            Ok(at(&["src/other.rs", "測った 1 件・測れない 2 件"])),
            "path の項目だけを名指し、測れない 2 件は数で出す"
        );
        let widened = at(&["src/lib.rs", "src/other.rs"]);
        let passed = Rework { write_set: &widened, ..rework };
        assert_eq!(unaddressed(FindingKind::TeethOutsideWriteSet, &mixed, &passed), Ok(Vec::new()), "path を足せば通す");
        assert_eq!(
            unaddressed(FindingKind::TeethOutsideWriteSet, &at(&["headless_lens_promise_", "§33", "Refuse::Other"]), &rework),
            Ok(Vec::new()),
            "path の項目が 0 なら測れない＝通す"
        );
        assert_eq!(
            unaddressed(FindingKind::TeethOutsideWriteSet, &at(&["Cargo.toml", "+tests/new.rs", "docs/"]), &rework),
            Ok(at(&["+tests/new.rs", "Cargo.toml", "docs/"])),
            "tracked の file・+ 付きの新規 file・末尾 / の dir は path（測れない項目が無ければ数を足さない）"
        );
    }

    /// 約束の 1 行（`n` と 4 欄の字面を `n` から作る・`files` / `teeth` は写しに載らない面）。
    fn promise(n: u64) -> PromiseRow {
        PromiseRow {
            line: n,
            of: "ah".to_owned(),
            n,
            text: format!("約束 {n}"),
            files: vec![format!("src/f{n}.rs")],
            symbols: Vec::new(),
            teeth: vec![format!("tooth_{n}")],
            place: String::new(),
            fixture: format!("fixture {n}"),
            expect: format!("expect {n}"),
        }
    }

    /// (e) 約束の行の写しは `n` の順（doc 順が 2 → 1 でも）に 4 欄（n / text / fixture / expect）だけを載せ、`files` と
    /// `teeth` は載せない。約束の行が無ければ空（＝材料の file を置かず雛形は 1 字も変わらない）。
    #[test]
    fn contract_promise_review_renders_four_fields_in_n_order() {
        let (one, two) = (promise(1), promise(2));
        let rendered = render_promises(&[&two, &one]);
        assert_eq!(
            rendered,
            "- n: 1\n  text: 約束 1\n  fixture: fixture 1\n  expect: expect 1\n- n: 2\n  text: 約束 2\n  fixture: fixture 2\n  expect: expect 2"
        );
        assert!(!rendered.contains("src/f1.rs") && !rendered.contains("tooth_1"), "4 欄の外は載せない: {rendered}");
        assert_eq!(render_promises(&[]), "", "約束の行が無ければ空");
    }

    /// (e) Promised の行の FAIL は kind が 3 語（母集団 3・宣言順）の中ならそのまま、外（残りの 3 語と `unparsed`）なら
    /// INCONCLUSIVE に倒れ kind は lens の値のまま。PASS・INCONCLUSIVE と約束の行を持たない行の判定は 7 語とも不変。
    #[test]
    fn contract_promise_review_kind_outside_three_falls_to_inconclusive() {
        let inside: Vec<FindingKind> = FINDING_KINDS.iter().copied().filter(|kind| kind.promised()).collect();
        assert_eq!(
            inside,
            [FindingKind::GoalDoneContradiction, FindingKind::VacuousAssert, FindingKind::Other],
            "母集団 3 語（宣言順）"
        );
        let fail = |kind: FindingKind| Finding { verdict: Verdict::Fail, evidence: "e".to_owned(), kind: Some(kind), at: Some("x".to_owned()) };
        let mut fallen = 0;
        for kind in FINDING_KINDS {
            let found = narrow(fail(*kind), true);
            if kind.promised() {
                assert_eq!(found, fail(*kind), "{} は 3 語の中＝そのまま", kind.as_str());
            } else {
                fallen += 1;
                assert_eq!(found.verdict, Verdict::Inconclusive, "{} は 3 語の外", kind.as_str());
                assert_eq!((found.kind, found.at.as_deref()), (Some(*kind), Some("x")), "kind と at は lens の値のまま");
                assert!(found.evidence.contains(kind.as_str()) && found.evidence.ends_with("（e）"),"{}", found.evidence);
            }
            assert_eq!(narrow(fail(*kind), false), fail(*kind), "{} は約束の行を持たない行で不変", kind.as_str());
            let unsure = Finding { verdict: Verdict::Inconclusive, ..fail(*kind) };
            assert_eq!(narrow(unsure.clone(), true), unsure, "INCONCLUSIVE は不変");
        }
        assert_eq!(fallen, 4, "3 語の外は 7 語のうち 4 語");
        let passed = Finding { verdict: Verdict::Pass, evidence: "e".to_owned(), kind: None, at: None };
        assert_eq!(narrow(passed.clone(), true), passed, "PASS は不変");
    }

    /// 書けない周は本 file が生まれず書きかけも残さない（親 dir が無い）。
    #[test]
    fn pipe_review_write_failure_leaves_no_partial() {
        let dir = scratch("partial");
        let absent = dir.join("absent").join("review.json");
        assert!(write_review(&absent, "{}\n").is_err());
        assert!(!absent.exists() && !Path::new(&absent.with_extension("json.partial")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
