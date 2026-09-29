//! park の区画の席の移動の判定（設計 docs/design/seat-heartbeat.md §21 形 1・契約表の行 z・FR95・ADR-0091）。
//!
//! 区画（Tier9）の席は群と違って「今の口座の記録」を持たない。管理 tick が周ごとに、席の登録 row の口座が session 用の閾値
//! （R-C9-1）以上かを見て、以上の周だけ区画の行に並べた口座から次の口座を選ぶ（[`judge`]）。**読むだけ**で、計測・lock・file の
//! 書き込みは 0（鮮度の内側の最新の回だけを読む）。結果は閉じた 6 値の [`Park`] で、判定行の語と 1 周の結果への写しは親の側に置く
//! （この file は tick の判定の列の値を名指さない＝他の行の touches の閉包を広げない）。

use crate::fleet::select::{select, Input as SelectInput, NoCandidateReason, Purpose, Selection};
use crate::fleet::usage::fresh_rows;
use crate::fleet::{RegistrationLatest, State};
use crate::rules::manifest::Manifest;
use crate::seat::cycle::choose;
use crate::seat::{int_rule_of, ID_THRESHOLD};
use std::collections::{BTreeMap, BTreeSet};

/// 区画の判定の結果（**閉じた 6 値**・形 1 の (a)〜(e) の順に最初に立った値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Park {
    /// (a) anchor が区画の anchors に無い。
    Outside,
    /// (b) R-C9-1 か `fleet.usage_fresh_s` を読めない。
    NoRule,
    /// (c) row の口座の鮮度の内側の記録が無い（記録なしか鮮度の外）。選定が数える窓を持たない周もここ。
    Unmeasured,
    /// (d) row の口座が閾値未満。
    Stay,
    /// (e) 移り先の口座。
    To(String),
    /// (e) 移り先が無い（候補 0 を含む）。
    NoCandidate,
}

/// target の最新の登録 row（`seq` つき・複数の鍵が同じ target なら `seq` の大きい方）。
pub(super) fn latest_of<'a>(fleet: &'a State, target: &str) -> Option<&'a RegistrationLatest> {
    fleet.registrations.values().filter(|latest| latest.registration.target == target).max_by_key(|latest| latest.seq)
}

/// 区画の席の移動の鍵（設計 §21 形 2）: （row の口座, `park.<登録 row の seq>`）。移り先が周ごとに変わっても、row の口座と登録が
/// 同じ間は同じ移動で、退避の合図を重ねない。
pub(super) fn key(seat: &RegistrationLatest) -> (String, String) {
    (seat.registration.account.clone(), format!("park.{}", seat.seq))
}

/// 区画の判定の 1 関数（面を合わせた `manifest`・replay `fleet`・自席の登録 row `seat`・今の UTC `now`）。読むだけ。
pub(super) fn judge(manifest: &Manifest, fleet: &State, seat: &RegistrationLatest, now: &str) -> Park {
    let row = &seat.registration;
    let Some(lot) = manifest.park().filter(|lot| lot.anchors().contains(&row.anchor)) else {
        return Park::Outside;
    };
    let Ok(threshold_pct) = int_rule_of(manifest, ID_THRESHOLD) else {
        return Park::NoRule;
    };
    match fresh_rows(manifest, fleet, &row.account) {
        Err(_) => return Park::NoRule,
        Ok(None) => return Park::Unmeasured,
        Ok(Some(_)) => {}
    }
    let (labels, exclude, inflight) = ([row.account.clone()], BTreeSet::new(), BTreeMap::new());
    let (allowance, purpose, model) = (&fleet.allowance, Purpose::Session, row.model.as_deref());
    let input = SelectInput { labels: &labels, allowance, purpose, model, exclude: &exclude, inflight: &inflight, threshold_pct, now, prefer: None };
    match select(&input) {
        Selection::Chosen(_) => return Park::Stay,
        Selection::None(found) if found.reason == NoCandidateReason::Unmeasured => return Park::Unmeasured,
        Selection::None(_) => {}
    }
    let candidates: Vec<String> = lot
        .accounts()
        .iter()
        .filter(|label| **label != row.account && !fleet.retired.contains_key(*label))
        .filter(|label| matches!(fresh_rows(manifest, fleet, label), Ok(Some(_))))
        .cloned()
        .collect();
    let anchors: BTreeSet<String> = lot.anchors().iter().cloned().collect();
    match choose((row.role, &row.anchor, None), fleet, (&candidates, &anchors), model, threshold_pct) {
        Selection::Chosen(label) => Park::To(label),
        Selection::None(_) => Park::NoCandidate,
    }
}
