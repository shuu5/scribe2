//! 発話の記帳（設計 fleet-event-log.md §13・ADR-0083 / ADR-0087・FR82）: `user-prompt-submit` の枝で、user の prompt を model が
//! 読む前に `UtteranceReceived` として 1 件記帳し、ts の 1 行を返す。台帳は読まず（bd を撃たず）、読む byte は log の大きさに
//! 依らない（[`store::append_utterance`]）。器が入力欄へ差し込んだ行・別の session と harness からの包み・runner は記帳しない。
//! 書けない周も prompt を止めず（rc 0）、理由の 1 語を stderr に 1 行出す。

use super::{flag_of, Hooked, FLAG_PLUGIN_ROOT, KEY_SESSION_ID};
use crate::fleet::store::{self, LockPolicy, StoreError};
use crate::fleet::{json_tree, Case, Channel, Event, EventKind, SCHEMA};
use crate::name::NAME;
use crate::rules::manifest::Manifest;
use std::path::Path;

/// payload から拾う key（user が打った字）。
const KEY_PROMPT: &str = "prompt";

/// 器の差し込みの行が `<NAME> <語>:` で名乗る語（**閉じた 4 つ**・作り手は §13 の 9 種）。
const LEAD_WORDS: [&str; 4] = ["pipe", "seat", "group", "tick"];

/// 別の session と harness からの包みの頭（**閉じた 6 つ**・先頭の空白を除いて照らす）。
const WRAPPER_HEADS: [&str; 6] = [
    "Another Claude session sent a message:",
    "<cross-session-message",
    "<agent-message",
    "<teammate-message",
    "<task-notification>",
    "This session is being continued from a previous conversation",
];

/// 記帳しない周の理由の語（**閉じた 3 つ**）。
const NO_SESSION: &str = "no-session";
/// lock を取れない（待ちの rules 行を読めない周を含む）。
const LOCK: &str = "lock";
/// 書けない。
const WRITE: &str = "write";

/// 発話を記帳し、(stdout の行, stderr の行) を返す。記帳した周だけ stdout に `<NAME> utterance: ts=<ts>` の 1 行。
pub(super) fn record(hooked: &Hooked, args: &[String], payload: &str) -> (Vec<String>, Vec<String>) {
    let tree = json_tree::parse(payload).ok();
    let text = |key: &str| tree.as_ref().and_then(|found| found.get(key)).and_then(json_tree::Tree::as_str);
    let Some(prompt) = text(KEY_PROMPT).filter(|words| !words.trim().is_empty()) else {
        return (Vec::new(), Vec::new());
    };
    if is_injected(prompt) || is_runner(hooked.dir, flag_of(args, FLAG_PLUGIN_ROOT)) {
        return (Vec::new(), Vec::new());
    }
    let Some(session) = text(KEY_SESSION_ID).filter(|sid| !sid.is_empty()) else {
        return unrecorded(NO_SESSION);
    };
    match write(hooked, &utterance(session, prompt)) {
        Ok((ts, warnings)) => (vec![format!("{NAME} utterance: ts={ts}")], warnings),
        Err(reason) => unrecorded(reason),
    }
}

/// 記帳できない周の外形: stdout 0 byte・stderr に理由の 1 行。
fn unrecorded(reason: &str) -> (Vec<String>, Vec<String>) {
    (Vec::new(), vec![format!("{NAME}: utterance unrecorded reason={reason}")])
}

/// 書く行（`UtteranceReceived`・actor human・経路 chat・session・逐語の detail・run 無し）。ts は store が lock の中で振る。
fn utterance(session: &str, words: &str) -> Event {
    Event {
        schema: SCHEMA,
        ts: String::new(),
        kind: EventKind::UtteranceReceived,
        run: String::new(),
        bead: String::new(),
        host: crate::fleet::cli::host(),
        actor: EventKind::UtteranceReceived.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: Some(words.to_owned()),
        allowance: None,
        registration: None,
        mark: None,
        account: None,
        cost: None,
        rule: None,
        case: Some(Case::Utterance { channel: Channel::Chat, session: Some(session.to_owned()) }),
    }
}

/// lock の待ちを rules から読み、発話を追記する。失敗は理由の語（[`LOCK`]・[`WRITE`]）に畳む。
fn write(hooked: &Hooked, event: &Event) -> Result<(String, Vec<String>), &'static str> {
    let manifest = hooked.rules.map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path)));
    let policy = manifest.ok().and_then(|found| LockPolicy::from_rules(&found).ok()).ok_or(LOCK)?;
    match store::append_utterance(hooked.dir, event, policy) {
        Ok((ts, warnings)) => Ok((ts, warnings.iter().map(|found| found.as_str().to_owned()).collect())),
        Err(StoreError::Lock(_) | StoreError::ReclaimToken(_)) => Err(LOCK),
        Err(_) => Err(WRITE),
    }
}

/// 器が入力欄へ差し込んだ行か（頭が `<NAME> <語>:`・語は [`LEAD_WORDS`]）と、別の session と harness からの包みか
/// （先頭の空白を除いた頭が [`WRAPPER_HEADS`] のどれか）。
fn is_injected(prompt: &str) -> bool {
    let named = prompt.strip_prefix(NAME).and_then(|rest| rest.strip_prefix(' ')).is_some_and(|rest| {
        LEAD_WORDS.iter().any(|word| rest.strip_prefix(word).is_some_and(|tail| tail.starts_with(':')))
    });
    named || WRAPPER_HEADS.iter().any(|head| prompt.trim_start().starts_with(head))
}

/// runner か（`--plugin-root` が置き場の `pipe` の dir の下・便の plugin の写しを積む起動）。
fn is_runner(state_dir: &Path, plugin_root: Option<&str>) -> bool {
    plugin_root.is_some_and(|root| Path::new(root).starts_with(state_dir.join(crate::pipe::DIR)))
}

#[cfg(test)]
mod tests {
    use super::{is_injected, utterance};
    use crate::fleet::store::{self, LockPolicy};
    use crate::fleet::{cli, epoch_ms_of, Event, EventKind, Stage};
    use crate::pipe::fixture::{event, scratch};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    const SIZE_KIB: usize = 1024;

    fn now_ms() -> u64 {
        let since = SystemTime::now().duration_since(UNIX_EPOCH).expect("時計は epoch の後");
        u64::try_from(since.as_millis()).expect("ms は u64 に収まる")
    }

    /// `ts` の発話 1 件の後ろに、他の event の行を合わせて `filler` byte 以上続けた log を置く。
    fn log_after(name: &str, ts: &str, filler: usize) -> PathBuf {
        let dir = scratch(name);
        let mut first = utterance("s", "先");
        first.ts = ts.to_owned();
        let mut text = format!("{}\n", first.to_line());
        let other = format!("{}\n", event("r", EventKind::RunStage, Some(Stage::Gated), None, None).to_line());
        let start = text.len();
        while text.len() - start < filler {
            text.push_str(&other);
        }
        std::fs::create_dir_all(store::events_path(&dir).parent().expect("親 dir")).expect("dir を作れる");
        std::fs::write(store::events_path(&dir), text).expect("log を書ける");
        dir
    }

    /// 1 件追記して、振られた ts（ミリ秒）と撃つ前後の時刻を返す。
    fn append_once(dir: &Path) -> (u64, u64, u64) {
        let policy = LockPolicy { retry_ms: 1_000, stale_ms: 600_000 };
        let before = now_ms();
        let (ts, _) = store::append_utterance(dir, &utterance("s", "後"), policy).expect("追記できる");
        let after = now_ms();
        (epoch_ms_of(&ts).expect("ミリ秒の字面"), before, after)
    }

    /// (f) 最後の発話の ts が今より 1 時間先の log に 1 件足すと、振られた ts はちょうど先の ts + 1 ms。
    #[test]
    fn utterance_tail_after_a_future_utterance_is_exactly_one_ms_later() {
        let far = now_ms() + 3_600_000;
        let dir = log_after("tail-future", &cli::format_utc_ms(far), 0);
        assert_eq!(append_once(&dir).0, far + 1, "先の ts + 1 ms");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// (g) 先の発話の後に他の event が 80 KiB 続く log では、窓（末尾 64 KiB）の外の発話は見ず、振られた ts は今。
    #[test]
    fn utterance_tail_window_does_not_reach_an_utterance_beyond_64_kib() {
        let dir = log_after("tail-beyond", &cli::format_utc_ms(now_ms() + 3_600_000), 80 * SIZE_KIB);
        let (got, before, after) = append_once(&dir);
        assert!((before..=after).contains(&got), "今: {before} <= {got} <= {after}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// (h) 同じ先の発話の後に 60 KiB なら窓の内で、振られた ts は先の ts + 1 ms（最後の 1 行だけを見る実装を落とす）。
    #[test]
    fn utterance_tail_window_reaches_an_utterance_within_64_kib() {
        let far = now_ms() + 3_600_000;
        let dir = log_after("tail-within", &cli::format_utc_ms(far), 60 * SIZE_KIB);
        assert_eq!(append_once(&dir).0, far + 1, "窓の内の発話 + 1 ms");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// (i) 作り手 9 種の 1 行を実物の関数で作り、全部を差し込みと読む。頭の語を 1 つ変えた行と包みの 2 行目だけの形は読まない。
    #[test]
    fn utterance_tail_every_injected_line_maker_is_read_as_injected() {
        use crate::pipe::dispatch::facts::{Fact, Facts};
        use crate::pipe::dispatch::{Candidate, Turn};
        use crate::pipe::notify::{idle_line, precheck_line, terminal_line, Memos, Terminal};
        let dir = scratch("tail-makers");
        std::fs::write(dir.join("host.toml"), "schema = 1\n\n[[account]]\nlabel = \"l1\"\n\n[[account-group]]\nname = \"Tier1\"\nanchors = [\"/g\"]\naccounts = [\"l1\"]\n")
            .expect("host の面を書ける");
        let manifest = crate::rules::manifest::Manifest::embedded().expect("面を読める").with_host(&dir.join("host.toml")).expect("host の面を合わせられる");
        let group = manifest.groups().first().expect("群が 1 つ");
        let candidate = Candidate { bead: "b".to_owned(), priority: None, mark: None, reason: None };
        let turn = Turn { candidates: vec![candidate], launches: Vec::new(), revives: Vec::new(), unmeasured: None, drive: None, vessel: None, lifecycle: None, triage: None };
        let facts = Facts { live: Fact::Absent, idle: Fact::Absent, held: None, precheck: None, unreflected: 0, floor: None };
        let pace = crate::seat::tick::signal::Pace::of(60, &["30".to_owned(), "90".to_owned()]).expect("梯子を読める");
        let pressed = crate::hook::group::Pressed { window: crate::fleet::WindowKind::FiveHour, used: 90, cap: 85 };
        let made = [
            ("notify::terminal_line", terminal_line(&Terminal { bead: "b", run: "r", stage: "Landed", word: "ok", streak: 0 })),
            ("notify::idle_line", idle_line(&turn, &facts, Some(&[]), &Memos::default()).expect("候補が在る周は idle の行")),
            ("notify::precheck_line", precheck_line(&(vec![("x".to_owned(), PathBuf::from("/p"))], 1))),
            ("deliver::line", crate::seat::deliver::line("r-1")),
            ("signal::signal", crate::seat::tick::signal::signal(0, &pace)),
            ("tick::relaunch_signal", crate::seat::tick::relaunch_signal()),
            ("group::evacuate_line", crate::hook::group::evacuate_line("Tier1", "l1", 60)),
            ("group::refused_line", crate::hook::group::refused_line(group)),
            ("dispatch::pressure_line", crate::pipe::dispatch::group::pressure_line("Tier1", "l1", pressed)),
        ];
        assert_eq!(made.len(), 9, "作り手は 9 種（母集団）");
        for (maker, line) in &made {
            assert!(is_injected(line), "{maker} の行は差し込み: {line}");
            assert!(is_injected(&format!("{line}\n本文")), "{maker}: 後ろに本文の行が続いても差し込み");
            assert!(!is_injected(&line.replacen(crate::name::NAME, "other", 1)), "{maker}: 別の名の行は差し込みでない");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// (j) ミリ秒の字面の作りと読みが往復し、秒の形と桁の違う形は読まない。
    #[test]
    fn utterance_tail_millisecond_form_round_trips() {
        for ms in [0, 1, 999, 1_000, 1_759_190_400_123, 4_102_444_799_999] {
            let text = cli::format_utc_ms(ms);
            assert_eq!(text.len(), "YYYY-MM-DDTHH:MM:SS.mmmZ".len(), "字面の長さ: {text}");
            assert_eq!(epoch_ms_of(&text), Some(ms), "往復: {text}");
        }
        for bad in ["2026-09-30T01:02:03Z", "2026-09-30T01:02:03.4Z", "2026-09-30T01:02:03.4567Z", "2026-09-30T01:02:03.abcZ", "2026-09-30T01:02:03.123"] {
            assert_eq!(epoch_ms_of(bad), None, "読まない: {bad}");
        }
    }

    /// 書く行は log の 1 行として読み戻せ、逐語（改行と `"`）は 1 byte も変わらず run を持たない。
    #[test]
    fn utterance_tail_line_reads_back_as_an_utterance_event() {
        let mut line = utterance("s", "a\n\"b\"");
        line.ts = cli::format_utc_ms(1);
        let back = Event::from_line(&line.to_line()).expect("読める");
        assert_eq!((back.kind, back.detail.as_deref(), back.run.as_str()), (EventKind::UtteranceReceived, Some("a\n\"b\""), ""));
    }
}
