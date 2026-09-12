//! 縦 1 本の pipeline（設計 docs/design/pipeline.md）。
//!
//! 各 subcommand は **fleet の replay から現在 stage を読んで前提を検査し、event を
//! 1 件以上追記して終わる**（FR3・AC4）。process 間で持ち越す面は event log と
//! `<state_dir>/pipe/<run>/` だけで、process の記憶には何も置かない。
//!
//! **runner を起動できる関数は [`spawn::spawn`] の 1 本**で、その引数 [`Budget`] は
//! [`Precheck::measure`] の実測を消費してしか作れない（憲法 C6「起動口は 1 つ」）。
//! CLI の `spawn` / `resume` はこの 1 関数への経路であって別の口ではない。
//!
//! **env も HOME も読まない**（C2.2）。置き場は repo に紐づいた git 設定か `--state-dir`。

pub mod approve;
pub mod cli;
pub mod contract;
pub mod declaration;
pub mod gate;
pub mod land;
pub mod refuse;
pub mod report;
pub mod spawn;

use crate::polarity::{OnFailure, Polarity, Timing};
use crate::fleet::store::{self, LockPolicy, StoreError};
use crate::fleet::{self, replay, Event, EventKind, Stage, State, SCHEMA};
use crate::name::NAME;
use std::path::{Path, PathBuf};

/// 便ごとの写しを置く dir 名。
pub const DIR: &str = "pipe";

/// runner の包みが「質問 record で止まった」ことを名乗る rc（設計 pipeline-question.md §3）。
///
/// pipeline 側の契約として置く（包み = `headless` はこれを import する）。上限の rc 75
/// （実行の中断）とは意味が違い、**包みが終了後に付ける typed な名札**である。
pub const RC_QUESTION: u8 = 76;

/// 契約 file の写しの名。
pub const CONTRACT_FILE: &str = "contract.toml";

/// 便ごとに凍結した vessel 宣言（Effective）の写しの名。
pub const VESSEL_FILE: &str = "vessel.toml";

/// 便 1 本の写しを置く dir。
pub fn run_dir(state_dir: &Path, id: &str) -> PathBuf {
    state_dir.join(DIR).join(id)
}

/// 便の契約 file の写し。
pub fn contract_path(state_dir: &Path, id: &str) -> PathBuf {
    run_dir(state_dir, id).join(CONTRACT_FILE)
}

/// 便の vessel 宣言（Effective）の写し。**以後の段はこれだけを読む**（設計 §5.1）。
pub fn vessel_path(state_dir: &Path, id: &str) -> PathBuf {
    run_dir(state_dir, id).join(VESSEL_FILE)
}

/// 便の runner へ渡す plugin の写し（run dir 配下＝**repo の外**・設計 §5.2）。
///
/// run dir の規則は [`run_dir`] ただ 1 本から導く（dir の字面を 2 本目として書かない）。
pub fn plugin_path(state_dir: &Path, id: &str) -> PathBuf {
    run_dir(state_dir, id).join("plugin")
}

/// spawn が捕らえた runner の stdout を残す file（診断 file・機械は読まない）。
///
/// stdout を捕らえる（質問 record の読み面）と、包みが出す観測行（`runner: rc=… records=…
/// observed=…`・rate-limit status の集合を育てる唯一の口）が端末から消える。捕らえた全文を
/// 周ごとに見出し付きで append し、観測面を塞がない。
pub fn runner_stdout_path(state_dir: &Path, id: &str) -> PathBuf {
    run_dir(state_dir, id).join("runner.stdout.log")
}

/// gate が逐条の rc を書く file。
pub fn verify_log_path(state_dir: &Path, id: &str) -> PathBuf {
    run_dir(state_dir, id).join("verify.jsonl")
}

/// gate の判定を書く file。
pub fn verdict_path(state_dir: &Path, id: &str) -> PathBuf {
    run_dir(state_dir, id).join("verdict.json")
}

/// 便の対象 repo を書き留める file。
///
/// repo は event の schema（閉じた key 集合）に載らないので、便ごとの写し面に置く。
/// ここに無いと `show` や `resume` が **その process の cwd** を見ることになり、
/// 「現在地は永続面から読む」（GOAL 3）が崩れる。
pub fn repo_path(state_dir: &Path, id: &str) -> PathBuf {
    run_dir(state_dir, id).join("repo")
}

/// 便に紐づいた repo を読む。書かれていなければ `None`。
pub fn repo_of_run(state_dir: &Path, id: &str) -> Option<PathBuf> {
    let text = std::fs::read_to_string(repo_path(state_dir, id)).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// 便の worktree を集める dir（repo 相対の固定 path・設計 §5.2）。
///
/// 便の worktree も land 後の retired も main 実測用の tmp も、**この 1 本から導く**
/// （dir の字面を module ごとに書くと、置き場を変えたとき片側だけが取り残される）。
pub fn worktrees_dir(repo: &Path) -> PathBuf {
    repo.join(".worktrees").join(NAME)
}

/// 便の worktree（repo 相対の固定 path・設計 §5.2）。
pub fn worktree_path(repo: &Path, id: &str) -> PathBuf {
    worktrees_dir(repo).join(id)
}

/// 便の branch 名。
pub fn branch_name(id: &str) -> String {
    format!("{NAME}/{id}")
}

/// run id = `<bead>-<UTC stamp>`。
///
/// stamp から `-` と `:` を落とすのは、id が dir 名と branch 名になるためである
/// （字面の出所は [`fleet::cli::now_utc`] ただ 1 本）。
pub fn run_id(bead: &str, now: &str) -> String {
    let stamp: String = now.chars().filter(|ch| *ch != '-' && *ch != ':').collect();
    format!("{bead}-{stamp}")
}

pub use measure::{Budget, Precheck};

/// spawn の予算の極性（[`Budget`]）: 起動の前に測り、測れない repo（git repo でない）では起動しない。
pub const BUDGET_POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 実測と予算を**兄弟 module から作れない**位置に閉じ込める（憲法 C6）。
///
/// `Budget` の field をこの module の private にすると、`pipe::spawn` は兄弟なので
/// 値を組み立てられない。`Precheck::measure` を通る以外に `Budget` を得る道が無く、
/// 「測らずに起動する」経路が型として存在しない状態を compile 時に保てる。
mod measure {
    use super::contract::Contract;
    use std::path::Path;

    /// 起動の前に実測した量。**[`Precheck`] を通してしか作れない**。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Budget {
        write_set: usize,
        verify: usize,
        size: String,
    }

    impl Budget {
        /// write-set の本数。
        pub fn write_set(&self) -> usize {
            self.write_set
        }

        /// verify 行の本数。
        pub fn verify(&self) -> usize {
            self.verify
        }

        /// 見積の目安。
        pub fn size(&self) -> &str {
            &self.size
        }
    }

    /// 起動前の実測。
    ///
    /// MVP は上限を効かせない（`R-C6-1` が未定）が、**型の形を先に置く**ことで
    /// 「runner を起動する前に必ず測る」を compile 時に守る。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Precheck {
        budget: Budget,
    }

    impl Precheck {
        /// 契約と repo を実測する。repo が git repo でなければ `Err`。
        pub fn measure(contract: &Contract, repo: &Path) -> Result<Self, String> {
            if super::head_of(repo).is_none() {
                return Err(format!("{} は git repo でない", repo.display()));
            }
            Ok(Self {
                budget: Budget {
                    write_set: contract.write_set.len(),
                    verify: contract.verify.len(),
                    size: contract.size.clone(),
                },
            })
        }

        /// 実測を [`Budget`] へ変える。**これが唯一の作り方である**。
        pub fn into_budget(self) -> Budget {
            self.budget
        }
    }
}

/// repo の HEAD。git repo でなければ `None`。
pub fn head_of(repo: &Path) -> Option<String> {
    git_line(repo, &["rev-parse", "HEAD"])
}

/// 便の base を event log から読む（spawn が記録した `base:<sha>`、または land の追随が
/// 記録した `rebase:<old>..<new>` の新しい側・**物理順で後の行が勝つ**）。
///
/// **replay の `Run::detail` からは読めない**。`detail` は「最後に見た自由文」なので、
/// gate が `verdict:<V>` を書いた時点で `base:<sha>` は上書きされて消える。base は
/// land の CAS と stale 判定の両方が要る値ゆえ、追記だけの log を遡って原本を読む。
/// **読み手はこの 1 本だけ**——spawn の再開・gate の `{base}`・land の CAS が同じ値を見る。
pub fn base_of_run(state_dir: &Path, id: &str) -> Option<String> {
    let events = store::read_all(state_dir).ok()?;
    events
        .iter()
        .rev()
        .filter(|event| event.run == id)
        .find_map(|event| {
            let detail = event.detail.as_deref()?;
            match event.stage {
                Some(Stage::Spawned) => detail.strip_prefix("base:").map(str::to_owned),
                Some(Stage::Implemented) => detail
                    .strip_prefix("rebase:")
                    .and_then(|range| range.split_once(".."))
                    .map(|(_, new)| new.to_owned()),
                _ => None,
            }
        })
}

/// 便の最新の質問と、それへの回答（在れば）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// 質問の逐語（`QuestionRaised.detail`）。
    pub question: String,
    /// 契約のどの key に関する質問か（`RunStage(Questioned).detail` の `about:` の後ろ・任意）。
    pub about: Option<String>,
    /// 回答の逐語（最新の質問より**後**の `QuestionAnswered.detail`・非空のものだけ）。
    pub answer: Option<String>,
}

/// 便の**最新の**質問を event log から読む。質問が 1 件も無ければ `None`。
///
/// replay の `Run::detail` からは読めない（最後に見た自由文しか残らない）ので、追記だけの
/// log を遡って原本を読む（[`base_of_run`] と同じ理由）。回答は最新の質問より後の行だけを
/// 数える＝前の質問への回答で次の質問の関門が開かない。
pub fn question_of_run(state_dir: &Path, id: &str) -> Option<Question> {
    let events = store::read_all(state_dir).ok()?;
    let own: Vec<&Event> = events.iter().filter(|event| event.run == id).collect();
    let raised = own
        .iter()
        .rposition(|event| event.kind == EventKind::QuestionRaised)?;
    let after = own.get(raised..)?;
    let question = after.first()?.detail.clone().unwrap_or_default();
    let about = after
        .iter()
        .find(|event| event.kind == EventKind::RunStage && event.stage == Some(Stage::Questioned))
        .and_then(|event| event.detail.as_deref())
        .and_then(|detail| detail.strip_prefix("about:"))
        .map(str::to_owned);
    let answer = after
        .iter()
        .filter(|event| event.kind == EventKind::QuestionAnswered)
        .find_map(|event| event.detail.clone().filter(|words| !words.trim().is_empty()));
    Some(Question { question, about, answer })
}

/// git を 1 回撃って stdout を byte のまま得る。rc≠0 は `None`。
///
/// [`git_line`] は trim して 1 行にするので、diff の byte 数を測る面には使えない
/// （末尾改行と空行が落ちて **cap との照合が実際より小さく出る**）。
pub fn git_bytes(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

/// git を 1 回撃って rc だけを見る。
pub fn git_ok(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}

/// git を 1 回撃って stdout の 1 行を得る。失敗・空はいずれも `None`。
pub fn git_line(dir: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

/// 追記する 1 件の材料。
///
/// 引数で受けず struct で束ねるのは、event の field が 7 つ在り、関数 1 本の引数の
/// 上限（rules 行 `R-C4-4.args`）を超えるためである。
pub struct Emit<'a> {
    /// 起きたことの種類。
    pub kind: EventKind,
    /// 便 id。
    pub run: &'a str,
    /// 契約の bead id。
    pub bead: &'a str,
    /// 段（任意）。
    pub stage: Option<Stage>,
    /// 席 id（任意）。
    pub seat: Option<String>,
    /// runner の pid（任意）。
    pub pid: Option<u64>,
    /// 自由文（任意）。
    pub detail: Option<String>,
}

/// event を 1 件追記する。**追記の口は fleet の 1 本だけを通る**（C6.3）。
pub fn emit(state_dir: &Path, entry: &Emit<'_>, policy: LockPolicy) -> Result<(), StoreError> {
    let event = Event {
        schema: SCHEMA,
        ts: fleet::cli::now_utc(),
        kind: entry.kind,
        run: entry.run.to_owned(),
        bead: entry.bead.to_owned(),
        host: fleet::cli::host(),
        actor: entry.kind.default_actor().to_owned(),
        stage: entry.stage,
        seat: entry.seat.clone(),
        pid: entry.pid,
        detail: entry.detail.clone(),
        // pipeline の段は必ず便に紐づく（口座残量の行は `fleet` 側の口が書く）。
        allowance: None,
    };
    store::append(state_dir, &event, policy).map(|_| ())
}

/// 永続面から現在地を読む。**process の記憶を使わない**（GOAL 3）。
pub fn current(state_dir: &Path) -> Result<State, Vec<StoreError>> {
    store::read_all(state_dir).map(|events| replay(&events))
}
