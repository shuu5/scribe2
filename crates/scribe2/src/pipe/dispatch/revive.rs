//! driver の継ぎの判定と起こし直す便の選別・構築（設計 docs/design/dispatcher.md §44・契約表の行 as）。
//!
//! 段の動きと渡すかの判定（[`rank`]・[`advance`]・[`handoff`]・[`progress_of`]・[`admits_gated`]）と、止まった便を
//! 起こし直す選別と構築（[`revivals`]・[`revive_of`]・[`resume`]）の群である。`pipe/dispatch.rs` からの**純移動**で、
//! 歯は 1 本も足していない（親に残る in-file の歯と e2e が従来どおり測る）。型 `Advance`・`Handoff` と起こす面は
//! **親に残る**。親が呼ぶ 4 名だけが `pub(super)` で、`rank` は親の歯が引く。`pub` の 3 名は親が再輸出する。

use super::super::cli::{live, stage_of};
use super::super::gate::Verdict;
use super::super::land::verdict_of;
use super::super::regate::{followed_since_gate, regated_since_gate};
use super::super::{current, Ticket};
use super::{spawn_self, tools, Advance, Handoff, Input, Revive, DRIVE};
use crate::fleet::store;
use crate::fleet::{replay, Event, Stage, State, STAGES};
use std::path::Path;

/// 段の宣言順の位置（[`STAGES`] から導く＝順序の宣言は 1 か所・C2）。
///
/// 全 variant が [`STAGES`] に在ることは in-file の歯が母集団つきで測る（`unwrap_or` の値は
/// 到達しない）。
pub(super) fn rank(stage: Stage) -> usize {
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
pub(super) fn progress_of(input: &Input<'_>) -> Option<(Option<Advance>, Handoff)> {
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
pub(super) fn revivals(input: &Input<'_>, gated: bool) -> Vec<Revive> {
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
pub(super) fn revive_of(input: &Input<'_>, run: &str) -> Revive {
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
pub(super) fn resume(input: &Input<'_>, revive: &Revive) -> bool {
    spawn_self(input.state_dir, &revive.argv)
}
