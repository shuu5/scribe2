//! CLI の骨格。`name` / `--version` / `doctor` / `rules` / `fleet` / `vessel` / `hook` / `pipe` /
//! `runner` / `lens` の 10 subcommand を持つ。
//!
//! subcommand の結果は [`Outcome`] ただ 1 型で、rc はその `rc` をそのまま返す。
//!
//! 名前の字面は `name.rs` にだけ在り、この file には書かない。実体は lib 側に在り、
//! この file は引数の dispatch と出力層だけを持つ。出力は [`emit`] と [`emit_err`]
//! の 2 つに閉じ、rc は `main` が返す [`ExitCode`] で表す。

use std::process::ExitCode;
use vessel::cli_outcome::{Outcome, RC_REFUSED};
use vessel::name::NAME;

/// 出力層。stdout へ書くのはこの関数だけである。
#[expect(
    clippy::print_stdout,
    reason = "CLI の出力層をこの 1 関数に閉じるための例外"
)]
fn emit(line: &str) {
    println!("{line}");
}

/// 出力層。stderr へ書くのはこの関数だけである。
#[expect(
    clippy::print_stderr,
    reason = "CLI の error 出力層をこの 1 関数に閉じるための例外"
)]
fn emit_err(line: &str) {
    eprintln!("{line}");
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
    format!("usage: {NAME} <name|--version|doctor|rules|fleet|vessel|hook|pipe|runner|lens>")
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

/// 引数列を 1 回分の結果へ写す。
///
/// subcommand は **1 つの [`Outcome`] 型だけ**を返す（憲法 C2）。rc も字面もここでは
/// 作り替えない。
fn run(args: &[String]) -> Outcome {
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("rules") => vessel::rules::cli::dispatch(rest),
        Some("fleet") => vessel::fleet::cli::dispatch(rest),
        Some("vessel") => vessel::hook::vessel::dispatch(rest),
        Some("pipe") => vessel::pipe::cli::dispatch(rest),
        // headless の 2 つは stdin を**自分で**読む（runner は契約 text・lens は diff の
        // byte で、cap の判定に byte 数が要る＝ここで String へ均すと大きさが変わる）。
        Some("runner") => vessel::headless::runner::dispatch(rest),
        Some("lens") => vessel::headless::lens::dispatch(rest),
        // hook だけは stdin の payload を要る（Claude Code が JSON を流し込む）。
        Some("hook") => vessel::hook::dispatch(rest, &read_stdin()),
        first => match dispatch(first) {
            Ok(lines) => Outcome::ok(lines),
            // 使い方の行は従来どおり stdout へ出し rc 1 で終える（外形は変えない）。
            Err(usage) => Outcome {
                out: vec![usage],
                err: Vec::new(),
                rc: RC_REFUSED,
            },
        },
    }
}

/// stdin をすべて読む。読めなければ空文字（payload 不在として扱う）。
fn read_stdin() -> String {
    let mut buffer = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer);
    buffer
}

/// 組んだ行を出力層へ流す。
fn emit_all(out: &[String], err: &[String]) {
    for line in out {
        emit(line);
    }
    for line in err {
        emit_err(line);
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = run(&args);
    emit_all(&outcome.out, &outcome.err);
    ExitCode::from(outcome.rc)
}

#[cfg(test)]
mod tests {
    use super::{render_doctor, render_name, render_usage, render_version, NAME};
    use std::ffi::OsStr;
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
    /// `doctor` / usage / `--version` の外形を 1 つの snapshot に固定する。
    ///
    /// 結合の順序は doctor の 2 行 → usage → version で、区切り文字は LF ただ 1 種
    /// である。版番号は assert の前に `[version]` へ置換する（`default-features =
    /// false` では `Settings::add_filter` が無いので `filters` feature に頼らない）。
    #[test]
    fn doctor_external_form() {
        let mut lines = render_doctor();
        lines.push(render_usage());
        lines.push(render_version());
        let masked = lines.join("\n").replace(env!("CARGO_PKG_VERSION"), "[version]");
        insta::assert_snapshot!(masked);
    }

    /// insta の force 系 env が立っていない（未設定・空・`0` のいずれか）。
    fn force_flag_is_off(value: Option<&OsStr>) -> bool {
        match value {
            None => true,
            Some(found) => matches!(found.to_str(), Some("") | Some("0")),
        }
    }

    /// `INSTA_UPDATE` が許容集合（未設定・空 / `no` / `new` / `auto`）の内側か。
    ///
    /// `unseen` は insta 1.48.0 では「snapshot file が在る」で真になる極性反転を持ち、
    /// **snapshot 不在時に InPlace へ落ちて実出力から `.snap` を無音生成し rc 0 で
    /// 通す**ので、安全そうな名前だが許容集合へ入れない（`always` / `1` / `force` と
    /// 未知値も同じく落とす）。
    fn update_mode_is_safe(value: Option<&OsStr>) -> bool {
        match value {
            None => true,
            Some(found) => matches!(
                found.to_str(),
                Some("") | Some("no") | Some("new") | Some("auto")
            ),
        }
    }

    /// insta の判定を無効化する経路が **判定プロセスの実効 env / file** に無いこと。
    ///
    /// 外から経路を列挙して塞ぐのをやめた面である。env はどの機構で設定されても子
    /// プロセスが必ず見るので cargo config の `[env]` も `$CARGO_HOME` も
    /// setup-script も一度に閉じ、config file は insta 1.48.0 の探索集合が 3 名で
    /// 閉じているのでその不在で閉じる。
    #[test]
    fn insta_env_is_clean() {
        for name in [
            "INSTA_FORCE_PASS",
            "INSTA_FORCE_UPDATE",
            "INSTA_FORCE_UPDATE_SNAPSHOTS",
        ] {
            let value = std::env::var_os(name);
            assert!(
                force_flag_is_off(value.as_deref()),
                "{name} が立っている（値 {value:?}）: insta の判定が無効化される"
            );
        }
        let update = std::env::var_os("INSTA_UPDATE");
        assert!(
            update_mode_is_safe(update.as_deref()),
            "INSTA_UPDATE が許容外（値 {update:?}）: 許すのは未設定 / 空 / no / new / auto だけである"
        );
        let runtime = std::env::var_os("INSTA_WORKSPACE_ROOT");
        assert!(
            runtime.is_none(),
            "INSTA_WORKSPACE_ROOT が実行時 env に在る（値 {runtime:?}）: config 探索の root が差し替わる"
        );
        assert!(
            option_env!("INSTA_WORKSPACE_ROOT").is_none(),
            "INSTA_WORKSPACE_ROOT が compile 時 env に在る: config 探索の root が差し替わる"
        );
        for name in [".config/insta.yaml", "insta.yaml", ".insta.yaml"] {
            let path = workspace_root().join(name);
            assert!(
                std::fs::symlink_metadata(&path).is_err(),
                "insta の config file が在る: {}（force_pass / update を外から立てられる）",
                path.display()
            );
        }
    }
}
