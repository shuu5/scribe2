//! 席の実口座の記録（設計 account-lifecycle.md §16 形 (1) / (2)・契約表の行 d・FR42 / FR40）。
//!
//! `session-start` が入力 JSON の `transcript_path`（transcript は credential dir の下に置かれる）から実口座を導き、
//! 席の打刻 dir（読み込み元の記録 `plugin` と同じ置き場）へ **`account` 1 file 1 行**を毎 SessionStart に上書きする
//! （unknown も書く＝前の session の値を残さない）。label を得るのは path が字面で `<state_dir>/accounts/<label>/` の下に在り、
//! `<label>` が 1 要素で `.` 始まりでない周だけで、それ以外・key が無い周は unknown。**env は読まない**（C2.2）。
//!
//! 登録 row（`SeatRegistered` の account）と `state.jsonl` は書かない（row は宣言から導いた値の写し・C10）。読む側は
//! 「在る / 無い / 読めない」を型で分ける（[`AccountRecord`]・`hook/vessel/digest.rs` の `PluginRecord` と同型）。記録を読んで
//! 合図を撃つ・席を止める経路は置かない（ADR-0045 §2 (2)・FR27 / FR38 の恒常の不在）。

use super::cycle::ACCOUNTS_DIR;
use std::path::{Component, Path, PathBuf};

/// 記録 file の名前（`<state_dir>/seat/<target>/account`）。
pub const FILE: &str = "account";
/// 記録の schema 版。
pub const SCHEMA: u64 = 1;
/// 実口座を導けない周の `account=` の字面。
pub const UNKNOWN: &str = "unknown";

/// 記録 file の path。
pub fn record_path(seat_dir: &Path) -> PathBuf {
    seat_dir.join(FILE)
}

/// 測った実口座（closed）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAccount {
    /// transcript が `<state_dir>/accounts/<label>/` の下に在った。
    Label(String),
    /// 導けない（置き場の accounts の外・`.` 始まり・key 無し）。
    Unknown,
}

impl SessionAccount {
    /// transcript の path から実口座を導く（pure・字面だけ・実体の path は解かない）。`transcript` が `None`（key 無し）・
    /// `<state_dir>/accounts/` の下でない・`<label>` の直下に要素が無い・`.` 始まり・`..` などの非通常要素を含む周は
    /// [`Self::Unknown`]。
    pub fn of_transcript(state_dir: &Path, transcript: Option<&Path>) -> Self {
        let Some(rest) = transcript.and_then(|path| path.strip_prefix(state_dir.join(ACCOUNTS_DIR)).ok()) else {
            return Self::Unknown;
        };
        let mut parts = Vec::new();
        for part in rest.components() {
            let Component::Normal(name) = part else {
                return Self::Unknown;
            };
            parts.push(name);
        }
        match parts.split_first() {
            Some((label, below)) if !below.is_empty() => match label.to_str() {
                Some(found) if !found.is_empty() && !found.starts_with('.') => Self::Label(found.to_owned()),
                _ => Self::Unknown,
            },
            _ => Self::Unknown,
        }
    }

    /// `account=` の字面（label か `unknown`）。
    pub fn as_str(&self) -> &str {
        match self {
            Self::Label(label) => label,
            Self::Unknown => UNKNOWN,
        }
    }

    /// 字面から戻す（`unknown` だけが [`Self::Unknown`]・空は読めない）。
    fn from_word(word: &str) -> Option<Self> {
        match word {
            "" => None,
            UNKNOWN => Some(Self::Unknown),
            label => Some(Self::Label(label.to_owned())),
        }
    }
}

/// 実口座の記録（読み・closed）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountRecord {
    /// 記録が在って読めた。
    Recorded {
        /// 測った実口座。
        account: SessionAccount,
        /// session id。
        sid: String,
        /// 書いた時刻（1970 年からの秒・UTC）。
        ts: u64,
    },
    /// file が無い（記録の不在）。
    Absent,
    /// 在るが読めない・形が違う（不在に潰さない）。
    Unreadable,
}

impl AccountRecord {
    /// 1 行の字面（`schema=1 sid=<sid> account=<label|unknown> ts=<秒>`）。`Absent` / `Unreadable` は書く形を持たない（`None`）。
    pub fn to_line(&self) -> Option<String> {
        let Self::Recorded { account, sid, ts } = self else {
            return None;
        };
        Some(format!("schema={SCHEMA} sid={sid} account={} ts={ts}", account.as_str()))
    }

    /// 1 行を読む（key の順序は固定・`ts` は末尾から切る）。
    fn from_line(line: &str) -> Option<Self> {
        let rest = line.strip_prefix(&format!("schema={SCHEMA} sid="))?;
        let (rest, ts) = rest.rsplit_once(" ts=")?;
        let ts = ts.parse::<u64>().ok()?;
        let (sid, account) = rest.rsplit_once(" account=")?;
        let account = SessionAccount::from_word(account)?;
        Some(Self::Recorded { account, sid: sid.to_owned(), ts })
    }

    /// 席の打刻 dir の記録を読む。**無い**（NotFound）だけが [`Self::Absent`]・他の失敗と 1 行でない形は [`Self::Unreadable`]。
    pub fn read(seat_dir: &Path) -> Self {
        let text = match std::fs::read_to_string(record_path(seat_dir)) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::Absent,
            Err(_) => return Self::Unreadable,
        };
        let mut lines = text.lines();
        match (lines.next(), lines.next()) {
            (Some(line), None) => Self::from_line(line).unwrap_or(Self::Unreadable),
            _ => Self::Unreadable,
        }
    }
}

/// 記録を 1 行で書く（上書き・毎 SessionStart・unknown も書く）。打刻 dir が無ければ作る。書けない周は理由の 1 行
/// （席は止めない＝呼び手が stderr に載せる）。
pub fn write(seat_dir: &Path, account: SessionAccount, sid: &str) -> Result<(), String> {
    let record = AccountRecord::Recorded { account, sid: sid.to_owned(), ts: super::state::now_secs() };
    let line = record.to_line().ok_or_else(|| "記録の形が無い".to_owned())?;
    std::fs::create_dir_all(seat_dir).map_err(|err| format!("{} を作れない: {err}", seat_dir.display()))?;
    let path = record_path(seat_dir);
    std::fs::write(&path, format!("{line}\n")).map_err(|err| format!("{} を書けない: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{AccountRecord, SessionAccount};
    use std::path::Path;

    /// 導出の極性: accounts の下の 1 要素の label だけが Label、外・`.` 始まり・直下に要素が無い・`..`・key 無しは Unknown。
    #[test]
    fn seat_account_mismatch_record_derives_the_label_from_the_path_lexically() {
        let state = Path::new("/s/state");
        let of = |path: &str| SessionAccount::of_transcript(state, Some(Path::new(path)));
        assert_eq!(of("/s/state/accounts/a1/projects/x/t.jsonl"), SessionAccount::Label("a1".to_owned()));
        assert_eq!(of("/s/state/accounts/a1/t.jsonl"), SessionAccount::Label("a1".to_owned()));
        for unknown in [
            "/s/other/accounts/a1/t.jsonl",
            "/s/state/accounts/.hidden/t.jsonl",
            "/s/state/accounts/a1",
            "/s/state/accounts/../accounts/a1/t.jsonl",
            "/s/state/accounts/a1/../../x/t.jsonl",
            "/s/state/accountsx/a1/t.jsonl",
            "/s/state/t.jsonl",
        ] {
            assert_eq!(of(unknown), SessionAccount::Unknown, "{unknown}");
        }
        assert_eq!(SessionAccount::of_transcript(state, None), SessionAccount::Unknown, "key 無し");
    }

    /// 1 行の書き / 読みが往復し（label・unknown）、壊れた行・別 schema・空の account は読めない。
    #[test]
    fn seat_account_mismatch_record_line_round_trips_and_refuses_broken_forms() {
        for account in [SessionAccount::Label("a b".to_owned()), SessionAccount::Unknown] {
            let record = AccountRecord::Recorded { account, sid: "sid-1".to_owned(), ts: 7 };
            let line = record.to_line().unwrap_or_default();
            assert_eq!(AccountRecord::from_line(&line), Some(record), "{line}");
        }
        let unknown = AccountRecord::Recorded { account: SessionAccount::Unknown, sid: "s".to_owned(), ts: 1 };
        assert_eq!(unknown.to_line().as_deref(), Some("schema=1 sid=s account=unknown ts=1"));
        assert_eq!(AccountRecord::Absent.to_line(), None);
        for broken in ["schema=2 sid=s account=a ts=1", "schema=1 sid=s account=a ts=x", "schema=1 sid=s account= ts=1", ""] {
            assert_eq!(AccountRecord::from_line(broken), None, "{broken:?}");
        }
    }
}
