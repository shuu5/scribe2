//! 門が通した周の古さの印（設計 docs/design/case-lifecycle.md §14・行 e・FR90 / AC60・ADR-0088）: PreToolUse の Bash の道が最後に
//! allow と決めた周の command が、台帳の書き（subcommand が [`WRITES`] に在る片）か `gh pr merge` の片を持てば、局面の出力
//! （`lifecycle.json`）へ「古い」の印を足す。**書き直さない**（台帳も git も撃たない・読むのは印の file と ref の file だけ・NFR5）。
//!
//! 印の値は行 c の読み手が返す: 台帳は書きの前の印（[`read_ledger`]）・main は sha（[`read_main`]・git を撃たない）。出力の無い置き場・
//! 値を読めない周・`lifecycle.stale.lock` を取れない周・`lifecycle.stale` が読めない周は **allow を変えずに**付けない（fail-open）。
//! 片の割りと読みは起票の門の [`segments`] / [`write_of`]、`gh pr merge` の見分けは anchor の門の [`is_pr_merge`]（2 本目を書かない）。

use super::anchor_guard::is_pr_merge;
use super::ledger_guard::{segments, write_of, WRITES};
use crate::fleet::cli::format_utc;
use crate::fleet::lifecycle_mark::{
    add_mark, fleet_dir, read_ledger, read_main, Kind, Mark, Value, JSON_FILE,
};
use crate::fleet::store::LockPolicy;
use crate::seat::state::now_secs;
use std::path::Path;

/// `lifecycle.stale.lock` を取り直す上限（ミリ秒）。hook の予算（NFR5）を食わない短さで、取れなければ印を付けずに通す。
const LOCK_WAIT_MS: u64 = 200;

/// command が通った周に、持つ片に応じて古さの印を足す（印は種類ごとに 1 つ・後の印が置き換え）。
///
/// `root` は台帳と main を読む repo（anchor）・`state_dir` は出力の置き場。足せない周（出力が無い・値を読めない・lock を取れない・
/// 印の file が読めない）は何も書かず返る＝呼び手の allow は変わらない。
pub fn mark(command: &str, root: &Path, state_dir: &Path) {
    let parts = segments(command);
    let ledger = parts.iter().any(|words| write_of(words).is_some_and(|found| WRITES.contains(&found.subcommand.as_str())));
    let merge = parts.iter().any(|words| is_pr_merge(words));
    if !(ledger || merge) || !fleet_dir(state_dir).join(JSON_FILE).is_file() {
        return;
    }
    let Ok(embedded) = LockPolicy::embedded() else {
        return;
    };
    let policy = LockPolicy { retry_ms: LOCK_WAIT_MS, ..embedded };
    let at = format_utc(now_secs());
    let put = |kind: Kind, value: Option<Value>| {
        if let Some(value) = value {
            let _ = add_mark(state_dir, &Mark { kind, at: at.clone(), value }, policy);
        }
    };
    if ledger {
        put(Kind::LedgerGate, read_ledger(root).map(Value::Ledger));
    }
    if merge {
        put(Kind::MergeGate, read_main(root).map(Value::Main));
    }
}
