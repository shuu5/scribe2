//! `pipe` subcommand の面（設計 §5）。
//!
//! **env も HOME も読まない**（憲法 C2.2）。置き場は `--state-dir` か、repo に紐づいた
//! git 設定（`vessel init` が書いたもの）から解く。規則の値は `--rules` か埋め込みの
//! manifest から読み、数値をこの file に焼かない（C1 / C5）。
//!
//! 前提違反は **rc 1 + stderr 1 行で何もしない**（event も追記しない・設計 §4）。
//! 契約 file が読めない周は「対象そのものが壊れている」ので rc 2 で、理由を全件出す。

use super::contract::Contract;
use super::spawn::{spawn, Launch};
use super::{contract_path, current, emit, run_dir, run_id, worktree_path, Emit, Precheck};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::{LockPolicy, StoreError};
use crate::fleet::{self, Completion, EventKind, SeatState, Stage, State};
use crate::hook::vessel;
use crate::name::NAME;
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 停止の猶予を持つ rules 行。
const ROW_GRACE: &str = "pipe.stop_grace_ms";

/// `pipe` の使い方。
pub fn usage() -> String {
    format!("usage: {NAME} pipe <intake|spawn|show|resume|stop> [--state-dir D] [--rules PATH] [flags]")
}

/// `pipe` に続く引数を捌く。
pub fn dispatch(args: &[String]) -> Outcome {
    let manifest = match manifest_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let policy = match LockPolicy::from_rules(&manifest) {
        Ok(found) => found,
        Err(err) => return broken(err.to_string()),
    };
    match args.first().map(String::as_str) {
        Some("intake") => intake(args, policy),
        Some("spawn") => start(args, policy),
        Some("show") => show(args),
        Some("resume") => resume(args, policy),
        Some("stop") => stop(args, &manifest, policy),
        _ => Outcome::failed(RC_REFUSED, vec![usage()]),
    }
}

/// `--<name> <値>` を読む。値欠けは黙って落とさず `Err`（SRS NFR4）。
///
/// 器の中で 3 本目の flag reader である。4 本目が要るときは 1 本へ畳む
/// （いまは fleet / vessel / pipe がそれぞれ自分の必須 flag だけを見ている）。
fn flag<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, String> {
    let Some(at) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    match args.get(at + 1) {
        Some(found) if !found.starts_with("--") => Ok(Some(found)),
        _ => Err(format!("{name} に値が無い")),
    }
}

/// 必須の flag。
fn need<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    flag(args, name)?.ok_or(format!("{name} が要る"))
}

/// 規則の値。`--rules` が在ればその file、無ければ埋め込み。
fn manifest_of(args: &[String]) -> Result<Manifest, String> {
    let path = flag(args, "--rules")?;
    let loaded = match path {
        Some(found) => Manifest::load(Path::new(found)),
        None => Manifest::embedded(),
    };
    loaded.map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
            .join(" / ")
    })
}

/// rules 行の整数値。
fn int_row(manifest: &Manifest, id: &str) -> Result<u64, String> {
    let row = manifest.get(id).ok_or(format!("{id} が無い"))?;
    if !row.enabled {
        return Err(format!("{id} は不発効である"));
    }
    match row.value {
        RuleValue::Int(found) => Ok(found),
        _ => Err(format!("{id} が整数でない")),
    }
}

/// 置き場。`--state-dir` が上書きし、無ければ repo に紐づいた git 設定から読む。
///
/// repo の解き方は [`repo_of`] ただ 1 本（`--repo` → cwd の root）。ここで cwd だけを
/// 見ると、`--repo` を渡した周に**別の repo の置き場**を読んでしまう。
fn state_dir_of(args: &[String]) -> Result<PathBuf, String> {
    if let Some(found) = flag(args, "--state-dir")? {
        return Ok(PathBuf::from(found));
    }
    let root = repo_of(args)?;
    vessel::state_dir(&root)
        .ok_or_else(|| format!("{} に置き場が紐づいていない（vessel init）", root.display()))
}

/// 契約 file を読み込み、置き場へ写して run を起こす。
fn intake(args: &[String], policy: LockPolicy) -> Outcome {
    let parsed = (|| {
        Ok::<_, String>((
            PathBuf::from(need(args, "--contract")?),
            need(args, "--bead")?.to_owned(),
            PathBuf::from(need(args, "--repo")?),
        ))
    })();
    let (path, bead, repo) = match parsed {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    // repo は spawn まで使わないが、**intake の時点で** git repo かを確かめる。
    // 後段で初めて落ちると、契約は受理されたのに進めない run が残る。
    if super::head_of(&repo).is_none() {
        return refused(format!("{} は git repo でない", repo.display()));
    }
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let contract = match Contract::load(&path) {
        Ok(found) => found,
        Err(errors) => {
            return Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect())
        }
    };
    let id = run_id(&bead, &fleet::cli::now_utc());
    // stamp は秒までなので、同じ bead を同じ秒に 2 回 intake すると id が衝突する。
    // 黙って上書きすると **前の便の契約が別物に化ける**ので、何も書かずに断る。
    if run_dir(&state_dir, &id).exists() {
        return refused(format!("run {id} は既に在る（同じ秒の再 intake）"));
    }
    if let Err(reason) = copy_contract(&state_dir, &id, &path) {
        return broken(reason);
    }
    if let Err(reason) = remember_repo(&state_dir, &id, &repo) {
        return broken(reason);
    }
    let emitted = emit(
        &state_dir,
        &Emit {
            kind: EventKind::RunCreated,
            run: &id,
            bead: &bead,
            stage: Some(Stage::Intake),
            seat: None,
            pid: None,
            detail: Some(format!("classes:{}", contract.classes.join("+"))),
        },
        policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => Outcome::ok_line(format!("run={id}")),
    }
}

/// 便の対象 repo を写し面へ書き留める（現在地を cwd に依らせない）。
fn remember_repo(state_dir: &Path, id: &str, repo: &Path) -> Result<(), String> {
    let path = super::repo_path(state_dir, id);
    std::fs::write(&path, format!("{}\n", repo.display()))
        .map_err(|err| format!("{} を書けない: {err}", path.display()))
}

/// 便の repo。`--repo` が上書きし、無ければ写し面 → cwd の順で解く。
fn run_repo(args: &[String], state_dir: &Path, id: &str) -> Result<PathBuf, String> {
    if let Some(found) = flag(args, "--repo")? {
        return Ok(PathBuf::from(found));
    }
    match super::repo_of_run(state_dir, id) {
        Some(found) => Ok(found),
        None => repo_of(args),
    }
}

/// 契約 file を置き場へ写す（process 間で持ち越す面は event log とこの写しだけ）。
fn copy_contract(state_dir: &Path, id: &str, from: &Path) -> Result<(), String> {
    let dir = run_dir(state_dir, id);
    std::fs::create_dir_all(&dir).map_err(|err| format!("{} を作れない: {err}", dir.display()))?;
    let to = contract_path(state_dir, id);
    std::fs::copy(from, &to).map_err(|err| format!("{} を写せない: {err}", to.display()))?;
    Ok(())
}

/// 前提の段を replay から読む。無ければ `Err`。
fn stage_of(state: &State, id: &str) -> Result<Stage, String> {
    state
        .runs
        .get(id)
        .map(|run| run.stage)
        .ok_or(format!("run {id} が無い"))
}

/// `pipe spawn`。前提 stage = Intake。
fn start(args: &[String], policy: LockPolicy) -> Outcome {
    let parsed = (|| Ok::<_, String>((need(args, "--run")?.to_owned(), need(args, "--runner")?.to_owned())))();
    let (id, runner) = match parsed {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    launch(args, &id, &runner, policy, &[Stage::Intake])
}

/// 段を確かめてから起動口を通す。
fn launch(
    args: &[String],
    id: &str,
    runner: &str,
    policy: LockPolicy,
    allowed: &[Stage],
) -> Outcome {
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
    };
    let stage = match stage_of(&state, id) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    if !allowed.contains(&stage) {
        return refused(format!("run {id} の段は {} である", stage.as_str()));
    }
    let Some(run) = state.runs.get(id) else {
        return refused(format!("run {id} が無い"));
    };
    let contract = match Contract::load(&contract_path(&state_dir, id)) {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect()),
    };
    let repo = match run_repo(args, &state_dir, id) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let budget = match Precheck::measure(&contract, &repo) {
        Ok(found) => found.into_budget(),
        Err(reason) => return refused(reason),
    };
    spawn(
        budget,
        &Launch {
            run: id,
            bead: &run.bead,
            repo: &repo,
            state_dir: &state_dir,
            contract: &contract,
            runner,
            policy,
        },
    )
}

/// 対象 repo。`--repo` が無ければ cwd の repo root。
fn repo_of(args: &[String]) -> Result<PathBuf, String> {
    if let Some(found) = flag(args, "--repo")? {
        return Ok(PathBuf::from(found));
    }
    let cwd = std::env::current_dir().map_err(|err| format!("cwd を解決できない: {err}"))?;
    vessel::repo_root(&cwd).ok_or("repo の root を解決できない".to_owned())
}

/// `pipe show`。
fn show(args: &[String]) -> Outcome {
    let id = match need(args, "--run") {
        Ok(found) => found.to_owned(),
        Err(reason) => return refused(reason),
    };
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
    };
    let Some(run) = state.runs.get(&id) else {
        return refused(format!("run {id} が無い"));
    };
    let repo = run_repo(args, &state_dir, &id).unwrap_or_else(|_| PathBuf::from("."));
    Outcome::ok_line(format!(
        "run={} bead={} stage={} approved={} worktree={}",
        run.id,
        run.bead,
        run.stage.as_str(),
        run.approved,
        worktree_path(&repo, &id).display()
    ))
}

/// `pipe resume`。現在の段から続きの段だけを通す。
fn resume(args: &[String], policy: LockPolicy) -> Outcome {
    let id = match need(args, "--run") {
        Ok(found) => found.to_owned(),
        Err(reason) => return refused(reason),
    };
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
    };
    match stage_of(&state, &id) {
        Err(reason) => refused(reason),
        // gate は (b) の手番。ここでは次の段を名乗って rc 0 で終える。
        Ok(Stage::Implemented) => Outcome::ok_line(format!("run={id} next=gate")),
        Ok(Stage::Intake) => match need(args, "--runner") {
            Err(reason) => refused(reason),
            Ok(runner) => launch(args, &id, runner, policy, &[Stage::Intake]),
        },
        Ok(stage) => refused(format!("run {id} の段 {} からは再開しない", stage.as_str())),
    }
}

/// `pipe stop --all`。生きている席を止める。**冪等**（対象なしは rc 0）。
fn stop(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    if !args.iter().any(|arg| arg == "--all") {
        return refused("--all が要る".to_owned());
    }
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        // state が読めない周だけ rc 2（stop の rc 語彙 3 値）。
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
        Ok(found) => found,
    };
    let grace = match int_row(manifest, ROW_GRACE) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    // **pid を持たない Live 席も母集団に数える**。落とすと「対象なし rc 0」に化け、
    // 止まっていない席が在るのに全クリアを名乗ってしまう。
    let live: Vec<(String, String, Option<u64>)> = state
        .seats
        .values()
        .filter(|seat| seat.state == SeatState::Live)
        .map(|seat| (seat.id.clone(), seat.run.clone(), seat.pid))
        .collect();
    let mut stopped = 0_usize;
    for (id, run, pid) in &live {
        // 止められなかった席に「止めた」を記帳しない。記帳すると次の周が
        // 「対象なし」を返し、生きている席が終端として消える（偽の全クリア）。
        if !pid.is_some_and(|found| terminate(found, grace)) {
            continue;
        }
        stopped += 1;
        if let Err(err) = record_stop(&state_dir, &state, (id, run, *pid), policy) {
            return broken(err);
        }
    }
    let line = format!("stop: seats={} stopped={stopped}", live.len());
    if stopped == live.len() {
        Outcome::ok_line(line)
    } else {
        Outcome {
            out: vec![line],
            err: vec![format!("pipe: 止められない席が {} 残った", live.len() - stopped)],
            rc: RC_REFUSED,
        }
    }
}

/// TERM → 猶予だけ待つ → 残れば KILL。**待機は fleet の 1 実装を通る**（C3.4）。
fn terminate(pid: u64, grace_ms: u64) -> bool {
    let target = u32::try_from(pid).unwrap_or(u32::MAX);
    signal(pid, "-TERM");
    if fleet::wait(Completion::SeatGone(target), Duration::from_millis(grace_ms)).is_ok() {
        return true;
    }
    signal(pid, "-KILL");
    fleet::wait(Completion::SeatGone(target), Duration::from_millis(grace_ms)).is_ok()
}

/// pid へ signal を送る（std に kill は無いので `kill` を撃つ）。
fn signal(pid: u64, name: &str) {
    let _ = std::process::Command::new("kill")
        .arg(name)
        .arg(pid.to_string())
        .output();
}

/// 席と便に「止めた」を記帳する。
fn record_stop(
    state_dir: &Path,
    state: &State,
    seat: (&str, &str, Option<u64>),
    policy: LockPolicy,
) -> Result<(), String> {
    let (id, run, pid) = seat;
    let bead = state.runs.get(run).map_or("", |found| found.bead.as_str());
    emit(
        state_dir,
        &Emit {
            kind: EventKind::SeatStopped,
            run,
            bead,
            stage: None,
            seat: Some(id.to_owned()),
            pid,
            detail: None,
        },
        policy,
    )
    .map_err(|err| err.to_string())?;
    emit(
        state_dir,
        &Emit {
            kind: EventKind::RunStopped,
            run,
            bead,
            stage: Some(Stage::Stopped),
            seat: None,
            pid: None,
            detail: None,
        },
        policy,
    )
    .map_err(|err| err.to_string())
}

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行・何もしない）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}
