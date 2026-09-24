//! 着地の列（merge train・設計 docs/design/pipeline.md §40・契約表の行 ah・ADR-0039）。
//!
//! 列の先頭（番待ちで `First` / `Waited` を得た便）が着地する周、列の自分の後ろに並ぶ便を rules 行 `land.train_max`
//! − 1 本まで取り（[`train_now`]）、自分を先頭に並べた列を **1 つの候補の木**に積む。候補の木は main の先端から切った
//! tmp worktree（主実測の `verify` の隣の `train`・便の worktree と記録の base は触らない）で、便ごとに
//! `git cherry-pick <base>..<HEAD>` で順に積む。積めなかった後続（衝突）は `cherry-pick --abort` で外し（その便の
//! event は書かない・後続は詰める）、自分の land で従来どおり追随する。
//!
//! 積んだ便ごとにその段の tree を覚える（検出線は撃たない＝着地後の検出の口だけ・設計 gate-cost.md §44 形 (9)）。全部積んだ
//! 木に対して共通 verify を **1 回**（record は先頭の便の `verify.jsonl`・field `train=<N>`）と各便の契約 verify をその便の分撃つ
//! ＝lens は撃たない（各便の verdict PASS が入口の条件で、候補の木で測るのは木の緑）。**緑**なら段の tree で列の順に
//! 着地する（[`land_train`]・主実測は先端の木で 1 回）。**赤**・切れない・積めない・読めない周は候補の木を畳んで
//! **列を解き**、先頭 1 本は既存の経路（追随 → 撃ち直し）へそのまま入る（どの便が赤かは帰属しない・後続は列に残る）。
//!
//! stdout の 1 行に `train=<積んだ本数>`（解いた周は `train=<N> dissolved why=<段>`）。上限 1 と行の不在は現行の経路
//! そのもの（[`Train::Solo`]）。

use super::contract::Contract;
use super::declaration::Effective;
use super::gate::{is_unreadable, run_checks, step_record, Check, Checks, Step};
use super::land::{land_train, Car, Land, WorktreeCheck, MAIN_REF};
use super::queue::{train_now, Order};
use super::{base_of_run, contract_path, current, git_line, git_ok, repo_of_run, vessel_path, worktree_path, worktrees_dir};
use crate::cli_outcome::Outcome;
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::append_line;
use std::path::{Path, PathBuf};

/// 候補の木の tmp worktree を置く dir 名（主実測の `verify` の隣・run id は `<bead>-<stamp>` なので衝突しない）。
const TRAIN_DIR: &str = "train";

/// 列の周の結末（**閉じた 3 値**・[`super::land::land`] の番待ちの直後が受ける）。
pub(super) enum Train {
    /// 列が着地した（main の実測の赤 / 測れないを含む・呼び手はそのまま返す）。
    Landed(Outcome),
    /// 列を解いた（stdout の 1 行）。先頭 1 本は既存の経路へ進む。
    Dissolved(String),
    /// 積む便が無い・上限 1・行の不在・先頭でない周（何もしていない＝現行の経路）。
    Solo,
}

/// 列を解いた段（stdout の `why=`・**閉じた 5 値**）。
#[derive(Debug, Clone, Copy)]
enum Why {
    /// main・先頭の base / HEAD・写しを読めない。
    Read,
    /// 候補の木を切れない。
    Cut,
    /// 先頭を積めない（衝突・空）か、`cherry-pick --abort` が木を戻せない。
    Stack,
    /// 候補の木の検査（共通 / 契約）が赤か、record を書けない。
    Verify,
    /// main を CAS で進められない（main は動いていない）。
    Cas,
}

impl Why {
    /// stdout の `why=` の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Cut => "cut",
            Self::Stack => "stack",
            Self::Verify => "verify",
            Self::Cas => "cas",
        }
    }
}

/// 列に乗る便 1 本の材料（置き場から読んだもの・先頭も同じ形で持つ）。
struct Rider {
    /// 便 id。
    run: String,
    /// 契約の bead id。
    bead: String,
    /// 便の契約（`pipe/<run>/contract.toml` の写し）。
    contract: Contract,
    /// 承認 event が在るか（replay の導出値）。
    approved: bool,
    /// 便の worktree。
    worktree: PathBuf,
    /// 記録の base。
    base: String,
    /// worktree の HEAD（積む範囲は `<base>..<tip>`）。
    tip: String,
}

/// 積めた便 1 本（段の commit と tree）。
struct Stacked {
    /// 列の中の位置（0 = 先頭）。
    index: usize,
    /// 段の commit（候補の木の中）。
    commit: String,
    /// 段の tree（squash の材料）。
    tree: String,
}

/// 列の周を通す（設計 §40）。先頭でない周・上限 1 以下・後ろに便が居ない周は [`Train::Solo`]（何もしない）。
pub(super) fn train(entry: &Land<'_>, worktree: &Path, order: Order) -> Train {
    if !order.is_front() || entry.train_max <= 1 {
        return Train::Solo;
    }
    let behind = train_now(entry.state_dir, entry.run, entry.train_max);
    if behind.is_empty() {
        return Train::Solo;
    }
    let Some(head) = head_rider(entry, worktree) else {
        return dissolved(entry, 1, Why::Read);
    };
    let mut riders = vec![head];
    riders.extend(behind.iter().filter_map(|run| rider_of(entry, run)));
    if riders.len() < 2 {
        return Train::Solo;
    }
    let Some(old) = git_line(entry.repo, &["rev-parse", MAIN_REF]) else {
        return dissolved(entry, 1, Why::Read);
    };
    let candidate = worktrees_dir(entry.repo).join(TRAIN_DIR).join(entry.run);
    if !cut(entry.repo, &candidate, &old) {
        return dissolved(entry, 1, Why::Cut);
    }
    let result = run_train(entry, &candidate, &riders, &old);
    fold(entry.repo, &candidate);
    let (stacked, check) = match result {
        Ok(found) => found,
        Err((count, why)) => return dissolved(entry, count, why),
    };
    // 後続がどれも積めなかった周は列が無い（先頭 1 本の既存の経路・`train=` を出さない）。
    if stacked.len() < 2 {
        return Train::Solo;
    }
    let count = stacked.len();
    if let Err(why) = check {
        return dissolved(entry, count, why);
    }
    let mut cars = Vec::new();
    for found in &stacked {
        let Some(rider) = riders.get(found.index) else {
            return dissolved(entry, count, Why::Read);
        };
        cars.push(Car {
            entry: land_of(entry, rider),
            worktree: rider.worktree.clone(),
            tree: found.tree.clone(),
            order: if found.index == 0 { order } else { Order::Train },
        });
    }
    match land_train(&cars, &old) {
        Err(_) => dissolved(entry, count, Why::Cas),
        Ok(mut outcome) => {
            outcome.out.push(format!("run={} train={count}", entry.run));
            Train::Landed(outcome)
        }
    }
}

/// 積んだ列と候補の木の検査の結末（検査の `Err` は解く段）。
type Ran = (Vec<Stacked>, Result<(), Why>);

/// 積んで検査する本体（候補の木を畳む責任は呼び手）。`Err` は先頭を積めない / 木を戻せない周（積んだ本数と段・
/// 解く）、`Ok` の検査の `Err` は積んだ後の検査が赤の周（解く）。
fn run_train(entry: &Land<'_>, candidate: &Path, riders: &[Rider], old: &str) -> Result<Ran, (usize, Why)> {
    let mut stacked: Vec<Stacked> = Vec::new();
    for (index, rider) in riders.iter().enumerate() {
        match stack(candidate, rider) {
            Some((commit, tree)) => stacked.push(Stacked { index, commit, tree }),
            // 先頭が積めない周は列そのものを解く（先頭は既存の経路で追随と衝突を扱う）。
            None if index == 0 => return Err((1, Why::Stack)),
            // 後続の衝突は外して詰める（その便の event は書かない）。木が直前の段へ戻らない周は解く（fail-closed）。
            None if unstacked(candidate, stacked.last().map(|found| found.commit.as_str())) => {}
            None => return Err((stacked.len(), Why::Stack)),
        }
    }
    if stacked.len() < 2 {
        return Ok((stacked, Ok(())));
    }
    let check = verify_candidate(entry, candidate, riders, &stacked, old);
    Ok((stacked, check))
}

/// 候補の木の検査（共通 verify は先端で 1 回・契約 verify は便ごとに先端で・検出線は撃たない＝着地後の検出の口
/// だけ・設計 gate-cost.md §44 形 (9)）。1 行でも赤 / 撃てない / 読めない・record を書けない周は `Err`。
fn verify_candidate(entry: &Land<'_>, candidate: &Path, riders: &[Rider], stacked: &[Stacked], old: &str) -> Result<(), Why> {
    let count = u64::try_from(stacked.len()).unwrap_or(u64::MAX);
    let tip = stacked.last().map(|found| found.commit.clone()).ok_or(Why::Read)?;
    let mut red = false;
    if !git_ok(candidate, &["checkout", "-q", "-f", "--detach", &tip]) {
        return Err(Why::Verify);
    }
    // 先端の木: 先頭は共通 verify（列で 1 回・`train=<N>`）と自分の契約 verify、後続は契約 verify だけ。
    for found in stacked {
        let rider = riders.get(found.index).ok_or(Why::Read)?;
        let common = match found.index {
            0 => frozen_of(entry, &rider.run)?.common_verify().to_vec(),
            _ => Vec::new(),
        };
        let steps = checks_on(entry, candidate, old, &rider.contract, &common);
        let kept: Vec<Step> = steps.into_iter().filter(|step| matches!(step.stage, Check::Common | Check::Contract)).collect();
        red |= record(entry, &rider.run, &kept, count)?;
    }
    match red {
        true => Err(Why::Verify),
        false => Ok(()),
    }
}

/// 候補の木で行を撃つ（gate と land の主実測と同じ [`run_checks`] の 1 本・受付は通らない＝主実測と同じ）。
fn checks_on(entry: &Land<'_>, candidate: &Path, base: &str, contract: &Contract, common: &[String]) -> Vec<Step> {
    run_checks(&Checks {
        worktree: candidate,
        base,
        contract,
        common,
        detection: &[],
        host: entry.limits.breaker(),
    })
}

/// 撃った段を便の `verify.jsonl` へ足し（`n` は既存の record からの通し・共通 verify だけ `train=<N>`・`count` は
/// 列の本数）、赤か（rc≠0・撃てない・読めない）を返す。書けない周は `Err`（記録が無いものを緑と読まない）。
fn record(entry: &Land<'_>, run: &str, steps: &[Step], count: u64) -> Result<bool, Why> {
    let written = written_records(entry, run)?;
    let path = super::verify_log_path(entry.state_dir, run);
    for (offset, step) in steps.iter().enumerate() {
        let number = super::gate::next_number(written.saturating_add(offset));
        let body = step_record(number, step);
        let line = match step.stage {
            Check::Common => with_train(&body, count).ok_or(Why::Verify)?,
            _ => body,
        };
        append_line(&path, &line, entry.policy).map_err(|_| Why::Verify)?;
    }
    Ok(steps.iter().any(|step| step.rc != 0 || step.is_closed() || is_unreadable(step)))
}

/// 便の `verify.jsonl` の既存の record 数（空行を除く）。読めない周は `Err`。
fn written_records(entry: &Land<'_>, run: &str) -> Result<usize, Why> {
    std::fs::read_to_string(super::verify_log_path(entry.state_dir, run))
        .map(|text| text.lines().filter(|line| !line.trim().is_empty()).count())
        .map_err(|_| Why::Verify)
}

/// record の 1 行に `train=<N>` を足す（形は [`step_record`] のまま任意 field を末尾に 1 つ・schema は 1 のまま）。
fn with_train(body: &str, count: u64) -> Option<String> {
    let mut pairs = json_lite::parse_object(body.trim()).ok()?;
    pairs.push(("train".to_owned(), Value::Num(count)));
    let borrowed: Vec<(&str, Value)> = pairs.iter().map(|(key, value)| (key.as_str(), value.clone())).collect();
    Some(json_lite::write_object(&borrowed))
}

/// 便の写しの宣言（共通 verify と検出線の出所・repo / worktree の宣言は読み直さない）。
fn frozen_of(entry: &Land<'_>, run: &str) -> Result<Effective, Why> {
    Effective::load(&vessel_path(entry.state_dir, run)).map_err(|_| Why::Read)
}

/// 候補の木を main の先端から切る（前の周の残骸が在れば先に畳む）。
fn cut(repo: &Path, candidate: &Path, old: &str) -> bool {
    if candidate.exists() {
        fold(repo, candidate);
        let _ = git_ok(repo, &["worktree", "prune"]);
    }
    if let Some(parent) = candidate.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    let path = candidate.display().to_string();
    git_ok(repo, &["worktree", "add", "--detach", &path, old])
}

/// 便 1 本を積む（`cherry-pick <base>..<tip>`）。積めた周は段の commit と tree。
fn stack(candidate: &Path, rider: &Rider) -> Option<(String, String)> {
    let range = format!("{}..{}", rider.base, rider.tip);
    if !git_ok(candidate, &["cherry-pick", "--allow-empty-message", &range]) {
        return None;
    }
    let commit = git_line(candidate, &["rev-parse", "HEAD"])?;
    let tree = git_line(candidate, &["rev-parse", "HEAD^{tree}"])?;
    Some((commit, tree))
}

/// 積めなかった後続を外す: `cherry-pick --abort` で戻し（範囲が空で始まっていない周は abort が断るので結果は見ない）、
/// 木が**直前の段の commit のまま clean** かを実測する。戻っていない周は偽（呼び手が列を解く）。
fn unstacked(candidate: &Path, previous: Option<&str>) -> bool {
    let _ = git_ok(candidate, &["cherry-pick", "--abort"]);
    let head = git_line(candidate, &["rev-parse", "HEAD"]);
    let clean = super::git_bytes(candidate, &["status", "--porcelain", "--untracked-files=no"]).is_some_and(|bytes| bytes.is_empty());
    clean && previous.is_some() && head.as_deref() == previous
}

/// 候補の木を畳む（中間物ごと・履歴とデータは触らない＝主実測の tmp と同じ扱い）。
fn fold(repo: &Path, candidate: &Path) {
    let path = candidate.display().to_string();
    let _ = git_ok(repo, &["worktree", "remove", "--force", &path]);
}

/// 列を解いた周の 1 行。
fn dissolved(entry: &Land<'_>, count: usize, why: Why) -> Train {
    Train::Dissolved(format!("run={} train={count} dissolved why={}", entry.run, why.as_str()))
}

/// 先頭の材料（land の材料から組む・base と HEAD は置き場と worktree から読む）。
fn head_rider(entry: &Land<'_>, worktree: &Path) -> Option<Rider> {
    Some(Rider {
        run: entry.run.to_owned(),
        bead: entry.bead.to_owned(),
        contract: entry.contract.clone(),
        approved: entry.approved,
        worktree: worktree.to_path_buf(),
        base: base_of_run(entry.state_dir, entry.run).known()?,
        tip: git_line(worktree, &["rev-parse", "HEAD"])?,
    })
}

/// 後続 1 本の材料。**同じ repo の便**で、契約・base・HEAD を読めて worktree が clean な周だけ（読めない便は積まない
/// ＝自分の land で従来どおり進む）。
fn rider_of(entry: &Land<'_>, run: &str) -> Option<Rider> {
    if repo_of_run(entry.state_dir, run)?.as_path() != entry.repo {
        return None;
    }
    let state = current(entry.state_dir).ok()?;
    let found = state.runs.get(run)?;
    let worktree = worktree_path(entry.repo, run);
    if !WorktreeCheck::judge(&worktree).is_clean() {
        return None;
    }
    Some(Rider {
        run: run.to_owned(),
        bead: found.bead.clone(),
        contract: Contract::load(&contract_path(entry.state_dir, run)).ok()?,
        approved: found.approved,
        base: base_of_run(entry.state_dir, run).known()?,
        tip: git_line(&worktree, &["rev-parse", "HEAD"])?,
        worktree,
    })
}

/// 後続の land の材料（先頭の材料から便の 4 欄だけを差し替える・PR の seam と起こし直しは持たない）。
fn land_of<'a>(entry: &Land<'a>, rider: &'a Rider) -> Land<'a> {
    Land {
        run: &rider.run,
        bead: &rider.bead,
        contract: &rider.contract,
        approved: rider.approved,
        pr_cmd: None,
        runner: None,
        ..*entry
    }
}
