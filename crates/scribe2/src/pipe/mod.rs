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

pub mod admission;
pub mod approve;
pub mod cli;
pub mod closure;
pub mod confine;
pub mod contract;
pub mod declaration;
pub mod dispatch;
pub mod follow;
pub mod gate;
pub mod land;
pub mod lens_record;
pub mod move_proof;
pub mod refuse;
pub mod report;
pub mod review;
mod size;
pub mod spawn;
pub mod table;
mod stop;
mod ratelimit;
mod queue;
mod retire;

use crate::polarity::{OnFailure, Polarity, Timing};
use crate::fleet::store::{self, LockPolicy, StoreError};
use crate::fleet::{self, replay, Event, EventKind, Mark, Stage, State, SCHEMA};
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

/// driver の札の名（run dir の直下・設計 dispatcher.md §5「driver の死亡」）。
pub const DRIVER_FILE: &str = "driver";

/// driver の札（`pipe run` / `pipe resume` の process が入口で置き、終端で消す）。
pub fn driver_path(state_dir: &Path, id: &str) -> PathBuf {
    run_dir(state_dir, id).join(DRIVER_FILE)
}

/// driver の札を握る（設計 dispatcher.md §5）。
///
/// 本文は所有者の pid（10 進 1 行）で、生死の判定は lock の所有者と**同じ 1 本**
/// （[`store::lock_owner`] + [`store::started_ms`]・C6.3・第 2 の probe を作らない）。`Drop` で消すので、
/// typed な断りで終わった周も畳まれた周も札は残らない——**残るのは process が死んだ周だけ**で、それが
/// 列の起こし直しの入力である。
pub struct Driver {
    /// 置き場。
    state_dir: PathBuf,
    /// 便 id。
    run: String,
    /// 置いた札。
    path: PathBuf,
    /// 札を置いた時点でその便が持っていた event の件数（**前進したか**の基準）。
    events: usize,
}

impl Driver {
    /// 札を置く。書けない周は `None` で、その便は**札の無い便**として扱われる（列は触らない・§5）。
    pub fn hold(state_dir: &Path, id: &str) -> Option<Self> {
        let path = driver_path(state_dir, id);
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, format!("{}\n", std::process::id())).ok()?;
        let events = run_events(state_dir, id);
        Some(Self { state_dir: state_dir.to_path_buf(), run: id.to_owned(), path, events })
    }
}

impl Drop for Driver {
    /// **札を消すのは便が live で無くなった周だけ**（設計 dispatcher.md §5・`s2-07l.482`）。
    ///
    /// driver が **live な便を残したまま**終わった周は札を残す——所有者が死んだ便として次の契機が
    /// 起こし直す。`pipe resume` は 1 段ずつ進める口なので、便はこの連鎖で終端まで自走する（札を
    /// 正常終了のたびに消すと、便は「札の無い live 便」＝触らない側に落ちて二度と進まない・実測 2026-09-19:
    /// 殺した driver の便が `Implemented` で止まった）。
    ///
    /// **前進しなかった周も消す**: 何も記帳せずに終わった driver（承認待ち・回答待ち・測り直しが要る
    /// `Gated` の INCONCLUSIVE 等、`pipe resume` が rc 3 で何もしない段）の札を残すと、契機のたびに
    /// 起こし直しが空撃ちされ、その空撃ち自身が次の契機になって**止まらない**（`s2-07l.482` の実装中に
    /// 見つけた形）。前進した driver の札だけが「続きが要る便」を名乗る。
    ///
    /// 段を読めない周も消す（触らない側へ倒す・fail-closed）。消せない札は次の 1 周が所有者の生死で判じる。
    fn drop(&mut self) {
        let alive = current(&self.state_dir)
            .ok()
            .and_then(|state| state.runs.get(&self.run).map(|run| run.stage))
            .and_then(|stage| cli::live(&self.state_dir, &self.run, stage));
        let advanced = run_events(&self.state_dir, &self.run) > self.events;
        if alive == Some(true) && advanced {
            return;
        }
        // **消すのは自分の札だけである**: 同じ便に別の driver が後から入って札を置き換えていれば、
        // ここで消すと**生きている driver の札**を落とす（その便は以後「札の無い便」＝誰も継がない）。
        let mine = std::fs::read_to_string(&self.path)
            .is_ok_and(|body| body.trim().parse::<u32>() == Ok(std::process::id()));
        if mine {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// 便に紐づく event の件数（**前進したか**の基準・読めない周は 0）。
fn run_events(state_dir: &Path, id: &str) -> usize {
    store::read_all(state_dir).map_or(0, |events| events.iter().filter(|event| event.run == id).count())
}

/// 便の driver が**もう駆動していない**か（**札が無い・読めない周は `false`＝触らない**・測れないを
/// 「死んだ」に読み替えない）。
///
/// 「所有者の process が無い」に加えて、**札の所有者が自分自身である周**も含む。終端の直後に撃つ 1 周は、
/// その便を駆動していた process（＝自分）が仕事を終えて抜ける直前に走る（設計 §5「終端の記帳の後」）。
/// 自分の札を「生きている」と読むと、1 段進めて抜ける driver の後を誰も継がない——`pipe resume` は 1 段ずつ
/// 進める口なので、便はそこで止まる（`s2-07l.482` の実測: 起こし直した便が `Implemented` で止まった）。
pub fn driver_is_stale(state_dir: &Path, id: &str) -> bool {
    let Ok(body) = std::fs::read_to_string(driver_path(state_dir, id)) else {
        return false;
    };
    if body.trim().parse::<u32>() == Ok(std::process::id()) {
        return true;
    }
    store::lock_owner(&body, store::started_ms) == store::Owner::Dead
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

/// base（`HEAD`）の tree の file を読む（`git show HEAD:<path>`・**作業木ではない**）。
///
/// 受付が設計 pointer の doc を読む口である（契約 (b)・設計 contract-source.md §2「生成」）: 契約の正本は
/// 記録する base と同じ commit の行で、作業木の書きかけを受け付けると **runner が base で見るもの**と
/// 契約が食い違う。読めない（commit に無い・git を撃てない・UTF-8 でない）周は `None` ＝呼び側が断る。
pub fn show_head(repo: &Path, path: &str) -> Option<String> {
    let bytes = git_bytes(repo, &["show", &format!("HEAD:{path}")])?;
    String::from_utf8(bytes).ok()
}

/// 便の base を event log から読む（spawn が記録した `base:<sha>`、または land の追随が
/// 記録した `rebase:<old>..<new>` の新しい側・**物理順で後の行が勝つ**）。
///
/// **replay の `Run::detail` からは読めない**。`detail` は「最後に見た自由文」なので、
/// gate が `verdict:<V>` を書いた時点で `base:<sha>` は上書きされて消える。base は
/// land の CAS と stale 判定の両方が要る値ゆえ、追記だけの log を遡って原本を読む。
/// **読み手はこの 1 本だけ**——spawn の再開・gate の `{base}`・land の CAS が同じ値を見る。
///
/// `Spawned` の行は `base:<sha>` か `base:<sha>,account:<label>`（器が口座を選んで起こした周・設計
/// account-autonomy.md §4）で、sha は `base:` の直後から**最初の `,` まで**（無ければ末尾まで）。
pub fn base_of_run(state_dir: &Path, id: &str) -> Base {
    let Ok(events) = store::read_all(state_dir) else {
        return Base::Unreadable;
    };
    events
        .iter()
        .rev()
        .filter(|event| event.run == id)
        .find_map(|event| {
            let detail = event.detail.as_deref()?;
            match event.stage {
                Some(Stage::Spawned) => detail
                    .strip_prefix("base:")
                    .map(|rest| rest.split_once(',').map_or(rest, |(sha, _)| sha).to_owned()),
                Some(Stage::Implemented) => detail
                    .strip_prefix("rebase:")
                    .and_then(|range| range.split_once(".."))
                    .map(|(_, new)| new.to_owned()),
                _ => None,
            }
        })
        .map_or(Base::Absent, Base::Known)
}

/// 便の base の読みの結果（**「便に base が無い」と「置き場を読めない」を分ける**・C10・設計
/// dispatcher.md §5）。
///
/// `Option` に潰すと、置き場が読めない周が「spawn を通っていない便」と同じ断りに化ける——land と gate は
/// 前者を rc 2（対象そのものが壊れている）・後者を rc 1（前提違反）で断る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Base {
    /// base の sha が分かった。
    Known(String),
    /// 便に base の記帳が無い（spawn を通っていない・追随の行も無い）。
    Absent,
    /// 置き場の event log を読めない。
    Unreadable,
}

impl Base {
    /// 分かった sha（`Absent` と `Unreadable` はどちらも `None`）。
    ///
    /// **2 つを同じに扱ってよい呼び手だけが使う**——読めない周も「base が無い」周も同じ既定へ倒す
    /// ところ（追随の節を渡さない・verdict の size の材料を持たない）に限る。
    pub fn known(self) -> Option<String> {
        match self {
            Self::Known(found) => Some(found),
            Self::Absent | Self::Unreadable => None,
        }
    }

    /// 読めなかったか（呼び手が rc 2 へ倒す周の判定）。
    pub fn is_unreadable(&self) -> bool {
        matches!(*self, Self::Unreadable)
    }
}

/// 便の**最後の `RunStage`** が名乗った `detail`（物理順で最後の 1 件）。読めない周は `None`。
///
/// 終端の理由（`rebase-empty` / `rebase-conflict` / `main-red` / …）も、衝突を記帳した
/// `rebase-conflict:<base>..<main>` も、`Failed` / `Implemented` という段だけでは弁別できない。
/// replay の `Run::detail` は「最後に見た**自由文**」なので使えない——`retire` 自身が書く
/// `detail=retired` や、段を持たない event の自由文が後から被さって理由が消える。読むのは
/// 追記だけの log の原本である（[`base_of_run`] と同じ理由）。
///
/// 読み手は 2 面（`retire` の入口の弁別・`resume` の起こし直しの弁別）で、**判定は 1 本**である。
///
/// **停止中の印（[`STOPPING`]）の行は読み飛ばし**、その手前の最後の `RunStage` の detail を返す（設計 §23）。
/// 印は `Stopped` に落ちた正常な便にも最後の `RunStage` として残るので、読み飛ばさないと衝突の記帳も
/// `Failed` の理由も印に隠れ、2 面の読み手の意味が変わる。停止中かは [`is_stopping`] が読む。
pub fn last_stage_detail(state_dir: &Path, id: &str) -> Option<String> {
    let events = store::read_all(state_dir).ok()?;
    events
        .iter()
        .rev()
        .filter(|event| event.run == id && event.kind == EventKind::RunStage)
        .find(|event| event.detail.as_deref() != Some(STOPPING))?
        .detail
        .clone()
}

/// `pipe stop --run` が最初の signal を送る**前**に書く停止中の印（`RunStage stage=<現段> detail=stopping`・
/// 設計 §23）。書く側（`pipe stop`）と読む側（[`last_stage_detail`] の読み飛ばし・[`is_stopping`]）の字面はこの 1 本。
pub const STOPPING: &str = "stopping";

/// 便が停止中か＝**生の**最後の `RunStage` の detail が [`STOPPING`] か。store を読めない周は `None`。
///
/// 読み手は `pipe stop`（印を 2 度書かない）と spawn の終端検出（停止中なら段を書かず `RunStopped` の経路に
/// 任せる）の 2 つだけである。再 spawn が書く `RunStage` は最後の記帳を置き換えるので、印は自然に読まれなくなる。
pub fn is_stopping(state_dir: &Path, id: &str) -> Option<bool> {
    let events = store::read_all(state_dir).ok()?;
    Some(
        events
            .iter()
            .rev()
            .find(|event| event.run == id && event.kind == EventKind::RunStage)
            .is_some_and(|event| event.detail.as_deref() == Some(STOPPING)),
    )
}

/// 便の runner が**起きていない**か（最後の `SeatSpawned` より後に `SeatStopped` が在る）。
///
/// 起こし直しの前提である（走っている runner の隣にもう 1 つ起こさない）。席の event を
/// 1 件も持たない便も「起きていない」＝起こしてよい側である。store を読めない周は `None`
/// ＝呼び手が起こさない側へ倒す（fail-closed）。
pub fn runner_is_idle(state_dir: &Path, id: &str) -> Option<bool> {
    let events = store::read_all(state_dir).ok()?;
    let own: Vec<&Event> = events.iter().filter(|event| event.run == id).collect();
    let spawned = own.iter().rposition(|event| event.kind == EventKind::SeatSpawned);
    let stopped = own.iter().rposition(|event| event.kind == EventKind::SeatStopped);
    Some(match (spawned, stopped) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(up), Some(down)) => down > up,
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

/// 便の質問と回答の対を event log から**発生順に全部**読む。質問が 1 件も無ければ空（log を
/// 読めない周も空＝呼び手が「質問なし」の側へ倒す）。
///
/// replay の `Run::detail` からは読めない（最後に見た自由文しか残らない）ので、追記だけの
/// log を遡って原本を読む（[`base_of_run`] と同じ理由）。1 対の区間は `QuestionRaised` から
/// 次の `QuestionRaised` の直前までで、`about` と回答はその区間の行だけを数える＝前の質問への
/// 回答で次の質問の関門が開かない。gate はこの列を裁定の写し（`rulings.txt`）に写して lens へ
/// 渡す（`s2-07l.309`・設計 pipeline-question.md）。
pub fn questions_of_run(state_dir: &Path, id: &str) -> Vec<Question> {
    let Ok(events) = store::read_all(state_dir) else {
        return Vec::new();
    };
    let own: Vec<&Event> = events.iter().filter(|event| event.run == id).collect();
    let starts: Vec<usize> = own
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind == EventKind::QuestionRaised)
        .map(|(at, _)| at)
        .collect();
    starts
        .iter()
        .enumerate()
        .filter_map(|(nth, raised)| {
            let end = starts.get(nth.saturating_add(1)).copied().unwrap_or(own.len());
            let span = own.get(*raised..end)?;
            let question = span.first()?.detail.clone().unwrap_or_default();
            let about = span
                .iter()
                .find(|event| event.kind == EventKind::RunStage && event.stage == Some(Stage::Questioned))
                .and_then(|event| event.detail.as_deref())
                .and_then(|detail| detail.strip_prefix("about:"))
                .map(str::to_owned);
            let answer = span
                .iter()
                .filter(|event| event.kind == EventKind::QuestionAnswered)
                .find_map(|event| event.detail.clone().filter(|words| !words.trim().is_empty()));
            Some(Question { question, about, answer })
        })
        .collect()
}

/// 便の**最新の**質問（[`questions_of_run`] の末尾）。質問が 1 件も無ければ `None`。
pub fn question_of_run(state_dir: &Path, id: &str) -> Option<Question> {
    questions_of_run(state_dir, id).pop()
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
        registration: None,
        mark: None,
        account: None,
    };
    store::append(state_dir, &event, policy).map(|_| ())
}

/// 列の介入の印を 1 件追記する（[`EventKind::DispatchMark`]・設計 dispatcher.md §4）。
///
/// 段の event（[`emit`]）と**本体の形が違う**ので口を分ける——印は便でなく bead に付き、`run` を持たず、
/// typed な [`Mark`] が本体である（自由文の `detail` を判定入力にしない・憲法 C3.3）。追記そのものは
/// fleet の 1 本（[`store::append`]）を通る（C6.3）。
pub fn emit_mark(state_dir: &Path, bead: &str, mark: Mark, policy: LockPolicy) -> Result<(), StoreError> {
    let event = Event {
        schema: SCHEMA,
        ts: fleet::cli::now_utc(),
        kind: EventKind::DispatchMark,
        run: String::new(),
        bead: bead.to_owned(),
        host: fleet::cli::host(),
        actor: EventKind::DispatchMark.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: None,
        allowance: None,
        registration: None,
        mark: Some(mark),
        account: None,
    };
    store::append(state_dir, &event, policy).map(|_| ())
}

/// 永続面から現在地を読む。**process の記憶を使わない**（GOAL 3）。
pub fn current(state_dir: &Path) -> Result<State, Vec<StoreError>> {
    store::read_all(state_dir).map(|events| replay(&events))
}

/// in-file の歯が共有する置き場の fixture（event の並びを固定 ts で積む・env を読まない〔C2.2〕）。
#[cfg(test)]
pub(crate) mod fixture {
    use super::contract::Contract;
    use crate::fleet::store::{self, LockPolicy};
    use crate::fleet::{Event, EventKind, Stage, SCHEMA};
    use std::path::{Path, PathBuf};

    /// 契約（write-set と 3 クラスの自己申告だけを呼び手が選ぶ）。
    pub(crate) fn contract(write_set: &[&str], classes: &[&str]) -> Contract {
        let owned = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect();
        Contract {
            goal: "g".to_owned(),
            done: "d".to_owned(),
            size: "S".to_owned(),
            owner: "s2-mutant".to_owned(),
            disposition: "A-now".to_owned(),
            write_set: owned(write_set),
            verify: Vec::new(),
            req: Vec::new(),
            design: "docs/design/pipeline.md".to_owned(),
            classes: owned(classes),
            opens: Vec::new(),
            touches: Vec::new(),
        }
    }

    /// 歯ごとの空の tmp dir。
    pub(crate) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pipe-mutant-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// log の 1 行（固定 ts・段と席と pid と detail は呼び手が選ぶ）。
    pub(crate) fn event(run: &str, kind: EventKind, stage: Option<Stage>, seat: Option<&str>, detail: Option<&str>) -> Event {
        Event {
            schema: SCHEMA,
            ts: "2026-09-14T00:00:00Z".to_owned(),
            kind,
            run: run.to_owned(),
            bead: "b".to_owned(),
            host: "h".to_owned(),
            actor: kind.default_actor().to_owned(),
            stage,
            seat: seat.map(str::to_owned),
            pid: None,
            detail: detail.map(str::to_owned),
            allowance: None,
            registration: None,
            mark: None,
            account: None,
        }
    }

    /// 着地待ちの列に入る便を 1 本置く（`Gated` の event・repo の写し・worktree の dir・判定）。
    pub(crate) fn gated_run(state_dir: &Path, repo: &Path, run: &str, verdict: &str) {
        append_all(state_dir, &[event(run, EventKind::RunStage, Some(Stage::Gated), None, None)]);
        let _ = std::fs::create_dir_all(super::run_dir(state_dir, run));
        let _ = std::fs::write(super::repo_path(state_dir, run), format!("{}\n", repo.display()));
        let _ = std::fs::create_dir_all(super::worktree_path(repo, run));
        let _ = std::fs::write(super::verdict_path(state_dir, run), format!("{{\"verdict\":\"{verdict}\"}}\n"));
    }

    /// 置き場へ event を順に積む（書けない周は読み手の assert が落ちる）。
    pub(crate) fn append_all(state_dir: &Path, events: &[Event]) {
        let Ok(policy) = LockPolicy::embedded() else {
            return;
        };
        for found in events {
            let _ = store::append(state_dir, found, policy);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{append_all, event, scratch};
    use super::{base_of_run, last_stage_detail, question_of_run, questions_of_run, runner_is_idle, Base, Question};
    use crate::fleet::{EventKind, Stage};

    /// `base_of_run` は `Spawned` の `base:<sha>` と `base:<sha>,account:<label>`（器が口座を選んで起こした周）の
    /// 両方から sha を読む（`,` の手前まで・`s2-07l.285`）。`account:<label>,resume:rate-limit` の行は飛ばし、
    /// `rebase:<old>..<new>` の新しい側が物理順で後なら勝つ。
    #[test]
    fn pipe_spawn_account_base_of_run_reads_sha_before_the_account_suffix() {
        let root = scratch("base-of-run");
        let spawned = |run: &str, detail: &str| event(run, EventKind::RunStage, Some(Stage::Spawned), None, Some(detail));
        append_all(
            &root,
            &[
                spawned("plain", "base:aaa111"),
                spawned("chosen", "base:bbb222,account:a2"),
                spawned("resumed", "base:ccc333,account:a1"),
                spawned("resumed", "account:a2,resume:rate-limit"),
                spawned("moved", "base:ddd444,account:a1"),
                event("moved", EventKind::RunStage, Some(Stage::Implemented), None, Some("rebase:ddd444..eee555")),
            ],
        );
        let known = |sha: &str| Base::Known(sha.to_owned());
        assert_eq!(base_of_run(&root, "plain"), known("aaa111"), "従来の base:<sha>");
        assert_eq!(base_of_run(&root, "chosen"), known("bbb222"), "`,account:` の手前まで");
        assert_eq!(base_of_run(&root, "resumed"), known("ccc333"), "再開の行は飛ばす");
        assert_eq!(base_of_run(&root, "moved"), known("eee555"), "追随の新しい側が勝つ");
        // **「行の無い便」と「置き場を読めない」は別の値**（C10・`s2-07l.482`）。
        assert_eq!(base_of_run(&root, "none"), Base::Absent, "行の無い便");
        // 読めない周は**行が 1 本でも壊れている**周である（dir が無い周は 0 件＝`Absent` で正しい）。
        let broken = scratch("base-of-run-broken");
        let log = broken.join("fleet").join("events.jsonl");
        std::fs::create_dir_all(log.parent().unwrap_or(&broken)).ok();
        std::fs::write(&log, "{\"schema\":1,\"kind\":\"Nonsense\"}\n").ok();
        assert_eq!(base_of_run(&broken, "plain"), Base::Unreadable, "置き場を読めない");
        let _ = std::fs::remove_dir_all(&broken);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `questions_of_run` は対を**発生順に全部**返し、`question_of_run` はその末尾である（`s2-07l.309`）。
    /// 1 対の区間は次の `QuestionRaised` の直前まで＝1 つ目の回答は 2 つ目の質問に付かず、`about` の無い対は
    /// `None`・空白だけの回答は数えない。他の便の質問は数えず、質問の無い便は空 / `None`。
    #[test]
    fn pipe_mod_questions_of_returns_all_pairs_and_question_of_is_the_last() {
        let root = scratch("questions-of");
        append_all(
            &root,
            &[
                event("me", EventKind::QuestionRaised, None, None, Some("q1")),
                event("me", EventKind::RunStage, Some(Stage::Questioned), None, Some("about:verify")),
                event("me", EventKind::QuestionAnswered, None, None, Some("a1")),
                event("other", EventKind::QuestionRaised, None, None, Some("not-mine")),
                event("me", EventKind::QuestionRaised, None, None, Some("q2")),
                event("me", EventKind::RunStage, Some(Stage::Questioned), None, None),
                event("me", EventKind::QuestionAnswered, None, None, Some("  ")),
                event("me", EventKind::QuestionAnswered, None, None, Some("a2")),
                event("me", EventKind::QuestionRaised, None, None, Some("q3")),
                event("me", EventKind::RunStage, Some(Stage::Questioned), None, Some("about:done")),
            ],
        );
        let pair = |question: &str, about: Option<&str>, answer: Option<&str>| Question {
            question: question.to_owned(),
            about: about.map(str::to_owned),
            answer: answer.map(str::to_owned),
        };
        let all = questions_of_run(&root, "me");
        assert_eq!(
            all,
            vec![pair("q1", Some("verify"), Some("a1")), pair("q2", None, Some("a2")), pair("q3", Some("done"), None)],
            "発生順に全部・区間は次の質問の直前まで"
        );
        assert_eq!(question_of_run(&root, "me"), all.last().cloned(), "最新は列の末尾");
        assert_eq!(questions_of_run(&root, "none"), Vec::new(), "質問の無い便は空");
        assert_eq!(question_of_run(&root, "none"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    // flip-check: retroactive s2-07l.222
    /// `runner_is_idle` の 4 分岐を片側ずつ撃つ: 席の event が無い便は `true`（`Some(false)` 固定で落ちる）・
    /// 起こしただけの便は `false`（`Some(true)` 固定で落ちる）・起こして止めた便は `true`／止めて起こし直した
    /// 便は `false`（`>` を `==` / `<` にすると片側が落ちる）。他の便の event は数えない（run の `==` を `!=` に
    /// すると他の便の席を自分の席と読む）。起こしと止めの kind を取り違える変異（`==` → `!=`）は、2 件の並びで
    /// 位置が入れ替わって落ちる。`>` → `>=` は 1 行が 1 kind ゆえ 2 つの位置が等しくならず equivalent。
    #[test]
    fn mutant_in_pipe_runner_is_idle_pins_each_branch() {
        let root = scratch("runner-idle");
        let spawned = |run: &str| event(run, EventKind::SeatSpawned, None, Some("seat-1"), None);
        let stopped = |run: &str| event(run, EventKind::SeatStopped, None, Some("seat-1"), None);

        let none = root.join("none");
        append_all(&none, &[spawned("other")]);
        assert_eq!(runner_is_idle(&none, "me"), Some(true), "席の event が無い便（他の便の席は数えない）");

        let up = root.join("up");
        append_all(&up, &[spawned("me"), stopped("other")]);
        assert_eq!(runner_is_idle(&up, "me"), Some(false), "起こしただけ（他の便の止めは数えない）");

        let down = root.join("down");
        append_all(&down, &[spawned("me"), stopped("me")]);
        assert_eq!(runner_is_idle(&down, "me"), Some(true), "起こした後に止めた");

        let again = root.join("again");
        append_all(&again, &[stopped("me"), spawned("me")]);
        assert_eq!(runner_is_idle(&again, "me"), Some(false), "止めた後に起こし直した");
        let _ = std::fs::remove_dir_all(&root);
    }

    // flip-check: retroactive s2-07l.222
    /// `last_stage_detail` の `&&` を片側ずつ撃つ: 読むのは**自分の便の** `RunStage` の最後の detail で、後から
    /// 積まれた他の便の `RunStage`（run の条件を外すと読む）と自分の便の段を持たない event（kind の条件を
    /// 外す・`&&` を `||` にすると読む）の detail を読まない。`RunStage` を 1 件も持たない便は `None`。
    #[test]
    fn mutant_in_pipe_last_stage_detail_needs_both_run_and_kind() {
        let root = scratch("last-stage");
        append_all(
            &root,
            &[
                event("me", EventKind::RunStage, Some(Stage::Failed), None, Some("own-stage")),
                event("other", EventKind::RunStage, Some(Stage::Failed), None, Some("other-stage")),
                event("me", EventKind::SeatStopped, None, Some("seat-1"), Some("own-seat")),
                event("seatless", EventKind::SeatStopped, None, Some("seat-2"), Some("no-stage")),
            ],
        );
        assert_eq!(last_stage_detail(&root, "me"), Some("own-stage".to_owned()));
        assert_eq!(last_stage_detail(&root, "seatless"), None, "RunStage を持たない便");
        let _ = std::fs::remove_dir_all(&root);
    }
}
