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
    "usage: rules <validate|get <id>> [--rules PATH]".to_owned()
}

/// `rules` に続く引数を捌く。
pub fn dispatch(args: &[String]) -> Outcome {
    let manifest = match open(args) {
        Ok(found) => found,
        Err(errors) => return Outcome::failed(RC_REFUSED, render(&errors)),
    };
    match args.first().map(String::as_str) {
        Some("validate") => validate(&manifest),
        Some("get") => match args.get(1) {
            Some(id) if !id.starts_with("--") => get(&manifest, id),
            _ => Outcome::failed_line(RC_REFUSED, usage()),
        },
        _ => Outcome::failed_line(RC_REFUSED, usage()),
    }
}

/// 規則をどこから読むか。
enum Source<'a> {
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
fn open(args: &[String]) -> Result<Manifest, Vec<RuleError>> {
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
fn source(args: &[String]) -> Source<'_> {
    let Some(at) = args.iter().position(|arg| arg == "--rules") else {
        return Source::Embedded;
    };
    match args.get(at + 1) {
        Some(path) if !path.starts_with("--") => Source::File(path),
        _ => Source::Incomplete,
    }
}

/// 読み込みに成功した manifest の要約 1 行。
fn validate(manifest: &Manifest) -> Outcome {
    let rows = manifest.rows();
    let kinds = super::ALL
        .iter()
        .filter(|kind| rows.iter().any(|row| row.kind == **kind))
        .count();
    Outcome::ok_line(format!("rules: ok rows={} kinds={kinds}", rows.len()))
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

/// error を 1 件 1 行へ写す。
fn render(errors: &[RuleError]) -> Vec<String> {
    errors.iter().map(RuleError::to_string).collect()
}
