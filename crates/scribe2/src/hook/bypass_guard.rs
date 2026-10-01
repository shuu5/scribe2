//! 席の道具の呼び出しの 3 形を断る 1 つの門（設計 docs/design/limit-permit.md §17・SRS FR112 / AC86 / FR45・NFR5）。
//!
//! 上限の許可は記帳された user の言葉を根拠にする。その根拠を席が普通の道具で作れないよう、(a) hook の subcommand を自分で撃つ形
//! （`hook-subcommand`）、(b) 置き場の event log へ直に書く形（`event-log-write`）、(c) `pipe gate` / `land` / `resume` へ写しの設定
//! `--rules` を渡す形（`rules-swap`）を実行の前に止める。位置は権能の guard の断らなかった周の後ろ・走っている便の行の門の前
//! （権能の guard が断る呼び出しにはその断りを出す・FR112）。
//!
//! 入力は tool 名・Bash の command・編集系の path・payload の cwd・hook の置き場だけで、台帳も rules も event log も読まず、子 process を
//! 撃たない。判定は**通す値を持たない**閉じた 2 値（断る・関係ない）。席が起こした子 process（script・python の書き）は門の外。

use super::command::BASH;
use super::guard::GUARDED;
use super::host_guard::{lands_on, own_targets};
use super::ledger_guard::{is_assignment, segments};
use super::EVENTS;
use crate::fleet::store::events_path;
use crate::name::NAME;
use crate::polarity::{OnFailure, Polarity, Timing};
use std::path::Path;

/// 記録の `what` の頭（極性一覧の語と同じ・後ろに理由の語が付く）。
pub const WHAT: &str = "bypass-deny";

/// この境界の極性: 道具の呼び出しの時点で止め、通す値を持たない（止める側へしか倒れない）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 断る理由の語（閉じた 3 つ・宣言順が当たりの順＝形の宣言順が segment の順に勝つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// hook の subcommand（`hook <event>`）を撃つ形。
    HookSubcommand,
    /// 置き場の event log への書き。
    EventLogWrite,
    /// `pipe gate` / `land` / `resume` への `--rules`。
    RulesSwap,
}

/// [`Reason`] の全 variant（宣言順）。
pub const REASONS: [Reason; 3] = [Reason::HookSubcommand, Reason::EventLogWrite, Reason::RulesSwap];

impl Reason {
    /// 断りの行と記録に出す理由の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HookSubcommand => "hook-subcommand",
            Self::EventLogWrite => "event-log-write",
            Self::RulesSwap => "rules-swap",
        }
    }

    /// 次の一手（1 語ごとに 1 文）。
    fn route(self) -> String {
        match self {
            Self::HookSubcommand => format!("user の発話は prompt の入口で hook が記帳する・問いへは `{NAME} seat ruling bind` で結ぶ"),
            Self::EventLogWrite => format!("event log は器の口（`{NAME}` の seat・pipe・fleet の subcommand）だけが書く"),
            Self::RulesSwap => "--rules を外して撃つ（埋め込みの manifest を読む）".to_owned(),
        }
    }
}

/// 判定。**通す値の variant を持たない**閉じた 2 値（他の呼び出しは「関係ない」で、後ろの門へ渡す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BypassDecision {
    /// 断る。
    Deny {
        /// 当たった理由（記録の `what` の後ろの語）。
        reason: Reason,
        /// stderr の 1 行。
        line: String,
    },
    /// この門と関係ない呼び出し。
    Unrelated,
}

/// 判定の場（hook が解いた入力だけ・読むのは字と path の関係だけ）。
pub struct Scene<'a> {
    /// tool 名。
    pub tool: &'a str,
    /// Bash の command 行。
    pub command: Option<&'a str>,
    /// 編集系の道具の編集先の path。
    pub path: Option<&'a str>,
    /// payload の作業 dir（相対 path の基準）。
    pub cwd: &'a Path,
    /// hook の置き場（event log の在る場所）。
    pub state_dir: &'a Path,
}

/// 語の前後から外す括りの字（`$(… stop)` や backtick の中の並びも直の並びに数える）。
const BRACKETS: [char; 5] = ['(', ')', '`', '{', '}'];
/// 頭の語が shell か eval のとき、後ろの語を同じ分け方で分け直す対象（閉じた 5 語）。
const WRAPPERS: [&str; 5] = ["sh", "bash", "zsh", "dash", "eval"];
/// hook の subcommand の名。
const HOOK: &str = "hook";
/// 器の pipe の口の名。
const PIPE: &str = "pipe";
/// `--rules` を断る pipe の 2 語目（完全一致）。
const RULES_VERBS: [&str; 3] = ["gate", "land", "resume"];
/// 写しの設定を渡す flag。
const FLAG_RULES: &str = "--rules";

/// 判定する。Bash の周だけ command を segment に分け（shell の包みの中も足す）、形の宣言順に全 segment を照らして最初の 1 つで断る。
pub fn decide(scene: &Scene) -> BypassDecision {
    let read = if scene.tool == BASH { widened(scene.command.unwrap_or_default()) } else { Vec::new() };
    let found = REASONS.iter().find_map(|reason| hit_of(*reason, scene, &read).map(|hit| (*reason, hit)));
    match found {
        Some((reason, hit)) => BypassDecision::Deny { reason, line: deny_line(reason, &hit) },
        None => BypassDecision::Unrelated,
    }
}

/// 断りの 1 行（当たった字と FR112 と次の一手）。
fn deny_line(reason: Reason, hit: &str) -> String {
    format!(
        "{NAME}: deny bypass reason={}（hit={hit}・席の道具の呼び出しでは撃てない形・FR112・limit-permit.md §17）。{}",
        reason.as_str(),
        reason.route()
    )
}

/// 1 つの形が当たった字（当たらなければ `None`）。
fn hit_of(reason: Reason, scene: &Scene, read: &[Vec<String>]) -> Option<String> {
    match reason {
        Reason::HookSubcommand => read.iter().find_map(|segment| hook_in(segment)),
        Reason::EventLogWrite => log_in(scene, read),
        Reason::RulesSwap => read.iter().find_map(|segment| rules_in(segment)),
    }
}

/// 起票の門と同じ分け方の segment に、shell の包みの後ろの語を同じ分け方で分けた segment を足した列（足した segment にも繰り返す）。
fn widened(command: &str) -> Vec<Vec<String>> {
    let mut all = Vec::new();
    for segment in segments(command) {
        widen(segment, &mut all);
    }
    all
}

/// segment を足し、頭の語（前の代入を除く）の basename が [`WRAPPERS`] なら、後ろの `-` で始まらない語を分け直して続ける。
fn widen(segment: Vec<String>, all: &mut Vec<Vec<String>>) {
    let head = segment.iter().position(|word| !is_assignment(word));
    let wrapped = head.and_then(|at| segment.get(at)).is_some_and(|word| WRAPPERS.contains(&word.rsplit('/').next().unwrap_or(word)));
    let inner: Vec<Vec<String>> = match head.filter(|_| wrapped) {
        Some(at) => segment.iter().skip(at.saturating_add(1)).filter(|word| !word.starts_with('-')).flat_map(|word| segments(word)).collect(),
        None => Vec::new(),
    };
    all.push(segment);
    for next in inner {
        widen(next, all);
    }
}

/// segment の連続した 2 語が `hook` と event の 6 語の 1 つ（引用の中の字は 1 語に閉じているので並びにならない）。
fn hook_in(segment: &[String]) -> Option<String> {
    segment.windows(2).find_map(|pair| match pair {
        [first, second] => {
            let (first, second) = (first.trim_matches(BRACKETS), second.trim_matches(BRACKETS));
            (first == HOOK && EVENTS.contains(&second)).then(|| format!("{HOOK} {second}"))
        }
        _ => None,
    })
}

/// 編集系の道具の path か、Bash の書き込みの向け先（host の見張りの読み手）が置き場の event log に当たる最初の 1 つ。
fn log_in(scene: &Scene, read: &[Vec<String>]) -> Option<String> {
    let log = events_path(scene.state_dir);
    if GUARDED.contains(&scene.tool) {
        let path = scene.path?;
        return lands_on(&log, path, false, scene.cwd).then(|| path.to_owned());
    }
    own_targets(read, scene.cwd).into_iter().find(|(word, after_cd)| lands_on(&log, word, *after_cd, scene.cwd)).map(|(word, _)| word)
}

/// segment の連続した 2 語が `pipe` と gate / land / resume で、同じ segment に `--rules` か `--rules=…` の語が在る。
fn rules_in(segment: &[String]) -> Option<String> {
    let words: Vec<&str> = segment.iter().map(|word| word.trim_matches(BRACKETS)).collect();
    let verb = words.windows(2).find_map(|pair| match pair {
        [first, second] => (*first == PIPE && RULES_VERBS.contains(second)).then_some(*second),
        _ => None,
    })?;
    words
        .iter()
        .any(|word| *word == FLAG_RULES || word.starts_with("--rules="))
        .then(|| format!("{PIPE} {verb} {FLAG_RULES}"))
}
