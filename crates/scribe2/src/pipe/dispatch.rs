//! `pipe dispatch` — 審査を通った契約を器が自動で起こす列（設計 docs/design/dispatcher.md §2〜§4・§6）。
//!
//! 起動に要る判定は**器が既に持っている**（台帳の依存・live 便との write-set の交差・受付の余地と host の
//! memory・審査の verdict）。本 module はそれを読み直して並べるだけで、**判定を 2 本目に実装しない**
//! （憲法 C2）: 交差は受付の [`crate::pipe::cli::crossings`]、余地は受付の [`crate::pipe::cli::judge`]、
//! 枠は [`super::admission::has_room`] を**記帳せずに**撃つ。
//!
//! 順序は 1 関数 [`order`] だけが持つ（散文の順序を持たない・設計 §2）: (1) 介入 `first` (2) 台帳の
//! `priority`（P0 → P4）(3) 起票順（id の数字）。
//!
//! 台帳を読めない周は列を空と読まず [`Unmeasured`] で 1 本も起こさない（`0 件`と融合しない・C10・NFR4）。

use super::cli::{live, stage_of, Denial, Materials};
use super::contract::Contract;
use super::gate::{Limits, Verdict};
use super::health;
use super::land::verdict_of;
use super::regate::{followed_since_gate, regated_since_gate};
use super::table::Pointer;
use super::{current, Ticket};
use crate::cli_outcome::Outcome;
use crate::fleet::store;
use crate::fleet::{replay, Event, EventKind, Mark, Stage, State, SCHEMA, STAGES};
use crate::invocation::Invocation;
use crate::name::NAME;
use crate::rules::manifest::Manifest;
use crate::seat::ledger;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Stdio;

/// 台帳から候補を組む群（設計 §20・`s2-07l.531` の純移動）。
mod candidates;

/// 1 周の群の段（群の逼迫の通知と自動の移動・設計 account-lifecycle.md §19 形 2〜4・§20）。
mod group;

use candidates::{entry_of, is_input, marks_of, settle, tools};

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

/// 起こした事実の印（[`Mark::Launched`]）を書けない・event log を読めず印を測れない周の理由の字面
/// （設計 §17・記帳できない起動を数えない＝測れない側）。
const MARK: &str = "mark";

/// 子の stderr を append する診断 file の置き場（`<state_dir>/pipe/launch.log`・設計 §17・機械は読まない）。
const LAUNCH_LOG: [&str; 2] = ["pipe", "launch.log"];


/// [`WaitReason`] の全 variant の名（宣言順・`enum-slices` が集合完全性を測る）。
pub const WAIT_REASONS: &[&str] =
    &["dependency", "overlap", "admission", "host-busy", "hold", "launched", "settled", "no-design-pointer"];

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
        /// 受付が断った名（[`crate::pipe::refuse::Refuse::as_str`] か [`SLOT`] / [`SPAWN`] / [`MARK`]・すべて `'static`）。
        reason: &'static str,
    },
    /// 器の健康の遮断器が「待つ」を返した周（gate と同じ 1 関数 [`health::act`] が [`health::Action::Wait`]・
    /// 設計 §18）。混んだ host に便を起こしても落ちるだけなので、その周は 1 本も起こさない。
    HostBusy,
    /// 介入 `hold` が付いている。
    Hold {
        /// 印を付けた event の ts。
        since: String,
    },
    /// 列が起こした便がまだ受付に届いていない（最新の `launched` の後に同じ bead の `RunCreated` も
    /// `release` も無い・設計 §17）。受付で落ちた便を毎周起こし直さない。
    Launched {
        /// 最新の `launched` の印の ts。
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
            Self::HostBusy => "host-busy",
            Self::Hold { .. } => "hold",
            Self::Launched { .. } => "launched",
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
            Self::Hold { ref since } | Self::Launched { ref since } => format!("{name}:{since}"),
            Self::Settled { ref sha, stage } => format!("{name}:{sha}/{}", stage.as_str()),
            Self::HostBusy | Self::NoDesignPointer => name.to_owned(),
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
/// 起こせなかった周は理由の名（[`MARK`] か [`SPAWN`]）を返して**その便を起こさなかった事実だけ**を残す
/// （終端の rc は呼び手が変えない・次の契機で拾う・§5）。判定は turn の 1 回だが、`pipe run` 側の受付は
/// 外さない（二重に守る・planner 裁定 2026-09-19 の条件 (2)）。
///
/// **起こす前に印を書く**（設計 §17）: bead 名義の [`Mark::Launched`] を記帳してから子を起こす＝印は子の
/// `RunCreated` より前の行に並ぶ。書けない周は起こさない（fail-closed・記帳できない起動を数えない）。
fn start(input: &Input<'_>, launch: &Launch) -> Result<(), &'static str> {
    if !launched(input, launch) {
        return Err(MARK);
    }
    if !spawn_self(input.state_dir, &launch.argv) {
        return Err(SPAWN);
    }
    Ok(())
}

/// 起こした事実の印を 1 件記帳する（`DispatchMark` mark = `launched`・detail = 起こした argv の subcommand 1 語）。
///
/// 追記は fleet の 1 本（[`store::append`]・C6.3）で、lock の待ち方は列と同じ manifest から読む。読めない周も
/// 書けない周と同じ `false`（記帳できない起動を数えない）。
fn launched(input: &Input<'_>, launch: &Launch) -> bool {
    let Ok(policy) = store::LockPolicy::from_rules(input.manifest) else {
        return false;
    };
    let event = Event {
        schema: SCHEMA,
        ts: crate::fleet::cli::now_utc(),
        kind: EventKind::DispatchMark,
        run: String::new(),
        bead: launch.bead.clone(),
        host: crate::fleet::cli::host(),
        actor: EventKind::DispatchMark.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: launch.argv.first().cloned(),
        allowance: None,
        registration: None,
        mark: Some(Mark::Launched),
        account: None,
        cost: None,
        rule: None,
    };
    store::append(input.state_dir, &event, policy).is_ok()
}

/// 自分自身を `pipe <argv>` で起こす（**起こす側と起こし直す側の 1 実装**・C2）。
///
/// 子の stderr は `<state_dir>/pipe/launch.log` に append する（設計 §17・受付で落ちた子の死因を席が読める
/// 場所に残す・C10）。file を開けない周は stderr を捨てて**起こす**（起動を記録の失敗で止めない）。
/// land の着地後の検出（設計 gate-cost.md §44 形 (11)）も同じ 1 本で起こす。
pub(in crate::pipe) fn spawn_self(state_dir: &Path, argv: &[String]) -> bool {
    Invocation::new(myself())
        .arg(PIPE)
        .args(argv)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(launch_log(state_dir))
        .spawn()
        .is_ok()
}

/// 子の stderr の行き先（`launch.log` を append で開く・開けない周は [`Stdio::null`]）。
fn launch_log(state_dir: &Path) -> Stdio {
    let path = LAUNCH_LOG.iter().fold(state_dir.to_path_buf(), |dir, part| dir.join(part));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => Stdio::from(file),
        Err(_) => Stdio::null(),
    }
}

/// 自分の binary（`argv[0]`）。PATH で呼ばれた周は同じ名で子も PATH から解ける。
///
/// **`current_exe` は使わない**——`/proc/self/exe` を読むのは「器は env も HOME も読まない」（C2.2）の
/// 外側で、xtask の門が違反として数える。`argv[0]` は**呼ばれ方そのもの**なので、同じ呼ばれ方で子を起こす。
///
/// 席の hook が鮮度の外の口座を子で測る口（`hook::group`・設計 account-lifecycle.md §19 形 5）も同じ 1 本を読む。
pub(crate) fn myself() -> String {
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
    /// 終端の周の軸を評価した周の値（起こす側の [`fire`] だけ `Some` になりうる・見る側の [`turn`] は常に `None`・
    /// 設計 consumer-sync.md §15 形 2）。
    pub vessel: Option<crate::hook::vessel::Upstream>,
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
    /// 呼び手が自分で段を進めた便の id（`pipe run` / `pipe resume` の周・**flag の有無に依らず** `Some`・
    /// 設計 §15「flag の無い driver は自分の便をこの候補にしない」）。
    ///
    /// [`revivals`] の **PASS の `Gated` の枝だけ**がこの便を候補から外す。flag の無い resume が
    /// Implemented → Gated（PASS）で抜けた直後の自分の 1 周は、札の外れた自分の便を拾って着地まで運んで
    /// しまう＝「1 段だけ」（§5）が破れる。flag の在る driver の自分の便は今までどおり [`Driving`] の
    /// 経路（handoff）が運ぶ。他の枝と観測の口（[`turn`]）は見ない。
    pub driven: Option<&'a str>,
}

/// 列を 1 周する（**判定は器の既存の関数・記帳はしない**）。
///
/// 台帳を読めない周は [`Unmeasured`] を持って返り、1 本も起こさない（fail-closed・設計 §7）。
pub fn turn(input: &Input<'_>) -> Turn {
    let Some(timeout) = ledger::timeout_of(input.manifest) else {
        return unmeasured(Unmeasured::NoRule);
    };
    // 台帳の子 process は **`--repo` の中で**撃つ（設計 §14）: 台帳 client は cwd から台帳を解くので、
    // process の cwd を継がせると設計 doc と契約表は `--repo`・台帳は cwd 側という食い違った 1 周になる。
    // `--repo` が dir でない周は spawn が落ちて `Unreadable`＝`unmeasured reason=ledger`（0 件と融合しない・C10）。
    let Ok(issues) = ledger::read_ledger(input.bd, input.repo, timeout) else {
        return unmeasured(Unmeasured::Ledger);
    };
    // 1 周ぶん固定な材料は**ここで 1 回だけ**解く（候補ごとに rules 行と台帳を読み直さない）。
    // event log を読めない周は起こした事実の印を測れない＝`launched` を `None` に持ち、起こせる候補を
    // [`MARK`] で待たせる（読めないを「印が無い」に読み替えない・fail-closed・設計 §17）。
    let read = store::read_all(input.state_dir);
    let unreadable = read.is_err();
    let events = read.unwrap_or_default();
    let marks = marks_of(&events);
    let ledger = Ledger {
        marks: marks.order,
        launched: (!unreadable).then_some(marks.launched),
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
    let mut turn = settle(input, order(candidates), &ready, ledger.materials.as_ref().ok());
    if !turn.launches.is_empty() && host_busy(input.manifest) {
        hold_for_host(&mut turn);
    }
    turn
}

/// 器の健康の遮断器が「待つ」を返すか（**gate と同じ 1 関数**・設計 §18・C2）。
///
/// 倍率は gate と同じ 2 行を同じ読み手（[`Limits::of`]）で読み、host は [`health::now`] の 1 回で読む。行を読めない
/// 周と host を測れない周は [`health::act`] のとおり起こす側（`Unmeasured` は撃つ・gate と同じ極性）である。
fn host_busy(manifest: &Manifest) -> bool {
    let Ok(limits) = Limits::of(manifest) else {
        return false;
    };
    health::act(health::now(limits.breaker().per_core)).0 == health::Action::Wait
}

/// 起こせる候補を全部 [`WaitReason::HostBusy`] で待たせる（**その周は 1 本も起こさない**・理由の無い件だけ）。
fn hold_for_host(turn: &mut Turn) {
    turn.launches.clear();
    for candidate in &mut turn.candidates {
        if candidate.reason.is_none() {
            candidate.reason = Some(WaitReason::HostBusy);
        }
    }
}

/// 1 周ぶん固定な台帳側の材料（候補ごとに読み直さない）。
struct Ledger<'a> {
    /// bead ごとの最後の介入の印（`first` / `hold`）。
    marks: BTreeMap<String, (Mark, String)>,
    /// 起こしたのにまだ受付に届いていない bead と最新の `launched` の ts（event log を読めない周は `None`＝測れない）。
    launched: Option<BTreeMap<String, String>>,
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
    // **群の段は起こす側の 1 周の先頭で走る**（設計 account-lifecycle.md §19 形 2 / 7）: 台帳を読まないので道具
    // （`--runner`）の無い周も走り、見る側（[`turn`]・`dispatch ls`）は撃たない。群 0 の host は 1 語も出さない。移動（§20）も
    // この段の中で便の列の前に走り、段が typed に止まった周（lock の残り・読めない面）も列の rc と行は変えない（§20 形 7）。
    let _ = group::round(input);
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
    turn.revives.retain(|revive| resume(input, revive));
    // **`launches` は印を書けて起こせた分だけ**（設計 §17）: 印を書けない便は [`MARK`]、起こせない便は [`SPAWN`]。
    let failed: BTreeMap<String, &'static str> = turn
        .launches
        .iter()
        .filter_map(|launch| start(input, launch).err().map(|reason| (launch.bead.clone(), reason)))
        .collect();
    turn.launches.retain(|launch| !failed.contains_key(&launch.bead));
    for candidate in &mut turn.candidates {
        if let Some(&reason) = failed.get(&candidate.bead) {
            candidate.reason = Some(WaitReason::Admission { reason });
        }
    }
    // **終端の周の軸は起こし終えた後に 1 回**（設計 consumer-sync.md §15 形 2）: この周に起こした便・起こし直した便が
    // 在れば live は 0 でない（子の `RunCreated` を待たずに数える＝走り出した便の下で binary を入れ替えない）。
    turn.vessel = crate::hook::vessel::sync(input.state_dir, idle(input, &turn));
    turn
}

/// live な便が 0 の周か（`None` = 置き場か便の生死を測れない・live 0 に読み替えない・C10）。
///
/// 生死は起こし直しと同じ 1 本（[`live`]）で読む。この周に起こした便か起こし直した便が在れば `Some(false)`。
fn idle(input: &Input<'_>, turn: &Turn) -> Option<bool> {
    if !turn.launches.is_empty() || !turn.revives.is_empty() {
        return Some(false);
    }
    let state = current(input.state_dir).ok()?;
    let lives: Vec<Option<bool>> = state.runs.iter().map(|(id, run)| live(input.state_dir, id, run.stage)).collect();
    if lives.contains(&Some(true)) {
        return Some(false);
    }
    lives.iter().all(Option::is_some).then_some(true)
}

/// **人の手を待つ段**（承認待ち・回答待ち）。関門が閉じたままの便は起こし直しの候補から**段で外す**（札は残す）。
///
/// `pipe resume` はこの 2 段で関門が閉じていれば何もせず rc 3 を返す（待っている事実は段が既に持つ）ので、
/// 札が残ったまま契機のたびに起こし直すと空撃ちになる。**札は消さない**——消すと、承認や回答が記帳された
/// 後に driver の居ない live 便が「札の無い便＝触らない」に落ちて二度と自走せず、人が `pipe resume` を撃つ
/// 手順が戻る（planner 裁定 2026-09-19）。関門が開いた便（[`super::gate_is_open`]）は [`gated`] が候補に戻す。
const WAITING: [Stage; 2] = [Stage::Blocked, Stage::Questioned];

/// 起こし直す便（run id の順・設計 §5「driver の死亡」+ §13「関門が開いた待ちの便」+ §15「PASS の `Gated`」）。
///
/// 待ちの段でない live 便は **driver の札の所有者が死んでいる**ものだけ（§5 の規則・1 字も変えない）:
/// 札が無い・読めない便は触らない（測れないを「死んだ」に読み替えない・fail-closed）。別の process が
/// 生きて持っている札の便も、`pid` の再利用で生きて見える便も触らない（判定は lock の所有者と同じ 1 本）。
/// **`Gated` の便だけ**は、これに加えて [`passed_gate`]（verdict が PASS ∧ 札が無いか所有者が死んでいる）
/// でも候補にする（§15・FR68 の 3 種目）。足す側だけで既存の枝は変えない＝gate の途中で driver が死んだ
/// 便は verdict に依らず今までどおり候補である。
///
/// 待ちの段（[`WAITING`]）の live 便は `gated` の周だけ [`gated`] で判じる（関門が開いていて driver が
/// 居ないと測れた便）。閉じたままの便は今までどおり候補にしない。PASS の `Gated` の枝も同じ `gated` の絞りを
/// 受ける（§15「§13 の絞りをそのまま受ける」）——この候補の札は起こす前も後も無いので、§5 の止め金
/// （resume が抜けると札が消えて候補から落ちる）が効かない。
///
/// **regate で `Implemented` へ戻された便**も同じ `gated` の絞りで候補にする（[`regated`]・§23・FR68 の 4 種目）。
/// event の列は置き場から 1 回だけ読み、便の表と regate の記帳の読みに同じ列を渡す（表と列を食い違わせない）。
fn revivals(input: &Input<'_>, gated: bool) -> Vec<Revive> {
    let Ok(events) = store::read_all(input.state_dir) else {
        return Vec::new();
    };
    let state = replay(&events);
    state
        .runs
        .iter()
        .filter(|(id, run)| live(input.state_dir, id, run.stage) == Some(true))
        .filter(|(id, run)| match WAITING.contains(&run.stage) {
            false => {
                super::driver_is_dead(input.state_dir, id)
                    || (gated && passed_gate(input, id, run.stage))
                    || (gated && regated(input.state_dir, &events, id, run.stage))
                    || (gated && followed(input.state_dir, &events, id, run.stage))
            }
            true => gated && self::gated(input.state_dir, &state, id),
        })
        .map(|(id, _)| revive_of(input, id))
        .collect()
}

/// regate で戻され、その後 gate を通っていない便か（段が `Implemented` ∧ 最新の `Gated` より後ろに regate の
/// 記帳 ∧ 札が無いか所有者が死んでいる・設計 §23・FR68 の 4 種目）。
///
/// regate の記帳の読みは regate の口と同じ 1 本（[`regated_since_gate`]・C2）。札は §13 と同じ 4 値で読み、
/// `Live` / `Unreadable` は触らない（測れないを「居ない」に読み替えない）。regate の記帳を持たない `Implemented`
/// の便と、regate の後に `Gated` を経た便（追随で戻った便を含む）は候補にしない（§5 の「札の無い便は触らない」）。
fn regated(state_dir: &Path, events: &[Event], id: &str, stage: Stage) -> bool {
    stage == Stage::Implemented
        && regated_since_gate(events, id)
        && matches!(super::driver_ticket(state_dir, id), Ticket::Absent | Ticket::Dead)
}

/// 追随で `Implemented` へ戻った後に driver が抜けた便か（段が `Implemented` ∧ 最新の `Gated` より後ろに追随の記帳
/// ∧ その後ろに `Gated` / `Landed` が無い ∧ 札が無いか所有者が死んでいる・設計 §25・FR68 の 5 種目）。
///
/// 追随の記帳の読みは [`followed_since_gate`] の 1 本。札の読みと除外は [`regated`] と同じ（`Live` / `Unreadable`
/// は触らない・追随の記帳を持たない `Implemented` と追随の後に `Gated` を経た便は候補にしない）。
fn followed(state_dir: &Path, events: &[Event], id: &str, stage: Stage) -> bool {
    stage == Stage::Implemented
        && followed_since_gate(events, id)
        && matches!(super::driver_ticket(state_dir, id), Ticket::Absent | Ticket::Dead)
}

/// 席が測り直して PASS になった `Gated` の便か（`Gated` ∧ verdict が PASS ∧ 札が無いか所有者が死んでいる・
/// 設計 §15・FR68 の 3 種目）。
///
/// verdict の読みは着地の段が持つ既存の 1 本（[`verdict_of`]・site を 2 つにしない・C2）。**PASS 以外は候補に
/// しない**: INCONCLUSIVE を候補にすると `pipe resume` が `next=gate` で止まる空撃ちになる（測り直しは席の
/// `pipe gate`＝器が勝手に 1 周ぶんの費用を払い直さない）。読めない周（`None`）も候補にしない（測れないを
/// 「通った」に読み替えない・fail-closed・NFR4）。札は §13 と同じ 4 値で読み、`Live` / `Unreadable` は触らない。
///
/// **呼び手が自分で段を進めた便（[`Input::driven`]）は外す**（§15「flag の無い driver は自分の便をこの候補に
/// しない」）: flag の無い driver は「その process が進める段は 1 つ」の約束を持つ。別の契機（手動の 1 周・
/// 他の便の driver の終端の 1 周）は、その driver が置いていった PASS の `Gated` の便を拾う。
fn passed_gate(input: &Input<'_>, id: &str, stage: Stage) -> bool {
    stage == Stage::Gated
        && input.driven != Some(id)
        && verdict_of(input.state_dir, id) == Some(Verdict::Pass)
        && matches!(super::driver_ticket(input.state_dir, id), Ticket::Absent | Ticket::Dead)
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
fn resume(input: &Input<'_>, revive: &Revive) -> bool {
    spawn_self(input.state_dir, &revive.argv)
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
    Turn {
        candidates: Vec::new(),
        launches: Vec::new(),
        revives: Vec::new(),
        unmeasured: Some(reason),
        drive: None,
        vessel: None,
    }
}

/// 印の畳み込みの結果（[`marks_of`]）。
struct Marks {
    /// bead ごとの**最後の**介入の印（`first` / `hold`・`release` が外す・設計 §4）。
    order: BTreeMap<String, (Mark, String)>,
    /// bead ごとの最新の `launched` の ts（その後に同じ bead の `RunCreated` も `release` も無いものだけ・設計 §17）。
    /// 介入の印とは**独立の値**で持つ（`hold` と同じ側に畳まない）。
    launched: BTreeMap<String, String>,
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

/// 終端の周の軸を評価した周の 1 行（`vessel=<値>`・評価していない周は `None`・設計 consumer-sync.md §15 形 3）。
///
/// [`line`] とは**別の行**である（`dispatch=` の行の書式と `drive=` の token は 1 字も変えない）。
pub fn vessel_line(turn: &Turn) -> Option<String> {
    turn.vessel.as_ref().map(|found| format!("vessel={}", found.render()))
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

// 歯（`mod tests`）だけが引く 5 名（`requeues` は親の doc の link からも引かれるが、link は use に数えられない）。
// 歯の区間の直前に置く（file の最初の行頭 `#[cfg(test)]` を src の本体より後に保つ・設計 §20）。
#[cfg(test)]
use candidates::{launch_of, released_after, requeues, review_unmeasured, section_keyed};

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.531
    use super::{
        admits_gated, advance, digits_of, handoff, launch_of, marks_of, order, rank, released_after, requeues,
        review_unmeasured, revive_of, section_keyed, tools, Advance, Candidate, Handoff, Input, Pointer, WaitReason,
        DRIVE, HANDOFFS, WAIT_REASONS,
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
            cost: None,
            rule: None,
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
            cost: None,
            rule: None,
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

    /// 判定で引く戻しが真になるのは **`INCONCLUSIVE` ∧ `Unparsed` の対だけ**で、`FAIL`（kind を問わず）・
    /// `INCONCLUSIVE` で kind が他の 6 語・`PASS` は偽（設計 §22・母集団 = [`crate::pipe::review::FINDING_KINDS`] の 7 語 × 3 値）。
    #[test]
    fn pipe_dispatch_release_unparsed_is_the_only_judgement_that_requeues() {
        use crate::pipe::gate::Verdict;
        use crate::pipe::review::{FindingKind, Judgement, FINDING_KINDS};
        assert_eq!(FINDING_KINDS.len(), 7, "前提: 理由の型は 7 語");
        let mut back = Vec::new();
        for verdict in [Verdict::Pass, Verdict::Fail, Verdict::Inconclusive] {
            for kind in FINDING_KINDS {
                if review_unmeasured(&Judgement { verdict, kind: Some(*kind), at: Vec::new() }) {
                    back.push((verdict, *kind));
                }
            }
            assert!(!review_unmeasured(&Judgement { verdict, kind: None, at: Vec::new() }), "kind の無い判定は戻さない");
        }
        assert_eq!(
            back,
            vec![(Verdict::Inconclusive, FindingKind::Unparsed)],
            "母集団 {} 対のうち戻すのは測れなかった 1 対だけ",
            FINDING_KINDS.len() * 3
        );
        let unparsed = |verdict| Judgement { verdict, kind: Some(FindingKind::Unparsed), at: Vec::new() };
        assert!(!review_unmeasured(&unparsed(Verdict::Fail)), "FAIL は kind を問わず戻さない（FR49）");
        assert!(review_unmeasured(&unparsed(Verdict::Inconclusive)), "INCONCLUSIVE ∧ unparsed は戻す");
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
            driven: None,
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
        let found = marks_of(&events).order;
        assert_eq!(found.get("s2-a").map(|(mark, _)| *mark), Some(Mark::Hold), "後の印が勝つ");
        assert_eq!(found.get("s2-a").map(|(_, ts)| ts.clone()), Some("t3".to_owned()), "ts は勝った印の行");
        assert_eq!(found.get("s2-b"), None, "release は印を外す");
        assert_eq!(found.len(), 1, "母集団 {} 行のうち残る印は 1 つ", events.len());
    }

    /// 起こした事実の印（`launched`）は介入の印と**独立の値**で最新の 1 つを持ち、同じ bead の `RunCreated` と
    /// `release` が外す（別の bead の `RunCreated` は外さない・pure・設計 §17）。
    #[test]
    fn pipe_dispatch_launched_marks_are_cleared_by_run_created_or_release() {
        let mut created = staged("t4", "s2-b-1", "s2-b", Stage::Intake);
        created.kind = EventKind::RunCreated;
        let events = vec![
            marked("t1", "s2-a", Mark::Hold),
            marked("t2", "s2-a", Mark::Launched),
            marked("t3", "s2-b", Mark::Launched),
            created,
            marked("t5", "s2-c", Mark::Launched),
            marked("t6", "s2-c", Mark::Release),
            marked("t7", "s2-d", Mark::Launched),
            marked("t8", "s2-d", Mark::Launched),
        ];
        let found = marks_of(&events);
        assert_eq!(found.order.get("s2-a").map(|(mark, _)| *mark), Some(Mark::Hold), "launched は hold を上書きしない");
        assert_eq!(found.launched.get("s2-a").map(String::as_str), Some("t2"), "受付に届いていない bead");
        assert_eq!(found.launched.get("s2-b"), None, "RunCreated が外す");
        assert_eq!(found.launched.get("s2-c"), None, "release が外す");
        assert_eq!(found.launched.get("s2-d").map(String::as_str), Some("t8"), "最新の 1 つ");
        assert_eq!(found.launched.len(), 2, "母集団 4 bead のうち残るのは 2 つ");
    }

    /// 理由の名は [`WAIT_REASONS`] と 1 対 1 で、値を持つ variant は値も描く（`dispatch ls` の `reason=`）。
    #[test]
    fn pipe_dispatch_wait_reasons_render_the_name_and_the_value() {
        let listed = vec![
            WaitReason::Dependency { on: vec!["s2-x".to_owned(), "s2-y".to_owned()] },
            WaitReason::Overlap { with: "r1".to_owned(), files: 2 },
            WaitReason::Admission { reason: "cap-headroom" },
            WaitReason::HostBusy,
            WaitReason::Hold { since: "t1".to_owned() },
            WaitReason::Launched { since: "t2".to_owned() },
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
                "host-busy",
                "hold:t1",
                "launched:t2",
                "settled:abc/Landed",
                "no-design-pointer",
            ],
            "値を持つ 6 件は値も描く"
        );
    }

    /// 自分自身の起動は起動の記述を通る（設計 core-boundary.md §9 行 d）: 記録の program は [`super::myself`]・引数は
    /// `pipe` に続く argv の逐語。記録する stub は spawn を断るので、起動は偽（呼び手は「起こせなかった」と読む）。
    /// argv は子が起きても何も撃たない形（歯の binary の該当 0 本の filter）にする。
    #[test]
    fn invocation_pipe_flow_dispatch_self_launch_failure_is_false() {
        use crate::pipe::fixture::{exited, scratch, Stub};
        let state = scratch("dispatch-self-launch");
        let argv = vec!["--exact".to_owned(), "no-such-test-invocation-dispatch".to_owned()];
        let stub = Stub::install(|_| exited(0, b""));
        assert!(!super::spawn_self(&state, &argv), "stub の断りは起動の失敗＝偽");
        let calls = stub.calls();
        assert_eq!(calls.len(), 1, "起動は 1 回: {calls:?}");
        let found: Vec<(String, Vec<String>)> = calls.into_iter().map(|call| (call.program, call.args)).collect();
        let expected: Vec<String> = std::iter::once(super::PIPE.to_owned()).chain(argv.iter().cloned()).collect();
        assert_eq!(found, vec![(super::myself(), expected)], "program は自分・引数は pipe と argv");
        let _ = std::fs::remove_dir_all(&state);
    }
}
