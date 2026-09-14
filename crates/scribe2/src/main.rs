//! CLI の骨格。`name` / `--version` / `doctor` / `rules` / `fleet` / `vessel` / `hook` / `pipe` /
//! `runner` / `lens` / `seat` / `polarity` / `contracts` の 13 subcommand を持つ。
//!
//! subcommand の結果は [`Outcome`] ただ 1 型で、rc はその `rc` をそのまま返す。
//!
//! 名前の字面は `name.rs` にだけ在り、この file には書かない。実体は lib 側に在り、
//! この file は引数の dispatch と出力層だけを持つ。出力は [`emit`] と [`emit_err`]
//! の 2 つに閉じ、rc は `main` が返す [`ExitCode`] で表す。

use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;
use vessel::cli_outcome::{Outcome, RC_REFUSED};
use vessel::fleet::json_tree::{self, Tree};
use vessel::name::NAME;
use vessel::rules::manifest::{HostManifest, Manifest};

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

/// `doctor` の出力行。`--state-dir S [--tmux-socket PATH] [--rules FILE]` 付きは登録 row の一覧（`model` の欄
/// つき・1 row 1 行）と実在の target の突合 1 行（C3.2・seat-roles.md §9 (e)）の後ろに、host の面の 1 行と口座の
/// 前提の行（[`account_lines`]・account-autonomy.md §5）を足す。値欠け・空文字・重複・未知の引数は使い方の誤り（`Err`）。
fn render_doctor_with(rest: &[String]) -> Result<Vec<String>, ()> {
    let (mut lines, mut state_dir, mut socket, mut rules) = (render_doctor(), None, None, None);
    for pair in rest.chunks(2) {
        match (pair.first().map(String::as_str), pair.get(1).filter(|v| !v.trim().is_empty() && !v.starts_with("--"))) {
            (Some("--state-dir"), Some(found)) if state_dir.is_none() => state_dir = Some(found),
            (Some("--tmux-socket"), Some(found)) if socket.is_none() => socket = Some(found.as_str()),
            (Some("--rules"), Some(found)) if rules.is_none() => rules = Some(found.as_str()),
            _ => return Err(()),
        }
    }
    match (state_dir, socket, rules) {
        (Some(dir), _, _) => {
            lines.extend(vessel::seat::role::doctor_lines(Path::new(dir), socket));
            lines.extend(account_lines(Path::new(dir), rules));
        }
        (None, None, None) => {}
        (None, _, _) => return Err(()),
    }
    Ok(lines)
}

/// 口座の dir・credential・config の在る / 無い（`dir=` / `credential=` / `config=` の値・閉じた enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    /// 在る。
    Present,
    /// 無い。
    Missing,
}

impl Presence {
    /// 真偽から写す。
    fn of(found: bool) -> Self {
        if found {
            Self::Present
        } else {
            Self::Missing
        }
    }

    /// 行の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Missing => "missing",
        }
    }
}

/// agent view の読み（`agentview=` の値・account-autonomy.md §5「agent view の前提 (4)」・閉じた enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentView {
    /// `settings.json` の `disableAgentView` が `true`。
    Off,
    /// key が無い / `false`。
    On,
    /// file が無い・読めない・形が違う（`on` に潰さない・C11）。
    Unreadable,
}

impl AgentView {
    /// 真偽の key の読み（[`flag_at`]）から写す。
    fn of(read: Result<bool, ()>) -> Self {
        match read {
            Ok(true) => Self::Off,
            Ok(false) => Self::On,
            Err(()) => Self::Unreadable,
        }
    }

    /// 行の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Unreadable => "unreadable",
        }
    }
}

/// 「設定 dir × anchor」の trust の読み（`trust=` の値・account-autonomy.md §5「trust の前提」・閉じた enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trust {
    /// `.claude.json` の `projects[<anchor>].hasTrustDialogAccepted` が `true`。
    Accepted,
    /// key が無い / `false`。
    Missing,
    /// file が無い・読めない・形が違う・event log を読めない（`missing` に潰さない・C11）。
    Unreadable,
    /// 登録 row が 0 件（突き合わせる anchor が無い）。
    NotApplicable,
}

impl Trust {
    /// 真偽の key の読み（[`flag_at`]）から写す。
    fn of(read: Result<bool, ()>) -> Self {
        match read {
            Ok(true) => Self::Accepted,
            Ok(false) => Self::Missing,
            Err(()) => Self::Unreadable,
        }
    }

    /// 行の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Missing => "missing",
            Self::Unreadable => "unreadable",
            Self::NotApplicable => "n/a",
        }
    }
}

/// 口座 1 つの前提の読み（`<state_dir>/accounts/<label>` の直下を読んだ結果・何も書かない）。
struct AccountProbe {
    /// dir か（link を辿って dir）。
    dir: Presence,
    /// 直下の `.credentials.json` が file か（中身は読まない）。
    credential: Presence,
    /// 直下の `settings.json` が file か（Claude Code の設定 dir の印）。
    config: Presence,
    /// 直下の `settings.json` の `disableAgentView`。
    agentview: AgentView,
    /// 登録 row の anchor ごとの trust（anchor の辞書順・event log を読めない周は `None`）。
    trust: Option<Vec<(String, Trust)>>,
}

/// JSON file を入れ子の reader で読む。file が無い・読めない・JSON でない周は `None`。
fn read_tree(path: &Path) -> Option<Tree> {
    json_tree::parse(&std::fs::read_to_string(path).ok()?).ok()
}

/// `path` の key を辿った真偽。途中か末端の key が無い周は `Ok(false)`・読めない file（`None`）と object で
/// ない途中・真偽でない末端は `Err`（形が違う）。
fn flag_at(tree: Option<&Tree>, path: &[&str]) -> Result<bool, ()> {
    let mut node = tree.ok_or(())?;
    for key in path {
        let Tree::Object(_) = node else { return Err(()) };
        match node.get(key) {
            Some(next) => node = next,
            None => return Ok(false),
        }
    }
    node.as_bool().ok_or(())
}

/// 口座の dir を読む（読むだけ・`.claude.json` / credential / settings に書かない）。
fn probe_account(dir: &Path, anchors: Option<&BTreeSet<String>>) -> AccountProbe {
    let is_file = |name: &str| std::fs::metadata(dir.join(name)).is_ok_and(|found| found.is_file());
    let claude = read_tree(&dir.join(".claude.json"));
    let trust_of = |anchor: &String| {
        let read = flag_at(claude.as_ref(), &["projects", anchor, "hasTrustDialogAccepted"]);
        (anchor.clone(), Trust::of(read))
    };
    AccountProbe {
        dir: Presence::of(std::fs::metadata(dir).is_ok_and(|found| found.is_dir())),
        credential: Presence::of(is_file(".credentials.json")),
        config: Presence::of(is_file("settings.json")),
        agentview: AgentView::of(flag_at(read_tree(&dir.join("settings.json")).as_ref(), &["disableAgentView"])),
        trust: anchors.map(|found| found.iter().map(trust_of).collect()),
    }
}

/// 口座 1 行（pure）。trust は anchor が 1 つなら `trust=<値>`・複数なら anchor ごとに `trust=<潰した anchor>:<値>`
/// を並べる（潰し方は席の dir 名と同じ [`vessel::seat::sanitize_target`]）。
fn render_account(label: &str, probe: &AccountProbe) -> String {
    let head = format!(
        "account={label} dir={} credential={} config={} agentview={}",
        probe.dir.as_str(),
        probe.credential.as_str(),
        probe.config.as_str(),
        probe.agentview.as_str()
    );
    let cells: Vec<String> = match probe.trust.as_deref() {
        None => vec![Trust::Unreadable.as_str().to_owned()],
        Some([]) => vec![Trust::NotApplicable.as_str().to_owned()],
        Some([(_, only)]) => vec![only.as_str().to_owned()],
        Some(many) => many
            .iter()
            .map(|(anchor, found)| format!("{}:{}", vessel::seat::sanitize_target(anchor), found.as_str()))
            .collect(),
    };
    cells.iter().fold(head, |line, cell| format!("{line} trust={cell}"))
}

/// doctor の host の面の 1 行（`host-manifest=<present|absent|unreadable>`・account-lifecycle.md §2・読むだけ）。
fn render_host_manifest(word: &str) -> String {
    format!("host-manifest={word}")
}

/// doctor の口座の項目（C3.2 の「口座」の面・account-autonomy.md §5）: 先頭に host の面の 1 行（[`render_host_manifest`]）、
/// 続けて宣言（`--rules FILE` か埋め込みの tracked の面 + `<state_dir>/host.toml`・env を読まない）の `[[account]]` の
/// label の辞書順に 1 行。判定しない（rc を変えず行を出すだけ）。宣言を読めない周は 1 行
/// `accounts: manifest=unreadable`（0 行に潰さない・C11）。host の面が壊れている周も報告は止めない。
fn account_lines(state_dir: &Path, rules: Option<&str>) -> Vec<String> {
    let host = HostManifest::read(&vessel::rules::host_manifest_path(state_dir));
    let word = host.as_str();
    let declared = rules.map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path))).map(|tracked| tracked.joined(host));
    // tracked の面が読めて合わせで落ちた周は host の面の欠陥（面をまたぐ重複を含む）＝file 単体が読めても unreadable。
    let word = if matches!(declared, Ok(Err(_))) { HostManifest::Unreadable(Vec::new()).as_str() } else { word };
    let mut lines = vec![render_host_manifest(word)];
    let Ok(Ok(manifest)) = declared else {
        lines.push("accounts: manifest=unreadable".to_owned());
        return lines;
    };
    let labels: BTreeSet<&str> = manifest.accounts().iter().map(|account| account.label()).collect();
    let anchors: Option<BTreeSet<String>> = vessel::fleet::store::read_all(state_dir).ok().map(|events| {
        let state = vessel::fleet::replay(&events);
        state.registrations.values().map(|latest| latest.registration.anchor.clone()).collect()
    });
    let line = |label: &&str| {
        let probe = probe_account(&vessel::fleet::account_dir(state_dir, label), anchors.as_ref());
        render_account(label, &probe)
    };
    lines.extend(labels.iter().map(line));
    lines
}

/// 未知の引数に対する使い方の行。
fn render_usage() -> String {
    format!("usage: {NAME} <name|--version|doctor|rules|fleet|vessel|hook|pipe|runner|lens|seat|polarity|contracts>")
}

/// 先頭の引数と続く引数を出力行の列へ写す。未知なら `Err` に使い方を載せる。
fn dispatch(arg: Option<&str>, rest: &[String]) -> Result<Vec<String>, String> {
    match arg {
        Some("name") => Ok(vec![render_name()]),
        Some("--version") => Ok(vec![render_version()]),
        Some("doctor") => render_doctor_with(rest).map_err(|()| render_usage()),
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
        Some("seat") => vessel::seat::cli::dispatch(rest),
        // 極性一覧（ADR-0014 §2.2）。引数も stdin も env も読まない。
        Some("polarity") => Outcome::ok(vessel::polarity::render()),
        // 契約表の検査と欄の生成物（設計 contract-source.md §2）。env を読まない。
        Some("contracts") => vessel::pipe::cli::contracts(rest),
        // headless の 2 つは stdin を**自分で**読む（runner は契約 text・lens は diff の
        // byte で、cap の判定に byte 数が要る＝ここで String へ均すと大きさが変わる）。
        Some("runner") => vessel::headless::runner::dispatch(rest),
        Some("lens") => vessel::headless::lens::dispatch(rest),
        // hook だけは stdin の payload を要る（Claude Code が JSON を流し込む）。
        Some("hook") => vessel::hook::dispatch(rest, &read_stdin()),
        first => match dispatch(first, rest) {
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
    use super::{
        render_account, render_doctor, render_host_manifest, render_name, render_usage, render_version, AccountProbe,
        AgentView, Presence, Trust, NAME,
    };
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
    /// 結合の順序は doctor の 2 行 → host の面の行 → 口座の行（fixture 1 つ・anchor 2 つの形）→ usage → version で、区切り文字は
    /// LF ただ 1 種である。版番号は assert の前に `[version]` へ置換する（`default-features =
    /// false` では `Settings::add_filter` が無いので `filters` feature に頼らない）。
    #[test]
    fn doctor_external_form() {
        let mut lines = render_doctor();
        let probe = AccountProbe {
            dir: Presence::Present,
            credential: Presence::Present,
            config: Presence::Missing,
            agentview: AgentView::Unreadable,
            trust: Some(vec![("/repo/a".to_owned(), Trust::Accepted), ("/repo/b".to_owned(), Trust::Unreadable)]),
        };
        lines.push(render_host_manifest("present"));
        lines.push(render_account("acct", &probe));
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
