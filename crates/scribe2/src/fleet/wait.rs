//! 完了待ち（[`wait`]）——**唯一の待機実装**（設計 pipeline-conflict.md §3・憲法 C3.4）。
//!
//! 待つ対象は [`Completion`] のデータだけで、満たされたと判じる読み手はこの file の内側が持つ
//! （`s2-07l.260` で挙動不変に分割・外の呼び手の path は `fleet` の再 export で保つ）。

use super::{cli, replay, select, select_for_run, store, Stage};
use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

/// 待つ対象。**述語を受ける口は作らない**（C3.4: 待機は 1 実装）。
///
/// variant が運ぶのは**データ**だけである。何を読んで満たされたと判じるかは [`wait`] の
/// 内側が持つ（受付の枠なら meminfo と札の読み手・設計 gate-cost.md §3.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    /// runner の process が終わること。
    RunnerExited(u32),
    /// 席の process が消えること（TERM の後）。
    SeatGone(u32),
    /// host の受付に枠が 1 つ以上空くこと（[`crate::pipe::admission`]）。
    SlotFree {
        /// 受付札の置き場（`<state_dir の親>/<NAME>-host/slots/`）。
        slots_dir: std::path::PathBuf,
        /// この受付が要る枠（jobs の数）。
        want: u64,
        /// job 1 つが要る memory（MiB・rules 行 `gate.job_memory_mb`）。
        job_mb: u64,
        /// 席と host のために残す memory（MiB・rules 行 `host.reserve_memory_mb`）。
        reserve_mb: u64,
        /// 並列度の上限（rules 行 `gate.mutants_jobs`）。
        cap: u64,
    },
    /// process group の全員が消えること（group 宛ての TERM / KILL の後・値は group id）。
    GroupGone(u32),
    /// land の番が来ること（[`crate::pipe::land`]・設計 gate-cost.md §6）: 同じ置き場の着地待ちの列で
    /// 自分より前の便が居なくなる。列を導けない周も満たされた側である（待たずに進む・記録は land が残す）。
    LandTurn {
        /// event log の置き場（列は replay から導く・別の状態 file を持たない）。
        state_dir: std::path::PathBuf,
        /// 待つ便の id。
        run: String,
    },
    /// 便用の口座が 1 つ空くこと（設計 account-autonomy.md §4・ADR-0020 §2.3）: 置き場の最新の実測行で
    /// §3 の便用の規則を再評価して `Chosen` になる。deadline は呼び手が `reset_at` から計算する
    /// （rules 行ではない・縮退を持たない）。
    AccountFree {
        /// 当たっている口座が開き直る最も早い時刻（`YYYY-MM-DDTHH:MM:SSZ`・deadline の出所）。
        reset_at: String,
        /// 実測行の置き場（`SlotFree` が `slots_dir` を運ぶのと同型）。
        state_dir: std::path::PathBuf,
        /// 便の repo（`pipe run --repo` の値・`Turn.repo`）。便用の除外はこの repo を anchor に持つ席の口座だけ
        /// （設計 account-autonomy.md §14）＝観測の再評価も選定と同じ repo で除外する（C3.4）。
        repo: std::path::PathBuf,
        /// 待つ便の id。**便が [`expected`](Self::AccountFree::expected) の段でなくなった周は満たされた側**
        /// （`pipe stop --run` が待ちの途中の便を終端した周に待ちから抜ける・呼び手が段を読み直す）。
        run: String,
        /// 待ちの間、便が居るはずの段（`RateLimited` の再開なら `RateLimited`・初回の起動なら `Reviewed` /
        /// `Blocked` / `Questioned`・衝突の起こし直しなら `Implemented`・設計 account-autonomy.md §4）。
        /// 呼び手が自分の段を運ぶ＝この variant は段を決め打たない（初回の起動の待ちが busy loop に化けない）。
        expected: Stage,
        /// manifest の `[[account]]` の label 列（宣言値・置き場は持たないので運ぶ）。
        labels: Vec<String>,
        /// 便が使う model（rules 行 `runner.model` の値・字面のまま運び [`select_for_run`] へ渡す・`s2-07l.297`）。
        model: Option<String>,
    },
    /// **CI の判定が出ること**（`pipe land` の終端・設計 contract-source.md §5）: forge の CLI を子 process で
    /// 撃ち、着地した commit の run が**終端の判定**（success / failure）に達する。まだ走っている周・
    /// run が 1 本も無い周・読めない周は満たされない（deadline まで待つ）。判定そのものは呼び手が
    /// [`ci_now`] で読み直す（`LandTurn` と同型＝待ちは「解けたか」だけを答える）。
    CiResult {
        /// CI の行を撃つ作業 dir（対象 repo）。
        repo: std::path::PathBuf,
        /// **着地した commit の 40 桁の sha**（行の `{sha}` の穴に入る）。短縮 sha を渡すと forge の CLI は
        /// 完了済みの run でも空を返し続け、待ちが上限まで空回りする（実測の罠）。
        sha: String,
        /// 判定を読む 1 行（宣言 `ci-cmd` か既定・`{sha}` の穴を持つ）。
        cmd: String,
    },
}

impl Completion {
    /// 見張る pid。**pid を見張らない variant（[`Self::SlotFree`] / [`Self::LandTurn`] /
    /// [`Self::AccountFree`] / [`Self::CiResult`]）は 0**——pid 0 は `/proc/0` を持たない（user の process に振られない）ので、
    /// 生きている pid と取り違えない。[`Self::GroupGone`] は group id（= group leader の pid）を返す。
    pub fn pid(&self) -> u32 {
        match *self {
            Self::RunnerExited(pid) | Self::SeatGone(pid) | Self::GroupGone(pid) => pid,
            Self::SlotFree { .. } | Self::LandTurn { .. } | Self::AccountFree { .. } | Self::CiResult { .. } => 0,
        }
    }

    /// 満たされたか（1 周分の観測・前回の観測を持たない周）。
    fn is_met(&self) -> bool {
        match self {
            Self::RunnerExited(pid) | Self::SeatGone(pid) => !pid_is_live(*pid),
            Self::GroupGone(group) => !group_is_live(*group),
            Self::SlotFree { slots_dir, want, job_mb, reserve_mb, cap } => {
                crate::pipe::admission::has_room(
                    slots_dir,
                    (*want).min(*cap),
                    crate::pipe::admission::Sizes { job_mb: *job_mb, reserve_mb: *reserve_mb },
                )
            }
            Self::LandTurn { .. } => self.round(None).met,
            Self::CiResult { repo, sha, cmd } => ci_now(repo, sha, cmd).is_some(),
            Self::AccountFree { state_dir, repo, run, expected, labels, model, .. } => {
                account_free(state_dir, run, *expected, &RunSelect { repo, labels, model: model.as_deref() })
            }
        }
    }

    /// 1 周分の観測（[`wait`] の loop が周ごとに撃つ口・前回の観測を受けて今の観測を返す）。
    ///
    /// **[`Self::LandTurn`] だけが印を持つ**（設計 fleet-event-log.md §4「着地の列の待ちの費用」）: 列の材料の
    /// 印を**先に**取り（[`mark_of`]）、前回の観測と印が同じ周は replay を省いて前回の判定を使う
    /// （[`reuse`]）。違う周・印を取れない周は印を [`observe`] へ渡して読み直す。他の variant は印なし
    /// （`None`）で毎周そのまま評価する（meminfo / 実測行 / pid の生存は不変・C3.4 の 1 実装のまま）。
    fn round(&self, last: Option<Glance>) -> Glance {
        let Self::LandTurn { state_dir, run } = self else {
            return Glance { mark: None, met: self.is_met() };
        };
        let mark = mark_of(state_dir);
        match reuse(last.as_ref(), mark.as_ref()) {
            Some(met) => Glance { mark, met },
            None => observe(mark, state_dir, run),
        }
    }
}

/// CI の run 1 本が着いた**終端の判定**（**閉じた 2 値**・設計 contract-source.md §5）。
///
/// 「まだ出ていない」はこの型に入れない（[`ci_now`] が `None` で返す）——走っている run を
/// `Failure` に畳むと、待つ前に close しない側へ倒れて上限の意味が消える（C10）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiRun {
    /// 完了して success。
    Success,
    /// 完了して success でない（failure / cancelled / timed_out …）。
    Failure,
}

/// forge の CLI が `--json status,conclusion` で返す key（字面は forge のもの）。
const CI_STATUS: &str = "status";

/// 同上（判定の key）。
const CI_CONCLUSION: &str = "conclusion";

/// 完了した run の `status` の字面。
const CI_COMPLETED: &str = "completed";

/// 成功した run の `conclusion` の字面。
const CI_SUCCESS: &str = "success";

/// CI の判定を**1 回だけ**読む（子 process 1 回・設計 contract-source.md §5）。
///
/// 返すのは 3 形である: `Some(Failure)`（**完了した run に success でないものが 1 本以上在る**・他の run が
/// まだ走っていても待たない）・`Some(Success)`（run が 1 本以上在り、落ちた run が無く全部が完了している）・
/// `None`（run が 0 本・落ちた run は無いがまだ走っている run が在る・行を撃てない・JSON を読めない）。**`None` を「成功していない」と読まない**のは
/// 呼び手の側で、`None` は「まだ測れていない」である（C10）。
///
/// 行は **argv 1 本として撃つ**（shell を通さない）。宣言 `ci-cmd` は対象 repo の tracked file から来るので、
/// shell に渡すと宣言 1 行が別の command を継ぎ足せる（契約の verify 行と同じ線）。
pub fn ci_now(repo: &Path, sha: &str, cmd: &str) -> Option<CiRun> {
    let line = cmd.replace(crate::pipe::declaration::CI_SHA_HOLE, sha);
    let mut words = line.split_whitespace();
    let head = words.next()?;
    let out = Command::new(head).args(words).current_dir(repo).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let tree = crate::fleet::json_tree::parse(&String::from_utf8_lossy(&out.stdout)).ok()?;
    let runs = tree.as_array()?;
    if runs.is_empty() {
        return None;
    }
    let status_of = |run: &crate::fleet::json_tree::Tree| {
        run.get(CI_STATUS).and_then(crate::fleet::json_tree::Tree::as_str).map(str::to_owned)
    };
    let failed = |run: &crate::fleet::json_tree::Tree| {
        status_of(run).as_deref() == Some(CI_COMPLETED)
            && run.get(CI_CONCLUSION).and_then(crate::fleet::json_tree::Tree::as_str) != Some(CI_SUCCESS)
    };
    // **落ちた run を先に見る**。実 CI では複数の workflow が並ぶので、1 本が落ちた後も別の 1 本が
    // 走っていることが常態である。未完了を先に見ると、**測って落ちた事実**が上限いっぱい待った末の
    // 「測れていない」に化ける（C10 の反転）。落ちたと分かった時点で待つ理由は無い。
    if runs.iter().any(failed) {
        return Some(CiRun::Failure);
    }
    // 落ちた run が 1 本も無い周は、**全部が完了している**ときだけ success と言える
    // （走っている run を成功に数えない）。
    if runs.iter().any(|run| status_of(run).as_deref() != Some(CI_COMPLETED)) {
        return None;
    }
    Some(CiRun::Success)
}

/// file 1 本の印（長さ・mtime・inode・metadata だけで中身を parse しない）。
///
/// inode を含めるのは、器の atomic な書き（`.partial` → rename）が inode を必ず変えるので、同じ byte 数で
/// mtime の粒度が粗い file system でも印が動くためである（壁時計の粒度に賭けない）。
type FileMark = (u64, SystemTime, u64);

/// 着地の列の材料の印（設計 fleet-event-log.md §4）: event log と `<state_dir>/pipe/*/verdict.json`
/// （run id の辞書順）それぞれの [`FileMark`]。列の中身は verdict で決まる（gate-cost.md §6.1）ので
/// verdict.json も材料である。worktree の実在（retire）は必ず event を伴うので log の印で足りる。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mark {
    /// event log の印。
    log: FileMark,
    /// 便ごとの `verdict.json` の印（無い便は載らない＝現れれば印が動く）。
    verdicts: BTreeMap<String, FileMark>,
}

/// 1 周分の観測（[`wait`] の loop の**局所状態**・[`Completion`] には持たせない）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Glance {
    /// 観測の前に取った列の材料の印（印を持たない variant・取れない周は `None`）。
    mark: Option<Mark>,
    /// 満たされたか。
    met: bool,
}

/// file 1 本の印を取る。**無い file は `Ok(None)`**（印の値の 1 つ）・metadata を読めない周は `Err`。
fn file_mark(path: &Path) -> std::io::Result<Option<FileMark>> {
    let meta = match std::fs::metadata(path) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    Ok(Some((meta.len(), meta.modified()?, meta.ino())))
}

/// 列の材料の印を取る（**replay の前に呼ぶ**・その値を [`observe`] へ渡す＝順序は引数の依存で固定）。
///
/// path は `store::events_path` と `pipe::verdict_path` の 1 本ずつから取る（新しい path 定数を書かない）。
/// event log が無い・`pipe/` の dir を列挙できない・metadata を読めない周は **`None`＝必ず読み直す側**
/// （fail-closed・費用を払って正しさを取る）。verdict.json の無い便は列に載らない（現れれば印が動く）。
fn mark_of(state_dir: &Path) -> Option<Mark> {
    let log = file_mark(&store::events_path(state_dir)).ok()??;
    let mut verdicts = BTreeMap::new();
    for entry in std::fs::read_dir(state_dir.join(crate::pipe::DIR)).ok()? {
        let id = entry.ok()?.file_name().into_string().ok()?;
        if let Some(found) = file_mark(&crate::pipe::verdict_path(state_dir, &id)).ok()? {
            verdicts.insert(id, found);
        }
    }
    Some(Mark { log, verdicts })
}

/// 前回の判定を使い回せるか（**pure**・判定はこの 1 本）: 前回の観測と今の印が**両方在って等しい**周だけ
/// 前回の `met`。どちらかが無い・違う周は `None`＝読み直す。
fn reuse(last: Option<&Glance>, now: Option<&Mark>) -> Option<bool> {
    let (last, now) = (last?, now?);
    (last.mark.as_ref()? == now).then_some(last.met)
}

/// [`Completion::LandTurn`] を replay で観測する（印は呼び手が**先に**取って渡す）。
///
/// replay の間に file が変わった周は次の周の印が違うので必ず読み直す（§4「材料が変われば必ず読み直す」）。
fn observe(mark: Option<Mark>, state_dir: &Path, run: &str) -> Glance {
    #[cfg(test)]
    tests::REPLAYS.with(|count| count.set(count.get() + 1));
    let met = !matches!(crate::pipe::land::turn_now(state_dir, run), crate::pipe::land::Turn::After(_));
    Glance { mark, met }
}

/// [`Completion::AccountFree`] が運ぶ便用の選定の入力のうち、[`select_for_run`] へ**そのまま**渡すもの
/// （置き場と時刻以外・待ちの観測と選定が同じ入力で除外する・C3.4）。
struct RunSelect<'a> {
    /// 便の repo（除外はこの repo を anchor に持つ席の口座だけ・設計 account-autonomy.md §14）。
    repo: &'a Path,
    /// 宣言の label 列。
    labels: &'a [String],
    /// 便が使う model（字面のまま）。
    model: Option<&'a str>,
}

/// [`Completion::AccountFree`] の 1 周分の観測。
///
/// 置き場を replay し、便がまだ `expected` の段なら最新の実測行で便用の規則（[`select_for_run`]・除外は便の
/// `repo` を anchor に持つ席の口座だけ）を再評価して `Chosen` の周だけ満たされる。便が `expected` の段でなくなった周
/// （stop で終端した・別の process が起こし直した）は**満たされた側**＝待ち続ける理由が無い。置き場を読めない周は
/// 満たされない（読めなさで起こし直さない・期限で Timeout に倒れて計測から撃ち直す）。
fn account_free(state_dir: &Path, run: &str, expected: Stage, select: &RunSelect<'_>) -> bool {
    let Ok(events) = store::read_all(state_dir) else {
        return false;
    };
    let state = replay(&events);
    if state.runs.get(run).map(|found| found.stage) != Some(expected) {
        return true;
    }
    matches!(
        select_for_run(&state, select.repo, select.labels, select.model, &cli::now_utc()),
        select::Selection::Chosen(_)
    )
}

/// `YYYY-MM-DDTHH:MM:SSZ` を UNIX 秒にする（[`cli::format_utc`] の逆・**それ以外の形は `None`**）。
///
/// 待ちの deadline を reset 時刻から計算する読み手である。数の読み替えを持たない（形が違う字面を
/// 0 秒にしない＝呼び手は `None` を「待つ時刻が無い」と読む）。
pub fn epoch_of(ts: &str) -> Option<u64> {
    let shape = b"0000-00-00T00:00:00Z";
    let bytes = ts.as_bytes();
    let fits = bytes.len() == shape.len()
        && bytes.iter().zip(shape.iter()).all(|(found, want)| match want {
            b'0' => found.is_ascii_digit(),
            _ => found == want,
        });
    if !fits {
        return None;
    }
    let num = |from: usize, len: usize| ts.get(from..from + len)?.parse::<u64>().ok();
    let (year, month, day) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (hour, minute, second) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    // 暦の (年, 月, 日) を 1970-01-01 からの日数にする（`cli::civil_from_days` の逆・chrono を足さない）。
    let shifted_year = if month <= 2 { year.checked_sub(1)? } else { year };
    let era = shifted_year / 400;
    let yoe = shifted_year - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = (era * 146_097 + doe).checked_sub(719_468)?;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// 期限までに終わらなかった。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeout;

/// 待つ間隔。
const POLL: Duration = Duration::from_millis(20);

/// [`Completion`] が満たされるまで待つ。**これが唯一の待機実装である**。
///
/// process の生存は `/proc/<pid>` の有無で見る（libc を足さないため・NFR3）。受付の枠は
/// 周ごとに meminfo と札を読み直す（周期はこの [`POLL`] のまま・上限は呼び手の期限）。着地の列は
/// 前回の観測（[`Glance`]・loop の局所状態）を次の周へ渡し、材料の印が変わらない周は replay を省く。
pub fn wait(completion: Completion, deadline: Duration) -> Result<(), Timeout> {
    let started = Instant::now();
    let mut last = None;
    loop {
        let now = completion.round(last);
        if now.met {
            return Ok(());
        }
        if started.elapsed() >= deadline {
            return Err(Timeout);
        }
        last = Some(now);
        std::thread::sleep(POLL);
    }
}

/// pid が生きているか。
fn pid_is_live(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// pgid が `group` の process が `/proc` に 1 つでも在るか（zombie も数える＝回収されるまで在る）。
///
/// **`/proc` を読めない周は「在る」**（消えたと測れていないものを消えたにしない・fail-closed）。
/// 読む間に消えた process の stat は読めないので飛ばす。
fn group_is_live(group: u32) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return true;
    };
    entries.flatten().any(|entry| {
        let numeric = entry.file_name().to_str().is_some_and(|name| name.bytes().all(|b| b.is_ascii_digit()));
        numeric
            && std::fs::read_to_string(entry.path().join("stat"))
                .ok()
                .and_then(|text| pgid_of(&text))
                == Some(group)
    })
}

/// `/proc/<pid>/stat` の 1 行から pgid（第 5 欄）を読む（pure）。
///
/// `comm` は空白も `)` も含みうるので、**最後の `)`** の後ろから数える（state・ppid・pgrp の順）。
/// 欄が足りない行・数でない欄は `None`。
fn pgid_of(stat_text: &str) -> Option<u32> {
    let (_, rest) = stat_text.rsplit_once(')')?;
    rest.split_whitespace().nth(2)?.parse().ok()
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.260
    use super::{cli::format_utc, epoch_of, mark_of, pgid_of, reuse, store, wait, Completion, Glance, Mark, Timeout};
    use crate::fleet::{EventKind, Stage};
    use crate::pipe::fixture::{append_all, event, gated_run, scratch};
    use crate::pipe::verdict_path;
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, UNIX_EPOCH};

    thread_local! {
        /// [`super::observe`] が replay を撃った回数（歯だけの counter・`wait` は同期で別 thread を起こさないので
        /// loop と歯は同じ thread＝同一 process で歯が並んでも干渉しない）。
        pub(super) static REPLAYS: Cell<usize> = const { Cell::new(0) };
    }

    /// 印の fixture（`log` の秒と `a-front` の verdict の秒だけを呼び手が選ぶ）。
    fn mark(log_secs: u64, verdict_secs: u64) -> Mark {
        let at = |secs: u64| UNIX_EPOCH + Duration::from_secs(secs);
        Mark {
            log: (10, at(log_secs), 7),
            verdicts: BTreeMap::from([("a-front".to_owned(), (19, at(verdict_secs), 8))]),
        }
    }

    /// 着地待ちの列の fixture（`Gated(PASS)` の便 2 本・後ろの便 `b-me` が前の便 `a-front` を待つ）。
    fn queue_fixture(name: &str) -> (PathBuf, PathBuf) {
        let root = scratch(name);
        let (state, repo) = (root.join("state"), root.join("repo"));
        ["a-front", "b-me"].iter().for_each(|run| gated_run(&state, &repo, run, "PASS"));
        REPLAYS.with(|count| count.set(0));
        (root, state)
    }

    /// 後ろの便 `b-me` の待ち。
    fn land_turn(state: &Path) -> Completion {
        Completion::LandTurn { state_dir: state.to_path_buf(), run: "b-me".to_owned() }
    }

    /// (a) 前回の観測と今の印が両方在って等しい周だけ前回の `met` を返す。
    #[test]
    fn fleet_wait_land_turn_reuses_verdict_when_stamp_unchanged() {
        let waiting = Glance { mark: Some(mark(1, 2)), met: false };
        assert_eq!(reuse(Some(&waiting), Some(&mark(1, 2))), Some(false), "待ち続ける判定を使い回す");
        let met = Glance { mark: Some(mark(1, 2)), met: true };
        assert_eq!(reuse(Some(&met), Some(&mark(1, 2))), Some(true), "前回の met をそのまま返す");
    }

    /// (b) len / mtime / inode / verdict の列のどれかが違う周は `None`＝読み直す。
    #[test]
    fn fleet_wait_land_turn_rereads_when_stamp_changes() {
        let last = Glance { mark: Some(mark(1, 2)), met: false };
        let mut longer = mark(1, 2);
        longer.log.0 += 1;
        assert_eq!(reuse(Some(&last), Some(&longer)), None, "log の len");
        assert_eq!(reuse(Some(&last), Some(&mark(3, 2))), None, "log の mtime");
        let mut relinked = mark(1, 2);
        relinked.log.2 += 1;
        assert_eq!(reuse(Some(&last), Some(&relinked)), None, "log の inode");
        assert_eq!(reuse(Some(&last), Some(&mark(1, 4))), None, "verdict の mtime");
        let mut rewritten = mark(1, 2);
        rewritten.verdicts.insert("a-front".to_owned(), (19, UNIX_EPOCH + Duration::from_secs(2), 9));
        assert_eq!(reuse(Some(&last), Some(&rewritten)), None, "verdict の inode（同じ byte 数・同じ mtime）");
        let mut appeared = mark(1, 2);
        appeared.verdicts.insert("b-me".to_owned(), (19, UNIX_EPOCH, 10));
        assert_eq!(reuse(Some(&last), Some(&appeared)), None, "verdict が現れた");
    }

    /// (c) 前回か今の印が `None` の周は `None`（fail-closed の pin・印を取れない周は必ず読み直す）。
    #[test]
    fn fleet_wait_land_turn_rereads_when_stamp_missing() {
        let last = Glance { mark: Some(mark(1, 2)), met: false };
        assert_eq!(reuse(None, Some(&mark(1, 2))), None, "前回の観測が無い");
        assert_eq!(reuse(Some(&last), None), None, "今の印を取れない");
        let unmarked = Glance { mark: None, met: false };
        assert_eq!(reuse(Some(&unmarked), Some(&mark(1, 2))), None, "前回の印を取れていない");
        assert_eq!(reuse(None, None), None);
    }

    /// (d) 実 file の印: log の len は file の byte 数・append 後に len が増え印が変わる。log か `pipe/` の
    /// dir が無い周は `None`（壁時計の等号は pin しない）。
    #[test]
    fn fleet_wait_land_turn_stamp_reads_len_and_mtime() {
        let state = scratch("wait-stamp").join("state");
        assert_eq!(mark_of(&state), None, "log が無い");
        append_all(&state, &[event("r", EventKind::RunStage, Some(Stage::Implemented), None, None)]);
        assert_eq!(mark_of(&state), None, "pipe/ の dir を列挙できない");
        std::fs::create_dir_all(state.join(crate::pipe::DIR)).expect("pipe/ を作れる");
        let first = mark_of(&state).expect("印を取れる");
        let len = std::fs::metadata(store::events_path(&state)).expect("log の metadata").len();
        assert_eq!(first.log.0, len, "len は file の byte 数");
        assert!(first.verdicts.is_empty(), "verdict の無い置き場");
        append_all(&state, &[event("r", EventKind::RunStage, Some(Stage::Gated), None, None)]);
        let second = mark_of(&state).expect("印を取れる");
        assert!(second.log.0 > first.log.0, "append で len が増える");
        assert_ne!(second, first, "印が変わる");
        let _ = std::fs::remove_dir_all(state.parent().expect("scratch の root"));
    }

    /// (e) 配線の pin: 印が同じ周は前回の観測を使い replay しない。log を同じ byte 数の壊れた内容で上書きし
    /// mtime を戻す（inode も同じ＝印は不変）と、replay していれば malformed → `Unmeasurable` → met になる。
    #[test]
    fn fleet_wait_land_turn_reuses_last_observation_without_replay() {
        let (root, state) = queue_fixture("wait-reuse");
        let turn = land_turn(&state);
        let first = turn.round(None);
        assert!(!first.met, "後ろの便は前の便を待つ");
        assert!(first.mark.is_some(), "印を取れた");
        let log = store::events_path(&state);
        let bytes = std::fs::read(&log).expect("log を読める").len();
        let modified = std::fs::metadata(&log).expect("log の metadata").modified().expect("mtime");
        std::fs::write(&log, vec![b'x'; bytes]).expect("同じ byte 数の壊れた内容で上書きできる");
        std::fs::File::options().write(true).open(&log).and_then(|file| file.set_modified(modified)).expect("mtime を戻せる");
        assert_eq!(mark_of(&state), first.mark, "印は不変");
        assert!(!turn.round(Some(first)).met, "印が同じ周は前回の判定のまま（replay しない）");
        assert!(turn.round(None).met, "負例の対: 前回の観測が無い周は読み直し、壊れた log は Unmeasurable＝met");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// (f) loop が前回の観測を次の周へ渡す: 変化しない正常な store で `wait` を撃つと Timeout ∧ replay は 1 周目の
    /// 1 回だけ（周回数は `POLL` 20 ms × 200 ms で ≥ 2 の下限だけを前提にし、等号は counter にしか置かない）。
    #[test]
    fn fleet_wait_land_turn_wait_loop_carries_the_observation() {
        let (root, state) = queue_fixture("wait-carry");
        assert_eq!(wait(land_turn(&state), Duration::from_millis(200)), Err(Timeout), "前の便が居るまま上限");
        assert_eq!(REPLAYS.with(Cell::get), 1, "1 周目だけ replay し、以後は前回の観測を再利用する");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// (g) 印が違えば読み直す: 正常な event 1 行を append（len が変わる）した次の周は前回の観測を渡されても
    /// replay する（counter == 2 ∧ 印が違う）。
    #[test]
    fn fleet_wait_land_turn_rereads_when_log_grows() {
        let (root, state) = queue_fixture("wait-grow");
        let turn = land_turn(&state);
        let first = turn.round(None);
        assert!(!first.met && first.mark.is_some(), "1 周目: 待つ ∧ 印を取れた");
        assert_eq!(REPLAYS.with(Cell::get), 1, "1 周目は replay");
        append_all(&state, &[event("c-late", EventKind::RunStage, Some(Stage::Implemented), None, None)]);
        let second = turn.round(Some(first.clone()));
        assert_eq!(REPLAYS.with(Cell::get), 2, "印が違う周は前回の観測を渡されても replay");
        assert_ne!(second.mark, first.mark, "2 周目の印は 1 周目と違う");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// (h) event log は不変のまま前の便の verdict.json を `.partial` → rename で FAIL に書き換える（同じ byte 数・
    /// inode が変わる）と、次の周は読み直して met（verdict を印に含めない・inode を見ない実装は RED）。
    #[test]
    fn fleet_wait_land_turn_rereads_when_a_verdict_changes() {
        let (root, state) = queue_fixture("wait-verdict");
        let turn = land_turn(&state);
        let first = turn.round(None);
        assert!(!first.met, "前の便が PASS の間は待つ");
        let front = verdict_path(&state, "a-front");
        let bytes = std::fs::read(&front).expect("判定を読める").len();
        let partial = front.with_extension("json.partial");
        let fail = "{\"verdict\":\"FAIL\"}\n";
        assert_eq!(fail.len(), bytes, "同じ byte 数で書き換える");
        std::fs::write(&partial, fail).and_then(|()| std::fs::rename(&partial, &front)).expect("判定を書き換えられる");
        let second = turn.round(Some(first.clone()));
        assert!(second.met, "前の便が FAIL に外れた周は読み直して met");
        assert_ne!(second.mark, first.mark, "verdict の inode で印が動く");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `epoch_of` は `format_utc` の逆で、形の違う字面は `None`（0 秒に読み替えない）。
    #[test]
    fn pipe_ratelimit_resume_epoch_of_inverts_format_utc() {
        for secs in [0_u64, 951_782_400, 1_789_000_000, 4_102_444_800, 1_709_251_199] {
            let text = format_utc(secs);
            assert_eq!(epoch_of(&text), Some(secs), "{text}");
        }
        assert_eq!(epoch_of("2026-09-13T06:00:00Z"), Some(1_789_279_200));
        for bad in [
            "2026-09-13T06:00:00",
            "2026-09-13T06:00:00+00:00",
            "2026-13-13T06:00:00Z",
            "2026-09-13T24:00:00Z",
            "2026-09-13 06:00:00Z",
            "",
            "-",
        ] {
            assert_eq!(epoch_of(bad), None, "{bad:?}");
        }
    }

    /// `AccountFree` は pid を見張らない（0）。
    #[test]
    fn pipe_ratelimit_resume_account_free_watches_no_pid() {
        let found = Completion::AccountFree {
            reset_at: "2026-09-13T06:00:00Z".to_owned(),
            state_dir: std::path::PathBuf::from("state"),
            repo: std::path::PathBuf::from("repo"),
            run: "r".to_owned(),
            expected: Stage::RateLimited,
            labels: Vec::new(),
            model: Some("opus".to_owned()),
        };
        assert_eq!(found.pid(), 0);
    }

    #[test]
    fn pipe_stop_group_pgid_of_reads_the_fifth_field() {
        assert_eq!(pgid_of("4242 (sleep) S 4200 4100 4100 0 -1 4194560 91 0"), Some(4100), "通常の comm");
        assert_eq!(pgid_of("4242 (Web Content) S 4200 777 777 0 -1"), Some(777), "空白入り comm");
        assert_eq!(pgid_of("4242 (a) S 1 2 (b)) R 9 31 32 0"), Some(31), "`)` 入り comm は最後の `)` から数える");
        assert_eq!(pgid_of("4242 (sleep) S 4200"), None, "欄が足りない");
        assert_eq!(pgid_of("4242 (sleep) S 4200 x 1"), None, "数でない欄");
        assert_eq!(pgid_of("4242 sleep S 4200 4100"), None, "comm の閉じが無い");
    }

    #[test]
    fn pipe_stop_group_completion_pid_is_the_group_id() {
        assert_eq!(Completion::GroupGone(31337).pid(), 31337);
    }

    /// wait の網羅 match が新 variant を含む（variant を足したら compile で気付く形の歯）。
    #[test]
    fn pipe_stop_group_completion_match_is_exhaustive() {
        let all = [
            Completion::RunnerExited(7),
            Completion::SeatGone(8),
            Completion::SlotFree {
                slots_dir: std::path::PathBuf::from("slots"),
                want: 1,
                job_mb: 1,
                reserve_mb: 1,
                cap: 1,
            },
            Completion::GroupGone(9),
            Completion::LandTurn { state_dir: std::path::PathBuf::from("state"), run: "r".to_owned() },
            Completion::AccountFree {
                reset_at: "2026-09-13T06:00:00Z".to_owned(),
                state_dir: std::path::PathBuf::from("state"),
                repo: std::path::PathBuf::from("repo"),
                run: "r".to_owned(),
                expected: Stage::RateLimited,
                labels: Vec::new(),
                model: None,
            },
            Completion::CiResult {
                repo: std::path::PathBuf::from("repo"),
                sha: "0".repeat(40),
                cmd: "true {sha}".to_owned(),
            },
        ];
        let names: Vec<&str> = all
            .iter()
            .map(|found| match found {
                Completion::RunnerExited(_) => "RunnerExited",
                Completion::SeatGone(_) => "SeatGone",
                Completion::SlotFree { .. } => "SlotFree",
                Completion::GroupGone(_) => "GroupGone",
                Completion::LandTurn { .. } => "LandTurn",
                Completion::AccountFree { .. } => "AccountFree",
                Completion::CiResult { .. } => "CiResult",
            })
            .collect();
        assert_eq!(
            names,
            ["RunnerExited", "SeatGone", "SlotFree", "GroupGone", "LandTurn", "AccountFree", "CiResult"],
            "宣言順の末尾に CiResult"
        );
    }
}
