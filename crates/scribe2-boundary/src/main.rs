//! CLI の骨格。`name` / `--version` / `doctor` / `account` / `rules` / `fleet` / `vessel` / `hook` / `host-guard` /
//! `pipe` / `runner` / `lens` / `seat` / `polarity` / `contracts` の 15 subcommand と、memo の plan の口 `ledger` と、雛形の
//! pointer を書く口 `host`（host-init.md §3）と、repo を器に載せる口 `init`（同 §4）を持つ。
//!
//! subcommand の結果は [`Outcome`] ただ 1 型で、rc はその `rc` をそのまま返す。
//!
//! 名前の字面は `name.rs` にだけ在り、この file には書かない。実体は lib 側に在り、
//! この file は引数の dispatch と出力層だけを持つ。出力は [`emit`] と [`emit_err`]
//! の 2 つに閉じ、rc は `main` が返す [`ExitCode`] で表す。

use std::path::Path;
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

/// `--version` が出力する行を組み立てる（`<NAME> <version> (<build 元 commit>)`・設計 consumer-sync.md §2）。
///
/// 括弧の中身は `build.rs` が compile time に焼いた `SCRIBE2_BUILD_COMMIT`（`<sha12>` / `<sha12>+dirty` /
/// 測れない周は `unknown`・C10）。実行時に env を読まない（C2.2）。doctor の 2 行目も同じ関数（FR51・FR61）。
fn render_version() -> String {
    format!("{NAME} {} ({})", env!("CARGO_PKG_VERSION"), env!("SCRIBE2_BUILD_COMMIT"))
}

/// `doctor` が出力する行を組み立てる（骨格の stub: NAME と version を 1 行ずつ）。
fn render_doctor() -> Vec<String> {
    vec![render_name(), render_version()]
}

/// `doctor` の出力行。骨格の 2 行の直後は引数に依らず雛形の pointer の 1 行（`host-template=`・[`vessel::init::doctor_lines`]・
/// host-init.md §3）で、`--state-dir S` 付きはその直後に `init=` の 1 行（ROOT は `--repo` か cwd・欠落の名指しと `next=`・
/// 同 §6）。`--state-dir S [--tmux-socket PATH] [--rules FILE]` 付きは続けて run 無しの裁定の突合の 1 行（数えるものが
/// 在る周だけ・[`vessel::seat::ruling::doctor_lines`]・fleet-event-log.md §9 (4)）、続けて登録 row の一覧（`model` の欄
/// つき・1 row 1 行）と実在の target の突合 1 行（C3.2・seat-roles.md §9 (e)）の後ろに、host の面の 1 行と口座の
/// 前提の行（`account ls` と同じ 1 関数・`retired=` つき・[`vessel::account::doctor_lines`]・account-lifecycle.md §3）、
/// さらに導入先の行（1 導入先 1 行・[`vessel::account::consumers::doctor_lines`]・consumer-sync.md §4・FR61）を
/// 足し、最後に host-guard の 1 行（種類ごとの on / off・配線・binary の解決・[`vessel::account::wire::doctor_line`]・
/// vessel-hook.md §12 形 5）を足す。`--bin B`（`--state-dir` と並べる歯の seam）は binary の解決に撃つ program を
/// `<NAME>` から差し替える。`--repo R` 付きは末尾に台帳 lint の 1 行と台帳の形の 1 行（[`vessel::ledger::lint::doctor_lines`]・
/// contract-source.md §6 / ledger-form.md §3 の 4・台帳は 1 回だけ読む・`--state-dir` 無しでも `--rules` と並べて
/// 撃てる）。`--unit-dir U --binary PATH`（`--state-dir` と並べる・2 つはそろって在る）は登録 row の行の末尾に `tick-unit=` の
/// 1 語を足す（seat-heartbeat.md §3）。2 つとも無い周は host の面の `[[tick]]` の値が既定（面にも無ければ足さない・flag が勝つ・§5 形 3）。
/// 値欠け・空文字・重複・未知の引数・片方だけの `--unit-dir` / `--binary` は使い方の誤り（`Err`）。
fn render_doctor_with(rest: &[String]) -> Result<Vec<String>, ()> {
    let (mut lines, mut state_dir, mut socket, mut rules, mut repo, mut bin) = (render_doctor(), None, None, None, None, None);
    let (mut unit_dir, mut binary) = (None, None);
    for pair in rest.chunks(2) {
        match (pair.first().map(String::as_str), pair.get(1).filter(|v| !v.trim().is_empty() && !v.starts_with("--"))) {
            (Some("--state-dir"), Some(found)) if state_dir.is_none() => state_dir = Some(found),
            (Some("--tmux-socket"), Some(found)) if socket.is_none() => socket = Some(found.as_str()),
            (Some("--rules"), Some(found)) if rules.is_none() => rules = Some(found.as_str()),
            (Some("--repo"), Some(found)) if repo.is_none() => repo = Some(found.as_str()),
            (Some("--bin"), Some(found)) if bin.is_none() => bin = Some(found.as_str()),
            (Some("--unit-dir"), Some(found)) if unit_dir.is_none() => unit_dir = Some(Path::new(found.as_str())),
            (Some("--binary"), Some(found)) if binary.is_none() => binary = Some(Path::new(found.as_str())),
            _ => return Err(()),
        }
    }
    let units = match (unit_dir, binary) {
        (Some(unit_dir), Some(binary)) => Some(vessel::seat::tick::install::Probe { unit_dir, binary, rules: rules.map(Path::new) }),
        (None, None) => None,
        _ => return Err(()),
    };
    // 骨格の 2 行の直後に雛形の pointer の 1 行（置き場を渡さない周も出す＝`init` の前に確かめられる・host-init.md §3）と、
    // 置き場を渡した周だけ `init=` の 1 行（ROOT は `--repo` か cwd・host-init.md §6）。
    let root = repo.map_or_else(|| std::env::current_dir().unwrap_or_default(), std::path::PathBuf::from);
    lines.extend(vessel::init::doctor_lines(state_dir.map(Path::new), &root, socket));
    match (state_dir, socket, rules, repo) {
        (Some(dir), _, _, _) => {
            lines.extend(vessel::seat::ruling::doctor_lines(Path::new(dir), rules));
            lines.extend(vessel::seat::role::doctor_lines(Path::new(dir), socket, rules, units.as_ref()));
            lines.extend(vessel::account::doctor_lines(Path::new(dir), rules));
            lines.extend(vessel::account::consumers::doctor_lines(Path::new(dir), rules));
            let program = Path::new(bin.unwrap_or(NAME));
            lines.push(vessel::account::wire::doctor_line(Path::new(dir), rules, &render_version(), program));
        }
        (None, None, None, _) | (None, None, Some(_), Some(_)) if bin.is_none() && units.is_none() => {}
        (None, _, _, _) => return Err(()),
    }
    if let Some(found) = repo {
        lines.extend(vessel::ledger::lint::doctor_lines(Path::new(found), rules));
    }
    Ok(lines)
}

/// 未知の引数に対する使い方の行。
fn render_usage() -> String {
    format!("usage: {NAME} <name|--version|doctor|account|rules|fleet|vessel|hook|host-guard|pipe|runner|lens|seat|polarity|contracts>")
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
/// 作り替えない。`program` は自分自身の呼ばれ方（`argv[0]`）で、`init` の 9 段目が子の program に使う（host-init.md §5）。
fn run(args: &[String], program: &Path) -> Outcome {
    let rest = args.get(1..).unwrap_or_default();
    // 人向けの案内の口（`help [<command>]`・頂点と各 command の直後の `--help` / `-h`・cli-help.md §2）。他の周は下の今の経路。
    if let Some(outcome) = vessel::help::answer(args, &render_usage()) {
        return outcome;
    }
    match args.first().map(String::as_str) {
        // 口座の口（account-lifecycle.md §3）。置き場は `--state-dir` だけ・env を読まない。
        Some("account") => vessel::account::cli::dispatch(rest),
        Some("rules") => vessel::rules::cli::dispatch(rest),
        Some("fleet") => vessel::fleet::cli::dispatch(rest),
        Some("vessel") => vessel::hook::vessel::dispatch(rest),
        // 雛形の pointer を host に 1 回書く口（host-init.md §3）。使い方は `host` 自身が持つ（上の 1 行の usage は外形
        // snapshot が pin しているので動かさない）。
        Some("host") => vessel::init::host_dispatch(rest),
        // repo を器に載せる 9 段（host-init.md §4 / §5）。使い方は `init` 自身が持つ。9 段目の `seat launch` の子は自分自身の
        // 呼ばれ方（`argv[0]`・`current_exe` は読まない）で撃つ。
        Some("init") => vessel::init::dispatch(rest, program),
        Some("pipe") => vessel::pipe::cli::dispatch(rest),
        Some("seat") => vessel::seat::cli::dispatch(rest),
        // 極性一覧（ADR-0014 §2.2）。引数も stdin も env も読まない。
        Some("polarity") => Outcome::ok(vessel::polarity::render()),
        // 契約表の検査と欄の生成物（設計 contract-source.md §2）。env を読まない。
        Some("contracts") => vessel::pipe::cli::contracts(rest),
        // memo の plan（設計 ledger-form.md §3 の 8）。台帳を読まず書かず、plan を stdout に出すだけ。使い方は
        // `ledger` 自身の 1 枚が持つ（上の 1 行の usage は外形 snapshot が pin しているので動かさない）。
        Some("ledger") => vessel::ledger::dispatch(rest),
        // headless の 2 つは stdin を**自分で**読む（runner は契約 text・lens は diff の
        // byte で、cap の判定に byte 数が要る＝ここで String へ均すと大きさが変わる）。
        Some("runner") => vessel::headless::runner::dispatch(rest),
        Some("lens") => vessel::headless::lens::dispatch(rest),
        // hook だけは stdin の payload を要る（Claude Code が JSON を流し込む）。
        Some("hook") => vessel::hook::dispatch(rest, &read_stdin()),
        // host の破壊防止の見張り（設計 vessel-hook.md §11）も stdin の payload を読む。marker と anchor に依らない。
        Some("host-guard") => vessel::hook::host_guard::dispatch(rest, &read_stdin()),
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
    // 起動の実物を最初に据える（core の起動の記述は据えていない周に撃てない・設計 core-boundary.md §9 採る形 3）。
    scribe2_boundary::spawner::install();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let program = std::env::args_os().next().map_or_else(|| std::path::PathBuf::from(NAME), std::path::PathBuf::from);
    let outcome = run(&args, &program);
    emit_all(&outcome.out, &outcome.err);
    ExitCode::from(outcome.rc)
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.198.2
    use super::{render_doctor, render_name, render_usage, render_version, NAME};
    use std::ffi::OsStr;
    use std::path::PathBuf;
    use vessel::account::consumers::{drift_of, render_consumer, Consumer, Head, Source};
    use vessel::account::{render_account, render_host_manifest, AccountProbe, AgentView, Presence, Retired, Trust};
    use vessel::hook::vessel::digest::PluginRecord;
    use vessel::init::{render_host_template, render_init, Gap, Readiness, Template};
    use vessel::name::PLUGIN_DIR;

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

    /// NAME・bin name・plugin.json の name・出力層へ渡る文字列の 4 者が一致し、package name は `<NAME>-boundary`（境界
    /// crate の名は NAME から導く・設計 core-boundary.md §3・C2.2）。plugin.json は生成 dir（`PLUGIN_DIR`）の
    /// 下から読む（root 直下の旧 path は読まない・設計 consumer-sync.md §17 形 5）。
    #[test]
    fn name_is_single_source() {
        let manifest_name = env!("CARGO_BIN_NAME");
        assert_eq!(env!("CARGO_PKG_NAME"), format!("{NAME}-boundary"), "[package] name は NAME から導く");
        let plugin_path = workspace_root().join(PLUGIN_DIR).join(".claude-plugin").join("plugin.json");
        let plugin_src = std::fs::read_to_string(&plugin_path)
            .unwrap_or_else(|err| panic!("{} を読めない: {err}", plugin_path.display()));
        let plugin_name = json_string_field(&plugin_src, "name")
            .unwrap_or_else(|| panic!("plugin.json に name が無い"));

        assert_eq!(NAME, manifest_name, "name.rs の NAME と [[bin]] name");
        assert_eq!(render_name(), manifest_name, "出力層へ渡る文字列と [[bin]] name");
        assert_eq!(plugin_name, manifest_name, "plugin.json の name と [[bin]] name");
    }

    /// 括弧の中身が build 元 commit の 3 形（`<sha12>` / `<sha12>+dirty` / `unknown`）のどれかか（手書きの分岐 3 本・regex を足さない）。
    fn is_build_commit_form(inner: &str) -> bool {
        let is_sha12 = |text: &str| text.len() == 12 && text.chars().all(|ch| matches!(ch, '0'..='9' | 'a'..='f'));
        if inner == "unknown" {
            return true;
        }
        match inner.strip_suffix("+dirty") {
            Some(sha) => is_sha12(sha),
            None => is_sha12(inner),
        }
    }

    /// `--version` の行が `<NAME> <version> (<build 元 commit>)` の形で、括弧の中身が 3 形のどれか（FR61・C10）。
    #[test]
    fn version_line_carries_build_commit_or_unknown() {
        let line = render_version();
        let prefix = format!("{NAME} {} (", env!("CARGO_PKG_VERSION"));
        assert!(line.starts_with(&prefix), "先頭は NAME + version + ` (`: {line}");
        assert!(line.ends_with(')'), "末尾は `)`: {line}");
        let inner = line.strip_prefix(&prefix).and_then(|rest| rest.strip_suffix(')')).unwrap_or_default();
        assert!(!inner.is_empty(), "括弧の中身が空: {line}");
        assert!(is_build_commit_form(inner), "括弧の中身が sha12 / sha12+dirty / unknown のどれでもない: {line}");
        assert_eq!(line.matches('(').count(), 1, "括弧は 1 組: {line}");
        assert!(!is_build_commit_form(""), "空は 3 形の外");
        assert!(!is_build_commit_form("0123456789ab+other"), "未知の接尾辞は 3 形の外");
        assert!(!is_build_commit_form("0123456789ABC"), "大文字・13 桁は 3 形の外");
        assert!(is_build_commit_form("0123456789ab+dirty"), "sha12+dirty は 3 形の内");
    }

    /// doctor の 2 行目は `--version` の行そのもの（同じ 1 関数・FR51）。
    #[test]
    fn version_line_is_shared_by_doctor() {
        assert_eq!(render_doctor().get(1), Some(&render_version()), "doctor の 2 行目 == --version の行");
    }

    /// `doctor` / usage / `--version` の外形を 1 つの snapshot に固定する。
    ///
    /// 結合の順序は doctor の 2 行 → 雛形の pointer の行（path の形・host-init.md §3）→ `init=` の行（欠落 3 つ・§6）→ host の面の行 → 口座の行（fixture 1 つ・anchor 2 つ・退役していない形）→ 導入先の行
    /// （記録の無い launch+install の形・帳簿は HEAD と食い違う・consumer-sync.md §4）→ usage → version で、
    /// 区切り文字は LF ただ 1 種である。版番号は assert の前に `[version]` へ、build 元 commit（build ごとに変わる）は
    /// `[commit]` へ置換する 2 段の mask（`default-features = false` では `Settings::add_filter` が無いので `filters`
    /// feature に頼らない・regex も足さない＝`env!` の実値を置換する）。
    #[test]
    fn doctor_external_form() {
        let mut lines = render_doctor();
        let template = Template::Path(PathBuf::from("/host/template"));
        lines.push(render_host_template(&template));
        lines.push(render_init(&Readiness::Gaps(vec![Gap::Declaration, Gap::Session, Gap::Registration]), &template));
        let probe = AccountProbe {
            dir: Presence::Present,
            credential: Presence::Present,
            config: Presence::Missing,
            agentview: AgentView::Unreadable,
            trust: Some(vec![("/repo/a".to_owned(), Trust::Accepted), ("/repo/b".to_owned(), Trust::Unreadable)]),
        };
        lines.push(render_host_manifest("present", None, &[]));
        lines.push(render_account("acct", &probe, Retired::No));
        let consumer = Consumer {
            source: Source::Both,
            scope: Some("project".to_owned()),
            record: PluginRecord::Absent,
            ledger: Some("0123456789abcdef0123456789abcdef01234567".to_owned()),
            cache: Some("cbf29ce484222325".to_owned()),
        };
        let head = Head::Sha("fedcba9876543210fedcba9876543210fedcba98".to_owned());
        lines.push(render_consumer("/repo/a", &consumer, &head, &drift_of(&consumer, &head, None)));
        lines.push(render_usage());
        lines.push(render_version());
        let masked = lines
            .join("\n")
            .replace(env!("CARGO_PKG_VERSION"), "[version]")
            .replace(&format!("({})", env!("SCRIBE2_BUILD_COMMIT")), "([commit])");
        insta::assert_snapshot!(masked);
    }

    /// doctor の台帳の形の 1 行（ledger-form.md §6 行 a (e)）の外形: doctor の 2 行 → 雛形の pointer の行（absent の形）→ `init=` の行（next=host-init の形）→ 測れた周の 1 行（5 つの欠陥が
    /// 全部 1 件以上・0 件の欄も同じ行）→ 測れない周の 1 行 → usage → version。台帳は `bd list --json` の形の fixture を
    /// 席の reader（`issues_of`）で読み、契約表と tracked は [`vessel::ledger::form::Docs`] を直に組む。
    #[test]
    fn ledger_form_doctor_external_form() {
        use vessel::ledger::form::{judge, render, render_unreadable, Docs, Row};
        let json = r####"[
            {"id":"s2-m.1","status":"open","issue_type":"task","labels":["intake:memo"],"description":"## memo\n### 観測\n### 候補\n### 昇格条件\n"},
            {"id":"s2-m.2","status":"open","issue_type":"task","labels":["intake:memo"],"description":"## memo\n### 出所\n### 観測\n### 候補\n### 昇格条件\n"},
            {"id":"s2-b.1","status":"open","issue_type":"task","labels":["intake:memo"],"acceptance_criteria":"design = docs/design/toy.md#a","description":"## memo\n### 出所\n### 観測\n### 候補\n### 昇格条件\n"},
            {"id":"s2-n.1","status":"open","issue_type":"task"},
            {"id":"s2-e.1","status":"open","issue_type":"epic"},
            {"id":"s2-c.1","status":"open","issue_type":"task","acceptance_criteria":"design = docs/design/toy.md#a","description":"s2-m.1 から"},
            {"id":"s2-c.2","status":"closed","issue_type":"task","acceptance_criteria":"design = docs/design/toy.md#b","dependencies":[{"issue_id":"s2-c.2","depends_on_id":"s2-m.2","type":"discovered-from"}]}
        ]"####;
        let issues = vessel::seat::ledger::issues_of(json).unwrap_or_default();
        assert_eq!(issues.len(), 7, "fixture を読める");
        let row = |id: &str| Row { doc: "docs/design/toy.md".to_owned(), id: id.to_owned(), plus: vec![format!("src/{id}.rs")] };
        let docs = Docs { rows: vec![row("a"), row("b")], ..Docs::default() };
        let mut lines = render_doctor();
        lines.push(render_host_template(&Template::Absent));
        lines.push(render_init(&Readiness::Gaps(vec![Gap::HostFace, Gap::Registration]), &Template::Absent));
        lines.push(render(&judge(&issues, &docs)));
        lines.push(render_unreadable("ledger-unreadable"));
        lines.push(render_usage());
        lines.push(render_version());
        let masked = lines
            .join("\n")
            .replace(env!("CARGO_PKG_VERSION"), "[version]")
            .replace(&format!("({})", env!("SCRIBE2_BUILD_COMMIT")), "([commit])");
        insta::assert_snapshot!(masked);
    }

    /// doctor の台帳 lint の 1 行（contract-source.md §6 行 e）の外形: doctor の 2 行 → 雛形の pointer の行（unreadable の形）→ `init=` の行（no-repo の形）→ 測れた周の 1 行（3 つの欠陥の
    /// 件数を違え・id を欠陥ごとに名指す）→ 欠陥 0 の周の 1 行 → 測れない周の 1 行 → usage → version。台帳は
    /// `bd list --json` の形の fixture を席の reader（`issues_of`）で読み、解けた pointer の集合は直に組む。
    #[test]
    fn ledger_lint_doctor_external_form() {
        use std::collections::BTreeSet;
        use vessel::ledger::lint::{judge, render, render_unreadable};
        let json = r####"[
            {"id":"s2-u.1","status":"open","acceptance_criteria":"design = docs/design/toy.md#z"},
            {"id":"s2-b.1","status":"open","acceptance_criteria":"design = docs/design/toy.md#a\n本文"},
            {"id":"s2-b.2","status":"open","acceptance_criteria":"design = docs/design/toy.md#a\n本文"},
            {"id":"s2-r.1","status":"open","acceptance_criteria":"design = docs/design/toy.md#a"},
            {"id":"s2-m.1","status":"open","labels":["intake:memo"],"description":"## memo\n本文"},
            {"id":"s2-m.2","status":"open","labels":["intake:memo"],"description":"## memo\n本文"},
            {"id":"s2-m.3","status":"open","labels":["intake:memo"],"description":"## memo\n本文"},
            {"id":"s2-m.4","status":"open","labels":["intake:memo"],"description":"## memo\ndesign = docs/design/toy.md#a"},
            {"id":"s2-c.1","status":"closed","acceptance_criteria":"design = docs/design/toy.md#z"}
        ]"####;
        let issues = vessel::seat::ledger::issues_of(json).unwrap_or_default();
        assert_eq!(issues.len(), 9, "fixture を読める");
        let resolved: BTreeSet<String> = ["docs/design/toy.md#a".to_owned()].into();
        let mut lines = render_doctor();
        lines.push(render_host_template(&Template::Unreadable));
        lines.push(render_init(&Readiness::NoRepo, &Template::Unreadable));
        lines.push(render(&judge(&issues, &resolved)));
        lines.push(render(&judge(issues.get(3..4).unwrap_or_default(), &resolved)));
        lines.push(render_unreadable("ledger-unreadable"));
        lines.push(render_usage());
        lines.push(render_version());
        let masked = lines
            .join("\n")
            .replace(env!("CARGO_PKG_VERSION"), "[version]")
            .replace(&format!("({})", env!("SCRIBE2_BUILD_COMMIT")), "([commit])");
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
