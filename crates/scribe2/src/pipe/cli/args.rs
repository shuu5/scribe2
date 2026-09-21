//! 引数と規則の行の helper（設計 §5「subcommand の置き場」・`s2-07l.349` で `cli.rs` から純移動・本文は不変）。
//!
//! flag の読み（[`flag`] / [`need`]）・規則の値（[`manifest_of`] / [`int_row`] / [`list_row`]）・置き場と repo の
//! 解き（[`state_dir_of`] / [`repo_of`]）・断りの 2 形（[`refused`] = rc 1 / [`broken`] = rc 2）。外から呼ぶ path は
//! `cli` の再輸出で不変（`super::flag` 等）。

use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::hook::vessel;
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::path::{Path, PathBuf};

/// `--<name> <値>` を読む。値欠けは黙って落とさず `Err`（SRS NFR4）。
///
/// 器の中で 3 本目の flag reader である。4 本目が要るときは 1 本へ畳む
/// （いまは fleet / vessel / pipe がそれぞれ自分の必須 flag だけを見ている）。
pub(in crate::pipe) fn flag<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, String> {
    let Some(at) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    match args.get(at + 1) {
        Some(found) if !found.starts_with("--") => Ok(Some(found)),
        _ => Err(format!("{name} に値が無い")),
    }
}

/// `--<name>`（**値を持たない flag**）が在るか。
///
/// 値なし flag の読み手は `pipe` の中でこの 1 本だけである（`s2-07l.485`）。同じ 1 行を書き写すと、
/// 片方だけが `starts_with` や部分一致へ緩む形で穴が開く（[`flag`] が 3 本目の値つき reader を
/// 畳んだのと同じ理由）。
pub(in crate::pipe) fn present(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

/// 必須の flag。
pub(super) fn need<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    flag(args, name)?.ok_or(format!("{name} が要る"))
}

/// 規則の値。`--rules` が在ればその file、無ければ埋め込み。
pub(super) fn manifest_of(args: &[String]) -> Result<Manifest, String> {
    let path = flag(args, "--rules")?;
    let loaded = match path {
        Some(found) => Manifest::load(Path::new(found)),
        None => Manifest::embedded(),
    };
    loaded.map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
            .join(" / ")
    })
}

/// rules 行の整数値。
pub(in crate::pipe) fn int_row(manifest: &Manifest, id: &str) -> Result<u64, String> {
    let row = manifest.get(id).ok_or(format!("{id} が無い"))?;
    if !row.enabled {
        return Err(format!("{id} は不発効である"));
    }
    match row.value {
        RuleValue::Int(found) => Ok(found),
        _ => Err(format!("{id} が整数でない")),
    }
}

/// rules 行の文字列の列。
pub(super) fn list_row(manifest: &Manifest, id: &str) -> Result<Vec<String>, String> {
    let row = manifest.get(id).ok_or(format!("{id} が無い"))?;
    if !row.enabled {
        return Err(format!("{id} は不発効である"));
    }
    match row.value {
        RuleValue::List(ref found) => Ok(found.clone()),
        _ => Err(format!("{id} が文字列の列でない")),
    }
}

/// 置き場。`--state-dir` が上書きし、無ければ repo に紐づいた git 設定から読む。
///
/// repo の解き方は [`repo_of`] ただ 1 本（`--repo`・無ければ断る＝cwd を読まない）。
pub(in crate::pipe) fn state_dir_of(args: &[String]) -> Result<PathBuf, String> {
    if let Some(found) = flag(args, "--state-dir")? {
        return Ok(PathBuf::from(found));
    }
    let root = repo_of(args)?;
    vessel::state_dir(&root)
        .ok_or_else(|| format!("{} に置き場が紐づいていない（vessel init）", root.display()))
}

/// 対象 repo の flag（読む側と usage の字面の同じ 1 つ）。
pub(in crate::pipe) const REPO_FLAG: &str = "--repo";

/// `--repo` の値。**pipe の `--repo` の読み手はこの 1 本**である（設計 dispatcher.md §12・C2）。
///
/// 値は `std::path::absolute` で**絶対にしてから**返す（標準 library・symlink も存在も見ない＝席の打刻の
/// 絶対化と同じ関数）。相対のまま使うと便の worktree の場所も相対になり、cwd を worktree に移した子から
/// 解けない（2026-09-19 の実測）。絶対にできない入力は空文字だけで、断る variant を足すより直す 1 行が
/// 小さい（C17.4）——その空文字は理由の 1 行で断る（NFR4）。無い周は `None`（断るか別の面から解くかは呼び手が決める）。
pub(in crate::pipe) fn repo_flag(args: &[String]) -> Result<Option<PathBuf>, String> {
    let Some(found) = flag(args, REPO_FLAG)? else {
        return Ok(None);
    };
    std::path::absolute(found)
        .map(Some)
        .map_err(|err| format!("{REPO_FLAG} {found} を絶対 path にできない: {err}"))
}

/// 対象 repo。`--repo` が無ければ flag 不在の断り（`need` と同じ字面）で、**cwd を読まない**（設計 pipeline.md §15）。
///
/// cwd から解くと、cargo-mutants の一時コピーのように `.git` が本物の gitdir を指す木の中で、呼び手の指さない
/// repo に worktree と branch が切られる（2026-09-15 の実測）。
pub(super) fn repo_of(args: &[String]) -> Result<PathBuf, String> {
    repo_flag(args)?.ok_or(format!("{REPO_FLAG} が要る"))
}

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行・何もしない）。
pub(in crate::pipe) fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
pub(in crate::pipe) fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}
