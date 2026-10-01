//! 局面の出力の書き手と全部の書き直し（設計 docs/design/case-lifecycle.md §12・FR90・FR94・ADR-0088・ADR-0100）。
//!
//! 書き手は 1 本（[`write`]）で、出力の型の値（[`Output`]）と scope を受けて `<state_dir>/fleet/lifecycle.json` を丸ごと入れ替える:
//! `lifecycle.lock` を死んだ所有者だけ外す取り方で取り（生きた所有者は古くても外さず [`Wrote::Busy`]）、一時 file に書いて
//! fsync の後 rename する（書きかけは読み手の名に見えない）。rename の前に今の file の入力の印を読み、どれかが自分の印より新しければ
//! 書かず（[`Wrote::Discarded`]・順を持たない組は古くないと読む）、中身が今の file と `generated_at` の他に同じなら rename しない
//! （[`Wrote::Unchanged`]）。出力を型の値へ読み戻す読み（[`read_output`]）と、年齢と `owned` を数え直す関数（[`count_owned`]）を
//! 後の行（部分の書き直し・読み手）のために兄弟の module から呼べる可視性で置く。
//!
//! 全部の書き直し（[`full`]）は lock を取ってから FR90 の 5 入力（台帳の全部・event log・main の設計 doc の契約表と SRS・main の
//! commit の trailer・main の先端の宣言）を読み、行 a1・a2・b・b1 の純関数で部品を導いて [`write`] の中身を撃つ。読めない周は
//! 書かず、`unreadable` の印に理由（`ledger`・`events`・`main`・`table`・`srs`・`declaration` の最初の 1 語）を持たせる。
//! `Written` か `Unchanged` の周にだけ、json の rename の後に古さの印を消し、線を記帳する。

use super::json_tree::{self, Tree};
use super::lifecycle_line::{book_lines, read_lines, Lines as EventLines};
use super::lifecycle_mark::{
    self as mark, bindings_of, events_of, events_order, events_tree, fleet_dir, hold, is_open_contract, ledger_is_newer, ledger_of, ledger_order,
    ledger_tree, latest_runs, main_is_descendant, main_of, main_order, num, num_of, open_write_set, publish, read_commits, read_marks, read_rows,
    read_srs, read_stale, refusals_of, secs_of, Face, Kind as MarkKind, Ledger, Marks, Stale, JSON_FILE, JSON_LOCK,
};
use super::phase::{self as run_phase, Judged, OpenContract};
use super::store::{self, LockPolicy};
use super::{cli::format_utc, epoch_of, replay, Event};
use crate::cli_outcome::{Outcome, RC_REFUSED};
use crate::case::{
    turn_of, Channel, Destination, Extra, Kind, Links, Misfit, Part, Phase, Sink, Turn, TriggerView, COMMON_KEYS, LINK_KEYS, PHASES,
};
use crate::ledger::citation::prefix_of;
use crate::ledger::form::pointer_text;
use crate::ledger::phase::{self as ledger_phase, Lines as LineTimes};
use crate::ledger::phase_main::{self, Commit, Row};
use crate::ledger::phase_ruling;
use crate::pipe::declaration::{close_check_at_sha, CloseCheck};
use crate::rules::int_row;
use crate::rules::manifest::Manifest;
use crate::seat::ledger::Issue;
use std::collections::BTreeMap;
use std::path::Path;

/// 書き直しの範囲。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// 台帳と main から判じた部分を作った全部の書き直し。
    Full,
    /// 部分の書き直し（event log の末尾だけから足す）。
    Partial,
}

impl Scope {
    /// 出力の字。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
        }
    }
}

/// 最古の 1 件（`owned.oldest`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Oldest {
    /// 種類。
    pub part: Kind,
    /// id。
    pub id: String,
    /// 局面。
    pub phase: Phase,
    /// その局面に入った時刻。
    pub since: String,
}

/// 席の手番の閾値越えの数え（`owned`・requirement はどれにも数えない）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Owned {
    /// 閾値を越えた件数。
    pub count: u64,
    /// 手番が seat で閾値の行が無い件数。
    pub unset: u64,
    /// 閾値の行が在って since が無い件数。
    pub unknown: u64,
    /// 越えた件のうち最も古い 1 件。
    pub oldest: Option<Oldest>,
}

/// 局面の出力の型（§5.2・`lifecycle.json` 1 つぶん）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// 生成の時刻（UTC の秒まで）。
    pub generated_at: String,
    /// 範囲。
    pub scope: Scope,
    /// 台帳と main から判じた部分を作った全部の書き直しの時刻。
    pub full_at: String,
    /// 部分の書き直しの周期の約束（無ければ `None`）。
    pub interval_s: Option<u64>,
    /// rules 行 `lifecycle.closed_window_h` の写し。
    pub closed_window_h: Option<u64>,
    /// 入力の印。
    pub marks: Marks,
    /// 測れなかった種類と理由の語。
    pub unmeasured: Vec<(String, String)>,
    /// 席の手番の閾値越えの数え。
    pub owned: Owned,
    /// 部品。
    pub parts: Vec<Part>,
}

/// 出力の読み戻し（閉じた 3 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    /// `lifecycle.json` が無い。
    Absent,
    /// 在るが読める形でない（版が違う・形が違う・JSON でない）。
    Unreadable,
    /// 読めた出力（知らない語の部品は落とす）。
    Read(Box<Output>),
}

/// 書きの返り（閉じた 6 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrote {
    /// 書いた（rename した）。
    Written,
    /// 中身が同じで rename しなかった。
    Unchanged,
    /// 撃った時の印より古くない file が在り、書き直さなかった。
    Coalesced,
    /// 今の file の印が自分の印より新しく、書かなかった。
    Discarded,
    /// `lifecycle.lock` を取れなかった。
    Busy,
    /// 入力を読めない（語は読む順の最初の 1 つ）か、書き出せなかった（`write`）。
    Unreadable(&'static str),
}

impl Wrote {
    /// 返りの語（`lifecycle=<語>` の字）。
    pub fn word(self) -> &'static str {
        match self {
            Self::Written => "written",
            Self::Unchanged => "unchanged",
            Self::Coalesced => "coalesced",
            Self::Discarded => "discarded",
            Self::Busy => "busy",
            Self::Unreadable(_) => "unreadable",
        }
    }

    /// 呼び手が stderr に 1 行出す語（`Written`・`Unchanged`・`Coalesced` の外だけ）。
    pub fn loud(self) -> Option<&'static str> {
        match self {
            Self::Written | Self::Unchanged | Self::Coalesced => None,
            Self::Discarded | Self::Busy | Self::Unreadable(_) => Some(self.word()),
        }
    }
}

/// 文字列の欄。
fn text(value: &str) -> Tree {
    Tree::Str(value.to_owned())
}

/// 文字列か null の欄。
fn maybe(value: &Option<String>) -> Tree {
    value.clone().map_or(Tree::Null, Tree::Str)
}

/// 数か null の欄。
fn maybe_num(value: Option<u64>) -> Tree {
    value.map_or(Tree::Null, num)
}

/// key の順を保つ object。
fn object(items: Vec<(&str, Tree)>) -> Tree {
    Tree::Object(items.into_iter().map(|(key, value)| (key.to_owned(), value)).collect())
}

/// 文字列の列。
fn strings(items: &[String]) -> Tree {
    Tree::Array(items.iter().map(|item| text(item)).collect())
}

/// `links` の木（空の key は省く・key の順は [`LINK_KEYS`]）。
fn links_tree(links: &Links) -> Tree {
    let destination = links.destination.iter().map(|found| {
        let id = found.id.as_ref().map_or(Tree::Null, |id| text(id));
        object(vec![("to", text(found.to.as_str())), ("id", id)])
    });
    let values: [(bool, Tree); 8] = [
        (!links.source.is_empty(), strings(&links.source)),
        (!links.questions.is_empty(), strings(&links.questions)),
        (!links.rulings.is_empty(), strings(&links.rulings)),
        (!links.promoted.is_empty(), strings(&links.promoted)),
        (!links.runs.is_empty(), strings(&links.runs)),
        (!links.commits.is_empty(), strings(&links.commits)),
        (!links.destination.is_empty(), Tree::Array(destination.collect())),
        (!links.on.is_empty(), strings(&links.on)),
    ];
    let kept = LINK_KEYS.iter().zip(values).filter(|(_, (present, _))| *present).map(|(key, (_, tree))| ((*key).to_owned(), tree));
    Tree::Object(kept.collect())
}

/// 種類ごとの欄（§5.2・memo の `triggers` と `keep` は任意）。
fn extra_pairs(extra: &Extra) -> Vec<(&'static str, Tree)> {
    match extra {
        Extra::None => Vec::new(),
        Extra::Utterance { session, channel } => vec![("session", maybe(session)), ("channel", text(channel.as_str()))],
        Extra::Memo { due, triggers, keep } => {
            let view = |found: &TriggerView| {
                object(vec![("form", text(&found.form)), ("value", text(&found.value)), ("met", Tree::Bool(found.met))])
            };
            let mut items = vec![("due", maybe(due))];
            items.extend(triggers.as_ref().map(|found| ("triggers", Tree::Array(found.iter().map(view).collect()))));
            items.extend(keep.map(|found| ("keep", Tree::Bool(found))));
            items
        }
        Extra::Contract { pointer } => vec![("pointer", maybe(pointer))],
        Extra::Run { bead } => vec![("bead", text(bead))],
    }
}

/// 部品 1 つの木（共通の 9 key は [`COMMON_KEYS`] の順に書く）。
fn part_tree(part: &Part) -> Tree {
    let common = [
        text(part.part.as_str()),
        text(&part.id),
        text(part.phase.as_str()),
        text(part.turn.as_str()),
        maybe(&part.since),
        maybe(&part.reason),
        Tree::Bool(part.closed),
        part.overdue.map_or(Tree::Null, Tree::Bool),
        links_tree(&part.links),
    ];
    let mut items: Vec<(String, Tree)> = COMMON_KEYS.iter().map(|key| (*key).to_owned()).zip(common).collect();
    items.extend(extra_pairs(&part.extra).into_iter().map(|(key, value)| (key.to_owned(), value)));
    Tree::Object(items)
}

/// 出力の木（§5.2）。
pub fn output_tree(out: &Output) -> Tree {
    let unmeasured = out.unmeasured.iter().map(|(part, reason)| object(vec![("part", text(part)), ("reason", text(reason))]));
    let oldest = out.owned.oldest.as_ref().map_or(Tree::Null, |found| {
        object(vec![("part", text(found.part.as_str())), ("id", text(&found.id)), ("phase", text(found.phase.as_str())), ("since", text(&found.since))])
    });
    let owned = object(vec![("count", num(out.owned.count)), ("unset", num(out.owned.unset)), ("unknown", num(out.owned.unknown)), ("oldest", oldest)]);
    let inputs = object(vec![
        ("ledger", ledger_tree(&out.marks.ledger)),
        ("events", events_tree(&out.marks.events)),
        ("main", mark::main_tree(&out.marks.main)),
    ]);
    object(vec![
        ("version", num(1)),
        ("generated_at", text(&out.generated_at)),
        ("scope", text(out.scope.as_str())),
        ("full_at", text(&out.full_at)),
        ("interval_s", maybe_num(out.interval_s)),
        ("closed_window_h", maybe_num(out.closed_window_h)),
        ("inputs", inputs),
        ("unmeasured", Tree::Array(unmeasured.collect())),
        ("phases", Tree::Array(PHASES.iter().map(|word| text(word)).collect())),
        ("owned", owned),
        ("parts", Tree::Array(out.parts.iter().map(part_tree).collect())),
    ])
}

/// 出力の本文（末尾に改行）。
pub fn render_output(out: &Output) -> String {
    format!("{}\n", json_tree::render(&output_tree(out)))
}

/// 字の欄か null（欠けた key と型違いは `None`）。
fn optional_text(node: &Tree, key: &str) -> Option<Option<String>> {
    match node.get(key)? {
        Tree::Null => Some(None),
        Tree::Str(found) => Some(Some(found.clone())),
        _ => None,
    }
}

/// 文字列の列の key（欠けた key は空の列・型違いは `None`）。
fn strings_of(node: &Tree, key: &str) -> Option<Vec<String>> {
    match node.get(key) {
        None => Some(Vec::new()),
        Some(found) => found.as_array()?.iter().map(|item| item.as_str().map(str::to_owned)).collect(),
    }
}

/// `links` を木から読む（欠けた key は空の列・読み手は知らない key を無視する）。
fn links_of(node: &Tree) -> Option<Links> {
    let destination = match node.get("destination") {
        None => Vec::new(),
        Some(found) => {
            let item = |entry: &Tree| {
                let word = entry.get("to")?.as_str()?;
                Some(Destination { to: Sink::ALL.into_iter().find(|sink| sink.as_str() == word)?, id: optional_text(entry, "id")? })
            };
            found.as_array()?.iter().map(item).collect::<Option<Vec<_>>>()?
        }
    };
    Some(Links {
        source: strings_of(node, "source")?,
        questions: strings_of(node, "questions")?,
        rulings: strings_of(node, "rulings")?,
        promoted: strings_of(node, "promoted")?,
        runs: strings_of(node, "runs")?,
        commits: strings_of(node, "commits")?,
        destination,
        on: strings_of(node, "on")?,
    })
}

/// 種類ごとの欄を木から読む。
fn extra_of(kind: Kind, node: &Tree) -> Option<Extra> {
    Some(match kind {
        Kind::Utterance => {
            let word = node.get("channel")?.as_str()?;
            Extra::Utterance { session: optional_text(node, "session")?, channel: Channel::ALL.into_iter().find(|found| found.as_str() == word)? }
        }
        Kind::Memo => {
            let view = |entry: &Tree| {
                Some(TriggerView { form: entry.get("form")?.as_str()?.to_owned(), value: entry.get("value")?.as_str()?.to_owned(), met: entry.get("met")?.as_bool()? })
            };
            let triggers = match node.get("triggers") {
                None => None,
                Some(found) => Some(found.as_array()?.iter().map(view).collect::<Option<Vec<_>>>()?),
            };
            let keep = match node.get("keep") {
                None => None,
                Some(found) => Some(found.as_bool()?),
            };
            Extra::Memo { due: optional_text(node, "due")?, triggers, keep }
        }
        Kind::Contract => Extra::Contract { pointer: optional_text(node, "pointer").unwrap_or(None) },
        Kind::Run => Extra::Run { bead: node.get("bead")?.as_str()?.to_owned() },
        Kind::Question | Kind::Row | Kind::Requirement | Kind::Epic | Kind::Commit => Extra::None,
    })
}

/// 部品 1 つを木から読む（知らない語・欠けた欄は `None`）。
fn part_of(node: &Tree) -> Option<Part> {
    let word = |key: &str| node.get(key).and_then(Tree::as_str);
    let (kind_word, turn_word) = (word("part")?, word("turn")?);
    let kind = Kind::ALL.into_iter().find(|found| found.as_str() == kind_word)?;
    let overdue = match node.get("overdue")? {
        Tree::Null => None,
        other => Some(other.as_bool()?),
    };
    Some(Part {
        part: kind,
        id: word("id")?.to_owned(),
        phase: Phase::of(word("phase")?)?,
        turn: Turn::ALL.into_iter().find(|found| found.as_str() == turn_word)?,
        since: optional_text(node, "since")?,
        reason: optional_text(node, "reason")?,
        closed: node.get("closed")?.as_bool()?,
        overdue,
        links: links_of(node.get("links")?)?,
        extra: extra_of(kind, node)?,
    })
}

/// `owned` を木から読む。
fn owned_of(node: &Tree) -> Option<Owned> {
    let oldest = match node.get("oldest")? {
        Tree::Null => None,
        found => {
            let word = |key: &str| found.get(key).and_then(Tree::as_str);
            let kind_word = word("part")?;
            Some(Oldest {
                part: Kind::ALL.into_iter().find(|kind| kind.as_str() == kind_word)?,
                id: word("id")?.to_owned(),
                phase: Phase::of(word("phase")?)?,
                since: word("since")?.to_owned(),
            })
        }
    };
    Some(Owned { count: num_of(node.get("count")?)?, unset: num_of(node.get("unset")?)?, unknown: num_of(node.get("unknown")?)?, oldest })
}

/// 数か null の欄。
fn optional_num(node: &Tree, key: &str) -> Option<Option<u64>> {
    match node.get(key)? {
        Tree::Null => Some(None),
        found => Some(Some(num_of(found)?)),
    }
}

/// 出力を木から読む（版が 1 でない周は `None`・知らない語の部品は落とす）。
pub fn output_of(tree: &Tree) -> Option<Output> {
    if !matches!(tree.get("version"), Some(Tree::Num(version)) if version == "1") {
        return None;
    }
    let inputs = tree.get("inputs")?;
    let unmeasured = tree.get("unmeasured")?.as_array()?.iter().map(|entry| {
        Some((entry.get("part")?.as_str()?.to_owned(), entry.get("reason")?.as_str()?.to_owned()))
    });
    let scope = match tree.get("scope")?.as_str()? {
        "full" => Scope::Full,
        "partial" => Scope::Partial,
        _ => return None,
    };
    Some(Output {
        generated_at: tree.get("generated_at")?.as_str()?.to_owned(),
        scope,
        full_at: tree.get("full_at")?.as_str()?.to_owned(),
        interval_s: optional_num(tree, "interval_s")?,
        closed_window_h: optional_num(tree, "closed_window_h")?,
        marks: Marks { ledger: ledger_of(inputs.get("ledger")?)?, events: events_of(inputs.get("events")?)?, main: main_of(inputs.get("main")?)? },
        unmeasured: unmeasured.collect::<Option<Vec<_>>>()?,
        owned: owned_of(tree.get("owned")?)?,
        parts: tree.get("parts")?.as_array()?.iter().filter_map(part_of).collect(),
    })
}

/// 出力を型の値へ読み戻す（閉じた 3 値・後の行が呼ぶ・読み手が見る名は `lifecycle.json` だけ）。
pub fn read_output(state_dir: &Path) -> Reading {
    let body = match std::fs::read_to_string(fleet_dir(state_dir).join(JSON_FILE)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Reading::Absent,
        Err(_) => return Reading::Unreadable,
    };
    match json_tree::parse(&body).ok().as_ref().and_then(output_of) {
        Some(found) => Reading::Read(Box::new(found)),
        None => Reading::Unreadable,
    }
}

/// 年齢（`generated_at` − `since`）と `owned` を数え直す（後の行が呼ぶ）。手番が seat で `hours` が値を返す語の部品は、`since` が在れば
/// 越えたかを `overdue` に置く。ほかの `overdue` は `None`。`owned.unset` は行の無い seat の語（requirement はどれにも数えない）。
pub fn count_owned(parts: &mut [Part], hours: &dyn Fn(&str) -> Option<u64>, now: u64) -> Owned {
    let mut owned = Owned::default();
    let mut oldest: Option<(u64, usize)> = None;
    for (at, part) in parts.iter_mut().enumerate() {
        part.overdue = None;
        if part.turn != Turn::Seat || part.part == Kind::Requirement {
            continue;
        }
        let Some(limit) = hours(part.phase.as_str()) else {
            owned.unset += 1;
            continue;
        };
        let Some(since) = part.since.as_deref().and_then(epoch_of) else {
            owned.unknown += 1;
            continue;
        };
        let over = now.saturating_sub(since) > limit.saturating_mul(3_600);
        part.overdue = Some(over);
        if over {
            owned.count += 1;
            oldest = oldest.filter(|(best, _)| *best <= since).or(Some((since, at)));
        }
    }
    owned.oldest = oldest.and_then(|(_, at)| parts.get(at)).and_then(|part| {
        Some(Oldest { part: part.part, id: part.id.clone(), phase: part.phase, since: part.since.clone()? })
    });
    owned
}

/// 今の file の印が自分の印より新しいか（どれか 1 つでも大きければ真・順を持たない組は古くないと読む）。
fn newer_than_mine(repo: &Path, current: &Marks, mine: &Marks) -> bool {
    ledger_order(&current.ledger, &mine.ledger) == Some(std::cmp::Ordering::Greater)
        || events_order(&current.events, &mine.events) == Some(std::cmp::Ordering::Greater)
        || main_order(repo, &current.main, &mine.main) == Some(std::cmp::Ordering::Greater)
}

/// 今の file の印が撃った時の印より古いか（どれか 1 つでも小さければ真・順を持たない組は古くないと読む）。
fn older_than_taken(repo: &Path, current: &Marks, taken: &Marks) -> bool {
    ledger_order(&current.ledger, &taken.ledger) == Some(std::cmp::Ordering::Less)
        || events_order(&current.events, &taken.events) == Some(std::cmp::Ordering::Less)
        || main_order(repo, &current.main, &taken.main) == Some(std::cmp::Ordering::Less)
}

/// lock を取った書き手の本体: 今の file の印を読み、捨てる・rename しない・rename するを決める。
fn publish_locked(state_dir: &Path, repo: &Path, out: &Output) -> Wrote {
    let current = read_output(state_dir);
    if let Reading::Read(found) = &current {
        if newer_than_mine(repo, &found.marks, &out.marks) {
            return Wrote::Discarded;
        }
        let full_at = if out.scope == Scope::Full { out.full_at.clone() } else { found.full_at.clone() };
        if (Output { generated_at: out.generated_at.clone(), full_at, ..(**found).clone() }) == *out {
            return Wrote::Unchanged;
        }
    }
    match publish(&fleet_dir(state_dir), JSON_FILE, &render_output(out)) {
        Ok(()) => Wrote::Written,
        Err(_) => Wrote::Unreadable("write"),
    }
}

/// 後の行が呼ぶ書き手 1 本（出力の型の値と範囲を受け、同じ 6 値を返す）。`wait_ms` だけ lock を待つ。全部の書き直しの後始末
/// （古さの印・線）は [`full`] が持ち、部分の書き直しは印を消さない。
pub fn write(place: &Place<'_>, out: &Output, wait_ms: Option<u64>) -> Wrote {
    let policy = LockPolicy { retry_ms: wait_ms.unwrap_or(place.policy.retry_ms), stale_ms: place.policy.stale_ms };
    let Some(_held) = hold(&fleet_dir(place.state_dir), JSON_LOCK, policy) else { return Wrote::Busy };
    publish_locked(place.state_dir, place.repo, out)
}

/// 書き直しの材料の置き場（すべて永続面から解いたもの）。
pub struct Place<'a> {
    /// 置き場。
    pub state_dir: &'a Path,
    /// 対象 repo（`--repo`・台帳は `<repo>/.beads`・main は `origin/main`）。
    pub repo: &'a Path,
    /// 規則の値。
    pub manifest: &'a Manifest,
    /// 台帳 client。
    pub bd: &'a str,
    /// lock の待ち方（rules 行 `fleet.lock_retry_ms` と `fleet.lock_stale_ms`）。
    pub policy: LockPolicy,
}

/// 契機 (a) が借りる `fire` の 1 回の読み（台帳の全件と列の 1 周の判定）。
pub struct Round<'a> {
    /// 台帳の全件。
    pub issues: &'a [Issue],
    /// 列の 1 周の判定。
    pub judged: Vec<Judged>,
}

/// 台帳と列の判定の出所。
pub enum Source<'a> {
    /// `fire` の 1 回の読みを借りる（契機 (a)）。
    Borrowed(Round<'a>),
    /// 観測の 1 周（`dispatch ls` と同じ判定・起こさない）を撃つ（契機 (d)(e)）。
    Observe,
}

/// 全部の書き直しの頼み方。
#[derive(Debug, Clone, Copy, Default)]
pub struct Request {
    /// lock を待つ上限（無ければ rules 行の値）。
    pub wait_ms: Option<u64>,
    /// 撃った時の印より古くない file が在れば書き直さず `Coalesced` を返す（口の契機）。
    pub coalesce: bool,
}

/// 契機の口（終端の close の後・`fire` の後）が撃つ 1 本: 全部を書き直し、`Written`・`Unchanged`・`Coalesced` の外の語だけを返す。
pub fn round(place: &Place<'_>, source: Source<'_>) -> Option<&'static str> {
    full(place, source, Request::default()).loud()
}

/// 契機 (d)（land の終端の close の Ok の後）の 1 本: 観測の 1 周（起こさない）を撃って全部を書き直し、`Written`・`Unchanged`・
/// `Coalesced` の外の語だけを stderr の `lifecycle=<語>` の 1 行にして返す（呼び手の rc と stdout は変えない・crate の中から呼べる）。
pub(crate) fn after_close(place: &Place<'_>) -> Vec<String> {
    round(place, Source::Observe).map(|word| format!("lifecycle={word}")).into_iter().collect()
}

/// 全部の書き直し 1 回（設計 §12 約束 3・4・6・7）。
pub fn full(place: &Place<'_>, source: Source<'_>, request: Request) -> Wrote {
    let taken = read_marks(place.state_dir, place.repo).ok();
    let policy = LockPolicy { retry_ms: request.wait_ms.unwrap_or(place.policy.retry_ms), stale_ms: place.policy.stale_ms };
    let Some(_held) = hold(&fleet_dir(place.state_dir), JSON_LOCK, policy) else { return Wrote::Busy };
    let start = crate::seat::state::now_secs();
    if let (true, Some(taken), Reading::Read(found)) = (request.coalesce, taken.as_ref(), read_output(place.state_dir)) {
        if !older_than_taken(place.repo, &found.marks, taken) {
            return Wrote::Coalesced;
        }
    }
    let world = match gather(place, source) {
        Ok(found) => found,
        Err(word) => {
            let at = format_utc(start);
            let value = mark::Mark { kind: MarkKind::Unreadable, at, value: mark::Value::Reason(word.to_owned()) };
            let _ = mark::add_mark(place.state_dir, &value, place.policy);
            return Wrote::Unreadable(word);
        }
    };
    let out = derive(place, &world, start);
    let wrote = publish_locked(place.state_dir, place.repo, &out);
    if matches!(wrote, Wrote::Written | Wrote::Unchanged) {
        after_full(place, &world, start);
    }
    wrote
}

/// `Written` か `Unchanged` の全部の書き直しの後始末: json の rename の後に古さの印を消し、線を記帳する。
fn after_full(place: &Place<'_>, world: &World, start: u64) {
    let gone = |found: &mark::Mark| {
        found.at_secs().is_some_and(|at| start > at)
            && match &found.value {
                mark::Value::Ledger(old) => ledger_is_newer(&world.marks.ledger, old),
                mark::Value::Main(old) => main_is_descendant(place.repo, &world.marks.main, old),
                mark::Value::Reason(_) => true,
            }
    };
    let _ = mark::settle(place.state_dir, gone, place.policy);
    let _ = book_lines(place.state_dir, &world.marks.main, world.close_check, place.policy);
}

/// 読んだ世界（FR90 の 5 入力と入力の印）。
struct World {
    /// 入力の印。
    marks: Marks,
    /// 台帳の全件。
    issues: Vec<Issue>,
    /// event log。
    events: Vec<Event>,
    /// 列の 1 周の判定。
    judged: Vec<Judged>,
    /// 契約表の行（無いか読めない形なら `None`）。
    rows: Option<Vec<Row>>,
    /// 開いた契約の write-set の項目。
    write_set: Vec<String>,
    /// 要件 id（無いか読めない形なら `None`）。
    requirements: Option<Vec<String>>,
    /// 切り替えの線より後の main の commit（古い順）。
    commits: Vec<Commit>,
    /// 2 つの線。
    lines: EventLines,
    /// main の先端の宣言が close-check を true で持つか。
    close_check: bool,
    /// 台帳の接頭辞。
    prefix: Option<String>,
}

/// 台帳と列の判定の出所を読む（読めなければ `ledger`）。
fn ledger_side(place: &Place<'_>, source: Source<'_>) -> Result<(Vec<Issue>, Vec<Judged>), &'static str> {
    match source {
        Source::Borrowed(round) => Ok((round.issues.to_vec(), round.judged)),
        Source::Observe => {
            let observed = crate::pipe::dispatch::observe_round(place.state_dir, place.repo, place.manifest, place.bd).map_err(|_| "ledger")?;
            Ok((observed.issues, observed.judged))
        }
    }
}

/// 5 入力を読む（読む順は `ledger`・`events`・`main`・`table`・`srs`・`declaration`・最初に読めなかった語を返す）。
fn gather(place: &Place<'_>, source: Source<'_>) -> Result<World, &'static str> {
    let ledger = mark::read_ledger(place.repo).ok_or("ledger")?;
    let (issues, judged) = ledger_side(place, source)?;
    let events_mark = mark::read_events(place.state_dir).ok_or("events")?;
    let events = store::read_all(place.state_dir).map_err(|_| "events")?;
    let main = mark::read_main(place.repo).ok_or("main")?;
    let table = read_rows(place.repo, &main);
    let srs = read_srs(place.repo, &main);
    let (Face::Found(_) | Face::Missing, Face::Found(_) | Face::Missing) = (&table, &srs) else {
        return Err(if matches!(table, Face::Fault) { "table" } else { "srs" });
    };
    let close_check = match close_check_at_sha(place.repo, &main) {
        CloseCheck::Unreadable => return Err("declaration"),
        found => found == CloseCheck::Joins,
    };
    let lines = read_lines(&events);
    let commits = match lines.cutover.as_ref() {
        Some(line) => read_commits(place.repo, &line.main, &main, &issues).ok_or("main")?,
        None => Vec::new(),
    };
    let (rows, write_sets) = match table {
        Face::Found(found) => (Some(found.0), found.1),
        Face::Missing | Face::Fault => (None, Vec::new()),
    };
    Ok(World {
        write_set: open_write_set(&issues, &write_sets),
        prefix: prefix_of(place.repo),
        marks: Marks { ledger, events: events_mark, main },
        issues,
        events,
        judged,
        rows,
        requirements: match srs {
            Face::Found(found) => Some(found),
            Face::Missing | Face::Fault => None,
        },
        commits,
        lines,
        close_check,
    })
}

/// 窓と年齢と周期の規則（行が無いか読めない周は `None`・窓は掛けない側へ倒す）。
struct Conf<'a> {
    /// 窓の秒。
    window_s: u64,
    /// 窓の時間（出力の写し）。
    window_h: Option<u64>,
    /// 規則の値。
    manifest: &'a Manifest,
}

/// 部品を導いて出力の型の値にする（行 a1・a2・b・b1 の関数を呼び、置き換え・結び・継ぎ・窓・`owned` を足す）。
fn derive(place: &Place<'_>, world: &World, now: u64) -> Output {
    let window_h = int_row(place.manifest, "lifecycle.closed_window_h").ok();
    let conf = Conf { window_s: window_h.map_or(u64::MAX, |hours| hours.saturating_mul(3_600)), window_h, manifest: place.manifest };
    let generated_at = format_utc(now);
    let cutover = world.lines.cutover.as_ref().and_then(|line| secs_of(&line.ts));
    let times = LineTimes { cutover, close_check: world.lines.close_check.as_ref().and_then(|line| secs_of(&line.ts)) };
    let prefix = world.prefix.as_deref();
    let base = ledger_phase::derive(&ledger_phase::Input {
        issues: &world.issues,
        prefix,
        now,
        window_s: conf.window_s,
        lines: times,
        unreflected: &[],
        unjudged: &[],
        write_set: &world.write_set,
    });
    let mut parts = base.parts;
    let mut unmeasured = base.unmeasured;
    let bound = bindings_of(&world.events);
    let misfits = phase_ruling::derive(&phase_ruling::Input { issues: &world.issues, prefix, lines: times, bound: &bound });
    replace_with_misfits(&mut parts, &misfits);
    parts.extend(run_parts(place, world, &conf, &generated_at));
    let runs: Vec<String> = replay(&world.events).runs.into_keys().collect();
    let main = phase_main::derive(&phase_main::Input {
        commits: &world.commits,
        cutover: cutover.unwrap_or(now),
        runs: &runs,
        issues: &world.issues,
        rows: world.rows.as_deref(),
        requirements: world.requirements.as_deref(),
    });
    unmeasured.extend(main.unmeasured);
    tie_commits(&mut parts, &main.ties);
    parts.extend(main.parts);
    end_window(&mut parts, &conf, now);
    let prior = read_output(place.state_dir);
    inherit_since(&mut parts, &prior, &generated_at);
    link_utterances(&mut parts);
    let hours = |word: &str| int_row(conf.manifest, &format!("lifecycle.age_h.{word}")).ok();
    let owned = count_owned(&mut parts, &hours, now);
    Output {
        full_at: generated_at.clone(),
        generated_at,
        scope: Scope::Full,
        interval_s: int_row(conf.manifest, "seat.tick_interval_s").ok(),
        closed_window_h: conf.window_h,
        marks: world.marks.clone(),
        unmeasured: unmeasured.into_iter().map(|(kind, reason)| (kind.as_str().to_owned(), reason.to_owned())).collect(),
        owned,
        parts,
    }
}

/// 行 a2 の関数が名指した閉じた部品を misfit に置き換える（同じ id の部品は 1 つのまま・局面は misfit・理由は a2 の語）。
fn replace_with_misfits(parts: &mut [Part], misfits: &[(String, Misfit)]) {
    for (id, word) in misfits {
        if let Some(part) = parts.iter_mut().find(|part| part.id == *id) {
            part.phase = Phase::Misfit;
            part.turn = turn_of(Phase::Misfit.as_str(), None).unwrap_or(Turn::Seat);
            part.reason = Some(word.as_str().to_owned());
        }
    }
}

/// 開いた契約・その最新の便・線より後の発話の部品（行 b の関数・列の判定は材料が持つ）。
fn run_parts(place: &Place<'_>, world: &World, conf: &Conf<'_>, generated_at: &str) -> Vec<Part> {
    let state = replay(&world.events);
    let open: Vec<OpenContract> = world
        .issues
        .iter()
        .filter(|issue| is_open_contract(issue))
        .map(|issue| OpenContract { bead: issue.id.clone(), pointer: pointer_text(&issue.acceptance).map(str::to_owned) })
        .collect();
    let run_beads: BTreeMap<String, String> = state.runs.iter().map(|(id, run)| (id.clone(), run.bead.clone())).collect();
    run_phase::phases(&run_phase::Input {
        queue: &world.judged,
        open: &open,
        latest: &latest_runs(place.state_dir, &world.events, &state),
        run_beads: &run_beads,
        refusals: &refusals_of(&world.events),
        events: &world.events,
        line: world.lines.cutover.as_ref().map(|line| line.ts.as_str()),
        now: generated_at,
        window_s: conf.window_s,
    })
}

/// 行 b1 が返す結び（便の run id と契約の bead id ごとの commit の sha）を便と契約の `links.commits` に写す。
fn tie_commits(parts: &mut [Part], ties: &phase_main::Ties) {
    for part in parts.iter_mut() {
        let shas = match part.part {
            Kind::Run => ties.runs.get(&part.id),
            Kind::Contract => ties.beads.get(&part.id),
            _ => None,
        };
        if let Some(shas) = shas {
            part.links.commits = shas.clone();
        }
    }
}

/// 終わりの局面の row と commit に窓を掛ける（b1 は窓を持たない・since が無い部品は載せる側へ倒す）。
fn end_window(parts: &mut Vec<Part>, conf: &Conf<'_>, now: u64) {
    parts.retain(|part| {
        let ends = matches!(part.phase, Phase::CommitLanded | Phase::RowLanded);
        let old = part.since.as_deref().and_then(epoch_of).is_some_and(|since| now.saturating_sub(since) > conf.window_s);
        !(ends && old)
    });
}

/// `since` を導けない部品は前の出力の同じ (part, id, phase) から継ぐ（無ければ前の出力が在る周は `generated_at`・前の出力が無いか
/// 読めない周は null）。
fn inherit_since(parts: &mut [Part], prior: &Reading, generated_at: &str) {
    for part in parts.iter_mut().filter(|part| part.since.is_none()) {
        part.since = match prior {
            Reading::Read(old) => match old.parts.iter().find(|found| found.part == part.part && found.id == part.id && found.phase == part.phase) {
                Some(found) => found.since.clone(),
                None => Some(generated_at.to_owned()),
            },
            Reading::Absent | Reading::Unreadable => None,
        };
    }
}

/// 発話の行き先の memo の各々の部品の `links.source` に発話の ts を足す（発端の正本は仕分けの event・FR88）。
fn link_utterances(parts: &mut [Part]) {
    let sources: Vec<(String, String)> = parts
        .iter()
        .filter(|part| part.part == Kind::Utterance)
        .flat_map(|part| part.links.destination.iter().filter(|dest| dest.to == Sink::ToMemo).filter_map(|dest| dest.id.clone()).map(|memo| (memo, part.id.clone())))
        .collect();
    for (memo, utterance) in sources {
        if let Some(part) = parts.iter_mut().find(|part| part.part == Kind::Memo && part.id == memo) {
            if !part.links.source.contains(&utterance) {
                part.links.source.push(utterance);
            }
        }
    }
}

/// 古さの印の種類の列（頭の行の `stale=` の字・無ければ `-`・読めなければ `unreadable`）。
pub fn stale_word(state_dir: &Path) -> String {
    match read_stale(state_dir) {
        Stale::Absent => "-".to_owned(),
        Stale::Unreadable => "unreadable".to_owned(),
        Stale::Marks(marks) if marks.is_empty() => "-".to_owned(),
        Stale::Marks(marks) => marks.iter().map(|found| found.kind.as_str()).collect::<Vec<_>>().join(","),
    }
}

/// `fleet lifecycle show` の outcome（同じ renderer で今の組を出し、書き直さない・読み手の順は stale → json・無い周は rc 1
/// `lifecycle=absent`・読めない周は rc 1 `lifecycle=unreadable`）。
pub fn show(state_dir: &Path) -> Outcome {
    let stale = stale_word(state_dir);
    match read_output(state_dir) {
        Reading::Read(out) => Outcome::ok(text_lines(&out, &stale)),
        Reading::Absent => Outcome::failed_line(RC_REFUSED, "lifecycle=absent".to_owned()),
        Reading::Unreadable => Outcome::failed_line(RC_REFUSED, "lifecycle=unreadable".to_owned()),
    }
}

/// `fleet lifecycle write` の outcome: `Written`・`Unchanged`・`Coalesced`・`Discarded` は rc 0 で今の組を `show` と同じ字で出し、
/// `Busy` と `Unreadable` は rc 1 で `lifecycle=<語>` の 1 行だけを stderr へ出す。
pub fn written(state_dir: &Path, wrote: Wrote) -> Outcome {
    match wrote {
        Wrote::Busy | Wrote::Unreadable(_) => Outcome::failed_line(RC_REFUSED, format!("lifecycle={}", wrote.word())),
        Wrote::Written | Wrote::Unchanged | Wrote::Coalesced | Wrote::Discarded => show(state_dir),
    }
}

/// text の字（設計 §12 約束 9）: 頭の 1 行と、部品ごとの 1 行（値の無い欄は `-`）。`stale` は古さの印の種類の列（[`stale_word`]）。
pub fn text_lines(out: &Output, stale: &str) -> Vec<String> {
    let ledger = match &out.marks.ledger {
        Ledger::Noms { root, chunks, .. } => format!("{root}/{chunks}"),
        Ledger::Files { len, .. } => len.to_string(),
    };
    let head = format!(
        "lifecycle version=1 generated={} scope={} ledger={ledger} events={} main={} stale={stale}",
        out.generated_at,
        out.scope.as_str(),
        out.marks.events.len,
        out.marks.main
    );
    let dash = |value: &Option<String>| value.clone().unwrap_or_else(|| "-".to_owned());
    let parts = out.parts.iter().map(|part| {
        format!(
            "part={} id={} phase={} turn={} since={} reason={}",
            part.part.as_str(),
            part.id,
            part.phase.as_str(),
            part.turn.as_str(),
            dash(&part.since),
            dash(&part.reason)
        )
    });
    std::iter::once(head).chain(parts).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::lifecycle_mark::Events;
    use crate::fleet::store::{append_line, events_path, read_all};
    use crate::pipe::land::{contract_key, RUN_TRAILER};
    use crate::pipe::{git_bytes, git_ok};
    use crate::seat::ledger::issues_of;
    use std::path::PathBuf;

    const POLICY: LockPolicy = LockPolicy { retry_ms: 50, stale_ms: 600_000 };
    const OLD: &str = "2020-01-01T00:00:00Z";
    const FUTURE: &str = "2999-01-01T00:00:00Z";
    const POINTER: &str = "design = docs/design/x.md#a";

    /// 使い捨ての repo（main の ref・契約表・SRS・台帳の files の形を持つ）と置き場。
    struct Toy {
        repo: PathBuf,
        state: PathBuf,
        manifest: Manifest,
    }

    fn put(path: &Path, text: &str) {
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(path));
        assert!(std::fs::write(path, text).is_ok(), "{} を書けた", path.display());
    }

    const DECL: &str = "schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git diff --quiet\"]\n";
    const TABLE: &str = "<!-- contracts:begin -->\n[[contract]]\nid = \"a\"\ntitle = \"t\"\nreq = [\"FR1\"]\nsection = \"1\"\nwrite-set = [\"+src/a.rs\"]\nverify = [\"true\"]\nsize = \"S\"\ndone = \"d\"\n<!-- contracts:end -->\n";

    impl Toy {
        fn new(name: &str) -> Self {
            let repo = crate::pipe::fixture::scratch(&format!("lifecycle-writer-{name}"));
            let state = crate::pipe::fixture::scratch(&format!("lifecycle-writer-{name}-state"));
            for args in [&["init", "-q", "-b", "main"][..], &["config", "user.name", "t"], &["config", "user.email", "t@example.invalid"], &["config", "commit.gpgsign", "false"]] {
                assert!(git_ok(&repo, args), "{args:?}");
            }
            put(&repo.join(".vessel.toml"), &format!("{DECL}close-check = false\n"));
            put(&repo.join("docs/design/x.md"), TABLE);
            put(&repo.join("design-intent/spec/srs.html"), "<h2 id=\"FR1\">a</h2><h2 id=\"FR2\">b</h2>\n");
            let toy = Self { repo, state, manifest: Manifest::embedded().unwrap_or_else(|_| panic!("埋め込みの manifest を読める")) };
            let sha = toy.commit("README", "r", "first");
            toy.land(&sha);
            put(&toy.repo.join(".beads/config.yaml"), "issue-prefix: toy\n");
            toy.ledger("[]\n");
            toy
        }

        fn ledger(&self, text: &str) {
            put(&self.repo.join(".beads/issues.jsonl"), text);
        }

        fn commit(&self, file: &str, text: &str, message: &str) -> String {
            put(&self.repo.join(file), text);
            assert!(git_ok(&self.repo, &["add", "-A"]) && git_ok(&self.repo, &["commit", "-q", "-m", message]), "commit");
            self.head()
        }

        fn head(&self) -> String {
            git_bytes(&self.repo, &["rev-parse", "HEAD"]).map(|out| String::from_utf8_lossy(&out).trim().to_owned()).unwrap_or_default()
        }

        fn land(&self, sha: &str) {
            assert!(git_ok(&self.repo, &["update-ref", MAIN_REF_NAME, sha]), "main の ref");
        }

        fn place_with(&self, policy: LockPolicy) -> Place<'_> {
            Place { state_dir: &self.state, repo: &self.repo, manifest: &self.manifest, bd: "bd-unused", policy }
        }

        fn place(&self) -> Place<'_> {
            self.place_with(POLICY)
        }

        fn full_with(&self, issues: &[Issue], policy: LockPolicy) -> Wrote {
            full(&self.place_with(policy), Source::Borrowed(Round { issues, judged: Vec::new() }), Request::default())
        }

        fn full(&self, issues: &[Issue]) -> Wrote {
            self.full_with(issues, POLICY)
        }

        fn out(&self) -> Output {
            match read_output(&self.state) {
                Reading::Read(found) => *found,
                other => panic!("出力を読める: {other:?}"),
            }
        }

        fn log(&self, line: &str) {
            assert!(append_line(&events_path(&self.state), line, POLICY).is_ok(), "event を足せた");
        }

        /// 線の event（`close_check` が真なら detail つき）。
        fn line(&self, ts: &str, main: &str, close_check: bool) {
            let detail = if close_check { ",\"detail\":\"close-check\"" } else { "" };
            self.log(&format!("{{\"schema\":1,\"ts\":\"{ts}\",\"kind\":\"LifecycleCutover\",\"version\":\"0.1.0\",\"main\":\"{main}\",\"host\":\"h\",\"actor\":\"machine\"{detail}}}"));
        }

        fn lines(&self) -> EventLines {
            read_lines(&read_all(&self.state).unwrap_or_default())
        }

        fn stale(&self) -> Vec<MarkKind> {
            match read_stale(&self.state) {
                Stale::Marks(found) => found.iter().map(|one| one.kind).collect(),
                other => panic!("印を読める: {other:?}"),
            }
        }

        fn mark(&self, kind: MarkKind, at: &str, value: mark::Value) {
            assert_eq!(mark::add_mark(&self.state, &mark::Mark { kind, at: at.to_owned(), value }, POLICY), mark::Added::Added);
        }
    }

    const MAIN_REF_NAME: &str = "refs/remotes/origin/main";

    /// 台帳の bead 1 本の JSON（欄は label・設計 pointer の行・閉じた理由・閉じた時刻の順・label と時刻は空なら無し）。
    fn bead(id: &str, status: &str, [label, acceptance, reason, closed]: [&str; 4]) -> String {
        let labels = if label.is_empty() { String::new() } else { format!("\"{label}\"") };
        let closed = if closed.is_empty() { String::new() } else { format!(",\"closed_at\":\"{closed}\"") };
        format!("{{\"id\":\"{id}\",\"status\":\"{status}\",\"issue_type\":\"task\",\"labels\":[{labels}],\"acceptance_criteria\":\"{acceptance}\",\"description\":\"\",\"notes\":\"\",\"close_reason\":\"{reason}\",\"dependencies\":[]{closed}}}")
    }

    const MEMO: &str = "\"description\":\"### 出所\\n### 観測\\n### 候補\\n### 昇格条件\\n\"";

    fn memo(id: &str) -> String {
        bead(id, "open", ["intake:memo", "", "", ""]).replacen("\"description\":\"\"", MEMO, 1)
    }

    fn issues(items: &[String]) -> Vec<Issue> {
        issues_of(&format!("[{}]", items.join(","))).unwrap_or_else(|| panic!("fixture の JSON を読める"))
    }

    /// 今から `hours` 時間前の時刻の字。
    fn ago(hours: u64) -> String {
        format_utc(crate::seat::state::now_secs().saturating_sub(hours * 3_600))
    }

    fn find<'p>(out: &'p Output, kind: Kind, id: &str) -> Vec<&'p Part> {
        out.parts.iter().filter(|part| part.part == kind && part.id == id).collect()
    }

    fn marks_with(out: &Output, events: Events, ledger: Ledger, main: &str) -> Output {
        Output { marks: Marks { ledger, events, main: main.to_owned() }, ..out.clone() }
    }

    /// 書きかけは名に見えず、同じ中身は rename せず、古い印の書きは 3 つとも捨て、順を持たない組は古くないと読む。
    #[test]
    fn lifecycle_writer_publishes_atomically_skips_identical_writes_and_discards_older_ones() {
        let toy = Toy::new("publish");
        put(&fleet_dir(&toy.state).join(".lifecycle.json.1.tmp"), "{half");
        assert_eq!(read_output(&toy.state), Reading::Absent, "書きかけの一時 file は lifecycle.json の名に見えない");
        assert_eq!(toy.full(&[]), Wrote::Written);
        assert_eq!(toy.full(&[]), Wrote::Written, "1 周目が足した切り替えの線で event log の印が進む");
        let json = fleet_dir(&toy.state).join(JSON_FILE);
        let inode = || std::fs::metadata(&json).map(|meta| std::os::unix::fs::MetadataExt::ino(&meta)).unwrap_or(0);
        let first = inode();
        let base = toy.out();
        assert_eq!(toy.full(&[]), Wrote::Unchanged, "入力も部品も同じ周");
        assert_eq!(inode(), first, "Unchanged は rename しない");
        let later = Output { generated_at: "2999-01-01T00:00:00Z".to_owned(), ..base.clone() };
        assert_eq!(write(&toy.place(), &later, None), Wrote::Unchanged, "generated_at の他に同じ中身");
        let changed = Output { owned: Owned { count: 1, ..Owned::default() }, ..base.clone() };
        assert_eq!(write(&toy.place(), &changed, None), Wrote::Written);
        assert_ne!(inode(), first, "中身が違えば rename する");
        let tmp_left = std::fs::read_dir(fleet_dir(&toy.state)).map(|dir| dir.flatten().any(|entry| entry.file_name().to_string_lossy().ends_with(".tmp") && entry.file_name() != ".lifecycle.json.1.tmp")).unwrap_or(true);
        assert!(!tmp_left, "一時 file は rename で消える");
        let out = toy.out();
        let tree = json_tree::parse(&render_output(&out));
        assert_eq!(tree.ok().as_ref().and_then(output_of), Some(out), "render の字は parse で読める");
    }

    /// 古い印の書きは 3 つとも捨て、順を持たない組（head が違う・形が違う・祖先の関係が読めない）は古くないと読む。
    #[test]
    fn lifecycle_writer_discards_older_marks_and_reads_unordered_pairs_as_not_older() {
        let toy = Toy::new("discard");
        assert_eq!(toy.full(&[]), Wrote::Written);
        let base = toy.out();
        let (ledger, main) = (base.marks.ledger.clone(), base.marks.main.clone());
        let events = |len, head: Option<&str>| Events { len, head: head.map(str::to_owned) };
        let Ledger::Files { len, mtime_ns } = ledger.clone() else { panic!("files の形") };
        assert_eq!(write(&toy.place(), &marks_with(&base, events(100, Some("h")), ledger.clone(), &main), None), Wrote::Written, "head が違う組は順を持たない");
        assert_eq!(write(&toy.place(), &marks_with(&base, events(50, Some("h")), ledger.clone(), &main), None), Wrote::Discarded, "event log の印が古い");
        let older = Ledger::Files { len: len - 1, mtime_ns };
        assert_eq!(write(&toy.place(), &marks_with(&base, events(100, Some("h")), older, &main), None), Wrote::Discarded, "台帳の印が古い");
        let noms = Ledger::Noms { root: "r".to_owned(), generation: "g".to_owned(), chunks: 1 };
        assert_eq!(write(&toy.place(), &marks_with(&base, events(100, Some("h")), noms, &main), None), Wrote::Written, "形が違う台帳の組は順を持たない");
        let second = toy.commit("a.txt", "a", "second");
        assert_eq!(write(&toy.place(), &marks_with(&base, events(100, Some("h")), ledger.clone(), &second), None), Wrote::Written, "main が進んだ書き");
        assert_eq!(write(&toy.place(), &marks_with(&base, events(100, Some("h")), ledger.clone(), &main), None), Wrote::Discarded, "main の印が古い");
        let stray = "f".repeat(40);
        assert_eq!(write(&toy.place(), &marks_with(&base, events(100, Some("h")), ledger, &stray), None), Wrote::Written, "祖先の関係が読めない main は順を持たない");
    }

    /// 生きた pid の lock は rules 行の stale より古くても外さず Busy、死んだ pid の lock は外して Written。
    #[test]
    fn lifecycle_writer_lock_is_busy_for_a_live_owner_and_reclaimed_from_a_dead_one() {
        let toy = Toy::new("lock");
        let policy = LockPolicy { retry_ms: 30, stale_ms: 1 };
        let lock = fleet_dir(&toy.state).join(JSON_LOCK);
        put(&lock, &format!("{}\n", std::process::id()));
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(toy.full_with(&[], policy), Wrote::Busy, "生きた所有者は古くても外さない");
        assert_eq!(write(&toy.place_with(policy), &toy_output(&toy), None), Wrote::Busy);
        assert_eq!(read_output(&toy.state), Reading::Absent, "書かない");
        put(&lock, "4194300 1\n");
        assert_eq!(toy.full_with(&[], policy), Wrote::Written, "死んだ所有者の lock は外して書く");
        assert!(!lock.exists(), "取った lock は外される");
    }

    /// 出力の型の値（空の部品）。
    fn toy_output(toy: &Toy) -> Output {
        let marks = mark::read_marks(&toy.state, &toy.repo).unwrap_or_else(|word| panic!("印を読める: {word}"));
        Output {
            generated_at: OLD.to_owned(),
            scope: Scope::Full,
            full_at: OLD.to_owned(),
            interval_s: None,
            closed_window_h: None,
            marks,
            unmeasured: Vec::new(),
            owned: Owned::default(),
            parts: Vec::new(),
        }
    }

    /// 窓は終わりの局面の閉じた部品だけに掛かり（misfit の閉じは残る）、a2 の misfit が a1 の閉じた部品を置き換え、since の継ぎは 3 形で、
    /// 規則の値が出力に写る。
    #[test]
    fn lifecycle_writer_window_misfit_since_and_config_copies() {
        let toy = Toy::new("parts");
        let main = toy.head();
        toy.line(OLD, &main, false);
        toy.line("2020-01-02T00:00:00Z", &main, true);
        let landed = format!("landed {main} ci=success");
        let base = vec![
            bead("toy-q1", "closed", ["intake:question", "", "裁定 batch:x", &ago(1)]),
            bead("toy-q2", "closed", ["intake:question", "", &landed, &ago(240)]),
            bead("toy-c9", "closed", ["", POINTER, &landed, &ago(240)]),
            memo("toy-m1"),
        ];
        assert_eq!(toy.full(&issues(&base)), Wrote::Written);
        let out = toy.out();
        let one = |id: &str, kind: Kind| match find(&out, kind, id).as_slice() {
            [only] => (*only).clone(),
            found => panic!("{id} の部品が 1 つ: {found:?}"),
        };
        let replaced = one("toy-q1", Kind::Question);
        assert_eq!((replaced.phase, replaced.reason.as_deref()), (Phase::Misfit, Some("close-ruling-not-bound")), "a2 の語で置き換わる");
        let old_misfit = one("toy-q2", Kind::Question);
        assert_eq!(old_misfit.phase, Phase::Misfit, "窓より古く閉じた misfit は残る");
        assert!(find(&out, Kind::Contract, "toy-c9").is_empty(), "窓より古く閉じた終わりの局面の部品は載らない（対）");
        assert_eq!((out.owned.count, out.owned.unknown, out.owned.unset), (0, 2, 0), "since を導けない 2 つの misfit は unknown に数える");
        assert_eq!((out.scope, out.full_at.as_str(), out.closed_window_h), (Scope::Full, out.generated_at.as_str(), Some(72)), "full_at と窓の写し");
        assert_eq!(out.interval_s, int_row(&toy.manifest, "seat.tick_interval_s").ok(), "interval_s の写し");
    }

    /// since の継ぎは 3 形: 前の出力が無ければ null・前の出力が在って同じ組が無ければ `generated_at`・前の出力の同じ (part, id, phase) から継ぐ。
    #[test]
    fn lifecycle_writer_since_is_inherited_in_three_forms() {
        let toy = Toy::new("since");
        let base = vec![memo("toy-m1")];
        assert_eq!(toy.full(&issues(&base)), Wrote::Written);
        assert_eq!(find(&toy.out(), Kind::Memo, "toy-m1").first().map(|part| part.since.clone()), Some(None), "前の出力が無い周は null");
        toy.ledger("[]\n\n");
        let with_two = [base.clone(), vec![memo("toy-m2")]].concat();
        assert_eq!(toy.full(&issues(&with_two)), Wrote::Written);
        let second = toy.out();
        let since = |out: &Output, id: &str| find(out, Kind::Memo, id).first().and_then(|part| part.since.clone());
        assert_eq!(since(&second, "toy-m2"), Some(second.generated_at.clone()), "前の出力が在って同じ組が無ければ generated_at");
        assert_eq!(since(&second, "toy-m1"), None, "前の出力の同じ組の値（null）を継ぐ");
        let mut tampered = second.clone();
        for part in tampered.parts.iter_mut().filter(|part| part.id == "toy-m2") {
            part.since = Some(OLD.to_owned());
        }
        assert!(publish(&fleet_dir(&toy.state), JSON_FILE, &render_output(&tampered)).is_ok());
        toy.ledger("[]\n\n\n");
        assert_eq!(toy.full(&issues(&with_two)), Wrote::Written);
        assert_eq!(since(&toy.out(), "toy-m2"), Some(OLD.to_owned()), "前の出力の同じ (part, id, phase) から継ぐ");
    }

    /// 行 b1 の結びは契約の `links.commits` に、request の仕分けの発話の ts は memo の `links.source` に載る。
    #[test]
    fn lifecycle_writer_ties_commits_and_sources_onto_contracts_and_memos() {
        let toy = Toy::new("ties");
        let first = toy.head();
        toy.line(OLD, &first, false);
        toy.log("{\"schema\":1,\"ts\":\"2026-09-29T00:00:00Z\",\"kind\":\"RunCreated\",\"run\":\"r1\",\"bead\":\"toy-c1\",\"host\":\"h\",\"actor\":\"machine\"}");
        let trailer = format!("landed\n\n{RUN_TRAILER}r1\n{}docs/design/x.md#a\n", contract_key());
        let second = toy.commit("b.txt", "b", &trailer);
        toy.land(&second);
        let (utterance, at) = ("2026-09-30T00:00:00Z", "2026-09-30T00:00:01Z");
        toy.log(&format!("{{\"schema\":1,\"ts\":\"{utterance}\",\"kind\":\"UtteranceReceived\",\"channel\":\"chat\",\"session\":\"s\",\"host\":\"h\",\"actor\":\"human\",\"detail\":\"x\"}}"));
        toy.log(&format!("{{\"schema\":1,\"ts\":\"{at}\",\"kind\":\"UtteranceSorted\",\"utterance\":\"{utterance}\",\"sorting\":\"request\",\"bead\":\"toy-m1\",\"host\":\"h\",\"actor\":\"machine\"}}"));
        let items = [bead("toy-c1", "open", ["", POINTER, "", ""]), memo("toy-m1")];
        assert_eq!(toy.full(&issues(&items)), Wrote::Written);
        let out = toy.out();
        let contract = find(&out, Kind::Contract, "toy-c1");
        assert_eq!(contract.first().map(|part| part.links.commits.clone()), Some(vec![second.clone()]), "結びが契約の links.commits に載る");
        let run = find(&out, Kind::Run, "r1");
        assert!(!run.is_empty(), "便の部品が在る");
        assert_eq!(run.first().map(|part| part.links.commits.clone()), Some(vec![second.clone()]), "結びが便の links.commits に載る");
        let memos = find(&out, Kind::Memo, "toy-m1");
        assert_eq!(memos.first().map(|part| part.links.source.clone()), Some(vec![utterance.to_owned()]), "発話の ts が memo の links.source に載る");
    }

    /// 印の消えの表（3 種 × {開始が印より前・印の後で同じ入力・印の後で新しい入力}）と、merge-gate は event log だけの進みでは消えない。
    #[test]
    fn lifecycle_writer_stale_marks_clear_only_for_a_later_start_and_the_kind_condition() {
        let toy = Toy::new("stale");
        let first = toy.head();
        let second = toy.commit("a.txt", "a", "second");
        toy.land(&second);
        assert_eq!(toy.full(&[]), Wrote::Written);
        assert_eq!(read_stale(&toy.state), Stale::Marks(Vec::new()), "最初の Written で空の marks を作る");
        toy.log("{\"schema\":1,\"ts\":\"2026-09-30T00:00:00Z\",\"kind\":\"UtteranceReceived\",\"channel\":\"gui\",\"host\":\"h\",\"actor\":\"human\",\"detail\":\"x\"}");
        let now = mark::read_ledger(&toy.repo).unwrap_or_else(|| panic!("台帳を読める"));
        let cases = [
            (MarkKind::LedgerGate, [mark::Value::Ledger(now), mark::Value::Ledger(Ledger::Files { len: 0, mtime_ns: 0 })]),
            (MarkKind::MergeGate, [mark::Value::Main(second), mark::Value::Main(first)]),
            (MarkKind::Unreadable, [mark::Value::Reason("main".to_owned()), mark::Value::Reason("main".to_owned())]),
        ];
        for (kind, [same, newer]) in cases {
            let expected = [(FUTURE, &newer, false), (OLD, &same, kind == MarkKind::Unreadable), (OLD, &newer, true)];
            for (at, value, cleared) in expected {
                let _ = mark::settle(&toy.state, |_| true, POLICY);
                toy.mark(kind, at, value.clone());
                assert!(matches!(toy.full(&[]), Wrote::Written | Wrote::Unchanged));
                assert_eq!(!toy.stale().contains(&kind), cleared, "{kind:?} at={at} value={value:?}");
            }
        }
    }

    /// Discarded・Busy・rename の落ちた周は条件に当たる印も消さず、同じ歯の中の Written の周は消す。
    #[test]
    fn lifecycle_writer_discarded_busy_and_failed_rename_keep_marks_until_a_written_round() {
        let toy = Toy::new("keep");
        assert_eq!(toy.full(&[]), Wrote::Written);
        toy.mark(MarkKind::Unreadable, OLD, mark::Value::Reason("main".to_owned()));
        toy.mark(MarkKind::LedgerGate, OLD, mark::Value::Ledger(Ledger::Files { len: 0, mtime_ns: 0 }));
        let both = vec![MarkKind::LedgerGate, MarkKind::Unreadable];
        let mut newer = toy.out();
        newer.marks.ledger = Ledger::Files { len: 1_000_000, mtime_ns: u64::MAX };
        assert_eq!(write(&toy.place(), &newer, None), Wrote::Written);
        assert_eq!(toy.full(&[]), Wrote::Discarded, "今の file の印が新しい");
        assert_eq!(toy.stale(), both, "Discarded の周は印を消さない");
        let lock = fleet_dir(&toy.state).join(JSON_LOCK);
        put(&lock, &format!("{}\n", std::process::id()));
        assert_eq!(toy.full_with(&[], LockPolicy { retry_ms: 30, stale_ms: 600_000 }), Wrote::Busy);
        assert_eq!(toy.stale(), both, "Busy の周は印を消さない");
        assert!(std::fs::remove_file(&lock).is_ok());
        let json = fleet_dir(&toy.state).join(JSON_FILE);
        assert!(std::fs::remove_file(&json).is_ok() && std::fs::create_dir_all(&json).is_ok(), "lifecycle.json の名に dir を置く");
        assert_eq!(toy.full(&[]), Wrote::Unreadable("write"), "rename が落ちる");
        assert_eq!(toy.stale(), both, "rename の落ちた周は印を消さない");
        assert!(std::fs::remove_dir(&json).is_ok());
        assert_eq!(toy.full(&[]), Wrote::Written);
        assert_eq!(toy.stale(), Vec::<MarkKind>::new(), "Written の周は消す");
    }

    /// 読めない 6 語の各 1 fixture: 書かず、理由の語を持つ印を置く。2 つの入力が同時に読めない周は読む順で先の語。SRS の読みが落ちた周は
    /// `srs` で書かず、SRS の無い repo は unmeasured で書く（対）。
    #[test]
    fn lifecycle_writer_unreadable_words_write_nothing_and_leave_a_reason_mark() {
        type Break = fn(&Toy);
        let no_ledger: Break = |toy| drop(std::fs::remove_file(toy.repo.join(".beads/issues.jsonl")));
        let events_dir: Break = |toy| {
            let log = events_path(&toy.state);
            assert!(std::fs::remove_file(&log).is_ok() && std::fs::create_dir_all(&log).is_ok(), "log を dir に置き換える");
        };
        let no_main: Break = |toy| drop(std::fs::remove_file(toy.repo.join(".git").join(MAIN_REF_NAME)));
        let bad_declaration: Break = |toy| {
            let sha = toy.commit(".vessel.toml", &format!("{DECL}close-check = \"true\"\n"), "broken declaration");
            toy.land(&sha);
        };
        let cases: [(&str, &[Break], &str); 8] = [
            ("ledger", &[no_ledger], "ledger"),
            ("events", &[events_dir], "events"),
            ("main", &[no_main], "main"),
            ("table", &[|toy| put(&toy.repo.join(".git").join(MAIN_REF_NAME), &format!("{}\n", "a".repeat(40)))], "table"),
            ("srs", &[|toy| {
                let _ = std::fs::remove_file(toy.repo.join("design-intent/spec/srs.html"));
                assert!(git_ok(&toy.repo, &["rm", "-q", "--cached", "design-intent/spec/srs.html"]), "index から外す");
                let entry = format!("160000,{},design-intent/spec/srs.html", "b".repeat(40));
                assert!(git_ok(&toy.repo, &["update-index", "--add", "--cacheinfo", &entry]), "gitlink にする");
                assert!(git_ok(&toy.repo, &["commit", "-q", "-m", "submodule"]), "commit");
                toy.land(&toy.head());
            }], "srs"),
            ("declaration", &[bad_declaration], "declaration"),
            ("two-ledger-main", &[no_ledger, no_main], "ledger"),
            ("two-events-declaration", &[events_dir, bad_declaration], "events"),
        ];
        for (name, breaks, word) in cases {
            let toy = Toy::new(name);
            assert_eq!(toy.full(&[]), Wrote::Written, "{name}: 読める周");
            let before = std::fs::read(fleet_dir(&toy.state).join(JSON_FILE)).unwrap_or_default();
            for found in breaks {
                found(&toy);
            }
            assert_eq!(toy.full(&[]), Wrote::Unreadable(word), "{name}");
            assert_eq!(std::fs::read(fleet_dir(&toy.state).join(JSON_FILE)).unwrap_or_default(), before, "{name}: 書かない");
            let Stale::Marks(marks) = read_stale(&toy.state) else { panic!("{name}: 印を読める") };
            assert_eq!(marks.iter().map(|found| (found.kind, found.value.clone())).collect::<Vec<_>>(), [(MarkKind::Unreadable, mark::Value::Reason(word.to_owned()))], "{name}");
        }
        let toy = Toy::new("no-srs");
        assert!(git_ok(&toy.repo, &["rm", "-q", "design-intent/spec/srs.html", "docs/design/x.md"]) && git_ok(&toy.repo, &["commit", "-q", "-m", "drop both"]));
        toy.land(&toy.head());
        assert_eq!(toy.full(&[]), Wrote::Written, "SRS と契約表の無い repo は書く");
        let reasons: Vec<String> = toy.out().unmeasured.into_iter().map(|(_, reason)| reason).collect();
        assert!(reasons.iter().any(|reason| reason.contains("srs")) && reasons.iter().any(|reason| reason.contains("table")), "{reasons:?}");
    }

    /// 線は 1 度だけ記帳し、宣言は読んだ main の sha の tree から読む（HEAD が true でも main が false なら足さない）・読めない宣言と
    /// Discarded と Busy の周は足さない。
    #[test]
    fn lifecycle_writer_books_lines_once_and_reads_the_declaration_at_the_main_sha() {
        let toy = Toy::new("lines");
        assert_eq!(toy.full(&[]), Wrote::Written);
        let main = toy.head();
        assert_eq!(toy.lines().cutover.map(|found| found.main), Some(main.clone()), "切り替えの線の main は読んだ sha");
        assert_eq!(toy.lines().close_check, None, "宣言が false の repo は close-check の線を足さない");
        let head_true = toy.commit(".vessel.toml", &format!("{DECL}close-check = true\n"), "head declares true");
        assert_eq!(toy.full(&[]), Wrote::Written);
        assert_eq!(toy.lines().close_check, None, "HEAD の宣言は読まない（main の sha の宣言は false）");
        assert_eq!(read_all(&toy.state).map(|found| found.len()).unwrap_or(0), 1, "線は 1 本のまま");
        toy.land(&head_true);
        assert!(matches!(toy.full(&[]), Wrote::Written | Wrote::Unchanged));
        assert_eq!(toy.lines().close_check.map(|found| found.main), Some(head_true), "宣言が true の main で close-check の線を足す");
        assert!(matches!(toy.full(&[]), Wrote::Written | Wrote::Unchanged));
        assert_eq!(read_all(&toy.state).map(|found| found.len()).unwrap_or(0), 2, "在る線は動かさない");
    }

    /// 読めない宣言と Discarded と Busy の周は線を足さない。
    #[test]
    fn lifecycle_writer_books_no_line_for_an_unreadable_declaration_or_a_discarded_or_busy_round() {
        let broken = Toy::new("lines-broken");
        assert_eq!(broken.full(&[]), Wrote::Written);
        let sha = broken.commit(".vessel.toml", &format!("{DECL}close-check = \"true\"\n"), "broken");
        broken.land(&sha);
        assert_eq!(broken.full(&[]), Wrote::Unreadable("declaration"));
        assert_eq!(read_all(&broken.state).map(|found| found.len()).unwrap_or(0), 1, "読めない宣言で記帳しない");
        let quiet = Toy::new("lines-quiet");
        let mut newer = toy_output(&quiet);
        newer.marks.ledger = Ledger::Files { len: 1_000_000, mtime_ns: u64::MAX };
        assert_eq!(write(&quiet.place(), &newer, None), Wrote::Written);
        assert_eq!(quiet.full(&[]), Wrote::Discarded);
        put(&fleet_dir(&quiet.state).join(JSON_LOCK), &format!("{}\n", std::process::id()));
        assert_eq!(quiet.full_with(&[], LockPolicy { retry_ms: 30, stale_ms: 600_000 }), Wrote::Busy);
        assert_eq!(read_all(&quiet.state).map(|found| found.len()).unwrap_or(9), 0, "Discarded と Busy の周は線を足さない");
    }

    fn part(kind: Kind, id: &str, phase: Phase, since: Option<&str>) -> Part {
        let turn = turn_of(phase.as_str(), None).unwrap_or(Turn::Nobody);
        Part { part: kind, id: id.to_owned(), phase, turn, since: since.map(str::to_owned), reason: None, closed: false, overdue: None, links: Links::default(), extra: Extra::None }
    }

    /// owned の 4 欄: count・unset（行の無い seat の語）・unknown（行が在って since が無い）・oldest。requirement は数えず、overdue は手番が seat でない部品と
    /// 行の無い部品で null。
    #[test]
    fn lifecycle_writer_owned_counts_four_fields_and_nulls_overdue_outside_the_seat_rows() {
        let now = crate::seat::state::now_secs();
        let (recent, old, older) = (ago(1), ago(30), ago(60));
        let mut parts = vec![
            part(Kind::Run, "r-asking", Phase::RunAsking, Some(&old)),
            part(Kind::Requirement, "FR1", Phase::RequirementUnrowed, Some(&old)),
            part(Kind::Question, "q-old", Phase::Misfit, Some(&old)),
            part(Kind::Question, "q-older", Phase::Misfit, Some(&older)),
            part(Kind::Question, "q-recent", Phase::Misfit, Some(&recent)),
            part(Kind::Question, "q-unknown", Phase::Misfit, None),
            part(Kind::Epic, "e-open", Phase::EpicOpen, Some(&old)),
        ];
        let hours = |word: &str| (word == Phase::Misfit.as_str()).then_some(24);
        let owned = count_owned(&mut parts, &hours, now);
        assert_eq!((owned.count, owned.unset, owned.unknown), (2, 1, 1), "requirement-unrowed は unset にも数えない・run-asking は行が無いので unset");
        assert_eq!(owned.oldest.as_ref().map(|found| (found.id.as_str(), found.since.clone())), Some(("q-older", older)));
        let overdue: Vec<Option<bool>> = parts.iter().map(|found| found.overdue).collect();
        assert_eq!(overdue, [None, None, Some(true), Some(true), Some(false), None, None], "手番が seat でない部品・行の無い部品・since の無い部品は null");
    }
}
