//! gate（設計 docs/design/pipeline.md §5.3・FR8 / FR9 / NFR1）。
//!
//! 契約の `verify` 各行の逐条 rc（機械検証）と lens 1 本の判定を合わせて 3 値を出す。
//! **判定は wildcard 無しの順序で決める**: verify に rc≠0 → FAIL ／ diff byte が cap 超
//! → INCONCLUSIVE（lens を呼ばない）／ lens 側の不備 → INCONCLUSIVE ／ それ以外は
//! lens の verdict。
//!
//! **偽の PASS を作らない**（AC3）。判定に届かなかった周はすべて INCONCLUSIVE へ倒す
//! ——「測れなかった」を「通った」に化けさせないためで、極性は fail-closed（C11.2）。
//!
//! **同じ便を 2 度以上通ることが在る**（INCONCLUSIVE からの測り直し）。`verdict.json` は
//! 最後の判定で上書きし、`RunStage stage=Gated detail=verdict:<V>` は追記する。
//! 残るのは **3 値の履歴だけ**である——「1 度目は測れなかった」は event から読めるが、
//! **なぜ測れなかったか（evidence）は上書きで消える**（理由まで残すには面を 1 つ増やす
//! ことになり、MVP では取らない）。**測り直してよい便か**の判定はここではなく段の入口
//! （[`super::cli`]）が持つ。

use crate::polarity::{OnFailure, Polarity, Timing};
use super::admission::{self, Grant};
use super::confine::{self, Confinement, Reason, Usage};
use super::contract::Contract;
use super::declaration::{Effective, BASE_HOLE, JOBS_HOLE};
use super::{
    contract_path, emit, git_bytes, git_line, verdict_path, verify_log_path, vessel_path,
    worktree_path, Emit,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{append_line, LockPolicy};
use crate::fleet::{cli::now_utc, EventKind, Stage, SCHEMA};
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

/// 判定できなかったときの rc（設計 §5.3）。
///
/// `cli_outcome` の 0 / 1 / 2 は器の全 subcommand が共有する語彙で、3 を要るのは
/// gate の 3 値判定だけである。共有語彙へ足すと「断り」でも「壊れ」でもない値が
/// 全 subcommand の面に生えるので、**要る側の module に置く**。
pub const RC_INCONCLUSIVE: u8 = 3;

/// lens の出力から拾う JSON 行の始まり。
const JSON_HEAD: char = '{';

/// **受付を通らない行に渡す並列度**（設計 gate-cost.md §3.3・ADR-0021 §2.1）。
///
/// 宣言値（rules 行 `gate.mutants_jobs`）を**そのまま実効にしない**（C10: 宣言値・測定値・
/// 実効値は別物である）。実効値を上げてよいのは host 単位の受付（[`admission`]・設計 §3.2）で
/// 枠を取った行だけで、受付を持たない経路（land の main 実測・契約の verify 行）はこの値で撃つ
/// ——合計を守る面の無い経路が満額を取ると host の memory が溢れて席まで死ぬ。
///
/// 1 は常に許される（従来と同じ費用）ので、この値で縮退しても便は流れる。
const UNADMITTED_JOBS: u64 = 1;

/// write-set 照合の record に載せる `cmd`。**shell の行ではない**（Rust で照合する）ので、
/// 実行した行の字面を持てない段の名前をここで 1 つだけ決める。
const WRITE_SET_CMD: &str = "write-set";

/// 赤い verify 行の stderr を残す診断 file の名（`verify.jsonl` と同じ dir）。
///
/// **機械はこの file を読まない**。`verify.jsonl` の record（`schema` / `n` / `rc` /
/// `cmd`）は跨版の契約なので形を変えず、「なぜ赤かったか」だけを別の面へ逃がす。
const STDERR_LOG_FILE: &str = "verify.stderr.log";

/// 診断 file に残す stderr の行数（末尾から数える）。
///
/// **判定に効く値ではない**（人が理由を読むための窓の大きさ）ので規則行にしない
/// ——rules manifest は判定を動かす閾値の置き場である（憲法 C1 / C5）。**末尾**を
/// 採るのは、落ちた command が理由を最後に出すためである。
const STDERR_TAIL_LINES: usize = 20;

/// gate の段の極性（[`Check`]）: 実装の後に測り、判定に届かなかった周は INCONCLUSIVE（≠ PASS・AC3）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::PostHoc,
    on_failure: OnFailure::FailClosed,
};

/// gate の lens の極性（[`Verdict`]）: 実装の後に審査し、lens を呼べない・読めない周は INCONCLUSIVE（≠ PASS）。
pub const LENS_POLARITY: Polarity = Polarity {
    timing: Timing::PostHoc,
    on_failure: OnFailure::FailClosed,
};

/// 機械検証の段。**適用順序は [`CHECKS`] の並びが唯一の権威**である（憲法 C2）。
///
/// 順序を散文の注記で持たないための形である——enum が段の集合を閉じ、[`run_checks`] の
/// 網羅 match が新しい variant を必ずこの並びへ置かせる（置き忘れは compile error）。
/// 閉じた enum と全 variant の並びを対で持つのは器の既定の形である（[`VERDICTS`] /
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
    /// stderr の末尾（緑の段は空）。
    pub stderr: String,
    /// cgroup の scope で包めたか（record の `confined=`・設計 gate-cost.md §4）。
    pub confined: bool,
    /// 包めなかった理由 か 箱の中で起きたこと（record の `reason=`・閉じた enum）。
    pub reason: Option<Reason>,
    /// scope の peak（MiB）。**読めない周は `None`**＝record は `-`（0 と書かない）。
    pub peak_mb: Option<u64>,
    /// この行に渡した実効 jobs（record の `jobs=`）。
    pub jobs: u64,
    /// 受付の結果（record の `slot=`・受付を通らない行は `None`）。
    pub slot: Option<String>,
    /// 受付が測れなかった理由（record の `slot_why=`・測れた周と受付を通らない行は `None`）。
    pub slot_why: Option<admission::Unreadable>,
}

/// 撃つ process を持たない段（write-set 照合）の封じ込め欄。
///
/// 包めなかったのではなく**包む対象が無い**（Rust で照合するだけで子 process を起こさない）。
/// 理由を持たせないのはそのためである。
fn unwrapped(cmd: String, rc: i32, stderr: String) -> Step {
    Step {
        stage: Check::WriteSet,
        cmd,
        rc,
        stderr,
        confined: false,
        reason: None,
        peak_mb: None,
        jobs: UNADMITTED_JOBS,
        slot: None,
        slot_why: None,
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
}

/// 全段を**順序どおり**に撃つ。
///
/// **gate も land もこの 1 本を通る**——2 本になると gate が通した行と main で撃った行の
/// 意味が静かにずれる（行を撃つ実装を [`run_line_captured`] 1 本に保っているのと同じ理由）。
pub fn run_checks(checks: &Checks<'_>) -> Vec<Step> {
    run_checks_admitted(checks, None)
}

/// 全段を**順序どおり**に撃つ（受付を持つ形）。[`run_checks`] はこれの受付なしの形である。
///
/// `admit` が在る周だけ、`{jobs}` を持つ共通 verify の行が host の受付を通る（設計 gate-cost.md
/// §3.2・§3.3）。行を撃つ実装はこの 1 本のままである。
pub fn run_checks_admitted(checks: &Checks<'_>, admit: Option<&Admit<'_>>) -> Vec<Step> {
    // 封じ込めの 3 線は 1 便で 1 度だけ読む（行ごとに manifest を開き直さない）。
    let caps = confine::Caps::embedded();
    let mut steps = Vec::new();
    for check in CHECKS {
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
            steps.push(fire(&entry, caps, admit));
        }
    }
    steps
}

/// 共通 verify の行の穴を実値へ置く（**契約の行には置換しない**）。
///
/// **1 走査で埋めない**のは、穴の値が sha と数字だけで、互いの字面を含まないためである
/// （`{worktree}` のように外から来る path を埋める面とは条件が違う）。
fn fill_holes(line: &str, base: &str, jobs: u64) -> String {
    line.replace(BASE_HOLE, base)
        .replace(JOBS_HOLE, &jobs.to_string())
}

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
/// （設計 §3.2）。実効 jobs = `min(gate.mutants_jobs, 受け付けた枠)`。
fn fire(entry: &Fire<'_>, caps: Option<confine::Caps>, admit: Option<&Admit<'_>>) -> Step {
    let place = entry
        .checks
        .worktree
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unit = confine::unit_name(&place, entry.stage.as_str(), entry.n);
    let grant = admitted(entry, caps, &unit, admit);
    let jobs = grant.as_ref().map_or(UNADMITTED_JOBS, |held| held.jobs);
    let cmd = if entry.holes {
        fill_holes(entry.raw, entry.checks.base, jobs)
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
        confined,
        reason,
        peak_mb,
        jobs,
        slot,
        slot_why,
    }
}

/// 行が受付を通るなら枠を取る（通らない行は `None`）。
///
/// 通るのは **`{jobs}` を持つ宣言の行**（共通 verify・検出線）だけである（`{jobs}` を持たない行は枠を取らない
/// ＝mutants を持たない consumer は費用を払わない・設計 §3.3）。**包めない周は 1 枠だけを
/// 取りにいく**——箱の無い行に並列度を上げると、溢れたときに殺されるのが席の側になる。
fn admitted(
    entry: &Fire<'_>,
    caps: Option<confine::Caps>,
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
fn listed(path: &str, write_set: &[String]) -> bool {
    write_set.iter().any(|entry| {
        let trimmed = entry.trim_end_matches('/');
        path == trimmed || path.starts_with(&format!("{trimmed}/"))
    })
}

/// gate の 3 値。**bool で持たない**（「PASS でない」に 2 つの意味があるため）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 通した。
    Pass,
    /// 落ちた。
    Fail,
    /// 判定できなかった。
    Inconclusive,
}

/// [`Verdict`] の全 variant。
pub const VERDICTS: &[Verdict] = &[Verdict::Pass, Verdict::Fail, Verdict::Inconclusive];

impl Verdict {
    /// JSON と stdout に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Inconclusive => "INCONCLUSIVE",
        }
    }

    /// 字面から引く。3 値の外は `None`（＝呼び手が INCONCLUSIVE へ倒す）。
    pub fn parse(text: &str) -> Option<Self> {
        VERDICTS.iter().copied().find(|found| found.as_str() == text)
    }

    /// process の rc。
    pub fn rc(self) -> u8 {
        match self {
            Self::Pass => RC_OK,
            Self::Fail => RC_REFUSED,
            Self::Inconclusive => RC_INCONCLUSIVE,
        }
    }
}

/// 規則から読んだ線。**数値をこの file に焼かない**（憲法 C1 / C5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// 要る lens の本数（rules 行 `gate.lens_count`）。
    pub lens_count: u64,
    /// diff の byte 数の上限（rules 行 `gate.token_cap`）。
    pub token_cap: u64,
    /// 変異検査の並列度の上限（rules 行 `gate.mutants_jobs`・宣言値）。
    pub mutants_jobs: u64,
    /// job 1 つが要る memory（MiB・rules 行 `gate.job_memory_mb`）。
    pub job_memory_mb: u64,
    /// 席と host のために残す memory（MiB・rules 行 `host.reserve_memory_mb`）。
    pub reserve_memory_mb: u64,
    /// 受付で枠が空くのを待つ上限（秒・rules 行 `gate.slot_wait_s`）。
    pub slot_wait_s: u64,
}

/// gate 1 回の材料。
pub struct Gate<'a> {
    /// 便 id。
    pub run: &'a str,
    /// 契約の bead id。
    pub bead: &'a str,
    /// 対象 repo。
    pub repo: &'a Path,
    /// 置き場。
    pub state_dir: &'a Path,
    /// 読み込み済みの契約。
    pub contract: &'a Contract,
    /// lens のコマンド（無ければ `None`）。
    pub lens: Option<&'a str>,
    /// 規則から読んだ線。
    pub limits: Limits,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// 実測した 2 つの量。
struct Measured {
    /// rc≠0 だった verify 行の本数（**測れた行**だけを数える）。
    red: u64,
    /// `git diff <base>..HEAD` の生 byte（測れなかった周は空）。
    diff: Vec<u8>,
    /// 段①（write-set 照合）で diff の path を**読めなかった**か（`s2-07l.65`）。
    ///
    /// 読めない周は「測れなかった」であって赤ではない——rc -1 を赤に数えると、道具の
    /// 失敗が FAIL（判定に届いた便の終端）に化ける。land の `MainCheck::Unmeasurable` と
    /// 同じ極性で INCONCLUSIVE へ倒す（fail-closed は保つ＝PASS には決してならない）。
    unreadable: bool,
    /// 箱の中で殺された行の理由（在れば・設計 gate-cost.md §4.2）。
    ///
    /// 溢れた箱の中で死んだ行は、その内容が赤いのではなく**測れていない**。rc に依らず
    /// INCONCLUSIVE へ倒し、record の `reason=` で外からの kill と弁別する。
    killed: Option<Reason>,
}

/// 書き留める判定 1 件。
struct Decision {
    /// 3 値。
    verdict: Verdict,
    /// 理由（lens の evidence か、lens を呼ばなかった理由）。
    evidence: String,
    /// rc≠0 だった verify 行の本数。
    red: u64,
    /// diff の byte 数。
    diff_bytes: u64,
    /// gate を撃った HEAD の木（`HEAD^{tree}`・読めない周は `None`＝field を書かない）。
    tree: Option<String>,
}

/// gate を 1 回通す。
pub fn gate(entry: &Gate<'_>) -> Outcome {
    let worktree = worktree_path(entry.repo, entry.run);
    let Some(base) = super::base_of_run(entry.state_dir, entry.run) else {
        return refused(format!("run {} に base が無い（spawn を通っていない）", entry.run));
    };
    if let Some(reason) = precheck(&worktree, &base) {
        return precheck_failed(entry, &reason);
    }
    // **撃つ前の木を読む**（precheck が clean を見た木＝verify を撃つ木・設計 gate-cost.md §5）。
    let tree = git_line(&worktree, &["rev-parse", "HEAD^{tree}"]);
    let measured = match measure(entry, &worktree, &base) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let (verdict, evidence) = decide(entry, &worktree, &measured);
    let decision = Decision {
        verdict,
        evidence,
        red: measured.red,
        diff_bytes: byte_count(&measured.diff),
        tree,
    };
    match settle(entry, &decision) {
        Err(reason) => broken(reason),
        Ok(()) => Outcome {
            out: vec![format!("run={} verdict={}", entry.run, verdict.as_str())],
            err: Vec::new(),
            rc: verdict.rc(),
        },
    }
}

/// 前提を見る。満たしていれば `None`、違反なら理由 1 行。
///
/// **段（Implemented）の検査はここに置かない**。段違いは他の subcommand と同じく
/// 「何もしない rc 1」で、`Failed` を書いて便を終端させる筋合いが無いためである
/// （早く叩いただけの便が resume 不能になる）。ここが見るのは worktree の事実だけ。
fn precheck(worktree: &Path, base: &str) -> Option<String> {
    if !worktree.exists() {
        return Some(format!("worktree {} が無い", worktree.display()));
    }
    match git_bytes(worktree, &["status", "--porcelain"]) {
        None => return Some(format!("{} の状態を読めない", worktree.display())),
        Some(bytes) if !bytes.is_empty() => return Some("worktree が clean でない".to_owned()),
        Some(_) => {}
    }
    let range = format!("{base}..HEAD");
    let commits: u64 = git_line(worktree, &["rev-list", "--count", &range])
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    (commits < 1).then(|| "commit が 1 本も無い".to_owned())
}

/// verify を逐条で撃ち、diff を測る。
fn measure(entry: &Gate<'_>, worktree: &Path, base: &str) -> Result<Measured, String> {
    let counted = record_verify(entry, worktree, base)?;
    let (red, unreadable, killed) = (counted.red, counted.unreadable, counted.killed);
    if unreadable {
        // 段①が diff を読めない周は同じ range の生 diff も読めない。ここで broken（rc 2・
        // verdict を書かない）にすると便は Implemented のまま「測り直せる便」に見えない。
        return Ok(Measured { red, diff: Vec::new(), unreadable, killed });
    }
    let range = format!("{base}..HEAD");
    let diff = git_bytes(worktree, &["diff", &range])
        .ok_or_else(|| format!("{} の diff を測れない", worktree.display()))?;
    Ok(Measured { red, diff, unreadable, killed })
}

/// verify 各行を撃ち、行ごとの rc を `verify.jsonl` へ逐条で残す。
///
/// **赤い行だけ** stderr の末尾を診断 file（[`STDERR_LOG_FILE`]）へも append する。
/// rc だけでは「何がどう赤いか」が便の外から読めず、gate が落ちるたびに人が同じ行を
/// 手で撃ち直して理由を取り直すことになる（実測 2026-09-10・`s2-07l.49`）。緑の行は
/// 残さない——読む理由が無い出力で診断 file を埋めると、赤い行の見出しが埋もれる。
fn record_verify(entry: &Gate<'_>, worktree: &Path, base: &str) -> Result<Counted, String> {
    let frozen = frozen_copy(entry)?;
    let admit = Admit {
        state_dir: entry.state_dir,
        run: entry.run,
        rules: admission::Rules {
            sizes: admission::Sizes {
                job_mb: entry.limits.job_memory_mb,
                reserve_mb: entry.limits.reserve_memory_mb,
            },
            cap: entry.limits.mutants_jobs,
            wait_s: entry.limits.slot_wait_s,
            policy: entry.policy,
        },
    };
    let checks = Checks {
        worktree,
        base,
        contract: entry.contract,
        common: frozen.common_verify(),
        detection: frozen.detection_verify(),
    };
    let steps = run_checks_admitted(&checks, Some(&admit));
    let path = verify_log_path(entry.state_dir, entry.run);
    let tail_path = path.with_file_name(STDERR_LOG_FILE);
    let mut red = 0;
    // 段①が読めなかった周（rc -1）は**赤に数えない**——record は残す（現物を消さない）が、
    // 判定は「測れなかった」側へ倒す（`s2-07l.65`）。箱ごと OOM で殺された行も同じ極性で
    // ある（rc に依らず「測れなかった」・設計 gate-cost.md §4.2）。
    let unreadable = steps.iter().any(is_unreadable);
    let killed = steps.iter().find_map(box_kill);
    for (index, step) in steps.iter().enumerate() {
        let number = index as u64 + 1;
        if step.rc != 0 {
            if !is_unreadable(step) && box_kill(step).is_none() {
                red += 1;
            }
            let head = format!("## n={number} rc={} cmd={}", step.rc, step.cmd);
            append_stderr(&tail_path, entry.policy, &head, &step.stderr)?;
        }
        append_line(&path, &step_record(number, step), entry.policy).map_err(|err| err.to_string())?;
    }
    Ok(Counted { red, unreadable, killed })
}

/// 撃った 1 段の record（`verify.jsonl` と land の `verify-main.jsonl` が**同じ形**で書く）。
///
/// **schema は 1 のまま任意 field を足す**（古い読み手は未知の field を無視する・
/// ADR-0017 §2.1 の event と同じ足し方・設計 gate-cost.md §5）。
pub fn step_record(number: u64, step: &Step) -> String {
    let mut fields = vec![
        ("schema", Value::Num(SCHEMA)),
        ("n", Value::Num(number)),
        ("rc", Value::Num(recorded_rc(step.rc))),
        ("cmd", Value::Str(step.cmd.clone())),
        ("jobs", Value::Num(step.jobs)),
        ("confined", Value::Bool(step.confined)),
        ("peak_mb", Value::Str(shown_peak(step.peak_mb))),
        ("kind", Value::Str(step.stage.as_str().to_owned())),
    ];
    if let Some(reason) = step.reason {
        fields.push(("reason", Value::Str(reason.as_str().to_owned())));
    }
    if let Some(slot) = &step.slot {
        fields.push(("slot", Value::Str(slot.clone())));
    }
    if let Some(why) = step.slot_why {
        fields.push(("slot_why", Value::Str(why.as_str().to_owned())));
    }
    json_lite::write_object(&fields)
}

/// `verify.jsonl` の数え上げ（赤の本数と、2 つの「測れなかった」）。
struct Counted {
    /// rc≠0 だった verify 行の本数（**測れた行**だけを数える）。
    red: u64,
    /// 段①（write-set 照合）で diff の path を読めなかったか。
    unreadable: bool,
    /// 箱の中で殺された行の理由（在れば）。
    killed: Option<Reason>,
}

/// peak の字面。**読めない周は `-`**（0 と書かない＝「測って 0」と弁別する）。
fn shown_peak(peak_mb: Option<u64>) -> String {
    peak_mb.map_or_else(|| "-".to_owned(), |mb| mb.to_string())
}

/// 箱の中で殺された段か（殺された理由・設計 gate-cost.md §4.2 / §4.3）。
///
/// **rc では見ない**——kernel が殺した process の rc は行の内容次第で何にでもなる。根拠は
/// 包みが出した `memory.events` の `oom_kill` で、包みごと死んで終端行を出せなかった周は
/// signal 死をその代理にする。**包めなかった周は当たらない**（素の行が外から kill された
/// 周を「箱が溢れた」と読まない）。
fn box_kill(step: &Step) -> Option<Reason> {
    if !step.confined {
        return None;
    }
    step.reason
        .filter(|found| matches!(*found, Reason::OomKill | Reason::Signal))
}

/// 便の写し（共通 verify と検出線）を読む。
///
/// **読むのは `<state_dir>/pipe/<run>/vessel.toml` だけ**である——repo や worktree の
/// `.vessel.toml` を読み直すと、便の実装が自分の検証を書き換えられる（ADR-0010 §2.4）。
fn frozen_copy(entry: &Gate<'_>) -> Result<Effective, String> {
    let path = vessel_path(entry.state_dir, entry.run);
    Effective::load(&path)
        .map_err(|errors| {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            format!("{} を読めない: {}", path.display(), lines.join(" / "))
        })
}

/// 見出し 1 行と stderr の末尾を診断 file へ 1 件 append する。
///
/// **見出しは呼び手が組む**。n / rc / cmd をそのまま渡す形にすると引数が 5 個を超え、
/// C4 の線（`too_many_arguments`）に当たる。書き口は [`append_line`] の 1 本のまま
/// （lock を持つ writer を 2 本にしない・憲法 C6.3）。
fn append_stderr(path: &Path, policy: LockPolicy, head: &str, stderr: &str) -> Result<(), String> {
    append_line(path, &format!("{head}\n{stderr}"), policy).map_err(|err| err.to_string())?;
    Ok(())
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
    /// stderr の末尾（[`STDERR_TAIL_LINES`] 行）。
    pub stderr: String,
    /// 包みが stdout の終端に出した数（包めなかった周は既定）。
    pub usage: Usage,
    /// 包めたか。
    pub confinement: Confinement,
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
        if self.usage.oom_kill >= 1 {
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
/// stdout を読むのは**包みの終端行のため**だけで、判定には使わない（判定は rc である）。
pub fn run_line_captured(worktree: &Path, line: &str, wrap: &confine::Wrap<'_>) -> Fired {
    let (mut command, confinement) = confine::wrap_line(line, wrap);
    let spawned = command.current_dir(worktree).output();
    let Ok(out) = spawned else {
        // 起動できなかった周は rc も stderr も**器の外に無い**。空を「何も言わなかった」
        // として返し、極性は従来どおり RED 側（-1）へ倒す。
        return Fired { rc: -1, stderr: String::new(), usage: Usage::default(), confinement };
    };
    // **包めなかった周の stdout は読まない**。素の行が出した `confine-usage` の字面を
    // 包みの測定として読むと、撃たれた行が自分の peak を名乗れてしまう。
    let usage = if confinement.confined() {
        confine::read_usage(&String::from_utf8_lossy(&out.stdout))
    } else {
        Usage::default()
    };
    Fired {
        rc: out.status.code().unwrap_or(-1),
        stderr: tail_of(&String::from_utf8_lossy(&out.stderr)),
        usage,
        confinement,
    }
}

/// 末尾 [`STDERR_TAIL_LINES`] 行を改行で継いで返す（末尾の改行は行の区切りとして落ちる）。
fn tail_of(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(STDERR_TAIL_LINES);
    lines.get(from..).unwrap_or_default().join("\n")
}

/// rc を JSON の非負整数へ写す。`sh` が signal で落ちた周（負）は 255 に畳む。
fn recorded_rc(rc: i32) -> u64 {
    u64::try_from(rc).unwrap_or(u64::from(u8::MAX))
}

/// byte 数を数える。
fn byte_count(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

/// 判定順を 1 か所に閉じる（**wildcard 無し・上から順に効く**）。
fn decide(entry: &Gate<'_>, worktree: &Path, measured: &Measured) -> (Verdict, String) {
    // **測れなかったは赤より先**（C10・AC3）。段①の diff が読めない周は判定に届いていない
    // ので lens も呼ばず INCONCLUSIVE（測り直せる側・FR14）。
    if measured.unreadable {
        return (
            Verdict::Inconclusive,
            "diff の path を読めない（write-set を照合できない＝測れなかった）".to_owned(),
        );
    }
    // **箱の中で殺された行も赤より先**（設計 gate-cost.md §4.2）。溢れた箱の中で死んだ行は
    // 内容が赤いのではなく測れていない——赤に化けさせると、host の memory が足りない周ほど
    // 便が FAIL（終端）で落ちる。
    if let Some(reason) = measured.killed {
        return (
            Verdict::Inconclusive,
            format!(
                "verify の行が scope の中で死んだ（reason={}・測れなかった）",
                reason.as_str()
            ),
        );
    }
    if measured.red > 0 {
        return (
            Verdict::Fail,
            format!("verify の {} 行が rc≠0", measured.red),
        );
    }
    let size = byte_count(&measured.diff);
    if size > entry.limits.token_cap {
        // **換算係数を持たない**（NFR1）。byte ≥ token の保守的な読みで直接比べる。
        return (
            Verdict::Inconclusive,
            format!("diff {size} byte が cap {} を超えた", entry.limits.token_cap),
        );
    }
    // **本数は照合する**。0 本（lens を呼ばずに通す）も 2 本以上（1 本で足りたことに
    // する）も「lens の verdict」を得ていないので、判定順の 4 番目は成立しない。
    // どちらも判定できていない周ゆえ INCONCLUSIVE へ倒す（AC3・C11.2）。
    if entry.limits.lens_count != 1 {
        return (
            Verdict::Inconclusive,
            format!(
                "規則は lens {} 本を定める（通せるのは 1 本だけ）",
                entry.limits.lens_count
            ),
        );
    }
    let Some(cmd) = entry.lens else {
        return (Verdict::Inconclusive, "lens が要るのに --lens が無い".to_owned());
    };
    let contract = contract_path(entry.state_dir, entry.run);
    let unit = confine::unit_name(entry.run, LENS_STAGE, 1);
    let wrap = confine::Wrap {
        unit: &unit,
        // lens は `{jobs}` を持たない起動なので host の箱である（設計 gate-cost.md §4.2）。
        limit: confine::Limit::HostReserve,
        caps: confine::Caps::embedded(),
    };
    ask_lens(&substitute(cmd, &contract, worktree), worktree, &measured.diff, &wrap)
}

/// lens の scope の unit 名に載せる段の名。
const LENS_STAGE: &str = "lens";

/// `--lens` の cmd の `{contract}` / `{worktree}` を run の path へ置く。
///
/// **置く穴は 2 つである**（`{contract}` / `{worktree}`・出所 s2-07l.60）。1 つだった頃の
/// 理由（読み手を増やさない・planner 裁定 2026-09-10 Q1）は生きているが、lens に憲法を
/// 載せる経路が起動 cwd しか無く、tracked file に絶対 path は書けない（PUBLIC repo）ので
/// worktree は gate が埋めるほかない（planner 裁定 2026-09-10・admin Q2）。`--runner` 側
/// （[`super::spawn`]）と共有するのは placeholder の**語彙**であって関数ではない。
///
/// **1 走査で埋める**。重ねて replace すると、先に埋めた path の中の `{worktree}` まで
/// 展開されうる（runner / lens の prompt と同じ理由）。
///
/// **渡すのは path であって本文ではない**。cmd は `sh -c` へ渡る 1 行なので、本文を
/// 埋めると契約の中の引用符 1 つで cmd の構造が変わる。
fn substitute(cmd: &str, contract: &Path, worktree: &Path) -> String {
    crate::headless::fill(
        cmd,
        &[
            ("{contract}", &contract.display().to_string()),
            ("{worktree}", &worktree.display().to_string()),
        ],
    )
}

/// lens へ diff を stdin で渡し、stdout の JSON 1 行を読む。
///
/// 契約は cmd の `{contract}`（[`substitute`] が埋めた path）で渡る＝**stdin は diff 専用**。
///
/// lens も scope で包む（設計 gate-cost.md §4.1 の 3 つ目）。**箱の中で殺された周は
/// INCONCLUSIVE** ——FR9 の既存極性そのままで、stdout を parse できない周と同じ経路である
/// （判定順は動かさない・便の成果は残っているので終端しない・設計 §4.2）。
fn ask_lens(
    cmd: &str,
    worktree: &Path,
    diff: &[u8],
    wrap: &confine::Wrap<'_>,
) -> (Verdict, String) {
    let (mut command, confinement) = confine::wrap_line(cmd, wrap);
    let spawned = command
        .current_dir(worktree)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return (Verdict::Inconclusive, format!("lens を起動できない: {err}")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        // 読まずに終える lens への write は EPIPE になる。**判定は出力で決める**ので
        // ここの失敗は理由にしない（take で drop され、lens は EOF を見る）。
        let _ = stdin.write_all(diff);
    }
    let out = match child.wait_with_output() {
        Ok(found) => found,
        Err(err) => return (Verdict::Inconclusive, format!("lens の出力を読めない: {err}")),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    // **箱の中で死んだ周は rc より先に見る**（設計 §4.2）。溢れた箱で死んだ lens の rc を
    // 「lens が rc N で終わった」と記すと、外からの kill と弁別できない（lens-132d L1）。
    if confinement.confined() {
        let usage = confine::read_usage(&text);
        let killed = (usage.oom_kill >= 1).then_some(Reason::OomKill);
        let killed = killed.or_else(|| (out.status.code().is_none()).then_some(Reason::Signal));
        if let Some(reason) = killed {
            return (
                Verdict::Inconclusive,
                format!("lens が scope の中で死んだ（reason={}）", reason.as_str()),
            );
        }
    }
    if !out.status.success() {
        let rc = out.status.code().unwrap_or(-1);
        return (Verdict::Inconclusive, format!("lens が rc {rc} で終わった"));
    }
    parse_lens(&text)
}

/// stdout の**最後の JSON 行**を 1 つの flat object に読む（lens の verdict と runner の
/// 質問 record が**共有する 1 本**・設計 pipeline-question.md §3）。読む条件は呼び手が持つ
/// （gate は lens の rc 0 の周・spawn は包みの rc [`super::RC_QUESTION`] の周）。
pub(crate) fn last_json_object(text: &str) -> Result<Vec<(String, Value)>, String> {
    let found = text
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with(JSON_HEAD));
    let Some(line) = found else {
        return Err("出力に JSON 行が無い".to_owned());
    };
    json_lite::parse_object(line.trim()).map_err(|reason| format!("出力を読めない: {reason}"))
}

/// lens の stdout から最後の JSON 行を読む。読めない周は INCONCLUSIVE。
fn parse_lens(text: &str) -> (Verdict, String) {
    let pairs = match last_json_object(text) {
        Ok(parsed) => parsed,
        Err(reason) => return (Verdict::Inconclusive, format!("lens の{reason}")),
    };
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(found_key, _)| found_key == key)
            .and_then(|(_, value)| value.as_str())
    };
    let evidence = get("evidence").unwrap_or_default().to_owned();
    match get("verdict").and_then(Verdict::parse) {
        Some(verdict) => (verdict, evidence),
        None => (
            Verdict::Inconclusive,
            "lens の verdict が 3 値でない".to_owned(),
        ),
    }
}

/// 書きかけの `verdict.json` の拡張子（同じ dir に置いて rename する）。
const VERDICT_PARTIAL_EXT: &str = "json.partial";

/// `verdict.json` を **atomic に**書く（`s2-07l.147`・設計 gate-cost.md §6・**書きはこの 1 本**）。
///
/// 同じ dir の書きかけへ書いて rename する＝読み手（land の着地待ちの列）は途中の file を見ない。
/// 撃ち直しで判定を書き直す瞬間を「読めない」と測らせない（`Unmeasurable` の瞬間を出す側で塞ぐ）。
/// 書けなかった周は書きかけを残さず、本 file も生まれない（前の判定のまま）。
fn write_verdict(path: &Path, text: &str) -> Result<(), String> {
    let partial = path.with_extension(VERDICT_PARTIAL_EXT);
    std::fs::write(&partial, text)
        .and_then(|()| std::fs::rename(&partial, path))
        .map_err(|err| {
            let _ = std::fs::remove_file(&partial);
            format!("{} を書けない: {err}", path.display())
        })
}

/// 判定を `verdict.json` へ書き、`Gated` を 1 件追記する。
///
/// **測り直しの周も同じ経路を通る**: file は最後の判定で上書きし、event は追記する。
fn settle(entry: &Gate<'_>, decision: &Decision) -> Result<(), String> {
    let mut fields = vec![
        ("schema", Value::Num(SCHEMA)),
        ("run", Value::Str(entry.run.to_owned())),
        ("verdict", Value::Str(decision.verdict.as_str().to_owned())),
        ("evidence", Value::Str(decision.evidence.clone())),
        ("verify_red", Value::Num(decision.red)),
        ("diff_bytes", Value::Num(decision.diff_bytes)),
    ];
    // **schema は 1 のまま field を足す**（読み手は未知の field を無視する）。読めない周は書かない
    // ——land は `tree` の無い verdict を「木を比べられない」として全段を撃つ（設計 gate-cost.md §5）。
    if let Some(tree) = &decision.tree {
        fields.push(("tree", Value::Str(tree.clone())));
    }
    fields.push(("ts", Value::Str(now_utc())));
    let body = json_lite::write_object(&fields);
    write_verdict(&verdict_path(entry.state_dir, entry.run), &format!("{body}\n"))?;
    emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Gated),
            seat: None,
            pid: None,
            detail: Some(format!("verdict:{}", decision.verdict.as_str())),
        },
        entry.policy,
    )
    .map_err(|err| err.to_string())
}

/// 前提違反を `Failed detail=precheck:<理由>` で残して断る（lens は起動しない）。
fn precheck_failed(entry: &Gate<'_>, reason: &str) -> Outcome {
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Failed),
            seat: None,
            pid: None,
            detail: Some(format!("precheck:{reason}")),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => refused(format!("gate の前提を満たさない（{reason}）")),
    }
}

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::{substitute, write_verdict};
    use std::path::{Path, PathBuf};

    /// 歯ごとの空の tmp dir（in-file の歯の置き場・env を読まないのは器の本体の規律〔C2.2〕）。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gate-verdict-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// dir の直下の名前（名前順）。
    fn names(dir: &Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    }

    /// 判定の書きは完了後に書きかけを残さず、本 file の中身は完全（前の判定を丸ごと置き換える）。
    #[test]
    fn pipe_order_verdict_write_is_whole_and_leaves_no_partial() {
        let dir = scratch("whole");
        let path = dir.join("verdict.json");
        std::fs::write(&path, "{\"verdict\":\"PASS\",\"evidence\":\"a longer previous verdict\"}\n")
            .expect("前の判定を置ける");
        let body = "{\"schema\":1,\"verdict\":\"FAIL\"}\n";
        assert_eq!(write_verdict(&path, body), Ok(()));
        assert_eq!(std::fs::read_to_string(&path).ok().as_deref(), Some(body), "本 file の中身が完全");
        assert_eq!(names(&dir), ["verdict.json"], "書きかけを残さない");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 書けない周は本 file が生まれず（部分 file 0）、書きかけも残さない: 親 dir が無い / 行き先が dir で
    /// rename が断られる（書きかけは書けた後に落ちる形）。
    #[test]
    fn pipe_order_verdict_write_failure_leaves_no_partial_file() {
        let dir = scratch("unwritable");
        let absent = dir.join("absent").join("verdict.json");
        assert!(write_verdict(&absent, "{}\n").is_err(), "親 dir が無い");
        assert!(!absent.exists(), "本 file が生まれない");
        let blocked = dir.join("verdict.json");
        std::fs::create_dir_all(blocked.join("inside")).expect("行き先を dir で塞げる");
        assert!(write_verdict(&blocked, "{}\n").is_err(), "rename が断られる");
        assert!(blocked.is_dir(), "行き先は元のまま");
        assert_eq!(names(&dir), ["verdict.json"], "書きかけを残さない");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `--lens` の cmd の穴は **2 つ**（契約 / worktree）で、どちらも埋まる。
    ///
    /// worktree 側が埋まらないと lens は `--worktree` を受け取れず rc 1 で断り、gate は
    /// INCONCLUSIVE になる（＝憲法の載らない判定は出ないが、便も進まない）。
    #[test]
    fn gate_substitute_fills_contract_and_worktree() {
        let line = substitute(
            "lens --contract {contract} --worktree {worktree}",
            Path::new("/state/CONTRACT-MARKER.toml"),
            Path::new("/runs/WORKTREE-MARKER"),
        );
        assert_eq!(
            line,
            "lens --contract /state/CONTRACT-MARKER.toml --worktree /runs/WORKTREE-MARKER",
        );
    }

    /// **1 走査で埋める**。埋めた値の中の marker は展開しない。
    ///
    /// 重ねて replace すると、契約 path の中に `{worktree}` が在るだけで cmd の構造へ
    /// 触れられる（runner / lens の prompt と同じ経路）。
    #[test]
    fn gate_substitute_does_not_expand_filled_values() {
        let line = substitute(
            "lens --contract {contract}",
            Path::new("/state/{worktree}/CONTRACT-MARKER.toml"),
            Path::new("/runs/WORKTREE-MARKER"),
        );
        assert_eq!(line, "lens --contract /state/{worktree}/CONTRACT-MARKER.toml");
    }
}
