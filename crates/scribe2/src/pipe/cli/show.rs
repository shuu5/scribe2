//! `pipe show`（設計 §5「subcommand の置き場」・`s2-07l.349` で `cli.rs` から純移動・本文は不変）。
//!
//! 便の段の 1 行と、gate の検出線の判定行（[`detection_lines`]・在る周だけ・段の秒を伴う）。

use super::intake::run_repo;
use super::{need, refused, state_dir_of};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::store::StoreError;
use crate::pipe::gate::{detection_copies, DetectionCopy};
use crate::pipe::{current, run_dir, worktree_path};
use std::path::{Path, PathBuf};

/// `pipe show`。1 行目は便の段、2 行目以降は gate の検出線の判定行（[`detection_lines`]・在る周だけ）。
pub(super) fn show(args: &[String]) -> Outcome {
    let id = match need(args, "--run") {
        Ok(found) => found.to_owned(),
        Err(reason) => return refused(reason),
    };
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()),
    };
    let Some(run) = state.runs.get(&id) else {
        return refused(format!("run {id} が無い"));
    };
    let repo = run_repo(args, &state_dir, &id).unwrap_or_else(|_| PathBuf::from("."));
    let mut lines = vec![format!(
        "run={} bead={} stage={} approved={} worktree={}",
        run.id,
        run.bead,
        run.stage.as_str(),
        run.approved,
        worktree_path(&repo, &id).display()
    )];
    lines.extend(detection_lines(&run_dir(&state_dir, &id)));
    Outcome::ok(lines)
}

/// gate の**周ごとの写し**（run dir・設計 gate-cost.md §15 (1)）の判定行を逐語で並べる（値の読み手）。
///
/// **出所は写しだけ**で `verify.jsonl` は読まない（判定行の出所を 2 つ持たない・C2・設計 §15 (3)）。
/// 無い周は空: 写しが無い（gate 前・検出線を撃たない便）。測り直しで gate を 2 周した便は写しが
/// 周ごとに在るので、周の数だけ並ぶ。判定行を写せていない周は不在の 1 行が出る（0 件と弁別・§15 (2)）。
fn detection_lines(dir: &Path) -> Vec<String> {
    detection_copies(dir).into_iter().map(shown_copy).collect()
}

/// 写し 1 つの行: 判定行の逐語 + **段の秒**（`secs=<秒>`・設計 gate-cost.md §26 形 (1)）。
///
/// 秒は写した値をそのまま出す（器は数え直さない）。秒の写しを持たない周（撃たなかった段・古い便）は
/// 判定行だけを出す——`secs=0` と書くと「測って 0 秒」に化ける（C10）。
fn shown_copy(copy: DetectionCopy) -> String {
    match copy.secs {
        Some(secs) => format!("{} secs={secs}", copy.line),
        None => copy.line,
    }
}
