//! runner を起動する**唯一の関数**（設計 §5.2・FR4 / FR6・憲法 C6）。
//!
//! [`spawn`] は [`Budget`] を引数に取り、`Budget` は [`super::Precheck::measure`] を
//! 消費してしか作れない。したがって「測らずに起動する」経路は型として存在しない。
//!
//! **scribe2 固有の env は 1 つも足さない**（C2.2・ADR-0004 §2.4）。runner へは
//! 親の env をそのまま継承させ、必要な値は cmd の placeholder 置換で渡す。

use super::approve::{block, Approval, Approve, RC_BLOCKED};
use super::confine;
use super::follow::{Halt, Resumption};
use super::gate::last_json_object;
use super::refuse;
use super::{
    base_of_run, branch_name, contract_path, emit, git_line, plugin_path, runner_stdout_path, vessel_path,
    worktree_path, Budget, Emit, Question, RC_QUESTION,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::LockPolicy;
use crate::fleet::{EventKind, Stage};
use crate::headless::runner::{stop_status, top_level_string};
use crate::headless::{NO_VALUE, RC_RATE_LIMIT};
use crate::name::NAME;
use crate::pipe::contract::Contract;
use crate::polarity::{OnFailure, Polarity, Timing};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// policy file の名前（guard が読む形・vessel-hook.md §5）。
const WRITE_SET_FILE: &str = "write-set.txt";

/// runner へ載せる plugin の中身（repo 相対・設計 §6）。
const PLUGIN_DIRS: [&str; 2] = [".claude-plugin", "hooks"];

/// 器の plugin manifest（`gen-manifest` の生成物＝tracked と同じ bytes・設計 §5.2 手順 5）。
const EMBEDDED_PLUGIN_JSON: &str = include_str!("../../../../.claude-plugin/plugin.json");

/// 器の hooks（[`EMBEDDED_PLUGIN_JSON`] と同じく生成物の埋め込み）。
const EMBEDDED_HOOKS_JSON: &str = include_str!("../../../../hooks/hooks.json");

/// 器の plugin として root の `<NAME>/` へ必ず書く 3 つ組（dir・file 名・本文）。
const EMBEDDED_PLUGIN: [(&str, &str, &str); 2] = [
    (".claude-plugin", "plugin.json", EMBEDDED_PLUGIN_JSON),
    ("hooks", "hooks.json", EMBEDDED_HOOKS_JSON),
];

/// consumer の plugin を写す root 配下の subdir 名。
const CONSUMER_DIR: &str = "consumer";

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
    /// **回答済みの質問**（`Questioned` からの再 spawn だけが持つ・設計 pipeline-question.md §5）。
    /// 在る周は同じ run の worktree と base を使い、runner の stdin に「回答」節を付ける。
    pub answered: Option<Question>,
    /// **追随の相手**（main の sha・便の base が main の真の祖先である周だけ・設計
    /// pipeline-conflict.md §3）。在る周は同じ run の worktree と base を使い、runner の
    /// stdin に「追随」節を付ける。値の出所は [`super::follow::section`] ただ 1 本である。
    pub follow: Option<String>,
    /// **途中再開**（上限で止まった `RateLimited` からの再 spawn と、runner が死んだ `Spawned` からの再 spawn
    /// だけが持つ・設計 account-autonomy.md §4）。在る周は同じ run の worktree と base を使い、runner の stdin に
    /// 「途中再開」節を付ける。値の出所は [`super::follow::resumption`] ただ 1 本で、止まった理由（[`Halt`]）も
    /// そこから来る（[`Account::Resumed`] の detail の印はこの理由で分かれる）。
    pub resumed: Option<Resumption>,
    /// runner を起こす口座（閉じた 3 値・ADR-0017 §2.3・設計 account-autonomy.md §4）。label を持つ周は runner の
    /// 行に `--account-dir <state_dir>/accounts/<label>` を足し、[`Account::Inherit`] は親の環境をそのまま継承させる。
    pub account: Account<'a>,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// runner を起こす口座（**閉じた 3 値**・設計 account-autonomy.md §4・FR36 / FR37）。
///
/// 段の detail の形は variant ごとに固定である: [`Inherit`](Self::Inherit) は `base:<sha>`、
/// [`Chosen`](Self::Chosen) は `base:<sha>,account:<label>`、[`Resumed`](Self::Resumed) は
/// `account:<label>,resume:rate-limit` / `account:<label>,resume:runner-dead`（印は止まった理由 [`Halt`] で
/// 分かれる・base は初回の行が持ったまま）。読み手（`base_of_run`）は `base:` の直後から最初の `,` までを sha と読む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Account<'a> {
    /// 口座の宣言が無い周: 親の環境をそのまま継承させる（どの口座かを器は知らない）。
    Inherit,
    /// 初回の起動・承認後・回答後・衝突の起こし直しで器が便用の規則で選んだ口座。
    Chosen(&'a str),
    /// 途中再開（上限で止まった便の別口座での起こし直し・runner が死んだ便の起こし直し）で器が選んだ口座。
    Resumed(&'a str),
}

impl<'a> Account<'a> {
    /// 器が選んだ label（[`Self::Inherit`] は `None`）。`--account-dir` を足すかと `RateLimited` の detail の
    /// label はこの 1 本で決まる（選んだ経路の違いは見ない）。
    fn label(self) -> Option<&'a str> {
        match self {
            Self::Inherit => None,
            Self::Chosen(label) | Self::Resumed(label) => Some(label),
        }
    }
}

/// 口座を渡していない周に段の detail へ書く label の代わり（閉じた 1 つ）。
const INHERITED_ACCOUNT: &str = "inherited";

/// 上限で止まった便の別口座での起こし直しを段の detail に名乗る印（`Spawned detail=account:<label>,resume:rate-limit`）。
const RESUME_RATE_LIMIT: &str = "resume:rate-limit";

/// runner が死んだ便の起こし直しを段の detail に名乗る印（`Spawned detail=account:<label>,resume:runner-dead`）。
const RESUME_RUNNER_DEAD: &str = "resume:runner-dead";

/// 途中再開の印（閉じた 2 つ・理由 [`Halt`] の値ごとに固定）。
fn resume_mark(halt: Halt) -> &'static str {
    match halt {
        Halt::RateLimit => RESUME_RATE_LIMIT,
        Halt::RunnerDead => RESUME_RUNNER_DEAD,
    }
}

/// runner を起動して結果まで見届ける。**これが唯一の起動口である**。
pub fn spawn(budget: Budget, launch: &Launch<'_>) -> Outcome {
    let _ = budget.write_set();
    // **A1「実行前」の関門はここに置く**（設計 §5.5）。起動口が 1 本なので、この 1 行が
    // spawn / resume / run のすべての経路を覆う。呼び手側に置くと経路が増えるたびに
    // 素通りの穴が空く。
    match Approval::judge(launch.contract, launch.approved) {
        Approval::Required(classes) => return block(&approval(launch), classes),
        Approval::Granted => {}
    }
    let (worktree, base) = match prepare_worktree(launch) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let write_set = match write_policy(&worktree, &launch.contract.write_set) {
        Ok(path) => path,
        Err(reason) => return broken(reason),
    };
    let plugin = match copy_plugin(&worktree, launch.state_dir, launch.run) {
        Ok(path) => path,
        Err(reason) => return broken(reason),
    };
    // **起動行はここで組み上げる**（`Spawned` の記帳より前）。口座の断り（[`LineRefusal`]）を
    // `launch_runner` に置くと、段を記帳した後で起こさない周ができる——記帳した口座と実行が
    // 一致しない行が置き場に残る。worktree は作ったまま（`prepare_worktree` の後の断りと同じ形）。
    let line = substitute(launch, &worktree, &write_set, &plugin, &base);
    let cmd = match with_account(line, launch.account.label(), launch.state_dir) {
        Ok(line) => line,
        Err(refusal) => return refused(refusal.to_string()),
    };
    // 途中再開は `account:<label>,resume:<理由>` を名乗る（設計 account-autonomy.md §4・印は止まった理由で分かれる）。
    // base は初回の `base:<sha>` が持ったままで、読み手（`base_of_run`）は接頭辞の違う行を飛ばす。
    // 器が選んだ口座での起動は `base:<sha>,account:<label>`（読み手は最初の `,` までを sha と読む）。
    // 理由を読めない途中再開（節の材料が無い）は起こさない（印を推量しない・fail-closed）。
    let detail = match (launch.account, launch.resumed.as_ref()) {
        (Account::Inherit, _) => format!("base:{base}"),
        (Account::Chosen(label), _) => format!("base:{base},account:{label}"),
        (Account::Resumed(label), Some(resumed)) => format!("account:{label},{}", resume_mark(resumed.halt)),
        (Account::Resumed(_), None) => return refused(format!("run {} の途中再開の理由を読めない", launch.run)),
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
            detail: Some(detail),
        },
        launch.policy,
    ) {
        return broken(err.to_string());
    }
    launch_runner(launch, &worktree, &cmd, &base)
}

/// runner を起こし、終わりまで見届けて段を決める（起動行は [`spawn`] が組み上げて渡す）。
fn launch_runner(launch: &Launch<'_>, worktree: &Path, cmd: &str, base: &str) -> Outcome {
    // **turn 開始時の tip**（ADR-0019 §2.6）。質問の判定はこの点からの commit 数で見る
    // ——base 基準だと、起こし直しの turn は便が base から持つ commit を数えてしまい、
    // 質問で止まった turn が常に「commit を作った」側へ倒れる。初回は tip = base ゆえ同値。
    // 読めない周は base へ落とす（従来の基準）。
    let tip = git_line(worktree, &["rev-parse", "HEAD"]).unwrap_or_else(|| base.to_owned());
    // **runner も cgroup の scope で包む**（設計 gate-cost.md §4.1 の 2 つ目）。`{jobs}` を
    // 持つ起動ではないので箱は host の予約分（同 §4.2）で、包めない host では素のまま撃つ
    // （止めない・縮退する）。
    let unit = confine::unit_name(launch.run, RUNNER_STAGE, 1);
    let wrap = confine::Wrap {
        unit: &unit,
        limit: confine::Limit::HostReserve,
        caps: confine::Caps::embedded(),
    };
    let (mut command, confinement) = confine::wrap_line(cmd, &wrap);
    // **env を 1 つも足さない**: `.env()` / `.envs()` を呼ばず親の env をそのまま継承する（`TMUX_PANE` だけは外す＝confine）。
    // stdout は捕らえる（質問 record の読み面・`gate.rs::ask_lens` と同じ形）。stderr は継承。
    // **先頭 process を新しい process group の leader にする**（setsid ではない・cgroup の scope とは
    // 独立）。`SeatSpawned` の pid はそのまま group id として読まれ、`pipe stop` は group 宛てに
    // 撃つ＝wrapper だけが死んで runner や claude が残る形を塞ぐ（設計 §5.6）。
    let child = command
        .process_group(0)
        .current_dir(worktree)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(found) => found,
        Err(err) => return broken(format!("runner を起動できない: {err}")),
    };
    let pid = u64::from(child.id());
    if let Err(err) = seat(launch, EventKind::SeatSpawned, Some(pid), None) {
        return broken(err);
    }
    if let Some(mut stdin) = child.stdin.take() {
        // 読まずに終える runner への write は EPIPE になる。**段は rc と stdout で決める**ので
        // ここの失敗は理由にしない（take で drop され、runner は EOF を見る）。
        let _ = stdin.write_all(prompt(launch).as_bytes());
    }
    // rc が要るのでここは `wait_with_output`（`Child::wait` と同じ待ち・stdout を回収する形）。
    // pid の生存だけを見る待機（`pipe stop`）は `fleet::wait` のままで、**待機の実装は
    // 増えていない**（C3.4）。
    let waited = child.wait_with_output();
    // **終端で scope を片付ける**（設計 gate-cost.md §4.4 errata・`s2-07l.234`）。runner が孤児を
    // 残しても scope を active のまま置かない。段の判定は変えない（結果は stderr の 1 行だけ）。
    let scope = confine::release_scope(&confinement).map(|released| format!("pipe: runner scope={}", released.as_str()));
    let out = match waited {
        Ok(found) => found,
        Err(err) => return broken(format!("runner の終了を待てない: {err}")),
    };
    if let Err(err) = seat(launch, EventKind::SeatStopped, Some(pid), None) {
        return broken(err);
    }
    let rc = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // 捕らえた stdout は診断 file へ残す（包みの観測行を端末から消さない）。書けない周は
    // 段の判定を変えない（stderr 1 行で loud）。
    let kept = keep_stdout(launch, rc, &stdout).err();
    let mut outcome = if super::is_stopping(launch.state_dir, launch.run) == Some(true) {
        // **停止中の便は段を 1 件も書かない**（設計 pipeline.md §23）。runner を消したのは `pipe stop` で、
        // 終端は `RunStopped` の経路が書く——ここで `Failed` を書くと stop の終端を上書きする。
        stopped_underneath(launch)
    } else if let Some(reason) = box_killed(&confinement, rc, &stdout) {
        // **箱の中で死んだ周は便を終端する**（設計 gate-cost.md §4.2）。verify 行の「測れなかった」
        // とは極性が違う——便の内容が測れないのではなく、便自身が箱の中で死んだ。
        // 理由は閉じた語彙の 1 つ（`runner-rc` と同じ終端の段）で、`Failed` から resume しない。
        record_stage(launch, Stage::Failed, Some(reason.as_str().to_owned()))
    } else if rc == i32::from(RC_QUESTION) {
        settle_question(launch, worktree, &tip, &stdout)
    } else if rc == i32::from(RC_RATE_LIMIT) {
        settle_rate_limit(launch, rc, &stdout)
    } else {
        // **rc が 76 / 75 でない周は最終行を読まない**（従来どおり）。
        settle(launch, worktree, base, rc)
    };
    if let Some(reason) = kept {
        outcome.err.push(format!("pipe: runner の stdout を残せない: {reason}"));
    }
    outcome.err.extend(scope);
    outcome
}

/// runner の scope の unit 名に載せる段の名。
const RUNNER_STAGE: &str = "runner";

/// runner の包みが箱の中で死んだ理由（設計 gate-cost.md §4.2 / §4.3・pipeline.md §23）。
///
/// **oom-kill は kernel の証拠がある周だけ**＝包みが stdout の終端に出した `oom_kill` が 1 以上。
/// 終端行が在って `oom_kill` が 0 の周は箱の中の死と読まず `None`（従来の settle へ落ちる）。
/// 終端行が無い / 読めない周の **signal 死**（rc が無い＝`code()` が `None` の周・器は -1 と記す）は
/// 証拠が無いので [`confine::Reason::Unknown`]（外からの kill を oom-kill に化けさせない・C10）。
/// **包めなかった周は当たらない**。
fn box_killed(confinement: &confine::Confinement, rc: i32, stdout: &str) -> Option<confine::Reason> {
    if !confinement.confined() {
        return None;
    }
    match confine::read_usage(stdout).oom_kill {
        Some(count) => (count >= 1).then_some(confine::Reason::OomKill),
        None => (rc < 0).then_some(confine::Reason::Unknown),
    }
}

/// 停止中の便で runner が消えた周（設計 pipeline.md §23）: 段は書かず（`SeatStopped` は書き済み）、終端を
/// `pipe stop` の `RunStopped` に任せて rc 1 で止まる（呼び手が次の段へ進まない）。
fn stopped_underneath(launch: &Launch<'_>) -> Outcome {
    Outcome::failed_line(
        RC_REFUSED,
        format!("pipe: run {} は停止中に runner が消えた（終端は pipe stop が書く）", launch.run),
    )
}

/// 捕らえた runner の stdout を `<run_dir>/runner.stdout.log` へ見出し付きで append する。
/// 空の周は書かない（読む理由の無い見出しで埋めない）。
fn keep_stdout(launch: &Launch<'_>, rc: i32, stdout: &str) -> Result<(), String> {
    if stdout.trim().is_empty() {
        return Ok(());
    }
    let path = runner_stdout_path(launch.state_dir, launch.run);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| format!("{} を開けない: {err}", path.display()))?;
    let body = format!("## {} rc={rc}\n{}\n", crate::fleet::cli::now_utc(), stdout.trim_end());
    file.write_all(body.as_bytes())
        .map_err(|err| format!("{} を書けない: {err}", path.display()))
}

/// 器が足す口座の flag（足す側と、行が既に持つかを見る側の**同じ 1 つの字面**）。
const ACCOUNT_DIR_FLAG: &str = "--account-dir";

/// 起動行の受付の極性（[`LineRefusal`]・設計 account-autonomy.md §16・C11.2）: runner を起こす**前**に
/// 測り、既に口座を持つ行は足さずに断る（どちらの口座が正かを器は決められない＝断る側へ倒す）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 起動行の受付の断り（**閉じた 1 つ**・C11.2「境界ごとの enum が極性型を運ぶ」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineRefusal {
    /// 起動行が既に `--account-dir` を持つ（値は**行が持っていた**方）。
    AccountDirPresent(String),
}

impl std::fmt::Display for LineRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccountDirPresent(found) => write!(f, "起動行に {ACCOUNT_DIR_FLAG} が既に在る（{found}）"),
        }
    }
}

/// 器が選んだ口座を起動行に足す（`--account-dir <state_dir>/accounts/<label>`・FR5 の口のまま）。
///
/// placeholder でなく**末尾に足す**——runner の雛形は口座を知らず（口座は便でなく器が選ぶ）、穴を
/// 雛形に要ると、穴の無い雛形の便が黙って親の口座で起きる。渡していない周は行を変えない（親の
/// 環境をそのまま継承させる・C2.2）。label の有無だけを見る（選んだ経路が初回か再開かは見ない）。
///
/// **既に在る周は足さずに断る**（`s2-07l.411`）: 2 つ並べて渡すと読み手が最初の値を採り、記帳した口座と
/// 実際に走る口座がずれる。置換もしない——どちらが正かを器は決められない（C10）。label が `None`
/// （宣言 0）は従来どおり行を変えない＝器は口座を選んでおらず、launcher の値が唯一の口座。
///
/// **引数は label と置き場だけ**（`s2-07l.412`・設計 account-autonomy.md §15 (2)）: 足す口は runner と
/// lens で**この 1 関数**である。`Launch` 全体を取ると runner の材料を持たない gate から呼べず、
/// 「起動行に口座を足す」規則が 2 つに割れる。
pub(super) fn with_account(cmd: String, label: Option<&str>, state_dir: &Path) -> Result<String, LineRefusal> {
    let Some(label) = label else {
        return Ok(cmd);
    };
    if let Some(found) = account_dir_in(&cmd) {
        return Err(LineRefusal::AccountDirPresent(found));
    }
    Ok(format!("{cmd} {ACCOUNT_DIR_FLAG} {}", crate::fleet::account_dir(state_dir, label).display()))
}

/// 起動行が既に持つ口座の値（token [`ACCOUNT_DIR_FLAG`] の次の語・値の無い末尾は [`NO_VALUE`]）。
/// 持たない行は `None`。
fn account_dir_in(cmd: &str) -> Option<String> {
    let mut tokens = cmd.split_whitespace();
    tokens.find(|token| *token == ACCOUNT_DIR_FLAG)?;
    Some(tokens.next().map_or_else(|| NO_VALUE.to_owned(), str::to_owned))
}

/// runner の stdin に流す本文 = 契約の写し（再読）+ 回答済みの質問が在れば「回答」節 +
/// 途中再開なら「途中再開」節 + 追随の相手が在れば「追随」節。**順序は 契約 → 回答 → 途中再開 →
/// 追随**である（節の読み方は `headless/runner.txt` の雛形が持ち、ここは run ごとの値だけを載せる）。
fn prompt(launch: &Launch<'_>) -> String {
    let mut body = std::fs::read_to_string(contract_path(launch.state_dir, launch.run)).unwrap_or_default();
    if let Some(Question { question, answer: Some(answer), .. }) = &launch.answered {
        body.push_str(&format!("\n## 回答\n- 質問: {question}\n- 回答: {answer}\n"));
    }
    if let Some(resumed) = &launch.resumed {
        body.push_str(&format!(
            "\n## 途中再開\n- 前の turn は {} {}\n- base からの commit（worktree に在る・やり直さない）: {}\n- 未 commit の変更（worktree に在る・消さない・続きから commit する）: {}\n",
            resumed.stopped_at,
            resumed.halt.as_stop_clause(),
            item_list(&resumed.commits),
            item_list(&resumed.uncommitted)
        ));
    }
    if let Some(main) = &launch.follow {
        body.push_str(&format!("\n## 追随\n- main が {main} へ進んだ\n- `git rebase {main}` を実行し、衝突を解いて `git rebase --continue` で終える\n"));
    }
    body
}

/// 「途中再開」節の一覧（commit / 未 commit の変更・1 行 1 項目・無ければ `なし`）。
fn item_list(items: &[String]) -> String {
    if items.is_empty() {
        return "なし".to_owned();
    }
    let mut text = String::new();
    for line in items {
        text.push_str("\n  - ");
        text.push_str(line);
    }
    text
}

/// base から先の commit 数。読めない周は 0（**commit 0 は完了ではない**側へ倒れる）。
fn commit_count(worktree: &Path, base: &str) -> u64 {
    let range = format!("{base}..HEAD");
    git_line(worktree, &["rev-list", "--count", &range])
        .and_then(|text| text.parse().ok())
        .unwrap_or(0)
}

/// commit の有無まで見て段を決める。**commit 0 は完了ではない**。
fn settle(launch: &Launch<'_>, worktree: &Path, base: &str, rc: i32) -> Outcome {
    let commits = commit_count(worktree, base);
    let (stage, detail) = if rc == 0 && commits >= 1 {
        (Stage::Implemented, None)
    } else {
        (
            Stage::Failed,
            Some(format!("runner-rc:{rc},commits:{commits}")),
        )
    };
    record_stage(launch, stage, detail)
}

/// 包みが rc [`RC_RATE_LIMIT`] で終わった周: stdout の最後の停止行を読み、
/// `RunStage(RateLimited) detail=rc:<rc>,status:<status>,account:<label>` を記帳する（設計
/// account-autonomy.md §2）。label は runner を起こした口座で、器が渡していない周は
/// [`INHERITED_ACCOUNT`]（親の環境を継承した＝どの口座かを器は知らない）。
///
/// 末尾から**停止行として読める行**を探す（包めた周は箱の終端行 `confine-usage` が停止行の後ろに
/// 付くので、素の最終行を読むと常に unknown に化ける）。
///
/// **終端でない段**である（ADR-0020 §2.1）: worktree・base・commit・質問と回答の event は保つ。
/// 停止行を読めない周は `status:unknown` で、段は変えない（読めないを `Failed` に倒さない）。
fn settle_rate_limit(launch: &Launch<'_>, rc: i32, stdout: &str) -> Outcome {
    let status = stdout.lines().rev().find_map(stop_status).unwrap_or(UNKNOWN_STATUS);
    let account = launch.account.label().unwrap_or(INHERITED_ACCOUNT);
    record_stage(launch, Stage::RateLimited, Some(format!("rc:{rc},status:{status},account:{account}")))
}

/// 停止行を読めない周の status（閉じた 1 つ）。
const UNKNOWN_STATUS: &str = "unknown";

/// 包みが rc [`RC_QUESTION`] で終わった周: stdout の最終行を質問 record として読み、
/// `QuestionRaised(detail=逐語)` → `RunStage(Questioned)` の順で記帳して **rc 3 で止まる**
/// （`Blocked` と同型・設計 pipeline-question.md §3 / §4）。
///
/// record が無い・読めない周は `Failed`（`question-record-missing`・fail-closed）。record と
/// commit が同時に在る周は質問ではなく実装の失敗（runner の rc を写す）。
///
/// 数えるのは **turn で増えた commit**（`tip` 基準・ADR-0019 §2.6）である。初回の turn では
/// tip = base ゆえ `.115` の判定と同値で、起こし直しの turn では「便が base から持つ commit」を
/// 数えない——数えると、追随を解けずに質問へ倒れた turn が必ず実装の失敗に化ける。
fn settle_question(launch: &Launch<'_>, worktree: &Path, tip: &str, stdout: &str) -> Outcome {
    let commits = commit_count(worktree, tip);
    let (question, about) = match question_record(stdout) {
        Ok(found) if commits == 0 => found,
        Ok(_) => {
            return record_stage(
                launch,
                Stage::Failed,
                Some(format!("runner-rc:{RC_QUESTION},commits:{commits}")),
            )
        }
        Err(reason) => {
            return record_stage(
                launch,
                Stage::Failed,
                Some(format!("question-record-missing:{reason},commits:{commits}")),
            )
        }
    };
    let raised = emit(
        launch.state_dir,
        &Emit {
            kind: EventKind::QuestionRaised,
            run: launch.run,
            bead: launch.bead,
            stage: None,
            seat: None,
            pid: None,
            detail: Some(question.clone()),
        },
        launch.policy,
    );
    if let Err(err) = raised {
        return broken(err.to_string());
    }
    let staged = record_stage(launch, Stage::Questioned, about.map(|key| format!("about:{key}")));
    if staged.rc != 0 {
        return staged;
    }
    Outcome {
        out: vec![format!("run={} stage={} question={}", launch.run, Stage::Questioned.as_str(), launch.run)],
        err: vec![format!(
            "pipe: 質問で止まった（{question}）・`pipe answer --run {} --words \"<回答の逐語>\"`",
            launch.run
        )],
        rc: RC_BLOCKED,
    }
}

/// 質問 record（`{"question":"<1 行>","about":"<key>"}`）を stdout の最終 JSON 行から読む。
/// `question` は必須・非空・1 行。`about` は任意。
fn question_record(stdout: &str) -> Result<(String, Option<String>), String> {
    let pairs = last_json_object(stdout)?;
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(found, _)| found == key)
            .and_then(|(_, value)| value.as_str())
    };
    let question = get("question")
        .filter(|text| !text.trim().is_empty())
        .ok_or("question が無いか空である")?;
    if question.contains('\n') {
        return Err("question が 1 行でない".to_owned());
    }
    Ok((question.to_owned(), get("about").map(str::to_owned)))
}

/// 段を 1 件記帳して判定行を返す。
fn record_stage(launch: &Launch<'_>, stage: Stage, detail: Option<String>) -> Outcome {
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

/// plugin の root を run dir 配下に組み、その path を返す（設計 §5.2 手順 5 / §6）。
///
/// root の配下は 1 dir = 1 plugin で、runner が名前順に claude の `--plugin-dir` へ渡す:
/// - `<NAME>/`: **器の plugin**。binary に埋め込んだ [`EMBEDDED_PLUGIN`] を**必ず**書く＝
///   plugin を持たない consumer repo の便にも hook の in-loop guard が載る（憲法 C16.2・
///   `s2-07l.149` 裁定 (A)）。
/// - `consumer/`: worktree が**別名の** plugin を持つ周だけ（[`consumer_plugin`]）、その
///   [`PLUGIN_DIRS`] を写す。
///
/// Claude Code は**読み込んだ plugin dir の配下**を acceptEdits の自動承認から外す
/// （sensitive）。便の worktree は `<repo>/.worktrees/<NAME>/<run>` ＝ repo を
/// `--plugin-dir` に渡すと **その内側**なので、便の全 file で Edit / Write が deny される。
/// 写しを repo の外（run dir 配下）へ置くことで、「repo の plugin を載せる」意図を保った
/// まま worktree を保護対象から外す。
///
/// 写すのは **worktree の** [`PLUGIN_DIRS`]（＝便の base の内容）であって anchor の
/// 現在値ではない。
fn copy_plugin(worktree: &Path, state_dir: &Path, run: &str) -> Result<PathBuf, String> {
    let dest = plugin_path(state_dir, run);
    // 再走で古い写しが残らないよう、先に空にする。
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|err| format!("{} を空にできない: {err}", dest.display()))?;
    }
    let vessel = dest.join(NAME);
    for (dir, name, body) in EMBEDDED_PLUGIN {
        let parent = vessel.join(dir);
        std::fs::create_dir_all(&parent)
            .map_err(|err| format!("{} を作れない: {err}", parent.display()))?;
        let path = parent.join(name);
        std::fs::write(&path, body).map_err(|err| format!("{} を書けない: {err}", path.display()))?;
    }
    if consumer_plugin(worktree) {
        let consumer = dest.join(CONSUMER_DIR);
        for name in PLUGIN_DIRS {
            let from = worktree.join(name);
            // **`Path::is_dir` では判定しない**。あれは link を辿るので、`hooks` が dir への
            // symlink（例 `hooks -> ../..`）の周に「dir だ」と読んで link 先の木を丸ごと写す
            // ＝「symlink は追わない」が top-level だけ抜ける。最終要素を辿らない
            // `symlink_metadata` で見て、link なら**写さない**（fail-closed）。
            if real_dir(&from) {
                copy_tree(&from, &consumer.join(name))?;
            }
        }
    }
    Ok(dest)
}

/// worktree が **consumer の plugin** を持つか（設計 §5.2 手順 5 (ii)）。
///
/// `.claude-plugin/plugin.json` と `hooks/hooks.json` が**両方**、link を辿らずに dir の中の
/// file として在り、plugin.json の top-level の `name` が [`NAME`] と**違う**周だけ真。
/// `name` が同じ周は器自身の repo＝世代がずれていても器の 1 本だけを載せる（同じ hook を
/// 2 度走らせない）。片方だけの周・`name` が読めない周は consumer の plugin と見ない。
fn consumer_plugin(worktree: &Path) -> bool {
    let present = EMBEDDED_PLUGIN.iter().all(|(dir, file, _)| {
        let parent = worktree.join(dir);
        real_dir(&parent) && std::fs::symlink_metadata(parent.join(file)).is_ok_and(|meta| meta.is_file())
    });
    let [(manifest_dir, manifest, _), _] = EMBEDDED_PLUGIN;
    present
        && std::fs::read_to_string(worktree.join(manifest_dir).join(manifest))
            .ok()
            .and_then(|body| top_level_string(&body, "name"))
            .is_some_and(|name| name != NAME)
}

/// path が link でない dir か（最終要素を辿らない）。
fn real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
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

/// 便の worktree と base を用意する。
///
/// 初回は repo の HEAD を base にして worktree を**切る**。**再開の turn**——回答済みの質問
/// （[`Launch::answered`]）か追随（[`Launch::follow`]）か途中再開（[`Launch::resumed`]）を持つ周
/// ——は**同じ run の worktree と記録済みの base を使う**（設計 pipeline-question.md §5 /
/// pipeline-conflict.md §3 / account-autonomy.md §4: 再開は同じ便・worktree が無い / 別 branch に
/// 居る周は断る）。
fn prepare_worktree(launch: &Launch<'_>) -> Result<(PathBuf, String), String> {
    let worktree = worktree_path(launch.repo, launch.run);
    if launch.answered.is_none() && launch.follow.is_none() && launch.resumed.is_none() {
        let base = super::head_of(launch.repo)
            .ok_or_else(|| format!("{} の HEAD を読めない", launch.repo.display()))?;
        add_worktree(launch.repo, &worktree, launch.run, &base)?;
        return Ok((worktree, base));
    }
    let base = base_of_run(launch.state_dir, launch.run)
        .ok_or_else(|| format!("run {} の base を読めない", launch.run))?;
    let branch = branch_name(launch.run);
    let on_branch = git_line(&worktree, &["rev-parse", "--abbrev-ref", "HEAD"]).is_some_and(|found| found == branch);
    if !on_branch {
        return Err(format!("{} は branch {branch} の worktree でない", worktree.display()));
    }
    Ok((worktree, base))
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
///
/// 各項目は**接頭辞（`+` / `-`）を剥がした素の path** で書く（設計 contract-source.md §3・接頭辞は受付の宣言だけの
/// 文法で、guard は素の path を読む）。剥がす規則は [`refuse::normalize`] の 1 本で、dir 項目の末尾 `/` はそのまま残る
/// （guard の dir 判定は既存のまま）。
fn write_policy(worktree: &Path, write_set: &[String]) -> Result<PathBuf, String> {
    let git_dir = git_line(worktree, &["rev-parse", "--absolute-git-dir"])
        .ok_or_else(|| format!("{} の git dir を読めない", worktree.display()))?;
    let dir = PathBuf::from(git_dir).join(NAME);
    std::fs::create_dir_all(&dir).map_err(|err| format!("{} を作れない: {err}", dir.display()))?;
    let path = dir.join(WRITE_SET_FILE);
    let plain: Vec<String> = write_set.iter().map(|item| refuse::normalize(item)).collect();
    let body = format!("{}\n", plain.join("\n"));
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
