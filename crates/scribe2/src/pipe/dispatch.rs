//! `pipe dispatch` — 審査を通った契約を器が自動で起こす列（設計 docs/design/dispatcher.md §2〜§4・§6）。
//!
//! 起動に要る判定は**器が既に持っている**（台帳の依存・live 便との write-set の交差・受付の余地と host の
//! memory・審査の verdict）。本 module はそれを読み直して並べるだけで、**判定を 2 本目に実装しない**
//! （憲法 C2）: 交差は受付の [`crate::pipe::cli::crossings`]、余地は受付の [`crate::pipe::cli::judge`]、
//! 枠は [`admission::has_room`] を**記帳せずに**撃つ。
//!
//! 順序は 1 関数 [`order`] だけが持つ（散文の順序を持たない・設計 §2）: (1) 介入 `first` (2) 台帳の
//! `priority`（P0 → P4）(3) 起票順（id の数字）。
//!
//! 台帳を読めない周は列を空と読まず [`Unmeasured`] で 1 本も起こさない（`0 件`と融合しない・C10・NFR4）。

use super::admission::{self, Sizes};
use super::cli::{ceiling_of, crossings, generated, int_row, judge, Denial, Material, Rows};
use super::contract::Contract;
use super::refuse::overlaps;
use super::review::ReviewCheck;
use super::table::{self, Pointer};
use super::{contract_path, current, git_bytes};
use crate::cli_outcome::Outcome;
use crate::fleet::store;
use crate::fleet::{Event, EventKind, Mark};
use crate::rules::manifest::Manifest;
use crate::seat::host_slots_dir;
use crate::seat::ledger::{self, Dep, Issue};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// `intake:memo` の bead（契約が未確定＝列に載せない・`.beads/PRIME.md` R3）。
const MEMO_LABEL: &str = "intake:memo";

/// 台帳の閉じた status の字面（依存が閉じたかの判定が読む）。
const CLOSED: &str = "closed";

/// 列の入力になる status の字面（`in_progress` の bead は既に走っている便が持つ）。
const OPEN: &str = "open";

/// 順序を決める依存の種別（`parent-child` は所属であって順序ではない・`.beads/PRIME.md` R2）。
const BLOCKS: &str = "blocks";

/// acceptance が持つ設計 pointer の行の書き出し（受付の `--design` と**同じ字面**・設計 §2）。
const DESIGN_KEY: &str = "design = ";

/// job 1 つが要る memory の rules 行（値は読むだけ・憲法 C5）。
const ROW_JOB_MB: &str = "gate.job_memory_mb";

/// 席と host のために残す memory の rules 行。
const ROW_RESERVE_MB: &str = "host.reserve_memory_mb";

/// 枠が空いていない周の理由の字面。
const SLOT: &str = "slot";

/// [`WaitReason`] の全 variant の名（宣言順・`enum-slices` が集合完全性を測る）。
pub const WAIT_REASONS: &[&str] =
    &["dependency", "overlap", "admission", "hold", "review-failed", "no-design-pointer"];

/// 列に載ったのに起こさない理由（**閉じた型**・設計 §3 の表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitReason {
    /// 台帳の依存が閉じていない。
    Dependency {
        /// 閉じていない依存先の bead id（台帳の順）。
        on: Vec<String>,
    },
    /// live な便と write-set が交差する。
    Overlap {
        /// 交差した相手の run id（先頭の 1 本）。
        with: String,
        /// その相手と交差した契約側の file の本数。
        files: usize,
    },
    /// 受付（余地・host の memory）を通らない。
    Admission {
        /// 受付が断った名（[`crate::pipe::refuse::Refuse::as_str`] か [`SLOT`]・どちらも `'static`）。
        reason: &'static str,
    },
    /// 介入 `hold` が付いている。
    Hold {
        /// 印を付けた event の ts。
        since: String,
    },
    /// 同じ契約で審査 FAIL / INCONCLUSIVE に終わった便が在る（契約 file の sha が変わっていない）。
    ReviewFailed {
        /// 審査に落ちた便の契約 file の sha。
        sha: String,
    },
    /// acceptance に設計 pointer の行が無い。
    NoDesignPointer,
}

impl WaitReason {
    /// 一覧と pin が読む名（kebab・宣言順は [`WAIT_REASONS`]）。
    pub fn as_str(&self) -> &'static str {
        match *self {
            Self::Dependency { .. } => "dependency",
            Self::Overlap { .. } => "overlap",
            Self::Admission { .. } => "admission",
            Self::Hold { .. } => "hold",
            Self::ReviewFailed { .. } => "review-failed",
            Self::NoDesignPointer => "no-design-pointer",
        }
    }

    /// `dispatch ls` の `reason=` に書く字面（名 + 値・値を持たない variant は名だけ）。
    pub fn render(&self) -> String {
        let name = self.as_str();
        match *self {
            Self::Dependency { ref on } => format!("{name}:{}", on.join(",")),
            Self::Overlap { ref with, files } => format!("{name}:{with}/{files}"),
            Self::Admission { reason } => format!("{name}:{reason}"),
            Self::Hold { ref since } => format!("{name}:{since}"),
            Self::ReviewFailed { ref sha } => format!("{name}:{sha}"),
            Self::NoDesignPointer => name.to_owned(),
        }
    }
}

/// 列の 1 件（台帳の bead + 介入の印 + 起こさない理由）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// bead id。
    pub bead: String,
    /// 台帳の priority（P0 → P4・読めない周は `None`＝最後尾）。
    pub priority: Option<u64>,
    /// 介入の印（`release` は印を外すので `None` に戻る）。
    pub mark: Option<Mark>,
    /// 起こさない理由（`None` = 起こせる）。
    pub reason: Option<WaitReason>,
}

/// 台帳を読めなかった理由（**閉じた型**・`0 件`と融合しない・C10）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmeasured {
    /// 待ち上限の rules 行が不発効・別の形・不在。
    NoRule,
    /// 台帳の子 process が起動できない・rc 非 0・JSON 不能・待ち上限超過。
    Ledger,
}

impl Unmeasured {
    /// `[DISPATCH-UNMEASURED reason=…]` に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoRule => "no-rule",
            Self::Ledger => "ledger",
        }
    }
}

/// 起動の構築点（`pipe run` の引数まで組んだ 1 件・**撃つのは契機の便**〔設計 §5・契約 (b)〕）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// bead id。
    pub bead: String,
    /// `pipe` に続く引数（`run --design <pointer> --bead … --repo … --state-dir …`）。設計 pointer は
    /// この列の中に在る（同じ値を 2 つの field で持たない）。
    pub argv: Vec<String>,
}

/// 列の 1 周の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// 列の全件（[`order`] の順・`reason` が `None` の件が起こせる便・`dispatch ls` はこれを描く）。
    pub candidates: Vec<Candidate>,
    /// 起こす便の構築点（`candidates` の `reason` が `None` の件と同じ順・同じ本数）。
    pub launches: Vec<Launch>,
    /// 台帳を読めなかった周の理由（`Some` なら他の 2 つは空）。
    pub unmeasured: Option<Unmeasured>,
}

/// 列の 1 周に要る材料（すべて永続面から解いたもの・process の記憶を持たない）。
pub struct Input<'a> {
    /// 置き場。
    pub state_dir: &'a Path,
    /// 対象 repo（anchor・base = `HEAD`）。
    pub repo: &'a Path,
    /// 規則の値。
    pub manifest: &'a Manifest,
    /// 台帳 client（`--bd` か [`ledger::DEFAULT_BD`]）。
    pub bd: &'a str,
}

/// 列を 1 周する（**判定は器の既存の関数・記帳はしない**）。
///
/// 台帳を読めない周は [`Unmeasured`] を持って返り、1 本も起こさない（fail-closed・設計 §7）。
pub fn turn(input: &Input<'_>) -> Turn {
    let Some(timeout) = ledger::timeout_of(input.manifest) else {
        return unmeasured(Unmeasured::NoRule);
    };
    let Ok(issues) = ledger::read_ledger(input.bd, timeout) else {
        return unmeasured(Unmeasured::Ledger);
    };
    // 1 周ぶん固定な材料は**ここで 1 回だけ**解く（候補ごとに rules 行と台帳を読み直さない）。
    let ledger = Ledger {
        marks: marks_of(&read_events(input.state_dir)),
        closed: issues.iter().filter(|issue| issue.status == CLOSED).map(|issue| issue.id.as_str()).collect(),
        ceiling: ceiling_of(input.manifest),
    };
    let mut ready: BTreeMap<String, (Pointer, Contract)> = BTreeMap::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    for issue in issues.iter().filter(|issue| is_input(issue)) {
        let (candidate, found) = entry_of(input, issue, &ledger);
        if let Some(entry) = found {
            ready.insert(candidate.bead.clone(), entry);
        }
        candidates.push(candidate);
    }
    // **順序は [`order`] の 1 本だけが決める**（生産経路も歯も同じ関数を通る・C2）。
    settle(input, order(candidates), &ready)
}

/// 1 周ぶん固定な台帳側の材料（候補ごとに読み直さない）。
struct Ledger<'a> {
    /// bead ごとの最後の印。
    marks: BTreeMap<String, (Mark, String)>,
    /// 閉じた bead の id（依存が閉じたかを同じ一覧の中で引く）。
    closed: BTreeSet<&'a str>,
    /// 契約表の検査の上限（読めない周は断りの名を全候補が受ける）。
    ceiling: Result<Rows, Denial>,
}

/// 列の順序を決める 1 関数（**pure**・設計 §2「順序」）: (1) 介入 `first` (2) 台帳の `priority`（P0 → P4）
/// (3) 起票順（id の数字）。同順は id の辞書順（全順序ゆえ待ちは循環しない）。
pub fn order(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by_key(key_of);
    candidates
}

/// 並べ替えの鍵（`first` は 0・priority は読めない周を最後尾に倒す・id は数字の列で比べる）。
fn key_of(candidate: &Candidate) -> (u8, u64, Vec<u64>, String) {
    let first = u8::from(candidate.mark != Some(Mark::First));
    let priority = candidate.priority.unwrap_or(u64::MAX);
    (first, priority, digits_of(&candidate.bead), candidate.bead.clone())
}

/// id の数字の列（`s2-07l.345` → `[2, 7, 345]`・起票順の鍵・数字を持たない id は空）。
fn digits_of(bead: &str) -> Vec<u64> {
    bead.split(|glyph: char| !glyph.is_ascii_digit()).filter_map(|run| run.parse().ok()).collect()
}

/// 台帳を読めなかった周の 1 周（1 本も起こさない）。
fn unmeasured(reason: Unmeasured) -> Turn {
    Turn { candidates: Vec::new(), launches: Vec::new(), unmeasured: Some(reason) }
}

/// 列の入力になる bead か（設計 §2・**ここで落ちた bead は `ls` にも出ない**＝契約が未確定か終わっている）。
fn is_input(issue: &Issue) -> bool {
    issue.status == OPEN
        && !issue.acceptance.trim().is_empty()
        && !issue.labels.iter().any(|label| label == MEMO_LABEL)
}

/// 台帳の 1 件を列の 1 件に解く（依存 → 印 → 設計 pointer → 審査 FAIL → 契約の生成の順）。
///
/// 交差と枠は**順序の後**に測る（§3「1 周で起こした便は次の候補の交差の相手」）ので、ここでは決めない。
/// 理由の付かなかった候補だけが設計 pointer と契約を持って返る。
fn entry_of(input: &Input<'_>, issue: &Issue, ledger: &Ledger<'_>) -> (Candidate, Option<(Pointer, Contract)>) {
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
    let Some(pointer) = pointer_of(&issue.acceptance) else {
        return wait(WaitReason::NoDesignPointer);
    };
    let ceiling = match &ledger.ceiling {
        Ok(found) => found.borrow(),
        Err(denial) => return wait(WaitReason::Admission { reason: denial.name }),
    };
    let contract = match generated(input.repo, &pointer, &ceiling) {
        Ok((found, body)) => match failed_review(input, &issue.id, &body) {
            Some(sha) => return wait(WaitReason::ReviewFailed { sha }),
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
fn settle(input: &Input<'_>, candidates: Vec<Candidate>, ready: &BTreeMap<String, (Pointer, Contract)>) -> Turn {
    let room = Room {
        tracked: table::tracked_files(input.repo).unwrap_or_default(),
        sizes: sizes_of(input.manifest),
        slots: host_slots_dir(input.state_dir),
    };
    let mut started: Vec<(String, Vec<String>)> = Vec::new();
    let mut turn = Turn { candidates: Vec::new(), launches: Vec::new(), unmeasured: None };
    for mut candidate in candidates {
        if let Some((pointer, contract)) = ready.get(&candidate.bead) {
            match blocker(input, contract, &room, &started) {
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

/// 交差と枠を測るのに 1 周ぶん固定な材料（候補ごとに読み直さない）。
struct Room {
    /// base の tracked file（交差の dir の展開が読む）。
    tracked: Vec<String>,
    /// 受付の枠の式の 2 線。
    sizes: Sizes,
    /// host の枠の札の置き場。
    slots: std::path::PathBuf,
}

/// 交差・余地・枠のうち最初に落ちた理由（通れば `None`）。**判定は受付の関数をそのまま撃つ**（記帳なし）。
fn blocker(
    input: &Input<'_>,
    contract: &Contract,
    room: &Room,
    started: &[(String, Vec<String>)],
) -> Option<WaitReason> {
    for (bead, write_set) in started {
        let crossed = overlaps(&contract.write_set, write_set, &room.tracked);
        if !crossed.is_empty() {
            return Some(WaitReason::Overlap { with: bead.clone(), files: crossed.len() });
        }
    }
    match crossings(input.state_dir, contract, &room.tracked) {
        Ok(found) => {
            if let Some((run, files)) = found.runs.iter().find(|(_, files)| !files.is_empty()) {
                return Some(WaitReason::Overlap { with: run.clone(), files: files.len() });
            }
        }
        Err(denial) => return Some(WaitReason::Admission { reason: denial.name }),
    }
    // 余地は受付の判定をそのまま撃つ。置き場は渡さない——交差は上で [`crossings`] が測り済みで、
    // 同じ周に 2 度測ると store を 2 度読むだけになる（重複 run の検査も run を作らない列には要らない）。
    let material =
        Material { repo: input.repo, manifest: input.manifest, contract, state_dir: None, bead: "" };
    if let Some(denial) = judge(&material).denials.first() {
        return Some(WaitReason::Admission { reason: denial.name });
    }
    if !admission::has_room(&room.slots, 1, room.sizes) {
        return Some(WaitReason::Admission { reason: SLOT });
    }
    None
}

/// 起動の構築点（`pipe run` の引数を組む・**撃たない**）。
fn launch_of(input: &Input<'_>, bead: &str, pointer: &Pointer) -> Launch {
    let argv = vec![
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
    Launch { bead: bead.to_owned(), argv }
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

/// 同じ契約で審査に落ちた便が在れば、その便の契約 file の sha（設計 §2「審査 FAIL の列外」）。
///
/// 突き合わせるのは**便の写しの中身**である（sha は名札）。`git hash-object` を撃てない周は sha を測れない
/// ので列外にしない（測れないを「落ちた」に読み替えない・C10）。
fn failed_review(input: &Input<'_>, bead: &str, body: &str) -> Option<String> {
    let state = current(input.state_dir).ok()?;
    // **直前の便から見る**（run id は `<bead>-<UTC の秒>` ＝ id の昇順が時系列なので、逆順が新しい側）。
    // 同じ契約 file を持つ最初の 1 本だけを見る——古い便の FAIL は、その後 PASS した同じ契約を塞がない。
    let (id, path) = state.runs.iter().rev().filter(|(_, run)| run.bead == bead).find_map(|(id, _)| {
        let path = contract_path(input.state_dir, id);
        std::fs::read_to_string(&path).is_ok_and(|found| found == body).then_some((id, path))
    })?;
    if !matches!(ReviewCheck::judge(input.state_dir, id), ReviewCheck::Stopped(_)) {
        return None;
    }
    let sha = git_bytes(input.repo, &["hash-object", "--", &path.display().to_string()])?;
    String::from_utf8(sha).ok().map(|found| found.trim().to_owned())
}

/// 受付の枠の式の 2 線（読めない行は 0＝[`admission::has_room`] が `Free::Unmeasured` で待たせない側に倒す）。
fn sizes_of(manifest: &Manifest) -> Sizes {
    let row = |id: &str| int_row(manifest, id).unwrap_or(0);
    Sizes { job_mb: row(ROW_JOB_MB), reserve_mb: row(ROW_RESERVE_MB) }
}

/// 置き場の event を全部読む（読めない周は空＝印が無い周と同じ扱い・印は「起こさない」側の材料だけ）。
fn read_events(state_dir: &Path) -> Vec<Event> {
    store::read_all(state_dir).unwrap_or_default()
}

/// bead ごとの**最後の**印（`release` は印を外す・**pure**・設計 §4）。
fn marks_of(events: &[Event]) -> BTreeMap<String, (Mark, String)> {
    let mut found: BTreeMap<String, (Mark, String)> = BTreeMap::new();
    for event in events.iter().filter(|event| event.kind == EventKind::DispatchMark) {
        let Some(mark) = event.mark else {
            continue;
        };
        match mark {
            Mark::Release => {
                found.remove(&event.bead);
            }
            Mark::First | Mark::Hold => {
                found.insert(event.bead.clone(), (mark, event.ts.clone()));
            }
        }
    }
    found
}

/// 列の 1 行の書き出し（設計 §6）。
const LINE: &str = "[DISPATCH]";

/// 件数の行の書き出し。
const COUNT: &str = "[DISPATCH-COUNT]";

/// 列が空の周の行（**台帳を読めない周と融合しない**・C10）。
const NONE_LINE: &str = "[DISPATCH-NONE]";

/// 値を持たない欄の字面（priority が読めない・理由が無い・印が無い）。
const DASH: &str = "-";

/// `pipe dispatch` の使い方。
pub fn usage() -> String {
    format!(
        "usage: {} pipe dispatch <ls|first|hold|release> [BEAD] [--state-dir D] [--repo R] [--bd PATH] [--rules PATH]",
        crate::name::NAME
    )
}

/// 列の 1 周を描く（`dispatch ls`・**観測の面はこの 1 口だけである**・設計 §6）。
pub fn render(turn: &Turn) -> Outcome {
    if let Some(reason) = turn.unmeasured {
        return Outcome::ok(vec![format!("[DISPATCH-UNMEASURED reason={}]", reason.as_str())]);
    }
    if turn.candidates.is_empty() {
        return Outcome::ok(vec![NONE_LINE.to_owned()]);
    }
    let mut out: Vec<String> = turn.candidates.iter().map(line_of).collect();
    out.push(format!("{COUNT} total={} ready={}", turn.candidates.len(), turn.launches.len()));
    Outcome::ok(out)
}

/// 列の 1 件の行。
fn line_of(candidate: &Candidate) -> String {
    let prio = candidate.priority.map_or(DASH.to_owned(), |found| found.to_string());
    let mark = candidate.mark.map_or(DASH, Mark::as_str);
    let reason = candidate.reason.as_ref().map_or(DASH.to_owned(), WaitReason::render);
    format!("{LINE} bead={} prio={prio} mark={mark} reason={reason}", candidate.bead)
}

/// 介入の印を記帳する（`dispatch first|hold|release <bead>`・設計 §4）。
///
/// 印は台帳の priority を書き換えない（憲法 C15）。`release` も 1 行として残す——印を外した事実が
/// 記録から消えると「なぜこの順か」が読めなくなる（設計 §10）。
pub fn mark(state_dir: &Path, bead: &str, mark: Mark, policy: store::LockPolicy) -> Outcome {
    match super::emit_mark(state_dir, bead, mark, policy) {
        Ok(()) => Outcome::ok(vec![format!("{LINE} bead={bead} mark={}", mark.as_str())]),
        Err(err) => Outcome::failed_line(crate::cli_outcome::RC_BROKEN, format!("pipe: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{digits_of, marks_of, order, Candidate, WaitReason, WAIT_REASONS};
    use crate::fleet::{Event, EventKind, Mark, SCHEMA};

    /// 候補 1 件（印と priority だけを呼び手が選ぶ）。
    fn candidate(bead: &str, priority: Option<u64>, mark: Option<Mark>) -> Candidate {
        Candidate { bead: bead.to_owned(), priority, mark, reason: None }
    }

    /// 印の行 1 件。
    fn marked(ts: &str, bead: &str, mark: Mark) -> Event {
        Event {
            schema: SCHEMA,
            ts: ts.to_owned(),
            kind: EventKind::DispatchMark,
            run: String::new(),
            bead: bead.to_owned(),
            host: "h".to_owned(),
            actor: EventKind::DispatchMark.default_actor().to_owned(),
            stage: None,
            seat: None,
            pid: None,
            detail: None,
            allowance: None,
            registration: None,
            mark: Some(mark),
            account: None,
        }
    }

    /// 順序は (1) `first` (2) priority (3) 起票順（設計 dispatcher.md §2）。**散文の順序を持たない**ので、
    /// 3 つの軸が同時に食い違う列で 1 関数の順だけを測る。
    #[test]
    fn pipe_dispatch_order_puts_first_before_priority_then_the_issue_number() {
        let listed = vec![
            candidate("s2-07l.100", Some(0), None),
            candidate("s2-07l.99", Some(4), Some(Mark::First)),
            candidate("s2-07l.20", Some(2), None),
            candidate("s2-07l.3", Some(0), None),
            candidate("s2-07l.7", None, None),
        ];
        let sorted: Vec<String> = order(listed.clone()).into_iter().map(|found| found.bead).collect();
        assert_eq!(
            sorted,
            vec!["s2-07l.99", "s2-07l.3", "s2-07l.100", "s2-07l.20", "s2-07l.7"],
            "母集団 {} 件（first → P0 の 2 件は起票順 → P2 → priority を読めない 1 件は最後尾）",
            listed.len()
        );
    }

    /// 起票順の鍵は id の**数字の列**である（辞書順だと `.100` が `.20` より前に来る）。
    #[test]
    fn pipe_dispatch_order_reads_the_issue_number_as_digits_not_text() {
        assert_eq!(digits_of("s2-07l.345"), vec![2, 7, 345], "数字の列");
        assert_eq!(digits_of("s2-07l.479.2"), vec![2, 7, 479, 2], "小数点の先も 1 つの数");
        assert_eq!(digits_of("s2-abc"), vec![2], "数字を持たない部分は鍵に入らない");
        assert!(digits_of("s2-07l.20") < digits_of("s2-07l.100"), "辞書順なら逆になる 2 件");
    }

    /// 印は bead ごとに**最後の 1 件**が効き、`release` は印を外す（event log の往復・設計 §4）。
    #[test]
    fn pipe_dispatch_marks_keep_the_last_one_and_release_removes_it() {
        let events = vec![
            marked("t1", "s2-a", Mark::First),
            marked("t2", "s2-b", Mark::Hold),
            marked("t3", "s2-a", Mark::Hold),
            marked("t4", "s2-b", Mark::Release),
        ];
        let found = marks_of(&events);
        assert_eq!(found.get("s2-a").map(|(mark, _)| *mark), Some(Mark::Hold), "後の印が勝つ");
        assert_eq!(found.get("s2-a").map(|(_, ts)| ts.clone()), Some("t3".to_owned()), "ts は勝った印の行");
        assert_eq!(found.get("s2-b"), None, "release は印を外す");
        assert_eq!(found.len(), 1, "母集団 {} 行のうち残る印は 1 つ", events.len());
    }

    /// 理由の名は [`WAIT_REASONS`] と 1 対 1 で、値を持つ variant は値も描く（`dispatch ls` の `reason=`）。
    #[test]
    fn pipe_dispatch_wait_reasons_render_the_name_and_the_value() {
        let listed = vec![
            WaitReason::Dependency { on: vec!["s2-x".to_owned(), "s2-y".to_owned()] },
            WaitReason::Overlap { with: "r1".to_owned(), files: 2 },
            WaitReason::Admission { reason: "cap-headroom" },
            WaitReason::Hold { since: "t1".to_owned() },
            WaitReason::ReviewFailed { sha: "abc".to_owned() },
            WaitReason::NoDesignPointer,
        ];
        let names: Vec<&str> = listed.iter().map(WaitReason::as_str).collect();
        assert_eq!(names, WAIT_REASONS, "母集団 {} 件（宣言順）", WAIT_REASONS.len());
        let rendered: Vec<String> = listed.iter().map(WaitReason::render).collect();
        assert_eq!(
            rendered,
            vec![
                "dependency:s2-x,s2-y",
                "overlap:r1/2",
                "admission:cap-headroom",
                "hold:t1",
                "review-failed:abc",
                "no-design-pointer",
            ],
            "値を持つ 5 件は値も描く"
        );
    }
}
