//! 便と列と発話の側の部品（設計 docs/design/case-lifecycle.md §9・FR90 / FR88）。
//!
//! 純関数 [`phases`] が、列の 1 周の判定・開いた契約・bead ごとの最新の便・受付の断り・event の列を受け、開いた契約の
//! 局面（contract-running → contract-refused → contract-queued → misfit `no-phase` の順）・その最新の便の局面・線より
//! 後の発話の局面を返す。便 1 本の段の写しは、段の値だけを受ける別の純関数 [`run_part`] が `Stage` の網羅 `match` で持つ
//! （段を足すと compile が落ちる・部分の書き直しは event log の末尾だけからこの関数を呼ぶ）。発話の仕分け済みかと行き先は
//! [`sorted_of`] の 1 本だけが決める（turn の終わりの判定と同じ入力に同じ結果・FR88）。
//!
//! 待ちの理由は名と値の字で受け、event は本体の `Case` で見分ける（理由と event の種類の enum の変種を名指さない）。
//! I/O は持たない。台帳・event log・札の生死は書き手が集めて渡す。

use super::wait::{epoch_of, epoch_ms_of};
use super::{Case, Channel as EventChannel, Event, Stage};
use crate::case::{turn_of, Channel, Destination, Extra, Kind, Links, Misfit, Part, Phase, Sink, Turn};
use crate::utterance::{sorted_of, Standing};
use std::collections::BTreeMap;

/// 審査と門の判定の語のうち、先へ進む側（これ以外は `run-review-failed` / `run-gate-failed`）。
pub const PASS: &str = "PASS";

/// 列の 1 周の判定 1 件（`dispatch ls` の `reason=` の名と値の字・値を持たない理由は `value` が空）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judged {
    /// 契約の bead id。
    pub bead: String,
    /// 起こさない理由の名（`dependency`・`overlap`・`admission` ほか）。
    pub name: String,
    /// 理由の値の字（`dependency` は `,` 区切りの bead id・`overlap` は `<run id>/<file 数>`・`admission` は断りの名）。
    pub value: String,
}

/// 開いた契約 1 本（閉じていない契約の bead id と、設計 pointer の字）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenContract {
    /// 契約の bead id。
    pub bead: String,
    /// 設計 pointer の字（持たない契約は `None`＝行 a1 の form-neither が持つ・本 module は部品にしない）。
    pub pointer: Option<String>,
}

/// bead ごとの最新の便（最新の段の値だけ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    /// 便の run id。
    pub run: String,
    /// 便が属する契約の bead id。
    pub bead: String,
    /// 最新の段。
    pub stage: Stage,
    /// 審査か門の判定の語（`PASS` かそれ以外・無ければ `None`）。
    pub verdict: Option<String>,
    /// 最後の detail の頭（例 `rebase-conflict`）。
    pub detail_head: Option<String>,
    /// 最新の終端の語（`terminal=` の値）。
    pub terminal: Option<String>,
    /// 運転手の札が生きているか。
    pub alive: bool,
    /// 最新の段の event の ts。
    pub ts: String,
}

/// 受付の断りの最新（bead ごと）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// 契約の bead id。
    pub bead: String,
    /// 断りの名。
    pub name: String,
    /// 断りの event の ts。
    pub ts: String,
    /// その断りの後に便が起きたか（起きていれば断りは局面に効かない）。
    pub run_after: bool,
}

/// [`phases`] への入力。
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// 列の 1 周の判定。
    pub queue: &'a [Judged],
    /// 開いた契約の列。
    pub open: &'a [OpenContract],
    /// bead ごとの最新の便（閉じた契約の bead を含んでよい）。
    pub latest: &'a [Latest],
    /// 便の run id → bead id の対応。
    pub run_beads: &'a BTreeMap<String, String>,
    /// 受付の断りの最新。
    pub refusals: &'a [Refused],
    /// event の列（発話・仕分け・bind の結びを本体の `Case` で見分ける）。
    pub events: &'a [Event],
    /// 切り替えの線の時刻（無ければ発話は 1 件も載らない）。
    pub line: Option<&'a str>,
    /// 周の時刻。
    pub now: &'a str,
    /// 終わりの局面（仕分け済みの発話）の窓の秒。
    pub window_s: u64,
}

/// 開いた契約・その最新の便・線より後の発話の部品を返す（契約の並びの順に `[契約, 便]`・後ろに発話）。
pub fn phases(input: &Input<'_>) -> Vec<Part> {
    let latest: BTreeMap<&str, &Latest> = input.latest.iter().map(|found| (found.bead.as_str(), found)).collect();
    let mut parts = Vec::new();
    for open in input.open.iter().filter(|open| open.pointer.is_some()) {
        let run = latest.get(open.bead.as_str()).copied();
        parts.push(contract_part(open, run, input));
        parts.extend(run.map(run_part));
    }
    parts.extend(utterance_parts(input));
    parts
}

/// 部品 1 つの骨（手番は語から引く・表に無い語と理由は misfit `no-phase` へ倒す＝fail-closed）。
fn part(kind: Kind, id: &str, phase: Phase, reason: Option<String>) -> Part {
    let (phase, turn, reason) = match turn_of(phase.as_str(), reason.as_deref()) {
        Some(turn) => (phase, turn, reason),
        None => (Phase::Misfit, misfit_turn(), Some(Misfit::NoPhase.as_str().to_owned())),
    };
    Part { part: kind, id: id.to_owned(), phase, turn, since: None, reason, closed: false, overdue: None, links: Links::default(), extra: Extra::None }
}

/// misfit の手番（§3 の表から引く）。
fn misfit_turn() -> Turn {
    turn_of(Phase::Misfit.as_str(), None).unwrap_or(Turn::Seat)
}

/// 便 1 本の部品（最新の段の値だけから §2.1 の表を写す・`since` は最新の段の event の ts）。
pub fn run_part(latest: &Latest) -> Part {
    let (phase, reason) = run_phase(latest);
    let mut found = part(Kind::Run, &latest.run, phase, reason);
    found.since = Some(latest.ts.clone());
    found.extra = Extra::Run { bead: latest.bead.clone() };
    found
}

/// 同じ bead の便の現在地の列（run id の昇順）を最も新しい便から前へ見て、連続の非 PASS の便の数を返す（設計 dispatcher.md §45）。
///
/// 各便の局面は [`run_part`] で写す: run-review-failed・run-gate-failed・run-failed は 1 本と数え、run-ci-waiting と run-landed-open
/// （Landed・札の生死に依らない）で止め、run-stopped とほかの局面（成否の判定に着かずに残った便）は数えず止めない。空の列は 0。
pub fn streak(runs: &[Latest]) -> usize {
    let mut count = 0;
    for latest in runs.iter().rev() {
        match run_part(latest).phase {
            Phase::RunReviewFailed | Phase::RunGateFailed | Phase::RunFailed => count += 1,
            Phase::RunCiWaiting | Phase::RunLandedOpen => break,
            _ => {}
        }
    }
    count
}

/// 段 → (語, 理由)（§2.1・`Stage` の網羅 `match`）。
fn run_phase(latest: &Latest) -> (Phase, Option<String>) {
    let stage = Some(latest.stage.as_str().to_owned());
    let passed = latest.verdict.as_deref() == Some(PASS);
    let verdict = latest.verdict.clone();
    match latest.stage {
        Stage::Intake => (Phase::RunIntake, stage),
        Stage::Reviewed if passed => (Phase::RunReviewed, stage),
        Stage::Reviewed => (Phase::RunReviewFailed, verdict),
        Stage::Blocked => (Phase::RunBlocked, stage),
        Stage::Spawned => (Phase::RunImplementing, stage),
        Stage::Questioned => (Phase::RunAsking, stage),
        Stage::RateLimited => (Phase::RunRateLimited, stage),
        Stage::Implemented => (Phase::RunGating, stage),
        Stage::Gated if passed => (Phase::RunLanding, stage),
        Stage::Gated => (Phase::RunGateFailed, verdict),
        Stage::Landed if latest.alive => (Phase::RunCiWaiting, latest.terminal.clone().or(stage)),
        Stage::Landed => (Phase::RunLandedOpen, latest.terminal.clone()),
        Stage::Stopped => (Phase::RunStopped, stage),
        Stage::Failed => (Phase::RunFailed, latest.detail_head.clone()),
    }
}

/// 開いた契約の部品（札の生きた便 → 断り → 列で待つ → `no-phase` の順・上が勝つ）。
fn contract_part(open: &OpenContract, run: Option<&Latest>, input: &Input<'_>) -> Part {
    let queued = input.queue.iter().find(|judged| judged.bead == open.bead);
    let refusal = input.refusals.iter().find(|refused| refused.bead == open.bead && !refused.run_after);
    let (phase, reason, since) = if let Some(live) = run.filter(|found| found.alive) {
        (Phase::ContractRunning, Some(run_phase(live).0.as_str().to_owned()), None)
    } else if let Some((name, since)) = refusal_of(queued, refusal) {
        (Phase::ContractRefused, Some(name), since)
    } else if let Some(judged) = queued {
        (Phase::ContractQueued, Some(judged.name.clone()), None)
    } else {
        (Phase::Misfit, Some(Misfit::NoPhase.as_str().to_owned()), None)
    };
    let mut found = part(Kind::Contract, &open.bead, phase, reason);
    found.since = since;
    found.links.runs = run.map(|latest| latest.run.clone()).into_iter().collect();
    found.links.on = queued.map(|judged| links_on(judged, input.run_beads)).unwrap_or_default();
    found.extra = Extra::Contract { pointer: open.pointer.clone() };
    found
}

/// 断りの名と since（列の理由が `admission` ならその値の断りの名・無ければ便の後に起きていない受付の断り）。
fn refusal_of(queued: Option<&Judged>, refusal: Option<&Refused>) -> Option<(String, Option<String>)> {
    match (queued, refusal) {
        (Some(judged), _) if judged.name == "admission" => Some((judged.value.clone(), None)),
        (_, Some(refused)) => Some((refused.name.clone(), Some(refused.ts.clone()))),
        _ => None,
    }
}

/// 列の理由が `dependency` なら相手の列、`overlap` なら相手の便の bead（対応に無い run id は載せない）、`reserved` なら行を予約した bead。
fn links_on(judged: &Judged, run_beads: &BTreeMap<String, String>) -> Vec<String> {
    match judged.name.as_str() {
        "dependency" => judged.value.split(',').filter(|found| !found.is_empty()).map(str::to_owned).collect(),
        "overlap" => {
            let run = judged.value.rsplit_once('/').map_or(judged.value.as_str(), |(run, _)| run);
            run_beads.get(run).cloned().into_iter().collect()
        }
        "reserved" => {
            let bead = judged.value.split_once('/').map_or(judged.value.as_str(), |(bead, _)| bead);
            Some(bead).filter(|found| !found.is_empty()).map(str::to_owned).into_iter().collect()
        }
        _ => Vec::new(),
    }
}

/// 時刻の字（秒の字も ms の字も）から epoch ms。
fn ms_of(ts: &str) -> Option<u64> {
    epoch_ms_of(ts).or_else(|| epoch_of(ts).map(|secs| secs.saturating_mul(1_000)))
}

/// 秒より下の桁を落とす（出力の時刻は UTC の秒まで・id だけが発話の ts の字のまま）。
fn seconds(ts: &str) -> String {
    ts.split_once('.').map_or_else(|| ts.to_owned(), |(head, _)| format!("{head}Z"))
}

/// 線より後の発話の部品（event の並びの順）。
fn utterance_parts(input: &Input<'_>) -> Vec<Part> {
    let Some(line) = input.line.and_then(ms_of) else {
        return Vec::new();
    };
    let after = |event: &Event| !ms_of(&event.ts).is_some_and(|at| at <= line);
    let said = input.events.iter().filter(|event| after(event));
    said.filter_map(|event| match &event.case {
        Some(Case::Utterance { channel, session }) => utterance_part(event, *channel, session, input),
        _ => None,
    })
    .collect()
}

/// 発話 1 件の部品（仕分け済みかと行き先は [`sorted_of`] の結果・逐語は持たない・窓の外の仕分け済みは載せない）。
fn utterance_part(event: &Event, channel: EventChannel, session: &Option<String>, input: &Input<'_>) -> Option<Part> {
    let sorted_at = earliest_pointing_ts(&event.ts, input.events);
    let (phase, since, destination) = match sorted_of(&event.ts, input.events) {
        Standing::Unsorted => (Phase::UtteranceOpen, Some(seconds(&event.ts)), Vec::new()),
        Standing::ChatOnly => (Phase::UtteranceSorted, sorted_at.map(seconds), vec![Destination { to: Sink::ToChat, id: None }]),
        Standing::Linked { memos, rulings } => {
            let to = |sink: Sink, id: String| Destination { to: sink, id: Some(id) };
            let memos = memos.into_iter().map(|id| to(Sink::ToMemo, id));
            (Phase::UtteranceSorted, sorted_at.map(seconds), memos.chain(rulings.into_iter().map(|id| to(Sink::ToRuling, id))).collect())
        }
    };
    if phase == Phase::UtteranceSorted && !within_window(sorted_at, input) {
        return None;
    }
    let mut found = part(Kind::Utterance, &event.ts, phase, None);
    found.since = since;
    found.links.destination = destination;
    found.extra = Extra::Utterance { session: session.clone(), channel: channel_word(channel) };
    Some(found)
}

/// 発話の入口の写し（網羅 `match`）。
fn channel_word(channel: EventChannel) -> Channel {
    match channel {
        EventChannel::Chat => Channel::Chat,
        EventChannel::Gui => Channel::Gui,
    }
}

/// 発話 `ts` を指す仕分けと bind の結びの event のうち最も早い ts（時刻を読めない event は数えない）。
fn earliest_pointing_ts<'a>(ts: &str, events: &'a [Event]) -> Option<&'a str> {
    let points = |event: &Event| matches!(&event.case, Some(Case::Sorted { utterance, .. } | Case::Ruling { utterance, .. }) if utterance == ts);
    let stamped = events.iter().filter(|event| points(event)).filter_map(|event| Some((ms_of(&event.ts)?, event.ts.as_str())));
    stamped.min_by_key(|(at, _)| *at).map(|(_, found)| found)
}

/// 仕分けの時刻が窓の内か（時刻が読めない周は載せる側へ倒す）。
fn within_window(sorted_at: Option<&str>, input: &Input<'_>) -> bool {
    match (sorted_at.and_then(ms_of), ms_of(input.now)) {
        (Some(at), Some(now)) => now.saturating_sub(at) <= input.window_s.saturating_mul(1_000),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{phases, run_part, streak, Input, Judged, Latest, OpenContract, Refused, PASS};
    use crate::case::{Channel, Extra, Kind, Part, Phase, Sink, Turn};
    use crate::fleet::{Event, Stage};
    use std::collections::BTreeMap;

    const NOW: &str = "2026-09-30T12:00:00Z";
    const LINE: &str = "2026-09-30T00:00:00Z";
    const WINDOW_S: u64 = 3_600;

    /// 便の対応・列の判定・最新の便・断り・event を持つ入力の元。
    #[derive(Default)]
    struct World {
        queue: Vec<Judged>,
        open: Vec<OpenContract>,
        latest: Vec<Latest>,
        run_beads: BTreeMap<String, String>,
        refusals: Vec<Refused>,
        events: Vec<Event>,
    }

    impl World {
        fn with_events(events: Vec<Event>) -> Self {
            Self { events, ..Self::default() }
        }

        fn phases(&self) -> Vec<Part> {
            phases(&Input {
                queue: &self.queue,
                open: &self.open,
                latest: &self.latest,
                run_beads: &self.run_beads,
                refusals: &self.refusals,
                events: &self.events,
                line: Some(LINE),
                now: NOW,
                window_s: WINDOW_S,
            })
        }

        fn contract(&self, bead: &str) -> Part {
            let found = self.phases().into_iter().find(|found| found.part == Kind::Contract && found.id == bead);
            found.unwrap_or_else(|| panic!("契約 {bead} の部品が無い"))
        }
    }

    fn open(bead: &str) -> OpenContract {
        OpenContract { bead: bead.to_owned(), pointer: Some("docs/design/x.md#b".to_owned()) }
    }

    fn judged(bead: &str, name: &str, value: &str) -> Judged {
        Judged { bead: bead.to_owned(), name: name.to_owned(), value: value.to_owned() }
    }

    fn latest(bead: &str, run: &str, stage: Stage, alive: bool) -> Latest {
        Latest {
            run: run.to_owned(),
            bead: bead.to_owned(),
            stage,
            verdict: Some(PASS.to_owned()),
            detail_head: None,
            terminal: None,
            alive,
            ts: "2026-09-30T11:00:00Z".to_owned(),
        }
    }

    fn refused(bead: &str, name: &str, run_after: bool) -> Refused {
        Refused { bead: bead.to_owned(), name: name.to_owned(), ts: "2026-09-30T10:00:00Z".to_owned(), run_after }
    }

    fn event(line: &str) -> Event {
        Event::from_line(line).unwrap_or_else(|why| panic!("{line}: {why}"))
    }

    fn utterance(ts: &str, channel: &str, session: Option<&str>) -> Event {
        let session = session.map(|found| format!(r#""session":"{found}","#)).unwrap_or_default();
        event(&format!(r#"{{"schema":1,"ts":"{ts}","kind":"UtteranceReceived","channel":"{channel}",{session}"host":"h","actor":"human","detail":"ひみつの逐語"}}"#))
    }

    fn sorted(ts: &str, of: &str, sorting: &str, memo: Option<&str>) -> Event {
        let bead = memo.map(|found| format!(r#""bead":"{found}","#)).unwrap_or_default();
        event(&format!(r#"{{"schema":1,"ts":"{ts}","kind":"UtteranceSorted","utterance":"{of}","sorting":"{sorting}",{bead}"host":"h","actor":"machine"}}"#))
    }

    fn bound(ts: &str, of: &str, question: &str, ruling: &str) -> Event {
        event(&format!(
            r#"{{"schema":1,"ts":"{ts}","kind":"RulingReceived","bead":"{question}","ruling":"{ruling}","utterance":"{of}","channel":"chat","question_ts":"2026-09-30T05:00:00Z","host":"h","actor":"human","detail":"裁定の逐語"}}"#
        ))
    }

    fn utterance_parts(world: &World) -> Vec<Part> {
        world.phases().into_iter().filter(|found| found.part == Kind::Utterance).collect()
    }

    /// (1) 入力の組が不足でも落ちない（空の入力は部品を返さない）。
    #[test]
    fn phase_event_empty_inputs_return_no_parts() {
        assert!(World::default().phases().is_empty());
        let mut world = World::default();
        world.queue.push(judged("b-1", "hold", ""));
        assert!(world.phases().is_empty(), "開いた契約の列が空なら列の判定だけでは部品を作らない");
    }

    /// (2) 札の生きた便は running（理由は便の局面の語・手番 none）で、admission の理由が同時に在っても上が勝つ。
    #[test]
    fn phase_event_running_wins_over_admission_and_names_the_run_phase() {
        let mut world = World::default();
        world.open.push(open("b-1"));
        world.queue.push(judged("b-1", "admission", "no-headroom"));
        world.latest.push(latest("b-1", "r-1", Stage::Implemented, true));
        let found = world.contract("b-1");
        assert_eq!((found.phase, found.turn), (Phase::ContractRunning, Turn::Nobody));
        assert_eq!(found.reason.as_deref(), Some("run-gating"));
        assert_eq!(found.links.runs, ["r-1"]);
        let run = world.phases().into_iter().find(|found| found.part == Kind::Run).map(|found| (found.id, found.phase, found.turn));
        assert_eq!(run, Some(("r-1".to_owned(), Phase::RunGating, Turn::Vessel)));
        world.latest[0].alive = false;
        assert_eq!(world.contract("b-1").phase, Phase::ContractRefused, "札が死ねば admission の断りへ落ちる");
    }

    /// (2) refused の 2 経路（列の理由 admission は値の断りの名・受付の断りの event は event の断りの名・手番 seat）。
    #[test]
    fn phase_event_refused_takes_the_name_from_admission_or_from_the_intake_refusal() {
        let mut world = World::default();
        world.open.extend([open("b-1"), open("b-2")]);
        world.queue.push(judged("b-1", "admission", "no-headroom"));
        world.refusals.push(refused("b-2", "floor-red", false));
        let (queue, event) = (world.contract("b-1"), world.contract("b-2"));
        assert_eq!((queue.phase, queue.turn, queue.reason.as_deref()), (Phase::ContractRefused, Turn::Seat, Some("no-headroom")));
        assert_eq!((event.phase, event.turn, event.reason.as_deref()), (Phase::ContractRefused, Turn::Seat, Some("floor-red")));
        assert_eq!(event.since.as_deref(), Some("2026-09-30T10:00:00Z"));
    }

    /// (2) 便の後に起きていない受付の断りは、列の理由が dependency でも refused（queued より上）・断りの後に便が起きたら queued。
    #[test]
    fn phase_event_refusal_beats_dependency_until_a_run_follows_it() {
        let mut world = World::default();
        world.open.push(open("b-1"));
        world.queue.push(judged("b-1", "dependency", "b-9"));
        world.refusals.push(refused("b-1", "floor-red", false));
        assert_eq!(world.contract("b-1").phase, Phase::ContractRefused);
        world.refusals[0].run_after = true;
        let found = world.contract("b-1");
        assert_eq!((found.phase, found.reason.as_deref()), (Phase::ContractQueued, Some("dependency")));
    }

    /// (2) queued の理由の各語の手番（§3）。
    #[test]
    fn phase_event_queued_reasons_carry_their_turn() {
        let table = [
            ("dependency", Turn::Vessel),
            ("overlap", Turn::Vessel),
            ("host-busy", Turn::Vessel),
            ("launched", Turn::Vessel),
            ("hold", Turn::Seat),
            ("no-design-pointer", Turn::Seat),
            ("unreflected-ruling", Turn::Seat),
            ("floor", Turn::Seat),
            ("settled", Turn::Nobody),
        ];
        for (name, turn) in table {
            let mut world = World::default();
            world.open.push(open("b-1"));
            world.queue.push(judged("b-1", name, "v"));
            let found = world.contract("b-1");
            assert_eq!((found.phase, found.turn, found.reason.as_deref()), (Phase::ContractQueued, turn, Some(name)), "{name}");
        }
        let mut world = World::default();
        world.open.push(open("b-1"));
        world.queue.push(judged("b-1", "no-such-reason", ""));
        let found = world.contract("b-1");
        assert_eq!((found.phase, found.turn, found.reason.as_deref()), (Phase::Misfit, Turn::Seat, Some("no-phase")));
    }

    /// (2) dependency の links.on は相手の列・overlap の links.on は相手の便の bead（対応に無い run id は持たない）。
    #[test]
    fn phase_event_links_on_follows_dependency_and_overlap_partner() {
        let mut world = World::default();
        world.open.extend([open("b-1"), open("b-2"), open("b-3")]);
        world.queue.extend([judged("b-1", "dependency", "b-7,b-8"), judged("b-2", "overlap", "r-9/3"), judged("b-3", "overlap", "r-0/1")]);
        world.run_beads.insert("r-9".to_owned(), "b-9".to_owned());
        assert_eq!(world.contract("b-1").links.on, ["b-7", "b-8"]);
        assert_eq!(world.contract("b-2").links.on, ["b-9"]);
        assert!(world.contract("b-3").links.on.is_empty(), "引けない run id は載せない");
    }

    /// (1) reserved の links.on は値（<予約した bead>/<file 数>・末尾に /unset）の最初の / より前の bead 1 つ（/ が無ければ値の全体）。
    #[test]
    fn phase_event_links_on_reserved_takes_the_reserving_bead() {
        let mut world = World::default();
        world.open.extend([open("b-1"), open("b-2"), open("b-3")]);
        world.queue.extend([judged("b-1", "reserved", "b-6/2"), judged("b-2", "reserved", "b-6/2/unset"), judged("b-3", "reserved", "b-6")]);
        for bead in ["b-1", "b-2", "b-3"] {
            assert_eq!(world.contract(bead).links.on, ["b-6"], "{bead}");
        }
    }

    /// (2) 列の判定に無い開いた契約は no-phase・pointer の無い契約は部品にならず（同じ契約が pointer を持てば queued）・閉じた契約の便は載らない。
    #[test]
    fn phase_event_no_phase_pointerless_and_closed_contracts() {
        let mut world = World::default();
        world.open.push(open("b-1"));
        let found = world.contract("b-1");
        assert_eq!((found.phase, found.reason.as_deref()), (Phase::Misfit, Some("no-phase")));

        let mut world = World::default();
        world.open.push(OpenContract { bead: "b-2".to_owned(), pointer: None });
        world.queue.push(judged("b-2", "dependency", "b-9"));
        world.latest.push(latest("b-2", "r-2", Stage::Spawned, true));
        assert!(world.phases().is_empty(), "pointer の無い契約は列の理由が何でも便も含めて部品にならない");
        world.open[0].pointer = Some("docs/design/x.md#b".to_owned());
        assert_eq!(world.contract("b-2").phase, Phase::ContractRunning);

        let mut world = World::default();
        world.latest.push(latest("b-closed", "r-3", Stage::Landed, false));
        assert!(world.phases().is_empty(), "開いた契約の列に無い bead の便は載らない");
        world.open.push(open("b-closed"));
        assert_eq!(world.phases().iter().filter(|found| found.part == Kind::Run).count(), 1);
    }

    /// (3) 判定の PASS と PASS でない・Landed の札の生死・Failed の理由。
    #[test]
    fn phase_event_run_part_reads_verdict_terminal_and_detail_head() {
        let mut spec = latest("b-1", "r-1", Stage::Gated, false);
        spec.verdict = Some("FAIL".to_owned());
        let found = run_part(&spec);
        assert_eq!((found.phase, found.turn, found.reason.as_deref()), (Phase::RunGateFailed, Turn::Seat, Some("FAIL")));
        spec.verdict = Some(PASS.to_owned());
        assert_eq!(run_part(&spec).phase, Phase::RunLanding);
        spec.stage = Stage::Landed;
        spec.terminal = Some("ci:failure".to_owned());
        let dead = run_part(&spec);
        assert_eq!((dead.phase, dead.reason.as_deref()), (Phase::RunLandedOpen, Some("ci:failure")));
        spec.alive = true;
        let live = run_part(&spec);
        assert_eq!((live.phase, live.turn, live.reason.as_deref()), (Phase::RunCiWaiting, Turn::Ci, Some("ci:failure")));
        spec.terminal = None;
        assert_eq!(run_part(&spec).reason.as_deref(), Some("Landed"));
        spec.stage = Stage::Failed;
        spec.detail_head = Some("rebase-conflict".to_owned());
        assert_eq!(run_part(&spec).reason.as_deref(), Some("rebase-conflict"));
        assert_eq!(run_part(&spec).extra, Extra::Run { bead: "b-1".to_owned() });
        assert_eq!(run_part(&spec).since.as_deref(), Some("2026-09-30T11:00:00Z"));
    }

    /// (4) 未仕分け・会話だけ・結びありの 3 形と、行き先の全部（memo 2 つと裁定 1 つ）・id と session と channel の写し・since。
    #[test]
    fn phase_event_utterance_open_chat_only_and_linked_destinations() {
        let (a, b, c) = ("2026-09-30T07:05:09.123Z", "2026-09-30T08:00:00.500Z", "2026-09-30T09:00:00.001Z");
        let world = World::with_events(vec![
            utterance(a, "chat", Some("s-1")),
            utterance(b, "gui", None),
            utterance(c, "chat", Some("s-2")),
            sorted("2026-09-30T11:30:00Z", b, "chat", None),
            sorted("2026-09-30T11:31:00Z", c, "request", Some("m-2")),
            sorted("2026-09-30T11:20:00Z", c, "request", Some("m-1")),
            bound("2026-09-30T11:40:00Z", c, "q-1", "r-7"),
        ]);
        let found = utterance_parts(&world);
        let ids: Vec<&str> = found.iter().map(|part| part.id.as_str()).collect();
        assert_eq!(ids, [a, b, c], "id は発話の ts の字のまま");
        assert_eq!((found[0].phase, found[0].turn), (Phase::UtteranceOpen, Turn::Seat));
        assert_eq!(found[0].since.as_deref(), Some("2026-09-30T07:05:09Z"));
        assert_eq!(found[0].extra, Extra::Utterance { session: Some("s-1".to_owned()), channel: Channel::Chat });
        assert_eq!(found[1].extra, Extra::Utterance { session: None, channel: Channel::Gui });
        assert_eq!((found[1].phase, found[1].turn), (Phase::UtteranceSorted, Turn::Nobody));
        assert_eq!((found[1].links.destination[0].to, found[1].links.destination[0].id.clone()), (Sink::ToChat, None));
        assert_eq!(found[1].links.destination.len(), 1);
        let to: Vec<(Sink, Option<&str>)> = found[2].links.destination.iter().map(|dest| (dest.to, dest.id.as_deref())).collect();
        assert_eq!(to, [(Sink::ToMemo, Some("m-1")), (Sink::ToMemo, Some("m-2")), (Sink::ToRuling, Some("r-7"))]);
        assert_eq!(found[2].since.as_deref(), Some("2026-09-30T11:20:00Z"), "仕分けなら最も早い仕分けか bind の ts");
    }

    /// (4) 会話の後に要望が足された発話は memo だけを行き先に持ち chat を持たない・since は最も早い仕分け（会話の札）の ts。
    #[test]
    fn phase_event_chat_then_request_has_only_the_memo_destination() {
        let ts = "2026-09-30T07:05:09.123Z";
        let world = World::with_events(vec![
            utterance(ts, "chat", Some("s-1")),
            sorted("2026-09-30T11:00:00Z", ts, "chat", None),
            sorted("2026-09-30T11:10:00Z", ts, "request", Some("m-1")),
        ]);
        let found = utterance_parts(&world);
        let to: Vec<(Sink, Option<&str>)> = found[0].links.destination.iter().map(|dest| (dest.to, dest.id.as_deref())).collect();
        assert_eq!(to, [(Sink::ToMemo, Some("m-1"))]);
        assert_eq!(found[0].since.as_deref(), Some("2026-09-30T11:00:00Z"));
    }

    /// (4) 線より前の発話は載らず（線より後へ動かすと載る）・窓の外の仕分け済みは載らず（開きは窓を掛けない）・逐語は出力に無い。
    #[test]
    fn phase_event_utterance_line_window_and_no_verbatim() {
        let (before, old, fresh) = ("2026-09-29T23:59:59.999Z", "2026-09-30T01:00:00.000Z", "2026-09-30T02:00:00.000Z");
        let world = World::with_events(vec![
            utterance(before, "chat", Some("s-1")),
            utterance(old, "chat", Some("s-1")),
            utterance(fresh, "chat", Some("s-1")),
            sorted("2026-09-30T02:00:00Z", old, "chat", None),
        ]);
        let ids: Vec<String> = utterance_parts(&world).into_iter().map(|found| found.id).collect();
        assert_eq!(ids, [fresh], "before は線より前・old は仕分けが窓の外・fresh は開きで窓を掛けない");
        let parts = world.phases();
        assert!(!format!("{parts:?}").contains("ひみつの逐語"), "逐語は部品に無い");
        let after = phases(&Input { line: Some("2026-09-29T00:00:00Z"), ..input_of(&world) });
        assert!(after.iter().any(|found| found.id == before), "線を前へ動かせば載る");
        let none = phases(&Input { line: None, ..input_of(&world) });
        assert!(none.is_empty(), "線が無ければ発話は載らない");
    }

    /// 判定の語 `verdict` を持つ `stage` の便（札は死んでいる）。
    fn judged_run(stage: Stage, verdict: &str) -> Latest {
        Latest { verdict: Some(verdict.to_owned()), ..latest("b-1", "r-1", stage, false) }
    }

    fn failed() -> Latest {
        Latest { detail_head: Some("rebase-conflict".to_owned()), ..latest("b-1", "r-1", Stage::Failed, false) }
    }

    fn landed(alive: bool) -> Latest {
        Latest { terminal: Some("closed".to_owned()), ..latest("b-1", "r-1", Stage::Landed, alive) }
    }

    fn stopped() -> Latest {
        latest("b-1", "r-1", Stage::Stopped, false)
    }

    /// (a) 数える 3 局面（INCONCLUSIVE の Reviewed・FAIL の Gated・Failed）は各 1 本で 1・3 本で 3・空で 0。
    #[test]
    fn phase_streak_counts_review_gate_and_failed_runs() {
        let (reviewed, gated) = (judged_run(Stage::Reviewed, "INCONCLUSIVE"), judged_run(Stage::Gated, "FAIL"));
        assert_eq!(streak(&[reviewed.clone(), gated.clone(), failed()]), 3);
        for single in [reviewed, gated, failed()] {
            assert_eq!(streak(&[single]), 1);
        }
        assert_eq!(streak(&[]), 0);
    }

    /// (b) Landed（札の死んだ便も生きた便も）で止め、それより前を数えない。
    #[test]
    fn phase_streak_stops_at_landed_dead_or_alive() {
        for alive in [false, true] {
            let runs = [failed(), failed(), landed(alive), judged_run(Stage::Gated, "FAIL")];
            assert_eq!(streak(&runs), 1, "alive={alive}");
        }
    }

    /// (c) Stopped は数えず止めもしない（最も新しい便が Stopped でも前を数える）。
    #[test]
    fn phase_streak_skips_stopped_without_stopping() {
        assert_eq!(streak(&[failed(), stopped(), judged_run(Stage::Gated, "FAIL")]), 2);
        assert_eq!(streak(&[judged_run(Stage::Gated, "FAIL"), stopped()]), 1);
    }

    /// (d) ほかの 8 局面は数えず止めない（Failed 2 本の間に置いても 2）。
    #[test]
    fn phase_streak_skips_the_other_eight_phases() {
        let others = [
            latest("b-1", "r-1", Stage::Intake, false),
            judged_run(Stage::Reviewed, PASS),
            latest("b-1", "r-1", Stage::Blocked, false),
            latest("b-1", "r-1", Stage::Spawned, false),
            latest("b-1", "r-1", Stage::Questioned, false),
            latest("b-1", "r-1", Stage::RateLimited, false),
            latest("b-1", "r-1", Stage::Implemented, false),
            judged_run(Stage::Gated, PASS),
        ];
        let mut checked = 0;
        for other in others {
            assert_eq!(streak(&[failed(), other, failed()]), 2);
            checked += 1;
        }
        assert_eq!(checked, 8);
    }

    fn input_of(world: &World) -> Input<'_> {
        Input {
            queue: &world.queue,
            open: &world.open,
            latest: &world.latest,
            run_beads: &world.run_beads,
            refusals: &world.refusals,
            events: &world.events,
            line: Some(LINE),
            now: NOW,
            window_s: WINDOW_S,
        }
    }
}
