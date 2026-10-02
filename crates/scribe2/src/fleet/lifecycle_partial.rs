//! 部分の書き直し（設計 docs/design/case-lifecycle.md §13・FR90 / FR27 / NFR5・ADR-0088）。
//!
//! 管理 tick の周に、台帳も git も撃たず、event log の末尾（今の出力の `inputs.events.len` から先）と今の出力だけから、発話・発端の
//! 結び・便・期日・年齢と `owned` を書き直す（[`rewrite`]）。読むのは今の `lifecycle.json`・log の 1 行目と `len` の直前の 1 byte と
//! 末尾（store の [`store::read_joined`]）・埋め込みの rules・末尾に段の行を持つ便の運転手の札だけで、子 process を撃たない。
//! 出力が無い置き場は何もせず（[`Rewrote::Absent`]）、lock は死んだ所有者だけ外す取り方で 200 ms まで待って取れなければ
//! [`Wrote::Busy`] で飛ぶ。log が繋がらない周（head が違う・短い・直前が改行でない）は書かず、`unreadable`（理由 `events`）の印を
//! 付ける。部分の書き直しは印を消さない。書くのは全部の書き直しと同じ書き手 [`write`] の 1 本で、`scope` を `partial` にし、
//! `inputs.events` だけを進める（ledger・main・`full_at` と、契約・問い・行・要件・epic・commit の部品の局面と理由は動かさない）。
//!
//! 判定は行 a・b の関数を呼ぶ: 発話は [`run_phase::phases`] と [`sorted_of`]・便の段は [`run_part`]・期日は [`trigger::met`]。
//! event は本体の `Case` と段の欄と kind の字で見分ける。

use super::lifecycle::{count_owned, full, read_output, write, Output, Place, Reading, Request, Scope, Source, Wrote};
use super::lifecycle_line::read_lines;
use super::lifecycle_mark::{
    self as mark, census_anchors, fleet_dir, hold, latest_runs, publish, read_ledger, read_main, secs_of, AnchorCensus, Events, Kind as MarkKind, Marks, JSON_FILE, JSON_LOCK,
};
use super::phase::{self as run_phase, run_part, Latest};
use super::store::{self, LockPolicy, Tail};
use super::{cli::format_utc, epoch_of, replay, Case, Event};
use crate::case::{turn_of, Destination, Extra, Kind, Part, Phase, Sink};
use crate::ledger::phase::REASON_TRIGGER_MET;
use crate::ledger::trigger::{self, Kind as TriggerKind, Trigger, World};
use crate::rules::int_row;
use crate::rules::manifest::Manifest;
use crate::utterance::{sorted_of, Standing};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek};
use std::path::Path;

/// `lifecycle.lock` を待つ上限（hook の予算を食わない短さ）。
const LOCK_WAIT_MS: u64 = 200;

/// 部分の書き直しの返り。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rewrote {
    /// 出力が無い置き場（作るのは全部の書き直しだけ・何もしない）。
    Absent,
    /// 書き手（または読めない周の印付け）の返り。
    Wrote(Wrote),
}

/// 管理 tick の周が撃つ 1 本: 置き場の出力を部分に書き直す（呼び手の rc と字は変えない・返りは捨ててよい）。
pub fn rewrite(state_dir: &Path) -> Rewrote {
    if !fleet_dir(state_dir).join(JSON_FILE).is_file() {
        return Rewrote::Absent;
    }
    let (Ok(manifest), Ok(policy)) = (Manifest::embedded(), LockPolicy::embedded()) else {
        return Rewrote::Wrote(Wrote::Unreadable("rules"));
    };
    let place = Place { state_dir, repo: state_dir, manifest: &manifest, bd: "", policy };
    let now = crate::seat::state::now_secs();
    match std::fs::File::open(store::events_path(state_dir)) {
        Ok(mut log) => rewrite_with(&place, &mut log, now),
        Err(_) => rewrite_with(&place, &mut Cursor::new(Vec::new()), now),
    }
}

/// [`rewrite`] の本体（log の読み手と周の時刻を受ける・歯が読む byte を数える包みを渡せる）。
pub fn rewrite_with<R: Read + Seek>(place: &Place<'_>, log: &mut R, now: u64) -> Rewrote {
    if !fleet_dir(place.state_dir).join(JSON_FILE).is_file() {
        return Rewrote::Absent;
    }
    let policy = LockPolicy { retry_ms: LOCK_WAIT_MS, stale_ms: place.policy.stale_ms };
    let Some(held) = hold(&fleet_dir(place.state_dir), JSON_LOCK, policy) else { return Rewrote::Wrote(Wrote::Busy) };
    let prior = match read_output(place.state_dir) {
        Reading::Read(found) => *found,
        Reading::Absent => return Rewrote::Absent,
        Reading::Unreadable => return Rewrote::Wrote(Wrote::Unreadable("output")),
    };
    let Ok(joined) = store::read_joined(log, prior.marks.events.len, &prior.marks.events.head) else {
        let found = mark::Mark { kind: MarkKind::Unreadable, at: format_utc(now), value: mark::Value::Reason("events".to_owned()) };
        let _ = mark::add_mark(place.state_dir, &found, place.policy);
        return Rewrote::Wrote(Wrote::Unreadable("events"));
    };
    let out = derive(place, prior, joined, now);
    drop(held);
    Rewrote::Wrote(write(place, &out, Some(LOCK_WAIT_MS)))
}

/// 管理 tick が全部の書き直しを撃った記録（置き場の `fleet/` の下・1 行 `ts=<epoch 秒> wrote=<返りの語>`・設計 §19 約束 3）。
const TICK_FILE: &str = "lifecycle.tick";

/// 管理 tick の周が撃つ全部の書き直し 1 本（設計 §19 約束 2・6）: 出力が読め・tick の manifest に `lifecycle.full_min_s` の行が在り・今が
/// `full_at` と記録の ts の新しい方から行の秒以上後で・anchor の台帳の印か main の sha が出力の印と等しくなく・置き場の anchor が 1 つ
/// （[`census_anchors`] が One）の周だけ、観測の 1 周・coalesce・lock を待たない形で撃ち、返りの語を記録に書く。撃たない周は `None`
/// （bd も git も撃たない比べだけ・数えの git は 5 つを満たした周だけ）。`events` は tick が判定の前に読んだ列。
pub fn tick_full(state_dir: &Path, manifest: &Manifest, (anchor, bd): (&Path, &str), events: &[Event]) -> Option<Wrote> {
    let Reading::Read(out) = read_output(state_dir) else { return None };
    let min = int_row(manifest, "lifecycle.full_min_s").ok()?;
    let now = crate::seat::state::now_secs();
    let fired = std::fs::read_to_string(fleet_dir(state_dir).join(TICK_FILE)).ok().and_then(|text| text.strip_prefix("ts=")?.split(' ').next()?.parse::<u64>().ok());
    let last = epoch_of(&out.full_at).unwrap_or(0).max(fired.unwrap_or(0));
    let moved = read_ledger(anchor).zip(read_main(anchor)).is_some_and(|(ledger, main)| ledger != out.marks.ledger || main != out.marks.main);
    if now < last.saturating_add(min) || !moved || census_anchors(state_dir, anchor, events) != AnchorCensus::One {
        return None;
    }
    let place = Place { state_dir, repo: anchor, manifest, bd, policy: LockPolicy::from_rules(manifest).ok()? };
    let wrote = full(&place, Source::Observe, Request { wait_ms: Some(0), coalesce: true });
    let _ = publish(&fleet_dir(state_dir), TICK_FILE, &format!("ts={now} wrote={}\n", wrote.word()));
    Some(wrote)
}

/// 末尾の周の材料（発話の線・周の時刻・窓の秒）。
struct Beat<'a> {
    /// 末尾の event。
    events: &'a [Event],
    /// 発話の線（末尾に切り替えの線が在ればその ts・無ければ出力の head）。
    line: Option<String>,
    /// 周の時刻。
    now: String,
    /// 終わりの局面の窓の秒。
    window_s: u64,
}

/// 末尾と今の出力から出力の型の値を導く（scope は partial・`inputs.events` だけを進める）。
fn derive(place: &Place<'_>, mut prior: Output, joined: (Option<String>, Tail), now: u64) -> Output {
    let (head, tail) = joined;
    let mut parts = std::mem::take(&mut prior.parts);
    let line = read_lines(&tail.events).cutover.map(|found| found.ts).or_else(|| prior.marks.events.head.clone());
    let window_s = prior.closed_window_h.map_or(u64::MAX, |hours| hours.saturating_mul(3_600));
    let beat = Beat { events: &tail.events, line, now: format_utc(now), window_s };
    utterances(&mut parts, &beat);
    runs(&mut parts, place.state_dir, &tail.events);
    for part in parts.iter_mut() {
        pass_due(part, now);
    }
    link_sources(&mut parts);
    let hours = |word: &str| int_row(place.manifest, &format!("lifecycle.age_h.{word}")).ok();
    let owned = count_owned(&mut parts, &hours, now);
    Output {
        generated_at: beat.now.clone(),
        scope: Scope::Partial,
        marks: Marks { events: Events { len: tail.len, head }, ..prior.marks.clone() },
        owned,
        parts,
        ..prior
    }
}

/// 発話の部品: 出力に在る発話は末尾の仕分けと結びを合わせ、末尾で受けた発話は行 b の関数で導いて足し、窓の外の仕分け済みを外す。
fn utterances(parts: &mut Vec<Part>, beat: &Beat<'_>) {
    for part in parts.iter_mut().filter(|part| part.part == Kind::Utterance) {
        merge_standing(part, beat.events);
    }
    let run_beads = BTreeMap::new();
    let fresh = run_phase::phases(&run_phase::Input {
        queue: &[],
        open: &[],
        latest: &[],
        run_beads: &run_beads,
        refusals: &[],
        events: beat.events,
        line: beat.line.as_deref(),
        now: &beat.now,
        window_s: beat.window_s,
    });
    for part in fresh {
        match parts.iter_mut().find(|found| found.part == part.part && found.id == part.id) {
            Some(old) => *old = part,
            None => parts.push(part),
        }
    }
    let now = epoch_of(&beat.now);
    parts.retain(|part| !(part.phase == Phase::UtteranceSorted && aged(part.since.as_deref(), now, beat.window_s)));
}

/// `since` が窓より古いか（時刻を読めない周は載せる側へ倒す）。
fn aged(since: Option<&str>, now: Option<u64>, window_s: u64) -> bool {
    match (since.and_then(epoch_of), now) {
        (Some(at), Some(now)) => now.saturating_sub(at) > window_s,
        _ => false,
    }
}

/// 発話 `id` を指す仕分けと結びの event のうち最も早い ts の秒（時刻を読めない event は数えない）。
fn earliest_pointing(events: &[Event], id: &str) -> Option<u64> {
    let points = |event: &&Event| matches!(&event.case, Some(Case::Sorted { utterance, .. } | Case::Ruling { utterance, .. }) if utterance == id);
    events.iter().filter(points).filter_map(|event| secs_of(&event.ts)).min()
}

/// 出力に在る発話の部品へ、末尾の仕分けと結びを合わせる（行き先は前と末尾の memo と裁定の和・和が空でなければ chat を落とす・
/// since は前が仕分け済みなら前と末尾の早い方・開きだったなら末尾の最も早い ts）。末尾に指す event が無い部品は動かさない。
fn merge_standing(part: &mut Part, events: &[Event]) {
    let standing = sorted_of(&part.id, events);
    if standing == Standing::Unsorted {
        return;
    }
    let (mut memos, mut rulings, mut chat) = (BTreeSet::new(), BTreeSet::new(), false);
    for found in &part.links.destination {
        match (found.to, &found.id) {
            (Sink::ToMemo, Some(id)) => {
                memos.insert(id.clone());
            }
            (Sink::ToRuling, Some(id)) => {
                rulings.insert(id.clone());
            }
            (Sink::ToChat, _) => chat = true,
            _ => {}
        }
    }
    match standing {
        Standing::Linked { memos: more, rulings: others } => {
            memos.extend(more);
            rulings.extend(others);
        }
        Standing::ChatOnly | Standing::Unsorted => chat = true,
    }
    let to = |sink: Sink, id: String| Destination { to: sink, id: Some(id) };
    let mut destination: Vec<Destination> = memos.into_iter().map(|id| to(Sink::ToMemo, id)).collect();
    destination.extend(rulings.into_iter().map(|id| to(Sink::ToRuling, id)));
    if destination.is_empty() && chat {
        destination.push(Destination { to: Sink::ToChat, id: None });
    }
    let before = (part.phase == Phase::UtteranceSorted).then(|| part.since.as_deref().and_then(epoch_of)).flatten();
    let merged = [earliest_pointing(events, &part.id), before].into_iter().flatten().min().map(format_utc);
    part.since = merged.or_else(|| part.since.clone());
    part.links.destination = destination;
    part.phase = Phase::UtteranceSorted;
    part.turn = turn_of(Phase::UtteranceSorted.as_str(), None).unwrap_or(part.turn);
}

/// 便の部品: 末尾に段の行を持つ便だけを、その便の末尾の行の全部で組み立てて行 b の関数に通す（札の生死は運転手の札）。
fn runs(parts: &mut Vec<Part>, state_dir: &Path, tail: &[Event]) {
    let staged: BTreeSet<&str> = tail.iter().filter(|event| event.stage.is_some() && !event.run.is_empty()).map(|event| event.run.as_str()).collect();
    let rows: Vec<Event> = tail.iter().filter(|event| staged.contains(event.run.as_str())).cloned().collect();
    let created: BTreeSet<&str> = rows.iter().filter(|event| event.kind.as_str() == "RunCreated").map(|event| event.run.as_str()).collect();
    for latest in latest_runs(state_dir, &rows, &replay(&rows)) {
        seat_run(parts, &latest, created.contains(latest.run.as_str()));
    }
}

/// 便 1 本の部品を出力へ置く（閉じていない契約の部品に属する便だけ・同じ契約の最新の便の部品を置き換える・末尾で起きていない
/// 別の便は今の最新の便を置き換えない）。
fn seat_run(parts: &mut Vec<Part>, latest: &Latest, created: bool) {
    let Some(contract) = parts.iter().position(|part| part.part == Kind::Contract && part.id == latest.bead && !part.closed) else { return };
    let mut fresh = run_part(latest);
    let owns = |part: &Part| part.part == Kind::Run && matches!(&part.extra, Extra::Run { bead } if *bead == latest.bead);
    match parts.iter().position(owns) {
        None => parts.insert(contract + 1, fresh),
        Some(at) => {
            let Some(old) = parts.get_mut(at) else { return };
            if old.id == latest.run {
                fresh.links = old.links.clone();
            } else if !created {
                return;
            }
            *old = fresh;
        }
    }
}

/// 期日: 前の全部の書き直しが残した `due` が今以前の memo-waiting で `keep` の無いものを、行 a の `met` で満ちと判じて
/// memo-actionable（理由 `trigger-met`・since は期日）へ移し、`due` を次の満ちていない期日か null にする。ほかは動かさない。
fn pass_due(part: &mut Part, now: u64) {
    if part.part != Kind::Memo || part.phase != Phase::MemoWaiting {
        return;
    }
    let Extra::Memo { due, triggers, keep } = &mut part.extra else { return };
    let Some(since) = due.clone().filter(|found| epoch_of(found).is_some_and(|at| at <= now) && *keep != Some(true)) else { return };
    let world = World { recurrences: 0, write_set: &[], closed: &[], closed_pointers: &[], now };
    let mut next: Option<u64> = None;
    for view in triggers.iter_mut().flatten().filter(|view| view.form == TriggerKind::Deadline.as_str() && !view.met) {
        let Some(at) = epoch_of(&view.value) else { continue };
        view.met = trigger::met(&Trigger::Deadline(at), &world);
        if !view.met {
            next = Some(next.map_or(at, |best| best.min(at)));
        }
    }
    *due = next.map(format_utc);
    part.phase = Phase::MemoActionable;
    part.turn = turn_of(Phase::MemoActionable.as_str(), None).unwrap_or(part.turn);
    part.reason = Some(REASON_TRIGGER_MET.to_owned());
    part.since = Some(since);
}

/// 発話の行き先の memo の各々の部品の `links.source` に発話の ts を足す（発端の正本は仕分けの event・FR88）。
fn link_sources(parts: &mut [Part]) {
    let sources: Vec<(String, String)> = parts
        .iter()
        .filter(|part| part.part == Kind::Utterance)
        .flat_map(|part| part.links.destination.iter().filter(|dest| dest.to == Sink::ToMemo).filter_map(|dest| dest.id.clone()).map(|memo| (memo, part.id.clone())))
        .collect();
    for (memo, utterance) in sources {
        if let Some(part) = parts.iter_mut().find(|part| part.part == Kind::Memo && part.id == memo) {
            if !part.links.source.contains(&utterance) {
                part.links.source.push(utterance);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::{Links, TriggerView};
    use crate::fleet::lifecycle::{render_output, Owned};
    use crate::fleet::lifecycle_mark::{read_stale, Ledger, Stale, Value};
    use crate::pipe::driver_path;
    use std::path::PathBuf;

    const HEAD_TS: &str = "2026-09-01T00:00:00Z";
    const NOW: &str = "2026-10-01T12:00:00Z";
    const POLICY: LockPolicy = LockPolicy { retry_ms: 50, stale_ms: 600_000 };
    const MAIN: &str = "abcdef0123456789abcdef0123456789abcdef01";
    const POINTER: &str = "design = docs/design/x.md#a";
    const C1: &str = "s2-c.1";

    fn now() -> u64 {
        epoch_of(NOW).unwrap_or(0)
    }

    /// 今から `hours` 時間前の時刻の字。
    fn ago(hours: u64) -> String {
        format_utc(now() - hours * 3_600)
    }

    /// event 1 行の字（schema 1・host h・actor machine）。
    fn row(ts: &str, kind: &str, rest: &str) -> String {
        format!("{{\"schema\":1,\"ts\":\"{ts}\",\"kind\":\"{kind}\",\"host\":\"h\",\"actor\":\"machine\"{rest}}}")
    }

    fn head_row() -> String {
        row(HEAD_TS, "RunStage", ",\"run\":\"r0\",\"bead\":\"b0\",\"stage\":\"Intake\"")
    }

    fn cutover(ts: &str) -> String {
        row(ts, "LifecycleCutover", &format!(",\"version\":\"0.1.0\",\"main\":\"{MAIN}\""))
    }

    fn said(ts: &str) -> String {
        row(ts, "UtteranceReceived", ",\"channel\":\"gui\",\"detail\":\"SECRET-VERBATIM\"")
    }

    fn sorted(ts: &str, utterance: &str, memo: Option<&str>) -> String {
        match memo {
            Some(id) => row(ts, "UtteranceSorted", &format!(",\"utterance\":\"{utterance}\",\"sorting\":\"request\",\"bead\":\"{id}\"")),
            None => row(ts, "UtteranceSorted", &format!(",\"utterance\":\"{utterance}\",\"sorting\":\"chat\"")),
        }
    }

    fn bound(ts: &str, utterance: &str) -> String {
        let rest = format!(",\"bead\":\"s2-q1\",\"ruling\":\"s2-q1:r1\",\"utterance\":\"{utterance}\",\"channel\":\"gui\",\"question_ts\":\"2026-09-30T06:00:00Z\",\"asked\":\"seat\",\"detail\":\"SECRET-VERBATIM\"");
        row(ts, "RulingReceived", &rest)
    }

    fn stage(ts: &str, kind: &str, [run, bead, at]: [&str; 3], detail: Option<&str>) -> String {
        let detail = detail.map_or_else(String::new, |found| format!(",\"detail\":\"{found}\""));
        row(ts, kind, &format!(",\"run\":\"{run}\",\"bead\":\"{bead}\",\"stage\":\"{at}\"{detail}"))
    }

    /// 便の段の行の列（時刻は 2026-09-30 の `from` 時から 1 時間おき・Intake の行だけ RunCreated）。
    fn rows(run: &str, bead: &str, stages: &[&str], from: usize) -> Vec<String> {
        let kind = |at: &str| if at == "Intake" { "RunCreated" } else { "RunStage" };
        stages.iter().enumerate().map(|(at, name)| stage(&format!("2026-09-30T{:02}:00:00Z", from + at), kind(name), [run, bead, name], None)).collect()
    }

    /// 1 行目・前の行・読まれない詰め物（`filler` byte 以上）と、末尾の行。
    struct Log {
        bytes: Vec<u8>,
        len: u64,
    }

    fn log(pre: &[String], filler: usize, tail: &[String]) -> Log {
        let mut text: String = std::iter::once(head_row()).chain(pre.iter().cloned()).map(|line| format!("{line}\n")).collect();
        let pad = format!("{}\n", "x".repeat(1_023));
        while text.len() < filler {
            text.push_str(&pad);
        }
        let len = text.len() as u64;
        text.extend(tail.iter().map(|line| format!("{line}\n")));
        Log { bytes: text.into_bytes(), len }
    }

    /// 読んだ byte を数える包み。
    struct Counting<R> {
        inner: R,
        read: u64,
    }

    impl<R: Read> Read for Counting<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let found = self.inner.read(buf)?;
            self.read += found as u64;
            Ok(found)
        }
    }

    impl<R: Seek> Seek for Counting<R> {
        fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    struct Bed {
        state: PathBuf,
        manifest: Manifest,
    }

    impl Bed {
        fn new(name: &str) -> Self {
            let state = crate::pipe::fixture::scratch(&format!("lifecycle-partial-{name}"));
            Self { state, manifest: Manifest::embedded().unwrap_or_else(|_| panic!("埋め込みの manifest を読める")) }
        }

        fn place_with(&self, policy: LockPolicy) -> Place<'_> {
            Place { state_dir: &self.state, repo: &self.state, manifest: &self.manifest, bd: "", policy }
        }

        fn put(&self, out: &Output) {
            let dir = fleet_dir(&self.state);
            assert!(std::fs::create_dir_all(&dir).is_ok() && std::fs::write(dir.join(JSON_FILE), render_output(out)).is_ok(), "出力を置ける");
        }

        fn json(&self) -> String {
            std::fs::read_to_string(fleet_dir(&self.state).join(JSON_FILE)).unwrap_or_default()
        }

        fn out(&self) -> Output {
            match read_output(&self.state) {
                Reading::Read(found) => *found,
                other => panic!("出力を読める: {other:?}"),
            }
        }

        /// 読む byte を数える包みで部分の書き直しを 1 回撃つ。
        fn go(&self, found: &Log) -> (Rewrote, u64) {
            let mut reader = Counting { inner: Cursor::new(found.bytes.as_slice()), read: 0 };
            let back = rewrite_with(&self.place_with(POLICY), &mut reader, now());
            (back, reader.read)
        }

        /// 出力 `parts` を置き、周を 1 回撃って書き直された出力を返す。
        fn after(&self, found: &Log, parts: Vec<Part>) -> Output {
            self.put(&output(found, parts));
            assert_eq!(self.go(found).0, Rewrote::Wrote(Wrote::Written));
            self.out()
        }

        fn stale(&self) -> Vec<(MarkKind, Value)> {
            match read_stale(&self.state) {
                Stale::Marks(found) => found.into_iter().map(|one| (one.kind, one.value)).collect(),
                _ => Vec::new(),
            }
        }

        /// 運転手の札を置く。
        fn card(&self, run: &str, body: &str) {
            let path = driver_path(&self.state, run);
            assert!(path.parent().is_some_and(|dir| std::fs::create_dir_all(dir).is_ok()) && std::fs::write(&path, body).is_ok(), "札を置ける");
        }

        /// 便の行から行 b の関数で組んだ便の部品（前の全部の書き直しが書いた部品）。
        fn run_of(&self, lines: &[String]) -> Part {
            let events = events_of(lines.join("\n").as_bytes());
            let latest = latest_runs(&self.state, &events, &replay(&events));
            run_part(latest.first().unwrap_or_else(|| panic!("便が 1 本")))
        }
    }

    /// 前の全部の書き直しが書いた出力（印は log の今の長さと head・ledger と main は印のまま）。
    fn output(found: &Log, parts: Vec<Part>) -> Output {
        Output {
            generated_at: "2026-10-01T11:00:00Z".to_owned(),
            scope: Scope::Full,
            full_at: "2026-10-01T11:00:00Z".to_owned(),
            interval_s: Some(600),
            closed_window_h: Some(72),
            marks: Marks { ledger: Ledger::Files { len: 7, mtime_ns: 9 }, events: Events { len: found.len, head: Some(HEAD_TS.to_owned()) }, main: MAIN.to_owned() },
            unmeasured: vec![("requirement".to_owned(), "srs-unreadable".to_owned())],
            owned: Owned::default(),
            parts,
        }
    }

    fn part(kind: Kind, id: &str, phase: Phase, [reason, since]: [Option<&str>; 2]) -> Part {
        let turn = turn_of(phase.as_str(), reason).unwrap_or_else(|| panic!("表に在る語"));
        Part { part: kind, id: id.to_owned(), phase, turn, since: since.map(str::to_owned), reason: reason.map(str::to_owned), closed: false, overdue: None, links: Links::default(), extra: Extra::None }
    }

    fn contract(bead: &str) -> Part {
        let mut found = part(Kind::Contract, bead, Phase::ContractRunning, [Some("run-implementing"), None]);
        found.extra = Extra::Contract { pointer: Some(POINTER.to_owned()) };
        found
    }

    fn memo(id: &str, phase: Phase, (due, triggers, keep): (Option<&str>, Vec<(&str, bool)>, bool)) -> Part {
        let views = triggers.into_iter().map(|(value, met)| TriggerView { form: "期日".to_owned(), value: value.to_owned(), met }).collect();
        let mut found = part(Kind::Memo, id, phase, [None, None]);
        found.extra = Extra::Memo { due: due.map(str::to_owned), triggers: Some(views), keep: Some(keep) };
        found
    }

    fn events_of(bytes: &[u8]) -> Vec<Event> {
        String::from_utf8_lossy(bytes).lines().map(|line| Event::from_line(line).unwrap_or_else(|err| panic!("{err}: {line}"))).collect()
    }

    /// log の全部を行 b の発話の関数に渡した結果（発話だけ・線は log の切り替えの線）。
    fn whole(events: &[Event]) -> Vec<Part> {
        let line = read_lines(events).cutover.map(|found| found.ts);
        let run_beads = BTreeMap::new();
        let input = run_phase::Input { queue: &[], open: &[], latest: &[], run_beads: &run_beads, refusals: &[], events, line: line.as_deref(), now: NOW, window_s: 259_200 };
        run_phase::phases(&input).into_iter().filter(|found| found.part == Kind::Utterance).collect()
    }

    /// 前の log の発話の部品を出力に置き、末尾で部分に書き直した発話の部品と、log の全部の結果と、出力の字を返す。
    fn utterances_both(name: &str, pre: &[String], tail: &[String]) -> (Vec<Part>, Vec<Part>, String) {
        let (bed, found) = (Bed::new(name), log(pre, 0, tail));
        let before = whole(&events_of(found.bytes.get(..found.len as usize).unwrap_or_default()));
        let out = bed.after(&found, before);
        let utterances = out.parts.into_iter().filter(|one| one.part == Kind::Utterance).map(|one| Part { overdue: None, ..one }).collect();
        (utterances, whole(&events_of(&found.bytes)), bed.json())
    }

    fn destinations(found: &Part) -> Vec<(&'static str, Option<&str>)> {
        found.links.destination.iter().map(|one| (one.to.as_str(), one.id.as_deref())).collect()
    }

    #[test]
    fn lifecycle_partial_absent_place_does_nothing_and_creates_nothing() {
        let bed = Bed::new("absent");
        assert_eq!(bed.go(&log(&[], 0, &[])).0, Rewrote::Absent);
        assert_eq!(rewrite(&bed.state), Rewrote::Absent);
        assert!(!fleet_dir(&bed.state).exists(), "fleet の dir も lock も作らない");
    }

    #[test]
    fn lifecycle_partial_lock_is_busy_for_a_live_owner_after_200ms_and_reclaimed_from_a_dead_one() {
        let bed = Bed::new("lock");
        let found = log(&[], 0, &[]);
        bed.put(&output(&found, vec![contract(C1)]));
        let (before, lock) = (bed.json(), fleet_dir(&bed.state).join(JSON_LOCK));
        assert!(std::fs::write(&lock, format!("{}\n", std::process::id())).is_ok());
        std::thread::sleep(std::time::Duration::from_millis(5));
        let (start, mut reader) = (std::time::Instant::now(), Cursor::new(found.bytes.as_slice()));
        let policy = LockPolicy { retry_ms: 1, stale_ms: 1 };
        assert_eq!(rewrite_with(&bed.place_with(policy), &mut reader, now()), Rewrote::Wrote(Wrote::Busy), "生きた所有者は古くても外さない");
        assert!(start.elapsed() >= std::time::Duration::from_millis(190), "200 ms まで待つ: {:?}", start.elapsed());
        assert_eq!(bed.json(), before, "書かない");
        assert!(std::fs::write(&lock, "4194300 1\n").is_ok());
        assert_eq!(bed.go(&found).0, Rewrote::Wrote(Wrote::Written), "死んだ所有者の lock は外して書き直す");
        assert!(!lock.exists(), "取った lock は外される");
    }

    /// 開いた部品 200 本と窓の中の閉じた部品 700 本の出力。
    fn bulk(found: &Log) -> Output {
        let open = (0..200).map(|at| part(Kind::Contract, &format!("s2-o.{at}"), Phase::ContractQueued, [Some("dependency"), None]));
        let closed = (0..700).map(|at| Part { closed: true, ..part(Kind::Commit, &format!("{at:040x}"), Phase::CommitLanded, [None, Some("2026-10-01T10:00:00Z")]) });
        output(found, open.chain(closed).collect())
    }

    /// 書いた中身から generated_at と events の len を除く。
    fn content(out: Output) -> Output {
        Output { generated_at: String::new(), marks: Marks { events: Events { len: 0, head: out.marks.events.head.clone() }, ..out.marks.clone() }, ..out }
    }

    /// 同じ 1 行目・同じ末尾で 10 MB と 20 MB の log: 読む byte・書いた中身（generated_at と events の len を除く）・Unchanged が一致する。
    #[test]
    fn lifecycle_partial_reads_the_same_bytes_and_writes_the_same_content_at_any_log_size() {
        let tail = [said("2026-09-30T00:00:01.123Z"), cutover("2026-09-30T00:00:02Z"), said("2026-09-30T00:00:03.456Z")];
        let mut seen = Vec::new();
        for (name, size) in [("ten", 10 << 20), ("twenty", 20 << 20)] {
            let (bed, found) = (Bed::new(name), log(&[], size, &tail));
            bed.put(&bulk(&found));
            let (first, bytes) = bed.go(&found);
            let out = content(bed.out());
            seen.push((first, bytes, bed.go(&found), out));
        }
        assert_eq!(seen.first(), seen.get(1), "10 MB と 20 MB で同じ");
        let (first, bytes, again, out) = seen.first().cloned().unwrap_or_else(|| panic!("2 周"));
        assert_eq!((first, again.0), (Rewrote::Wrote(Wrote::Written), Rewrote::Wrote(Wrote::Unchanged)));
        assert!(bytes < 64 * 1024, "読む byte は log の大きさに依らない: {bytes}");
        assert_eq!(out.parts.len(), 901, "部品は 900 本と線より後の発話 1 本");
    }

    /// 発話の 3 形（未仕分け・会話だけ・要望 2 つと裁定 1 つ）は log の全部の結果と一致し、逐語は出力に無い。
    #[test]
    fn lifecycle_partial_utterance_three_forms_match_the_whole_log_and_carry_no_verbatim() {
        let (u1, u2, u3) = ("2026-09-30T00:00:01.123Z", "2026-09-30T00:00:03.456Z", "2026-09-30T00:00:05.789Z");
        let tail = [said(u1), said(u2), sorted("2026-09-30T00:00:04Z", u2, None), said(u3), sorted("2026-09-30T00:00:06Z", u3, Some("s2-m.1")), sorted("2026-09-30T00:00:07Z", u3, Some("s2-m.2")), bound("2026-09-30T00:00:08Z", u3)];
        let (partial, full, json) = utterances_both("three", &[cutover("2026-09-30T00:00:00Z")], &tail);
        assert_eq!(partial, full, "log の全部の結果と同じ部品");
        let phases: Vec<&str> = partial.iter().map(|found| found.phase.as_str()).collect();
        assert_eq!(phases, ["utterance-open", "utterance-sorted", "utterance-sorted"]);
        assert_eq!(destinations(&partial[1]), [("chat", None)]);
        assert_eq!(destinations(&partial[2]), [("memo", Some("s2-m.1")), ("memo", Some("s2-m.2")), ("ruling", Some("s2-q1:r1"))]);
        assert!(!json.contains("SECRET-VERBATIM"), "逐語は出力に無い");
    }

    /// 末尾より前に受けた発話に末尾で要望が足された周（開き・会話の後）も、log の全部の結果と同じ局面・since・行き先になる。
    #[test]
    fn lifecycle_partial_utterance_sorted_in_the_tail_merges_with_the_earlier_part_like_the_whole_log() {
        let (u1, u2) = ("2026-09-30T00:00:01.123Z", "2026-09-30T00:00:03.456Z");
        let pre = [cutover("2026-09-30T00:00:00Z"), said(u1), said(u2), sorted("2026-09-30T00:00:04Z", u2, None)];
        let tail = [sorted("2026-09-30T00:00:05Z", u1, Some("s2-m.1")), sorted("2026-09-30T00:00:06Z", u2, Some("s2-m.2"))];
        let (partial, full, _) = utterances_both("merge", &pre, &tail);
        assert_eq!(partial, full, "log の全部の結果と同じ部品");
        assert_eq!(destinations(&partial[1]), [("memo", Some("s2-m.2"))], "会話の後に要望が足された発話は memo だけで chat を持たない");
        assert_eq!((partial[0].since.as_deref(), partial[1].since.as_deref()), (Some("2026-09-30T00:00:05Z"), Some("2026-09-30T00:00:04Z")), "開きは末尾の仕分けの ts・仕分け済みは早い方");
    }

    /// 切り替えの線が末尾に在る周は線より前の発話が載らず、線が末尾より前の周は末尾の発話が載る。窓の外へ出る合わせ方は載らない。
    #[test]
    fn lifecycle_partial_utterance_line_in_the_tail_or_before_it_and_the_window_match_the_whole_log() {
        let (u1, u2) = ("2026-09-30T00:00:01.123Z", "2026-09-30T00:00:03.456Z");
        let tail = [said(u1), cutover("2026-09-30T00:00:02Z"), said(u2)];
        let (partial, full, _) = utterances_both("line-tail", &[], &tail);
        assert_eq!((partial.iter().map(|found| found.id.as_str()).collect::<Vec<_>>(), partial.clone()), (vec![u2], full), "末尾の線より前の発話は載らない");
        let (partial, full, _) = utterances_both("line-before", &[cutover("2026-09-30T00:00:00Z")], &[said(u1)]);
        assert_eq!((partial.len(), partial.clone()), (1, full), "線が末尾より前なら末尾の発話が載る");
        let old = "2026-09-20T00:00:01.500Z";
        let pre = [cutover("2026-09-10T00:00:00Z"), said(old), sorted("2026-09-20T00:00:05Z", old, None)];
        let (partial, full, _) = utterances_both("window", &pre, &[sorted("2026-10-01T11:00:00Z", old, Some("s2-m.9"))]);
        assert_eq!((partial, full), (Vec::new(), Vec::new()), "窓の外の仕分けは載らず、出力に無い発話を指す仕分けは読まない");
    }

    /// request の仕分けで、出力に在る memo の `links.source` に発話の ts が足される。
    #[test]
    fn lifecycle_partial_request_sorting_adds_the_utterance_to_the_memo_source() {
        let u = "2026-09-30T00:00:01.123Z";
        let tail = [said(u), sorted("2026-09-30T00:00:02Z", u, Some("s2-m.1"))];
        let bed = Bed::new("source");
        let out = bed.after(&log(&[cutover("2026-09-30T00:00:00Z")], 0, &tail), vec![memo("s2-m.1", Phase::MemoWaiting, (None, Vec::new(), false))]);
        let found = out.parts.iter().find(|one| one.part == Kind::Memo);
        assert_eq!(found.map(|one| one.links.source.clone()), Some(vec![u.to_owned()]));
    }

    fn run_in<'o>(out: &'o Output, id: &str) -> Option<&'o Part> {
        out.parts.iter().find(|one| one.part == Kind::Run && one.id == id)
    }

    /// 札の生きた Landed の便は ci-waiting・死んだ札の同じ便は landed-open（対）。契約の部品は動かない。
    #[test]
    fn lifecycle_partial_landed_run_is_ci_waiting_with_a_live_card_and_landed_open_with_a_dead_one() {
        for (body, want) in [(format!("{}\n", std::process::id()), "run-ci-waiting"), ("4194300 1\n".to_owned(), "run-landed-open")] {
            let bed = Bed::new(want);
            let (pre, tail) = (rows("r1", C1, &["Intake", "Spawned"], 1), rows("r1", C1, &["Landed"], 5));
            bed.card("r1", &body);
            let out = bed.after(&log(&pre, 0, &tail), vec![contract(C1), bed.run_of(&pre)]);
            let found = run_in(&out, "r1").unwrap_or_else(|| panic!("便の部品"));
            assert_eq!((found.phase.as_str(), found.since.as_deref()), (want, Some("2026-09-30T05:00:00Z")));
            assert_eq!(out.parts.iter().find(|one| one.part == Kind::Contract), Some(&contract(C1)), "契約の部品は動かさない");
        }
    }

    /// 審査と門の判定の語で便の局面が分かれる（Gated の PASS と PASS でない）。
    #[test]
    fn lifecycle_partial_gated_run_follows_the_verdict_word() {
        for (detail, want, reason) in [("verdict:PASS", "run-landing", "Gated"), ("verdict:FAIL", "run-gate-failed", "FAIL")] {
            let bed = Bed::new(want);
            let pre = rows("r1", C1, &["Intake", "Implemented"], 1);
            let tail = [stage("2026-09-30T05:00:00Z", "RunStage", ["r1", C1, "Gated"], Some(detail))];
            let out = bed.after(&log(&pre, 0, &tail), vec![contract(C1), bed.run_of(&pre)]);
            let found = run_in(&out, "r1").unwrap_or_else(|| panic!("便の部品"));
            assert_eq!((found.phase.as_str(), found.reason.as_deref()), (want, Some(reason)));
        }
    }

    /// 末尾で新しく起きた便は同じ契約の前の便の部品を置き換える。末尾で起きていない古い便は今の最新の便を置き換えない（対）。
    #[test]
    fn lifecycle_partial_a_new_run_replaces_the_previous_run_of_the_same_contract() {
        let bed = Bed::new("replace");
        let pre = rows("r1", C1, &["Intake", "Spawned"], 1);
        let tail = rows("r2", C1, &["Intake", "Spawned"], 5);
        let out = bed.after(&log(&pre, 0, &tail), vec![contract(C1), bed.run_of(&pre)]);
        assert!(run_in(&out, "r1").is_none() && run_in(&out, "r2").is_some_and(|found| found.phase == Phase::RunImplementing), "r2 が r1 を置き換える");
        assert_eq!(out.parts.iter().filter(|one| one.part == Kind::Run).count(), 1);
        let bed = Bed::new("stale-run");
        let old = [stage("2026-09-30T06:00:00Z", "RunStage", ["r0", C1, "Failed"], None)];
        let out = bed.after(&log(&pre, 0, &old), vec![contract(C1), bed.run_of(&pre)]);
        assert!(run_in(&out, "r1").is_some_and(|found| found.phase == Phase::RunImplementing) && run_in(&out, "r0").is_none(), "古い便は今の最新を置き換えない");
    }

    /// 閉じた契約の便と出力に無い契約の便は載らず、開いた契約の便は契約の部品の後ろに載る（対）。段の行を持たない便は動かさない。
    #[test]
    fn lifecycle_partial_runs_of_closed_or_unknown_contracts_are_not_added() {
        let bed = Bed::new("closed");
        let tail: Vec<String> = [("r1", C1), ("r9", "s2-d.1"), ("r8", "s2-u.1")].iter().flat_map(|(run, bead)| rows(run, bead, &["Intake", "Spawned"], 5)).collect();
        let closed = Part { closed: true, ..contract("s2-d.1") };
        let out = bed.after(&log(&[], 0, &tail), vec![contract(C1), closed]);
        let ids: Vec<&str> = out.parts.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(ids, [C1, "r1", "s2-d.1"], "開いた契約の便だけが契約の部品の後ろに載る");
        let (bed, quiet) = (Bed::new("no-stage"), row("2026-09-30T05:00:00Z", "ApprovalRequested", &format!(",\"run\":\"r1\",\"bead\":\"{C1}\"")));
        let pre = rows("r1", C1, &["Intake", "Spawned"], 1);
        let out = bed.after(&log(&pre, 0, &[quiet]), vec![contract(C1), bed.run_of(&pre)]);
        assert_eq!(run_in(&out, "r1").map(|found| found.phase), Some(Phase::RunImplementing), "段の行を持たない便は動かさない");
    }

    /// 期日の移り: keep の無い memo-waiting は memo-actionable（trigger-met・since は期日・due は次の期日か null）へ移り、keep の在る
    /// memo・memo-asking・期日がまだの memo は動かない（対）。
    #[test]
    fn lifecycle_partial_due_memo_turns_actionable_while_keep_asking_and_future_stay() {
        let (past, soon) = ("2026-09-28T00:00:00Z", "2026-10-05T00:00:00Z");
        let waiting = |id: &str, keep: bool| memo(id, Phase::MemoWaiting, (Some(past), vec![(past, false), (soon, false)], keep));
        let parts = vec![
            waiting("s2-m.1", false),
            waiting("s2-m.2", true),
            memo("s2-m.3", Phase::MemoAsking, (Some(past), vec![(past, false)], false)),
            memo("s2-m.4", Phase::MemoWaiting, (Some(soon), vec![(soon, false)], false)),
            memo("s2-m.5", Phase::MemoWaiting, (Some(past), vec![(past, false)], false)),
        ];
        let out = Bed::new("due").after(&log(&[], 0, &[]), parts.clone());
        let found = |id: &str| out.parts.iter().find(|one| one.id == id).unwrap_or_else(|| panic!("{id}"));
        let moved = found("s2-m.1");
        assert_eq!((moved.phase, moved.reason.as_deref(), moved.since.as_deref(), moved.turn.as_str()), (Phase::MemoActionable, Some("trigger-met"), Some(past), "seat"));
        let Extra::Memo { due, triggers, .. } = &moved.extra else { panic!("memo の欄") };
        assert_eq!((due.as_deref(), triggers.as_ref().map(|views| views.iter().map(|view| view.met).collect::<Vec<_>>())), (Some(soon), Some(vec![true, false])), "due は次の満ちていない期日・期日の写しは満ちに");
        let Extra::Memo { due, .. } = &found("s2-m.5").extra else { panic!("memo の欄") };
        assert_eq!((found("s2-m.5").phase, due.clone()), (Phase::MemoActionable, None), "次の期日が無ければ null");
        for id in ["s2-m.2", "s2-m.3", "s2-m.4"] {
            assert_eq!(found(id), parts.iter().find(|one| one.id == id).unwrap_or_else(|| panic!("{id}")), "{id} は動かない");
        }
        assert_eq!(out.owned.count, 2, "移った memo は 24 時間の閾値を越える");
        assert_eq!(out.owned.oldest.as_ref().map(|one| one.id.as_str()), Some("s2-m.1"));
    }

    /// 年齢の閾値を越えた部品が overdue true になり owned が数え直される（行の無い seat の語は unset・requirement は数えない）。
    #[test]
    fn lifecycle_partial_age_marks_overdue_parts_and_recounts_owned() {
        let seat = |kind: Kind, id: &str, phase: Phase, hours: u64| part(kind, id, phase, [None, Some(&ago(hours))]);
        let refused = |id: &str, hours: u64| Part { phase: Phase::ContractRefused, turn: turn_of("contract-refused", None).unwrap_or(contract(id).turn), since: Some(ago(hours)), ..contract(id) };
        let mut asking = part(Kind::Run, "r1", Phase::RunAsking, [None, Some(&ago(1))]);
        asking.extra = Extra::Run { bead: C1.to_owned() };
        let parts = vec![refused("s2-c.5", 5), refused("s2-c.6", 1), seat(Kind::Row, "row-a", Phase::RowUnbeaded, 30), seat(Kind::Epic, "s2-e.1", Phase::EpicClosable, 10), asking, seat(Kind::Requirement, "FR1", Phase::RequirementUnrowed, 99)];
        let out = Bed::new("age").after(&log(&[], 0, &[]), parts);
        let overdue: Vec<Option<bool>> = out.parts.iter().map(|one| one.overdue).collect();
        assert_eq!(overdue, [Some(true), Some(false), Some(true), Some(false), None, None]);
        let oldest = out.owned.oldest.as_ref().map(|one| (one.id.as_str(), one.phase));
        assert_eq!((out.owned.count, out.owned.unset, out.owned.unknown, oldest), (2, 1, 0, Some(("row-a", Phase::RowUnbeaded))));
    }

    /// scope は partial・inputs は events だけが進み（ledger と main は前の値）・full_at と契約の部品と測れなかった種類は前のまま。
    #[test]
    fn lifecycle_partial_moves_only_the_events_input_and_leaves_contracts_and_full_at() {
        let bed = Bed::new("scope");
        let found = log(&[], 0, &[stage("2026-09-30T00:00:00Z", "RunStage", ["r7", "s2-u.1", "Spawned"], None)]);
        let before = output(&Log { len: 0, bytes: Vec::new() }, vec![contract(C1), part(Kind::Requirement, "FR1", Phase::RequirementUnrowed, [None, None])]);
        let before = Output { marks: Marks { events: Events { len: found.len, head: Some(HEAD_TS.to_owned()) }, ..before.marks.clone() }, ..before };
        bed.put(&before);
        assert_eq!(bed.go(&found).0, Rewrote::Wrote(Wrote::Written));
        let out = bed.out();
        assert_eq!((out.scope, out.generated_at.as_str(), out.full_at.as_str()), (Scope::Partial, NOW, "2026-10-01T11:00:00Z"));
        assert_eq!((&out.marks.ledger, &out.marks.main, out.interval_s, out.closed_window_h, &out.unmeasured), (&before.marks.ledger, &before.marks.main, Some(600), Some(72), &before.unmeasured));
        assert_eq!(out.marks.events, Events { len: found.bytes.len() as u64, head: Some(HEAD_TS.to_owned()) }, "events の印だけが末尾の次まで進む");
        assert_eq!(out.parts, before.parts, "契約と要件の部品は動かさない");
    }

    /// log が繋がらない 3 形（head が違う・短い・直前が改行でない）は書かず、読めない印（理由 events）を付け、既に在る印は消さない。
    #[test]
    fn lifecycle_partial_unjoined_log_writes_nothing_marks_unreadable_and_keeps_the_marks() {
        let found = log(&[], 0, &[]);
        let ledger = Value::Ledger(Ledger::Files { len: 7, mtime_ns: 9 });
        let forms: [fn(&mut Output); 3] = [
            |out| out.marks.events.head = Some("2026-01-01T00:00:00Z".to_owned()),
            |out| out.marks.events.len += 999,
            |out| out.marks.events.len -= 3,
        ];
        for (at, spoil) in forms.into_iter().enumerate() {
            let bed = Bed::new(&format!("unjoined-{at}"));
            let mut out = output(&found, vec![contract(C1)]);
            bed.put(&out);
            let mark = mark::Mark { kind: MarkKind::LedgerGate, at: "2026-10-01T10:00:00Z".to_owned(), value: ledger.clone() };
            assert_eq!(mark::add_mark(&bed.state, &mark, POLICY), mark::Added::Added);
            spoil(&mut out);
            bed.put(&out);
            let before = bed.json();
            assert_eq!(bed.go(&found).0, Rewrote::Wrote(Wrote::Unreadable("events")), "形 {at}");
            assert_eq!(bed.json(), before, "書かない");
            assert_eq!(bed.stale(), [(MarkKind::LedgerGate, ledger.clone()), (MarkKind::Unreadable, Value::Reason("events".to_owned()))], "読めない印を付け、前の印は消さない");
            bed.put(&output(&found, vec![contract(C1)]));
            assert_eq!(bed.go(&found).0, Rewrote::Wrote(Wrote::Written));
            assert_eq!(bed.stale().len(), 2, "部分の書き直しが通っても印は消さない");
        }
    }

    /// 本物の file の log を読む口（tick が撃つ 1 本）: log の無い置き場も、末尾の在る置き場も書き直す。
    #[test]
    fn lifecycle_partial_rewrite_reads_the_real_log_file() {
        let bed = Bed::new("real");
        bed.put(&output(&Log { len: 0, bytes: Vec::new() }, Vec::new()));
        let empty = Output { marks: Marks { events: Events { len: 0, head: None }, ..bed.out().marks }, ..bed.out() };
        bed.put(&empty);
        assert_eq!(rewrite(&bed.state), Rewrote::Wrote(Wrote::Written), "log の無い置き場");
        let found = log(&[], 0, &[said("2999-01-01T00:00:00Z")]);
        assert!(std::fs::create_dir_all(fleet_dir(&bed.state)).is_ok() && std::fs::write(store::events_path(&bed.state), &found.bytes).is_ok());
        bed.put(&Output { marks: Marks { events: Events { len: 0, head: None }, ..empty.marks.clone() }, ..empty });
        assert_eq!(rewrite(&bed.state), Rewrote::Wrote(Wrote::Written), "末尾の在る置き場");
        assert_eq!(bed.out().marks.events, Events { len: found.bytes.len() as u64, head: Some(HEAD_TS.to_owned()) });
    }
}
