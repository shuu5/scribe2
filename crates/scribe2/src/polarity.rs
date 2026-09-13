//! 全 guard の極性を型で持ち、一覧を外形として描く（設計 docs/design/polarity.md・ADR-0014・
//! 憲法 C11.2 / C16.2 / C12.5 / C2・SRS FR20 / FR26 / AC3）。
//!
//! guard は「行為（編集・起動・merge・書込・session の作り直し）を止めうる判定を返す境界」で、
//! 2 軸の極性を持つ: **いつ止めるか**（[`Timing`]）と**測れない周にどちらへ倒れるか**
//! （[`OnFailure`]）。値は**境界が持つ**（C11.2「境界ごとの enum が極性型を運ぶ」）——各 guard の
//! module が自分の判定 enum の隣に `pub const POLARITY: Polarity` を置き、ここはそれを**集める
//! だけ**で値を持たない（2 面化しない）。
//!
//! [`Guard`] は閉じた enum で、全 variant の const slice [`ALL`]・網羅 match・判別子順 pin の
//! 4 つ組（ADR-0013 §2.2）で持つ。境界を足す便は variant を足し、`ALL` に並べ、境界に
//! `POLARITY` を置く（入れ忘れは網羅 match と `enum-slices` の measure が落とす）。
//!
//! 一覧の生成物は `<NAME> polarity` の全出力を pin した tracked snapshot（C11.2「build 時に
//! 生成」の充足形・ADR-0014 §2.2）で、C16.2 の門は `cargo xtask check` がその snapshot の
//! 集計行を読む（§2.3）。**FailOpen の境界は隠さない**——cap guard は「測れない周は deny
//! しない」（FR26）と設計で決めた FailOpen で、一覧はそれをそのまま出す。

/// いつ止めるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    /// 行為の時点で止める（C16「edit time」）。
    InLoop,
    /// 行為の後に測って落とす。
    PostHoc,
}

impl Timing {
    /// 一覧の行に出す語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InLoop => "in-loop",
            Self::PostHoc => "post-hoc",
        }
    }
}

/// 測れない・読めない周にどちらへ倒れるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnFailure {
    /// 止める側へ倒す。
    FailClosed,
    /// 通す側へ倒し、記録を残す。
    FailOpen,
}

impl OnFailure {
    /// 一覧の行に出す語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FailClosed => "fail-closed",
            Self::FailOpen => "fail-open",
        }
    }
}

/// 2 軸の対。境界が定数として持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Polarity {
    /// いつ止めるか。
    pub timing: Timing,
    /// 測れない周の倒れ方。
    pub on_failure: OnFailure,
}

/// 行為を止めうる判定を返す境界の全数。**宣言順は行為の流れ**（hook → intake → spawn〔予算・承認〕→
/// runner → gate → land〔main 実測・anchor 同期・worktree の clean・追随の起こし直し〕→ store → 注入 → cycle → 退避 → 消費）で、順序に意味は無いが C2 の形（[`ALL`] と判別子順 pin）に合わせる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    /// `pre-tool-use` の write-set guard（[`crate::hook::guard`]）。
    WriteSet,
    /// 内蔵 guard の承認の問いへの一律 deny（[`crate::hook::permission`]）。
    Permission,
    /// 席の context 上限の cap guard（[`crate::hook::seat_guard`]・FailOpen）。
    Cap,
    /// intake の断り＝vessel 宣言の verify 行の不適合（[`crate::pipe::declaration`]）。
    Intake,
    /// intake の排他＝live な便と write-set が交差する契約を受け付けない（[`crate::pipe::refuse`]）。
    IntakeRefuse,
    /// spawn の予算＝実測を経ずに起動できない口（[`crate::pipe::Budget`]）。
    Budget,
    /// A1 の承認関門＝3 クラスを名乗る契約を承認 event 無しに起動しない（[`crate::pipe::approve`]）。
    Approval,
    /// runner の上限 record による便の中断（[`crate::headless::runner::Decision`]・FailOpen）。
    RunnerStop,
    /// runner の包みが最終行の質問 record で便を `Questioned` へ倒す判定（[`crate::headless::runner::Ending`]・FailOpen）。
    RunnerQuestion,
    /// gate の機械検証の段（[`crate::pipe::gate::Check`]）。
    GateCheck,
    /// gate の lens 1 本の判定（[`crate::pipe::gate::Verdict`]）。
    GateLens,
    /// land の main 実測（`pipe::land::MainCheck`）。
    LandMain,
    /// land の後に anchor を新 main へ揃えるかの見立て（`pipe::land::AnchorPlan`）。
    LandAnchor,
    /// 便の worktree が clean か＝rebase（`.119`）と retire の move の前提（[`crate::pipe::land::WorktreeCheck`]）。
    LandWorktree,
    /// 追随が衝突した便を起こし直す回数の上限（[`crate::pipe::follow::FollowCheck`]）。
    FollowRetry,
    /// event log の書込 lock（[`crate::fleet::store`]）。
    StoreLock,
    /// tmux pane への注入の断り＝入力欄が非空なら 1 key も送らない（[`crate::seat::inject`]）。
    Inject,
    /// session を作り直す口の断り（[`crate::seat::cycle`]）。
    Cycle,
    /// 退避物の書込を止める判定＝二重退避・上限超過・文法の断り（[`crate::seat::externalize`]）。
    Externalize,
    /// 退避物の消費（move）を止める判定＝曖昧・移し先の既在（[`crate::seat::consume`]）。
    Consume,
}

/// [`Guard`] の全 variant（宣言順）。
pub const ALL: &[Guard] = &[
    Guard::WriteSet,
    Guard::Permission,
    Guard::Cap,
    Guard::Intake,
    Guard::IntakeRefuse,
    Guard::Budget,
    Guard::Approval,
    Guard::RunnerStop,
    Guard::RunnerQuestion,
    Guard::GateCheck,
    Guard::GateLens,
    Guard::LandMain,
    Guard::LandAnchor,
    Guard::LandWorktree,
    Guard::FollowRetry,
    Guard::StoreLock,
    Guard::Inject,
    Guard::Cycle,
    Guard::Externalize,
    Guard::Consume,
];

impl Guard {
    /// 境界が持つ極性を返す**だけ**（一覧の側に値を書かない）。
    pub fn polarity(self) -> Polarity {
        match self {
            Self::WriteSet => crate::hook::guard::POLARITY,
            Self::Permission => crate::hook::permission::POLARITY,
            Self::Cap => crate::hook::seat_guard::POLARITY,
            Self::Intake => crate::pipe::declaration::POLARITY,
            Self::IntakeRefuse => crate::pipe::refuse::POLARITY,
            Self::Budget => crate::pipe::BUDGET_POLARITY,
            Self::Approval => crate::pipe::approve::POLARITY,
            Self::RunnerStop => crate::headless::runner::POLARITY,
            Self::RunnerQuestion => crate::headless::runner::QUESTION_POLARITY,
            Self::GateCheck => crate::pipe::gate::POLARITY,
            Self::GateLens => crate::pipe::gate::LENS_POLARITY,
            Self::LandMain => crate::pipe::land::POLARITY,
            Self::LandAnchor => crate::pipe::land::ANCHOR_POLARITY,
            Self::LandWorktree => crate::pipe::land::WORKTREE_POLARITY,
            Self::FollowRetry => crate::pipe::follow::POLARITY,
            Self::StoreLock => crate::fleet::store::POLARITY,
            Self::Inject => crate::seat::inject::POLARITY,
            Self::Cycle => crate::seat::cycle::POLARITY,
            Self::Externalize => crate::seat::externalize::POLARITY,
            Self::Consume => crate::seat::consume::POLARITY,
        }
    }

    /// 境界の pointer（`module::Type`・crate 相対）。
    pub fn boundary(self) -> &'static str {
        match self {
            Self::WriteSet => "hook::guard::Decision",
            Self::Permission => "hook::permission::PermissionDecision",
            Self::Cap => "hook::seat_guard::SeatDecision",
            Self::Intake => "pipe::declaration::Unfit",
            Self::IntakeRefuse => "pipe::refuse::Refuse",
            Self::Budget => "pipe::Budget",
            Self::Approval => "pipe::approve::Approval",
            Self::RunnerStop => "headless::runner::Decision",
            Self::RunnerQuestion => "headless::runner::Ending",
            Self::GateCheck => "pipe::gate::Check",
            Self::GateLens => "pipe::gate::Verdict",
            Self::LandMain => "pipe::land::MainCheck",
            Self::LandAnchor => "pipe::land::AnchorPlan",
            Self::LandWorktree => "pipe::land::WorktreeCheck",
            Self::FollowRetry => "pipe::follow::FollowCheck",
            Self::StoreLock => "fleet::store::StoreError",
            Self::Inject => "seat::inject::Delivery",
            Self::Cycle => "seat::cycle::Cycle",
            Self::Externalize => "seat::externalize::ExternalizeError",
            Self::Consume => "seat::consume::ConsumeError",
        }
    }

    /// 一覧の行に出す名前（kebab）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WriteSet => "write-set-guard",
            Self::Permission => "permission-deny",
            Self::Cap => "cap-guard",
            Self::Intake => "intake-unfit",
            Self::IntakeRefuse => "intake-refuse",
            Self::Budget => "spawn-budget",
            Self::Approval => "approval-gate",
            Self::RunnerStop => "runner-stop",
            Self::RunnerQuestion => "runner-question",
            Self::GateCheck => "gate-check",
            Self::GateLens => "gate-lens",
            Self::LandMain => "land-main-check",
            Self::LandAnchor => "land-anchor-sync",
            Self::LandWorktree => "land-worktree-clean",
            Self::FollowRetry => "follow-retry",
            Self::StoreLock => "store-lock",
            Self::Inject => "inject-refusal",
            Self::Cycle => "cycle-refusal",
            Self::Externalize => "externalize-refusal",
            Self::Consume => "consume-refusal",
        }
    }

    /// 一覧の 1 行（設計 §4 の形・token の名前と順序は設計が正）。
    pub fn line(self) -> String {
        let polarity = self.polarity();
        format!(
            "guard={} timing={} on-failure={} boundary={}",
            self.as_str(),
            polarity.timing.as_str(),
            polarity.on_failure.as_str(),
            self.boundary()
        )
    }
}

/// 集計行（`polarity: guards=<N> in-loop=<K> post-hoc=<M> fail-open=<F>`・N = K + M）。
pub fn summary() -> String {
    let in_loop = ALL.iter().filter(|g| g.polarity().timing == Timing::InLoop).count();
    let post_hoc = ALL.iter().filter(|g| g.polarity().timing == Timing::PostHoc).count();
    let fail_open = ALL.iter().filter(|g| g.polarity().on_failure == OnFailure::FailOpen).count();
    format!(
        "polarity: guards={} in-loop={in_loop} post-hoc={post_hoc} fail-open={fail_open}",
        ALL.len()
    )
}

/// `<NAME> polarity` の全出力（1 guard 1 行 + 集計 1 行）。引数も stdin も env も読まない。
pub fn render() -> Vec<String> {
    let mut lines: Vec<String> = ALL.iter().map(|g| g.line()).collect();
    lines.push(summary());
    lines
}
