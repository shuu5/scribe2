//! 性質の歯（憲法 C12.7「property testing で歯を量産」の初着手・s2-07l.98）。
//!
//! 対象は **純関数 5 群**（Command を呼ぶ e2e は対象外）: 席の打刻 [`Stamp`]・fleet の
//! [`EventKind`] / [`Stage`] / [`Event`]・rules manifest の toml-lite・headless runner の
//! `rate_limit_status`・入れ子 JSON reader の [`Tree`]。入力は proptest の strategy で生成し、
//! 既知の反例（入れ子の status・空白・非 JSON・空要素・巨大な指数）を strategy の空間に含める。
//!
//! 反例の永続化（`proptest-regressions/`）は切る＝落ちた周に tracked tree を汚さない
//! （落ちた入力は nextest の出力に出る）。case 数は既定（256）。

use proptest::prelude::*;
use proptest::test_runner::Config;
use vessel::fleet::json_tree::{parse as parse_tree, Tree};
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

    /// 文字列の中へ入れてよい雑音（`{` `}` の任意の並びと **escape された backslash**）。
    /// `\\` を空間に含めるのは、閉じ引用符の直前が backslash の形（`"…\\"` ＝正しく閉じる）
    /// を通すためである——escape を「1 つ前が backslash か」だけで見る形はここで外れる。
    const NOISE_TEXT: &str = r"(\\\\|[{}]){0,8}";

    /// `status` より**前**に置く雑音の member（`"n<key>":<値>`）。値は 3 形:
    /// 文字列中の brace と escape された引用符・入れ子の object・入れ子の array である。
    /// key は `n` 接頭辞ゆえ `status` と衝突しない。
    fn noise_members() -> impl Strategy<Value = Vec<String>> {
        let member = ("[a-z]{1,8}", NOISE_TEXT, NOISE_TEXT, 0_usize..3);
        prop::collection::vec(member, 0..4).prop_map(|items| {
            items
                .into_iter()
                .map(|(key, braces, inner, shape)| {
                    let value = match shape {
                        0 => format!("\"{}\\\"{}\"", braces, inner),
                        1 => format!("{{\"a\":\"{}\\\"x\",\"b\":{{\"c\":\"{}\"}}}}", braces, inner),
                        _ => format!("[\"{}\",{{\"d\":\"{}\\\"\"}},[]]", braces, inner),
                    };
                    format!("\"n{}\":{}", key, value)
                })
                .collect()
        })
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

        /// `status` より前に何が来ても直下の status をそのまま返す: 文字列中の brace
        /// （`{` `}` の任意の並び）・escape された引用符・入れ子の object / array で
        /// 切り出しが手前で終わらない（`s2-07l.126` の反例を strategy の空間に置く）。
        ///
        /// 測るのは `s2-07l.126` で**既に land した挙動**である（本便は挙動不変の refactor で、
        /// この歯は base でも緑＝flip の RED にならない）。本便の RED は原始を直接撃つ
        /// `headless::runner` の in-file の歯が担う。
        // flip-check: retroactive s2-07l.129
        #[test]
        fn prop_rate_limit_status_survives_noise_before_status(status in status_ident(), noise in noise_members(), gap in gaps()) {
            let mut members = noise;
            members.push(format!("\"status\":\"{}\"", status));
            let info = format!("{{{}}}", members.join(","));
            let line = rate_limit_line("rate_limit_event", &info, &gap);
            prop_assert_eq!(rate_limit_status(&line), Some(status.as_str()), "{}", line);
        }
    }
}

/// 入れ子 JSON reader の性質（(5) fleet/json_tree.rs）。
mod json_tree {
    use super::{config, json_text, parse_tree, Tree};
    use proptest::prelude::*;
    use std::time::{Duration, Instant};

    /// JSON の number の字面（符号・小数部・指数の有無を振る）。
    fn number_text() -> impl Strategy<Value = String> {
        "-?(0|[1-9][0-9]{0,5})(\\.[0-9]{1,5})?([eE][+-]?[0-9]{1,3})?"
    }

    /// 深さ 8 までの任意の [`Tree`]（object の key は重複を落とす＝parse が拒む形を作らない）。
    fn any_tree() -> impl Strategy<Value = Tree> {
        let leaf = prop_oneof![
            Just(Tree::Null),
            any::<bool>().prop_map(Tree::Bool),
            json_text().prop_map(Tree::Str),
            number_text().prop_map(Tree::Num),
        ];
        leaf.prop_recursive(8, 48, 4, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..4).prop_map(Tree::Array),
                prop::collection::vec((json_text(), inner), 0..4)
                    .prop_map(|pairs| Tree::Object(dedup(pairs))),
            ]
        })
    }

    /// 同じ key の後続を落とす。
    fn dedup(pairs: Vec<(String, Tree)>) -> Vec<(String, Tree)> {
        let mut kept: Vec<(String, Tree)> = Vec::new();
        for (key, value) in pairs {
            if !kept.iter().any(|(found, _)| *found == key) {
                kept.push((key, value));
            }
        }
        kept
    }

    /// [`Tree`] を JSON text へ写す（**test 側の renderer**・実装は writer を持たない）。
    fn render(tree: &Tree) -> String {
        match tree {
            Tree::Null => "null".to_owned(),
            Tree::Bool(flag) => flag.to_string(),
            Tree::Num(text) => text.clone(),
            Tree::Str(text) => quote(text),
            Tree::Array(items) => {
                let body: Vec<String> = items.iter().map(render).collect();
                format!("[{}]", body.join(","))
            }
            Tree::Object(pairs) => {
                let body: Vec<String> = pairs
                    .iter()
                    .map(|(key, value)| format!("{}:{}", quote(key), render(value)))
                    .collect();
                format!("{{{}}}", body.join(","))
            }
        }
    }

    /// 文字列を escape して `"` で囲む。**非 ASCII は `\uXXXX`**（BMP 外は surrogate 対）で書く
    /// ＝round-trip が escape の経路を通る。
    fn quote(text: &str) -> String {
        let mut out = String::from("\"");
        for ch in text.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                other if other.is_ascii() && (other as u32) >= 0x20 => out.push(other),
                other => out.push_str(&escaped(other)),
            }
        }
        out.push('"');
        out
    }

    /// 1 文字を `\uXXXX`（BMP 外は上位 + 下位 surrogate の 2 つ）へ写す。
    fn escaped(ch: char) -> String {
        let code = u32::from(ch);
        if code <= 0xffff {
            return format!("\\u{code:04x}");
        }
        let rest = code.saturating_sub(0x10000);
        let high = 0xd800_u32.saturating_add(rest >> 10);
        let low = 0xdc00_u32.saturating_add(rest & 0x3ff);
        format!("\\u{high:04x}\\u{low:04x}")
    }

    /// 数の字面を組む（小数部は空なら `.` ごと落とす）。
    fn literal_of(int: &str, frac: &str, exp: i64, negative: bool) -> String {
        let sign = if negative { "-" } else { "" };
        let point = if frac.is_empty() {
            String::new()
        } else {
            format!(".{frac}")
        };
        format!("{sign}{int}{point}e{exp}")
    }

    /// 実装と**独立に**、10 進の字面を ×100 して切り捨てた値を求める（桁を文字列で数える形）。
    /// 20 桁を超える形と負数は `None`。
    fn expected_pct(int: &str, frac: &str, exp: i64, negative: bool) -> Option<u64> {
        let digits = format!("{int}{frac}");
        let significant = digits.trim_start_matches('0');
        if significant.is_empty() {
            return Some(0);
        }
        if negative {
            return None;
        }
        let lead = i128::try_from(digits.len().saturating_sub(significant.len())).ok()?;
        let int_len = i128::try_from(int.len()).ok()?;
        let point = i128::from(exp) + int_len + 2 - lead;
        if point <= 0 {
            return Some(0);
        }
        let width = usize::try_from(point).ok().filter(|found| *found <= 20)?;
        let mut taken: String = significant.chars().take(width).collect();
        while taken.len() < width {
            taken.push('0');
        }
        taken.parse::<u128>().ok().and_then(|found| u64::try_from(found).ok())
    }

    proptest! {
        #![proptest_config(config())]

        /// 任意の Tree は text へ写して読み戻すと同じ Tree に戻る。
        #[test]
        fn prop_json_tree_round_trips_through_text(tree in any_tree()) {
            let text = render(&tree);
            prop_assert_eq!(parse_tree(&text), Ok(tree), "{}", text);
        }

        /// 任意の文字列で panic しない。読めた字面は書き戻しても同じ Tree で、
        /// 読めない字面は理由を持つ（どちらも値で返る）。
        #[test]
        fn prop_json_tree_never_panics_on_any_text(raw in "(?s).{0,64}") {
            match parse_tree(&raw) {
                Ok(tree) => {
                    let again = parse_tree(&render(&tree));
                    prop_assert_eq!(again, Ok(tree), "{}", raw);
                }
                Err(reason) => prop_assert!(!reason.to_string().is_empty(), "{}", raw),
            }
        }

        /// 任意の数（整数部 30 桁・小数部 30 桁・指数は i64 全域）で `as_pct` は 1 秒以内に
        /// Option を返し、Some は 10 進の評価 ×100 の切り捨てと一致する。
        #[test]
        fn prop_json_tree_pct_matches_decimal_evaluation(
            int in "(0|[1-9][0-9]{0,29})",
            frac in "[0-9]{0,30}",
            exp in any::<i64>(),
            negative in any::<bool>(),
        ) {
            let literal = literal_of(&int, &frac, exp, negative);
            let started = Instant::now();
            let read = parse_tree(&literal);
            let got = match &read {
                Ok(tree) => tree.as_pct(),
                Err(_) => None,
            };
            let elapsed = started.elapsed();
            prop_assert!(read.is_ok(), "読めない字面: {}", literal);
            prop_assert!(elapsed < Duration::from_secs(1), "{:?} かかった: {}", elapsed, literal);
            prop_assert_eq!(got, expected_pct(&int, &frac, exp, negative), "{}", literal);
        }

        /// 独立した算術との一致: 整数は ×100、`0.<小数>` は先頭 2 桁（切り捨て）。
        #[test]
        fn prop_json_tree_pct_agrees_with_plain_arithmetic(whole in any::<u32>(), frac in "[0-9]{2,8}") {
            let read = parse_tree(&whole.to_string());
            prop_assert_eq!(read.ok().and_then(|tree| tree.as_pct()), Some(u64::from(whole) * 100), "{}", whole);
            let text = format!("0.{}", frac);
            let head: String = frac.chars().take(2).collect();
            let read = parse_tree(&text);
            prop_assert_eq!(read.ok().and_then(|tree| tree.as_pct()), head.parse::<u64>().ok(), "{}", text);
        }
    }
}
