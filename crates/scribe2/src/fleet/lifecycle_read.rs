//! 局面の出力の読み手 1 本と doctor の 3 行の字（設計 docs/design/ledger-form.md §19 約束 1・2・FR51・FR88・FR94・ADR-0088）。
//!
//! 読み手（[`read`]）は置き場と、出力と比べる入力の種類の組（[`Input`]・台帳・event log・main の部分集合）を受け、閉じた 3 値
//! （[`Lifecycle`]・無い・読めない・読めた）を返す。読む順は stale → json（case-lifecycle §5.2）で、json は行 c の出力の読み
//! （[`read_output`]・`fleet lifecycle show` が使う 1 本）、stale は [`read_stale`]、今の印は [`read_ledger`]・[`read_events`]・
//! [`read_main`] を組の種類ごとに呼ぶ（写しを持たず、台帳も git も撃たない）。古い理由（[`Reason`]）は古さの印の種類と、比べる組の
//! 今の値が出力の入力の印と違う種類（今の印を読めない種類も違うと数える・C10）をこの順に並べる。
//!
//! doctor の 3 行（[`doctor_lines`]）は読み手を台帳・event log・main の 3 つ全部で呼び、出力が持つ部品と `owned` から数える。

use super::epoch_of;
use super::lifecycle::{read_output, Output, Owned, Reading};
use super::lifecycle_mark::{read_events, read_ledger, read_main, read_stale, Kind as MarkKind, Marks, Stale};
use crate::case::{Kind, Part, Phase, Sink};
use std::path::Path;

/// 出力の入力の印と今の印を比べる種類（読み手ごとに部分集合を選ぶ・case-lifecycle §15 の表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// 台帳。
    Ledger,
    /// event log。
    Events,
    /// main。
    Main,
}

impl Input {
    /// 全種類（理由の並びの順）。
    pub const ALL: [Self; 3] = [Self::Ledger, Self::Events, Self::Main];

    /// 理由の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ledger => "ledger",
            Self::Events => "events",
            Self::Main => "main",
        }
    }
}

/// 古い理由 1 つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// 古さの印の種類が在る。
    Marked(MarkKind),
    /// 比べる組の今の値が出力の入力の印と違う（今の値を読めない種類を含む）。
    Differs(Input),
}

impl Reason {
    /// 理由の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Marked(kind) => kind.as_str(),
            Self::Differs(input) => input.as_str(),
        }
    }
}

/// 読めた出力。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// 出力の生成の時刻。
    pub generated_at: String,
    /// 部品。
    pub parts: Vec<Part>,
    /// 席の手番の閾値越えの数え。
    pub owned: Owned,
    /// 出力の入力の印。
    pub marks: Marks,
    /// 古い理由（古さの印の種類 → 台帳 → event log → main の順・古くなければ空）。
    pub stale: Vec<Reason>,
}

/// 読み手の返り（閉じた 3 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lifecycle {
    /// `lifecycle.json` が無い。
    Absent,
    /// json か古さの印の file が読めない。
    Unreadable,
    /// 読めた。
    Read(Box<Found>),
}

/// 今の印が出力の入力の印と違うか（今の印を読めなければ違う）。
fn differs(input: Input, state_dir: &Path, repo: &Path, marks: &Marks) -> bool {
    match input {
        Input::Ledger => read_ledger(repo).as_ref() != Some(&marks.ledger),
        Input::Events => read_events(state_dir).as_ref() != Some(&marks.events),
        Input::Main => read_main(repo).as_deref() != Some(marks.main.as_str()),
    }
}

/// 局面の出力を読む（stale → json の順・`against` は今の印と比べる組）。
pub fn read(state_dir: &Path, repo: &Path, against: &[Input]) -> Lifecycle {
    let marked: Vec<MarkKind> = match read_stale(state_dir) {
        Stale::Unreadable => return Lifecycle::Unreadable,
        Stale::Absent => Vec::new(),
        Stale::Marks(marks) => MarkKind::ALL.into_iter().filter(|kind| marks.iter().any(|mark| mark.kind == *kind)).collect(),
    };
    let out: Output = match read_output(state_dir) {
        Reading::Absent => return Lifecycle::Absent,
        Reading::Unreadable => return Lifecycle::Unreadable,
        Reading::Read(found) => *found,
    };
    let changed = Input::ALL.into_iter().filter(|input| against.contains(input) && differs(*input, state_dir, repo, &out.marks));
    let stale = marked.into_iter().map(Reason::Marked).chain(changed.map(Reason::Differs)).collect();
    Lifecycle::Read(Box::new(Found { generated_at: out.generated_at, parts: out.parts, owned: out.owned, marks: out.marks, stale }))
}

/// 3 行の頭。
const HEADS: [&str; 3] = ["lifecycle-utterance:", "lifecycle-memo:", "lifecycle-owned:"];

/// `since` から出力の生成までの時間（時間・読めない時刻は `None`）。
fn age_h(generated_at: &str, since: &str) -> Option<u64> {
    Some(epoch_of(generated_at)?.saturating_sub(epoch_of(since)?) / 3_600)
}

/// 最も古い `since` を持つ部品（`since` を読めない部品は数えない）。
fn oldest<'p>(parts: impl Iterator<Item = &'p Part>) -> Option<(&'p Part, &'p str)> {
    parts.filter_map(|part| part.since.as_deref().and_then(|since| epoch_of(since).map(|at| (at, part, since)))).min_by_key(|(at, _, _)| *at).map(|(_, part, since)| (part, since))
}

/// 未仕分けの発話（部品の種類が発話で局面が utterance-open）。
fn open_utterances(parts: &[Part]) -> impl Iterator<Item = &Part> {
    parts.iter().filter(|part| part.part == Kind::Utterance && part.phase == Phase::UtteranceOpen)
}

/// 未仕分けの発話の数（doctor の発話の行と管理 tick の alarm の語 `unsorted` が呼ぶ 1 本・FR94）。
pub fn unsorted(parts: &[Part]) -> usize {
    open_utterances(parts).count()
}

/// 発話の行。窓の中の仕分け済みの発話は、行き先が会話だけなら chat・要望か答えの行き先を持てば request と、発話の単位で 1 と数える。
fn utterance_line(parts: &[Part]) -> String {
    let utterances = || parts.iter().filter(|part| part.part == Kind::Utterance);
    let sorted = || utterances().filter(|part| part.phase == Phase::UtteranceSorted);
    let request = sorted().filter(|part| part.links.destination.iter().any(|found| found.to != Sink::ToChat)).count();
    let chat = sorted().filter(|part| !part.links.destination.iter().any(|found| found.to != Sink::ToChat)).count();
    let first = oldest(open_utterances(parts)).map_or("-", |(_, since)| since);
    format!("{} unsorted={} oldest={first} request={request} chat={chat}", HEADS[0], unsorted(parts))
}

/// memo の行（処置の待ちの本数と、その中の最古の年齢）。
fn memo_line(parts: &[Part], generated_at: &str) -> String {
    let memos = || parts.iter().filter(|part| part.part == Kind::Memo);
    let waiting = || memos().filter(|part| part.phase == Phase::MemoActionable);
    let first = oldest(waiting())
        .and_then(|(part, since)| Some(format!("{}:{}h", part.id, age_h(generated_at, since)?)))
        .unwrap_or_else(|| "-".to_owned());
    format!("{} open={} actionable={} oldest={first}", HEADS[1], memos().filter(|part| !part.closed).count(), waiting().count())
}

/// 席の手番の閾値越えの行（出力が持つ件数と最古をそのまま写す）。
fn owned_line(owned: &Owned, generated_at: &str) -> String {
    let first = owned
        .oldest
        .as_ref()
        .and_then(|found| Some(format!("{}:{}:{}:{}h", found.part.as_str(), found.id, found.phase.as_str(), age_h(generated_at, &found.since)?)))
        .unwrap_or_else(|| "-".to_owned());
    format!("{} count={} oldest={first}", HEADS[2], owned.count)
}

/// doctor の 3 行（読めた周は古い理由を各行の末尾に ` stale=<種類,…>`・無いか読めない周は件数を出さず `unreadable reason=<absent|unparsed>`）。
pub fn doctor_lines(state_dir: &Path, repo: &Path) -> Vec<String> {
    let reason = match read(state_dir, repo, &Input::ALL) {
        Lifecycle::Read(found) => {
            let stale = if found.stale.is_empty() {
                String::new()
            } else {
                format!(" stale={}", found.stale.iter().map(|reason| reason.as_str()).collect::<Vec<_>>().join(","))
            };
            let lines = [utterance_line(&found.parts), memo_line(&found.parts, &found.generated_at), owned_line(&found.owned, &found.generated_at)];
            return lines.map(|line| format!("{line}{stale}")).to_vec();
        }
        Lifecycle::Absent => "absent",
        Lifecycle::Unreadable => "unparsed",
    };
    HEADS.map(|head| format!("{head} unreadable reason={reason}")).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::lifecycle::{render_output, Scope};
    use crate::fleet::lifecycle_mark::{add_mark, fleet_dir, publish, Added, Mark, Value, JSON_FILE, STALE_FILE};
    use crate::fleet::store::{events_path, LockPolicy};
    use std::path::PathBuf;

    const POLICY: LockPolicy = LockPolicy { retry_ms: 50, stale_ms: 600_000 };
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const OTHER: &str = "fedcba9876543210fedcba9876543210fedcba98";

    fn put(path: &Path, text: &str) {
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(path));
        assert!(std::fs::write(path, text).is_ok(), "{} を書けた", path.display());
    }

    /// 台帳の files の形の印と main の ref を持つ repo と、空の置き場。
    fn place(name: &str) -> (PathBuf, PathBuf) {
        let repo = crate::pipe::fixture::scratch(&format!("lifecycle-read-{name}"));
        let state = crate::pipe::fixture::scratch(&format!("lifecycle-read-{name}-state"));
        put(&repo.join(".beads/issues.jsonl"), "[]\n");
        put(&repo.join(".git/refs/remotes/origin/main"), &format!("{SHA}\n"));
        (repo, state)
    }

    /// 今の入力の印で出力を書く。
    fn written(repo: &Path, state: &Path) -> Marks {
        let marks = Marks {
            ledger: read_ledger(repo).unwrap_or_else(|| panic!("台帳の印を読める")),
            events: read_events(state).unwrap_or_else(|| panic!("event log の印を読める")),
            main: read_main(repo).unwrap_or_else(|| panic!("main の印を読める")),
        };
        let out = Output {
            generated_at: "2026-10-01T00:00:00Z".to_owned(),
            scope: Scope::Full,
            full_at: "2026-10-01T00:00:00Z".to_owned(),
            interval_s: None,
            closed_window_h: Some(72),
            marks: marks.clone(),
            unmeasured: Vec::new(),
            owned: Owned::default(),
            parts: Vec::new(),
        };
        let _ = std::fs::create_dir_all(fleet_dir(state));
        assert!(publish(&fleet_dir(state), JSON_FILE, &render_output(&out)).is_ok(), "出力を書けた");
        marks
    }

    /// 古さの印を 1 つ足す。
    fn mark(state: &Path, kind: MarkKind, value: Value) {
        let found = Mark { kind, at: "2026-10-01T00:00:00Z".to_owned(), value };
        assert_eq!(add_mark(state, &found, POLICY), Added::Added, "{kind:?} の印を足せた");
    }

    fn words(found: Lifecycle) -> Vec<&'static str> {
        match found {
            Lifecycle::Read(read) => read.stale.iter().map(|reason| reason.as_str()).collect(),
            other => panic!("読めた周のはず: {other:?}"),
        }
    }

    /// (i) 出力の無い置き場は無い・json が壊れていれば読めない・読めた出力は部品と印を持つ（古くなければ理由は空）。
    #[test]
    fn lifecycle_read_returns_absent_unreadable_or_read() {
        let (repo, state) = place("three");
        assert_eq!(read(&state, &repo, &Input::ALL), Lifecycle::Absent, "json が無い");
        put(&fleet_dir(&state).join(JSON_FILE), "{\"version\":2}\n");
        assert_eq!(read(&state, &repo, &Input::ALL), Lifecycle::Unreadable, "json が読める形でない");
        let marks = written(&repo, &state);
        match read(&state, &repo, &Input::ALL) {
            Lifecycle::Read(found) => {
                assert_eq!((found.marks, found.generated_at.as_str(), found.parts.len(), found.stale), (marks, "2026-10-01T00:00:00Z", 0, Vec::new()));
            }
            other => panic!("読めた周のはず: {other:?}"),
        }
    }

    /// (j) 古さの印の 3 種がそのまま理由の語になる（比べる組が空でも）。
    #[test]
    fn lifecycle_read_names_the_three_stale_marks_as_they_are() {
        let (repo, state) = place("marks");
        let marks = written(&repo, &state);
        mark(&state, MarkKind::MergeGate, Value::Main(SHA.to_owned()));
        assert_eq!(words(read(&state, &repo, &[])), ["merge-gate"], "1 種");
        mark(&state, MarkKind::LedgerGate, Value::Ledger(marks.ledger));
        mark(&state, MarkKind::Unreadable, Value::Reason("ledger".to_owned()));
        assert_eq!(words(read(&state, &repo, &[])), ["ledger-gate", "merge-gate", "unreadable"], "3 種は種類の宣言順");
    }

    /// (k) 比べる組に無い印の違いは理由にしない。比べる組の今の印を読めない種類は理由に入る（main の ref の無い repo）。
    #[test]
    fn lifecycle_read_compares_only_the_asked_marks_and_counts_an_unreadable_current_one() {
        let (repo, state) = place("asked");
        written(&repo, &state);
        put(&repo.join(".beads/issues.jsonl"), "[]\n\n");
        assert_eq!(words(read(&state, &repo, &[Input::Events])), Vec::<&str>::new(), "events だけの組で台帳の違いは古くない");
        assert_eq!(words(read(&state, &repo, &[Input::Ledger])), ["ledger"], "台帳を含む組では違いが理由");
        assert_eq!(words(read(&state, &repo, &[Input::Events, Input::Main])), Vec::<&str>::new(), "main は同じ");
        let _ = std::fs::remove_file(repo.join(".git/refs/remotes/origin/main"));
        assert_eq!(words(read(&state, &repo, &[Input::Events])), Vec::<&str>::new(), "main を比べなければ読めなくても理由にしない");
        assert_eq!(words(read(&state, &repo, &[Input::Events, Input::Main])), ["main"], "main の ref の無い repo は main を含む組で理由に入る");
    }

    /// (l) 読む順は stale → json: stale を読めない置き場は json が在っても読めない（stale が無ければ読める対）。
    #[test]
    fn lifecycle_read_reads_stale_before_json() {
        let (repo, state) = place("order");
        written(&repo, &state);
        assert!(matches!(read(&state, &repo, &Input::ALL), Lifecycle::Read(_)), "stale が無ければ読める");
        put(&fleet_dir(&state).join(STALE_FILE), "not json\n");
        assert_eq!(read(&state, &repo, &Input::ALL), Lifecycle::Unreadable, "stale が読めなければ json が在っても読めない");
    }

    /// (m) 理由の並びは古さの印の種類（宣言順）→ ledger → events → main。
    #[test]
    fn lifecycle_read_orders_the_reasons_marks_first_then_ledger_events_main() {
        let (repo, state) = place("reasons");
        let marks = written(&repo, &state);
        mark(&state, MarkKind::Unreadable, Value::Reason("main".to_owned()));
        mark(&state, MarkKind::LedgerGate, Value::Ledger(marks.ledger));
        put(&repo.join(".git/refs/remotes/origin/main"), &format!("{OTHER}\n"));
        put(&events_path(&state), "{}\n");
        put(&repo.join(".beads/issues.jsonl"), "[]\n\n");
        assert_eq!(words(read(&state, &repo, &Input::ALL)), ["ledger-gate", "unreadable", "ledger", "events", "main"]);
        assert_eq!(words(read(&state, &repo, &[Input::Main, Input::Ledger])), ["ledger-gate", "unreadable", "ledger", "main"], "組の並びでなく固定の順");
    }
}
