//! 席の状態（busy / idle）を hook の打刻で typed に持つ面（設計 docs/design/seat-state.md §2 / §4・
//! ADR-0015・憲法 C3.3 / C10 / C11）。
//!
//! 出所は Claude Code の hook event（`UserPromptSubmit` → Busy・`Stop` / `SessionStart` → Idle）で、
//! 打刻は `<state_dir>/seat/<target>/state.jsonl` へ 1 行 JSON を append する（lock は fleet と同じ
//! 1 実装 [`store::append_line`]）。
//! 読み手は**証拠の 1 本**（[`evidence_after`]・設計 §6・`s2-07l.112`）だけである: 送る前に
//! [`baseline`] を取り、その後ろに足された打刻のうち送達 ts 以後のものだけを証拠に採る。
//! 起動と立て直しの確認（`seat launch`・ADR-0045 §2 (4) の不変の面）と注入の消費の確認がこれを読む。
//! pane の字面は判定入力にしない（C3.3）。
//!
//! **最終行を状態として読む面は消えた**（`s2-07l.479.3`）: その読み手（管理 tick と作り直しの cycle）は
//! `s2-07l.479.1` で機構ごと消え、閾値の rules 行も同じ便で消えた。最終行の sid だけは起こし直しが会話を運ぶ読み手
//! （[`resume_carry`]・seat-heartbeat.md §7 形 3）が読む。

use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{self, LockPolicy, StoreError, Warning};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 打刻 file の名前。
pub const FILE: &str = "state.jsonl";
/// 打刻 1 行の schema 版。非互換な変更で上げる。
pub const SCHEMA: u64 = 1;
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

/// 1 行を追記する。lock は fleet と同じ実装を通る（第 2 の writer を作らない・C6.3）。
pub fn append(seat_dir: &Path, stamp: &Stamp) -> Result<Vec<Warning>, StoreError> {
    let policy = LockPolicy::embedded()?;
    store::append_line(&path(seat_dir), &stamp.to_line(), policy)
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
    /// 送る前の行数。file が無い周は `Some(0)`（足された行は全部が候補）。**読めない周は `None`**
    /// （基線を測れていない＝以後の証拠を採らない・fail-closed。0 行に潰すと、送る前から在った
    /// 同じ秒の打刻が「送達 ts 以後」に化ける・lens-112 HIGH-2）。
    lines: Option<usize>,
}

/// 基線を取る。
pub fn baseline(seat_dir: &Path) -> Baseline {
    let lines = match std::fs::read_to_string(path(seat_dir)) {
        Ok(text) => Some(text.lines().count()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Some(0),
        Err(_) => None,
    };
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
    let Some(skip) = baseline.lines else {
        return Evidence::Unreadable;
    };
    let text = match std::fs::read_to_string(path(seat_dir)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Evidence::Missing,
        Err(_) => return Evidence::Unreadable,
    };
    text.lines()
        .skip(skip)
        .filter_map(|line| Stamp::from_line(line).ok())
        .find(|stamp| stamp.event == event && stamp.ts >= since)
        .map_or(Evidence::NotYet, Evidence::Found)
}

/// 起こし直しが会話を運ぶ flag（claude の起動行の末尾・設計 seat-heartbeat.md §7 形 3）。
pub const RESUME_FLAG: &str = "--resume";

/// 起こし直しの `carry`（**読み手はこの 1 本**・設計 seat-heartbeat.md §7 形 3 / §8 形 1）: 席の置き場の打刻の最終行（読めた行の
/// うち最後）の sid が会話 id の形なら `--resume <sid>` の 2 語、file が無い・読めない・sid が無い・形違いの周は空（運ばない）。
pub fn resume_carry(seat_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(path(seat_dir))
        .ok()
        .and_then(|text| last_sid(&text))
        .map(|sid| vec![RESUME_FLAG.to_owned(), sid])
        .unwrap_or_default()
}

/// 打刻の本文の最終行（読めた行のうち最後）の sid。会話 id の形（[`is_session_id`]）でない周は `None`（前の行の sid に倒れない）。
pub fn last_sid(text: &str) -> Option<String> {
    let last = text.lines().rev().find_map(|line| Stamp::from_line(line).ok())?;
    is_session_id(&last.sid).then_some(last.sid)
}

/// 会話 id の形か: UUID の 8-4-4-4-12 の 16 進（claude の session id）。起動行へ写す字面なので形の外は 1 字も通さない。
fn is_session_id(sid: &str) -> bool {
    let parts: Vec<&str> = sid.split('-').collect();
    parts.iter().map(|part| part.len()).eq([8, 4, 4, 4, 12]) && parts.iter().all(|part| part.chars().all(|found| found.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::{last_sid, resume_carry, Event, SeatState, Stamp, RESUME_FLAG, SCHEMA};

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

    /// 会話 id の形の sid の打刻 1 行（最終行の読み手の fixture）。
    fn sid_line(event: Event, sid: &str) -> String {
        Stamp { schema: SCHEMA, state: event.state(), event, ts: 1_757_600_000, sid: sid.to_owned() }.to_line()
    }

    /// 会話 id の形。
    const UUID: &str = "0b7c3f5e-9a1d-4c2e-8f6a-1d2e3f4a5b6c";

    /// 最終行の sid の読み手は UUID の形だけを返す: 最終行の sid が形どおりなら返し、最終行が空・形違いなら前の行の UUID に
    /// 倒れず `None`。壊れた行は最終行に数えない（読めた行のうち最後）。
    #[test]
    fn stamp_sid_reader_returns_only_a_uuid_shaped_last_sid() {
        let text = format!("{}\n{}\n", sid_line(Event::SessionStart, "sid-0"), sid_line(Event::UserPromptSubmit, UUID));
        assert_eq!(last_sid(&text).as_deref(), Some(UUID), "最終行の UUID");
        assert_eq!(last_sid(&format!("{text}not json\n")).as_deref(), Some(UUID), "壊れた行は最終行に数えない");
        let upper = UUID.to_ascii_uppercase();
        assert_eq!(last_sid(&sid_line(Event::Stop, &upper)).as_deref(), Some(upper.as_str()), "16 進の大文字も形の内");
        for bad in [
            "",
            "sid-1",
            "0b7c3f5e-9a1d-4c2e-8f6a-1d2e3f4a5b6",
            "0b7c3f5e-9a1d-4c2e-8f6a-1d2e3f4a5b6c7",
            "0b7c3f5e9a1d-4c2e-8f6a-1d2e-3f4a5b6c",
            "0b7c3f5e-9a1d-4c2e-8f6a-1d2e3f4a5b6g",
            "0b7c3f5e-9a1d-4c2e-8f6a-1d2e3f4a5b6c-",
            "0b7c3f5e-9a1d-4c2e-8f6a-1d2e3f4a5b6c ",
            "0b7c3f5e-9a1d-4c2e-8f6a-1d2e3f4a5b; x",
        ] {
            let after = format!("{}\n", sid_line(Event::Stop, bad));
            assert_eq!(last_sid(&format!("{text}{after}")), None, "{bad:?}: 最終行が形違いなら前の行に倒れない");
        }
        assert_eq!(last_sid(""), None, "空の本文");
        assert_eq!(last_sid("not json\n"), None, "読める行が無い");
    }

    /// carry は `--resume <sid>` の 2 語か空: 打刻 file の最終行が UUID なら 2 語・形違いと file 無しは空。
    #[test]
    fn stamp_sid_carry_is_resume_and_the_sid_or_empty() {
        let dir = std::env::temp_dir().join(format!("stamp-sid-carry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        assert_eq!(resume_carry(&dir), Vec::<String>::new(), "file 無し");
        let _ = std::fs::write(super::path(&dir), format!("{}\n", sid_line(Event::UserPromptSubmit, UUID)));
        assert_eq!(resume_carry(&dir), [RESUME_FLAG.to_owned(), UUID.to_owned()], "最終行の UUID を運ぶ");
        assert_eq!(RESUME_FLAG, "--resume", "flag の字面");
        let _ = std::fs::write(super::path(&dir), format!("{}\n", sid_line(Event::UserPromptSubmit, "sid-1")));
        assert_eq!(resume_carry(&dir), Vec::<String>::new(), "形違いは運ばない");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
