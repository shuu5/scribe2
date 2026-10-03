//! `pipe permit`: 上限の許可の口（設計 docs/design/limit-permit.md §19 行 c・ADR-0106・SRS FR110 / FR41 / FR17 / NFR4・AC84）。
//!
//! 形は 2 つ: 記帳（`--value` `--until` `--ruling`）と取り消し（`--revoke`）。記帳の周は照らす前に manifest → event log の全件 → 台帳
//! （渡した裁定 id の裁定 event が在る周だけ）→ orchestrator の席の打刻の最後の行（`--repo` を anchor に持つ登録 row が在る周だけ）の順に読み、
//! 最初に読めない置き場を rc 2 で名指して何も書かない（照合の順に関わらない）。読めたら閉じた 10 語（[`Reason`]）を宣言順に 1 つの
//! 関数（[`judge`]）で照らし、最初に外れた語で rc 1 に倒す。通る周だけ行 b の書き手（[`Record`]）で `LimitPermitted` を 1 件書く。
//! 取り消しは manifest と event log だけを読み、`Reason::RuleNotListed` だけを照らす。**列の 1 周は撃たない**（撃ち直しは席の今の手）。
//!
//! 時刻は event の字を [`epoch_of`] / [`epoch_ms_of`] で秒にして比べ（字面で比べない）、今は [`now_secs`]。

use super::{broken, flag, int_row, list_row, need, present, refused, repo_of, state_dir_of};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{epoch_ms_of, epoch_of, replay, Case, Channel, Event, EventKind, ACTOR_HUMAN};
use crate::ledger::{self, close_reason::is_ruling_id};
use crate::pipe::permit::{landed, Record};
use crate::pipe::{emit, Emit};
use crate::rules::manifest::Manifest;
use crate::seat::role::{registration_of_key, Role};
use crate::seat::ruling::{asked_order, Asked};
use crate::seat::state::{last_line, now_secs, path as stamp_path, LastLine};
use crate::seat::{ledger::DEFAULT_BD, seat_dir};
use std::path::{Path, PathBuf};

/// 対象の行の列の rules 行（設計 §18 約束 1）。
const ROWS: &str = "pipe.permit_rows";

/// 期限の上限（時間）の rules 行。
const MAX_H: &str = "pipe.permit_max_h";

/// 値なしの flag（取り消しの形）。
const REVOKE: &str = "--revoke";

/// 記帳の形だけが持つ値つきの flag（取り消しとの併せ持ちは形の誤り）。
const GRANT_FLAGS: [&str; 3] = ["--value", "--until", "--ruling"];

/// 照合を断る閉じた 10 語（**宣言順＝判定の順**・[`judge`] が 1 本で上から照らす・設計 §19 約束 3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reason {
    /// 行が rules 行 `pipe.permit_rows` の要素に無い。
    RuleNotListed,
    /// 値が 10 進の数字でないか、manifest の値より大きくない。
    NotRaise,
    /// 裁定 id が問い id の形でないか、その裁定 event が無い。
    NoRuling,
    /// 裁定の発話の event が無いか、actor が human でない。
    NoUtterance,
    /// 発話が対話面の席の chat でない（打刻の最後の行の会話 id と違う）。
    NotSurface,
    /// 問いの起票が発話より後（か読めない）。
    BeforeQuestion,
    /// 期限が分の形でない・今より前・発話の秒 + 期限の上限を越える。
    BadUntil,
    /// 同じ裁定 id か同じ発話を引く許可が在る。
    Reused,
    /// 問いの本文が bead・行 id・値を字のまま持たない。
    NotStated,
    /// bead の便が着地している。
    Landed,
}

impl Reason {
    /// 行に出す理由の 1 語。
    const fn as_str(self) -> &'static str {
        match self {
            Self::RuleNotListed => "rule-not-listed",
            Self::NotRaise => "not-raise",
            Self::NoRuling => "no-ruling",
            Self::NoUtterance => "no-utterance",
            Self::NotSurface => "not-surface",
            Self::BeforeQuestion => "before-question",
            Self::BadUntil => "bad-until",
            Self::Reused => "reused",
            Self::NotStated => "not-stated",
            Self::Landed => "landed",
        }
    }
}

/// 照合の断り（語と、語ごとに閉じた名指しの字）。
struct Refusal {
    /// 外れた語。
    reason: Reason,
    /// 名指し（`key=値` の列）。
    naming: String,
}

impl Refusal {
    /// rc 1・stdout 0 byte・stderr 1 行（何も書かない）。
    fn outcome(&self) -> Outcome {
        let line = format!("pipe: permit refused reason={} {}", self.reason.as_str(), self.naming);
        Outcome::failed_line(RC_REFUSED, one_line(&line))
    }
}

/// 読んだ flag（2 形）。
enum Form<'a> {
    /// 取り消し（裁定を要らない）。
    Revoke { bead: &'a str, rule: &'a str },
    /// 記帳。
    Grant(Grant<'a>),
}

/// 記帳の flag（値は渡された字のまま）。
struct Grant<'a> {
    bead: &'a str,
    rule: &'a str,
    value: &'a str,
    until: &'a str,
    ruling: &'a str,
    /// 台帳の cwd と orchestrator の席の anchor（絶対 path）。
    repo: PathBuf,
}

/// flag から形を読む（形の誤りは今の rc 1 の字・何も読まず書かない・照合の 10 語に数えない）。
fn shape_of(args: &[String]) -> Result<Form<'_>, String> {
    let (bead, rule) = (need(args, "--bead")?, need(args, "--rule")?);
    if present(args, REVOKE) {
        if let Some(clash) = GRANT_FLAGS.iter().find(|name| present(args, name)) {
            return Err(format!("{REVOKE} は {clash} と併せて渡せない"));
        }
        return Ok(Form::Revoke { bead, rule });
    }
    let [value, until, ruling] = GRANT_FLAGS.map(|name| need(args, name));
    Ok(Form::Grant(Grant { bead, rule, value: value?, until: until?, ruling: ruling?, repo: repo_of(args)? }))
}

/// 入口の manifest の読みが断った周（rc 2・形の誤りは先に rc 1）。
pub(super) fn manifest_unreadable(args: &[String], reason: &str) -> Outcome {
    match shape_of(args) {
        Err(why) => refused(why),
        Ok(_) => unreadable("manifest", reason),
    }
}

/// 読めない周（rc 2・stdout 0 byte・stderr 1 行・何も書かない）。
fn unreadable(source: &str, why: &str) -> Outcome {
    Outcome::failed_line(RC_BROKEN, one_line(&format!("pipe: permit unreadable source={source} {why}")))
}

/// 改行を空白にして 1 行にする。
fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

/// `pipe permit`（manifest は入口が読んだもの）。
pub(super) fn permit(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let form = match shape_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let events = match store::read_all(&state_dir) {
        Ok(found) => found,
        Err(errors) => return unreadable("event-log", &errors.iter().map(ToString::to_string).collect::<Vec<_>>().join(" / ")),
    };
    match form {
        Form::Revoke { bead, rule } => {
            let listed = listed_of(manifest);
            if !listed.iter().any(|item| item == rule) {
                return not_listed(bead, rule, &listed).outcome();
            }
            write(&state_dir, (bead, &Record::Revoke { rule: rule.to_owned() }), policy, format!("permit: bead={bead} rule={rule} revoked"))
        }
        Form::Grant(grant) => grant_run(args, &grant, (manifest, &state_dir, &events), policy),
    }
}

/// 記帳の周: 照らす前の読み（台帳・打刻）を済ませてから [`judge`] で照らし、通れば 1 件書く。
fn grant_run(args: &[String], grant: &Grant<'_>, place: (&Manifest, &Path, &[Event]), policy: LockPolicy) -> Outcome {
    let (manifest, state_dir, events) = place;
    let ruling = events.iter().find(|event| event.kind == EventKind::RulingReceived && ruling_case(event).is_some_and(|(id, _, _)| id == grant.ruling));
    let body = match ruling {
        None => String::new(),
        Some(event) => match ledger::show(flag(args, "--bd").ok().flatten().unwrap_or(DEFAULT_BD), &grant.repo, &event.bead) {
            Ok(found) => found.map(|bead| bead.description).unwrap_or_default(),
            Err(_) => return unreadable("ledger", &format!("question={}", event.bead)),
        },
    };
    let stamp = match stamp_of(state_dir, events, &grant.repo) {
        Ok(found) => found,
        Err(why) => return unreadable("stamp", &why),
    };
    let facts = Facts { manifest, events, ruling, body: &body, stamp: &stamp, now: now_secs() };
    match judge(grant, &facts) {
        Err(refusal) => refusal.outcome(),
        Ok(passed) => {
            let line = format!(
                "permit: bead={} rule={} value={} declared={} until={} ruling={}",
                grant.bead, grant.rule, passed.value, passed.declared, passed.until, grant.ruling
            );
            let record = Record::Permit { rule: grant.rule.to_owned(), value: passed.value, until: passed.until, ruling: grant.ruling.to_owned() };
            write(state_dir, (grant.bead, &record), policy, line)
        }
    }
}

/// 通った記帳か取り消しを 1 件書き、書けた周に stdout の 1 行を返す（書けない周は store の書きの今の極性＝rc 2・行は書いていない）。
fn write(state_dir: &Path, (bead, record): (&str, &Record), policy: LockPolicy, line: String) -> Outcome {
    let entry = Emit { kind: EventKind::LimitPermitted, run: "", bead, stage: None, seat: None, pid: None, detail: Some(record.render()) };
    match emit(state_dir, &entry, policy) {
        Ok(()) => Outcome::ok_line(line),
        Err(err) => broken(format!("permit unwritable {}", one_line(&err.to_string()))),
    }
}

/// orchestrator の席の打刻の最後の行（照合の入力・読めた 5 形）。
enum Stamp {
    /// `--repo` を anchor に持つ登録 row が無い。
    NoSeat,
    /// 打刻の file が無い。
    Absent,
    /// 空白でない行を持たない。
    Blank,
    /// 最後の行の会話 id が空か形でない。
    Unshaped,
    /// 会話 id（形どおり）。
    Sid(String),
}

/// 打刻を読む。登録 row が無い周は読まない。NotFound の外で開けない周と最後の行が打刻の形でない周は `Err`（`path=<file> why=<open|last-line>`）。
fn stamp_of(state_dir: &Path, events: &[Event], repo: &Path) -> Result<Stamp, String> {
    let state = replay(events);
    let Some(row) = registration_of_key(&state, Role::Orchestrator, &repo.to_string_lossy()) else {
        return Ok(Stamp::NoSeat);
    };
    let dir = seat_dir(state_dir, &row.target);
    let broken = |why: &str| format!("path={} why={why}", stamp_path(&dir).display());
    match last_line(&dir) {
        LastLine::Sid(sid) => Ok(Stamp::Sid(sid)),
        LastLine::Absent => Ok(Stamp::Absent),
        LastLine::Blank => Ok(Stamp::Blank),
        LastLine::Unshaped => Ok(Stamp::Unshaped),
        LastLine::Unopened => Err(broken("open")),
        LastLine::NotStamp => Err(broken("last-line")),
    }
}

/// 照合が読む材料（読みの済んだもの）。
struct Facts<'a> {
    manifest: &'a Manifest,
    /// event log の全件。
    events: &'a [Event],
    /// 渡した裁定 id の裁定 event（無ければ `None`）。
    ruling: Option<&'a Event>,
    /// 問いの本文（問いが台帳に無い周と本文の無い周は空）。
    body: &'a str,
    stamp: &'a Stamp,
    /// 今（UNIX 秒）。
    now: u64,
}

/// 通った記帳の値（stdout の 1 行と detail の材料）。
struct Passed {
    value: u64,
    declared: u64,
    /// detail の期限（秒の形）。
    until: String,
}

/// 裁定 event の Ruling 形の（裁定 id・発話の ts・問いの起票の時刻）。
fn ruling_case(event: &Event) -> Option<(&str, &str, &str)> {
    let Some(Case::Ruling { ruling, utterance, question_ts, .. }) = &event.case else {
        return None;
    };
    Some((ruling, utterance, question_ts))
}

/// 対象の行の列（行が無い・不発効・列でない周は空＝要素 0）。
fn listed_of(manifest: &Manifest) -> Vec<String> {
    list_row(manifest, ROWS).unwrap_or_default()
}

/// rule-not-listed の断り（取り消しと記帳が同じ 1 本）。
fn not_listed(bead: &str, rule: &str, listed: &[String]) -> Refusal {
    let shown = if listed.is_empty() { "-".to_owned() } else { listed.join(",") };
    Refusal { reason: Reason::RuleNotListed, naming: format!("bead={bead} rule={rule} listed={shown}") }
}

/// 照合 10 語（**1 つの関数が閉じた enum の宣言順に上から照らし**、最初に外れた語で止まる・何も書かない）。
fn judge(grant: &Grant<'_>, facts: &Facts<'_>) -> Result<Passed, Refusal> {
    let (bead, rule, value, until, ruling) = (grant.bead, grant.rule, grant.value, grant.until, grant.ruling);
    let refuse = |reason: Reason, naming: String| Refusal { reason, naming };
    // 1 rule-not-listed
    let listed = listed_of(facts.manifest);
    if !listed.iter().any(|item| item == rule) {
        return Err(not_listed(bead, rule, &listed));
    }
    // 2 not-raise
    let declared = int_row(facts.manifest, rule).ok();
    let Some((raised, declared)) = raised(value, declared) else {
        let shown = declared.map_or_else(|| "-".to_owned(), |found| found.to_string());
        return Err(refuse(Reason::NotRaise, format!("bead={bead} rule={rule} value={value} declared={shown}")));
    };
    // 3 no-ruling
    let no_ruling = || refuse(Reason::NoRuling, format!("ruling={ruling}"));
    let Some((question, utterance, question_ts)) = facts.ruling.filter(|_| question_form(ruling)).and_then(|event| ruling_case(event).map(|(_, said, asked)| (event.bead.as_str(), said, asked))) else {
        return Err(no_ruling());
    };
    // 4 no-utterance（同じ ts の最初の 1 件が human）
    let said = facts.events.iter().find(|event| event.kind == EventKind::UtteranceReceived && event.ts == utterance).filter(|event| event.actor == ACTOR_HUMAN);
    let Some(said) = said else {
        return Err(refuse(Reason::NoUtterance, format!("ruling={ruling} utterance={utterance}")));
    };
    // 5 not-surface（chat で、session が打刻の最後の行の会話 id と同じ）
    let (channel, session) = surface_of(said);
    let stamp_sid = match facts.stamp {
        Stamp::Sid(sid) => Some(sid.as_str()),
        Stamp::NoSeat | Stamp::Absent | Stamp::Blank | Stamp::Unshaped => None,
    };
    if channel != Channel::Chat || session.is_none() || session != stamp_sid {
        let stamp = match facts.stamp {
            Stamp::NoSeat => "no-seat",
            Stamp::Absent => "absent",
            Stamp::Blank => "blank",
            Stamp::Unshaped => "unshaped",
            Stamp::Sid(sid) => sid,
        };
        return Err(refuse(Reason::NotSurface, format!("utterance={utterance} channel={} session={} stamp={stamp}", channel.as_str(), session.unwrap_or("-"))));
    }
    // 6 before-question
    if asked_order(question_ts, utterance) != Asked::NotAfter {
        return Err(refuse(Reason::BeforeQuestion, format!("utterance={utterance} question={question} question_ts={question_ts}")));
    }
    // 7 bad-until
    let max_h = int_row(facts.manifest, MAX_H).ok();
    let Some(until_at) = until_ok(until, utterance, max_h, facts.now) else {
        let shown = max_h.map_or_else(|| "-".to_owned(), |found| found.to_string());
        return Err(refuse(Reason::BadUntil, format!("until={until} utterance={utterance} max_h={shown}")));
    };
    // 8 reused（1 つの発話に許可 1 つ・取り消しに依らない）
    let used = facts.events.iter().filter(|event| event.kind == EventKind::LimitPermitted).filter_map(|event| match event.detail.as_deref().and_then(Record::parse) {
        Some(Record::Permit { ruling, .. }) => Some(ruling),
        Some(Record::Revoke { .. }) | None => None,
    });
    if used.into_iter().any(|other| other == ruling || utterance_of(facts.events, &other).is_some_and(|ts| ts == utterance)) {
        return Err(refuse(Reason::Reused, format!("ruling={ruling} utterance={utterance}")));
    }
    // 9 not-stated（本文の ASCII の語に bead・行 id・値が字のまま在る）
    let words = words_of(facts.body);
    let missing: Vec<&str> = [("bead", bead), ("rule", rule), ("value", value)].iter().filter(|(_, needle)| !words.contains(needle)).map(|(name, _)| *name).collect();
    if !missing.is_empty() {
        return Err(refuse(Reason::NotStated, format!("question={question} missing={}", missing.join(","))));
    }
    // 10 landed
    if let Some(run) = landed(bead, facts.events) {
        return Err(refuse(Reason::Landed, format!("bead={bead} run={run}")));
    }
    Ok(Passed { value: raised, declared, until: until_at })
}

/// 値が 10 進の数字だけ（先頭 0・符号・区切り無し）で u64 に収まり、manifest の値（有効な Int）より大きいなら（値・manifest の値）。
fn raised(text: &str, declared: Option<u64>) -> Option<(u64, u64)> {
    let digits = !text.starts_with('0') && text.bytes().all(|byte| byte.is_ascii_digit());
    let value: u64 = text.parse().ok().filter(|_| digits)?;
    let declared = declared?;
    (value > declared).then_some((value, declared))
}

/// 裁定が問い id の形か（`<問い id>:<YYYYMMDDTHHMMZ>-<n>`・`batch:` と `policy:` の形は問いを持たないので偽）。
fn question_form(id: &str) -> bool {
    let prefix = id.split_once(':').and_then(|(question, _)| question.split_once('-')).map(|(prefix, _)| prefix);
    prefix.is_some_and(|prefix| is_ruling_id(id, Some(prefix)))
}

/// 発話 event の経路と session（Utterance 形を持たない行は gui・session 無し＝chat の照合を通さない側）。
fn surface_of(event: &Event) -> (Channel, Option<&str>) {
    match &event.case {
        Some(Case::Utterance { channel, session }) => (*channel, session.as_deref()),
        _ => (Channel::Gui, None),
    }
}

/// 期限が `YYYY-MM-DDTHH:MMZ` の形で今より後、かつ発話の秒に期限の上限（時間）を足した時刻以下なら、detail の期限（秒の形）。
fn until_ok(until: &str, utterance: &str, max_h: Option<u64>, now: u64) -> Option<String> {
    let head = until.strip_suffix('Z').filter(|head| head.len() == 16)?;
    let detail = format!("{head}:00Z");
    let at = epoch_of(&detail)?;
    let limit = (epoch_ms_of(utterance)? / 1_000).saturating_add(max_h?.saturating_mul(3_600));
    (at > now && at <= limit).then_some(detail)
}

/// 裁定 id の裁定 event が引く発話の ts（最初の 1 件）。
fn utterance_of<'a>(events: &'a [Event], ruling: &str) -> Option<&'a str> {
    events.iter().filter(|event| event.kind == EventKind::RulingReceived).find_map(|event| ruling_case(event).filter(|(id, _, _)| *id == ruling).map(|(_, said, _)| said))
}

/// 本文の語の列（ASCII の英数字と `.` `_` `-` の最長の連なり・末尾の `.` を剥ぐ）。
fn words_of(text: &str) -> Vec<&str> {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')))
        .map(|word| word.trim_end_matches('.'))
        .filter(|word| !word.is_empty())
        .collect()
}
