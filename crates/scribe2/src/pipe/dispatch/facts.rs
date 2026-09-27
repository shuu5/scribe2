//! 並列の実測（live の本数・0 本の分数・重なりで待つ本数と file 名）を作る 1 関数と、その字面の 1 関数
//! （設計 docs/design/dispatcher.md §26・契約表の行 w）。
//!
//! 呼び手は 2 つ: 終端の周の idle の知らせ（`pipe/cli.rs` の `notices`・列の 1 周の結果を渡す）と、時計で撃つ
//! heartbeat（`crate::seat::tick`・列の結果を持たない）。**live の判定は 2 本目を書かない**（C2）: 生死は受付・列・
//! 終端の軸が共用する [`live`] を全便に撃つだけで、重なりは列の待ちの理由（[`WaitReason::Overlap`]）が既に持つ
//! 交差の file の列を読むだけである。事実は記帳しない（読むだけ）。
//!
//! 歯は e2e（`crates/scribe2-boundary/tests/e2e/notify.rs` の `pipe_notify_facts_`）が外形で測る——新設の module に
//! in-file の歯を置くと、base に `mod` 宣言ごと無く flip-check が `not-flippable` で断る。

use super::super::cli::live;
use super::super::current;
use super::super::refuse::normalize;
use super::{Turn, WaitReason};
use crate::fleet::epoch_of;
use std::collections::BTreeSet;
use std::path::Path;

/// 測れない値の字面（読めない判定・読めない ts・読めない台帳）。
const UNMEASURED: &str = "?";

/// 値の無い欄の字面（live が 1 本以上の周・便が 1 本も無い周の 0 本の分数）。
const ABSENT: &str = "-";

/// 1 つの事実の 3 形（**測れないと値なしを 0 に畳まない**・C10）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fact<T> {
    /// 測れた値。
    Value(T),
    /// その周には値が無い。
    Absent,
    /// 測れない。
    Unmeasured,
}

/// 重なりで待つ便の事実（列の 1 周の結果を持つ呼び手だけ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Held {
    /// 理由が今の `Overlap` の候補の本数。
    pub(crate) count: usize,
    /// その交差の file 名（path の最後の 1 要素・dir 項目は末尾の `/` を残す・重複なし・字の順）。
    pub(crate) names: Vec<String>,
}

/// 並列の実測の 3 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    /// live な便の本数（1 本でも生死を測れなければ測れない）。
    pub(crate) live: Fact<usize>,
    /// live が 0 本になってからの分（全便の `updated` の最大から今まで・切り捨て）。
    pub(crate) idle: Fact<u64>,
    /// 重なりで待つ便（列の結果の無い呼び手は `None`＝`held=` を出さない）。
    pub(crate) held: Option<Fact<Held>>,
}

/// 置き場・列の 1 周の結果（無い呼び手は `None`）・今の UTC 秒から並列の実測を作る（設計 §26 形 1）。
///
/// 置き場を読めない周は live も分数も測れない（0 本に読み替えない）。
pub(crate) fn facts(state_dir: &Path, turn: Option<&Turn>, now: u64) -> Facts {
    let held = turn.map(held_of);
    let Ok(state) = current(state_dir) else {
        return Facts { live: Fact::Unmeasured, idle: Fact::Unmeasured, held };
    };
    let mut count = 0;
    for (id, run) in &state.runs {
        match live(state_dir, id, run.stage) {
            Some(true) => count += 1,
            Some(false) => {}
            None => return Facts { live: Fact::Unmeasured, idle: Fact::Unmeasured, held },
        }
    }
    let idle = if count > 0 || state.runs.is_empty() {
        Fact::Absent
    } else {
        let stamps: Option<Vec<u64>> = state.runs.values().map(|run| epoch_of(&run.updated)).collect();
        match stamps.and_then(|found| found.into_iter().max()) {
            Some(last) => Fact::Value(now.saturating_sub(last) / 60),
            None => Fact::Unmeasured,
        }
    };
    Facts { live: Fact::Value(count), idle, held }
}

/// 列の結果の候補のうち理由が今の `Overlap` の本数と、その交差の file 名。台帳を読めなかった周は測れない。
fn held_of(turn: &Turn) -> Fact<Held> {
    if turn.unmeasured.is_some() {
        return Fact::Unmeasured;
    }
    let mut count = 0;
    let mut names: BTreeSet<String> = BTreeSet::new();
    for candidate in &turn.candidates {
        if let Some(WaitReason::Overlap { ref files, .. }) = candidate.reason {
            count += 1;
            names.extend(files.iter().map(|path| leaf(path)));
        }
    }
    Fact::Value(Held { count, names: names.into_iter().collect() })
}

/// path の最後の 1 要素（正規化は交差の照合と同じ 1 本＝`+` 等の接頭辞を剥がす・dir 項目は末尾の `/` を残す）。
fn leaf(path: &str) -> String {
    let normal = normalize(path);
    match normal.strip_suffix('/') {
        Some(dir) => format!("{}/", dir.rsplit('/').next().unwrap_or(dir)),
        None => normal.rsplit('/').next().unwrap_or(&normal).to_owned(),
    }
}

/// 知らせの末尾に足す字面（` live=<n> idle=<m>m held=<k>:<名,名>`・設計 §26 形 2）。
///
/// 測れない値は `?`、値なしは `-`、重なり 0 は `held=0`（コロンなし）、列の結果の無い呼び手は `held=` を出さない。
pub(crate) fn line(facts: &Facts) -> String {
    let live = match facts.live {
        Fact::Value(count) => count.to_string(),
        Fact::Absent => ABSENT.to_owned(),
        Fact::Unmeasured => UNMEASURED.to_owned(),
    };
    let idle = match facts.idle {
        Fact::Value(minutes) => format!("{minutes}m"),
        Fact::Absent => ABSENT.to_owned(),
        Fact::Unmeasured => UNMEASURED.to_owned(),
    };
    let held = match facts.held {
        None => String::new(),
        Some(Fact::Value(ref found)) if found.count == 0 => " held=0".to_owned(),
        Some(Fact::Value(ref found)) => format!(" held={}:{}", found.count, found.names.join(",")),
        Some(Fact::Absent) => format!(" held={ABSENT}"),
        Some(Fact::Unmeasured) => format!(" held={UNMEASURED}"),
    };
    format!(" live={live} idle={idle}{held}")
}
