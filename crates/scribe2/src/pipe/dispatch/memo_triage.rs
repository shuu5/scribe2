//! 起こす便が 0 の周に、引き金の満ちない memo を間隔と本数の内で裏の審査へ渡す選びと撃ち（設計 docs/design/dispatcher.md §42・契約表の行 aq・FR87 / FR68・ADR-0085）。
//!
//! 候補は行 ao の母集団（[`memo::population`]）で引き金が満ちず、局面の出力で memo-actionable でなく、置き場に死んだ持ち主でない `pid` が無く、
//! 前の判定の時刻（置き場の `verdict` の `at`・無ければ台帳の作られた時刻）から rules 行 [`ROW_INTERVAL`] の時間が過ぎた memo。
//! 前の判定の時刻の古い順（同じなら bead id の字の順）に、rules 行 [`ROW_PER_ROUND`] から全 memo の撃ち中の数を引いた本数まで、置き場の
//! `fired` に周の時刻を書いて `dispatch memo-lens <memo>` を裏に起こす。周は終わりを待たない。
//!
//! 歯は e2e（`crates/scribe2-boundary/tests/e2e/pipe/dispatch/waiting.rs` の `pipe_dispatch_memo_triage_`）が外形で測る——新設の module に
//! in-file の歯を置くと、base に `mod` 宣言ごと無く flip-check が断る。

use super::candidates::tools;
use super::{memo, spawn_self, Input, Launch, Read};
use crate::case::{Kind, Phase};
use crate::fleet::epoch_of;
use crate::fleet::lifecycle_read::{self, Lifecycle};
use crate::fleet::store::{lock_owner, started_ms, Owner};
use crate::rules::int_row;
use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::Path;

/// 審査の間隔（時間）の rules 行。
const ROW_INTERVAL: &str = "memo.triage_interval_h";

/// 1 周に渡す本数の rules 行。
const ROW_PER_ROUND: &str = "memo.triage_per_round";

/// 規則の行を読めず撃たなかった周の語。
const NO_RULE: &str = "no-rule";

/// 起こす便が 0 で台帳を読めた周に、候補を選んで撃つ。返りは規則の行を読めず撃たなかった周の語（撃った周・撃たない周は `None`）。
pub(super) fn round(input: &Input<'_>, launches: &[Launch], read: &Read) -> Option<&'static str> {
    if !launches.is_empty() || input.lens.is_none() {
        return None;
    }
    let (Ok(interval_h), Ok(per_round)) = (int_row(input.manifest, ROW_INTERVAL), int_row(input.manifest, ROW_PER_ROUND)) else {
        return Some(NO_RULE);
    };
    let now = crate::seat::state::now_secs();
    let room = per_round.saturating_sub(firing(input.state_dir));
    let mut due = candidates(input, read, now, interval_h.saturating_mul(3600));
    due.sort();
    for (_, memo) in due.into_iter().take(usize::try_from(room).unwrap_or(usize::MAX)) {
        fire(input, memo, now);
    }
    None
}

/// 候補の（前の判定の時刻・memo の id）。
fn candidates<'a>(input: &Input<'_>, read: &'a Read, now: u64, interval_s: u64) -> Vec<(u64, &'a str)> {
    let actionable = actionable(input);
    memo::population(input, read, now)
        .into_iter()
        .filter(|(id, trigger, _)| !memo::is_met(trigger) && !actionable.contains(*id) && !owned(&memo::dir(input.state_dir, id)))
        .filter_map(|(id, _, created)| judged_at(input.state_dir, id, created).map(|at| (at, id)))
        .filter(|(at, _)| now.saturating_sub(*at) >= interval_s)
        .collect()
}

/// 局面の出力で memo-actionable の memo の id（出力が無いか読めない周は空＝除かない・比べる組は空）。
fn actionable(input: &Input<'_>) -> BTreeSet<String> {
    match lifecycle_read::read(input.state_dir, input.repo, &[]) {
        Lifecycle::Read(found) => {
            found.parts.iter().filter(|part| part.part == Kind::Memo && part.phase == Phase::MemoActionable).map(|part| part.id.clone()).collect()
        }
        Lifecycle::Absent | Lifecycle::Unreadable => BTreeSet::new(),
    }
}

/// 前の判定の時刻（置き場の `verdict` の `at`・無ければ作られた時刻）。時刻の無い memo と読めない `verdict` の file は `None`。
fn judged_at(state_dir: &Path, memo: &str, created: Option<u64>) -> Option<u64> {
    match memo::judgement(state_dir, memo) {
        memo::Judgement::Absent => created,
        memo::Judgement::Unreadable => None,
        memo::Judgement::Judged(found) => epoch_of(&found.at),
    }
}

/// 置き場に撃ち中の印（死んだ持ち主でない `pid`）が在るか（印の無い置き場は無い・読めない印は撃ち中・`memo-lens` の `claim` と同じ読み）。
fn owned(place: &Path) -> bool {
    match std::fs::read_to_string(place.join(memo::PID)) {
        Ok(body) => lock_owner(&body, started_ms) != Owner::Dead,
        Err(error) => error.kind() != ErrorKind::NotFound,
    }
}

/// 全 memo の置き場の撃ち中の数。
fn firing(state_dir: &Path) -> u64 {
    let Ok(places) = std::fs::read_dir(memo::root(state_dir)) else {
        return 0;
    };
    let count = places.flatten().filter(|place| place.path().is_dir() && owned(&place.path())).count();
    u64::try_from(count).unwrap_or(u64::MAX)
}

/// 1 本を撃つ（置き場に `fired` を書いて `dispatch memo-lens <memo>` を裏に起こす・起こせなかった周は `fired` だけが残り次の周が撃ち直す）。
fn fire(input: &Input<'_>, memo: &str, now: u64) {
    let place = memo::dir(input.state_dir, memo);
    if std::fs::create_dir_all(&place).is_err() || std::fs::write(place.join(memo::FIRED), format!("{}\n", crate::fleet::cli::format_utc(now))).is_err() {
        return;
    }
    let mut argv: Vec<String> = ["dispatch", "memo-lens", memo, "--state-dir"].map(str::to_owned).into();
    argv.push(input.state_dir.display().to_string());
    argv.push("--repo".to_owned());
    argv.push(input.repo.display().to_string());
    argv.extend(tools(input));
    let _ = spawn_self(input.state_dir, &argv);
}
