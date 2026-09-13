//! runner を起動する**唯一の関数**（設計 §5.2・FR4 / FR6・憲法 C6）。
//!
//! [`spawn`] は [`Budget`] を引数に取り、`Budget` は [`super::Precheck::measure`] を
//! 消費してしか作れない。したがって「測らずに起動する」経路は型として存在しない。
//!
//! **scribe2 固有の env は 1 つも足さない**（C2.2・ADR-0004 §2.4）。runner へは
//! 親の env をそのまま継承させ、必要な値は cmd の placeholder 置換で渡す。

use super::approve::{block, Approval, Approve, RC_BLOCKED};
use super::confine;
use super::gate::last_json_object;
use super::{
    base_of_run, branch_name, contract_path, emit, git_line, plugin_path, runner_stdout_path, vessel_path,
    worktree_path, Budget, Emit, Question, RC_QUESTION,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::LockPolicy;
use crate::fleet::{EventKind, Stage};
use crate::headless::runner::stop_status;
use crate::headless::RC_RATE_LIMIT;
use crate::name::NAME;
use crate::pipe::contract::Contract;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

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
    /// **回答済みの質問**（`Questioned` からの再 spawn だけが持つ・設計 pipeline-question.md §5）。
    /// 在る周は同じ run の worktree と base を使い、runner の stdin に「回答」節を付ける。
    pub answered: Option<Question>,
    /// **追随の相手**（main の sha・便の base が main の真の祖先である周だけ・設計
    /// pipeline-conflict.md §3）。在る周は同じ run の worktree と base を使い、runner の
    /// stdin に「追随」節を付ける。値の出所は [`super::follow::section`] ただ 1 本である。
    pub follow: Option<String>,
    /// lock の待ち方。
    pub policy: LockPolicy,
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
    let (mut command, confinement) = confine::wrap_line(&cmd, &wrap);
    // **env を 1 つも足さない**: `.env()` / `.envs()` を呼ばず親の env をそのまま継承する。
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
    let out = match child.wait_with_output() {
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
    let mut outcome = if box_killed(&confinement, rc, &stdout) {
        // **箱が溢れた周は便を終端する**（設計 gate-cost.md §4.2）。verify 行の「測れなかった」
        // とは極性が違う——便の内容が測れないのではなく、便自身が host の予約分を超えた。
        // 理由は閉じた 1 つ（`runner-rc` と同じ終端の段）で、`Failed` から resume しない。
        record_stage(launch, Stage::Failed, Some(OOM_DETAIL.to_owned()))
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
    outcome
}

/// runner の scope の unit 名に載せる段の名。
const RUNNER_STAGE: &str = "runner";

/// 箱が溢れて終端した便の理由（**閉じた 1 つ**・設計 gate-cost.md §4.2・憲法 C2）。
const OOM_DETAIL: &str = "oom-kill";

/// runner の包みが箱の中で殺されたか（設計 gate-cost.md §4.2 / §4.3）。
///
/// 根拠は包みが stdout の終端に出した `oom_kill` である。包みごと死んで終端行を出せなかった
/// 周は **signal 死**（rc が無い＝`code()` が `None` の周・器は -1 と記す）を代理にする。
/// **包めなかった周は当たらない**——素の runner が外から止められた周（`pipe stop`）を
/// 「箱が溢れた」と読まない。
fn box_killed(confinement: &confine::Confinement, rc: i32, stdout: &str) -> bool {
    confinement.confined() && (confine::read_usage(stdout).oom_kill >= 1 || rc < 0)
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

/// runner の stdin に流す本文 = 契約の写し（再読）+ 回答済みの質問が在れば「回答」節 +
/// 追随の相手が在れば「追随」節。**順序は 契約 → 回答 → 追随**である（節の読み方は
/// `headless/runner.txt` の雛形が持ち、ここは run ごとの値だけを載せる）。
fn prompt(launch: &Launch<'_>) -> String {
    let mut body = std::fs::read_to_string(contract_path(launch.state_dir, launch.run)).unwrap_or_default();
    if let Some(Question { question, answer: Some(answer), .. }) = &launch.answered {
        body.push_str(&format!("\n## 回答\n- 質問: {question}\n- 回答: {answer}\n"));
    }
    if let Some(main) = &launch.follow {
        body.push_str(&format!("\n## 追随\n- main が {main} へ進んだ\n- `git rebase {main}` を実行し、衝突を解いて `git rebase --continue` で終える\n"));
    }
    body
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
/// `RunStage(RateLimited) detail=rc:<rc>,status:<status>` を記帳する（設計 account-autonomy.md §2）。
///
/// 末尾から**停止行として読める行**を探す（包めた周は箱の終端行 `confine-usage` が停止行の後ろに
/// 付くので、素の最終行を読むと常に unknown に化ける）。
///
/// **終端でない段**である（ADR-0020 §2.1）: worktree・base・commit・質問と回答の event は保つ。
/// 停止行を読めない周は `status:unknown` で、段は変えない（読めないを `Failed` に倒さない）。
fn settle_rate_limit(launch: &Launch<'_>, rc: i32, stdout: &str) -> Outcome {
    let status = stdout.lines().rev().find_map(stop_status).unwrap_or(UNKNOWN_STATUS);
    record_stage(launch, Stage::RateLimited, Some(format!("rc:{rc},status:{status}")))
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

/// 便の worktree と base を用意する。
///
/// 初回は repo の HEAD を base にして worktree を**切る**。**再開の turn**——回答済みの質問
/// （[`Launch::answered`]）か追随（[`Launch::follow`]）を持つ周——は**同じ run の worktree と
/// 記録済みの base を使う**（設計 pipeline-question.md §5 / pipeline-conflict.md §3: 再開は
/// 同じ便・worktree が無い / 別 branch に居る周は断る）。
fn prepare_worktree(launch: &Launch<'_>) -> Result<(PathBuf, String), String> {
    let worktree = worktree_path(launch.repo, launch.run);
    if launch.answered.is_none() && launch.follow.is_none() {
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
