//! 着地後の検出の口（`pipe land --run <id> --detection-only`・設計 docs/design/gate-cost.md §44 行 ak・ADR-0060・
//! `s2-07l.465`）。
//!
//! `Landed` の便の着地した commit（記録の `sha:`）を主実測と同じ置き場の別名へ detach で出し、凍結した宣言の写しの
//! 検出線の行**だけ**を gate と同じ受付札・縮退・箱・遮断器の行の手（[`run_detection_admitted`]）で 1 回撃つ。`{base}` は
//! 着地した commit の親・`{teeth}` は契約の写しから・的と純移動の付け足しと写しの周の番号は gate と同じ 1 本
//! （[`aimed_lines`] / [`population_lines`] / [`keep_detection`] と周の番号）を置き場と便 id の対で呼ぶ（C2）。
//!
//! 残すのは 3 つ: 便自身の `verify-main.jsonl` に `landed` 付きの record（撃った周は行ごとに 1 本・撃たなかった周と
//! 撃てなかった周は理由を持つ 1 本＝どの周も 1 本以上）、§15 の置き場の写しと理由の file、`RunDone stage=Landed` の
//! detail の `detection:<語>` 1 件。**台帳 client は呼ばない**（C15・FR50）。gate と land の段は変えない。口は land が
//! `Landed` の後に子 process で起こし（[`super::finish`]・待たない・設計 §44 形 (11)）、人も同じ口を撃てる。
//!
//! 極性: 検出線は止めない線（C12.4）なので、測れなかった周も rc 0 で語が `unmeasured` になる。rc 2 は record・写し・
//! event を書けない周だけ。宣言に検出線の行が無い便は何も書かずに rc 1 で断る。

use super::super::contract::Contract;
use super::super::declaration::Effective;
use super::super::gate::{
    aimed_lines, keep_detection, keep_reason, landed_step_record, landed_unfired_record, next_copy_dir, next_number,
    population_lines, recorded_rc, run_detection_admitted, Admit, Checks, LandedMark, Limits, Record, Step, Unfired,
};
use super::super::{emit, git_bytes, git_line, git_ok, verify_log_path, vessel_path, Emit};
use super::verify::{check_path, MAIN_UNKNOWN, VERIFY_MAIN_FILE, VERIFY_MAIN_STDERR_FILE};
use super::{broken, detection_needed, nul_paths, refused};
use crate::cli_outcome::Outcome;
use crate::fleet::store::{append_line, LockPolicy};
use crate::fleet::{EventKind, Stage};
use std::path::{Path, PathBuf};

/// 着地後の検出 1 回の材料（`pipe land --detection-only` の手が組む・着地の材料 [`super::Land`] の部分集合）。
pub(in crate::pipe) struct Detect<'a> {
    /// 便 id。
    pub(in crate::pipe) run: &'a str,
    /// 契約の bead id（event に載る）。
    pub(in crate::pipe) bead: &'a str,
    /// 対象 repo（着地した commit を出す）。
    pub(in crate::pipe) repo: &'a Path,
    /// 置き場。
    pub(in crate::pipe) state_dir: &'a Path,
    /// 契約の写し（`{teeth}` の出所）。
    pub(in crate::pipe) contract: &'a Contract,
    /// 規則から読んだ線（受付札と遮断器の材料・gate と同じ 1 本）。
    pub(in crate::pipe) limits: Limits,
    /// lock の待ち方。
    pub(in crate::pipe) policy: LockPolicy,
}

/// 置き場の別名の接尾辞（主実測の tmp `verify/<id>` と重ねない・`verify/<id>-detection`）。
const PLACE_SUFFIX: &str = "-detection";

/// `RunDone stage=Landed` の detail の前置き（`sha:` も `terminal:` も持たない＝着地と終端の読み手は読み飛ばす）。
pub(super) const DETAIL_HEAD: &str = "detection:";

/// land が口を子 process で起こせた周の detail の語（設計 §44 形 (11)・終えた語ではない＝子が後から終えた語を記す）。
pub(super) const SPAWNED: &str = "spawned";

/// worktree か親を出せなかった周の理由の語。
const UNPREPARED: &str = "unprepared";

/// 遮断器が閉じて撃たなかった周の理由の語。
const HOST_CLOSED: &str = "host-closed";

/// land が口を子 process で起こせなかった周の理由の語（detail の語も同じ字面・設計 §44 形 (11)）。
pub(super) const UNSPAWNED: &str = "unspawned";

/// 口が終えた語（stdout の `detection=` と detail の `detection:` の値・閉じた 3 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Finished {
    /// 撃って測れた（rc 0 か 1）。
    Measured,
    /// 測れなかった（rc が 0 と 1 以外・遮断器が閉じた・worktree か親を出せない）。
    Unmeasured,
    /// 撃たなかった（面の外）。
    Skipped,
}

impl Finished {
    /// 字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Measured => "measured",
            Self::Unmeasured => "unmeasured",
            Self::Skipped => "skipped",
        }
    }
}

/// 着地した commit（`sha`）に検出線を 1 回撃ち、record・写し・event を残す（stdout は `run=<id> detection=<語>`）。
pub(in crate::pipe) fn detect(entry: &Detect<'_>, sha: &str) -> Outcome {
    let path = vessel_path(entry.state_dir, entry.run);
    let frozen = match Effective::load(&path) {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            return broken(format!("{} を読めない: {}", path.display(), lines.join(" / ")));
        }
    };
    let lines = frozen.detection_verify();
    if lines.is_empty() {
        return refused(format!("run {} の宣言の写しに検出線の行が無い", entry.run));
    }
    let finished = match measure(entry, lines, sha) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunDone,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Landed),
            seat: None,
            pid: None,
            detail: Some(format!("{DETAIL_HEAD}{}", finished.as_str())),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => Outcome::ok_line(format!("run={} detection={}", entry.run, finished.as_str())),
    }
}

/// 親と木を読み、面を照らし、worktree を出して撃つ（`Err` は record か写しを書けない周＝rc 2）。
///
/// 撃った後は worktree を畳む（`git worktree remove --force`・ref は動かさない＝列の次の便の CAS と交差しない）。
fn measure(entry: &Detect<'_>, lines: &[String], sha: &str) -> Result<Finished, String> {
    let parent = git_line(entry.repo, &["rev-parse", &format!("{sha}^")]);
    let tree = parent.as_ref().and_then(|_| git_line(entry.repo, &["rev-parse", &format!("{sha}^{{tree}}")]));
    let mark = LandedMark { sha, tree: tree.as_deref().unwrap_or(MAIN_UNKNOWN) };
    let Some(parent) = parent else {
        return unfired(entry, mark, Unfired::Unmeasured(UNPREPARED));
    };
    if !touches_scope(entry.repo, &parent, sha) {
        return unfired(entry, mark, Unfired::OutsideScope);
    }
    let tmp = check_path(entry.repo, &format!("{}{PLACE_SUFFIX}", entry.run));
    if !prepare(entry.repo, &tmp, sha) {
        return unfired(entry, mark, Unfired::Unmeasured(UNPREPARED));
    }
    let fired = fire_lines(entry, lines, &tmp, &parent);
    // 出力の写しは worktree を畳む前に取る（出力は tmp の `target/` に在る）。全行が閉じた周は写しを作らない。
    let kept = match &fired {
        Ok((steps, _)) if !all_closed(steps) => keep_detection(entry.state_dir, entry.run, &tmp, steps),
        _ => Ok(None),
    };
    let _ = git_ok(entry.repo, &["worktree", "remove", "--force", &tmp.display().to_string()]);
    let (steps, pure_move) = fired?;
    if all_closed(&steps) {
        return unfired(entry, mark, Unfired::Unmeasured(HOST_CLOSED));
    }
    let dir = kept?.unwrap_or_else(|| next_copy_dir(entry.state_dir, entry.run));
    record_fired(entry, mark, &steps, pure_move)?;
    match unmeasured_of(&steps) {
        None => Ok(Finished::Measured),
        Some(reason) => {
            keep_reason(&dir, &Unfired::Unmeasured(&reason).word())?;
            Ok(Finished::Unmeasured)
        }
    }
}

/// 着地した commit と親の path が検出線の面に触れるか（**読めない周は触れる側**＝撃つ・fail-closed・設計 §44 形 (3)）。
fn touches_scope(repo: &Path, parent: &str, sha: &str) -> bool {
    let Some(bytes) = git_bytes(repo, &["diff-tree", "-r", "--name-only", "-z", parent, sha]) else {
        return true;
    };
    detection_needed(nul_paths(&bytes).iter().map(String::as_str))
}

/// 着地した commit を置き場の別名へ detach で出す（出せた周だけ真）。
fn prepare(repo: &Path, tmp: &Path, sha: &str) -> bool {
    let Some(parent) = tmp.parent() else {
        return false;
    };
    std::fs::create_dir_all(parent).is_ok() && git_ok(repo, &["worktree", "add", "--detach", &tmp.display().to_string(), sha])
}

/// 写しの検出線に gate と同じ付け足し（的・純移動の母集団）をして、検出線の行だけを受付の中で撃つ。
fn fire_lines(entry: &Detect<'_>, lines: &[String], tmp: &Path, parent: &str) -> Result<(Vec<Step>, Option<usize>), String> {
    let aimed = aimed_lines(entry.state_dir, entry.run, lines)?;
    let (lines, pure_move) = population_lines(entry.state_dir, entry.run, tmp, parent, aimed)?;
    let admit = Admit { state_dir: entry.state_dir, run: entry.run, rules: entry.limits.admission(entry.policy) };
    let checks = Checks {
        worktree: tmp,
        base: parent,
        contract: entry.contract,
        common: &[],
        detection: &lines,
        host: entry.limits.breaker(),
    };
    Ok((run_detection_admitted(&checks, Some(&admit)), pure_move))
}

/// 全行が遮断器で閉じた（1 行も撃っていない）周か。閉じた印は以後の行へ伝わるので先頭の行で決まる。
fn all_closed(steps: &[Step]) -> bool {
    steps.first().is_some_and(Step::is_closed)
}

/// 撃った周の測れなかった理由（在れば）: 途中で閉じた行が在れば `host-closed`、無ければ rc が 0 と 1 以外の最初の行。
fn unmeasured_of(steps: &[Step]) -> Option<String> {
    if steps.iter().any(Step::is_closed) {
        return Some(HOST_CLOSED.to_owned());
    }
    steps
        .iter()
        .find(|step| !matches!(step.rc, 0 | 1))
        .map(|step| format!("rc-{}", recorded_rc(step.rc)))
}

/// 撃たなかった / 撃てなかった周: 理由を持つ record 1 本と、次の周の置き場に理由の file 1 つ。
///
/// land が口を起こせなかった周（`unmeasured=unspawned`）も [`super::finish`] がこの 1 本で書く（書き手を増やさない・C2）。
pub(super) fn unfired(entry: &Detect<'_>, mark: LandedMark<'_>, why: Unfired<'_>) -> Result<Finished, String> {
    let path = record_path(entry);
    let n = next_n(&path)?;
    append_line(&path, &landed_unfired_record(n, mark, why), entry.policy).map_err(|err| err.to_string())?;
    keep_reason(&next_copy_dir(entry.state_dir, entry.run), &why.word())?;
    Ok(match why {
        Unfired::OutsideScope => Finished::Skipped,
        Unfired::Unmeasured(_) => Finished::Unmeasured,
    })
}

/// 撃った行ごとの record を追記し、赤い行の stderr の写しを主実測と同じ診断 file へ残す（gate と同じ書き口）。
fn record_fired(entry: &Detect<'_>, mark: LandedMark<'_>, steps: &[Step], pure_move: Option<usize>) -> Result<(), String> {
    let path = record_path(entry);
    let diagnosis = path.with_file_name(VERIFY_MAIN_STDERR_FILE);
    let first = next_n(&path)?;
    for (offset, step) in steps.iter().enumerate() {
        let n = first.saturating_add(u64::try_from(offset).unwrap_or(u64::MAX));
        let record = Record { n, body: landed_step_record(n, step, mark, pure_move), step: Some(step) };
        record.diagnose(&diagnosis, entry.policy)?;
        append_line(&path, &record.body, entry.policy).map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// 便自身の `verify-main.jsonl`（主実測と同じ file）。
fn record_path(entry: &Detect<'_>) -> PathBuf {
    verify_log_path(entry.state_dir, entry.run).with_file_name(VERIFY_MAIN_FILE)
}

/// 次の record の `n`（既存の非空の行の次・無い file は 1・在るのに読めない周は `Err`）。
fn next_n(path: &Path) -> Result<u64, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(next_number(text.lines().filter(|line| !line.trim().is_empty()).count())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(next_number(0)),
        Err(err) => Err(format!("{} を読めない: {err}", path.display())),
    }
}
