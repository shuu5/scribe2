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
    /// 内訳（設計 account-lifecycle.md §25 形 1）: 除外集合に在る口座の label（宣言順・畳む前の値）。
    pub excluded: Vec<String>,
    /// 測れない口座（実測行なし・Unmeasured・reset 過ぎ）の label（宣言順）。
    pub unmeasured: Vec<String>,
    /// 当たっている口座の label（宣言順）。閾値以上の口座（session 用）はどの列にも入らない。
    pub limited: Vec<String>,
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
    let mut outs: Vec<(&str, NoCandidateReason, Option<String>)> = Vec::new();
    for label in input.labels {
        match standing(input, label) {
            Standing::Candidate(found) => candidates.push(found),
            Standing::Out(reason, reopens) => outs.push((label.as_str(), reason, reopens)),
        }
    }
    match pick(input.purpose, input.prefer, &candidates) {
        Some(label) => Selection::Chosen(label.to_owned()),
        None => Selection::None(NoCandidate {
            reason: outs
                .iter()
                .map(|(_, reason, _)| *reason)
                .min()
                .unwrap_or(NoCandidateReason::Unmeasured),
            excluded: labels_of(&outs, NoCandidateReason::Excluded),
            unmeasured: labels_of(&outs, NoCandidateReason::Unmeasured),
            limited: labels_of(&outs, NoCandidateReason::AllLimited),
            earliest_reset: outs.into_iter().filter_map(|(_, _, reopens)| reopens).min(),
        }),
    }
}

/// 外れた口座のうち理由が `reason` の label（宣言順・1 口座の理由は 1 つなので 1 列にだけ入る）。
fn labels_of(outs: &[(&str, NoCandidateReason, Option<String>)], reason: NoCandidateReason) -> Vec<String> {
    outs.iter().filter(|(_, found, _)| *found == reason).map(|(label, _, _)| (*label).to_owned()).collect()
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
pub(crate) fn counts(model: Option<&str>, window: Option<WindowKind>, row_model: Option<&str>) -> bool {
    match (window, model, row_model) {
        (Some(WindowKind::SevenDayModel), Some(want), Some(found)) => match Model::parse(want) {
            Some(want) => Model::parse(found) == Some(want),
            None => true,
        },
        _ => true,
    }
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
// flip-check: moved s2-07l.460
