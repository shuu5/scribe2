//! workspace 保守の内製 task runner（`cargo xtask <subcommand>`）。
//!
//! 依存は std だけ。憲法 C4 の歯（行数 / 比 / manifest parity / lints）を
//! [`check`] が測り、[`genmanifest`] が plugin manifest を NAME から生成する。
//! rc は `main` が返す [`ExitCode`] で表し、helper から `process::exit` は呼ばない。

mod check;
mod genmanifest;
mod limits;
mod toml_lite;

use std::path::PathBuf;
use std::process::ExitCode;

/// 使い方の 1 行。
const USAGE: &str = "usage: cargo xtask <check|gen-manifest> [ROOT]";

/// stdout 出力層。stdout へ書くのはこの関数だけである。
#[expect(
    clippy::print_stdout,
    reason = "CLI の stdout 出力をこの 1 関数に閉じるための例外"
)]
fn emit(line: &str) {
    println!("{line}");
}

/// stderr 出力層。stderr へ書くのはこの関数だけである。
#[expect(
    clippy::print_stderr,
    reason = "CLI の stderr 出力をこの 1 関数に閉じるための例外"
)]
fn emit_err(line: &str) {
    eprintln!("{line}");
}

/// `ROOT` 引数を解決する。省略時**のみ** cwd を root とする（■D1）。
fn resolve_root(arg: Option<&str>) -> Result<PathBuf, String> {
    match arg {
        Some(path) => Ok(PathBuf::from(path)),
        None => std::env::current_dir().map_err(|err| format!("cwd を解決できない: {err}")),
    }
}

/// `check` subcommand。違反 0 ならサマリ 1 行、1 件以上なら違反行を並べて rc 1。
fn run_check(root_arg: Option<&str>) -> Result<ExitCode, String> {
    let root = resolve_root(root_arg)?;
    let violations = check::check(&root);
    if violations.is_empty() {
        emit(&check::summary(&root));
        return Ok(ExitCode::SUCCESS);
    }
    for violation in &violations {
        emit(violation);
    }
    Ok(ExitCode::FAILURE)
}

/// `gen-manifest` subcommand。
fn run_gen_manifest(root_arg: Option<&str>) -> Result<ExitCode, String> {
    let root = resolve_root(root_arg)?;
    emit(&genmanifest::generate(&root)?);
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root_arg = args.get(1).map(String::as_str);
    let outcome = match args.first().map(String::as_str) {
        Some("check") => run_check(root_arg),
        Some("gen-manifest") => run_gen_manifest(root_arg),
        _ => Err(USAGE.to_owned()),
    };
    match outcome {
        Ok(code) => code,
        Err(reason) => {
            emit_err(&reason);
            ExitCode::FAILURE
        }
    }
}
