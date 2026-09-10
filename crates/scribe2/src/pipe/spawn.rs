//! runner を起動する**唯一の関数**（設計 §5.2・FR4 / FR6・憲法 C6）。
//!
//! [`spawn`] は [`Budget`] を引数に取り、`Budget` は [`super::Precheck::measure`] を
//! 消費してしか作れない。したがって「測らずに起動する」経路は型として存在しない。
//!
//! **scribe2 固有の env は 1 つも足さない**（C2.2・ADR-0004 §2.4）。runner へは
//! 親の env をそのまま継承させ、必要な値は cmd の placeholder 置換で渡す。

use super::approve::{block, needs_approval, Approve};
use super::{
    branch_name, contract_path, emit, git_line, plugin_path, vessel_path, worktree_path, Budget, Emit,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::LockPolicy;
use crate::fleet::{EventKind, Stage};
use crate::name::NAME;
use crate::pipe::contract::Contract;
use std::path::{Path, PathBuf};
use std::process::Command;

/// policy file の名前（guard が読む形・vessel-hook.md §5）。
const WRITE_SET_FILE: &str = "write-set.txt";

/// runner へ載せる plugin の中身（repo 相対・設計 §6）。
const PLUGIN_DIRS: [&str; 2] = [".claude-plugin", "hooks"];

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
    /// 承認 event が在るか（replay の導出値）。
    pub approved: bool,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// runner を起動して結果まで見届ける。**これが唯一の起動口である**。
pub fn spawn(budget: Budget, launch: &Launch<'_>) -> Outcome {
    let _ = budget.write_set();
    // **A1「実行前」の関門はここに置く**（設計 §5.5）。起動口が 1 本なので、この 1 行が
    // spawn / resume / run のすべての経路を覆う。呼び手側に置くと経路が増えるたびに
    // 素通りの穴が空く。
    if needs_approval(launch.contract, launch.approved) {
        return block(&approval(launch), &launch.contract.classes);
    }
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
    let plugin = match copy_plugin(&worktree, launch.state_dir, launch.run) {
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
    launch_runner(launch, &worktree, &write_set, &plugin, &base)
}

/// runner を起こし、終わりまで見届けて段を決める。
fn launch_runner(
    launch: &Launch<'_>,
    worktree: &Path,
    write_set: &Path,
    plugin: &Path,
    base: &str,
) -> Outcome {
    let cmd = substitute(launch, worktree, write_set, plugin, base);
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
fn substitute(
    launch: &Launch<'_>,
    worktree: &Path,
    write_set: &Path,
    plugin: &Path,
    base: &str,
) -> String {
    launch
        .runner
        .replace("{run}", launch.run)
        .replace("{worktree}", &worktree.display().to_string())
        .replace(
            "{contract}",
            &contract_path(launch.state_dir, launch.run).display().to_string(),
        )
        .replace(
            "{vessel}",
            &vessel_path(launch.state_dir, launch.run).display().to_string(),
        )
        .replace("{write_set}", &write_set.display().to_string())
        .replace("{base}", base)
        .replace("{plugin_dir}", &plugin.display().to_string())
}

/// repo の plugin を run dir 配下へ写し、その path を返す（設計 §5.2 / §6）。
///
/// Claude Code は**読み込んだ plugin dir の配下**を acceptEdits の自動承認から外す
/// （sensitive）。便の worktree は `<repo>/.worktrees/<NAME>/<run>` ＝ repo を
/// `--plugin-dir` に渡すと **その内側**なので、便の全 file で Edit / Write が deny される。
/// 写しを repo の外（run dir 配下）へ置くことで、「repo の plugin を載せる」意図を保った
/// まま worktree を保護対象から外す。
///
/// 写すのは **worktree の** [`PLUGIN_DIRS`]（＝便の base の内容）であって anchor の
/// 現在値ではない。plugin を持たない repo では**空 dir を作るだけ**で進む
/// （`{plugin_dir}` を使わない便を止めない）。
fn copy_plugin(worktree: &Path, state_dir: &Path, run: &str) -> Result<PathBuf, String> {
    let dest = plugin_path(state_dir, run);
    // 再走で古い写しが残らないよう、先に空にする。
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|err| format!("{} を空にできない: {err}", dest.display()))?;
    }
    std::fs::create_dir_all(&dest).map_err(|err| format!("{} を作れない: {err}", dest.display()))?;
    for name in PLUGIN_DIRS {
        let from = worktree.join(name);
        // **`Path::is_dir` では判定しない**。あれは link を辿るので、`hooks` が dir への
        // symlink（例 `hooks -> ../..`）の周に「dir だ」と読んで link 先の木を丸ごと写す
        // ＝「symlink は追わない」が top-level だけ抜ける。最終要素を辿らない
        // `symlink_metadata` で見て、link なら**写さない**（fail-closed）。
        if std::fs::symlink_metadata(&from).is_ok_and(|meta| meta.is_dir()) {
            copy_tree(&from, &dest.join(name))?;
        }
    }
    Ok(dest)
}

/// dir を再帰 copy する。**file だけを写し、symlink は追わない**。
///
/// symlink を写すと、便の外を指す link 1 本で plugin dir の見かけが repo の外に
/// なったまま中身が repo を指す（保護を外した意味が消える）。
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|err| format!("{} を作れない: {err}", to.display()))?;
    let entries =
        std::fs::read_dir(from).map_err(|err| format!("{} を読めない: {err}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("{} を読めない: {err}", from.display()))?;
        // `DirEntry::file_type` は link を辿らない（`symlink_metadata` 相当）。
        let kind = entry
            .file_type()
            .map_err(|err| format!("{} の種別を読めない: {err}", entry.path().display()))?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)
                .map_err(|err| format!("{} を写せない: {err}", entry.path().display()))?;
        }
    }
    Ok(())
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

/// 承認まわりの材料を組む。
fn approval<'a>(launch: &'a Launch<'a>) -> Approve<'a> {
    Approve {
        run: launch.run,
        bead: launch.bead,
        state_dir: launch.state_dir,
        words: "",
        policy: launch.policy,
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
