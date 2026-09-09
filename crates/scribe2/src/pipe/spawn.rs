//! runner を起動する**唯一の関数**（設計 §5.2・FR4 / FR6・憲法 C6）。
//!
//! [`spawn`] は [`Budget`] を引数に取り、`Budget` は [`super::Precheck::measure`] を
//! 消費してしか作れない。したがって「測らずに起動する」経路は型として存在しない。
//!
//! **scribe2 固有の env は 1 つも足さない**（C2.2・ADR-0004 §2.4）。runner へは
//! 親の env をそのまま継承させ、必要な値は cmd の placeholder 置換で渡す。

use super::{branch_name, contract_path, emit, git_line, worktree_path, Budget, Emit};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::LockPolicy;
use crate::fleet::{EventKind, Stage};
use crate::name::NAME;
use crate::pipe::contract::Contract;
use std::path::{Path, PathBuf};
use std::process::Command;

/// policy file の名前（guard が読む形・vessel-hook.md §5）。
const WRITE_SET_FILE: &str = "write-set.txt";

/// 起動 1 回の材料。
pub struct Launch<'a> {
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
    /// runner のコマンド（placeholder を含む）。
    pub runner: &'a str,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// runner を起動して結果まで見届ける。**これが唯一の起動口である**。
pub fn spawn(budget: Budget, launch: &Launch<'_>) -> Outcome {
    let _ = budget.write_set();
    let Some(base) = super::head_of(launch.repo) else {
        return refused(format!("{} の HEAD を読めない", launch.repo.display()));
    };
    let worktree = worktree_path(launch.repo, launch.run);
    if let Err(reason) = add_worktree(launch.repo, &worktree, launch.run, &base) {
        return refused(reason);
    }
    let write_set = match write_policy(&worktree, &launch.contract.write_set) {
        Ok(path) => path,
        Err(reason) => return broken(reason),
    };
    if let Err(err) = emit(
        launch.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: launch.run,
            bead: launch.bead,
            stage: Some(Stage::Spawned),
            seat: None,
            pid: None,
            detail: Some(format!("base:{base}")),
        },
        launch.policy,
    ) {
        return broken(err.to_string());
    }
    launch_runner(launch, &worktree, &write_set, &base)
}

/// runner を起こし、終わりまで見届けて段を決める。
fn launch_runner(launch: &Launch<'_>, worktree: &Path, write_set: &Path, base: &str) -> Outcome {
    let cmd = substitute(launch, worktree, write_set, base);
    // **env を 1 つも足さない**: `.env()` / `.envs()` を呼ばず親の env をそのまま継承する。
    let child = Command::new("sh").arg("-c").arg(&cmd).current_dir(worktree).spawn();
    let mut child = match child {
        Ok(found) => found,
        Err(err) => return broken(format!("runner を起動できない: {err}")),
    };
    let pid = u64::from(child.id());
    if let Err(err) = seat(launch, EventKind::SeatSpawned, Some(pid), None) {
        return broken(err);
    }
    // rc が要るのでここは `Child::wait`。pid の生存だけを見る待機（`pipe stop`）は
    // `fleet::wait` のままで、**待機の実装は増えていない**（C3.4）。
    let status = match child.wait() {
        Ok(found) => found,
        Err(err) => return broken(format!("runner の終了を待てない: {err}")),
    };
    if let Err(err) = seat(launch, EventKind::SeatStopped, Some(pid), None) {
        return broken(err);
    }
    settle(launch, worktree, base, status.code().unwrap_or(-1))
}

/// commit の有無まで見て段を決める。**commit 0 は完了ではない**。
fn settle(launch: &Launch<'_>, worktree: &Path, base: &str, rc: i32) -> Outcome {
    let range = format!("{base}..HEAD");
    let commits: u64 = git_line(worktree, &["rev-list", "--count", &range])
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let (stage, detail) = if rc == 0 && commits >= 1 {
        (Stage::Implemented, None)
    } else {
        (
            Stage::Failed,
            Some(format!("runner-rc:{rc},commits:{commits}")),
        )
    };
    if let Err(err) = emit(
        launch.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: launch.run,
            bead: launch.bead,
            stage: Some(stage),
            seat: None,
            pid: None,
            detail,
        },
        launch.policy,
    ) {
        return broken(err.to_string());
    }
    Outcome::ok_line(format!("run={} stage={}", launch.run, stage.as_str()))
}

/// 席の event を 1 件書く。
fn seat(
    launch: &Launch<'_>,
    kind: EventKind,
    pid: Option<u64>,
    detail: Option<String>,
) -> Result<(), String> {
    emit(
        launch.state_dir,
        &Emit {
            kind,
            run: launch.run,
            bead: launch.bead,
            stage: None,
            seat: Some(launch.run.to_owned()),
            pid,
            detail,
        },
        launch.policy,
    )
    .map_err(|err| err.to_string())
}

/// cmd の placeholder を実値へ置く。
fn substitute(launch: &Launch<'_>, worktree: &Path, write_set: &Path, base: &str) -> String {
    launch
        .runner
        .replace("{run}", launch.run)
        .replace("{worktree}", &worktree.display().to_string())
        .replace(
            "{contract}",
            &contract_path(launch.state_dir, launch.run).display().to_string(),
        )
        .replace("{write_set}", &write_set.display().to_string())
        .replace("{base}", base)
}

/// worktree を切る。既に在れば断る。
fn add_worktree(repo: &Path, worktree: &Path, run: &str, base: &str) -> Result<(), String> {
    if worktree.exists() {
        return Err(format!("{} は既に在る", worktree.display()));
    }
    let branch = branch_name(run);
    let path = worktree.display().to_string();
    let added = git_line(repo, &["worktree", "add", "-b", &branch, &path, base]);
    if added.is_none() && !worktree.exists() {
        return Err(format!("worktree を切れない（branch {branch}）"));
    }
    Ok(())
}

/// write-set を worktree の git dir の私有 dir へ書く（tracked 面に触れない）。
fn write_policy(worktree: &Path, write_set: &[String]) -> Result<PathBuf, String> {
    let git_dir = git_line(worktree, &["rev-parse", "--absolute-git-dir"])
        .ok_or_else(|| format!("{} の git dir を読めない", worktree.display()))?;
    let dir = PathBuf::from(git_dir).join(NAME);
    std::fs::create_dir_all(&dir).map_err(|err| format!("{} を作れない: {err}", dir.display()))?;
    let path = dir.join(WRITE_SET_FILE);
    let body = format!("{}\n", write_set.join("\n"));
    std::fs::write(&path, body).map_err(|err| format!("{} を書けない: {err}", path.display()))?;
    Ok(path)
}

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行・何もしない）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}
