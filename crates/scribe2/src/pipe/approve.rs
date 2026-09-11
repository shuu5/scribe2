//! 承認（設計 docs/design/pipeline.md §5.5・FR15 / FR16 / AC5・憲法 A1 / C7.2）。
//!
//! **承認は event log に残った逐語だけである**（C7.2）。会話の記憶も、席の思い込みも
//! 根拠にしない——`pipe approve --words "<user の逐語>"` が `ApprovalReceived`
//! （`actor=human`）を 1 件書いた事実だけが「人が許した」の証拠になる。
//!
//! 止める位置は **spawn の手前**（A1「実行前」）。runner を起こしてから聞くのでは、
//! 消す / 出す / 使うが**もう起きた後**になる。

use crate::polarity::{OnFailure, Polarity, Timing};
use super::contract::Contract;
use super::{emit, Emit};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::LockPolicy;
use crate::fleet::{EventKind, Stage};
use std::path::Path;

/// 人の手番で止まっている周の rc（設計 §5.5）。
///
/// [`super::gate::RC_INCONCLUSIVE`] と**同じ数**である。段を 1 つだけ通す
/// `pipe spawn` / `pipe gate` なら撃った段が語彙を決めるので取り違えようがないが、
/// **`pipe run` と `pipe resume` は 1 つの subcommand でどちらも返しうる**——
/// rc だけでは「人待ち」と「判定できなかった」を弁別できない。弁別は永続面の段
/// （`Blocked` か `Gated`）と stderr の 1 行で行う。
pub const RC_BLOCKED: u8 = 3;

/// 承認 1 回の材料。
pub struct Approve<'a> {
    /// 便 id。
    pub run: &'a str,
    /// 契約の bead id。
    pub bead: &'a str,
    /// 置き場。
    pub state_dir: &'a Path,
    /// user の逐語。
    pub words: &'a str,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// 承認を 1 件記帳する。**逐語が空なら 1 byte も書かない**。
pub fn approve(entry: &Approve<'_>) -> Outcome {
    // 空の承認を通すと「聞いた形」だけが残って中身が無い記録になる。
    if entry.words.trim().is_empty() {
        return Outcome::failed_line(RC_REFUSED, "pipe: 承認の逐語が空である".to_owned());
    }
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::ApprovalReceived,
            run: entry.run,
            bead: entry.bead,
            stage: None,
            seat: None,
            pid: None,
            // **要約せずそのまま**書く（C7.2）。言い換えた時点で承認ではなくなる。
            detail: Some(entry.words.to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => Outcome::failed_line(RC_BROKEN, err.to_string()),
        Ok(()) => Outcome::ok_line(format!("run={} approved=true", entry.run)),
    }
}

/// この境界の極性（A1 の承認関門・[`needs_approval`] → [`block`]）: spawn の手前＝起動の時点で止め、承認は event log の逐語だけを根拠にする（読めない・無い周は起動しない）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 3 クラスを名乗る契約が未承認のまま実行されようとしているか（A1「実行前」）。
pub fn needs_approval(contract: &Contract, approved: bool) -> bool {
    !contract.classes.is_empty() && !approved
}

/// 人の手番で止める。`ApprovalRequested` + `Blocked` を記帳して rc 3 で終える。
///
/// **人の入力を待たない**（FR15）。待つと席が塞がり、承認が来るまで他の便も進まない。
pub fn block(entry: &Approve<'_>, classes: &[String]) -> Outcome {
    let detail = classes.join("+");
    let requested = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::ApprovalRequested,
            run: entry.run,
            bead: entry.bead,
            stage: None,
            seat: None,
            pid: None,
            detail: Some(detail.clone()),
        },
        entry.policy,
    );
    if let Err(err) = requested {
        return Outcome::failed_line(RC_BROKEN, err.to_string());
    }
    let blocked = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Blocked),
            seat: None,
            pid: None,
            detail: Some(detail.clone()),
        },
        entry.policy,
    );
    match blocked {
        Err(err) => Outcome::failed_line(RC_BROKEN, err.to_string()),
        Ok(()) => Outcome {
            out: vec![format!("run={} stage={}", entry.run, Stage::Blocked.as_str())],
            err: vec![format!(
                "pipe: 承認が要る（{detail}）・`pipe approve --run {} --words \"<user の逐語>\"`",
                entry.run
            )],
            rc: RC_BLOCKED,
        },
    }
}
