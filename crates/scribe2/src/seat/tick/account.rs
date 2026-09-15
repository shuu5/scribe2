//! tick の口座の軸（登録 row の口座の逼迫度・定期計測・閾値以上の席への退避の合図・account-autonomy.md §3 / §5・
//! [`super`] から純移動・`s2-07l.279`）。判定の順（context の直後・状態の門の外）は親（[`super::judge`]）が持つ。

use super::exit::{exit_turn, parked_entry, relaunch_turn, Entry};
use super::render::{inject_line, Signal};
use super::{account_pointer, Account, InjectKind, Request, Seen, SignalOrigin, Verdict};
use crate::fleet::store;
use crate::fleet::{cli as fleet_cli, replay, Allowance, AllowanceLatest, Registration, State, WindowKind};
use crate::rules::manifest::Manifest;
use crate::seat::{cycle, role, state, WmScan};
use std::path::Path;

/// 口座の軸の結果: 判定が決まった（立て直し・退避の合図）か、逼迫度を持って次の条件へ進むか。
pub(super) enum Turn {
    /// この周の判定が決まった。
    Settled(Verdict),
    /// 次の条件へ（逼迫度は判定行と合図の brake が使う）。
    Pass(Account),
}

/// 口座の軸（account-autonomy.md §5・context の直後＝状態の門の外）: 登録 row の在る席だけ評価する。実測行が
/// 古い周は計測を 1 回撃ってから逼迫度を読み（[`seated`]）、退避して止まった席は前面が shell なら立て直し
/// （[`relaunch_turn`]）・shell でなく合図が口座由来（`origin=account`・`s2-07l.307`）なら終了の手（[`exit_turn`]・
/// [`parked_entry`]）、閾値以上の席へは FR29 と
/// 同じ除外の下で idle を待たずに退避の合図を注入して自打刻する（[`account_signal`]）。閾値未満・測れない・除外で
/// 注入しない周は逼迫度を持って次の条件へ（注入も停止もしない）。
pub(super) fn account_turn(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen) -> Turn {
    let Some(seated) = seated(request, place, seen.stale_s) else {
        return Turn::Pass(Account::Unevaluated);
    };
    let account = reading(&seated, seen.threshold);
    match parked_entry(&place.path, request.target, request.socket, dir, &seen.wm) {
        Entry::Relaunch => {
            return Turn::Settled(Verdict { account, ..relaunch_turn(request, place, dir, seen, &seated) });
        }
        Entry::Exit => return Turn::Settled(Verdict { account, ..exit_turn(request, place, dir, seen) }),
        Entry::None => {}
    }
    let Some(payload) = account_signal(&account, seen, dir) else {
        return Turn::Pass(account);
    };
    let signal = Signal {
        kind: InjectKind::Externalize,
        origin: Some(SignalOrigin::Account),
        payload: &payload,
        state: seen.state,
    };
    Turn::Settled(Verdict { account, origin: signal.origin, ..Verdict::of(inject_line(request, place, dir, &signal)) })
}

/// 閾値以上の席へ送る退避の合図の 1 行。**FR29 と同じ除外**（[`over_cap`] と同じ極性）: 自席の未 consumed
/// 退避物が 0 件と確かめられ、cycle lock が空いている周だけ（退避済み・作り直しの最中の席へ重ねない）。
fn account_signal(account: &Account, seen: &Seen, dir: &Path) -> Option<String> {
    match (account, &seen.wm) {
        (Account::Over(label, pct, threshold), WmScan::None) if !cycle::lock_is_live(dir, seen.ttl_s) => {
            Some(account_pointer(label, *pct, *threshold))
        }
        _ => None,
    }
}

/// 口座の軸の材料（登録 row と、実測行の鮮度を保った replay）。
pub(super) struct Seated {
    /// 自席の登録 row（口座・`model`・起動の雛形）。
    pub(super) row: Registration,
    /// replay の現在地（実測行と他の席の登録 row）。
    pub(super) state: State,
}

/// 登録 row を引き、その口座の最新の実測行が `seat.tick_stale_s` より古い・無い周は FR33 の計測を 1 回撃って
/// から読み直す（account-autonomy.md §5 (1)・定期計測はこの 1 形に限る・`fleet usage` と同じ関数）。log を
/// 読めない・登録 row が無い周は `None`（軸を評価しない）。計測の失敗は行として記録されるだけで止めない
/// （fleet-usage.md §6・FailOpen）——読み直した行が Unmeasured なら逼迫度は測れない側に倒れる。
///
/// manifest の `[[account]]` に無い口座は撃たない: 計測は宣言した口座だけを読むので行が積まれず、撃つと毎周の
/// 計測に化ける（測れないまま＝逼迫度は `unmeasured`）。宣言は tick が開いた rules の label 列 + 置き場の `host.toml`
/// で、計測にも同じ `--rules` と同じ置き場を渡す（tick と `fleet usage` が別の宣言を読まない・`s2-07l.224`）。
fn seated(request: &Request, place: &super::StateDir, stale_s: u64) -> Option<Seated> {
    let state = replay(&store::read_all(&place.path).ok()?);
    let row = role::registration_of_target(&state, request.target)?.clone();
    if is_fresh(&state, &row.account, stale_s) || !request.accounts.contains(&row.account) {
        return Some(Seated { row, state });
    }
    let args: Vec<String> = request
        .rules
        .map(|path| vec!["--rules".to_owned(), path.to_owned()])
        .unwrap_or_default();
    let _ = crate::fleet::usage::run(&args, &place.path);
    let state = store::read_all(&place.path).map_or(state, |events| replay(&events));
    Some(Seated { row, state })
}

/// 口座の最新の実測行（全窓・Measured / Unmeasured）の ts が `stale_s` 以内か（行が無い周は偽＝計測する）。
fn is_fresh(state: &State, label: &str, stale_s: u64) -> bool {
    let cutoff = fleet_cli::format_utc(state::now_secs().saturating_sub(stale_s));
    state
        .allowance
        .iter()
        .any(|(key, latest)| key.account == label && latest.ts >= cutoff)
}

/// 登録 row の口座の逼迫度を閾値で分ける（閾値ちょうどは以上＝R-C9-1 は「未満なら候補」の境界）。
fn reading(seated: &Seated, threshold: u64) -> Account {
    let label = seated.row.account.clone();
    match pressure(&seated.state, &label, seated.row.model.as_deref()) {
        None => Account::Unmeasured(label),
        Some(pct) if pct >= threshold => Account::Over(label, pct, threshold),
        Some(pct) => Account::Under(label, pct),
    }
}

/// 口座の逼迫度（account-autonomy.md §3 の定義・model = 登録 row の `model`〔無い row は None＝全 model 窓の最大の
/// 保守側〕）。窓の数え方は選定（[`crate::fleet::select`]）と同じ: 口座の最新の回のうち、数える窓に Unmeasured が
/// 在れば測れない・reset を過ぎた行は数えない（reset 無しの行は古くない実測として数える・ADR-0024 §2.2）・残った窓の
/// 最大の使用率。数える窓が 1 つも無ければ `None`。
fn pressure(state: &State, label: &str, model: Option<&str>) -> Option<u64> {
    let mine: Vec<&AllowanceLatest> = state
        .allowance
        .iter()
        .filter(|(key, _)| key.account == label)
        .map(|(_, latest)| latest)
        .collect();
    let newest = mine.iter().map(|latest| latest.ts.as_str()).max()?;
    let now = fleet_cli::now_utc();
    let mut found: Option<u64> = None;
    for latest in mine.iter().filter(|latest| latest.ts == newest) {
        match &latest.allowance {
            Allowance::Unmeasured(row) if counted(model, row.window, row.model.as_deref()) => return None,
            Allowance::Measured(row)
                if counted(model, Some(row.window), row.model.as_deref())
                    && row.resets_at.as_deref().is_none_or(|resets_at| resets_at >= now.as_str()) =>
            {
                found = found.max(Some(row.used_pct));
            }
            Allowance::Unmeasured(_) | Allowance::Measured(_) => {}
        }
    }
    found
}

/// その行を逼迫度に数えるか。model が与えられた周のモデル別窓はその model の行だけを数える（model の分からない行は
/// 保守側で数える・選定と同じ）。
fn counted(model: Option<&str>, window: Option<WindowKind>, row_model: Option<&str>) -> bool {
    match (window, model, row_model) {
        (Some(WindowKind::SevenDayModel), Some(want), Some(found)) => want == found,
        _ => true,
    }
}

/// 開いた manifest（tracked の面）の `[[account]]` の label 列（宣言値・`--rules` が在ればその file の宣言・
/// `s2-07l.224`）。host の面の宣言は [`run`] が置き場から足す。宣言が無い manifest は空＝選定は候補なし。
pub fn account_labels(manifest: &Manifest) -> Vec<String> {
    manifest.accounts().iter().map(|account| account.label().to_owned()).collect()
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.279
    use super::pressure;
    use crate::fleet::{Allowance, AllowanceLatest, Measured, State, Unmeasured, UnmeasuredReason, WindowKind};

    /// reset がどの「いま」より後の窓。
    const LATER: &str = "2099-01-01T00:00:00Z";
    /// reset を過ぎた窓。
    const PAST: &str = "2000-01-01T00:00:00Z";

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

    /// 回の列を物理順に replay したのと同じ表（同じ key は後が勝つ）。
    fn table(rounds: &[(&str, Vec<Allowance>)]) -> State {
        let mut state = State::default();
        for (ts, rows) in rounds {
            for row in rows {
                state.allowance.insert(row.key(), AllowanceLatest { ts: (*ts).to_owned(), allowance: row.clone() });
            }
        }
        state
    }

    /// 席の逼迫度は登録 row の model の窓だけを数え（無い row は全 model の最大）、最新の回だけを読み、reset を
    /// 過ぎた窓は数えず、数える窓が Unmeasured なら測れない（選定と同じ窓の数え方・account-autonomy.md §3）。
    #[test]
    fn seat_account_pressure_reads_the_seat_model_and_the_latest_round() {
        let ts = "2026-09-13T05:59:00Z";
        let rows = table(&[(ts, vec![
            measured("a1", WindowKind::FiveHour, None, 10, LATER),
            measured("a1", WindowKind::SevenDay, None, 12, LATER),
            measured("a1", WindowKind::SevenDayModel, Some("Fable"), 95, LATER),
            measured("a1", WindowKind::SevenDayModel, Some("Opus"), 20, LATER),
            measured("a2", WindowKind::FiveHour, None, 100, PAST),
            measured("a2", WindowKind::SevenDay, None, 40, LATER),
        ])]);
        assert_eq!(pressure(&rows, "a1", Some("Opus")), Some(20), "Opus の席は Fable の 95 を数えない");
        assert_eq!(pressure(&rows, "a1", Some("Fable")), Some(95));
        assert_eq!(pressure(&rows, "a1", None), Some(95), "model の無い row は全 model の最大（保守側）");
        assert_eq!(pressure(&rows, "a2", None), Some(40), "reset を過ぎた 100 は数えない");
        assert_eq!(pressure(&rows, "a3", None), None, "実測行なし");
        let later = table(&[
            (ts, vec![measured("a1", WindowKind::FiveHour, None, 10, LATER)]),
            ("2026-09-13T06:00:00Z", vec![Allowance::Unmeasured(Unmeasured {
                account: "a1".to_owned(),
                window: None,
                model: None,
                endpoint: "oauth-usage".to_owned(),
                reason: UnmeasuredReason::NoCredentials,
            })]),
        ]);
        assert_eq!(pressure(&later, "a1", None), None, "最新の回が Unmeasured なら前の回の実測を読まない");
        let stale = table(&[(ts, vec![measured("a1", WindowKind::FiveHour, None, 99, PAST)])]);
        assert_eq!(pressure(&stale, "a1", None), None, "reset を過ぎた行だけ＝測れない");
    }

    /// 消費の無い窓（0%・reset 無し）は古くない実測として数える＝逼迫度が `None` に倒れない（ADR-0024 §2.2）。
    #[test]
    fn seat_account_pressure_counts_idle_window_without_reset() {
        let ts = "2026-09-13T05:59:00Z";
        let idle = |window| {
            Allowance::Measured(Measured {
                account: "a1".to_owned(),
                window,
                model: None,
                endpoint: "oauth-usage".to_owned(),
                used_pct: 0,
                resets_at: None,
            })
        };
        let only_idle = table(&[(ts, vec![idle(WindowKind::FiveHour), idle(WindowKind::SevenDay)])]);
        assert_eq!(pressure(&only_idle, "a1", None), Some(0), "reset 無しだけでも測れた口座");
        let mixed = table(&[(ts, vec![idle(WindowKind::FiveHour), measured("a1", WindowKind::SevenDay, None, 30, LATER)])]);
        assert_eq!(pressure(&mixed, "a1", None), Some(30), "reset 無しの 0 は最大を動かさない");
    }

    /// Unmeasured 1 行（model 別窓）。
    fn unmeasured(account: &str, window: Option<WindowKind>, model: Option<&str>) -> Allowance {
        Allowance::Unmeasured(Unmeasured {
            account: account.to_owned(),
            window,
            model: model.map(str::to_owned),
            endpoint: "oauth-usage".to_owned(),
            reason: UnmeasuredReason::ShapeMismatch,
        })
    }

    /// 席の model と**違う** model の SevenDayModel 窓だけが Unmeasured なら、その行は数えない＝測れた口座
    /// （Unmeasured 腕の guard を `true` に落とす: 落とすと None に化ける）。
    // flip-check: retroactive s2-07l.232
    #[test]
    fn mutant_in_seat_account_pressure_ignores_unmeasured_window_of_another_model() {
        let ts = "2026-09-13T05:59:00Z";
        let rows = table(&[(ts, vec![
            measured("a1", WindowKind::FiveHour, None, 10, LATER),
            measured("a1", WindowKind::SevenDayModel, Some("Opus"), 20, LATER),
            unmeasured("a1", Some(WindowKind::SevenDayModel), Some("Fable")),
        ])]);
        assert_eq!(pressure(&rows, "a1", Some("Opus")), Some(20), "Fable 窓の Unmeasured は Opus の席に効かない");
    }

    /// 数える窓（five_hour・席の model と一致する SevenDayModel）が Unmeasured なら測れない
    /// （Unmeasured 腕の guard を `false` に落とす: 落とすと測れたことにする）。
    // flip-check: retroactive s2-07l.232
    #[test]
    fn mutant_in_seat_account_pressure_is_unmeasured_when_a_counted_window_is_unmeasured() {
        let ts = "2026-09-13T05:59:00Z";
        let five_hour = table(&[(ts, vec![
            unmeasured("a1", Some(WindowKind::FiveHour), None),
            measured("a1", WindowKind::SevenDayModel, Some("Opus"), 20, LATER),
        ])]);
        assert_eq!(pressure(&five_hour, "a1", Some("Opus")), None, "five_hour の Unmeasured は席の model に関係なく数える");
        let same_model = table(&[(ts, vec![
            measured("a1", WindowKind::FiveHour, None, 10, LATER),
            unmeasured("a1", Some(WindowKind::SevenDayModel), Some("Opus")),
        ])]);
        assert_eq!(pressure(&same_model, "a1", Some("Opus")), None, "席の model と同じ SevenDayModel 窓の Unmeasured");
        assert_eq!(pressure(&same_model, "a1", None), None, "model の無い row は全 model 窓を数える（保守側）");
    }
}
