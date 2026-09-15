//! hook 集合の digest と読み込み元の記録（設計 consumer-sync.md §3・ADR-0028 §2.2・FR61）。
//!
//! `session-start` が `--plugin-root`（生成 hooks.json の shell 行が `$CLAUDE_PLUGIN_ROOT` から渡す＝器は env を
//! 読まない・C2.2）を受けた周に、席の打刻 dir（seat-state.md §2）へ **`plugin` 1 file 1 行**を書く（毎 SessionStart
//! に上書き＝最新 session の値）。digest は `<root>/hooks/hooks.json` の bytes の **FNV-1a 64**（16 hex・std だけ・
//! persist する値に `DefaultHasher` は使わない〔std の hasher は版で変わりうる〕）。読む側は「無い」と「読めない」を
//! 型で分ける（[`PluginRecord`]・C10.2）。書く側も読む側も判定しない（食い違いを名指すのは doctor・§4）。

use std::path::{Path, PathBuf};

/// 記録 file の名前（`<state_dir>/seat/<target>/plugin`）。
pub const FILE: &str = "plugin";
/// 記録の schema 版。
pub const SCHEMA: u64 = 1;
/// hooks.json を読めない周の `hooks=` の字面（記録はする＝doctor が名指す）。
pub const UNREADABLE: &str = "unreadable";
/// plugin root から hooks.json への相対 path（生成物 `hooks/hooks.json`）。
const HOOKS_DIR: &str = "hooks";
/// 同上（file 名）。
const HOOKS_FILE: &str = "hooks.json";
/// FNV-1a 64 の offset basis。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 の prime。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64（16 hex・小文字）。pure・依存なし・release を跨いで不変。
pub fn fnv1a_64(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(FNV_OFFSET, |acc, byte| (acc ^ u64::from(*byte)).wrapping_mul(FNV_PRIME));
    format!("{hash:016x}")
}

/// `<root>/hooks/hooks.json` の path。
pub fn hooks_path(root: &Path) -> PathBuf {
    root.join(HOOKS_DIR).join(HOOKS_FILE)
}

/// `<root>/hooks/hooks.json` の digest。無い・読めない周は `None`（推測で埋めない・C10）。
pub fn hooks_digest(root: &Path) -> Option<String> {
    std::fs::read(hooks_path(root)).ok().map(|bytes| fnv1a_64(&bytes))
}

/// 記録 file の path。
pub fn record_path(seat_dir: &Path) -> PathBuf {
    seat_dir.join(FILE)
}

/// 読み込み元の記録（読み・closed）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRecord {
    /// 記録が在って読めた。`hooks` は digest（hooks.json を読めなかった周は `None` = `unreadable`）。
    Recorded {
        /// plugin の root（hooks.json の在る場所・cache か作業ツリーかを器は推測しない）。
        root: String,
        /// `<root>/hooks/hooks.json` の digest。
        hooks: Option<String>,
        /// その session を起動した binary の build 元 commit。
        binary: String,
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

impl PluginRecord {
    /// 1 行の字面（`schema=1 sid=<sid> root=<root> hooks=<digest|unreadable> binary=<sha> ts=<秒>`）。
    /// `Absent` / `Unreadable` は書く形を持たない（`None`）。
    pub fn to_line(&self) -> Option<String> {
        let Self::Recorded { root, hooks, binary, sid, ts } = self else {
            return None;
        };
        let hooks = hooks.as_deref().unwrap_or(UNREADABLE);
        Some(format!("schema={SCHEMA} sid={sid} root={root} hooks={hooks} binary={binary} ts={ts}"))
    }

    /// 1 行を読む（key の順序は固定・`root` は空白を含みうるので次の key の字面で切る）。
    fn from_line(line: &str) -> Option<Self> {
        let rest = line.strip_prefix(&format!("schema={SCHEMA} sid="))?;
        let (sid, rest) = rest.split_once(" root=")?;
        let (root, rest) = rest.split_once(" hooks=")?;
        let (hooks, rest) = rest.split_once(" binary=")?;
        let (binary, ts) = rest.split_once(" ts=")?;
        let ts = ts.parse::<u64>().ok()?;
        let hooks = (hooks != UNREADABLE).then(|| hooks.to_owned());
        Some(Self::Recorded { root: root.to_owned(), hooks, binary: binary.to_owned(), sid: sid.to_owned(), ts })
    }

    /// 席の打刻 dir の記録を読む。**無い**（NotFound）だけが [`Self::Absent`]・他の失敗と 1 行でない形は
    /// [`Self::Unreadable`]。
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

/// 記録を 1 行で書く（上書き・毎 SessionStart）。`binary` は呼び手の compile time の値（`env!`・C2.2）。
/// 打刻 dir が無ければ作る。書けない周は理由の 1 行（席は止めない＝呼び手が stderr に載せる）。
pub fn write(seat_dir: &Path, root: &Path, sid: &str, binary: &str) -> Result<(), String> {
    let record = PluginRecord::Recorded {
        root: root.display().to_string(),
        hooks: hooks_digest(root),
        binary: binary.to_owned(),
        sid: sid.to_owned(),
        ts: crate::seat::state::now_secs(),
    };
    let line = record.to_line().ok_or_else(|| "記録の形が無い".to_owned())?;
    std::fs::create_dir_all(seat_dir).map_err(|err| format!("{} を作れない: {err}", seat_dir.display()))?;
    let path = record_path(seat_dir);
    std::fs::write(&path, format!("{line}\n")).map_err(|err| format!("{} を書けない: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{fnv1a_64, PluginRecord, UNREADABLE};

    /// FNV-1a 64 の既知値（空・`a`・`foobar`）。
    #[test]
    fn hook_plugin_record_fnv1a_64_matches_the_reference_vectors() {
        assert_eq!(fnv1a_64(b""), "cbf29ce484222325");
        assert_eq!(fnv1a_64(b"a"), "af63dc4c8601ec8c");
        assert_eq!(fnv1a_64(b"foobar"), "85944171f73967e8");
    }

    /// 1 行の書き / 読みが往復し、`root` の空白と `hooks=unreadable` を保つ。壊れた行・別 schema は読めない。
    #[test]
    fn hook_plugin_record_line_round_trips_and_refuses_broken_forms() {
        let record = PluginRecord::Recorded {
            root: "/a b/plugin".to_owned(),
            hooks: None,
            binary: "0123456789ab".to_owned(),
            sid: "sid-1".to_owned(),
            ts: 7,
        };
        let line = record.to_line().unwrap_or_default();
        assert_eq!(line, format!("schema=1 sid=sid-1 root=/a b/plugin hooks={UNREADABLE} binary=0123456789ab ts=7"));
        assert_eq!(PluginRecord::from_line(&line), Some(record));
        assert_eq!(PluginRecord::Absent.to_line(), None);
        for broken in ["schema=2 sid=s root=/r hooks=x binary=b ts=1", "schema=1 sid=s root=/r hooks=x binary=b ts=x", ""] {
            assert_eq!(PluginRecord::from_line(broken), None, "{broken:?}");
        }
    }
}
