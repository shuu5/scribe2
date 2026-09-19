//! 圧縮の直前の 1 枠（設計 seat-roles.md §22・FR42 / FR19・`s2-07l.489`）: `PreCompact` が transcript の**末尾**から
//! 席の直近の発言（assistant の text block）を逐語で抜いて席の置き場の **1 枠**（file 1 つ・上書き）に書き、
//! `source = compact` の SessionStart が `[PRECOMPACT]` の 1 行と抜いた文を **1 回だけ**出して枠を消す。
//!
//! §21 の DATA は台帳と git に**書かれた後**の事実しか持たないので、自動圧縮が手番の途中で走ると「いま何を
//! している途中だったか」が落ちる。hook は LLM を呼べない（shell の command）ので、できるのは機械の記録だけ
//! である。抜くのは席の自分の発言だけ（tool の出力・user の発言は抜かない＝機微の混入の面を狭める）。枠は
//! 置き場（repo の外）にだけ在り、repo には 1 byte も書かない。
//!
//! 極性は書く側・読む側とも **guard ではない**（[`WRITE_POLARITY`] / [`READ_POLARITY`]・`polarity::NOT_A_GUARD`
//! の 2 行）: PreCompact は何が起きても圧縮を止めず（rc 0・stdout 0 byte）、SessionStart は枠が無い・読めない周に
//! `[PRECOMPACT]` を出さないだけで他は出す。transcript は末尾の [`TAIL_BYTES`] だけを読み（全読しない）、JSON
//! として読めない行は読み飛ばし、assistant の text が 1 つも取れない周は枠を書かない（空の枠を作らない）。

use crate::fleet::cli::format_utc;
use crate::fleet::json_tree::{self, Tree};
use crate::polarity::{OnFailure, Polarity, Timing};
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};

/// 書く側の極性（[`Skip`]）: transcript を読めない・text が無い・書けない周も圧縮を止めない（rc 0・stdout 0 byte・
/// 失敗は stderr 1 行と記録 1 行）。行為を止める判定ではないので Guard ではない（`polarity::NOT_A_GUARD`）。
pub const WRITE_POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailOpen,
};

/// 読む側の極性（[`Read`]）: 枠が無い・読めない周は `[PRECOMPACT]` を出さないだけで名乗り・指示文・DATA は出す。
/// 同じく Guard ではない（`polarity::NOT_A_GUARD`）。
pub const READ_POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailOpen,
};

/// 枠の file 名（`<state_dir>/seat/<target>/precompact`）。
pub const FILE: &str = "precompact";
/// 枠の schema 版。
pub const SCHEMA: u64 = 1;
/// SessionStart が出す 1 行の行頭の marker。
pub const MARKER: &str = "[PRECOMPACT]";
/// transcript の末尾から読む byte 数（全読しない・表示の幅であって閾値ではない＝rules 行を足さない）。
pub const TAIL_BYTES: u64 = 256 * 1024;
/// 抜いた文を切る幅（文字）。
pub const TEXT_WIDTH: usize = 2_000;
/// `PreCompact` の payload から拾う key（`manual` / `auto`）。
pub const KEY_TRIGGER: &str = "trigger";
/// `PreCompact` の payload から拾う key（transcript の path）。
pub const KEY_TRANSCRIPT: &str = "transcript_path";
/// `SessionStart` の payload から拾う key（`startup` / `resume` / `clear` / `compact`）。
pub const KEY_SOURCE: &str = "source";
/// 枠を読む周の `source` の字面。
pub const SOURCE_COMPACT: &str = "compact";
/// transcript の 1 行の `type`（席の発言）。
const TYPE_ASSISTANT: &str = "assistant";
/// transcript の 1 行の key（行の種類）。
const KEY_TYPE: &str = "type";
/// transcript の 1 行の key（message 本体）。
const KEY_MESSAGE: &str = "message";
/// message の key（content block の配列）。
const KEY_CONTENT: &str = "content";
/// content block の `type`（text block）。
const TYPE_TEXT: &str = "text";
/// text block の key（本文）。
const KEY_TEXT: &str = "text";
/// trigger が無い・空の周の字面（推測で埋めない・C10）。
const ABSENT: &str = "-";

/// 枠を書かなかった理由（**境界の enum**・[`WRITE_POLARITY`]・記録の `what` と stderr の字面は [`Skip::as_str`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// payload に `transcript_path` が無い。
    NoTranscript,
    /// transcript を読めない（無い・権限・読みの失敗）。
    Unreadable,
    /// 末尾の読みの範囲に assistant の text block が 1 つも無い。
    NoText,
}

/// [`Skip`] の全 variant（宣言順）。
pub const SKIPS: &[Skip] = &[Skip::NoTranscript, Skip::Unreadable, Skip::NoText];

impl Skip {
    /// 記録と stderr に出す字面（kebab・閉じた列）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoTranscript => "no-transcript",
            Self::Unreadable => "transcript-unreadable",
            Self::NoText => "no-text",
        }
    }

    /// 席へ surface する側か（読めないのは失敗＝stderr 1 行・無いのは失敗ではない＝黙る）。
    pub fn surfaces(self) -> bool {
        matches!(self, Self::Unreadable)
    }
}

/// 枠の読み（**無い**と**読めない**を分ける・C10.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// 枠が在って読めた。
    Slot(Slot),
    /// 枠が無い（圧縮の直前に書かれていない・既に出した）。
    Absent,
    /// 在るが読めない・形が違う（不在に潰さない）。
    Unreadable,
}

/// 枠の中身（時刻・trigger・抜いた文・切る前の長さ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// payload の `trigger` の字面（空白と制御文字は `_`・無ければ [`ABSENT`]）。
    pub trigger: String,
    /// 書いた時刻（1970 年からの秒・UTC・記録の `ts` と同じ時計）。
    pub ts: u64,
    /// 切る前の文字数（切った事実は `shown < total` が持つ）。
    pub total: usize,
    /// 抜いた文（逐語・[`TEXT_WIDTH`] で切った後）。
    pub text: String,
}

impl Slot {
    /// 抜いた文から枠を組む（[`TEXT_WIDTH`] を超える文は切る・切った事実は `total` が持つ）。
    pub fn capture(trigger: &str, text: &str, ts: u64) -> Self {
        let total = text.chars().count();
        let kept: String = text.chars().take(TEXT_WIDTH).collect();
        Self { trigger: fold_trigger(trigger), ts, total, text: kept }
    }

    /// 出す文字数（切った後）。
    pub fn shown(&self) -> usize {
        self.text.chars().count()
    }

    /// 切ったか。
    pub fn cut(&self) -> bool {
        self.shown() < self.total
    }

    /// SessionStart が出す 1 行（`[PRECOMPACT] trigger=<字面> ts=<時刻> lines=<行数>`・切った周は
    /// ` cut=<shown>/<total>` が続く＝切った事実が行に出る）。
    pub fn header(&self) -> String {
        let lines = self.text.lines().count();
        let mut line = format!("{MARKER} trigger={} ts={} lines={lines}", self.trigger, format_utc(self.ts));
        if self.cut() {
            line.push_str(&format!(" cut={}/{}", self.shown(), self.total));
        }
        line
    }

    /// SessionStart が出す行（1 行目が [`Self::header`]・続きが抜いた文の逐語）。
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![self.header()];
        lines.extend(self.text.lines().map(str::to_owned));
        lines
    }

    /// 枠の本文（1 行目 `schema=1 trigger=<t> ts=<秒> total=<n>`・2 行目以降が抜いた文の逐語）。
    pub fn to_text(&self) -> String {
        format!("schema={SCHEMA} trigger={} ts={} total={}\n{}", self.trigger, self.ts, self.total, self.text)
    }

    /// 枠の本文を読む（[`Self::to_text`] の逆・key の順序は固定・別 schema と壊れた 1 行目は `None`）。
    pub fn parse(body: &str) -> Option<Self> {
        let (head, text) = body.split_once('\n')?;
        let rest = head.strip_prefix(&format!("schema={SCHEMA} trigger="))?;
        let (trigger, rest) = rest.split_once(" ts=")?;
        let (ts, total) = rest.split_once(" total=")?;
        Some(Self {
            trigger: trigger.to_owned(),
            ts: ts.parse().ok()?,
            total: total.parse().ok()?,
            text: text.to_owned(),
        })
    }
}

/// trigger の字面を 1 語に畳む（空白・制御文字は `_`・空は [`ABSENT`]）＝header の `trigger=` が次の key を偽装しない。
fn fold_trigger(trigger: &str) -> String {
    let fold = |c: char| if c.is_whitespace() || c.is_control() { '_' } else { c };
    let folded: String = trigger.trim().chars().map(fold).collect();
    if folded.is_empty() {
        ABSENT.to_owned()
    } else {
        folded
    }
}

/// 枠の path。
pub fn slot_path(seat_dir: &Path) -> PathBuf {
    seat_dir.join(FILE)
}

/// transcript の末尾 [`TAIL_BYTES`] を読む（途中から読んだ周は先頭の欠けた行を捨てる）。
pub fn tail_of(transcript: &Path) -> Result<String, Skip> {
    let mut file = std::fs::File::open(transcript).map_err(|_| Skip::Unreadable)?;
    let len = file.metadata().map_err(|_| Skip::Unreadable)?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).map_err(|_| Skip::Unreadable)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| Skip::Unreadable)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start == 0 {
        return Ok(text);
    }
    Ok(text.split_once('\n').map_or_else(String::new, |(_, rest)| rest.to_owned()))
}

/// transcript の 1 行から、席の発言の**最後の** text block を抜く（`type == assistant` で `message.content` の
/// 配列に `type == text` の block が在る行だけ・空白だけの text は無いと見る・JSON として読めない行は `None`）。
pub fn text_of_line(line: &str) -> Option<String> {
    let tree = json_tree::parse(line).ok()?;
    if tree.get(KEY_TYPE).and_then(Tree::as_str) != Some(TYPE_ASSISTANT) {
        return None;
    }
    let content = tree.get(KEY_MESSAGE)?.get(KEY_CONTENT)?;
    let blocks = content.as_array()?;
    blocks
        .iter()
        .rev()
        .filter(|block| block.get(KEY_TYPE).and_then(Tree::as_str) == Some(TYPE_TEXT))
        .find_map(|block| block.get(KEY_TEXT).and_then(Tree::as_str))
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
}

/// transcript の末尾から直近の席の発言を抜く（末尾の [`TAIL_BYTES`] の中を後ろから走査）。
pub fn last_text(transcript: Option<&Path>) -> Result<String, Skip> {
    let tail = tail_of(transcript.ok_or(Skip::NoTranscript)?)?;
    tail.lines().rev().find_map(text_of_line).ok_or(Skip::NoText)
}

/// 枠を書く（上書き・打刻 dir が無ければ作る）。書けない周は理由の 1 行（呼び手が stderr に載せる）。
pub fn write(seat_dir: &Path, slot: &Slot) -> Result<(), String> {
    std::fs::create_dir_all(seat_dir).map_err(|err| format!("{} を作れない: {err}", seat_dir.display()))?;
    let path = slot_path(seat_dir);
    std::fs::write(&path, slot.to_text()).map_err(|err| format!("{} を書けない: {err}", path.display()))
}

/// 枠を読む。**無い**（NotFound）だけが [`Read::Absent`]・他の失敗と読めない形は [`Read::Unreadable`]。
pub fn read(seat_dir: &Path) -> Read {
    match std::fs::read_to_string(slot_path(seat_dir)) {
        Ok(text) => Slot::parse(&text).map_or(Read::Unreadable, Read::Slot),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Read::Absent,
        Err(_) => Read::Unreadable,
    }
}

/// 枠を消す（出した後・読めない枠も消す＝古い枠が次の圧縮で化けない）。無い周も `Ok`。
pub fn remove(seat_dir: &Path) -> Result<(), String> {
    let path = slot_path(seat_dir);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("{} を消せない: {err}", path.display())),
    }
}
