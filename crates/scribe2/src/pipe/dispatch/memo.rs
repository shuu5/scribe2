//! memo の引き金の満ちと審査の置き場（設計 docs/design/dispatcher.md §40・契約表の行 ao・FR87 / FR91・ADR-0085 / ADR-0089）。
//!
//! 列の 1 周の読み（[`super::Read`]）から、開いた memo（最後の昇格の行が「全部」の memo を除く）ごとに引き金の満ちを判じて
//! `[DISPATCH-MEMO]` の 1 行にする。満ちの判定は [`trigger::met`] を呼ぶだけで写しを持たず、台帳も event log も読み直さない。
//! 審査の置き場 `<state_dir>/pipe/memo/<memo id>/` の形（[`FIRED`]・[`PID`]・[`RC`]・[`OUT`]・[`VERDICT`]）と `verdict` の読み
//! （[`judgement`]）もここに置く——書き手（行 ap）・起こす側（行 aq）・通知（行 ar）が同じ 1 本を呼ぶ。

use super::candidates::pointer_of;
use super::{Input, Read, CLOSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::epoch_of;
use crate::ledger::form::{is_memo, is_question};
use crate::ledger::promotion::{self, Promotion, Scope};
use crate::ledger::trigger::{self, Kind, Reason, Reading, Trigger, World};
use crate::pipe::cli::generated;
use crate::pipe::table::Pointer;
use crate::seat::brief::pointer::Anchor;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// memo の行の書き出し。
pub(super) const HEAD: &str = "[DISPATCH-MEMO]";

/// 置き場の下の dir（`<state>/pipe/memo`・memo id ごとに 1 dir）。
const DIR: [&str; 2] = ["pipe", "memo"];

/// 起こした時刻の file の名。
pub const FIRED: &str = "fired";

/// 撃ち中の印（`<pid> <起動時刻>`）の file の名。
pub const PID: &str = "pid";

/// lens の rc の file の名。
pub const RC: &str = "rc";

/// lens の出力の file の名。
pub const OUT: &str = "out";

/// 器が出力を読んだ 1 行の判定の file の名。
pub const VERDICT: &str = "verdict";

/// 値を持たない欄の字面。
const DASH: &str = "-";

/// 判定の語（閉じた 4 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    /// 契約へ上げる。
    Promote,
    /// 閉じる。
    Close,
    /// 残す。
    Keep,
    /// 出力を読めなかった。
    Unparsed,
}

impl Word {
    /// 4 値（宣言順）。
    pub const ALL: [Self; 4] = [Self::Promote, Self::Close, Self::Keep, Self::Unparsed];

    /// file と行に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Promote => "promote",
            Self::Close => "close",
            Self::Keep => "keep",
            Self::Unparsed => "unparsed",
        }
    }
}

/// `verdict` の 1 行の中身。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// 判定の語。
    pub word: Word,
    /// 判定の時刻（字のまま）。
    pub at: String,
    /// 根拠。
    pub evidence: String,
    /// 契約の素描。
    pub sketch: String,
}

impl Verdict {
    /// file に書く 1 行（`{"verdict":…,"at":…,"evidence":…,"sketch":…}`）。
    pub fn to_line(&self) -> String {
        json_lite::write_object(&[
            ("verdict", Value::Str(self.word.as_str().to_owned())),
            ("at", Value::Str(self.at.clone())),
            ("evidence", Value::Str(self.evidence.clone())),
            ("sketch", Value::Str(self.sketch.clone())),
        ])
    }

    /// file の字を読む（4 key が文字列で揃い、語が閉じた 4 値の内なら `Some`）。
    pub fn parse(text: &str) -> Option<Self> {
        let pairs = json_lite::parse_object(text.trim()).ok()?;
        let get = |key: &str| pairs.iter().find(|(found, _)| found == key).and_then(|(_, value)| value.as_str());
        let word = Word::ALL.into_iter().find(|found| Some(found.as_str()) == get("verdict"))?;
        Some(Self { word, at: get("at")?.to_owned(), evidence: get("evidence")?.to_owned(), sketch: get("sketch")?.to_owned() })
    }
}

/// 置き場の最新の判定の読み。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgement {
    /// 置き場か `verdict` の file が無い。
    Absent,
    /// file は在るが読めない（形が合わない・読めない）。
    Unreadable,
    /// 読めた判定。
    Judged(Verdict),
}

/// memo 1 つの置き場の dir（`<state>/pipe/memo/<memo id>`）。
pub fn dir(state_dir: &Path, memo: &str) -> PathBuf {
    DIR.iter().fold(state_dir.to_path_buf(), |path, step| path.join(step)).join(memo)
}

/// memo 1 つの最新の判定を読む（**読みの 1 本**・置き場の外へ出る id は読めない側）。
pub fn judgement(state_dir: &Path, memo: &str) -> Judgement {
    if memo.is_empty() || memo.contains(['/', '\\']) || memo == ".." || memo == "." {
        return Judgement::Unreadable;
    }
    match std::fs::read_to_string(dir(state_dir, memo).join(VERDICT)) {
        Ok(text) => Verdict::parse(&text).map_or(Judgement::Unreadable, Judgement::Judged),
        Err(error) if error.kind() == ErrorKind::NotFound => Judgement::Absent,
        Err(_) => Judgement::Unreadable,
    }
}

/// 母集団の memo ごとの `[DISPATCH-MEMO]` の行（bead id の字の順）。`now` は周の時刻（UNIX 秒）。
///
/// 母集団は開いた memo から最後の昇格の行が「全部」の memo を除いた全部（「一部」は含む・FR91）。
pub(super) fn lines(input: &Input<'_>, read: &Read, now: u64) -> Vec<String> {
    let prefix = Anchor::open(input.repo).and_then(|anchor| anchor.prefixes().first().cloned());
    triggers(input, read, now, |_, notes| !fully_promoted(notes, prefix.as_deref()))
        .into_iter()
        .map(|(id, trigger, created)| {
            let (verdict, judged) = match judgement(input.state_dir, id) {
                Judgement::Absent => (DASH, DASH.to_owned()),
                Judgement::Unreadable => ("unreadable", DASH.to_owned()),
                Judgement::Judged(found) => (found.word.as_str(), found.at),
            };
            let age = created.map_or(DASH.to_owned(), |at| format!("{}h", now.saturating_sub(at) / 3600));
            format!("{HEAD} memo={id} trigger={trigger} verdict={verdict} age={age} judged={judged}")
        })
        .collect()
}

/// 開いた memo 1 本の引き金の欄の値（[`lines`] と同じ World の組み立てと [`trigger_of`] を通した同じ字・開いた memo でなければ `None`）。
pub(super) fn trigger_value(input: &Input<'_>, read: &Read, now: u64, memo: &str) -> Option<String> {
    triggers(input, read, now, |id, _| id == memo).into_iter().next().map(|(_, trigger, _)| trigger)
}

/// `pick` が選んだ開いた memo ごとの（id・引き金の欄の値・作られた時刻）を bead id の字の順に返す（世界の 5 入力は `read` の 1 回の読みから組む）。
fn triggers<'a>(
    input: &Input<'_>,
    read: &'a Read,
    now: u64,
    pick: impl Fn(&str, &str) -> bool,
) -> Vec<(&'a str, String, Option<u64>)> {
    let prefix = Anchor::open(input.repo).and_then(|anchor| anchor.prefixes().first().cloned());
    let mut memos: Vec<_> = read.issues.iter().filter(|issue| is_memo(issue) && issue.status != CLOSED).collect();
    memos.sort_by(|left, right| left.id.cmp(&right.id));
    let rows: Vec<(&str, Reading, Option<u64>)> = memos
        .into_iter()
        .filter(|issue| pick(&issue.id, &issue.notes))
        .map(|issue| {
            let reading = trigger::read(&issue.description, &issue.notes, prefix.as_deref());
            (issue.id.as_str(), reading, issue.created_at.as_deref().and_then(epoch_at))
        })
        .collect();
    let closed: Vec<String> = read.issues.iter().filter(|issue| issue.status == CLOSED).map(|issue| issue.id.clone()).collect();
    let closed_pointers: Vec<Pointer> =
        read.issues.iter().filter(|issue| issue.status == CLOSED).filter_map(|issue| pointer_of(&issue.acceptance)).collect();
    // 開いた契約の write-set は同梱の引き金が 1 本でも在る周だけ導く（生成は候補と同じ材料の 1 回の読みを借りる）。
    let bundled = rows.iter().any(|(_, reading, _)| reading.readable().any(|found| matches!(found, Trigger::Bundle(_))));
    let write_set = if bundled { open_write_set(input.repo, read) } else { Vec::new() };
    let world = World { recurrences: 0, write_set: &write_set, closed: &closed, closed_pointers: &closed_pointers, now };
    rows.iter().map(|(id, reading, created)| (*id, trigger_of(reading, &World { recurrences: reading.recurrences, ..world }), *created)).collect()
}

/// 最後の昇格の行が読めて範囲が「全部」か（読めない行と行の無い notes は母集団に残す）。
fn fully_promoted(notes: &str, prefix: Option<&str>) -> bool {
    matches!(
        promotion::read(notes, prefix.unwrap_or_default()),
        Some(promotion::Line::Readable(Promotion { scope: Scope::All, .. }))
    )
}

/// 引き金の欄の値（満ちた形を宣言順に・満ちなければ `unmet`・読める行が無ければ読めない理由）。
fn trigger_of(reading: &Reading, world: &World<'_>) -> String {
    let met: Vec<&str> = Kind::ALL
        .into_iter()
        .filter(|kind| reading.readable().any(|found| found.kind() == *kind && trigger::met(found, world)))
        .map(Kind::as_str)
        .collect();
    if !met.is_empty() {
        return format!("met:{}", met.join(","));
    }
    if reading.readable().next().is_some() {
        return "unmet".to_owned();
    }
    format!("unreadable:{}", reading.first_unreadable().map_or("none", |(_, reason)| reason_word(reason)))
}

/// 読めない引き金の行の理由の語（空白を持たない・行の key に写せる字）。
fn reason_word(reason: Reason) -> &'static str {
    match reason {
        Reason::Words => "words",
        Reason::Kind => "kind",
        Reason::Value(_) => "value",
        Reason::NoPrefix => "no-prefix",
    }
}

/// 閉じていない契約（acceptance の設計 pointer が行を指す bead）の write-set の項目（材料を読めない行は数えない）。
fn open_write_set(repo: &Path, read: &Read) -> Vec<String> {
    let Ok(materials) = read.materials.as_ref() else {
        return Vec::new();
    };
    let mut pointers: Vec<Pointer> = Vec::new();
    for pointer in read
        .issues
        .iter()
        .filter(|issue| issue.status != CLOSED && !is_memo(issue) && !is_question(issue))
        .filter_map(|issue| pointer_of(&issue.acceptance))
    {
        if !pointers.contains(&pointer) {
            pointers.push(pointer);
        }
    }
    // 契約の生成は受付と同じ 1 本（読めない行は write-set を持たない側に倒す）。
    pointers.iter().filter_map(|pointer| generated(repo, pointer, materials).ok()).flat_map(|(contract, _)| contract.write_set).collect()
}

/// 台帳の時刻の字（秒の小数を持つ形も許す）を UNIX 秒へ。
fn epoch_at(text: &str) -> Option<u64> {
    epoch_of(text).or_else(|| {
        let (head, tail) = text.split_once('.')?;
        let digits = tail.strip_suffix('Z')?;
        (!digits.is_empty() && digits.bytes().all(|found| found.is_ascii_digit())).then(|| epoch_of(&format!("{head}Z")))?
    })
}
