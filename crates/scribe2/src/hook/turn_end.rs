//! turn の終わりの止め（設計 docs/design/dialogue-surface.md §12・ADR-0087・FR88 / NFR5）: `SessionStart` が session の置き場へ event log の
//! 長さを開始の位置として 1 度だけ書き（[`start`]）、`Stop` が台帳を読まずに開始の位置から log の末尾までを読んで、その session の
//! 未仕分けの発話（[`crate::utterance::sorted_of`] が [`Standing::Unsorted`] と判じたもの）のうち未告のものが在る周を 1 度だけ rc 2 で止める
//! （[`stop`]）。止めた周は打刻せず（席は Busy のまま）、続く再入で Idle に戻す。読めない周は止めず [`Reason`] の 1 語で
//! `TurnEndUnjudged` を記帳して今のまま打刻する。`EventKind` と `Case` の match の arm は書かない（`==` と構築だけ）。

use super::{field, stamp, KEY_SESSION_ID};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{cli as fleet_cli, Case, Event, EventKind};
use crate::name::NAME;
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::seat::state::Event as Stamped;
use crate::utterance::{sorted_of, Standing};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// この境界の極性: turn の終わりの時点で止め、読めない周は止めずに記帳して通す。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailOpen,
};

/// session の置き場の dir 名（`<state_dir>/session/<session_id>/`・跨版でない）。
const DIR: &str = "session";
/// 開始の位置の file 名（event log の長さの 10 進）。
const START_FILE: &str = "start";
/// 告げ済みの控えの file 名（1 行 1 記録）。
const TOLD_FILE: &str = "told";
/// session id の字の数の上限（file 名に使うので）。
const SESSION_MAX: usize = 128;

/// 判じられなかった理由（**閉じた 5 語**・`TurnEndUnjudged` の `reason`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// payload に session_id が無いか、path に使えない字を持つ。
    NoSession,
    /// 開始の位置が無い（この止めの着地の前に始まった session）。
    NoStart,
    /// event log を開始の位置から読めない。
    LogUnreadable,
    /// 告げ済みの控えを読めない。
    ToldUnreadable,
    /// 告げ済みの控えを書けない。
    ToldUnwritable,
}

/// [`Reason`] の全 variant（宣言順・`enum-slices` が集合完全性を測る）。
pub const REASONS: &[Reason] = &[
    Reason::NoSession,
    Reason::NoStart,
    Reason::LogUnreadable,
    Reason::ToldUnreadable,
    Reason::ToldUnwritable,
];

impl Reason {
    /// 記帳する語。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoSession => "no-session",
            Self::NoStart => "no-start",
            Self::LogUnreadable => "log-unreadable",
            Self::ToldUnreadable => "told-unreadable",
            Self::ToldUnwritable => "told-unwritable",
        }
    }

    /// 字面から引く。未知なら `None`。
    pub fn parse(text: &str) -> Option<Self> {
        REASONS.iter().copied().find(|found| found.as_str() == text)
    }

    /// 控えで重ねを判じられる語か（session を持ち、控えを読め・書ける周の 2 語）。ほかの 3 語は毎回記帳する。
    pub const fn keeps_record(self) -> bool {
        matches!(self, Self::NoStart | Self::LogUnreadable)
    }
}

/// 告げ済みの控えの 1 記録（1 行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    /// 止めの 1 行で告げた未仕分けの発話の ts。
    Told(String),
    /// 止めた（この行の後の再入で席を Idle に戻す）。
    Block,
    /// 止めの続きの終わりで席を Idle に戻した。
    Released,
    /// 控えを持てる語を記帳した。
    Unjudged(Reason),
}

impl Record {
    /// 控えの 1 行。
    pub fn to_line(&self) -> String {
        match self {
            Self::Told(ts) => format!("told {ts}"),
            Self::Block => "block".to_owned(),
            Self::Released => "released".to_owned(),
            Self::Unjudged(reason) => format!("unjudged {}", reason.as_str()),
        }
    }

    /// 控えの 1 行を読む。読めない行は `None`。
    pub fn parse(line: &str) -> Option<Self> {
        match line.split_once(' ') {
            Some(("told", ts)) if !ts.is_empty() => Some(Self::Told(ts.to_owned())),
            Some(("unjudged", word)) => Reason::parse(word).map(Self::Unjudged),
            None if line == "block" => Some(Self::Block),
            None if line == "released" => Some(Self::Released),
            _ => None,
        }
    }
}

/// 告げ済みの控え（読めた記録の列・読めない行は捨てる）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Told(Vec<Record>);

impl Told {
    /// 控えの全文から読む。
    pub fn parse(text: &str) -> Self {
        Self(text.lines().filter_map(Record::parse).collect())
    }

    /// `ts` を告げ済みか。
    pub fn has_told(&self, ts: &str) -> bool {
        self.0.iter().any(|found| matches!(found, Record::Told(told) if told == ts))
    }

    /// 最後の記録が [`Record::Block`] か（再入で席を Idle に戻す周）。
    pub fn last_is_block(&self) -> bool {
        self.0.last() == Some(&Record::Block)
    }

    /// `reason` を控えに記帳済みか。
    pub fn has_unjudged(&self, reason: Reason) -> bool {
        self.0.contains(&Record::Unjudged(reason))
    }
}

/// turn の終わりの判定（**この境界の閉じた 3 値**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEndDecision {
    /// 告げる未仕分けが無い（打刻して通す）。
    Pass,
    /// 未告の未仕分けが在る: `all` はその session の未仕分けの ts の全部、`fresh` はそのうち控えに無いもの。
    Block { all: Vec<String>, fresh: Vec<String> },
    /// 判じられなかった（止めずに記帳して通す）。
    Unjudged(Reason),
}

/// 読めた未仕分けの列と控えから判定する（pure）。未告が 1 つも無ければ [`TurnEndDecision::Pass`]。
pub fn decide(unsorted: Result<Vec<String>, Reason>, told: &Told) -> TurnEndDecision {
    match unsorted {
        Err(reason) => TurnEndDecision::Unjudged(reason),
        Ok(all) => {
            let fresh: Vec<String> = all.iter().filter(|ts| !told.has_told(ts)).cloned().collect();
            if fresh.is_empty() {
                TurnEndDecision::Pass
            } else {
                TurnEndDecision::Block { all, fresh }
            }
        }
    }
}

/// `SessionStart` の枝: session の置き場へ event log の今の長さを開始の位置として 1 度だけ書く。置き場に既に在れば上書きしない
/// （圧縮と resume の SessionStart）。session_id の無い周・log の長さを測れない周は書かない。書けなかった周だけ stderr の行を返す。
pub(super) fn start(state_dir: &Path, payload: &str) -> Vec<String> {
    let Some(session) = session_of(payload) else {
        return Vec::new();
    };
    let Some(len) = log_len(state_dir) else {
        return Vec::new();
    };
    let file = place(state_dir, &session).join(START_FILE);
    let made = file.parent().map_or(Ok(()), fs::create_dir_all);
    let written = made.and_then(|()| OpenOptions::new().write(true).create_new(true).open(&file)).and_then(|mut found| found.write_all(len.to_string().as_bytes()));
    match written {
        Err(err) if err.kind() == ErrorKind::AlreadyExists => Vec::new(),
        Err(err) => vec![format!("{NAME}: turn-end 開始の位置を書けない: {err}")],
        Ok(()) => Vec::new(),
    }
}

/// `Stop` の枝。再入（`stop_hook_active`）は止めず、控えの最後が `block` の周だけ Idle を打って `released` を足す。
/// 再入でない周は判定し、未告が在れば控えを書いてから rc 2・stdout 0 byte・stderr 1 行で止める（打刻しない）。それ以外は打刻・rc 0。
pub(super) fn stop(args: &[String], payload: &str, state_dir: &Path) -> Outcome {
    let session = session_of(payload);
    if stamp::is_reentry(payload) {
        return release(args, payload, state_dir, session.as_deref());
    }
    let Some(session) = session else {
        return settle(args, payload, state_dir, note(state_dir, None, Reason::NoSession));
    };
    match judge(state_dir, &session) {
        TurnEndDecision::Pass => settle(args, payload, state_dir, Vec::new()),
        TurnEndDecision::Unjudged(reason) => settle(args, payload, state_dir, note(state_dir, Some(&session), reason)),
        TurnEndDecision::Block { all, fresh } => match block(state_dir, &session, &fresh) {
            Ok(()) => Outcome::failed_line(RC_BROKEN, block_line(&all)),
            Err(reason) => settle(args, payload, state_dir, note(state_dir, Some(&session), reason)),
        },
    }
}

/// 止めなかった周の外形: 今のまま打刻（stdout 0 byte・rc 0）し、記帳の失敗の行だけを stderr へ足す。
fn settle(args: &[String], payload: &str, state_dir: &Path, notes: Vec<String>) -> Outcome {
    let mut outcome = Outcome::ok(Vec::new());
    outcome.err = stamp::stamp(args, payload, Stamped::Stop, state_dir);
    outcome.err.extend(notes);
    outcome
}

/// 再入の周: 控えの最後が `block` のときだけ Idle を打って `released` を足す。ほかは黙る。
fn release(args: &[String], payload: &str, state_dir: &Path, session: Option<&str>) -> Outcome {
    let mut outcome = Outcome::ok(Vec::new());
    let Some(session) = session else {
        return outcome;
    };
    if read_told(state_dir, session).is_ok_and(|told| told.last_is_block()) {
        outcome.err = stamp::stamp_release(args, payload, state_dir);
        if let Err(err) = append_told(state_dir, session, &[Record::Released]) {
            outcome.err.push(format!("{NAME}: turn-end released を書けない: {err}"));
        }
    }
    outcome
}

/// 控えと開始の位置と log を読んで判定する（台帳は読まない）。
fn judge(state_dir: &Path, session: &str) -> TurnEndDecision {
    let Ok(told) = read_told(state_dir, session) else {
        return TurnEndDecision::Unjudged(Reason::ToldUnreadable);
    };
    decide(unsorted(state_dir, session), &told)
}

/// その session の未仕分けの発話の ts（log の順）。開始の位置が無ければ [`Reason::NoStart`]、log を読めなければ [`Reason::LogUnreadable`]。
fn unsorted(state_dir: &Path, session: &str) -> Result<Vec<String>, Reason> {
    let text = fs::read_to_string(place(state_dir, session).join(START_FILE)).map_err(|_| Reason::NoStart)?;
    let start = text.trim().parse::<u64>().map_err(|_| Reason::NoStart)?;
    let (mut said, mut marks) = (Vec::new(), Vec::new());
    for event in read_from(state_dir, start).ok_or(Reason::LogUnreadable)? {
        match &event.case {
            Some(Case::Utterance { session: Some(found), .. }) if found == session => said.push(event.ts.clone()),
            Some(Case::Sorted { .. } | Case::Ruling { .. }) => marks.push(event),
            _ => {}
        }
    }
    Ok(said.into_iter().filter(|ts| sorted_of(ts, &marks) == Standing::Unsorted).collect())
}

/// event log を開始の位置から末尾まで読む（読む byte は開始の位置より前の大きさに依らない）。読めない log（開けない・開始の位置より
/// 短い）は `None`。log が無く開始の位置が 0 なら空。読めない行は捨てる。
fn read_from(state_dir: &Path, start: u64) -> Option<Vec<Event>> {
    let mut file = match File::open(store::events_path(state_dir)) {
        Ok(found) => found,
        Err(err) if err.kind() == ErrorKind::NotFound => return (start == 0).then(Vec::new),
        Err(_) => return None,
    };
    if file.metadata().ok()?.len() < start {
        return None;
    }
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(start)).and_then(|_| file.read_to_end(&mut bytes)).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    Some(text.lines().filter_map(|line| Event::from_line(line).ok()).collect())
}

/// event log の今の長さ（byte）。log が無ければ 0。file でない・測れない log は `None`。
fn log_len(state_dir: &Path) -> Option<u64> {
    match fs::metadata(store::events_path(state_dir)) {
        Ok(found) => found.is_file().then_some(found.len()),
        Err(err) if err.kind() == ErrorKind::NotFound => Some(0),
        Err(_) => None,
    }
}

/// 控えを書いて止める前の準備: 告げる ts と `block` を 1 回の書きで足す。書けなければ [`Reason::ToldUnwritable`]。
fn block(state_dir: &Path, session: &str, fresh: &[String]) -> Result<(), Reason> {
    let mut records: Vec<Record> = fresh.iter().cloned().map(Record::Told).collect();
    records.push(Record::Block);
    append_told(state_dir, session, &records).map_err(|_| Reason::ToldUnwritable)
}

/// 止めの 1 行（未仕分けの ts を全部・仕分けの口・bind・show の名・逐語は運ばない）。
fn block_line(all: &[String]) -> String {
    format!(
        "{NAME}: turn-end block 未仕分けの発話 ts={} — 発話ごとに {NAME} utterance sort <ts> --as request --memo <id> か --as chat で仕分け、\
         問いへの答えは {NAME} seat ruling bind で結ぶ（中身は {NAME} utterance show <ts>）",
        all.join(",")
    )
}

/// 判じられなかった周の記帳。控えを持てる語（[`Reason::keeps_record`]）は session ごとに 1 度だけ記帳し（控えに足せなければ
/// [`Reason::ToldUnwritable`] として毎回）、ほかは毎回記帳する。記帳できなかったときだけ stderr の行を返す。
fn note(state_dir: &Path, session: Option<&str>, reason: Reason) -> Vec<String> {
    let mut reason = reason;
    if let (true, Some(found)) = (reason.keeps_record(), session) {
        match read_told(state_dir, found) {
            Err(_) => reason = Reason::ToldUnreadable,
            Ok(told) if told.has_unjudged(reason) => return Vec::new(),
            Ok(_) if append_told(state_dir, found, &[Record::Unjudged(reason)]).is_err() => reason = Reason::ToldUnwritable,
            Ok(_) => {}
        }
    }
    match LockPolicy::embedded().and_then(|policy| store::append(state_dir, &unjudged_event(session, reason), policy)) {
        Ok(_) => Vec::new(),
        Err(err) => vec![format!("{NAME}: turn-end unjudged reason={} を記帳できない: {err}", reason.as_str())],
    }
}

/// 書く行（`TurnEndUnjudged`・actor machine・session は任意・run 無し）。
fn unjudged_event(session: Option<&str>, reason: Reason) -> Event {
    Event {
        schema: crate::fleet::SCHEMA,
        ts: fleet_cli::now_utc(),
        kind: EventKind::TurnEndUnjudged,
        run: String::new(),
        bead: String::new(),
        host: fleet_cli::host(),
        actor: EventKind::TurnEndUnjudged.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: None,
        allowance: None,
        registration: None,
        mark: None,
        account: None,
        cost: None,
        rule: None,
        case: Some(Case::TurnEnd { session: session.map(str::to_owned), reason: reason.as_str().to_owned() }),
    }
}

/// payload の session_id（path に使える字だけ・無い周と使えない周は `None`）。
fn session_of(payload: &str) -> Option<String> {
    let ok = |sid: &str| {
        !sid.is_empty() && sid.len() <= SESSION_MAX && sid.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    };
    field(payload, KEY_SESSION_ID).filter(|sid| ok(sid))
}

/// session の置き場。
fn place(state_dir: &Path, session: &str) -> PathBuf {
    state_dir.join(DIR).join(session)
}

/// 控えを読む。file が無ければ空・それ以外の読めなさは `Err`。
fn read_told(state_dir: &Path, session: &str) -> Result<Told, std::io::Error> {
    match fs::read_to_string(place(state_dir, session).join(TOLD_FILE)) {
        Ok(text) => Ok(Told::parse(&text)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(Told::default()),
        Err(err) => Err(err),
    }
}

/// 控えへ記録を 1 回の書きで足す（置き場の dir は無ければ作る）。
fn append_told(state_dir: &Path, session: &str, records: &[Record]) -> Result<(), std::io::Error> {
    let dir = place(state_dir, session);
    fs::create_dir_all(&dir)?;
    let text: String = records.iter().map(|record| format!("{}\n", record.to_line())).collect();
    OpenOptions::new().create(true).append(true).open(dir.join(TOLD_FILE))?.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::{decide, Reason, Record, Told, TurnEndDecision, REASONS};

    const TS_A: &str = "2026-09-30T07:05:09.123Z";
    const TS_B: &str = "2026-09-30T07:05:09.456Z";

    fn tss(list: &[&str]) -> Vec<String> {
        list.iter().map(|ts| (*ts).to_owned()).collect()
    }

    /// 理由の語は閉じた 5 語で、字面が重ならず往復し、控えを持てるのは no-start と log-unreadable の 2 語だけ。
    #[test]
    fn hook_unsorted_stop_reasons_are_five_closed_words_and_two_keep_a_record() {
        let words: Vec<&str> = REASONS.iter().map(|reason| reason.as_str()).collect();
        assert_eq!(words, ["no-session", "no-start", "log-unreadable", "told-unreadable", "told-unwritable"]);
        for reason in REASONS {
            assert_eq!(Reason::parse(reason.as_str()), Some(*reason), "{reason:?}");
        }
        assert_eq!(Reason::parse("lock"), None);
        let keeping: Vec<&str> = REASONS.iter().filter(|reason| reason.keeps_record()).map(|reason| reason.as_str()).collect();
        assert_eq!(keeping, ["no-start", "log-unreadable"]);
    }

    /// 控えの 4 形は 1 行 1 記録で往復し、読めない行・未知の語・空の ts は捨てる。
    #[test]
    fn hook_unsorted_stop_told_reads_four_forms_and_drops_the_rest() {
        let records = [Record::Told(TS_A.to_owned()), Record::Block, Record::Released, Record::Unjudged(Reason::NoStart)];
        for record in &records {
            assert_eq!(Record::parse(&record.to_line()).as_ref(), Some(record), "{record:?}");
        }
        let text = format!("told {TS_A}\nblock\nnonsense\ntold \nunjudged maybe\nunjudged no-start\n");
        let told = Told::parse(&text);
        assert!(told.has_told(TS_A) && !told.has_told(TS_B), "告げ済みの ts");
        assert!(told.has_unjudged(Reason::NoStart) && !told.has_unjudged(Reason::LogUnreadable), "記帳済みの語");
        assert!(!told.last_is_block(), "最後は unjudged");
        assert_eq!(told, Told::parse(&format!("told {TS_A}\nblock\nunjudged no-start\n")), "読めない行は捨てる");
    }

    /// 再入の表: 最後の記録が block の周だけ真（空・released・告げ済みだけは偽）。
    #[test]
    fn hook_unsorted_stop_reentry_table_releases_only_after_a_block() {
        let table = [
            ("空", "", false),
            ("block", "told a\nblock\n", true),
            ("block の後の released", "told a\nblock\nreleased\n", false),
            ("released の後の block", "block\nreleased\ntold b\nblock\n", true),
            ("告げ済みだけ", "told a\n", false),
        ];
        for (name, text, expected) in table {
            assert_eq!(Told::parse(text).last_is_block(), expected, "{name}");
        }
    }

    /// 判定の表: 未仕分け 0 と告げ済みだけは通し、未告が 1 つでも在れば全部の ts を並べて止め、読めない周は理由を運ぶ。
    #[test]
    fn hook_unsorted_stop_decides_pass_block_or_unjudged() {
        let told = Told::parse(&format!("told {TS_A}\nblock\n"));
        let table = [
            ("未仕分け 0", Ok(tss(&[])), TurnEndDecision::Pass),
            ("告げ済みだけ", Ok(tss(&[TS_A])), TurnEndDecision::Pass),
            ("未告が増えた", Ok(tss(&[TS_A, TS_B])), TurnEndDecision::Block { all: tss(&[TS_A, TS_B]), fresh: tss(&[TS_B]) }),
            ("読めない", Err(Reason::LogUnreadable), TurnEndDecision::Unjudged(Reason::LogUnreadable)),
        ];
        for (name, unsorted, expected) in table {
            assert_eq!(decide(unsorted.clone(), &told), expected, "{name}");
            assert_eq!(decide(unsorted, &told), expected, "{name}: 同じ入力は同じ結果");
        }
        let first = decide(Ok(tss(&[TS_A, TS_B])), &Told::default());
        assert_eq!(first, TurnEndDecision::Block { all: tss(&[TS_A, TS_B]), fresh: tss(&[TS_A, TS_B]) }, "控えが空なら全部が未告");
    }
}
