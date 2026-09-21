//! gate（設計 docs/design/pipeline.md §5.3・FR8 / FR9 / NFR1）。
//!
//! 契約の `verify` 各行の逐条 rc（機械検証）と lens 1 本の判定を合わせて 3 値を出す。
//! **判定は wildcard 無しの順序で決める**: 測れなかった（段①が読めない・箱の中の死・器の健康の
//! 遮断器が閉じた＝設計 gate-cost.md §32）→ INCONCLUSIVE ／ verify に rc≠0 → FAIL ／ 検出線の rc 2（赤が 0 の周だけ・設計
//! gate-cost.md §28）→ INCONCLUSIVE ／ lens に渡す本文の byte が cap 超
//! → INCONCLUSIVE（lens を呼ばない）／ lens 側の不備 → INCONCLUSIVE（出力の**形が読めなかった**
//! 周だけ同じ gate の中で 1 回撃ち直し、2 回目の戻りで読む・設計 gate-cost.md §29）／ それ以外は
//! lens の verdict。lens に渡す本文は閉じた型 [`LensInput`]（diff か、純移動の要約・
//! [`super::move_proof`]・`s2-07l.266`）で、判定は純関数・file の読みだけをここが担う。
//!
//! **偽の PASS を作らない**（AC3）。判定に届かなかった周はすべて INCONCLUSIVE へ倒す
//! ——「測れなかった」を「通った」に化けさせないためで、極性は fail-closed（C11.2）。
//!
//! **lens の口座も器が選ぶ**（設計 account-autonomy.md §15・`s2-07l.412`）。lens を起こす直前に便用の
//! 選定（計測 → [`super::ratelimit::select_lens_account`]）を通し、起動行の末尾に runner と同じ 1 関数
//! （[`super::spawn::with_account`]）で `--account-dir` を足す。**候補なしでも待たない**（gate は段の判定で
//! 待ちを持たない）——lens を起こさず INCONCLUSIVE へ倒し、`resume` が撃ち直す。宣言 0 の周は継承。
//!
//! **同じ便を 2 度以上通ることが在る**（INCONCLUSIVE からの測り直し）。`verdict.json` は
//! 最後の判定で上書きし、`RunStage stage=Gated detail=verdict:<V>`（器が口座を選んだ周は
//! `,account:<label>` 付き）は追記する。
//! 残るのは **3 値の履歴だけ**である——「1 度目は測れなかった」は event から読めるが、
//! **なぜ測れなかったか（evidence）は上書きで消える**（理由まで残すには面を 1 つ増やす
//! ことになり、MVP では取らない）。**測り直してよい便か**の判定はここではなく段の入口
//! （[`super::cli`]）が持つ。
//!
//! 本 file は判定の入口と終端（[`gate`] → `precheck` → `measure` → `decide` → `settle`）と型・定数を
//! 持つ。verify 行の実行は [`verify`]、lens の呼び出しと parse は [`lens`]、記録と診断は [`record`]
//! （`s2-07l.286` の純移動・外から呼ぶ path は本 file の再輸出で不変）。周ごとの検出線の写しも
//! [`record`] が持つ（書き手 = 記録と同じ 1 本・読み手 = [`detection_copies`]・設計 gate-cost.md §15）。

mod findings;
mod lens;
mod record;
mod verify;

pub(crate) use lens::last_json_object;
pub use record::{
    detection_copies, next_number, records_of, skip_record, step_record, DetectionCopy, Record, Skipped,
};
pub use verify::{is_unreadable, run_checks, Check, Checks, Step, CHECKS};

use crate::polarity::{OnFailure, Polarity, Timing};
use super::confine::{self, Reason, Released};
use super::contract::Contract;
use super::cli::int_row;
use super::health;
use super::lens_record::LensSource;
use super::move_proof::{self, LensInput, NotPure};
use super::ratelimit::{select_lens_account, LensAccount, Pool};
use super::{
    contract_path, emit, git_bytes, git_line, run_dir, verdict_path, worktree_path, Emit,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::LockPolicy;
use crate::fleet::{cli::now_utc, EventKind, Stage, SCHEMA};
use crate::rules::manifest::Manifest;
use findings::Tally;
use lens::{ask_lens, lens_input, substitute, unjudged, write_verdict, Judged, LENS_STAGE};
use record::{record_notice, record_verify};
use std::path::Path;
use verify::byte_count;

/// 判定できなかったときの rc（設計 §5.3）。
///
/// `cli_outcome` の 0 / 1 / 2 は器の全 subcommand が共有する語彙で、3 を要るのは
/// gate の 3 値判定だけである。共有語彙へ足すと「断り」でも「壊れ」でもない値が
/// 全 subcommand の面に生えるので、**要る側の module に置く**。
pub const RC_INCONCLUSIVE: u8 = 3;

/// lens の出力から拾う JSON 行の始まり。
pub(super) const JSON_HEAD: char = '{';

/// **受付を通らない行に渡す並列度**（設計 gate-cost.md §3.3・ADR-0021 §2.1）。
///
/// 宣言値（rules 行 `gate.mutants_jobs`）を**そのまま実効にしない**（C10: 宣言値・測定値・
/// 実効値は別物である）。実効値を上げてよいのは host 単位の受付（[`super::admission`]・設計 §3.2）で
/// 枠を取った行だけで、受付を持たない経路（land の main 実測・契約の verify 行）はこの値で撃つ
/// ——合計を守る面の無い経路が満額を取ると host の memory が溢れて席まで死ぬ。
///
/// 1 は常に許される（従来と同じ費用）ので、この値で縮退しても便は流れる。
pub(super) const UNADMITTED_JOBS: u64 = 1;

/// write-set 照合の record に載せる `cmd`。**shell の行ではない**（Rust で照合する）ので、
/// 実行した行の字面を持てない段の名前をここで 1 つだけ決める。
pub(super) const WRITE_SET_CMD: &str = "write-set";

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
    /// 受付で枠が空くのを待つ上限（秒・rules 行 `gate.slot_wait_s`）。器の健康の遮断器の待ちの上限も
    /// これを使い回す（3 本目の値の線を足さない・設計 gate-cost.md §32 約束 4）。
    pub slot_wait_s: u64,
    /// 走行可能の core あたりの倍率（rules 行 `host.runnable_per_core`・設計 gate-cost.md §32 約束 2）。
    pub runnable_per_core: u64,
    /// 待ちの core あたりの倍率（rules 行 `host.blocked_per_core`）。
    pub blocked_per_core: u64,
}

/// gate が要る lens の本数を持つ rules 行。
const ROW_LENS: &str = "gate.lens_count";

/// gate の diff 上限（byte）を持つ rules 行。
const ROW_CAP: &str = "gate.token_cap";

/// 変異検査の並列度の上限を持つ rules 行（受付の宣言値）。
const ROW_MUTANTS_JOBS: &str = "gate.mutants_jobs";

/// job 1 つが要る memory（MiB）を持つ rules 行（受付の分母）。
const ROW_JOB_MEMORY: &str = "gate.job_memory_mb";

/// 席と host のために残す memory（MiB）を持つ rules 行（受付の差引）。
const ROW_RESERVE_MEMORY: &str = "host.reserve_memory_mb";

/// 受付で枠が空くのを待つ上限（秒）を持つ rules 行。
const ROW_SLOT_WAIT: &str = "gate.slot_wait_s";

/// 器の健康の遮断器の走行可能の core あたりの倍率を持つ rules 行（設計 gate-cost.md §32）。
const ROW_RUNNABLE_PER_CORE: &str = "host.runnable_per_core";

/// 器の健康の遮断器の待ちの core あたりの倍率を持つ rules 行。
const ROW_BLOCKED_PER_CORE: &str = "host.blocked_per_core";

impl Limits {
    /// 規則から gate の線（判定の 2 行・受付の 4 行・遮断器の倍率 2 行）を読む。**数値を .rs へ焼かない**（憲法 C1 / C5）。
    ///
    /// 受付の 4 行と遮断器の 2 行も `--rules` の manifest から読む（埋め込みから直に読まない）——待ちの上限と
    /// 倍率を振る歯が fixture の値を gate へ届ける口はここだけである。`pipe/cli/step.rs` から純移動した読み手 1 本で、
    /// 列の遮断器（`pipe::dispatch`・設計 dispatcher.md §18）も同じ 1 本で倍率を読む。
    pub(crate) fn of(manifest: &Manifest) -> Result<Limits, String> {
        Ok(Limits {
            lens_count: int_row(manifest, ROW_LENS)?,
            token_cap: int_row(manifest, ROW_CAP)?,
            mutants_jobs: int_row(manifest, ROW_MUTANTS_JOBS)?,
            job_memory_mb: int_row(manifest, ROW_JOB_MEMORY)?,
            reserve_memory_mb: int_row(manifest, ROW_RESERVE_MEMORY)?,
            slot_wait_s: int_row(manifest, ROW_SLOT_WAIT)?,
            runnable_per_core: int_row(manifest, ROW_RUNNABLE_PER_CORE)?,
            blocked_per_core: int_row(manifest, ROW_BLOCKED_PER_CORE)?,
        })
    }

    /// 器の健康の遮断器の材料（gate と land の主実測の 2 つの [`Checks`] が同じ欄をここから埋める・§32 約束 9）。
    pub fn breaker(&self) -> health::Breaker {
        health::Breaker {
            per_core: health::PerCore { runnable: self.runnable_per_core, blocked: self.blocked_per_core },
            wait_s: self.slot_wait_s,
        }
    }
}

/// 検出線（変異検査）を撃つか（**閉じた enum**・設計 §30・`s2-07l.397`）。
///
/// literal の構築点は 2 つ——`pipe gate`（[`super::cli`]・常に [`Run`](Self::Run)）と、main が動いた便の追随
/// （[`super::land`]・`<base>..<main>` の path が検出線の面に 1 つも触れない周だけ [`Skip`](Self::Skip)）。
/// 撃たない周も `verify.jsonl` に `kind=detection skipped=detection reason=<理由>` の record を残す
/// （**撃たなかった事実を黙って落とさない**・[`skip_record`]）。共通 verify と契約 verify は従来どおり撃つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detection {
    /// 撃つ（従来の形・読めない周もこちら＝fail-closed）。
    Run,
    /// 撃たない（理由は record の `reason=`）。
    Skip(DetectionSkip),
}

/// 検出線を撃たない理由（record の `reason=`・閉じた enum・憲法 C11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionSkip {
    /// 差分の path が検出線の面（[`super::land::DETECTION_SCOPE`]）に 1 つも触れない。
    OutsideScope,
    /// gate を撃った木と land した木が同じ（主実測だけ・ADR-0021 §2.4）。
    SameTree,
}

impl DetectionSkip {
    /// record の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OutsideScope => "outside-scope",
            Self::SameTree => "same-tree",
        }
    }
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
    /// lens のコマンドの出所（`--lens` か run dir の写し・無い / 読めないは別の値・[`super::lens_record`]・§26）。
    pub lens: &'a LensSource,
    /// lens の口座の選定の材料（[`Pool::declared`]・宣言が 1 つ以上在る周だけ `Some`・設計
    /// account-autonomy.md §15）。`None` の周は口座を選ばず、lens は親の環境を継承する（起動行は不変）。
    pub pool: Option<&'a Pool>,
    /// 規則から読んだ線。
    pub limits: Limits,
    /// 検出線を撃つか（設計 §30・追随の再 gate だけが [`Detection::Skip`] を渡しうる）。
    pub detection: Detection,
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
    /// INCONCLUSIVE へ倒し、record の `reason=` で外からの kill と弁別する（検出線の行の
    /// `oom_kill` は除く＝道具が吸収して完走した周・`record::box_kill`）。
    killed: Option<Reason>,
    /// 検出線が rc 2（測れなかった）で終えた行の `n`（在れば・`s2-07l.331`・設計 §5.3）。
    ///
    /// `cargo xtask mutants-diff` の rc 2 は「生存も時間切れも無いが測れていない」（baseline が
    /// 落ちた等）で、赤（rc 1 = deny 昇格後）ではない。赤に数えると測れなかった便が FAIL で終端し
    /// 測り直せない（C10）。rc 1 の検出線と検出線以外の rc 2 は従来どおり赤（`record::detection_unmeasured`）。
    /// INCONCLUSIVE へ倒すのは [`red`](Self::red) が 0 の周だけ（赤が在れば FAIL が先・設計 gate-cost.md §28）。
    detection_unmeasured: Option<u64>,
    /// 器の健康の遮断器が閉じて撃たなかった行の `n`（在れば・設計 gate-cost.md §32 約束 5 / 6）。
    ///
    /// 凍った host の下の行は内容を測れていない——赤が在っても信用できないので、赤より先に INCONCLUSIVE へ倒す
    /// （負荷で便を終端させない・負荷が引いた後に同じ木を測り直せる）。
    busy: Option<u64>,
    /// lens に渡す本文の型（純移動の要約か diff か・設計 §5.3・測れなかった周は diff）。
    input: LensInput,
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
    /// lens の findings の集計（`s2-07l.188`・読めた周だけ `Some`＝field `findings` / `population`）。
    tally: Option<Tally>,
    /// lens の scope を片付けた結果（record に書く周だけ `Some`＝field `scope`・設計 gate-cost.md §4.4 errata）。
    scope: Option<Released>,
    /// lens を起こした口座（器が選んだ周だけ `Some`＝`Gated` の detail の `account:<label>`・設計
    /// account-autonomy.md §15）。宣言 0 の周と選べなかった周は `None`（足さない＝継承と弁別できる・C10）。
    account: Option<String>,
}

/// [`decide`] の戻り（判定・lens の scope の片付け・lens を起こした口座）。
struct Decided {
    /// lens 1 本から得た 3 値（lens を呼ばなかった周は [`unjudged`]）。
    judged: Judged,
    /// lens の scope を片付けた結果（lens を撃たない周は `None`）。
    scope: Option<Released>,
    /// 器が選んで起動行に足した口座（選ばなかった周は `None`）。
    account: Option<String>,
}

/// gate を 1 回通す。
pub fn gate(entry: &Gate<'_>) -> Outcome {
    let worktree = worktree_path(entry.repo, entry.run);
    // **「無い」と「読めない」を分ける**（C10）: 置き場が壊れている周を前提違反に化けさせない。
    let base = match super::base_of_run(entry.state_dir, entry.run) {
        super::Base::Known(found) => found,
        super::Base::Absent => return refused(format!("run {} に base が無い（spawn を通っていない）", entry.run)),
        super::Base::Unreadable => return broken(format!("run {} の base を読めない（置き場）", entry.run)),
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
    // 口座の計測の行は stderr 側へ写す（`fleet select` と同じ・判定は変えない）。
    let mut notes = Vec::new();
    let decided = match decide(entry, &worktree, &measured, &mut notes) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let decision = Decision {
        verdict: decided.judged.verdict,
        evidence: decided.judged.evidence,
        red: measured.red,
        diff_bytes: byte_count(&measured.diff),
        tree,
        tally: decided.judged.tally,
        scope: decided.scope,
        account: decided.account,
    };
    match settle(entry, &decision) {
        Err(reason) => broken(reason),
        Ok(()) => Outcome {
            out: vec![format!(
                "run={} verdict={} lens-input={} bytes={}",
                entry.run,
                decision.verdict.as_str(),
                measured.input.kind(),
                byte_count(measured.input.body(&measured.diff))
            )],
            err: notes.into_iter().chain(measured.input.notice()).collect(),
            rc: decision.verdict.rc(),
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

/// verify を逐条で撃ち、diff を測り、**lens へ何を渡すかを記録へ残す**。
///
/// 通知（[`record_notice`]・設計 §21 (3)）は rc に依らず・入力の型に依らず 1 行で、判定は動かさない
/// ——`Outcome.err` の 1 行は呼び手が捨てうる面なので、事後に読める面（run dir）にも同じ事実を置く。
fn measure(entry: &Gate<'_>, worktree: &Path, base: &str) -> Result<Measured, String> {
    let counted = record_verify(entry, worktree, base)?;
    let (red, unreadable, killed) = (counted.red, counted.unreadable, counted.killed);
    let (detection_unmeasured, busy) = (counted.detection_unmeasured, counted.busy);
    // 段①が diff を読めない周は同じ range の生 diff も読めない。ここで broken（rc 2・
    // verdict を書かない）にすると便は Implemented のまま「測り直せる便」に見えない。
    let (diff, input) = if unreadable {
        (Vec::new(), LensInput::Diff(NotPure::Unreadable))
    } else {
        let range = format!("{base}..HEAD");
        let diff = git_bytes(worktree, &["diff", &range])
            .ok_or_else(|| format!("{} の diff を測れない", worktree.display()))?;
        let input = lens_input(worktree, base, &diff);
        (diff, input)
    };
    record_notice(entry, &input)?;
    Ok(Measured { red, diff, unreadable, killed, detection_unmeasured, busy, input })
}

/// 判定順を 1 か所に閉じる（**wildcard 無し・上から順に効く**）。
///
/// 戻りは [`Decided`]（判定・lens の scope の片付け・lens を起こした口座）。`Err` は裁定の写しを
/// 書けなかった周（判定に届かず gate を止める＝rc 2・verdict を書かない）。`notes` は stderr へ写す行。
fn decide(
    entry: &Gate<'_>,
    worktree: &Path,
    measured: &Measured,
    notes: &mut Vec<String>,
) -> Result<Decided, String> {
    // 機械検証の段の判定（測れなかった → 遮断器 → 赤 → 検出線の rc 2）は pure な 1 本で先に読む。
    if let Some(judged) = machine_order(measured) {
        return Ok(Decided { judged, scope: None, account: None });
    }
    // **予算の照合は lens に渡す本文の byte で行う**（FR9・純移動の周は要約・`verdict.json` の
    // `diff_bytes` は従来どおり diff の byte）。
    let body = measured.input.body(&measured.diff);
    let size = byte_count(body);
    if size > entry.limits.token_cap {
        // **換算係数を持たない**（NFR1）。byte ≥ token の保守的な読みで直接比べる。
        return inconclusive(format!(
            "{} {size} byte が cap {} を超えた",
            measured.input.kind(),
            entry.limits.token_cap
        ));
    }
    // **本数は照合する**。0 本（lens を呼ばずに通す）も 2 本以上（1 本で足りたことに
    // する）も「lens の verdict」を得ていないので、判定順の 4 番目は成立しない。
    // どちらも判定できていない周ゆえ INCONCLUSIVE へ倒す（AC3・C11.2）。
    if entry.limits.lens_count != 1 {
        return inconclusive(format!(
            "規則は lens {} 本を定める（通せるのは 1 本だけ）",
            entry.limits.lens_count
        ));
    }
    // **無いと読めないは別の理由**（設計 §26・C10）: 写しの無い周は従来の字面・在って読めない周は path と理由。
    let cmd = match entry.lens {
        LensSource::Cmd(cmd) => cmd.as_str(),
        LensSource::Absent => return inconclusive("lens が要るのに --lens が無い".to_owned()),
        LensSource::Unreadable { path, reason } => {
            return inconclusive(format!("lens の写し {} を読めない（{reason}）", path.display()));
        }
    };
    // 純移動の周は渡した要約を run dir に残す（事後に読める・NFR4）。残せない周は判定に届かない。
    if let LensInput::Summary(summary) = &measured.input {
        if let Err(reason) = move_proof::keep(&run_dir(entry.state_dir, entry.run), summary) {
            return inconclusive(reason);
        }
    }
    // **裁定の写しは lens を起こす直前に書く**（`s2-07l.309`）。書けない周は lens を「裁定なし」で
    // 起こさない——回答で認めた逸脱が契約違反に読まれ、偽 FAIL / 偽 INCONCLUSIVE へ倒れる。
    keep_rulings(entry)?;
    let contract = contract_path(entry.state_dir, entry.run);
    // **lens の口座は起こす直前に選ぶ**（裁定の写しを書いた後・設計 account-autonomy.md §15）。
    let (line, account) = match lens_account(entry, substitute(cmd, &contract, worktree), notes) {
        Ok(found) => found,
        Err(reason) => return inconclusive(reason),
    };
    let (judged, scope) = ask_lens_rereading(entry.run, &line, worktree, body, notes);
    Ok(Decided { judged, scope, account })
}

/// 機械検証の段の判定順（[`decide`] の前半・**wildcard 無し・上から順に効く**・pure）。lens に届く周は `None`。
///
/// 順は 段①が読めない → 箱の中で死んだ → **遮断器が閉じた**（設計 gate-cost.md §32 約束 6）→ 赤 → 検出線の rc 2。
/// 遮断器の印が無い周の順と字面は §28 のまま動かない。
fn machine_order(measured: &Measured) -> Option<Judged> {
    // **測れなかったは赤より先**（C10・AC3）。段①の diff が読めない周は判定に届いていない
    // ので lens も呼ばず INCONCLUSIVE（測り直せる側・FR14）。
    if measured.unreadable {
        return Some(unjudged("diff の path を読めない（write-set を照合できない＝測れなかった）".to_owned()));
    }
    // **箱の中で殺された行も赤より先**（設計 gate-cost.md §4.2）。溢れた箱の中で死んだ行は
    // 内容が赤いのではなく測れていない——赤に化けさせると、host の memory が足りない周ほど
    // 便が FAIL（終端）で落ちる。
    if let Some(reason) = measured.killed {
        return Some(unjudged(format!(
            "verify の行が scope の中で死んだ（reason={}・測れなかった）",
            reason.as_str()
        )));
    }
    // **遮断器が閉じた周も赤より先**（設計 gate-cost.md §32 約束 6）。host が混んだまま待ちの上限を超えた周は
    // 行を撃っていない＝赤が在っても信用できない。FAIL で終端させず Gated に留め、負荷が引いた後に測り直す。
    if let Some(n) = measured.busy {
        return Some(unjudged(format!(
            "host が混んだまま待ちの上限を超えた（n={n} 以後の verify の行を撃っていない＝測れなかった）"
        )));
    }
    // **赤は検出線の rc 2 より先**（設計 gate-cost.md §28・`s2-07l.495`）。検出線は測る前に元の木の
    // 歯を全部走らせるので、歯が赤い木では必ず rc 2 で終わる——rc 2 を先に読むと、赤いと分かって
    // いる便が INCONCLUSIVE のまま居座る。赤の数え方は変えない（検出線 ∧ rc 2 だけ除く・rc 1 は赤）。
    if measured.red > 0 {
        // 赤い周は lens を呼ばない＝findings は測っていない（`tally` は `None`・C10）。
        let evidence = format!("verify の {} 行が rc≠0", measured.red);
        return Some(Judged { verdict: Verdict::Fail, evidence, tally: None, reread: false });
    }
    // **検出線の rc 2（測れなかった）は、赤が 0 の周だけ INCONCLUSIVE**（`s2-07l.331`・設計 §5.3 の③・
    // FR14）。道具が「測れていない」と言った周を FAIL にすると、便は終端して測り直せない。Gated に
    // 留め、検出線を撃ち直せる側へ倒す（PASS には決してならない・C10）。
    measured
        .detection_unmeasured
        .map(|n| unjudged(format!("検出線（n={n}）が測れなかった（rc 2・赤ではない）")))
}

/// lens を 1 回撃ち、**出力は在るが形が読めなかった**周（[`Judged::reread`]）だけ同じ行・同じ本文・
/// 同じ箱の形で **1 回だけ**撃ち直して 2 回目の戻りを採る（設計 gate-cost.md §29・`s2-07l.495`・
/// 検出線の撃ち直し §21 と同じ 1 回）。
///
/// 撃ち直すのは「形が読めない」側だけ——「読めたが規則で断った」（母集団 0）と箱の中の死・rc 非 0・
/// 起動の失敗は撃ち直さない（どれも撃ち直しで向きが変わらない・印は [`lens::parse_lens`] だけが立てる）。
/// 1 回目の理由は `notes` の 1 行（stderr）に残し**判定は変えない**（record の field も `verdict.json`
/// の schema も足さない・C10 = 撃ち直した事実を 0 に潰さない）。2 回目も読めなければ理由は 2 回目のもの。
fn ask_lens_rereading(
    run: &str,
    line: &str,
    worktree: &Path,
    body: &[u8],
    notes: &mut Vec<String>,
) -> (Judged, Option<Released>) {
    let first = ask_lens_attempt(run, line, worktree, body, 1);
    if !first.0.reread {
        return first;
    }
    notes.push(format!("pipe: lens-reread=1 reason={}", first.0.evidence));
    ask_lens_attempt(run, line, worktree, body, 2)
}

/// lens を `attempt` 番の箱（unit 名の試行の番号の欄・設計 §4.2）で 1 回撃つ。
fn ask_lens_attempt(
    run: &str,
    line: &str,
    worktree: &Path,
    body: &[u8],
    attempt: usize,
) -> (Judged, Option<Released>) {
    let unit = confine::unit_name(run, LENS_STAGE, attempt);
    let wrap = confine::Wrap {
        unit: &unit,
        // lens は `{jobs}` を持たない起動なので host の箱である（設計 gate-cost.md §4.2）。
        limit: confine::Limit::HostReserve,
        caps: confine::Caps::embedded(),
    };
    ask_lens(line, worktree, body, &wrap)
}

/// 器が選んだ口座を lens の起動行の末尾に足す（設計 account-autonomy.md §15 (1)(2)(4)・FR36）。
///
/// 宣言 0（[`Gate::pool`] が `None`）の周は行も記帳も変えない＝lens は親の環境を継承する（起動行不変）。
/// 選定は [`select_lens_account`]（計測 → 便用の規則）の 1 本で、**候補なしでも待たない**——`Err` は
/// lens を起こさず INCONCLUSIVE へ倒れる理由で、`resume` が撃ち直す（gate は段の判定で待ちを持たない）。
/// 起動行が既に `--account-dir` を持つ周も足さずに断る（runner と同じ [`super::spawn::with_account`]）。
fn lens_account(entry: &Gate<'_>, line: String, notes: &mut Vec<String>) -> Result<(String, Option<String>), String> {
    let Some(pool) = entry.pool else {
        return Ok((line, None));
    };
    let label = match select_lens_account(pool, entry.state_dir, entry.repo, notes) {
        Ok(LensAccount::Chosen(label)) => label,
        Ok(LensAccount::None(reason)) => {
            return Err(format!("lens の口座の候補が無い（account:none={reason}・待たずに測り直す）"));
        }
        Err(reason) => return Err(format!("lens の口座を選べない（{reason}）")),
    };
    let line = super::spawn::with_account(line, Some(&label), entry.state_dir)
        .map_err(|refusal| format!("lens の{refusal}"))?;
    Ok((line, Some(label)))
}

/// 判定に届かなかった腕の戻り（[`decide`] の INCONCLUSIVE・lens を撃たない周なので scope も口座も `None`）。
///
/// 腕ごとに [`Decided`] を組むと [`decide`] が C4 の線（`too_many_lines`）に当たる。**判定順は動かさない**
/// （腕の並びは [`decide`] が 1 か所で持つ・C2）。
fn inconclusive(reason: String) -> Result<Decided, String> {
    Ok(Decided { judged: unjudged(reason), scope: None, account: None })
}

/// 便の裁定（質問と planner の回答の対・発生順・[`super::questions_of_run`]）を run dir の
/// [`move_proof::RULINGS_FILE`] へ写す（設計 pipeline-question.md・C3「真実は event log」）。
///
/// 1 対 = `question:` / `about:` / `answer:` の 3 行（無い `about` は `-`）・対の間は空行。対が 0 の周は
/// 書かない（[`move_proof::keep`] と同じ・無いことが「裁定なし」・C10）。
fn keep_rulings(entry: &Gate<'_>) -> Result<(), String> {
    let questions = super::questions_of_run(entry.state_dir, entry.run);
    if questions.is_empty() {
        return Ok(());
    }
    let shown: Vec<String> = questions
        .iter()
        .map(|found| {
            let about = found.about.as_deref().unwrap_or("-");
            let answer = found.answer.as_deref().unwrap_or("-");
            format!("question: {}\nabout: {about}\nanswer: {answer}\n", found.question)
        })
        .collect();
    let path = run_dir(entry.state_dir, entry.run).join(move_proof::RULINGS_FILE);
    std::fs::write(&path, shown.join("\n")).map_err(|err| format!("{} を書けない: {err}", path.display()))
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
    if let Some(released) = decision.scope {
        fields.push(("scope", Value::Str(released.as_str().to_owned())));
    }
    // **findings と母集団は判定と同じ record に載る**（`s2-07l.188`・設計 §6 / §17）。読めた周だけ
    // 書く＝2 key を持たない lens は INCONCLUSIVE なので、field の無い verdict は「件数を測って
    // いない」と読める（0 件の verdict と弁別できる・C10）。
    if let Some(tally) = &decision.tally {
        fields.push(("findings", Value::Str(tally.findings_field())));
        fields.push(("population", Value::Str(tally.population_field())));
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
            detail: Some(gated_detail(decision)),
        },
        entry.policy,
    )
    .map_err(|err| err.to_string())
}

/// `Gated` の detail（`verdict:<V>`・器が lens の口座を選んだ周は `,account:<label>`）。
///
/// 語彙は `Spawned` の `account:<label>` と同じ 1 つで、**足すのは器が選んだ周だけ**（設計
/// account-autonomy.md §15 (3)）——宣言 0 の継承と「選べなかった」を接尾辞の不在で弁別できる（C10）。
fn gated_detail(decision: &Decision) -> String {
    let verdict = format!("verdict:{}", decision.verdict.as_str());
    match &decision.account {
        None => verdict,
        Some(label) => format!("{verdict},account:{label}"),
    }
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
    use super::{machine_order, Measured, Verdict};
    use crate::pipe::move_proof::{LensInput, NotPure};

    /// 機械検証の段の実測（赤の本数・遮断器の印・検出線の rc 2 だけを振る）。
    fn measured(red: u64, busy: Option<u64>, detection_unmeasured: Option<u64>) -> Measured {
        Measured {
            red,
            diff: Vec::new(),
            unreadable: false,
            killed: None,
            detection_unmeasured,
            busy,
            input: LensInput::Diff(NotPure::Unreadable),
        }
    }

    /// 遮断器の印が在る周は**赤が 1 行在っても** INCONCLUSIVE（印の行を名指す）で、印が無い周の順（赤 → 検出線の
    /// rc 2）と字面は 1 字も変わらない（設計 gate-cost.md §32 約束 6・2 つの枝を対で並べる）。
    #[test]
    fn gate_busy_order_mark_wins_over_red_and_leaves_the_unmarked_order() {
        let marked = machine_order(&measured(1, Some(2), Some(3))).expect("印の周は判定に届く");
        assert_eq!(marked.verdict, Verdict::Inconclusive, "赤が在っても FAIL で終端しない");
        assert!(marked.evidence.contains("n=2"), "印の行を名指す: {}", marked.evidence);
        let marked_green = machine_order(&measured(0, Some(2), None)).expect("印の周は判定に届く");
        assert_eq!(marked_green.verdict, Verdict::Inconclusive, "赤が無くても印の周は INCONCLUSIVE");
        // 印が無い周: 赤は検出線の rc 2 より先に FAIL（§28 の字面のまま）。
        let red = machine_order(&measured(1, None, Some(3))).expect("赤の周は判定に届く");
        assert_eq!(red.verdict, Verdict::Fail, "印が無ければ赤が FAIL");
        assert_eq!(red.evidence, "verify の 1 行が rc≠0", "字面は不変");
        let unmeasured = machine_order(&measured(0, None, Some(3))).expect("検出線の rc 2 の周は判定に届く");
        assert_eq!(unmeasured.verdict, Verdict::Inconclusive);
        assert_eq!(unmeasured.evidence, "検出線（n=3）が測れなかった（rc 2・赤ではない）", "字面は不変");
        assert!(machine_order(&measured(0, None, None)).is_none(), "何も無い周は lens へ進む");
    }
}
