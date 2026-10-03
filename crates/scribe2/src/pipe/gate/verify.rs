//! gate の verify 行の実行（段の列 [`Check`] と、行を撃つ 1 本 [`run_line_captured`]・
//! [`super`] から純移動・`s2-07l.286`）。判定の順と終端は親（[`super::gate`]）が持つ。

use super::record::{excerpt_of, Failed, USAGE_HEAD};
use super::{UNADMITTED_JOBS, WRITE_SET_CMD};
use crate::pipe::admission::{self, Grant};
use crate::name::NAME;
use crate::pipe::closure::{self, selects, tooth_sites, Base, ClosureError, Source};
use crate::pipe::confine::{self, Confinement, Reason, Released, Usage};
use crate::pipe::contract::{done_teeth_of, Contract};
use crate::pipe::declaration::{fixed_roots, with_fixed, RootsAtHead, BASE_HOLE, JOBS_HOLE, TEETH_HOLE, THREADS_HOLE};
use crate::pipe::git_bytes;
use crate::pipe::health;
use crate::pipe::refuse::{self, DELETE_FILE, NEW_FILE};
use crate::pipe::table::{self, parse_element, Tooth};
use crate::seat::RuleRead;
use std::path::Path;

/// 受付を通らない行（land の main 実測・受付の無い呼び手）の `{threads}` の実値（設計 gate-cost.md §31 約束 6）。
///
/// jobs と同じく **1**（[`UNADMITTED_JOBS`]）——受付を通らない周に core 数ぶんの thread を許さない。
const UNADMITTED_THREADS: u64 = UNADMITTED_JOBS;

/// 機械検証の段。**適用順序は [`CHECKS`] の並びが唯一の権威**である（憲法 C2）。
///
/// 順序を散文の注記で持たないための形である——enum が段の集合を閉じ、[`run_checks`] の
/// 網羅 match が新しい variant を必ずこの並びへ置かせる（置き忘れは compile error）。
/// 閉じた enum と全 variant の並びを対で持つのは器の既定の形である（[`super::VERDICTS`] /
/// `rules::ALL` と同型）。並びが宣言順のままであることは
/// [`crate::order::is_declaration_order`] を通す歯が測る（ADR-0013 §2.2・限界は §2.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// diff が契約の write-set の内に収まっているか（ADR-0009 §2.4）。
    WriteSet,
    /// 便の写し `vessel.toml` の共通 verify（`{base}` を置換して撃つ）。
    Common,
    /// 便の写しの検出線（`detection-verify`・穴は共通 verify と同じ・設計 gate-cost.md §5）。
    Detection,
    /// 契約の verify（穴を持たない）。
    Contract,
}

/// [`Check`] の全 variant。**この並びが適用順序である**。
pub const CHECKS: &[Check] = &[Check::WriteSet, Check::Common, Check::Detection, Check::Contract];

/// gate・land の主実測・候補の木が撃つ段の列（[`CHECKS`] から ③ を除いた ①②④・宣言順のまま・設計 gate-cost.md
/// §44 形 (9)）。
///
/// ③ は着地後の検出の口だけが撃つ（[`run_detection_admitted`]）。部分集合を `&[Check]` の const で書かず関数で組むのは、
/// 閉じた enum の const slice は全 variant を持つ形だけにするためである（xtask の enum-slices の門）。
pub(super) fn gate_checks() -> Vec<Check> {
    CHECKS.iter().copied().filter(|check| *check != Check::Detection).collect()
}

impl Check {
    /// 段の名（scope の unit 名と record の `kind` に載せる字面）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WriteSet => WRITE_SET_CMD,
            Self::Common => "common",
            Self::Detection => "detection",
            Self::Contract => "contract",
        }
    }
}

/// 撃った 1 段の結果。
pub struct Step {
    /// どの段か（record の `kind=`）。
    pub stage: Check,
    /// 実行した行の字面（置換後）。write-set 照合は [`WRITE_SET_CMD`]。
    pub cmd: String,
    /// process の rc（起動できない周は -1）。
    pub rc: i32,
    /// stderr の写し（落ちた歯の区間 + 末尾・[`excerpt_of`]・緑の段は空）。
    pub stderr: String,
    /// nextest 形の stderr で落ちた歯（record の `failed=` / `failed_stderr=`・`FAIL [` の行が無い周は `None`）。
    pub failed: Option<Failed>,
    /// cgroup の scope で包めたか（record の `confined=`・設計 gate-cost.md §4）。
    pub confined: bool,
    /// 包めなかった理由 か 箱の中で起きたこと（record の `reason=`・閉じた enum）。
    pub reason: Option<Reason>,
    /// scope の peak（MiB）。**読めない周は `None`**＝record は `-`（0 と書かない）。
    pub peak_mb: Option<u64>,
    /// 段の壁時計（秒・record の `secs=`・設計 gate-cost.md §26 形 (1)）。
    ///
    /// **撃つ process を持たない段（[`unwrapped`]）は `None`**＝record は field を欠く（0 と書かない
    /// ＝「測って 0 秒」と弁別する・C10）。撃った段は起動できなかった周も秒を持つ（[`Fired::secs`]）。
    pub secs: Option<u64>,
    /// この行に渡した実効 jobs（record の `jobs=`）。
    pub jobs: u64,
    /// 受付の結果（record の `slot=`・受付を通らない行は `None`）。
    pub slot: Option<String>,
    /// 受付が測れなかった理由（record の `slot_why=`・測れた周と受付を通らない行は `None`）。
    pub slot_why: Option<admission::Unreadable>,
    /// 行の終端で scope を片付けた結果（record の `scope=`・包めなかった周と `Gone` は `None`）。
    pub scope: Option<Released>,
    /// 行が stdout に出した末尾の非空 1 行（record の `line=`・逐語・設計 gate-cost.md §5.1）。
    ///
    /// **kind と rc を問わず**運ぶ（xtask の検出線の 1 行も flip-check の判定行も、rc 0 で通った周の
    /// stdout にしか現れない）。無い周は `None`＝field を欠く（空文字を書かない・C10）。
    pub line: Option<String>,
    /// 器の健康の遮断器の印（record の `host=`・設計 gate-cost.md §32 約束 5 / 7）。
    ///
    /// [`health::Mark::Closed`] は**撃たなかった行**（待ちの上限を超えた・rc は撃てなかった -1 で赤に数えない）、
    /// [`health::Mark::Unmeasured`] は測れないまま撃った行。空いていた周と撃つ process を持たない段は `None`＝
    /// field を欠く（0 や空を書かない・C10）。
    pub host: Option<health::Mark>,
}

impl Step {
    /// 遮断器が閉じて**撃たなかった**行か（判定は gate の `decide` が赤より先に読む・設計 gate-cost.md §32 約束 6）。
    pub fn is_closed(&self) -> bool {
        self.host == Some(health::Mark::Closed)
    }
}

/// 撃つ process を持たない段（write-set 照合）の封じ込め欄。
///
/// 包めなかったのではなく**包む対象が無い**（Rust で照合するだけで子 process を起こさない）。
/// 理由を持たせないのはそのためである。**秒を持たないのも同じ理由**である（測る process が無い
/// ＝0 秒で撃ったのではない・設計 gate-cost.md §26 形 (1)）。
fn unwrapped(cmd: String, rc: i32, stderr: String) -> Step {
    Step {
        stage: Check::WriteSet,
        cmd,
        rc,
        stderr,
        failed: None,
        confined: false,
        reason: None,
        peak_mb: None,
        secs: None,
        jobs: UNADMITTED_JOBS,
        slot: None,
        slot_why: None,
        scope: None,
        line: None,
        host: None,
    }
}

/// 遮断器が閉じて**撃たなかった**行の結果（設計 gate-cost.md §32 約束 5）。
///
/// process を起こさないので秒も封じ込めの欄も持たない（[`unwrapped`] と同じ理由）。cmd は置換前の行（受付を
/// 通っていない＝実効 jobs を持たない）、rc は撃てなかった周の -1 で、赤には数えない（印が先に効く）。
fn closed(entry: &Fire<'_>) -> Step {
    Step {
        stage: entry.stage,
        cmd: entry.raw.to_owned(),
        rc: -1,
        stderr: String::new(),
        failed: None,
        confined: false,
        reason: None,
        peak_mb: None,
        secs: None,
        jobs: UNADMITTED_JOBS,
        slot: None,
        slot_why: None,
        scope: None,
        line: None,
        host: Some(health::Mark::Closed),
    }
}

/// 受付の材料（gate だけが持つ・land の main 実測は受付を通らない）。
pub struct Admit<'a> {
    /// 置き場（host の slot dir はこの親から導く）。
    pub state_dir: &'a Path,
    /// 便 id（札の名に載る）。
    pub run: &'a str,
    /// rules 行の値。
    pub rules: admission::Rules,
}

/// 検証を撃つ材料。
pub struct Checks<'a> {
    /// 撃つ場所。
    pub worktree: &'a Path,
    /// 便の base（`{base}` の実値）。
    pub base: &'a str,
    /// 読み込み済みの契約。
    pub contract: &'a Contract,
    /// **便の写しの**共通 verify（repo / worktree の宣言は読み直さない）。
    pub common: &'a [String],
    /// **便の写しの**検出線（撃つのは着地後の検出の口だけ・[`run_detection_admitted`]・段の列に ③ を持たない
    /// [`run_checks_admitted`] は読まない）。
    pub detection: &'a [String],
    /// 器の健康の遮断器（倍率 2 本と待ちの上限・gate と land の主実測が同じ欄を埋める・設計 gate-cost.md §32 約束 9）。
    pub host: health::Breaker,
    /// 便の契約 file（段 ① が key `done-teeth` の名の歯を測る・埋めるのは gate と終わりの門が通る `record_checks` だけで、
    /// 主実測・候補の木・着地後の検出は `None`＝歯を測らない・設計 contract-source.md §66 形 5）。
    pub contract_file: Option<&'a Path>,
}

/// ①②④ を**順序どおり**に撃つ（③ は撃たない・[`gate_checks`]・設計 gate-cost.md §44 形 (9)）。
///
/// **gate も land もこの 1 本を通る**——2 本になると gate が通した行と main で撃った行の
/// 意味が静かにずれる（行を撃つ実装を [`run_line_captured`] 1 本に保っているのと同じ理由）。
pub fn run_checks(checks: &Checks<'_>) -> Vec<Step> {
    run_checks_admitted(checks, &gate_checks(), None)
}

/// `stages` の段を**順序どおり**に撃つ（受付を持つ形）。[`run_checks`] はこれの受付なしの形である。
///
/// `stages` は gate の列（[`gate_checks`]・③ を除く）。`admit` が在る周だけ、共通 verify と契約の verify の行が
/// 全部 host の受付を通る（設計 gate-cost.md §3.2・§3.3・§45 形 2）。行を撃つ実装はこの 1 本のままである。
///
/// どの行も 1 回だけ撃つ——検出線の rc 2 も撃ち直さない（設計 gate-cost.md §44 形 (6)・撃ち直すのは人が
/// 着地後の検出の口を撃つ形）。
///
/// **行を撃つ前に器の健康の遮断器を通す**（[`health::pass`]・設計 gate-cost.md §32）。待ちの上限を超えた行は
/// 撃たずに閉じた印の [`Step`] を積み、**以後の行も待たずに閉じる**（上限を行の本数だけ重ねない）。
pub fn run_checks_admitted(checks: &Checks<'_>, stages: &[Check], admit: Option<&Admit<'_>>) -> Vec<Step> {
    // 封じ込めの 3 線は 1 便で 1 度だけ読む（行ごとに manifest を開き直さない）。
    let caps = confine::Caps::embedded();
    let mut steps: Vec<Step> = Vec::new();
    for check in stages {
        let (lines, holes): (&[String], bool) = match *check {
            Check::WriteSet => {
                steps.push(check_write_set(checks));
                continue;
            }
            Check::Common => (checks.common, true),
            Check::Detection => (checks.detection, true),
            Check::Contract => (&checks.contract.verify, false),
        };
        for line in lines {
            let n = steps.len().saturating_add(1);
            let entry = Fire { checks, raw: line.as_str(), holes, stage: *check, n };
            let step = fire_row(&entry, &steps, (caps, admit));
            steps.push(step);
        }
    }
    steps
}

/// **検出線の行だけ**を撃つ入口（着地後の検出の口・設計 gate-cost.md §44 形 (2)）。
///
/// 撃つのは `checks.detection` の行だけで（①②④ は撃たない）、行ごとの受付・箱・遮断器は [`run_checks_admitted`] と
/// 同じ行の手（[`fire_row`]）を通る。rc 2 の撃ち直しは持たない（撃ち直すのは人が同じ口を撃つ形）。
pub fn run_detection_admitted(checks: &Checks<'_>, admit: Option<&Admit<'_>>) -> Vec<Step> {
    let caps = confine::Caps::embedded();
    let mut steps: Vec<Step> = Vec::new();
    for line in checks.detection {
        let n = steps.len().saturating_add(1);
        let entry = Fire { checks, raw: line.as_str(), holes: true, stage: Check::Detection, n };
        let step = fire_row(&entry, &steps, (caps, admit));
        steps.push(step);
    }
    steps
}

/// 行 1 本の手: 遮断器を通し（前の行が閉じていれば待たずに閉じる）、受付と箱の中で 1 回撃つ（**gate と着地後の検出の
/// 1 実装**・C2）。
fn fire_row(
    entry: &Fire<'_>,
    steps: &[Step],
    held: (Result<confine::Caps, RuleRead>, Option<&Admit<'_>>),
) -> Step {
    let (caps, admit) = held;
    let passage = if steps.iter().any(Step::is_closed) {
        health::Passage::Closed
    } else {
        health::pass(entry.checks.host)
    };
    let health::Passage::Fire(mark) = passage else {
        return closed(entry);
    };
    let mut step = fire(entry, caps, admit);
    step.host = mark;
    step
}

/// 共通 verify の行の穴を実値へ置く（**契約の行には置換しない**）。置換する穴の列は
/// [`crate::pipe::declaration::BASE_HOLES`] と同じ 4 つである（intake の判定と同じ列・設計 §3.3 errata・§34 約束 4）。
///
/// **1 走査で埋めない**のは、穴の値が sha と数字と filter 語（`[A-Za-z0-9_]` と `,` / `-`）だけで、互いの字面を
/// 含まないためである（`{worktree}` のように外から来る path を埋める面とは条件が違う）。
pub(crate) fn fill_holes(line: &str, base: &str, jobs: u64, threads: u64, teeth: &str) -> String {
    line.replace(BASE_HOLE, base)
        .replace(JOBS_HOLE, &jobs.to_string())
        .replace(THREADS_HOLE, &threads.to_string())
        .replace(TEETH_HOLE, teeth)
}

/// `{teeth}` の実値: 契約の verify 行の filter 語を宣言順に `,` で結ぶ（filter を持たない行は飛ばし・0 本は
/// [`NO_TEETH`]・設計 gate-cost.md §34 約束 5）。語の導出は置き場の導出と同じ関数（[`closure::teeth_words`]）で、
/// 環境変数では渡さない（C2.2）。gate も land の主実測も [`Checks::contract`] から同じここを通る。
pub(crate) fn teeth_of(verify: &[String]) -> String {
    let words = closure::teeth_words(verify);
    if words.is_empty() {
        NO_TEETH.to_owned()
    } else {
        words.join(",")
    }
}

/// 契約の verify 行が filter 語を 1 つも持たない周の `{teeth}`（道具の `--teeth -` = 空）。
const NO_TEETH: &str = "-";

/// 1 行を撃つ材料。
struct Fire<'a> {
    /// 撃つ場所と材料。
    checks: &'a Checks<'a>,
    /// **置換前**の行（どの箱に入れるか・受付を通るかはここから決まる）。
    raw: &'a str,
    /// 穴を置換する段か（共通 verify だけ・契約の行は穴を持たない）。
    holes: bool,
    /// 段（scope の unit 名に載る）。
    stage: Check,
    /// `verify.jsonl` の record 番号（scope の unit 名に載る）。
    n: usize,
}

/// 1 行を scope に包んで撃ち、結果を組む。
///
/// 受付が在る周の行は、**撃つ前に受付で枠を取り、撃った後に返す**（設計 §3.2・§45 形 2）。実効 jobs = `min(gate.mutants_jobs, 受け付けた枠)`。実効 thread は受付が jobs と
/// 対で決めた値（[`Grant::threads`]・設計 §31 約束 4）で、受付を通らない行は jobs と同じく 1 を埋める。
fn fire(entry: &Fire<'_>, caps: Result<confine::Caps, RuleRead>, admit: Option<&Admit<'_>>) -> Step {
    let place = entry
        .checks
        .worktree
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unit = confine::unit_name(&place, entry.stage.as_str(), entry.n);
    let grant = admitted(entry, caps, &unit, admit);
    let (jobs, threads) = grant
        .as_ref()
        .map_or((UNADMITTED_JOBS, UNADMITTED_THREADS), |held| (held.jobs, held.threads));
    let cmd = if entry.holes {
        fill_holes(entry.raw, entry.checks.base, jobs, threads, &teeth_of(&entry.checks.contract.verify))
    } else {
        entry.raw.to_owned()
    };
    let wrap = confine::Wrap {
        unit: &unit,
        limit: confine::limit_of(entry.raw, jobs),
        caps,
        // 受付が配った幅（縮退と測れない周の Grant は 1 × 1）。受付を通らない周は `None`＝1 job の値段（設計 §45 形 4）。
        width: grant.as_ref().map(|_| jobs.saturating_mul(threads)),
    };
    let fired = run_line_captured(entry.checks.worktree, &cmd, &wrap);
    let (confined, reason, peak_mb) =
        (fired.confinement.confined(), fired.reason(), fired.usage.peak_mb);
    let (slot, slot_why) = grant
        .as_ref()
        .map_or((None, None), |held| (Some(held.detail.clone()), held.why));
    if let Some(held) = grant {
        admission::release(held);
    }
    Step {
        stage: entry.stage,
        cmd,
        rc: fired.rc,
        stderr: fired.stderr,
        failed: fired.failed,
        confined,
        reason,
        peak_mb,
        secs: Some(fired.secs),
        jobs,
        slot,
        slot_why,
        scope: fired.scope,
        line: fired.stdout_tail,
        host: None,
    }
}

/// 受付が在る周は行を全部受付に通し、枠を取る（受付の無い周は `None`・設計 §45 形 2）。
///
/// **`{jobs}` を持つ宣言の行**（共通 verify・検出線）は上限までの枠を求める。**`{jobs}` を持たない行**（共通 verify・
/// 契約の verify）は包めるかに依らず 1 枠（1 job）を求める——並列度を受け取らない行が host の core を全部取りにいく
/// 形を、同時の本数の勘定で塞ぐ。**包めない周は 1 枠だけを取りにいく**——箱の無い行に並列度を上げると、溢れたときに
/// 殺されるのが席の側になる。
fn admitted(
    entry: &Fire<'_>,
    caps: Result<confine::Caps, RuleRead>,
    unit: &str,
    admit: Option<&Admit<'_>>,
) -> Option<Grant> {
    let admit = admit?;
    let want = if entry.holes && entry.raw.contains(JOBS_HOLE) {
        // 包めるかは箱の大きさ（jobs ≥ 1）に依らない。撃つ前に同じ判定を 1 度だけ引く。
        let probe = confine::Wrap { unit, limit: confine::limit_of(entry.raw, UNADMITTED_JOBS), caps, width: None };
        let (_, confinement) = confine::wrap_line(entry.raw, &probe);
        if confinement.confined() { admit.rules.cap } else { UNADMITTED_JOBS }
    } else {
        UNADMITTED_JOBS
    };
    let mut grant = admission::admit(admit.state_dir, admit.run, want, &admit.rules);
    grant.jobs = admit.rules.cap.min(grant.jobs).max(UNADMITTED_JOBS);
    Some(grant)
}

/// diff の path が契約の write-set に収まっているか（ADR-0009 §2.4）と、契約の約束が便の HEAD の木に在るか（設計
/// pipeline.md §58）。
///
/// hook の guard とは**面が違う**: あちらは編集時に実体（symlink）まで解いて 1 件ずつ止める
/// backstop で、こちらは便が終わった後に git が出した名前を数える gate である。
///
/// 約束の外れ（[`broken_promises`]）は diff の外れと**同じ段・同じ極性**（rc 1）で名指す（段・理由の型・verdict を足さない・
/// §58 形 3）。木か約束の材料を読めない周は diff を読めない周と同じ rc -1（[`is_unreadable`]）。
fn check_write_set(checks: &Checks<'_>) -> Step {
    let cmd = WRITE_SET_CMD.to_owned();
    let range = format!("{}..HEAD", checks.base);
    // **`-z`**（NUL 区切り・quote しない）で受ける。既定の `--name-only` は非 ASCII の path を
    // `"…"` へ quote するので、字面照合が偽の RED を出す（本 repo は日本語の doc を持つ）。
    let Some(bytes) = git_bytes(checks.worktree, &["diff", "--name-only", "-z", &range]) else {
        return unwrapped(cmd, -1, "diff の path を読めない".to_owned());
    };
    let text = String::from_utf8_lossy(&bytes);
    let paths: Vec<&str> = text.split('\0').filter(|path| !path.is_empty()).collect();
    let outside: Vec<&str> = paths.iter().copied().filter(|path| !listed(path, &checks.contract.write_set)).collect();
    let placed: Vec<&str> = paths.iter().copied().filter(|path| place_only(path, &checks.contract.write_set)).collect();
    let broken = match head_tree(checks) {
        Ok(tree) => {
            let names: Vec<&str> = tree.names.iter().map(String::as_str).collect();
            broken_promises(&tree.paths, &checks.contract.write_set, &names, &tree.sources)
                .map_err(|error| error.reason())
        }
        Err(reason) => Err(reason),
    };
    let broken = match broken {
        Ok(found) => found,
        Err(reason) => return unwrapped(cmd, -1, reason),
    };
    let measured = checks.contract_file.map(|file| done_teeth_sections(file, (checks.worktree, checks.base), &checks.contract.verify));
    let teeth = match measured.unwrap_or_else(|| Ok(Vec::new())) {
        Ok(found) => found,
        Err(reason) => return unwrapped(cmd, -1, reason),
    };
    let mut lines: Vec<String> = Vec::new();
    if !outside.is_empty() {
        lines.push(format!("契約の write-set の外へ出た path:\n{}", outside.join("\n")));
    }
    if !placed.is_empty() {
        lines.push(format!("契約の write-set の = の file が便の diff に在る:\n{}", placed.join("\n")));
    }
    lines.extend(broken.sections());
    lines.extend(teeth);
    let rc = i32::from(!lines.is_empty());
    lines.extend(broken.note.map(str::to_owned));
    unwrapped(cmd, rc, lines.join("\n"))
}

/// 段 ① の約束の測りが読む便の HEAD の木の材料（設計 pipeline.md §58）。
struct HeadTree {
    /// 便の HEAD の木の path（`git ls-tree` の 1 回の読み）。
    paths: Vec<String>,
    /// 設計 pointer の行の約束の行の `symbols` の `+` の名（`+` を剥がした字面・約束の行の無い行は空）。
    names: Vec<String>,
    /// 木の `.rs` の本文（名が空の周は読まない）。
    sources: Vec<Source>,
}

/// 便の HEAD の木の path と、約束の行の `+` の名と、名を解く `.rs` の本文を読む（読めない周は理由）。
///
/// 約束の行は**便の base** の設計 doc から引く（受付が読んだ行と同じ commit の行・[`table::read_table`] と
/// [`table::promises_of`] の読み手）。本文は木の path のうち `.rs` を作業木から読む（段 ① は verify 行より先に撃つので
/// clean な HEAD の字面・受付の材料と同じ [`table::read_all`]）。
fn head_tree(checks: &Checks<'_>) -> Result<HeadTree, String> {
    let listed = git_bytes(checks.worktree, &["ls-tree", "-r", "-z", "--name-only", "HEAD"])
        .ok_or_else(|| "便の HEAD の木を読めない".to_owned())?;
    let paths: Vec<String> =
        String::from_utf8_lossy(&listed).split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect();
    let names = promised_names(checks)?;
    let sources = if names.is_empty() { Vec::new() } else { table::read_all(checks.worktree, &paths, RS) };
    Ok(HeadTree { paths, names, sources })
}

/// 設計 pointer の行の約束の行の `symbols` の `+` の名（`design` が pointer でない契約・約束の行の無い行は空）。
fn promised_names(checks: &Checks<'_>) -> Result<Vec<String>, String> {
    let Ok(pointer) = table::parse_pointer(&checks.contract.design) else {
        return Ok(Vec::new());
    };
    let shown = git_bytes(checks.worktree, &["show", &format!("{}:{}", checks.base, pointer.path)])
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or_else(|| format!("{} を便の base から読めない", pointer.path))?;
    let (_, promises) = table::read_table(&pointer.path, &shown).unwrap_or_default();
    Ok(table::promises_of(&promises, &pointer.id)
        .iter()
        .flat_map(|promise| promise.symbols.iter())
        .filter_map(|name| name.strip_prefix(NEW_FILE))
        .map(str::to_owned)
        .collect())
}

/// 約束の名を解く本文の読み手の母集団の拡張子（[`closure::symbols_in_base`] が読む `.rs`）。
const RS: &str = ".rs";

/// 名を測らなかった周の注記（木が読み手の母集団の file を 1 本も持たない・rc は変えない・§58 形 2）。
const NO_SOURCES: &str = "約束の行の + の名を測らない: 便の木に .rs が 1 本も無い（名の読み手の母集団の外）";

/// 約束の外れ（設計 pipeline.md §58 形 1 / 2・[`broken_promises`] の値）。
#[derive(Debug, Default, PartialEq, Eq)]
struct Broken {
    /// write-set の `+` の項目で便の木に無い path（write-set の順・接頭辞を剥がした字面）。
    absent: Vec<String>,
    /// write-set の `~` の項目で便の木に在る path。
    present: Vec<String>,
    /// 約束の行の `+` の名で便の木の `.rs` に解けない名（書かれた順）。
    unresolved: Vec<String>,
    /// 名を測らなかった周の注記（[`NO_SOURCES`]・測った周は `None`）。
    note: Option<&'static str>,
}

impl Broken {
    /// 外れの見出しつきの列（外れの無い種類は出さない・注記は含まない）。
    fn sections(&self) -> Vec<String> {
        [
            ("契約の write-set の + の file が便の木に無い", &self.absent),
            ("契約の write-set の ~ の file が便の木に在る", &self.present),
            ("約束の行の + の名が便の木で解けない", &self.unresolved),
        ]
        .into_iter()
        .filter(|(_, items)| !items.is_empty())
        .map(|(head, items)| format!("{head}:\n{}", items.join("\n")))
        .collect()
    }
}

/// 約束の外れを返す pure な 1 本（入力は木の path・write-set・約束の `+` の名・木の `.rs` の本文・§58 形 1 / 2）。
///
/// `+` の項目は木に在り、`~` の項目は木に無いこと（接頭辞を剥がした path・dir 項目は配下の path の有無）。接頭辞の無い
/// 項目と `-` / `=` の項目は測らない。名は [`closure::symbols_in_base`] と同じ読み手で解き、`Some(false)` だけを外れに
/// 数える（名指しの形でない字面は測れない＝下界）。`sources` が空の周（`.rs` を持たない木）は名を測らず注記を返す。
fn broken_promises(tree: &[String], write_set: &[String], names: &[&str], sources: &[Source]) -> Result<Broken, ClosureError> {
    let plain = |prefix: char| {
        write_set.iter().filter(move |item| item.starts_with(prefix)).map(|item| refuse::normalize(item))
    };
    let absent = plain(NEW_FILE).filter(|path| !in_tree(tree, path)).collect();
    let present = plain(DELETE_FILE).filter(|path| in_tree(tree, path)).collect();
    let mut broken = Broken { absent, present, ..Broken::default() };
    if names.is_empty() {
        return Ok(broken);
    }
    if sources.is_empty() {
        broken.note = Some(NO_SOURCES);
        return Ok(broken);
    }
    let found = closure::symbols_in_base(names, tree, sources)?;
    broken.unresolved =
        names.iter().zip(found).filter(|(_, solved)| *solved == Some(false)).map(|(name, _)| (*name).to_owned()).collect();
    Ok(broken)
}

/// 名の歯の外れの見出し（段 ① の stderr・設計 contract-source.md §66 形 5・外れの種類ごとに 1 つ）。
const UNWRITTEN: &str = "契約の done-teeth の書かれていない歯（便の HEAD の歯の区間に無い）";
const UNMOVED: &str = "契約の done-teeth の動いていない歯（本文が base と同じ・動かさない既存の歯は = で書く）";
const TWO_SITES: &str = "契約の done-teeth の 2 か所の名（便の HEAD の歯の区間に 2 か所以上）";
const VANISHED: &str = "契約の done-teeth の消えた既存の歯（便の HEAD の歯の区間に無い）";

/// key `done-teeth` の名の歯（名と `=` の名）の照らし（設計 contract-source.md §66 形 5・段 ① の一部）: 契約 file `file` の key を
/// [`done_teeth_of`] で読み、便の HEAD の木（`at` は作業木と便の base）の歯の区間で、選ぶ検証行 `verify` の crate と scope の歯を
/// [`tooth_sites`] で測る。key の無い契約・名の歯の無い key・`.rs` を 1 本も持たない木は何も測らず空（rc を変えない）。外れは見出しつきの
/// 列、key か木か宣言の根を読めない周は理由（段 ① の rc -1）。`@` と `!` の歯は測らない。
fn done_teeth_sections(file: &Path, at: (&Path, &str), verify: &[String]) -> Result<Vec<String>, String> {
    let named = named_teeth(done_teeth_of(file)?);
    if named.is_empty() {
        return Ok(Vec::new());
    }
    let (worktree, base) = at;
    let git = |args: &[&str], what: &str| git_bytes(worktree, args).ok_or_else(|| format!("{what}を読めない"));
    let listed = git(&["ls-tree", "-r", "-z", "--name-only", "HEAD"], "便の HEAD の木")?;
    let paths: Vec<String> = nul_split(&listed);
    let sources = table::read_all(worktree, &paths, RS);
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(reason) = sources.iter().find_map(|source| source.body.as_ref().err()) {
        return Err(reason.clone());
    }
    let roots = match RootsAtHead::read(worktree) {
        RootsAtHead::Unreadable => return Err("便の HEAD の宣言の crate-roots を読めない".to_owned()),
        RootsAtHead::Fixed => fixed_roots(),
        RootsAtHead::Declared(added) => with_fixed(&added),
    };
    let head = Base { sources: &sources, snapshots: &[], tracked: &[], core_crate: NAME, roots: &roots };
    let changed = nul_split(&git(&["diff", "--name-only", "-z", &format!("{base}..HEAD")], "diff の path")?);
    let mut misses: Vec<(&str, String)> = Vec::new();
    for (element, kept, name) in &named {
        let lines: Vec<&String> = verify.iter().filter(|line| selects(line, name, NAME)).collect();
        let sites = tooth_places(&lines, name, &head);
        let miss = match sites.as_slice() {
            [] if *kept => (VANISHED, element.clone()),
            [] => (UNWRITTEN, element.clone()),
            [one] if !*kept && (!changed.contains(&one.0) || unmoved(worktree, (base, &head), (&lines, name), one)) => {
                (UNMOVED, element.clone())
            }
            [_] => continue,
            many => (TWO_SITES, format!("{element}（{}）", many.iter().map(|(path, _)| path.as_str()).collect::<Vec<&str>>().join(", "))),
        };
        misses.push(miss);
    }
    Ok([UNWRITTEN, UNMOVED, TWO_SITES, VANISHED]
        .iter()
        .filter_map(|head| {
            let items: Vec<&str> = misses.iter().filter(|(found, _)| found == head).map(|(_, item)| item.as_str()).collect();
            (!items.is_empty()).then(|| format!("{head}:\n{}", items.join("\n")))
        })
        .collect())
}

/// NUL 区切りの path の列（空の区切りは落とす）。
fn nul_split(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes).split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect()
}

/// 欄の要素のうち名の歯（`(要素の字・既存の歯か・名)`）。形が読めない要素と `@` と `!` の歯は測らない。
fn named_teeth(elements: Vec<String>) -> Vec<(String, bool, String)> {
    elements
        .into_iter()
        .filter_map(|element| match parse_element(&element) {
            Ok((_, Tooth::Named(name))) => Some((element, false, name)),
            Ok((_, Tooth::Kept(name))) => Some((element, true, name)),
            _ => None,
        })
        .collect()
}

/// 歯 `name` が `base` の木に在る所（path と本文・同じ file に 2 つ在れば 2 件）を、それを選ぶ検証行 `lines` の全部で集める
/// （2 本の行が同じ file を指す周は 1 度に畳む）。
fn tooth_places(lines: &[&String], name: &str, base: &Base<'_>) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for line in lines {
        let here = tooth_sites(line, name, base);
        for (path, body) in &here {
            let count = |list: &[(String, String)]| list.iter().filter(|(other, _)| other == path).count();
            if count(&found) < count(&here) {
                found.push((path.clone(), body.clone()));
            }
        }
    }
    found
}

/// 便の HEAD の歯 `site`（path と本文）が base と同じ本文か（base の同じ file の版を [`tooth_sites`] で読む・base に無い file と歯は
/// 新しい歯で `false`）。
fn unmoved(worktree: &Path, at: (&str, &Base<'_>), tooth: (&[&String], &str), site: &(String, String)) -> bool {
    let (base, head) = at;
    let (lines, name) = tooth;
    let Some(old) = git_bytes(worktree, &["show", &format!("{base}:{}", site.0)]) else {
        return false;
    };
    let before = [Source { path: site.0.clone(), body: Ok(String::from_utf8_lossy(&old).into_owned()) }];
    let was = Base { sources: &before, ..*head };
    lines.iter().flat_map(|line| tooth_sites(line, name, &was)).next().is_some_and(|(_, text)| text == site.1)
}

/// 項目の path が木に在るか（file の一致 か dir 項目〔末尾 `/`〕の配下の path・[`listed`] と同じ segment 境界）。
fn in_tree(tree: &[String], path: &str) -> bool {
    let trimmed = path.trim_end_matches('/');
    tree.iter().any(|found| found == trimmed || found.starts_with(&format!("{trimmed}/")))
}

/// path が write-set のいずれか（file の一致 か dir の prefix）に含まれるか。
///
/// 項目は [`refuse::normalize`] に通してから比べる（接頭辞 `+` / `-` は受付の宣言であって path の一部ではない
/// ＝diff の素の path と照合する・設計 contract-source.md §3・剥がす規則を 2 か所に持たない・`s2-07l.291`）。
fn listed(path: &str, write_set: &[String]) -> bool {
    write_set.iter().any(|entry| covers(path, entry))
}

/// path が write-set の置き場だけの項目（`=`・中身を変えない）のいずれかに含まれるか（照合は [`listed`] と同じ [`covers`]）。
/// diff に在れば便が中身を変えた＝段 ① が名指して落とす。
fn place_only(path: &str, write_set: &[String]) -> bool {
    write_set.iter().filter(|entry| entry.starts_with(refuse::PLACE_ONLY_FILE)).any(|entry| covers(path, entry))
}

/// path が項目 1 つ（接頭辞を剥がした file の一致 か dir の prefix）に含まれるか。
fn covers(path: &str, entry: &str) -> bool {
    let plain = refuse::normalize(entry);
    let trimmed = plain.trim_end_matches('/');
    path == trimmed || path.starts_with(&format!("{trimmed}/"))
}

/// 段①（write-set 照合）を**読めなかった**段か（`s2-07l.65`）。
///
/// 位置（`CHECKS` の宣言順）ではなく **段の名と rc** で見る（順序が変わっても診断が黙って消えない）。
/// rc だけで見ない: 撃った sh が signal で死んだ周も `code()` が無く -1 になる（[`recorded_rc`]）ので、
/// rc -1 の全数を「読めなかった」に倒すと**走って死んだ赤**が「測れなかった」に化ける。
/// gate と land が**同じ 1 本**で判定する（極性を 2 面に持たない・`s2-07l.103`）。
pub fn is_unreadable(step: &Step) -> bool {
    step.cmd == WRITE_SET_CMD && step.rc == -1
}

/// verify 1 行を撃った結果。
pub struct Fired {
    /// process の rc（起動できない周は -1）。
    pub rc: i32,
    /// stderr の写し（落ちた歯の区間 + 末尾 [`super::record::STDERR_TAIL_LINES`] 行・[`excerpt_of`]）。
    pub stderr: String,
    /// nextest 形の stderr で落ちた歯（[`Step::failed`] へ運ぶ・起動できなかった周は `None`）。
    pub failed: Option<Failed>,
    /// 包みが stdout の終端に出した数（包めなかった周は既定）。
    pub usage: Usage,
    /// 行の壁時計（秒・[`Step::secs`] 経由で record の `secs=`・設計 gate-cost.md §26 形 (1)）。
    ///
    /// 測るのは **process の起動から終了まで**で、起動できなかった周も（起動に失敗するまでの）
    /// 秒を持つ——判定は rc のままで、秒は費用の値である（1 便の時間を器が測る・C10）。
    /// 行の終端で scope を片付ける時間（[`confine::release_scope`]）は行の費用ではないので入れない。
    pub secs: u64,
    /// 包めたか。
    pub confinement: Confinement,
    /// 行の終端で scope を片付けた結果（record に書く周だけ `Some`・[`confine::release_scope`]）。
    pub scope: Option<Released>,
    /// stdout の末尾の非空 1 行（包みの終端行を剥がした残り・逐語・無ければ `None`・[`last_line`]）。
    ///
    /// **判定には使わない**（判定は rc である）。record の `line=` へ運ぶだけの値で、起動できなかった
    /// 周は stdout が器の外に無いので `None`。
    pub stdout_tail: Option<String>,
}

impl Fired {
    /// record の `reason=`。
    ///
    /// 包めなかった周はその理由、包めた周は**箱の中で起きたこと**を載せる——`oom_kill` が
    /// 立った周と、包みごと signal で死んだ周（`memory.events` を読む前に死ぬので oom の
    /// 代理・設計 §4.3）を、外からの kill と弁別するためである（lens-132d L1）。
    fn reason(&self) -> Option<Reason> {
        if let Some(found) = self.confinement.reason() {
            return Some(found);
        }
        if self.usage.oom_kill.is_some_and(|count| count >= 1) {
            return Some(Reason::OomKill);
        }
        (self.rc < 0).then_some(Reason::Signal)
    }
}

/// verify 1 行を **cgroup の scope に包んで**撃ち、rc・stderr の末尾・包みの数を得る。
///
/// **撃つ実装はここ 1 本だけ**である（gate も land も [`run_checks`] 経由でここへ来る）。
/// 出力の要る側と要らない側で `Command` を 2 本に割ると、gate が通した行と land が
/// main で撃った行が別の実装になり、意味が静かにずれる。包む口も同じ理由で 1 本である。
///
/// stdout を読むのは**包みの終端行と record の `line=` のため**だけで、判定には使わない（判定は rc である）。
pub fn run_line_captured(worktree: &Path, line: &str, wrap: &confine::Wrap<'_>) -> Fired {
    let (mut command, confinement) = confine::wrap_line(line, wrap);
    // 壁時計は**起動の直前から終了の直後まで**の 1 対で取る（設計 gate-cost.md §26 形 (1)）。
    // 秒は起動できた周も起動できなかった周も同じこの 1 つを運ぶ（下の 2 つの返り口）。
    let started = std::time::Instant::now();
    let spawned = command.current_dir(worktree).output();
    let secs = started.elapsed().as_secs();
    // **行の終端で scope を片付ける**（設計 gate-cost.md §4.4 errata・`s2-07l.234`）。行が孤児の
    // process を残すと scope は active のまま残り、同じ名の次の周の相手になる。判定は変えない。
    let scope = confine::release_scope(&confinement);
    let Ok(out) = spawned else {
        // 起動できなかった周は rc も stderr も**器の外に無い**。空を「何も言わなかった」
        // として返し、極性は従来どおり RED 側（-1）へ倒す。
        return Fired {
            rc: -1,
            stderr: String::new(),
            failed: None,
            usage: Usage::default(),
            secs,
            confinement,
            scope,
            stdout_tail: None,
        };
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    // **包めなかった周の stdout は測定として読まない**。素の行が出した `confine-usage` の字面を
    // 包みの測定として読むと、撃たれた行が自分の peak を名乗れてしまう。
    let usage = if confinement.confined() {
        confine::read_usage(&stdout)
    } else {
        Usage::default()
    };
    let excerpt = excerpt_of(&String::from_utf8_lossy(&out.stderr));
    Fired {
        rc: out.status.code().unwrap_or(-1),
        stderr: excerpt.text,
        failed: excerpt.failed,
        usage,
        secs,
        confinement,
        scope,
        stdout_tail: last_line(&stdout),
    }
}

/// stdout の**末尾の非空 1 行**（record の `line=`・設計 gate-cost.md §5.1・pure）。
///
/// 包みの終端行（[`USAGE_HEAD`] で始まる行）は**剥がしてから**取る＝道具の判定行が終端行の
/// 直前に在る周（包めた周の常）にその行を返す。行の中身は逐語（CRLF の `\r` だけ `lines` が
/// 区切りとして落とす）。空白だけの行は非空に数えない。非空の行が 1 つも無い周は `None`
/// （空文字を書かない）。
fn last_line(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .rev()
        .find(|line| {
            let head = line.trim_start();
            !head.is_empty() && !head.starts_with(USAGE_HEAD)
        })
        .map(str::to_owned)
}

/// rc を JSON の非負整数へ写す。`sh` が signal で落ちた周（負）は 255 に畳む（着地後の検出の理由の語 `rc-<rc>` も同じ値）。
pub(crate) fn recorded_rc(rc: i32) -> u64 {
    u64::try_from(rc).unwrap_or(u64::from(u8::MAX))
}

/// byte 数を数える。
pub(super) fn byte_count(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

#[cfg(test)]
pub(crate) mod tests {
    // flip-check: moved s2-07l.286
    use super::super::record::USAGE_HEAD;
    use super::{
        broken_promises, done_teeth_sections, fill_holes, gate_checks, last_line, listed, run_line_captured, teeth_of, unwrapped,
        Broken, Check, Source, NO_SOURCES, NO_TEETH, TEETH_HOLE, WRITE_SET_CMD,
    };
    use crate::pipe::confine::{read_usage, Limit, Reason, Wrap};
    use crate::seat::RuleRead;
    use std::path::{Path, PathBuf};

    /// write-set の項目は接頭辞（`+` 新規 / `-` 縮む面）を剥がした素の path で照合し、dir 項目（末尾 `/`）は配下を
    /// segment 境界で含む（`s2-07l.291`）。diff の path に接頭辞の字面は来ない＝`+x` の path は `+x` の項目に当たらない。
    #[test]
    fn pipe_gate_write_set_prefixed_items_are_listed_as_plain_paths() {
        let set = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect::<Vec<String>>();
        assert!(listed("x", &set(&["+x"])), "`+x` は新規 file `x` の宣言");
        assert!(listed("x", &set(&["-x"])), "`-x` は縮む面 `x` の宣言");
        assert!(listed("dir/a.rs", &set(&["dir/"])), "dir 項目は配下を含む");
        assert!(!listed("dir.rs", &set(&["dir/"])), "dir 項目は segment 境界で外れる");
        assert!(!listed("+x", &set(&["+x"])), "接頭辞は path の一部ではない");
        assert!(!listed("y", &set(&["+x", "-x", "dir/"])), "剥がしても外の path は外");
    }

    /// 文字列の列（歯の fixture）。
    fn owned(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    /// 約束の測りの fixture の木（`.rs` 2 本と非 `.rs` 1 本・dir `src/pipe/` の配下を持つ）。
    fn promised_tree() -> Vec<String> {
        owned(&["src/lib.rs", "src/pipe/fresh.rs", "docs/a.md"])
    }

    /// 約束の測りの fixture の `.rs` の本文（`fn fresh_helper` を宣言する 1 本）。
    fn promised_sources() -> Vec<Source> {
        vec![
            Source { path: "src/lib.rs".to_owned(), body: Ok("// seed\n".to_owned()) },
            Source { path: "src/pipe/fresh.rs".to_owned(), body: Ok("pub fn fresh_helper() -> u8 {\n    1\n}\n".to_owned()) },
        ]
    }

    /// (a) `+` の file（と dir 項目）が木に在れば外れ無し、無ければ接頭辞を剥がした path を write-set の順に名指す。
    #[test]
    fn pipe_gate_promised_plus_items_must_be_in_the_tree() {
        let tree = promised_tree();
        let held = broken_promises(&tree, &owned(&["+src/pipe/fresh.rs", "+src/pipe/"]), &[], &[]);
        assert_eq!(held, Ok(Broken::default()), "在る `+` は外れ無し");
        let found = broken_promises(&tree, &owned(&["+src/none.rs", "+src/pipe/fresh.rs", "+gone/", "+src/lib"]), &[], &[]);
        let want = Broken { absent: owned(&["src/none.rs", "gone/", "src/lib"]), ..Broken::default() };
        assert_eq!(found, Ok(want), "無い `+` を全部名指す（`src/lib` は segment 境界で `src/lib.rs` に当たらない）");
        let sections = broken_promises(&tree, &owned(&["+src/none.rs"]), &[], &[]).map(|broken| broken.sections());
        assert_eq!(sections, Ok(owned(&["契約の write-set の + の file が便の木に無い:\nsrc/none.rs"])), "見出しつきの列");
    }

    /// (b) `~` の file が木に在れば名指し、無ければ外れ無し。
    #[test]
    fn pipe_gate_promised_tilde_items_must_be_gone_from_the_tree() {
        let tree = promised_tree();
        assert_eq!(broken_promises(&tree, &owned(&["~src/old.rs"]), &[], &[]), Ok(Broken::default()), "無い `~` は外れ無し");
        let found = broken_promises(&tree, &owned(&["~src/old.rs", "~docs/a.md", "~src/pipe/"]), &[], &[]);
        let want = Broken { present: owned(&["docs/a.md", "src/pipe/"]), ..Broken::default() };
        assert_eq!(found, Ok(want), "在る `~` を名指す");
        let sections = broken_promises(&tree, &owned(&["~docs/a.md"]), &[], &[]).map(|broken| broken.sections());
        assert_eq!(sections, Ok(owned(&["契約の write-set の ~ の file が便の木に在る:\ndocs/a.md"])), "見出しつきの列");
    }

    /// (c) 接頭辞の無い項目と `-` / `=` の項目は測らない（木に無くても在っても外れ無し）。
    #[test]
    fn pipe_gate_promised_unprefixed_items_are_not_measured() {
        let tree = promised_tree();
        let items = owned(&["src/none.rs", "src/lib.rs", "-src/none.rs", "=src/none.rs", "-docs/a.md"]);
        assert_eq!(broken_promises(&tree, &items, &[], &[]), Ok(Broken::default()));
    }

    /// (d) 約束の `+` の名は `.rs` の本文に宣言が在れば解け、無ければ書かれた順に名指す（名指しの形でない字面は測らない）。
    #[test]
    fn pipe_gate_promised_new_names_resolve_in_the_tree_sources() {
        let (tree, sources) = (promised_tree(), promised_sources());
        assert_eq!(broken_promises(&tree, &[], &["fresh_helper("], &sources), Ok(Broken::default()), "宣言の在る名は解ける");
        let found = broken_promises(&tree, &[], &["stale_helper(", "fresh_helper(", "Tide::Ebb", "src/pipe/none.rs", "散文"], &sources);
        let want = Broken { unresolved: owned(&["stale_helper(", "Tide::Ebb", "src/pipe/none.rs"]), ..Broken::default() };
        assert_eq!(found, Ok(want), "解けない名を全部名指す");
        let sections = broken_promises(&tree, &[], &["stale_helper("], &sources).map(|broken| broken.sections());
        assert_eq!(sections, Ok(owned(&["約束の行の + の名が便の木で解けない:\nstale_helper("])), "見出しつきの列");
        let mut unreadable = sources.clone();
        unreadable.push(Source { path: "src/bad.rs".to_owned(), body: Err("bad".to_owned()) });
        assert!(broken_promises(&tree, &[], &["fresh_helper("], &unreadable).is_err(), "本文を読めない周は外れでなく Err");
    }

    /// (e) `.rs` を 1 本も持たない木では名を測らず外れ 0 と注記の 1 行を返し、同じ木の `+` / `~` の照合は続ける。名の無い周は
    /// 注記を出さない。
    #[test]
    fn pipe_gate_promised_tree_without_sources_notes_and_keeps_path_checks() {
        let tree = owned(&["docs/a.md"]);
        let found = broken_promises(&tree, &owned(&["+docs/none.md", "~docs/a.md"]), &["stale_helper("], &[]);
        let want = Broken {
            absent: owned(&["docs/none.md"]),
            present: owned(&["docs/a.md"]),
            unresolved: Vec::new(),
            note: Some(NO_SOURCES),
        };
        assert_eq!(found, Ok(want), "名は測らず注記・path の照合は続く");
        assert_eq!(NO_SOURCES.lines().count(), 1, "注記は 1 行");
        assert_eq!(broken_promises(&tree, &[], &[], &[]), Ok(Broken::default()), "名の無い周は注記しない");
    }

    /// record の `line=` は stdout の**末尾の非空 1 行**（設計 gate-cost.md §5.1）: 空 / 空白だけ → `None`・
    /// 末尾改行は区切り・包みの終端行は剥がしてその直前の行・CRLF の `\r` は落ちる・空行を跨いで遡る。
    #[test]
    fn pipe_record_last_line_is_the_trailing_nonblank_line_before_the_usage_line() {
        assert_eq!(last_line(""), None, "空は None（空文字を書かない）");
        assert_eq!(last_line("\n  \n"), None, "空白だけの行は非空に数えない");
        assert_eq!(last_line("one\ntwo\n"), Some("two".to_owned()), "末尾改行は区切り");
        assert_eq!(last_line("one\ntwo"), Some("two".to_owned()), "末尾改行の無い周も同じ");
        assert_eq!(
            last_line("noise\nmutants-diff: total=3 caught=2\nconfine-usage peak_bytes=1048576 oom_kill=0\n"),
            Some("mutants-diff: total=3 caught=2".to_owned()),
            "包みの終端行を剥がした直前の行"
        );
        assert_eq!(
            last_line("confine-usage peak_bytes=- oom_kill=0\n"),
            None,
            "終端行しか無い周は None（終端行を判定行に化けさせない）"
        );
        assert_eq!(last_line("one\r\ntwo\r\n"), Some("two".to_owned()), "CRLF の \\r は落ちる");
        assert_eq!(last_line("last\n\n\n"), Some("last".to_owned()), "空行を跨いで遡る");
        assert_eq!(last_line("  padded  \n"), Some("  padded  ".to_owned()), "行の中身は逐語（trim しない）");
    }

    /// ここで剥がす見出しは [`read_usage`] が読む見出しと**同じ字面**である（ずれると終端行が `line` に化ける）。
    #[test]
    fn pipe_record_usage_head_matches_the_confine_reader() {
        let usage = read_usage(&format!("{USAGE_HEAD} peak_bytes=2097152 oom_kill=1\n"));
        assert_eq!(usage.peak_mb, Some(2), "同じ見出しを包みの読み手が測定として読む");
        assert_eq!(usage.oom_kill, Some(1));
    }

    // flip-check: retroactive s2-07l.222
    /// 起動できなかった行は **rc -1**（RED 側の極性・`-` を消すと rc 1 に化ける）で、stderr は空・scope は撃たない。
    /// 起動を Err にする fixture は**存在しない cwd**である——PATH に無い command 名の行は `sh -c` が起動して
    /// rc 127 を返す（spawn は Err にならない・下の対で pin する）。包めない `Wrap`（rules の読めない周）で撃つ
    /// ので `systemd-run` も `systemctl` も起こさない。
    #[test]
    fn mutant_in_pipe_run_line_captured_unspawnable_line_is_minus_one() {
        let root = scratch("unspawnable");
        let wrap = Wrap { unit: "scribe2-mutant-unit", limit: Limit::HostReserve, caps: Err(RuleRead::Missing), width: None };
        let fired = run_line_captured(&root.join("absent-worktree"), "true", &wrap);
        assert_eq!(fired.rc, -1, "起動できない周は -1");
        assert_eq!(fired.stderr, "", "器の外に stderr は無い");
        assert_eq!(fired.confinement.reason(), Some(Reason::NoRules), "包まずに撃った");
        assert_eq!(fired.scope, None, "scope を片付けない");
        assert_eq!(fired.stdout_tail, None, "stdout も器の外に無い");
        let missing = run_line_captured(&root, "scribe2-mutant-no-such-command", &wrap);
        assert_eq!(missing.rc, 127, "PATH に無い command は sh が起動して 127（-1 ではない）");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 段の秒は **process の起動から終了まで**の壁時計である（設計 gate-cost.md §26 形 (1)）: 1 秒眠る行の
    /// [`super::Fired`] は 1 以上を運び、起動できなかった周（存在しない cwd・rc -1 は不変）も秒を持ち、
    /// 撃つ process を持たない段（[`unwrapped`]・write-set 照合）は秒を持たない（`None`＝record は field を欠く）。
    ///
    /// 上限に当てるのは**外から測った秒**だけである——固定の壁時計 bound は負荷の高い host で偽に落ちる
    /// （設計 gate-cost.md §25 の実測）。包めない `Wrap` で撃つので `systemd-run` も `systemctl` も起こさない。
    #[test]
    fn gate_secs_fired_measures_the_wall_clock_of_the_process() {
        let root = scratch("secs");
        let wrap = Wrap { unit: "scribe2-secs-unit", limit: Limit::HostReserve, caps: Err(RuleRead::Missing), width: None };
        let slept = run_line_captured(&root, "sleep 1", &wrap);
        assert_eq!(slept.rc, 0, "行は完走した");
        assert!(slept.secs >= 1, "1 秒眠った行の壁時計は 1 以上: {}", slept.secs);
        let started = std::time::Instant::now();
        let unspawnable = run_line_captured(&root.join("absent-worktree"), "true", &wrap);
        let outer = started.elapsed().as_secs();
        assert_eq!(unspawnable.rc, -1, "起動できない周の極性は -1 のまま");
        assert!(unspawnable.secs <= outer, "起動失敗までの壁時計（外から測った秒を超えない）: {}", unspawnable.secs);
        let matched = unwrapped(WRITE_SET_CMD.to_owned(), 0, String::new());
        assert_eq!(matched.secs, None, "撃つ process を持たない段は秒を持たない（0 と書かない）");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// gate の段の列は [`super::CHECKS`] から ③ だけを除いた ①②④ で、順は宣言順のまま（設計 gate-cost.md §44 形 (9)）。
    #[test]
    fn gate_checks_drop_only_the_detection_stage() {
        assert_eq!(gate_checks(), [Check::WriteSet, Check::Common, Check::Contract]);
        assert_eq!(super::CHECKS.len(), gate_checks().len() + 1, "除くのは ③ の 1 段だけ");
    }

    /// 検出線の形（4 つの穴・`.vessel.toml` と同じ字面）。
    const DETECTION: &str = "cargo xtask mutants-diff --base {base} --jobs {jobs} --threads {threads} --teeth {teeth}";

    /// 契約の verify 行の列から `{teeth}` を埋めた検出線（base `abc`・jobs 2・threads 3）。
    fn filled(verify: &[&str]) -> String {
        let owned: Vec<String> = verify.iter().map(|line| (*line).to_owned()).collect();
        fill_holes(DETECTION, "abc", 2, 3, &teeth_of(&owned))
    }

    /// (a) verify 行 2 本（`--lib … foo_` / `--test e2e … bar_`）から `foo_,bar_` が置かれ、`{teeth}` の字面は残らない
    /// （設計 gate-cost.md §34 約束 5）。他の 3 つの穴も同じ 1 本で埋まる。
    #[test]
    fn gate_fill_teeth_two_lines_join_their_words_with_a_comma() {
        let line = filled(&[
            "cargo nextest run -p scribe2 --lib --no-tests=fail foo_",
            "cargo nextest run -p scribe2 --test e2e --no-tests=fail bar_",
        ]);
        assert_eq!(line, "cargo xtask mutants-diff --base abc --jobs 2 --threads 3 --teeth foo_,bar_");
        assert!(!line.contains(TEETH_HOLE), "穴の字面が残らない: {line}");
    }

    /// (b) filter を持たない行（nextest でない行・filter 語の無い nextest 行）は飛ばされる（`-` や空の語を置かない）。
    #[test]
    fn gate_fill_teeth_lines_without_a_filter_are_skipped() {
        let line = filled(&[
            "cargo xtask check-facts",
            "cargo nextest run -p scribe2 --lib --no-tests=fail foo_",
            "cargo nextest run -p scribe2 --lib",
            "git diff --quiet",
        ]);
        assert_eq!(line, "cargo xtask mutants-diff --base abc --jobs 2 --threads 3 --teeth foo_");
    }

    /// (c) 語が 0 本の周は `-`（道具の `--teeth -` = 空・空文字を置いて引数を欠かせない）。verify の列が空の周も同じ。
    #[test]
    fn gate_fill_teeth_zero_words_is_a_dash() {
        assert_eq!(teeth_of(&[]), NO_TEETH);
        assert_eq!(NO_TEETH, "-");
        let line = filled(&["cargo xtask check-facts", "cargo build"]);
        assert_eq!(line, "cargo xtask mutants-diff --base abc --jobs 2 --threads 3 --teeth -");
    }

    /// (d) 語の順は verify 行の宣言順（辞書順に並べ替えない・重ねて入れ替えると順も入れ替わる）。
    #[test]
    fn gate_fill_teeth_words_keep_the_declaration_order() {
        let (zeta, alpha, mid) =
            ("cargo nextest run -p scribe2 --lib zeta_", "cargo nextest run -p scribe2 --lib alpha_", "cargo nextest run -p scribe2 --lib mid_");
        let (forward, backward) = ([zeta, alpha, mid], [mid, alpha, zeta]);
        assert!(filled(&forward).ends_with("--teeth zeta_,alpha_,mid_"), "宣言順: {}", filled(&forward));
        assert!(filled(&backward).ends_with("--teeth mid_,alpha_,zeta_"), "入れ替えた順: {}", filled(&backward));
    }

    /// (1) key を読めない周: 在らない契約 file の path を名の歯の照らしに渡すと、外れの空ではなく読めない（`Err`・段 ① が rc -1 に倒す値）を返し、
    /// 理由はその path を名乗る。
    #[test]
    fn done_teeth_gate_unreadable_key_file_is_minus_one() {
        let root = scratch("teeth-key");
        let missing = root.join("absent-contract.toml");
        let found = done_teeth_sections(&missing, (&root, "abc"), &[]);
        assert!(found.as_ref().is_err_and(|reason| reason.contains("absent-contract.toml")), "読めない理由は path を名乗る: {found:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 歯ごとの空の tmp dir（in-file の歯の置き場・env を読まないのは器の本体の規律〔C2.2〕）。
    pub(in crate::pipe::gate) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gate-verdict-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// dir の直下の名前（名前順）。
    pub(in crate::pipe::gate) fn names(dir: &Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    }
}
