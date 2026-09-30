//! run 無しの user 裁定を承認 event として持つ口（設計 docs/design/fleet-event-log.md §9 / §14・ADR-0037・ADR-0087・SRS FR41 /
//! FR22 / FR89・憲法 C7.2）。
//!
//! 書き手は `seat ruling bind` の 1 本だけで、記帳された発話（[`EventKind::UtteranceReceived`]）と開いた台帳の問いを結び、
//! 裁定 id を発行して notes に 5 欄の行を書き、`裁定 <id>` で close し、[`EventKind::RulingReceived`]（actor = `human`・
//! `run` 無し・本体は [`Case::Ruling`]）を 1 件書く（[`bind`]）。逐語は発話 event から写す（席が逐語を渡す口は無い・ADR-0087）。
//! 逐語を受ける口は `seat ruling answer` の 1 本だけで、経路 gui の発話を書いてから同じ [`bind`] を呼ぶ（[`answer`]・hook が席の撃ちを止める）。
//! 読み手は `seat ruling ls`（1 件 1 行）と doctor の突合の 1 行（[`doctor_lines`]・manifest の `user <ts>` の行ごとに
//! 同じ分の event の有無を数えるだけ・判定しない＝C10.2）。

use crate::fleet::json_lite;
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{cli, epoch_ms_of, epoch_of, Case, Channel, Event, EventKind, ACTOR_HUMAN, SCHEMA};
use crate::ledger::close_reason::is_ruling_id;
use crate::ledger::form::QUESTION_LABEL;
use crate::ledger::{self, Bead};
use crate::rules::manifest::Manifest;
use std::collections::BTreeSet;
use std::path::Path;

/// 結びを断る理由（**閉じた 4 語**・判定の順・断る周は何も書かない・設計 §14 約束 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// その ts の発話が記帳されていない。
    NoUtterance,
    /// 同じ発話と同じ問いの組の裁定が在る。
    Bound,
    /// 問いの status が open でない。
    Closed,
    /// bead が無い、または label が問いでない。
    NotQuestion,
}

/// [`Refusal`] の全 variant（判定の順・`enum-slices` が集合完全性を測る）。
pub const REFUSALS: &[Refusal] = &[Refusal::NoUtterance, Refusal::Bound, Refusal::Closed, Refusal::NotQuestion];

impl Refusal {
    /// 行に出す理由の 1 語。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoUtterance => "no-utterance",
            Self::Bound => "bound",
            Self::Closed => "closed",
            Self::NotQuestion => "not-question",
        }
    }
}

/// 書きが途中で止まった段（設計 §14 約束 8・止まった後の撃ち直しが仕上げる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// notes は書いた・close が落ちた。
    Close,
    /// notes と close は済んだ・裁定 event が落ちた。
    Event,
}

impl Stage {
    /// 行に出す段の 1 語。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::Event => "event",
        }
    }
}

/// 結びが通らなかった形（rc と行は呼び手が決める）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    /// 閉じた 4 語の断り（何も書いていない）。
    Refused(Refusal),
    /// 台帳の bead を読めない（何も書いていない）。
    LedgerUnreadable,
    /// event log を読めない（何も書いていない・理由の本文）。
    LogUnreadable(Vec<String>),
    /// notes の追記が落ちた（何も書いていない）。
    NotesFailed,
    /// 書きが途中で止まった（止まった段と裁定 id）。
    Partial { stage: Stage, id: String },
}

/// 結びの入力（`seat ruling bind` の引数）。
pub struct Bind<'a> {
    /// 台帳を撃つ repo（bd の cwd）。
    pub repo: &'a Path,
    /// event log の置き場。
    pub state_dir: &'a Path,
    /// 問いの bead id。
    pub question: &'a str,
    /// 発話の ts（[`EventKind::UtteranceReceived`] の ts の字面）。
    pub utterance: &'a str,
    /// 台帳 client。
    pub bd: &'a str,
}

/// 結べた裁定（stdout の 1 行の材料）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    /// 裁定 id。
    pub id: String,
    /// 発話の経路。
    pub channel: Channel,
}

/// 問いの status（open の間だけ新しく結べる）。
const OPEN: &str = "open";

/// 裁定 id の末尾の番号（1 問 1 裁定なので常に 1）。
const COUNT: u32 = 1;

/// 裁定 id `<問い id>:<発話の年月日と時分 YYYYMMDDTHHMMZ>-1`（pure）。発話の時刻から作るので、撃ち直しても同じ id になる。
/// ts の形が読めない・問い id が同じ台帳の bead id の形でない周は `None`（[`is_ruling_id`] が真の字だけを返す）。
pub fn ruling_id(question: &str, utterance: &str) -> Option<String> {
    let part = |range: std::ops::Range<usize>| utterance.get(range).filter(|found| found.bytes().all(|byte| byte.is_ascii_digit()));
    let (year, month, day, hour, minute) = (part(0..4)?, part(5..7)?, part(8..10)?, part(11..13)?, part(14..16)?);
    let id = format!("{question}:{year}{month}{day}T{hour}{minute}Z-{COUNT}");
    let prefix = question.split_once('-')?.0;
    is_ruling_id(&id, Some(prefix)).then_some(id)
}

/// notes に足す裁定の 1 行 `<裁定 id> | <問い id> | <発話の ts> | <経路> | <逐語>`（pure）。逐語は JSON の文字列の字面で最後の欄に置く
/// ＝改行を含んでも 1 行で、戻すと 1 byte も違わない。
fn row_of(id: &str, question: &str, utterance: &str, channel: Channel, words: &str) -> String {
    format!("{id} | {question} | {utterance} | {} | {}", channel.as_str(), json_lite::quote(words))
}

/// 記帳された発話と開いた台帳の問いを結ぶ（設計 §14）。断りは (a) 発話が無い (b) 結び済み (c) 閉じた問い (d) 問いでない の順で、
/// 台帳は (b) の後に 1 回だけ読む。書きは notes → close → 裁定 event の順（途中で止まった周は [`BindError::Partial`]・同じ組の
/// 撃ち直しが続きだけを書く）。
pub fn bind(args: &Bind<'_>) -> Result<Bound, BindError> {
    let events =
        store::read_all(args.state_dir).map_err(|errors| BindError::LogUnreadable(errors.iter().map(ToString::to_string).collect()))?;
    let said = events.iter().find(|event| event.kind == EventKind::UtteranceReceived && event.ts == args.utterance);
    let Some((said, Some(Case::Utterance { channel, .. }))) = said.map(|event| (event, event.case.clone())) else {
        return Err(BindError::Refused(Refusal::NoUtterance));
    };
    let bound = events.iter().any(|event| {
        event.kind == EventKind::RulingReceived
            && event.bead == args.question
            && matches!(&event.case, Some(Case::Ruling { utterance, .. }) if utterance == args.utterance)
    });
    if bound {
        return Err(BindError::Refused(Refusal::Bound));
    }
    let bead = ledger::show(args.bd, args.repo, args.question).map_err(|_| BindError::LedgerUnreadable)?;
    let not_question = BindError::Refused(Refusal::NotQuestion);
    let (Some(bead), Some(id)) = (bead, ruling_id(args.question, &said.ts)) else {
        return Err(not_question);
    };
    let words = said.detail.clone().unwrap_or_default();
    let row = row_of(&id, args.question, &said.ts, channel, &words);
    let written = bead.notes.lines().any(|line| line == row);
    if bead.status != OPEN && !written {
        return Err(BindError::Refused(Refusal::Closed));
    }
    if bead.status == OPEN && !bead.labels.iter().any(|label| label == QUESTION_LABEL) {
        return Err(not_question);
    }
    let partial = |stage| BindError::Partial { stage, id: id.clone() };
    if bead.status == OPEN {
        if !written {
            ledger::append_notes(args.bd, args.repo, args.question, &row).map_err(|_| BindError::NotesFailed)?;
        }
        ledger::close(args.bd, args.repo, args.question, &format!("裁定 {id}")).map_err(|_| partial(Stage::Close))?;
    }
    let event = ruling_event(args, &bead, &id, channel, words);
    let policy = LockPolicy::embedded().map_err(|_| partial(Stage::Event))?;
    store::append(args.state_dir, &event, policy).map_err(|_| partial(Stage::Event))?;
    Ok(Bound { id, channel })
}

/// 答えの口の断り（何も書いていない）の語のうち、結びの 4 語にも台帳を読めない周の語にも無いもの（設計 dialogue-surface.md §11 約束 2）。
pub const WORDS_EMPTY: &str = "words-empty";

/// 台帳を読めない周の断りの語（結びの [`BindError::LedgerUnreadable`] を行にするときの字と同じ）。
pub const LEDGER_UNREADABLE: &str = "ledger-unreadable";

/// 答えの口が通らなかった形（rc と行は呼び手が決める）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerError {
    /// 何も書かずに断った（語は [`WORDS_EMPTY`]・[`LEDGER_UNREADABLE`]・[`Refusal::Closed`]・[`Refusal::NotQuestion`] のどれか）。
    Refused(&'static str),
    /// 発話を書けなかった（何も書いていない）。
    Unwritten,
    /// 発話は書いた後で結びが落ちた（発話の ts・`seat ruling bind` で同じ ts を結び直せる）。
    Partial(String),
}

/// 答えの口（設計 dialogue-surface.md §11 約束 2 / 3）: 逐語が空白だけ → 台帳を読めない → 問いが閉じている → 台帳の問いでない
/// の順に調べ（どれも何も書かない）、通る周は経路 gui の発話を 1 件書き（ts は store が lock の中で振る）、その ts で [`bind`]
/// を呼んで裁定 id を返す。結びのどの失敗も [`AnswerError::Partial`]（発話は残る）。承認 event は書かない。
pub fn answer(args: &Bind<'_>, words: &str) -> Result<String, AnswerError> {
    if words.trim().is_empty() {
        return Err(AnswerError::Refused(WORDS_EMPTY));
    }
    let bead = ledger::show(args.bd, args.repo, args.question).map_err(|_| AnswerError::Refused(LEDGER_UNREADABLE))?;
    let not_question = AnswerError::Refused(Refusal::NotQuestion.as_str());
    let Some(bead) = bead else {
        return Err(not_question);
    };
    if bead.status != OPEN {
        return Err(AnswerError::Refused(Refusal::Closed.as_str()));
    }
    // 裁定 id を作れない問い（id の形・時刻の形）は結びも断るので、発話を書く前に断る。
    if !bead.labels.iter().any(|label| label == QUESTION_LABEL) || ruling_id(args.question, SAMPLE_TS).is_none() {
        return Err(not_question);
    }
    let policy = LockPolicy::embedded().map_err(|_| AnswerError::Unwritten)?;
    let (ts, _) = store::append_utterance(args.state_dir, &utterance_event(words), policy).map_err(|_| AnswerError::Unwritten)?;
    bind(&Bind { utterance: &ts, ..*args }).map(|done| done.id).map_err(|_| AnswerError::Partial(ts))
}

/// 裁定 id を作れるか確かめるための ts の字（形だけ・発話の ts は store が振る）。
const SAMPLE_TS: &str = "2026-01-01T00:00:00.000Z";

/// 書く発話の event（`UtteranceReceived`・actor human・経路 gui・session 無し・逐語の detail・run 無し）。ts は store が振る。
fn utterance_event(words: &str) -> Event {
    Event {
        schema: SCHEMA,
        ts: String::new(),
        kind: EventKind::UtteranceReceived,
        run: String::new(),
        bead: String::new(),
        host: cli::host(),
        actor: ACTOR_HUMAN.to_owned(),
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
        case: Some(Case::Utterance { channel: Channel::Gui, session: None }),
    }
}

/// 結んだ裁定の event（actor human・bead は問い id・detail は発話の逐語・本体は [`Case::Ruling`]）。
fn ruling_event(args: &Bind<'_>, bead: &Bead, id: &str, channel: Channel, words: String) -> Event {
    Event {
        schema: SCHEMA,
        ts: cli::now_utc(),
        kind: EventKind::RulingReceived,
        run: String::new(),
        bead: args.question.to_owned(),
        host: cli::host(),
        actor: ACTOR_HUMAN.to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: Some(words),
        allowance: None,
        registration: None,
        mark: None,
        account: None,
        cost: None,
        rule: None,
        case: Some(Case::Ruling {
            ruling: id.to_owned(),
            utterance: args.utterance.to_owned(),
            channel,
            question_ts: bead.created_at.clone(),
            asked: bead.asked.clone(),
        }),
    }
}

/// 裁定 1 件の 1 行（pure・`ruling: ts=<ts> bead=<b> rule=<id> words=<逐語>`）。無い欄は `-`、逐語は改行や `"` を含んでも 1 行に
/// 収まるよう引用符つきで escape する（中身は逐語のまま・要約しない）。結びの形の行は `rule=` の代わりに `ruling=<裁定 id>` を出す。
pub fn render(event: &Event) -> String {
    let or_dash = |text: &str| if text.is_empty() { "-".to_owned() } else { text.to_owned() };
    let key = match &event.case {
        Some(Case::Ruling { ruling, .. }) => format!("ruling={ruling}"),
        _ => format!("rule={}", or_dash(event.rule.as_deref().unwrap_or_default())),
    };
    format!("ruling: ts={} bead={} {key} words={:?}", event.ts, or_dash(&event.bead), event.detail.as_deref().unwrap_or_default())
}

/// log の裁定の一覧（物理順・1 件 1 行）。読めない log は `Err`（0 件に潰さない）。
pub fn ls(state_dir: &Path) -> Result<Vec<String>, Vec<store::StoreError>> {
    let events = store::read_all(state_dir)?;
    Ok(events.iter().filter(|event| event.kind == EventKind::RulingReceived).map(render).collect())
}

/// manifest の行の `ruling` 欄が指す裁定の分（`user <YYYY-MM-DDTHH:MMZ>` の形の先頭の語・pure）。
///
/// `user ` で始まらない行は `None`（母集団の外）、`user ` で始まるが分まで一意に読めない形（`4xZ`・日付だけ・散文）は
/// `Some(None)`（`skipped` に数える）、読める行は `Some(Some("YYYY-MM-DDTHH:MM"))`。
pub fn ruling_minute(ruling: &str) -> Option<Option<String>> {
    let rest = ruling.strip_prefix("user ")?;
    let token = rest.split_whitespace().next().unwrap_or_default();
    let minute = token.strip_suffix('Z').filter(|body| {
        let bytes = body.as_bytes();
        bytes.len() == 16
            && bytes.iter().enumerate().all(|(at, byte)| match at {
                4 | 7 => *byte == b'-',
                10 => *byte == b'T',
                13 => *byte == b':',
                _ => byte.is_ascii_digit(),
            })
    });
    Some(minute.map(str::to_owned))
}

/// doctor の突合の数（pure・[`Tally::line`] の材料）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tally {
    /// log の裁定の数。
    pub rulings: usize,
    /// 母集団（`user <分>` の行）のうち同じ分の裁定が在る行の数。
    pub matched: usize,
    /// 母集団の行の数。
    pub of: usize,
    /// 同じ分の裁定が無い行の id（manifest の順）。
    pub unmatched: Vec<String>,
    /// `user ` で始まるが分まで読めない行の数（母集団の外）。
    pub skipped: usize,
}

impl Tally {
    /// log と manifest から数える（pure）。同じ分に裁定が複数在る行も matched 1 行に数える（1 行が 1 件を一意に指すことは保証しない）。
    pub fn of(events: &[Event], manifest: &Manifest) -> Self {
        let rulings: Vec<&Event> = events.iter().filter(|event| event.kind == EventKind::RulingReceived).collect();
        let minutes: BTreeSet<&str> = rulings.iter().filter_map(|event| event.ts.get(..16)).collect();
        let (mut matched, mut of, mut skipped, mut unmatched) = (0_usize, 0_usize, 0_usize, Vec::new());
        for row in manifest.rows() {
            match ruling_minute(&row.ruling) {
                None => {}
                Some(None) => skipped = skipped.saturating_add(1),
                Some(Some(minute)) => {
                    of = of.saturating_add(1);
                    if minutes.contains(minute.as_str()) {
                        matched = matched.saturating_add(1);
                    } else {
                        unmatched.push(row.id.clone());
                    }
                }
            }
        }
        Self { rulings: rulings.len(), matched, of, unmatched, skipped }
    }

    /// doctor の 1 行（`rulings=<n> rule-rulings=<matched>/<of> unmatched=<id,…|-> skipped=<n>`）。
    pub fn line(&self) -> String {
        let unmatched = if self.unmatched.is_empty() { "-".to_owned() } else { self.unmatched.join(",") };
        format!(
            "rulings={} rule-rulings={}/{} unmatched={unmatched} skipped={}",
            self.rulings, self.matched, self.of, self.skipped
        )
    }

    /// 数えるものが何も無い周か（裁定 0・母集団 0・skipped 0）。
    fn is_empty(&self) -> bool {
        self.rulings == 0 && self.of == 0 && self.skipped == 0
    }
}

/// 問いの起票が発話より後の結びの数（pure・[`AskedAfter::line`] の材料・設計 dialogue-surface.md §13・FR89）。母集団は
/// [`Case::Ruling`] を持つ裁定 event（`utterance` を持たない古い裁定 event は数えない）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AskedAfter {
    /// 母集団の結びの数。
    pub binds: usize,
    /// asked が seat で問いの起票が発話より後の結びの裁定 id（log の順）。
    pub after_seat: Vec<String>,
    /// asked が user で問いの起票が発話より後の結びの裁定 id（log の順）。
    pub after_user: Vec<String>,
    /// asked の無い結びの裁定 id（発話との前後に依らない・log の順）。
    pub asked_none: Vec<String>,
    /// 問いの起票か発話の時刻を読めない結びの数（`after-*` に入れない）。
    pub unreadable: usize,
}

/// 裁定 id の列の字（無ければ `-`）。
fn ids_or_dash(ids: &[String]) -> String {
    if ids.is_empty() {
        "-".to_owned()
    } else {
        ids.join(",")
    }
}

impl AskedAfter {
    /// log から数える（pure）。問いの起票の時刻は [`epoch_of`]、発話の ts は [`epoch_ms_of`] で読み、発話を秒へ切り捨てて比べる
    /// （字面で比べない・同じ秒は後に数えない）。
    pub fn of(events: &[Event]) -> Self {
        let mut found = Self::default();
        for event in events.iter().filter(|event| event.kind == EventKind::RulingReceived) {
            let Some(Case::Ruling { ruling, utterance, question_ts, asked, .. }) = &event.case else {
                continue;
            };
            found.binds = found.binds.saturating_add(1);
            if asked.is_none() {
                found.asked_none.push(ruling.clone());
            }
            let (Some(asked_at), Some(said_ms)) = (epoch_of(question_ts), epoch_ms_of(utterance)) else {
                found.unreadable = found.unreadable.saturating_add(1);
                continue;
            };
            if asked_at <= said_ms / 1_000 {
                continue;
            }
            match asked.as_deref() {
                Some("seat") => found.after_seat.push(ruling.clone()),
                Some("user") => found.after_user.push(ruling.clone()),
                _ => {}
            }
        }
        found
    }

    /// doctor の 1 行（`binds=<N> after-seat=<n> after-seat-ids=<id,…|-> after-user=<n> after-user-ids=<id,…|-> asked-none=<n>
    /// asked-none-ids=<id,…|->`・読めない結びが在れば末尾に ` unreadable=<n>`）。
    pub fn line(&self) -> String {
        let mut line = format!(
            "binds={} after-seat={} after-seat-ids={} after-user={} after-user-ids={} asked-none={} asked-none-ids={}",
            self.binds,
            self.after_seat.len(),
            ids_or_dash(&self.after_seat),
            self.after_user.len(),
            ids_or_dash(&self.after_user),
            self.asked_none.len(),
            ids_or_dash(&self.asked_none)
        );
        if self.unreadable > 0 {
            line.push_str(&format!(" unreadable={}", self.unreadable));
        }
        line
    }
}

/// doctor の問いの起票の行（[`AskedAfter`]・母集団 0 の周は行を出さず、読めない log は `binds=unreadable`）。
fn asked_after_lines(events: Option<&[Event]>) -> Vec<String> {
    let Some(events) = events else {
        return vec!["binds=unreadable".to_owned()];
    };
    let found = AskedAfter::of(events);
    if found.binds == 0 {
        Vec::new()
    } else {
        vec![found.line()]
    }
}

/// doctor の突合の行（設計 §9 (4)・読むだけ・判定しない＝rc を変えない）。manifest は `rules`（`--rules` の値）か埋め込み。
///
/// 裁定 0・母集団 0・skipped 0 の周は**行を出さない**（数えるものの無い置き場の doctor の外形を動かさない）。log か manifest を
/// 読めない周は数を 0 と書かず `unreadable` を名乗る 1 行を出す。`events` は呼び手が 1 回だけ読んだ log（読めない周は `None`）。
/// 突合の行の直後に問いの起票の行（[`asked_after_lines`]）が続く。
pub fn doctor_lines(events: Option<&[Event]>, rules: Option<&str>) -> Vec<String> {
    let mut lines = tally_lines(events, rules);
    lines.extend(asked_after_lines(events));
    lines
}

/// 突合の行（[`doctor_lines`] の前半）。
fn tally_lines(events: Option<&[Event]>, rules: Option<&str>) -> Vec<String> {
    let manifest = super::manifest_read(rules.map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path))));
    match (events, manifest) {
        (Some(events), Ok(found)) => {
            let tally = Tally::of(events, &found);
            if tally.is_empty() {
                Vec::new()
            } else {
                vec![tally.line()]
            }
        }
        (None, _) => vec!["rulings=unreadable rule-rulings=unmeasurable".to_owned()],
        (Some(events), Err(_)) => {
            let rulings = events.iter().filter(|event| event.kind == EventKind::RulingReceived).count();
            vec![format!("rulings={rulings} rule-rulings=manifest-unreadable")]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ruling_id, ruling_minute, AskedAfter, Tally};
    use crate::fleet::{Case, Channel, Event, EventKind, ACTOR_HUMAN, SCHEMA};
    use crate::ledger::close_reason::is_ruling_id;
    use crate::rules::manifest::Manifest;

    /// `[[rule]]` の行の本文（id・ruling・発効）。
    fn row(id: &str, kind: &str, value: &str, enabled: bool, ruling: &str) -> String {
        format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = {enabled}\nruling = \"{ruling}\"\nruled_at = \"d\"\n")
    }

    /// 本文から manifest を読む。
    fn manifest(rows: &str) -> Manifest {
        match Manifest::parse(&format!("schema = 1\n{rows}")) {
            Ok(found) => found,
            Err(errors) => panic!("fixture の manifest を読める: {errors:?}"),
        }
    }

    /// 裁定 1 件（ts だけを選ぶ）。
    fn ruling(ts: &str) -> Event {
        Event {
            schema: SCHEMA,
            ts: ts.to_owned(),
            kind: EventKind::RulingReceived,
            run: String::new(),
            bead: String::new(),
            host: "h".to_owned(),
            actor: ACTOR_HUMAN.to_owned(),
            stage: None,
            seat: None,
            pid: None,
            detail: Some("推奨で".to_owned()),
            allowance: None,
            registration: None,
            mark: None,
            account: None,
            cost: None,
            rule: None,
            case: None,
        }
    }

    /// 裁定 id（pure）: 問い id と発話の ts（秒とミリ秒を持つ字）から `<問い id>:<YYYYMMDDTHHMMZ>-1` を作り、秒とミリ秒は id に効かない
    /// （同じ分の発話は同じ id）・同じ入力で同じ字を返し・`is_ruling_id` に台帳の接頭辞を渡すと真になる。
    #[test]
    fn seat_ruling_id_is_minute_precise_deterministic_and_a_ruling_id() {
        let id = ruling_id("s2-q1.2", "2026-09-30T07:05:09.123Z");
        assert_eq!(id.as_deref(), Some("s2-q1.2:20260930T0705Z-1"), "問い id・発話の時分・-1");
        assert_eq!(ruling_id("s2-q1.2", "2026-09-30T07:05:09.123Z"), id, "同じ入力で同じ id");
        assert_eq!(ruling_id("s2-q1.2", "2026-09-30T07:05:59.999Z"), id, "秒とミリ秒は効かない");
        assert_ne!(ruling_id("s2-q1.2", "2026-09-30T07:06:00.000Z"), id, "分が違えば id も違う");
        assert_ne!(ruling_id("s2-q1.3", "2026-09-30T07:05:09.123Z"), id, "問い id が違えば id も違う");
        assert!(is_ruling_id(id.as_deref().unwrap_or_default(), Some("s2")), "台帳の接頭辞を渡すと裁定 id の形");
        assert_eq!(ruling_id("s2-q1", "2026-09-30T07:05:09Z").as_deref(), Some("s2-q1:20260930T0705Z-1"), "秒までの字面も読む");
    }

    /// 読めない入力は id を作らない（黙って別の字を作らない）: 形の崩れた ts・存在しない日時・接頭辞の無い問い id。
    #[test]
    fn seat_ruling_id_refuses_unreadable_input() {
        for utterance in ["", "2026-09-30", "2026-09-30T07", "2026-9-30T07:05:09Z", "2026-13-30T07:05:09Z", "2026-09-30T25:05:09Z", "xxxx-xx-xxTxx:xxZ"] {
            assert_eq!(ruling_id("s2-q1", utterance), None, "{utterance:?}");
        }
        for question in ["", "q1", "s2-", "s2-a b", "-q1"] {
            assert_eq!(ruling_id(question, "2026-09-30T07:05:09.123Z"), None, "{question:?}");
        }
    }

    /// `ruling` 欄の読み（pure）: `user <分>Z` は分、`user ` で始まるが分まで読めない形は `Some(None)`（skipped）、`user ` で
    /// 始まらない行は母集団の外（`None`）。
    #[test]
    fn fleet_ruling_minute_reads_only_minute_precise_user_rulings() {
        assert_eq!(ruling_minute("user 2026-09-17T07:30Z"), Some(Some("2026-09-17T07:30".to_owned())));
        assert_eq!(ruling_minute("user 2026-09-14T13:23Z bats in runner"), Some(Some("2026-09-14T13:23".to_owned())), "後ろの散文は読まない");
        for vague in ["user 2026-09-15T11:2xZ", "user 2026-09-14", "user 裁定 2026-09-10", "user 2026-09-17T07:30", "user "] {
            assert_eq!(ruling_minute(vague), Some(None), "{vague} は分まで読めない");
        }
        for outside in ["RULING-v2 論点 2", "grill U3", "r", "users 2026-09-17T07:30Z"] {
            assert_eq!(ruling_minute(outside), None, "{outside} は母集団の外");
        }
    }

    /// 突合（pure）: 同じ分の裁定が在る行は matched・無い行は id で名指し・分の曖昧な行は skipped・同じ分に 2 件在っても 1 行。
    #[test]
    fn fleet_ruling_tally_matches_rows_by_the_same_minute() {
        let rows = [
            row("a.one", "CoreLines", "1", true, "user 2026-09-17T07:30Z"),
            row("b.two", "CoreLines", "1", true, "user 2026-09-18T01:02Z"),
            row("c.vague", "CoreLines", "1", true, "user 2026-09-15T11:2xZ"),
            row("d.other", "CoreLines", "1", true, "grill U3"),
        ]
        .concat();
        let events = [ruling("2026-09-17T07:30:59Z"), ruling("2026-09-17T07:30:01Z"), ruling("2026-09-18T01:03:00Z")];
        let tally = Tally::of(&events, &manifest(&rows));
        assert_eq!(tally, Tally { rulings: 3, matched: 1, of: 2, unmatched: vec!["b.two".to_owned()], skipped: 1 });
        assert_eq!(tally.line(), "rulings=3 rule-rulings=1/2 unmatched=b.two skipped=1");
        let none = Tally::of(&[], &manifest(""));
        assert!(none.is_empty(), "数えるものの無い周");
        assert_eq!(Tally::of(&events, &manifest("")).line(), "rulings=3 rule-rulings=0/0 unmatched=- skipped=0");
    }

    /// 結びの形の裁定 1 件（裁定 id・発話の ts・問いの起票の時刻・asked を選ぶ）。
    fn bound(id: &str, utterance: &str, question_ts: &str, asked: Option<&str>) -> Event {
        let case = Case::Ruling {
            ruling: id.to_owned(),
            utterance: utterance.to_owned(),
            channel: Channel::Chat,
            question_ts: question_ts.to_owned(),
            asked: asked.map(str::to_owned),
        };
        Event { case: Some(case), ..ruling("2026-09-30T09:00:00.000Z") }
    }

    /// 発話（秒より下の桁を持つ）と秒までの question_ts の比べ: 発話の秒の後の question_ts は後・前は後でない・同じ秒（ミリ秒が 999 でも
    /// 0 でも）は後に数えない。字面で比べると崩れる桁の数の違い（`.5Z` と `:10Z`）の形でも秒で比べる。
    #[test]
    fn doctor_asked_after_compares_seconds_truncating_the_utterance_and_skips_the_same_second() {
        let said = "2026-09-30T07:05:09.999Z";
        let events = [
            bound("a:1", said, "2026-09-30T07:05:10Z", Some("seat")),
            bound("b:1", said, "2026-09-30T07:05:09Z", Some("seat")),
            bound("c:1", "2026-09-30T07:05:09.000Z", "2026-09-30T07:05:09Z", Some("user")),
            bound("d:1", said, "2026-09-30T07:05:08Z", Some("user")),
            bound("e:1", "2026-09-30T07:05:09.001Z", "2026-09-30T07:06:00Z", Some("user")),
        ];
        let found = AskedAfter::of(&events);
        assert_eq!((found.binds, found.unreadable), (5, 0));
        assert_eq!((found.after_seat, found.after_user), (vec!["a:1".to_owned()], vec!["e:1".to_owned()]));
    }

    /// 母集団は結びの形（case が Ruling）だけ・asked の無い結びは前後に依らず全部 asked-none に log の順で並び、
    /// 読めない時刻（question_ts・発話のどちらも）は binds と asked-none に数えて after-* に入れず unreadable に数える。
    #[test]
    fn doctor_asked_after_counts_the_population_and_keeps_unreadable_times_out_of_after() {
        let events = [
            ruling("2026-09-30T07:05:09.123Z"),
            bound("z:1", "2026-09-30T07:05:09.123Z", "2026-09-30T08:00:00Z", None),
            bound("m:1", "2026-09-30T07:05:09.123Z", "2026-09-30T06:00:00Z", None),
            bound("s:1", "2026-09-30T07:05:09.123Z", "not-a-time", Some("seat")),
            bound("u:1", "2026-09-30T07:05:09Z", "2026-09-30T08:00:00Z", Some("user")),
            bound("n:1", "2026-09-30T07:05:09.123Z", "bad", None),
        ];
        let found = AskedAfter::of(&events);
        assert_eq!((found.binds, found.unreadable), (5, 3), "古い裁定は数えない・秒の発話の ts も読めない");
        assert_eq!(found.asked_none, ["z:1", "m:1", "n:1"], "log の順");
        assert!(found.after_seat.is_empty() && found.after_user.is_empty(), "読めない結びは after に入れない");
        assert_eq!(
            found.line(),
            "binds=5 after-seat=0 after-seat-ids=- after-user=0 after-user-ids=- asked-none=3 asked-none-ids=z:1,m:1,n:1 unreadable=3"
        );
        assert_eq!(AskedAfter::of(&[ruling("2026-09-30T07:05:09.123Z")]).binds, 0, "古い裁定だけの母集団は 0");
    }
}
