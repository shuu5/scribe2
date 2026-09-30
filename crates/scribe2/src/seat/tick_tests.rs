//! `seat::tick` の歯。**本体は `tick.rs`** で、ここには test だけが在る（憲法 C4 の余地を作るための純移動・設計 seat-heartbeat §23）。
// flip-check: moved s2-07l.738.37.8
// flip-check: moved s2-07l.737.4
use super::{
    candidate, pointer_of, render, settle, signal, Ladder, Move, NoopReason, Pace, Pointer, Sent, TickDecision, TickError,
    Verdict, NOOP_REASONS, TICK_ERRORS,
};
use crate::name::NAME;
use crate::order::is_declaration_order;
use crate::seat::inject::{Settled, Unmeasured};
use crate::seat::state::{Event, Stamp, SCHEMA};

/// 初期値の梯子の列（rules 行の値を写さない fixture・値の対応は e2e が manifest で測る）。
const LADDER: [u64; 6] = [1800, 3600, 10_800, 21_600, 43_200, 86_400];

/// 初期値の梯子（黙りの閾値 1800 と列）。
fn pace() -> Pace {
    Pace { stale_s: 1800, ladder: LADDER.to_vec() }
}

/// 打刻 1 行。
fn stamp(event: Event, ts: u64) -> Stamp {
    Stamp { schema: SCHEMA, state: event.state(), event, ts, sid: String::new() }
}

/// 段 → 待ちは列の n 番目で、段 0〜5 が列の内・段 6 から打ち切り（合図の本数は列の長さ）。
#[test]
fn seat_tick_wait_follows_the_ladder_and_stops_past_it() {
    let pace = pace();
    let waits: Vec<Option<u64>> = (0..=6).map(|step| pace.wait_of(step)).collect();
    let want: Vec<Option<u64>> = LADDER.iter().copied().map(Some).chain([None]).collect();
    assert_eq!(waits, want, "列の待ちと段 6 の打ち切り");
    let open = (0..=6).filter(|step| pointer_of(&pace, None, *step, 0) == Pointer::Open).count();
    assert_eq!(open, LADDER.len(), "送る段は列の長さ");
    assert_eq!(pointer_of(&pace, None, 6, 0), Pointer::Stopped, "段 6 は列を越える（段 0〜5 は記録なしで床を通る）");
    assert_eq!(pointer_of(&pace, None, u32::MAX, 0), Pointer::Stopped, "段の飽和も打ち切り");
    let one = Pace { stale_s: 1, ladder: vec![10] };
    assert_eq!((pointer_of(&one, None, 0, 0), pointer_of(&one, None, 1, 0)), (Pointer::Open, Pointer::Stopped), "1 要素の列");
}

/// 列の字面: 数で狭義に昇順の列は読み、数でない・昇順でない（等しい隣を含む）・空の列は断る。
#[test]
fn seat_tick_pace_reads_the_ladder_and_refuses_malformed_lists() {
    let words = ["1800", "3600", "10800", "21600", "43200", "86400"];
    assert_eq!(Pace::of(1800, &words.map(str::to_owned)), Ok(pace()), "初期値の列");
    for bad in [&["1800", "x"][..], &["1800", "-1"], &["1800", " 3600"], &["3600", "1800"], &["1800", "1800"], &[]] {
        let items: Vec<String> = bad.iter().map(|word| (*word).to_owned()).collect();
        assert!(Pace::of(1800, &items).is_err(), "{bad:?} は断る");
    }
}

/// 床: `sent_at` から待ちが経っていない周は残り秒の Wait・ちょうど経った周は Open。
#[test]
fn seat_tick_floor_counts_the_seconds_left_from_sent_at() {
    let pace = pace();
    assert_eq!(pointer_of(&pace, Some(1000), 1, 1000 + 3000), Pointer::Wait(600));
    assert_eq!(pointer_of(&pace, Some(1000), 1, 1000 + 3600), Pointer::Open, "境界は経った側");
    assert_eq!(pointer_of(&pace, Some(1000), 0, 1000 + 1799), Pointer::Wait(1));
    assert_eq!(pointer_of(&pace, Some(5000), 0, 10), Pointer::Wait(1800), "未来の sent_at は経過 0");
}

/// 段の候補: 記録なし → 0・基準と同じ digest → 段 + 1・変化 → 0・基準の無い記録 → 0。
#[test]
fn seat_tick_candidate_climbs_on_same_digest_and_resets_on_change() {
    let record = Ladder { sent_at: 10, step: 3, digest: Some(77) };
    assert_eq!(candidate(None, 77), 0, "記録なし");
    assert_eq!(candidate(Some(&record), 77), 4, "無変化");
    assert_eq!(candidate(Some(&record), 78), 0, "変化");
    assert_eq!(candidate(Some(&Ladder { digest: None, ..record }), 77), 0, "基準なし");
    assert_eq!(candidate(Some(&Ladder { step: u32::MAX, ..record }), 77), u32::MAX, "段は飽和");
}

/// settle: `sent_at` より後の Stop の最後の ts・無ければ stale を過ぎた周の今の digest・どちらでもなければ None。
#[test]
fn seat_tick_settle_takes_the_answer_or_the_stale_digest() {
    let record = Ladder { sent_at: 100, step: 0, digest: None };
    let answered = [stamp(Event::Stop, 90), stamp(Event::UserPromptSubmit, 101), stamp(Event::Stop, 102), stamp(Event::Stop, 105)];
    assert_eq!(settle(&record, &answered, 105, 110, 2400), Some(105), "後の Stop の最後");
    let same_second = [stamp(Event::Stop, 100), stamp(Event::SessionStart, 104)];
    assert_eq!(settle(&record, &same_second, 104, 110, 2400), None, "sent_at と同じ秒の Stop と SessionStart は応答でない");
    assert_eq!(settle(&record, &same_second, 104, 100 + 2400, 2400), Some(104), "応えない席は stale を過ぎた周の digest");
    assert_eq!(settle(&record, &same_second, 104, 100 + 2399, 2400), None, "stale の手前は settling");
}

/// 記録の 1 行は round-trip し、1 行でない・schema 違い・key の欠け・型違い・段の桁あふれは読まない。
#[test]
fn seat_tick_ladder_record_round_trips_and_rejects_malformed_lines() {
    for record in [Ladder { sent_at: 1_757_600_000, step: 5, digest: Some(1_757_599_000) }, Ladder { sent_at: 1, step: 0, digest: None }] {
        let line = record.to_line();
        assert_eq!(Ladder::parse(&line), Some(record), "{line}");
        assert_eq!(Ladder::parse(&format!("{line}\n")), Some(record), "末尾の改行");
    }
    assert_eq!(
        Ladder { sent_at: 7, step: 1, digest: None }.to_line(),
        r#"{"schema":1,"sent_at":7,"step":1,"digest":null}"#,
        "外形"
    );
    for bad in [
        "",
        "{\"schema\":1,\"sent_at\":7,\"step\":1,\"digest\":null}\n{\"schema\":1,\"sent_at\":7,\"step\":1,\"digest\":null}",
        r#"{"schema":2,"sent_at":7,"step":1,"digest":null}"#,
        r#"{"schema":1,"step":1,"digest":null}"#,
        r#"{"schema":1,"sent_at":7,"step":1}"#,
        r#"{"schema":1,"sent_at":"7","step":1,"digest":null}"#,
        r#"{"schema":1,"sent_at":7,"step":4294967296,"digest":null}"#,
        r#"{"schema":1,"sent_at":7,"step":1,"digest":"x"}"#,
    ] {
        assert_eq!(Ladder::parse(bad), None, "{bad}");
    }
}

/// 理由の語は宣言順の列で重複せず、網羅 match の字面と一致する（error の語も同じ）。
#[test]
fn seat_tick_reasons_are_unique_in_declaration_order() {
    assert!(is_declaration_order(NOOP_REASONS, |reason| reason as usize), "NOOP_REASONS は宣言順");
    assert_eq!(NOOP_REASONS.len(), 20, "母集団");
    let words: Vec<&str> = NOOP_REASONS.iter().map(|reason| reason.as_str()).collect();
    assert_eq!(
        words,
        [
            "no-row", "state-missing", "state-unreadable", "busy", "state-stale", "settling", "record-unreadable",
            "stamp-recent", "stopped", "wait", "account-pressed", "pane-missing", "input-busy", "input-unknown",
            "input-own-queued", "record-unwritable", "group-unreadable", "group-locked", "heartbeat-off", "account-dead",
        ]
    );
    assert!(is_declaration_order(TICK_ERRORS, |error| error as usize), "TICK_ERRORS は宣言順");
    let errors: Vec<&str> = TICK_ERRORS.iter().map(|error| error.as_str()).collect();
    assert_eq!(errors, ["state-dir", "no-rule", "store"]);
    let mut all: Vec<&str> = words.iter().chain(errors.iter()).copied().collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 23, "noop と error の語は重ならない");
}

/// 判定行: 梯子の手前は `pointer=- step=-`・梯子の後は評価と段・注入した周だけ `consumed=`（届かない周は unknown:理由）。
/// 移動の周でない行は末尾に `move=- launched=-` を持つ。
#[test]
fn seat_tick_render_dashes_before_the_ladder() {
    assert_eq!(
        render("s:w", &Verdict::noop(NoopReason::NoRow)),
        "decision=noop target=s_w reason=no-row pointer=- step=- consumed=- move=- launched=-"
    );
    assert_eq!(
        render("s:w", &Verdict::error(TickError::NoRule)),
        "decision=error target=s_w reason=no-rule pointer=- step=- consumed=- move=- launched=-"
    );
    assert_eq!(
        render("s:w", &Verdict::noop_at(NoopReason::Wait, Pointer::Wait(30), 2)),
        "decision=noop target=s_w reason=wait pointer=wait:30 step=2 consumed=- move=- launched=-"
    );
    assert_eq!(
        render("s:w", &Verdict::noop_at(NoopReason::StampRecent, Pointer::Open, 0)),
        "decision=noop target=s_w reason=stamp-recent pointer=wait:0 step=0 consumed=- move=- launched=-"
    );
    let sent =
        |found| Verdict { decision: TickDecision::Inject, sent: Some(found), ..Verdict::noop_at(NoopReason::Wait, Pointer::Open, 1) };
    assert_eq!(
        render("s:w", &sent(Sent::Settled(Settled::Queued))),
        "decision=inject target=s_w reason=- pointer=sent step=1 consumed=false move=- launched=-"
    );
    assert!(render("s:w", &sent(Sent::Settled(Settled::Unmeasured(Unmeasured::StateMissing))))
        .ends_with(" consumed=unknown:state-missing move=- launched=-"));
    assert!(render("s:w", &sent(Sent::Unconfirmed("absent"))).ends_with(" consumed=unknown:absent move=- launched=-"));
}

/// 移動の門の 2 値と停止の記録の門の 1 値（設計 §12 形 3）と墓標の門の 1 値（account-lifecycle.md §38 形 9）は `NoopReason` の
/// 宣言順の末尾に在り、既存の 16 値の語と重ならない（母集団は 16 → 18 → 19 → 20）。移動の周の手は 5 値（猶予の合図と待ちが末尾・設計 §13 形 6）。
#[test]
fn seat_tick_tail_reasons_are_the_last_three_in_declaration_order() {
    assert!(is_declaration_order(NOOP_REASONS, |reason| reason as usize), "NOOP_REASONS は宣言順");
    let tail: Vec<&str> = NOOP_REASONS.iter().rev().take(3).map(|reason| reason.as_str()).collect();
    assert_eq!(tail, ["account-dead", "heartbeat-off", "group-locked"], "末尾の 3 値（逆順）");
    let mut words: Vec<&str> = NOOP_REASONS.iter().map(|reason| reason.as_str()).collect();
    let before = words.len();
    words.sort_unstable();
    words.dedup();
    assert_eq!((before, words.len()), (20, 20), "20 値で重複しない");
    let moves = [Move::Launch, Move::Exit, Move::Enter, Move::Signal, Move::Wait];
    let words: Vec<&str> = moves.iter().map(|found| found.as_str()).collect();
    assert_eq!(words, ["launch", "exit", "enter", "signal", "wait"], "手の語 5 値（設計 §13 形 6・signal / wait は末尾）");
}

/// 移動の周の判定行は `decision=move reason=- pointer=- step=-` で、送りの結果を `consumed=`・手を `move=`・起こした周だけ
/// 起動の結果を `launched=` に載せる（他は `-`）。
#[test]
fn seat_tick_move_render_names_the_step_and_the_launch_word() {
    assert_eq!(
        render("s:w", &Verdict::moved(Move::Launch, None, Some("launch-unconfirmed"))),
        "decision=move target=s_w reason=- pointer=- step=- consumed=- move=launch launched=launch-unconfirmed"
    );
    assert_eq!(
        render("s:w", &Verdict { trust: Some("written"), ..Verdict::moved(Move::Launch, None, Some("done")) }),
        "decision=move target=s_w reason=- pointer=- step=- consumed=- move=launch launched=done trust=written",
        "起こせた周は launched= の後ろに trust="
    );
    assert_eq!(
        render("s:w", &Verdict::moved(Move::Exit, Some(Sent::Settled(Settled::Queued)), None)),
        "decision=move target=s_w reason=- pointer=- step=- consumed=false move=exit launched=-"
    );
    assert_eq!(
        render("s:w", &Verdict::moved(Move::Enter, Some(Sent::Unconfirmed("exit-dialog")), None)),
        "decision=move target=s_w reason=- pointer=- step=- consumed=unknown:exit-dialog move=enter launched=-"
    );
    assert_eq!(
        render("s:w", &Verdict::noop(NoopReason::GroupLocked)),
        "decision=noop target=s_w reason=group-locked pointer=- step=- consumed=- move=- launched=-"
    );
}

/// 合図の文面は器の目印で始まり、段と次の段の待ち（列から引く）を名乗り、最後の段は次が無いと書く。
#[test]
fn seat_tick_signal_names_the_step_and_the_next_wait() {
    let pace = pace();
    let line = signal(0, &pace);
    assert!(line.starts_with(&format!("{NAME} tick: heartbeat step=0 — ")), "{line}");
    assert!(line.ends_with("（変化が無ければ次の合図は 3600 秒後）"), "{line}");
    assert!(!line.contains('\n'), "1 行");
    let nexts: Vec<bool> =
        (0u32..).zip(LADDER.iter().skip(1)).map(|(step, next)| signal(step, &pace).contains(&format!("次の合図は {next} 秒後"))).collect();
    assert_eq!(nexts, [true; 5], "段 0〜4 の次は列の次の要素");
    let last = signal(5, &pace);
    assert!(last.ends_with("（変化が無ければ次の合図は無い・打ち切り）"), "最後の段は次が無い: {last}");
}
