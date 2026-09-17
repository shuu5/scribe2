//! 口座の選定（設計 docs/design/account-autonomy.md §3・ADR-0020 §2.2・ADR-0027・FR36）。
//!
//! 入力は値だけ（口座 label の列・replay の `allowance`・用途・model・除外集合・走行中の便数・R-C9-1 の値・
//! `now`）で、I/O も env も持たない（C2.2・C10: 実測行を通してだけ選ぶ）。**選定はこの 1 関数**（[`select`]）が
//! 持ち、便の再開と席の立て直しが同じものを呼ぶ（C2）。候補なしは断りではなく typed な理由
//! （[`NoCandidate`]・[`NoCandidateReason::POLARITY`] = FailOpen）。
//!
//! 便用の順序（ADR-0042・ADR-0027 §2.2 の鍵 (1) を supersede・C9.2「窓の終わりまで使い切る」）: 候補を
//! **(1) 口座単位の 7 日窓の reset**（昇順・7 日窓の reset を持たない口座は最後）→ **(2) 走行中の便数**（昇順）→
//! **(3) label** で並べた先頭。7 日窓の枠は reset までに使わなければ消えるので、**消える順に使い潰す**。
//! 5 時間窓とモデル別窓は鍵にしない（当たっている判定と逼迫度にだけ効く）。逼迫度（使用率の最大）は当たって
//! いる判定と session 用にだけ残る。
//!
//! session 用は並べ替えの**前**に [`Input::prefer`]（自席の登録 row の口座）を見る（ADR-0028 §2.4・`s2-07l.312`）:
//! それが候補（除外に無く・測れていて・当たっておらず・R-C9-1 未満）ならその口座に留まる。planner / admin の席が
//! 立て直しのたびに逼迫度最小の別口座へ動く形（2026-09-15 01:15Z 実測）を閉じる。便用は `prefer` を読まない。

use super::{Allowance, AllowanceKey, AllowanceLatest, Measured, WindowKind};
use crate::polarity::{OnFailure, Polarity, Timing};
use std::collections::{BTreeMap, BTreeSet};

/// 当たっている（窓の全量に達した）使用率。**規則値ではない**（設計 §3）。
pub const LIMIT_PCT: u64 = 100;

/// 選定の用途。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// 便用: 当たっていない口座のうち **7 日窓の reset** が最も早い → 走行中の便数が最少 → label（reset で消える
    /// 残りから使う側・C9.2・ADR-0042）。閾値を持たない。
    Run,
    /// session 用: [`Input::prefer`] が候補ならそれ・でなければ逼迫度が最小かつ R-C9-1 の値未満（余裕を残す側）。
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

/// claude の model（**閉じた enum**・`s2-07l.297`・憲法 C2 / C10「宣言値と実測値を型で分ける」）。
///
/// model には**2 つの語彙**が在る: rules 行 `runner.model` と claude CLI の `--model` が使う**別名**
/// （[`Model::alias`]・`opus` …）と、口座残量の実測行 `SevenDayModel` の `model` が持つ usage API の
/// **表示名**（[`Model::display`]・`Opus` …）。字面で比べると本番の組（`opus` × `Opus`）が 1 行も
/// 一致せず、便用の選定が「その model の窓」を数え損ねる（run 1 の Gated FAIL 2026-09-15）。比較は
/// [`Model::parse`] の**閉じた表 1 つ**で両側を型にしてから行う（大小文字の無視・字面の寄せは採らない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    /// Opus。
    Opus,
    /// Fable。
    Fable,
    /// Sonnet。
    Sonnet,
    /// Haiku。
    Haiku,
}

/// [`Model`] の全 variant（宣言順）。
pub const MODELS: &[Model] = &[
    Model::Opus,
    Model::Fable,
    Model::Sonnet,
    Model::Haiku,
];

impl Model {
    /// claude CLI の別名（`--model` の値・rules 行 `runner.model` の語彙）。
    pub fn alias(self) -> &'static str {
        match self {
            Self::Opus => "opus",
            Self::Fable => "fable",
            Self::Sonnet => "sonnet",
            Self::Haiku => "haiku",
        }
    }

    /// 口座残量の実測行の表示名（usage API の `scope.model.display_name` の語彙・席の登録 row の `model`）。
    pub fn display(self) -> &'static str {
        match self {
            Self::Opus => "Opus",
            Self::Fable => "Fable",
            Self::Sonnet => "Sonnet",
            Self::Haiku => "Haiku",
        }
    }

    /// 別名か表示名との**完全一致**で引く（case-fold しない）。表に無い字面は `None`。
    pub fn parse(text: &str) -> Option<Self> {
        MODELS.iter().copied().find(|found| found.alias() == text || found.display() == text)
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
    /// 使う model（別名か表示名・[`Model::parse`] の語彙・与えられた周はモデル別窓のうちその model の行だけを
    /// 数える）。字面のまま運び、型にするのは [`counts`] の中（構築点は `fleet select` / 便用 / 席の 3 つ）。
    pub model: Option<&'a str>,
    /// 候補から外す label。
    pub exclude: &'a BTreeSet<String>,
    /// 口座 label → 走行中の便数（replay の導出値・[`super::State::inflight_by_account`]・無い label は 0）。便用の
    /// 2 つ目の鍵（ADR-0027 §2.3）。session 用は読まない（空の map で良い）。
    pub inflight: &'a BTreeMap<String, usize>,
    /// R-C9-1 の値（session 用の閾値・使用率の百分率・未満なら候補）。
    pub threshold_pct: u64,
    /// いまの UTC（`YYYY-MM-DDTHH:MM:SSZ`）。reset を過ぎた行を古いと読むのに使う。
    pub now: &'a str,
    /// session 用が留まる口座（自席の登録 row の口座・ADR-0028 §2.4）。候補（除外に無く・測れていて・当たっておらず・
    /// 閾値未満）ならその口座を選び、候補でなければ逼迫度の最小へ。`seat launch` の初回（row が無い）・`fleet select`・
    /// 便用は `None`（便用は与えられても読まない）。
    pub prefer: Option<&'a str>,
}

/// 候補 1 つ（並べる鍵を全部持つ）。
struct Candidate<'a> {
    /// 口座 label（3 つ目の鍵・session 用の同点の鍵）。
    label: &'a str,
    /// 逼迫度（session 用の鍵）。
    pressure: u64,
    /// 口座単位の 7 日窓の reset（便用の 1 つ目の鍵・`None` = 7 日窓が reset を持たない＝最後・ADR-0042）。
    week_reset: Option<String>,
    /// 走行中の便数（便用の 2 つ目の鍵）。
    inflight: usize,
}

/// 口座 1 つの見立て。
enum Standing<'a> {
    /// 候補。
    Candidate(Candidate<'a>),
    /// 候補から外れた（理由と、当たっている周はその口座が開き直る時刻）。
    Out(NoCandidateReason, Option<String>),
}

/// 口座を 1 つ選ぶ。同点は label の辞書順で先の口座。
pub fn select(input: &Input<'_>) -> Selection {
    let mut candidates: Vec<Candidate<'_>> = Vec::new();
    let mut outs: Vec<(NoCandidateReason, Option<String>)> = Vec::new();
    for label in input.labels {
        match standing(input, label) {
            Standing::Candidate(found) => candidates.push(found),
            Standing::Out(reason, reopens) => outs.push((reason, reopens)),
        }
    }
    match pick(input.purpose, input.prefer, &candidates) {
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

/// 候補の中から用途の規則で 1 つ選ぶ。便用は [`run_key`] の昇順の先頭・session 用は `prefer` が候補ならそれ・
/// でなければ逼迫度の最小（同点は label）。候補に無い `prefer`（除外・測れない・当たっている・閾値以上・宣言に
/// 無い）は [`standing`] で既に外れているので、ここでは候補の列に在るかだけを見る。
fn pick<'a>(purpose: Purpose, prefer: Option<&str>, candidates: &[Candidate<'a>]) -> Option<&'a str> {
    let ranked = candidates.iter();
    let found = match purpose {
        Purpose::Run => ranked.min_by(|a, b| run_key(a).cmp(&run_key(b))),
        Purpose::Session => candidates
            .iter()
            .find(|found| Some(found.label) == prefer)
            .or_else(|| ranked.min_by(|a, b| a.pressure.cmp(&b.pressure).then(a.label.cmp(b.label)))),
    };
    found.map(|found| found.label)
}

/// 便用の並べ鍵（ADR-0042・ADR-0027 §2.2 の鍵 (1) を supersede）: **7 日窓の reset** の最も早いもの（鍵の reset を
/// 持たない口座は最後＝先頭の `bool` が立つ）→ 走行中の便数 → label。5 時間窓・モデル別窓の reset はここに入らない。
/// 辞書順の比較でそのまま並ぶ形にしておく（比較関数に分岐を持たない）。
fn run_key<'a>(found: &'a Candidate<'_>) -> (bool, Option<&'a str>, usize, &'a str) {
    (found.week_reset.is_none(), found.week_reset.as_deref(), found.inflight, found.label)
}

/// 口座 1 つを候補か、外れた理由かに分ける（除外 → 測れない → 当たっている → 閾値の順に見る）。
fn standing<'a>(input: &Input<'_>, label: &'a str) -> Standing<'a> {
    if input.exclude.contains(label) {
        return Standing::Out(NoCandidateReason::Excluded, None);
    }
    let Some(found) = reading(input, label) else {
        return Standing::Out(NoCandidateReason::Unmeasured, None);
    };
    if found.pressure >= LIMIT_PCT {
        return Standing::Out(NoCandidateReason::AllLimited, found.reopens);
    }
    if input.purpose == Purpose::Session && found.pressure >= input.threshold_pct {
        return Standing::Out(NoCandidateReason::OverThreshold, None);
    }
    Standing::Candidate(Candidate {
        label,
        pressure: found.pressure,
        week_reset: found.week_reset,
        inflight: input.inflight.get(label).copied().unwrap_or(0),
    })
}

/// 口座 1 つの読み（数える窓の古くない実測から導く値の組）。
struct Reading {
    /// 逼迫度 = 数える窓のうち最大の使用率。
    pressure: u64,
    /// 開き直る時刻 = 当たっている窓の reset の**遅い方**（全部の窓が開くまで当たったまま）。
    reopens: Option<String>,
    /// 口座単位の 7 日窓（[`WindowKind::SevenDay`]・モデル別窓ではない）の古くない実測の reset（便用の 1 つ目の
    /// 鍵・ADR-0042）。7 日窓が消費の無い窓（reset 未定）か、古くて落ちた周は `None`。
    week_reset: Option<String>,
}

/// 口座の読み。測れない口座は `None`。reset 無しの行は開き直る時刻にも 7 日窓の鍵にも入らない
/// （待つ対象でも「reset で消える残り」でもない・ADR-0024 §2.2）。
fn reading(input: &Input<'_>, label: &str) -> Option<Reading> {
    let windows = fresh_windows(input, label)?;
    let pressure = windows.iter().map(|found| found.used_pct).max()?;
    let reopens = windows
        .iter()
        .filter(|found| found.used_pct >= LIMIT_PCT)
        .filter_map(|found| found.resets_at.clone())
        .max();
    let week_reset = windows
        .iter()
        .filter(|found| found.window == WindowKind::SevenDay)
        .filter_map(|found| found.resets_at.clone())
        .min();
    Some(Reading { pressure, reopens, week_reset })
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
///
/// **型の境目はここ**: 与えられた model（別名か表示名）と行の表示名を [`Model::parse`] で型にしてから比べる
/// （字面比較を `counts` の外に残さない）。与えられた model が表に無い周は**保守側で数える**（`model = None`
/// と同じ・fail-open にしない）。行の表示名が表に無い周は数えない（別 model の窓・従来どおり）。
fn counts(model: Option<&str>, window: Option<WindowKind>, row_model: Option<&str>) -> bool {
    match (window, model, row_model) {
        (Some(WindowKind::SevenDayModel), Some(want), Some(found)) => match Model::parse(want) {
            Some(want) => Model::parse(found) == Some(want),
            None => true,
        },
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        line, select, Input, Model, NoCandidate, NoCandidateReason, Purpose, Selection, LIMIT_PCT,
        MODELS, NO_CANDIDATE_REASONS, PURPOSES,
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
        inflight: &'a [(&'a str, usize)],
        threshold_pct: u64,
        prefer: Option<&'a str>,
    }

    fn run() -> Ask<'static> {
        Ask { purpose: Purpose::Run, model: None, exclude: &[], inflight: &[], threshold_pct: THRESHOLD, prefer: None }
    }

    fn session() -> Ask<'static> {
        Ask { purpose: Purpose::Session, ..run() }
    }

    fn choose(labels: &[&str], allowance: &BTreeMap<AllowanceKey, AllowanceLatest>, ask: &Ask<'_>) -> Selection {
        let labels: Vec<String> = labels.iter().map(|label| (*label).to_owned()).collect();
        let exclude: BTreeSet<String> = ask.exclude.iter().map(|label| (*label).to_owned()).collect();
        let inflight: BTreeMap<String, usize> = ask.inflight.iter().map(|(label, n)| ((*label).to_owned(), *n)).collect();
        select(&Input {
            labels: &labels,
            allowance,
            purpose: ask.purpose,
            model: ask.model,
            exclude: &exclude,
            inflight: &inflight,
            threshold_pct: ask.threshold_pct,
            now: NOW,
            prefer: ask.prefer,
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

    /// `NOW` の 1 時間後の reset（`FIVE_RESET` より早い）。
    const SOON_RESET: &str = "2026-09-13T07:00:00Z";
    /// `NOW` の 4 時間後の reset（`SOON_RESET` より遅く `FIVE_RESET` より遅い）。
    const LATE_RESET: &str = "2026-09-13T10:00:00Z";
    /// 7 日窓の早い側の reset（`WEEK_RESET` より早く、5 時間窓のどの reset より**遅い**＝鍵が窓の種類で分かれる・
    /// ADR-0042）。
    const WEEK_SOON_RESET: &str = "2026-09-16T00:00:00Z";

    /// (a) 便用は逼迫度でなく **7 日窓の reset が最も早い**候補を選ぶ（ADR-0042・C9.2「reset で消える残りから使う」）:
    /// a1（5h 20%・5h の reset は遅く 7d の reset が早い）と a2（5h 80%・5h の reset は早く 7d の reset が遅い）→ a1。
    /// base（数える窓の最小＝5h）は a2 → RED。
    #[test]
    fn select_run_prefers_the_earliest_reset() {
        let rows = table(&[(TS, vec![
            measured("a1", WindowKind::FiveHour, None, 20, LATE_RESET),
            measured("a1", WindowKind::SevenDay, None, 10, WEEK_SOON_RESET),
            measured("a2", WindowKind::FiveHour, None, 80, SOON_RESET),
            measured("a2", WindowKind::SevenDay, None, 10, WEEK_RESET),
        ])]);
        assert_eq!(choose(&["a1", "a2"], &rows, &run()), chosen("a1"), "7d の reset が早い a1（5h の reset なら a2）");
        assert_eq!(choose(&["a2", "a1"], &rows, &run()), chosen("a1"), "入力順に依らない");
        assert_eq!(choose(&["a1", "a2"], &rows, &session()), chosen("a1"), "session 用は逼迫度の最小のまま（20）");
        // 向きを入れ替えても鍵は 7 日窓: a2 の 7d が早ければ 5h が遅くても a2。
        let week_first = table(&[(TS, vec![
            measured("a1", WindowKind::FiveHour, None, 20, SOON_RESET),
            measured("a1", WindowKind::SevenDay, None, 10, WEEK_RESET),
            measured("a2", WindowKind::FiveHour, None, 80, LATE_RESET),
            measured("a2", WindowKind::SevenDay, None, 10, WEEK_SOON_RESET),
        ])]);
        assert_eq!(choose(&["a1", "a2"], &week_first, &run()), chosen("a2"), "鍵は 7 日窓の reset だけ");
        // 古い 7 日窓の行（reset を過ぎた）は `select_run_week_stale_seven_day_row_goes_last` が測る。
    }

    /// (b) reset を持つ窓が 1 つも無い口座（消費の無い窓だけ・ADR-0024）は**最後**: a1（reset 無し）と a2（reset +4h）→ a2。
    /// 候補が a1 だけなら a1 を選ぶ（候補から外れはしない）。
    #[test]
    fn select_run_puts_accounts_without_any_reset_last() {
        let rows = table(&[(TS, vec![
            idle("a1", WindowKind::FiveHour),
            idle("a1", WindowKind::SevenDay),
            measured("a2", WindowKind::FiveHour, None, 90, LATE_RESET),
            measured("a2", WindowKind::SevenDay, None, 90, WEEK_RESET),
        ])]);
        assert_eq!(choose(&["a1", "a2"], &rows, &run()), chosen("a2"), "reset の無い a1 は最後");
        assert_eq!(choose(&["a1"], &rows, &run()), chosen("a1"), "reset が無くても候補");
        assert_eq!(choose(&["a1", "a2"], &rows, &session()), chosen("a1"), "session 用は逼迫度の最小（0）");
    }

    /// (c) reset が同点なら**走行中の便数**が少ない口座: `inflight` a1 = 2 / a2 = 0 → a2。map に無い label は 0。
    #[test]
    fn select_run_breaks_ties_by_fewer_inflight_runs() {
        let rows = table(&[(TS, [round("a1", 10, 10), round("a2", 10, 10), round("a3", 10, 10)].concat())]);
        let pair = ["a1", "a2"];
        assert_eq!(choose(&pair, &rows, &Ask { inflight: &[("a1", 2), ("a2", 0)], ..run() }), chosen("a2"));
        assert_eq!(choose(&pair, &rows, &Ask { inflight: &[("a1", 2)], ..run() }), chosen("a2"), "無い label は 0");
        assert_eq!(choose(&pair, &rows, &Ask { inflight: &[("a2", 1)], ..run() }), chosen("a1"));
        assert_eq!(choose(&pair, &rows, &Ask { inflight: &[("a1", 1), ("a2", 1)], ..run() }), chosen("a1"), "同数は label");
        assert_eq!(
            choose(&["a1", "a2", "a3"], &rows, &Ask { inflight: &[("a1", 3), ("a2", 1), ("a3", 2)], ..run() }),
            chosen("a2"),
            "3 口座でも最少"
        );
        // reset が先: 便数が多くても 7 日窓の reset が早い口座が勝つ（5 時間窓の reset は逆向きに置く）。
        let soon = table(&[(TS, vec![
            measured("a1", WindowKind::FiveHour, None, 10, FIVE_RESET),
            measured("a1", WindowKind::SevenDay, None, 10, WEEK_SOON_RESET),
            measured("a2", WindowKind::FiveHour, None, 10, SOON_RESET),
            measured("a2", WindowKind::SevenDay, None, 10, WEEK_RESET),
        ])]);
        assert_eq!(choose(&pair, &soon, &Ask { inflight: &[("a1", 5)], ..run() }), chosen("a1"), "7 日窓の reset が便数より先");
        assert_eq!(choose(&pair, &rows, &Ask { inflight: &[("a1", 5)], ..session() }), chosen("a1"), "session 用は便数を読まない");
    }

    /// (d) reset も便数も同点なら label の辞書順の先頭（`three()` は全口座が同じ reset・a3 は当たっている）。
    #[test]
    fn select_run_then_label_order() {
        assert_eq!(choose(THREE, &three(), &run()), chosen("a1"), "同じ reset・便数 0・a3 は当たっている");
        assert_eq!(choose(&["a2", "a1", "a3"], &three(), &run()), chosen("a1"), "入力順に依らない");
        assert_eq!(choose(&["a2", "a3"], &three(), &run()), chosen("a2"));
    }

    // ───── ADR-0042: 便用の 1 つ目の鍵は口座単位の 7 日窓の reset（接頭辞 `select_run_week_`） ─────

    /// (c) の表: 7 日窓の reset が同じ 2 口座で、5 時間窓の reset だけが違う（a1 = 遅い・a2 = 早い）。
    /// 逼迫度は a1 = 40 / a2 = 10（席用の答えが便用と別になる向き）。
    fn week_tied() -> BTreeMap<AllowanceKey, AllowanceLatest> {
        table(&[(TS, vec![
            measured("a1", WindowKind::FiveHour, None, 40, LATE_RESET),
            measured("a1", WindowKind::SevenDay, None, 10, WEEK_RESET),
            measured("a2", WindowKind::FiveHour, None, 10, SOON_RESET),
            measured("a2", WindowKind::SevenDay, None, 10, WEEK_RESET),
        ])])
    }

    /// (d) の表: a1 は 7 日窓が消費の無い窓（reset 未定・ADR-0024）で 5 時間窓に早い reset・a2 は 7 日窓に reset。
    /// 逼迫度は a1 = 10 / a2 = 60。
    fn week_missing() -> BTreeMap<AllowanceKey, AllowanceLatest> {
        table(&[(TS, vec![
            measured("a1", WindowKind::FiveHour, None, 10, SOON_RESET),
            idle("a1", WindowKind::SevenDay),
            measured("a2", WindowKind::FiveHour, None, 60, LATE_RESET),
            measured("a2", WindowKind::SevenDay, None, 50, WEEK_RESET),
        ])])
    }

    /// (e) の表: a1 の 7 日窓の実測は reset を過ぎて古い（`fresh_windows` から落ちる）・5 時間窓は新しい。
    /// 逼迫度は a1 = 10（古い 90 は数えない）/ a2 = 60。
    fn week_stale() -> BTreeMap<AllowanceKey, AllowanceLatest> {
        table(&[(TS, vec![
            measured("a1", WindowKind::FiveHour, None, 10, SOON_RESET),
            measured("a1", WindowKind::SevenDay, None, 90, PAST_RESET),
            measured("a2", WindowKind::FiveHour, None, 60, LATE_RESET),
            measured("a2", WindowKind::SevenDay, None, 50, WEEK_RESET),
        ])])
    }

    /// (f) の表: a1 はモデル別 7 日窓の reset が表の中で最も早く、口座単位の 7 日窓の reset は遅い・a2 は 7 日窓の
    /// reset が早い。逼迫度は a1 = 20 / a2 = 60。
    fn week_model() -> BTreeMap<AllowanceKey, AllowanceLatest> {
        table(&[(TS, vec![
            measured("a1", WindowKind::FiveHour, None, 10, LATE_RESET),
            measured("a1", WindowKind::SevenDay, None, 20, WEEK_RESET),
            measured("a1", WindowKind::SevenDayModel, Some("Opus"), 15, SOON_RESET),
            measured("a2", WindowKind::FiveHour, None, 60, LATE_RESET),
            measured("a2", WindowKind::SevenDay, None, 50, WEEK_SOON_RESET),
        ])])
    }

    /// (c) 7 日窓の reset が同じなら**走行中の便数** → label（5 時間窓の reset の差は並びに効かない）。
    /// base（数える窓の最小）は 5h の早い a2 を便数に依らず選ぶ → RED。
    #[test]
    fn select_run_week_same_seven_day_reset_falls_to_inflight_then_label() {
        let rows = week_tied();
        let pair = ["a1", "a2"];
        assert_eq!(
            choose(&pair, &rows, &Ask { inflight: &[("a2", 1)], ..run() }),
            chosen("a1"),
            "5h の reset が早い a2 でも便数で負ける"
        );
        assert_eq!(choose(&pair, &rows, &Ask { inflight: &[("a1", 1)], ..run() }), chosen("a2"), "便数の少ない側");
        assert_eq!(choose(&pair, &rows, &run()), chosen("a1"), "同数は label の先頭");
        assert_eq!(choose(&["a2", "a1"], &rows, &run()), chosen("a1"), "入力順に依らない");
    }

    /// (d) 7 日窓が reset を持たない口座（消費の無い窓・ADR-0024）は、5 時間窓に早い reset を持っても**最後**。
    /// 候補からは外れない（その口座だけならそれを選ぶ）。base は 5h の早い a1 → RED。
    #[test]
    fn select_run_week_account_without_seven_day_reset_goes_last_but_stays_a_candidate() {
        let rows = week_missing();
        assert_eq!(choose(&["a1", "a2"], &rows, &run()), chosen("a2"), "7 日窓の reset を持たない a1 は最後");
        assert_eq!(choose(&["a2", "a1"], &rows, &run()), chosen("a2"), "入力順に依らない");
        assert_eq!(choose(&["a1"], &rows, &run()), chosen("a1"), "鍵の reset が無くても候補");
    }

    /// (e) 7 日窓の実測が古くて `fresh_windows` から落ちた口座も**最後**（5 時間窓が新しいので候補からは外れない）。
    /// base は a1 の新しい 5h の reset を鍵にして a1 → RED。
    #[test]
    fn select_run_week_stale_seven_day_row_goes_last() {
        let rows = week_stale();
        assert_eq!(choose(&["a1", "a2"], &rows, &run()), chosen("a2"), "古い 7 日窓の行は鍵に入らない");
        assert_eq!(choose(&["a2", "a1"], &rows, &run()), chosen("a2"), "入力順に依らない");
        assert_eq!(choose(&["a1"], &rows, &run()), chosen("a1"), "5 時間窓が新しいので候補のまま");
    }

    /// (f) モデル別 7 日窓の reset は鍵にしない（`model` を与えた周も与えない周も）: a1 のモデル別窓の reset が表で
    /// 最も早くても、口座単位の 7 日窓の reset が早い a2 が先。base は a1 → RED。
    #[test]
    fn select_run_week_model_window_reset_is_not_a_key() {
        let rows = week_model();
        let pair = ["a1", "a2"];
        assert_eq!(choose(&pair, &rows, &run()), chosen("a2"), "model なし（モデル別窓も数える周）");
        assert_eq!(choose(&pair, &rows, &Ask { model: Some("Opus"), ..run() }), chosen("a2"), "その model の窓を数える周");
        assert_eq!(choose(&pair, &rows, &Ask { model: Some("Fable"), ..run() }), chosen("a2"), "数えない周も同じ");
        assert_eq!(choose(&["a2", "a1"], &rows, &run()), chosen("a2"), "入力順に依らない");
    }

    /// (g) 同じ表の席用の答えは逼迫度の最小のまま（7 日窓の reset を動かしても変わらない・ADR-0042 は便用の順序
    /// だけを差し替える）。どの表でも便用の答えとは別の口座になる置き方にしてある。
    #[test]
    fn select_run_week_session_answer_is_unchanged() {
        let pair = ["a1", "a2"];
        assert_eq!(choose(&pair, &week_tied(), &session()), chosen("a2"), "(c) 最小は 10 の a2（便用は a1）");
        assert_eq!(choose(&pair, &week_missing(), &session()), chosen("a1"), "(d) 最小は 10 の a1（便用は a2）");
        assert_eq!(choose(&pair, &week_stale(), &session()), chosen("a1"), "(e) 古い 90 を外した 10 の a1（便用は a2）");
        assert_eq!(choose(&pair, &week_model(), &session()), chosen("a1"), "(f) 最小は 20 の a1（便用は a2）");
        assert_eq!(
            choose(&pair, &week_model(), &Ask { model: Some("Opus"), ..session() }),
            chosen("a1"),
            "(f) model を与えた周も同じ"
        );
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
        assert_eq!(choose(THREE, &spread, &run()), chosen("a1"), "同じ表で便用は逼迫度を読まない（同じ reset → label・閾値も持たない）");
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

    /// (a) session 用は `prefer`（自席の登録 row の口座）が候補ならそれに留まる（ADR-0028 §2.4・`s2-07l.312`）:
    /// a1（13%・prefer）と a2（5%）→ a1。base（逼迫度最小）は a2 → RED。`prefer` 無しは従来どおり最小の a2。
    #[test]
    fn select_session_prefers_the_given_account_below_threshold() {
        let rows = table(&[(TS, [round("a1", 13, 0), round("a2", 5, 0)].concat())]);
        let pair = ["a1", "a2"];
        assert_eq!(choose(&pair, &rows, &Ask { prefer: Some("a1"), ..session() }), chosen("a1"), "13% でも自席の口座に留まる");
        assert_eq!(choose(&["a2", "a1"], &rows, &Ask { prefer: Some("a1"), ..session() }), chosen("a1"), "入力順に依らない");
        assert_eq!(choose(&pair, &rows, &session()), chosen("a2"), "prefer 無しは逼迫度の最小");
        assert_eq!(choose(&pair, &rows, &Ask { prefer: Some("a2"), ..session() }), chosen("a2"), "prefer が最小と同じなら同じ答え");
        // 候補でない prefer は読まない: 除外・宣言に無い・測れない・当たっている。
        assert_eq!(choose(&pair, &rows, &Ask { prefer: Some("a1"), exclude: &["a1"], ..session() }), chosen("a2"), "除外が先");
        assert_eq!(choose(&pair, &rows, &Ask { prefer: Some("a9"), ..session() }), chosen("a2"), "宣言に無い口座は候補でない");
        assert_eq!(choose(&["a1", "a2", "a3"], &rows, &Ask { prefer: Some("a3"), ..session() }), chosen("a2"), "実測行の無い口座");
        let limited = table(&[(TS, [round("a1", 100, 0), round("a2", 5, 0)].concat())]);
        assert_eq!(choose(&pair, &limited, &Ask { prefer: Some("a1"), ..session() }), chosen("a2"), "当たっている口座には留まらない");
        assert_eq!(
            choose(&["a1"], &limited, &Ask { prefer: Some("a1"), ..session() }),
            none(NoCandidateReason::AllLimited, Some(FIVE_RESET)),
            "prefer だけで候補なしなら候補なし"
        );
    }

    /// (b) 極性の対: `prefer` の口座が閾値以上（85 ≥ 85）なら留まらず別口座（a2 = 5%）へ。閾値の 1 つ下（84）なら留まる。
    #[test]
    fn select_session_leaves_the_preferred_account_at_threshold() {
        let pair = ["a1", "a2"];
        let at = table(&[(TS, [round("a1", THRESHOLD, 0), round("a2", 5, 0)].concat())]);
        assert_eq!(choose(&pair, &at, &Ask { prefer: Some("a1"), ..session() }), chosen("a2"), "閾値ちょうどは留まらない");
        let over = table(&[(TS, [round("a1", THRESHOLD + 10, 0), round("a2", 5, 0)].concat())]);
        assert_eq!(choose(&pair, &over, &Ask { prefer: Some("a1"), ..session() }), chosen("a2"), "閾値以上は留まらない");
        let under = table(&[(TS, [round("a1", THRESHOLD - 1, 0), round("a2", 5, 0)].concat())]);
        assert_eq!(choose(&pair, &under, &Ask { prefer: Some("a1"), ..session() }), chosen("a1"), "閾値未満は留まる");
        assert_eq!(
            choose(&["a1"], &at, &Ask { prefer: Some("a1"), ..session() }),
            none(NoCandidateReason::OverThreshold, None),
            "prefer だけで閾値以上なら候補なし（理由は従来どおり）"
        );
    }

    /// (c) 便用は `prefer` を読まない: 同じ表・同じ prefer で `Purpose::Run` の答えは prefer 無しと同じ（reset →
    /// 便数 → label の順・a1 に便 1 本なら a2）。
    #[test]
    fn select_run_ignores_prefer() {
        let rows = table(&[(TS, [round("a1", 13, 0), round("a2", 5, 0)].concat())]);
        let pair = ["a1", "a2"];
        let busy = [("a1", 1)];
        assert_eq!(choose(&pair, &rows, &Ask { prefer: Some("a1"), inflight: &busy, ..run() }), chosen("a2"), "便数が先・prefer は無視");
        assert_eq!(choose(&pair, &rows, &Ask { inflight: &busy, ..run() }), chosen("a2"), "prefer 無しと同じ");
        assert_eq!(choose(&pair, &rows, &Ask { prefer: Some("a2"), ..run() }), chosen("a1"), "同じ reset・便数 0 → label の先頭");
        assert_eq!(choose(&pair, &rows, &Ask { prefer: Some("a9"), ..run() }), choose(&pair, &rows, &run()), "宣言に無い prefer も同じ");
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
        assert_eq!(choose(&all, &rows, &run()), chosen("a2"), "候補は a2 と a4・7 日窓の reset は同じ → 便数 0 → label で a2 が先");
        assert_eq!(choose(&["a1", "a3", "a4", "a5", "a6"], &rows, &run()), chosen("a4"), "古い 5h の 100 は数えない・測れない口座は選ばない");
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
        assert_eq!(choose(&["a1", "a2", "a3"], &rows, &run()), chosen("a1"), "reset 無しの窓は数える・reset を持つ a1 が reset の無い a2 より先");
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
        assert_eq!(choose(&pair, &rows, &run()), chosen("a1"), "便用: 95 でも当たってはいない・同じ reset → label");
        assert_eq!(choose(&pair, &rows, &Ask { model: Some("Opus"), ..run() }), chosen("a1"), "Opus だけでも同じ reset → label");
        let limited = table(&[(TS, vec![
            measured("a1", WindowKind::FiveHour, None, 0, FIVE_RESET),
            measured("a1", WindowKind::SevenDayModel, Some("Fable"), 100, WEEK_RESET),
        ])]);
        assert_eq!(choose(&["a1"], &limited, &Ask { model: Some("Opus"), ..run() }), chosen("a1"), "他の model の 100 は数えない");
        assert_eq!(choose(&["a1"], &limited, &run()), none(NoCandidateReason::AllLimited, Some(WEEK_RESET)));
    }

    /// `Model::parse` は別名（`opus`）と表示名（`Opus`）の**完全一致**で同じ variant を引き、大小文字を寄せない
    /// （`OPUS` は `None`）。表は 4 つで宣言順（`s2-07l.297`）。
    #[test]
    fn model_parse_accepts_alias_and_display_exactly() {
        assert_eq!(Model::parse("opus"), Some(Model::Opus), "別名");
        assert_eq!(Model::parse("Opus"), Some(Model::Opus), "表示名");
        assert_eq!(Model::parse("opus"), Model::parse("Opus"), "2 つの語彙が同じ型に落ちる");
        assert_eq!(Model::parse("OPUS"), None, "case-fold しない");
        assert_eq!(Model::parse("claude-opus-5"), None, "model id は表に無い");
        assert_eq!(Model::parse(""), None);
        assert_eq!(MODELS.len(), 4, "閉じた表は 4 つ");
        assert!(is_declaration_order(MODELS, |model| model as usize));
        for model in MODELS {
            assert_eq!(Model::parse(model.alias()), Some(*model), "{}", model.alias());
            assert_eq!(Model::parse(model.display()), Some(*model), "{}", model.display());
            assert_ne!(model.alias(), model.display(), "2 つの語彙は別の字面");
        }
        assert_eq!(MODELS.iter().map(|model| model.alias()).collect::<Vec<_>>(), ["opus", "fable", "sonnet", "haiku"]);
        assert_eq!(MODELS.iter().map(|model| model.display()).collect::<Vec<_>>(), ["Opus", "Fable", "Sonnet", "Haiku"]);
    }

    /// 本番の組: `Input.model` は rules 行の**別名**（`opus`）・実測行の model は usage API の**表示名**（`Opus`）。
    /// `counts` が両側を型にして比べるので、Opus の窓 100 の口座は候補から外れ、Fable の窓 100 だけの口座は残る
    /// （run 1 の fixture は両側を `Opus` に揃えて隠していた）。表に無い model は保守側（全 model 窓の最大）。
    #[test]
    fn select_counts_the_runner_model_window_across_the_two_faces() {
        let rows = table(&[(TS, vec![
            // a1: Opus の窓が当たっている（5h / 7d は余裕）。
            measured("a1", WindowKind::FiveHour, None, 0, FIVE_RESET),
            measured("a1", WindowKind::SevenDay, None, 81, WEEK_RESET),
            measured("a1", WindowKind::SevenDayModel, Some("Opus"), 100, WEEK_RESET),
            // a2: Fable の窓だけが当たっている（便には無関係の窓）。
            measured("a2", WindowKind::FiveHour, None, 0, FIVE_RESET),
            measured("a2", WindowKind::SevenDay, None, 50, WEEK_RESET),
            measured("a2", WindowKind::SevenDayModel, Some("Fable"), 100, WEEK_RESET),
        ])]);
        let pair = ["a1", "a2"];
        assert_eq!(
            choose(&pair, &rows, &Ask { model: Some("opus"), ..run() }),
            chosen("a2"),
            "別名 × 表示名: Opus 100 の a1 は外れ・Fable 100 だけの a2 は残る"
        );
        assert_eq!(
            choose(&["a1"], &rows, &Ask { model: Some("opus"), ..run() }),
            none(NoCandidateReason::AllLimited, Some(WEEK_RESET)),
            "a1 の Opus の窓 100 を数える（字面が違っても同じ model）"
        );
        assert_eq!(choose(&pair, &rows, &Ask { model: Some("Opus"), ..run() }), chosen("a2"), "表示名で与えても同じ");
        assert_eq!(choose(&pair, &rows, &Ask { model: Some("fable"), ..run() }), chosen("a1"), "fable なら a2 が外れ a1 = 81");
        assert_eq!(
            choose(&pair, &rows, &run()),
            none(NoCandidateReason::AllLimited, Some(WEEK_RESET)),
            "model なしは全 model 窓の最大＝両方 100"
        );
        assert_eq!(
            choose(&pair, &rows, &Ask { model: Some("nope"), ..run() }),
            none(NoCandidateReason::AllLimited, Some(WEEK_RESET)),
            "表に無い model は保守側（model なしと同じ・fail-open にしない）"
        );
        let unknown_row = table(&[(TS, vec![
            measured("a3", WindowKind::FiveHour, None, 10, FIVE_RESET),
            measured("a3", WindowKind::SevenDayModel, Some("Nope"), 100, WEEK_RESET),
        ])]);
        assert_eq!(choose(&["a3"], &unknown_row, &Ask { model: Some("opus"), ..run() }), chosen("a3"), "表に無い表示名の行は別 model の窓＝数えない");
        assert_eq!(choose(&["a3"], &unknown_row, &run()), none(NoCandidateReason::AllLimited, Some(WEEK_RESET)), "model なしなら数える");
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

    /// 口座 1 つの振り方。`kind`: 0 = 実測なし・1 = 口座単位の Unmeasured・2 = 実測（5h・7d・Fable）。`soon` は
    /// **7 日窓**の reset を `WEEK_SOON_RESET`（早い方）にし、5 時間窓とモデル別窓の reset は**逆向き**に置く
    /// （鍵でない窓の reset が並びに入っていないことを振る・ADR-0042）・`inflight` は走行中の便数。
    #[derive(Debug, Clone)]
    struct Spec {
        kind: u8,
        five: u64,
        seven: u64,
        fable: u64,
        stale: bool,
        excluded: bool,
        soon: bool,
        inflight: usize,
    }

    fn spec() -> impl Strategy<Value = Spec> {
        (
            0_u8..3,
            0_u64..=120,
            0_u64..=120,
            0_u64..=120,
            prop::bool::weighted(0.2),
            prop::bool::weighted(0.2),
            prop::bool::ANY,
            0_usize..3,
        )
            .prop_map(|(kind, five, seven, fable, stale, excluded, soon, inflight)| Spec {
                kind,
                five,
                seven,
                fable,
                stale,
                excluded,
                soon,
                inflight,
            })
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
            let (five_reset, week_reset, model_reset) = match (spec.stale, spec.soon) {
                (true, _) => (PAST_RESET, PAST_RESET, PAST_RESET),
                (false, true) => (FIVE_RESET, WEEK_SOON_RESET, WEEK_RESET),
                (false, false) => (SOON_RESET, WEEK_RESET, WEEK_SOON_RESET),
            };
            match spec.kind {
                1 => rows.push(unmeasured(label, None)),
                2 => {
                    rows.push(measured(label, WindowKind::FiveHour, None, spec.five, five_reset));
                    rows.push(measured(label, WindowKind::SevenDay, None, spec.seven, week_reset));
                    rows.push(measured(label, WindowKind::SevenDayModel, Some("Fable"), spec.fable, model_reset));
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

    /// 選ばれた label の振り方。
    fn spec_of<'a>(specs: &'a [Spec], label: &str) -> Option<&'a Spec> {
        let at = POOL.iter().position(|found| *found == label)?;
        specs.get(at)
    }

    /// 選ばれた label の振り方の逼迫度。
    fn chosen_pressure(specs: &[Spec], label: &str, model: Option<&str>) -> Option<u64> {
        pressure(spec_of(specs, label)?, model)
    }

    /// 振り方の便用の並べ鍵（実測の候補だけ・**7 日窓**の reset＝`soon` なら `WEEK_SOON_RESET`・便数・label・
    /// ADR-0042）。実測の候補（`kind == 2` かつ古くない）は必ず 7 日窓の行を持つので `Option` にならない。
    fn run_key_of(spec: &Spec, label: &str) -> (&'static str, usize, String) {
        (if spec.soon { WEEK_SOON_RESET } else { WEEK_RESET }, spec.inflight, label.to_owned())
    }

    /// `order` の順に label を並べて選ぶ（除外と走行中の便数は振り方から組む）。
    fn evaluate(specs: &[Spec], order: &[usize], purpose: Purpose, model: Option<&str>, threshold: u64) -> Selection {
        let labels: Vec<String> = order.iter().filter_map(|at| POOL.get(*at)).map(|label| (*label).to_owned()).collect();
        let exclude: BTreeSet<String> = POOL
            .iter()
            .zip(specs)
            .filter(|(_, spec)| spec.excluded)
            .map(|(label, _)| (*label).to_owned())
            .collect();
        let inflight: BTreeMap<String, usize> =
            POOL.iter().zip(specs).map(|(label, spec)| ((*label).to_owned(), spec.inflight)).collect();
        let allowance = world(specs);
        select(&Input {
            labels: &labels,
            allowance: &allowance,
            purpose,
            model,
            exclude: &exclude,
            inflight: &inflight,
            threshold_pct: threshold,
            now: NOW,
            prefer: None,
        })
    }

    /// 同じ振り方を**走行中の便数を空**にして選ぶ（候補なしの理由と `earliest_reset` が便数に依らないことを測る対）。
    fn evaluate_without_inflight(specs: &[Spec], purpose: Purpose, model: Option<&str>, threshold: u64) -> Selection {
        let idle: Vec<Spec> = specs.iter().map(|spec| Spec { inflight: 0, ..spec.clone() }).collect();
        evaluate(&idle, &IDENTITY, purpose, model, threshold)
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

        /// (e) 便用の選択は候補の中で **7 日窓の reset が最小**で、同じ reset の候補の中で**走行中の便数が最少**
        /// （＝並べ鍵 (7 日窓の reset, 便数, label) がどの候補にも上回られない・ADR-0042）。候補なしの理由と
        /// `earliest_reset` は便数に依らない（便数を空にしても同じ `None(..)`）。
        #[test]
        fn prop_select_run_choice_has_the_earliest_reset_then_fewest_inflight(specs in specs(), model in models()) {
            let found = evaluate(&specs, &IDENTITY, Purpose::Run, model, THRESHOLD);
            match &found {
                Selection::Chosen(label) => {
                    let mine = spec_of(&specs, label).map(|spec| run_key_of(spec, label));
                    prop_assert!(mine.is_some());
                    for (other, spec) in POOL.iter().zip(&specs) {
                        if is_candidate(spec, Purpose::Run, model, THRESHOLD) {
                            prop_assert!(mine.as_ref() <= Some(&run_key_of(spec, other)));
                        }
                    }
                }
                Selection::None(_) => {
                    prop_assert_eq!(&found, &evaluate_without_inflight(&specs, Purpose::Run, model, THRESHOLD));
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
