//! `rules` subcommand の面（設計 §6）。
//!
//! stdout / stderr へは書かない。行を組んで [`Outcome`] で返し、出力は bin 側の
//! `emit` / `emit_err` が行う（出力層を 1 箇所に閉じるため）。

use super::manifest::Manifest;
use super::RuleError;
use crate::cli_outcome::{Outcome, RC_REFUSED};
use std::path::Path;

/// `rules` の使い方の行。
pub fn usage() -> String {
    "usage: rules <validate|get <id>> [--rules PATH] [--state-dir S]".to_owned()
}

/// `rules` に続く引数を捌く。`--state-dir S` が在れば host の面（`S/host.toml`）も同じ拒否形で読む。
pub fn dispatch(args: &[String]) -> Outcome {
    let resolved = state_dir(args).and_then(|dir| Ok((super::with_state_dir(open(args)?, dir)?, dir)));
    let (manifest, dir) = match resolved {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_REFUSED, render_defects(&errors)),
    };
    match args.first().map(String::as_str) {
        Some("validate") => validate(&manifest, dir),
        Some("get") => match args.get(1) {
            Some(id) if !id.starts_with("--") => get(&manifest, id),
            _ => Outcome::failed_line(RC_REFUSED, usage()),
        },
        _ => Outcome::failed_line(RC_REFUSED, usage()),
    }
}

/// 規則をどこから読むか。
pub(crate) enum Source<'a> {
    /// binary に埋め込んだ manifest。
    Embedded,
    /// `--rules` が指した file。
    File(&'a str),
    /// `--rules` は在るが PATH が無い。
    Incomplete,
}

/// `--rules PATH` が在れば file から、無ければ埋め込みから読む。
///
/// PATH の無い `--rules` を埋め込みへ倒さないのは、指定した規則で走っているつもりの
/// 呼出しが黙って別の規則で通るのを塞ぐためである（fail-closed・SRS NFR4）。
///
/// `seat` の `--rules` も**この 1 本**を通る（`s2-07l.151`）: 同じ flag の字面で別の解き方を
/// 持つと、`--rules` を渡した周に面ごとに違う規則で走る。
pub(crate) fn open(args: &[String]) -> Result<Manifest, Vec<RuleError>> {
    match source(args) {
        Source::Embedded => Manifest::embedded(),
        Source::File(path) => Manifest::load(Path::new(path)),
        Source::Incomplete => Err(vec![RuleError::new(
            0,
            "--rules に PATH が無い".to_owned(),
        )]),
    }
}

/// `--rules` の指定を読む。
pub(crate) fn source(args: &[String]) -> Source<'_> {
    let Some(at) = args.iter().position(|arg| arg == "--rules") else {
        return Source::Embedded;
    };
    match args.get(at + 1) {
        Some(path) if !path.starts_with("--") => Source::File(path),
        _ => Source::Incomplete,
    }
}

/// `--state-dir S` の指定を読む。無ければ `None`・PATH の無い `--state-dir` は `--rules` と同じく断る
/// （host の面を読んだつもりの呼出しを tracked の面だけで黙って通さない・NFR4）。
fn state_dir(args: &[String]) -> Result<Option<&Path>, Vec<RuleError>> {
    let Some(at) = args.iter().position(|arg| arg == "--state-dir") else {
        return Ok(None);
    };
    match args.get(at + 1) {
        Some(path) if !path.is_empty() && !path.starts_with("--") => Ok(Some(Path::new(path))),
        _ => Err(vec![RuleError::new(0, "--state-dir に PATH が無い".to_owned())]),
    }
}

/// 読み込みに成功した manifest の要約 1 行。`--state-dir` の周は宣言の数と host の面の在る / 無いを足す。
fn validate(manifest: &Manifest, state_dir: Option<&Path>) -> Outcome {
    let rows = manifest.rows();
    let kinds = super::ALL
        .iter()
        .filter(|kind| rows.iter().any(|row| row.kind == **kind))
        .count();
    let head = format!("rules: ok rows={} kinds={kinds}", rows.len());
    let Some(dir) = state_dir else {
        return Outcome::ok_line(head);
    };
    // 合わせ終えた後なので、file が在る＝読めた（在るが読めない周はここへ来ない）。
    let host = if super::host_manifest_path(dir).exists() { "present" } else { "absent" };
    Outcome::ok_line(format!(
        "{head} accounts={} plugins={} launch-args={} host={host}",
        manifest.accounts().len(),
        manifest.plugins().len(),
        manifest.launch_args().len()
    ))
}

/// 1 行の値を返す。無い行と不発効の行は rc 1 にする。
fn get(manifest: &Manifest, id: &str) -> Outcome {
    let Some(row) = manifest.get(id) else {
        return Outcome::failed_line(RC_REFUSED, "rules: no such id".to_owned());
    };
    if !row.enabled {
        return Outcome::failed_line(RC_REFUSED, format!("rules: disabled {id}"));
    }
    Outcome::ok_line(row.value.render())
}

/// error を 1 件 1 行へ写す（拒否 5 形・すべて `line=<N>` 付き・設計 rules-manifest.md §4.2）。
///
/// **描画はここと [`RuleError`] の `Display` だけ**である: `seat` の `--rules` も同じ行を返す
/// （§5「同じ拒否 5 形」・`s2-07l.154`）——面ごとに描画を持つと、同じ欠陥が面ごとに違う字で出る。
pub fn render_defects(errors: &[RuleError]) -> Vec<String> {
    errors.iter().map(RuleError::to_string).collect()
}
