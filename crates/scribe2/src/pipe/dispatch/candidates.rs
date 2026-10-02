//! 台帳から候補を組む群（設計 docs/design/dispatcher.md §20・契約表の行 q・`s2-07l.531`）。
//!
//! 台帳の 1 件を列の 1 件に解き（[`is_input`] / [`entry_of`]）、順序の後に交差と枠を測って起こす便と待つ便に
//! 分ける（[`settle`]）群である。`pipe/dispatch.rs` からの**純移動**で、歯は 1 本も足していない（親に残る
//! in-file の歯と e2e が従来どおり測る）。外の呼び手は 0 で、親の周の本体からだけ入る。
//!
//! 1 周ぶん固定な材料の型 `Ledger` と印の畳み込みの結果 `Marks` は**親に残る**——親の `turn` が struct literal と
//! field access で使うので、ここへ移すと field の可視性を上げることになる（§20）。子孫は親の私有 item と
//! field を `super::` でそのまま見るので、**親側の可視性は 1 語も上げていない**。親が呼ぶ 10 名だけが
//! `pub(super)` で、残りはこの module に閉じる。

// flip-check: moved s2-07l.531

use super::super::admission::{self, Sizes};
use super::super::cli::{crossings, generated, int_row, judge, live, Material, Materials};
use super::super::contract::Contract;
use super::super::gate::Verdict;
use super::super::refuse::{overlaps, INDEX_BUILD_TRIGGERS};
use super::super::review;
use super::super::row_review::{self, Basis};
use super::super::table::{self, read_rows, Pointer};
use super::super::{contract_path, current, git_bytes, git_line, show_head};
use super::index_build::{status as index_status, Status as IndexStatus};
use super::{
    measure, reserve, spawn_self, Candidate, Input, Launch, Ledger, Marks, Read, Turn, Unmeasured, WaitReason, BLOCKS, DESIGN_KEY, DRIVE, MARK,
    OPEN, ROW_JOB_MB, ROW_RESERVE_MB, SLOT,
};
use crate::fleet::lifecycle::{self, Place, Round, Source};
use crate::fleet::phase::Judged;
use crate::fleet::store::LockPolicy;
use crate::fleet::{Event, EventKind, Mark, Stage};
use crate::ledger::form::{is_memo, is_question};
use crate::rules::manifest::Manifest;
use crate::seat::host_slots_dir;
use crate::seat::ledger::{Dep, Issue};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// 列の入力になる bead か（設計 §2・**ここで落ちた bead は `ls` にも出ない**＝契約が未確定か終わっているか、
/// memo か台帳の問い＝契約でない・§31）。
pub(super) fn is_input(issue: &Issue) -> bool {
    issue.status == OPEN && !issue.acceptance.trim().is_empty() && !is_memo(issue) && !is_question(issue)
}

/// 台帳の 1 件を列の 1 件に解く（依存 → 印 → 設計 pointer → 審査 FAIL → 契約の生成の順）。
///
/// 交差と枠は**順序の後**に測る（§3「1 周で起こした便は次の候補の交差の相手」）ので、ここでは決めない。
/// 理由の付かなかった候補だけが設計 pointer と契約を持って返る。
pub(super) fn entry_of(input: &Input<'_>, issue: &Issue, ledger: &Ledger<'_>, kins: &[Kin]) -> (Candidate, Option<(Pointer, Contract)>) {
    let marked = ledger.marks.get(&issue.id);
    let at = |reason: Option<WaitReason>| Candidate {
        bead: issue.id.clone(),
        priority: issue.priority,
        mark: marked.map(|(found, _)| *found),
        reason,
    };
    let wait = |reason: WaitReason| (at(Some(reason)), None);
    let blocked: Vec<String> =
        issue.deps.iter().filter(|dep| is_blocking(dep, &ledger.closed)).map(|dep| dep.on.clone()).collect();
    if !blocked.is_empty() {
        return wait(WaitReason::Dependency { on: blocked });
    }
    if let Some((Mark::Hold, since)) = marked {
        return wait(WaitReason::Hold { since: since.clone() });
    }
    // **起こした便が受付に届くまで同じ bead を起こさない**（設計 §17）。印を測れない周は起こさない側に倒す。
    match ledger.launched.as_ref().map(|found| found.get(&issue.id)) {
        None => return wait(WaitReason::Admission { reason: MARK }),
        Some(Some(since)) => return wait(WaitReason::Launched { since: since.clone() }),
        Some(None) => {}
    }
    let Some(pointer) = pointer_of(&issue.acceptance) else {
        return wait(WaitReason::NoDesignPointer);
    };
    let materials = match &ledger.materials {
        Ok(found) => found,
        Err(denial) => return wait(WaitReason::Admission { reason: denial.name }),
    };
    let contract = match generated(input.repo, &pointer, materials) {
        Ok((found, body)) => match settled(input, &issue.id, &body, &found.design, &ledger.events) {
            Some((sha, stage)) => return wait(WaitReason::Settled { sha, stage }),
            None => {
                // 兄弟の待ちは settled の後・床の上書きの前（設計 row-review.md §8）。介入 `first` の印を持つ候補は待たせない。
                let first = matches!(marked, Some((Mark::First, _)));
                if let Some(by) = (!first).then(|| sibling_of(input, &issue.id, &pointer, (&found, &body), kins)).flatten() {
                    return wait(WaitReason::Sibling(by));
                }
                found
            }
        },
        Err(denial) => return wait(WaitReason::Admission { reason: denial.name }),
    };
    (at(None), Some((pointer, contract)))
}

/// 材料に base（`HEAD`）の commit の索引の状態を載せる（設計 reverse-index.md §7 (b)・状態の読みだけで撃たず待たない・`HEAD` を解けない周は
/// 状態なしのまま）。観測の口（`dispatch ls`）と起こす側の周が同じ 1 本で載せる。
pub(super) fn indexed(input: &Input<'_>, materials: Materials) -> Materials {
    match git_line(input.repo, &["rev-parse", "HEAD"]) {
        Some(sha) => materials.indexed(input.state_dir, input.repo, &sha),
        None => materials,
    }
}

/// 起こす側の 1 周だけが撃つ索引の組み立て（設計 reverse-index.md §7 (b)・[`fire`](super::fire) だけが呼ぶ＝観測の口は撃たない）: `HEAD` の
/// 状態が absent で、起こす契機の語（[`INDEX_BUILD_TRIGGERS`]・文字列の列で引く）で受付の理由に待つ候補が在る周に、
/// `pipe index build --ref <HEAD>` を [`spawn_self`] で裏に 1 本起こして待たない（撃ち中の印が 2 本目を止める）。
/// 生きた印の周・failed の鍵（毎周の失敗を避ける）・契機の語で待つ候補の無い周は起こさない。
pub(super) fn build_index(input: &Input<'_>, turn: &Turn) {
    let waits = turn
        .candidates
        .iter()
        .any(|candidate| matches!(&candidate.reason, Some(WaitReason::Admission { reason }) if INDEX_BUILD_TRIGGERS.contains(reason)));
    let Some(sha) = git_line(input.repo, &["rev-parse", "HEAD"]).filter(|_| waits) else {
        return;
    };
    if !matches!(index_status(input.state_dir, input.repo, &sha), IndexStatus::Absent) {
        return;
    }
    // 規則の写し（`--rules`）は渡さない: 組み立てが読む rules 行 2 本は埋め込みの値で、列の写しは gate の上限の差し替え口である。
    let argv = ["index", "build", "--repo", &input.repo.display().to_string(), "--state-dir", &input.state_dir.display().to_string(), "--ref", &sha]
        .map(str::to_owned);
    let _ = spawn_self(input.state_dir, &argv);
}

/// 兄弟の待ちの元 B 1 つ（設計 row-review.md §8・[`siblings_of`] が周の頭に 1 回組む）。
pub(super) struct Kin {
    /// B の bead id。
    bead: String,
    /// 期限を測れない周か（値の末尾に `/unset`）。
    unset: bool,
    /// B が台帳の blocks で待つ祖先（推移・待たせない）。
    ancestors: BTreeSet<String>,
    /// B の終端の段の event の ts の秒（読めない周は兄弟自身の記録で解かない）。
    since: Option<u64>,
    /// B の設計 doc の path。
    doc: String,
    /// 兄弟の行 id（同じ doc・B の行を除く・(a) 同じ section と (b) B の digest を載せた ref の記録の和）。
    rows: BTreeSet<String>,
}

/// 末尾の改行を 1 つ除く（便の置き場の写しから digest を求める形と同じ・行の審査の口と同じ整え方）。
fn trimmed(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

/// 行の digest（契約 file の字と設計の節の本文・口 (B)）。
fn digest_of(input: &Input<'_>, design: &str, body: &str) -> String {
    row_review::row_digest(trimmed(body), trimmed(&review::design_material(input.repo, design)))
}

/// 兄弟の待ちの元の列を周の頭に 1 回組む（元は行の予約の導き [`reserve::derive`] の返りから段で絞る・run id の昇順・台帳も event log も読み直さない）。
///
/// 残すのは、段が Gated（落ちた Gated は判定 FAIL）の B と、段が Reviewed で判定を読めて測れなかった判定でない（FAIL か unparsed でない INCONCLUSIVE）B。
/// B の終端の後に release の印が在る B と、B の今の行の digest が直前の便の写しと違う B は外す（今の行を生成できない周は違うと数えない）。
pub(super) fn siblings_of(input: &Input<'_>, issues: &[Issue], reserved: &[reserve::Reservation], ledger: &Ledger<'_>) -> Vec<Kin> {
    let mut fallen: Vec<&reserve::Reservation> = reserved.iter().filter(|found| design_side(input.state_dir, found)).collect();
    fallen.sort_by(|left, right| left.run.cmp(&right.run));
    fallen.into_iter().filter_map(|found| kin_of(input, issues, found, ledger)).collect()
}

/// 直前の便が設計の側の終端か（Reviewed の判定が FAIL か unparsed でない INCONCLUSIVE・Gated の判定が FAIL・**段の型の網羅の match 1 本**）。
/// Failed（起動の失敗・環境）と Stopped（人の停止）と unparsed の INCONCLUSIVE は元にしない。
fn design_side(state_dir: &Path, found: &reserve::Reservation) -> bool {
    match found.stage {
        Stage::Gated => true,
        Stage::Reviewed => review::judgement_of(state_dir, &found.run).is_some_and(|judged| !review_unmeasured(&judged)),
        Stage::Failed
        | Stage::Stopped
        | Stage::Landed
        | Stage::Intake
        | Stage::Blocked
        | Stage::Spawned
        | Stage::Questioned
        | Stage::RateLimited
        | Stage::Implemented => false,
    }
}

/// 元 B 1 つを組む（B の行を指す pointer を読めない周・解けた B は `None`）。
fn kin_of(input: &Input<'_>, issues: &[Issue], found: &reserve::Reservation, ledger: &Ledger<'_>) -> Option<Kin> {
    let pointer = pointer_of(&issues.iter().find(|issue| issue.id == found.bead)?.acceptance)?;
    if released_after(&ledger.events, &found.run, &found.bead) {
        return None;
    }
    let design = format!("{}#{}", pointer.path, pointer.id);
    // 写しの digest は契約 file の写しと材料の dir の design.txt から（design.txt を読めない B は (b) と比べを持たない）。
    let copy = std::fs::read_to_string(contract_path(input.state_dir, &found.run))
        .ok()
        .zip(std::fs::read_to_string(review::review_dir(input.state_dir, &found.run).join(review::DESIGN_FILE)).ok())
        .map(|(contract, section)| row_review::row_digest(trimmed(&contract), trimmed(&section)));
    let now = ledger.materials.as_ref().ok().and_then(|materials| generated(input.repo, &pointer, materials).ok()).map(|(_, body)| digest_of(input, &design, &body));
    if copy.is_some() && now.is_some() && copy != now {
        return None;
    }
    let mut rows = same_section(input.repo, &pointer);
    if let Some(digest) = &copy {
        let prefix = format!("{}#", pointer.path);
        rows.extend(row_review::siblings(input.state_dir, &design, digest).0.iter().filter_map(|row| row.strip_prefix(&prefix)).map(str::to_owned));
    }
    rows.remove(&pointer.id);
    Some(Kin { bead: found.bead.clone(), unset: found.unset, ancestors: found.ancestors.clone(), since: found.since, doc: pointer.path, rows })
}

/// main の先端の doc の契約表で、`pointer` の行と同じ section の行 id（B の行を含む・表を読めない周は空）。
fn same_section(repo: &Path, pointer: &Pointer) -> BTreeSet<String> {
    let rows = show_head(repo, &pointer.path).and_then(|text| read_rows(&pointer.path, &text).ok()).unwrap_or_default();
    let Some(section) = rows.iter().find(|row| row.id == pointer.id).map(|row| row.section.clone()) else {
        return BTreeSet::new();
    };
    rows.into_iter().filter(|row| row.section == section).map(|row| row.id).collect()
}

/// 候補を待たせる元 B の値（無ければ `None`）。元は run id の昇順で最初に当たる 1 つ。B 自身でも B の祖先でもなく、候補の設計 pointer の doc が
/// B の doc で行 id が兄弟の集合に在り、兄弟自身の記録で解けていない。解くのは、B の終端より後の今の digest の記録のうち判定が PASS のもの
/// か、basis が forecast か partial で判定が INCONCLUSIVE・理由の型が unparsed でないもの（読めない記録は無いものとして数える）。
fn sibling_of(input: &Input<'_>, bead: &str, pointer: &Pointer, made: (&Contract, &str), kins: &[Kin]) -> Option<String> {
    let hits: Vec<&Kin> = kins
        .iter()
        .filter(|kin| kin.bead != bead && kin.doc == pointer.path && kin.rows.contains(&pointer.id) && !kin.ancestors.contains(bead))
        .collect();
    if hits.is_empty() {
        return None;
    }
    let digest = digest_of(input, &made.0.design, made.1);
    let (records, _) = row_review::listed(input.state_dir, &format!("{}#{}", pointer.path, pointer.id), &digest);
    let closes = |record: &row_review::Listed| {
        record.verdict == Verdict::Pass
            || (record.basis != Basis::Actual && record.verdict == Verdict::Inconclusive && record.kind.is_some_and(|kind| kind != review::FindingKind::Unparsed))
    };
    let resolved = |kin: &Kin| kin.since.is_some_and(|since| records.iter().any(|record| record.at > since && closes(record)));
    hits.into_iter().find(|kin| !resolved(kin)).map(|kin| if kin.unset { format!("{}/unset", kin.bead) } else { kin.bead.clone() })
}

/// 順序を守って交差と枠を測り、起こす便と待つ便に分ける。
///
/// **1 周で起こした便は次の候補の交差の相手に入る**（設計 §3）: 起こした契約の write-set を live 側に足して
/// 次を測る（同じ [`overlaps`] の 1 実装で測る・C2）。
pub(super) fn settle(
    input: &Input<'_>,
    candidates: Vec<Candidate>,
    ready: &BTreeMap<String, (Pointer, Contract)>,
    reserved: &[reserve::Reservation],
    materials: Option<&Materials>,
) -> Turn {
    let room = materials.map(|found| Room {
        materials: found,
        sizes: sizes_of(input.manifest),
        slots: host_slots_dir(input.state_dir),
    });
    let mut started: Vec<(String, Vec<String>)> = Vec::new();
    let mut turn =
        Turn { candidates: Vec::new(), launches: Vec::new(), revives: Vec::new(), unmeasured: None, drive: None, vessel: None, lifecycle: None, triage: None };
    for mut candidate in candidates {
        if let (Some((pointer, contract)), Some(room)) = (ready.get(&candidate.bead), room.as_ref()) {
            // 行の予約は交差と枠より先（設計 row-review.md §7）。
            let held = reserve::held(reserved, &candidate, &contract.write_set, room.materials.tracked());
            match held.map(WaitReason::Reserved).or_else(|| blocker(input, contract, room, &started)) {
                Some(reason) => candidate.reason = Some(reason),
                None => {
                    started.push((candidate.bead.clone(), contract.write_set.clone()));
                    turn.launches.push(launch_of(input, &candidate.bead, pointer));
                }
            }
        }
        turn.candidates.push(candidate);
    }
    turn
}

/// 交差と枠を測るのに 1 周ぶん固定な材料（候補ごとに読み直さない・設計 §5）。
struct Room<'a> {
    /// base の走査（tracked / sources / snapshots / 契約表の facts）。
    materials: &'a Materials,
    /// 受付の枠の式の 2 線。
    sizes: Sizes,
    /// host の枠の札の置き場。
    slots: std::path::PathBuf,
}

/// 交差・余地・枠のうち最初に落ちた理由（通れば `None`）。**判定は受付の関数をそのまま撃つ**（記帳なし）。
fn blocker(
    input: &Input<'_>,
    contract: &Contract,
    room: &Room<'_>,
    started: &[(String, Vec<String>)],
) -> Option<WaitReason> {
    let tracked = room.materials.tracked();
    for (bead, write_set) in started {
        let crossed = overlaps(&contract.write_set, write_set, tracked);
        if !crossed.is_empty() {
            return Some(WaitReason::Overlap { with: bead.clone(), files: crossed.into_iter().map(|(mine, _)| mine).collect() });
        }
    }
    match crossings(input.state_dir, contract, tracked) {
        Ok(found) => {
            if let Some((run, files)) = found.runs.into_iter().find(|(_, files)| !files.is_empty()) {
                return Some(WaitReason::Overlap { with: run, files });
            }
        }
        Err(denial) => return Some(WaitReason::Admission { reason: denial.name }),
    }
    // 余地は受付の判定をそのまま撃つ。置き場は渡さない——交差は上で [`crossings`] が測り済みで、
    // 同じ周に 2 度測ると store を 2 度読むだけになる（重複 run の検査も run を作らない列には要らない）。
    // lock の前の読みは渡さない＝judge が freeze を撃ち、base の木は撃たない（入口の断りは立てない・設計 pipeline.md §56 形 7）。
    let material = Material {
        repo: input.repo,
        manifest: input.manifest,
        contract,
        state_dir: None,
        bead: "",
        materials: room.materials,
        early: None,
    };
    if let Some(denial) = judge(&material).denials.first() {
        return Some(WaitReason::Admission { reason: denial.name });
    }
    if !admission::has_room(&room.slots, 1, room.sizes) {
        return Some(WaitReason::Admission { reason: SLOT });
    }
    None
}

/// 起動の構築点（`pipe run` の引数を組む・**撃たない**）。
pub(super) fn launch_of(input: &Input<'_>, bead: &str, pointer: &Pointer) -> Launch {
    let mut argv = vec![
        "run".to_owned(),
        "--design".to_owned(),
        format!("{}#{}", pointer.path, pointer.id),
        "--bead".to_owned(),
        bead.to_owned(),
        "--repo".to_owned(),
        input.repo.display().to_string(),
        "--state-dir".to_owned(),
        input.state_dir.display().to_string(),
    ];
    argv.extend(tools(input));
    // **列が起こす便は必ず自走する**（設計 §5・[`DRIVE`]）。
    argv.push(DRIVE.to_owned());
    Launch { bead: bead.to_owned(), argv }
}

/// 列に渡された道具（起こす便と起こし直す便へ**そのまま全部**渡す＝列と便が同じ道具で動く）。
///
/// 渡されていない道具は何も足さない（既定は便の側が持つ）。**台帳 client（`--bd`）も渡す**: 起こした子
/// （`pipe run` / `pipe resume`）自身も終端で 1 周撃つので、落とすと子の 1 周が既定の台帳を読み、
/// **1 hop で列の名指した台帳と食い違う**。道具の受け渡しは全部か皆無かで、1 つだけ落とすと「同じ道具で
/// 動く」が静かに破れる（`s2-07l.366` の lens の実測）。
pub(super) fn tools(input: &Input<'_>) -> Vec<String> {
    [
        ("--rules", input.rules),
        ("--lens", input.lens),
        ("--runner", input.runner),
        ("--bd", input.bd_flag),
        ("--curl", input.curl),
    ]
        .into_iter()
        .filter_map(|(name, value)| value.map(|found| [name.to_owned(), found.to_owned()]))
        .flatten()
        .collect()
}

/// 順序を止める依存か（`blocks` の未 closed だけ・所属〔`parent-child`〕は順序ではない・`.beads/PRIME.md` R2）。
///
/// 依存の要素は status を持たない（`seat::ledger::Dep`）ので、**同じ一覧の中の依存先**で閉じたかを引く。
/// 一覧（`--all`＝closed も含む）に依存先が居ない周は閉じたと読まない（測れないを「通った」に倒さない・C10）。
pub(super) fn is_blocking(dep: &Dep, closed: &BTreeSet<&str>) -> bool {
    dep.kind == BLOCKS && !closed.contains(dep.on.as_str())
}

/// acceptance の `design = <doc>#<id>` の行から設計 pointer を引く（受付の `--design` と同じ字面・同じ parse）。
pub(super) fn pointer_of(acceptance: &str) -> Option<Pointer> {
    let line = acceptance.lines().map(str::trim).find_map(|line| line.strip_prefix(DESIGN_KEY))?;
    table::parse_pointer(line.trim()).ok()
}

/// 同じ契約 file の sha で**終端に着いた**便が在れば、その便の sha と段（設計 §2「終端の便は列外」）。
///
/// 突き合わせるのは**便の写しの中身**である（sha は名札）。終端かは受付と同じ 1 本（[`live`]）で判じ、
/// 測れない周（`None`）はここで外さない——その便は交差の検査が `WriteSetUnreadable` で断る側に倒す。
/// `git hash-object` を撃てない周は sha を測れないので列外にしない（`generated` が base を読めている＝
/// git は撃てているので、実際には到達しない）。
///
/// **列へ戻す印**（設計 §12・`s2-07l.495`）: 直前の便が終端でも、その便の最後の記帳より**後**に同じ bead
/// への `release` が在る周は列外にしない（[`released_after`]・材料は event log の並びだけ・新しい event kind
/// も field も足さない・C17.1）。起こし直した便は新しい run id を持ち、その記帳は `release` より後に並ぶ
/// ので、同じ sha でまた終端に着けば再び列外になる＝**印 1 回で起き直るのは 1 回**（§2 の無限再起動を
/// 開け直さない）。戻す段は [`requeues`] が段の型の網羅の match 1 本で決める。
///
/// **審査役へ渡る材料も鍵に入る**（設計 §16・`s2-07l.495`）: `Reviewed` で終端した便（審査 FAIL / INCONCLUSIVE）
/// は、行の `section` が指す § の本文を審査役が読んだ。直前の便の材料の dir に在る § の写しと、いま base から
/// 読んだ § の本文（読みは審査と同じ 1 本・[`review::design_material`]）が違う周は列外にしない＝**同じ材料 →
/// 同じ判定**が鍵の意味である。写しが無い / 読めない周は契約 file だけの鍵に倒す（[`section_moved`]）。
/// § を鍵に入れる段は [`section_keyed`] が段の型の網羅の match 1 本で決める。
pub(super) fn settled(input: &Input<'_>, bead: &str, body: &str, design: &str, events: &[Event]) -> Option<(String, Stage)> {
    let state = current(input.state_dir).ok()?;
    // **直前の便から見る**（run id は `<bead>-<UTC の秒>` ＝ id の昇順が時系列なので、逆順が新しい側）。
    // 同じ契約 file を持つ最初の 1 本だけを見る——古い便の終端は、その後起こし直した同じ契約を塞がない。
    let (id, stage, path) = state.runs.iter().rev().filter(|(_, run)| run.bead == bead).find_map(|(id, run)| {
        let path = contract_path(input.state_dir, id);
        std::fs::read_to_string(&path).is_ok_and(|found| found == body).then_some((id, run.stage, path))
    })?;
    if live(input.state_dir, id, stage) != Some(false) {
        return None;
    }
    if requeues(stage) && released_after(events, id, bead) {
        return None;
    }
    // **判定で引く戻し**（設計 §22）: 審査を測れなかった `Reviewed` の便だけは印で戻す（段で引く戻しの次・§ の鍵の前）。
    if stage == Stage::Reviewed
        && released_after(events, id, bead)
        && review::judgement_of(input.state_dir, id).is_some_and(|found| review_unmeasured(&found))
    {
        return None;
    }
    if section_keyed(stage) && section_moved(input, id, design) {
        return None;
    }
    let sha = git_bytes(input.repo, &["hash-object", "--", &path.display().to_string()])?;
    String::from_utf8(sha).ok().map(|found| (found.trim().to_owned(), stage))
}

/// 列外の鍵に § の本文を含める段か（**段の型の網羅の match 1 本**・設計 §16「§ を鍵に入れるのは `Reviewed` の
/// 段だけ」）。
///
/// § はその段で審査役が読んだ材料であって、`Landed`（済んでいる・起こし直すと同じ変更をもう一度作る）とも、
/// `release` が戻す段（[`requeues`]・`Failed` / `Stopped` / `Gated`）とも関係が無い。終端に着かない段はここに
/// 届かないが、届いても含めない側に倒す。段が増えた便は compile が止めて、その段の鍵に § が要るかを決めさせる。
pub(super) fn section_keyed(stage: Stage) -> bool {
    match stage {
        Stage::Reviewed => true,
        Stage::Landed | Stage::Failed | Stage::Stopped | Stage::Gated => false,
        Stage::Intake
        | Stage::Blocked
        | Stage::Spawned
        | Stage::Questioned
        | Stage::RateLimited
        | Stage::Implemented => false,
    }
}

/// 直前の便の材料の dir に在る § の写し（[`review::DESIGN_FILE`]）と、いま base から読んだ § の本文が**違う**か。
///
/// 写しが**無い**周（審査へ届かずに終端した便）と**在るのに読めない**周は偽＝契約 file だけの鍵に倒す（今までの
/// 挙動のまま・設計 §16「材料の写しが無い周は契約 file だけの鍵に倒す」）。「無い」を「違う」と読むと、審査へ
/// 届かないまま終端する便が終端のたびに起こし直され、§2 が塞いだ無限再起動が開く（**「無い」と「違う」を
/// 畳まない**・C10・fail-closed）。突き合わせる本文は材料を書く側と同じ 1 本から出る（末尾の整え方も同じ）。
fn section_moved(input: &Input<'_>, run: &str, design: &str) -> bool {
    let copy = review::review_dir(input.state_dir, run).join(review::DESIGN_FILE);
    std::fs::read_to_string(copy).is_ok_and(|kept| kept != review::design_material(input.repo, design))
}

/// `release` で列へ戻す段か（**段の型の網羅の match 1 本**・設計 §12「戻さない段が 2 つ在る」）。
///
/// 戻すのは `Failed` / `Stopped` / gate の判定で終端になった `Gated`——gate の FAIL には flaky な歯で落ちた
/// 周が含まれ、契約の字を変えずに測り直す口が他に無い。戻さないのは `Landed`（済んでいる・起こし直すと
/// 同じ変更をもう一度作る）と審査 FAIL の `Reviewed`（FR49「中身が変わるまで列に入らない」）。終端に
/// 着かない段（[`live`] が `Some(true)` の段）はここに届かないが、届いても戻さない側に倒す（fail-closed）。
/// 段が増えた便は compile が止めて、その段を戻すかを決めさせる。
pub(super) fn requeues(stage: Stage) -> bool {
    match stage {
        Stage::Failed | Stage::Stopped | Stage::Gated => true,
        Stage::Landed | Stage::Reviewed => false,
        Stage::Intake
        | Stage::Blocked
        | Stage::Spawned
        | Stage::Questioned
        | Stage::RateLimited
        | Stage::Implemented => false,
    }
}

/// 便の終端が Landed でない 4 形か（Reviewed の判定が PASS でない・Gated の判定が FAIL・Failed・Stopped・**段の型の網羅の match 1 本**・
/// 設計 row-review.md §7 の行の予約が読む）。Reviewed と Gated は判定を読めない周も落ちた形に数えない（測れないを予約に読み替えない・
/// 生死は受付と同じ [`live`]）。
pub(super) fn fallen(state_dir: &Path, id: &str, stage: Stage) -> bool {
    match stage {
        Stage::Failed | Stage::Stopped => true,
        Stage::Gated | Stage::Reviewed => live(state_dir, id, stage) == Some(false),
        Stage::Landed
        | Stage::Intake
        | Stage::Blocked
        | Stage::Spawned
        | Stage::Questioned
        | Stage::RateLimited
        | Stage::Implemented => false,
    }
}

/// 審査の判定が「測れなかった」か（**pure**・`release` で列へ戻す判定・設計 §22）。
///
/// 真は `INCONCLUSIVE` ∧ [`review::FindingKind::Unparsed`] の対だけ——lens の出力に判定の行が無かった周で、
/// 契約に穴が在るのではない。`FAIL`（kind を問わず・FR49 の判定）と、`INCONCLUSIVE` で kind が他の 6 語
/// （審査役が材料を読んで出した理由＝契約か § を直す経路）と `PASS` は偽。`review.json` が無い / 読めない周は
/// 呼び手が `None` のまま偽に倒す（fail-closed）。
pub(super) fn review_unmeasured(judgement: &review::Judgement) -> bool {
    judgement.verdict == Verdict::Inconclusive && judgement.kind == Some(review::FindingKind::Unparsed)
}

/// 便の最後の記帳より**後**に、同じ bead への `release` が在るか（**pure**・材料は event log の並びだけ）。
///
/// 位置で引く（ts の字面は比べない）: 終端より**前**の `release` は効かない。便の記帳が 1 件も無い周は
/// 「後」を測れないので効かない側に倒す（replay に在る便は必ず記帳を持つので、実際には到達しない）。
pub(super) fn released_after(events: &[Event], run: &str, bead: &str) -> bool {
    let Some(last) = events.iter().rposition(|event| event.run == run) else {
        return false;
    };
    events.iter().skip(last + 1).any(|event| {
        event.kind == EventKind::DispatchMark && event.mark == Some(Mark::Release) && event.bead == bead
    })
}

/// 列の 1 周の判定を局面の出力の材料にする（`dispatch ls` の `reason=` と同じ名と値の字・理由の無い候補は起こす便として `launched`）。
fn judged_of(turn: &Turn) -> Vec<Judged> {
    let one = |candidate: &Candidate| {
        let (name, value) = match &candidate.reason {
            Some(reason) => {
                let text = reason.render();
                (reason.as_str().to_owned(), text.split_once(':').map(|(_, value)| value.to_owned()).unwrap_or_default())
            }
            None => ("launched".to_owned(), String::new()),
        };
        Judged { bead: candidate.bead.clone(), name, value }
    };
    turn.candidates.iter().map(one).collect()
}

/// 契機 (a): `fire` の 1 回の読み（台帳の全件）と列の判定を借りて局面の出力を全部書き直し、`Written`・`Unchanged`・`Coalesced` の外の語を返す。
pub(super) fn lifecycle_round(input: &Input<'_>, turn: &Turn, read: &Read) -> Option<&'static str> {
    let policy = LockPolicy::from_rules(input.manifest).ok()?;
    let place = Place { state_dir: input.state_dir, repo: input.repo, manifest: input.manifest, bd: input.bd, policy };
    lifecycle::round(&place, Source::Borrowed(Round { issues: &read.issues, judged: judged_of(turn) }))
}

/// 観測の 1 周（`dispatch ls` と同じ判定・起こさない）の結果のうち局面の出力が借りる分。
pub struct Observed {
    /// 台帳の全件。
    pub issues: Vec<Issue>,
    /// 列の 1 周の判定。
    pub judged: Vec<Judged>,
}

/// 観測の 1 周を置き場・repo・rules・台帳 client から撃つ（起こさない・契機 (d)(e) の全部の書き直しが列の判定を得る口）。
/// 台帳を読めない周は理由を返す。
pub fn observe_round(state_dir: &Path, repo: &Path, manifest: &Manifest, bd: &str) -> Result<Observed, Unmeasured> {
    let input = Input { state_dir, repo, manifest, bd, bd_flag: None, rules: None, lens: None, curl: None, runner: None, driving: None, driven: None };
    let (turn, read) = measure(&input);
    match (turn.unmeasured, read) {
        (Some(reason), _) => Err(reason),
        (None, None) => Err(Unmeasured::Ledger),
        (None, Some(read)) => Ok(Observed { judged: judged_of(&turn), issues: read.issues }),
    }
}

/// 受付の枠の式の 2 線（読めない行は 0＝[`admission::has_room`] が `Free::Unmeasured` で待たせない側に倒す）。
fn sizes_of(manifest: &Manifest) -> Sizes {
    let row = |id: &str| int_row(manifest, id).unwrap_or(0);
    Sizes { job_mb: row(ROW_JOB_MB), reserve_mb: row(ROW_RESERVE_MB) }
}

/// bead ごとの印を畳む（`release` は介入の印も起こした事実の印も外す・`RunCreated` は起こした事実の印を外す・
/// **pure**・設計 §4・§17）。
pub(super) fn marks_of(events: &[Event]) -> Marks {
    let mut found = Marks { order: BTreeMap::new(), launched: BTreeMap::new() };
    for event in events {
        if event.kind == EventKind::RunCreated {
            found.launched.remove(&event.bead);
            continue;
        }
        let (EventKind::DispatchMark, Some(mark)) = (event.kind, event.mark) else {
            continue;
        };
        match mark {
            Mark::Release => {
                found.order.remove(&event.bead);
                found.launched.remove(&event.bead);
            }
            Mark::First | Mark::Hold => {
                found.order.insert(event.bead.clone(), (mark, event.ts.clone()));
            }
            Mark::Launched => {
                found.launched.insert(event.bead.clone(), event.ts.clone());
            }
        }
    }
    found
}
