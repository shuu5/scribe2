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
use super::cli::{crossings, generated, int_row, judge, live, Denial, Material, Materials};
use super::contract::Contract;
use super::refuse::overlaps;
use super::table::{self, Pointer};
use super::{contract_path, current, git_bytes};
use crate::cli_outcome::Outcome;
use crate::fleet::store;
use crate::fleet::{Event, EventKind, Mark, Stage};
use crate::name::NAME;
use crate::rules::manifest::Manifest;
use crate::seat::host_slots_dir;
use crate::seat::ledger::{self, Dep, Issue};
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

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

/// 子 process を起こせなかった周の理由の字面。
const SPAWN: &str = "spawn";


/// [`WaitReason`] の全 variant の名（宣言順・`enum-slices` が集合完全性を測る）。
pub const WAIT_REASONS: &[&str] = &["dependency", "overlap", "admission", "hold", "settled", "no-design-pointer"];

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
    /// 同じ契約 file の sha で**終端に着いた**便が在る（`Landed` / `Failed` / `Stopped`、審査や gate の
    /// 判定で終端になった段も含む）。契約が改訂されて sha が動けば列に戻る。
    ///
    /// `s2-07l.366` で「審査 FAIL の便」から広げた: 便が終端に着いても bead は台帳で `open` のまま
    /// （器は台帳に書かない・C15）で live な便も無いので、終端が来るたびに同じ契約が起こし直される
    /// （着地から close までの無限再起動）。
    Settled {
        /// 終端に着いた便の契約 file の sha。
        sha: String,
        /// その便の段（replay が見た最新）。
        stage: Stage,
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
            Self::Settled { .. } => "settled",
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
            Self::Settled { ref sha, stage } => format!("{name}:{sha}/{}", stage.as_str()),
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
    /// 実装役の口（`--runner`）が無い＝起こせないので列を測らない（見る口は `dispatch ls`）。
    NoRunner,
}

impl Unmeasured {
    /// `[DISPATCH-UNMEASURED reason=…]` に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoRule => "no-rule",
            Self::Ledger => "ledger",
            Self::NoRunner => "no-runner",
        }
    }
}

/// `pipe` の subcommand の書き出し（子 process の argv の先頭）。
const PIPE: &str = "pipe";

/// 起こす（通る便だけ `pipe run` を**子 process で**起こす・設計 §3・§5・契約表の行 b）。
///
/// **待たない**: 子の完了を待つと終端が次の便の全行程を待つことになる（`pipe run` は intake → 審査 →
/// spawn → gate → land の driver である）。新しい process group の leader にするのは [`super::spawn`] と
/// 同じ理由で、終端の process が畳まれても起こした便が道連れにならないためである。
///
/// 起こせなかった周は `false` を返して**その便を起こさなかった事実だけ**を残す（終端の rc は呼び手が
/// 変えない・次の契機で拾う・§5）。判定は turn の 1 回だが、`pipe run` 側の受付は外さない（二重に守る・
/// planner 裁定 2026-09-19 の条件 (2)）。
fn start(launch: &Launch) -> bool {
    spawn_self(&launch.argv)
}

/// 自分自身を `pipe <argv>` で起こす（**起こす側と起こし直す側の 1 実装**・C2）。
fn spawn_self(argv: &[String]) -> bool {
    Command::new(myself())
        .arg(PIPE)
        .args(argv)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// 自分の binary（`argv[0]`）。PATH で呼ばれた周は同じ名で子も PATH から解ける。
///
/// **`current_exe` は使わない**——`/proc/self/exe` を読むのは「器は env も HOME も読まない」（C2.2）の
/// 外側で、xtask の門が違反として数える。`argv[0]` は**呼ばれ方そのもの**なので、同じ呼ばれ方で子を起こす。
fn myself() -> String {
    std::env::args().next().unwrap_or_else(|| NAME.to_owned())
}

/// 起動の構築点（`pipe run` の引数まで組んだ 1 件・[`start`] がそのまま子 process へ渡す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// bead id。
    pub bead: String,
    /// `pipe` に続く引数（`run --design <pointer> --bead … --repo … --state-dir …`）。設計 pointer は
    /// この列の中に在る（同じ値を 2 つの field で持たない）。
    pub argv: Vec<String>,
}

/// 起こし直しの構築点（`pipe resume` の引数まで組んだ 1 件・設計 §5「driver の死亡」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revive {
    /// 便 id。
    pub run: String,
    /// `pipe` に続く引数（`resume --run <id> --repo … --state-dir …` + 列に渡された道具）。
    pub argv: Vec<String>,
}

/// 列の 1 周の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// 列の全件（[`order`] の順・`reason` が `None` の件が起こせる便・`dispatch ls` はこれを描く）。
    pub candidates: Vec<Candidate>,
    /// 起こす便の構築点（`candidates` の `reason` が `None` の件と同じ順・同じ本数）。
    pub launches: Vec<Launch>,
    /// 起こし直す便（driver の札の所有者が死んでいる live 便・run id の順）。
    pub revives: Vec<Revive>,
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
    /// 台帳 client（`--bd` か [`ledger::DEFAULT_BD`]）。列自身が読むときの値。
    pub bd: &'a str,
    /// `--bd` が**引数で名指されていた**か（起こす便へ渡すのはこちら・既定は渡さない＝便の側が持つ）。
    pub bd_flag: Option<&'a str>,
    /// 規則の写しの path（`--rules`）。起こす便へ**そのまま渡す**（列と便が同じ規則で動く）。
    pub rules: Option<&'a str>,
    /// 審査の lens の口（`--lens`）。渡された周だけ起こす便へそのまま渡す（既定は便の側が持つ）。
    pub lens: Option<&'a str>,
    /// 口座残量の計測の口（`--curl`）。起こす便と起こし直す便へそのまま渡す（既定は便の側が持つ）。
    pub curl: Option<&'a str>,
    /// 実装役の口（`--runner`）。**無ければ 1 本も起こさない**——`pipe run` は `--runner` を要り、
    /// 器は既定を持たない（宣言にも rules 行にも無い・2026-09-19 の実測）。列が勝手な既定を作ると、
    /// 「何を起こすか」が契約の外で決まる（C5 / C1）。
    pub runner: Option<&'a str>,
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
        materials: Materials::of(input.repo, input.manifest),
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
    settle(input, order(candidates), &ready, ledger.materials.as_ref().ok())
}

/// 1 周ぶん固定な台帳側の材料（候補ごとに読み直さない）。
struct Ledger<'a> {
    /// bead ごとの最後の印。
    marks: BTreeMap<String, (Mark, String)>,
    /// 閉じた bead の id（依存が閉じたかを同じ一覧の中で引く）。
    closed: BTreeSet<&'a str>,
    /// 1 周ぶんの repo の材料（**読みは 1 周に 1 回**・設計 §5・読めない周は断りの名を全候補が受ける）。
    materials: Result<Materials, Denial>,
}

/// 列を 1 周して**起こす**（契機の口＝終端の直後・手動の 1 周・印の直後・設計 §5）。
///
/// [`turn`] との違いは**起こすかどうかだけ**である（判定は同じ 1 本・C2）。観測の口（`dispatch ls`）は
/// [`turn`] を撃つ＝**見るだけでは 1 本も起こらない**（§6）。起こせなかった便は起こした数に数えず、
/// 理由つきで待ちに残す（終端の rc は呼び手が変えない・次の契機で拾う・C10）。
pub fn fire(input: &Input<'_>) -> Turn {
    // **driver の死んだ便を先に起こし直す**（設計 §5）: 起こし直した便は live のままなので列の交差は
    // 動かない。起こす側より先に撃つのは、同じ 1 周の中で「止まっている便」を先に動かすためである。
    // **実装役の口が無い周は列を測らない**（`pipe run` は `--runner` を要り、器は既定を持たない）。
    // 起こせないと分かっている周に台帳の子 process を撃つと、便の終端ごとに読みが 1 回乗る（実測: e2e
    // 全体が 21 秒 → 111 秒）。測っていないので `0 件`とも言わない（C10）——列を見る口は `dispatch ls`。
    if input.runner.is_none() {
        return unmeasured(Unmeasured::NoRunner);
    }
    let mut turn = turn(input);
    // **測れなかった周は 1 つも動かさない**（起こすのも起こし直すのも同じ 1 周の中の手・fail-closed）。
    // 起こし直しは台帳を読まないが、列を 1 周として成立させられない周に片方だけ動かすと、
    // `dispatch=unmeasured` の行が「何もしなかった」を意味しなくなる（C10）。
    if turn.unmeasured.is_some() {
        return turn;
    }
    turn.revives = revivals(input);
    turn.revives.retain(resume);
    let failed: Vec<String> =
        turn.launches.iter().filter(|launch| !start(launch)).map(|launch| launch.bead.clone()).collect();
    turn.launches.retain(|launch| !failed.contains(&launch.bead));
    for candidate in &mut turn.candidates {
        if failed.contains(&candidate.bead) {
            candidate.reason = Some(WaitReason::Admission { reason: SPAWN });
        }
    }
    turn
}

/// **人の手を待つ段**（承認待ち・回答待ち）。起こし直しの候補から**段で外す**（札は残す）。
///
/// `pipe resume` はこの 2 段で何もせず rc 3 を返す（待っている事実は段が既に持つ）ので、札が残ったまま
/// 契機のたびに起こし直すと空撃ちになる。**札は消さない**——消すと、承認や回答が記帳された後に driver の
/// 居ない live 便が「札の無い便＝触らない」に落ちて二度と自走せず、人が `pipe resume` を撃つ手順が戻る
/// （planner 裁定 2026-09-19）。段で外せば、回答の後の次の契機で自走に戻る。
const WAITING: [Stage; 2] = [Stage::Blocked, Stage::Questioned];

/// 起こし直す便（live 便のうち **driver の札の所有者が死んでいる**もの・設計 §5「driver の死亡」）。
///
/// **札が無い・読めない便は触らない**（測れないを「死んだ」に読み替えない・fail-closed）。別の process が
/// 生きて持っている札の便も、`pid` の再利用で生きて見える便も触らない（判定は lock の所有者と同じ 1 本）。
/// 人の手を待つ段（[`WAITING`]）も候補から外す。自分の札の便は**継ぐ**（[`super::driver_is_stale`]）。
fn revivals(input: &Input<'_>) -> Vec<Revive> {
    let Ok(state) = current(input.state_dir) else {
        return Vec::new();
    };
    state
        .runs
        .iter()
        .filter(|(id, run)| live(input.state_dir, id, run.stage) == Some(true))
        .filter(|(_, run)| !WAITING.contains(&run.stage))
        .filter(|(id, _)| super::driver_is_stale(input.state_dir, id))
        .map(|(id, _)| revive_of(input, id))
        .collect()
}

/// 起こし直しの構築点（`pipe resume` の引数を組む・**撃たない**）。道具は起こす側と同じ 1 本から渡す。
fn revive_of(input: &Input<'_>, run: &str) -> Revive {
    let mut argv = vec![
        "resume".to_owned(),
        "--run".to_owned(),
        run.to_owned(),
        "--repo".to_owned(),
        input.repo.display().to_string(),
        "--state-dir".to_owned(),
        input.state_dir.display().to_string(),
    ];
    argv.extend(tools(input));
    Revive { run: run.to_owned(), argv }
}

/// 起こし直す（子 process・[`start`] と同じ形で待たない）。
fn resume(revive: &Revive) -> bool {
    spawn_self(&revive.argv)
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
    Turn { candidates: Vec::new(), launches: Vec::new(), revives: Vec::new(), unmeasured: Some(reason) }
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
    let materials = match &ledger.materials {
        Ok(found) => found,
        Err(denial) => return wait(WaitReason::Admission { reason: denial.name }),
    };
    let contract = match generated(input.repo, &pointer, materials) {
        Ok((found, body)) => match settled(input, &issue.id, &body) {
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
fn settle(
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
    let mut turn = Turn { candidates: Vec::new(), launches: Vec::new(), revives: Vec::new(), unmeasured: None };
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
    let material = Material {
        repo: input.repo,
        manifest: input.manifest,
        contract,
        state_dir: None,
        bead: "",
        materials: room.materials,
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
fn launch_of(input: &Input<'_>, bead: &str, pointer: &Pointer) -> Launch {
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
    Launch { bead: bead.to_owned(), argv }
}

/// 列に渡された道具（起こす便と起こし直す便へ**そのまま全部**渡す＝列と便が同じ道具で動く）。
///
/// 渡されていない道具は何も足さない（既定は便の側が持つ）。**台帳 client（`--bd`）も渡す**: 起こした子
/// （`pipe run` / `pipe resume`）自身も終端で 1 周撃つので、落とすと子の 1 周が既定の台帳を読み、
/// **1 hop で列の名指した台帳と食い違う**。道具の受け渡しは全部か皆無かで、1 つだけ落とすと「同じ道具で
/// 動く」が静かに破れる（`s2-07l.366` の lens の実測）。
fn tools(input: &Input<'_>) -> Vec<String> {
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
fn settled(input: &Input<'_>, bead: &str, body: &str) -> Option<(String, Stage)> {
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
    let sha = git_bytes(input.repo, &["hash-object", "--", &path.display().to_string()])?;
    String::from_utf8(sha).ok().map(|found| (found.trim().to_owned(), stage))
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

/// 列の 1 周の結果の 1 行（終端と手動の 1 周が stdout に足す・設計 §5）。
///
/// **0 件と「測れない」を融合しない**（C10）: 台帳を読めない周は件数でなく理由を名乗る。
pub fn line(turn: &Turn) -> String {
    match turn.unmeasured {
        Some(reason) => format!("dispatch=unmeasured reason={}", reason.as_str()),
        None => format!(
            "dispatch=started:{},resumed:{},waiting:{}",
            turn.launches.len(),
            turn.revives.len(),
            turn.candidates.len().saturating_sub(turn.launches.len())
        ),
    }
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
    use crate::fleet::{Event, EventKind, Mark, Stage, SCHEMA};

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
            WaitReason::Settled { sha: "abc".to_owned(), stage: Stage::Landed },
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
                "settled:abc/Landed",
                "no-design-pointer",
            ],
            "値を持つ 5 件は値も描く"
        );
    }
}
