//! 案件の局面の語と型（設計 docs/design/case-lifecycle.md §2〜§6・FR90・FR91・ADR-0088）。
//!
//! 語の正本は設計 doc の表で、本 module は字を 1 字も違えずに code の型にする: 部品の種類 9（[`Kind`]）・局面の語 38
//! （[`Phase`]・宣言順＝優先の順・字の列は [`PHASES`]）・手番 6（[`Turn`]）・misfit の理由 15（[`Misfit`]・字の列は
//! [`MISFITS`]）・部品の型（[`Part`]・共通の欄の key の列 [`COMMON_KEYS`] と `links` の key の列 [`LINK_KEYS`]）。語から手番を
//! 返す 1 関数（[`turn_of`]）が §3 の表を網羅の match で持つ。導く関数は純関数で、I/O は書き手が集めて渡す。
//! 便の段や待ちの理由の enum の変種は名指さない（入力の語は字で受け、表の歯が母集団の増減を測る）。

/// 閉じた語の 1 組（変種・宣言順の字の列・全変種の列・字の関数）を 1 か所の宣言から作る（字と変種の順が 1 つの列で決まる）。
macro_rules! words {
    ($(#[$meta:meta])* $name:ident, $list:ident, $count:literal; $($variant:ident => $word:literal,)*) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $(#[doc = $word] $variant,)*
        }

        #[doc = concat!("[`", stringify!($name), "`] の字の列（宣言順）。")]
        pub const $list: [&str; $count] = [$($word,)*];

        impl $name {
            /// 全変種（宣言順）。
            pub const ALL: [Self; $count] = [$(Self::$variant,)*];

            /// 語の字面。
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $word,)*
                }
            }
        }
    };
}

words! {
    /// 局面の語（閉じた 38 語・宣言順＝優先の順・§2 の表と 1 字も違わない）。
    Phase, PHASES, 38;
    UtteranceOpen => "utterance-open",
    UtteranceSorted => "utterance-sorted",
    QuestionOpen => "question-open",
    RulingUnreflected => "ruling-unreflected",
    QuestionClosed => "question-closed",
    MemoPromoting => "memo-promoting",
    MemoAsking => "memo-asking",
    MemoActionable => "memo-actionable",
    MemoWaiting => "memo-waiting",
    MemoClosed => "memo-closed",
    ContractRunning => "contract-running",
    ContractRefused => "contract-refused",
    ContractQueued => "contract-queued",
    ContractClosed => "contract-closed",
    RunIntake => "run-intake",
    RunReviewed => "run-reviewed",
    RunReviewFailed => "run-review-failed",
    RunBlocked => "run-blocked",
    RunImplementing => "run-implementing",
    RunAsking => "run-asking",
    RunRateLimited => "run-rate-limited",
    RunGating => "run-gating",
    RunLanding => "run-landing",
    RunGateFailed => "run-gate-failed",
    RunCiWaiting => "run-ci-waiting",
    RunLandedOpen => "run-landed-open",
    RunStopped => "run-stopped",
    RunFailed => "run-failed",
    RowUnbeaded => "row-unbeaded",
    RowBeaded => "row-beaded",
    RowLanded => "row-landed",
    RequirementUnrowed => "requirement-unrowed",
    RequirementRowed => "requirement-rowed",
    EpicClosable => "epic-closable",
    EpicOpen => "epic-open",
    EpicClosed => "epic-closed",
    CommitLanded => "commit-landed",
    Misfit => "misfit",
}

words! {
    /// misfit の理由（閉じた 15 語・§4 の表の順・行 a1・a2・b1 が出す語も先に全部置く）。
    Misfit, MISFITS, 15;
    NoPhase => "no-phase",
    FormBoth => "form-both",
    FormNeither => "form-neither",
    MemoNoTrigger => "memo-no-trigger",
    CloseKindMismatch => "close-kind-mismatch",
    CloseUnresolved => "close-unresolved",
    MergedIntoNotOpen => "merged-into-not-open",
    PromotedUnmet => "promoted-unmet",
    PromotedListMismatch => "promoted-list-mismatch",
    CloseRulingUnresolved => "close-ruling-unresolved",
    CloseRulingNotBound => "close-ruling-not-bound",
    DeferredNotChildRuling => "deferred-not-child-ruling",
    SourceUnresolved => "source-unresolved",
    RunTrailerUnknown => "run-trailer-unknown",
    CommitNoTrailer => "commit-no-trailer",
}

words! {
    /// 部品の種類（閉じた 9 つ・§2・語は局面の語の接頭辞で、出力の `part` の値）。
    Kind, KINDS, 9;
    Utterance => "utterance",
    Question => "question",
    Memo => "memo",
    Contract => "contract",
    Run => "run",
    Row => "row",
    Requirement => "requirement",
    Epic => "epic",
    Commit => "commit",
}

words! {
    /// 手番（閉じた 6 値・§3・`Nobody` の字は `none`＝誰の手も待たない）。
    Turn, TURNS, 6;
    User => "user",
    Seat => "seat",
    Vessel => "vessel",
    Runner => "runner",
    Ci => "ci",
    Nobody => "none",
}

/// 部品の共通の欄の key（§5.2・全部が必須・書き手はこの列で key を書く）。
pub const COMMON_KEYS: [&str; 9] = ["part", "id", "phase", "turn", "since", "reason", "closed", "overdue", "links"];

/// `links` の key（§5.2・空の key は書き手が省く）。
pub const LINK_KEYS: [&str; 8] = ["source", "questions", "rulings", "promoted", "runs", "commits", "destination", "on"];

/// memo-promoting の理由: 契約が開いている（手番は契約と便の部品が持つ）。
pub const REASON_CONTRACT_OPEN: &str = "contract-open";

/// memo-promoting の理由: FR93 の自動の close を待つ。
pub const REASON_CLOSE_DUE: &str = "close-due";

/// contract-queued の理由から手番の表（§3・11 語）。`WAIT_REASONS` の 8 語と、dispatcher の後の行が足す語
/// unreflected-ruling・floor・reserved を含む。`admission` は contract-refused と同じ手番で持つ（表が `WAIT_REASONS` を全部覆うため）。
pub const QUEUED_TURNS: [(&str, Turn); 11] = [
    ("dependency", Turn::Vessel),
    ("overlap", Turn::Vessel),
    ("admission", Turn::Seat),
    ("host-busy", Turn::Vessel),
    ("hold", Turn::Seat),
    ("launched", Turn::Vessel),
    ("settled", Turn::Nobody),
    ("no-design-pointer", Turn::Seat),
    ("unreflected-ruling", Turn::Seat),
    ("floor", Turn::Seat),
    ("reserved", Turn::Seat),
];

impl Phase {
    /// 字から語（表に無い字は `None`）。
    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|phase| phase.as_str() == word)
    }
}

/// 語から手番（§3 の表・網羅の match）。`reason` は memo-promoting と contract-queued だけが読む。表に無い語・理由は `None`
/// （呼び手は misfit `no-phase` に置く・fail-closed）。
pub fn turn_of(word: &str, reason: Option<&str>) -> Option<Turn> {
    let turn = match Phase::of(word)? {
        Phase::UtteranceSorted
        | Phase::QuestionClosed
        | Phase::MemoClosed
        | Phase::ContractClosed
        | Phase::RowBeaded
        | Phase::RowLanded
        | Phase::RequirementRowed
        | Phase::EpicOpen
        | Phase::EpicClosed
        | Phase::CommitLanded
        | Phase::MemoWaiting
        | Phase::ContractRunning => Turn::Nobody,
        Phase::UtteranceOpen
        | Phase::RulingUnreflected
        | Phase::MemoActionable
        | Phase::ContractRefused
        | Phase::RunAsking
        | Phase::RunReviewFailed
        | Phase::RunGateFailed
        | Phase::RunLandedOpen
        | Phase::RunStopped
        | Phase::RunFailed
        | Phase::RowUnbeaded
        | Phase::EpicClosable
        | Phase::Misfit
        | Phase::RequirementUnrowed => Turn::Seat,
        Phase::QuestionOpen | Phase::MemoAsking | Phase::RunBlocked => Turn::User,
        Phase::RunIntake | Phase::RunReviewed | Phase::RunRateLimited | Phase::RunGating | Phase::RunLanding => Turn::Vessel,
        Phase::RunImplementing => Turn::Runner,
        Phase::RunCiWaiting => Turn::Ci,
        Phase::MemoPromoting => match reason? {
            REASON_CONTRACT_OPEN => Turn::Nobody,
            REASON_CLOSE_DUE => Turn::Vessel,
            _ => return None,
        },
        Phase::ContractQueued => {
            let reason = reason?;
            QUEUED_TURNS.iter().find(|(word, _)| *word == reason)?.1
        }
    };
    Some(turn)
}

words! {
    /// 発話の行き先の種類（`destination` の `to`・字の形から推さない）。
    Sink, SINKS, 3;
    ToMemo => "memo",
    ToRuling => "ruling",
    ToChat => "chat",
}

words! {
    /// 発話の入口（`channel`）。
    Channel, CHANNELS, 2;
    Chat => "chat",
    Gui => "gui",
}

/// 仕分け済みの発話の行き先 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    /// 行き先の種類。
    pub to: Sink,
    /// 行き先の id（無ければ `None`）。
    pub id: Option<String>,
}

/// 部品の結び（`links` の 8 key・結びの先が部品として載っているとは限らない）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Links {
    /// 発端。
    pub source: Vec<String>,
    /// 子の問い。
    pub questions: Vec<String>,
    /// 問いに結んだ裁定 id。
    pub rulings: Vec<String>,
    /// 昇格した契約。
    pub promoted: Vec<String>,
    /// 便。
    pub runs: Vec<String>,
    /// 着地の commit。
    pub commits: Vec<String>,
    /// 仕分け済みの発話の行き先。
    pub destination: Vec<Destination>,
    /// 契約の待ちの理由が dependency か overlap のときの相手の bead id。
    pub on: Vec<String>,
}

/// memo の引き金 1 つの写し（FR87 と同じ判定の写し）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerView {
    /// 引き金の形の語。
    pub form: String,
    /// 値の字。
    pub value: String,
    /// 満ちたか。
    pub met: bool,
}

/// 種類ごとの欄（§5.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extra {
    /// 種類ごとの欄を持たない種類。
    None,
    /// 発話: `session`（字か null）・`channel`。
    Utterance {
        /// セッションの字。
        session: Option<String>,
        /// 入口。
        channel: Channel,
    },
    /// memo: `due`（満ちていない期日の最も早い値）・`triggers`（任意）・`keep`（任意・keep の記帳が在るか）。
    Memo {
        /// 満ちていない期日の最も早い値。
        due: Option<String>,
        /// 引き金の写し。
        triggers: Option<Vec<TriggerView>>,
        /// keep の記帳が在るか。
        keep: Option<bool>,
    },
    /// 契約: `pointer`（任意・設計 pointer の字か null）。
    Contract {
        /// 設計 pointer の字。
        pointer: Option<String>,
    },
    /// 便: `bead`（便が属する契約の bead id）。
    Run {
        /// 契約の bead id。
        bead: String,
    },
}

/// 案件の部品 1 つ（共通の欄 9 つと種類ごとの欄）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// 種類。
    pub part: Kind,
    /// id（種類ごとの形は §5.2）。
    pub id: String,
    /// 局面の語。
    pub phase: Phase,
    /// 手番。
    pub turn: Turn,
    /// その局面に入った時刻（UTC の秒まで・導けなければ `None`）。
    pub since: Option<String>,
    /// 理由の語。
    pub reason: Option<String>,
    /// 閉じたか。
    pub closed: bool,
    /// 手番が seat で閾値を越えたか（行が無いか since が無ければ `None`）。
    pub overdue: Option<bool>,
    /// 結び。
    pub links: Links,
    /// 種類ごとの欄。
    pub extra: Extra,
}

#[cfg(test)]
mod tests {
    use super::{turn_of, Kind, Misfit, Phase, Turn, COMMON_KEYS, LINK_KEYS, MISFITS, PHASES, QUEUED_TURNS};
    use crate::pipe::dispatch::WAIT_REASONS;

    /// §2 の表の字と宣言順。
    const EXPECTED: [&str; 38] = [
        "utterance-open",
        "utterance-sorted",
        "question-open",
        "ruling-unreflected",
        "question-closed",
        "memo-promoting",
        "memo-asking",
        "memo-actionable",
        "memo-waiting",
        "memo-closed",
        "contract-running",
        "contract-refused",
        "contract-queued",
        "contract-closed",
        "run-intake",
        "run-reviewed",
        "run-review-failed",
        "run-blocked",
        "run-implementing",
        "run-asking",
        "run-rate-limited",
        "run-gating",
        "run-landing",
        "run-gate-failed",
        "run-ci-waiting",
        "run-landed-open",
        "run-stopped",
        "run-failed",
        "row-unbeaded",
        "row-beaded",
        "row-landed",
        "requirement-unrowed",
        "requirement-rowed",
        "epic-closable",
        "epic-open",
        "epic-closed",
        "commit-landed",
        "misfit",
    ];

    /// §3 の表（理由を読まない 36 語）。
    const TURN_TABLE: [(&str, Turn); 36] = {
        use Turn::{Ci, Nobody, Runner, Seat, User, Vessel};
        [
            ("utterance-open", Seat),
            ("utterance-sorted", Nobody),
            ("question-open", User),
            ("ruling-unreflected", Seat),
            ("question-closed", Nobody),
            ("memo-asking", User),
            ("memo-actionable", Seat),
            ("memo-waiting", Nobody),
            ("memo-closed", Nobody),
            ("contract-running", Nobody),
            ("contract-refused", Seat),
            ("contract-closed", Nobody),
            ("run-intake", Vessel),
            ("run-reviewed", Vessel),
            ("run-review-failed", Seat),
            ("run-blocked", User),
            ("run-implementing", Runner),
            ("run-asking", Seat),
            ("run-rate-limited", Vessel),
            ("run-gating", Vessel),
            ("run-landing", Vessel),
            ("run-gate-failed", Seat),
            ("run-ci-waiting", Ci),
            ("run-landed-open", Seat),
            ("run-stopped", Seat),
            ("run-failed", Seat),
            ("row-unbeaded", Seat),
            ("row-beaded", Nobody),
            ("row-landed", Nobody),
            ("requirement-unrowed", Seat),
            ("requirement-rowed", Nobody),
            ("epic-closable", Seat),
            ("epic-open", Nobody),
            ("epic-closed", Nobody),
            ("commit-landed", Nobody),
            ("misfit", Seat),
        ]
    };

    /// §3 の contract-queued の理由の表（11 語）。
    const QUEUED_TABLE: [(&str, Turn); 11] = {
        use Turn::{Nobody, Seat, Vessel};
        [
            ("dependency", Vessel),
            ("overlap", Vessel),
            ("host-busy", Vessel),
            ("launched", Vessel),
            ("hold", Seat),
            ("no-design-pointer", Seat),
            ("unreflected-ruling", Seat),
            ("floor", Seat),
            ("reserved", Seat),
            ("settled", Nobody),
            ("admission", Seat),
        ]
    };

    /// (a) 38 語は ASCII の小文字と `-`・一意。種類の名を頭に持たない 2 語（`ruling-unreflected`・`misfit`）を除く 36 語は
    /// `<種類>-` を頭に持ち、種類ごとの語の数は 2・2・5・4・14・3・2・3・1。
    #[test]
    fn phase_table_words_are_lowercase_unique_and_prefixed_by_kind() {
        let mut seen = std::collections::BTreeSet::new();
        for word in PHASES {
            assert!(word.chars().all(|found| found.is_ascii_lowercase() || found == '-'), "{word}");
            assert!(seen.insert(word), "重複 {word}");
        }
        assert_eq!(seen.len(), 38);
        let prefixed: Vec<&str> = PHASES.into_iter().filter(|word| !["ruling-unreflected", "misfit"].contains(word)).collect();
        assert_eq!(prefixed.len(), 36);
        let counts = Kind::ALL.map(|kind| {
            let head = format!("{}-", kind.as_str());
            prefixed.iter().filter(|word| word.starts_with(&head)).count()
        });
        assert_eq!(counts, [2, 2, 5, 4, 14, 3, 2, 3, 1]);
        assert_eq!(counts.iter().sum::<usize>(), 36, "どの語も 1 つの種類の頭だけを持つ");
    }

    /// (b) 38 語の列は §2 の表の字と宣言順に 1 字も違わず、型の変種の順も同じ。
    #[test]
    fn phase_table_words_match_the_design_table_in_order() {
        assert_eq!(PHASES, EXPECTED);
        assert_eq!(Phase::ALL.map(Phase::as_str), EXPECTED);
        assert_eq!(Phase::of("run-gating"), Some(Phase::RunGating));
        assert_eq!(Phase::of("misfit"), Some(Phase::Misfit));
    }

    /// (c) 手番の 6 語。
    #[test]
    fn phase_table_turns_are_the_six_words() {
        assert_eq!(Turn::ALL.map(Turn::as_str), ["user", "seat", "vessel", "runner", "ci", "none"]);
    }

    /// (d) contract-queued の理由の表は `WAIT_REASONS` を全部と unreflected-ruling・floor を含む 11 語で、語は一意。
    #[test]
    fn phase_table_queued_reasons_cover_the_wait_reasons() {
        let words: Vec<&str> = QUEUED_TURNS.iter().map(|(word, _)| *word).collect();
        for reason in WAIT_REASONS {
            assert!(words.contains(reason), "{reason}");
        }
        assert!(words.contains(&"unreflected-ruling") && words.contains(&"floor") && words.contains(&"reserved"));
        assert_eq!(words.len(), 11);
        let unique: std::collections::BTreeSet<&str> = words.iter().copied().collect();
        assert_eq!(unique.len(), 11);
    }

    /// (e) 表に無い語（語・理由）は `None`。
    #[test]
    fn phase_table_unknown_words_have_no_turn() {
        assert_eq!(turn_of("no-such-phase", None), None);
        assert_eq!(turn_of("", None), None);
        assert_eq!(turn_of("Run-Gating", None), None);
        assert_eq!(turn_of("contract-queued", Some("no-such-reason")), None);
        assert_eq!(turn_of("contract-queued", None), None);
        assert_eq!(turn_of("memo-promoting", Some("no-such-reason")), None);
        assert_eq!(turn_of("memo-promoting", None), None);
        assert_eq!(turn_of("run-gating", None), Some(Turn::Vessel), "理由を読まない語は理由が無くても引ける");
    }

    /// (f) misfit の 15 語の字。
    #[test]
    fn phase_table_misfit_words_are_the_fifteen() {
        let expected = [
            "no-phase",
            "form-both",
            "form-neither",
            "memo-no-trigger",
            "close-kind-mismatch",
            "close-unresolved",
            "merged-into-not-open",
            "promoted-unmet",
            "promoted-list-mismatch",
            "close-ruling-unresolved",
            "close-ruling-not-bound",
            "deferred-not-child-ruling",
            "source-unresolved",
            "run-trailer-unknown",
            "commit-no-trailer",
        ];
        assert_eq!(MISFITS, expected);
        assert_eq!(Misfit::ALL.map(Misfit::as_str), expected);
    }

    /// (g) 共通の欄の 9 key と `links` の 8 key の列（§5.2 の字と順）。
    #[test]
    fn phase_table_key_lists_match_the_output_spec() {
        assert_eq!(COMMON_KEYS, ["part", "id", "phase", "turn", "since", "reason", "closed", "overdue", "links"]);
        assert_eq!(LINK_KEYS, ["source", "questions", "rulings", "promoted", "runs", "commits", "destination", "on"]);
    }

    /// (h) 9 つの種類の語の字と順。
    #[test]
    fn phase_table_kinds_are_the_nine_words() {
        let words = Kind::ALL.map(Kind::as_str);
        assert_eq!(words, ["utterance", "question", "memo", "contract", "run", "row", "requirement", "epic", "commit"]);
    }

    /// (i) 語から手番の関数は §3 の表の全行と一致する（38 語・memo-promoting の理由 2 つ・contract-queued の理由 11 語）。
    #[test]
    fn phase_table_turn_of_matches_every_row_of_the_table() {
        for (word, turn) in TURN_TABLE {
            assert_eq!(turn_of(word, None), Some(turn), "{word}");
            assert_eq!(turn_of(word, Some("anything")), Some(turn), "{word}: 理由を読まない語");
        }
        assert_eq!(turn_of("memo-promoting", Some("contract-open")), Some(Turn::Nobody));
        assert_eq!(turn_of("memo-promoting", Some("close-due")), Some(Turn::Vessel));
        for (reason, turn) in QUEUED_TABLE {
            assert_eq!(turn_of("contract-queued", Some(reason)), Some(turn), "{reason}");
        }
        let mut covered: Vec<&str> = TURN_TABLE.iter().map(|(word, _)| *word).chain(["memo-promoting", "contract-queued"]).collect();
        let mut all = EXPECTED.to_vec();
        covered.sort_unstable();
        all.sort_unstable();
        assert_eq!(covered, all, "表の 36 行 + 理由で分かれる 2 語 = 38 語");
        assert_eq!(QUEUED_TURNS.len(), QUEUED_TABLE.len());
    }
}
