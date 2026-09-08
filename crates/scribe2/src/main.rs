//! CLI の骨格。`name` / `--version` / `doctor` の 3 subcommand を持つ。
//!
//! 名前の字面は [`name::NAME`] にだけ在り、この file には書かない。
//! 出力は [`emit`] ただ 1 つに閉じ、rc は `main` が返す [`ExitCode`] で表す。

mod name;

use name::NAME;
use std::process::ExitCode;

/// 出力層。stdout へ書くのはこの関数だけである。
#[expect(
    clippy::print_stdout,
    reason = "CLI の出力層をこの 1 関数に閉じるための例外"
)]
fn emit(line: &str) {
    println!("{line}");
}

/// `name` subcommand が出力する行を組み立てる。
///
/// `name_is_single_source` はこの戻り値を観測点に使う（bin crate の in-module
/// test からは binary を spawn できないため、出力層へ渡る文字列を直接見る）。
fn render_name() -> String {
    NAME.to_owned()
}

/// `--version` が出力する行を組み立てる。
fn render_version() -> String {
    format!("{NAME} {}", env!("CARGO_PKG_VERSION"))
}

/// `doctor` が出力する行を組み立てる（骨格の stub: NAME と version を 1 行ずつ）。
fn render_doctor() -> Vec<String> {
    vec![render_name(), render_version()]
}

/// 未知の引数に対する使い方の行。
fn render_usage() -> String {
    format!("usage: {NAME} <name|--version|doctor>")
}

/// 引数 1 つを出力行の列へ写す。未知なら `Err` に使い方を載せる。
fn dispatch(arg: Option<&str>) -> Result<Vec<String>, String> {
    match arg {
        Some("name") => Ok(vec![render_name()]),
        Some("--version") => Ok(vec![render_version()]),
        Some("doctor") => Ok(render_doctor()),
        _ => Err(render_usage()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(args.first().map(String::as_str)) {
        Ok(lines) => {
            for line in &lines {
                emit(line);
            }
            ExitCode::SUCCESS
        }
        Err(usage) => {
            emit(&usage);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{render_name, NAME};
    use std::path::PathBuf;

    /// workspace root（この crate の 2 つ上）。
    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
    }

    /// JSON から `"<key>": "<値>"` の値を std だけで抜く。
    ///
    /// 骨格に JSON crate を足さないための最小抽出である（依存は 0 本）。
    fn json_string_field(src: &str, key: &str) -> Option<String> {
        let after_key = src.split_once(&format!("\"{key}\""))?.1;
        let after_colon = after_key.split_once(':')?.1;
        let after_open = after_colon.split_once('"')?.1;
        after_open.split_once('"').map(|(value, _)| value.to_owned())
    }

    /// NAME・package name・plugin.json の name・出力層へ渡る文字列の 4 者が一致する。
    #[test]
    fn name_is_single_source() {
        let manifest_name = env!("CARGO_PKG_NAME");
        let plugin_path = workspace_root().join(".claude-plugin").join("plugin.json");
        let plugin_src = std::fs::read_to_string(&plugin_path)
            .unwrap_or_else(|err| panic!("{} を読めない: {err}", plugin_path.display()));
        let plugin_name = json_string_field(&plugin_src, "name")
            .unwrap_or_else(|| panic!("plugin.json に name が無い"));

        assert_eq!(NAME, manifest_name, "name.rs の NAME と [package] name");
        assert_eq!(render_name(), manifest_name, "出力層へ渡る文字列と [package] name");
        assert_eq!(plugin_name, manifest_name, "plugin.json の name と [package] name");
    }
}
