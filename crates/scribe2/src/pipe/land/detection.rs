//! 着地後の検出の口（`pipe land --run <id> --detection-only`・設計 docs/design/gate-cost.md §44 行 ak・ADR-0060・
//! `s2-07l.465`）。
//!
//! `Landed` の便の着地した commit（記録の `sha:`）を主実測と同じ置き場の別名へ detach で出し、凍結した宣言の写しの
//! 検出線の行**だけ**を gate と同じ受付札・縮退・箱・遮断器の行の手（[`run_detection_admitted`]）で 1 回撃つ。`{base}` は
//! 着地した commit の親・`{teeth}` は契約の写しから・的と純移動の付け足しと写しの周の番号は gate と同じ 1 本
//! （[`aimed_lines`] / [`population_lines`] / [`keep_detection`] と周の番号）を置き場と便 id の対で呼ぶ（C2）。
//!
//! 残すのは 3 つ: 便自身の `verify-main.jsonl` に `landed` 付きの record（撃った周は行ごとに 1 本・撃たなかった周と
//! 撃てなかった周は理由を持つ 1 本＝どの周も 1 本以上）、§15 の置き場の写しと理由の file、`RunDone stage=Landed` の
//! detail の `detection:<語>` 1 件。**台帳 client は呼ばない**（C15・FR50）。gate と land の段は変えない。口は land が
//! `Landed` の後に子 process で起こし（[`super::finish`]・待たない・設計 §44 形 (11)）、人も同じ口を撃てる。
//!
//! 極性: 検出線は止めない線（C12.4）なので、測れなかった周も rc 0 で語が `unmeasured` になる。rc 2 は record・写し・
//! event を書けない周だけ。宣言に検出線の行が無い便は何も書かずに rc 1 で断る。

mod origin;

use self::origin::Origin;
use super::super::closure::teeth_words;
use super::super::contract::Contract;
use super::super::declaration::{Effective, RootsAtHead};
use super::super::gate::{
    aimed_lines, keep_detection, keep_reason, landed_step_record, landed_unfired_record, next_copy_dir, next_number,
    population_lines, recorded_rc, run_detection_admitted, Admit, Checks, LandedMark, Limits, Record, Step, Unfired,
};
use super::super::{contract_path, emit, git_bytes, git_line, git_ok, verify_log_path, vessel_path, Emit};
use super::finish::SHA_PREFIX;
use super::verify::{check_path, MAIN_UNKNOWN, VERIFY_MAIN_FILE, VERIFY_MAIN_STDERR_FILE};
use super::{broken, nul_paths, refused, scope_touched, RUN_TRAILER};
use crate::cli_outcome::Outcome;
use crate::fleet::store::{append_line, read_all, LockPolicy};
use crate::fleet::{EventKind, Stage};
use crate::rules::manifest::Manifest;
use std::path::{Path, PathBuf};

/// 着地後の検出 1 回の材料（`pipe land --detection-only` の手が組む・着地の材料 [`super::Land`] の部分集合）。
pub(in crate::pipe) struct Detect<'a> {
    /// 便 id。
    pub(in crate::pipe) run: &'a str,
    /// 契約の bead id（event に載る）。
    pub(in crate::pipe) bead: &'a str,
    /// 対象 repo（着地した commit を出す）。
    pub(in crate::pipe) repo: &'a Path,
    /// 置き場。
    pub(in crate::pipe) state_dir: &'a Path,
    /// 契約の写し（`{teeth}` の出所）。
    pub(in crate::pipe) contract: &'a Contract,
    /// 規則から読んだ線（受付札と遮断器の材料・gate と同じ 1 本）。
    pub(in crate::pipe) limits: Limits,
    /// lock の待ち方。
    pub(in crate::pipe) policy: LockPolicy,
    /// 検出線を起こす間隔の下限（rules 行 `detection.daily_min_s` の値・設計 gate-cost.md §50）。行を読めない周は無し＝
    /// 日次の検出でない今の形（起点を読まず書かず・base は親・record と写しに `since` と `runs` を足さない）。
    pub(in crate::pipe) daily: Option<u64>,
}

/// 日次の検出の下限を rules から読む（行を読めない周は `None`・設計 gate-cost.md §50 形 (11)）。読み手は 1 本。
pub(in crate::pipe) fn daily_floor(manifest: &Manifest) -> Option<u64> {
    origin::daily_min_s(manifest).ok()
}

/// 置き場の別名の接尾辞（主実測の tmp `verify/<id>` と重ねない・`verify/<id>-detection`）。
const PLACE_SUFFIX: &str = "-detection";

/// `RunDone stage=Landed` の detail の前置き（`sha:` も `terminal:` も持たない＝着地と終端の読み手は読み飛ばす）。
pub(super) const DETAIL_HEAD: &str = "detection:";

/// land が口を子 process で起こせた周の detail の語（設計 §44 形 (11)・終えた語ではない＝子が後から終えた語を記す）。
pub(super) const SPAWNED: &str = "spawned";

/// worktree か親を出せなかった周の理由の語。
const UNPREPARED: &str = "unprepared";

/// 遮断器が閉じて撃たなかった周の理由の語。
const HOST_CLOSED: &str = "host-closed";

/// land が口を子 process で起こせなかった周の理由の語（detail の語も同じ字面・設計 §44 形 (11)）。
pub(super) const UNSPAWNED: &str = "unspawned";

/// 日次の検出が下限の内で口を起こさなかった周の語（detail の語も同じ字面・設計 §50 形 (5)）。
pub(super) const DEFERRED: &str = "deferred";

/// land の終端が口を起こすかの結果（設計 gate-cost.md §50 形 (3)(4)）。
pub(super) enum Wake {
    /// 起こす（起点を書いた周か、起点を書かずに起こす周は stderr の理由の行を持つ）。
    Fire(Vec<String>),
    /// 下限の内なので起こさない（理由の file と detail は呼び手が [`deferred`] と event で残す）。
    Defer,
}

/// land の終端が口を起こすか決める。行を読めない周（`daily` が無い）は着地ごとに起こす今の形のまま起点に触れない。読めた周は
/// 起点の lock の内で起点を読み、読めない（無いを含む）か下限を過ぎていれば `measured` を読めた値のまま `fired` を今にした
/// 起点を書いて起こす（同じ下限の内に 2 つの終端が両方起こす形を lock で 1 つに絞る）。lock を取れない周・起点を書けない周は
/// 起点を書かずに起こし、stderr の理由の 1 行を返す（測らない周を作らない側）。
pub(super) fn wake(entry: &Detect<'_>) -> Wake {
    let Some(floor) = entry.daily else {
        return Wake::Fire(Vec::new());
    };
    let now = origin::now_epoch();
    let decided = origin::locked(entry.state_dir, entry.policy, |file| {
        let current = origin::read(file);
        if !origin::due(now, current.as_ref().map(|found| found.fired), floor) {
            return Ok(false);
        }
        let measured = current.and_then(|found| found.measured);
        origin::write(file, &Origin { measured, fired: now }).map(|()| true)
    });
    match decided {
        Ok(Ok(true)) => Wake::Fire(Vec::new()),
        Ok(Ok(false)) => Wake::Defer,
        Ok(Err(reason)) | Err(reason) => Wake::Fire(vec![format!(
            "pipe: run {} の検出の起点を書かずに起こす: {reason}",
            entry.run
        )]),
    }
}

/// 口を起こさなかった周の理由の file を次の周の置き場に置く（中身 `skipped=deferred`・判定行の file は置かない・record は
/// 足さない・設計 §50 形 (5)）。書き手は口の書き手の file の 1 本で、land の終端が呼ぶ。
pub(super) fn deferred(entry: &Detect<'_>) -> Result<(), String> {
    keep_reason(&next_copy_dir(entry.state_dir, entry.run), &format!("skipped={DEFERRED}"))
}

/// 口が終えた語（stdout の `detection=` と detail の `detection:` の値・閉じた 3 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Finished {
    /// 撃って測れた（rc 0 か 1）。
    Measured,
    /// 測れなかった（rc が 0 と 1 以外・遮断器が閉じた・worktree か親を出せない）。
    Unmeasured,
    /// 撃たなかった（面の外）。
    Skipped,
}

impl Finished {
    /// 字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Measured => "measured",
            Self::Unmeasured => "unmeasured",
            Self::Skipped => "skipped",
        }
    }
}

/// 着地した commit（`sha`）に検出線を 1 回撃ち、record・写し・event を残す（stdout は `run=<id> detection=<語>`）。
pub(in crate::pipe) fn detect(entry: &Detect<'_>, sha: &str) -> Outcome {
    let path = vessel_path(entry.state_dir, entry.run);
    let frozen = match Effective::load(&path) {
        Ok(found) => found,
        Err(errors) => {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            return broken(format!("{} を読めない: {}", path.display(), lines.join(" / ")));
        }
    };
    let lines = frozen.detection_verify();
    if lines.is_empty() {
        return refused(format!("run {} の宣言の写しに検出線の行が無い", entry.run));
    }
    let finished = match measure(entry, lines, sha) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    // 測り終えた周（測れた・面の外）だけ起点を進める。測れなかった周は動かさない（その差分は次の日次の検出に含まれる）。
    // event を見た読み手が進む前の起点を読まないよう、起点を進めてから event を書く（event が口の最後の書き・設計 §51）。
    let advanced = if entry.daily.is_some() && matches!(finished, Finished::Measured | Finished::Skipped) {
        advance_origin(entry, sha)
    } else {
        None
    };
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunDone,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Landed),
            seat: None,
            pid: None,
            detail: Some(format!("{DETAIL_HEAD}{}", finished.as_str())),
        },
        entry.policy,
    );
    if let Err(err) = emitted {
        return broken(err.to_string());
    }
    let mut outcome = Outcome::ok_line(format!("run={} detection={}", entry.run, finished.as_str()));
    outcome.err.extend(advanced);
    outcome
}

/// 起点の `measured` を着地した `sha` へ進める（lock の内・後ろへ戻さない・`fired` は読めた値のまま）。lock を取れない周と
/// 書けない周は起点を書かずに stderr の 1 行を返す（口の rc と語は変えない）。
fn advance_origin(entry: &Detect<'_>, sha: &str) -> Option<String> {
    let now = origin::now_epoch();
    let behind = |old: &str, new: &str| git_ok(entry.repo, &["merge-base", "--is-ancestor", old, new]);
    let moved = origin::locked(entry.state_dir, entry.policy, |file| {
        match origin::advanced(origin::read(file).as_ref(), sha, now, behind) {
            Some(next) => origin::write(file, &next),
            None => Ok(()),
        }
    });
    match moved {
        Ok(Ok(())) => None,
        Ok(Err(reason)) | Err(reason) => Some(format!("pipe: run {} の検出の起点を進められない: {reason}", entry.run)),
    }
}

/// 親と木を読み、面を照らし、worktree を出して撃つ（`Err` は record か写しを書けない周＝rc 2）。
///
/// 撃った後は worktree を畳む（`git worktree remove --force`・ref は動かさない＝列の次の便の CAS と交差しない）。
///
/// 日次の検出（行を読めた周・設計 §50）は `{base}` を起点の `measured` から選び、親でない周は面を base..着地した sha で
/// 照らし、便の列を読めない周は撃たず `unprepared` にする。
fn measure(entry: &Detect<'_>, lines: &[String], sha: &str) -> Result<Finished, String> {
    let parent = git_line(entry.repo, &["rev-parse", &format!("{sha}^")]);
    let tree = parent.as_ref().and_then(|_| git_line(entry.repo, &["rev-parse", &format!("{sha}^{{tree}}")]));
    let mark = LandedMark { sha, tree: tree.as_deref().unwrap_or(MAIN_UNKNOWN), since: None, runs: 0 };
    let Some(parent) = parent else {
        return unfired(entry, mark, Unfired::Unmeasured(UNPREPARED));
    };
    let base = base_of(entry, sha, &parent);
    if !touches_scope(entry.repo, &base, sha) {
        return unfired(entry, mark, Unfired::OutsideScope);
    }
    match Shot::of(entry, &parent, base, sha) {
        Some(shot) => fire(entry, lines, mark, &shot),
        None => unfired(entry, mark, Unfired::Unmeasured(UNPREPARED)),
    }
}

/// worktree を出して撃ち、record と写しと理由を残す（`Err` は record か写しを書けない周）。
fn fire(entry: &Detect<'_>, lines: &[String], mark: LandedMark<'_>, shot: &Shot) -> Result<Finished, String> {
    let tmp = check_path(entry.repo, &format!("{}{PLACE_SUFFIX}", entry.run));
    if !prepare(entry.repo, &tmp, mark.sha) {
        return unfired(entry, mark, Unfired::Unmeasured(UNPREPARED));
    }
    let fired = fire_lines(entry, lines, &tmp, shot);
    // 日次の検出の行を読めた周は、測った範囲（base と便の列）を写しの置き場に置く。
    let span = entry.daily.map(|_| (shot.base.as_str(), shot.runs.as_slice()));
    // 出力の写しは worktree を畳む前に取る（出力は tmp の `target/` に在る）。全行が閉じた周は写しを作らない。
    let kept = match &fired {
        Ok((steps, _)) if !all_closed(steps) => keep_detection(entry.state_dir, entry.run, &tmp, steps, span),
        _ => Ok(None),
    };
    let _ = git_ok(entry.repo, &["worktree", "remove", "--force", &tmp.display().to_string()]);
    let (steps, pure_move) = fired?;
    if all_closed(&steps) {
        return unfired(entry, mark, Unfired::Unmeasured(HOST_CLOSED));
    }
    let dir = kept?.unwrap_or_else(|| next_copy_dir(entry.state_dir, entry.run));
    let ranged = LandedMark { since: span.map(|(since, _)| since), runs: shot.runs.len(), ..mark };
    record_fired(entry, ranged, &steps, pure_move)?;
    match unmeasured_of(&steps) {
        None => Ok(Finished::Measured),
        Some(reason) => {
            keep_reason(&dir, &Unfired::Unmeasured(&reason).word())?;
            Ok(Finished::Unmeasured)
        }
    }
}

/// 撃つ範囲（設計 gate-cost.md §50 形 (6)(8)）: `{base}`・base が親でない周の契約（verify 行を列の便の filter 語が初出の行に
/// 差し替えた複製）・便の列（古い順・読めない契約の便は ` contract=unreadable` を後ろに置く）。
struct Shot {
    /// `{base}` の sha。
    base: String,
    /// base が親でない周の契約（親の周は無し＝自分の契約の写しのまま）。
    spread: Option<Contract>,
    /// 便の列の行（`runs` の file の 1 行 1 本）。
    runs: Vec<String>,
}

impl Shot {
    /// base が親の周は自分 1 本、親でない周は base..着地した sha の便の列（読めない周は `None`）。
    fn of(entry: &Detect<'_>, parent: &str, base: String, sha: &str) -> Option<Self> {
        if base == parent {
            return Some(Self { base, spread: None, runs: vec![entry.run.to_owned()] });
        }
        let ids = runs_between(entry.repo, &base, sha)?;
        let (contract, runs) = span_of(entry, &ids);
        Some(Self { base, spread: Some(contract), runs })
    }
}

/// 日次の検出の `{base}`（行を読めない周は親）。起点を lock の内で読み（lock を取れない周は読めない周と同じ・書かない）、
/// measured が着地した sha の真の祖先ならその measured・measured が着地した sha と等しいか祖先なら親（起点より古い便）・
/// 読めない／`-`／祖先の関係が無い周は記録の移行の種（[`seed_base`]）か親（設計 §50 形 (6)）。
fn base_of(entry: &Detect<'_>, sha: &str, parent: &str) -> String {
    if entry.daily.is_none() {
        return parent.to_owned();
    }
    let read = origin::locked(entry.state_dir, entry.policy, origin::read);
    let measured = read.ok().flatten().and_then(|found| found.measured);
    let ancestor = |old: &str, new: &str| git_ok(entry.repo, &["merge-base", "--is-ancestor", old, new]);
    if let Some(found) = measured.as_deref() {
        if found != sha && ancestor(found, sha) {
            return found.to_owned();
        }
        if found == sha || ancestor(sha, found) {
            return parent.to_owned();
        }
    }
    seed_base(entry, sha, &ancestor).unwrap_or_else(|| parent.to_owned())
}

/// 移行の種: 記録を新しい順に辿り、detail `detection:measured` か `detection:skipped` を持ち着地した sha が着地した `sha` の
/// 真の祖先である最初の便の sha（無い・記録を読めない周は `None`）。
fn seed_base(entry: &Detect<'_>, sha: &str, ancestor: &dyn Fn(&str, &str) -> bool) -> Option<String> {
    let events = read_all(entry.state_dir).ok()?;
    let mut landed: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for event in events.iter().filter(|event| event.kind == EventKind::RunDone) {
        let words = event.detail.as_deref().unwrap_or_default().split_whitespace();
        if let Some(found) = words.into_iter().find_map(|word| word.strip_prefix(SHA_PREFIX)) {
            landed.insert(event.run.as_str(), found);
        }
    }
    let measured = [Finished::Measured, Finished::Skipped].map(|word| format!("{DETAIL_HEAD}{}", word.as_str()));
    events
        .iter()
        .rev()
        .filter(|event| event.kind == EventKind::RunDone && event.stage == Some(Stage::Landed))
        .filter(|event| event.detail.as_ref().is_some_and(|detail| measured.contains(detail)))
        .find_map(|event| {
            let found = *landed.get(event.run.as_str())?;
            (found != sha && ancestor(found, sha)).then(|| found.to_owned())
        })
}

/// base..着地した sha の commit のうち本文に行 `run: <id>` を持つ commit の便 id を古い順に（commit の列を読めない周は
/// `None`・設計 §50 形 (8)）。
fn runs_between(repo: &Path, base: &str, sha: &str) -> Option<Vec<String>> {
    let log = git_bytes(repo, &["log", "--reverse", "--format=%x01%B", &format!("{base}..{sha}")])?;
    let text = String::from_utf8_lossy(&log);
    Some(
        text.split('\u{1}')
            .filter_map(|body| body.lines().rev().find_map(|line| line.strip_prefix(RUN_TRAILER)))
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty())
            .collect(),
    )
}

/// 便の列の契約の複製（verify 行を、列の順・行の順に filter 語が初出の行だけにしたもの）と `runs` の file の行。写しを読めない
/// 便は語を足さず、その行の後ろに ` contract=unreadable` を置く。自分の契約は読み込み済みの値（同じ写し）を使う。
fn span_of(entry: &Detect<'_>, ids: &[String]) -> (Contract, Vec<String>) {
    let (mut words, mut verify, mut runs) = (Vec::<String>::new(), Vec::<String>::new(), Vec::<String>::new());
    for id in ids {
        let loaded = if id == entry.run {
            Some(entry.contract.clone())
        } else {
            Contract::load(&contract_path(entry.state_dir, id)).ok()
        };
        let Some(found) = loaded else {
            runs.push(format!("{id} contract=unreadable"));
            continue;
        };
        runs.push(id.clone());
        for line in &found.verify {
            let word = teeth_words(std::slice::from_ref(line)).first().map(|found| (*found).to_owned());
            if let Some(word) = word.filter(|word| !words.contains(word)) {
                words.push(word);
                verify.push(line.clone());
            }
        }
    }
    (Contract { verify, ..entry.contract.clone() }, runs)
}

/// 着地した commit と親の path が検出線の面に触れるか（**読めない周は触れる側**＝撃つ・fail-closed・設計 §44 形 (3)）。
fn touches_scope(repo: &Path, parent: &str, sha: &str) -> bool {
    let Some(bytes) = git_bytes(repo, &["diff-tree", "-r", "--name-only", "-z", parent, sha]) else {
        return true;
    };
    let paths = nul_paths(&bytes);
    scope_touched(&RootsAtHead::read(repo), &paths.iter().map(String::as_str).collect::<Vec<&str>>())
}

/// 着地した commit を置き場の別名へ detach で出す（出せた周だけ真）。
fn prepare(repo: &Path, tmp: &Path, sha: &str) -> bool {
    let Some(parent) = tmp.parent() else {
        return false;
    };
    std::fs::create_dir_all(parent).is_ok() && git_ok(repo, &["worktree", "add", "--detach", &tmp.display().to_string(), sha])
}

/// 写しの検出線に gate と同じ付け足し（的・純移動の母集団）をして、検出線の行だけを受付の中で撃つ。base が親でない周
/// （日次の検出が便の列をまとめて測る周）は付け足さず、{teeth} は列の契約の複製から取る（設計 §50 形 (8)）。
fn fire_lines(entry: &Detect<'_>, lines: &[String], tmp: &Path, shot: &Shot) -> Result<(Vec<Step>, Option<usize>), String> {
    let (lines, pure_move) = match shot.spread {
        Some(_) => (lines.to_vec(), None),
        None => {
            let aimed = aimed_lines(entry.state_dir, entry.run, lines)?;
            population_lines(entry.state_dir, entry.run, tmp, &shot.base, aimed)?
        }
    };
    let admit = Admit { state_dir: entry.state_dir, run: entry.run, rules: entry.limits.admission(entry.policy) };
    let checks = Checks {
        worktree: tmp,
        base: &shot.base,
        contract: shot.spread.as_ref().unwrap_or(entry.contract),
        common: &[],
        detection: &lines,
        host: entry.limits.breaker(),
    };
    Ok((run_detection_admitted(&checks, Some(&admit)), pure_move))
}

/// 全行が遮断器で閉じた（1 行も撃っていない）周か。閉じた印は以後の行へ伝わるので先頭の行で決まる。
fn all_closed(steps: &[Step]) -> bool {
    steps.first().is_some_and(Step::is_closed)
}

/// 撃った周の測れなかった理由（在れば）: 途中で閉じた行が在れば `host-closed`、無ければ rc が 0 と 1 以外の最初の行。
fn unmeasured_of(steps: &[Step]) -> Option<String> {
    if steps.iter().any(Step::is_closed) {
        return Some(HOST_CLOSED.to_owned());
    }
    steps
        .iter()
        .find(|step| !matches!(step.rc, 0 | 1))
        .map(|step| format!("rc-{}", recorded_rc(step.rc)))
}

/// 撃たなかった / 撃てなかった周: 理由を持つ record 1 本と、次の周の置き場に理由の file 1 つ。
///
/// land が口を起こせなかった周（`unmeasured=unspawned`）も [`super::finish`] がこの 1 本で書く（書き手を増やさない・C2）。
pub(super) fn unfired(entry: &Detect<'_>, mark: LandedMark<'_>, why: Unfired<'_>) -> Result<Finished, String> {
    let path = record_path(entry);
    let n = next_n(&path)?;
    append_line(&path, &landed_unfired_record(n, mark, why), entry.policy).map_err(|err| err.to_string())?;
    keep_reason(&next_copy_dir(entry.state_dir, entry.run), &why.word())?;
    Ok(match why {
        Unfired::OutsideScope => Finished::Skipped,
        Unfired::Unmeasured(_) => Finished::Unmeasured,
    })
}

/// 撃った行ごとの record を追記し、赤い行の stderr の写しを主実測と同じ診断 file へ残す（gate と同じ書き口）。
fn record_fired(entry: &Detect<'_>, mark: LandedMark<'_>, steps: &[Step], pure_move: Option<usize>) -> Result<(), String> {
    let path = record_path(entry);
    let diagnosis = path.with_file_name(VERIFY_MAIN_STDERR_FILE);
    let first = next_n(&path)?;
    for (offset, step) in steps.iter().enumerate() {
        let n = first.saturating_add(u64::try_from(offset).unwrap_or(u64::MAX));
        let record = Record { n, body: landed_step_record(n, step, mark, pure_move), step: Some(step) };
        record.diagnose(&diagnosis, entry.policy)?;
        append_line(&path, &record.body, entry.policy).map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// 便自身の `verify-main.jsonl`（主実測と同じ file）。
fn record_path(entry: &Detect<'_>) -> PathBuf {
    verify_log_path(entry.state_dir, entry.run).with_file_name(VERIFY_MAIN_FILE)
}

/// 次の record の `n`（既存の非空の行の次・無い file は 1・在るのに読めない周は `Err`）。
fn next_n(path: &Path) -> Result<u64, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(next_number(text.lines().filter(|line| !line.trim().is_empty()).count())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(next_number(0)),
        Err(err) => Err(format!("{} を読めない: {err}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::notes_repo;
    use super::origin::{advanced, compose_line, due, parse_line, Origin};
    use super::touches_scope;

    /// 検出の起点の 1 行（設計 gate-cost.md §50 形 (2)）は空白で割った 2 語ちょうどのときだけ読める: 40 字・64 字・`-` の
    /// measured が読め、空・逆順・3 語・key 違い・39 字・16 進でない字・`fired=-1`・`fired=` は読めず、読めた 2 値から組む字
    /// は元の 1 行と改行に等しい。後ろへ戻さない進め方（等しい・祖先は動かさず、読めない起点は fired を今にする）も測る。
    #[test]
    fn detection_origin_line_reads_two_words_and_refuses_other_shapes() {
        let (short, long) = ("a".repeat(40), "0123456789abcdef".repeat(4));
        for (line, measured, fired) in [
            (format!("measured={short} fired=1759453740\n"), Some(short.as_str()), 1_759_453_740),
            (format!("measured={long} fired=0\n"), Some(long.as_str()), 0),
            ("measured=- fired=7\n".to_owned(), None, 7),
        ] {
            let read = parse_line(&line).unwrap_or_else(|| panic!("読める: {line:?}"));
            assert_eq!((read.measured.as_deref(), read.fired), (measured, fired), "{line:?}");
            assert_eq!(compose_line(&read), line, "読めた 2 値から組む字は元の 1 行と改行");
        }
        for line in [
            String::new(),
            format!("fired=5 measured={short}"),
            format!("measured={short} fired=5 extra=1"),
            format!("measure={short} fired=5"),
            format!("measured={short} fire=5"),
            format!("measured={} fired=5", "a".repeat(39)),
            format!("measured={} fired=5", "g".repeat(40)),
            format!("measured={short} fired=-1"),
            format!("measured={short} fired="),
            format!("measured={short} fired=99999999999999999999999"),
            format!("measured={short}"),
        ] {
            assert_eq!(parse_line(&line), None, "読めない形: {line:?}");
        }
        let (old, new) = ("1".repeat(40), "2".repeat(40));
        let behind = |a: &str, b: &str| (a, b) == (old.as_str(), new.as_str());
        let at = |measured: Option<&str>| Origin { measured: measured.map(str::to_owned), fired: 9 };
        assert_eq!(advanced(None, &new, 50, behind), Some(Origin { measured: Some(new.clone()), fired: 50 }), "読めない起点は今");
        assert_eq!(advanced(Some(&at(None)), &new, 50, behind), Some(Origin { measured: Some(new.clone()), fired: 9 }), "fired は読めた値");
        assert_eq!(advanced(Some(&at(Some(&new))), &new, 50, behind), None, "等しい周は動かさない");
        assert_eq!(advanced(Some(&at(Some(&new))), &old, 50, behind), None, "祖先の周は後ろへ戻さない");
    }

    /// 起こすかの判定（設計 §50 形 (3)）は今・fired・下限だけで決まる: 読めない起点は起こし、fired 100・下限 50 で今 150 は
    /// 起こし 149 は起こさず、下限 0 は今 = fired で起こし、fired が u64 の最大で今 1000 は起こさず panic しない。
    #[test]
    fn detection_origin_due_counts_from_fired_by_the_floor() {
        assert!(due(0, None, 86_400), "読めない起点は起こす");
        assert!(due(150, Some(100), 50), "fired + 下限 = 今は起こす");
        assert!(!due(149, Some(100), 50), "1 秒手前は起こさない");
        assert!(due(100, Some(100), 0), "下限 0 は今 = fired で起こす");
        assert!(!due(1000, Some(u64::MAX), 86_400), "飽和加算で上限に張り付き、起こさない");
    }

    /// (g) 宣言が在って読めない周（key の値が絶対 path）は、面の外の `notes/` の file だけの着地でも検出線を撃つ側
    /// （`touches_scope` が真）。key の無い形は今どおり撃たない（偽）。読めない周を固定の根だけに倒す実装は前者で落ちる。
    #[test]
    fn declaration_crate_roots_touches_scope_reads_an_unreadable_declaration_as_touching() {
        let (broken, parent, sha) = notes_repo("crate-roots-touch-broken", "crate-roots = [\"/abs/\"]\n");
        assert!(touches_scope(&broken, &parent, &sha), "読めない宣言は撃つ");
        let (plain, parent, sha) = notes_repo("crate-roots-touch-plain", "");
        assert!(!touches_scope(&plain, &parent, &sha), "key の無い宣言は面の外だけなら撃たない（対照）");
    }
}
