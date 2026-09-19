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

pub(super) use args::{broken, flag, int_row, present, refused, state_dir_of};
// 列（`pipe::dispatch`）は受付の判定を**記帳せずに**撃つ（設計 dispatcher.md §3・C2 の 1 実装）。
// 可視性を上げるだけで本文は不変——2 本目の判定を作らないための再輸出である。
pub(in crate::pipe) use intake::{crossings, generated, judge, Denial, Material, Materials};
pub(super) use run::turn_of;
pub(super) use state::{live, resolve, stage_of};
use args::{list_row, manifest_of, need, repo_flag, repo_of, REPO_FLAG};
use resume::{resume, review_then_launch};
use show::show;
use state::by_run;

use super::approve;
use super::contract::Contract;
use super::dispatch as queue;
use super::declaration::{Ceiling, CEILING_ROW, DENIED_ROW};
use super::gate;
use super::land;
use super::stop::stop;
use super::{head_of, repo_of_run, repo_path};
use crate::cli_outcome::{Outcome, RC_OK, RC_REFUSED};
use crate::fleet::store::LockPolicy;
use crate::fleet::{Mark, Stage};
use crate::rules::manifest::Manifest;
use crate::seat::ledger::DEFAULT_BD;
use crate::name::NAME;
use intake::intake;
use preflight::preflight;
use run::{run_all, start};
use std::path::PathBuf;
use step::{answer_run, approve_run, gate_run, land_run, retire_run};

/// `pipe` の使い方。
pub fn usage() -> String {
    format!(
        "usage: {NAME} pipe <intake|preflight|spawn|approve|answer|gate|land|retire|run|show|resume|stop|report|dispatch> [--state-dir D] [--repo R] [--rules PATH] [stop: --all|--run ID] [dispatch: (1 周)|ls|first|hold|release BEAD] [run|resume: --drive] [land: --terminal-only] [--runner CMD] [flags]"
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
    let verb = args.first().map(String::as_str);
    // **自分が段を進めた便**は subcommand しか知らない（run は便を作り、resume は入口の段を読む）ので、
    // 判定の材料を typed に受け取る（stdout の字面から run id を読み戻さない・C3.3）。
    let mut driven: Option<Driven> = None;
    let mut outcome = subcommand(args, &manifest, policy, verb, &mut driven);
    // **`--drive` を持つ周だけ自分の便を次の driver に渡す**（設計 dispatcher.md §5）。flag の無い周は
    // 今までどおり 1 段だけ進めて抜ける＝段を手で 1 つずつ進める既存の歯は 1 本も動かない。
    let driving = driven.filter(|_| present(args, queue::DRIVE));
    // **終端の記帳の後・lock の外で列を 1 周撃つ**（設計 dispatcher.md §5）。観測の面は増やさない（§6）ので
    // flag の無い周には行を足さず、**効果（起こした便の `RunCreated`）だけ**が残る。1 周が失敗しても終端の
    // rc は変えない——起こせなかった便は次の契機で拾う。
    if verb.is_some_and(|found| TERMINALS.contains(&found)) {
        if let Some(queue) = queue_of(args, &manifest, driving.as_ref()) {
            let turn = queue::fire(&queue.borrow());
            // 自走を頼んだ周は、渡したか・渡さなかった理由を 1 行で残す（C10・黙って止まらない）。
            if driving.is_some() {
                outcome.out.push(queue::line(&turn));
            }
        }
    }
    outcome
}

/// **自分が駆動した便**（`pipe run` / `pipe resume` が名乗る・設計 §5「渡す周と渡さない周」）。
///
/// `--drive` を読むのは [`dispatch`] の 1 か所である——subcommand の側で読むと、flag の意味
/// （自走するか）が 2 か所で決まる。ここが名乗るのは**事実**（どの便の、入口の段は何だったか）だけ。
pub(super) struct Driven {
    /// 便 id。
    pub(super) run: String,
    /// 入口で読んだ段（便を作る `pipe run` は `None`）。
    pub(super) entry: Option<Stage>,
}

/// **便が live で無くなりうる subcommand**（設計 dispatcher.md §5「便の終端」）。
///
/// 終端を作ったかを見分けずに撃つ——終端が無かった周は交差も受付も動いておらず、列は同じ答えを返す
/// （起こせる便が増えないだけ）。見分ける述語を足すと、終端の検出と列の判定を 2 か所が別々に決めることになる。
const TERMINALS: [&str; 5] = ["run", "resume", "land", "stop", "retire"];

/// 列の 1 周の材料を引数から解く（解けない面が 1 つでも在れば `None`＝1 周を撃たない）。
///
/// **repo も置き場も引数で名指されていなければ撃たない**（cwd へ落ちない）。列は「この置き場の便」と
/// 「この repo の契約」を突き合わせる口なので、片方を cwd から推すと**別の repo の契約を別の置き場へ
/// 起こす**（2026-09-19 の実測: toy の置き場の終端が cwd の repo の bead を起こした）。列を起こす側の
/// 判定は fail-closed に倒す（NFR4）。
fn queue_of<'a>(args: &'a [String], manifest: &'a Manifest, driven: Option<&'a Driven>) -> Option<Queue<'a>> {
    // repo の読み手は [`repo_flag`] の 1 本（絶対 path に直す・設計 dispatcher.md §12）。
    let (Some(state_dir), Some(repo)) = (flag(args, "--state-dir").ok()?, repo_flag(args).ok()?) else {
        return None;
    };
    let state_dir = PathBuf::from(state_dir);
    Some(Queue {
        state_dir,
        repo,
        manifest,
        bd: flag(args, "--bd").ok()?,
        rules: flag(args, "--rules").ok()?,
        lens: flag(args, "--lens").ok()?,
        curl: flag(args, "--curl").ok()?,
        runner: flag(args, "--runner").ok()?,
        driven,
    })
}

/// 解いた材料（[`queue::Input`] は借りだけを持つので、その借り元をここで持つ）。
struct Queue<'a> {
    /// 置き場。
    state_dir: PathBuf,
    /// 対象 repo（anchor）。
    repo: PathBuf,
    /// 規則の値。
    manifest: &'a Manifest,
    /// 台帳 client（引数で名指されていなければ `None`＝列は既定を読み、起こす便には渡さない）。
    bd: Option<&'a str>,
    /// 規則の写しの path（起こす便へそのまま渡す）。
    rules: Option<&'a str>,
    /// 審査の lens の口（同上）。
    lens: Option<&'a str>,
    /// 口座残量の計測の口（同上）。
    curl: Option<&'a str>,
    /// 実装役の口（同上・無ければ列は 1 本も起こさない）。
    runner: Option<&'a str>,
    /// 自走を頼んだ呼び手の便（`--drive` の周だけ `Some`）。
    driven: Option<&'a Driven>,
}

impl Queue<'_> {
    /// 借りの形（列の 1 周が読む）。
    fn borrow(&self) -> queue::Input<'_> {
        queue::Input {
            state_dir: &self.state_dir,
            repo: &self.repo,
            manifest: self.manifest,
            bd: self.bd.unwrap_or(DEFAULT_BD),
            bd_flag: self.bd,
            rules: self.rules,
            lens: self.lens,
            curl: self.curl,
            runner: self.runner,
            driving: self.driven.map(|found| queue::Driving { run: &found.run, entry: found.entry }),
        }
    }
}

/// subcommand 1 つを撃つ（列の 1 周は呼び手が足す）。
fn subcommand(
    args: &[String],
    manifest: &Manifest,
    policy: LockPolicy,
    verb: Option<&str>,
    driven: &mut Option<Driven>,
) -> Outcome {
    match verb {
        Some("intake") => intake(args, manifest, policy),
        Some("preflight") => preflight(args, manifest),
        Some("spawn") => start(args, policy),
        Some("approve") => by_run(args, |id| approve_run(args, id, policy)),
        Some("answer") => by_run(args, |id| answer_run(args, id, policy)),
        Some("gate") => by_run(args, |id| gate_run(args, id, manifest, policy)),
        Some("land") => by_run(args, |id| land_run(args, id, manifest, policy)),
        Some("retire") => by_run(args, |id| retire_run(args, id, policy)),
        Some("run") => run_all(args, manifest, policy, driven),
        Some("show") => show(args),
        Some("resume") => resume(args, manifest, policy, driven),
        Some("stop") => stop(args, manifest, policy),
        Some("dispatch") => queued(args, manifest, policy),
        Some("report") => match state_dir_of(args) {
            Err(reason) => refused(reason),
            Ok(state_dir) => super::report::report(&state_dir),
        },
        _ => Outcome::failed(RC_REFUSED, vec![usage()]),
    }
}

/// 列を 1 周撃ち、その結果の 1 行を outcome に足す（**rc は変えない**・設計 dispatcher.md §5）。
///
/// 材料（置き場・repo）を解けない周は 1 周を撃たず、`dispatch=unmeasured reason=args` を足す
/// （**測れないを「起こす便 0」に読み替えない**・C10）。
fn with_turn(args: &[String], manifest: &Manifest, mut outcome: Outcome) -> Outcome {
    outcome.out.push(turn_line(args, manifest));
    outcome
}

/// 列の 1 周の 1 行（引数から材料を解いて [`queue::fire`] を撃つ＝**起こす側**）。
fn turn_line(args: &[String], manifest: &Manifest) -> String {
    match queue_of(args, manifest, None) {
        Some(queue) => queue::line(&queue::fire(&queue.borrow())),
        None => format!("dispatch=unmeasured reason={ARGS_UNMEASURED}"),
    }
}

/// 引数から列の材料を解けなかった周の理由（台帳の読めなさ〔`ledger`〕と別の値である）。
const ARGS_UNMEASURED: &str = "args";

/// `pipe dispatch <ls|first|hold|release>`: 審査を通った契約の列の観測と介入の印（設計 dispatcher.md §4・§6）。
///
/// **権能なしの口**である（誰が撃っても同じ 1 周・起動の権能は列の判定であって席の権能ではない・
/// ADR-0045 §2 (1)）。列の 1 周そのものは [`queue::turn`] の 1 本で、本 file は引数を解くだけである。
fn queued(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    match args.get(1).map(String::as_str) {
        // **観測は起こさない**（設計 §6）: `ls` は [`queue::turn`] を撃ち、[`queue::fire`] は撃たない。
        Some("ls") => match queue_of(args, manifest, None) {
            Some(queue) => queue::render(&queue::turn(&queue.borrow())),
            None => refused("列の材料（置き場・repo・台帳 client）を解けない".to_owned()),
        },
        Some(name) if !name.starts_with("--") => match (Mark::parse(name), args.get(2)) {
            // **印の直後にも 1 周撃つ**（設計 §5）: `hold` は起こす側を増やさないので撃たない。
            (Some(mark), Some(bead)) if !bead.starts_with("--") => {
                let marked = queue::mark(&state_dir, bead, mark, policy);
                if mark == Mark::Hold || marked.rc != RC_OK {
                    return marked;
                }
                with_turn(args, manifest, marked)
            }
            _ => Outcome::failed(RC_REFUSED, vec![queue::usage()]),
        },
        // **手動の 1 周**（権能なしの口・設計 §5）: subcommand の無い周（flag だけ・引数なし）は列を 1 周撃つ。
        _ => Outcome::ok(vec![turn_line(args, manifest)]),
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
        // repo の読み手は pipe と同じ [`repo_flag`] の 1 本（`need` と同じ字面で必須を断る）。
        let repo = repo_flag(args)?.ok_or(format!("{REPO_FLAG} が要る"))?;
        let manifest = manifest_of(args)?;
        let (commands, denied) = (list_row(&manifest, CEILING_ROW)?, list_row(&manifest, DENIED_ROW)?);
        let ceiling = Ceiling { row: CEILING_ROW, commands: &commands, denied: &denied };
        Ok(super::table::check_repo(&repo, &ceiling))
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
