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
use super::super::refuse::overlaps;
use super::super::review;
use super::super::table::{self, Pointer};
use super::super::{contract_path, current, git_bytes};
use super::{
    Candidate, Input, Launch, Ledger, Marks, Turn, WaitReason, BLOCKS, DESIGN_KEY, DRIVE, MARK, MEMO_LABEL, OPEN,
    ROW_JOB_MB, ROW_RESERVE_MB, SLOT,
};
use crate::fleet::{Event, EventKind, Mark, Stage};
use crate::rules::manifest::Manifest;
use crate::seat::host_slots_dir;
use crate::seat::ledger::{Dep, Issue};
use std::collections::{BTreeMap, BTreeSet};

/// 列の入力になる bead か（設計 §2・**ここで落ちた bead は `ls` にも出ない**＝契約が未確定か終わっている）。
pub(super) fn is_input(issue: &Issue) -> bool {
    issue.status == OPEN
        && !issue.acceptance.trim().is_empty()
        && !issue.labels.iter().any(|label| label == MEMO_LABEL)
}

/// 台帳の 1 件を列の 1 件に解く（依存 → 印 → 設計 pointer → 審査 FAIL → 契約の生成の順）。
///
/// 交差と枠は**順序の後**に測る（§3「1 周で起こした便は次の候補の交差の相手」）ので、ここでは決めない。
/// 理由の付かなかった候補だけが設計 pointer と契約を持って返る。
pub(super) fn entry_of(input: &Input<'_>, issue: &Issue, ledger: &Ledger<'_>) -> (Candidate, Option<(Pointer, Contract)>) {
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
            None => found,
        },
        Err(denial) => return wait(WaitReason::Admission { reason: denial.name }),
    };
    (at(None), Some((pointer, contract)))
}

/// 順序を守って交差と枠を測り、起こす便と待つ便に分ける。
///
/// **1 周で起こした便は次の候補の交差の相手に入る**（設計 §3）: 起こした契約の write-set を live 側に足して
/// 次を測る（同じ [`overlaps`] の 1 実装で測る・C2）。
pub(super) fn settle(
    input: &Input<'_>,
    candidates: Vec<Candidate>,
    ready: &BTreeMap<String, (Pointer, Contract)>,
    materials: Option<&Materials>,
) -> Turn {
    let room = materials.map(|found| Room {
        materials: found,
        sizes: sizes_of(input.manifest),
        slots: host_slots_dir(input.state_dir),
    });
    let mut started: Vec<(String, Vec<String>)> = Vec::new();
    let mut turn =
        Turn { candidates: Vec::new(), launches: Vec::new(), revives: Vec::new(), unmeasured: None, drive: None, vessel: None };
    for mut candidate in candidates {
        if let (Some((pointer, contract)), Some(room)) = (ready.get(&candidate.bead), room.as_ref()) {
            match blocker(input, contract, room, &started) {
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
            return Some(WaitReason::Overlap { with: bead.clone(), files: crossed.len() });
        }
    }
    match crossings(input.state_dir, contract, tracked) {
        Ok(found) => {
            if let Some((run, files)) = found.runs.iter().find(|(_, files)| !files.is_empty()) {
                return Some(WaitReason::Overlap { with: run.clone(), files: files.len() });
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
fn is_blocking(dep: &Dep, closed: &BTreeSet<&str>) -> bool {
    dep.kind == BLOCKS && !closed.contains(dep.on.as_str())
}

/// acceptance の `design = <doc>#<id>` の行から設計 pointer を引く（受付の `--design` と同じ字面・同じ parse）。
fn pointer_of(acceptance: &str) -> Option<Pointer> {
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
