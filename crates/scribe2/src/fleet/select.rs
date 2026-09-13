//! 口座の選定（設計 docs/design/account-autonomy.md §3・ADR-0020 §2.2・FR36）。
//!
//! 入力は値だけ（口座 label の列・replay の `allowance`・用途・model・除外集合・R-C9-1 の値・`now`）で、
//! I/O も env も持たない（C2.2・C10: 実測行を通してだけ選ぶ）。**選定はこの 1 関数**（[`select`]）が
//! 持ち、便の再開と席の立て直しが同じものを呼ぶ（C2）。候補なしは断りではなく typed な理由
//! （[`NoCandidate`]・[`NoCandidateReason::POLARITY`] = FailOpen）。

use super::{Allowance, AllowanceKey, AllowanceLatest, Measured, WindowKind};
use crate::polarity::{OnFailure, Polarity, Timing};
use std::collections::{BTreeMap, BTreeSet};

/// 当たっている（窓の全量に達した）使用率。**規則値ではない**（設計 §3）。
pub const LIMIT_PCT: u64 = 100;

/// 選定の用途。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// 便用: 当たっていない口座のうち逼迫度が最大（使い切る側・C9.2）。閾値を持たない。
    Run,
    /// session 用: 逼迫度が最小かつ R-C9-1 の値未満（余裕を残す側）。
    Session,
}

/// [`Purpose`] の全 variant。
pub const PURPOSES: &[Purpose] = &[
    Purpose::Run,
    Purpose::Session,
];

impl Purpose {
    /// `--purpose` と出力行の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Session => "session",
        }
    }

    /// 字面から引く。未知なら `None`。
    pub fn parse(text: &str) -> Option<Self> {
        PURPOSES.iter().copied().find(|found| found.as_str() == text)
    }
}

/// 候補なしの理由。**複数が当てはまる周は宣言順で前の variant が勝つ**（1 つに畳む）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NoCandidateReason {
    /// session 用の閾値（R-C9-1）以上で、当たってはいない口座が在る。
    OverThreshold,
    /// 当たっている口座が在る（測れて除外されていない口座が全部当たっている）。
    AllLimited,
    /// 測れない口座（実測行なし・最新が Unmeasured・reset を過ぎた古い行だけ）。口座 0 もここ。
    Unmeasured,
    /// 除外集合に在る（席の登録 row が持つ口座）。
    Excluded,
}

/// [`NoCandidateReason`] の全 variant（宣言順＝優先順）。
pub const NO_CANDIDATE_REASONS: &[NoCandidateReason] = &[
    NoCandidateReason::OverThreshold,
    NoCandidateReason::AllLimited,
    NoCandidateReason::Unmeasured,
    NoCandidateReason::Excluded,
];

impl NoCandidateReason {
    /// この境界の極性（設計 §6）: 候補なしは待つか記帳するだけで止めない。**Guard ではない**
    /// （極性一覧には載らない）。
    pub const POLARITY: Polarity = Polarity {
        timing: Timing::InLoop,
        on_failure: OnFailure::FailOpen,
    };

    /// 出力行の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OverThreshold => "over-threshold",
            Self::AllLimited => "all-limited",
            Self::Unmeasured => "unmeasured",
            Self::Excluded => "excluded",
        }
    }
}

/// 候補なしの周の中身。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoCandidate {
    /// 1 つに畳んだ理由。
    pub reason: NoCandidateReason,
    /// 当たっている口座が開き直る時刻の最も早いもの（当たっている口座が無ければ `None`）。
    pub earliest_reset: Option<String>,
}

/// 選定の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// 選んだ口座の label。
    Chosen(String),
    /// 候補が無い。
    None(NoCandidate),
}

/// 選定の入力（値だけ）。
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// 口座 label の列（manifest の `[[account]]`・宣言値）。
    pub labels: &'a [String],
    /// 口座 × 窓 × model の最新の行（replay の産物・実測値）。
    pub allowance: &'a BTreeMap<AllowanceKey, AllowanceLatest>,
    /// 用途。
    pub purpose: Purpose,
    /// 使う model の display name（与えられた周はモデル別窓のうちその model の行だけを数える）。
    pub model: Option<&'a str>,
    /// 候補から外す label。
    pub exclude: &'a BTreeSet<String>,
    /// R-C9-1 の値（session 用の閾値・使用率の百分率・未満なら候補）。
    pub threshold_pct: u64,
    /// いまの UTC（`YYYY-MM-DDTHH:MM:SSZ`）。reset を過ぎた行を古いと読むのに使う。
    pub now: &'a str,
}

/// 口座 1 つの見立て。
enum Standing {
    /// 候補（逼迫度つき）。
    Candidate(u64),
    /// 候補から外れた（理由と、当たっている周はその口座が開き直る時刻）。
    Out(NoCandidateReason, Option<String>),
}

/// 口座を 1 つ選ぶ。同点は label の辞書順で先の口座。
pub fn select(input: &Input<'_>) -> Selection {
    let mut candidates: Vec<(u64, &str)> = Vec::new();
    let mut outs: Vec<(NoCandidateReason, Option<String>)> = Vec::new();
    for label in input.labels {
        match standing(input, label) {
            Standing::Candidate(pressure) => candidates.push((pressure, label.as_str())),
            Standing::Out(reason, reopens) => outs.push((reason, reopens)),
        }
    }
    match pick(input.purpose, &candidates) {
        Some(label) => Selection::Chosen(label.to_owned()),
        None => Selection::None(NoCandidate {
            reason: outs
                .iter()
                .map(|(reason, _)| *reason)
                .min()
                .unwrap_or(NoCandidateReason::Unmeasured),
            earliest_reset: outs.into_iter().filter_map(|(_, reopens)| reopens).min(),
        }),
    }
}

/// `fleet select` の stdout 1 行。
pub fn line(purpose: Purpose, selection: &Selection) -> String {
    match selection {
        Selection::Chosen(label) => format!("select purpose={} chosen={label}", purpose.as_str()),
        Selection::None(found) => format!(
            "select purpose={} none={} earliest_reset={}",
            purpose.as_str(),
            found.reason.as_str(),
            found.earliest_reset.as_deref().unwrap_or("-")
        ),
    }
}

/// 候補の中から用途の規則で 1 つ選ぶ。
fn pick<'a>(purpose: Purpose, candidates: &[(u64, &'a str)]) -> Option<&'a str> {
    let ranked = candidates.iter().copied();
    let found = match purpose {
        Purpose::Run => ranked.min_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1))),
        Purpose::Session => ranked.min_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1))),
    };
    found.map(|(_, label)| label)
}

/// 口座 1 つを候補か、外れた理由かに分ける（除外 → 測れない → 当たっている → 閾値の順に見る）。
fn standing(input: &Input<'_>, label: &str) -> Standing {
    if input.exclude.contains(label) {
        return Standing::Out(NoCandidateReason::Excluded, None);
    }
    let Some((pressure, reopens)) = reading(input, label) else {
        return Standing::Out(NoCandidateReason::Unmeasured, None);
    };
    if pressure >= LIMIT_PCT {
        return Standing::Out(NoCandidateReason::AllLimited, reopens);
    }
    if input.purpose == Purpose::Session && pressure >= input.threshold_pct {
        return Standing::Out(NoCandidateReason::OverThreshold, None);
    }
    Standing::Candidate(pressure)
}

/// 口座の（逼迫度・開き直る時刻）。逼迫度 = 数える窓のうち最大の使用率。開き直る時刻は当たっている
/// 窓の reset の**遅い方**（全部の窓が開くまで当たったまま）。測れない口座は `None`。
/// reset 無しの行は開き直る時刻の導出に入らない（待つ対象ではない・ADR-0024 §2.2）。
fn reading(input: &Input<'_>, label: &str) -> Option<(u64, Option<String>)> {
    let windows = fresh_windows(input, label)?;
    let pressure = windows.iter().map(|found| found.used_pct).max()?;
    let reopens = windows
        .iter()
        .filter(|found| found.used_pct >= LIMIT_PCT)
        .filter_map(|found| found.resets_at.clone())
        .max();
    Some((pressure, reopens))
}

/// 口座の最新の回のうち、数える窓の古くない実測。数える窓に Unmeasured が在る・古くない実測が
/// 1 つも無い周は `None`（測れない口座を選ばない・C10）。reset 無しの実測は古くない実測として
/// 数える（古さの判定は reset 時刻を持つ行にだけ掛かる・ADR-0024 §2.2）。
fn fresh_windows<'a>(input: &Input<'a>, label: &str) -> Option<Vec<&'a Measured>> {
    let mut fresh = Vec::new();
    for row in latest_round(input.allowance, label) {
        match row {
            Allowance::Unmeasured(found) => {
                if counts(input.model, found.window, found.model.as_deref()) {
                    return None;
                }
            }
            Allowance::Measured(found) => {
                let counted = counts(input.model, Some(found.window), found.model.as_deref());
                let not_stale = found.resets_at.as_deref().is_none_or(|resets_at| resets_at >= input.now);
                if counted && not_stale {
                    fresh.push(found);
                }
            }
        }
    }
    (!fresh.is_empty()).then_some(fresh)
}

/// 口座の最新の回（`ts` が最大の行の集まり）。前の回にしか無い窓の行は最新と読まない。
fn latest_round<'a>(
    allowance: &'a BTreeMap<AllowanceKey, AllowanceLatest>,
    label: &str,
) -> Vec<&'a Allowance> {
    let mine: Vec<&'a AllowanceLatest> = allowance
        .iter()
        .filter(|(key, _)| key.account == label)
        .map(|(_, latest)| latest)
        .collect();
    let Some(newest) = mine.iter().copied().map(|latest| latest.ts.as_str()).max() else {
        return Vec::new();
    };
    mine.into_iter()
        .filter(|latest| latest.ts == newest)
        .map(|latest| &latest.allowance)
        .collect()
}

/// その行を逼迫度に数えるか。model が与えられた周のモデル別窓はその model の行だけを数える
/// （model の分からない行は保守側で数える）。
fn counts(model: Option<&str>, window: Option<WindowKind>, row_model: Option<&str>) -> bool {
    match (window, model, row_model) {
        (Some(WindowKind::SevenDayModel), Some(want), Some(found)) => want == found,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        line, select, Input, NoCandidate, NoCandidateReason, Purpose, Selection, LIMIT_PCT,
        NO_CANDIDATE_REASONS, PURPOSES,
    };
    use crate::fleet::{
        Allowance, AllowanceKey, AllowanceLatest, Measured, Unmeasured, UnmeasuredReason,
        WindowKind,
    };
    use crate::order::is_declaration_order;
    use crate::polarity::OnFailure;
    use proptest::prelude::*;
    use proptest::test_runner::Config;
    use std::collections::{BTreeMap, BTreeSet};

    /// 選定の「いま」。
    const NOW: &str = "2026-09-13T06:00:00Z";
    /// 計測の回の時刻。
    const TS: &str = "2026-09-13T05:59:00Z";
    /// 次の回の時刻。
    const LATER_TS: &str = "2026-09-13T05:59:30Z";
    /// 5 時間窓の reset（`NOW` より後）。
    const FIVE_RESET: &str = "2026-09-13T09:00:00Z";
    /// 7 日窓の reset（`NOW` より後）。
    const WEEK_RESET: &str = "2026-09-18T00:00:00Z";
    /// `NOW` より前の reset（古い行）。
    const PAST_RESET: &str = "2026-09-13T05:00:00Z";
    /// R-C9-1 の値（裁定 id `user 2026-09-13T03:14Z` と同じ数）。
    const THRESHOLD: u64 = 85;

    /// 実測 1 行。
    fn measured(account: &str, window: WindowKind, model: Option<&str>, used_pct: u64, resets_at: &str) -> Allowance {
        Allowance::Measured(Measured {
            account: account.to_owned(),
            window,
            model: model.map(str::to_owned),
            endpoint: "oauth-usage".to_owned(),
            used_pct,
            resets_at: Some(resets_at.to_owned()),
        })
    }

    /// 消費の無い窓の実測 1 行（0%・reset 無し）。
    fn idle(account: &str, window: WindowKind) -> Allowance {
        Allowance::Measured(Measured {
            account: account.to_owned(),
            window,
            model: None,
            endpoint: "oauth-usage".to_owned(),
            used_pct: 0,
            resets_at: None,
        })
    }

    /// 測れなかった 1 行。
    fn unmeasured(account: &str, window: Option<WindowKind>) -> Allowance {
        Allowance::Unmeasured(Unmeasured {
            account: account.to_owned(),
            window,
            model: None,
            endpoint: "oauth-usage".to_owned(),
            reason: UnmeasuredReason::Timeout,
        })
    }

    /// 5 時間窓と 7 日窓の 1 回分（reset はどちらも `NOW` より後）。
    fn round(account: &str, five: u64, seven: u64) -> Vec<Allowance> {
        vec![
            measured(account, WindowKind::FiveHour, None, five, FIVE_RESET),
            measured(account, WindowKind::SevenDay, None, seven, WEEK_RESET),
        ]
    }

    /// 回の列を物理順に replay したのと同じ表（同じ key は後が勝つ）。
    fn table(rounds: &[(&str, Vec<Allowance>)]) -> BTreeMap<AllowanceKey, AllowanceLatest> {
        let mut found = BTreeMap::new();
        for (ts, rows) in rounds {
            for row in rows {
                found.insert(row.key(), AllowanceLatest { ts: (*ts).to_owned(), allowance: row.clone() });
            }
        }
        found
    }

    /// 選定の条件。
    struct Ask<'a> {
        purpose: Purpose,
        model: Option<&'a str>,
        exclude: &'a [&'a str],
        threshold_pct: u64,
    }

    fn run() -> Ask<'static> {
        Ask { purpose: Purpose::Run, model: None, exclude: &[], threshold_pct: THRESHOLD }
    }

    fn session() -> Ask<'static> {
        Ask { purpose: Purpose::Session, ..run() }
    }

    fn choose(labels: &[&str], allowance: &BTreeMap<AllowanceKey, AllowanceLatest>, ask: &Ask<'_>) -> Selection {
        let labels: Vec<String> = labels.iter().map(|label| (*label).to_owned()).collect();
        let exclude: BTreeSet<String> = ask.exclude.iter().map(|label| (*label).to_owned()).collect();
        select(&Input {
            labels: &labels,
            allowance,
            purpose: ask.purpose,
            model: ask.model,
            exclude: &exclude,
            threshold_pct: ask.threshold_pct,
            now: NOW,
        })
    }

    fn chosen(label: &str) -> Selection {
        Selection::Chosen(label.to_owned())
    }

    fn none(reason: NoCandidateReason, earliest_reset: Option<&str>) -> Selection {
        Selection::None(NoCandidate { reason, earliest_reset: earliest_reset.map(str::to_owned) })
    }

    const THREE: &[&str] = &["a1", "a2", "a3"];

    /// a1 = 30（5h）・a2 = 70（7d）・a3 = 100（5h・当たっている）。
    fn three() -> BTreeMap<AllowanceKey, AllowanceLatest> {
        table(&[(TS, [round("a1", 30, 10), round("a2", 20, 70), round("a3", 100, 5)].concat())])
    }

    #[test]
    fn select_run_picks_the_most_pressed_unlimited_account() {
        assert_eq!(choose(THREE, &three(), &run()), chosen("a2"), "逼迫度は窓の最大（a2 = 7d の 70）・a3 は当たっている");
    }

    #[test]
    fn select_excludes_the_seat_accounts() {
        assert_eq!(choose(THREE, &three(), &Ask { exclude: &["a2"], ..run() }), chosen("a1"));
        assert_eq!(choose(THREE, &three(), &Ask { exclude: &["a1"], ..session() }), chosen("a2"), "session 用も同じ除外");
        assert_eq!(
            choose(THREE, &three(), &Ask { exclude: &["a2", "a1"], ..run() }),
            none(NoCandidateReason::AllLimited, Some(FIVE_RESET)),
            "除外の残りは当たっている口座だけ"
        );
        assert_eq!(
            choose(&["a1", "a2"], &three(), &Ask { exclude: &["a1", "a2"], ..run() }),
            none(NoCandidateReason::Excluded, None),
            "除外で空"
        );
    }

    #[test]
    fn select_session_picks_the_least_pressed_under_threshold() {
        assert_eq!(choose(THREE, &three(), &session()), chosen("a1"));
        let spread = table(&[(TS, [round("a1", 60, 0), round("a2", 5, 40), round("a3", 90, 0)].concat())]);
        assert_eq!(choose(THREE, &spread, &session()), chosen("a2"), "最小（a2 = 40）");
        assert_eq!(choose(THREE, &spread, &run()), chosen("a3"), "同じ表で便用は最大（閾値を持たない）");
    }

    #[test]
    fn select_threshold_is_exclusive() {
        let edge = table(&[(TS, [round("a1", THRESHOLD, 0), round("a2", THRESHOLD - 1, 0)].concat())]);
        assert_eq!(
            choose(&["a1", "a2"], &edge, &Ask { exclude: &["a2"], ..session() }),
            none(NoCandidateReason::OverThreshold, None),
            "閾値ちょうどは候補外"
        );
        assert_eq!(choose(&["a1", "a2"], &edge, &session()), chosen("a2"), "閾値未満は候補");
        assert_eq!(choose(&["a1"], &edge, &run()), chosen("a1"), "便用は閾値を持たない");
    }

    #[test]
    fn select_never_picks_a_limited_account() {
        let full = table(&[(TS, [round("a1", 100, 0), round("a2", 0, 130), round("a3", 0, 99)].concat())]);
        assert_eq!(choose(THREE, &full, &run()), chosen("a3"), "100 と 130（cap しない）は当たっている");
        assert_eq!(
            choose(&["a1", "a2"], &full, &run()),
            none(NoCandidateReason::AllLimited, Some(FIVE_RESET)),
            "当たっている口座だけなら候補なし"
        );
    }

    #[test]
    fn select_skips_unmeasured_and_stale_accounts() {
        let rows = table(&[
            (TS, [round("a1", 10, 10), round("a2", 20, 20)].concat()),
            // a1 の次の回は口座単位で測れなかった（前の回の実測を最新と読まない）。
            (LATER_TS, vec![unmeasured("a1", None)]),
            // a3: reset を過ぎた行しか無い。
            (TS, vec![
                measured("a3", WindowKind::FiveHour, None, 50, PAST_RESET),
                measured("a3", WindowKind::SevenDay, None, 60, PAST_RESET),
            ]),
            // a4: 5h は古い（100 でも当たっていない）・7d は 40。
            (TS, vec![
                measured("a4", WindowKind::FiveHour, None, 100, PAST_RESET),
                measured("a4", WindowKind::SevenDay, None, 40, WEEK_RESET),
            ]),
            // a5: 窓 1 つが Unmeasured。
            (TS, vec![
                measured("a5", WindowKind::FiveHour, None, 70, FIVE_RESET),
                unmeasured("a5", Some(WindowKind::SevenDay)),
            ]),
        ]);
        let all = ["a1", "a2", "a3", "a4", "a5", "a6"];
        assert_eq!(choose(&all, &rows, &run()), chosen("a4"), "古い 5h の 100 は数えない・測れない口座は選ばない");
        assert_eq!(choose(&all, &rows, &session()), chosen("a2"));
        assert_eq!(
            choose(&["a1", "a3", "a5", "a6"], &rows, &run()),
            none(NoCandidateReason::Unmeasured, None),
            "最新が Unmeasured・古い行だけ・窓の欠け・行なし"
        );
    }

    #[test]
    fn select_counts_idle_window_without_reset_as_fresh_and_never_as_reopen_time() {
        let rows = table(&[(TS, vec![
            // a1: 5h は消費が無い（reset 無し）・7d は 20。
            idle("a1", WindowKind::FiveHour),
            measured("a1", WindowKind::SevenDay, None, 20, WEEK_RESET),
            // a2: 2 窓とも消費が無い。
            idle("a2", WindowKind::FiveHour),
            idle("a2", WindowKind::SevenDay),
            // a3: 7d が当たっている・5h は reset 無し。
            idle("a3", WindowKind::FiveHour),
            measured("a3", WindowKind::SevenDay, None, 100, WEEK_RESET),
        ])]);
        assert_eq!(choose(&["a1", "a2", "a3"], &rows, &run()), chosen("a1"), "reset 無しの窓は数える・逼迫度は最大の 20");
        assert_eq!(choose(&["a2"], &rows, &run()), chosen("a2"), "全窓が reset 無しでも測れた口座");
        assert_eq!(choose(&["a1", "a2"], &rows, &session()), chosen("a2"), "session 用は最小（0）");
        assert_eq!(
            choose(&["a3"], &rows, &run()),
            none(NoCandidateReason::AllLimited, Some(WEEK_RESET)),
            "reset 無しの行は開き直る時刻に入らない"
        );
    }

    #[test]
    fn select_ties_break_by_label_order() {
        let tied = table(&[(TS, [round("b", 50, 0), round("a", 0, 50), round("c", 10, 0)].concat())]);
        assert_eq!(choose(&["b", "a", "c"], &tied, &run()), chosen("a"));
        let low = table(&[(TS, [round("b", 5, 0), round("a", 0, 5)].concat())]);
        assert_eq!(choose(&["b", "a"], &low, &session()), chosen("a"));
    }

    #[test]
    fn select_all_limited_carries_the_earliest_reset() {
        let soon = "2026-09-13T08:00:00Z";
        let rows = table(&[(TS, vec![
            // a1 は 2 窓とも当たっている＝開き直るのは遅い方（WEEK_RESET）。
            measured("a1", WindowKind::FiveHour, None, 100, soon),
            measured("a1", WindowKind::SevenDay, None, 100, WEEK_RESET),
            measured("a2", WindowKind::FiveHour, None, 100, FIVE_RESET),
            measured("a2", WindowKind::SevenDay, None, 3, WEEK_RESET),
        ])]);
        let want = none(NoCandidateReason::AllLimited, Some(FIVE_RESET));
        assert_eq!(choose(&["a1", "a2"], &rows, &run()), want, "a2 が先に開く");
        assert_eq!(choose(&["a1", "a2"], &rows, &session()), want);
    }

    #[test]
    fn select_model_windows_count_only_the_given_model() {
        let rows = table(&[(TS, vec![
            measured("a1", WindowKind::FiveHour, None, 10, FIVE_RESET),
            measured("a1", WindowKind::SevenDay, None, 10, WEEK_RESET),
            measured("a1", WindowKind::SevenDayModel, Some("Fable"), 95, WEEK_RESET),
            measured("a1", WindowKind::SevenDayModel, Some("Opus"), 20, WEEK_RESET),
            measured("a2", WindowKind::FiveHour, None, 50, FIVE_RESET),
            measured("a2", WindowKind::SevenDay, None, 50, WEEK_RESET),
        ])]);
        let pair = ["a1", "a2"];
        assert_eq!(choose(&pair, &rows, &Ask { model: Some("Opus"), ..session() }), chosen("a1"), "Opus の窓（20）だけ数える");
        assert_eq!(choose(&pair, &rows, &Ask { model: Some("Fable"), ..session() }), chosen("a2"), "Fable は 95");
        assert_eq!(choose(&pair, &rows, &session()), chosen("a2"), "model なしは全 model の最大（95）");
        assert_eq!(choose(&pair, &rows, &run()), chosen("a1"), "便用: a1 = 95 が最大");
        assert_eq!(choose(&pair, &rows, &Ask { model: Some("Opus"), ..run() }), chosen("a2"), "Opus だけなら a1 = 20");
        let limited = table(&[(TS, vec![
            measured("a1", WindowKind::FiveHour, None, 0, FIVE_RESET),
            measured("a1", WindowKind::SevenDayModel, Some("Fable"), 100, WEEK_RESET),
        ])]);
        assert_eq!(choose(&["a1"], &limited, &Ask { model: Some("Opus"), ..run() }), chosen("a1"), "他の model の 100 は数えない");
        assert_eq!(choose(&["a1"], &limited, &run()), none(NoCandidateReason::AllLimited, Some(WEEK_RESET)));
    }

    #[test]
    fn select_reason_folds_to_one_in_declaration_order() {
        let rows = table(&[(TS, [round("a1", 0, 0), round("a3", 100, 0), round("a4", 90, 0)].concat())]);
        // a1 = 除外・a2 = 実測なし・a3 = 当たっている・a4 = 閾値以上。
        let ask = Ask { exclude: &["a1"], ..session() };
        assert_eq!(choose(&["a1", "a2", "a3", "a4"], &rows, &ask), none(NoCandidateReason::OverThreshold, Some(FIVE_RESET)));
        assert_eq!(choose(&["a1", "a2", "a3"], &rows, &ask), none(NoCandidateReason::AllLimited, Some(FIVE_RESET)));
        assert_eq!(choose(&["a1", "a2"], &rows, &ask), none(NoCandidateReason::Unmeasured, None));
        assert_eq!(choose(&["a1"], &rows, &ask), none(NoCandidateReason::Excluded, None));
        assert_eq!(choose(&[], &rows, &ask), none(NoCandidateReason::Unmeasured, None), "口座 0 は測れる口座が無い");
    }

    #[test]
    fn select_names_are_pinned_in_declaration_order() {
        let reasons: Vec<&str> = NO_CANDIDATE_REASONS.iter().map(|reason| reason.as_str()).collect();
        assert_eq!(reasons, ["over-threshold", "all-limited", "unmeasured", "excluded"]);
        assert!(is_declaration_order(NO_CANDIDATE_REASONS, |reason| reason as usize));
        assert!(is_declaration_order(PURPOSES, |purpose| purpose as usize));
        for purpose in PURPOSES {
            assert_eq!(Purpose::parse(purpose.as_str()), Some(*purpose));
        }
        assert_eq!(Purpose::parse("Run"), None, "字面は小文字だけ");
        assert_eq!(NoCandidateReason::POLARITY.on_failure, OnFailure::FailOpen);
    }

    #[test]
    fn select_line_names_the_choice_or_the_reason() {
        assert_eq!(line(Purpose::Run, &chosen("a2")), "select purpose=run chosen=a2");
        assert_eq!(
            line(Purpose::Session, &none(NoCandidateReason::AllLimited, Some(FIVE_RESET))),
            format!("select purpose=session none=all-limited earliest_reset={FIVE_RESET}")
        );
        assert_eq!(
            line(Purpose::Session, &none(NoCandidateReason::OverThreshold, None)),
            "select purpose=session none=over-threshold earliest_reset=-"
        );
    }

    /// 反例の永続化を切り、case 数を 256 に pin する（`tests/e2e/prop.rs` と同じ形）。
    fn config() -> Config {
        Config {
            cases: 256,
            failure_persistence: None,
            ..Config::default()
        }
    }

    /// property の口座の母集団。
    const POOL: [&str; 5] = ["a1", "a2", "a3", "a4", "a5"];

    /// 入力の順序そのまま。
    const IDENTITY: [usize; 5] = [0, 1, 2, 3, 4];

    /// 口座 1 つの振り方。`kind`: 0 = 実測なし・1 = 口座単位の Unmeasured・2 = 実測（5h・7d・Fable）。
    #[derive(Debug, Clone)]
    struct Spec {
        kind: u8,
        five: u64,
        seven: u64,
        fable: u64,
        stale: bool,
        excluded: bool,
    }

    fn spec() -> impl Strategy<Value = Spec> {
        (0_u8..3, 0_u64..=120, 0_u64..=120, 0_u64..=120, prop::bool::weighted(0.2), prop::bool::weighted(0.2))
            .prop_map(|(kind, five, seven, fable, stale, excluded)| Spec { kind, five, seven, fable, stale, excluded })
    }

    fn specs() -> impl Strategy<Value = Vec<Spec>> {
        prop::collection::vec(spec(), POOL.len())
    }

    fn purposes() -> impl Strategy<Value = Purpose> {
        prop::sample::select(PURPOSES.to_vec())
    }

    fn models() -> impl Strategy<Value = Option<&'static str>> {
        prop::sample::select(vec![None, Some("Fable"), Some("Opus")])
    }

    /// 振り方の列から表を組む（`POOL` の順に対応）。
    fn world(specs: &[Spec]) -> BTreeMap<AllowanceKey, AllowanceLatest> {
        let mut rows = Vec::new();
        for (label, spec) in POOL.iter().zip(specs) {
            let (five_reset, week_reset) = if spec.stale { (PAST_RESET, PAST_RESET) } else { (FIVE_RESET, WEEK_RESET) };
            match spec.kind {
                1 => rows.push(unmeasured(label, None)),
                2 => {
                    rows.push(measured(label, WindowKind::FiveHour, None, spec.five, five_reset));
                    rows.push(measured(label, WindowKind::SevenDay, None, spec.seven, week_reset));
                    rows.push(measured(label, WindowKind::SevenDayModel, Some("Fable"), spec.fable, week_reset));
                }
                _ => {}
            }
        }
        table(&[(TS, rows)])
    }

    /// 振り方から読んだ逼迫度（候補から外れる口座は `None`）。
    fn pressure(spec: &Spec, model: Option<&str>) -> Option<u64> {
        if spec.kind != 2 || spec.stale || spec.excluded {
            return None;
        }
        let fable = if matches!(model, None | Some("Fable")) { spec.fable } else { 0 };
        Some(spec.five.max(spec.seven).max(fable))
    }

    /// 振り方が用途の候補か。
    fn is_candidate(spec: &Spec, purpose: Purpose, model: Option<&str>, threshold: u64) -> bool {
        pressure(spec, model).is_some_and(|found| found < LIMIT_PCT && (purpose == Purpose::Run || found < threshold))
    }

    /// 選ばれた label の振り方の逼迫度。
    fn chosen_pressure(specs: &[Spec], label: &str, model: Option<&str>) -> Option<u64> {
        let at = POOL.iter().position(|found| *found == label)?;
        pressure(specs.get(at)?, model)
    }

    /// `order` の順に label を並べて選ぶ。
    fn evaluate(specs: &[Spec], order: &[usize], purpose: Purpose, model: Option<&str>, threshold: u64) -> Selection {
        let labels: Vec<String> = order.iter().filter_map(|at| POOL.get(*at)).map(|label| (*label).to_owned()).collect();
        let exclude: BTreeSet<String> = POOL
            .iter()
            .zip(specs)
            .filter(|(_, spec)| spec.excluded)
            .map(|(label, _)| (*label).to_owned())
            .collect();
        let allowance = world(specs);
        select(&Input { labels: &labels, allowance: &allowance, purpose, model, exclude: &exclude, threshold_pct: threshold, now: NOW })
    }

    proptest! {
        #![proptest_config(config())]

        /// 選ばれた label は入力の列の元で、除外集合に無く、当たっていない。候補が在る周は必ず選ぶ。
        #[test]
        fn prop_select_chosen_is_an_input_not_excluded_and_not_limited(
            specs in specs(), purpose in purposes(), model in models(), threshold in 0_u64..=120
        ) {
            let found = evaluate(&specs, &IDENTITY, purpose, model, threshold);
            let any = specs.iter().any(|spec| is_candidate(spec, purpose, model, threshold));
            prop_assert_eq!(matches!(found, Selection::Chosen(_)), any);
            if let Selection::Chosen(label) = &found {
                let at = POOL.iter().position(|item| *item == label.as_str());
                prop_assert!(at.is_some());
                let spec = at.and_then(|at| specs.get(at));
                prop_assert!(spec.is_some_and(|spec| !spec.excluded));
                prop_assert!(chosen_pressure(&specs, label, model).is_some_and(|found| found < LIMIT_PCT));
            }
        }

        /// 便用の選択は候補の中で逼迫度が最大（他のどの候補も上回らない）。
        #[test]
        fn prop_select_run_choice_is_exceeded_by_no_candidate(specs in specs(), model in models()) {
            if let Selection::Chosen(label) = evaluate(&specs, &IDENTITY, Purpose::Run, model, THRESHOLD) {
                let mine = chosen_pressure(&specs, &label, model);
                for spec in &specs {
                    if is_candidate(spec, Purpose::Run, model, THRESHOLD) {
                        prop_assert!(pressure(spec, model) <= mine);
                    }
                }
            }
        }

        /// session 用の選択は閾値未満で、候補の中で逼迫度が最小。
        #[test]
        fn prop_select_session_choice_is_under_threshold(
            specs in specs(), model in models(), threshold in 0_u64..=120
        ) {
            if let Selection::Chosen(label) = evaluate(&specs, &IDENTITY, Purpose::Session, model, threshold) {
                let mine = chosen_pressure(&specs, &label, model);
                prop_assert!(mine.is_some_and(|found| found < threshold));
                for spec in &specs {
                    if is_candidate(spec, Purpose::Session, model, threshold) {
                        prop_assert!(mine <= pressure(spec, model));
                    }
                }
            }
        }

        /// 入力の順序を入れ替えても結果は不変。
        #[test]
        fn prop_select_is_invariant_under_input_order(
            specs in specs(),
            purpose in purposes(),
            model in models(),
            threshold in 0_u64..=120,
            order in Just(IDENTITY.to_vec()).prop_shuffle()
        ) {
            prop_assert_eq!(
                evaluate(&specs, &order, purpose, model, threshold),
                evaluate(&specs, &IDENTITY, purpose, model, threshold)
            );
        }
    }
}
