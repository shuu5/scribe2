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
use super::cli::{crossings, generated, int_row, judge, live, stage_of, Denial, Material, Materials};
use super::contract::Contract;
use super::refuse::overlaps;
use super::review;
use super::table::{self, Pointer};
use super::{contract_path, current, git_bytes, Ticket};
use crate::cli_outcome::Outcome;
use crate::fleet::store;
use crate::fleet::{Event, EventKind, Mark, Stage, State, STAGES};
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
    ///
    /// 契約の字が正しいのに器の側の理由で終端に着いた便は、その便の最後の記帳より**後**の `release`
    /// が 1 回だけ列外を外す（[`requeues`] の段だけ・設計 §12・`s2-07l.495`）。
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

/// **便の自走を選ぶ flag**（`pipe run` / `pipe resume`・設計 §5「便の自走は起こす側の引数で選ぶ」）。
///
/// 列が起こす便（起こす側・起こし直す側・継ぎの子）には**常に**付ける——道具の pass-through
/// （[`tools`]・値を持つ flag の対の配列・全部か皆無か）とは**別の定数**である。列の判断で起きた便は
/// 自走する、が意味であって、呼び手が道具を渡したかとは関係しない。
pub const DRIVE: &str = "--drive";

/// 段の動き（**閉じた 3 形**・pure・設計 §5「渡す周と渡さない周」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advance {
    /// 段が進んだ。
    Forward,
    /// 入口と同じ段のまま。
    Same,
    /// 段が戻った。
    Backward,
}

/// 自分の便を次の driver へ渡すか（**閉じた 5 値**・設計 §5「渡す周と渡さない周」）。
///
/// 渡さない周は理由を名乗る（C10・黙って止まらない）。設計が名指す 3 つの理由に
/// [`Handoff::Unmeasured`] を足してある——段も生死も読めない周を「終端に着いた」に読み替えると、
/// 測れなかった事実が記録から消える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handoff {
    /// 渡す（前進 ∧ 待ちの段でない ∧ 終端でない）。
    Pass,
    /// 人の手を待つ段に着いた（[`WAITING`]）。
    Waiting,
    /// 終端に着いた。
    Settled,
    /// 段が動かなかった（同じ段・戻った段）。
    NoProgress,
    /// 段か生死を読めなかった（**終端に読み替えない**・C10）。
    Unmeasured,
}

/// [`Handoff`] の全 variant の字面（`drive=` の値・宣言順）。
pub const HANDOFFS: &[&str] = &["pass", "waiting", "settled", "no-progress", "unmeasured"];

impl Handoff {
    /// `drive=` に載る字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Waiting => "waiting",
            Self::Settled => "settled",
            Self::NoProgress => "no-progress",
            Self::Unmeasured => "unmeasured",
        }
    }
}

/// 呼び手が渡す自分の便（`--drive` を持つ `pipe run` / `pipe resume` の周だけ・設計 §5）。
///
/// この便は**札の所有者（＝呼び手）が生きていても**起こし直しの候補に入れる——1 段進めて抜ける
/// driver の後を誰も継がないと、便は止まったまま次の契機を待つ（`s2-07l.482` の実測: 起こし直した
/// 便が `Implemented` で止まった）。
pub struct Driving<'a> {
    /// 便 id。
    pub run: &'a str,
    /// **入口で読んだ段**（便を作る `pipe run` は入口に段が無いので `None`＝前進）。
    pub entry: Option<Stage>,
}

/// 段の宣言順の位置（[`STAGES`] から導く＝順序の宣言は 1 か所・C2）。
///
/// 全 variant が [`STAGES`] に在ることは in-file の歯が母集団つきで測る（`unwrap_or` の値は
/// 到達しない）。
fn rank(stage: Stage) -> usize {
    STAGES.iter().position(|found| *found == stage).unwrap_or(STAGES.len())
}

/// 入口の段と終端の段から段の動きを判じる（**pure**・設計 §5）。
///
/// 入口に段が無い周（`pipe run` は便を作る）は前進である——作った便は必ず段を 1 つ持つ。
pub fn advance(entry: Option<Stage>, exit: Stage) -> Advance {
    let Some(entry) = entry else {
        return Advance::Forward;
    };
    match rank(exit).cmp(&rank(entry)) {
        std::cmp::Ordering::Greater => Advance::Forward,
        std::cmp::Ordering::Equal => Advance::Same,
        std::cmp::Ordering::Less => Advance::Backward,
    }
}

/// 渡す周か（**pure**・設計 §5「渡す周と渡さない周」）。
///
/// 渡すのは「前進 ∧ 待ちの段でない ∧ 終端でない」周だけである。**前進なしの周を渡すと、同じ段を
/// 空撃ちする子が無限に連なる**。生死は呼び手が [`live`] で測った 3 値をそのまま受ける（測れない周を
/// 終端と融合しない・C10）。
pub fn handoff(advance: Advance, exit: Stage, live: Option<bool>) -> Handoff {
    match live {
        None => Handoff::Unmeasured,
        Some(false) => Handoff::Settled,
        Some(true) if WAITING.contains(&exit) => Handoff::Waiting,
        Some(true) => match advance {
            Advance::Forward => Handoff::Pass,
            Advance::Same | Advance::Backward => Handoff::NoProgress,
        },
    }
}

/// 呼び手の便の段の動きと渡すかを永続面から判じる（段は replay・生死は [`live`] の 1 本）。
///
/// driver でない周は `None`。段を読めない driver の周は動きが `None`（**測れないを「前進」にも「同じ段」
/// にも読み替えない**・C10）で、渡すかは [`Handoff::Unmeasured`]。
fn progress_of(input: &Input<'_>) -> Option<(Option<Advance>, Handoff)> {
    let driving = input.driving.as_ref()?;
    let Ok(state) = current(input.state_dir) else {
        return Some((None, Handoff::Unmeasured));
    };
    let Ok(exit) = stage_of(&state, driving.run) else {
        return Some((None, Handoff::Unmeasured));
    };
    let moved = advance(driving.entry, exit);
    Some((Some(moved), handoff(moved, exit, live(input.state_dir, driving.run, exit))))
}

/// 関門が開いた待ちの便を起こす周か（**pure**・設計 §13「段を進めなかった driver の終端の 1 周は、関門の
/// 候補を 1 本も起こさない」）。
///
/// driver でない周（手動の 1 周・印の直後・回答や承認の記帳の直後・`None`）は絞らない。driver の周は
/// **段を前へ進めた周だけ**起こす——札の無い便を候補にすると「resume が抜けると札が消えて候補から落ちる」
/// 止め金が効かないので、段を 1 つも進められずに抜けた resume が自分の終端の 1 周で同じ便をまた起こし、
/// 待ちの便が 2 本在れば互いを起こし合う。連鎖は段の前進を 1 回ずつ要るので有限である。
pub fn admits_gated(driver: Option<Advance>) -> bool {
    match driver {
        None | Some(Advance::Forward) => true,
        Some(Advance::Same | Advance::Backward) => false,
    }
}

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
    /// 起こし直す便（driver の札の所有者が死んでいる live 便と、関門が開いて driver の居ない待ちの便・run id の順）。
    pub revives: Vec<Revive>,
    /// 台帳を読めなかった周の理由（`Some` なら他の 2 つは空）。
    pub unmeasured: Option<Unmeasured>,
    /// 呼び手の便を次の driver へ渡したか（`--drive` の周だけ `Some`・設計 §5）。
    pub drive: Option<Handoff>,
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
    /// 呼び手が渡す自分の便（`--drive` の周だけ `Some`・設計 §5「渡す周と渡さない周」）。
    /// **観測の口（[`turn`]）は見ない**——見るだけで便が動くと `dispatch ls` が起こす口になる（§6）。
    pub driving: Option<Driving<'a>>,
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
    let events = read_events(input.state_dir);
    let ledger = Ledger {
        marks: marks_of(&events),
        events,
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
    /// 置き場の event の並び（`release` が終端の便の最後の記帳より後かを位置で引く・設計 §12）。
    events: Vec<Event>,
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
    // **関門が開いた待ちの便は、driver の周なら段を前へ進めた周だけ起こす**（設計 §13・[`admits_gated`]）。
    // 段を読めない driver の周は 0 本（測れないを「前進」に読み替えない・fail-closed）。
    let progress = progress_of(input);
    let gated = match progress {
        None => admits_gated(None),
        Some((moved, _)) => moved.is_some_and(|found| admits_gated(Some(found))),
    };
    turn.revives = revivals(input, gated);
    // **呼び手の便を継ぐ**（設計 §5「1 段進めた driver は終端の 1 周で自分の便を次の driver に渡す」）:
    // 自分の札は生きている（いま握っているのは自分である）ので [`revivals`] は拾わない。渡す周だけ
    // 足し、渡さなかった周は理由を [`Turn::drive`] に残す（C10）。
    turn.drive = progress.map(|(_, handoff)| handoff);
    if let (Some(Handoff::Pass), Some(driving)) = (turn.drive, input.driving.as_ref()) {
        if !turn.revives.iter().any(|revive| revive.run == driving.run) {
            turn.revives.push(revive_of(input, driving.run));
            turn.revives.sort_by(|left, right| left.run.cmp(&right.run));
        }
    }
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

/// **人の手を待つ段**（承認待ち・回答待ち）。関門が閉じたままの便は起こし直しの候補から**段で外す**（札は残す）。
///
/// `pipe resume` はこの 2 段で関門が閉じていれば何もせず rc 3 を返す（待っている事実は段が既に持つ）ので、
/// 札が残ったまま契機のたびに起こし直すと空撃ちになる。**札は消さない**——消すと、承認や回答が記帳された
/// 後に driver の居ない live 便が「札の無い便＝触らない」に落ちて二度と自走せず、人が `pipe resume` を撃つ
/// 手順が戻る（planner 裁定 2026-09-19）。関門が開いた便（[`super::gate_is_open`]）は [`gated`] が候補に戻す。
const WAITING: [Stage; 2] = [Stage::Blocked, Stage::Questioned];

/// 起こし直す便（run id の順・設計 §5「driver の死亡」+ §13「関門が開いた待ちの便」）。
///
/// 待ちの段でない live 便は **driver の札の所有者が死んでいる**ものだけ（§5 の規則・1 字も変えない）:
/// 札が無い・読めない便は触らない（測れないを「死んだ」に読み替えない・fail-closed）。別の process が
/// 生きて持っている札の便も、`pid` の再利用で生きて見える便も触らない（判定は lock の所有者と同じ 1 本）。
///
/// 待ちの段（[`WAITING`]）の live 便は `gated` の周だけ [`gated`] で判じる（関門が開いていて driver が
/// 居ないと測れた便）。閉じたままの便は今までどおり候補にしない。
fn revivals(input: &Input<'_>, gated: bool) -> Vec<Revive> {
    let Ok(state) = current(input.state_dir) else {
        return Vec::new();
    };
    state
        .runs
        .iter()
        .filter(|(id, run)| live(input.state_dir, id, run.stage) == Some(true))
        .filter(|(id, run)| match WAITING.contains(&run.stage) {
            false => super::driver_is_dead(input.state_dir, id),
            true => gated && self::gated(input.state_dir, &state, id),
        })
        .map(|(id, _)| revive_of(input, id))
        .collect()
}

/// 関門が開いた待ちの便か（live ∧ 待ちの段 ∧ 関門が開いている ∧ **driver が居ないと測れた**・設計 §13）。
///
/// 関門の判定は resume の入口と同じ述語 1 本（[`super::gate_is_open`]・C2）。札は 4 値で読む
/// （[`Ticket`]）: 無い → 候補（待ちの段で止まった driver は正常に抜けて札を外す＝§5 の「札が無い便は
/// 触らない」を**この候補にだけ**緩める・C17.2）／所有者が死んでいる → 候補／所有者が生きている → 触らない
/// ／**在るのに読めない → 触らない**（測れないを「居ない」に読み替えない・fail-closed）。
fn gated(state_dir: &Path, state: &State, id: &str) -> bool {
    super::gate_is_open(state_dir, state, id)
        && matches!(super::driver_ticket(state_dir, id), Ticket::Absent | Ticket::Dead)
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
    // **列が起こす便は必ず自走する**（設計 §5）: 道具の pass-through と別の定数で、渡されたかに依らない。
    argv.push(DRIVE.to_owned());
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
    Turn { candidates: Vec::new(), launches: Vec::new(), revives: Vec::new(), unmeasured: Some(reason), drive: None }
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
    let mut turn =
        Turn { candidates: Vec::new(), launches: Vec::new(), revives: Vec::new(), unmeasured: None, drive: None };
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
fn settled(input: &Input<'_>, bead: &str, body: &str, design: &str, events: &[Event]) -> Option<(String, Stage)> {
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
fn section_keyed(stage: Stage) -> bool {
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
fn requeues(stage: Stage) -> bool {
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

/// 便の最後の記帳より**後**に、同じ bead への `release` が在るか（**pure**・材料は event log の並びだけ）。
///
/// 位置で引く（ts の字面は比べない）: 終端より**前**の `release` は効かない。便の記帳が 1 件も無い周は
/// 「後」を測れないので効かない側に倒す（replay に在る便は必ず記帳を持つので、実際には到達しない）。
fn released_after(events: &[Event], run: &str, bead: &str) -> bool {
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
        None => {
            let counts = format!(
                "dispatch=started:{},resumed:{},waiting:{}",
                turn.launches.len(),
                turn.revives.len(),
                turn.candidates.len().saturating_sub(turn.launches.len())
            );
            // **`--drive` の周だけ token を足す**（観測の面を増やさない・§6）: flag の無い周の行は
            // 1 byte も変わらない＝段を手で 1 つずつ進める既存の歯は 1 本も動かない。
            match turn.drive {
                None => counts,
                Some(drive) => format!("{counts} drive={}", drive.as_str()),
            }
        }
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
    use super::{
        admits_gated, advance, digits_of, handoff, launch_of, marks_of, order, rank, released_after, requeues,
        revive_of, section_keyed, tools, Advance, Candidate, Handoff, Input, Pointer, WaitReason, DRIVE, HANDOFFS,
        WAIT_REASONS,
    };
    use crate::fleet::{Event, EventKind, Mark, Stage, SCHEMA, STAGES};
    use crate::rules::manifest::Manifest;
    use std::path::Path;

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

    /// 便の記帳 1 件（段の行・印を持たない）。
    fn staged(ts: &str, run: &str, bead: &str, stage: Stage) -> Event {
        Event {
            schema: SCHEMA,
            ts: ts.to_owned(),
            kind: EventKind::RunStage,
            run: run.to_owned(),
            bead: bead.to_owned(),
            host: "h".to_owned(),
            actor: EventKind::RunStage.default_actor().to_owned(),
            stage: Some(stage),
            seat: None,
            pid: None,
            detail: None,
            allowance: None,
            registration: None,
            mark: None,
            account: None,
        }
    }

    /// `release` で列へ戻す段は **`Failed` / `Stopped` / `Gated`** の 3 つで、`Landed` と審査 FAIL の
    /// `Reviewed` は戻さない（設計 §12・母集団 = [`STAGES`] の全段・網羅の match 1 本）。
    #[test]
    fn pipe_dispatch_release_requeues_failed_stopped_and_gated_but_not_landed_or_reviewed() {
        let back: Vec<Stage> = STAGES.iter().copied().filter(|stage| requeues(*stage)).collect();
        assert_eq!(
            back,
            vec![Stage::Gated, Stage::Stopped, Stage::Failed],
            "母集団 {} 段のうち戻すのは終端の 3 段（宣言順）",
            STAGES.len()
        );
        assert!(!requeues(Stage::Landed), "Landed は済んでいる（起こし直すと同じ変更をもう一度作る）");
        assert!(!requeues(Stage::Reviewed), "審査 FAIL は中身が変わるまで列に入らない（FR49）");
    }

    /// 列外の鍵に § の本文を含める段は **`Reviewed` だけ**で、`Landed` と `release` が戻す 3 段は含めない
    /// （設計 §16・母集団 = [`STAGES`] の全段・網羅の match 1 本・[`requeues`] とは交わらない）。
    #[test]
    fn pipe_dispatch_section_key_applies_to_reviewed_only() {
        let keyed: Vec<Stage> = STAGES.iter().copied().filter(|stage| section_keyed(*stage)).collect();
        assert_eq!(keyed, vec![Stage::Reviewed], "母集団 {} 段のうち § を鍵に入れるのは審査の段だけ", STAGES.len());
        assert!(!section_keyed(Stage::Landed), "Landed は済んでいる（§ を直しても起こし直さない）");
        assert!(
            STAGES.iter().all(|stage| !(section_keyed(*stage) && requeues(*stage))),
            "§ の鍵と release の印は同じ段を持たない（審査の終端は印で戻さない・FR49）"
        );
    }

    /// `release` が効くのは**便の最後の記帳より後**の 1 件だけで、位置で引く（終端より前の印・別の
    /// bead への印・`release` でない印は効かない・pure・設計 §12）。
    #[test]
    fn pipe_dispatch_release_requeues_only_when_the_mark_follows_the_last_record_of_the_run() {
        let run = "s2-a-1";
        let after = vec![
            staged("t1", run, "s2-a", Stage::Intake),
            staged("t2", run, "s2-a", Stage::Failed),
            marked("t3", "s2-a", Mark::Release),
        ];
        assert!(released_after(&after, run, "s2-a"), "終端の後の release は効く");
        let before = vec![
            staged("t1", run, "s2-a", Stage::Intake),
            marked("t2", "s2-a", Mark::Release),
            staged("t3", run, "s2-a", Stage::Failed),
        ];
        assert!(!released_after(&before, run, "s2-a"), "終端より前の release は効かない");
        let other = vec![staged("t1", run, "s2-a", Stage::Failed), marked("t2", "s2-b", Mark::Release)];
        assert!(!released_after(&other, run, "s2-a"), "別の bead への release は効かない");
        let held = vec![staged("t1", run, "s2-a", Stage::Failed), marked("t2", "s2-a", Mark::Hold)];
        assert!(!released_after(&held, run, "s2-a"), "release でない印は効かない");
        // 起こし直した便の記帳が release の後に並べば、その便から見て release は**前**に戻る（印は 1 回）。
        let again = "s2-a-2";
        let relaunched = vec![
            staged("t1", run, "s2-a", Stage::Failed),
            marked("t2", "s2-a", Mark::Release),
            staged("t3", again, "s2-a", Stage::Failed),
        ];
        assert!(!released_after(&relaunched, again, "s2-a"), "起こし直した便の終端の後には release が無い");
        assert!(!released_after(&[], run, "s2-a"), "記帳の無い便は「後」を測れない（効かない側）");
    }


    /// 関門が開いた待ちの便の候補の選別（**pure**・設計 §13）: driver でない周は絞らず、driver の周は
    /// 段の前進の 3 値のうち**前進だけ**が候補をそのまま起こし、同じ段のままと段が戻った周は 0 本にする
    /// （空撃ちの連鎖を塞ぐ・母集団 = 3 値 + driver でない周）。
    #[test]
    fn pipe_dispatch_waiting_gate_admits_only_forward_drivers_and_every_non_driver() {
        assert!(admits_gated(None), "driver でない周（手動の 1 周・印・回答や承認の直後）は絞らない");
        assert!(admits_gated(Some(Advance::Forward)), "段を前へ進めた driver の周は候補をそのまま起こす");
        assert!(!admits_gated(Some(Advance::Same)), "同じ段のままの driver の周は 0 本");
        assert!(!admits_gated(Some(Advance::Backward)), "段が戻った driver の周は 0 本");
        let listed = [Advance::Forward, Advance::Same, Advance::Backward];
        let admitted = listed.iter().filter(|moved| admits_gated(Some(**moved))).count();
        assert_eq!(admitted, 1, "母集団 {} 値のうち起こすのは前進の 1 値だけ", listed.len());
    }

    /// 列が起こす便には**常に** `--drive` が付く（起こす側・起こし直す側の両方）。道具の
    /// pass-through（[`tools`]）とは**別の定数**である——渡された道具に混ぜると、`--rules` 等を
    /// 渡していない呼び手の周だけ自走しなくなる（「全部か皆無か」の列に載せない理由）。
    #[test]
    fn pipe_dispatch_drive_is_added_to_every_run_the_queue_starts() {
        let manifest = Manifest::embedded().expect("埋め込みの manifest を読める");
        let input = Input {
            state_dir: Path::new("s"),
            repo: Path::new("r"),
            manifest: &manifest,
            bd: "bd",
            bd_flag: None,
            rules: None,
            lens: None,
            curl: None,
            runner: Some("true"),
            driving: None,
        };
        let pointer = Pointer { path: "docs/design/toy.md".to_owned(), id: "a".to_owned() };
        let started = launch_of(&input, "s2-toy.1", &pointer).argv;
        let revived = revive_of(&input, "s2-toy.1-1").argv;
        assert_eq!(started.last().map(String::as_str), Some(DRIVE), "起こす側: {started:?}");
        assert_eq!(revived.last().map(String::as_str), Some(DRIVE), "起こし直す側: {revived:?}");
        // **道具の列には入らない**（母集団 = 渡された道具 1 件 `--runner`）。
        let passed = tools(&input);
        assert_eq!(passed, vec!["--runner".to_owned(), "true".to_owned()], "道具は渡された分だけ");
        assert!(!passed.contains(&DRIVE.to_owned()), "自走の flag は道具の pass-through ではない: {passed:?}");
    }

    /// 段の位置は [`STAGES`] の宣言から導く（順序の宣言は 1 か所・C2）。**全 variant が母集団に在る**
    /// ことをここで測る——1 つでも欠けると [`rank`] の `unwrap_or` が同じ値を 2 つの段に配り、
    /// [`advance`] が静かに「同じ段」を返す。
    #[test]
    fn pipe_dispatch_drive_ranks_every_stage_from_the_declared_order() {
        let ranks: Vec<usize> = STAGES.iter().map(|stage| rank(*stage)).collect();
        assert_eq!(ranks, (0..STAGES.len()).collect::<Vec<usize>>(), "母集団 {} 段が宣言順の位置を持つ", STAGES.len());
        assert!(ranks.iter().all(|found| *found < STAGES.len()), "外れ値（母集団の外）の段は無い: {ranks:?}");
    }

    /// 段の動きは**閉じた 3 形**（pure・設計 §5）: 入口に段が無い周（`pipe run`）は前進・
    /// 宣言順に進めば前進・同じ段は `Same`・戻れば `Backward`。
    #[test]
    fn pipe_dispatch_drive_advance_is_forward_same_or_backward() {
        assert_eq!(advance(None, Stage::Intake), Advance::Forward, "便を作った周は入口に段が無い");
        assert_eq!(advance(Some(Stage::Implemented), Stage::Gated), Advance::Forward, "Implemented → Gated");
        assert_eq!(advance(Some(Stage::Gated), Stage::Gated), Advance::Same, "同じ段");
        assert_eq!(advance(Some(Stage::Gated), Stage::Implemented), Advance::Backward, "追随で戻った段");
    }

    /// 渡すのは **前進 ∧ 待ちの段でない ∧ 終端でない** 周だけで、渡さない周は理由を名乗る
    /// （**閉じた 5 値**・C10）。生死が読めない周を「終端」に読み替えない。
    #[test]
    fn pipe_dispatch_drive_hands_off_only_on_forward_and_names_the_reason() {
        let live = Some(true);
        assert_eq!(handoff(Advance::Forward, Stage::Gated, live), Handoff::Pass, "前進・生きている・待ちでない");
        assert_eq!(handoff(Advance::Same, Stage::Gated, live), Handoff::NoProgress, "段が動かなかった");
        assert_eq!(handoff(Advance::Backward, Stage::Implemented, live), Handoff::NoProgress, "戻った段");
        assert_eq!(handoff(Advance::Forward, Stage::Blocked, live), Handoff::Waiting, "承認待ち");
        assert_eq!(handoff(Advance::Forward, Stage::Questioned, live), Handoff::Waiting, "回答待ち");
        assert_eq!(handoff(Advance::Forward, Stage::Landed, Some(false)), Handoff::Settled, "終端");
        assert_eq!(handoff(Advance::Forward, Stage::Gated, None), Handoff::Unmeasured, "生死を読めない");
    }

    /// `drive=` の値は [`HANDOFFS`] と 1 対 1（宣言順・字面は 1 か所）。
    #[test]
    fn pipe_dispatch_drive_tokens_are_the_closed_five() {
        let listed =
            [Handoff::Pass, Handoff::Waiting, Handoff::Settled, Handoff::NoProgress, Handoff::Unmeasured];
        let names: Vec<&str> = listed.iter().map(|found| found.as_str()).collect();
        assert_eq!(names, HANDOFFS, "母集団 {} 値（宣言順）", HANDOFFS.len());
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
