//! `pipe` subcommand の面（設計 §5）。
//!
//! **env も HOME も読まない**（憲法 C2.2）。置き場は `--state-dir` か、repo に紐づいた
//! git 設定（`vessel init` が書いたもの）から解く。規則の値は `--rules` か埋め込みの
//! manifest から読み、数値をこの file に焼かない（C1 / C5）。
//!
//! 前提違反は **rc 1 + stderr 1 行で何もしない**（event も追記しない・設計 §4）。
//! 契約 file が読めない周は「対象そのものが壊れている」ので rc 2 で、理由を全件出す。
//!
//! 本 file は入口（[`dispatch`] / [`contracts`] / [`usage`]）と共通の材料（flag の読み・規則の値・置き場・段の
//! 前提の解き [`resolve`]）と表示（[`show`]）と再開（[`resume`]）を持つ。受付は [`intake`]、段の手は [`step`]、
//! 起動と連鎖は [`run`]（`s2-07l.295` の純移動・外から呼ぶ path は本 file の再輸出で不変）。
//! 子 module の本文は兄弟 module を `super::approve` / `super::gate` / `super::land` の path で呼ぶ（本文を
//! 書き換えない）ので、その名は本 file の `use` が親として持つ（`table` は受付が `crate::pipe::table` で直に引く）。

mod intake;
mod run;
mod step;

pub(super) use run::turn_of;

use super::approve::{self, RC_BLOCKED};
use super::contract::Contract;
use super::declaration::{Ceiling, CEILING_ROW, DENIED_ROW};
use super::follow;
use super::gate::{self, Check, Verdict, RC_INCONCLUSIVE};
use super::land::{self, verdict_of, REBASE_EMPTY};
use super::ratelimit::ride_out_rate_limit;
use super::review::ReviewCheck;
use super::stop::stop;
use super::{
    contract_path, current, head_of, last_stage_detail, question_of_run, repo_of_run, repo_path,
    runner_is_idle, verify_log_path, worktree_path,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::json_lite;
use crate::fleet::store::{LockPolicy, StoreError};
use crate::fleet::{Stage, State};
use crate::hook::vessel;
use crate::name::NAME;
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use intake::{intake, run_repo};
use run::{chain, launch, run_all, start};
use std::path::{Path, PathBuf};
use step::{answer_run, approve_run, gate_run, land_run, retire_run, review_run};

/// `pipe` の使い方。
pub fn usage() -> String {
    format!(
        "usage: {NAME} pipe <intake|spawn|approve|answer|gate|land|retire|run|show|resume|stop|report> [--state-dir D] [--rules PATH] [stop: --all|--run ID] [flags]"
    )
}

/// **作らない口**の字面（設計 contract-source.md §4「人の関与 0」・AC22・C16）。審査の段を人が飛ばす flag は
/// 無い——黙って読み飛ばすと「効いている」ように見える launcher が残るので、usage で断る（lens の `--cap` と同型）。
const REFUSED_FLAGS: [&str; 1] = ["--no-review"];

/// `pipe` に続く引数を捌く。
pub fn dispatch(args: &[String]) -> Outcome {
    if let Some(found) = args.iter().find(|arg| REFUSED_FLAGS.contains(&arg.as_str())) {
        return Outcome::failed(RC_REFUSED, vec![format!("pipe: 未知の引数 {found}（審査の段を飛ばす口は無い）"), usage()]);
    }
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

/// `contracts` の使い方（設計 contract-source.md §2「表の検査」）。
pub fn contracts_usage() -> String {
    format!("usage: {NAME} contracts <check --repo R [--rules PATH]|schema>")
}

/// `<NAME> contracts <check|schema>`: 契約表の全行の検査（上限は `--rules` か埋め込みの `runner.allowed_commands` と
/// 対の `runner.denied_commands`）と欄の生成物の描画（tracked な `contracts/schema.toml` の出所・設計 contract-source.md §2）。
pub fn contracts(args: &[String]) -> Outcome {
    let checked = || -> Result<Outcome, String> {
        let (repo, manifest) = (need(args, "--repo")?, manifest_of(args)?);
        let (commands, denied) = (list_row(&manifest, CEILING_ROW)?, list_row(&manifest, DENIED_ROW)?);
        let ceiling = Ceiling { row: CEILING_ROW, commands: &commands, denied: &denied };
        Ok(super::table::check_repo(Path::new(repo), &ceiling))
    };
    match args.first().map(String::as_str) {
        Some("schema") if args.len() == 1 => Outcome::ok(super::table::render_schema()),
        Some("check") => checked().unwrap_or_else(|reason| Outcome::failed_line(RC_REFUSED, format!("contracts: {reason}"))),
        _ => Outcome::failed_line(RC_REFUSED, contracts_usage()),
    }
}

/// `--<name> <値>` を読む。値欠けは黙って落とさず `Err`（SRS NFR4）。
///
/// 器の中で 3 本目の flag reader である。4 本目が要るときは 1 本へ畳む
/// （いまは fleet / vessel / pipe がそれぞれ自分の必須 flag だけを見ている）。
pub(super) fn flag<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, String> {
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
pub(super) fn int_row(manifest: &Manifest, id: &str) -> Result<u64, String> {
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
pub(super) fn state_dir_of(args: &[String]) -> Result<PathBuf, String> {
    if let Some(found) = flag(args, "--state-dir")? {
        return Ok(PathBuf::from(found));
    }
    let root = repo_of(args)?;
    vessel::state_dir(&root)
        .ok_or_else(|| format!("{} に置き場が紐づいていない（vessel init）", root.display()))
}

/// 便が live（終端でない）か。**段の網羅 match で書く**（段が増えたら compile で気付く）。
///
/// 終端 = `Landed` / `Failed` / `Stopped`、または `Gated` で verdict が FAIL（pipeline.md §4
/// 「FAIL は終端」）、または `Reviewed` で verdict が PASS でない（contract-source.md §4「FAIL / INCONCLUSIVE は
/// 終端」）。`RateLimited` は終端でない（口座の窓の都合で止まっただけ・ADR-0020 §2.1）。`Gated` / `Reviewed` の
/// 判定を読めない周は `None`＝**測れなかった**で、呼び手が断る側へ倒す（読めない判定を「終端でない」にも
/// 「終端」にも読み替えない）。
pub(super) fn live(state_dir: &Path, id: &str, stage: Stage) -> Option<bool> {
    match stage {
        Stage::Landed | Stage::Failed | Stage::Stopped => Some(false),
        Stage::Gated => verdict_of(state_dir, id).map(|found| found != Verdict::Fail),
        Stage::Reviewed => ReviewCheck::judge(state_dir, id).live(),
        Stage::Intake
        | Stage::Blocked
        | Stage::Spawned
        | Stage::Questioned
        | Stage::RateLimited
        | Stage::Implemented => Some(true),
    }
}

/// 前提の段を replay から読む。無ければ `Err`。
pub(super) fn stage_of(state: &State, id: &str) -> Result<Stage, String> {
    state
        .runs
        .get(id)
        .map(|run| run.stage)
        .ok_or(format!("run {id} が無い"))
}

/// 段を通すのに要る材料（すべて永続面から解いたもの）。
pub(super) struct Resolved {
    /// 置き場。
    pub(super) state_dir: PathBuf,
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
pub(super) enum Extra {
    /// 段の一致だけで足りる。
    Nothing,
    /// `Gated` を**測り直し**として通してよいか（verdict が INCONCLUSIVE の周だけ）。
    Regate,
    /// **畳んで**よいか（`Failed` は detail が `rebase-empty` / `rebase-conflict` の周だけ・
    /// `Gated` は verdict が FAIL の周だけ）。
    Retire,
    /// **起こして**よいか（`Reviewed` は `review.json` の verdict が PASS の周だけ・FR49・[`ReviewCheck`]）。
    /// 他の段（`Blocked` / `Questioned` / `Implemented` からの起こし直し）は段の一致だけで足りる。
    Spawn,
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
pub(super) fn resolve(
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
/// - `Reviewed`: **起こせるのは verdict が PASS の周だけ**（FR49・設計 contract-source.md §4「効き方」）。
///   FAIL / INCONCLUSIVE は終端で、判定を読めない周も起こさない（fail-closed・[`ReviewCheck`]）。
///
/// **理由も名乗る**: 段違いの一般則で断っている事実と、その便が通らない理由は別の情報で、
/// 片方だけだと読み手に届かない。
fn discriminate(extra: &Extra, state_dir: &Path, id: &str, stage: Stage) -> Result<(), Outcome> {
    match (extra, stage) {
        (&Extra::Regate, Stage::Gated) => gated_is(state_dir, id, Verdict::Inconclusive),
        (&Extra::Retire, Stage::Gated) => gated_is(state_dir, id, Verdict::Fail),
        (&Extra::Spawn, Stage::Reviewed) => match ReviewCheck::judge(state_dir, id) {
            ReviewCheck::Passed => Ok(()),
            found => Err(refused(format!("run {id} の段は Reviewed である（verdict={}）", found.as_str()))),
        },
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
        (&Extra::Nothing | &Extra::Regate | &Extra::Retire | &Extra::Spawn, _) => Ok(()),
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

/// `--run` を読んでから段の関数へ渡す。
fn by_run(args: &[String], step: impl FnOnce(&str) -> Outcome) -> Outcome {
    match need(args, "--run") {
        Err(reason) => refused(reason),
        Ok(id) => step(id),
    }
}

/// 対象 repo。`--repo` が無ければ cwd の repo root。
fn repo_of(args: &[String]) -> Result<PathBuf, String> {
    if let Some(found) = flag(args, "--repo")? {
        return Ok(PathBuf::from(found));
    }
    let cwd = std::env::current_dir().map_err(|err| format!("cwd を解決できない: {err}"))?;
    vessel::repo_root(&cwd).ok_or("repo の root を解決できない".to_owned())
}

/// `pipe show`。1 行目は便の段、2 行目以降は gate の検出線の判定行（[`detection_lines`]・在る周だけ）。
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
    let mut lines = vec![format!(
        "run={} bead={} stage={} approved={} worktree={}",
        run.id,
        run.bead,
        run.stage.as_str(),
        run.approved,
        worktree_path(&repo, &id).display()
    )];
    lines.extend(detection_lines(&verify_log_path(&state_dir, &id)));
    Outcome::ok(lines)
}

/// gate の `verify.jsonl` の **検出線の record の `line=`** を逐語で並べる（設計 gate-cost.md §5.1・
/// 値の読み手）。他の kind の `line`（flip-check の判定行など）は出さない＝record を読む。
///
/// 無い周は空: file が無い（gate 前）・record に `line` が無い（stdout の無い道具・省いた周）。
/// 測り直しで gate を 2 周した便は record が追記されているので、周の数だけ並ぶ。
fn detection_lines(path: &Path) -> Vec<String> {
    let detection = Check::Detection.as_str();
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|text| json_lite::parse_object(text.trim()).ok())
        .filter(|pairs| field(pairs, "kind") == Some(detection))
        .filter_map(|pairs| field(&pairs, "line").map(str::to_owned))
        .collect()
}

/// flat object の文字列 field。
fn field<'a>(pairs: &'a [(String, json_lite::Value)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(found, _)| found == key).and_then(|(_, value)| value.as_str())
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
            true => relaunch(args, &id, policy, Stage::Implemented),
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
        // 審査を通っていない便（`RunCreated` の直後に process が落ちた周）は**先に審査**し、PASS の周だけ
        // 起こす（FR49・設計 contract-source.md §4「効き方」）。審査の段の event と `review.json` はここで残る。
        Ok(Stage::Intake) => match need(args, "--runner") {
            Err(reason) => refused(reason),
            Ok(runner) => review_then_launch(args, &id, runner, manifest, policy),
        },
        // `Reviewed` から起こせるのは verdict が PASS の周だけ（[`Extra::Spawn`] が弁別する）。
        Ok(Stage::Reviewed) => relaunch(args, &id, policy, Stage::Reviewed),
        // Blocked から先へ進めるのは承認 event が在る周だけ。未承認は **rc 3 のまま
        // 何も書かない**——待っている事実は既に Blocked が記帳しており、resume の
        // たびに ApprovalRequested を積むと「何回聞いたか」が事実と食い違う。
        Ok(Stage::Blocked) => match state.runs.get(&id).is_some_and(|run| run.approved) {
            false => Outcome::failed_line(
                RC_BLOCKED,
                format!("pipe: run {id} は承認待ちである（pipe approve --words \"<user の逐語>\"）"),
            ),
            true => relaunch(args, &id, policy, Stage::Blocked),
        },
        // Questioned から先へ進めるのは**最新の質問への回答**が在る周だけ（`Blocked` と同型・
        // FR32）。無ければ rc 3 で何も書かない（待っている事実は Questioned が既に持つ）。
        Ok(Stage::Questioned) => match question_of_run(&state_dir, &id).is_some_and(|q| q.answer.is_some()) {
            false => Outcome::failed_line(
                RC_BLOCKED,
                format!("pipe: run {id} は回答待ちである（pipe answer --run {id} --words \"<回答の逐語>\"）"),
            ),
            true => relaunch(args, &id, policy, Stage::Questioned),
        },
        // 上限で止まった便は器が別口座を選んで起こし直す（設計 account-autonomy.md §4・FR37）。人の
        // 操作は要らない（候補なしは reset まで待つ・終端は stop だけ）。
        Ok(Stage::RateLimited) => match need(args, "--runner") {
            Err(reason) => refused(reason),
            Ok(runner) => ride_out_rate_limit(args, &id, runner, manifest, policy),
        },
        Ok(stage) => refused(format!("run {id} の段 {} からは再開しない", stage.as_str())),
    }
}

/// `--runner` を読んで、その段の便を起こし直す（`resume` の各段が共有する形・`--runner` 欠けは rc 1）。
fn relaunch(args: &[String], id: &str, policy: LockPolicy, stage: Stage) -> Outcome {
    match need(args, "--runner") {
        Err(reason) => refused(reason),
        Ok(runner) => launch(args, id, runner, policy, &[stage]),
    }
}

/// 審査（`Intake` → `Reviewed`）を通してから起こす（`pipe run` と `resume` が共有する 1 本・FR49）。
///
/// 審査が PASS でない周はその判定行と rc（FAIL = 1 / INCONCLUSIVE = 3）で止まり、**runner を起こさない**。
/// PASS の周だけ [`launch`] へ進む（[`Extra::Spawn`] が `review.json` を読み直す＝判定の読み手は 1 本）。
pub(super) fn review_then_launch(
    args: &[String],
    id: &str,
    runner: &str,
    manifest: &Manifest,
    policy: LockPolicy,
) -> Outcome {
    let mut lines = Vec::new();
    if let Some(stopped) = chain(&mut lines, review_run(args, id, manifest, policy)) {
        return stopped;
    }
    let spawned = launch(args, id, runner, policy, &[Stage::Reviewed]);
    chain(&mut lines, spawned).unwrap_or_else(|| Outcome::ok(lines))
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

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行・何もしない）。
pub(super) fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
pub(super) fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}
