//! gate の verify 行の実行（段の列 [`Check`] と、行を撃つ 1 本 [`run_line_captured`]・
//! [`super`] から純移動・`s2-07l.286`）。判定の順と終端は親（[`super::gate`]）が持つ。

use super::record::{excerpt_of, Failed, USAGE_HEAD};
use super::{UNADMITTED_JOBS, WRITE_SET_CMD};
use crate::pipe::admission::{self, Grant};
use crate::pipe::closure;
use crate::pipe::confine::{self, Confinement, Reason, Released, Usage};
use crate::pipe::contract::Contract;
use crate::pipe::declaration::{BASE_HOLE, JOBS_HOLE, TEETH_HOLE, THREADS_HOLE};
use crate::pipe::git_bytes;
use crate::pipe::health;
use crate::pipe::refuse;
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

/// gate が撃つ段の列（[`CHECKS`] から ③ を除いた ①②④・宣言順のまま・設計 gate-cost.md §44 形 (9)）。
///
/// ③ は着地後の検出の口だけが撃つ（[`run_detection_admitted`]）。部分集合を `&[Check]` の const で書かず関数で組むのは、
/// 閉じた enum の const slice は全 variant を持つ形だけにするためである（xtask の enum-slices の門）。land の主実測と
/// 候補の木は [`CHECKS`] のまま撃つ。
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
    /// **便の写しの**検出線。land の main 実測は木が gate と同じ周に空を渡す（ADR-0021 §2.4）。
    pub detection: &'a [String],
    /// 器の健康の遮断器（倍率 2 本と待ちの上限・gate と land の主実測が同じ欄を埋める・設計 gate-cost.md §32 約束 9）。
    pub host: health::Breaker,
}

/// 全段を**順序どおり**に撃つ。
///
/// **gate も land もこの 1 本を通る**——2 本になると gate が通した行と main で撃った行の
/// 意味が静かにずれる（行を撃つ実装を [`run_line_captured`] 1 本に保っているのと同じ理由）。
pub fn run_checks(checks: &Checks<'_>) -> Vec<Step> {
    run_checks_admitted(checks, CHECKS, None)
}

/// `stages` の段を**順序どおり**に撃つ（受付を持つ形）。[`run_checks`] はこれの全段・受付なしの形である。
///
/// `stages` は [`CHECKS`] か gate の列（[`gate_checks`]・③ を除く）。`admit` が在る周だけ、`{jobs}` を持つ共通 verify の
/// 行が host の受付を通る（設計 gate-cost.md §3.2・§3.3）。行を撃つ実装はこの 1 本のままである。
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
fn fill_holes(line: &str, base: &str, jobs: u64, threads: u64, teeth: &str) -> String {
    line.replace(BASE_HOLE, base)
        .replace(JOBS_HOLE, &jobs.to_string())
        .replace(THREADS_HOLE, &threads.to_string())
        .replace(TEETH_HOLE, teeth)
}

/// `{teeth}` の実値: 契約の verify 行の filter 語を宣言順に `,` で結ぶ（filter を持たない行は飛ばし・0 本は
/// [`NO_TEETH`]・設計 gate-cost.md §34 約束 5）。語の導出は置き場の導出と同じ関数（[`closure::teeth_words`]）で、
/// 環境変数では渡さない（C2.2）。gate も land の主実測も [`Checks::contract`] から同じここを通る。
fn teeth_of(verify: &[String]) -> String {
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
/// `{jobs}` を持つ共通 verify の行は、**撃つ前に受付で枠を取り、撃った後に返す**
/// （設計 §3.2）。実効 jobs = `min(gate.mutants_jobs, 受け付けた枠)`。実効 thread は受付が jobs と
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

/// 行が受付を通るなら枠を取る（通らない行は `None`）。
///
/// 通るのは **`{jobs}` を持つ宣言の行**（共通 verify・検出線）だけである（`{jobs}` を持たない行は枠を取らない
/// ＝mutants を持たない consumer は費用を払わない・設計 §3.3）。**包めない周は 1 枠だけを
/// 取りにいく**——箱の無い行に並列度を上げると、溢れたときに殺されるのが席の側になる。
fn admitted(
    entry: &Fire<'_>,
    caps: Result<confine::Caps, RuleRead>,
    unit: &str,
    admit: Option<&Admit<'_>>,
) -> Option<Grant> {
    let admit = admit?;
    if !entry.holes || !entry.raw.contains(JOBS_HOLE) {
        return None;
    }
    // 包めるかは箱の大きさ（jobs ≥ 1）に依らない。撃つ前に同じ判定を 1 度だけ引く。
    let probe = confine::Wrap { unit, limit: confine::limit_of(entry.raw, UNADMITTED_JOBS), caps };
    let (_, confinement) = confine::wrap_line(entry.raw, &probe);
    let want = if confinement.confined() { admit.rules.cap } else { UNADMITTED_JOBS };
    let mut grant = admission::admit(admit.state_dir, admit.run, want, &admit.rules);
    grant.jobs = admit.rules.cap.min(grant.jobs).max(UNADMITTED_JOBS);
    Some(grant)
}

/// diff の path が契約の write-set に収まっているか（ADR-0009 §2.4）。
///
/// hook の guard とは**面が違う**: あちらは編集時に実体（symlink）まで解いて 1 件ずつ止める
/// backstop で、こちらは便が終わった後に git が出した名前を数える gate である。
fn check_write_set(checks: &Checks<'_>) -> Step {
    let cmd = WRITE_SET_CMD.to_owned();
    let range = format!("{}..HEAD", checks.base);
    // **`-z`**（NUL 区切り・quote しない）で受ける。既定の `--name-only` は非 ASCII の path を
    // `"…"` へ quote するので、字面照合が偽の RED を出す（本 repo は日本語の doc を持つ）。
    let Some(bytes) = git_bytes(checks.worktree, &["diff", "--name-only", "-z", &range]) else {
        return unwrapped(cmd, -1, "diff の path を読めない".to_owned());
    };
    let text = String::from_utf8_lossy(&bytes);
    let outside: Vec<&str> = text
        .split('\0')
        .filter(|path| !path.is_empty() && !listed(path, &checks.contract.write_set))
        .collect();
    if outside.is_empty() {
        return unwrapped(cmd, 0, String::new());
    }
    unwrapped(
        cmd,
        1,
        format!("契約の write-set の外へ出た path:\n{}", outside.join("\n")),
    )
}

/// path が write-set のいずれか（file の一致 か dir の prefix）に含まれるか。
///
/// 項目は [`refuse::normalize`] に通してから比べる（接頭辞 `+` / `-` は受付の宣言であって path の一部ではない
/// ＝diff の素の path と照合する・設計 contract-source.md §3・剥がす規則を 2 か所に持たない・`s2-07l.291`）。
fn listed(path: &str, write_set: &[String]) -> bool {
    write_set.iter().any(|entry| {
        let plain = refuse::normalize(entry);
        let trimmed = plain.trim_end_matches('/');
        path == trimmed || path.starts_with(&format!("{trimmed}/"))
    })
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
        fill_holes, gate_checks, last_line, listed, run_line_captured, teeth_of, unwrapped, Check, NO_TEETH, TEETH_HOLE,
        WRITE_SET_CMD,
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
        let wrap = Wrap { unit: "scribe2-mutant-unit", limit: Limit::HostReserve, caps: Err(RuleRead::Missing) };
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
        let wrap = Wrap { unit: "scribe2-secs-unit", limit: Limit::HostReserve, caps: Err(RuleRead::Missing) };
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
