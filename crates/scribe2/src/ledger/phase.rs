//! 台帳の側の部品と閉じの misfit（設計 docs/design/case-lifecycle.md §7・FR90・FR91・FR93・ADR-0089）。
//!
//! 純関数 1 本（[`derive`]）が、台帳の読み（[`Issue`] の列）・台帳の接頭辞・周の時刻・窓の秒・2 つの線の時刻・未反映の裁定 id・
//! 処置の無い判定を持つ memo id・開いた契約の write-set の項目（[`Input`]）を受け、question・memo・epic の部品と、閉じた
//! contract の部品と、形と閉じの misfit（[`Output`]）を返す。I/O も時計も持たない（書き手が集めて渡す）。開いた契約の局面は
//! 便と列の側の部品が持つので返さない（設計 §9）。ただし設計 pointer を持たない開いた契約は形の misfit `form-neither` で返す。
//!
//! 種類の判定は §2 の順（問いの label か decision の型 → question ／ memo の label → memo ／ epic の型 → epic ／ ほかは
//! contract）。memo は FR91 の順（form-both → promoting → asking → no-trigger → actionable → waiting）に判じ、waiting は前の
//! 段に当たらない開いた memo の全部を受けるので、行 a1 の部品は `no-phase` に落ちない。辿れる契約は contract の種類の
//! `discovered-from` だけ、子の問いは `parent-child` だけを数える。FR93 の条件の 1 関数は [`close_due`] で、memo の自動の
//! close の行が同じ関数を使う。
//!
//! 閉じの misfit 5 語は、2 つの線がどちらも在り、閉じた時刻が両方より後の閉じにだけ判じる。ほかの閉じは `*-closed` に置いて
//! 窓を掛け、misfit の閉じには窓を掛けない。`since` は導ける 8 つの部品（閉じた問い・閉じた epic・question-open・promoting の
//! 2 理由・epic-closable・期日の満ち・依存と着地の満ち）の値だけで、ほかは `None`（継ぎは書き手が行う・§5.2）。

use crate::case::{turn_of, Extra, Kind, Links, Misfit, Part, Phase, Turn, TriggerView, REASON_CLOSE_DUE, REASON_CONTRACT_OPEN};
use crate::fleet::cli::format_utc;
use crate::fleet::epoch_of;
use crate::ledger::close_reason::{self, Defect, Form, Head, LandedTail};
use crate::ledger::form::{is_memo, is_question, pointer_text};
use crate::ledger::promotion::{self, Scope};
use crate::ledger::trigger::{self, Trigger};
use crate::pipe::table::{parse_pointer, Pointer};
use crate::seat::ledger::Issue;

/// 閉じた bead の status。
const CLOSED: &str = "closed";

/// epic の型。
const EPIC: &str = "epic";

/// 問いに数える型（label を持たない decision）。
const DECISION: &str = "decision";

/// memo から契約へ張る edge の種別。
const DISCOVERED_FROM: &str = "discovered-from";

/// 子から親へ張る edge の種別。
const PARENT_CHILD: &str = "parent-child";

/// memo-actionable の理由: 満ちて keep の無い引き金。
pub const REASON_TRIGGER_MET: &str = "trigger-met";

/// memo-actionable の理由: 処置の無い判定（FR87）。
pub const REASON_VERDICT: &str = "verdict";

/// memo-actionable の理由: 昇格の行を持ち、辿れる開いた契約が無く、FR93 を満たさない。
pub const REASON_PROMOTION_UNMET: &str = "promotion-unmet";

/// memo-actionable の理由: 最後の昇格の行が読めない。
pub const REASON_PROMOTION_UNREADABLE: &str = "promotion-unreadable";

/// memo-waiting の理由: 満ちた引き金に keep が付いている。
pub const REASON_KEEP: &str = "keep";

/// `unmeasured` の理由: 台帳の接頭辞が解けない。
pub const UNMEASURED_LEDGER_PREFIX: &str = "ledger-prefix";

/// 2 つの線の時刻（UNIX 秒）。無い線は `None`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lines {
    /// 切り替えの線。
    pub cutover: Option<u64>,
    /// close-check の線（close-check を true で持たない repo は `None`）。
    pub close_check: Option<u64>,
}

/// [`derive`] の入力（全部を書き手が集めて渡す）。
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// 台帳の読み（閉じた bead を含む全部）。
    pub issues: &'a [Issue],
    /// 台帳の接頭辞（解けない周は `None`）。
    pub prefix: Option<&'a str>,
    /// 周の時刻（UNIX 秒）。
    pub now: u64,
    /// 閉じの窓の秒（rules 行 `lifecycle.closed_window_h` の秒）。
    pub window_s: u64,
    /// 2 つの線。
    pub lines: Lines,
    /// 閉じて未反映の裁定を持つ問いの bead id（FR84・後の行が渡すまで空）。
    pub unreflected: &'a [String],
    /// 処置の無い判定を持つ memo の bead id（FR87・後の行が渡すまで空）。
    pub unjudged: &'a [String],
    /// 開いた契約の write-set の項目（`+` `-` `=` の印つきの字のまま）。
    pub write_set: &'a [String],
}

/// [`derive`] の出力。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// 部品（台帳の順）。`overdue` は書き手が決めるので `None`。
    pub parts: Vec<Part>,
    /// 測れなかった種類と理由の語。
    pub unmeasured: Vec<(Kind, &'static str)>,
}

/// 台帳の側の部品を導く（**純関数**・§7）。
pub fn derive(input: &Input<'_>) -> Output {
    let ctx = Ctx::new(input);
    let parts = input.issues.iter().filter_map(|issue| ctx.part(issue)).collect();
    let unmeasured = match input.prefix {
        Some(_) => Vec::new(),
        None => [Kind::Question, Kind::Memo, Kind::Contract, Kind::Epic].map(|kind| (kind, UNMEASURED_LEDGER_PREFIX)).to_vec(),
    };
    Output { parts, unmeasured }
}

/// FR93 の条件（memo の close を待つ判定・**1 つの純関数**）。5 つが全部そろうとき真:
/// (1) 最後の昇格の行が `全部` で読める (2) 行の列と辿れる契約の集合が等しく 1 本以上 (3) 辿れる契約の全部が着地の形で閉じた
/// (4) 子の開いた問いが無い (5) 処置の無い判定が無い。`prefix` が解けない周は偽。
pub fn close_due(memo: &Issue, issues: &[Issue], prefix: Option<&str>, unjudged: &[String]) -> bool {
    !unjudged.contains(&memo.id) && settled(memo, issues, prefix, None)
}

/// FR93 の条件のうち判定を除く 4 つ（(1)〜(4)）。`at` が在れば、その時刻の状態で測る（閉じた時点・[`Misfit::PromotedUnmet`]）。
fn settled(memo: &Issue, issues: &[Issue], prefix: Option<&str>, at: Option<u64>) -> bool {
    let Some(prefix) = prefix else { return false };
    let Some(promotion::Line::Readable(line)) = promotion::read(&memo.notes, prefix) else { return false };
    let traced: Vec<&Issue> = traced_of(memo, issues).collect();
    let traced_ids: Vec<String> = traced.iter().map(|contract| contract.id.clone()).collect();
    line.scope == Scope::All
        && !traced.is_empty()
        && same_set(&line.ids, &traced_ids)
        && traced.iter().all(|contract| landed_by(contract, prefix, at))
        && !issues.iter().any(|question| is_child_question(question, memo) && open_at(question, at))
}

/// memo から `discovered-from` で辿れる契約（contract の種類だけ）。
fn traced_of<'i>(memo: &'i Issue, issues: &'i [Issue]) -> impl Iterator<Item = &'i Issue> {
    issues.iter().filter(move |issue| kind_of(issue) == Kind::Contract && points_to(issue, DISCOVERED_FROM, &memo.id))
}

/// memo の子の問い（`parent-child` だけ）。
fn is_child_question(issue: &Issue, memo: &Issue) -> bool {
    kind_of(issue) == Kind::Question && points_to(issue, PARENT_CHILD, &memo.id)
}

/// 契約が着地の形（読める尾）で閉じ、`at` より後に閉じていない。
fn landed_by(contract: &Issue, prefix: &str, at: Option<u64>) -> bool {
    let landed = matches!(
        close_reason::read(&contract.close_reason, Some(prefix)),
        Ok(Form::Landed { tail, .. }) if !matches!(tail, LandedTail::Unreadable(_))
    );
    is_closed(contract) && landed && at.is_none_or(|limit| closed_at(contract).is_none_or(|found| found <= limit))
}

/// 問いが `at` の時点で開いている（`at` が無ければ今の status）。
fn open_at(question: &Issue, at: Option<u64>) -> bool {
    !is_closed(question) || at.is_some_and(|limit| closed_at(question).is_some_and(|found| found > limit))
}

/// 2 つの id の列が集合として等しいか。
fn same_set(left: &[String], right: &[String]) -> bool {
    let sorted = |ids: &[String]| {
        let mut ids = ids.to_vec();
        ids.sort();
        ids.dedup();
        ids
    };
    sorted(left) == sorted(right)
}

/// bead の種類（§2 の順）。
fn kind_of(issue: &Issue) -> Kind {
    if is_question(issue) || issue.kind == DECISION {
        Kind::Question
    } else if is_memo(issue) {
        Kind::Memo
    } else if issue.kind == EPIC {
        Kind::Epic
    } else {
        Kind::Contract
    }
}

/// bead が閉じたか。
fn is_closed(issue: &Issue) -> bool {
    issue.status == CLOSED
}

/// `issue` が種別 `kind` の edge で `target` に結ばれているか。
fn points_to(issue: &Issue, kind: &str, target: &str) -> bool {
    issue.deps.iter().any(|dep| dep.kind == kind && dep.on == target)
}

/// 台帳の時刻の字（秒の小数を持つ形も許す）を UNIX 秒へ。
fn epoch_at(text: &str) -> Option<u64> {
    epoch_of(text).or_else(|| {
        let (head, tail) = text.split_once('.')?;
        let digits = tail.strip_suffix('Z')?;
        (!digits.is_empty() && digits.bytes().all(|found| found.is_ascii_digit())).then(|| epoch_of(&format!("{head}Z")))?
    })
}

/// 閉じた時刻（無いか読めなければ `None`）。
fn closed_at(issue: &Issue) -> Option<u64> {
    issue.closed_at.as_deref().and_then(epoch_at)
}

/// 起票の時刻（無いか読めなければ `None`）。
fn created_at(issue: &Issue) -> Option<u64> {
    issue.created_at.as_deref().and_then(epoch_at)
}

/// bead の id の列。
fn ids_of(issues: &[&Issue]) -> Vec<String> {
    issues.iter().map(|issue| issue.id.clone()).collect()
}

/// 閉じの種類ごとに許す理由の頭（§7 (6)）。
fn heads_of(kind: Kind) -> &'static [Head] {
    match kind {
        Kind::Contract => &[Head::Landed, Head::Duplicate, Head::Successor, Head::Withdrawn],
        Kind::Question => &[Head::Ruling],
        Kind::Memo => &[Head::Promoted, Head::Merged, Head::Deferred],
        _ => &[Head::Done, Head::Withdrawn],
    }
}

/// 読めない理由の欠陥から閉じの misfit（裁定と見送りの値の崩れは行 a2 が判じる）。
fn defect_misfit(defect: Defect, kind: Kind) -> Option<Misfit> {
    match defect {
        Defect::Empty | Defect::UnknownHead => Some(Misfit::CloseKindMismatch),
        Defect::Value(head) if head == Head::Landed || !heads_of(kind).contains(&head) => Some(Misfit::CloseKindMismatch),
        Defect::Value(Head::Duplicate | Head::Successor | Head::Merged | Head::Promoted) => Some(Misfit::CloseUnresolved),
        Defect::Value(_) | Defect::NoPrefix => None,
    }
}

/// 値を字で組んだ引き金の値の字。
fn value_text(trigger: &Trigger) -> String {
    match trigger {
        Trigger::Recurrence(count) => count.to_string(),
        Trigger::Bundle(path) => path.clone(),
        Trigger::Dependency(id) => id.clone(),
        Trigger::Deadline(at) => format_utc(*at),
        Trigger::Landing(pointer) => format!("{}#{}", pointer.path, pointer.id),
    }
}

/// 部品の下書き（[`Draft::seal`] が手番を引いて [`Part`] にする）。
struct Draft {
    kind: Kind,
    id: String,
    phase: Phase,
    reason: Option<&'static str>,
    since: Option<u64>,
    closed: bool,
    links: Links,
    extra: Extra,
}

impl Draft {
    /// 欄が空の下書き。
    fn new(issue: &Issue, kind: Kind, phase: Phase) -> Self {
        Self { kind, id: issue.id.clone(), phase, reason: None, since: None, closed: is_closed(issue), links: Links::default(), extra: Extra::None }
    }

    /// misfit に置き換える（局面は `misfit`・理由は misfit の語・since は持たない）。
    fn misfit(mut self, word: Misfit) -> Self {
        (self.phase, self.reason, self.since) = (Phase::Misfit, Some(word.as_str()), None);
        self
    }

    /// 手番を引いて部品にする。表に無い語と理由は misfit `no-phase` に倒す（fail-closed）。
    fn seal(self) -> Part {
        let (phase, reason, since, turn) = match turn_of(self.phase.as_str(), self.reason) {
            Some(turn) => (self.phase, self.reason, self.since, turn),
            None => (Phase::Misfit, Some(Misfit::NoPhase.as_str()), None, Turn::Seat),
        };
        Part {
            part: self.kind,
            id: self.id,
            phase,
            turn,
            since: since.map(format_utc),
            reason: reason.map(str::to_owned),
            closed: self.closed,
            overdue: None,
            links: self.links,
            extra: self.extra,
        }
    }
}

/// memo 1 つの材料（辿れる契約・子の問い・引き金の読み）。
struct Facts<'i> {
    traced: Vec<&'i Issue>,
    children: Vec<&'i Issue>,
    reading: trigger::Reading,
}

/// 周の材料（入力と、閉じた bead の id と設計 pointer）。
struct Ctx<'a> {
    input: &'a Input<'a>,
    closed: Vec<String>,
    pointers: Vec<Pointer>,
}

impl<'a> Ctx<'a> {
    fn new(input: &'a Input<'a>) -> Self {
        let closed = || input.issues.iter().filter(|issue| is_closed(issue));
        let pointers = closed().filter_map(|issue| pointer_text(&issue.acceptance)).filter_map(|text| parse_pointer(text).ok()).collect();
        Self { input, closed: closed().map(|issue| issue.id.clone()).collect(), pointers }
    }

    /// 台帳の bead 1 本。
    fn find(&self, id: &str) -> Option<&Issue> {
        self.input.issues.iter().find(|issue| issue.id == id)
    }

    /// 部品 1 つ（載せない bead は `None`）。
    fn part(&self, issue: &Issue) -> Option<Part> {
        let kind = kind_of(issue);
        let draft = if is_closed(issue) { self.closed_draft(issue, kind)? } else { self.open_draft(issue, kind)? };
        Some(draft.seal())
    }

    /// 開いた bead の下書き（開いた契約は形の misfit だけ）。
    fn open_draft(&self, issue: &Issue, kind: Kind) -> Option<Draft> {
        match kind {
            Kind::Question => Some(self.question_open(issue)),
            Kind::Memo => Some(self.memo_draft(issue)),
            Kind::Epic => Some(self.epic_open(issue)),
            _ => pointer_text(&issue.acceptance).is_none().then(|| Draft::new(issue, kind, Phase::Misfit).misfit(Misfit::FormNeither)),
        }
    }

    /// 親の memo の id（`parent-child` で結んだ先のうち memo）。
    fn parent_memos(&self, issue: &Issue) -> Vec<String> {
        let is_memo_id = |id: &str| self.find(id).is_some_and(|found| kind_of(found) == Kind::Memo);
        issue.deps.iter().filter(|dep| dep.kind == PARENT_CHILD && is_memo_id(&dep.on)).map(|dep| dep.on.clone()).collect()
    }

    fn question_open(&self, issue: &Issue) -> Draft {
        let links = Links { source: self.parent_memos(issue), ..Links::default() };
        Draft { since: created_at(issue), links, ..Draft::new(issue, Kind::Question, Phase::QuestionOpen) }
    }

    fn epic_open(&self, issue: &Issue) -> Draft {
        let children: Vec<&Issue> = self.input.issues.iter().filter(|child| points_to(child, PARENT_CHILD, &issue.id)).collect();
        if children.is_empty() || !children.iter().all(|child| is_closed(child)) {
            return Draft::new(issue, Kind::Epic, Phase::EpicOpen);
        }
        Draft { since: children.iter().filter_map(|child| closed_at(child)).max(), ..Draft::new(issue, Kind::Epic, Phase::EpicClosable) }
    }

    /// 閉じた bead の下書き（窓の外の閉じは載せない・misfit の閉じは窓を掛けない）。
    fn closed_draft(&self, issue: &Issue, kind: Kind) -> Option<Draft> {
        let base = self.closed_base(issue, kind);
        if kind == Kind::Question && self.input.unreflected.contains(&issue.id) {
            return Some(Draft { phase: Phase::RulingUnreflected, since: None, ..base });
        }
        if let Some(word) = self.closed_misfit(issue, kind) {
            return Some(base.misfit(word));
        }
        self.in_window(issue).then_some(base)
    }

    /// 閉じた bead の `*-closed` の下書き（結びと欄つき）。
    fn closed_base(&self, issue: &Issue, kind: Kind) -> Draft {
        let closed = |phase| Draft::new(issue, kind, phase);
        match kind {
            Kind::Question => Draft {
                since: closed_at(issue),
                links: Links { source: self.parent_memos(issue), rulings: self.ruling_of(issue), ..Links::default() },
                ..closed(Phase::QuestionClosed)
            },
            Kind::Memo => {
                let facts = self.facts(issue);
                let links = Links { questions: ids_of(&facts.children), promoted: ids_of(&facts.traced), ..Links::default() };
                Draft { links, extra: Extra::Memo { due: None, triggers: None, keep: None }, ..closed(Phase::MemoClosed) }
            }
            Kind::Epic => Draft { since: closed_at(issue), ..closed(Phase::EpicClosed) },
            _ => Draft { extra: Extra::Contract { pointer: pointer_text(&issue.acceptance).map(str::to_owned) }, ..closed(Phase::ContractClosed) },
        }
    }

    /// 閉じた問いの理由の裁定 id（読めなければ空）。
    fn ruling_of(&self, issue: &Issue) -> Vec<String> {
        match close_reason::read(&issue.close_reason, self.input.prefix) {
            Ok(Form::Ruling(id)) => vec![id],
            _ => Vec::new(),
        }
    }

    /// 閉じた時刻が窓の内か（時刻が読めない閉じは載せる）。
    fn in_window(&self, issue: &Issue) -> bool {
        closed_at(issue).is_none_or(|found| self.input.now.saturating_sub(found) <= self.input.window_s)
    }

    /// 閉じの misfit（2 つの線がどちらも在り、閉じた時刻が両方より後の閉じだけ・接頭辞が解けない周は判じない）。
    fn closed_misfit(&self, issue: &Issue, kind: Kind) -> Option<Misfit> {
        let prefix = self.input.prefix?;
        let closed = closed_at(issue)?;
        let (cutover, check) = (self.input.lines.cutover?, self.input.lines.close_check?);
        if closed <= cutover.max(check) {
            return None;
        }
        match close_reason::read(&issue.close_reason, Some(prefix)) {
            Err(defect) => defect_misfit(defect, kind),
            Ok(form) => self.form_misfit(issue, kind, (&form, closed)),
        }
    }

    /// 読めた理由の形から閉じの misfit（表の順・1 つの閉じに 1 語まで）。
    fn form_misfit(&self, issue: &Issue, kind: Kind, (form, closed): (&Form, u64)) -> Option<Misfit> {
        if !heads_of(kind).contains(&form.head()) {
            return Some(Misfit::CloseKindMismatch);
        }
        match form {
            Form::Landed { tail: LandedTail::Unreadable(_), .. } => Some(Misfit::CloseKindMismatch),
            Form::Duplicate(id) | Form::Successor(id) => self.find(id).is_none().then_some(Misfit::CloseUnresolved),
            Form::Merged(id) => self.merged(id, closed),
            Form::Promoted(ids) => self.promoted(issue, ids, closed),
            _ => None,
        }
    }

    /// まとめた: まとめ先が無い → unresolved・memo でない／この閉じより前に閉じた → not-open。
    fn merged(&self, id: &str, closed: u64) -> Option<Misfit> {
        let Some(target) = self.find(id) else { return Some(Misfit::CloseUnresolved) };
        let earlier = is_closed(target) && closed_at(target).is_some_and(|found| found < closed);
        (kind_of(target) != Kind::Memo || earlier).then_some(Misfit::MergedIntoNotOpen)
    }

    /// 昇格済み: id が台帳に無い → unresolved・閉じた時点で FR93 を満たさない → unmet・列が最後の行と違う → list-mismatch。
    fn promoted(&self, memo: &Issue, ids: &[String], closed: u64) -> Option<Misfit> {
        if ids.iter().any(|id| self.find(id).is_none()) {
            return Some(Misfit::CloseUnresolved);
        }
        if !settled(memo, self.input.issues, self.input.prefix, Some(closed)) {
            return Some(Misfit::PromotedUnmet);
        }
        match promotion::read(&memo.notes, self.input.prefix?) {
            Some(promotion::Line::Readable(line)) if same_set(&line.ids, ids) => None,
            _ => Some(Misfit::PromotedListMismatch),
        }
    }

    /// memo 1 つの材料。
    fn facts<'i>(&self, memo: &'i Issue) -> Facts<'i>
    where
        'a: 'i,
    {
        let issues: &'i [Issue] = self.input.issues;
        Facts {
            traced: traced_of(memo, issues).collect(),
            children: issues.iter().filter(|issue| is_child_question(issue, memo)).collect(),
            reading: trigger::read(&memo.description, &memo.notes, self.input.prefix),
        }
    }

    /// 引き金が満ちたかを判じる世界。
    fn world(&self, recurrences: usize) -> trigger::World<'_> {
        trigger::World { recurrences, write_set: self.input.write_set, closed: &self.closed, closed_pointers: &self.pointers, now: self.input.now }
    }

    /// 満ちた引き金。
    fn met<'f>(&self, facts: &'f Facts<'_>) -> Vec<&'f Trigger> {
        let world = self.world(facts.reading.recurrences);
        facts.reading.readable().filter(|found| trigger::met(found, &world)).collect()
    }

    /// 開いた memo の下書き（FR91 の順で判じる）。
    fn memo_draft(&self, memo: &Issue) -> Draft {
        let facts = self.facts(memo);
        let (phase, reason) = self.memo_phase(memo, &facts);
        let links = Links { questions: ids_of(&facts.children), promoted: ids_of(&facts.traced), ..Links::default() };
        Draft { reason, since: self.memo_since(&facts, phase, reason), links, extra: self.memo_extra(&facts), ..Draft::new(memo, Kind::Memo, phase) }
    }

    /// memo の局面と理由（form-both → promoting → asking → no-trigger → actionable → waiting）。
    fn memo_phase(&self, memo: &Issue, facts: &Facts<'_>) -> (Phase, Option<&'static str>) {
        let misfit = |word: Misfit| (Phase::Misfit, Some(word.as_str()));
        if pointer_text(&memo.acceptance).is_some() {
            return misfit(Misfit::FormBoth);
        }
        if facts.traced.iter().any(|contract| !is_closed(contract)) {
            return (Phase::MemoPromoting, Some(REASON_CONTRACT_OPEN));
        }
        if close_due(memo, self.input.issues, self.input.prefix, self.input.unjudged) {
            return (Phase::MemoPromoting, Some(REASON_CLOSE_DUE));
        }
        if facts.children.iter().any(|question| !is_closed(question)) {
            return (Phase::MemoAsking, None);
        }
        if facts.reading.readable().next().is_none() {
            return misfit(Misfit::MemoNoTrigger);
        }
        match self.actionable(memo, facts) {
            Some(reason) => (Phase::MemoActionable, Some(reason)),
            None => (Phase::MemoWaiting, self.kept(facts).then_some(REASON_KEEP)),
        }
    }

    /// 満ちた引き金が在り、keep の記帳が 1 本以上在るか（keep 済みと読む）。
    fn kept(&self, facts: &Facts<'_>) -> bool {
        !self.met(facts).is_empty() && !facts.reading.keeps.is_empty()
    }

    /// memo-actionable の理由（trigger-met → verdict → promotion-unmet / promotion-unreadable の順）。
    fn actionable(&self, memo: &Issue, facts: &Facts<'_>) -> Option<&'static str> {
        if !self.met(facts).is_empty() && facts.reading.keeps.is_empty() {
            return Some(REASON_TRIGGER_MET);
        }
        if self.input.unjudged.contains(&memo.id) {
            return Some(REASON_VERDICT);
        }
        match promotion::read(&memo.notes, self.input.prefix?) {
            Some(promotion::Line::Readable(_)) => Some(REASON_PROMOTION_UNMET),
            Some(promotion::Line::Unreadable { .. }) => Some(REASON_PROMOTION_UNREADABLE),
            None => None,
        }
    }

    /// memo の since（導ける 4 形だけ: promoting の 2 理由と、期日・依存・着地だけが満ちた trigger-met）。
    fn memo_since(&self, facts: &Facts<'_>, phase: Phase, reason: Option<&str>) -> Option<u64> {
        match (phase, reason) {
            (Phase::MemoPromoting, Some(REASON_CONTRACT_OPEN)) => {
                facts.traced.iter().filter(|contract| !is_closed(contract)).filter_map(|contract| created_at(contract)).max()
            }
            (Phase::MemoPromoting, Some(REASON_CLOSE_DUE)) => facts.traced.iter().filter_map(|contract| closed_at(contract)).max(),
            (Phase::MemoActionable, Some(REASON_TRIGGER_MET)) => {
                let times: Option<Vec<u64>> = self.met(facts).into_iter().map(|found| self.met_time(found)).collect();
                times?.into_iter().min()
            }
            _ => None,
        }
    }

    /// 満ちた引き金が満ちた時刻（期日は期日・依存と着地は相手の閉じ・再発と同梱は導けない）。
    fn met_time(&self, found: &Trigger) -> Option<u64> {
        match found {
            Trigger::Deadline(at) => Some(*at),
            Trigger::Dependency(id) => self.find(id).and_then(closed_at),
            Trigger::Landing(pointer) => self
                .input
                .issues
                .iter()
                .filter(|issue| is_closed(issue))
                .filter(|issue| pointer_text(&issue.acceptance).and_then(|text| parse_pointer(text).ok()).as_ref() == Some(pointer))
                .filter_map(closed_at)
                .min(),
            Trigger::Recurrence(_) | Trigger::Bundle(_) => None,
        }
    }

    /// memo の欄（満ちていない期日の最も早い値・引き金の写し・keep の記帳の有無）。
    fn memo_extra(&self, facts: &Facts<'_>) -> Extra {
        let world = self.world(facts.reading.recurrences);
        let views: Vec<TriggerView> = facts
            .reading
            .readable()
            .map(|found| TriggerView { form: found.kind().as_str().to_owned(), value: value_text(found), met: trigger::met(found, &world) })
            .collect();
        let due = facts
            .reading
            .readable()
            .filter_map(|found| match found {
                Trigger::Deadline(at) if !trigger::met(found, &world) => Some(*at),
                _ => None,
            })
            .min()
            .map(format_utc);
        Extra::Memo { due, triggers: (!views.is_empty()).then_some(views), keep: Some(!facts.reading.keeps.is_empty()) }
    }
}

#[cfg(test)]
mod tests {
    use super::{derive, Input, Lines, Output};
    use crate::case::{Extra, Kind, Part, TriggerView};
    use crate::fleet::epoch_of;
    use crate::fleet::json_lite::quote;
    use crate::seat::ledger::{issues_of, Issue};

    /// 契約の設計 pointer の行。
    const POINTER: &str = "design = docs/design/x.md#a";

    /// 閉じた時刻（2 つの線より後・窓の内）。
    const LATE: &str = "2026-09-28T00:00:00Z";

    /// 40 桁の 16 進。
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    /// 着地で閉じた理由。
    fn landed() -> String {
        format!("landed {SHA} ci=success")
    }

    /// 時刻の字を UNIX 秒にする。
    fn at(text: &str) -> u64 {
        epoch_of(text).expect("時刻の字を読める")
    }

    /// 台帳の bead 1 本（JSON の字にして [`issues_of`] で読む・`Issue` は字で組まない）。
    struct Spec {
        id: String,
        status: String,
        labels: Vec<String>,
        kind: String,
        acceptance: String,
        description: String,
        notes: String,
        reason: String,
        created: Option<String>,
        closed: Option<String>,
        deps: Vec<(String, String)>,
    }

    /// 開いた task（ほかの欄は空）。
    fn open(id: &str) -> Spec {
        let own = str::to_owned;
        Spec {
            id: own(id),
            status: own("open"),
            labels: Vec::new(),
            kind: own("task"),
            acceptance: String::new(),
            description: String::new(),
            notes: String::new(),
            reason: String::new(),
            created: None,
            closed: None,
            deps: Vec::new(),
        }
    }

    /// 閉じた task（理由と閉じた時刻つき）。
    fn closed(id: &str, reason: &str, when: &str) -> Spec {
        Spec { status: "closed".to_owned(), reason: reason.to_owned(), closed: Some(when.to_owned()), ..open(id) }
    }

    /// 開いた契約（設計 pointer を持つ）。
    fn contract(id: &str) -> Spec {
        open(id).design(POINTER)
    }

    /// 着地で閉じた契約。
    fn landed_contract(id: &str, when: &str) -> Spec {
        Spec { status: "closed".to_owned(), reason: landed(), closed: Some(when.to_owned()), ..contract(id) }
    }

    /// 開いた問い。
    fn question(id: &str) -> Spec {
        open(id).label("intake:question")
    }

    /// 開いた memo（引き金の行を description の `### 昇格条件` に書く）。
    fn memo(id: &str, triggers: &[&str]) -> Spec {
        let lines: String = triggers.iter().map(|found| format!("- 引き金: {found}\n")).collect();
        Spec { description: format!("### 出所\n### 観測\n### 候補\n### 昇格条件\n{lines}"), ..open(id).label("intake:memo") }
    }

    impl Spec {
        fn label(mut self, label: &str) -> Self {
            self.labels.push(label.to_owned());
            self
        }

        fn kind(mut self, kind: &str) -> Self {
            self.kind = kind.to_owned();
            self
        }

        fn design(mut self, line: &str) -> Self {
            self.acceptance = line.to_owned();
            self
        }

        fn described(mut self, description: &str) -> Self {
            self.description = description.to_owned();
            self
        }

        fn notes(mut self, notes: &str) -> Self {
            self.notes = notes.to_owned();
            self
        }

        fn created(mut self, when: &str) -> Self {
            self.created = Some(when.to_owned());
            self
        }

        fn dep(mut self, on: &str, kind: &str) -> Self {
            self.deps.push((on.to_owned(), kind.to_owned()));
            self
        }

        /// bd の JSON の 1 要素。
        fn json(&self) -> String {
            let labels: Vec<String> = self.labels.iter().map(|label| quote(label)).collect();
            let deps: Vec<String> =
                self.deps.iter().map(|(on, kind)| format!("{{\"depends_on_id\":{},\"type\":{}}}", quote(on), quote(kind))).collect();
            let time = |key: &str, value: &Option<String>| value.as_ref().map(|text| format!(",\"{key}\":{}", quote(text))).unwrap_or_default();
            format!(
                "{{\"id\":{},\"status\":{},\"issue_type\":{},\"labels\":[{}],\"acceptance_criteria\":{},\"description\":{},\"notes\":{},\"close_reason\":{},\"dependencies\":[{}]{}{}}}",
                quote(&self.id),
                quote(&self.status),
                quote(&self.kind),
                labels.join(","),
                quote(&self.acceptance),
                quote(&self.description),
                quote(&self.notes),
                quote(&self.reason),
                deps.join(","),
                time("created_at", &self.created),
                time("closed_at", &self.closed),
            )
        }
    }

    /// 台帳を JSON の字から作る。
    fn ledger(specs: &[Spec]) -> Vec<Issue> {
        let body: Vec<String> = specs.iter().map(Spec::json).collect();
        issues_of(&format!("[{}]", body.join(","))).expect("fixture の JSON を読める")
    }

    /// 周の入力（既定は 2026-10-01・窓 30 日・切り替え 09-20・close-check 09-25・接頭辞 s2）。
    struct Probe {
        now: u64,
        window_s: u64,
        cutover: Option<u64>,
        check: Option<u64>,
        prefix: Option<&'static str>,
        unreflected: Vec<String>,
        unjudged: Vec<String>,
        write_set: Vec<String>,
    }

    fn probe() -> Probe {
        Probe {
            now: at("2026-10-01T00:00:00Z"),
            window_s: 30 * 86_400,
            cutover: Some(at("2026-09-20T00:00:00Z")),
            check: Some(at("2026-09-25T00:00:00Z")),
            prefix: Some("s2"),
            unreflected: Vec::new(),
            unjudged: Vec::new(),
            write_set: Vec::new(),
        }
    }

    impl Probe {
        fn run(&self, specs: &[Spec]) -> Output {
            let issues = ledger(specs);
            derive(&Input {
                issues: &issues,
                prefix: self.prefix,
                now: self.now,
                window_s: self.window_s,
                lines: Lines { cutover: self.cutover, close_check: self.check },
                unreflected: &self.unreflected,
                unjudged: &self.unjudged,
                write_set: &self.write_set,
            })
        }
    }

    /// 部品 1 つ（無ければ落ちる）。
    fn part<'a>(out: &'a Output, id: &str) -> &'a Part {
        let ids: Vec<&str> = out.parts.iter().map(|found| found.id.as_str()).collect();
        out.parts.iter().find(|found| found.id == id).unwrap_or_else(|| panic!("部品 {id} が無い: {ids:?}"))
    }

    /// 部品の (局面・手番・理由)。
    fn shape<'a>(out: &'a Output, id: &str) -> (&'static str, &'static str, Option<&'a str>) {
        let found = part(out, id);
        (found.phase.as_str(), found.turn.as_str(), found.reason.as_deref())
    }

    /// 部品が載っているか。
    fn listed(out: &Output, id: &str) -> bool {
        out.parts.iter().any(|found| found.id == id)
    }

    /// 閉じの misfit の形（局面・手番・理由）。
    fn misfit(word: &str) -> (&'static str, &'static str, Option<&str>) {
        ("misfit", "seat", Some(word))
    }

    /// (1) 入力の組の不足で落ちず、開いた契約を返さない。
    #[test]
    fn phase_ledger_input_gaps_do_not_fail_and_open_contracts_are_not_returned() {
        let gap = Probe { prefix: None, cutover: None, check: None, window_s: 0, ..probe() };
        let empty = gap.run(&[]);
        let names = [Kind::Question, Kind::Memo, Kind::Contract, Kind::Epic].map(|kind| (kind, "ledger-prefix")).to_vec();
        assert_eq!((empty.parts.is_empty(), empty.unmeasured), (true, names.clone()), "空の台帳・接頭辞なしは部品 0 件で 4 種を名指す");
        let specs = [contract("s2-c"), memo("s2-m", &["再発 5"])];
        let out = probe().run(&specs);
        assert!(!listed(&out, "s2-c"), "開いた契約は返さない: {:?}", out.parts);
        assert_eq!((out.parts.len(), out.unmeasured.is_empty()), (1, true), "接頭辞が解ける周は名指さない");
        let out = gap.run(&specs);
        assert_eq!(shape(&out, "s2-m"), ("memo-waiting", "none", None), "接頭辞なしでも memo は導く");
        assert_eq!(out.unmeasured, names, "接頭辞なしは 4 種を名指す");
    }

    /// (2) 種類の判定は §2 の順・重なり 2 形と形の misfit 2 語。
    #[test]
    fn phase_ledger_kind_follows_the_section_2_order() {
        let specs = [
            question("s2-qm").label("intake:memo"),
            memo("s2-em", &["再発 5"]).kind("epic"),
            open("s2-dec").kind("decision"),
            open("s2-ep").kind("epic"),
            memo("s2-both", &["再発 5"]).design(POINTER),
            open("s2-none"),
        ];
        let out = probe().run(&specs);
        let want = [
            ("s2-qm", Kind::Question, ("question-open", "user", None)),
            ("s2-em", Kind::Memo, ("memo-waiting", "none", None)),
            ("s2-dec", Kind::Question, ("question-open", "user", None)),
            ("s2-ep", Kind::Epic, ("epic-open", "none", None)),
            ("s2-both", Kind::Memo, misfit("form-both")),
            ("s2-none", Kind::Contract, misfit("form-neither")),
        ];
        for (id, kind, shaped) in want {
            assert_eq!((part(&out, id).part, shape(&out, id)), (kind, shaped), "{id}");
        }
    }

    /// (2) memo の 4 局面の各 1（局面・手番・理由と結び）。
    #[test]
    fn phase_ledger_memo_four_phases_carry_turn_reason_and_links() {
        let specs = [
            memo("s2-p", &["再発 5"]),
            contract("s2-pc").dep("s2-p", "discovered-from"),
            memo("s2-a", &["再発 5"]),
            question("s2-aq").dep("s2-a", "parent-child"),
            memo("s2-t", &["再発 1"]).notes("[再発] 1 回目"),
            memo("s2-w", &["再発 5"]),
        ];
        let out = probe().run(&specs);
        assert_eq!(shape(&out, "s2-p"), ("memo-promoting", "none", Some("contract-open")));
        assert_eq!(shape(&out, "s2-a"), ("memo-asking", "user", None));
        assert_eq!(shape(&out, "s2-t"), ("memo-actionable", "seat", Some("trigger-met")));
        assert_eq!(shape(&out, "s2-w"), ("memo-waiting", "none", None));
        assert_eq!(part(&out, "s2-p").links.promoted, ["s2-pc"], "昇格した契約");
        assert_eq!(part(&out, "s2-a").links.questions, ["s2-aq"], "子の問い");
        assert!(part(&out, "s2-w").links.promoted.is_empty() && part(&out, "s2-w").overdue.is_none(), "結びは空・overdue は書き手が決める");
    }

    /// (2) 2 局面に当たる fixture 3 本で上の局面が勝つ（AC61）。
    #[test]
    fn phase_ledger_upper_phase_wins_when_two_phases_match() {
        let specs = [
            memo("s2-pa", &["再発 1"]).notes("[再発] x"),
            contract("s2-pac").dep("s2-pa", "discovered-from"),
            question("s2-paq").dep("s2-pa", "parent-child"),
            memo("s2-aa", &["再発 1"]).notes("[再発] x"),
            question("s2-aaq").dep("s2-aa", "parent-child"),
            memo("s2-aw", &["再発 1", "再発 9"]).notes("[再発] x"),
        ];
        let out = probe().run(&specs);
        assert_eq!(shape(&out, "s2-pa"), ("memo-promoting", "none", Some("contract-open")), "promoting ∧ asking ∧ actionable");
        assert_eq!(shape(&out, "s2-aa"), ("memo-asking", "user", None), "asking ∧ actionable");
        assert_eq!(shape(&out, "s2-aw"), ("memo-actionable", "seat", Some("trigger-met")), "actionable ∧ waiting");
    }

    /// (2) 引き金の行の無い memo は、actionable の条件（処置の無い判定）にも当たっても misfit（AC61）。
    #[test]
    fn phase_ledger_no_trigger_memo_is_misfit_even_when_a_verdict_awaits() {
        let probe = Probe { unjudged: vec!["s2-n".to_owned(), "s2-v".to_owned()], ..probe() };
        let specs = [memo("s2-n", &[]), memo("s2-u", &[]).described("### 昇格条件\n- 引き金: 謎 1\n"), memo("s2-v", &["再発 5"])];
        let out = probe.run(&specs);
        assert_eq!(shape(&out, "s2-n"), misfit("memo-no-trigger"), "引き金の行が 0 本");
        assert_eq!(shape(&out, "s2-u"), misfit("memo-no-trigger"), "読める引き金が 0 本");
        assert_eq!(shape(&out, "s2-v"), ("memo-actionable", "seat", Some("verdict")), "対照: 引き金が在れば verdict の actionable");
    }

    /// (2) actionable の理由 4 語の各 1。
    #[test]
    fn phase_ledger_actionable_reasons_are_the_four_words() {
        let probe = Probe { unjudged: vec!["s2-r2".to_owned()], ..probe() };
        let specs = [
            memo("s2-r1", &["再発 1"]).notes("[再発] x"),
            memo("s2-r2", &["再発 5"]),
            memo("s2-r3", &["再発 5"]).notes("昇格: 全部 s2-zz"),
            memo("s2-r4", &["再発 5"]).notes("昇格: 全部 zz"),
        ];
        let out = probe.run(&specs);
        for (id, reason) in [("s2-r1", "trigger-met"), ("s2-r2", "verdict"), ("s2-r3", "promotion-unmet"), ("s2-r4", "promotion-unreadable")] {
            assert_eq!(shape(&out, id), ("memo-actionable", "seat", Some(reason)), "{id}");
        }
    }

    /// (2) keep の付いた満ちは waiting（理由 keep）。keep の無い同じ満ちは actionable、満ちていない引き金に keep を付けても理由は無い。
    #[test]
    fn phase_ledger_keep_turns_a_met_trigger_into_waiting() {
        let specs = [
            memo("s2-k", &["再発 1"]).notes("[再発] x\n[keep] 様子を見る"),
            memo("s2-nk", &["再発 1"]).notes("[再発] x"),
            memo("s2-ku", &["再発 5"]).notes("[keep] 様子を見る"),
        ];
        let out = probe().run(&specs);
        assert_eq!(shape(&out, "s2-k"), ("memo-waiting", "none", Some("keep")));
        assert_eq!(shape(&out, "s2-nk"), ("memo-actionable", "seat", Some("trigger-met")), "keep が無ければ actionable");
        assert_eq!(shape(&out, "s2-ku"), ("memo-waiting", "none", None), "満ちていなければ keep の理由は無い");
    }

    /// (2) form-both は promoting より先に判じる（後に判じる実装を落とす）。
    #[test]
    fn phase_ledger_both_shapes_is_judged_before_promoting() {
        let specs = [memo("s2-b", &["再発 5"]).design(POINTER), contract("s2-bc").dep("s2-b", "discovered-from")];
        assert_eq!(shape(&probe().run(&specs), "s2-b"), misfit("form-both"));
    }

    /// (2) 引き金の行の無い promoting の memo は promoting（no-trigger を先に判じる実装を落とす）。
    #[test]
    fn phase_ledger_no_trigger_is_judged_after_promoting() {
        let specs = [memo("s2-np", &[]), contract("s2-npc").dep("s2-np", "discovered-from")];
        assert_eq!(shape(&probe().run(&specs), "s2-np"), ("memo-promoting", "none", Some("contract-open")));
    }

    /// (2) 引き金の行の無い asking の memo は asking（no-trigger を先に判じる実装を落とす）。
    #[test]
    fn phase_ledger_no_trigger_is_judged_after_asking() {
        let specs = [memo("s2-na", &[]), question("s2-naq").dep("s2-na", "parent-child")];
        assert_eq!(shape(&probe().run(&specs), "s2-na"), ("memo-asking", "user", None));
    }

    /// (2) 数えの外: 別の memo だけが `discovered-from` で指す memo は promoting にならない（contract なら promoting）。
    #[test]
    fn phase_ledger_a_memo_pointing_at_a_memo_does_not_promote() {
        let specs = [memo("s2-a", &["再発 5"]), memo("s2-b", &["再発 5"]).dep("s2-a", "discovered-from")];
        assert_eq!(shape(&probe().run(&specs), "s2-a"), ("memo-waiting", "none", None), "memo の指しは辿れる契約に数えない");
        let specs = [memo("s2-a", &["再発 5"]), contract("s2-c").dep("s2-a", "discovered-from")];
        assert_eq!(shape(&probe().run(&specs), "s2-a").0, "memo-promoting", "対照: contract の指しは数える");
    }

    /// (2) 数えの外: `parent-child` でなく `discovered-from` で結ぶ開いた問いだけの memo は asking にならない（parent-child なら asking）。
    #[test]
    fn phase_ledger_a_discovered_from_question_does_not_make_the_memo_ask() {
        let specs = [memo("s2-a", &["再発 5"]), question("s2-q").dep("s2-a", "discovered-from")];
        assert_eq!(shape(&probe().run(&specs), "s2-a"), ("memo-waiting", "none", None), "discovered-from の問いは子に数えない");
        let specs = [memo("s2-a", &["再発 5"]), question("s2-q").dep("s2-a", "parent-child")];
        assert_eq!(shape(&probe().run(&specs), "s2-a").0, "memo-asking", "対照: parent-child の問いは数える");
    }

    /// (4) 問いの 3 局面と links（閉じた問いの `rulings` が閉じの理由の裁定 id・`source` が親の memo）。
    #[test]
    fn phase_ledger_question_three_phases_and_links() {
        let probe = Probe { unreflected: vec!["s2-q2".to_owned()], ..probe() };
        let specs = [
            memo("s2-m", &["再発 5"]),
            question("s2-q1").dep("s2-m", "parent-child").created("2026-09-29T01:02:03Z"),
            closed("s2-q2", "裁定 s2-q2:20260928T0000Z-1", LATE).label("intake:question"),
            closed("s2-q3", "裁定 s2-q3:20260928T0100Z-1", LATE).label("intake:question").dep("s2-m", "parent-child"),
        ];
        let out = probe.run(&specs);
        assert_eq!(shape(&out, "s2-q1"), ("question-open", "user", None));
        assert_eq!((part(&out, "s2-q1").links.source.clone(), part(&out, "s2-q1").closed), (vec!["s2-m".to_owned()], false));
        assert_eq!(shape(&out, "s2-q2"), ("ruling-unreflected", "seat", None), "未反映の置き場に在る閉じた問い");
        assert_eq!(shape(&out, "s2-q3"), ("question-closed", "none", None));
        assert_eq!((part(&out, "s2-q3").links.rulings.clone(), part(&out, "s2-q3").links.source.clone(), part(&out, "s2-q3").closed), (vec!["s2-q3:20260928T0100Z-1".to_owned()], vec!["s2-m".to_owned()], true));
    }

    /// (5) epic の 3 局面と、子の無い開いた epic が epic-open。
    #[test]
    fn phase_ledger_epic_three_phases_and_a_childless_epic() {
        let specs = [
            open("s2-e1").kind("epic"),
            open("s2-e1a").dep("s2-e1", "parent-child"),
            open("s2-e2").kind("epic"),
            landed_contract("s2-e2a", LATE).dep("s2-e2", "parent-child"),
            landed_contract("s2-e2b", "2026-09-29T00:00:00Z").dep("s2-e2", "parent-child"),
            open("s2-e3").kind("epic"),
            Spec { status: "closed".to_owned(), reason: "完了".to_owned(), closed: Some(LATE.to_owned()), ..open("s2-e4").kind("epic") },
        ];
        let out = probe().run(&specs);
        assert_eq!(shape(&out, "s2-e1"), ("epic-open", "none", None), "子が開いている");
        assert_eq!(shape(&out, "s2-e2"), ("epic-closable", "seat", None), "子が全部閉じた");
        assert_eq!(shape(&out, "s2-e3"), ("epic-open", "none", None), "子の無い開いた epic は closable でない");
        assert_eq!(shape(&out, "s2-e4"), ("epic-closed", "none", None));
        assert!(part(&out, "s2-e4").closed && !part(&out, "s2-e2").closed, "closed の欄");
    }

    /// (6) close-kind-mismatch の 4 形（種類と頭の食い違い・空・9 頭の外・着地の値の崩れ）と対照。
    #[test]
    fn phase_ledger_close_kind_mismatch_four_forms() {
        let specs = [
            closed("s2-k1", "裁定 batch:x", LATE).design(POINTER),
            closed("s2-k2", "", LATE).design(POINTER),
            closed("s2-k3", "終了した", LATE).design(POINTER),
            closed("s2-k4", "landed abc ci=success", LATE).design(POINTER),
            landed_contract("s2-ok", LATE),
        ];
        let out = probe().run(&specs);
        for id in ["s2-k1", "s2-k2", "s2-k3", "s2-k4"] {
            assert_eq!((shape(&out, id), part(&out, id).closed, part(&out, id).part), (misfit("close-kind-mismatch"), true, Kind::Contract), "{id}");
        }
        assert_eq!(shape(&out, "s2-ok"), ("contract-closed", "none", None), "対照: 読める着地は contract-closed");
    }

    /// (6) close-unresolved の 2 形（値の崩れ・台帳に無い id）と対照。
    #[test]
    fn phase_ledger_close_unresolved_two_forms() {
        let specs = [
            closed("s2-u1", "重複 bogus", LATE).design(POINTER),
            closed("s2-u2", "重複 s2-nope", LATE).design(POINTER),
            closed("s2-u3", "重複 s2-real", LATE).design(POINTER),
            contract("s2-real"),
        ];
        let out = probe().run(&specs);
        assert_eq!(shape(&out, "s2-u1"), misfit("close-unresolved"), "値の崩れ");
        assert_eq!(shape(&out, "s2-u2"), misfit("close-unresolved"), "台帳に無い id");
        assert_eq!(shape(&out, "s2-u3"), ("contract-closed", "none", None), "対照: 台帳に在る id");
    }

    /// (6) merged-into-not-open の 2 形（まとめ先が memo でない・この閉じより前に閉じていた）と対照 2 本。
    #[test]
    fn phase_ledger_merged_into_not_open_two_forms() {
        let memo_closed = |id: &str, reason: &str, when: &str| Spec { status: "closed".to_owned(), reason: reason.to_owned(), closed: Some(when.to_owned()), ..memo(id, &[]) };
        let specs = [
            contract("s2-target"),
            memo_closed("s2-m1", "まとめた s2-target", LATE),
            memo_closed("s2-old", "見送り batch:x", "2026-09-26T00:00:00Z"),
            memo_closed("s2-m2", "まとめた s2-old", LATE),
            memo("s2-open", &["再発 5"]),
            memo_closed("s2-m3", "まとめた s2-open", LATE),
            memo_closed("s2-new", "見送り batch:x", "2026-09-29T00:00:00Z"),
            memo_closed("s2-m4", "まとめた s2-new", LATE),
        ];
        let out = probe().run(&specs);
        assert_eq!(shape(&out, "s2-m1"), misfit("merged-into-not-open"), "まとめ先が memo でない");
        assert_eq!(shape(&out, "s2-m2"), misfit("merged-into-not-open"), "まとめ先がこの閉じより前に閉じた");
        assert_eq!(shape(&out, "s2-m3"), ("memo-closed", "none", None), "対照: 開いた memo");
        assert_eq!(shape(&out, "s2-m4"), ("memo-closed", "none", None), "対照: この閉じより後に閉じた memo");
    }

    /// 閉じた memo と辿れる契約 2 本（昇格済みの閉じの fixture）。
    fn promoted_group(memo: &str, listed_ids: &[&str], second_closed: Option<&str>) -> Vec<Spec> {
        let (first, second) = (format!("{memo}1"), format!("{memo}2"));
        let reason = format!("昇格済み {}", listed_ids.join(" "));
        let closed_memo = Spec { notes: format!("昇格: 全部 {first} {second}"), ..closed(memo, &reason, LATE).label("intake:memo") };
        let link = |spec: Spec| spec.dep(memo, "discovered-from");
        let second_spec = match second_closed {
            Some(when) => landed_contract(&second, when),
            None => contract(&second),
        };
        vec![closed_memo, link(landed_contract(&first, "2026-09-27T00:00:00Z")), link(second_spec)]
    }

    /// (6) promoted-unmet（閉じた時点で FR93 を満たさない）と promoted-list-mismatch（理由の列が最後の行と違う）と対照。
    #[test]
    fn phase_ledger_promoted_unmet_and_list_mismatch() {
        let mut specs = promoted_group("s2-pok", &["s2-pok1", "s2-pok2"], Some("2026-09-27T00:00:00Z"));
        specs.extend(promoted_group("s2-pla", &["s2-pla1", "s2-pla2"], Some("2026-09-29T00:00:00Z")));
        specs.extend(promoted_group("s2-pop", &["s2-pop1", "s2-pop2"], None));
        specs.extend(promoted_group("s2-pli", &["s2-pli1"], Some("2026-09-27T00:00:00Z")));
        let out = probe().run(&specs);
        assert_eq!(shape(&out, "s2-pok"), ("memo-closed", "none", None), "対照: 条件を満たす閉じ");
        assert_eq!(shape(&out, "s2-pla"), misfit("promoted-unmet"), "辿れる契約の閉じがこの閉じより後");
        assert_eq!(shape(&out, "s2-pop"), misfit("promoted-unmet"), "辿れる契約が開いている");
        assert_eq!(shape(&out, "s2-pli"), misfit("promoted-list-mismatch"), "理由の列が最後の昇格の行と違う");
    }

    /// (6) 線の前後: close-check の線より前の同じ閉じは `*-closed`・後へ動かすと 1 件（AC61）。
    #[test]
    fn phase_ledger_close_before_the_close_check_line_stays_closed() {
        let before = probe().run(&[closed("s2-c", "", "2026-09-22T00:00:00Z").design(POINTER)]);
        assert_eq!(shape(&before, "s2-c"), ("contract-closed", "none", None), "線より前の閉じは判じない");
        let after = probe().run(&[closed("s2-c", "", LATE).design(POINTER)]);
        assert_eq!(shape(&after, "s2-c"), misfit("close-kind-mismatch"), "線の後へ動かすと 1 件");
        assert_eq!(after.parts.iter().filter(|found| found.phase.as_str() == "misfit").count(), 1);
    }

    /// (6) close-check が None の repo は数えない・線を渡すと 1 件。切り替えの線が close-check の線より後なら、その前の閉じも数えない。
    #[test]
    fn phase_ledger_missing_or_later_lines_count_nothing() {
        let specs = [closed("s2-c", "", LATE).design(POINTER)];
        let none = Probe { check: None, ..probe() }.run(&specs);
        assert_eq!(shape(&none, "s2-c"), ("contract-closed", "none", None), "close-check が None");
        assert_eq!(shape(&probe().run(&specs), "s2-c"), misfit("close-kind-mismatch"), "対照: 線を渡す");
        let no_cutover = Probe { cutover: None, ..probe() }.run(&specs);
        assert_eq!(shape(&no_cutover, "s2-c"), ("contract-closed", "none", None), "切り替えの線が無い");
        let later = Probe { cutover: Some(at("2026-09-29T00:00:00Z")), ..probe() }.run(&specs);
        assert_eq!(shape(&later, "s2-c"), ("contract-closed", "none", None), "切り替えの線より前・close-check の線より後");
    }

    /// (6) 裁定の値の崩れは行 a2 の語で、行 a1 は `*-closed` に置く（`裁定 <id> 束 batch:x`・値の崩れた見送り）。
    #[test]
    fn phase_ledger_ruling_shapes_are_left_to_the_next_row() {
        let specs = [
            closed("s2-q", "裁定 s2-q:20260928T0000Z-1 束 batch:x", LATE).label("intake:question"),
            closed("s2-m", "見送り 謎の値", LATE).label("intake:memo"),
            closed("s2-q2", "裁定 謎の値", LATE).label("intake:question"),
        ];
        let out = probe().run(&specs);
        assert_eq!(shape(&out, "s2-q"), ("question-closed", "none", None), "束 batch:x の閉じ");
        assert_eq!(shape(&out, "s2-m"), ("memo-closed", "none", None), "値の崩れた見送り");
        assert_eq!(shape(&out, "s2-q2"), ("question-closed", "none", None), "値の崩れた裁定");
    }

    /// (6) 窓の外の閉じは載らず、misfit の閉じは窓を掛けずに残る。
    #[test]
    fn phase_ledger_window_drops_old_closes_but_keeps_misfit_closes() {
        let probe = Probe { window_s: 86_400, ..probe() };
        let specs = [
            landed_contract("s2-old", LATE),
            landed_contract("s2-new", "2026-09-30T12:00:00Z"),
            closed("s2-bad", "", LATE).design(POINTER),
        ];
        let out = probe.run(&specs);
        assert!(!listed(&out, "s2-old"), "窓の外の閉じ");
        assert_eq!(shape(&out, "s2-new"), ("contract-closed", "none", None), "窓の内の閉じ");
        assert_eq!(shape(&out, "s2-bad"), misfit("close-kind-mismatch"), "misfit の閉じは窓の外でも残る");
    }

    /// (6) 接頭辞が None の周は閉じの misfit を判じず `unmeasured` に 4 種を `ledger-prefix` で名指す。
    #[test]
    fn phase_ledger_prefix_none_judges_no_close_shape_and_names_four_kinds() {
        let specs = [closed("s2-bad", "", LATE).design(POINTER)];
        let out = Probe { prefix: None, ..probe() }.run(&specs);
        assert_eq!(shape(&out, "s2-bad"), ("contract-closed", "none", None), "接頭辞が解けない周は判じない");
        assert_eq!(out.unmeasured.len(), 4);
        assert!(out.unmeasured.iter().all(|(_, reason)| *reason == "ledger-prefix"));
        assert_eq!(shape(&probe().run(&specs), "s2-bad"), misfit("close-kind-mismatch"), "対照: 接頭辞が解ける");
    }

    /// 導ける 8 つの部品の since 用の台帳（前半: 問い・epic・promoting の 2 理由・epic-closable）。
    fn since_specs_a() -> Vec<Spec> {
        vec![
            closed("s2-sq", "裁定 s2-sq:20260928T0000Z-1", "2026-09-28T01:02:03Z").label("intake:question"),
            Spec { status: "closed".to_owned(), reason: "完了".to_owned(), closed: Some("2026-09-28T04:00:00Z".to_owned()), ..open("s2-se").kind("epic") },
            question("s2-so").created("2026-09-29T05:06:07Z"),
            memo("s2-mp", &["再発 5"]),
            contract("s2-mp1").dep("s2-mp", "discovered-from").created("2026-09-27T00:00:00Z"),
            contract("s2-mp2").dep("s2-mp", "discovered-from").created("2026-09-29T00:00:00Z"),
            Spec { created: Some("2026-09-30T00:00:00Z".to_owned()), ..landed_contract("s2-mp3", "2026-09-30T01:00:00Z").dep("s2-mp", "discovered-from") },
            Spec { notes: "昇格: 全部 s2-md1 s2-md2".to_owned(), ..memo("s2-md", &["再発 5"]) },
            landed_contract("s2-md1", "2026-09-26T00:00:00Z").dep("s2-md", "discovered-from"),
            landed_contract("s2-md2", LATE).dep("s2-md", "discovered-from"),
            open("s2-ec").kind("epic"),
            landed_contract("s2-ec1", "2026-09-27T00:00:00Z").dep("s2-ec", "parent-child"),
            landed_contract("s2-ec2", "2026-09-29T10:00:00Z").dep("s2-ec", "parent-child"),
        ]
    }

    /// 導ける 8 つの部品の since 用の台帳（後半: 期日・依存・着地の満ち）。
    fn since_specs_b() -> Vec<Spec> {
        vec![
            memo("s2-mdl", &["期日 2026-09-30T00:00Z"]),
            memo("s2-mdp", &["依存 s2-dep"]),
            landed_contract("s2-dep", "2026-09-29T12:00:00Z"),
            memo("s2-mlg", &["着地 docs/design/y.md#b"]),
            landed_contract("s2-lg1", "2026-09-29T00:00:00Z").design("design = docs/design/y.md#b"),
            landed_contract("s2-lg2", "2026-09-27T03:00:00Z").design("design = docs/design/y.md#b"),
        ]
    }

    /// (7) since を導ける 8 つの部品の各 1。
    #[test]
    fn phase_ledger_since_is_derived_for_the_eight_parts() {
        let specs: Vec<Spec> = since_specs_a().into_iter().chain(since_specs_b()).collect();
        let out = probe().run(&specs);
        let want = [
            ("s2-sq", "2026-09-28T01:02:03Z"),
            ("s2-se", "2026-09-28T04:00:00Z"),
            ("s2-so", "2026-09-29T05:06:07Z"),
            ("s2-mp", "2026-09-29T00:00:00Z"),
            ("s2-md", "2026-09-28T00:00:00Z"),
            ("s2-ec", "2026-09-29T10:00:00Z"),
            ("s2-mdl", "2026-09-30T00:00:00Z"),
            ("s2-mdp", "2026-09-29T12:00:00Z"),
            ("s2-mlg", "2026-09-27T03:00:00Z"),
        ];
        for (id, since) in want {
            assert_eq!(part(&out, id).since.as_deref(), Some(since), "{id} は {:?}", shape(&out, id));
        }
        assert_eq!(shape(&out, "s2-md"), ("memo-promoting", "vessel", Some("close-due")));
        assert_eq!(shape(&out, "s2-mp"), ("memo-promoting", "none", Some("contract-open")), "promoting は開いた契約の起票の最新");
        assert_eq!(shape(&out, "s2-mlg"), ("memo-actionable", "seat", Some("trigger-met")));
    }

    /// (7) 導けない部品の since は None（1 本の歯に 9 つの fixture と、導けない満ちを含む満ち）。
    #[test]
    fn phase_ledger_since_is_none_for_the_rest() {
        let probe = Probe { unjudged: vec!["s2-mv".to_owned()], unreflected: vec!["s2-ru".to_owned()], write_set: vec!["+crates/x.rs".to_owned()], ..probe() };
        let specs = [
            memo("s2-ma", &["再発 5"]),
            question("s2-maq").dep("s2-ma", "parent-child"),
            memo("s2-mw", &["再発 5"]),
            memo("s2-mv", &["再発 5"]),
            memo("s2-mr", &["再発 1"]).notes("[再発] x"),
            memo("s2-mb", &["同梱 crates/x.rs"]),
            memo("s2-mx", &["再発 1", "期日 2026-09-30T00:00Z"]).notes("[再発] x"),
            closed("s2-mc", "見送り batch:x", LATE).label("intake:memo"),
            landed_contract("s2-cc", LATE),
            closed("s2-ru", "裁定 s2-ru:20260928T0000Z-1", LATE).label("intake:question"),
            memo("s2-mf", &["再発 5"]).design(POINTER),
        ];
        let out = probe.run(&specs);
        let rest = ["s2-ma", "s2-mw", "s2-mv", "s2-mr", "s2-mb", "s2-mx", "s2-mc", "s2-cc", "s2-ru", "s2-mf"];
        for id in rest {
            assert_eq!(part(&out, id).since, None, "{id} は導けない: {:?}", shape(&out, id));
        }
        assert_eq!(shape(&out, "s2-mb"), ("memo-actionable", "seat", Some("trigger-met")), "同梱の満ちも trigger-met");
    }

    /// (8) memo の due・triggers・keep と、閉じた契約の pointer。
    #[test]
    fn phase_ledger_memo_fields_and_the_pointer_of_a_closed_contract() {
        let triggers = ["期日 2026-09-01T00:00Z", "期日 2026-10-09T00:00Z", "期日 2026-10-05T00:00Z", "再発 5"];
        let specs = [memo("s2-m", &triggers).notes("[keep] 様子を見る"), memo("s2-n", &["期日 2026-09-01T00:00Z"]), landed_contract("s2-c", LATE)];
        let out = probe().run(&specs);
        let view = |form: &str, value: &str, met: bool| TriggerView { form: form.to_owned(), value: value.to_owned(), met };
        let want = vec![
            view("期日", "2026-09-01T00:00:00Z", true),
            view("期日", "2026-10-09T00:00:00Z", false),
            view("期日", "2026-10-05T00:00:00Z", false),
            view("再発", "5", false),
        ];
        let memo_extra = Extra::Memo { due: Some("2026-10-05T00:00:00Z".to_owned()), triggers: Some(want), keep: Some(true) };
        assert_eq!(part(&out, "s2-m").extra, memo_extra, "満ちていない期日の最も早い値・引き金の写し・keep");
        let only_met = Extra::Memo { due: None, triggers: Some(vec![view("期日", "2026-09-01T00:00:00Z", true)]), keep: Some(false) };
        assert_eq!(part(&out, "s2-n").extra, only_met, "期日が全部満ちていれば due は無い");
        assert_eq!(part(&out, "s2-c").extra, Extra::Contract { pointer: Some("docs/design/x.md#a".to_owned()) }, "閉じた契約の pointer");
    }
}
