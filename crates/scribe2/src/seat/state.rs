//! 席の状態（busy / idle）を hook の打刻で typed に持つ面（設計 docs/design/seat-state.md §2 / §4・
//! ADR-0015・憲法 C3.3 / C10 / C11・SRS FR27 / FR28）。
//!
//! 出所は Claude Code の hook event（`UserPromptSubmit` → Busy・`Stop` / `SessionStart` → Idle）で、
//! 打刻は `<state_dir>/seat/<target>/state.jsonl` へ 1 行 JSON を append する（lock は fleet と同じ
//! 1 実装 [`store::append_line`]）。tick と cycle は**最終行**を [`read_last`] の 1 本で読む。
//! 作り直しと送達の**証拠**（設計 §6・`s2-07l.112`）は [`evidence_after`] の 1 本で読む: 送る前に
//! [`baseline`] を取り、その後ろに足された打刻のうち送達 ts 以後のものだけを証拠に採る。
//! pane の字面は判定入力にしない（C3.3）。
//!
//! **読めない側は注入しない側へ倒す**（fail-closed・ADR-0015 §2.3）: file が無い・読めない・Busy が
//! 古い、はいずれも [`Read`] の別 variant で持ち、missing を idle に、stale を busy に読み替えない。

use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{self, LockPolicy, StoreError, Warning};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 打刻 file の名前。
pub const FILE: &str = "state.jsonl";
/// 打刻 1 行の schema 版。非互換な変更で上げる。
pub const SCHEMA: u64 = 1;
/// Busy の打刻を stale と見なす閾値を宣言する rules 行の id（**値は code に焼かない**・憲法 C5）。
/// tick の鮮度と**共用**する（新しい閾値は足さない・ADR-0015 §2.4）。
pub const ID_STALE: &str = "seat.tick_stale_s";

/// key: schema 版。
const KEY_SCHEMA: &str = "schema";
/// key: 状態。
const KEY_STATE: &str = "state";
/// key: 出所の hook event。
const KEY_EVENT: &str = "event";
/// key: 打刻時刻（1970 年からの秒・UTC）。
const KEY_TS: &str = "ts";
/// key: 打刻した session の id（hook の stdin JSON から写す・無ければ空）。
const KEY_SID: &str = "sid";

/// 席の状態。**閉じた 2 値**（憲法 C3.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatState {
    /// user の入力を受けて turn が走っている。
    Busy,
    /// turn が終わった・または session が始まった直後。
    Idle,
}

impl SeatState {
    /// 打刻と判定行に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::Idle => "idle",
        }
    }

    /// 字面から読む。未知は `None`。
    fn parse(text: &str) -> Option<Self> {
        match text {
            "busy" => Some(Self::Busy),
            "idle" => Some(Self::Idle),
            _ => None,
        }
    }
}

/// 打刻の出所 = Claude Code の hook event（**閉じた 3 値**・C10 の出所）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// session が始まった（作り直し・再開・`/clear` の後）。
    SessionStart,
    /// user の入力（注入された 1 行を含む）が submit された。
    UserPromptSubmit,
    /// turn が終わった。
    Stop,
}

impl Event {
    /// 打刻と判定行に使う字面（Claude Code の event 名そのまま）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::Stop => "Stop",
        }
    }

    /// この event が意味する状態（設計 §2）。
    pub fn state(self) -> SeatState {
        match self {
            Self::UserPromptSubmit => SeatState::Busy,
            Self::SessionStart | Self::Stop => SeatState::Idle,
        }
    }

    /// 字面から読む。未知は `None`。
    fn parse(text: &str) -> Option<Self> {
        match text {
            "SessionStart" => Some(Self::SessionStart),
            "UserPromptSubmit" => Some(Self::UserPromptSubmit),
            "Stop" => Some(Self::Stop),
            _ => None,
        }
    }
}

/// 打刻 1 行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    /// schema 版。
    pub schema: u64,
    /// 状態（[`Event::state`] と一致する）。
    pub state: SeatState,
    /// 出所の hook event。
    pub event: Event,
    /// 打刻時刻（1970 年からの秒・UTC）。
    pub ts: u64,
    /// 打刻した session の id（無ければ空）。
    pub sid: String,
}

impl Stamp {
    /// いま打つ 1 行。
    pub fn now(event: Event, sid: &str) -> Self {
        Self {
            schema: SCHEMA,
            state: event.state(),
            event,
            ts: unix_secs(SystemTime::now()),
            sid: sid.to_owned(),
        }
    }

    /// 1 行の flat JSON にする。
    pub fn to_line(&self) -> String {
        json_lite::write_object(&[
            (KEY_SCHEMA, Value::Num(self.schema)),
            (KEY_STATE, Value::Str(self.state.as_str().to_owned())),
            (KEY_EVENT, Value::Str(self.event.as_str().to_owned())),
            (KEY_TS, Value::Num(self.ts)),
            (KEY_SID, Value::Str(self.sid.clone())),
        ])
    }

    /// 1 行を読む。key の欠落・型違い・schema 違い・state と event の不一致はいずれも `Err`
    /// （黙って別物を通さない）。
    pub fn from_line(line: &str) -> Result<Self, String> {
        let pairs = json_lite::parse_object(line)?;
        let field = |key: &str| {
            pairs
                .iter()
                .find(|(found, _)| found == key)
                .map(|(_, value)| value)
                .ok_or_else(|| format!("{key} が無い"))
        };
        let schema = field(KEY_SCHEMA)?
            .as_num()
            .ok_or_else(|| format!("{KEY_SCHEMA} が整数でない"))?;
        if schema != SCHEMA {
            return Err(format!("{KEY_SCHEMA} が {SCHEMA} でない（{schema}）"));
        }
        let state = str_field(field(KEY_STATE)?, KEY_STATE)
            .and_then(|text| SeatState::parse(text).ok_or_else(|| format!("{KEY_STATE} が未知（{text}）")))?;
        let event = str_field(field(KEY_EVENT)?, KEY_EVENT)
            .and_then(|text| Event::parse(text).ok_or_else(|| format!("{KEY_EVENT} が未知（{text}）")))?;
        if event.state() != state {
            return Err(format!("{KEY_STATE} と {KEY_EVENT} が食い違う"));
        }
        let ts = field(KEY_TS)?
            .as_num()
            .ok_or_else(|| format!("{KEY_TS} が整数でない"))?;
        let sid = str_field(field(KEY_SID)?, KEY_SID)?.to_owned();
        Ok(Self {
            schema,
            state,
            event,
            ts,
            sid,
        })
    }
}

/// 文字列の値を借りる。
fn str_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{key} が文字列でない"))
}

/// 1970 年からの秒。読めなければ 0。
fn unix_secs(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// 打刻 file の path。
pub fn path(seat_dir: &Path) -> PathBuf {
    seat_dir.join(FILE)
}

/// stale 閾値（秒）を manifest から読む。不発効・別の形・不在は `None`。
pub fn stale_s() -> Option<u64> {
    super::int_rule(ID_STALE)
}

/// 1 行を追記する。lock は fleet と同じ実装を通る（第 2 の writer を作らない・C6.3）。
pub fn append(seat_dir: &Path, stamp: &Stamp) -> Result<Vec<Warning>, StoreError> {
    let policy = LockPolicy::embedded()?;
    store::append_line(&path(seat_dir), &stamp.to_line(), policy)
}

/// 最終行の読み。**5 値で閉じる**（憲法 C11: 読めないことを状態に潰さない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// 最終行が Busy で、閾値の内側。
    Busy(Event),
    /// 最終行が Idle。
    Idle(Event),
    /// file が無い（hook が載っていない席・v1 の席）。
    Missing,
    /// file は在るが読めない・最終行が壊れている・空。
    Unreadable,
    /// 最終行が Busy で、`ts` が閾値より古い（hook が死んだ疑い・busy とも idle とも言わない）。
    Stale(Event),
}

impl Read {
    /// 判定行に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Busy(_) => "busy",
            Self::Idle(_) => "idle",
            Self::Missing => "missing",
            Self::Unreadable => "unreadable",
            Self::Stale(_) => "stale",
        }
    }

    /// 値の出所（読めなかった周は `None`）。
    pub fn event(self) -> Option<Event> {
        match self {
            Self::Busy(event) | Self::Idle(event) | Self::Stale(event) => Some(event),
            Self::Missing | Self::Unreadable => None,
        }
    }

    /// 判定行の末尾に足す字面（`state=<値> event=<出所|none>`・C10 の出所付き）。
    pub fn suffix(self) -> String {
        format!(
            " state={} event={}",
            self.as_str(),
            self.event().map_or("none", Event::as_str)
        )
    }
}

/// 最終行を読む（tick と cycle の**唯一の読み口**）。
///
/// `stale_s` は rules 行 [`ID_STALE`] の値（呼び側が引く）。Busy の `ts` が `now` より
/// `stale_s` 秒を超えて古ければ [`Read::Stale`]。未来の `ts` は経過 0 と数える（stale 側へ倒さない）。
pub fn read_last(seat_dir: &Path, stale_s: u64) -> Read {
    let text = match std::fs::read_to_string(path(seat_dir)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Read::Missing,
        Err(_) => return Read::Unreadable,
    };
    let Some(line) = text.lines().rev().find(|line| !line.trim().is_empty()) else {
        return Read::Unreadable;
    };
    let Ok(stamp) = Stamp::from_line(line) else {
        return Read::Unreadable;
    };
    classify(&stamp, stale_s, unix_secs(SystemTime::now()))
}

/// 打刻を読みに写す（時刻を引数に取る＝判定を時計から切り離す）。
fn classify(stamp: &Stamp, stale_s: u64, now: u64) -> Read {
    match stamp.state {
        SeatState::Idle => Read::Idle(stamp.event),
        SeatState::Busy if now.saturating_sub(stamp.ts) > stale_s => Read::Stale(stamp.event),
        SeatState::Busy => Read::Busy(stamp.event),
    }
}

/// いまの 1970 年からの秒（送達 ts を取る呼び側の 1 本・打刻と同じ時計）。
pub fn now_secs() -> u64 {
    unix_secs(SystemTime::now())
}

/// 送る**前**に取る基線 = 打刻 file の行数。証拠に採るのは**この行より後ろに足された**打刻だけ
/// ——送る前から在った打刻（前の cycle の `SessionStart`・同じ秒の古い行）を「送達 ts 以後」と
/// 読まないため（設計 seat-state.md §6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Baseline {
    /// 送る前の行数（file が無い・読めない周は 0＝足された行は全部が候補・ts が守る）。
    lines: usize,
}

/// 基線を取る。
pub fn baseline(seat_dir: &Path) -> Baseline {
    let lines = std::fs::read_to_string(path(seat_dir)).map_or(0, |text| text.lines().count());
    Baseline { lines }
}

/// 証拠の読み。**4 値で閉じる**（憲法 C11: 「まだ無い」「file が無い」「読めない」を混ぜない・
/// missing を消費や作り直しに読み替えない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evidence {
    /// 基線より後ろに、送達 ts 以後の当該 event の打刻が在る。
    Found(Stamp),
    /// file は読めるが、該当する打刻が**まだ**無い（古い打刻・別 event・壊れた行は数えない）。
    NotYet,
    /// file が無い（hook が載っていない席）。
    Missing,
    /// file を読めない。
    Unreadable,
}

/// 基線より後ろに足された行のうち、`event` で `ts >= since` の打刻を探す（作り直し = `SessionStart`・
/// 送達の消費 = `UserPromptSubmit`・設計 §6）。
///
/// `>=` は打刻が秒粒度で、送った同じ秒に hook が打つ周を落とさないため。古い行を `>=` で拾わない
/// のは基線が守る（送る前に在った行は見ない）。壊れた行は証拠に数えない（読めた行だけ）。
pub fn evidence_after(seat_dir: &Path, baseline: Baseline, event: Event, since: u64) -> Evidence {
    let text = match std::fs::read_to_string(path(seat_dir)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Evidence::Missing,
        Err(_) => return Evidence::Unreadable,
    };
    text.lines()
        .skip(baseline.lines)
        .filter_map(|line| Stamp::from_line(line).ok())
        .find(|stamp| stamp.event == event && stamp.ts >= since)
        .map_or(Evidence::NotYet, Evidence::Found)
}

#[cfg(test)]
mod tests {
    use super::{classify, Event, Read, SeatState, Stamp, SCHEMA};

    /// 打刻は書いた行から同じ値で読める（round-trip）。
    #[test]
    fn seat_state_stamp_round_trips_through_line() {
        let stamp = Stamp {
            schema: SCHEMA,
            state: SeatState::Busy,
            event: Event::UserPromptSubmit,
            ts: 1_757_600_000,
            sid: "sid-1".to_owned(),
        };
        assert_eq!(Stamp::from_line(&stamp.to_line()), Ok(stamp));
    }

    /// state と event の食い違い・schema 違い・key 欠落は読まない（黙って別物を通さない）。
    #[test]
    fn seat_state_stamp_rejects_inconsistent_lines() {
        let cases = [
            r#"{"schema":1,"state":"idle","event":"UserPromptSubmit","ts":1,"sid":""}"#,
            r#"{"schema":2,"state":"busy","event":"UserPromptSubmit","ts":1,"sid":""}"#,
            r#"{"schema":1,"state":"busy","event":"UserPromptSubmit","sid":""}"#,
            r#"{"schema":1,"state":"running","event":"UserPromptSubmit","ts":1,"sid":""}"#,
        ];
        for line in cases {
            assert!(Stamp::from_line(line).is_err(), "{line}");
        }
    }

    /// stale の境界は閾値**ちょうど**まで busy、1 秒超で stale。Idle は経過に依らず idle。
    #[test]
    fn seat_state_classify_uses_threshold_as_inclusive_busy_bound() {
        let busy = Stamp::now(Event::UserPromptSubmit, "");
        let at = busy.ts;
        assert_eq!(classify(&busy, 10, at + 10), Read::Busy(Event::UserPromptSubmit));
        assert_eq!(classify(&busy, 10, at + 11), Read::Stale(Event::UserPromptSubmit));
        assert_eq!(classify(&busy, 10, at.saturating_sub(5)), Read::Busy(Event::UserPromptSubmit), "未来の ts は stale にしない");
        let idle = Stamp::now(Event::Stop, "");
        assert_eq!(classify(&idle, 10, idle.ts + 1_000_000), Read::Idle(Event::Stop));
    }
}
