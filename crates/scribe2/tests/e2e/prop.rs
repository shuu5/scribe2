//! 性質の歯（憲法 C12.7「property testing で歯を量産」の初着手・s2-07l.98）。
//!
//! 対象は **純関数 4 群**（Command を呼ぶ e2e は対象外）: 席の打刻 [`Stamp`]・fleet の
//! [`EventKind`] / [`Stage`] / [`Event`]・rules manifest の toml-lite・headless runner の
//! `rate_limit_status`。入力は proptest の strategy で生成し、既知の反例（入れ子の status・
//! 空白・非 JSON・空要素）を strategy の空間に含める。
//!
//! 反例の永続化（`proptest-regressions/`）は切る＝落ちた周に tracked tree を汚さない
//! （落ちた入力は nextest の出力に出る）。case 数は既定（256）。

use proptest::prelude::*;
use proptest::test_runner::Config;
use vessel::fleet::{Event as FleetEvent, EventKind, Stage, ACTOR_HUMAN, ACTOR_MACHINE, KINDS, SCHEMA as FLEET_SCHEMA, STAGES};
use vessel::headless::runner::rate_limit_status;
use vessel::rules::manifest::{elements, list, quoted_once, scalar, Scalar};
use vessel::seat::state::{Event, SeatState, Stamp, SCHEMA};

/// 反例の永続化を切り、case 数を既定値 256 に pin した設定（`PROPTEST_CASES` 等の
/// 継承 env で歯の強さが黙って下がらない）。
fn config() -> Config {
    Config {
        cases: 256,
        failure_persistence: None,
        ..Config::default()
    }
}

/// 打刻の 3 event（closed enum の全 variant）。
fn any_event() -> impl Strategy<Value = Event> {
    prop::sample::select(vec![Event::SessionStart, Event::UserPromptSubmit, Event::Stop])
}

/// 1 行 JSON に入れてよい文字列（`"` `\` 改行・制御文字は escape で通る。`(?s)` で `.` に
/// 改行を含める＝JSONL で最も効く反例を空間に入れる）。
fn json_text() -> impl Strategy<Value = String> {
    "(?s).{0,32}"
}

/// 席 id / 便 id / host のような識別子。
fn ident() -> impl Strategy<Value = String> {
    "[A-Za-z0-9._-]{1,16}"
}

/// key と colon と値の間に置く空白。
fn ws() -> impl Strategy<Value = String> {
    "[ \t]{0,3}"
}

/// `rate_limit_status` が返す識別子（escape を含まない）。
fn status_ident() -> impl Strategy<Value = String> {
    "[a-z_]{1,16}"
}

/// `{"type":"<kind>","rate_limit_info":<info>}` を空白入りで組む。
fn rate_limit_line(kind: &str, info: &str, gap: &[String]) -> String {
    let g = |i: usize| gap.get(i).map_or("", String::as_str);
    format!(
        "{{{}\"type\"{}:{}\"{kind}\"{},{}\"rate_limit_info\"{}:{}{info}{}}}",
        g(0),
        g(1),
        g(2),
        g(3),
        g(4),
        g(5),
        g(6),
        g(7)
    )
}

/// 打刻の性質（(1) seat/state.rs）。
mod stamp {
    use super::{any_event, config, json_text, SeatState, Stamp, SCHEMA};
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(config())]

        /// 任意の event × 任意の sid × 任意の ts で、書いた行から同じ打刻が読める。
        #[test]
        fn prop_stamp_round_trips_through_line(event in any_event(), sid in json_text(), ts in any::<u64>()) {
            let stamp = Stamp { schema: SCHEMA, state: event.state(), event, ts, sid };
            prop_assert_eq!(Stamp::from_line(&stamp.to_line()), Ok(stamp));
        }

        /// state が event の意味と食い違う行は、形が正しくても読まない。
        #[test]
        fn prop_stamp_rejects_state_that_contradicts_event(event in any_event(), sid in json_text(), ts in any::<u64>()) {
            let wrong = match event.state() {
                SeatState::Busy => SeatState::Idle,
                SeatState::Idle => SeatState::Busy,
            };
            let stamp = Stamp { schema: SCHEMA, state: wrong, event, ts, sid };
            prop_assert!(Stamp::from_line(&stamp.to_line()).is_err());
        }

        /// 任意の文字列で panic せず、object で始まらない字面は `Err`。
        #[test]
        fn prop_stamp_from_line_never_panics_and_rejects_non_json(line in "(?s).{0,64}") {
            let read = Stamp::from_line(&line);
            if !line.trim_start().starts_with('{') {
                prop_assert!(read.is_err(), "{:?} → {:?}", line, read);
            }
        }
    }
}

/// fleet の閉じた enum と log 行の性質（(2) fleet/mod.rs）。
mod fleet {
    use super::{config, ident, json_text, FleetEvent, EventKind, Stage, ACTOR_HUMAN, ACTOR_MACHINE, FLEET_SCHEMA, KINDS, STAGES};
    use proptest::prelude::*;

    /// 全 variant の字面と、その近傍（小文字化・英字だけの任意文字列）。
    fn kind_like() -> impl Strategy<Value = String> {
        prop_oneof![
            2 => "[A-Za-z]{1,18}",
            1 => prop::sample::select(KINDS).prop_map(|kind| kind.as_str().to_owned()),
            1 => prop::sample::select(KINDS).prop_map(|kind| kind.as_str().to_lowercase()),
        ]
    }

    /// 全 variant の字面と、その近傍。
    fn stage_like() -> impl Strategy<Value = String> {
        prop_oneof![
            2 => "[A-Za-z]{1,12}",
            1 => prop::sample::select(STAGES).prop_map(|stage| stage.as_str().to_owned()),
            1 => prop::sample::select(STAGES).prop_map(|stage| stage.as_str().to_lowercase()),
        ]
    }

    /// log 行 1 本（任意 field は独立に有無を振る）。
    fn any_fleet_event() -> impl Strategy<Value = FleetEvent> {
        (
            "[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z",
            prop::sample::select(KINDS),
            ident(),
            ident(),
            ident(),
            prop::sample::select(vec![ACTOR_MACHINE, ACTOR_HUMAN]),
            prop::option::of(prop::sample::select(STAGES)),
            prop::option::of(ident()),
            prop::option::of(any::<u64>()),
            prop::option::of(json_text()),
        )
            .prop_map(|(ts, kind, run, bead, host, actor, stage, seat, pid, detail)| FleetEvent {
                schema: FLEET_SCHEMA,
                ts,
                kind,
                run,
                bead,
                host,
                actor: actor.to_owned(),
                stage,
                seat,
                pid,
                detail,
            })
    }

    proptest! {
        #![proptest_config(config())]

        /// 全 variant で as_str → parse が戻る。
        #[test]
        fn prop_fleet_kind_and_stage_round_trip(kind in prop::sample::select(KINDS), stage in prop::sample::select(STAGES)) {
            prop_assert_eq!(EventKind::parse(kind.as_str()), Some(kind));
            prop_assert_eq!(Stage::parse(stage.as_str()), Some(stage));
        }

        /// parse が `Some` を返すなら、その variant の字面は入力と一致する（大文字小文字も）。
        #[test]
        fn prop_fleet_parse_accepts_only_exact_spelling(text in kind_like(), stage_text in stage_like()) {
            if let Some(kind) = EventKind::parse(&text) {
                prop_assert_eq!(kind.as_str(), text.as_str());
            }
            if let Some(stage) = Stage::parse(&stage_text) {
                prop_assert_eq!(stage.as_str(), stage_text.as_str());
            }
        }

        /// 任意 field の有無に依らず、log 行は書いて読むと同じ event になる。
        #[test]
        fn prop_fleet_event_round_trips_through_line(event in any_fleet_event()) {
            prop_assert_eq!(FleetEvent::from_line(&event.to_line()), Ok(event));
        }
    }
}

/// toml-lite の性質（(3) rules/manifest.rs）。
mod toml_lite {
    use super::{config, elements, list, quoted_once, scalar, Scalar};
    use proptest::prelude::*;

    /// 引用符を含まない 1 要素の中身。
    fn body() -> impl Strategy<Value = String> {
        "[^\"\n]{1,16}"
    }

    /// 要素の区切り（空白の有無を振る）。
    fn separator() -> impl Strategy<Value = &'static str> {
        prop::sample::select(vec![",", ", ", " ,", " , "])
    }

    /// `["a", "b"]` の字面を組む。
    fn render(items: &[String], sep: &str, trailing: bool) -> String {
        let quoted: Vec<String> = items.iter().map(|item| format!("\"{item}\"")).collect();
        let tail = if trailing { sep } else { "" };
        format!("[{}{tail}]", quoted.join(sep))
    }

    proptest! {
        #![proptest_config(config())]

        /// 任意の入力で 4 関数のどれも panic しない。
        #[test]
        fn prop_toml_lite_never_panics(raw in "(?s).{0,64}") {
            let _ = scalar(&raw);
            let _ = list(&raw);
            let _ = elements(&raw);
            let _ = quoted_once(&raw);
        }

        /// 引用符 1 組ちょうど（周りの空白は不問）なら true、中身に裸の `"` が在れば false。
        #[test]
        fn prop_toml_lite_quoted_once_sees_exactly_one_pair(inner in "[^\"\n]{0,16}", lead in "[ \t]{0,2}", tail in "[ \t]{0,2}", extra in "[^\"\n]{0,8}") {
            let exact = format!("{}\"{}\"{}", lead, inner, tail);
            let bare = format!("\"{}\"{}\"", inner, extra);
            prop_assert!(quoted_once(&exact), "{}", exact);
            prop_assert!(!quoted_once(&bare), "{}", bare);
        }

        /// 引用した要素の列は、区切りの空白や末尾の `,` に依らず同じ要素へ戻る。
        #[test]
        fn prop_toml_lite_list_round_trips_quoted_items(items in prop::collection::vec(body(), 1..6), sep in separator(), trailing in any::<bool>()) {
            prop_assert_eq!(list(&render(&items, sep, trailing)), Ok(items));
        }

        /// 空の配列と空文字の要素は受けない（「規則が無い」を空で表さない）。
        #[test]
        fn prop_toml_lite_list_rejects_empty_array_and_empty_item(items in prop::collection::vec(body(), 0..4), at in 0_usize..4, sep in separator()) {
            let mut with_empty = items;
            with_empty.insert(at.min(with_empty.len()), String::new());
            prop_assert!(list(&render(&with_empty, sep, false)).is_err());
            prop_assert!(list("[]").is_err());
        }

        /// 整数・真偽・引用文字列は scalar が同じ値で読む。
        #[test]
        fn prop_toml_lite_scalar_reads_each_shape(n in any::<u64>(), flag in any::<bool>(), text in "[^\"\n]{0,16}") {
            prop_assert_eq!(scalar(&n.to_string()), Some(Scalar::Int(n)));
            prop_assert_eq!(scalar(&flag.to_string()), Some(Scalar::Bool(flag)));
            prop_assert_eq!(scalar(&format!("\"{}\"", text)), Some(Scalar::Str(text)));
        }
    }
}

/// `rate_limit_status` の性質（(4) headless/runner.rs）。
mod rate_limit {
    use super::{config, rate_limit_line, rate_limit_status, status_ident, ws};
    use proptest::prelude::*;

    /// 8 箇所の空白（key の前後・colon の前後）。
    fn gaps() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec(ws(), 8)
    }

    proptest! {
        #![proptest_config(config())]

        /// key の周りの空白に依らず、`rate_limit_info` 直下の status を返す。
        #[test]
        fn prop_rate_limit_reads_status_through_whitespace(status in status_ident(), gap in gaps(), inner in gaps()) {
            let g = |i: usize| inner.get(i).map_or("", String::as_str);
            let info = format!("{{{}\"status\"{}:{}\"{}\"{}}}", g(0), g(1), g(2), status, g(3));
            let line = rate_limit_line("rate_limit_event", &info, &gap);
            prop_assert_eq!(rate_limit_status(&line), Some(status.as_str()), "{}", line);
        }

        /// type が違う行・`rate_limit_info` の無い行・object で始まらない行（正しい行から
        /// 先頭の `{` だけを落とした近傍）は None。
        #[test]
        fn prop_rate_limit_ignores_other_records(kind in "[a-z_]{1,16}", status in status_ident(), gap in gaps()) {
            prop_assume!(kind != "rate_limit_event");
            let info = format!("{{\"status\":\"{}\"}}", status);
            let other_kind = rate_limit_line(&kind, &info, &gap);
            prop_assert_eq!(rate_limit_status(&other_kind), None, "{}", other_kind);
            let without_info = format!("{{\"type\":\"rate_limit_event\",\"status\":\"{}\"}}", status);
            prop_assert_eq!(rate_limit_status(&without_info), None);
            let good = rate_limit_line("rate_limit_event", &info, &gap);
            prop_assert_eq!(rate_limit_status(&good), Some(status.as_str()), "{}", good);
            let unbraced = good.replacen('{', "", 1);
            prop_assert_eq!(rate_limit_status(&unbraced), None, "{}", unbraced);
        }

        /// 入れ子の別 object が持つ status は読まない（並び順に依らず）。
        #[test]
        fn prop_rate_limit_never_reads_nested_status(direct in status_ident(), nested in status_ident(), window in "[a-zA-Z]{1,12}", gap in gaps()) {
            prop_assume!(direct != nested);
            let nested_first = format!("{{\"{}\":{{\"status\":\"{}\"}},\"status\":\"{}\"}}", window, nested, direct);
            let line = rate_limit_line("rate_limit_event", &nested_first, &gap);
            prop_assert_ne!(rate_limit_status(&line), Some(nested.as_str()), "入れ子の status を読んだ: {}", line);
            let direct_first = format!("{{\"status\":\"{}\",\"{}\":{{\"status\":\"{}\"}}}}", direct, window, nested);
            let line = rate_limit_line("rate_limit_event", &direct_first, &gap);
            prop_assert_eq!(rate_limit_status(&line), Some(direct.as_str()), "{}", line);
        }
    }
}
