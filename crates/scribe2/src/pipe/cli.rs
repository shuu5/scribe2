//! `pipe` subcommand の面（設計 §5）。
//!
//! **env も HOME も読まない**（憲法 C2.2）。置き場は `--state-dir` か、repo に紐づいた
//! git 設定（`vessel init` が書いたもの）から解く。規則の値は `--rules` か埋め込みの
//! manifest から読み、数値をこの file に焼かない（C1 / C5）。
//!
//! 前提違反は **rc 1 + stderr 1 行で何もしない**（event も追記しない・設計 §4）。
//! 契約 file が読めない周は「対象そのものが壊れている」ので rc 2 で、理由を全件出す。
//!
//! 本 file は入口（[`dispatch`] / [`contracts`] / [`usage`]）と材料の型（[`Resolved`] / [`Extra`]）と `mod` 宣言、
//! および再輸出の shim だけを持つ（`pipe/cli/` は subcommand と helper の責務ごとに 1 file・設計 §5）。引数と規則の
//! 行の helper は [`args`]、便の状態の helper は [`state`]、表示は [`show`]、再開は [`resume`]（`s2-07l.349` の
//! 純移動）。受付は [`intake`]、段の手は [`step`]、起動と連鎖は [`run`]（`s2-07l.295` の純移動）、受付と同じ判定を
//! run を作らず撃つ口は [`preflight`]（契約表の行 u・contract-source.md §21）。外から呼ぶ path
//! は本 file の再輸出で不変（子 module は helper を `super::` で引き、兄弟 module を `super::approve` /
//! `super::gate` / `super::land` の path で呼ぶので、その名は本 file の `use` が親として持つ）。

mod args;
mod intake;
mod preflight;
mod resume;
mod run;
mod show;
mod state;
mod step;

pub(super) use args::{broken, flag, int_row, refused, state_dir_of};
pub(super) use run::turn_of;
pub(super) use state::{live, resolve, stage_of};
use args::{list_row, manifest_of, need, repo_of};
use resume::{resume, review_then_launch};
use show::show;
use state::by_run;

use super::approve;
use super::contract::Contract;
use super::declaration::{Ceiling, CEILING_ROW, DENIED_ROW};
use super::gate;
use super::land;
use super::stop::stop;
use super::{head_of, repo_of_run, repo_path};
use crate::cli_outcome::{Outcome, RC_REFUSED};
use crate::fleet::store::LockPolicy;
use crate::fleet::Stage;
use crate::name::NAME;
use intake::intake;
use preflight::preflight;
use run::{run_all, start};
use std::path::{Path, PathBuf};
use step::{answer_run, approve_run, gate_run, land_run, retire_run};

/// `pipe` の使い方。
pub fn usage() -> String {
    format!(
        "usage: {NAME} pipe <intake|preflight|spawn|approve|answer|gate|land|retire|run|show|resume|stop|report> [--state-dir D] [--rules PATH] [stop: --all|--run ID] [flags]"
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
        Some("preflight") => preflight(args, &manifest),
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
    /// **起こして**よいか（`Reviewed` は `review.json` の verdict が PASS の周だけ・FR49・[`super::review::ReviewCheck`]）。
    /// 他の段（`Blocked` / `Questioned` / `Implemented` からの起こし直し）は段の一致だけで足りる。
    Spawn,
}
