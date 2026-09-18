//! `pipe show`（設計 §5「subcommand の置き場」・`s2-07l.349` で `cli.rs` から純移動・本文は不変）。
//!
//! 便の段の 1 行と、gate の検出線の判定行（[`detection_lines`]・在る周だけ・段の秒を伴う）。

use super::intake::run_repo;
use super::{need, refused, state_dir_of};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::json_lite;
use crate::fleet::store::StoreError;
use crate::pipe::gate::Check;
use crate::pipe::{current, verify_log_path, worktree_path};
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
    lines.extend(detection_lines(&verify_log_path(&state_dir, &id)));
    Outcome::ok(lines)
}

/// gate の `verify.jsonl` の **検出線の record の `line=`** を逐語で並べる（設計 gate-cost.md §5.1・
/// 値の読み手）。他の kind の `line`（flip-check の判定行など）は出さない＝record を読む。
///
/// 無い周は空: file が無い（gate 前）・record に `line` が無い（stdout の無い道具・省いた周）。
/// 測り直しで gate を 2 周した便は record が追記されているので、周の数だけ並ぶ。
fn detection_lines(path: &Path) -> Vec<String> {
    let detection = Check::Detection.as_str();
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|text| json_lite::parse_object(text.trim()).ok())
        .filter(|pairs| field(pairs, "kind") == Some(detection))
        .filter_map(|pairs| shown_record(&pairs))
        .collect()
}

/// record 1 本の行: 判定行の逐語 + **段の秒**（`secs=<秒>`・設計 gate-cost.md §26 形 (1)）。
///
/// 秒は record の値をそのまま写す（器は数え直さない）。`secs` を欠く record（撃たなかった段・
/// 古い便の `verify.jsonl`）は判定行だけを出す——`secs=0` と書くと「測って 0 秒」に化ける（C10）。
fn shown_record(pairs: &[(String, json_lite::Value)]) -> Option<String> {
    let line = field(pairs, "line")?;
    match num(pairs, "secs") {
        Some(secs) => Some(format!("{line} secs={secs}")),
        None => Some(line.to_owned()),
    }
}

/// flat object の文字列 field。
fn field<'a>(pairs: &'a [(String, json_lite::Value)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(found, _)| found == key).and_then(|(_, value)| value.as_str())
}

/// flat object の数 field。
fn num(pairs: &[(String, json_lite::Value)], key: &str) -> Option<u64> {
    pairs.iter().find(|(found, _)| found == key).and_then(|(_, value)| value.as_num())
}
