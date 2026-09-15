//! tick の hook 集合の軸（設計 consumer-sync.md §6・ADR-0028 §2.4・SRS FR62 / AC32・`s2-07l.304`）。判定の順
//! （口座の軸の**後**・状態の門の**前**）は親（[`super::judge`]）が持つ。
//!
//! hook の集合は session 起動時の snapshot で、`/clear` は session を作り直さない＝hooks.json に event / matcher が
//! 増えた版は走行中の席に載らない（実測・auto-memory）。`.303` が SessionStart に書く読み込み元の記録
//! （[`PluginRecord`]・`root=<plugin root> hooks=<digest> binary=<sha>`）を読み、記録の root に**今**在る hooks.json の
//! digest（同じ FNV-1a 64・[`digest::hooks_digest`]）と比べる。違えば退避の合図（`kind=externalize origin=hook`・
//! payload の理由は `hook-drift`）を注入する——出所が hook の合図の後は終了の手（`/exit`）→ 立て直し（同じ target・
//! 閾値未満なら同じ口座）→ `/rebrief` の既存の経路（`.307`）で席が作り直され、新しい hook 集合が載る。
//!
//! **除外は FR29 と同じ**（自席の未 consumed 退避物が在る周・cycle lock が live な周は送らない）・**busy でも送る**
//! （context cap と同じ運び・queue の形で届く）・再送は [`super::signal_brake`]（`seat.signal_backoff_s`）。
//! **binary だけの食い違い**（hooks の digest が同じ）は注入しない（次の hook の起動で新しい本体が走る・rebrief の
//! DATA `[PLUGIN]` には載る）。記録が無い・読めない・root の hooks.json が無い / 読めない・digest が同じ周は注入せず
//! 次の条件へ（`NoopReason` を増やさない）。**測れないことを黙らせない**（C10）: 判定行に `plugin=<same|drift|
//! unrecorded|unreadable>` の token を 1 つ足す（登録 row の在る席だけ＝評価していない周は載らない）。
//! 閾値は無い（digest の一致 / 不一致だけ・rules 行を足さない・C1）。Guard ではない（inject / noop の判定・極性一覧に
//! 載せない）。

use super::render::{inject_line, Signal};
use super::{Account, InjectKind, Request, Seen, SignalOrigin, TickDecision, Verdict, EXTERNALIZE_SKILL};
use crate::hook::vessel::digest::{self, PluginRecord};
use crate::seat::{cycle, WmScan};
use std::path::Path;

/// 退避の合図の payload が名指す理由の語（AC32・記録から `hook-drift` で引ける）。
pub const REASON_HOOK_DRIFT: &str = "hook-drift";

/// hook 集合の軸の結果: 判定が決まった（退避の合図・合図の brake）か、読みを持って次の条件へ進むか。
pub(super) enum Axis {
    /// この周の判定が決まった。
    Settled(Verdict),
    /// 次の条件へ（読みは判定行の token に載る）。
    Pass(Plugin),
}

/// 読み込み元の記録と今の hooks.json の比較（closed・判定行の `plugin=` の値）。**bool で持たない**（憲法 C11）。
/// 登録 row の無い席・軸まで進まなかった周は [`Self::Unevaluated`]（`account=` と同じ「評価していない」の印＝
/// 判定行に載らない）。
#[derive(Clone)]
pub(super) enum Plugin {
    /// 評価していない。
    Unevaluated,
    /// 記録の digest と今の digest が同じ（binary だけの食い違いもここ）。
    Same,
    /// 違う（記録の digest, 今の digest）。
    Drift(String, String),
    /// 記録が無い（`Absent`・hook が `--plugin-root` を渡さない席・v1 の席）。**同じと読み替えない**。
    Unrecorded,
    /// 記録が読めない・記録の digest が `unreadable`・root の hooks.json が無い / 読めない。**無いと読み替えない**。
    Unreadable,
}

impl Plugin {
    /// 判定行の末尾に足す字面（`plugin=<…>`・評価していない周は空）。
    pub(super) fn suffix(&self) -> String {
        match self {
            Self::Unevaluated => String::new(),
            Self::Same => " plugin=same".to_owned(),
            Self::Drift(..) => " plugin=drift".to_owned(),
            Self::Unrecorded => " plugin=unrecorded".to_owned(),
            Self::Unreadable => " plugin=unreadable".to_owned(),
        }
    }
}

/// 席の打刻 dir の記録を読んで今の digest と比べる（判定の正本・語ごとの定義は [`Plugin`] の doc）。
fn read(seat_dir: &Path) -> Plugin {
    match PluginRecord::read(seat_dir) {
        PluginRecord::Absent => Plugin::Unrecorded,
        PluginRecord::Unreadable | PluginRecord::Recorded { hooks: None, .. } => Plugin::Unreadable,
        PluginRecord::Recorded { root, hooks: Some(recorded), .. } => match digest::hooks_digest(Path::new(&root)) {
            None => Plugin::Unreadable,
            Some(current) if current == recorded => Plugin::Same,
            Some(current) => Plugin::Drift(recorded, current),
        },
    }
}

/// 退避を促す 1 行（hook 集合が食い違った席へ）。理由の語（[`REASON_HOOK_DRIFT`]）と記録 / 今の digest を含め、
/// [`super::externalize_pointer`] と同じ理由で pane の幅に収まる短さに保ち、`--pointer` では上書きしない。
pub fn hook_pointer(recorded: &str, current: &str) -> String {
    format!("退避 tick: {REASON_HOOK_DRIFT} hooks {recorded}→{current}・{EXTERNALIZE_SKILL} で退避してください")
}

/// hook 集合の軸（口座の軸の直後＝状態の門の外）: 登録 row の在る席（口座の軸が評価した席）だけ評価する。記録を読んで
/// 今の digest と比べ（[`read`]）、違えば FR29 と同じ除外の下で idle を待たずに退避の合図を注入して自打刻する
/// （直近の合図から `seat.signal_backoff_s` 未満の周は [`super::signal_brake`] が `signal-recent` で止める）。同じ・
/// 記録が無い / 読めない・除外で注入しない周は読みを持って次の条件へ（注入も停止もしない）。
pub(super) fn plugin_turn(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen, account: &Account) -> Axis {
    if matches!(account, Account::Unevaluated) {
        return Axis::Pass(Plugin::Unevaluated);
    }
    let plugin = read(dir);
    let (recorded, current) = match &plugin {
        Plugin::Drift(recorded, current) if matches!(seen.wm, WmScan::None) && !cycle::lock_is_live(dir, seen.ttl_s) => {
            (recorded.clone(), current.clone())
        }
        _ => return Axis::Pass(plugin),
    };
    // 退避の合図の brake（context / 口座の軸と同じ 1 関数・`s2-07l.315`）: 直近の合図から back-off 未満の周は再送しない。
    if let Some(reason) = super::signal_brake(place, request.target, seen.backoff_s) {
        return Axis::Settled(Verdict { plugin, ..Verdict::of(TickDecision::Noop(reason)) });
    }
    let payload = hook_pointer(&recorded, &current);
    let signal = Signal {
        kind: InjectKind::Externalize,
        origin: Some(SignalOrigin::Hook),
        payload: &payload,
        state: seen.state,
    };
    Axis::Settled(Verdict { plugin, origin: signal.origin, ..Verdict::of(inject_line(request, place, dir, &signal)) })
}
