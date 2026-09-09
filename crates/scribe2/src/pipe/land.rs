//! land（設計 docs/design/pipeline.md §5.4・FR10 / FR11 / FR12・憲法 N1）。
//!
//! **PASS が無ければ 1 byte も動かさない**。前提は Gated ∧ `verdict.json` が PASS ∧
//! `refs/heads/main` が便の記録した base と同一（CAS の old）で、どれか 1 つでも欠けたら
//! rc 1 で何もしない。
//!
//! main を進めた後の実測が赤でも **auto revert はしない**（MVP 外）。main は進んだまま
//! `Failed detail=main-red` を残して loud に落ちる——黙って巻き戻すと「何が起きたか」が
//! 履歴から消え、赤い main が緑に見える瞬間が生まれるためである。
//!
//! 後始末は **可逆な move**（N1.2）。worktree は `retired/<run>` へ移すだけで削除せず、
//! branch も消さない（squash commit は branch の祖先でないので `-d` は通らず、`-D` は
//! N1 が禁じる形である）。

use super::contract::Contract;
use super::gate::{run_line, Verdict};
use super::{
    emit, git_line, git_ok, verdict_path, worktree_path, worktrees_dir, Emit,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{self, append_line, LockPolicy};
use crate::fleet::{cli::now_utc, EventKind, Stage, SCHEMA};
use std::path::{Path, PathBuf};

/// 進める ref。設計 §5.4 が名指す 1 本である。
const MAIN_REF: &str = "refs/heads/main";

/// 面 5 の export 先の file 名（ADR-0004 §2.2・**版番号に依らず固定**）。
const VERDICTS_FILE: &str = "verdicts.jsonl";

/// land 済み worktree を寄せる dir 名。
const RETIRED_DIR: &str = "retired";

/// main 実測用の tmp worktree を置く dir 名。
///
/// **`std::env::temp_dir` を使わない**（`TMPDIR` を読む＝憲法 C2.2 に反する）。置き場は
/// 便の worktree と同じ repo 配下から導く。run id は `<bead>-<stamp>` なのでこの名と
/// 衝突しない。
const CHECK_DIR: &str = "verify";

/// main 実測の結果。**「赤かった」と「測れなかった」を混ぜない**。
///
/// gate が「測れなかったを通ったに化けさせない」と決めているのと同じ理由で、land も
/// 「測れなかった」を「赤かった」に化けさせない。実測を 1 行も撃てていないのに
/// `main-red` を記帳すると、event log が事実と違うものを述べる。
enum MainCheck {
    /// verify 全行が rc 0。
    Green,
    /// 1 行以上が rc≠0（**実測した上での赤**）。
    Red(String),
    /// 実測そのものができなかった（tmp worktree を切れない等）。
    Unmeasurable(String),
}

/// land 1 回の材料。
pub struct Land<'a> {
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
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// 面 5 の export 先。
///
/// dir 名を 2 度書かず event log の隣として導く（`store` が dir を変えたら追随する）。
pub fn verdicts_path(state_dir: &Path) -> PathBuf {
    store::events_path(state_dir).with_file_name(VERDICTS_FILE)
}

/// land 後に worktree を寄せる先。
pub fn retired_path(repo: &Path, id: &str) -> PathBuf {
    worktrees_dir(repo).join(RETIRED_DIR).join(id)
}

/// main 実測用の tmp worktree。
fn check_path(repo: &Path, id: &str) -> PathBuf {
    worktrees_dir(repo).join(CHECK_DIR).join(id)
}

/// land を 1 回通す。
pub fn land(entry: &Land<'_>) -> Outcome {
    let worktree = worktree_path(entry.repo, entry.run);
    let Some(base) = super::base_of_run(entry.state_dir, entry.run) else {
        return refused(format!("run {} に base が無い", entry.run));
    };
    if verdict_of(entry.state_dir, entry.run) != Some(Verdict::Pass) {
        return refused(format!("run {} の verdict が PASS でない", entry.run));
    }
    let Some(old) = git_line(entry.repo, &["rev-parse", MAIN_REF]) else {
        return refused(format!("{MAIN_REF} を読めない"));
    };
    if old != base {
        // **CAS の old が動いている**。ここで進めると別便の land を巻き込む。
        return refused(format!("stale base（base={base} main={old}）"));
    }
    let new = match squash(entry, &worktree, &old) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    match verify_main(entry, &new) {
        MainCheck::Green => finish(entry, &worktree, &new),
        MainCheck::Red(reason) => main_red(entry, &reason),
        MainCheck::Unmeasurable(reason) => main_unmeasured(entry, &reason),
    }
}

/// worktree の tree を 1 commit にして main を CAS で進める。**tree の同一を実測する**。
fn squash(entry: &Land<'_>, worktree: &Path, old: &str) -> Result<String, String> {
    let tree = git_line(worktree, &["rev-parse", "HEAD^{tree}"])
        .ok_or_else(|| format!("{} の tree を読めない", worktree.display()))?;
    let message = format!("{}: {}", entry.bead, entry.contract.goal);
    let new = git_line(entry.repo, &["commit-tree", &tree, "-p", old, "-m", &message])
        .ok_or_else(|| "squash commit を作れない".to_owned())?;
    if !git_ok(entry.repo, &["update-ref", MAIN_REF, &new, old]) {
        return Err(format!("{MAIN_REF} を付け替えられない（CAS が外れた）"));
    }
    let landed = git_line(entry.repo, &["rev-parse", &format!("{new}^{{tree}}")])
        .ok_or_else(|| "land した tree を読めない".to_owned())?;
    if landed != tree {
        return Err(format!("tree が同一でない（{tree} → {landed}）"));
    }
    Ok(new)
}

/// 進めた main を別の worktree で実測する。
fn verify_main(entry: &Land<'_>, new: &str) -> MainCheck {
    let tmp = check_path(entry.repo, entry.run);
    if let Some(parent) = tmp.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            return MainCheck::Unmeasurable(format!("{} を作れない: {err}", parent.display()));
        }
    }
    let path = tmp.display().to_string();
    if !git_ok(entry.repo, &["worktree", "add", "--detach", &path, new]) {
        // **ここで赤を名乗らない**: verify 行を 1 本も撃てていない。
        return MainCheck::Unmeasurable(format!("{} を切れない", tmp.display()));
    }
    let red = entry
        .contract
        .verify
        .iter()
        .filter(|line| run_line(&tmp, line) != 0)
        .count();
    // 成果は `new` に載っているので、この tmp だけは remove してよい（設計 §5.4）。
    // `--force` は verify が tmp に生んだ中間物ごと畳むためで、履歴・データは触らない。
    let _ = git_ok(entry.repo, &["worktree", "remove", "--force", &path]);
    if red > 0 {
        return MainCheck::Red(format!("main で verify の {red} 行が rc≠0"));
    }
    MainCheck::Green
}

/// export → `Landed` → 後始末。ここまで来た周は land が成立している。
fn finish(entry: &Land<'_>, worktree: &Path, new: &str) -> Outcome {
    if let Err(reason) = export_verdict(entry, new) {
        return broken(reason);
    }
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunDone,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Landed),
            seat: None,
            pid: None,
            detail: Some(format!("sha:{new}")),
        },
        entry.policy,
    );
    if let Err(err) = emitted {
        return broken(err.to_string());
    }
    Outcome {
        out: vec![format!("run={} landed={new}", entry.run)],
        // 後始末の失敗は land を取り消さない（**rc 0 のまま stderr 1 行**）。
        err: retire(entry, worktree),
        rc: crate::cli_outcome::RC_OK,
    }
}

/// 面 5 の 1 行を `verdicts.jsonl` へ append する（跨版 契約・key 列は固定）。
fn export_verdict(entry: &Land<'_>, new: &str) -> Result<(), String> {
    let evidence = verdict_path(entry.state_dir, entry.run).display().to_string();
    let line = json_lite::write_object(&[
        ("schema", Value::Num(SCHEMA)),
        ("run", Value::Str(entry.run.to_owned())),
        ("bead", Value::Str(entry.bead.to_owned())),
        ("sha", Value::Str(new.to_owned())),
        ("verdict", Value::Str(Verdict::Pass.as_str().to_owned())),
        ("evidence", Value::Str(evidence)),
        ("ts", Value::Str(now_utc())),
    ]);
    append_line(&verdicts_path(entry.state_dir), &line, entry.policy)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

/// worktree を `retired/<run>` へ move する。**削除しない・branch も消さない**（N1.2）。
fn retire(entry: &Land<'_>, worktree: &Path) -> Vec<String> {
    let dest = retired_path(entry.repo, entry.run);
    let Some(parent) = dest.parent() else {
        return vec!["pipe: retired の親 dir を解けない".to_owned()];
    };
    if let Err(err) = std::fs::create_dir_all(parent) {
        return vec![format!("pipe: {} を作れない: {err}", parent.display())];
    }
    let from = worktree.display().to_string();
    let to = dest.display().to_string();
    if git_ok(entry.repo, &["worktree", "move", &from, &to]) {
        return Vec::new();
    }
    vec![format!("pipe: {from} を {to} へ移せなかった")]
}

/// `verdict.json` から 3 値を読む。読めない周は `None`（＝PASS ではない）。
///
/// **判定の読み手はこの 1 本だけである**。land の前提（PASS か）だけでなく、gate の
/// 測り直し（Gated ∧ INCONCLUSIVE か）と resume の行き先（land か gate か）も同じ値を
/// 見る。読み手を増やすと、同じ JSON の解釈が場所ごとに静かにずれる。
pub fn verdict_of(state_dir: &Path, id: &str) -> Option<Verdict> {
    let text = std::fs::read_to_string(verdict_path(state_dir, id)).ok()?;
    let pairs = json_lite::parse_object(text.trim()).ok()?;
    pairs
        .iter()
        .find(|(key, _)| key == "verdict")
        .and_then(|(_, value)| value.as_str())
        .and_then(Verdict::parse)
}

/// main の実測が赤だった周。**auto revert しない**（main は進んだまま）。
fn main_red(entry: &Land<'_>, reason: &str) -> Outcome {
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Failed),
            seat: None,
            pid: None,
            detail: Some("main-red".to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => refused(format!("main が赤い（{reason}）・revert しない")),
    }
}

/// main を実測できなかった周。**赤とは別の名で残す**（rc 2 = 対象が壊れている）。
fn main_unmeasured(entry: &Land<'_>, reason: &str) -> Outcome {
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Failed),
            seat: None,
            pid: None,
            detail: Some("main-unmeasured".to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => broken(format!("main を実測できない（{reason}）・revert しない")),
    }
}

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行・何もしない）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}
