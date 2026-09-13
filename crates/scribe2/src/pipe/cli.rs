//! `pipe` subcommand の面（設計 §5）。
//!
//! **env も HOME も読まない**（憲法 C2.2）。置き場は `--state-dir` か、repo に紐づいた
//! git 設定（`vessel init` が書いたもの）から解く。規則の値は `--rules` か埋め込みの
//! manifest から読み、数値をこの file に焼かない（C1 / C5）。
//!
//! 前提違反は **rc 1 + stderr 1 行で何もしない**（event も追記しない・設計 §4）。
//! 契約 file が読めない周は「対象そのものが壊れている」ので rc 2 で、理由を全件出す。

use super::approve::{Approve, RC_BLOCKED};
use super::contract::Contract;
use super::declaration::{self, Ceiling, Effective, CEILING_ROW};
use super::follow::{self, Turn};
use super::gate::{Gate, Limits, Verdict, RC_INCONCLUSIVE};
use super::land::{verdict_of, Land, Retire, REBASE_EMPTY};
use super::refuse::{overlaps, Refuse};
use super::{
    contract_path, current, emit, last_stage_detail, question_of_run, run_dir, run_id,
    runner_is_idle, vessel_path, worktree_path, Emit,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
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

/// 追随が衝突した便を起こし直す回数の上限を持つ rules 行。
const ROW_RETRIES: &str = "pipe.follow_retries";

/// land が着地待ちの列で自分の番を待つ上限（秒）を持つ rules 行（設計 gate-cost.md §6）。
const ROW_LAND_WAIT: &str = "pipe.land_wait_s";

/// `pipe` の使い方。
pub fn usage() -> String {
    format!(
        "usage: {NAME} pipe <intake|spawn|approve|answer|gate|land|retire|run|show|resume|stop|report> [--state-dir D] [--rules PATH] [stop: --all|--run ID] [flags]"
    )
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
        Some("intake") => intake(args, &manifest, policy),
        Some("spawn") => start(args, policy),
        Some("approve") => by_run(args, |id| approve_run(args, id, policy)),
        Some("answer") => by_run(args, |id| answer_run(args, id, policy)),
        Some("gate") => by_run(args, |id| gate_run(args, id, &manifest, policy)),
        Some("land") => by_run(args, |id| land_run(args, id, &manifest, policy)),
        Some("retire") => by_run(args, |id| retire_run(args, id, policy)),
        Some("run") => run_all(args, &manifest, policy),
        Some("show") => show(args),
        Some("resume") => resume(args, &manifest, policy),
        Some("stop") => stop(args, &manifest, policy),
        Some("report") => match state_dir_of(args) {
            Err(reason) => refused(reason),
            Ok(state_dir) => super::report::report(&state_dir),
        },
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

/// rules 行の文字列の列。
fn list_row(manifest: &Manifest, id: &str) -> Result<Vec<String>, String> {
    let row = manifest.get(id).ok_or(format!("{id} が無い"))?;
    if !row.enabled {
        return Err(format!("{id} は不発効である"));
    }
    match row.value {
        RuleValue::List(ref found) => Ok(found.clone()),
        _ => Err(format!("{id} が文字列の列でない")),
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
fn intake(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    match intake_id(args, manifest, policy) {
        Ok(id) => Outcome::ok_line(intake_line(args, &id)),
        Err(outcome) => outcome,
    }
}

/// intake の 1 行。`--rules` で上限を差し替えて通した周は**その事実を同じ行に残す**
/// （`ceiling-overridden=<path>`・値は渡した path の字面そのもの・`s2-07l.65`）。
///
/// `--rules` は test の seam で、上限（`runner.allowed_commands`）を無条件に差し替える。
/// 差し替えた周が通常の周と同じ 1 行しか出さないと、review は「埋め込みの上限で通った便」と
/// 区別できない（`.56` lens M1）。差し替えていない周は出さない＝不在が既定。
fn intake_line(args: &[String], id: &str) -> String {
    match flag(args, "--rules") {
        Ok(Some(path)) => format!("run={id} ceiling-overridden={path}"),
        _ => format!("run={id}"),
    }
}

/// intake の本体。**id を返す**のは `run` が続きの段へ渡すためである
/// （自分の stdout を読み直して id を取る形にすると、表示を変えた瞬間に連鎖が壊れる）。
fn intake_id(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Result<String, Outcome> {
    let parsed = (|| {
        Ok::<_, String>((
            PathBuf::from(need(args, "--contract")?),
            need(args, "--bead")?.to_owned(),
            PathBuf::from(need(args, "--repo")?),
        ))
    })();
    let (path, bead, repo) = parsed.map_err(refused)?;
    // repo は spawn まで使わないが、**intake の時点で** git repo かを確かめる。
    // 後段で初めて落ちると、契約は受理されたのに進めない run が残る。
    if super::head_of(&repo).is_none() {
        return Err(refuse(&Refuse::NotARepo { repo: repo.display().to_string() }, &[]));
    }
    let state_dir = state_dir_of(args).map_err(refused)?;
    let contract = Contract::load(&path).map_err(|errors| {
        Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect())
    })?;
    // **宣言は上限と突き合わせてから**。ここで断つ周は run dir も event も作らない
    // ——撃てない契約の run が置き場に残ると、続きから引ける便に見えてしまう。
    let effective = freeze(&repo, manifest, &contract)?;
    // **入口で排他する**（ADR-0019 §2.1）。live な便と write-set が交差する契約は、
    // run dir も event も作らずに断る——後段（land の rebase）で衝突を知るより安い。
    exclude_overlap(&state_dir, &contract)?;
    let id = run_id(&bead, &fleet::cli::now_utc());
    // stamp は秒までなので、同じ bead を同じ秒に 2 回 intake すると id が衝突する。
    // 黙って上書きすると **前の便の契約が別物に化ける**ので、何も書かずに断る。
    if run_dir(&state_dir, &id).exists() {
        return Err(refuse(&Refuse::DuplicateRun { run: id.clone() }, &[]));
    }
    copy_contract(&state_dir, &id, &path).map_err(broken)?;
    copy_vessel(&state_dir, &id, &effective).map_err(broken)?;
    remember_repo(&state_dir, &id, &repo).map_err(broken)?;
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
        Err(err) => Err(broken(err.to_string())),
        Ok(()) => Ok(id),
    }
}

/// live な便（終端でない run）と write-set が交差する契約を断る（設計 pipeline-conflict.md §2）。
///
/// **読めない側が勝つ**: live な便の写しを 1 つでも読めなければ、交差の有無に関わらず
/// `WriteSetUnreadable`（rc 2）で止まる。読めない store を「交差なし」に読み替えると、
/// 排他が黙って無効化される（fail-closed・NFR4）。
///
/// 交差した周は**全組を stderr へ並べ**、理由の 1 行は先頭の 1 組を名乗る。
fn exclude_overlap(state_dir: &Path, contract: &Contract) -> Result<(), Outcome> {
    let state = current(state_dir).map_err(|errors| {
        Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect())
    })?;
    let mut first: Option<Refuse> = None;
    let mut lines: Vec<String> = Vec::new();
    for (id, run) in &state.runs {
        let Some(alive) = live(state_dir, id, run.stage) else {
            return Err(refuse(&Refuse::WriteSetUnreadable { run: id.clone() }, &[]));
        };
        if !alive {
            continue;
        }
        let Ok(live_contract) = Contract::load(&contract_path(state_dir, id)) else {
            return Err(refuse(&Refuse::WriteSetUnreadable { run: id.clone() }, &[]));
        };
        for (mine, theirs) in overlaps(&contract.write_set, &live_contract.write_set) {
            if first.is_none() {
                first = Some(Refuse::WriteSetOverlap { run: id.clone(), path: mine.clone() });
            }
            lines.push(format!("pipe: overlap run={id} contract={mine} live={theirs}"));
        }
    }
    match first {
        None => Ok(()),
        Some(found) => Err(refuse(&found, &lines)),
    }
}

/// 便が live（終端でない）か。**段の網羅 match で書く**（段が増えたら compile で気付く）。
///
/// 終端 = `Landed` / `Failed` / `Stopped`、または `Gated` で verdict が FAIL（pipeline.md §4
/// 「FAIL は終端」）。`Gated` の判定を読めない周は `None`＝**測れなかった**で、呼び手が
/// 断る側へ倒す（読めない判定を「終端でない」にも「終端」にも読み替えない）。
fn live(state_dir: &Path, id: &str, stage: Stage) -> Option<bool> {
    match stage {
        Stage::Landed | Stage::Failed | Stage::Stopped => Some(false),
        Stage::Gated => verdict_of(state_dir, id).map(|found| found != Verdict::Fail),
        Stage::Intake
        | Stage::Blocked
        | Stage::Spawned
        | Stage::Questioned
        | Stage::Implemented => Some(true),
    }
}

/// 契約単位の拒否（**rc は理由の variant が持つ**）。`extra` は理由の後ろに並べる行。
fn refuse(found: &Refuse, extra: &[String]) -> Outcome {
    let mut err = vec![format!("pipe: {}", found.reason())];
    err.extend(extra.iter().cloned());
    Outcome::failed(found.rc(), err)
}

/// 対象 repo の HEAD から vessel 宣言を読み、器の上限と突き合わせて有効値にする。
///
/// **外れは rc 1**（前提違反）で、宣言が読めない周も同じ極性である——「宣言が無い」と
/// 「宣言が壊れている」で扱いを変えると、器の視野の外の verify 行が片方から入る。
fn freeze(repo: &Path, manifest: &Manifest, contract: &Contract) -> Result<Effective, Outcome> {
    let commands = list_row(manifest, CEILING_ROW).map_err(refused)?;
    let ceiling = Ceiling { row: CEILING_ROW, commands: &commands };
    declaration::measure(repo, &ceiling, &contract.verify).map_err(|errors| {
        Outcome::failed(RC_REFUSED, errors.iter().map(ToString::to_string).collect())
    })
}

/// 有効値を便の写し面へ凍結する（以後の段は repo の宣言を読み直さない）。
fn copy_vessel(state_dir: &Path, id: &str, effective: &Effective) -> Result<(), String> {
    let path = vessel_path(state_dir, id);
    std::fs::write(&path, effective.render())
        .map_err(|err| format!("{} を書けない: {err}", path.display()))
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

/// 段を通すのに要る材料（すべて永続面から解いたもの）。
struct Resolved {
    /// 置き場。
    state_dir: PathBuf,
    /// 対象 repo。
    repo: PathBuf,
    /// 読み込み済みの契約。
    contract: Contract,
    /// 契約の bead id。
    bead: String,
    /// replay が見た現在の段（`allowed` のいずれか）。**段を動かさない口が使う**
    /// ——retire は畳んだ事実をこの段のまま残す（`s2-07l.128`）。
    stage: Stage,
    /// 承認 event が在るか（replay の導出値）。
    approved: bool,
}

/// 段の一致だけでは決まらない周の**追加の弁別**（`s2-07l.128`）。
///
/// 同じ段の中で扱いが分かれる面が 2 つ在る——`Gated` は `verdict.json` の 3 値で、`Failed` は
/// 終端の理由で分かれる。どちらも**段の検査の一部**ゆえ [`resolve`] の中（＝契約より前）に置く。
enum Extra {
    /// 段の一致だけで足りる。
    Nothing,
    /// `Gated` を**測り直し**として通してよいか（verdict が INCONCLUSIVE の周だけ）。
    Regate,
    /// **畳んで**よいか（`Failed` は detail が `rebase-empty` / `rebase-conflict` の周だけ・
    /// `Gated` は verdict が FAIL の周だけ）。
    Retire,
}

/// 段の前提を確かめ、材料を永続面から解く。**3 つの段（spawn / gate / land）が共有する**。
///
/// **順序を変えない**: 置き場 → replay → 段 → 契約 → repo。段の検査を契約より後ろへ
/// 動かすと、段違いの周に契約の error（rc 2）が先に出て「前提違反は何もせず rc 1」が
/// 崩れる（event も 1 件も書かない、という不変条件はこの順序に乗っている）。
///
/// [`Extra`] は段だけでは決まらない周の弁別を頼む印である。この弁別も段の検査の一部ゆえ
/// **契約より前**に置く——外へ出すと「段違いなのに rc 2」が特定の段だけで起こり、上の
/// 不変条件が rc の語彙ごと崩れる（lens 実測 F1）。
fn resolve(
    args: &[String],
    id: &str,
    allowed: &[Stage],
    extra: &Extra,
) -> Result<Resolved, Outcome> {
    let state_dir = state_dir_of(args).map_err(refused)?;
    let state = current(&state_dir).map_err(|errors| {
        Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect())
    })?;
    let stage = stage_of(&state, id).map_err(refused)?;
    if !allowed.contains(&stage) {
        return Err(refused(format!("run {id} の段は {} である", stage.as_str())));
    }
    discriminate(extra, &state_dir, id, stage)?;
    let Some(run) = state.runs.get(id) else {
        return Err(refused(format!("run {id} が無い")));
    };
    let contract = Contract::load(&contract_path(&state_dir, id)).map_err(|errors| {
        Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect())
    })?;
    let repo = run_repo(args, &state_dir, id).map_err(refused)?;
    Ok(Resolved {
        state_dir,
        repo,
        contract,
        bead: run.bead.clone(),
        stage,
        approved: run.approved,
    })
}

/// 段の中の弁別。外れは**段違いと同じ扱い**＝rc 1 で何も書かない（event も contract も触らない）。
///
/// - `Gated`: **測り直せるのは「測れなかった」周だけ**。PASS / FAIL は判定に届いた終端で、
///   判定が読めない周（file 不在 / 壊れ / 3 値の外）も測り直さない（fail-closed・C11.2）。
/// - `Failed`: **畳めるのは `rebase-empty` と `rebase-conflict` の周だけ**（前者は変更が既に
///   main に在る＝close してよい合図・後者は起こし直しの上限に達した便で、planner が契約を
///   切り直して流し直す・設計 pipeline-conflict.md §5）。他の理由（`main-red` /
///   `main-unmeasured` / `rebase-dirty` / `precheck:…`）は人が読む前に入れ物が動くと
///   「何が起きたか」を現物から追えなくなるので断る。理由を読めない周も断る（読めなかったを
///   畳める理由に読み替えない・fail-closed）。
/// - `Gated`: **畳めるのは verdict が FAIL の周だけ**（判定に届いた終端・`.132` の memo）。
///   PASS はまだ land が残っており、INCONCLUSIVE は測り直せる側ゆえ断る。
///
/// **理由も名乗る**: 段違いの一般則で断っている事実と、その便が通らない理由は別の情報で、
/// 片方だけだと読み手に届かない。
fn discriminate(extra: &Extra, state_dir: &Path, id: &str, stage: Stage) -> Result<(), Outcome> {
    match (extra, stage) {
        (&Extra::Regate, Stage::Gated) => gated_is(state_dir, id, Verdict::Inconclusive),
        (&Extra::Retire, Stage::Gated) => gated_is(state_dir, id, Verdict::Fail),
        (&Extra::Retire, Stage::Failed) => {
            let detail = last_stage_detail(state_dir, id);
            let foldable = detail.as_deref().is_some_and(|found| {
                found == REBASE_EMPTY || found == follow::EXHAUSTED
            });
            match foldable {
                true => Ok(()),
                false => Err(refused(format!(
                    "run {id} の段は Failed である（detail={}）",
                    detail.as_deref().unwrap_or("読めない")
                ))),
            }
        }
        (&Extra::Nothing | &Extra::Regate | &Extra::Retire, _) => Ok(()),
    }
}

/// `Gated` の便の判定が求める 3 値か。**判定を読めない周は断る**（fail-closed・C11.2）。
fn gated_is(state_dir: &Path, id: &str, want: Verdict) -> Result<(), Outcome> {
    let verdict = verdict_of(state_dir, id);
    match verdict == Some(want) {
        true => Ok(()),
        false => Err(refused(format!(
            "run {id} の段は Gated である（verdict={}）",
            verdict.map_or("読めない", Verdict::as_str)
        ))),
    }
}

/// 段を確かめてから turn の口を通す。
///
/// **runner を起こす経路はここ 1 本**で、材料を解いた後は `pipe::follow` の turn へ渡す
/// （Precheck → spawn → 追随の後始末が 1 本に収まる＝起こし直しと通常の起動で後始末が
/// 分かれない）。
fn launch(
    args: &[String],
    id: &str,
    runner: &str,
    policy: LockPolicy,
    allowed: &[Stage],
) -> Outcome {
    let resolved = match resolve(args, id, allowed, &Extra::Nothing) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    follow::spawn_turn(&Turn {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        contract: &resolved.contract,
        runner: Some(runner),
        approved: resolved.approved,
        policy,
    })
}

/// `pipe approve`。**逐語を event へ写すだけ**で、段は動かさない（resume が進める）。
fn approve_run(args: &[String], id: &str, policy: LockPolicy) -> Outcome {
    let words = match need(args, "--words") {
        Ok(found) => found.to_owned(),
        Err(reason) => return refused(reason),
    };
    // 段は問わない（承認は「これから起こすこと」への許しで、遅れて来ても記帳する）が、
    // 便が在ることは確かめる＝無い run へ承認を書くと宛先の無い記録が残る。
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Ok(found) => found,
        Err(errors) => {
            return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect())
        }
    };
    let Some(run) = state.runs.get(id) else {
        return refused(format!("run {id} が無い"));
    };
    super::approve::approve(&Approve {
        run: id,
        bead: &run.bead,
        state_dir: &state_dir,
        words: &words,
        policy,
    })
}

/// `pipe answer`。**`Questioned` の run にだけ**逐語を event へ写す（段は動かさない・resume が進める）。
///
/// 承認（[`approve_run`]）と同型だが、段は問う——質問の無い便へ回答を書くと、後で来た質問の
/// 関門が前の回答で開く。段違いは `Blocked` の未承認と同じ **rc 3 で何も書かない**。
fn answer_run(args: &[String], id: &str, policy: LockPolicy) -> Outcome {
    let words = match need(args, "--words") {
        Ok(found) => found.to_owned(),
        Err(reason) => return refused(reason),
    };
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Ok(found) => found,
        Err(errors) => {
            return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect())
        }
    };
    let Some(run) = state.runs.get(id) else {
        return refused(format!("run {id} が無い"));
    };
    if run.stage != Stage::Questioned {
        return Outcome::failed_line(
            RC_BLOCKED,
            format!("pipe: run {id} は質問で止まっていない（段 {}）", run.stage.as_str()),
        );
    }
    super::approve::answer(&Approve {
        run: id,
        bead: &run.bead,
        state_dir: &state_dir,
        words: &words,
        policy,
    })
}

/// `--run` を読んでから段の関数へ渡す。
fn by_run(args: &[String], step: impl FnOnce(&str) -> Outcome) -> Outcome {
    match need(args, "--run") {
        Err(reason) => refused(reason),
        Ok(id) => step(id),
    }
}

/// 規則から gate の線（判定の 2 行と受付の 4 行）を読む。**数値を .rs へ焼かない**（憲法 C1 / C5）。
///
/// 受付の 4 行も `--rules` の manifest から読む（埋め込みから直に読まない）——待ちの上限を
/// 振る歯が fixture の値を gate へ届ける口はここだけである。
fn limits_of(manifest: &Manifest) -> Result<Limits, String> {
    Ok(Limits {
        lens_count: int_row(manifest, ROW_LENS)?,
        token_cap: int_row(manifest, ROW_CAP)?,
        mutants_jobs: int_row(manifest, ROW_MUTANTS_JOBS)?,
        job_memory_mb: int_row(manifest, ROW_JOB_MEMORY)?,
        reserve_memory_mb: int_row(manifest, ROW_RESERVE_MEMORY)?,
        slot_wait_s: int_row(manifest, ROW_SLOT_WAIT)?,
    })
}

/// `pipe gate`。前提 stage = `Implemented` ∨ (`Gated` ∧ verdict が INCONCLUSIVE)。
///
/// **測り直せるのは「測れなかった」周だけ**である。INCONCLUSIVE は道具が足りなくて
/// 判定に届かなかった印（`--lens` 無し / diff が cap 超 / lens の不備）なので、道具を
/// 揃えれば同じ便を撃ち直せる。PASS / FAIL は判定に届いた周ゆえ**終端のまま**で、
/// 段違いの一般則どおり何もせず rc 1 を返す——FAIL から撃ち直す口を開けると、契約の
/// verify が赤い便が「壊れたまま進む」経路になる。
fn gate_run(args: &[String], id: &str, manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let resolved = match resolve(args, id, &[Stage::Implemented, Stage::Gated], &Extra::Regate) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let limits = match limits_of(manifest) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let lens = match flag(args, "--lens") {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    super::gate::gate(&Gate {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        contract: &resolved.contract,
        lens,
        limits,
        policy,
    })
}

/// `pipe land`。前提 stage = Gated（PASS の検査は land 側が持つ）。
///
/// `--pr-cmd` は自 repo への PR の口ゆえ**承認 event を前提としない**（A4.3・ADR-0008）。
/// `--lens` と規則の線は main が動いた便の追随（rebase → gate の撃ち直し・設計 §5.4）で
/// gate へ渡すために読む（land 自身は数値を見ない）。
fn land_run(args: &[String], id: &str, manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let resolved = match resolve(args, id, &[Stage::Gated], &Extra::Nothing) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let limits = match limits_of(manifest) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let pr_cmd = match flag(args, "--pr-cmd") {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let lens = match flag(args, "--lens") {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    // 追随が衝突した周は実装役を起こし直す（設計 pipeline-conflict.md §3）。`pipe run` は
    // 自分の runner をそのまま渡し、`--runner` を持たない `pipe land` は起こし直せない
    // ——衝突の記帳だけ残して断り、`pipe resume --runner` で続けられる。
    let runner = match flag(args, "--runner") {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let retries = match int_row(manifest, ROW_RETRIES) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    // 着地の順番を待つ上限（設計 gate-cost.md §6）。`--rules` の manifest から読む＝上限を振る歯の
    // fixture が land へ届く口はここだけである。
    let land_wait_s = match int_row(manifest, ROW_LAND_WAIT) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    super::land::land(&Land {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        contract: &resolved.contract,
        pr_cmd,
        lens,
        limits,
        runner,
        retries,
        land_wait_s,
        approved: resolved.approved,
        policy,
    })
}

/// `pipe retire`。前提 stage = `Landed` ∨ (`Failed` ∧ 最後の `RunStage` の detail が
/// `rebase-empty` / `rebase-conflict`) ∨ (`Gated` ∧ verdict が FAIL)（worktree 在り・clean の
/// 検査は retire 側が持つ）。
///
/// **段を動かさない口である**。`--pr-cmd` 形の便は main を動かさず worktree も残して
/// `Landed` で終端するので、merge の後に入れ物だけを畳む段が要る。同一変更の便が
/// `rebase-empty` で終端した周も**成果は既に main に在る**ので入れ物だけが残る形は同じで、
/// 畳める側に数える（`s2-07l.128`）。起こし直しの上限に達した便（`rebase-conflict`）と
/// 判定に届いた `Gated(FAIL)` も、終端して入れ物だけが残る形は同じである（設計
/// pipeline-conflict.md §5）。走っている便・他の理由で落ちた便を通すと「まだ読まれて
/// いない現物を動かす」経路になるため、段違いは一般則どおり rc 1。
///
/// 残す event の段は [`Resolved::stage`] のまま＝**`Landed` に決め打ちしない**（終端を動かさない）。
fn retire_run(args: &[String], id: &str, policy: LockPolicy) -> Outcome {
    let allowed = [Stage::Landed, Stage::Failed, Stage::Gated];
    let resolved = match resolve(args, id, &allowed, &Extra::Retire) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    super::land::retire(&Retire {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        stage: resolved.stage,
        policy,
    })
}

/// `pipe run`。intake → spawn → gate → land を 1 process で連続させる。
///
/// 各段は永続面を読み書きするので、途中で落ちても `resume` が続きを引ける。
fn run_all(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let runner = match need(args, "--runner") {
        Ok(found) => found.to_owned(),
        Err(reason) => return refused(reason),
    };
    let id = match intake_id(args, manifest, policy) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    // **run id は落ちた周も stdout に出す**。`resume` がこの id を要るためで、
    // ここで黙ると続きから引けない便が置き場に残る。
    let mut lines = vec![intake_line(args, &id)];
    let spawned = launch(args, &id, &runner, policy, &[Stage::Intake]);
    if let Some(stopped) = chain(&mut lines, spawned) {
        return stopped;
    }
    let gated = gate_run(args, &id, manifest, policy);
    if let Some(stopped) = chain(&mut lines, gated) {
        return stopped;
    }
    let landed = land_run(args, &id, manifest, policy);
    if let Some(stopped) = chain(&mut lines, landed) {
        return stopped;
    }
    Outcome::ok(lines)
}

/// 段の結果を畳む。rc≠0 ならそこまでの行を載せて**止める形**を返す。
fn chain(lines: &mut Vec<String>, outcome: Outcome) -> Option<Outcome> {
    if outcome.rc == RC_OK {
        lines.extend(outcome.out);
        return None;
    }
    let mut stopped = outcome;
    let mut out = std::mem::take(lines);
    out.extend(stopped.out);
    stopped.out = out;
    Some(stopped)
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
fn resume(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
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
        // `Implemented` の先は 2 つに分かれる（設計 pipeline-conflict.md §3・ADR-0019 §2.6）。
        // 追随が衝突して段が戻った便（最後の `RunStage` の detail が `rebase-conflict:` で
        // 始まり、runner が起きていない）は**起こし直しの続き**で、`--runner` を要る。
        // それ以外の `Implemented` は従来どおり gate。
        Ok(Stage::Implemented) => match follow_pending(&state_dir, &id) {
            false => gate_run(args, &id, manifest, policy),
            true => match need(args, "--runner") {
                Err(reason) => refused(reason),
                Ok(runner) => launch(args, &id, runner, policy, &[Stage::Implemented]),
            },
        },
        // Gated の先は判定で分かれる。**INCONCLUSIVE は land を試さない**——測れて
        // いない便に land の「PASS でない」を返すのは、吸収状態を言い換えただけである。
        // 次に撃つ段だけを名乗って rc 3 で止まる（**自動では測り直さない**＝道具の
        // 不足は人が直す）。PASS / FAIL の弁別は land 側が持ち、読む関数は
        // [`verdict_of`] の 1 本で共有する（判定の読み手は増やさない）。
        Ok(Stage::Gated) => match verdict_of(&state_dir, &id) {
            Some(Verdict::Inconclusive) => Outcome {
                out: vec![format!("run={id} next=gate")],
                err: Vec::new(),
                rc: RC_INCONCLUSIVE,
            },
            _ => land_run(args, &id, manifest, policy),
        },
        Ok(Stage::Intake) => match need(args, "--runner") {
            Err(reason) => refused(reason),
            Ok(runner) => launch(args, &id, runner, policy, &[Stage::Intake]),
        },
        // Blocked から先へ進めるのは承認 event が在る周だけ。未承認は **rc 3 のまま
        // 何も書かない**——待っている事実は既に Blocked が記帳しており、resume の
        // たびに ApprovalRequested を積むと「何回聞いたか」が事実と食い違う。
        Ok(Stage::Blocked) => match state.runs.get(&id).is_some_and(|run| run.approved) {
            false => Outcome::failed_line(
                RC_BLOCKED,
                format!("pipe: run {id} は承認待ちである（pipe approve --words \"<user の逐語>\"）"),
            ),
            true => match need(args, "--runner") {
                Err(reason) => refused(reason),
                Ok(runner) => launch(args, &id, runner, policy, &[Stage::Blocked]),
            },
        },
        // Questioned から先へ進めるのは**最新の質問への回答**が在る周だけ（`Blocked` と同型・
        // FR32）。無ければ rc 3 で何も書かない（待っている事実は Questioned が既に持つ）。
        Ok(Stage::Questioned) => match question_of_run(&state_dir, &id).is_some_and(|q| q.answer.is_some()) {
            false => Outcome::failed_line(
                RC_BLOCKED,
                format!("pipe: run {id} は回答待ちである（pipe answer --run {id} --words \"<回答の逐語>\"）"),
            ),
            true => match need(args, "--runner") {
                Err(reason) => refused(reason),
                Ok(runner) => launch(args, &id, runner, policy, &[Stage::Questioned]),
            },
        },
        Ok(stage) => refused(format!("run {id} の段 {} からは再開しない", stage.as_str())),
    }
}

/// `Implemented` の便が**起こし直しの続き**か（設計 pipeline-conflict.md §3 の `resume`）。
///
/// 条件は 2 つ——最後の `RunStage` の detail が `rebase-conflict:` で始まり、かつ runner が
/// 起きていない（走っている runner の隣にもう 1 つ起こさない）。どちらかを読めない周は
/// `false`＝従来どおり gate へ流す（読めなさで runner を起こさない・fail-closed）。
fn follow_pending(state_dir: &Path, id: &str) -> bool {
    let conflicted = last_stage_detail(state_dir, id)
        .is_some_and(|detail| follow::is_conflict(&detail));
    conflicted && runner_is_idle(state_dir, id) == Some(true)
}

/// `pipe stop`。`--run <id>` は便 1 本を外し、`--all` は生きている席を全部止める。
///
/// **2 つの口の意味は別である**: `--all` は席の掃除（対象なしは rc 0 の冪等）、`--run` は
/// 放置された便を排他の母集団から外す管理席の操作（設計 pipeline-conflict.md §2）。
fn stop(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    match flag(args, "--run") {
        Err(reason) => refused(reason),
        Ok(Some(id)) => stop_run(args, manifest, policy, id),
        Ok(None) => stop_all(args, manifest, policy),
    }
}

/// `pipe stop --run <id>`。終端でない便 1 本に `RunStopped` を書く（席が Live なら先に group 宛てに
/// 止める・**止め切れなかった周は `RunStopped` を書かず rc 1**＝run は live のまま）。
///
/// **終端の便には event を増やさず rc 1**（書込は冪等・rc は冪等でない）。2 回撃った 2 件目が
/// この経路に落ちる＝events.jsonl は 1 件しか増えない。判定を読めない `Gated` は rc 2 で断る
/// （読めない周を「終端でない」に読み替えない・fail-closed）。
fn stop_run(args: &[String], manifest: &Manifest, policy: LockPolicy, id: &str) -> Outcome {
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
        Ok(found) => found,
    };
    let Some(run) = state.runs.get(id) else {
        return refused(format!("run {id} が無い"));
    };
    match live(&state_dir, id, run.stage) {
        None => return broken(format!("run {id} の判定を読めない（終端かを測れない）")),
        Some(false) => {
            return refused(format!("run {id} は既に終端である（段 {}）", run.stage.as_str()))
        }
        Some(true) => {}
    }
    let grace = match int_row(manifest, ROW_GRACE) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    // **pid を持たない Live 席も母集団に数える**（`--all` と同じ理由＝止めていない席を
    // 黙って落とすと「全部止めた」に化ける）。止められた席にだけ `SeatStopped` を書く。
    let live_seats: Vec<(String, Option<u64>)> = state
        .seats
        .values()
        .filter(|seat| seat.state == SeatState::Live && seat.run == id)
        .map(|seat| (seat.id.clone(), seat.pid))
        .collect();
    let mut stopped = 0_usize;
    for (seat, pid) in &live_seats {
        if !pid.is_some_and(|found| terminate(found, grace)) {
            continue;
        }
        stopped = stopped.saturating_add(1);
        if let Err(err) = record_seat_stop(&state_dir, &state, (seat, id, *pid), policy) {
            return broken(err);
        }
    }
    let line = format!("stop: run={id} seats={} stopped={stopped}", live_seats.len());
    // **席を 1 つでも止め切れなかった周は `RunStopped` を書かない**（FailClosed・C9 / C6.2）。
    // 書くと run は終端として排他の母集団から外れるのに、その runner は走り続ける。
    if stopped != live_seats.len() {
        return unstoppable(line, live_seats.len().saturating_sub(stopped));
    }
    if let Err(err) = record_run_stopped(&state_dir, &state, id, policy) {
        return broken(err);
    }
    Outcome::ok_line(line)
}

/// 止め切れなかった周の形（rc 1・run は終端にしない）。
fn unstoppable(line: String, left: usize) -> Outcome {
    Outcome {
        out: vec![line],
        err: vec![format!("pipe: 止められない席が {left} 残った（run は終端にしない）")],
        rc: RC_REFUSED,
    }
}

/// `pipe stop --all`。生きている席を止める。**冪等**（対象なしは rc 0）。
fn stop_all(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
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
    // 止めた席の便（記帳順）と、止め切れなかった席を持つ便。
    let mut stopped_runs: Vec<&str> = Vec::new();
    let mut unstopped_runs: Vec<&str> = Vec::new();
    for (id, run, pid) in &live {
        // 止められなかった席に「止めた」を記帳しない。記帳すると次の周が
        // 「対象なし」を返し、生きている席が終端として消える（偽の全クリア）。
        if !pid.is_some_and(|found| terminate(found, grace)) {
            unstopped_runs.push(run.as_str());
            continue;
        }
        stopped += 1;
        if let Err(err) = record_seat_stop(&state_dir, &state, (id, run, *pid), policy) {
            return broken(err);
        }
        if !stopped_runs.contains(&run.as_str()) {
            stopped_runs.push(run);
        }
    }
    // `--run` と同じ極性: **止め切れなかった席を持つ便には `RunStopped` を書かない**。
    for run in stopped_runs.iter().filter(|run| !unstopped_runs.contains(run)) {
        if let Err(err) = record_run_stopped(&state_dir, &state, run, policy) {
            return broken(err);
        }
    }
    let line = format!("stop: seats={} stopped={stopped}", live.len());
    if stopped == live.len() {
        Outcome::ok_line(line)
    } else {
        unstoppable(line, live.len().saturating_sub(stopped))
    }
}

/// TERM → 猶予だけ待つ → 残れば KILL。**待機は fleet の 1 実装を通る**（C3.4）。
///
/// 席の pid は `pipe spawn` が立てた process group の leader（= group id）なので、**group 宛て**に
/// 撃ち、group の全員が消えるのを待つ（wrapper だけが死んで子・孫が残る形を塞ぐ）。group が無い周
/// （`kill` が rc 非 0 = group leader でない旧 record の席）と pid ≤ 1 は**単一 pid** へ撃つ（互換）。
/// true を返すのは group（互換の周は pid）が消えた周だけである。
fn terminate(pid: u64, grace_ms: u64) -> bool {
    let grace = Duration::from_millis(grace_ms);
    let Ok(target) = u32::try_from(pid) else {
        return false;
    };
    if let StopPlan::Group(group) = stop_plan(pid) {
        if signal(&group.target(), "-TERM") {
            if fleet::wait(Completion::GroupGone(target), grace).is_ok() {
                return true;
            }
            signal(&group.target(), "-KILL");
            return fleet::wait(Completion::GroupGone(target), grace).is_ok();
        }
    }
    signal(&pid.to_string(), "-TERM");
    if fleet::wait(Completion::SeatGone(target), grace).is_ok() {
        return true;
    }
    signal(&pid.to_string(), "-KILL");
    fleet::wait(Completion::SeatGone(target), grace).is_ok()
}

/// 席の止め方（2 値）。**選ぶのは [`stop_plan`] ただ 1 本**で、実 signal を送らずに pin できる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopPlan {
    /// process group 宛て（group id = 席の pid）。
    Group(GroupId),
    /// 単一 pid 宛て（group 宛てにしてはならない pid）。
    Single,
}

/// group 宛ての signal の宛先。**pid ≥ 2 だけを持てる**——`kill -- -1` は user の全 process、
/// `kill -- -0` は自分の group であり、`-{pid}` の字面はこの型からしか作らない
/// （guard 1 本の短絡に依らない・2026-09-13 の事故）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GroupId(u64);

impl GroupId {
    /// pid ≤ 1 は `None`。
    fn new(pid: u64) -> Option<Self> {
        (pid >= 2).then_some(Self(pid))
    }

    /// `kill` へ渡す負の pid の字面。
    fn target(self) -> String {
        format!("-{}", self.0)
    }
}

/// 席の pid から止め方を選ぶ（pure）。
fn stop_plan(pid: u64) -> StopPlan {
    match GroupId::new(pid) {
        Some(group) => StopPlan::Group(group),
        None => StopPlan::Single,
    }
}

/// pid へ signal を送る（std に kill は無いので `kill` を撃つ）。rc 0 の周だけ true。
fn signal(target: &str, name: &str) -> bool {
    std::process::Command::new("kill")
        .arg(name)
        .arg("--")
        .arg(target)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// 席に「止めた」を記帳する。**止められた席にだけ書く**（偽の全クリアを作らない）。
fn record_seat_stop(
    state_dir: &Path,
    state: &State,
    seat: (&str, &str, Option<u64>),
    policy: LockPolicy,
) -> Result<(), String> {
    let (id, run, pid) = seat;
    emit(
        state_dir,
        &Emit {
            kind: EventKind::SeatStopped,
            run,
            bead: bead_of(state, run),
            stage: None,
            seat: Some(id.to_owned()),
            pid,
            detail: None,
        },
        policy,
    )
    .map_err(|err| err.to_string())
}

/// 便に「止めた」を記帳する（段 = `Stopped`＝終端＝排他の母集団から外れる）。
fn record_run_stopped(
    state_dir: &Path,
    state: &State,
    run: &str,
    policy: LockPolicy,
) -> Result<(), String> {
    emit(
        state_dir,
        &Emit {
            kind: EventKind::RunStopped,
            run,
            bead: bead_of(state, run),
            stage: Some(Stage::Stopped),
            seat: None,
            pid: None,
            detail: None,
        },
        policy,
    )
    .map_err(|err| err.to_string())
}

/// 便の bead id（replay に無ければ空）。
fn bead_of<'a>(state: &'a State, run: &str) -> &'a str {
    state.runs.get(run).map_or("", |found| found.bead.as_str())
}

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行・何もしない）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::{stop_plan, GroupId, StopPlan};

    /// pid 0 / 1 は単一 pid 宛て・2 以上は group 宛て（実 signal を送らずに止め方の選択を pin する）。
    #[test]
    fn pipe_stop_group_plan_never_targets_pid_zero_or_one_as_group() {
        assert_eq!(stop_plan(0), StopPlan::Single, "pid 0 を group 宛てにしない（自分の group）");
        assert_eq!(stop_plan(1), StopPlan::Single, "pid 1 を group 宛てにしない（user の全 process）");
        assert!(matches!(stop_plan(2), StopPlan::Group(found) if found.target() == "-2"), "{:?}", stop_plan(2));
        assert!(
            matches!(stop_plan(4242), StopPlan::Group(found) if found.target() == "-4242"),
            "{:?}",
            stop_plan(4242)
        );
    }

    /// group 宛ての字面は型からしか作れず、型は pid ≤ 1 を持てない。
    #[test]
    fn pipe_stop_group_id_rejects_pid_zero_and_one() {
        assert_eq!(GroupId::new(0), None);
        assert_eq!(GroupId::new(1), None);
        assert_eq!(GroupId::new(2).map(GroupId::target), Some("-2".to_owned()));
        assert_eq!(GroupId::new(u64::MAX).map(GroupId::target), Some(format!("-{}", u64::MAX)));
    }
}
