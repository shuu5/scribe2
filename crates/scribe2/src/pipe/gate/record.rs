//! gate の記録と診断（`verify.jsonl` の record [`step_record`]・赤い行の stderr の診断 file・
//! 便の写しの読み・[`super`] から純移動・`s2-07l.286`）。判定の順と終端は親（[`super::gate`]）が持つ。

use super::verify::{is_unreadable, recorded_rc, run_checks_admitted, Admit, Check, Checks, Step};
use super::Gate;
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{append_line, LockPolicy};
use crate::fleet::SCHEMA;
use crate::pipe::admission;
use crate::pipe::confine::Reason;
use crate::pipe::declaration::Effective;
use crate::pipe::{verify_log_path, vessel_path};
use std::path::Path;

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
pub(super) const STDERR_TAIL_LINES: usize = 20;

/// 包みが stdout の終端に出す行の見出し（[`crate::pipe::confine`] の `script` が printf する固定形）。
///
/// record の `line=` は**この行を剥がした残り**の末尾 1 行である（設計 gate-cost.md §5.1）——包みの
/// 測定行を「道具の判定行」として書くと、`peak_mb` と同じ数が別の名で 2 度残る。字面は
/// [`crate::pipe::confine::read_usage`] が読む見出しと同じで、in-file の歯が両者の一致を測る（ずれると
/// 剥がせない＝終端行が `line` に化ける）。
pub(super) const USAGE_HEAD: &str = "confine-usage";

/// verify 各行を撃ち、行ごとの rc を `verify.jsonl` へ逐条で残す。
///
/// **赤い行だけ** stderr の末尾を診断 file（[`STDERR_LOG_FILE`]）へも append する。
/// rc だけでは「何がどう赤いか」が便の外から読めず、gate が落ちるたびに人が同じ行を
/// 手で撃ち直して理由を取り直すことになる（実測 2026-09-10・`s2-07l.49`）。緑の行は
/// 残さない——読む理由が無い出力で診断 file を埋めると、赤い行の見出しが埋もれる。
pub(super) fn record_verify(entry: &Gate<'_>, worktree: &Path, base: &str) -> Result<Counted, String> {
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
///
/// `line=` は **kind と rc を問わず**、stdout の末尾 1 行が在った step 全部に書く（設計 §5.1・
/// planner 裁定 2026-09-14）。検出線の行だけに絞ると flip-check の `base-retried=N`（rc 0 の周の
/// stdout にしか出ない）が残らず、行の種類で分岐する形にもなる（C2）。
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
    if let Some(released) = step.scope {
        fields.push(("scope", Value::Str(released.as_str().to_owned())));
    }
    if let Some(line) = &step.line {
        fields.push(("line", Value::Str(line.clone())));
    }
    json_lite::write_object(&fields)
}

/// `verify.jsonl` の数え上げ（赤の本数と、2 つの「測れなかった」）。
pub(super) struct Counted {
    /// rc≠0 だった verify 行の本数（**測れた行**だけを数える）。
    pub(super) red: u64,
    /// 段①（write-set 照合）で diff の path を読めなかったか。
    pub(super) unreadable: bool,
    /// 箱の中で殺された行の理由（在れば）。
    pub(super) killed: Option<Reason>,
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
///
/// **検出線の行の `oom_kill` は数えない**（`s2-07l.228`・設計 §4.2）。変異ごとの test process を
/// 箱の中で起こす道具は、無限 loop の変異 1 つが kernel に殺されてもその死を吸収して完走する
/// ——測れた周であり、判定は rc と outcomes が持つ。record の `reason=oom-kill` は残る
/// （[`step_record`] は `step.reason` をそのまま書く）。包みごとの signal 死は終端行が無い＝
/// 測れていないので、検出線でも従来どおり数える。
fn box_kill(step: &Step) -> Option<Reason> {
    if !step.confined {
        return None;
    }
    let absorbs_oom = step.stage == Check::Detection;
    step.reason.filter(|found| {
        matches!(*found, Reason::Signal) || (matches!(*found, Reason::OomKill) && !absorbs_oom)
    })
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
