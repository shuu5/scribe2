//! gate（設計 docs/design/pipeline.md §5.3・FR8 / FR9 / NFR1）。
//!
//! 契約の `verify` 各行の逐条 rc（機械検証）と lens 1 本の判定を合わせて 3 値を出す。
//! **判定は wildcard 無しの順序で決める**: 測れなかった（段①が読めない・箱の中の死・検出線の rc 2）
//! → INCONCLUSIVE ／ verify に rc≠0 → FAIL ／ lens に渡す本文の byte が cap 超
//! → INCONCLUSIVE（lens を呼ばない）／ lens 側の不備 → INCONCLUSIVE ／ それ以外は
//! lens の verdict。lens に渡す本文は閉じた型 [`LensInput`]（diff か、純移動の要約・
//! [`super::move_proof`]・`s2-07l.266`）で、判定は純関数・file の読みだけをここが担う。
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
//!
//! 本 file は判定の入口と終端（[`gate`] → `precheck` → `measure` → `decide` → `settle`）と型・定数を
//! 持つ。verify 行の実行は [`verify`]、lens の呼び出しと parse は [`lens`]、記録と診断は [`record`]
//! （`s2-07l.286` の純移動・外から呼ぶ path は本 file の再輸出で不変）。

mod lens;
mod record;
mod verify;

pub(crate) use lens::last_json_object;
pub use record::step_record;
pub use verify::{is_unreadable, run_checks, Check, Checks, Step, CHECKS};

use crate::polarity::{OnFailure, Polarity, Timing};
use super::confine::{self, Reason, Released};
use super::contract::Contract;
use super::lens_record::LensSource;
use super::move_proof::{self, LensInput, NotPure};
use super::{
    contract_path, emit, git_bytes, git_line, run_dir, verdict_path, worktree_path, Emit,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::LockPolicy;
use crate::fleet::{cli::now_utc, EventKind, Stage, SCHEMA};
use lens::{ask_lens, lens_input, substitute, write_verdict, LENS_STAGE};
use record::record_verify;
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
    /// lens のコマンドの出所（`--lens` か run dir の写し・無い / 読めないは別の値・[`super::lens_record`]・§26）。
    pub lens: &'a LensSource,
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
    /// INCONCLUSIVE へ倒し、record の `reason=` で外からの kill と弁別する（検出線の行の
    /// `oom_kill` は除く＝道具が吸収して完走した周・`record::box_kill`）。
    killed: Option<Reason>,
    /// 検出線が rc 2（測れなかった）で終えた行の `n`（在れば・`s2-07l.331`・設計 §5.3）。
    ///
    /// `cargo xtask mutants-diff` の rc 2 は「生存も時間切れも無いが測れていない」（baseline が
    /// 落ちた等）で、赤（rc 1 = deny 昇格後）ではない。赤に数えると測れなかった便が FAIL で終端し
    /// 測り直せない（C10）。rc 1 の検出線と検出線以外の rc 2 は従来どおり赤（`record::detection_unmeasured`）。
    detection_unmeasured: Option<u64>,
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
    /// lens の scope を片付けた結果（record に書く周だけ `Some`＝field `scope`・設計 gate-cost.md §4.4 errata）。
    scope: Option<Released>,
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
    let (verdict, evidence, scope) = match decide(entry, &worktree, &measured) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let decision = Decision {
        verdict,
        evidence,
        red: measured.red,
        diff_bytes: byte_count(&measured.diff),
        tree,
        scope,
    };
    match settle(entry, &decision) {
        Err(reason) => broken(reason),
        Ok(()) => Outcome {
            out: vec![format!(
                "run={} verdict={} lens-input={} bytes={}",
                entry.run,
                verdict.as_str(),
                measured.input.kind(),
                byte_count(measured.input.body(&measured.diff))
            )],
            err: measured.input.notice().into_iter().collect(),
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
    let detection_unmeasured = counted.detection_unmeasured;
    if unreadable {
        // 段①が diff を読めない周は同じ range の生 diff も読めない。ここで broken（rc 2・
        // verdict を書かない）にすると便は Implemented のまま「測り直せる便」に見えない。
        let input = LensInput::Diff(NotPure::Unreadable);
        return Ok(Measured { red, diff: Vec::new(), unreadable, killed, detection_unmeasured, input });
    }
    let range = format!("{base}..HEAD");
    let diff = git_bytes(worktree, &["diff", &range])
        .ok_or_else(|| format!("{} の diff を測れない", worktree.display()))?;
    let input = lens_input(worktree, base, &diff);
    Ok(Measured { red, diff, unreadable, killed, detection_unmeasured, input })
}

/// 判定順を 1 か所に閉じる（**wildcard 無し・上から順に効く**）。
///
/// 3 つ目は lens の scope を片付けた結果（record に書く周だけ `Some`・lens を撃たない周は `None`）。
/// `Err` は裁定の写しを書けなかった周（判定に届かず gate を止める＝rc 2・verdict を書かない）。
fn decide(entry: &Gate<'_>, worktree: &Path, measured: &Measured) -> Result<(Verdict, String, Option<Released>), String> {
    // **測れなかったは赤より先**（C10・AC3）。段①の diff が読めない周は判定に届いていない
    // ので lens も呼ばず INCONCLUSIVE（測り直せる側・FR14）。
    if measured.unreadable {
        return inconclusive("diff の path を読めない（write-set を照合できない＝測れなかった）".to_owned());
    }
    // **箱の中で殺された行も赤より先**（設計 gate-cost.md §4.2）。溢れた箱の中で死んだ行は
    // 内容が赤いのではなく測れていない——赤に化けさせると、host の memory が足りない周ほど
    // 便が FAIL（終端）で落ちる。
    if let Some(reason) = measured.killed {
        return inconclusive(format!(
            "verify の行が scope の中で死んだ（reason={}・測れなかった）",
            reason.as_str()
        ));
    }
    // **検出線の rc 2（測れなかった）も赤より先**（`s2-07l.331`・設計 §5.3 の③・FR14）。道具が
    // 「測れていない」と言った周を FAIL にすると、便は終端して測り直せない。Gated に留め、
    // 検出線を撃ち直せる側へ倒す（PASS には決してならない・C10）。
    if let Some(n) = measured.detection_unmeasured {
        return inconclusive(format!("検出線（n={n}）が測れなかった（rc 2・赤ではない）"));
    }
    if measured.red > 0 {
        return Ok((Verdict::Fail, format!("verify の {} 行が rc≠0", measured.red), None));
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
    let unit = confine::unit_name(entry.run, LENS_STAGE, 1);
    let wrap = confine::Wrap {
        unit: &unit,
        // lens は `{jobs}` を持たない起動なので host の箱である（設計 gate-cost.md §4.2）。
        limit: confine::Limit::HostReserve,
        caps: confine::Caps::embedded(),
    };
    Ok(ask_lens(&substitute(cmd, &contract, worktree), worktree, body, &wrap))
}

/// 判定に届かなかった腕の戻り（[`decide`] の INCONCLUSIVE・lens を撃たない周なので scope は `None`）。
///
/// 腕ごとに 3 つ組を書くと [`decide`] が C4 の線（`too_many_lines`）に当たる。**判定順は動かさない**
/// （腕の並びは [`decide`] が 1 か所で持つ・C2）。
fn inconclusive(reason: String) -> Result<(Verdict, String, Option<Released>), String> {
    Ok((Verdict::Inconclusive, reason, None))
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
